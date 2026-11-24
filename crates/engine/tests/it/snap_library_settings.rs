//! The library's new settings (R3 places on a locked move's line of travel, R15 places that stop an arrow key, R28 a line mark to the nearest shape on each side)
//! and the F1 re-review's items; the helpers `snap_turned_sides.rs` (row 3) shares. Each test names the spec's R and V number it pins. The settings are the spec's "Settings for vectors" (see `snap_adoption.rs`): the Snap and Glue switches, guides and the
//! dynamic grid on, the grid and the ruler off, a 20 by 20 inch page, 96 screen points to an inch; every shape a plain rectangle with a visible line.

use diacraft_doc::{Document, ShapeType};
use diacraft_engine::Session;
use diacraft_engine::draw::{connection_points, drop_shape, shape_corners};
use diacraft_engine::snap::{Rect, box_of};
use diacraft_engine::stencil::find_master;
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};
use serde_json::{Value, json};

use crate::{blank_session, run};

// ---------------------------------------------------------------------------------------------------------------- helpers

/// A plain rectangle `w` by `h` inches with its pin in its middle at `pin`, turned `degrees` (counterclockwise).
pub fn turned(id: u32, pin: (f64, f64), w: f64, h: f64, degrees: f64) -> ShapeBuilder {
    ShapeBuilder::new(id)
        .cell("PinX", &pin.0.to_string())
        .cell("PinY", &pin.1.to_string())
        .cell("Width", &w.to_string())
        .cell("Height", &h.to_string())
        .cell("LocPinX", &(w / 2.0).to_string())
        .cell("LocPinY", &(h / 2.0).to_string())
        .cell("Angle", &degrees.to_radians().to_string())
}

/// A rectangle from (`x0`, `y0`) to (`x1`, `y1`).
pub fn rect(id: u32, x0: f64, y0: f64, x1: f64, y1: f64) -> ShapeBuilder {
    turned(id, ((x0 + x1) / 2.0, (y0 + y1) / 2.0), x1 - x0, y1 - y0, 0.0)
}

/// A guide (the line through `pin` along `degrees`), which is a page shape of kind `Guide`.
pub fn guide_at(id: u32, pin: (f64, f64), degrees: f64) -> ShapeBuilder {
    ShapeBuilder::new(id)
        .kind(ShapeType::Guide)
        .cell("PinX", &pin.0.to_string())
        .cell("PinY", &pin.1.to_string())
        .cell("Width", "0")
        .cell("Height", "0")
        .cell("LocPinX", "0")
        .cell("LocPinY", "0")
        .cell("Angle", &degrees.to_radians().to_string())
}

pub fn page_of_shapes(shapes: Vec<ShapeBuilder>) -> Document {
    let mut page = PageBuilder::new().cell("PageWidth", "20").cell("PageHeight", "20");
    for shape in shapes {
        page = page.shape(shape);
    }
    DocBuilder::new().page(1, page).build()
}

/// A session holding `doc`, with the grid and the ruler off.
pub fn session_of(doc: Document) -> Session {
    let mut s = blank_session();
    s.edit("build", |d| {
        *d = doc;
        Ok(())
    })
    .unwrap();
    run(&mut s, "snap.set", json!({"grid": false, "ruler": false}));
    s
}

/// The point at `x` along the own x axis and `y` along the own y axis of a frame turned `degrees`, from `p`.
pub fn at(p: (f64, f64), degrees: f64, x: f64, y: f64) -> (f64, f64) {
    let (s, c) = degrees.to_radians().sin_cos();
    (p.0 + x * c - y * s, p.1 + x * s + y * c)
}

pub fn corners(s: &Session, id: u32) -> [(f64, f64); 4] {
    shape_corners(s.document().unwrap(), 1, id).unwrap()
}

/// The middle of a shape's box (its pin, for the shapes of these tests).
pub fn middle(s: &Session, id: u32) -> (f64, f64) {
    let c = corners(s, id);
    ((c[0].0 + c[2].0) / 2.0, (c[0].1 + c[2].1) / 2.0)
}

pub fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

pub fn near2(a: (f64, f64), b: (f64, f64), tol: f64) -> bool {
    close(a.0, b.0, tol) && close(a.1, b.1, tol)
}

pub fn marks_of<'a>(r: &'a Value, kind: &str) -> Vec<&'a Value> {
    r["snap"]["marks"].as_array().map(|m| m.iter().filter(|m| m["mark"] == json!(kind)).collect()).unwrap_or_default()
}

pub fn box_in(s: &Session, id: u32) -> Rect {
    box_of(s.document().unwrap(), 1, &[id]).unwrap()
}

// ---------------------------------------------------------------------------------------------------------------- R28, a line mark to the nearest shape on each side

#[test]
fn a_line_mark_runs_from_the_nearest_shape_on_one_side_to_the_nearest_on_the_other() {
    // A (2, 2) to (3, 3) and B (8, 2) to (9, 3), and a 1 by 1 shape dragged between them to (4.5, 2.03): it is 1.5 inches from A and 2.5 from B. Its top, middle and bottom
    // lines are marked, each from A's near end to B's far end: the line runs to the nearest shape on each side (the 2009 evidence's second image), not to the nearer only (2 to 5.5).
    let mut s = session_of(page_of_shapes(vec![rect(1, 2.0, 2.0, 3.0, 3.0), rect(2, 8.0, 2.0, 9.0, 3.0), rect(3, 4.5, 2.03, 5.5, 3.03)]));
    let r = run(&mut s, "snap.at", json!({"page": 1, "left": 4.5, "bottom": 2.03, "right": 5.5, "top": 3.03, "ids": [3]}));
    let lines: Vec<&Value> = r["marks"].as_array().unwrap().iter().filter(|m| m["mark"] == json!("line") && m["axis"] == json!("y")).collect();
    assert_eq!(lines.len(), 3, "{r}");
    for m in lines {
        assert_eq!(m["count"], json!(2), "both shapes are on the line: {m}");
        let (a, b) = (m["from"][0].as_f64().unwrap(), m["to"][0].as_f64().unwrap());
        assert!(close(a.min(b), 2.0, 1e-9) && close(a.max(b), 9.0, 1e-9), "the line runs from A to B: {m}");
    }
}

#[test]
fn a_line_with_shapes_on_one_side_only_runs_to_the_nearest_of_them_and_no_farther() {
    // A (2, 2) to (3, 3), B (4, 2) to (5, 3): a third shape to the right of both, dragged to (6.5, 2.03), runs its lines to B and not to A (the second case: from 4 to 7.5).
    let mut s = session_of(page_of_shapes(vec![rect(1, 2.0, 2.0, 3.0, 3.0), rect(2, 4.0, 2.0, 5.0, 3.0), rect(3, 6.5, 2.03, 7.5, 3.03)]));
    let r = run(&mut s, "snap.at", json!({"page": 1, "left": 6.5, "bottom": 2.03, "right": 7.5, "top": 3.03, "ids": [3]}));
    let lines: Vec<&Value> = r["marks"].as_array().unwrap().iter().filter(|m| m["mark"] == json!("line") && m["axis"] == json!("y")).collect();
    assert_eq!(lines.len(), 3, "{r}");
    for m in lines {
        let (a, b) = (m["from"][0].as_f64().unwrap(), m["to"][0].as_f64().unwrap());
        assert!(close(a.min(b), 4.0, 1e-9) && close(a.max(b), 7.5, 1e-9) && m["count"] == json!(2), "{m}");
    }
}

// ---------------------------------------------------------------------------------------------------------------- R3, places on the line a locked move travels along

/// A page with shape 1 at (2, 2) to (3, 3) and a rectangle 2 at (6, 2.5) to (7, 3.5) whose handles are places: its left side middle (6, 3) is on the line y = 3 that the box's top
/// places travel along; its corner (6, 2.5) is on y = 2.5, the line of the box's middle places. With the handles on.
fn handles_page(raise: f64) -> Session {
    let mut s = session_of(page_of_shapes(vec![rect(1, 2.0, 2.0, 3.0, 3.0), rect(2, 6.0, 2.5 + raise, 7.0, 3.5 + raise)]));
    run(&mut s, "snap.set", json!({"snap_handles": true}));
    s
}

#[test]
fn a_place_on_the_line_a_locked_move_travels_along_pulls_it_and_the_box_lands_on_the_place() {
    // The box moved 2.97 right and locked to x: its top right corner (5.97, 3) is 0.03 inch short of the rectangle's left side middle (6, 3), which is on the line the corner
    // travels along. A place pulls a locked move there ("a place target pulls a locked move only where it lies on the line of travel"), the corner lands on it, and the
    // other axis did not move, to the bit.
    let mut s = handles_page(0.0);
    let r = run(&mut s, "shape.move", json!({"ids": [1], "dx": 2.97, "dy": 0.3, "snap": {"lock": "x"}}));
    let landed = box_in(&s, 1);
    assert!(
        close(landed.x1, 6.0, 1e-9) && landed.y0 == 2.0 && landed.y1 == 3.0,
        "the corner is on the place, the other axis did not move: {landed:?} {r}"
    );
    let hit = &r["snap"]["hits"][0];
    assert_eq!((hit["target"].clone(), hit["shape"].clone()), (json!("handle"), json!(2)), "{r}");
}

#[test]
fn a_place_off_the_line_of_travel_pulls_a_locked_move_not_at_all() {
    // The rectangle raised 0.3 inch: its places are not on any line the box's places travel along, and a place that is only near the line is not met (the subject could land
    // only at its foot, with nothing true to draw). The move goes as asked.
    let mut s = handles_page(0.3);
    let r = run(&mut s, "shape.move", json!({"ids": [1], "dx": 2.97, "dy": 0.3, "snap": {"lock": "x"}}));
    let landed = box_in(&s, 1);
    assert!(close(landed.x1, 5.97, 1e-9) && landed.y0 == 2.0, "{landed:?} {r}");
    assert_eq!(r["snap"]["hits"], json!([]), "{r}");
}

#[test]
fn a_locked_move_meets_a_connection_point_on_its_line_and_snap_at_says_the_same() {
    // The stencil rectangle's connection points are places of the page. The rectangle is dropped where its first connection point is on y = 2.5, the line the middle places of the
    // box (2, 2) to (3, 3) travel along when the move is held to x. Dragged to 0.03 inch short of the point, the box's right middle place is pulled onto it, by the command and
    // by `snap.at` with the same words. Only the connection points are on.
    let mut doc = page_of_shapes(vec![rect(1, 2.0, 2.0, 3.0, 3.0)]);
    let probe = drop_shape(&mut doc, 1, find_master("rectangle").unwrap(), 8.0, 8.0).unwrap();
    let first = connection_points(&doc, 1, probe.id).into_iter().find(|p| p.index == 0).expect("a connection point");
    let to_y = first.y - 8.0;
    let mut doc = page_of_shapes(vec![rect(1, 2.0, 2.0, 3.0, 3.0)]);
    let target = drop_shape(&mut doc, 1, find_master("rectangle").unwrap(), 9.0, 2.5 - to_y).unwrap();
    let point = connection_points(&doc, 1, target.id).into_iter().find(|p| p.index == 0).unwrap();
    assert!(close(point.y, 2.5, 1e-9), "{point:?}");
    let mut s = session_of(doc);
    run(
        &mut s,
        "snap.set",
        json!({"snap_connection_points": true, "snap_handles": false, "snap_vertices": false, "snap_geometry": false, "intersections": false, "dynamic_grid": false, "alignment_box": false}),
    );
    let dx = point.x - 0.03 - 3.0;
    let asked = run(&mut s, "snap.at", json!({"page": 1, "left": 2.0 + dx, "bottom": 2.3, "right": 3.0 + dx, "top": 3.3, "ids": [1], "lock": "x"}));
    // `snap.at` moves from the box it is given: the box was raised 0.3 and the lock holds it where it is asked, so the place is on y = 2.8 for the middle: not on the line.
    assert_eq!(asked["hits"], json!([]), "a box whose lines of travel miss the point: {asked}");
    let asked = run(&mut s, "snap.at", json!({"page": 1, "left": 2.0 + dx, "bottom": 2.0, "right": 3.0 + dx, "top": 3.0, "ids": [1], "lock": "x"}));
    assert!(close(asked["dx"].as_f64().unwrap(), 0.03, 1e-9) && asked["dy"] == json!(0.0), "{asked}");
    assert_eq!(asked["hits"][0]["target"], json!("connection_point"), "{asked}");
    let moved = run(&mut s, "shape.move", json!({"ids": [1], "dx": dx, "dy": 0.3, "snap": {"lock": "x"}}));
    let landed = box_in(&s, 1);
    assert!(close(landed.x1, point.x, 1e-9) && landed.y0 == 2.0, "{landed:?} against the point {point:?}: {moved}");
    assert_eq!(moved["snap"]["hits"][0]["target"], json!("connection_point"), "{moved}");
}

// ---------------------------------------------------------------------------------------------------------------- R15, places that stop an arrow key

/// One press of an arrow key on shape 1, `tick` inches when nothing is ahead.
fn press(s: &mut Session, dx: f64, dy: f64) -> Value {
    run(s, "shape.move", json!({"ids": [1], "dx": dx, "dy": dy, "snap": {"zoom": 96.0, "nudge": {"tick": 0.125}}}))
}

#[test]
fn a_place_on_the_line_an_arrow_key_travels_along_stops_it_before_the_next_like_line() {
    // The box (1, 1) to (2, 2) and a rectangle (3, 0.5) to (4, 1.5) whose left side middle (3, 1) is a handle on the line y = 1 the box's bottom places travel along. The next
    // like lines are 2 inches ahead (the rectangle's left, middle and right lines against the box's own); the place is 1 inch ahead (the box's bottom right corner onto it): the
    // key stops at the nearest stop whatever its kind, 1 inch.
    let mut s = session_of(page_of_shapes(vec![rect(1, 1.0, 1.0, 2.0, 2.0), rect(2, 3.0, 0.5, 4.0, 1.5)]));
    run(&mut s, "snap.set", json!({"snap_handles": true}));
    let r = press(&mut s, 1.0, 0.0);
    let b = box_in(&s, 1);
    assert!(close(b.x0, 2.0, 1e-9), "{b:?} {r}");
    assert_eq!(r["snap"]["hits"][0]["target"], json!("handle"), "{r}");
}

#[test]
fn the_nearest_place_ahead_stops_the_key_whatever_its_rank_and_a_better_ranked_one_farther_on_does_not() {
    // The engine ranks a connection point above a handle (`Choose::Ranked`). Ahead of the box (1, 1) to (2, 2): a handle 1 inch ahead (the rectangle's left side
    // middle on y = 1), and the stencil rectangle's connection point 3 inches ahead on y = 1.5. A key goes to the nearest stop and priority only breaks a tie: 1 inch.
    let mut doc = page_of_shapes(vec![rect(1, 1.0, 1.0, 2.0, 2.0), rect(2, 3.0, 0.5, 4.0, 1.5)]);
    let far = drop_shape(&mut doc, 1, find_master("rectangle").unwrap(), 9.0, 9.0).unwrap();
    let point = connection_points(&doc, 1, far.id).into_iter().find(|p| p.index == 0).unwrap();
    // Put the stencil rectangle where its first connection point is on y = 1.5 and 5 inches from x = 0, so that the box's right middle place reaches it after 3 inches.
    let (dx, dy) = (5.0 - point.x, 1.5 - point.y);
    let mut s = session_of(doc);
    run(&mut s, "shape.move", json!({"ids": [far.id], "dx": dx, "dy": dy}));
    // Only places stop it: no like line or equal gap is in the way.
    run(&mut s, "snap.set", json!({"snap_handles": true, "snap_connection_points": true, "dynamic_grid": false, "alignment_box": false}));
    let r = press(&mut s, 1.0, 0.0);
    let b = box_in(&s, 1);
    assert!(close(b.x0, 2.0, 1e-9), "{b:?} {r}");
    assert_eq!(r["snap"]["hits"][0]["target"], json!("handle"), "{r}");
}

// ---------------------------------------------------------------------------------------------------------------- the F1 re-review's items

/// What `snap.at` says of `corner_boxes` for a box that is the box of the shapes `ids`.
fn corner_boxes_of(s: &mut Session, ids: &[u32]) -> Value {
    let b = diacraft_engine::snap::bounds_of(s.document().unwrap(), 1, ids).unwrap().rect;
    let r = run(s, "snap.at", json!({"page": 1, "left": b.x0, "bottom": b.y0, "right": b.x1, "top": b.y1, "ids": ids}));
    r["corner_boxes"].clone()
}

#[test]
fn snap_at_tells_how_many_of_the_shapes_it_aligned_by_their_corners_as_shape_move_and_shape_place_do() {
    // `snap.at` sets `corner_boxes` as the commands do (the F1 re-review). A plain rectangle turned 30 degrees draws no path, so the box round its paths is no box and the corners'
    // is kept: counted. A shape that is not turned is not counted, and of two shapes named one that is turned is.
    let mut s = session_of(page_of_shapes(vec![turned(1, (5.0, 5.0), 2.0, 1.0, 30.0), rect(2, 9.0, 9.0, 10.0, 10.0), rect(3, 12.0, 3.0, 13.0, 4.0)]));
    assert_eq!(corner_boxes_of(&mut s, &[1]), json!(1));
    assert_eq!(corner_boxes_of(&mut s, &[2]), json!(0));
    assert_eq!(corner_boxes_of(&mut s, &[1, 2]), json!(1));
    // The same number as `shape.move` and `shape.place` tell of the same shape.
    let moved = run(&mut s, "shape.move", json!({"ids": [1], "dx": 0.0, "dy": 0.0, "snap": {"zoom": 96.0}}));
    assert_eq!(moved["snap"]["corner_boxes"], json!(1), "{moved}");
    let placed = run(&mut s, "shape.place", json!({"ids": [1], "y": 5.0, "snap": {"zoom": 96.0}}));
    assert_eq!(placed["snap"]["corner_boxes"], json!(1), "{placed}");
    // With no shape named there is no box of shapes: none counted.
    let r = run(&mut s, "snap.at", json!({"page": 1, "left": 1.0, "bottom": 1.0, "right": 2.0, "top": 2.0}));
    assert_eq!(r["corner_boxes"], json!(0), "{r}");
}

#[test]
fn snap_at_refuses_connect_beside_a_box_edges_nudge_keep_and_rotate_in_words_and_takes_it_with_a_place() {
    // `connect: true` is the end of a connector, a place. Beside a box, `edges`, `nudge`, `keep` or `rotate` it did nothing that was asked (the glue targets are the places of a
    // connector's end and not of a box; a resize, a press and a turn have no end): it is refused in words (the F1 re-review). With a place it is what it was, with a lock too.
    let mut s = session_of(page_of_shapes(vec![rect(1, 2.0, 2.0, 3.0, 3.0)]));
    let the_box = json!({"left": 4.0, "bottom": 4.0, "right": 5.0, "top": 5.0});
    for extra in [
        the_box.clone(),
        json!({"left": 4.0, "bottom": 4.0, "right": 5.0, "top": 5.0, "edges": ["right"]}),
        json!({"left": 4.0, "bottom": 4.0, "right": 5.0, "top": 5.0, "nudge": {"direction": "right"}}),
        json!({"left": 4.0, "bottom": 4.0, "right": 5.0, "top": 5.0, "edges": ["right"], "keep": "proportions", "ids": [1]}),
        json!({"rotate": {"angle": 10.0, "radius": 1.0}, "ids": [1]}),
    ] {
        let mut p = json!({"page": 1, "connect": true});
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        let err = s.execute("snap.at", &p).unwrap_err();
        assert!(err.to_string().contains("`connect` is for a place"), "{extra}: {err}");
        // Without `connect` the same words are what they were.
        let mut q = p.clone();
        q.as_object_mut().unwrap().remove("connect");
        assert!(s.execute("snap.at", &q).is_ok(), "{extra}");
        // And `connect: false` is no word at all.
        q.as_object_mut().unwrap().insert("connect".into(), json!(false));
        assert!(s.execute("snap.at", &q).is_ok(), "{extra}");
    }
    // Each word alone, with no box to shadow it, is refused as the `connect` it does not go with (`edges`, `nudge` and `keep` are refused by their own rules without it).
    for word in [json!({"edges": ["right"]}), json!({"nudge": {"direction": "right"}}), json!({"keep": "proportions", "ids": [1]})] {
        let mut p = json!({"page": 1, "connect": true});
        p.as_object_mut().unwrap().extend(word.as_object().unwrap().clone());
        let err = s.execute("snap.at", &p).unwrap_err();
        assert!(err.to_string().contains("`connect` is for a place"), "{word}: {err}");
    }
    // A place is the end of a connector, with a lock or without.
    for place in [json!({"x": 2.97, "y": 2.5}), json!({"x": 2.97, "y": 2.5, "lock": "x"})] {
        let mut p = json!({"page": 1, "connect": true});
        p.as_object_mut().unwrap().extend(place.as_object().unwrap().clone());
        assert!(s.execute("snap.at", &p).is_ok(), "{place}");
    }
}

#[test]
fn snap_at_with_connect_is_the_end_of_a_connector_and_tells_what_it_glues_to() {
    // The end of a connector given to `snap.at` is a place that glues to a connection point within reach: the glue is told with `connect` and is none without it.
    let mut doc = page_of_shapes(vec![rect(1, 2.0, 2.0, 3.0, 3.0)]);
    let target = drop_shape(&mut doc, 1, find_master("rectangle").unwrap(), 8.0, 8.0).unwrap();
    let p = connection_points(&doc, 1, target.id).into_iter().find(|p| p.index == 0).unwrap();
    let mut s = session_of(doc);
    let glued = run(&mut s, "snap.at", json!({"page": 1, "x": p.x + 0.03, "y": p.y, "connect": true}));
    assert_eq!(
        (glued["glue"]["kind"].clone(), glued["glue"]["shape"].clone(), glued["glue"]["point"].clone()),
        (json!("connection_point"), json!(target.id), json!(p.index)),
        "{glued}"
    );
    let placed = run(&mut s, "snap.at", json!({"page": 1, "x": p.x + 0.03, "y": p.y}));
    assert_eq!(placed["glue"], Value::Null, "{placed}");
}

#[test]
fn a_resize_to_no_width_or_no_height_is_refused_as_before_and_a_snap_does_not_give_it_a_size() {
    // A resize with a width or a height of 0 is the edit's refusal, snapped or not: a guide 0.02 inch from the right side would have given the box a width of 0.02, and the
    // snap is not asked (a turned or flipped shape included).
    for shape in [rect(1, 2.0, 2.0, 3.0, 3.0), turned(1, (2.5, 2.5), 1.0, 1.0, 30.0), turned(1, (2.5, 2.5), 1.0, 1.0, 0.0).cell("FlipX", "1")] {
        let mut s = session_of(page_of_shapes(vec![shape]));
        run(&mut s, "guide.add", json!({"page": 1, "orientation": "vertical", "at": 2.02}));
        run(&mut s, "guide.add", json!({"page": 1, "orientation": "horizontal", "at": 2.02}));
        let c0 = shape_corners(s.document().unwrap(), 1, 1).unwrap()[0];
        for (width, height) in [(0.0, 1.0), (1.0, 0.0)] {
            let err = s
                .execute("shape.resize", &json!({"id": 1, "x": c0.0, "y": c0.1, "width": width, "height": height, "snap": {"zoom": 96.0}}))
                .unwrap_err();
            assert!(err.to_string().contains("more than 0"), "{width} by {height}: {err}");
        }
    }
}
