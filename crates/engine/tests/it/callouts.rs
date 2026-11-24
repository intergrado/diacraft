//! The Insert tab's callouts (`callout.styles`, `callout.insert`, `callout.attach`, `callout.detach`, `callout.list`), through `Session::execute` as the control channel, the CLI and MCP call
//! them. The rules are the specifications': a callout is added tied to the shape selected, and moves, is copied and is deleted with it, and the ties are the
//! `Relationships` cell's, both sides. What the specifications leave open is in `docs/editing-commands.md`, "Callouts".

use diacraft_doc::{CellAddr, Document, FormulaSlot, Owner, Resolver, RowKey, SheetRef};
use diacraft_engine::{EngineError, Session};
use serde_json::{Value as Json, json};

use crate::edit::{doc, drop_shape, id_of, num};
use crate::{blank_session, check_session, run, snapshot};

const EPS: f64 = 1.0e-6;

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < EPS
}

fn sheet_of(d: &Document, id: u32) -> SheetRef {
    SheetRef::Shape(d.find_shape(Owner::Page(1), id).unwrap_or_else(|| panic!("no shape {id} on page 1")))
}

/// A cell of `Scratch` row 1 (`IX` 0) of shape `id`, as a number.
fn scratch(d: &Document, id: u32, cell: &str) -> f64 {
    let found = Resolver::new(d)
        .resolve(&sheet_of(d, id), &CellAddr::in_row("Scratch", None, RowKey::Ix(0), cell))
        .unwrap_or_else(|| panic!("shape {id} has no Scratch.{cell}1"));
    found.value.cell.and_then(|c| c.value.as_ref()).and_then(|v| v.number()).unwrap_or_else(|| panic!("Scratch.{cell}1 holds no number"))
}

/// A cell of row `ix` (0 for the first) of the `Scratch` section of shape `id`.
fn scratch_row(d: &Document, id: u32, ix: u32, cell: &str) -> f64 {
    let found = Resolver::new(d)
        .resolve(&sheet_of(d, id), &CellAddr::in_row("Scratch", None, RowKey::Ix(ix), cell))
        .unwrap_or_else(|| panic!("shape {id} has no Scratch row {ix} cell {cell}"));
    found.value.cell.and_then(|c| c.value.as_ref()).and_then(|v| v.number()).unwrap_or_else(|| panic!("Scratch row {ix} {cell} holds no number"))
}

/// The text of the `Relationships` formula of shape `id`, if it holds one.
fn relationships(d: &Document, id: u32) -> Option<String> {
    let at = d.find_shape(Owner::Page(1), id)?;
    let shape = d.shape(&at)?;
    match &shape.sheet.cell("Relationships")?.formula {
        FormulaSlot::Local(f) => Some(f.source.to_string()),
        _ => None,
    }
}

fn list(s: &mut Session, id: u32) -> Json {
    run(s, "callout.list", json!({"id": id}))
}

fn callouts_of(s: &mut Session, id: u32) -> Vec<u32> {
    list(s, id)["callouts"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap() as u32).collect()
}

fn target_of(s: &mut Session, id: u32) -> Option<u32> {
    list(s, id)["target"].as_u64().map(|v| v as u32)
}

fn tip_of(s: &mut Session, id: u32) -> (f64, f64) {
    let tip = list(s, id)["tip"].clone();
    (tip[0].as_f64().unwrap(), tip[1].as_f64().unwrap())
}

fn pin(d: &Document, id: u32) -> (f64, f64) {
    (num(d, id, "PinX"), num(d, id, "PinY"))
}

/// A callout of the style `style` for the shape `target`.
fn callout_for(s: &mut Session, target: u32) -> u32 {
    let made = run(s, "callout.insert", json!({"ids": [target]}));
    made["id"].as_u64().unwrap() as u32
}

/// The page box of shape `id`: (x0, y0, x1, y1).
fn page_box(s: &Session, id: u32) -> (f64, f64, f64, f64) {
    let c = diacraft_engine::draw::shape_corners(doc(s), 1, id).unwrap();
    let xs = c.map(|p| p.0);
    let ys = c.map(|p| p.1);
    (
        xs.iter().cloned().fold(f64::INFINITY, f64::min),
        ys.iter().cloned().fold(f64::INFINITY, f64::min),
        xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
        ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
    )
}

/// How far (x, y) is from the edge of the box, which is 0 for a point on the edge and more when it is inside or outside.
fn off_edge((x, y): (f64, f64), (x0, y0, x1, y1): (f64, f64, f64, f64)) -> f64 {
    let outside = (x < x0 - EPS) || (x > x1 + EPS) || (y < y0 - EPS) || (y > y1 + EPS);
    if outside {
        return f64::INFINITY;
    }
    [x - x0, x1 - x, y - y0, y1 - y].into_iter().fold(f64::INFINITY, f64::min)
}

fn refused(s: &mut Session, command: &str, params: Json) -> String {
    let e = s.execute(command, &params).expect_err("the call is refused");
    match &e {
        EngineError::BadParams { msg, .. } | EngineError::Failed { msg, .. } => msg.clone(),
        other => panic!("expected a refusal in words, got {other:?}"),
    }
}

#[test]
fn the_gallery_has_fourteen_tiles_with_ids_and_names() {
    let mut s = blank_session();
    let styles = run(&mut s, "callout.styles", json!({}));
    let tiles = styles["styles"].as_array().unwrap();
    assert_eq!(tiles.len(), 14);
    let ids: std::collections::BTreeSet<&str> = tiles.iter().map(|t| t["id"].as_str().unwrap()).collect();
    assert_eq!(ids.len(), 14, "the ids differ");
    assert!(tiles.iter().all(|t| !t["name"].as_str().unwrap().is_empty()));
    assert!(styles["width"].as_f64().unwrap() > 0.0 && styles["height"].as_f64().unwrap() > 0.0);
}

#[test]
fn a_callout_for_a_shape_is_tied_to_it_on_both_sides_and_says_text() {
    let mut s = blank_session();
    let t = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    let made = run(&mut s, "callout.insert", json!({"ids": [t], "style": "speech_box"}));
    let c = id_of(&made);
    assert_eq!(made["ids"], json!([c]));
    assert_eq!(made["targets"], json!([t]));
    assert_eq!(made["style"], "speech_box");
    assert!(c > t);
    // Both sides hold the tie.
    assert_eq!(callouts_of(&mut s, t), [c]);
    assert_eq!(target_of(&mut s, c), Some(t));
    assert_eq!(relationships(doc(&s), t).as_deref(), Some(&format!("SUM(DEPENDSON(3,Sheet.{c}!SheetRef()))")[..]));
    assert_eq!(relationships(doc(&s), c).as_deref(), Some(&format!("SUM(DEPENDSON(6,Sheet.{t}!SheetRef()))")[..]));
    let info = list(&mut s, c);
    assert_eq!(info["is_callout"], true);
    assert_eq!(info["style"], "speech_box");
    assert_eq!(list(&mut s, t)["is_callout"], false);
    assert_eq!(diacraft_engine::draw::text_view(doc(&s), 1, c).unwrap().chars, "Text");
    check_session(&s).unwrap();
}

#[test]
fn the_end_of_the_leader_is_on_the_edge_of_the_target_nearest_the_callout() {
    let mut s = blank_session();
    let t = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    let c = callout_for(&mut s, t);
    let target_box = page_box(&s, t);
    let tip = tip_of(&mut s, c);
    assert!(off_edge(tip, target_box) < EPS, "the end {tip:?} is on the edge of the target {target_box:?}");
    // The callout is up and to the right of the target, so the end is on the side facing it.
    let callout_pin = pin(doc(&s), c);
    assert!(callout_pin.0 > target_box.2 - EPS || callout_pin.1 > target_box.3 - EPS, "the callout is beside or above the target");
    assert!(
        tip.0 >= (target_box.0 + target_box.2) / 2.0 - EPS && tip.1 >= (target_box.1 + target_box.3) / 2.0 - EPS,
        "the end is on the upper right half of the edge: {tip:?}"
    );
}

#[test]
fn a_callout_made_with_no_shape_is_where_asked_and_tied_to_nothing() {
    let mut s = blank_session();
    let made = run(&mut s, "callout.insert", json!({"x": 4.0, "y": 5.0, "style": "thought_cloud", "text": "Hello"}));
    let c = id_of(&made);
    assert_eq!(made["targets"], json!([null]));
    let d = doc(&s);
    assert!(near(num(d, c, "PinX"), 4.0) && near(num(d, c, "PinY"), 5.0));
    assert_eq!(target_of(&mut s, c), None);
    assert!(relationships(doc(&s), c).is_none(), "a callout tied to nothing holds no ties");
    assert_eq!(list(&mut s, c)["is_callout"], true);
    // Its end stays where it was put in its own frame: below and to the left of the box.
    let (tx, ty) = tip_of(&mut s, c);
    assert!(tx < 4.0 - 0.75 && ty < 5.0 - 0.25, "the end is outside the box, down and to the left: {tx} {ty}");
    // With no place it is in the middle of the page.
    let page = run(&mut s, "callout.insert", json!({}));
    let c2 = id_of(&page);
    let d = doc(&s);
    let page_cell = |name: &str| {
        Resolver::new(d)
            .resolve(&SheetRef::Page(1), &CellAddr::direct(name))
            .and_then(|f| f.value.cell.and_then(|c| c.value.as_ref()).and_then(|v| v.number()))
            .unwrap()
    };
    assert!(near(num(d, c2, "PinX"), page_cell("PageWidth") / 2.0) && near(num(d, c2, "PinY"), page_cell("PageHeight") / 2.0));
}

#[test]
fn moving_the_target_moves_its_callouts_the_same_distance_and_the_end_of_the_leader_with_them() {
    let mut s = blank_session();
    let t = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    let c = callout_for(&mut s, t);
    let (before_pin, before_tip) = (pin(doc(&s), c), tip_of(&mut s, c));
    let target_before = pin(doc(&s), t);
    let moved = run(&mut s, "shape.move", json!({"ids": [t], "dx": 1.5, "dy": -0.5}));
    assert_eq!(moved["moved"], 1, "the callout is not counted among the shapes moved");
    let after_pin = pin(doc(&s), c);
    assert!(near(after_pin.0, before_pin.0 + 1.5) && near(after_pin.1, before_pin.1 - 0.5), "{before_pin:?} -> {after_pin:?}");
    let target_after = pin(doc(&s), t);
    assert!(near(target_after.0, target_before.0 + 1.5) && near(target_after.1, target_before.1 - 0.5));
    let after_tip = tip_of(&mut s, c);
    assert!(near(after_tip.0, before_tip.0 + 1.5) && near(after_tip.1, before_tip.1 - 0.5), "{before_tip:?} -> {after_tip:?}");
    assert!(off_edge(after_tip, page_box(&s, t)) < EPS, "the end is still on the target's edge");
    check_session(&s).unwrap();
}

#[test]
fn one_undo_takes_the_move_of_the_target_and_its_callout_back() {
    let mut s = blank_session();
    let t = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    callout_for(&mut s, t);
    let before = snapshot(&s);
    run(&mut s, "shape.move", json!({"ids": [t], "dx": 1.0, "dy": 1.0}));
    assert_eq!(snapshot(&s).undo_steps, before.undo_steps + 1, "one undo step");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(snapshot(&s).document, before.document);
}

#[test]
fn moving_the_callout_alone_leaves_the_target_and_the_end_of_the_leader_on_it() {
    let mut s = blank_session();
    let t = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    let c = callout_for(&mut s, t);
    let target_pin = pin(doc(&s), t);
    let before_tip = tip_of(&mut s, c);
    run(&mut s, "shape.move", json!({"ids": [c], "dx": 2.0, "dy": 0.0}));
    assert_eq!(pin(doc(&s), t), target_pin, "the target stays");
    let tip = tip_of(&mut s, c);
    assert!(off_edge(tip, page_box(&s, t)) < EPS, "the end is on the target's edge: {tip:?}");
    assert!(
        tip.0 >= before_tip.0 - EPS,
        "the callout went right, so the end on the target's edge nearest it does not go left: {before_tip:?} {tip:?}"
    );
    assert_eq!(target_of(&mut s, c), Some(t), "still tied");
}

#[test]
fn a_target_and_its_callout_moved_together_move_once() {
    let mut s = blank_session();
    let t = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    let c = callout_for(&mut s, t);
    let (t0, c0) = (pin(doc(&s), t), pin(doc(&s), c));
    let moved = run(&mut s, "shape.move", json!({"ids": [t, c], "dx": 1.0, "dy": 0.5}));
    assert_eq!(moved["moved"], 2);
    let (t1, c1) = (pin(doc(&s), t), pin(doc(&s), c));
    assert!(near(t1.0, t0.0 + 1.0) && near(t1.1, t0.1 + 0.5));
    assert!(near(c1.0, c0.0 + 1.0) && near(c1.1, c0.1 + 0.5), "{c0:?} -> {c1:?}");
}

#[test]
fn the_pin_set_in_the_size_and_position_window_and_align_take_the_callouts_along() {
    let mut s = blank_session();
    let t = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    let c = callout_for(&mut s, t);
    let (t0, c0) = (pin(doc(&s), t), pin(doc(&s), c));
    run(&mut s, "shape.place", json!({"ids": [t], "x": 6.0, "y": 2.0}));
    let (t1, c1) = (pin(doc(&s), t), pin(doc(&s), c));
    assert!(near(t1.0, 6.0) && near(t1.1, 2.0));
    assert!(near(c1.0, c0.0 + (6.0 - t0.0)) && near(c1.1, c0.1 + (2.0 - t0.1)), "{c0:?} -> {c1:?}");
    // Align moves the second shape to the first: its callout goes with it.
    let other = drop_shape(&mut s, "rectangle", 1.0, 6.0);
    let oc = callout_for(&mut s, other);
    let oc0 = pin(doc(&s), oc);
    run(&mut s, "shape.align", json!({"ids": [t, other], "how": "left"}));
    let oc1 = pin(doc(&s), oc);
    let moved_by = pin(doc(&s), other).0 - 1.0;
    assert!(moved_by.abs() > 1.0, "the second shape went left");
    assert!(near(oc1.0, oc0.0 + moved_by), "the callout of the shape aligned went with it: {oc0:?} -> {oc1:?}");
}

#[test]
fn a_callout_of_a_callout_follows_too_and_a_circle_in_the_file_does_not_loop() {
    let mut s = blank_session();
    let t = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    let c = callout_for(&mut s, t);
    let c2 = callout_for(&mut s, c);
    let c20 = pin(doc(&s), c2);
    run(&mut s, "shape.move", json!({"ids": [t], "dx": 1.0, "dy": 1.0}));
    let c21 = pin(doc(&s), c2);
    assert!(near(c21.0, c20.0 + 1.0) && near(c21.1, c20.1 + 1.0), "{c20:?} -> {c21:?}");
    // A circle: the target names the callout as its callout and the callout names the target as its callout. Moving ends.
    s.edit("test: a circle", |d| {
        let page = std::sync::Arc::make_mut(d.pages.get_mut(0).unwrap());
        for (id, other) in [(t, c), (c, t)] {
            let at = page.shapes.iter().position(|sh| sh.id == id).unwrap();
            let shape = std::sync::Arc::make_mut(page.shapes.get_mut(at).unwrap());
            let sheet = std::sync::Arc::make_mut(&mut shape.sheet);
            sheet.cells.insert(
                "Relationships".into(),
                diacraft_doc::Cell::with_formula(&format!("SUM(DEPENDSON(3,Sheet.{other}!SheetRef(),Sheet.{id}!SheetRef()))"), None),
            );
        }
        Ok(())
    })
    .unwrap();
    run(&mut s, "shape.move", json!({"ids": [t], "dx": 0.5, "dy": 0.0}));
    check_session(&s).unwrap();
}

#[test]
fn deleting_the_target_deletes_its_callouts_and_one_undo_brings_all_back() {
    let mut s = blank_session();
    let t = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    let keep = drop_shape(&mut s, "rectangle", 7.0, 7.0);
    let c = callout_for(&mut s, t);
    let c2 = callout_for(&mut s, c);
    let before = snapshot(&s);
    let deleted = run(&mut s, "shape.delete", json!({"ids": [t]}));
    assert_eq!(deleted["deleted"], 1);
    let d = doc(&s);
    for gone in [t, c, c2] {
        assert!(d.find_shape(Owner::Page(1), gone).is_none(), "shape {gone} is gone");
    }
    assert!(d.find_shape(Owner::Page(1), keep).is_some());
    assert_eq!(snapshot(&s).undo_steps, before.undo_steps + 1);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(snapshot(&s).document, before.document);
    assert_eq!(callouts_of(&mut s, t), [c]);
    check_session(&s).unwrap();
}

#[test]
fn deleting_a_callout_takes_its_name_out_of_the_target_and_leaves_the_target() {
    let mut s = blank_session();
    let t = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    let c = callout_for(&mut s, t);
    let c2 = callout_for(&mut s, t);
    run(&mut s, "shape.delete", json!({"ids": [c]}));
    assert_eq!(callouts_of(&mut s, t), [c2], "the other callout stays tied");
    let rel = relationships(doc(&s), t).unwrap();
    assert!(!rel.contains(&format!("Sheet.{c}!")), "no cell names a shape that is not there: {rel}");
    run(&mut s, "shape.delete", json!({"ids": [c2]}));
    assert!(callouts_of(&mut s, t).is_empty());
    assert!(doc(&s).find_shape(Owner::Page(1), t).is_some());
    check_session(&s).unwrap();
}

#[test]
fn copying_the_target_copies_the_callout_tied_to_the_copy_and_the_original_keeps_its_own() {
    let mut s = blank_session();
    let t = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    let c = callout_for(&mut s, t);
    let copied = run(&mut s, "edit.copy", json!({"ids": [t]}));
    assert_eq!(copied["copied"], 2, "the shape and its callout");
    let pasted = run(&mut s, "edit.paste", json!({}));
    let ids: Vec<u32> = pasted["ids"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap() as u32).collect();
    assert_eq!(ids.len(), 2);
    let (t2, c2) = (ids[0], ids[1]);
    assert_eq!(callouts_of(&mut s, t2), [c2], "the copy of the target has the copy of the callout");
    assert_eq!(target_of(&mut s, c2), Some(t2));
    assert_eq!(callouts_of(&mut s, t), [c], "the original keeps its callout and no other");
    assert_eq!(target_of(&mut s, c), Some(t));
    // The copy's end is on the copy's edge.
    let tip = tip_of(&mut s, c2);
    assert!(off_edge(tip, page_box(&s, t2)) < EPS, "{tip:?}");
    // Moving the copy moves its callout and not the original's.
    let (c0, o0) = (pin(doc(&s), c2), pin(doc(&s), c));
    run(&mut s, "shape.move", json!({"ids": [t2], "dx": 1.0, "dy": 0.0}));
    assert!(near(pin(doc(&s), c2).0, c0.0 + 1.0));
    assert_eq!(pin(doc(&s), c), o0);
    check_session(&s).unwrap();
}

#[test]
fn duplicating_and_cutting_and_pasting_the_target_take_the_callout_too() {
    let mut s = blank_session();
    let t = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    let c = callout_for(&mut s, t);
    let dup = run(&mut s, "shape.duplicate", json!({"ids": [t]}));
    let ids: Vec<u32> = dup["ids"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap() as u32).collect();
    assert_eq!(ids.len(), 2);
    assert_eq!(callouts_of(&mut s, ids[0]), [ids[1]]);
    // A cut takes the shape and the callout and a paste puts both back tied to each other.
    run(&mut s, "edit.cut", json!({"ids": [t]}));
    assert!(doc(&s).find_shape(Owner::Page(1), t).is_none() && doc(&s).find_shape(Owner::Page(1), c).is_none());
    let pasted = run(&mut s, "edit.paste", json!({"x": 6.0, "y": 6.0}));
    let ids: Vec<u32> = pasted["ids"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap() as u32).collect();
    assert_eq!(ids.len(), 2);
    assert_eq!(callouts_of(&mut s, ids[0]), [ids[1]]);
    assert_eq!(target_of(&mut s, ids[1]), Some(ids[0]));
    check_session(&s).unwrap();
}

#[test]
fn a_callout_copied_alone_is_tied_to_nothing_and_the_original_keeps_its_tie() {
    let mut s = blank_session();
    let t = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    let c = callout_for(&mut s, t);
    run(&mut s, "edit.copy", json!({"ids": [c]}));
    let pasted = run(&mut s, "edit.paste", json!({}));
    let copy = pasted["ids"][0].as_u64().unwrap() as u32;
    assert_eq!(pasted["ids"].as_array().unwrap().len(), 1);
    assert_eq!(target_of(&mut s, copy), None);
    assert_eq!(list(&mut s, copy)["is_callout"], true);
    assert_eq!(callouts_of(&mut s, t), [c], "the target names the original only");
    assert_eq!(target_of(&mut s, c), Some(t));
    // The copy holds its end where it was in its own frame: the same distance from the copy's pin as the original's end is from the original's pin.
    let (ct, ot) = (tip_of(&mut s, copy), tip_of(&mut s, c));
    let (cp, op) = (pin(doc(&s), copy), pin(doc(&s), c));
    assert!(near(ct.0 - cp.0, ot.0 - op.0) && near(ct.1 - cp.1, ot.1 - op.1), "{ct:?} {cp:?} {ot:?} {op:?}");
    check_session(&s).unwrap();
}

#[test]
fn a_callout_is_tied_to_another_shape_and_untied_and_the_old_target_forgets_it() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 2.0, 2.0);
    let b = drop_shape(&mut s, "ellipse", 7.0, 6.0);
    let c = id_of(&run(&mut s, "callout.insert", json!({"x": 4.5, "y": 4.0})));
    let attached = run(&mut s, "callout.attach", json!({"id": c, "target": a}));
    assert_eq!(attached["changed"], true);
    assert_eq!(callouts_of(&mut s, a), [c]);
    let again = run(&mut s, "callout.attach", json!({"id": c, "target": a}));
    assert_eq!(again["changed"], false, "a tie that is made is no new step");
    // The end goes to the edge of the target nearest the callout.
    assert!(off_edge(tip_of(&mut s, c), page_box(&s, a)) < EPS);
    // Re-tied to another shape: the first forgets it.
    run(&mut s, "callout.attach", json!({"id": c, "target": b}));
    assert!(callouts_of(&mut s, a).is_empty());
    assert_eq!(callouts_of(&mut s, b), [c]);
    assert_eq!(target_of(&mut s, c), Some(b));
    assert!(off_edge(tip_of(&mut s, c), page_box(&s, b)) < EPS);
    // Moving the old target moves nothing of the callout now.
    let c0 = pin(doc(&s), c);
    run(&mut s, "shape.move", json!({"ids": [a], "dx": 1.0, "dy": 1.0}));
    assert_eq!(pin(doc(&s), c), c0);
    // Untied with a place: the end is at the place and stays when the callout moves.
    let detached = run(&mut s, "callout.detach", json!({"id": c, "x": 1.0, "y": 8.0}));
    assert_eq!(detached["changed"], true);
    assert!(callouts_of(&mut s, b).is_empty());
    assert_eq!(target_of(&mut s, c), None);
    let (x, y) = tip_of(&mut s, c);
    assert!(near(x, 1.0) && near(y, 8.0), "{x} {y}");
    run(&mut s, "shape.move", json!({"ids": [c], "dx": 1.0, "dy": 0.0}));
    let (x, y) = tip_of(&mut s, c);
    assert!(near(x, 2.0) && near(y, 8.0), "the end of a callout tied to nothing moves with it: {x} {y}");
    // Untying one that is tied to nothing, with no place, is no step.
    let steps = snapshot(&s).undo_steps;
    let none = run(&mut s, "callout.detach", json!({"id": c}));
    assert_eq!(none["changed"], false);
    assert_eq!(snapshot(&s).undo_steps, steps);
    check_session(&s).unwrap();
}

#[test]
fn untying_with_no_place_leaves_the_end_where_it_is() {
    let mut s = blank_session();
    let t = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    let c = callout_for(&mut s, t);
    let tip = tip_of(&mut s, c);
    run(&mut s, "callout.detach", json!({"id": c}));
    let after = tip_of(&mut s, c);
    assert!(near(tip.0, after.0) && near(tip.1, after.1), "{tip:?} {after:?}");
    assert_eq!(target_of(&mut s, c), None);
    assert!(callouts_of(&mut s, t).is_empty());
}

#[test]
fn a_target_that_is_not_allowed_is_refused_in_words_and_nothing_changes() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 2.0, 2.0);
    let b = drop_shape(&mut s, "rectangle", 5.0, 2.0);
    let c = id_of(&run(&mut s, "callout.insert", json!({"x": 4.5, "y": 4.0})));
    let before = snapshot(&s);
    assert!(refused(&mut s, "callout.attach", json!({"id": c, "target": c})).contains("itself"));
    assert!(refused(&mut s, "callout.attach", json!({"id": a, "target": b})).contains("not a callout"));
    assert!(refused(&mut s, "callout.attach", json!({"id": c, "target": 99})).contains("no shape 99"));
    // A group's member is no target: tie the group.
    let g = id_of(&run(&mut s, "shape.group", json!({"ids": [a, b]})));
    let member = a;
    assert!(refused(&mut s, "callout.attach", json!({"id": c, "target": member})).contains("inside a group"));
    assert!(refused(&mut s, "callout.insert", json!({"ids": [member]})).contains("inside a group"));
    run(&mut s, "callout.attach", json!({"id": c, "target": g}));
    assert_eq!(callouts_of(&mut s, g), [c]);
    // Both a callout for shapes and a place, a place with one coordinate and a style that is not there are refused.
    assert!(refused(&mut s, "callout.insert", json!({"ids": [g], "x": 1.0, "y": 1.0})).contains("not both"));
    assert!(refused(&mut s, "callout.insert", json!({"x": 1.0})).contains("go together"));
    assert!(refused(&mut s, "callout.insert", json!({"style": "nothing"})).contains("no callout style"));
    assert!(refused(&mut s, "callout.insert", json!({"ids": [999]})).contains("no shape 999"));
    assert!(refused(&mut s, "callout.detach", json!({"id": c, "x": 1.0})).contains("go together"));
    assert!(refused(&mut s, "callout.detach", json!({"id": g})).contains("not a callout"));
    // A callout of a callout of a callout that points back is a circle.
    let c1 = callout_for(&mut s, g);
    let c2 = callout_for(&mut s, c1);
    assert!(refused(&mut s, "callout.attach", json!({"id": c1, "target": c2})).contains("circle"));
    assert!(before.document.is_some());
    check_session(&s).unwrap();
}

#[test]
fn a_refusal_changes_nothing_and_adds_no_step() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 2.0, 2.0);
    let before = snapshot(&s);
    refused(&mut s, "callout.insert", json!({"ids": [a, 999]}));
    assert_eq!(snapshot(&s).document, before.document);
    assert_eq!(snapshot(&s).undo_steps, before.undo_steps);
}

#[test]
fn a_shape_has_at_most_256_callouts() {
    let mut s = blank_session();
    let t = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    for _ in 0..256 {
        run(&mut s, "callout.insert", json!({"ids": [t], "style": "text_line"}));
    }
    assert_eq!(callouts_of(&mut s, t).len(), 256);
    let before = snapshot(&s);
    assert!(refused(&mut s, "callout.insert", json!({"ids": [t]})).contains("256"));
    assert_eq!(snapshot(&s).document, before.document);
}

#[test]
fn every_tile_makes_a_callout_whose_leader_follows_the_target() {
    let mut s = blank_session();
    let styles = run(&mut s, "callout.styles", json!({}));
    for tile in styles["styles"].as_array().unwrap() {
        let id = tile["id"].as_str().unwrap();
        let t = drop_shape(&mut s, "rectangle", 3.0, 3.0);
        let c = id_of(&run(&mut s, "callout.insert", json!({"ids": [t], "style": id})));
        let d = doc(&s);
        // The shape has geometry, and a figure whose cells all hold numbers worked out: no error.
        let shape = d.shape(&d.find_shape(Owner::Page(1), c).unwrap()).unwrap();
        let geometry: Vec<_> = shape.sheet.sections.iter().filter(|sec| sec.name.as_str() == "Geometry").collect();
        assert!(!geometry.is_empty(), "{id}");
        for section in geometry {
            for row in section.rows.iter() {
                for (name, cell) in row.cells.iter() {
                    assert!(cell.error.is_none(), "{id}: geometry {name} has the error {:?}", cell.error);
                    assert!(cell.value.as_ref().and_then(|v| v.number()).is_some(), "{id}: geometry {name} holds no number: {:?}", cell.value);
                }
            }
        }
        let tip = tip_of(&mut s, c);
        assert!(off_edge(tip, page_box(&s, t)) < EPS, "{id}: {tip:?}");
        run(&mut s, "shape.move", json!({"ids": [t], "dx": 0.5, "dy": 0.25}));
        let moved = tip_of(&mut s, c);
        assert!(near(moved.0, tip.0 + 0.5) && near(moved.1, tip.1 + 0.25), "{id}: {tip:?} {moved:?}");
        // The leader reaches the end: some Geometry row of the callout lies at the end in its own frame. A thought cloud's leader is two small circles, the last of which is on the way to it.
        let (lx, ly) = (scratch(doc(&s), c, "X"), scratch(doc(&s), c, "Y"));
        let shape = doc(&s).shape(&doc(&s).find_shape(Owner::Page(1), c).unwrap()).unwrap().clone();
        let rows: Vec<_> =
            shape.sheet.sections.iter().filter(|sec| sec.name.as_str() == "Geometry").flat_map(|sec| sec.rows.iter().cloned()).collect();
        let at = |row: &diacraft_doc::Row, n: &str| row.cells.get(n).and_then(|c| c.value.as_ref()).and_then(|v| v.number()).unwrap_or(f64::NAN);
        let reaches = rows
            .iter()
            .filter(|row| matches!(row.row_type.as_deref(), Some("LineTo" | "MoveTo")))
            .any(|row| near(at(row, "X"), lx) && near(at(row, "Y"), ly));
        if id == "thought_cloud" {
            let (sx, sy) = (scratch_row(doc(&s), c, 1, "X"), scratch_row(doc(&s), c, 1, "Y"));
            let whole = ((lx - sx).powi(2) + (ly - sy).powi(2)).sqrt();
            let circles: Vec<f64> = rows
                .iter()
                .filter(|row| row.row_type.as_deref() == Some("Ellipse"))
                .map(|row| ((at(row, "X") - lx).powi(2) + (at(row, "Y") - ly).powi(2)).sqrt())
                .collect();
            assert_eq!(circles.len(), 2, "{id}: two small circles lead to the end");
            assert!(
                circles[1] < circles[0] && circles[1] <= 0.45 * whole + EPS,
                "{id}: the circles are on the way to the end: {circles:?} of {whole}"
            );
        } else {
            assert!(reaches, "{id}: no row of the figure reaches the end ({lx}, {ly})");
        }
    }
}

#[test]
fn a_callout_on_a_turned_and_flipped_callout_still_points_at_the_target() {
    let mut s = blank_session();
    let t = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    let c = callout_for(&mut s, t);
    run(&mut s, "shape.rotate", json!({"ids": [c], "angle": 30.0}));
    run(&mut s, "shape.flip", json!({"ids": [c], "axis": "horizontal"}));
    let tip = tip_of(&mut s, c);
    assert!(off_edge(tip, page_box(&s, t)) < EPS, "{tip:?}");
    run(&mut s, "shape.move", json!({"ids": [t], "dx": 1.0, "dy": 1.0}));
    let moved = tip_of(&mut s, c);
    assert!(near(moved.0, tip.0 + 1.0) && near(moved.1, tip.1 + 1.0), "{tip:?} {moved:?}");
}

#[test]
fn one_callout_for_each_of_several_shapes_is_one_undo_step() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 2.0, 2.0);
    let b = drop_shape(&mut s, "rectangle", 5.0, 2.0);
    let before = snapshot(&s);
    let made = run(&mut s, "callout.insert", json!({"ids": [a, b, a]}));
    assert_eq!(made["ids"].as_array().unwrap().len(), 2, "a shape named twice gets one");
    assert_eq!(snapshot(&s).undo_steps, before.undo_steps + 1);
    let c: Vec<u32> = made["ids"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap() as u32).collect();
    assert_eq!(callouts_of(&mut s, a), [c[0]]);
    assert_eq!(callouts_of(&mut s, b), [c[1]]);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(snapshot(&s).document, before.document);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(callouts_of(&mut s, a), [c[0]]);
}

#[test]
fn a_page_with_a_theme_gives_the_callout_its_look() {
    let mut s = blank_session();
    run(&mut s, "theme.apply", json!({"theme": "tidepool", "page": 1}));
    let t = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    let c = callout_for(&mut s, t);
    let d = doc(&s);
    let callout = d.shape(&d.find_shape(Owner::Page(1), c).unwrap()).unwrap();
    // The links to the root style (0) are the theme's style now, for the three families, as a shape dropped on the page has them.
    for (family, link) in [("line", callout.sheet.links.line), ("fill", callout.sheet.links.fill), ("text", callout.sheet.links.text)] {
        assert!(matches!(link, Some(n) if n != 0), "the {family} follows the theme: {link:?}");
    }
}

#[test]
fn a_target_changed_to_another_master_keeps_its_callouts_and_they_still_follow_it() {
    let mut s = blank_session();
    let t = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    let c = callout_for(&mut s, t);
    run(&mut s, "shape.change", json!({"ids": [t], "master": "ellipse"}));
    assert_eq!(callouts_of(&mut s, t), [c], "the shape keeps its ID and its Relationships");
    assert_eq!(target_of(&mut s, c), Some(t));
    let tip0 = tip_of(&mut s, c);
    assert!(off_edge(tip0, page_box(&s, t)) < EPS, "the end of the leader is on the edge of the box of the shape as it is now: {tip0:?}");
    let c0 = pin(doc(&s), c);
    run(&mut s, "shape.move", json!({"ids": [t], "dx": 1.0, "dy": 0.0}));
    assert!(near(pin(doc(&s), c).0, c0.0 + 1.0), "the callout still goes with it");
    check_session(&s).unwrap();
}
