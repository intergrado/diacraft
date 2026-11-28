//! `shape.delete` takes the connectors glued to the shapes it deletes (the specification's Delete "removes the selection along with connectors attached to it",
//! its vector V6: "given shapes A and B and a connector between them, deleting A, expect the connector gone"). The command and the window's Delete key do the same; `connectors:
//! false` is the earlier rule (the connector stays, its end a plain point where it was). A connector the pointer cannot pick (on a locked layer, or only on hidden ones)
//! stays, as the window leaves it.

use diacraft_doc::{Document, Owner};
use diacraft_engine::{EngineError, Session};
use serde_json::json;

use crate::edit::{doc, drop_shape, id_of, num};
use crate::{blank_session, check_session, run, snapshot};

/// Rectangle 1 at (2, 2) and rectangle 2 at (6, 4), joined by connector 3.
fn two_joined() -> Session {
    let mut s = blank_session();
    assert_eq!(drop_shape(&mut s, "rectangle", 2.0, 2.0), 1);
    assert_eq!(drop_shape(&mut s, "rectangle", 6.0, 4.0), 2);
    assert_eq!(id_of(&run(&mut s, "connector.add", json!({"from": {"id": 1}, "to": {"id": 2}}))), 3);
    s
}

/// The shapes at the top of page 1, by `ID`.
fn top(d: &Document) -> Vec<u32> {
    d.page(1).unwrap().shapes.iter().map(|shape| shape.id).collect()
}

fn connect_rows(d: &Document) -> usize {
    d.page(1).unwrap().connects.len()
}

#[test]
fn deleting_a_shape_takes_the_connector_glued_to_it() {
    let mut s = two_joined();
    let steps = s.history().unwrap().undo_len();
    let answer = run(&mut s, "shape.delete", json!({"ids": [2]}));
    assert_eq!(answer["deleted"], 2, "the shape and its connector: {answer}");
    assert_eq!(answer["connectors_deleted"], json!([3]), "{answer}");
    let d = doc(&s);
    assert_eq!(top(d), [1], "only rectangle 1 is left");
    assert_eq!(connect_rows(d), 0, "no Connect row names the connector or the shape");
    assert_eq!(s.history().unwrap().undo_len(), steps + 1, "one undo step");
    assert_eq!(check_session(&s), Ok(()));
    // One undo brings the shape, the connector and the glue back.
    run(&mut s, "edit.undo", json!({}));
    let d = doc(&s);
    assert_eq!(top(d), [1, 2, 3]);
    assert_eq!(connect_rows(d), 2);
}

#[test]
fn a_connector_glued_to_the_first_shape_goes_with_it_as_well() {
    let mut s = two_joined();
    run(&mut s, "shape.delete", json!({"ids": [1]}));
    assert_eq!(top(doc(&s)), [2]);
    assert_eq!(connect_rows(doc(&s)), 0);
}

#[test]
fn a_connector_glued_to_two_shapes_that_both_go_is_counted_once() {
    let mut s = two_joined();
    let answer = run(&mut s, "shape.delete", json!({"ids": [1, 2]}));
    assert_eq!(answer["deleted"], 3, "{answer}");
    assert_eq!(answer["connectors_deleted"], json!([3]), "{answer}");
    assert!(top(doc(&s)).is_empty());
    // The connector named with them is no connector taken along.
    let mut s = two_joined();
    let answer = run(&mut s, "shape.delete", json!({"ids": [1, 2, 3]}));
    assert_eq!(answer["deleted"], 3, "{answer}");
    assert_eq!(answer["connectors_deleted"], json!([]), "{answer}");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_connector_with_one_end_free_goes_with_the_shape_its_other_end_is_glued_to() {
    let mut s = blank_session();
    assert_eq!(drop_shape(&mut s, "rectangle", 2.0, 2.0), 1);
    assert_eq!(drop_shape(&mut s, "rectangle", 6.0, 4.0), 2);
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"id": 1}, "to": {"x": 8.0, "y": 8.0}})));
    let answer = run(&mut s, "shape.delete", json!({"ids": [1]}));
    assert_eq!(answer["connectors_deleted"], json!([c]), "{answer}");
    assert_eq!(top(doc(&s)), [2]);
}

#[test]
fn connectors_false_leaves_the_connector_with_a_free_end_where_it_was() {
    let mut s = two_joined();
    let answer = run(&mut s, "shape.delete", json!({"ids": [2], "connectors": false}));
    assert_eq!(answer["deleted"], 1, "{answer}");
    assert_eq!(answer["connectors_deleted"], json!([]), "{answer}");
    let d = doc(&s);
    assert_eq!(top(d), [1, 3], "the connector stays");
    assert!((num(d, 3, "EndX") - 5.25).abs() < 1.0e-6 && (num(d, 3, "EndY") - 4.0).abs() < 1.0e-6, "its end is where it was");
    assert_eq!(connect_rows(d), 1, "the begin is still glued to rectangle 1");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn connectors_must_be_true_or_false() {
    let mut s = two_joined();
    let before = snapshot(&s);
    for params in [json!({"ids": [2], "connectors": 1}), json!({"ids": [2], "connectors": "no"}), json!({"ids": [2], "connectors": []})] {
        let e = s.execute("shape.delete", &params).unwrap_err();
        assert!(matches!(&e, EngineError::BadParams { msg, .. } if msg.contains("`connectors` must be true or false")), "{params}: {e}");
    }
    assert_eq!(snapshot(&s), before);
    // `null` is no word on it.
    run(&mut s, "shape.delete", json!({"ids": [2], "connectors": null}));
    assert_eq!(top(doc(&s)), [1]);
}

#[test]
fn a_connector_glued_to_a_member_goes_with_the_member_and_with_the_group_it_is_in() {
    for named in ["member", "group"] {
        let mut s = blank_session();
        assert_eq!(drop_shape(&mut s, "rectangle", 2.0, 2.0), 1);
        assert_eq!(drop_shape(&mut s, "ellipse", 6.0, 4.0), 2);
        assert_eq!(drop_shape(&mut s, "rectangle", 9.0, 6.0), 3);
        let c = id_of(&run(&mut s, "connector.add", json!({"from": {"id": 1}, "to": {"id": 3}})));
        let g = id_of(&run(&mut s, "shape.group", json!({"ids": [1, 2]})));
        let target = if named == "member" { 1 } else { g };
        let answer = run(&mut s, "shape.delete", json!({"ids": [target]}));
        assert_eq!(answer["connectors_deleted"], json!([c]), "{named}: {answer}");
        let d = doc(&s);
        assert!(d.find_shape(Owner::Page(1), c).is_none(), "{named}: the connector is gone");
        assert!(d.find_shape(Owner::Page(1), 3).is_some(), "{named}: the shape at its other end stays");
        assert_eq!(connect_rows(d), 0, "{named}");
        assert_eq!(check_session(&s), Ok(()), "{named}");
    }
}

#[test]
fn a_connector_on_a_locked_layer_stays_with_a_free_end_and_the_shape_goes() {
    let mut s = two_joined();
    run(&mut s, "layer.add", json!({"name": "Held"}));
    run(&mut s, "layer.assign", json!({"ids": [3], "layers": ["Held"]}));
    run(&mut s, "layer.set", json!({"layer": "Held", "lock": true}));
    let answer = run(&mut s, "shape.delete", json!({"ids": [2]}));
    assert_eq!(answer["deleted"], 1, "{answer}");
    assert_eq!(answer["connectors_deleted"], json!([]), "{answer}");
    let d = doc(&s);
    assert_eq!(top(d), [1, 3]);
    assert_eq!(connect_rows(d), 1, "the end at the deleted shape is a plain point, the other still glued");
}

#[test]
fn a_connector_on_hidden_layers_only_stays_with_a_free_end_and_the_shape_goes() {
    let mut s = two_joined();
    run(&mut s, "layer.add", json!({"name": "Away"}));
    run(&mut s, "layer.assign", json!({"ids": [3], "layers": ["Away"]}));
    run(&mut s, "layer.set", json!({"layer": "Away", "visible": false}));
    let answer = run(&mut s, "shape.delete", json!({"ids": [2]}));
    assert_eq!(answer["connectors_deleted"], json!([]), "{answer}");
    assert_eq!(top(doc(&s)), [1, 3]);
}

#[test]
fn a_connector_taken_along_takes_its_comments_and_is_one_undo_step() {
    let mut s = two_joined();
    run(&mut s, "comment.add", json!({"id": 3, "text": "check"}));
    let steps = s.history().unwrap().undo_len();
    run(&mut s, "shape.delete", json!({"ids": [2]}));
    assert_eq!(s.history().unwrap().undo_len(), steps + 1);
    assert_eq!(doc(&s).comments.len(), 0, "the comment on the connector went with it");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(doc(&s).comments.len(), 1);
}

#[test]
fn a_cut_still_leaves_the_connector_with_a_free_end() {
    // A cut sends the shapes selected and no more.
    let mut s = two_joined();
    run(&mut s, "edit.cut", json!({"ids": [2]}));
    assert_eq!(top(doc(&s)), [1, 3], "the connector stays");
}
