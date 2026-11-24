//! Opening a file from the window: choosing it, reading it off the UI thread, and what the person is told.
//!
//! **Nothing here waits on the UI thread.** The Open dialog runs on a thread of its own (`pick_and_open`), and the file is read on
//! another (`open_path`): that thread makes a session of its own and runs `file.open` in it, which reads the file whole, checks it
//! and builds the document. Only a finished session comes back, and the window then puts it in the place of the old one. So **a
//! file that cannot be opened leaves the open document exactly as it was**, and a panic in reading is caught on that thread and is
//! a message, not the end of the app. The window shows a plain progress state while the thread works.
//!
//! The dialog itself is the desktop app's (`FilePicker`, given to [`DiacraftApp::with_file_picker`]): this crate depends on no
//! windowing back end. In the browser the window is given a host for files instead (`file_host.rs`): the page chooses the file, and the reading is done on the page's thread a frame after
//! the progress card is drawn, in the same session of its own.
//!
//! A refusal is told in words for a person by its **kind** ([`OpenProblem`]): "is damaged", "is not a drawing file", never a class
//! name and never the reader's own text. Warnings the reader gave go in the list of the status bar (`findings.rs`), not in a dialog.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::time::Duration;

use diacraft_engine::{EngineError, OpenProblem, Session, guard, shown};
use egui::{Align2, Context, CornerRadius, Frame, Margin, Order, Rect, Sense, Stroke, Ui, vec2};
use serde_json::{Value, json};

use crate::app::DiacraftApp;
use crate::control::ControlReply;
use crate::findings::{self, Finding};
use crate::theme::{self, Tokens, size};
use crate::widgets;

/// What chooses a file: the desktop app's native dialog. It blocks until the person chooses or cancels, so the window runs it on
/// a thread of its own. `None`: nothing was chosen.
pub type FilePicker = Arc<dyn Fn() -> Option<PathBuf> + Send + Sync>;

/// The most characters of a file's name the messages hold.
const NAME_CHARS: usize = 64;

/// How often the window looks again at a thread it is waiting for, if the thread has not woken it.
const POLL: Duration = Duration::from_millis(250);

/// What a thread that opened a file brings back.
struct Opened {
    session: Session,
    /// The reply of `file.open`: the document and `file`, the reader's counts and warnings.
    result: Value,
    /// The stencils the window had open that did not open again in the session, in words (`stencil_files::reopen`).
    stencil_notes: Vec<String>,
}

/// Why a file was not opened.
enum Failure {
    Refused(EngineError),
    Panicked(String),
    /// The path is not text a command can be given.
    NotText,
}

/// What a thread reads a file for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OpenKind {
    /// `file.open`: the drawing, stencil or template at the path.
    Open,
    /// `file.new {template: path}`: a new drawing made from the template file at the path. It is a new unsaved drawing; its answer says it was made
    /// from a template, and `began_opened_drawing` counts it as another unnamed drawing, as it counts `file.open` of a template.
    NewFromTemplate,
}

/// A file being read on a thread.
pub(crate) struct OpenJob {
    name: String,
    /// The path being read: a host that keeps something for it lets go when the reading ends.
    path: PathBuf,
    /// When the reading began, by the window's clock.
    started: Duration,
    done: Receiver<Result<Opened, Failure>>,
    /// The control channel's caller, waiting for the answer.
    reply: Option<Sender<ControlReply>>,
}

/// The most reading threads that may be alive at once. A thread blocked on a disk that does not answer (a network mount that hung) cannot
/// be stopped, only left: Cancel lets the person go on, and this keeps a person who cancels again and again from piling threads up.
const MAX_READERS: usize = 4;

/// Counts a reading thread while it lives; dropped when the thread ends, however it ends.
struct Reading(Arc<AtomicUsize>);

impl Reading {
    fn begin(count: &Arc<AtomicUsize>) -> Reading {
        count.fetch_add(1, Ordering::SeqCst);
        Reading(Arc::clone(count))
    }
}

impl Drop for Reading {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// What reads a file into a session. The real one is [`read_in_new_session`]; a test of a reader that panics sets its own.
type Reader = fn(&Path) -> Result<Opened, Failure>;

/// What the window keeps of opening files.
pub(crate) struct Opening {
    reader: Reader,
    job: Option<OpenJob>,
    /// The open dialog, running on its own thread.
    pick: Option<Receiver<Option<PathBuf>>>,
    pub(crate) picker: Option<FilePicker>,
    /// A file to open when the first frame is drawn (`diacraft FILE`).
    start: Option<PathBuf>,
    /// The message of the last file that could not be opened, until it is dismissed.
    pub(crate) banner: Option<String>,
    /// What the reader said of the file that is open.
    pub(crate) reading: Vec<Finding>,
    /// How many reading threads are alive, the ones whose opening was cancelled included.
    readers: Arc<AtomicUsize>,
}

impl Default for Opening {
    fn default() -> Opening {
        Opening {
            reader: read_in_new_session,
            job: None,
            pick: None,
            picker: None,
            start: None,
            banner: None,
            reading: Vec::new(),
            readers: Arc::new(AtomicUsize::new(0)),
        }
    }
}

/// Reads the file at `path` in a session of its own.
fn read_in_new_session(path: &Path) -> Result<Opened, Failure> {
    let text = path.to_str().ok_or(Failure::NotText)?;
    let mut session = Session::new();
    let result = session.execute("file.open", &json!({ "path": text })).map_err(Failure::Refused)?;
    Ok(Opened { session, result, stencil_notes: Vec::new() })
}

/// Makes a new drawing from the template file at `path` in a session of its own (`file.new {template}`): the file is read here, off the window's thread.
fn new_from_template_in_new_session(path: &Path) -> Result<Opened, Failure> {
    let text = path.to_str().ok_or(Failure::NotText)?;
    let mut session = Session::new();
    let result = session.execute("file.new", &json!({ "template": text })).map_err(Failure::Refused)?;
    Ok(Opened { session, result, stencil_notes: Vec::new() })
}

/// The file's name for a message: no folders, escaped, cut short.
pub(crate) fn name_of(path: &Path) -> String {
    let name = path.file_name().map_or_else(|| path.to_string_lossy().into_owned(), |n| n.to_string_lossy().into_owned());
    shown(&name, NAME_CHARS)
}

/// What a person is told when `name` could not be opened for this `problem`, in words and without the reader's own text.
pub fn refusal_text(name: &str, problem: OpenProblem) -> String {
    match problem {
        OpenProblem::NotFound => format!("{name} was not found. It may have been moved, renamed or deleted."),
        OpenProblem::Denied => format!("{name} could not be opened: this account is not allowed to read it."),
        OpenProblem::NotAFile => format!("{name} is not a file: it is a folder or a device."),
        OpenProblem::FileTooLarge => format!("{name} is too large to open."),
        OpenProblem::OutOfMemory => format!("There is not enough memory to open {name}."),
        OpenProblem::Unreadable => format!("{name} could not be read from the disk."),
        OpenProblem::NotAPackage => {
            format!(
                "{name} is not a drawing file DiaCraft can open. It opens .diacraft, .diacraft-stencil and .diacraft-template files, .vsdx, .vsdm, .vssx, .vssm, .vstx and .vstm files, and OpenDocument drawings and templates (.odg, .otg and .fodg)."
            )
        }
        OpenProblem::TooBig => format!("{name} is too big to open safely: it holds more than DiaCraft will read."),
        OpenProblem::Damaged => format!("{name} is damaged, so DiaCraft cannot read it."),
    }
}

/// What a person is told when `name` could not be opened for this `problem`, as [`refusal_text`] says it, **except for a `.diacraft` file the engine refuses for a reason that is
/// not that the file is no drawing**: one made by a newer version of the format, or one that needs a part this DiaCraft does not know. The engine reports both as
/// [`OpenProblem::NotAPackage`] and says which in its own words (they begin `made by a newer DiaCraft (format 2.0)` and `the file needs a part`, the words of the format's
/// section 2.4), and a person is told so: the file is not damaged and is no wrong kind, and a newer DiaCraft opens it.
pub fn refusal_words(name: &str, problem: OpenProblem, engine_words: &str) -> String {
    if problem == OpenProblem::NotAPackage {
        if let Some(rest) = engine_words.strip_prefix("made by a newer DiaCraft") {
            // ` (format 2.0): ...`: the version, when it is the plain numbers it is.
            let version = rest
                .strip_prefix(" (format ")
                .and_then(|after| after.split_once(')'))
                .map(|(version, _)| version)
                .filter(|version| version.len() <= 24 && !version.is_empty() && version.chars().all(|c| c.is_ascii_digit() || c == '.'))
                .map_or_else(String::new, |version| format!(" (format {version})"));
            return format!(
                "{name} was made by a newer DiaCraft{version}, which this DiaCraft cannot open. It is not damaged: a newer DiaCraft opens it."
            );
        }
        if engine_words.starts_with("the file needs a part") {
            return format!(
                "{name} needs a part that this DiaCraft does not know, so it cannot be opened. It is not damaged: a newer DiaCraft may open it."
            );
        }
    }
    refusal_text(name, problem)
}

/// The whole message for a failure: what happened, and that the drawing that was open is still open.
fn failure_text(name: &str, failure: &Failure, had_document: bool) -> String {
    let what = match failure {
        Failure::Refused(EngineError::CannotOpen { problem, msg, .. }) => refusal_words(name, *problem, msg),
        Failure::Refused(EngineError::Internal { .. }) | Failure::Panicked(_) => {
            format!("DiaCraft met an internal problem while opening {name}, so it was not opened. Please report this bug.")
        }
        Failure::Refused(other) => format!("{name} could not be opened: {other}"),
        Failure::NotText => format!("{name} could not be opened: its name is not text DiaCraft can use."),
    };
    if had_document { format!("{what} The drawing that was open is still open.") } else { what }
}

impl DiacraftApp {
    /// Gives the window a native Open dialog. Without one, the Open controls stay disabled.
    pub fn with_file_picker(mut self, picker: FilePicker) -> DiacraftApp {
        self.opening.picker = Some(picker);
        self
    }

    /// Opens `path` when the first frame is drawn, as `diacraft FILE` does.
    pub fn opening_at_start(mut self, path: impl Into<PathBuf>) -> DiacraftApp {
        self.opening.start = Some(path.into());
        self
    }

    /// Whether there is a dialog to choose a file with, and nothing is being opened or chosen now: whether Open can be used.
    pub fn can_pick_file(&self) -> bool {
        (self.opening.picker.is_some() || self.has_file_host()) && self.opening.job.is_none() && self.opening.pick.is_none()
    }

    /// Whether the Open dialog is on the screen (its thread has not answered yet). A caller that waits for a chosen file waits on this, then on
    /// `opening_now` and `prompt_open`: the answer is taken in the frame after the thread sends it.
    pub fn open_dialog_open(&self) -> bool {
        self.opening.pick.is_some()
    }

    /// The file being opened and the seconds it has taken, while one is.
    pub fn opening_now(&self) -> Option<(&str, u64)> {
        self.opening.job.as_ref().map(|j| (j.name.as_str(), self.clock.now().saturating_sub(j.started).as_secs()))
    }

    /// The message of the last file that could not be opened.
    pub fn open_error(&self) -> Option<&str> {
        self.opening.banner.as_deref()
    }

    /// Shows the open dialog on a thread of its own, and opens what it chooses. The File view closes when a file is chosen.
    pub fn pick_and_open(&mut self, ctx: &Context) {
        // With a host (the browser) the page chooses the file, and answers later with a path in the engine's memory (`file_host.rs`).
        if self.has_file_host() {
            if self.can_pick_file() {
                self.opening.pick = self.host_choose(ctx, crate::file_host::Want::Drawing);
            }
            return;
        }
        let Some(picker) = self.opening.picker.clone() else {
            self.set_notice("Opening a file needs a file dialog, which this build does not have.");
            return;
        };
        if !self.can_pick_file() {
            return;
        }
        let (tx, rx) = channel();
        let wake = ctx.clone();
        let spawned = std::thread::Builder::new().name("diacraft-open-dialog".into()).spawn(move || {
            // A dialog that panics is no file chosen.
            let chosen = guard::catch_panic(|| picker()).ok().flatten();
            let _ = tx.send(chosen);
            wake.request_repaint();
        });
        match spawned {
            Ok(_) => self.opening.pick = Some(rx),
            Err(e) => self.set_notice(&format!("The file dialog could not be started: {e}")),
        }
    }

    /// Opens the file at `path` on a thread. A file is opened at a time: while one is read, another request is told to wait.
    pub fn open_path(&mut self, ctx: &Context, path: PathBuf) {
        if let Err(message) = self.start_open(ctx, path, None, OpenKind::Open) {
            self.set_notice(&message);
        }
    }

    /// Starts a new drawing from the template file at `path`, read on a thread as `open_path` reads a file: `file.new {template: path}` in a session
    /// of its own, so that a template of any size never holds the window. A built-in template is made by `file.new {template: id}` at once, as it is small.
    pub fn new_from_template_file(&mut self, ctx: &Context, path: PathBuf) {
        if let Err(message) = self.start_open(ctx, path, None, OpenKind::NewFromTemplate) {
            self.set_notice(&message);
        }
    }

    /// Starts reading `path`. With a `reply`, the control channel's caller is answered when the reading ends, one way or the other.
    pub(crate) fn start_open(&mut self, ctx: &Context, path: PathBuf, reply: Option<Sender<ControlReply>>, kind: OpenKind) -> Result<(), String> {
        if let Some(job) = &self.opening.job {
            return Err(format!("Still opening {}: wait for it to finish.", job.name));
        }
        let name = name_of(&path);
        if self.opening.readers.load(Ordering::SeqCst) >= MAX_READERS {
            return Err(format!(
                "{name} was not opened: earlier files that were cancelled are still waiting on the disk. Wait for them to give up, or restart DiaCraft."
            ));
        }
        // A label being typed is part of the drawing the person is leaving (M1).
        self.commit_pending_text();
        let (tx, done) = channel();
        let wake = ctx.clone();
        let reader = self.opening.reader;
        // The stencils the window has open are opened again in the session this thread makes (`stencil_files.rs`).
        let stencils = self.stencil_paths();
        let reading = Reading::begin(&self.opening.readers);
        let name_for_job = path.clone();
        let spawned = self.run_in_background("diacraft-open", move || {
            // Counted until the thread ends, even if nobody is waiting for its answer any more.
            let _reading = reading;
            let outcome = guard::catch_panic(|| {
                let read = match kind {
                    OpenKind::Open => reader(&path),
                    OpenKind::NewFromTemplate => new_from_template_in_new_session(&path),
                };
                read.map(|mut opened| {
                    opened.stencil_notes = crate::stencil_files::reopen(&mut opened.session, &stencils);
                    // A program that waits on the control channel is told too: the answer of the open lists the stencils that were left out.
                    if !opened.stencil_notes.is_empty()
                        && let Some(answer) = opened.result.as_object_mut()
                    {
                        answer.insert("stencil_notes".to_owned(), json!(opened.stencil_notes));
                    }
                    opened
                })
            })
            .unwrap_or_else(|msg| Err(Failure::Panicked(msg)));
            // The window may have been closed meanwhile; there is nobody to tell.
            let _ = tx.send(outcome);
            wake.request_repaint();
        });
        if spawned.is_err() {
            return Err(format!("{name} could not be opened: this build cannot read a file in the background."));
        }
        self.ui.backstage = None;
        self.opening.job = Some(OpenJob { name, path: name_for_job, started: self.clock.now(), done, reply });
        ctx.request_repaint();
        Ok(())
    }

    /// Gives up on the file being opened (the Cancel button, Escape). A disk that does not answer (a network mount that hung) leaves its thread
    /// blocked and nothing can stop it; the window stops waiting for it. **What the thread brings back, if it ever does, is thrown away**: the
    /// drawing that is open stays as it is, and another file can be opened at once. A caller waiting on the control channel is told it was
    /// cancelled. → whether a file was being opened.
    pub fn cancel_open(&mut self) -> bool {
        let Some(job) = self.opening.job.take() else { return false };
        log::warn!("opening {} was cancelled after {} s", job.name, self.clock.now().saturating_sub(job.started).as_secs());
        // With a host the reading may not have begun: it is dropped. What the host keeps of the file is let go.
        self.cancel_background("diacraft-open");
        self.release_file(&job.path);
        // The receiver goes with the job, so the thread's answer has nowhere to go and is dropped when it arrives.
        if let Some(reply) = job.reply {
            let _ = reply.send(Err(format!("Opening {} was cancelled.", job.name)));
        }
        // About the opening, not the window: it goes when another drawing is shown.
        self.set_drawing_notice(&format!("Cancelled opening {}.", job.name));
        true
    }

    /// Moves opening along: starts the file given on the command line, takes the dialog's answer, and takes in a file that has been
    /// read. Runs before every frame.
    pub(crate) fn advance_open(&mut self, ctx: &Context) {
        if let Some(path) = self.opening.start.take() {
            self.open_path(ctx, path);
        }
        if let Some(rx) = &self.opening.pick {
            match rx.try_recv() {
                Ok(chosen) => {
                    self.opening.pick = None;
                    if let Some(path) = chosen {
                        // Asks first if the drawing has unsaved changes: a file replaces it without a word otherwise.
                        self.ask_then(ctx, crate::save::Then::Open(path));
                    }
                }
                Err(TryRecvError::Empty) => ctx.request_repaint_after(POLL),
                Err(TryRecvError::Disconnected) => self.opening.pick = None,
            }
        }
        let finished = match self.opening.job.as_ref().map(|j| j.done.try_recv()) {
            None => return,
            Some(Ok(outcome)) => outcome,
            Some(Err(TryRecvError::Empty)) => {
                // The seconds on the progress card move.
                ctx.request_repaint_after(POLL);
                return;
            }
            Some(Err(TryRecvError::Disconnected)) => Err(Failure::Panicked("the reading stopped without an answer".to_owned())),
        };
        if let Some(job) = self.opening.job.take() {
            self.finish_open(job, finished);
        }
    }

    fn finish_open(&mut self, job: OpenJob, outcome: Result<Opened, Failure>) {
        // Whatever the reading was, what the host kept of the file is not needed now: the engine has the bytes, or has refused them.
        self.release_file(&job.path);
        match outcome {
            Ok(opened) => {
                self.session = opened.session;
                self.stencils_after_open();
                self.began_opened_drawing(&opened.result);
                // After the drawing's own notices are cleared: the stencils that did not open again are said beside the drawing that was read.
                if !opened.stencil_notes.is_empty() {
                    self.set_drawing_notice(&crate::stencil_files::notes_message(&opened.stencil_notes));
                }
                if let Some(reply) = job.reply {
                    let _ = reply.send(Ok(opened.result));
                }
            }
            Err(failure) => {
                if let Failure::Panicked(msg) = &failure {
                    log::error!("opening {} stopped on a panic: {msg}", job.name);
                }
                let text = failure_text(&job.name, &failure, self.session.document().is_some());
                self.opening.banner = Some(text.clone());
                if let Some(reply) = job.reply {
                    let message = match &failure {
                        Failure::Refused(e) => e.to_string(),
                        _ => text,
                    };
                    let _ = reply.send(Err(message));
                }
            }
        }
    }

    /// A file replaced the drawing: it shows its first page, fitted, with its View settings as the drawing holds them (`window_settings.rs`).
    pub(crate) fn began_opened_drawing(&mut self, result: &Value) {
        self.drawing_replaced();
        // A template makes a new unnamed drawing, and is counted as one: whether it came by `file.open` of a template file or by `file.new` with a template
        // that is a file. A file that is not a template has the name of its file.
        let new_from_template = result.get("file").and_then(|f| f.get("new_from_template")).and_then(Value::as_bool) == Some(true);
        if new_from_template || result.get("template").is_some() {
            self.numbered_another_drawing();
        }
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
        // What the drawing shows: a new drawing's settings with no windows part, its drawing window's with one, and where that holds no
        // value the grid and the rulers off, the guides on, the connection points off (`window_settings.rs`).
        self.showed_drawings_window_settings();
        self.opening.banner = None;
        let file = result.get("file");
        // `file.open` lists the reader's warnings under `file`; `file.new {template}` lists them at the top of its answer (and counts them in `file`).
        let warnings: Vec<String> = file
            .and_then(|f| f.get("warnings"))
            .or_else(|| result.get("warnings"))
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|w| w.as_str().map(str::to_owned)).collect())
            .unwrap_or_default();
        let total = file.and_then(|f| f.get("warning_count")).and_then(Value::as_u64).unwrap_or(warnings.len() as u64);
        self.opening.reading = findings::from_reading(&warnings, total);
    }

    /// `engine.execute file.open` from the control channel: read on a thread, answered when the reading ends.
    pub(crate) fn control_open(&mut self, ctx: &Context, params: &Value, reply: Sender<ControlReply>) -> Result<(), String> {
        let object = params.as_object().ok_or("invalid parameters for `file.open`: they must be an object")?;
        if let Some(key) = object.keys().find(|k| k.as_str() != "path") {
            return Err(format!("invalid parameters for `file.open`: unknown parameter `{}`; the command takes: path", shown(key, 64)));
        }
        let Some(Value::String(path)) = object.get("path") else {
            return Err("invalid parameters for `file.open`: `path` must be text".to_owned());
        };
        self.start_open(ctx, PathBuf::from(path), Some(reply), OpenKind::Open)
    }

    /// `engine.execute file.new` from the control channel with a template that is a file: read on a thread, answered when the reading ends.
    pub(crate) fn control_new_from_file(&mut self, ctx: &Context, params: &Value, reply: Sender<ControlReply>) -> Result<(), String> {
        let object = params.as_object().ok_or("invalid parameters for `file.new`: they must be an object")?;
        if let Some(key) = object.keys().find(|k| k.as_str() != "template") {
            return Err(format!("invalid parameters for `file.new`: unknown parameter `{}`; the command takes: template", shown(key, 64)));
        }
        let Some(Value::String(path)) = object.get("template") else {
            return Err("invalid parameters for `file.new`: `template` must be text".to_owned());
        };
        self.start_open(ctx, PathBuf::from(path), Some(reply), OpenKind::NewFromTemplate)
    }

    /// Dismisses the message about a file that could not be opened.
    pub fn dismiss_open_error(&mut self) {
        self.opening.banner = None;
    }

    /// The message over the drawing area about a file that could not be opened, and the progress card while one is read.
    pub(crate) fn open_overlay(&mut self, ui: &mut Ui, t: Tokens) {
        let ctx = ui.ctx().clone();
        let area = self.ui.canvas_rect.unwrap_or_else(|| ctx.content_rect());
        if let Some((name, seconds)) = self.opening_now().map(|(n, s)| (n.to_owned(), s)) {
            let mut cancelled = false;
            let text = if seconds >= 2 { format!("Opening {name}... {seconds} s") } else { format!("Opening {name}...") };
            // A modal: while the file is read the window takes no edits from the person, and Cancel is the one thing to press.
            let modal =
                egui::Modal::new(egui::Id::unique("card_opening")).backdrop_color(t.scrim).frame(DiacraftApp::card_frame(&t)).show(&ctx, |ui| {
                    // The words wrap inside the room the area has, and Cancel sits under them, so that a long name or a narrow
                    // window cuts nothing off.
                    ui.set_max_width((area.width() - 80.0).clamp(160.0, 420.0));
                    ui.horizontal_top(|ui| {
                        ui.add(egui::Spinner::new().color(t.accent));
                        let label = ui.add(egui::Label::new(egui::RichText::new(&text).font(theme::font(size::TAB)).color(t.text)).wrap());
                        label.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &text));
                    });
                    ui.add_space(8.0);
                    let cancel = ui.add(egui::Button::new(egui::RichText::new("Cancel").font(theme::font(size::LABEL))).min_size(vec2(84.0, 26.0)));
                    self.note("opening.cancel", cancel.rect, true);
                    if cancel.on_hover_text("Stop waiting for this file (Esc)").clicked() {
                        cancelled = true;
                    }
                });
            crate::ask::keep_centred(&ctx, modal.response.rect);
            // Escape is Cancel, as in a dialog. Nothing else is under way that Escape could mean while a file is being read.
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
                cancelled = true;
            }
            if cancelled {
                self.cancel_open();
            }
        }
        // The save note is drawn under the message when there is one, in the same place.
        let mut below = area.top() + 10.0;
        if let Some(message) = self.opening.banner.clone() {
            below = self.banner_overlay(&ctx, t, area, &message);
        }
        self.save_note_overlay(&ctx, t, area, below);
    }

    /// The message about a file that could not be opened (or a save, an export or a print that did not work, or the note of an `.odg`) over the top of the drawing area, with the button that
    /// dismisses it. Gives the bottom of the card, where the save note starts.
    fn banner_overlay(&mut self, ctx: &Context, t: Tokens, area: Rect, message: &str) -> f32 {
        let mut dismissed = false;
        let shown = egui::Area::new(egui::Id::unique("open_error"))
            .order(Order::Foreground)
            .pivot(Align2::CENTER_TOP)
            .fixed_pos(area.center_top() + vec2(0.0, 10.0))
            .show(ctx, |ui| {
                Frame::new()
                    .fill(t.ribbon_fill)
                    .stroke(Stroke::new(1.0, t.notice_text))
                    .corner_radius(CornerRadius::same(6))
                    .inner_margin(Margin::symmetric(12, 8))
                    .show(ui, |ui| {
                        // The words wrap inside the room the area has, whatever its width, and the close button stays beside them.
                        let words_width = (area.width() - 40.0 - 24.0 - 30.0).clamp(120.0, 520.0);
                        ui.horizontal_top(|ui| {
                            ui.scope(|ui| {
                                ui.set_max_width(words_width);
                                let label = ui.add(egui::Label::new(egui::RichText::new(message).font(theme::font(size::TAB)).color(t.text)).wrap());
                                label.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, message));
                            });
                            let (rect, resp) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::click());
                            widgets::name_button(&resp, true, "Dismiss message");
                            widgets::paint_button_bg(ui, rect, &resp, true, false, &t, 4);
                            let c = rect.center();
                            let ink = Stroke::new(1.4, t.text);
                            ui.painter().line_segment([c + vec2(-4.0, -4.0), c + vec2(4.0, 4.0)], ink);
                            ui.painter().line_segment([c + vec2(-4.0, 4.0), c + vec2(4.0, -4.0)], ink);
                            if resp.on_hover_text("Dismiss").clicked() {
                                dismissed = true;
                            }
                            self.note("open-error.dismiss", rect, true);
                        });
                    })
                    .response
                    .rect
                    .bottom()
            });
        if dismissed {
            self.opening.banner = None;
        }
        shown.inner + 8.0
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use super::*;

    #[test]
    fn every_problem_has_words_that_name_the_file_and_no_class() {
        let problems = [
            OpenProblem::NotFound,
            OpenProblem::Denied,
            OpenProblem::NotAFile,
            OpenProblem::FileTooLarge,
            OpenProblem::OutOfMemory,
            OpenProblem::Unreadable,
            OpenProblem::NotAPackage,
            OpenProblem::TooBig,
            OpenProblem::Damaged,
        ];
        for problem in problems {
            let text = refusal_text("plan.vsdx", problem);
            assert!(text.contains("plan.vsdx"), "{text}");
            // The engine's fixed word for a problem is a snake_case class (`not_found`); a person is not shown one.
            assert!(!text.contains(problem.word()) || !problem.word().contains('_'), "{text}");
            assert!(!text.contains('_') && !text.contains("::"), "{text}");
            assert!(text.ends_with('.'), "{text}");
        }
    }

    /// The engine reports a file of a newer version of the format, and one that needs a part it does not know, as `NotAPackage`, and says which in its own words. The window
    /// tells those by name; every other refusal is told by its kind, whatever the engine's words are, and nothing from the engine's words but a plain version reaches a person.
    #[test]
    fn a_newer_version_of_the_format_is_told_by_its_version_and_every_other_refusal_by_its_kind() {
        let newer = "made by a newer DiaCraft (format 2.0): this DiaCraft cannot open it, and a newer one can (newer_major)";
        let said = refusal_words("plan.diacraft", OpenProblem::NotAPackage, newer);
        assert!(said.starts_with("plan.diacraft was made by a newer DiaCraft (format 2.0), which this DiaCraft cannot open."), "{said}");
        assert!(said.contains("not damaged") && !said.contains("newer_major") && !said.contains("not a drawing file"), "{said}");
        // A version that is more than numbers and dots is not repeated (the words are the engine's, and the file chose the numbers), and the words are the same without one.
        for odd in [
            "made by a newer DiaCraft (format 2.0<b>): x",
            "made by a newer DiaCraft (format ): x",
            "made by a newer DiaCraft: x",
            "made by a newer DiaCraft (format 1234567890123456789012345.0): x",
        ] {
            let said = refusal_words("a.diacraft", OpenProblem::NotAPackage, odd);
            assert!(said.starts_with("a.diacraft was made by a newer DiaCraft, which this DiaCraft cannot open."), "{odd}: {said}");
        }
        let needs = "the file needs a part of the role \"x\" and this DiaCraft does not know it: this DiaCraft cannot open it (required_role)";
        let said = refusal_words("a.diacraft", OpenProblem::NotAPackage, needs);
        assert!(
            said.contains("needs a part that this DiaCraft does not know")
                && said.contains("not damaged")
                && !said.contains("required_role")
                && !said.contains("\"x\""),
            "{said}"
        );
        // Another reason, or the same words under another kind, is told as the kind says.
        assert_eq!(
            refusal_words("a", OpenProblem::NotAPackage, "this is not a drawing file (not_zip): x"),
            refusal_text("a", OpenProblem::NotAPackage)
        );
        assert_eq!(refusal_words("a", OpenProblem::Damaged, newer), refusal_text("a", OpenProblem::Damaged));
        assert_eq!(refusal_words("a", OpenProblem::NotAPackage, ""), refusal_text("a", OpenProblem::NotAPackage));
    }

    #[test]
    fn a_failure_says_the_old_drawing_is_still_open_only_when_there_is_one() {
        let failure =
            Failure::Refused(EngineError::CannotOpen { cmd: "file.open".into(), problem: OpenProblem::Damaged, msg: "raw reader text".into() });
        let with = failure_text("a.vsdx", &failure, true);
        assert!(with.contains("still open") && with.contains("damaged") && !with.contains("raw reader text"), "{with}");
        assert!(!failure_text("a.vsdx", &failure, false).contains("still open"));
        let panic = failure_text("a.vsdx", &Failure::Panicked("index out of bounds: secret.txt".into()), true);
        assert!(!panic.contains("secret") && panic.contains("internal problem"), "{panic}");
    }

    #[test]
    fn a_file_name_has_no_folders_and_no_control_characters() {
        assert_eq!(name_of(Path::new("/home/someone/Plans/floor.vsdx")), "floor.vsdx");
        assert!(!name_of(Path::new("a\u{1b}[31m.vsdx")).contains('\u{1b}'));
    }

    fn panicking(_: &Path) -> Result<Opened, Failure> {
        panic!("the reader broke on secret.vsdx")
    }

    /// As [`wait_for_the_reading`], for a read that takes as long as the machine makes it (two parses of 80,000 shapes, one after the other, on a loaded one): a hang is still an end, after
    /// two minutes of looks (the window reads no time of its own: `tests/it/clock.rs`).
    fn wait_for_a_long_reading(app: &mut DiacraftApp, ctx: &Context) {
        for _ in 0..24_000 {
            app.advance_open(ctx);
            if app.opening_now().is_none() {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("the reading did not end in two minutes");
    }

    fn wait_for_the_reading(app: &mut DiacraftApp, ctx: &Context) {
        for _ in 0..2000 {
            app.advance_open(ctx);
            if app.opening_now().is_none() {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("the reading did not end");
    }

    /// The brief's case: a panic in reading shows an error, the app goes on, and the old document is kept.
    #[test]
    fn a_panic_in_reading_is_a_message_and_the_old_document_stays() {
        let mut session = Session::new();
        session.execute("file.new", &json!({})).unwrap();
        session.execute("page.add", &json!({"name": "Second"})).unwrap();
        let revision = session.revision();
        let mut app = DiacraftApp::new(session, None);
        app.opening.reader = panicking;
        let ctx = Context::default();
        let (reply, answer) = channel();
        app.start_open(&ctx, PathBuf::from("/somewhere/plans.vsdx"), Some(reply), OpenKind::Open).unwrap();
        assert!(app.opening_now().is_some(), "reading has begun on its thread");
        wait_for_the_reading(&mut app, &ctx);
        let message = app.open_error().expect("an error is shown").to_owned();
        assert!(message.contains("plans.vsdx") && message.contains("internal problem") && message.contains("still open"), "{message}");
        assert!(!message.contains("secret") && !message.contains("somewhere"), "nothing of the panic or the folder: {message}");
        assert_eq!(app.session().revision(), revision, "the session is the one that was open");
        assert_eq!(app.session().document().map(|d| d.pages.len()), Some(2));
        assert_eq!(app.ui_panics(), 0, "caught on the reading thread, not by a frame");
        let told = answer.try_recv().expect("the control channel is answered").unwrap_err();
        assert!(!told.contains("secret"), "{told}");
        // The app goes on: another file can be opened at once.
        app.opening.reader = panicking;
        assert!(app.start_open(&ctx, PathBuf::from("/x/other.vsdx"), None, OpenKind::Open).is_ok());
        wait_for_the_reading(&mut app, &ctx);
        assert!(app.open_error().is_some_and(|m| m.contains("other.vsdx")));
    }

    #[test]
    fn a_second_open_while_one_is_read_is_told_to_wait_and_starts_nothing() {
        fn slow(_: &Path) -> Result<Opened, Failure> {
            std::thread::sleep(Duration::from_millis(300));
            Err(Failure::NotText)
        }
        let mut app = DiacraftApp::new(Session::new(), None);
        app.opening.reader = slow;
        let ctx = Context::default();
        app.start_open(&ctx, PathBuf::from("/x/first.vsdx"), None, OpenKind::Open).unwrap();
        let refused = app.start_open(&ctx, PathBuf::from("/x/second.vsdx"), None, OpenKind::Open).unwrap_err();
        assert!(refused.contains("first.vsdx") && refused.contains("wait"), "{refused}");
        assert_eq!(app.opening_now().map(|(n, _)| n), Some("first.vsdx"));
        wait_for_the_reading(&mut app, &ctx);
    }

    static HUNG: AtomicUsize = AtomicUsize::new(0);
    static BLOCKED: AtomicUsize = AtomicUsize::new(0);

    /// A disk that does not answer: blocked until the test lets it go (and for at most thirty seconds, so a failing test ends).
    fn hung(_: &Path) -> Result<Opened, Failure> {
        for _ in 0..3000 {
            if HUNG.load(Ordering::SeqCst) == 1 {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let mut session = Session::new();
        session.execute("file.new", &json!({})).unwrap();
        session.execute("page.add", &json!({"name": "From the hung disk"})).unwrap();
        Ok(Opened { session, result: json!({}), stencil_notes: Vec::new() })
    }

    fn quick(_: &Path) -> Result<Opened, Failure> {
        let mut session = Session::new();
        session.execute("file.new", &json!({})).unwrap();
        session.execute("page.add", &json!({"name": "Second"})).unwrap();
        session.execute("page.add", &json!({"name": "Third"})).unwrap();
        Ok(Opened { session, result: json!({}), stencil_notes: Vec::new() })
    }

    /// The review's case: an open on a hung network mount left "Opening..." up for ever, and every later Open was told to wait. Cancel lets
    /// the person go on; the hung thread's answer, if it ever comes, is thrown away; a caller on the control channel is told.
    #[test]
    fn an_open_on_a_hung_disk_can_be_cancelled_and_the_next_open_goes_through() {
        let mut session = Session::new();
        session.execute("file.new", &json!({})).unwrap();
        let revision = session.revision();
        let mut app = DiacraftApp::new(session, None);
        let ctx = Context::default();
        HUNG.store(0, Ordering::SeqCst);
        app.opening.reader = hung;
        let (reply, answer) = channel();
        app.start_open(&ctx, PathBuf::from("/mnt/hung/plans.vsdx"), Some(reply), OpenKind::Open).unwrap();
        app.advance_open(&ctx);
        assert_eq!(app.opening_now().map(|(n, _)| n), Some("plans.vsdx"));
        assert!(
            app.start_open(&ctx, PathBuf::from("/x/other.vsdx"), None, OpenKind::Open).unwrap_err().contains("wait"),
            "while it hangs, another waits"
        );
        assert!(app.cancel_open());
        assert!(app.opening_now().is_none(), "the progress card goes");
        assert_eq!(answer.try_recv().expect("the control channel is told").unwrap_err(), "Opening plans.vsdx was cancelled.");
        assert!(app.notice().is_some_and(|n| n == "Cancelled opening plans.vsdx."), "{:?}", app.notice());
        assert!(!app.cancel_open(), "nothing left to cancel");
        // The next open goes through, though the hung thread is still alive.
        app.opening.reader = quick;
        app.start_open(&ctx, PathBuf::from("/x/other.vsdx"), None, OpenKind::Open).unwrap();
        wait_for_the_reading(&mut app, &ctx);
        assert_eq!(app.session().document().map(|d| d.pages.len()), Some(3), "the file that was not cancelled is open");
        // The disk answers at last: what it brings is thrown away, not put in the place of the drawing the person went on to.
        HUNG.store(1, Ordering::SeqCst);
        for _ in 0..50 {
            app.advance_open(&ctx);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(app.session().document().map(|d| d.pages.len()), Some(3));
        assert!(app.open_error().is_none() && app.opening_now().is_none());
        assert_ne!(app.session().revision(), revision);
    }

    static GATE: AtomicUsize = AtomicUsize::new(0);

    /// A disk that answers when the test lets it (and at the latest after thirty seconds, so a failing test ends).
    fn gated(_: &Path) -> Result<Opened, Failure> {
        for _ in 0..3000 {
            if GATE.load(Ordering::SeqCst) == 1 {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let mut session = Session::new();
        session.execute("file.new", &json!({})).unwrap();
        session.execute("page.add", &json!({"name": "From the file"})).unwrap();
        Ok(Opened { session, result: json!({}), stencil_notes: Vec::new() })
    }

    /// L7 of the M3 review (ours: the window takes no edits meanwhile, rather than asking when the file arrives): while a file is being read the
    /// drawing under the progress card takes no drag, no key and no click on the toolbar, so nothing the person did is replaced without a word.
    #[test]
    fn while_a_file_is_read_the_window_takes_no_edits_from_the_person() {
        use egui::{Event, Key, Modifiers, PointerButton};
        let mut session = Session::new();
        session.execute("file.new", &json!({})).unwrap();
        let page = session.document().unwrap().pages.get(0).unwrap().id;
        let id = session.execute("shape.drop", &json!({"master": "rectangle", "page": page, "x": 3.0, "y": 3.0})).unwrap()["id"].as_u64().unwrap();
        let mut w = egui_kittest::Harness::builder().with_size([1280.0, 900.0]).with_step_dt(1.0 / 60.0).build_eframe(|_| {
            // A clock that stands still, as in every test window (`clock.rs`): the window's timers do not run out in the middle of the `run`s below.
            DiacraftApp::new(session, None).with_inline_pictures(crate::render_worker::bundled_drawer()).with_clock(crate::clock::Clock::manual())
        });
        w.run();
        w.state_mut().execute("view.zoom", &json!({"percent": 100})).unwrap();
        w.run();
        w.state_mut().select(&[id as u32]);
        w.run();
        let centre = w.state().screen_point_at((3.0, 3.0)).unwrap();
        let undo = w.state().element("qat.Undo").unwrap().rect.center();
        GATE.store(0, Ordering::SeqCst);
        w.state_mut().opening.reader = gated;
        let ctx = w.ctx.clone();
        w.state_mut().start_open(&ctx, PathBuf::from("/x/other.vsdx"), None, OpenKind::Open).unwrap();
        w.step();
        w.step();
        assert!(w.state().opening_now().is_some() && w.state().is_modal_open(), "a file is being read");
        let revision = w.state().session().revision();
        let send = |w: &mut egui_kittest::Harness<'static, DiacraftApp>, event: Event| {
            w.input_mut().events.push(event);
            w.step();
        };
        // A drag of the selected rectangle.
        for _ in 0..3 {
            send(&mut w, Event::PointerMoved(centre));
        }
        send(&mut w, Event::PointerButton { pos: centre, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
        for step in 1..=5 {
            send(&mut w, Event::PointerMoved(centre + egui::vec2(0.0, -20.0 * step as f32)));
        }
        let end = centre + egui::vec2(0.0, -100.0);
        send(&mut w, Event::PointerButton { pos: end, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
        // Delete, Undo by the keys, Undo by the toolbar.
        for (key, modifiers) in [(Key::Delete, Modifiers::NONE), (Key::Z, Modifiers::COMMAND)] {
            send(&mut w, Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers });
            send(&mut w, Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers });
        }
        for pressed in [true, false] {
            send(&mut w, Event::PointerMoved(undo));
            send(&mut w, Event::PointerButton { pos: undo, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        }
        assert!(w.state().opening_now().is_some(), "still being read");
        assert_eq!(w.state().session().revision(), revision, "the drawing was not changed while the file was read");
        assert_eq!(w.state().session().document().unwrap().pages.get(0).unwrap().shapes.len(), 1, "and the shape is still there");
        // The file arrives, and replaces a drawing the person did not change in the meantime.
        GATE.store(1, Ordering::SeqCst);
        for _ in 0..500 {
            w.step();
            if w.state().opening_now().is_none() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(w.state().opening_now().is_none());
        assert_eq!(w.state().session().document().map(|d| d.pages.len()), Some(2), "the file that was read is open");
    }

    /// Cancelling over and over does not pile threads up without end: past four that are still blocked, an Open is refused in words.
    #[test]
    fn threads_left_blocked_by_cancelled_opens_are_bounded() {
        fn blocked(_: &Path) -> Result<Opened, Failure> {
            for _ in 0..3000 {
                if BLOCKED.load(Ordering::SeqCst) == 1 {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(Failure::NotText)
        }
        BLOCKED.store(0, Ordering::SeqCst);
        let mut app = DiacraftApp::new(Session::new(), None);
        app.opening.reader = blocked;
        let ctx = Context::default();
        for n in 0..MAX_READERS {
            app.start_open(&ctx, PathBuf::from(format!("/mnt/hung/{n}.vsdx")), None, OpenKind::Open).unwrap();
            assert!(app.cancel_open());
        }
        let refused = app.start_open(&ctx, PathBuf::from("/mnt/hung/last.vsdx"), None, OpenKind::Open).unwrap_err();
        assert!(refused.contains("still waiting on the disk") && !refused.contains("::"), "{refused}");
        assert!(app.opening_now().is_none(), "nothing was started");
        // They end when the disk gives up, and Open works again.
        BLOCKED.store(1, Ordering::SeqCst);
        for _ in 0..300 {
            if app.opening.readers.load(Ordering::SeqCst) == 0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(app.opening.readers.load(Ordering::SeqCst), 0, "the threads ended and were counted out");
        app.opening.reader = quick;
        assert!(app.start_open(&ctx, PathBuf::from("/x/ok.vsdx"), None, OpenKind::Open).is_ok());
        wait_for_the_reading(&mut app, &ctx);
    }

    // ------------------------------------------------------------------------------------- the open thread beside a drawing that is held

    const CHILD_FIRST: &str = "DIACRAFT_UI_OPEN_FIRST";
    const CHILD_SECOND: &str = "DIACRAFT_UI_OPEN_SECOND";

    /// A line of `/proc/self/status`, in kibibytes.
    fn status_kib(key: &str) -> u64 {
        let text = std::fs::read_to_string("/proc/self/status").unwrap();
        text.lines()
            .find_map(|l| l.strip_prefix(key))
            .and_then(|rest| rest.trim_start_matches(':').split_whitespace().next().and_then(|n| n.parse().ok()))
            .unwrap()
    }

    /// The child: a window whose session holds a drawing of 80,000 shapes, and the real reader of the open thread (a session of its own, on a thread) asked for a second file.
    #[test]
    #[ignore = "run by the test below under `ulimit -v`; alone it needs the files in the environment"]
    fn window_child() {
        let (Ok(first), Ok(second)) = (std::env::var(CHILD_FIRST), std::env::var(CHILD_SECOND)) else { return };
        let mut session = Session::new();
        session.execute("file.open", &json!({ "path": first })).unwrap();
        let mut app = DiacraftApp::new(session, None);
        let ctx = Context::default();
        app.start_open(&ctx, PathBuf::from(&second), None, OpenKind::Open).unwrap();
        wait_for_the_reading(&mut app, &ctx);
        println!("\nWINDOW message={}", app.open_error().unwrap_or("none"));
        println!("WINDOW pages={} VmPeak={}KiB", app.session().document().map_or(0, |d| d.pages.len()), status_kib("VmPeak"));
    }

    /// The child of the cancelled read: a window with no drawing is asked to open a drawing of 80,000 shapes, Cancel is pressed at once, and the same file is opened again while the
    /// cancelled thread (a thread cannot be stopped) still parses.
    #[test]
    #[ignore = "run by the test below under `ulimit -v`; alone it needs the file in the environment"]
    fn cancelled_child() {
        let Ok(path) = std::env::var(CHILD_FIRST) else { return };
        // The wait for the cancelled thread is as long as a loaded machine needs to parse 80,000 shapes (the product's is 30 seconds).
        diacraft_engine::memory::set_read_wait(Duration::from_secs(120));
        let mut app = DiacraftApp::new(Session::new(), None);
        let ctx = Context::default();
        app.start_open(&ctx, PathBuf::from(&path), None, OpenKind::Open).unwrap();
        assert!(app.cancel_open(), "a file was being opened");
        // The cancelled thread loads its file and then holds the budget while it parses; the next Open is made when it does, so that it finds the budget held and waits. (Made before, its
        // thread would hold the budget first, and the cancelled one would be the one to wait: the wait is then not what is tested.) A bound of looks, and no time of the machine's.
        for _ in 0..12_000 {
            if diacraft_engine::memory::reserved() > 0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(diacraft_engine::memory::reserved() > 0, "the cancelled thread holds the budget while it parses");
        // The same file again, or the file the test names (`CHILD_SECOND`): a small one, which waits for the cancelled thread and is read when it ends.
        let next = std::env::var(CHILD_SECOND).unwrap_or_else(|_| path.clone());
        app.start_open(&ctx, PathBuf::from(&next), None, OpenKind::Open).unwrap();
        wait_for_a_long_reading(&mut app, &ctx);
        // The cancelled thread runs to its end, and gives back what it held.
        while app.opening.readers.load(Ordering::SeqCst) > 0 {
            std::thread::sleep(Duration::from_millis(10));
        }
        println!("\nWINDOW message={}", app.open_error().unwrap_or("none"));
        println!("WINDOW pages={} VmPeak={}KiB", app.session().document().map_or(0, |d| d.pages.len()), status_kib("VmPeak"));
        println!("WINDOW reserved={}", diacraft_engine::memory::reserved());
    }

    /// Runs `cancelled_child` under the limit on `first`, and then `second` (the same file when there is none): what it printed.
    fn run_cancelled_child(first: &Path, second: Option<&Path>) -> String {
        let exe = std::env::current_exe().unwrap();
        let script = "ulimit -v 3000000 && exec \"$0\" --exact file_open::tests::cancelled_child --ignored --nocapture --test-threads=1";
        let mut command = std::process::Command::new("/bin/sh");
        command.arg("-c").arg(script).arg(exe).env(CHILD_FIRST, first);
        if let Some(second) = second {
            command.env(CHILD_SECOND, second);
        }
        let out = command.output().unwrap();
        let (stdout, stderr) = (String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned());
        assert!(out.status.success(), "the window aborted under `ulimit -v 3000000`: {:?}\n{stdout}\n{stderr}", out.status);
        assert!(stdout.contains("WINDOW reserved=0"), "the reads that ended gave back what they held: {stdout}");
        stdout
    }

    #[test]
    #[cfg_attr(
        target_os = "macos",
        ignore = "macOS cannot limit a process's address space (`ulimit -v` fails there), and this test runs its child under that limit"
    )]
    fn a_small_file_opened_after_a_cancelled_read_of_80000_shapes_waits_for_it_and_is_opened() {
        // The wait is what this holds: the next Open after a Cancel, while the cancelled thread still parses, finds the budget held, and is read when that thread ends (a read that was
        // refused for it, or that took the part of the budget a drawing gave back, is a file that main opens and this does not: the review).
        let dir = std::env::temp_dir().join(format!("diacraft-ui-cancel-small-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (big, small) = (dir.join("big.vsdx"), dir.join("small.vsdx"));
        std::fs::write(&big, diacraft_testkit::package::typical_drawing(20, 4_000, None)).unwrap();
        std::fs::write(&small, diacraft_testkit::package::typical_drawing(1, 10, None)).unwrap();
        let stdout = run_cancelled_child(&big, Some(&small));
        let _ = std::fs::remove_dir_all(&dir);
        let message = stdout.lines().find(|l| l.starts_with("WINDOW message=")).unwrap_or("no message line");
        let state = stdout.lines().find(|l| l.starts_with("WINDOW pages=")).unwrap_or("no state line");
        assert_eq!(message, "WINDOW message=none", "the small file was opened: {stdout}");
        assert!(state.starts_with("WINDOW pages=1 "), "and it is the drawing that is open: {state}");
    }

    #[test]
    #[cfg_attr(
        target_os = "macos",
        ignore = "macOS cannot limit a process's address space (`ulimit -v` fails there), and this test runs its child under that limit"
    )]
    fn a_file_opened_while_a_cancelled_read_still_parses_is_read_or_refused_in_words_and_does_not_abort() {
        // Cancel, then Open, while the cancelled thread still parses (`MAX_READERS` lets four live): each read held nothing until it ended, so both were given the whole budget, and
        // two reads of 80,000 shapes were a process of 3.3 GB that aborted under `ulimit -v 3000000`. A read holds the budget while it runs now.
        let dir = std::env::temp_dir().join(format!("diacraft-ui-cancel-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bytes = diacraft_testkit::package::typical_drawing(20, 4_000, None);
        let path = dir.join("a.vsdx");
        std::fs::write(&path, &bytes).unwrap();
        drop(bytes);
        let stdout = run_cancelled_child(&path, None);
        let _ = std::fs::remove_dir_all(&dir);
        let message = stdout.lines().find(|l| l.starts_with("WINDOW message=")).unwrap_or("no message line");
        let state = stdout.lines().find(|l| l.starts_with("WINDOW pages=")).unwrap_or("no state line");
        // Either the next Open was first to the budget and the drawing is open, or the cancelled read was and the next Open is refused in words: never both read, and never neither.
        assert!(state.contains("pages=20") || (message.contains("a.vsdx") && message.contains("not enough memory")), "{message}\n{state}");
        let kib: u64 = state.split("VmPeak=").nth(1).and_then(|r| r.split("KiB").next()).and_then(|n| n.parse().ok()).unwrap();
        assert!(kib < 3_000_000 / 100 * 90, "the address space reached {kib} KiB of the 3,000,000 the limit allows: {state}");
    }

    #[test]
    #[cfg_attr(
        target_os = "macos",
        ignore = "macOS cannot limit a process's address space (`ulimit -v` fails there), and this test runs its child under that limit"
    )]
    fn a_file_the_open_thread_reads_beside_the_drawing_of_80000_shapes_the_window_holds_is_refused_in_words_and_the_drawing_stays_open() {
        // File > Open, `diacraft FILE` and the control channel's `file.open` read in a session of their own on a thread while the window's session holds the drawing: the second
        // read was given the whole budget, and the process, 3.3 GB, aborted under `ulimit -v 3000000` (the review). The ledger of the engine holds the first.
        let dir = std::env::temp_dir().join(format!("diacraft-ui-open-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bytes = diacraft_testkit::package::typical_drawing(20, 4_000, None);
        let (first, second) = (dir.join("a.vsdx"), dir.join("b.vsdx"));
        std::fs::write(&first, &bytes).unwrap();
        std::fs::write(&second, &bytes).unwrap();
        drop(bytes);
        let exe = std::env::current_exe().unwrap();
        let script = "ulimit -v 3000000 && exec \"$0\" --exact file_open::tests::window_child --ignored --nocapture --test-threads=1";
        let out = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(script)
            .arg(exe)
            .env(CHILD_FIRST, &first)
            .env(CHILD_SECOND, &second)
            .output()
            .unwrap();
        let (stdout, stderr) = (String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned());
        let _ = std::fs::remove_dir_all(&dir);
        assert!(out.status.success(), "the window's open thread aborted under `ulimit -v 3000000`: {:?}\n{stdout}\n{stderr}", out.status);
        let message = stdout.lines().find(|l| l.starts_with("WINDOW message=")).unwrap_or("no message line");
        assert!(message.contains("b.vsdx") && message.contains("not enough memory") && message.contains("still open"), "{message}");
        assert!(!message.contains("diacraft-ui-open") && !message.contains('_'), "nothing of the folder and no class word: {message}");
        let state = stdout.lines().find(|l| l.starts_with("WINDOW pages=")).unwrap_or("no state line");
        assert!(state.contains("pages=20"), "the drawing that was open is open still: {state}");
        let kib: u64 = state.split("VmPeak=").nth(1).and_then(|r| r.split("KiB").next()).and_then(|n| n.parse().ok()).unwrap();
        assert!(kib < 3_000_000 / 100 * 90, "the address space reached {kib} KiB of the 3,000,000 the limit allows: {state}");
    }
}
