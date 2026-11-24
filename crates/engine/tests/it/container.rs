//! Containers: `container.add` and its settings, who is a member when shapes are dropped, moved, copied and deleted, Fit to Contents and the automatic resizing,
//! locking, disbanding, the shapes on an edge, and what a file holds. Through `Session::execute` as the control channel, the CLI and MCP call. Numbers are paper inches on a page
//! of the default scale, where they are the cells' own. Each test names the row of
//! it holds to.

use diacraft_doc::{Document, Owner};
use diacraft_engine::{EngineError, Session};
use serde_json::{Value as Json, json};

use crate::edit::{doc, drop_shape, id_of, num};
use crate::{bad_params_msg, blank_session, check_session, run, snapshot};

/// The `Relationships` formula a shape holds of its own, or nothing.
pub(crate) fn ties(d: &Document, id: u32) -> String {
    let at = d.find_shape(Owner::Page(1), id).unwrap_or_else(|| panic!("no shape {id}"));
    d.shape(&at).and_then(|s| s.sheet.cell("Relationships").and_then(|c| c.formula.text().map(str::to_owned))).unwrap_or_default()
}

/// What `container.info` says of the containers among `ids`.
pub(crate) fn info(s: &mut Session, ids: &[u32]) -> Vec<Json> {
    run(s, "container.info", json!({ "ids": ids }))["containers"].as_array().cloned().unwrap_or_default()
}

pub(crate) fn members(s: &mut Session, id: u32) -> Vec<u32> {
    let all = info(s, &[id]);
    all.first().map(|c| c["members"].as_array().unwrap().iter().map(|m| u32::try_from(m.as_u64().unwrap()).unwrap()).collect()).unwrap_or_default()
}

/// The box of a shape on page 1 (not turned): left, bottom, right, top.
pub(crate) fn boxed(d: &Document, id: u32) -> (f64, f64, f64, f64) {
    let (w, h) = (num(d, id, "Width"), num(d, id, "Height"));
    let (px, py) = (num(d, id, "PinX"), num(d, id, "PinY"));
    let (lx, ly) = (num(d, id, "LocPinX"), num(d, id, "LocPinY"));
    (px - lx, py - ly, px - lx + w, py - ly + h)
}

pub(crate) fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1.0e-6
}

pub(crate) fn assert_box(what: &str, got: (f64, f64, f64, f64), want: (f64, f64, f64, f64)) {
    assert!(near(got.0, want.0) && near(got.1, want.1) && near(got.2, want.2) && near(got.3, want.3), "{what}: got {got:?}, want {want:?}");
}

/// The order of the page's own list, back to front.
pub(crate) fn order(d: &Document) -> Vec<u32> {
    d.pages.iter().find(|p| p.id == 1).unwrap().shapes.iter().map(|s| s.id).collect()
}

pub(crate) fn add_container(s: &mut Session, params: Json) -> u32 {
    id_of(&run(s, "container.add", params))
}

/// A page with two squares side by side, and a container round them. → (session, the squares, the container)
pub(crate) fn two_in_a_container() -> (Session, u32, u32, u32) {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    let b = drop_shape(&mut s, "rectangle", 6.0, 4.0);
    let c = add_container(&mut s, json!({"ids": [a, b], "heading": "Stage"}));
    (s, a, b, c)
}

// ---- adding a container ------------------------------------------------------------------------------------------------------------------------------------------

#[test]
fn a_container_added_round_shapes_holds_them_and_is_the_box_that_holds_them_with_a_margin_and_the_strip() {
    // "with shapes selected, the container is placed around them and contains them".
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    let b = drop_shape(&mut s, "rectangle", 6.0, 4.0);
    let (ba, bb) = (boxed(doc(&s), a), boxed(doc(&s), b));
    let made = run(&mut s, "container.add", json!({"ids": [a, b], "heading": "Stage"}));
    let c = id_of(&made);
    assert_eq!(made["members"], json!([a, b]));
    assert_eq!(made["style"], "strip");
    let d = doc(&s);
    // The margin is a quarter inch all round and the strip adds 0.3 inch on the heading's side (the top).
    let (x0, y0, x1, y1) = (ba.0.min(bb.0), ba.1.min(bb.1), ba.2.max(bb.2), ba.3.max(bb.3));
    assert_box("the container", boxed(d, c), (x0 - 0.25, y0 - 0.25, x1 + 0.25, y1 + 0.25 + 0.3));
    // Both sides of the tie are written, and a call this code does not own is not made.
    assert_eq!(ties(d, c), format!("SUM(DEPENDSON(1,Sheet.{a}!SheetRef(),Sheet.{b}!SheetRef()))"));
    assert_eq!(ties(d, a), format!("SUM(DEPENDSON(4,Sheet.{c}!SheetRef()))"));
    assert_eq!(ties(d, b), format!("SUM(DEPENDSON(4,Sheet.{c}!SheetRef()))"));
    // It draws behind what it holds: it is at the back of the page.
    assert_eq!(order(d)[0], c);
    // It is a container to the model's own functions and to the selection by role.
    let by_role = run(&mut s, "select.by_type", json!({"roles": ["container"]}));
    assert_eq!(by_role["ids"], json!([c]));
    assert_eq!(members(&mut s, c), [a, b]);
    check_session(&s).unwrap();
}

#[test]
fn a_container_added_with_nothing_selected_is_dropped_in_the_middle_and_holds_nothing_even_over_shapes() {
    // "with nothing selected it is dropped in the middle of the view and contains nothing, even if shapes sit inside it".
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 4.25, 5.5);
    let made = run(&mut s, "container.add", json!({"x": 4.25, "y": 5.5, "width": 4.0, "height": 3.0}));
    let c = id_of(&made);
    assert_eq!(made["members"], json!([]));
    assert_box("the container", boxed(doc(&s), c), (2.25, 4.0, 6.25, 7.0));
    assert_eq!(ties(doc(&s), a), "", "the shape that lies inside is not a member");
    assert_eq!(members(&mut s, c), Vec::<u32>::new());
    // It still says it is a container, by its role.
    assert_eq!(run(&mut s, "select.by_type", json!({"roles": ["container"]}))["ids"], json!([c]));
}

#[test]
fn a_container_is_added_in_one_undo_step_and_undo_takes_it_and_the_ties_back() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    let before = snapshot(&s);
    run(&mut s, "container.add", json!({"ids": [a]}));
    assert_eq!(snapshot(&s).undo_steps, before.undo_steps + 1);
    assert_ne!(ties(doc(&s), a), "");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(ties(doc(&s), a), "");
    assert_eq!(order(doc(&s)), [a]);
    run(&mut s, "edit.redo", json!({}));
    assert_ne!(ties(doc(&s), a), "");
}

#[test]
fn every_style_of_the_gallery_makes_a_container_and_an_unknown_one_is_refused_in_words() {
    let mut s = blank_session();
    let styles = run(&mut s, "container.styles", json!({}));
    let ids: Vec<String> = styles["styles"].as_array().unwrap().iter().map(|x| x["id"].as_str().unwrap().to_owned()).collect();
    assert_eq!(ids.len(), 14);
    for style in &ids {
        let c = add_container(&mut s, json!({"style": style, "heading": "H"}));
        assert_eq!(info(&mut s, &[c])[0]["style"], json!(style));
    }
    let before = snapshot(&s);
    let e = s.execute("container.add", &json!({"style": "no-such"})).unwrap_err();
    assert!(bad_params_msg(&e).contains("no container style `no-such`") && bad_params_msg(&e).contains("strip"), "{e}");
    assert_eq!(snapshot(&s), before, "a refusal leaves the session as it was");
}

#[test]
fn a_container_is_not_put_round_shapes_that_are_not_on_the_page_or_that_a_locked_container_holds() {
    let (mut s, a, _b, c) = two_in_a_container();
    run(&mut s, "container.lock", json!({"ids": [c], "locked": true}));
    let before = snapshot(&s);
    let e = s.execute("container.add", &json!({"ids": [a]})).unwrap_err();
    assert!(e.to_string().contains("locked container") && e.to_string().contains("unlock"), "{e}");
    let e = s.execute("container.add", &json!({"ids": [9999]})).unwrap_err();
    assert!(matches!(e, EngineError::BadParams { .. }), "{e}");
    assert_eq!(snapshot(&s), before);
}

// ---- membership as shapes move --------------------------------------------------------------------------------------------------------------------------------

#[test]
fn a_shape_dropped_inside_a_container_becomes_a_member_and_one_dropped_outside_does_not() {
    // "Shapes are added by dropping them inside".
    let (mut s, a, b, c) = two_in_a_container();
    let inside = drop_shape(&mut s, "rectangle", 4.5, 4.0);
    let outside = drop_shape(&mut s, "rectangle", 4.5, 9.0);
    assert_eq!(members(&mut s, c), [a, b, inside]);
    assert_eq!(ties(doc(&s), inside), format!("SUM(DEPENDSON(4,Sheet.{c}!SheetRef()))"));
    assert_eq!(ties(doc(&s), outside), "");
}

#[test]
fn a_shape_moved_into_a_container_joins_it_and_one_moved_out_leaves_it() {
    // "moving a shape a little inside the container ... makes the membership".
    let (mut s, a, _b, c) = two_in_a_container();
    let loose = drop_shape(&mut s, "rectangle", 4.5, 9.0);
    run(&mut s, "shape.move", json!({"ids": [loose], "dx": 0.0, "dy": -5.0}));
    assert!(members(&mut s, c).contains(&loose), "moved in: {:?}", info(&mut s, &[c]));
    run(&mut s, "shape.move", json!({"ids": [loose], "dx": 0.0, "dy": 5.0}));
    assert!(!members(&mut s, c).contains(&loose), "moved out");
    assert_eq!(ties(doc(&s), loose), "", "no tie is left on the shape");
    // A member moved but kept inside stays a member.
    run(&mut s, "shape.move", json!({"ids": [a], "dx": 0.1, "dy": 0.0}));
    assert!(members(&mut s, c).contains(&a));
}

#[test]
fn a_shape_moved_into_an_inner_container_leaves_the_outer_one_and_the_inner_is_a_member_of_the_outer() {
    let mut s = blank_session();
    let outer = add_container(&mut s, json!({"x": 4.25, "y": 5.5, "width": 7.0, "height": 6.0}));
    let inner = add_container(&mut s, json!({"x": 4.25, "y": 5.0, "width": 3.0, "height": 2.0}));
    assert_eq!(members(&mut s, outer), [inner], "the inner container is dropped inside the outer one");
    let a = drop_shape(&mut s, "rectangle", 1.5, 7.5);
    assert_eq!(members(&mut s, outer), [inner, a]);
    run(&mut s, "shape.move", json!({"ids": [a], "dx": 2.75, "dy": -2.5}));
    assert_eq!(members(&mut s, inner), [a]);
    assert_eq!(members(&mut s, outer), [inner]);
    assert_eq!(ties(doc(&s), a), format!("SUM(DEPENDSON(4,Sheet.{inner}!SheetRef()))"));
}

#[test]
fn moving_a_container_moves_what_it_holds_and_keeps_the_ties() {
    // "Moving, copying or deleting a container does the same to its contents".
    let (mut s, a, b, c) = two_in_a_container();
    let (ba, bb, bc) = (boxed(doc(&s), a), boxed(doc(&s), b), boxed(doc(&s), c));
    let moved = run(&mut s, "shape.move", json!({"ids": [c], "dx": 1.0, "dy": -2.0}));
    assert_eq!(moved["moved"], 1, "the shape named: what goes with it is not counted, as the callouts tied to a shape are not");
    let d = doc(&s);
    assert_box("a", boxed(d, a), (ba.0 + 1.0, ba.1 - 2.0, ba.2 + 1.0, ba.3 - 2.0));
    assert_box("b", boxed(d, b), (bb.0 + 1.0, bb.1 - 2.0, bb.2 + 1.0, bb.3 - 2.0));
    assert_box("the container", boxed(d, c), (bc.0 + 1.0, bc.1 - 2.0, bc.2 + 1.0, bc.3 - 2.0));
    assert_eq!(members(&mut s, c), [a, b], "still its members");
    // Moving the container and a member together moves the member once.
    run(&mut s, "shape.move", json!({"ids": [c, a], "dx": 0.5, "dy": 0.0}));
    assert!(near(boxed(doc(&s), a).0, ba.0 + 1.5));
}

#[test]
fn moving_an_outer_container_moves_the_inner_one_and_what_it_holds() {
    let mut s = blank_session();
    let outer = add_container(&mut s, json!({"x": 4.25, "y": 5.5, "width": 7.0, "height": 6.0}));
    let inner = add_container(&mut s, json!({"x": 4.25, "y": 5.0, "width": 3.0, "height": 2.0}));
    let a = drop_shape(&mut s, "rectangle", 4.25, 5.0);
    assert_eq!(members(&mut s, inner), [a]);
    let before = (boxed(doc(&s), a), boxed(doc(&s), inner));
    run(&mut s, "shape.move", json!({"ids": [outer], "dx": 0.0, "dy": 1.0}));
    assert!(near(boxed(doc(&s), a).1, before.0.1 + 1.0));
    assert!(near(boxed(doc(&s), inner).1, before.1.1 + 1.0));
    assert_eq!(members(&mut s, outer), [inner]);
}

#[test]
fn a_page_with_no_container_moves_shapes_as_it_did() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    let before = boxed(doc(&s), a);
    let r = run(&mut s, "shape.move", json!({"ids": [a], "dx": 1.0, "dy": 0.0}));
    assert_eq!(r["moved"], 1);
    assert!(near(boxed(doc(&s), a).0, before.0 + 1.0));
    assert_eq!(ties(doc(&s), a), "");
}

// ---- delete, disband, copy ----------------------------------------------------------------------------------------------------------------------------------

#[test]
fn deleting_a_container_deletes_what_it_holds_and_disbanding_it_keeps_the_shapes() {
    // Delete removes all four, Disband Container removes only the container.
    let (mut s, a, b, c) = two_in_a_container();
    run(&mut s, "container.disband", json!({"ids": [c]}));
    assert_eq!(order(doc(&s)), [a, b], "the container is gone and the two shapes stay");
    assert_eq!(ties(doc(&s), a), "", "and they hold no tie to it");
    let (mut s, a, b, c) = two_in_a_container();
    let r = run(&mut s, "shape.delete", json!({"ids": [c]}));
    assert_eq!(r["deleted"], 3);
    assert!(order(doc(&s)).is_empty(), "{:?} {a} {b}", order(doc(&s)));
    check_session(&s).unwrap();
}

#[test]
fn a_shape_deleted_leaves_the_container_it_was_in_and_no_tie_names_it() {
    // And the ID-reuse hazard: a tie left naming a deleted shape would name the next shape given its ID after a save and a reopening.
    let (mut s, a, b, c) = two_in_a_container();
    run(&mut s, "shape.delete", json!({"ids": [a]}));
    assert_eq!(ties(doc(&s), c), format!("SUM(DEPENDSON(1,Sheet.{b}!SheetRef()))"));
    run(&mut s, "shape.delete", json!({"ids": [b]}));
    assert_eq!(ties(doc(&s), c), "SUM(DEPENDSON(1))", "a container with no member is still one");
    assert_eq!(info(&mut s, &[c]).len(), 1);
}

#[test]
fn disbanding_a_container_inside_another_hands_its_members_to_the_outer_one() {
    let mut s = blank_session();
    let outer = add_container(&mut s, json!({"x": 4.25, "y": 5.5, "width": 7.0, "height": 6.0}));
    let inner = add_container(&mut s, json!({"x": 4.25, "y": 5.0, "width": 3.0, "height": 2.0}));
    let a = drop_shape(&mut s, "rectangle", 4.25, 5.0);
    run(&mut s, "container.disband", json!({"ids": [inner]}));
    assert_eq!(members(&mut s, outer), [a]);
    assert_eq!(ties(doc(&s), a), format!("SUM(DEPENDSON(4,Sheet.{outer}!SheetRef()))"));
}

#[test]
fn a_container_is_copied_with_what_it_holds_and_the_copies_tie_to_each_other() {
    // Copying a container copies its contents.
    let (mut s, a, b, c) = two_in_a_container();
    let r = run(&mut s, "shape.duplicate", json!({"ids": [c], "dx": 0.0, "dy": -3.0}));
    let copies: Vec<u32> = r["ids"].as_array().unwrap().iter().map(|v| u32::try_from(v.as_u64().unwrap()).unwrap()).collect();
    assert_eq!(copies.len(), 3, "the container and its two members: {r}");
    let copy_container = *copies.iter().find(|id| ties(doc(&s), **id).contains("DEPENDSON(1,")).expect("a copy that holds members");
    let held = members(&mut s, copy_container);
    assert_eq!(held.len(), 2);
    assert!(held.iter().all(|m| *m != a && *m != b), "the copy holds copies: {held:?}");
    for m in &held {
        assert_eq!(ties(doc(&s), *m), format!("SUM(DEPENDSON(4,Sheet.{copy_container}!SheetRef()))"));
    }
    // The originals are as they were.
    assert_eq!(members(&mut s, c), [a, b]);
}

#[test]
fn a_member_copied_alone_holds_no_tie_to_the_container_it_was_in_and_joins_the_one_it_lands_in() {
    let (mut s, a, _b, c) = two_in_a_container();
    // The copy lands inside the same container, so it joins it as a shape dropped there does.
    let r = run(&mut s, "shape.duplicate", json!({"ids": [a], "dx": 0.2, "dy": 0.0}));
    let copy = u32::try_from(r["ids"][0].as_u64().unwrap()).unwrap();
    assert_eq!(members(&mut s, c).len(), 3);
    assert_eq!(ties(doc(&s), copy), format!("SUM(DEPENDSON(4,Sheet.{c}!SheetRef()))"));
    // One copied far outside holds no tie at all.
    let r = run(&mut s, "shape.duplicate", json!({"ids": [a], "dx": 0.0, "dy": 5.0}));
    let far = u32::try_from(r["ids"][0].as_u64().unwrap()).unwrap();
    assert_eq!(ties(doc(&s), far), "");
}

#[test]
fn cut_and_paste_a_container_brings_it_back_with_its_members() {
    let (mut s, _a, _b, c) = two_in_a_container();
    run(&mut s, "edit.cut", json!({"ids": [c]}));
    assert!(order(doc(&s)).is_empty());
    let r = run(&mut s, "edit.paste", json!({"page": 1}));
    assert_eq!(r["ids"].as_array().unwrap().len(), 3);
    let all = order(doc(&s));
    let holder = *all.iter().find(|id| ties(doc(&s), **id).contains("DEPENDSON(1,")).unwrap();
    assert_eq!(members(&mut s, holder).len(), 2);
}
