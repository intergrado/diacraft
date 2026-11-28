//! What the page and the program agree on, and what the page says when the app cannot start.
//!
//! Nothing here touches the browser, so it builds and is tested on every target.

use diacraft_engine::{Session, shown};
use diacraft_ui_egui::DiacraftApp;

/// The id of the `<canvas>` in `index.html` that the app draws on.
pub const CANVAS_ID: &str = "diacraft_canvas";

/// The id of the element in `index.html` that says `Loading DiaCraft…` and then, if the app cannot start, why.
pub const LOADING_ID: &str = "diacraft_loading";

/// The longest reason, in characters, a start failure shows. A browser's error text can be long, and it is not
/// ours to put in front of a person whole.
const MAX_REASON_CHARS: usize = 300;

/// The window the page runs: an empty session and no control channel (a browser cannot listen on a socket).
///
/// It is `DiacraftApp` as the desktop runs it, so the fonts (Hack and Ubuntu Light, installed on its first pass)
/// and the theme are the desktop's. The shell never installs a font of its own. **It starts with the splash popup**, with the same git release and year as
/// the desktop app's: this crate's build script says them (`build.rs`), and the engine answers them in `app.version` too.
pub fn new_app() -> DiacraftApp {
    diacraft_engine::build_info::set(diacraft_engine::build_info::BuildInfo::new(env!("DIACRAFT_BUILD_GIT"), env!("DIACRAFT_BUILD_YEAR")));
    DiacraftApp::new(Session::new(), None).with_start_splash()
}

/// [`new_app`] with a host for files (`FileHost`): the browser's own way of choosing a file, taking one dropped on the page and handing one over, so that the window opens, saves, exports and
/// prints as the desktop's does. The page makes this one in the browser; the tests make theirs.
pub fn new_app_with_files(host: std::sync::Arc<dyn diacraft_ui_egui::FileHost>) -> DiacraftApp {
    new_app().with_file_host(host)
}

/// [`new_app_with_files`] with a host for the assets the module does not carry (`AssetHost`: the splash picture, the cover, the licence list), which the page fetches, and a host for the window's place
/// in the page (`PageHost`: a window of its own, the keys the browser keeps, full screen). The page makes this one in the browser, and the tests make theirs.
pub fn new_app_in_browser(
    files: std::sync::Arc<dyn diacraft_ui_egui::FileHost>,
    assets: std::sync::Arc<dyn diacraft_ui_egui::AssetHost>,
    page: std::sync::Arc<dyn diacraft_ui_egui::PageHost>,
) -> DiacraftApp {
    new_app_with_files(files).with_asset_host(assets).with_page_host(page)
}

/// `app` with the person's settings opened in `host`, which in the browser is the page's `localStorage` (`prefs::StorageHost`): the window saves what changes. The system's
/// language, as the browser's languages say it, is the first editing language of a profile that has none. What is wrong with the settings (a text that is broken, too big or from a newer build,
/// a storage that is full or forbidden) is logged and is the status bar's notice, once. The page makes this one in the browser, and the tests make theirs with a host of their own.
pub fn with_prefs_in_browser(app: DiacraftApp, host: std::sync::Arc<dyn diacraft_ui_egui::prefs_host::PrefsHost>) -> DiacraftApp {
    use diacraft_ui_egui::prefs_host::{LocaleSource, Profile, WindowPrefs};
    let (profile, report) = Profile::<WindowPrefs>::open_host(host, LocaleSource::System);
    for note in &report.notes {
        log::warn!("diacraft-web: settings: {note}");
    }
    let app = app.with_prefs(profile);
    match report.sentence() {
        Some(sentence) => app.with_notice(sentence),
        None => app,
    }
}

/// The lines shown when the page has no canvas to draw on.
pub fn missing_canvas() -> Vec<String> {
    vec![
        format!("DiaCraft could not start: this page has no <canvas id=\"{CANVAS_ID}\">."),
        "Reload the page. If it happens again, please report it as a bug.".to_owned(),
    ]
}

/// The lines shown when the browser could not start the app, with `reason` as the browser gave it (escaped, and
/// cut short).
pub fn start_failed(reason: &str) -> Vec<String> {
    vec![format!("DiaCraft could not start: {}", shown(reason, MAX_REASON_CHARS)), "A browser with WebGPU or WebGL2 is needed.".to_owned()]
}

/// What the browser's side must do to its handler for `beforeunload` after the window said something ([`LeaveGuard`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuardStep {
    /// Put the handler on the page: the browser asks before the page is left.
    Attach,
    /// Take the handler off the page.
    Detach,
    /// Leave it as it is.
    Keep,
}

/// Whether the browser asks before the page is closed, reloaded or left, as the page's side decides it. The handler is on the page only while the window says the drawing has unsaved
/// changes **and** the window is not being closed: a page with nothing to lose asks nothing and stays eligible for the browser's back/forward cache, and the window's own close (Ctrl+W, after
/// DiaCraft's question) must not meet a second question from the browser. When the browser keeps the window after all, the handler comes back if the drawing still has changes.
///
/// It decides and the browser's side does: each call answers what to do to the handler now.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LeaveGuard {
    /// The window said the drawing has unsaved changes.
    wanted: bool,
    /// The page asked the browser to close the window and does not yet know whether it was kept.
    closing: bool,
    /// The handler is on the page.
    attached: bool,
}

impl LeaveGuard {
    /// The handler is on the page now.
    pub fn attached(&self) -> bool {
        self.attached
    }

    /// What to do to the handler so that it is on the page exactly when the window wants it and the window is not closing.
    fn settle(&mut self) -> GuardStep {
        let should = self.wanted && !self.closing;
        if should == self.attached {
            return GuardStep::Keep;
        }
        self.attached = should;
        if should { GuardStep::Attach } else { GuardStep::Detach }
    }

    /// The window says whether the drawing has unsaved changes (`PageHost::guard_leaving`).
    pub fn want(&mut self, on: bool) -> GuardStep {
        self.wanted = on;
        self.settle()
    }

    /// The page is about to ask the browser to close the window (`PageHost::close`): the handler goes first, and stays away until [`LeaveGuard::kept`].
    pub fn closing(&mut self) -> GuardStep {
        self.closing = true;
        self.settle()
    }

    /// The browser kept the window after all (it refused the close, or a moment passed and the page is still here): the handler is on again if the drawing still has changes.
    pub fn kept(&mut self) -> GuardStep {
        self.closing = false;
        self.settle()
    }
}
