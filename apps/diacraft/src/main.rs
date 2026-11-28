//! DiaCraft desktop app.
//!
//! Usage: `diacraft [--control <port> [--automation-read-root <dir>]... [--automation-write-root <dir>]...] [--reset-graphics] [FILE]`, `diacraft --version`, `diacraft --help` (or `-h`). `FILE` is a drawing to open.
//!
//! `--control <port>` (or `DIACRAFT_CONTROL_PORT`; a port from 1 to 65535, or the app does not start) starts
//! a loopback JSON-lines control server: `{"id":1,"method":"ui.inspect","params":{}}` is answered by
//! `{"id":1,"ok":true,"result":…}`. If the port is already in use, the app logs why, says so in its status bar
//! and runs without the channel. The methods are in `docs/control-protocol.md`.
//!
//! **A start that fails says so** (`failure`): the window's graphics are tried one way after another before the start gives up (`fallback`), and a start that ends
//! with no window (no graphics adapter can show it, a bad command line, an internal error) is logged, written to standard error and, where nothing would show that
//! (`diacraft.exe` has no console), told in a message box that says why, where the log is and what to try. **A graphics driver that crashes or blocks during the adapter test** does not keep
//! the next start from opening: the test is logged first, made on a thread the start gives up on, and remembered in the settings folder, so the adapter is left out (`graphics`, `marker`;
//! `--reset-graphics` tests it again).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

use std::cell::RefCell;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use diacraft_desktop::args::{self, Action, Options, USAGE, splash_enabled};
use diacraft_desktop::failure::{self, Cause};
use diacraft_desktop::fallback::{self, Climb, Rung, Step};
use diacraft_desktop::graphics::{GraphicsEnv, Memory, System};
use diacraft_desktop::marker::{self, Store};
use diacraft_desktop::{control_server, dialogs, logging, printing, startup};
use diacraft_engine::guard;
use diacraft_ui_egui::prefs_host::{Profile, WindowPrefs};
use diacraft_ui_egui::{DiacraftApp, WindowButton, WindowButtons};

/// mimalloc is the global allocator of the desktop app on every target but the browser's (owner 2026-11-21). Opening, saving and re-reading a large
/// drawing is mostly many small allocations and frees, and mimalloc does them in less CPU time than the system's allocator. Our budgets count our own
/// allocations, not the allocator's overhead, so they are unchanged. The web build never compiles it (`Cargo.toml` takes it for the other targets only).
#[cfg(not(target_arch = "wasm32"))]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// The id the desktop knows the app by (the Wayland app id, the launcher's name).
const APP_ID: &str = "app.diacraft";

/// The window's size when it opens, and the smallest it can be made, in points.
const WINDOW_SIZE: [f32; 2] = [1280.0, 800.0];
const MIN_WINDOW_SIZE: [f32; 2] = [480.0, 320.0];

/// Writes `text` where `out` goes. Not `print!`, which panics when the stream is closed (`| head -0`):
/// there is nobody left to tell then.
fn say(mut out: impl Write, text: &str) {
    let _ = out.write_all(text.as_bytes());
}

fn main() -> ExitCode {
    // Not `std::env::args`, which panics on an argument that is not valid Unicode.
    let args = std::env::args_os().skip(1).map(|a| a.to_string_lossy().into_owned());
    // Every way a start ends with no window goes through `failure::exit_with`: a panic too, wherever in the start it is (the parser, the settings, the logger, the window).
    match guard::catch_panic(|| args::parse_with(args, args::Environment::of_process())) {
        Ok(Ok(Action::Version)) => {
            say(std::io::stdout(), &format!("diacraft {}\n", env!("CARGO_PKG_VERSION")));
            ExitCode::SUCCESS
        }
        Ok(Ok(Action::Help)) => {
            say(std::io::stdout(), USAGE);
            ExitCode::SUCCESS
        }
        Ok(Ok(Action::Run(options))) => match guard::catch_panic(|| run(&options)) {
            Ok(code) => code,
            Err(message) => failure::exit_with(Cause::Panic(message), options.control_port.is_some()),
        },
        Ok(Err(e)) => failure::exit_with(Cause::Arguments(e), false),
        Err(message) => failure::exit_with(Cause::Panic(message), false),
    }
}

/// The git release and the year this build says (`build.rs`, `build_info.rs`): given to the engine for `app.version`, and to the window for its splash and About panel.
fn say_build() {
    let build = diacraft_engine::build_info::BuildInfo::new(env!("DIACRAFT_BUILD_GIT"), env!("DIACRAFT_BUILD_YEAR"));
    log::info!("DiaCraft {} ({}), {}", env!("CARGO_PKG_VERSION"), build.git, build.year);
    diacraft_engine::build_info::set(build);
}

/// The folders the control channel is kept to, made from the flags and the variables, or `None` when there are none or there is no control channel. A folder that is not there is an
/// error, and the app does not start: the alternative is a control channel that was asked to be limited and is not.
fn automation_roots(options: &Options) -> Result<Option<Arc<diacraft_engine::file_access::AutomationRoots>>, String> {
    if options.control_port.is_none() {
        return Ok(None);
    }
    options.roots.open()
}

/// What the window's creator needs, kept until a try makes the window: a try that fails before the app is built leaves it for the next.
struct Launch {
    fonts: diacraft_engine::measure::SharedFonts,
    roots: Option<Arc<diacraft_engine::file_access::AutomationRoots>>,
    control_port: Option<u16>,
    path: Option<String>,
    /// The notices the window starts with besides the control server's and the graphics': the window icon's, and the settings'.
    notes: [Option<String>; 2],
    profile: Profile<WindowPrefs>,
}

fn run(options: &Options) -> ExitCode {
    let roots = match automation_roots(options) {
        Ok(roots) => roots,
        // Only `--control` has folders to check, so a script drives this start: no box holds it up.
        Err(e) => return failure::exit_with(Cause::Folders(e), true),
    };
    start_logging();
    say_build();
    match &roots {
        Some(roots) => log::info!("the control channel is kept to the folders granted: {}", roots.describe()),
        None if options.control_port.is_some() => log::info!(
            "the control channel may reach any file you can: give --automation-read-root and --automation-write-root to keep it to folders"
        ),
        None => {}
    }
    // The person's settings: `config.toml` and the files beside it, none under `DIACRAFT_NO_PREFS` or an empty `DIACRAFT_CONFIG_DIR`. What is wrong with them is logged
    // here and shown in the status bar once the window is up; the window saves what changes, and when it closes.
    let (profile, prefs_note, settings_dir) = open_prefs();
    // The machine's fonts are read on a thread of its own from here on, and shared with the engine at once: the first frame never waits for
    // them (a cold disk takes seconds). What needs them waits, on its own thread (`startup`).
    let fonts = startup::begin();
    // Who draws the window frame: the desktop (most), or the app (GNOME's Wayland session leaves it to it).
    let choice = craft_window::Choice::probe(craft_window::Prefix::new("DIACRAFT"));
    let frame = choice.into_frame();
    log::info!("window frame: {frame:?}");
    let (viewport, icon_notice) = window_viewport();
    let launch = RefCell::new(Some(Launch {
        fonts,
        roots,
        control_port: options.control_port,
        path: options.path.clone(),
        notes: [icon_notice, prefs_note],
        profile,
    }));
    // The window's graphics are tried one way after another until a window opens (`fallback`): a try that fails before the app is built leaves `launch` as it was, and the adapter it
    // chose is left out of the next.
    let env = GraphicsEnv::read();
    let memory = Arc::new(graphics_memory(options, settings_dir));
    let rungs = fallback::ladder(System::this(), &env);
    // A panic that ends the window (a graphics driver's, say) is caught here, and told like any other start that failed: never a crash with nothing said.
    let climbed = fallback::climb(&rungs, |rung| {
        let native_options = native_options(&viewport, &frame, rung, &env, &memory);
        let outcome = guard::catch_panic(|| eframe::run_native("DiaCraft", native_options, Box::new(|cc| create_app(&launch, &frame, rung, cc))));
        let step = fallback::step_of(outcome, launch.borrow().is_none());
        if matches!(step, Step::Retry(_))
            && let Some((name, backend)) = memory.fail_chosen()
        {
            log::warn!("the window could not be made on {name} ({backend}): the next try leaves it out");
        }
        step
    });
    let scripted = options.control_port.is_some();
    match climbed {
        Climb::Opened => ExitCode::SUCCESS,
        Climb::Failed(reasons) => {
            failure::exit_with_left_out(Cause::Window(reasons), scripted, memory.left_out().iter().map(marker::describe).collect())
        }
        Climb::Panicked(message) => failure::exit_with(Cause::Panic(message), scripted),
    }
}

/// The window's description: title, size, app id and icon, and its notice when the icon did not load.
fn window_viewport() -> (egui::ViewportBuilder, Option<String>) {
    let viewport =
        egui::ViewportBuilder::default().with_title("DiaCraft").with_inner_size(WINDOW_SIZE).with_min_inner_size(MIN_WINDOW_SIZE).with_app_id(APP_ID);
    let (viewport, icon_notice) = match diacraft_ui_egui::branding::window_icon() {
        Ok(icon) => (viewport.with_icon(icon), None),
        Err(error) => {
            let message = format!("DiaCraft window icon could not load: {error}");
            log::error!("{message}");
            (viewport.with_icon(egui::IconData::default()), Some(message))
        }
    };
    let viewport = if on_wayland() { viewport.with_clamp_size_to_monitor_size(false) } else { viewport };
    (viewport, icon_notice)
}

/// eframe's options for one try: the window's `viewport` drawn as `frame` says, and the graphics of `rung` (`fallback::configure_rung`).
fn native_options(
    viewport: &egui::ViewportBuilder,
    frame: &craft_window::Frame,
    rung: Rung,
    env: &GraphicsEnv,
    memory: &Arc<Memory>,
) -> eframe::NativeOptions {
    let mut native_options = eframe::NativeOptions { viewport: craft_window::decorate(viewport.clone(), frame), ..eframe::NativeOptions::default() };
    // On macOS and Windows, the GPU that draws the window is checked to run commands first, and on macOS it is the power-saving one
    // (`graphics`). A try after the usual one chooses its own way (`fallback`).
    native_options.wgpu_options = fallback::configure_rung(native_options.wgpu_options, System::this(), env, rung, Arc::clone(memory));
    // On macOS, Quit in the app menu (Cmd+Q) and in the Dock ends the program at once unless winit is told otherwise: the window never hears
    // of it, and a drawing with unsaved changes is lost without a question. Told, winit asks the window to close, as its close button does,
    // and the window asks about unsaved changes first (`save.rs`, `watch_close`).
    #[cfg(target_os = "macos")]
    {
        native_options.event_loop_builder = Some(Box::new(|builder| {
            use winit::platform::macos::EventLoopBuilderExtMacOS;
            builder.with_close_requested_on_terminate(true);
        }));
    }
    native_options
}

/// eframe's creator: the window's app, made once the graphics are up. It takes `launch` (a try that failed before this leaves it for the next).
fn create_app(
    launch: &RefCell<Option<Launch>>,
    frame: &craft_window::Frame,
    rung: Rung,
    cc: &eframe::CreationContext<'_>,
) -> Result<Box<dyn eframe::App>, Box<dyn std::error::Error + Send + Sync>> {
    let Some(launch) = launch.borrow_mut().take() else {
        return Err("the window's start-up state was already used".into());
    };
    // The window opens on an empty new drawing, not on a screen with nothing in it, without waiting for the fonts.
    let first = startup::start(launch.fonts);
    // A busy port is a notice and an app without the channel, never an exit.
    let started = launch.control_port.map(|port| control_server::start(port, cc.egui_ctx.clone()));
    let (inbox, notice) = started.map_or((None, None), |s| (s.inbox, s.notice));
    let mut app = DiacraftApp::new(first.session, inbox)
        .with_automation_roots(launch.roots)
        .with_fonts(first.fonts)
        .with_file_picker(dialogs::open_picker())
        .with_stencil_picker(dialogs::stencil_picker())
        .with_picture_picker(dialogs::picture_picker())
        .with_save_picker(dialogs::save_picker())
        .with_export_picker(dialogs::export_picker())
        .with_print_service(Arc::new(printing::SystemPrint))
        .with_print_pdf_picker(dialogs::print_pdf_picker());
    // `diacraft FILE`: read on a thread once the window is up, so the window shows at once.
    if let Some(path) = launch.path {
        app = app.opening_at_start(path);
    }
    let [icon_notice, prefs_note] = launch.notes;
    if let Some(notice) = join_notices([notice, icon_notice, prefs_note, fallback_notice(cc, rung)]) {
        app = app.with_notice(notice);
    }
    app = app.with_prefs(launch.profile);
    if let Some(buttons) = window_buttons(frame) {
        app = app.with_window_buttons(buttons);
    }
    // The splash popup over the first frames, unless `DIACRAFT_NO_SPLASH` is set: the window, the file and the fonts load behind it.
    if splash_enabled() {
        app = app.with_start_splash();
    }
    Ok(Box::new(app))
}

/// The status bar's notice for a window drawn by a try after the usual one (`fallback::notice`), with the adapter it draws on.
fn fallback_notice(cc: &eframe::CreationContext<'_>, rung: Rung) -> Option<String> {
    let on = cc.wgpu_render_state.as_ref().map(|state| {
        let info = state.adapter.get_info();
        format!("{} ({})", info.name.trim(), info.backend)
    });
    fallback::notice(rung, on.as_deref())
}

/// What the tries of this start remember between them: the adapters earlier starts left out (`marker`), read from the settings folder `dir` (none under `DIACRAFT_NO_PREFS`, which
/// keeps nothing on the disk), after `--reset-graphics` made them forgotten when it was given. Never fails: what is wrong with the files is a line in the log.
fn graphics_memory(options: &Options, dir: Option<PathBuf>) -> Memory {
    let store = Store::new(dir, env!("CARGO_PKG_VERSION"));
    if options.reset_graphics {
        let forgotten: Vec<String> = store.reset().iter().map(marker::describe).collect();
        match (store.dir(), forgotten.is_empty()) {
            (None, _) => log::info!("--reset-graphics: no settings folder is kept for this run, so no adapter is remembered as left out"),
            (Some(_), true) => log::info!("--reset-graphics: no adapter was left out"),
            (Some(_), false) => log::info!("--reset-graphics: tested again from now on: {}", forgotten.join("; ")),
        }
    }
    let left_out = store.open();
    Memory::new(store, left_out)
}

/// Opens the person's settings and logs what is wrong with them, once. The sentence is for the status bar; the folder is where they are (`None` for a run that keeps none).
fn open_prefs() -> (Profile<WindowPrefs>, Option<String>, Option<PathBuf>) {
    let (profile, report) = Profile::<WindowPrefs>::open_from_env();
    match &report.dir {
        Some(dir) => log::info!("settings in {}", dir.display()),
        None => log::info!("settings: none are read or written ({})", report.rule),
    }
    for note in &report.notes {
        log::warn!("settings: {note}");
    }
    let sentence = report.sentence();
    (profile, sentence, report.dir)
}

/// The notices the window starts with, one sentence: those there are, in order.
fn join_notices(parts: [Option<String>; 4]) -> Option<String> {
    let parts: Vec<String> = parts.into_iter().flatten().collect();
    (!parts.is_empty()).then(|| parts.join("; "))
}

/// Whether the session is Wayland, where winit will pick the Wayland back end (unless it is told otherwise).
///
/// **Why the monitor clamp is off there (gaps U3):** eframe cuts a window's asked size to the largest monitor's size in points, and
/// works that out with the output's scale factor. Wayland tells a client a whole number for it (2 for a monitor set to 1.15, say),
/// so a 2560 by 1440 monitor reads as 1280 by 720 points and the window came up 1280 by 720, not the 1280 by 800 asked for. A
/// compositor places a window as it likes whatever size is asked; the clamp is for systems where a window larger than the monitor
/// fails (some X11 setups, Windows), and those keep it.
fn on_wayland() -> bool {
    cfg!(target_os = "linux")
        && std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty())
        && std::env::var_os("WINIT_UNIX_BACKEND").is_none_or(|v| v != "x11")
}

/// Installs the logger and, unless `DIACRAFT_NO_PREFS` is set, attaches its log file.
fn start_logging() {
    let Some(logger) = logging::install() else { return };
    match logging::log_dir().filter(|_| logging::prefs_enabled()) {
        Some(dir) => match logger.attach_dir(&dir) {
            Ok(path) => log::info!("DiaCraft {}, log file {}", env!("CARGO_PKG_VERSION"), path.display()),
            // Standard error only by now (`attach_dir` gave up on the file).
            Err(e) => log::warn!("no log file: {e}"),
        },
        None => logger.no_file(),
    }
}

/// The window buttons an app-drawn frame has, as the desktop lays them out; none when the desktop draws the frame.
fn window_buttons(frame: &craft_window::Frame) -> Option<WindowButtons> {
    let layout = frame.layout()?;
    let button = |b: &craft_window::Button| match b {
        craft_window::Button::Minimize => WindowButton::Minimize,
        craft_window::Button::Maximize => WindowButton::Maximize,
        craft_window::Button::Close => WindowButton::Close,
    };
    Some(WindowButtons { left: layout.left.iter().map(button).collect(), right: layout.right.iter().map(button).collect() })
}
