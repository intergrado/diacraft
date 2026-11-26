//! draw.io files built in code: the tests' own encoder, written, so that a reader can be tested from the
//! bytes in and a hostile file can be made as easily as an honest one. Nothing here comes from a file made by another program: the XML is the text a test writes
//! (the spec's vectors are XML fragments), and the builders only supply what each vector leaves out (the root cell and the first layer, the wrapper of a page).
//!
//! * [`encode`] is the three forward steps, [`uri_encode`] (as JavaScript's `encodeURIComponent` does), [`deflate_raw`] (at level 9, the one the reference snippet
//!   uses) and [`base64`] (standard, padded, no line breaks). A test compares the XML a stream decodes to, never the compressed bytes: different DEFLATE encoders give
//!   different bytes.
//! * [`bare_model`], [`plain_page`], [`compressed_page`] and [`mxfile`] make the document: a model of the cells a test gives (with the root cell `0` and the layer
//!   `1` in front of them), a page holding it as an element or as compressed text, and a file of pages.
//! * [`bomb`] makes the stream that no honest writer makes: `n` copies of one byte as raw DEFLATE, a hundred megabytes of them in about a hundred kilobytes, written a
//!   megabyte at a time so that the test never holds what it makes.

// A fixture that cannot be built must fail its test, like an assertion; the shipped-code ban on panicking (AGENTS.md, Never crash) is for
// code that runs for users.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::Write;

use flate2::Compression;
use flate2::write::{DeflateEncoder, ZlibEncoder};

/// The root cell and the first layer: what every model starts with.
pub const ROOT_CELLS: &str = r#"<mxCell id="0"/><mxCell id="1" parent="0"/>"#;

/// `text` as JavaScript's `encodeURIComponent` writes it: every byte of its UTF-8 as `%XX` (upper case) but those of `A-Z a-z 0-9 - _ . ! ~ * ' ( )`.
pub fn uri_encode(text: &str) -> String {
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

/// `data` as raw DEFLATE (no header, no checksum) at `level`.
pub fn deflate_raw(data: &[u8], level: u32) -> Vec<u8> {
    let mut enc = DeflateEncoder::new(Vec::new(), Compression::new(level));
    enc.write_all(data).unwrap();
    enc.finish().unwrap()
}

/// `data` as a zlib stream (a two-byte header, DEFLATE and the Adler-32) at `level`.
pub fn deflate_zlib(data: &[u8], level: u32) -> Vec<u8> {
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::new(level));
    enc.write_all(data).unwrap();
    enc.finish().unwrap()
}

/// `data` as standard Base64: the alphabet `A-Z a-z 0-9 + /`, padded with `=`, in one line.
pub fn base64(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for group in data.chunks(3) {
        let n = group.iter().fold(0u32, |acc, b| acc << 8 | u32::from(*b)) << (8 * (3 - group.len()));
        for k in 0..4 {
            if k <= group.len() {
                out.push(char::from(ALPHABET[(n >> (18 - 6 * k) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The forward steps on the XML of a model: URI-encoded, deflated at level 9 (raw), and written in Base64. This is the text of a compressed page.
pub fn encode(xml: &str) -> String {
    base64(&deflate_raw(uri_encode(xml).as_bytes(), 9))
}

/// `value` as the text of an attribute value written in double quotes.
pub fn escape_attr(value: &str) -> String {
    value.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\n', "&#xa;")
}

/// A model that holds the root cell `0`, the layer `1` and then `cells` (the XML of more cells, written as the test wants them).
pub fn bare_model(cells: &str) -> String {
    format!(
        "<mxGraphModel dx=\"0\" dy=\"0\" grid=\"1\" gridSize=\"10\" page=\"1\" pageScale=\"1\" pageWidth=\"850\" pageHeight=\"1100\"><root>{ROOT_CELLS}{cells}</root></mxGraphModel>"
    )
}

/// A page named `name` that holds its model as an element (the plain form), the model of `cells` (see [`bare_model`]).
pub fn plain_page(name: &str, cells: &str) -> String {
    format!(
        "<diagram id=\"{id}\" name=\"{n}\">{model}</diagram>",
        id = escape_attr(&format!("id-{name}")),
        n = escape_attr(name),
        model = bare_model(cells)
    )
}

/// A page named `name` that holds its model as compressed text (the compressed form), the model of `cells` (see [`bare_model`]).
pub fn compressed_page(name: &str, cells: &str) -> String {
    format!(
        "<diagram id=\"{id}\" name=\"{n}\">{text}</diagram>",
        id = escape_attr(&format!("id-{name}")),
        n = escape_attr(name),
        text = encode(&bare_model(cells))
    )
}

/// A file of `pages` (each a `<diagram>` element, as [`plain_page`] and [`compressed_page`] make them), with the XML declaration of a file draw.io's editor saves.
pub fn mxfile(pages: &[String]) -> String {
    format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<mxfile host=\"app.diagrams.net\" version=\"24.0.0\">{}</mxfile>", pages.concat())
}

/// `n` copies of `byte` as raw DEFLATE, at level 9, which makes the one-bit codes a run of one byte allows: about a thousand to one, so that a hundred megabytes is about a
/// hundred kilobytes. It is written a megabyte at a time, so the test holds the compressed stream and never what it inflates to.
pub fn bomb(byte: u8, n: u64) -> Vec<u8> {
    let block = vec![byte; 1 << 20];
    let mut enc = DeflateEncoder::new(Vec::new(), Compression::new(9));
    let mut left = n;
    while left > 0 {
        let take = usize::try_from(left.min(block.len() as u64)).unwrap();
        enc.write_all(&block[..take]).unwrap();
        left -= take as u64;
    }
    enc.finish().unwrap()
}
