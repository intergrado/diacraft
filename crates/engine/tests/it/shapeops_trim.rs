//! Trim: `shape.trim` through `Session::execute` as the control channel calls it. Each test is named for the vector or the rule of
//! it holds to. Numbers are inches on a page of the default scale.
//!
//! **The order of the pieces** (SHAPEOPS-ORDER) is: the operands in the order they were named, and for each one its pieces **in path order from its first cut**:
//! a closed path starts at the first place it is cut after its own start, so the piece that holds the path's start comes last. The vectors list pieces as sets and
//! fix no order, so the tests that name an order hold to this one.

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Document, Row, Value};
use kurbo::Shape as _;
use serde_json::json;

use crate::shapeops::{
    Build, Pt, assert_near, assert_points, assert_unchanged, at, built, doc, error_of, flag_of, ids, line, local, near, num, pose_map, rect,
    row_types, section_count, session_with, top,
};
use crate::shapeops_cases::{Dir, cells_of, has_id, id_of, instance, text_of};
use crate::shapeops_join::{in_order_or_reverse, section_on_page};
use crate::{blank_session, run, snapshot};

fn red() -> Cell {
    Cell::constant(Value::from_text("#FF0000"))
}

fn blue() -> Cell {
    Cell::constant(Value::from_text("#0000FF"))
}

fn value_of(d: &Document, id: u32, name: &str) -> Option<String> {
    local(d, id, name).and_then(|(_, value)| value)
}

fn row_of(ix: u32, kind: &str, cells: &[(&str, f64)]) -> Arc<Row> {
    let mut row = Row { ix: Some(ix), row_type: Some(Arc::from(kind)), ..Row::default() };
    for (name, n) in cells {
        row.cells.insert(CellName::new(name), Cell::constant(Value::from_text(&format!("{n}"))).with_unit("IN"));
    }
    Arc::new(row)
}

/// An ellipse in the box (`x0`, `y0`) to (`x1`, `y1`): one closed `Ellipse` row, which starts at its right end and runs anticlockwise.
pub(crate) fn ellipse(id: u32, (x0, y0, x1, y1): (f64, f64, f64, f64)) -> Build {
    let (w, h) = (x1 - x0, y1 - y0);
    let row = row_of(1, "Ellipse", &[("X", w / 2.0), ("Y", h / 2.0), ("A", w), ("B", h / 2.0), ("C", w / 2.0), ("D", h)]);
    Build::new(id, w, h).pin((x0 + x1) / 2.0, (y0 + y1) / 2.0).raw_section(vec![row], false)
}

/// The box of the shape's section `k` on the page.
fn page_box(d: &Document, id: u32, k: usize) -> kurbo::Rect {
    let mut path = built(d, id)[k].1.clone();
    path.apply_affine(pose_map(d, id));
    path.bounding_box()
}

/// A box within `tolerance` of the wanted one: the sides of a curve's box are as exact as the curve is, which is within 1E-4.
fn assert_box(what: &str, got: kurbo::Rect, want: (f64, f64, f64, f64), tolerance: f64) {
    let within = |a: f64, b: f64| (a - b).abs() <= tolerance;
    assert!(
        within(got.x0, want.0) && within(got.y0, want.1) && within(got.x1, want.2) && within(got.y1, want.3),
        "{what}: got ({}, {}) to ({}, {}), want ({}, {}) to ({}, {})",
        got.x0,
        got.y0,
        got.x1,
        got.y1,
        want.0,
        want.1,
        want.2,
        want.3
    );
}

// ----: a rectangle and a line --------------------------------------------------------------------------------------

#[test]
fn v18_a_line_across_a_rectangle_gives_five_open_pieces_with_the_rectangles_line_and_no_text() {
    let mut s = session_with(vec![
        rect(1, (0.0, 0.0, 4.0, 2.0)).cell("LineColor", red()).text("R").build(),
        line(2, (2.0, -1.0), (2.0, 3.0)).cell("LineColor", blue()).text("L").build(),
    ]);
    let result = run(&mut s, "shape.trim", json!({"ids": [1, 2]}));
    let made = ids(&result, "ids");
    assert_eq!(made.len(), 5, "{result}");
    assert_eq!(ids(&result, "deleted"), [1, 2]);
    let d = doc(&s);
    assert_eq!(top(d), made, "the originals are gone and the pieces are the last of the page, the last listed frontmost (SHAPEOPS-ORDER)");
    for id in &made {
        assert_eq!(section_count(d, *id), 1);
        assert_eq!(flag_of(d, *id, 0, "NoFill"), 1.0, "every piece is open: NoFill 1");
        assert_eq!(value_of(d, *id, "LineColor").as_deref(), Some("#FF0000"), "the rectangle's line, the first shape's");
        assert_eq!(text_of(d, *id), None, "no text");
    }
    // From R, then the line's three pieces, in path order from the first cut.
    assert_points("R, right", &section_on_page(d, made[0], 0), &[(2.0, 0.0), (4.0, 0.0), (4.0, 2.0), (2.0, 2.0)]);
    assert_points("R, left", &section_on_page(d, made[1], 0), &[(2.0, 2.0), (0.0, 2.0), (0.0, 0.0), (2.0, 0.0)]);
    assert!(in_order_or_reverse(&section_on_page(d, made[2], 0), &[(2.0, -1.0), (2.0, 0.0)]));
    assert!(in_order_or_reverse(&section_on_page(d, made[3], 0), &[(2.0, 0.0), (2.0, 2.0)]));
    assert!(in_order_or_reverse(&section_on_page(d, made[4], 0), &[(2.0, 2.0), (2.0, 3.0)]));
    // The middle piece has no width, and its x coordinates are the constant 0.
    assert_near("Width of the middle piece", num(d, made[3], "Width"), 0.0);
    assert_near("Height of the middle piece", num(d, made[3], "Height"), 2.0);
    let shape = d.shape(&at(d, made[3])).unwrap();
    let section = shape.sheet.section(&diacraft_doc::SectionKey::new("Geometry", Some(0))).unwrap();
    for row in section.rows.iter() {
        let x = row.cells.get("X").unwrap();
        assert!(x.formula.text().is_none() && x.value.as_ref().map(|v| v.text()) == Some("0"), "x is the constant 0: {x:?}");
    }
}

#[test]
fn the_pieces_are_two_dimensional_shapes_each_with_the_frame_of_its_own_box() {
    let mut s = session_with(vec![rect(1, (0.0, 0.0, 4.0, 2.0)).build(), line(2, (2.0, -1.0), (2.0, 3.0)).build()]);
    let made = ids(&run(&mut s, "shape.trim", json!({"ids": [1, 2]})), "ids");
    let d = doc(&s);
    // R's right piece: x 2 to 4, y 0 to 2.
    for (cell, want) in
        [("Width", 2.0), ("Height", 2.0), ("PinX", 3.0), ("PinY", 1.0), ("LocPinX", 1.0), ("LocPinY", 1.0), ("Angle", 0.0), ("FlipX", 0.0)]
    {
        assert_near(cell, num(d, made[0], cell), want);
    }
    assert!(made.iter().all(|id| d.shape(&at(d, *id)).unwrap().kind == diacraft_doc::ShapeType::Shape));
}

// ---- V19: Learn's example -------------------------------------------------------------------------------------------------------

#[test]
fn v19_a_rectangle_and_an_oval_give_four_shapes() {
    let mut s = session_with(vec![rect(1, (1.0, 1.0, 4.0, 4.0)).build(), ellipse(2, (2.0, 2.0, 3.0, 6.0)).build()]);
    let made = ids(&run(&mut s, "shape.trim", json!({"ids": [1, 2]})), "ids");
    assert_eq!(made.len(), 4, "Fragment's regions would give three");
    let d = doc(&s);
    assert_points("the rectangle's top between (2,4) and (3,4)", &section_on_page(d, made[0], 0), &[(3.0, 4.0), (2.0, 4.0)]);
    assert_points(
        "the rest of the rectangle",
        &section_on_page(d, made[1], 0),
        &[(2.0, 4.0), (1.0, 4.0), (1.0, 1.0), (4.0, 1.0), (4.0, 4.0), (3.0, 4.0)],
    );
    // The oval's two halves, each by its true extent: the upper through (2.5, 6), the lower through (2.5, 2).
    assert_box("the oval's upper half", page_box(d, made[2], 0), (2.0, 4.0, 3.0, 6.0), 1.0e-4);
    assert_box("its lower half", page_box(d, made[3], 0), (2.0, 2.0, 3.0, 4.0), 1.0e-4);
    for id in &made {
        assert_eq!(flag_of(d, *id, 0, "NoFill"), 1.0);
    }
}

#[test]
fn the_pieces_of_a_curve_are_curves_that_lie_on_it() {
    // A circle of radius 1 about the origin, and a chord across it at y = 0.5.
    let mut s = session_with(vec![ellipse(1, (-1.0, -1.0, 1.0, 1.0)).build(), line(2, (-2.0, 0.5), (2.0, 0.5)).build()]);
    let made = ids(&run(&mut s, "shape.trim", json!({"ids": [1, 2]})), "ids");
    assert_eq!(made.len(), 5, "two arcs and the line's three pieces");
    let d = doc(&s);
    let mut lengths = Vec::new();
    for id in &made[..2] {
        let mut path = built(d, *id)[0].1.clone();
        path.apply_affine(pose_map(d, *id));
        for seg in path.segments() {
            for k in 0..=10 {
                let p = kurbo::ParamCurve::eval(&seg, f64::from(k) / 10.0);
                assert!((p.to_vec2().hypot() - 1.0).abs() < 1.0e-4, "a point of an arc is on the circle: {p:?}");
            }
        }
        lengths.push(path.perimeter(1e-9));
    }
    assert!((lengths[0] - 2.0 * std::f64::consts::PI / 3.0).abs() < 1.0e-3, "the upper arc is 120 degrees: {lengths:?}");
    assert!((lengths[1] - 4.0 * std::f64::consts::PI / 3.0).abs() < 1.0e-3, "the lower arc is 240 degrees: {lengths:?}");
}

// ----: no crossing ------------------------------------------------------------------------------------------------------

#[test]
fn v20_two_disjoint_squares_come_back_as_two_closed_shapes_with_the_first_shapes_formatting() {
    let mut s = session_with(vec![
        rect(1, (0.0, 0.0, 1.0, 1.0)).cell("LineColor", red()).build(),
        rect(2, (3.0, 0.0, 4.0, 1.0)).cell("LineColor", blue()).build(),
    ]);
    let made = ids(&run(&mut s, "shape.trim", json!({"ids": [1, 2]})), "ids");
    assert_eq!(made.len(), 2);
    let d = doc(&s);
    for (id, corners) in made.iter().zip([[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)], [(3.0, 0.0), (4.0, 0.0), (4.0, 1.0), (3.0, 1.0)]]) {
        assert_eq!(flag_of(d, *id, 0, "NoFill"), 0.0, "a closed path nothing crosses stays closed");
        assert_eq!(row_types(d, *id, 0), ["MoveTo", "LineTo", "LineTo", "LineTo", "LineTo"]);
        assert_points("the square", &section_on_page(d, *id, 0), &corners);
        assert_eq!(value_of(d, *id, "LineColor").as_deref(), Some("#FF0000"), "both take the first shape's formatting");
    }
}

#[test]
fn an_open_path_that_nothing_crosses_comes_back_whole() {
    let mut s = session_with(vec![
        Build::new(1, 2.0, 1.0).pin(11.0, 10.5).section(&[(0.0, 0.0), (1.0, 1.0), (2.0, 0.0)], false, true).build(),
        rect(2, (0.0, 0.0, 1.0, 1.0)).build(),
    ]);
    let made = ids(&run(&mut s, "shape.trim", json!({"ids": [1, 2]})), "ids");
    assert_eq!(made.len(), 2);
    let d = doc(&s);
    assert_eq!(row_types(d, made[0], 0), ["MoveTo", "LineTo", "LineTo"]);
    assert!(in_order_or_reverse(&section_on_page(d, made[0], 0), &[(10.0, 10.0), (11.0, 11.0), (12.0, 10.0)]));
    assert_eq!(flag_of(d, made[0], 0, "NoFill"), 1.0);
}

// ----: one shape, its own crossings --------------------------------------------------------------------------------------

#[test]
fn trim_takes_one_shape_and_cuts_it_where_it_crosses_itself() {
    // A bowtie: (0,0), (2,2), (2,0), (0,2), whose first and third edges cross at (1,1).
    let bowtie = Build::new(1, 2.0, 2.0).pin(1.0, 1.0).section(&[(0.0, 0.0), (2.0, 2.0), (2.0, 0.0), (0.0, 2.0)], true, false).build();
    let mut s = session_with(vec![bowtie]);
    let made = ids(&run(&mut s, "shape.trim", json!({"ids": [1]})), "ids");
    assert_eq!(made.len(), 2, "the two lobes");
    let d = doc(&s);
    assert_points("the right lobe", &section_on_page(d, made[0], 0), &[(1.0, 1.0), (2.0, 2.0), (2.0, 0.0)]);
    assert_points("the left lobe", &section_on_page(d, made[1], 0), &[(1.0, 1.0), (0.0, 2.0), (0.0, 0.0)]);
    for id in &made {
        assert_eq!(flag_of(d, *id, 0, "NoFill"), 1.0, "a piece that cutting made is open, though its ends meet");
    }
}

#[test]
fn a_closed_path_cut_in_one_place_gives_one_open_piece_starting_and_ending_there() {
    // A line that ends on the bottom of the square at (1, 0): the square is cut once, there.
    let mut s = session_with(vec![rect(1, (0.0, 0.0, 2.0, 2.0)).build(), line(2, (1.0, -1.0), (1.0, 0.0)).build()]);
    let made = ids(&run(&mut s, "shape.trim", json!({"ids": [1, 2]})), "ids");
    assert_eq!(made.len(), 2, "the square as one piece, and the line");
    let d = doc(&s);
    assert_points(
        "the square from the cut round to the cut",
        &section_on_page(d, made[0], 0),
        &[(1.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0), (0.0, 0.0)],
    );
    assert_eq!(flag_of(d, made[0], 0, "NoFill"), 1.0, "open: it was cut");
    assert!(in_order_or_reverse(&section_on_page(d, made[1], 0), &[(1.0, -1.0), (1.0, 0.0)]));
}

#[test]
fn a_path_is_cut_where_another_ends_on_it_and_not_where_two_ends_meet() {
    let mut s = session_with(vec![line(1, (0.0, 0.0), (4.0, 0.0)).build(), line(2, (2.0, 0.0), (2.0, 2.0)).build()]);
    let made = ids(&run(&mut s, "shape.trim", json!({"ids": [1, 2]})), "ids");
    assert_eq!(made.len(), 3, "the first line cut at (2,0), and the second whole");
    let d = doc(&s);
    assert!(in_order_or_reverse(&section_on_page(d, made[0], 0), &[(0.0, 0.0), (2.0, 0.0)]));
    assert!(in_order_or_reverse(&section_on_page(d, made[1], 0), &[(2.0, 0.0), (4.0, 0.0)]));
    assert!(in_order_or_reverse(&section_on_page(d, made[2], 0), &[(2.0, 0.0), (2.0, 2.0)]));
    // Two lines that meet end to end are already two stretches: nothing is cut.
    let mut s = session_with(vec![line(1, (0.0, 0.0), (1.0, 0.0)).build(), line(2, (1.0, 0.0), (2.0, 0.0)).build()]);
    let made = ids(&run(&mut s, "shape.trim", json!({"ids": [1, 2]})), "ids");
    assert_eq!(made.len(), 2);
}

#[test]
fn a_path_whose_end_is_within_the_closure_tolerance_of_another_touches_it_and_one_beyond_does_not() {
    // R35 with the tolerance: the end of the second line is 5E-6 above the first, which touches it; at 2E-5 it does not.
    for (gap, pieces) in [(0.000_005, 3), (0.000_02, 2)] {
        let mut s = session_with(vec![line(1, (0.0, 0.0), (4.0, 0.0)).build(), line(2, (2.0, gap), (2.0, 2.0)).build()]);
        let made = ids(&run(&mut s, "shape.trim", json!({"ids": [1, 2]})), "ids");
        assert_eq!(made.len(), pieces, "the end of the second line {gap} above the first");
    }
}

#[test]
fn a_stretch_two_paths_run_along_is_kept_once_in_the_place_of_the_earlier_operand() {
    // Two squares that share the edge x = 2: it is kept once, as part of the first square.
    let mut s = session_with(vec![rect(1, (0.0, 0.0, 2.0, 2.0)).build(), rect(2, (2.0, 0.0, 4.0, 2.0)).build()]);
    let made = ids(&run(&mut s, "shape.trim", json!({"ids": [1, 2]})), "ids");
    assert_eq!(made.len(), 3, "the shared edge once, the first square's other three sides, the second's other three");
    let d = doc(&s);
    assert!(
        in_order_or_reverse(&section_on_page(d, made[0], 0), &[(2.0, 0.0), (2.0, 2.0)]),
        "the shared edge, from the first square: {:?}",
        section_on_page(d, made[0], 0)
    );
    assert_points("the first square's other sides", &section_on_page(d, made[1], 0), &[(2.0, 2.0), (0.0, 2.0), (0.0, 0.0), (2.0, 0.0)]);
    assert_points("the second square's other sides", &section_on_page(d, made[2], 0), &[(2.0, 0.0), (4.0, 0.0), (4.0, 2.0), (2.0, 2.0)]);
}

#[test]
fn every_section_of_a_shape_is_a_path_of_its_own_and_the_hidden_ones_are_not_read() {
    // One shape with two crossing sections, and a hidden third that would cross them again.
    let two = Build::new(1, 2.0, 2.0)
        .pin(1.0, 1.0)
        .section(&[(0.0, 0.0), (2.0, 2.0)], false, true)
        .section(&[(0.0, 2.0), (2.0, 0.0)], false, true)
        .section(&[(0.0, 1.0), (2.0, 1.0)], false, true)
        .section_flag(2, "NoShow", true)
        .build();
    let mut s = session_with(vec![two]);
    let made = ids(&run(&mut s, "shape.trim", json!({"ids": [1]})), "ids");
    assert_eq!(made.len(), 4, "each of the two lines cut at (1,1); the hidden one is not a path");
}

// ----: what is produced and what is refused -----------------------------------------------------------------

#[test]
fn trim_needs_a_shape_with_a_path_and_says_so_in_words_and_changes_nothing() {
    let mut s = session_with(vec![
        rect(1, (0.0, 0.0, 1.0, 1.0)).build(),
        Build::new(2, 1.0, 1.0).pin(5.0, 5.0).build(),
        line(3, (7.0, 7.0), (8.0, 8.0)).build(),
        line(4, (8.0, 8.0), (9.0, 7.0)).build(),
    ]);
    let group = id_of(&run(&mut s, "shape.group", json!({"ids": [3, 4]})));
    let before = snapshot(&s);
    for (params, why) in [
        (json!({"ids": []}), "between 1 and 4096 shape ids"),
        (json!({"ids": [2]}), "no geometry to draw"),
        (json!({"ids": [group]}), "is a group"),
        (json!({"ids": [3, 1], "page": 1}), "share one container"),
    ] {
        let error = error_of(&mut s, "shape.trim", &params);
        assert!(error.to_string().contains(why), "{params}: {error}");
        assert!(error.to_string().contains("`shape.trim`"), "{error}");
        assert_unchanged(&s, &before, "a refused Trim");
    }
}

#[test]
fn a_shape_that_cannot_be_deleted_refuses_trim() {
    let mut s = session_with(vec![
        rect(1, (0.0, 0.0, 2.0, 2.0)).cell("LockDelete", Cell::constant(Value::from_text("1"))).build(),
        line(2, (1.0, -1.0), (1.0, 3.0)).build(),
    ]);
    let before = snapshot(&s);
    let error = error_of(&mut s, "shape.trim", &json!({"ids": [1, 2]}));
    assert!(error.to_string().contains("cannot be deleted"), "{error}");
    assert_unchanged(&s, &before, "a refused Trim");
}

#[test]
fn the_pieces_are_at_the_front_so_what_was_between_the_originals_is_behind_them() {
    let mut s =
        session_with(vec![rect(1, (0.0, 0.0, 2.0, 2.0)).build(), rect(2, (8.0, 8.0, 9.0, 9.0)).build(), line(3, (1.0, -1.0), (1.0, 3.0)).build()]);
    let made = ids(&run(&mut s, "shape.trim", json!({"ids": [1, 3]})), "ids");
    assert_eq!(made.len(), 5);
    let mut want = vec![2];
    want.extend(&made);
    assert_eq!(top(doc(&s)), want, "X then the pieces");
    assert!(made.iter().all(|id| *id > 3), "new ids, above every one on the page");
}

#[test]
fn a_turned_operand_takes_part_through_its_turn() {
    // A line 4 long turned 90 degrees about its middle (2, 1), so it runs from (2, -1) to (2, 3); it cuts the rectangle (0,0)-(4,2).
    let turned = Build::new(2, 4.0, 0.0).pin(2.0, 1.0).angle_degrees(90.0).section(&[(0.0, 0.0), (4.0, 0.0)], false, true).build();
    let mut s = session_with(vec![rect(1, (0.0, 0.0, 4.0, 2.0)).build(), turned]);
    let made = ids(&run(&mut s, "shape.trim", json!({"ids": [1, 2]})), "ids");
    assert_eq!(made.len(), 5, "as V18");
    let d = doc(&s);
    assert!(made.iter().all(|id| near(num(d, *id, "Angle"), 0.0)), "the pieces are unturned");
}

// ----: glue and undo -----------------------------------------------------------------------------------------------

#[test]
fn a_connector_glued_to_an_original_keeps_its_end_unglued_and_undo_brings_everything_back_in_one_step() {
    let mut s = blank_session();
    let (a, b, d_) = (instance(&mut s, (1.0, 1.0, 3.0, 3.0)), instance(&mut s, (2.0, 2.0, 4.0, 4.0)), instance(&mut s, (7.0, 1.0, 8.0, 2.0)));
    let connector = id_of(&run(&mut s, "connector.add", json!({"from": {"id": d_}, "to": {"id": a, "point": 0}})));
    let end_before = (num(doc(&s), connector, "EndX"), num(doc(&s), connector, "EndY"));
    let before = snapshot(&s);
    let connects_to = |d: &Document, target: u32| d.page(1).unwrap().connects.iter().filter(|c| c.to_sheet == target).count();
    assert_eq!(connects_to(doc(&s), a), 1);
    let made = ids(&run(&mut s, "shape.trim", json!({"ids": [a, b]})), "ids");
    assert!(made.len() >= 4, "the crossing squares are cut: {made:?}");
    let after = doc(&s);
    assert_eq!(connects_to(after, a), 0, "no Connect row for the end that was glued to the original");
    assert!(made.iter().all(|id| connects_to(after, *id) == 0), "the connector is glued to no piece");
    assert_eq!(connects_to(after, d_), 1, "its end at D is still glued");
    assert!(has_id(after, connector));
    assert_near("EndX", num(after, connector, "EndX"), end_before.0);
    assert_near("EndY", num(after, connector, "EndY"), end_before.1);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(doc(&s), before.document.as_ref().unwrap(), "undo brings the originals back as they were");
    assert_eq!((snapshot(&s).undo_steps, snapshot(&s).redo_steps), (before.undo_steps, 1), "one step, now to redo");
    run(&mut s, "edit.redo", json!({}));
    assert!(made.iter().all(|id| has_id(doc(&s), *id)) && !has_id(doc(&s), a));
}

#[test]
fn the_step_is_named_for_the_operation_and_a_refused_trim_adds_none() {
    let mut s = session_with(vec![rect(1, (0.0, 0.0, 2.0, 2.0)).build(), line(2, (1.0, -1.0), (1.0, 3.0)).build()]);
    let before = snapshot(&s);
    let _ = error_of(&mut s, "shape.trim", &json!({"ids": [99]}));
    assert_eq!(snapshot(&s).undo_steps, before.undo_steps);
    run(&mut s, "shape.trim", json!({"ids": [1, 2]}));
    let after = snapshot(&s);
    assert_eq!(after.undo_steps, before.undo_steps + 1);
    assert_eq!(after.next_undo.as_deref(), Some("Trim"));
}

// ----: inside a group ----------------------------------------------------------------------------------------------------

#[test]
fn shapes_picked_inside_a_group_give_pieces_that_are_the_groups_last_members_in_order() {
    let mut s =
        session_with(vec![rect(1, (0.0, 0.0, 2.0, 2.0)).build(), line(2, (1.0, -1.0), (1.0, 3.0)).build(), rect(3, (8.0, 8.0, 9.0, 9.0)).build()]);
    let group = id_of(&run(&mut s, "shape.group", json!({"ids": [1, 2, 3]})));
    let made = ids(&run(&mut s, "shape.trim", json!({"ids": [1, 2]})), "ids");
    assert_eq!(made.len(), 5);
    let d = doc(&s);
    let members: Vec<u32> = d.shape(&at(d, group)).unwrap().children.iter().map(|m| m.id).collect();
    let mut want = vec![3];
    want.extend(&made);
    assert_eq!(members, want, "C is unchanged and the pieces follow it, the last frontmost");
    assert_eq!(top(d), [group]);
}

// ---- the file and the far-out drawing -------------------------------------------------------------------------------------------

#[test]
fn the_pieces_save_to_vsdx_and_reopen_with_the_cells_equal() {
    let mut s = session_with(vec![rect(1, (0.0, 0.0, 4.0, 2.0)).text("A").build(), line(2, (2.0, -1.0), (2.0, 3.0)).build()]);
    let made = ids(&run(&mut s, "shape.trim", json!({"ids": [1, 2]})), "ids");
    let dir = Dir::new();
    let path = dir.path("out.vsdx");
    run(&mut s, "file.save_as", json!({"path": path}));
    let mut reopened = diacraft_engine::Session::new();
    run(&mut reopened, "file.open", json!({"path": path}));
    for id in made {
        assert_eq!(cells_of(doc(&s), id), cells_of(doc(&reopened), id), "shape {id}");
    }
}

#[test]
fn a_drawing_far_from_the_origin_is_cut_as_one_near_it() {
    let far = 1.0e8;
    let mut s =
        session_with(vec![rect(1, (far, far, far + 4.0, far + 2.0)).build(), line(2, (far + 2.0, far - 1.0), (far + 2.0, far + 3.0)).build()]);
    let made = ids(&run(&mut s, "shape.trim", json!({"ids": [1, 2]})), "ids");
    assert_eq!(made.len(), 5);
    let d = doc(&s);
    assert_points(
        "R, right",
        &section_on_page(d, made[0], 0).iter().map(|p| (p.0 - far, p.1 - far)).collect::<Vec<Pt>>(),
        &[(2.0, 0.0), (4.0, 0.0), (4.0, 2.0), (2.0, 2.0)],
    );
}
