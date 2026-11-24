//! Escapes: how a string the XML cannot hold as it is, or the attribute cap would refuse, and a number, are written and read back exactly.
//!
//! **Strings.** A character XML 1.0 forbids (a control character other than tab, line feed and carriage return, U+FFFE and U+FFFF) is written
//! in base64: the string's UTF-8 bytes in the standard alphabet with padding. The XML reader caps an attribute by the bytes as written, and the
//! writer spells `"` as `&quot;` (six bytes) and a line feed as `&#10;` (five), so a value is held to [`ATTR_MAX_BYTES`] as an attribute, and a
//! longer one goes in the text of an element (which has no cap but the part's).
//!
//! **Numbers.** An integer is its decimal text. An `f64` is Rust's shortest text that reads back as the same bits (`-0.0` is `-0`), and a value
//! that is not finite is a named token: `NaN`, `INF` and `-INF` ([`f64_text`], [`parse_f64`]).

use diacraft_container::xml::first_bad_char;

/// The longest text held as an attribute, in bytes: sixteen times the 1 MiB cap of the XML reader leaves to the escapes the writer spells a byte
/// with (at most six bytes), and a base64 text (four thirds). A longer text is written in an element.
pub const ATTR_MAX_BYTES: usize = 16 * 1024;

/// The most bytes an attribute value may have **as written**: the XML reader's cap ([`diacraft_container::limits::MAX_XML_ATTR_BYTES`]), which it holds
/// the value to as the bytes of the file, escapes and all.
pub const ATTR_WRITTEN_MAX: usize = diacraft_container::limits::MAX_XML_ATTR_BYTES;

/// How many bytes `value` is as the container's writer spells it in an attribute: `&` is `&amp;` (5), `<` is `&lt;` (4), `"` is `&quot;` (6), a line feed and
/// a carriage return are `&#10;` and `&#13;` (5 each), a tab is `&#9;` (4), and any other character is its UTF-8 bytes. It saturates, and stops counting once
/// it is past the cap, so that a value of any size is measured in at most the cap's worth of characters.
pub fn attr_written_len(value: &str) -> usize {
    let mut total = 0usize;
    for c in value.chars() {
        total = total.saturating_add(attr_char_len(c));
        if total > ATTR_WRITTEN_MAX {
            break;
        }
    }
    total
}

/// How many bytes a character is as the container's writer spells it in an attribute.
fn attr_char_len(c: char) -> usize {
    match c {
        '&' => 5,
        '<' => 4,
        '"' => 6,
        '\n' | '\r' => 5,
        '\t' => 4,
        other => other.len_utf8(),
    }
}

/// [`attr_written_len`] without its stop at the reader's cap: the whole length, for the total of a part.
pub fn attr_written_total(value: &str) -> u64 {
    value.chars().map(|c| attr_char_len(c) as u64).sum()
}

/// Whether the container's writer can write a processing instruction of this target and data **as a node of the XML**, which the reader takes more of than the
/// writer gives back: the reader takes a target that is no XML name (`<??>`, `<?1a d?>`), and the writer refuses it, and `xml` in any case.
pub fn pi_writable(target: &str, data: &str) -> bool {
    diacraft_container::xml::is_name(target) && !target.eq_ignore_ascii_case("xml") && !data.contains("?>") && first_bad_char(data).is_none()
}

/// Whether the container's writer can write a comment of this text as a node: no `--`, no dash at the end, no character XML forbids.
pub fn comment_writable(text: &str) -> bool {
    !text.contains("--") && !text.ends_with('-') && first_bad_char(text).is_none()
}

/// Whether `text` has a character the XML reader refuses, so that it is written in base64.
pub fn needs_base64(text: &str) -> bool {
    first_bad_char(text).is_some()
}

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn symbol(six: u32) -> char {
    // `six` is masked to six bits by the callers, so the lookup is in range; an index past the table cannot happen.
    ALPHABET.get(usize::try_from(six & 63).unwrap_or(0)).map_or('A', |b| char::from(*b))
}

/// Base64 of `bytes`: RFC 4648's standard alphabet, padded, no line breaks.
pub fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3).saturating_mul(4));
    for chunk in bytes.chunks(3) {
        let a = u32::from(chunk.first().copied().unwrap_or(0));
        let b = u32::from(chunk.get(1).copied().unwrap_or(0));
        let c = u32::from(chunk.get(2).copied().unwrap_or(0));
        let n = (a << 16) | (b << 8) | c;
        out.push(symbol(n >> 18));
        out.push(symbol(n >> 12));
        out.push(if chunk.len() > 1 { symbol(n >> 6) } else { '=' });
        out.push(if chunk.len() > 2 { symbol(n) } else { '=' });
    }
    out
}

fn value(byte: u8) -> Option<u32> {
    match byte {
        b'A'..=b'Z' => Some(u32::from(byte - b'A')),
        b'a'..=b'z' => Some(u32::from(byte - b'a') + 26),
        b'0'..=b'9' => Some(u32::from(byte - b'0') + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// The bytes of base64 `text`: the standard alphabet, padded, with nothing else in it (no white space, no padding but at the end) and the unused
/// bits of the last symbol zero, so that what is read is what [`base64_encode`] writes. `None` for anything else.
pub fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let (groups, rest) = text.as_bytes().as_chunks::<4>();
    if !rest.is_empty() {
        return None;
    }
    let count = groups.len();
    let mut out: Vec<u8> = Vec::new();
    out.try_reserve(count.saturating_mul(3)).ok()?;
    for (at, group) in groups.iter().enumerate() {
        let [a, b, c, d] = *group;
        let last = at + 1 == count;
        let (a, b) = (value(a)?, value(b)?);
        match (c, d) {
            (b'=', b'=') if last => {
                if b & 0xF != 0 {
                    return None;
                }
                out.push(u8::try_from(((a << 2) | (b >> 4)) & 0xFF).ok()?);
            }
            (c, b'=') if last => {
                let c = value(c)?;
                if c & 0x3 != 0 {
                    return None;
                }
                let n = (a << 10) | (b << 4) | (c >> 2);
                out.push(u8::try_from((n >> 8) & 0xFF).ok()?);
                out.push(u8::try_from(n & 0xFF).ok()?);
            }
            (c, d) => {
                let n = (a << 18) | (b << 12) | (value(c)? << 6) | value(d)?;
                out.push(u8::try_from((n >> 16) & 0xFF).ok()?);
                out.push(u8::try_from((n >> 8) & 0xFF).ok()?);
                out.push(u8::try_from(n & 0xFF).ok()?);
            }
        }
    }
    Some(out)
}

/// The string `text` holds when it is base64 of UTF-8, else `None`.
pub fn base64_string(text: &str) -> Option<String> {
    String::from_utf8(base64_decode(text)?).ok()
}

/// An `f64` as text that reads back as the same bits: Rust's shortest form, in the exponent form where the plain digits would run long, and the
/// tokens `NaN`, `INF` and `-INF` for a value that is not finite.
pub fn f64_text(x: f64) -> String {
    if x.is_nan() {
        "NaN".to_owned()
    } else if x.is_infinite() {
        if x > 0.0 { "INF".to_owned() } else { "-INF".to_owned() }
    } else {
        let magnitude = x.abs();
        if magnitude >= 1e16 || (magnitude != 0.0 && magnitude < 1e-6) { format!("{x:e}") } else { format!("{x}") }
    }
}

/// The `f64` that `text` is: what [`f64_text`] writes, and no other spelling (no `inf`, `nan`, hex, blanks or underscores, which Rust's own reading
/// would take or refuse in ways a file must not depend on).
pub fn parse_f64(text: &str) -> Option<f64> {
    match text {
        "NaN" => return Some(f64::NAN),
        "INF" => return Some(f64::INFINITY),
        "-INF" => return Some(f64::NEG_INFINITY),
        _ => {}
    }
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'+' | b'-' | b'e' | b'E')) {
        return None;
    }
    text.parse::<f64>().ok().filter(|n| n.is_finite())
}
