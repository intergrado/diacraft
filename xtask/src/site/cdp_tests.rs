//! Tests of the DevTools client (`cdp.rs`): the frames it writes and reads, and a call against a stand-in browser on a loopback socket. Tests may unwrap and panic.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::{Cursor, Read, Write};
use std::net::TcpListener;
use std::thread;

use serde_json::{Value, json};

use super::cdp::{
    Cdp, Frame, MAX_MESSAGE, check_handshake_response, encode_text_frame, handshake_request, parse_active_port, read_frame, read_message,
};

/// A frame as a server writes it: unmasked.
fn server_frame(fin: bool, opcode: u8, payload: &[u8]) -> Vec<u8> {
    let mut frame = vec![(if fin { 0x80 } else { 0 }) | opcode];
    if payload.len() < 126 {
        frame.push(payload.len() as u8);
    } else if payload.len() <= 0xffff {
        frame.push(126);
        frame.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    } else {
        frame.push(127);
        frame.extend_from_slice(&(payload.len() as u64).to_be_bytes());
    }
    frame.extend_from_slice(payload);
    frame
}

/// Reads a client's frame from `reader` and gives its payload unmasked (what a server does).
fn read_client_frame(reader: &mut impl Read) -> Vec<u8> {
    let mut head = [0u8; 2];
    reader.read_exact(&mut head).unwrap();
    assert_eq!(head[0], 0x81, "a text frame, final");
    assert_eq!(head[1] & 0x80, 0x80, "a client's frame is masked");
    let len = match head[1] & 0x7f {
        126 => {
            let mut b = [0u8; 2];
            reader.read_exact(&mut b).unwrap();
            u16::from_be_bytes(b) as usize
        }
        127 => {
            let mut b = [0u8; 8];
            reader.read_exact(&mut b).unwrap();
            u64::from_be_bytes(b) as usize
        }
        n => n as usize,
    };
    let mut mask = [0u8; 4];
    reader.read_exact(&mut mask).unwrap();
    let mut payload = vec![0u8; len];
    reader.read_exact(&mut payload).unwrap();
    payload.iter().zip(mask.iter().cycle()).map(|(b, m)| b ^ m).collect()
}

#[test]
fn the_handshake_asks_for_an_upgrade_of_the_path() {
    let request = handshake_request("127.0.0.1:9222", "/devtools/browser/abc");
    assert!(request.starts_with("GET /devtools/browser/abc HTTP/1.1\r\n"));
    for line in ["Host: 127.0.0.1:9222", "Upgrade: websocket", "Connection: Upgrade", "Sec-WebSocket-Version: 13"] {
        assert!(request.contains(&format!("{line}\r\n")), "no `{line}` in {request}");
    }
    assert!(request.contains("Sec-WebSocket-Key: "));
    assert!(request.ends_with("\r\n\r\n"));
}

#[test]
fn only_a_101_opens_the_socket() {
    assert!(check_handshake_response("HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n\r\n").is_ok());
    let refused = check_handshake_response("HTTP/1.1 403 Forbidden\r\n\r\n").unwrap_err();
    assert!(refused.contains("403 Forbidden"), "{refused}");
    assert!(check_handshake_response("").is_err());
    assert!(check_handshake_response("garbage").is_err());
}

#[test]
fn a_short_frame_is_masked_with_the_key() {
    let frame = encode_text_frame(b"hi", [1, 2, 3, 4]);
    assert_eq!(frame, vec![0x81, 0x82, 1, 2, 3, 4, b'h' ^ 1, b'i' ^ 2]);
}

#[test]
fn the_length_takes_the_form_its_size_needs() {
    let medium = encode_text_frame(&[b'a'; 200], [0; 4]);
    assert_eq!(&medium[..4], &[0x81, 0x80 | 126, 0x00, 200]);
    assert_eq!(medium.len(), 4 + 4 + 200);
    let edge = encode_text_frame(&[b'a'; 125], [0; 4]);
    assert_eq!(&edge[..2], &[0x81, 0x80 | 125]);
    let first_medium = encode_text_frame(&[b'a'; 126], [0; 4]);
    assert_eq!(&first_medium[..4], &[0x81, 0x80 | 126, 0, 126]);
    let large = encode_text_frame(&vec![b'a'; 65_536], [0; 4]);
    assert_eq!(&large[..10], &[0x81, 0x80 | 127, 0, 0, 0, 0, 0, 1, 0, 0]);
    assert_eq!(large.len(), 10 + 4 + 65_536);
}

#[test]
fn a_masked_frame_unmasks_to_its_payload_at_every_size() {
    for size in [0usize, 1, 125, 126, 4000, 65_535, 65_536, 70_000] {
        let payload: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();
        let frame = encode_text_frame(&payload, [0xde, 0xad, 0xbe, 0xef]);
        assert_eq!(read_client_frame(&mut Cursor::new(frame)), payload, "size {size}");
    }
}

#[test]
fn a_server_frame_is_read_in_each_length_form() {
    for size in [0usize, 5, 125, 126, 300, 65_535, 65_536] {
        let payload = vec![b'x'; size];
        let frame = read_frame(&mut Cursor::new(server_frame(true, 1, &payload))).unwrap();
        assert_eq!(frame, Frame { fin: true, opcode: 1, payload }, "size {size}");
    }
    let continuation = read_frame(&mut Cursor::new(server_frame(false, 0, b"ab"))).unwrap();
    assert_eq!((continuation.fin, continuation.opcode), (false, 0));
}

#[test]
fn a_frame_that_breaks_the_protocol_is_refused() {
    let mut reserved = server_frame(true, 1, b"x");
    reserved[0] |= 0x40;
    assert!(read_frame(&mut Cursor::new(reserved)).unwrap_err().contains("reserved bit"));
    let mut masked = server_frame(true, 1, b"x");
    masked[1] |= 0x80;
    assert!(read_frame(&mut Cursor::new(masked)).unwrap_err().contains("masked"));
    assert!(read_frame(&mut Cursor::new(Vec::new())).unwrap_err().contains("ended"));
    // A frame that says 10 bytes and has 3.
    assert!(read_frame(&mut Cursor::new(vec![0x81, 10, b'a', b'b', b'c'])).unwrap_err().contains("inside a frame"));
}

/// A length that is a lie costs nothing: it is refused before a byte of payload is read or any room is made for it.
#[test]
fn a_frame_longer_than_the_most_is_refused_before_its_payload_is_read() {
    let mut head = vec![0x81, 127];
    head.extend_from_slice(&((MAX_MESSAGE as u64) + 1).to_be_bytes());
    let error = read_frame(&mut Cursor::new(head)).unwrap_err();
    assert!(error.contains("more than"), "{error}");
    let mut absurd = vec![0x81, 127];
    absurd.extend_from_slice(&u64::MAX.to_be_bytes());
    assert!(read_frame(&mut Cursor::new(absurd)).unwrap_err().contains("more than"));
}

#[test]
fn fragments_are_joined_and_pings_are_skipped() {
    let mut wire = server_frame(true, 9, b"ping");
    wire.extend(server_frame(false, 1, b"hel"));
    wire.extend(server_frame(true, 0xa, b"pong"));
    wire.extend(server_frame(false, 0, b"lo, "));
    wire.extend(server_frame(true, 0, b"world"));
    assert_eq!(read_message(&mut Cursor::new(wire)).unwrap(), "hello, world");
}

#[test]
fn a_message_that_is_not_a_text_message_is_an_error() {
    assert!(read_message(&mut Cursor::new(server_frame(true, 8, b""))).unwrap_err().contains("closed"));
    assert!(read_message(&mut Cursor::new(server_frame(true, 2, b"\x00\x01"))).unwrap_err().contains("does not take"));
    // A continuation with nothing to continue, and a text frame in the middle of a message.
    assert!(read_message(&mut Cursor::new(server_frame(true, 0, b"x"))).is_err());
    let mut wire = server_frame(false, 1, b"a");
    wire.extend(server_frame(true, 1, b"b"));
    assert!(read_message(&mut Cursor::new(wire)).is_err());
    assert!(read_message(&mut Cursor::new(server_frame(true, 1, &[0xff, 0xfe]))).unwrap_err().contains("UTF-8"));
}

#[test]
fn fragments_that_add_up_past_the_most_are_refused() {
    let piece = vec![b'a'; MAX_MESSAGE / 2 + 1];
    let mut wire = server_frame(false, 1, &piece);
    wire.extend(server_frame(true, 0, &piece));
    assert!(read_message(&mut Cursor::new(wire)).unwrap_err().contains("more than"));
}

#[test]
fn the_active_port_file_gives_the_port_and_the_path() {
    assert_eq!(parse_active_port("9222\n/devtools/browser/abc-123\n").unwrap(), (9222, "/devtools/browser/abc-123".to_owned()));
    assert_eq!(parse_active_port("40001\r\n/devtools/browser/x\r\n").unwrap().0, 40001);
    assert!(parse_active_port("").is_err());
    assert!(parse_active_port("not a port\n/devtools/browser/x\n").is_err());
    assert!(parse_active_port("0\n/devtools/browser/x\n").is_err());
    assert!(parse_active_port("70000\n/devtools/browser/x\n").is_err());
    assert!(parse_active_port("9222\n").is_err());
    assert!(parse_active_port("9222\n/etc/passwd\n").is_err());
}

/// A stand-in browser on a loopback socket: does the handshake, then answers each call after an event and an answer to another call, as a browser does. `Bad.call` is answered with an error.
fn fake_browser() -> (u16, thread::JoinHandle<Vec<Value>>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            stream.read_exact(&mut byte).unwrap();
            head.push(byte[0]);
        }
        assert!(String::from_utf8(head).unwrap().starts_with("GET /devtools/browser/test HTTP/1.1"));
        let mut seen = Vec::new();
        let mut first = true;
        // The head and an event come in one write, as a fast browser's may: nothing of the event may be lost to the handshake.
        let mut opening = b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n".to_vec();
        opening.extend(server_frame(true, 1, json!({"method": "Target.targetCreated", "params": {}}).to_string().as_bytes()));
        stream.write_all(&opening).unwrap();
        loop {
            let mut probe = [0u8; 1];
            // The client ends the connection when it is dropped.
            match stream.peek(&mut probe) {
                Ok(n) if n > 0 => {}
                _ => break,
            }
            let request: Value = serde_json::from_slice(&read_client_frame(&mut stream)).unwrap();
            let id = request["id"].as_u64().unwrap();
            let event = json!({"method": "Page.frameNavigated", "params": {}});
            stream.write_all(&server_frame(true, 1, event.to_string().as_bytes())).unwrap();
            let other = json!({"id": id + 1000, "result": {}});
            stream.write_all(&server_frame(true, 1, other.to_string().as_bytes())).unwrap();
            let answer = if request["method"] == "Bad.call" {
                json!({"id": id, "error": {"code": -32601, "message": "not found"}})
            } else {
                json!({"id": id, "result": {"echo": request["params"], "session": request["sessionId"]}})
            };
            let text = answer.to_string();
            // The first answer is split over two frames; later ones are not.
            if first {
                first = false;
                let (a, b) = text.as_bytes().split_at(text.len() / 2);
                stream.write_all(&server_frame(false, 1, a)).unwrap();
                stream.write_all(&server_frame(true, 0, b)).unwrap();
            } else {
                stream.write_all(&server_frame(true, 1, text.as_bytes())).unwrap();
            }
            seen.push(request);
        }
        seen
    });
    (port, handle)
}

#[test]
fn a_call_gets_its_own_answer_past_events_and_other_answers() {
    let (port, browser) = fake_browser();
    let mut cdp = Cdp::connect(port, "/devtools/browser/test").unwrap();
    let first = cdp.call(None, "Target.createTarget", json!({"url": "about:blank"})).unwrap();
    assert_eq!(first["echo"], json!({"url": "about:blank"}));
    assert_eq!(first["session"], Value::Null);
    let second = cdp.call(Some("S1"), "Runtime.evaluate", json!({"expression": "1"})).unwrap();
    assert_eq!(second["echo"], json!({"expression": "1"}));
    assert_eq!(second["session"], json!("S1"));
    drop(cdp);
    let seen = browser.join().unwrap();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[0]["method"], "Target.createTarget");
    assert_eq!(seen[1]["id"].as_u64().unwrap(), seen[0]["id"].as_u64().unwrap() + 1, "each call has its own id");
}

#[test]
fn an_error_of_the_browser_is_the_calls_error() {
    let (port, browser) = fake_browser();
    let mut cdp = Cdp::connect(port, "/devtools/browser/test").unwrap();
    let error = cdp.call(None, "Bad.call", json!({})).unwrap_err();
    assert!(error.contains("Bad.call") && error.contains("not found"), "{error}");
    drop(cdp);
    browser.join().unwrap();
}

#[test]
fn a_path_that_is_not_a_devtools_path_is_not_connected_to() {
    for path in ["devtools", "/dev tools", "/a\r\nHost: x"] {
        assert!(Cdp::connect(1, path).unwrap_err().contains("not a path"), "{path:?}");
    }
}

#[test]
fn a_browser_that_is_not_there_is_an_error_and_not_a_hang() {
    // A port nobody listens on: bound, then released.
    let port = TcpListener::bind(("127.0.0.1", 0)).unwrap().local_addr().unwrap().port();
    assert!(Cdp::connect(port, "/devtools/browser/x").unwrap_err().contains("could not connect"));
}
