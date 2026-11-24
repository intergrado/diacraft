//! Change Shape: `shape.change {ids, master}` swaps the master of the shapes it names for another and keeps what the shapes are: their
//! place, size, text, local formatting, shape data and glue. A one-dimensional shape is swapped only for a one-dimensional master and a two-dimensional shape
//! only for a two-dimensional one. Numbers are paper inches on pages of the default scale.

use diacraft_doc::{CellAddr, CellEdit, Document, Owner, Resolver, SheetRef, TextPart, Value, WriteOrigin};
use diacraft_engine::draw::connection_points;
use diacraft_engine::{EngineError, Session};
use serde_json::{Value as Json, json};

use crate::edit::{doc, drop_shape, geometry, id_of, num};
use crate::{blank_session, check_session, run, snapshot};

const TOL: f64 = 1.0e-6;

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < TOL
}

fn assert_near(what: &str, got: f64, want: f64) {
    assert!(near(got, want), "{what}: got {got}, want {want}");
}

fn shape_of(d: &Document, id: u32) -> &diacraft_doc::Shape {
    d.shape(&d.find_shape(Owner::Page(1), id).unwrap_or_else(|| panic!("no shape {id} on page 1"))).unwrap()
}

/// The name of the master shape `id` is an instance of, if it is one.
fn master_name(d: &Document, id: u32) -> Option<String> {
    let m = shape_of(d, id).master?;
    d.master(m).and_then(|m| m.name.as_deref().map(str::to_owned))
}

/// The text of a shape: its characters, in order.
fn text_of(d: &Document, id: u32) -> String {
    shape_of(d, id)
        .text
        .as_ref()
        .map(|t| t.parts.iter().filter_map(|p| if let TextPart::Chars(c) = p { Some(&**c) } else { None }).collect::<String>())
        .unwrap_or_default()
}

fn write_cell(s: &mut Session, id: u32, name: &str, text: &str) {
    s.edit("test", |d| {
        let sheet = SheetRef::Shape(d.find_shape(Owner::Page(1), id).unwrap());
        d.write_cell(&sheet, &CellAddr::direct(name), CellEdit::Value(Value::from_text(text)), WriteOrigin::Drawing)
            .map(|_| ())
            .map_err(|e| EngineError::Internal { cmd: "test".into(), msg: e.to_string() })
    })
    .unwrap();
}

/// The `Connect` rows of page 1: (from shape, from cell, to shape, to part).
fn connects(d: &Document) -> Vec<(u32, String, u32, u32)> {
    d.page(1)
        .unwrap()
        .connects
        .iter()
        .map(|c| (c.from_sheet, c.from_cell.as_deref().unwrap_or("").to_owned(), c.to_sheet, c.to_part.unwrap_or(0)))
        .collect()
}

fn refusal(s: &mut Session, params: Json) -> String {
    let before = snapshot(s);
    let e = s.execute("shape.change", &params).expect_err("the change is refused");
    assert_eq!(snapshot(s), before, "a refused shape.change changes nothing: {params}");
    assert_eq!(check_session(s), Ok(()));
    e.to_string()
}

/// Where the end of connector `id` is, on the page (the connector is on the page itself).
fn end_of(d: &Document, id: u32, which: &str) -> (f64, f64) {
    (num(d, id, &format!("{which}X")), num(d, id, &format!("{which}Y")))
}

// ---- two-dimensional shapes -------------------------------------------------------------------------------------------------------

#[test]
fn a_process_becomes_a_decision_where_it_was_as_big_as_it_was_with_its_text() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "process", 3.0, 2.0);
    run(&mut s, "shape.set_text", json!({"id": a, "text": "Check"}));
    run(&mut s, "shape.resize", json!({"id": a, "x": 1.9, "y": 1.45, "width": 2.2, "height": 1.1}));
    let other = drop_shape(&mut s, "rectangle", 7.0, 5.0);
    let before = doc(&s).clone();
    let steps = s.history().unwrap().undo_len();
    let r = run(&mut s, "shape.change", json!({"ids": [a], "master": "decision"}));
    assert_eq!(r["changed"], json!([a]), "{r}");
    assert_eq!(s.history().unwrap().undo_len(), steps + 1, "one undo step");
    let d = doc(&s);
    assert_eq!(master_name(d, a).as_deref(), Some("Decision"));
    assert_eq!(shape_of(d, a).id, a, "the shape keeps its ID");
    for (cell, was) in [("PinX", 3.0), ("PinY", 2.0), ("Width", 2.2), ("Height", 1.1)] {
        assert_near(cell, num(d, a, cell), was);
    }
    assert_eq!(text_of(d, a), "Check");
    assert!(shape_of(d, a).name.as_deref().is_some_and(|n| n.starts_with("Decision.")), "{:?}", shape_of(d, a).name);
    // The outline is the decision's, at the size the shape had: the geometry of a decision dropped and made as big.
    let probe = drop_shape(&mut s, "decision", 9.0, 9.0);
    run(&mut s, "shape.resize", json!({"id": probe, "x": 7.9, "y": 8.45, "width": 2.2, "height": 1.1}));
    assert_eq!(geometry(doc(&s), a), geometry(doc(&s), probe), "the outline of the new master, at the shape's own size");
    // The others are untouched, and the order of the page is kept.
    assert_eq!(master_name(doc(&s), other).as_deref(), Some("Rectangle"));
    let order: Vec<u32> = doc(&s).page(1).unwrap().shapes.iter().map(|s| s.id).collect();
    assert_eq!(order, vec![a, other, probe]);
    assert_eq!(check_session(&s), Ok(()));
    // Undo brings the process back as it was.
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(doc(&s), &before, "undo: the document is as it was");
}

#[test]
fn the_new_shape_is_drawn_as_the_new_master_draws_it_where_the_old_one_was() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 4.0, 3.0);
    run(&mut s, "shape.change", json!({"ids": [a], "master": "ellipse"}));
    let d = doc(&s);
    assert_eq!(master_name(d, a).as_deref(), Some("Ellipse"));
    // The ellipse is one `Ellipse` row; the old rectangle's five rows are not there.
    assert_eq!(geometry(d, a).iter().map(|r| r.0.as_str()).collect::<Vec<_>>(), ["Ellipse"]);
    assert_near("PinX", num(d, a, "PinX"), 4.0);
    assert_near("Width", num(d, a, "Width"), 1.5);
    assert_near("Height", num(d, a, "Height"), 1.0);
}

#[test]
fn a_shape_keeps_its_local_formatting_and_takes_the_rest_from_the_new_master() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    run(&mut s, "shape.format", json!({"ids": [a], "fill": "#ff0000"}));
    let fill = |s: &Session, id: u32| {
        let res = Resolver::new(doc(s));
        let sheet = SheetRef::Shape(doc(s).find_shape(Owner::Page(1), id).unwrap());
        res.resolve(&sheet, &CellAddr::direct("FillForegnd")).and_then(|r| r.value.cell.and_then(|c| c.value.clone())).map(|v| v.text().to_owned())
    };
    let red = fill(&s, a);
    assert!(red.is_some());
    run(&mut s, "shape.change", json!({"ids": [a], "master": "diamond"}));
    assert_eq!(fill(&s, a), red, "the colour the shape was given stays");
}

#[test]
fn several_shapes_are_changed_in_one_undo_step_and_a_second_change_of_the_same_master_is_no_step() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 2.0, 2.0);
    let b = drop_shape(&mut s, "ellipse", 5.0, 2.0);
    let c = drop_shape(&mut s, "rectangle", 8.0, 2.0);
    let steps = s.history().unwrap().undo_len();
    let r = run(&mut s, "shape.change", json!({"ids": [a, b, c], "master": "hexagon"}));
    assert_eq!(r["changed"], json!([a, b, c]), "{r}");
    assert_eq!(s.history().unwrap().undo_len(), steps + 1);
    for id in [a, b, c] {
        assert_eq!(master_name(doc(&s), id).as_deref(), Some("Hexagon"));
    }
    // Everything already is one: nothing changes, and no step is made.
    let before = snapshot(&s);
    let r = run(&mut s, "shape.change", json!({"ids": [a, b, c], "master": "hexagon"}));
    assert_eq!(r["changed"], json!([]), "{r}");
    assert_eq!(r["unchanged"], json!([a, b, c]), "{r}");
    assert_eq!(snapshot(&s).document, before.document);
    assert_eq!(s.history().unwrap().undo_len(), steps + 1, "no empty step");
}

#[test]
fn a_shape_with_no_master_gets_one() {
    // A box of text has no master.
    let mut s = blank_session();
    let t = id_of(&run(&mut s, "text.box", json!({"page": 1, "x": 4.0, "y": 4.0, "text": "Label"})));
    assert!(shape_of(doc(&s), t).master.is_none());
    let r = run(&mut s, "shape.change", json!({"ids": [t], "master": "rectangle"}));
    assert_eq!(r["changed"], json!([t]), "{r}");
    assert_eq!(master_name(doc(&s), t).as_deref(), Some("Rectangle"));
    assert_eq!(text_of(doc(&s), t), "Label");
    assert_near("PinX", num(doc(&s), t, "PinX"), 4.0);
}

// ---- glue ---------------------------------------------------------------------------------------------------------------------------

fn two_joined() -> Session {
    let mut s = blank_session();
    assert_eq!(drop_shape(&mut s, "rectangle", 2.0, 2.0), 1);
    assert_eq!(drop_shape(&mut s, "rectangle", 6.0, 4.0), 2);
    assert_eq!(id_of(&run(&mut s, "connector.add", json!({"from": {"id": 1}, "to": {"id": 2}}))), 3);
    s
}

#[test]
fn connectors_glued_to_the_shape_stay_glued_and_their_ends_follow_the_new_connection_points() {
    let mut s = two_joined();
    let rows = connects(doc(&s));
    assert_eq!(rows.len(), 2);
    // Make the first shape a good deal bigger and another outline: its connection points are somewhere else now.
    run(&mut s, "shape.resize", json!({"id": 1, "x": 1.0, "y": 1.0, "width": 3.0, "height": 2.0}));
    run(&mut s, "shape.change", json!({"ids": [1], "master": "triangle"}));
    let d = doc(&s);
    assert_eq!(connects(d), rows, "the same Connect rows");
    // The connector's begin is at the connection point it is glued to, which is the triangle's now.
    let begin = connects(d).into_iter().find(|c| c.1 == "BeginX").map(|c| c.3 - 100).unwrap();
    let point = connection_points(d, 1, 1).into_iter().find(|p| p.index == begin).unwrap();
    let got = end_of(d, 3, "Begin");
    assert!(near(got.0, point.x) && near(got.1, point.y), "the begin {got:?} is at point {begin} {point:?}");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn undoing_a_change_puts_the_glue_back_with_the_shape() {
    let mut s = two_joined();
    let before = doc(&s).clone();
    run(&mut s, "shape.change", json!({"ids": [2], "master": "triangle"}));
    assert_ne!(doc(&s), &before);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(doc(&s), &before);
}

// ---- one-dimensional shapes ---------------------------------------------------------------------------------------------------------

#[test]
fn a_connector_is_swapped_for_a_connector_master_and_keeps_its_glue_ends_and_arrow() {
    let mut s = two_joined();
    let before = doc(&s).clone();
    let r = run(&mut s, "shape.change", json!({"ids": [3], "master": "gane-sarson.data-flow"}));
    assert_eq!(r["changed"], json!([3]), "{r}");
    let d = doc(&s);
    assert_eq!(master_name(d, 3).as_deref(), Some("Data Flow"));
    assert_eq!(connects(d), connects(&before), "still glued to the same shapes at the same points");
    for cell in ["BeginX", "BeginY", "EndX", "EndY", "PinX", "PinY", "Width", "Angle"] {
        assert_near(cell, num(d, 3, cell), num(&before, 3, cell));
    }
    assert_eq!(geometry(d, 3), geometry(&before, 3), "the route is the connector's own");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_connector_is_refused_a_rectangle_and_a_rectangle_a_connector_in_words_and_nothing_changes() {
    let mut s = two_joined();
    let why = refusal(&mut s, json!({"ids": [3], "master": "rectangle"}));
    assert!(why.contains("one-dimensional") && why.contains("3"), "{why}");
    let why = refusal(&mut s, json!({"ids": [1], "master": "gane-sarson.data-flow"}));
    assert!(why.contains("two-dimensional") && why.contains("1"), "{why}");
}

#[test]
fn a_mixed_selection_changes_the_shapes_that_can_be_changed_and_says_why_it_left_the_others() {
    let mut s = two_joined();
    let r = run(&mut s, "shape.change", json!({"ids": [1, 3, 2], "master": "ellipse"}));
    assert_eq!(r["changed"], json!([1, 2]), "{r}");
    assert_eq!(r["skipped"].as_array().map(Vec::len), Some(1), "{r}");
    assert_eq!(r["skipped"][0]["id"], 3);
    assert!(r["skipped"][0]["why"].as_str().unwrap().contains("one-dimensional"), "{r}");
    assert_eq!(master_name(doc(&s), 1).as_deref(), Some("Ellipse"));
    assert!(shape_of(doc(&s), 3).master.is_none());
}

// ---- refusals ---------------------------------------------------------------------------------------------------------------------

#[test]
fn bad_parameters_are_refused_in_words_and_change_nothing() {
    let mut s = two_joined();
    for (params, word) in [
        (json!({"ids": [], "master": "ellipse"}), "ids"),
        (json!({"master": "ellipse"}), "ids"),
        (json!({"ids": [1]}), "master"),
        (json!({"ids": [1], "master": "no such master"}), "no such master"),
        (json!({"ids": [99], "master": "ellipse"}), "99"),
        (json!({"ids": [1], "master": "ellipse", "page": 9}), "page 9"),
        (json!({"ids": [1], "master": "ellipse", "stencil": "file-9"}), "file-9"),
        (json!({"ids": [1], "master": {"a": 1}}), "master"),
        (json!({"ids": [1], "master": "ellipse", "extra": 1}), "extra"),
    ] {
        let why = refusal(&mut s, params.clone());
        assert!(why.contains(word), "{params}: {why}");
    }
}

#[test]
fn a_shape_that_cannot_be_swapped_is_skipped_and_a_selection_of_only_those_is_refused() {
    // A guide, and a group's member of a user's own: a member is swapped on its own; a group that holds shapes of the user's is not swapped.
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 2.0, 2.0);
    let b = drop_shape(&mut s, "rectangle", 4.0, 2.0);
    let g = id_of(&run(&mut s, "shape.group", json!({"ids": [a, b]})));
    let why = refusal(&mut s, json!({"ids": [g], "master": "ellipse"}));
    assert!(why.contains(&format!("{g}")) && why.contains("group"), "{why}");
    let r = run(&mut s, "shape.change", json!({"ids": [a], "master": "ellipse"}));
    assert_eq!(r["changed"], json!([a]), "a member can be swapped on its own: {r}");
    assert_eq!(master_name(doc(&s), a).as_deref(), Some("Ellipse"));
}

#[test]
fn the_locks_of_the_replace_cells_keep_a_shape_out_of_the_swap() {
    // `LockReplace` (shape), `PageLockReplace` and `DocLockReplace`.
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 2.0, 2.0);
    let b = drop_shape(&mut s, "rectangle", 5.0, 2.0);
    write_cell(&mut s, a, "LockReplace", "1");
    let r = run(&mut s, "shape.change", json!({"ids": [a, b], "master": "ellipse"}));
    assert_eq!(r["changed"], json!([b]), "{r}");
    assert!(r["skipped"][0]["why"].as_str().unwrap().contains("LockReplace"), "{r}");
    assert_eq!(master_name(doc(&s), a).as_deref(), Some("Rectangle"));
    let why = refusal(&mut s, json!({"ids": [a], "master": "ellipse"}));
    assert!(why.contains("LockReplace"), "{why}");

    // The page's lock: a shape whose own lock is 0 is kept out too, and the page's lock alone is enough.
    let mut s = blank_session();
    let c = drop_shape(&mut s, "rectangle", 2.0, 2.0);
    s.edit("lock", |d| {
        d.write_cell(&SheetRef::Page(1), &CellAddr::direct("PageLockReplace"), CellEdit::Value(Value::from_text("1")), WriteOrigin::Drawing)
            .map(|_| ())
            .map_err(|e| EngineError::Internal { cmd: "test".into(), msg: e.to_string() })
    })
    .unwrap();
    let why = refusal(&mut s, json!({"ids": [c], "master": "ellipse"}));
    assert!(why.contains("PageLockReplace"), "{why}");

    // The document's lock greys the command out.
    let mut s = blank_session();
    let e = drop_shape(&mut s, "rectangle", 2.0, 2.0);
    s.edit("lock", |d| {
        d.write_cell(&SheetRef::Document, &CellAddr::direct("DocLockReplace"), CellEdit::Value(Value::from_text("1")), WriteOrigin::Drawing)
            .map(|_| ())
            .map_err(|e| EngineError::Internal { cmd: "test".into(), msg: e.to_string() })
    })
    .unwrap();
    let info = s.commands().into_iter().find(|c| c.id == "shape.change").expect("the command is registered");
    assert!(!info.enabled && info.disabled_reason.as_deref().is_some_and(|w| w.contains("DocLockReplace")), "{info:?}");
    let why = refusal(&mut s, json!({"ids": [e], "master": "ellipse"}));
    assert!(why.contains("DocLockReplace"), "{why}");
}

#[test]
fn the_ids_are_capped_and_a_repeated_id_is_changed_once() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 2.0, 2.0);
    let r = run(&mut s, "shape.change", json!({"ids": [a, a, a], "master": "ellipse"}));
    assert_eq!(r["changed"], json!([a]), "{r}");
    let many: Vec<u32> = (0..600).collect();
    let why = refusal(&mut s, json!({"ids": many, "master": "ellipse"}));
    assert!(why.contains("at most"), "{why}");
}
