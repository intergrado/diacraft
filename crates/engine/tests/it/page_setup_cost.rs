//! What Auto Size costs: the hook is a read of two cells with Auto Size off, one walk of the page with it on, and a carry of the top-level
//! shapes only when the page grows on the left or the bottom. The cost is counted, not timed (`diacraft_calc::work`: the commits a command makes
//! and the steps they take, which do not move with the load on the machine; a ceiling in seconds failed tests of this kind under load): the
//! assertions stop a quadratic cost or a recalculation of the whole page. The times in `docs/editing-commands.md`, "What it costs", were measured
//! once and are not asserted.

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Shape, Sheet, Value};
use diacraft_engine::{EngineError, Session};
use serde_json::json;

use crate::edit_cost::{CONNECTORS, big_page, recalculation_of, steps_of_the_whole_page};
use crate::{blank_session, check_session, run};

fn cell(s: &Session, name: &str) -> String {
    s.document().unwrap().page(1).unwrap().sheet.cell(name).unwrap().value.as_ref().unwrap().text().to_owned()
}

#[test]
fn one_move_on_a_page_of_500_shapes_and_300_connectors_with_auto_size_off_and_on() {
    let mut s = big_page();
    // The fixture's shapes run to x 75 and y 40: a page that holds them, so that no move grows it.
    run(&mut s, "page.size", json!({"width": 100, "height": 60}));
    let whole = steps_of_the_whole_page(&s);
    let moved = 5 * 25 + 4 + 1;
    let one_move = |s: &mut Session, label: &str| {
        let mut worst = (0, 0);
        for round in 0..7 {
            let (commits, steps) =
                recalculation_of(s, "shape.move", &json!({"ids": [moved], "dx": 0.05, "dy": if round % 2 == 0 { 0.1 } else { -0.1 }}));
            worst = (commits.max(worst.0), steps.max(worst.1));
        }
        eprintln!(
            "one move on 500 shapes and {CONNECTORS} connectors, Auto Size {label}: {} commits of {} steps; the whole page is {whole}",
            worst.0, worst.1
        );
        worst
    };
    let off = one_move(&mut s, "off");
    run(&mut s, "page.auto_size", json!({"on": true}));
    let on = one_move(&mut s, "on, no growth");
    assert_eq!((off.0, on.0), (1, 1), "one commit for a move, with the hook on and off");
    // The hook with the page not growing is a read of two cells: it adds no step of the recalculation, and a move still costs a part of the page.
    assert_eq!(on.1, off.1, "the hook works nothing out when the page does not grow");
    assert!(on.1 * 3 < whole, "a move took {} steps with Auto Size on, the whole page {whole}", on.1);
    assert_eq!(cell(&s, "PageWidth"), "100", "the shapes are inside the page, so it did not grow");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_growth_on_the_left_carries_500_shapes_and_300_connectors_once() {
    let mut s = big_page();
    run(&mut s, "page.auto_size", json!({"on": true}));
    let whole = steps_of_the_whole_page(&s);
    // The page is 11 inches and the 25 columns of the fixture run to x 75: grow it first, on the right, so the carry has its shapes.
    let (right_commits, right_steps) = recalculation_of(&mut s, "shape.drop", &json!({"master": "rectangle", "page": 1, "x": 80.0, "y": 4.0}));
    let width = cell(&s, "PageWidth").parse::<f64>().unwrap();
    let (left_commits, left_steps) = recalculation_of(&mut s, "shape.drop", &json!({"master": "rectangle", "page": 1, "x": -0.5, "y": 4.0}));
    eprintln!(
        "a drop that grows the page on the right: {right_commits} commits of {right_steps} steps; on the left, carrying 501 shapes and {CONNECTORS} connectors: {left_commits} commits of {left_steps} steps; the whole page is {whole}"
    );
    assert_eq!(cell(&s, "PageWidth").parse::<f64>().unwrap(), width + 11.0);
    // Counted, not timed: the carry is one commit, and works out the cells that read what it moved, not the page over again for each shape
    // (a recalculation for each of the 501 shapes would be 501 commits).
    assert_eq!((right_commits, left_commits), (1, 1), "one commit for each drop");
    assert!(right_steps < whole && left_steps < whole, "{right_steps} and {left_steps} steps, the whole page {whole}");
    assert_eq!(check_session(&s), Ok(()));
}

/// A session whose first page holds `n` top-level shapes made straight in the model: a pin and a size each, no master, no connector.
fn page_of(n: u32) -> Session {
    let mut s = blank_session();
    s.edit("build", |doc| {
        let page = doc.pages.get_mut(0).map(Arc::make_mut).ok_or(EngineError::Failed { cmd: "test".into(), msg: "no page".into() })?;
        for i in 0..n {
            let mut sheet = Sheet::default();
            let (col, row) = (f64::from(i % 100), f64::from(i / 100));
            for (name, value) in
                [("PinX", col * 0.1 + 0.5), ("PinY", row * 0.05 + 0.5), ("Width", 0.05), ("Height", 0.04), ("LocPinX", 0.025), ("LocPinY", 0.02)]
            {
                sheet.cells.insert(CellName::new(name), Cell::constant(Value::from_text(&value.to_string())).with_unit("IN"));
            }
            let mut shape = Shape::new(i + 1);
            shape.sheet = Arc::new(sheet);
            page.shapes.push(Arc::new(shape));
        }
        Ok(())
    })
    .unwrap();
    s
}

#[test]
fn a_page_of_20000_shapes_is_carried_in_one_commit_and_a_page_of_100000_is_left_where_it_is() {
    // The most a carry takes: MAX_SHIFT_SHAPES top-level shapes, the shape that made the page grow among them.
    let n = u32::try_from(diacraft_engine::draw::MAX_SHIFT_SHAPES).unwrap();
    let mut s = page_of(n - 1);
    run(&mut s, "page.auto_size", json!({"on": true}));
    let (commits, steps) = recalculation_of(&mut s, "shape.drop", &json!({"master": "rectangle", "page": 1, "x": -0.5, "y": 4.0}));
    eprintln!("a drop that grows a page of {} shapes on the left, carrying them all: {commits} commits of {steps} steps", n - 1);
    assert_eq!(cell(&s, "PageWidth"), "22");
    // Counted, not timed: one commit for the drop and the carry, a few steps for each shape carried, and not one commit for each of the 20,000.
    assert_eq!(commits, 1, "one commit for the carry of {} shapes", n - 1);
    assert!(steps <= 5 * u64::from(n), "{steps} steps to carry {} shapes that hold no formula (four for each measured)", n - 1);

    // One more shape than a carry takes: the left is not grown, and the drop costs the walk and nothing more.
    let mut big = page_of(n);
    run(&mut big, "page.auto_size", json!({"on": true}));
    let (commits, steps) = recalculation_of(&mut big, "shape.drop", &json!({"master": "rectangle", "page": 1, "x": -0.5, "y": 4.0}));
    eprintln!("a drop past the left of a page of {n} shapes, one of them the new one, which is not carried: {commits} commits of {steps} steps");
    assert_eq!(cell(&big, "PageWidth"), "11");
    assert!(commits <= 1, "{commits} commits for a drop that carries nothing");
    assert!(steps < 10_000, "{steps} steps for a drop that carries nothing");
}

#[test]
fn the_hook_on_a_page_of_100000_shapes_is_one_walk() {
    let mut s = page_of(100_000);
    let drops = |s: &mut Session, label: &str| {
        let mut worst = (0, 0);
        for _ in 0..5 {
            let (commits, steps) = recalculation_of(s, "shape.drop", &json!({"master": "rectangle", "page": 1, "x": 5.0, "y": 4.0}));
            worst = (commits.max(worst.0), steps.max(worst.1));
        }
        eprintln!("a drop inside a page of 100000 shapes, Auto Size {label}: {} commits of {} steps", worst.0, worst.1);
        worst
    };
    let off = drops(&mut s, "off");
    run(&mut s, "page.auto_size", json!({"on": true}));
    let on = drops(&mut s, "on, no growth");
    // Counted, not timed: a drop is one commit of the new shape's cells, with the hook on or off, whatever the page holds. That the hook is one
    // walk of the page (a visit to each shape) is counted in the unit test of `draw/page_fit.rs`.
    assert!(off.0 <= 1 && on.0 <= 1, "{off:?} and {on:?}: one commit at the most for a drop");
    assert!(off.1 < 10_000 && on.1 < 10_000, "{off:?} and {on:?}: the 100,000 shapes hold no formula, so a drop works out the new shape's cells");
    assert_eq!(cell(&s, "PageWidth"), "11");
}
