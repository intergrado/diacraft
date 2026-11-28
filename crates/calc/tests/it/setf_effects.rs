//! `SETF` writes when its cell is worked out (documented behaviour
//! wins over), within a loop budget and a work budget, and the old reading as a setting (`SetfMode::Inert`).
//!
//! The evaluator holds the document and never changes it, so what these tests watch is the commit: `commit`, `commit_in` and `commit_changed` make the writes
//! the calls asked for, work out again what they changed, and stop at the caps. The vectors are of the spec, run on a commit, and the
//! hostile shapes (a call that sets itself off, two that set each other off, a chain of thousands of cells, thousands of calls) are counted, and bounded in time too.

use std::time::{Duration, Instant};

use diacraft_calc::rules::{MAX_SETF_ROUNDS, MAX_SETF_WRITES};
use diacraft_calc::{Budget, CommitReport, EvalOptions, Scope, SetfMode, Value, changes_between, commit, commit_changed, commit_in};
use diacraft_doc::{Cell, CellAddr, CellEdit, Document, FormulaSlot, Lookup, Owner, RowKey, ShapeRef, SheetRef, Value as DocValue, WriteOrigin};
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};

/// A bound a sound run is far inside: a hang is what it is there for.
const BOUND: Duration = Duration::from_secs(60);

fn user(name: &str) -> CellAddr {
    CellAddr::in_row("User", None, RowKey::name(name), "Value")
}

/// `Scratch.<letter><n>`.
fn scratch(letter: &str, n: u32) -> CellAddr {
    CellAddr::in_row("Scratch", None, RowKey::Ix(n - 1), letter)
}

fn shape_sheet(id: u32) -> SheetRef {
    SheetRef::Shape(ShapeRef::top(Owner::Page(1), id - 1))
}

fn doc_of(shapes: Vec<ShapeBuilder>) -> Document {
    let mut page = PageBuilder::new();
    for shape in shapes {
        page = page.shape(shape);
    }
    DocBuilder::new().page(1, page).build()
}

fn one(shape: ShapeBuilder) -> Document {
    doc_of(vec![shape])
}

/// A user cell with a formula (the saved value 0), the way a file holds one that SETF cells sit in.
fn with_formula(shape: ShapeBuilder, name: &str, formula: &str) -> ShapeBuilder {
    shape.row_formula("User", None, RowKey::name(name), "Value", formula, "0")
}

fn found(doc: &Document, id: u32, addr: &CellAddr) -> Cell {
    let at = ShapeRef::top(Owner::Page(1), id - 1);
    match doc.shape(&at).expect("the shape").sheet.lookup(addr) {
        Lookup::Found(c) => c.clone(),
        other => panic!("{addr:?}: {other:?}"),
    }
}

/// The formula the cell holds of its own, as text, or none.
fn formula(doc: &Document, id: u32, addr: &CellAddr) -> Option<String> {
    match found(doc, id, addr).formula {
        FormulaSlot::Local(f) => Some(f.source.to_string()),
        _ => None,
    }
}

fn value(doc: &Document, id: u32, addr: &CellAddr) -> Option<String> {
    found(doc, id, addr).value.map(|v| v.text().to_owned())
}

fn error(doc: &Document, id: u32, addr: &CellAddr) -> Option<String> {
    found(doc, id, addr).error.map(|e| e.to_string())
}

fn works(doc: &mut Document) -> CommitReport {
    commit(doc, &Budget::default(), EvalOptions::default())
}

fn inert() -> EvalOptions {
    EvalOptions { setf: SetfMode::Inert, ..EvalOptions::default() }
}

// ---- the vectors, on a commit -----------------------------------------------------------------------------------------------------------------

/// Turned over: a `SETF` in a `User` cell outside any action writes when its cell is worked out, and what it wrote is worked out from its own formula.
#[test]
fn a_setf_writes_its_target_when_its_cell_is_worked_out() {
    let mut doc = one(with_formula(ShapeBuilder::new(1).row_cell("Scratch", None, RowKey::Ix(0), "B", "1"), "Auto", "SETF(GETREF(Scratch.B1),4.25)"));
    let report = works(&mut doc);
    assert_eq!(formula(&doc, 1, &scratch("B", 1)).as_deref(), Some("4.25"), "the formula is the cell's own now: {report:?}");
    assert_eq!(value(&doc, 1, &scratch("B", 1)).as_deref(), Some("4.25"), "and it was worked out");
    assert_eq!((report.setf_written, report.setf_dropped, report.setf_refused), (1, 0, 0), "{report:?}");
    assert!(!report.budget_exhausted, "{report:?}");
    // The call is 0, and a second commit finds the cell holding its text: nothing is written.
    assert_eq!(value(&doc, 1, &user("Auto")).as_deref(), Some("0"));
    let again = works(&mut doc);
    assert_eq!((again.setf_written, again.setf_same), (0, 1), "{again:?}");
    assert_eq!(formula(&doc, 1, &scratch("B", 1)).as_deref(), Some("4.25"));
}

/// A text is the formula, and its value is worked out from it.
#[test]
fn a_text_is_written_as_the_formula_and_worked_out() {
    let shape = with_formula(
        ShapeBuilder::new(1).cell("Height", "13").row_cell("Scratch", None, RowKey::Ix(0), "A", "0"),
        "Go",
        r#"SETF(GETREF(Scratch.A1),"Height*7/13")"#,
    );
    let mut doc = one(shape);
    works(&mut doc);
    assert_eq!(formula(&doc, 1, &scratch("A", 1)).as_deref(), Some("Height*7/13"));
    assert_eq!(value(&doc, 1, &scratch("A", 1)).as_deref(), Some("7"));
}

/// A string is written three quotation marks deep, and is a string in its cell.
#[test]
fn a_string_is_written_as_a_string() {
    let shape =
        with_formula(ShapeBuilder::new(1).row_cell("User", None, RowKey::name("Tag"), "Value", "0"), "Go", r#"SETF(GETREF(User.Tag),"""Basalt""")"#);
    let mut doc = one(shape);
    works(&mut doc);
    assert_eq!(formula(&doc, 1, &user("Tag")).as_deref(), Some("\"Basalt\""));
    assert_eq!(value(&doc, 1, &user("Tag")).as_deref(), Some("Basalt"));
}

/// The target is named through `GETREF` or as a string.
#[test]
fn a_string_that_names_a_cell_is_a_target() {
    let mut doc = one(with_formula(ShapeBuilder::new(1).row_cell("Scratch", None, RowKey::Ix(0), "A", "0"), "Go", r#"SETF("Scratch.A1",3)"#));
    let report = works(&mut doc);
    assert_eq!(formula(&doc, 1, &scratch("A", 1)).as_deref(), Some("3"), "{report:?}");
    assert_eq!(value(&doc, 1, &scratch("A", 1)).as_deref(), Some("3"));
}

/// A target named neither through `GETREF` nor as a string is an error, and no cell changes. Naive: writing 2.
#[test]
fn a_target_that_is_neither_getref_nor_a_string_is_an_error_and_nothing_is_written() {
    let mut doc = one(with_formula(ShapeBuilder::new(1).row_cell("Scratch", None, RowKey::Ix(0), "C", "5"), "Go", "SETF(Scratch.C1,2)"));
    let report = works(&mut doc);
    assert_eq!(error(&doc, 1, &user("Go")).as_deref(), Some("#VALUE!"), "{report:?}");
    assert_eq!(formula(&doc, 1, &scratch("C", 1)), None, "the cell was not given a formula");
    assert_eq!(value(&doc, 1, &scratch("C", 1)).as_deref(), Some("5"));
    assert_eq!(report.setf_written, 0, "{report:?}");
}

/// A formula with a syntax error is an error, and the target is left as it was.
#[test]
fn a_formula_that_does_not_parse_is_an_error_and_leaves_the_target() {
    let mut doc =
        one(with_formula(ShapeBuilder::new(1).row_cell("Scratch", None, RowKey::Ix(0), "A", "5"), "Go", r#"SETF(GETREF(Scratch.A1),"1 +")"#));
    let report = works(&mut doc);
    assert_eq!(error(&doc, 1, &user("Go")).as_deref(), Some("#VALUE!"), "{report:?}");
    assert_eq!((formula(&doc, 1, &scratch("A", 1)), value(&doc, 1, &scratch("A", 1)).as_deref()), (None, Some("5")));
}

/// A cell no sheet holds is not made by a write: `#REF!`, and the row is not there afterwards.
#[test]
fn a_target_that_is_nowhere_is_a_reference_error_and_no_row_is_made() {
    let mut doc = one(with_formula(ShapeBuilder::new(1), "Go", "SETF(GETREF(User.Nope),1)"));
    let report = works(&mut doc);
    assert_eq!(error(&doc, 1, &user("Go")).as_deref(), Some("#REF!"), "{report:?}");
    let at = ShapeRef::top(Owner::Page(1), 0);
    assert!(matches!(doc.shape(&at).expect("a shape").sheet.lookup(&user("Nope")), Lookup::Missing));
    assert_eq!(report.setf_written, 0);
}

/// The order, on a commit: the writes are made in the order the calls were reached, so the last one stays.
#[test]
fn calls_write_in_the_order_they_were_reached() {
    let shape = with_formula(
        ShapeBuilder::new(1).row_cell("User", None, RowKey::name("Step"), "Value", "0"),
        "Go",
        "SETF(GETREF(User.Step),2)+SETF(GETREF(User.Step),3)",
    );
    let mut doc = one(shape);
    let report = works(&mut doc);
    assert_eq!(formula(&doc, 1, &user("Step")).as_deref(), Some("3"), "{report:?}");
    assert_eq!(value(&doc, 1, &user("Step")).as_deref(), Some("3"));
    assert_eq!(report.setf_written, 1, "the call that wrote 2 was overwritten by the one after it, and the cell comes to rest at once: {report:?}");
    assert_eq!(report.setf_rounds, 2, "the cell was worked out again, its calls found it holding 3, and nothing more was written: {report:?}");
}

/// A branch that is not taken is not reached, and so writes nothing; the one that is taken writes, and the cell comes to rest when it is worked out again.
#[test]
fn only_the_branch_that_is_taken_writes() {
    for (a1, expected, written) in [("3", "3", 0), ("7", "5", 1)] {
        let shape = with_formula(
            ShapeBuilder::new(1).row_cell("Scratch", None, RowKey::Ix(0), "A", a1),
            "Go",
            "IF(Scratch.A1>5,SETF(GetRef(Scratch.A1),5),0)",
        );
        let mut doc = one(shape);
        let report = works(&mut doc);
        assert_eq!(value(&doc, 1, &scratch("A", 1)).as_deref(), Some(expected), "A1 = {a1}: {report:?}");
        assert_eq!(report.setf_written, written, "A1 = {a1}: {report:?}");
    }
}

/// A number is written in its unit, so the cell reads back what the call was given.
#[test]
fn a_number_with_a_unit_is_written_in_its_unit() {
    let mut doc = one(with_formula(ShapeBuilder::new(1).cell("Width", "1"), "Go", "SETF(GETREF(Width),2 in)"));
    let report = works(&mut doc);
    let written = formula(&doc, 1, &CellAddr::direct("Width")).expect("a formula of its own");
    assert!(written.starts_with('2') && written.to_lowercase().contains("in"), "{written:?}: {report:?}");
    assert_eq!(value(&doc, 1, &CellAddr::direct("Width")).as_deref(), Some("2"), "{report:?}");
}

/// A write is a cell write: no guard stops it, and it makes the formula the cell's own.
#[test]
fn a_write_replaces_a_guarded_formula() {
    let shape =
        with_formula(ShapeBuilder::new(1).row_formula("User", None, RowKey::name("G"), "Value", "GUARD(1)", "1"), "Go", "SETF(GETREF(User.G),6)");
    let mut doc = one(shape);
    works(&mut doc);
    assert_eq!(formula(&doc, 1, &user("G")).as_deref(), Some("6"));
    assert_eq!(value(&doc, 1, &user("G")).as_deref(), Some("6"));
}

/// `SETF` reaches any sheet of the drawing: the target may be another shape's cell.
#[test]
fn a_target_may_be_on_another_shape() {
    let go = with_formula(ShapeBuilder::new(1), "Go", "SETF(GETREF(Sheet.2!User.X),7)");
    let other = ShapeBuilder::new(2).row_cell("User", None, RowKey::name("X"), "Value", "0");
    let mut doc = doc_of(vec![go, other]);
    works(&mut doc);
    assert_eq!(formula(&doc, 2, &user("X")).as_deref(), Some("7"));
    assert_eq!(value(&doc, 2, &user("X")).as_deref(), Some("7"));
}

/// A call that writes its own cell replaces its own formula, and what is left is a number: the commit comes to rest.
#[test]
fn a_call_that_writes_its_own_cell_is_replaced_by_what_it_wrote() {
    let mut doc = one(with_formula(ShapeBuilder::new(1), "Me", "SETF(GETREF(User.Me),5)"));
    let report = works(&mut doc);
    assert_eq!(formula(&doc, 1, &user("Me")).as_deref(), Some("5"), "{report:?}");
    assert_eq!(report.setf_written, 1);
    assert_eq!(works(&mut doc).setf_written, 0);
}

// ---- the setting ------------------------------------------------------------------------------------------------------------------------------

/// As a setting: with `SetfMode::Inert` the call is 0 whatever it names, and nothing is written.
#[test]
fn the_inert_setting_keeps_the_old_reading() {
    let shape = with_formula(ShapeBuilder::new(1).row_cell("Scratch", None, RowKey::Ix(0), "B", "1"), "Auto", "SETF(GETREF(Scratch.B1),4.25)");
    let mut doc = one(with_formula(shape, "Bad", "SETF(Scratch.C1,2)"));
    let report = commit(&mut doc, &Budget::default(), inert());
    assert_eq!((formula(&doc, 1, &scratch("B", 1)), value(&doc, 1, &scratch("B", 1)).as_deref()), (None, Some("1")), "{report:?}");
    assert_eq!(error(&doc, 1, &user("Bad")), None, "a target the call does not name rightly is not looked at: it is 0");
    assert_eq!((report.setf_written, report.setf_same, report.setf_dropped, report.setf_rounds), (0, 0, 0, 0), "{report:?}");
}

/// The dry run of a typed formula writes nothing and says what a commit would make of the cell: 0, or the error of a call that names no cell.
#[test]
fn the_dry_run_of_a_text_writes_nothing() {
    let doc = one(ShapeBuilder::new(1).cell("Width", "3").row_cell("Scratch", None, RowKey::Ix(0), "C", "5"));
    let sheet = shape_sheet(1);
    let at = user("Probe");
    let run = |text: &str, options: &EvalOptions| diacraft_calc::evaluate_text_with(&doc, &sheet, Some(&at), text, options, &Budget::default());
    for (text, expected) in [("SETF(GetRef(Width),9)", Value::num(0.0)), ("SETF(Scratch.C1,2)", Value::Error(diacraft_calc::CellError::Value))] {
        let done = run(text, &EvalOptions::default());
        assert_eq!(done.result, diacraft_calc::EvalResult::Value(expected), "{text}");
    }
    let done = run("SETF(Scratch.C1,2)", &inert());
    assert_eq!(done.result, diacraft_calc::EvalResult::Value(Value::num(0.0)), "the old reading does not look at the target");
    assert_eq!(value(&doc, 1, &CellAddr::direct("Width")).as_deref(), Some("3"));
}

// ---- an edit's recalculation --------------------------------------------------------------------------------------------------------------------

fn set(doc: &mut Document, id: u32, addr: &CellAddr, text: &str) {
    doc.write_cell(&shape_sheet(id), addr, CellEdit::Value(DocValue::from_text(text)), WriteOrigin::Drawing).expect("a write");
}

/// A cell that reads what an edit changed is worked out again by it, and its `SETF` writes: the commit of an edit (`commit_changed`) performs the writes too.
#[test]
fn an_edit_that_changes_what_a_setf_cell_reads_makes_it_write() {
    let shape = with_formula(
        ShapeBuilder::new(1).cell("Width", "3").row_cell("Scratch", None, RowKey::Ix(0), "A", "0"),
        "Go",
        "SETF(GETREF(Scratch.A1),Width*2)",
    );
    let before = one(shape);
    let mut doc = before.clone();
    set(&mut doc, 1, &CellAddr::direct("Width"), "5");
    let changes = changes_between(&before, &doc, 1);
    let report = commit_changed(&mut doc, &Budget::default(), EvalOptions::default(), 1, &changes);
    assert_eq!(formula(&doc, 1, &scratch("A", 1)).as_deref(), Some("10"), "{report:?}");
    assert_eq!(value(&doc, 1, &scratch("A", 1)).as_deref(), Some("10"), "{report:?}");
    assert_eq!(report.setf_written, 1);
    // The same edit again: the target holds the text, and nothing is written.
    let before = doc.clone();
    set(&mut doc, 1, &CellAddr::direct("Width"), "5");
    let changes = changes_between(&before, &doc, 1);
    let again = commit_changed(&mut doc, &Budget::default(), EvalOptions::default(), 1, &changes);
    assert_eq!(again.setf_written, 0, "{again:?}");
    // The page's commit and the document's commit do the same.
    let mut page = doc.clone();
    set(&mut page, 1, &CellAddr::direct("Width"), "6");
    let r = commit_in(&mut page, &Budget::default(), EvalOptions::default(), Scope::Page(1));
    assert_eq!(formula(&page, 1, &scratch("A", 1)).as_deref(), Some("12"), "{r:?}");
}

/// An edit made as the engine makes one: the cell is written, and the commit answers what the comparison with the document before finds changed.
fn edited(doc: &mut Document, id: u32, addr: &CellAddr, text: &str) -> CommitReport {
    let before = doc.clone();
    set(doc, id, addr, text);
    let changes = changes_between(&before, doc, 1);
    commit_changed(doc, &Budget::default(), EvalOptions::default(), 1, &changes)
}

/// One shape with an idempotent `SETF` (`Go` writes twice the width into `Scratch.A1`), a counter (`Tick` adds `Step` to `N`, which it reads, each time it is worked out)
/// and a cell nothing reads (`Other`).
fn setf_shape() -> Document {
    let shape = ShapeBuilder::new(1)
        .cell("Width", "3")
        .row_cell("Scratch", None, RowKey::Ix(0), "A", "0")
        .row_cell("User", None, RowKey::name("N"), "Value", "0")
        .row_cell("User", None, RowKey::name("Step"), "Value", "1")
        .row_cell("User", None, RowKey::name("Other"), "Value", "0");
    let shape = with_formula(shape, "Go", "SETF(GETREF(Scratch.A1),Width*2)");
    one(with_formula(shape, "Tick", "SETF(GETREF(User.N),User.N+User.Step)"))
}

/// A `SETF` is worked out when its inputs change, and not after every edit (`SETF` was in `DYNAMIC_FUNCTIONS`, the calls that read what their text does not name,
/// which are worked out after every edit that changed anything: a counter moved by sixteen on each edit, and an idempotent call asked for a write each time). Nothing in
/// says a call is worked out on any other edit.
#[test]
fn an_edit_that_changes_nothing_a_setf_reads_leaves_it_alone() {
    let mut doc = setf_shape();
    let first = works(&mut doc);
    assert_eq!(value(&doc, 1, &scratch("A", 1)).as_deref(), Some("6"), "{first:?}");
    assert_eq!(value(&doc, 1, &user("N")).as_deref(), Some("16"), "the counter ran to the loop budget: {first:?}");
    // An edit of a cell neither call reads: neither is worked out, so nothing is queued, written, counted or dropped.
    let report = edited(&mut doc, 1, &user("Other"), "7");
    assert_eq!(
        (report.setf_written, report.setf_same, report.setf_rounds, report.setf_dropped),
        (0, 0, 0, 0),
        "no call was reached by an edit that none of them reads: {report:?}"
    );
    assert_eq!(value(&doc, 1, &user("N")).as_deref(), Some("16"), "the counter did not move: {report:?}");
    assert_eq!((formula(&doc, 1, &scratch("A", 1)).as_deref(), value(&doc, 1, &scratch("A", 1)).as_deref()), (Some("6"), Some("6")));
    // And another edit of the same cell: still no call is reached.
    let report = edited(&mut doc, 1, &user("Other"), "8");
    assert_eq!((report.setf_written, report.setf_rounds), (0, 0), "{report:?}");
    assert_eq!(value(&doc, 1, &user("N")).as_deref(), Some("16"));
}

/// The other half: the edit of an input of a call works it out. `Step` is read by the counter only, and `Width` by the idempotent call only, so each edit runs one call and
/// leaves the other as it was.
#[test]
fn an_edit_that_changes_an_input_of_a_setf_works_it_out() {
    let mut doc = setf_shape();
    works(&mut doc);
    assert_eq!(value(&doc, 1, &user("N")).as_deref(), Some("16"));
    // The counter's input: it adds 2 sixteen times more. The idempotent call reads neither `Step` nor `N`, so it is not asked.
    let report = edited(&mut doc, 1, &user("Step"), "2");
    assert_eq!(value(&doc, 1, &user("N")).as_deref(), Some("48"), "16 + 16 rounds of 2: {report:?}");
    assert_eq!((report.setf_written, report.setf_same), (16, 0), "{report:?}");
    assert!(report.setf_dropped >= 1 && report.setf_rounds == MAX_SETF_ROUNDS as u64, "it ended at the loop budget: {report:?}");
    assert_eq!(value(&doc, 1, &scratch("A", 1)).as_deref(), Some("6"), "the other call was not asked");
    // The idempotent call's input: it writes once, and the counter does not move.
    let report = edited(&mut doc, 1, &CellAddr::direct("Width"), "5");
    assert_eq!((formula(&doc, 1, &scratch("A", 1)).as_deref(), value(&doc, 1, &scratch("A", 1)).as_deref()), (Some("10"), Some("10")), "{report:?}");
    assert_eq!(report.setf_written, 1, "{report:?}");
    assert_eq!(value(&doc, 1, &user("N")).as_deref(), Some("48"), "the counter did not move: {report:?}");
}

/// The page's commit and the document's work out every cell, so a `SETF` is worked out by them whatever it reads (a commit that answers no edit): the change is in what an edit
/// reaches, and not in what a pass over the cells does.
#[test]
fn a_commit_of_a_whole_page_works_every_setf_out() {
    let mut doc = setf_shape();
    works(&mut doc);
    let mut page = doc.clone();
    let report = commit_in(&mut page, &Budget::default(), EvalOptions::default(), Scope::Page(1));
    assert_eq!(value(&page, 1, &user("N")).as_deref(), Some("32"), "the counter was worked out again, sixteen more: {report:?}");
    assert!(report.setf_same >= 1, "the idempotent call was worked out, and found its target holding its text: {report:?}");
}

// ---- the loop budget and the work budget ---------------------------------------------------------------------------------------------------------

/// A call that sets itself off: a counter that writes `N+1` into `N`, which it reads, so each write works it out again. It ends at the loop budget, with the
/// calls past it dropped and counted, and the budget of work not spent.
#[test]
fn a_counter_ends_at_the_loop_budget() {
    let shape = with_formula(ShapeBuilder::new(1).row_cell("User", None, RowKey::name("N"), "Value", "0"), "Tick", "SETF(GETREF(User.N),User.N+1)");
    let mut doc = one(shape);
    let started = Instant::now();
    let report = works(&mut doc);
    assert!(started.elapsed() < BOUND, "{report:?}");
    assert_eq!(report.setf_rounds, MAX_SETF_ROUNDS as u64, "{report:?}");
    assert_eq!(report.setf_written, MAX_SETF_ROUNDS as u64, "one write a round: {report:?}");
    assert!(report.setf_dropped >= 1, "the call the last round set off was dropped and counted: {report:?}");
    assert_eq!(value(&doc, 1, &user("N")).as_deref(), Some(MAX_SETF_ROUNDS.to_string().as_str()), "{report:?}");
    assert!(!report.budget_exhausted && !report.memory_exhausted, "{report:?}");
}

/// Two cells that write each other's inputs forever: they end at the loop budget too.
#[test]
fn two_cells_that_set_each_other_off_end_at_the_loop_budget() {
    let shape =
        ShapeBuilder::new(1).row_cell("User", None, RowKey::name("Av"), "Value", "0").row_cell("User", None, RowKey::name("Bv"), "Value", "0");
    let shape = with_formula(shape, "A", "SETF(GETREF(User.Bv),User.Av+1)");
    let shape = with_formula(shape, "B", "SETF(GETREF(User.Av),User.Bv+1)");
    let mut doc = one(shape);
    let started = Instant::now();
    let report = works(&mut doc);
    assert!(started.elapsed() < BOUND, "{report:?}");
    assert_eq!(report.setf_rounds, MAX_SETF_ROUNDS as u64, "{report:?}");
    assert_eq!(report.setf_written, 2 * MAX_SETF_ROUNDS as u64, "two writes a round: {report:?}");
    assert!(report.setf_dropped >= 2, "{report:?}");
    assert!(!report.budget_exhausted, "{report:?}");
}

/// Thousands of calls in one working-out: the queue and the writes are capped, and what is past the writes is dropped and counted, whatever the number of cells.
#[test]
fn ten_thousand_calls_make_at_most_the_writes_of_the_loop_budget() {
    let mut shape = ShapeBuilder::new(1);
    for k in 0..10_000 {
        shape = shape.row_cell("User", None, RowKey::name(&format!("T{k}")), "Value", "0");
        shape = with_formula(shape, &format!("S{k}"), &format!("SETF(GETREF(User.T{k}),{})", k + 1));
    }
    let mut doc = one(shape);
    let started = Instant::now();
    let report = works(&mut doc);
    assert!(started.elapsed() < BOUND, "{report:?}");
    assert_eq!(report.setf_written, MAX_SETF_WRITES as u64, "{report:?}");
    assert_eq!(report.setf_dropped, 10_000 - MAX_SETF_WRITES as u64, "every call that was not written is counted once: {report:?}");
    assert!(!report.budget_exhausted && !report.memory_exhausted, "{report:?}");
    // The first calls reached are the ones written, in the order they were reached.
    assert_eq!(value(&doc, 1, &user("T0")).as_deref(), Some("1"));
    assert_eq!(formula(&doc, 1, &user(&format!("T{}", MAX_SETF_WRITES))), None);
}

/// A chain of ten thousand cells that all read the one a `SETF` writes: the write works the whole chain out again, once, within the work budget.
#[test]
fn a_chain_of_ten_thousand_cells_is_worked_out_again_once() {
    const N: u32 = 10_000;
    let mut shape = ShapeBuilder::new(1).row_cell("User", None, RowKey::name("R0"), "Value", "1");
    for k in 1..=N {
        shape = shape.row_formula("User", None, RowKey::name(&format!("R{k}")), "Value", &format!("User.R{}+1", k - 1), "0");
    }
    let mut doc = one(with_formula(shape, "Go", "SETF(GETREF(User.R0),2)"));
    let started = Instant::now();
    let report = works(&mut doc);
    assert!(started.elapsed() < BOUND, "{report:?}");
    assert_eq!(report.setf_written, 1, "{report:?}");
    assert_eq!(
        value(&doc, 1, &user(&format!("R{N}"))).as_deref(),
        Some((N + 2).to_string().as_str()),
        "the end of the chain read the write: {report:?}"
    );
    assert!(!report.budget_exhausted && !report.memory_exhausted, "{report:?}");
}

/// The work budget is the commit's, shared by its rounds: given the steps of the first pass and a half more, the round that works the chain out again runs out,
/// and the commit ends as `budget_exhausted` and no more than its budget was taken.
#[test]
fn the_rounds_share_the_work_budget_of_the_commit() {
    const N: u32 = 2_000;
    let build = || {
        let mut shape = ShapeBuilder::new(1).row_cell("User", None, RowKey::name("R0"), "Value", "1");
        for k in 1..=N {
            shape = shape.row_formula("User", None, RowKey::name(&format!("R{k}")), "Value", &format!("User.R{}+1", k - 1), "0");
        }
        one(with_formula(shape, "Go", "SETF(GETREF(User.R0),2)"))
    };
    let whole = works(&mut build());
    assert!(whole.setf_rounds >= 1 && !whole.budget_exhausted, "{whole:?}");
    // The first pass alone (the setting that never writes) costs this much.
    let first = commit(&mut build(), &Budget::default(), inert());
    let budget = first.stats.steps + first.stats.steps / 2;
    let mut doc = build();
    let report = commit(&mut doc, &Budget::with_steps(budget), EvalOptions::default());
    assert!(report.budget_exhausted, "{report:?}");
    assert!(report.stats.steps <= budget, "{} steps of {budget}: {report:?}", report.stats.steps);
    assert_eq!(report.setf_written, 1, "the first pass completed and its call was written: {report:?}");
}

/// A commit whose budget ran out in its first pass makes no write: a run cut short is no result to act on. The calls are dropped and counted.
#[test]
fn a_commit_that_ran_out_of_budget_writes_nothing() {
    let shape = with_formula(ShapeBuilder::new(1).row_cell("Scratch", None, RowKey::Ix(0), "B", "1"), "Auto", "SETF(GETREF(Scratch.B1),4.25)");
    let mut doc = one(shape);
    let report = commit(&mut doc, &Budget::with_steps(3), EvalOptions::default());
    assert!(report.budget_exhausted, "{report:?}");
    assert_eq!((report.setf_written, formula(&doc, 1, &scratch("B", 1))), (0, None), "{report:?}");
}
