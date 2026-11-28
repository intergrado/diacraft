//! The desktop app's logger: filters, the log file's place, rotation, its size limit.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use diacraft_desktop::logging::{
    AppLogger, DEFAULT_FILTER, Filter, KEEP, LOG_FILE, MAX_FILE_BYTES, MAX_PENDING, Sink, format_line, log_dir_with, prefs_enabled_with, rotate,
    timestamp,
};
use log::{Level, LevelFilter, Log};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> TempDir {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("diacraft-log-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

fn env(pairs: &[(&'static str, &'static str)]) -> impl Fn(&str) -> Option<OsString> {
    let pairs: Vec<(&'static str, &'static str)> = pairs.to_vec();
    move |key| pairs.iter().find(|(k, _)| *k == key).map(|(_, v)| OsString::from(*v))
}

#[test]
fn the_documented_limits() {
    assert_eq!(KEEP, 2, "two older runs are kept");
    assert_eq!(MAX_FILE_BYTES, 16 * 1024 * 1024);
    assert_eq!(LOG_FILE, "diacraft.log");
}

#[test]
fn the_log_lives_in_the_config_folder_under_diacraft_logs() {
    let linux = log_dir_with("linux", &env(&[("XDG_CONFIG_HOME", "/xdg"), ("HOME", "/home/u")]));
    assert_eq!(linux, Some(PathBuf::from("/xdg/diacraft/logs")));
    let linux = log_dir_with("linux", &env(&[("HOME", "/home/u")]));
    assert_eq!(linux, Some(PathBuf::from("/home/u/.config/diacraft/logs")));
    // A relative or empty XDG_CONFIG_HOME is not a place to write: the specification says to ignore it.
    for bad in ["", "relative/dir"] {
        let other = log_dir_with("other", &env(&[("XDG_CONFIG_HOME", bad), ("HOME", "/home/u")]));
        assert_eq!(other, Some(PathBuf::from("/home/u/.config/diacraft/logs")), "{bad:?}");
    }
    let mac = log_dir_with("macos", &env(&[("HOME", "/Users/u")]));
    assert_eq!(mac, Some(PathBuf::from("/Users/u/Library/Application Support/diacraft/logs")));
    let windows = log_dir_with("windows", &env(&[("APPDATA", "C:\\Users\\u\\AppData\\Roaming")]));
    assert_eq!(windows, Some(PathBuf::from("C:\\Users\\u\\AppData\\Roaming").join("diacraft").join("logs")));
    assert_eq!(log_dir_with("linux", &env(&[])), None, "no home, no place: standard error only");
    assert_eq!(log_dir_with("windows", &env(&[("HOME", "/h")])), None);
}

#[test]
fn no_prefs_means_no_log_file() {
    assert!(prefs_enabled_with(&env(&[])));
    assert!(!prefs_enabled_with(&env(&[("DIACRAFT_NO_PREFS", "1")])));
    assert!(!prefs_enabled_with(&env(&[("DIACRAFT_NO_PREFS", "")])), "set at all, whatever it holds");
}

/// The settings' folder is the log's. `DIACRAFT_CONFIG_DIR` (craft-settings' variable) moves both, and set but empty keeps a program away from both, as
/// `DIACRAFT_NO_PREFS` does: an agent that isolates itself that way must not rotate away the logs of the person's own runs either.
#[test]
fn the_config_dir_variable_moves_the_log_and_empty_means_none() {
    let moved = log_dir_with("linux", &env(&[("DIACRAFT_CONFIG_DIR", "/elsewhere"), ("HOME", "/home/u")]));
    assert_eq!(moved, Some(PathBuf::from("/elsewhere/logs")));
    let windows = log_dir_with("windows", &env(&[("DIACRAFT_CONFIG_DIR", "D:\\cfg"), ("APPDATA", "C:\\a")]));
    assert_eq!(windows, Some(PathBuf::from("D:\\cfg").join("logs")));
    // Empty is not a place: the platform's folder is what is left, and `prefs_enabled_with` says not to use it.
    let empty = log_dir_with("linux", &env(&[("DIACRAFT_CONFIG_DIR", ""), ("HOME", "/home/u")]));
    assert_eq!(empty, Some(PathBuf::from("/home/u/.config/diacraft/logs")));
    assert!(!prefs_enabled_with(&env(&[("DIACRAFT_CONFIG_DIR", "")])), "isolated: no settings and no log file");
    assert!(prefs_enabled_with(&env(&[("DIACRAFT_CONFIG_DIR", "/elsewhere")])), "a folder of its own is a place to write");
}

#[test]
fn the_filter_picks_the_most_specific_directive() {
    let f = Filter::parse(DEFAULT_FILTER);
    assert_eq!(f.level_for("diacraft_engine"), LevelFilter::Info);
    assert_eq!(f.level_for("diacraft_ui_egui::app"), LevelFilter::Info);
    assert_eq!(f.level_for("wgpu_core"), LevelFilter::Warn, "everything else only warns: wgpu and naga are chatty");
    let f = Filter::parse("warn,wgpu_core=info,wgpu_core::device=trace,other*=debug, bad=nonsense ,=error");
    assert_eq!(f.level_for("wgpu_core"), LevelFilter::Info);
    assert_eq!(f.level_for("wgpu_core::device::queue"), LevelFilter::Trace);
    assert_eq!(f.level_for("wgpu_corefoo"), LevelFilter::Warn, "a name matches a module, not a longer name");
    assert_eq!(f.level_for("others_here"), LevelFilter::Debug, "a trailing * matches every target that starts with it");
    assert_eq!(f.max(), LevelFilter::Trace);
    // With no bare level, targets no directive names log errors only.
    assert_eq!(Filter::parse("diacraft=debug").level_for("elsewhere"), LevelFilter::Error);
    assert_eq!(Filter::parse("").level_for("x"), LevelFilter::Error);
}

#[test]
fn timestamps_are_utc_with_milliseconds() {
    assert_eq!(timestamp(UNIX_EPOCH), "1970-01-01T00:00:00.000Z");
    assert_eq!(timestamp(UNIX_EPOCH + Duration::new(1_000_000_000, 123_456_789)), "2001-09-09T01:46:40.123Z");
    // The leap day, and the end of a year.
    assert_eq!(timestamp(UNIX_EPOCH + Duration::from_secs(1_709_164_800)), "2024-02-29T00:00:00.000Z");
    assert_eq!(timestamp(UNIX_EPOCH + Duration::from_secs(1_767_225_599)), "2025-12-31T23:59:59.000Z");
    // A clock before 1970 reads as the epoch rather than failing (where the system can hold such a time at all).
    if let Some(before) = UNIX_EPOCH.checked_sub(Duration::from_secs(1)) {
        assert_eq!(timestamp(before), "1970-01-01T00:00:00.000Z");
    }
}

/// A clock past the years jiff can hold (9999) still gives a date, and never panics.
#[test]
fn a_clock_far_in_the_future_still_converts() {
    let year_10000 = UNIX_EPOCH.checked_add(Duration::from_secs(253_402_300_800)).unwrap();
    assert_eq!(timestamp(year_10000), "10000-01-01T00:00:00.000Z");
    // 400 Gregorian years are 12_622_780_800 seconds, and the calendar repeats after them.
    let later = SystemTime::checked_add(&UNIX_EPOCH, Duration::from_secs(12_622_780_800 * 1_000)).unwrap();
    assert_eq!(timestamp(later), "401970-01-01T00:00:00.000Z");
    // Whatever the system can represent converts.
    for secs in [u64::from(u32::MAX), 1 << 40, 1 << 50, u64::MAX / 4] {
        if let Some(t) = UNIX_EPOCH.checked_add(Duration::from_secs(secs)) {
            assert!(timestamp(t).ends_with('Z'));
        }
    }
}

/// Chrono to jiff: the log's timestamp at the edges of jiff's years and of the 400-year cycles, pinned before the change (what chrono gave) and unchanged by it. The text
/// of a clock past the year 9999 is the date of the same day of the cycle with the cycles added back to the year, so none of it depends on the range of the date library.
#[test]
fn the_timestamp_at_the_edges_of_the_years_and_of_the_cycles_is_what_it_was() {
    for (secs, nanos, want) in [
        (253_402_300_799u64, 999_999_999u32, "9999-12-31T23:59:59.999Z"),
        (253_402_300_800, 0, "10000-01-01T00:00:00.000Z"),
        (12_622_780_799, 0, "2369-12-31T23:59:59.000Z"),
        (12_622_780_800, 0, "2370-01-01T00:00:00.000Z"),
        (13_574_563_200, 5_000_000, "2400-02-29T00:00:00.005Z"),
        (12_622_780_800_000, 0, "401970-01-01T00:00:00.000Z"),
        (1 << 40, 7, "36812-02-20T00:36:16.000Z"),
        (1 << 50, 999_999_999, "35680317-09-25T18:57:04.999Z"),
        (u64::MAX / 4, 123_456_789, "146138514283-06-19T07:45:03.123Z"),
        (i64::MAX as u64, 999_999_999, "292277026596-12-04T15:30:07.999Z"),
    ] {
        // A system clock that cannot hold the instant has nothing to convert.
        if let Some(t) = UNIX_EPOCH.checked_add(Duration::new(secs, nanos)) {
            assert_eq!(timestamp(t), want, "{secs} s and {nanos} ns");
        }
    }
    // The edges that every system holds.
    for (secs, want) in [
        (253_402_300_799u64, "9999-12-31T23:59:59.000Z"),
        (253_402_300_800, "10000-01-01T00:00:00.000Z"),
        (12_622_780_800_000, "401970-01-01T00:00:00.000Z"),
    ] {
        assert_eq!(timestamp(UNIX_EPOCH.checked_add(Duration::from_secs(secs)).unwrap()), want, "{secs}");
    }
}

#[test]
fn a_line_has_time_level_thread_target_and_message() {
    let line = format_line("2026-10-31T10:00:00.000Z", Level::Warn, "main", "diacraft_engine::guard", "recovered\n");
    assert_eq!(line, "2026-10-31T10:00:00.000Z WARN  [main] diacraft_engine::guard: recovered\n");
}

/// A record may repeat what a client sent, and the log file is what a bug report attaches and a person reads in a terminal:
/// the CLI's logger writes a record as one line with its control characters escaped, and so does this one.
#[test]
fn a_message_with_control_characters_is_one_escaped_line() {
    let line = format_line(
        "2026-10-31T10:00:00.000Z",
        Level::Error,
        "main",
        "x",
        "bad\u{1b}[2J\n2026-10-31T10:00:01.000Z INFO  [main] x: fake\u{202e}\u{2028}end\n",
    );
    assert_eq!(line.matches('\n').count(), 1, "one line: {line:?}");
    assert!(line.ends_with('\n') && !line[..line.len() - 1].chars().any(|c| c.is_control() || c == '\u{202e}' || c == '\u{2028}'), "{line:?}");
    assert!(line.contains("bad\\u{1b}[2J\\n2026-10-31T10:00:01.000Z INFO  [main] x: fake\\u{202e}\\u{2028}end"), "{line:?}");
    // A trailing newline is still the end of the record, not part of it.
    assert!(format_line("t", Level::Info, "m", "x", "plain\n").ends_with("x: plain\n"));
}

#[test]
fn a_very_long_message_is_cut_and_the_file_gets_one_line_for_a_hostile_record() {
    let line = format_line("t", Level::Info, "m", "x", &"y".repeat(100_000));
    assert!(line.len() < 2_200 && line.ends_with("…\n"), "{} bytes", line.len());
    let dir = TempDir::new();
    let logger = AppLogger::new(Filter::parse("info"), false);
    let path = logger.attach_dir(dir.path()).unwrap();
    logger.log(&log::Record::builder().args(format_args!("first\nSECOND LINE\u{1b}[31m")).level(Level::Warn).target("x").build());
    logger.log(&log::Record::builder().args(format_args!("third")).level(Level::Warn).target("x").build());
    let text = read(&path);
    assert_eq!(text.lines().count(), 2, "a record is one line: {text:?}");
    assert!(text.contains("first\\nSECOND LINE\\u{1b}[31m") && !text.contains('\u{1b}'), "{text:?}");
}

#[test]
fn each_start_keeps_the_two_runs_before_it() {
    let dir = TempDir::new();
    for run in ["first", "second", "third", "fourth"] {
        let (path, mut file) = rotate(dir.path()).unwrap();
        assert_eq!(path, dir.path().join("diacraft.log"));
        std::io::Write::write_all(&mut file, run.as_bytes()).unwrap();
    }
    assert_eq!(read(&dir.path().join("diacraft.log")), "fourth");
    assert_eq!(read(&dir.path().join("diacraft.1.log")), "third");
    assert_eq!(read(&dir.path().join("diacraft.2.log")), "second");
    assert!(!dir.path().join("diacraft.3.log").exists(), "only two older runs are kept");
    let files = std::fs::read_dir(dir.path()).unwrap().count();
    assert_eq!(files, 3, "nothing else is left in the folder");
}

#[test]
fn a_missing_folder_is_made() {
    let dir = TempDir::new();
    let nested = dir.path().join("a").join("b").join("logs");
    let (path, _file) = rotate(&nested).unwrap();
    assert!(path.exists());
}

#[test]
fn the_log_file_stops_at_its_limit_and_says_so() {
    let dir = TempDir::new();
    let (path, file) = rotate(dir.path()).unwrap();
    let mut sink = Sink::new(1_000);
    sink.attach(file);
    for n in 0..200 {
        sink.write(&format!("line number {n:04}\n"));
    }
    let size = std::fs::metadata(&path).unwrap().len();
    let text = read(&path);
    assert!(size <= 1_000 + 200, "the file grew to {size} bytes past a 1,000-byte limit");
    assert!(text.contains("line number 0000") && text.contains("log file reached 1000 bytes"), "{text}");
    assert!(!text.contains("line number 0199"), "lines after the limit were written");
    // Later records do not change the file.
    sink.write("one more\n");
    assert_eq!(std::fs::metadata(&path).unwrap().len(), size);
}

#[test]
fn records_before_the_file_exists_are_kept_in_order_up_to_a_bound() {
    let dir = TempDir::new();
    let mut sink = Sink::new(MAX_FILE_BYTES);
    for n in 0..MAX_PENDING + 25 {
        sink.write(&format!("early {n}\n"));
    }
    let (path, file) = rotate(dir.path()).unwrap();
    sink.attach(file);
    sink.write("after\n");
    let text = read(&path);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.first(), Some(&"early 0"));
    assert_eq!(lines.get(MAX_PENDING - 1), Some(&format!("early {}", MAX_PENDING - 1).as_str()));
    assert!(text.contains("25 earlier log lines were dropped"), "{}", &text[text.len().saturating_sub(300)..]);
    assert_eq!(lines.last(), Some(&"after"));
}

#[test]
fn a_run_with_no_file_keeps_nothing() {
    let mut sink = Sink::new(MAX_FILE_BYTES);
    sink.write("early\n");
    sink.no_file();
    let dir = TempDir::new();
    let (path, file) = rotate(dir.path()).unwrap();
    sink.attach(file);
    // `attach` after `no_file` starts a file again, and the early line is not in it.
    sink.write("late\n");
    assert_eq!(read(&path), "late\n");
}

#[test]
fn the_logger_writes_what_its_filter_lets_through() {
    let dir = TempDir::new();
    let logger = AppLogger::new(Filter::parse("warn,diacraft*=info"), false);
    let path = logger.attach_dir(dir.path()).unwrap();
    assert_eq!(path, dir.path().join("diacraft.log"));
    let record = |level, target: &str, text: &str| {
        logger.log(&log::Record::builder().args(format_args!("{text}")).level(level).target(target).build());
    };
    record(Level::Info, "diacraft_engine", "engine said hello");
    record(Level::Debug, "diacraft_engine", "too chatty");
    record(Level::Info, "wgpu_core", "somebody else's chatter");
    record(Level::Warn, "wgpu_core", "somebody else's warning");
    let text = read(&path);
    assert!(text.contains("INFO ") && text.contains("diacraft_engine: engine said hello"), "{text}");
    assert!(text.contains("wgpu_core: somebody else's warning"), "{text}");
    assert!(!text.contains("too chatty") && !text.contains("chatter"), "{text}");
    assert_eq!(text.lines().count(), 2);
}

#[test]
fn a_folder_that_cannot_be_made_leaves_standard_error_the_only_sink() {
    let dir = TempDir::new();
    // A file where the folder should be.
    let blocker = dir.path().join("logs");
    std::fs::write(&blocker, "not a folder").unwrap();
    let logger = AppLogger::new(Filter::parse("info"), false);
    let error = logger.attach_dir(&blocker.join("deeper")).unwrap_err();
    assert!(error.contains("logs"), "{error}");
    // Logging goes on without a file, and does not panic.
    logger.log(&log::Record::builder().args(format_args!("still alive")).level(Level::Error).target("x").build());
}

#[test]
fn the_logger_knows_its_file_for_the_message_of_a_start_that_failed() {
    // `failure` tells the person where the log is: the logger says which file it made.
    let dir = TempDir::new();
    let logger = AppLogger::new(Filter::parse("info"), false);
    assert_eq!(logger.file(), None, "no file yet");
    let path = logger.attach_dir(dir.path()).unwrap();
    assert_eq!(logger.file(), Some(path));
    let blocker = dir.path().join("a-file");
    std::fs::write(&blocker, "not a folder").unwrap();
    let other = AppLogger::new(Filter::parse("info"), false);
    other.attach_dir(&blocker.join("deeper")).unwrap_err();
    assert_eq!(other.file(), None, "a folder that could not be made leaves no file to point to");
    other.no_file();
    assert_eq!(other.file(), None);
}

#[test]
fn a_flush_leaves_the_lines_in_the_file_and_never_fails() {
    // The start flushes the log before it tests a graphics adapter, so that the line that says which one is on the disk if the driver takes the machine down.
    let dir = TempDir::new();
    let logger = AppLogger::new(Filter::parse("info"), false);
    log::Log::flush(&logger);
    let path = logger.attach_dir(dir.path()).unwrap();
    logger.log(
        &log::Record::builder()
            .args(format_args!("testing Intel(R) UHD Graphics (Dx12, IntegratedGpu) (try usual, test 1)"))
            .level(Level::Info)
            .target("x")
            .build(),
    );
    log::Log::flush(&logger);
    assert!(read(&path).contains("testing Intel(R) UHD Graphics (Dx12, IntegratedGpu) (try usual, test 1)"));
    // With no file there is nothing to flush.
    let none = AppLogger::new(Filter::parse("info"), false);
    none.no_file();
    log::Log::flush(&none);
}
