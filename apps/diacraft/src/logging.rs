//! The desktop app's logger: `log::` records go to standard error and to a log file.
//!
//! Without a logger every `log::warn!` and `log::error!` in the workspace (a panic the engine's guard
//! recovered from, a control port that was busy) would vanish, and a launch from a desktop menu has no
//! terminal. So the file is what a bug report can attach: `<config>/diacraft/logs/diacraft.log`, where
//! `<config>` is the platform's settings folder (`$XDG_CONFIG_HOME` or `~/.config`; `~/Library/Application
//! Support`; `%APPDATA%`). Each start moves the previous log to `diacraft.1.log` (and that one to `.2`), so the
//! log of a run that crashed survives the next two starts, and the file stops growing at [`MAX_FILE_BYTES`].
//! A run with `DIACRAFT_NO_PREFS` set, or with an empty `DIACRAFT_CONFIG_DIR`, keeps no file: it logs to standard error only, so a test run does not
//! rotate away the logs of the person's own runs. A `DIACRAFT_CONFIG_DIR` that names a folder moves the settings there, and the log goes to its `logs` folder.
//!
//! Levels: `info` for DiaCraft's own crates and `warn` for everything else (wgpu and naga are chatty).
//! `RUST_LOG` replaces that with env_logger-style directives: `debug`, `warn,diacraft_engine=trace`. A
//! directive ending in `*` matches every target that starts with it (`diacraft*=debug`).
//!
//! Records logged before the log folder is known are kept (up to [`MAX_PENDING`]) and written once the file
//! is attached. Writing never panics: a file that cannot be made or written leaves standard error the only sink.
//!
//! A record is handed to the system as it is logged, so a program that crashes leaves its log up to the last record. `log::logger().flush()` ([`AppLogger::flush`]) also has the system write
//! the file to the disk, which is what a machine that goes down (a graphics driver's reset that ends in a blue screen) needs: the start calls it before it tests a graphics adapter
//! So that the next report shows which one the start died on.

use std::ffi::OsString;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::SystemTime;

use diacraft_engine::shown;
use log::LevelFilter;

/// The most characters of a record's message one line holds, as in the CLI's logger.
const MOST_CHARS: usize = 2000;

/// The current log file's name inside the log folder.
pub const LOG_FILE: &str = "diacraft.log";

/// How many earlier runs are kept (`diacraft.1.log` to `diacraft.<KEEP>.log`).
pub const KEEP: usize = 2;

/// The log file stops growing past this size, so a runaway warning cannot fill the disk.
pub const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;

/// Records kept in memory until the log file is attached.
pub const MAX_PENDING: usize = 512;

/// The filter when `RUST_LOG` is unset or empty.
pub const DEFAULT_FILTER: &str = "warn,diacraft*=info";

/// A level for each target, from env_logger-style directives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Filter {
    default: LevelFilter,
    directives: Vec<(String, LevelFilter)>,
}

impl Filter {
    /// Reads `spec`. An unknown level and an empty part are skipped. Without a bare level, targets that no
    /// directive names log errors only.
    pub fn parse(spec: &str) -> Filter {
        let mut filter = Filter { default: LevelFilter::Error, directives: Vec::new() };
        for part in spec.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            match part.split_once('=') {
                Some((name, level)) => {
                    let name = name.trim();
                    if let (false, Some(level)) = (name.is_empty(), parse_level(level)) {
                        filter.directives.push((name.to_owned(), level));
                    }
                }
                None => match parse_level(part) {
                    Some(level) => filter.default = level,
                    // A bare target name: everything from it (env_logger does the same).
                    None => filter.directives.push((part.to_owned(), LevelFilter::Trace)),
                },
            }
        }
        filter
    }

    /// The level that applies to `target`: the most specific matching directive wins.
    pub fn level_for(&self, target: &str) -> LevelFilter {
        self.directives.iter().filter(|(name, _)| matches(name, target)).max_by_key(|(name, _)| name.len()).map_or(self.default, |(_, level)| *level)
    }

    /// The most verbose level any target can reach (for `log::set_max_level`).
    pub fn max(&self) -> LevelFilter {
        self.directives.iter().map(|(_, level)| *level).fold(self.default, Ord::max)
    }
}

fn parse_level(text: &str) -> Option<LevelFilter> {
    text.trim().parse().ok()
}

/// `name` matches `target` itself and its submodules (`a` matches `a` and `a::b`, not `ab`); `name*` matches
/// every target that starts with `name`.
fn matches(name: &str, target: &str) -> bool {
    match name.strip_suffix('*') {
        Some(prefix) => target.starts_with(prefix),
        None => target.strip_prefix(name).is_some_and(|rest| rest.is_empty() || rest.starts_with("::")),
    }
}

/// `2026-10-08T07:59:17.728Z`: UTC, with milliseconds. A clock before 1970 reads as the epoch. A clock past
/// the years jiff holds (to 9999) still converts, because the calendar repeats every 400 years: jiff gives the month,
/// day and time, and only the year carries the cycles.
pub fn timestamp(t: SystemTime) -> String {
    let since = t.duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    // `as_secs` is a `u64`; a value past `i64::MAX` seconds saturates rather than falling back to the epoch.
    let secs = i64::try_from(since.as_secs()).unwrap_or(i64::MAX);
    let (year, time) = utc_parts(secs, since.subsec_nanos());
    // The text of a civil date and a format of this program's own cannot fail; the epoch's text rather than a panic if it ever did.
    let rest = jiff::fmt::strtime::format("%m-%dT%H:%M:%S%.3fZ", time).unwrap_or_else(|_| "01-01T00:00:00.000Z".to_owned());
    // `{:04}` rather than `%Y`: the year is not jiff's (it carries the cycles), and `%Y` stops at 9999.
    format!("{year:04}-{rest}")
}

/// Unix seconds and nanoseconds to the proleptic Gregorian year, and the same wall-clock instant moved into
/// jiff's range by whole 400-year cycles (146,097 days each, after which the calendar repeats). Month, day
/// and time of day come from jiff, so every `i64` is exact and no calendar logic is written here.
fn utc_parts(secs: i64, nanos: u32) -> (i64, jiff::civil::DateTime) {
    use jiff::civil::{Date, DateTime, Time};
    let (days, second_of_day) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let (cycles, rest) = (days.div_euclid(146_097), days.rem_euclid(146_097));
    // 1970-01-01 and `rest` days (under 146,097) is a day of 1970 to 2369, always a day jiff has, and the second
    // of the day is under 86,400.
    let date = jiff::Span::new().try_days(rest).ok().and_then(|days| Date::constant(1970, 1, 1).checked_add(days).ok());
    let time = (|| {
        let (hour, minute, second) = (second_of_day / 3600, second_of_day % 3600 / 60, second_of_day % 60);
        Time::new(i8::try_from(hour).ok()?, i8::try_from(minute).ok()?, i8::try_from(second).ok()?, i32::try_from(nanos.min(999_999_999)).ok()?).ok()
    })();
    match (date, time) {
        (Some(date), Some(time)) => {
            let year = i128::from(date.year()) + i128::from(cycles) * 400;
            (i64::try_from(year).unwrap_or(if year < 0 { i64::MIN } else { i64::MAX }), date.to_datetime(time))
        }
        // Not reachable by the reasoning above; a timestamp that reads as the epoch is better than a panic.
        _ => (1970, DateTime::constant(1970, 1, 1, 0, 0, 0, 0)),
    }
}

fn numbered(dir: &Path, n: usize) -> PathBuf {
    if n == 0 { dir.join(LOG_FILE) } else { dir.join(format!("diacraft.{n}.log")) }
}

/// Moves the earlier logs up one (`.log` to `.1.log` to … `.<KEEP>.log`, the oldest dropped) and makes a fresh
/// empty log file in `dir`, which is made if it is missing.
pub fn rotate(dir: &Path) -> std::io::Result<(PathBuf, File)> {
    std::fs::create_dir_all(dir)?;
    for n in (1..=KEEP).rev() {
        match std::fs::rename(numbered(dir, n - 1), numbered(dir, n)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
            // Moved, or there was no such log yet.
            _ => {}
        }
    }
    let path = numbered(dir, 0);
    let file = File::create(&path)?;
    Ok((path, file))
}

/// One log line: `<time> <LEVEL> [<thread>] <target>: <message>`. A record may repeat text a client sent, so the message is
/// what [`shown`] makes it, as in the CLI's logger: control, line-separator and direction characters escaped, so a record is
/// one line and cannot fake another or move a terminal's cursor, and at most 2,000 characters. The newline a message ends with
/// is the end of the record, not part of it.
pub fn format_line(time: &str, level: log::Level, thread: &str, target: &str, message: &str) -> String {
    let message = shown(message.strip_suffix('\n').unwrap_or(message), MOST_CHARS);
    format!("{time} {level:<5} [{thread}] {target}: {message}\n")
}

/// Where lines go besides standard error: kept in memory until a file is attached, then appended to it until
/// it reaches its size limit.
pub struct Sink {
    file: Option<File>,
    pending: Vec<String>,
    dropped: usize,
    written: u64,
    max_bytes: u64,
    capped: bool,
}

impl Sink {
    pub fn new(max_bytes: u64) -> Sink {
        Sink { file: None, pending: Vec::new(), dropped: 0, written: 0, max_bytes, capped: false }
    }

    /// Appends `line` to the file, or keeps it until [`Sink::attach`].
    pub fn write(&mut self, line: &str) {
        if self.file.is_none() {
            if self.capped {
                // No file is coming ([`Sink::no_file`]), and standard error had the line.
            } else if self.pending.len() < MAX_PENDING {
                self.pending.push(line.to_owned());
            } else {
                self.dropped = self.dropped.saturating_add(1);
            }
            return;
        }
        self.append(line);
    }

    /// Starts writing to `file`, a fresh one (the size limit starts over), first the lines kept so far.
    pub fn attach(&mut self, file: File) {
        self.file = Some(file);
        self.written = 0;
        self.capped = false;
        for line in std::mem::take(&mut self.pending) {
            self.append(&line);
        }
        if self.dropped > 0 {
            let note = format!("{} earlier log lines were dropped before the log file was opened\n", self.dropped);
            self.dropped = 0;
            self.append(&note);
        }
    }

    /// There will be no log file: forgets the lines kept for it and keeps no more.
    pub fn no_file(&mut self) {
        self.pending = Vec::new();
        self.dropped = 0;
        self.capped = true;
    }

    /// Has the system write what the file holds to the disk. A file that cannot be flushed is no reason to stop: the lines are in the system's hands already.
    pub fn sync(&mut self) {
        if let Some(file) = self.file.as_ref() {
            let _ = file.sync_data();
        }
    }

    fn append(&mut self, line: &str) {
        if self.capped {
            return;
        }
        let Some(file) = self.file.as_mut() else { return };
        let len = u64::try_from(line.len()).unwrap_or(u64::MAX);
        let text = if self.written.saturating_add(len) > self.max_bytes {
            self.capped = true;
            format!("log file reached {} bytes; later records go to standard error only\n", self.max_bytes)
        } else {
            self.written = self.written.saturating_add(len);
            line.to_owned()
        };
        // A full disk or a vanished file must not take the app down: the file sink is dropped.
        if file.write_all(text.as_bytes()).is_err() {
            self.file = None;
            self.capped = true;
        }
    }
}

/// The installed logger: a [`Filter`], standard error and a [`Sink`].
pub struct AppLogger {
    filter: Filter,
    stderr: bool,
    sink: Mutex<Sink>,
    /// The log file this run made, for the message of a start that failed (`failure`).
    file: Mutex<Option<PathBuf>>,
}

impl AppLogger {
    pub fn new(filter: Filter, stderr: bool) -> AppLogger {
        AppLogger { filter, stderr, sink: Mutex::new(Sink::new(MAX_FILE_BYTES)), file: Mutex::new(None) }
    }

    /// The log file [`AppLogger::attach_dir`] made; `None` before it, after an error, and when the run keeps no file.
    pub fn file(&self) -> Option<PathBuf> {
        self.file.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// Rotates the logs in `dir` and sends records to the fresh file; returns its path. On an error, standard
    /// error stays the only sink.
    pub fn attach_dir(&self, dir: &Path) -> Result<PathBuf, String> {
        // A thread that panicked while logging leaves the sink usable: take it back.
        let mut sink = self.sink.lock().unwrap_or_else(PoisonError::into_inner);
        match rotate(dir) {
            Ok((path, file)) => {
                sink.attach(file);
                *self.file.lock().unwrap_or_else(PoisonError::into_inner) = Some(path.clone());
                Ok(path)
            }
            Err(e) => {
                sink.no_file();
                *self.file.lock().unwrap_or_else(PoisonError::into_inner) = None;
                Err(format!("{}: {e}", dir.display()))
            }
        }
    }

    /// No log file for this run: standard error only.
    pub fn no_file(&self) {
        self.sink.lock().unwrap_or_else(PoisonError::into_inner).no_file();
        *self.file.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }
}

impl log::Log for AppLogger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= self.filter.level_for(metadata.target())
    }

    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        // Formatted before the sink is locked, so a message that logs while it is formatted cannot deadlock.
        let thread = std::thread::current();
        let line =
            format_line(&timestamp(SystemTime::now()), record.level(), thread.name().unwrap_or("?"), record.target(), &record.args().to_string());
        if self.stderr {
            // No terminal (a Windows GUI build, a closed pipe) is not an error worth reporting.
            let _ = std::io::stderr().write_all(line.as_bytes());
        }
        self.sink.lock().unwrap_or_else(PoisonError::into_inner).write(&line);
    }

    fn flush(&self) {
        self.sink.lock().unwrap_or_else(PoisonError::into_inner).sync();
    }
}

/// Installs the logger (filter from `RUST_LOG`, else [`DEFAULT_FILTER`]). `None` when another logger was
/// installed first. Call [`AppLogger::attach_dir`] once the log folder is known.
pub fn install() -> Option<&'static AppLogger> {
    let spec = std::env::var("RUST_LOG").ok().filter(|s| !s.trim().is_empty());
    let filter = Filter::parse(spec.as_deref().unwrap_or(DEFAULT_FILTER));
    let max = filter.max();
    let logger = LOGGER.get_or_init(|| AppLogger::new(filter, true));
    log::set_logger(logger).ok()?;
    log::set_max_level(max);
    Some(logger)
}

/// Where the logs go on `os` (as `std::env::consts::OS` names it), given the environment: `<config>/diacraft/logs`, or `<dir>/logs` when `DIACRAFT_CONFIG_DIR` names the settings'
/// folder (craft-settings' variable: the settings and the log are in one folder). `None` without a home to put them in.
pub fn log_dir_with(os: &str, env: &dyn Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    if let Some(dir) = env("DIACRAFT_CONFIG_DIR").filter(|dir| !dir.is_empty()) {
        return Some(PathBuf::from(dir).join("logs"));
    }
    let config = match os {
        "macos" => env("HOME").map(|home| PathBuf::from(home).join("Library").join("Application Support")),
        "windows" => env("APPDATA").map(PathBuf::from),
        // The XDG specification says a relative or empty `XDG_CONFIG_HOME` is invalid and to be ignored.
        _ => env("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| env("HOME").map(|home| PathBuf::from(home).join(".config"))),
    };
    config.map(|c| c.join("diacraft").join("logs"))
}

/// The installed logger ([`install`]); `None` before it.
static LOGGER: OnceLock<AppLogger> = OnceLock::new();

/// The log file this run's logger made, for the message of a start that failed (`failure`): `None` before the logger is installed and attached, and when the run keeps no file.
pub fn log_file() -> Option<PathBuf> {
    LOGGER.get()?.file()
}

/// Where this run's logs go.
pub fn log_dir() -> Option<PathBuf> {
    log_dir_with(std::env::consts::OS, &|key| std::env::var_os(key))
}

/// Whether this run may read and write preferences and keep a log file: not when `DIACRAFT_NO_PREFS` is set, whatever it holds, and not when `DIACRAFT_CONFIG_DIR` is set and empty
/// (craft-settings' way of saying the same: nothing is read or written).
pub fn prefs_enabled_with(env: &dyn Fn(&str) -> Option<OsString>) -> bool {
    env("DIACRAFT_NO_PREFS").is_none() && env("DIACRAFT_CONFIG_DIR").is_none_or(|dir| !dir.is_empty())
}

/// [`prefs_enabled_with`] for this process's environment.
pub fn prefs_enabled() -> bool {
    prefs_enabled_with(&|key| std::env::var_os(key))
}
