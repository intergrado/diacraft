//! The input and the properties of the fuzz target `mxfile_decode`: bytes become the text of a compressed page, or the steps of its decoding one by one, or a
//! whole file, and the reader is held to its contract on them. It is a library so that the reader's own tests include this file (`crates/mxfile/tests/it/fuzz.rs`) and hold
//! a seed corpus and random inputs to the same law the fuzzer holds the bytes to, and replay on it the inputs the target crashed on.
//!
//! **The input.** The first byte picks what the rest is: 0, the text of a page, as it would stand between `<diagram>` and `</diagram>`; 1, the XML of a page, which is
//! encoded as the editor encodes it and decoded again; 2, the steps of R11 on their own (Base64, URI decoding, the clean) on the bytes and on what the encoder
//! makes of them; 3, a file; 4, a file of one page whose text is the rest, and one of one page whose text is the rest encoded.
//!
//! **The properties.** The reader never panics and never hangs; the work it counts never passes the budget it was given, whether it reads or refuses; what it gives
//! is the same the second time; a text it gives holds no character the clean takes out and no more than the page cap; what the encoder encodes the decoder decodes (a
//! page of under a third of the floor of the ratio, which no ratio refuses, is always read back to the cleaned text it was made of); a drawing it gives holds the model's
//! caps and a page for each page it counts.

/// The law of the target `mxfile_read`: the file, the pages and the cells.
#[path = "read.rs"]
pub mod read;

use diacraft_mxfile::decode::{self, Fail};
use diacraft_mxfile::limits::MAX_PAGE_TEXT;
use diacraft_mxfile::{Limits, ReadWork, Reading, read_counted};

/// The most bytes of the floor of the ratio (1 MiB) that an encoded page of the round trip may come to, so that no ratio refuses it: URI encoding makes a byte at most three.
const ROUND_TRIP_MAX: usize = (1 << 20) / 3;

/// `text` as JavaScript's `encodeURIComponent` writes it.
fn uri_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for &b in text.as_bytes() {
        if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// `data` as standard padded Base64.
fn base64(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for group in data.chunks(3) {
        let n = group.iter().fold(0u32, |acc, b| acc << 8 | u32::from(*b)) << (8 * (3 - group.len()));
        for k in 0..4 {
            if k <= group.len() {
                out.push(char::from(ALPHABET.get((n >> (18 - 6 * k) & 63) as usize).copied().unwrap_or(b'A')));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The text of a compressed page for `xml`: the three forward steps.
fn encode(xml: &str) -> String {
    use std::io::Write;
    let mut enc = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::new(6));
    let _ = enc.write_all(uri_encode(xml).as_bytes()); // a write into a Vec does not fail
    base64(&enc.finish().unwrap_or_default())
}

/// `text` as the clean leaves it.
fn cleaned(text: &str) -> String {
    decode::clean(text).map(std::borrow::Cow::into_owned).unwrap_or_default()
}

/// The text a byte stream is read as: UTF-8 with what is not made into replacement characters.
fn text_of(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Whether a result of the text stage is one that holds the contract of the stage, and what it says as a line, so that two runs can be compared.
fn stage_line(r: &Result<decode::TextStage, Fail>, reading: &Reading) -> String {
    let work = reading.report.work;
    assert!(work.total() <= reading.limits.max_read_work, "the work {} is past the budget", work.total());
    match r {
        Ok(stage) => {
            assert!(stage.text.len() as u64 <= MAX_PAGE_TEXT, "a text past the cap of a page");
            assert_eq!(cleaned(&stage.text), stage.text, "a text that holds a character the clean takes out");
            format!("ok {} {} {}", stage.not_uri_encoded, stage.zlib_wrapped, stage.text)
        }
        Err(Fail::Undecodable { step, why }) => format!("undecodable {} {why}", step.number()),
        Err(Fail::Refused(e)) => format!("refused {} {}", e.class, e.words),
    }
}

/// The text stage on `text`: no panic, the work within its budget, and the same answer twice.
fn text_stage(text: &str) -> Result<decode::TextStage, Fail> {
    let limits = Limits::default();
    let mut reading = Reading::new(&limits);
    let first = decode::compressed_text(text, &mut reading);
    let line = stage_line(&first, &reading);
    let mut again = Reading::new(&limits);
    let second = decode::compressed_text(text, &mut again);
    assert_eq!(line, stage_line(&second, &again), "the same text, two answers");
    assert_eq!(reading.report.work, again.report.work, "the same text, two counts");
    first
}

/// The whole read of `bytes`: no panic, the work within its budget, the same answer twice, and a drawing that holds the model's caps.
fn whole(bytes: &[u8]) {
    let limits = Limits::default();
    let mut work = ReadWork::default();
    let first = read_counted(bytes, &limits, &mut work);
    assert!(work.total() <= limits.max_read_work, "the work {} is past the budget", work.total());
    let mut again = ReadWork::default();
    let second = read_counted(bytes, &limits, &mut again);
    assert_eq!(work, again, "the same file, two counts");
    match (first, second) {
        (Ok(a), Ok(b)) => {
            assert_eq!(a.doc, b.doc, "the same file, two drawings");
            assert_eq!(a.report, b.report, "the same file, two reports");
            assert!(a.doc.check_limits().is_ok(), "a drawing past the caps of the model");
            assert_eq!(a.doc.pages.len(), a.report.pages as usize, "a page for each page counted");
            assert!(a.report.work.total() <= limits.max_read_work);
        }
        (Err(a), Err(b)) => assert_eq!((a.kind, a.class, a.words), (b.kind, b.class, b.words), "the same file, two refusals"),
        _ => panic!("the same file, a drawing and a refusal"),
    }
}

/// The contract of the reader on `data`; panics when it breaks it.
pub fn check(data: &[u8]) {
    let (mode, body) = data.split_first().map_or((0, &[][..]), |(m, b)| (*m, b));
    match mode % 5 {
        0 => {
            let _ = text_stage(&text_of(body));
        }
        1 => {
            let xml = text_of(body);
            let result = text_stage(&encode(&xml));
            if xml.len() <= ROUND_TRIP_MAX {
                match result {
                    Ok(stage) => assert_eq!(stage.text, cleaned(&xml), "what the encoder encodes the decoder does not decode"),
                    Err(e) => panic!("a page that no cap and no ratio refuses was refused: {e:?}"),
                }
            }
        }
        2 => {
            let text = text_of(body);
            let _ = decode::base64_forgiving(&text);
            let _ = decode::uri_decode(body);
            let once = cleaned(&text);
            assert_eq!(cleaned(&once), once, "the clean is not idempotent");
            // What is encoded is decoded, byte for byte and character for character.
            assert_eq!(decode::base64_forgiving(&base64(body)).ok().as_deref(), Some(body), "Base64 did not come back");
            assert_eq!(decode::uri_decode(uri_encode(&text).as_bytes()).ok().as_deref(), Some(text.as_str()), "a URI-encoded text did not come back");
        }
        3 => whole(body),
        _ => {
            let escaped = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('"', "&quot;");
            let text = text_of(body);
            whole(format!("<mxfile><diagram name=\"d\">{}</diagram></mxfile>", escaped(&text)).as_bytes());
            whole(format!("<mxfile><diagram name=\"d\">{}</diagram><diagram>{}</diagram></mxfile>", encode(&text), encode(&text)).as_bytes());
        }
    }
}
