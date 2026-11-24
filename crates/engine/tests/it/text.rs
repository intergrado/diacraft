//! `shown`: the one way text from a client reaches a message, a log line or a status bar.

use diacraft_engine::shown;

#[test]
fn control_characters_come_out_escaped() {
    assert_eq!(shown("a\nb\rc\td\0e", 100), "a\\nb\\rc\\td\\0e");
    assert_eq!(shown("\u{1b}[2J", 100), "\\u{1b}[2J");
    // The C0 range, DEL and the C1 range (U+0085 is a line break to some terminals).
    for c in (0_u32..0x20).chain(0x7f..0xa0).filter_map(char::from_u32) {
        let out = shown(&format!("x{c}y"), 100);
        assert!(!out.chars().any(char::is_control), "U+{:04X} reached the output raw: {out:?}", c as u32);
        assert!(out.starts_with('x') && out.ends_with('y') && out.len() > 3, "U+{:04X}: {out:?}", c as u32);
    }
}

#[test]
fn direction_controls_come_out_escaped() {
    let bidi = [
        '\u{061c}', '\u{200e}', '\u{200f}', '\u{202a}', '\u{202b}', '\u{202c}', '\u{202d}', '\u{202e}', '\u{2066}', '\u{2067}', '\u{2068}',
        '\u{2069}',
    ];
    for c in bidi {
        let out = shown(&format!("a{c}b"), 100);
        assert_eq!(out, format!("a\\u{{{:x}}}b", c as u32), "U+{:04X}", c as u32);
    }
    assert_eq!(shown("rtl\u{202e}spoof", 100), "rtl\\u{202e}spoof");
}

/// U+2028 and U+2029 are not control characters (general categories Zl and Zp, not Cc), but they end a line for
/// anything that follows Unicode's line-break rules, so client text that holds one could fake a second line in a log
/// or a status bar. `escape_debug`, which the engine used before `shown`, escaped them.
#[test]
fn line_and_paragraph_separators_come_out_escaped() {
    assert_eq!(shown("first\u{2028}second", 100), "first\\u{2028}second");
    assert_eq!(shown("first\u{2029}second", 100), "first\\u{2029}second");
    // Every mandatory line break of UAX #14 (BK, CR, LF, NL): none reaches the output raw.
    for c in ['\u{a}', '\u{b}', '\u{c}', '\u{d}', '\u{85}', '\u{2028}', '\u{2029}'] {
        let out = shown(&format!("INFO one line{c}ERROR a second, faked line"), 100);
        assert!(!out.contains(c), "U+{:04X} reached the output raw: {out:?}", c as u32);
        assert!(out.contains("one line\\") && out.contains("faked line"), "U+{:04X}: {out:?}", c as u32);
    }
}

/// Nothing but control, line-separator and direction characters is escaped. Text people write is shown as it is:
/// apostrophes, quotes, backslashes (a Windows path), accents built from combining marks, the joiner inside an emoji,
/// other scripts.
#[test]
fn everything_else_is_shown_as_it_is() {
    for text in [
        "John's plan",
        "say \"hi\"",
        "C:\\Users\\me\\drawing.vsdx",
        "cafe\u{301}",
        "caf\u{e9}",
        "\u{1f469}\u{200d}\u{1f4bb}",
        "a\u{200c}b",
        "\u{65e5}\u{672c}\u{8a9e}",
        "\u{3b1}\u{3b2}\u{3b3}",
        "page \u{b7} 2",
        "tab-free text with spaces",
    ] {
        assert_eq!(shown(text, 100), text);
    }
}

#[test]
fn the_limit_counts_the_escaped_text_and_ends_with_an_ellipsis() {
    assert_eq!(shown("abcdef", 6), "abcdef", "exactly the limit is not cut");
    assert_eq!(shown("abcdefg", 6), "abcdef\u{2026}");
    assert_eq!(shown("", 6), "");
    assert_eq!(shown("abc", 0), "\u{2026}");
    assert_eq!(shown("", 0), "");
    // An escape is whole or not there: `\u{1b}` is 6 characters, so 5 do not hold it.
    assert_eq!(shown("\u{1b}", 6), "\\u{1b}");
    assert_eq!(shown("\u{1b}", 5), "\u{2026}");
    assert_eq!(shown("ab\u{1b}cd", 7), "ab\u{2026}");
    let long = shown(&"\u{1b}".repeat(100), 64);
    assert!(long.ends_with('\u{2026}'), "{long}");
    assert!(!long.trim_end_matches('\u{2026}').replace("\\u{1b}", "").contains('\\'), "an escape was cut: {long}");
    assert!(long.chars().count() <= 65);
}

/// A very long text costs only the characters that are shown: counted in the unit test of `text.rs` (the characters looked at), not timed here.
#[test]
fn a_huge_text_is_cut_to_what_is_shown() {
    let huge = "x".repeat(64 * 1024 * 1024);
    let out = shown(&huge, 64);
    assert_eq!(out.chars().count(), 65);
}
