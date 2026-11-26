//! Which cell a reference written in a formula names (`Locator`), as the evaluator finds it: the same qualifiers (`Sheet.<id>!`, `ThePage!`, `TheDoc!`,
//! `Pages[..]!`), the same spelling of a row's name in any letter case, and a reference that finds no sheet is none. A command that numbers rows again uses it to
//! tell the formulas that name a row that moves from those that do not, without a second reading of the language.

use diacraft_calc::{Budget, EvalCtx, EvalOptions, Locator};
use diacraft_doc::{CellAddr, Document, Owner, RowKey, ShapeRef, SheetRef};
use diacraft_formula::{Expr, parse};
use diacraft_testkit::build::{DocBuilder, MasterBuilder, PageBuilder, ShapeBuilder};

/// The sheet of the shape with this `ID` on page 1.
fn shape(doc: &Document, id: u32) -> SheetRef {
    SheetRef::Shape(doc.find_shape(Owner::Page(1), id).unwrap())
}

/// The reference a text is, which must be one.
fn reference(text: &str) -> diacraft_formula::CellRef {
    match parse(text).ast.unwrap() {
        Expr::Ref(r) => *r,
        other => panic!("{text} is no reference: {other:?}"),
    }
}

fn document() -> Document {
    DocBuilder::new()
        .master(1, MasterBuilder::new().shape(ShapeBuilder::new(5).cell("Width", "2")))
        .page(
            1,
            PageBuilder::new()
                .cell("PageWidth", "11")
                .shape(ShapeBuilder::new(10).row_cell("Scratch", None, RowKey::Ix(2), "A", "7").row_cell(
                    "Property",
                    None,
                    RowKey::name("Cost"),
                    "Value",
                    "9",
                ))
                .shape(ShapeBuilder::new(11).named("Box", "Box").cell("Width", "3"))
                .shape(ShapeBuilder::new(12).master(1)),
        )
        .page(2, PageBuilder::new().name("Page-2").shape(ShapeBuilder::new(10).cell("Width", "8")))
        .doc_cell("Width", "5")
        .build()
}

fn locate(doc: &Document, from: &SheetRef, text: &str) -> Option<(SheetRef, CellAddr)> {
    let mut ctx = EvalCtx::new(EvalOptions::default(), Budget::default());
    Locator::new(doc, &mut ctx).locate(from, &reference(text))
}

#[test]
fn a_reference_names_a_cell_of_its_own_sheet_a_shape_of_the_page_the_page_and_the_document() {
    let doc = document();
    let ten = shape(&doc, 10);
    assert_eq!(locate(&doc, &ten, "Scratch.A3"), Some((ten.clone(), CellAddr::in_row("Scratch", None, RowKey::Ix(2), "A"))));
    assert_eq!(locate(&doc, &ten, "Width"), Some((ten.clone(), CellAddr::direct("Width"))));
    let eleven = shape(&doc, 11);
    assert_eq!(locate(&doc, &ten, "Sheet.11!Width"), Some((eleven.clone(), CellAddr::direct("Width"))));
    assert_eq!(locate(&doc, &ten, "Box!Width"), Some((eleven, CellAddr::direct("Width"))), "a peer by its name");
    assert_eq!(locate(&doc, &ten, "ThePage!PageWidth"), Some((SheetRef::Page(1), CellAddr::direct("PageWidth"))));
    assert_eq!(locate(&doc, &ten, "TheDoc!Width"), Some((SheetRef::Document, CellAddr::direct("Width"))));
    // Another page, named by its place: its shape 10 is not this page's.
    let other = SheetRef::Shape(doc.find_shape(Owner::Page(2), 10).unwrap());
    assert_eq!(locate(&doc, &ten, "Pages[Page-2]!Sheet.10!Width"), Some((other, CellAddr::direct("Width"))));
}

#[test]
fn a_row_named_in_another_letter_case_is_the_row_the_sheet_holds() {
    let doc = document();
    let ten = shape(&doc, 10);
    assert_eq!(locate(&doc, &ten, "Prop.COST.Value"), Some((ten.clone(), CellAddr::in_row("Property", None, RowKey::name("Cost"), "Value"))), "R63");
    assert_eq!(locate(&doc, &ten, "Prop.Cost"), Some((ten, CellAddr::in_row("Property", None, RowKey::name("Cost"), "Value"))));
}

#[test]
fn a_reference_that_finds_no_sheet_or_names_no_cell_is_none() {
    let doc = document();
    let ten = shape(&doc, 10);
    assert_eq!(locate(&doc, &ten, "Sheet.99!Width"), None);
    assert_eq!(locate(&doc, &ten, "Nobody!Width"), None);
    assert_eq!(locate(&doc, &ten, "Pages[Missing]!Width"), None);
    assert_eq!(locate(&doc, &ten, "Scratch.A[x]"), None, "no cell is named by a row that is no number");
    // A reference with a call before it is chosen when the formula runs: it names no one cell.
    assert_eq!(locate(&doc, &ten, "LISTSHEETREF()!Width"), None);
    // From a sheet that is not there.
    assert_eq!(
        locate(&doc, &SheetRef::Page(9), "Width"),
        Some((SheetRef::Page(9), CellAddr::direct("Width"))),
        "an unqualified name is the sheet's own, there or not"
    );
    assert_eq!(locate(&doc, &SheetRef::Style(9), "Sheet.1!Width"), None, "a style is on no page");
}

#[test]
fn a_locator_keeps_what_it_builds_and_stops_at_its_budget() {
    let doc = document();
    let ten = shape(&doc, 10);
    let mut ctx = EvalCtx::new(EvalOptions::default(), Budget::default());
    let mut locator = Locator::new(&doc, &mut ctx);
    let r = reference("Sheet.11!Width");
    for _ in 0..1000 {
        assert!(locator.locate(&ten, &r).is_some());
    }
    assert!(!locator.exhausted());
    // A budget of nothing: no reference is found and the locator says why.
    let mut ctx = EvalCtx::new(EvalOptions::default(), Budget::with_steps(0));
    let mut locator = Locator::new(&doc, &mut ctx);
    assert_eq!(locator.locate(&ten, &r), None);
    assert!(locator.exhausted());
    // The master's shape has the page's `ThePage`: the master's own page sheet.
    let master_shape = SheetRef::Shape(ShapeRef { owner: Owner::Master(1), path: vec![0] });
    assert_eq!(locate(&doc, &master_shape, "ThePage!PageWidth"), Some((SheetRef::Master(1), CellAddr::direct("PageWidth"))));
}
