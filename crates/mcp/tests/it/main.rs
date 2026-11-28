//! The MCP server's integration tests: one binary, each file a module (AGENTS.md: one test binary per crate).
//!
//! Tests may unwrap and panic: a failing assertion is the point of a test.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![forbid(unsafe_code)]

mod automation_roots;
mod backend;
mod check_before;
mod protocol;
mod protocol_props;
mod remote_props;

use std::sync::{Mutex, Once};

use diacraft_engine::Session;
use diacraft_mcp::{Backend, serve_with};
use serde_json::{Value, json};

/// What the `log` crate was told since `capture_logs`, one `LEVEL message` string each. The tests of one binary share it
/// (under `cargo test`, one process), so a test looks only for the records that name something of its own.
static LOGGED: Mutex<Vec<String>> = Mutex::new(Vec::new());

struct Capture;

impl log::Log for Capture {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        LOGGED.lock().unwrap().push(format!("{} {}", record.level(), record.args()));
    }

    fn flush(&self) {}
}

/// Makes the `log` records of this process readable through [`logged`]. Safe to call from every test that wants it.
pub fn capture_logs() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        log::set_logger(&Capture).expect("no other logger is installed in this test binary");
        log::set_max_level(log::LevelFilter::Debug);
    });
}

/// The records captured so far.
pub fn logged() -> Vec<String> {
    LOGGED.lock().unwrap().clone()
}

/// A request line as a client sends it.
pub fn request(id: u64, method: &str, params: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
}

/// A notification: a message with no id, which gets no reply.
pub fn notification(method: &str) -> Value {
    json!({"jsonrpc": "2.0", "method": method})
}

/// A `tools/call` request.
pub fn call(id: u64, tool: &str, arguments: Value) -> Value {
    request(id, "tools/call", json!({"name": tool, "arguments": arguments}))
}

/// What `input` makes the server write, as it is. The server is run on `backend` to the end of `input`, and must
/// come back `Ok`.
pub fn raw_on(backend: &mut Backend, input: &[u8]) -> String {
    let mut out = Vec::new();
    serve_with(input, &mut out, backend).expect("the server ends cleanly at the end of its input");
    String::from_utf8(out).expect("the server writes UTF-8")
}

/// The lines `input` makes the server write, parsed: one entry per line of output.
pub fn talk_on(backend: &mut Backend, input: &[u8]) -> Vec<Value> {
    let text = raw_on(backend, input);
    text.lines().map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("output line is not JSON ({e}): {l}"))).collect()
}

/// `messages`, each sent as one line, to a server with a headless session; the replies in order.
pub fn talk(messages: &[Value]) -> Vec<Value> {
    talk_on(&mut Backend::Headless(Box::new(Session::new())), &lines(messages))
}

/// `messages` as lines of input.
pub fn lines(messages: &[Value]) -> Vec<u8> {
    let mut input = Vec::new();
    for m in messages {
        input.extend_from_slice(m.to_string().as_bytes());
        input.push(b'\n');
    }
    input
}

/// The text of a tool result's first content block.
pub fn tool_text(reply: &Value) -> String {
    reply["result"]["content"][0]["text"].as_str().unwrap_or_else(|| panic!("no tool text in {reply}")).to_owned()
}

/// A tool's text, parsed as the JSON it holds.
pub fn tool_json(reply: &Value) -> Value {
    serde_json::from_str(&tool_text(reply)).unwrap_or_else(|e| panic!("tool text is not JSON ({e}): {reply}"))
}
