//! Join: `shape.join` through `Session::execute` as the control channel calls it. Each test is named for the vector or the rule of
//! it holds to. Numbers are inches on a page of the default scale, where
//! they are the cells' own.

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Document, ShapeType, Value};
use diacraft_engine::Session;
use diacraft_engine::draw::{Op, check_shape_ops};
use kurbo::{ParamCurve, Point};
use serde_json::json;

use crate::shapeops::{
    Build, Pt, assert_near, assert_points, assert_unchanged, at, built, doc, error_of, flag_of, ids, line, local, local_points, made, near, num,
    pose_map, rect, row_types, section_count, session_with, top,
};
use crate::shapeops_cases::{Dir, cells_of, has_id, id_of, instance, text_of};
use crate::{blank_session, run, snapshot};

/// The vertices of the shape's section number `k` on the page, in the order the rows pass through them (a closed path's start once).
pub(crate) fn section_on_page(d: &Document, id: u32, k: usize) -> Vec<Pt> {
    let map = pose_map(d, id);
    local_points(d, id)[k].iter().map(|p| map * Point::new(p.0, p.1)).map(|p| (p.x, p.y)).collect()
}

fn same_order(got: &[Pt], want: &[Pt]) -> bool {
    got.len() == want.len() && got.iter().zip(want).all(|(a, b)| near(a.0, b.0) && near(a.1, b.1))
}

/// Whether `got` is `want` or `want` backwards: a path's direction is not fixed by a vector unless it says so.
pub(crate) fn in_order_or_reverse(got: &[Pt], want: &[Pt]) -> bool {
    let backwards: Vec<Pt> = want.iter().rev().copied().collect();
    same_order(got, want) || same_order(got, &backwards)
}

fn red() -> Cell {
    Cell::constant(Value::from_text("#FF0000"))
}

fn value_of(d: &Document, id: u32, name: &str) -> Option<String> {
    local(d, id, name).and_then(|(_, value)| value)
}

/// The X cell of the rows of section `ix` as (formula, value).
fn x_cells(d: &Document, id: u32, ix: u32) -> Vec<(Option<String>, Option<String>)> {
    let shape = d.shape(&at(d, id)).unwrap();
    let section = shape.sheet.section(&diacraft_doc::SectionKey::new("Geometry", Some(ix))).unwrap();
    section
        .rows
        .iter()
        .map(|r| {
            let cell = r.cells.get("X").unwrap();
            (cell.formula.text().map(str::to_owned), cell.value.as_ref().map(|v| v.text().to_owned()))
        })
        .collect()
}

// ----: chaining -----------------------------------------------------------------------------------------------

#[test]
fn v15_three_lines_join_into_one_open_section_through_four_points_with_the_first_lines_line() {
    let mut s = session_with(vec![
        line(1, (0.0, 0.0), (2.0, 0.0)).cell("LineColor", red()).text("one").build(),
        line(2, (2.0, 0.0), (2.0, 2.0)).text("two").build(),
        line(3, (0.0, 2.0), (2.0, 2.0)).build(),
    ]);
    let result = run(&mut s, "shape.join", json!({"ids": [1, 2, 3]}));
    let id = made(&result);
    assert_eq!(ids(&result, "deleted"), [1, 2, 3]);
    assert!(result["kept"].is_null());
    let d = doc(&s);
    assert_eq!(top(d), [id], "the originals are gone and the produced shape is the last of the page");
    assert_eq!(section_count(d, id), 1, "one section, not one for each line as Combine would give");
    assert_eq!(flag_of(d, id, 0, "NoFill"), 1.0, "an open section does not fill");
    assert!(
        in_order_or_reverse(&section_on_page(d, id, 0), &[(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0)]),
        "through (0,0), (2,0), (2,2), (0,2): {:?}",
        section_on_page(d, id, 0)
    );
    for (cell, want) in [("Width", 2.0), ("Height", 2.0), ("PinX", 1.0), ("PinY", 1.0), ("Angle", 0.0)] {
        assert_near(cell, num(d, id, cell), want);
    }
    assert_eq!(value_of(d, id, "LineColor").as_deref(), Some("#FF0000"), "the first line's line");
    assert_eq!(text_of(d, id).as_deref(), Some("one"), "the first shape's text and no other");
}

#[test]
fn v15_a_fourth_line_back_to_the_start_closes_the_chain_and_it_takes_a_fill() {
    let mut s = session_with(vec![
        line(1, (0.0, 0.0), (2.0, 0.0)).build(),
        line(2, (2.0, 0.0), (2.0, 2.0)).build(),
        line(3, (0.0, 2.0), (2.0, 2.0)).build(),
        line(4, (0.0, 2.0), (0.0, 0.0)).build(),
    ]);
    let id = made(&run(&mut s, "shape.join", json!({"ids": [1, 2, 3, 4]})));
    let d = doc(&s);
    assert_eq!(section_count(d, id), 1);
    assert_eq!(flag_of(d, id, 0, "NoFill"), 0.0, "a closed section takes the fill");
    assert_eq!(row_types(d, id, 0), ["MoveTo", "LineTo", "LineTo", "LineTo", "LineTo"], "four segments, the last back to the start");
    assert_points("the corners", &section_on_page(d, id, 0), &[(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0)]);
}

#[test]
fn the_vertex_where_two_paths_join_is_written_once_and_segments_in_line_are_not_merged() {
    let mut s = session_with(vec![line(1, (0.0, 0.0), (1.0, 0.0)).build(), line(2, (1.0, 0.0), (2.0, 0.0)).build()]);
    let id = made(&run(&mut s, "shape.join", json!({"ids": [1, 2]})));
    let d = doc(&s);
    assert_eq!(row_types(d, id, 0), ["MoveTo", "LineTo", "LineTo"], "two segments, three vertices");
    assert!(in_order_or_reverse(&section_on_page(d, id, 0), &[(0.0, 0.0), (1.0, 0.0), (2.0, 0.0)]));
    assert_near("Width", num(d, id, "Width"), 2.0);
    assert_near("Height", num(d, id, "Height"), 0.0);
}

#[test]
fn paths_that_cross_without_abutting_are_not_cut_and_come_back_as_two_sections() {
    let mut s = session_with(vec![line(1, (0.0, 0.0), (2.0, 2.0)).build(), line(2, (0.0, 2.0), (2.0, 0.0)).build()]);
    let id = made(&run(&mut s, "shape.join", json!({"ids": [1, 2]})));
    let d = doc(&s);
    assert_eq!(section_count(d, id), 2, "Join does not cut: that is Trim's work");
    assert_points("the first line", &section_on_page(d, id, 0), &[(0.0, 0.0), (2.0, 2.0)]);
    assert_points("the second line", &section_on_page(d, id, 1), &[(0.0, 2.0), (2.0, 0.0)]);
}

// ----: the tolerance -----------------------------------------------------------------------------------------------

#[test]
fn v16_a_gap_of_a_hundredth_does_not_join_and_a_gap_of_five_millionths_does() {
    let mut s = session_with(vec![line(1, (0.0, 0.0), (2.0, 0.0)).build(), line(2, (2.01, 0.0), (2.0, 2.0)).build()]);
    let id = made(&run(&mut s, "shape.join", json!({"ids": [1, 2]})));
    assert_eq!(section_count(doc(&s), id), 2, "0.01 apart: two sections");
    let mut s = session_with(vec![line(1, (0.0, 0.0), (2.0, 0.0)).build(), line(2, (2.000_005, 0.0), (2.0, 2.0)).build()]);
    let id = made(&run(&mut s, "shape.join", json!({"ids": [1, 2]})));
    let d = doc(&s);
    assert_eq!(section_count(d, id), 1, "5E-6 apart is within the closure tolerance of 1E-5: one section");
    assert_eq!(row_types(d, id, 0), ["MoveTo", "LineTo", "LineTo"]);
    // ... and 2E-5 apart is not.
    let mut s = session_with(vec![line(1, (0.0, 0.0), (2.0, 0.0)).build(), line(2, (2.000_02, 0.0), (2.0, 2.0)).build()]);
    let id = made(&run(&mut s, "shape.join", json!({"ids": [1, 2]})));
    assert_eq!(section_count(doc(&s), id), 2, "2E-5 apart: two sections");
}

// ----: a branch ---------------------------------------------------------------------------------------------------------

#[test]
fn v17_where_three_ends_meet_the_earliest_pair_joins_and_the_rest_go_to_another_chain() {
    let mut s =
        session_with(vec![line(1, (0.0, 0.0), (1.0, 0.0)).build(), line(2, (1.0, 0.0), (2.0, 0.0)).build(), line(3, (1.0, 0.0), (1.0, 1.0)).build()]);
    let id = made(&run(&mut s, "shape.join", json!({"ids": [1, 2, 3]})));
    let d = doc(&s);
    assert_eq!(section_count(d, id), 2);
    assert!(in_order_or_reverse(&section_on_page(d, id, 0), &[(0.0, 0.0), (1.0, 0.0), (2.0, 0.0)]), "{:?}", section_on_page(d, id, 0));
    assert!(in_order_or_reverse(&section_on_page(d, id, 1), &[(1.0, 0.0), (1.0, 1.0)]), "{:?}", section_on_page(d, id, 1));
}

#[test]
fn a_chain_grows_at_its_head_as_well_as_its_tail_and_takes_the_earliest_path_that_abuts() {
    // L2 is first, so the chain starts there: its tail takes L3 (the earliest that abuts the tail), and its head takes L1.
    let mut s =
        session_with(vec![line(1, (0.0, 0.0), (1.0, 0.0)).build(), line(2, (1.0, 0.0), (2.0, 0.0)).build(), line(3, (2.0, 0.0), (3.0, 1.0)).build()]);
    let id = made(&run(&mut s, "shape.join", json!({"ids": [2, 1, 3]})));
    let d = doc(&s);
    assert_eq!(section_count(d, id), 1);
    assert!(in_order_or_reverse(&section_on_page(d, id, 0), &[(0.0, 0.0), (1.0, 0.0), (2.0, 0.0), (3.0, 1.0)]), "{:?}", section_on_page(d, id, 0));
}

#[test]
fn a_path_that_is_already_closed_passes_through_as_a_section_of_its_own_in_its_place() {
    let mut s =
        session_with(vec![line(1, (5.0, 5.0), (6.0, 5.0)).build(), rect(2, (0.0, 0.0, 1.0, 1.0)).build(), line(3, (6.0, 5.0), (6.0, 6.0)).build()]);
    let id = made(&run(&mut s, "shape.join", json!({"ids": [1, 2, 3]})));
    let d = doc(&s);
    assert_eq!(section_count(d, id), 2, "the two lines are one chain, the square is a section of its own");
    assert!(in_order_or_reverse(&section_on_page(d, id, 0), &[(5.0, 5.0), (6.0, 5.0), (6.0, 6.0)]), "the chain of the first path comes first");
    assert_eq!(flag_of(d, id, 0, "NoFill"), 1.0);
    assert_points("the square", &section_on_page(d, id, 1), &[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]);
    assert_eq!(flag_of(d, id, 1, "NoFill"), 0.0, "closed: it takes the fill");
}

#[test]
fn the_sections_of_one_shape_chain_with_each_other_and_with_the_other_shapes() {
    // One shape with two sections that abut, and a second shape that abuts the second section.
    let two =
        Build::new(1, 2.0, 1.0).pin(1.0, 0.5).section(&[(0.0, 0.0), (1.0, 0.0)], false, true).section(&[(1.0, 0.0), (2.0, 1.0)], false, true).build();
    let mut s = session_with(vec![two, line(2, (2.0, 1.0), (2.0, 3.0)).build()]);
    let id = made(&run(&mut s, "shape.join", json!({"ids": [1, 2]})));
    let d = doc(&s);
    assert_eq!(section_count(d, id), 1);
    assert!(in_order_or_reverse(&section_on_page(d, id, 0), &[(0.0, 0.0), (1.0, 0.0), (2.0, 1.0), (2.0, 3.0)]), "{:?}", section_on_page(d, id, 0));
}

#[test]
fn a_hidden_section_is_not_read_and_so_does_not_join() {
    let mut s = session_with(vec![
        line(1, (0.0, 0.0), (1.0, 0.0)).build(),
        Build::new(2, 1.0, 0.0)
            .pin(1.5, 0.0)
            .section(&[(0.0, 0.0), (1.0, 0.0)], false, true)
            .section_flag(0, "NoShow", true)
            .section(&[(0.0, 5.0), (1.0, 5.0)], false, true)
            .build(),
    ]);
    let id = made(&run(&mut s, "shape.join", json!({"ids": [1, 2]})));
    let d = doc(&s);
    assert_eq!(section_count(d, id), 2, "the hidden line from (1,0) to (2,0) is not carried and does not abut");
}

// ----: what is produced ----------------------------------------------------------------------------------

#[test]
fn the_produced_shape_is_at_the_front_so_what_was_between_the_originals_is_behind_it() {
    let mut s =
        session_with(vec![line(1, (0.0, 0.0), (1.0, 0.0)).build(), rect(2, (5.0, 5.0, 6.0, 6.0)).build(), line(3, (1.0, 0.0), (2.0, 0.0)).build()]);
    let id = made(&run(&mut s, "shape.join", json!({"ids": [1, 3]})));
    assert_eq!(top(doc(&s)), [2, id], "X then the produced shape");
    assert!(id > 3, "a new id above every one on the page");
}

#[test]
fn the_produced_shape_is_two_dimensional_with_the_pin_in_the_middle_and_its_rows_are_shares_of_the_box() {
    let mut s = session_with(vec![line(1, (1.0, 1.0), (4.0, 1.0)).build(), line(2, (4.0, 1.0), (4.0, 3.0)).build()]);
    let id = made(&run(&mut s, "shape.join", json!({"ids": [1, 2]})));
    let d = doc(&s);
    for (cell, want) in
        [("Width", 3.0), ("Height", 2.0), ("PinX", 2.5), ("PinY", 2.0), ("LocPinX", 1.5), ("LocPinY", 1.0), ("FlipX", 0.0), ("FlipY", 0.0)]
    {
        assert_near(cell, num(d, id, cell), want);
    }
    assert_eq!(local(d, id, "LocPinX").and_then(|c| c.0).as_deref(), Some("Width*0.5"));
    assert_eq!(d.shape(&at(d, id)).unwrap().kind, ShapeType::Shape, "a 2-D shape, even though every operand was 1-D (SHAPEOPS-2D)");
    let xs = x_cells(d, id, 0);
    assert_eq!(xs[0].0, None, "the first vertex is at the box's left: the constant 0");
    assert_eq!(xs[1].0.as_deref(), Some("Width"), "the second is at its right: plain Width");
}

#[test]
fn a_vertical_path_has_no_width_and_its_x_coordinates_are_the_constant_zero() {
    let mut s = session_with(vec![line(1, (2.0, 0.0), (2.0, 1.0)).build(), line(2, (2.0, 1.0), (2.0, 3.0)).build()]);
    let id = made(&run(&mut s, "shape.join", json!({"ids": [1, 2]})));
    let d = doc(&s);
    assert_near("Width", num(d, id, "Width"), 0.0);
    assert_near("Height", num(d, id, "Height"), 3.0);
    assert!(x_cells(d, id, 0).iter().all(|(formula, value)| formula.is_none() && value.as_deref() == Some("0")), "{:?}", x_cells(d, id, 0));
}

#[test]
fn a_turned_operand_takes_part_through_its_turn_and_the_result_is_unturned() {
    // A, 2 long and 0 high, with its pin at (2, 2) turned 90 degrees, runs from (2, 1) to (2, 3); B starts where A ends.
    let a = Build::new(1, 2.0, 0.0).pin(2.0, 2.0).angle_degrees(90.0).section(&[(0.0, 0.0), (2.0, 0.0)], false, true).build();
    let mut s = session_with(vec![a, line(2, (2.0, 3.0), (5.0, 3.0)).build()]);
    let id = made(&run(&mut s, "shape.join", json!({"ids": [1, 2]})));
    let d = doc(&s);
    assert_eq!(section_count(d, id), 1, "A's end is at (2,3) once it is turned");
    assert_near("Angle", num(d, id, "Angle"), 0.0);
    assert!(in_order_or_reverse(&section_on_page(d, id, 0), &[(2.0, 1.0), (2.0, 3.0), (5.0, 3.0)]), "{:?}", section_on_page(d, id, 0));
}

/// A row of constants in inches.
fn plain_row(ix: u32, kind: &str, x: f64, y: f64) -> diacraft_doc::Row {
    let mut row = diacraft_doc::Row { ix: Some(ix), row_type: Some(Arc::from(kind)), ..diacraft_doc::Row::default() };
    row.cells.insert(CellName::new("X"), Cell::constant(Value::from_text(&format!("{x}"))).with_unit("IN"));
    row.cells.insert(CellName::new("Y"), Cell::constant(Value::from_text(&format!("{y}"))).with_unit("IN"));
    row
}

/// An open shape of one clamped cubic from `from` to `to`, whose control points are the two between, in the page's own coordinates (no turn), with its box the
/// control polygon's: a `NURBSTo` row whose control points are shares of the box, as the engine writes them.
pub(crate) fn cubic_shape(id: u32, from: Pt, c1: Pt, c2: Pt, to: Pt) -> diacraft_doc::Shape {
    let (x0, x1) = (from.0.min(c1.0).min(c2.0).min(to.0), from.0.max(c1.0).max(c2.0).max(to.0));
    let (y0, y1) = (from.1.min(c1.1).min(c2.1).min(to.1), from.1.max(c1.1).max(c2.1).max(to.1));
    let (w, h) = (x1 - x0, y1 - y0);
    let local = |p: Pt| (p.0 - x0, p.1 - y0);
    let share = |p: Pt| format!("{},{}", local(p).0 / w, local(p).1 / h);
    let (start, end) = (local(from), local(to));
    let mut curve = plain_row(2, "NURBSTo", end.0, end.1);
    for (name, n) in [("A", 0.0), ("B", 1.0), ("C", 0.0), ("D", 1.0)] {
        curve.cells.insert(CellName::new(name), Cell::constant(Value::from_text(&format!("{n}"))));
    }
    let constructor = format!("NURBS(1,3,0,0,{},0,1,{},0,1)", share(c1), share(c2));
    curve.cells.insert(CellName::new("E"), Cell::constant(Value::from_text(&constructor)).with_unit("NURBS"));
    let rows = vec![Arc::new(plain_row(1, "MoveTo", start.0, start.1)), Arc::new(curve)];
    Build::new(id, w, h).pin(x0 + w / 2.0, y0 + h / 2.0).raw_section(rows, true).build()
}

#[test]
fn curves_join_as_the_cubics_they_are_and_keep_their_shape() {
    let (a_from, a_c1, a_c2, a_to) = ((0.0, 0.0), (1.0, 2.0), (2.0, 2.0), (3.0, 0.0));
    let (b_from, b_c1, b_c2, b_to) = ((3.0, 0.0), (4.0, -2.0), (5.0, -2.0), (6.0, 0.0));
    let mut s = session_with(vec![cubic_shape(1, a_from, a_c1, a_c2, a_to), cubic_shape(2, b_from, b_c1, b_c2, b_to)]);
    let id = made(&run(&mut s, "shape.join", json!({"ids": [1, 2]})));
    let d = doc(&s);
    assert_eq!(section_count(d, id), 1);
    assert_eq!(row_types(d, id, 0), ["MoveTo", "NURBSTo", "NURBSTo"], "each curve is a clamped cubic of its own");
    // The joined curve passes through the originals' points: the middle of each cubic, and the join.
    let path = {
        let mut p = built(d, id)[0].1.clone();
        p.apply_affine(pose_map(d, id));
        p
    };
    let cubic = |from: Pt, c1: Pt, c2: Pt, to: Pt| kurbo::CubicBez::new(from, c1, c2, to);
    let (first, second) = (cubic(a_from, a_c1, a_c2, a_to), cubic(b_from, b_c1, b_c2, b_to));
    let on_path = |want: Point| path.segments().any(|seg| kurbo::ParamCurveNearest::nearest(&seg, want, 1e-9).distance_sq.sqrt() < 1.0e-4);
    for k in 0..=8 {
        let t = f64::from(k) / 8.0;
        assert!(on_path(first.eval(t)), "the first curve at {t}");
        assert!(on_path(second.eval(t)), "the second curve at {t}");
    }
    assert_near("Width", num(d, id, "Width"), 6.0);
    // Each curve rises 1.5 above or falls 1.5 below its ends (its control points are 2 off): the box holds them by their true extent, 3, and not by the polygon's 4.
    assert!((num(d, id, "Height") - 3.0).abs() < 1.0e-4, "{}", num(d, id, "Height"));
}

// ----: what Join needs --------------------------------------------------------------------------------------------------

#[test]
fn join_needs_two_shapes_with_a_path_each_and_says_so_in_words_and_changes_nothing() {
    let mut s = session_with(vec![
        line(1, (0.0, 0.0), (1.0, 0.0)).build(),
        line(2, (1.0, 0.0), (2.0, 0.0)).build(),
        Build::new(3, 1.0, 1.0).pin(5.0, 5.0).build(),
        line(4, (7.0, 7.0), (8.0, 8.0)).build(),
        line(5, (8.0, 8.0), (9.0, 9.0)).build(),
    ]);
    let group = id_of(&run(&mut s, "shape.group", json!({"ids": [4, 5]})));
    let before = snapshot(&s);
    for (params, why) in [
        (json!({"ids": [1]}), "two shapes at least"),
        (json!({"ids": [1, 1]}), "two shapes at least"),
        (json!({"ids": [1, 3]}), "no geometry to draw"),
        (json!({"ids": [group, 1]}), "is a group"),
        (json!({"ids": [4, 1], "page": 1}), "share one container"),
    ] {
        let error = error_of(&mut s, "shape.join", &params);
        assert!(error.to_string().contains(why), "{params}: {error}");
        assert!(error.to_string().contains("`shape.join`"), "the refusal names the command: {error}");
        assert_unchanged(&s, &before, "a refused Join");
    }
}

#[test]
fn a_shape_that_cannot_be_deleted_refuses_join() {
    let mut s = session_with(vec![
        line(1, (0.0, 0.0), (1.0, 0.0)).build(),
        line(2, (1.0, 0.0), (2.0, 0.0)).cell("LockDelete", Cell::constant(Value::from_text("1"))).build(),
    ]);
    let before = snapshot(&s);
    let error = error_of(&mut s, "shape.join", &json!({"ids": [1, 2]}));
    assert!(error.to_string().contains("cannot be deleted"), "{error}");
    assert_unchanged(&s, &before, "a refused Join");
}

#[test]
fn a_shape_on_a_locked_layer_refuses_join_and_trim() {
    let mut s = session_with(vec![
        line(1, (0.0, 0.0), (1.0, 0.0)).cell("LayerMember", Cell::constant(Value::from_text("0"))).build(),
        line(2, (1.0, 0.0), (2.0, 0.0)).build(),
    ]);
    s.edit("a locked layer", |doc| {
        let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
        let mut page_sheet = (*page.sheet).clone();
        page_sheet
            .set_cell(&diacraft_doc::CellAddr::in_row("Layer", None, diacraft_doc::RowKey::Ix(0), "Lock"), Cell::constant(Value::from_text("1")))
            .unwrap();
        page.sheet = Arc::new(page_sheet);
        Ok(())
    })
    .unwrap();
    let before = snapshot(&s);
    for (command, ids) in [("shape.join", json!([1, 2])), ("shape.trim", json!([1]))] {
        let error = error_of(&mut s, command, &json!({"ids": ids}));
        assert!(error.to_string().contains("locked layer"), "{command}: {error}");
        assert_unchanged(&s, &before, command);
    }
}

#[test]
fn check_says_what_join_and_trim_would_refuse_without_running_them() {
    let s = session_with(vec![rect(1, (0.0, 0.0, 1.0, 1.0)).build(), line(2, (0.0, 0.0), (4.0, 4.0)).build(), Build::new(3, 1.0, 1.0).build()]);
    let d = doc(&s);
    let before = d.clone();
    assert_eq!(check_shape_ops(d, 1, Op::Join, &[1, 2]), Ok(()));
    assert!(check_shape_ops(d, 1, Op::Join, &[1]).unwrap_err().contains("Join needs two shapes at least"));
    assert!(check_shape_ops(d, 1, Op::Join, &[1, 3]).unwrap_err().contains("no geometry to draw"));
    assert_eq!(check_shape_ops(d, 1, Op::Trim, &[1]), Ok(()), "Trim takes one shape");
    assert_eq!(check_shape_ops(d, 1, Op::Trim, &[1, 2]), Ok(()));
    assert!(check_shape_ops(d, 1, Op::Trim, &[3]).unwrap_err().contains("no geometry to draw"));
    assert!(check_shape_ops(d, 1, Op::Trim, &[]).unwrap_err().contains("Trim needs a shape"));
    assert_eq!((Op::Join.id(), Op::Trim.id()), ("shape.join", "shape.trim"));
    assert_eq!((Op::Join.fewest(), Op::Trim.fewest()), (2, 1));
    assert_eq!(d, &before, "nothing is changed");
}

// ----: glue and undo -----------------------------------------------------------------------------------------------

#[test]
fn a_connector_glued_to_an_original_keeps_its_end_unglued_and_undo_brings_everything_back_in_one_step() {
    let mut s = blank_session();
    // Two rectangles have a path each, so Join takes them; the connector is glued from D to a connection point of A.
    let (a, b, d_) = (instance(&mut s, (1.0, 1.0, 3.0, 3.0)), instance(&mut s, (3.0, 1.0, 5.0, 3.0)), instance(&mut s, (7.0, 1.0, 8.0, 2.0)));
    let connector = id_of(&run(&mut s, "connector.add", json!({"from": {"id": d_}, "to": {"id": a, "point": 0}})));
    let end_before = (num(doc(&s), connector, "EndX"), num(doc(&s), connector, "EndY"));
    let before = snapshot(&s);
    let connects_to = |d: &Document, target: u32| d.page(1).unwrap().connects.iter().filter(|c| c.to_sheet == target).count();
    assert_eq!(connects_to(doc(&s), a), 1);
    let produced = made(&run(&mut s, "shape.join", json!({"ids": [a, b]})));
    let after = doc(&s);
    assert_eq!(connects_to(after, a), 0, "no Connect row for the end that was glued to the original");
    assert_eq!(connects_to(after, produced), 0, "the connector is not glued to the produced shape");
    assert_eq!(connects_to(after, d_), 1, "its end at D is still glued");
    assert!(has_id(after, connector), "the connector is not deleted");
    assert_near("EndX", num(after, connector, "EndX"), end_before.0);
    assert_near("EndY", num(after, connector, "EndY"), end_before.1);
    assert!(
        after.shape(&at(after, produced)).unwrap().sheet.section(&diacraft_doc::SectionKey::new("Connection", None)).is_none(),
        "the produced shape has no connection points"
    );
    run(&mut s, "edit.undo", json!({}));
    let undone = doc(&s);
    assert_eq!(undone, before.document.as_ref().unwrap(), "undo brings the originals back as they were");
    assert_eq!(connects_to(undone, a), 1);
    assert!(has_id(undone, a) && has_id(undone, b) && !has_id(undone, produced));
    assert_eq!((snapshot(&s).undo_steps, snapshot(&s).redo_steps), (before.undo_steps, 1), "one step, now to redo");
    run(&mut s, "edit.redo", json!({}));
    assert!(has_id(doc(&s), produced) && !has_id(doc(&s), a));
}

#[test]
fn the_step_is_named_for_the_operation_and_a_refused_join_adds_none() {
    let mut s = session_with(vec![line(1, (0.0, 0.0), (1.0, 0.0)).build(), line(2, (1.0, 0.0), (2.0, 0.0)).build()]);
    let before = snapshot(&s);
    let _ = error_of(&mut s, "shape.join", &json!({"ids": [1]}));
    assert_eq!(snapshot(&s).undo_steps, before.undo_steps, "no step for a refusal");
    run(&mut s, "shape.join", json!({"ids": [1, 2]}));
    let after = snapshot(&s);
    assert_eq!(after.undo_steps, before.undo_steps + 1);
    assert_eq!(after.next_undo.as_deref(), Some("Join"));
}

// ----: inside a group ----------------------------------------------------------------------------------------------------

#[test]
fn shapes_picked_inside_a_group_give_a_result_that_is_the_groups_last_member() {
    let mut s =
        session_with(vec![line(1, (0.0, 0.0), (1.0, 0.0)).build(), line(2, (1.0, 0.0), (2.0, 0.0)).build(), rect(3, (5.0, 5.0, 6.0, 6.0)).build()]);
    let group = id_of(&run(&mut s, "shape.group", json!({"ids": [1, 2, 3]})));
    let c_before = doc(&s).shape(&at(doc(&s), 3)).unwrap().clone();
    let produced = made(&run(&mut s, "shape.join", json!({"ids": [1, 2]})));
    let d = doc(&s);
    let members: Vec<u32> = d.shape(&at(d, group)).unwrap().children.iter().map(|m| m.id).collect();
    assert_eq!(members, [3, produced], "C is unchanged and the produced shape is the group's last member");
    assert_eq!(d.shape(&at(d, 3)).unwrap(), &c_before);
    assert_eq!(d.shape(&at(d, group)).unwrap().kind, ShapeType::Group);
}

// ---- the file -------------------------------------------------------------------------------------------------------------------

#[test]
fn the_joined_shape_saves_to_vsdx_and_reopens_with_the_cells_equal() {
    let mut s = session_with(vec![
        line(1, (0.0, 0.0), (2.0, 0.0)).text("A").build(),
        line(2, (2.0, 0.0), (2.0, 2.0)).build(),
        line(3, (2.0, 2.0), (0.0, 0.0)).build(),
    ]);
    let produced = ids(&run(&mut s, "shape.join", json!({"ids": [1, 2, 3]})), "ids");
    let dir = Dir::new();
    let path = dir.path("out.vsdx");
    run(&mut s, "file.save_as", json!({"path": path}));
    let mut reopened = Session::new();
    run(&mut reopened, "file.open", json!({"path": path}));
    for id in produced {
        assert_eq!(cells_of(doc(&s), id), cells_of(doc(&reopened), id), "shape {id}");
    }
}
