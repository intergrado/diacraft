//! The logger: records go to standard error. Standard output carries results and the MCP protocol, and nothing else.

use std::io::Write;
use std::str::FromStr;
use std::sync::OnceLock;

use diacraft_engine::shown;
use log::{LevelFilter, Log, Metadata, Record};

/// The most characters of a record's message one line holds.
const MOST_CHARS: usize = 2000;

/// Writes each record that is at or above its level as one line on standard error.
struct StderrLogger {
    level: LevelFilter,
}

impl Log for StderrLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= self.level
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        // A record may repeat text a client sent: control characters in it do not reach the terminal as they are,
        // and a record is one line.
        let message = shown(&record.args().to_string(), MOST_CHARS);
        // Nowhere to report a failure to write to the stream that reports failures.
        let _ = writeln!(std::io::stderr(), "diacraft-cli: [{} {}] {message}", record.level(), record.target());
    }

    fn flush(&self) {}
}

/// Starts logging at the level `RUST_LOG` names (`off`, `error`, `warn`, `info`, `debug` or `trace`), `warn` if it is not
/// set. Returns a note for the person when it names something else.
pub fn init(rust_log: Option<&str>) -> Option<String> {
    let (level, note) = match rust_log.map(str::trim).filter(|v| !v.is_empty()) {
        None => (LevelFilter::Warn, None),
        Some(text) => match LevelFilter::from_str(text) {
            Ok(level) => (level, None),
            Err(_) => {
                (LevelFilter::Warn, Some(format!("RUST_LOG={} is not one of off, error, warn, info, debug, trace; logging at warn", shown(text, 64))))
            }
        },
    };
    static LOGGER: OnceLock<StderrLogger> = OnceLock::new();
    let logger = LOGGER.get_or_init(|| StderrLogger { level });
    // Fails only if a logger is already installed, which leaves that one in charge.
    if log::set_logger(logger).is_ok() {
        log::set_max_level(level);
    }
    note
}
