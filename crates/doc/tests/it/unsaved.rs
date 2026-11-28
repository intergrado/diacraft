//! A cell with a formula and no value saved anywhere on the way to it: the model says so ([`Resolved::holds_unsaved_formula`]), and says whether such a
//! cell, or one that holds the theme's marker, follows the theme ([`Resolved::reads_theme`]).

use diacraft_doc::{Cell, CellAddr, Document, Owner, Resolved, Resolver, ShapeRef, SheetRef, Value};
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder, StyleBuilder};

fn top() -> SheetRef {
    SheetRef::Shape(ShapeRef { owner: Owner::Page(1), path: vec![0] })
}

fn with<R>(doc: &Document, cell: &str, f: impl FnOnce(&Resolved<'_>) -> R) -> R {
    let found = Resolver::new(doc).resolve(&top(), &CellAddr::direct(cell)).expect("the cell resolves");
    f(&found)
}

/// One shape that links the style 3 for all three families, holding `own` for `Fill`, and the style holding `style` for it.
fn doc_of(own: Option<Cell>, style: Option<Cell>) -> Document {
    let mut st = StyleBuilder::new().name("Theme");
    if let Some(cell) = style {
        st = st.set(CellAddr::direct("FillForegnd"), cell);
    }
    let mut shape = ShapeBuilder::new(1).links(Some(3), Some(3), Some(3));
    if let Some(cell) = own {
        shape = shape.set(CellAddr::direct("FillForegnd"), cell);
    }
    DocBuilder::new().style(3, st).page(1, PageBuilder::new().shape(shape)).build()
}

fn marker(formula: &str) -> Cell {
    Cell::with_formula(formula, Some(Value::from_text("Themed")))
}

const CELL: &str = "FillForegnd";

#[test]
fn a_formula_with_no_value_anywhere_on_the_way_is_unsaved_whether_it_is_the_shapes_own_or_a_styles() {
    let unsaved = || Cell::with_formula("1+1", None);
    for doc in [doc_of(Some(unsaved()), None), doc_of(None, Some(unsaved())), doc_of(Some(Cell::inherited(None)), Some(unsaved()))] {
        assert!(with(&doc, CELL, |r| r.holds_unsaved_formula()));
    }
}

#[test]
fn a_cell_with_a_value_a_marker_or_no_formula_is_not() {
    let saved = Cell::with_formula("1+1", Some(Value::from_text("2")));
    let cases = [
        ("a value beside the formula", doc_of(Some(saved.clone()), None)),
        ("a value the style saved, found by an `Inh` cell", doc_of(Some(Cell::inherited(None)), Some(saved))),
        ("the marker", doc_of(Some(marker("THEMEVAL()")), None)),
        ("a constant", doc_of(Some(Cell::constant(Value::from_text("2"))), None)),
        ("no cell at all", doc_of(None, None)),
    ];
    for (what, doc) in cases {
        assert!(!with(&doc, CELL, |r| r.holds_unsaved_formula()), "{what}");
    }
}

#[test]
fn a_cell_follows_the_theme_when_it_holds_the_marker_or_a_theme_call_with_no_value() {
    let theme = |formula: &str| Cell::with_formula(formula, None);
    let follows = [
        ("the marker", doc_of(Some(marker("THEMEVAL()")), None)),
        ("the marker beside any formula", doc_of(Some(marker("RGB(1,2,3)")), None)),
        ("the marker found on the style", doc_of(Some(Cell::inherited(None)), Some(marker("THEMEVAL()")))),
        ("a call with no value", doc_of(Some(theme("THEMEVAL()")), None)),
        ("a call with no value, on the style", doc_of(None, Some(theme("THEMEVAL()")))),
        ("a call with no value, on the style, behind an `Inh` cell", doc_of(Some(Cell::inherited(None)), Some(theme("THEMEVAL()")))),
        ("a guarded call", doc_of(None, Some(theme("THEMEGUARD(THEMEVAL(\"AccentColor\"))")))),
        ("a tint of a call", doc_of(None, Some(theme("MSOTINT(THEMEVAL(\"Dark\"),45)")))),
    ];
    for (what, doc) in follows {
        assert!(with(&doc, CELL, |r| r.reads_theme()), "{what}");
    }
    let does_not = [
        ("a colour formula with no value", doc_of(Some(theme("RGB(1,2,3)")), None)),
        ("a call with a value saved beside it", doc_of(Some(Cell::with_formula("THEMEVAL()", Some(Value::from_text("#112233")))), None)),
        ("a constant", doc_of(Some(Cell::constant(Value::from_text("2"))), None)),
        ("no cell", doc_of(None, None)),
        ("a formula that does not parse", doc_of(Some(theme("THEMEVAL(")), None)),
    ];
    for (what, doc) in does_not {
        assert!(!with(&doc, CELL, |r| r.reads_theme()), "{what}");
    }
}

// ---- what a cell reads as when its formula gives no value ----------------------------------------------------------------------

/// A page with one shape, and `own` as its `LineWeight` (none when `None`), over a root style whose `LineWeight` is `root` (none when `None`).
fn weighted(own: Option<Cell>, root: Option<&str>) -> Document {
    let mut shape = ShapeBuilder::new(1);
    if let Some(cell) = own {
        shape = shape.set(CellAddr::direct("LineWeight"), cell);
    }
    DocBuilder::new()
        .root(|r| match root {
            Some(weight) => r.cell("LineWeight", weight),
            None => r,
        })
        .page(1, PageBuilder::new().shape(shape))
        .build()
}

fn standin(doc: &Document, at: &SheetRef) -> Option<String> {
    let res = Resolver::new(doc);
    let addr = CellAddr::direct("LineWeight");
    let found = res.resolve(at, &addr)?;
    let root = res.root_standin(at, &addr, &found)?;
    Some(root.value.cell?.value.as_ref()?.text().to_owned())
}

#[test]
fn a_shapes_cell_that_holds_no_value_anywhere_on_the_way_reads_the_root_styles_cell_which_is_what_a_formula_that_gives_none_reads_as() {
    // A formula and no value (the answer of the formula is the caller's to find), or no cell at all: the root style's value stands in.
    for (what, own) in [("a formula and no value", Some(Cell::with_formula("1/0", None))), ("no cell", None)] {
        assert_eq!(standin(&weighted(own, Some("0.02")), &top()).as_deref(), Some("0.02"), "{what}");
    }
}

#[test]
fn nothing_stands_in_for_a_cell_that_holds_a_value_or_that_the_root_style_does_not_hold_or_that_is_not_a_shapes() {
    let saved = Cell::with_formula("1/0", Some(Value::from_text("0.5")));
    assert_eq!(standin(&weighted(Some(saved), Some("0.02")), &top()), None, "a value is saved: it stands as saved");
    assert_eq!(standin(&weighted(Some(Cell::with_formula("1/0", None)), None), &top()), None, "the root style holds no such cell");
    let page = SheetRef::Page(1);
    let doc = DocBuilder::new()
        .root(|r| r.cell("LineWeight", "0.02"))
        .page(1, PageBuilder::new().set(CellAddr::direct("LineWeight"), Cell::with_formula("1/0", None)))
        .build();
    assert_eq!(standin(&doc, &page), None, "only a shape's cell reads the root style: the page's own reads as saved");
}
