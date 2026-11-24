//! The survivors of the review's `cargo mutants` run that an input kills (the review's risk 5 and 6): each test is the input, and says which line of the
//! adapter it watches.

use diacraft_doc::{Document, RowKey};
use diacraft_engine::snap::{Axis, Rect, SnapMark, Target};
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};
use serde_json::json;

use super::snap_review_fixes::{ask, page_of, session_of, shapes_round_a_box, vector_settings};
use crate::run;

/// A square drawn in a shape's frame (one `Geometry` section of five rows) from (`x0`, `y0`) to (`x1`, `y1`) on the page.
fn drawn_square(id: u32, x0: f64, y0: f64, x1: f64, y1: f64) -> ShapeBuilder {
    let (w, h) = (x1 - x0, y1 - y0);
    let mut shape = ShapeBuilder::new(id)
        .cell("PinX", &((x0 + x1) / 2.0).to_string())
        .cell("PinY", &((y0 + y1) / 2.0).to_string())
        .cell("Width", &w.to_string())
        .cell("Height", &h.to_string())
        .cell("LocPinX", &(w / 2.0).to_string())
        .cell("LocPinY", &(h / 2.0).to_string())
        .cell("Angle", "0");
    for (i, (x, y)) in [(0.0, 0.0), (w, 0.0), (w, h), (0.0, h), (0.0, 0.0)].iter().enumerate() {
        let row = RowKey::Ix(u32::try_from(i).unwrap());
        let kind = if i == 0 { "MoveTo" } else { "LineTo" };
        shape = shape.row_type("Geometry", Some(0), row.clone(), kind).row_cell("Geometry", Some(0), row.clone(), "X", &x.to_string()).row_cell(
            "Geometry",
            Some(0),
            row,
            "Y",
            &y.to_string(),
        );
    }
    shape
}

// ---------------------------------------------------------------------------------------------------------------- adapter.rs 547 and 549: the cut of either of two queries

/// 90 shapes 0.2 inch across and 0.3 apart (10 by 9) round the place (11.45, 5.4), and a big square drawn from (5.3, -2) to (15, 12): its left side is a line whose nearest point
/// to a box beside it is a place, and its corners are more than a pull from any place of a box at y 1 to 2. The crowd is out of the band of that box, which a row of shapes
/// (equal spacing) would gather whole, and its shapes are not so small that the library's grid of buckets is finer than a window across the row.
fn crowd_and_a_square() -> Document {
    let mut page = PageBuilder::new().cell("PageWidth", "20").cell("PageHeight", "20");
    for i in 0..90u32 {
        let (x, y) = (10.0 + f64::from(i % 10) * 0.3, 4.05 + f64::from(i / 10) * 0.3);
        page = page.shape(
            ShapeBuilder::new(i + 1)
                .cell("PinX", &(x + 0.1).to_string())
                .cell("PinY", &(y + 0.1).to_string())
                .cell("Width", "0.2")
                .cell("Height", "0.2")
                .cell("LocPinX", "0.1")
                .cell("LocPinY", "0.1")
                .cell("Angle", "0"),
        );
    }
    DocBuilder::new().page(1, page.shape(drawn_square(200, 5.3, -2.0, 15.0, 12.0))).build()
}

#[test]
fn a_crowd_at_an_own_point_cuts_the_query_that_asks_for_it_and_the_result_says_so_whether_or_not_the_other_snapped_to_a_place() {
    // The moving shape has a place of its own 6 inches from its box, in a crowd of 90 shapes: the query that is asked with the own place meets more shapes than it looks
    // into (64) and is cut, and the query that asks for the outlines alone (the box's own nine places) never meets the crowd and is not.
    let doc = crowd_and_a_square();
    let own = [(11.45, 5.4)];
    // The outline of the square is a place the box lands on in the second query (line 547: the result is that query's, and the first query's cut is told with it).
    let beside = Rect { x0: 4.0, y0: 1.0, x1: 5.0, y1: 2.0 };
    let out = ask(&doc, &vector_settings(), beside, &own, 6.0);
    assert!(out.hits.iter().any(|h| h.axis.is_none() && h.target == Target::Outline), "the box lands on the square's outline: {out:?}");
    assert!(out.truncated, "the crowd was cut in the first query: {out:?}");
    // Nothing snaps to a place in either (line 549: the first query's result, with the second's cut or not): the box is far from the square.
    let far = Rect { x0: 2.0, y0: 1.0, x1: 3.0, y1: 2.0 };
    let out = ask(&doc, &vector_settings(), far, &own, 6.0);
    assert!(out.hits.iter().all(|h| h.axis.is_some()), "no place: {out:?}");
    assert!(out.truncated, "{out:?}");
    // With no place of its own the crowd is not met by either query, and nothing is cut.
    let out = ask(&doc, &vector_settings(), beside, &[], 6.0);
    assert!(!out.truncated, "{out:?}");
}

// ---------------------------------------------------------------------------------------------------------------- adapter.rs 927: a line mark on each axis at one offset

#[test]
fn two_lines_hit_at_the_same_offset_on_the_two_axes_each_run_over_their_own_shapes() {
    // A is a 1 by 1 box at (1, 5), with its left side on x = 1, and B one at (8, 1), with its bottom on y = 1. The box dragged from (1.03, 1.03) lands its left side on x = 1
    // and its bottom on y = 1: two lines at the offset 1, one along each axis. The line on x = 1 runs along y from the box to A (1 to 6), the line on y = 1 along x from the
    // box to B (1 to 9): a mark found by its offset alone gave one line the span of the other.
    let doc = page_of(&[(1, 1.0, 5.0, 2.0, 6.0), (2, 8.0, 1.0, 9.0, 2.0)]);
    let r = Rect { x0: 1.03, y0: 1.03, x1: 2.03, y1: 2.03 };
    let out = ask(&doc, &vector_settings(), r, &[], 96.0);
    let hit = |axis: Axis| out.hits.iter().find(|h| h.target == Target::Alignment && h.axis == Some(axis)).copied();
    let (x, y) = (hit(Axis::X).expect("the left sides line up"), hit(Axis::Y).expect("the bottoms line up"));
    assert_eq!((x.shape, y.shape), (Some(1), Some(2)), "{out:?}");
    assert!((x.line - 1.0).abs() < 1e-9 && (y.line - 1.0).abs() < 1e-9, "both at the offset 1: {out:?}");
    let (xs, ys) = (x.span.unwrap(), y.span.unwrap());
    assert!((xs.0 - 1.0).abs() < 1e-9 && (xs.1 - 6.0).abs() < 1e-9, "{xs:?}");
    assert!((ys.0 - 1.0).abs() < 1e-9 && (ys.1 - 9.0).abs() < 1e-9, "{ys:?}");
}

// ---------------------------------------------------------------------------------------------------------------- adapter.rs 946: equal spacing on both axes

#[test]
fn equal_spacing_on_both_axes_gives_each_hit_the_gap_of_its_own_axis() {
    // L and R in the box's row with gaps 2.03 and 1.97 (the box is 0.03 off the middle), Bo and T in its column the same: the box lands in the middle on both axes, with a
    // spacing hit on each. The gap each hit says is 2 inches long, along its own axis: a hit that took the gap mark of the other axis said a gap of nothing.
    let doc = page_of(&[(1, 0.0, 2.0, 1.0, 5.5), (2, 6.0, 2.0, 7.0, 5.5), (3, 2.0, 0.0, 5.5, 1.0), (4, 2.0, 6.0, 5.5, 7.0)]);
    let r = Rect { x0: 3.03, y0: 3.03, x1: 4.03, y1: 4.03 };
    let out = ask(&doc, &vector_settings(), r, &[], 96.0);
    let spacing = |axis: Axis| out.hits.iter().find(|h| h.target == Target::Spacing && h.axis == Some(axis)).copied();
    for axis in [Axis::X, Axis::Y] {
        let h = spacing(axis).unwrap_or_else(|| panic!("a spacing hit on {axis:?}: {out:?}"));
        let (a, b) = h.span.unwrap();
        assert!(((b - a) - 2.0).abs() < 1e-9 && (h.gap.unwrap() - 2.0).abs() < 1e-9, "{axis:?}: {h:?}");
        // The span is along the hit's own axis: for the x hit, between x = 1 and 3 or 4 and 6, and for the y hit the same on y.
        assert!(((a - 1.0).abs() < 1e-9 && (b - 3.0).abs() < 1e-9) || ((a - 4.0).abs() < 1e-9 && (b - 6.0).abs() < 1e-9), "{axis:?}: {h:?}");
    }
    assert!((out.dx + 0.03).abs() < 1e-9 && (out.dy + 0.03).abs() < 1e-9, "{out:?}");
}

// ---------------------------------------------------------------------------------------------------------------- adapter.rs marks_of: a line's kinds are told once each

#[test]
fn a_line_that_is_an_edge_of_one_shape_and_the_centre_of_another_is_told_as_alignment_once() {
    // A zero-width box (a vertical line of a shape) has its left, middle and right on one place, so a line it lies on with the centre of A (x 5 to 7) and the left edge of B
    // (x 6 to 8) is both: the mark's kinds are `Centre` and `Edge`, which are one word, `alignment`.
    let doc = page_of(&[(1, 5.0, 2.0, 7.0, 3.0), (2, 6.0, 5.0, 8.0, 6.0)]);
    let r = Rect { x0: 6.03, y0: 8.0, x1: 6.03, y1: 9.0 };
    let out = ask(&doc, &vector_settings(), r, &[], 96.0);
    let line = out
        .marks
        .iter()
        .find_map(|m| match m {
            SnapMark::Line { axis: Axis::X, at, targets, hit: true, .. } if (at - 6.0).abs() < 1e-9 => Some(targets.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("a mark on x = 6: {out:?}"));
    assert_eq!(line, vec![Target::Alignment], "{out:?}");
}

// ---------------------------------------------------------------------------------------------------------------- snap_params.rs: a view is four numbers and nothing else

#[test]
fn a_view_of_four_numbers_and_something_else_is_refused_by_every_command_that_takes_one() {
    let mut s = session_of(page_of(&[(1, 1.0, 1.0, 3.0, 2.0)]));
    for view in
        [json!([0.0, 0.0, 10.0, 10.0, "x"]), json!([0.0, 0.0, 10.0, 10.0, 5]), json!([0.0, 0.0, 10.0, "x", 10.0]), json!(["a", 0.0, 10.0, 10.0])]
    {
        for (command, base) in [
            ("shape.resize", json!({"id": 1, "x": 1.0, "y": 1.0, "width": 2.0, "height": 1.0})),
            ("shape.move", json!({"ids": [1], "dx": 1.0, "dy": 0.0})),
            ("shape.place", json!({"ids": [1], "x": 5.0})),
            ("shape.drop", json!({"master": "rectangle", "page": 1, "x": 9.0, "y": 9.0})),
        ] {
            let mut p = base;
            p["snap"] = json!({"view": view.clone()});
            let err = s.execute(command, &p).unwrap_err().to_string();
            assert!(err.contains("snap.view"), "{command} {view}: {err}");
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------- adapter.rs 549: the cut of the outlines asked for alone

#[test]
fn a_cut_of_the_outlines_in_the_second_query_alone_is_said_when_nothing_snapped_to_a_place() {
    // The moving shape has a place of its own, so the library is asked twice. Vertices, crossings and connection points are off: the first query reads no outline, and the second
    // (the outlines alone) reads those of the shapes near the box, 16 at most for a call. 17 plain shapes round the box: the cut is the second query's, and nothing snaps to a place.
    let mut s = vector_settings();
    s.snap_vertices = false;
    s.intersections = false;
    s.snap_connection_points = false;
    let own = [(8.5, 8.5)];
    for (n, cut) in [(16u32, false), (17, true)] {
        let (doc, r) = shapes_round_a_box(n);
        let out = ask(&doc, &s, r, &own, 5.5);
        assert!(out.hits.iter().all(|h| h.axis.is_some()), "{n} shapes: nothing snaps to a place: {out:?}");
        assert_eq!(out.truncated, cut, "{n} shapes: {out:?}");
    }
}

// ---------------------------------------------------------------------------------------------------------------- adapter.rs one_side_of_each_axis: what an axis at rest still marks

#[test]
fn a_resize_that_moves_both_sides_of_an_axis_still_marks_the_line_the_other_axis_lies_on() {
    // B (6 to 7.5 across, 2 to 3 up) beside A (3.4 to 5.4, 2 to 3): B's left and right sides both move (5.9 to 7.7) and nothing snaps, but its bottom lies on A's and that line
    // is marked, as a line the box lies on is for any other call (the hits say what was pulled to, the marks what holds where the box lands).
    let mut s = session_of(page_of(&[(1, 3.4, 2.0, 5.4, 3.0), (2, 6.0, 2.0, 7.5, 3.0)]));
    let r = run(
        &mut s,
        "shape.resize",
        json!({"id": 2, "x": 5.9, "y": 2.0, "width": 1.8, "height": 1.0, "snap": {"zoom": 96.0, "view": [0.0, 0.0, 10.0, 10.0]}}),
    );
    assert_eq!(r["snap"]["hits"], json!([]), "{r}");
    let marks = r["snap"]["marks"].as_array().unwrap();
    assert!(marks.iter().any(|m| m["mark"] == json!("line") && m["axis"] == json!("y") && m["at"] == json!(2.0) && m["hit"] == json!(false)), "{r}");
}

#[test]
fn a_resize_that_moves_both_sides_of_an_axis_snaps_one_and_marks_the_lines_the_other_axis_lies_on() {
    // B (1 to 3 across, 5 to 6 up) stands over A (1 to 3, 2 to 3), their left sides on x = 1. B's bottom and top both move (4.9 to 6.1), the top is 0.02
    // inch short of a level guide and lands on it: a hit on y, and nothing on x, where the box lies on A's left side and that line is marked.
    let mut doc = page_of(&[(1, 1.0, 2.0, 3.0, 3.0), (2, 1.0, 5.0, 3.0, 6.0)]);
    diacraft_engine::snap::guides::add_guide(&mut doc, 1, diacraft_engine::snap::Axis::Y, 6.12, 0).unwrap();
    let mut s = session_of(doc);
    let r = run(
        &mut s,
        "shape.resize",
        json!({"id": 2, "x": 1.0, "y": 4.9, "width": 2.0, "height": 1.2, "snap": {"zoom": 96.0, "view": [0.0, 0.0, 10.0, 10.0]}}),
    );
    assert_eq!(r["snap"]["hits"][0]["target"], json!("guide"), "{r}");
    let marks = r["snap"]["marks"].as_array().unwrap();
    assert!(marks.iter().any(|m| m["mark"] == json!("line") && m["axis"] == json!("x") && m["at"] == json!(1.0) && m["hit"] == json!(false)), "{r}");
    assert!(marks.iter().any(|m| m["mark"] == json!("line") && m["axis"] == json!("y") && m["at"] == json!(6.12) && m["hit"] == json!(true)), "{r}");
}
