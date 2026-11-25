//! A small client of Chrome's DevTools protocol, for `cargo xtask web-dpr`: one WebSocket (RFC 6455, the client's side) to a Chrome started with `--remote-debugging-port=0`, std only.
//!
//! **Why it is here.** The only faithful check of how the web app sizes its canvas at a device scale factor is a browser that is told to emulate one (`Emulation.setDeviceMetricsOverride`): nothing but the protocol
//! can ask for that. The project's tools are Rust and have no library for a WebSocket, so this is the part of it a loopback connection to a browser we started needs: the opening handshake (the answer's
//! `Sec-WebSocket-Accept` is not verified: the peer is a program this one started, on the loopback address), text frames out (masked, as a client must), and text messages in (fragments joined,
//! pings and pongs skipped). A message is held to [`MAX_MESSAGE`] before a byte of it is kept, and a read that stops for [`READ_TIMEOUT`] is an error, never a hang.

use std::io::{BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::time::Duration;

use serde_json::{Value, json};

/// The most bytes of one message the client takes. The messages of this check are a few hundred bytes.
pub(super) const MAX_MESSAGE: usize = 4 * 1024 * 1024;

/// How long a read waits for the browser: a page that is loading the app's 18 MB of code answers an evaluation late, and a browser that says nothing for this long is not coming back.
pub(super) const READ_TIMEOUT: Duration = Duration::from_secs(60);

/// The most bytes of the answer to the handshake that are read: a browser's is a few hundred.
const MAX_HEAD: usize = 16 * 1024;

/// The `Sec-WebSocket-Key` of the handshake: the example of RFC 6455, section 1.3. It is not a secret and not checked (see the top of this file).
const WEBSOCKET_KEY: &str = "dGhlIHNhbXBsZSBub25jZQ==";

/// The request that opens the WebSocket at `path` of `host` (`127.0.0.1:9222`).
pub(super) fn handshake_request(host: &str, path: &str) -> String {
    format!(
        "GET {path} HTTP/1.1\r\nHost: {host}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {WEBSOCKET_KEY}\r\nSec-WebSocket-Version: 13\r\n\r\n"
    )
}

/// Whether the head the browser answered the handshake with agrees to the upgrade (status 101).
pub(super) fn check_handshake_response(head: &str) -> Result<(), String> {
    let status = head.lines().next().unwrap_or_default();
    if status.split_whitespace().nth(1) == Some("101") {
        Ok(())
    } else {
        Err(format!("the browser did not open the WebSocket: it answered `{}`", status.chars().take(120).collect::<String>()))
    }
}

/// A text frame carrying `payload`, as a client sends it: final, masked with `mask`, with the length in 7 bits, 16 or 64 as its size needs.
pub(super) fn encode_text_frame(payload: &[u8], mask: [u8; 4]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(payload.len() + 14);
    frame.push(0x81);
    let len = payload.len();
    if let Ok(short) = u8::try_from(len)
        && short < 126
    {
        frame.push(0x80 | short);
    } else if let Ok(medium) = u16::try_from(len) {
        frame.push(0x80 | 126);
        frame.extend_from_slice(&medium.to_be_bytes());
    } else {
        frame.push(0x80 | 127);
        frame.extend_from_slice(&u64::try_from(len).unwrap_or(u64::MAX).to_be_bytes());
    }
    frame.extend_from_slice(&mask);
    frame.extend(payload.iter().zip(mask.iter().cycle()).map(|(byte, key)| byte ^ key));
    frame
}

/// One frame as the server sent it: whether it is the last of its message, its opcode and its payload.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Frame {
    pub(super) fin: bool,
    pub(super) opcode: u8,
    pub(super) payload: Vec<u8>,
}

/// Reads one frame from `reader`. A server's frames are never masked, a length past [`MAX_MESSAGE`] is refused before the payload is read, and a reserved bit is a protocol error.
pub(super) fn read_frame<R: Read>(reader: &mut R) -> Result<Frame, String> {
    let mut head = [0u8; 2];
    reader.read_exact(&mut head).map_err(|e| format!("the browser's connection ended: {e}"))?;
    let [first, second] = head;
    if first & 0x70 != 0 {
        return Err("the browser sent a frame with a reserved bit set".into());
    }
    if second & 0x80 != 0 {
        return Err("the browser sent a masked frame".into());
    }
    let len = match second & 0x7f {
        126 => {
            let mut bytes = [0u8; 2];
            reader.read_exact(&mut bytes).map_err(|e| format!("the browser's connection ended: {e}"))?;
            u64::from(u16::from_be_bytes(bytes))
        }
        127 => {
            let mut bytes = [0u8; 8];
            reader.read_exact(&mut bytes).map_err(|e| format!("the browser's connection ended: {e}"))?;
            u64::from_be_bytes(bytes)
        }
        short => u64::from(short),
    };
    if len > MAX_MESSAGE as u64 {
        return Err(format!("the browser sent a frame of {len} bytes, more than the {MAX_MESSAGE} this check takes"));
    }
    // Read up to the length and no further, and ask for the room as the bytes arrive: a length that is a lie costs nothing.
    let mut payload = Vec::new();
    reader.take(len).read_to_end(&mut payload).map_err(|e| format!("the browser's connection ended: {e}"))?;
    if payload.len() as u64 != len {
        return Err("the browser's connection ended inside a frame".into());
    }
    Ok(Frame { fin: first & 0x80 != 0, opcode: first & 0x0f, payload })
}

/// The next text message: its fragments joined, pings and pongs skipped. A close frame, a binary message and a message past [`MAX_MESSAGE`] are errors.
pub(super) fn read_message<R: Read>(reader: &mut R) -> Result<String, String> {
    let mut message: Vec<u8> = Vec::new();
    let mut started = false;
    loop {
        let frame = read_frame(reader)?;
        match frame.opcode {
            // Ping and pong: nothing to say to a browser that does not wait for an answer.
            0x9 | 0xa => continue,
            0x8 => return Err("the browser closed the connection".into()),
            0x1 if !started => started = true,
            0x0 if started => {}
            other => return Err(format!("the browser sent a frame of kind {other:#x} that this check does not take")),
        }
        if message.len() + frame.payload.len() > MAX_MESSAGE {
            return Err(format!("the browser sent a message of more than {MAX_MESSAGE} bytes"));
        }
        message.extend_from_slice(&frame.payload);
        if frame.fin {
            return String::from_utf8(message).map_err(|_| "the browser sent text that is not UTF-8".to_owned());
        }
    }
}

/// A connection to a Chrome's DevTools protocol.
#[derive(Debug)]
pub(super) struct Cdp {
    writer: TcpStream,
    reader: BufReader<TcpStream>,
    next_id: u64,
}

impl Cdp {
    /// Opens the WebSocket at `path` (`/devtools/browser/<id>`) of the browser listening on the loopback `port`.
    pub(super) fn connect(port: u16, path: &str) -> Result<Cdp, String> {
        if !path.starts_with('/') || path.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err(format!("`{path}` is not a path the browser's DevTools listen on"));
        }
        let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(10))
            .map_err(|e| format!("could not connect to the browser on port {port}: {e}"))?;
        stream.set_read_timeout(Some(READ_TIMEOUT)).map_err(|e| format!("could not set a timeout on the connection: {e}"))?;
        stream.set_write_timeout(Some(Duration::from_secs(30))).map_err(|e| format!("could not set a timeout on the connection: {e}"))?;
        stream
            .write_all(handshake_request(&format!("127.0.0.1:{port}"), path).as_bytes())
            .map_err(|e| format!("could not write to the browser: {e}"))?;
        // The head of the answer is read a byte at a time, so that not a byte of the first frame is taken with it.
        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            if head.len() >= MAX_HEAD {
                return Err("the browser's answer to the handshake is too long".into());
            }
            stream.read_exact(&mut byte).map_err(|e| format!("the browser did not answer the handshake: {e}"))?;
            head.extend_from_slice(&byte);
        }
        check_handshake_response(&String::from_utf8_lossy(&head))?;
        let reader = BufReader::new(stream.try_clone().map_err(|e| format!("could not share the connection: {e}"))?);
        Ok(Cdp { writer: stream, reader, next_id: 0 })
    }

    /// Runs `method` with `params` (in the page's `session`, when one is given) and returns its `result`. Events and answers to other calls that arrive meanwhile are skipped.
    pub(super) fn call(&mut self, session: Option<&str>, method: &str, params: Value) -> Result<Value, String> {
        self.next_id += 1;
        let id = self.next_id;
        let mut request = json!({ "id": id, "method": method, "params": params });
        if let (Some(session), Some(object)) = (session, request.as_object_mut()) {
            object.insert("sessionId".to_owned(), Value::String(session.to_owned()));
        }
        // The mask is no secret (the peer is on this machine): the id spread over four bytes.
        let mask = u32::try_from(id % u64::from(u32::MAX)).unwrap_or(1).wrapping_mul(0x9E37_79B1).to_be_bytes();
        let frame = encode_text_frame(request.to_string().as_bytes(), mask);
        self.writer.write_all(&frame).map_err(|e| format!("{method}: could not write to the browser: {e}"))?;
        loop {
            let text = read_message(&mut self.reader).map_err(|e| format!("{method}: {e}"))?;
            let Ok(message) = serde_json::from_str::<Value>(&text) else { continue };
            if message.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if let Some(error) = message.get("error") {
                return Err(format!("{method}: the browser answered with an error: {}", error.to_string().chars().take(300).collect::<String>()));
            }
            return Ok(message.get("result").cloned().unwrap_or(Value::Null));
        }
    }
}

/// What the browser wrote in its `DevToolsActivePort` file: the port it listens on and the path of its DevTools WebSocket (`9222` and `/devtools/browser/<id>`).
pub(super) fn parse_active_port(text: &str) -> Result<(u16, String), String> {
    let mut lines = text.lines();
    let port = lines
        .next()
        .unwrap_or_default()
        .trim()
        .parse::<u16>()
        .map_err(|_| "the browser's DevToolsActivePort file has no port in its first line".to_owned())?;
    let path = lines.next().unwrap_or_default().trim();
    if port == 0 || !path.starts_with("/devtools/") {
        return Err("the browser's DevToolsActivePort file has no DevTools path in its second line".into());
    }
    Ok((port, path.to_owned()))
}
