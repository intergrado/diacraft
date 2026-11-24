//! Where a tool call ends up: this process's own session, or a running DiaCraft app.
//!
//! Both answer the control channel's method names (`docs/control-protocol.md`), so a tool is written once:
//!
//! - [`Backend::Headless`] holds a [`Session`], boxed (it is far larger than the control client, and clippy's `large_enum_variant` refuses an enum
//!   that holds it inline). It answers `engine.execute`, `engine.commands` and `document.inspect` itself. The `ui.*` methods need a window, and it has none.
//! - [`Backend::Remote`] holds a [`ControlClient`], one connection to the app's loopback port.
//!
//! The one rule that matters about the app is who it is: another craft app may be listening on the port the
//! documentation names, and a command meant for DiaCraft must never run in it. [`ControlClient`] asks `ui.inspect`
//! before it sends anything else, on every connection it opens, and speaks only to a program that answers
//! `{"app": "diacraft"}`.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

use diacraft_engine::{Session, shown};
use serde_json::{Map, Value, json};

/// Where the app's control channel is, by the documentation: the loopback, port 7997.
pub const DEFAULT_ADDR: &str = "127.0.0.1:7997";

/// The longest request line the app reads (4 MiB, `docs/control-protocol.md`). A longer one is refused by the app,
/// which then closes the connection, so it is not sent.
const MAX_REQUEST_BYTES: usize = 4 * 1024 * 1024;

/// The most characters of a name or an address a message repeats.
const MOST_NAME: usize = 64;

/// The most characters of the app's own error text a message repeats.
const MOST_ERROR: usize = 2000;

/// The most addresses one name is tried at.
const MOST_ADDRESSES: usize = 8;

/// What a tool call is run on.
pub enum Backend {
    /// This process's own session, with no window.
    Headless(Box<Session>),
    /// A running DiaCraft app.
    Remote(ControlClient),
}

impl Backend {
    /// A headless backend with no document open.
    pub fn headless() -> Backend {
        Backend::Headless(Box::new(Session::new()))
    }

    /// Whether a window is attached, so that `ui.*` methods work.
    pub fn has_app(&self) -> bool {
        matches!(self, Backend::Remote(_))
    }

    /// A short description for the person and for the client: `headless`, or where the app is.
    pub fn describe(&self) -> String {
        match self {
            Backend::Headless(_) => "headless (no window)".to_owned(),
            Backend::Remote(client) => format!("the DiaCraft app at {}", shown(client.addr(), MOST_NAME)),
        }
    }

    /// Calls a control-channel method: `Ok` is its `result`, `Err` the reason it did not work.
    pub fn call(&mut self, method: &str, params: &Value) -> Result<Value, String> {
        match self {
            Backend::Remote(client) => client.call(method, params),
            Backend::Headless(session) => headless_call(session, method, params),
        }
    }
}

/// What a headless backend does with a method. Like the app, it takes a fixed set of keys for each. The params are only
/// borrowed, never copied: they may be as big as a line.
fn headless_call(session: &mut Session, method: &str, params: &Value) -> Result<Value, String> {
    let nothing = Map::new();
    let keys = match params {
        Value::Null => &nothing,
        Value::Object(map) => map,
        _ => return Err(format!("`{}`: params must be an object", shown(method, MOST_NAME))),
    };
    let only = |allowed: &[&str]| match keys.keys().find(|k| !allowed.contains(&k.as_str())) {
        None => Ok(()),
        Some(key) => Err(format!(
            "unknown key `{}`; `{method}` takes: {}",
            shown(key, MOST_NAME),
            if allowed.is_empty() { "nothing".to_owned() } else { allowed.join(", ") }
        )),
    };
    match method {
        "engine.execute" => {
            only(&["command", "params"])?;
            let Some(Value::String(command)) = keys.get("command") else {
                return Err("`engine.execute` needs a text `command`: the id of the command to run".to_owned());
            };
            let no_params = Value::Object(Map::new());
            let command_params = match keys.get("params") {
                None | Some(Value::Null) => &no_params,
                Some(object @ Value::Object(_)) => object,
                Some(_) => return Err("`engine.execute`: `params` must be an object".to_owned()),
            };
            session.execute(command, command_params).map_err(|e| e.to_string())
        }
        "engine.commands" => {
            only(&[])?;
            serde_json::to_value(session.commands()).map_err(|e| format!("cannot encode the command list: {e}"))
        }
        "document.inspect" => session.execute("document.inspect", params).map_err(|e| e.to_string()),
        m if m.starts_with("ui.") => Err(format!("`{}` needs the app: a headless session has no window", shown(m, MOST_NAME))),
        other => Err(format!(
            "unknown method `{}`; a headless session answers engine.execute, engine.commands and document.inspect",
            shown(other, MOST_NAME)
        )),
    }
}

/// The app's address if it is DiaCraft and answers; a headless session, with the reason, if not. This is the
/// default of `diacraft-cli mcp`: it asks once, and never drives anything else.
pub fn connect_or_headless(addr: &str) -> (Backend, Option<ConnectError>) {
    match ControlClient::connect(addr) {
        Ok(client) => (Backend::Remote(client), None),
        Err(why) => (Backend::headless(), Some(why)),
    }
}

/// Why there is no connection to DiaCraft.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConnectError {
    /// Nothing answered the way a control channel does: nothing listens there, or it did not answer in time or in
    /// JSON lines.
    #[error("cannot reach a DiaCraft control channel: {0}")]
    Refused(String),
    /// Something answered, and it is not DiaCraft. `app` is what it says it is (`unknown` when it says nothing
    /// usable), escaped. It was sent the question who it is and nothing else.
    #[error("not DiaCraft: the program that answers there says it is `{app}`; nothing was sent to it but the question who it is")]
    NotDiacraft { app: String },
}

/// The limits of a connection to the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientConfig {
    /// How long a connection may take to open, for each address a name stands for.
    pub connect: Duration,
    /// How long the app has to say who it is, once connected.
    pub probe: Duration,
    /// How long a request may take in all: sending it (a peer that takes its bytes slowly is waited for only this long, not
    /// for each write) and then waiting for the reply's newline. The app gives up on a request after 60 seconds of its own.
    pub reply: Duration,
    /// The longest reply line read, in bytes. Longer, and the connection is dropped.
    pub max_reply: usize,
}

impl Default for ClientConfig {
    /// 0.8 seconds to connect, 5 to say who it is, 90 for a reply, and 16 MiB of reply.
    fn default() -> ClientConfig {
        ClientConfig {
            connect: Duration::from_millis(800),
            probe: Duration::from_secs(5),
            reply: Duration::from_secs(90),
            max_reply: 16 * 1024 * 1024,
        }
    }
}

/// A reader that stops waiting at a fixed time, however slowly bytes come: each read waits for what is left of the
/// budget, so a peer that sends a byte a second cannot hold the caller for ever.
struct Deadline {
    stream: TcpStream,
    until: Instant,
}

impl Read for Deadline {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let left = self
            .until
            .checked_duration_since(Instant::now())
            .filter(|left| !left.is_zero())
            .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "no answer in time"))?;
        self.stream.set_read_timeout(Some(left))?;
        match self.stream.read(buf) {
            // Unix reports the end of the read timeout as `WouldBlock`, Windows as `TimedOut`.
            Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {
                Err(io::Error::new(io::ErrorKind::TimedOut, "no answer in time"))
            }
            other => other,
        }
    }
}

/// A time `within` from now. A span too long to add is a day.
fn after(within: Duration) -> Instant {
    let now = Instant::now();
    now.checked_add(within).or_else(|| now.checked_add(Duration::from_secs(86_400))).unwrap_or(now)
}

/// Writes all of `bytes` to `stream` by `until`. The write timeout is set again before each write to what is left of the
/// budget: a timeout set once is for each write, and a peer that takes a few bytes at a time makes every write succeed, so
/// sending a request of megabytes could go on for as long as the peer likes.
fn write_by(stream: &mut TcpStream, bytes: &[u8], until: Instant) -> io::Result<()> {
    let too_slow = || io::Error::new(io::ErrorKind::TimedOut, "the app did not take the request in time");
    let mut sent = 0;
    while let Some(rest) = bytes.get(sent..).filter(|rest| !rest.is_empty()) {
        let left = until.checked_duration_since(Instant::now()).filter(|left| !left.is_zero()).ok_or_else(too_slow)?;
        stream.set_write_timeout(Some(left))?;
        match stream.write(rest) {
            Ok(0) => return Err(io::Error::new(io::ErrorKind::WriteZero, "the connection was closed")),
            Ok(n) => sent += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            // Unix reports the end of a write timeout as `WouldBlock`, Windows as `TimedOut`.
            Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => return Err(too_slow()),
            Err(e) => return Err(e),
        }
    }
    stream.flush()
}

/// One open connection to something that may be the app.
struct Conn {
    reader: BufReader<Deadline>,
    writer: TcpStream,
}

impl Conn {
    /// Sends `request`, a whole line with its newline, and reads the reply line, all within `within`: one budget for sending
    /// and waiting together. The reply is at most `max` bytes.
    fn roundtrip(&mut self, request: &[u8], within: Duration, max: usize) -> io::Result<Value> {
        let until = after(within);
        write_by(&mut self.writer, request, until)?;
        self.reader.get_mut().until = until;
        let mut line = Vec::new();
        // One byte more than the limit is read, to tell a line at the limit from a longer one.
        let limit = u64::try_from(max).unwrap_or(u64::MAX).saturating_add(1);
        let n = (&mut self.reader).take(limit).read_until(b'\n', &mut line)?;
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "the connection was closed"));
        }
        if line.last() != Some(&b'\n') {
            return Err(if line.len() > max {
                io::Error::new(io::ErrorKind::InvalidData, format!("a reply line longer than {max} bytes"))
            } else {
                io::Error::new(io::ErrorKind::UnexpectedEof, "the connection was closed in the middle of a reply")
            });
        }
        line.pop();
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        // The app answers each request with one line. More is not the answer to anything asked.
        if !self.reader.buffer().is_empty() {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "data after the reply that nobody asked for"));
        }
        serde_json::from_slice(&line)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("a reply that is not JSON ({})", shown(&e.to_string(), 200))))
    }

    /// Whether the connection is still open and quiet: the app closes one that has been idle for five minutes,
    /// without a word, and a connection that has bytes waiting that nobody asked for cannot be read from.
    fn is_usable(&self) -> bool {
        if !self.reader.buffer().is_empty() {
            return false;
        }
        let stream = &self.reader.get_ref().stream;
        if stream.set_nonblocking(true).is_err() {
            return false;
        }
        let state = stream.peek(&mut [0_u8; 1]);
        let restored = stream.set_nonblocking(false).is_ok();
        // No bytes and not closed is `WouldBlock`; `Ok(0)` is a closed connection and `Ok(n)` is unasked-for data.
        restored && matches!(state, Err(ref e) if e.kind() == io::ErrorKind::WouldBlock)
    }
}

/// Whether `reply` is the app turning a line away before it read it: it says it did not work and names no request.
fn turned_away(reply: &Value) -> bool {
    reply.get("id").is_none_or(Value::is_null) && reply.get("ok") == Some(&Value::Bool(false))
}

/// What the app said went wrong in `reply`, escaped and cut short like any other text from it.
fn error_text(reply: &Value) -> String {
    shown(reply.get("error").and_then(Value::as_str).unwrap_or("unknown error"), MOST_ERROR)
}

/// Opens a connection to `addr` and checks that DiaCraft is on the other end.
fn open(addr: &str, config: &ClientConfig) -> Result<Conn, ConnectError> {
    let refused = |why: String| ConnectError::Refused(format!("{why} (at {})", shown(addr, MOST_NAME)));
    let candidates = addr.to_socket_addrs().map_err(|e| refused(format!("the address does not resolve: {e}")))?;
    let mut last = "the address names no host".to_owned();
    let mut stream = None;
    for candidate in candidates.take(MOST_ADDRESSES) {
        match TcpStream::connect_timeout(&candidate, config.connect) {
            Ok(s) => {
                stream = Some(s);
                break;
            }
            Err(e) => last = e.to_string(),
        }
    }
    let stream = stream.ok_or_else(|| refused(last))?;
    // A request is one small line: send it at once. A stream that cannot be tuned still works.
    let _ = stream.set_nodelay(true);
    let writer = stream.try_clone().map_err(|e| refused(e.to_string()))?;
    let mut conn = Conn { reader: BufReader::new(Deadline { stream, until: Instant::now() }), writer };
    let question = format!("{}\n", json!({"id": 1, "method": "ui.inspect", "params": {}}));
    let reply =
        conn.roundtrip(question.as_bytes(), config.probe, config.max_reply).map_err(|e| refused(format!("it did not answer who it is: {e}")))?;
    // An app that is full, or that will not read the question, turns it away with a reply that names no request. What it said
    // is the reason to give, as it is for a call, and it is looked for before the id, which such a reply does not have.
    if turned_away(&reply) {
        return Err(refused(format!("the app refused the question who it is: {}", error_text(&reply))));
    }
    if reply.get("id") != Some(&json!(1)) {
        return Err(refused("its answer to who it is names another request".to_owned()));
    }
    let named = reply.get("result").and_then(|r| r.get("app")).and_then(Value::as_str);
    match (reply.get("ok"), named) {
        (Some(Value::Bool(true)), Some("diacraft")) => Ok(conn),
        (Some(Value::Bool(true)), Some(other)) => Err(ConnectError::NotDiacraft { app: shown(other, MOST_NAME) }),
        _ => Err(ConnectError::NotDiacraft { app: "unknown".to_owned() }),
    }
}

/// A connection to a running DiaCraft app's control channel (`docs/control-protocol.md`).
///
/// The app closes a connection that has been idle for five minutes, so a call first looks whether its connection is
/// still open and, if not, opens another and checks again who answers. Calls are one at a time: a request is sent,
/// and its answer read, before the next.
pub struct ControlClient {
    addr: String,
    config: ClientConfig,
    conn: Option<Conn>,
    next_id: u64,
}

impl ControlClient {
    /// Connects to `addr` (`127.0.0.1:7997`) with the default limits.
    pub fn connect(addr: &str) -> Result<ControlClient, ConnectError> {
        ControlClient::connect_with(addr, ClientConfig::default())
    }

    /// Connects to `addr` with these limits. Fails with [`ConnectError::NotDiacraft`] when another program answers.
    pub fn connect_with(addr: &str, config: ClientConfig) -> Result<ControlClient, ConnectError> {
        let conn = open(addr, &config)?;
        // Request 1 was the question who answers.
        Ok(ControlClient { addr: addr.to_owned(), config, conn: Some(conn), next_id: 2 })
    }

    /// The address it was given.
    pub fn addr(&self) -> &str {
        &self.addr
    }

    /// Calls a method of the control channel: `Ok` is its `result`, `Err` why it did not work. An error says whether
    /// the request was sent: `nothing was sent` and `nothing was run` can be tried again as they are; `may have
    /// completed` cannot, before the document has been looked at.
    pub fn call(&mut self, method: &str, params: &Value) -> Result<Value, String> {
        let (name, addr) = (shown(method, MOST_NAME), shown(&self.addr, MOST_NAME));
        let mut request =
            encode_request(self.next_id, method, params).map_err(|e| format!("`{name}`: the request cannot be encoded: {e}; nothing was sent"))?;
        if request.len() > MAX_REQUEST_BYTES {
            return Err(format!("`{name}`: the request is {} bytes, and the app reads at most {MAX_REQUEST_BYTES}; nothing was sent", request.len()));
        }
        request.push(b'\n');
        self.ensure_open().map_err(|why| match why {
            ConnectError::NotDiacraft { app } => {
                format!("the program at {addr} is no longer DiaCraft (it says it is `{app}`); `{name}`: nothing was sent")
            }
            // The reason already says where (`... (at 127.0.0.1:7997)`).
            ConnectError::Refused(why) => format!("DiaCraft is not reachable: {why}; `{name}`: nothing was sent"),
        })?;
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        let (reply_within, max_reply) = (self.config.reply, self.config.max_reply);
        let Some(conn) = self.conn.as_mut() else {
            return Err(format!("DiaCraft at {addr}: there is no connection (please report this bug); `{name}`: nothing was sent"));
        };
        let reply = match conn.roundtrip(&request, reply_within, max_reply) {
            Ok(reply) => reply,
            Err(e) => {
                self.conn = None;
                return Err(format!(
                    "DiaCraft at {addr}: `{name}` failed: {e}; the operation may have completed, so look at the document before trying again"
                ));
            }
        };
        self.read_reply(&addr, &name, id, reply)
    }

    /// The call's outcome, from the reply that came for request `id`. The result is moved out of the reply, not copied: it
    /// may be megabytes.
    fn read_reply(&mut self, addr: &str, name: &str, id: u64, mut reply: Value) -> Result<Value, String> {
        // A reply that names no request and says it did not work is the app turning the line away unread, because it is
        // full or the line is over its limit. Nothing ran. The app hangs up after it, so the next call opens a new connection.
        if turned_away(&reply) {
            self.conn = None;
            return Err(format!("DiaCraft at {addr} refused `{name}`: {}; nothing was run", error_text(&reply)));
        }
        if reply.get("id") != Some(&json!(id)) {
            self.conn = None;
            return Err(format!(
                "DiaCraft at {addr}: the reply to `{name}` is for another request (expected id {id}); the connection was dropped; the operation may have completed, so look at the document before trying again"
            ));
        }
        match reply.get("ok").and_then(Value::as_bool) {
            Some(true) => Ok(reply.get_mut("result").map_or(Value::Null, Value::take)),
            Some(false) => Err(error_text(&reply)),
            None => {
                self.conn = None;
                Err(format!(
                    "DiaCraft at {addr}: the reply to `{name}` says neither ok nor not ok; the connection was dropped; the operation may have completed"
                ))
            }
        }
    }

    /// Makes sure there is a usable connection, opening another (and asking again who answers) if the last is gone.
    fn ensure_open(&mut self) -> Result<(), ConnectError> {
        if self.conn.as_ref().is_some_and(Conn::is_usable) {
            return Ok(());
        }
        self.conn = None;
        self.conn = Some(open(&self.addr, &self.config)?);
        // Said out loud, because a quiet reconnect would hide a restart: the app that answers may be a new one with another
        // document, where the page a command names by its id is not the page the earlier commands meant.
        log::warn!(
            "the connection to the DiaCraft app at {} was gone and has been opened again; if the app was restarted its document is a different one, so look at it (doc_inspect) before using ids from before",
            shown(&self.addr, MOST_NAME)
        );
        Ok(())
    }
}

/// The request line for `method`, without its newline: `{"id", "method", "params"}`. The params are written from the
/// reference, not copied into a new value first: they may be megabytes.
fn encode_request(id: u64, method: &str, params: &Value) -> serde_json::Result<Vec<u8>> {
    let mut request = Vec::new();
    request.extend_from_slice(b"{\"id\":");
    serde_json::to_writer(&mut request, &id)?;
    request.extend_from_slice(b",\"method\":");
    serde_json::to_writer(&mut request, method)?;
    request.extend_from_slice(b",\"params\":");
    serde_json::to_writer(&mut request, params)?;
    request.push(b'}');
    Ok(request)
}
