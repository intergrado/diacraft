//! The small functions of the compressed page's steps, held to their rules on every input they can have where that is small enough to try them all, and by property where it is
//! not: the value of a Base64 character, the length rule, a group of four, a hexadecimal digit, a `%XX`, the length of a UTF-8 sequence, the character the clean
//! keeps. Each is a function of a few bytes, so that a proof can hold it for every input (design §9); these are the tests that say what the proof must say.

use diacraft_mxfile::decode::{
    StepError, b64_length_ok, b64_quad, b64_value, base64_forgiving, clean, hex_digit, keep_char, percent_byte, trim_js, uri_decode, utf8_len,
};
use diacraft_testkit::mxfile::{base64, uri_encode};
use proptest::prelude::*;

const ALPHABET: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

#[test]
fn a_base64_character_has_a_value_exactly_when_it_is_in_the_alphabet_and_the_value_is_its_place() {
    for b in 0..=255u8 {
        let place = ALPHABET.bytes().position(|a| a == b);
        assert_eq!(b64_value(b), place.map(|p| p as u8), "{b:#04x}");
    }
}

#[test]
fn a_length_that_leaves_one_over_four_is_the_only_one_that_is_refused() {
    for n in 0..10_000usize {
        assert_eq!(b64_length_ok(n), n % 4 != 1, "{n}");
    }
    for n in [usize::MAX, usize::MAX - 1, usize::MAX - 2, usize::MAX - 3] {
        assert_eq!(b64_length_ok(n), n % 4 != 1, "{n}");
    }
}

#[test]
fn a_group_of_values_is_the_inverse_of_the_standard_encoding_and_never_more_than_three_bytes() {
    let values = |text: &str| -> [u8; 4] {
        let mut q = [0u8; 4];
        for (slot, c) in q.iter_mut().zip(text.bytes().filter(|c| *c != b'=')) {
            *slot = b64_value(c).unwrap();
        }
        q
    };
    // Every one byte and every two bytes, and every third three bytes (the first four million of the sixteen million are the same cases at another scale).
    for a in 0..=255u8 {
        let one = base64(&[a]);
        assert_eq!(b64_quad(values(&one), 2), Some(([a, 0, 0], 1)), "{a}");
        for b in 0..=255u8 {
            let two = base64(&[a, b]);
            assert_eq!(b64_quad(values(&two), 3), Some(([a, b, 0], 2)), "{a} {b}");
            for c in (0..=255u8).step_by(3) {
                let three = base64(&[a, b, c]);
                assert_eq!(b64_quad(values(&three), 4), Some(([a, b, c], 3)), "{a} {b} {c}");
            }
        }
    }
    // The count decides what is made: none for 0, nothing for 1 or for more than 4, and the bytes that are not made are 0 whatever the unused values hold.
    let q = [63u8, 63, 63, 63];
    assert_eq!(b64_quad(q, 0), Some(([0, 0, 0], 0)));
    for n in [1usize, 5, 6, 100, usize::MAX] {
        assert_eq!(b64_quad(q, n), None, "{n}");
    }
    assert_eq!(b64_quad([1, 2, 63, 63], 2).map(|(b, n)| (b[1], b[2], n)), Some((0, 0, 1)));
    // A value past six bits is read by its low six, and nothing overflows.
    assert_eq!(b64_quad([255, 255, 255, 255], 4), Some(([255, 255, 255], 3)));
}

#[test]
fn hexadecimal_digits_and_percent_bytes_are_total_and_exact() {
    for c in 0..=255u8 {
        let want = (c as char).to_digit(16).map(|d| d as u8);
        assert_eq!(hex_digit(c), want.filter(|_| c.is_ascii()), "{c:#04x}");
    }
    for a in 0..=255u8 {
        for b in 0..=255u8 {
            let want = match ((a as char).to_digit(16), (b as char).to_digit(16)) {
                (Some(x), Some(y)) if a.is_ascii() && b.is_ascii() => Some((x * 16 + y) as u8),
                _ => None,
            };
            assert_eq!(percent_byte(a, b), want, "{a:#04x} {b:#04x}");
        }
    }
    assert_eq!(percent_byte(b'F', b'f'), Some(0xFF));
    assert_eq!(percent_byte(b'0', b'0'), Some(0));
}

#[test]
fn the_length_of_a_utf8_sequence_is_its_leading_ones_and_none_for_a_continuation_or_past_four() {
    for b in 0..=255u8 {
        let want = match b.leading_ones() {
            0 => Some(1),
            1 => None,
            n @ 2..=4 => Some(n as usize),
            _ => None,
        };
        assert_eq!(utf8_len(b), want, "{b:#04x}");
    }
}

#[test]
fn the_clean_keeps_every_character_but_the_controls_below_space_that_are_not_white_space_and_fffe_and_ffff() {
    for c in (0..=0x10FFFFu32).filter_map(char::from_u32) {
        let removed = matches!(c as u32, 0..=8 | 11 | 12 | 14..=31 | 0xFFFE | 0xFFFF);
        assert_eq!(keep_char(c), !removed, "U+{:04X}", c as u32);
    }
    assert_eq!(clean("a\u{0}b\u{1}c\td\ne\rf\u{FFFE}g\u{FFFF}h\u{1F}i").unwrap(), "abc\td\ne\rfghi");
    assert!(matches!(clean("plain \u{e9} \u{1F600}").unwrap(), std::borrow::Cow::Borrowed(_)), "nothing to remove: not copied");
    assert_eq!(clean("").unwrap(), "");
}

#[test]
fn trimming_is_javascripts_and_includes_the_byte_order_mark() {
    assert_eq!(trim_js("  \t\n x y \r\n"), "x y");
    assert_eq!(trim_js("\u{FEFF}abc\u{FEFF}"), "abc");
    assert_eq!(trim_js("\u{a0}abc\u{2028}"), "abc");
    assert_eq!(trim_js(""), "");
    assert_eq!(trim_js(" \n "), "");
}

#[test]
fn base64_by_the_forgiving_rule_in_a_table() {
    let ok: [(&str, &[u8]); 9] = [
        ("", b""),
        ("YQ==", b"a"),
        ("YQ", b"a"),
        ("YWI=", b"ab"),
        ("YWI", b"ab"),
        ("YWJj", b"abc"),
        (" Y W\nJ\tj \r", b"abc"),
        ("YWJjZA==", b"abcd"),
        ("/+8=", &[0xFF, 0xEF]),
    ];
    for (text, bytes) in ok {
        assert_eq!(base64_forgiving(text).as_deref(), Ok(bytes), "{text:?}");
    }
    // `Y` is one over four, `=` where it is no padding, three `=`, a character outside the alphabet (and the URL-safe ones, and a non-ASCII one), `=` that is not at the end.
    for text in
        ["Y", "YWJjZ", "YQ=", "YQ===", "=", "====", "YQ=a", "Y=Q=", "YQ==YQ==", "YW-j", "YW_j", "YW\u{e9}j", "YW\u{0}j", "YW\x0Bj", "YWJj\u{a0}Z"]
    {
        assert_eq!(base64_forgiving(text), Err(StepError::Invalid), "{text:?}");
    }
    // The leftover bits of a group of two or three are dropped, set or not (the forgiving decode does not check them).
    assert_eq!(base64_forgiving("YR==").as_deref(), Ok(&b"a"[..]));
    assert_eq!(base64_forgiving("YWJ=").as_deref(), Ok(&b"ab"[..]));
}

#[test]
fn percent_decoding_by_decodeuricomponents_rules_in_a_table() {
    let ok: [(&str, &str); 9] = [
        ("", ""),
        ("abc", "abc"),
        ("a+b", "a+b"),
        ("%20%41", " A"),
        ("%c3%a9", "\u{e9}"),
        ("%E4%B8%AD", "\u{4E2D}"),
        ("%F0%9F%98%80", "\u{1F600}"),
        ("caf\u{e9} %C3%A9 \u{4E2D}", "caf\u{e9} \u{e9} \u{4E2D}"),
        ("%25%2525", "%%25"),
    ];
    for (input, want) in ok {
        assert_eq!(uri_decode(input.as_bytes()).as_deref(), Ok(want), "{input:?}");
    }
    let bad = [
        "%",
        "%4",
        "%G1",
        "%1G",
        "% 41",
        "a%",
        "%C3",
        "%C3%",
        "%C3%2",
        "%C3%28",
        "%C3A9",
        "%E4%B8",
        "%E4%B8%",
        "%80",
        "%BF",
        "%F8%80%80%80%80",
        "%FF",
        "%C0%80",
        "%C1%BF",
        "%E0%80%80",
        "%ED%A0%80",
        "%ED%BF%BF",
        "%F4%90%80%80",
        "%F5%80%80%80",
        "%F0%80%80%80",
    ];
    for input in bad {
        assert_eq!(uri_decode(input.as_bytes()), Err(StepError::Invalid), "{input:?}");
    }
    // Bytes that are not UTF-8 and are not escapes are no text either.
    assert_eq!(uri_decode(&[b'a', 0xFF, b'b']), Err(StepError::Invalid));
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    /// What is written in Base64 is read back, with white space put anywhere in it and with or without its padding.
    #[test]
    fn base64_comes_back_with_white_space_anywhere_and_without_padding(
        data in proptest::collection::vec(any::<u8>(), 0..300),
        gaps in proptest::collection::vec((0usize..400, 0usize..5), 0..12),
        unpadded in any::<bool>(),
    ) {
        let mut text = base64(&data);
        if unpadded {
            text = text.trim_end_matches('=').to_owned();
        }
        let mut chars: Vec<char> = text.chars().collect();
        for (at, ws) in gaps {
            let at = at.min(chars.len());
            chars.insert(at, [' ', '\n', '\t', '\r', '\u{c}'][ws]);
        }
        let text: String = chars.into_iter().collect();
        prop_assert_eq!(base64_forgiving(&text).unwrap(), data);
    }

    /// A character that is not Base64 anywhere in a text makes it fail, and a text that is cut to one over four fails.
    #[test]
    fn a_bad_character_or_a_bad_length_fails(data in proptest::collection::vec(any::<u8>(), 1..100), at in 0usize..200, bad in prop::sample::select(vec!['*', '-', '_', '.', '\u{e9}', '\0', '#'])) {
        let text = base64(&data).trim_end_matches('=').to_owned();
        let mut chars: Vec<char> = text.chars().collect();
        let at = at.min(chars.len());
        chars.insert(at, bad);
        let mangled: String = chars.into_iter().collect();
        prop_assert_eq!(base64_forgiving(&mangled), Err(StepError::Invalid));
        // The same data written one character past a good length is one over four, or a good length of a different text: the rule is the length's alone.
        let chars: Vec<char> = text.chars().collect();
        if chars.len() % 4 == 2 || chars.len() % 4 == 3 {
            let extra: String = text.chars().chain(['A']).collect();
            let still_ok = extra.len() % 4 != 1;
            prop_assert_eq!(base64_forgiving(&extra).is_ok(), still_ok);
        }
    }

    /// What `encodeURIComponent` writes, `decodeURIComponent` reads back, for any text.
    #[test]
    fn a_uri_encoded_text_comes_back(text in "\\PC{0,200}") {
        prop_assert_eq!(uri_decode(uri_encode(&text).as_bytes()).unwrap(), text);
    }

    /// Any bytes at all are decoded or refused, never a panic, and what is given is the text that the escapes and the stand-for-themselves bytes make.
    #[test]
    fn any_bytes_are_decoded_or_refused(bytes in proptest::collection::vec(any::<u8>(), 0..300)) {
        if let Ok(text) = uri_decode(&bytes) {
            prop_assert!(text.len() <= bytes.len(), "an escape is three bytes and makes at most one");
        }
        let text = String::from_utf8_lossy(&bytes);
        let _ = base64_forgiving(&text);
        prop_assert!(clean(&text).unwrap().chars().all(keep_char));
    }
}
