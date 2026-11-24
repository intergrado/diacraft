//! A connector glued to a connection point of a shape that was flipped or turned leaves the point in the direction the point
//! faces, and goes round the shape it is glued to, by the shorter way, and not through it.
//!
//! The screenshot that raised it (the second slice's 12): a flowchart shape was flipped (Ctrl+H), the flip moved the connection point the
//! connector was glued to from one side of the shape to the other, and the route's fixed detour ("above or to the right") carried the line across
//! the shape. These tests use the built-in masters, the flips and turns the commands make, and the connection points' own directions.

use diacraft_doc::Document;
use diacraft_engine::draw::{connection_points, shape_corners};
use serde_json::json;

use crate::edit::{doc, drop_shape, id_of, route};
use crate::{blank_session, check_session, run};

type P = (f64, f64);

/// Whether `p` is inside the convex quadrilateral by more than `margin`.
fn strictly_inside(quad: &[P; 4], p: P, margin: f64) -> bool {
    let area: f64 = (0..4).map(|i| quad[i].0 * quad[(i + 1) % 4].1 - quad[(i + 1) % 4].0 * quad[i].1).sum();
    let sign = if area >= 0.0 { 1.0 } else { -1.0 };
    (0..4).all(|i| {
        let (a, b) = (quad[i], quad[(i + 1) % 4]);
        let length = (b.0 - a.0).hypot(b.1 - a.1);
        let cross = (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0);
        sign * cross / length > margin
    })
}

/// A leg of the route that goes into the box, if one does: found by walking each leg in small steps, without the code under test.
fn enters(route: &[P], quad: &[P; 4]) -> Option<(P, P)> {
    for w in route.windows(2) {
        let (a, b) = (w[0], w[1]);
        let steps = 2000;
        for i in 0..=steps {
            let t = f64::from(i) / f64::from(steps);
            if strictly_inside(quad, (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t), 1.0e-3) {
                return Some((a, b));
            }
        }
    }
    None
}

fn length(route: &[P]) -> f64 {
    route.windows(2).map(|w| (w[1].0 - w[0].0).abs() + (w[1].1 - w[0].1).abs()).sum()
}

/// The axis direction nearest a vector.
fn snap(v: P) -> P {
    if v.0.abs() >= v.1.abs() { (v.0.signum(), 0.0) } else { (0.0, v.1.signum()) }
}

fn unit(a: P, b: P) -> P {
    let d = (b.0 - a.0, b.1 - a.1);
    let n = d.0.hypot(d.1);
    (d.0 / n, d.1 / n)
}

fn same_way(a: P, b: P) -> bool {
    (a.0 - b.0).abs() < 1.0e-6 && (a.1 - b.1).abs() < 1.0e-6
}

fn corners_of(d: &Document, id: u32) -> [P; 4] {
    shape_corners(d, 1, id).unwrap()
}

fn point_of(d: &Document, id: u32, index: u32) -> (P, P) {
    let p = connection_points(d, 1, id).into_iter().find(|p| p.index == index).unwrap();
    ((p.x, p.y), snap((p.dir_x, p.dir_y)))
}

/// Two rectangles, A at (`a`) and B at (`b`), joined from A's point `p` to B's point `q`.
fn joined(a: P, b: P, p: u32, q: u32) -> (diacraft_engine::Session, u32, u32, u32) {
    let mut s = blank_session();
    let ia = drop_shape(&mut s, "rectangle", a.0, a.1);
    let ib = drop_shape(&mut s, "rectangle", b.0, b.1);
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"id": ia, "point": p}, "to": {"id": ib, "point": q}})));
    (s, ia, ib, c)
}

/// What a route must be: from the begin point to the end point, out along the begin point's direction and in against the end point's, flat of
/// flaws, and not through the box of either shape.
fn assert_clear(d: &Document, a: u32, p: u32, b: u32, q: u32, c: u32, what: &str) {
    let r = route(d, c);
    let ((begin, begin_dir), (end, end_dir)) = (point_of(d, a, p), point_of(d, b, q));
    assert!(same_way(r[0], begin) && same_way(r[r.len() - 1], end), "{what}: the route {r:?} does not run from {begin:?} to {end:?}");
    assert!(same_way(snap(unit(r[0], r[1])), begin_dir), "{what}: the route {r:?} does not leave along {begin_dir:?}");
    let last = (r[r.len() - 2], r[r.len() - 1]);
    assert!(same_way(snap(unit(last.1, last.0)), end_dir), "{what}: the route {r:?} does not arrive along {end_dir:?}");
    for (id, name) in [(a, "the begin shape"), (b, "the end shape")] {
        if let Some(leg) = enters(&r, &corners_of(d, id)) {
            panic!("{what}: the route {r:?} goes through {name}, on the leg {leg:?}");
        }
    }
    for w in r.windows(2) {
        assert!((w[0].0 - w[1].0).abs() < 1.0e-6 || (w[0].1 - w[1].1).abs() < 1.0e-6, "{what}: a slanted leg in {r:?}");
        assert!(length(w) > 1.0e-6, "{what}: a zero-length leg in {r:?}");
    }
}

#[test]
fn a_flipped_target_is_gone_round_and_not_through() {
    // A's right point (3.75, 4) to B's left point (8.25, 4); a flip carries B's left point to B's right side, (9.75, 4), facing right.
    let (mut s, a, b, c) = joined((3.0, 4.0), (9.0, 4.0), 3, 1);
    assert_eq!(route(doc(&s), c), vec![(3.75, 4.0), (8.25, 4.0)]);
    run(&mut s, "shape.flip", json!({"ids": [b], "axis": "horizontal"}));
    let r = route(doc(&s), c);
    assert_eq!(point_of(doc(&s), b, 1), ((9.75, 4.0), (1.0, 0.0)), "the flip moved the point to the far side, facing out of it");
    assert_clear(doc(&s), a, 3, b, 1, c, "after the flip");
    assert!(r.len() >= 5, "the route goes round: {r:?}");
    // Round the top or the bottom of B (level with it, so by a standoff past its edge), by one standoff past each end.
    assert!(r.iter().any(|p| (p.1 - 4.75).abs() < 1.0e-6 || (p.1 - 3.25).abs() < 1.0e-6), "{r:?}");
    assert!((length(&r) - 8.0).abs() < 1.0e-6, "the shortest way round is 8 inches long: {r:?}");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn the_shorter_way_round_is_taken_whichever_side_it_is() {
    // The begin is a little above the middle of B's height, so the way over the top is shorter than the one under.
    let (mut s, a, b, c) = joined((3.0, 4.2), (9.0, 4.0), 3, 1);
    run(&mut s, "shape.flip", json!({"ids": [b], "axis": "horizontal"}));
    assert_clear(doc(&s), a, 3, b, 1, c, "over");
    let over = route(doc(&s), c);
    assert!(over.iter().any(|p| p.1 > 4.5), "it goes over B: {over:?}");
    // And lower than the middle, under it.
    let (mut s, a, b, c) = joined((3.0, 3.8), (9.0, 4.0), 3, 1);
    run(&mut s, "shape.flip", json!({"ids": [b], "axis": "horizontal"}));
    assert_clear(doc(&s), a, 3, b, 1, c, "under");
    let under = route(doc(&s), c);
    assert!(under.iter().any(|p| p.1 < 3.5), "it goes under B: {under:?}");
    assert!(length(&under) < 8.0 + 1.0e-6, "{under:?}");
}

#[test]
fn every_flip_and_turn_of_either_shape_leaves_by_the_point_and_goes_round_and_not_through() {
    let steps: [(&str, &[(&str, serde_json::Value)]); 9] = [
        ("flip horizontally", &[("shape.flip", json!({"axis": "horizontal"}))]),
        ("flip vertically", &[("shape.flip", json!({"axis": "vertical"}))]),
        ("turn 90", &[("shape.rotate", json!({"by": 90}))]),
        ("turn 180", &[("shape.rotate", json!({"by": 180}))]),
        ("turn 270", &[("shape.rotate", json!({"by": 270}))]),
        ("turn 90 and flip", &[("shape.rotate", json!({"by": 90})), ("shape.flip", json!({"axis": "horizontal"}))]),
        ("turn 180 and flip", &[("shape.rotate", json!({"by": 180})), ("shape.flip", json!({"axis": "vertical"}))]),
        ("flip both ways", &[("shape.flip", json!({"axis": "horizontal"})), ("shape.flip", json!({"axis": "vertical"}))]),
        ("turn 30", &[("shape.rotate", json!({"by": 30}))]),
    ];
    let mut checked = 0;
    for (what, commands) in steps {
        for (p, q) in [(3, 1), (3, 3), (1, 1), (0, 2), (2, 0), (3, 0)] {
            for (a_at, b_at) in [((3.0, 4.0), (9.0, 4.0)), ((3.0, 4.0), (9.0, 6.5)), ((3.0, 4.0), (9.0, 1.5)), ((9.0, 4.0), (3.0, 4.0))] {
                for turn_a in [false, true] {
                    let (mut s, a, b, c) = joined(a_at, b_at, p, q);
                    let who = if turn_a { a } else { b };
                    for (command, params) in commands {
                        let mut params = params.clone();
                        params["ids"] = json!([who]);
                        run(&mut s, command, params);
                    }
                    let d = doc(&s);
                    // A 30 degree turn leaves a direction between two axes, which the route snaps to one: only the end points are held to it.
                    if what != "turn 30" {
                        assert_clear(
                            d,
                            a,
                            p,
                            b,
                            q,
                            c,
                            &format!("{what} of {} (points {p}, {q}, at {a_at:?}, {b_at:?})", if turn_a { "A" } else { "B" }),
                        );
                    }
                    checked += 1;
                }
            }
        }
    }
    assert_eq!(checked, 9 * 6 * 4 * 2);
}
