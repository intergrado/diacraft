//! SECNAME: the handles of a custom shape the `.odg` reader makes are the model's `Control` section, so that the formulas the reader writes for them
//! (`Controls.Row_1.X`, where `Controls` is how a formula spells the section) resolve to it and calc works the modifier a handle drives out. The reader built a
//! section named by the formula's word, `Controls`; no reference found it, and a handle's drag moved nothing the file's `draw:modifiers` follows.
//!
//! The drawing is written by hand; the model is the reader's; the recalculation is calc's; the save is the writer's.

use diacraft_calc::{Budget, CellAddress, Changes, EvalCtx, EvalOptions, Value as Calc, commit_changed, evaluate};
use diacraft_doc::{Cell, CellAddr, CellEdit, Document, Lookup, Owner, RowKey, SectionKey, ShapeRef, SheetRef, Value, WriteOrigin};
use diacraft_odg::{Limits, OriginalPackage, WriteOptions, read, write_with};

use crate::odg_build::{Drawing, content_of};

const MODIFIERS: &str = r#"draw:modifiers="5400 7""#;

/// A custom shape 4 cm square at (1 cm, 2 cm) with one handle that drives the modifier 0 along x.
fn drawing() -> Vec<u8> {
    let body = r#"<draw:custom-shape draw:name="h" draw:layer="layout" svg:x="1cm" svg:y="2cm" svg:width="4cm" svg:height="4cm"><text:p/><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:modifiers="5400 7" draw:enhanced-path="M 0 0 L $0 0 21600 21600 Z N"><draw:handle draw:handle-position-x="$0" draw:handle-position-y="top"/></draw:enhanced-geometry></draw:custom-shape>"#;
    Drawing::new().page(body).bytes()
}

/// The sheet of the one shape of the first page.
fn sheet_of(doc: &Document) -> SheetRef {
    SheetRef::Shape(ShapeRef::top(Owner::Page(doc.pages.get(0).unwrap().id), 0))
}

fn handle(cell: &str) -> CellAddr {
    CellAddr::in_row("Control", None, RowKey::name("Row_1"), cell)
}

fn modifier() -> CellAddr {
    CellAddr::in_row("User", None, RowKey::name("ODFMod0"), "Value")
}

/// The cell of the one shape of the first page, as the model holds it.
fn held(doc: &Document, addr: &CellAddr) -> Option<Cell> {
    match doc.sheet_of(&sheet_of(doc))?.lookup(addr) {
        Lookup::Found(c) => Some(c.clone()),
        _ => None,
    }
}

/// What calc works the cell out to, as a plain number.
fn worked_out(doc: &Document, addr: &CellAddr) -> f64 {
    match evaluate(doc, &CellAddress::new(sheet_of(doc), addr.clone()), &mut EvalCtx::default()) {
        Calc::Number(q) => q.stored,
        other => panic!("{addr:?} is not a number: {other:?}"),
    }
}

/// The save of `doc` over `bytes`, which `report`'s index stands for.
fn saved(doc: &Document, bytes: &[u8], index: &diacraft_odg::ShapeIndex) -> diacraft_odg::Written {
    write_with(doc, Some(&OriginalPackage { bytes, index }), &WriteOptions::default()).unwrap_or_else(|e| panic!("not saved: {e}"))
}

/// The reader makes the handles' section `Control`, and nothing is left under the word a formula spells it with.
#[test]
fn the_reader_names_the_handles_section_as_the_model_does() {
    let (doc, _) = read(&drawing(), &Limits::default()).unwrap();
    let shape = doc.pages.get(0).unwrap().shapes.get(0).unwrap().clone();
    assert!(shape.sheet.section(&SectionKey::new("Control", None)).is_some_and(|s| s.rows.len() == 1), "the handle's row is in `Control`");
    assert!(shape.sheet.section(&SectionKey::new("Controls", None)).is_none(), "no section is named by the formula's word");
    assert!(diacraft_doc::SECTION_NAMES.contains(&"Control"));
    let formula = held(&doc, &modifier()).and_then(|c| c.formula.text().map(str::to_owned));
    assert_eq!(formula.as_deref(), Some("Controls.Row_1.X/Width*21600"), "the formula spells the section the way a reference does");
}

/// A driven modifier is worked out by calc on a shape the reader made (the formula `Controls.Row_1.X/Width*21600` finds `Control.Row_1.X`), a drag of the handle
/// moves it, the save writes the one attribute that holds it, and the file saved again with no edit is the same bytes.
#[test]
fn a_driven_modifier_follows_a_dragged_handle_through_calc_the_save_and_a_second_save_that_is_no_edit() {
    let bytes = drawing();
    let (mut doc, report) = read(&bytes, &Limits::default()).unwrap();
    let sheet = sheet_of(&doc);
    // The formulas answer: the handle is a quarter of the way across, and the modifier is 5400 of 21600.
    let x = worked_out(&doc, &handle("X"));
    assert!(worked_out(&doc, &handle("XDyn")) > 0.0 && (worked_out(&doc, &handle("XDyn")) - x).abs() < 1e-9, "`Controls.Row_1.X` is the handle's X");
    assert!((worked_out(&doc, &modifier()) - 5400.0).abs() < 1e-6, "the modifier works out to the file's 5400");
    // Nothing is edited: the file saved from the model is the file.
    let same = saved(&doc, &bytes, &report.index);
    assert!(!same.edited);
    // The drag: the handle goes to half of where it was, and what reads it is worked out.
    let half = Value::from_number(x / 2.0).unwrap();
    doc.write_cell(&sheet, &handle("X"), CellEdit::Value(half), WriteOrigin::Drawing).unwrap();
    let page = doc.pages.get(0).unwrap().id;
    let changes = Changes { cells: vec![(sheet.clone(), handle("X"))], ..Changes::default() };
    let done = commit_changed(&mut doc, &Budget::default(), EvalOptions::default(), page, &changes);
    assert!(!done.budget_exhausted && !done.memory_exhausted);
    assert!((worked_out(&doc, &modifier()) - 2700.0).abs() < 1e-6, "the modifier follows the handle: half of 5400");
    let cached = held(&doc, &modifier()).and_then(|c| c.value.as_ref()?.number());
    assert!(cached.is_some_and(|v| (v - 2700.0).abs() < 1e-6), "and calc wrote it back to the cell: {cached:?}");
    // The save writes the modifier and nothing else of the file's content, and carries no section beside it.
    let out = saved(&doc, &bytes, &report.index);
    assert!(out.edited);
    let (before, after) = (content_of(&bytes), content_of(&out.bytes));
    assert_eq!(after, before.replacen(MODIFIERS, r#"draw:modifiers="2700 7""#, 1), "the one attribute that holds the modifier");
    assert!(!after.contains("diacraft:s-Control"), "the handles' section is not carried beside the modifier: {after}");
    // The file read again holds the handle where the model had it, and a second save of it is no edit.
    let (back, back_report) = read(&out.bytes, &Limits::default()).unwrap();
    assert!((worked_out(&back, &modifier()) - 2700.0).abs() < 1e-6);
    assert!((worked_out(&back, &handle("X")) - x / 2.0).abs() < 1e-9, "the handle is where the drag left it");
    let again = saved(&back, &out.bytes, &back_report.index);
    assert!(!again.edited, "a save with no edit");
    assert_eq!(again.bytes, out.bytes);
}
