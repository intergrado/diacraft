//! The shapes and boxes of the line ends:
//! the pixels of what `ends.rs` plans, which its own tests measure as geometry.

use diacraft_doc::RowKey;
use diacraft_render::SkipKind;
use diacraft_testkit::build::ShapeBuilder;

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
