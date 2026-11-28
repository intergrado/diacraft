//! Setting cells: `sheet.set_cell` and `sheet.set_cells`.
//!
//! The rules are named in `docs/editing-commands.md`, "The ShapeSheet (SHEETWIN)": what a write does, a typed number
//! (a cell must be there), the limits. The spec is .
//! A cost is held to a count, never to a time.

use std::sync::Arc;

use diacraft_doc::{Cell, CellAddr, Document, Owner, Resolver, RowKey, SheetRef, Value as Held};
use diacraft_engine::{EngineError, Session};
use diacraft_testkit::build::{DocBuilder, MasterBuilder, PageBuilder, ShapeBuilder, StyleBuilder};
use serde_json::{Value, json};

use crate::edit::{doc, drop_shape, id_of, route};
use crate::{blank_session, check_session, run, snapshot};

/// A session whose document is `doc`.
fn session_of(doc: Document) -> Session {
    let mut s = blank_session();
    s.edit("build", |d| {
        *d = doc;
        Ok(())
    })
    .unwrap();
    s
}

/// A session with one page and one shape, 10, on it.
fn one_shape(shape: ShapeBuilder) -> Session {
    session_of(DocBuilder::new().page(1, PageBuilder::new().name("Page-1").shape(shape)).build())
}

/// The words of a refusal.
fn refused(s: &mut Session, id: &str, params: Value) -> String {
    s.execute(id, &params).unwrap_err().to_string()
}

/// `sheet.cell` of a cell of shape 10.
fn cell(s: &mut Session, name: &str) -> Value {
    run(s, "sheet.cell", json!({"id": 10, "cell": name}))
}

/// `sheet.set_cell` on shape 10.
fn set(s: &mut Session, name: &str, formula: &str) -> Value {
    run(s, "sheet.set_cell", json!({"id": 10, "cell": name, "formula": formula}))
}

fn undo_len(s: &Session) -> usize {
    s.history().map_or(0, |h| h.undo_len())
}

/// The document and the history as they are, without the journal (a call that changes nothing is still a call).
fn state(s: &Session) -> (Option<Document>, usize, usize, u64) {
    let snap = snapshot(s);
    (snap.document, snap.undo_steps, snap.redo_steps, snap.revision)
}

/// The number the cell at `addr` of shape 10 holds, read as the model holds it (inches, radians), through inheritance.
fn held_number(s: &Session, name: &str) -> f64 {
    let d = doc(s);
    let sheet = SheetRef::Shape(d.find_shape(Owner::Page(1), 10).unwrap());
    Resolver::new(d).resolve(&sheet, &CellAddr::direct(name)).unwrap().value.cell.unwrap().value.as_ref().unwrap().number().unwrap()
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1.0e-9
}

const ME: Value = Value::Null;

// ---- the answer and the history ---------------------------------------------------------------------------------------------------------

/// V1 of the inventory and V9 of the window's spec: typing `3` in a Width holding `2 in` gives `3 in`; the answer names the sheet and the cell, says it changed, gives the cell as the
/// sheet shows it, and the write is one undo step named for what it did. A leading `=` and the white space around the text are cut.
#[test]
fn typing_a_bare_number_in_a_width_holding_two_inches_gives_three_inches_in_one_step() {
    let mut s = one_shape(ShapeBuilder::new(10).cell_u("Width", "2", "IN"));
    let before = undo_len(&s);
    let answer = set(&mut s, "Width", "3");
    assert_eq!(answer["sheet"], json!({"kind": "shape", "page": 1, "id": 10}));
    assert_eq!(answer["cell"], "Width");
    assert_eq!(answer["changed"], true);
    assert_eq!((answer["value"].clone(), answer["unit"].clone(), answer["shown"].clone()), (json!(3.0), json!("IN"), json!("3.0000 in")));
    assert_eq!((answer["origin"].clone(), answer["error"].clone(), answer["worked_out"].clone()), (json!("local"), ME, json!(true)));
    assert_eq!(answer["formula"], "3 in", "a value with no formula shows as a literal with its unit");
    assert_eq!(undo_len(&s), before + 1, "one undo step");
    assert_eq!(s.history().unwrap().next_undo_label(), Some("Edit formula"));
    let read = cell(&mut s, "Width");
    assert_eq!((read["value"].clone(), read["unit"].clone(), read["origin"].clone()), (json!(3.0), json!("IN"), json!("local")));
    assert_eq!(held_number(&s, "Width"), 3.0);
    // The same cell again, typed as a formula would be: `=` and blanks go.
    let again = set(&mut s, "Width", "  = 4 in ");
    assert_eq!((again["value"].clone(), again["changed"].clone()), (json!(4.0), json!(true)));
    assert_eq!(check_session(&s), Ok(()));
}

/// V1 in millimetres: a bare number is read in the unit the cell shows, so `3` in a cell showing `50 mm` is 3 mm, stored as its inches with `U` `MM`; a number typed with its unit
/// keeps that unit; a cell with no previous unit takes the default unit of its kind; a flag takes the number as it is.
#[test]
fn a_bare_number_is_read_in_the_unit_the_cell_shows_and_a_typed_unit_is_kept() {
    let fifty_mm = (50.0_f64 / 25.4).to_string();
    let mut s = one_shape(
        ShapeBuilder::new(10).cell_u("Width", &fifty_mm, "MM").cell("Angle", "0").cell_u("FillForegndTrans", "0.5", "PER").cell("FlipX", "0"),
    );
    let answer = set(&mut s, "Width", "3");
    assert_eq!((answer["value"].clone(), answer["unit"].clone(), answer["shown"].clone()), (json!(3.0), json!("MM"), json!("3.0000 mm")));
    assert!(near(held_number(&s, "Width"), 3.0 / 25.4), "stored as inches: {}", held_number(&s, "Width"));
    // A number with a unit of its own keeps it for display, whatever the cell showed.
    let inches = set(&mut s, "Width", "3 in");
    assert_eq!((inches["value"].clone(), inches["unit"].clone()), (json!(3.0), json!("IN")));
    assert_eq!(held_number(&s, "Width"), 3.0);
    let cm = set(&mut s, "Width", "2.54 cm");
    assert_eq!((cm["value"].clone(), cm["unit"].clone()), (json!(2.54), json!("CM")));
    assert!(near(held_number(&s, "Width"), 1.0), "2.54 cm are one inch exactly");
    let negative = set(&mut s, "Width", "-0.25 mm");
    assert_eq!((negative["value"].clone(), negative["unit"].clone()), (json!(-0.25), json!("MM")));

    // No unit before and one needed: the default unit of the kind (a length in inches, an angle in degrees).
    let height = set(&mut s, "Height", "4");
    assert_eq!((height["value"].clone(), height["unit"].clone(), height["shown"].clone()), (json!(4.0), json!("IN"), json!("4.0000 in")));
    assert_eq!(held_number(&s, "Height"), 4.0);
    let angle = set(&mut s, "Angle", "90");
    assert_eq!((angle["value"].clone(), angle["unit"].clone()), (json!(90.0), json!("DEG")));
    assert!(near(held_number(&s, "Angle"), std::f64::consts::FRAC_PI_2), "stored in radians: {}", held_number(&s, "Angle"));
    // A cell that shows a percentage reads the bare number as a percentage; `25%` is one too.
    let ten = set(&mut s, "FillForegndTrans", "10");
    assert_eq!((ten["value"].clone(), ten["unit"].clone()), (json!(10.0), json!("PER")));
    assert!(near(held_number(&s, "FillForegndTrans"), 0.1));
    let quarter = set(&mut s, "FillForegndTrans", "25%");
    assert!(near(held_number(&s, "FillForegndTrans"), 0.25), "{quarter}");
    // A cell with no unit takes the number as it is.
    let flip = set(&mut s, "FlipX", "1");
    assert_eq!((flip["value"].clone(), flip["unit"].clone()), (json!(1.0), ME));
    assert_eq!(check_session(&s), Ok(()));
}

/// An unchanged text makes no step: the document, the history and the revision stay as they were, and the answer says `changed: false`.
#[test]
fn a_text_that_is_the_cells_own_makes_no_step() {
    let mut s = one_shape(ShapeBuilder::new(10).cell_u("Width", "2", "IN").formula("Height", "Width*2", "4").cell("PinX", "1"));
    let literal = set(&mut s, "Width", "2 in");
    assert_eq!(literal["changed"], false, "{literal}");
    let formula = set(&mut s, "Height", "Width*2");
    assert_eq!(formula["changed"], false);
    let bare = set(&mut s, "Width", "2");
    assert_eq!(bare["changed"], false, "a bare number in inches is the value it already is");
    let spaced = set(&mut s, "Height", " =Width*2 ");
    assert_eq!(spaced["changed"], false, "the same formula, cut as a write cuts it");
    let clear = set(&mut s, "PinY", "");
    assert_eq!(clear["changed"], false, "a cell with no formula of its own is not changed by a clear");
    let anchor = state(&s);
    for (name, text) in [("Width", "2 in"), ("Height", "Width*2"), ("PinY", "")] {
        set(&mut s, name, text);
    }
    assert_eq!(state(&s), anchor, "no step, no revision, no change");
    // A change is a step.
    let moved = set(&mut s, "Height", "Width*3");
    assert_eq!(moved["changed"], true);
    assert_eq!(undo_len(&s), anchor.1 + 1);
}

/// Undo takes the write back, with what it recalculated, and redo puts it again.
#[test]
fn undo_takes_a_write_and_what_it_worked_out_back_and_redo_puts_it_again() {
    let mut s = one_shape(ShapeBuilder::new(10).cell_u("Width", "2", "IN").formula("LocPinX", "Width*0.5", "1"));
    let before = state(&s);
    set(&mut s, "Width", "4");
    assert_eq!(held_number(&s, "LocPinX"), 2.0);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(held_number(&s, "Width"), 2.0);
    assert_eq!(held_number(&s, "LocPinX"), 1.0);
    assert_eq!(doc(&s), before.0.as_ref().unwrap(), "the document is the one before");
    let old = cell(&mut s, "Width");
    assert_eq!(old["formula"], "2 in", "undo shows the old formula");
    run(&mut s, "edit.redo", json!({}));
    assert_eq!((held_number(&s, "Width"), held_number(&s, "LocPinX")), (4.0, 2.0));
}

/// What a write recalculates: `LocPinX` `Width*0.5` follows a write of Width, and the answer lists the cells that changed by it (not the one written).
#[test]
fn a_cell_that_reads_the_written_cell_is_worked_out_and_listed() {
    let mut s = one_shape(ShapeBuilder::new(10).cell_u("Width", "2", "IN").formula("LocPinX", "Width*0.5", "1").formula("Height", "Width*3", "6"));
    let answer = set(&mut s, "Width", "4");
    assert_eq!((held_number(&s, "LocPinX"), held_number(&s, "Height")), (2.0, 12.0));
    let r = &answer["recalculated"];
    assert_eq!(r["cells"], 2, "{answer}");
    assert_eq!(r["more"], false);
    let listed: Vec<(Value, Value)> = r["listed"].as_array().unwrap().iter().map(|c| (c["sheet"].clone(), c["cell"].clone())).collect();
    let shape = json!({"kind": "shape", "page": 1, "id": 10});
    assert!(listed.contains(&(shape.clone(), json!("LocPinX"))) && listed.contains(&(shape.clone(), json!("Height"))), "{listed:?}");
    assert!(!listed.iter().any(|(_, c)| c == "Width"), "the cell written is not a recalculated one: {listed:?}");
    // A formula is written as typed, and worked out.
    let formula = set(&mut s, "Height", "Width * 2");
    assert_eq!((formula["formula"].clone(), formula["value"].clone(), formula["origin"].clone()), (json!("Width * 2"), json!(8.0), json!("local")));
}

/// The listed cells are at most 256, with the total and `more`.
#[test]
fn the_cells_a_write_changed_are_listed_up_to_256_with_the_total() {
    let mut shape = ShapeBuilder::new(10).cell_u("Width", "2", "IN");
    for k in 0..300_u32 {
        shape = shape.row_formula("Scratch", None, RowKey::Ix(k), "A", "Width*2", "4");
    }
    let mut s = one_shape(shape);
    let answer = set(&mut s, "Width", "3");
    let r = &answer["recalculated"];
    assert!(r["cells"].as_u64().unwrap() >= 300, "{r}");
    assert_eq!(r["listed"].as_array().unwrap().len(), 256);
    assert_eq!(r["more"], true);
    assert_eq!(cell(&mut s, "Scratch.A300")["value"], json!(6.0));
}

// ---- an empty formula: it returns to inheriting ------------------------------------------------------------------------------------------

/// V2 of the inventory: an instance that overrides Width follows its master again after an empty formula, however the master holds it (a plain value or a formula).
#[test]
fn an_empty_formula_on_an_instances_width_follows_the_master_again() {
    // A dropped Process shape: Width is the master's until it is resized.
    let mut s = blank_session();
    let id = id_of(&run(&mut s, "shape.drop", json!({"master": "process", "page": 1, "x": 5.0, "y": 4.0})));
    let own = |s: &mut Session, name: &str| run(s, "sheet.cell", json!({"id": id, "cell": name}));
    let master_width = own(&mut s, "Width");
    assert_eq!(master_width["origin"], "inherited");
    run(&mut s, "shape.resize", json!({"id": id, "x": 3.0, "y": 3.0, "width": 3.5, "height": 1.0}));
    assert_eq!(own(&mut s, "Width")["origin"], "local");
    assert_eq!(own(&mut s, "Width")["value"], json!(3.5));
    let answer = run(&mut s, "sheet.set_cell", json!({"id": id, "cell": "Width", "formula": ""}));
    assert_eq!(answer["changed"], true);
    assert_eq!(s.history().unwrap().next_undo_label(), Some("Clear formula"));
    let back = own(&mut s, "Width");
    assert_eq!((back["origin"].clone(), back["value"].clone()), (json!("inherited"), master_width["value"].clone()), "{back}");
    assert_eq!(back["from"]["kind"], "master");
    assert_eq!(answer["origin"], "inherited");
    assert_eq!(answer["value"], master_width["value"]);
    assert_eq!(check_session(&s), Ok(()));

    // A local formula over the master's: the master's formula is worked out on the instance again.
    let master = MasterBuilder::new().shape(ShapeBuilder::new(5).cell("Height", "2").formula("Width", "Height*2", "4"));
    let page = PageBuilder::new().shape(ShapeBuilder::new(10).master(1).formula("Width", "1+1", "2").cell("Height", "5"));
    let mut s = session_of(DocBuilder::new().master(1, master).page(1, page).build());
    let cleared = set(&mut s, "Width", "");
    assert_eq!(
        (cleared["origin"].clone(), cleared["formula"].clone(), cleared["value"].clone()),
        (json!("inherited"), json!("Height*2"), json!(10.0))
    );
    assert_eq!(held_number(&s, "Width"), 10.0, "the master's formula, on the instance's own Height");
}

/// V2 of the inventory, second half: a Scratch cell with a local formula and nothing inherited reads `No Formula`, keeping the last value.
#[test]
fn an_empty_formula_on_a_scratch_cell_with_nothing_to_inherit_reads_no_formula() {
    let mut s = one_shape(ShapeBuilder::new(10).row_formula("Scratch", None, RowKey::Ix(0), "A", "1+1", "2"));
    let answer = set(&mut s, "Scratch.A1", "");
    assert_eq!((answer["formula"].clone(), answer["changed"].clone(), answer["origin"].clone()), (json!("No Formula"), json!(true), json!("local")));
    assert_eq!(answer["value"], json!(2.0), "the last value stays");
    assert_eq!(cell(&mut s, "Scratch.A1")["formula"], "No Formula");
    // `=` alone is empty too.
    let mut s = one_shape(ShapeBuilder::new(10).row_formula("Scratch", None, RowKey::Ix(0), "A", "1+1", "2"));
    assert_eq!(set(&mut s, "Scratch.A1", " = ")["formula"], "No Formula");
}

// ---- errors ------------------------------------------------------------------------------------------------------------------------------

/// V4 of the inventory: a formula dividing by zero reads `#DIV/0!`. The cell keeps its last good value, and a cell that reads it reads that value (of the formula
/// language: a reference to a failing cell with a value to fall back on does not carry the error), which is calc's rule and not this command's.
#[test]
fn a_formula_dividing_by_zero_reads_div_zero_and_a_cell_reading_it_reads_its_last_good_value() {
    let shape = ShapeBuilder::new(10).row_formula("Scratch", None, RowKey::Ix(0), "A", "1+1", "2").row_formula(
        "Scratch",
        None,
        RowKey::Ix(0),
        "B",
        "Scratch.A1*5",
        "10",
    );
    let mut s = one_shape(shape);
    let answer = set(&mut s, "Scratch.A1", "1/0");
    assert_eq!((answer["error"].clone(), answer["shown"].clone(), answer["changed"].clone()), (json!("#DIV/0!"), json!("#DIV/0!"), json!(true)));
    assert_eq!(answer["value"], json!(2.0), "the last good value");
    assert_eq!(cell(&mut s, "Scratch.A1")["error"], "#DIV/0!");
    let reader = cell(&mut s, "Scratch.B1");
    assert_eq!((reader["value"].clone(), reader["error"].clone()), (json!(10.0), ME), "R56");
    // The error goes when the formula is mended.
    let mended = set(&mut s, "Scratch.A1", "1+3");
    assert_eq!((mended["error"].clone(), mended["value"].clone()), (ME, json!(4.0)));
    assert_eq!(cell(&mut s, "Scratch.B1")["value"], json!(20.0));
    // A cell that is not there yet and fails reads the error all the same.
    let new = set(&mut s, "Height", "1/0");
    assert_eq!(new["error"], "#DIV/0!", "{new}");
    assert_eq!(cell(&mut s, "Height")["error"], "#DIV/0!");
}

/// A formula that does not parse is refused with the kind of the fault, its byte offset and words; nothing changes (the editor marks the fault).
#[test]
fn a_formula_that_does_not_parse_is_refused_with_its_fault_and_changes_nothing() {
    let mut s = one_shape(ShapeBuilder::new(10).cell_u("Width", "2", "IN"));
    let anchor = state(&s);
    let e = refused(&mut s, "sheet.set_cell", json!({"id": 10, "cell": "Width", "formula": "1/"}));
    assert!(e.contains("the formula has an error: the formula ends too soon: something is due at character 3"), "{e}");
    assert!(e.contains("unexpected_end") && e.contains("byte 2"), "the kind and the byte offset are in the words: {e}");
    // The offset is in the text as typed, a leading `=` and blanks counted.
    let e = refused(&mut s, "sheet.set_cell", json!({"id": 10, "cell": "Width", "formula": " =1+*2"}));
    assert!(e.contains("a value is due at character 5") && e.contains("expected_operand") && e.contains("byte 4"), "{e}");
    assert_eq!(state(&s), anchor);
    // A text with an unknown function parses: it is a call.
    let unknown = set(&mut s, "Height", "FOOBAR(1)");
    assert_eq!(
        (unknown["changed"].clone(), unknown["worked_out"].clone(), unknown["formula"].clone()),
        (json!(true), json!(false), json!("FOOBAR(1)"))
    );
}

// ---- GUARD ------------------------------------------------------------------------------------------------------------------------------------

/// V9 of the window's spec and R43: a guarded formula is not replaced by a call without `force`, in words, and is by one with it.
#[test]
fn a_guarded_cell_needs_force_and_is_replaced_with_it() {
    let mut s = one_shape(ShapeBuilder::new(10).formula("Width", "GUARD(2 in)", "2").formula("Height", "GUARD(Width)", "2"));
    let anchor = state(&s);
    let e = refused(&mut s, "sheet.set_cell", json!({"id": 10, "cell": "Width", "formula": "3"}));
    assert!(e.contains("the cell Width is guarded by GUARD(): send force: true to replace it"), "{e}");
    assert_eq!(state(&s), anchor, "refused: nothing changes");
    let e = refused(&mut s, "sheet.set_cell", json!({"id": 10, "cell": "Width", "formula": "", "force": false}));
    assert!(e.contains("guarded by GUARD()"), "an empty formula is a write too: {e}");
    let forced = run(&mut s, "sheet.set_cell", json!({"id": 10, "cell": "Width", "formula": "3", "force": true}));
    assert_eq!((forced["value"].clone(), forced["formula"].clone(), forced["guarded"].clone()), (json!(3.0), json!("3 in"), ME));
    assert_eq!(held_number(&s, "Width"), 3.0);
    // A formula typed over a guarded one is one too, and keeps it guarded if it says so.
    let guarded_again = run(&mut s, "sheet.set_cell", json!({"id": 10, "cell": "Height", "formula": "GUARD(Width*2)", "force": true}));
    assert_eq!(guarded_again["value"], json!(6.0));
    assert_eq!(cell(&mut s, "Height")["guarded"], true);
    // The refusal names the write that was refused in a batch, and the batch changes nothing.
    let anchor = state(&s);
    let e = refused(&mut s, "sheet.set_cells", json!({"id": 10, "writes": [{"cell": "Width", "formula": "4"}, {"cell": "Height", "formula": "5"}]}));
    assert!(e.contains("write 2:") && e.contains("the cell Height is guarded by GUARD()"), "{e}");
    assert_eq!(state(&s), anchor);
}

// ---- where a write may go: ---------------------------------------------------------------------------------------------

/// A cell must be there: one the sheet holds or inherits, or that the catalogue lists at that place for that kind of sheet. Any other is refused in words.
#[test]
fn a_cell_that_is_not_there_is_refused_in_words() {
    let shape =
        ShapeBuilder::new(10).row_cell("User", None, RowKey::name("Rate"), "Value", "5").row_cell("Geometry", Some(0), RowKey::Ix(1), "X", "0");
    let mut s = one_shape(shape);
    let anchor = state(&s);
    let words = |s: &mut Session, params: Value| refused(s, "sheet.set_cell", params);
    let e = words(&mut s, json!({"id": 10, "cell": "User.Total.Value", "formula": "1"}));
    assert!(e.contains("the sheet has no cell User.Total.Value: add the row first (sheet.add_row)"), "{e}");
    let e = words(&mut s, json!({"id": 10, "cell": "Prop.Cost.Value", "formula": "1"}));
    assert!(e.contains("the sheet has no cell Prop.Cost.Value: add the row first (sheet.add_row)"), "{e}");
    assert!(e.contains("sheet.add_section"), "a row of a section the sheet has not got needs the section too: {e}");
    let e = words(&mut s, json!({"id": 10, "cell": "Geometry2.NoFill", "formula": "1"}));
    assert!(e.contains("the sheet has no cell Geometry2.NoFill: add the section first (sheet.add_section)"), "{e}");
    // A page-sheet cell is no cell of a shape; a cell no batch lists is no cell anyone's reader knows.
    let e = words(&mut s, json!({"id": 10, "cell": "PageWidth", "formula": "5"}));
    assert!(e.contains("the sheet has no cell PageWidth"), "{e}");
    let e = words(&mut s, json!({"id": 10, "cell": "NoSuchCell", "formula": "5"}));
    assert!(e.contains("the sheet has no cell NoSuchCell"), "{e}");
    // A cell of a row the sheet has, that the catalogue does not list for the section, is not made either.
    let e = words(&mut s, json!({"id": 10, "cell": "User.Rate.Nonsense", "formula": "5"}));
    assert!(e.contains("the sheet has no cell User.Rate.Nonsense"), "{e}");
    assert_eq!(state(&s), anchor, "nothing was made");
    assert_eq!(check_session(&s), Ok(()));
}

/// The cells a catalogue lists for a kind of sheet may be made on it: a page-sheet cell on a page and a master's page sheet, a style flag on a style, a cell of a row that is there.
#[test]
fn a_cell_the_catalogue_lists_for_the_kind_of_sheet_may_be_made_on_it() {
    let master = MasterBuilder::new().shape(ShapeBuilder::new(5));
    let doc = DocBuilder::new()
        .style(3, StyleBuilder::new().name("Plain"))
        .master(2, master)
        .page(1, PageBuilder::new().cell("PageWidth", "11").shape(ShapeBuilder::new(10).row_cell("Geometry", Some(0), RowKey::Ix(1), "X", "0")))
        .build();
    let mut s = session_of(doc);
    let page = run(&mut s, "sheet.set_cell", json!({"page": 1, "cell": "PageHeight", "formula": "8.5"}));
    assert_eq!((page["value"].clone(), page["unit"].clone(), page["sheet"].clone()), (json!(8.5), json!("IN"), json!({"kind": "page", "page": 1})));
    let master_page = run(&mut s, "sheet.set_cell", json!({"master": 2, "cell": "PageWidth", "formula": "4"}));
    assert_eq!(master_page["sheet"], json!({"kind": "master", "master": 2}));
    let style = run(&mut s, "sheet.set_cell", json!({"style": 3, "cell": "EnableLineProps", "formula": "1"}));
    assert_eq!(style["sheet"], json!({"kind": "style", "style": 3}));
    // The flag is a style's: a shape has none.
    let e = refused(&mut s, "sheet.set_cell", json!({"id": 10, "cell": "EnableLineProps", "formula": "1"}));
    assert!(e.contains("the sheet has no cell EnableLineProps"), "{e}");
    // The Y of a Geometry row that is there, which the row does not hold yet.
    let y = run(&mut s, "sheet.set_cell", json!({"id": 10, "cell": "Geometry1.Y1", "formula": "2"}));
    assert_eq!((y["value"].clone(), y["cell"].clone()), (json!(2.0), json!("Geometry1.Y1")));
    // A cell the section itself holds, and the section is there.
    let flag = run(&mut s, "sheet.set_cell", json!({"id": 10, "cell": "Geometry1.NoFill", "formula": "1"}));
    assert_eq!(flag["value"], json!(1.0));
    assert_eq!(check_session(&s), Ok(()));
}

/// A row's name matches in any letter case, as the evaluator matches it: the cell written is the row the sheet holds.
#[test]
fn a_row_named_in_another_letter_case_is_written_where_the_sheet_holds_it() {
    let mut s = one_shape(ShapeBuilder::new(10).row_cell("Property", None, RowKey::name("Cost"), "Value", "9"));
    let answer = set(&mut s, "Prop.COST.Value", "12");
    assert_eq!((answer["cell"].clone(), answer["value"].clone()), (json!("Prop.Cost.Value"), json!(12.0)));
    assert_eq!(cell(&mut s, "Prop.Cost.Value")["value"], json!(12.0));
    let d = doc(&s);
    let shape = d.shape(&d.find_shape(Owner::Page(1), 10).unwrap()).unwrap();
    let section = shape.sheet.sections.iter().find(|sec| sec.name.as_str() == "Property").unwrap();
    assert_eq!(section.rows.len(), 1, "no second row was made for the other spelling");
}

// ---- the name of a cell and of a sheet ---------------------------------------------------------------------------------------------------

#[test]
fn a_reference_with_a_sheet_a_text_that_is_no_reference_and_a_missing_key_are_refused_in_words() {
    let mut s = one_shape(ShapeBuilder::new(10).cell_u("Width", "2", "IN"));
    let anchor = state(&s);
    for (params, words) in [
        (json!({"id": 10, "cell": "Sheet.4!Width", "formula": "1"}), "name the sheet with id, page, master, style or document, not in the reference"),
        (json!({"id": 10, "cell": "1+2", "formula": "1"}), "`cell`"),
        (json!({"id": 10, "cell": "=Width", "formula": "1"}), "`cell`"),
        (json!({"id": 10, "formula": "1"}), "missing `cell`"),
        (json!({"id": 10, "cell": "Width"}), "missing `formula`"),
        (json!({"id": 10, "cell": "Width", "formula": 3}), "`formula` must be text"),
        (json!({"id": 10, "cell": 5, "formula": "3"}), "`cell` must be text"),
        (json!({"id": 10, "cell": "Width", "formula": "3", "force": "yes"}), "`force` must be true or false"),
        (json!({"cell": "Width", "formula": "3"}), "give one sheet: id (with page), page, master (with id), style or document"),
        (json!({"id": 10, "style": 1, "cell": "Width", "formula": "3"}), "give one sheet: id (with page), page, master (with id), style or document"),
        (json!({"id": 99, "cell": "Width", "formula": "3"}), "no page has a shape 99"),
        (json!({"style": 9, "cell": "LineWeight", "formula": "3"}), "the drawing has no style 9"),
    ] {
        let e = refused(&mut s, "sheet.set_cell", params.clone());
        assert!(e.contains(words), "{params}: {e}");
    }
    let e = s.execute("sheet.set_cell", &json!({"id": 10, "cell": "Width", "formula": "3", "frm": 1})).unwrap_err().to_string();
    assert!(e.contains("unknown parameter `frm`"), "an unknown key is refused by the registry before the command runs: {e}");
    assert_eq!(state(&s), anchor);
}

// ---- text cells: R34 ---------------------------------------------------------------------------------------------------------------------

/// R34 (changed): in a cell that expects text, what is typed without a leading `=` is a string; with one it is a formula. V11 of the window's spec.
#[test]
fn text_typed_into_a_text_cell_is_a_string_and_with_an_equals_sign_a_formula() {
    let shape = ShapeBuilder::new(10)
        .row_cell("Property", None, RowKey::name("Cost"), "Label", "Cost")
        .row_cell("Property", None, RowKey::name("Cost"), "Prompt", "how much")
        .row_cell("Property", None, RowKey::name("Cost"), "Value", "9");
    let mut s = one_shape(shape);
    let label = set(&mut s, "Prop.Cost.Label", "Unit cost");
    assert_eq!((label["formula"].clone(), label["value"].clone(), label["error"].clone()), (json!("\"Unit cost\""), json!("Unit cost"), ME), "V11");
    assert_eq!(cell(&mut s, "Prop.Cost.Label")["value"], "Unit cost");
    let formula = set(&mut s, "Prop.Cost.Label", "=Prop.Cost.Prompt");
    assert_eq!((formula["formula"].clone(), formula["value"].clone()), (json!("Prop.Cost.Prompt"), json!("how much")), "V11");
    // A quotation mark inside is doubled, as a string literal spells it; a number typed is a string; a text that is a string literal already is the formula as it stands.
    let quoted = set(&mut s, "Prop.Cost.Label", "say \"hi\"");
    assert_eq!((quoted["formula"].clone(), quoted["value"].clone()), (json!("\"say \"\"hi\"\"\""), json!("say \"hi\"")));
    let number = set(&mut s, "Prop.Cost.Label", "3");
    assert_eq!((number["formula"].clone(), number["value"].clone()), (json!("\"3\""), json!("3")));
    let literal = set(&mut s, "Prop.Cost.Label", "\"Price\"");
    assert_eq!((literal["formula"].clone(), literal["value"].clone()), (json!("\"Price\""), json!("Price")));
    // An empty text clears, it is not an empty string.
    let cleared = set(&mut s, "Prop.Cost.Label", "");
    assert_eq!(cleared["formula"], "No Formula");
    // A cell that is no text cell reads the same text as a formula, and refuses it.
    let e = refused(&mut s, "sheet.set_cell", json!({"id": 10, "cell": "Prop.Cost.Value", "formula": "Unit cost"}));
    assert!(e.contains("the formula has an error"), "{e}");
    assert_eq!(check_session(&s), Ok(()));
}

// ---- the sheets: master, page, document and style ----------------------------------------------------------------------------------------

/// A master's cell is seen on every instance, on both pages.
#[test]
fn a_masters_cell_changes_its_instances_on_two_pages() {
    let master = MasterBuilder::new().shape(ShapeBuilder::new(5).cell_u("Width", "2", "IN").formula("LocPinX", "Width*0.5", "1"));
    let doc = DocBuilder::new()
        .master(1, master)
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).master(1)))
        .page(2, PageBuilder::new().shape(ShapeBuilder::new(10).master(1)))
        .build();
    let mut s = session_of(doc);
    let answer = run(&mut s, "sheet.set_cell", json!({"master": 1, "id": 5, "cell": "Width", "formula": "4"}));
    assert_eq!(answer["sheet"], json!({"kind": "shape", "master": 1, "id": 5}));
    assert_eq!(answer["value"], json!(4.0));
    for page in [1, 2] {
        let w = run(&mut s, "sheet.cell", json!({"id": 10, "page": page, "cell": "Width"}));
        assert_eq!((w["value"].clone(), w["origin"].clone()), (json!(4.0), json!("inherited")), "page {page}");
        let loc = run(&mut s, "sheet.cell", json!({"id": 10, "page": page, "cell": "LocPinX"}));
        assert_eq!(loc["value"], json!(2.0), "page {page}: what reads Width is worked out on the instance");
    }
    assert_eq!(undo_len(&s), 2, "one step (the build, and this)");
    assert_eq!(check_session(&s), Ok(()));
}

/// `TheDoc!User.Rate` read by a shape follows a write of the document's cell.
#[test]
fn a_cell_read_through_thedoc_follows_a_write_of_the_documents_cell() {
    let mut doc = DocBuilder::new().page(1, PageBuilder::new().shape(ShapeBuilder::new(10).formula("Width", "TheDoc!User.Rate*2", "6"))).build();
    Arc::make_mut(&mut doc.sheet)
        .set_cell(&CellAddr::in_row("User", None, RowKey::name("Rate"), "Value"), Cell::constant(Held::from_text("3")))
        .unwrap();
    let mut s = session_of(doc);
    let answer = run(&mut s, "sheet.set_cell", json!({"document": true, "cell": "User.Rate.Value", "formula": "5"}));
    assert_eq!(answer["sheet"], json!({"kind": "document"}));
    assert_eq!(answer["value"], json!(5.0));
    assert_eq!(cell(&mut s, "Width")["value"], json!(10.0));
    let listed = answer["recalculated"]["listed"].as_array().unwrap();
    assert!(listed.iter().any(|c| c["sheet"] == json!({"kind": "shape", "page": 1, "id": 10}) && c["cell"] == "Width"), "{answer}");
    assert_eq!(check_session(&s), Ok(()));
}

/// A page's cell and a style's cell: what reads them is worked out.
#[test]
fn a_pages_cell_and_a_styles_cell_are_written_and_what_reads_them_is_worked_out() {
    let doc = DocBuilder::new()
        .style(3, StyleBuilder::new().name("Heavy").cell("LineWeight", "0.02"))
        .page(
            1,
            PageBuilder::new().cell("PageWidth", "11").shape(
                ShapeBuilder::new(10).formula("Width", "ThePage!PageWidth*0.5", "5.5").formula("Height", "LineWeight*100", "2").links(
                    Some(3),
                    None,
                    None,
                ),
            ),
        )
        .build();
    let mut s = session_of(doc);
    run(&mut s, "sheet.set_cell", json!({"page": 1, "cell": "PageWidth", "formula": "20"}));
    assert_eq!(cell(&mut s, "Width")["value"], json!(10.0));
    let answer = run(&mut s, "sheet.set_cell", json!({"style": 3, "cell": "LineWeight", "formula": "0.05"}));
    assert_eq!(answer["sheet"], json!({"kind": "style", "style": 3}));
    assert!(near(cell(&mut s, "Height")["value"].as_f64().unwrap(), 5.0), "{}", cell(&mut s, "Height"));
    assert_eq!(check_session(&s), Ok(()));
}

// ---- the drawing follows: connectors -----------------------------------------------------------------------------------------------------

/// A glued connector is routed again after a write of `PinX`, as it is after the shape is moved.
#[test]
fn a_glued_connector_is_routed_again_after_pinx_is_written() {
    let make = || {
        let mut s = blank_session();
        assert_eq!(drop_shape(&mut s, "rectangle", 2.0, 2.0), 1);
        assert_eq!(drop_shape(&mut s, "rectangle", 6.0, 4.0), 2);
        assert_eq!(id_of(&run(&mut s, "connector.add", json!({"from": {"id": 1}, "to": {"id": 2}}))), 3);
        s
    };
    let (mut written, mut moved) = (make(), make());
    let start = route(doc(&written), 3);
    run(&mut moved, "shape.move", json!({"ids": [1], "dx": 3.0, "dy": 0.0}));
    let answer = run(&mut written, "sheet.set_cell", json!({"id": 1, "cell": "PinX", "formula": "5"}));
    assert_eq!(answer["changed"], true);
    let (a, b) = (route(doc(&written), 3), route(doc(&moved), 3));
    assert_ne!(a, start, "the connector was routed again");
    assert_eq!(a.len(), b.len(), "{a:?} against {b:?}");
    assert!(a.iter().zip(&b).all(|(p, q)| near(p.0, q.0) && near(p.1, q.1)), "the route is the one a move makes: {a:?} against {b:?}");
    assert_eq!(check_session(&written), Ok(()));
}

// ---- a locked layer: --------------------------------------------------------------------------------------------------------

/// A page with a rectangle, 1, on the locked layer `Frozen`, and 2 on none.
fn locked() -> Session {
    let mut s = blank_session();
    assert_eq!(drop_shape(&mut s, "rectangle", 2.0, 3.0), 1);
    assert_eq!(drop_shape(&mut s, "rectangle", 5.0, 3.0), 2);
    run(&mut s, "layer.add", json!({"name": "Frozen"}));
    run(&mut s, "layer.assign", json!({"ids": [1], "layers": ["Frozen"]}));
    run(&mut s, "layer.set", json!({"layer": "Frozen", "lock": true}));
    s
}

/// A shape on a locked layer is refused by the registry's rule, in its words, for one write and for a write inside a batch; the other shape and a master's shape with the same `ID`
/// are not.
#[test]
fn a_shape_on_a_locked_layer_is_refused_and_a_masters_shape_with_its_id_is_not() {
    let mut s = locked();
    let anchor = state(&s);
    let e = refused(&mut s, "sheet.set_cell", json!({"id": 1, "cell": "Width", "formula": "3"}));
    assert!(e.contains("shape 1 is on layer") && e.contains("\"Frozen\"") && e.contains("locked"), "{e}");
    let e = refused(
        &mut s,
        "sheet.set_cells",
        json!({"writes": [{"id": 2, "cell": "Width", "formula": "3"}, {"id": 1, "cell": "Width", "formula": "3"}]}),
    );
    assert!(e.contains("shape 1 is on layer") && e.contains("locked"), "a write in a batch is judged too: {e}");
    assert_eq!(state(&s), anchor, "the write on the open shape was not kept either");
    let e = refused(&mut s, "sheet.set_cells", json!({"id": 1, "writes": [{"cell": "Width", "formula": "3"}]}));
    assert!(e.contains("shape 1 is on layer"), "{e}");
    run(&mut s, "sheet.set_cell", json!({"id": 2, "cell": "Width", "formula": "3"}));
    // A page's own sheet is no shape's.
    run(&mut s, "sheet.set_cell", json!({"page": 1, "cell": "PageWidth", "formula": "12"}));

    // A master that holds a shape with the locked shape's `ID`: no page's layer locks a master's shape.
    let master = MasterBuilder::new().shape(ShapeBuilder::new(1).cell_u("Width", "2", "IN"));
    let mut d = doc(&s).clone();
    let one = DocBuilder::new().master(7, master).build();
    d.masters = one.masters;
    let mut s = session_of(d);
    run(&mut s, "sheet.set_cell", json!({"master": 7, "id": 1, "cell": "Width", "formula": "5"}));
    run(&mut s, "sheet.set_cells", json!({"master": 7, "id": 1, "writes": [{"cell": "Width", "formula": "6"}]}));
    run(&mut s, "sheet.set_cells", json!({"writes": [{"master": 7, "id": 1, "cell": "Width", "formula": "7"}]}));
    let e = refused(&mut s, "sheet.set_cell", json!({"id": 1, "cell": "Width", "formula": "3"}));
    assert!(e.contains("shape 1 is on layer"), "a page's shape 1 still is: {e}");
}

// ---- a batch: sheet.set_cells ---------------------------------------------------------------------------------------------------------------

/// A shape with `n` Scratch rows, each with a cell `A` holding 0.
fn scratchy(n: u32) -> Session {
    let mut shape = ShapeBuilder::new(10).cell_u("Width", "2", "IN");
    for k in 0..n {
        shape = shape.row_cell("Scratch", None, RowKey::Ix(k), "A", "0");
    }
    one_shape(shape)
}

#[test]
fn a_batch_is_one_step_with_one_answer_and_each_write_may_name_its_own_sheet() {
    let doc = DocBuilder::new()
        .style(3, StyleBuilder::new().name("Heavy").cell("LineWeight", "0.02"))
        .page(1, PageBuilder::new().cell("PageWidth", "11").shape(ShapeBuilder::new(10).cell_u("Width", "2", "IN").formula("Height", "Width*2", "4")))
        .build();
    let mut s = session_of(doc);
    let before = undo_len(&s);
    let answer = run(
        &mut s,
        "sheet.set_cells",
        json!({"id": 10, "writes": [
            {"cell": "Width", "formula": "3"},
            {"cell": "Width", "formula": "3"},
            {"page": 1, "cell": "PageWidth", "formula": "12"},
            {"style": 3, "cell": "LineWeight", "formula": "0.04"},
            {"cell": "Height", "formula": ""},
        ]}),
    );
    assert_eq!(undo_len(&s), before + 1, "one step for the whole call");
    assert_eq!(s.history().unwrap().next_undo_label(), Some("Edit formulas"));
    assert_eq!((answer["written"].clone(), answer["unchanged"].clone()), (json!(4), json!(1)), "{answer}");
    let results = answer["results"].as_array().unwrap();
    assert_eq!(results.len(), 5);
    assert_eq!(results[0]["sheet"], json!({"kind": "shape", "page": 1, "id": 10}));
    assert_eq!((results[0]["cell"].clone(), results[0]["changed"].clone(), results[0]["value"].clone()), (json!("Width"), json!(true), json!(3.0)));
    assert_eq!(results[1]["changed"], false, "the second write of the same text changes nothing");
    assert_eq!((results[2]["sheet"].clone(), results[2]["value"].clone()), (json!({"kind": "page", "page": 1}), json!(12.0)));
    assert_eq!(results[3]["sheet"], json!({"kind": "style", "style": 3}));
    assert_eq!(results[4]["formula"], "No Formula");
    assert!(answer["recalculated"]["cells"].is_number() && answer["recalculated"]["listed"].is_array(), "{answer}");
    // Undo takes all of it back at once.
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(cell(&mut s, "Width")["value"], json!(2.0));
    assert_eq!(run(&mut s, "sheet.cell", json!({"page": 1, "cell": "PageWidth"}))["value"], json!(11.0));
}

/// A batch of 4,096 writes is one step; one past it is refused; and a write that is refused refuses the call whole, naming the write.
#[test]
fn a_batch_of_4096_writes_is_one_step_and_the_4097th_is_refused() {
    let mut s = scratchy(4_096);
    let before = undo_len(&s);
    let writes: Vec<Value> = (1..=4_096).map(|n| json!({"cell": format!("Scratch.A{n}"), "formula": n.to_string()})).collect();
    let answer = run(&mut s, "sheet.set_cells", json!({"id": 10, "writes": writes}));
    assert_eq!((answer["written"].clone(), answer["unchanged"].clone()), (json!(4_096), json!(0)));
    assert_eq!(answer["results"].as_array().unwrap().len(), 256, "the first 256 results");
    assert_eq!(undo_len(&s), before + 1);
    assert_eq!(cell(&mut s, "Scratch.A4096")["value"], json!(4_096.0));
    assert_eq!(cell(&mut s, "Scratch.A1")["value"], json!(1.0));
    // One too many.
    let anchor = state(&s);
    let writes: Vec<Value> = (0..4_097).map(|_| json!({"cell": "Scratch.A1", "formula": "7"})).collect();
    let e = refused(&mut s, "sheet.set_cells", json!({"id": 10, "writes": writes}));
    assert!(e.contains("`writes` holds at most 4096 writes"), "{e}");
    assert_eq!(state(&s), anchor);
    let e = refused(&mut s, "sheet.set_cells", json!({"id": 10, "writes": []}));
    assert!(e.contains("`writes` must hold at least one write"), "{e}");
}

/// All or nothing: a write that is refused (a cell that is not there, a formula that does not parse) refuses the call, and the writes before it are not kept.
#[test]
fn a_batch_is_all_or_nothing_and_names_the_write_that_was_refused() {
    let mut s = scratchy(3);
    let anchor = state(&s);
    for (writes, words) in [
        (json!([{"cell": "Scratch.A1", "formula": "5"}, {"cell": "Scratch.A9", "formula": "5"}]), "write 2: the sheet has no cell Scratch.A9"),
        (
            json!([{"cell": "Scratch.A1", "formula": "5"}, {"cell": "Scratch.A2", "formula": "5"}, {"cell": "Scratch.A3", "formula": "1+"}]),
            "write 3: the formula has an error",
        ),
        (json!([{"cell": "Scratch.A1", "formula": "5", "oops": 1}]), "write 1 has an unknown key `oops`"),
        (json!([{"cell": "Scratch.A1"}]), "write 1: missing `formula`"),
        (json!([{"formula": "5"}]), "write 1: missing `cell`"),
        (json!([5]), "write 1 must be an object"),
        (json!([{"cell": "Scratch.A1", "formula": "5", "id": 99}]), "write 1: no page has a shape 99"),
        (json!([{"cell": "Scratch.A1", "formula": "5", "id": 10, "style": 0}]), "write 1: give one sheet"),
        (json!({"cell": "Scratch.A1"}), "`writes` must be a list of writes"),
    ] {
        let e = refused(&mut s, "sheet.set_cells", json!({"id": 10, "writes": writes.clone()}));
        assert!(e.contains(words), "{writes}: {e}");
        assert_eq!(state(&s), anchor, "{writes}");
    }
    let e = refused(&mut s, "sheet.set_cells", json!({"id": 10}));
    assert!(e.contains("missing `writes`"), "{e}");
}

// ---- the limits: ------------------------------------------------------------------------------------------------------------

/// A string literal of exactly `bytes` bytes, quotes included.
fn string_of(bytes: usize) -> String {
    format!("\"{}\"", "x".repeat(bytes - 2))
}

/// 65,536 bytes is the most a formula may be; 65,537 is refused, and so is a text of that length that is only blanks around a short one (the limit is on the text given).
#[test]
fn a_formula_of_65536_bytes_is_written_and_one_of_65537_is_refused() {
    let shape = ShapeBuilder::new(10).row_cell("Property", None, RowKey::name("Cost"), "Label", "Cost");
    let mut s = one_shape(shape);
    let anchor = state(&s);
    let e = refused(&mut s, "sheet.set_cell", json!({"id": 10, "cell": "Prop.Cost.Label", "formula": string_of(65_537)}));
    assert!(e.contains("a formula is at most 65536 bytes long, and this one has 65537"), "{e}");
    let e = refused(&mut s, "sheet.set_cell", json!({"id": 10, "cell": "Prop.Cost.Label", "formula": format!("{}1", " ".repeat(65_536))}));
    assert!(e.contains("a formula is at most 65536 bytes long"), "{e}");
    assert_eq!(state(&s), anchor);
    let written = set(&mut s, "Prop.Cost.Label", &string_of(65_536));
    assert_eq!(written["changed"], true);
    assert_eq!(cell(&mut s, "Prop.Cost.Label")["formula"].as_str().unwrap().len(), 65_536);
}

/// A call holds at most 4 MiB of formula text in all, and at most 262,144 parts of formulas in all: counts, held whole.
#[test]
fn a_call_holds_at_most_four_mebibytes_of_formula_text_and_a_budget_of_parts() {
    let mut shape = ShapeBuilder::new(10);
    for k in 0..70_u32 {
        shape = shape.row_cell("Property", None, RowKey::name(&format!("P{k}")), "Label", "x");
    }
    let mut s = one_shape(shape);
    let anchor = state(&s);
    let writes = |n: u32, text: &str| -> Vec<Value> { (0..n).map(|k| json!({"cell": format!("Prop.P{k}.Label"), "formula": text})).collect() };
    // 65 formulas of 65,536 bytes are 4,259,840 bytes: past 4 MiB.
    let e = refused(&mut s, "sheet.set_cells", json!({"id": 10, "writes": writes(65, &string_of(65_536))}));
    assert!(e.contains("the formulas of one call are at most 4194304 bytes in all"), "{e}");
    assert_eq!(state(&s), anchor);
    // 64 are exactly 4 MiB: written.
    let answer = run(&mut s, "sheet.set_cells", json!({"id": 10, "writes": writes(64, &string_of(65_536))}));
    assert_eq!(answer["written"], 64);
    // Many parts: a call of nine formulas of some 32,000 arguments each is past the parts a call may hold.
    let wide = format!("=MAX({}1)", "1,".repeat(32_000));
    let anchor = state(&s);
    let e = refused(&mut s, "sheet.set_cells", json!({"id": 10, "writes": writes(9, &wide)}));
    assert!(e.contains("the formulas of one call have too many parts: at most 262144 in all"), "{e}");
    assert_eq!(state(&s), anchor);
}

// ---- the journal, a batch of commands, hostile params ---------------------------------------------------------------------------------

#[test]
fn a_write_is_journaled_inside_a_command_batch_it_is_part_of_that_step_and_a_read_is_not_journaled() {
    let mut s = one_shape(ShapeBuilder::new(10).cell_u("Width", "2", "IN").formula("LocPinX", "Width*0.5", "1"));
    let journal = s.journal().len();
    set(&mut s, "Width", "4");
    assert_eq!(s.journal().len(), journal + 1);
    assert_eq!(s.journal().last().map(|(id, _)| id.as_str()), Some("sheet.set_cell"));
    cell(&mut s, "Width");
    assert_eq!(s.journal().len(), journal + 1);
    let before = undo_len(&s);
    run(
        &mut s,
        "command.batch",
        json!({"commands": [
            {"command": "sheet.set_cell", "params": {"id": 10, "cell": "Width", "formula": "6"}},
            {"command": "sheet.set_cell", "params": {"id": 10, "cell": "Height", "formula": "Width*2"}},
        ]}),
    );
    assert_eq!(undo_len(&s), before + 1, "a batch of writes is one step");
    assert_eq!(cell(&mut s, "Height")["value"], json!(12.0));
}

#[test]
fn hostile_params_are_refused_without_a_panic_and_change_nothing() {
    let mut s = one_shape(ShapeBuilder::new(10).cell_u("Width", "2", "IN"));
    let junk = [json!(null), json!(true), json!(-1), json!(1.5e300), json!([]), json!({}), json!("\u{0}"), json!("é".repeat(40))];
    for v in &junk {
        for key in ["id", "page", "master", "style", "document", "cell", "formula", "force"] {
            let mut p = json!({"id": 10, "cell": "Width", "formula": "3"});
            p[key] = v.clone();
            let _ = s.execute("sheet.set_cell", &p);
        }
        let _ = s.execute("sheet.set_cells", &json!({"id": 10, "writes": v}));
        let _ = s.execute("sheet.set_cells", &json!({"id": 10, "writes": [v]}));
        let _ = s.execute("sheet.set_cells", &json!({"id": 10, "writes": [{"cell": v, "formula": "1"}]}));
        let _ = s.execute("sheet.set_cells", &json!({"id": 10, "writes": [{"cell": "Width", "formula": v}]}));
    }
    // Some of them were legal writes (a `force` of true is no fault): whatever they did, the drawing is sound.
    assert_eq!(check_session(&s), Ok(()));
    let e: EngineError = s.execute("sheet.set_cell", &json!([1, 2])).unwrap_err();
    assert!(e.to_string().contains("parameters must be a JSON object"), "{e}");
}

// ---- the drawing follows a master, a style and the document: the connectors glued to what moved ---------------------------------------------

/// Where connector 3's rows put its ends on the page, against where the connection points it is glued to are: a connector routed again after the shapes moved ends on both.
fn ends_on_their_points(s: &Session) -> bool {
    let d = doc(s);
    let points = |id: u32| diacraft_engine::draw::connection_points(d, 1, id);
    let anchor = |from_cell: &str| {
        let c = d.page(1).unwrap().connects.iter().find(|c| c.from_sheet == 3 && c.from_cell.as_deref() == Some(from_cell)).unwrap();
        let p = points(c.to_sheet).into_iter().find(|p| p.index == c.to_part.unwrap() - 100).unwrap();
        (p.x, p.y)
    };
    let rows = route(d, 3);
    let (begin, end) = (anchor("BeginX"), anchor("EndX"));
    let (first, last) = (rows[0], rows[rows.len() - 1]);
    near(first.0, begin.0) && near(first.1, begin.1) && near(last.0, end.0) && near(last.1, end.1)
}

/// A write of a master's cell moves every instance; the connectors glued to them are routed again, where a worked-out page alone would leave their rows as they were.
#[test]
fn a_master_cell_that_widens_the_instances_routes_the_connectors_glued_to_them_again() {
    let mut s = blank_session();
    assert_eq!(drop_shape(&mut s, "rectangle", 2.0, 2.0), 1);
    assert_eq!(drop_shape(&mut s, "rectangle", 6.0, 4.0), 2);
    assert_eq!(id_of(&run(&mut s, "connector.add", json!({"from": {"id": 1}, "to": {"id": 2}}))), 3);
    assert!(ends_on_their_points(&s));
    let (master, shape) = {
        let d = doc(&s);
        let m = d.masters.iter().next().unwrap();
        (m.id, m.shapes.iter().next().unwrap().id)
    };
    let before = route(doc(&s), 3);
    run(&mut s, "sheet.set_cell", json!({"master": master, "id": shape, "cell": "Width", "formula": "3"}));
    assert_eq!(cell_of(&mut s, 1, "Width"), json!(3.0), "the instance reads the master's new width");
    assert_ne!(route(doc(&s), 3), before, "the connector was routed again");
    assert!(ends_on_their_points(&s), "its rows end on the connection points of the wider shapes");
    assert_eq!(check_session(&s), Ok(()));
}

fn cell_of(s: &mut Session, id: u32, name: &str) -> Value {
    run(s, "sheet.cell", json!({"id": id, "cell": name}))["value"].clone()
}

/// A member of a group is found by its `ID` as any shape is, and written.
#[test]
fn a_member_of_a_group_is_written_by_its_id() {
    let group = ShapeBuilder::new(10).group().child(ShapeBuilder::new(11).cell_u("Width", "2", "IN"));
    let mut s = one_shape(group);
    let answer = run(&mut s, "sheet.set_cell", json!({"id": 11, "cell": "Width", "formula": "3"}));
    assert_eq!((answer["sheet"].clone(), answer["value"].clone()), (json!({"kind": "shape", "page": 1, "id": 11}), json!(3.0)));
    assert_eq!(check_session(&s), Ok(()));
}

/// A clear of a cell the sheet does not hold of its own is no write, whatever it inherits: a guard it inherits does not refuse it.
#[test]
fn a_clear_of_a_cell_the_sheet_does_not_hold_is_not_refused_by_an_inherited_guard() {
    let master = MasterBuilder::new().shape(ShapeBuilder::new(5).formula("Width", "GUARD(2 in)", "2"));
    let mut s = session_of(DocBuilder::new().master(1, master).page(1, PageBuilder::new().shape(ShapeBuilder::new(10).master(1))).build());
    let answer = set(&mut s, "Width", "");
    assert_eq!(answer["changed"], false);
    // A formula over it is refused without force (an inherited GUARD protects the inheritor), and written with it.
    let e = refused(&mut s, "sheet.set_cell", json!({"id": 10, "cell": "Width", "formula": "3"}));
    assert!(e.contains("guarded by GUARD()"), "{e}");
    let forced = run(&mut s, "sheet.set_cell", json!({"id": 10, "cell": "Width", "formula": "3", "force": true}));
    assert_eq!((forced["value"].clone(), forced["origin"].clone()), (json!(3.0), json!("local")));
}

// ---- a deleted row, and the work of a call as counts ------------------------------------------------------------------------------------------

/// A cell of a row or a section that is deleted here (`Del`) is not there, and is refused in words.
#[test]
fn a_cell_in_a_deleted_row_or_section_is_refused() {
    let shape = ShapeBuilder::new(10)
        .row_cell("User", None, RowKey::name("Rate"), "Value", "5")
        .del_row("User", None, RowKey::name("Rate"))
        .row_cell("Scratch", None, RowKey::Ix(0), "A", "1")
        .del_section("Scratch", None);
    let mut s = one_shape(shape);
    let anchor = state(&s);
    for cell in ["User.Rate.Value", "Scratch.A1"] {
        let e = refused(&mut s, "sheet.set_cell", json!({"id": 10, "cell": cell, "formula": "2"}));
        assert!(e.contains(&format!("the sheet has no cell {cell}: the row or the section it is in is deleted here")), "{e}");
    }
    assert_eq!(state(&s), anchor);
}

/// A call works out **each page it touched once, at the end**, however many writes it holds, and only the pages the writes can reach: a count of commits, not a time.
#[test]
fn a_batch_works_out_each_page_it_touched_once_and_a_masters_cell_only_the_pages_that_hold_an_instance() {
    let master = MasterBuilder::new().shape(ShapeBuilder::new(5).cell_u("Width", "2", "IN"));
    let page = |with_instance: bool| {
        let mut p = PageBuilder::new().shape(ShapeBuilder::new(10).cell_u("Width", "2", "IN").formula("LocPinX", "Width*0.5", "1"));
        if with_instance {
            p = p.shape(ShapeBuilder::new(11).master(1));
        }
        p
    };
    let doc = DocBuilder::new().master(1, master).page(1, page(true)).page(2, page(true)).page(3, page(false)).build();
    let mut s = session_of(doc);
    // Fifty writes to each of two pages: two commits.
    let writes: Vec<Value> = (0..100).map(|k| json!({"page": 1 + (k % 2), "id": 10, "cell": "Width", "formula": format!("{}", 3 + k)})).collect();
    diacraft_calc::work::reset();
    run(&mut s, "sheet.set_cells", json!({"writes": writes}));
    assert_eq!(diacraft_calc::work::commits(), 2, "one commit for each page touched");
    // A master's cell: the two pages that hold an instance, and not the third.
    diacraft_calc::work::reset();
    run(&mut s, "sheet.set_cell", json!({"master": 1, "id": 5, "cell": "Width", "formula": "4"}));
    assert_eq!(diacraft_calc::work::commits(), 2, "the pages with an instance of the master");
    // The document's cell: every page.
    diacraft_calc::work::reset();
    let mut d = doc_of(&s).clone();
    Arc::make_mut(&mut d.sheet)
        .set_cell(&CellAddr::in_row("User", None, RowKey::name("Rate"), "Value"), Cell::constant(Held::from_text("3")))
        .unwrap();
    let mut s = session_of(d);
    diacraft_calc::work::reset();
    run(&mut s, "sheet.set_cell", json!({"document": true, "cell": "User.Rate.Value", "formula": "5"}));
    assert_eq!(diacraft_calc::work::commits(), 3, "every page, as a whole page");
    assert_eq!(check_session(&s), Ok(()));
}

fn doc_of(s: &Session) -> &Document {
    doc(s)
}

// ---- the sheets no page holds ------------------------------------------------------------------------------------------------

/// A sheet's keys with `extra` beside them.
fn on(keys: &Value, extra: Value) -> Value {
    let mut params = keys.clone();
    if let (Some(map), Some(more)) = (params.as_object_mut(), extra.as_object()) {
        map.extend(more.clone());
    }
    params
}

/// A formula typed into a cell of a sheet that no page works out, in a chain of three cells: the cell is worked out as one typed on a shape is, the cells of its own sheet that read it
/// follow it, a full recalculation finds nothing left over, and undo takes the write and what it worked out back together.
fn a_chain_on(s: &mut Session, label: &str, keys: &Value) {
    run(s, "sheet.add_section", on(keys, json!({"section": "User"})));
    run(s, "sheet.add_row", on(keys, json!({"section": "User", "name": "B"})));
    run(s, "sheet.add_row", on(keys, json!({"section": "User", "name": "C"})));
    run(s, "sheet.set_cell", on(keys, json!({"cell": "User.Row_1.Value", "formula": "5"})));
    let b = run(s, "sheet.set_cell", on(keys, json!({"cell": "User.B.Value", "formula": "User.Row_1.Value+1"})));
    assert_eq!(b["value"], json!(6.0), "{label}: the cell written reads what its formula gives: {b}");
    let c = run(s, "sheet.set_cell", on(keys, json!({"cell": "User.C.Value", "formula": "User.B.Value*2"})));
    assert_eq!(c["value"], json!(12.0), "{label}: {c}");
    // The cell they read is written again: both follow it.
    run(s, "sheet.set_cell", on(keys, json!({"cell": "User.Row_1.Value", "formula": "8"})));
    let read = |s: &mut Session, name: &str| run(s, "sheet.cell", on(keys, json!({"cell": name})))["value"].clone();
    assert_eq!(
        (read(s, "User.B.Value"), read(s, "User.C.Value")),
        (json!(9.0), json!(18.0)),
        "{label}: the cells that read the written one follow it"
    );
    let report = diacraft_calc::recalc_all(doc(s), &diacraft_calc::Budget::default());
    assert!(report.differed.is_empty(), "{label}: a full recalculation differs from the drawing: {:?}", report.differed);
    run(s, "edit.undo", json!({}));
    assert_eq!((read(s, "User.B.Value"), read(s, "User.C.Value")), (json!(6.0), json!(12.0)), "{label}: undo");
    assert_eq!(check_session(s), Ok(()));
}

/// The sheets no page works out: the document's, a style's, a master's page sheet, and a master's shape when no page holds an instance of it. The property test found the first of
/// them left at the 0 it was given, whatever it read, and a full recalculation saying so. (A master's shape that a page holds an instance of is worked out by that page.)
#[test]
fn a_formula_in_the_sheet_of_the_document_a_style_a_masters_page_or_a_masters_shape_is_worked_out_and_followed() {
    let master = MasterBuilder::new().shape(ShapeBuilder::new(5).cell_u("Width", "1", "IN"));
    let page = PageBuilder::new().name("Page-1").shape(ShapeBuilder::new(10).cell_u("Width", "2", "IN"));
    let mut s = session_of(DocBuilder::new().master(1, master).style(1, StyleBuilder::new().name("S1")).page(1, page).build());
    for (label, keys) in [
        ("the document", json!({"document": true})),
        ("a style", json!({"style": 1})),
        ("a master's page", json!({"master": 1})),
        ("the shape of a master no page holds an instance of", json!({"master": 1, "id": 5})),
    ] {
        a_chain_on(&mut s, label, &keys);
    }
}

/// A call that puts every cell back as it was is no change and no step, though the first write of it stood a value in the cell until the recalculation gave it the value it had (
/// the property test found a batch that wrote a cell and wrote it back made an undo step, and the drawing after it was equal to the drawing before).
#[test]
fn a_batch_that_writes_a_cell_and_writes_it_back_is_no_step() {
    let mut s = one_shape(ShapeBuilder::new(10).cell_u("Width", "2", "IN").formula("Height", "Width*2", "4"));
    let anchor = state(&s);
    let writes = json!([{"cell": "Height", "formula": "7"}, {"cell": "Height", "formula": "Width*2"}]);
    let answer = run(&mut s, "sheet.set_cells", json!({"id": 10, "writes": writes}));
    assert_eq!(state(&s), anchor, "the drawing, the history and the revision are as they were: {answer}");
}

/// The same chain on the shape of a master that a page holds an instance of, which the pages that hold one work out.
#[test]
fn a_formula_in_the_shape_of_a_master_a_page_holds_an_instance_of_is_worked_out_and_followed() {
    let master = MasterBuilder::new().shape(ShapeBuilder::new(5).cell_u("Width", "1", "IN"));
    let page = PageBuilder::new().name("Page-1").shape(ShapeBuilder::new(10).master(1));
    let mut s = session_of(DocBuilder::new().master(1, master).page(1, page).build());
    a_chain_on(&mut s, "the shape of a master with an instance", &json!({"master": 1, "id": 5}));
}
