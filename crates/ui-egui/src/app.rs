//! The DiaCraft window: a title bar, a tab row and ribbon, the Shapes window, the drawing area with its page tabs, and a
//! status bar; over them all the File view when it is open.
//!
//! The window reads the engine session and changes it only by running commands. Every frame runs under the
//! panic guard, so a bug that panics while drawing costs one frame and shows an error, not the app.

use std::cell::RefCell;
use std::time::Duration;

use diacraft_engine::{EngineError, Session, guard, shown};
use egui::{CursorIcon, Margin, Pos2, Rect, ResizeDirection, Role, Sense, Stroke, Ui, Vec2, ViewportCommand, WidgetInfo};
use serde_json::Value;

use crate::control::{ControlInbox, DEFAULT_SCREENSHOT_TIMEOUT, Shot};
use crate::file_open::Opening;
use crate::save::Saving;
use crate::state::UiState;
use crate::theme::Tokens;
use crate::title_bar::APP_NAME;

/// The height of the title bar's row, in points.
pub(crate) const TOP_BAR_ROW_HEIGHT: f32 = 32.0;

/// The inner margin of the top bar, in points: the title bar is the row and this margin around it.
pub(crate) const TOP_BAR_MARGIN: Margin = Margin { left: 8, right: 8, top: 3, bottom: 3 };

/// How thick the strip along each edge of an app-drawn frame is, in points, where a press resizes the window.
const EDGE: f32 = 4.0;

/// The side of the square at each corner of an app-drawn frame, in points. It lies in the top bar's margin, clear
/// of the window buttons.
const CORNER: f32 = 8.0;

/// How soon after a panic in a frame the next frame is asked for, so the notice shows without waiting for input.
/// Not at once: a panic that happens every frame is then a frame every 100 ms, not a frame as fast as the screen goes.
const PANIC_REPAINT: Duration = Duration::from_millis(100);

/// The most controls a frame remembers by name for `ui.elements`.
const MAX_ELEMENTS: usize = 4096;

/// The name the engine gives a drawing that has none yet; the window calls such a drawing `Drawing1`, `Drawing2`...
const UNNAMED: &str = "Untitled";

/// A window button an app-drawn frame has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowButton {
    Minimize,
    Maximize,
    Close,
}

/// Which window buttons sit at which end of the top bar, outermost last. Used only when the app draws the
/// window frame (a desktop that leaves it to the app); otherwise the desktop's own frame has them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowButtons {
    pub left: Vec<WindowButton>,
    pub right: Vec<WindowButton>,
}

impl Default for WindowButtons {
    /// All three, on the right.
    fn default() -> Self {
        WindowButtons { left: Vec::new(), right: vec![WindowButton::Minimize, WindowButton::Maximize, WindowButton::Close] }
    }
}

/// Where a [`Probe`] runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage<'a> {
    /// At the start of drawing a frame.
    Frame,
    /// At the start of handling a control request, with its method.
    Request(&'a str),
    /// Inside a panel's contents, in the middle of drawing the frame: `top_bar`, `status_bar` or `canvas`.
    Panel(&'a str),
}

/// Code that runs inside the panic guard at the start of each frame and control request, and inside each panel. It
/// exists so a test can make the guard's entry points, and a panic in the middle of a frame, and see the window go
/// on; the app installs none.
pub type Probe = Box<dyn FnMut(Stage<'_>)>;

/// A control the last frame drew: where it is and whether it can be used. `ui.elements` lists them, so that a test or an
/// agent can find a control by name and click it.
#[derive(Clone, Debug)]
pub struct Element {
    pub name: String,
    pub rect: Rect,
    pub enabled: bool,
}

/// The DiaCraft window.
pub struct DiacraftApp {
    pub(crate) session: Session,
    pub(crate) artwork: crate::branding::Artwork,
    /// Where the artwork and the licence list come from: the host of the browser, which fetches them, or (with none) the program itself (`asset_host.rs`).
    pub(crate) assets: crate::asset_host::Source,
    /// The list of licences, made when its window first opens (`licenses.rs`).
    pub(crate) licence_list: crate::licenses::Listing,
    /// The git release and the year the splash and About panel say (`splash.rs`): what the application handed the engine, until a test gives its own.
    pub(crate) build: diacraft_engine::build_info::BuildInfo,
    /// The time the window's own logic reads (`clock.rs`): the machine's, or a test's that stands still until it is advanced.
    pub(crate) clock: crate::clock::Clock,
    /// Opening files: the dialog, the thread that reads, the message about a file that would not open.
    pub(crate) opening: Opening,
    /// Saving, and the question about unsaved changes.
    pub(crate) saving: Saving,
    /// Exporting a page as a picture.
    pub(crate) exporting: crate::export::Exporting,
    /// Files in a window with no file system and no threads (the browser): the host, the card that names a file, what is being handed over (`file_host.rs`).
    pub(crate) hosting: crate::file_host::Hosting,
    /// The page of the browser the window is in: its host, and what the window asked of it (`page_host.rs`).
    pub(crate) page: crate::page_host::Page,
    pub(crate) control: Option<ControlInbox>,
    /// The status bar's notice, with its kind and when it was first shown (`notice.rs`).
    pub(crate) notice: Option<crate::notice::Notice>,
    pub(crate) frames: u64,
    pub(crate) ui_panics: u64,
    pub(crate) screenshot: Option<Shot>,
    pub(crate) screenshot_timeout: Duration,
    /// When the window closes after `app.quit`: a reading of the window's clock (`clock.rs`), so a test's clock decides when, and not the machine's speed.
    pub(crate) quit_at: Option<Duration>,
    pub(crate) quit_sent: bool,
    /// Whether the window's fonts have been handed to egui (on the first pass).
    fonts_installed: bool,
    /// Optional interface faces prepared off the UI thread and installed once.
    interface_fonts: Option<crate::fonts::Preparation>,
    window_buttons: Option<WindowButtons>,
    probe: RefCell<Option<Probe>>,
    /// What the window itself remembers (`state`).
    pub(crate) ui: UiState,
    /// The theme egui was last given.
    applied_theme: Option<egui::Theme>,
    /// The window title last sent to the desktop.
    sent_title: Option<String>,
    /// Which drawing this is among those made in this window: `Drawing1`, `Drawing2`...
    drawing_number: u32,
    /// The controls the last frame drew, and the ones this frame has drawn so far.
    drawn: Vec<Element>,
    last_elements: Vec<Element>,
    /// Input events from the control channel, handed to egui at the start of the next frame.
    pub(crate) synthetic: Vec<egui::Event>,
}

impl DiacraftApp {
    /// A window on `session`. With a `control` inbox, the window answers the requests the control channel
    /// puts in it.
    pub fn new(session: Session, control: Option<ControlInbox>) -> DiacraftApp {
        let drawing_number = u32::from(session.document().is_some());
        let mut app = DiacraftApp {
            session,
            artwork: crate::branding::Artwork::default(),
            assets: crate::asset_host::Source::default(),
            licence_list: crate::licenses::Listing::default(),
            build: diacraft_engine::build_info::get(),
            clock: crate::clock::Clock::real(),
            opening: Opening::default(),
            saving: Saving::default(),
            exporting: crate::export::Exporting::default(),
            hosting: crate::file_host::Hosting::default(),
            page: crate::page_host::Page::default(),
            control,
            notice: None,
            frames: 0,
            ui_panics: 0,
            screenshot: None,
            screenshot_timeout: DEFAULT_SCREENSHOT_TIMEOUT,
            quit_at: None,
            quit_sent: false,
            fonts_installed: false,
            interface_fonts: None,
            window_buttons: None,
            probe: RefCell::new(None),
            ui: UiState::default(),
            applied_theme: None,
            sent_title: None,
            drawing_number,
            drawn: Vec::new(),
            last_elements: Vec::new(),
            synthetic: Vec::new(),
        };
        // The View settings of the drawing the window starts on are the drawing's.
        app.showed_drawings_window_settings();
        app
    }

    /// The session the window shows.
    pub fn session(&self) -> &Session {
        &self.session
    }

    /// Runs the command `id` with `params`: how the window changes anything. The window's own commands (zoom, panels, tabs,
    /// the page shown) are run here; the others go to the engine. A command that panics leaves the session as it was and
    /// puts a notice in the status bar.
    pub fn execute(&mut self, id: &str, params: &Value) -> diacraft_engine::Result<Value> {
        // A label being typed is sent before a command changes the drawing or the page on view (of the M3 review).
        if crate::commands::ends_text_edit(id) {
            self.commit_pending_text();
        }
        // A command that may make the page grow (Auto Size) is watched, so that the view follows a drawing the engine carried.
        let growth = self.watch_growth(id, params);
        let result = match self.run_window_command(id, params) {
            Some(result) => result,
            None => self.session.execute(id, params),
        };
        // Something done takes away the notice it follows (`notice.rs`), before what this command says itself is put in its place.
        if result.is_ok() {
            self.notice_after_command(id);
            // What a picture inserted by the window command says of itself goes in the place of the notice the command took away.
            if id == "window.insertPicture" {
                self.say_picture_note();
            }
            // So does what Change Shape left as it was.
            if id == "arrange.changeShape"
                && let Some(note) = self.ui.change_note.take()
            {
                self.set_notice(&note);
            }
        }
        if let (Ok(_), Some(growth)) = (&result, growth) {
            self.settle_growth(growth);
        }
        if let Err(EngineError::Internal { cmd, msg }) = &result {
            self.set_notice(&format!("internal error in `{cmd}`: {msg} (the document was left as it was; please report this bug)"));
        }
        if let (Ok(value), true) = (&result, id == "file.new") {
            self.began_new_drawing(value);
        }
        // What a command warns of on a drawing saved as an `.odg` (a shape or a connector the `.odg` save does not write yet) is in the status bar, where the
        // save's own refusal would show it too late.
        if let (Ok(value), "shape.drop" | "connector.add" | "connector.style") = (&result, id)
            && let Some(warning) = value.get("warnings").and_then(Value::as_array).and_then(|w| w.first()).and_then(Value::as_str)
        {
            self.set_drawing_notice(warning);
        }
        // A move that left a member or a callout a GUARD holds where it was says which, in the status bar: a person who dragged a container sees why
        // a shape stayed behind.
        if let (Ok(value), "shape.move") = (&result, id)
            && value.get("left").is_some()
            && let Some(note) =
                value.get("notes").and_then(Value::as_array).and_then(|notes| notes.iter().filter_map(Value::as_str).find(|n| n.contains("GUARD")))
        {
            self.set_notice(note);
        }
        match (&result, id) {
            (Ok(value), "stencil.open") => self.noted_stencil_open(params, value),
            (Ok(_), "stencil.close") => self.noted_stencil_close(params),
            // The Quick Shapes of a stencil changed (by the window, a program or a batch): the list that holds them is read again.
            (Ok(_), "stencil.add_quick" | "stencil.remove_quick" | "stencil.reset") => self.ui.stencils.forget_list(),
            _ => {}
        }
        // A copy of a page is the page on view (Duplicate This Page; the page tab's Duplicate): the way a page chosen by its tab is.
        if let (Ok(value), "page.duplicate") = (&result, id)
            && let Some(copy) = value.get("id").and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok())
        {
            self.ui.page = Some(copy);
        }
        if let (Ok(value), true) = (&result, id == "file.open") {
            self.began_opened_drawing(value);
        }
        // The View settings are the drawing's: a command that changed its windows part (a box turned, an undo of one, a batch, a program's
        // `window.settings.set`) has the window show what the drawing shows now.
        self.followed_window_settings();
        result
    }

    /// A new unnamed drawing is the next in the count that names it ("Drawing2").
    pub(crate) fn numbered_another_drawing(&mut self) {
        self.drawing_number = self.drawing_number.saturating_add(1);
    }

    /// A template's answer (`file.new {template}`, `file.open` of a template) carries `stencils`: the stencils the template opened. The stencil shown is then
    /// the first of them, and not the one the drawing before it was showing; a plain `file.open` and a blank `file.new` carry
    /// none and leave what is shown as it is.
    pub(crate) fn showed_template_stencils(&mut self, result: &Value) {
        if result.get("stencils").is_some() {
            self.ui.stencils.current = None;
        }
        // Both drawing-replacement paths invalidate the catalogue before this hook. Refresh it now:
        // API state and the filter must follow the new stencil even while Shapes is collapsed.
        self.stencil_list();
    }

    /// A new drawing replaced the old one: it has its own name and shows its first page, fitted. `result` is the answer of `file.new`.
    fn began_new_drawing(&mut self, result: &Value) {
        self.drawing_replaced();
        self.numbered_another_drawing();
        // The stencils open are the new drawing's: a template names the ones it opens.
        self.ui.stencils.forget_list();
        self.showed_template_stencils(result);
        self.ui.page = None;
        self.ui.views.clear();
        self.ui.pictures.reset();
        self.ui.print.began_another_drawing();
        self.forget_page_setup();
        self.ui.backstage = None;
        self.edit_reset();
        self.clear_drawing_notice();
        self.opening.reading.clear();
        self.opening.banner = None;
        // The view toggles are the new drawing's, not where the last file left them: a drawing with no windows part shows the grid, the rulers and
        // the guides on (the new drawing), the connection points off, and one made from a template shows what the template's window holds
        // (`window_settings.rs`).
        self.showed_drawings_window_settings();
    }

    /// Gives the window `clock` to read the time from, in place of the machine's: a test's clock stands still until the test advances it, so that the window's
    /// timed work (a picture drawn again at rest, a drawing that failed asked for again, an edit's outline kept while the picture catches up, the seconds a
    /// file takes) happens when the test lets time pass and not when the machine happens to be slow (`clock.rs`).
    pub fn with_clock(mut self, clock: crate::clock::Clock) -> DiacraftApp {
        self.clock = clock;
        self
    }

    /// The clock the window reads the time from.
    pub fn clock(&self) -> &crate::clock::Clock {
        &self.clock
    }

    /// Starts with `notice` in the status bar: something the person should know. It is escaped and cut short.
    pub fn with_notice(mut self, notice: impl AsRef<str>) -> DiacraftApp {
        self.set_notice(notice.as_ref());
        self
    }

    /// How long a `ui.screenshot` waits for a frame to be presented before it is an error.
    pub fn with_screenshot_timeout(mut self, timeout: Duration) -> DiacraftApp {
        self.screenshot_timeout = timeout;
        self
    }

    /// The app draws the window frame: the top bar carries the window buttons, and its empty part moves the
    /// window. The frame always has a close button; if `buttons` has none, one is added at the right end.
    pub fn with_window_buttons(mut self, mut buttons: WindowButtons) -> DiacraftApp {
        if !buttons.left.contains(&WindowButton::Close) && !buttons.right.contains(&WindowButton::Close) {
            buttons.right.push(WindowButton::Close);
        }
        self.window_buttons = Some(buttons);
        self
    }

    /// Draws the pictures of a theme preview with `make`'s drawer on the calling thread, at once. For tests, which then read the pixels of the preview.
    pub fn with_inline_theme_preview(mut self, make: crate::render_worker::MakeDrawer) -> DiacraftApp {
        self.ui.theme_preview.pictures = crate::canvas::picture::Pictures::inline(make);
        self
    }

    /// Draws pages with `make`'s drawer on the calling thread, at once, in place of the renderer on a thread of its own. For tests,
    /// which then see a picture in the frame that asked for it.
    pub fn with_inline_pictures(mut self, make: crate::render_worker::MakeDrawer) -> DiacraftApp {
        self.ui.pictures = crate::canvas::picture::Pictures::inline(make);
        // A theme preview is drawn the same way, in the bundled faces.
        self.ui.theme_preview.pictures = crate::canvas::picture::Pictures::inline(crate::render_worker::bundled_drawer());
        // The sheets of the Print view are drawn the same way, and a print is laid out in the bundled faces alone.
        self.ui.print.preview = crate::print_preview::Preview::inline_bundled();
        self.ui.print.fonts = crate::print::PrintFonts::Bundled;
        self
    }

    /// Makes the index of a page's shapes and the targets of a drag ready by `reader` instead of on a thread of the window's own (`snap_reader.rs`): a test runs it inline,
    /// so that a drag's first query finds the page ready, or holds the thread back with a hook of its own.
    pub fn with_snap_reader(mut self, reader: crate::snap_reader::SnapReader) -> DiacraftApp {
        self.ui.edit.set_reader(reader);
        self
    }

    /// Whether a reader of pages was given by [`DiacraftApp::with_snap_reader`], and not the window's own.
    pub fn snap_reader_is_set(&self) -> bool {
        self.ui.edit.reader_given()
    }

    /// Sets all the text of the window in `fonts`, the one set of faces the application found at its start: the page's pictures, the sheets of
    /// the Print view, a print, and the list of the Font box. The application gives the same faces to the engine
    /// (`diacraft_engine::measure::start_system_fonts` does both), so that a block sized by a commit is drawn in the faces it was sized in.
    ///
    /// **This does not wait for the faces.** They may still be being read from the machine's folders (seconds, on a cold disk): the first frame
    /// is drawn without them, and each document consumer takes them on its own thread, where it waits, when it first has work.
    /// Native interface text prefers an open sans family from the same database, loaded on its own worker.
    /// Web builds keep the initial bundled/craft-fonts interface faces.
    pub fn with_fonts(mut self, fonts: diacraft_engine::measure::SharedFonts) -> DiacraftApp {
        let interface = fonts.clone();
        self.interface_fonts = crate::fonts::Preparation::start(move || crate::fonts::font_definitions_with(&interface.get()));
        self.ui.pictures = crate::canvas::picture::Pictures::threaded(crate::render_worker::shared_drawer(fonts.clone()));
        self.ui.theme_preview.pictures = crate::canvas::picture::Pictures::threaded(crate::render_worker::shared_drawer(fonts.clone()));
        self.ui.print.preview = crate::print_preview::Preview::threaded_with(crate::render_worker::shared_drawer(fonts.clone()));
        self.ui.print.fonts = crate::print::PrintFonts::Shared(fonts.clone());
        self.ui.format.fonts = crate::font_list::FontList::with_scan(crate::font_list::shared_scan(fonts.clone()));
        self.ui.format.shared = Some(fonts);
        self
    }

    /// Watches the scan of the machine's faces `fonts` without drawing in them: while it goes the window asks for a frame every tenth of a second, so that the
    /// face the Font box names (the default of the database once it is found, `Ubuntu` before) is read again when it ends. `with_fonts` does this as well.
    pub fn with_font_watch(mut self, fonts: diacraft_engine::measure::SharedFonts) -> DiacraftApp {
        self.ui.format.shared = Some(fonts);
        self
    }

    /// Draws pages with `make`'s drawer on a thread of its own.
    pub fn with_threaded_pictures(mut self, make: crate::render_worker::MakeDrawer) -> DiacraftApp {
        self.ui.pictures = crate::canvas::picture::Pictures::threaded(make);
        self
    }

    /// Finds the font families of the Font box with `scan`, on its thread, in place of reading the machine's fonts. For tests.
    pub fn with_font_scan(mut self, scan: crate::font_list::Scan) -> DiacraftApp {
        self.ui.format.fonts = crate::font_list::FontList::with_scan(scan);
        self
    }

    /// Whether the scan for the Font box's families is going and has not answered yet.
    pub fn fonts_scanning(&self) -> bool {
        self.ui.format.fonts.scanning()
    }

    /// While the scan of the machine's faces goes (`with_fonts`, `with_font_watch`), a frame is asked for every tenth of a second, so that the face the Font box
    /// names is read again when the scan ends (the format view's key holds whether the faces are in hand, `format.rs`). It asks for nothing once they are.
    pub(crate) fn watch_fonts(&mut self, ctx: &egui::Context) {
        if self.ui.format.shared.as_ref().is_some_and(|fonts| fonts.try_get().is_none()) {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        if let Some(preparation) = &self.interface_fonts {
            if let Some(result) = preparation.poll() {
                self.interface_fonts = None;
                match result {
                    Ok(definitions) => ctx.set_fonts(definitions),
                    Err(error) => log::warn!("interface fonts could not load ({error}); keeping the bundled faces"),
                }
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
        }
    }

    /// Runs `probe` inside the panic guard at the start of every frame and control request. For tests.
    pub fn with_probe(mut self, probe: impl FnMut(Stage<'_>) + 'static) -> DiacraftApp {
        self.probe = RefCell::new(Some(Box::new(probe)));
        self
    }

    /// Whether the app draws the window frame.
    pub(crate) fn has_window_buttons(&self) -> bool {
        self.window_buttons.is_some()
    }

    /// The window buttons of a frame the app draws.
    pub(crate) fn window_buttons_layout(&self) -> Option<WindowButtons> {
        self.window_buttons.clone()
    }

    /// The name the window gives the drawing: its own title, or `Drawing1`, `Drawing2`... for one the engine calls
    /// `Untitled`. A project rule: the numbering is the window's, counted from the first drawing it made.
    ///
    /// **A drawing opened from a file is named by the file** (`plan.vsdx`), not by the title in its properties: the file's name is
    /// what a person knows it by. A project rule.
    pub fn display_name(&self) -> String {
        let file_name = self.session.source().and_then(|f| f.path().file_name()).map(|n| shown(&n.to_string_lossy(), 64));
        match self.session.document() {
            None => APP_NAME.to_owned(),
            Some(_) if file_name.is_some() => file_name.unwrap_or_default(),
            Some(doc) if doc.title.is_empty() || &*doc.title == UNNAMED => format!("Drawing{}", self.drawing_number.max(1)),
            Some(doc) => shown(&doc.title, 64),
        }
    }

    /// The window's title: the drawing's name followed by the application's.
    ///
    /// **A drawing with changes that are not saved has a star before its name** (a project rule: no spec says how the reference app shows it). A label
    /// being typed, not yet sent to the drawing, counts: it is a change that a close would lose.
    pub fn window_title(&self) -> String {
        match self.session.document() {
            None => APP_NAME.to_owned(),
            Some(_) => format!("{}{} - {APP_NAME}", if self.is_dirty() || self.has_unsent_text() { "*" } else { "" }, self.display_name()),
        }
    }

    /// What the status line says of the document: `No document open`, or `Drawing1 · 2 pages`.
    pub fn status_text(&self) -> String {
        match self.session.document() {
            None => "No document open".to_owned(),
            Some(doc) => {
                let pages = doc.pages.len();
                format!("{} · {pages} {}", self.display_name(), if pages == 1 { "page" } else { "pages" })
            }
        }
    }

    /// How many frames have been drawn.
    pub fn frames(&self) -> u64 {
        self.frames
    }

    /// How many panics the window's own guard caught, in a frame or a control request. The engine counts
    /// its own ([`Session::panics_caught`]).
    pub fn ui_panics(&self) -> u64 {
        self.ui_panics
    }

    /// Whether `app.quit` has been asked for: the window is about to close, or has been told to.
    pub fn quit_requested(&self) -> bool {
        self.quit_at.is_some() || self.quit_sent
    }

    /// The input events the control channel queued for the next frame: up to and including the first press, release or key, so
    /// the events after it meet a frame that has seen the pointer where it was. The desktop app hands them to egui from
    /// `raw_input_hook`; a test that drives the window by hand does the same.
    pub fn next_input_frame(&mut self) -> Vec<egui::Event> {
        let n = self.synthetic_frame_len();
        self.synthetic.drain(..n).collect()
    }

    /// How many input events from the control channel wait to be handed to egui.
    pub fn pending_input(&self) -> usize {
        self.synthetic.len()
    }

    /// The controls the last frame drew, by name.
    pub fn elements(&self) -> &[Element] {
        &self.last_elements
    }

    /// The control the last frame drew under `name`.
    pub fn element(&self, name: &str) -> Option<&Element> {
        self.last_elements.iter().find(|e| e.name == name)
    }

    /// Remembers that this frame drew a control, for [`DiacraftApp::elements`].
    pub(crate) fn note(&mut self, name: impl Into<String>, rect: Rect, enabled: bool) {
        if self.drawn.len() < MAX_ELEMENTS {
            self.drawn.push(Element { name: name.into(), rect, enabled });
        }
    }

    pub(crate) fn run_probe(&self, stage: Stage<'_>) {
        // A probe that is running (and panicked, say) is not run again from inside itself.
        if let Ok(mut probe) = self.probe.try_borrow_mut()
            && let Some(probe) = probe.as_mut()
        {
            probe(stage);
        }
    }

    /// Runs `f` under the panic guard. A panic is counted and shown as a notice, another frame is asked for soon so
    /// the notice shows and the frame that was cut short is drawn again, and `None` comes back.
    fn guarded<T>(&mut self, ctx: &egui::Context, what: &str, f: impl FnOnce(&mut DiacraftApp) -> T) -> Option<T> {
        match guard::catch_panic(|| f(self)) {
            Ok(value) => Some(value),
            Err(msg) => {
                self.ui_panics += 1;
                self.set_notice(&format!("internal error in the {what}: {msg} (the window carries on; please report this bug)"));
                ctx.request_repaint_after(PANIC_REPAINT);
                None
            }
        }
    }

    fn draw(&mut self, ui: &mut Ui, tokens: Tokens) {
        self.drawn.clear();
        self.title_bar(ui, tokens);
        if let Some(page) = self.ui.backstage {
            self.backstage(ui, tokens, page);
        } else {
            self.ribbon_panel(ui);
            // The formula bar, directly under the ribbon body while a ShapeSheet is open (`sheet/area.rs`).
            self.sheet_bar_panel(ui, tokens);
            self.status_bar(ui, tokens);
            self.shapes_window(ui, tokens);
            // The panes at the right, before the page tabs and the drawing area: the tabs run under the drawing area only.
            self.right_dock(ui, tokens);
            // The ShapeSheet area, below or beside the drawing, or filling the band in place of it.
            let sheet_fills = self.sheet_area(ui, tokens);
            if !sheet_fills {
                self.page_tabs(ui, tokens);
                self.canvas(ui, tokens);
            }
            self.open_overlay(ui, tokens);
            self.sheet_focus_update(&ui.ctx().clone());
        }
        // The dialogs of page setup, then the questions over them: a question asked while a dialog is open is the one to answer first.
        self.page_dialogs(&ui.ctx().clone(), tokens);
        self.questions(&ui.ctx().clone(), tokens);
        // Over everything, the splash or About panel (`splash.rs`).
        self.splash(&ui.ctx().clone(), tokens);
        // Last, so that it is over the panels where they meet the edge.
        if self.window_buttons.is_some() {
            resize_edges(ui);
        }
        self.last_elements = std::mem::take(&mut self.drawn);
    }

    /// Gives egui the theme the window follows, and the window's title to the desktop, when either has changed.
    fn sync_with_desktop(&mut self, ctx: &egui::Context) {
        if self.applied_theme != Some(self.ui.theme) {
            self.applied_theme = Some(self.ui.theme);
            crate::theme::apply(ctx, self.ui.theme);
        }
        let title = self.window_title();
        if self.sent_title.as_deref() != Some(title.as_str()) {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.sent_title = Some(title);
        }
    }
}

impl eframe::App for DiacraftApp {
    /// Answers the control channel, and moves a screenshot or a quit along. Runs before every frame, and
    /// also while the window is hidden, so the channel still answers a minimised window.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Before any text is laid out: egui's own default fonts (empty, with `default_fonts` off) never draw.
        if !self.fonts_installed {
            self.fonts_installed = true;
            // Native optional faces are loaded and validated by with_fonts' worker. Browser inputs
            // are compile-time bundled bytes only; there is no filesystem or native worker there.
            let definitions = if cfg!(target_arch = "wasm32") {
                crate::fonts::font_definitions_with(&diacraft_fonts::FontDb::bundled())
            } else {
                crate::fonts::font_definitions()
            };
            ctx.set_fonts(definitions);
        }
        self.guarded(ctx, "control channel", |app| {
            app.sync_with_desktop(ctx);
            // With a host (the browser): the work that waited for a frame is done, and what the page has for the window is taken (`file_host.rs`).
            app.run_deferred(ctx);
            app.advance_host(ctx);
            app.advance_page(ctx);
            app.advance_open(ctx);
            app.advance_save(ctx);
            app.advance_export(ctx);
            app.advance_print(ctx);
            app.advance_stencil_dialog(ctx);
            app.advance_insert(ctx);
            app.drain_control(ctx);
            app.advance_screenshot(ctx);
            app.advance_quit(ctx);
        });
    }

    /// Hands the input the control channel queued to egui, as if the pointer or keyboard had made it.
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        raw_input.events.extend(self.next_input_frame());
        if !self.synthetic.is_empty() {
            ctx.request_repaint();
        }
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.frames += 1;
        let ctx = ui.ctx().clone();
        let tokens = Tokens::for_theme(ctx.theme());
        self.guarded(&ctx, "window", |app| {
            app.run_probe(Stage::Frame);
            // A value typed in a field of a pane is sent before the title bar, the ribbon or the File view act on a click that leaves the field: a Save,
            // an Undo or an Open that the click makes finds the value in the drawing (`panes/fields.rs`).
            app.flush_edits(&ctx);
            app.watch_fonts(&ctx);
            app.prepare_shape_filter_keys(&ctx);
            // Escape dismisses the splash before anything else can take it (`splash.rs`).
            app.splash_keys(&ctx);
            // The ShapeSheet's keys while it has the focus: before the shortcuts and the drawing's keys, which then stand aside (`sheet/keys.rs`).
            app.run_sheet_keys(&ctx);
            app.run_shortcuts(&ctx);
            app.run_edit_keys(&ctx);
            // A notice whose time is up is not drawn; one the frame shows starts its time, and the frame asks for the next at the moment it ends.
            app.expire_notice();
            app.draw(ui, tokens);
            app.time_notice(&ctx);
            // What this frame did to the drawing (an edit, a save, a new drawing) is known to the browser now, which asks before the page is left while there are changes.
            app.sync_leave_guard();
            // What a copy or a cut put aside for the system clipboard (`clipboard.rs`): the text of the labels, where egui carries text or an image.
            if let Some(text) = app.take_system_copy() {
                ctx.copy_text(text);
            }
        });
        if !self.synthetic.is_empty() {
            ctx.request_repaint();
        }
    }
}

/// How many `StartDrag` commands this pass has sent so far.
pub(crate) fn drags_sent(ctx: &egui::Context) -> usize {
    ctx.viewport(|v| v.commands.iter().filter(|c| matches!(c, ViewportCommand::StartDrag)).count())
}

/// The window manager runs the move or the resize from here, and on X11 and Wayland it keeps the button release.
/// egui would go on holding the press and the drag until the next press, and that press would start no drag on the
/// bar and no resize (found by running the window: the first drag moved it, the second did nothing; the same
/// workaround is in the sibling craft apps, which cite emilk/egui#7959). So end both now. Where the window system
/// hands the release back (Windows), this ends nothing.
pub(crate) fn handed_to_os(ctx: &egui::Context) {
    ctx.stop_dragging();
    ctx.input_mut(|i| i.pointer = Default::default());
}

/// The strips along the edges and the squares at the corners of an app-drawn frame, with what a press in each
/// resizes and the cursor over it. The frame has no border of the desktop's to grab, so these are its borders.
fn resize_zones(screen: Rect) -> [(ResizeDirection, CursorIcon, Rect); 8] {
    let (l, r, t, b) = (screen.left(), screen.right(), screen.top(), screen.bottom());
    let side = |x0: f32, y0: f32, x1: f32, y1: f32| Rect::from_min_max(Pos2::new(x0, y0), Pos2::new(x1, y1));
    [
        (ResizeDirection::NorthWest, CursorIcon::ResizeNorthWest, side(l, t, l + CORNER, t + CORNER)),
        (ResizeDirection::NorthEast, CursorIcon::ResizeNorthEast, side(r - CORNER, t, r, t + CORNER)),
        (ResizeDirection::SouthWest, CursorIcon::ResizeSouthWest, side(l, b - CORNER, l + CORNER, b)),
        (ResizeDirection::SouthEast, CursorIcon::ResizeSouthEast, side(r - CORNER, b - CORNER, r, b)),
        (ResizeDirection::North, CursorIcon::ResizeNorth, side(l + CORNER, t, r - CORNER, t + EDGE)),
        (ResizeDirection::South, CursorIcon::ResizeSouth, side(l + CORNER, b - EDGE, r - CORNER, b)),
        (ResizeDirection::West, CursorIcon::ResizeWest, side(l, t + CORNER, l + EDGE, b - CORNER)),
        (ResizeDirection::East, CursorIcon::ResizeEast, side(r - EDGE, t + CORNER, r, b - CORNER)),
    ]
}

/// Lets a press on the edge of an app-drawn frame resize the window, as the desktop's own frame would. Not in a
/// maximised or full-screen window, which has no edges to pull.
fn resize_edges(ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    let (maximized, fullscreen) = ctx.input(|i| (i.viewport().maximized.unwrap_or(false), i.viewport().fullscreen.unwrap_or(false)));
    if maximized || fullscreen {
        return;
    }
    for (index, (direction, cursor, rect)) in resize_zones(ctx.content_rect()).into_iter().enumerate() {
        let response = ui.interact(rect, ui.scope_id().with(("resize_edge", index)), Sense::drag());
        if response.hovered() || response.dragged() {
            ctx.set_cursor_icon(cursor);
        }
        // On the press itself: the desktop resizes only while the button is still down.
        if response.is_pointer_button_down_on() && ctx.input(|i| i.pointer.primary_pressed()) {
            ctx.send_viewport_cmd(ViewportCommand::BeginResize(direction));
            handed_to_os(&ctx);
        }
    }
}

/// One window button, drawn in code: a minus, a square or a cross.
pub(crate) fn window_button(ui: &mut Ui, button: WindowButton, t: Tokens) {
    let maximized = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
    let label = match button {
        WindowButton::Minimize => "Minimize",
        WindowButton::Maximize if maximized => "Restore",
        WindowButton::Maximize => "Maximize",
        WindowButton::Close => "Close",
    };
    let (rect, response) = ui.allocate_exact_size(Vec2::new(32.0, 24.0), Sense::click());
    response.widget_info(|| WidgetInfo::labeled(Role::Button, true, label));
    let response = response.on_hover_text(label);
    let closing = button == WindowButton::Close;
    let (fill, ink) = match (response.hovered(), closing) {
        (true, true) => (t.close_hover_fill, t.close_hover_icon),
        (true, false) => (t.top_bar_hover, t.top_bar_text),
        (false, _) => (t.control_idle_fill, t.top_bar_text),
    };
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        painter.rect_filled(rect, 3.0, fill);
        let c = rect.center();
        let stroke = Stroke::new(1.25, ink);
        let r = 5.0;
        match button {
            WindowButton::Minimize => {
                painter.hline(c.x - r..=c.x + r, c.y + 2.0, stroke);
            }
            WindowButton::Maximize => {
                painter.rect_stroke(egui::Rect::from_center_size(c, Vec2::splat(2.0 * r)), 1.0, stroke, egui::StrokeKind::Inside);
            }
            WindowButton::Close => {
                painter.line_segment([c + Vec2::new(-r, -r), c + Vec2::new(r, r)], stroke);
                painter.line_segment([c + Vec2::new(-r, r), c + Vec2::new(r, -r)], stroke);
            }
        }
    }
    if response.clicked() {
        let ctx = ui.ctx();
        ctx.send_viewport_cmd(match button {
            WindowButton::Minimize => ViewportCommand::Minimized(true),
            WindowButton::Maximize => ViewportCommand::Maximized(!maximized),
            WindowButton::Close => ViewportCommand::Close,
        });
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use std::sync::Arc;
    use std::sync::mpsc::channel;
    use std::time::Duration;

    use diacraft_engine::measure::scan_fonts_with;
    use diacraft_fonts::FontDb;

    use super::*;

    /// One set of faces for the window, found on a thread of its own. `with_fonts` gives the same faces to the page's pictures, the Print view's
    /// sheets, a print, the Font box's list and the window's watch of the scan (the format view's), builds none of its own, and **does not wait for the
    /// scan** (the first frame comes before the faces): the scan here is held back, and the test holds one holder and each of the others. When the scan
    /// is done they all have the one database.
    #[test]
    fn with_fonts_shares_one_set_of_faces_with_everything_that_sets_text_and_does_not_wait_for_it() {
        let (release, gate) = channel::<()>();
        // A scan that is let go after five seconds whatever happens, so that a `with_fonts` that waited would fail below and not hang.
        let backstop = release.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(5));
            let _ = backstop.send(());
        });
        let fonts = scan_fonts_with(move || {
            let _ = gate.recv();
            FontDb::bundled()
        });
        let mut app = DiacraftApp::new(Session::new(), None).with_fonts(fonts.clone());
        assert!(!fonts.is_ready(), "`with_fonts` did not wait for the scan");
        assert_eq!(
            fonts.holders(),
            9,
            "the test, the engine's commits, the pictures, the theme preview's pictures, the Print view's preview, the print, the Font box, the window's watch and the interface-font worker hold the faces"
        );
        assert!(matches!(&app.ui.print.fonts, crate::print::PrintFonts::Shared(held) if held.same_as(&fonts)));
        release.send(()).unwrap();
        // The renderer a print job builds draws in the database the scan found.
        let db = fonts.get();
        app.interface_fonts.as_mut().unwrap().finish_for_test().unwrap();
        app.watch_fonts(&egui::Context::default());
        assert!(app.interface_fonts.is_none(), "the completed answer was consumed once");
        let before = Arc::strong_count(&db);
        let renderer = app.ui.print.fonts.clone().renderer();
        assert_eq!(Arc::strong_count(&db), before + 1, "the print's renderer holds the same database and not a copy");
        drop(renderer);
        diacraft_engine::measure::set_measure_fonts(None);
        // A window given none keeps to the machine's faces, read by the job's own thread, and the tests' windows to the bundled faces.
        let plain = DiacraftApp::new(Session::new(), None);
        assert!(matches!(plain.ui.print.fonts, crate::print::PrintFonts::System));
    }

    #[test]
    fn interface_font_preparation_never_blocks_frames_after_metadata_is_ready() {
        let (release, gate) = channel();
        let (started, began) = channel();
        let (finished, prepared) = channel();
        let mut db = FontDb::empty();
        assert_eq!(db.add_font_data(diacraft_fonts::synth::SynthFont::new("Inter").ascii(550).build()).added, 1);
        let fonts = diacraft_engine::measure::SharedFonts::ready(Arc::new(db));
        assert!(fonts.is_ready(), "the metadata scan is already complete");
        let mut app = DiacraftApp::new(Session::new(), None).with_font_watch(fonts.clone());
        app.interface_fonts = crate::fonts::Preparation::start(move || {
            started.send(std::thread::current().id()).unwrap();
            gate.recv_timeout(Duration::from_secs(5)).unwrap();
            let definitions = crate::fonts::font_definitions_with(&fonts.get());
            finished.send(()).unwrap();
            definitions
        });
        assert!(app.interface_fonts.is_some());
        assert_ne!(began.recv_timeout(Duration::from_secs(5)).unwrap(), std::thread::current().id());
        let mut window = egui_kittest::Harness::builder().with_size([800.0, 500.0]).build_eframe(|_| app);
        let first_frame = window.state().frames();
        for _ in 0..5 {
            window.step();
        }
        assert!(window.state().frames() >= first_frame + 5, "frames continue while face loading is held");
        assert!(window.state().interface_fonts.is_some());
        assert!(prepared.try_recv().is_err(), "preparation has not been released");
        release.send(()).unwrap();
        prepared.recv_timeout(Duration::from_secs(5)).unwrap();
        window.state_mut().interface_fonts.as_mut().unwrap().finish_for_test().unwrap();
        window.step();
        assert!(window.state().interface_fonts.is_none(), "the completed answer was consumed once");
        window.step();
        window.ctx.fonts(|fonts| assert!(fonts.definitions().font_data.contains_key("Inter-UI-Regular")));
    }
}
