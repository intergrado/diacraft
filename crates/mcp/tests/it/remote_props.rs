//! `Backend::Remote` fuzzed against a fake app on a real socket: the app answers each request from a random script (a good
//! reply, an error, a refusal with no id, a reply to another request, JSON of any shape, bytes of any kind, a line too long,
//! two lines at once, a close, silence), and the client makes random calls.
//!
//! AGENTS.md: protocol methods are fuzzed, and what comes back from the app is as hostile as what goes to it. Whatever the
//! app does, the client never panics, comes back within its limits, says why in text that is safe to print, and never takes
//! an answer meant for another request as the answer to this one: the replies a good app gives carry the call's own marker,
//! and an `Ok` that does not hold it is a stale or foreign reply taken for a fresh one.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use diacraft_mcp::{Backend, ClientConfig, ControlClient};
use proptest::prelude::*;
use serde_json::{Value, json};

/// The longest reply the client reads in these tests: a line of `HUGE` bytes is over it.
const MAX_REPLY: usize = 4096;
const HUGE: usize = 6000;

/// The longest error text a case may see: the app's own error is cut at 2,000 characters, and a message around it is short.
const MOST_ERROR_CHARS: usize = 5000;

/// What the fake app does with one request.
#[derive(Clone, Debug)]
enum Act {
    /// A good reply: `{"id", "ok": true, "result": {"for": id, "marker": …}}` (`{"app": "diacraft"}` to the question who it is).
    Valid,
    /// `{"id", "ok": false, "error": text}`.
    Failure(String),
    /// `{"ok": false, "error": text}` and a close: what the app says to a line it will not read.
    Refusal(String),
    /// A good reply with an id that is not the request's.
    WrongId,
    /// `{"ok": true, "result": 1}` with no id.
    NoId,
    /// `{"id", "ok": "true"}`: an `ok` that is not a boolean.
    OkNotBool,
    /// `{"id"}`: neither `ok` nor an error.
    NeitherOk,
    /// Any JSON, as the whole line.
    Odd(Value),
    /// These bytes and a newline.
    Raw(Vec<u8>),
    /// These bytes, no newline, and a close.
    RawThenClose(Vec<u8>),
    /// A line longer than the client reads.
    Huge,
    /// The good reply, twice.
    Two,
    /// A close with no reply.
    Close,
    /// No reply, and the connection kept.
    Silent,
}

fn arb_json() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::from),
        any::<i64>().prop_map(Value::from),
        any::<f64>().prop_map(Value::from),
        any::<String>().prop_map(|s| Value::from(s.chars().take(24).collect::<String>())),
    ];
    leaf.prop_recursive(3, 24, 4, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..4).prop_map(Value::Array),
            prop::collection::vec(("[a-z_]{0,6}", inner), 0..4).prop_map(|pairs| Value::Object(pairs.into_iter().collect())),
        ]
    })
}

/// Text that may hold any character: letters, now and then any character at all, and the ones that must not reach a terminal
/// or a log as they are (an escape, a newline, NUL, DEL, the line separator, a right-to-left override).
fn arb_text() -> impl Strategy<Value = String> {
    let hostile = prop_oneof![
        4 => proptest::char::range('a', 'z'),
        1 => any::<char>(),
        1 => proptest::sample::select(vec!['\u{1b}', '\n', '\r', '\t', '\0', '\u{7f}', '\u{2028}', '\u{2029}', '\u{202e}', '\u{2067}']),
    ];
    prop::collection::vec(hostile, 0..40).prop_map(|chars| chars.into_iter().collect())
}

fn arb_act() -> impl Strategy<Value = Act> {
    prop_oneof![
        8 => Just(Act::Valid),
        2 => arb_text().prop_map(Act::Failure),
        1 => arb_text().prop_map(Act::Refusal),
        1 => Just(Act::WrongId),
        1 => Just(Act::NoId),
        1 => Just(Act::OkNotBool),
        1 => Just(Act::NeitherOk),
        2 => arb_json().prop_map(Act::Odd),
        1 => prop::collection::vec(any::<u8>(), 0..80).prop_map(Act::Raw),
        1 => prop::collection::vec(any::<u8>(), 0..80).prop_map(Act::RawThenClose),
        1 => Just(Act::Huge),
        1 => Just(Act::Two),
        1 => Just(Act::Close),
        1 => Just(Act::Silent),
    ]
}

/// A method name: one of the protocol's, or anything.
fn arb_method() -> impl Strategy<Value = String> {
    prop_oneof![
        6 => proptest::sample::select(vec!["engine.execute", "engine.commands", "document.inspect", "ui.inspect", "ui.screenshot", "app.quit"]).prop_map(str::to_owned),
        1 => arb_text(),
    ]
}

/// What the fake app's thread is told: the script (one act for each request it reads, on any connection, in order; a good
/// reply once it runs out), and when to stop.
struct Peer {
    addr: String,
    stop: Arc<AtomicBool>,
}

impl Drop for Peer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

/// The fake app on a free port, answering from `script`.
fn peer(script: Vec<Act>) -> Peer {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let stop = Arc::new(AtomicBool::new(false));
    let acts: Arc<Mutex<VecDeque<Act>>> = Arc::new(Mutex::new(script.into()));
    let flag = Arc::clone(&stop);
    std::thread::spawn(move || {
        while !flag.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((stream, _)) => {
                    // An accepted socket keeps the listener's non-blocking mode on macOS (not on Linux): `serve` reads blocking.
                    stream.set_nonblocking(false).unwrap();
                    let acts = Arc::clone(&acts);
                    std::thread::spawn(move || serve(stream, &acts));
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(2)),
                Err(_) => return,
            }
        }
    });
    Peer { addr, stop }
}

fn serve(stream: TcpStream, acts: &Mutex<VecDeque<Act>>) {
    let Ok(mut writer) = stream.try_clone() else { return };
    let mut reader = BufReader::new(stream);
    // The first request on a connection is the question who the app is.
    let mut probe = true;
    loop {
        let mut line = Vec::new();
        if !matches!(reader.read_until(b'\n', &mut line), Ok(n) if n > 0) {
            return;
        }
        let Ok(request) = serde_json::from_slice::<Value>(&line) else { return };
        let (id, marker) = (request["id"].clone(), request["params"]["marker"].clone());
        let asking_who = std::mem::take(&mut probe);
        let act = acts.lock().unwrap().pop_front().unwrap_or(Act::Valid);
        let result = if asking_who { json!({"app": "diacraft", "version": "0"}) } else { json!({"for": id, "marker": marker}) };
        let line = |value: Value| format!("{value}\n").into_bytes();
        let (bytes, close) = match act {
            Act::Valid => (line(json!({"id": id, "ok": true, "result": result})), false),
            Act::Failure(text) => (line(json!({"id": id, "ok": false, "error": text})), false),
            Act::Refusal(text) => (line(json!({"ok": false, "error": text})), true),
            Act::WrongId => (line(json!({"id": id.as_u64().unwrap_or(0) + 1000, "ok": true, "result": result})), false),
            Act::NoId => (line(json!({"ok": true, "result": 1})), false),
            Act::OkNotBool => (line(json!({"id": id, "ok": "true", "result": result})), false),
            Act::NeitherOk => (line(json!({"id": id})), false),
            Act::Odd(value) => (line(value), false),
            Act::Raw(mut bytes) => {
                bytes.retain(|b| *b != b'\n');
                bytes.push(b'\n');
                (bytes, false)
            }
            Act::RawThenClose(mut bytes) => {
                bytes.retain(|b| *b != b'\n');
                (bytes, true)
            }
            Act::Huge => ([vec![b'x'; HUGE], b"\n".to_vec()].concat(), false),
            Act::Two => {
                ([line(json!({"id": id, "ok": true, "result": result})), line(json!({"id": id, "ok": true, "result": result}))].concat(), false)
            }
            Act::Close => return,
            Act::Silent => continue,
        };
        if writer.write_all(&bytes).and_then(|()| writer.flush()).is_err() || close {
            return;
        }
    }
}

/// Short limits, so a case with a silent app takes a fraction of a second and not the default minute and a half.
fn quick() -> ClientConfig {
    ClientConfig { connect: Duration::from_millis(400), probe: Duration::from_millis(250), reply: Duration::from_millis(250), max_reply: MAX_REPLY }
}

/// What a message to a person must be, whatever the app said: text without a control character, line separator or direction
/// mark, and not huge.
fn check_error_text(text: &str) -> Result<(), TestCaseError> {
    let dangerous = |c: char| c.is_control() || matches!(c, '\u{2028}' | '\u{2029}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}');
    prop_assert!(!text.chars().any(dangerous), "an error with a character that is not safe to print: {:?}", text);
    prop_assert!(text.chars().count() <= MOST_ERROR_CHARS, "an error of {} characters", text.chars().count());
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, failure_persistence: None, ..ProptestConfig::default() })]

    /// Calls through `Backend::Remote` to an app that behaves as scripted: nothing panics; every error is text that is safe to
    /// print; and an `Ok` holds the marker of the call it answers, never one meant for another.
    #[test]
    fn remote_calls_to_a_scripted_app_never_panic_and_never_take_a_stale_answer(
        script in prop::collection::vec(arb_act(), 0..12),
        calls in prop::collection::vec((arb_method(), arb_json()), 1..6),
    ) {
        let app = peer(script);
        let client = match ControlClient::connect_with(&app.addr, quick()) {
            Ok(client) => client,
            Err(refused) => {
                // An app that does not answer who it is, or answers something else: refused, in text that is safe to print.
                check_error_text(&refused.to_string())?;
                return Ok(());
            }
        };
        let mut backend = Backend::Remote(client);
        for (i, (method, params)) in calls.into_iter().enumerate() {
            let params = match params {
                Value::Object(mut object) => {
                    object.insert("marker".to_owned(), json!(i));
                    Value::Object(object)
                }
                other => json!({"marker": i, "other": other}),
            };
            match backend.call(&method, &params) {
                Ok(result) => prop_assert_eq!(&result["marker"], &json!(i), "an answer meant for another request: {}", result),
                Err(error) => check_error_text(&error)?,
            }
        }
        prop_assert!(backend.describe().starts_with("the DiaCraft app at "));
    }
}
