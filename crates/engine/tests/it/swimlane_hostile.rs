//! Swimlanes on hostile drawings: a list of 10,000 lanes, lanes nested in lanes, a list whose ties loop. A file may hold anything, so each is built by hand and held to a **count of
//! work or a refusal in words**, never a long run or a crash: the most one command lays out is `MAX_LIST_MEMBERS`, the most it follows is `MAX_FOLLOWING` (20,000), and a loop is broken at the open.

use std::sync::Arc;

use diacraft_doc::{Cell, CellAddr, RowKey, Shape, Value};
use serde_json::json;

use crate::container::boxed;
use crate::edit::{doc, num};
use crate::shapeops::{error_of, rect, session_with};
use crate::{check_session, run, snapshot};

/// The most members of a list that are laid out (`draw::container::MAX_LIST_MEMBERS`, spelled out so that the test is its own yardstick).
const MAX_LIST_MEMBERS: usize = 512;

/// A `Relationships` cell of `DEPENDSON` calls, one for each code that names someone.
fn calls_cell(calls: &[(u32, Vec<u32>)]) -> Option<Cell> {
    let calls: Vec<String> = calls
        .iter()
        .filter(|(_, ids)| !ids.is_empty())
        .map(|(code, ids)| {
            let list: Vec<String> = ids.iter().map(|id| format!("Sheet.{id}!SheetRef()")).collect();
            format!("DEPENDSON({code},{})", list.join(","))
        })
        .collect();
    (!calls.is_empty()).then(|| Cell::with_formula(&format!("SUM({})", calls.join(",")), Some(Value::from_text("0"))))
}

/// A box with the ties given and the rows a part of a structure has: its kind (`msvStructureType`), its role and, for a list, its direction.
fn part(id: u32, at: (f64, f64, f64, f64), calls: &[(u32, Vec<u32>)], kind: &str, role: &str, direction: Option<f64>) -> Shape {
    let mut build = rect(id, at);
    if let Some(cell) = calls_cell(calls) {
        build = build.cell("Relationships", cell);
    }
    let mut shape = build.build();
    let sheet = Arc::make_mut(&mut shape.sheet);
    let text = |t: &str| Cell::constant(Value::from_text(t));
    let row = |name: &str| CellAddr::in_row("User", None, RowKey::name(name), "Value");
    sheet.set_cell(&row("msvStructureType"), text(kind)).unwrap();
    sheet.set_cell(&row("DiaCraftSwimlane"), text(role)).unwrap();
    sheet.set_cell(&row("DiaCraftContainerResize"), text("0")).unwrap();
    if let Some(d) = direction {
        sheet.set_cell(&row("DiaCraftListDirection"), text(&d.to_string())).unwrap();
    }
    shape
}

/// A frame (1) that holds a list (2) of `n` thin lanes (3 up), one under the next.
fn many_lanes(n: u32) -> Vec<Shape> {
    let lanes: Vec<u32> = (3..3 + n).collect();
    let height = f64::from(n) * 0.01;
    let mut shapes = vec![
        part(1, (1.0, 1.0, 7.0, 1.0 + height), &[(1, vec![2])], "Container", "frame", None),
        part(2, (1.0, 1.0, 7.0, 1.0 + height), &[(2, lanes.clone()), (4, vec![1])], "List", "lanes", Some(0.0)),
    ];
    for (k, id) in lanes.iter().enumerate() {
        let top = 1.0 + height - k as f64 * 0.01;
        shapes.push(part(*id, (1.0, top - 0.01, 7.0, top), &[(5, vec![2])], "Container", "lane", None));
    }
    shapes
}

#[test]
fn a_list_of_ten_thousand_lanes_moves_with_its_frame_and_is_refused_in_words_when_a_lane_is_moved_or_added() {
    let mut s = session_with(many_lanes(10_000));
    // The frame takes the list and each lane once (20,000 shapes follow an edit at most).
    run(&mut s, "shape.move", json!({"ids": [1], "dx": 1.0, "dy": 0.0}));
    assert!(
        (num(doc(&s), 3, "PinX") - 5.0).abs() < 1e-9 && (num(doc(&s), 10_002, "PinX") - 5.0).abs() < 1e-9,
        "the first lane and the last moved with the frame"
    );
    // A lane moved in it would put the whole list in order again, which is more than a list of this length is laid out.
    let before = snapshot(&s);
    let e = error_of(&mut s, "shape.move", &json!({"ids": [5], "dx": 0.0, "dy": 0.05}));
    assert!(e.to_string().contains(&format!("more than the {MAX_LIST_MEMBERS} that DiaCraft lays out")), "{e}");
    assert_eq!(snapshot(&s).document, before.document, "a refusal changes nothing");
    let e = error_of(&mut s, "swimlane.insert", &json!({"lane": 3}));
    assert!(e.to_string().contains("at most 256 lanes"), "{e}");
    // The structure is told in a bounded answer.
    let info = run(&mut s, "swimlane.info", json!({}));
    assert!(info["structures"][0]["lanes"].as_array().unwrap().len() <= MAX_LIST_MEMBERS);
    check_session(&s).unwrap();
}

#[test]
fn lanes_nested_a_thousand_deep_move_with_the_outermost_and_a_lane_among_them_is_laid_out() {
    // Lane `n` holds lane `n + 1`; only the first is in the list.
    const DEPTH: u32 = 1_000;
    let mut shapes = vec![
        part(1, (0.0, 0.0, 9.0, 3.0), &[(1, vec![2])], "Container", "frame", None),
        part(2, (0.0, 0.0, 9.0, 3.0), &[(2, vec![3]), (4, vec![1])], "List", "lanes", Some(0.0)),
    ];
    for n in 0..DEPTH {
        let id = 3 + n;
        let calls: Vec<(u32, Vec<u32>)> = if n == 0 {
            vec![(5, vec![2]), (1, vec![id + 1])]
        } else if n + 1 < DEPTH {
            vec![(4, vec![id - 1]), (1, vec![id + 1])]
        } else {
            vec![(4, vec![id - 1])]
        };
        let inset = f64::from(n) * 0.002;
        shapes.push(part(id, (inset, inset, 9.0 - inset, 3.0 - inset), &calls, "Container", "lane", None));
    }
    let mut s = session_with(shapes);
    run(&mut s, "shape.move", json!({"ids": [1], "dx": 2.0, "dy": 0.0}));
    assert!((num(doc(&s), 3 + DEPTH - 1, "PinX") - (4.5 + 2.0)).abs() < 1e-9, "the innermost lane moved with the frame, once");
    run(&mut s, "swimlane.resize", json!({"id": 3, "size": 2.0}));
    assert!((boxed(doc(&s), 3).3 - boxed(doc(&s), 3).1 - 2.0).abs() < 1e-9);
    check_session(&s).unwrap();
}

#[test]
fn a_list_that_holds_a_lane_that_holds_the_list_is_moved_each_shape_once_and_laid_out_without_a_loop() {
    // A file may hold a loop that the open breaks; a library caller can make one by hand, and no walk of a command loops on it.
    let shapes = vec![
        part(1, (0.0, 0.0, 6.0, 3.0), &[(2, vec![2, 3]), (1, vec![1])], "List", "lanes", Some(0.0)),
        part(2, (0.0, 1.5, 6.0, 3.0), &[(5, vec![1]), (1, vec![1])], "Container", "lane", None),
        part(3, (0.0, 0.0, 6.0, 1.5), &[(5, vec![1])], "Container", "lane", None),
    ];
    let mut s = session_with(shapes);
    run(&mut s, "shape.move", json!({"ids": [2], "dx": 1.0, "dy": 0.0}));
    assert!((num(doc(&s), 2, "PinX") - 4.0).abs() < 1e-9, "the lane moved once");
    assert!((num(doc(&s), 1, "PinX") - 4.0).abs() < 1e-9, "and the list it holds moved once with it: {}", num(doc(&s), 1, "PinX"));
    check_session(&s).unwrap();
}

#[test]
fn a_list_of_ten_thousand_phases_is_refused_in_words_when_a_separator_is_added() {
    let phases: Vec<u32> = (4..4 + 10_000).collect();
    let mut shapes = vec![
        part(1, (0.0, 0.0, 100.0, 3.0), &[(1, vec![2, 3])], "Container", "frame", None),
        part(2, (0.0, 0.0, 100.0, 2.5), &[(2, vec![]), (4, vec![1])], "List", "lanes", Some(0.0)),
        part(3, (0.0, 2.5, 100.0, 2.85), &[(2, phases.clone()), (4, vec![1])], "List", "phases", Some(1.0)),
    ];
    for (k, id) in phases.iter().enumerate() {
        let x = k as f64 * 0.01;
        shapes.push(part(*id, (x, 0.0, x + 0.01, 2.85), &[(5, vec![3])], "Container", "phase", None));
    }
    let mut s = session_with(shapes);
    let e = error_of(&mut s, "swimlane.insert_phase", &json!({"phase": 4}));
    assert!(e.to_string().contains("at most 256 phases") || e.to_string().contains(&format!("more than the {MAX_LIST_MEMBERS}")), "{e}");
    check_session(&s).unwrap();
}

#[test]
fn one_command_never_charges_more_than_the_edits_budget_for_a_structure_of_the_most_lanes() {
    // The most lanes a command makes (256), each with a phase: made in one command, and laid out again by an insert, within the budget of one edit.
    let mut s = crate::blank_session();
    let made = run(&mut s, "swimlane.create", json!({"x": 0.5, "y": 10.0, "length": 7.0, "thickness": 0.4, "lanes": 255, "phases": 255}));
    assert_eq!(made["lanes"].as_array().unwrap().len(), 255);
    let new = run(&mut s, "swimlane.insert", json!({}));
    assert_eq!(new["index"], 255);
    let e = error_of(&mut s, "swimlane.insert", &json!({}));
    assert!(e.to_string().contains("at most 256 lanes"), "{e}");
    check_session(&s).unwrap();
}

#[test]
fn a_structure_asked_for_with_hostile_numbers_is_refused_in_words() {
    let mut s = crate::blank_session();
    let before = snapshot(&s);
    for (params, words) in [
        (json!({"lanes": 0}), "`lanes` must be from 1 to 256"),
        (json!({"lanes": 257}), "at most 256"),
        (json!({"phases": 1000}), "at most 256"),
        (json!({"thickness": 0.01}), "at least 0.4"),
        (json!({"length": 0.1}), "at least 0.4"),
        (json!({"orientation": "diagonal"}), "horizontal or vertical"),
        (json!({"x": 1.0e300}), "x"),
    ] {
        let e = error_of(&mut s, "swimlane.create", &params);
        assert!(e.to_string().contains(words), "{params}: {e}");
    }
    assert_eq!(snapshot(&s), before, "a refusal leaves the session as it was");
}

#[test]
fn a_label_of_a_hundred_thousand_characters_is_told_in_a_bounded_answer_and_costs_no_search_of_the_page() {
    let mut s = crate::blank_session();
    let made = run(&mut s, "swimlane.create", json!({"x": 1.0, "y": 8.0, "length": 6.0, "thickness": 1.0, "lanes": 3, "phases": 3}));
    let lanes: Vec<u32> = made["lanes"].as_array().unwrap().iter().map(|v| u32::try_from(v.as_u64().unwrap()).unwrap()).collect();
    run(&mut s, "shape.set_text", json!({"id": lanes[0], "text": "x".repeat(100_000)}));
    let info = run(&mut s, "swimlane.info", json!({}));
    let told = info["structures"][0]["lanes"][0]["text"].as_str().unwrap();
    assert_eq!(told.chars().count(), 256, "the answer tells the first 256 characters of a label and not all of it");
    check_session(&s).unwrap();
}

#[test]
fn rotating_the_labels_of_a_list_of_more_lanes_than_a_list_is_laid_out_with_is_refused_in_words() {
    // A frame names every lane of its list: a list of 600 lanes is more than the 512 a list is laid out with, and each lane's label is restyled, so the command says so and changes nothing.
    let mut s = session_with(many_lanes(600));
    let before = snapshot(&s);
    let e = error_of(&mut s, "swimlane.rotate_label", &json!({"ids": [1]}));
    assert!(e.to_string().contains(&format!("more than the {MAX_LIST_MEMBERS}")), "{e}");
    assert_eq!(snapshot(&s).document, before.document, "a refusal changes nothing");
}
