//! Unit tests of what the swimlane answers read: the work a read of a lane's label may do is the command's budget, charged for each part of the text it walks; and what a file's own
//! cells make of a list is read once for a page read, with the reading of a frame's lists stopped at the third.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use diacraft_doc::{Cell, CellAddr, CellName, RowKey, Shape, ShapeType, Text, TextPart, Value};
use diacraft_testkit::fixtures::blank_document;

use super::lists::{Dir, MAX_LIST_MEMBERS, Role, direction, role};
use super::page::Page;
use super::swimlane::label_of;
use crate::draw::page_mut;

/// The most parts of a label that are walked (`swimlane::MAX_LABEL_PARTS`, spelled out so that the test is its own yardstick).
const CAP: usize = 1_024;

/// A document whose first page holds the shape 1, with a label of `fields` field parts and then one character.
fn page_with_a_label_of(fields: usize) -> diacraft_doc::Document {
    let mut doc = blank_document();
    let page = page_mut(&mut doc, 1).unwrap();
    let mut shape = Shape::new(1);
    shape.kind = ShapeType::Shape;
    let mut parts: Vec<TextPart> = (0..fields).map(|_| TextPart::Fld { ix: 0, text: Arc::from("") }).collect();
    parts.push(TextPart::Chars(Arc::from("x")));
    shape.text = Some(Arc::new(Text { parts, ..Text::default() }));
    page.shapes.push(Arc::new(shape));
    doc
}

#[test]
fn a_label_of_a_million_field_parts_is_walked_at_a_step_for_each_part_and_no_further_than_the_cap() {
    // The security scan's finding: the walk charged once and then visited every part that was no characters for nothing, so a label of a million fields was walked in full for 4 steps.
    let doc = page_with_a_label_of(1_000_000);
    let p = Page::read(&doc, 1).unwrap();
    let before = p.work.left();
    let label = label_of(&p, 1).unwrap();
    let spent = before - p.work.left();
    assert!(spent >= CAP, "{spent} steps for a label of a million parts: each part walked is a step");
    assert!(spent <= 2 * CAP, "{spent} steps: the walk stops at the cap of parts and does not go on to the million");
    assert_eq!(label, "", "the characters after the cap are not read");
}

#[test]
fn a_label_of_few_parts_is_read_whole_and_a_spent_budget_is_an_error_and_never_an_empty_label() {
    let doc = page_with_a_label_of(3);
    let p = Page::read(&doc, 1).unwrap();
    assert_eq!(label_of(&p, 1).unwrap(), "x");
    // A budget that cannot pay for the parts is an error in words.
    let _edit = super::EditWork::enter_with(1_500);
    let doc = page_with_a_label_of(1_000);
    let p = Page::read(&doc, 1).unwrap();
    label_of(&p, 1).unwrap();
    let e = label_of(&p, 1).expect_err("the second read of a thousand parts is more than 1,500 steps");
    assert!(e.message.contains("steps of work in one command"), "{}", e.message);
}

#[test]
fn a_label_that_is_long_enough_stops_the_walk_before_the_parts_that_follow() {
    // 256 characters in the first part: the walk ends there, whatever the million parts after it.
    let mut doc = blank_document();
    let page = page_mut(&mut doc, 1).unwrap();
    let mut shape = Shape::new(1);
    shape.kind = ShapeType::Shape;
    let mut parts = vec![TextPart::Chars(Arc::from("y".repeat(300)))];
    parts.extend((0..1_000_000).map(|_| TextPart::Fld { ix: 0, text: Arc::from("") }));
    shape.text = Some(Arc::new(Text { parts, ..Text::default() }));
    page.shapes.push(Arc::new(shape));
    let p = Page::read(&doc, 1).unwrap();
    let before = p.work.left();
    let label = label_of(&p, 1).unwrap();
    assert_eq!(label.chars().count(), 256);
    assert!(before - p.work.left() <= 8, "{} steps: the parts after the 256th character are not visited", before - p.work.left());
}

// ---- the lists another program wrote -------------------------------------------------------------------------------------------------------------------

/// A shape of a file: typed `kind` by its own `User.msvStructureType`, with the `Relationships` formula `ties`, and the box from `bottom` to `top`, 6 inches wide.
fn file_shape(id: u32, kind: &str, ties: &str, (bottom, top): (f64, f64)) -> Shape {
    let mut shape = Shape::new(id);
    shape.kind = ShapeType::Shape;
    let sheet = Arc::make_mut(&mut shape.sheet);
    let n = |v: f64| Cell::constant(Value::from_text(&format!("{v}"))).with_unit("IN");
    for (name, cell) in [
        ("PinX", n(3.0)),
        ("PinY", n(f64::midpoint(bottom, top))),
        ("Width", n(6.0)),
        ("Height", n(top - bottom)),
        ("LocPinX", n(3.0)),
        ("LocPinY", n((top - bottom) / 2.0)),
    ] {
        sheet.cells.insert(CellName::new(name), cell);
    }
    let typed = CellAddr::in_row("User", None, RowKey::name("msvStructureType"), "Value");
    sheet.set_cell(&typed, Cell::with_formula(&format!("\"{kind}\""), Some(Value::from_text(kind)))).unwrap();
    sheet.cells.insert(CellName::new("Relationships"), Cell::with_formula(ties, Some(Value::from_text("0"))));
    shape
}

/// `Sheet.<id>!SheetRef()` for each of `ids`, parted by commas.
fn sheets(ids: impl Iterator<Item = u32>) -> String {
    ids.map(|id| format!("Sheet.{id}!SheetRef()")).collect::<Vec<_>>().join(",")
}

#[test]
fn what_a_files_list_and_lanes_are_is_found_once_for_a_read_and_not_once_for_each_lane() {
    // A list of 512 lanes, the most that is laid out: a role or a direction asked of each lane reads the list once for the read, and not its 512 names for each of 512 lanes.
    let n = u32::try_from(MAX_LIST_MEMBERS).unwrap();
    let mut doc = blank_document();
    let page = page_mut(&mut doc, 1).unwrap();
    page.shapes.push(Arc::new(file_shape(1, "List", &format!("SUM(DEPENDSON(2,{}))", sheets(2..=n + 1)), (0.0, f64::from(n)))));
    for id in 2..=n + 1 {
        let top = f64::from(n + 2 - id);
        page.shapes.push(Arc::new(file_shape(id, "Container", "SUM(DEPENDSON(5,Sheet.1!SheetRef()))", (top - 1.0, top))));
    }
    let p = Page::read(&doc, 1).unwrap();
    let before = p.work.left();
    for id in 2..=n + 1 {
        assert_eq!(role(&p, id).unwrap(), Role::Lane, "lane {id}");
        assert_eq!(direction(&p, 1).unwrap(), Some(Dir::Down));
    }
    assert_eq!(role(&p, 1).unwrap(), Role::Lanes);
    let spent = before - p.work.left();
    assert!(spent < 64 * MAX_LIST_MEMBERS, "{spent} steps for {n} lanes: the list is read once, and each lane in a few steps");
}

#[test]
fn a_frame_of_a_file_that_holds_a_thousand_lists_is_read_for_three_of_them_and_left_as_it_is() {
    // A structure has two lists at most: the third stops the reading of the frame's lists, whatever comes after it.
    let lists = 1_000_u32;
    let mut doc = blank_document();
    let page = page_mut(&mut doc, 1).unwrap();
    page.shapes.push(Arc::new(file_shape(1, "Container", &format!("SUM(DEPENDSON(1,{}))", sheets(2..=lists + 1)), (0.0, 10.0))));
    for id in 2..=lists + 1 {
        let lane = id + lists;
        let list_ties = format!("SUM(DEPENDSON(4,Sheet.1!SheetRef()),DEPENDSON(2,Sheet.{lane}!SheetRef()))");
        page.shapes.push(Arc::new(file_shape(id, "List", &list_ties, (0.0, 1.0))));
        page.shapes.push(Arc::new(file_shape(lane, "Container", &format!("SUM(DEPENDSON(5,Sheet.{id}!SheetRef()))"), (0.0, 1.0))));
    }
    let p = Page::read(&doc, 1).unwrap();
    // What the frame's formula costs to read, twice (the list it is in and its members), is paid whatever is done with it; the lists past the third are not looked at.
    let before = p.work.left();
    assert_eq!(role(&p, 1).unwrap(), Role::Other, "which list is the lanes is not guessed");
    let spent = before - p.work.left();
    let formula = 2 * (sheets(2..=lists + 1).len() / 16 + usize::try_from(lists).unwrap() / 4);
    assert!(spent < formula + 200, "{spent} steps, of which {formula} read the frame's formula: the lists after the third are not read");
    assert_eq!(role(&p, 2).unwrap(), Role::Other, "and a list of the frame is no list of lanes");
}

/// A page of `shapes`, each `(id, kind, Relationships formula)`, back to front.
fn page_of(shapes: &[(u32, &str, String)]) -> diacraft_doc::Document {
    let mut doc = blank_document();
    let page = page_mut(&mut doc, 1).unwrap();
    for (id, kind, ties) in shapes {
        page.shapes.push(Arc::new(file_shape(*id, kind, ties, (0.0, 1.0))));
    }
    doc
}

#[test]
fn a_chain_of_a_hundred_thousand_containers_each_naming_the_next_as_its_list_is_read_without_walking_down_it() {
    // The security scan's finding: a container's role followed the list it names through the role of that list, so a container naming a container naming a container... recursed
    // once for each link, and a file of such a chain overflowed the stack. A container names a list only when the shape it names is typed `List`.
    let n = 100_000_u32;
    let shapes: Vec<(u32, &str, String)> = (1..=n)
        .map(|id| (id, "Container", if id < n { format!("SUM(DEPENDSON(5,Sheet.{}!SheetRef()))", id + 1) } else { "SUM(DEPENDSON(5))".to_owned() }))
        .collect();
    let doc = page_of(&shapes);
    let p = Page::read(&doc, 1).unwrap();
    for id in [1, n / 2, n] {
        assert_eq!(role(&p, id).unwrap(), Role::Other, "container {id} is in no list of lanes");
    }
    // Every shape asked, as `swimlane.info` asks the page's containers: within the one budget.
    for id in 1..=n {
        role(&p, id).unwrap();
    }
}

#[test]
fn two_containers_that_name_each_other_as_their_list_and_a_list_that_names_itself_are_no_parts_of_a_structure() {
    let loop_of_two = page_of(&[
        (1, "Container", "SUM(DEPENDSON(5,Sheet.2!SheetRef()))".to_owned()),
        (2, "Container", "SUM(DEPENDSON(5,Sheet.1!SheetRef()))".to_owned()),
    ]);
    let p = Page::read(&loop_of_two, 1).unwrap();
    assert_eq!((role(&p, 1).unwrap(), role(&p, 2).unwrap()), (Role::Other, Role::Other));
    assert_eq!(direction(&p, 1).unwrap(), None);
    // A list that holds itself and is its own list, and a container that names it as its list.
    let own = page_of(&[
        (1, "List", "SUM(DEPENDSON(2,Sheet.1!SheetRef()),DEPENDSON(5,Sheet.1!SheetRef()),DEPENDSON(4,Sheet.1!SheetRef()))".to_owned()),
        (2, "Container", "SUM(DEPENDSON(5,Sheet.1!SheetRef()))".to_owned()),
    ]);
    let p = Page::read(&own, 1).unwrap();
    assert_eq!((role(&p, 1).unwrap(), role(&p, 2).unwrap()), (Role::Other, Role::Other));
    assert_eq!(direction(&p, 1).unwrap(), None);
    // Two lists, each the other's only member and each in the other: neither holds a container, so neither is a list of lanes.
    let lists = page_of(&[
        (1, "List", "SUM(DEPENDSON(2,Sheet.2!SheetRef()),DEPENDSON(5,Sheet.2!SheetRef()),DEPENDSON(4,Sheet.2!SheetRef()))".to_owned()),
        (2, "List", "SUM(DEPENDSON(2,Sheet.1!SheetRef()),DEPENDSON(5,Sheet.1!SheetRef()),DEPENDSON(4,Sheet.1!SheetRef()))".to_owned()),
    ]);
    let p = Page::read(&lists, 1).unwrap();
    assert_eq!((role(&p, 1).unwrap(), role(&p, 2).unwrap()), (Role::Other, Role::Other));
    assert_eq!((direction(&p, 1).unwrap(), direction(&p, 2).unwrap()), (None, None));
}
