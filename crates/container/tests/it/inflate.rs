//! The bounded inflate of a slice: raw DEFLATE and zlib streams made into bytes within a cap and a ratio, with what it cost counted in
//! [`Work::inflated`] and never timed. The streams are built here by `flate2`'s encoder (a test compares what a stream inflates to, never the compressed
//! bytes: different encoders give different bytes), and the hostile ones are the bombs no honest writer makes: a stream of a hundred megabytes of one letter
//! is a hundred kilobytes.

use std::io::Write;

use diacraft_container::Work;
use diacraft_container::inflate::{self, CHUNK, InflateError, Limits, ratio_exceeded, zlib_header};
use flate2::Compression;
use flate2::write::{DeflateEncoder, ZlibEncoder};
use proptest::prelude::*;

/// 1 MiB: the floor of the ratio.
const MIB: u64 = 1 << 20;

/// `data` as raw DEFLATE at `level`.
fn deflated(data: &[u8], level: u32) -> Vec<u8> {
    let mut enc = DeflateEncoder::new(Vec::new(), Compression::new(level));
    enc.write_all(data).unwrap();
    enc.finish().unwrap()
}

/// `data` as a zlib stream at `level`.
fn zlibbed(data: &[u8], level: u32) -> Vec<u8> {
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::new(level));
    enc.write_all(data).unwrap();
    enc.finish().unwrap()
}

/// `n` copies of the letter `A` as raw DEFLATE, made by writing it a megabyte at a time so that the test never holds the bomb it makes.
fn bomb(n: u64) -> Vec<u8> {
    let block = vec![b'A'; 1 << 20];
    // Level 9 makes the dynamic block of one-bit codes that a stream of one letter allows: about a thousand to one, as a bomb is.
    let mut enc = DeflateEncoder::new(Vec::new(), Compression::new(9));
    let mut left = n;
    while left > 0 {
        let take = left.min(block.len() as u64) as usize;
        enc.write_all(&block[..take]).unwrap();
        left -= take as u64;
    }
    enc.finish().unwrap()
}

/// Text that deflates at a ratio far below 200: numbered lines.
fn lines(n: usize) -> Vec<u8> {
    (0..n)
        .map(|i| format!("<mxCell id=\"{i}\" value=\"cell number {}\" parent=\"1\"/>\n", i.wrapping_mul(2_654_435_761) % 100_003))
        .collect::<String>()
        .into_bytes()
}

fn caps(max_out: u64) -> Limits {
    Limits { max_out, ratio: None }
}

fn class_of<T>(r: &Result<T, InflateError>) -> &'static str {
    match r {
        Ok(_) => "ok",
        Err(e) => e.class(),
    }
}

#[test]
fn a_stream_inflates_to_what_it_was_made_of_whatever_its_level_and_size() {
    let data = lines(20_000);
    assert!(data.len() > 3 * CHUNK, "the data spans several chunks: {}", data.len());
    for level in [0, 1, 6, 9] {
        let mut work = Work::default();
        let out = inflate::raw(&deflated(&data, level), &caps(1 << 30), &mut work).unwrap();
        assert_eq!(out, data, "level {level}");
        assert_eq!(work.inflated, data.len() as u64, "the work is the bytes made, at level {level}");
    }
    // Nothing at all: a stream of one empty final block.
    let mut work = Work::default();
    assert_eq!(inflate::raw(&deflated(b"", 6), &caps(0), &mut work).unwrap(), Vec::<u8>::new());
    assert_eq!(work.inflated, 0);
    // The work adds up over calls and is never reset by one.
    let mut work = Work::default();
    for _ in 0..3 {
        inflate::raw(&deflated(b"hello", 6), &caps(100), &mut work).unwrap();
    }
    assert_eq!(work.inflated, 15);
}

#[test]
fn raw_inflate_stops_at_its_cap_and_keeps_no_more_than_a_chunk_past_it() {
    // A hundred megabytes of one letter is about a hundred kilobytes: the cap of a megabyte stops it there.
    let input = bomb(100 << 20);
    assert!(input.len() < 200 << 10, "the bomb is small: {}", input.len());
    let mut work = Work::default();
    let r = inflate::raw(&input, &caps(MIB), &mut work);
    assert_eq!(r, Err(InflateError::TooLarge { max: MIB }));
    assert!(work.inflated > MIB, "the byte past the cap was made: {}", work.inflated);
    assert!(work.inflated <= MIB + CHUNK as u64, "and no more than a chunk past it: {}", work.inflated);

    // The cap is a cap, not a stop short of it: a stream of exactly the cap is read, and one more byte is not.
    let exactly = vec![b'x'; 300_000];
    let mut work = Work::default();
    assert_eq!(inflate::raw(&deflated(&exactly, 6), &caps(300_000), &mut work).unwrap().len(), 300_000);
    assert_eq!(work.inflated, 300_000);
    let mut work = Work::default();
    let r = inflate::raw(&deflated(&exactly, 6), &caps(299_999), &mut work);
    assert_eq!(r, Err(InflateError::TooLarge { max: 299_999 }));
    assert!(work.inflated <= 299_999 + CHUNK as u64, "{}", work.inflated);
    // A cap of nothing refuses any byte.
    let r = inflate::raw(&deflated(b"a", 6), &caps(0), &mut Work::default());
    assert_eq!(r, Err(InflateError::TooLarge { max: 0 }));
}

#[test]
fn the_ratio_is_refused_past_its_floor_and_not_before() {
    let limits = Limits { max_out: 1 << 30, ratio: Some((MIB, 200)) };
    // Exactly the floor, at a ratio of a thousand: not past the floor, so read.
    let at_floor = vec![b'A'; MIB as usize];
    let input = deflated(&at_floor, 6);
    assert!(MIB > 200 * input.len() as u64, "the stream is far past the ratio: {} bytes", input.len());
    assert_eq!(inflate::raw(&input, &limits, &mut Work::default()).unwrap().len(), MIB as usize);
    // One byte past the floor: refused.
    let past = vec![b'A'; MIB as usize + 1];
    let mut work = Work::default();
    let r = inflate::raw(&deflated(&past, 6), &limits, &mut work);
    assert_eq!(r, Err(InflateError::Ratio), "{}", class_of(&r));
    assert!(work.inflated <= MIB + 1 + CHUNK as u64, "{}", work.inflated);
    // A hundred megabytes is stopped long before it is made: at the first chunk past 200 times its own size (about 20 MiB), and not at the cap.
    let input = bomb(100 << 20);
    let mut work = Work::default();
    let r = inflate::raw(&input, &limits, &mut work);
    assert_eq!(r, Err(InflateError::Ratio));
    let at = 200 * input.len() as u64;
    assert!(work.inflated > at, "the ratio was passed before it was refused: {} against {at}", work.inflated);
    assert!(work.inflated <= at + CHUNK as u64, "stopped at once, not at the cap: {} against {at}", work.inflated);
    // A real-looking stream of two megabytes (a ratio far below 200) is read.
    let real = lines(40_000);
    assert!(real.len() > 2 * MIB as usize, "{}", real.len());
    let compressed = deflated(&real, 6);
    assert!((real.len() as u64) < 200 * compressed.len() as u64, "ratio {}", real.len() / compressed.len());
    assert_eq!(inflate::raw(&compressed, &limits, &mut Work::default()).unwrap(), real);
    // With no ratio asked for, the same bomb runs to the cap.
    let r = inflate::raw(&bomb(100 << 20), &Limits { max_out: 5 * MIB, ratio: None }, &mut Work::default());
    assert_eq!(r, Err(InflateError::TooLarge { max: 5 * MIB }));
}

#[test]
fn the_ratio_rule_is_exact_at_its_edges_and_never_overflows() {
    let (floor, ratio) = (MIB, 200);
    // Not past the floor: never refused, whatever the ratio.
    assert!(!ratio_exceeded(0, 0, floor, ratio));
    assert!(!ratio_exceeded(floor, 1, floor, ratio));
    // Past the floor, and exactly at the ratio: not refused; one past it: refused.
    assert!(!ratio_exceeded(floor + 1, floor / 200 + 1, floor, ratio), "a ratio under 200");
    let c = 10_000u64;
    assert!(!ratio_exceeded(200 * c, c, floor, ratio), "exactly 200 to 1");
    assert!(ratio_exceeded(200 * c + 1, c, floor, ratio), "one byte past 200 to 1");
    assert!(ratio_exceeded(floor + 1, 0, floor, ratio), "output from no input at all");
    // The products that would overflow are held at the top, and so are never a refusal that is not there.
    assert!(!ratio_exceeded(u64::MAX, u64::MAX, floor, ratio));
    assert!(!ratio_exceeded(u64::MAX, u64::MAX / 2, floor, u64::MAX));
    assert!(ratio_exceeded(u64::MAX, 1, floor, ratio));
}

#[test]
fn a_zlib_stream_is_read_and_its_checksum_checked() {
    let data = lines(5_000);
    let stream = zlibbed(&data, 9);
    let mut work = Work::default();
    assert_eq!(inflate::zlib(&stream, &caps(1 << 30), &mut work).unwrap(), data);
    assert_eq!(work.inflated, data.len() as u64);
    // The Adler-32 at the end is checked: one bit of it changed, and the stream is damaged.
    let mut bad = stream.clone();
    let last = bad.len() - 1;
    bad[last] ^= 1;
    assert_eq!(inflate::zlib(&bad, &caps(1 << 30), &mut Work::default()), Err(InflateError::Damaged));
    // A stream cut short is damaged too, and so is one with no header.
    assert_eq!(inflate::zlib(&stream[..stream.len() / 2], &caps(1 << 30), &mut Work::default()), Err(InflateError::Damaged));
    assert_eq!(inflate::zlib(&deflated(&data, 6), &caps(1 << 30), &mut Work::default()), Err(InflateError::Damaged));
    // The caps hold for it as for a raw stream.
    let r = inflate::zlib(&zlibbed(&vec![b'A'; 3 << 20], 6), &Limits { max_out: MIB, ratio: None }, &mut Work::default());
    assert_eq!(r, Err(InflateError::TooLarge { max: MIB }));
    let r = inflate::zlib(&zlibbed(&vec![b'A'; 3 << 20], 6), &Limits { max_out: 1 << 30, ratio: Some((MIB, 200)) }, &mut Work::default());
    assert_eq!(r, Err(InflateError::Ratio));
}

#[test]
fn damaged_and_cut_streams_are_damaged_and_never_a_panic() {
    let data = lines(2_000);
    let stream = deflated(&data, 6);
    for cut in [0, 1, 2, stream.len() / 3, stream.len() - 1] {
        let r = inflate::raw(&stream[..cut], &caps(1 << 30), &mut Work::default());
        assert_eq!(r, Err(InflateError::Damaged), "cut at {cut} of {}", stream.len());
    }
    // A block type that does not exist (3), and a stored block whose length and its complement disagree.
    assert_eq!(inflate::raw(&[0b0000_0111], &caps(100), &mut Work::default()), Err(InflateError::Damaged));
    assert_eq!(inflate::raw(&[0x01, 0x05, 0x00, 0x00, 0x00, b'a'], &caps(100), &mut Work::default()), Err(InflateError::Damaged));
    assert_eq!(inflate::raw(&[], &caps(100), &mut Work::default()), Err(InflateError::Damaged));
    // Noise.
    let noise: Vec<u8> = (0..5_000u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8).collect();
    for start in 0..50 {
        let r = inflate::raw(&noise[start..], &caps(1 << 20), &mut Work::default());
        assert!(matches!(r, Ok(_) | Err(InflateError::Damaged | InflateError::TooLarge { .. })), "{r:?}");
    }
}

#[test]
fn zlib_header_is_r13s_rule_for_every_pair_of_bytes() {
    // The low four bits of the first byte are 8 (the method), and the two bytes as a big-endian number are a multiple of 31.
    for b0 in 0..=255u8 {
        for b1 in 0..=255u8 {
            let method = b0 % 16 == 8;
            let number = u32::from(b0) * 256 + u32::from(b1);
            assert_eq!(zlib_header(b0, b1), method && number % 31 == 0, "{b0:#04x} {b1:#04x}");
        }
    }
    // The four headers a zlib encoder writes (levels 1, 2 to 5, 6, 7 to 9), and one a neighbour of them.
    for (b0, b1) in [(0x78, 0x01), (0x78, 0x5E), (0x78, 0x9C), (0x78, 0xDA)] {
        assert!(zlib_header(b0, b1), "{b0:#04x} {b1:#04x}");
    }
    assert!(!zlib_header(0x78, 0x9D));
    assert!(!zlib_header(0x77, 0x9C), "method 7");
    // The header an encoder really wrote is accepted.
    let stream = zlibbed(b"abc", 6);
    assert!(zlib_header(stream[0], stream[1]));
}

#[test]
fn every_error_has_a_class_word_and_a_message_that_names_no_content() {
    let cases = [
        (InflateError::TooLarge { max: 64 }, "inflate_too_large"),
        (InflateError::Ratio, "inflate_ratio_exceeded"),
        (InflateError::Damaged, "inflate_damaged"),
        (InflateError::OutOfMemory, "out_of_memory"),
    ];
    for (error, class) in cases {
        assert_eq!(error.class(), class);
        assert!(!error.to_string().is_empty());
    }
    assert!(InflateError::TooLarge { max: 64 }.to_string().contains("64"), "the cap is said");
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 48, ..ProptestConfig::default() })]

    /// Any bytes at any level come back the same through a raw stream and a zlib one, and the work is their length.
    #[test]
    fn what_is_deflated_is_inflated(data in proptest::collection::vec(any::<u8>(), 0..70_000), level in 0u32..10) {
        let mut work = Work::default();
        let out = inflate::raw(&deflated(&data, level), &caps(1 << 30), &mut work).unwrap();
        prop_assert_eq!(&out, &data);
        prop_assert_eq!(work.inflated, data.len() as u64);
        let out = inflate::zlib(&zlibbed(&data, level), &caps(1 << 30), &mut Work::default()).unwrap();
        prop_assert_eq!(&out, &data);
    }

    /// A cap below the length is refused with at most a chunk past it made, and a cap at or above it is read.
    #[test]
    fn a_cap_decides_by_the_length_and_by_nothing_else(len in 0usize..200_000, cap in 0u64..250_000) {
        let data = vec![b'q'; len];
        let mut work = Work::default();
        let r = inflate::raw(&deflated(&data, 1), &caps(cap), &mut work);
        if (len as u64) <= cap {
            prop_assert_eq!(r.map(|v| v.len()), Ok(len));
        } else {
            prop_assert_eq!(r, Err(InflateError::TooLarge { max: cap }));
            prop_assert!(work.inflated <= cap + CHUNK as u64);
        }
    }
}
