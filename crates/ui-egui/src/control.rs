//! The window's side of the control channel: the requests the transport hands over, and what each method does.
//!
//! The desktop app runs a loopback TCP server (`apps/diacraft`, `docs/control-protocol.md`). It turns each
//! line a client sends into a [`ControlRequest`] on a channel, the [`ControlInbox`], and waits for the
//! answer. The window drains the inbox once per frame, at most [`MAX_REQUESTS_PER_FRAME`] requests, and
//! answers each one under the panic guard, so a bug in one request is an error reply and not a crash.
//!
//! Methods ([`METHODS`]):
//!
//! - `engine.execute {command, params?}` runs a command, as a menu would. The command's own result is the reply. `file.open` reads
//!   the file on a thread and answers when it is read (or refused), so the window stays alive while a large file loads; so does `file.new` with a
//!   template that is a file.
//! - `engine.commands` lists every command with whether it can run now and, if not, why.
//! - `document.inspect` is the command of that name: the open document.
//! - `ui.inspect` describes the window: `{app, version, frames, window, revision, status, document, notice, panics, ...}`.
//! - `ui.screenshot {path}` writes the next frame the window presents to `path` as a PNG, and answers
//!   `{path, width, height}`. If no frame is presented in time (a locked screen, a window nobody can
//!   see), the answer is an error and no file is written.
//! - `app.quit` answers, then closes the window a moment later, so the answer leaves before the process does.
//! - `ui.elements {prefix?}` lists the controls the last frame drew, by name, with where each is and whether it can be used.
//! - `ui.resize {width, height}` asks the desktop to make the window that size (in points), to see a layout at another size.
//! - `ui.click`, `ui.drag`, `ui.release`, `ui.move`, `ui.wheel`, `ui.key` and `ui.text` give egui pointer and keyboard input, as a person at
//!   the window would (see [`input`]). Only this window gets it: nothing is sent to the desktop.
//!
//! `engine.execute` and `engine.commands` cover the window's own commands (zoom, panels, tabs: [`crate::commands`]) as well as
//! the engine's.
//!
//! Every method takes a fixed set of keys. One it does not know is an error, not a default.

use std::borrow::Cow;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use diacraft_engine::file_access::{self, Access, ScopeHandle};
use diacraft_engine::{guard, shown};
use egui::ColorImage;
use serde_json::{Map, Value, json};

use crate::app::{DiacraftApp, Stage};

mod input;
#[cfg(test)]
mod tests;

/// Most requests the window answers in one frame. Later ones stay on the channel and the window asks for
/// another frame to serve them, so a flood of requests slows the control channel, never the window.
pub const MAX_REQUESTS_PER_FRAME: usize = 64;

/// Most requests looked at in one frame, answered or not: requests that their caller withdrew cost the
/// frame nothing, but not an unbounded amount of looking either.
const MAX_POLLED_PER_FRAME: usize = 4 * MAX_REQUESTS_PER_FRAME;

/// Longest error the window itself makes for a panic, in characters of its escaped text.
const MAX_REPLY_ERROR_CHARS: usize = 400;

/// The methods, in the order the protocol documents them.
pub const METHODS: &[&str] = &[
    "engine.execute",
    "engine.commands",
    "document.inspect",
    "ui.inspect",
    "ui.screenshot",
    "ui.elements",
    "ui.click",
    "ui.drag",
    "ui.release",
    "ui.move",
    "ui.wheel",
    "ui.key",
    "ui.text",
    "ui.resize",
    "app.quit",
];

/// How long a screenshot waits for the integration to present a frame, once asked for.
pub const DEFAULT_SCREENSHOT_TIMEOUT: Duration = Duration::from_secs(8);

/// How long after `app.quit` the window closes: time for the answer to get out of the process.
pub const QUIT_DELAY: Duration = Duration::from_millis(150);

/// The largest screenshot the window takes, in bytes of RGBA pixels (a 4K window at 2x is about 127 MiB).
const MAX_SCREENSHOT_BYTES: usize = 128 * 1024 * 1024;

/// What the window says of a screenshot no frame came for.
const NO_FRAME: &str = "no frame was presented in time, so there is no screenshot (the screen may be locked, or the window minimized or covered)";

/// An answer: the method's result, or the reason it did not work.
pub type ControlReply = Result<Value, String>;

/// A request from the control channel, with where to answer it.
pub struct ControlRequest {
    pub method: String,
    pub params: Value,
    pub reply: Sender<ControlReply>,
    state: Arc<AtomicU8>,
}

/// Where a request is: waiting, taken by the window, or given up on by its caller before that. It leaves
/// `QUEUED` once, one way or the other.
const QUEUED: u8 = 0;
const STARTED: u8 = 1;
const GIVEN_UP: u8 = 2;

/// The queue the window drains.
pub type ControlInbox = Receiver<ControlRequest>;

/// The caller's side of a queued request.
pub struct Ticket(Arc<AtomicU8>);

impl Ticket {
    /// Stops waiting for the request. `true`: the window had not taken it, so it never runs, and the caller
    /// can say "timeout, it will not run" and retry without the work being done twice. `false`: the window
    /// has taken it (or has finished it); what came of it is not known to the caller.
    pub fn give_up(&self) -> bool {
        self.0.compare_exchange(QUEUED, GIVEN_UP, Ordering::SeqCst, Ordering::SeqCst).is_ok()
    }
}

impl ControlRequest {
    /// A request, and the receiver its answer arrives on.
    pub fn new(method: impl Into<String>, params: Value) -> (ControlRequest, Receiver<ControlReply>) {
        let (reply, answer) = channel();
        (ControlRequest { method: method.into(), params, reply, state: Arc::new(AtomicU8::new(QUEUED)) }, answer)
    }

    /// What the caller keeps to withdraw the request with.
    pub fn ticket(&self) -> Ticket {
        Ticket(Arc::clone(&self.state))
    }

    /// The window takes the request to run it. `false`: its caller withdrew it first, so it must not run.
    pub fn claim(&self) -> bool {
        self.state.compare_exchange(QUEUED, STARTED, Ordering::SeqCst, Ordering::SeqCst).is_ok()
    }
}

/// What a handled request leaves: an answer now, or an answer the window will send later.
enum Outcome {
    Reply(ControlReply),
    Deferred,
}

/// A screenshot asked for and not yet answered. At most one at a time.
pub(crate) struct Shot {
    path: std::path::PathBuf,
    reply: Sender<ControlReply>,
    /// When to give up, as a reading of the window's clock (`clock.rs`). `None`: never (a timeout so long that the time it ends at is not a time at all).
    deadline: Option<Duration>,
    /// Where the integration's callback leaves the frame.
    slot: Arc<Mutex<Option<Arc<ColorImage>>>>,
    /// The roots in force when it was asked for: the file is written in a later frame, and is judged again then, under the roots the request had.
    scope: ScopeHandle,
}

impl DiacraftApp {
    /// Answers the requests waiting in the inbox: at most [`MAX_REQUESTS_PER_FRAME`].
    pub(crate) fn drain_control(&mut self, ctx: &egui::Context) {
        let Some(inbox) = self.control.take() else { return };
        let (mut handled, mut polled, mut closed) = (0, 0, false);
        while handled < MAX_REQUESTS_PER_FRAME && polled < MAX_POLLED_PER_FRAME {
            match inbox.try_recv() {
                Ok(request) => {
                    polled += 1;
                    // Its caller stopped waiting and was told it will not run: an edit now would come from nowhere.
                    if !request.claim() {
                        continue;
                    }
                    handled += 1;
                    self.answer(ctx, request);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    closed = true;
                    break;
                }
            }
        }
        if handled >= MAX_REQUESTS_PER_FRAME || polled >= MAX_POLLED_PER_FRAME {
            // There may be more waiting: another frame serves them.
            ctx.request_repaint();
        }
        if closed {
            log::warn!("the control channel closed: no more control requests");
        } else {
            self.control = Some(inbox);
        }
    }

    /// Runs one request under the panic guard and sends its answer. A panic is an error reply, and the
    /// window goes on.
    fn answer(&mut self, ctx: &egui::Context, request: ControlRequest) {
        let reply = request.reply.clone();
        // The roots (`--automation-read-root`, `--automation-write-root`) are in force for this request and for the threads it starts, and for nothing else the window does:
        // what the person does in the window (its dialogs, a file named on the command line, a drop) is not a request, and is not kept to them.
        let roots = self.automation_roots.clone();
        let result = match guard::catch_panic(|| file_access::confine(roots.as_ref(), || self.handle(ctx, &request))) {
            Ok(Outcome::Reply(result)) => result,
            // The window kept the sender and will answer.
            Ok(Outcome::Deferred) => return,
            Err(msg) => {
                self.ui_panics += 1;
                // The panic message is raw: escaped once for the window, and once for the reply a client prints.
                let raw = format!("internal error in the control channel: {msg} (please report this bug)");
                self.set_notice(&raw);
                Err(shown(&raw, MAX_REPLY_ERROR_CHARS))
            }
        };
        // The caller may have gone away meanwhile; there is nobody to tell.
        let _ = reply.send(result);
    }

    fn handle(&mut self, ctx: &egui::Context, request: &ControlRequest) -> Outcome {
        self.run_probe(Stage::Request(&request.method));
        let method = request.method.as_str();
        if !METHODS.contains(&method) {
            return Outcome::Reply(Err(format!("unknown method `{}`; the methods are: {}", shown(method, 64), METHODS.join(", "))));
        }
        let params = match params_object(&request.params) {
            Ok(params) => params,
            Err(e) => return Outcome::Reply(Err(format!("`{method}`: {e}"))),
        };
        let reply = match method {
            "engine.execute" => return self.method_execute(ctx, request, &params),
            "engine.commands" => only_keys(&params, method, &[]).and_then(|()| self.method_commands()),
            "document.inspect" => self.method_document(&request.params),
            "ui.inspect" => only_keys(&params, method, &[]).map(|()| self.inspect(ctx)),
            "ui.screenshot" => return self.method_screenshot(ctx, request, &params),
            "ui.elements" => self.method_elements(&params),
            "ui.click" | "ui.drag" | "ui.release" | "ui.move" | "ui.wheel" | "ui.key" | "ui.text" => self.method_input(ctx, method, &params),
            "ui.resize" => self.method_resize(ctx, &params),
            "app.quit" => only_keys(&params, method, &[]).map(|()| self.method_quit(ctx)),
            // `METHODS` was checked above; a method listed there and not handled here is a bug in this match.
            other => Err(format!("method `{}` is listed but not handled (please report this bug)", shown(other, 64))),
        };
        Outcome::Reply(reply)
    }

    /// `engine.execute`. `file.open`, and `file.new` with a template that is a file, are read on a thread and answered when the reading ends, so a large file
    /// never holds the window; any other command is run and answered now.
    fn method_execute(&mut self, ctx: &egui::Context, request: &ControlRequest, params: &Map<String, Value>) -> Outcome {
        let (command, command_params) = match execute_args(params) {
            Ok(args) => args,
            Err(e) => return Outcome::Reply(Err(e)),
        };
        if command == "file.open" {
            return match self.control_open(ctx, &command_params, request.reply.clone()) {
                Ok(()) => Outcome::Deferred,
                Err(e) => Outcome::Reply(Err(e)),
            };
        }
        // A template that is a file is read on a thread too, as a file to open is: a built-in template (an id) is small and made at once, and a word that is
        // neither an id nor written as a path (a typo) is answered at once, by the engine, as the refusal it is (`template::is_path` is the engine's own rule).
        if command == "file.new"
            && let Some(key) = command_params.get("template").and_then(Value::as_str)
            && diacraft_engine::template::find(key).is_none()
            && diacraft_engine::template::is_path(key)
        {
            return match self.control_new_from_file(ctx, &command_params, request.reply.clone()) {
                Ok(()) => Outcome::Deferred,
                Err(e) => Outcome::Reply(Err(e)),
            };
        }
        Outcome::Reply(self.execute(command, &command_params).map_err(|e| e.to_string()))
    }

    /// The engine's commands, then the window's, each with whether it can run now.
    fn method_commands(&self) -> ControlReply {
        let mut list = serde_json::to_value(self.session().commands()).map_err(|e| format!("cannot encode the command list: {e}"))?;
        if let Value::Array(items) = &mut list {
            for c in crate::commands::UI_COMMANDS {
                let state = self.window_command_enabled(c.id);
                let mut entry = json!({
                    "id": c.id,
                    "label": c.label,
                    "menu": c.menu,
                    "shortcut": c.shortcut,
                    "params": c.params,
                    "enabled": state.is_ok(),
                });
                if let (Err(reason), Some(map)) = (state, entry.as_object_mut()) {
                    map.insert("disabled_reason".to_owned(), Value::String(reason));
                }
                items.push(entry);
            }
        }
        Ok(list)
    }

    /// `document.inspect` is a command like any other; its own parameter check reports a key it does not take.
    fn method_document(&mut self, params: &Value) -> ControlReply {
        self.execute("document.inspect", params).map_err(|e| e.to_string())
    }

    fn method_quit(&mut self, ctx: &egui::Context) -> Value {
        if self.quit_at.is_none() && !self.quit_sent {
            // By the window's clock, read where the request is taken: the delay is counted from here, and a test's clock, which stands still until the
            // test advances it, decides when it ends (`clock.rs`).
            self.quit_at = Some(self.clock.now().saturating_add(QUIT_DELAY));
        }
        ctx.request_repaint_after(QUIT_DELAY);
        Value::Null
    }

    /// Closes the window once `app.quit` has had its moment to answer: when the window's clock has gone `QUIT_DELAY` past the request.
    pub(crate) fn advance_quit(&mut self, ctx: &egui::Context) {
        let Some(at) = self.quit_at else { return };
        let now = self.clock.now();
        if now >= at {
            self.quit_at = None;
            self.quit_sent = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else {
            // The machine's own time wakes the window for the next look: what is left of the delay by the window's clock.
            ctx.request_repaint_after(at.saturating_sub(now));
        }
    }

    /// The `ui.inspect` reply.
    fn inspect(&self, ctx: &egui::Context) -> Value {
        let size = ctx.content_rect().size();
        let document = self.session().document().map(|d| json!({"title": d.title, "pages": d.pages.len()}));
        json!({
            "app": "diacraft",
            "version": env!("CARGO_PKG_VERSION"),
            "frames": self.frames,
            "window": [size.x, size.y],
            "revision": self.session().revision(),
            "status": self.status_text(),
            "title": self.window_title(),
            "ui": self.window_state(),
            "document": document,
            "notice": self.notice(),
            "opening": self.opening_now().map(|(file, seconds)| json!({"file": file, "seconds": seconds})),
            "openError": self.open_error(),
            "saveNote": self.save_note_state(),
            "picture": self.picture_state(),
            "findings": self.findings_summary(),
            "panics": {"engine": self.session().panics_caught(), "ui": self.ui_panics},
            "screenshotPending": self.screenshot.is_some(),
            // The folders the control channel may read and write, as `{read, write}`; null when it is kept to none (any path the person can reach).
            "automation": self.automation_roots.as_ref().map(|roots| roots.to_json()),
            "quitRequested": self.quit_requested(),
        })
    }

    fn method_screenshot(&mut self, ctx: &egui::Context, request: &ControlRequest, params: &Map<String, Value>) -> Outcome {
        let checked = only_keys(params, "ui.screenshot", &["path"])
            .and_then(|()| screenshot_path(params.get("path")))
            .and_then(|path| if self.screenshot.is_some() { Err(SHOT_BUSY.to_owned()) } else { Ok(path) })
            .and_then(|path| window_bytes(ctx).map(|_| path));
        match checked {
            Err(e) => Outcome::Reply(Err(e)),
            Ok(path) => {
                let slot: Arc<Mutex<Option<Arc<ColorImage>>>> = Arc::default();
                // Asked for now, in the pass that applied the requests before it: the next frame painted is
                // the one with their effects in it.
                let (arrives, repaint) = (Arc::clone(&slot), ctx.clone());
                ctx.request_screenshot(move |image| {
                    *arrives.lock().unwrap_or_else(PoisonError::into_inner) = Some(image);
                    repaint.request_repaint();
                });
                let deadline = self.clock.now().checked_add(self.screenshot_timeout);
                self.screenshot = Some(Shot { path, reply: request.reply.clone(), deadline, slot, scope: ScopeHandle::capture() });
                ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaintWhileHidden);
                ctx.request_repaint();
                Outcome::Deferred
            }
        }
    }

    /// Moves a pending screenshot along: writes the frame when the integration has delivered it, and gives up
    /// with an error when the time is up.
    pub(crate) fn advance_screenshot(&mut self, ctx: &egui::Context) {
        let Some(shot) = self.screenshot.take() else { return };
        let arrived = shot.slot.lock().unwrap_or_else(PoisonError::into_inner).take();
        if let Some(image) = arrived {
            // The reply carries the outcome of the write; the caller may have gone, with nobody to tell.
            let (path, scope) = (shot.path.clone(), shot.scope.clone());
            let _ = shot.reply.send(scope.run(|| write_png(&path, &image)));
            return;
        }
        if shot.deadline.is_some_and(|deadline| self.clock.now() >= deadline) {
            let _ = shot.reply.send(Err(NO_FRAME.to_owned()));
            return;
        }
        // A window nobody can see runs no frame, so none would be painted to capture: a pending screenshot asks for
        // the frame anyway (for one frame at a time, so it is asked again on every pass).
        ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaintWhileHidden);
        // Come back soon: the frame, or the deadline, whichever is first.
        ctx.request_repaint_after(Duration::from_millis(50));
        self.screenshot = Some(shot);
    }
}

const SHOT_BUSY: &str = "a screenshot is already being taken; wait for its answer before asking for another";

/// The request's `params`: an object, with `null` or none as `{}`. An object is read where it is, not copied: the params of a
/// request can be megabytes, and this runs on the thread the window is drawn by.
pub(crate) fn params_object(params: &Value) -> Result<Cow<'_, Map<String, Value>>, String> {
    match params {
        Value::Null => Ok(Cow::Owned(Map::new())),
        Value::Object(map) => Ok(Cow::Borrowed(map)),
        _ => Err("params must be a JSON object".to_owned()),
    }
}

/// What `engine.execute` runs: the command's id and its params (`{}` when none were given), the params borrowed from the
/// request as they are.
pub(crate) fn execute_args(params: &Map<String, Value>) -> Result<(&str, Cow<'_, Value>), String> {
    only_keys(params, "engine.execute", &["command", "params"])?;
    let Some(Value::String(command)) = params.get("command") else {
        return Err("`engine.execute` needs a string `command`: the id of the command to run".to_owned());
    };
    let command_params = match params.get("params") {
        None | Some(Value::Null) => Cow::Owned(Value::Object(Map::new())),
        Some(object @ Value::Object(_)) => Cow::Borrowed(object),
        Some(_) => return Err("`engine.execute`: `params` must be an object".to_owned()),
    };
    Ok((command, command_params))
}

/// Fails on a key the method does not take, and says what it takes.
fn only_keys(params: &Map<String, Value>, method: &str, allowed: &[&str]) -> Result<(), String> {
    match params.keys().find(|k| !allowed.contains(&k.as_str())) {
        None => Ok(()),
        Some(key) if allowed.is_empty() => Err(format!("unknown key `{}`; `{method}` takes no parameters", shown(key, 64))),
        Some(key) => Err(format!("unknown key `{}`; `{method}` takes: {}", shown(key, 64), allowed.join(", "))),
    }
}

/// The `path` of `ui.screenshot`: given in full, a `.png`, in a folder that exists. It is the client's, so it is
/// never echoed back.
fn screenshot_path(value: Option<&Value>) -> Result<std::path::PathBuf, String> {
    let Some(Value::String(text)) = value else {
        return Err("`ui.screenshot` needs a string `path`: where to write the PNG".to_owned());
    };
    if text.is_empty() {
        return Err("`path` is empty".to_owned());
    }
    if text.chars().any(char::is_control) {
        return Err("`path` must not contain control characters".to_owned());
    }
    let path = std::path::Path::new(text);
    if !path.is_absolute() {
        return Err("`path` must be an absolute path".to_owned());
    }
    if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("png")) {
        return Err("`path` must name a .png file".to_owned());
    }
    // Roots in force: a path outside the write roots is refused before the file system is asked anything about it (whether its folder is there, what stands at it).
    file_access::allow(path, Access::Write).map_err(|why| format!("the screenshot was not taken: {why}"))?;
    if !path.parent().is_some_and(std::path::Path::is_dir) {
        return Err("the folder of `path` does not exist".to_owned());
    }
    // Something that is there and is not a plain file (a named pipe, whose open waits for a reader; a folder; a
    // link to somewhere else) is not what a screenshot replaces. Checked now, so that no frame is captured for it.
    if std::fs::symlink_metadata(path).is_ok_and(|m| !m.file_type().is_file()) {
        return Err("`path` exists and is not a regular file".to_owned());
    }
    Ok(path.to_path_buf())
}

/// The bytes of RGBA pixels a capture of the window would take, if that is a size the window will take.
fn window_bytes(ctx: &egui::Context) -> Result<usize, String> {
    let size = ctx.content_rect().size();
    let scale = ctx.pixels_per_point();
    let side = |points: f32| -> Option<usize> {
        let pixels = f64::from(points) * f64::from(scale);
        // `as usize` would turn NaN into 0 and clamp the rest: only a plain finite size is cast.
        (pixels.is_finite() && (1.0..=100_000.0).contains(&pixels)).then(|| pixels.round() as usize)
    };
    let (Some(w), Some(h)) = (side(size.x), side(size.y)) else {
        return Err("the window has no size a screenshot can be taken of".to_owned());
    };
    match w.checked_mul(h).and_then(|n| n.checked_mul(4)) {
        Some(bytes) if bytes <= MAX_SCREENSHOT_BYTES => Ok(bytes),
        _ => Err(format!("a {w} by {h} pixel screenshot is over the limit of {MAX_SCREENSHOT_BYTES} bytes")),
    }
}

/// Writes `image` to `path` as an 8-bit RGBA PNG → `{path, width, height}`.
fn write_png(path: &std::path::Path, image: &ColorImage) -> ControlReply {
    let [width, height] = image.size;
    let bytes = width.checked_mul(height).and_then(|n| n.checked_mul(4)).filter(|b| *b <= MAX_SCREENSHOT_BYTES);
    let (Some(bytes), Ok(w32), Ok(h32)) = (bytes, u32::try_from(width), u32::try_from(height)) else {
        return Err(format!("the frame is {width} by {height} pixels, which is not a size a screenshot can have"));
    };
    if width == 0 || height == 0 || image.pixels.len() != width.saturating_mul(height) {
        return Err(format!("the frame is {width} by {height} pixels with {} pixels in it", image.pixels.len()));
    }
    let mut rgba = Vec::with_capacity(bytes);
    for pixel in &image.pixels {
        rgba.extend_from_slice(&pixel.to_srgba_unmultiplied());
    }
    let mut encoded = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut encoded, w32, h32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|e| format!("cannot encode the screenshot: {e}"))?;
        writer.write_image_data(&rgba).map_err(|e| format!("cannot encode the screenshot: {e}"))?;
    }
    // Whole or not at all: written beside the destination and renamed over it, so a full disk or a crash leaves the
    // old file, never half a picture. The destination is replaced, not opened, so even a file that turned into a
    // named pipe since the path was checked does not block the window.
    // Roots in force: judged again now, a frame after the request, and written at its folder's real path and its own name: a link that stands at the name by now is refused, and not
    // written through.
    let real = file_access::allow_entry(path, Access::Write).map_err(|why| format!("the screenshot was not written: {why}"))?;
    craft_atomic::atomic_write(&real, &encoded).map_err(|e| format!("cannot write the screenshot: {}", shown(&e.to_string(), 300)))?;
    Ok(json!({"path": path.to_string_lossy(), "width": width, "height": height}))
}
