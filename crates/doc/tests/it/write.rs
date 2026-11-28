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

// ----: a row an instance makes of its own holds the type of its master's row

/// A master of a box: its `Geometry` 0 has rows `IX` 1 (`MoveTo`) to 5 (`LineTo`), the third and fourth named by `Width` and `Height`, and its `Connection` rows have `T` `Connection`.
fn box_master() -> MasterBuilder {
    let mut shape = ShapeBuilder::new(5).cell("Width", "2").cell("Height", "1");
    for (ix, kind) in [(1, "MoveTo"), (2, "LineTo"), (3, "LineTo"), (4, "LineTo"), (5, "LineTo")] {
        shape = shape.row_type("Geometry", Some(0), RowKey::Ix(ix), kind).row_cell("Geometry", Some(0), RowKey::Ix(ix), "X", "0").row_cell(
            "Geometry",
            Some(0),
            RowKey::Ix(ix),
            "Y",
            "0",
        );
    }
    shape =
        shape.row_formula("Geometry", Some(0), RowKey::Ix(3), "Y", "Height", "1").row_formula("Geometry", Some(0), RowKey::Ix(4), "Y", "Height", "1");
    for ix in 0..2 {
        shape = shape.row_type("Connection", None, RowKey::Ix(ix), "Connection").row_cell("Connection", None, RowKey::Ix(ix), "X", "1");
    }
    MasterBuilder::new().shape(shape)
}

fn geometry() -> diacraft_doc::SectionKey {
    diacraft_doc::SectionKey::new("Geometry", Some(0))
}

/// The rows the sheet holds of the section, as `(IX, T)`.
fn held_rows(doc: &Document, at: &SheetRef, section: &diacraft_doc::SectionKey) -> Vec<(Option<u32>, Option<String>)> {
    let sheet = Resolver::new(doc).sheet(at).unwrap();
    sheet.section(section).map(|s| s.rows.iter().map(|r| (r.ix, r.row_type.as_deref().map(str::to_owned))).collect()).unwrap_or_default()
}

fn cached(text: &str) -> CellEdit {
    CellEdit::Cached { value: Value::from_text(text), error: None, unit: None }
}

fn y_of(ix: u32) -> CellAddr {
    CellAddr::in_row("Geometry", Some(0), RowKey::Ix(ix), "Y")
}

fn box_doc(instance: ShapeBuilder) -> Document {
    DocBuilder::new().master(1, box_master()).page(1, PageBuilder::new().shape(instance)).build()
}

fn row(ix: u32, t: &str) -> (Option<u32>, Option<String>) {
    (Some(ix), Some(t.to_owned()))
}

#[test]
fn a_result_written_to_a_row_the_instance_lacks_makes_the_row_with_the_masters_type() {
    let mut doc = box_doc(ShapeBuilder::new(10).master(1));
    let at = shape(&[0]);
    assert_eq!(held_rows(&doc, &at, &geometry()), []);
    let writes = vec![
        (at.clone(), diacraft_doc::CellWrite { addr: y_of(3), edit: cached("3"), origin: WriteOrigin::Evaluation }),
        (at.clone(), diacraft_doc::CellWrite { addr: y_of(4), edit: cached("3"), origin: WriteOrigin::Evaluation }),
    ];
    doc.write_batch(writes).unwrap();
    assert_eq!(held_rows(&doc, &at, &geometry()), [row(3, "LineTo"), row(4, "LineTo")]);
    // The cell is the `Inh` cell the commit makes, with the result.
    let cell = Resolver::new(&doc).sheet(&at).unwrap().lookup(&y_of(3));
    let diacraft_doc::Lookup::Found(cell) = cell else { panic!("{cell:?}") };
    assert_eq!((&cell.formula, cell.value.as_ref().map(|v| v.text())), (&FormulaSlot::Inherit, Some("3")));
}

#[test]
fn a_value_or_a_formula_written_to_a_row_the_instance_lacks_gives_it_the_type_as_well() {
    for edit in [CellEdit::Value(Value::from_text("2")), formula("Height*2")] {
        let mut doc = box_doc(ShapeBuilder::new(10).master(1));
        let at = shape(&[0]);
        doc.write_cell(&at, &y_of(2), edit, WriteOrigin::Automation).unwrap();
        assert_eq!(held_rows(&doc, &at, &geometry()), [row(2, "LineTo")]);
    }
}

#[test]
fn a_row_the_instance_holds_with_no_type_is_given_the_masters_by_the_next_write_to_it() {
    // A row an older build saved: it has its `IX` and its cell, and no `T`.
    let mut doc = box_doc(ShapeBuilder::new(10).master(1).row_inh("Geometry", Some(0), RowKey::Ix(3), "Y", Some("3")));
    let at = shape(&[0]);
    assert_eq!(held_rows(&doc, &at, &geometry()), [(Some(3), None)]);
    doc.write_cell(&at, &y_of(3), cached("4"), WriteOrigin::Evaluation).unwrap();
    assert_eq!(held_rows(&doc, &at, &geometry()), [row(3, "LineTo")]);
    assert_eq!(value_of_row(&doc, &at, &y_of(3)).as_deref(), Some("4"));
}

/// The text of the value of the cell the sheet holds at `addr`.
fn value_of_row(doc: &Document, at: &SheetRef, addr: &CellAddr) -> Option<String> {
    match Resolver::new(doc).sheet(at)?.lookup(addr) {
        diacraft_doc::Lookup::Found(cell) => cell.value.as_ref().map(|v| v.text().to_owned()),
        _ => None,
    }
}

#[test]
fn a_row_with_a_type_of_its_own_keeps_it() {
    let mut doc = box_doc(ShapeBuilder::new(10).master(1).row_type("Geometry", Some(0), RowKey::Ix(3), "ArcTo").row_inh(
        "Geometry",
        Some(0),
        RowKey::Ix(3),
        "Y",
        Some("3"),
    ));
    let at = shape(&[0]);
    doc.write_cell(&at, &y_of(3), cached("4"), WriteOrigin::Evaluation).unwrap();
    assert_eq!(held_rows(&doc, &at, &geometry()), [row(3, "ArcTo")]);
}

#[test]
fn a_row_the_master_does_not_have_is_made_with_no_type_and_so_is_one_of_a_shape_with_no_master() {
    let mut doc = box_doc(ShapeBuilder::new(10).master(1));
    let at = shape(&[0]);
    doc.write_cell(&at, &y_of(9), CellEdit::Value(Value::from_text("1")), WriteOrigin::Automation).unwrap();
    assert_eq!(held_rows(&doc, &at, &geometry()), [(Some(9), None)], "nothing to inherit it from");
    let mut plain = doc_with(ShapeBuilder::new(10));
    plain.write_cell(&at, &y_of(3), CellEdit::Value(Value::from_text("1")), WriteOrigin::Automation).unwrap();
    assert_eq!(held_rows(&plain, &at, &geometry()), [(Some(3), None)]);
}

/// `Connection` rows are left as they are: the files hold `T` on some and not on the partial row of an instance.
#[test]
fn a_connection_row_the_instance_makes_has_no_type_though_the_masters_has_one() {
    let mut doc = box_doc(ShapeBuilder::new(10).master(1));
    let at = shape(&[0]);
    let x = CellAddr::in_row("Connection", None, RowKey::Ix(1), "X");
    doc.write_cell(&at, &x, cached("2"), WriteOrigin::Evaluation).unwrap();
    assert_eq!(held_rows(&doc, &at, &diacraft_doc::SectionKey::new("Connection", None)), [(Some(1), None)]);
}

#[test]
fn a_row_deleted_here_is_refused_and_given_nothing() {
    let mut doc = box_doc(ShapeBuilder::new(10).master(1).del_row("Geometry", Some(0), RowKey::Ix(3)));
    let at = shape(&[0]);
    let before = doc.clone();
    assert!(matches!(doc.write_cell(&at, &y_of(3), cached("4"), WriteOrigin::Evaluation), Err(DocError::Deleted(_))));
    assert_eq!(doc, before);
}

/// A member of an instance of a group master, which stands for a shape of the master by `MasterShape`, takes the type of that shape's row.
#[test]
fn a_member_of_an_instance_takes_the_type_of_the_row_of_the_master_shape_it_stands_for() {
    let member = ShapeBuilder::new(6)
        .row_type("Geometry", Some(0), RowKey::Ix(1), "MoveTo")
        .row_cell("Geometry", Some(0), RowKey::Ix(1), "X", "0")
        .row_type("Geometry", Some(0), RowKey::Ix(2), "RelLineTo")
        .row_cell("Geometry", Some(0), RowKey::Ix(2), "X", "1");
    let master = MasterBuilder::new().shape(ShapeBuilder::new(5).group().child(member));
    let instance = ShapeBuilder::new(10).master(1).group().child(ShapeBuilder::new(11).master_shape(6));
    let mut doc = DocBuilder::new().master(1, master).page(1, PageBuilder::new().shape(instance)).build();
    let at = shape(&[0, 0]);
    doc.write_cell(&at, &CellAddr::in_row("Geometry", Some(0), RowKey::Ix(2), "X"), cached("3"), WriteOrigin::Evaluation).unwrap();
    assert_eq!(held_rows(&doc, &at, &geometry()), [row(2, "RelLineTo")]);
}

/// `Document::give_row_type`, for a writer that puts a cell in with the sheet's own methods.
#[test]
fn giving_a_row_its_type_answers_whether_it_did() {
    let mut doc = box_doc(
        ShapeBuilder::new(10)
            .master(1)
            .row_inh("Geometry", Some(0), RowKey::Ix(3), "Y", Some("3"))
            .row_type("Geometry", Some(0), RowKey::Ix(4), "ArcTo")
            .row_inh("Geometry", Some(0), RowKey::Ix(9), "Y", Some("1")),
    );
    let at = shape(&[0]);
    assert_eq!(doc.give_row_type(&at, &geometry(), &RowKey::Ix(3)), Ok(true));
    assert_eq!(doc.give_row_type(&at, &geometry(), &RowKey::Ix(3)), Ok(false), "it has one now");
    assert_eq!(doc.give_row_type(&at, &geometry(), &RowKey::Ix(4)), Ok(false), "its own");
    assert_eq!(doc.give_row_type(&at, &geometry(), &RowKey::Ix(9)), Ok(false), "the master has no such row");
    assert_eq!(doc.give_row_type(&at, &geometry(), &RowKey::Ix(2)), Ok(false), "the instance holds no such row, and none is made");
    assert_eq!(held_rows(&doc, &at, &geometry()), [row(3, "LineTo"), row(4, "ArcTo"), (Some(9), None)]);
    assert_eq!(doc.give_row_type(&shape(&[7]), &geometry(), &RowKey::Ix(3)), Ok(false), "no such shape");
    assert_eq!(doc.give_row_type(&SheetRef::Page(1), &geometry(), &RowKey::Ix(3)), Ok(false), "a page is no instance");
}

/// A row a write makes goes among the rows by its `IX`, which rise through a section: a resize that wrote rows 3 and 4 and then one that writes row 2 give 2, 3, 4.
#[test]
fn a_row_a_write_makes_goes_in_the_order_of_the_ix_of_the_rows() {
    let mut doc = box_doc(ShapeBuilder::new(10).master(1));
    let at = shape(&[0]);
    for ix in [3, 4, 2, 5, 1] {
        doc.write_cell(&at, &y_of(ix), cached("1"), WriteOrigin::Evaluation).unwrap();
    }
    let ixs: Vec<Option<u32>> = held_rows(&doc, &at, &geometry()).into_iter().map(|(ix, _)| ix).collect();
    assert_eq!(ixs, [Some(1), Some(2), Some(3), Some(4), Some(5)]);
    // A row below all of them goes first, and one above all of them last; the others are where they were.
    doc.write_cell(&at, &y_of(0), CellEdit::Value(Value::from_text("1")), WriteOrigin::Automation).unwrap();
    doc.write_cell(&at, &y_of(8), CellEdit::Value(Value::from_text("1")), WriteOrigin::Automation).unwrap();
    let ixs: Vec<Option<u32>> = held_rows(&doc, &at, &geometry()).into_iter().map(|(ix, _)| ix).collect();
    assert_eq!(ixs, [Some(0), Some(1), Some(2), Some(3), Some(4), Some(5), Some(8)]);
}

/// A section whose rows are out of order is not searched without end: a new row goes at the end when more than the scan's rows come after its place.
#[test]
fn a_row_goes_at_the_end_of_a_section_that_is_far_out_of_order() {
    let rows = |count: u32| {
        let mut shape = ShapeBuilder::new(10);
        for ix in (100..100 + count).rev() {
            shape = shape.row_cell("Geometry", Some(0), RowKey::Ix(ix), "X", "0");
        }
        shape
    };
    let at = shape(&[0]);
    // 70 rows in falling order: the new row 5 is more than 64 places from where it belongs, and goes last.
    let mut far = doc_with(rows(70));
    far.write_cell(&at, &y_of(5), CellEdit::Value(Value::from_text("1")), WriteOrigin::Automation).unwrap();
    assert_eq!(held_rows(&far, &at, &geometry()).last().map(|r| r.0), Some(Some(5)));
    // 10 rows in falling order: it goes first, ahead of the ten that are above it.
    let mut near = doc_with(rows(10));
    near.write_cell(&at, &y_of(5), CellEdit::Value(Value::from_text("1")), WriteOrigin::Automation).unwrap();
    assert_eq!(held_rows(&near, &at, &geometry()).first().map(|r| r.0), Some(Some(5)));
}

/// Only the rows of a `Geometry` section are placed by their `IX`: the order of its rows is the order of its path. A row made in another section goes after the others, as it always did.
#[test]
fn a_row_made_in_a_section_other_than_geometry_goes_after_the_others() {
    let mut doc = box_doc(ShapeBuilder::new(10).master(1));
    let at = shape(&[0]);
    for ix in [1, 0] {
        doc.write_cell(&at, &CellAddr::in_row("Scratch", None, RowKey::Ix(ix), "X"), CellEdit::Value(Value::from_text("1")), WriteOrigin::Automation)
            .unwrap();
    }
    let ixs: Vec<Option<u32>> = held_rows(&doc, &at, &diacraft_doc::SectionKey::new("Scratch", None)).into_iter().map(|(ix, _)| ix).collect();
    assert_eq!(ixs, [Some(1), Some(0)]);
}
