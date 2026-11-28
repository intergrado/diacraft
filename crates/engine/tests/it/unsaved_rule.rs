//! One rule for a formula that cannot be worked out (a follow-up of public issue 14): a cell with a formula and no saved value is drawn by what its formula works out
//! to; when the formula gives no value (an error, a function the evaluator lacks, steps that ran out) the picture read the root style's value for a shape's cell that the root style
//! holds, and the readers of the edit commands (`Worked`) read it as saved, which is no value. A cell could be drawn one way and picked, moved or reported another.
//!
//! These tests hold the two to each other: for every kind of failure, the number the commands' reader gives is the number the renderer reads (`diacraft_render::read_cell`).

use diacraft_doc::{Cell, CellAddr, Document, Owner, Resolver, ShapeRef, SheetRef};
use diacraft_engine::draw::worked::{Answer, Worked, WorkedBudget};
use diacraft_render::{CellValue, read_cell, read_cell_within};
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};

const WEIGHT: &str = "LineWeight";
const ROOT_WEIGHT: f64 = 0.0625;

fn shape_sheet(at: u32) -> SheetRef {
    SheetRef::Shape(ShapeRef::top(Owner::Page(1), at))
}

/// A page of one shape whose `LineWeight` and `Width` are `formula` and no value, over a root style that holds a `LineWeight` and no `Width`.
fn doc_of(formula: &str) -> Document {
    let shape = ShapeBuilder::new(1)
        .set(CellAddr::direct(WEIGHT), Cell::with_formula(formula, None))
        .set(CellAddr::direct("Width"), Cell::with_formula(formula, None));
    DocBuilder::new().root(|r| r.cell(WEIGHT, &ROOT_WEIGHT.to_string())).page(1, PageBuilder::new().shape(shape)).build()
}

/// What the commands' reader gives for the cell, and what the renderer reads, as numbers.
fn both(doc: &Document, cell: &str) -> (Option<f64>, Option<f64>) {
    let addr = CellAddr::direct(cell);
    let commands = Worked::new(doc).number_at(&Resolver::new(doc), &shape_sheet(0), &addr);
    let drawn = match read_cell(doc, &shape_sheet(0), &addr) {
        CellValue::Num(n) => Some(n),
        _ => None,
    };
    (commands, drawn)
}

#[test]
fn a_formula_that_fails_is_read_by_the_commands_as_the_picture_reads_it_the_root_styles_value() {
    for (what, formula) in [("an error", "1/0"), ("a function the evaluator lacks", "NOSUCHFUNCTION(1)"), ("a bad reference", "Sheet.99!PinX")] {
        let doc = doc_of(formula);
        let (commands, drawn) = both(&doc, WEIGHT);
        assert_eq!(drawn, Some(ROOT_WEIGHT), "{what}: the picture reads the root style's weight");
        assert_eq!(commands, drawn, "{what}: the commands read what the picture reads");
    }
}

#[test]
fn the_two_agree_on_every_kind_of_answer_a_formula_gives() {
    // A number is the number; a text, a colour and `#N/A` are no number, and nothing stands in for them (the picture reads the caller's default).
    for (what, formula, want) in [
        ("a number", "1+1", Some(2.0)),
        ("a truth value", "TRUE", Some(1.0)),
        ("a text", "\"abc\"", None),
        ("a colour", "RGB(1,2,3)", None),
        ("no value", "NA()", None),
    ] {
        let doc = doc_of(formula);
        let (commands, drawn) = both(&doc, WEIGHT);
        assert_eq!(drawn, want, "{what}: the picture");
        assert_eq!(commands, drawn, "{what}: the commands");
    }
}

#[test]
fn a_cell_the_root_style_does_not_hold_reads_no_value_in_both() {
    for formula in ["1/0", "NOSUCHFUNCTION(1)"] {
        let doc = doc_of(formula);
        assert_eq!(both(&doc, "Width"), (None, None), "{formula}");
    }
}

#[test]
fn steps_that_run_out_read_the_cell_by_the_same_rule_in_both() {
    // The picture with no steps to work a cell out reads the root's value (and says so); so does a reader of the commands that has none.
    let doc = doc_of("1+1");
    let drawn = read_cell_within(&doc, &shape_sheet(0), &CellAddr::direct(WEIGHT), 0);
    assert_eq!(drawn, CellValue::Num(ROOT_WEIGHT), "the picture with no steps");
    let budget = WorkedBudget::new(0);
    let _scope = budget.enter();
    let commands = Worked::new(&doc).number_at(&Resolver::new(&doc), &shape_sheet(0), &CellAddr::direct(WEIGHT));
    assert_eq!(commands, Some(ROOT_WEIGHT), "the commands with no steps");
}

#[test]
fn the_answer_of_a_formula_is_told_apart_from_no_answer() {
    let (colour, empty, failed) = (doc_of("RGB(1,2,3)"), doc_of("NA()"), doc_of("1/0"));
    let addr = CellAddr::direct(WEIGHT);
    assert_eq!(Worked::new(&colour).answer(&shape_sheet(0), &addr), Some(Answer::Colour(1, 2, 3)));
    assert_eq!(Worked::new(&empty).answer(&shape_sheet(0), &addr), Some(Answer::Empty));
    assert_eq!(Worked::new(&failed).answer(&shape_sheet(0), &addr), None);
}
