//! Where a text block sits and what its background covers, built in code from the vectors of (item 2).
//!
//! The face is built in code (`quill`: every letter 0.6 of an em wide), so the width of a line is known to the thousandth and no machine
//! font enters. At 0.2 inch a line of `AAAA` is 0.48 inch wide and its line box, at `SpLine` -1.2, is 0.24 inch tall.

use diacraft_doc::RowKey;
use diacraft_render::Renderer;
use diacraft_testkit::build::ShapeBuilder;

use crate::support::*;

const FOUR_POINTS: &str = "0.0555556";

/// A renderer whose database also holds `quill`, a face of box glyphs 0.6 em to the advance.
fn quill() -> Renderer {
    let mut db = diacraft_fonts::FontDb::bundled();
    db.add_font_data(diacraft_fonts::synth::SynthFont::new("quill").ascii(600).build());
    Renderer::with_fonts(db)
}

/// Shape `id`, 2 inches by 1 at (1, 1), with no fill and no line, showing `text` in `quill` at 0.2 inch in black, centred, `SpLine` -1.2, with
/// margins of four points, and `background` in `TextBkgnd`.
fn labelled(id: u32, text: &str, background: &str) -> ShapeBuilder {
    let mut sb = rect(id, 1.0, 1.0, 2.0, 1.0)
        .cell("FillPattern", "0")
        .cell("LinePattern", "0")
        .cell("TextBkgnd", background)
        .text(text)
        .row_cell("Character", None, RowKey::Ix(0), "Size", "0.2")
        .row_cell("Character", None, RowKey::Ix(0), "Font", "quill")
        .row_cell("Character", None, RowKey::Ix(0), "Color", "#000000")
        .row_cell("Paragraph", None, RowKey::Ix(0), "SpLine", "-1.2")
        .row_cell("Paragraph", None, RowKey::Ix(0), "HorzAlign", "1");
    for margin in ["LeftMargin", "RightMargin", "TopMargin", "BottomMargin"] {
        sb = sb.cell(margin, FOUR_POINTS);
    }
    sb
}

fn green(pic: &Pic, x: f64, y: f64) -> bool {
    pic.is(x, y, (0, 255, 0), 8)
}

fn draw_quill(shapes: Vec<ShapeBuilder>) -> Pic {
    draw_with(&mut quill(), &one_page(shapes))
}

/// A blue plate over the whole 4 inch page, behind what follows, so that a white rectangle shows against it.
fn plate() -> ShapeBuilder {
    rect(9, 0.0, 0.0, 4.0, 4.0).cell("FillForegnd", "#0000FF").cell("LinePattern", "0")
}

#[test]
fn the_background_is_one_rectangle_round_the_line_boxes_and_takes_no_margin() {
    // A centred line 0.48 wide in a block 2 by 1 with margins: the rectangle runs 1.76 to 2.24 across and, for a line box of 0.24 about
    // the middle of the area, 1.38 to 1.62 up. The block (1 to 3) and the area inside the margins are both wider and taller.
    let pic = draw_quill(vec![labelled(1, "AAAA", "#00FF00")]);
    for (x, y) in [(1.77, 1.39), (2.23, 1.39), (1.77, 1.61), (2.23, 1.61)] {
        assert!(green(&pic, x, y), "({x}, {y}) is inside the rectangle: {:?}", pic.at(x, y));
    }
    for (x, y) in [(1.74, 1.5), (2.26, 1.5), (2.0, 1.36), (2.0, 1.64), (1.2, 1.2)] {
        assert!(pic.is_white(x, y), "({x}, {y}) is outside it: {:?}", pic.at(x, y));
    }
}

#[test]
fn two_lines_have_one_rectangle_as_wide_as_the_wider_and_as_tall_as_both() {
    // AAAA (0.48) over AA (0.24), each line box 0.24. The two boxes stand 0.48 tall about the middle: 1.26 to 1.74.
    let pic = draw_quill(vec![labelled(1, "AAAA\nAA", "#00FF00")]);
    assert!(green(&pic, 1.77, 1.73) && green(&pic, 2.23, 1.27), "the corners of the wider line's box, top and bottom");
    assert!(green(&pic, 1.77, 1.27), "the narrower line is not a rectangle of its own: the lower left of the wide one is covered");
    assert!(pic.is_white(1.74, 1.5) && pic.is_white(2.26, 1.5) && pic.is_white(2.0, 1.23) && pic.is_white(2.0, 1.77));
}

#[test]
fn a_background_of_nothing_paints_nothing() {
    // 0 is none, and the black of colour index 0 is wrong. 255 is none too.
    for none in ["0", "255"] {
        let pic = draw_quill(vec![plate(), labelled(1, "AAAA", none)]);
        assert!(pic.is(1.77, 1.39, (0, 0, 255), 6) && pic.is(2.23, 1.61, (0, 0, 255), 6), "TextBkgnd {none}: {:?}", pic.at(1.77, 1.39));
    }
}

#[test]
fn a_background_of_no_text_paints_nothing() {
    // The rectangle hugs text, and there is none.
    let pic = draw_quill(vec![labelled(1, "", "#00FF00")]);
    assert_eq!(pic.ink_pixels(), 0);
}

#[test]
fn a_number_in_the_background_cell_is_the_colour_one_less_than_it() {
    // our rule: Learn's "plus one" makes 1 the first colour of the table, black, and 2 the second, white; the index
    // of `vColor` (2, red) is the other reading. A blue plate behind the label shows the rectangle's colour.
    let pic = draw_quill(vec![plate(), labelled(1, "AAAA", "2")]);
    assert!(pic.is(1.77, 1.39, (255, 255, 255), 6), "2 is white, not red: {:?}", pic.at(1.77, 1.39));
    let pic = draw_quill(vec![plate(), labelled(1, "AAAA", "1")]);
    assert!(pic.is(1.77, 1.39, (0, 0, 0), 6), "1 is the first colour, black: {:?}", pic.at(1.77, 1.39));
    // A colour as `#RRGGBB` is itself: the "plus one" is in the formula and not in the value.
    let pic = draw_quill(vec![plate(), labelled(1, "AAAA", "#FF7FFF")]);
    assert!(pic.is(1.77, 1.39, (255, 127, 255), 6), "{:?}", pic.at(1.77, 1.39));
}

#[test]
fn a_block_with_none_of_the_seven_cells_is_the_shape_pinned_at_its_middle() {
    // The printed defaults are Width, Height, half of each, and no turn. Zero inches would put the text at the corner.
    let pic = draw_quill(vec![labelled(1, "AAAA", "#00FF00")]);
    let b = pic.ink_box().expect("drawn");
    assert!((((b.0 + b.2) / 2.0) - 2.0).abs() < 0.01, "centred on the shape's middle: {b:?}");
}

/// `labelled` with a block of its own: width, height, pin, local pin and turn (degrees).
fn blocked(background: &str, width: f64, height: f64, pin: (f64, f64), loc_pin: (f64, f64), degrees: f64) -> ShapeBuilder {
    labelled(1, "AAAA", background)
        .cell("TxtWidth", &width.to_string())
        .cell("TxtHeight", &height.to_string())
        .cell("TxtPinX", &pin.0.to_string())
        .cell("TxtPinY", &pin.1.to_string())
        .cell("TxtLocPinX", &loc_pin.0.to_string())
        .cell("TxtLocPinY", &loc_pin.1.to_string())
        .cell("TxtAngle", &degrees.to_radians().to_string())
}

#[test]
fn a_block_is_placed_by_its_pin_and_turned_about_its_local_pin() {
    // The block 1 by 0.5, its local pin at (0.25, 0.1), pinned at (1, 0.5) of the shape and turned 90 degrees. The block's middle
    // (0.5, 0.25) is (0.25, 0.15) from the local pin, which the turn makes (-0.15, 0.25), so it lies at (0.85, 0.75) in the shape, which is at
    // (1.85, 1.75) on the page. The line box, 0.48 by 0.24 in the block, stands 0.24 wide and 0.48 tall: x 1.73 to 1.97, y 1.51 to 1.99.
    let pic = draw_quill(vec![blocked("#00FF00", 1.0, 0.5, (1.0, 0.5), (0.25, 0.1), 90.0)]);
    for (x, y) in [(1.74, 1.52), (1.96, 1.52), (1.74, 1.98), (1.96, 1.98)] {
        assert!(green(&pic, x, y), "({x}, {y}) is a corner of the turned rectangle: {:?}", pic.at(x, y));
    }
    for (x, y) in [(1.70, 1.75), (2.00, 1.75), (1.85, 1.48), (1.85, 2.02)] {
        assert!(pic.is_white(x, y), "({x}, {y}) is outside it: {:?}", pic.at(x, y));
    }
}

#[test]
fn a_block_is_placed_the_same_when_the_shape_is_flipped_and_the_text_is_not_mirrored() {
    // FlipX, the block 2 wide (the shape's width), left aligned with a margin of 0.1. The shape is pinned at its right so
    // that it covers 1 to 3 on the page whichever way it is flipped. The line starts 0.1 from the left of the block as seen on the page.
    let flipped = |flip: &str| {
        let sb = labelled(1, "AAAA", "#00FF00")
            .cell("FlipX", flip)
            .cell("PinX", if flip == "1" { "3" } else { "1" })
            .cell("LeftMargin", "0.1")
            .row_cell("Paragraph", None, RowKey::Ix(0), "HorzAlign", "0");
        draw_quill(vec![sb])
    };
    for flip in ["0", "1"] {
        let pic = flipped(flip);
        assert!(green(&pic, 1.11, 1.5) || pic.is_dark(1.11, 1.5), "FlipX {flip}: the line starts at 1.1: {:?}", pic.at(1.11, 1.5));
        assert!(pic.is_white(1.09, 1.5), "FlipX {flip}: and not before it");
        assert!(pic.is_white(1.59, 1.5), "FlipX {flip}: and ends 0.48 on, at 1.58");
    }
}

#[test]
fn a_margin_is_on_paper_and_a_block_is_in_drawing_units_on_a_scaled_page() {
    // PageScale 1 over DrawingScale 2, so a drawing unit is half an inch. A shape 4 by 2 drawing units is 2 by 1 on
    // paper; margins of four points stay four points; the line is 0.48 wide on paper. The page is 8 by 8 drawing units, 4 by 4 inches.
    let sb = rect(1, 2.0, 2.0, 4.0, 2.0)
        .cell("FillPattern", "0")
        .cell("LinePattern", "0")
        .cell("TextBkgnd", "#00FF00")
        .text("AAAA")
        .row_cell("Character", None, RowKey::Ix(0), "Size", "0.2")
        .row_cell("Character", None, RowKey::Ix(0), "Font", "quill")
        .row_cell("Character", None, RowKey::Ix(0), "Color", "#000000")
        .row_cell("Paragraph", None, RowKey::Ix(0), "SpLine", "-1.2")
        .row_cell("Paragraph", None, RowKey::Ix(0), "HorzAlign", "1")
        .cell("LeftMargin", FOUR_POINTS)
        .cell("RightMargin", FOUR_POINTS)
        .cell("TopMargin", FOUR_POINTS)
        .cell("BottomMargin", FOUR_POINTS);
    let p = page(8.0, 8.0).cell("PageScale", "1").cell("DrawingScale", "2").shape(sb);
    let pic = draw_with(&mut quill(), &base().page(1, p).build());
    // The shape covers paper x 1 to 3, y 1 to 2, as in the unscaled case: the same rectangle.
    for (x, y) in [(1.77, 1.39), (2.23, 1.61)] {
        assert!(green(&pic, x, y), "({x}, {y}): {:?}", pic.at(x, y));
    }
    for (x, y) in [(1.74, 1.5), (2.26, 1.5), (2.0, 1.36), (2.0, 1.64)] {
        assert!(pic.is_white(x, y), "({x}, {y}): {:?}", pic.at(x, y));
    }
}
