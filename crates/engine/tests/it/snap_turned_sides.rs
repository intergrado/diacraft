//! Row 3 of the design's change table: a turned shape lines up its own sides with the like sides of shapes turned the same way and with slanted guides, and a turned
//! or flipped shape's resize maps its handles to its sides through its corners (spec). The settings are the spec's "Settings for vectors".

use diacraft_doc::{RowKey, ShapeType};
use diacraft_engine::draw::{shape_corners, shape_frame};
use diacraft_engine::snap::guides::add_guide;
use diacraft_engine::snap::{Axis, Extras, OwnFrame, Query, Rect, SnapPage, Subject, Use, bounds_of, frame_of_shapes, snap_ext};
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};
use serde_json::{Value, json};

use super::snap_library_settings::{at, box_in, close, corners, guide_at, marks_of, middle, near2, page_of_shapes, rect, session_of, turned};
use super::snap_review_fixes::vector_settings;
use crate::run;
use diacraft_engine::Session;

#[test]
fn a_turned_shape_lines_up_its_own_sides_with_another_turned_the_same_way() {
    // A is 2 by 1 inches turned 30 degrees, its pin at (3, 3). B, the same size and turn, is dragged to where its pin is 4 inches along A's own x axis and 0.03 inch along its
    // own y axis off A's own lines. B's own top, middle and bottom lines come to A's: the pin lands 4 inches along A's x axis (6.464102, 5.000000) and three marks run along 30
    // degrees. The boxes round them are far apart on every page line, so no page line competes. Naive (boxes round them only): no hit, the pin stays where it was dragged.
    let a = (3.0, 3.0);
    let raw = at(a, 30.0, 4.0, 0.03);
    let mut s = session_of(page_of_shapes(vec![turned(1, a, 2.0, 1.0, 30.0), turned(2, (10.0, 3.0), 2.0, 1.0, 30.0)]));
    let r = run(&mut s, "shape.move", json!({"ids": [2], "dx": raw.0 - 10.0, "dy": raw.1 - 3.0, "snap": {"zoom": 96.0}}));
    let want = at(a, 30.0, 4.0, 0.0);
    assert!(near2(middle(&s, 2), want, 1e-9), "{:?} against {want:?}: {r}", middle(&s, 2));
    // Every line B lies on is marked: its own top, middle and bottom, each along 30 degrees, one of them the line that pulled.
    let marks: Vec<&Value> = marks_of(&r, "line");
    assert_eq!(marks.len(), 3, "{r}");
    assert_eq!(marks.iter().filter(|m| m["hit"] == json!(true)).count(), 1, "{r}");
    for m in marks {
        assert_eq!(m["axis"], Value::Null, "a line along 30 degrees is along no page axis: {m}");
        assert!(close(m["normal"].as_f64().unwrap(), 120.0, 1e-6), "the normal of a line along 30 degrees is at 120: {m}");
        assert_eq!(m["shape"], json!(1), "{m}");
        // The line runs from one shape to the other along 30 degrees.
        let (a, b) = (m["from"].clone(), m["to"].clone());
        let run = (b[0].as_f64().unwrap() - a[0].as_f64().unwrap(), b[1].as_f64().unwrap() - a[1].as_f64().unwrap());
        assert!(close(run.1.atan2(run.0).to_degrees().rem_euclid(180.0), 30.0, 1e-3), "{m}");
    }
    let hits = r["snap"]["hits"].as_array().unwrap();
    assert!(!hits.is_empty() && hits.iter().all(|h| h["target"] == json!("alignment") && h.get("axis").is_none() && h["normal"].is_number()), "{r}");
}

#[test]
fn a_turned_shape_far_from_the_others_own_lines_snaps_to_nothing_by_them() {
    // The same shapes with B 0.2 inch off A's own lines (past the 6-point reach): nothing snaps.
    let a = (3.0, 3.0);
    let raw = at(a, 30.0, 4.0, 0.2);
    let mut s = session_of(page_of_shapes(vec![turned(1, a, 2.0, 1.0, 30.0), turned(2, (10.0, 3.0), 2.0, 1.0, 30.0)]));
    let r = run(&mut s, "shape.move", json!({"ids": [2], "dx": raw.0 - 10.0, "dy": raw.1 - 3.0, "snap": {"zoom": 96.0}}));
    assert!(near2(middle(&s, 2), raw, 1e-9), "{r}");
    assert_eq!(r["snap"]["hits"], json!([]), "{r}");
}

#[test]
fn a_shape_turned_by_another_angle_offers_no_own_lines_to_a_turned_shape() {
    // Two shapes both turned but by angles that are not a quarter turn apart give each other no own lines; only the boxes round them line up. B is turned 40 degrees and A 30;
    // dragged to where its own lines would be 0.03 off A's if it were turned the same, nothing snaps by own lines.
    let a = (3.0, 3.0);
    let raw = at(a, 30.0, 4.0, 0.03);
    let mut s = session_of(page_of_shapes(vec![turned(1, a, 2.0, 1.0, 30.0), turned(2, (10.0, 3.0), 2.0, 1.0, 40.0)]));
    let r = run(&mut s, "shape.move", json!({"ids": [2], "dx": raw.0 - 10.0, "dy": raw.1 - 3.0, "snap": {"zoom": 96.0}}));
    let hits = r["snap"]["hits"].as_array().unwrap();
    assert!(hits.iter().all(|h| h["normal"].is_null()), "no own line is met: {r}");
}

#[test]
fn a_shape_turned_a_quarter_turn_more_lines_up_its_own_lines_with_a_turned_shape() {
    // "the same way, or a quarter, half or three-quarter turn from it". B is turned 120 degrees (30 plus a quarter turn): its own x axis lies along A's y axis. Dragged 4
    // inches along A's own y axis... B's own lines are A's own lines swapped; the same landing as for two shapes turned alike, 0.03 inch off the lines of A's own x axis.
    let a = (3.0, 3.0);
    // B's pin 4 inches along A's own x and 0.03 along its own y: B (turned 120) has its own x axis along A's y, and its own y axis along A's -x.
    let raw = at(a, 30.0, 4.0, 0.03);
    let mut s = session_of(page_of_shapes(vec![turned(1, a, 2.0, 1.0, 30.0), turned(2, (10.0, 3.0), 2.0, 1.0, 120.0)]));
    let r = run(&mut s, "shape.move", json!({"ids": [2], "dx": raw.0 - 10.0, "dy": raw.1 - 3.0, "snap": {"zoom": 96.0}}));
    let own: Vec<&Value> = r["snap"]["hits"].as_array().unwrap().iter().filter(|h| h["normal"].is_number()).collect();
    assert!(!own.is_empty(), "B's own lines meet A's: {r}");
}

#[test]
fn a_locked_move_of_a_turned_shape_keeps_to_a_page_axis_and_lines_up_its_own_sides_along_the_free_degree() {
    // A is 2 by 1 turned 30 degrees at (5, 5), B the same at 4 inches along A's x axis and 0.02 along its y axis off A's own lines. A Shift-drag of B by (0.05, 0.01) is sent
    // with `lock: "x"` (x is the larger): B goes along x only, dy exactly 0, and its own lines come to A's at dx 0.04 (the pin ends at (8.494102, 7.017321)).
    let a = (5.0, 5.0);
    let start = at(a, 30.0, 4.0, 0.02);
    let mut s = session_of(page_of_shapes(vec![turned(1, a, 2.0, 1.0, 30.0), turned(2, start, 2.0, 1.0, 30.0)]));
    let r = run(&mut s, "shape.move", json!({"ids": [2], "dx": 0.05, "dy": 0.01, "snap": {"zoom": 96.0, "lock": "x"}}));
    let now = middle(&s, 2);
    assert!(now.1 == start.1, "the locked axis did not move, to the bit: {now:?} from {start:?}: {r}");
    assert!(close(now.0, start.0 + 0.04, 1e-9), "{now:?}: {r}");
    let own: Vec<&Value> = marks_of(&r, "line").into_iter().filter(|m| m["normal"].is_number()).collect();
    assert_eq!((own.len(), own.iter().filter(|m| m["hit"] == json!(true)).count()), (3, 1), "{r}");
}

#[test]
fn a_locked_move_of_a_turned_shape_with_no_target_goes_as_dragged_along_the_axis() {
    let a = (5.0, 5.0);
    let mut s = session_of(page_of_shapes(vec![turned(2, a, 2.0, 1.0, 30.0)]));
    let r = run(&mut s, "shape.move", json!({"ids": [2], "dx": 2.0, "dy": 0.6, "snap": {"zoom": 96.0, "lock": "x"}}));
    let now = middle(&s, 2);
    assert!(now.1 == a.1 && close(now.0, 7.0, 1e-9), "{now:?}: {r}");
}

// ---------------------------------------------------------------------------------------------------------------- row 3, slanted guides

#[test]
fn a_shape_turned_like_a_slanted_guide_lines_up_a_side_with_it_and_is_held_to_nothing() {
    // A guide through (0, 0) along 30 degrees and a 2 by 1 inch shape turned 30 dragged so that its own bottom side is 0.03 inch above the guide. The side comes onto the guide:
    // the pin moves 0.03 along the shape's own y axis, to (4.080127, 2.933013), and no hold is written (only a shape that is not turned is held). Naive (today: a tilted guide is
    // left out of snapping altogether): it stays where it was dragged.
    let start = at((0.0, 0.0), 30.0, 5.0, 0.53);
    let mut s = session_of(page_of_shapes(vec![guide_at(1, (0.0, 0.0), 30.0), turned(2, (9.0, 9.0), 2.0, 1.0, 30.0)]));
    let r = run(&mut s, "shape.move", json!({"ids": [2], "dx": start.0 - 9.0, "dy": start.1 - 9.0, "snap": {"zoom": 96.0}}));
    let want = at((0.0, 0.0), 30.0, 5.0, 0.5);
    assert!(near2(middle(&s, 2), want, 1e-9), "{:?} against {want:?}: {r}", middle(&s, 2));
    let hit = &r["snap"]["hits"][0];
    assert_eq!((hit["target"].clone(), hit["shape"].clone()), (json!("guide"), json!(1)), "{r}");
    assert!(close(hit["normal"].as_f64().unwrap().to_degrees(), 120.0, 1e-6) || close(hit["normal"].as_f64().unwrap(), 120.0, 1e-6), "{r}");
    assert_eq!(r["snap"]["held"], json!([]), "a snap to a slanted guide writes no hold: {r}");
}

#[test]
fn a_shape_that_is_not_turned_does_not_snap_to_a_slanted_guide() {
    // A 1 by 1 shape that is not turned has no side along the guide: nothing snaps.
    let start = at((0.0, 0.0), 30.0, 5.0, 0.01);
    let mut s = session_of(page_of_shapes(vec![guide_at(1, (0.0, 0.0), 30.0), rect(2, start.0 - 0.5, start.1 - 0.5, start.0 + 0.5, start.1 + 0.5)]));
    let r = run(&mut s, "shape.move", json!({"ids": [2], "dx": 0.0, "dy": 0.0, "snap": {"zoom": 96.0}}));
    assert_eq!(r["snap"]["hits"], json!([]), "{r}");
}

#[test]
fn a_slanted_guide_is_no_target_for_a_shape_turned_another_way() {
    // The guide along 30 degrees and a shape turned 50: none of its sides is parallel to it.
    let start = at((0.0, 0.0), 30.0, 5.0, 0.53);
    let mut s = session_of(page_of_shapes(vec![guide_at(1, (0.0, 0.0), 30.0), turned(2, start, 2.0, 1.0, 50.0)]));
    let r = run(&mut s, "shape.move", json!({"ids": [2], "dx": 0.0, "dy": 0.0, "snap": {"zoom": 96.0}}));
    assert!(r["snap"]["hits"].as_array().unwrap().iter().all(|h| h["target"] != json!("guide")), "{r}");
}

#[test]
fn a_place_is_no_target_of_a_slanted_guide_the_end_of_a_connector_included() {
    // A slanted guide is a target of a turned shape whose own side is parallel to it, and of no other subject: a place put down, or the end of a connector (which glues to
    // guides), 0.01 inch off the guide is not pulled to it.
    let near = at((0.0, 0.0), 30.0, 5.0, 0.01);
    let mut s = session_of(page_of_shapes(vec![guide_at(1, (0.0, 0.0), 30.0)]));
    for connect in [false, true] {
        let r = run(&mut s, "snap.at", json!({"page": 1, "x": near.0, "y": near.1, "connect": connect}));
        assert_eq!(r["hits"], json!([]), "connect {connect}: {r}");
        assert!(close(r["x"].as_f64().unwrap(), near.0, 1e-12) && close(r["y"].as_f64().unwrap(), near.1, 1e-12), "{r}");
    }
}

// ---------------------------------------------------------------------------------------------------------------- row 3, a turned or flipped shape's resize

/// The page position of the corner at the shape's own (0, 0), for `shape.resize`.
fn origin_corner(s: &Session, id: u32) -> (f64, f64) {
    corners(s, id)[0]
}

#[test]
fn a_turned_shapes_resize_moves_its_side_along_its_own_axis_and_a_corner_meets_a_page_guide() {
    // A 2 by 1 inch shape turned 30 degrees, its pin in its middle at (3, 3), an upright guide at x = 4.58, and its right side handle dragged to a width of 2.5 with the left side
    // fixed: the lower right corner of the width-height box comes to the guide, the width is 2.535752 (the guide's distance along the shape's own axis), the height 1, the angle
    // 30 degrees to the bit, the left side's middle where it was, and the pin at (3.231987, 3.133938). Naive (the page distance added to the width): 2.530962. Today: no snap, 2.5.
    let mut doc = page_of_shapes(vec![turned(1, (3.0, 3.0), 2.0, 1.0, 30.0)]);
    add_guide(&mut doc, 1, Axis::X, 4.58, 0).unwrap();
    let mut s = session_of(doc);
    let c0 = origin_corner(&s, 1);
    let left_middle = at(c0, 30.0, 0.0, 0.5);
    let r = run(&mut s, "shape.resize", json!({"id": 1, "x": c0.0, "y": c0.1, "width": 2.5, "height": 1.0, "snap": {"zoom": 96.0}}));
    let c = corners(&s, 1);
    let width = ((c[1].0 - c[0].0).powi(2) + (c[1].1 - c[0].1).powi(2)).sqrt();
    let height = ((c[3].0 - c[0].0).powi(2) + (c[3].1 - c[0].1).powi(2)).sqrt();
    assert!(close(width, 2.535752, 1e-6) && close(height, 1.0, 1e-9), "{width} by {height}: {r}");
    assert!(close(c[1].0, 4.58, 1e-9), "the corner is on the guide: {c:?}");
    let now_left_middle = ((c[0].0 + c[3].0) / 2.0, (c[0].1 + c[3].1) / 2.0);
    assert!(near2(now_left_middle, left_middle, 1e-9), "the left side did not move: {now_left_middle:?} against {left_middle:?}");
    assert!(near2(middle(&s, 1), (3.231987, 3.133938), 2e-6), "{:?}", middle(&s, 1));
    // The angle is the shape's, to the bit.
    assert_eq!(shape_frame(s.document().unwrap(), 1, 1).unwrap().angle, 30.0_f64.to_radians());
    assert_eq!(r["snap"]["hits"][0]["target"], json!("guide"), "{r}");
    assert_eq!(r["snap"]["held"], json!([]), "a turned shape is held to nothing: {r}");
}

#[test]
fn a_resize_of_a_turned_shape_with_nothing_in_reach_goes_as_dragged() {
    let mut s = session_of(page_of_shapes(vec![turned(1, (3.0, 3.0), 2.0, 1.0, 30.0)]));
    let c0 = origin_corner(&s, 1);
    let r = run(&mut s, "shape.resize", json!({"id": 1, "x": c0.0, "y": c0.1, "width": 2.5, "height": 1.0, "snap": {"zoom": 96.0}}));
    let c = corners(&s, 1);
    let width = ((c[1].0 - c[0].0).powi(2) + (c[1].1 - c[0].1).powi(2)).sqrt();
    assert!(close(width, 2.5, 1e-9), "{width}: {r}");
    assert_eq!(r["snap"]["hits"], json!([]), "{r}");
}

#[test]
fn a_resize_of_a_turned_shape_meets_the_own_side_of_another_shape_turned_alike() {
    // Shape 1 (2 by 1, turned 30, pin (3, 3)) widened by its right side, its left side fixed 1 inch behind its pin along its own x axis. Shape 2, the same size and turn, has its
    // pin 2 inches along that axis and 2 inches along the other, so its own right side is 3 inches along from 1's pin: shape 1's right side meets it (like with like) when the
    // width is 4. Dragged to 3.97.
    let a = (3.0, 3.0);
    let other = at(a, 30.0, 2.0, 2.0);
    let mut s = session_of(page_of_shapes(vec![turned(1, a, 2.0, 1.0, 30.0), turned(2, other, 2.0, 1.0, 30.0)]));
    let c0 = origin_corner(&s, 1);
    let r = run(&mut s, "shape.resize", json!({"id": 1, "x": c0.0, "y": c0.1, "width": 3.97, "height": 1.0, "snap": {"zoom": 96.0}}));
    let c = corners(&s, 1);
    let width = ((c[1].0 - c[0].0).powi(2) + (c[1].1 - c[0].1).powi(2)).sqrt();
    assert!(close(width, 4.0, 1e-9), "{width}: {r}");
}

#[test]
fn a_flipped_shapes_handle_maps_to_the_side_it_is_on_through_the_corners() {
    // A 2 by 1 shape flipped left to right (FlipX), its pin at (3, 3): its box is (2, 2.5) to (4, 3.5), and its own x axis runs to the left from the corner at its lower right
    // (4, 2.5). `shape.resize` takes that corner and a width along the own axis: 2.55 puts the left edge at 1.45. A guide at x = 1.4 is 0.05 inch from it: the side comes onto the
    // guide and the width is 2.6. Today (a flipped shape is not snapped): 2.55.
    let mut doc = page_of_shapes(vec![turned(1, (3.0, 3.0), 2.0, 1.0, 0.0).cell("FlipX", "1")]);
    add_guide(&mut doc, 1, Axis::X, 1.4, 0).unwrap();
    let mut s = session_of(doc);
    let c0 = origin_corner(&s, 1);
    assert!(near2(c0, (4.0, 2.5), 1e-9), "the corner at the shape's own (0, 0) is its lower right: {c0:?}");
    let r = run(&mut s, "shape.resize", json!({"id": 1, "x": c0.0, "y": c0.1, "width": 2.55, "height": 1.0, "snap": {"zoom": 96.0}}));
    let b = box_in(&s, 1);
    assert!(close(b.x0, 1.4, 1e-9) && close(b.x1, 4.0, 1e-9), "{b:?}: {r}");
    assert_eq!(r["snap"]["hits"][0]["target"], json!("guide"), "{r}");
}

#[test]
fn a_shape_flipped_top_to_bottom_snaps_the_side_the_handle_is_on() {
    // FlipY: the own y axis runs down from the upper left corner (2, 3.5). A height of 1.05 puts the bottom edge at 2.45; a level guide at y = 2.4 is 0.05 below it.
    let mut doc = page_of_shapes(vec![turned(1, (3.0, 3.0), 2.0, 1.0, 0.0).cell("FlipY", "1")]);
    add_guide(&mut doc, 1, Axis::Y, 2.4, 0).unwrap();
    let mut s = session_of(doc);
    let c0 = origin_corner(&s, 1);
    assert!(near2(c0, (2.0, 3.5), 1e-9), "{c0:?}");
    let r = run(&mut s, "shape.resize", json!({"id": 1, "x": c0.0, "y": c0.1, "width": 2.0, "height": 1.1 - 0.05, "snap": {"zoom": 96.0}}));
    let b = box_in(&s, 1);
    assert!(close(b.y0, 2.4, 1e-9) && close(b.y1, 3.5, 1e-9), "{b:?}: {r}");
}

#[test]
fn a_resize_that_keeps_the_proportions_of_a_turned_shape_keeps_its_own_proportions() {
    // Kept proportions of a turned shape are its own width over its height (2 to 1), whatever its box on the page is.
    let mut doc = page_of_shapes(vec![turned(1, (3.0, 3.0), 2.0, 1.0, 30.0)]);
    add_guide(&mut doc, 1, Axis::X, 4.58, 0).unwrap();
    let mut s = session_of(doc);
    let c0 = origin_corner(&s, 1);
    // The far corner dragged to a width of 2.5 and a height of 1, off the shape's diagonal: the box the resize comes to keeps the shape's own ratio of 2 to 1 (2.5 by 1.25), and
    // the guide at x = 4.58 is met inside it.
    let r = run(
        &mut s,
        "shape.resize",
        json!({"id": 1, "x": c0.0, "y": c0.1, "width": 2.5, "height": 1.0, "snap": {"zoom": 96.0, "keep": "proportions"}}),
    );
    let c = corners(&s, 1);
    let width = ((c[1].0 - c[0].0).powi(2) + (c[1].1 - c[0].1).powi(2)).sqrt();
    let height = ((c[3].0 - c[0].0).powi(2) + (c[3].1 - c[0].1).powi(2)).sqrt();
    assert!(close(width / height, 2.0, 1e-9), "{width} by {height}: {r}");
}

// ---------------------------------------------------------------------------------------------------------------- the own frame of a moving shape

#[test]
fn the_own_frame_of_one_turned_shape_is_its_box_and_angle_and_of_anything_else_none() {
    let s = session_of(page_of_shapes(vec![
        turned(1, (3.0, 3.0), 2.0, 1.0, 30.0),
        turned(2, (9.0, 3.0), 2.0, 1.0, 0.0),
        turned(3, (9.0, 9.0), 2.0, 1.0, 90.0),
        turned(4, (5.0, 9.0), 2.0, 1.0, 45.0),
    ]));
    let doc = s.document().unwrap();
    let f = diacraft_engine::snap::frame_of_shapes(doc, 1, &[1]).expect("a frame");
    assert!(
        near2(f.centre, (3.0, 3.0), 1e-12)
            && close(f.width, 2.0, 1e-12)
            && close(f.height, 1.0, 1e-12)
            && close(f.angle, 30.0_f64.to_radians(), 1e-12),
        "{f:?}"
    );
    // Not turned, turned by a quarter turn (its own sides are the page's), two shapes (the box of a selection is square to the page), no shape.
    assert!(diacraft_engine::snap::frame_of_shapes(doc, 1, &[2]).is_none());
    assert!(diacraft_engine::snap::frame_of_shapes(doc, 1, &[3]).is_none());
    assert!(diacraft_engine::snap::frame_of_shapes(doc, 1, &[1, 4]).is_none());
    assert!(diacraft_engine::snap::frame_of_shapes(doc, 1, &[9]).is_none());
    assert!(diacraft_engine::snap::frame_of_shapes(doc, 1, &[]).is_none());
    // The same shape named twice is one shape.
    assert!(diacraft_engine::snap::frame_of_shapes(doc, 1, &[4, 4]).is_some());
    // Its box round its paths is the box `bounds_of` gives.
    assert!(bounds_of(doc, 1, &[1]).is_some());
}

// ---------------------------------------------------------------------------------------------------------------- shape.place and snap.at line a turned shape up as shape.move does

#[test]
fn shape_place_and_snap_at_line_up_the_own_sides_of_one_turned_shape_as_shape_move_does() {
    // V5 three ways: B (2 by 1 turned 30 degrees) is put where its pin is 0.03 inch off A's own lines (`shape.place`), is asked where a box at its place would land (`snap.at`
    // with the shape named), and is moved there (`shape.move`): the same landing, 0.03 inch along its own y axis, each time.
    let a = (3.0, 3.0);
    let raw = at(a, 30.0, 4.0, 0.03);
    let want = at(a, 30.0, 4.0, 0.0);
    let page = || page_of_shapes(vec![turned(1, a, 2.0, 1.0, 30.0), turned(2, (10.0, 3.0), 2.0, 1.0, 30.0)]);
    let mut placed = session_of(page());
    let r = run(&mut placed, "shape.place", json!({"ids": [2], "x": raw.0, "y": raw.1, "snap": {"zoom": 96.0}}));
    assert!(near2(middle(&placed, 2), want, 1e-9), "place: {:?} against {want:?}: {r}", middle(&placed, 2));
    assert!(r["snap"]["hits"].as_array().unwrap().iter().any(|h| h["normal"].is_number()), "{r}");
    let mut moved = session_of(page());
    let m = run(&mut moved, "shape.move", json!({"ids": [2], "dx": raw.0 - 10.0, "dy": raw.1 - 3.0, "snap": {"zoom": 96.0}}));
    assert!(near2(middle(&moved, 2), want, 1e-9), "{m}");
    // `snap.at` with a box where the shape would be dragged to: the box round its paths (here its corners), moved as the drag moves it.
    let mut asked = session_of(page());
    let b = bounds_of(asked.document().unwrap(), 1, &[2]).unwrap().rect;
    let (dx, dy) = (raw.0 - 10.0, raw.1 - 3.0);
    let q = run(
        &mut asked,
        "snap.at",
        json!({"page": 1, "left": b.x0 + dx, "bottom": b.y0 + dy, "right": b.x1 + dx, "top": b.y1 + dy, "ids": [2], "zoom": 96.0}),
    );
    let landed = (raw.0 + q["dx"].as_f64().unwrap(), raw.1 + q["dy"].as_f64().unwrap());
    assert!(near2(landed, want, 1e-6), "snap.at: {landed:?} against {want:?}: {q}");
    assert_eq!(q["hits"], m["snap"]["hits"], "the same hits as the command tells");
    assert_eq!(q["marks"], m["snap"]["marks"], "the same marks as the command tells");
}

// ---------------------------------------------------------------------------------------------------------------- the same on a page of any scale

/// A shape for [`scaled_session`]: its id, pin, width, height and degrees.
type Placed = (u32, (f64, f64), f64, f64, f64);

/// A page `k` drawing units to a paper inch, with the shapes `(id, pin, width, height, degrees)` given in paper inches (the cells hold drawing units).
fn scaled_session(k: f64, shapes: &[Placed]) -> Session {
    let mut page = PageBuilder::new().cell("PageWidth", "20").cell("PageHeight", "20").cell("PageScale", "1").cell("DrawingScale", &k.to_string());
    for (id, pin, w, h, degrees) in shapes {
        page = page.shape(turned(*id, (pin.0 * k, pin.1 * k), w * k, h * k, *degrees));
    }
    session_of(DocBuilder::new().page(1, page).build())
}

#[test]
fn a_turned_shape_snaps_by_its_own_sides_and_its_own_axes_on_a_page_of_any_scale() {
    // Commands take and give paper inches; the cells hold drawing units. V5 by `shape.place` and `shape.move`, and V6 by `shape.resize`, on pages of 1, 2 and 0.5 drawing units to
    // an inch land where they land at 1.
    for k in [1.0, 2.0, 0.5] {
        let a = (3.0, 3.0);
        let raw = at(a, 30.0, 4.0, 0.03);
        let want = at(a, 30.0, 4.0, 0.0);
        let shapes = [(1, a, 2.0, 1.0, 30.0), (2, (10.0, 3.0), 2.0, 1.0, 30.0)];
        let paper = |s: &Session, id: u32| {
            let m = middle(s, id);
            (m.0 / k, m.1 / k)
        };
        let mut placed = scaled_session(k, &shapes);
        let r = run(&mut placed, "shape.place", json!({"ids": [2], "x": raw.0, "y": raw.1, "snap": {"zoom": 96.0}}));
        assert!(near2(paper(&placed, 2), want, 1e-9), "{k} units to an inch, placed: {:?} against {want:?}: {r}", paper(&placed, 2));
        let mut moved = scaled_session(k, &shapes);
        let r = run(&mut moved, "shape.move", json!({"ids": [2], "dx": raw.0 - 10.0, "dy": raw.1 - 3.0, "snap": {"zoom": 96.0}}));
        assert!(near2(paper(&moved, 2), want, 1e-9), "{k} units to an inch, moved: {:?} against {want:?}: {r}", paper(&moved, 2));
        // The guide at x = 4.58 inches, the right side dragged to a width of 2.5 inches.
        let mut sized = scaled_session(k, &shapes[..1]);
        run(&mut sized, "guide.add", json!({"page": 1, "orientation": "vertical", "at": 4.58}));
        let c0 = corners(&sized, 1)[0];
        let r = run(&mut sized, "shape.resize", json!({"id": 1, "x": c0.0 / k, "y": c0.1 / k, "width": 2.5, "height": 1.0, "snap": {"zoom": 96.0}}));
        let c = corners(&sized, 1);
        assert!(close(c[1].0 / k, 4.58, 1e-9), "{k} units to an inch, resized: {:?}: {r}", c[1]);
        let width = (c[1].0 - c[0].0).hypot(c[1].1 - c[0].1) / k;
        assert!(close(width, 2.535752, 1e-6), "{k}: {width}: {r}");
    }
}

// ---------------------------------------------------------------------------------------------------------------- what the hits of a turned shape tell

#[test]
fn a_hit_on_an_own_line_tells_the_line_it_is_on_by_its_normal_and_the_offset_of_the_side_that_met_it() {
    // V5, then the shape the other way up and a guide: the hit's `line` is the offset along the normal (120 degrees for lines along 30) of the side of the subject that met the line,
    // once moved: the middle of its own frame for a middle, half its own height either side for a low or a high side (the height is along its own y axis, the width along its x).
    let a = (3.0, 3.0);
    let raw = at(a, 30.0, 4.0, 0.03);
    let want = at(a, 30.0, 4.0, 0.0);
    let n = (120.0_f64.to_radians().cos(), 120.0_f64.to_radians().sin());
    let centre = n.0 * want.0 + n.1 * want.1;
    let mut s = session_of(page_of_shapes(vec![turned(1, a, 2.0, 1.0, 30.0), turned(2, (10.0, 3.0), 2.0, 1.0, 30.0)]));
    let r = run(&mut s, "shape.move", json!({"ids": [2], "dx": raw.0 - 10.0, "dy": raw.1 - 3.0, "snap": {"zoom": 96.0}}));
    let hit = r["snap"]["hits"][0].clone();
    let expected = match hit["anchor"].as_str().unwrap() {
        "low" => centre - 0.5,
        "high" => centre + 0.5,
        _ => centre,
    };
    assert!(close(hit["normal"].as_f64().unwrap(), 120.0, 1e-6) && close(hit["line"].as_f64().unwrap(), expected, 2e-6), "{expected}: {r}");
    // A guide through the origin along 30 degrees (its line is n . p = 0), and the shape's own y side that is high 0.03 inch below it, then low 0.03 inch above it.
    for (offset, anchor) in [(-0.53, "high"), (0.53, "low")] {
        let start = at((0.0, 0.0), 30.0, 5.0, offset);
        let mut s = session_of(page_of_shapes(vec![guide_at(1, (0.0, 0.0), 30.0), turned(2, (9.0, 9.0), 2.0, 1.0, 30.0)]));
        let r = run(&mut s, "shape.move", json!({"ids": [2], "dx": start.0 - 9.0, "dy": start.1 - 9.0, "snap": {"zoom": 96.0}}));
        let hit = &r["snap"]["hits"][0];
        assert_eq!(hit["anchor"], json!(anchor), "{r}");
        assert!(close(hit["line"].as_f64().unwrap(), 0.0, 2e-6), "the guide's line is at 0: {r}");
        assert!(close(middle_n(&s, 2), offset.signum() * 0.5, 1e-9), "{:?}", middle_n(&s, 2));
    }
}

/// The offset of the middle of shape `id` along the normal of lines along 30 degrees.
fn middle_n(s: &Session, id: u32) -> f64 {
    let m = middle(s, id);
    (120.0_f64.to_radians().cos()) * m.0 + (120.0_f64.to_radians().sin()) * m.1
}

#[test]
fn a_turned_shape_near_two_page_guides_farther_off_together_than_the_larger_reach_snaps_to_one_and_every_mark_holds() {
    // A turned subject is asked once: the library takes the combination that snaps the most degrees within the larger reach of its candidates, and two page lines 0.06 inch off on
    // each axis are 0.085 apart in all, which is past a guide's pull of 8 points (0.0833 inch). One is taken, and what is drawn is where the shape lands; a box that is square to the
    // page has each axis taken apart (the follow-up of its own), which a move of a turned shape would spoil, since a move along an axis takes its own lines off theirs.
    let mut doc = page_of_shapes(vec![turned(1, (5.0, 5.0), 2.0, 1.0, 30.0)]);
    let b = shape_bounds(&doc, 1);
    add_guide(&mut doc, 1, Axis::X, b.2 + 0.06, 0).unwrap();
    add_guide(&mut doc, 1, Axis::Y, b.3 + 0.06, 0).unwrap();
    let mut s = session_of(doc);
    // The guides' pull is 8 points here (the premise of the numbers above; the spec's default is 10).
    run(&mut s, "snap.set", json!({"strength_guides": 8}));
    let r = run(&mut s, "shape.move", json!({"ids": [1], "dx": 0.0, "dy": 0.0, "snap": {"zoom": 96.0}}));
    assert_eq!(r["snap"]["hits"].as_array().map(Vec::len), Some(1), "{r}");
    for m in marks_of(&r, "line").into_iter().filter(|m| m["hit"] == json!(true)) {
        assert!(mark_holds(&s, 1, m), "{m}");
    }
}

/// The box round a shape's corners as (x0, y0, x1, y1), before the shape moves.
fn shape_bounds(doc: &diacraft_doc::Document, id: u32) -> (f64, f64, f64, f64) {
    let c = shape_corners(doc, 1, id).unwrap();
    let xs = c.iter().map(|p| p.0);
    let ys = c.iter().map(|p| p.1);
    (
        xs.clone().fold(f64::INFINITY, f64::min),
        ys.clone().fold(f64::INFINITY, f64::min),
        xs.fold(f64::NEG_INFINITY, f64::max),
        ys.fold(f64::NEG_INFINITY, f64::max),
    )
}

#[test]
fn a_turned_resize_that_moves_both_sides_of_an_axis_snaps_each_to_its_own_line() {
    // The shape turned 30 degrees (2 by 1, pin (3, 3)) with its low own x side moved 0.2 and its high one 0.6 along its own x axis, and two upright guides: one 0.03 inch from where
    // the middle of the low side comes to, the other 0.03 inch from where the high side's lower corner comes to. Every side that moves snaps (the library solves the frame whole),
    // so both guides are hit, the low side and the high, where the old rule for a box square to the page keeps one of the two.
    let a = (3.0, 3.0);
    let (u, v) = ((30.0_f64.to_radians().cos(), 30.0_f64.to_radians().sin()), (-0.5, 30.0_f64.to_radians().cos()));
    let c0 = at(a, 30.0, -1.0, -0.5);
    let c0_new = (c0.0 + 0.2 * u.0, c0.1 + 0.2 * u.1);
    let width = 2.0 - 0.2 + 0.6;
    // Where the low side's middle and the high side's lower corner are, once dragged.
    let low_mid = (c0_new.0 + 0.5 * v.0, c0_new.1 + 0.5 * v.1);
    let high_corner = (c0_new.0 + width * u.0, c0_new.1 + width * u.1);
    let mut doc = page_of_shapes(vec![turned(1, a, 2.0, 1.0, 30.0)]);
    add_guide(&mut doc, 1, Axis::X, low_mid.0 + 0.03, 0).unwrap();
    add_guide(&mut doc, 1, Axis::X, high_corner.0 - 0.03, 0).unwrap();
    let mut s = session_of(doc);
    let r = run(&mut s, "shape.resize", json!({"id": 1, "x": c0_new.0, "y": c0_new.1, "width": width, "height": 1.0, "snap": {"zoom": 96.0}}));
    let hits = r["snap"]["hits"].as_array().unwrap();
    let anchors: Vec<&str> = hits.iter().map(|h| h["anchor"].as_str().unwrap()).collect();
    assert!(hits.len() == 2 && anchors.contains(&"low") && anchors.contains(&"high"), "{anchors:?}: {r}");
}

#[test]
fn a_turned_resize_tells_the_side_it_pulled_and_the_line_it_met_and_a_size_match_tells_width_or_height() {
    // V6 with a rectangle whose left edge is on the guide too (a guide wins the kind, and the line is the one the mark names): the hit is the high side of x, on the line 4.58. Then
    // the width of the shape matched to a shape 3 inches wide, and its height to a shape 1.5 inches high, each told with its axis, the side that moved and the length.
    let mut doc = page_of_shapes(vec![turned(1, (3.0, 3.0), 2.0, 1.0, 30.0), rect(2, 4.58, 12.0, 6.0, 13.0)]);
    add_guide(&mut doc, 1, Axis::X, 4.58, 0).unwrap();
    let mut s = session_of(doc);
    let c0 = corners(&s, 1)[0];
    let r = run(&mut s, "shape.resize", json!({"id": 1, "x": c0.0, "y": c0.1, "width": 2.5, "height": 1.0, "snap": {"zoom": 96.0}}));
    let hit = &r["snap"]["hits"][0];
    assert_eq!((hit["target"].clone(), hit["axis"].clone(), hit["anchor"].clone()), (json!("guide"), json!("x"), json!("high")), "{r}");
    assert!(close(hit["line"].as_f64().unwrap(), 4.58, 1e-9), "{r}");
    // A size match: shape 2 is 3 inches wide and 0.4 high, shape 3 1.4 inches wide and 1.5 high, far from the shape and from each other.
    let mut s =
        session_of(page_of_shapes(vec![turned(1, (3.0, 3.0), 2.0, 1.0, 30.0), rect(2, 10.0, 16.0, 13.0, 16.4), rect(3, 15.0, 3.0, 16.4, 4.5)]));
    let c0 = corners(&s, 1)[0];
    let r = run(&mut s, "shape.resize", json!({"id": 1, "x": c0.0, "y": c0.1, "width": 3.03, "height": 1.0, "snap": {"zoom": 96.0}}));
    let size = r["snap"]["hits"].as_array().unwrap().iter().find(|h| h["target"] == json!("size")).unwrap_or_else(|| panic!("{r}")).clone();
    assert_eq!((size["axis"].clone(), size["anchor"].clone(), size["shape"].clone()), (json!("x"), json!("high"), json!(2)), "{r}");
    assert!(close(size["gap"].as_f64().unwrap(), 3.0, 1e-9), "{r}");
    let c0 = corners(&s, 1)[0];
    let r = run(&mut s, "shape.resize", json!({"id": 1, "x": c0.0, "y": c0.1, "width": 3.0, "height": 1.53, "snap": {"zoom": 96.0}}));
    let size = r["snap"]["hits"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["target"] == json!("size") && h["axis"] == json!("y"))
        .unwrap_or_else(|| panic!("{r}"))
        .clone();
    assert_eq!((size["anchor"].clone(), size["shape"].clone()), (json!("high"), json!(3)), "{r}");
    assert!(close(size["gap"].as_f64().unwrap(), 1.5, 1e-9), "{r}");
}

#[test]
fn a_turned_resize_that_moves_one_side_leaves_the_other_where_it_is_and_snaps_only_the_one() {
    // The low own x side dragged 0.3 along the shape's own x axis (the corner at its own (0, 0) moves with it and the width shrinks by as much), a guide 0.03 inch from where the
    // middle of that side comes to, and another 0.02 inch from the high side's lower corner, which stays: the side that moves snaps, the one that stays is not pulled to the
    // guide that is near it, and its corners are where they were.
    let a = (3.0, 3.0);
    let u = (30.0_f64.to_radians().cos(), 30.0_f64.to_radians().sin());
    let c0 = at(a, 30.0, -1.0, -0.5);
    let c0_new = (c0.0 + 0.3 * u.0, c0.1 + 0.3 * u.1);
    let low_mid = (c0_new.0 - 0.25, c0_new.1 + 0.5 * 30.0_f64.to_radians().cos());
    let c1 = at(a, 30.0, 1.0, -0.5);
    let mut doc = page_of_shapes(vec![turned(1, a, 2.0, 1.0, 30.0)]);
    add_guide(&mut doc, 1, Axis::X, low_mid.0 + 0.03, 0).unwrap();
    add_guide(&mut doc, 1, Axis::X, c1.0 - 0.02, 0).unwrap();
    let mut s = session_of(doc);
    let before = corners(&s, 1);
    let r = run(&mut s, "shape.resize", json!({"id": 1, "x": c0_new.0, "y": c0_new.1, "width": 1.7, "height": 1.0, "snap": {"zoom": 96.0}}));
    let after = corners(&s, 1);
    assert!(near2(after[1], before[1], 1e-9) && near2(after[2], before[2], 1e-9), "the high side did not move: {before:?} to {after:?}: {r}");
    let anchors: Vec<&str> = r["snap"]["hits"].as_array().unwrap().iter().map(|h| h["anchor"].as_str().unwrap()).collect();
    assert_eq!(anchors, vec!["low"], "{r}");
}

#[test]
fn a_member_of_a_turned_group_is_not_snapped_in_its_own_axes_and_a_turned_or_flipped_shape_is() {
    // `snaps_in_its_own_axes` is the question `shape.resize` and the window's preview ask: a shape that is turned or flipped and in no group that is. The member of a turned group
    // is refused by the resize in any case, so its snap is the old one's (a box square to the page).
    use diacraft_engine::snap::snaps_in_its_own_axes;
    let s = session_of(page_of_shapes(vec![
        turned(1, (3.0, 3.0), 2.0, 1.0, 30.0),
        turned(2, (9.0, 3.0), 2.0, 1.0, 0.0).cell("FlipX", "1"),
        turned(3, (3.0, 9.0), 2.0, 1.0, 0.0),
        group_at(4, (12.0, 12.0), 30.0),
        // A group turned 30 degrees whose member is itself turned 20: the member's own corners are a rectangle on the page, but it is in a group that is turned.
        group_at_with(6, (16.0, 5.0), 30.0, 20.0),
    ]));
    let doc = s.document().unwrap();
    assert_eq!([1, 2, 3, 4, 5, 6, 7].map(|id| snaps_in_its_own_axes(doc, 1, id)), [true, true, false, true, false, true, false]);
}

#[test]
fn the_result_of_a_turned_resize_tells_the_box_it_comes_to_in_the_shapes_own_axes() {
    // The box the resize comes to is `snap.frame`: its middle (3.231987, 3.133938), its width 2.535752, its height 1 and its angle 30 degrees; a resize that snaps nothing tells the
    // box it was given, and a move tells none.
    let mut doc = page_of_shapes(vec![turned(1, (3.0, 3.0), 2.0, 1.0, 30.0)]);
    add_guide(&mut doc, 1, Axis::X, 4.58, 0).unwrap();
    let mut s = session_of(doc);
    let c0 = corners(&s, 1)[0];
    let r = run(&mut s, "shape.resize", json!({"id": 1, "x": c0.0, "y": c0.1, "width": 2.5, "height": 1.0, "snap": {"zoom": 96.0}}));
    let f = &r["snap"]["frame"];
    let centre = (f["centre"][0].as_f64().unwrap(), f["centre"][1].as_f64().unwrap());
    assert!(near2(centre, (3.231987, 3.133938), 2e-6), "{r}");
    assert!(close(f["width"].as_f64().unwrap(), 2.535752, 1e-6) && close(f["height"].as_f64().unwrap(), 1.0, 1e-9), "{r}");
    assert!(close(f["angle"].as_f64().unwrap(), 30.0, 1e-6), "{r}");
    let m = run(&mut s, "shape.move", json!({"ids": [1], "dx": 0.5, "dy": 0.0, "snap": {"zoom": 96.0}}));
    assert_eq!(m["snap"]["frame"], Value::Null, "{m}");
}

#[test]
fn a_shape_turned_a_quarter_or_a_half_turn_is_snapped_in_its_own_axes_too() {
    // A shape turned 90 degrees has its own x axis along the page's y, one turned 180 along the page's -x (neither was snapped by a resize at all before): its width grows along its
    // own axis from the corner at its own (0, 0). Turned 90, a level guide 0.03 inch past where the width of 2.5 puts its high side (4.5 up) gives a width of 2.53; turned 180, an
    // upright guide 0.03 past where it puts its left side (1.5) does.
    for (degrees, guide) in [(90.0, (Axis::Y, 4.53)), (180.0, (Axis::X, 1.47))] {
        let mut doc = page_of_shapes(vec![turned(1, (3.0, 3.0), 2.0, 1.0, degrees)]);
        add_guide(&mut doc, 1, guide.0, guide.1, 0).unwrap();
        let mut s = session_of(doc);
        let c0 = corners(&s, 1)[0];
        let r = run(&mut s, "shape.resize", json!({"id": 1, "x": c0.0, "y": c0.1, "width": 2.5, "height": 1.0, "snap": {"zoom": 96.0}}));
        let c = corners(&s, 1);
        let width = (c[1].0 - c[0].0).hypot(c[1].1 - c[0].1);
        assert!(close(width, 2.53, 1e-9), "turned {degrees}: {width}: {r}");
        assert_eq!(r["snap"]["hits"][0]["anchor"], json!("high"), "{r}");
        assert!(near2(c[0], c0, 1e-9), "the corner at its own (0, 0) stayed: {c0:?} {:?}", c[0]);
    }
}

#[test]
fn the_same_turned_command_twice_gives_the_same_result_and_undo_restores_the_shape_to_the_bit() {
    // A second command with the same input gives the same answer, and one undo step puts the shape back as it was, for a move that snaps by own sides, a resize in own axes and a
    // press of an arrow key.
    let a = (3.0, 3.0);
    let raw = at(a, 30.0, 4.0, 0.03);
    let page = || {
        let mut doc = page_of_shapes(vec![turned(1, a, 2.0, 1.0, 30.0), turned(2, (10.0, 3.0), 2.0, 1.0, 30.0)]);
        add_guide(&mut doc, 1, Axis::X, 4.58, 0).unwrap();
        doc
    };
    let c0 = shape_corners(&page(), 1, 1).unwrap()[0];
    let commands = [
        ("shape.move", json!({"ids": [2], "dx": raw.0 - 10.0, "dy": raw.1 - 3.0, "snap": {"zoom": 96.0}})),
        ("shape.resize", json!({"id": 1, "x": c0.0, "y": c0.1, "width": 2.5, "height": 1.0, "snap": {"zoom": 96.0}})),
        ("shape.move", json!({"ids": [2], "dx": 1.0, "dy": 0.0, "snap": {"zoom": 96.0, "nudge": {"tick": 0.125}}})),
    ];
    for (command, params) in commands {
        let mut first = session_of(page());
        let before: Vec<_> = [1, 2].iter().map(|id| shape_corners(first.document().unwrap(), 1, *id).unwrap()).collect();
        let one = run(&mut first, command, params.clone());
        let mut second = session_of(page());
        assert_eq!(run(&mut second, command, params.clone()), one, "{command}: the same command on the same page");
        run(&mut first, "edit.undo", json!({}));
        let after: Vec<_> = [1, 2].iter().map(|id| shape_corners(first.document().unwrap(), 1, *id).unwrap()).collect();
        assert_eq!(before, after, "{command}: one undo restores the shapes to the bit");
    }
}

// ---------------------------------------------------------------------------------------------------------------- the spans of two hits at once, the sides that move

/// The span of a hit on an own line, as the marks say it: the end points of the mark of the hit's line (`normal` in degrees, JSON) taken along the line.
fn along_the_line(r: &Value, hit: &Value) -> (f64, f64) {
    let normal = hit["normal"].as_f64().unwrap().to_radians();
    let t = (-normal.sin(), normal.cos());
    let line = hit["line"].as_f64().unwrap();
    let marks = marks_of(r, "line");
    let m = marks
        .iter()
        .filter(|m| m["hit"] == json!(true) && !m["normal"].is_null() && close(m["normal"].as_f64().unwrap(), hit["normal"].as_f64().unwrap(), 1e-6))
        .min_by(|a, b| {
            let off = |m: &Value| (m["at"].as_f64().unwrap() - line).abs();
            off(a).total_cmp(&off(b))
        })
        .unwrap_or_else(|| panic!("no mark: {r}"));
    let (from, to) = ((m["from"][0].as_f64().unwrap(), m["from"][1].as_f64().unwrap()), (m["to"][0].as_f64().unwrap(), m["to"][1].as_f64().unwrap()));
    let (a, b) = (t.0 * from.0 + t.1 * from.1, t.0 * to.0 + t.1 * to.1);
    (a.min(b), a.max(b))
}

#[test]
fn the_span_of_a_hit_on_an_own_line_runs_along_the_line_from_the_subject_to_the_shape_it_met() {
    // Shape 1 is 2 inches long along 30 degrees at its own x of 0 to the subject's 4 (its sides at -1 to 1 and 3 to 5): the span of the hit is 6 inches along the line, and the
    // ends are the ends of the line's mark, taken along it.
    let a = (3.0, 3.0);
    let raw = at(a, 30.0, 4.0, 0.03);
    let mut s = session_of(page_of_shapes(vec![turned(1, a, 2.0, 1.0, 30.0), turned(2, (10.0, 3.0), 2.0, 1.0, 30.0)]));
    let r = run(&mut s, "shape.move", json!({"ids": [2], "dx": raw.0 - 10.0, "dy": raw.1 - 3.0, "snap": {"zoom": 96.0}}));
    let hit = &r["snap"]["hits"][0];
    let span = (hit["span"][0].as_f64().unwrap(), hit["span"][1].as_f64().unwrap());
    let want = along_the_line(&r, hit);
    assert!(close(span.0, want.0, 1e-5) && close(span.1, want.1, 1e-5), "{span:?} against {want:?}: {r}");
    assert!(close(span.1 - span.0, 6.0, 1e-5), "{span:?}: {r}");
}

#[test]
fn a_subject_that_meets_lines_in_both_its_own_directions_tells_each_hit_the_span_of_its_own_line() {
    // Shapes turned 30 degrees: P1 (2 by 0.4) 6 inches to the left of the subject along its own x with its own bottom side where the subject's will be, and P2 (1 by 1) 5 inches up
    // along its own y with its left side where the subject's left side will be. The subject, dragged 0.03 inch off in both its own directions, meets both lines at once: the hit on
    // each tells the span of its own line (the marks' ends taken along it) and not the span of the other.
    let o = (3.0, 3.0);
    let (p1, p2) = (at(o, 30.0, -6.0, -0.3), at(o, 30.0, -0.5, 5.0));
    let raw = at(o, 30.0, 0.03, 0.03);
    let mut s =
        session_of(page_of_shapes(vec![turned(1, p1, 2.0, 0.4, 30.0), turned(2, p2, 1.0, 1.0, 30.0), turned(3, (14.0, 14.0), 2.0, 1.0, 30.0)]));
    let r = run(&mut s, "shape.move", json!({"ids": [3], "dx": raw.0 - 14.0, "dy": raw.1 - 14.0, "snap": {"zoom": 96.0}}));
    let hits: Vec<&Value> =
        r["snap"]["hits"].as_array().unwrap().iter().filter(|h| h["target"] == json!("alignment") && !h["span"].is_null()).collect();
    let mut normals: Vec<f64> = hits.iter().map(|h| h["normal"].as_f64().unwrap()).collect();
    normals.sort_by(f64::total_cmp);
    assert!(normals.len() == 2 && close(normals[0], 30.0, 1e-6) && close(normals[1], 120.0, 1e-6), "{normals:?}: {r}");
    let mut found = Vec::new();
    for hit in &hits {
        let span = (hit["span"][0].as_f64().unwrap(), hit["span"][1].as_f64().unwrap());
        let want = along_the_line(&r, hit);
        assert!(close(span.0, want.0, 1e-5) && close(span.1, want.1, 1e-5), "{span:?} against {want:?}: {r}");
        found.push(span);
    }
    assert!(!(close(found[0].0, found[1].0, 1e-3) && close(found[0].1, found[1].1, 1e-3)), "the two lines run to different shapes: {found:?}: {r}");
}

#[test]
fn two_hits_in_two_own_directions_on_lines_of_the_same_offset_each_tell_the_span_of_their_own_line() {
    // As the test before, but placed so that the line the bottom side meets and the line the right side meets have the very same offset (about 8.1; the two directions have their
    // own normals, and the offsets are measured along each): the span of a hit is that of the mark of its own direction, not of the first mark with the offset.
    let (n30, n120) = ((30.0_f64.to_radians().cos(), 30.0_f64.to_radians().sin()), (120.0_f64.to_radians().cos(), 120.0_f64.to_radians().sin()));
    let cx = 1.0;
    let cy = (1.5 - (n120.0 - n30.0) * cx) / (n120.1 - n30.1);
    let c = (cx, cy);
    let (p1, p2) = (at(c, 30.0, 6.0, -0.3), at(c, 30.0, 0.5, -5.0));
    let raw = at(c, 30.0, 0.03, 0.03);
    let mut s =
        session_of(page_of_shapes(vec![turned(1, p1, 2.0, 0.4, 30.0), turned(2, p2, 1.0, 1.0, 30.0), turned(3, (14.0, 14.0), 2.0, 1.0, 30.0)]));
    let r = run(&mut s, "shape.move", json!({"ids": [3], "dx": raw.0 - 14.0, "dy": raw.1 - 14.0, "snap": {"zoom": 96.0}}));
    let hits: Vec<&Value> =
        r["snap"]["hits"].as_array().unwrap().iter().filter(|h| h["target"] == json!("alignment") && !h["span"].is_null()).collect();
    assert_eq!(hits.len(), 2, "{r}");
    assert!(close(hits[0]["line"].as_f64().unwrap(), hits[1]["line"].as_f64().unwrap(), 1e-5), "the two lines have one offset: {r}");
    let mut found = Vec::new();
    for hit in &hits {
        let span = (hit["span"][0].as_f64().unwrap(), hit["span"][1].as_f64().unwrap());
        let want = along_the_line(&r, hit);
        assert!(close(span.0, want.0, 1e-5) && close(span.1, want.1, 1e-5), "{span:?} against {want:?}: {r}");
        found.push(span);
    }
    assert!(!(close(found[0].0, found[1].0, 1e-3) && close(found[0].1, found[1].1, 1e-3)), "{found:?}: {r}");
}

#[test]
fn a_subject_with_an_equal_gap_in_each_of_its_own_directions_is_told_one_spacing_hit_with_the_gap_of_its_own_direction() {
    // Shapes 1 by 1 turned 30 degrees. A row along the subject's own x: two shapes 1 inch apart ending 1 inch short of where the subject will be, 0.4 above the middle line. A column
    // along its own y: two shapes 2 inches apart, 0.4 to the side. The subject, dragged 0.03 off in both own directions, is the next of both. The library tells one spacing hit for
    // the move (the nearer gap, the row's, along 30 degrees), with its gap of 1 inch and the marks of that gap only: no gap of the column's direction is marked as a hit.
    let o = (3.0, 3.0);
    let row = [at(o, 30.0, -4.0, 0.4), at(o, 30.0, -2.0, 0.4)];
    let column = [at(o, 30.0, 0.4, -6.0), at(o, 30.0, 0.4, -3.0)];
    let raw = at(o, 30.0, 0.03, 0.03);
    let mut s = session_of(page_of_shapes(vec![
        turned(1, row[0], 1.0, 1.0, 30.0),
        turned(2, row[1], 1.0, 1.0, 30.0),
        turned(3, column[0], 1.0, 1.0, 30.0),
        turned(4, column[1], 1.0, 1.0, 30.0),
        turned(5, (14.0, 14.0), 1.0, 1.0, 30.0),
    ]));
    let r = run(&mut s, "shape.move", json!({"ids": [5], "dx": raw.0 - 14.0, "dy": raw.1 - 14.0, "snap": {"zoom": 96.0}}));
    let spacing: Vec<&Value> = r["snap"]["hits"].as_array().unwrap().iter().filter(|h| h["target"] == json!("spacing")).collect();
    assert_eq!(spacing.len(), 1, "{r}");
    assert!(close(spacing[0]["normal"].as_f64().unwrap(), 30.0, 1e-6) && close(spacing[0]["gap"].as_f64().unwrap(), 1.0, 1e-6), "{r}");
    for gap in marks_of(&r, "gap") {
        assert!(
            gap["hit"] != json!(true) || (close(gap["normal"].as_f64().unwrap(), 30.0, 1e-6) && close(gap["value"].as_f64().unwrap(), 1.0, 1e-6)),
            "{r}"
        );
    }
}

#[test]
fn a_size_match_on_a_turned_resize_names_the_side_that_moved_low_or_high() {
    // The low own side dragged outward until the width is 3.03 (a shape 3 inches wide is far off), and the low own y side until the height is 1.53 (one 1.5 high): each size hit is
    // told with the side that moved, the low one.
    let mut s =
        session_of(page_of_shapes(vec![turned(1, (3.0, 3.0), 2.0, 1.0, 30.0), rect(2, 10.0, 16.0, 13.0, 16.4), rect(3, 15.0, 3.0, 16.4, 4.5)]));
    let c = corners(&s, 1);
    let u = ((c[1].0 - c[0].0) / 2.0, (c[1].1 - c[0].1) / 2.0);
    let r = run(
        &mut s,
        "shape.resize",
        json!({"id": 1, "x": c[0].0 - 1.03 * u.0, "y": c[0].1 - 1.03 * u.1, "width": 3.03, "height": 1.0, "snap": {"zoom": 96.0}}),
    );
    let size = r["snap"]["hits"].as_array().unwrap().iter().find(|h| h["target"] == json!("size")).unwrap_or_else(|| panic!("{r}")).clone();
    assert_eq!((size["axis"].clone(), size["anchor"].clone(), size["shape"].clone()), (json!("x"), json!("low"), json!(2)), "{r}");
    let c = corners(&s, 1);
    let v = (c[3].0 - c[0].0, c[3].1 - c[0].1);
    let r = run(
        &mut s,
        "shape.resize",
        json!({"id": 1, "x": c[0].0 - 0.53 * v.0, "y": c[0].1 - 0.53 * v.1, "width": 3.0, "height": 1.53, "snap": {"zoom": 96.0}}),
    );
    let size = r["snap"]["hits"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["target"] == json!("size") && h["axis"] == json!("y"))
        .unwrap_or_else(|| panic!("{r}"))
        .clone();
    assert_eq!((size["anchor"].clone(), size["shape"].clone()), (json!("low"), json!(3)), "{r}");
}

#[test]
fn a_turned_resize_that_moves_both_sides_tells_each_hit_the_line_of_its_own_guide() {
    // Two upright guides, the low own side's middle 0.03 inch from the one and the high side's corner 0.03 from the other: each hit names its own guide's place as its line (the low
    // hit the one on the low side, the high hit the one on the high side), and the guide's shape.
    let a = (3.0, 3.0);
    let (u, v) = ((30.0_f64.to_radians().cos(), 30.0_f64.to_radians().sin()), (-0.5, 30.0_f64.to_radians().cos()));
    let c0 = at(a, 30.0, -1.0, -0.5);
    let c0_new = (c0.0 + 0.2 * u.0, c0.1 + 0.2 * u.1);
    let width = 2.0 - 0.2 + 0.6;
    let low_mid = (c0_new.0 + 0.5 * v.0, c0_new.1 + 0.5 * v.1);
    let high_corner = (c0_new.0 + width * u.0, c0_new.1 + width * u.1);
    let (g_low, g_high) = (low_mid.0 + 0.03, high_corner.0 - 0.03);
    let mut doc = page_of_shapes(vec![turned(1, a, 2.0, 1.0, 30.0)]);
    let low_id = add_guide(&mut doc, 1, Axis::X, g_low, 0).unwrap().id;
    let high_id = add_guide(&mut doc, 1, Axis::X, g_high, 0).unwrap().id;
    let mut s = session_of(doc);
    let r = run(&mut s, "shape.resize", json!({"id": 1, "x": c0_new.0, "y": c0_new.1, "width": width, "height": 1.0, "snap": {"zoom": 96.0}}));
    let hits = r["snap"]["hits"].as_array().unwrap();
    for hit in hits {
        let (line, shape) = match hit["anchor"].as_str().unwrap() {
            "low" => (g_low, low_id),
            _ => (g_high, high_id),
        };
        assert!(close(hit["line"].as_f64().unwrap(), line, 1e-6) && hit["shape"] == json!(shape), "{hit}: {r}");
    }
    assert_eq!(hits.len(), 2, "{r}");
}

#[test]
fn a_turned_resize_is_the_resize_asked_for_to_the_bit_when_the_snap_finds_nothing() {
    // The box a snap that finds nothing hands back is the box that was dragged, not a rebuilt copy of it: the corners of the shape are the same as with no snap at all.
    for (degrees, flip) in [(30.0, false), (77.0, false), (0.0, true), (123.0, true)] {
        let make = || {
            let shape = turned(1, (3.123456, 4.654321), 2.735179, 1.465133, degrees);
            if flip { shape.cell("FlipX", "1") } else { shape }
        };
        let (mut plain, mut snapped) = (session_of(page_of_shapes(vec![make()])), session_of(page_of_shapes(vec![make()])));
        let c0 = corners(&plain, 1)[0];
        let asked = json!({"id": 1, "x": c0.0 + 0.123456, "y": c0.1 - 0.0654321, "width": 2.345678, "height": 1.234567});
        run(&mut plain, "shape.resize", asked.clone());
        let mut with_snap = asked;
        with_snap["snap"] = json!({"zoom": 96.0});
        let r = run(&mut snapped, "shape.resize", with_snap);
        assert_eq!(r["snap"]["hits"], json!([]), "{r}");
        assert_eq!(corners(&plain, 1), corners(&snapped, 1), "{degrees} {flip}");
    }
}

#[test]
fn a_turned_resize_that_a_snap_would_collapse_is_left_as_dragged() {
    // A shape flipped left to right (2 by 1, its lower right corner at (4, 2.5) the origin of its own axes) dragged to a width of 0.02, with a guide on the side that stays (x = 4):
    // the high side would come onto it and leave no width, so the snap is not taken and the shape is as dragged.
    let mut doc = page_of_shapes(vec![turned(1, (3.0, 3.0), 2.0, 1.0, 0.0).cell("FlipX", "1")]);
    add_guide(&mut doc, 1, Axis::X, 4.0, 0).unwrap();
    let mut s = session_of(doc);
    let c0 = origin_corner(&s, 1);
    let r = run(&mut s, "shape.resize", json!({"id": 1, "x": c0.0, "y": c0.1, "width": 0.02, "height": 1.0, "snap": {"zoom": 96.0}}));
    let b = box_in(&s, 1);
    assert!(close(b.x1 - b.x0, 0.02, 1e-9) && close(b.x1, 4.0, 1e-9), "{b:?}: {r}");
}

#[test]
fn a_resize_of_a_shape_flipped_or_turned_round_snaps_the_sides_that_move_and_none_that_stay() {
    // A 2 by 1 shape flipped left to right, top to bottom and both (the last is a half turn drawn with flips: its own axes point left and down the page). Its sides are the page's, and
    // `shape.resize` takes the corner at its own (0, 0) and a size along its own axes. For each move of the own sides (low x, high x, low y, high y, moved inward by the given inches)
    // a guide lies 0.03 inch inside each of the four sides' places once moved: the sides that move land on their guides and the sides that stay are where they were.
    let moves: [(f64, f64, f64, f64); 8] = [
        (0.3, 0.0, 0.0, 0.0),
        (0.0, -0.3, 0.0, 0.0),
        (0.2, -0.2, 0.0, 0.0),
        (0.0, 0.0, 0.2, 0.0),
        (0.0, 0.0, 0.0, -0.2),
        (0.0, 0.0, 0.15, -0.15),
        (0.2, 0.0, 0.0, -0.2),
        (0.0, -0.2, 0.2, 0.0),
    ];
    let (quarter, half, three) = ("1.5707963267948966", "3.141592653589793", "4.71238898038469");
    for (flips, cells) in [
        ("across", vec![("FlipX", "1")]),
        ("up", vec![("FlipY", "1")]),
        ("both", vec![("FlipX", "1"), ("FlipY", "1")]),
        ("a quarter turn", vec![("Angle", quarter)]),
        ("a quarter turn and across", vec![("Angle", quarter), ("FlipX", "1")]),
        ("a quarter turn and up", vec![("Angle", quarter), ("FlipY", "1")]),
        ("a half turn", vec![("Angle", half)]),
        ("a half turn and across", vec![("Angle", half), ("FlipX", "1")]),
        ("three quarters and both", vec![("Angle", three), ("FlipX", "1"), ("FlipY", "1")]),
    ] {
        for (lo_x, hi_x, lo_y, hi_y) in moves {
            let make = || {
                let mut shape = rect(1, 2.0, 2.5, 4.0, 3.5);
                for (name, value) in &cells {
                    shape = shape.cell(name, value);
                }
                shape
            };
            let c = shape_corners(&page_of_shapes(vec![make()]), 1, 1).unwrap();
            let (w, h) = ((c[1].0 - c[0].0).hypot(c[1].1 - c[0].1), (c[3].0 - c[0].0).hypot(c[3].1 - c[0].1));
            let (u, v) = (((c[1].0 - c[0].0) / w, (c[1].1 - c[0].1) / w), ((c[3].0 - c[0].0) / h, (c[3].1 - c[0].1) / h));
            // The box as dragged: the corner at its own (0, 0) moved along its own axes, the size changed by the sides.
            let c0 = (c[0].0 + u.0 * lo_x + v.0 * lo_y, c[0].1 + u.1 * lo_x + v.1 * lo_y);
            let (nw, nh) = (w - lo_x + hi_x, h - lo_y + hi_y);
            let at_corners = [
                c0,
                (c0.0 + u.0 * nw, c0.1 + u.1 * nw),
                (c0.0 + v.0 * nh, c0.1 + v.1 * nh),
                (c0.0 + u.0 * nw + v.0 * nh, c0.1 + u.1 * nw + v.1 * nh),
            ];
            let (nx0, nx1) =
                (at_corners.iter().map(|p| p.0).fold(f64::INFINITY, f64::min), at_corners.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max));
            let (ny0, ny1) =
                (at_corners.iter().map(|p| p.1).fold(f64::INFINITY, f64::min), at_corners.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max));
            let old = (
                c.iter().map(|p| p.0).fold(f64::INFINITY, f64::min),
                c.iter().map(|p| p.1).fold(f64::INFINITY, f64::min),
                c.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max),
                c.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max),
            );
            let mut doc = page_of_shapes(vec![make()]);
            for (axis, at_) in [(Axis::X, nx0 + 0.03), (Axis::X, nx1 - 0.03), (Axis::Y, ny0 + 0.03), (Axis::Y, ny1 - 0.03)] {
                add_guide(&mut doc, 1, axis, at_, 0).unwrap();
            }
            let mut sess = session_of(doc);
            let r = run(&mut sess, "shape.resize", json!({"id": 1, "x": c0.0, "y": c0.1, "width": nw, "height": nh, "snap": {"zoom": 96.0}}));
            let b = shape_bounds(sess.document().unwrap(), 1);
            let moved = |new: f64, was: f64| (new - was).abs() > 1.0e-6;
            let want = (
                if moved(nx0, old.0) { nx0 + 0.03 } else { old.0 },
                if moved(ny0, old.1) { ny0 + 0.03 } else { old.1 },
                if moved(nx1, old.2) { nx1 - 0.03 } else { old.2 },
                if moved(ny1, old.3) { ny1 - 0.03 } else { old.3 },
            );
            assert!(
                close(b.0, want.0, 1e-9) && close(b.1, want.1, 1e-9) && close(b.2, want.2, 1e-9) && close(b.3, want.3, 1e-9),
                "flipped {flips}, moves {lo_x} {hi_x} {lo_y} {hi_y}: {b:?} against {want:?}: {r}"
            );
        }
    }
}

#[test]
fn the_own_frame_of_a_shape_names_at_most_512_copies_of_one_id_and_a_flipped_one_a_half_turn_more() {
    // `frame_of_shapes` is the frame of one shape however often the id is repeated, up to the cap on a selection (512); and a turned shape flipped left to right has its own x axis
    // pointing a half turn from where its angle cell says (30 + 180 degrees, as a number: 3.665).
    let doc = page_of_shapes(vec![turned(1, (3.0, 3.0), 2.0, 1.0, 30.0)]);
    assert!(frame_of_shapes(&doc, 1, &vec![1; 512]).is_some());
    assert!(frame_of_shapes(&doc, 1, &vec![1; 513]).is_none());
    let flipped = page_of_shapes(vec![turned(1, (3.0, 3.0), 2.0, 1.0, 30.0).cell("FlipX", "1")]);
    let f = frame_of_shapes(&flipped, 1, &[1]).unwrap();
    assert!(close(f.angle, 30.0_f64.to_radians() + std::f64::consts::PI, 1e-12), "{f:?}");
    // The same shape's resize tells the frame's angle in degrees: 210.
    let mut s = session_of(flipped);
    let c0 = corners(&s, 1)[0];
    let r = run(
        &mut s,
        "shape.resize",
        json!({"id": 1, "x": c0.0, "y": c0.1, "width": 2.5, "height": 1.0, "snap": {"zoom": 96.0, "keep": "proportions"}}),
    );
    assert!(close(r["snap"]["frame"]["angle"].as_f64().unwrap(), 210.0, 1e-6), "{r}");
}

// ---------------------------------------------------------------------------------------------------------------- guides as a file draws them

/// A guide as a file holds it: a page shape of kind `Guide` pinned at `pin`, with an `InfiniteLine` row from `from` along the way to `to` (both in its own frame).
fn infinite_guide(id: u32, pin: (f64, f64), from: (f64, f64), to: (f64, f64)) -> ShapeBuilder {
    let row = || RowKey::Ix(1);
    ShapeBuilder::new(id)
        .kind(ShapeType::Guide)
        .cell("PinX", &pin.0.to_string())
        .cell("PinY", &pin.1.to_string())
        .cell("Width", "0")
        .cell("Height", "0")
        .cell("LocPinX", "0")
        .cell("LocPinY", "0")
        .row_type("Geometry", Some(0), row(), "InfiniteLine")
        .row_cell("Geometry", Some(0), row(), "X", &from.0.to_string())
        .row_cell("Geometry", Some(0), row(), "Y", &from.1.to_string())
        .row_cell("Geometry", Some(0), row(), "A", &to.0.to_string())
        .row_cell("Geometry", Some(0), row(), "B", &to.1.to_string())
}

#[test]
fn a_guide_with_an_infinite_line_row_runs_from_its_x_and_y_along_the_way_to_its_a_and_b() {
    // A guide pinned at (2, 1) whose row starts at (1, 2) in its own frame and runs to (1 + 3 cos 30, 2 + 3 sin 30): the line through (3, 3) along 30 degrees (the length of the way is
    // no part of the line). A shape turned 30 dragged 0.03 inch off it by its own bottom side lands on it.
    let (s30, c30) = 30.0_f64.to_radians().sin_cos();
    let through = (3.0, 3.0);
    let guide = infinite_guide(1, (2.0, 1.0), (1.0, 2.0), (1.0 + 3.0 * c30, 2.0 + 3.0 * s30));
    let start = at(through, 30.0, 5.0, 0.53);
    let mut s = session_of(page_of_shapes(vec![guide, turned(2, (9.0, 9.0), 2.0, 1.0, 30.0)]));
    let page = SnapPage::read(s.document().unwrap(), 1).unwrap();
    assert_eq!(page.slanted.len(), 1, "{:?}", page.slanted);
    assert!(near2(page.slanted[0].at, through, 1e-12) && close(page.slanted[0].angle, 30.0_f64.to_radians(), 1e-12), "{:?}", page.slanted[0]);
    let r = run(&mut s, "shape.move", json!({"ids": [2], "dx": start.0 - 9.0, "dy": start.1 - 9.0, "snap": {"zoom": 96.0}}));
    let want = at(through, 30.0, 5.0, 0.5);
    assert!(near2(middle(&s, 2), want, 1e-9), "{:?} against {want:?}: {r}", middle(&s, 2));
}

#[test]
fn a_guide_is_along_an_axis_within_a_thousandth_of_a_radian_whatever_the_length_of_its_way() {
    // The way of the row, in the guide's frame: 1000 long and 5e-4 off level is a level guide at the pin's y; 1000 long and 5e-4 off upright an upright one at its x; 3e-3 off level
    // is along no axis, and so is a short way (0.01 long, 0.0001 up) that is 1e-2 off level: the tilt is the angle and not the angle times the length.
    for (way, kind) in [
        ((1000.0, 0.5), "level"),
        ((-1000.0, -0.5), "level"),
        ((0.5, 1000.0), "upright"),
        ((-0.5, -1000.0), "upright"),
        ((1000.0, 3.0), "slanted"),
        ((0.01, 0.0001), "slanted"),
    ] {
        let doc = page_of_shapes(vec![infinite_guide(1, (4.0, 5.0), (0.0, 0.0), way)]);
        let page = SnapPage::read(&doc, 1).unwrap();
        match kind {
            "level" => assert!(
                page.slanted.is_empty() && page.guides.len() == 1 && page.guides[0].axis == Axis::Y && page.guides[0].at == 5.0,
                "{way:?}: {:?}",
                page.guides
            ),
            "upright" => assert!(
                page.slanted.is_empty() && page.guides.len() == 1 && page.guides[0].axis == Axis::X && page.guides[0].at == 4.0,
                "{way:?}: {:?}",
                page.guides
            ),
            _ => assert!(page.guides.is_empty() && page.slanted.len() == 1, "{way:?}: {:?}", page.slanted),
        }
    }
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS's hypot is not correctly rounded: no way of rise 1 within 200,000 steps of the root of 999,999 has a length of exactly 1000 (it goes from 999.99999999999989 to 1000.0000000000001), so the exact case cannot be built there"
)]
fn a_guide_exactly_a_thousandth_off_an_axis_is_along_none_and_a_hair_less_is_along_it() {
    // The tilt is the sine of the angle of the way, 1e-3 at most to be along an axis, and the way is `(dx, 1)` with `dx.hypot(1)` exactly 1000: a sine of exactly 0.001. It is not
    // under the limit, so the guide is along no axis; with a longer way (a sine a hair under) it is along the axis, and so is the mirror image of each way.
    let base = 999_999.0_f64.sqrt();
    let dx = (0..400_u64)
        .flat_map(|k| [f64::from_bits(base.to_bits() + k), f64::from_bits(base.to_bits() - k)])
        .find(|d| d.hypot(1.0) == 1000.0)
        .expect("a way whose length is exactly 1000 and whose rise is 1");
    assert_eq!(1.0 / dx.hypot(1.0), 0.001);
    let read = |way: (f64, f64)| SnapPage::read(&page_of_shapes(vec![infinite_guide(1, (4.0, 5.0), (0.0, 0.0), way)]), 1).unwrap();
    for (way, near) in [((dx, 1.0), "level"), ((-dx, -1.0), "level"), ((1.0, dx), "upright"), ((-1.0, -dx), "upright")] {
        let page = read(way);
        assert!(
            page.guides.is_empty() && page.slanted.len() == 1,
            "{near}: exactly a thousandth is along no axis: {way:?}: {:?} {:?}",
            page.guides,
            page.slanted
        );
        let a_hair_less = (way.0 * 1.0000001, way.1 * 1.0000001);
        let way = if near == "level" { (a_hair_less.0, way.1) } else { (way.0, a_hair_less.1) };
        let page = read(way);
        assert!(
            page.slanted.is_empty() && page.guides.len() == 1,
            "{near}: a hair less is along the axis: {way:?}: {:?} {:?}",
            page.guides,
            page.slanted
        );
    }
}

#[test]
fn a_slanted_guide_is_read_in_paper_inches_on_a_page_of_any_scale_and_one_that_overflows_is_left_out() {
    // A guide through (3, 2) paper inches along 30 degrees on a page of 2 and of 0.5 drawing units to an inch; and one pinned at 1e308 drawing units on a page of 0.5 units to an inch,
    // whose place in inches is past what a number holds and so is no guide, the way an upright one at that place is none.
    for k in [1.0, 2.0, 0.5] {
        let page = PageBuilder::new()
            .cell("PageWidth", "20")
            .cell("PageHeight", "20")
            .cell("PageScale", "1")
            .cell("DrawingScale", &k.to_string())
            .shape(guide_at(1, (3.0 * k, 2.0 * k), 30.0));
        let doc = DocBuilder::new().page(1, page).build();
        let read = SnapPage::read(&doc, 1).unwrap();
        assert!(read.slanted.len() == 1 && near2(read.slanted[0].at, (3.0, 2.0), 1e-12), "{k}: {:?}", read.slanted);
    }
    let page = PageBuilder::new()
        .cell("PageWidth", "20")
        .cell("PageHeight", "20")
        .cell("PageScale", "1")
        .cell("DrawingScale", "0.5")
        .shape(guide_at(1, (1.0e308, 1.0), 30.0))
        .shape(guide_at(2, (1.0e308, 1.0), 90.0))
        .shape(guide_at(3, (1.0, 1.0e308), 0.0));
    let doc = DocBuilder::new().page(1, page).build();
    let read = SnapPage::read(&doc, 1).unwrap();
    assert!(read.slanted.is_empty() && read.guides.is_empty(), "{:?} {:?}", read.slanted, read.guides);
}

// ---------------------------------------------------------------------------------------------------------------- equal gaps along the shapes' own direction

#[test]
fn a_row_of_shapes_turned_alike_is_marked_along_their_own_direction() {
    // Three 2 by 1 inch shapes turned 30 degrees, pins 4 inches apart along their own x axis (gaps of 2 inches), and a fourth, 4 by 1 turned 30 too, dragged 0.03 inch off the place
    // that repeats the gap: its pin lands 13 inches along from the first (16.258330, 11.5) and the three gaps are one chain along 30 degrees, each "2 in". No two boxes round them
    // share a band, so no page-axis spacing applies. (The fourth is 4 inches long so that its row reaches 16 inches and takes in the first shape.)
    let a = (5.0, 5.0);
    let raw = at(a, 30.0, 13.03, 0.0);
    let mut s = session_of(page_of_shapes(vec![
        turned(1, a, 2.0, 1.0, 30.0),
        turned(2, at(a, 30.0, 4.0, 0.0), 2.0, 1.0, 30.0),
        turned(3, at(a, 30.0, 8.0, 0.0), 2.0, 1.0, 30.0),
        turned(4, (3.0, 14.0), 4.0, 1.0, 30.0),
    ]));
    let r = run(&mut s, "shape.move", json!({"ids": [4], "dx": raw.0 - 3.0, "dy": raw.1 - 14.0, "snap": {"zoom": 96.0}}));
    let want = at(a, 30.0, 13.0, 0.0);
    assert!(near2(middle(&s, 4), want, 1e-9), "{:?} against {want:?}: {r}", middle(&s, 4));
    let gaps = marks_of(&r, "gap");
    assert_eq!(gaps.len(), 3, "{r}");
    assert!(gaps.iter().all(|g| g["label"] == json!("2 in") && g["chain"] == gaps[0]["chain"] && g["axis"] == Value::Null), "{r}");
    // The row runs along 30 degrees.
    for g in gaps {
        let run = (g["to"][0].as_f64().unwrap() - g["from"][0].as_f64().unwrap(), g["to"][1].as_f64().unwrap() - g["from"][1].as_f64().unwrap());
        assert!(close(run.1.atan2(run.0).to_degrees().rem_euclid(180.0), 30.0, 1e-3), "{g}");
    }
    let hit = r["snap"]["hits"].as_array().unwrap().iter().find(|h| h["target"] == json!("spacing")).unwrap_or_else(|| panic!("{r}")).clone();
    assert!(hit["normal"].is_number() && close(hit["gap"].as_f64().unwrap(), 2.0, 1e-9), "{r}");
}

// ---------------------------------------------------------------------------------------------------------------- a group, the half-turn wrap, angles a hair apart

/// A group 4 by 2 inches, turned `degrees`, its pin in its middle at `pin`, holding one small undrawn member.
fn group_at(id: u32, pin: (f64, f64), degrees: f64) -> ShapeBuilder {
    group_at_with(id, pin, degrees, 0.0)
}

/// [`group_at`] with its member turned `member_degrees` in the group.
fn group_at_with(id: u32, pin: (f64, f64), degrees: f64, member_degrees: f64) -> ShapeBuilder {
    ShapeBuilder::new(id)
        .group()
        .cell("PinX", &pin.0.to_string())
        .cell("PinY", &pin.1.to_string())
        .cell("Width", "4")
        .cell("Height", "2")
        .cell("LocPinX", "2")
        .cell("LocPinY", "1")
        .cell("Angle", &degrees.to_radians().to_string())
        .child(
            ShapeBuilder::new(id + 1)
                .cell("PinX", "2")
                .cell("PinY", "1")
                .cell("Width", "1")
                .cell("Height", "0.5")
                .cell("LocPinX", "0.5")
                .cell("LocPinY", "0.25")
                .cell("Angle", &member_degrees.to_radians().to_string()),
        )
}

#[test]
fn a_turned_group_lines_up_its_own_sides_like_a_shape_does() {
    // A turned group is one shape with a frame of its own (its width and height, its turn): dragged 0.03 inch off the own lines of a 4 by 2 rectangle turned the same, it lands on
    // them, 6 inches along the rectangle's own x axis.
    let a = (3.0, 3.0);
    let raw = at(a, 30.0, 6.0, 0.03);
    let mut s = session_of(page_of_shapes(vec![turned(1, a, 4.0, 2.0, 30.0), group_at(2, (12.0, 12.0), 30.0)]));
    let r = run(&mut s, "shape.move", json!({"ids": [2], "dx": raw.0 - 12.0, "dy": raw.1 - 12.0, "snap": {"zoom": 96.0}}));
    let want = at(a, 30.0, 6.0, 0.0);
    assert!(near2(middle(&s, 2), want, 1e-9), "{:?} against {want:?}: {r}", middle(&s, 2));
    assert!(r["snap"]["hits"].as_array().unwrap().iter().any(|h| h["normal"].is_number()), "{r}");
}

#[test]
fn a_shape_turned_a_half_turn_more_has_the_same_own_lines_and_the_like_sides_meet() {
    // A turned 30 degrees and B turned 210 (a half turn more): their own sides are the same lines, and low meets low along the direction the lines are folded to. B is dragged
    // 0.03 inch off A's own lines and lands on them like B turned 30 does.
    let a = (3.0, 3.0);
    let raw = at(a, 30.0, 4.0, 0.03);
    let mut s = session_of(page_of_shapes(vec![turned(1, a, 2.0, 1.0, 30.0), turned(2, (10.0, 3.0), 2.0, 1.0, 210.0)]));
    let r = run(&mut s, "shape.move", json!({"ids": [2], "dx": raw.0 - 10.0, "dy": raw.1 - 3.0, "snap": {"zoom": 96.0}}));
    let want = at(a, 30.0, 4.0, 0.0);
    assert!(near2(middle(&s, 2), want, 1e-9), "{:?} against {want:?}: {r}", middle(&s, 2));
}

#[test]
fn two_angles_within_a_billionth_of_a_radian_are_one_direction_and_a_few_billionths_apart_are_two() {
    // The library's tolerance for two directions to be one (1e-9 radian): B turned 4e-10 radian off A shares A's own lines; turned 3e-9 radian off, it shares none.
    let a = (3.0, 3.0);
    let raw = at(a, 30.0, 4.0, 0.03);
    let degrees = |rad: f64| (30.0_f64.to_radians() + rad).to_degrees();
    for (off, snaps) in [(4.0e-10, true), (3.0e-9, false)] {
        let mut s = session_of(page_of_shapes(vec![turned(1, a, 2.0, 1.0, 30.0), turned(2, (10.0, 3.0), 2.0, 1.0, degrees(off))]));
        let r = run(&mut s, "shape.move", json!({"ids": [2], "dx": raw.0 - 10.0, "dy": raw.1 - 3.0, "snap": {"zoom": 96.0}}));
        let own = r["snap"]["hits"].as_array().unwrap().iter().filter(|h| h["normal"].is_number()).count();
        assert_eq!(own > 0, snaps, "{off:e} radian off: {r}");
    }
}

// ---------------------------------------------------------------------------------------------------------------- far drawings

/// What a drag of the turned shape `id` by (`dx`, `dy`) snaps to, asked of the library as `shape.move` asks it (the commands refuse a pin more than a million inches out, so this is the
/// function they call): where the shape lands, and the hits.
fn dragged(doc: &diacraft_doc::Document, id: u32, (dx, dy): (f64, f64)) -> (f64, f64, Vec<diacraft_engine::snap::Hit>) {
    let page = SnapPage::read(doc, 1).unwrap();
    let b = bounds_of(doc, 1, &[id]).unwrap().rect;
    let f = frame_of_shapes(doc, 1, &[id]).expect("a turned shape has a frame");
    let rect = Rect { x0: b.x0 + dx, y0: b.y0 + dy, x1: b.x1 + dx, y1: b.y1 + dy };
    let frame = OwnFrame { centre: (f.centre.0 + dx, f.centre.1 + dy), ..f };
    let query = Query { subject: Subject::Box(rect), using: Use::Place, moving: &[id], zoom: 96.0, own_points: &[] };
    let out = snap_ext(doc, &page, &vector_settings(), &query, Extras { frame: Some(frame), ..Extras::default() });
    (dx + out.dx, dy + out.dy, out.hits)
}

#[test]
fn two_turned_shapes_far_from_the_origin_line_up_their_own_sides_as_they_do_at_the_origin() {
    // V5 moved by (off, off): the own lines meet and the shape lands where the same drag lands at the origin, to the rounding of numbers that big.
    for off in [0.0, 1.0e3, 1.0e6, 1.0e8, 1.0e10] {
        let a = (3.0 + off, 3.0 + off);
        let start = (10.0 + off, 3.0 + off);
        let raw = at(a, 30.0, 4.0, 0.03);
        let doc = page_of_shapes(vec![turned(1, a, 2.0, 1.0, 30.0), turned(2, start, 2.0, 1.0, 30.0)]);
        let (dx, dy, hits) = dragged(&doc, 2, (raw.0 - start.0, raw.1 - start.1));
        let want = at(a, 30.0, 4.0, 0.0);
        let tol = 1.0e-9 + 64.0 * f64::EPSILON * off;
        assert!(near2((start.0 + dx, start.1 + dy), want, tol), "{off:e}: {:?} against {want:?}", (start.0 + dx, start.1 + dy));
        assert!(hits.iter().any(|h| h.normal.is_some()), "{off:e}: {hits:?}");
    }
}

#[test]
fn a_slanted_guide_far_from_the_origin_is_met_by_a_turned_shape_as_at_the_origin() {
    // V7 moved by (off, off). The guide's direction is its turn applied to the way its line runs in its frame, so it agrees with a shape turned alike to a billionth of a radian
    // wherever it is (two far points of it would not).
    for off in [0.0, 1.0e6, 1.0e8, 1.0e10] {
        let start = at((off, off), 30.0, 5.0, 0.53);
        let doc = page_of_shapes(vec![guide_at(1, (off, off), 30.0), turned(2, (9.0 + off, 9.0 + off), 2.0, 1.0, 30.0)]);
        let (dx, dy, hits) = dragged(&doc, 2, (start.0 - 9.0 - off, start.1 - 9.0 - off));
        let want = at((off, off), 30.0, 5.0, 0.5);
        let tol = 1.0e-9 + 64.0 * f64::EPSILON * off;
        assert!(near2((9.0 + off + dx, 9.0 + off + dy), want, tol), "{off:e}: {:?} against {want:?}", (9.0 + off + dx, 9.0 + off + dy));
        assert!(hits.iter().any(|h| h.target == diacraft_engine::snap::Target::Guide), "{off:e}: {hits:?}");
    }
}

// ---------------------------------------------------------------------------------------------------------------- properties, through the commands

/// A seeded generator: no clock, no entropy.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 11) as f64) / ((1u64 << 53) as f64)
    }
    fn between(&mut self, a: f64, b: f64) -> f64 {
        a + (b - a) * self.next()
    }
    fn below(&mut self, n: usize) -> usize {
        ((self.next() * n as f64) as usize).min(n.saturating_sub(1))
    }
}

/// The angles shapes are turned by: a few shared ones, so that own lines meet, and the rest random.
fn some_angle(r: &mut Rng) -> f64 {
    match r.below(5) {
        0 => 30.0,
        1 => 30.0,
        2 => 75.0,
        3 => -40.0,
        _ => r.between(1.0, 89.0),
    }
}

/// The pin, the width, the height and the angle in degrees of a shape made.
type Made = ((f64, f64), f64, f64, f64);

/// A shape's own frame on the page: its middle, the unit vectors of its own axes, its width and its height.
type OnPage = ((f64, f64), (f64, f64), (f64, f64), f64, f64);

/// A page of random turned rectangles (ids from 1), and the pin, size and angle of each.
fn random_turned_page(r: &mut Rng, n: usize) -> (Vec<ShapeBuilder>, Vec<Made>) {
    let mut shapes = Vec::new();
    let mut made = Vec::new();
    for i in 0..n {
        let (pin, w, h, degrees) = ((r.between(4.0, 16.0), r.between(4.0, 16.0)), r.between(0.5, 3.0), r.between(0.5, 2.0), some_angle(r));
        let mut shape = turned(i as u32 + 1, pin, w, h, degrees);
        if r.below(6) == 0 {
            shape = shape.cell("FlipX", "1");
        }
        shapes.push(shape);
        made.push((pin, w, h, degrees));
    }
    (shapes, made)
}

/// The own frame of a shape on the page: its middle, the unit vectors of its own axes and its width and height.
fn frame_on_page(s: &Session, id: u32) -> OnPage {
    let c = corners(s, id);
    let (w, h) = (((c[1].0 - c[0].0).hypot(c[1].1 - c[0].1)), ((c[3].0 - c[0].0).hypot(c[3].1 - c[0].1)));
    let centre = ((c[0].0 + c[2].0) / 2.0, (c[0].1 + c[2].1) / 2.0);
    (centre, ((c[1].0 - c[0].0) / w, (c[1].1 - c[0].1) / w), ((c[3].0 - c[0].0) / h, (c[3].1 - c[0].1) / h), w, h)
}

/// Whether a line mark that was snapped to holds on the shape where it landed: one of the shape's own sides (or, for a mark along an axis, a side of the box round it) is on it.
fn mark_holds(s: &Session, id: u32, mark: &Value) -> bool {
    let (centre, u, v, w, h) = frame_on_page(s, id);
    let at = mark["at"].as_f64().unwrap();
    let c = corners(s, id);
    let tol = 3.0e-6;
    match mark["normal"].as_f64() {
        Some(normal) => {
            let n = (normal.to_radians().cos(), normal.to_radians().sin());
            let half = if (n.0 * u.0 + n.1 * u.1).abs() >= (n.0 * v.0 + n.1 * v.1).abs() { w } else { h } / 2.0;
            let at_c = n.0 * centre.0 + n.1 * centre.1;
            [-half, 0.0, half].iter().any(|d| close(at_c + d, at, tol))
        }
        None => {
            let along: fn((f64, f64)) -> f64 = if mark["axis"] == json!("x") { |p| p.0 } else { |p| p.1 };
            let (lo, hi) = (c.iter().map(|p| along(*p)).fold(f64::INFINITY, f64::min), c.iter().map(|p| along(*p)).fold(f64::NEG_INFINITY, f64::max));
            [lo, (lo + hi) / 2.0, hi].iter().any(|d| close(*d, at, tol))
        }
    }
}

#[test]
fn the_line_drawn_is_where_a_turned_shape_lands_whichever_way_it_is_moved() {
    // 400 seeded pages of turned and flipped rectangles, and a turned shape dragged near the own lines of one of them, by `shape.move` free, locked to x or to y, and as one press of
    // an arrow key: every line mark that was snapped to is a line the shape lies on where it landed, and a locked move did not move on its other axis, to the bit.
    let mut r = Rng(0x5eed_0001);
    let (mut own_hits, mut checked) = (0usize, 0usize);
    for case in 0..400 {
        let n = 3 + r.below(5);
        let (mut shapes, made) = random_turned_page(&mut r, n);
        let target = made[r.below(made.len())];
        let (tpin, tw, _, tdeg) = target;
        let (w, h) = (r.between(0.5, 3.0), r.between(0.5, 2.0));
        let id = 100;
        shapes.push(turned(id, (2.0, 2.0), w, h, if r.below(4) == 0 { some_angle(&mut r) } else { tdeg }));
        let mut s = session_of(page_of_shapes(shapes));
        // Where the subject is dragged to: along the target's own x axis, a little off its own lines.
        let raw = at(tpin, tdeg, tw / 2.0 + w / 2.0 + r.between(0.2, 3.0), r.between(-0.07, 0.07));
        let (dx, dy) = (raw.0 - 2.0, raw.1 - 2.0);
        let mode = r.below(4);
        let snap = match mode {
            0 => json!({"zoom": 96.0}),
            1 => json!({"zoom": 96.0, "lock": "x"}),
            2 => json!({"zoom": 96.0, "lock": "y"}),
            _ => json!({"zoom": 96.0, "nudge": {"tick": 0.125}}),
        };
        let (dx, dy) = if mode == 3 { (if r.below(2) == 0 { 1.0 } else { -1.0 }, 0.0) } else { (dx, dy) };
        let before = middle(&s, id);
        let out = run(&mut s, "shape.move", json!({"ids": [id], "dx": dx, "dy": dy, "snap": snap}));
        let now = middle(&s, id);
        if mode == 1 {
            assert!(now.1 == before.1, "case {case}: x lock moved y: {before:?} to {now:?}: {out}");
        }
        if mode == 2 {
            assert!(now.0 == before.0, "case {case}: y lock moved x: {before:?} to {now:?}: {out}");
        }
        for m in marks_of(&out, "line").into_iter().filter(|m| m["hit"] == json!(true)) {
            checked += 1;
            own_hits += usize::from(m["normal"].is_number());
            assert!(mark_holds(&s, id, m), "case {case} mode {mode}: a mark that was snapped to does not hold where the shape landed: {m}\n{out}");
        }
    }
    assert!(own_hits > 60 && checked > own_hits, "the property is not vacuous: {own_hits} own lines of {checked}");
}

#[test]
fn a_turned_move_has_the_mirror_image_of_its_mirror_image() {
    // A page mirrored about x = 10 (a shape turned t at (x, y) becomes one turned -t at (20 - x, y); a rectangle is its own mirror image) and the same drag mirrored: the shape lands
    // at the mirror image of where it lands, and snaps to the mirror image's lines. 300 seeded pages.
    let mut r = Rng(0x5eed_0002);
    let mut snapped = 0usize;
    for case in 0..300 {
        let n = 3 + r.below(4);
        let mut specs = Vec::new();
        for _ in 0..n {
            specs.push(((r.between(4.0, 16.0), r.between(4.0, 16.0)), r.between(0.5, 3.0), r.between(0.5, 2.0), some_angle(&mut r)));
        }
        let target = specs[r.below(specs.len())];
        let (w, h, deg) = (r.between(0.5, 3.0), r.between(0.5, 2.0), target.3);
        let raw = at(target.0, target.3, target.1 / 2.0 + w / 2.0 + r.between(0.2, 3.0), r.between(-0.07, 0.07));
        let build = |mirror: bool| {
            let m = |p: (f64, f64)| if mirror { (20.0 - p.0, p.1) } else { p };
            let a = |d: f64| if mirror { -d } else { d };
            let mut shapes: Vec<ShapeBuilder> =
                specs.iter().enumerate().map(|(i, (p, sw, sh, d))| turned(i as u32 + 1, m(*p), *sw, *sh, a(*d))).collect();
            shapes.push(turned(100, m((2.0, 2.0)), w, h, a(deg)));
            let mut s = session_of(page_of_shapes(shapes));
            let to = m(raw);
            let out = run(&mut s, "shape.move", json!({"ids": [100], "dx": to.0 - m((2.0, 2.0)).0, "dy": to.1 - 2.0, "snap": {"zoom": 96.0}}));
            (middle(&s, 100), out)
        };
        let ((plain, out), (mirrored, out_m)) = (build(false), build(true));
        assert!(
            close(plain.0, 20.0 - mirrored.0, 1e-7) && close(plain.1, mirrored.1, 1e-7),
            "case {case}: {plain:?} against the mirror of {mirrored:?}\n{out}\n{out_m}"
        );
        snapped += usize::from(out["snap"]["hits"].as_array().is_some_and(|h| !h.is_empty()));
    }
    assert!(snapped > 100, "{snapped} of 300 snapped");
}

#[test]
fn a_turned_or_flipped_resize_keeps_its_angle_and_its_fixed_sides_and_every_line_it_snapped_to_holds() {
    // 400 seeded resizes of turned and flipped shapes by a side or a corner, with guides and other shapes about: the angle is the shape's to the bit, the corner at the shape's own
    // (0, 0) does not move (the sides that stay are where they were) unless the proportions are kept, which grow the other axis about its middle from a side and keep the shape's own
    // ratio, the box keeps a size, and a line the resize snapped to is one the shape lies on where it ended.
    let mut r = Rng(0x5eed_0003);
    let (mut hits, mut flipped) = (0usize, 0usize);
    for case in 0..400 {
        let n = 2 + r.below(4);
        let (mut shapes, _) = random_turned_page(&mut r, n);
        let (pin, w, h, degrees) = ((r.between(6.0, 14.0), r.between(6.0, 14.0)), r.between(1.0, 3.0), r.between(0.6, 2.0), some_angle(&mut r));
        let mut subject = turned(100, pin, w, h, degrees);
        let flip = r.below(3) == 0;
        if flip {
            subject = subject.cell(if r.below(2) == 0 { "FlipX" } else { "FlipY" }, "1");
            flipped += 1;
        }
        shapes.push(subject);
        let mut doc = page_of_shapes(shapes);
        for _ in 0..r.below(3) {
            let axis = if r.below(2) == 0 { Axis::X } else { Axis::Y };
            add_guide(&mut doc, 1, axis, r.between(4.0, 16.0), 0).unwrap();
        }
        let mut s = session_of(doc);
        let c0 = corners(&s, 100)[0];
        let angle_before = shape_frame(s.document().unwrap(), 1, 100).unwrap().angle;
        let (grow_w, grow_h) = match r.below(3) {
            0 => (r.between(-0.5, 1.0), 0.0),
            1 => (0.0, r.between(-0.3, 0.8)),
            _ => (r.between(-0.5, 1.0), r.between(-0.3, 0.8)),
        };
        let (nw, nh) = ((w + grow_w).max(0.2), (h + grow_h).max(0.2));
        let kept = r.below(4) == 0;
        let keep = if kept { json!({"zoom": 96.0, "keep": "proportions"}) } else { json!({"zoom": 96.0}) };
        let out = run(&mut s, "shape.resize", json!({"id": 100, "x": c0.0, "y": c0.1, "width": nw, "height": nh, "snap": keep}));
        let c = corners(&s, 100);
        if kept {
            let (_, _, _, kw, kh) = frame_on_page(&s, 100);
            assert!(close(kw / kh, w / h, 1e-9), "case {case}: the proportions were not kept: {kw} by {kh} against {w} by {h}\n{out}");
        } else {
            assert!(
                close(c[0].0, c0.0, 1e-9) && close(c[0].1, c0.1, 1e-9),
                "case {case}: the corner at the shape's own (0, 0) moved: {c0:?} to {:?}\n{out}",
                c[0]
            );
        }
        assert_eq!(shape_frame(s.document().unwrap(), 1, 100).unwrap().angle, angle_before, "case {case}: the angle changed");
        let (_, _, _, fw, fh) = frame_on_page(&s, 100);
        assert!(fw > 0.0 && fh > 0.0, "case {case}: {fw} by {fh}");
        for m in marks_of(&out, "line").into_iter().filter(|m| m["hit"] == json!(true)) {
            hits += 1;
            // Some place of the shape's box (its corners, the middles of its sides, its middle) is on the line.
            let places: Vec<(f64, f64)> = {
                let c = corners(&s, 100);
                let mid = |a: (f64, f64), b: (f64, f64)| ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
                vec![c[0], c[1], c[2], c[3], mid(c[0], c[1]), mid(c[1], c[2]), mid(c[2], c[3]), mid(c[3], c[0]), mid(c[0], c[2])]
            };
            let at_line = m["at"].as_f64().unwrap();
            let normal = m["normal"]
                .as_f64()
                .map_or((if m["axis"] == json!("x") { 1.0 } else { 0.0 }, if m["axis"] == json!("x") { 0.0 } else { 1.0 }), |d| {
                    (d.to_radians().cos(), d.to_radians().sin())
                });
            assert!(
                places.iter().any(|p| close(normal.0 * p.0 + normal.1 * p.1, at_line, 3.0e-6)),
                "case {case}: no place of the shape is on the line it snapped to: {m}\n{out}"
            );
        }
    }
    assert!(hits > 40 && flipped > 60, "the property is not vacuous: {hits} lines snapped to, {flipped} flipped shapes");
}
