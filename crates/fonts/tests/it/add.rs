//! Faces a caller adds from memory: the way a browser shell or an embedded font reaches the database, and the way a damaged font
//! must not.

use diacraft_fonts::FontDb;
use diacraft_fonts::synth::SynthFont;

use crate::REGULAR;

fn font(family: &str) -> Vec<u8> {
    SynthFont::new(family).ascii(500).build()
}

#[test]
fn a_font_is_added_and_found() {
    let mut db = FontDb::empty();
    let report = db.add_font_data(font("Added One"));
    assert_eq!((report.added, report.rejected), (1, 0));
    assert!(db.has_family("Added One"));
    assert_eq!(db.pick("Added One", REGULAR).family(), "Added One");
}

#[test]
fn bytes_that_are_not_a_font_are_rejected_and_change_nothing() {
    let mut db = FontDb::bundled();
    let before = db.face_count();
    for bytes in
        [vec![], vec![0; 11], vec![0; 4096], b"not a font at all, just some text".to_vec(), vec![0xFF; 100_000], b"ttcf".to_vec(), b"OTTO".to_vec()]
    {
        let report = db.add_font_data(bytes);
        assert_eq!(report.added, 0);
        assert_eq!(report.rejected, 1);
    }
    assert_eq!(db.face_count(), before);
    assert!(db.pick("anything", REGULAR).covers('a'), "the database still works");
}

#[test]
fn a_font_cut_short_is_rejected_or_harmless() {
    // Every prefix of a font: either no face at all, or a face that can still be used without a panic.
    let bytes = font("Cut Short");
    for len in (0..bytes.len()).step_by(7) {
        let mut db = FontDb::empty();
        let report = db.add_font_data(bytes[..len].to_vec());
        if report.added == 1 {
            let face = db.pick("Cut Short", REGULAR);
            let _ = (face.glyph_for('a'), face.advance(1), face.outline(1), face.covers('a'), face.shape("ab", &Default::default()));
        }
    }
}

#[test]
fn a_font_with_a_zero_units_per_em_is_rejected() {
    // Dividing by it would give infinities everywhere; the `head` table is at a known place in the built font.
    let mut bytes = font("Zero Em");
    let head = table_offset(&bytes, b"head").expect("head table");
    bytes[head + 18] = 0;
    bytes[head + 19] = 0;
    let mut db = FontDb::empty();
    assert_eq!(db.add_font_data(bytes).added, 0);
}

#[test]
fn a_font_is_a_face_if_its_em_is_from_16_to_16384_units_and_not_otherwise() {
    // The range the specification gives. The reader reads an em outside it as 1000, so a face that kept the number the table states would
    // measure by one em and be shaped by another (a font of 8 to the em: a glyph it cannot measure, half an em of 500, would be 62 ems).
    for (em, accepted) in
        [(0u16, false), (1, false), (8, false), (15, false), (16, true), (1000, true), (16_384, true), (16_385, false), (u16::MAX, false)]
    {
        let mut f = SynthFont::new("Em Range").ascii(1);
        f.units_per_em = em;
        let mut db = FontDb::empty();
        assert_eq!(db.add_font_data(f.build()).added, usize::from(accepted), "an em of {em}");
        if accepted {
            let face = db.pick("Em Range", REGULAR);
            assert_eq!(face.family(), "Em Range");
            assert_eq!(face.units_per_em(), f64::from(em));
        }
    }
}

#[test]
fn a_collection_lists_each_distinct_face_once() {
    // A collection of three entries pointing at the same face, and one with a second distinct face.
    let one = font("Collected");
    let mut db = FontDb::empty();
    let ttc = collection(&[&one, &one, &one]);
    assert_eq!(db.add_font_data(ttc).added, 1, "three entries, one table directory");
    let mut db = FontDb::empty();
    let ttc = collection(&[&font("First Of Two"), &font("Second Of Two")]);
    let report = db.add_font_data(ttc);
    assert_eq!(report.added, 2);
    assert_eq!(db.pick("Second Of Two", REGULAR).family(), "Second Of Two");
}

#[test]
fn a_collection_claiming_a_huge_count_is_bounded() {
    // A header that says four billion fonts, over a body of a few bytes: the count is capped by what the bytes hold.
    let mut bytes = b"ttcf".to_vec();
    bytes.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    bytes.extend_from_slice(&u32::MAX.to_be_bytes());
    bytes.extend_from_slice(&[0; 64]);
    let mut db = FontDb::empty();
    let report = db.add_font_data(bytes);
    assert_eq!(report.added, 0);
}

#[test]
fn a_font_over_the_cap_is_rejected_without_being_read() {
    let mut db = FontDb::empty();
    let report = db.add_font_data(vec![0; (128 << 20) + 1]);
    assert_eq!((report.added, report.rejected), (0, 1));
}

pub fn table_offset(font: &[u8], tag: &[u8; 4]) -> Option<usize> {
    let n = usize::from(u16::from_be_bytes([font[4], font[5]]));
    (0..n).find_map(|i| {
        let rec = &font[12 + i * 16..12 + i * 16 + 16];
        (&rec[..4] == tag).then(|| u32::from_be_bytes([rec[8], rec[9], rec[10], rec[11]]) as usize)
    })
}

/// How long a table of a font is, by its directory record.
pub fn table_len(font: &[u8], tag: &[u8; 4]) -> Option<usize> {
    let n = usize::from(u16::from_be_bytes([font[4], font[5]]));
    (0..n).find_map(|i| {
        let rec = &font[12 + i * 16..12 + i * 16 + 16];
        (&rec[..4] == tag).then(|| u32::from_be_bytes([rec[12], rec[13], rec[14], rec[15]]) as usize)
    })
}

/// A collection of these fonts: a header, then each font with its table offsets moved to where it now sits. A font repeated
/// is stored once and listed more than once.
pub fn collection(fonts: &[&Vec<u8>]) -> Vec<u8> {
    let header = 12 + 4 * fonts.len();
    let mut body: Vec<u8> = Vec::new();
    let mut starts: Vec<u32> = Vec::new();
    let mut stored: Vec<(&Vec<u8>, u32)> = Vec::new();
    for f in fonts {
        if let Some((_, at)) = stored.iter().find(|(g, _)| g == f) {
            starts.push(*at);
            continue;
        }
        let at = (header + body.len()) as u32;
        let mut copy = (*f).clone();
        let n = usize::from(u16::from_be_bytes([copy[4], copy[5]]));
        for i in 0..n {
            let p = 12 + i * 16 + 8;
            let off = u32::from_be_bytes([copy[p], copy[p + 1], copy[p + 2], copy[p + 3]]) + at;
            copy[p..p + 4].copy_from_slice(&off.to_be_bytes());
        }
        body.extend_from_slice(&copy);
        stored.push((f, at));
        starts.push(at);
    }
    let mut out = b"ttcf".to_vec();
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    out.extend_from_slice(&(fonts.len() as u32).to_be_bytes());
    for s in starts {
        out.extend_from_slice(&s.to_be_bytes());
    }
    out.extend_from_slice(&body);
    out
}
