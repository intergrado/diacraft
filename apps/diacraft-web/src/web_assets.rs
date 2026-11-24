//! The browser's side of the window's assets: fetching the splash picture, the cover and the licence list from the hashed files of the site beside the app. It exists on `wasm32` only, and
//! is, with `web.rs` and `web_files.rs`, a place that touches the browser. `assets.rs` holds what does not need a browser (reading the manifest).
//!
//! **Same origin, and nothing else.** The files are named by the manifest the page's loader handed over (`diacraft_manifest`, set by `start.js` from `app/manifest.json`), each by a plain name under
//! `assets/` ([`assets::is_asset_file`]) that is fetched relative to the page, so the page's content security policy keeps `connect-src 'self'`. **What is read is bounded and checked.** The size the
//! manifest gives is at most the window's limit for that asset, the room for it is asked for with `try_reserve_exact` before the first byte is read, the body is read in the pieces the browser gives
//! it and stops at that size (one more byte is a failure, and the rest is cancelled), and the whole is held to its SHA-256 where the browser has `crypto.subtle` (every page served over https, and
//! `localhost`; elsewhere the length alone is checked, and the console says so).
//!
//! A failure of any kind is the asset's: it is [`Fetch::Failed`] with words, and the window shows them where the asset would be and works without it. Nothing here may panic (WebAssembly aborts
//! on a panic): every browser call that can fail is matched.

use std::cell::RefCell;
use std::sync::Arc;

use diacraft_ui_egui::{Asset, AssetHost, Bytes, Fetch};
use js_sys::{Array, Reflect, Uint8Array};
use wasm_bindgen::{JsCast as _, JsValue};
use wasm_bindgen_futures::JsFuture;

use crate::assets::{self, Entry, Table};
use crate::web_files::{call, call_async, describe};

/// The property of the window the loader (`start.js`) puts the manifest's text in.
const MANIFEST_PROPERTY: &str = "diacraftManifest";

/// Where one asset stands in the page.
enum Slot {
    /// Nobody has asked.
    Idle,
    /// The fetch is under way.
    Fetching,
    /// Here: the bytes, until the window has taken what it needs.
    Ready(Arc<[u8]>),
    /// It could not be had.
    Failed(String),
}

struct State {
    slots: [(Asset, Slot); 3],
    /// What the manifest says, read the first time an asset is asked for.
    table: Option<Table>,
    /// The window's context, to wake it when an asset has come: known from the first time the window asks for one.
    ctx: Option<egui::Context>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State { slots: Asset::ALL.map(|asset| (asset, Slot::Idle)), table: None, ctx: None });
}

/// Runs `f` on the page's state. `f` never calls anything that reaches the state again.
fn with<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    // A state that is in use (a call inside a call) is left alone and the work is dropped: the page has one thread, so this is a bug that must not abort.
    STATE.with(|state| state.try_borrow_mut().ok().map(|mut state| f(&mut state)))
}

fn set_slot(asset: Asset, slot: Slot) {
    let _ = with(|s| {
        if let Some((_, place)) = s.slots.iter_mut().find(|(a, _)| *a == asset) {
            *place = slot;
        }
    });
}

/// The host the window is given in the browser.
pub struct WebAssets;

/// Starts fetching `asset` if nobody has: **before the window exists** for the splash picture (`web.rs` calls it as the page starts the program), so that the picture is on its way while the
/// graphics start and is there, or near, when the window draws its first frame. Later calls, and the window's own (`want`), change nothing but what the window is woken with.
pub fn begin(asset: Asset) {
    let idle = with(|s| s.slots.iter().any(|(a, slot)| *a == asset && matches!(slot, Slot::Idle))).unwrap_or(false);
    if !idle {
        return;
    }
    set_slot(asset, Slot::Fetching);
    wasm_bindgen_futures::spawn_local(async move {
        let slot = match entry(asset) {
            Ok(entry) => match fetch(asset, &entry).await {
                Ok(bytes) => Slot::Ready(bytes),
                Err(words) => Slot::Failed(words),
            },
            Err(words) => Slot::Failed(words),
        };
        if let Slot::Failed(words) = &slot {
            log::warn!("diacraft-web: {words}");
        }
        set_slot(asset, slot);
        // The window draws what it was waiting for (when it has asked for anything, it is known).
        if let Some(Some(ctx)) = with(|s| s.ctx.clone()) {
            ctx.request_repaint();
        }
    });
}

impl AssetHost for WebAssets {
    fn want(&self, ctx: &egui::Context, asset: Asset) {
        let _ = with(|s| {
            if s.ctx.is_none() {
                s.ctx = Some(ctx.clone());
            }
        });
        begin(asset);
    }

    fn state(&self, asset: Asset) -> Fetch {
        with(|s| {
            s.slots.iter().find(|(a, _)| *a == asset).map_or(Fetch::Waiting, |(_, slot)| match slot {
                Slot::Idle | Slot::Fetching => Fetch::Waiting,
                Slot::Ready(bytes) => Fetch::Ready(Bytes::Shared(bytes.clone())),
                Slot::Failed(words) => Fetch::Failed(words.clone()),
            })
        })
        .unwrap_or(Fetch::Waiting)
    }

    fn release(&self, asset: Asset) {
        // The window has what it needs (a texture, the rows): the bytes go. A later `want` finds the slot `Idle` and fetches again, from the browser's cache.
        set_slot(asset, Slot::Idle);
    }
}

/// What the manifest says of `asset`, or why it says nothing.
fn entry(asset: Asset) -> Result<Entry, String> {
    let known = with(|s| s.table.is_some()).unwrap_or(false);
    if !known {
        let table = assets::parse(&manifest_text()?);
        let _ = with(|s| s.table = Some(table));
    }
    with(|s| s.table.as_ref().and_then(|table| table.iter().find(|(a, _)| *a == asset).map(|(_, entry)| entry.clone())))
        .flatten()
        .unwrap_or_else(|| Err(format!("the page's manifest could not be read for {}", asset.words())))
}

/// The manifest's text, as the loader left it on the window.
fn manifest_text() -> Result<String, String> {
    let window = web_sys::window().ok_or("the page has no window")?;
    // Its length is looked at before a byte of it is copied: the loader keeps it under the bound, and the program does not take that on trust.
    let text = Reflect::get(&window, &JsValue::from_str(MANIFEST_PROPERTY))
        .ok()
        .and_then(|v| v.dyn_into::<js_sys::JsString>().ok())
        .filter(|v| usize::try_from(v.length()).is_ok_and(|n| n <= assets::MAX_MANIFEST_BYTES))
        .and_then(|v| v.as_string());
    text.ok_or_else(|| {
        "the page was not started by the site's loader (it left no manifest), so the files of the app cannot be found; build the site with `cargo xtask site --web`".to_owned()
    })
}

/// Fetches the file `entry` names, bounded and checked.
async fn fetch(asset: Asset, entry: &Entry) -> Result<Arc<[u8]>, String> {
    let window = web_sys::window().ok_or("the page has no window")?;
    // Relative to the page: the app's own folder. The name has been judged ([`assets::is_asset_file`]).
    let url = format!("./{}", entry.file);
    let response: web_sys::Response = JsFuture::from(window.fetch_with_str(&url))
        .await
        .map_err(|e| format!("{} could not be fetched ({})", asset.words(), describe(&e)))?
        .dyn_into()
        .map_err(|_| format!("{} could not be fetched (the browser gave no response)", asset.words()))?;
    if !response.ok() {
        return Err(format!("{} could not be fetched (the server answered {})", asset.words(), response.status()));
    }
    let body = response.body().ok_or_else(|| format!("{} could not be fetched (the response has no body)", asset.words()))?;
    let reader = call(body.as_ref(), "getReader", &[]).map_err(|e| format!("{} could not be read ({})", asset.words(), describe(&e)))?;
    let mut bytes: Vec<u8> = Vec::new();
    bytes.try_reserve_exact(entry.bytes).map_err(|_| format!("there is no room in the page for {}", asset.words()))?;
    loop {
        let step = call_async(&reader, "read", &[]).await.map_err(|e| format!("{} could not be read ({})", asset.words(), describe(&e)))?;
        let done = Reflect::get(&step, &JsValue::from_str("done")).ok().and_then(|d| d.as_bool()).unwrap_or(true);
        if done {
            break;
        }
        let piece: Uint8Array = Reflect::get(&step, &JsValue::from_str("value"))
            .ok()
            .and_then(|v| v.dyn_into().ok())
            .ok_or_else(|| format!("{} could not be read (the browser gave a piece that is not bytes)", asset.words()))?;
        let length = usize::try_from(piece.length()).map_err(|_| format!("{} could not be read", asset.words()))?;
        let at = bytes.len();
        let to = at.checked_add(length).filter(|to| *to <= entry.bytes);
        let Some(to) = to else {
            // More than the manifest says: the rest is not wanted. `Err` only when the stream is closed already.
            let _ = call_async(&reader, "cancel", &[]).await;
            return Err(assets::wrong_length(asset, entry.bytes, at.saturating_add(length)));
        };
        // Within what was reserved: no allocation.
        bytes.resize(to, 0);
        let Some(room) = bytes.get_mut(at..to) else { return Err(format!("{} could not be read", asset.words())) };
        piece.copy_to(room);
    }
    if bytes.len() != entry.bytes {
        return Err(assets::wrong_length(asset, entry.bytes, bytes.len()));
    }
    match digest(&window, &bytes).await {
        Ok(Some(got)) if got != entry.sha256 => return Err(assets::wrong_digest(asset)),
        Ok(Some(_)) => {}
        Ok(None) => log::warn!(
            "diacraft-web: the browser has no crypto.subtle (the page is not served over https): {} was checked by its length only",
            asset.words()
        ),
        // The browser has the means and they failed: the download is not vouched for.
        Err(words) => return Err(format!("{} could not be checked ({words})", asset.words())),
    }
    Ok(Arc::from(bytes))
}

/// The SHA-256 of `bytes`, by the browser's own `crypto.subtle`. `Ok(None)` where the browser has none (a page that is not in a secure context); `Err` when it has and it fails.
async fn digest(window: &web_sys::Window, bytes: &[u8]) -> Result<Option<[u8; 32]>, String> {
    let present = |v: JsValue| if v.is_undefined() || v.is_null() { None } else { Some(v) };
    let Some(crypto) = Reflect::get(window, &JsValue::from_str("crypto")).ok().and_then(present) else { return Ok(None) };
    let Some(subtle) = Reflect::get(&crypto, &JsValue::from_str("subtle")).ok().and_then(present) else { return Ok(None) };
    let data = Uint8Array::from(bytes);
    let args = Array::new();
    args.push(&JsValue::from_str("SHA-256"));
    args.push(&data);
    let promise: js_sys::Promise = call(&subtle, "digest", &args.to_vec())
        .map_err(|e| describe(&e))?
        .dyn_into()
        .map_err(|_| "the browser did not answer with a promise".to_owned())?;
    let buffer = JsFuture::from(promise).await.map_err(|e| describe(&e))?;
    let array = Uint8Array::new(&buffer);
    if array.length() != 32 {
        return Err("the digest is not 32 bytes".to_owned());
    }
    let mut out = [0u8; 32];
    array.copy_to(&mut out);
    Ok(Some(out))
}
