//! `changes_between` and `commit_changed`: an edit works out the cells it changed and the cells that read them, and no others.
//! A cell that is not worked out keeps its saved value, whatever this evaluator would give for it: every page here has cells that say 9 where
//! their formulas give something else, and the tests check that those stay 9 unless something they read changed.

use diacraft_calc::{Budget, EvalOptions, Scope, changes_between, commit_changed, commit_in};
use diacraft_doc::{CellAddr, CellEdit, Document, Lookup, Owner, RowKey, ShapeRef, SheetRef, Value as DocValue, WriteOrigin};
use diacraft_testkit::build::{DocBuilder, MasterBuilder, PageBuilder, ShapeBuilder};

fn user(name: &str) -> CellAddr {
    CellAddr::in_row("User", None, RowKey::name(name), "Value")
}

/// A shape with a user cell `name` that has this formula and saved value.
fn with(shape: ShapeBuilder, name: &str, formula: &str, value: &str) -> ShapeBuilder {
    shape.row_formula("User", None, RowKey::name(name), "Value", formula, value)
}

fn sheet_of(doc: &Document, id: u32) -> SheetRef {
    SheetRef::Shape(doc.find_shape(Owner::Page(1), id).expect("a shape"))
}

/// The saved `V` text of a user cell of a shape, and its `E`.
fn saved(doc: &Document, id: u32, name: &str) -> (Option<String>, Option<String>) {
    let at = doc.find_shape(Owner::Page(1), id).expect("a shape");
    match doc.shape(&at).expect("a shape").sheet.lookup(&user(name)) {
        Lookup::Found(c) => (c.value.as_ref().map(|v| v.text().to_owned()), c.error.as_deref().map(str::to_owned)),
        other => panic!("{other:?}"),
    }
}

fn set(doc: &mut Document, id: u32, addr: &CellAddr, value: &str) {
    let sheet = sheet_of(doc, id);
    doc.write_cell(&sheet, addr, CellEdit::Value(DocValue::from_text(value)), WriteOrigin::Drawing).expect("a write");
}

/// Shape 1 holds `PinX` 1, `User.A` = `PinX*2` (2), `User.B` = `User.A+1` (3) and a stale `User.Stale`; shape 2 holds `User.C` =
/// `Sheet.1!PinX+10` (11) and a stale cell too.
fn page() -> Document {
    let one = with(with(with(ShapeBuilder::new(1).cell("PinX", "1"), "A", "PinX*2", "2"), "B", "User.A+1", "3"), "Stale", "1+1", "9");
    let two = with(with(ShapeBuilder::new(2), "C", "Sheet.1!PinX+10", "11"), "Stale", "2+2", "9");
    DocBuilder::new().page(1, PageBuilder::new().shape(one).shape(two)).build()
}

fn commit(doc: &mut Document, before: &Document) -> diacraft_calc::CommitReport {
    let changes = changes_between(before, doc, 1);
    commit_changed(doc, &Budget::default(), EvalOptions::default(), 1, &changes)
}

#[test]
fn a_changed_cell_is_found_and_its_readers_are_worked_out_through_a_chain_and_a_sheet_reference() {
    let before = page();
    let mut doc = before.clone();
    set(&mut doc, 1, &CellAddr::direct("PinX"), "5");
    let changes = changes_between(&before, &doc, 1);
    assert_eq!(changes.cells, vec![(sheet_of(&doc, 1), CellAddr::direct("PinX"))]);
    assert!(!changes.whole_page && changes.fresh.is_empty() && changes.removed.is_empty());
    let report = commit(&mut doc, &before);
    // A, B (which reads A) and C (which reads shape 1's pin) are written; the two stale cells are not read by anything that changed.
    assert_eq!(report.written, 3, "{report:?}");
    assert_eq!(saved(&doc, 1, "A").0.as_deref(), Some("10"));
    assert_eq!(saved(&doc, 1, "B").0.as_deref(), Some("11"));
    assert_eq!(saved(&doc, 2, "C").0.as_deref(), Some("15"));
    assert_eq!(saved(&doc, 1, "Stale").0.as_deref(), Some("9"));
    assert_eq!(saved(&doc, 2, "Stale").0.as_deref(), Some("9"));
    // The whole page would have written the stale ones too.
    let mut everything = doc.clone();
    assert_eq!(commit_in(&mut everything, &Budget::default(), EvalOptions::default(), Scope::Page(1)).written, 2);
    // And a second commit of the same change writes nothing.
    assert_eq!(commit(&mut doc, &before).written, 0);
}

#[test]
fn nothing_changed_is_an_empty_commit() {
    let before = page();
    let mut doc = before.clone();
    let changes = changes_between(&before, &doc, 1);
    assert!(changes.is_empty());
    let report = commit_changed(&mut doc, &Budget::default(), EvalOptions::default(), 1, &changes);
    assert_eq!((report.written, report.unchanged, report.skipped), (0, 0, 0));
    assert_eq!(doc, before);
}

#[test]
fn a_cell_that_changes_to_what_it_held_is_no_change_and_a_reader_that_comes_out_as_it_was_is_not_written() {
    let before = page();
    let mut doc = before.clone();
    // The stale cell has a value that its formula does not give; a reader of a cell that changed *to its own value* comes out the same.
    set(&mut doc, 1, &CellAddr::direct("PinX"), "1");
    let report = commit(&mut doc, &before);
    assert_eq!(report.written, 0, "{report:?}");
    assert_eq!(saved(&doc, 1, "Stale").0.as_deref(), Some("9"));
}

#[test]
fn a_shape_taken_out_reaches_the_cells_that_named_it_and_no_others() {
    let before = page();
    let two = with(with(ShapeBuilder::new(2), "C", "Sheet.1!PinX+10", "11"), "Stale", "2+2", "9");
    let mut doc = DocBuilder::new().page(1, PageBuilder::new().shape(two)).build();
    let changes = changes_between(&before, &doc, 1);
    assert_eq!(changes.removed, vec![1]);
    let report = commit(&mut doc, &before);
    // `Sheet.1` is nothing now: the cell that named it holds an error, with its last good value, and the stale cell is left alone.
    assert_eq!(report.written, 1, "{report:?}");
    let (value, error) = saved(&doc, 2, "C");
    assert_eq!((value.as_deref(), error.is_some()), (Some("11"), true));
    assert_eq!(saved(&doc, 2, "Stale"), (Some("9".to_owned()), None));
}

#[test]
fn a_cell_that_reads_what_its_text_does_not_name_is_worked_out_after_any_change() {
    // `SHEETREF()!PinX` names the sheet by a call: no reference says which cell it reads, so it is always worked out.
    let shape = with(ShapeBuilder::new(1).cell("PinX", "1"), "Through", "SHEETREF()!PinX*2", "9");
    let other = with(ShapeBuilder::new(2), "Stale", "2+2", "9");
    let before = DocBuilder::new().page(1, PageBuilder::new().shape(shape).shape(other)).build();
    let mut doc = before.clone();
    set(&mut doc, 2, &CellAddr::direct("Width"), "3");
    let report = commit(&mut doc, &before);
    assert_eq!(saved(&doc, 1, "Through").0.as_deref(), Some("2"), "{report:?}");
    assert_eq!(saved(&doc, 2, "Stale").0.as_deref(), Some("9"));
}

#[test]
fn a_frame_function_reads_the_pose_of_the_shape_it_names_and_not_the_cells_it_does_not() {
    // Shape 2 holds a point in shape 1's frame carried to the page, as a connector's end is glued. Shape 1's pin moves; its `Width` does not
    // matter to it, and a user cell of shape 1 that reads `Width` is left alone.
    let one = with(
        ShapeBuilder::new(1).cell("PinX", "1").cell("PinY", "1").cell("LocPinX", "0").cell("LocPinY", "0").cell("Width", "2"),
        "Wide",
        "Width*3",
        "9",
    );
    let two = with(ShapeBuilder::new(2), "Where", "PNTX(PAR(PNT(Sheet.1!Width,Sheet.1!Width)))", "0");
    let before = DocBuilder::new().page(1, PageBuilder::new().shape(one).shape(two)).build();
    let mut doc = before.clone();
    set(&mut doc, 1, &CellAddr::direct("PinX"), "4");
    let report = commit(&mut doc, &before);
    // The point (2, 2) in shape 1's frame is at (2 + 4, 2 + 1): its x is 6.
    assert_eq!(saved(&doc, 2, "Where").0.as_deref(), Some("6"), "{report:?}");
    assert_eq!(saved(&doc, 1, "Wide").0.as_deref(), Some("9"), "nothing that changed is read by it");
}

#[test]
fn a_change_to_a_master_or_a_style_is_a_whole_page_and_a_master_added_is_not() {
    let master = |cell: &str| MasterBuilder::new().shape(ShapeBuilder::new(1).cell("Width", cell));
    let built = |width: &str, more: bool| {
        let mut b = DocBuilder::new().master(1, master(width)).page(1, PageBuilder::new().shape(ShapeBuilder::new(1)));
        if more {
            b = b.master(2, master("3"));
        }
        b.build()
    };
    let before = built("2", false);
    assert!(!changes_between(&before, &built("2", true), 1).whole_page, "a master added is not a change to what the page inherits");
    assert!(changes_between(&before, &built("5", false), 1).whole_page, "a master changed is");
    assert!(changes_between(&before, &Document::empty(), 1).whole_page, "a page that is not there is");
    let styled = DocBuilder::new().root(|r| r.cell("LineWeight", "0.5")).page(1, PageBuilder::new().shape(ShapeBuilder::new(1))).build();
    assert!(changes_between(&before, &styled, 1).whole_page, "a style changed is");
}

#[test]
fn a_spent_budget_ends_the_commit_without_writing_a_cell_it_did_not_finish() {
    let before = page();
    let mut doc = before.clone();
    set(&mut doc, 1, &CellAddr::direct("PinX"), "5");
    let changes = changes_between(&before, &doc, 1);
    let report = commit_changed(&mut doc, &Budget::with_steps(3), EvalOptions::default(), 1, &changes);
    assert!(report.budget_exhausted, "{report:?}");
    // What was written, if anything, is right: no cell holds a value that is neither the old one nor the new.
    for (id, name, old, new) in [(1, "A", "2", "10"), (1, "B", "3", "11"), (2, "C", "11", "15")] {
        let value = saved(&doc, id, name).0.unwrap();
        assert!(value == old || value == new, "{id}.{name} is {value}");
    }
}

#[test]
fn shapes_added_are_new_in_every_cell_and_their_readers_follow() {
    let before = page();
    // A third shape is added, and shape 2 names it.
    let three = with(ShapeBuilder::new(3).cell("PinX", "7"), "K", "PinX+1", "0");
    let two = with(with(ShapeBuilder::new(2), "C", "Sheet.3!PinX+10", "11"), "Stale", "2+2", "9");
    let doc_after = {
        let one = with(with(with(ShapeBuilder::new(1).cell("PinX", "1"), "A", "PinX*2", "2"), "B", "User.A+1", "3"), "Stale", "1+1", "9");
        DocBuilder::new().page(1, PageBuilder::new().shape(one).shape(two).shape(three)).build()
    };
    let mut doc = doc_after;
    let changes = changes_between(&before, &doc, 1);
    assert_eq!(changes.fresh, vec![SheetRef::Shape(ShapeRef::top(Owner::Page(1), 2))]);
    let report = commit(&mut doc, &before);
    assert!(report.written >= 2, "{report:?}");
    assert_eq!(saved(&doc, 3, "K").0.as_deref(), Some("8"), "the new shape's own cell is worked out");
    assert_eq!(saved(&doc, 2, "C").0.as_deref(), Some("17"), "the cell that now names it is too");
    assert_eq!(saved(&doc, 1, "Stale").0.as_deref(), Some("9"));
}

#[test]
fn a_long_chain_of_readers_is_worked_out_without_recursion_in_a_bounded_time() {
    // 5,000 cells, each reading the one before: one change reaches all of them, and the walk is a loop, not a chain of calls.
    const LINKS: usize = 5_000;
    let mut shape = ShapeBuilder::new(1).cell("PinX", "1");
    for i in 0..LINKS {
        let formula = if i == 0 { "PinX+1".to_owned() } else { format!("User.C{}+1", i - 1) };
        shape = shape.row_formula("User", None, RowKey::name(&format!("C{i}")), "Value", &formula, &format!("{}", i + 2));
    }
    let before = DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build();
    let mut doc = before.clone();
    set(&mut doc, 1, &CellAddr::direct("PinX"), "10");
    let started = std::time::Instant::now();
    let changes = changes_between(&before, &doc, 1);
    let report = commit_changed(&mut doc, &Budget::default(), EvalOptions::default(), 1, &changes);
    let took = started.elapsed();
    // A hang guard and no measure of cost: the walk of 5,000 readers takes milliseconds, and `written` below is the count of its work.
    assert!(took < std::time::Duration::from_secs(60), "{took:?}");
    assert!(!report.budget_exhausted, "{report:?}");
    assert_eq!(report.written, LINKS as u64, "{report:?}");
    assert_eq!(saved(&doc, 1, &format!("C{}", LINKS - 1)).0, Some(format!("{}", 10 + LINKS)));
}

#[test]
fn a_reader_that_comes_out_as_it_was_does_not_pass_the_change_on() {
    // Of the M3 review. `A` reads the pin and gives 0 at 1 and at 5: it is worked out and not written. `B` (in its sheet) and `D` (in another)
    // read `A`, and each holds a value that its formula does not give. Nothing they read changed, so they are left as the file had them; the
    // pin is what changed, and only the cells that read the pin are worked out.
    let one = with(
        with(with(with(ShapeBuilder::new(1).cell("PinX", "1"), "A", "PinX*0", "0"), "B", "User.A+1", "9"), "P", "PinX+1", "2"),
        "Stale",
        "1+1",
        "9",
    );
    let two = with(ShapeBuilder::new(2), "D", "Sheet.1!User.A+2", "9");
    let before = DocBuilder::new().page(1, PageBuilder::new().shape(one).shape(two)).build();
    let mut doc = before.clone();
    set(&mut doc, 1, &CellAddr::direct("PinX"), "5");
    let report = commit(&mut doc, &before);
    assert_eq!(saved(&doc, 1, "P").0.as_deref(), Some("6"), "a reader of the pin that changes is written: {report:?}");
    assert_eq!(saved(&doc, 1, "A").0.as_deref(), Some("0"));
    assert_eq!(saved(&doc, 1, "B").0.as_deref(), Some("9"), "B reads A, which came out as it was");
    assert_eq!(saved(&doc, 2, "D").0.as_deref(), Some("9"), "D reads A through a sheet reference");
    assert_eq!(saved(&doc, 1, "Stale").0.as_deref(), Some("9"));
    assert_eq!(report.written, 1, "{report:?}");
}

#[test]
fn a_cell_the_evaluator_does_not_write_does_not_pass_the_change_on_either() {
    // A cell whose function this evaluator does not know is skipped; what reads it holds the file's own value and keeps it (a connector end
    // glued by a function `calc` cannot work out, and the width that reads the end).
    let one = with(with(ShapeBuilder::new(1).cell("PinX", "1"), "Unknown", "NOSUCHFUNCTION(PinX)", "3.4999"), "Reads", "User.Unknown*1", "3.5001");
    let before = DocBuilder::new().page(1, PageBuilder::new().shape(one)).build();
    let mut doc = before.clone();
    set(&mut doc, 1, &CellAddr::direct("PinX"), "5");
    let report = commit(&mut doc, &before);
    assert_eq!(saved(&doc, 1, "Unknown").0.as_deref(), Some("3.4999"));
    assert_eq!(saved(&doc, 1, "Reads").0.as_deref(), Some("3.5001"), "{report:?}");
}
