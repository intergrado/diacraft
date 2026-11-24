//! Swimlanes: the Cross-Functional Flowchart's structure, a frame that holds a **list of lanes** (and a list of phases), through `Session::execute` as the control
//! channel, the CLI and MCP call it. Numbers are paper inches on a page of the default scale, where they are the cells' own. Each test names the row of
//! (Diagram kinds), (B1) or (LISTORDER, LISTMEMBERCOUNT) it holds to.

use std::collections::BTreeMap;

use diacraft_doc::{CellAddr, Document, Owner, Resolver, RowKey, SheetRef};
use diacraft_engine::Session;
use serde_json::{Value as Json, json};

use crate::container::{assert_box, boxed, info, members, ties};
use crate::edit::{doc, drop_shape, id_of, num};
use crate::{bad_params_msg, blank_session, check_session, run, snapshot};

/// The role a swimlane shape holds in its own `User` row (`DiaCraftSwimlane`).
pub(crate) fn role(d: &Document, id: u32) -> String {
    user_text(d, id, "DiaCraftSwimlane")
}

pub(crate) fn user_text(d: &Document, id: u32, row: &str) -> String {
    let at = d.find_shape(Owner::Page(1), id).unwrap_or_else(|| panic!("no shape {id}"));
    let found = Resolver::new(d).resolve(&SheetRef::Shape(at), &CellAddr::in_row("User", None, RowKey::name(row), "Value"));
    found.and_then(|f| f.value.cell.and_then(|c| c.value.as_ref().map(|v| v.text().to_owned()))).unwrap_or_default()
}

/// The ties of a shape by code: `{code: [shape, ...]}`, in the order the formula names them.
pub(crate) fn codes(d: &Document, id: u32) -> BTreeMap<u32, Vec<u32>> {
    let text = ties(d, id);
    let mut out: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for call in text.split("DEPENDSON(").skip(1) {
        let code: u32 = call.split(',').next().unwrap().trim_end_matches(')').parse().unwrap();
        let entry = out.entry(code).or_default();
        let mut rest = call;
        while let Some(at) = rest.find("Sheet.") {
            rest = &rest[at + 6..];
            let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
            entry.push(digits.parse().unwrap());
        }
    }
    out
}

/// A cross-functional structure: two lanes and two phases, the top left corner at (1, 8), 6 inches long, the lanes 1.5 inches thick.
/// → (session, the answer of `swimlane.create`).
pub(crate) fn made(orientation: &str) -> (Session, Json) {
    let mut s = blank_session();
    let r = run(
        &mut s,
        "swimlane.create",
        json!({"x": 1.0, "y": 8.0, "length": 6.0, "thickness": 1.5, "lanes": 2, "phases": 2, "orientation": orientation}),
    );
    (s, r)
}

pub(crate) fn ids(v: &Json) -> Vec<u32> {
    v.as_array().unwrap().iter().map(|x| u32::try_from(x.as_u64().unwrap()).unwrap()).collect()
}

/// What `swimlane.info` says of the first structure of the page.
pub(crate) fn structure(s: &mut Session) -> Json {
    run(s, "swimlane.info", json!({}))["structures"][0].clone()
}

fn lane_ids(s: &mut Session) -> Vec<u32> {
    structure(s)["lanes"].as_array().unwrap().iter().map(|l| u32::try_from(l["id"].as_u64().unwrap()).unwrap()).collect()
}

fn phase_ids(s: &mut Session) -> Vec<u32> {
    structure(s)["phases"].as_array().unwrap().iter().map(|l| u32::try_from(l["id"].as_u64().unwrap()).unwrap()).collect()
}

// ---- the structure -------------------------------------------------------------------------------------------------------------------------------------------

#[test]
fn a_new_structure_is_a_frame_that_holds_a_lane_list_and_a_phase_list_and_the_lists_hold_lanes_and_phases_with_the_ties_on_both_sides() {
    // "the lanes to be members of one lane list, and the lane list and a phase list to be members of one outer container"; both sides are stored.
    let (s, made) = made("horizontal");
    let (frame, lanes, phases) = (id_of(&json!({"id": made["frame"]})), ids(&made["lanes"]), ids(&made["phases"]));
    let (lane_list, phase_list) =
        (u32::try_from(made["lane_list"].as_u64().unwrap()).unwrap(), u32::try_from(made["phase_list"].as_u64().unwrap()).unwrap());
    assert_eq!((lanes.len(), phases.len()), (2, 2));
    let d = doc(&s);
    assert_eq!(role(d, frame), "frame");
    assert_eq!(role(d, lane_list), "lanes");
    assert_eq!(role(d, phase_list), "phases");
    assert!(lanes.iter().all(|l| role(d, *l) == "lane") && phases.iter().all(|p| role(d, *p) == "phase"));
    // The kinds the files say: the frame and the lanes and the phases are containers, the two lists are lists.
    assert_eq!(user_text(d, frame, "msvStructureType"), "Container");
    assert_eq!(user_text(d, lane_list, "msvStructureType"), "List");
    assert_eq!(user_text(d, phase_list, "msvStructureType"), "List");
    assert!(lanes.iter().chain(&phases).all(|l| user_text(d, *l, "msvStructureType") == "Container"));
    // The frame holds the two lists under 1, and each list names it under 4.
    assert_eq!(codes(d, frame).get(&1).cloned().unwrap(), [lane_list, phase_list]);
    assert_eq!(codes(d, lane_list).get(&4).cloned().unwrap(), [frame]);
    assert_eq!(codes(d, phase_list).get(&4).cloned().unwrap(), [frame]);
    // A list holds its members under 2 in their order, and each member names the list under 5.
    assert_eq!(codes(d, lane_list).get(&2).cloned().unwrap(), lanes);
    assert_eq!(codes(d, phase_list).get(&2).cloned().unwrap(), phases);
    for l in &lanes {
        assert_eq!(codes(d, *l).get(&5).cloned().unwrap(), [lane_list], "lane {l} names its list");
    }
    for p in &phases {
        assert_eq!(codes(d, *p).get(&5).cloned().unwrap(), [phase_list]);
    }
    check_session(&s).unwrap();
}

#[test]
fn horizontal_lanes_lie_one_under_the_next_with_the_same_length_and_the_lists_and_the_frame_are_the_box_of_what_they_hold() {
    // Diagram kinds: Cross-Functional Flowchart template ("the drawing opens with swimlanes already placed"), Swimlane orientation.
    let (mut s, r) = made("horizontal");
    let lanes = ids(&r["lanes"]);
    let phases = ids(&r["phases"]);
    let d = doc(&s);
    assert_box("lane 1", boxed(d, lanes[0]), (1.0, 6.5, 7.0, 8.0));
    assert_box("lane 2", boxed(d, lanes[1]), (1.0, 5.0, 7.0, 6.5));
    // The phase band is above the lanes (0.35 inch) and each phase is the band and the lanes' height, the length shared between them.
    assert_box("phase 1", boxed(d, phases[0]), (1.0, 5.0, 4.0, 8.35));
    assert_box("phase 2", boxed(d, phases[1]), (4.0, 5.0, 7.0, 8.35));
    assert_box("the lane list", boxed(d, u32::try_from(r["lane_list"].as_u64().unwrap()).unwrap()), (1.0, 5.0, 7.0, 8.0));
    assert_box("the phase list", boxed(d, u32::try_from(r["phase_list"].as_u64().unwrap()).unwrap()), (1.0, 5.0, 7.0, 8.35));
    assert_box("the frame", boxed(d, u32::try_from(r["frame"].as_u64().unwrap()).unwrap()), (1.0, 5.0, 7.0, 8.35));
    let st = structure(&mut s);
    assert_eq!(st["orientation"], "horizontal");
    assert_eq!(st["lanes"][0]["heading"], "left", "a horizontal lane's label runs up its left side");
    assert_eq!(st["lanes"][0]["text"], "Function", "the placeholder label");
    assert_eq!(st["phases"][0]["text"], "Phase");
    assert!((st["length"].as_f64().unwrap() - 6.0).abs() < 1e-9);
}

#[test]
fn vertical_lanes_lie_side_by_side_with_the_phase_band_to_their_left() {
    // Swimlane orientation: "a prompt on creation may ask for horizontal or vertical lanes".
    let (mut s, r) = made("vertical");
    let lanes = ids(&r["lanes"]);
    let phases = ids(&r["phases"]);
    let d = doc(&s);
    assert_box("lane 1", boxed(d, lanes[0]), (1.0, 2.0, 2.5, 8.0));
    assert_box("lane 2", boxed(d, lanes[1]), (2.5, 2.0, 4.0, 8.0));
    assert_box("phase 1", boxed(d, phases[0]), (0.65, 5.0, 4.0, 8.0));
    assert_box("phase 2", boxed(d, phases[1]), (0.65, 2.0, 4.0, 5.0));
    assert_box("the frame", boxed(d, u32::try_from(r["frame"].as_u64().unwrap()).unwrap()), (0.65, 2.0, 4.0, 8.0));
    let st = structure(&mut s);
    assert_eq!(st["orientation"], "vertical");
    assert_eq!(st["lanes"][0]["heading"], "top", "a vertical lane's label runs along its top");
    assert_eq!(st["phases"][0]["heading"], "left");
}

#[test]
fn a_structure_with_no_phases_is_a_frame_and_a_lane_list_and_asks_for_no_phase_list() {
    let mut s = blank_session();
    let r = run(&mut s, "swimlane.create", json!({"x": 1.0, "y": 8.0, "length": 6.0, "thickness": 1.0, "lanes": 3}));
    assert!(r["phase_list"].is_null() && ids(&r["phases"]).is_empty());
    let (frame, lane_list) = (u32::try_from(r["frame"].as_u64().unwrap()).unwrap(), u32::try_from(r["lane_list"].as_u64().unwrap()).unwrap());
    assert_box("the frame", boxed(doc(&s), frame), (1.0, 5.0, 7.0, 8.0));
    assert_eq!(codes(doc(&s), frame).get(&1).cloned().unwrap(), [lane_list]);
    assert_eq!(ids(&r["lanes"]).len(), 3);
}

// ---- insert a swimlane ---------------------------------------------------------------------------------------------------------------------------------------

#[test]
fn a_swimlane_is_inserted_before_or_after_a_lane_and_the_lanes_below_it_and_what_they_hold_move_down() {
    // Diagram kinds: Insert a swimlane ("a new lane can be added before or after an existing one"); the list reflows.
    let (mut s, r) = made("horizontal");
    let lanes = ids(&r["lanes"]);
    let held = drop_shape(&mut s, "rectangle", 4.0, 5.75);
    assert_eq!(members(&mut s, lanes[1]), [held], "dropped in lane 2, the shape is its member");
    let added = run(&mut s, "swimlane.insert", json!({"lane": lanes[0], "where": "after"}));
    let new = id_of(&added);
    assert_eq!(added["index"], 1);
    let d = doc(&s);
    assert_box("lane 1 stays", boxed(d, lanes[0]), (1.0, 6.5, 7.0, 8.0));
    assert_box("the new lane takes the place after it", boxed(d, new), (1.0, 5.0, 7.0, 6.5));
    assert_box("lane 2 is one lane lower", boxed(d, lanes[1]), (1.0, 3.5, 7.0, 5.0));
    assert!((num(d, held, "PinY") - 4.25).abs() < 1e-9, "the shape in lane 2 went down with it: {}", num(d, held, "PinY"));
    assert_eq!(members(&mut s, lanes[1]), [held], "and is still its member");
    // The list says the order; the frame and the phases grew by the new lane.
    assert_eq!(
        structure(&mut s)["lanes"].as_array().unwrap().iter().map(|l| l["id"].as_u64().unwrap()).collect::<Vec<_>>(),
        [lanes[0] as u64, new as u64, lanes[1] as u64]
    );
    assert_eq!(codes(doc(&s), u32::try_from(r["lane_list"].as_u64().unwrap()).unwrap()).get(&2).cloned().unwrap(), [lanes[0], new, lanes[1]]);
    assert_box("the frame", boxed(doc(&s), u32::try_from(r["frame"].as_u64().unwrap()).unwrap()), (1.0, 3.5, 7.0, 8.35));
    assert_box("the phase is as tall as the lanes", boxed(doc(&s), ids(&r["phases"])[0]), (1.0, 3.5, 4.0, 8.35));
    assert_eq!(role(doc(&s), new), "lane");
    assert_eq!(structure(&mut s)["lanes"][1]["text"], "Function", "a new lane has the placeholder label");
    // Before the first lane.
    let first = id_of(&run(&mut s, "swimlane.insert", json!({"lane": lanes[0], "where": "before"})));
    assert_box("the new first lane", boxed(doc(&s), first), (1.0, 6.5, 7.0, 8.0));
    assert_box("the old first lane moved down", boxed(doc(&s), lanes[0]), (1.0, 5.0, 7.0, 6.5));
    check_session(&s).unwrap();
}

#[test]
fn a_swimlane_inserted_with_no_lane_named_goes_at_the_end_and_a_page_with_no_lanes_refuses_in_words() {
    // Diagram kinds: "Without a selected lane the button adds one at the end."
    let (mut s, r) = made("horizontal");
    let lanes = ids(&r["lanes"]);
    let new = id_of(&run(&mut s, "swimlane.insert", json!({})));
    assert_eq!(lane_ids(&mut s), [lanes[0], lanes[1], new]);
    assert_box("at the end", boxed(doc(&s), new), (1.0, 3.5, 7.0, 5.0));
    let mut plain = blank_session();
    let before = snapshot(&plain);
    let e = plain.execute("swimlane.insert", &json!({})).unwrap_err();
    assert!(bad_params_msg(&e).contains("no swimlanes") && bad_params_msg(&e).contains("swimlane.create"), "{e}");
    assert_eq!(snapshot(&plain), before);
}

#[test]
fn inserting_a_swimlane_into_vertical_lanes_makes_a_column() {
    let (mut s, r) = made("vertical");
    let lanes = ids(&r["lanes"]);
    let new = id_of(&run(&mut s, "swimlane.insert", json!({"lane": lanes[0], "where": "before"})));
    assert_box("the new first column", boxed(doc(&s), new), (1.0, 2.0, 2.5, 8.0));
    assert_box("the old first column", boxed(doc(&s), lanes[0]), (2.5, 2.0, 4.0, 8.0));
    assert_box("the old second column", boxed(doc(&s), lanes[1]), (4.0, 2.0, 5.5, 8.0));
    assert_box("the phase is as wide as the lanes", boxed(doc(&s), ids(&r["phases"])[0]), (0.65, 5.0, 5.5, 8.0));
}

#[test]
fn a_swimlane_insert_is_one_undo_step_and_undo_and_redo_give_the_lanes_back() {
    let (mut s, r) = made("horizontal");
    let lanes = ids(&r["lanes"]);
    let before = snapshot(&s);
    run(&mut s, "swimlane.insert", json!({"lane": lanes[0]}));
    assert_eq!(snapshot(&s).undo_steps, before.undo_steps + 1);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(doc(&s), before.document.as_ref().unwrap(), "undo puts the document back");
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(lane_ids(&mut s).len(), 3);
}

// ---- a swimlane holds its shapes -----------------------------------------------------------------------------------------------------------------------------

#[test]
fn a_shape_dropped_in_a_lane_is_its_member_and_moves_with_the_lane_and_deleting_the_lane_deletes_it() {
    // Diagram kinds: Swimlane holds its shapes ("a shape dropped into a lane becomes a member ... and moves with it ... Deleting a lane deletes its members"); V7.
    let (mut s, r) = made("horizontal");
    let (frame, lanes) = (u32::try_from(r["frame"].as_u64().unwrap()).unwrap(), ids(&r["lanes"]));
    let held = drop_shape(&mut s, "rectangle", 4.0, 7.25);
    let other = drop_shape(&mut s, "rectangle", 4.0, 5.75);
    assert_eq!(members(&mut s, lanes[0]), [held]);
    assert_eq!(members(&mut s, lanes[1]), [other]);
    assert_eq!(codes(doc(&s), held).get(&4).cloned().unwrap(), [lanes[0]]);
    // The frame moves, and the lanes, the lists, the phases and what the lanes hold go with it.
    run(&mut s, "shape.move", json!({"ids": [frame], "dx": 1.0, "dy": -1.0}));
    assert_box("the frame moved", boxed(doc(&s), frame), (2.0, 4.0, 8.0, 7.35));
    assert_box("lane 1 moved", boxed(doc(&s), lanes[0]), (2.0, 5.5, 8.0, 7.0));
    assert!((num(doc(&s), held, "PinX") - 5.0).abs() < 1e-9 && (num(doc(&s), held, "PinY") - 6.25).abs() < 1e-9, "the shape moved with its lane");
    assert!((num(doc(&s), other, "PinY") - 4.75).abs() < 1e-9);
    // A lane moved on its own takes its members and leaves the others (a lane moved sideways is put back in its place in the list).
    // Deleting a lane deletes what it holds, and the lane below moves up into the room.
    let gone = run(&mut s, "shape.delete", json!({"ids": [lanes[0]]}));
    assert_eq!(gone["deleted"], 2, "the lane and the shape it held");
    assert!(doc(&s).find_shape(Owner::Page(1), held).is_none());
    assert!(doc(&s).find_shape(Owner::Page(1), other).is_some());
    assert_eq!(lane_ids(&mut s), [lanes[1]]);
    assert_box("the lane that is left moved up into the room", boxed(doc(&s), lanes[1]), (2.0, 5.5, 8.0, 7.0));
    assert_box("the frame is the box of what is left", boxed(doc(&s), frame), (2.0, 5.5, 8.0, 7.35));
    check_session(&s).unwrap();
}

#[test]
fn a_lane_made_over_a_free_shape_does_not_capture_it_until_the_shape_is_moved() {
    // Diagram kinds: "A lane dropped over an existing shape does not capture it until the shape is moved".
    let mut s = blank_session();
    let free = drop_shape(&mut s, "rectangle", 4.0, 7.25);
    let r = run(&mut s, "swimlane.create", json!({"x": 1.0, "y": 8.0, "length": 6.0, "thickness": 1.5, "lanes": 2}));
    let (frame, lanes) = (u32::try_from(r["frame"].as_u64().unwrap()).unwrap(), ids(&r["lanes"]));
    assert_eq!(members(&mut s, lanes[0]), Vec::<u32>::new(), "the lane holds nothing the shape lying under it");
    run(&mut s, "shape.move", json!({"ids": [frame], "dx": 0.0, "dy": -1.0}));
    assert!((num(doc(&s), free, "PinY") - 7.25).abs() < 1e-9, "the free shape stayed where it was");
    // Moved a little, it is inside the lane and joins it.
    run(&mut s, "shape.move", json!({"ids": [free], "dx": 0.1, "dy": -1.0}));
    let lane = lanes.iter().copied().find(|l| members_of(&mut s, *l).contains(&free));
    assert!(lane.is_some(), "the shape moved inside a lane is its member now");
}

fn members_of(s: &mut Session, id: u32) -> Vec<u32> {
    members(s, id)
}

// ---- move and resize swimlanes -------------------------------------------------------------------------------------------------------------------------------

#[test]
fn a_lane_resized_changes_its_thickness_and_the_lanes_after_it_and_what_they_hold_move_to_make_the_room() {
    // Diagram kinds: Move and resize swimlanes ("a lane's width changes, and the list reflows").
    let (mut s, r) = made("horizontal");
    let (frame, lanes, phases) = (u32::try_from(r["frame"].as_u64().unwrap()).unwrap(), ids(&r["lanes"]), ids(&r["phases"]));
    let held = drop_shape(&mut s, "rectangle", 4.0, 5.75);
    run(&mut s, "swimlane.resize", json!({"id": lanes[0], "size": 2.5}));
    assert_box("lane 1 is thicker", boxed(doc(&s), lanes[0]), (1.0, 5.5, 7.0, 8.0));
    assert_box("lane 2 is below it", boxed(doc(&s), lanes[1]), (1.0, 4.0, 7.0, 5.5));
    assert!((num(doc(&s), held, "PinY") - 4.75).abs() < 1e-9, "what lane 2 holds went with it");
    assert_box("the frame", boxed(doc(&s), frame), (1.0, 4.0, 7.0, 8.35));
    assert_box("the phases are as tall", boxed(doc(&s), phases[0]), (1.0, 4.0, 4.0, 8.35));
    // The same by hand: the box of lane 1 is dragged down by an inch (the Size & Position window, or a drag of its edge).
    run(&mut s, "shape.resize", json!({"id": lanes[0], "x": 1.0, "y": 4.5, "width": 6.0, "height": 3.5}));
    assert_box("lane 1 by hand", boxed(doc(&s), lanes[0]), (1.0, 4.5, 7.0, 8.0));
    assert_box("lane 2 moved again", boxed(doc(&s), lanes[1]), (1.0, 3.0, 7.0, 4.5));
    // A lane cannot be made thinner than the least a lane is, and the refusal says so.
    let before = snapshot(&s);
    let e = s.execute("swimlane.resize", &json!({"id": lanes[0], "size": 0.01})).unwrap_err();
    assert!(bad_params_msg(&e).contains("at least"), "{e}");
    assert_eq!(snapshot(&s), before);
    check_session(&s).unwrap();
}

#[test]
fn the_length_of_the_lanes_is_one_for_all_of_them_and_the_last_phase_takes_the_difference() {
    // Diagram kinds: "dragging a separator or end line changes its width or length".
    let (mut s, r) = made("horizontal");
    let (frame, lanes, phases) = (u32::try_from(r["frame"].as_u64().unwrap()).unwrap(), ids(&r["lanes"]), ids(&r["phases"]));
    run(&mut s, "swimlane.resize", json!({"id": lanes[1], "length": 8.0}));
    assert_box("lane 1 is as long", boxed(doc(&s), lanes[0]), (1.0, 6.5, 9.0, 8.0));
    assert_box("lane 2", boxed(doc(&s), lanes[1]), (1.0, 5.0, 9.0, 6.5));
    assert_box("the first phase is as it was", boxed(doc(&s), phases[0]), (1.0, 5.0, 4.0, 8.35));
    assert_box("the last phase took the difference", boxed(doc(&s), phases[1]), (4.0, 5.0, 9.0, 8.35));
    assert_box("the frame", boxed(doc(&s), frame), (1.0, 5.0, 9.0, 8.35));
    // By hand: lane 2 dragged out at its end line.
    run(&mut s, "shape.resize", json!({"id": lanes[1], "x": 1.0, "y": 5.0, "width": 9.0, "height": 1.5}));
    assert_box("every lane followed", boxed(doc(&s), lanes[0]), (1.0, 6.5, 10.0, 8.0));
    assert_box("the last phase again", boxed(doc(&s), phases[1]), (4.0, 5.0, 10.0, 8.35));
}

#[test]
fn a_lane_dragged_past_the_middle_of_its_neighbour_changes_places_with_it_and_a_lane_dragged_sideways_is_put_back() {
    // Diagram kinds: "dragging a lane by its header reorders it at an indicator between lanes".
    let (mut s, r) = made("horizontal");
    let lanes = ids(&r["lanes"]);
    let held = drop_shape(&mut s, "rectangle", 4.0, 5.75);
    // Sideways: nothing changes in the end.
    run(&mut s, "shape.move", json!({"ids": [lanes[1]], "dx": 2.0, "dy": 0.0}));
    assert_box("lane 2 put back", boxed(doc(&s), lanes[1]), (1.0, 5.0, 7.0, 6.5));
    assert!((num(doc(&s), held, "PinX") - 4.0).abs() < 1e-9, "and what it holds with it");
    // Up past the middle of lane 1: the lanes change places.
    run(&mut s, "shape.move", json!({"ids": [lanes[1]], "dx": 0.0, "dy": 1.6}));
    assert_eq!(lane_ids(&mut s), [lanes[1], lanes[0]]);
    assert_box("lane 2 is first", boxed(doc(&s), lanes[1]), (1.0, 6.5, 7.0, 8.0));
    assert_box("lane 1 is second", boxed(doc(&s), lanes[0]), (1.0, 5.0, 7.0, 6.5));
    assert!((num(doc(&s), held, "PinY") - 7.25).abs() < 1e-9, "the shape in the lane went with it: {}", num(doc(&s), held, "PinY"));
    assert_eq!(members(&mut s, lanes[1]), [held]);
    // The command does the same.
    run(&mut s, "swimlane.move", json!({"lane": lanes[1], "to": 1}));
    assert_eq!(lane_ids(&mut s), [lanes[0], lanes[1]]);
    assert_box("lane 2 last again", boxed(doc(&s), lanes[1]), (1.0, 5.0, 7.0, 6.5));
    check_session(&s).unwrap();
}

#[test]
fn a_lane_that_is_moved_is_not_made_a_member_of_the_frame_that_holds_it() {
    let (mut s, r) = made("horizontal");
    let (frame, lanes) = (u32::try_from(r["frame"].as_u64().unwrap()).unwrap(), ids(&r["lanes"]));
    run(&mut s, "shape.move", json!({"ids": [lanes[0]], "dx": 0.0, "dy": -0.5}));
    assert_eq!(codes(doc(&s), lanes[0]).get(&4), None, "a lane names its list and no container");
    assert_eq!(codes(doc(&s), frame).get(&1).unwrap().len(), 2, "the frame holds its two lists and no lane");
}

// ---- orientation, the label and the phases -------------------------------------------------------------------------------------------------------------------

#[test]
fn the_orientation_turns_the_whole_structure_a_quarter_over_and_what_the_lanes_hold_stays_in_its_lane() {
    // Diagram kinds: Swimlane orientation ("the desktop page says the template's own tab can change it afterwards"); what happens to existing lanes is ours, the lane k stays the k-th.
    let (mut s, r) = made("horizontal");
    let (frame, lanes, phases) = (u32::try_from(r["frame"].as_u64().unwrap()).unwrap(), ids(&r["lanes"]), ids(&r["phases"]));
    let held = drop_shape(&mut s, "rectangle", 4.0, 5.75);
    assert_eq!(members(&mut s, lanes[1]), [held]);
    run(&mut s, "swimlane.orientation", json!({"id": frame, "orientation": "vertical"}));
    let d = doc(&s);
    assert_box("the frame", boxed(d, frame), (1.0, 2.35, 4.35, 8.35));
    assert_box("lane 1 is the first column", boxed(d, lanes[0]), (1.35, 2.35, 2.85, 8.35));
    assert_box("lane 2 is the second", boxed(d, lanes[1]), (2.85, 2.35, 4.35, 8.35));
    assert_box("phase 1 is the first row", boxed(d, phases[0]), (1.0, 5.35, 4.35, 8.35));
    assert_eq!(structure(&mut s)["orientation"], "vertical");
    assert_eq!(structure(&mut s)["lanes"][0]["heading"], "top", "a column's label runs along its top");
    assert_eq!(members(&mut s, lanes[1]), [held], "the shape is still in the second lane");
    let b = boxed(doc(&s), lanes[1]);
    let (px, py) = (num(doc(&s), held, "PinX"), num(doc(&s), held, "PinY"));
    assert!(px > b.0 && px < b.2 && py > b.1 && py < b.3, "and lies inside it: ({px}, {py}) in {b:?}");
    // And back.
    run(&mut s, "swimlane.orientation", json!({"id": frame, "orientation": "horizontal"}));
    assert_box("the frame is as it was", boxed(doc(&s), frame), (1.0, 5.0, 7.0, 8.35));
    assert_box("lane 2 is as it was", boxed(doc(&s), lanes[1]), (1.0, 5.0, 7.0, 6.5));
    assert!((num(doc(&s), held, "PinX") - 4.0).abs() < 1e-9 && (num(doc(&s), held, "PinY") - 5.75).abs() < 1e-9, "the shape is where it was");
    // The orientation it already has changes nothing and is no undo step.
    let before = snapshot(&s);
    run(&mut s, "swimlane.orientation", json!({"id": frame, "orientation": "horizontal"}));
    assert_eq!(snapshot(&s).undo_steps, before.undo_steps);
    check_session(&s).unwrap();
}

#[test]
fn rotate_lane_label_turns_the_label_of_the_lanes_named_and_the_text_stays() {
    // Diagram kinds: Swimlane label text and direction ("Rotate Lane Label on the tab turns its direction").
    let (mut s, r) = made("horizontal");
    let lanes = ids(&r["lanes"]);
    run(&mut s, "shape.set_text", json!({"id": lanes[0], "text": "Sales"}));
    run(&mut s, "swimlane.rotate_label", json!({"ids": [lanes[0]]}));
    assert_eq!(info(&mut s, &[lanes[0]])[0]["heading"], "top");
    assert_eq!(info(&mut s, &[lanes[1]])[0]["heading"], "left", "the other lane is as it was");
    assert_eq!(structure(&mut s)["lanes"][0]["text"], "Sales", "the text a person typed is kept");
    run(&mut s, "swimlane.rotate_label", json!({"ids": [lanes[0]]}));
    assert_eq!(info(&mut s, &[lanes[0]])[0]["heading"], "left");
    // Named by the frame, every lane of the structure turns.
    run(&mut s, "swimlane.rotate_label", json!({"ids": [u32::try_from(r["frame"].as_u64().unwrap()).unwrap()]}));
    assert!(lanes.iter().all(|l| info(&mut s, &[*l])[0]["heading"] == "top"));
}

#[test]
fn a_separator_splits_a_phase_in_two_and_a_phase_resized_takes_from_the_next_one_and_never_changes_the_length() {
    // Diagram kinds: Phase separators ("a separator dropped across the lanes marks a change of phase, with its own label").
    let (mut s, r) = made("horizontal");
    let (frame, phases) = (u32::try_from(r["frame"].as_u64().unwrap()).unwrap(), ids(&r["phases"]));
    let added = run(&mut s, "swimlane.insert_phase", json!({"phase": phases[0], "where": "after", "label": "Review"}));
    let new = id_of(&added);
    assert_eq!(phase_ids(&mut s), [phases[0], new, phases[1]]);
    assert_box("the first phase is half as wide", boxed(doc(&s), phases[0]), (1.0, 5.0, 2.5, 8.35));
    assert_box("the new phase takes the other half", boxed(doc(&s), new), (2.5, 5.0, 4.0, 8.35));
    assert_box("the last is as it was", boxed(doc(&s), phases[1]), (4.0, 5.0, 7.0, 8.35));
    assert_eq!(structure(&mut s)["phases"][1]["text"], "Review", "the new phase has its own label");
    assert_box("the frame is the same", boxed(doc(&s), frame), (1.0, 5.0, 7.0, 8.35));
    // A separator dropped across at a place splits there.
    let at = id_of(&run(&mut s, "swimlane.insert_phase", json!({"phase": phases[1], "where": "after", "at": 5.0})));
    assert_box("split at 5", boxed(doc(&s), phases[1]), (4.0, 5.0, 5.0, 8.35));
    assert_box("the rest", boxed(doc(&s), at), (5.0, 5.0, 7.0, 8.35));
    // A phase made wider takes the room from the next one; the length of the lanes is as it was.
    run(&mut s, "swimlane.resize", json!({"id": phases[0], "size": 2.0}));
    assert_box("wider", boxed(doc(&s), phases[0]), (1.0, 5.0, 3.0, 8.35));
    assert_box("the next is narrower", boxed(doc(&s), new), (3.0, 5.0, 4.0, 8.35));
    assert_box("the lanes are as long", boxed(doc(&s), ids(&r["lanes"])[0]), (1.0, 6.5, 7.0, 8.0));
    // By hand, the same.
    run(&mut s, "shape.resize", json!({"id": phases[0], "x": 1.0, "y": 5.0, "width": 2.5, "height": 3.35}));
    assert_box("by hand", boxed(doc(&s), new), (3.5, 5.0, 4.0, 8.35));
    // A phase cannot be made so wide that the next is thinner than the least.
    let before = snapshot(&s);
    let e = s.execute("swimlane.resize", &json!({"id": phases[0], "size": 2.95})).unwrap_err();
    assert!(bad_params_msg(&e).contains("at least"), "{e}");
    assert_eq!(snapshot(&s), before);
    check_session(&s).unwrap();
}

#[test]
fn a_separator_on_a_structure_with_no_phases_makes_the_phase_list_with_two_phases() {
    let mut s = blank_session();
    let r = run(&mut s, "swimlane.create", json!({"x": 1.0, "y": 8.0, "length": 6.0, "thickness": 1.5, "lanes": 2}));
    let frame = u32::try_from(r["frame"].as_u64().unwrap()).unwrap();
    let made = run(&mut s, "swimlane.insert_phase", json!({}));
    let phases = ids(&made["phases"]);
    assert_eq!(phases.len(), 2);
    assert_box("the first", boxed(doc(&s), phases[0]), (1.0, 5.0, 4.0, 8.35));
    assert_box("the second", boxed(doc(&s), phases[1]), (4.0, 5.0, 7.0, 8.35));
    assert_box("the frame grew by the band", boxed(doc(&s), frame), (1.0, 5.0, 7.0, 8.35));
    assert_eq!(codes(doc(&s), frame).get(&1).unwrap().len(), 2, "the frame holds the lane list and the new phase list");
}

#[test]
fn a_phase_takes_no_shape_by_a_drop_so_a_shape_over_it_goes_to_the_lane() {
    let (mut s, r) = made("horizontal");
    let lanes = ids(&r["lanes"]);
    let phases = ids(&r["phases"]);
    let held = drop_shape(&mut s, "rectangle", 2.0, 7.25);
    assert_eq!(members(&mut s, lanes[0]), [held]);
    assert_eq!(info(&mut s, &[phases[0]])[0]["members"], json!([]));
}

/// What the formula `formula` gives on shape `id`, put in a cell of a copy of the document.
fn evaluate(d: &Document, id: u32, formula: &str) -> f64 {
    use diacraft_calc::{Budget, CellAddress, EvalCtx, Value as Calc};
    let mut d = d.clone();
    let at = d.find_shape(Owner::Page(1), id).unwrap();
    let addr = CellAddr::in_row("User", None, RowKey::name("Probe"), "Value");
    let page = std::sync::Arc::make_mut(d.pages.get_mut(0).unwrap());
    let shape = std::sync::Arc::make_mut(page.shapes.get_mut(usize::try_from(at.path[0]).unwrap()).unwrap());
    std::sync::Arc::make_mut(&mut shape.sheet).set_cell(&addr, diacraft_doc::Cell::with_formula(formula, None)).unwrap();
    let mut ctx = EvalCtx::new(diacraft_calc::EvalOptions::default(), Budget::default());
    match diacraft_calc::evaluate(&d, &CellAddress::new(SheetRef::Shape(at), addr), &mut ctx) {
        Calc::Number(q) => q.stored,
        other => panic!("{formula} gave {other:?}"),
    }
}

#[test]
fn the_lanes_are_the_members_a_list_counts_to_the_formula_engine() {
    // Reads code 2, the place among the list's code 2 targets, code 1 (the frame holds its two lists).
    let (mut s, r) = made("horizontal");
    let (frame, lanes, list) =
        (u32::try_from(r["frame"].as_u64().unwrap()).unwrap(), ids(&r["lanes"]), u32::try_from(r["lane_list"].as_u64().unwrap()).unwrap());
    let held = drop_shape(&mut s, "rectangle", 4.0, 5.75);
    let d = doc(&s);
    assert_eq!(evaluate(d, list, "LISTMEMBERCOUNT()"), 2.0);
    assert_eq!(evaluate(d, lanes[0], "LISTORDER()"), 1.0);
    assert_eq!(evaluate(d, lanes[1], "LISTORDER()"), 2.0);
    assert_eq!(evaluate(d, frame, "CONTAINERMEMBERCOUNT()"), 2.0);
    assert_eq!(evaluate(d, lanes[1], "CONTAINERMEMBERCOUNT()"), 1.0, "the lane holds the shape in it");
    assert_eq!(evaluate(d, held, "CONTAINERCOUNT()"), 1.0);
    // A lane inserted ahead of lane 1 makes it the second.
    run(&mut s, "swimlane.insert", json!({"lane": lanes[0], "where": "before"}));
    assert_eq!(evaluate(doc(&s), lanes[0], "LISTORDER()"), 2.0);
    assert_eq!(evaluate(doc(&s), list, "LISTMEMBERCOUNT()"), 3.0);
}

#[test]
fn a_lane_on_a_locked_layer_is_not_moved_resized_or_given_a_neighbour_by_a_command_that_names_it() {
    // A command that changes the drawing refuses a shape it names on a locked layer; the `lane` and `phase` of a `swimlane.*` command are changed too.
    let (mut s, r) = made("horizontal");
    let (lanes, phases) = (ids(&r["lanes"]), ids(&r["phases"]));
    run(&mut s, "layer.add", json!({"name": "Frozen"}));
    run(&mut s, "layer.assign", json!({"ids": [lanes[0], phases[0]], "layers": ["Frozen"]}));
    run(&mut s, "layer.set", json!({"layer": "Frozen", "lock": true}));
    let before = snapshot(&s);
    for (id, params, named) in [
        ("swimlane.insert", json!({"lane": lanes[0]}), lanes[0]),
        ("swimlane.insert_phase", json!({"phase": phases[0]}), phases[0]),
        ("swimlane.move", json!({"lane": lanes[0], "to": 1}), lanes[0]),
        ("swimlane.resize", json!({"id": lanes[0], "size": 2.0}), lanes[0]),
    ] {
        let e = s.execute(id, &params).unwrap_err();
        let words = e.to_string();
        assert!(words.contains(&format!("shape {named} ")) && words.contains("\"Frozen\"") && words.contains("locked"), "{id}: {words}");
        assert_eq!(snapshot(&s), before, "{id}: a refusal changes nothing");
    }
    // A layout that would change a shape on the layer is refused too: the lane after it grows, and the phase on the layer would have to grow with it.
    let e = s.execute("swimlane.resize", &json!({"id": lanes[1], "size": 2.0})).unwrap_err();
    assert!(e.to_string().contains(&format!("shape {} ", phases[0])) && e.to_string().contains("\"Frozen\""), "{e}");
    assert_eq!(snapshot(&s), before, "a refusal changes nothing");
}

#[test]
fn a_structure_copied_and_pasted_is_a_structure_of_its_own_and_one_deleted_takes_all_it_holds() {
    // A copy and a delete of the frame take everything below it, through the lists (`Page::parts_of`), and the copies' ties name only copies.
    let (mut s, r) = made("horizontal");
    let (frame, lanes) = (u32::try_from(r["frame"].as_u64().unwrap()).unwrap(), ids(&r["lanes"]));
    let held = drop_shape(&mut s, "rectangle", 4.0, 5.75);
    assert_eq!(members(&mut s, lanes[1]), [held]);
    run(&mut s, "edit.copy", json!({"ids": [frame]}));
    let pasted = run(&mut s, "edit.paste", json!({"x": 5.0, "y": 3.0}));
    let copy = ids(&pasted["ids"]);
    // The frame, the two lists, two lanes, two phases and the shape in a lane: eight shapes, all of the page's own list.
    assert_eq!(copy.len(), 8, "the structure was pasted whole: {pasted}");
    assert_eq!(pasted["created"], 8);
    let all = run(&mut s, "swimlane.info", json!({}));
    let structures = all["structures"].as_array().unwrap();
    assert_eq!(structures.len(), 2, "{all}");
    let (first, second) = (&structures[0], &structures[1]);
    let original: Vec<u64> = first["lanes"].as_array().unwrap().iter().map(|l| l["id"].as_u64().unwrap()).collect();
    let copied: Vec<u64> = second["lanes"].as_array().unwrap().iter().map(|l| l["id"].as_u64().unwrap()).collect();
    assert!(original.iter().all(|l| !copied.contains(l)), "the copy has lanes of its own: {original:?} {copied:?}");
    assert_eq!(second["phases"].as_array().unwrap().len(), 2);
    // The copy's lane holds a copy of the shape, and the original keeps its own.
    let copied_lane = u32::try_from(copied[1]).unwrap();
    let held_copy = members(&mut s, copied_lane);
    assert_eq!(held_copy.len(), 1);
    assert_ne!(held_copy[0], held);
    assert_eq!(members(&mut s, lanes[1]), [held]);
    // Every tie of the copy names a shape of the copy.
    let d = doc(&s);
    let copy_ids: std::collections::BTreeSet<u32> = copy.iter().copied().collect();
    for id in &copy_ids {
        for named in codes(d, *id).values().flatten() {
            assert!(copy_ids.contains(named), "shape {id} of the copy names {named}, which is not of the copy");
        }
    }
    // Deleting the copy's frame deletes the structure it holds (eight shapes), and the original is as it was.
    let copy_frame = u32::try_from(second["frame"].as_u64().unwrap()).unwrap();
    let gone = run(&mut s, "shape.delete", json!({"ids": [copy_frame]}));
    assert_eq!(gone["deleted"], 8);
    assert_eq!(run(&mut s, "swimlane.info", json!({}))["structures"].as_array().unwrap().len(), 1);
    assert_eq!(members(&mut s, lanes[1]), [held]);
    check_session(&s).unwrap();
}

#[test]
fn a_cut_and_pasted_lane_is_a_lane_of_no_list_and_the_lanes_that_were_below_it_close_the_gap() {
    let (mut s, r) = made("horizontal");
    let lanes = ids(&r["lanes"]);
    run(&mut s, "edit.cut", json!({"ids": [lanes[0]]}));
    assert_eq!(lane_ids(&mut s), [lanes[1]]);
    assert_box("the lane that is left closed the gap", boxed(doc(&s), lanes[1]), (1.0, 6.5, 7.0, 8.0));
    let pasted = run(&mut s, "edit.paste", json!({"x": 4.0, "y": 3.0}));
    let lane = ids(&pasted["ids"])[0];
    assert_eq!(codes(doc(&s), lane).get(&5), None, "the pasted lane is in no list: its list was not copied");
    assert_eq!(lane_ids(&mut s), [lanes[1]], "and the structure has the one lane");
    check_session(&s).unwrap();
}
