//! The browser's side of DiaCraft's files: choosing a file (a file input, or the File System Access API where the browser has it), taking a file dropped on the page, handing a saved file over (a
//! download, or a write to the file the person chose). It exists on `wasm32` only, and is the one place besides `web.rs` that touches the DOM. `files.rs` holds what does not need a browser.
//!
//! **Everything stays on the reader's computer.** A file is read from the browser's `File` into the page's memory in pieces ([`files::chunks`]); nothing is sent anywhere, and the page makes no request. A file is
//! judged from its **size** before a byte is read ([`diacraft_engine::memfs::admit`]: the rule `file.open` applies), so one the engine would refuse costs nothing, and the memory for the bytes is asked for
//! with `try_reserve`, so one that cannot be had is told in words and not an abort.
//!
//! The window sees all this as a [`FileHost`]: a path in the engine's memory is what a choice answers with, and what a command wrote there is what is handed over. WebAssembly aborts on a panic, so nothing
//! here may panic: every browser call that can fail is matched and its failure is words for the person.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};

use diacraft_engine::memfs::{self, Keep};
use diacraft_ui_egui::{Delivered, FileHost, Want, place};
use js_sys::{Array, Function, Object, Promise, Reflect, Uint8Array};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast as _, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{Blob, BlobPropertyBag, DragEvent, File, HtmlAnchorElement, HtmlInputElement, Url, Window};

use crate::files::{self, MAX_DROPPED, MAX_HANDLES};

/// How long after the window gets the keyboard back a file input that has said nothing is taken to have been cancelled: a browser that sends no `cancel` event says nothing when the chooser is closed.
const CANCEL_GRACE_MS: i32 = 1500;

/// How long after a `cancel` event the input is looked at again, for a file that came after it (see `choose_with_input`).
const CANCEL_SETTLE_MS: i32 = 400;

/// How long a download's address lives: the browser has fetched the file long before.
const DOWNLOAD_ADDRESS_MS: i32 = 60_000;

/// What the page keeps between one frame and the next.
#[derive(Default)]
struct State {
    /// Files dropped on the page, read and put in the engine's memory, and not yet taken by the window.
    dropped: Vec<PathBuf>,
    /// What could not be taken, in words, and not yet said.
    refusals: Vec<String>,
    /// The drawing that was chosen or dropped last and has not been opened: the next one lets it go (one file waits for the question about unsaved changes).
    waiting: Option<PathBuf>,
    /// The handles of the files the person chose with the File System Access API, newest last: where a save writes in place.
    handles: Vec<(PathBuf, JsValue)>,
    /// The window's context, to ask for a frame when a file has arrived.
    ctx: Option<egui::Context>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// Runs `f` on the page's state. `f` never calls anything that reaches the state again.
fn with<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    // A state that is in use (a call inside a call) is left alone and the work is dropped: the page has one thread, so this is a bug that must not abort.
    STATE.with(|state| state.try_borrow_mut().ok().map(|mut state| f(&mut state)))
}

/// Says something to the person, once.
fn refuse(words: String) {
    log::warn!("diacraft-web: {words}");
    let _ = with(|s| {
        if s.refusals.len() < 8 {
            s.refusals.push(words);
        }
    });
}

fn wake() {
    if let Some(Some(ctx)) = with(|s| s.ctx.clone()) {
        ctx.request_repaint();
    }
}

/// The browser's words for what went wrong.
pub(crate) fn describe(value: &JsValue) -> String {
    if let Some(error) = value.dyn_ref::<js_sys::Error>() {
        return String::from(error.message());
    }
    value.as_string().unwrap_or_else(|| "the browser gave no reason".to_owned())
}

/// Whether `value` is a DOM exception called `name`: the browser's `AbortError` is what a person closing its chooser makes.
fn is_named(value: &JsValue, name: &str) -> bool {
    Reflect::get(value, &JsValue::from_str("name")).ok().and_then(|n| n.as_string()).is_some_and(|n| n == name)
}

fn window() -> Option<Window> {
    web_sys::window()
}

/// Whether the browser has the function `name` on its window (the File System Access API's choosers).
fn has_window_function(name: &str) -> bool {
    window().is_some_and(|w| Reflect::get(&w, &JsValue::from_str(name)).is_ok_and(|f| f.is_function()))
}

/// Calls the method `name` of `target` with `args`.
pub(crate) fn call(target: &JsValue, name: &str, args: &[JsValue]) -> Result<JsValue, JsValue> {
    let function: Function =
        Reflect::get(target, &JsValue::from_str(name))?.dyn_into().map_err(|_| JsValue::from_str("the browser has no such function"))?;
    let list = Array::new();
    for arg in args {
        list.push(arg);
    }
    function.apply(target, &list)
}

/// Calls the method `name` of `target` and waits for the promise it returns.
pub(crate) async fn call_async(target: &JsValue, name: &str, args: &[JsValue]) -> Result<JsValue, JsValue> {
    let promise: Promise = call(target, name, args)?.dyn_into().map_err(|_| JsValue::from_str("the browser did not answer with a promise"))?;
    JsFuture::from(promise).await
}

fn set(object: &Object, key: &str, value: &JsValue) {
    // `Err` only for an object that cannot be written to; these are plain objects made here.
    let _ = Reflect::set(object, &JsValue::from_str(key), value);
}

/// The `types` of a File System Access chooser: one entry for the extensions given, listed under the type of the first.
fn types_entry(description: &str, extensions: &[&str]) -> Option<Object> {
    let listed = files::listable(extensions);
    let first = *extensions.first()?;
    if listed.is_empty() {
        return None;
    }
    let accept = Object::new();
    let list = Array::new();
    for extension in &listed {
        list.push(&JsValue::from_str(extension));
    }
    set(&accept, files::mime_of(&format!("x.{first}")), &list);
    let entry = Object::new();
    set(&entry, "description", &JsValue::from_str(description));
    set(&entry, "accept", &accept);
    Some(entry)
}

/// Reads `file` into the engine's memory under a name of its own ([`place`]) and answers the path, or the words for why not.
///
/// The size is judged first, from `File.size`, and the room for the bytes is asked for before the first piece is read: a file the engine would refuse, or the page has no room for, is told in words and
/// nothing is read.
async fn take_file(file: File, keep: Keep) -> Result<PathBuf, String> {
    let name = file.name();
    let size = files::size_of(file.size()).ok_or_else(|| files::unreadable(&name))?;
    if let Err(problem) = memfs::admit(size) {
        return Err(files::refused(&name, problem));
    }
    let wanted = usize::try_from(size).map_err(|_| files::no_room(&name))?;
    let mut bytes: Vec<u8> = Vec::new();
    bytes.try_reserve_exact(wanted).map_err(|_| files::no_room(&name))?;
    for (start, end) in files::chunks(size) {
        let piece = file.slice_with_f64_and_f64(start as f64, end as f64).map_err(|_| files::unreadable(&name))?;
        let buffer = JsFuture::from(piece.array_buffer()).await.map_err(|_| files::unreadable(&name))?;
        let array = Uint8Array::new(&buffer);
        let length = usize::try_from(array.length()).map_err(|_| files::unreadable(&name))?;
        // A file that changed while it was read has another size than it had: it is not taken (what it holds now is not what was judged).
        if length as u64 != end - start {
            return Err(files::unreadable(&name));
        }
        let at = bytes.len();
        let Some(to) = at.checked_add(length) else { return Err(files::unreadable(&name)) };
        // Within what was reserved: no allocation.
        bytes.resize(to, 0);
        let Some(room) = bytes.get_mut(at..to) else { return Err(files::unreadable(&name)) };
        array.copy_to(room);
    }
    let path = place(&name);
    memfs::put(&path, bytes, keep).map_err(|_| files::no_room(&name))?;
    Ok(path)
}

/// What `want` keeps its file as: a stencil stays while it is open (the window opens it again in each drawing it reads), the others are read once.
fn keep_for(want: Want) -> Keep {
    if want == Want::Stencil { Keep::Held } else { Keep::Once }
}

/// Lets go of the drawing that was waiting and says `path` is the one that waits now. A file that is not a drawing is not waited for.
fn note_chosen(path: &Path, want: Want) {
    if want != Want::Drawing {
        return;
    }
    let old = with(|s| s.waiting.replace(path.to_path_buf())).flatten();
    if let Some(old) = old
        && old != path
    {
        memfs::remove(&old);
    }
}

/// Remembers the handle of the file at `path`, for writing it in place. The oldest is let go past [`MAX_HANDLES`].
fn keep_handle(path: &Path, handle: JsValue) {
    let _ = with(|s| {
        s.handles.retain(|(known, _)| known != path);
        s.handles.push((path.to_path_buf(), handle));
        while s.handles.len() > MAX_HANDLES {
            s.handles.remove(0);
        }
    });
}

fn handle_of(path: &Path) -> Option<JsValue> {
    with(|s| s.handles.iter().rev().find(|(known, _)| known == path).map(|(_, handle)| handle.clone())).flatten()
}

/// A file chosen: what the chooser gave.
struct Chosen {
    file: File,
    /// The handle to write the file in place, where the browser has the File System Access API.
    handle: Option<JsValue>,
}

/// The File System Access API's chooser for a file to open. `Ok(None)`: the person closed it.
async fn choose_with_api(want: Want) -> Result<Option<Chosen>, JsValue> {
    let window: JsValue = window().ok_or_else(|| JsValue::from_str("the page has no window"))?.into();
    let options = Object::new();
    set(&options, "multiple", &JsValue::FALSE);
    let types = Array::new();
    if let Some(entry) = types_entry(files::description(want), files::extensions(want)) {
        types.push(&entry);
    }
    set(&options, "types", &types);
    let handles = match call_async(&window, "showOpenFilePicker", &[options.into()]).await {
        Ok(handles) => handles,
        Err(e) if is_named(&e, "AbortError") => return Ok(None),
        Err(e) => return Err(e),
    };
    let handle = Array::from(&handles).get(0);
    if handle.is_undefined() {
        return Ok(None);
    }
    let file: File = call_async(&handle, "getFile", &[]).await?.dyn_into().map_err(|_| JsValue::from_str("the browser gave no file"))?;
    Ok(Some(Chosen { file, handle: Some(handle) }))
}

/// A file input, for a browser without the File System Access API (or for a person who dropped nothing): made, clicked and removed. `Ok(None)`: the person closed the chooser.
async fn choose_with_input(want: Want) -> Result<Option<Chosen>, JsValue> {
    let window = window().ok_or_else(|| JsValue::from_str("the page has no window"))?;
    let document = window.document().ok_or_else(|| JsValue::from_str("the page has no document"))?;
    let body = document.body().ok_or_else(|| JsValue::from_str("the page has no body"))?;
    let input: HtmlInputElement = document.create_element("input")?.dyn_into().map_err(|_| JsValue::from_str("the browser made no input"))?;
    input.set_type("file");
    input.set_accept(&files::input_accept(want));
    input.set_multiple(false);
    // Hidden by the attribute, never by an inline style: the page's content security policy has no inline styles.
    input.set_hidden(true);
    body.append_child(&input)?;
    let answered = {
        let input = input.clone();
        let window = window.clone();
        Promise::new(&mut |resolve: Function, _reject: Function| {
            let on_change = {
                let (input, resolve) = (input.clone(), resolve.clone());
                Closure::once_into_js(move || {
                    let file = input.files().and_then(|list| list.get(0)).map_or(JsValue::NULL, JsValue::from);
                    // `Err` only if the promise was settled already, which is no matter.
                    let _ = resolve.call1(&JsValue::NULL, &file);
                })
            };
            // What the input holds a little later settles the promise: a `cancel` is not taken as the last word at once, because a browser that is driven by a program (WebDriver) reports the
            // chooser as cancelled and then gives the file it was told to choose, and the file then comes as `change`; and a browser that has no `cancel` event says nothing when the chooser is
            // closed, so the window getting the keyboard back is waited on in the same way, and if no file came a moment later none will.
            let settle_in = {
                let (input, resolve, window) = (input.clone(), resolve.clone(), window.clone());
                move |milliseconds: i32| {
                    let late = {
                        let (input, resolve) = (input.clone(), resolve.clone());
                        Closure::once_into_js(move || {
                            let file = input.files().and_then(|list| list.get(0)).map_or(JsValue::NULL, JsValue::from);
                            let _ = resolve.call1(&JsValue::NULL, &file);
                        })
                    };
                    let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(late.unchecked_ref(), milliseconds);
                }
            };
            let on_cancel = {
                let settle_in = settle_in.clone();
                Closure::once_into_js(move || settle_in(CANCEL_SETTLE_MS))
            };
            let on_focus = Closure::once_into_js(move || settle_in(CANCEL_GRACE_MS));
            let _ = input.add_event_listener_with_callback("change", on_change.unchecked_ref());
            let _ = input.add_event_listener_with_callback("cancel", on_cancel.unchecked_ref());
            let _ = window.add_event_listener_with_callback("focus", on_focus.unchecked_ref());
        })
    };
    input.click();
    let outcome = JsFuture::from(answered).await;
    input.remove();
    let chosen = outcome?;
    if chosen.is_null() || chosen.is_undefined() {
        return Ok(None);
    }
    let file: File = chosen.dyn_into().map_err(|_| JsValue::from_str("the browser gave no file"))?;
    Ok(Some(Chosen { file, handle: None }))
}

/// Chooses a file for `want`, and takes it into the engine's memory. `Ok(None)`: nothing was chosen. An `Err` is words for the person.
async fn choose_and_take(want: Want) -> Result<Option<PathBuf>, String> {
    let chosen = if has_window_function("showOpenFilePicker") {
        choose_with_api(want).await.map_err(|e| files::no_chooser(&describe(&e)))?
    } else {
        choose_with_input(want).await.map_err(|e| files::no_chooser(&describe(&e)))?
    };
    let Some(Chosen { file, handle }) = chosen else { return Ok(None) };
    let path = take_file(file, keep_for(want)).await?;
    note_chosen(&path, want);
    if let Some(handle) = handle
        && want == Want::Drawing
    {
        keep_handle(&path, handle);
    }
    Ok(Some(path))
}

/// Starts the reading of the files of a drop, one after another, and puts each in the list the window takes from.
fn take_dropped(list: web_sys::FileList) {
    let mut dropped: Vec<File> = Vec::new();
    for n in 0..list.length().min(MAX_DROPPED as u32) {
        if let Some(file) = list.get(n) {
            dropped.push(file);
        }
    }
    let more = list.length().saturating_sub(MAX_DROPPED as u32);
    wasm_bindgen_futures::spawn_local(async move {
        if more > 0 {
            refuse(format!("More than {MAX_DROPPED} files were dropped: the first {MAX_DROPPED} were looked at and {more} were not."));
        }
        for file in dropped {
            match take_file(file, Keep::Once).await {
                Ok(path) => {
                    note_chosen(&path, Want::Drawing);
                    let _ = with(|s| s.dropped.push(path));
                }
                Err(words) => refuse(words),
            }
            wake();
        }
    });
}

/// Whether a drag carries files (and not text, or a link): only a drag of files is taken.
fn carries_files(event: &DragEvent) -> bool {
    event.data_transfer().is_some_and(|transfer| transfer.types().iter().any(|t| t.as_string().is_some_and(|t| t == "Files")))
}

/// Makes the page take files dropped on it: from now on a drop opens the file, and **the browser's own handling of the drop (which would read the file whole into the page with no look at its size, and
/// would open it in the tab if it were left alone) never sees it**: the handlers are on the window, in the capture phase, so they run first and stop the event.
pub fn install(ctx: &egui::Context) {
    let _ = with(|s| s.ctx = Some(ctx.clone()));
    let Some(window) = window() else { return };
    let over = Closure::<dyn FnMut(DragEvent)>::new(move |event: DragEvent| {
        if carries_files(&event) {
            // Without this the browser refuses the drop.
            event.prevent_default();
            event.stop_immediate_propagation();
        }
    });
    let drop = Closure::<dyn FnMut(DragEvent)>::new(move |event: DragEvent| {
        if !carries_files(&event) {
            return;
        }
        event.prevent_default();
        event.stop_immediate_propagation();
        if let Some(list) = event.data_transfer().and_then(|transfer| transfer.files()) {
            take_dropped(list);
        }
    });
    // Both stay as long as the page does.
    for (kind, handler) in [("dragenter", &over), ("dragover", &over), ("drop", &drop)] {
        if let Err(e) = window.add_event_listener_with_callback_and_bool(kind, handler.as_ref().unchecked_ref(), true) {
            log::error!("diacraft-web: could not take {kind} events: {}", describe(&e));
        }
    }
    over.forget();
    drop.forget();
}

/// Writes `bytes` to the file `handle` stands for, in place.
async fn write_in_place(handle: &JsValue, bytes: &[u8]) -> Result<(), JsValue> {
    let writable = call_async(handle, "createWritable", &[]).await?;
    let data = Uint8Array::from(bytes);
    let written = call_async(&writable, "write", &[data.into()]).await;
    // The stream is closed whether or not the write worked: an open one holds the file.
    let closed = call_async(&writable, "close", &[]).await;
    written?;
    closed.map(|_| ())
}

/// Downloads `bytes` as a file called `name`: a Blob, an address for it, and a click on a link that asks to save it. The page makes no request: the address is the browser's own, of memory it holds.
fn download(name: &str, bytes: &[u8]) -> Result<(), String> {
    let window = window().ok_or("the page has no window")?;
    let document = window.document().ok_or("the page has no document")?;
    let body = document.body().ok_or("the page has no body")?;
    let data = Uint8Array::from(bytes);
    let parts = Array::new();
    parts.push(&data);
    let options = BlobPropertyBag::new();
    options.set_type(files::mime_of(name));
    let blob = Blob::new_with_u8_array_sequence_and_options(&parts, &options).map_err(|e| describe(&e))?;
    let address = Url::create_object_url_with_blob(&blob).map_err(|e| describe(&e))?;
    let anchor: HtmlAnchorElement =
        document.create_element("a").map_err(|e| describe(&e))?.dyn_into().map_err(|_| "the browser made no link".to_owned())?;
    anchor.set_href(&address);
    anchor.set_download(name);
    anchor.set_hidden(true);
    body.append_child(&anchor).map_err(|e| describe(&e))?;
    anchor.click();
    anchor.remove();
    // The address is let go once the browser has had the file.
    let revoke = {
        let address = address.clone();
        Closure::once_into_js(move || {
            // `Err` only for an address that is gone already.
            let _ = Url::revoke_object_url(&address);
        })
    };
    if window.set_timeout_with_callback_and_timeout_and_arguments_0(revoke.unchecked_ref(), DOWNLOAD_ADDRESS_MS).is_err() {
        // No timer: the address is let go now. The click has started the download, which holds its own reference to the data.
        let _ = Url::revoke_object_url(&address);
    }
    Ok(())
}

/// The browser's side of the window's files.
pub struct WebFiles;

fn send<T>(tx: &Sender<T>, value: T) {
    // The window may have been closed; there is nobody to answer.
    let _ = tx.send(value);
    wake();
}

impl FileHost for WebFiles {
    fn choose(&self, ctx: &egui::Context, want: Want) -> Receiver<Option<PathBuf>> {
        let (tx, rx) = channel();
        let _ = with(|s| s.ctx = Some(ctx.clone()));
        wasm_bindgen_futures::spawn_local(async move {
            let answer = match choose_and_take(want).await {
                Ok(path) => path,
                Err(words) => {
                    refuse(words);
                    None
                }
            };
            send(&tx, answer);
        });
        rx
    }

    fn names_saves(&self) -> bool {
        has_window_function("showSaveFilePicker")
    }

    fn choose_save(&self, ctx: &egui::Context, request: &diacraft_ui_egui::SaveRequest) -> Receiver<Option<PathBuf>> {
        let (tx, rx) = channel();
        let _ = with(|s| s.ctx = Some(ctx.clone()));
        let suggested = request.name.clone();
        let entries: Vec<(String, String)> = request.kinds.iter().map(|k| (k.label.to_owned(), k.extension.to_owned())).collect();
        wasm_bindgen_futures::spawn_local(async move {
            let answer = match choose_save_place(&suggested, &entries).await {
                Ok(path) => path,
                Err(words) => {
                    refuse(words);
                    None
                }
            };
            send(&tx, answer);
        });
        rx
    }

    fn deliver(&self, ctx: &egui::Context, path: &Path) -> Receiver<Result<Delivered, String>> {
        let (tx, rx) = channel();
        let _ = with(|s| s.ctx = Some(ctx.clone()));
        let Some(bytes) = memfs::take(path) else {
            send(&tx, Err("there was nothing to hand over.".to_owned()));
            return rx;
        };
        let name = files::download_name(path);
        let handle = handle_of(path);
        wasm_bindgen_futures::spawn_local(async move {
            let result = match handle {
                Some(handle) => match write_in_place(&handle, &bytes).await {
                    Ok(()) => Ok(Delivered::Wrote),
                    // The file could not be written (the person said no, the disk is full, the file is gone): the bytes are still here, and are downloaded instead.
                    Err(e) => download(&name, &bytes).map(|()| Delivered::Fallback(describe(&e))),
                },
                None => download(&name, &bytes).map(|()| Delivered::Downloaded),
            };
            send(&tx, result);
        });
        rx
    }

    fn dropped(&self) -> Vec<PathBuf> {
        with(|s| std::mem::take(&mut s.dropped)).unwrap_or_default()
    }

    fn refusals(&self) -> Vec<String> {
        with(|s| std::mem::take(&mut s.refusals)).unwrap_or_default()
    }

    fn release(&self, path: &Path) {
        memfs::remove(path);
        let _ = with(|s| {
            if s.waiting.as_deref() == Some(path) {
                s.waiting = None;
            }
        });
    }
}

/// The File System Access API's dialog for a file to save: the name to start with, and the kinds offered, the first selected. The answer is a path in the engine's memory with the name the person chose,
/// and the handle is kept to write there in place.
async fn choose_save_place(suggested: &str, kinds: &[(String, String)]) -> Result<Option<PathBuf>, String> {
    let window: JsValue = window().ok_or_else(|| files::no_chooser("the page has no window"))?.into();
    let options = Object::new();
    set(&options, "suggestedName", &JsValue::from_str(suggested));
    let types = Array::new();
    for (label, extension) in kinds {
        if let Some(entry) = types_entry(label, &[extension.as_str()]) {
            types.push(&entry);
        }
    }
    if types.length() > 0 {
        set(&options, "types", &types);
    }
    let handle = match call_async(&window, "showSaveFilePicker", &[options.into()]).await {
        Ok(handle) => handle,
        Err(e) if is_named(&e, "AbortError") => return Ok(None),
        Err(e) => return Err(files::no_chooser(&describe(&e))),
    };
    let name = Reflect::get(&handle, &JsValue::from_str("name")).ok().and_then(|n| n.as_string()).unwrap_or_else(|| suggested.to_owned());
    let path = place(&name);
    keep_handle(&path, handle);
    Ok(Some(path))
}
