//! A bounded inflate of a slice: raw DEFLATE ([RFC 1951]) and zlib ([RFC 1950]) streams made into bytes, within a cap on what comes out and a ratio of what comes
//! out to what went in. It is the one door for bytes of a hostile stream that are not an entry of a
//! ZIP archive: the archive reader inflates its own entries through its own limits (`zipread.rs`), and every other stream a reader meets (a page of a draw.io
//! file, a text chunk of a PNG) comes here, so that the way a bomb is stopped is written once.
//!
//! **What it promises.**
//!
//! * The output is **charged before each chunk is kept**. The stream is read through a buffer of [`CHUNK`] bytes, and no read asks for more than the byte past the
//!   cap, so that at most one byte more than [`Limits::max_out`] is ever made, and none of it is kept: a stream that passes the cap is [`InflateError::TooLarge`] and
//!   the buffer that was filled is let go with the error. [`Work::inflated`] counts every byte the decoder made, the one past the cap included.
//! * The ratio is held from the first byte past a floor: [`Limits::ratio`] is `(floor, ratio)`, and a stream whose output has passed `floor` bytes and is more than
//!   `ratio` times the size of the input is [`InflateError::Ratio`] **at once**, at the first chunk that shows it, not after the cap (DEFLATE cannot pass about
//!   1,032 to 1, so a stream far past the ratio is a bomb and not a page). The check is after each chunk, so at most a chunk past the floor is made. When one chunk
//!   passes both the cap and the ratio, the cap is the refusal.
//! * **Memory that runs out is [`InflateError::OutOfMemory`]**, not an abort: the buffer grows with `try_reserve_exact`, doubling and never past the cap.
//! * A stream that ends early, a block that is not one, a checksum that is not the stream's (zlib) and any other fault is [`InflateError::Damaged`]. Bytes after the
//!   end of the stream are not looked at.
//!
//! [RFC 1951]: https://www.rfc-editor.org/rfc/rfc1951
//! [RFC 1950]: https://www.rfc-editor.org/rfc/rfc1950

use std::fmt;
use std::io::{ErrorKind, Read};

use crate::work::Work;

/// The bytes a stream is read in: what is made, charged and kept at a time. A bomb is stopped at most this far past its refusal.
pub const CHUNK: usize = 64 * 1024;

/// What an inflate may make.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// The most bytes the stream may inflate to: a stream of more is [`InflateError::TooLarge`], however it is made.
    pub max_out: u64,
    /// `(floor, ratio)`: once the output has passed `floor` bytes, a stream whose output is more than `ratio` times the size of its input is
    /// [`InflateError::Ratio`]. `None` holds no ratio (a caller that knows its streams).
    pub ratio: Option<(u64, u64)>,
}

/// Why a stream could not be inflated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InflateError {
    /// The output passed [`Limits::max_out`]. Class `inflate_too_large`.
    TooLarge {
        /// The cap that was passed.
        max: u64,
    },
    /// The output passed the floor and is more than the ratio of the input. Class `inflate_ratio_exceeded`.
    Ratio,
    /// The stream is not a stream, or is cut short, or its checksum is not the one it states. Class `inflate_damaged`.
    Damaged,
    /// Memory ran out for the output: reported, not an abort. Class `out_of_memory`.
    OutOfMemory,
}

impl InflateError {
    /// The class word.
    pub fn class(&self) -> &'static str {
        match self {
            InflateError::TooLarge { .. } => "inflate_too_large",
            InflateError::Ratio => "inflate_ratio_exceeded",
            InflateError::Damaged => "inflate_damaged",
            InflateError::OutOfMemory => "out_of_memory",
        }
    }
}

impl fmt::Display for InflateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InflateError::TooLarge { max } => write!(f, "the data inflates to more than {max} bytes"),
            InflateError::Ratio => write!(f, "the data inflates to far more than its own size"),
            InflateError::Damaged => write!(f, "the compressed data is damaged"),
            InflateError::OutOfMemory => write!(f, "there was not enough memory to inflate the data"),
        }
    }
}

impl std::error::Error for InflateError {}

/// Whether an output of `out` bytes from an input of `compressed` bytes is past the ratio: `out` is more than `floor` and more than `ratio` times `compressed`.
/// The product saturates, so no input makes it wrap: a product that does not fit is `u64::MAX`, which no output is more than.
pub fn ratio_exceeded(out: u64, compressed: u64, floor: u64, ratio: u64) -> bool {
    out > floor && out > ratio.saturating_mul(compressed)
}

/// Whether two bytes are the header of a zlib stream: the low four bits of the first byte are 8 (the method, deflate), and the two bytes
/// as a big-endian number are a multiple of 31 (RFC 1950's check of the header). The window size and the dictionary flag are not looked at: a stream that has
/// either wrong is damaged when it is inflated.
pub fn zlib_header(b0: u8, b1: u8) -> bool {
    b0 & 0x0F == 8 && (u16::from(b0) * 256 + u16::from(b1)) % 31 == 0
}

/// `input` inflated as raw DEFLATE (no header, no checksum) within `limits`, with the bytes made added to `work.inflated`.
pub fn raw(input: &[u8], limits: &Limits, work: &mut Work) -> Result<Vec<u8>, InflateError> {
    inflate(flate2::read::DeflateDecoder::new(input), input.len(), limits, work)
}

/// `input` inflated as a zlib stream (a two-byte header, DEFLATE, and the Adler-32 of the output, which is checked) within `limits`, with the bytes made added to
/// `work.inflated`.
pub fn zlib(input: &[u8], limits: &Limits, work: &mut Work) -> Result<Vec<u8>, InflateError> {
    inflate(flate2::read::ZlibDecoder::new(input), input.len(), limits, work)
}

/// The body of [`raw`] and [`zlib`]: the decoder read a chunk at a time, each chunk charged to the cap and the ratio before it is kept.
fn inflate(mut source: impl Read, input_len: usize, limits: &Limits, work: &mut Work) -> Result<Vec<u8>, InflateError> {
    let compressed = u64::try_from(input_len).unwrap_or(u64::MAX);
    let mut chunk: Vec<u8> = Vec::new();
    chunk.try_reserve_exact(CHUNK).map_err(|_| InflateError::OutOfMemory)?;
    chunk.resize(CHUNK, 0);
    let mut out: Vec<u8> = Vec::new();
    let mut length = 0u64;
    // At the top of the loop `length` is at most the cap, so this is at least one: the byte past the cap is the most a read asks for.
    let stop = limits.max_out.saturating_add(1);
    loop {
        let want = usize::try_from(stop.saturating_sub(length)).unwrap_or(usize::MAX).min(CHUNK);
        let Some(buffer) = chunk.get_mut(..want) else { return Err(InflateError::Damaged) };
        let made = match source.read(buffer) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            // An incomplete stream, a block that is none, a bad checksum: the decoder's own words are not shown, they come from the bytes.
            Err(_) => return Err(InflateError::Damaged),
        };
        Work::add(&mut work.inflated, made);
        length = length.saturating_add(u64::try_from(made).unwrap_or(u64::MAX));
        // Charged before the chunk is kept: the cap first, then the ratio.
        if length > limits.max_out {
            return Err(InflateError::TooLarge { max: limits.max_out });
        }
        if let Some((floor, ratio)) = limits.ratio
            && ratio_exceeded(length, compressed, floor, ratio)
        {
            return Err(InflateError::Ratio);
        }
        let Some(got) = chunk.get(..made) else { return Err(InflateError::Damaged) };
        keep(&mut out, got, limits.max_out)?;
    }
    Ok(out)
}

/// Appends `got` to `out`, growing the buffer by doubling and never past `max_out`, every growth a `try_reserve_exact`.
fn keep(out: &mut Vec<u8>, got: &[u8], max_out: u64) -> Result<(), InflateError> {
    let need = out.len().saturating_add(got.len());
    if need > out.capacity() {
        let target = grown(out.capacity(), need, usize::try_from(max_out).unwrap_or(usize::MAX));
        out.try_reserve_exact(target.saturating_sub(out.len())).map_err(|_| InflateError::OutOfMemory)?;
    }
    out.extend_from_slice(got);
    Ok(())
}

/// The capacity a buffer of `capacity` is grown to when `need` bytes must fit: double, but never past `ceiling` (the cap, so that the buffer of a page at its cap is not twice it)
/// and never less than `need`. No input makes it wrap: it is at least `need`, and at most the larger of `need` and `ceiling`.
fn grown(capacity: usize, need: usize, ceiling: usize) -> usize {
    capacity.saturating_mul(2).max(need).min(ceiling).max(need)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_buffer_grows_by_doubling_and_never_past_the_cap_or_below_what_must_fit() {
        assert_eq!(grown(0, 10, 1000), 10);
        assert_eq!(grown(100, 101, 1000), 200);
        assert_eq!(grown(600, 601, 1000), 1000, "doubling would pass the cap");
        assert_eq!(grown(900, 1000, 1000), 1000);
        assert_eq!(grown(10, 50, 20), 50, "what must fit wins over a cap that is less (the cap is checked before the chunk is kept)");
        assert_eq!(grown(usize::MAX, usize::MAX, usize::MAX), usize::MAX);
        assert_eq!(grown(usize::MAX / 2 + 1, 5, usize::MAX), usize::MAX);
        for capacity in [0usize, 1, 7, 1 << 20, usize::MAX] {
            for need in [0usize, 1, 100, usize::MAX] {
                for ceiling in [0usize, 100, usize::MAX] {
                    let g = grown(capacity, need, ceiling);
                    assert!(g >= need && g <= need.max(ceiling), "{capacity} {need} {ceiling}: {g}");
                }
            }
        }
    }
}
