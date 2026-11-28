//! The command line: `diacraft [--control <port>] [--reset-graphics] [--version] [--help] [FILE]`.
//!
//! `FILE` is a drawing to open when the window appears. One file, and `--` before it if its name begins with a dash (`--help` and `--version` included: after `--` they are names of files). The window
//! opens it on a thread and says plainly if it cannot; the arguments only keep it.
//!
//! `--control <port>` (or `--control=<port>`, or the `DIACRAFT_CONTROL_PORT` variable when the flag is not
//! given) starts the control server on that port. A port is a decimal number from 1 to 65535; anything
//! else, and an option this program does not have, is an error that stops the start-up before a window
//! opens. Nothing is ignored: a mistyped option that started the app without its control channel would
//! look like a channel that does not answer.
//!
//! `--automation-read-root <folder>` and `--automation-write-root <folder>` (each as often as there are folders, or the `DIACRAFT_AUTOMATION_READ_ROOT` and
//! `DIACRAFT_AUTOMATION_WRITE_ROOT` variables, lists written as `PATH` is, for a kind that no flag names) keep the files the control channel reads and writes to
//! those folders (gaps A2 and A5, `diacraft_engine::file_access`). They are the control channel's, so a flag without `--control` is an error; the variables are
//! left unused by a start with no channel, so that a person who sets them for every start is not stopped by the ones that have none.
//!
//! `--reset-graphics` starts DiaCraft after forgetting the graphics adapters that earlier starts left out because a start ended while testing them
//! (`marker`): every adapter is tested again. It is an option of the start, not a command: the window opens as usual.

use std::ffi::OsString;

use diacraft_engine::file_access::{READ_ENV, READ_FLAG, RootRequest, WRITE_ENV, WRITE_FLAG};
use diacraft_engine::shown;

/// The most characters of an argument an error echoes.
const MOST_SHOWN: usize = 64;

/// The port the documentation and the examples use. Sibling craft apps default to 7979.
pub const DEFAULT_CONTROL_PORT: u16 = 7997;

/// What to do, as the command line says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Start the app.
    Run(Options),
    /// Print the version and exit.
    Version,
    /// Print [`USAGE`] and exit.
    Help,
}

/// What a start of the app is asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    /// The port of the control server, if one was asked for.
    pub control_port: Option<u16>,
    /// The drawing to open, if one was named.
    pub path: Option<String>,
    /// The folders the control channel may read and write: the flags' (or, for a kind that has none, the environment's). Empty: any path the person can reach.
    pub roots: RootRequest,
    /// `--reset-graphics`: forget the adapters earlier starts left out before this start tests any.
    pub reset_graphics: bool,
}

/// The environment variables the command line reads (`DIACRAFT_CONTROL_PORT`, `DIACRAFT_AUTOMATION_READ_ROOT`, `DIACRAFT_AUTOMATION_WRITE_ROOT`); `None`: not set.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Environment {
    pub control_port: Option<OsString>,
    pub read_roots: Option<OsString>,
    pub write_roots: Option<OsString>,
}

impl Environment {
    /// The variables of this process.
    pub fn of_process() -> Environment {
        Environment {
            control_port: std::env::var_os("DIACRAFT_CONTROL_PORT"),
            read_roots: std::env::var_os(READ_ENV),
            write_roots: std::env::var_os(WRITE_ENV),
        }
    }
}

/// `diacraft --help`.
pub const USAGE: &str = "\
Usage: diacraft [options] [FILE]

Opens the DiaCraft window, on FILE (a .vsdx or another drawing file) if one is named, else on an empty drawing.

Options:
  --control <port>   Start the control server on 127.0.0.1:<port>, a port from 1 to 65535
                     (or DIACRAFT_CONTROL_PORT). Examples use 7997. The port has no password:
                     use it only while you drive the app, on a machine you trust.
  --automation-read-root <folder>
                     Keep the files the control channel reads (a drawing, a stencil, a template or a
                     picture that a request names) to this folder and the folders in it. Give it again
                     for more folders. Needs --control. Links and .. are followed before the check, and
                     a refusal says so in words. The window's own Open dialog is not kept to it.
  --automation-write-root <folder>
                     The same for the files the control channel writes (a save, an export, a print, a
                     screenshot). A kind with no folder grants no access of its kind: give both, or
                     give / (and a drive's root on Windows) to keep no limit. With neither flag the
                     channel may reach any path you can.
  --reset-graphics   Start after forgetting the graphics adapters that earlier starts left out. When a
                     start ends (a driver crashes or blocks) while DiaCraft tests an adapter, the next
                     start leaves that adapter out, and says so in the log; this tests it again
  --version          Print the version and exit
  -h, --help         Print this help and exit
  --                 What follows is the file, even if its name begins with a dash (--help too)

Environment:
  DIACRAFT_CONTROL_PORT   The port of --control, when the flag is not given
  DIACRAFT_AUTOMATION_READ_ROOT, DIACRAFT_AUTOMATION_WRITE_ROOT
                          The folders of the two flags above, a list written as PATH is (: between
                          folders, ; on Windows), for the kind that has no flag. Used only with --control
  DIACRAFT_NO_PREFS       Set it (to anything, empty too) to read and write no settings (config.toml, the
                          dictionaries) and keep no log file; log to standard error only. The run keeps
                          what it changes in memory until it ends
  DIACRAFT_CONFIG_DIR     The folder of the settings and of the log, in place of the platform's
                          (~/.config/diacraft on Linux, ~/Library/Application Support/diacraft
                          on macOS, %APPDATA%\\diacraft on Windows). Set but empty, it does what
                          DIACRAFT_NO_PREFS does. A settings file that is broken, too big, from a newer
                          build or not writable is said once in the status bar, and the app goes on
  DIACRAFT_NO_SPLASH      Set it to start without the splash popup (the octopus, the version, the
                          git release and the licence); About DiaCraft in the File view shows it
  DIACRAFT_NO_ERROR_BOX   Set it (to anything) to never open the message box that says DiaCraft could
                          not start (no graphics adapter can show the window, say): the failure goes
                          to the log and standard error, and the exit code is 1. A start with
                          --control opens none either, and neither does one run from a terminal
  WGPU_BACKEND            Which graphics DiaCraft draws with: dx12, vulkan, gl (or metal on a Mac), or
                          a list with commas. Left unset, DiaCraft picks (DirectX 12 before Vulkan
                          on Windows), and falls back to the others when the first does not work.
                          Set, it is the only one tried
  DIACRAFT_GPU_PROBE_FAIL A diagnostic, for trying the defences against a graphics driver that
                          crashes or blocks while DiaCraft tests it: <part of an adapter's name>:abort
                          or :hang makes the test of that adapter end the process, or block (the start
                          gives up after 12 s and leaves it out). The part is looked for, in any case,
                          in the adapter as the log writes it, e.g. UHD:abort, or Intel(R) UHD
                          Graphics (dx12):hang. Start again to see the adapter left out, and use
                          --reset-graphics to test it again. Leave it unset otherwise
";

/// Reads the command line (without the program name) and the `DIACRAFT_CONTROL_PORT` variable: [`parse_with`] with no other variable set.
pub fn parse(args: impl IntoIterator<Item = String>, env_port: Option<OsString>) -> Result<Action, String> {
    parse_with(args, Environment { control_port: env_port, ..Environment::default() })
}

/// Reads the command line (without the program name) and the variables in `env`.
///
/// `--version` and `--help` win over everything else, even a bad port or an option that does not exist, and a
/// bad environment value does not stop them or a `--control` that replaces it. They win before a `--` only: after it they are the names of files.
pub fn parse_with(args: impl IntoIterator<Item = String>, env: Environment) -> Result<Action, String> {
    let args: Vec<String> = args.into_iter().collect();
    // `--` ends the options: after it `--version` and `--help` are the names of files (`diacraft -- --help` opens a drawing named `--help`), so they are looked for before it only.
    let options = args.split(|a| a == "--").next().unwrap_or_default();
    if options.iter().any(|a| a == "--version") {
        return Ok(Action::Version);
    }
    if options.iter().any(|a| a == "--help" || a == "-h") {
        return Ok(Action::Help);
    }
    let mut control_port = None;
    let mut path: Option<String> = None;
    let mut only_files = false;
    let mut roots = RootRequest::default();
    let mut reset_graphics = false;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        // A name that does not begin with a dash is the file; so is anything after `--`.
        if only_files || !arg.starts_with('-') || arg == "-" {
            if path.replace(arg.clone()).is_some() {
                return Err(format!("only one file can be opened, but a second was given: `{}`; see --help", shown(arg, MOST_SHOWN)));
            }
            continue;
        }
        if arg == "--" {
            only_files = true;
            continue;
        }
        if arg == "--reset-graphics" {
            reset_graphics = true;
            continue;
        }
        if roots.flag(arg, &mut rest)? {
            continue;
        }
        let value = match arg.strip_prefix("--control") {
            Some("") => Some(rest.next().map(String::as_str)),
            Some(tail) => tail.strip_prefix('=').map(Some),
            None => None,
        };
        match value {
            Some(value) => {
                if control_port.is_some() {
                    return Err("--control was given twice".to_owned());
                }
                control_port = Some(port("--control", value)?);
            }
            None => return Err(format!("unknown argument `{}`; see --help", shown(arg, MOST_SHOWN))),
        }
    }
    if control_port.is_none() {
        control_port = match env.control_port.filter(|v| !v.is_empty()) {
            Some(value) => Some(port("DIACRAFT_CONTROL_PORT", Some(&value.to_string_lossy()))?),
            None => None,
        };
    }
    // The flags are the control channel's: with none, a flag asks for what the program does not do, and nothing is ignored. The variables are not asked for by this start.
    if control_port.is_none() && !roots.is_empty() {
        return Err(format!(
            "{READ_FLAG} and {WRITE_FLAG} keep the control channel to folders, and there is no control channel: give --control <port> (or set DIACRAFT_CONTROL_PORT)"
        ));
    }
    let roots = roots.with_environment(env.read_roots.as_deref(), env.write_roots.as_deref());
    Ok(Action::Run(Options { control_port, path, roots, reset_graphics }))
}

/// Whether the window shows its splash popup when it starts: not when `DIACRAFT_NO_SPLASH` is set, whatever it holds (as `DIACRAFT_NO_PREFS` is). The
/// splash uses up the first click, so a script that drives a window with clicks sets it. The control channel does not turn it off by itself.
pub fn splash_enabled_with(env: &dyn Fn(&str) -> Option<OsString>) -> bool {
    env("DIACRAFT_NO_SPLASH").is_none()
}

/// [`splash_enabled_with`] for this process's environment.
pub fn splash_enabled() -> bool {
    splash_enabled_with(&|key| std::env::var_os(key))
}

/// A port from 1 to 65535, written in decimal digits and nothing else.
fn port(what: &str, value: Option<&str>) -> Result<u16, String> {
    let Some(text) = value else {
        return Err(format!("{what} needs a port: a number from 1 to 65535"));
    };
    let digits = !text.is_empty() && text.chars().all(|c| c.is_ascii_digit());
    match text.parse::<u16>() {
        Ok(port) if digits && port >= 1 => Ok(port),
        // Arguments and variables are the caller's: a terminal escape in one must not reach the terminal as one.
        _ => Err(format!("{what}: `{}` is not a port: it must be a number from 1 to 65535", shown(text, MOST_SHOWN))),
    }
}
