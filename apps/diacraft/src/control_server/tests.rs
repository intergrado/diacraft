//! The accept loop's own tests: what a panic in taking one connection does to the loop. The transport as a client sees it is
//! tested over TCP in `tests/it/control_transport.rs`.

use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use log::{LevelFilter, Log, Metadata, Record};

use super::accept_each;

/// How long a test waits for the loop to take a connection. It only ends a test whose loop is gone.
const WAIT: Duration = Duration::from_secs(10);

/// The records this test binary logged, kept so a test can see what the server said about a panic.
static RECORDS: Mutex<Vec<String>> = Mutex::new(Vec::new());

struct Capture;

impl Log for Capture {
    fn enabled(&self, _: &Metadata<'_>) -> bool {
        true
    }

    fn log(&self, record: &Record<'_>) {
        RECORDS.lock().unwrap().push(format!("{} {}", record.level(), record.args()));
    }

    fn flush(&self) {}
}

fn capture_logs() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        // Another logger installed first would leave this test with nothing to read, and it would say so.
        let _ = log::set_logger(&Capture);
        log::set_max_level(LevelFilter::Info);
    });
}

fn records_with(marker: &str) -> Vec<String> {
    RECORDS.lock().unwrap().iter().filter(|r| r.contains(marker)).cloned().collect()
}

/// The loop ran the work for a connection that panics, and went on to take the next one: before the guard, the panic ended the
/// accept thread, and with it the control channel, without a word.
#[test]
fn a_panic_in_taking_one_connection_does_not_end_the_accept_loop() {
    capture_logs();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let addr = listener.local_addr().unwrap();
    let (taken, taken_rx) = channel();
    let marker = "accept boom \u{1b}[2J \n marker-7d1";
    std::thread::spawn(move || {
        let mut n = 0_u32;
        accept_each(&listener, |stream: TcpStream| {
            n += 1;
            if n == 1 {
                panic!("{marker}");
            }
            taken.send((n, stream)).ok();
        });
    });
    let _first = TcpStream::connect(addr).unwrap();
    let _second = TcpStream::connect(addr).unwrap();
    match taken_rx.recv_timeout(WAIT) {
        Ok((n, _)) => assert_eq!(n, 2, "the second connection is the one that was taken after the panic"),
        Err(RecvTimeoutError::Timeout) => panic!("the accept loop took no connection after one that panicked: the loop is gone"),
        Err(RecvTimeoutError::Disconnected) => panic!("the accept loop ended after a panic"),
    }
    // The reason is in the log, escaped: the text of a panic can hold what a client sent.
    let ours = records_with("marker-7d1").into_iter().filter(|r| r.contains("accepting a connection")).collect::<Vec<_>>();
    assert_eq!(ours.len(), 1, "{ours:?}");
    assert!(ours[0].starts_with("ERROR ") && ours[0].contains("accept boom \\u{1b}[2J \\n marker-7d1"), "{ours:?}");
    assert!(!ours[0].chars().any(char::is_control), "{ours:?}");
}

/// A loop whose work does not panic still takes every connection, in order.
#[test]
fn every_connection_is_taken_when_nothing_panics() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let addr = listener.local_addr().unwrap();
    let (taken, taken_rx) = channel();
    std::thread::spawn(move || {
        let mut n = 0_u32;
        accept_each(&listener, |stream: TcpStream| {
            n += 1;
            taken.send((n, stream)).ok();
        });
    });
    let _clients: Vec<TcpStream> = (0..3).map(|_| TcpStream::connect(addr).unwrap()).collect();
    let order: Vec<u32> = (0..3).map(|_| taken_rx.recv_timeout(WAIT).unwrap().0).collect();
    assert_eq!(order, [1, 2, 3]);
}
