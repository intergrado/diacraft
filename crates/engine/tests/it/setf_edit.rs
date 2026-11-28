//! A `SETF` in a cell that an edit works out again writes its target, in the edit's own undo step (`docs/editing-commands.md`).
//!
//! The calculator makes the writes (`crates/calc/tests/it/setf_effects.rs` holds the vectors and the caps); what these tests hold is the engine's part: the recalculation
//! of an editing command is the commit that makes them, the answer of the dry run writes nothing, and undo takes the write back with the edit.

use diacraft_doc::{Document, RowKey};
use diacraft_engine::Session;
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};
use serde_json::{Value, json};

use crate::{blank_session, check_session, run};

fn one_shape(shape: ShapeBuilder) -> Session {
    let doc: Document = DocBuilder::new().page(1, PageBuilder::new().name("Page-1").shape(shape)).build();
    let mut s = blank_session();
    s.edit("build", |d| {
        *d = doc;
        Ok(())
    })
    .unwrap();
    s
}

/// Shape 10: a width of 2 in, a scratch cell `A1`, and a user cell that writes twice the width into it.
fn shape() -> ShapeBuilder {
    ShapeBuilder::new(10).cell_u("Width", "2", "IN").row_cell("Scratch", None, RowKey::Ix(0), "A", "0").row_formula(
        "User",
        None,
        RowKey::name("Go"),
        "Value",
        "SETF(GETREF(Scratch.A1),Width*2)",
        "0",
    )
}

fn cell(s: &mut Session, name: &str) -> Value {
    run(s, "sheet.cell", json!({"id": 10, "cell": name}))
}

fn set(s: &mut Session, name: &str, formula: &str) -> Value {
    run(s, "sheet.set_cell", json!({"id": 10, "cell": name, "formula": formula}))
}

fn undo_len(s: &Session) -> usize {
    s.history().map_or(0, |h| h.undo_len())
}

#[test]
fn an_edit_that_a_setf_cell_reads_makes_it_write_in_the_same_undo_step() {
    let mut s = one_shape(shape());
    let steps = undo_len(&s);
    set(&mut s, "Width", "3");
    let target = cell(&mut s, "Scratch.A1");
    assert_eq!(target["value"], json!(6.0), "twice the new width: {target}");
    assert!(target["formula"].as_str().is_some_and(|f| f.starts_with('6')), "the formula is the cell's own now: {target}");
    assert_eq!(cell(&mut s, "User.Go")["value"], json!(0.0), "the call is 0");
    assert_eq!(undo_len(&s), steps + 1, "the write is not a step of its own");
    // The same edit again leaves the target as it is: it holds the text.
    set(&mut s, "Width", "3 in");
    assert_eq!(cell(&mut s, "Scratch.A1")["value"], json!(6.0));
    // Undo takes back the edits and what the call wrote with them: the cell is as it was.
    while undo_len(&s) > steps {
        run(&mut s, "edit.undo", json!({}));
    }
    let back = cell(&mut s, "Scratch.A1");
    assert_eq!(back["value"], json!(0.0), "{back}");
    assert!(back["formula"].as_str().is_none_or(|f| !f.starts_with('6')), "the cell has no formula of the call's again: {back}");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn the_dry_run_of_a_typed_formula_writes_nothing() {
    let mut s = one_shape(shape());
    let steps = undo_len(&s);
    let answer = run(&mut s, "sheet.evaluate", json!({"id": 10, "cell": "User.Go", "formula": "SETF(GETREF(Scratch.A1),9)"}));
    assert_eq!(answer["value"], json!(0.0), "{answer}");
    assert_eq!(cell(&mut s, "Scratch.A1")["value"], json!(0.0));
    assert_eq!(undo_len(&s), steps, "no step");
}

/// The notes of an answer, as strings.
fn notes(answer: &Value) -> Vec<String> {
    answer["notes"].as_array().map(|list| list.iter().filter_map(|n| n.as_str().map(str::to_owned)).collect()).unwrap_or_default()
}

/// A counter, as the cap tests have it: `Tick` adds `Height` to `N` each time it is worked out, and `Tick` reads `N`, so each write works it out again. `Other` is read by nobody.
fn counter() -> ShapeBuilder {
    let shape = ShapeBuilder::new(10).cell("Height", "1").row_cell("User", None, RowKey::name("Other"), "Value", "0");
    shape.row_cell("User", None, RowKey::name("N"), "Value", "0").row_formula(
        "User",
        None,
        RowKey::name("Tick"),
        "Value",
        "SETF(GETREF(User.N),User.N+Height)",
        "0",
    )
}

/// The effects past the cap "are dropped with a note": an edit that sets a counter off says, in its answer, how many calls were dropped, and in words.
#[test]
fn an_edit_that_drops_setf_calls_says_so_in_its_answer() {
    let mut s = one_shape(counter());
    let answer = set(&mut s, "Height", "2");
    assert_eq!(cell(&mut s, "User.N")["value"], json!(32.0), "16 rounds of 2");
    let dropped = answer["setf_dropped"].as_u64().unwrap_or(0);
    assert!(dropped >= 1, "the calls past the loop budget are counted in the answer: {answer}");
    let said = notes(&answer);
    assert_eq!(said.len(), 1, "one note: {answer}");
    let words = said.first().cloned().unwrap_or_default();
    assert!(words.contains("SETF") && words.contains("dropped") && words.contains("64") && words.contains("16"), "{words}");
    assert!(words.starts_with(&format!("{dropped} SETF call")), "the note counts the calls the answer counts: {words}");
    assert_eq!(check_session(&s), Ok(()));
}

/// More calls than one edit may write: 100 calls that each write one cell are worked out by one edit, 64 are written and the other 36 are dropped, counted once each.
#[test]
fn an_edit_with_more_calls_than_the_writes_it_may_make_counts_the_rest() {
    let mut shape = ShapeBuilder::new(10).cell("Height", "1");
    for k in 0..100 {
        shape = shape.row_cell("User", None, RowKey::name(&format!("T{k}")), "Value", "0").row_formula(
            "User",
            None,
            RowKey::name(&format!("S{k}")),
            "Value",
            &format!("SETF(GETREF(User.T{k}),Height*{})", k + 1),
            "0",
        );
    }
    let mut s = one_shape(shape);
    let answer = set(&mut s, "Height", "2");
    assert_eq!(answer["setf_dropped"], json!(36), "{answer}");
    assert!(notes(&answer).iter().any(|n| n.starts_with("36 SETF calls were dropped")), "{answer}");
    let written = (0..100).filter(|k| cell(&mut s, &format!("User.T{k}"))["value"] != json!(0.0)).count();
    assert_eq!(written, 64, "the writes the edit may make");
}

/// An edit that drops nothing says nothing: no count and no note, and what a command says of its own is kept beside the note when there is one.
#[test]
fn an_edit_that_drops_nothing_says_nothing_of_it() {
    let mut s = one_shape(shape());
    let answer = set(&mut s, "Width", "3");
    assert_eq!(cell(&mut s, "Scratch.A1")["value"], json!(6.0), "the call wrote");
    assert!(answer.get("setf_dropped").is_none() && notes(&answer).iter().all(|n| !n.contains("SETF")), "{answer}");
}

/// The note is the edit's: the command after it, which sets nothing off, is not given it, and the counter stays where the first edit left it.
#[test]
fn the_note_is_said_once_by_the_edit_that_dropped_the_calls() {
    let mut s = one_shape(counter());
    assert!(set(&mut s, "Height", "2").get("setf_dropped").is_some());
    let unrelated = set(&mut s, "User.Other", "5");
    assert!(unrelated.get("setf_dropped").is_none(), "an edit that no call reads drops nothing, and is not told the last one's: {unrelated}");
    assert_eq!(cell(&mut s, "User.N")["value"], json!(32.0), "no call was worked out again, so the counter did not move");
}

/// A counter that sets itself off ends at the caps with the document sound, and the edit that set it off is one undo step.
#[test]
fn a_counter_does_not_hang_an_edit_and_the_edit_is_one_step() {
    let counter = ShapeBuilder::new(10).cell("Height", "1").row_cell("User", None, RowKey::name("N"), "Value", "0").row_formula(
        "User",
        None,
        RowKey::name("Tick"),
        "Value",
        "SETF(GETREF(User.N),User.N+Height)",
        "0",
    );
    let mut s = one_shape(counter);
    let steps = undo_len(&s);
    set(&mut s, "Height", "2");
    let n = cell(&mut s, "User.N");
    assert_eq!(n["value"], json!(32.0), "16 rounds of 2: {n}");
    assert_eq!(undo_len(&s), steps + 1);
    assert_eq!(check_session(&s), Ok(()));
}

/// A `GETREF` argument, and so a `SETF`'s target, is no read: a person's write to the cell a `SETF` names does not work the
/// call out, so it does not write its own text over the person's. The call's own input still works it out, and the write to the target is one undo step and nothing else.
#[test]
fn a_persons_write_to_the_target_of_a_setf_stands() {
    let mut s = one_shape(shape());
    set(&mut s, "Width", "3");
    assert_eq!(cell(&mut s, "Scratch.A1")["value"], json!(6.0), "the call wrote twice the width");
    let steps = undo_len(&s);
    let answer = set(&mut s, "Scratch.A1", "99");
    let target = cell(&mut s, "Scratch.A1");
    assert_eq!(target["value"], json!(99.0), "the person's value stands: {target} {answer}");
    assert_eq!(target["formula"], json!("99"), "{target}");
    assert!(answer.get("setf_dropped").is_none() && notes(&answer).iter().all(|n| !n.contains("SETF")), "{answer}");
    assert_eq!(undo_len(&s), steps + 1, "one step, the person's");
    // What the call does read still works it out, and its write goes over the person's value, as the call says.
    set(&mut s, "Width", "4");
    assert_eq!(cell(&mut s, "Scratch.A1")["value"], json!(8.0));
    // Undo takes the edit and what the call wrote with it: the person's value is as it was.
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(cell(&mut s, "Scratch.A1")["value"], json!(99.0));
    assert_eq!(check_session(&s), Ok(()));
}

/// A cell that reads a `SETF` cell's value (the call is 0) is worked out by an edit of what it reads, and that does not make the call write. `Go` writes 6 into
/// `Scratch.A1` and reads nothing that changes; `Reader` reads `Go` and the width. A person's 99 at `A1` stands when the width changes (the evaluator worked `Go` out in place for
/// `Reader`, and queued its write over the person's).
#[test]
fn a_cell_that_reads_a_setf_cell_does_not_make_it_write_over_a_persons_value() {
    let shape = ShapeBuilder::new(10)
        .cell_u("Width", "2", "IN")
        .row_cell("Scratch", None, RowKey::Ix(0), "A", "0")
        .row_formula("User", None, RowKey::name("Reader"), "Value", "User.Go+Width", "0")
        .row_formula("User", None, RowKey::name("Go"), "Value", "SETF(GETREF(Scratch.A1),6)", "0");
    let mut s = one_shape(shape);
    set(&mut s, "Scratch.A1", "99");
    assert_eq!(cell(&mut s, "Scratch.A1")["value"], json!(99.0), "the person's value");
    let steps = undo_len(&s);
    let answer = set(&mut s, "Width", "3");
    let target = cell(&mut s, "Scratch.A1");
    assert_eq!(target["value"], json!(99.0), "the person's value stands: {target} {answer}");
    assert_eq!(target["formula"], json!("99"), "{target}");
    assert_eq!(cell(&mut s, "User.Reader")["value"], json!(3.0), "the reader was worked out: it takes the call's 0 and the width");
    assert!(answer.get("setf_dropped").is_none() && notes(&answer).iter().all(|n| !n.contains("SETF")), "{answer}");
    assert_eq!(undo_len(&s), steps + 1, "one step, the person's");
    assert_eq!(check_session(&s), Ok(()));
}
