//! The compressed page and the walk of a file to its pages (the design's §2).
//!
//! **A compressed page** is the page's `mxGraphModel` XML, URI-encoded, compressed with raw DEFLATE and written in Base64, and the reader reverses the steps:
//!
//! 1. [`base64_forgiving`]: the rule browsers use (WHATWG's forgiving-base64 decode): ASCII white space anywhere is removed, one or two trailing `=` are removed when the
//!    length is then a multiple of four, a length that leaves one over four fails, a character outside the alphabet fails. Line breaks and missing padding are fine.
//! 2. The inflate of the container ([`diacraft_container::inflate`]): raw DEFLATE, held to the page's cap and the file's room and to the ratio, and, when it fails behind a
//!    valid zlib header, zlib (`drawio_page_zlib_wrapped`).
//! 3. [`uri_decode`]: JavaScript's `decodeURIComponent`: `%XX` is a byte, runs of them are UTF-8, anything else stands for itself. When the inflated text begins with `<`
//!    this step is skipped (`drawio_page_not_uri_encoded`): URI-decoding XML that was never encoded would change every `%` sequence in its labels.
//! 4. [`clean`]: the characters XML cannot hold (below U+0020 but tab, line feed and carriage return, and U+FFFE and U+FFFF) are removed, as draw.io does after every
//!    decompression. (An unpaired surrogate, which that rule removes too, cannot be in a Rust string: bytes that would make one are bytes that are not UTF-8, and fail step 3.)
//! 5. The parse, with the container's XML, whose root must be `mxGraphModel`.
//!
//! A page that fails step 1, 2, 3 or 5 is **empty, with a warning that names the page and the step** (`drawio_page_undecodable`), and the other pages are read; a
//! page that passes a cap, or holds a document type, ends the whole open: a page cut off would be a silent loss. The stages are public, each on its own, so that
//! the vectors of the specification can hold each to its text. Every loop charges its units to the read's work before it runs ([`Reading::charge`]), but the inflate, whose
//! output is bounded by its own cap and ratio and not known beforehand, which is charged for what it made.
//!
//! **The walk of a file** (`walk`) reads an `mxfile` one `diagram` at a time through the container's pull (a stream of the root's children, so that what is held is one page's
//! tree and not the file's), gives each page its name by R7 and its model, and hands it on. What a page holds is the reader of the cells'.

use std::borrow::Cow;
use std::collections::HashSet;

use diacraft_container::Work;
use diacraft_container::inflate::{self, InflateError};
use diacraft_container::xml::{El, Node, ParseOptions, PullLimits, XmlError, XmlText, parse_with_options};

use crate::error::{DrawioError, ErrorKind, class, size_words};
use crate::report::{Form, NAME_CHARS, Reading, Unit};
use crate::sniff::Kind;

/// The class words of the warnings of this stage and of the frame.
pub mod warn {
    /// A child of `mxfile` that is not a `diagram`.
    pub const UNKNOWN_ELEMENT: &str = "drawio_unknown_element";
    /// An `mxfile` with no `diagram`.
    pub const NO_PAGES: &str = "drawio_no_pages";
    /// A page id that an earlier page has.
    pub const DUPLICATE_PAGE_ID: &str = "drawio_duplicate_page_id";
    /// A page with text and a child element: the text wins.
    pub const PAGE_TEXT_AND_MODEL: &str = "drawio_page_text_and_model";
    /// A page whose first child element is not a model.
    pub const PAGE_UNREADABLE: &str = "drawio_page_unreadable";
    /// A compressed page that was never URI-encoded.
    pub const PAGE_NOT_URI_ENCODED: &str = "drawio_page_not_uri_encoded";
    /// A compressed page that is a zlib stream.
    pub const PAGE_ZLIB_WRAPPED: &str = "drawio_page_zlib_wrapped";
    /// A compressed page that fails a step.
    pub const PAGE_UNDECODABLE: &str = "drawio_page_undecodable";
}

/// A step of R11 that a page can fail, numbered as the rule numbers it (the clean, step 4, cannot fail).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Step 1: the text is not Base64.
    Base64,
    /// Step 2: the bytes are not a DEFLATE stream.
    Inflate,
    /// Step 3: a `%` that is no escape, or bytes that are not UTF-8.
    UriDecode,
    /// Step 5: the text is not XML, or its root is not `mxGraphModel`.
    Parse,
}

impl Step {
    /// The step's number in R11.
    pub fn number(self) -> u8 {
        match self {
            Step::Base64 => 1,
            Step::Inflate => 2,
            Step::UriDecode => 3,
            Step::Parse => 5,
        }
    }

    /// The step in words.
    pub fn words(self) -> &'static str {
        match self {
            Step::Base64 => "Base64",
            Step::Inflate => "inflate",
            Step::UriDecode => "URI decoding",
            Step::Parse => "XML",
        }
    }
}

/// Why a step of the pipeline gave no bytes or text: the input is not what the step reads, or memory ran out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepError {
    /// The input is not what the step reads.
    Invalid,
    /// Memory ran out for the output: reported, not an abort.
    OutOfMemory,
}

/// Why a compressed page gave no model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fail {
    /// The page fails a step: it is empty, with a warning, and the file goes on.
    Undecodable {
        /// The step that failed.
        step: Step,
        /// What was wrong, in words.
        why: &'static str,
    },
    /// The page passes a cap or holds a document type: the open ends.
    Refused(DrawioError),
}

impl From<DrawioError> for Fail {
    fn from(e: DrawioError) -> Fail {
        Fail::Refused(e)
    }
}

// ------------------------------------------------------------------------------------------------------------------------------------------------------------ Base64

/// ASCII white space, as WHATWG's Infra standard has it: tab, line feed, form feed, carriage return and space.
fn is_ascii_ws(c: u8) -> bool {
    matches!(c, b'\t' | b'\n' | 0x0C | b'\r' | b' ')
}

/// `text` without the white space JavaScript's `trim` removes at its ends (Unicode white space and the byte order mark).
pub fn trim_js(text: &str) -> &str {
    text.trim_matches(|c: char| c.is_whitespace() || c == '\u{FEFF}')
}

/// The value of a Base64 character of the standard alphabet (`A-Z a-z 0-9 + /`), or `None` for any other byte.
pub fn b64_value(c: u8) -> Option<u8> {
    match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// Whether `n` Base64 characters (white space and padding already removed) can be a text: not when one is left over a multiple of four (WHATWG, step 3).
pub fn b64_length_ok(n: usize) -> bool {
    n % 4 != 1
}

/// The bytes of a group of `n` Base64 values (`q` holds them, six bits each, the rest of it unused): 3 bytes for 4 values, 2 for 3, 1 for 2, none for 0, and `None` for 1 or
/// more than 4. The bytes that are not made are 0. The leftover bits of a group of 2 or 3 are dropped, as the forgiving decode does (WHATWG, step 11).
pub fn b64_quad(q: [u8; 4], n: usize) -> Option<([u8; 3], usize)> {
    let [a, b, c, d] = q.map(|v| u32::from(v & 0x3F));
    let word = a << 18 | b << 12 | c << 6 | d;
    let [_, b0, b1, b2] = word.to_be_bytes();
    match n {
        0 => Some(([0, 0, 0], 0)),
        2 => Some(([b0, 0, 0], 1)),
        3 => Some(([b0, b1, 0], 2)),
        4 => Some(([b0, b1, b2], 3)),
        _ => None,
    }
}

/// `text` decoded by the forgiving rule browsers use (R11 step 1; WHATWG's forgiving-base64 decode, which `atob` follows): the text is trimmed, ASCII white space anywhere
/// in it is removed, one or two `=` at the end are removed when the length is then a multiple of four, a length that leaves one over four fails, and so does a character
/// outside the alphabet (an `=` that is not at the end among them). One pass, with an output of the size of what it makes.
pub fn base64_forgiving(text: &str) -> Result<Vec<u8>, StepError> {
    let bytes = trim_js(text).as_bytes();
    let mut out: Vec<u8> = Vec::new();
    out.try_reserve_exact(bytes.len() / 4 * 3 + 3).map_err(|_| StepError::OutOfMemory)?;
    let (mut group, mut k, mut values, mut pad) = ([0u8; 4], 0usize, 0usize, 0usize);
    for &c in bytes {
        if is_ascii_ws(c) {
            continue;
        }
        if c == b'=' {
            pad += 1;
            continue;
        }
        // A character after an `=`: that `=` was no padding.
        let value = if pad > 0 { None } else { b64_value(c) }.ok_or(StepError::Invalid)?;
        values += 1;
        if let Some(slot) = group.get_mut(k) {
            *slot = value;
        }
        k += 1;
        if k == 4 {
            let (made, n) = b64_quad(group, 4).ok_or(StepError::Invalid)?;
            out.extend_from_slice(made.get(..n).ok_or(StepError::Invalid)?);
            (group, k) = ([0u8; 4], 0);
        }
    }
    // Padding is removed only when the length with it is a multiple of four, and at most two: the length that is left has to be possible.
    if pad > 0 && (pad > 2 || (values + pad) % 4 != 0) {
        return Err(StepError::Invalid);
    }
    if !b64_length_ok(values) {
        return Err(StepError::Invalid);
    }
    if k > 0 {
        let (made, n) = b64_quad(group, k).ok_or(StepError::Invalid)?;
        out.extend_from_slice(made.get(..n).ok_or(StepError::Invalid)?);
    }
    Ok(out)
}

// ------------------------------------------------------------------------------------------------------------------------------------------------------------ URI

/// The value of a hexadecimal digit, either case.
pub fn hex_digit(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// The byte two hexadecimal digits write.
pub fn percent_byte(a: u8, b: u8) -> Option<u8> {
    Some(hex_digit(a)? << 4 | hex_digit(b)?)
}

/// How many bytes a UTF-8 sequence that begins with `lead` has, by the rule `decodeURIComponent` uses (ECMAScript, Decode): the number of leading one bits, and none for a
/// continuation byte (one leading one) or more than four. Whether the sequence is UTF-8 is the check made after, of the whole.
pub fn utf8_len(lead: u8) -> Option<usize> {
    match lead {
        0x00..=0x7F => Some(1),
        0xC0..=0xDF => Some(2),
        0xE0..=0xEF => Some(3),
        0xF0..=0xF7 => Some(4),
        _ => None,
    }
}

/// The byte a `%XX` at `at` writes, or `None` when what is there is no escape.
fn escape_at(bytes: &[u8], at: usize) -> Option<u8> {
    if *bytes.get(at)? != b'%' {
        return None;
    }
    percent_byte(*bytes.get(at + 1)?, *bytes.get(at + 2)?)
}

/// `bytes` decoded as `decodeURIComponent` decodes a string (R11 step 3): each `%` followed by two hexadecimal digits is a byte, the bytes of a multi-byte character are
/// `%XX` each and follow one another, and anything else stands for itself (a `+` stays a `+`). A `%` that is no escape, a multi-byte character that is cut or whose
/// continuation is not one, and a result that is not UTF-8 (overlong forms, surrogates, past U+10FFFF) fail. One pass, with an output of at most the size of the input.
pub fn uri_decode(bytes: &[u8]) -> Result<String, StepError> {
    let mut out: Vec<u8> = Vec::new();
    out.try_reserve_exact(bytes.len()).map_err(|_| StepError::OutOfMemory)?;
    let mut i = 0usize;
    while let Some(&b) = bytes.get(i) {
        if b != b'%' {
            out.push(b);
            i += 1;
            continue;
        }
        let first = escape_at(bytes, i).ok_or(StepError::Invalid)?;
        i += 3;
        out.push(first);
        let n = utf8_len(first).ok_or(StepError::Invalid)?;
        for _ in 1..n {
            let next = escape_at(bytes, i).ok_or(StepError::Invalid)?;
            if next & 0xC0 != 0x80 {
                return Err(StepError::Invalid);
            }
            out.push(next);
            i += 3;
        }
    }
    String::from_utf8(out).map_err(|_| StepError::Invalid)
}

// ------------------------------------------------------------------------------------------------------------------------------------------------------------ the clean

/// Whether a character may stay in the XML of a page (R11 step 4): not a control below U+0020 other than tab, line feed and carriage return, and not U+FFFE or U+FFFF.
pub fn keep_char(c: char) -> bool {
    !matches!(c, '\u{0}'..='\u{8}' | '\u{B}' | '\u{C}' | '\u{E}'..='\u{1F}' | '\u{FFFE}' | '\u{FFFF}')
}

/// `text` as the clean leaves it (R11 step 4): borrowed when it holds nothing to remove.
pub fn clean(text: &str) -> Result<Cow<'_, str>, StepError> {
    if text.chars().all(keep_char) {
        return Ok(Cow::Borrowed(text));
    }
    let mut out = String::new();
    out.try_reserve_exact(text.len()).map_err(|_| StepError::OutOfMemory)?;
    out.extend(text.chars().filter(|c| keep_char(*c)));
    Ok(Cow::Owned(out))
}

/// The same as [`clean`] for text that is owned: the text itself when it holds nothing to remove.
fn clean_owned(text: Cow<'_, str>) -> Result<String, StepError> {
    match clean(&text)? {
        Cow::Borrowed(_) => Ok(text.into_owned()),
        Cow::Owned(cleaned) => Ok(cleaned),
    }
}

// ------------------------------------------------------------------------------------------------------------------------------------------------------------ the pipeline

/// What the steps 1 and 2 of a compressed page give: its inflated bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inflated {
    /// The inflated bytes: the page's URI-encoded text.
    pub bytes: Vec<u8>,
    /// The stream had a zlib header and was read as zlib.
    pub zlib_wrapped: bool,
}

/// What the steps 1 to 4 of a compressed page give: its XML text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextStage {
    /// The page's XML, URI-decoded and cleaned.
    pub text: String,
    /// The inflated text began with `<`, so the URI step was skipped.
    pub not_uri_encoded: bool,
    /// The stream had a zlib header and was read as zlib.
    pub zlib_wrapped: bool,
}

/// How the page being decoded is named in a sentence.
fn the_page(reading: &Reading<'_>) -> String {
    if reading.page.is_empty() { "A page".to_owned() } else { format!("The page \"{}\"", reading.page) }
}

/// Steps 1 and 2 of a compressed page: `text` as Base64 and then as DEFLATE, within the page's cap (the file's room, when that is less) and the ratio.
pub fn inflate_page(text: &str, reading: &mut Reading<'_>) -> Result<Inflated, Fail> {
    let trimmed = trim_js(text);
    reading.charge(Unit::Base64Chars, trimmed.len() as u64)?;
    let bytes = match base64_forgiving(trimmed) {
        Ok(bytes) => bytes,
        Err(StepError::Invalid) => return Err(Fail::Undecodable { step: Step::Base64, why: "the text is not Base64" }),
        Err(StepError::OutOfMemory) => return Err(DrawioError::out_of_memory().into()),
    };
    let limits = reading.limits;
    let room = reading.file_text_left();
    let cap = limits.max_page_text.min(room);
    let held = inflate::Limits { max_out: cap, ratio: Some((limits.ratio_floor, limits.max_ratio)) };
    let mut work = Work::default();
    let first = inflate::raw(&bytes, &held, &mut work);
    // When raw inflation fails behind a valid zlib header, the stream is zlib.
    let wrapped =
        matches!(first, Err(InflateError::Damaged)) && bytes.first().zip(bytes.get(1)).is_some_and(|(b0, b1)| inflate::zlib_header(*b0, *b1));
    let (result, zlib_wrapped) = if wrapped { (inflate::zlib(&bytes, &held, &mut work), true) } else { (first, false) };
    drop(bytes);
    // What both attempts made is the work, whether the page is read or refused.
    reading.charge(Unit::Inflated, work.inflated)?;
    match result {
        Ok(inflated) => {
            if !reading.spend_file_text(inflated.len() as u64) {
                return Err(too_large_pages(limits.max_file_text, &reading.page).into());
            }
            Ok(Inflated { bytes: inflated, zlib_wrapped })
        }
        Err(InflateError::TooLarge { .. }) if cap < limits.max_page_text => Err(too_large_pages(limits.max_file_text, &reading.page).into()),
        Err(InflateError::TooLarge { max }) => {
            Err(DrawioError::too_large(format!("{} is larger than {} once it is decoded.", the_page(reading), size_words(max))).into())
        }
        Err(InflateError::Ratio) => Err(DrawioError::new(
            ErrorKind::TooBig,
            class::RATIO_EXCEEDED,
            format!("{} expands to more than {} times its stored size, which no real page does.", the_page(reading), limits.max_ratio),
        )
        .into()),
        Err(InflateError::Damaged) => Err(Fail::Undecodable { step: Step::Inflate, why: "the data is not a DEFLATE stream" }),
        Err(InflateError::OutOfMemory) => Err(DrawioError::out_of_memory().into()),
    }
}

/// The refusal for the pages of a file that pass their cap together, at the page that does it when it is named (R14 names the page).
fn too_large_pages(cap: u64, page: &str) -> DrawioError {
    let at = if page.is_empty() { String::new() } else { format!(", at the page \"{page}\"") };
    DrawioError::too_large(format!("The pages of the file are larger than {} together once they are decoded{at}.", size_words(cap)))
}

/// Steps 3 and 4 of a compressed page: the inflated bytes URI-decoded, unless they begin with `<`, and cleaned.
pub fn decode_inflated(bytes: &[u8], reading: &mut Reading<'_>) -> Result<TextStage, Fail> {
    let not_uri_encoded = bytes.iter().copied().find(|b| !is_ascii_ws(*b)) == Some(b'<');
    let decoded: Cow<'_, str> = if not_uri_encoded {
        // Never URI-encoded: the bytes are the text, if they are text.
        match std::str::from_utf8(bytes) {
            Ok(text) => Cow::Borrowed(text),
            Err(_) => return Err(Fail::Undecodable { step: Step::Parse, why: "the text is not UTF-8" }),
        }
    } else {
        reading.charge(Unit::UriBytes, bytes.len() as u64)?;
        match uri_decode(bytes) {
            Ok(text) => Cow::Owned(text),
            Err(StepError::Invalid) => return Err(Fail::Undecodable { step: Step::UriDecode, why: "the text is not URI-encoded UTF-8" }),
            Err(StepError::OutOfMemory) => return Err(DrawioError::out_of_memory().into()),
        }
    };
    reading.charge(Unit::CleanBytes, decoded.len() as u64)?;
    let text = clean_owned(decoded).map_err(|_| Fail::Refused(DrawioError::out_of_memory()))?;
    Ok(TextStage { text, not_uri_encoded, zlib_wrapped: false })
}

/// Steps 1 to 4 of a compressed page: the XML text of the page.
pub fn compressed_text(text: &str, reading: &mut Reading<'_>) -> Result<TextStage, Fail> {
    let inflated = inflate_page(text, reading)?;
    let mut stage = decode_inflated(&inflated.bytes, reading)?;
    stage.zlib_wrapped = inflated.zlib_wrapped;
    Ok(stage)
}

/// What a refusal of the XML parser is, as the open's refusal.
pub(crate) fn xml_refusal(e: &XmlError) -> DrawioError {
    match e {
        XmlError::Malformed { offset, .. } => {
            DrawioError::new(ErrorKind::Damaged, class::XML_MALFORMED, format!("The file is not well-formed XML (at byte {offset})."))
        }
        XmlError::EntityRefused { offset, .. } => DrawioError::new(
            ErrorKind::Refused,
            class::ENTITY_REFUSED,
            format!("The file declares a document type or an entity (at byte {offset}), which DiaCraft does not read."),
        ),
        XmlError::TooDeep { limit } => {
            DrawioError::new(ErrorKind::TooBig, class::TOO_DEEP, format!("The file nests elements deeper than {limit} levels."))
        }
        XmlError::TooManyNodes { limit } => {
            DrawioError::too_complex(format!("A page of the file has more than {limit} items, more than DiaCraft reads."))
        }
        XmlError::AttributeTooLarge { limit } => {
            DrawioError::too_large(format!("A value in the file is longer than {}.", size_words(u64::try_from(*limit).unwrap_or(u64::MAX))))
        }
    }
}

/// A compressed page decoded into its model: the `mxGraphModel` element (steps 1 to 5), or `None` when the page fails a step and is empty, with the warning
/// `drawio_page_undecodable` that names the page and the step. A page that was never URI-encoded or was a zlib stream is read, with the warning that says so. A page past a
/// cap, or with a document type, is the open's refusal.
pub fn compressed_page(page: &str, text: &str, reading: &mut Reading<'_>) -> Result<Option<El>, DrawioError> {
    reading.page = crate::error::shown(page, NAME_CHARS);
    let result = compressed_model(page, text, reading);
    reading.page.clear();
    result
}

fn compressed_model(page: &str, text: &str, reading: &mut Reading<'_>) -> Result<Option<El>, DrawioError> {
    let stage = match compressed_text(text, reading) {
        Ok(stage) => stage,
        Err(Fail::Refused(e)) => return Err(e),
        Err(Fail::Undecodable { step, .. }) => {
            reading.note_step(warn::PAGE_UNDECODABLE, page, step);
            return Ok(None);
        }
    };
    if stage.zlib_wrapped {
        reading.note(warn::PAGE_ZLIB_WRAPPED, page);
    }
    if stage.not_uri_encoded {
        reading.note(warn::PAGE_NOT_URI_ENCODED, page);
    }
    // Step 5.
    reading.charge(Unit::XmlBytes, stage.text.len() as u64)?;
    let mut work = Work::default();
    let parsed = parse_with_options(stage.text.as_bytes(), &reading.limits.xml(), ParseOptions::default(), &mut work);
    reading.charge(Unit::XmlEvents, work.xml_events)?;
    match parsed {
        Ok(root) if root.name == "mxGraphModel" => Ok(Some(root)),
        Ok(_) | Err(XmlError::Malformed { .. }) => {
            reading.note_step(warn::PAGE_UNDECODABLE, page, Step::Parse);
            Ok(None)
        }
        Err(e) => Err(xml_refusal(&e)),
    }
}

// ------------------------------------------------------------------------------------------------------------------------------------------------------------ the pages

/// A page of a file, as the walk finds it: the place, the id and name the file gives it, and its model.
#[derive(Debug)]
pub struct Page {
    /// The page's place among the pages, from 1.
    pub position: usize,
    /// The page's id: the file's, or its place counting from 0.
    pub id: String,
    /// The page's `name` as the file has it; `None` when it has none.
    pub name: Option<String>,
    /// The page's `mxGraphModel` element, or `None` for an empty page (an empty `diagram`, a page that is unreadable or undecodable).
    pub model: Option<El>,
}

/// What the walk of a file leaves for the reader of the cells: the root `mxfile` element without its children, when the file had one (its `vars` are the file's
/// properties).
#[derive(Debug, Default)]
pub struct Walked {
    /// The root's start tag, with its attributes and no children.
    pub head: Option<El>,
}

/// The text a page holds directly: its text and CDATA children, as one string (borrowed when there is one).
fn direct_text(el: &El) -> Cow<'_, str> {
    let mut parts = el.children.iter().filter_map(|n| match n {
        Node::Text(t) | Node::CData(t) => Some(t.as_str()),
        _ => None,
    });
    let Some(first) = parts.next() else { return Cow::Borrowed("") };
    let Some(second) = parts.next() else { return Cow::Borrowed(first) };
    let mut joined = String::with_capacity(first.len() + second.len());
    joined.push_str(first);
    joined.push_str(second);
    for part in parts {
        joined.push_str(part);
    }
    Cow::Owned(joined)
}

/// The page's name in a sentence or a case: its `name`, or `Page-N` for its place.
pub(crate) fn page_label(name: Option<&str>, position: usize) -> String {
    match name {
        Some(name) if !name.trim().is_empty() => name.to_owned(),
        _ => format!("Page-{position}"),
    }
}

/// One `diagram` element as a page. `span` is the bytes of text the element took, for the cap of a plain page.
fn read_diagram(el: El, position: usize, span: u64, ids: &mut HashSet<String>, reading: &mut Reading<'_>) -> Result<Page, DrawioError> {
    let name = el.attr("name").map(str::to_owned);
    let label = page_label(name.as_deref(), position);
    let id = el.attr("id").map_or_else(|| position.saturating_sub(1).to_string(), str::to_owned);
    if !ids.insert(id.clone()) {
        reading.note(warn::DUPLICATE_PAGE_ID, &label);
    }
    let has_text = !trim_js(&direct_text(&el)).is_empty();
    let model = if has_text {
        // Text that is not blank is the compressed form, and wins over a child element.
        if el.elements().next().is_some() {
            reading.note(warn::PAGE_TEXT_AND_MODEL, &label);
        }
        reading.report.form = Form::Compressed;
        let text = direct_text(&el);
        compressed_page(&label, trim_js(&text), reading)?
    } else {
        // The plain form: the first child element is the model, and the page's text is the element's.
        if span > reading.limits.max_page_text {
            reading.page = crate::error::shown(&label, NAME_CHARS);
            let e = DrawioError::too_large(format!("{} is larger than {}.", the_page(reading), size_words(reading.limits.max_page_text)));
            reading.page.clear();
            return Err(e);
        }
        if !reading.spend_file_text(span) {
            return Err(too_large_pages(reading.limits.max_file_text, &crate::error::shown(&label, NAME_CHARS)));
        }
        match el.children.into_iter().find_map(|n| if let Node::El(child) = n { Some(child) } else { None }) {
            None => None,
            Some(model) if model.name == "mxGraphModel" => Some(model),
            Some(_) => {
                reading.note(warn::PAGE_UNREADABLE, &label);
                None
            }
        }
    };
    Ok(Page { position, id, name, model })
}

/// Walks the file in `bytes`, of the kind `kind` ([`Kind::Document`] or [`Kind::Page`]), and hands each page to `page`, one at a time, so that what is held is one page's
/// tree. The reader of the cells is the closure `page` is.
pub(crate) fn walk(
    bytes: &[u8],
    kind: Kind,
    reading: &mut Reading<'_>,
    mut page: impl FnMut(&mut Reading<'_>, Page) -> Result<(), DrawioError>,
) -> Result<Walked, DrawioError> {
    let options = ParseOptions { allow_us_ascii: true, ..ParseOptions::default() };
    if kind != Kind::Document {
        return walk_page(bytes, options, reading, &mut page);
    }
    let text = XmlText::new(bytes).map_err(|e| xml_refusal(&e))?;
    reading.charge(Unit::XmlBytes, text.as_str().len() as u64)?;
    let limits = PullLimits { xml: reading.limits.xml(), max_document_nodes: usize::try_from(reading.limits.max_file_bytes).unwrap_or(usize::MAX) };
    let (mut pull, head) = text.pull(&limits, options).map_err(|e| xml_refusal(&e))?;
    if head.name != "mxfile" {
        return Err(DrawioError::not_a_drawing());
    }
    let mut ids: HashSet<String> = HashSet::new();
    let mut position = 0usize;
    let walked = (|| -> Result<(), DrawioError> {
        loop {
            let before = pull.position();
            let Some(child) = pull.next_child().map_err(|e| xml_refusal(&e))? else { break };
            if child.name != "diagram" {
                reading.note(warn::UNKNOWN_ELEMENT, &child.name);
                continue;
            }
            position += 1;
            if position > reading.limits.max_pages {
                return Err(DrawioError::too_complex(format!("The file has more than {} pages.", reading.limits.max_pages)));
            }
            reading.charge(Unit::Pages, 1)?;
            let span = u64::try_from(pull.position().saturating_sub(before)).unwrap_or(u64::MAX);
            let read = read_diagram(child, position, span, &mut ids, reading)?;
            page(reading, read)?;
        }
        pull.finish().map_err(|e| xml_refusal(&e))
    })();
    // What the parse cost is counted whether the walk read or was refused.
    let counted = reading.charge(Unit::XmlEvents, pull.events());
    walked?;
    counted?;
    Ok(Walked { head: Some(head) })
}

/// A file that is one page: a bare `mxGraphModel`, or a `diagram`.
fn walk_page(
    bytes: &[u8],
    options: ParseOptions,
    reading: &mut Reading<'_>,
    page: &mut impl FnMut(&mut Reading<'_>, Page) -> Result<(), DrawioError>,
) -> Result<Walked, DrawioError> {
    reading.report.form = Form::Page;
    let len = bytes.len() as u64;
    if len > reading.limits.max_page_text {
        return Err(DrawioError::too_large(format!("The page is larger than {}.", size_words(reading.limits.max_page_text))));
    }
    reading.charge(Unit::XmlBytes, len)?;
    let mut work = Work::default();
    let parsed = parse_with_options(bytes, &reading.limits.xml(), options, &mut work);
    let counted = reading.charge(Unit::XmlEvents, work.xml_events);
    let root = parsed.map_err(|e| xml_refusal(&e))?;
    counted?;
    reading.charge(Unit::Pages, 1)?;
    let read = match root.name.as_str() {
        "mxGraphModel" => {
            if !reading.spend_file_text(len) {
                return Err(too_large_pages(reading.limits.max_file_text, ""));
            }
            Page { position: 1, id: "0".to_owned(), name: None, model: Some(root) }
        }
        "diagram" => {
            let mut ids = HashSet::new();
            let read = read_diagram(root, 1, len, &mut ids, reading)?;
            // A bare `diagram` is a page, whatever form its model has.
            reading.report.form = Form::Page;
            read
        }
        _ => return Err(DrawioError::not_a_drawing()),
    };
    page(reading, read)?;
    Ok(Walked::default())
}
