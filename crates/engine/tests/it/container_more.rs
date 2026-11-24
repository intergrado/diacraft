//! Containers, the settings and the ways in: lock, fit, resizing, margins, styles, the heading, select contents, Add to Underlying Container, the shapes on an
//! edge, and a container whose ties are no list. Through `Session::execute`. Numbers are paper inches on a page of the default scale. Each test names the row of
//! it holds to.

use std::sync::Arc;

use diacraft_doc::{Cell, CellAddr, CellName, Document, Owner, RowKey, Value};
use diacraft_engine::{EngineError, Session};
use serde_json::json;

use crate::container::{add_container, assert_box, boxed, info, members, near, ties, two_in_a_container};
use crate::edit::{doc, drop_shape, num};
use crate::shapeops::{rect, session_with};
use crate::{bad_params_msg, blank_session, run, snapshot};

#[test]
fn a_locked_container_takes_no_member_and_a_member_cannot_leave_it_or_be_deleted() {
    // Lock Container: shapes cannot be added or removed, though they can move inside, and dropped shapes do not become members.
    let (mut s, a, _b, c) = two_in_a_container();
    assert_eq!(run(&mut s, "container.lock", json!({"ids": [c], "locked": true}))["locked"], true);
    let inside = drop_shape(&mut s, "rectangle", 4.5, 4.0);
    assert!(!members(&mut s, c).contains(&inside), "a shape dropped over a locked container does not join");
    // Inside it a member moves.
    let before_a = boxed(doc(&s), a);
    run(&mut s, "shape.move", json!({"ids": [a], "dx": 0.1, "dy": 0.0}));
    assert!(near(boxed(doc(&s), a).0, before_a.0 + 0.1));
    // It cannot be moved out, and the refusal is in words, with nothing changed.
    let before = snapshot(&s);
    let e = s.execute("shape.move", &json!({"ids": [a], "dx": 0.0, "dy": 5.0})).unwrap_err();
    assert!(e.to_string().contains("locked container") && e.to_string().contains("moved out of it"), "{e}");
    assert_eq!(snapshot(&s), before);
    // It cannot be deleted, nor the container disbanded.
    let e = s.execute("shape.delete", &json!({"ids": [a]})).unwrap_err();
    assert!(e.to_string().contains("locked container") && e.to_string().contains("deleted"), "{e}");
    let e = s.execute("container.disband", &json!({"ids": [c]})).unwrap_err();
    assert!(e.to_string().contains("locked") && e.to_string().contains("disbanded"), "{e}");
    let e = s.execute("container.add_member", &json!({"ids": [inside], "container": c})).unwrap_err();
    assert!(e.to_string().contains("locked"), "{e}");
    assert_eq!(snapshot(&s), before);
    // Delete on the container itself still deletes all it holds.
    let deleted = run(&mut s, "shape.delete", json!({"ids": [c]}));
    assert_eq!(deleted["deleted"], 3);
}

#[test]
fn lock_container_toggles_when_it_is_not_told_and_unlocking_lets_a_member_go() {
    let (mut s, a, _b, c) = two_in_a_container();
    assert_eq!(run(&mut s, "container.lock", json!({"ids": [c]}))["locked"], true);
    assert_eq!(info(&mut s, &[c])[0]["locked"], true);
    assert_eq!(run(&mut s, "container.lock", json!({"ids": [c]}))["locked"], false);
    run(&mut s, "shape.move", json!({"ids": [a], "dx": 0.0, "dy": 5.0}));
    assert!(!members(&mut s, c).contains(&a));
}

/// A container round two shapes whose resizing is none, and a member nudged so that it pokes out of the right side: its middle is inside, its right edge beyond the
/// container's. → (session, the member, the container, the container's box before)
fn poking() -> (Session, u32, u32, (f64, f64, f64, f64)) {
    let (mut s, _a, b, c) = two_in_a_container();
    let was = boxed(doc(&s), c);
    run(&mut s, "container.auto_resize", json!({"ids": [c], "mode": "none"}));
    run(&mut s, "shape.move", json!({"ids": [b], "dx": 0.6, "dy": 0.0}));
    (s, b, c, was)
}

#[test]
fn a_container_whose_resizing_is_none_stays_the_size_it_is_and_fit_to_contents_makes_it_hold_its_members() {
    // Fit to Contents resizes the container to just enclose its shapes; Automatic Resize None.
    let (mut s, b, c, was) = poking();
    assert_box("none leaves it", boxed(doc(&s), c), was);
    let r = run(&mut s, "container.fit", json!({"ids": [c]}));
    assert_eq!(r["resized"], json!([c]));
    let d = doc(&s);
    let (a_box, b_box) = (boxed(d, b - 1), boxed(d, b));
    let want = (a_box.0 - 0.25, a_box.1.min(b_box.1) - 0.25, b_box.2 + 0.25, a_box.3.max(b_box.3) + 0.25 + 0.3);
    assert_box("fitted", boxed(d, c), want);
    // A second fit changes nothing and is no undo step.
    let steps = snapshot(&s).undo_steps;
    let r = run(&mut s, "container.fit", json!({"ids": [c]}));
    assert_eq!(r["resized"], json!([]));
    assert_eq!(snapshot(&s).undo_steps, steps);
}

#[test]
fn expand_as_needed_grows_the_container_and_never_shrinks_it() {
    // Expand as Needed.
    let (mut s, _a, b, c) = two_in_a_container();
    let was = boxed(doc(&s), c);
    assert_eq!(info(&mut s, &[c])[0]["resize"], "expand", "the default of a container made here");
    run(&mut s, "shape.move", json!({"ids": [b], "dx": 0.6, "dy": 0.0}));
    let grown = boxed(doc(&s), c);
    assert!(near(grown.2, boxed(doc(&s), b).2 + 0.25), "it holds the member with its margin: {grown:?}");
    assert!(near(grown.0, was.0), "and grew on one side only");
    run(&mut s, "shape.move", json!({"ids": [b], "dx": -0.6, "dy": 0.0}));
    assert_box("it did not shrink", boxed(doc(&s), c), grown);
}

#[test]
fn always_fit_to_contents_follows_the_members_both_ways() {
    let (mut s, _a, b, c) = two_in_a_container();
    let was = boxed(doc(&s), c);
    run(&mut s, "container.auto_resize", json!({"ids": [c], "mode": "fit"}));
    run(&mut s, "shape.move", json!({"ids": [b], "dx": 0.6, "dy": 0.0}));
    let grown = boxed(doc(&s), c);
    assert!(grown.2 > was.2 + 0.5);
    run(&mut s, "shape.move", json!({"ids": [b], "dx": -0.6, "dy": 0.0}));
    assert_box("back to the box it had", boxed(doc(&s), c), was);
}

#[test]
fn a_member_resized_makes_its_container_follow_and_a_deleted_member_makes_a_fitted_one_shrink() {
    let (mut s, a, b, c) = two_in_a_container();
    run(&mut s, "container.auto_resize", json!({"ids": [c], "mode": "fit"}));
    let ba = boxed(doc(&s), a);
    run(&mut s, "shape.resize", json!({"id": a, "x": ba.0, "y": ba.1, "width": 3.0, "height": ba.3 - ba.1}));
    assert!(near(boxed(doc(&s), c).2, boxed(doc(&s), b).2 + 0.25), "{:?}", boxed(doc(&s), c));
    run(&mut s, "shape.delete", json!({"ids": [a]}));
    let bb = boxed(doc(&s), b);
    assert_box("it holds only what is left", boxed(doc(&s), c), (bb.0 - 0.25, bb.1 - 0.25, bb.2 + 0.25, bb.3 + 0.25 + 0.3));
}

#[test]
fn the_margins_are_presets_or_inches_and_the_container_is_brought_to_them() {
    let (mut s, a, b, c) = two_in_a_container();
    let (ba, bb) = (boxed(doc(&s), a), boxed(doc(&s), b));
    run(&mut s, "container.margin", json!({"ids": [c], "margin": "wide"}));
    assert_box("wide (0.5 inch) grows it", boxed(doc(&s), c), (ba.0 - 0.5, ba.1.min(bb.1) - 0.5, bb.2 + 0.5, ba.3.max(bb.3) + 0.5 + 0.3));
    run(&mut s, "container.auto_resize", json!({"ids": [c], "mode": "fit"}));
    run(&mut s, "container.margin", json!({"ids": [c], "margin": 0.0}));
    assert_box("none, always fit", boxed(doc(&s), c), (ba.0, ba.1.min(bb.1), bb.2, ba.3.max(bb.3) + 0.3));
    assert_eq!(info(&mut s, &[c])[0]["margin"], 0.0);
    let before = snapshot(&s);
    for bad in [json!("huge"), json!(-1.0), json!(null)] {
        let e = s.execute("container.margin", &json!({"ids": [c], "margin": bad})).unwrap_err();
        assert!(matches!(e, EngineError::BadParams { .. }), "{bad}: {e}");
    }
    assert_eq!(snapshot(&s), before);
}

/// The text of a shape, as its parts are written.
fn text_of(d: &Document, id: u32) -> String {
    let at = d.find_shape(Owner::Page(1), id).unwrap();
    d.shape(&at).and_then(|s| s.text.as_ref()).map(|t| t.parts.iter().map(|p| format!("{p:?}")).collect::<String>()).unwrap_or_default()
}

#[test]
fn a_new_style_keeps_the_size_the_heading_and_the_members_and_changes_the_look() {
    // A gallery of container styles.
    let (mut s, a, b, c) = two_in_a_container();
    let (was, text) = (boxed(doc(&s), c), text_of(doc(&s), c));
    assert!(text.contains("Stage"));
    let fill = |s: &Session, id: u32| num(doc(s), id, "FillPattern");
    assert_eq!(fill(&s, c), 0.0);
    let r = run(&mut s, "container.style", json!({"ids": [c], "style": "tint-strip-centre"}));
    assert_eq!(r["styled"], 1);
    assert_eq!(fill(&s, c), 1.0, "a tint fills the box");
    assert_box("the size stays", boxed(doc(&s), c), was);
    assert_eq!(text_of(doc(&s), c), text);
    assert_eq!(members(&mut s, c), [a, b]);
    assert_eq!(info(&mut s, &[c])[0]["style"], "tint-strip-centre");
    let e = s.execute("container.style", &json!({"ids": [c], "style": "no-such"})).unwrap_err();
    assert!(matches!(e, EngineError::BadParams { .. }), "{e}");
}

#[test]
fn rotate_heading_goes_clockwise_round_the_sides_and_the_container_makes_room_for_the_strip() {
    // Heading Style; "Rotate Heading moves the heading around the sides one step at a time".
    let (mut s, a, _b, c) = two_in_a_container();
    let side = |s: &mut Session| info(s, &[c])[0]["heading"].as_str().unwrap().to_owned();
    assert_eq!(side(&mut s), "top");
    let was = boxed(doc(&s), c);
    run(&mut s, "container.heading", json!({"ids": [c], "rotate": true}));
    assert_eq!(side(&mut s), "right");
    assert!(boxed(doc(&s), c).2 > was.2 + 0.29, "the strip on the right needs room: {:?}", boxed(doc(&s), c));
    for want in ["bottom", "left", "top"] {
        run(&mut s, "container.heading", json!({"ids": [c], "rotate": true}));
        assert_eq!(side(&mut s), want);
    }
    // A side can be named too; on the left the heading's block is turned and runs the height of the box.
    run(&mut s, "container.heading", json!({"ids": [c], "side": "left"}));
    let d = doc(&s);
    assert!(near(num(d, c, "TxtAngle"), std::f64::consts::FRAC_PI_2), "{}", num(d, c, "TxtAngle"));
    assert!(near(num(d, c, "TxtWidth"), num(d, c, "Height")));
    assert!(members(&mut s, c).contains(&a));
    let e = s.execute("container.heading", &json!({"ids": [c]})).unwrap_err();
    assert!(matches!(e, EngineError::BadParams { .. }), "{e}");
    let e = s.execute("container.heading", &json!({"ids": [c], "side": "middle"})).unwrap_err();
    assert!(bad_params_msg(&e).contains("top, right, bottom or left"), "{e}");
}

/// A rectangle typed a container, as a file holds one: no setting of ours and no heading.
fn a_files_container(id: u32, at: (f64, f64, f64, f64)) -> diacraft_doc::Shape {
    let mut shape = rect(id, at).build();
    Arc::make_mut(&mut shape.sheet)
        .set_cell(&CellAddr::in_row("User", None, RowKey::name("msvStructureType"), "Value"), Cell::constant(Value::from_text("Container")))
        .unwrap();
    shape
}

#[test]
fn a_container_a_file_holds_takes_a_style_but_has_no_heading_of_ours_to_move() {
    let mut s = session_with(vec![a_files_container(1, (1.0, 1.0, 5.0, 4.0))]);
    let e = s.execute("container.heading", &json!({"ids": [1], "rotate": true})).unwrap_err();
    assert!(e.to_string().contains("not made from the container gallery"), "{e}");
    run(&mut s, "container.style", json!({"ids": [1], "style": "frame"}));
    run(&mut s, "container.heading", json!({"ids": [1], "rotate": true}));
    assert_eq!(info(&mut s, &[1])[0]["heading"], "right");
}

#[test]
fn select_contents_answers_the_members_and_changes_nothing() {
    let (mut s, a, b, c) = two_in_a_container();
    let before = snapshot(&s);
    assert_eq!(run(&mut s, "container.select_contents", json!({"ids": [c]}))["ids"], json!([a, b]));
    assert_eq!(snapshot(&s), before, "a query: no undo step");
}

#[test]
fn add_to_underlying_container_adds_a_shape_that_lies_over_a_container_and_is_none_of_its_members() {
    // Right-click, Container, Add to Underlying Container (a container added with nothing selected holds nothing, even over shapes).
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 4.25, 5.5);
    let c = add_container(&mut s, json!({"x": 4.25, "y": 5.5, "width": 4.0, "height": 3.0}));
    assert_eq!(members(&mut s, c), Vec::<u32>::new());
    let r = run(&mut s, "container.add_member", json!({"ids": [a]}));
    assert_eq!(r["joined"], json!([{"shape": a, "container": c}]));
    assert_eq!(members(&mut s, c), [a]);
    // Already a member: nothing is done and it is no undo step.
    let steps = snapshot(&s).undo_steps;
    assert_eq!(run(&mut s, "container.add_member", json!({"ids": [a]}))["joined"], json!([]));
    assert_eq!(snapshot(&s).undo_steps, steps);
    // Nothing lies under a shape far away: refused in words.
    let far = drop_shape(&mut s, "rectangle", 1.0, 1.0);
    let e = s.execute("container.add_member", &json!({"ids": [far]})).unwrap_err();
    assert!(e.to_string().contains("no container lies under shape"), "{e}");
    // A named container takes the shape wherever it is, and an Expand as Needed container grows to hold it.
    let before = boxed(doc(&s), c);
    let r = run(&mut s, "container.add_member", json!({"ids": [far], "container": c}));
    assert_eq!(r["resized"], json!([c]));
    assert!(boxed(doc(&s), c).0 < before.0, "it grew towards the shape");
    assert!(members(&mut s, c).contains(&far));
}

#[test]
fn a_shape_attached_to_an_edge_sits_on_it_and_follows_the_container_when_it_is_resized_or_moved() {
    // Codes 7 to 10.
    let mut s = blank_session();
    let c = add_container(&mut s, json!({"x": 4.25, "y": 5.5, "width": 4.0, "height": 3.0}));
    let a = drop_shape(&mut s, "rectangle", 4.25, 5.5);
    assert_eq!(members(&mut s, c), [a]);
    run(&mut s, "container.attach_edge", json!({"id": a, "container": c, "edge": "left"}));
    let d = doc(&s);
    assert!(near(num(d, a, "PinX"), 2.25), "its middle is on the container's left edge: {}", num(d, a, "PinX"));
    assert_eq!(ties(d, a), format!("SUM(DEPENDSON(7,Sheet.{c}!SheetRef()))"));
    assert_eq!(ties(d, c), format!("SUM(DEPENDSON(1,Sheet.{a}!SheetRef()))"));
    // Resized by hand: the shape stays on the edge it is on.
    run(&mut s, "shape.resize", json!({"id": c, "x": 1.25, "y": 4.0, "width": 6.0, "height": 3.0}));
    assert!(near(num(doc(&s), a, "PinX"), 1.25), "{}", num(doc(&s), a, "PinX"));
    // Moved with its container.
    run(&mut s, "shape.move", json!({"ids": [c], "dx": 1.0, "dy": 0.0}));
    assert!(near(num(doc(&s), a, "PinX"), 2.25));
    // A container that fits its members does not grow to hold a shape that sits on its edge.
    let was = boxed(doc(&s), c);
    let r = run(&mut s, "container.fit", json!({"ids": [c]}));
    assert_eq!(r["resized"], json!([]), "{r}");
    assert_box("no member that asks for room", boxed(doc(&s), c), was);
    // Moved by hand it is a plain member again.
    run(&mut s, "shape.move", json!({"ids": [a], "dx": 1.0, "dy": 0.0}));
    assert_eq!(ties(doc(&s), a), format!("SUM(DEPENDSON(4,Sheet.{c}!SheetRef()))"));
    let e = s.execute("container.attach_edge", &json!({"id": a, "container": c, "edge": "middle"})).unwrap_err();
    assert!(bad_params_msg(&e).contains("left, right, top or bottom"), "{e}");
}

#[test]
fn a_copy_of_a_container_a_quarter_inch_off_is_not_inside_the_original() {
    // The default duplicate: a quarter inch right and down. The copy's middle is in the original, its whole box is not: it is not nested.
    let (mut s, a, b, c) = two_in_a_container();
    let r = run(&mut s, "shape.duplicate", json!({"ids": [c]}));
    assert_eq!(r["created"], 3);
    assert_eq!(members(&mut s, c), [a, b], "the original holds the same two shapes");
    assert_eq!(info(&mut s, &[c])[0]["inside"], json!([]));
}

#[test]
fn a_container_whose_ties_are_no_list_is_left_alone_by_a_move_and_refused_in_words_by_a_command() {
    // A `Relationships` formula that is not a SUM of DEPENDSON calls is never rewritten, and a move is not refused for it.
    let mut container = a_files_container(1, (0.0, 0.0, 10.0, 10.0));
    Arc::make_mut(&mut container.sheet).cells.insert(CellName::new("Relationships"), Cell::with_formula("Width*2", Some(Value::from_text("0"))));
    let loose = rect(2, (4.0, 4.0, 5.0, 5.0)).build();
    let mut s = session_with(vec![container, loose]);
    let r = run(&mut s, "shape.move", json!({"ids": [2], "dx": 0.5, "dy": 0.0}));
    assert_eq!(r["moved"], 1);
    assert_eq!(ties(doc(&s), 1), "Width*2", "the formula is as it was");
    assert_eq!(ties(doc(&s), 2), "", "and the shape did not join a container whose ties are not understood");
    let e = s.execute("container.add_member", &json!({"ids": [2], "container": 1})).unwrap_err();
    assert!(e.to_string().contains("not a list of ties"), "{e}");
}

// ---- one walk with the callouts (the merge round) ----------------------------------------------------------------------------------------

#[test]
fn moving_a_container_moves_its_members_and_a_callout_tied_to_a_member_goes_too_each_once() {
    let (mut s, a, b, c) = two_in_a_container();
    let made = run(&mut s, "callout.insert", json!({"ids": [a]}));
    let callout = made["id"].as_u64().unwrap() as u32;
    let pins = |s: &Session| [a, b, c, callout].map(|id| (num(doc(s), id, "PinX"), num(doc(s), id, "PinY")));
    let before = pins(&s);
    assert_eq!(members(&mut s, c), [a, b], "the callout is no member");
    let moved = run(&mut s, "shape.move", json!({"ids": [c], "dx": 1.5, "dy": -1.0}));
    assert_eq!(moved["moved"], 1, "the shape named: what goes with it is not counted");
    for (was, now) in before.iter().zip(pins(&s)) {
        assert!(near(now.0, was.0 + 1.5) && near(now.1, was.1 - 1.0), "each went the distance once: {was:?} to {now:?}");
    }
    // The container and the member named together go once, the callout of the member with them.
    let before = pins(&s);
    run(&mut s, "shape.move", json!({"ids": [c, a], "dx": 0.5, "dy": 0.0}));
    for (was, now) in before.iter().zip(pins(&s)) {
        assert!(near(now.0, was.0 + 0.5) && near(now.1, was.1), "{was:?} to {now:?}");
    }
    assert_eq!(members(&mut s, c), [a, b], "still its members");
}

#[test]
fn a_container_on_a_locked_layer_takes_no_member_by_a_command_that_names_it() {
    // A command that changes the drawing refuses a shape it names on a locked layer. A container named as `container` is changed too: its members and its box.
    let (mut s, a, _b, c) = two_in_a_container();
    let outside = drop_shape(&mut s, "rectangle", 12.0, 8.0);
    run(&mut s, "layer.add", json!({"name": "Frozen"}));
    run(&mut s, "layer.assign", json!({"ids": [c], "layers": ["Frozen"]}));
    run(&mut s, "layer.set", json!({"layer": "Frozen", "lock": true}));
    let before = snapshot(&s);
    for (id, params) in [
        ("container.add_member", json!({"ids": [outside], "container": c})),
        ("container.attach_edge", json!({"id": a, "container": c, "edge": "left"})),
    ] {
        let e = s.execute(id, &params).unwrap_err();
        let words = e.to_string();
        assert!(words.contains(&format!("shape {c} ")) && words.contains("\"Frozen\"") && words.contains("locked"), "{id}: {words}");
        assert_eq!(snapshot(&s), before, "{id}: a refusal changes nothing");
    }
    // A query still answers, and a shape that names no container on the layer is not refused.
    assert_eq!(members(&mut s, c), [a, _b]);
}

// ---------- a member whose move a GUARD refuses ----------

/// The vector, built here: a container round a rectangle and a free line, the line's `BeginX` under `GUARD`. → the session, the rectangle, the line and the container.
fn guarded_member() -> (Session, u32, u32, u32) {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    let line = crate::edit::id_of(&run(&mut s, "connector.add", json!({"from": {"x": 4.5, "y": 3.5}, "to": {"x": 6.0, "y": 4.5}})));
    let c = add_container(&mut s, json!({"ids": [a, line]}));
    assert_eq!(members(&mut s, c), [a, line], "both are members");
    s.edit("guard", |d| {
        let at = d.find_shape(Owner::Page(1), line).unwrap();
        d.write_cell(
            &diacraft_doc::SheetRef::Shape(at),
            &CellAddr::direct("BeginX"),
            diacraft_doc::CellEdit::Cell(Cell::with_formula("GUARD(4.5)", Some(Value::from_text("4.5")))),
            diacraft_doc::WriteOrigin::Automation,
        )
        .map(|_| ())
        .map_err(|e| EngineError::Failed { cmd: "test".into(), msg: e.to_string() })
    })
    .unwrap();
    (s, a, line, c)
}

fn ends(d: &Document, id: u32) -> [f64; 4] {
    ["BeginX", "BeginY", "EndX", "EndY"].map(|cell| num(d, id, cell))
}

/// The gate found two sample drawings whose container move was refused whole because a member's end is guarded. The container now moves with every member that
/// can move; the member a GUARD holds is left where it was, still a member, and the move's answer names it and the cell. One undo step takes all of it back.
#[test]
fn a_container_moves_with_its_free_members_and_leaves_the_one_a_guard_holds_with_a_note() {
    let (mut s, a, line, c) = guarded_member();
    let (box_a, box_c, line_ends) = (boxed(doc(&s), a), boxed(doc(&s), c), ends(doc(&s), line));
    let (before, steps) = (doc(&s).clone(), snapshot(&s).undo_steps);
    let out = run(&mut s, "shape.move", json!({"ids": [c], "dx": 1.0, "dy": 0.5}));
    assert_eq!(out["moved"], 1, "{out}");
    assert_eq!(out["left"], json!([line]), "{out}");
    let notes: Vec<&str> = out["notes"].as_array().unwrap().iter().map(|n| n.as_str().unwrap()).collect();
    assert!(
        notes
            .iter()
            .any(|n| n.contains(&format!("shape {line} ")) && n.contains("`BeginX`") && n.contains("GUARD") && n.contains("left where it was")),
        "{notes:?}"
    );
    let moved = |b: (f64, f64, f64, f64), was: (f64, f64, f64, f64)| near(b.0, was.0 + 1.0) && near(b.1, was.1 + 0.5) && near(b.2, was.2 + 1.0);
    assert!(moved(boxed(doc(&s), c), box_c), "the container moved");
    assert!(moved(boxed(doc(&s), a), box_a), "and its free member with it");
    assert_eq!(ends(doc(&s), line), line_ends, "the guarded member was left where it was, both its ends");
    assert!(members(&mut s, c).contains(&line), "and is still a member");
    assert_eq!(snapshot(&s).undo_steps, steps + 1, "one undo step");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(doc(&s), &before, "which takes all of it back");
}

/// A shape named whose own end is guarded still refuses the move in words, as before, and a member on a locked layer still refuses the container's: only a
/// follower a GUARD holds is left.
#[test]
fn a_guarded_shape_named_and_a_locked_member_still_refuse_the_move() {
    let (mut s, _a, line, c) = guarded_member();
    let before = snapshot(&s);
    let e = s.execute("shape.move", &json!({"ids": [line], "dx": 1.0, "dy": 0.0})).unwrap_err();
    assert!(e.to_string().contains("BeginX") && e.to_string().contains("GUARD"), "{e}");
    assert_eq!(snapshot(&s), before);
    run(&mut s, "layer.add", json!({"name": "Frozen"}));
    run(&mut s, "layer.assign", json!({"ids": [line], "layers": ["Frozen"]}));
    run(&mut s, "layer.set", json!({"layer": "Frozen", "lock": true}));
    let before = snapshot(&s);
    let e = s.execute("shape.move", &json!({"ids": [c], "dx": 1.0, "dy": 0.0})).unwrap_err();
    assert!(e.to_string().contains("Frozen") && e.to_string().contains(&line.to_string()), "{e}");
    assert_eq!(snapshot(&s), before);
}
