//! Integration tests for the desktop app's parts: the command line, the control server's transport against
//! a fake inbox, the logger, which GPU draws the window (`graphics`), what a start remembers about graphics tests that never returned (`marker`), the tries a start goes through when the usual graphics fail (`fallback`), what a start that failed tells the person (`failure`), and the macOS bundle's program name (`bundle`). None of them opens a window but
//! `window`'s, which is ignored unless asked for: it starts the app on this machine's screen.
//!
//! Tests may unwrap and panic: a failing assertion is the point of a test.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![forbid(unsafe_code)]

mod allocator;
mod app_id;
mod args;
mod build_info;
mod bundle;
mod control_props;
mod control_transport;
mod failure;
mod fallback;
mod graphics;
mod logging;
mod marker;
mod window;

use std::sync::{Mutex, OnceLock};

/// What the `log` crate was told since [`capture_logs`], one `LEVEL message` string each. The tests of one binary share it
/// (under `cargo test`, one process), so a test looks only for the records that name something of its own.
static RECORDS: Mutex<Vec<String>> = Mutex::new(Vec::new());

struct Capture;

impl log::Log for Capture {
    fn enabled(&self, _: &log::Metadata<'_>) -> bool {
        true
    }

    fn log(&self, record: &log::Record<'_>) {
        RECORDS.lock().unwrap().push(format!("{} {}", record.level(), record.args()));
    }

    fn flush(&self) {}
}

/// Makes the `log` records of this process readable through [`logged`]. Safe to call from every test that wants it.
pub fn capture_logs() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        // Another logger installed first would leave a test with nothing to read, and the test would say so.
        let _ = log::set_logger(&Capture);
        log::set_max_level(log::LevelFilter::Info);
    });
}

/// The records captured so far.
pub fn logged() -> Vec<String> {
    RECORDS.lock().unwrap().clone()
}
