//! `Changes::only_if_read` with a section written whole (`Changes::sections`): **a cell that reads one cell of the section is worked out, as a function that reads the whole of it is.** A command that writes a route
//! replaces the rows of the connector's `Geometry` section whole, and a cell that names one of them (`Sheet.6!Geometry1.X1`, a row's `Y`, or a cell of the connector itself that reads its own
//! `Geometry1.X2`) reads a cell the commit did not name. The rows that are not there any more, and the ones that are new, are read as a full evaluation reads them.

use diacraft_calc::{Budget, Changes, CommitReport, EvalOptions, commit_changed, recalc_all_with};
use diacraft_doc::{CellAddr, Document, Lookup, Owner, RowKey, SectionKey, SheetRef};
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};

use crate::path_functions::{ln, mv, section};

fn user(name: &str) -> CellAddr {
    CellAddr::in_row("User", None, RowKey::name(name), "Value")
}

fn sheet_of(doc: &Document, id: u32) -> SheetRef {
    SheetRef::Shape(doc.find_shape(Owner::Page(1), id).expect("a shape"))
}

/// The saved `V` text of a user cell of a shape, and its error.
fn saved(doc: &Document, id: u32, name: &str) -> String {
    let at = doc.find_shape(Owner::Page(1), id).expect("a shape");
    match doc.shape(&at).expect("a shape").sheet.lookup(&user(name)) {
        Lookup::Found(c) => match (&c.error, &c.value) {
            (Some(error), _) => format!("error {error:?}"),
            (None, Some(v)) => v.text().to_owned(),
            (None, None) => String::new(),
        },
        other => panic!("{other:?}"),
    }
}

/// The commit of `changes` on page 1, with the counts of the work started over: → the report and the commits counted.
fn commit(doc: &mut Document, changes: &Changes) -> (CommitReport, u64) {
    diacraft_calc::work::reset();
    let report = commit_changed(doc, &Budget::default(), EvalOptions::default(), 1, changes);
    (report, diacraft_calc::work::commits())
}

/// The rows of the `Geometry` section of shape `id` written whole, and what reads them is to be worked out, and nothing else.
fn route_written(doc: &Document, id: u32) -> Changes {
    Changes { sections: vec![(sheet_of(doc, id), SectionKey::new("Geometry", Some(0)))], only_if_read: true, ..Changes::default() }
}

/// The commit of the rows of shape `id` written whole.
fn commit_route(doc: &mut Document, id: u32) -> (CommitReport, u64) {
    let changes = route_written(doc, id);
    commit(doc, &changes)
}

/// The cells of `ids` that a full evaluation of the document changes, as `shape cell`.
fn stale(doc: &Document, ids: &[u32]) -> Vec<String> {
    let report = recalc_all_with(doc, &Budget::default(), EvalOptions::default());
    report
        .differed
        .iter()
        .filter(|x| ids.iter().any(|id| x.sheet == format!("page:1/shape:{id}")))
        .map(|x| format!("{} {} saved {:?} actual {:?}", x.sheet, x.cell, x.saved, x.actual))
        .collect()
}

/// Shape 1 is a route of three rows, (0, 0), (3, 0) and (3, 4), which holds `Own`, a cell that reads its own second row (`Geometry1.Y2`, saved 0). Shape 2 reads one cell of it each:
/// `First` the `X` of row 1 (saved 0), `Last` the `Y` of row 2 (saved 0), `Twice` a cell that reads `First` (saved 0) and `Gone` a row the route does not have, `Geometry1.X9` (saved 7,
/// as it was when the route had more rows). Shape 3 is a route nothing reads, with `Alone`, a formula that gives 2 and a saved value of 5, which nothing here works out.
fn page() -> Document {
    let route = section(ShapeBuilder::new(1), 0, vec![mv(0.0, 0.0), ln(3.0, 0.0), ln(3.0, 4.0)]).row_formula(
        "User",
        None,
        RowKey::name("Own"),
        "Value",
        "Geometry1.Y2",
        "0",
    );
    let reads = ShapeBuilder::new(2)
        .row_formula("User", None, RowKey::name("First"), "Value", "Sheet.1!Geometry1.X1", "0")
        .row_formula("User", None, RowKey::name("Last"), "Value", "Sheet.1!Geometry1.Y2", "0")
        .row_formula("User", None, RowKey::name("Twice"), "Value", "User.First*2", "0")
        .row_formula("User", None, RowKey::name("Gone"), "Value", "Sheet.1!Geometry1.X9", "7");
    let alone =
        section(ShapeBuilder::new(3), 0, vec![mv(0.0, 0.0), ln(1.0, 0.0)]).row_formula("User", None, RowKey::name("Alone"), "Value", "1+1", "5");
    DocBuilder::new().page(1, PageBuilder::new().shape(route).shape(reads).shape(alone)).build()
}

#[test]
fn a_cell_that_reads_one_cell_of_the_rows_written_is_worked_out_with_what_reads_it() {
    let mut doc = page();
    let (report, commits) = commit_route(&mut doc, 1);
    assert_eq!(saved(&doc, 2, "First"), "3", "`Sheet.1!Geometry1.X1` reads the 3 of the second row ({report:?})");
    assert_eq!(saved(&doc, 2, "Last"), "4", "`Sheet.1!Geometry1.Y2` reads the 4 of the third row");
    assert_eq!(saved(&doc, 2, "Twice"), "6", "what reads a reader is worked out in its turn");
    assert_eq!(commits, 1);
}

#[test]
fn a_cell_of_the_connector_that_reads_its_own_row_is_worked_out_with_the_rows() {
    let mut doc = page();
    let (report, _) = commit_route(&mut doc, 1);
    assert_eq!(saved(&doc, 1, "Own"), "4", "`Geometry1.Y2` of its own sheet ({report:?})");
}

#[test]
fn a_reader_of_a_row_the_route_does_not_have_holds_what_a_full_evaluation_gives() {
    let mut doc = page();
    commit_route(&mut doc, 1);
    assert_ne!(saved(&doc, 2, "Gone"), "7", "the row is not there: the reader does not keep the value of the row that was");
    assert_eq!(stale(&doc, &[1, 2]), Vec::<String>::new(), "every reader holds what a full evaluation gives");
}

#[test]
fn a_cell_that_reads_one_cell_of_a_route_nothing_names_is_left_as_it_was() {
    let mut doc = page();
    let before = doc.clone();
    let (report, commits) = commit_route(&mut doc, 3);
    // What it cost before a reader of one cell was followed (measured on the code that listed a cell's readers by the cell alone): the page's graph, the one place followed, and nothing worked out.
    // (39 on the code, and one more on the merged code: the cross-page work of looks at what names the page in every commit.)
    assert_eq!(report.stats.steps, 40, "a route nothing reads costs no more steps than it did ({report:?})");
    assert_eq!((commits, report.written), (0, 0), "nothing reads a cell of shape 3's rows: no commit ({report:?})");
    assert_eq!(doc, before);
}

#[test]
fn the_cells_that_read_nothing_of_the_rows_written_are_not_worked_out() {
    let mut doc = page();
    commit_route(&mut doc, 1);
    assert_eq!(saved(&doc, 3, "Alone"), "5", "a cell that reads nothing of the route keeps the value it had, whatever its formula gives");
}
