//! The one comparer, case by case: the cells it asks for and how it addresses them (the mutants of the review's I4), what is
//! left out and why (I1, I2), the error compared first (I6), every cell of a document in one place, a calculator that hands back the saved
//! values (I3), a sheet the model cannot tell from another (M9), and the allowance a score rests on (M8).

use std::sync::Arc;

use diacraft_calc::{
    Budget, Calculator, CellAddress, CellEval, CellKind, DifferenceKind, Evaluator, Native, RecalcReport, SheetPlace, ValueClass, recalc_all,
    recalc_all_using,
};
use diacraft_doc::{Cell, CellAddr, Document, FormulaSlot, Owner, RowKey, ShapeList, ShapeRef, Sheet, SheetRef, Value as DocValue};
use diacraft_testkit::build::{DocBuilder, MasterBuilder, PageBuilder, ShapeBuilder, StyleBuilder};

fn user(name: &str) -> CellAddr {
    CellAddr::in_row("User", None, RowKey::name(name), "Value")
}

/// A formula cell with its saved value, unit and error.
fn cell(formula: &str, v: &str, u: Option<&str>, e: Option<&str>) -> Cell {
    let mut c = Cell::with_formula(formula, Some(DocValue::from_text(v)));
    if let Some(u) = u {
        c = c.with_unit(u);
    }
    c.error = e.map(Arc::from);
    c
}

fn doc_of(shape: ShapeBuilder) -> Document {
    DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build()
}

/// A calculator that records the address of every cell it is asked for, and works it out as the evaluator does.
#[derive(Default)]
struct Recorder {
    asked: Vec<CellAddress>,
}

impl Calculator for Recorder {
    fn calculate(&mut self, ev: &mut Evaluator<'_, '_>, at: &CellAddress) -> Arc<CellEval> {
        self.asked.push(at.clone());
        ev.evaluate_cell(at)
    }
}

/// A calculator that hands back what the file saved: the echo the harness's control must catch.
struct Echo;

impl Calculator for Echo {
    fn calculate(&mut self, ev: &mut Evaluator<'_, '_>, at: &CellAddress) -> Arc<CellEval> {
        let mut e = (*ev.evaluate_cell(at)).clone();
        if e.kind == CellKind::Formula {
            if let Some(saved) = e.saved.clone() {
                e.raw = saved.clone();
                e.view = saved;
            }
            e.error = e.saved_error;
            e.unsupported = None;
        }
        Arc::new(e)
    }
}

fn run_recorded(doc: &Document) -> (RecalcReport, Vec<CellAddress>) {
    let mut recorder = Recorder::default();
    let report = recalc_all_using(doc, &Budget::default(), diacraft_calc::EvalOptions::default(), &mut recorder);
    (report, recorder.asked)
}

fn shape_at(owner: Owner, path: &[u32]) -> SheetRef {
    SheetRef::Shape(ShapeRef { owner, path: path.to_vec() })
}

// ---- how the walk addresses a cell (I4) ---------------------------------------------------------------------------------------

/// A document with one formula cell on every kind of sheet and at every depth, each worth `1+1`, saved as 2.
fn every_place() -> Document {
    let two = |s: ShapeBuilder| s;
    let mut doc = DocBuilder::new()
        .style(7, StyleBuilder::new().formula("LineWeight", "1+1", "2"))
        .master(
            3,
            MasterBuilder::new().formula("PageWidth", "1+1", "2").shape(
                two(ShapeBuilder::new(5).formula("Width", "1+1", "2")).child(ShapeBuilder::new(6).row_formula(
                    "Geometry",
                    Some(1),
                    RowKey::Ix(2),
                    "X",
                    "1+1",
                    "2",
                )),
            ),
        )
        .page(
            1,
            PageBuilder::new().formula("PageHeight", "1+1", "2").shape(ShapeBuilder::new(10).formula("Width", "1+1", "2")).shape(
                ShapeBuilder::new(11).group().child(
                    ShapeBuilder::new(12)
                        .set(user("A"), cell("1+1", "2", None, None))
                        .child(ShapeBuilder::new(13).set(CellAddr::in_section("Geometry", Some(0), "NoFill"), cell("1+1", "2", None, None))),
                ),
            ),
        )
        .build();
    Arc::make_mut(&mut doc.sheet).set_cell(&CellAddr::direct("DocLangID"), cell("1+1", "2", None, None)).unwrap();
    doc
}

#[test]
fn the_walk_asks_for_every_formula_cell_at_its_full_address_and_once() {
    let (report, mut asked) = run_recorded(&every_place());
    let mut want = vec![
        (SheetRef::Document, CellAddr::direct("DocLangID")),
        (SheetRef::Style(7), CellAddr::direct("LineWeight")),
        (SheetRef::Page(1), CellAddr::direct("PageHeight")),
        (shape_at(Owner::Page(1), &[0]), CellAddr::direct("Width")),
        (shape_at(Owner::Page(1), &[1, 0]), user("A")),
        (shape_at(Owner::Page(1), &[1, 0, 0]), CellAddr::in_section("Geometry", Some(0), "NoFill")),
        (SheetRef::Master(3), CellAddr::direct("PageWidth")),
        (shape_at(Owner::Master(3), &[0]), CellAddr::direct("Width")),
        (shape_at(Owner::Master(3), &[0, 0]), CellAddr::in_row("Geometry", Some(1), RowKey::Ix(2), "X")),
    ];
    let mut got: Vec<(SheetRef, CellAddr)> = asked.drain(..).map(|a| (a.sheet, a.addr)).collect();
    let key = |p: &(SheetRef, CellAddr)| format!("{p:?}");
    want.sort_by_key(key);
    got.sort_by_key(key);
    assert_eq!(got, want, "each cell once, at the sheet and the address that holds it");
    assert_eq!((report.cells, report.agreed, report.differed.len()), (9, 9, 0), "{report:?}");
}

#[test]
fn a_difference_names_its_sheet_by_ids_and_its_cell_by_section_row_and_name() {
    // I7: the sheet's id, the shape's own `ID` and depth, the section's `IX`, the row, the unit and whether the cell inherited.
    let doc = DocBuilder::new()
        .master(3, MasterBuilder::new().shape(ShapeBuilder::new(5).formula("Width", "1+1", "2")))
        .page(
            1,
            PageBuilder::new().shape(
                ShapeBuilder::new(11)
                    .group()
                    .child(ShapeBuilder::new(12).set(CellAddr::in_row("Geometry", Some(1), RowKey::Ix(2), "X"), cell("1+1", "9", Some("IN"), None))),
            ),
        )
        .page(2, PageBuilder::new().shape(ShapeBuilder::new(20).master(3).inh("Width", Some("7"))))
        .build();
    let r = recalc_all(&doc, &Budget::default());
    assert_eq!(r.differed.len(), 2, "{r:?}");
    let nested = r.differed.iter().find(|d| d.inherited.eq(&false)).unwrap();
    assert_eq!(nested.place, SheetPlace::Shape { owner: Owner::Page(1), id: Some(12), depth: 2 });
    assert_eq!(nested.address, CellAddr::in_row("Geometry", Some(1), RowKey::Ix(2), "X"));
    assert_eq!((nested.sheet.as_str(), nested.cell.as_str()), ("page:1/shape:12", "Geometry[1].IX=2.X"));
    assert_eq!((nested.saved.as_deref(), nested.actual.as_deref()), (Some("9 [IN]"), Some("2 [IN]")));
    assert_eq!((nested.saved_class, nested.actual_class), (Some(ValueClass::Number), Some(ValueClass::Number)));
    let inherited = r.differed.iter().find(|d| d.inherited).unwrap();
    assert_eq!(inherited.place, SheetPlace::Shape { owner: Owner::Page(2), id: Some(20), depth: 1 });
    assert_eq!(inherited.address, CellAddr::direct("Width"));
    assert_eq!(inherited.kind, DifferenceKind::Value);
}

#[test]
fn a_text_value_is_a_class_with_its_length_and_never_the_text_itself() {
    let r = recalc_all(&doc_of(ShapeBuilder::new(1).set(user("T"), cell(r#""abc"&"def""#, "zzzz", Some("STR"), None))), &Budget::default());
    let d = &r.differed[0];
    assert_eq!((d.saved_class, d.actual_class), (Some(ValueClass::Text { chars: 4 }), Some(ValueClass::Text { chars: 6 })));
}

// ---- every cell in one place (I6) ---------------------------------------------------------------------------------------------

fn count_sheet(s: &Sheet) -> u64 {
    let mut n = s.cells.len() as u64;
    for section in &s.sections {
        n += section.cells.len() as u64;
        for row in &section.rows {
            n += row.cells.len() as u64;
        }
    }
    n
}

fn count_shapes(list: &ShapeList) -> u64 {
    list.iter().map(|s| count_sheet(&s.sheet) + count_shapes(&s.children)).sum()
}

/// The cells of a document by a walk written apart from the oracle's: recursion over the model, nothing shared.
fn cells_in(doc: &Document) -> u64 {
    count_sheet(&doc.sheet)
        + doc.styles.iter().map(|s| count_sheet(&s.sheet)).sum::<u64>()
        + doc.pages.iter().map(|p| count_sheet(&p.sheet) + count_shapes(&p.shapes)).sum::<u64>()
        + doc.masters.iter().map(|m| count_sheet(&m.sheet) + count_shapes(&m.shapes)).sum::<u64>()
}

/// A shape with a cell of every kind the oracle must place: a formula, a constant, `No Formula`, `Inh`, an error with no formula, a deleted
/// row and a deleted section.
fn a_cell_of_every_kind() -> ShapeBuilder {
    let mut no_formula = Cell::constant(DocValue::from_text("3"));
    no_formula.formula = FormulaSlot::NoFormula;
    let mut no_formula_with_error = no_formula.clone();
    no_formula_with_error.error = Some(Arc::from("#N/A"));
    let mut value_with_error = Cell::constant(DocValue::from_text("4"));
    value_with_error.error = Some(Arc::from("#N/A"));
    ShapeBuilder::new(1)
        .set(user("Formula"), cell("1+1", "2", None, None))
        .cell("Width", "5")
        .set(CellAddr::direct("Height"), no_formula)
        .set(CellAddr::direct("PinX"), no_formula_with_error)
        .set(CellAddr::direct("PinY"), value_with_error)
        .inh("Angle", Some("1"))
        .row_formula("Scratch", None, RowKey::Ix(0), "A", "1+1", "2")
        .row_cell("Scratch", None, RowKey::Ix(0), "B", "1")
        .del_row("Scratch", None, RowKey::Ix(0))
        .row_formula("Prop", None, RowKey::name("Gone"), "Value", "1+1", "2")
        .del_section("Prop", None)
}

#[test]
fn every_cell_of_a_document_is_in_exactly_one_place_of_the_report() {
    for doc in [doc_of(a_cell_of_every_kind()), every_place()] {
        let r = recalc_all(&doc, &Budget::default());
        assert_eq!(r.cells_in_document(), cells_in(&doc), "{r:?}");
        assert_eq!(r.cells, r.accounted(), "every formula cell is scored, left out or not calculated: {r:?}");
        // Compared + left out + not calculated + non-candidates is every cell.
        let total = r.scored() + r.left_out() + r.not_calculated() + r.not_formulas.total();
        assert_eq!(total, cells_in(&doc), "{r:?}");
    }
    let r = recalc_all(&doc_of(a_cell_of_every_kind()), &Budget::default());
    let n = r.not_formulas;
    // A constant and a constant with an error, `No Formula` twice (one with an error), the root style's three flags: seven; and the cells of the deleted row (2) and section (1).
    assert_eq!((n.value_only, n.no_formula, n.deleted), (5, 2, 3), "{r:?}");
    assert_eq!(n.with_saved_error, 2, "an error on a cell that has no formula is counted, and not compared: {r:?}");
    // The formula `User.Formula`, and `Angle`'s `Inh`: the chain holds nothing, so it is left out as such.
    assert_eq!((r.cells, r.agreed, r.skipped.inherited_no_formula), (2, 1, 1), "{r:?}");
}

#[test]
fn a_deleted_row_or_section_is_not_a_candidate_and_is_counted() {
    let r = recalc_all(&doc_of(a_cell_of_every_kind()), &Budget::default());
    let (_, asked) = run_recorded(&doc_of(a_cell_of_every_kind()));
    let sections: Vec<&str> = asked.iter().filter_map(|a| a.addr.section.as_ref().map(|s| s.name.as_str())).collect();
    assert!(!sections.contains(&"Scratch") && !sections.contains(&"Prop"), "a deleted row and a deleted section are not asked for: {asked:?}");
    assert_eq!(r.not_formulas.deleted, 3);
}

// ---- E first (I6) -----------------------------------------------------------------------------------------------------------------

fn score_cell(c: Cell) -> RecalcReport {
    recalc_all(&doc_of(ShapeBuilder::new(1).set(user("X"), c)), &Budget::default())
}

#[test]
fn a_saved_error_is_compared_whatever_the_value_is() {
    // Themed, empty and absent values, and a date past the year 9999: no value to compare, and the error is still the file's word.
    let unused = |c: Cell| {
        let mut c = c;
        c.value = None;
        c
    };
    let cases: Vec<(&str, Cell, bool)> = vec![
        ("Themed with an E the formula does not give", cell("1+1", "Themed", None, Some("#N/A")), false),
        ("Themed with the E the formula gives", cell("1/0", "Themed", None, Some("#DIV/0!")), true),
        ("empty with an E the formula does not give", cell("1+1", "", None, Some("#N/A")), false),
        ("empty with the E the formula gives", cell("1/0", "", None, Some("#DIV/0!")), true),
        ("no V with an E the formula does not give", unused(cell("1+1", "x", None, Some("#N/A"))), false),
        ("no V with the E the formula gives", unused(cell("1/0", "x", None, Some("#DIV/0!"))), true),
        ("a far date with an E the formula does not give", cell("DATE(1899,12,29)", "30177-04-13T00:00:00", Some("DATE"), Some("#N/A")), false),
    ];
    for (label, c, agrees) in cases {
        let r = score_cell(c);
        assert_eq!((r.agreed, r.differed.len()), if agrees { (1, 0) } else { (0, 1) }, "{label}: {r:?}");
        assert_eq!((r.skipped.themed, r.skipped.far_year, r.skipped.nothing_saved), (0, 0, 0), "{label}: it was compared, not left out: {r:?}");
        if let Some(d) = r.differed.first() {
            assert_eq!(d.kind, DifferenceKind::Error, "{label}: only the error differs: {d:?}");
            assert_eq!((d.saved_error.as_deref(), d.actual_error.as_deref()), (Some("#N/A"), None), "{label}");
        }
    }
    // With no error saved either, the same cells are left out, for their own reasons.
    let r = score_cell(cell("1+1", "Themed", None, None));
    assert_eq!((r.skipped.themed, r.agreed, r.differed.len()), (1, 0, 0), "{r:?}");
    let r = score_cell(cell("DATE(1899,12,29)", "30177-04-13T00:00:00", Some("DATE"), None));
    assert_eq!((r.skipped.far_year, r.agreed, r.differed.len()), (1, 0, 0), "{r:?}");
    let r = score_cell(unused(cell("1+1", "x", None, None)));
    assert_eq!((r.skipped.nothing_saved, r.agreed, r.differed.len()), (1, 0, 0), "{r:?}");
}

#[test]
fn an_inherited_cell_with_an_error_and_no_value_is_compared_by_its_error() {
    let master = |formula: &str| MasterBuilder::new().shape(ShapeBuilder::new(5).formula("Width", formula, "2"));
    let instance = |error: &str| {
        let mut c = Cell::inherited(None);
        c.error = Some(Arc::from(error));
        PageBuilder::new().shape(ShapeBuilder::new(10).master(3).set(CellAddr::direct("Width"), c))
    };
    // The master's formula gives no error and the instance saved one: a difference. The master's own cell agrees.
    let doc = DocBuilder::new().master(3, master("1+1")).page(1, instance("#N/A")).build();
    let r = recalc_all(&doc, &Budget::default());
    assert_eq!((r.agreed, r.differed.len()), (1, 1), "{r:?}");
    assert_eq!((r.differed[0].kind, r.differed[0].inherited), (DifferenceKind::Error, true));
    // The master's formula gives the error the instance saved.
    let doc = DocBuilder::new().master(3, master("1/0")).page(1, instance("#DIV/0!")).build();
    let r = recalc_all(&doc, &Budget::default());
    assert_eq!(r.differed.len(), 1, "the master's own cell saved no error, so it differs; the instance agrees: {r:?}");
    assert_eq!((r.agreed, r.differed[0].inherited), (1, false));
}

// ---- what reads the machine, by address and not by name (I1) ---------------------------------------------------------------------

#[test]
fn a_cell_is_left_out_for_what_it_reads_and_not_for_the_name_it_shares() {
    // `User.F.Value` calls FILENAME. Cells that read it are left out wherever they sit; cells that share a name with it (`Value`, `Prompt`)
    // and read nothing of it are scored.
    let shape = ShapeBuilder::new(1)
        .set(user("F"), cell("FILENAME()", "a.vsdx", None, None))
        .set(CellAddr::direct("Comment"), cell("User.F", "a.vsdx", None, None))
        .set(CellAddr::in_row("User", None, RowKey::name("G"), "Prompt"), cell("User.F&\"!\"", "a.vsdx!", None, None))
        .set(user("G"), cell("1+1", "2", None, None))
        .set(CellAddr::in_row("Prop", None, RowKey::name("H"), "Value"), cell("2+2", "4", None, None))
        .set(CellAddr::in_row("Prop", None, RowKey::name("H"), "Label"), cell("\"x\"", "x", None, None));
    let r = recalc_all(&doc_of(shape), &Budget::default());
    assert_eq!((r.skipped.clock_readers, r.agreed, r.differed.len()), (3, 3, 0), "the caller, and the two cells that read it: {r:?}");
}

#[test]
fn a_cell_that_reads_a_text_with_a_machine_field_is_left_out_by_any_form_of_the_read() {
    // At second hand: `TheText` named in a test, not only `TEXTHEIGHT(TheText, ...)`.
    let shape = ShapeBuilder::new(1)
        .text("Plant")
        .row_formula("Field", None, RowKey::Ix(0), "Value", "DIRECTORY()", "c:/")
        .set(user("R"), cell("IF(TheText=\"x\",1,2)", "2", None, None))
        .set(user("S"), cell("LEN(TheText)", "5", None, None))
        .set(user("Clean"), cell("1+1", "2", None, None));
    let r = recalc_all(&doc_of(shape), &Budget::default());
    assert_eq!((r.skipped.clock_readers, r.agreed), (3, 1), "the field, and the two readers of the text: {r:?}");
}

#[test]
fn a_cell_that_inherits_a_machine_formula_from_its_master_is_left_out() {
    let doc = DocBuilder::new()
        .master(3, MasterBuilder::new().shape(ShapeBuilder::new(5).set(user("F"), cell("FILENAME()", "m.vsdx", None, None))))
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).master(3).set(user("F"), Cell::inherited(Some(DocValue::from_text("i.vsdx"))))))
        .build();
    let r = recalc_all(&doc, &Budget::default());
    assert_eq!((r.skipped.clock_readers, r.agreed, r.differed.len()), (2, 0, 0), "the master's cell and the instance that inherits it: {r:?}");
}

// ---- the rules of the specs, one cell each (I2) ----------------------------------------------------------------------------------

#[test]
fn every_oracle_rule_of_the_specs_is_a_reason_the_report_gives() {
    // NOW, RAND, the three DOC dates and DOCLASTPRINT; FILENAME and
    // DIRECTORY; a year past 9999; a colour under a scheme the file no longer selects;:
    // a step of one in a channel. Each is a count of its own.
    let mut shape = ShapeBuilder::new(1);
    for (i, f) in ["NOW", "RAND", "DOCCREATION", "DOCLASTSAVE", "DOCLASTEDIT", "DOCLASTPRINT", "FILENAME", "DIRECTORY"].iter().enumerate() {
        shape = shape.set(user(&format!("C{i}")), cell(&format!("{f}()"), "x", None, None));
    }
    // A colour made by MSOTINT, saved one step off in red: agreed, and counted as agreeing through an allowance.
    let computed = match crate::eval("MSOTINT(RGB(100,150,200),30)") {
        diacraft_calc::Value::Color(c) => c,
        other => panic!("{other:?}"),
    };
    let one_off = format!("#{:02x}{:02x}{:02x}", computed.r.wrapping_add(1), computed.g, computed.b);
    shape = shape
        .set(user("Far"), cell("DATE(1899,12,29)", "30177-04-13T00:00:00", Some("DATE"), None))
        .set(user("Step"), cell("MSOTINT(RGB(100,150,200),30)", &one_off, None, None))
        .set(CellAddr::direct("ColorSchemeIndex"), Cell::constant(DocValue::from_text("65534")))
        // The saved colour is the file's first theme part's `lt1`, and not the table's `Light` (the narrowed rule leaves out that kind only).
        .set(user("Stale"), cell(r#"THEMEVAL("Light")"#, "#112233", None, None));
    let mut doc = doc_of(shape);
    let mut part = diacraft_testkit::theme::ThemeXml::default();
    part.colours[1] = "112233";
    doc.themes.push(part.part());
    let r = recalc_all(&doc, &Budget::default());
    assert_eq!(r.skipped.clock_readers, 8, "{r:?}");
    assert_eq!(r.skipped.far_year, 1, "{r:?}");
    assert_eq!(r.skipped.stale_theme_colour, 1, "{r:?}");
    assert_eq!((r.agreed, r.differed.len(), r.agreed_by_allowance), (1, 0, 1), "{r:?}");
    assert_eq!(r.accounted(), r.cells, "{r:?}");
    // A date a whole number of quarter hours off the core property fits; the report says so beside the skip.
    let mut doc = doc_of(ShapeBuilder::new(1).set(user("D"), cell("DOCCREATION()", "2031-03-07T05:05:09", Some("DATE"), None)));
    Arc::make_mut(&mut doc.props).created = Some(Arc::from("2031-03-07T09:05:09Z"));
    let r = recalc_all(&doc, &Budget::default());
    assert_eq!((r.skipped.clock_readers, r.document_date_checks.fit, r.document_date_checks.not_fit), (1, 1, 0), "{r:?}");
}

// ---- a calculator that hands back the saved values (I3) --------------------------------------------------------------------------

#[test]
fn a_calculator_that_echoes_the_saved_values_agrees_with_every_cell_and_the_report_cannot_say_so() {
    // Three cells whose saved values are wrong, one by value, one by error and one by an error the formula does not give.
    let shape = ShapeBuilder::new(1)
        .set(user("A"), cell("1+1", "3", None, None))
        .set(user("B"), cell("1/0", "1", None, None))
        .set(user("C"), cell("1+1", "2", None, Some("#N/A")));
    let doc = doc_of(shape);
    let native = recalc_all_using(&doc, &Budget::default(), diacraft_calc::EvalOptions::default(), &mut Native);
    assert_eq!((native.agreed, native.differed.len()), (0, 3), "{native:?}");
    let echo = recalc_all_using(&doc, &Budget::default(), diacraft_calc::EvalOptions::default(), &mut Echo);
    assert_eq!((echo.agreed, echo.differed.len()), (3, 0), "an echo scores 1.0: only a control document with wrong values can tell: {echo:?}");
}

// ---- a sheet the model cannot tell from another (M9) -----------------------------------------------------------------------------

#[test]
fn a_page_master_or_style_with_an_id_an_earlier_one_has_is_left_out_with_its_shapes() {
    let doc = DocBuilder::new()
        .style(7, StyleBuilder::new().formula("LineWeight", "1+1", "2"))
        .style(7, StyleBuilder::new().formula("LineWeight", "1+1", "9"))
        .master(3, MasterBuilder::new().formula("PageWidth", "1+1", "2").shape(ShapeBuilder::new(5).formula("Width", "1+1", "2")))
        .master(3, MasterBuilder::new().formula("PageWidth", "1+1", "9").shape(ShapeBuilder::new(5).formula("Width", "1+1", "9")))
        .page(1, PageBuilder::new().formula("PageHeight", "1+1", "2").shape(ShapeBuilder::new(10).formula("Width", "1+1", "2")))
        .page(1, PageBuilder::new().formula("PageHeight", "1+1", "9").shape(ShapeBuilder::new(10).formula("Width", "1+1", "9")))
        .build();
    let (r, asked) = run_recorded(&doc);
    // Five formula cells in the first of each kind, scored and agreeing; five in the later ones, left out and not scored against the first's.
    assert_eq!((r.cells, r.agreed, r.differed.len(), r.skipped.ambiguous_sheet), (10, 5, 0, 5), "{r:?}");
    assert_eq!(asked.len(), 5, "the later sheets are not asked for: {asked:?}");
    assert_eq!(r.cells_in_document(), cells_in(&doc), "and they are counted: {r:?}");
}

// ---- the allowance a score rests on (M8) ------------------------------------------------------------------------------------------

#[test]
fn a_cell_that_agrees_only_through_an_allowance_is_counted_apart() {
    // 1,000,000.0005 against 1,000,000.0004 is 1E-4 apart: inside a relative 1E-9 of its size, outside an absolute one. The geometry batch's
    // 1E-6 is an allowance too. An exact match, a gap under 1E-9, and a text match are not. (A colour step is in the test of the rules.)
    let shape = ShapeBuilder::new(1)
        .set(user("Exact"), cell("2+2", "4", None, None))
        .set(user("Tiny"), cell("0.1+0.2", "0.3", None, None))
        .set(user("Text"), cell("\"a\"&\"b\"", "ab", Some("STR"), None))
        .set(user("Big"), cell("1000000+0.0005", "1000000.0004", None, None))
        .set(user("Geometry"), cell("PNTX(PNT(3,4))", "3.0000001", None, None));
    let r = recalc_all(&doc_of(shape), &Budget::default());
    assert_eq!((r.agreed, r.differed.len(), r.agreed_by_allowance), (5, 0, 2), "{r:?}");
}

#[test]
fn a_cell_tally_names_the_kind_of_cell_and_not_the_row() {
    let shape = ShapeBuilder::new(1)
        .set(user("A"), cell("1+1", "2", None, None))
        .set(user("B"), cell("1+1", "3", None, None))
        .set(CellAddr::direct("Width"), cell("1+1", "2", None, None));
    let r = recalc_all(&doc_of(shape), &Budget::default());
    let score = |s: &str, c: &str| r.by_cell.get(&(s.to_owned(), c.to_owned())).map(|x| (x.agreed, x.differed));
    assert_eq!(score("User", "Value"), Some((1, 1)));
    assert_eq!(score("", "Width"), Some((1, 0)));
    assert_eq!(r.by_cell.len(), 2, "no row names in the keys: {:?}", r.by_cell);
}
