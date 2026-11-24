//! The control server's transport over real TCP, against a fake inbox that stands in for the window.
//!
//! Every test that expects a refusal also checks that the fake window saw nothing: junk must never
//! reach the UI thread. The budgets are short here (`Limits`), but each wait on the server is far longer
//! than the budget under test, so a slow machine fails nothing and a server that never acts still fails.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use diacraft_desktop::control_server::{ControlError, Limits, MAX_CONNECTIONS, MAX_LINE, Server, start};
use serde_json::{Value, json};

/// How long a test waits for the server to answer or to close. It only ends a test whose server never does: a hang guard, generous for a machine
/// at load 50, and no measure of cost. The lower bounds of the deadline tests (closed no sooner than the deadline) are exact at any load.
const WAIT: Duration = Duration::from_secs(30);

/// What the fake window saw, in order: the method of each request it took, `withdrawn <method>` for one
/// its caller had given up on.
type Seen = Arc<Mutex<Vec<String>>>;

struct Fixture {
    addr: SocketAddr,
    seen: Seen,
}

impl Fixture {
    fn seen(&self) -> Vec<String> {
        self.seen.lock().unwrap().clone()
    }

    fn client(&self) -> Client {
        Client::connect(self.addr)
    }
}

/// The server on a free port, and a thread that answers its requests the way a window would:
/// `echo` returns its params, `fail` an error, `big` a reply too large to sit in the socket buffers,
/// `hold` takes the request and never answers, `sleepy` is slow to take it.
fn start_with(limits: Limits) -> Fixture {
    let server = Server::bind(0).unwrap().with_limits(limits);
    let addr = SocketAddr::from(([127, 0, 0, 1], server.port()));
    let inbox = server.spawn(egui::Context::default()).unwrap();
    let seen: Seen = Arc::default();
    let log = Arc::clone(&seen);
    std::thread::spawn(move || {
        let mut held = Vec::new();
        for request in inbox {
            if request.method == "sleepy" {
                std::thread::sleep(Duration::from_millis(1500));
            }
            if !request.claim() {
                log.lock().unwrap().push(format!("withdrawn {}", request.method));
                continue;
            }
            log.lock().unwrap().push(request.method.clone());
            let reply = match request.method.as_str() {
                "echo" => Ok(request.params.clone()),
                "fail" => Err("nope".to_owned()),
                "big" => Ok(json!("x".repeat(48 * 1024 * 1024))),
                "hold" => {
                    held.push(request);
                    continue;
                }
                other => Err(format!("fake window: no method {other}")),
            };
            request.reply.send(reply).ok();
        }
    });
    Fixture { addr, seen }
}

fn start_default() -> Fixture {
    start_with(Limits::default())
}

struct Client {
    stream: TcpStream,
    reader: BufReader<TcpStream>,
}

impl Client {
    fn connect(addr: SocketAddr) -> Client {
        let stream = TcpStream::connect(addr).unwrap();
        stream.set_read_timeout(Some(WAIT)).unwrap();
        let reader = BufReader::new(stream.try_clone().unwrap());
        Client { stream, reader }
    }

    fn send(&mut self, text: &str) {
        self.stream.write_all(text.as_bytes()).unwrap();
    }

    /// Writes what it can: a server that refuses a request may close before reading all of it.
    fn send_what_fits(&mut self, bytes: &[u8]) {
        let _ = self.stream.write_all(bytes);
    }

    fn request(&mut self, value: &Value) -> Value {
        self.send(&format!("{value}\n"));
        self.reply()
    }

    fn reply(&mut self) -> Value {
        let mut line = String::new();
        let n = self.reader.read_line(&mut line).unwrap();
        assert!(n > 0, "the server closed the connection without a reply");
        serde_json::from_str(&line).unwrap_or_else(|e| panic!("not one JSON line: {e}: {:?}", &line[..line.len().min(200)]))
    }

    /// The connection is closed: the next read is a clean end of stream, not a reset and not more data.
    fn expect_closed(&mut self) {
        let mut rest = Vec::new();
        match self.reader.read_to_end(&mut rest) {
            Ok(_) => assert!(rest.is_empty(), "more than one reply: {:?}", String::from_utf8_lossy(&rest[..rest.len().min(300)])),
            Err(e) => panic!("the connection was reset instead of closed: {e}"),
        }
    }

    /// Reads to the end of the stream; the replies, one per line.
    fn all_replies(&mut self) -> Vec<Value> {
        let mut text = String::new();
        self.reader.read_to_string(&mut text).unwrap();
        text.lines().map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("not JSON: {e}: {l:?}"))).collect()
    }
}

/// The one error reply of a refusal: `ok` is false, the error says the connection is being closed, and
/// the connection then really closes.
fn expect_refused(client: &mut Client, mention: &str) -> String {
    let reply = client.reply();
    assert_eq!(reply["ok"], false, "{reply}");
    let error = reply["error"].as_str().unwrap().to_owned();
    assert!(error.contains(mention), "{error}");
    assert!(error.contains("closing the connection"), "{error}");
    client.expect_closed();
    error
}

#[test]
fn a_request_is_answered_with_its_id_and_a_result() {
    let fx = start_default();
    let mut c = fx.client();
    assert_eq!(c.request(&json!({"id": 1, "method": "echo", "params": {"a": [1, 2]}})), json!({"id": 1, "ok": true, "result": {"a": [1, 2]}}));
    assert_eq!(c.request(&json!({"id": "x", "method": "fail"})), json!({"id": "x", "ok": false, "error": "nope"}));
    // `id` and `params` are optional; a blank line is skipped; \r\n ends a line too.
    c.send("\n   \n{\"method\":\"echo\"}\r\n");
    assert_eq!(c.reply(), json!({"id": null, "ok": true, "result": null}));
    assert_eq!(fx.seen(), ["echo", "fail", "echo"]);
}

#[test]
fn malformed_line_gets_one_error_and_closes() {
    let fx = start_default();
    for junk in
        ["this is not json\n", "{\"method\": \n", "{'method':'echo'}\n", "{\"method\":\"echo\"} trailing\n", "\u{feff}{\"method\":\"echo\"}\n"]
    {
        let mut c = fx.client();
        // A good request after the junk, in the same write: it must not run.
        c.send(&format!("{junk}{{\"id\":2,\"method\":\"echo\"}}\n"));
        expect_refused(&mut c, "bad JSON");
    }
    assert_eq!(fx.seen(), Vec::<String>::new(), "junk reached the window");
}

#[test]
fn non_object_json_gets_one_error_and_closes() {
    let fx = start_default();
    for (junk, mention) in [
        ("[1,2]", "JSON object"),
        ("42", "JSON object"),
        ("\"echo\"", "JSON object"),
        ("null", "JSON object"),
        ("true", "JSON object"),
        ("{}", "method"),
        ("{\"id\":1}", "method"),
        ("{\"method\":5}", "method"),
        ("{\"method\":null}", "method"),
        ("{\"method\":[\"echo\"]}", "method"),
    ] {
        let mut c = fx.client();
        c.send(&format!("{junk}\n{{\"id\":2,\"method\":\"echo\"}}\n"));
        expect_refused(&mut c, mention);
    }
    assert_eq!(fx.seen(), Vec::<String>::new());
}

#[test]
fn invalid_utf8_gets_one_error_and_closes() {
    let fx = start_default();
    let mut c = fx.client();
    c.send_what_fits(b"{\"method\":\"\xff\xfe\"}\n{\"method\":\"echo\"}\n");
    expect_refused(&mut c, "UTF-8");
    assert_eq!(fx.seen(), Vec::<String>::new());
}

/// A browser page's cross-origin `fetch` to the port sends an HTTP request: its first line is not JSON,
/// so the connection closes before the body, which holds a request, is read.
#[test]
fn an_http_request_is_refused_before_its_body_runs() {
    let fx = start_default();
    let mut c = fx.client();
    c.send(
        "POST / HTTP/1.1\r\nHost: 127.0.0.1\r\nOrigin: https://elsewhere.example\r\nContent-Type: text/plain\r\n\r\n\
         {\"id\":7,\"method\":\"echo\"}\n",
    );
    expect_refused(&mut c, "bad JSON");
    assert_eq!(fx.seen(), Vec::<String>::new());
}

/// Closing a socket with the peer's bytes unread resets it, and on some systems the reset takes the answer the peer
/// has not yet read with it. So after a refusal the server reads and drops what the peer goes on sending, for a
/// moment: the peer's later writes are not met by a reset.
#[test]
fn a_refused_peer_is_not_reset_while_it_is_still_sending() {
    let fx = start_default();
    let mut c = fx.client();
    c.send("this is not json\n");
    expect_refused(&mut c, "bad JSON");
    // The end of stream has come, and the peer, which has not noticed, sends more: twice, with time between for a
    // reset to arrive if the server had closed.
    for round in 0..2 {
        c.stream.write_all(&vec![b'x'; 64 * 1024]).unwrap_or_else(|e| panic!("round {round}: the refused peer was reset while sending: {e}"));
        std::thread::sleep(Duration::from_millis(60));
    }
}

#[test]
fn a_request_before_the_junk_still_runs() {
    let fx = start_default();
    let mut c = fx.client();
    c.send("{\"id\":1,\"method\":\"echo\"}\nGET / HTTP/1.1\n{\"id\":2,\"method\":\"echo\"}\n");
    let replies = c.all_replies();
    assert_eq!(replies.len(), 2, "{replies:?}");
    assert_eq!(replies[0], json!({"id": 1, "ok": true, "result": null}));
    assert_eq!(replies[1]["ok"], false);
    assert_eq!(fx.seen(), ["echo"]);
}

/// A request object with a key the protocol does not have is an error, not a request with something
/// ignored: a misspelt `params` must not run as if there were none. The connection stays open, for
/// there is nothing in it that is not a request.
#[test]
fn unknown_keys_in_a_request_get_an_error_and_the_connection_stays() {
    let fx = start_default();
    let mut c = fx.client();
    for (request, mention) in [
        (json!({"id": 1, "method": "echo", "parms": {}}), "parms"),
        (json!({"id": 2, "method": "echo", "params": {}, "extra": true}), "extra"),
        (json!({"id": 3, "method": "echo", "jsonrpc": "2.0"}), "jsonrpc"),
        // Shown as sent, apart from what is dangerous.
        (json!({"id": 6, "method": "echo", "it's": true}), "`it's`"),
        (json!({"id": 7, "method": "echo", "k\u{1b}[2J": true}), "`k\\u{1b}[2J`"),
    ] {
        let reply = c.request(&request);
        assert_eq!(reply["ok"], false, "{reply}");
        assert_eq!(reply["id"], request["id"], "the error is addressed to the request: {reply}");
        let error = reply["error"].as_str().unwrap();
        assert!(error.contains(mention) && error.contains("id, method and params"), "{error}");
    }
    // An `id` that is not a number, a string or null is refused the same way.
    for id in [json!([1]), json!({"a": 1}), json!(true)] {
        let reply = c.request(&json!({"id": id, "method": "echo"}));
        assert_eq!(reply["ok"], false);
        assert_eq!(reply["id"], Value::Null);
        assert!(reply["error"].as_str().unwrap().contains("`id`"), "{reply}");
    }
    assert_eq!(fx.seen(), Vec::<String>::new(), "none of them ran");
    // The same connection still serves a good request.
    assert_eq!(c.request(&json!({"id": 4, "method": "echo", "params": {"ok": true}}))["result"], json!({"ok": true}));
    assert_eq!(fx.seen(), ["echo"]);
}

#[test]
fn oversized_line_is_refused() {
    let fx = start_default();
    // One byte over the limit, then a valid request: neither runs.
    let mut c = fx.client();
    let mut input = vec![b'x'; MAX_LINE + 1];
    input.extend_from_slice(b"\n{\"id\":1,\"method\":\"echo\"}\n");
    c.send_what_fits(&input);
    let error = expect_refused(&mut c, "longer than");
    assert!(error.contains(&MAX_LINE.to_string()), "{error}");
    assert_eq!(fx.seen(), Vec::<String>::new());
}

#[test]
fn a_line_of_exactly_the_limit_is_accepted() {
    let fx = start_default();
    let mut c = fx.client();
    // A valid request padded with spaces to exactly MAX_LINE bytes before the newline.
    let head = b"{\"id\":9,\"method\":\"echo\",\"params\":{\"n\":1}}";
    let mut line = head.to_vec();
    line.resize(MAX_LINE, b' ');
    line.push(b'\n');
    c.send(std::str::from_utf8(&line).unwrap());
    assert_eq!(c.reply(), json!({"id": 9, "ok": true, "result": {"n": 1}}));
    // One more byte and it is refused.
    let mut c = fx.client();
    let mut line = head.to_vec();
    line.resize(MAX_LINE + 1, b' ');
    line.push(b'\n');
    c.send_what_fits(&line);
    expect_refused(&mut c, "longer than");
    assert_eq!(fx.seen(), ["echo"]);
}

/// serde_json's recursion limit (128 levels) is the bound on how deep a request may nest: a client
/// cannot make the server recurse a thousand levels. The answer is one clear error, at once.
#[test]
fn a_deeply_nested_value_gets_one_clear_error() {
    let fx = start_default();
    let depth = 1_000;
    let arrays = format!("{}{}", "[".repeat(depth), "]".repeat(depth));
    let objects = format!("{}1{}", "{\"a\":".repeat(depth), "}".repeat(depth));
    let in_params = format!("{{\"id\":1,\"method\":\"echo\",\"params\":{arrays}}}");
    let unclosed = "[".repeat(100_000);
    for (what, text) in [("arrays", arrays), ("objects", objects), ("inside params", in_params), ("never closed", unclosed)] {
        let started = Instant::now();
        let mut c = fx.client();
        c.send(&format!("{text}\n{{\"id\":2,\"method\":\"echo\"}}\n"));
        let error = expect_refused(&mut c, "recursion limit");
        // A hang guard and no measure of cost: the refusal is one short error (below), and a server that recursed or read on would not answer.
        assert!(started.elapsed() < Duration::from_secs(30), "{what}: took {:?}", started.elapsed());
        assert!(error.len() < 300, "{what}: one short error, not an echo of the input: {} bytes", error.len());
    }
    assert_eq!(fx.seen(), Vec::<String>::new(), "nothing nested too deep reached the window");
}

/// The limit is not stricter than it must be: a request nested well under 128 levels runs.
#[test]
fn nesting_under_the_limit_is_accepted() {
    let fx = start_default();
    let mut c = fx.client();
    let depth = 100;
    let nested: Value = serde_json::from_str(&format!("{}1{}", "[".repeat(depth), "]".repeat(depth))).unwrap();
    let reply = c.request(&json!({"id": 1, "method": "echo", "params": nested}));
    assert_eq!(reply["ok"], true, "{reply}");
    assert_eq!(reply["result"], nested);
}

#[test]
fn seventeenth_connection_is_refused() {
    assert_eq!(MAX_CONNECTIONS, 16);
    let fx = start_default();
    // Sixteen connections, each served: the limit is not lower than it says.
    let mut held: Vec<Client> = (0..MAX_CONNECTIONS).map(|_| fx.client()).collect();
    for (n, c) in held.iter_mut().enumerate() {
        let reply = c.request(&json!({"id": n, "method": "echo", "params": {"n": n}}));
        assert_eq!(reply, json!({"id": n, "ok": true, "result": {"n": n}}), "connection {n} was not served");
    }
    // The seventeenth gets one error line, then end of stream. (It sends nothing: a server that closes on
    // unread bytes would reset the connection, and a client could lose the line.)
    let mut extra = fx.client();
    let reply = extra.reply();
    assert_eq!(reply["ok"], false);
    assert!(reply["error"].as_str().unwrap().contains("too many control connections"), "{reply}");
    extra.expect_closed();
    assert_eq!(fx.seen().len(), MAX_CONNECTIONS, "only the sixteen served connections made requests");
    // Closing one frees its place for the next client: a new connection gets no refusal, and is served.
    held.pop();
    let deadline = Instant::now() + WAIT;
    loop {
        let mut next = fx.client();
        next.stream.set_read_timeout(Some(Duration::from_millis(300))).unwrap();
        let mut line = String::new();
        match next.reader.read_line(&mut line) {
            // A refusal arrives at once, so a line or an end of stream here is "still full".
            Ok(_) => {
                assert!(Instant::now() < deadline, "the freed slot was never given to a new connection: {line:?}");
                std::thread::sleep(Duration::from_millis(20));
            }
            // Nothing said: the connection is being served and waits for its first request.
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {
                next.stream.set_read_timeout(Some(WAIT)).unwrap();
                assert_eq!(next.request(&json!({"id": 100, "method": "echo"}))["ok"], true);
                break;
            }
            Err(e) => panic!("{e}"),
        }
    }
    // And the other fifteen are untouched.
    for c in &mut held {
        assert_eq!(c.request(&json!({"id": 5, "method": "echo"}))["ok"], true);
    }
}

#[test]
fn port_in_use_returns_a_clear_error() {
    // Another app holds the port: a listener, as a sibling craft app's control server would be.
    let other = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = other.local_addr().unwrap().port();
    let error = Server::bind(port).err().expect("binding a port someone listens on must fail");
    assert!(matches!(error, ControlError::PortInUse { port: p } if p == port), "{error:?}");
    let text = error.to_string();
    assert!(text.contains(&port.to_string()) && text.contains("already in use"), "{text}");
    assert!(text.contains("--control") && text.contains("another"), "it says what to do: {text}");
    // The other app is not disturbed: it still accepts a connection.
    TcpStream::connect(other.local_addr().unwrap()).unwrap();
    other.accept().unwrap();

    // A listener on every address holds the port against the loopback too, on Linux. macOS lets a socket that asks to reuse an address (as
    // Rust's listeners do) bind the loopback beside a listener on every address, and gives the more particular one the loopback's connections:
    // there the channel starts (measured on macOS 26.7).
    let wildcard = TcpListener::bind(("0.0.0.0", 0)).unwrap();
    let port = wildcard.local_addr().unwrap().port();
    if cfg!(target_os = "macos") {
        assert!(Server::bind(port).is_ok());
    } else {
        assert!(matches!(Server::bind(port).err(), Some(ControlError::PortInUse { .. })));
    }
}

/// With `--control` on a port someone else holds, the app starts anyway: no channel, and a notice that
/// says why, for the window's status bar. It never panics and never exits.
#[test]
fn a_busy_port_keeps_the_app_running_without_a_channel_and_says_so() {
    let other = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = other.local_addr().unwrap().port();
    let started = start(port, egui::Context::default());
    assert!(started.inbox.is_none(), "no channel on a port that is not ours");
    let notice = started.notice.expect("the busy port is reported");
    assert!(notice.contains(&port.to_string()) && notice.contains("already in use") && notice.contains("off"), "{notice}");
    assert!(!notice.chars().any(char::is_control));

    // A free port starts the channel, with no notice.
    let free = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = free.local_addr().unwrap().port();
    drop(free);
    let started = start(port, egui::Context::default());
    assert!(started.inbox.is_some() && started.notice.is_none(), "{:?}", started.notice);
    TcpStream::connect(("127.0.0.1", port)).unwrap();
}

#[test]
fn the_server_listens_on_the_loopback_only() {
    let server = Server::bind(0).unwrap();
    assert_eq!(server.addr().ip(), std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST), "bound to 127.0.0.1, not a public address");
    assert_ne!(server.addr().port(), 0, "the port is the one the system gave");
    assert_eq!(server.port(), server.addr().port());
}

// ---- time budgets ----

fn limits(idle: u64, line: u64, write: u64, reply: u64) -> Limits {
    Limits {
        idle: Duration::from_millis(idle),
        line: Duration::from_millis(line),
        write: Duration::from_millis(write),
        reply: Duration::from_millis(reply),
    }
}

#[test]
fn the_default_budgets_are_the_documented_ones() {
    let d = Limits::default();
    assert_eq!(
        (d.idle, d.line, d.write, d.reply),
        (Duration::from_secs(300), Duration::from_secs(30), Duration::from_secs(60), Duration::from_secs(60))
    );
}

/// An idle connection is closed without a line: a client that sends its next request late must not find
/// an error waiting where its reply should be.
#[test]
fn an_idle_connection_is_closed_without_a_word() {
    let fx = start_with(limits(300, 30_000, 30_000, 30_000));
    let mut c = fx.client();
    let started = Instant::now();
    c.expect_closed();
    assert!(started.elapsed() >= Duration::from_millis(250), "closed after {:?}, before its 300 ms", started.elapsed());
    assert!(started.elapsed() < WAIT);
}

#[test]
fn a_stalled_line_is_refused_with_the_reason() {
    let fx = start_with(limits(30_000, 300, 30_000, 30_000));
    let mut c = fx.client();
    c.send("{\"method\":");
    expect_refused(&mut c, "not completed in time");
    assert_eq!(fx.seen(), Vec::<String>::new());
}

/// Bytes that trickle in do not renew the line's deadline.
#[test]
fn a_trickled_line_has_an_absolute_deadline() {
    let fx = start_with(limits(30_000, 400, 30_000, 30_000));
    let mut c = fx.client();
    c.send("{");
    let mut writer = c.stream.try_clone().unwrap();
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let running = Arc::clone(&stop);
    let trickle = std::thread::spawn(move || {
        while !running.load(std::sync::atomic::Ordering::SeqCst) {
            if writer.write_all(b" ").is_err() {
                break;
            }
            std::thread::sleep(Duration::from_millis(40));
        }
    });
    let started = Instant::now();
    let reply = c.reply();
    stop.store(true, std::sync::atomic::Ordering::SeqCst);
    trickle.join().unwrap();
    assert!(reply["error"].as_str().unwrap().contains("not completed in time"), "{reply}");
    // The deadline is 400 ms from the first byte; a hang guard for a server that renewed it with each byte of the trickle, which would not end.
    assert!(started.elapsed() < Duration::from_secs(30), "{:?}", started.elapsed());
}

/// A peer that sends a request and never reads the answer cannot keep the writer: the reply is cut off
/// when its time is up, and the connection is closed.
#[test]
fn a_peer_that_does_not_read_cannot_hold_the_writer() {
    let fx = start_with(limits(30_000, 30_000, 400, 30_000));
    let mut c = fx.client();
    c.send("{\"id\":1,\"method\":\"big\"}\n");
    std::thread::sleep(Duration::from_millis(1500));
    // Whatever the buffers held arrives, then the end: the 48 MiB reply does not.
    let mut got = 0_usize;
    let mut buf = vec![0_u8; 1 << 20];
    loop {
        match c.reader.get_mut().read(&mut buf) {
            Ok(0) => break,
            Ok(n) => got += n,
            // A reset is also the server having closed it.
            Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset => break,
            Err(e) => panic!("{e}"),
        }
    }
    assert!(got < 48 * 1024 * 1024, "the whole reply was written to a peer that was not reading ({got} bytes)");
}

/// A request the window did not start in time is withdrawn, so it never runs once its caller was told it
/// failed, and the error says so.
#[test]
fn a_request_the_window_is_slow_to_take_is_withdrawn() {
    let fx = start_with(limits(30_000, 30_000, 30_000, 300));
    let mut c = fx.client();
    let reply = c.request(&json!({"id": 1, "method": "sleepy"}));
    assert_eq!(reply["ok"], false);
    let error = reply["error"].as_str().unwrap();
    assert!(error.starts_with("timeout") && error.contains("will not run"), "{error}");
    // The window gets to it later, and finds it withdrawn.
    let deadline = Instant::now() + WAIT;
    while !fx.seen().contains(&"withdrawn sleepy".to_owned()) {
        assert!(Instant::now() < deadline, "the window never saw the withdrawn request: {:?}", fx.seen());
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!fx.seen().contains(&"sleepy".to_owned()), "a withdrawn request ran");
    // The connection is still good.
    assert_eq!(c.request(&json!({"id": 2, "method": "echo"}))["ok"], true);
}

/// A request the window took but has not answered may yet complete: the error says to check before retrying.
#[test]
fn a_request_the_window_is_still_running_says_so() {
    let fx = start_with(limits(30_000, 30_000, 30_000, 300));
    let mut c = fx.client();
    let reply = c.request(&json!({"id": 1, "method": "hold"}));
    assert_eq!(reply["ok"], false);
    let error = reply["error"].as_str().unwrap();
    assert!(error.starts_with("timeout") && error.contains("still running") && error.contains("may yet complete"), "{error}");
    assert_eq!(fx.seen(), ["hold"]);
}

/// A window that goes away before it answers is not the same as a timeout.
#[test]
fn a_window_that_closes_before_answering_is_told_apart_from_a_timeout() {
    let server = Server::bind(0).unwrap().with_limits(limits(30_000, 30_000, 30_000, 30_000));
    let addr = SocketAddr::from(([127, 0, 0, 1], server.port()));
    let inbox = server.spawn(egui::Context::default()).unwrap();
    std::thread::spawn(move || {
        // Takes the request and drops it unanswered, as a window closing does.
        if let Ok(request) = inbox.recv() {
            request.claim();
        }
    });
    let mut c = Client::connect(addr);
    let reply = c.request(&json!({"id": 1, "method": "echo"}));
    assert_eq!(reply["ok"], false);
    let error = reply["error"].as_str().unwrap();
    assert!(!error.contains("timeout") && error.contains("closed"), "{error}");
}

// ---------- a panic in a thread of the server ----------

/// Waits for a record that holds `marker` and `also`, which is written a moment after the connection is closed.
fn wait_for_record(marker: &str, also: &str) -> String {
    let until = Instant::now() + WAIT;
    loop {
        let found = crate::logged().into_iter().find(|r| r.contains(marker) && r.contains(also));
        match found {
            Some(record) => return record,
            None if Instant::now() > until => panic!("no log record with {marker:?} and {also:?}: {:?}", crate::logged()),
            None => std::thread::sleep(Duration::from_millis(10)),
        }
    }
}

/// A bug in the thread that serves a connection (here a repaint callback that panics, which the thread calls after it has
/// queued a request) ends that connection, says why in the log with the reason escaped, and leaves the server taking others.
/// Without the guard the thread ended with nothing in the app's log, whose file is all a person who launched the app from a
/// menu can attach.
#[test]
fn a_panic_in_a_connection_thread_is_logged_and_closes_only_that_connection() {
    crate::capture_logs();
    let armed = Arc::new(AtomicBool::new(true));
    let flag = Arc::clone(&armed);
    let ctx = egui::Context::default();
    ctx.set_request_repaint_callback(move |_| {
        if flag.load(Ordering::SeqCst) && std::thread::current().name() == Some("diacraft-control") {
            panic!("connection boom \u{1b}[2J \n marker-4c2");
        }
    });
    let server = Server::bind(0).unwrap();
    let addr = SocketAddr::from(([127, 0, 0, 1], server.port()));
    let inbox = server.spawn(ctx).unwrap();
    std::thread::spawn(move || {
        for request in inbox {
            if request.claim() {
                request.reply.send(Ok(request.params.clone())).ok();
            }
        }
    });

    let mut victim = Client::connect(addr);
    victim.send(&format!("{}\n", json!({"id": 1, "method": "echo", "params": {"a": 1}})));
    victim.expect_closed();
    let record = wait_for_record("marker-4c2", "ended with a panic");
    assert!(record.starts_with("ERROR ") && record.contains("connection boom \\u{1b}[2J \\n marker-4c2"), "{record:?}");
    assert!(!record.chars().any(char::is_control), "{record:?}");

    // The next connection is served as before, and the place the first one held is free again.
    armed.store(false, Ordering::SeqCst);
    for _ in 0..MAX_CONNECTIONS + 2 {
        let mut client = Client::connect(addr);
        let reply = client.request(&json!({"id": 7, "method": "echo", "params": {"b": 2}}));
        assert_eq!(reply, json!({"id": 7, "ok": true, "result": {"b": 2}}));
    }
}
