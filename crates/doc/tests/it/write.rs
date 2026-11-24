//! Writing a cell: the origin of a write and what a guard does to it, and what a write copies.

use std::sync::Arc;

use diacraft_doc::{
    Cell, CellAddr, CellEdit, DocError, Document, FormulaSlot, Origin, Owner, Resolver, RowKey, ShapeRef, SheetRef, Source, Value, WriteOrigin,
    WriteOutcome,
};
use diacraft_testkit::build::{DocBuilder, MasterBuilder, PageBuilder, ShapeBuilder, StyleBuilder};

fn shape(path: &[u32]) -> SheetRef {
    SheetRef::Shape(ShapeRef { owner: Owner::Page(1), path: path.to_vec() })
}

fn formula(source: &str) -> CellEdit {
    CellEdit::Formula { formula: Arc::new(diacraft_formula_parse(source)), value: None }
}

fn diacraft_formula_parse(source: &str) -> diacraft_doc::Formula {
    match Cell::with_formula(source, None).formula {
        FormulaSlot::Local(f) => (*f).clone(),
        other => panic!("{other:?}"),
    }
}

fn local<'a>(doc: &'a Document, at: &SheetRef, name: &str) -> Option<&'a Cell> {
    Resolver::new(doc).sheet(at).and_then(|s| s.cell(name))
}

fn formula_of(doc: &Document, at: &SheetRef, name: &str) -> Option<String> {
    local(doc, at, name).and_then(|c| c.formula.text()).map(str::to_owned)
}

fn value_of(doc: &Document, at: &SheetRef, name: &str) -> Option<String> {
    local(doc, at, name).and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned())
}

fn write(doc: &mut Document, at: &SheetRef, name: &str, edit: CellEdit, origin: WriteOrigin) -> WriteOutcome {
    doc.write_cell(at, &CellAddr::direct(name), edit, origin).unwrap()
}

fn doc_with(shape: ShapeBuilder) -> Document {
    DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build()
}

#[test]
fn writing_a_formula_makes_it_local_and_stops_later_edits_reaching_the_cell() {
    // An instance with no local cell reads the master's formula; once it writes one, that is local.
    let mut doc = DocBuilder::new()
        .master(1, MasterBuilder::new().shape(ShapeBuilder::new(5).formula("LocPinX", "Width*0.5", "1")))
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).master(1)))
        .build();
    let at = shape(&[0]);
    assert!(matches!(Resolver::new(&doc).resolve(&at, &CellAddr::direct("LocPinX")).unwrap().formula.source, Source::Master(_)));
    assert_eq!(write(&mut doc, &at, "LocPinX", formula("1 in"), WriteOrigin::Automation), WriteOutcome::Written);
    let r = Resolver::new(&doc).resolve(&at, &CellAddr::direct("LocPinX")).unwrap();
    assert_eq!(r.formula.source, Source::Local);
    assert_eq!(formula_of(&doc, &at, "LocPinX").as_deref(), Some("1 in"));
    assert_eq!(local(&doc, &at, "LocPinX").map(Cell::origin), Some(Origin::Local));
}

#[test]
fn a_formula_write_keeps_the_unit_and_the_old_value_until_an_evaluation_gives_a_new_one() {
    let mut doc = doc_with(ShapeBuilder::new(10).set(CellAddr::direct("Width"), Cell::constant(Value::from_text("2")).with_unit("MM")));
    let at = shape(&[0]);
    write(&mut doc, &at, "Width", formula("Height*2"), WriteOrigin::Automation);
    assert_eq!(value_of(&doc, &at, "Width").as_deref(), Some("2"));
    assert_eq!(local(&doc, &at, "Width").and_then(|c| c.unit.as_deref()), Some("MM"));
    let evaluated = CellEdit::Formula { formula: Arc::new(diacraft_formula_parse("Height*2")), value: Some(Value::from_text("8")) };
    write(&mut doc, &at, "Width", evaluated, WriteOrigin::Automation);
    assert_eq!(value_of(&doc, &at, "Width").as_deref(), Some("8"));
}

#[test]
fn a_guard_protects_its_own_cell_and_not_the_others() {
    // `Width` formula `GUARD(2 in)` and `PinX` formula `3 in`, a corner drag to 4 inches wide: Width kept, PinX replaced. A
    // shape-wide guard would keep both.
    let mut doc = doc_with(ShapeBuilder::new(10).formula("Width", "GUARD(2 in)", "2").formula("PinX", "3 in", "3"));
    let at = shape(&[0]);
    let drag = |doc: &mut Document, name: &str, v: &str| write(doc, &at, name, CellEdit::Value(Value::from_text(v)), WriteOrigin::Drawing);
    assert_eq!(drag(&mut doc, "Width", "4"), WriteOutcome::KeptGuarded);
    assert_eq!(drag(&mut doc, "PinX", "4"), WriteOutcome::Written);
    assert_eq!(formula_of(&doc, &at, "Width").as_deref(), Some("GUARD(2 in)"));
    assert_eq!(value_of(&doc, &at, "Width").as_deref(), Some("2"));
    assert_eq!(formula_of(&doc, &at, "PinX"), None, "the drag replaced the formula with the value");
    assert_eq!(value_of(&doc, &at, "PinX").as_deref(), Some("4"));
}

#[test]
fn each_kind_of_write_against_a_guarded_cell() {
    // Given `Width` `GUARD(2 in)`: a plain formula write leaves the formula; a forced write replaces it; SETF replaces it; a
    // `ResultIU` write of 5 leaves Width at 2 and is reported.
    let guarded = || doc_with(ShapeBuilder::new(10).formula("Width", "GUARD(2 in)", "2"));
    let at = shape(&[0]);

    let mut doc = guarded();
    assert_eq!(write(&mut doc, &at, "Width", formula("3 in"), WriteOrigin::Automation), WriteOutcome::KeptGuarded);
    assert_eq!(formula_of(&doc, &at, "Width").as_deref(), Some("GUARD(2 in)"));

    let mut doc = guarded();
    assert_eq!(write(&mut doc, &at, "Width", formula("3 in"), WriteOrigin::AutomationForce), WriteOutcome::Written);
    assert_eq!(formula_of(&doc, &at, "Width").as_deref(), Some("3 in"));

    let mut doc = guarded();
    assert_eq!(write(&mut doc, &at, "Width", formula("3 in"), WriteOrigin::Setf), WriteOutcome::Written);
    assert_eq!(formula_of(&doc, &at, "Width").as_deref(), Some("3 in"));

    let mut doc = guarded();
    assert_eq!(write(&mut doc, &at, "Width", CellEdit::Value(Value::from_text("5")), WriteOrigin::Automation), WriteOutcome::KeptGuarded);
    assert_eq!(value_of(&doc, &at, "Width").as_deref(), Some("2"), "Width stays at 2");
    assert_eq!(formula_of(&doc, &at, "Width").as_deref(), Some("GUARD(2 in)"));

    // A forced value write goes through.
    let mut doc = guarded();
    assert_eq!(write(&mut doc, &at, "Width", CellEdit::Value(Value::from_text("5")), WriteOrigin::AutomationForce), WriteOutcome::Written);
    assert_eq!(value_of(&doc, &at, "Width").as_deref(), Some("5"));
}

#[test]
fn a_data_refresh_skips_a_guarded_cell_and_writes_an_unguarded_one() {
    let mut doc = doc_with(ShapeBuilder::new(10).formula("Width", "GUARD(2 in)", "2").formula("Height", "1 in", "1"));
    let at = shape(&[0]);
    assert_eq!(write(&mut doc, &at, "Width", CellEdit::Value(Value::from_text("9")), WriteOrigin::DataRefresh), WriteOutcome::Skipped);
    assert_eq!(write(&mut doc, &at, "Height", CellEdit::Value(Value::from_text("9")), WriteOrigin::DataRefresh), WriteOutcome::Written);
    assert_eq!(value_of(&doc, &at, "Width").as_deref(), Some("2"));
    assert_eq!(value_of(&doc, &at, "Height").as_deref(), Some("9"));
}

#[test]
fn a_guard_the_cell_inherits_protects_the_instance() {
    // R44 (our rule): an inherited GUARD protects the inheritor. The instance holds no cell; a drag finds the master's guarded `Width`
    // and changes nothing, and writes no cell.
    let mut doc = DocBuilder::new()
        .master(1, MasterBuilder::new().shape(ShapeBuilder::new(5).formula("Width", "GUARD(2 in)", "2")))
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).master(1)))
        .build();
    let at = shape(&[0]);
    let before = doc.clone();
    assert_eq!(write(&mut doc, &at, "Width", CellEdit::Value(Value::from_text("4")), WriteOrigin::Drawing), WriteOutcome::KeptGuarded);
    assert_eq!(doc, before, "nothing changed");
    // A forced write is made, and is local.
    assert_eq!(write(&mut doc, &at, "Width", CellEdit::Value(Value::from_text("4")), WriteOrigin::AutomationForce), WriteOutcome::Written);
    assert_eq!(value_of(&doc, &at, "Width").as_deref(), Some("4"));
}

#[test]
fn applying_a_style_removes_unguarded_local_formulas_and_leaves_guarded_ones() {
    // Local `LineWeight` formula `1 pt` and `LineColor` formula `GUARD(23)`; applying a line style: LineWeight goes back to the style
    // and LineColor stays guarded at 23.
    let mut doc = DocBuilder::new()
        .style(1, StyleBuilder::new().cell("LineWeight", "0.05").cell("LineColor", "1"))
        .page(
            1,
            PageBuilder::new().shape(ShapeBuilder::new(10).links(Some(1), None, None).formula("LineWeight", "1 pt", "0.0139").formula(
                "LineColor",
                "GUARD(23)",
                "23",
            )),
        )
        .build();
    let at = shape(&[0]);
    assert_eq!(write(&mut doc, &at, "LineWeight", CellEdit::ClearFormula, WriteOrigin::StyleApply), WriteOutcome::Written);
    assert_eq!(write(&mut doc, &at, "LineColor", CellEdit::ClearFormula, WriteOrigin::StyleApply), WriteOutcome::KeptGuarded);
    assert_eq!(local(&doc, &at, "LineWeight").map(|c| c.formula.clone()), Some(FormulaSlot::Inherit), "back to the style");
    assert_eq!(formula_of(&doc, &at, "LineColor").as_deref(), Some("GUARD(23)"));
    let r = Resolver::new(&doc).resolve(&at, &CellAddr::direct("LineWeight")).unwrap();
    assert!(matches!(r.formula.source, Source::Style { id: 1, .. }), "the style's cell is the formula now: {:?}", r.formula.source);
}

#[test]
fn clearing_a_formula_reads_no_formula_or_goes_back_to_what_it_inherits() {
    // A `Scratch` cell with a local formula and no master row: clearing it reads `No Formula`. An instance's `Width` over a master formula:
    // clearing brings the master's formula back, with the local formula gone (LOCALFORMULAEXISTS 1 to 0).
    let mut doc = DocBuilder::new()
        .master(1, MasterBuilder::new().shape(ShapeBuilder::new(5).formula("Width", "Height*2", "4")))
        .page(
            1,
            PageBuilder::new().shape(ShapeBuilder::new(10).master(1).formula("Width", "3 in", "3").row_formula(
                "Scratch",
                None,
                RowKey::Ix(0),
                "X",
                "Width*0.5",
                "1.5",
            )),
        )
        .build();
    let at = shape(&[0]);
    let scratch = CellAddr::in_row("Scratch", None, RowKey::Ix(0), "X");
    assert_eq!(doc.write_cell(&at, &scratch, CellEdit::ClearFormula, WriteOrigin::Automation), Ok(WriteOutcome::Written));
    let r = Resolver::new(&doc).resolve(&at, &scratch).unwrap();
    assert_eq!(r.formula.cell.map(|c| c.formula.clone()), Some(FormulaSlot::NoFormula));
    assert_eq!(r.value.cell.and_then(|c| c.value.as_ref()).map(Value::text), Some("1.5"));

    assert!(local(&doc, &at, "Width").is_some_and(Cell::owns_formula));
    write(&mut doc, &at, "Width", CellEdit::ClearFormula, WriteOrigin::Automation);
    assert!(!local(&doc, &at, "Width").is_some_and(Cell::owns_formula), "the local formula is gone");
    let r = Resolver::new(&doc).resolve(&at, &CellAddr::direct("Width")).unwrap();
    assert!(matches!(r.formula.source, Source::Master(_)));
    assert_eq!(r.formula.cell.and_then(|c| c.formula.text()), Some("Height*2"));
}

#[test]
fn clearing_a_cell_that_is_not_there_changes_nothing() {
    let mut doc = doc_with(ShapeBuilder::new(10));
    let before = doc.clone();
    assert_eq!(write(&mut doc, &shape(&[0]), "Width", CellEdit::ClearFormula, WriteOrigin::Automation), WriteOutcome::Written);
    assert_eq!(doc, before);
}

#[test]
fn a_write_in_a_row_adds_the_row_and_section_to_the_sheet() {
    // The model's half: an instance with no Geometry section gets a local section with the row and the cell.
    let mut doc = DocBuilder::new()
        .master(1, MasterBuilder::new().shape(ShapeBuilder::new(5).row_formula("Geometry", Some(0), RowKey::Ix(2), "X", "Width*0.5", "1")))
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).master(1).cell("Width", "6")))
        .build();
    let at = shape(&[0]);
    let addr = CellAddr::in_row("Geometry", Some(0), RowKey::Ix(2), "X");
    let cached = Cell::inherited(Some(Value::from_text("3")));
    assert_eq!(doc.write_cell(&at, &addr, CellEdit::Cell(cached), WriteOrigin::Automation), Ok(WriteOutcome::Written));
    let sheet = doc.sheet_of(&at).unwrap();
    assert_eq!(sheet.sections.len(), 1);
    let r = Resolver::new(&doc).resolve(&at, &addr).unwrap();
    assert_eq!((r.value.source.clone(), r.value.cell.and_then(|c| c.value.as_ref()).map(Value::text)), (Source::Local, Some("3")));
    assert!(matches!(r.formula.source, Source::Master(_)), "the formula is still the master's: no text was copied down");
    assert_eq!(r.formula.cell.and_then(|c| c.formula.text()), Some("Width*0.5"));
}

#[test]
fn writes_reach_every_kind_of_sheet() {
    let mut doc = DocBuilder::new().style(1, StyleBuilder::new()).master(1, MasterBuilder::new()).page(1, PageBuilder::new()).build();
    for at in [SheetRef::Document, SheetRef::Style(1), SheetRef::Page(1), SheetRef::Master(1)] {
        assert_eq!(write(&mut doc, &at, "Mine", CellEdit::Value(Value::from_text("1")), WriteOrigin::Automation), WriteOutcome::Written, "{at:?}");
        assert_eq!(value_of(&doc, &at, "Mine").as_deref(), Some("1"), "{at:?}");
    }
}

#[test]
fn a_write_to_a_sheet_that_is_not_there_is_an_error_and_changes_nothing() {
    let mut doc = doc_with(ShapeBuilder::new(10));
    let before = doc.clone();
    for at in [shape(&[3]), shape(&[0, 0]), SheetRef::Style(9), SheetRef::Page(9), SheetRef::Master(9), shape(&[u32::MAX]), shape(&vec![0; 500])] {
        let e = doc.write_cell(&at, &CellAddr::direct("Width"), CellEdit::Value(Value::from_text("1")), WriteOrigin::Automation);
        assert_eq!(e, Err(DocError::NoSuchSheet), "{:?}", at);
    }
    assert_eq!(doc, before);
}

#[test]
fn a_refused_write_changes_nothing() {
    let mut doc = doc_with(ShapeBuilder::new(10));
    let before = doc.clone();
    let e = doc.write_cell(&shape(&[0]), &CellAddr::direct(""), CellEdit::Value(Value::from_text("1")), WriteOrigin::Automation);
    assert_eq!(e, Err(DocError::BadName("a cell name")));
    assert_eq!(doc, before);
}

#[test]
fn a_write_copies_the_path_to_the_cell_and_the_older_document_keeps_its_value() {
    let mut doc = DocBuilder::new()
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).cell("Width", "1")).shape(ShapeBuilder::new(11).cell("Width", "1")))
        .page(2, PageBuilder::new())
        .build();
    let before = doc.clone();
    write(&mut doc, &shape(&[1]), "Width", CellEdit::Value(Value::from_text("2")), WriteOrigin::Automation);
    assert!(Arc::ptr_eq(doc.pages.get(1).unwrap(), before.pages.get(1).unwrap()), "the other page is shared");
    assert!(!Arc::ptr_eq(doc.pages.get(0).unwrap(), before.pages.get(0).unwrap()));
    assert!(
        Arc::ptr_eq(doc.pages.get(0).unwrap().shapes.get(0).unwrap(), before.pages.get(0).unwrap().shapes.get(0).unwrap()),
        "the other shape is shared"
    );
    assert!(!Arc::ptr_eq(doc.pages.get(0).unwrap().shapes.get(1).unwrap(), before.pages.get(0).unwrap().shapes.get(1).unwrap()));
    assert_eq!(value_of(&before, &shape(&[1]), "Width").as_deref(), Some("1"));
    assert_eq!(value_of(&doc, &shape(&[1]), "Width").as_deref(), Some("2"));
}
