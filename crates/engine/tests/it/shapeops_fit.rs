//! Fit Curve: `shape.fit_curve` through `Session::execute` as the control channel calls it. Each test is named for the vector or the rule of
//! it holds to (UNITS). Numbers are inches on a page of the default scale, where they are the cells' own.
//!
//! **How a result is read.** As in `shapeops.rs`: not through the engine's own reader of rows, but as the renderer builds each section's path from the rows, carried
//! through the shape's pose.

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Document, Row, Section, SectionKey, Shape, Value};
use kurbo::{BezPath, ParamCurve, ParamCurveNearest, Point};
use serde_json::json;

use crate::shapeops::{Build, assert_near, assert_unchanged, at, built, doc, error_of, ids, near, num, pose_map, put, rect, session_with, top};
use crate::{blank_session, run, snapshot};

// ---- building and reading ---------------------------------------------------------------------------------------------------

/// A row of constants.
fn row(ix: u32, kind: &str, cells: &[(&str, f64)]) -> Arc<Row> {
    let mut row = Row { ix: Some(ix), row_type: Some(Arc::from(kind)), ..Row::default() };
    for (name, n) in cells {
        row.cells.insert(CellName::new(name), Cell::constant(Value::from_text(&format!("{n}"))));
    }
    Arc::new(row)
}

/// A shown `Geometry` section `ix` of rows, the five flags 0 unless `no_fill`.
fn section(ix: u32, rows: Vec<Arc<Row>>, no_fill: bool) -> Arc<Section> {
    let mut section = Section::new("Geometry", Some(ix));
    for flag in ["NoFill", "NoLine", "NoShow", "NoSnap", "NoQuickDrag"] {
        section.cells.insert(CellName::new(flag), Cell::constant(Value::from_text(if flag == "NoFill" && no_fill { "1" } else { "0" })));
    }
    section.rows = rows.into();
    Arc::new(section)
}

/// A closed polyline through the points of a circle of radius `r` about (`cx`, `cy`) at every `degrees`, as one shape whose box is exactly the polygon's: `LineTo` rows.
fn polygon_circle(id: u32, (cx, cy): (f64, f64), r: f64, degrees: f64) -> Build {
    let n = (360.0 / degrees).round() as u32;
    let around: Vec<(f64, f64)> = (0..n)
        .map(|i| {
            let a = f64::from(i) * degrees.to_radians();
            (r * a.cos(), r * a.sin())
        })
        .collect();
    let (x0, x1) = (around.iter().map(|p| p.0).fold(f64::MAX, f64::min), around.iter().map(|p| p.0).fold(f64::MIN, f64::max));
    let (y0, y1) = (around.iter().map(|p| p.1).fold(f64::MAX, f64::min), around.iter().map(|p| p.1).fold(f64::MIN, f64::max));
    let local: Vec<(f64, f64)> = around.iter().map(|p| (p.0 - x0, p.1 - y0)).collect();
    // The local pin is the middle by formula, as a drawn shape's is: a refit box keeps its pin in the middle.
    let middle = |of: &str, side: f64| Cell::with_formula(of, Some(Value::from_text(&format!("{}", side / 2.0)))).with_unit("IN");
    Build::new(id, x1 - x0, y1 - y0)
        .pin(cx + (x0 + x1) / 2.0, cy + (y0 + y1) / 2.0)
        .cell("LocPinX", middle("Width*0.5", x1 - x0))
        .cell("LocPinY", middle("Height*0.5", y1 - y0))
        .section(&local, true, false)
}

/// The shape with a cell of its own set to this number.
fn with_cell(mut shape: Shape, name: &str, n: f64) -> Shape {
    Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new(name), Cell::constant(Value::from_text(&format!("{n}"))));
    shape
}

fn page_path(d: &Document, id: u32, section: usize) -> BezPath {
    let map = pose_map(d, id);
    let (_, mut stroke, _) = built(d, id).swap_remove(section);
    stroke.apply_affine(map);
    stroke
}
/// Every row type of every shown `Geometry` section of the shape, in order.
fn all_row_types(d: &Document, id: u32) -> Vec<String> {
    let shape = d.shape(&at(d, id)).unwrap();
    shape
        .sheet
        .sections
        .iter()
        .filter(|s| s.name.as_str() == "Geometry" && !s.del)
        .flat_map(|s| s.rows.iter().map(|r| r.row_type.as_deref().unwrap_or("").to_owned()).collect::<Vec<_>>())
        .collect()
}

/// The page path of every shown section of the shape, drawn as the rows say.
fn drawn(d: &Document, id: u32) -> Vec<BezPath> {
    (0..built(d, id).len()).map(|k| page_path(d, id, k)).collect()
}

/// The farthest a point of the paths `a` is from the paths `b`.
fn stray_between(a: &[BezPath], b: &[BezPath]) -> f64 {
    let points: Vec<Point> =
        a.iter().flat_map(|p| p.segments().flat_map(|seg| (0..=40).map(move |k| seg.eval(f64::from(k) / 40.0))).collect::<Vec<_>>()).collect();
    points
        .iter()
        .map(|p| b.iter().flat_map(|path| path.segments()).map(|sg| sg.nearest(*p, 1e-9).distance_sq.sqrt()).fold(f64::MAX, f64::min))
        .fold(0.0, f64::max)
}

#[test]
fn v23_fit_curve_of_a_circle_of_thirty_six_lines_gives_arcs_within_the_tolerance_and_the_same_shape() {
    let mut s = session_with(vec![polygon_circle(1, (0.0, 0.0), 1.0, 10.0).build()]);
    assert_eq!(all_row_types(doc(&s), 1).iter().filter(|k| *k == "LineTo").count(), 36);
    let old = drawn(doc(&s), 1);
    let before = snapshot(&s);
    let result = run(&mut s, "shape.fit_curve", json!({"ids": [1], "tolerance": 0.02, "flags": 2}));
    assert_eq!(ids(&result, "changed"), [1], "the same shape, kept");
    assert!(ids(&result, "ids").is_empty() && ids(&result, "deleted").is_empty());
    let d = doc(&s);
    assert_eq!(top(d), [1]);
    let kinds = all_row_types(d, 1);
    let segments = kinds.iter().filter(|k| *k != "MoveTo").count();
    assert!(segments < 36, "fewer segments: {kinds:?}");
    assert!(kinds.contains(&"ArcTo".to_owned()), "circular arcs: {kinds:?}");
    assert!(!kinds.iter().any(|k| k == "NURBSTo" || k == "SplineStart" || k == "SplineKnot"), "no spline rows: {kinds:?}");
    let new = drawn(d, 1);
    assert!(stray_between(&new, &old) <= 0.02, "every point of the new path is within 0.02 of the old one: {}", stray_between(&new, &old));
    assert!(stray_between(&old, &new) <= 0.02, "and the other way: {}", stray_between(&old, &new));
    // One undo step, and undo brings the lines back.
    assert_eq!(snapshot(&s).undo_steps, before.undo_steps + 1);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(doc(&s), before.document.as_ref().unwrap());
    run(&mut s, "edit.redo", json!({}));
    assert!(all_row_types(doc(&s), 1).contains(&"ArcTo".to_owned()));
}

#[test]
fn v23_with_a_tolerance_of_nothing_the_path_is_unchanged_and_the_command_makes_no_undo_step() {
    let mut s = session_with(vec![polygon_circle(1, (0.0, 0.0), 1.0, 10.0).build()]);
    let before = snapshot(&s);
    let result = run(&mut s, "shape.fit_curve", json!({"ids": [1], "tolerance": 0, "flags": 2}));
    assert!(ids(&result, "changed").is_empty());
    assert_unchanged(&s, &before, "a tolerance of 0");
}

#[test]
fn r42_without_the_circles_flag_the_circle_is_splines_and_no_arcs() {
    let mut s = session_with(vec![polygon_circle(1, (0.0, 0.0), 1.0, 10.0).build()]);
    let old = drawn(doc(&s), 1);
    run(&mut s, "shape.fit_curve", json!({"ids": [1], "tolerance": 0.02}));
    let kinds = all_row_types(doc(&s), 1);
    assert!(kinds.iter().any(|k| k == "NURBSTo"), "{kinds:?}");
    assert!(!kinds.iter().any(|k| k == "ArcTo" || k == "LineTo"), "{kinds:?}");
    let new = drawn(doc(&s), 1);
    assert!(stray_between(&new, &old) <= 0.02 && stray_between(&old, &new) <= 0.02);
}

#[test]
fn r42_the_flags_are_a_sum_of_one_two_and_four_and_nothing_else_and_a_tolerance_is_not_negative() {
    let mut s = session_with(vec![polygon_circle(1, (0.0, 0.0), 1.0, 10.0).build()]);
    let before = snapshot(&s);
    for bad in [
        json!({"flags": 8}),
        json!({"flags": 16}),
        json!({"flags": -1}),
        json!({"flags": 1.5}),
        json!({"tolerance": -0.1}),
        json!({"tolerance": "x"}),
    ] {
        let mut params = json!({"ids": [1], "tolerance": 0.02});
        for (k, v) in bad.as_object().unwrap() {
            params[k] = v.clone();
        }
        let error = error_of(&mut s, "shape.fit_curve", &params);
        assert!(error.to_string().contains("flags") || error.to_string().contains("tolerance"), "{params}: {error}");
        assert_unchanged(&s, &before, "bad arguments");
    }
    let missing = error_of(&mut s, "shape.fit_curve", &json!({"ids": [1]}));
    assert!(missing.to_string().contains("tolerance"), "{missing}");
    for flags in 0..8 {
        let mut s = session_with(vec![polygon_circle(1, (0.0, 0.0), 1.0, 10.0).build()]);
        run(&mut s, "shape.fit_curve", json!({"ids": [1], "tolerance": 0.02, "flags": flags}));
    }
}

#[test]
fn r43_the_box_is_refitted_to_the_new_geometry_with_nothing_moving_on_the_page_unless_the_size_is_locked() {
    // 50 points at 7.2 degrees: no vertex at the top or the bottom, so the polygon is 1.9961 high and the arcs reach the circle's top and bottom, 2 apart.
    let circle = || polygon_circle(1, (3.0, 4.0), 1.0, 7.2).build();
    let mut s = session_with(vec![circle()]);
    let old = drawn(doc(&s), 1);
    let h0 = num(doc(&s), 1, "Height");
    assert!(near(num(doc(&s), 1, "Width"), 2.0) && (h0 - 1.996_053_456_856_543).abs() < 1e-9, "the box is the polygon's: {h0}");
    run(&mut s, "shape.fit_curve", json!({"ids": [1], "tolerance": 0.02, "flags": 2}));
    let d = doc(&s);
    assert!((num(d, 1, "Height") - 2.0).abs() < 1e-6, "the box is the arcs': {}", num(d, 1, "Height"));
    assert_near("Width", num(d, 1, "Width"), 2.0);
    assert_near("PinX", num(d, 1, "PinX"), 3.0);
    assert!((num(d, 1, "PinY") - 4.0).abs() < 1e-6, "{}", num(d, 1, "PinY"));
    assert!(stray_between(&drawn(d, 1), &old) <= 0.02 && stray_between(&old, &drawn(d, 1)) <= 0.02, "nothing moved on the page");
    // With LockCalcWH the box stays exactly as it was and the geometry is written in it.
    let mut s = session_with(vec![with_cell(circle(), "LockCalcWH", 1.0)]);
    let (w, h, px, py) = (num(doc(&s), 1, "Width"), num(doc(&s), 1, "Height"), num(doc(&s), 1, "PinX"), num(doc(&s), 1, "PinY"));
    run(&mut s, "shape.fit_curve", json!({"ids": [1], "tolerance": 0.02, "flags": 2}));
    let d = doc(&s);
    assert_eq!(
        (num(d, 1, "Width"), num(d, 1, "Height"), num(d, 1, "PinX"), num(d, 1, "PinY")),
        (w, h, px, py),
        "the frame is left exactly as it was"
    );
    assert!(all_row_types(d, 1).contains(&"ArcTo".to_owned()));
    assert!(stray_between(&drawn(d, 1), &old) <= 0.02, "and it draws the same");
}

#[test]
fn r13_a_shape_whose_vertices_are_locked_refuses_fit_curve_and_the_other_shapes_are_not_changed_either() {
    let mut s = session_with(vec![
        polygon_circle(1, (0.0, 0.0), 1.0, 10.0).build(),
        with_cell(polygon_circle(2, (5.0, 0.0), 1.0, 10.0).build(), "LockVtxEdit", 1.0),
    ]);
    let before = snapshot(&s);
    let error = error_of(&mut s, "shape.fit_curve", &json!({"ids": [1, 2], "tolerance": 0.02, "flags": 2}));
    assert!(error.to_string().contains("LockVtxEdit"), "{error}");
    assert_unchanged(&s, &before, "a vertices lock");
    // LockDelete refuses nothing: Fit Curve deletes nothing.
    let mut s = session_with(vec![with_cell(polygon_circle(1, (0.0, 0.0), 1.0, 10.0).build(), "LockDelete", 1.0)]);
    let result = run(&mut s, "shape.fit_curve", json!({"ids": [1], "tolerance": 0.02, "flags": 2}));
    assert_eq!(ids(&result, "changed"), [1]);
}

#[test]
fn r41_each_shape_is_fitted_on_its_own_and_one_the_fit_cannot_make_smaller_is_not_touched() {
    let mut s = session_with(vec![
        polygon_circle(1, (0.0, 0.0), 1.0, 10.0).build(),
        rect(2, (5.0, 0.0, 7.0, 2.0)).build(),
        polygon_circle(3, (10.0, 0.0), 1.0, 10.0).build(),
    ]);
    let rectangle_before = doc(&s).shape(&at(doc(&s), 2)).unwrap().clone();
    let result = run(&mut s, "shape.fit_curve", json!({"ids": [1, 2, 3], "tolerance": 0.02, "flags": 2}));
    assert_eq!(ids(&result, "changed"), [1, 3], "never combined, and the rectangle's four lines are no smaller");
    assert_eq!(top(doc(&s)), [1, 2, 3], "no shape made or deleted, none moved in the drawing order");
    assert_eq!(doc(&s).shape(&at(doc(&s), 2)).unwrap(), &rectangle_before);
}

#[test]
fn e10_a_hidden_section_and_a_section_the_fit_leaves_are_kept_and_the_frame_with_them() {
    let mut shape = polygon_circle(1, (0.0, 0.0), 1.0, 10.0).build();
    let hidden = section(5, vec![row(1, "MoveTo", &[("X", 0.0), ("Y", 0.0)]), row(2, "LineTo", &[("X", 2.0), ("Y", 2.0)])], true);
    let mut hidden = (*hidden).clone();
    hidden.cells.insert(CellName::new("NoShow"), Cell::constant(Value::from_text("1")));
    Arc::make_mut(&mut shape.sheet).sections.push(Arc::new(hidden));
    let mut s = session_with(vec![shape]);
    let (w, h) = (num(doc(&s), 1, "Width"), num(doc(&s), 1, "Height"));
    run(&mut s, "shape.fit_curve", json!({"ids": [1], "tolerance": 0.02, "flags": 2}));
    let d = doc(&s);
    let kept = d.shape(&at(d, 1)).unwrap().sheet.section(&SectionKey::new("Geometry", Some(5))).expect("the hidden section is still there");
    assert_eq!(kept.rows.len(), 2);
    assert_eq!((num(d, 1, "Width"), num(d, 1, "Height")), (w, h), "a shape that keeps a section keeps its frame");
    assert!(all_row_types(d, 1).contains(&"ArcTo".to_owned()));
}

#[test]
fn r43_a_section_keeps_its_flags_when_its_rows_are_replaced() {
    let mut shape = Build::new(1, 2.0, 2.0).pin(0.0, 0.0).build();
    let points: Vec<(f64, f64)> =
        (0..36).map(|i| (1.0 + (f64::from(i) * 10f64.to_radians()).cos(), 1.0 + (f64::from(i) * 10f64.to_radians()).sin())).collect();
    let mut rows = vec![row(1, "MoveTo", &[("X", points[0].0), ("Y", points[0].1)])];
    for (i, p) in points.iter().enumerate().skip(1) {
        rows.push(row(u32::try_from(i + 1).unwrap(), "LineTo", &[("X", p.0), ("Y", p.1)]));
    }
    rows.push(row(37, "LineTo", &[("X", points[0].0), ("Y", points[0].1)]));
    let mut flagged = (*section(3, rows, true)).clone();
    for flag in ["NoLine", "NoSnap", "NoQuickDrag"] {
        flagged.cells.insert(CellName::new(flag), Cell::constant(Value::from_text("1")));
    }
    Arc::make_mut(&mut shape.sheet).sections.push(Arc::new(flagged));
    let mut s = session_with(vec![shape]);
    // The shape was built with one section 0 (empty of rows, from `Build::new`)? No: `Build::new` has none, so the one section is 3.
    run(&mut s, "shape.fit_curve", json!({"ids": [1], "tolerance": 0.02, "flags": 2}));
    let d = doc(&s);
    let shape = d.shape(&at(d, 1)).unwrap();
    let section = shape.sheet.sections.iter().find(|sec| sec.name.as_str() == "Geometry" && !sec.del).unwrap();
    let flag = |name: &str| section.cells.get(name).and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned()).unwrap();
    assert_eq!([flag("NoFill"), flag("NoLine"), flag("NoShow"), flag("NoSnap"), flag("NoQuickDrag")], ["1", "1", "0", "1", "1"]);
}

#[test]
fn e5_a_turned_operand_is_fitted_in_its_own_frame_and_draws_where_it_did() {
    let mut s = session_with(vec![polygon_circle(1, (4.0, 4.0), 1.0, 10.0).angle_degrees(30.0).build()]);
    let old = drawn(doc(&s), 1);
    run(&mut s, "shape.fit_curve", json!({"ids": [1], "tolerance": 0.02, "flags": 2}));
    let d = doc(&s);
    assert_near("the angle is kept", num(d, 1, "Angle"), 30f64.to_radians());
    assert!(all_row_types(d, 1).contains(&"ArcTo".to_owned()));
    assert!(stray_between(&drawn(d, 1), &old) <= 0.02 && stray_between(&old, &drawn(d, 1)) <= 0.02, "it draws where it did");
}

#[test]
fn unit_1_the_tolerance_is_the_cells_own_units_and_is_not_converted_by_the_drawing_scale() {
    // A quarter-inch scale: 48 drawing units to a paper inch. A circle of radius 48 units as 36 lines, fitted to 0.48 of a unit: arcs (the bow of a chord is 0.18 units,
    // under it). The same tolerance as a hundredth of a unit finds none (the chords are 0.18 off the circle, over it).
    let mut s = blank_session();
    run(&mut s, "page.scale", json!({"preset": "arch_1_4"}));
    put(&mut s, vec![polygon_circle(1, (100.0, 100.0), 48.0, 10.0).build()]);
    let old = drawn(doc(&s), 1);
    run(&mut s, "shape.fit_curve", json!({"ids": [1], "tolerance": 0.01, "flags": 2}));
    assert!(!all_row_types(doc(&s), 1).contains(&"ArcTo".to_owned()), "a hundredth of a unit is under the polygon's own error of 0.18");
    run(&mut s, "shape.fit_curve", json!({"ids": [1], "tolerance": 0.48, "flags": 2}));
    let d = doc(&s);
    assert!(all_row_types(d, 1).contains(&"ArcTo".to_owned()), "{:?}", all_row_types(d, 1));
    let off = stray_between(&drawn(d, 1), &old).max(stray_between(&old, &drawn(d, 1)));
    assert!(off <= 0.48 && off > 0.01, "within 0.48 of the polygon, and not within a hundredth of a unit: {off}");
}
