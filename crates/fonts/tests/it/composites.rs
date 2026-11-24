//! Outlines that are made of other outlines, and fonts that ask for far more than their size says: a variable composite (`VARC`) glyph
//! that draws one big glyph again and again, and transforms that grow without end.

use std::time::{Duration, Instant};

use diacraft_fonts::synth::SynthFont;
use diacraft_fonts::{Face, FontDb, Outline};

use crate::REGULAR;
use crate::hostile::with_tables;

/// A `glyf` glyph of one contour of `points` on-curve points that are all at the origin. The flags say "same as the one before" for both
/// coordinates and repeat, so a contour of 65,535 points is 528 bytes.
fn glyph_of_coincident_points(points: u16) -> Vec<u8> {
    let mut g = Vec::new();
    g.extend_from_slice(&1i16.to_be_bytes());
    g.extend_from_slice(&[0; 8]);
    g.extend_from_slice(&(points - 1).to_be_bytes());
    g.extend_from_slice(&0u16.to_be_bytes());
    let mut left = usize::from(points);
    while left > 0 {
        let run = left.min(256);
        // On curve, x and y the same as before; a run of more than one repeats the flag.
        g.push(if run > 1 { 0x39 } else { 0x31 });
        if run > 1 {
            g.push((run - 1) as u8);
        }
        left -= run;
    }
    g
}

/// A `VARC` table that covers glyph 1 only, whose glyph is `references` components, each glyph 0 with no transform.
fn varc_of_references(references: usize) -> Vec<u8> {
    // Header: version 1.0, then the offsets of the coverage, the variation store (none), the conditions (none), the axis indices (none)
    // and the composite glyphs.
    let coverage = [0u8, 1, 0, 1, 0, 1];
    let mut glyph = Vec::new();
    for _ in 0..references {
        // Flags 0 (a variable-length number of one byte), glyph id 0.
        glyph.extend_from_slice(&[0, 0, 0]);
    }
    let glyphs_at = 24 + coverage.len();
    let mut t = Vec::new();
    t.extend_from_slice(&[0, 1, 0, 0]);
    for offset in [24u32, 0, 0, 0, glyphs_at as u32] {
        t.extend_from_slice(&offset.to_be_bytes());
    }
    t.extend_from_slice(&coverage);
    // An index of one glyph: its count, the size of an offset, the offsets, and the data.
    t.extend_from_slice(&1u32.to_be_bytes());
    t.push(4);
    t.extend_from_slice(&1u32.to_be_bytes());
    t.extend_from_slice(&(glyph.len() as u32 + 1).to_be_bytes());
    t.extend_from_slice(&glyph);
    t
}

/// A font of two glyphs: glyph 0 is `leaf_points` points at the origin, and glyph 1 (the one `a` maps to) is a variable composite of
/// `references` copies of glyph 0.
fn composite_font(leaf_points: u16, references: usize) -> Vec<u8> {
    let base = SynthFont::new("Composite").chars("a", 500).build();
    let leaf = glyph_of_coincident_points(leaf_points);
    let mut loca = Vec::new();
    for offset in [0u32, leaf.len() as u32, leaf.len() as u32] {
        loca.extend_from_slice(&offset.to_be_bytes());
    }
    with_tables(&base, &[(*b"glyf", leaf), (*b"loca", loca), (*b"VARC", varc_of_references(references))])
}

/// Is this the answer for a glyph that could not be drawn? (Not an `assert_eq!`: a path of a hundred million elements prints as a page.)
fn is_broken(outline: &Outline) -> bool {
    matches!(outline, Outline::Broken)
}

fn face_of(font: Vec<u8>) -> Face {
    let mut db = FontDb::empty();
    assert_eq!(db.add_font_data(font).added, 1);
    db.pick("Composite", REGULAR)
}

#[test]
fn the_composite_font_built_here_is_a_font_whose_glyph_one_is_made_of_glyph_zero() {
    // A few copies of a few points, which are well inside every limit: the glyph is drawn, from every copy.
    let face = face_of(composite_font(5, 3));
    assert_eq!(face.glyph_for('a'), 1);
    assert!(!face.is_broken());
    match face.outline_glyph(0) {
        Outline::Drawn(path) => assert_eq!(path.elements().len(), 1 + 4 + 1, "a move, four lines and the close of one contour of five points"),
        other => panic!("glyph 0 is a contour: {other:?}"),
    }
    match face.outline_glyph(1) {
        Outline::Drawn(path) => assert_eq!(path.elements().len(), 3 * 6, "three copies of it"),
        other => panic!("glyph 1 is three copies of glyph 0: {other:?}"),
    }
}

#[test]
fn a_composite_that_draws_a_big_glyph_thousands_of_times_is_broken_and_not_a_hundred_million_path_elements() {
    // The most references one composite follows is 2,048; each draws a contour of 65,535 points into the sink whole, and nothing else
    // limits the total: 2,047 of them were 134 million path elements, about 7.5 GB, in one call. A face makes a glyph of at most 262,144
    // elements, and says that it could not draw this one.
    let face = face_of(composite_font(u16::MAX, 2047));
    let start = Instant::now();
    diacraft_fonts::work::reset();
    assert!(is_broken(&face.outline_glyph(1)));
    // Counted: the path never held more than the limit (the 134 million are drawn into a pen that keeps nothing past it).
    let held = diacraft_fonts::work::peak_path_elements();
    assert!((65_536..=262_144).contains(&held), "the path held {held} elements at most");
    assert!(face.outline(1).is_none());
    assert!(!face.is_broken(), "no panic: the glyph is too big, the face is whole");
    // The leaf alone is a glyph of 65,536 elements, well inside the limit.
    assert!(matches!(face.outline_glyph(0), Outline::Drawn(path) if path.elements().len() == 65_536));
    // A hang guard and no measure of cost: the test takes seconds on a busy machine, because the drawing of the 134 million elements into the
    // pen goes on, and a minute is generous for one at load 50.
    assert!(start.elapsed() < Duration::from_secs(60), "took {:?}", start.elapsed());
}

#[test]
fn a_composite_just_over_the_limit_is_broken_and_one_just_under_it_is_drawn() {
    // Each copy of 4,095 points is 4,096 elements: 63 of them are 258,048, 65 are 266,240, and the limit is 262,144.
    let drawn = face_of(composite_font(4095, 63));
    assert!(matches!(drawn.outline_glyph(1), Outline::Drawn(path) if path.elements().len() == 63 * 4096));
    let broken = face_of(composite_font(4095, 65));
    assert!(is_broken(&broken.outline_glyph(1)));
    // A single contour of the most points a glyph has is one fourth of the limit.
    let one = face_of(composite_font(u16::MAX, 1));
    assert!(matches!(one.outline_glyph(1), Outline::Drawn(path) if path.elements().len() == 65_536));
}

/// A `glyf` glyph of one contour whose points each lie a step right and up of the one before, the first of the origin.
fn glyph_of_steps(steps: &[(u8, u8)]) -> Vec<u8> {
    let mut g = Vec::new();
    g.extend_from_slice(&1i16.to_be_bytes());
    g.extend_from_slice(&[0; 8]);
    g.extend_from_slice(&((steps.len() - 1) as u16).to_be_bytes());
    g.extend_from_slice(&0u16.to_be_bytes());
    // On curve, a short x and a short y, both positive.
    g.extend(std::iter::repeat_n(0x37u8, steps.len()));
    g.extend(steps.iter().map(|(x, _)| *x));
    g.extend(steps.iter().map(|(_, y)| *y));
    g
}

/// A font whose glyphs 1 to `levels` are variable composites of one component each, glyph `n` of glyph `n + 1` scaled by `scale` (16
/// bits, in 1/1024), and the last of glyph 0, which is the points (0, 0) and (10, 10). Glyph 1 is `levels` scales deep.
fn scale_chain_font(levels: usize, scale: i16) -> Vec<u8> {
    let letters: String = (0..levels).map(|i| char::from_u32(0x61 + i as u32).unwrap_or('a')).collect();
    let base = SynthFont::new("Composite").chars(&letters, 500).build();
    let leaf = glyph_of_steps(&[(0, 0), (10, 10)]);
    let mut loca = Vec::new();
    for i in 0..levels + 2 {
        loca.extend_from_slice(&(if i == 0 { 0 } else { leaf.len() as u32 }).to_be_bytes());
    }
    // The components: glyph n (n from 1) has one, flags "scale x" (256, two bytes), the glyph id, the scale.
    let mut records: Vec<Vec<u8>> = Vec::new();
    for n in 1..=levels {
        let target = if n == levels { 0 } else { (n + 1) as u16 };
        let mut c = vec![0x81, 0x00];
        c.extend_from_slice(&target.to_be_bytes());
        c.extend_from_slice(&scale.to_be_bytes());
        records.push(c);
    }
    let mut coverage = vec![0u8, 1];
    coverage.extend_from_slice(&(levels as u16).to_be_bytes());
    for n in 1..=levels {
        coverage.extend_from_slice(&(n as u16).to_be_bytes());
    }
    let glyphs_at = 24 + coverage.len();
    let mut t = vec![0, 1, 0, 0];
    for offset in [24u32, 0, 0, 0, glyphs_at as u32] {
        t.extend_from_slice(&offset.to_be_bytes());
    }
    t.extend_from_slice(&coverage);
    t.extend_from_slice(&(levels as u32).to_be_bytes());
    t.push(4);
    let mut at = 1u32;
    t.extend_from_slice(&at.to_be_bytes());
    for r in &records {
        at += r.len() as u32;
        t.extend_from_slice(&at.to_be_bytes());
    }
    records.iter().for_each(|r| t.extend_from_slice(r));
    with_tables(&base, &[(*b"glyf", leaf), (*b"loca", loca), (*b"VARC", t)])
}

#[test]
fn composites_that_scale_by_thirty_two_at_every_level_are_drawn_while_they_stay_in_range_and_broken_when_they_do_not() {
    // 0x7FFF / 1024 is just under 32. Three levels put the far point of the leaf at 10 x 32^3 = 327,680 font units, which a glyph may
    // have (the limit is 1,048,576, sixty-four ems of the largest em a face has); five put it at 335 million, and thirty at a number
    // that does not fit in 32 bits, where the origin of the leaf is infinity times zero.
    let drawn = face_of(scale_chain_font(3, i16::MAX));
    match drawn.outline_glyph(1) {
        Outline::Drawn(path) => {
            let bounds = kurbo::Shape::bounding_box(&path);
            assert!(bounds.max_x() > 300_000.0 && bounds.max_x() < 400_000.0, "{bounds:?}");
        }
        other => panic!("three levels fit: {other:?}"),
    }
    for levels in [5, 8, 30, 64] {
        let broken = face_of(scale_chain_font(levels, i16::MAX));
        assert!(is_broken(&broken.outline_glyph(1)), "{levels} levels");
        assert!(!broken.is_broken());
    }
}
