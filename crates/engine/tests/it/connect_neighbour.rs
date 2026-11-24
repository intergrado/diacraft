//! The neighbour an AutoConnect arrow joins, `connector.neighbour`, and the two ways `connector.autoconnect` joins a shape that is
//! already on the page: `existing` (a click on an arrow: the nearest neighbour in its direction, else a copy as before) and `to` (a drag from an arrow onto a shape).

use diacraft_doc::Document;
use diacraft_engine::draw::route::Dir;
use diacraft_engine::draw::{neighbour, shape_corners};
use serde_json::json;

use crate::edit::{doc, drop_shape};
use crate::{blank_session, check_session, run, snapshot};

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1.0e-6
}

fn shapes(d: &Document) -> usize {
    d.page(1).unwrap().shapes.len()
}

fn centre(d: &Document, id: u32) -> (f64, f64) {
    let c = shape_corners(d, 1, id).unwrap();
    (c.iter().map(|p| p.0).sum::<f64>() / 4.0, c.iter().map(|p| p.1).sum::<f64>() / 4.0)
}

/// Rectangles are 1.5 by 1 inch. Three in a row at the height of 4, 3 inches apart between centres: a gap of 1.5.
fn row() -> (diacraft_engine::Session, u32, u32, u32) {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    let b = drop_shape(&mut s, "rectangle", 6.0, 4.0);
    let c = drop_shape(&mut s, "rectangle", 9.0, 4.0);
    (s, a, b, c)
}

#[test]
fn the_neighbour_is_the_nearest_shape_beyond_the_side_whose_extent_overlaps_the_shapes() {
    let (s, a, b, c) = row();
    let d = doc(&s);
    assert_eq!(neighbour(d, 1, a, Dir::Right), Some(b), "the nearest of the two on the right");
    assert_eq!(neighbour(d, 1, b, Dir::Right), Some(c));
    assert_eq!(neighbour(d, 1, b, Dir::Left), Some(a));
    assert_eq!(neighbour(d, 1, c, Dir::Right), None, "nothing beyond the last");
    assert_eq!(neighbour(d, 1, a, Dir::Left), None);
    assert_eq!(neighbour(d, 1, a, Dir::Up), None, "a row has nothing above or below");
    assert_eq!(neighbour(d, 1, a, Dir::Down), None);
}

#[test]
fn a_shape_out_of_the_lane_is_no_neighbour_and_one_that_overlaps_it_by_a_little_is() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    // Above the lane of a (4 +- 0.5): its lowest edge is 5.5, so it shares no height with a.
    let off = drop_shape(&mut s, "rectangle", 6.0, 6.0);
    assert_eq!(neighbour(doc(&s), 1, a, Dir::Right), None, "a diagonal shape is not on the right");
    // Overlapping a's lane by 0.1 inch (its bottom edge is 4.4, a's top edge is 4.5).
    let partly = drop_shape(&mut s, "rectangle", 7.0, 4.9);
    assert_eq!(neighbour(doc(&s), 1, a, Dir::Right), Some(partly));
    // Touching the lane by an edge only (bottom edge 4.5): not overlapping.
    let touching = drop_shape(&mut s, "rectangle", 5.0, 5.0);
    assert_eq!(neighbour(doc(&s), 1, a, Dir::Right), Some(partly), "an edge shared is no overlap, {touching} {off}");
    // Straight up from a: the shape at (3, 6).
    let above = drop_shape(&mut s, "rectangle", 3.0, 7.0);
    assert_eq!(neighbour(doc(&s), 1, a, Dir::Up), Some(above));
    assert_eq!(neighbour(doc(&s), 1, above, Dir::Down), Some(a));
}

#[test]
fn the_nearest_wins_and_a_tie_goes_to_the_one_whose_centre_is_nearer_the_line() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    // Both 1.5 beyond the right side: the second is centred on the line, the first 0.4 off it.
    let high = drop_shape(&mut s, "rectangle", 6.0, 4.4);
    let level = drop_shape(&mut s, "rectangle", 6.0, 4.0);
    assert_eq!(neighbour(doc(&s), 1, a, Dir::Right), Some(level));
    // One nearer wins whatever its offset.
    let nearer = drop_shape(&mut s, "rectangle", 5.5, 4.45);
    assert_eq!(neighbour(doc(&s), 1, a, Dir::Right), Some(nearer), "{high}");
}

#[test]
fn connectors_and_shapes_with_no_connection_point_are_no_neighbours() {
    let (mut s, a, b, c) = row();
    // A connector between b and c lies along the lane: it is not a neighbour of anything.
    run(&mut s, "connector.add", json!({"from": {"id": b}, "to": {"id": c}}));
    assert_eq!(neighbour(doc(&s), 1, a, Dir::Right), Some(b));
    // A group has no connection point, so a connector could not be glued to it: it is passed over.
    let near_one = drop_shape(&mut s, "rectangle", 5.0, 4.0);
    let far_one = drop_shape(&mut s, "ellipse", 5.2, 4.0);
    run(&mut s, "shape.group", json!({"ids": [near_one, far_one]}));
    assert_eq!(neighbour(doc(&s), 1, a, Dir::Right), Some(b), "the group between them is passed over");
}

#[test]
fn the_nearest_neighbour_already_joined_to_the_shape_is_none_so_that_a_click_makes_a_new_shape() {
    let (mut s, a, b, c) = row();
    run(&mut s, "connector.add", json!({"from": {"id": a}, "to": {"id": b}}));
    assert_eq!(neighbour(doc(&s), 1, a, Dir::Right), None, "b is joined to a already, and c is not the nearest");
    assert_eq!(neighbour(doc(&s), 1, b, Dir::Right), Some(c), "b is joined to nothing on its right");
    // The join works either way round: a connector that runs from b to a.
    let (mut s, a, b, _) = row();
    run(&mut s, "connector.add", json!({"from": {"id": b}, "to": {"id": a}}));
    assert_eq!(neighbour(doc(&s), 1, a, Dir::Right), None);
}

#[test]
fn nothing_is_a_neighbour_of_a_shape_that_is_not_there_or_a_connector() {
    let (mut s, a, b, _) = row();
    assert_eq!(neighbour(doc(&s), 1, 999, Dir::Right), None);
    assert_eq!(neighbour(doc(&s), 9, a, Dir::Right), None, "a page that is not there");
    let joined = run(&mut s, "connector.add", json!({"from": {"id": a}, "to": {"id": b}}));
    let connector = joined["id"].as_u64().unwrap() as u32;
    assert_eq!(neighbour(doc(&s), 1, connector, Dir::Right), None);
}

#[test]
fn the_query_names_the_neighbour_and_changes_nothing() {
    let (mut s, a, b, _) = row();
    let before = snapshot(&s);
    let r = run(&mut s, "connector.neighbour", json!({"from": a, "direction": "right"}));
    assert_eq!(
        (r["neighbour"].as_u64(), r["from"].as_u64(), r["direction"].as_str()),
        (Some(u64::from(b)), Some(u64::from(a)), Some("right")),
        "{r}"
    );
    let r = run(&mut s, "connector.neighbour", json!({"from": a, "direction": "up"}));
    assert!(r["neighbour"].is_null(), "{r}");
    assert_eq!(snapshot(&s), before, "a query writes nothing");
    let e = s.execute("connector.neighbour", &json!({"from": a, "direction": "sideways"})).unwrap_err().to_string();
    assert!(e.contains("direction") && e.contains("right, left, up or down"), "{e}");
    let e = s.execute("connector.neighbour", &json!({"direction": "up"})).unwrap_err().to_string();
    assert!(e.contains("from"), "{e}");
}

#[test]
fn a_click_joins_the_nearest_neighbour_and_makes_no_shape() {
    let (mut s, a, b, c) = row();
    let (shapes_before, undo_before) = (shapes(doc(&s)), s.history().unwrap().undo_len());
    let r = run(&mut s, "connector.autoconnect", json!({"from": a, "direction": "right", "existing": true}));
    assert_eq!(r["created"], false, "{r}");
    assert_eq!(r["id"].as_u64(), Some(u64::from(b)), "the shape joined is the nearest on the right: {r}");
    assert_eq!(shapes(doc(&s)), shapes_before + 1, "the connector alone is new");
    assert_eq!(s.history().unwrap().undo_len(), undo_before + 1, "one undo step");
    assert_eq!(s.history().unwrap().next_undo_label(), Some("AutoConnect"));
    // The connector runs from the right point of a to the left point of b: the ends that face each other.
    assert_eq!(r["begin"], json!({"id": a, "point": 3}), "{r}");
    assert_eq!(r["end"], json!({"id": b, "point": 1}), "{r}");
    // Neither moved, and c is untouched.
    assert!(near(centre(doc(&s), a).0, 3.0) && near(centre(doc(&s), b).0, 6.0) && near(centre(doc(&s), c).0, 9.0));
    assert_eq!((r["width"].as_f64(), r["height"].as_f64()), (Some(1.5), Some(1.0)));
    assert!(near(r["x"].as_f64().unwrap(), 6.0) && near(r["y"].as_f64().unwrap(), 4.0), "{r}");
    assert_eq!(check_session(&s), Ok(()));
    // A second click on the same arrow finds b joined, and makes a copy beside the shape instead of a second connector to b: the place beside a is where b is,
    // so the copy is slid along to the first free one.
    let again = run(&mut s, "connector.autoconnect", json!({"from": a, "direction": "right", "existing": true}));
    assert_eq!(again["created"], true, "{again}");
    assert_ne!(again["id"].as_u64(), Some(u64::from(b)));
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(shapes(doc(&s)), shapes_before, "both clicks undone");
}

#[test]
fn a_click_with_no_neighbour_makes_a_copy_as_an_arrow_did_before() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    let r = run(&mut s, "connector.autoconnect", json!({"from": a, "direction": "right", "existing": true}));
    assert_eq!(r["created"], true, "{r}");
    assert!(near(r["x"].as_f64().unwrap(), 5.25) && near(r["y"].as_f64().unwrap(), 4.0), "the same place as without `existing`: {r}");
    let plain = {
        let mut t = blank_session();
        let a = drop_shape(&mut t, "rectangle", 3.0, 4.0);
        run(&mut t, "connector.autoconnect", json!({"from": a, "direction": "right"}))
    };
    assert_eq!(plain["created"], true, "a call without `existing` always makes a shape");
    assert_eq!((plain["x"].as_f64(), plain["y"].as_f64(), plain["id"].as_u64()), (r["x"].as_f64(), r["y"].as_f64(), r["id"].as_u64()));
}

#[test]
fn a_drag_onto_a_shape_joins_it_whatever_side_it_is_on_and_a_point_can_be_named() {
    let (mut s, a, b, c) = row();
    // c is two shapes along: joined by name, from the right point of a to the nearest point of c.
    let r = run(&mut s, "connector.autoconnect", json!({"from": a, "direction": "right", "to": c}));
    assert_eq!((r["created"].as_bool(), r["id"].as_u64()), (Some(false), Some(u64::from(c))), "{r}");
    assert_eq!(r["begin"], json!({"id": a, "point": 3}));
    assert_eq!(r["end"], json!({"id": c, "point": 1}), "the nearest point of c to the begin is its left one: {r}");
    // A point named wins: the bottom point of b (the rectangle's points go right, top, left, bottom: 3, 4... the engine tells which by its answer).
    let named = run(&mut s, "connector.autoconnect", json!({"from": a, "direction": "right", "to": b, "to_point": 2}));
    assert_eq!(named["end"], json!({"id": b, "point": 2}), "{named}");
    assert_eq!(check_session(&s), Ok(()));
    assert_eq!(s.history().unwrap().next_undo_label(), Some("AutoConnect"));
}

#[test]
fn a_join_that_cannot_be_made_is_refused_in_words_and_changes_nothing() {
    let (mut s, a, b, _) = row();
    let joined = run(&mut s, "connector.add", json!({"from": {"id": a}, "to": {"id": b}}));
    let connector = joined["id"].as_u64().unwrap() as u32;
    let fresh = drop_shape(&mut s, "rectangle", 3.0, 8.0);
    let before = snapshot(&s);
    let refuse = |s: &mut diacraft_engine::Session, params: serde_json::Value, words: &[&str]| {
        let e = s.execute("connector.autoconnect", &params).unwrap_err().to_string();
        for w in words {
            assert!(e.contains(w), "`{w}` not in {e} for {params}");
        }
    };
    refuse(&mut s, json!({"from": fresh, "direction": "right", "to": fresh}), &["to", "itself"]);
    refuse(&mut s, json!({"from": fresh, "direction": "right", "to": connector}), &["connector"]);
    refuse(&mut s, json!({"from": fresh, "direction": "right", "to": 999}), &["999"]);
    refuse(&mut s, json!({"from": fresh, "direction": "right", "to": b, "to_point": 99}), &["point"]);
    refuse(&mut s, json!({"from": fresh, "direction": "right", "to": b, "master": "ellipse"}), &["to", "master"]);
    refuse(&mut s, json!({"from": fresh, "direction": "right", "existing": true, "master": "ellipse"}), &["existing", "master"]);
    refuse(&mut s, json!({"from": fresh, "direction": "right", "existing": true, "to": b}), &["existing", "to"]);
    refuse(&mut s, json!({"from": fresh, "direction": "right", "existing": "yes"}), &["existing", "true or false"]);
    refuse(&mut s, json!({"from": fresh, "direction": "right", "to_point": 1}), &["to_point", "to"]);
    assert_eq!(snapshot(&s), before, "a refusal leaves the drawing as it was");
}

#[test]
fn a_group_cannot_be_joined_by_a_drag_because_it_has_no_connection_point() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    let b = drop_shape(&mut s, "rectangle", 6.0, 4.0);
    let c = drop_shape(&mut s, "rectangle", 6.0, 6.0);
    let group = run(&mut s, "shape.group", json!({"ids": [b, c]}));
    let group = group["id"].as_u64().unwrap() as u32;
    let before = snapshot(&s);
    let e = s.execute("connector.autoconnect", &json!({"from": a, "direction": "right", "to": group})).unwrap_err().to_string();
    assert!(e.contains("connection point"), "{e}");
    assert_eq!(snapshot(&s), before);
}
