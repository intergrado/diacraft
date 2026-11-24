//! The connection to a running app, against a fake app on a real socket: who it talks to, and what it does
//! when the other end is slow, wrong, or gone.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use diacraft_mcp::{Backend, ClientConfig, ConnectError, ControlClient, DEFAULT_ADDR, connect_or_headless};
use serde_json::{Value, json};

use crate::{call, lines, talk_on, tool_json, tool_text};

/// What a fake app does with one request.
enum Act {
    /// `{"id", "ok": true, "result"}`.
    Result(Value),
    /// `{"id", "ok": false, "error"}`.
    Error(String),
    /// These bytes, as they are (no newline added).
    Raw(Vec<u8>),
    /// Reads the request and never answers.
    Silent,
    /// Answers, then closes the connection, as the app does after its idle time.
    ResultThenClose(Value),
    /// A refusal with no `id`, then close: what the app says to a line it will not read.
    RefusalThenClose(String),
    /// Answers with an `id` that is not the request's.
    WrongId(Value),
    /// Sends a byte every 50 milliseconds for ten seconds, and never a whole line.
    Trickle,
}

/// A fake DiaCraft control channel (or another app's) on a free port. It answers `ui.inspect` with the app it
/// pretends to be, and records every method it was asked, in order.
struct FakeApp {
    addr: String,
    seen: Arc<Mutex<Vec<String>>>,
    connections: Arc<AtomicUsize>,
    app: Arc<Mutex<String>>,
    stop: Arc<AtomicBool>,
}

type Special = dyn Fn(&str, &Value) -> Option<Act> + Send + Sync;

impl FakeApp {
    /// An app that says it is `app` and answers `engine.execute` with `{"ran": <command>}`.
    fn start(app: &str) -> FakeApp {
        FakeApp::with(app, |_, _| None)
    }

    /// As `start`, but `special` may decide what to do with a request first (`None`: the default).
    fn with(app: &str, special: impl Fn(&str, &Value) -> Option<Act> + Send + Sync + 'static) -> FakeApp {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let fake = FakeApp {
            addr: listener.local_addr().unwrap().to_string(),
            seen: Arc::default(),
            connections: Arc::default(),
            app: Arc::new(Mutex::new(app.to_owned())),
            stop: Arc::default(),
        };
        let special: Arc<Special> = Arc::new(special);
        let (seen, connections, name, stop) = (fake.seen.clone(), fake.connections.clone(), fake.app.clone(), fake.stop.clone());
        std::thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        // On macOS (and the BSDs) an accepted socket keeps the listener's non-blocking mode, where on Linux it does not: the reads
                        // below wait for the request, so the connection is put back to blocking.
                        stream.set_nonblocking(false).unwrap();
                        connections.fetch_add(1, Ordering::SeqCst);
                        let (seen, name, special) = (seen.clone(), name.clone(), special.clone());
                        std::thread::spawn(move || serve_connection(stream, &seen, &name, &*special));
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(5)),
                    Err(_) => return,
                }
            }
        });
        fake
    }

    /// The methods asked so far.
    fn seen(&self) -> Vec<String> {
        self.seen.lock().unwrap().clone()
    }

    fn connections(&self) -> usize {
        self.connections.load(Ordering::SeqCst)
    }

    /// From now on the fake says it is `app`.
    fn become_app(&self, app: &str) {
        *self.app.lock().unwrap() = app.to_owned();
    }

    /// Stops accepting connections and closes the port, as an app that quit does. Connections already open stay
    /// as they are. Returns once the port is closed.
    fn stop_listening(&self) {
        self.stop.store(true, Ordering::SeqCst);
        let end = Instant::now() + Duration::from_secs(5);
        while TcpStream::connect(&self.addr).is_ok() && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(10));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

impl Drop for FakeApp {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

fn serve_connection(stream: TcpStream, seen: &Mutex<Vec<String>>, app: &Mutex<String>, special: &Special) {
    let mut writer = stream.try_clone().unwrap();
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    loop {
        line.clear();
        if !matches!(reader.read_line(&mut line), Ok(n) if n > 0) {
            return;
        }
        let Ok(request) = serde_json::from_str::<Value>(&line) else { return };
        let (id, method, params) = (request["id"].clone(), request["method"].as_str().unwrap_or("").to_owned(), request["params"].clone());
        seen.lock().unwrap().push(method.clone());
        let act = special(&method, &params).unwrap_or_else(|| match method.as_str() {
            "ui.inspect" => Act::Result(json!({"app": *app.lock().unwrap(), "version": env!("CARGO_PKG_VERSION")})),
            "engine.execute" => Act::Result(json!({"ran": params["command"]})),
            "engine.commands" => Act::Result(json!([])),
            other => Act::Error(format!("unknown method `{other}`")),
        });
        let reply = |value: Value| format!("{value}\n").into_bytes();
        let (bytes, close) = match act {
            Act::Result(result) => (reply(json!({"id": id, "ok": true, "result": result})), false),
            Act::Error(error) => (reply(json!({"id": id, "ok": false, "error": error})), false),
            Act::Raw(bytes) => (bytes, false),
            Act::Silent => continue,
            Act::Trickle => {
                for _ in 0..200 {
                    if writer.write_all(b"x").and_then(|()| writer.flush()).is_err() {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                continue;
            }
            Act::ResultThenClose(result) => (reply(json!({"id": id, "ok": true, "result": result})), true),
            Act::RefusalThenClose(error) => (reply(json!({"ok": false, "error": error})), true),
            Act::WrongId(result) => (reply(json!({"id": id.as_u64().unwrap_or(0) + 1000, "ok": true, "result": result})), false),
        };
        if writer.write_all(&bytes).and_then(|()| writer.flush()).is_err() || close {
            return;
        }
    }
}

/// Short limits, so a test about a peer that does not answer does not take the default minute and a half.
fn quick() -> ClientConfig {
    ClientConfig {
        connect: Duration::from_millis(500),
        probe: Duration::from_millis(400),
        reply: Duration::from_millis(400),
        ..ClientConfig::default()
    }
}

/// Runs `f` on another thread and fails the test if it has not come back in `secs`. A client that waits for ever on
/// a peer that never answers is the bug these tests are for, and it must show as a failure, not as a test that never ends.
fn within<T: Send + 'static>(secs: u64, f: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        // The test has gone if it is not listening any more.
        let _ = tx.send(f());
    });
    rx.recv_timeout(Duration::from_secs(secs)).unwrap_or_else(|_| panic!("did not come back in {secs} s"))
}

fn connect(app: &FakeApp) -> Result<ControlClient, ConnectError> {
    ControlClient::connect_with(&app.addr, quick())
}

// ---------- the limits ----------

#[test]
fn the_default_limits_are_the_documented_ones() {
    // docs/mcp.md: 90 seconds for a reply, 16 MiB of reply, 5 seconds to say who it is, port 7997.
    let config = ClientConfig::default();
    assert_eq!(config.connect, Duration::from_millis(800));
    assert_eq!(config.probe, Duration::from_secs(5));
    assert_eq!(config.reply, Duration::from_secs(90));
    assert_eq!(config.max_reply, 16 * 1024 * 1024);
    assert_eq!(DEFAULT_ADDR, "127.0.0.1:7997");
}

// ---------- who answers ----------

#[test]
fn connect_refuses_another_app() {
    let other = FakeApp::start("deckcraft");
    match connect(&other) {
        Err(ConnectError::NotDiacraft { app }) => assert_eq!(app, "deckcraft"),
        Err(other) => panic!("expected NotDiacraft, got {other:?}"),
        Ok(_) => panic!("connected to another app"),
    }
    // The question was asked and nothing else: no command was sent to it.
    assert_eq!(other.seen(), ["ui.inspect"]);
    // The same through the default constructor.
    assert!(matches!(ControlClient::connect(&other.addr), Err(ConnectError::NotDiacraft { .. })));
}

#[test]
fn connect_refuses_an_answer_that_names_no_app() {
    let named_badly = FakeApp::with("diacraft", |m, _| (m == "ui.inspect").then(|| Act::Result(json!({"app": 7}))));
    let no_app = FakeApp::with("diacraft", |m, _| (m == "ui.inspect").then(|| Act::Result(json!({"frames": 1}))));
    let an_error = FakeApp::with("diacraft", |m, _| (m == "ui.inspect").then(|| Act::Error("no such method".into())));
    let nasty = FakeApp::with("diacraft", |m, _| (m == "ui.inspect").then(|| Act::Result(json!({"app": "evil\n\u{1b}[2J\u{202e}"}))));
    for fake in [&named_badly, &no_app, &an_error] {
        assert_eq!(connect(fake).err(), Some(ConnectError::NotDiacraft { app: "unknown".into() }));
    }
    // What the other app calls itself does not reach a message as it is.
    let Some(ConnectError::NotDiacraft { app }) = connect(&nasty).err() else { panic!("expected NotDiacraft") };
    assert!(!app.chars().any(|c| c.is_control() || c == '\u{202e}'), "{app:?}");
    assert!(app.contains("\\u{1b}"), "{app:?}");
}

/// An app that is full, or will not read the question, answers it with a refusal that has no id and closes. The refusal's own
/// words are the reason to give, as for a call that is refused (`a_refusal_without_an_id_means_nothing_ran`): it was read as
/// "its answer names another request", which is wrong and drops the app's text.
#[test]
fn a_refusal_at_the_probe_says_what_the_app_said() {
    let full =
        FakeApp::with("diacraft", |m, _| (m == "ui.inspect").then(|| Act::RefusalThenClose("too many control connections (at most 16)".into())));
    let Err(ConnectError::Refused(why)) = connect(&full) else { panic!("expected the connection to be refused") };
    assert!(why.contains("too many control connections (at most 16)"), "the app's text is lost: {why}");
    assert!(why.contains("refused") && why.contains(&full.addr), "{why}");
    assert!(!why.contains("another request"), "a refusal is not a reply to another request: {why}");
    // Nothing but the question was sent.
    assert_eq!(full.seen(), ["ui.inspect"]);
    // Through the default mode: the person is told why the app was not used.
    let (backend, why) = connect_or_headless(&full.addr);
    assert!(!backend.has_app());
    assert!(why.is_some_and(|e| e.to_string().contains("too many control connections")));
    // The app's words are escaped like any other text from it.
    let nasty = FakeApp::with("diacraft", |m, _| (m == "ui.inspect").then(|| Act::RefusalThenClose("full\n\u{1b}[2J\u{202e}".into())));
    let Err(ConnectError::Refused(why)) = connect(&nasty) else { panic!("expected the connection to be refused") };
    assert!(!why.chars().any(|c| c.is_control() || c == '\u{202e}') && why.contains("full\\n\\u{1b}[2J"), "{why:?}");
    // A reply with another id that says it did not work is still a reply to another request, and an id-less success is no
    // refusal: only `ok: false` without an id is the app turning the line away.
    let confused = FakeApp::with("diacraft", |m, _| (m == "ui.inspect").then(|| Act::WrongId(json!({"app": "diacraft"}))));
    let Err(ConnectError::Refused(why)) = connect(&confused) else { panic!("expected the connection to be refused") };
    assert!(why.contains("another request"), "{why}");
}

#[test]
fn connect_accepts_diacraft_and_forwards_calls() {
    let app = FakeApp::start("diacraft");
    let mut client = connect(&app).expect("a fake DiaCraft is accepted");
    let ran = client.call("engine.execute", &json!({"command": "app.version", "params": {}})).unwrap();
    assert_eq!(ran, json!({"ran": "app.version"}));
    assert_eq!(app.seen(), ["ui.inspect", "engine.execute"]);
    // An error answer is the call's error, as the app said it.
    assert_eq!(client.call("no.such.method", &json!({})), Err("unknown method `no.such.method`".to_owned()));
    // And the connection is still good afterwards.
    assert!(client.call("engine.commands", &json!({})).is_ok());
    assert_eq!(app.connections(), 1, "one connection served all of it");
}

#[test]
fn errors_from_the_app_are_escaped() {
    let app = FakeApp::with("diacraft", |m, _| (m == "engine.execute").then(|| Act::Error("bad\n\u{1b}[2J\u{202e} thing".into())));
    let mut client = connect(&app).unwrap();
    let error = client.call("engine.execute", &json!({"command": "x"})).unwrap_err();
    assert!(!error.chars().any(|c| c.is_control() || c == '\u{202e}'), "{error:?}");
}

#[test]
fn default_mode_falls_back_to_headless_when_other_app_answers() {
    let other = FakeApp::start("deckcraft");
    let (mut backend, why) = connect_or_headless(&other.addr);
    assert!(matches!(backend, Backend::Headless(_)), "the backend is the app that answered");
    assert_eq!(why, Some(ConnectError::NotDiacraft { app: "deckcraft".into() }));

    // Everything a client then does happens in this process's own session.
    let replies = talk_on(
        &mut backend,
        &lines(&[call(1, "command_run", json!({"id": "file.new"})), call(2, "doc_inspect", json!({})), call(3, "ui_inspect", json!({}))]),
    );
    assert_eq!(replies[0]["result"]["isError"], false, "{replies:?}");
    assert_eq!(tool_json(&replies[1])["pages"].as_array().map(Vec::len), Some(1));
    assert!(tool_text(&replies[2]).contains("needs the app"), "{replies:?}");
    // The other app was asked who it is, and was sent nothing else: it was not driven.
    assert_eq!(other.seen(), ["ui.inspect"]);
}

#[test]
fn default_mode_uses_the_app_when_it_is_diacraft() {
    let app = FakeApp::start("diacraft");
    let (mut backend, why) = connect_or_headless(&app.addr);
    assert!(matches!(backend, Backend::Remote(_)));
    assert_eq!(why, None);
    let replies = talk_on(&mut backend, &lines(&[call(1, "command_run", json!({"id": "file.new"}))]));
    assert_eq!(tool_json(&replies[0]), json!({"ran": "file.new"}), "the command went to the app");
    assert_eq!(app.seen(), ["ui.inspect", "engine.execute"]);
}

#[test]
fn default_mode_with_nothing_listening_is_headless() {
    let gone = {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().to_string()
    };
    let (backend, why) = connect_or_headless(&gone);
    assert!(matches!(backend, Backend::Headless(_)));
    assert!(matches!(why, Some(ConnectError::Refused(_))), "{why:?}");
}

#[test]
fn the_tools_reach_the_app_by_the_methods_of_the_control_protocol() {
    let app = FakeApp::start("diacraft");
    let mut backend = Backend::Remote(connect(&app).unwrap());
    let replies = talk_on(
        &mut backend,
        &lines(&[
            call(1, "command_list", json!({})),
            call(2, "command_run", json!({"id": "page.add", "params": {"name": "X"}})),
            call(3, "command_batch", json!({"steps": [{"id": "page.add"}], "label": "L"})),
            call(4, "doc_inspect", json!({})),
            call(5, "ui_inspect", json!({})),
            call(6, "ui_screenshot", json!({"path": "/tmp/x.png"})),
        ]),
    );
    assert_eq!(replies.len(), 6);
    assert_eq!(app.seen(), ["ui.inspect", "engine.commands", "engine.execute", "engine.execute", "document.inspect", "ui.inspect", "ui.screenshot"]);
}

// ---------- when the other end is wrong ----------

#[test]
fn connect_to_nothing_is_refused() {
    let gone = {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().to_string()
    };
    assert!(matches!(ControlClient::connect_with(&gone, quick()), Err(ConnectError::Refused(_))));
    assert!(matches!(ControlClient::connect_with("not an address", quick()), Err(ConnectError::Refused(_))));
}

#[test]
fn a_silent_peer_is_refused_in_time() {
    let silent = FakeApp::with("diacraft", |_, _| Some(Act::Silent));
    let addr = silent.addr.clone();
    let start = Instant::now();
    let refused = within(20, move || ControlClient::connect_with(&addr, quick()).err());
    assert!(matches!(refused, Some(ConnectError::Refused(_))), "{refused:?}");
    // A hang guard: the deadline is 400 ms, and a client that waited on a silent peer would not come back at all.
    assert!(start.elapsed() < Duration::from_secs(30), "took {:?}", start.elapsed());
}

#[test]
fn a_peer_that_trickles_cannot_hold_the_client() {
    // A byte every 50 ms never lets a read time out, so only a limit on the whole wait stops this.
    let slow = FakeApp::with("diacraft", |_, _| Some(Act::Trickle));
    let addr = slow.addr.clone();
    let start = Instant::now();
    let refused = within(20, move || ControlClient::connect_with(&addr, quick()).err());
    assert!(matches!(refused, Some(ConnectError::Refused(_))), "{refused:?}");
    // A hang guard: the deadline is 400 ms, and a trickle with no limit on the whole wait would not end.
    assert!(start.elapsed() < Duration::from_secs(30), "a byte every 50 ms held the client for {:?}", start.elapsed());
}

/// A fake app that says who it is and then takes what it is sent 8 KiB at a time, one chunk a second, and never answers.
fn slow_reader_app() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            std::thread::spawn(move || {
                let mut writer = stream.try_clone().unwrap();
                let mut reader = BufReader::new(stream);
                let mut question = String::new();
                if !matches!(reader.read_line(&mut question), Ok(n) if n > 0) {
                    return;
                }
                if writeln!(writer, "{}", json!({"id": 1, "ok": true, "result": {"app": "diacraft"}})).is_err() {
                    return;
                }
                let mut chunk = [0_u8; 8192];
                while matches!(std::io::Read::read(&mut reader, &mut chunk), Ok(n) if n > 0) {
                    std::thread::sleep(Duration::from_secs(1));
                }
            });
        }
    });
    addr
}

#[test]
fn a_peer_that_reads_slowly_cannot_hold_the_client_while_it_sends() {
    // The largest request the app reads is 4 MiB, more than the sockets hold. A peer that takes it 8 KiB a second keeps every
    // write making progress, so a limit on each write never fires, and sending the rest would take minutes: only one budget for
    // sending and waiting together stops this. (3 seconds here; the default is 90.)
    let addr = slow_reader_app();
    let config = ClientConfig { reply: Duration::from_secs(3), ..quick() };
    let mut client = ControlClient::connect_with(&addr, config).unwrap();
    let request = json!({"command": "page.rename", "params": {"id": 1, "name": "x".repeat(4 * 1024 * 1024 - 1024)}});
    let start = Instant::now();
    let error = within(30, move || client.call("engine.execute", &request).unwrap_err());
    // A hang guard: the budget is 3 s, and sending the 4 MiB at 8 KiB a second would take minutes.
    assert!(start.elapsed() < Duration::from_secs(60), "a peer reading 8 KiB a second held the client for {:?}", start.elapsed());
    assert!(error.contains("may have completed"), "{error}");
}

#[test]
fn a_batch_over_the_step_limit_is_refused_before_anything_is_built_or_sent() {
    // The engine, here and in the app, limits a batch to 256 steps. The tool refuses a longer one itself, before it builds a
    // command for each step (a line may hold a million) and before a request leaves: the app is not asked.
    let app = FakeApp::start("diacraft");
    let mut backend = Backend::Remote(connect(&app).unwrap());
    let batch = |n: usize| call(1, "command_batch", json!({"steps": (0..n).map(|_| json!({"id": "page.add"})).collect::<Vec<_>>()}));
    let replies = talk_on(&mut backend, &lines(&[batch(257)]));
    assert_eq!(replies[0]["result"]["isError"], true, "{replies:?}");
    assert!(tool_text(&replies[0]).contains("holds 257 steps") && tool_text(&replies[0]).contains("at most 256"), "{replies:?}");
    assert_eq!(app.seen(), ["ui.inspect"], "the request was sent to the app");
    // Exactly the limit is sent.
    let replies = talk_on(&mut backend, &lines(&[batch(256)]));
    assert_eq!(replies[0]["result"]["isError"], false, "{replies:?}");
    assert_eq!(app.seen(), ["ui.inspect", "engine.execute"]);
}

#[test]
fn a_reconnect_is_logged_as_a_warning() {
    // A restarted app is a new document, and the next command is sent to it unannounced: the server says so in its log.
    crate::capture_logs();
    let first = Arc::new(AtomicBool::new(true));
    let app = FakeApp::with("diacraft", {
        let first = first.clone();
        move |m, _| (m == "engine.execute" && first.swap(false, Ordering::SeqCst)).then(|| Act::ResultThenClose(json!({"ran": "first"})))
    });
    let mut client = connect(&app).unwrap();
    assert!(client.call("engine.execute", &json!({"command": "a"})).is_ok());
    assert!(
        crate::logged().iter().all(|record| !record.contains(&app.addr)),
        "a warning before there was anything to warn about: {:?}",
        crate::logged()
    );
    std::thread::sleep(Duration::from_millis(150));
    assert!(client.call("engine.execute", &json!({"command": "b"})).is_ok());
    let records: Vec<String> = crate::logged().into_iter().filter(|record| record.contains(&app.addr)).collect();
    assert_eq!(records.len(), 1, "{records:?}");
    assert!(records[0].starts_with("WARN") && records[0].contains("opened again"), "{records:?}");
}

#[test]
fn a_peer_that_does_not_speak_json_is_refused() {
    let web = FakeApp::with("diacraft", |_, _| Some(Act::Raw(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n".to_vec())));
    assert!(matches!(connect(&web), Err(ConnectError::Refused(_))));
}

#[test]
fn an_endless_reply_is_cut_off() {
    let endless = FakeApp::with("diacraft", |_, _| Some(Act::Raw(vec![b'x'; 64 * 1024])));
    let config = ClientConfig { max_reply: 1024, ..quick() };
    let start = Instant::now();
    let Err(ConnectError::Refused(why)) = ControlClient::connect_with(&endless.addr, config) else { panic!("expected a refusal") };
    assert!(why.contains("longer than"), "{why}");
    // A hang guard: the reply is cut at 1 KiB, and an endless one would not end.
    assert!(start.elapsed() < Duration::from_secs(30));

    // After the handshake, in a call: the call fails, and says it may have run.
    let big = FakeApp::with("diacraft", |m, _| (m == "engine.execute").then(|| Act::Raw(vec![b'y'; 64 * 1024])));
    let mut client = ControlClient::connect_with(&big.addr, ClientConfig { max_reply: 1024, ..quick() }).unwrap();
    let error = client.call("engine.execute", &json!({"command": "x"})).unwrap_err();
    assert!(error.contains("longer than") && error.contains("may have completed"), "{error}");
}

#[test]
fn a_reply_that_never_comes_is_an_error_that_says_it_may_have_run() {
    let slow = FakeApp::with("diacraft", |m, _| (m == "engine.execute").then_some(Act::Silent));
    let mut client = connect(&slow).unwrap();
    let start = Instant::now();
    let error = within(20, move || client.call("engine.execute", &json!({"command": "x"})).unwrap_err());
    assert!(error.contains("may have completed"), "{error}");
    // A hang guard: the reply deadline is 400 ms, and a reply that never comes would not end the call.
    assert!(start.elapsed() < Duration::from_secs(30), "took {:?}", start.elapsed());
}

#[test]
fn a_reply_to_another_request_drops_the_connection() {
    let confused = FakeApp::with("diacraft", |m, _| (m == "engine.execute").then(|| Act::WrongId(json!({"ran": "wrong"}))));
    let mut client = connect(&confused).unwrap();
    let error = client.call("engine.execute", &json!({"command": "x"})).unwrap_err();
    assert!(error.contains("id") && error.contains("may have completed"), "{error}");
    // The stream is not trusted any more: the next call starts on a new connection, which is checked again.
    assert!(client.call("engine.commands", &json!({})).is_ok());
    assert_eq!(confused.connections(), 2);
    assert_eq!(confused.seen().iter().filter(|m| *m == "ui.inspect").count(), 2);
}

#[test]
fn a_refusal_without_an_id_means_nothing_ran() {
    let full = FakeApp::with("diacraft", |m, _| (m == "engine.execute").then(|| Act::RefusalThenClose("too many control connections".into())));
    let mut client = connect(&full).unwrap();
    let error = client.call("engine.execute", &json!({"command": "x"})).unwrap_err();
    assert!(error.contains("refused") && error.contains("nothing was run") && error.contains("too many control connections"), "{error}");
}

#[test]
fn a_request_the_app_would_refuse_for_its_size_is_not_sent() {
    let app = FakeApp::start("diacraft");
    let mut client = connect(&app).unwrap();
    let huge = "x".repeat(5 * 1024 * 1024);
    let error = client.call("engine.execute", &json!({"command": "page.rename", "params": {"name": huge}})).unwrap_err();
    assert!(error.contains("nothing was sent"), "{error}");
    assert_eq!(app.seen(), ["ui.inspect"]);
    // The connection is still good.
    assert!(client.call("engine.commands", &json!({})).is_ok());
    assert_eq!(app.connections(), 1);
}

// ---------- when the app closes the connection ----------

#[test]
fn a_connection_the_app_closed_is_opened_again_and_checked_again() {
    // The app closes a connection that has been idle for 5 minutes, without a word.
    let first = Arc::new(AtomicBool::new(true));
    let app = FakeApp::with("diacraft", {
        let first = first.clone();
        move |m, _| (m == "engine.execute" && first.swap(false, Ordering::SeqCst)).then(|| Act::ResultThenClose(json!({"ran": "first"})))
    });
    let mut client = connect(&app).unwrap();
    assert_eq!(client.call("engine.execute", &json!({"command": "a"})).unwrap(), json!({"ran": "first"}));
    std::thread::sleep(Duration::from_millis(150));
    assert_eq!(client.call("engine.execute", &json!({"command": "b"})).unwrap(), json!({"ran": "b"}));
    assert_eq!(app.connections(), 2);
    assert_eq!(app.seen(), ["ui.inspect", "engine.execute", "ui.inspect", "engine.execute"], "the new connection asked who it was talking to");
}

#[test]
fn a_reconnect_to_another_app_sends_nothing() {
    let first = Arc::new(AtomicBool::new(true));
    let app = FakeApp::with("diacraft", {
        let first = first.clone();
        move |m, _| (m == "engine.execute" && first.swap(false, Ordering::SeqCst)).then(|| Act::ResultThenClose(json!({"ran": "first"})))
    });
    let mut client = connect(&app).unwrap();
    assert!(client.call("engine.execute", &json!({"command": "a"})).is_ok());
    // The port is taken over by another app while the connection is closed.
    app.become_app("deckcraft");
    std::thread::sleep(Duration::from_millis(150));
    let error = client.call("engine.execute", &json!({"command": "b"})).unwrap_err();
    assert!(error.contains("deckcraft") && error.contains("nothing was sent"), "{error}");
    assert_eq!(app.seen(), ["ui.inspect", "engine.execute", "ui.inspect"], "the second command never left");
}

#[test]
fn an_app_that_is_gone_is_an_error_that_says_nothing_was_sent() {
    let first = Arc::new(AtomicBool::new(true));
    let app = FakeApp::with("diacraft", {
        let first = first.clone();
        move |m, _| (m == "engine.execute" && first.swap(false, Ordering::SeqCst)).then(|| Act::ResultThenClose(json!({"ran": "first"})))
    });
    let mut client = connect(&app).unwrap();
    assert!(client.call("engine.execute", &json!({"command": "a"})).is_ok());
    // The app quits: the connection is closed and the port is not listened at any more.
    app.stop_listening();
    let error = client.call("engine.execute", &json!({"command": "b"})).unwrap_err();
    assert!(error.contains("not reachable") && error.contains("nothing was sent"), "{error}");
    assert_eq!(app.seen(), ["ui.inspect", "engine.execute"]);
}
