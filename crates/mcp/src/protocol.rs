//! JSON-RPC 2.0 over lines, and the parts of the Model Context Protocol this server speaks: `initialize`, `ping`,
//! `tools/list` and `tools/call`.
//!
//! A message is one line of JSON on the input, and a reply is one line on the output, in the order the messages
//! came. Everything on the input is hostile: a line is read at most [`MAX_LINE_BYTES`] long, JSON nests at most
//! as deep as `serde_json` allows (128 levels), and a batch holds at most [`MAX_BATCH_MESSAGES`] messages. A line
//! that is wrong gets one error reply and the server goes on with the next line. Text the client sent is
//! escaped before an error repeats it ([`diacraft_engine::shown`]).
//!
//! The server is one thread: a message is handled, and its reply written, before the next line is read.

use std::io::{self, BufRead, Write};

use diacraft_engine::{guard, shown};
use serde_json::{Map, Value, json};

use crate::backend::Backend;
use crate::tools;

/// The protocol revision this server speaks, and answers a client that asks for any other.
pub const PROTOCOL_VERSION: &str = "2025-06-18";

/// The revisions a client may ask for. The messages this server handles are the same in all of them.
const SUPPORTED_VERSIONS: [&str; 3] = [PROTOCOL_VERSION, "2025-03-26", "2024-11-05"];

/// Longest accepted line, in bytes without the line ending: 16 MiB. A longer one is answered with an error and skipped.
pub const MAX_LINE_BYTES: usize = 16 * 1024 * 1024;

/// Most messages in one batch. Batches were taken out of the protocol in 2025-06-18; older clients may still send them.
pub const MAX_BATCH_MESSAGES: usize = 64;

/// The keys a JSON-RPC message has. Another is refused, not ignored: a client that misspells `params` hears of it.
const ENVELOPE_KEYS: [&str; 4] = ["jsonrpc", "id", "method", "params"];

/// The keys of the `params` of `tools/call`. `_meta` is the protocol's own.
const CALL_KEYS: [&str; 3] = ["name", "arguments", "_meta"];

/// The most characters of a client's text an error repeats.
const MOST_SHOWN: usize = 64;

/// The most characters of a caught panic's message a reply repeats.
const MOST_PANIC: usize = 400;

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;
const INTERNAL_ERROR: i64 = -32603;

/// Serves the client at the other end of `input` and `output` until `input` ends, on `backend`. Logs go to the
/// `log` crate, never to `output`: that carries the protocol and nothing else.
///
/// Returns `Ok` when `input` ends, and `Err` when reading it or writing `output` fails.
pub fn serve(input: impl BufRead, output: impl Write, mut backend: Backend) -> io::Result<()> {
    serve_with(input, output, &mut backend)
}

/// As [`serve`], on a backend the caller keeps, to look at afterwards.
pub fn serve_with(mut input: impl BufRead, mut output: impl Write, backend: &mut Backend) -> io::Result<()> {
    let mut line = Vec::new();
    loop {
        let reply = match read_line(&mut input, &mut line, MAX_LINE_BYTES)? {
            Next::End => return Ok(()),
            Next::Line => handle_line(backend, &line),
            Next::TooLong { rest_unread } => {
                // Answered first, so that a client sending on and on is told before the end of the line, which may
                // never come, and then the rest of it is read and thrown away without holding it.
                write_reply(
                    &mut output,
                    &error(Value::Null, INVALID_REQUEST, format!("request line longer than {MAX_LINE_BYTES} bytes; it was skipped")),
                )?;
                if rest_unread {
                    skip_line(&mut input)?;
                }
                continue;
            }
        };
        if let Some(reply) = reply {
            write_reply(&mut output, &reply)?;
        }
    }
}

/// What one read of the input came to.
enum Next {
    /// The input has ended.
    End,
    /// A line is in the buffer, without its line ending.
    Line,
    /// The line is longer than the limit. `rest_unread`: its end has not been read yet (see [`skip_line`]).
    TooLong { rest_unread: bool },
}

/// Reads one line into `buf` (the `\n`, and a `\r` before it, taken off): at most `max` bytes of it, and no more
/// than `max` plus its line ending is ever buffered however long the line is.
fn read_line(input: &mut impl BufRead, buf: &mut Vec<u8>, max: usize) -> io::Result<Next> {
    buf.clear();
    // A line within the limit is at most `max` bytes and `\r\n`; one byte more shows a line over it.
    let limit = u64::try_from(max).unwrap_or(u64::MAX).saturating_add(3);
    let n = io::Read::take(&mut *input, limit).read_until(b'\n', buf)?;
    if n == 0 {
        return Ok(Next::End);
    }
    let ended = buf.last() == Some(&b'\n');
    if ended {
        buf.pop();
        if buf.last() == Some(&b'\r') {
            buf.pop();
        }
    }
    if buf.len() > max {
        // Unread: no line ending yet, and the input did not end (the limit, not the input, stopped the read).
        return Ok(Next::TooLong { rest_unread: !ended && u64::try_from(n).is_ok_and(|n| n == limit) });
    }
    Ok(Next::Line)
}

/// Reads and drops the rest of a line, a buffer at a time.
fn skip_line(input: &mut impl BufRead) -> io::Result<()> {
    loop {
        let (used, done) = match input.fill_buf() {
            Ok([]) => return Ok(()),
            Ok(chunk) => match chunk.iter().position(|&b| b == b'\n') {
                Some(end) => (end.saturating_add(1), true),
                None => (chunk.len(), false),
            },
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        input.consume(used);
        if done {
            return Ok(());
        }
    }
}

/// `value` as one line of JSON, without the line ending. U+2028 and U+2029 are written as escapes: they are legal
/// in JSON text, but some line readers split a line at them, so a value holding one (a page's name may) would
/// arrive as two broken lines. Both spellings mean the same text.
pub fn json_line(value: &Value) -> Result<String, serde_json::Error> {
    // JSON text holds these two characters only inside a string, where the escape is the same character.
    Ok(serde_json::to_string(value)?.replace('\u{2028}', "\\u2028").replace('\u{2029}', "\\u2029"))
}

/// Writes a reply as one line and flushes it.
fn write_reply(output: &mut impl Write, reply: &Value) -> io::Result<()> {
    let mut line = json_line(reply).map_err(io::Error::from)?;
    line.push('\n');
    output.write_all(line.as_bytes())?;
    output.flush()
}

fn response(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn error(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message.into()}})
}

/// The reply to one line, if it has one.
fn handle_line(backend: &mut Backend, line: &[u8]) -> Option<Value> {
    let Ok(text) = std::str::from_utf8(line) else {
        return Some(error(Value::Null, PARSE_ERROR, "parse error: the line is not valid UTF-8"));
    };
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    // serde_json stops at 128 levels of nesting ("recursion limit exceeded"): a message cannot make the server recurse
    // deeper. The limit is kept on purpose: nothing here calls `disable_recursion_limit`, and the feature that provides
    // it is on nowhere in the workspace (an xtask test fails if it is).
    let value = match serde_json::from_str::<Value>(text) {
        Ok(value) => value,
        Err(e) => return Some(error(Value::Null, PARSE_ERROR, format!("parse error: {}", shown(&e.to_string(), 200)))),
    };
    let id = reply_id(&value);
    guarded(id, || handle_value(backend, value))
}

/// The id a reply to `message` carries if the server fails on it: `None` for a notification, which has no reply.
fn reply_id(message: &Value) -> Option<Value> {
    match message {
        Value::Object(o) => o.get("id").filter(|id| id.is_string() || id.is_number()).cloned(),
        _ => Some(Value::Null),
    }
}

/// The reply from `handle`; a panic in it (a bug, caught) is an internal error for the message's `id`, and the server
/// goes on. The tools guard themselves and report in a tool result; this is the net under the protocol code.
pub(crate) fn guarded(id: Option<Value>, handle: impl FnOnce() -> Option<Value>) -> Option<Value> {
    match guard::catch_panic(handle) {
        Ok(reply) => reply,
        Err(panic) => id.map(|id| error(id, INTERNAL_ERROR, shown(&format!("internal error: {panic} (please report this bug)"), MOST_PANIC))),
    }
}

fn handle_value(backend: &mut Backend, value: Value) -> Option<Value> {
    match value {
        Value::Array(messages) => handle_batch(backend, messages),
        other => handle_message(backend, other),
    }
}

/// A batch: refused whole if empty or too big, else each message answered, and the answers that there are returned.
fn handle_batch(backend: &mut Backend, messages: Vec<Value>) -> Option<Value> {
    if messages.is_empty() {
        return Some(error(Value::Null, INVALID_REQUEST, "an empty batch"));
    }
    if messages.len() > MAX_BATCH_MESSAGES {
        return Some(error(
            Value::Null,
            INVALID_REQUEST,
            format!("a batch of {} messages is over the limit of {MAX_BATCH_MESSAGES}; none of it was run", messages.len()),
        ));
    }
    let replies: Vec<Value> = messages.into_iter().filter_map(|message| handle_message(backend, message)).collect();
    (!replies.is_empty()).then_some(Value::Array(replies))
}

/// One message: a request is answered, a notification is not.
fn handle_message(backend: &mut Backend, message: Value) -> Option<Value> {
    let Value::Object(mut o) = message else {
        return Some(error(Value::Null, INVALID_REQUEST, "a message must be a JSON object"));
    };
    let id = match o.get("id") {
        None => None,
        Some(id @ (Value::String(_) | Value::Number(_))) => Some(id.clone()),
        Some(_) => return Some(error(Value::Null, INVALID_REQUEST, "`id` must be a string or a number")),
    };
    // The one envelope there is. A notification in another has no id to answer, so it is dropped, not run.
    if o.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return id.map(|id| error(id, INVALID_REQUEST, "expected \"jsonrpc\": \"2.0\""));
    }
    let Some(Value::String(method)) = o.remove("method") else {
        // A reply to something the server asked the client: it asks nothing, so there is nothing to do with one.
        if o.contains_key("result") || o.contains_key("error") {
            return None;
        }
        return Some(error(id.unwrap_or(Value::Null), INVALID_REQUEST, "a message needs a text `method`"));
    };
    if let Some(key) = o.keys().find(|k| !ENVELOPE_KEYS.contains(&k.as_str())) {
        let key = shown(key, MOST_SHOWN);
        return match id {
            Some(id) => Some(error(id, INVALID_PARAMS, format!("unknown key `{key}` in the message; a message has jsonrpc, id, method and params"))),
            None => {
                log::warn!("ignoring a notification with an unknown key `{key}`");
                None
            }
        };
    }
    let params = match o.remove("params") {
        None | Some(Value::Null) => Map::new(),
        Some(Value::Object(params)) => params,
        Some(_) => return id.map(|id| error(id, INVALID_PARAMS, "`params` must be an object")),
    };
    // A message without an id asks for no answer, so it is never run, whatever method it names: a command whose
    // outcome nobody can be told would be an edit from nowhere.
    let Some(id) = id else {
        notification(&method);
        return None;
    };
    Some(match request(backend, &method, params) {
        Ok(result) => response(id, result),
        Err((code, message)) => error(id, code, message),
    })
}

/// A notification: there is nothing to do for any of them. The client says it has initialized, or cancels a request
/// (there is never one waiting: requests are answered before the next line is read).
fn notification(method: &str) {
    log::debug!("ignoring notification `{}`", shown(method, MOST_SHOWN));
}

/// What a request's method answers: a result, or an error code and message.
type Outcome = Result<Value, (i64, String)>;

fn request(backend: &mut Backend, method: &str, params: Map<String, Value>) -> Outcome {
    match method {
        "initialize" => initialize(backend, &params),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({"tools": tools::tool_definitions()})),
        "tools/call" => tools_call(backend, params),
        other => Err((METHOD_NOT_FOUND, format!("method not found: `{}`", shown(other, MOST_SHOWN)))),
    }
}

fn initialize(backend: &Backend, params: &Map<String, Value>) -> Outcome {
    let asked = match params.get("protocolVersion") {
        None => PROTOCOL_VERSION,
        Some(Value::String(version)) => version.as_str(),
        Some(_) => return Err((INVALID_PARAMS, "`protocolVersion` must be text".to_owned())),
    };
    // The revision the client asks for, if this server speaks it, else the newest it does.
    let version = SUPPORTED_VERSIONS.iter().find(|v| **v == asked).copied().unwrap_or(PROTOCOL_VERSION);
    Ok(json!({
        "protocolVersion": version,
        "capabilities": {"tools": {"listChanged": false}},
        "serverInfo": {"name": "diacraft", "title": "DiaCraft", "version": env!("CARGO_PKG_VERSION")},
        "instructions": format!(
            "DiaCraft is a diagramming app. Every action is a command: list them with command_list, run one with command_run, \
             several as one undo step with command_batch, and look at the document with doc_inspect. \
             ui_inspect and ui_screenshot work only with the app attached. Backend: {}.",
            backend.describe()
        ),
    }))
}

fn tools_call(backend: &mut Backend, mut params: Map<String, Value>) -> Outcome {
    if let Some(key) = params.keys().find(|k| !CALL_KEYS.contains(&k.as_str())) {
        return Err((INVALID_PARAMS, format!("unknown key `{}` in the params of tools/call; it takes name and arguments", shown(key, MOST_SHOWN))));
    }
    let Some(Value::String(name)) = params.remove("name") else {
        return Err((INVALID_PARAMS, "tools/call needs a text `name`: the tool to call".to_owned()));
    };
    let Some(tool) = tools::find(&name) else {
        return Err((INVALID_PARAMS, format!("unknown tool `{}`; the tools are: {}", shown(&name, MOST_SHOWN), tools::names().join(", "))));
    };
    let arguments = match params.remove("arguments") {
        None | Some(Value::Null) => Map::new(),
        Some(Value::Object(arguments)) => arguments,
        Some(_) => return Err((INVALID_PARAMS, "`arguments` must be an object".to_owned())),
    };
    let takes = tool.arguments();
    if let Some(key) = arguments.keys().find(|k| !takes.contains(k)) {
        let key = shown(key, MOST_SHOWN);
        return Err((
            INVALID_PARAMS,
            if takes.is_empty() {
                format!("unknown argument `{key}` for `{}`; it takes no arguments", tool.name)
            } else {
                format!("unknown argument `{key}` for `{}`; it takes: {}", tool.name, takes.join(", "))
            },
        ));
    }
    Ok(tools::call(backend, tool, arguments).to_value())
}
