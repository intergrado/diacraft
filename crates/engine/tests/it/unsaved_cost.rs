//! What reading a cell that holds a formula and no saved value costs, counted: a command works out the cells of the shapes it reads and no others, the page
//! read for the snap is paid for once for a revision and not again by the queries a drag makes, and a hostile page runs the steps out and is read as saved from there on: it
//! never hangs.
//!
//! The counts are those of `diacraft_engine::draw::worked` (cells worked out, answers found again, cells left to their saved reading, evaluators made, steps spent), kept for the
//! thread that runs the command; a count does not move with the load on the machine, as a time does. The timed twin of the page of 30,000 shapes is ignored.

use diacraft_doc::{Cell, CellAddr, Document, RowKey};
use diacraft_engine::Session;
use diacraft_engine::draw::worked::{CHUNK, Counts, WORKED_STEPS, WorkedBudget, counts, reset_counts};
use diacraft_engine::snap::{Kind, SnapSettings, make_ready};
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};
use serde_json::json;

use crate::{blank_session, run};

fn session_with(built: Document) -> Session {
    let mut s = blank_session();
    s.edit("build", |d| {
        *d = built;
        Ok(())
    })
    .unwrap();
    s
}

/// One shape: 1 by 1, its pin at its middle, `PinX` the formula `pin_x` and no saved value, the rest saved.
fn shape(id: u32, pin_x: &str) -> ShapeBuilder {
    let (col, row) = (id % 200, id / 200);
    ShapeBuilder::new(id)
        .set(CellAddr::direct("PinX"), Cell::with_formula(pin_x, None))
        .cell("PinY", &(f64::from(row) * 1.5 + 1.0).to_string())
        .cell("Width", "1")
        .cell("Height", "1")
        .cell("LocPinX", "0.5")
        .cell("LocPinY", "0.5")
        .cell("Angle", "0")
        .cell("DisplayMode", &(col % 3).to_string())
}

/// A page of `n` shapes whose `PinX` is a formula and no saved value: shape `id` is at x = `id % 200` by the formula `id%200+0`.
fn page_of(n: u32) -> Document {
    let mut page = PageBuilder::new().cell("PageWidth", "400").cell("PageHeight", "400");
    for id in 1..=n {
        page = page.shape(shape(id, &format!("{}+0", id % 200)));
    }
    DocBuilder::new().page(1, page).build()
}

fn ids(first: u32, n: u32) -> Vec<u32> {
    (first..first + n).collect()
}

// ---- a command works out what it reads ---------------------------------------------------------------------------------------------------------------

/// What moving shapes 1 to 50 by an inch costs on a page of `n` shapes.
fn counted_move(n: u32) -> Counts {
    let mut s = session_with(page_of(n));
    reset_counts();
    let moved = run(&mut s, "shape.move", json!({"ids": ids(1, 50), "dx": 1.0, "dy": 0.0, "page": 1}));
    assert_eq!(moved["moved"], json!(50));
    counts()
}

#[test]
fn a_move_works_out_the_cells_of_the_shapes_it_moves_and_the_cost_does_not_grow_with_the_page() {
    let (small, large) = (counted_move(300), counted_move(30_000));
    eprintln!("moving 50 shapes: {small:?} on 300, {large:?} on 30,000");
    assert!(large.worked_out >= 50, "the pin of each shape moved was worked out: {large:?}");
    assert_eq!(large.unanswered, 0, "{large:?}");
    assert_eq!(small.worked_out, large.worked_out, "the same cells at 300 shapes and at 30,000: {small:?} {large:?}");
    assert_eq!(small.evaluators, large.evaluators, "{small:?} {large:?}");
    assert!(large.steps <= 50 * 8 * 100, "a few readers and a few steps a cell: {large:?}");
}

/// What Size & Position costs for shapes 1 to 50 on a page of `n` shapes.
fn counted_placement(n: u32) -> Counts {
    let mut s = session_with(page_of(n));
    reset_counts();
    let shown = run(&mut s, "shape.placement", json!({"ids": ids(1, 50), "page": 1}));
    assert_eq!(shown["shapes"].as_array().map(Vec::len), Some(50));
    counts()
}

#[test]
fn size_and_position_works_out_each_shape_once_and_with_one_evaluator_whatever_the_page() {
    let (small, large) = (counted_placement(300), counted_placement(30_000));
    assert_eq!((large.worked_out, large.evaluators, large.unanswered), (50, 1, 0), "a pin a shape, and one evaluator for the page: {large:?}");
    assert_eq!(small, large, "and the same at 300 shapes");
}

// ---- the snap's page is paid for once for a revision --------------------------------------------------------------------------------------------------

#[test]
fn the_page_read_for_the_snap_works_each_cell_out_once_and_the_queries_of_a_drag_work_out_no_page() {
    // 10,000 shapes: the most the index reads. The window's reader makes the page ready once for a revision; the commands a drag sends (`snap.at`, at every pointer move) find it.
    let mut s = session_with(page_of(10_000));
    reset_counts();
    let doc = s.document().unwrap();
    let page = s.snap_page(1).unwrap();
    make_ready(doc, &page, &SnapSettings::of(doc), &[1], Kind::Box { framed: false });
    let ready = counts();
    eprintln!("the page made ready: {ready:?}");
    assert!(ready.worked_out >= 9_000 && ready.worked_out <= 10_000, "the pin of each shape the index reads, once: {ready:?}");
    assert_eq!(ready.evaluators, 1, "one evaluator for the whole page: {ready:?}");
    assert_eq!(ready.unanswered, 0, "{ready:?}");
    reset_counts();
    for step in 0..20 {
        let near = 2.0 + f64::from(step) * 0.01;
        let at = run(&mut s, "snap.at", json!({"page": 1, "ids": [1], "left": near, "bottom": 1.0, "right": near + 1.0, "top": 2.0}));
        assert!(at.is_object());
    }
    let queries = counts();
    eprintln!("twenty queries: {queries:?}");
    assert!(queries.worked_out <= 20 * 40, "a query works out the cells of the few shapes it looks at, and no page: {queries:?}");
    assert!(queries.evaluators <= 20 * 4, "{queries:?}");
}

// ---- a hostile page ----------------------------------------------------------------------------------------------------------------------------------

/// A chain of `n` cells of one shape, each the one before and one, and the shape's pin the last.
fn chained(n: u32) -> Document {
    let user = |name: String| CellAddr::in_row("User", None, RowKey::name(&name), "Value");
    let mut shape = ShapeBuilder::new(1).set(user("C0".to_owned()), Cell::with_formula("1", None));
    for i in 1..=n {
        shape = shape.set(user(format!("C{i}")), Cell::with_formula(&format!("User.C{}+1", i - 1), None));
    }
    shape = shape.set(CellAddr::direct("PinX"), Cell::with_formula(&format!("User.C{n}"), None));
    let shape = shape.cell("PinY", "1").cell("Width", "1").cell("Height", "1").cell("LocPinX", "0").cell("LocPinY", "0");
    DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build()
}

#[test]
fn a_pin_that_is_a_chain_of_cells_too_long_for_a_part_of_the_steps_is_given_up_on_and_read_as_saved() {
    // About 400,000 steps in all against a part of 50,000: no hang, and Size & Position shows what a cell with no value always showed.
    let mut s = session_with(chained(50_000));
    reset_counts();
    let shown = run(&mut s, "shape.placement", json!({"ids": [1], "page": 1}));
    let c = counts();
    assert_eq!(shown["shapes"][0]["x"], json!(0.0), "{shown}");
    assert!(c.unanswered >= 1 && c.steps <= CHUNK + 1_000, "given up on at the end of a part, not after the whole chain: {c:?}");
    // A chain that fits is worked out.
    let mut s = session_with(chained(2_000));
    let shown = run(&mut s, "shape.placement", json!({"ids": [1], "page": 1}));
    assert_eq!(shown["shapes"][0]["x"], json!(2_001.0), "{shown}");
}

#[test]
fn a_cycle_of_cells_with_no_saved_value_is_no_position_and_no_hang() {
    let user = |name: &str| CellAddr::in_row("User", None, RowKey::name(name), "Value");
    let shape = ShapeBuilder::new(1)
        .set(CellAddr::direct("PinX"), Cell::with_formula("User.A+1", None))
        .set(user("A"), Cell::with_formula("User.B+1", None))
        .set(user("B"), Cell::with_formula("User.A+1", None))
        .set(CellAddr::direct("PinY"), Cell::with_formula("PinY+1", None))
        .cell("Width", "1")
        .cell("Height", "1");
    let mut s = session_with(DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build());
    let shown = run(&mut s, "shape.placement", json!({"ids": [1], "page": 1}));
    assert!(shown["shapes"][0]["x"].is_number() && shown["shapes"][0]["y"].is_number(), "{shown}");
}

#[test]
fn a_page_whose_formulas_cost_more_than_the_steps_of_its_budget_is_read_as_saved_from_there_on() {
    // A formula such as `3+0` is some eight steps, so 2,000 shapes are some 16,000: read under a budget of 5,000, the index is made, the first cells are answered, the rest are
    // read as saved (no value, which reads 0), and the steps spent are the budget's and no more. (The budget a command has is `WORKED_STEPS`, 16 million.)
    let s = session_with(page_of(2_000));
    let budget = WorkedBudget::new(5_000);
    let _scope = budget.enter();
    reset_counts();
    let doc = s.document().unwrap();
    let ready = s.snap_page(1).unwrap();
    make_ready(doc, &ready, &SnapSettings::of(doc), &[1], Kind::Box { framed: false });
    let c = counts();
    eprintln!("a page past its budget: {c:?}");
    assert!(c.worked_out > 100 && c.worked_out < 2_000, "some were answered and not all: {c:?}");
    assert!(c.unanswered > 0, "and the rest were left, and counted: {c:?}");
    assert!(c.steps <= 5_000, "the steps spent are the budget's: {c:?}");
    assert!(budget.left() < 100, "the budget was spent down to less than a formula: {}", budget.left());
    assert_eq!(c.evaluators, 1, "{c:?}");
}

#[test]
fn a_command_that_finds_its_budget_spent_reads_every_cell_as_saved_and_works_nothing_out() {
    // The steps of a command are one budget for all its readers: spent, the readers made after read as saved, and fail nothing. Size & Position under a budget of nothing shows the
    // pin as a cell with no value always showed it.
    let mut s = session_with(page_of(10));
    let budget = WorkedBudget::new(0);
    let _scope = budget.enter();
    reset_counts();
    // (The session enters a budget of its own for a command it runs; a reader made outside one takes this.)
    let doc = s.document().unwrap();
    let placed = diacraft_engine::draw::placements(doc, 1, &[1, 2, 3]).unwrap();
    assert!(placed.iter().all(|p| p.pin.0 == 0.0), "{placed:?}");
    let c = counts();
    assert_eq!((c.worked_out, c.evaluators), (0, 1), "{c:?}");
    assert!(c.unanswered >= 1, "{c:?}");
    // The same page read under the budget the commands have: the pins are where the formulas put them.
    drop(_scope);
    let full = WorkedBudget::new(WORKED_STEPS);
    let _scope = full.enter();
    let placed = diacraft_engine::draw::placements(s.document().unwrap(), 1, &[1, 2, 3]).unwrap();
    assert_eq!(placed.iter().map(|p| p.pin.0).collect::<Vec<_>>(), [1.0, 2.0, 3.0]);
    let _ = run(&mut s, "shape.placement", json!({"ids": [1], "page": 1}));
}

// ---- the timed twin ----------------------------------------------------------------------------------------------------------------------------------

#[test]
#[ignore = "timed: a page of 30,000 shapes whose pins are formulas and no saved value; the counted tests above hold the work, this the time"]
fn timed_a_page_of_thirty_thousand_such_shapes_is_read_by_a_command_and_the_snap_in_well_under_a_second_each() {
    let mut s = session_with(page_of(30_000));
    let started = std::time::Instant::now();
    let shown = run(&mut s, "shape.placement", json!({"ids": ids(1, 50), "page": 1}));
    let placement = started.elapsed();
    assert_eq!(shown["shapes"].as_array().map(Vec::len), Some(50));
    let started = std::time::Instant::now();
    let doc = s.document().unwrap();
    let page = s.snap_page(1).unwrap();
    make_ready(doc, &page, &SnapSettings::of(doc), &[1], Kind::Box { framed: false });
    let ready = started.elapsed();
    eprintln!("Size & Position of 50: {placement:?}; the page made ready for the snap: {ready:?}; steps of the budget: {WORKED_STEPS}");
    assert!(placement < std::time::Duration::from_secs(1), "{placement:?}");
    assert!(ready < std::time::Duration::from_secs(5), "{ready:?}");
}
