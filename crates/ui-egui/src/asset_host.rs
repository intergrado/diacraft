//! Data the web build keeps outside its WebAssembly module.
//!
//! The desktop programs carry their artwork and the licence list inside (`include_bytes!` in `branding.rs` and `licenses.rs`). The web build does not: the module is what a person
//! waits for, and the cover (2.4 MB of PNG, which no compression makes smaller), the splash picture and the licence list are 2.8 MB of it, none of which the first frame needs. They are
//! **files of the site beside the app**, hashed, and the page fetches them **after the first frame, or when they are first needed** (the splash picture at the start, the licence list
//! when its window opens, the cover when the File view shows it).
//!
//! The window sees all this as an [`AssetHost`] (`DiacraftApp::with_asset_host`), the page's side of it, and **the rest of the window is the same code**: it asks for an [`Asset`],
//! and the answer is [`Fetch::Waiting`] (the window shows a placeholder or a short wait and is woken when the answer has come), [`Fetch::Ready`] or [`Fetch::Failed`] (the window says it in words
//! and works without it). A window with **no host** (the desktop) takes the asset from the program itself where the program carries it, and an asset it does not carry is a
//! failure in words, never a panic. The window knows nothing of the browser: the host is a trait, and the tests of this flow run on the desktop with a host of their own.

use std::sync::Arc;

use egui::Context;

use crate::app::DiacraftApp;

/// One piece of data the web build fetches instead of carrying.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Asset {
    /// The splash picture (`branding.rs`): the cover at half its size, in at most 256 colours. The first thing the window shows, so it is asked for at the start.
    Splash,
    /// The cover (`branding.rs`), at its full size: the File view's Home page shows it, so it is asked for when the File view first does.
    Cover,
    /// The third-party licence list (`licenses.rs`), as text: asked for when its window first opens.
    Licenses,
}

impl Asset {
    /// Every asset.
    pub const ALL: [Asset; 3] = [Asset::Splash, Asset::Cover, Asset::Licenses];

    /// The name the manifest of the web build gives it (`manifest.json` at the app's root, "assets").
    pub const fn key(self) -> &'static str {
        match self {
            Asset::Splash => "splash",
            Asset::Cover => "cover",
            Asset::Licenses => "licenses",
        }
    }

    /// The asset the manifest names `key`, if it names one.
    pub fn from_key(key: &str) -> Option<Asset> {
        Asset::ALL.into_iter().find(|asset| asset.key() == key)
    }

    /// The most bytes the window takes for it: a host holds the asset to this as it reads, and refuses one that says it is more. The files are far below it (the cover is the largest, at 2.4 MB);
    /// the limit is for a response that is not the file (an error page, or a stream that does not end).
    pub const fn limit(self) -> usize {
        match self {
            Asset::Splash => 1 << 20,
            Asset::Cover => 8 << 20,
            Asset::Licenses => 2 << 20,
        }
    }

    /// What it is called in the words the window says when it could not be had.
    pub const fn words(self) -> &'static str {
        match self {
            Asset::Splash => "the splash picture",
            Asset::Cover => "the cover picture",
            Asset::Licenses => "the list of third-party licences",
        }
    }
}

/// The bytes of an asset: the program's own (a `static`, not copied) or what a host fetched (shared, so that asking again copies nothing).
#[derive(Clone, Debug)]
pub enum Bytes {
    Static(&'static [u8]),
    Shared(Arc<[u8]>),
}

impl Bytes {
    /// The bytes.
    pub fn as_slice(&self) -> &[u8] {
        match self {
            Bytes::Static(bytes) => bytes,
            Bytes::Shared(bytes) => bytes,
        }
    }
}

impl PartialEq for Bytes {
    fn eq(&self, other: &Bytes) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl Eq for Bytes {}

/// Where an asset stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fetch {
    /// Not here yet: the host is getting it and wakes the window (`Context::request_repaint`) when it has.
    Waiting,
    /// Here.
    Ready(Bytes),
    /// It could not be had: the reason, in words for the person, which [`Asset::words`] and the window put in a sentence.
    Failed(String),
}

/// The page's side of the window's assets. Every call returns at once; the answer that needs the network comes later, and the host wakes the window when it has it.
pub trait AssetHost: Send + Sync {
    /// Asks for `asset`: the first call starts getting it, and later calls (every frame the window needs it) change nothing. The host keeps `ctx` to wake the window.
    fn want(&self, ctx: &Context, asset: Asset);

    /// Where `asset` stands. An asset nobody has asked for is [`Fetch::Waiting`].
    fn state(&self, asset: Asset) -> Fetch;

    /// The window has taken what it needs from `asset` (a picture decoded into a texture, a list broken into rows): the host may let the bytes go. A later [`AssetHost::want`] gets it again.
    fn release(&self, asset: Asset) {
        let _ = asset;
    }
}

/// Where the window gets its assets from: its host, or, with none, the program. A copy of the window's own, so that a window can ask for an asset while it holds another part of itself.
#[derive(Clone, Default)]
pub(crate) struct Source(Option<Arc<dyn AssetHost>>);

impl Source {
    /// Where `asset` stands: asked of the host when there is one (and asked again every frame the window needs the asset, which costs the host a look at a table), else the program's own
    /// copy where the program carries one.
    pub(crate) fn get(&self, ctx: &Context, asset: Asset) -> Fetch {
        match &self.0 {
            Some(host) => {
                host.want(ctx, asset);
                host.state(asset)
            }
            None => embedded(asset),
        }
    }

    /// Where `asset` stands **without asking for it**: the host's answer as it is (an asset nobody has asked for is waiting, and stays so), else the program's own copy. For a window that
    /// would use an asset it has, and that is no reason to fetch it (the splash on a dense screen and the cover, which the web build fetches when the File view first shows it).
    pub(crate) fn peek(&self, asset: Asset) -> Fetch {
        match &self.0 {
            Some(host) => host.state(asset),
            None => embedded(asset),
        }
    }

    /// The window has what it needs from `asset`: the host may let the bytes go.
    pub(crate) fn taken(&self, asset: Asset) {
        if let Some(host) = &self.0 {
            host.release(asset);
        }
    }
}

impl DiacraftApp {
    /// Gives the window the page's side of its assets ([`AssetHost`]). With one, the window gets its artwork and the licence list from the host and not from the program: the web build
    /// carries none of them.
    pub fn with_asset_host(mut self, host: Arc<dyn AssetHost>) -> DiacraftApp {
        self.assets = Source(Some(host));
        self
    }

    /// Whether the window has a host for its assets.
    pub fn has_asset_host(&self) -> bool {
        self.assets.0.is_some()
    }

    /// Where the window gets its assets from, as a copy.
    pub(crate) fn asset_source(&self) -> Source {
        self.assets.clone()
    }
}

/// What the program carries of `asset`. **The desktop programs carry all of them. The web build carries none** (`cfg(target_arch = "wasm32")`): a window with no host there says so.
#[cfg(not(target_arch = "wasm32"))]
fn embedded(asset: Asset) -> Fetch {
    match asset {
        Asset::Splash => Fetch::Ready(Bytes::Static(crate::branding::SPLASH)),
        Asset::Cover => Fetch::Ready(Bytes::Static(crate::branding::COVER)),
        Asset::Licenses => Fetch::Ready(Bytes::Static(crate::licenses::TEXT.as_bytes())),
    }
}

#[cfg(target_arch = "wasm32")]
fn embedded(asset: Asset) -> Fetch {
    Fetch::Failed(format!("this build does not carry {} and has no way to fetch it", asset.words()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_keys_name_the_assets_and_only_them() {
        for asset in Asset::ALL {
            assert_eq!(Asset::from_key(asset.key()), Some(asset));
        }
        assert_eq!(Asset::from_key("hack"), None);
        assert_eq!(Asset::from_key(""), None);
        assert_eq!(Asset::from_key("Splash"), None, "the manifest's keys are exact");
        let keys: Vec<_> = Asset::ALL.iter().map(|a| a.key()).collect();
        assert_eq!(keys, ["splash", "cover", "licenses"]);
    }

    #[test]
    fn each_limit_is_above_its_file_and_a_picture_limit_is_not_a_text_limit() {
        // The files of the repository, as a build puts them in the site.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        for (asset, file) in [
            (Asset::Splash, "assets/branding/diacraft-splash.png"),
            (Asset::Cover, "assets/branding/diacraft-cover.png"),
            (Asset::Licenses, "THIRD-PARTY-LICENSES-WEB.txt"),
        ] {
            let size = std::fs::metadata(root.join(file)).expect("the file is in the repository").len();
            assert!(size < asset.limit() as u64, "{file} is {size} bytes, over the {} bytes of its limit", asset.limit());
            assert!(size * 2 > asset.limit() as u64 / 8, "{file}: a limit more than sixteen times the file is no limit");
        }
    }

    #[test]
    fn bytes_compare_by_content_whatever_they_are_held_in() {
        let shared: Arc<[u8]> = Arc::from(&b"abc"[..]);
        assert_eq!(Bytes::Static(b"abc"), Bytes::Shared(shared.clone()));
        assert_ne!(Bytes::Static(b"abd"), Bytes::Shared(shared));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_window_with_no_host_has_every_asset_from_the_program() {
        let app = DiacraftApp::new(diacraft_engine::Session::new(), None);
        let ctx = Context::default();
        for asset in Asset::ALL {
            match app.asset_source().get(&ctx, asset) {
                Fetch::Ready(bytes) => assert!(!bytes.as_slice().is_empty(), "{asset:?}"),
                other => panic!("{asset:?}: {other:?}"),
            }
        }
        assert!(!app.has_asset_host());
    }
}
