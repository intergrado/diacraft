//! A `Themed` marker that the theme cannot answer: 11 of 12 real files drew a blank page because the renderer asked for the
//! theme while it still held its list of notes open, and the second borrow of that list panicked, so the whole frame was lost.
//!
//! A cell answers `THEMEVAL()` only where its sheet selects no theme (`calc`'s `themeval`); a sheet whose five scheme cells are all above
//! 0 selects one, `calc` leaves the cell unanswered, and the renderer reads the root style's cell instead
//! and notes it. Every test here puts a theme on its shapes and then makes cells hold the marker, so the unanswered path runs for
//! each cell the renderer reads.

use diacraft_doc::{Cell, CellAddr, Keying, RowKey, Value, keying, listed_cells};
use diacraft_render::SkipKind;
use diacraft_testkit::build::ShapeBuilder;

use crate::support::*;

/// The five cells that select a theme (`calc`): all above 0 and `THEMEVAL()` has no answer.
const SCHEME_CELLS: [&str; 5] = ["ThemeIndex", "ColorSchemeIndex", "EffectSchemeIndex", "ConnectorSchemeIndex", "FontSchemeIndex"];

/// The cells that place a shape. Themed too, a shape has no size and nothing to draw.
const PLACEMENT: [&str; 9] = ["PinX", "PinY", "Width", "Height", "LocPinX", "LocPinY", "Angle", "FlipX", "FlipY"];

fn marker() -> Cell {
    Cell::with_formula("THEMEVAL()", Some(Value::from_text("Themed")))
}

/// The shape selects a theme: every scheme cell is 1.
fn on_a_theme(mut sb: ShapeBuilder) -> ShapeBuilder {
    for name in SCHEME_CELLS {
        sb = sb.cell(name, "1");
    }
    sb
}

fn themed(sb: ShapeBuilder, names: &[&str]) -> ShapeBuilder {
    names.iter().fold(sb, |sb, name| sb.set(CellAddr::direct(name), marker()))
}

fn themed_in_row(sb: ShapeBuilder, section: &str, names: &[&str]) -> ShapeBuilder {
    names.iter().fold(sb, |sb, name| sb.set(CellAddr::in_row(section, None, RowKey::Ix(0), name), marker()))
}

/// What a frame that lost nothing says: the renderer did not recover from a bug, and no shape was lost to one.
fn assert_nothing_lost(pic: &Pic) {
    assert_eq!(pic.r.count(SkipKind::Recovered), 0, "the whole frame was lost: {:?}", pic.r.skipped);
    assert_eq!(pic.r.count(SkipKind::ShapeFailed), 0, "a shape was lost: {:?}", pic.r.skipped);
}

const FILL: [&str; 7] =
    ["FillPattern", "FillForegnd", "FillBkgnd", "FillForegndTrans", "FillBkgndTrans", "FillGradientEnabled", "RotateGradientWithShape"];
const LINE: [&str; 11] = [
    "LinePattern",
    "LineColor",
    "LineWeight",
    "LineColorTrans",
    "LineCap",
    "Rounding",
    "BeginArrow",
    "EndArrow",
    "BeginArrowSize",
    "EndArrowSize",
    "LineGradientEnabled",
];
const SHADOW: [&str; 9] = [
    "ShapeShdwType",
    "ShdwForegnd",
    "ShdwForegndTrans",
    "ShapeShdwOffsetX",
    "ShapeShdwOffsetY",
    "ShapeShdwScaleFactor",
    "ShapeShdwBlur",
    "ShapeShdwShow",
    "ShapeShdwObliqueAngle",
];
const TEXT_BLOCK: [&str; 8] =
    ["LeftMargin", "RightMargin", "TopMargin", "BottomMargin", "VerticalAlign", "DefaultTabStop", "TextBkgnd", "TextBkgndTrans"];
const CHARACTER: [&str; 8] = ["Font", "Size", "Color", "ColorTrans", "Style", "Case", "Pos", "FontScale"];
const PARAGRAPH: [&str; 6] = ["HorzAlign", "IndFirst", "IndLeft", "IndRight", "SpLine", "Bullet"];

/// A shape of 2 by 2 inches at (1, 1), with text, on a theme, whose fill, line, text-block, character and paragraph cells and all the
/// shadow cells but `ShdwPattern` hold the marker.
fn themed_shape(id: u32, x: f64) -> ShapeBuilder {
    let sb = on_a_theme(rect(id, x, 1.0, 2.0, 2.0)).text("Hi").cell("ShdwPattern", "1");
    let sb = themed(sb, &[&FILL[..], &LINE, &SHADOW, &TEXT_BLOCK].concat());
    themed_in_row(themed_in_row(sb, "Character", &CHARACTER), "Paragraph", &PARAGRAPH)
}

#[test]
fn a_shape_with_themed_fill_line_shadow_and_text_cells_is_drawn_and_the_theme_is_said_to_be_silent() {
    // The page is 4 inches wide: the themed shape fills 1 to 3, a plain red one beside it at 3.25 shows the rest of the page is untouched.
    let plain = rect(2, 3.25, 1.0, 0.5, 2.0).cell("FillForegnd", "#FF0000");
    // The root's fill is a colour, so a filled shape differs from the bare paper.
    let doc = base().root(|r| r.cell("FillForegnd", "#3C5A78")).page(1, page(4.0, 4.0).shape(themed_shape(1, 1.0)).shape(plain)).build();
    let pic = draw(&doc);
    assert_nothing_lost(&pic);
    // The root style's solid fill stands for the unanswered FillPattern, in the root's colour for a themed FillForegnd.
    assert!(pic.is(1.3, 1.3, (0x3C, 0x5A, 0x78), 8), "the themed shape is not blank: {:?}", pic.at(1.3, 1.3));
    assert!(pic.is_red(3.5, 2.0), "the shape after it is drawn");
    // Every themed cell the renderer reads for this shape is asked of the theme once, and the answer is kept, so each is counted once: 43
    // of the 49 cells above. The 6 it does not ask for: `RotateGradientWithShape` (the fill has no gradient), the shape's own shadow offset,
    // scale and oblique angle (its shadow type reads 0, so the page's stand in), and `TextBkgndTrans` (a background of none). A number
    // that moves is a renderer that reads other cells, and wants a reason.
    //
    // Moved it from 43 to 24, and the reason is: a call naming no property, in a cell that no table
    // of THEMEVAL holds, gives the root style's cell of the host's own name with a theme as without, and no longer an error. Nineteen of the 43
    // are such cells (`ShapeShdwShow`, seven of the text block, five of the character row and six of the paragraph row), and the root's cell is
    // what the renderer's own fallback read for them, so nothing drawn moves. The other 24 are in a table, and the file has no theme part.
    assert_eq!(pic.r.count(SkipKind::ThemeUnanswered), 24, "{:?}", pic.r.skipped);
}

#[test]
fn a_themed_fill_pattern_and_shadow_pattern_are_unanswered_without_losing_the_shape() {
    // The two cells a third of real shapes hold as the marker. With the pattern unanswered the root style's pattern stands.
    let sb = themed(on_a_theme(rect(1, 1.0, 1.0, 2.0, 2.0)).cell("FillForegnd", "#FF0000"), &["FillPattern", "ShdwPattern"]);
    let pic = draw(&one_page(vec![sb]));
    assert_nothing_lost(&pic);
    assert!(pic.is_red(2.0, 2.0) && pic.ink_pixels() > 0, "the root's solid fill stands for the pattern: {:?}", pic.at(2.0, 2.0));
    assert!(pic.r.count(SkipKind::ThemeUnanswered) >= 2, "{:?}", pic.r.skipped);
}

/// Every cell the catalogue lists, as the marker: direct cells of the sheet, and each section's cells in one row. `keep` stays as it is.
fn everything_themed(mut sb: ShapeBuilder, keep: &[&str], skip_sections: &[&str]) -> ShapeBuilder {
    for (section, name, _) in listed_cells() {
        if keep.contains(&name) && section.is_none() {
            continue;
        }
        sb = match section {
            None => sb.set(CellAddr::direct(name), marker()),
            Some(s) if skip_sections.contains(&s) => sb,
            Some(s) => {
                let row = if keying(s) == Keying::Named { RowKey::name("Row_1") } else { RowKey::Ix(0) };
                let ix = (s == "Geometry").then_some(0);
                sb.set(CellAddr::in_row(s, ix, row, name), marker())
            }
        };
    }
    sb
}

#[test]
fn every_cell_the_catalogue_lists_is_themed_and_a_shape_that_keeps_its_place_and_outline_still_draws() {
    let kept: Vec<&str> = SCHEME_CELLS.iter().chain(PLACEMENT.iter()).copied().collect();
    // The geometry stays as it was built, so there is an outline to fill; every other section holds the marker in a row.
    let sb = everything_themed(on_a_theme(rect(1, 1.0, 1.0, 2.0, 2.0)).text("Hi"), &kept, &["Geometry"]);
    let sb = PLACEMENT.iter().zip([1.0, 1.0, 2.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0]).fold(sb, |sb, (name, v)| sb.cell(name, &v.to_string()));
    let pic = draw(&one_page(vec![sb]));
    assert_nothing_lost(&pic);
    assert!(pic.ink_pixels() > 0, "the page is not blank: {:?}", pic.r.skipped);
    // More than 20, where it was more than 40: answers a cell in no table of `THEMEVAL` with the root's cell, under a theme as
    // without, so the cells that no table holds are no longer among the unanswered. The cells a table holds and a file with no theme part cannot
    // answer still are.
    assert!(pic.r.count(SkipKind::ThemeUnanswered) > 20, "{:?}", pic.r.skipped);
}

#[test]
fn every_cell_the_catalogue_lists_is_themed_including_the_ones_that_place_the_shape_and_nothing_is_lost() {
    // The shape has no size and no outline that reads, and a plain shape beside it still draws.
    let sb = everything_themed(on_a_theme(rect(1, 1.0, 1.0, 2.0, 2.0)).text("Hi"), &SCHEME_CELLS, &[]);
    let plain = rect(2, 3.25, 1.0, 0.5, 2.0).cell("FillForegnd", "#FF0000");
    let pic = draw(&one_page(vec![sb, plain]));
    assert_nothing_lost(&pic);
    assert!(pic.is_red(3.5, 2.0), "the shape after it is drawn: {:?}", pic.r.skipped);
    assert!(pic.r.count(SkipKind::ThemeUnanswered) > 10, "{:?}", pic.r.skipped);
}

#[test]
fn a_renderer_goes_on_after_a_themed_page() {
    // The same renderer, the same kept caches: a second frame is as good as the first.
    let mut renderer = diacraft_render::Renderer::new();
    let doc = one_page(vec![themed_shape(1, 1.0)]);
    let first = draw_with(&mut renderer, &doc);
    let second = draw_with(&mut renderer, &doc);
    assert_nothing_lost(&first);
    assert_nothing_lost(&second);
    assert_eq!(first.r.rgba_premul, second.r.rgba_premul);
}
