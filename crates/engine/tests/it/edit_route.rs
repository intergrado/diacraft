//! The route a connector holds after a move: no repeated vertex, no zero-length leg, no leg that goes out and comes straight back, the
//! begin and the end on the glued points, the arrowhead on the end, and the one-dimensional frame (`BeginX`, `EndX`, `Angle`, `Width`, the pin)
//! in step with the ends, after a shape is moved left, right, up, down and across, whichever of its ends' shapes moves.
//!
//! A LibreOffice rendering of an edited drawing showed a moved connector with one end missing its arrowhead and a segment drawn with two heads;
//! a route that goes out along a line and straight back along it is drawn that way, and these tests would not let one through.

use diacraft_doc::{CellAddr, Document, Owner, Resolver, SheetRef};
use diacraft_engine::draw::{Pose, connection_points};
use serde_json::json;

use crate::edit::{doc, drop_shape, geometry, id_of, num, route};
use crate::{blank_session, check_session, run};

type P = (f64, f64);

/// The number a cell holds, or 0 where it holds none: a cell nothing sets is the plain default, which for an arrow is no arrow.
fn held_number(d: &Document, id: u32, name: &str) -> f64 {
    let sheet = SheetRef::Shape(d.find_shape(Owner::Page(1), id).unwrap());
    Resolver::new(d).resolve(&sheet, &CellAddr::direct(name)).and_then(|f| f.value.cell?.value.as_ref()?.number()).unwrap_or(0.0)
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1.0e-6
}

fn near_point(a: P, b: P) -> bool {
    near(a.0, b.0) && near(a.1, b.1)
}

/// Where each glued end of connector `id` should be: the connection point its `Connect` row names, on the page.
fn anchors(d: &Document, id: u32) -> (P, P) {
    let end_at = |from_cell: &str| -> P {
        let c = d.page(1).unwrap().connects.iter().find(|c| c.from_sheet == id && c.from_cell.as_deref() == Some(from_cell)).expect("a Connect row");
        let index = c.to_part.unwrap() - 100;
        let p = connection_points(d, 1, c.to_sheet).into_iter().find(|p| p.index == index).expect("the connection point");
        (p.x, p.y)
    };
    (end_at("BeginX"), end_at("EndX"))
}

/// What is wrong with a route's points, if anything, found without the code under test: a zero-length leg, a slanted leg, a leg that doubles
/// back on the one before it, and two legs of one line that share a stretch of it.
fn flaw(points: &[P]) -> Option<String> {
    for w in points.windows(2) {
        let (dx, dy) = (w[1].0 - w[0].0, w[1].1 - w[0].1);
        if dx.abs() < 1.0e-6 && dy.abs() < 1.0e-6 {
            return Some(format!("a zero-length leg at {:?}", w[0]));
        }
        if dx.abs() > 1.0e-6 && dy.abs() > 1.0e-6 {
            return Some(format!("a slanted leg from {:?} to {:?}", w[0], w[1]));
        }
    }
    for w in points.windows(3) {
        let (a, b) = ((w[1].0 - w[0].0, w[1].1 - w[0].1), (w[2].0 - w[1].0, w[2].1 - w[1].1));
        // Opposite ways along one line: the cosine between the legs is -1. (The rows are rounded to nine places, so two legs at a right angle
        // can have a dot product a little under zero.)
        let cosine = (a.0 * b.0 + a.1 * b.1) / (a.0.hypot(a.1) * b.0.hypot(b.1));
        if cosine < -0.999 {
            return Some(format!("it doubles back at {:?}", w[1]));
        }
    }
    for (i, a) in points.windows(2).enumerate() {
        for b in points.windows(2).skip(i + 2) {
            let (a_flat, b_flat) = (near(a[0].1, a[1].1), near(b[0].1, b[1].1));
            let (a_up, b_up) = (near(a[0].0, a[1].0), near(b[0].0, b[1].0));
            let span = |l: &[P], x: bool| if x { (l[0].0.min(l[1].0), l[0].0.max(l[1].0)) } else { (l[0].1.min(l[1].1), l[0].1.max(l[1].1)) };
            if a_flat && b_flat && near(a[0].1, b[0].1) {
                let (s, t) = (span(a, true), span(b, true));
                if s.0.max(t.0) < s.1.min(t.1) - 1.0e-6 {
                    return Some(format!("two legs run over each other on y {}", a[0].1));
                }
            }
            if a_up && b_up && near(a[0].0, b[0].0) {
                let (s, t) = (span(a, false), span(b, false));
                if s.0.max(t.0) < s.1.min(t.1) - 1.0e-6 {
                    return Some(format!("two legs run over each other on x {}", a[0].0));
                }
            }
        }
    }
    None
}

/// Everything a connector glued at both ends must be after an edit.
fn assert_sound(d: &Document, id: u32, what: &str) {
    let (begin, end) = anchors(d, id);
    let cell = |name: &str| num(d, id, name);
    assert!(
        near_point((cell("BeginX"), cell("BeginY")), begin),
        "{what}: the begin is ({}, {}), the glued point is {begin:?}",
        cell("BeginX"),
        cell("BeginY")
    );
    assert!(near_point((cell("EndX"), cell("EndY")), end), "{what}: the end is ({}, {}), the glued point is {end:?}", cell("EndX"), cell("EndY"));
    // The one-dimensional frame: `Width` the distance, `Angle` the way, the pin in the middle, the begin at (0, `LocPinY`).
    let length = (end.0 - begin.0).hypot(end.1 - begin.1);
    assert!(near(cell("Width"), length), "{what}: Width {} is not the distance {length}", cell("Width"));
    if length > 1.0e-6 {
        let (c, s) = (cell("Angle").cos(), cell("Angle").sin());
        assert!(
            near(c, (end.0 - begin.0) / length) && near(s, (end.1 - begin.1) / length),
            "{what}: Angle {} does not point from the begin to the end",
            cell("Angle")
        );
    }
    assert!(near_point((cell("PinX"), cell("PinY")), ((begin.0 + end.0) / 2.0, (begin.1 + end.1) / 2.0)), "{what}: the pin is not at the middle");
    assert!(
        near(cell("Height"), 0.0) && near(cell("LocPinX"), length / 2.0) && near(cell("LocPinY"), 0.0),
        "{what}: the local pin is not at the middle of the width"
    );
    // The arrowhead is on the end, and not on the begin.
    assert_eq!(cell("EndArrow"), 4.0, "{what}");
    assert_eq!(held_number(d, id, "BeginArrow"), 0.0, "{what}");
    // The rows: a move, then lines; the first at the begin of the frame and the last at its end.
    let rows = geometry(d, id);
    assert!(rows.len() >= 2, "{what}: {rows:?}");
    assert_eq!(rows[0].0, "MoveTo", "{what}");
    assert!(rows.iter().skip(1).all(|r| r.0 == "LineTo"), "{what}: {rows:?}");
    assert!(near_point((rows[0].1, rows[0].2), (0.0, cell("LocPinY"))), "{what}: the first row is {:?}", rows[0]);
    let last = &rows[rows.len() - 1];
    assert!(near_point((last.1, last.2), (cell("Width"), cell("LocPinY"))), "{what}: the last row is {last:?}, Width {}", cell("Width"));
    // And on the page they start and end on the glued points.
    let points = route(d, id);
    assert!(near_point(points[0], begin) && near_point(points[points.len() - 1], end), "{what}: route {points:?} between {begin:?} and {end:?}");
    if length > 1.0e-6 {
        if let Some(why) = flaw(&points) {
            panic!("{what}: {why}: {points:?}");
        }
    } else {
        // Ends that meet: a connector of no length, two equal points, the one route with a repeated vertex.
        assert_eq!(points.len(), 2, "{what}: {points:?}");
    }
}

/// Where the far shape's centre goes: across the near shape's own height and width, on each side of it and level with it, including the
/// places where two connection points line up or coincide.
fn places() -> Vec<P> {
    let xs = [-3.0, 0.0, 2.5, 3.5, 4.0, 4.5, 5.5, 8.0];
    let ys = [-2.0, 1.5, 3.5, 4.0, 4.5, 6.0, 9.0];
    xs.iter().flat_map(|x| ys.iter().map(move |y| (*x, *y))).collect()
}

/// Two rectangles, A (1) at (4, 4) and B (2) at (9, 4), joined from A's point `p` to B's point `q`.
fn joined(p: u32, q: u32) -> diacraft_engine::Session {
    let mut s = blank_session();
    assert_eq!(drop_shape(&mut s, "rectangle", 4.0, 4.0), 1);
    assert_eq!(drop_shape(&mut s, "rectangle", 9.0, 4.0), 2);
    let r = run(&mut s, "connector.add", json!({"from": {"id": 1, "point": p}, "to": {"id": 2, "point": q}}));
    assert_eq!(id_of(&r), 3);
    s
}

#[test]
fn a_connector_is_sound_after_its_far_shape_is_moved_to_any_side_of_the_near_one() {
    let mut checked = 0;
    for p in 0..4 {
        for q in 0..4 {
            let mut s = joined(p, q);
            assert_sound(doc(&s), 3, &format!("new {p}->{q}"));
            let mut at = (9.0, 4.0);
            for to in places() {
                run(&mut s, "shape.move", json!({"ids": [2], "dx": to.0 - at.0, "dy": to.1 - at.1}));
                at = to;
                assert_sound(doc(&s), 3, &format!("B of {p}->{q} moved to {to:?}"));
                checked += 1;
            }
            assert_eq!(check_session(&s), Ok(()));
        }
    }
    assert_eq!(checked, 16 * 56);
}

#[test]
fn a_connector_is_sound_after_its_near_shape_is_moved_to_any_side_of_the_far_one() {
    for p in 0..4 {
        for q in 0..4 {
            let mut s = joined(p, q);
            let mut at = (4.0, 4.0);
            // The far shape is at (9, 4), so these are the same places seen from the other end, moved in the reverse order.
            for to in places().into_iter().rev() {
                run(&mut s, "shape.move", json!({"ids": [1], "dx": to.0 - at.0, "dy": to.1 - at.1}));
                at = to;
                assert_sound(doc(&s), 3, &format!("A of {p}->{q} moved to {to:?}"));
            }
            assert_eq!(check_session(&s), Ok(()));
        }
    }
}

#[test]
fn both_ends_move_at_once_and_the_connector_stays_whole() {
    let mut s = joined(3, 1);
    for (dx, dy) in [(1.0, 0.0), (-3.0, 2.0), (0.0, -4.0), (5.0, 5.0), (-2.5, -3.0)] {
        run(&mut s, "shape.move", json!({"ids": [1, 2], "dx": dx, "dy": dy}));
        assert_sound(doc(&s), 3, &format!("both moved by ({dx}, {dy})"));
    }
}

#[test]
fn a_resize_keeps_the_connector_whole_too() {
    let mut s = joined(3, 1);
    for (x, y, w, h) in [(7.0, 3.0, 3.0, 2.0), (8.0, 3.5, 0.5, 1.0), (-1.0, 3.0, 2.0, 2.0), (3.0, 3.5, 1.5, 1.0)] {
        run(&mut s, "shape.resize", json!({"id": 2, "x": x, "y": y, "width": w, "height": h}));
        assert_sound(doc(&s), 3, &format!("B resized to ({x}, {y}) {w} by {h}"));
    }
}

// ---- the cases a render showed, by name -----------------------------------------------------------------------------------------------

/// The connector's route on the page, as (x, y) points, after B of `joined(3, 1)` (A's right to B's left) is moved by (`dx`, `dy`).
fn after_moving_b(dx: f64, dy: f64) -> Vec<P> {
    let mut s = joined(3, 1);
    run(&mut s, "shape.move", json!({"ids": [2], "dx": dx, "dy": dy}));
    assert_sound(doc(&s), 3, &format!("B moved by ({dx}, {dy})"));
    route(doc(&s), 3)
}

fn assert_points(got: &[P], want: &[P]) {
    assert!(got.len() == want.len() && got.iter().zip(want).all(|(g, w)| near_point(*g, *w)), "got {got:?}, want {want:?}");
}

#[test]
fn moved_right_or_along_the_line_the_route_is_one_straight_leg() {
    // A's right point is (4.75, 4); B's left was (8.25, 4).
    assert_points(&after_moving_b(2.0, 0.0), &[(4.75, 4.0), (10.25, 4.0)]);
    assert_points(&after_moving_b(-1.0, 0.0), &[(4.75, 4.0), (7.25, 4.0)]);
}

#[test]
fn moved_up_or_down_the_route_is_a_z_with_its_middle_leg_halfway() {
    assert_points(&after_moving_b(0.0, 3.0), &[(4.75, 4.0), (6.5, 4.0), (6.5, 7.0), (8.25, 7.0)]);
    assert_points(&after_moving_b(0.0, -3.0), &[(4.75, 4.0), (6.5, 4.0), (6.5, 1.0), (8.25, 1.0)]);
}

#[test]
fn moved_across_to_the_far_side_at_one_height_the_route_does_not_double_back() {
    // B's left point is now (0.25, 4), level with A's right point (4.75, 4) and behind it: out of both by the standoff, and around.
    let r = after_moving_b(-8.0, 0.0);
    assert!(near_point(r[0], (4.75, 4.0)) && near_point(r[r.len() - 1], (0.25, 4.0)), "{r:?}");
    assert!(near_point(r[1], (5.0, 4.0)) && near_point(r[r.len() - 2], (0.0, 4.0)), "it leaves to the right and arrives from the left: {r:?}");
    assert!(r.len() >= 4 && r.iter().any(|p| !near(p.1, 4.0)), "it goes off the line to get around: {r:?}");
}

#[test]
fn moved_across_and_up_or_down_the_route_is_an_s_through_the_middle_height() {
    assert_points(&after_moving_b(-8.0, 2.0), &[(4.75, 4.0), (5.0, 4.0), (5.0, 5.0), (0.0, 5.0), (0.0, 6.0), (0.25, 6.0)]);
    assert_points(&after_moving_b(-8.0, -2.0), &[(4.75, 4.0), (5.0, 4.0), (5.0, 3.0), (0.0, 3.0), (0.0, 2.0), (0.25, 2.0)]);
}

#[test]
fn two_points_that_face_the_same_way_at_one_height_take_a_hook_and_not_a_line_back() {
    // A's right (4.75, 4) and B's right (9.75, 4): both leave to the right, B's is ahead on the same line.
    let mut s = joined(3, 3);
    let r = route(doc(&s), 3);
    assert!(flaw(&r).is_none(), "{r:?}");
    assert!(near_point(r[0], (4.75, 4.0)) && near_point(r[r.len() - 1], (9.75, 4.0)), "{r:?}");
    // The arrowhead is on a leg of its own, which comes in from the right.
    let (last, before) = (r[r.len() - 1], r[r.len() - 2]);
    assert!(before.0 > last.0 + 0.1 && near(before.1, last.1), "{r:?}");
    // Moving B up one standoff's worth turns the hook into an ordinary C.
    run(&mut s, "shape.move", json!({"ids": [2], "dx": 0.0, "dy": 2.0}));
    assert_sound(doc(&s), 3, "B up");
    assert_points(&route(doc(&s), 3), &[(4.75, 4.0), (10.0, 4.0), (10.0, 6.0), (9.75, 6.0)]);
}

#[test]
fn ends_that_meet_are_a_connector_of_no_length_with_its_frame_intact() {
    // B's left point (8.25, 4) moved to A's right point (4.75, 4): 3.5 to the left.
    let r = after_moving_b(-3.5, 0.0);
    assert_points(&r, &[(4.75, 4.0), (4.75, 4.0)]);
    let mut s = joined(3, 1);
    run(&mut s, "shape.move", json!({"ids": [2], "dx": -3.5, "dy": 0.0}));
    assert_eq!(num(doc(&s), 3, "Width"), 0.0);
    // And the connector comes back to life when B moves away again.
    run(&mut s, "shape.move", json!({"ids": [2], "dx": 3.5, "dy": 2.0}));
    assert_sound(doc(&s), 3, "B moved away again");
    let r = route(doc(&s), 3);
    assert!(r.len() >= 3 && flaw(&r).is_none(), "{r:?}");
}

#[test]
fn the_pose_of_the_connector_carries_its_rows_to_the_glued_points_for_every_angle() {
    // The rows are in the connector's own frame, which turns with `Angle`: a check of `Pose` against the rows, for a connector at each of
    // the eight directions from its begin.
    for (dx, dy) in [(5.0, 0.0), (5.0, 5.0), (0.0, 5.0), (-5.0, 5.0), (-5.0, 0.0), (-5.0, -5.0), (0.0, -5.0), (5.0, -5.0)] {
        let mut s = joined(0, 0);
        run(&mut s, "shape.move", json!({"ids": [2], "dx": dx - 5.0, "dy": dy}));
        let d = doc(&s);
        assert_sound(d, 3, &format!("direction ({dx}, {dy})"));
        let pose = Pose {
            pin: (num(d, 3, "PinX"), num(d, 3, "PinY")),
            loc: (num(d, 3, "LocPinX"), num(d, 3, "LocPinY")),
            angle: num(d, 3, "Angle"),
            flip_x: false,
            flip_y: false,
        };
        let (begin, end) = anchors(d, 3);
        assert!(near_point(pose.up((0.0, num(d, 3, "LocPinY"))), begin) && near_point(pose.up((num(d, 3, "Width"), num(d, 3, "LocPinY"))), end));
    }
}
