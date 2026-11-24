//! Last line of defence against bugs: a frontend never crashes on a panic.
//!
//! Code must not panic (AGENTS.md, Never crash); fallible work returns `Result`. Should a bug panic
//! anyway, the entry points ([`crate::Session::run_spec`] and so `execute`, and the control channel,
//! the MCP server and the interface frame later) catch it with [`catch_panic`], put back what they
//! changed and report an error instead of taking the app, and the user's unsaved work, down.
//!
//! On wasm the panic strategy is `abort`: `catch_unwind` compiles there but catches nothing, so the
//! web build has no net and the rule against panicking code is its only protection.

use std::any::Any;
use std::panic::{AssertUnwindSafe, catch_unwind};

/// Runs `f`, turning a panic into `Err(message)`. The default panic hook still prints the message
/// and location to stderr. The caller must leave its state consistent after an `Err`: put back what
/// `f` was changing.
pub fn catch_panic<T>(f: impl FnOnce() -> T) -> Result<T, String> {
    catch_unwind(AssertUnwindSafe(f)).map_err(|payload| {
        let msg = panic_message(payload.as_ref());
        log::error!("recovered from a panic: {msg}");
        msg
    })
}

/// The message of a panic payload: `panic!("…")` carries a `&str` or a `String`.
pub fn panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_owned()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_owned()
    }
}
