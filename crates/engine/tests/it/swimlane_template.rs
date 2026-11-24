//! The Cross-Functional Flowchart template and its stencil: a drawing started from the template has its lanes and phases in place and the stencil open, and a Swimlane or a
//! Separator dropped from the stencil is **inserted** in the lanes and the phases. Rows: (Cross-Functional Flowchart template, Insert a swimlane, Phase
//! separators), C2.

use diacraft_engine::Session;
use serde_json::{Value as Json, json};

use crate::container::{assert_box, boxed, members};
use crate::edit::{doc, drop_shape};
use crate::swimlane::structure;
use crate::{blank_session, check_session, run};

fn ids_of(list: &Json, key: &str) -> Vec<String> {
    list[key].as_array().unwrap().iter().map(|s| s["id"].as_str().unwrap().to_owned()).collect()
}

fn started() -> Session {
    let mut s = Session::new();
    run(&mut s, "file.new", json!({ "template": "cross_functional_flowchart" }));
    s
}

fn lane_ids(s: &mut Session) -> Vec<u32> {
    structure(s)["lanes"].as_array().unwrap().iter().map(|l| u32::try_from(l["id"].as_u64().unwrap()).unwrap()).collect()
}

fn phase_ids(s: &mut Session) -> Vec<u32> {
    structure(s)["phases"].as_array().unwrap().iter().map(|l| u32::try_from(l["id"].as_u64().unwrap()).unwrap()).collect()
}

#[test]
fn the_template_is_listed_with_its_page_and_its_stencils_and_a_drawing_started_from_it_has_three_lanes_and_three_phases() {
    // Diagram kinds: Cross-Functional Flowchart template ("the drawing opens with swimlanes already placed and the Basic Flowchart stencil beside the swimlane stencil").
    let mut s = Session::new();
    let r = run(&mut s, "template.list", json!({}));
    let t = r["templates"].as_array().unwrap().iter().find(|t| t["id"] == "cross_functional_flowchart").expect("the template is listed").clone();
    assert_eq!((t["name"].as_str(), t["category"].as_str()), (Some("Cross-Functional Flowchart"), Some("Flowchart")));
    assert_eq!(t["page"]["orientation"], "landscape");
    assert_eq!(ids_of(&t, "stencils"), ["cross-functional", "flowchart", "basic"]);
    assert!(t["description"].as_str().is_some_and(|d| d.contains("swimlane")));
    let mut s = started();
    assert_eq!(
        ids_of(&run(&mut s, "stencil.list", json!({})), "stencils"),
        ["cross-functional", "flowchart", "basic"],
        "the swimlane stencil is the one it shows"
    );
    let st = structure(&mut s);
    assert_eq!(st["orientation"], "horizontal");
    assert_eq!(st["lanes"].as_array().unwrap().len(), 3);
    assert_eq!(st["phases"].as_array().unwrap().len(), 3);
    let (frame, lanes) = (u32::try_from(st["frame"].as_u64().unwrap()).unwrap(), lane_ids(&mut s));
    // As wide as the page less an inch, from half an inch in, 2 inches thick under the phase band.
    assert_box("the frame", boxed(doc(&s), frame), (0.5, 1.65, 10.5, 8.0));
    assert_box("lane 1", boxed(doc(&s), lanes[0]), (0.5, 5.65, 10.5, 7.65));
    assert_eq!(st["lanes"][0]["text"], "Function");
    assert_eq!(st["phases"][0]["text"], "Phase");
    check_session(&s).unwrap();
}

#[test]
fn a_shape_dropped_in_a_lane_of_the_template_is_its_member_and_the_flowchart_stencil_is_beside_it() {
    let mut s = started();
    let lanes = lane_ids(&mut s);
    let held = drop_shape(&mut s, "process", 3.0, 4.0);
    assert_eq!(members(&mut s, lanes[1]), [held]);
}

#[test]
fn the_stencil_has_a_swimlane_and_a_separator() {
    let mut s = blank_session();
    let listed = run(&mut s, "stencil.list", json!({}));
    let st = listed["stencils"].as_array().unwrap().iter().find(|x| x["id"] == "cross-functional").expect("a built-in stencil").clone();
    assert_eq!(st["name"], "Cross-Functional Flowchart Shapes");
    assert_eq!(ids_of(&st, "masters"), ["xfunc.swimlane", "xfunc.separator"]);
    assert_eq!(st["masters"][0]["name"], "Swimlane");
    assert_eq!(st["masters"][1]["name"], "Separator");
}

#[test]
fn a_swimlane_dropped_from_the_stencil_is_inserted_before_or_after_the_lane_it_is_dropped_on() {
    // Diagram kinds: Insert a swimlane ("or by dragging from the stencil").
    let mut s = started();
    let lanes = lane_ids(&mut s);
    // In the lower half of lane 2 (y from 3.65 to 5.65, its middle 4.65): after it.
    let r = run(&mut s, "shape.drop", json!({"master": "Swimlane", "stencil": "cross-functional", "page": 1, "x": 5.0, "y": 4.0}));
    let new = u32::try_from(r["id"].as_u64().unwrap()).unwrap();
    assert_eq!(lane_ids(&mut s), [lanes[0], lanes[1], new, lanes[2]]);
    assert_eq!(r["name"], "Swimlane");
    // In the upper half of lane 1: before it.
    let r = run(&mut s, "shape.drop", json!({"master": "xfunc.swimlane", "page": 1, "x": 5.0, "y": 7.5}));
    let first = u32::try_from(r["id"].as_u64().unwrap()).unwrap();
    assert_eq!(lane_ids(&mut s)[0], first);
    assert_eq!(lane_ids(&mut s).len(), 5);
    // No master was added to the drawing: the part is the lanes' own.
    assert!(doc(&s).masters.iter().next().is_none(), "no master of the stencil is kept");
    check_session(&s).unwrap();
}

#[test]
fn a_separator_dropped_across_the_phases_splits_the_phase_at_the_place_it_is_dropped() {
    // Diagram kinds: Phase separators ("a separator dropped across the lanes marks a change of phase").
    let mut s = started();
    let phases = phase_ids(&mut s);
    // The phases are 10/3 inches wide from 0.5: the first ends at 3.8333...; dropped at x 2.0 it splits there.
    let r = run(&mut s, "shape.drop", json!({"master": "xfunc.separator", "page": 1, "x": 2.0, "y": 5.0}));
    let new = u32::try_from(r["id"].as_u64().unwrap()).unwrap();
    assert_eq!(phase_ids(&mut s), [phases[0], new, phases[1], phases[2]]);
    assert_box("the first part", boxed(doc(&s), phases[0]), (0.5, 1.65, 2.0, 8.0));
    assert!((boxed(doc(&s), new).0 - 2.0).abs() < 1e-9, "the new phase begins at the separator");
}

#[test]
fn on_a_page_with_no_lanes_the_swimlane_and_the_separator_are_dropped_as_the_shapes_they_are_drawn_as() {
    // A drop is an insert only where there is a structure to insert in.
    let mut s = blank_session();
    for (master, x) in [("xfunc.separator", 4.0), ("xfunc.swimlane", 6.0)] {
        let plain = run(&mut s, "shape.drop", json!({"master": master, "page": 1, "x": x, "y": 4.0}));
        assert!(plain["master"].is_number(), "a master of the drawing: {plain}");
    }
    assert!(run(&mut s, "swimlane.info", json!({}))["structures"].as_array().unwrap().is_empty());
    check_session(&s).unwrap();
}
