//! Which face a character reads, by its Unicode script.
//!
//! A run names three faces: `Font`, `AsianFont` and `ComplexScriptFont`. The specification sends each letter to one by its script:
//! Han, Hiragana, Katakana, Hangul, Bopomofo and Yi read `AsianFont`; Arabic, Hebrew, Syriac, Thaana, Thai, Lao, Khmer,
//! Myanmar, Tibetan, Mongolian and the Indic scripts read `ComplexScriptFont`; everything else reads `Font`. A case of the specification adds that
//! digits, punctuation and spaces among such letters stay with `Font`, which is why a character of no script of its own
//! (Common or Inherited) answers `None` here and is never given its neighbour's slot.
//!
//! Adapted from DeckCraft's `crates/fonts/src/script.rs` (`docs/provenance.md`): the sorted range table with a binary search is
//! its shape. The ranges are rewritten for the list, the neighbour rule for Common characters is dropped, and the
//! DrawingML slot names are replaced.

/// An ISO 15924 script tag, such as `*b"Arab"`.
pub type ScriptTag = [u8; 4];

/// Which of a run's three faces a character reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FontSlot {
    /// `Font`.
    #[default]
    Latin,
    /// `AsianFont`: East Asian letters.
    Asian,
    /// `ComplexScriptFont`: complex script letters.
    Complex,
}

/// Inclusive code point ranges, sorted and disjoint, with the script of each. The Latin, Greek and Cyrillic rows exist so a
/// shaper can be told the script of ordinary text. Characters inside these blocks whose script is Common or Inherited are
/// carved out in [`script_of`].
const RANGES: &[(u32, u32, ScriptTag)] = &[
    (0x0041, 0x005A, *b"Latn"),
    (0x0061, 0x007A, *b"Latn"),
    (0x00AA, 0x00AA, *b"Latn"),
    (0x00BA, 0x00BA, *b"Latn"),
    (0x00C0, 0x00D6, *b"Latn"),
    (0x00D8, 0x00F6, *b"Latn"),
    (0x00F8, 0x02B8, *b"Latn"),
    (0x0370, 0x03FF, *b"Grek"),
    (0x0400, 0x052F, *b"Cyrl"),
    (0x0590, 0x05FF, *b"Hebr"),
    (0x0600, 0x06FF, *b"Arab"),
    (0x0700, 0x074F, *b"Syrc"),
    (0x0750, 0x077F, *b"Arab"),
    (0x0780, 0x07BF, *b"Thaa"),
    (0x0860, 0x086F, *b"Syrc"),
    (0x08A0, 0x08FF, *b"Arab"),
    (0x0900, 0x097F, *b"Deva"),
    (0x0980, 0x09FF, *b"Beng"),
    (0x0A00, 0x0A7F, *b"Guru"),
    (0x0A80, 0x0AFF, *b"Gujr"),
    (0x0B00, 0x0B7F, *b"Orya"),
    (0x0B80, 0x0BFF, *b"Taml"),
    (0x0C00, 0x0C7F, *b"Telu"),
    (0x0C80, 0x0CFF, *b"Knda"),
    (0x0D00, 0x0D7F, *b"Mlym"),
    (0x0D80, 0x0DFF, *b"Sinh"),
    (0x0E00, 0x0E7F, *b"Thai"),
    (0x0E80, 0x0EFF, *b"Laoo"),
    (0x0F00, 0x0FFF, *b"Tibt"),
    (0x1000, 0x109F, *b"Mymr"),
    (0x1100, 0x11FF, *b"Hang"),
    (0x1780, 0x17FF, *b"Khmr"),
    (0x1800, 0x18AF, *b"Mong"),
    (0x19E0, 0x19FF, *b"Khmr"),
    (0x1E00, 0x1EFF, *b"Latn"),
    (0x1F00, 0x1FFF, *b"Grek"),
    (0x2E80, 0x2FDF, *b"Hani"),
    (0x3005, 0x3007, *b"Hani"),
    (0x3021, 0x3029, *b"Hani"),
    (0x3038, 0x303B, *b"Hani"),
    (0x3041, 0x3096, *b"Hira"),
    (0x30A1, 0x30FA, *b"Kana"),
    (0x30FD, 0x30FF, *b"Kana"),
    (0x3105, 0x312F, *b"Bopo"),
    (0x3131, 0x318E, *b"Hang"),
    (0x31A0, 0x31BF, *b"Bopo"),
    (0x31F0, 0x31FF, *b"Kana"),
    (0x3400, 0x4DBF, *b"Hani"),
    (0x4E00, 0x9FFF, *b"Hani"),
    (0xA000, 0xA4CF, *b"Yiii"),
    (0xA8E0, 0xA8FF, *b"Deva"),
    (0xA960, 0xA97F, *b"Hang"),
    (0xA9E0, 0xA9FF, *b"Mymr"),
    (0xAA60, 0xAA7F, *b"Mymr"),
    (0xAC00, 0xD7FF, *b"Hang"),
    (0xF900, 0xFAFF, *b"Hani"),
    (0xFB1D, 0xFB4F, *b"Hebr"),
    (0xFB50, 0xFDFF, *b"Arab"),
    (0xFE70, 0xFEFF, *b"Arab"),
    (0xFF21, 0xFF3A, *b"Latn"),
    (0xFF41, 0xFF5A, *b"Latn"),
    (0xFF66, 0xFF9D, *b"Kana"),
    (0xFFA0, 0xFFDC, *b"Hang"),
    (0x10E60, 0x10E7F, *b"Arab"),
    (0x1EE00, 0x1EEFF, *b"Arab"),
    (0x20000, 0x323AF, *b"Hani"),
];

/// The range table, for tests and tools: sorted by start, disjoint, each with its ISO 15924 tag.
pub fn script_ranges() -> &'static [(u32, u32, ScriptTag)] {
    RANGES
}

/// Code points inside the ranges above that have no script of their own: Arabic punctuation and the combining marks of Arabic and
/// Devanagari, the Devanagari danda, the Thai baht sign. The Arabic tatweel (U+0640) is Common to Unicode but is part of a word, a
/// stretch of a joining letter, so it is left out of this list and reads Arabic like the letters either side of it.
fn is_common_or_inherited(cp: u32) -> bool {
    matches!(
        cp,
        0x060C | 0x061B | 0x061F | 0x064B..=0x0655 | 0x0670 | 0x06DD | 0x0951..=0x0954 | 0x0964 | 0x0965 | 0x0E3F | 0x0F3A..=0x0F3D | 0xFEFF
    )
}

/// The script of `c`, or `None` for Common and Inherited characters (spaces, digits, punctuation, most combining marks) and for
/// scripts this table doesn't list. Such a character reads `Font`.
pub fn script_of(c: char) -> Option<ScriptTag> {
    let cp = c as u32;
    if is_common_or_inherited(cp) {
        return None;
    }
    // The ranges are sorted and disjoint: the last one starting at or before `cp` is the only one that can hold it.
    let i = RANGES.partition_point(|(lo, _, _)| *lo <= cp);
    let (lo, hi, tag) = RANGES.get(i.checked_sub(1)?)?;
    (cp >= *lo && cp <= *hi).then_some(*tag)
}

/// The slot a script reads.
pub fn slot_of_script(script: ScriptTag) -> FontSlot {
    match &script {
        b"Hani" | b"Hira" | b"Kana" | b"Hang" | b"Bopo" | b"Yiii" => FontSlot::Asian,
        b"Arab" | b"Hebr" | b"Syrc" | b"Thaa" | b"Thai" | b"Laoo" | b"Khmr" | b"Mymr" | b"Tibt" | b"Mong" | b"Deva" | b"Beng" | b"Guru" | b"Gujr"
        | b"Orya" | b"Taml" | b"Telu" | b"Knda" | b"Mlym" | b"Sinh" => FontSlot::Complex,
        _ => FontSlot::Latin,
    }
}

/// The slot `c` reads on its own: `Latin` (that is, `Font`) for a character of no script.
pub fn slot_of(c: char) -> FontSlot {
    script_of(c).map_or(FontSlot::Latin, slot_of_script)
}

/// Does text in this script need a shaper to be drawn at all (joining, reordering, stacking marks)? The scripts that do are the
/// complex ones; Latin, Greek, Cyrillic and the East Asian scripts draw from the cmap, with the shaper only for marks.
pub fn needs_shaping(script: ScriptTag) -> bool {
    slot_of_script(script) == FontSlot::Complex
}

/// A default BCP 47 language for a script: a shaper picks its `locl` forms by language.
pub fn default_language(script: ScriptTag) -> Option<&'static str> {
    Some(match &script {
        b"Arab" => "ar",
        b"Hebr" => "he",
        b"Syrc" => "syr",
        b"Thaa" => "dv",
        b"Thai" => "th",
        b"Deva" => "hi",
        _ => return None,
    })
}
