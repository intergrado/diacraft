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
    // The fourth lane does not fit under the page: the template's page has Auto Size on, so it grew by a sheet and the structure moved up with the origin.
    let lift = run(&mut s, "page.setup", json!({}))["height"].as_f64().unwrap() - 8.5;
    assert!(lift > 0.0);
    // In the upper half of lane 1: before it.
    let r = run(&mut s, "shape.drop", json!({"master": "xfunc.swimlane", "page": 1, "x": 5.0, "y": 7.5 + lift}));
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

// ---- lanes and the page (issue 4 of the demo video) ---------------------------------------------------------------------------------------

/// The page's size in inches: `page.setup`'s `width` and `height`.
fn page_size(s: &mut Session) -> (f64, f64) {
    let setup = run(s, "page.setup", json!({}));
    (setup["width"].as_f64().unwrap(), setup["height"].as_f64().unwrap())
}

/// The boxes of the lanes of the first structure as `swimlane.info` tells them: `[left, bottom, right, top]` in inches.
fn lane_boxes(s: &mut Session) -> Vec<[f64; 4]> {
    structure(s)["lanes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| {
            let r = l["rect"].as_array().unwrap();
            [r[0].as_f64().unwrap(), r[1].as_f64().unwrap(), r[2].as_f64().unwrap(), r[3].as_f64().unwrap()]
        })
        .collect()
}

/// Every lane of the first structure lies on the page.
fn assert_lanes_on_the_page(s: &mut Session, what: &str) {
    let (w, h) = page_size(s);
    let eps = 1e-6;
    for (k, b) in lane_boxes(s).iter().enumerate() {
        assert!(b[0] >= -eps && b[1] >= -eps && b[2] <= w + eps && b[3] <= h + eps, "{what}: lane {k} is {b:?} on a page of {w} by {h}");
    }
}

#[test]
fn the_template_page_has_auto_size_on_and_a_blank_page_has_it_off() {
    // Our rule: the template's page grows by whole sheets, since a lane is added past its edge; a new drawing's page keeps the off.
    let mut s = started();
    assert_eq!(run(&mut s, "page.setup", json!({}))["auto_size"], true);
    let mut blank = blank_session();
    assert_eq!(run(&mut blank, "page.setup", json!({}))["auto_size"], false);
}

#[test]
fn a_fourth_lane_inserted_in_the_template_is_on_the_page_because_the_page_grew_by_a_sheet() {
    // The demo's steps: `file.new {template: "cross_functional_flowchart"}`, then `swimlane.insert {}`: the lane was at [0.5, -0.35, 10.5, 1.65] on a page 8.5 inches high.
    let mut s = started();
    assert_eq!(page_size(&mut s), (11.0, 8.5));
    let r = run(&mut s, "swimlane.insert", json!({}));
    assert_eq!(r["index"], 3);
    assert_eq!(lane_boxes(&mut s).len(), 4);
    assert_lanes_on_the_page(&mut s, "a fourth lane");
    // Whole sheets of the printer paper (Letter, landscape: 11 by 8.5), as Auto Size grows any page.
    assert_eq!(page_size(&mut s), (11.0, 17.0));
    // The structure was carried with the page's origin: the lanes keep their order and thickness, and the first lane is still 0.5 inch below the top of the page.
    let boxes = lane_boxes(&mut s);
    assert!((boxes[0][3] - (17.0 - 0.5 - 0.35)).abs() < 1e-6, "the first lane's top: {boxes:?}");
    for lane in &boxes {
        assert!((lane[3] - lane[1] - 2.0).abs() < 1e-6, "every lane is 2 inches thick: {boxes:?}");
    }
    check_session(&s).unwrap();
}

#[test]
fn a_lane_inserted_before_the_first_pushes_the_others_down_and_the_page_grows_for_them_too() {
    // The new lane is on the page at the top; the lane that was the last is the one pushed past the bottom.
    let mut s = started();
    let lanes = lane_ids(&mut s);
    run(&mut s, "swimlane.insert", json!({"lane": lanes[0], "where": "before"}));
    assert_eq!(lane_boxes(&mut s).len(), 4);
    assert_lanes_on_the_page(&mut s, "a lane before the first");
    assert_eq!(page_size(&mut s), (11.0, 17.0));
    check_session(&s).unwrap();
}

#[test]
fn a_swimlane_dropped_from_the_stencil_grows_the_page_as_an_inserted_one_does() {
    let mut s = started();
    run(&mut s, "shape.drop", json!({"master": "xfunc.swimlane", "page": 1, "x": 5.0, "y": 4.0}));
    assert_eq!(lane_boxes(&mut s).len(), 4);
    assert_lanes_on_the_page(&mut s, "a dropped swimlane");
}

#[test]
fn a_lane_made_thicker_and_lanes_turned_over_their_diagonal_take_the_page_with_them() {
    // The two other commands that put lanes where there were none: a lane resized pushes the ones after it, and turning the structure makes a column of the lanes.
    let mut s = started();
    let lanes = lane_ids(&mut s);
    run(&mut s, "swimlane.resize", json!({"id": lanes[0], "size": 5.0}));
    assert_lanes_on_the_page(&mut s, "a thicker lane");
    assert!(page_size(&mut s).1 > 8.5);
    let mut t = started();
    run(&mut t, "swimlane.orientation", json!({"orientation": "vertical"}));
    assert_lanes_on_the_page(&mut t, "vertical lanes");
    assert_eq!(page_size(&mut t), (11.0, 17.0), "ten inches of lanes under the top margin: one more sheet");
}

#[test]
fn lanes_inserted_up_to_the_most_a_list_takes_all_stay_on_the_page_and_the_page_stays_in_whole_sheets() {
    let mut s = started();
    let mut made = 3;
    loop {
        match s.execute("swimlane.insert", &json!({})) {
            Ok(_) => made += 1,
            Err(e) => {
                assert!(e.to_string().contains("at most 256 lanes"), "{e}");
                break;
            }
        }
        assert!(made <= 256, "the cap holds");
    }
    assert_eq!(made, 256);
    assert_eq!(lane_boxes(&mut s).len(), 256);
    assert_lanes_on_the_page(&mut s, "256 lanes");
    let (w, h) = page_size(&mut s);
    assert_eq!(w, 11.0);
    assert!((h / 8.5 - (h / 8.5).round()).abs() < 1e-9, "a whole number of sheets of 8.5 inches: {h}");
    check_session(&s).unwrap();
}

#[test]
fn a_page_with_auto_size_off_does_not_grow_for_a_lane_as_the_rule_says() {
    // Auto Size is the page's own choice (`page.auto_size`): off, a lane put past the edge stays there and the page is as it was (is about Auto Size on).
    let mut s = started();
    run(&mut s, "page.auto_size", json!({"on": false}));
    run(&mut s, "swimlane.insert", json!({}));
    assert_eq!(page_size(&mut s), (11.0, 8.5));
    let boxes = lane_boxes(&mut s);
    assert!(boxes[3][1] < 0.0, "the fourth lane is past the bottom edge: {boxes:?}");
}
