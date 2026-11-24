//! Writes in a batch, the evaluation origin and a write into a deleted row: what `calc`'s commit (Task 6) writes with.

use std::sync::Arc;

use diacraft_doc::{
    Cell, CellAddr, CellEdit, CellWrite, DocError, Document, FormulaSlot, Origin, Owner, Resolver, RowKey, SectionKey, ShapeRef, Sheet, SheetRef,
    Source, Style, Value, WriteOrigin, WriteOutcome,
};
use diacraft_testkit::build::{DocBuilder, MasterBuilder, PageBuilder, ShapeBuilder, StyleBuilder};

fn at(path: &[u32]) -> SheetRef {
    SheetRef::Shape(ShapeRef { owner: Owner::Page(1), path: path.to_vec() })
}

fn value(doc: &Document, sheet: &SheetRef, name: &str) -> Option<String> {
    Resolver::new(doc).sheet(sheet)?.cell(name)?.value.as_ref().map(|v| v.text().to_owned())
}

fn set(v: &str) -> CellEdit {
    CellEdit::Value(Value::from_text(v))
}

fn cached(v: &str) -> CellEdit {
    CellEdit::Cached { value: Value::from_text(v), error: None, unit: None }
}

// ---- a write into a deleted row or section --------------------------------------------------------------------------

#[test]
fn a_write_into_a_deleted_row_is_an_error_whatever_wrote_it_and_nothing_changes() {
    // The review: a write into a `Del` row returned `Ok(Written)`, and then the row read as gone, so the edit was lost without a word.
    let doc = DocBuilder::new()
        .page(
            1,
            PageBuilder::new().shape(ShapeBuilder::new(10).del_row("Scratch", None, RowKey::Ix(0)).row_cell(
                "Scratch",
                None,
                RowKey::Ix(1),
                "X",
                "1",
            )),
        )
        .build();
    let row = CellAddr::in_row("Scratch", None, RowKey::Ix(0), "X");
    for origin in [
        WriteOrigin::Drawing,
        WriteOrigin::StyleApply,
        WriteOrigin::Automation,
        WriteOrigin::AutomationForce,
        WriteOrigin::Setf,
        WriteOrigin::DataRefresh,
        WriteOrigin::Evaluation,
    ] {
        for edit in [set("5"), cached("5"), CellEdit::ClearFormula, CellEdit::Cell(Cell::constant(Value::from_text("5")))] {
            let mut copy = doc.clone();
            let e = copy.write_cell(&at(&[0]), &row, edit.clone(), origin);
            // An evaluation's edit that is not a cached result is refused first, for the reason of its own (below); it is refused either way.
            let want = if origin == WriteOrigin::Evaluation && !matches!(edit, CellEdit::Cached { .. }) {
                DocError::EvaluationEdit
            } else {
                DocError::Deleted("row")
            };
            assert_eq!(e, Err(want), "{origin:?} {edit:?}");
            assert_eq!(copy, doc, "nothing in the document changed");
        }
    }
    // A write to another row of the same section is made.
    let mut copy = doc.clone();
    let other = CellAddr::in_row("Scratch", None, RowKey::Ix(1), "X");
    assert_eq!(copy.write_cell(&at(&[0]), &other, set("2"), WriteOrigin::Automation), Ok(WriteOutcome::Written));
}

#[test]
fn a_write_into_a_deleted_section_is_an_error_too() {
    let doc = DocBuilder::new().page(1, PageBuilder::new().shape(ShapeBuilder::new(10).del_section("Geometry", Some(0)))).build();
    let mut copy = doc.clone();
    for addr in [CellAddr::in_row("Geometry", Some(0), RowKey::Ix(1), "X"), CellAddr::in_section("Geometry", Some(0), "NoFill")] {
        let e = copy.write_cell(&at(&[0]), &addr, set("1"), WriteOrigin::Automation);
        assert_eq!(e, Err(DocError::Deleted("section")), "{addr:?}");
    }
    assert_eq!(copy, doc);
    // Another `Geometry` section of the sheet is not deleted.
    let other = CellAddr::in_row("Geometry", Some(1), RowKey::Ix(1), "X");
    assert_eq!(copy.write_cell(&at(&[0]), &other, set("1"), WriteOrigin::Automation), Ok(WriteOutcome::Written));
}

#[test]
fn a_deleted_row_is_brought_back_by_the_row_and_then_takes_a_write() {
    let mut sheet = Sheet::default();
    let section = SectionKey::new("Scratch", None);
    sheet.row_mut(&section, &RowKey::Ix(0)).unwrap().del = true;
    let addr = CellAddr::in_row("Scratch", None, RowKey::Ix(0), "X");
    assert_eq!(sheet.set_cell(&addr, Cell::constant(Value::from_text("1"))), Err(DocError::Deleted("row")));
    sheet.row_mut(&section, &RowKey::Ix(0)).unwrap().del = false;
    assert_eq!(sheet.set_cell(&addr, Cell::constant(Value::from_text("1"))), Ok(None));
    // And a deleted section the same.
    sheet.section_mut(&section).unwrap().del = true;
    assert_eq!(sheet.set_cell(&addr, Cell::constant(Value::from_text("2"))), Err(DocError::Deleted("section")));
    sheet.section_mut(&section).unwrap().del = false;
    assert!(sheet.set_cell(&addr, Cell::constant(Value::from_text("2"))).is_ok());
    assert!(DocError::Deleted("row").to_string().contains("deleted"));
}

// ---- the evaluation origin ----------------------------------------------------------------------------------------------------

fn guarded() -> Document {
    DocBuilder::new()
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).cell("Width", "6").formula("LocPinX", "GUARD(Width*0.47)", "0.94")))
        .build()
}

#[test]
fn an_evaluation_commits_to_a_guarded_cell_and_leaves_its_formula() {
    // `LocPinX` `GUARD(Width*0.47)` with Width 2, and a write of 6 to Width: LocPinX is 2.82. A guard does not freeze a value. Before
    // this origin a commit to a guarded cell had to pose as a forced write.
    let mut doc = guarded();
    let formula_before = doc.pages.get(0).unwrap().shapes.get(0).unwrap().sheet.cell("LocPinX").unwrap().formula.clone();
    let out = doc.write_cell(&at(&[0]), &CellAddr::direct("LocPinX"), cached("2.82"), WriteOrigin::Evaluation);
    assert_eq!(out, Ok(WriteOutcome::Written));
    assert_eq!(value(&doc, &at(&[0]), "LocPinX").as_deref(), Some("2.82"));
    let cell = doc.pages.get(0).unwrap().shapes.get(0).unwrap().sheet.cell("LocPinX").unwrap().clone();
    assert_eq!(cell.formula, formula_before, "the formula is exactly as it was");
    assert!(cell.is_guarded());
    // Any other origin is stopped by the guard, which is why the evaluation has its own.
    for origin in [WriteOrigin::Drawing, WriteOrigin::Automation, WriteOrigin::StyleApply] {
        let mut copy = guarded();
        assert_eq!(copy.write_cell(&at(&[0]), &CellAddr::direct("LocPinX"), cached("9"), origin), Ok(WriteOutcome::KeptGuarded), "{origin:?}");
        assert_eq!(value(&copy, &at(&[0]), "LocPinX").as_deref(), Some("0.94"));
    }
    let mut refresh = guarded();
    assert_eq!(refresh.write_cell(&at(&[0]), &CellAddr::direct("LocPinX"), cached("9"), WriteOrigin::DataRefresh), Ok(WriteOutcome::Skipped));
}

#[test]
fn an_evaluation_may_write_a_cached_result_and_nothing_else() {
    // The review: under the evaluation origin a value or a clear removed a guard that no other origin may. Only a cached result leaves
    // the formula as it is, which is why a guard does not stop it; every other edit is refused, and nothing changes.
    let doc = guarded();
    let guard = CellAddr::direct("LocPinX");
    for edit in [set("9"), CellEdit::ClearFormula, formula_edit("Width*2"), CellEdit::Cell(Cell::constant(Value::from_text("9")))] {
        let mut copy = doc.clone();
        let e = copy.write_cell(&at(&[0]), &guard, edit.clone(), WriteOrigin::Evaluation);
        assert_eq!(e, Err(DocError::EvaluationEdit), "{edit:?}");
        assert_eq!(copy, doc, "the guard is where it was");
    }
    // In a batch it refuses the whole batch, and the write before it is taken back.
    let mut copy = doc.clone();
    let batch = vec![one(&at(&[0]), "Width", set("7"), WriteOrigin::Drawing), one(&at(&[0]), "LocPinX", set("9"), WriteOrigin::Evaluation)];
    assert_eq!(copy.write_batch(batch), Err(DocError::EvaluationEdit));
    assert_eq!(copy, doc);
}

#[test]
fn a_cached_result_keeps_the_formula_and_sets_the_value_and_the_error() {
    let mut doc = DocBuilder::new()
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).formula("Height", "Width*2", "4").cell_u("Width", "2", "IN")))
        .build();
    let edit = CellEdit::Cached { value: Value::from_text("8"), error: Some(Arc::from("#DIV/0!")), unit: None };
    doc.write_cell(&at(&[0]), &CellAddr::direct("Height"), edit, WriteOrigin::Evaluation).unwrap();
    let cell = doc.pages.get(0).unwrap().shapes.get(0).unwrap().sheet.cell("Height").unwrap().clone();
    assert_eq!((cell.formula.text(), cell.value.as_ref().map(Value::text), cell.error.as_deref()), (Some("Width*2"), Some("8"), Some("#DIV/0!")));
    // A good evaluation clears the error; a unit stays.
    doc.write_cell(&at(&[0]), &CellAddr::direct("Height"), cached("9"), WriteOrigin::Evaluation).unwrap();
    let cell = doc.pages.get(0).unwrap().shapes.get(0).unwrap().sheet.cell("Height").unwrap().clone();
    assert_eq!((cell.value.as_ref().map(Value::text), cell.error.as_deref()), (Some("9"), None));
    doc.write_cell(&at(&[0]), &CellAddr::direct("Width"), cached("3"), WriteOrigin::Evaluation).unwrap();
    let width = doc.pages.get(0).unwrap().shapes.get(0).unwrap().sheet.cell("Width").unwrap().clone();
    assert_eq!(width.unit.as_deref(), Some("IN"));
}

#[test]
fn a_cached_result_for_a_cell_that_is_not_there_is_an_inh_cell_and_never_the_formula_text() {
    // `F="Inh"` and the value; the master's formula text is not copied down.
    let mut doc = DocBuilder::new()
        .master(1, MasterBuilder::new().shape(ShapeBuilder::new(5).formula("LocPinX", "Width*0.5", "1")))
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).master(1).cell("Width", "6")))
        .build();
    doc.write_cell(&at(&[0]), &CellAddr::direct("LocPinX"), cached("3"), WriteOrigin::Evaluation).unwrap();
    let cell = doc.pages.get(0).unwrap().shapes.get(0).unwrap().sheet.cell("LocPinX").unwrap().clone();
    assert_eq!((cell.formula.clone(), cell.origin()), (FormulaSlot::Inherit, Origin::InheritedWithCachedValue));
    assert_eq!(cell.value.as_ref().map(Value::text), Some("3"));
    let r = Resolver::new(&doc).resolve(&at(&[0]), &CellAddr::direct("LocPinX")).unwrap();
    assert!(matches!(r.formula.source, Source::Master(_)), "the formula is still the master's");
    assert_eq!((r.value.source, r.value.cell.and_then(|c| c.value.as_ref()).map(Value::text)), (Source::Local, Some("3")));
    // The inherited guard of the master does not stop an evaluation either.
    let mut inherited = DocBuilder::new()
        .master(1, MasterBuilder::new().shape(ShapeBuilder::new(5).formula("LocPinX", "GUARD(Width*0.5)", "1")))
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).master(1)))
        .build();
    let out = inherited.write_cell(&at(&[0]), &CellAddr::direct("LocPinX"), cached("3"), WriteOrigin::Evaluation);
    assert_eq!(out, Ok(WriteOutcome::Written));
}

// ---- a batch -----------------------------------------------------------------------------------------------------------------

fn write(name: &str, edit: CellEdit, origin: WriteOrigin) -> CellWrite {
    CellWrite { addr: CellAddr::direct(name), edit, origin }
}

#[test]
fn a_batch_writes_each_cell_as_a_single_write_would_and_reports_each() {
    let mut batch = guarded();
    let mut single = guarded();
    let writes = vec![
        write("Width", set("7"), WriteOrigin::Drawing),
        write("LocPinX", set("1"), WriteOrigin::Drawing),
        write("Height", cached("3"), WriteOrigin::Evaluation),
        write("LocPinX", cached("3.29"), WriteOrigin::Evaluation),
    ];
    let outcomes = batch.write_cells(&at(&[0]), writes.clone()).unwrap();
    assert_eq!(outcomes, [WriteOutcome::Written, WriteOutcome::KeptGuarded, WriteOutcome::Written, WriteOutcome::Written]);
    for w in writes {
        single.write_cell(&at(&[0]), &w.addr, w.edit, w.origin).unwrap();
    }
    assert_eq!(batch, single, "one pass and four passes make the same document");
    assert!(batch.write_cells(&at(&[0]), Vec::new()).unwrap().is_empty());
}

// ---- a batch is the same writes one by one -----------------------------------------------------------------------------------------

fn formula_edit(source: &str) -> CellEdit {
    match Cell::with_formula(source, None).formula {
        FormulaSlot::Local(formula) => CellEdit::Formula { formula, value: None },
        other => panic!("{other:?}"),
    }
}

fn master_sheet() -> SheetRef {
    SheetRef::Shape(ShapeRef { owner: Owner::Master(1), path: vec![0] })
}

/// An instance of a master, whose fill comes from style 1, which holds a guarded fill colour (the cells the review's four cases use).
fn differential_doc() -> Document {
    DocBuilder::new()
        .style(1, StyleBuilder::new().cell("EnableFillProps", "1").formula("FillForegnd", "GUARD(3)", "3"))
        .master(
            1,
            MasterBuilder::new()
                .shape(ShapeBuilder::new(5).formula("LocPinX", "Width*0.5", "3").formula("PinX", "GUARD(Width)", "6").cell("Angle", "0")),
        )
        .page(
            1,
            PageBuilder::new().shape(
                ShapeBuilder::new(10)
                    .master(1)
                    .links(None, Some(1), None)
                    .cell("Width", "6")
                    .cell("LocPinX", "1")
                    .inh("FillForegnd", Some("3"))
                    .formula("Angle", "GUARD(0)", "0"),
            ),
        )
        .build()
}

/// What a batch and the same writes one by one must agree on: the outcomes and the document when they succeed, the first error and an
/// unchanged document when one is refused.
fn assert_same_as_one_by_one(doc: &Document, writes: &[(SheetRef, CellWrite)]) {
    let mut batch = doc.clone();
    let batched = batch.write_batch(writes.to_vec());
    let mut single = doc.clone();
    let mut outcomes = Vec::new();
    let mut refused = None;
    for (sheet, w) in writes {
        match single.write_cell(sheet, &w.addr, w.edit.clone(), w.origin) {
            Ok(outcome) => outcomes.push(outcome),
            Err(e) => {
                refused = Some(e);
                break;
            }
        }
    }
    match refused {
        None => {
            assert_eq!(batched, Ok(outcomes), "outcomes of {writes:?}");
            assert_eq!(&batch, &single, "documents after {writes:?}");
        }
        Some(e) => {
            assert_eq!(batched, Err(e), "the first refusal of {writes:?}");
            assert_eq!(&batch, doc, "a refused batch changes nothing");
        }
    }
}

fn one(sheet: &SheetRef, name: &str, edit: CellEdit, origin: WriteOrigin) -> (SheetRef, CellWrite) {
    (sheet.clone(), CellWrite { addr: CellAddr::direct(name), edit, origin })
}

fn cell_of(doc: &Document, name: &str) -> Cell {
    doc.pages.get(0).unwrap().shapes.get(0).unwrap().sheet.cell(name).unwrap().clone()
}

#[test]
fn a_guard_written_in_a_batch_stops_the_next_write_as_it_would_one_by_one() {
    let (doc, shape) = (differential_doc(), at(&[0]));
    let writes = [one(&shape, "Width", formula_edit("GUARD(Height)"), WriteOrigin::Automation), one(&shape, "Width", set("9"), WriteOrigin::Drawing)];
    assert_same_as_one_by_one(&doc, &writes);
    let mut batch = doc.clone();
    let out = batch.write_batch(writes.to_vec()).unwrap();
    assert_eq!(out, [WriteOutcome::Written, WriteOutcome::KeptGuarded], "the guard just added keeps the Drawing write out");
}

#[test]
fn a_guard_forced_away_in_a_batch_lets_the_next_write_through() {
    let (doc, shape) = (differential_doc(), at(&[0]));
    // `Angle` is guarded; a forced write replaces its formula with a constant, and then a Drawing write is not stopped.
    let writes = [one(&shape, "Angle", set("5"), WriteOrigin::AutomationForce), one(&shape, "Angle", set("9"), WriteOrigin::Drawing)];
    assert_same_as_one_by_one(&doc, &writes);
    let mut batch = doc.clone();
    assert_eq!(batch.write_batch(writes.to_vec()).unwrap(), [WriteOutcome::Written, WriteOutcome::Written]);
    assert_eq!(value(&batch, &shape, "Angle").as_deref(), Some("9"));
}

#[test]
fn clearing_a_formula_in_a_batch_goes_back_to_what_the_batch_just_wrote_on_the_master() {
    let (doc, shape) = (differential_doc(), at(&[0]));
    // The master has no formula for `Height` until the batch writes one, so the clear that follows has something to go back to. The
    // shape's own `Height` is a formula of its own first (a clear on a constant has no formula to take away).
    let writes = [
        one(&shape, "Height", formula_edit("Width*3"), WriteOrigin::Automation),
        one(&master_sheet(), "Height", formula_edit("Width*2"), WriteOrigin::Automation),
        one(&shape, "Height", CellEdit::ClearFormula, WriteOrigin::Automation),
    ];
    assert_same_as_one_by_one(&doc, &writes);
    let mut batch = doc.clone();
    batch.write_batch(writes.to_vec()).unwrap();
    assert_eq!(cell_of(&batch, "Height").formula, FormulaSlot::Inherit, "it goes back to the formula the master now has");
}

#[test]
fn a_style_flag_turned_off_in_a_batch_stops_the_style_guarding_the_next_write() {
    let (doc, shape) = (differential_doc(), at(&[0]));
    // The style's `FillForegnd` is guarded and the shape's own is `Inh`, so it inherits the guard: a Drawing write alone is kept
    // out. With `EnableFillProps` 0 written first, the style is skipped for the fill family and the same write is let through.
    let mut alone = doc.clone();
    let blocked = alone.write_batch(vec![one(&shape, "FillForegnd", set("7"), WriteOrigin::Drawing)]).unwrap();
    assert_eq!(blocked, [WriteOutcome::KeptGuarded], "the guard of the style stops it");
    let writes =
        [one(&SheetRef::Style(1), "EnableFillProps", set("0"), WriteOrigin::Automation), one(&shape, "FillForegnd", set("7"), WriteOrigin::Drawing)];
    assert_same_as_one_by_one(&doc, &writes);
    let mut batch = doc.clone();
    let out = batch.write_batch(writes.to_vec()).unwrap();
    assert_eq!(out, [WriteOutcome::Written, WriteOutcome::Written], "the style no longer takes part, so its guard is not met");
}

mod random {
    use proptest::prelude::*;

    use super::*;

    const NAMES: [&str; 7] = ["LocPinX", "PinX", "Width", "Height", "FillForegnd", "EnableFillProps", "Angle"];
    const ORIGINS: [WriteOrigin; 7] = [
        WriteOrigin::Drawing,
        WriteOrigin::StyleApply,
        WriteOrigin::Automation,
        WriteOrigin::AutomationForce,
        WriteOrigin::Setf,
        WriteOrigin::DataRefresh,
        WriteOrigin::Evaluation,
    ];

    fn sheets() -> [SheetRef; 3] {
        [at(&[0]), master_sheet(), SheetRef::Style(1)]
    }

    fn edit(kind: u8, n: u8) -> CellEdit {
        let v = (n % 4).to_string();
        match kind % 7 {
            0 | 1 => set(&v),
            2 => formula_edit("Width*2"),
            3 => formula_edit("GUARD(Height)"),
            4 => CellEdit::ClearFormula,
            5 => cached(&v),
            _ => CellEdit::Cached { value: Value::from_text(&v), error: Some(Arc::from("#REF!")), unit: None },
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 400, ..ProptestConfig::default() })]

        /// The same writes in one batch and one by one agree, whatever mix of sheets, cells, edits and origins: the shapes the review found
        /// (a guard added or forced away, a master cell written then cleared, a style flag switched) are four of them.
        #[test]
        fn a_batch_and_the_same_writes_one_by_one_agree(
            steps in proptest::collection::vec((0usize..3, 0usize..7, any::<u8>(), any::<u8>(), 0usize..7), 1..10)
        ) {
            let doc = differential_doc();
            let all = sheets();
            let writes: Vec<(SheetRef, CellWrite)> = steps
                .iter()
                .map(|(s, name, kind, n, origin)| {
                    let sheet = all.get(*s).cloned().unwrap_or(SheetRef::Document);
                    let name = NAMES.get(*name).copied().unwrap_or("Width");
                    one(&sheet, name, edit(*kind, *n), ORIGINS.get(*origin).copied().unwrap_or(WriteOrigin::Drawing))
                })
                .collect();
            assert_same_as_one_by_one(&doc, &writes);
        }
    }
}

#[test]
fn a_batch_is_all_or_nothing() {
    let doc = guarded();
    let mut copy = doc.clone();
    let writes = vec![
        write("Width", set("7"), WriteOrigin::Automation),
        write("", set("1"), WriteOrigin::Automation),
        write("Height", set("1"), WriteOrigin::Automation),
    ];
    assert_eq!(copy.write_cells(&at(&[0]), writes), Err(DocError::BadName("a cell name")));
    assert_eq!(copy, doc, "the first write was taken back with the others");
    // A sheet that is not there is refused before anything.
    let e = copy.write_cells(&at(&[4]), vec![write("Width", set("7"), WriteOrigin::Automation)]);
    assert_eq!(e, Err(DocError::NoSuchSheet));
}

#[test]
fn a_batch_builds_the_lookups_once_where_a_loop_of_writes_builds_them_each_time() {
    // The review: a write builds a resolver, and with 65,536 styles a cell in a family builds a map of them for the one write. The commit
    // writes many cells of one sheet, which a batch does with one resolver. The claim is relative (the batch against the loop on the
    // same document) and it is counted, not timed: the lookups the resolvers built (`diacraft_doc::work`), which does not move with the load.
    let mut doc = DocBuilder::new().page(1, PageBuilder::new().shape(ShapeBuilder::new(10).links(Some(1), None, None))).build();
    let styles: Vec<Arc<Style>> = (1..=65_536).map(|id| Arc::new(Style { id, ..Style::default() })).collect();
    doc.styles = styles.into_iter().collect();
    // Each is a cell the sheet does not hold yet, so judging the write walks on to the styles (the root style, for a cell no batch lists).
    let make = |origin: WriteOrigin| -> Vec<CellWrite> { (0..200).map(|i| write(&format!("Mine{i}"), set(&i.to_string()), origin)).collect() };
    diacraft_doc::work::reset();
    let mut one_by_one = doc.clone();
    for w in make(WriteOrigin::Automation) {
        one_by_one.write_cell(&at(&[0]), &w.addr, w.edit, w.origin).unwrap();
    }
    let loop_builds = diacraft_doc::work::resolver_builds();
    diacraft_doc::work::reset();
    let mut batched = doc.clone();
    batched.write_cells(&at(&[0]), make(WriteOrigin::Automation)).unwrap();
    let batch_builds = diacraft_doc::work::resolver_builds();
    eprintln!("200 writes of new cells on 65,536 styles: {loop_builds} lookups built one by one, {batch_builds} as a batch");
    assert_eq!(one_by_one, batched);
    assert!(loop_builds >= 200, "the loop builds the lookups for each write: {loop_builds}");
    assert!(batch_builds <= 8, "the batch builds them once: {batch_builds}");
}

#[test]
fn the_other_sheets_of_a_document_take_a_batch_too() {
    let mut doc = DocBuilder::new().page(1, PageBuilder::new()).master(1, MasterBuilder::new()).build();
    for sheet in [SheetRef::Document, SheetRef::Page(1), SheetRef::Master(1), SheetRef::Style(0)] {
        let out = doc
            .write_cells(&sheet, vec![write("Mine", set("1"), WriteOrigin::Automation), write("Mine2", cached("2"), WriteOrigin::Evaluation)])
            .unwrap();
        assert_eq!(out, [WriteOutcome::Written, WriteOutcome::Written], "{sheet:?}");
    }
}
