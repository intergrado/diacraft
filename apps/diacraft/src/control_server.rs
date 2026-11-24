//! The control server: a loopback JSON-lines transport between a client and the window.
//!
//! One request a line, one answer a line: `{"id":1,"method":"ui.inspect","params":{}}` is answered by
//! `{"id":1,"ok":true,"result":…}` or `{"id":1,"ok":false,"error":"…"}`. The server reads requests and
//! nothing else. A line that is not a JSON object with a string `method` (an HTTP request line from a web
//! page's cross-origin `fetch`, a stray `nc`, binary junk) gets one error reply and the connection is
//! closed, so nothing after it runs. Lines are bounded ([`MAX_LINE`]), so are the open connections
//! ([`MAX_CONNECTIONS`]) and every wait ([`Limits`]), and `serde_json`'s recursion limit bounds how deep a
//! value may nest: junk never reaches the window. See `docs/control-protocol.md`.
//!
//! The window is the other end of a channel: each request becomes a [`ControlRequest`] on the inbox
//! [`Server::spawn`] returns, and the server waits for its answer.

use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{RecvTimeoutError, Sender, channel};
use std::time::Duration;

use diacraft_engine::{guard, shown};
use diacraft_ui_egui::{ControlInbox, ControlReply, ControlRequest, Ticket};
use serde_json::{Value, json};

mod deadlines;
use deadlines::{DeadlineReader, DeadlineWriter};

#[cfg(test)]
mod tests;

/// Longest accepted request line, in bytes without the newline. Requests are small JSON objects; a longer
/// line is refused and the connection closed.
pub const MAX_LINE: usize = 4 * 1024 * 1024;

/// Connections served at once. Further ones get an error line and are closed.
pub const MAX_CONNECTIONS: usize = 16;

/// Size of the buffer a reply line is written through.
const REPLY_CHUNK: usize = 64 * 1024;

/// How long, and how much, the server reads from a peer it has just refused, so that the refusal arrives
/// and not a connection reset.
const LINGER: Duration = Duration::from_millis(250);
const LINGER_BYTES: u64 = 8 * 1024 * 1024;

/// The most characters of a client's key an error echoes.
const MOST_SHOWN: usize = 64;

/// The most characters of a panic's message a log line holds.
const MOST_REASON: usize = 400;

/// The keys a request has.
const REQUEST_KEYS: [&str; 3] = ["id", "method", "params"];

/// The time limits of a connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// How long a client may be silent between requests. The connection then closes without a word.
    pub idle: Duration,
    /// How long a line may take once its first byte has arrived, however slowly the bytes come.
    pub line: Duration,
    /// How long writing one reply may take.
    pub write: Duration,
    /// How long a request waits for the window's answer.
    pub reply: Duration,
}

impl Default for Limits {
    /// 5 minutes idle, 30 seconds a line, 60 seconds a reply written, 60 seconds for the window to answer.
    fn default() -> Limits {
        Limits { idle: Duration::from_secs(300), line: Duration::from_secs(30), write: Duration::from_secs(60), reply: Duration::from_secs(60) }
    }
}

/// Why the server did not start.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ControlError {
    /// Something else is listening on the port.
    #[error(
        "control port {port} is already in use: another program, perhaps another craft app (they default to port 7979), is listening on it; give --control another port"
    )]
    PortInUse { port: u16 },
    /// The port could not be bound for another reason.
    #[error("the control server cannot listen on 127.0.0.1:{port}: {reason}")]
    Bind { port: u16, reason: String },
    /// No thread could be started for it.
    #[error("the control server cannot start a thread: {0}")]
    Thread(String),
}

impl ControlError {
    /// What the window's status bar says: that the channel is off, and why.
    pub fn notice(&self) -> String {
        match self {
            ControlError::PortInUse { port } => format!(
                "The control channel is off: port {port} is already in use. Another app, perhaps another craft app, may be listening there; start DiaCraft with --control and another port."
            ),
            other => format!("The control channel is off: {other}."),
        }
    }
}

/// The control server, bound to a port on the loopback and not yet serving.
pub struct Server {
    listener: TcpListener,
    addr: SocketAddr,
    limits: Limits,
}

impl Server {
    /// Binds `127.0.0.1:port` (`0` lets the system pick one). The loopback only: the port has no password.
    pub fn bind(port: u16) -> Result<Server, ControlError> {
        let bind_error = |e: io::Error| match e.kind() {
            io::ErrorKind::AddrInUse => ControlError::PortInUse { port },
            _ => ControlError::Bind { port, reason: e.to_string() },
        };
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).map_err(bind_error)?;
        let addr = listener.local_addr().map_err(bind_error)?;
        Ok(Server { listener, addr, limits: Limits::default() })
    }

    /// With other time limits. The tests use short ones.
    pub fn with_limits(mut self, limits: Limits) -> Server {
        self.limits = limits;
        self
    }

    /// The address it listens on.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// The port it listens on.
    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    /// Starts serving. Each request is put on the returned inbox for the window to answer, and `ctx` is asked
    /// for a repaint so the window looks at its inbox even if it was idle.
    pub fn spawn(self, ctx: egui::Context) -> Result<ControlInbox, ControlError> {
        let (tx, inbox) = channel();
        let Server { listener, limits, .. } = self;
        // The thread runs as long as the process does: nothing joins it.
        std::thread::Builder::new()
            .name("diacraft-control-accept".into())
            .spawn(move || accept_loop(listener, tx, ctx, limits))
            .map_err(|e| ControlError::Thread(e.to_string()))?;
        Ok(inbox)
    }
}

/// What starting the control channel for the app came to.
pub struct Started {
    /// The inbox, if the channel is on.
    pub inbox: Option<ControlInbox>,
    /// What to tell the person, if it is not.
    pub notice: Option<String>,
}

/// Starts the control channel on `port` for the window that owns `ctx`. It never fails and never exits: if the
/// port cannot be used, the reason is logged and comes back as a notice for the status bar, and the app runs
/// without the channel.
pub fn start(port: u16, ctx: egui::Context) -> Started {
    let started = Server::bind(port).and_then(|server| {
        let addr = server.addr();
        server.spawn(ctx).map(|inbox| (addr, inbox))
    });
    match started {
        Ok((addr, inbox)) => {
            log::info!("control server listening on {addr}");
            Started { inbox: Some(inbox), notice: None }
        }
        Err(e) => {
            log::error!("{e}");
            Started { inbox: None, notice: Some(e.notice()) }
        }
    }
}

fn accept_loop(listener: TcpListener, tx: Sender<ControlRequest>, ctx: egui::Context, limits: Limits) {
    let open = Arc::new(AtomicUsize::new(0));
    accept_each(&listener, |stream| admit(stream, &open, &tx, &ctx, limits));
}

/// Calls `each` with every connection `listener` accepts, for as long as the listener lives. A panic in `each` is
/// caught and logged, and ends nothing: the connection it was for is dropped and the next one is taken. Without the
/// guard it would end this thread, and the control channel with it, and nothing would say so.
fn accept_each(listener: &TcpListener, mut each: impl FnMut(TcpStream)) {
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                if let Err(reason) = guard::catch_panic(|| each(stream)) {
                    log::error!(
                        "control server: a panic while accepting a connection ({}); it was dropped and the server goes on",
                        shown(&reason, MOST_REASON)
                    );
                }
            }
            Err(e) => {
                // Out of descriptors, say: do not spin on it.
                log::warn!("control server: accept failed: {e}");
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

/// Gives a new connection a place and a thread, or tells it there is none.
fn admit(stream: TcpStream, open: &Arc<AtomicUsize>, tx: &Sender<ControlRequest>, ctx: &egui::Context, limits: Limits) {
    let Some(slot) = acquire_slot(open) else {
        refuse(stream, limits.write);
        return;
    };
    let (tx, ctx) = (tx.clone(), ctx.clone());
    // A thread that cannot start drops the closure, and with it the slot.
    let spawned = std::thread::Builder::new().name("diacraft-control".into()).spawn(move || {
        let _slot = slot;
        // A panic while serving one connection is logged and ends that connection (the stream is dropped as it unwinds).
        // The window never sees it: the panic is in this thread, which holds nothing of the window's but the channel.
        if let Err(reason) = guard::catch_panic(|| serve(stream, limits, |method, params| ask_window(&tx, &ctx, limits.reply, method, params))) {
            log::error!("control server: a connection ended with a panic ({}); it was closed", shown(&reason, MOST_REASON));
        }
    });
    if let Err(e) = spawned {
        log::error!("control server: a connection's thread did not start: {e}");
    }
}

/// Tells a client over the limit so, and closes.
fn refuse(stream: TcpStream, write: Duration) {
    let mut out = DeadlineWriter::new(stream, write);
    let reply = json!({"ok": false, "error": format!("too many control connections (at most {MAX_CONNECTIONS}); close one and connect again")});
    // Best effort: the connection is dropped either way, and a peer that cannot read the reason loses nothing.
    if out.begin_reply().and_then(|()| write_line(&mut out, &reply)).is_ok() {
        out.finish();
    }
}

/// One of the [`MAX_CONNECTIONS`] places; returned when dropped.
struct ConnectionSlot(Arc<AtomicUsize>);

fn acquire_slot(open: &Arc<AtomicUsize>) -> Option<ConnectionSlot> {
    let mut n = open.load(Ordering::SeqCst);
    loop {
        if n >= MAX_CONNECTIONS {
            return None;
        }
        match open.compare_exchange_weak(n, n + 1, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => return Some(ConnectionSlot(Arc::clone(open))),
            Err(current) => n = current,
        }
    }
}

impl Drop for ConnectionSlot {
    fn drop(&mut self) {
        // Each acquisition makes exactly one slot, which cannot be cloned.
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Puts a request in the window's inbox and waits for the answer. `None`: the window is gone, and the
/// connection closes.
fn ask_window(tx: &Sender<ControlRequest>, ctx: &egui::Context, wait: Duration, method: String, params: Value) -> Option<ControlReply> {
    let (request, answer) = ControlRequest::new(method, params);
    let ticket = request.ticket();
    tx.send(request).ok()?;
    ctx.request_repaint();
    Some(match answer.recv_timeout(wait) {
        Ok(reply) => reply,
        Err(RecvTimeoutError::Timeout) => Err(timed_out(&ticket)),
        // The window dropped the request without answering: it was closing.
        Err(RecvTimeoutError::Disconnected) => Err("the window closed before it answered".to_owned()),
    })
}

/// The error for a request the window did not answer in time. One it had not started is withdrawn, so it
/// never runs after its caller was told it failed. One it is running cannot be stopped, and the error says
/// its outcome is not known.
fn timed_out(ticket: &Ticket) -> String {
    if ticket.give_up() {
        "timeout: the window did not start the request in time; it was withdrawn and will not run".to_owned()
    } else {
        "timeout: the window is still running the request; it may yet complete, so check before trying again".to_owned()
    }
}

/// One parsed line.
enum Line {
    Request {
        id: Value,
        method: String,
        params: Value,
    },
    Blank,
    /// A request, but a bad one: answer it with this error and carry on with the connection.
    Invalid {
        id: Value,
        error: String,
    },
    /// Not a request: answer with this error and close the connection.
    Reject(String),
}

fn parse_line(bytes: &[u8]) -> Line {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return Line::Reject("not a control request (invalid UTF-8); closing the connection".to_owned());
    };
    let text = text.trim();
    if text.is_empty() {
        return Line::Blank;
    }
    // serde_json stops at 128 levels of nesting ("recursion limit exceeded"): a request cannot make the server
    // recurse deeper, and the feature that removes the limit is not enabled.
    let value = match serde_json::from_str::<Value>(text) {
        Ok(value) => value,
        Err(e) => return Line::Reject(format!("bad JSON ({e}); expected one JSON request object per line; closing the connection")),
    };
    let Value::Object(mut object) = value else {
        return Line::Reject("not a control request (expected a JSON object); closing the connection".to_owned());
    };
    let Some(Value::String(method)) = object.remove("method") else {
        return Line::Reject("not a control request (missing string `method`); closing the connection".to_owned());
    };
    // From here it is a request, and a fault in it is an error addressed to it: the connection stays.
    let id = match object.get("id") {
        None | Some(Value::Null) => Value::Null,
        Some(id @ (Value::Number(_) | Value::String(_))) => id.clone(),
        Some(_) => return Line::Invalid { id: Value::Null, error: "`id` must be a number, a string or null".to_owned() },
    };
    if let Some(key) = object.keys().find(|k| !REQUEST_KEYS.contains(&k.as_str())) {
        // The key is the client's: escaped and cut short before it is echoed.
        let error = format!("unknown key `{}` in the request; a request has id, method and params", shown(key, MOST_SHOWN));
        return Line::Invalid { id, error };
    }
    Line::Request { id, method, params: object.remove("params").unwrap_or(Value::Null) }
}

/// Reads one `\n`-terminated line of at most `max` bytes into `buf` (newline, and a `\r` before it, taken off).
/// `Ok(false)` at the end of the stream; `Err` when the line is too long or the read fails.
fn read_bounded_line(reader: &mut impl BufRead, buf: &mut Vec<u8>, max: usize) -> Result<bool, String> {
    buf.clear();
    // `take` bounds what is buffered for one line: a peer cannot make the server hold more than `max`.
    let limit = u64::try_from(max).unwrap_or(u64::MAX).saturating_add(1);
    let n = reader.take(limit).read_until(b'\n', buf).map_err(|e| match e.kind() {
        // The line's deadline ran out (Windows reports it as TimedOut, Unix as WouldBlock).
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => "request line not completed in time; closing the connection".to_owned(),
        _ => format!("cannot read the request line ({e}); closing the connection"),
    })?;
    if n == 0 {
        return Ok(false);
    }
    if buf.last() == Some(&b'\n') {
        buf.pop();
        if buf.last() == Some(&b'\r') {
            buf.pop();
        }
        return Ok(true);
    }
    if buf.len() > max {
        return Err(format!("request line longer than {max} bytes; closing the connection"));
    }
    // The last line, without a newline, before the end of the stream.
    Ok(true)
}

/// Writes `value` and a newline through a buffer of [`REPLY_CHUNK`] bytes, then flushes.
fn write_line(out: &mut impl Write, value: &Value) -> io::Result<()> {
    let mut out = BufWriter::with_capacity(REPLY_CHUNK, out);
    serde_json::to_writer(&mut out, value)?;
    out.write_all(b"\n")?;
    out.flush()
}

/// The reply line for an answer to the request with `id`.
fn reply_line(id: Value, reply: ControlReply) -> Value {
    match reply {
        Ok(result) => json!({"id": id, "ok": true, "result": result}),
        Err(error) => json!({"id": id, "ok": false, "error": error}),
    }
}

/// Serves one connection until the peer closes it, sends something that is not a request, or `handle` returns
/// `None` (the window is gone). `handle` gets `(method, params)` and returns the answer.
fn serve(stream: TcpStream, limits: Limits, mut handle: impl FnMut(String, Value) -> Option<ControlReply>) {
    let Ok(read_half) = stream.try_clone() else { return };
    let mut reader = BufReader::new(DeadlineReader::new(read_half, limits.idle, limits.line));
    let mut out = DeadlineWriter::new(stream, limits.write);
    let mut line = Vec::new();
    loop {
        let buffered = !reader.buffer().is_empty();
        if reader.get_mut().begin_line(buffered).is_err() {
            return;
        }
        let refusal = match read_bounded_line(&mut reader, &mut line, MAX_LINE) {
            Ok(false) => return,
            // Nothing of a line arrived (the idle time ran out, or the peer is gone): close without a word. A line
            // nobody asked for would be read as the answer to the client's next request.
            Err(_) if line.is_empty() => return,
            Err(refusal) => refusal,
            Ok(true) => match parse_line(&line) {
                Line::Blank => continue,
                Line::Request { id, method, params } => {
                    let Some(reply) = handle(method, params) else { return };
                    if send(&mut out, &reply_line(id, reply)).is_err() {
                        return;
                    }
                    continue;
                }
                Line::Invalid { id, error } => {
                    if send(&mut out, &reply_line(id, Err(error))).is_err() {
                        return;
                    }
                    continue;
                }
                Line::Reject(refusal) => refusal,
            },
        };
        // The refusal, then a clean end: what the peer has sent after it is read and dropped for a moment, because
        // closing with it unread would reset the connection and could take the refusal with it.
        if send(&mut out, &json!({"ok": false, "error": refusal})).is_ok() {
            out.finish();
            reader.get_mut().drain(LINGER, LINGER_BYTES);
        }
        return;
    }
}

fn send(out: &mut DeadlineWriter, value: &Value) -> io::Result<()> {
    out.begin_reply()?;
    write_line(out, value)
}
