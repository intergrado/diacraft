//! `commit_changed` on the sheets no page holds: a cell written on the document's sheet, on a style's or on a master's page sheet is read by the cells of its own sheet that name
//! it, and those are worked out after it, as the cells of a page's sheet are; a cell of the same sheet that reads nothing that changed keeps the value the file saved.

use diacraft_calc::{Budget, Changes, EvalOptions, commit_changed};
use diacraft_doc::{CellAddr, CellEdit, Document, Lookup, RowKey, SheetRef, Value as DocValue, WriteOrigin};
use diacraft_testkit::build::{DocBuilder, MasterBuilder, PageBuilder, ShapeBuilder, StyleBuilder};

fn user(name: &str) -> CellAddr {
    CellAddr::in_row("User", None, RowKey::name(name), "Value")
}

/// The sheets' cells: `A` holds 5, `B` reads `A` plus 1 (saved 6), `C` reads `B` times 2 (saved 12), and `Stale` says 9 where its formula gives 4.
macro_rules! cells {
    ($builder:expr) => {
        $builder
            .row_cell("User", None, RowKey::name("A"), "Value", "5")
            .row_formula("User", None, RowKey::name("B"), "Value", "User.A+1", "6")
            .row_formula("User", None, RowKey::name("C"), "Value", "User.B*2", "12")
            .row_formula("User", None, RowKey::name("Stale"), "Value", "2+2", "9")
    };
}

fn drawing() -> Document {
    let master = cells!(MasterBuilder::new()).shape(ShapeBuilder::new(5).cell("Width", "1"));
    let style = cells!(StyleBuilder::new().name("S1"));
    let mut doc = DocBuilder::new().master(1, master).style(1, style).page(1, PageBuilder::new().shape(ShapeBuilder::new(10))).build();
    for (name, text, formula) in [("A", "5", None), ("B", "6", Some("User.A+1")), ("C", "12", Some("User.B*2")), ("Stale", "9", Some("2+2"))] {
        let cell = match formula {
            Some(f) => diacraft_doc::Cell::with_formula(f, Some(DocValue::from_text(text))),
            None => diacraft_doc::Cell::constant(DocValue::from_text(text)),
        };
        std::sync::Arc::make_mut(&mut doc.sheet).set_cell(&user(name), cell).expect("a cell");
    }
    doc
}

fn saved(doc: &Document, sheet: &SheetRef, name: &str) -> Option<String> {
    let found = match sheet {
        SheetRef::Document => doc.sheet.lookup(&user(name)),
        SheetRef::Style(id) => doc.style(*id).expect("a style").sheet.lookup(&user(name)),
        SheetRef::Master(id) => doc.master(*id).expect("a master").sheet.lookup(&user(name)),
        other => panic!("{other:?}"),
    };
    match found {
        Lookup::Found(cell) => cell.value.as_ref().map(|v| v.text().to_owned()),
        other => panic!("{other:?}"),
    }
}

/// A of the sheet is written 8, and the commit is told that cell and no other changed.
#[test]
fn the_cells_of_the_document_a_style_and_a_masters_page_that_read_a_written_cell_follow_it() {
    for sheet in [SheetRef::Document, SheetRef::Style(1), SheetRef::Master(1)] {
        let mut doc = drawing();
        doc.write_cell(&sheet, &user("A"), CellEdit::Value(DocValue::from_text("8")), WriteOrigin::Drawing).expect("a write");
        let changes = Changes { cells: vec![(sheet.clone(), user("A"))], ..Changes::default() };
        let report = commit_changed(&mut doc, &Budget::default(), EvalOptions::default(), 1, &changes);
        assert_eq!(report.written, 2, "{sheet:?}: B and C are written: {report:?}");
        assert_eq!(saved(&doc, &sheet, "B").as_deref(), Some("9"), "{sheet:?}");
        assert_eq!(saved(&doc, &sheet, "C").as_deref(), Some("18"), "{sheet:?}: a cell that reads one that changed follows it in turn");
        assert_eq!(
            saved(&doc, &sheet, "Stale").as_deref(),
            Some("9"),
            "{sheet:?}: a cell that reads nothing that changed keeps the value it was saved with"
        );
    }
}

/// A formula written to a cell of those sheets is worked out itself, whatever value it was written with.
#[test]
fn a_formula_written_to_such_a_sheet_is_worked_out_with_the_cells_that_read_it() {
    for sheet in [SheetRef::Document, SheetRef::Style(1), SheetRef::Master(1)] {
        let mut doc = drawing();
        let cell = diacraft_doc::Cell::with_formula("2+2", Some(DocValue::from_text("0")));
        doc.write_cell(&sheet, &user("A"), CellEdit::Cell(cell), WriteOrigin::Automation).expect("a write");
        let changes = Changes { cells: vec![(sheet.clone(), user("A"))], ..Changes::default() };
        commit_changed(&mut doc, &Budget::default(), EvalOptions::default(), 1, &changes);
        assert_eq!(saved(&doc, &sheet, "A").as_deref(), Some("4"), "{sheet:?}: the cell written");
        assert_eq!(
            (saved(&doc, &sheet, "B").as_deref(), saved(&doc, &sheet, "C").as_deref()),
            (Some("5"), Some("10")),
            "{sheet:?}: and the cells that read it"
        );
    }
}
