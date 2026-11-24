//! Numbers, colours and text as an SVG file writes them.

use std::fmt::Write as _;

use diacraft_color::Rgba;

/// A number with at most `places` decimals and no trailing zeros; 0 for one that is not a number.
pub(crate) fn num(v: f64, places: usize) -> String {
    if !v.is_finite() {
        return "0".to_owned();
    }
    let mut s = format!("{v:.places$}");
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    if s == "-0" { "0".to_owned() } else { s }
}

/// A coordinate or a length: thousandths of an output unit.
pub(crate) fn coord(v: f64) -> String {
    num(v, 3)
}

/// A transform entry: millionths, so that a turn of a long text line does not drift.
pub(crate) fn entry(v: f64) -> String {
    num(v, 6)
}

/// `matrix(a b c d e f)` for the six coefficients of an affine map.
pub(crate) fn matrix(coeffs: [f64; 6]) -> String {
    let mut s = String::from("matrix(");
    for (i, c) in coeffs.iter().enumerate() {
        if i > 0 {
            s.push(' ');
        }
        s.push_str(&entry(*c));
    }
    s.push(')');
    s
}

/// `#rrggbb`.
pub(crate) fn hex(c: Rgba) -> String {
    let mut s = String::with_capacity(7);
    let _ = write!(s, "#{:02x}{:02x}{:02x}", c.r, c.g, c.b); // Writing to a String does not fail.
    s
}

/// An opacity from an alpha byte: `None` for a fully opaque colour, which needs no attribute.
pub(crate) fn opacity(alpha: u8) -> Option<String> {
    (alpha != 255).then(|| num(f64::from(alpha) / 255.0, 4))
}

/// Text for an attribute value or element content: the five characters XML reserves are escaped, and a character XML 1.0 does not allow
/// (a control character other than tab, line feed and carriage return, U+FFFE and U+FFFF) becomes a space.
pub(crate) fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\t' | '\n' | '\r' => out.push(c),
            c if (c as u32) < 0x20 || c == '\u{FFFE}' || c == '\u{FFFF}' || (0x7F..=0x9F).contains(&(c as u32)) => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

/// Standard base64 with padding.
pub(crate) fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk.first().copied().unwrap_or(0), chunk.get(1).copied().unwrap_or(0), chunk.get(2).copied().unwrap_or(0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        let at = |shift: u32| char::from(ALPHABET.get(((n >> shift) & 63) as usize).copied().unwrap_or(b'A'));
        out.push(at(18));
        out.push(at(12));
        out.push(if chunk.len() > 1 { at(6) } else { '=' });
        out.push(if chunk.len() > 2 { at(0) } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_short_and_never_negative_zero() {
        assert_eq!(num(1.5, 3), "1.5");
        assert_eq!(num(2.0, 3), "2");
        assert_eq!(num(-0.0001, 3), "0");
        assert_eq!(num(1234.56789, 3), "1234.568");
        assert_eq!(num(f64::NAN, 3), "0");
        assert_eq!(num(f64::INFINITY, 3), "0");
        assert_eq!(num(100.0, 0), "100");
    }

    #[test]
    fn base64_matches_the_standard_vectors() {
        // RFC 4648 section 10.
        for (plain, coded) in
            [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foob", "Zm9vYg=="), ("fooba", "Zm9vYmE="), ("foobar", "Zm9vYmFy")]
        {
            assert_eq!(base64(plain.as_bytes()), coded);
        }
    }

    #[test]
    fn text_is_escaped_and_forbidden_characters_become_spaces() {
        assert_eq!(escape("a<b & \"c\" 'd' >"), "a&lt;b &amp; &quot;c&quot; &apos;d&apos; &gt;");
        assert_eq!(escape("x\u{0}y\u{1B}z\u{FFFE}"), "x y z ");
        assert_eq!(escape("tab\there"), "tab\there");
    }
}
