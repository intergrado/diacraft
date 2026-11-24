//! The command line: `diacraft [--control <port>] [--version] [--help] [FILE]`.
//!
//! `FILE` is a drawing to open when the window appears. One file, and `--` before it if its name begins with a dash. The window
//! opens it on a thread and says plainly if it cannot; the arguments only keep it.
//!
//! `--control <port>` (or `--control=<port>`, or the `DIACRAFT_CONTROL_PORT` variable when the flag is not
//! given) starts the control server on that port. A port is a decimal number from 1 to 65535; anything
//! else, and an option this program does not have, is an error that stops the start-up before a window
//! opens. Nothing is ignored: a mistyped option that started the app without its control channel would
//! look like a channel that does not answer.

use std::ffi::OsString;

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
}

/// `diacraft --help`.
pub const USAGE: &str = "\
Usage: diacraft [options] [FILE]

Opens the DiaCraft window, on FILE (a .vsdx or another drawing file) if one is named, else on an empty drawing.

Options:
  --control <port>   Start the control server on 127.0.0.1:<port>, a port from 1 to 65535
                     (or DIACRAFT_CONTROL_PORT). Examples use 7997. The port has no password:
                     use it only while you drive the app, on a machine you trust.
  --version          Print the version and exit
  -h, --help         Print this help and exit
  --                 What follows is the file, even if its name begins with a dash

Environment:
  DIACRAFT_CONTROL_PORT   The port of --control, when the flag is not given
  DIACRAFT_NO_PREFS       Set it to read and write no preferences and keep no log file; log to
                          standard error only
  DIACRAFT_NO_SPLASH      Set it to start without the splash popup (the octopus, the version, the
                          git release and the licence); About DiaCraft in the File view shows it
";

/// Reads the command line (without the program name) and the `DIACRAFT_CONTROL_PORT` variable.
///
/// `--version` and `--help` win over everything else, even a bad port or an option that does not exist, and a
/// bad environment value does not stop them or a `--control` that replaces it.
pub fn parse(args: impl IntoIterator<Item = String>, env_port: Option<OsString>) -> Result<Action, String> {
    let args: Vec<String> = args.into_iter().collect();
    if args.iter().any(|a| a == "--version") {
        return Ok(Action::Version);
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        return Ok(Action::Help);
    }
    let mut control_port = None;
    let mut path: Option<String> = None;
    let mut only_files = false;
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
        control_port = match env_port.filter(|v| !v.is_empty()) {
            Some(value) => Some(port("DIACRAFT_CONTROL_PORT", Some(&value.to_string_lossy()))?),
            None => None,
        };
    }
    Ok(Action::Run(Options { control_port, path }))
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
