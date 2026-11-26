//! The numbers of a custom line end: the master's drawing sized and put on a line, as geometry.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use diacraft_doc::{Cell, CellAddr, Document, RowKey, Value};
use diacraft_testkit::build::{DocBuilder, MasterBuilder, PageBuilder, ShapeBuilder};
use kurbo::{PathEl, Point};

use super::*;
use crate::ends::{self, Request};
use crate::read::{Cx, eval_ctx};

/// The side of the filled diamond's box in inches (`h`) and one point in inches.
const H: f64 = 0.05;
const POINT: f64 = 1.0 / 72.0;

/// A master shape of `w` by `h` inches whose pin is at (`px`, `py`), with one closed section of `points` that draws a fill and no outline.
fn master_shape(w: f64, h: f64, pin: (f64, f64), points: &[(f64, f64)]) -> ShapeBuilder {
    let mut shape = ShapeBuilder::new(5)
        .cell("PinX", &(w / 2.0).to_string())
        .cell("PinY", &(h / 2.0).to_string())
        .cell("Width", &w.to_string())
        .cell("Height", &h.to_string())
        .cell("LocPinX", &pin.0.to_string())
        .cell("LocPinY", &pin.1.to_string())
        .cell("Angle", "0")
        .cell("FillPattern", "1")
        .cell("LinePattern", "1");
    for (i, (x, y)) in points.iter().chain(points.first()).enumerate() {
        let row = RowKey::Ix(u32::try_from(i).unwrap());
        shape = shape
            .row_type("Geometry", Some(0), row.clone(), if i == 0 { "MoveTo" } else { "LineTo" })
            .row_cell("Geometry", Some(0), row.clone(), "X", &x.to_string())
            .row_cell("Geometry", Some(0), row, "Y", &y.to_string());
    }
    for (flag, value) in [("NoFill", "0"), ("NoLine", "1"), ("NoShow", "0")] {
        shape = shape.set(CellAddr::in_section("Geometry", Some(0), flag), Cell::constant(Value::from_text(value)));
    }
    shape
}

fn diamond_points() -> Vec<(f64, f64)> {
    vec![(H, H / 2.0), (-9.0 * H, 5.5 * H), (-19.0 * H, H / 2.0), (-9.0 * H, -4.5 * H)]
}

/// A document with the master `Filled Diamond` of `PatternFlags` `flags`.
fn doc_with(flags: u32, shape: ShapeBuilder) -> Document {
    let mut doc = DocBuilder::new()
        .root(|r| r.cell("FillPattern", "1").cell("FillForegnd", "0").cell("LinePattern", "1").cell("LineColor", "0").cell("LineWeight", "0.01"))
        .master(1, MasterBuilder::new().name("Filled Diamond").shape(shape))
        .page(1, PageBuilder::new())
        .build();
    if let Some(master) = doc.masters.get_mut(0) {
        Arc::make_mut(master).pattern_flags = Some(flags);
    }
    doc
}

/// The form of the master of `doc`, counted and built.
fn form_of(doc: &Document) -> Result<Form, Refusal> {
    let mut ctx = eval_ctx();
    let cx = Cx::new(doc, &mut ctx);
    let master = master_named(doc, "Filled Diamond").ok_or(Refusal::NotAnEnd)?;
    let counted = survey(&cx, master)?;
    build(&cx, &counted, usize::MAX)
}

/// The vertices of a path as (x, y) in points, rounded to 1e-6, in the order the path has them.
fn corners(path: &kurbo::BezPath) -> Vec<(f64, f64)> {
    let round = |v: f64| (v * 1.0e6).round() / 1.0e6;
    path.elements()
        .iter()
        .filter_map(|el| match *el {
            PathEl::MoveTo(p) | PathEl::LineTo(p) => Some((round(p.x / POINT), round(p.y / POINT))),
            _ => None,
        })
        .collect()
}

/// A line of 100 points from `from` to `to` (points), with the custom end at its end (or at its begin when `at_begin`), at `weight` points and size class 2.
fn ended(from: (f64, f64), to: (f64, f64), weight_pt: f64, end: &CustomEnd, at_begin: bool) -> ends::Ends {
    let mut path = kurbo::BezPath::new();
    path.move_to(Point::new(from.0 * POINT, from.1 * POINT));
    path.line_to(Point::new(to.0 * POINT, to.1 * POINT));
    let (begin_custom, end_custom) = if at_begin { (Some(end.clone()), None) } else { (None, Some(end.clone())) };
    let request = Request { begin: 0, end: 0, begin_size: 2, end_size: 2, weight: weight_pt * POINT, see_through: false, begin_custom, end_custom };
    ends::plan(&path, &request).ends.expect("an end is drawn")
}

/// The diamond sized for a line of `weight_pt` points at size class 2 on a page of 1:1.
fn diamond_at(weight_pt: f64, flags: u32) -> CustomEnd {
    let form = form_of(&doc_with(flags, master_shape(H, H, (H, H / 2.0), &diamond_points()))).unwrap();
    let Ok(Sizing::By(factor)) = form.sizing(2, weight_pt * POINT, 1.0) else { panic!("an end of a weight above 0 is sized") };
    form.sized(factor).expect("a drawing the line's size can hold")
}

#[test]
fn the_filled_diamond_at_0_75_point_is_15_points_long_and_7_5_across_and_the_shaft_stops_at_its_back() {
    // A line from (0, 0) to (100, 0) points, weight 0.75, size 2: the tip at (100, 0), the back at (85, 0), the sides at (92.5, 3.75) and (92.5, -3.75); the shaft stops at 85.
    let drawn = ended((0.0, 0.0), (100.0, 0.0), 0.75, &diamond_at(0.75, 2), false);
    assert_eq!(corners(&drawn.fill_even_odd), vec![(100.0, 0.0), (92.5, 3.75), (85.0, 0.0), (92.5, -3.75), (100.0, 0.0)]);
    assert!(drawn.fill.is_empty() && drawn.strokes.is_empty(), "one filled section and no outline");
    let shaft = drawn.shaft.bounding_box();
    assert!((shaft.x0 / POINT).abs() < 1e-9 && (shaft.x1 / POINT - 85.0).abs() < 1e-6, "the shaft runs from 0 to the back at 85: {shaft:?}");
    // At weight 1.5 points the box is twice as tall: the back is at 70 (not at 81.91, number 22's box at that weight: the S of 4.02 times 4.5).
    let heavy = ended((0.0, 0.0), (100.0, 0.0), 1.5, &diamond_at(1.5, 2), false);
    let back = corners(&heavy.fill_even_odd)[2].0;
    assert!((back - 70.0).abs() < 1e-6, "{back}");
    assert!((back - 81.91).abs() > 1.0);
    // A hairline of 0.25 point makes a diamond 5 points long.
    let hair = ended((0.0, 0.0), (100.0, 0.0), 0.25, &diamond_at(0.25, 2), false);
    assert!((corners(&hair.fill_even_odd)[2].0 - 95.0).abs() < 1e-6);
}

#[test]
fn at_the_begin_the_end_points_out_of_the_line_with_its_tip_on_the_begin_point() {
    // The project rule: with `BeginArrow` instead the tip is at (0, 0) and the back at (15, 0).
    let drawn = ended((0.0, 0.0), (100.0, 0.0), 0.75, &diamond_at(0.75, 2), true);
    assert_eq!(corners(&drawn.fill_even_odd), vec![(0.0, 0.0), (7.5, -3.75), (15.0, 0.0), (7.5, 3.75), (0.0, 0.0)]);
    let shaft = drawn.shaft.bounding_box();
    assert!((shaft.x0 / POINT - 15.0).abs() < 1e-6 && (shaft.x1 / POINT - 100.0).abs() < 1e-9, "{shaft:?}");
}

#[test]
fn an_upright_end_keeps_the_pages_orientation_and_leaves_the_line_untrimmed() {
    // 258 on a line from (0, 0) to (0, 100): the end stays pointing right, its tip at (0, 100) and its back to the left of it; the line runs to (0, 100).
    let drawn = ended((0.0, 0.0), (0.0, 100.0), 0.75, &diamond_at(0.75, 258), false);
    assert_eq!(corners(&drawn.fill_even_odd), vec![(0.0, 100.0), (-7.5, 103.75), (-15.0, 100.0), (-7.5, 96.25), (0.0, 100.0)]);
    let shaft = drawn.shaft.bounding_box();
    assert!((shaft.y1 / POINT - 100.0).abs() < 1e-9, "untrimmed: {shaft:?}");
    // 2 on the same line turns with it: the diamond points up the page, its back 15 below the tip, and the line stops there.
    let turning = ended((0.0, 0.0), (0.0, 100.0), 0.75, &diamond_at(0.75, 2), false);
    let back = corners(&turning.fill_even_odd)[2];
    assert!(back.0.abs() < 1e-6 && (back.1 - 85.0).abs() < 1e-6, "{back:?}");
    assert!((turning.shaft.bounding_box().y1 / POINT - 85.0).abs() < 1e-6);
}

#[test]
fn a_scaled_end_keeps_the_masters_own_size_and_the_weight_and_size_class_do_not_change_it() {
    // Flags 1026 on a page of 1:1 with a master whose box is 0.1 inch tall: the box stays 0.1 inch tall, the drawing at its own size, whatever the weight and class.
    let big = 0.1;
    let points = [(big, big / 2.0), (-9.0 * big, 5.5 * big), (-19.0 * big, big / 2.0), (-9.0 * big, -4.5 * big)];
    let form = form_of(&doc_with(1026, master_shape(big, big, (big, big / 2.0), &points))).unwrap();
    for (size, weight) in [(0, 0.0), (2, 0.75 * POINT), (4, 3.0 * POINT), (6, 20.0 * POINT)] {
        assert_eq!(form.sizing(size, weight, 1.0), Ok(Sizing::By(1.0)), "class {size}, weight {weight}");
    }
    // On a page of 1:2 (half a paper inch to a drawing inch) it is half the size, whatever else.
    assert_eq!(form.sizing(2, 1.0, 0.5), Ok(Sizing::By(0.5)));
    let sized = form.sized(1.0).unwrap();
    assert!((sized.trim - 2.0).abs() < 1e-9, "the back is 20 boxes behind the pin: {}", sized.trim);
}

#[test]
fn an_unscaled_end_is_as_tall_as_the_weight_at_medium_size_and_the_class_share_elsewhere() {
    let form = form_of(&doc_with(2, master_shape(H, H, (H, H / 2.0), &diamond_points()))).unwrap();
    // At Medium the box is as tall as the weight: the factor is the weight over the box's height.
    let weight = 0.02;
    assert_eq!(form.sizing(2, weight, 1.0), Ok(Sizing::By(weight / H)));
    // At class n it is the weight times S(n) over S(2), the unit at the same weight: at 0.02 inch the units are 0.04, 0.055 and 0.075.
    let (s0, s2, s4) = (0.020 + weight, 0.035 + weight, 0.055 + weight);
    let by = |size: i64| match form.sizing(size, weight, 1.0) {
        Ok(Sizing::By(f)) => f * H / weight,
        other => panic!("{other:?}"),
    };
    assert!((by(0) - s0 / s2).abs() < 1e-12 && (by(2) - 1.0).abs() < 1e-12 && (by(4) - s4 / s2).abs() < 1e-12);
    // A class outside 0 to 6 is read as 2.
    assert!((by(9) - 1.0).abs() < 1e-12 && (by(-1) - 1.0).abs() < 1e-12);
    // A weight of 0 has a box of no height: nothing is drawn. The box of a master with no height cannot size an unscaled end.
    assert_eq!(form.sizing(2, 0.0, 1.0), Ok(Sizing::Nothing));
    let flat = form_of(&doc_with(2, master_shape(H, 0.0, (H, 0.0), &diamond_points()))).unwrap();
    assert_eq!(flat.sizing(2, weight, 1.0), Err(Refusal::Unusable));
    // A factor that is no number, or past what a drawing holds, is an end the line's size cannot draw.
    let tiny = form_of(&doc_with(2, master_shape(H, 1.0e-300, (H, 0.0), &diamond_points()))).unwrap();
    assert!(matches!(tiny.sizing(2, 1.0e300, 1.0), Err(Refusal::Unusable) | Ok(Sizing::By(_))));
    if let Ok(Sizing::By(factor)) = tiny.sizing(2, weight, 1.0) {
        assert!(tiny.sized(factor).is_none(), "a drawing past {MOST_REACH} inches is not drawn");
    }
}

#[test]
fn a_master_is_counted_by_its_sections_and_rows_and_refused_before_anything_is_built() {
    let mut ctx = eval_ctx();
    let doc = doc_with(2, master_shape(H, H, (H, H / 2.0), &diamond_points()));
    let cx = Cx::new(&doc, &mut ctx);
    let counted = survey(&cx, master_named(&doc, "Filled Diamond").unwrap()).unwrap();
    // One shape and one section of the five rows of the diamond, closed.
    assert_eq!((counted.shapes, counted.rows, counted.flags, counted.master), (1, 5, 2, 1));
    // Not a line end: no 2 in the flags.
    for flags in [0, 1, 65, 4100] {
        let doc = doc_with(flags, master_shape(H, H, (H, H / 2.0), &diamond_points()));
        let cx = Cx::new(&doc, &mut ctx);
        assert_eq!(survey(&cx, master_named(&doc, "Filled Diamond").unwrap()), Err(Refusal::NotAnEnd), "flags {flags}");
    }
    // No section at all, past the sections, past the rows.
    let none = doc_with(2, ShapeBuilder::new(5).cell("Width", "0.1").cell("Height", "0.1"));
    let cx = Cx::new(&none, &mut ctx);
    assert_eq!(survey(&cx, master_named(&none, "Filled Diamond").unwrap()), Err(Refusal::NoGeometry));
    let long: Vec<(f64, f64)> = (0..MAX_END_ROWS + 1).map(|i| (i as f64 * 1.0e-3, (i % 2) as f64 * 1.0e-3)).collect();
    let past = doc_with(2, master_shape(H, H, (H, H / 2.0), &long));
    let cx = Cx::new(&past, &mut ctx);
    assert_eq!(survey(&cx, master_named(&past, "Filled Diamond").unwrap()), Err(Refusal::PastCaps));
    let edge: Vec<(f64, f64)> = (0..MAX_END_ROWS - 1).map(|i| (i as f64 * 1.0e-3, (i % 2) as f64 * 1.0e-3)).collect();
    let at_cap = doc_with(2, master_shape(H, H, (H, H / 2.0), &edge));
    let cx = Cx::new(&at_cap, &mut ctx);
    assert_eq!(survey(&cx, master_named(&at_cap, "Filled Diamond").unwrap()).map(|s| s.rows), Ok(MAX_END_ROWS));
}

#[test]
fn a_master_is_found_by_its_name_or_its_unique_id_and_a_use_names_one_or_none() {
    let mut doc = doc_with(2, master_shape(H, H, (H, H / 2.0), &diamond_points()));
    if let Some(master) = doc.masters.get_mut(0) {
        Arc::make_mut(master).unique_id = Some(Arc::from("{AAAAAAAA-0000-4000-8000-000000000001}"));
    }
    assert_eq!(master_named(&doc, "Filled Diamond").map(|m| m.id), Some(1));
    assert_eq!(master_named(&doc, "{AAAAAAAA-0000-4000-8000-000000000001}").map(|m| m.id), Some(1));
    assert!(master_named(&doc, "filled diamond").is_none() && master_named(&doc, "").is_none(), "the name is the master's NameU as it is written");
    // The name a cell's `USE` holds: a string and a GUID, found through the cell's own formula; a formula that is not a `USE` holds none.
    let line = |formula: &str| {
        let d =
            DocBuilder::new().page(1, PageBuilder::new().shape(ShapeBuilder::new(1).cell("Width", "1").formula("EndArrow", formula, "254"))).build();
        let mut ctx = eval_ctx();
        let cx = Cx::new(&d, &mut ctx);
        let at = diacraft_doc::SheetRef::Shape(diacraft_doc::ShapeRef::top(diacraft_doc::Owner::Page(1), 0));
        used_name(&cx, &at, "EndArrow").map(|n| n.to_string())
    };
    assert_eq!(line("USE(\"Filled Diamond\")").as_deref(), Some("Filled Diamond"));
    assert_eq!(line("use( \"Quo\"\"te\" )").as_deref(), Some("Quo\"te"));
    assert_eq!(line("USE({12345678-1234-4234-8234-123456789ABC})").as_deref(), Some("{12345678-1234-4234-8234-123456789ABC}"));
    for not in ["THEMEVAL()", "254", "USE()", "USE(1)", "GUARD(254)"] {
        assert_eq!(line(not), None, "{not}");
    }
}
