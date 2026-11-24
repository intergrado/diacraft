//! Shaping: nominal advances by default, the shaper's kerning when asked.

use diacraft_fonts::synth::SynthFont;
use diacraft_fonts::{FontDb, ShapeParams};

use crate::{REGULAR, only};

fn sum(glyphs: &[diacraft_fonts::ShapedGlyph]) -> i32 {
    glyphs.iter().map(|g| g.x_advance).sum()
}

#[test]
fn a_run_of_latin_text_shapes_to_the_sum_of_its_nominal_advances() {
    let mut font = SynthFont::new("Kerned").ascii(500).advance('W', 900).advance('A', 700);
    // A kern table that would pull AV together by 80 units: the naive reading of a width kerns it.
    font.kern = vec![('A', 'V', -80), ('W', 'A', -60)];
    font = font.advance('V', 700);
    let db = only(&[font]);
    let face = db.pick("Kerned", REGULAR);
    let nominal = face.shape("WAVE", &ShapeParams::default());
    assert_eq!(nominal.len(), 4);
    assert_eq!(sum(&nominal), 900 + 700 + 700 + 500, "no kerning by default");
    let kerned = face.shape("WAVE", &ShapeParams { nominal: false, ..ShapeParams::default() });
    assert_eq!(sum(&kerned), 900 + 700 + 700 + 500 - 80 - 60, "the table is there and works, so the default really turned it off");
}

#[test]
fn clusters_are_byte_offsets_into_the_text() {
    let db = only(&[SynthFont::new("Offsets").ascii(500).chars("é", 500)]);
    let face = db.pick("Offsets", REGULAR);
    let glyphs = face.shape("aéb", &ShapeParams::default());
    assert_eq!(glyphs.iter().map(|g| g.cluster).collect::<Vec<_>>(), vec![0, 1, 3], "é is two bytes");
}

#[test]
fn right_to_left_text_comes_back_in_visual_order() {
    let db = only(&[SynthFont::new("Hebrew").chars("אבג", 400)]);
    let face = db.pick("Hebrew", REGULAR);
    let rtl = face.shape("אבג", &ShapeParams { rtl: true, ..ShapeParams::default() });
    // Logical order alef, bet, gimel: on the page the gimel is leftmost, so it is first.
    let clusters: Vec<u32> = rtl.iter().map(|g| g.cluster).collect();
    assert_eq!(clusters, vec![4, 2, 0], "each Hebrew letter is two bytes");
    let ltr = face.shape("אבג", &ShapeParams::default());
    assert_eq!(ltr.iter().map(|g| g.cluster).collect::<Vec<_>>(), vec![0, 2, 4]);
}

#[test]
fn an_empty_string_and_a_missing_character_shape_without_failing() {
    let db = FontDb::bundled();
    let face = db.pick("Hack", REGULAR);
    assert!(face.shape("", &ShapeParams::default()).is_empty());
    let g = face.shape("中", &ShapeParams::default());
    assert_eq!(g.len(), 1);
    assert_eq!(g[0].gid, 0, "the .notdef glyph");
}

#[test]
fn the_bundled_monospace_face_gives_every_letter_one_width() {
    let db = FontDb::bundled();
    let face = db.pick("Consolas", REGULAR);
    assert_eq!(face.family(), "Hack");
    let g = face.shape("iiiWWW", &ShapeParams::default());
    let widths: Vec<i32> = g.iter().map(|g| g.x_advance).collect();
    assert!(widths.windows(2).all(|w| w[0] == w[1]), "{widths:?}");
}
