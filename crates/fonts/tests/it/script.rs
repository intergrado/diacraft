//! Which face a character reads.

use diacraft_fonts::FontSlot;
use diacraft_fonts::script::{needs_shaping, script_of, script_ranges, slot_of, slot_of_script};

#[test]
fn east_asian_letters_read_the_asian_face() {
    for c in ['中', '日', 'あ', 'ア', '한', '글', 'ㄅ', 'ꀀ', '々', '\u{20000}', 'ｱ'] {
        assert_eq!(slot_of(c), FontSlot::Asian, "{c:?} U+{:04X}", c as u32);
    }
}

#[test]
fn complex_script_letters_read_the_complex_face() {
    for c in ['ب', 'ש', 'ܐ', 'ހ', 'ก', 'ກ', 'ក', 'မ', 'ཀ', 'ᠠ', 'अ', 'অ', 'ਅ', 'અ', 'ଅ', 'அ', 'అ', 'ಅ', 'അ', 'අ']
    {
        assert_eq!(slot_of(c), FontSlot::Complex, "{c:?} U+{:04X}", c as u32);
    }
}

#[test]
fn latin_greek_and_cyrillic_read_font() {
    for c in ['A', 'z', 'é', 'λ', 'ж', 'ß'] {
        assert_eq!(slot_of(c), FontSlot::Latin, "{c:?}");
    }
}

#[test]
fn digits_punctuation_and_spaces_among_such_letters_stay_with_font() {
    // Not the neighbour's face, whatever the neighbours are. A reader giving them the slot of the letter before them
    // draws the digits of an Arabic sentence in the complex face.
    for c in ['0', '9', ' ', ',', '.', '(', '、', '。', '，', 'ー', '\u{3000}', '\u{060C}', '\u{064B}'] {
        assert_eq!(slot_of(c), FontSlot::Latin, "{c:?} U+{:04X}", c as u32);
        assert!(script_of(c).is_none(), "{c:?}");
    }
    // The Arabic-Indic digits are Arabic by the script property, so they read the complex face, unlike the ASCII digits.
    assert_eq!(slot_of('٠'), FontSlot::Complex);
}

#[test]
fn nothing_valid_panics_and_unlisted_code_points_read_font() {
    for cp in [0u32, 0x7F, 0xD7FF, 0xE000, 0xFFFF, 0x1_0000, 0x10_FFFF] {
        if let Some(c) = char::from_u32(cp) {
            let _ = slot_of(c);
        }
    }
    assert_eq!(slot_of('\u{10FFFF}'), FontSlot::Latin);
    assert_eq!(slot_of('\0'), FontSlot::Latin);
}

#[test]
fn the_range_table_is_sorted_and_disjoint() {
    let ranges = script_ranges();
    assert!(!ranges.is_empty());
    for r in ranges {
        assert!(r.0 <= r.1, "{r:x?}");
    }
    for w in ranges.windows(2) {
        assert!(w[0].1 < w[1].0, "{:x?} then {:x?}", w[0], w[1]);
    }
}

#[test]
fn the_slots_of_every_listed_script_are_the_ones_the_spec_names() {
    for tag in [*b"Hani", *b"Hira", *b"Kana", *b"Hang", *b"Bopo", *b"Yiii"] {
        assert_eq!(slot_of_script(tag), FontSlot::Asian, "{}", String::from_utf8_lossy(&tag));
    }
    for tag in [
        *b"Arab", *b"Hebr", *b"Syrc", *b"Thaa", *b"Thai", *b"Laoo", *b"Khmr", *b"Mymr", *b"Tibt", *b"Mong", *b"Deva", *b"Beng", *b"Guru", *b"Gujr",
        *b"Orya", *b"Taml", *b"Telu", *b"Knda", *b"Mlym", *b"Sinh",
    ] {
        assert_eq!(slot_of_script(tag), FontSlot::Complex, "{}", String::from_utf8_lossy(&tag));
        assert!(needs_shaping(tag));
    }
    for tag in [*b"Latn", *b"Grek", *b"Cyrl", *b"Zyyy"] {
        assert_eq!(slot_of_script(tag), FontSlot::Latin);
        assert!(!needs_shaping(tag));
    }
}

#[test]
fn the_edges_of_the_blocks_are_where_unicode_puts_them() {
    // Written out from the Unicode blocks, not read from the table under test.
    let tag = |c: char| script_of(c).map(|t| String::from_utf8_lossy(&t).into_owned());
    for (c, want) in [
        ('\u{4E00}', Some("Hani")),
        ('\u{9FFF}', Some("Hani")),
        ('\u{4DBF}', Some("Hani")),
        ('\u{4DC0}', None),
        ('\u{3041}', Some("Hira")),
        ('\u{3096}', Some("Hira")),
        ('\u{30A1}', Some("Kana")),
        ('\u{30FA}', Some("Kana")),
        ('\u{30FB}', None),
        ('\u{30FC}', None),
        ('\u{AC00}', Some("Hang")),
        ('\u{A000}', Some("Yiii")),
        ('\u{3105}', Some("Bopo")),
        ('\u{0627}', Some("Arab")),
        ('\u{0656}', Some("Arab")),
        ('\u{05D0}', Some("Hebr")),
        ('\u{0E01}', Some("Thai")),
        ('\u{0905}', Some("Deva")),
        ('\u{0041}', Some("Latn")),
        ('\u{03B1}', Some("Grek")),
        ('\u{0436}', Some("Cyrl")),
    ] {
        assert_eq!(tag(c), want.map(str::to_owned), "U+{:04X}", c as u32);
    }
    // The Arabic marks that have no script of their own, and the sign Arabic punctuation shares with other scripts.
    for cp in (0x064Bu32..=0x0655).chain([0x0670, 0x060C, 0x061B, 0x061F, 0x06DD]) {
        assert!(script_of(char::from_u32(cp).unwrap()).is_none(), "U+{cp:04X}");
    }
}

#[test]
fn a_script_gets_the_language_a_shaper_picks_its_forms_by() {
    use diacraft_fonts::script::default_language;
    for (tag, lang) in [(*b"Arab", "ar"), (*b"Hebr", "he"), (*b"Syrc", "syr"), (*b"Thaa", "dv"), (*b"Thai", "th"), (*b"Deva", "hi")] {
        assert_eq!(default_language(tag), Some(lang));
    }
    assert_eq!(default_language(*b"Latn"), None);
    assert_eq!(default_language(*b"Hani"), None);
}

#[test]
fn east_asian_scripts_draw_from_the_cmap_and_only_complex_ones_need_a_shaper() {
    for tag in [*b"Hani", *b"Hira", *b"Kana", *b"Hang", *b"Bopo", *b"Yiii"] {
        assert!(!needs_shaping(tag), "{}", String::from_utf8_lossy(&tag));
    }
}

#[test]
fn the_tatweel_is_arabic_so_a_stretched_word_keeps_one_face() {
    // U+0640 is Common to Unicode, but it lengthens a joining letter: drawn in another face it breaks the word.
    assert_eq!(script_of('\u{0640}'), Some(*b"Arab"));
    assert_eq!(slot_of('\u{0640}'), FontSlot::Complex);
}
