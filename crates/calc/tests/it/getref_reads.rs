//! A `GETREF` or `GETVAL` argument names a cell and is no read: a change to that cell does not work out the formula that holds the call ( "A `GETREF` or `GETVAL` argument: No", "`SETF`'s first argument, through `GETREF`: No, which is why it goes through `GETREF`",
//! and the vectors).
//!
//! The dependency graph treated the argument as any reference, so a person's write to the cell a `SETF` names made the cell that holds the call dirty, and the call
//! worked out again and wrote its own text over what the person had written (`setf_written: 1`). These tests go through the commit of an edit
//! (`changes_between`, then `commit_changed`), which is the path of every editing command's recalculation, and hold the other half too: a reference that is a
//! read still works the cell out, and the cells that a formula reads are worked out before it, whatever their place in the sheet.

use std::sync::Arc;

use diacraft_calc::{
    Budget, CommitReport, EvalOptions, ReaderIndex, changes_between, changes_between_with, commit, commit_changed, commit_changed_with,
};
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

// ---- the bug: a SETF's target -----------------------------------------------------------------------------------------------------------------

/// `Go` writes twice the width into `Scratch.A1`. A person writes 99 there: the call names the cell and does not read it, so `Go` is not worked out, and the person's
/// value stays (it was worked out and written over, `setf_written: 1`, before the argument of `GETREF` was no read).
#[test]
fn a_persons_write_to_the_cell_a_setf_names_does_not_work_the_call_out() {
    let shape = with_formula(
        ShapeBuilder::new(1).cell("Width", "3").row_cell("Scratch", None, RowKey::Ix(0), "A", "0"),
        "Go",
        "SETF(GETREF(Scratch.A1),Width*2)",
    );
    let mut doc = one(shape);
    let first = works(&mut doc);
    assert_eq!((formula(&doc, &a1()).as_deref(), value(&doc, &a1()).as_str()), (Some("6"), "6"), "the call wrote: {first:?}");
    let report = edited(&mut doc, &a1(), "99");
    assert_eq!(
        (report.setf_written, report.setf_same, report.setf_rounds),
        (0, 0, 0),
        "the call was not reached by a write to its target: {report:?}"
    );
    assert_eq!((formula(&doc, &a1()).as_deref(), value(&doc, &a1()).as_str()), (Some("99"), "99"), "the person's value stands: {report:?}");
    // What the call does read still works it out: the width.
    let report = edited(&mut doc, &direct("Width"), "5");
    assert_eq!(report.setf_written, 1, "{report:?}");
    assert_eq!((formula(&doc, &a1()).as_deref(), value(&doc, &a1()).as_str()), (Some("10"), "10"), "{report:?}");
    // And the person's value stands again after that: a write to the target is still no read.
    let report = edited(&mut doc, &a1(), "99");
    assert_eq!(report.setf_written, 0, "{report:?}");
    assert_eq!(value(&doc, &a1()), "99");
}

/// A call that names its target by a string reads nothing of it either (the string is no reference to a cell).
#[test]
fn a_persons_write_to_the_cell_a_string_names_does_not_work_the_call_out() {
    let shape = with_formula(
        ShapeBuilder::new(1).cell("Width", "3").row_cell("Scratch", None, RowKey::Ix(0), "A", "0"),
        "Go",
        r#"SETF("Scratch.A1",Width*2)"#,
    );
    let mut doc = one(shape);
    works(&mut doc);
    let report = edited(&mut doc, &a1(), "99");
    assert_eq!(report.setf_written, 0, "{report:?}");
    assert_eq!(value(&doc, &a1()), "99");
}

/// The target of a `SETF` on another shape: the qualified reference is no read either, so the reader is not listed in the index of readers across sheets.
#[test]
fn a_persons_write_to_a_target_on_another_shape_does_not_work_the_call_out() {
    let go = with_formula(ShapeBuilder::new(1).cell("Width", "3"), "Go", "SETF(GETREF(Sheet.2!Scratch.A1),Width*2)");
    let other = ShapeBuilder::new(2).row_cell("Scratch", None, RowKey::Ix(0), "A", "0");
    let mut doc = DocBuilder::new().page(1, PageBuilder::new().shape(go).shape(other)).build();
    let first = works(&mut doc);
    let target = |doc: &Document| match doc.shape(&ShapeRef::top(Owner::Page(1), 1)).expect("the shape").sheet.lookup(&a1()) {
        Lookup::Found(c) => c.value.as_ref().map(|v| v.text().to_owned()),
        other => panic!("{other:?}"),
    };
    assert_eq!(target(&doc).as_deref(), Some("6"), "the call wrote: {first:?}");
    let before = doc.clone();
    doc.write_cell(
        &SheetRef::Shape(ShapeRef::top(Owner::Page(1), 1)),
        &a1(),
        CellEdit::Formula { formula: Arc::new(parse("99")), value: None },
        WriteOrigin::Automation,
    )
    .expect("a write");
    let changes = changes_between(&before, &doc, 1);
    let report = commit_changed(&mut doc, &Budget::default(), EvalOptions::default(), 1, &changes);
    assert_eq!((report.setf_written, report.setf_same), (0, 0), "{report:?}");
    assert_eq!(target(&doc).as_deref(), Some("99"), "the person's value stands");
}

// ---- a read is still a read -------------------------------------------------------------------------------------------------------------------

/// A counter that stops at 3: it reads `N` through an ordinary reference (`IF(N<3,N+1,N)`) and names it through `GETREF` as the target. The ordinary reference is a read, so a
/// change to `N` works the call out whatever else the formula holds: put `N` back to 0 and the call counts it up to 3 again, in three writes.
#[test]
fn a_setf_that_reads_its_own_target_by_a_reference_is_worked_out_when_it_changes() {
    let shape = with_formula(
        ShapeBuilder::new(1).row_cell("User", None, RowKey::name("N"), "Value", "0"),
        "Tick",
        "SETF(GETREF(User.N),IF(User.N<3,User.N+1,User.N))",
    );
    let mut doc = one(shape);
    let first = works(&mut doc);
    assert_eq!(number(&doc, &user("N")), 3.0, "counted to 3 and rested: {first:?}");
    let report = edited(&mut doc, &user("N"), "0");
    assert_eq!(number(&doc, &user("N")), 3.0, "the read worked the call out, and it counted up again: {report:?}");
    assert_eq!(
        (report.setf_written, report.setf_same, report.setf_dropped),
        (3, 1, 0),
        "three writes and a call that found its target holding its text: {report:?}"
    );
}

/// The same cell as an address and as a read, in one formula: it is a read, so a change to it works the cell out.
#[test]
fn a_reference_that_is_an_address_and_also_a_read_is_a_read() {
    let shape = with_formula(
        ShapeBuilder::new(1).cell("Width", "3").row_cell("Scratch", None, RowKey::Ix(0), "A", "0"),
        "Probe",
        "GETVAL(Scratch.A1)+Scratch.A1",
    );
    let mut doc = one(shape);
    works(&mut doc);
    assert_eq!(number(&doc, &user("Probe")), 0.0);
    edited(&mut doc, &a1(), "4");
    assert_eq!(number(&doc, &user("Probe")), 8.0, "the plain reference makes the read");
}

// ---- GETVAL and GETREF in a value's place (the spec's vectors) -------------------------------------------------------------------------------

/// `GETVAL(PinX) + GETVAL(PinY) + Width` is 6 for 1, 2 and 3; a change of `Width` to 4 gives 7, and a change of `PinX` to 9 leaves 7. A link on `PinX` gives 15.
#[test]
fn a_getval_argument_is_no_link_getval_v2() {
    let shape =
        with_formula(ShapeBuilder::new(1).cell("PinX", "1").cell("PinY", "2").cell("Width", "3"), "Probe", "GETVAL(PinX) + GETVAL(PinY) + Width");
    let mut doc = one(shape);
    works(&mut doc);
    assert_eq!(number(&doc, &user("Probe")), 6.0);
    edited(&mut doc, &direct("Width"), "4");
    assert_eq!(number(&doc, &user("Probe")), 7.0);
    edited(&mut doc, &direct("PinX"), "9");
    assert_eq!(number(&doc, &user("Probe")), 7.0, "a change to the cell GETVAL names leaves the formula as it stands");
    // A change that works the formula out reads the cell as it is now, whatever the order the cells were changed in.
    edited(&mut doc, &direct("Width"), "5");
    assert_eq!(number(&doc, &user("Probe")), 16.0, "9 + 2 + 5");
}

/// `PinX*2+GetVal(PinY)+DEPENDSON(Angle)` has links on `PinX` and `Angle` and none on `PinY`. Taking every name in the text as a link adds
/// `PinY`, and reading only the value's operands drops `Angle`.
#[test]
fn the_links_of_a_formula_are_its_reads_and_its_dependson_all_v6() {
    let shape =
        with_formula(ShapeBuilder::new(1).cell("PinX", "1").cell("PinY", "2").cell("Angle", "0"), "Probe", "PinX*2+GetVal(PinY)+DEPENDSON(Angle)");
    let mut doc = one(shape);
    works(&mut doc);
    assert_eq!(number(&doc, &user("Probe")), 4.0, "1*2 + 2 + FALSE");
    edited(&mut doc, &direct("PinY"), "10");
    assert_eq!(number(&doc, &user("Probe")), 4.0, "no link on PinY");
    edited(&mut doc, &direct("PinX"), "3");
    assert_eq!(number(&doc, &user("Probe")), 16.0, "a link on PinX: 3*2 + 10, and PinY is read as it is now");
    edited(&mut doc, &direct("PinY"), "20");
    assert_eq!(number(&doc, &user("Probe")), 16.0);
    edited(&mut doc, &direct("Angle"), "1");
    assert_eq!(number(&doc, &user("Probe")), 26.0, "a link on Angle: 3*2 + 20");
}

/// `GetRef(User.Q)*22` is 11 for 0.5; a change to `User.Q` leaves it, and the call may sit inside others.
#[test]
fn a_getref_in_a_values_place_and_inside_other_calls_is_no_link() {
    let shape = with_formula(
        with_formula(
            ShapeBuilder::new(1).cell("Width", "2").row_cell("User", None, RowKey::name("Q"), "Value", "0.5"),
            "Product",
            "GetRef(User.Q)*22",
        ),
        "Inside",
        "IF(Width>1,MAX(GetRef(User.Q),1)*3,0)",
    );
    let mut doc = one(shape);
    works(&mut doc);
    assert_eq!((number(&doc, &user("Product")), number(&doc, &user("Inside"))), (11.0, 3.0));
    edited(&mut doc, &user("Q"), "2");
    assert_eq!((number(&doc, &user("Product")), number(&doc, &user("Inside"))), (11.0, 3.0), "no link: the cells that name Q by GETREF stand");
    edited(&mut doc, &direct("Width"), "3");
    assert_eq!(number(&doc, &user("Inside")), 6.0, "worked out by `Width`, it reads `Q` as it is now");
    assert_eq!(number(&doc, &user("Product")), 11.0, "and a cell `Width` does not reach stands");
}

// ---- evaluation order -------------------------------------------------------------------------------------------------------------------------

/// A cell that reads a value by `GETVAL` is worked out after the cell it names where both are worked out by one edit, whichever stands first in the sheet: nothing reads a stale
/// value inside one recalculation. `P` is first and reads `Q` by `GETVAL`; both read `Width`, which the edit changes.
#[test]
fn what_a_getval_reads_is_worked_out_first_in_the_same_recalculation() {
    let shape = with_formula(with_formula(ShapeBuilder::new(1).cell("Width", "1"), "P", "GETVAL(User.Q)*2+Width"), "Q", "Width+1");
    let mut doc = one(shape);
    works(&mut doc);
    assert_eq!((number(&doc, &user("P")), number(&doc, &user("Q"))), (5.0, 2.0));
    edited(&mut doc, &direct("Width"), "10");
    assert_eq!(number(&doc, &user("Q")), 11.0);
    assert_eq!(number(&doc, &user("P")), 32.0, "(10 + 1) * 2 + 10, with the new Q and not the old one");
}

/// The order holds for a chain: `P` reads `Q` by `GETVAL`, `Q` reads `R` the same way, `R` reads `Width`, and `P` and `Q` read `Width` too so that all three are worked out.
#[test]
fn a_chain_of_getval_reads_is_worked_out_in_order() {
    let shape = with_formula(
        with_formula(with_formula(ShapeBuilder::new(1).cell("Width", "1"), "P", "GETVAL(User.Q)+Width"), "Q", "GETVAL(User.R)+Width"),
        "R",
        "Width*100",
    );
    let mut doc = one(shape);
    works(&mut doc);
    assert_eq!((number(&doc, &user("P")), number(&doc, &user("Q")), number(&doc, &user("R"))), (102.0, 101.0, 100.0));
    edited(&mut doc, &direct("Width"), "2");
    assert_eq!((number(&doc, &user("P")), number(&doc, &user("Q")), number(&doc, &user("R"))), (204.0, 202.0, 200.0));
}

// ---- cycles ----------------------------------------------------------------------------------------------------------------------------------

/// A cycle through `GETREF` alone is no circular reference (`SETF`'s target goes through it "to avoid a circular reference"): `A` and `B` each name the other as
/// the target of a `SETF`. Neither the commit of the file nor an edit counts a cycle, and no cell holds the loop error: the two calls write once each and are constants after.
#[test]
fn a_cycle_that_goes_through_a_getref_alone_is_no_circular_reference() {
    let shape = with_formula(with_formula(ShapeBuilder::new(1), "A", "SETF(GETREF(User.B),1)"), "B", "SETF(GETREF(User.A),2)");
    let mut doc = one(shape);
    let first = works(&mut doc);
    assert_eq!(first.stats.cycles, 0, "{first:?}");
    assert_eq!((first.setf_written, first.setf_dropped, first.setf_refused), (2, 0, 0), "each call wrote its target once: {first:?}");
    assert_eq!((number(&doc, &user("A")), number(&doc, &user("B"))), (2.0, 1.0), "{first:?}");
    assert_eq!((formula(&doc, &user("A")).as_deref(), formula(&doc, &user("B")).as_deref()), (Some("2"), Some("1")));
}

/// A write to the target is no read of it by the call, and it is a read by a cell that reads it by a reference: the call is not worked out (nothing is written over the person's
/// value) and the reader is.
#[test]
fn a_write_to_a_target_works_out_the_cells_that_read_it_and_not_the_call() {
    let shape = with_formula(
        with_formula(
            ShapeBuilder::new(1).cell("Width", "3").row_cell("User", None, RowKey::name("Out"), "Value", "0"),
            "Go",
            "SETF(GETREF(User.Out),Width*2)",
        ),
        "Copy",
        "User.Out+1",
    );
    let mut doc = one(shape);
    let first = works(&mut doc);
    assert_eq!((number(&doc, &user("Out")), number(&doc, &user("Copy"))), (6.0, 7.0), "{first:?}");
    let report = edited(&mut doc, &user("Out"), "50");
    assert_eq!((report.stats.cycles, report.setf_written, report.setf_same), (0, 0, 0), "{report:?}");
    assert_eq!((number(&doc, &user("Out")), number(&doc, &user("Copy"))), (50.0, 51.0), "the reader was worked out, and the person's value stands");
    let report = edited(&mut doc, &direct("Width"), "4");
    assert_eq!((report.stats.cycles, report.setf_written), (0, 1), "{report:?}");
    assert_eq!((number(&doc, &user("Out")), number(&doc, &user("Copy"))), (8.0, 9.0), "the call's own input works it out, and its write is read");
}

/// A cell that names itself as its own target reads nothing of it: no cycle, and the write it asks for is made once.
#[test]
fn a_setf_that_names_its_own_cell_is_no_circular_reference() {
    let shape = with_formula(ShapeBuilder::new(1).cell("Width", "3"), "Go", "SETF(GETREF(User.Go),Width*2)");
    let mut doc = one(shape);
    let first = works(&mut doc);
    assert_eq!((first.stats.cycles, first.setf_written, first.setf_dropped), (0, 1, 0), "{first:?}");
    assert_eq!(formula(&doc, &user("Go")).as_deref(), Some("6"), "the call replaced itself with the text it was asked to write: {first:?}");
}

/// What a read of a value needs is its result, so two cells that read each other's value by `GETVAL` are a loop for the evaluator all the same: the dependency graph leaves the
/// argument out, and the evaluator works the cell out to read it (the cells it reads are worked out first, which is what keeps a read from being stale). The loop is counted, and
/// the cell that is read while it is being worked out is the loop error.
#[test]
fn two_cells_that_read_each_others_value_are_still_a_loop_for_the_evaluator() {
    let shape = with_formula(with_formula(ShapeBuilder::new(1).cell("Width", "1"), "A", "GETVAL(User.B)+Width"), "B", "GETVAL(User.A)+Width");
    let mut doc = one(shape);
    let first = works(&mut doc);
    assert!(first.stats.cycles >= 1, "{first:?}");
}

// ---- the index of readers that is kept from one edit to the next -----------------------------------------------------------------------------

/// A cell of shape `shape` (the `ID`, on page 1) is given the formula `text` as `sheet.set_cell` gives one, and the commit answers it with the index the session keeps.
fn written_with(index: &mut ReaderIndex, doc: &mut Document, shape: u32, addr: &CellAddr, text: &str) {
    let before = doc.clone();
    let sheet = SheetRef::Shape(ShapeRef::top(Owner::Page(1), shape - 1));
    doc.write_cell(&sheet, addr, CellEdit::Formula { formula: Arc::new(parse(text)), value: None }, WriteOrigin::Automation).expect("a write");
    let changes = changes_between_with(index, &before, doc, 1);
    commit_changed_with(index, doc, &Budget::default(), EvalOptions::default(), 1, &changes);
}

/// The session keeps one index of readers across sheets for the page (`ReaderIndex`) and puts right what an edit replaced: a cell whose formula changes from a read of another
/// shape's cell to an address of it (and back) is classified again, so the cell that was a reader is not one and the other way round.
#[test]
fn the_kept_index_follows_a_formula_that_turns_a_read_into_an_address_and_back() {
    let reader = with_formula(ShapeBuilder::new(1), "P", "GETVAL(Sheet.2!PinX)+1");
    let mut doc = DocBuilder::new().page(1, PageBuilder::new().shape(reader).shape(ShapeBuilder::new(2).cell("PinX", "5"))).build();
    let mut index = ReaderIndex::new();
    let (p, pin) = (user("P"), direct("PinX"));
    works(&mut doc);
    assert_eq!(number(&doc, &p), 6.0);
    written_with(&mut index, &mut doc, 2, &pin, "9");
    assert_eq!(number(&doc, &p), 6.0, "an address is no link, across shapes too");
    // The formula becomes a read: the cell is a reader of the other shape's, and follows it.
    written_with(&mut index, &mut doc, 1, &p, "Sheet.2!PinX+1");
    assert_eq!(number(&doc, &p), 10.0);
    written_with(&mut index, &mut doc, 2, &pin, "20");
    assert_eq!(number(&doc, &p), 21.0, "a read is a link");
    // And an address again: the entry it had is taken out of the index.
    written_with(&mut index, &mut doc, 1, &p, "GETVAL(Sheet.2!PinX)+1");
    assert_eq!(number(&doc, &p), 21.0);
    written_with(&mut index, &mut doc, 2, &pin, "30");
    assert_eq!(number(&doc, &p), 21.0, "the reader of the cell is gone from the index");
}
