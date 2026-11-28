//! A cell that reads a `SETF` cell's value does not make the `SETF` write (`docs/editing-commands.md`).
//!
//! A `SETF` asks for its write when **its own cell is worked out as itself**: because an input it reads changed (it is dirty), because a pass works every cell out, or because
//! a dynamic call (`EVALCELL`) asks for it. A formula that only reads the cell's value (the call is 0) takes that value and queues nothing. Before this task the evaluator
//! worked the `SETF` cell out in place, as an operand, and queued its write, so a cell that was worked out for some other input (`Reader = Go + Width`, after `Width` changed)
//! ran a call that did not need running, and the call wrote its text over a person's value at its target.
//!
//! The tests go through the commit of an edit (`changes_between`, then `commit_changed`), which is the path of every editing command's recalculation, and through the passes
//! that work every cell out. The report's counts say how often a call was queued: of the calls that name one cell the last wins and the ones before it are counted in
//! `setf_same`, so a call queued twice in one pass shows as one more of them.

use std::sync::Arc;

use diacraft_calc::rules::{MAX_SETF_QUEUED, MAX_SETF_ROUNDS, MAX_SETF_WRITES};
use diacraft_calc::{Budget, CommitReport, EvalOptions, Scope, SetfMode, changes_between, commit, commit_changed, commit_in};
use diacraft_doc::{Cell, CellAddr, CellEdit, Document, FormulaSlot, Lookup, Owner, RowKey, ShapeRef, SheetRef, WriteOrigin};
use diacraft_formula::parse;
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};

fn user(name: &str) -> CellAddr {
    CellAddr::in_row("User", None, RowKey::name(name), "Value")
}

/// `Scratch.A1`.
fn a1() -> CellAddr {
    CellAddr::in_row("Scratch", None, RowKey::Ix(0), "A")
}

fn direct(name: &str) -> CellAddr {
    CellAddr::direct(name)
}

fn one(shape: ShapeBuilder) -> Document {
    DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build()
}

/// A user cell with a formula and the saved value 0, as a file holds one.
fn with_formula(shape: ShapeBuilder, name: &str, formula: &str) -> ShapeBuilder {
    shape.row_formula("User", None, RowKey::name(name), "Value", formula, "0")
}

fn found(doc: &Document, addr: &CellAddr) -> Cell {
    let at = ShapeRef::top(Owner::Page(1), 0);
    match doc.shape(&at).expect("the shape").sheet.lookup(addr) {
        Lookup::Found(c) => c.clone(),
        other => panic!("{addr:?}: {other:?}"),
    }
}

fn formula(doc: &Document, addr: &CellAddr) -> Option<String> {
    match found(doc, addr).formula {
        FormulaSlot::Local(f) => Some(f.source.to_string()),
        _ => None,
    }
}

fn value(doc: &Document, addr: &CellAddr) -> String {
    found(doc, addr).value.map(|v| v.text().to_owned()).unwrap_or_default()
}

fn number(doc: &Document, addr: &CellAddr) -> f64 {
    value(doc, addr).parse().expect("a number")
}

/// Every formula cell worked out, as a file that was just opened and recalculated.
fn works(doc: &mut Document) -> CommitReport {
    commit(doc, &Budget::default(), EvalOptions::default())
}

/// An edit made as `sheet.set_cell` makes one: the cell is given the formula `text`, and the commit answers what the comparison with the document before finds changed.
fn edited(doc: &mut Document, addr: &CellAddr, text: &str) -> CommitReport {
    let before = doc.clone();
    let sheet = SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0));
    doc.write_cell(&sheet, addr, CellEdit::Formula { formula: Arc::new(parse(text)), value: None }, WriteOrigin::Automation).expect("a write");
    let changes = changes_between(&before, doc, 1);
    commit_changed(doc, &Budget::default(), EvalOptions::default(), 1, &changes)
}

/// The counts that say how often the calls were queued, written and found at rest, and how many rounds the commit made.
fn counts(report: &CommitReport) -> (u64, u64, u64) {
    (report.setf_written, report.setf_same, report.setf_rounds)
}

/// The cells of a shape in the two orders a sheet can hold a reader and the call it reads: the reader after the call, and before it (a reader that is worked out first
/// finds the call not yet worked out, and works it out in place).
fn reader_and_call(reader_first: bool, base: ShapeBuilder, call: &str, reader: &str) -> ShapeBuilder {
    if reader_first {
        with_formula(with_formula(base, "Reader", reader), "Go", call)
    } else {
        with_formula(with_formula(base, "Go", call), "Reader", reader)
    }
}

// ---- the bug --------------------------------------------------------------------------------------------------------------------------------

/// `Go` writes 6 into `Scratch.A1`, and `Reader` reads `Go`'s value. A person writes 99 into `A1`: the call names the cell and does not read it, so `Go` is not worked out.
/// Then `Width` changes, which `Reader` reads: `Reader` is worked out, and takes the value of `Go` (0). That is no reason for `Go` to write, and `A1` stays 99 (it was
/// written over with 6, `setf_written: 1`, when the evaluator worked `Go` out in place and queued its write).
#[test]
fn a_cell_that_reads_a_setf_cell_does_not_make_it_write() {
    for reader_first in [false, true] {
        let base = ShapeBuilder::new(1).cell("Width", "3").row_cell("Scratch", None, RowKey::Ix(0), "A", "0");
        let mut doc = one(reader_and_call(reader_first, base, "SETF(GETREF(Scratch.A1),6)", "User.Go+Width"));
        let first = works(&mut doc);
        assert_eq!((formula(&doc, &a1()).as_deref(), value(&doc, &a1()).as_str()), (Some("6"), "6"), "the pass wrote: {first:?}");
        assert_eq!(number(&doc, &user("Reader")), 3.0, "the reader took the call's 0 and the width: {first:?}");
        let report = edited(&mut doc, &a1(), "99");
        assert_eq!(counts(&report), (0, 0, 0), "a write to the target reaches nothing: {report:?}");
        assert_eq!(value(&doc, &a1()), "99");
        // The width: only the reader is worked out, and it reads the call as a value.
        let report = edited(&mut doc, &direct("Width"), "5");
        assert_eq!(number(&doc, &user("Reader")), 5.0, "the reader was worked out (reader first: {reader_first}): {report:?}");
        assert_eq!(
            counts(&report),
            (0, 0, 0),
            "the call was not worked out as itself, so nothing was queued (reader first: {reader_first}): {report:?}"
        );
        assert_eq!(
            (formula(&doc, &a1()).as_deref(), value(&doc, &a1()).as_str()),
            (Some("99"), "99"),
            "the person's value stands (reader first: {reader_first}): {report:?}"
        );
    }
}

/// The reader may sit at any depth: a cell that reads a cell that reads the call does not make it write either.
#[test]
fn a_chain_of_readers_does_not_make_the_call_write() {
    let base = ShapeBuilder::new(1).cell("Width", "3").row_cell("Scratch", None, RowKey::Ix(0), "A", "0");
    let shape = with_formula(with_formula(with_formula(base, "Go", "SETF(GETREF(Scratch.A1),6)"), "Mid", "User.Go+1"), "Top", "User.Mid+Width");
    let mut doc = one(shape);
    works(&mut doc);
    edited(&mut doc, &a1(), "99");
    let report = edited(&mut doc, &direct("Width"), "5");
    assert_eq!(number(&doc, &user("Top")), 6.0, "{report:?}");
    assert_eq!(counts(&report), (0, 0, 0), "{report:?}");
    assert_eq!(value(&doc, &a1()), "99", "{report:?}");
}

/// The page's own pass of a document works every cell out as itself, the call included, so the call writes in it whoever reads it (and a reader earlier in the sheet
/// does not make it write twice, or not at all).
#[test]
fn a_reader_in_a_pass_over_every_cell_leaves_the_call_to_write_once() {
    for reader_first in [false, true] {
        let base = ShapeBuilder::new(1).cell("Width", "3").row_cell("Scratch", None, RowKey::Ix(0), "A", "0");
        let mut doc = one(reader_and_call(reader_first, base, "SETF(GETREF(Scratch.A1),6)", "User.Go+Width"));
        let report = works(&mut doc);
        assert_eq!((formula(&doc, &a1()).as_deref(), value(&doc, &a1()).as_str()), (Some("6"), "6"), "reader first: {reader_first}: {report:?}");
        // One write in the first round; the second round works the pass out again, finds the target holding its text, and writes nothing. A call queued twice in a
        // round would count one more as at rest.
        assert_eq!(counts(&report), (1, 1, 2), "reader first: {reader_first}: {report:?}");
        // The same on the pass over the one page.
        let mut page = one(reader_and_call(
            reader_first,
            ShapeBuilder::new(1).cell("Width", "3").row_cell("Scratch", None, RowKey::Ix(0), "A", "0"),
            "SETF(GETREF(Scratch.A1),6)",
            "User.Go+Width",
        ));
        let report = commit_in(&mut page, &Budget::default(), EvalOptions::default(), Scope::Page(1));
        assert_eq!(counts(&report), (1, 1, 2), "the page's pass, reader first: {reader_first}: {report:?}");
        assert_eq!(value(&page, &a1()), "6");
    }
}

// ---- a dirty call that a reader reads --------------------------------------------------------------------------------------------------------

/// `Go` reads `Width` and so does `Reader`: one edit makes both dirty, whichever is worked out first, and the call is queued once and writes once. (A reader worked out
/// first reads the call as a value; the call is then worked out as itself, as any dirty cell is.)
#[test]
fn a_dirty_setf_read_by_a_reader_in_the_same_commit_runs_once() {
    for reader_first in [false, true] {
        let base = ShapeBuilder::new(1).cell("Width", "3").row_cell("Scratch", None, RowKey::Ix(0), "A", "0");
        let mut doc = one(reader_and_call(reader_first, base, "SETF(GETREF(Scratch.A1),Width*2)", "User.Go+Width"));
        works(&mut doc);
        assert_eq!(value(&doc, &a1()), "6");
        edited(&mut doc, &a1(), "99");
        let report = edited(&mut doc, &direct("Width"), "5");
        assert_eq!(number(&doc, &user("Reader")), 5.0, "{report:?}");
        assert_eq!(
            (formula(&doc, &a1()).as_deref(), value(&doc, &a1()).as_str()),
            (Some("10"), "10"),
            "the call is dirty and wrote (reader first: {reader_first}): {report:?}"
        );
        assert_eq!(counts(&report), (1, 0, 1), "queued once and written once (reader first: {reader_first}): {report:?}");
    }
}

/// A counter that a reader reads: each round it is worked out as itself once (a reader reading it does not add a second call to the round), so it counts to the loop
/// budget with no call found at rest, and the reader holds the call's 0.
#[test]
fn a_counter_that_a_reader_reads_moves_once_a_round() {
    for reader_first in [false, true] {
        let base =
            ShapeBuilder::new(1).row_cell("User", None, RowKey::name("N"), "Value", "0").row_cell("User", None, RowKey::name("Step"), "Value", "1");
        let mut doc = one(reader_and_call(reader_first, base, "SETF(GETREF(User.N),User.N+User.Step)", "User.Go+User.Step"));
        let report = works(&mut doc);
        assert_eq!(number(&doc, &user("N")), MAX_SETF_ROUNDS as f64, "reader first: {reader_first}: {report:?}");
        assert_eq!(report.setf_same, 0, "no round queued the call twice (reader first: {reader_first}): {report:?}");
        assert_eq!(report.setf_written, MAX_SETF_ROUNDS as u64, "{report:?}");
        // The step changes: the counter and the reader are both dirty, and the counter runs to the loop budget again, once a round.
        let report = edited(&mut doc, &user("Step"), "2");
        assert_eq!(number(&doc, &user("N")), MAX_SETF_ROUNDS as f64 * 3.0, "16 + 16 rounds of 2 (reader first: {reader_first}): {report:?}");
        assert_eq!(report.setf_same, 0, "no round queued the call twice (reader first: {reader_first}): {report:?}");
        assert_eq!(report.setf_written, MAX_SETF_ROUNDS as u64, "{report:?}");
        assert_eq!(number(&doc, &user("Reader")), 2.0, "the call's 0 and the step: {report:?}");
    }
}

// ---- a dynamic call asks for it ----------------------------------------------------------------------------------------------------------------

/// `EVALCELL` works a cell's formula out where it is called, and is worked out after every edit that changes anything (it is a dynamic call): the `SETF` it works out
/// writes, as the pages say it does. The cell `Go` itself reads nothing that changed and is not worked out as itself, so the call is queued by the `EVALCELL` and by nothing
/// else (it was queued twice: once when `Go` was worked out in place as an operand of the cell that names it, and once by the `EVALCELL`).
#[test]
fn evalcell_of_a_setf_cell_still_runs_it() {
    for caller_first in [false, true] {
        let base =
            ShapeBuilder::new(1).row_cell("Scratch", None, RowKey::Ix(0), "A", "0").row_cell("User", None, RowKey::name("Other"), "Value", "0");
        let shape = if caller_first {
            with_formula(with_formula(base, "Caller", "EVALCELL(User.Go)"), "Go", "SETF(GETREF(Scratch.A1),6)")
        } else {
            with_formula(with_formula(base, "Go", "SETF(GETREF(Scratch.A1),6)"), "Caller", "EVALCELL(User.Go)")
        };
        // The drawing as it was built, no pass made: the target holds 0.
        let mut doc = one(shape);
        assert_eq!(value(&doc, &a1()), "0");
        // An edit of a cell nothing reads: the dynamic call is worked out, and so it runs the `SETF` it names.
        let report = edited(&mut doc, &user("Other"), "7");
        assert_eq!(
            (formula(&doc, &a1()).as_deref(), value(&doc, &a1()).as_str()),
            (Some("6"), "6"),
            "the call that EVALCELL works out wrote (caller first: {caller_first}): {report:?}"
        );
        assert_eq!(
            counts(&report),
            (1, 1, 2),
            "queued once a round, by the EVALCELL (the second round works the dynamic call out again and finds the target holding its text) (caller first: {caller_first}): {report:?}"
        );
        // It is dynamic: a person's value at the target is written over by the next edit, which is what the call says.
        edited(&mut doc, &a1(), "99");
        assert_eq!(value(&doc, &a1()), "6", "the dynamic call ran again after the person's edit (caller first: {caller_first})");
    }
}

/// A reader that reads the cell that holds the `EVALCELL`, and is dirty itself: it may be worked out before that cell is worked out as itself, and the call is still
/// queued once, by the cell that holds the `EVALCELL`.
#[test]
fn a_reader_of_an_evalcell_cell_leaves_its_call_to_run_once() {
    for reader_first in [false, true] {
        let base =
            ShapeBuilder::new(1).row_cell("Scratch", None, RowKey::Ix(0), "A", "0").row_cell("User", None, RowKey::name("Other"), "Value", "0");
        let shape = if reader_first {
            with_formula(
                with_formula(with_formula(base, "Top", "User.Caller+User.Other"), "Caller", "EVALCELL(User.Go)"),
                "Go",
                "SETF(GETREF(Scratch.A1),6)",
            )
        } else {
            with_formula(
                with_formula(with_formula(base, "Go", "SETF(GETREF(Scratch.A1),6)"), "Caller", "EVALCELL(User.Go)"),
                "Top",
                "User.Caller+User.Other",
            )
        };
        let mut doc = one(shape);
        let report = edited(&mut doc, &user("Other"), "7");
        assert_eq!(number(&doc, &user("Top")), 7.0, "{report:?}");
        assert_eq!(value(&doc, &a1()), "6", "reader first: {reader_first}: {report:?}");
        assert_eq!(counts(&report), (1, 1, 2), "queued once a round, by the cell that holds the EVALCELL, reader first: {reader_first}: {report:?}");
    }
}

// ---- every call in a pass, the caps, the setting ------------------------------------------------------------------------------------------------

/// A pass over every cell works every `SETF` out as itself, once, however the cells that read them are placed: three calls that name three cells all write.
#[test]
fn a_pass_over_every_cell_runs_every_setf_once() {
    let mut shape = ShapeBuilder::new(1);
    for k in 0..3 {
        shape = shape.row_cell("User", None, RowKey::name(&format!("T{k}")), "Value", "0");
    }
    // Readers first: each reads the call after it, and one reads two.
    shape = with_formula(shape, "R0", "User.S0+1");
    shape = with_formula(shape, "R1", "User.S1+User.S2");
    for k in 0..3 {
        shape = with_formula(shape, &format!("S{k}"), &format!("SETF(GETREF(User.T{k}),{})", k + 1));
    }
    let mut doc = one(shape);
    let report = works(&mut doc);
    for k in 0..3 {
        assert_eq!(value(&doc, &user(&format!("T{k}"))), (k + 1).to_string(), "{report:?}");
    }
    assert_eq!(counts(&report), (3, 3, 2), "{report:?}");
}

/// The caps hold where readers read the calls first: the calls past the writes and past the queue are dropped and counted once each, as they are with no reader.
#[test]
fn the_caps_and_the_dropped_count_hold_with_readers() {
    const N: usize = 6_000;
    let mut shape = ShapeBuilder::new(1);
    for k in 0..N {
        shape = shape.row_cell("User", None, RowKey::name(&format!("T{k}")), "Value", "0");
        shape = with_formula(shape, &format!("R{k}"), &format!("User.S{k}+1"));
    }
    for k in 0..N {
        shape = with_formula(shape, &format!("S{k}"), &format!("SETF(GETREF(User.T{k}),{})", k + 1));
    }
    let mut doc = one(shape);
    let report = works(&mut doc);
    assert_eq!(report.setf_written, MAX_SETF_WRITES as u64, "{report:?}");
    assert_eq!(report.setf_dropped, (N - MAX_SETF_WRITES) as u64, "every call that was not written is counted once: {report:?}");
    assert!(!report.budget_exhausted && !report.memory_exhausted, "{report:?}");
    assert_eq!(value(&doc, &user("T0")), "1");
}

/// A commit whose budget ran out makes no write, with readers of the calls as without: a run cut short is no result to act on.
#[test]
fn a_commit_that_ran_out_of_budget_writes_nothing_with_readers() {
    let base = ShapeBuilder::new(1).row_cell("Scratch", None, RowKey::Ix(0), "B", "1");
    let mut doc = one(reader_and_call(true, base, "SETF(GETREF(Scratch.B1),4.25)", "User.Go+1"));
    let report = commit(&mut doc, &Budget::with_steps(3), EvalOptions::default());
    assert!(report.budget_exhausted, "{report:?}");
    assert_eq!(report.setf_written, 0, "{report:?}");
    assert_eq!(formula(&doc, &CellAddr::in_row("Scratch", None, RowKey::Ix(0), "B")), None);
}

/// The old reading as a setting still queues nothing, read or not.
#[test]
fn the_inert_setting_queues_nothing_for_a_reader_either() {
    let base = ShapeBuilder::new(1).row_cell("Scratch", None, RowKey::Ix(0), "A", "0");
    let mut doc = one(reader_and_call(true, base, "SETF(GETREF(Scratch.A1),6)", "User.Go+1"));
    let report = commit(&mut doc, &Budget::default(), EvalOptions { setf: SetfMode::Inert, ..EvalOptions::default() });
    assert_eq!((counts(&report), report.setf_dropped), ((0, 0, 0), 0), "{report:?}");
    assert_eq!(formula(&doc, &a1()), None);
}

/// A call's value, an error included, is the same read as an operand and worked out as itself: `Go` names no cell, so it is `#VALUE!` either way, and a reader of it takes
/// what a reader of any failing cell takes.
#[test]
fn a_call_gives_the_same_value_read_as_an_operand_or_worked_out_as_itself() {
    let base = ShapeBuilder::new(1).row_cell("Scratch", None, RowKey::Ix(0), "C", "5");
    let mut doc = one(reader_and_call(true, base, "SETF(Scratch.C1,2)", "User.Go+1"));
    let report = works(&mut doc);
    let error = |addr: &CellAddr| found(&doc, addr).error.map(|e| e.to_string());
    assert_eq!(error(&user("Go")).as_deref(), Some("#VALUE!"), "{report:?}");
    assert_eq!(number(&doc, &user("Reader")), 1.0, "a failing cell reads at its last good value, 0, as an operand: {report:?}");
    assert_eq!(counts(&report), (0, 0, 0), "{report:?}");
}

/// One cell can reach thousands of calls through nested `EVALCELL`s (`A` calls `B` a hundred times, `B` calls `C` a hundred times, and `C` holds the `SETF`: 10,000 calls from
/// one formula). The calls one cell keeps are capped as the queue is ([`MAX_SETF_QUEUED`]), and the rest are dropped and counted when the cell asks: of the 10,101 calls the
/// pass asks for (`C`'s one, `B`'s hundred and `A`'s 10,000), 4,096 are queued and 6,005 are dropped, in each of the two rounds the write makes.
#[test]
fn one_cell_that_reaches_thousands_of_calls_keeps_no_more_than_the_queue_holds() {
    let terms = |name: &str| vec![format!("EVALCELL(User.{name})"); 100].join("+");
    let shape = ShapeBuilder::new(1).row_cell("User", None, RowKey::name("T"), "Value", "0");
    let shape = with_formula(shape, "C", "SETF(GETREF(User.T),1)");
    let shape = with_formula(with_formula(shape, "B", &terms("C")), "A", &terms("B"));
    let mut doc = one(shape);
    let report = works(&mut doc);
    assert_eq!((formula(&doc, &user("T")).as_deref(), value(&doc, &user("T")).as_str()), (Some("1"), "1"), "{report:?}");
    assert_eq!(report.setf_written, 1, "every call names the one cell and the last wins: {report:?}");
    assert_eq!(report.setf_same, (MAX_SETF_QUEUED as u64 - 1) + MAX_SETF_QUEUED as u64, "{report:?}");
    assert_eq!(report.setf_dropped, 2 * (10_101 - MAX_SETF_QUEUED as u64), "every call past the queue is counted once in each round: {report:?}");
    assert!(!report.budget_exhausted && !report.memory_exhausted, "{report:?}");
}
