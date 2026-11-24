//! Unit tests of the container model: the work a command may do is one budget, a spent budget is an error and never an answer, and a listing of many members asks a set.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, ShapeType, Sheet, Value};
use diacraft_testkit::fixtures::blank_document;

use super::membership::{Graph, underlying};
use super::page::{EditWork, PROBES, Page};
use super::{Add, Document, add, arrive, page_mut, select_contents, with_contents};

/// A document with a container (the shape 1, the box 0 to 10 by 0 to 10) and a loose shape (the shape 2, the box 4 to 5 by 4 to 5) inside it that is not a member.
fn container_and_a_shape() -> Document {
    let mut doc = blank_document();
    let added = add(&mut doc, 1, Add { style: "frame", heading: "", around: &[], centre: (5.0, 5.0), size: (10.0, 10.0), above: 0 }).unwrap();
    assert_eq!(added.id, 1);
    let mut shape = diacraft_doc::Shape::new(2);
    shape.kind = ShapeType::Shape;
    let mut sheet = Sheet::default();
    for (name, v) in [("PinX", 4.5), ("PinY", 4.5), ("Width", 1.0), ("Height", 1.0), ("LocPinX", 0.5), ("LocPinY", 0.5), ("Angle", 0.0)] {
        sheet.cells.insert(CellName::new(name), Cell::constant(Value::from_text(&v.to_string())).with_unit("IN"));
    }
    shape.sheet = Arc::new(sheet);
    page_mut(&mut doc, 1).unwrap().shapes.push(Arc::new(shape));
    doc
}

#[test]
fn a_spent_budget_stops_a_membership_step_in_words_and_changes_nothing() {
    // The edit's budget is shared by every read of it: with too little, the step is refused and the document is as it was, and the shape is not taken for one that is in no
    // container.
    let mut doc = container_and_a_shape();
    let before = doc.clone();
    let _edit = EditWork::enter_with(12);
    let e = arrive(&mut doc, 1, &[2]).unwrap_err();
    assert!(e.message.contains("steps of work in one command"), "{}", e.message);
    assert_eq!(doc, before);
}

#[test]
fn with_enough_work_the_same_step_puts_the_shape_in_the_container() {
    let mut doc = container_and_a_shape();
    arrive(&mut doc, 1, &[2]).unwrap();
    let p = Page::read(&doc, 1).unwrap();
    assert_eq!(p.members_of(1).unwrap(), [2]);
}

#[test]
fn the_container_a_shape_is_under_is_never_none_for_want_of_work() {
    let doc = container_and_a_shape();
    let _edit = EditWork::enter_with(60);
    let page = Page::read(&doc, 1).unwrap();
    let mut answers = 0;
    loop {
        match underlying(&page, &mut Graph::new(), 2, None) {
            Ok(Some(c)) => {
                assert_eq!(c, 1);
                answers += 1;
            }
            Ok(None) => panic!("a spent budget was read as no container after {answers} answers"),
            Err(e) => {
                assert!(e.message.contains("steps of work in one command"), "{}", e.message);
                break;
            }
        }
        assert!(answers < 100, "the budget never ran out");
    }
    assert!(answers >= 1, "the first answer was had before the budget ran out");
}

#[test]
fn every_listing_of_many_members_asks_a_set_and_not_the_list() {
    const N: u32 = 20_000;
    let mut doc = container_and_a_shape();
    // A container that names N shapes, which are on the page.
    let names: Vec<String> = (10..10 + N).map(|k| format!("Sheet.{k}!SheetRef()")).collect();
    {
        let page = page_mut(&mut doc, 1).unwrap();
        for k in 10..10 + N {
            let mut shape = diacraft_doc::Shape::new(k);
            shape.kind = ShapeType::Shape;
            page.shapes.push(Arc::new(shape));
        }
        let first = page.shapes.get_mut(0).unwrap();
        let sheet = Arc::make_mut(&mut Arc::make_mut(first).sheet);
        sheet.cells.insert(
            CellName::new("Relationships"),
            Cell::with_formula(&format!("SUM(DEPENDSON(1,{}))", names.join(",")), Some(Value::from_text("0"))),
        );
    }
    let p = Page::read(&doc, 1).unwrap();
    PROBES.with(|c| c.set(0));
    let members = p.members_of(1).unwrap();
    let held = p.contents(&[1]).unwrap();
    let listed = select_contents(&doc, 1, &[1]).unwrap();
    let with = with_contents(&doc, 1, &[1]).unwrap();
    assert_eq!((members.len(), held.len(), listed.len(), with.len()), (N as usize, N as usize, N as usize, N as usize + 1));
    let probes = PROBES.with(|c| c.get());
    assert!(
        probes <= 4 * N as usize + 16,
        "{probes} probes for {N} shapes listed a few times: one for each shape each time, not a search of the list"
    );
}

#[test]
fn a_loop_of_a_thousand_containers_put_in_by_hand_is_not_fitted_and_its_plan_is_one_walk() {
    // No command makes a loop, and the fit stays bounded with one all the same: each container is looked at once, and the one named is left alone.
    const N: u32 = 1_000;
    let mut doc = blank_document();
    {
        let page = page_mut(&mut doc, 1).unwrap();
        for id in 1..=N {
            let mut shape = diacraft_doc::Shape::new(id);
            shape.kind = ShapeType::Shape;
            let mut sheet = Sheet::default();
            let (x, y) = (f64::from(id % 50) * 2.0, f64::from(id / 50) * 2.0);
            for (name, v) in [("PinX", x), ("PinY", y), ("Width", 1.0), ("Height", 1.0), ("LocPinX", 0.5), ("LocPinY", 0.5), ("Angle", 0.0)] {
                sheet.cells.insert(CellName::new(name), Cell::constant(Value::from_text(&v.to_string())).with_unit("IN"));
            }
            let (next, before) = (id % N + 1, if id == 1 { N } else { id - 1 });
            let ring = format!("SUM(DEPENDSON(1,Sheet.{next}!SheetRef()),DEPENDSON(4,Sheet.{before}!SheetRef()))");
            sheet.cells.insert(CellName::new("Relationships"), Cell::with_formula(&ring, Some(Value::from_text("0"))));
            // The automatic resizing is None: only the one named is fitted, by the command.
            let none = diacraft_doc::CellAddr::in_row("User", None, diacraft_doc::RowKey::name("DiaCraftContainerResize"), "Value");
            sheet.set_cell(&none, Cell::constant(Value::from_text("0"))).unwrap();
            shape.sheet = Arc::new(sheet);
            page.shapes.push(Arc::new(shape));
        }
    }
    let before = doc.clone();
    let _edit = EditWork::enter();
    let fitted = super::fit(&mut doc, 1, &[1]).unwrap();
    assert!(fitted.resized.is_empty() && fitted.skipped.contains(&1), "a container in a loop is left alone: {fitted:?}");
    assert_eq!(doc, before);
    let spent = super::MAX_WORK - super::page::Work::new().left();
    assert!(spent <= 64 * N as usize, "{spent} steps for a loop of {N}: each container is looked at once");
}
