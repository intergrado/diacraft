//! `changes_between` on a page whose lists repeat an `ID`, and on sheets with a row of no key. Both are in files: a deleted member
//! (`Del="1"`) beside a live one of its `ID`, and a row of a section keyed by name that has no `N`. They used to make the page one `changes_between`
//! gave up on: the whole page was worked out, every stale cached value was written again and **every instance on the page was given the inherited cells
//! its master's formulas give it another value for** (rows the shapes never had), though the edit was a move of one shape.

use std::sync::Arc;

use diacraft_calc::{Budget, EvalOptions, changes_between, commit_changed};
use diacraft_doc::{CellAddr, CellEdit, Document, Lookup, Owner, Row, RowKey, SectionKey, ShapeRef, SheetRef, Value as DocValue, WriteOrigin};
use diacraft_testkit::build::{DocBuilder, MasterBuilder, PageBuilder, ShapeBuilder};

fn user(name: &str) -> CellAddr {
    CellAddr::in_row("User", None, RowKey::name(name), "Value")
}

fn with(shape: ShapeBuilder, name: &str, formula: &str, value: &str) -> ShapeBuilder {
    shape.row_formula("User", None, RowKey::name(name), "Value", formula, value)
}

fn top(place: u32) -> SheetRef {
    SheetRef::Shape(ShapeRef::top(Owner::Page(1), place))
}

/// The saved `V` text of a user cell of the shape at `place`, or `None` when the shape does not hold the cell.
fn saved_at(doc: &Document, place: u32, name: &str) -> Option<String> {
    let shape = doc.shape(&ShapeRef::top(Owner::Page(1), place)).expect("a shape");
    match shape.sheet.lookup(&user(name)) {
        Lookup::Found(c) => c.value.as_ref().map(|v| v.text().to_owned()),
        _ => None,
    }
}

fn set_at(doc: &mut Document, place: u32, addr: &CellAddr, value: &str) {
    doc.write_cell(&top(place), addr, CellEdit::Value(DocValue::from_text(value)), WriteOrigin::Drawing).expect("a write");
}

/// Master 1's shape has `Width` 1 and `User.Half` = `Width*0.5`, saved as 0.5. The page holds shape 1 (moved by the tests), an instance of the master
/// whose own `Width` is 4 (so its inherited `User.Half` comes to 2 and is not in the file, as a file a writer of another program saved may have it), a
/// shape with a stale cell, and `repeated` after them.
fn page_with(repeated: Vec<ShapeBuilder>) -> Document {
    let master = MasterBuilder::new().shape(with(ShapeBuilder::new(5).cell("Width", "1"), "Half", "Width*0.5", "0.5"));
    let one = ShapeBuilder::new(1).cell("PinX", "1");
    let instance = ShapeBuilder::new(2).master(1).cell("Width", "4");
    let stale = with(ShapeBuilder::new(3), "Stale", "2+2", "9");
    let mut page = PageBuilder::new().shape(one).shape(instance).shape(stale);
    for shape in repeated {
        page = page.shape(shape);
    }
    DocBuilder::new().master(1, master).page(1, page).build()
}

/// A deleted member beside a live shape of its `ID`, as files hold them: shape 7 twice.
fn twins() -> Vec<ShapeBuilder> {
    vec![with(ShapeBuilder::new(7).deleted(), "Stale", "1+1", "9"), with(ShapeBuilder::new(7), "Stale", "3+3", "9")]
}

fn commit(doc: &mut Document, before: &Document) -> diacraft_calc::CommitReport {
    let changes = changes_between(before, doc, 1);
    commit_changed(doc, &Budget::default(), EvalOptions::default(), 1, &changes)
}

#[test]
fn a_move_on_a_page_whose_list_repeats_an_id_works_out_the_moved_shape_and_no_other() {
    let before = page_with(twins());
    let mut doc = before.clone();
    set_at(&mut doc, 0, &CellAddr::direct("PinX"), "5");
    let changes = changes_between(&before, &doc, 1);
    assert!(!changes.whole_page, "a list that repeats an ID is compared shape by shape: {changes:?}");
    assert_eq!(changes.cells, vec![(top(0), CellAddr::direct("PinX"))]);
    assert!(changes.fresh.is_empty() && changes.removed.is_empty() && changes.rows.is_empty(), "{changes:?}");
    let report = commit(&mut doc, &before);
    assert_eq!((report.written, report.added), (0, 0), "nothing reads shape 1's pin: {report:?}");
    // The instance was not reached: it is given no inherited cell, and the stale cells (the twins' too) keep the values the file saved.
    assert_eq!(saved_at(&doc, 1, "Half"), None, "the instance gained a cell the edit never reached");
    assert_eq!(saved_at(&doc, 2, "Stale").as_deref(), Some("9"));
    assert_eq!(saved_at(&doc, 3, "Stale").as_deref(), Some("9"));
    assert_eq!(saved_at(&doc, 4, "Stale").as_deref(), Some("9"));
}

#[test]
fn the_control_a_page_with_no_repeated_id_was_and_is_worked_out_the_same() {
    let before = page_with(Vec::new());
    let mut doc = before.clone();
    set_at(&mut doc, 0, &CellAddr::direct("PinX"), "5");
    let report = commit(&mut doc, &before);
    assert_eq!((report.written, report.added), (0, 0), "{report:?}");
    assert_eq!(saved_at(&doc, 1, "Half"), None);
    // And a pass over every cell of the page, which is what the repeated ID used to cost (`commit_changed` with the whole page was the recalculation of the
    // page then; it gives an instance only the inherited cells the edit changed), gives the instance its inherited cell: the cell the test
    // above must not see.
    let mut whole = before.clone();
    set_at(&mut whole, 0, &CellAddr::direct("PinX"), "5");
    let report = diacraft_calc::commit_in(&mut whole, &Budget::default(), EvalOptions::default(), diacraft_calc::Scope::Page(1));
    assert!(report.added >= 1, "{report:?}");
    assert_eq!(saved_at(&whole, 1, "Half").as_deref(), Some("2"));
}

#[test]
fn a_reader_of_a_shape_whose_id_is_repeated_is_still_worked_out() {
    // Shape 3 reads the first shape 7 (a reference finds the first); the first 7's cell changes, and the reader follows. The second 7 is left alone.
    let reader = with(ShapeBuilder::new(3), "Reads", "Sheet.7!User.Stale+100", "109");
    let first = with(ShapeBuilder::new(7), "Stale", "1+1", "9");
    let second = with(ShapeBuilder::new(7), "Stale", "3+3", "9");
    let before = DocBuilder::new().page(1, PageBuilder::new().shape(reader).shape(first).shape(second)).build();
    let mut doc = before.clone();
    doc.write_cell(&top(1), &user("Stale"), CellEdit::Value(DocValue::from_text("20")), WriteOrigin::Drawing).expect("a write");
    let changes = changes_between(&before, &doc, 1);
    assert!(!changes.whole_page, "{changes:?}");
    assert_eq!(changes.cells, vec![(top(1), user("Stale"))], "the changed cell is the first 7's, at its own place");
    commit(&mut doc, &before);
    assert_eq!(saved_at(&doc, 0, "Reads").as_deref(), Some("120"));
    assert_eq!(saved_at(&doc, 2, "Stale").as_deref(), Some("9"), "the second shape 7 was not worked out");
}

#[test]
fn taking_out_a_shape_beside_a_repeated_id_names_it_alone() {
    let before = page_with(twins());
    let mut doc = before.clone();
    // Shape 3 goes: the twins after it move up one place, and are still the twins they were.
    if let Some(page) = doc.pages.get_mut(0) {
        Arc::make_mut(page).shapes.remove(2);
    }
    let changes = changes_between(&before, &doc, 1);
    assert!(!changes.whole_page, "{changes:?}");
    assert_eq!(changes.removed, vec![3]);
    assert!(changes.cells.is_empty() && changes.fresh.is_empty(), "the twins are unchanged: {changes:?}");
    let report = commit(&mut doc, &before);
    assert_eq!(report.added, 0, "{report:?}");
    assert_eq!(saved_at(&doc, 1, "Half"), None);
}

#[test]
fn taking_out_one_of_two_shapes_of_one_id_still_says_its_id() {
    let before = page_with(twins());
    let mut doc = before.clone();
    if let Some(page) = doc.pages.get_mut(0) {
        Arc::make_mut(page).shapes.remove(4);
    }
    let changes = changes_between(&before, &doc, 1);
    assert!(!changes.whole_page, "{changes:?}");
    assert_eq!(changes.removed, vec![7]);
}

/// Pushes into the shape at `place` a `User` row with no `N` (and no `IX`), holding one cell `Value`.
fn push_keyless(doc: &mut Document, place: u32, value: &str) {
    let mut row = Row::default();
    row.cells.insert(diacraft_doc::CellName::new("Value"), diacraft_doc::Cell::constant(DocValue::from_text(value)));
    let page = Arc::make_mut(doc.pages.get_mut(0).expect("a page"));
    let shape = Arc::make_mut(page.shapes.get_mut(usize::try_from(place).expect("a place")).expect("a shape"));
    let sheet = Arc::make_mut(&mut shape.sheet);
    let section = sheet.sections.iter_mut().find(|s| s.name.as_str() == "User").expect("a User section");
    Arc::make_mut(section).rows.push(Arc::new(row));
}

#[test]
fn a_row_of_no_key_in_a_section_an_edit_changes_leaves_its_shape_a_shape_with_its_cells_and_not_a_new_one() {
    // Shape 1 has a user section with a named row and a row of no key.
    let one = with(ShapeBuilder::new(1).cell("PinX", "1"), "Named", "PinX*2", "2");
    let mut before = {
        let master = MasterBuilder::new().shape(with(ShapeBuilder::new(5).cell("Width", "1"), "Half", "Width*0.5", "0.5"));
        let instance = ShapeBuilder::new(2).master(1).cell("Width", "4");
        DocBuilder::new().master(1, master).page(1, PageBuilder::new().shape(one).shape(instance)).build()
    };
    push_keyless(&mut before, 0, "3");
    // A cell of the named row is written, so the section is a copy: its rows are compared, the one of no key among them.
    let mut doc = before.clone();
    doc.write_cell(&top(0), &user("Named"), CellEdit::Value(DocValue::from_text("7")), WriteOrigin::Drawing).expect("a write");
    let changes = changes_between(&before, &doc, 1);
    assert!(!changes.whole_page && changes.fresh.is_empty(), "a row of no key made the edited shape a new one: {changes:?}");
    assert_eq!(changes.cells, vec![(top(0), user("Named"))]);
    assert!(changes.rows.is_empty(), "the row of no key is as it was: {changes:?}");
    let report = commit(&mut doc, &before);
    assert_eq!(report.added, 0, "{report:?}");
    assert_eq!(saved_at(&doc, 1, "Half"), None, "the instance was given a cell the edit never reached");
    // The row of no key changed: the section changed as a whole, and no cell of it is named.
    let mut edited = before.clone();
    {
        let page = Arc::make_mut(edited.pages.get_mut(0).expect("a page"));
        let shape = Arc::make_mut(page.shapes.get_mut(0).expect("a shape"));
        let sheet = Arc::make_mut(&mut shape.sheet);
        let section = Arc::make_mut(sheet.sections.iter_mut().find(|s| s.name.as_str() == "User").expect("User"));
        let last = section.rows.len() - 1;
        let mut row = Row::default();
        row.cells.insert(diacraft_doc::CellName::new("Value"), diacraft_doc::Cell::constant(DocValue::from_text("4")));
        section.rows.truncate(last);
        section.rows.push(Arc::new(row));
    }
    let changes = changes_between(&before, &edited, 1);
    assert!(!changes.whole_page && changes.fresh.is_empty(), "{changes:?}");
    assert_eq!(changes.rows, vec![(top(0), SectionKey::new("User", None), None)], "{changes:?}");
    assert!(changes.cells.is_empty(), "{changes:?}");
}

#[test]
fn a_row_of_no_key_in_the_page_sheet_does_not_make_the_page_one_to_work_out_whole() {
    let mut before = page_with(Vec::new());
    {
        let page = Arc::make_mut(before.pages.get_mut(0).expect("a page"));
        let sheet = Arc::make_mut(&mut page.sheet);
        let mut section = diacraft_doc::Section::new("User", None);
        let mut row = Row::default();
        row.cells.insert(diacraft_doc::CellName::new("Value"), diacraft_doc::Cell::constant(DocValue::from_text("1")));
        section.rows.push(Arc::new(row));
        sheet.sections.push(Arc::new(section));
    }
    // A new row of the section that holds the row of no key: the section is a copy, and its rows are compared.
    let mut doc = before.clone();
    doc.write_cell(&SheetRef::Page(1), &user("Tag"), CellEdit::Value(DocValue::from_text("20")), WriteOrigin::Drawing).expect("a write");
    let changes = changes_between(&before, &doc, 1);
    assert!(!changes.whole_page, "{changes:?}");
    assert_eq!(changes.cells, vec![(SheetRef::Page(1), user("Tag"))]);
    commit(&mut doc, &before);
    assert_eq!(saved_at(&doc, 1, "Half"), None, "the instance was given a cell the edit never reached");
}

/// The saved `V` and whether an `E` is on the user cell `name` of the shape at `place`.
fn saved_with_error(doc: &Document, place: u32, name: &str) -> (Option<String>, bool) {
    let shape = doc.shape(&ShapeRef::top(Owner::Page(1), place)).expect("a shape");
    match shape.sheet.lookup(&user(name)) {
        Lookup::Found(c) => (c.value.as_ref().map(|v| v.text().to_owned()), c.error.is_some()),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_reference_that_never_found_a_sheet_is_left_alone_when_another_shape_is_taken_out() {
    // Shape 2 names `Sheet.99`, which the page never had: the file's value stands, whatever shape goes. Shape 3 names shape 1, which goes: its cell is worked out and
    // holds the error with its last value. Shape 4 names a shape by a name nothing has: that spelling is not told by an `ID`, and is worked out as before.
    let one = ShapeBuilder::new(1).cell("PinX", "1");
    let never = with(ShapeBuilder::new(2), "Far", "Sheet.99!PinX+1", "5");
    let near = with(ShapeBuilder::new(3), "Near", "Sheet.1!PinX+10", "11");
    let named = with(ShapeBuilder::new(4), "Ghost", "Nobody!PinX+1", "7");
    let before = DocBuilder::new().page(1, PageBuilder::new().shape(one).shape(never).shape(near).shape(named)).build();
    let mut doc = before.clone();
    if let Some(page) = doc.pages.get_mut(0) {
        Arc::make_mut(page).shapes.remove(0);
    }
    let changes = changes_between(&before, &doc, 1);
    assert_eq!(changes.removed, vec![1]);
    commit(&mut doc, &before);
    assert_eq!(saved_with_error(&doc, 0, "Far"), (Some("5".to_owned()), false), "a reference to a shape that never was took an error");
    assert_eq!(saved_with_error(&doc, 1, "Near"), (Some("11".to_owned()), true), "a reference to the shape taken out holds the error");
    assert_eq!(saved_with_error(&doc, 2, "Ghost"), (Some("7".to_owned()), true), "a name is worked out as before");
}
