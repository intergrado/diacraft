//! The control protocol fuzzed over real TCP: random lines (random bytes, JSON of any shape with the faults a client can make in
//! a request, nesting past serde_json's limit, huge strings, blank lines, CRLF) against a server whose window is a fake inbox.
//!
//! AGENTS.md: protocol methods are fuzzed. Whatever the lines, the server answers in protocol, passes on only well-formed
//! requests, closes after one refusal, goes on serving, and never has to fall back on its safety net: a panic in a thread of the
//! server is caught and logged (`guard::catch_panic`), so "the client got an answer" alone would hold for a server that panics
//! on every odd line. The last property looks for the log record of a caught panic.

use std::collections::HashSet;
use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use diacraft_desktop::control_server::{Limits, Server};
use proptest::prelude::*;
use serde_json::{Map, Value, json};

/// How long a read waits for the server. It only ends a case whose server never answers or closes.
const WAIT: Duration = Duration::from_secs(20);

/// What a log record says when a thread of the server caught a panic (`apps/diacraft/src/control_server.rs`, and the guard).
const CAUGHT: [&str; 3] = ["ended with a panic", "while accepting a connection", "recovered from a panic"];

/// The log records of a panic a thread of a server caught, but the one that `control_transport.rs` makes on purpose (its panic
/// text holds this marker): under `cargo test` the tests of this binary share one log.
fn caught_panics() -> Vec<String> {
    crate::logged().into_iter().filter(|r| CAUGHT.iter().any(|c| r.contains(c)) && !r.contains("marker-4c2")).collect()
}

/// A line a client sends: a JSON value it wrote, or bytes it did not.
#[derive(Clone, Debug)]
enum Line {
    Json(Value),
    Raw(Vec<u8>),
}

impl Line {
    /// The bytes on the wire, without the newline: a newline inside a line would make it two lines.
    fn bytes(&self) -> Vec<u8> {
        let bytes = match self {
            Line::Json(value) => value.to_string().into_bytes(),
            Line::Raw(bytes) => bytes.clone(),
        };
        bytes.into_iter().filter(|b| *b != b'\n').collect()
    }

    /// The method of a JSON object that has a text one.
    fn method(&self) -> Option<&str> {
        match self {
            Line::Json(value) => value.get("method").and_then(Value::as_str),
            Line::Raw(_) => None,
        }
    }
}

/// Any JSON value.
fn arb_json() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::from),
        any::<i64>().prop_map(Value::from),
        any::<u64>().prop_map(Value::from),
        any::<f64>().prop_map(Value::from),
        "\\PC{0,24}".prop_map(Value::from),
    ];
    leaf.prop_recursive(4, 40, 4, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..4).prop_map(Value::Array),
            prop::collection::vec(("[a-z_]{0,8}", inner), 0..4).prop_map(|pairs| Value::Object(pairs.into_iter().collect())),
        ]
    })
}

/// An object that looks like a request, with the faults a client makes: an `id` that is any JSON, a `method` that is not text,
/// `params` of any shape, a key the protocol does not have (any characters, long ones too).
fn arb_request() -> impl Strategy<Value = Value> {
    let id = prop_oneof![
        3 => Just(None),
        1 => Just(Some(Value::Null)),
        2 => any::<i64>().prop_map(|n| Some(json!(n))),
        2 => "\\PC{0,12}".prop_map(|s| Some(json!(s))),
        1 => arb_json().prop_map(Some),
    ];
    let method = prop_oneof![8 => "[a-z.]{1,12}".prop_map(Value::from), 1 => "\\PC{0,200}".prop_map(Value::from), 1 => arb_json()];
    let params = prop_oneof![1 => Just(None), 3 => arb_json().prop_map(Some)];
    let extra = prop_oneof![6 => Just(None), 1 => ("\\PC{1,200}", arb_json()).prop_map(Some)];
    (id, method, params, extra).prop_map(|(id, method, params, extra)| {
        let mut object = Map::new();
        if let Some(id) = id {
            object.insert("id".to_owned(), id);
        }
        object.insert("method".to_owned(), method);
        if let Some(params) = params {
            object.insert("params".to_owned(), params);
        }
        if let Some((key, value)) = extra {
            object.insert(key, value);
        }
        Value::Object(object)
    })
}

fn arb_line() -> impl Strategy<Value = Line> {
    prop_oneof![
        8 => arb_request().prop_map(Line::Json),
        2 => arb_json().prop_map(Line::Json),
        3 => prop::collection::vec(any::<u8>(), 0..120).prop_map(Line::Raw),
        1 => "\\PC{0,60}".prop_map(|s| Line::Raw(s.into_bytes())),
        // Nesting past serde_json's limit of 128 levels, closed and not.
        1 => (1_usize..400).prop_map(|n| Line::Raw(format!("{}{}", "[".repeat(n), "]".repeat(n)).into_bytes())),
        1 => (1_usize..400).prop_map(|n| Line::Raw("[".repeat(n).into_bytes())),
        1 => (1_usize..300).prop_map(|n| Line::Raw(format!("{{\"method\":\"deep\",\"params\":{}{}}}", "[".repeat(n), "]".repeat(n)).into_bytes())),
        // A request with a long string in it, and blank lines.
        1 => (1_usize..70_000).prop_map(|n| Line::Raw(format!("{{\"id\":1,\"method\":\"long\",\"params\":{{\"s\":\"{}\"}}}}", "x".repeat(n)).into_bytes())),
        1 => Just(Line::Raw(Vec::new())),
        1 => Just(Line::Raw(b"  \t ".to_vec())),
    ]
}

/// What a case sent to the server and what came back.
struct Talk {
    /// Each reply line, parsed.
    replies: Vec<Value>,
    /// The methods the fake window was asked to run, in order.
    seen: Vec<String>,
}

/// The server on a free port, with a fake window that records each method and answers it.
fn serve() -> (SocketAddr, Arc<Mutex<Vec<String>>>) {
    let limits =
        Limits { idle: Duration::from_secs(10), line: Duration::from_secs(10), write: Duration::from_secs(10), reply: Duration::from_secs(10) };
    let server = Server::bind(0).unwrap().with_limits(limits);
    let addr = SocketAddr::from(([127, 0, 0, 1], server.port()));
    let inbox = server.spawn(egui::Context::default()).unwrap();
    let seen: Arc<Mutex<Vec<String>>> = Arc::default();
    let log = Arc::clone(&seen);
    std::thread::spawn(move || {
        for request in inbox {
            if request.claim() {
                log.lock().unwrap().push(request.method.clone());
                request.reply.send(Ok(json!({"saw": request.method}))).ok();
            }
        }
    });
    (addr, seen)
}

/// Sends `lines` (joined with `\n`, or `\r\n`) in one connection, then ends the sending side, and reads until the server closes.
fn talk(lines: &[Line], crlf: bool) -> Talk {
    let (addr, seen) = serve();
    let mut input = Vec::new();
    for line in lines {
        input.extend(line.bytes());
        input.extend(if crlf { &b"\r\n"[..] } else { &b"\n"[..] });
    }
    let stream = TcpStream::connect(addr).unwrap();
    stream.set_read_timeout(Some(WAIT)).unwrap();
    let mut writer = stream.try_clone().unwrap();
    // Written on its own thread: the server may answer and stop reading while a long input is still going out.
    let sender = std::thread::spawn(move || {
        // A refused connection closes while the rest is unsent: the write fails, and that is the server's right.
        let _ = writer.write_all(&input);
        let _ = writer.shutdown(Shutdown::Write);
    });
    let mut text = Vec::new();
    let mut reader = stream;
    // A reset after the refusal is the end of the connection too: what arrived before it is what counts.
    let _ = reader.read_to_end(&mut text);
    sender.join().unwrap();
    let text = String::from_utf8(text).expect("the server writes UTF-8");
    let replies = text.lines().map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("a reply that is not JSON ({e}): {l:?}"))).collect();
    let seen = seen.lock().unwrap().clone();
    Talk { replies, seen }
}

/// How many of the lines the server reads: lines that are not blank, up to and including the first it refuses (a refusal
/// ends the connection, and what follows is never read). Counted only to bound the replies, not to predict them.
fn most_replies(lines: &[Line]) -> usize {
    lines.iter().filter(|l| !l.bytes().iter().all(u8::is_ascii_whitespace)).count()
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 96, failure_persistence: None, ..ProptestConfig::default() })]

    /// Whatever the lines: every reply is a JSON object with a boolean `ok`; there are no more replies than lines; a refusal
    /// (it says it is closing the connection) is the last thing sent; the window is asked only for methods that a request
    /// the client wrote carried; and no thread of the server caught a panic.
    #[test]
    fn random_lines_get_replies_in_protocol_and_never_panic(lines in prop::collection::vec(arb_line(), 0..10), crlf in any::<bool>()) {
        crate::capture_logs();
        let talk = talk(&lines, crlf);
        prop_assert!(talk.replies.len() <= most_replies(&lines), "{} replies to {} lines", talk.replies.len(), lines.len());
        for reply in &talk.replies {
            prop_assert!(reply.is_object() && reply["ok"].is_boolean(), "a reply that is not in protocol: {}", reply);
            let id = &reply["id"];
            prop_assert!(id.is_null() || id.is_number() || id.is_string(), "an id that is none of null, a number and a text: {}", reply);
            if reply["ok"] == true {
                prop_assert!(reply.get("result").is_some() && reply.get("error").is_none(), "{}", reply);
            } else {
                prop_assert!(reply["error"].as_str().is_some_and(|e| !e.is_empty()), "an error with no text: {}", reply);
                prop_assert!(!reply["error"].as_str().unwrap_or_default().chars().any(char::is_control), "an error with a control character: {}", reply);
            }
        }
        let refusals: Vec<usize> = talk.replies.iter().enumerate().filter(|(_, r)| r["error"].as_str().is_some_and(|e| e.contains("closing the connection"))).map(|(i, _)| i).collect();
        prop_assert!(refusals.len() <= 1 && refusals.first().is_none_or(|i| *i + 1 == talk.replies.len()), "a refusal is the last reply: {:?}", talk.replies);
        // The methods of the requests written as JSON, and of the two raw requests the strategy above writes by hand.
        let mut written: HashSet<&str> = lines.iter().filter_map(Line::method).collect();
        written.extend(["deep", "long"]);
        for method in &talk.seen {
            prop_assert!(written.contains(method.as_str()), "the window was asked to run {:?}, which no request carried", method);
        }
        prop_assert!(talk.seen.len() <= talk.replies.len(), "the window ran more than the server answered");
        let caught = caught_panics();
        prop_assert!(caught.is_empty(), "a thread of the server caught a panic: {:?}", caught);
    }

    /// A request before the junk still runs and is answered, and the junk ends the connection, not the server: a new
    /// connection to the same server is served as ever.
    #[test]
    fn a_request_before_junk_runs_and_the_server_serves_the_next_connection(junk in arb_line()) {
        crate::capture_logs();
        let (addr, seen) = serve();
        let mut first = TcpStream::connect(addr).unwrap();
        first.set_read_timeout(Some(WAIT)).unwrap();
        let mut input = b"{\"id\":1,\"method\":\"first\"}\n".to_vec();
        input.extend(junk.bytes());
        input.push(b'\n');
        let _ = first.write_all(&input);
        let _ = first.shutdown(Shutdown::Write);
        let mut text = Vec::new();
        let _ = first.read_to_end(&mut text);
        let text = String::from_utf8_lossy(&text).into_owned();
        prop_assert!(text.lines().next().is_some_and(|l| l.starts_with("{\"id\":1,\"ok\":true")), "the request before the junk was not answered: {:?}", text);
        let first_seen = seen.lock().unwrap().first().cloned();
        prop_assert_eq!(first_seen.as_deref(), Some("first"));

        let mut second = TcpStream::connect(addr).unwrap();
        second.set_read_timeout(Some(WAIT)).unwrap();
        second.write_all(b"{\"id\":2,\"method\":\"second\"}\n").unwrap();
        let mut line = String::new();
        let mut byte = [0_u8; 1];
        while second.read(&mut byte).unwrap_or(0) == 1 && byte[0] != b'\n' {
            line.push(char::from(byte[0]));
        }
        prop_assert!(line.starts_with("{\"id\":2,\"ok\":true"), "the next connection was not served: {:?}", line);
        let caught = caught_panics();
        prop_assert!(caught.is_empty(), "a thread of the server caught a panic: {:?}", caught);
    }
}
