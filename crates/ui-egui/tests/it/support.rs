//! Helpers the test modules share: a window on a fake inbox, requests and their answers, a scratch folder, and the one way a test lets
//! a window come to rest ([`Rest::rest`]).

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use diacraft_engine::Session;
use diacraft_ui_egui::clock::Clock;
use diacraft_ui_egui::render_worker::bundled_drawer;
use diacraft_ui_egui::snap_reader::SnapReader;
use diacraft_ui_egui::{ControlRequest, DiacraftApp};
use egui::{Vec2, ViewportCommand, ViewportId};
use egui_kittest::Harness;
use serde_json::Value;

pub type Window = Harness<'static, DiacraftApp>;
pub type Answer = Receiver<Result<Value, String>>;

/// The app with a clock that stands still until the test advances it ([`Rest::advance`]), unless the test gave it a clock of its own. **Every window of the
/// tests has one**: the window's timed work (a picture drawn again at rest, a failed drawing asked for again, an edit's outline kept while the picture
/// catches up) then happens when the test lets time pass and not when the machine is slow (`clock.rs`).
fn with_test_clock(app: DiacraftApp) -> DiacraftApp {
    if app.clock().is_manual() { app } else { app.with_clock(test_clock()) }
}

/// The app with the index of a page's shapes made ready for a drag on the test's thread, inside the frame that asks (`snap_reader.rs`): a thread would answer in whatever
/// frame the machine got to it, and a drag's first frames would snap or not by the load. A test of the reader's thread gives its own ([`DiacraftApp::with_snap_reader`]).
fn with_inline_reads(app: DiacraftApp) -> DiacraftApp {
    if app.snap_reader_is_set() { app } else { app.with_snap_reader(SnapReader::inline()) }
}

/// The clock a test window gets: a manual one, unless `DIACRAFT_TEST_CLOCK` asks for another to see what a slow machine does (never set in CI): `real` is the
/// machine's own clock, `tick:N` is a clock that goes N ms further each time the window reads it (once a frame), so that every frame takes N ms of the
/// window's time and its timers run out in the middle of a test, on any machine. **The tests must pass with `manual` (the default) on any machine at any
/// load**; with the others a test that depends on the window's timers not running out fails, which is how the tests were found that did.
fn test_clock() -> Clock {
    match std::env::var("DIACRAFT_TEST_CLOCK").ok().as_deref() {
        Some("real") => Clock::real(),
        Some(other) => match other.strip_prefix("tick:").and_then(|n| n.parse::<u64>().ok()) {
            Some(ms) => Clock::ticking(Duration::from_millis(ms)),
            None => Clock::manual(),
        },
        None => Clock::manual(),
    }
}

/// A window of `size` points with no control channel.
pub fn window(app: DiacraftApp, size: impl Into<Vec2>) -> Window {
    Harness::builder().with_size(size).build_eframe(|_| with_inline_reads(with_test_clock(app)))
}

/// A window of 900 × 600 points whose control inbox the test feeds through the returned sender.
pub fn controlled() -> (Window, Sender<ControlRequest>) {
    controlled_with(|app| app)
}

/// Like [`controlled`], with `build` giving the app its options. **Pictures are drawn inline** (on the test's thread, in the frame that
/// asked, over the bundled fonts) unless `build` chooses otherwise: a render thread would answer in whatever frame the machine got to it,
/// and would scan the machine's fonts first.
pub fn controlled_with(build: impl FnOnce(DiacraftApp) -> DiacraftApp) -> (Window, Sender<ControlRequest>) {
    let (tx, rx) = channel();
    (window(build(DiacraftApp::new(Session::new(), Some(rx)).with_inline_pictures(bundled_drawer())), [900.0, 600.0]), tx)
}

/// Queues a request and returns where its answer will arrive.
pub fn send(tx: &Sender<ControlRequest>, method: &str, params: Value) -> Answer {
    let (request, answer) = ControlRequest::new(method, params);
    tx.send(request).expect("the window keeps its inbox");
    answer
}

/// Runs one request to its answer: frames until it arrives, at most 20.
pub fn call(window: &mut Window, tx: &Sender<ControlRequest>, method: &str, params: Value) -> Result<Value, String> {
    let answer = send(tx, method, params);
    for _ in 0..20 {
        window.step();
        if let Ok(reply) = answer.try_recv() {
            return reply;
        }
    }
    panic!("`{method}` was not answered in 20 frames")
}

/// The viewport commands the last frame sent.
pub fn commands(window: &Window) -> Vec<ViewportCommand> {
    window.output().viewport_output.get(&ViewportId::ROOT).map(|v| v.commands.clone()).unwrap_or_default()
}

/// A scratch folder under the temporary directory, removed with it. Names are unique per process and call.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new(what: &str) -> TempDir {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("diacraft-ui-{what}-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    pub fn file(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

/// Ends the test process if the test it lives in is still running after `secs` seconds. For a test of code that
/// must not block (a write to a pipe nobody reads blocks for good): the failure is then a message and an exit
/// code, not a test run that never ends.
pub struct Watchdog(std::sync::Arc<std::sync::atomic::AtomicBool>);

impl Watchdog {
    pub fn new(what: &'static str, secs: u64) -> Watchdog {
        let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let watching = std::sync::Arc::clone(&done);
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(secs));
            if !watching.load(std::sync::atomic::Ordering::SeqCst) {
                eprintln!("watchdog: {what} did not finish in {secs} s; it is blocked");
                std::process::exit(101);
            }
        });
        Watchdog(done)
    }
}

impl Drop for Watchdog {
    fn drop(&mut self) {
        self.0.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

/// A window whose frames are 1/60 s apart, so two clicks in a row are a double click.
pub fn fast_window(app: DiacraftApp, size: impl Into<Vec2>) -> Window {
    Harness::builder().with_size(size).with_step_dt(1.0 / 60.0).build_eframe(|_| with_inline_reads(with_test_clock(app)))
}

/// As [`fast_window`], with `renderer` given the textures the window uploads (a test that reads the pixels of a picture the window shows).
pub fn fast_window_with(app: DiacraftApp, size: impl Into<Vec2>, renderer: impl egui_kittest::TestRenderer + 'static) -> Window {
    Harness::builder().with_size(size).with_step_dt(1.0 / 60.0).renderer(renderer).build_eframe(|_| with_inline_reads(with_test_clock(app)))
}

/// Runs one frame with `event`, and returns the viewport commands it sent.
pub fn input(window: &mut Window, event: egui::Event) -> Vec<ViewportCommand> {
    window.input_mut().events.push(event);
    window.step();
    commands(window)
}

pub fn move_to(window: &mut Window, pos: egui::Pos2) -> Vec<ViewportCommand> {
    input(window, egui::Event::PointerMoved(pos))
}

pub fn button(window: &mut Window, pos: egui::Pos2, pressed: bool) -> Vec<ViewportCommand> {
    input(window, egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE })
}

/// Presses at `from`, drags to `from + by` in steps and releases there; every viewport command sent on the way.
pub fn drag(window: &mut Window, from: egui::Pos2, by: Vec2) -> Vec<ViewportCommand> {
    // Hover first, for a few frames, so egui knows what is under the pointer before the press.
    let mut sent = Vec::new();
    for _ in 0..3 {
        sent.extend(move_to(window, from));
    }
    sent.extend(button(window, from, true));
    for step in 1..=5 {
        sent.extend(move_to(window, from + by * (step as f32 / 5.0)));
    }
    sent.extend(button(window, from + by, false));
    sent
}

/// A click: press and release at `pos`.
pub fn click(window: &mut Window, pos: egui::Pos2) -> Vec<ViewportCommand> {
    let mut sent = Vec::new();
    for _ in 0..3 {
        sent.extend(move_to(window, pos));
    }
    sent.extend(button(window, pos, true));
    sent.extend(button(window, pos, false));
    sent
}

/// A window of `size` points on a new drawing (pictures inline, as in [`controlled_with`]), drawn until it settles.
pub fn drawing_window(size: impl Into<Vec2>) -> Window {
    let mut session = Session::new();
    session.execute("file.new", &serde_json::json!({})).unwrap();
    let mut window = window(DiacraftApp::new(session, None).with_inline_pictures(bundled_drawer()), size);
    window.rest();
    window
}

/// Runs the command `id` as the window would, then draws until the window settles. The command must succeed.
pub fn run_command(window: &mut Window, id: &str, params: Value) {
    window.state_mut().execute(id, &params).unwrap_or_else(|e| panic!("`{id}` failed: {e}"));
    window.rest();
}

/// How long [`Rest::rest`] waits for the window's other threads before it says they are stuck.
const WORK_BOUND: Duration = Duration::from_secs(30);

/// Whether a thread the window started can still answer: a dialog (Open, Save, Export), a file being read, a picture being exported, a
/// picture being drawn off the window's thread. Each answer is taken, and a frame asked for, whenever its thread gets to it.
pub fn working(window: &Window) -> bool {
    let app = window.state();
    app.save_dialog_open()
        || app.open_dialog_open()
        || app.stencil_dialog_open()
        || app.picture_dialog_open()
        || app.opening_now().is_some()
        || app.export_dialog_open()
        || app.exporting_now()
        || app.fonts_scanning()
        || app.snap_reading()
        || app.print_working()
        || app.picture_state()["busy"].as_bool().unwrap_or(false)
}

/// How a test lets a window come to rest.
///
/// **Use this and never `Harness::run` on a window.** `run` ends once a frame asks for nothing at once, and fails when more than four
/// frames in a row do. A thread that answers in the middle of those frames adds the frames its answer takes (a saved drawing sets its
/// notice, its title and its picture: three or four frames), so on a machine busy enough to delay the thread, `run` fails with "exceeded
/// max_steps" for a window that works as it should. A key that starts the Save dialog passed on a machine with 32 cores and failed on
/// GitHub's.
pub trait Rest {
    /// Draws a frame (so that a thread this frame starts is seen), then frames with a sleep between them **while a thread of the window has an answer
    /// still to give** ([`working`]; thirty seconds at the longest), and then runs the window to rest as `Harness::run` does, with the same bound on frames that ask for another at once:
    /// a window that repaints for ever still fails here. A failure points at the line of the test that asked.
    #[track_caller]
    fn rest(&mut self) -> u64;

    /// Runs the window to rest **without waiting for its threads**: for a test that holds one on purpose (a dialog it has not answered,
    /// which asks for a frame only every quarter of a second). It has the same bound on frames that ask for another at once.
    #[track_caller]
    fn rest_holding(&mut self) -> u64;

    /// Lets `by` of the window's own time pass: the window's clock is the test's (`clock.rs`) and stands still until this moves it. A picture drawn
    /// while the view moved is drawn again in full once the view has stood still for `REST_AFTER`: `advance` that, then `rest`.
    fn advance(&mut self, by: Duration);
}

impl Rest for Window {
    #[track_caller]
    fn rest(&mut self) -> u64 {
        let start = Instant::now();
        // A frame first: a thread that a frame starts (the font scan the Text dialog starts as it is first drawn, a dialog the key of the test has just asked
        // for) is not there for `working` to see until that frame has been drawn, and a thread that is first seen inside `run`'s frames answers in the middle
        // of them and adds its frames to the four `run` allows (GitHub run 42398195820). The frame is part of the rest: `run` draws at least one more.
        self.step();
        while working(self) {
            assert!(start.elapsed() < WORK_BOUND, "a dialog, a file being read, an export or a picture did not finish in {WORK_BOUND:?}");
            self.step();
            std::thread::sleep(Duration::from_millis(2));
        }
        self.run()
    }

    #[track_caller]
    fn rest_holding(&mut self) -> u64 {
        self.run()
    }

    fn advance(&mut self, by: Duration) {
        self.state().clock().advance(by);
    }
}

/// Clicks the button named `name`, and gives the two frames its answer needs. Not [`Rest::rest`]: the click may start a dialog or a file
/// being read, and the test then waits for it by itself.
pub fn press(w: &mut Window, name: &str) {
    let at = w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).rect.center();
    click(w, at);
    w.step();
    w.step();
}

/// The controls the last frame drew whose names start with `prefix`: name, where, and whether each can be used.
pub fn elements_named(window: &Window, prefix: &str) -> Vec<(String, egui::Rect, bool)> {
    window.state().elements().iter().filter(|e| e.name.starts_with(prefix)).map(|e| (e.name.clone(), e.rect, e.enabled)).collect()
}
