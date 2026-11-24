//! Absolute transport budgets, independent of the time spent running a request: a peer can be slow to
//! start a line, slow to finish it, or slow to read the answer, and each has its own limit.

use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::time::{Duration, Instant};

fn deadline(budget: Duration) -> io::Result<Instant> {
    Instant::now().checked_add(budget).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "control transport deadline out of range"))
}

/// What is left of the budget ending at `until`, or a timeout once it has ended (a socket timeout of zero
/// means "never", so zero is not passed on).
fn remaining(until: Option<Instant>) -> io::Result<Duration> {
    until
        .and_then(|d| d.checked_duration_since(Instant::now()))
        .filter(|d| !d.is_zero())
        .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "control transport deadline exceeded"))
}

/// Reads a socket with a time limit on each line: the first byte must come within `idle`, and once it has,
/// the whole line within `line`, however slowly the bytes trickle.
pub(super) struct DeadlineReader {
    stream: TcpStream,
    idle: Duration,
    line: Duration,
    until: Option<Instant>,
    waiting_for_first_byte: bool,
}

impl DeadlineReader {
    pub(super) fn new(stream: TcpStream, idle: Duration, line: Duration) -> DeadlineReader {
        DeadlineReader { stream, idle, line, until: None, waiting_for_first_byte: false }
    }

    /// Starts the budget of the next line. `buffered`: part of it has already been read, so only the line's
    /// budget applies, never a fresh idle one.
    pub(super) fn begin_line(&mut self, buffered: bool) -> io::Result<()> {
        self.waiting_for_first_byte = !buffered;
        self.until = Some(deadline(if buffered { self.line } else { self.idle })?);
        Ok(())
    }

    /// Reads and throws away what the peer is still sending, for at most `budget` and `at_most` bytes. A socket
    /// closed with the peer's bytes unread is reset, and the reset can take an answer already sent with it:
    /// draining a little first lets the answer arrive, and the end of stream after it.
    pub(super) fn drain(&mut self, budget: Duration, at_most: u64) {
        let Ok(until) = deadline(budget) else { return };
        let mut left = at_most;
        let mut sink = [0_u8; 16 * 1024];
        while left > 0 {
            let Ok(wait) = remaining(Some(until)) else { return };
            if self.stream.set_read_timeout(Some(wait)).is_err() {
                return;
            }
            match self.stream.read(&mut sink) {
                Ok(0) | Err(_) => return,
                Ok(n) => left = left.saturating_sub(u64::try_from(n).unwrap_or(u64::MAX)),
            }
        }
    }
}

impl Read for DeadlineReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        self.stream.set_read_timeout(Some(remaining(self.until)?))?;
        let n = self.stream.read(buf)?;
        if n > 0 && self.waiting_for_first_byte {
            self.until = Some(deadline(self.line)?);
            self.waiting_for_first_byte = false;
        }
        Ok(n)
    }
}

/// Writes a socket with a time limit on each reply.
pub(super) struct DeadlineWriter {
    stream: TcpStream,
    budget: Duration,
    until: Option<Instant>,
}

impl DeadlineWriter {
    pub(super) fn new(stream: TcpStream, budget: Duration) -> DeadlineWriter {
        DeadlineWriter { stream, budget, until: None }
    }

    /// Starts the budget of the next reply.
    pub(super) fn begin_reply(&mut self) -> io::Result<()> {
        self.until = Some(deadline(self.budget)?);
        Ok(())
    }

    /// Ends the stream the peer reads: it sees what was written, then the end.
    pub(super) fn finish(&self) {
        // The peer may be gone already; then there is nothing to end.
        let _ = self.stream.shutdown(Shutdown::Write);
    }
}

impl Write for DeadlineWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        // Each partial write gets what is left of the budget, never a fresh full timeout.
        self.stream.set_write_timeout(Some(remaining(self.until)?))?;
        self.stream.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stream.set_write_timeout(Some(remaining(self.until)?))?;
        self.stream.flush()
    }
}
