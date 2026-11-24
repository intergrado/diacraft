//! A pass over the whole page that answers an edit outside the page: a style, a master's formula. `changes_between` cannot follow such
//! an edit cell by cell, so the page is worked out whole. The pass used to give **every instance on the page** each cell it inherits from its master's
//! formulas whose result is not the master's saved value, the edit's or not, and to leave the master's own cell as it was: an
//! instance at its master's size then held a copy of the very cell the edit changed, and one of another size the cells its file left to the master.
//! Now the pass is given the document the edit started from: it works out the master cells the page's instances use that the edit changed, and an
//! instance is given a cell only where the edit changed what the cell works out to on it and the result is not its master's.

use diacraft_calc::{Budget, Changes, CommitReport, EvalOptions, Scope, changes_between, commit_changed, commit_in};
use diacraft_doc::{CellAddr, CellEdit, Document, FormulaSlot, Lookup, Owner, RowKey, ShapeRef, SheetRef, WriteOrigin};
use diacraft_testkit::build::{DocBuilder, MasterBuilder, PageBuilder, ShapeBuilder, StyleBuilder};

const POINT: f64 = 1.0 / 72.0;

fn user(name: &str) -> CellAddr {
    CellAddr::in_row("User", None, RowKey::name(name), "Value")
}

fn master_sheet() -> SheetRef {
    SheetRef::Shape(ShapeRef::top(Owner::Master(1), 0))
}

/// Style 3 holds the line weight as a formula. Master 1's shape takes its line from it and holds the weight as files do (`Inh` with the value), and two
/// user cells of its size, `Half` = `Width*0.5` and `Span` = `Width`, saved for its `Width` of 1. The page holds an instance at the master's size (shape 10)
/// and one of `Width` 4 (shape 11) whose file left both user cells to the master, as a writer of another program may (they come to 2 and 4 on it).
fn drawing() -> Document {
    let style = StyleBuilder::new().name("Thick").links(Some(0), Some(0), Some(0)).formula("LineWeight", "1 pt", "0.01388888888888889");
    let master = MasterBuilder::new().shape(
        ShapeBuilder::new(5)
            .links(Some(3), Some(3), Some(3))
            .cell("Width", "1")
            .inh("LineWeight", Some("0.01388888888888889"))
            .row_formula("User", None, RowKey::name("Half"), "Value", "Width*0.5", "0.5")
            .row_formula("User", None, RowKey::name("Span"), "Value", "Width", "1"),
    );
    let page = PageBuilder::new().shape(ShapeBuilder::new(10).master(1).cell("PinX", "1")).shape(ShapeBuilder::new(11).master(1).cell("Width", "4"));
    DocBuilder::new().style(3, style).master(1, master).page(1, page).build()
}

/// The cell the sheet holds itself at `addr`: its formula slot and its value text.
fn held(doc: &Document, sheet: &SheetRef, addr: &CellAddr) -> Option<(FormulaSlot, Option<String>)> {
    let data = doc.sheet_of(sheet)?;
    match data.lookup(addr) {
        Lookup::Found(c) => Some((c.formula.clone(), c.value.as_ref().map(|v| v.text().to_owned()))),
        _ => None,
    }
}

fn instance(place: u32) -> SheetRef {
    SheetRef::Shape(ShapeRef::top(Owner::Page(1), place))
}

/// The cells shapes 10 and 11 hold themselves.
fn own(doc: &Document) -> [usize; 2] {
    [0, 1].map(|place| {
        let shape = doc.shape(&ShapeRef::top(Owner::Page(1), place)).expect("the instance");
        shape.sheet.cells.len() + shape.sheet.sections.iter().map(|s| s.rows.iter().map(|r| r.cells.len()).sum::<usize>()).sum::<usize>()
    })
}

fn commit_edit(doc: &mut Document, before: &Document) -> (Changes, CommitReport) {
    let changes = changes_between(before, doc, 1);
    let report = commit_changed(doc, &Budget::default(), EvalOptions::default(), 1, &changes);
    (changes, report)
}

fn number(text: Option<String>) -> f64 {
    text.and_then(|t| t.parse().ok()).expect("a number")
}

#[test]
fn a_style_edited_works_out_the_cell_of_the_master_and_gives_no_instance_a_copy() {
    let before = drawing();
    let mut doc = before.clone();
    let three = std::sync::Arc::new(diacraft_formula::parse("3 pt"));
    doc.write_cell(&SheetRef::Style(3), &CellAddr::direct("LineWeight"), CellEdit::Formula { formula: three, value: None }, WriteOrigin::Drawing)
        .expect("a write");
    let (changes, report) = commit_edit(&mut doc, &before);
    assert!(changes.whole_page, "a style is outside the page: {changes:?}");
    assert_eq!(report.added, 0, "{report:?}");
    assert_eq!(own(&doc), own(&before), "an instance was given a cell");
    // The master holds the new weight, so both instances read it through it.
    let (slot, value) = held(&doc, &master_sheet(), &CellAddr::direct("LineWeight")).expect("the master's cell");
    assert_eq!(slot, FormulaSlot::Inherit);
    assert!((number(value) - 3.0 * POINT).abs() < 1e-12);
}

#[test]
fn a_master_formula_edited_gives_a_cell_only_to_the_instance_it_reaches_with_another_value() {
    let before = drawing();
    let mut doc = before.clone();
    let thrice = std::sync::Arc::new(diacraft_formula::parse("Width*3"));
    doc.write_cell(&master_sheet(), &user("Span"), CellEdit::Formula { formula: thrice, value: None }, WriteOrigin::Drawing).expect("a write");
    let (changes, report) = commit_edit(&mut doc, &before);
    assert!(changes.whole_page, "a master's cell is outside the page: {changes:?}");
    // The master's own cell takes its new result, and the instance at its size reads it through it.
    assert_eq!(held(&doc, &master_sheet(), &user("Span")).and_then(|(_, v)| v).as_deref(), Some("3"));
    assert_eq!(held(&doc, &instance(0), &user("Span")), None, "the instance at the master's size kept a copy");
    // The instance of `Width` 4 works the new formula out to 12, not the master's 3: that one cell is held, `Inh` with its result.
    assert_eq!(held(&doc, &instance(1), &user("Span")), Some((FormulaSlot::Inherit, Some("12".to_owned()))));
    assert_eq!(report.added, 1, "{report:?}");
    // `Half` comes to 2 on it and to the master's 0.5 on the master, but the edit did not change it: the file's way of leaving it to the master stays.
    assert_eq!(held(&doc, &instance(1), &user("Half")), None, "a cell the edit did not change was added");
}

#[test]
fn a_pass_over_the_whole_page_with_no_document_before_adds_no_inherited_cell() {
    // A caller that says the page is to be worked out whole and cannot say what the edit started from gives no inherited cell to any instance: nothing tells
    // which results are the edit's.
    let mut doc = drawing();
    let report = commit_changed(&mut doc, &Budget::default(), EvalOptions::default(), 1, &Changes { whole_page: true, ..Changes::default() });
    assert_eq!(report.added, 0, "{report:?}");
    assert_eq!(held(&doc, &instance(1), &user("Half")), None);
    // The control: a recalculation of the page, which answers no edit, gives the instance of another size its cells as R34 says.
    let mut doc = drawing();
    let report = commit_in(&mut doc, &Budget::default(), EvalOptions::default(), Scope::Page(1));
    assert_eq!(report.added, 2, "{report:?}");
    assert_eq!(held(&doc, &instance(1), &user("Half")), Some((FormulaSlot::Inherit, Some("2".to_owned()))));
}

#[test]
fn the_cells_worked_out_in_the_document_before_are_charged_to_the_commit() {
    let before = drawing();
    let mut doc = before.clone();
    let thrice = std::sync::Arc::new(diacraft_formula::parse("Width*3"));
    doc.write_cell(&master_sheet(), &user("Span"), CellEdit::Formula { formula: thrice, value: None }, WriteOrigin::Drawing).expect("a write");
    let edited = doc.clone();
    let (_, report) = commit_edit(&mut doc, &before);
    // Each step of the pass is the budget's: a budget one step short of it ends the commit as spent, and a budget of exactly it does not.
    let steps = report.stats.steps;
    for (budget, spent) in [(steps - 1, true), (steps, false)] {
        let mut again = edited.clone();
        let changes = changes_between(&before, &again, 1);
        let r = commit_changed(&mut again, &Budget { steps: budget, ..Budget::default() }, EvalOptions::default(), 1, &changes);
        assert_eq!(r.budget_exhausted, spent, "{budget} of {steps}: {r:?}");
    }
    // The pass with the document before costs more than the pass without it, which works out no cell there.
    let mut blind = edited.clone();
    let without = commit_changed(&mut blind, &Budget::default(), EvalOptions::default(), 1, &Changes { whole_page: true, ..Changes::default() });
    assert!(steps > without.stats.steps, "{steps} against {}", without.stats.steps);
}

#[test]
fn a_page_the_document_before_did_not_hold_is_new_in_every_cell_its_instances_inherit() {
    // An edit that made the page (the document before has none of it) changed every cell of it: an instance of another size is given the cells its
    // master's formulas give it another value for, as a shape just dropped is.
    let before = DocBuilder::new().build();
    let mut doc = drawing();
    let (changes, report) = commit_edit(&mut doc, &before);
    assert!(changes.whole_page, "{changes:?}");
    assert_eq!(held(&doc, &instance(1), &user("Half")), Some((FormulaSlot::Inherit, Some("2".to_owned()))), "{report:?}");
    assert_eq!(held(&doc, &instance(0), &user("Half")), None);
}
