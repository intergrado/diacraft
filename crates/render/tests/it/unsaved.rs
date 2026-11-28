//! A cell with a formula and no saved value is worked out and drawn, not read from a default. The renderer draws what a file saved
//! And a cell with no `V` anywhere on the way saved nothing: its formula, found by inheritance, is the only
//! thing that says what it holds, and it runs on the sheet that asked, with the cells it reads worked out as well (a cell it reads may have no
//! saved value either). A result that is no value (an error, a function this evaluator lacks) leaves the cell as it was read before: the root style's
//! value for a shape that finds none, else the default.

use diacraft_doc::{Cell, CellAddr, RowKey};
use diacraft_render::SkipKind;

use crate::support::*;

fn unsaved(formula: &str) -> Cell {
    Cell::with_formula(formula, None)
}

/// A rectangle 2 inches square at (1, 1): its centre is (2, 2).
fn square(id: u32) -> diacraft_testkit::build::ShapeBuilder {
    rect(id, 1.0, 1.0, 2.0, 2.0)
}

#[test]
fn a_colour_formula_with_no_saved_value_is_worked_out() {
    // The root style's fill is white. A shape whose `FillForegnd` holds a formula and no value finds no `V` on the way, and used to read the root's.
    let sb = square(1).set(CellAddr::direct("FillForegnd"), unsaved("RGB(10,20,30)"));
    let pic = draw(&one_page(vec![sb]));
    assert!(pic.is(2.0, 2.0, (10, 20, 30), 2), "{:?}", pic.at(2.0, 2.0));
}

#[test]
fn the_same_formula_with_a_saved_value_stands_as_saved() {
    // Draw what was saved: the value beside a formula is the cell's, and nothing is worked out for it.
    let sb = square(1).formula("FillForegnd", "RGB(10,20,30)", "#FF0000");
    let pic = draw(&one_page(vec![sb]));
    assert!(pic.is_red(2.0, 2.0), "{:?}", pic.at(2.0, 2.0));
}

#[test]
fn a_formula_found_on_a_style_with_no_value_is_worked_out_on_the_shape_that_asked() {
    // The style holds the formula and no value; the shape links the style and holds `F="Inh"` with no value of its own. The formula reads the shape's own cell.
    let style = diacraft_testkit::build::StyleBuilder::new().name("Tinted").set(CellAddr::direct("FillForegnd"), unsaved("RGB(Width*100,0,0)"));
    let sb = square(1).links(None, Some(7), None).set(CellAddr::direct("FillForegnd"), Cell::inherited(None));
    let doc = base().style(7, style).page(1, page(4.0, 4.0).shape(sb)).build();
    let pic = draw(&doc);
    assert!(pic.is(2.0, 2.0, (200, 0, 0), 2), "Width is 2 inches on the shape: {:?}", pic.at(2.0, 2.0));
}

#[test]
fn a_cell_that_reads_a_cell_with_no_saved_value_reads_it_worked_out() {
    // `User.Red` has a formula and no value. Read as saved it would be 0 and the fill black; worked out it is 155.
    let sb = square(1)
        .set(CellAddr::in_row("User", None, RowKey::name("Red"), "Value"), unsaved("100+55"))
        .set(CellAddr::direct("FillForegnd"), unsaved("RGB(User.Red,0,0)"));
    let pic = draw(&one_page(vec![sb]));
    assert!(pic.is(2.0, 2.0, (155, 0, 0), 2), "{:?}", pic.at(2.0, 2.0));
}

#[test]
fn a_number_formula_with_no_saved_value_is_worked_out() {
    // A line weight of one tenth of an inch, as a formula alone.
    let sb = square(1).cell("FillPattern", "0").set(CellAddr::direct("LineWeight"), unsaved("0.1")).cell("LineColor", "#0000FF");
    let pic = draw(&one_page(vec![sb]));
    assert!(pic.is(1.0, 2.0, (0, 0, 255), 4), "the line is 0.1 inch wide, centred on the left edge: {:?}", pic.at(1.0, 2.0));
    assert!(pic.is(1.04, 2.0, (0, 0, 255), 4), "{:?}", pic.at(1.04, 2.0));
}

#[test]
fn a_formula_that_gives_no_value_leaves_the_cell_as_it_was_read() {
    // An error, a function this evaluator lacks, and a formula that does not parse: the shape finds nothing and takes the root style's white, and the
    // frame is whole.
    for formula in ["1/0", "NOSUCHFUNCTION(1)", "RGB(", "((("] {
        let sb = square(1).set(CellAddr::direct("FillForegnd"), unsaved(formula));
        let pic = draw(&one_page(vec![sb]));
        assert_eq!(pic.r.count(SkipKind::Recovered), 0, "{formula}: {:?}", pic.r.skipped);
        assert_eq!(pic.r.count(SkipKind::ShapeFailed), 0, "{formula}: {:?}", pic.r.skipped);
        assert!(pic.is_white(2.0, 2.0), "{formula}: {:?}", pic.at(2.0, 2.0));
    }
}

#[test]
fn cells_that_read_each_other_are_cut_and_the_frame_is_whole() {
    // A cycle of cells with no saved value: no hang, and the cell is left as it was read.
    let sb = square(1)
        .set(CellAddr::in_row("User", None, RowKey::name("A"), "Value"), unsaved("User.B+1"))
        .set(CellAddr::in_row("User", None, RowKey::name("B"), "Value"), unsaved("User.A+1"))
        .set(CellAddr::direct("FillForegnd"), unsaved("RGB(User.A,0,0)"));
    let pic = draw(&one_page(vec![sb]));
    assert_eq!(pic.r.count(SkipKind::Recovered), 0, "{:?}", pic.r.skipped);
    assert_eq!(pic.r.count(SkipKind::ShapeFailed), 0, "{:?}", pic.r.skipped);
}

#[test]
fn a_long_chain_of_cells_with_no_saved_value_is_worked_out_within_the_budget() {
    // Each cell of the chain reads the one before. The chain is walked once and the frame is whole, whether or not the budget of the frame reaches its end.
    let mut sb = square(1).set(CellAddr::in_row("User", None, RowKey::name("R0"), "Value"), unsaved("1"));
    for i in 1..200u32 {
        sb = sb.set(CellAddr::in_row("User", None, RowKey::name(&format!("R{i}")), "Value"), unsaved(&format!("User.R{}+1", i - 1)));
    }
    let sb = sb.set(CellAddr::direct("FillForegnd"), unsaved("RGB(10,20,30)"));
    let pic = draw(&one_page(vec![sb]));
    assert_eq!(pic.r.count(SkipKind::Recovered), 0, "{:?}", pic.r.skipped);
    assert!(pic.is(2.0, 2.0, (10, 20, 30), 2), "{:?}", pic.at(2.0, 2.0));
}

#[test]
fn a_position_given_as_a_formula_alone_is_drawn_there() {
    // The cells that place a shape are cells like any other: a `PinX` of `1+1` and no value stands the shape at 2 inches, where it stood at 0 when an unsaved
    // cell read as the default.
    let sb = rect(1, 0.0, 1.0, 1.0, 1.0).cell("FillForegnd", "#FF0000").set(CellAddr::direct("PinX"), unsaved("1+1"));
    let pic = draw(&one_page(vec![sb]));
    assert!(pic.is_red(2.5, 1.5), "{:?}", pic.at(2.5, 1.5));
    assert!(pic.is_white(0.5, 1.5), "nothing is drawn at the origin: {:?}", pic.at(0.5, 1.5));
}

/// A rectangle 2 inches square whose geometry holds formulas and no values: `Width` and `Height` for the far corner, `0` for the near one.
fn rect_of_formulas(id: u32) -> diacraft_testkit::build::ShapeBuilder {
    let rows = [("MoveTo", "0", "0"), ("LineTo", "Width", "0"), ("LineTo", "Width", "Height"), ("LineTo", "0", "Height"), ("LineTo", "0", "0")];
    let mut sb = shape_at(id, 1.0, 1.0, 2.0, 2.0).cell("FillForegnd", "#FF0000");
    for (i, (kind, x, y)) in rows.into_iter().enumerate() {
        let key = RowKey::Ix(i as u32);
        sb = sb.row_type("Geometry", Some(0), key.clone(), kind);
        sb = sb.set(CellAddr::in_row("Geometry", Some(0), key.clone(), "X"), unsaved(x));
        sb = sb.set(CellAddr::in_row("Geometry", Some(0), key, "Y"), unsaved(y));
    }
    sb
}

#[test]
fn an_outline_given_as_formulas_alone_is_drawn() {
    // The cells of the rows of a shape's geometry are cells like any other: `X` of `Width` and no value is 2 inches, where it read 0 and the outline was a point.
    let pic = draw(&one_page(vec![rect_of_formulas(1)]));
    assert!(pic.is_red(2.0, 2.0), "the outline is the square of 2 inches: {:?}", pic.at(2.0, 2.0));
    assert!(pic.is_red(1.1, 1.1) && pic.is_red(2.9, 2.9), "from corner to corner");
    assert!(pic.is_white(0.5, 0.5), "and nothing outside it");
}

/// `n` small squares on the page, each with a fill and a line weight that hold a formula and no value.
fn many_unsaved(n: u32) -> diacraft_doc::Document {
    let shapes = (0..n)
        .map(|i| {
            let (col, row) = (f64::from(i % 50), f64::from(i / 50));
            rect(i + 1, 0.05 + col * 0.075, 0.05 + row * 0.075, 0.06, 0.06)
                .set(CellAddr::direct("FillForegnd"), unsaved("RGB(10,20,30)"))
                .set(CellAddr::direct("LineWeight"), unsaved("0.001+0.001"))
        })
        .collect();
    one_page(shapes)
}

#[test]
fn a_page_of_shapes_with_cells_that_have_no_saved_value_is_drawn_whole_and_the_work_is_bounded_by_the_frame_s_steps() {
    // Every cell of 500 shapes is worked out once for its sheet; whatever the frame's steps do not reach is read as it was before, so the frame is whole.
    let pic = draw(&many_unsaved(500));
    assert_eq!(pic.r.count(SkipKind::Recovered), 0, "{:?}", pic.r.skipped);
    assert_eq!(pic.r.count(SkipKind::ShapeFailed), 0, "{:?}", pic.r.skipped);
    assert!(pic.is(0.08, 0.08, (10, 20, 30), 2), "the first shape is worked out: {:?}", pic.at(0.08, 0.08));
}

#[test]
#[ignore = "a clock, which the counted tests of calc and the test above stand for: run it by hand on a quiet machine"]
fn timed_a_page_of_three_thousand_shapes_with_cells_that_have_no_saved_value_is_drawn_in_seconds() {
    let doc = many_unsaved(3_000);
    let started = std::time::Instant::now();
    let pic = draw(&doc);
    let took = started.elapsed();
    assert_eq!(pic.r.count(SkipKind::Recovered), 0, "{:?}", pic.r.skipped);
    assert!(took < std::time::Duration::from_secs(10), "a frame of 3,000 shapes with 6,000 unsaved cells took {took:?}");
}
