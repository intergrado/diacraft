//! Character references in an HTML label: `&amp;` and its five named kin, and numeric references (S-R25's first row).
//!
//! Adapted from WordCraft's `entity.rs` (`docs/provenance.md`): the numeric branch of `decode_entity` and the bound on how far past an `&` `unescape` looks for the `;`. What
//! changed: the table of about 230 names is not kept, since S-R25 reads six (`amp`, `lt`, `gt`, `quot`, `apos`, `nbsp`) and leaves any other name as its literal text; the
//! charset sniffing and the decoding of bytes are not kept (the container gives this crate text); a number is read with saturating arithmetic, so that a reference with a hundred
//! digits is a number past the last character and not an overflow; the numbers HTML gives other characters (0x80 to 0x9F are Windows-1252's, a surrogate or a number past
//! U+10FFFF is U+FFFD) are the standard's; and `unescape` borrows the text when it holds no `&` and never copies a character by itself.
//!
//! **A reference needs its `;`**, within [`WINDOW`] bytes of its `&`: one without it, and a name that is not one of the six, is not a reference, and its `&` is a character like
//! any other. A browser reads a few references with no `;`; the label reader does not.

use std::borrow::Cow;

/// How far past an `&` the `;` of a reference is looked for, in bytes: 33, as WordCraft's reader has it. A name is at most 6 letters and a number at most 8 characters
/// (`#x10FFFF`) unless it is padded with zeros, and looking further would rescan the rest of the text for every stray `&`. Each `&` costs at most this many steps, and the label
/// reader charges them before it decodes ([`cost`]).
pub const WINDOW: usize = 33;

/// The character a body of a reference stands for (`amp`, `#38`, `#x26`), or `None` when it is not a reference this reader knows. U+0000 is the character `&#0;` names, and the
/// reader of labels removes it (R11 step 4) with the other controls.
pub fn decode_reference(body: &str) -> Option<char> {
    match body {
        "amp" | "AMP" => Some('&'),
        "lt" | "LT" => Some('<'),
        "gt" | "GT" => Some('>'),
        "quot" | "QUOT" => Some('"'),
        "apos" => Some('\''),
        "nbsp" => Some('\u{A0}'),
        _ => numeric(body.strip_prefix('#')?),
    }
}

/// The digits of a numeric reference (after the `#`), decimal or hexadecimal behind `x` or `X`, as a character; `None` for no digits or a character that is not a digit.
fn numeric(rest: &str) -> Option<char> {
    let (digits, radix) = match rest.strip_prefix(['x', 'X']) {
        Some(hex) => (hex, 16),
        None => (rest, 10),
    };
    if digits.is_empty() {
        return None;
    }
    let mut value: u32 = 0;
    for c in digits.chars() {
        let digit = c.to_digit(radix)?;
        // Saturating: a number with more digits than any character has stays past the last one.
        value = value.saturating_mul(radix).saturating_add(digit);
    }
    Some(numeric_char(value))
}

/// The character a number names as HTML reads a numeric reference: 0x80 to 0x9F are Windows-1252's characters, a surrogate and a number past U+10FFFF are U+FFFD.
pub fn numeric_char(value: u32) -> char {
    match value {
        0x80..=0x9F => windows_1252(value),
        _ => char::from_u32(value).unwrap_or('\u{FFFD}'),
    }
}

/// The character Windows-1252 has for a byte from 0x80 to 0x9F (the WHATWG encoding standard's table: the five bytes it leaves unassigned stay the controls of that number).
fn windows_1252(byte: u32) -> char {
    const TABLE: [char; 32] = [
        '\u{20AC}', '\u{81}', '\u{201A}', '\u{192}', '\u{201E}', '\u{2026}', '\u{2020}', '\u{2021}', '\u{2C6}', '\u{2030}', '\u{160}', '\u{2039}',
        '\u{152}', '\u{8D}', '\u{17D}', '\u{8F}', '\u{90}', '\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}', '\u{2022}', '\u{2013}', '\u{2014}',
        '\u{2DC}', '\u{2122}', '\u{161}', '\u{203A}', '\u{153}', '\u{9D}', '\u{17E}', '\u{178}',
    ];
    usize::try_from(byte.wrapping_sub(0x80)).ok().and_then(|n| TABLE.get(n)).copied().unwrap_or('\u{FFFD}')
}

/// The reference whose `&` is at byte `at` of `raw`: the character, and the byte after its `;`. `None` when `at` is no `&`, there is no `;` within [`WINDOW`] bytes, or the body
/// between them is not a reference this reader knows.
pub fn reference_at(raw: &str, at: usize) -> Option<(char, usize)> {
    let bytes = raw.as_bytes();
    if bytes.get(at) != Some(&b'&') {
        return None;
    }
    let from = at.checked_add(1)?;
    let to = from.checked_add(WINDOW).map_or(bytes.len(), |end| end.min(bytes.len()));
    let semi = bytes.get(from..to)?.iter().position(|b| *b == b';')?;
    let end = from.checked_add(semi)?;
    let c = decode_reference(raw.get(from..end)?)?;
    Some((c, end.checked_add(1)?))
}

/// `raw` with its references replaced by what they stand for: borrowed when it holds no `&`. Linear: every byte is looked at once, and the look ahead of an `&` is [`WINDOW`]
/// bytes at most.
pub fn unescape(raw: &str) -> Cow<'_, str> {
    if !raw.contains('&') {
        return Cow::Borrowed(raw);
    }
    let mut out = String::with_capacity(raw.len());
    let mut at = 0;
    while let Some(rest) = raw.get(at..) {
        let Some(k) = rest.find('&') else {
            out.push_str(rest);
            break;
        };
        let amp = at.saturating_add(k);
        out.push_str(raw.get(at..amp).unwrap_or(""));
        match reference_at(raw, amp) {
            Some((c, next)) => {
                out.push(c);
                at = next;
            }
            None => {
                out.push('&');
                at = amp.saturating_add(1);
            }
        }
    }
    Cow::Owned(out)
}

/// What [`unescape`] of `raw` costs, in the steps the label reader counts: a step for each byte, and [`WINDOW`] for each `&`, which may look that far ahead for its `;`.
pub fn cost(raw: &str) -> u64 {
    let amps = raw.bytes().filter(|b| *b == b'&').count();
    let len = u64::try_from(raw.len()).unwrap_or(u64::MAX);
    let look = u64::try_from(amps).unwrap_or(u64::MAX).saturating_mul(WINDOW as u64);
    len.saturating_add(look)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_six_names_and_numbers_are_read() {
        assert_eq!(unescape("a &amp; b &lt;c&gt; &quot;d&quot; &apos;e&apos; f&nbsp;g"), "a & b <c> \"d\" 'e' f\u{A0}g");
        assert_eq!(unescape("&#65;&#x42;&#X43;&AMP;&LT;&GT;&QUOT;"), "ABC&<>\"");
    }

    #[test]
    fn another_name_a_missing_semicolon_and_a_long_body_stay_as_written() {
        for text in ["&copy;", "&hellip;", "&amp", "&amp ;", "&;", "&#;", "&#x;", "&#xZZ;", "&#12a;", "&", "&&", "a & b", "&AmP;", "&NBSP;", "&APOS;"]
        {
            assert_eq!(unescape(text), text, "{text}");
        }
        let long = format!("&#{};", "0".repeat(40));
        assert_eq!(unescape(&long), long, "a body past the window is not looked for");
        let padded = format!("&#{}65;", "0".repeat(20));
        assert_eq!(unescape(&padded), "A", "zeros inside the window are digits");
    }

    #[test]
    fn a_number_no_character_has_is_the_replacement_character_and_never_overflows() {
        assert_eq!(unescape("&#xD800;&#xDFFF;&#x110000;&#4294967296;&#99999999999999999999;&#xFFFFFFFFFF;"), "\u{FFFD}".repeat(6));
        assert_eq!(unescape("&#x10FFFF;"), "\u{10FFFF}");
        assert_eq!(unescape("&#0;"), "\u{0}", "the reader of labels removes it with the controls");
    }

    #[test]
    fn the_numbers_from_0x80_to_0x9f_are_windows_1252() {
        assert_eq!(unescape("&#128;&#133;&#150;&#151;&#153;&#159;"), "\u{20AC}\u{2026}\u{2013}\u{2014}\u{2122}\u{178}");
        assert_eq!(unescape("&#x81;&#x8D;&#x8F;&#x90;&#x9D;"), "\u{81}\u{8D}\u{8F}\u{90}\u{9D}", "the five bytes it leaves unassigned stay controls");
        assert_eq!(unescape("&#127;&#160;"), "\u{7F}\u{A0}");
    }

    #[test]
    fn every_number_below_a_thousand_and_the_edges_are_a_character() {
        for n in (0..1000u32).chain([0xD7FF, 0xD800, 0xDFFF, 0xE000, 0xFFFE, 0xFFFF, 0x10000, 0x10FFFF, 0x110000, u32::MAX]) {
            let c = numeric_char(n);
            if (0x80..=0x9F).contains(&n) {
                assert_ne!(c, '\u{FFFD}');
            } else if char::from_u32(n).is_none() {
                assert_eq!(c, '\u{FFFD}', "{n:#x}");
            } else {
                assert_eq!(u32::from(c), n);
            }
        }
    }

    #[test]
    fn the_cost_counts_bytes_and_the_look_ahead_of_each_ampersand() {
        assert_eq!(cost(""), 0);
        assert_eq!(cost("abc"), 3);
        assert_eq!(cost("a&b"), 3 + WINDOW as u64);
        assert_eq!(cost("&&&&"), 4 + 4 * WINDOW as u64);
    }

    #[test]
    fn text_with_no_ampersand_is_borrowed_and_multibyte_text_survives() {
        assert!(matches!(unescape("plain \u{e9}\u{4e2d}"), Cow::Borrowed(_)));
        assert_eq!(unescape("\u{e9}&amp;\u{4e2d}&#x1F600;\u{1F600}&"), "\u{e9}&\u{4e2d}\u{1F600}\u{1F600}&");
        // The window ends inside a multibyte character: the body is never sliced there.
        assert_eq!(unescape(&format!("&{}", "\u{e9}".repeat(40))), format!("&{}", "\u{e9}".repeat(40)));
    }
}
