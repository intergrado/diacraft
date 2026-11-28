//! `Changes::only_if_read`: a commit for the readers of what an edit wrote after its recalculation (the rows of a connector's route, and the cells of its label).
//!
//! The cells the commit names hold the values that were written to them, so they are not worked out (a route's rows are constants, and a label's cells were put where the route
//! says): what reads them is. A commit that finds nothing to work out is not counted, so an edit on a page that nothing reads a route on makes the one commit it made.

use diacraft_calc::{Budget, Changes, CommitReport, EvalOptions, commit_changed};
use diacraft_doc::{CellAddr, Document, Lookup, Owner, RowKey, SectionKey, SheetRef};
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};

use crate::path_functions::{ln, mv, section};

fn user(name: &str) -> CellAddr {
    CellAddr::in_row("User", None, RowKey::name(name), "Value")
}

fn sheet_of(doc: &Document, id: u32) -> SheetRef {
    SheetRef::Shape(doc.find_shape(Owner::Page(1), id).expect("a shape"))
}

/// The saved `V` text of a user cell of a shape.
fn saved(doc: &Document, id: u32, name: &str) -> String {
    let at = doc.find_shape(Owner::Page(1), id).expect("a shape");
    match doc.shape(&at).expect("a shape").sheet.lookup(&user(name)) {
        Lookup::Found(c) => c.value.as_ref().map(|v| v.text().to_owned()).unwrap_or_default(),
        other => panic!("{other:?}"),
    }
}

fn number(doc: &Document, id: u32, name: &str) -> f64 {
    saved(doc, id, name).parse::<f64>().expect("a number")
}

/// The commit of `changes` on page 1, with the counts of the work started over.
fn commit(doc: &mut Document, changes: &Changes) -> (CommitReport, u64) {
    diacraft_calc::work::reset();
    let report = commit_changed(doc, &Budget::default(), EvalOptions::default(), 1, changes);
    (report, diacraft_calc::work::commits())
}

/// Shape 1 holds `Const`, a value of 5 that was written to it, and `Held`, whose formula gives 2 and whose saved value is 5; shape 2 reads `Const` (`Reads`, saved 0), and a cell
/// reads that (`Twice`, saved 0); shape 3 holds `Alone`, read by nothing (formula 1+1, saved 5), and a path of one leg of three units; shape 4 reads the path of shape 3 (`Len`, saved 0);
/// shape 5 has a path that nothing reads.
fn page() -> Document {
    let one = ShapeBuilder::new(1).row_cell("User", None, RowKey::name("Const"), "Value", "5").row_formula(
        "User",
        None,
        RowKey::name("Held"),
        "Value",
        "1+1",
        "5",
    );
    let two = ShapeBuilder::new(2).row_formula("User", None, RowKey::name("Reads"), "Value", "Sheet.1!User.Const+1", "0").row_formula(
        "User",
        None,
        RowKey::name("Twice"),
        "Value",
        "User.Reads*2",
        "0",
    );
    let three =
        section(ShapeBuilder::new(3), 0, vec![mv(0.0, 0.0), ln(3.0, 0.0)]).row_formula("User", None, RowKey::name("Alone"), "Value", "1+1", "5");
    let four = ShapeBuilder::new(4).row_formula("User", None, RowKey::name("Len"), "Value", "PATHLENGTH(Sheet.3!Geometry1.Path)", "0");
    let five = section(ShapeBuilder::new(5), 0, vec![mv(0.0, 0.0), ln(1.0, 0.0)]);
    DocBuilder::new().page(1, PageBuilder::new().shape(one).shape(two).shape(three).shape(four).shape(five)).build()
}

fn cells(doc: &Document, id: u32, names: &[&str]) -> Changes {
    Changes { cells: names.iter().map(|name| (sheet_of(doc, id), user(name))).collect(), ..Changes::default() }
}

fn geometry(doc: &Document, id: u32) -> Changes {
    Changes { rows: vec![(sheet_of(doc, id), SectionKey::new("Geometry", Some(0)), None)], ..Changes::default() }
}

#[test]
fn the_cells_written_hold_what_was_written_and_what_reads_them_is_worked_out() {
    let mut doc = page();
    let changes = Changes { only_if_read: true, ..cells(&doc, 1, &["Const", "Held"]) };
    let (report, commits) = commit(&mut doc, &changes);
    assert_eq!(saved(&doc, 1, "Held"), "5", "the written cell is not worked out: it keeps the value written to it ({report:?})");
    assert_eq!(saved(&doc, 2, "Reads"), "6", "the reader gives the value it reads, 5, and 1");
    assert_eq!(saved(&doc, 2, "Twice"), "12", "what reads a reader is worked out in its turn, as the reader came out different");
    assert_eq!(commits, 1);
    assert_eq!(report.written, 2, "{report:?}");
}

#[test]
fn without_the_flag_the_cells_written_are_worked_out_too() {
    let mut doc = page();
    let changes = cells(&doc, 1, &["Const", "Held"]);
    let (_, commits) = commit(&mut doc, &changes);
    assert_eq!(saved(&doc, 1, "Held"), "2", "an ordinary commit works the changed cell out");
    assert_eq!((saved(&doc, 2, "Reads"), saved(&doc, 2, "Twice")), ("6".to_owned(), "12".to_owned()));
    assert_eq!(commits, 1);
}

#[test]
fn a_commit_that_finds_nothing_reading_what_was_written_is_not_counted() {
    let mut doc = page();
    let changes = Changes { only_if_read: true, ..cells(&doc, 3, &["Alone"]) };
    let (report, commits) = commit(&mut doc, &changes);
    assert_eq!((commits, report.written), (0, 0), "nothing reads the cell: no commit, and nothing written ({report:?})");
    assert_eq!(saved(&doc, 3, "Alone"), "5", "the cell is not worked out");
    // The same change as an ordinary commit is one commit, and works the cell out.
    let changes = cells(&doc, 3, &["Alone"]);
    let (_, commits) = commit(&mut doc, &changes);
    assert_eq!(commits, 1);
    assert_eq!(saved(&doc, 3, "Alone"), "2");
}

#[test]
fn a_commit_that_names_nothing_is_not_counted() {
    let mut doc = page();
    let before = doc.clone();
    let (report, commits) = commit(&mut doc, &Changes { only_if_read: true, ..Changes::default() });
    assert_eq!((commits, report.written), (0, 0));
    assert_eq!(doc, before);
}

#[test]
fn the_rows_of_a_section_are_read_by_the_functions_that_read_the_whole_section() {
    let mut doc = page();
    let changes = Changes { only_if_read: true, ..geometry(&doc, 3) };
    let (report, commits) = commit(&mut doc, &changes);
    assert_eq!(commits, 1, "{report:?}");
    assert!((number(&doc, 4, "Len") - 3.0).abs() < 1e-9, "the path function reads the three units of the route: {}", saved(&doc, 4, "Len"));
    assert_eq!(saved(&doc, 3, "Alone"), "5", "no cell of the section's shape is worked out");
}

#[test]
fn the_rows_of_a_section_that_nothing_reads_make_no_commit() {
    let mut doc = page();
    let changes = Changes { only_if_read: true, ..geometry(&doc, 5) };
    let (report, commits) = commit(&mut doc, &changes);
    assert_eq!((commits, report.written), (0, 0), "{report:?}");
    assert_eq!(saved(&doc, 4, "Len"), "0", "the reader of another shape's route is left as it was");
}
