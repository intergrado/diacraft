//! The ShapeSheet shows a cell that holds a formula and no saved value as the formula works out, as the picture draws it, and not as the default of
//! its kind (`0.0000 in`, nothing): the window of the sheet and `sheet.cell` read the same way.

use diacraft_doc::{Cell, CellAddr, Document, RowKey};
use diacraft_engine::Session;
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};
use serde_json::{Value, json};

use crate::{blank_session, run};

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

fn user(name: &str) -> CellAddr {
    CellAddr::in_row("User", None, RowKey::name(name), "Value")
}

fn unsaved(formula: &str) -> Cell {
    Cell::with_formula(formula, None)
}

fn shape() -> ShapeBuilder {
    ShapeBuilder::new(10)
        .cell("Width", "2")
        .set(CellAddr::direct("Height"), unsaved("Width*1.5"))
        .set(CellAddr::direct("FillForegnd"), unsaved("RGB(10,20,30)"))
        .set(user("A"), unsaved("2+3"))
        .set(user("B"), unsaved("User.A+1"))
        .set(user("Broken"), unsaved("1/0"))
        .set(user("Chain"), unsaved("NOSUCHFUNCTION(1)"))
}

fn cell_of(s: &mut Session, name: &str) -> Value {
    run(s, "sheet.cell", json!({"id": 10, "cell": name}))
}

#[test]
fn a_length_that_a_formula_gives_and_no_value_is_shown_as_the_length() {
    let mut s = one_shape(shape());
    let c = cell_of(&mut s, "Height");
    assert_eq!(c["value"], json!(3.0), "{c}");
    assert_eq!(c["shown"], json!("3.0000 in"), "{c}");
    assert_eq!(c["formula"], json!("Width*1.5"), "the formula is the file's, and stays");
}

#[test]
fn a_colour_and_a_cell_that_reads_a_cell_with_no_value_are_shown_worked_out() {
    let mut s = one_shape(shape());
    let fill = cell_of(&mut s, "FillForegnd");
    assert_eq!(fill["shown"], json!("RGB(10, 20, 30)"), "{fill}");
    // `User.B` reads `User.A`, which has no value either: 5 and then 6, and not 0 and then 1.
    let b = cell_of(&mut s, "User.B");
    assert_eq!(b["value"], json!(6.0), "{b}");
}

#[test]
fn a_formula_that_gives_an_error_shows_the_error_and_one_the_evaluator_lacks_shows_what_it_showed() {
    let mut s = one_shape(shape());
    let broken = cell_of(&mut s, "User.Broken");
    assert_eq!(broken["shown"], json!("#DIV/0!"), "{broken}");
    // A function this evaluator does not have is no result: the cell shows as it did before (and says it was not worked out).
    let lacking = cell_of(&mut s, "User.Chain");
    assert_eq!(lacking["worked_out"], json!(false), "{lacking}");
    assert!(!lacking["shown"].as_str().unwrap().contains("??"), "{lacking}");
}

#[test]
fn the_window_reads_a_sheet_with_the_same_values_as_the_cell_does() {
    let mut s = one_shape(shape());
    let read = run(&mut s, "sheet.read", json!({"id": 10, "count": 2000}));
    let text = read.to_string();
    assert!(text.contains("3.0000 in"), "the height is shown worked out: {text}");
}
