//! The shapes and boxes of the line ends:
//! the pixels of what `ends.rs` plans, which its own tests measure as geometry.

use std::sync::Arc;

use diacraft_doc::{Cell, CellAddr, Document, RowKey, Value};
use diacraft_render::limits::{MAX_END_ROWS, MAX_END_SECTIONS, WORK_PER_DRAWN_SHAPE, WORK_PER_END_ROW};
use diacraft_render::{Renderer, SkipKind};
use diacraft_testkit::build::{MasterBuilder, ShapeBuilder};

use crate::support::*;

/// A horizontal line 3 inches long from (0.5, `y`) to (3.5, `y`), weight `weight`, a flat cap, with `number` at the end at size class `size`.
fn ended(id: u32, y: f64, weight: f64, number: i64, size: i64) -> ShapeBuilder {
    geometry_of(shape_at(id, 0.5, y, 3.0, 0.0), vec![("MoveTo", vec![("X", 0.0), ("Y", 0.0)]), ("LineTo", vec![("X", 3.0), ("Y", 0.0)])])
        .cell("FillPattern", "0")
        .cell("LineWeight", &weight.to_string())
        .cell("LinePattern", "1")
        .cell("LineCap", "1")
        .cell("LineColor", "#000000")
        .cell("EndArrow", &number.to_string())
        .cell("EndArrowSize", &size.to_string())
}

/// The pixel `t` units back from the tip of a mark whose line ends at (3.5, `y`), and `a` units across the line, `s` inches to the unit.
fn at_s(y: f64, s: f64, t: f64, a: f64) -> (f64, f64) {
    (3.5 - t * s, y + a * s)
}

#[test]
fn number_8_is_a_solid_head_with_a_hollowed_back_and_19_the_same_head_hollow_both_in_the_box_of_number_4() {
    // Class 6 and a weight of 0.02 inch: S = 0.25 + 0.02 = 0.27 inch, so a unit is 27 pixels and every probe below is at least 3 pixels
    // from an edge. The three lines are 4 (the triangle, 2 back and 1 across), 8 and 19 (its back hollowed to 1.5).
    let s = 0.25 + 0.02;
    let doc = one_page(vec![ended(1, 3.0, 0.02, 4, 6), ended(2, 2.0, 0.02, 8, 6), ended(3, 1.0, 0.02, 19, 6)]);
    let pic = draw(&doc);
    let dark = |y: f64, t: f64, a: f64| {
        let (x, y) = at_s(y, s, t, a);
        pic.is_dark(x, y)
    };
    let white = |y: f64, t: f64, a: f64| {
        let (x, y) = at_s(y, s, t, a);
        pic.is_white(x, y)
    };
    let (four, eight, nineteen) = (3.0, 2.0, 1.0);
    // The hollowed back: inside 4's triangle at 1.8 back and 0.3 across, in the notch of 8 and 19. The mark of 8 is solid on both sides of the notch.
    assert!(dark(four, 1.8, 0.3), "4 is whole at its back");
    assert!(white(eight, 1.8, 0.3) && white(nineteen, 1.8, 0.3), "8 and 19 are hollowed at the back, where 5's apex is at 1.75");
    assert!(dark(eight, 1.8, 0.75) && dark(eight, 1.8, -0.75), "and solid beside the notch");
    // Solid or hollow: the same place near the tip is ink for 8 and 4, and paper inside the ring of 19; the outline is there on 19.
    assert!(dark(eight, 1.0, 0.25) && dark(four, 1.0, 0.25));
    assert!(white(nineteen, 1.0, 0.25), "19 is hollow: its inside shows the paper");
    assert!(dark(nineteen, 1.0, 0.5) && dark(nineteen, 1.0, -0.5), "and its outline is one line weight");
    // The same box as 4: no ink across the line beyond a unit, and nothing past the tip.
    for y in [four, eight, nineteen] {
        assert!(
            white(y, 1.0, 0.75) && white(y, 1.0, -0.75),
            "the spread of 2 units, 1 each side: nothing at 0.75 across at 1 back (the edge is at 0.5)"
        );
        assert!(white(y, 1.0, 1.2) && white(y, 0.2, 1.2));
        assert!(white(y, -0.3, 0.0), "nothing past the vertex");
    }
    // The shaft: 19's runs up to the apex at 1.5 and not through the ring; 8's meets the solid.
    assert!(dark(nineteen, 1.7, 0.0), "the shaft reaches the apex");
    assert!(white(nineteen, 1.3, 0.0), "and stops there, not running through the hollow head");
    assert!(dark(eight, 1.3, 0.0), "8's inside is solid");
}

#[test]
fn class_6_is_eighteen_points_and_a_class_outside_0_to_6_is_drawn_as_class_2_and_says_so() {
    // Number 4 at a weight of 0.01 inch: class 6's S is 0.25 + 0.01 = 0.26 inch and its head reaches 0.52 back; class 2's S is 0.045.
    let (s6, s2) = (0.26, 0.045);
    let drawn = |size: i64| draw(&one_page(vec![ended(1, 2.0, 0.01, 4, size)]));
    let six = drawn(6);
    let (x, y) = at_s(2.0, s6, 1.8, 0.3);
    assert!(six.is_dark(x, y), "inside class 6's head, 0.47 back and 0.08 across");
    let (x, y) = at_s(2.0, s6, 1.9, 0.8);
    assert!(six.is_dark(x, y) && !six.is_dark(at_s(2.0, s6, 2.1, 0.5).0, at_s(2.0, s6, 2.1, 0.5).1), "and its back is at 2 units, 0.52 inch");
    let two = drawn(2);
    let (x, y) = at_s(2.0, s2, 1.8, 0.3);
    assert!(two.is_dark(x, y));
    assert!(two.is_white(at_s(2.0, s6, 1.8, 0.3).0, at_s(2.0, s6, 1.8, 0.3).1), "class 2's head is only 0.09 inch long");
    assert_ne!(six.r.rgba_premul, two.r.rgba_premul);
    for outside in [-1, 7, 8, 100, 254] {
        let other = drawn(outside);
        assert_eq!(other.r.rgba_premul, two.r.rgba_premul, "class {outside} is drawn as class 2");
        assert_eq!(other.r.count(SkipKind::LineEndApproximate), 1, "class {outside}: warns once");
        let first = other.r.skipped.iter().find(|s| s.kind == SkipKind::LineEndApproximate).map(|s| s.first.clone()).unwrap_or_default();
        assert!(first.contains("size class"), "{first}");
    }
    for inside in 0..=6 {
        assert_eq!(drawn(inside).r.count(SkipKind::LineEndApproximate), 0, "class {inside} is a class");
    }
}

#[test]
fn a_size_class_that_is_no_class_is_not_reported_on_a_line_with_no_end() {
    // The class cell of an end that is not there says nothing: 0 draws no mark, so there is no size to be wrong.
    let none = draw(&one_page(vec![ended(1, 2.0, 0.01, 0, 9)]));
    assert_eq!(none.r.count(SkipKind::LineEndApproximate), 0);
    // A begin with a class that is no class, and no end: the begin's class is the one reported.
    let begin =
        geometry_of(shape_at(1, 0.5, 2.0, 3.0, 0.0), vec![("MoveTo", vec![("X", 0.0), ("Y", 0.0)]), ("LineTo", vec![("X", 3.0), ("Y", 0.0)])])
            .cell("FillPattern", "0")
            .cell("LineColor", "#000000")
            .cell("BeginArrow", "4")
            .cell("BeginArrowSize", "9");
    assert_eq!(draw(&one_page(vec![begin])).r.count(SkipKind::LineEndApproximate), 1);
}

/// A number that no art is drawn for (46 and up, 255, a negative one) draws nothing, so there is no mark for its size class to be wrong at:
/// no size note. 254 is the custom end, drawn as number 4, and says so.
#[test]
fn a_size_class_that_is_no_class_is_reported_only_where_a_mark_is_drawn_at_it() {
    for number in [46, 99, 255, 1000, -1] {
        let pic = draw(&one_page(vec![ended(1, 2.0, 0.01, number, 9)]));
        assert_eq!(pic.r.count(SkipKind::LineEndApproximate), 0, "number {number} draws no mark, so its size says nothing");
    }
    // (Numbers whose box no source measures say so too, so they are not among these: 45 would count twice.)
    for number in [1, 4, 43, 254] {
        let pic = draw(&one_page(vec![ended(1, 2.0, 0.01, number, 9)]));
        assert_eq!(pic.r.count(SkipKind::LineEndApproximate), 1, "number {number} is drawn, at class 2 in place of 9");
        let first = pic.r.skipped.iter().find(|s| s.kind == SkipKind::LineEndApproximate).map(|s| s.first.clone()).unwrap_or_default();
        assert!(first.contains("size class"), "{first}");
    }
    // Each end is its own: a begin that draws nothing keeps its class from being reported, and the end's class is a class.
    let two = geometry_of(shape_at(1, 0.5, 2.0, 3.0, 0.0), vec![("MoveTo", vec![("X", 0.0), ("Y", 0.0)]), ("LineTo", vec![("X", 3.0), ("Y", 0.0)])])
        .cell("FillPattern", "0")
        .cell("LineColor", "#000000")
        .cell("BeginArrow", "99")
        .cell("BeginArrowSize", "9")
        .cell("EndArrow", "4")
        .cell("EndArrowSize", "2");
    assert_eq!(draw(&one_page(vec![two])).r.count(SkipKind::LineEndApproximate), 0, "the begin draws none and the end's class is a class");
}

#[test]
fn a_line_end_whose_box_no_source_measures_is_reported_and_a_measured_one_is_not() {
    // The dashes: 8, 18, 19, 31, 32, 34 to 38, 40, 44 and 45. 43 was measured (0 to 4) and is not among them.
    let unmeasured = [8, 18, 19, 31, 32, 34, 35, 36, 37, 38, 40, 44, 45];
    for number in 1..=45 {
        let pic = draw(&one_page(vec![ended(1, 2.0, 0.01, number, 2)]));
        let want = usize::from(unmeasured.contains(&number));
        assert_eq!(pic.r.count(SkipKind::LineEndApproximate), want, "number {number}");
    }
}

/// A 5-column page of every line end from 1 to 45 at size class 3 and a weight of 0.015 inch, each with its number above it.
fn gallery() -> diacraft_doc::Document {
    let mut p = page(8.0, 10.0);
    for number in 1..=45_i64 {
        let (row, col) = ((number - 1) / 5, (number - 1) % 5);
        let (x, y) = (0.15 + 1.55 * col as f64, 9.4 - row as f64);
        let id = u32::try_from(number).unwrap_or(1) * 2;
        p = p.shape(
            geometry_of(shape_at(id, x, y, 1.25, 0.0), vec![("MoveTo", vec![("X", 0.0), ("Y", 0.0)]), ("LineTo", vec![("X", 1.25), ("Y", 0.0)])])
                .cell("FillPattern", "0")
                .cell("LineWeight", "0.015")
                .cell("LinePattern", "1")
                .cell("LineCap", "1")
                .cell("LineColor", "#000000")
                .cell("EndArrow", &number.to_string())
                .cell("EndArrowSize", "3"),
        );
        p = p.shape(
            rect(id + 1, x, y + 0.12, 1.25, 0.25)
                .cell("FillPattern", "0")
                .cell("LinePattern", "0")
                .text(&number.to_string())
                .row_cell("Character", None, RowKey::Ix(0), "Size", "0.14")
                .row_cell("Character", None, RowKey::Ix(0), "Color", "#000000"),
        );
    }
    base().page(1, p).build()
}

#[test]
fn every_line_end_from_1_to_45_draws_a_mark_wider_than_its_line_and_the_gallery_can_be_written() {
    let doc = gallery();
    let pic = draw(&doc);
    for number in 1..=45_i64 {
        let (row, col) = ((number - 1) / 5, (number - 1) % 5);
        let (x, y) = (0.15 + 1.55 * col as f64, 9.4 - row as f64);
        // The mark lies within 0.4 inch of the end vertex at (x + 1.25, y): is there ink more than 0.03 inch from the axis, where a 0.015 line
        // has none, and not above the label (0.12 up)?
        let mut ink = 0;
        for px in 0..40 {
            for py in -10..=10 {
                let (qx, qy) = (x + 1.25 - 0.40 + f64::from(px) * 0.01 + 0.005, y + f64::from(py) * 0.01 + 0.005);
                if (qy - y).abs() > 0.03 && pic.is_dark(qx, qy) {
                    ink += 1;
                }
            }
        }
        assert!(ink > 0, "number {number} draws no mark off the line");
    }
    // `DIACRAFT_ENDS_GALLERY=/absolute/path.png cargo nextest run ... every_line_end_from_1_to_45` writes the picture to look at.
    if let Ok(path) = std::env::var("DIACRAFT_ENDS_GALLERY") {
        let out = diacraft_render::export::export_png(&mut diacraft_render::Renderer::new(), &doc, 1, 200.0).expect("exports");
        std::fs::write(path, out.bytes).expect("writes the gallery");
    }
}

// ---- A line end that is a master (DD18). ----
//
// A master is a line end when its `PatternFlags` holds 2. Its pin goes on the line's end point and it is drawn pointing right, as tall as the line's
// weight at Medium size. The tests below use DiaCraft's own filled diamond and masters of other forms a file may hold.

/// The side of the filled diamond's alignment box, in inches (its `Width` and its `Height`: `h`).
const H: f64 = 0.05;

/// The filled diamond's vertices in the master's own inches, them: the tip on the pin at the middle of the box's right edge, the back `20 h` behind the tip,
/// the sides `10 h` behind it and `5 h` across the line to either side.
fn diamond() -> Vec<(f64, f64)> {
    vec![(H, H / 2.0), (-9.0 * H, 5.5 * H), (-19.0 * H, H / 2.0), (-9.0 * H, -4.5 * H)]
}

/// Section `ix` of a master's shape: the closed polygon of `points` (the last row back at the first), with `NoLine` when `no_line`.
fn closed_section(mut shape: ShapeBuilder, ix: u32, points: &[(f64, f64)], no_line: bool) -> ShapeBuilder {
    for (i, (x, y)) in points.iter().chain(points.first()).enumerate() {
        let row = RowKey::Ix(u32::try_from(i).unwrap());
        shape = shape
            .row_type("Geometry", Some(ix), row.clone(), if i == 0 { "MoveTo" } else { "LineTo" })
            .row_cell("Geometry", Some(ix), row.clone(), "X", &x.to_string())
            .row_cell("Geometry", Some(ix), row, "Y", &y.to_string());
    }
    for (flag, on) in [("NoFill", false), ("NoLine", no_line), ("NoShow", false)] {
        shape = shape.set(CellAddr::in_section("Geometry", Some(ix), flag), Cell::constant(Value::from_text(if on { "1" } else { "0" })));
    }
    shape
}

/// The shape of a line-end master: `w` by `h`, its pin at `pin`, drawn pointing right, one closed section for each of `sections`. It is red and outlined in its own cells, so that a
/// test sees that the end takes the line's colour and not the master's.
fn end_shape(w: f64, h: f64, pin: (f64, f64), sections: &[Vec<(f64, f64)>]) -> ShapeBuilder {
    let mut shape = shape_at(5, w / 2.0, h / 2.0, w, h)
        .cell("LocPinX", &pin.0.to_string())
        .cell("LocPinY", &pin.1.to_string())
        .cell("FillPattern", "1")
        .cell("FillForegnd", "#FF0000")
        .cell("LinePattern", "1");
    for (ix, points) in sections.iter().enumerate() {
        shape = closed_section(shape, u32::try_from(ix).unwrap(), points, true);
    }
    shape
}

/// DiaCraft's filled diamond as a master shape.
fn diamond_shape() -> ShapeBuilder {
    end_shape(H, H, (H, H / 2.0), &[diamond()])
}

/// A 4 by 4 inch page of `shapes` and these masters (`ID` 1 up): the name a `USE` finds it by, its `PatternFlags`, and its shape.
fn with_ends(shapes: Vec<ShapeBuilder>, masters: Vec<(&str, u32, ShapeBuilder)>) -> Document {
    let mut page = page(4.0, 4.0);
    for shape in shapes {
        page = page.shape(shape);
    }
    let mut doc = base();
    let mut flags = Vec::new();
    for (i, (name, f, shape)) in masters.into_iter().enumerate() {
        doc = doc.master(u32::try_from(i + 1).unwrap(), MasterBuilder::new().name(name).shape(shape));
        flags.push(f);
    }
    let mut doc = doc.page(1, page).build();
    for (i, f) in flags.into_iter().enumerate() {
        if let Some(master) = doc.masters.get_mut(i) {
            Arc::make_mut(master).pattern_flags = Some(f);
        }
    }
    doc
}

/// A horizontal line from (0.5, `y`) to (3.5, `y`) of `weight` inches, flat cap, black, whose `cell` (`EndArrow` or `BeginArrow`) is `USE("<name>")` (value 254) at size class `size`.
fn line_ending_in(id: u32, y: f64, weight: f64, cell: &str, name: &str, size: i64) -> ShapeBuilder {
    geometry_of(shape_at(id, 0.5, y, 3.0, 0.0), vec![("MoveTo", vec![("X", 0.0), ("Y", 0.0)]), ("LineTo", vec![("X", 3.0), ("Y", 0.0)])])
        .cell("FillPattern", "0")
        .cell("LineWeight", &weight.to_string())
        .cell("LinePattern", "1")
        .cell("LineCap", "1")
        .cell("LineColor", "#000000")
        .formula(cell, &format!("USE(\"{name}\")"), "254")
        .cell(&format!("{cell}Size"), &size.to_string())
}

/// The same line with a numbered end: what a custom end that cannot be drawn is drawn as (number 4).
fn line_ending_in_number(id: u32, y: f64, weight: f64, number: i64, size: i64) -> ShapeBuilder {
    ended(id, y, weight, number, size).cell("LineWeight", &weight.to_string())
}

/// What a frame of `doc` was charged, in the units of the work budget.
fn work_of(doc: &Document) -> u64 {
    let mut renderer = Renderer::new();
    draw_with(&mut renderer, doc);
    renderer.work_done()
}

#[test]
fn a_line_end_master_is_drawn_by_its_pin_as_tall_as_the_weight_at_medium_size() {
    // V18 in pixels. The box is 0.05 inch square and the line 0.05 inch heavy, so the master is drawn at its own size: a diamond pointing right with its tip on the end point
    // (3.5, 2), 20 box heights (1 inch) long, so the back is at x 2.5, and 10 across (0.5 inch) at its widest, 0.5 inch behind the tip: the half height is half the distance from the
    // tip up to there and then half the distance to the back. Drawn in number 22's box at this weight (S 0.085, a diamond 0.38 long and 0.19 across) it fails every probe below.
    let doc = with_ends(vec![line_ending_in(1, 2.0, 0.05, "EndArrow", "Filled Diamond", 2)], vec![("Filled Diamond", 2, diamond_shape())]);
    let pic = draw(&doc);
    assert!(pic.is_dark(3.0, 2.2) && pic.is_dark(3.0, 1.8), "inside the diamond at its widest, 0.5 back from the tip and 0.2 across");
    assert!(pic.is_white(3.0, 2.3) && pic.is_white(3.0, 1.7), "and not 0.3 across, where it is 0.25 at the most");
    assert!(pic.is_dark(3.1, 2.18) && pic.is_white(3.1, 2.23), "0.4 back from the tip the half height is 0.2");
    assert!(pic.is_dark(2.7, 2.08) && pic.is_white(2.7, 2.13), "0.8 back from the tip, 0.2 from the back, the half height is 0.1");
    assert!(pic.is_white(3.55, 2.0) && pic.is_white(3.51, 2.2), "nothing past the tip: the pin is on the end point");
    assert!(pic.is_dark(3.2, 2.1), "in the line's colour: the master's own red is not used");
    assert_eq!(pic.r.count(SkipKind::CustomLineEnd), 0, "a master that is a line end is drawn as its master, not as number 4: {:?}", pic.r.skipped);
    // Half the weight, half the size (the box is as tall as the weight): 0.5 inch long, 0.25 across at its widest, 0.25 behind the tip.
    let half = draw(&with_ends(vec![line_ending_in(1, 2.0, 0.025, "EndArrow", "Filled Diamond", 2)], vec![("Filled Diamond", 2, diamond_shape())]));
    assert!(half.is_dark(3.25, 2.1) && half.is_white(3.25, 2.16), "the widest point: 0.125 across, at 0.25 back");
    assert!(half.is_white(2.9, 2.08), "and its back is at 3.0, so the 0.8 back of the full-size one is not ink");
    // Twice the weight, twice the size: 2 inches long and 1 across, widest 1.0 behind the tip.
    let twice = draw(&with_ends(vec![line_ending_in(1, 2.0, 0.1, "EndArrow", "Filled Diamond", 2)], vec![("Filled Diamond", 2, diamond_shape())]));
    assert!(twice.is_dark(2.5, 2.4) && twice.is_white(2.5, 2.6), "the widest point: 0.5 across, at 1.0 back");
    assert!(twice.is_dark(1.6, 2.04), "near the back, 1.9 behind the tip");
}

#[test]
fn a_custom_end_at_another_size_class_is_the_weights_share_of_s_over_the_mediums_and_the_begin_points_out_of_the_line() {
    // The project rules. Class n draws the box as tall as the weight times S(n) over S(2), the unit at the same weight: at 0.05 inch S(0), S(2) and S(4) are 0.07, 0.085
    // and 0.105 inch, so the box is 0.0412, 0.05 and 0.0618 inch and the diamond 0.82, 1.0 and 1.24 inch long.
    let masters = || vec![("Filled Diamond", 2, diamond_shape())];
    let at = |size: i64| draw(&with_ends(vec![line_ending_in(1, 2.0, 0.05, "EndArrow", "Filled Diamond", size)], masters()));
    let (small, medium, large) = (at(0), at(2), at(4));
    // 0.6 back from the tip and 0.27 across: inside only the large one (half height 0.3 there; 0.2 for the medium).
    assert!(large.is_dark(2.9, 2.27) && medium.is_white(2.9, 2.27) && small.is_white(2.9, 2.27));
    // 0.9 back and 0.04 across: inside the medium one (half height 0.05) and past the back of the small one (0.82).
    assert!(medium.is_dark(2.6, 2.04) && small.is_white(2.6, 2.04) && large.is_dark(2.6, 2.04));
    // The begin points out of the line, away from its other vertices, as places a numbered mark at either end: the tip is on (0.5, 2) and the body runs toward +x.
    let begin = draw(&with_ends(vec![line_ending_in(1, 2.0, 0.05, "BeginArrow", "Filled Diamond", 2)], masters()));
    assert!(begin.is_dark(1.0, 2.2) && begin.is_white(1.0, 2.3), "the widest point is 0.5 along from the begin");
    assert!(begin.is_white(0.45, 2.0) && begin.is_white(0.49, 2.2), "nothing before the begin");
    // A line at 45 degrees: the end turns with the line, so the axis of the diamond is the line. The tip (3, 3), the back (2.293, 2.293), the widest 0.5 back at (2.646, 2.646).
    let slanted =
        geometry_of(shape_at(1, 1.0, 1.0, 2.0, 2.0), vec![("MoveTo", vec![("X", 0.0), ("Y", 0.0)]), ("LineTo", vec![("X", 2.0), ("Y", 2.0)])])
            .cell("FillPattern", "0")
            .cell("LineWeight", "0.05")
            .cell("LinePattern", "1")
            .cell("LineCap", "1")
            .cell("LineColor", "#000000")
            .formula("EndArrow", "USE(\"Filled Diamond\")", "254");
    let pic = draw(&with_ends(vec![slanted], masters()));
    let side = std::f64::consts::FRAC_1_SQRT_2;
    let across = |t: f64, a: f64| (3.0 - t * side + a * -side, 3.0 - t * side + a * side);
    let (x, y) = across(0.5, 0.2);
    assert!(pic.is_dark(x, y), "0.2 across the axis at the widest point");
    let (x, y) = across(0.5, 0.3);
    assert!(pic.is_white(x, y), "and not 0.3");
    let (x, y) = across(0.5, -0.2);
    assert!(pic.is_dark(x, y), "on either side");
    assert_eq!(pic.r.count(SkipKind::CustomLineEnd), 0);
}

#[test]
fn a_files_custom_end_is_drawn_from_its_master_and_a_master_that_cannot_be_drawn_as_one_is_drawn_as_4_with_the_note() {
    // A master of a file that is a line end by its `PatternFlags` is drawn as its master says, whatever file it came from. One that is not a line end (no 2 in the flags), one
    // the `USE` does not find, a plain 254 with no master named, and a master with no geometry are drawn as number 4 at the cell's size, and the frame says so.
    // The box is 0.05 inch and the square 0.1 inch: the geometry is larger than the box, ("a master meant for every weight").
    let square = || end_shape(0.05, 0.05, (0.05, 0.025), &[vec![(-0.05, -0.025), (0.05, -0.025), (0.05, 0.075), (-0.05, 0.075)]]);
    let four = draw(&one_page(vec![line_ending_in_number(1, 2.0, 0.05, 4, 2)]));
    // The box is 0.05 inch and the weight 0.05: the master is drawn at its own size, a square 0.1 inch each way behind the end point (x 3.4 to 3.5, y 1.95 to 2.05), twice as tall as
    // the line is heavy, with the pin on the middle of its right edge, and the line stops at its back.
    let drawn = draw(&with_ends(vec![line_ending_in(1, 2.0, 0.05, "EndArrow", "Tick", 2)], vec![("Tick", 2, square())]));
    assert!(drawn.is_dark(3.45, 2.04) && drawn.is_dark(3.45, 1.96), "the square sits behind the end point and is taller than the line");
    assert!(
        drawn.is_white(3.45, 2.07) && drawn.is_white(3.45, 1.93) && drawn.is_white(3.55, 2.0),
        "and no taller than 0.1, and not past the end point"
    );
    assert!(drawn.is_white(3.35, 2.04), "the line stops where the square's back is");
    assert_eq!(drawn.r.count(SkipKind::CustomLineEnd), 0, "{:?}", drawn.r.skipped);
    assert_ne!(drawn.r.rgba_premul, four.r.rgba_premul, "not number 4");
    // The flags that are not a line end's, and the ones that say upright, scaled and both: 0, 1 (a line pattern), 65, 4100 (a fill pattern) and 3 hold no 2... but 3 does hold it.
    for (flags, is_end) in [(0, false), (1, false), (65, false), (4100, false), (258, true), (1026, true), (1282, true), (3, true)] {
        let pic = draw(&with_ends(vec![line_ending_in(1, 2.0, 0.05, "EndArrow", "Tick", 2)], vec![("Tick", flags, square())]));
        let noted = pic.r.count(SkipKind::CustomLineEnd);
        assert_eq!(noted, usize::from(!is_end), "flags {flags}: {:?}", pic.r.skipped);
        if !is_end {
            assert_eq!(pic.r.rgba_premul, four.r.rgba_premul, "flags {flags}: a master that is no line end is drawn as number 4");
        }
    }
    // No master of that name, a plain 254, and a name that is no master's.
    let cases = [
        with_ends(vec![line_ending_in(1, 2.0, 0.05, "EndArrow", "Nothing", 2)], vec![("Tick", 2, square())]),
        one_page(vec![ended(1, 2.0, 0.05, 254, 2).cell("LineWeight", "0.05")]),
        with_ends(vec![ended(1, 2.0, 0.05, 254, 2).cell("LineWeight", "0.05").formula("EndArrow", "THEMEVAL()", "254")], vec![("Tick", 2, square())]),
    ];
    for (i, doc) in cases.iter().enumerate() {
        let pic = draw(doc);
        assert_eq!(pic.r.count(SkipKind::CustomLineEnd), 1, "case {i}: {:?}", pic.r.skipped);
        assert_eq!(pic.r.rgba_premul, four.r.rgba_premul, "case {i}: number 4");
    }
    // A master with no geometry at all.
    let empty = with_ends(
        vec![line_ending_in(1, 2.0, 0.05, "EndArrow", "Tick", 2)],
        vec![("Tick", 2, shape_at(5, 0.05, 0.05, 0.1, 0.1).cell("LocPinX", "0.1"))],
    );
    let pic = draw(&empty);
    assert_eq!((pic.r.count(SkipKind::CustomLineEnd), &pic.r.rgba_premul), (1, &four.r.rgba_premul), "{:?}", pic.r.skipped);
    // The master is found by its `NameU` and by its `UniqueID` too: the name a `USE` holds is either (a GUID is written in braces, no quotes).
    let mut by_guid = with_ends(vec![line_ending_in(1, 2.0, 0.05, "EndArrow", "Tick", 2)], vec![("Tick", 2, square())]);
    if let Some(master) = by_guid.masters.get_mut(0) {
        Arc::make_mut(master).unique_id = Some(Arc::from("{12345678-1234-4234-8234-123456789ABC}"));
    }
    let shape = by_guid.pages.get_mut(0).and_then(|p| Arc::make_mut(p).shapes.get_mut(0)).map(Arc::make_mut).unwrap();
    Arc::make_mut(&mut shape.sheet).cells.insert(
        diacraft_doc::CellName::new("EndArrow"),
        Cell::with_formula("USE({12345678-1234-4234-8234-123456789ABC})", Some(Value::from_text("254"))),
    );
    let pic = draw(&by_guid);
    assert_eq!(pic.r.count(SkipKind::CustomLineEnd), 0, "{:?}", pic.r.skipped);
    assert!(pic.is_dark(3.45, 2.04));
}

/// A master whose one shape holds `sections` closed sections of `rows_each` rows (the polygon's rows, the last back at the first), each a thin triangle lying behind the pin.
fn sectioned(sections: u32, rows_each: u32) -> ShapeBuilder {
    let mut shape = shape_at(5, 0.05, 0.05, 0.1, 0.1).cell("LocPinX", "0.1").cell("LocPinY", "0.05").cell("FillPattern", "1");
    for ix in 0..sections {
        // `rows_each` rows are the corners `rows_each - 1` and the row that closes the path; at least three corners make a triangle.
        let corners = rows_each.saturating_sub(1).max(3);
        let points: Vec<(f64, f64)> = (0..corners)
            .map(|k| {
                let a = f64::from(k) / f64::from(corners) * std::f64::consts::TAU;
                (0.05 + 0.04 * a.cos(), 0.05 + 0.04 * a.sin())
            })
            .collect();
        shape = closed_section(shape, ix, &points, true);
    }
    shape
}

#[test]
fn a_line_end_master_past_16_sections_or_256_rows_is_drawn_as_4_in_counted_work() {
    // The design's caps and E3: within them the master is drawn and its rows are the work (each shape of it WORK_PER_DRAWN_SHAPE, each row WORK_PER_END_ROW, charged before
    // anything is built); past either, nothing is built and the end is number 4, in the work of number 4.
    let line = || vec![line_ending_in(1, 2.0, 0.05, "EndArrow", "Tick", 2)];
    let four = one_page(vec![line_ending_in_number(1, 2.0, 0.05, 4, 2)]);
    let (four_work, four_pic) = (work_of(&four), draw(&four));
    let sections = u32::try_from(MAX_END_SECTIONS).unwrap();
    let rows = u32::try_from(MAX_END_ROWS).unwrap();
    // Exactly the caps: 16 sections of 16 rows (256 in all), and one section of 256 rows. Drawn as the master, and charged for what was built.
    for (n, each) in [(sections, rows / sections), (1, rows)] {
        let doc = with_ends(line(), vec![("Tick", 2, sectioned(n, each))]);
        let pic = draw(&doc);
        assert_eq!(pic.r.count(SkipKind::CustomLineEnd), 0, "{n} sections of {each} rows: {:?}", pic.r.skipped);
        assert_ne!(pic.r.rgba_premul, four_pic.r.rgba_premul, "{n} sections of {each} rows are the master, not number 4");
        assert_eq!(work_of(&doc), four_work + WORK_PER_DRAWN_SHAPE + u64::from(n * each) * WORK_PER_END_ROW, "{n} sections of {each} rows");
    }
    // One past: 17 sections, 257 rows in one section, and a master of 10,000 rows: number 4, and no more work than number 4's.
    for (n, each) in [(sections + 1, 3), (1, rows + 1), (1, 10_000), (sections * 4, 64), (sections, rows)] {
        let doc = with_ends(line(), vec![("Tick", 2, sectioned(n, each))]);
        let pic = draw(&doc);
        assert_eq!(pic.r.count(SkipKind::CustomLineEnd), 1, "{n} sections of {each} rows: {:?}", pic.r.skipped);
        assert_eq!(pic.r.rgba_premul, four_pic.r.rgba_premul, "{n} sections of {each} rows: number 4");
        assert_eq!(work_of(&doc), four_work, "{n} sections of {each} rows: counted before anything is built");
    }
}

#[test]
fn a_use_naming_a_master_whose_own_end_is_custom_is_not_followed() {
    // A line-end master is geometry: the line ends of its own shapes are not drawn, and a `USE` among them is not followed (so a master that names itself, or another that names it,
    // ends at once). `Outer` is a vertical bar across the line's end, a stroke of its own, and its shape's own `EndArrow` names `Inner`, a big square; `Inner`'s names `Outer` back.
    let bar = |end: &str| {
        let shape = shape_at(5, 0.05, 0.05, 0.1, 0.1)
            .cell("LocPinX", "0.1")
            .cell("LocPinY", "0.05")
            .cell("FillPattern", "0")
            .cell("LinePattern", "1")
            .cell("LineWeight", "0.02");
        geometry_of(shape, vec![("MoveTo", vec![("X", 0.0), ("Y", -0.3)]), ("LineTo", vec![("X", 0.0), ("Y", 0.4)])]).formula(
            "EndArrow",
            &format!("USE(\"{end}\")"),
            "254",
        )
    };
    let square = |end: &str| {
        let shape = end_shape(0.1, 0.1, (0.1, 0.05), &[vec![(-1.0, -1.0), (-0.6, -1.0), (-0.6, -0.6), (-1.0, -0.6)]]);
        shape.formula("EndArrow", &format!("USE(\"{end}\")"), "254")
    };
    let line = || vec![line_ending_in(1, 2.0, 0.05, "EndArrow", "Outer", 2)];
    for inner in [square("Outer"), square("Inner"), square("Nothing")] {
        let pic = draw(&with_ends(line(), vec![("Outer", 2, bar("Inner")), ("Inner", 2, inner)]));
        // The weight 0.05 and the box 0.1 inch: the master is drawn at half size. The bar is at 0.05 behind the end point (x 3.45) and runs 0.175 either side of the line, as wide as the
        // line is heavy. A number 4 at this weight is 0.17 long and 0.085 across at its back, so it is not what draws at 0.12 above the line there.
        assert!(pic.is_dark(3.45, 2.12) && pic.is_dark(3.45, 1.88), "Outer's own stroke is drawn, in the line's colour and weight");
        assert!(pic.is_white(3.45, 2.2) && pic.is_white(3.45, 1.8), "and ends where its points do");
        // Inner's square, were it followed, would lie from x 2.95 to 3.15 and y 1.475 to 1.675.
        assert!(pic.is_white(3.05, 1.55), "Inner is not followed: the end of a master's own shape is not drawn");
        assert_eq!(pic.r.count(SkipKind::CustomLineEnd), 0, "{:?}", pic.r.skipped);
        assert_eq!(pic.r.count(SkipKind::ShapeFailed), 0, "{:?}", pic.r.skipped);
    }
}

#[test]
fn a_turning_end_trims_the_line_and_an_upright_one_does_not_and_stays_pointing_right() {
    // A see-through line shows the shaft twice where it runs under the diamond: a turning end stops the shaft at the diamond's back (the one thing under the diamond
    // is the diamond), an upright one does not trim.
    let see_through = |flags: u32| {
        let line = line_ending_in(1, 2.0, 0.05, "EndArrow", "Filled Diamond", 2).cell("LineColorTrans", "0.5");
        draw(&with_ends(vec![line], vec![("Filled Diamond", flags, diamond_shape())]))
    };
    let (turning, upright) = (see_through(2), see_through(258));
    let grey = |pic: &Pic| pic.at(3.2, 2.0)[0];
    assert!((110..=146).contains(&grey(&turning)), "the diamond alone, half see-through: {}", grey(&turning));
    assert!(grey(&upright) < 90, "the untrimmed shaft shows under it: {}", grey(&upright));
    assert!((110..=146).contains(&turning.at(2.0, 2.0)[0]) && (110..=146).contains(&upright.at(2.0, 2.0)[0]), "before the back, the shaft is alone");
    // 258 on a vertical line: the end keeps the page's orientation, pointing right whichever way the line runs, with its pin on the end point (2, 3.5). The diamond lies to the left of
    // the pin, its tip at the end of the line, and the line runs to its end untrimmed.
    let vertical = |flags: u32| {
        let line =
            geometry_of(shape_at(1, 2.0, 0.5, 0.0, 3.0), vec![("MoveTo", vec![("X", 0.0), ("Y", 0.0)]), ("LineTo", vec![("X", 0.0), ("Y", 3.0)])])
                .cell("FillPattern", "0")
                .cell("LineWeight", "0.05")
                .cell("LinePattern", "1")
                .cell("LineCap", "1")
                .cell("LineColor", "#000000")
                .formula("EndArrow", "USE(\"Filled Diamond\")", "254");
        draw(&with_ends(vec![line], vec![("Filled Diamond", flags, diamond_shape())]))
    };
    let (up, turned) = (vertical(258), vertical(2));
    assert!(
        up.is_dark(1.5, 3.68) && up.is_white(1.5, 3.78),
        "to the left of the pin, 0.5 back, 0.18 above the line's end (3.5): inside; 0.28: outside"
    );
    assert!(up.is_white(2.5, 3.5) && up.is_white(2.0, 3.8), "nothing to the right of the pin, nothing past the end of the line");
    assert!(up.is_dark(2.0, 3.45), "the line runs to its end, untrimmed");
    // A turning one on the same line points up the page: its axis is the line, the widest 0.5 below the end.
    assert!(turned.is_dark(2.2, 3.0) && turned.is_white(2.3, 3.0), "pointing along the line, 0.5 back: 0.2 across, inside; 0.3, outside");
    assert!(turned.is_white(1.5, 3.68), "it is not left of the pin");
}

#[test]
fn a_scaled_end_keeps_the_masters_own_size_at_every_weight_and_class_and_follows_the_pages_scale() {
    // A master whose box is 0.1 inch tall, flags 1026 (scaled and turning), on a page of 1:1: the box stays 0.1 inch tall whatever the weight and the class, and the
    // diamond is 20 boxes (2 inches) long, 1 across, widest 1 inch behind the tip.
    let big_h = 0.1;
    let shape = || {
        end_shape(
            big_h,
            big_h,
            (big_h, big_h / 2.0),
            &[vec![(big_h, big_h / 2.0), (-9.0 * big_h, 5.5 * big_h), (-19.0 * big_h, big_h / 2.0), (-9.0 * big_h, -4.5 * big_h)]],
        )
    };
    let own = |weight: f64, size: i64| {
        draw(&with_ends(vec![line_ending_in(1, 2.0, weight, "EndArrow", "Filled Diamond", size)], vec![("Filled Diamond", 1026, shape())]))
    };
    let first = own(0.01, 2);
    assert!(first.is_dark(2.5, 2.4) && first.is_white(2.5, 2.6), "widest point: 0.5 across at 1 inch back (x 2.5)");
    assert!(first.is_dark(1.6, 2.04) && first.is_white(1.4, 2.2), "near the back at 1.9 behind the tip; the back is at x 1.5");
    for (weight, size) in [(0.0, 2), (0.01, 0), (0.01, 4), (0.04, 2), (0.04, 6), (0.02, 3)] {
        let other = own(weight, size);
        // The diamond is the same: compare away from the shaft (the shaft is the line's, at another weight).
        for (x, y, dark) in [(2.5, 2.4, true), (2.5, 2.6, false), (3.0, 2.15, true), (1.6, 2.04, true), (1.4, 2.2, false)] {
            assert_eq!(other.is_dark(x, y), dark, "weight {weight}, class {size}: ({x}, {y})");
        }
    }
    // The same master unscaled (flags 2) at weight 0.01 is a tenth as tall as the box: a diamond 0.2 inch long.
    let small = draw(&with_ends(vec![line_ending_in(1, 2.0, 0.01, "EndArrow", "Filled Diamond", 2)], vec![("Filled Diamond", 2, shape())]));
    assert!(small.is_white(2.5, 2.4) && small.is_white(3.0, 2.15));
    // On a page of 1:2 (a drawing inch is half a paper inch) the scaled end follows the scale: the box is 0.1 drawing inch, 0.05 paper inch. The line is 6 drawing inches long here.
    let mut scaled_page = page(8.0, 8.0).cell("DrawingScale", "2");
    scaled_page = scaled_page.shape(
        geometry_of(shape_at(1, 1.0, 4.0, 6.0, 0.0), vec![("MoveTo", vec![("X", 0.0), ("Y", 0.0)]), ("LineTo", vec![("X", 6.0), ("Y", 0.0)])])
            .cell("FillPattern", "0")
            .cell("LineWeight", "0.05")
            .cell("LinePattern", "1")
            .cell("LineCap", "1")
            .cell("LineColor", "#000000")
            .formula("EndArrow", "USE(\"Filled Diamond\")", "254"),
    );
    let mut doc = base().master(1, MasterBuilder::new().name("Filled Diamond").shape(shape())).page(1, scaled_page).build();
    if let Some(master) = doc.masters.get_mut(0) {
        Arc::make_mut(master).pattern_flags = Some(1026);
    }
    let pic = draw(&doc);
    // The line ends at paper (3.5, 2). The diamond is 1 paper inch long and 0.5 across, widest 0.5 back: the same picture as the first test's, with the 0.05 box.
    assert!(pic.is_dark(3.0, 2.2) && pic.is_white(3.0, 2.3), "the scaled end follows the page's scale");
    // Unscaled (2), the same page and line: the weight 0.05 paper inch gives a box 0.05 paper inch tall too: the same size. Scaled at 1:1 it was 2 inches, so this is not a coincidence of
    // the weight: the same master at flags 1026 on the 1:1 page above reached 1.5.
}

#[test]
fn a_custom_end_at_weight_0_has_a_box_of_no_height_and_draws_nothing_and_says_nothing() {
    // The box is as tall as the weight, and a weight of 0 is no height. The shaft is the hairline it always was.
    let doc = with_ends(vec![line_ending_in(1, 2.0, 0.0, "EndArrow", "Filled Diamond", 2)], vec![("Filled Diamond", 2, diamond_shape())]);
    let pic = draw(&doc);
    assert!(pic.is_white(3.4, 2.1) && pic.is_white(3.0, 2.2), "no diamond");
    assert_eq!(pic.r.count(SkipKind::CustomLineEnd), 0, "{:?}", pic.r.skipped);
    assert_eq!(pic.r.count(SkipKind::LineEndApproximate), 0, "{:?}", pic.r.skipped);
    // A hairline: 0.25 point, 0.00347 inch: the diamond is 20 boxes, 0.07 inch long, which is as hard to see as the documented behaviour says.
    let hair =
        draw(&with_ends(vec![line_ending_in(1, 2.0, 0.25 / 72.0, "EndArrow", "Filled Diamond", 2)], vec![("Filled Diamond", 2, diamond_shape())]));
    assert!(hair.is_white(3.2, 2.1), "5 points long, so nothing 0.3 inch back");
}

/// A group master: a group of `members`, each a closed polygon in the group's own frame, in one 0.1 inch box with its pin at the middle of the right edge.
fn group_master(members: &[Vec<(f64, f64)>]) -> ShapeBuilder {
    let mut group = shape_at(5, 0.05, 0.05, 0.1, 0.1).cell("LocPinX", "0.1").cell("LocPinY", "0.05").group();
    for (i, points) in members.iter().enumerate() {
        let (w, h) = (0.1, 0.1);
        let member = closed_section(shape_at(10 + u32::try_from(i).unwrap(), 0.0, 0.0, w, h).cell("FillPattern", "1"), 0, points, true);
        group = group.child(member);
    }
    group
}

#[test]
fn a_group_master_is_drawn_member_by_member_and_one_nested_past_the_depth_or_with_no_size_is_drawn_as_4() {
    // A pattern master is one shape or one group. Two members: the filled diamond's left half and its right half drawn as separate squares behind the tip.
    let left = vec![(-0.4, -0.05), (-0.3, -0.05), (-0.3, 0.05), (-0.4, 0.05)];
    let right = vec![(-0.1, -0.05), (0.0, -0.05), (0.0, 0.05), (-0.1, 0.05)];
    let doc = with_ends(vec![line_ending_in(1, 2.0, 0.1, "EndArrow", "Pair", 2)], vec![("Pair", 2, group_master(&[left, right]))]);
    let pic = draw(&doc);
    // The box is 0.1 inch and the weight 0.1: scale 1. The group's pin is at its (0.1, 0.05), so a point of a member at (x, y) is at (3.5 + x - 0.1, 2 + y - 0.05): the squares are at
    // x 3.0 to 3.1 and 3.3 to 3.4, y 1.9 to 2.0. The line is trimmed at the group's back (0.5 behind the pin, x 3.0), so nothing of the shaft is between them.
    assert!(pic.is_dark(3.05, 1.95), "the first member");
    assert!(pic.is_dark(3.35, 1.95), "the second member");
    assert!(pic.is_white(3.2, 1.95), "nothing between them: the shaft stops at the group's back");
    assert!(pic.is_white(3.05, 2.05), "the members lie below the line's axis, as their points say");
    assert_eq!(pic.r.count(SkipKind::CustomLineEnd), 0, "{:?}", pic.r.skipped);
    // Nested past the depth the master may have (4 levels): number 4, with the note, and the work of number 4. A chain of groups 150 deep (the document's own limit is 200) is
    // the hostile one: it is refused by counting the levels, which takes no more than the cap's steps.
    let mut nested = shape_at(6, 0.05, 0.05, 0.1, 0.1).cell("LocPinX", "0.1").cell("LocPinY", "0.05").group();
    for level in 0..150u32 {
        nested = shape_at(7 + level, 0.0, 0.0, 0.1, 0.1).group().child(nested);
    }
    let top = shape_at(5, 0.05, 0.05, 0.1, 0.1).cell("LocPinX", "0.1").cell("LocPinY", "0.05").group().child(nested);
    let deep = with_ends(vec![line_ending_in(1, 2.0, 0.1, "EndArrow", "Deep", 2)], vec![("Deep", 2, top)]);
    let four = one_page(vec![line_ending_in_number(1, 2.0, 0.1, 4, 2)]);
    let pic = draw(&deep);
    assert_eq!(pic.r.count(SkipKind::CustomLineEnd), 1, "{:?}", pic.r.skipped);
    assert_eq!(pic.r.rgba_premul, draw(&four).r.rgba_premul, "number 4");
    assert_eq!(work_of(&deep), work_of(&four), "counted by the levels it was refused at, not built");
}

#[test]
fn a_custom_end_whose_master_has_no_size_or_whose_weight_is_huge_or_no_number_is_drawn_or_refused_without_a_fault() {
    // Hostile cells: a master whose `Height` is 0, a `Width` that is no number, and a height of 1e-300 (the scale is no number we can draw) are unusable and draw number 4 with the
    // note; the weight of the line is cut to what a stroke is (as it is for every line), and a weight that is no number is 0.
    let line = |weight: &str| vec![line_ending_in(1, 2.0, 0.05, "EndArrow", "Tick", 2).cell("LineWeight", weight)];
    let four = draw(&one_page(vec![line_ending_in_number(1, 2.0, 0.05, 4, 2)]));
    let flat = || end_shape(0.1, 0.1, (0.1, 0.05), &[diamond()]).cell("Height", "0");
    let nan = || end_shape(0.1, 0.1, (0.1, 0.05), &[diamond()]).cell("Height", "1.#QNAN");
    let tiny = || end_shape(0.1, 0.1, (0.1, 0.05), &[diamond()]).cell("Height", "1e-300");
    let inf = || end_shape(0.1, 0.1, (0.1, 0.05), &[diamond()]).cell("Height", "1.#INF");
    for (what, shape) in [("height 0", flat()), ("height NaN", nan()), ("height 1e-300", tiny())] {
        let doc = with_ends(line("0.05"), vec![("Tick", 2, shape)]);
        let pic = draw(&doc);
        assert_eq!(pic.r.count(SkipKind::CustomLineEnd), 1, "{what}: {:?}", pic.r.skipped);
        assert_eq!(pic.r.rgba_premul, four.r.rgba_premul, "{what}: number 4");
    }
    // A box of an infinite height (a cell reads as the largest number a drawing holds) makes the end so small that it cannot be seen, and is no fault.
    let pic = draw(&with_ends(line("0.05"), vec![("Tick", 2, inf())]));
    assert_eq!(pic.r.count(SkipKind::ShapeFailed) + pic.r.count(SkipKind::Recovered), 0, "{:?}", pic.r.skipped);
    // Weights: huge, infinite, NaN and negative. None panics, none is a fault of the frame, and the work stays what the master's rows and the line cost.
    let mut works = Vec::new();
    for weight in ["1e300", "1e15", "1.#INF", "1.#QNAN", "-3", "1e-300"] {
        let doc = with_ends(line(weight), vec![("Tick", 2, diamond_shape())]);
        let pic = draw(&doc);
        assert_eq!(pic.r.count(SkipKind::ShapeFailed) + pic.r.count(SkipKind::Recovered), 0, "weight {weight}: {:?}", pic.r.skipped);
        works.push(work_of(&doc));
    }
    assert!(works.iter().all(|w| *w < 1_000), "{works:?}");
}

/// A line of the gallery: from, to, weight in points, whether the diamond is at its begin and at its end, and the size class.
type GalleryLine = ((f64, f64), (f64, f64), f64, bool, bool, i64);

/// The diamond on lines of several weights and size classes, at the end and at both ends, and a fan of lines at other angles: the page to look at (`DIACRAFT_CUSTOM_ENDS_GALLERY=/absolute/path.png`
/// writes it as a picture).
fn custom_ends_gallery() -> Document {
    let mut lines: Vec<GalleryLine> = Vec::new();
    // The weights, with the diamond at the end and at both ends.
    for (k, weight) in [0.25, 0.75, 1.5, 3.0, 6.0].into_iter().enumerate() {
        let y = 5.5 - 0.5 * k as f64;
        lines.push(((0.4, y), (3.2, y), weight, false, true, 2));
        lines.push(((3.8, y), (6.6, y), weight, true, true, 2));
    }
    // The size classes 0 to 4 at 1.5 points.
    for size in 0..=4_i64 {
        let x = 0.4 + 1.3 * size as f64;
        lines.push(((x, 2.8), (x + 1.1, 2.8), 1.5, false, true, size));
    }
    // A fan of lines from one point: the end turns with the line.
    for k in 0..8 {
        let (sin, cos) = (f64::from(k) * 45.0).to_radians().sin_cos();
        lines.push(((3.5, 1.3), (3.5 + cos, 1.3 + sin), 1.5, false, true, 2));
    }
    let mut page = page(7.0, 6.0);
    for (i, (from, to, weight_pt, begin, end, size)) in lines.into_iter().enumerate() {
        let (dx, dy) = (to.0 - from.0, to.1 - from.1);
        let mut shape = geometry_of(
            shape_at(u32::try_from(i + 1).unwrap(), from.0, from.1, dx, dy),
            vec![("MoveTo", vec![("X", 0.0), ("Y", 0.0)]), ("LineTo", vec![("X", dx), ("Y", dy)])],
        )
        .cell("FillPattern", "0")
        .cell("LineWeight", &(weight_pt / 72.0).to_string())
        .cell("LinePattern", "1")
        .cell("LineCap", "1")
        .cell("LineColor", "#1F3A5F");
        for (cell, on) in [("BeginArrow", begin), ("EndArrow", end)] {
            if on {
                shape = shape.formula(cell, "USE(\"Filled Diamond\")", "254").cell(&format!("{cell}Size"), &size.to_string());
            }
        }
        page = page.shape(shape);
    }
    let mut doc = base().master(1, MasterBuilder::new().name("Filled Diamond").shape(diamond_shape())).page(1, page).build();
    if let Some(master) = doc.masters.get_mut(0) {
        Arc::make_mut(master).pattern_flags = Some(2);
    }
    doc
}

#[test]
fn the_custom_end_gallery_draws_every_end_from_its_master_and_can_be_written() {
    let doc = custom_ends_gallery();
    let pic = draw(&doc);
    assert_eq!(pic.r.count(SkipKind::CustomLineEnd), 0, "{:?}", pic.r.skipped);
    assert!(pic.ink_pixels() > 0);
    // The heaviest line, 6 points, at y 3.5 ending at x 3.2: its diamond is 20 weights (1.67 inches) long and 10 across, so 0.8 inch from its tip it is 0.8 across.
    let heavy = pic.at(3.2 - 0.8, 3.5 + 0.3);
    assert!(heavy[3] > 200 && heavy[0] < 100, "{heavy:?}");
    // `DIACRAFT_CUSTOM_ENDS_GALLERY=/absolute/path.png cargo nextest run ... the_custom_end_gallery` writes the picture to look at.
    if let Ok(path) = std::env::var("DIACRAFT_CUSTOM_ENDS_GALLERY") {
        let out = diacraft_render::export::export_png(&mut diacraft_render::Renderer::new(), &doc, 1, 200.0).expect("exports");
        std::fs::write(path, out.bytes).expect("writes the gallery");
    }
}

#[test]
fn a_page_of_lines_that_name_one_master_is_counted_line_by_line_and_cut_by_the_work_budget() {
    // A custom end is built again for each line that holds it (the work is a function of the line, so a shared walk and a walk in order count the same), and each build is charged before
    // it is made: 400 lines on a master of 16 sections of 16 rows are 400 builds, and a budget of three of them draws the first three ends and leaves the rest out in one count.
    let lines: Vec<ShapeBuilder> = (0..400u32)
        .map(|k| {
            let (x, y) = (0.1 + f64::from(k % 20) * 0.19, 0.1 + f64::from(k / 20) * 0.19);
            geometry_of(shape_at(k + 1, x, y, 0.15, 0.0), vec![("MoveTo", vec![("X", 0.0), ("Y", 0.0)]), ("LineTo", vec![("X", 0.15), ("Y", 0.0)])])
                .cell("FillPattern", "0")
                .cell("LineWeight", "0.01")
                .cell("LinePattern", "1")
                .formula("EndArrow", "USE(\"Tick\")", "254")
        })
        .collect();
    let doc = with_ends(
        lines,
        vec![("Tick", 2, sectioned(u32::try_from(MAX_END_SECTIONS).unwrap(), u32::try_from(MAX_END_ROWS / MAX_END_SECTIONS).unwrap()))],
    );
    let per_line = WORK_PER_DRAWN_SHAPE + u64::try_from(MAX_END_ROWS).unwrap() * WORK_PER_END_ROW;
    let shape = diacraft_render::limits::WORK_PER_SHAPE + diacraft_render::limits::WORK_PER_DRAWN_SHAPE;
    let drawn = |max_work: u64| {
        let mut renderer = Renderer::new();
        renderer.set_limits(diacraft_render::Limits { max_work, ..diacraft_render::Limits::unlimited_time() });
        let pic = draw_with(&mut renderer, &doc);
        (pic, renderer.work_done())
    };
    let (all, work) = drawn(u64::MAX);
    assert_eq!(work, 400 * (shape + per_line), "every line is charged its own build: {:?}", all.r.skipped);
    assert_eq!(all.r.count(SkipKind::WorkLimit), 0);
    let budget = 3 * (shape + per_line);
    let (cut, work) = drawn(budget);
    assert!(work <= budget, "{work} of {budget}");
    assert!(cut.r.count(SkipKind::WorkLimit) > 0, "the frame is cut where the budget ends: {:?}", cut.r.skipped);
    assert!(cut.ink_pixels() < all.ink_pixels());
}

/// The real-time side of the test above, for a person to run (`cargo nextest run --run-ignored all`): two thousand lines on the largest master a line end may be, drawn in seconds.
#[test]
#[ignore = "real time"]
fn a_page_of_two_thousand_lines_on_the_largest_master_is_drawn_in_seconds() {
    let lines: Vec<ShapeBuilder> = (0..2_000u32)
        .map(|k| {
            let (x, y) = (0.1 + f64::from(k % 40) * 0.09, 0.1 + f64::from(k / 40) * 0.07);
            geometry_of(shape_at(k + 1, x, y, 0.07, 0.0), vec![("MoveTo", vec![("X", 0.0), ("Y", 0.0)]), ("LineTo", vec![("X", 0.07), ("Y", 0.0)])])
                .cell("FillPattern", "0")
                .cell("LineWeight", "0.005")
                .cell("LinePattern", "1")
                .formula("EndArrow", "USE(\"Tick\")", "254")
        })
        .collect();
    let doc = with_ends(
        lines,
        vec![("Tick", 2, sectioned(u32::try_from(MAX_END_SECTIONS).unwrap(), u32::try_from(MAX_END_ROWS / MAX_END_SECTIONS).unwrap()))],
    );
    let started = std::time::Instant::now();
    let pic = draw(&doc);
    let elapsed = started.elapsed();
    eprintln!("2,000 lines on a master of 256 rows: {elapsed:?}");
    assert!(pic.ink_pixels() > 0 && elapsed < std::time::Duration::from_secs(10), "{elapsed:?}");
}
