//! How the reader recognises a file (the design's §1.9): by its first bytes and never by its name, looking at no more than
//! [`MAX_PROLOG_BYTES`] before the name of the root element, and claiming only what the reader can go on to read.
//!
//! In order: the PNG signature is an image that may hold a drawing (R2 step 1, [`Kind::Png`]). Otherwise the bytes are text (UTF-8 with or without a byte order mark,
//! UTF-16 with its mark, or without one when it begins `<?` as XML 1.0 appendix F says), and what the sniff reads of it is the **prolog**: white space, an XML declaration or
//! another instruction, comments, and a DOCTYPE, whose name is the root's by the rule of XML. The name of the root decides: `mxfile` is a file ([`Kind::Document`]),
//! `mxGraphModel` and `diagram` are a page with no wrapper ([`Kind::Page`]), `svg` is an image that may hold a drawing ([`Kind::Svg`]) and `mxlibrary` is a library
//! of shapes ([`Kind::Library`], which the read refuses in words). Any other root, a ZIP (a VSDX, an `.odg` or a `.diacraft` file: none begins with a root) and a file whose
//! root comes after the window are not claimed (`None`), so a file is never read as a drawing because it was not one, and the engine goes on to the next reader.
//!
//! The scan allocates nothing in proportion to the file, looks at each byte once, and says how many bytes it looked at ([`sniff_counted`]), so that a test holds it to the
//! window.

use std::borrow::Cow;

use crate::limits::MAX_PROLOG_BYTES;

/// The eight bytes every PNG image begins with.
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// What a file that the sniff claims may be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// An `mxfile`: pages in `diagram` elements.
    Document,
    /// A bare `mxGraphModel` or `diagram`, read as one page.
    Page,
    /// An SVG image that may hold a drawing in its `content` (read).
    Svg,
    /// A PNG image that may hold a drawing in a text chunk (read).
    Png,
    /// An `mxlibrary`: a library of shapes, which is no drawing. It is claimed so that the read can refuse it in words (`drawio_library_not_drawing`) and not leave it to say
    /// only that it is no VSDX file.
    Library,
}

/// What `bytes` are, by their first bytes: the kind of file the reader may read them as, or `None` when they are none.
pub fn sniff(bytes: &[u8]) -> Option<Kind> {
    sniff_counted(bytes).0
}

/// [`sniff`], and how many bytes of `bytes` it looked at, which is never more than [`MAX_PROLOG_BYTES`].
pub fn sniff_counted(bytes: &[u8]) -> (Option<Kind>, u64) {
    if bytes.starts_with(&PNG_SIGNATURE) {
        return (Some(Kind::Png), PNG_SIGNATURE.len() as u64);
    }
    let window = bytes.get(..bytes.len().min(MAX_PROLOG_BYTES)).unwrap_or_default();
    let Some((text, skipped)) = head_text(window) else { return (None, window.len().min(4) as u64) };
    let (kind, examined) = scan(&text);
    // A head that was made of UTF-16 was all looked at to make it; a UTF-8 one is looked at as far as the scan read.
    let looked = if matches!(text, Cow::Borrowed(_)) { skipped.saturating_add(examined) } else { window.len() };
    (kind, u64::try_from(looked.min(window.len())).unwrap_or(0))
}

/// The head of a file as UTF-8 text, and the bytes of a byte order mark that were skipped to reach it. `None` for bytes in an encoding this reader has none of (UTF-32).
/// A UTF-16 head is made into UTF-8, its characters that are none replaced, a half unit at the end left out.
pub(crate) fn head_text(window: &[u8]) -> Option<(Cow<'_, [u8]>, usize)> {
    if window.starts_with(&[0xFF, 0xFE, 0x00, 0x00]) || window.starts_with(&[0x00, 0x00, 0xFE, 0xFF]) {
        return None;
    }
    if let Some(body) = window.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return Some((Cow::Borrowed(body), 3));
    }
    let utf16 = |body: &[u8], little: bool| -> Cow<'_, [u8]> {
        let (pairs, _half) = body.as_chunks::<2>();
        let units = pairs.iter().map(|p| if little { u16::from_le_bytes(*p) } else { u16::from_be_bytes(*p) });
        let text: String = char::decode_utf16(units).map(|r| r.unwrap_or('\u{FFFD}')).collect();
        Cow::Owned(text.into_bytes())
    };
    if let Some(body) = window.strip_prefix(&[0xFF, 0xFE]) {
        return Some((utf16(body, true), 2));
    }
    if let Some(body) = window.strip_prefix(&[0xFE, 0xFF]) {
        return Some((utf16(body, false), 2));
    }
    if window.starts_with(&[b'<', 0, b'?', 0]) {
        return Some((utf16(window, true), 0));
    }
    if window.starts_with(&[0, b'<', 0, b'?']) {
        return Some((utf16(window, false), 0));
    }
    Some((Cow::Borrowed(window), 0))
}

fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\r' | b'\n')
}

/// The place of `needle` in `hay` at or after `from`.
fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    let tail = hay.get(from..)?;
    tail.windows(needle.len()).position(|w| w == needle).map(|p| p + from)
}

/// The kind a root's name stands for.
fn kind_of(name: &[u8]) -> Option<Kind> {
    match name {
        b"mxfile" => Some(Kind::Document),
        b"mxGraphModel" | b"diagram" => Some(Kind::Page),
        b"svg" => Some(Kind::Svg),
        b"mxlibrary" => Some(Kind::Library),
        _ => None,
    }
}

/// The longest name a root may have for this scan to name it: the longest of the five is twelve.
const NAME_MAX: usize = 16;

/// The prolog of `b` (UTF-8 text), and the name of the root that ends it: the kind it stands for, and how many bytes were examined.
fn scan(b: &[u8]) -> (Option<Kind>, usize) {
    let mut i = 0usize;
    loop {
        while b.get(i).is_some_and(|c| is_space(*c)) {
            i += 1;
        }
        let Some(rest) = b.get(i..).filter(|r| !r.is_empty()) else { return (None, b.len()) };
        if rest.first() != Some(&b'<') {
            return (None, i.saturating_add(1).min(b.len()));
        }
        if rest.starts_with(b"<?") {
            // The XML declaration, or any other instruction: skipped to its end.
            match find(rest, b"?>", 2) {
                Some(end) => i += end + 2,
                None => return (None, b.len()),
            }
        } else if rest.starts_with(b"<!--") {
            match find(rest, b"-->", 4) {
                Some(end) => i += end + 3,
                None => return (None, b.len()),
            }
        } else if let Some(after) = rest.strip_prefix(b"<!DOCTYPE") {
            // The name of a DOCTYPE is the name of the root: the file is claimed by it, and the read refuses what a DOCTYPE holds.
            let skipped = after.iter().take_while(|c| is_space(**c)).count();
            if skipped == 0 {
                return (None, i.saturating_add(b"<!DOCTYPE".len()).saturating_add(1).min(b.len()));
            }
            let start = b"<!DOCTYPE".len() + skipped;
            return name_at(b, i, rest, start, |c| is_space(c) || c == b'[' || c == b'>');
        } else {
            return name_at(b, i, rest, 1, |c| is_space(c) || c == b'/' || c == b'>');
        }
    }
}

/// The name that starts `start` bytes into `rest` (which begins at `at` in `b`), up to the first byte `ends` says ends it, and the kind it stands for.
fn name_at(b: &[u8], at: usize, rest: &[u8], start: usize, ends: impl Fn(u8) -> bool) -> (Option<Kind>, usize) {
    let tail = rest.get(start..).unwrap_or_default();
    let first_ok = tail.first().is_some_and(|c| c.is_ascii_alphabetic() || *c == b'_' || *c == b':');
    let len = tail.iter().take(NAME_MAX + 1).take_while(|c| !ends(**c)).count();
    let examined = at.saturating_add(start).saturating_add(len).saturating_add(1).min(b.len());
    // A name with no end inside the window, or longer than the five names, or that does not start like a name, is none of them.
    if !first_ok || len > NAME_MAX || tail.get(len).is_none() {
        return (None, examined);
    }
    (tail.get(..len).and_then(kind_of), examined)
}

/// The encoding the XML declaration at the start of `window` declares, lower-cased and trimmed: `None` when the file starts with no declaration or its declaration states
/// no encoding. Only a declaration at the very start (after a byte order mark) is one.
pub(crate) fn declared_encoding(bytes: &[u8]) -> Option<String> {
    let window = bytes.get(..bytes.len().min(MAX_PROLOG_BYTES)).unwrap_or_default();
    let (text, _) = head_text(window)?;
    let rest = text.strip_prefix(b"<?xml")?;
    if !rest.first().is_some_and(|c| is_space(*c)) {
        return None;
    }
    let end = find(rest, b"?>", 0)?;
    let decl = rest.get(..end)?;
    let at = find(decl, b"encoding", 0)? + b"encoding".len();
    let after = decl.get(at..)?;
    let after = after.get(after.iter().take_while(|c| is_space(**c)).count()..)?;
    let after = after.strip_prefix(b"=")?;
    let after = after.get(after.iter().take_while(|c| is_space(**c)).count()..)?;
    let quote = *after.first().filter(|q| **q == b'"' || **q == b'\'')?;
    let value = after.get(1..)?;
    let value = value.get(..value.iter().position(|c| *c == quote)?)?;
    Some(String::from_utf8_lossy(value).trim().to_owned())
}

/// Whether an encoding a file declares is one the reader reads: UTF-8, UTF-16 and US-ASCII, whatever the case.
pub(crate) fn encoding_supported(declared: &str) -> bool {
    ["utf-8", "utf-16", "utf-16le", "utf-16be", "us-ascii"].iter().any(|name| declared.eq_ignore_ascii_case(name))
}
