//! DiaCraft desktop app.
//!
//! Usage: `diacraft [--control <port>] [FILE]`, `diacraft --version`, `diacraft --help` (or `-h`). `FILE` is a drawing to open.
//!
//! `--control <port>` (or `DIACRAFT_CONTROL_PORT`; a port from 1 to 65535, or the app does not start) starts
//! a loopback JSON-lines control server: `{"id":1,"method":"ui.inspect","params":{}}` is answered by
//! `{"id":1,"ok":true,"result":…}`. If the port is already in use, the app logs why, says so in its status bar
//! and runs without the channel. The methods are in `docs/control-protocol.md`.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

use std::io::Write;
use std::process::ExitCode;

use diacraft_desktop::args::{self, Action, Options, USAGE, splash_enabled};
use diacraft_desktop::{control_server, dialogs, graphics, logging, printing, startup};
use diacraft_engine::guard;
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
    match args::parse(args, std::env::var_os("DIACRAFT_CONTROL_PORT")) {
        Ok(Action::Version) => {
            say(std::io::stdout(), &format!("diacraft {}\n", env!("CARGO_PKG_VERSION")));
            ExitCode::SUCCESS
        }
        Ok(Action::Help) => {
            say(std::io::stdout(), USAGE);
            ExitCode::SUCCESS
        }
        Ok(Action::Run(options)) => run(&options),
        Err(e) => {
            say(std::io::stderr(), &format!("diacraft: {e}\n"));
            ExitCode::FAILURE
        }
    }
}

/// The git release and the year this build says (`build.rs`, `build_info.rs`): given to the engine for `app.version`, and to the window for its splash and About panel.
fn say_build() {
    let build = diacraft_engine::build_info::BuildInfo::new(env!("DIACRAFT_BUILD_GIT"), env!("DIACRAFT_BUILD_YEAR"));
    log::info!("DiaCraft {} ({}), {}", env!("CARGO_PKG_VERSION"), build.git, build.year);
    diacraft_engine::build_info::set(build);
}

fn run(options: &Options) -> ExitCode {
    start_logging();
    say_build();
    // The machine's fonts are read on a thread of its own from here on, and shared with the engine at once: the first frame never waits for
    // them (a cold disk takes seconds). What needs them waits, on its own thread (`startup`).
    let fonts = startup::begin();
    // Who draws the window frame: the desktop (most), or the app (GNOME's Wayland session leaves it to it).
    let choice = craft_window::Choice::probe(craft_window::Prefix::new("DIACRAFT"));
    let frame = choice.into_frame();
    log::info!("window frame: {frame:?}");
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
    let mut native_options = eframe::NativeOptions { viewport: craft_window::decorate(viewport, &frame), ..eframe::NativeOptions::default() };
    // On macOS and Windows, the GPU that draws the window is checked to run commands first, and on macOS it is the power-saving one
    // (`graphics`).
    native_options.wgpu_options = graphics::configuration(native_options.wgpu_options);
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
    let control_port = options.control_port;
    let path = options.path.clone();
    // A panic that ends the window (a graphics driver's, say) is caught here: the app exits with an error and a
    // log line, never with a crash.
    let outcome = guard::catch_panic(|| {
        eframe::run_native(
            "DiaCraft",
            native_options,
            Box::new(move |cc| {
                // The window opens on an empty new drawing, not on a screen with nothing in it, without waiting for the fonts.
                let first = startup::start(fonts);
                // A busy port is a notice and an app without the channel, never an exit.
                let started = control_port.map(|port| control_server::start(port, cc.egui_ctx.clone()));
                let (inbox, notice) = started.map_or((None, None), |s| (s.inbox, s.notice));
                let mut app = DiacraftApp::new(first.session, inbox)
                    .with_fonts(first.fonts)
                    .with_file_picker(dialogs::open_picker())
                    .with_stencil_picker(dialogs::stencil_picker())
                    .with_picture_picker(dialogs::picture_picker())
                    .with_save_picker(dialogs::save_picker())
                    .with_export_picker(dialogs::export_picker())
                    .with_print_service(std::sync::Arc::new(printing::SystemPrint))
                    .with_print_pdf_picker(dialogs::print_pdf_picker());
                // `diacraft FILE`: read on a thread once the window is up, so the window shows at once.
                if let Some(path) = path {
                    app = app.opening_at_start(path);
                }
                let notice = match (notice, icon_notice) {
                    (Some(control), Some(icon)) => Some(format!("{control}; {icon}")),
                    (control, icon) => control.or(icon),
                };
                if let Some(notice) = notice {
                    app = app.with_notice(notice);
                }
                if let Some(buttons) = window_buttons(&frame) {
                    app = app.with_window_buttons(buttons);
                }
                // The splash popup over the first frames, unless `DIACRAFT_NO_SPLASH` is set: the window, the file and the fonts load behind it.
                if splash_enabled() {
                    app = app.with_start_splash();
                }
                Ok(Box::new(app))
            }),
        )
    });
    match outcome {
        Ok(Ok(())) => ExitCode::SUCCESS,
        Ok(Err(e)) => {
            log::error!("the window could not run: {e}");
            say(std::io::stderr(), &format!("diacraft: the window could not run: {e}\n"));
            ExitCode::FAILURE
        }
        Err(msg) => {
            log::error!("the window stopped on a panic: {msg}");
            say(std::io::stderr(), "diacraft: the window stopped on an internal error (see the log); please report this bug\n");
            ExitCode::FAILURE
        }
    }
}

/// Whether the session is Wayland, where winit will pick the Wayland back end (unless it is told otherwise).
///
/// **Why the monitor clamp is off there (gaps U3):** eframe cuts a window's asked size to the largest monitor's size in points, and
/// works that out with the output's scale factor. Wayland tells a client a whole number for it (2 for a monitor set to 1.15, say),
/// so a 2560 by 1440 monitor reads as 1280 by 720 points and the window came up 1280 by 720, not the 1280 by 800 asked for. A
/// compositor places a window as it likes whatever size is asked; the clamp is for systems where a window larger than the monitor
/// fails (some X11 setups, Windows), and those keep it.
fn on_wayland() -> bool {
    cfg!(any(target_os = "linux", target_os = "freebsd", target_os = "netbsd", target_os = "openbsd"))
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
