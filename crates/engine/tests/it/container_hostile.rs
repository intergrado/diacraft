//! Containers on hostile drawings (the security scan's three findings): ties that name thousands of shapes, containers nested a thousand deep, thousands of
//! containers that hold the same shapes. Each is built by hand (a file may hold anything) and held to a **count of work or a refusal in words**, never a long run: the
//! costs are the steps the command charges (`diacraft_engine::draw::container::MAX_WORK` is the most one command may charge) and the number of containers one command
//! fits (`MAX_FITTED`). A time is measured only by the tests marked `#[ignore]`, against a plain pass over the same shapes (run them with `--ignored`).

use std::sync::Arc;

use diacraft_doc::{Cell, CellAddr, RowKey, Shape, Value};
use diacraft_engine::draw::container::MAX_FITTED;
use serde_json::json;

use crate::edit::{doc, num};
use crate::shapeops::{error_of, put, rect, session_with};
use crate::{blank_session, check_session, run, snapshot};

/// The `Relationships` cell of a shape that holds the shapes under 1 and is held by the containers under 4.
fn ties_cell(holds: &[u32], held_by: &[u32]) -> Cell {
    let mut calls: Vec<String> = Vec::new();
    if !holds.is_empty() {
        let list: Vec<String> = holds.iter().map(|id| format!("Sheet.{id}!SheetRef()")).collect();
        calls.push(format!("DEPENDSON(1,{})", list.join(",")));
    }
    if !held_by.is_empty() {
        let list: Vec<String> = held_by.iter().map(|id| format!("Sheet.{id}!SheetRef()")).collect();
        calls.push(format!("DEPENDSON(4,{})", list.join(",")));
    }
    Cell::with_formula(&format!("SUM({})", calls.join(",")), Some(Value::from_text("0")))
}

/// A rectangle with the ties, typed a container when `container`, and in the automatic mode `resize` (0 none, 1 expand, 2 fit) when it is one.
fn tied(id: u32, at: (f64, f64, f64, f64), holds: &[u32], held_by: &[u32], container: bool, resize: u8) -> Shape {
    let mut shape = rect(id, at).cell("Relationships", ties_cell(holds, held_by)).build();
    let sheet = Arc::make_mut(&mut shape.sheet);
    if container {
        let text = |t: &str| Cell::constant(Value::from_text(t));
        sheet.set_cell(&CellAddr::in_row("User", None, RowKey::name("msvStructureType"), "Value"), text("Container")).unwrap();
        sheet.set_cell(&CellAddr::in_row("User", None, RowKey::name("DiaCraftContainerResize"), "Value"), text(&resize.to_string())).unwrap();
    }
    if holds.is_empty() && held_by.is_empty() {
        sheet.cells.remove("Relationships");
    }
    shape
}

#[test]
fn containers_nested_a_thousand_deep_are_all_fitted_when_the_innermost_member_grows() {
    // The refit goes up the nesting in one plan, and not in a pass for each level that gives up after 64: the outermost container holds the leaf with the margins of all of them.
    const DEPTH: u32 = 1_000;
    let leaf_id = DEPTH + 1;
    let mut shapes = Vec::new();
    // Container `n` (1 outermost) holds container `n + 1`, the last holds the leaf; each box is the leaf's with a quarter inch more for each level above it.
    for n in 1..=DEPTH {
        let margin = 0.25 * f64::from(DEPTH - n + 1);
        let holds = [n + 1];
        let held_by: Vec<u32> = if n > 1 { vec![n - 1] } else { Vec::new() };
        shapes.push(tied(n, (50.0 - margin, 50.0 - margin, 52.0 + margin, 52.0 + margin), &holds, &held_by, true, 2));
    }
    shapes.push(tied(leaf_id, (50.0, 50.0, 52.0, 52.0), &[], &[DEPTH], false, 0));
    let mut s = session_with(shapes);
    // The leaf grows by a unit to the right.
    run(&mut s, "shape.resize", json!({"id": leaf_id, "x": 50.0, "y": 50.0, "width": 3.0, "height": 2.0}));
    let right = |id: u32| {
        let d = doc(&s);
        num(d, id, "PinX") - num(d, id, "LocPinX") + num(d, id, "Width")
    };
    assert!((right(leaf_id) - 53.0).abs() < 1e-6);
    assert!((right(DEPTH) - 53.25).abs() < 1e-6, "the innermost container holds the leaf with its margin: {}", right(DEPTH));
    assert!((right(1) - (53.0 + 0.25 * f64::from(DEPTH))).abs() < 1e-6, "the outermost holds all the margins: {}", right(1));
    check_session(&s).unwrap();
}

#[test]
fn a_fit_that_would_resize_more_containers_than_one_command_may_is_refused_in_words() {
    // Ten thousand containers that hold the same shapes: a shape moved in them is in all, and fitting them all is more than a command does.
    let containers: Vec<u32> = (1..=10_000).collect();
    let members: Vec<u32> = (10_001..=10_010).collect();
    let mut shapes = Vec::new();
    for c in &containers {
        shapes.push(tied(*c, (0.0, 0.0, 10.0, 10.0), &members, &[], true, 2));
    }
    for (k, m) in members.iter().enumerate() {
        let x = 1.0 + k as f64 * 0.5;
        shapes.push(tied(*m, (x, 1.0, x + 0.25, 1.25), &[], &containers, false, 0));
    }
    let mut s = session_with(shapes);
    let before = snapshot(&s);
    let e = error_of(&mut s, "shape.move", &json!({"ids": [10_001], "dx": 0.1, "dy": 0.0}));
    let words = e.to_string();
    assert!(words.contains("containers") && words.contains(&MAX_FITTED.to_string()), "the refusal says what the most is: {words}");
    assert_eq!(snapshot(&s), before, "a refusal leaves the session as it was");
}

#[test]
fn a_container_that_holds_as_many_shapes_as_a_formula_can_name_is_read_in_one_pass() {
    // The members are named once each, and asked about by an index: no list is searched for a shape for each of the others.
    // A formula is read as naming 20,000 shapes at most (`MAX_TIE_MARKS`, below the parser's node budget): a container holds that many.
    const N: u32 = 20_000;
    let members: Vec<u32> = (2..=N + 1).collect();
    let mut shapes = vec![tied(1, (0.0, 0.0, 400.0, 400.0), &members, &[], true, 0)];
    for (k, m) in members.iter().enumerate() {
        let (col, row) = (k % 300, k / 300);
        let (x, y) = (1.0 + col as f64, 1.0 + row as f64);
        shapes.push(tied(*m, (x, y, x + 0.5, y + 0.5), &[], &[1], false, 0));
    }
    let mut s = session_with(shapes);
    let listed = run(&mut s, "container.select_contents", json!({"ids": [1]}));
    assert_eq!(listed["ids"].as_array().unwrap().len(), N as usize);
    let info = run(&mut s, "container.info", json!({"ids": [1]}));
    assert_eq!(info["containers"][0]["members"].as_array().unwrap().len(), N as usize);
    // Fit to Contents reads every member's box once.
    let fitted = run(&mut s, "container.fit", json!({"ids": [1]}));
    assert_eq!(fitted["resized"], json!([1]));
    check_session(&s).unwrap();
}

/// How long a closure takes, in seconds.
fn seconds(f: impl FnOnce()) -> f64 {
    let t = std::time::Instant::now();
    f();
    t.elapsed().as_secs_f64()
}

#[test]
#[ignore = "a time, against a plain pass over the same shapes: run with --ignored"]
fn the_reads_of_a_container_of_many_shapes_cost_a_small_multiple_of_a_plain_pass() {
    const N: u32 = 20_000;
    let members: Vec<u32> = (2..=N + 1).collect();
    let mut shapes = vec![tied(1, (0.0, 0.0, 400.0, 400.0), &members, &[], true, 0)];
    for (k, m) in members.iter().enumerate() {
        let (col, row) = (k % 300, k / 300);
        shapes.push(tied(*m, (1.0 + col as f64, 1.0 + row as f64, 1.5 + col as f64, 1.5 + row as f64), &[], &[1], false, 0));
    }
    let mut s = session_with(shapes);
    // The yardstick: one pass over every shape of the page (its box), which is what reading the members costs at least.
    let plain = seconds(|| {
        let d = doc(&s);
        let mut sum = 0.0;
        for sh in d.pages.iter().next().unwrap().shapes.iter() {
            sum += num(d, sh.id, "Width");
        }
        assert!(sum > 0.0);
    });
    let listed = seconds(|| {
        run(&mut s, "container.select_contents", json!({"ids": [1]}));
        run(&mut s, "container.info", json!({"ids": [1]}));
    });
    assert!(listed < 30.0 * plain + 1.0, "reading the members took {listed} s against {plain} s for a plain pass");
    let _ = blank_session;
    let _ = put;
}

#[test]
fn a_container_that_a_formula_cannot_name_all_the_members_of_is_refused_in_words_and_nothing_is_written() {
    // The `Relationships` cell is a formula, and a formula holds a bounded number of nodes: joining a shape to a container that names as many as it can hold is refused, with the
    // container as it was, and not written as a formula that no reader can take back.
    let names = |n: u32| -> String {
        let list: Vec<String> = (0..n).map(|k| format!("Sheet.{}!SheetRef()", 100_000 + k)).collect();
        format!("SUM(DEPENDSON(1,{}))", list.join(","))
    };
    // The most shapes a formula is read as naming (`MAX_TIE_MARKS`, below the parser's node budget), less a thousand.
    let held = diacraft_engine::draw::callouts::MAX_TIE_MARKS as u32 - 1_000;
    assert!(diacraft_formula::parse(&names(held)).is_parsed());
    let outside: Vec<u32> = (1..=2_000).collect();
    let mut container =
        rect(5_000_000, (0.0, 0.0, 10.0, 10.0)).cell("Relationships", Cell::with_formula(&names(held), Some(Value::from_text("0")))).build();
    Arc::make_mut(&mut container.sheet)
        .set_cell(&CellAddr::in_row("User", None, RowKey::name("msvStructureType"), "Value"), Cell::constant(Value::from_text("Container")))
        .unwrap();
    let mut shapes = vec![container];
    for id in &outside {
        shapes.push(rect(*id, (20.0, 20.0, 21.0, 21.0)).build());
    }
    let mut s = session_with(shapes);
    let before = snapshot(&s);
    let e = error_of(&mut s, "container.add_member", &json!({"ids": outside, "container": 5_000_000}));
    let words = e.to_string();
    assert!(words.contains("more shapes than a formula can hold"), "{words}");
    assert_eq!(snapshot(&s), before, "nothing was written");
    // Fewer shapes fit.
    let few: Vec<u32> = (1..=100).collect();
    let r = run(&mut s, "container.add_member", json!({"ids": few, "container": 5_000_000}));
    assert_eq!(r["joined"].as_array().unwrap().len(), 100);
}

#[test]
fn a_move_of_thousands_of_members_among_thousands_of_containers_keeps_each_member_where_it_is() {
    // Thousands of shapes each tried against thousands of containers: the work is charged, and the budget that is left decides, never "no box" or "no container" (a shape whose
    // box could not be measured was taken for one that is in no container, and left its own). The members stay inside their containers as they are moved a hundredth of an inch.
    const CONTAINERS: u32 = 2_000;
    let mut shapes = Vec::new();
    let mut members_of: Vec<(u32, [u32; 2])> = Vec::new();
    for k in 0..CONTAINERS {
        let id = k + 1;
        let (x, y) = (f64::from(k % 50) * 3.0, f64::from(k / 50) * 3.0);
        let members = [CONTAINERS + 1 + 2 * k, CONTAINERS + 2 + 2 * k];
        shapes.push(tied(id, (x, y, x + 2.5, y + 2.5), &members, &[], true, 0));
        shapes.push(tied(members[0], (x + 0.4, y + 0.4, x + 0.9, y + 0.9), &[], &[id], false, 0));
        shapes.push(tied(members[1], (x + 1.4, y + 0.4, x + 1.9, y + 0.9), &[], &[id], false, 0));
        members_of.push((id, members));
    }
    let mut s = session_with(shapes);
    let all: Vec<u32> = members_of.iter().flat_map(|(_, m)| m.iter().copied()).collect();
    assert_eq!(all.len(), 4_000);
    run(&mut s, "shape.move", json!({"ids": all, "dx": 0.01, "dy": 0.0}));
    let d = doc(&s);
    for (c, members) in &members_of {
        for m in members {
            let at = d.find_shape(diacraft_doc::Owner::Page(1), *m).unwrap();
            let text = d
                .shape(&at)
                .and_then(|sh| sh.sheet.cell("Relationships").and_then(|cell| cell.formula.text().map(str::to_owned)))
                .unwrap_or_default();
            assert_eq!(text, format!("SUM(DEPENDSON(4,Sheet.{c}!SheetRef()))"), "member {m} of container {c}");
        }
    }
    check_session(&s).unwrap();
}
