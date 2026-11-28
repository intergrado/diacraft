//! What DiaCraft tells the person when it cannot start.
//!
//! `diacraft.exe` is a Windows GUI program: it has no console, so a start that failed (a computer with no graphics adapter the window can draw on, a graphics driver that
//! refuses, a bad command line) wrote one line to a standard error nobody could see, and the Start menu entry did nothing at all (the public report that led here). Now
//! every start that fails ends in [`report`]:
//!
//! - the failure is logged once, with every reason;
//! - one line goes to standard error, as it always did;
//! - and, when nothing would show that line, a message box ([`Surroundings::shows_box`]) says in plain words that DiaCraft could not start, why, where the log is and what
//!   to try (update the graphics driver, or pick the graphics API by hand with `WGPU_BACKEND`). The box is the system's own: `rfd`'s message dialog, which is `MessageBoxW` on
//!   Windows, an alert on macOS and zenity on Linux (`dialogs::failure_box`); nothing shows where zenity is missing, and the log and standard error have the text.
//!
//! When the box is shown. Not when a terminal reads standard error (the person has it there), not when `--control` was given (a script drives the app and reads its exit code and
//! the log, and a box would hold it up), not when `DIACRAFT_NO_ERROR_BOX` is set (any value) and not where there is no screen (Linux with no `DISPLAY` or `WAYLAND_DISPLAY`). A
//! mistake in the command line or in a folder is boxed on Windows only: there nothing else would show it, and elsewhere the person who typed it has the terminal.
//!
//! A start that left out graphics adapters because their test did not finish ([`marker`](crate::marker)) names them in the box and says how to test them again
//! (`--reset-graphics`): they are the likely reason no adapter was left.
//!
//! The box goes through a hook ([`report`]'s `show`), so the tests never open one.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use crate::graphics::{BACKEND_ENV, System};

/// The title of the box for a start that could not make its window.
pub const TITLE: &str = "DiaCraft could not start";

/// The title for an internal error (a panic).
pub const STOPPED_TITLE: &str = "DiaCraft stopped";

/// Set to anything to never open the box: the failure is logged and written to standard error, and the exit code says it. For scripts and the tests of the app.
pub const NO_BOX_ENV: &str = "DIACRAFT_NO_ERROR_BOX";

/// The most reasons the box lists (one for each try; the log has them all) and the most characters of each.
const MOST_REASONS: usize = 5;
const MOST_REASON_CHARS: usize = 240;

/// Why DiaCraft could not start.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cause {
    /// The command line was wrong: the sentence the parser gave.
    Arguments(String),
    /// A folder the control channel is to be kept to is not there or not usable: the sentence the check gave.
    Folders(String),
    /// No window could be made: what each try said, in order (the last is the one that ended the start).
    Window(Vec<String>),
    /// A panic, caught before it ended the program: its message.
    Panic(String),
}

/// Where this run's log is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LogPlace {
    /// The log file of this run.
    File(PathBuf),
    /// No file is kept for this run (settings are off, or the folder could not be made).
    Unkept,
}

/// A start that failed, as far as the person is told about it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    pub cause: Cause,
    pub log: LogPlace,
    /// What `WGPU_BACKEND` is set to, when it names a back end: the start did not swap it for another.
    pub forced_backend: Option<String>,
    pub system: System,
    /// The adapters this start left out because their test did not finish (an earlier start ended while testing them, or a test did not answer in time), as the log writes them (`marker::describe`): the box says so, and how to test them again.
    pub left_out: Vec<String>,
}

impl Failure {
    /// The failure of this process: this system, this run's log and the variable `WGPU_BACKEND` as it is set.
    pub fn here(cause: Cause) -> Failure {
        let forced_backend = std::env::var(BACKEND_ENV).ok().map(|v| v.trim().to_owned()).filter(|v| !v.is_empty());
        Failure {
            cause,
            log: crate::logging::log_file().map_or(LogPlace::Unkept, LogPlace::File),
            forced_backend,
            system: System::this(),
            left_out: Vec::new(),
        }
    }

    /// This failure, with the adapters the start left out because an earlier start ended while testing them.
    pub fn with_left_out(mut self, left_out: Vec<String>) -> Failure {
        self.left_out = left_out;
        self
    }

    /// The title of the box.
    pub fn title(&self) -> &'static str {
        match self.cause {
            Cause::Panic(_) => STOPPED_TITLE,
            _ => TITLE,
        }
    }

    /// The exit code: 2 for a folder that cannot be used (as it always was), 1 for everything else.
    pub fn exit_code(&self) -> u8 {
        match self.cause {
            Cause::Folders(_) => 2,
            _ => 1,
        }
    }

    /// The reasons on one line, for standard error and the log.
    pub fn one_line(&self) -> String {
        let joined = match &self.cause {
            Cause::Arguments(e) | Cause::Folders(e) | Cause::Panic(e) => e.clone(),
            Cause::Window(reasons) => reasons.join("; "),
        };
        joined.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// The text of the box: that DiaCraft could not start, why, where the log is and what to try.
    pub fn message(&self) -> String {
        let mut text = String::new();
        match &self.cause {
            Cause::Window(reasons) => {
                text.push_str("DiaCraft could not open its window.\n\nWhat went wrong:\n");
                text.push_str(&reason_lines(reasons));
            }
            Cause::Panic(message) => {
                text.push_str("DiaCraft stopped on an internal error.\n\nWhat went wrong:\n");
                text.push_str(&reason_lines(std::slice::from_ref(message)));
                text.push_str("\nThis is a bug in DiaCraft. Please report it, with the log.\n");
            }
            Cause::Arguments(e) => {
                text.push_str("DiaCraft was started with something it does not understand, so it did not open.\n\nWhat went wrong:\n");
                text.push_str(&reason_lines(std::slice::from_ref(e)));
            }
            Cause::Folders(e) => {
                text.push_str("DiaCraft was asked to keep its control channel to folders it cannot use, so it did not open.\n\nWhat went wrong:\n");
                text.push_str(&reason_lines(std::slice::from_ref(e)));
            }
        }
        if matches!(self.cause, Cause::Window(_) | Cause::Panic(_)) {
            text.push_str(&self.log_paragraph());
        }
        if matches!(self.cause, Cause::Window(_)) {
            if let Some(backend) = &self.forced_backend {
                text.push_str(&format!(
                    "\n{BACKEND_ENV} is set to \"{backend}\", so DiaCraft tried only that kind of graphics and did not try the others. Remove it, or set it to another one, and start DiaCraft again.\n"
                ));
            }
            text.push_str(&self.left_out_paragraph());
            text.push_str(&advice(self.system));
        }
        text
    }

    /// The paragraph about the adapters left out, when there are any: they are why a start may have no adapter left, and `--reset-graphics` tests them again.
    fn left_out_paragraph(&self) -> String {
        if self.left_out.is_empty() {
            return String::new();
        }
        let program = if self.system == System::Windows { "diacraft.exe" } else { "diacraft" };
        let (test, object) = if self.left_out.len() == 1 { ("its test did", "it") } else { ("their tests did", "them") };
        format!(
            "\nDiaCraft left out {} because {test} not finish: a start ended in {object}, or {object} did not answer in time, as a graphics driver that crashes or blocks does. To test {object} again, start DiaCraft with  {program} --reset-graphics .\n",
            self.left_out.join("; ")
        )
    }

    fn log_paragraph(&self) -> String {
        match &self.log {
            LogPlace::File(path) => {
                format!("\nThe log of this start is in:\n{}\nIt says more. Please attach it if you report this.\n", path.display())
            }
            LogPlace::Unkept => "\nDiaCraft keeps no log file for this run, so what is written above is all there is.\n".to_owned(),
        }
    }
}

/// The reasons as `- ` lines, each cut to a readable length, at most [`MOST_REASONS`] of them and a line saying how many more there were.
fn reason_lines(reasons: &[String]) -> String {
    let mut lines = String::new();
    for reason in reasons.iter().take(MOST_REASONS) {
        // wgpu ends "Failed to create surface for any enabled backend" with the empty list of what each back end said: `{}`, which tells the person nothing.
        let flat = reason.split_whitespace().collect::<Vec<_>>().join(" ");
        let flat = flat.strip_suffix(" {}").map_or(flat.as_str(), |kept| kept.trim_end_matches(':').trim_end()).to_owned();
        let shown: String = flat.chars().take(MOST_REASON_CHARS).collect();
        let cut = if flat.chars().count() > MOST_REASON_CHARS { "…" } else { "" };
        lines.push_str(&format!("- {shown}{cut}\n"));
    }
    let more = reasons.len().saturating_sub(MOST_REASONS);
    if more > 0 {
        lines.push_str(&format!("(and {more} more, in the log)\n"));
    }
    lines
}

/// What to try, for a start that could not make its window.
fn advice(system: System) -> String {
    let steps = match system {
        System::Windows => {
            "- Update the graphics driver (from the maker of the graphics card, or of the computer), then start DiaCraft again.\n\
             - Or pick the graphics by hand. In a Command Prompt, run  set WGPU_BACKEND=dx12  and then  diacraft.exe . If that fails too, try  set WGPU_BACKEND=gl  instead.\n"
        }
        System::MacOs => "- Update macOS, then start DiaCraft again.\n- If it still does not start, report it with the log.\n",
        System::Other => {
            "- Install or update the graphics driver (Mesa, or the maker's), then start DiaCraft again.\n\
             - Or pick the graphics by hand. From a terminal, run  WGPU_BACKEND=gl diacraft  or  WGPU_BACKEND=vulkan diacraft .\n"
        }
    };
    format!("\nWhat to try:\n{steps}")
}

/// What a start can tell about the place it runs in, for [`Surroundings::shows_box`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Surroundings {
    pub system: System,
    /// Whether standard error is a terminal: somebody is reading it.
    pub stderr_is_terminal: bool,
    /// [`NO_BOX_ENV`] is set.
    pub opted_out: bool,
    /// `--control` was given: a script drives the app.
    pub scripted: bool,
    /// There is a screen to show a box on (always on Windows and macOS; on Linux a `DISPLAY` or a `WAYLAND_DISPLAY`).
    pub has_display: bool,
}

impl Surroundings {
    /// The surroundings of this process; `scripted` is whether `--control` was given.
    pub fn of_process(scripted: bool) -> Surroundings {
        use std::io::IsTerminal;
        let set = |name: &str| std::env::var_os(name).is_some_and(|v| !v.is_empty());
        let system = System::this();
        Surroundings {
            system,
            stderr_is_terminal: std::io::stderr().is_terminal(),
            opted_out: std::env::var_os(NO_BOX_ENV).is_some(),
            scripted,
            has_display: system != System::Other || set("DISPLAY") || set("WAYLAND_DISPLAY"),
        }
    }

    /// Whether `cause` is told in a message box here. A mistake in the command line or in a folder only on Windows, where nothing else would show it.
    pub fn shows_box(&self, cause: &Cause) -> bool {
        let command_line = matches!(cause, Cause::Arguments(_) | Cause::Folders(_));
        if command_line && self.system != System::Windows {
            return false;
        }
        !self.stderr_is_terminal && !self.opted_out && !self.scripted && self.has_display
    }
}

/// Tells the person that DiaCraft could not start: logs `failure` once, writes one line to `stderr`, and, where [`Surroundings::shows_box`] says so, asks `show` (title,
/// text) to open the message box. `show` is the seam: the program passes `dialogs::failure_box`, a test passes a recorder.
pub fn report(failure: &Failure, around: &Surroundings, show: &dyn Fn(&str, &str), stderr: &mut dyn Write) {
    log::error!("{}: {}", failure.title(), failure.one_line());
    let log_note = match &failure.log {
        LogPlace::File(path) => format!(" (the log is {})", path.display()),
        LogPlace::Unkept => String::new(),
    };
    let line = match &failure.cause {
        Cause::Window(_) => format!("diacraft: the window could not run: {}{log_note}\n", failure.one_line()),
        Cause::Panic(_) => format!("diacraft: stopped on an internal error: {}{log_note}; please report this bug\n", failure.one_line()),
        Cause::Arguments(_) | Cause::Folders(_) => format!("diacraft: {}\n", failure.one_line()),
    };
    // Nobody to read it (a Windows GUI program, a closed pipe) is not an error worth another message.
    let _ = stderr.write_all(line.as_bytes());
    if around.shows_box(&failure.cause) {
        // The system's dialog is code this program does not own: a panic in it must not turn a start that failed into a crash with nothing said (`catch_panic` logs it).
        let _ = diacraft_engine::guard::catch_panic(|| show(failure.title(), &failure.message()));
    }
}

/// [`report`] for this process, with the system's message box; the exit code to return. `scripted` is whether `--control` was given.
pub fn exit_with(cause: Cause, scripted: bool) -> ExitCode {
    exit_with_left_out(cause, scripted, Vec::new())
}

/// [`exit_with`] for a start that left out the adapters `left_out` (`marker::describe`): the box names them.
pub fn exit_with_left_out(cause: Cause, scripted: bool, left_out: Vec<String>) -> ExitCode {
    let failure = Failure::here(cause).with_left_out(left_out);
    report(&failure, &Surroundings::of_process(scripted), &crate::dialogs::failure_box, &mut std::io::stderr());
    ExitCode::from(failure.exit_code())
}
