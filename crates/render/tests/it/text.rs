//! Text.
//!
//! No test depends on a font the machine has: the renderer here uses the bundled faces, and every assertion is about where ink is, not
//! about what a glyph looks like.

use diacraft_doc::RowKey;
use diacraft_render::SkipKind;
use diacraft_testkit::build::ShapeBuilder;

use crate::support::*;

/// A 2 by 2 inch box at (1, 1) with no fill and no line, showing `text` at `size` inches in black.
fn label(text: &str, size: f64) -> ShapeBuilder {
    rect(1, 1.0, 1.0, 2.0, 2.0)
        .cell("FillPattern", "0")
        .cell("LinePattern", "0")
        .text(text)
        .row_cell("Character", None, RowKey::Ix(0), "Size", &size.to_string())
        .row_cell("Character", None, RowKey::Ix(0), "Color", "#000000")
}

fn centre(b: (f64, f64, f64, f64)) -> (f64, f64) {
    ((b.0 + b.2) / 2.0, (b.1 + b.3) / 2.0)
}

/// Draws a one-letter label whose `Font` is `font`, with a face of that name built in code beside the bundled ones: its glyphs are solid boxes,
/// which no real letter is. → how much of the inside of the ink is dark: 1 for the box, less for a real `H` with the gap between its stems.
fn solid_fraction(font: &str, built: &str) -> f64 {
    let mut db = diacraft_fonts::FontDb::bundled();
    db.add_font_data(diacraft_fonts::synth::SynthFont::new(built).ascii(600).build());
    let doc = one_page(vec![label("H", 1.0).row_cell("Character", None, RowKey::Ix(0), "Font", font)]);
    let pic = draw_with(&mut diacraft_render::Renderer::with_fonts(db), &doc);
    let b = pic.ink_box().expect("the letter is drawn");
    let (w, h) = (b.2 - b.0, b.3 - b.1);
    let (mut dark, mut all) = (0_u32, 0_u32);
    for i in 1..20 {
        for j in 1..20 {
            all += 1;
            dark += u32::from(pic.is_dark(b.0 + w * f64::from(i) / 20.0, b.1 + h * f64::from(j) / 20.0));
        }
    }
    f64::from(dark) / f64::from(all)
}

#[test]
fn a_face_named_with_hex_digits_is_drawn_in_that_face_and_is_not_read_as_a_colour() {
    // `Bed` is three letters that are all hex digits. A `Font` cell is text, and only `#RRGGBB` is a colour:
    // read as the colour `BBEEDD` it asked for a face of that name, found none and fell back to a stand-in, though a face called `Bed` was there.
    assert!(solid_fraction("Bed", "Bed") > 0.95, "the letter is the box of the face called Bed");
    // The control: a name no face has falls back to a stand-in, whose `H` is not solid; and a face with a name made of letters that are not all hex digits is
    // found as before.
    assert!(solid_fraction("Bed", "Other") < 0.9, "no face called Bed: the stand-in's H, with its gap");
    assert!(solid_fraction("Quill", "Quill") > 0.95, "a face found by its name");
}

#[test]
fn text_is_centred_in_its_shape_by_default() {
    let pic = draw(&one_page(vec![label("W", 1.0)]));
    let b = pic.ink_box().expect("the letter is drawn");
    let (cx, cy) = centre(b);
    assert!((cx - 2.0).abs() < 0.25 && (cy - 2.0).abs() < 0.35, "centre of ink {cx}, {cy} in {b:?}");
    assert!(b.0 > 1.0 && b.2 < 3.0 && b.1 > 1.0 && b.3 < 3.0, "inside the shape: {b:?}");
}

#[test]
fn vertical_alignment_seats_the_text_at_the_top_middle_or_bottom_of_the_block() {
    let at = |v: &str| {
        let pic = draw(&one_page(vec![label("W", 0.6).cell("VerticalAlign", v)]));
        centre(pic.ink_box().expect("drawn")).1
    };
    let (top, middle, bottom) = (at("0"), at("1"), at("2"));
    assert!(top > middle + 0.4 && middle > bottom + 0.4, "top {top}, middle {middle}, bottom {bottom}");
}

#[test]
fn text_turns_with_its_shape() {
    let wide = "WWWWW";
    let flat = draw(&one_page(vec![label(wide, 0.3)])).ink_box().expect("drawn");
    assert!(flat.2 - flat.0 > (flat.3 - flat.1) * 2.0, "unturned text is wider than tall: {flat:?}");
    let turned_shape = label(wide, 0.3)
        .cell("PinX", "2")
        .cell("PinY", "2")
        .cell("LocPinX", "1")
        .cell("LocPinY", "1")
        .cell("Angle", &std::f64::consts::FRAC_PI_2.to_string());
    let turned = draw(&one_page(vec![turned_shape])).ink_box().expect("drawn");
    assert!(turned.3 - turned.1 > (turned.2 - turned.0) * 2.0, "a quarter turn makes it taller than wide: {turned:?}");
}

#[test]
fn a_turn_is_counterclockwise_so_the_end_of_a_line_of_text_goes_up() {
    // Left-aligned text, turned a quarter turn counterclockwise about the pin at its centre, ends toward the top.
    let sb = label("WWWW", 0.25)
        .row_cell("Paragraph", None, RowKey::Ix(0), "HorzAlign", "0")
        .cell("PinX", "2")
        .cell("PinY", "2")
        .cell("LocPinX", "1")
        .cell("LocPinY", "1")
        .cell("Angle", &std::f64::consts::FRAC_PI_2.to_string());
    let b = draw(&one_page(vec![sb])).ink_box().expect("drawn");
    let (_, cy) = centre(b);
    assert!(cy < 1.6, "left-aligned text starts at the block's left edge (the bottom after the turn): centre of ink at {cy}");
}

#[test]
fn text_is_not_mirrored_by_a_flip_of_its_shape() {
    // The flip appears in both transforms and cancels. An L has its stem on the left and its bar along the bottom;
    // upright text keeps the ink in the left half and the bottom half, whatever the shape's flips.
    let ink_halves = |flip_x: &str, flip_y: &str| {
        let pic = draw(&one_page(vec![
            label("L", 1.4).cell("FlipX", flip_x).cell("FlipY", flip_y).cell("PinX", "2").cell("PinY", "2").cell("LocPinX", "1").cell("LocPinY", "1"),
        ]));
        let b = pic.ink_box().expect("drawn");
        let (mid_x, mid_y) = centre(b);
        let (w, h) = (pic.r.width as usize, pic.r.height as usize);
        let (mut left, mut right, mut top, mut bottom) = (0usize, 0usize, 0usize, 0usize);
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) * 4;
                let p = &pic.r.rgba_premul[i..i + 4];
                if p[3] > 0 && p[0] < 128 {
                    let (px, py) = ((x as f64 + 0.5) / pic.ppi, pic.page_h - (y as f64 + 0.5) / pic.ppi);
                    if px < mid_x {
                        left += 1
                    } else {
                        right += 1
                    }
                    if py > mid_y { top += 1 } else { bottom += 1 }
                }
            }
        }
        (left, right, top, bottom)
    };
    for (fx, fy) in [("0", "0"), ("1", "0"), ("0", "1"), ("1", "1")] {
        let (left, right, top, bottom) = ink_halves(fx, fy);
        assert!(left > right, "FlipX {fx} FlipY {fy}: the stem stays on the left ({left} against {right})");
        assert!(bottom > top, "FlipX {fx} FlipY {fy}: the bar stays at the bottom ({bottom} against {top})");
    }
}

#[test]
fn text_is_in_front_of_its_shapes_fill_and_outline() {
    let sb = label("W", 1.0).cell("FillPattern", "1").cell("FillForegnd", "#FF0000");
    let pic = draw(&one_page(vec![sb]));
    assert!(pic.is_red(1.1, 1.1), "the fill");
    let dark = (0..200)
        .flat_map(|i| (0..200).map(move |j| (i, j)))
        .filter(|(i, j)| pic.is_dark(1.0 + f64::from(*i) * 0.01, 1.0 + f64::from(*j) * 0.01))
        .count();
    assert!(dark > 100, "black ink over the red: {dark} pixels");
}

#[test]
fn an_instance_with_no_text_of_its_own_shows_its_masters() {
    let master = diacraft_testkit::build::MasterBuilder::new()
        .shape(label("W", 1.0).cell("PinX", "1").cell("PinY", "1").cell("LocPinX", "1").cell("LocPinY", "1"));
    let inst = ShapeBuilder::new(10)
        .master(1)
        .cell("PinX", "2")
        .cell("PinY", "2")
        .cell("Width", "2")
        .cell("Height", "2")
        .cell("LocPinX", "1")
        .cell("LocPinY", "1");
    let pic = draw(&base().master(1, master).page(1, page(4.0, 4.0).shape(inst)).build());
    assert!(pic.ink_pixels() > 100);
}

#[test]
fn an_empty_text_draws_nothing_and_a_cp_marker_alone_is_kept_without_ink() {
    let pic = draw(&one_page(vec![label("", 1.0)]));
    assert_eq!(pic.ink_pixels(), 0);
}

#[test]
fn letters_no_face_draws_are_counted_in_the_report() {
    // The Unicode private use area has no glyph in any face.
    let pic = draw(&one_page(vec![label("\u{E000}\u{E001}\u{E002}", 0.5)]));
    assert!(pic.r.count(SkipKind::MissingGlyphs) >= 1 || pic.r.count(SkipKind::TextNote) >= 1, "{:?}", pic.r.skipped);
}

#[test]
fn laid_out_text_is_kept_so_a_second_frame_does_not_lay_it_out_again() {
    let mut renderer = diacraft_render::Renderer::new();
    let doc = one_page(vec![label(&"W".repeat(500), 0.1)]);
    assert_eq!(renderer.texts_laid_out(), 0, "nothing is laid out before a frame asks");
    let first = draw_with(&mut renderer, &doc);
    // Counted, not timed: the first frame lays the block out once, and the second draws the one that is kept.
    assert_eq!(renderer.texts_laid_out(), 1, "the label is laid out once");
    let second = draw_with(&mut renderer, &doc);
    assert_eq!(first.r.rgba_premul, second.r.rgba_premul, "the same frame");
    assert_eq!(renderer.texts_laid_out(), 1, "a second frame lays nothing out again");
}

/// Issue 9: a face the file names that the machine lacks is its own kind of note, and the sample says which face and what drew it. A drawing made where Liberation Sans is
/// installed and opened where it is not (the web build) showed "1 text block drawn in a stand-in font or cut short" with the face named in neither the line nor the tooltip; and a face
/// the machine has is no note at all (the same drawing on the desktop showed nothing).
#[test]
fn a_face_the_machine_lacks_is_noted_with_its_name_and_the_face_that_drew_it() {
    let drawn_in = |font: &str, has: &[&str]| {
        let mut db = diacraft_fonts::FontDb::bundled();
        for name in has {
            db.add_font_data(diacraft_fonts::synth::SynthFont::new(name).ascii(600).build());
        }
        let doc = one_page(vec![label("Hello", 0.5).row_cell("Character", None, RowKey::Ix(0), "Font", font)]);
        draw_with(&mut diacraft_render::Renderer::with_fonts(db), &doc).r
    };
    let lacking = drawn_in("Liberation Sans", &[]);
    let noted = lacking
        .skipped
        .iter()
        .find(|s| s.kind == SkipKind::FaceStandIn)
        .unwrap_or_else(|| panic!("the missing face is noted: {:?}", lacking.skipped));
    assert_eq!(noted.count, 1, "one text block");
    assert!(noted.first.starts_with("Liberation Sans (drawn in ") && noted.first.ends_with(')'), "{:?}", noted.first);
    assert_eq!(lacking.count(SkipKind::TextNote), 0, "it is not the catch-all note");
    // The face is on the machine: nothing is said.
    let has = drawn_in("Liberation Sans", &["Liberation Sans"]);
    assert_eq!((has.count(SkipKind::FaceStandIn), has.count(SkipKind::TextNote)), (0, 0), "{:?}", has.skipped);
}

/// Two faces are named once each, in the order they were met, and a fifth is "and more": the sample is bounded, and a name from a file is cut.
#[test]
fn the_faces_named_in_the_sample_are_each_once_in_order_and_at_most_four() {
    let names = ["Aa Missing", "Bb Missing", "Aa Missing", "Cc Missing", "Dd Missing", "Ee Missing", "Ff Missing"];
    let blocks: Vec<ShapeBuilder> = names
        .iter()
        .enumerate()
        .map(|(i, name)| {
            rect(i as u32 + 1, 0.5 + i as f64 * 0.5, 1.0, 0.4, 0.4).cell("FillPattern", "0").cell("LinePattern", "0").text("x").row_cell(
                "Character",
                None,
                RowKey::Ix(0),
                "Font",
                name,
            )
        })
        .collect();
    let r = draw(&one_page(blocks)).r;
    let noted = r.skipped.iter().find(|s| s.kind == SkipKind::FaceStandIn).expect("noted");
    assert_eq!(noted.count, 7, "seven text blocks");
    let sample: Vec<&str> = noted.first.split("; ").collect();
    assert_eq!(sample.len(), 5, "{sample:?}");
    assert!(
        sample[0].starts_with("Aa Missing (drawn in ")
            && sample[1].starts_with("Bb Missing")
            && sample[2].starts_with("Cc Missing")
            && sample[3].starts_with("Dd Missing"),
        "{sample:?}"
    );
    assert_eq!(sample[4], "and more");
    // A name of any length is cut.
    let long = "L".repeat(500);
    let r = draw(&one_page(vec![label("x", 0.5).row_cell("Character", None, RowKey::Ix(0), "Font", &long)])).r;
    let noted = r.skipped.iter().find(|s| s.kind == SkipKind::FaceStandIn).expect("noted");
    assert!(noted.first.chars().count() < 200, "{}", noted.first.chars().count());
}
