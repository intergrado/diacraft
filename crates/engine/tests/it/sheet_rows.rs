//! Rows and sections of a ShapeSheet: `sheet.add_row`, `sheet.delete_row`, `sheet.add_section` and `sheet.delete_section`.
//!
//! The rules are the design of the ShapeSheet window's (section 7.6) as the window's spec amends them
//! A delete closes the numbers up, an insert in the middle numbers the rows after it again, the formulas that
//! name a row that moved follow it (a project rule) and one that names a row that went reads `#REF!`; a master's or a style's row that a sheet drops is written as a `Del`
//! row. A cost is held to a count, never to a time (a real-time check is `#[ignore]`d).

use std::sync::Arc;

use diacraft_doc::{Cell, CellAddr, Document, Owner, Resolver, RowKey, SectionKey, SheetRef, Value as Held};
use diacraft_engine::{EngineError, Session};
use diacraft_testkit::build::{DocBuilder, MasterBuilder, PageBuilder, ShapeBuilder, StyleBuilder};
use serde_json::{Value, json};

use crate::edit::{doc, drop_shape, geometry, id_of};
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

fn undo_len(s: &Session) -> usize {
    s.history().map_or(0, |h| h.undo_len())
}

/// The document and the history as they are, without the journal.
fn state(s: &Session) -> (Option<Document>, usize, usize, u64) {
    let snap = snapshot(s);
    (snap.document, snap.undo_steps, snap.redo_steps, snap.revision)
}

/// `sheet.add_row` on shape 10.
fn add_row(s: &mut Session, section: &str, extra: Value) -> Value {
    let mut p = json!({"id": 10, "section": section});
    if let (Some(map), Some(more)) = (p.as_object_mut(), extra.as_object()) {
        map.extend(more.clone());
    }
    run(s, "sheet.add_row", p)
}

/// `sheet.delete_row` on shape 10.
fn delete_row(s: &mut Session, section: &str, row: Value) -> Value {
    run(s, "sheet.delete_row", json!({"id": 10, "section": section, "row": row}))
}

/// The rows of a section of shape 10 as `sheet.read` lists them: each row's key and type.
fn rows(s: &mut Session, section: &str) -> Vec<(Value, Value)> {
    let read = run(s, "sheet.read", json!({"id": 10, "sections": [section], "count": 2000}));
    read["lines"].as_array().unwrap().iter().filter(|l| l["kind"] == "row").map(|l| (l["row"]["key"].clone(), l["row"]["type"].clone())).collect()
}

fn keys(s: &mut Session, section: &str) -> Vec<Value> {
    rows(s, section).into_iter().map(|(k, _)| k).collect()
}

fn cell(s: &mut Session, name: &str) -> Value {
    run(s, "sheet.cell", json!({"id": 10, "cell": name}))
}

/// The number a cell of shape 10 reads as.
fn num(s: &mut Session, name: &str) -> f64 {
    cell(s, name)["value"].as_f64().unwrap_or_else(|| panic!("{name} holds no number"))
}

/// The formula text of a cell of shape 10.
fn formula(s: &mut Session, name: &str) -> String {
    cell(s, name)["formula"].as_str().unwrap().to_owned()
}

/// A shape 10 with a Geometry section of rows `1..=n`: a `MoveTo` and `LineTo` rows, row `k` at `(k, 10k)`.
fn geometry_shape(n: u32) -> ShapeBuilder {
    let mut shape = ShapeBuilder::new(10).cell_u("Width", "6", "IN").cell_u("Height", "6", "IN");
    for k in 1..=n {
        shape = shape
            .row_type("Geometry", Some(0), RowKey::Ix(k), if k == 1 { "MoveTo" } else { "LineTo" })
            .set(CellAddr::in_row("Geometry", Some(0), RowKey::Ix(k), "X"), Cell::constant(Held::from_text(&k.to_string())).with_unit("IN"))
            .set(CellAddr::in_row("Geometry", Some(0), RowKey::Ix(k), "Y"), Cell::constant(Held::from_text(&(10 * k).to_string())).with_unit("IN"));
    }
    shape
}

/// A shape 10 with a Scratch section of rows `A1..=An`, row `k` holding `A = 10k`.
fn scratch_shape(n: u32) -> ShapeBuilder {
    let mut shape = ShapeBuilder::new(10).cell_u("Width", "2", "IN");
    for k in 0..n {
        shape = shape.row_cell("Scratch", None, RowKey::Ix(k), "A", &(10 * (k + 1)).to_string());
    }
    shape
}

const NULL: Value = Value::Null;

// ---- naming a row, and the answer -----------------------------------------------------------------------------------------------------

/// A section added holds one row, named `Row_1`; the next row added is `Row_2` (design 11): one undo step each, named for what it did; the answer names the sheet, the
/// section and the row and gives the cells the row holds, each at its default.
#[test]
fn a_user_row_is_named_row_1_then_row_2_and_each_is_one_undo_step() {
    let mut s = one_shape(ShapeBuilder::new(10).cell_u("Width", "2", "IN"));
    let before = undo_len(&s);
    let section = run(&mut s, "sheet.add_section", json!({"id": 10, "section": "User"}));
    assert_eq!(section["section"], "User", "{section}");
    assert_eq!(section["sheet"], json!({"kind": "shape", "page": 1, "id": 10}));
    assert_eq!(keys(&mut s, "User"), [json!("Row_1")], "a section added through the window holds one row");
    assert_eq!(s.history().unwrap().next_undo_label(), Some("Insert section"));
    let second = add_row(&mut s, "User", json!({}));
    assert_eq!(
        (second["section"].clone(), second["row"].clone(), second["label"].clone()),
        (json!("User"), json!("Row_2"), json!("User.Row_2")),
        "{second}"
    );
    assert_eq!(second["sheet"], json!({"kind": "shape", "page": 1, "id": 10}));
    let names: Vec<&str> = second["cells"].as_array().unwrap().iter().map(|c| c["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Value", "Prompt"], "every cell the catalogue lists for the section: {second}");
    assert_eq!(second["cells"][0]["ref"], "User.Row_2.Value");
    assert_eq!(undo_len(&s), before + 2, "one step for the section and one for the row");
    assert_eq!(s.history().unwrap().next_undo_label(), Some("Insert row"));
    assert_eq!(keys(&mut s, "User"), [json!("Row_1"), json!("Row_2")]);
    let third = add_row(&mut s, "User", json!({}));
    assert_eq!(third["row"], "Row_3");
    // Undo takes the row away and redo puts it back.
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(keys(&mut s, "User"), [json!("Row_1"), json!("Row_2")]);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(keys(&mut s, "User").len(), 3);
    assert_eq!(check_session(&s), Ok(()));
}

/// V7 of the window's spec: a Shape Data section with rows `Cost` and `Row_1`, an unnamed row inserted: `Row_2`, not `Property1`. The numbering takes account of the rows
/// that are there, in any letter case.
#[test]
fn an_unnamed_shape_data_row_is_row_2_after_cost_and_row_1_not_property_1() {
    let shape = ShapeBuilder::new(10)
        .row_cell("Property", None, RowKey::name("Cost"), "Value", "9")
        .row_cell("Property", None, RowKey::name("Row_1"), "Value", "1")
        .row_cell("Property", None, RowKey::name("ROW_3"), "Value", "3");
    let mut s = one_shape(shape);
    let answer = add_row(&mut s, "Prop", json!({}));
    assert_eq!((answer["row"].clone(), answer["label"].clone()), (json!("Row_2"), json!("Prop.Row_2")), "{answer}");
    let again = add_row(&mut s, "Shape Data", json!({}));
    assert_eq!(again["row"], "Row_4", "a row named in another letter case is taken: {again}");
    // The new row holds every cell the catalogue lists for Shape Data, at its default (a value, no formula).
    let names: Vec<&str> = answer["cells"].as_array().unwrap().iter().map(|c| c["name"].as_str().unwrap()).collect();
    for want in ["Label", "Prompt", "Type", "Format", "Value", "SortKey", "Invisible"] {
        assert!(names.contains(&want), "{want} in {names:?}");
    }
    assert_eq!(cell(&mut s, "Prop.Row_2.Type")["origin"], "local");
    assert_eq!(check_session(&s), Ok(()));
}

/// A row given a name takes it, and goes where `after` or `before` says; with neither it goes at the end.
#[test]
fn a_named_row_takes_its_name_and_goes_after_or_before_the_row_named() {
    let shape = ShapeBuilder::new(10).row_cell("Property", None, RowKey::name("Cost"), "Value", "9").row_cell(
        "Property",
        None,
        RowKey::name("Size"),
        "Value",
        "4",
    );
    let mut s = one_shape(shape);
    let end = add_row(&mut s, "Prop", json!({"name": "Weight"}));
    assert_eq!((end["row"].clone(), end["label"].clone()), (json!("Weight"), json!("Prop.Weight")));
    assert_eq!(keys(&mut s, "Prop"), [json!("Cost"), json!("Size"), json!("Weight")]);
    add_row(&mut s, "Prop", json!({"name": "Colour", "after": "Cost"}));
    add_row(&mut s, "Prop", json!({"name": "Alpha", "before": "COST"}));
    assert_eq!(
        keys(&mut s, "Prop"),
        [json!("Alpha"), json!("Cost"), json!("Colour"), json!("Size"), json!("Weight")],
        "a row's name matches in any letter case"
    );
    assert_eq!(check_session(&s), Ok(()));
}

/// A name is at most 256 characters of letters, digits and underscores, not starting with a digit, and not taken (in any letter case) in its section; each refusal is in words and
/// changes nothing.
#[test]
fn a_name_that_is_too_long_taken_or_not_a_name_is_refused_with_words() {
    let mut s = one_shape(ShapeBuilder::new(10).row_cell("Property", None, RowKey::name("Cost"), "Value", "9"));
    let anchor = state(&s);
    let long = "N".repeat(257);
    let e = refused(&mut s, "sheet.add_row", json!({"id": 10, "section": "Prop", "name": long}));
    assert!(e.contains("a row's name is at most 256 characters") && e.contains("257"), "{e}");
    let ok = "N".repeat(256);
    run(&mut s, "sheet.add_row", json!({"id": 10, "section": "Prop", "name": ok}));
    let anchor = if undo_len(&s) > anchor.1 { state(&s) } else { anchor };
    let e = refused(&mut s, "sheet.add_row", json!({"id": 10, "section": "Prop", "name": "cost"}));
    assert!(e.contains("the section already has a row Cost"), "{e}");
    for bad in ["1st", "a b", "a-b", "a.b", ""] {
        let e = refused(&mut s, "sheet.add_row", json!({"id": 10, "section": "Prop", "name": bad}));
        assert!(e.contains("a row's name"), "{bad:?}: {e}");
    }
    let e = refused(&mut s, "sheet.add_row", json!({"id": 10, "section": "Prop", "name": 5}));
    assert!(e.contains("`name` must be text"), "{e}");
    assert_eq!(state(&s), anchor, "nothing changed");
}

/// `after` and `before` name a row that is there, and not both; a row of an indexed section is its number as a reference spells it (a whole number or text of digits).
#[test]
fn the_row_an_insert_goes_by_is_named_by_its_name_or_number_and_must_be_there() {
    let mut s = one_shape(scratch_shape(3).row_cell("User", None, RowKey::name("Rate"), "Value", "5"));
    let anchor = state(&s);
    for (params, words) in [
        (json!({"id": 10, "section": "User", "after": "Nope"}), "the section has no row Nope"),
        (json!({"id": 10, "section": "User", "before": "Rate", "after": "Rate"}), "give after or before, not both"),
        (json!({"id": 10, "section": "Scratch", "after": 9}), "the section has no row 9"),
        (json!({"id": 10, "section": "Scratch", "after": 0}), "row numbers start at 1"),
        (json!({"id": 10, "section": "Scratch", "after": "x"}), "an indexed section's rows are numbered"),
        (json!({"id": 10, "section": "Scratch", "after": -1}), "`after` must be a row's name or a whole number"),
        (json!({"id": 10, "section": "Scratch", "name": "Mine"}), "rows of Scratch are numbered, not named"),
        (json!({"id": 10, "section": "User", "type": "LineTo"}), "only a Geometry row has a type"),
        (json!({"id": 10}), "missing `section`"),
        (json!({"id": 10, "section": 4}), "`section` must be text"),
        (json!({"id": 10, "section": "Nonsense"}), "no section called Nonsense"),
    ] {
        let e = refused(&mut s, "sheet.add_row", params.clone());
        assert!(e.contains(words), "{params}: {e}");
    }
    assert_eq!(state(&s), anchor, "every refusal changes nothing");
    // A number given as text of digits is the same row.
    let by_text = add_row(&mut s, "Scratch", json!({"after": "1"}));
    assert_eq!(by_text["row"], 2);
}

/// A section the sheet does not have takes no row until the section is added, and the words say so.
#[test]
fn a_row_of_a_section_the_sheet_has_not_got_is_refused_with_the_way_to_add_the_section() {
    let mut s = one_shape(ShapeBuilder::new(10));
    let e = refused(&mut s, "sheet.add_row", json!({"id": 10, "section": "Prop"}));
    assert!(e.contains("the sheet has no section Prop: add it first (sheet.add_section)"), "{e}");
    let e = refused(&mut s, "sheet.add_row", json!({"id": 10, "section": "Geometry2"}));
    assert!(e.contains("the sheet has no section Geometry2: add it first (sheet.add_section)"), "{e}");
    let e = refused(&mut s, "sheet.delete_row", json!({"id": 10, "section": "Scratch", "row": 1}));
    assert!(e.contains("the sheet has no section Scratch"), "{e}");
}

// ---- the sections that take no rows from here -------------------------------------------------------------------------------------------

/// Character, Paragraph, Tabs, Text Fields, Layers, Reviewer and Annotation rows are made by something else, and the words say what; a group of cells has no rows.
#[test]
fn rows_the_text_the_layers_or_the_comments_make_are_refused_with_the_command_that_makes_them() {
    let shape = ShapeBuilder::new(10)
        .row_cell("Character", None, RowKey::Ix(0), "Size", "0.1")
        .row_cell("Paragraph", None, RowKey::Ix(0), "HorzAlign", "1")
        .row_cell("Tabs", None, RowKey::Ix(0), "Position", "1")
        .row_cell("Field", None, RowKey::Ix(0), "Format", "0");
    let mut s = one_shape(shape);
    let anchor = state(&s);
    for (section, words) in [
        ("Char", "Character rows are made by the text: format the text instead (text.format)"),
        ("Para", "Paragraph rows are made by the text: format the text instead (text.format)"),
        ("Tabs", "Tabs rows are made by the text: format the text instead (text.format)"),
        ("Fields", "Text Fields rows are made by the text: insert a field instead (text.insert_field)"),
        ("Shape Transform", "Shape Transform is a group of cells and has no rows"),
        ("Protection", "Protection is a group of cells and has no rows"),
    ] {
        let e = refused(&mut s, "sheet.add_row", json!({"id": 10, "section": section}));
        assert!(e.contains(words), "{section}: {e}");
        let e = refused(&mut s, "sheet.delete_row", json!({"id": 10, "section": section, "row": 1}));
        assert!(e.contains(words.split(':').next().unwrap()), "{section}: {e}");
    }
    assert_eq!(state(&s), anchor);
    // A page's Layers rows are the layer commands'; the document's Reviewer rows are the comments'.
    let mut s = session_of(
        DocBuilder::new()
            .page(1, PageBuilder::new().set(CellAddr::in_row("Layer", None, RowKey::Ix(0), "Name"), Cell::constant(Held::from_text("Layer one"))))
            .build(),
    );
    let e = refused(&mut s, "sheet.add_row", json!({"page": 1, "section": "Layers"}));
    assert!(e.contains("Layers rows are made by layer.add and layer.delete, which keep the members' numbers right"), "{e}");
    let e = refused(&mut s, "sheet.delete_row", json!({"page": 1, "section": "Layers", "row": 1}));
    assert!(e.contains("layer.delete"), "{e}");
    let e = refused(&mut s, "sheet.add_row", json!({"document": true, "section": "Reviewer"}));
    assert!(e.contains("Reviewer rows are made by the comments (comment.add), not here"), "{e}");
    let e = refused(&mut s, "sheet.add_row", json!({"page": 1, "section": "Annotation"}));
    assert!(e.contains("Annotation rows are made by the comments (comment.add), not here"), "{e}");
}

// ---- Geometry rows: the numbers -------------------------------------------------------------

/// An insert after row 2 of five takes number 3 and numbers the rows after it again, and **the formulas that name them follow**: text outside the references is kept
/// as typed. The new row's `X` and `Y` are those of the row before it, so that the drawing does not change until it is edited.
#[test]
fn an_insert_in_the_middle_numbers_the_later_rows_again_and_the_formulas_that_name_them_follow() {
    let shape = geometry_shape(5)
        .row_formula("Scratch", None, RowKey::Ix(0), "A", "Geometry1.X4+Geometry1.Y3 * 2", "34")
        .row_formula("Scratch", None, RowKey::Ix(0), "B", "  Geometry1.X2+Width*0.5 ", "5")
        .row_formula("User", None, RowKey::name("Total"), "Value", "Geometry1.X5", "5")
        .row_formula("User", None, RowKey::name("Text"), "Value", "\"Geometry1.X4\" & Geometry1.X4", "\"x\"");
    let mut s = one_shape(shape);
    let answer = add_row(&mut s, "Geometry1", json!({"after": 2}));
    assert_eq!((answer["row"].clone(), answer["label"].clone(), answer["type"].clone()), (json!(3), json!("3"), json!("LineTo")), "{answer}");
    assert_eq!(answer["renumbered"]["rows"], 3, "rows 3, 4 and 5 moved: {answer}");
    assert_eq!(answer["renumbered"]["formulas"], 3, "{answer}");
    assert_eq!(keys(&mut s, "Geometry1"), [json!(1), json!(2), json!(3), json!(4), json!(5), json!(6)]);
    // The new row is the row before it, in place.
    assert_eq!((num(&mut s, "Geometry1.X3"), num(&mut s, "Geometry1.Y3")), (2.0, 20.0));
    // The rows that moved hold what they held.
    assert_eq!((num(&mut s, "Geometry1.X4"), num(&mut s, "Geometry1.X5"), num(&mut s, "Geometry1.X6")), (3.0, 4.0, 5.0));
    // The formulas that named them name them still, and the text round them is as it was.
    assert_eq!(formula(&mut s, "Scratch.A1"), "Geometry1.X5+Geometry1.Y4 * 2");
    assert_eq!(formula(&mut s, "Scratch.B1"), "  Geometry1.X2+Width*0.5 ", "a reference to a row that did not move is not touched");
    assert_eq!(formula(&mut s, "User.Total.Value"), "Geometry1.X6");
    assert_eq!(formula(&mut s, "User.Text.Value"), "\"Geometry1.X4\" & Geometry1.X5", "text inside a string is no reference");
    // What they read is what they read: X4 + Y3 * 2 = 4 + 30 * 2 and X2 + Width * 0.5 = 2 + 3, as before the insert.
    assert_eq!((num(&mut s, "Scratch.A1"), num(&mut s, "Scratch.B1")), (64.0, 5.0));
    assert_eq!(s.history().unwrap().next_undo_label(), Some("Insert row"));
    assert_eq!(check_session(&s), Ok(()));
}

/// V6 of the window's spec: Scratch rows 1, 2, 3 with row 2 selected: Insert Row (before) gives the new row as row 2 and the old rows 2 and 3 as rows 3 and 4; Geometry row 2
/// selected, Insert Row After: the new row is row 3.
#[test]
fn an_insert_before_a_row_takes_its_number_and_one_after_a_row_the_next() {
    let mut s = one_shape(scratch_shape(3));
    let answer = add_row(&mut s, "Scratch", json!({"before": 2}));
    assert_eq!(answer["row"], 2, "{answer}");
    assert_eq!(keys(&mut s, "Scratch"), [json!(1), json!(2), json!(3), json!(4)]);
    assert_eq!((num(&mut s, "Scratch.A1"), num(&mut s, "Scratch.A3"), num(&mut s, "Scratch.A4")), (10.0, 20.0, 30.0));
    assert_eq!(num(&mut s, "Scratch.A2"), 0.0, "a new row holds the default of its cells");
    let mut s = one_shape(geometry_shape(3));
    let answer = add_row(&mut s, "Geometry1", json!({"after": 2}));
    assert_eq!(answer["row"], 3);
    assert_eq!(keys(&mut s, "Geometry1"), [json!(1), json!(2), json!(3), json!(4)]);
}

/// A row added with no `after` and no `before` goes at the end: one past the largest number.
#[test]
fn a_row_added_with_no_place_goes_at_the_end() {
    let mut s = one_shape(scratch_shape(3).formula("Height", "Scratch.A3*2", "60"));
    let answer = add_row(&mut s, "Scratch", json!({}));
    assert_eq!(answer["row"], 4);
    assert_eq!(answer["renumbered"]["rows"], 0, "nothing moved");
    assert_eq!(answer["renumbered"]["formulas"], 0);
    assert_eq!(keys(&mut s, "Scratch"), [json!(1), json!(2), json!(3), json!(4)]);
    assert_eq!(formula(&mut s, "Height"), "Scratch.A3*2");
}

/// An insert into a gap a file left (rows 1, 2, 4, 5) takes the number that is free and moves nothing.
#[test]
fn an_insert_into_a_gap_takes_the_free_number_and_renumbers_nothing() {
    let mut shape = ShapeBuilder::new(10).formula("Width", "Geometry1.X4", "4");
    for k in [1_u32, 2, 4, 5] {
        shape = shape
            .row_type("Geometry", Some(0), RowKey::Ix(k), if k == 1 { "MoveTo" } else { "LineTo" })
            .row_cell("Geometry", Some(0), RowKey::Ix(k), "X", &k.to_string())
            .row_cell("Geometry", Some(0), RowKey::Ix(k), "Y", "0");
    }
    let mut s = one_shape(shape);
    let answer = add_row(&mut s, "Geometry1", json!({"after": 2}));
    assert_eq!(answer["row"], 3, "{answer}");
    assert_eq!((answer["renumbered"]["rows"].clone(), answer["renumbered"]["formulas"].clone()), (json!(0), json!(0)));
    assert_eq!(keys(&mut s, "Geometry1"), [json!(1), json!(2), json!(3), json!(4), json!(5)]);
    assert_eq!(formula(&mut s, "Width"), "Geometry1.X4", "no number changed, so no formula");
}

/// V8 of the window's spec: a Geometry section of rows 1 to 5, row 3 deleted: the rows are numbered 1 to 4 and the old row 4 is row 3; a formula that named row 4 names
/// the row it named (now 3), one that named row 5 names 4, and **one that named the row that went reads `#REF!`**, the call `REF()`.
#[test]
fn a_delete_closes_the_numbers_up_and_a_formula_that_named_the_row_that_went_reads_ref() {
    let shape = geometry_shape(5)
        .row_formula("Scratch", None, RowKey::Ix(0), "A", "Geometry1.X3", "3")
        .row_formula("Scratch", None, RowKey::Ix(0), "B", "Geometry1.X4+Geometry1.X5", "9")
        .row_formula("Scratch", None, RowKey::Ix(0), "C", "Scratch.A1+1", "4")
        .row_formula("Scratch", None, RowKey::Ix(0), "D", "Geometry1.X1", "1");
    let mut s = one_shape(shape);
    let answer = delete_row(&mut s, "Geometry1", json!(3));
    assert_eq!((answer["row"].clone(), answer["how"].clone()), (json!(3), json!("removed")), "{answer}");
    assert_eq!(answer["renumbered"]["rows"], 2, "{answer}");
    assert_eq!(keys(&mut s, "Geometry1"), [json!(1), json!(2), json!(3), json!(4)]);
    assert_eq!((num(&mut s, "Geometry1.X3"), num(&mut s, "Geometry1.X4")), (4.0, 5.0), "the old rows 4 and 5");
    assert_eq!(formula(&mut s, "Scratch.B1"), "Geometry1.X3+Geometry1.X4");
    assert_eq!(formula(&mut s, "Scratch.D1"), "Geometry1.X1");
    assert_eq!(formula(&mut s, "Scratch.A1"), "REF()", "the reference to the row that went reads #REF!");
    let a = cell(&mut s, "Scratch.A1");
    assert_eq!(a["error"], "#REF!", "{a}");
    assert_eq!(a["value"], json!(3.0), "the last good value stands");
    // A cell that reads the failing cell reads its last good value, with no error (the coordinator's ruling 2026-11-26).
    let reader = cell(&mut s, "Scratch.C1");
    assert_eq!((reader["error"].clone(), reader["value"].clone()), (NULL, json!(4.0)), "{reader}");
    assert_eq!(s.history().unwrap().next_undo_label(), Some("Delete row"));
    assert_eq!(check_session(&s), Ok(()));
}

/// A delete and then an insert at the same place give the row its number again (design 11).
#[test]
fn a_delete_then_an_insert_at_the_same_place_reuses_the_number() {
    let mut s = one_shape(scratch_shape(5));
    delete_row(&mut s, "Scratch", json!(3));
    assert_eq!(keys(&mut s, "Scratch"), [json!(1), json!(2), json!(3), json!(4)]);
    let answer = add_row(&mut s, "Scratch", json!({"after": 2}));
    assert_eq!(answer["row"], 3, "the number the row had: {answer}");
    assert_eq!(keys(&mut s, "Scratch"), [json!(1), json!(2), json!(3), json!(4), json!(5)]);
    assert_eq!((num(&mut s, "Scratch.A4"), num(&mut s, "Scratch.A5")), (40.0, 50.0), "the rows after it are where they were");
}

/// An insert that would pass the largest number a row can have is refused in words and changes nothing; so is one that would move a row past it.
#[test]
fn an_insert_past_the_largest_row_number_is_refused() {
    let shape = ShapeBuilder::new(10)
        .row_type("Geometry", Some(0), RowKey::Ix(1), "MoveTo")
        .row_cell("Geometry", Some(0), RowKey::Ix(1), "X", "0")
        .row_type("Geometry", Some(0), RowKey::Ix(u32::MAX - 1), "LineTo")
        .row_cell("Geometry", Some(0), RowKey::Ix(u32::MAX - 1), "X", "0")
        .row_type("Geometry", Some(0), RowKey::Ix(u32::MAX), "LineTo")
        .row_cell("Geometry", Some(0), RowKey::Ix(u32::MAX), "X", "0");
    let mut s = one_shape(shape);
    let anchor = state(&s);
    let e = refused(&mut s, "sheet.add_row", json!({"id": 10, "section": "Geometry1"}));
    assert!(e.contains("no room for a row after row 4294967295: row numbers end at 4294967295"), "{e}");
    let e = refused(&mut s, "sheet.add_row", json!({"id": 10, "section": "Geometry1", "after": 4_294_967_294_u64}));
    assert!(e.contains("row numbers end at 4294967295"), "a row that would be moved past the last number: {e}");
    assert_eq!(state(&s), anchor);
    // The gap between 1 and the others takes a row.
    let ok = add_row(&mut s, "Geometry1", json!({"after": 1}));
    assert_eq!(ok["row"], 2);
}

/// The first row of a Geometry section must be a move, an ellipse or an infinite line: a row inserted before it, or the first row deleted while others stay, is refused;
/// a MoveTo may go first, and the last row may be deleted.
#[test]
fn the_first_row_of_a_geometry_section_stays_a_move() {
    let mut s = one_shape(geometry_shape(3));
    let anchor = state(&s);
    let e = refused(&mut s, "sheet.add_row", json!({"id": 10, "section": "Geometry1", "before": 1}));
    assert!(e.contains("the first row of a Geometry section must be a MoveTo, RelMoveTo, Ellipse or InfiniteLine"), "{e}");
    let e = refused(&mut s, "sheet.delete_row", json!({"id": 10, "section": "Geometry1", "row": 1}));
    assert!(e.contains("the first row of a Geometry section must be a MoveTo"), "{e}");
    assert_eq!(state(&s), anchor);
    let moved = add_row(&mut s, "Geometry1", json!({"before": 1, "type": "MoveTo"}));
    assert_eq!((moved["row"].clone(), moved["type"].clone()), (json!(1), json!("MoveTo")));
    assert_eq!(
        rows(&mut s, "Geometry1").iter().map(|(_, t)| t.as_str().unwrap().to_owned()).collect::<Vec<_>>(),
        ["MoveTo", "MoveTo", "LineTo", "LineTo"]
    );
    delete_row(&mut s, "Geometry1", json!(4));
    assert_eq!(keys(&mut s, "Geometry1").len(), 3);
}

/// `type` is one of the fifteen Geometry row types and the new row holds the cells its type uses, at 0 (X and Y those of the row before): an unknown type is refused, listing them.
#[test]
fn a_geometry_row_takes_a_type_and_the_cells_that_type_uses() {
    let mut s = one_shape(geometry_shape(2));
    let arc = add_row(&mut s, "Geometry1", json!({"type": "ArcTo"}));
    assert_eq!((arc["row"].clone(), arc["type"].clone()), (json!(3), json!("ArcTo")));
    let names =
        |answer: &Value| -> Vec<String> { answer["cells"].as_array().unwrap().iter().map(|c| c["name"].as_str().unwrap().to_owned()).collect() };
    assert_eq!(names(&arc), ["X", "Y", "A"], "ArcTo uses X, Y and the bow A");
    assert_eq!((num(&mut s, "Geometry1.X3"), num(&mut s, "Geometry1.Y3"), num(&mut s, "Geometry1.A3")), (2.0, 20.0, 0.0));
    let ell = add_row(&mut s, "Geometry1", json!({"type": "EllipticalArcTo"}));
    assert_eq!(names(&ell), ["X", "Y", "A", "B", "C", "D"]);
    let quad = add_row(&mut s, "Geometry1", json!({"type": "RelQuadBezTo"}));
    assert_eq!(names(&quad), ["X", "Y", "A", "B"]);
    let line = add_row(&mut s, "Geometry1", json!({}));
    assert_eq!((line["type"].clone(), names(&line)), (json!("LineTo"), vec!["X".to_owned(), "Y".to_owned()]), "the default type");
    for ty in [
        "MoveTo",
        "LineTo",
        "ArcTo",
        "EllipticalArcTo",
        "NURBSTo",
        "PolylineTo",
        "SplineStart",
        "SplineKnot",
        "RelMoveTo",
        "RelLineTo",
        "RelCubBezTo",
        "RelQuadBezTo",
        "RelEllipticalArcTo",
    ] {
        let made = add_row(&mut s, "Geometry1", json!({"type": ty}));
        assert_eq!(made["type"], ty);
    }
    // The other two types are rows that are alone in their section: a section that holds rows takes neither; each is the one row of a section of its own.
    for ty in ["Ellipse", "InfiniteLine"] {
        let e = refused(&mut s, "sheet.add_row", json!({"id": 10, "section": "Geometry1", "type": ty}));
        assert!(e.contains("can only be added to an empty section"), "{ty}: {e}");
        let own = run(&mut s, "sheet.add_section", json!({"id": 10, "section": "Geometry", "variant": ty}));
        assert_eq!(rows(&mut s, own["section"].as_str().unwrap()).len(), 1, "{ty}");
    }
    let anchor = state(&s);
    let e = refused(&mut s, "sheet.add_row", json!({"id": 10, "section": "Geometry1", "type": "Bogus"}));
    assert!(e.contains("Bogus is no Geometry row type") && e.contains("MoveTo") && e.contains("RelEllipticalArcTo"), "{e}");
    let e = refused(&mut s, "sheet.add_row", json!({"id": 10, "section": "Geometry1", "type": 3}));
    assert!(e.contains("`type` must be text"), "{e}");
    assert_eq!(state(&s), anchor);
    assert_eq!(check_session(&s), Ok(()));
}

// ---- the formulas of other sheets, and the glue ---------------------------------------------------------------------------------------

/// A row of one shape that other shapes, the page and the document read through `Sheet.<id>!`, `ThePage!` and `TheDoc!` is numbered again with the formulas that name it
/// followed wherever they are (the design's refusal of this case was replaced by R29: rewrite them), and the answer counts them.
#[test]
fn the_formulas_of_other_shapes_and_the_page_that_name_a_row_follow_it() {
    let reader = |id: u32, text: &str, value: &str| ShapeBuilder::new(id).formula("Width", text, value);
    let page = PageBuilder::new()
        .set(CellAddr::direct("PageWidth"), Cell::with_formula("Sheet.10!Scratch.A3+5", Some(Held::from_text("35"))))
        .shape(scratch_shape(4))
        .shape(reader(11, "Sheet.10!Scratch.A3*2", "60"))
        .shape(reader(12, "Sheet.10!Scratch.A1+Sheet.10!Scratch.A4", "50"))
        .shape(ShapeBuilder::new(13).named("Other", "Other").child(reader(14, "Sheet.10!Scratch.A3", "30")));
    let mut s = session_of(DocBuilder::new().page(1, page).build());
    let answer = add_row(&mut s, "Scratch", json!({"after": 1}));
    assert_eq!(answer["renumbered"]["formulas"], 4, "{answer}");
    let d = doc(&s);
    let width = |id: u32| -> String {
        let at = d.find_shape(Owner::Page(1), id).unwrap();
        d.shape(&at).unwrap().sheet.cell("Width").unwrap().formula.text().unwrap().to_owned()
    };
    assert_eq!(width(11), "Sheet.10!Scratch.A4*2");
    assert_eq!(width(12), "Sheet.10!Scratch.A1+Sheet.10!Scratch.A5");
    assert_eq!(width(14), "Sheet.10!Scratch.A4");
    assert_eq!(d.page(1).unwrap().sheet.cell("PageWidth").unwrap().formula.text(), Some("Sheet.10!Scratch.A4+5"));
    // What they read is what they read: the same numbers.
    assert_eq!(run(&mut s, "sheet.cell", json!({"id": 11, "cell": "Width"}))["value"], json!(60.0));
    assert_eq!(run(&mut s, "sheet.cell", json!({"page": 1, "cell": "PageWidth"}))["value"], json!(35.0));
    assert_eq!(check_session(&s), Ok(()));
}

/// A formula of the document's sheet that names a row of a shape is not followed because it cannot: only `TheDoc!` reaches the document, and a shape's row is not its; but the
/// document's own rows are read as `TheDoc!User.Rate.Value` and a row of the document's Scratch section is numbered again with them (`TheDoc!Scratch.A3`).
#[test]
fn the_rows_of_the_documents_sheet_are_read_through_thedoc_and_follow() {
    let mut doc = DocBuilder::new().page(1, PageBuilder::new().shape(ShapeBuilder::new(10).formula("Width", "TheDoc!Scratch.A3*2", "6"))).build();
    let sheet = Arc::make_mut(&mut doc.sheet);
    for k in 0..4_u32 {
        sheet.set_cell(&CellAddr::in_row("Scratch", None, RowKey::Ix(k), "A"), Cell::constant(Held::from_text(&(k + 1).to_string()))).unwrap();
    }
    let mut s = session_of(doc);
    let answer = run(&mut s, "sheet.add_row", json!({"document": true, "section": "Scratch", "after": 1}));
    assert_eq!(answer["sheet"], json!({"kind": "document"}));
    assert_eq!(formula(&mut s, "Width"), "TheDoc!Scratch.A4*2");
    assert_eq!(num(&mut s, "Width"), 6.0);
}

/// A connector glued to connection point 3 of a shape (`Sheet.20!Connections.X3`, `Connect` `ToPart` 102 and `ToCell` `Connections.X3`) follows the point when a connection
/// point is inserted before it: the formulas, the `ToPart` and the `ToCell` name point 4, and the connector does not move. A point deleted before it moves it up one; the point
/// that went leaves its connector's end reading `#REF!` and the `Connect` row gone.
#[test]
fn a_connector_follows_its_connection_point_when_points_are_inserted_or_deleted() {
    let point = |shape: ShapeBuilder, ix: u32, x: &str, y: &str| {
        shape.row_cell("Connection", None, RowKey::Ix(ix), "X", x).row_cell("Connection", None, RowKey::Ix(ix), "Y", y)
    };
    let boxed = |id: u32, pin_x: &str| {
        let shape = ShapeBuilder::new(id)
            .cell_u("Width", "2", "IN")
            .cell_u("Height", "1", "IN")
            .cell_u("PinX", pin_x, "IN")
            .cell_u("PinY", "4", "IN")
            .cell_u("LocPinX", "1", "IN")
            .cell_u("LocPinY", "0.5", "IN");
        let shape = point(shape, 0, "0", "0.5");
        let shape = point(shape, 1, "1", "1");
        point(shape, 2, "2", "0.5")
    };
    let mut s = session_of(DocBuilder::new().page(1, PageBuilder::new().shape(boxed(20, "3")).shape(boxed(21, "9"))).build());
    let joined = run(&mut s, "connector.add", json!({"from": {"id": 20, "point": 2}, "to": {"id": 21, "point": 0}}));
    let connector = u32::try_from(joined["id"].as_u64().unwrap()).unwrap();
    let begin_x = |s: &mut Session| run(s, "sheet.cell", json!({"id": connector, "cell": "BeginX"}));
    let at = begin_x(&mut s)["value"].clone();
    assert_eq!(begin_x(&mut s)["formula"], "PAR(PNT(Sheet.20!Connections.X3,Sheet.20!Connections.Y3))");
    let connects = |s: &Session| -> Vec<(u32, String, u32, String, u32)> {
        doc(s)
            .page(1)
            .unwrap()
            .connects
            .iter()
            .map(|c| {
                (
                    c.from_sheet,
                    c.from_cell.as_deref().unwrap_or("").to_owned(),
                    c.to_sheet,
                    c.to_cell.as_deref().unwrap_or("").to_owned(),
                    c.to_part.unwrap_or(0),
                )
            })
            .collect()
    };
    assert!(connects(&s).contains(&(connector, "BeginX".to_owned(), 20, "Connections.X3".to_owned(), 102)), "{:?}", connects(&s));

    let answer = run(&mut s, "sheet.add_row", json!({"id": 20, "section": "Connections", "after": 1}));
    assert_eq!((answer["row"].clone(), answer["renumbered"]["rows"].clone()), (json!(2), json!(2)), "{answer}");
    assert!(answer["renumbered"]["formulas"].as_u64().unwrap() >= 2, "the begin cells of the connector: {answer}");
    assert_eq!(answer["renumbered"]["connects"], 1, "{answer}");
    assert_eq!(begin_x(&mut s)["formula"], "PAR(PNT(Sheet.20!Connections.X4,Sheet.20!Connections.Y4))");
    assert_eq!(begin_x(&mut s)["value"], at, "the connector did not move");
    assert!(connects(&s).contains(&(connector, "BeginX".to_owned(), 20, "Connections.X4".to_owned(), 103)), "{:?}", connects(&s));
    assert!(connects(&s).iter().any(|c| c.2 == 21 && c.4 == 100), "a glue to another shape is as it was");

    // A point deleted before it moves it up one.
    run(&mut s, "sheet.delete_row", json!({"id": 20, "section": "Connections", "row": 1}));
    assert_eq!(begin_x(&mut s)["formula"], "PAR(PNT(Sheet.20!Connections.X3,Sheet.20!Connections.Y3))");
    assert_eq!(begin_x(&mut s)["value"], at);
    assert!(connects(&s).contains(&(connector, "BeginX".to_owned(), 20, "Connections.X3".to_owned(), 102)));
    // The point it is glued to deleted: the end reads #REF! and keeps where it was, and the Connect row is gone.
    run(&mut s, "sheet.delete_row", json!({"id": 20, "section": "Connections", "row": 3}));
    let gone = begin_x(&mut s);
    assert_eq!(
        (gone["formula"].clone(), gone["error"].clone(), gone["value"].clone()),
        (json!("PAR(PNT(REF(),REF()))"), json!("#REF!"), at),
        "{gone}"
    );
    assert!(!connects(&s).iter().any(|c| c.0 == connector && c.1 == "BeginX"), "no glue to a point that is not there: {:?}", connects(&s));
    assert_eq!(check_session(&s), Ok(()));
}

/// Two boxes, 20 and 21, with three connection points each and a connector from point 3 of 20 to point 1 of 21: the session and the connector's `ID`.
fn glued_pair() -> (Session, u32) {
    let boxed = |id: u32, pin_x: &str| {
        let mut shape = ShapeBuilder::new(id)
            .cell_u("Width", "2", "IN")
            .cell_u("Height", "1", "IN")
            .cell_u("PinX", pin_x, "IN")
            .cell_u("PinY", "4", "IN")
            .cell_u("LocPinX", "1", "IN")
            .cell_u("LocPinY", "0.5", "IN");
        for (ix, x, y) in [(0_u32, "0", "0.5"), (1, "1", "1"), (2, "2", "0.5")] {
            shape = shape.row_cell("Connection", None, RowKey::Ix(ix), "X", x).row_cell("Connection", None, RowKey::Ix(ix), "Y", y);
        }
        shape
    };
    let mut s = session_of(DocBuilder::new().page(1, PageBuilder::new().shape(boxed(20, "3")).shape(boxed(21, "9"))).build());
    let joined = run(&mut s, "connector.add", json!({"from": {"id": 20, "point": 2}, "to": {"id": 21, "point": 0}}));
    let connector = u32::try_from(joined["id"].as_u64().unwrap()).unwrap();
    (s, connector)
}

/// A whole section of connection points that goes takes the glue to its points with it: the `Connect` rows to them are dropped, the connector's ends that were glued there read
/// `#REF!`, and the glue to another shape is as it was.
#[test]
fn a_connection_section_that_goes_takes_the_glue_to_its_points_with_it() {
    let (mut s, connector) = glued_pair();
    let gone = run(&mut s, "sheet.delete_section", json!({"id": 20, "section": "Connections"}));
    assert_eq!((gone["how"].clone(), gone["renumbered"]["connects"].clone()), (json!("removed"), json!(1)), "{gone}");
    let begin = run(&mut s, "sheet.cell", json!({"id": connector, "cell": "BeginX"}));
    assert_eq!((begin["formula"].clone(), begin["error"].clone()), (json!("PAR(PNT(REF(),REF()))"), json!("#REF!")), "{begin}");
    let connects: Vec<(u32, u32)> = doc(&s).page(1).unwrap().connects.iter().map(|c| (c.from_sheet, c.to_sheet)).collect();
    assert!(!connects.iter().any(|(from, to)| *from == connector && *to == 20), "{connects:?}");
    assert!(connects.iter().any(|(from, to)| *from == connector && *to == 21), "the glue to the other shape stays: {connects:?}");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(
        run(&mut s, "sheet.cell", json!({"id": connector, "cell": "BeginX"}))["formula"],
        "PAR(PNT(Sheet.20!Connections.X3,Sheet.20!Connections.Y3))"
    );
    assert_eq!(check_session(&s), Ok(()));
}

/// The other side of R56 (the coordinator's ruling of 2026-11-26): a reference to a failing cell that has **no value** to fall back on errors. A formula cell that had no cached
/// value and named a row that went reads `#REF!`, and the cell that reads it reads `#REF!` too; with a value (the test of the delete above) it keeps that value and reads no error.
#[test]
fn a_cell_that_named_a_row_that_went_and_had_no_value_makes_the_cells_that_read_it_error() {
    let shape = geometry_shape(5).set(CellAddr::in_row("Scratch", None, RowKey::Ix(0), "A"), Cell::with_formula("Geometry1.X3", None)).row_formula(
        "Scratch",
        None,
        RowKey::Ix(0),
        "B",
        "Scratch.A1+1",
        "4",
    );
    let mut s = one_shape(shape);
    delete_row(&mut s, "Geometry1", json!(3));
    // The cell itself has no value to keep, so calc's commit records nothing on it (`plan_write`); the cell that reads it has a value, keeps it, and records the error.
    assert_eq!(cell(&mut s, "Scratch.A1")["formula"], "REF()");
    let b = cell(&mut s, "Scratch.B1");
    assert_eq!((b["error"].clone(), b["value"].clone()), (json!("#REF!"), json!(4.0)), "no value to fall back on, so the error is read: {b}");
}

/// A formula on another page that reaches the shape through `Pages[..]` is followed, and the formulas of that page that do not hold the qualifier are not even read (the
/// work says so); a formula that does not parse and one that builds its reference as text are left as they are.
#[test]
fn a_formula_on_another_page_is_followed_through_pages_and_one_that_cannot_be_read_is_left_alone() {
    let page_one = PageBuilder::new().name("Page-1").shape(scratch_shape(4).formula("Height", "Scratch.A3 +", "0").formula(
        "Angle",
        "EVALTEXT(\"Scratch.A3\")",
        "0",
    ));
    let page_two = PageBuilder::new()
        .name("Page-2")
        .shape(ShapeBuilder::new(11).formula("Width", "Pages[Page-1]!Sheet.10!Scratch.A3*2", "60"))
        .shape(ShapeBuilder::new(12).formula("Width", "Scratch.A3*2", "0"));
    let mut s = session_of(DocBuilder::new().page(1, page_one).page(2, page_two).build());
    let answer = add_row(&mut s, "Scratch", json!({"after": 1}));
    assert_eq!(answer["renumbered"]["formulas"], 1, "{answer}");
    let work = &answer["renumbered"]["work"];
    assert_eq!(
        (work["parsed"].clone(), work["located"].clone()),
        (json!(2), json!(1)),
        "the two of page 1 that hold the name, and not the one of page 2 that has no Pages: {work}"
    );
    let width = |s: &mut Session, id: u32, page: u32| run(s, "sheet.cell", json!({"id": id, "page": page, "cell": "Width"}))["formula"].clone();
    assert_eq!(width(&mut s, 11, 2), "Pages[Page-1]!Sheet.10!Scratch.A4*2");
    assert_eq!(width(&mut s, 12, 2), "Scratch.A3*2", "a reference to the sheet's own Scratch, which is not the one that moved");
    assert_eq!(formula(&mut s, "Height"), "Scratch.A3 +", "a formula that does not parse is kept as the file has it");
    assert_eq!(formula(&mut s, "Angle"), "EVALTEXT(\"Scratch.A3\")", "a reference built as text is chosen when the formula runs");
}

/// The rows of the document's sheet are read by `TheDoc!` from every page, master and style, and are followed in all of them.
#[test]
fn a_master_and_a_style_that_read_the_documents_rows_follow_them() {
    let mut built = DocBuilder::new()
        .style(3, StyleBuilder::new().name("Heavy").formula("LineWeight", "TheDoc!Scratch.A3*0.01", "0.03"))
        .master(1, MasterBuilder::new().shape(ShapeBuilder::new(5).formula("Width", "TheDoc!Scratch.A3", "3")))
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).master(1)))
        .build();
    let sheet = Arc::make_mut(&mut built.sheet);
    for k in 0..4_u32 {
        sheet.set_cell(&CellAddr::in_row("Scratch", None, RowKey::Ix(k), "A"), Cell::constant(Held::from_text(&(k + 1).to_string()))).unwrap();
    }
    let mut s = session_of(built);
    let answer = run(&mut s, "sheet.add_row", json!({"document": true, "section": "Scratch", "after": 1}));
    assert_eq!(answer["renumbered"]["formulas"], 2, "{answer}");
    let d = doc(&s);
    assert_eq!(d.master(1).unwrap().shapes.get(0).unwrap().sheet.cell("Width").unwrap().formula.text(), Some("TheDoc!Scratch.A4"));
    assert_eq!(d.style(3).unwrap().sheet.cell("LineWeight").unwrap().formula.text(), Some("TheDoc!Scratch.A4*0.01"));
    // The instance reads the master's formula on its own page: the row it read is the row it reads.
    assert_eq!(run(&mut s, "sheet.cell", json!({"id": 10, "cell": "Width"}))["value"], json!(3.0));
}

/// A named row a master gives comes first among an instance's rows, so a row cannot be put after it (or before it) and the sheet's own rows still follow: the end is the place,
/// and the words say so.
#[test]
fn a_row_cannot_be_put_by_a_named_row_a_master_gives_and_goes_at_the_end() {
    let master_shape = ShapeBuilder::new(5).row_cell("User", None, RowKey::name("Cost"), "Value", "9");
    let built = DocBuilder::new()
        .master(1, MasterBuilder::new().shape(master_shape))
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).master(1)))
        .build();
    let mut s = session_of(built);
    let e = refused(&mut s, "sheet.add_row", json!({"id": 10, "section": "User", "after": "Cost"}));
    assert!(e.contains("a row of User that the master gives comes before the sheet's own rows: add the row at the end"), "{e}");
    assert_eq!(add_row(&mut s, "User", json!({}))["row"], "Row_1");
    assert_eq!(keys(&mut s, "User"), [json!("Cost"), json!("Row_1")]);
    // And now the sheet's own row can be gone by.
    assert_eq!(add_row(&mut s, "User", json!({"before": "row_1", "name": "Mine"}))["row"], "Mine");
    assert_eq!(keys(&mut s, "User"), [json!("Cost"), json!("Mine"), json!("Row_1")]);
}

/// A named row that goes is not renumbered (a name does not move), so a formula that names it is left as it is and reads `#REF!` as the evaluator reads a row nothing supplies.
#[test]
fn a_named_row_that_goes_leaves_the_formulas_that_name_it_to_read_ref() {
    let shape = ShapeBuilder::new(10)
        .row_cell("User", None, RowKey::name("Cost"), "Value", "9")
        .row_cell("User", None, RowKey::name("Size"), "Value", "4")
        .formula("Height", "User.Cost.Value+1", "10")
        .formula("Width", "User.Size.Value+1", "5");
    let mut s = one_shape(shape);
    let gone = delete_row(&mut s, "User", json!("COST"));
    assert_eq!((gone["how"].clone(), gone["renumbered"]["formulas"].clone()), (json!("removed"), json!(0)), "{gone}");
    assert_eq!(formula(&mut s, "Height"), "User.Cost.Value+1");
    assert_eq!(cell(&mut s, "Height")["error"], "#REF!");
    assert_eq!((formula(&mut s, "Width"), cell(&mut s, "Width")["error"].clone()), ("User.Size.Value+1".to_owned(), NULL));
}

/// A fixed sequence of inserts and deletes, at the end, before a row and after a row, over rows that formulas point at, each checked against a model: every formula still names the
/// row it named, by that row's number now, and reads its value; the formula of a row that went reads `REF()`. Three sequences of 120 steps.
#[test]
fn random_inserts_and_deletes_keep_every_reference_on_the_row_it_named() {
    struct Entry {
        id: u32,
        /// The id of the row its `B` cell reads the `A` of, and whether that row is gone.
        points_at: Option<u32>,
        gone: bool,
    }
    for seed in [1_u64, 7, 4242] {
        let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
        let mut next = move |below: usize| -> usize {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            usize::try_from(state % u64::try_from(below).unwrap()).unwrap()
        };
        // Six rows to begin: row i holds A = 100 + i, and B reads the A of the next row round.
        let mut model: Vec<Entry> = (1..=6_u32).map(|id| Entry { id, points_at: Some(id % 6 + 1), gone: false }).collect();
        let mut shape = ShapeBuilder::new(10);
        for e in &model {
            let at = e.id - 1;
            let target = e.points_at.unwrap();
            shape = shape.row_cell("Scratch", None, RowKey::Ix(at), "A", &(100 + e.id).to_string()).row_formula(
                "Scratch",
                None,
                RowKey::Ix(at),
                "B",
                &format!("Scratch.A{target}"),
                &(100 + target).to_string(),
            );
        }
        let mut s = one_shape(shape);
        let mut next_id = 7_u32;
        let number_of = |model: &[Entry], id: u32| model.iter().position(|e| e.id == id).map(|at| at + 1);
        for step in 0..120 {
            let len = model.len();
            let delete = len > 2 && (len >= 30 || next(100) < 45);
            if delete {
                let at = next(len);
                run(&mut s, "sheet.delete_row", json!({"id": 10, "section": "Scratch", "row": at + 1}));
                let gone = model.remove(at).id;
                for e in model.iter_mut().filter(|e| e.points_at == Some(gone)) {
                    e.gone = true;
                }
            } else {
                let place = next(len + 1);
                let params = match (place, place == len, next(2) == 0) {
                    (_, true, _) => json!({"id": 10, "section": "Scratch"}),
                    (0, _, _) | (_, false, true) => json!({"id": 10, "section": "Scratch", "before": place + 1}),
                    _ => json!({"id": 10, "section": "Scratch", "after": place}),
                };
                let made = run(&mut s, "sheet.add_row", params);
                assert_eq!(made["row"], place + 1, "seed {seed} step {step}: {made}");
                let id = next_id;
                next_id += 1;
                let pointee = model.get(next(len)).map(|e| e.id);
                model.insert(place, Entry { id, points_at: pointee, gone: false });
                let number = place + 1;
                run(&mut s, "sheet.set_cell", json!({"id": 10, "cell": format!("Scratch.A{number}"), "formula": (100 + id).to_string()}));
                if let Some(target) = pointee.and_then(|t| number_of(&model, t)) {
                    run(&mut s, "sheet.set_cell", json!({"id": 10, "cell": format!("Scratch.B{number}"), "formula": format!("Scratch.A{target}")}));
                }
            }
            for (at, e) in model.iter().enumerate() {
                let number = at + 1;
                let a = cell(&mut s, &format!("Scratch.A{number}"));
                assert_eq!(a["value"], json!(f64::from(100 + e.id)), "seed {seed} step {step}: the A of row {number}: {a}");
                let Some(target) = e.points_at else { continue };
                let b = cell(&mut s, &format!("Scratch.B{number}"));
                if e.gone {
                    assert_eq!(
                        (b["formula"].clone(), b["error"].clone()),
                        (json!("REF()"), json!("#REF!")),
                        "seed {seed} step {step}: row {number}: {b}"
                    );
                } else {
                    let at_now = number_of(&model, target).unwrap();
                    assert_eq!(b["formula"], format!("Scratch.A{at_now}"), "seed {seed} step {step}: row {number}: {b}");
                    assert_eq!(b["value"], json!(f64::from(100 + target)), "seed {seed} step {step}: row {number}: {b}");
                }
            }
        }
        assert_eq!(check_session(&s), Ok(()));
    }
}

/// The stops of a gradient past the tenth are ignored by whatever reads them (the index): a row can still be added, and the answer says it is ignored.
#[test]
fn a_gradient_takes_a_row_past_its_tenth_stop_and_the_answer_says_it_is_ignored() {
    let mut shape = ShapeBuilder::new(10);
    for k in 0..10_u32 {
        shape = shape.row_cell("FillGradient", None, RowKey::Ix(k), "GradientStopPosition", &format!("0.{k}"));
    }
    let mut s = one_shape(shape);
    let answer = add_row(&mut s, "FillGradientStops", json!({}));
    assert_eq!(answer["row"], 11, "{answer}");
    assert!(answer["note"].as_str().unwrap().contains("ignored"), "{answer}");
    let names: Vec<&str> = answer["cells"].as_array().unwrap().iter().map(|c| c["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["GradientStopColor", "GradientStopColorTrans", "GradientStopPosition"]);
    // With fewer than ten there is nothing to say.
    let mut s = one_shape(ShapeBuilder::new(10).row_cell("FillGradient", None, RowKey::Ix(0), "GradientStopPosition", "0"));
    assert!(add_row(&mut s, "Fill Gradient Stops", json!({})).get("note").is_none());
}

// ---- copy, then insert: the rows an instance gets from its master or its style (the owner's ruling of 2026-11-26) ----------------------------------

/// The rows the sheet of shape `id` on page 1 holds itself in a section: each one's `IX` and whether it is deleted (`Del`).
fn own_rows(s: &Session, id: u32, section: &str, ix: Option<u32>) -> Vec<(Option<u32>, bool)> {
    let d = doc(s);
    let shape = d.shape(&d.find_shape(Owner::Page(1), id).unwrap()).unwrap();
    shape.sheet.section(&SectionKey::new(section, ix)).map(|sec| sec.rows.iter().map(|r| (r.ix, r.del)).collect()).unwrap_or_default()
}

/// A master with a Scratch section of three rows (A is 0, 1 and 2) and two instances of it on page 1, shapes 10 and 11.
fn two_instances_of_a_scratch_master() -> Session {
    let mut master_shape = ShapeBuilder::new(5);
    for k in 0..3_u32 {
        master_shape = master_shape.row_cell("Scratch", None, RowKey::Ix(k), "A", &k.to_string());
    }
    let built = DocBuilder::new()
        .master(1, MasterBuilder::new().shape(master_shape))
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).master(1)).shape(ShapeBuilder::new(11).master(1)))
        .build();
    session_of(built)
}

/// The number the cell `name` of shape `id` reads as.
fn number_of(s: &mut Session, id: u32, name: &str) -> f64 {
    run(s, "sheet.cell", json!({"id": id, "cell": name}))["value"].as_f64().unwrap_or_else(|| panic!("{name} of {id} holds no number"))
}

/// A middle insert into a section an instance gets from its master is allowed: the instance first takes its own copy of the master's rows, then the insert goes as it does into
/// the sheet's own section. One undo step, no deleted row left behind; a later edit of the master's rows no longer reaches the instance, and still reaches the other instances.
#[test]
fn an_instance_takes_a_copy_of_its_masters_rows_and_the_insert_goes_in_the_middle() {
    let mut s = two_instances_of_a_scratch_master();
    let before = state(&s);
    let answer = add_row(&mut s, "Scratch", json!({"after": 1}));
    assert_eq!(answer["row"], 2, "{answer}");
    assert_eq!(answer["renumbered"]["rows"], 2, "rows 2 and 3 moved up one: {answer}");
    assert_eq!(answer["renumbered"]["copied"], json!({"rows": 3, "cells": 3}), "the master's three rows became the instance's own: {answer}");
    assert_eq!(undo_len(&s), before.1 + 1, "one undo step");
    assert_eq!(keys(&mut s, "Scratch"), [json!(1), json!(2), json!(3), json!(4)]);
    let values: Vec<f64> = (1..=4).map(|n| number_of(&mut s, 10, &format!("Scratch.A{n}"))).collect();
    assert_eq!(values, [0.0, 0.0, 1.0, 2.0], "the new row holds the default; the rows after it are the master's, moved");
    assert_eq!(
        own_rows(&s, 10, "Scratch", None),
        [(Some(0), false), (Some(1), false), (Some(2), false), (Some(3), false)],
        "the instance holds all four, and none is a Del row"
    );
    assert!(own_rows(&s, 11, "Scratch", None).is_empty(), "the other instance holds nothing of its own");
    assert_eq!(doc(&s).master(1).unwrap().shapes.len(), 1);
    // A later edit of the master's rows reaches the instance that has no copy, and no longer the one that has.
    run(&mut s, "sheet.set_cell", json!({"master": 1, "id": 5, "cell": "Scratch.A3", "formula": "99"}));
    assert_eq!(number_of(&mut s, 11, "Scratch.A3"), 99.0);
    assert_eq!(number_of(&mut s, 10, "Scratch.A3"), 1.0, "the instance's own copy");
    assert_eq!(number_of(&mut s, 10, "Scratch.A4"), 2.0);
    assert_eq!(check_session(&s), Ok(()));
    // Undo takes the copy and the insert back together, and redo does both again.
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    assert!(own_rows(&s, 10, "Scratch", None).is_empty(), "undo leaves no copy behind");
    assert_eq!(doc(&s), before.0.as_ref().unwrap());
    run(&mut s, "edit.redo", json!({}));
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(keys(&mut s, "Scratch").len(), 4);
    assert_eq!(number_of(&mut s, 10, "Scratch.A4"), 2.0);
}

/// An add at the end needs no copy: the instance gets the one row and keeps inheriting the others (what it did before the ruling).
#[test]
fn an_add_at_the_end_of_an_inherited_section_takes_no_copy() {
    let mut s = two_instances_of_a_scratch_master();
    let end = add_row(&mut s, "Scratch", json!({}));
    assert_eq!(end["row"], 4);
    assert_eq!(end["renumbered"]["copied"], json!({"rows": 0, "cells": 0}), "{end}");
    assert_eq!(own_rows(&s, 10, "Scratch", None), [(Some(3), false)]);
    // The master's rows still reach it.
    run(&mut s, "sheet.set_cell", json!({"master": 1, "id": 5, "cell": "Scratch.A2", "formula": "77"}));
    assert_eq!(number_of(&mut s, 10, "Scratch.A2"), 77.0);
}

/// A dropped Process shape's own Geometry is its master's, and takes an insert in the middle now: the path is what it was with the one new point in it, the rows after it are
/// numbered again, and the copy follows the shape's size as the master's rows did.
#[test]
fn a_dropped_process_shapes_geometry_takes_an_insert_in_the_middle_and_goes_on_following_its_size() {
    let mut s = blank_session();
    let id = drop_shape(&mut s, "process", 5.0, 4.0);
    let control = drop_shape(&mut s, "process", 9.0, 4.0);
    let was = geometry(doc(&s), id);
    assert!(was.len() >= 4, "{was:?}");
    let answer = run(&mut s, "sheet.add_row", json!({"id": id, "section": "Geometry1", "after": 2}));
    assert_eq!((answer["row"].clone(), answer["type"].clone()), (json!(3), json!("LineTo")), "{answer}");
    assert_eq!(answer["renumbered"]["copied"]["rows"], was.len(), "{answer}");
    let mut want = was.clone();
    want.insert(2, ("LineTo".to_owned(), was[1].1, was[1].2));
    assert_eq!(geometry(doc(&s), id), want, "the path with the new point (the row before it's) in it");
    assert_eq!(geometry(doc(&s), control), was, "the other shape is as it was");
    // The copy follows the size: both shapes sized alike draw alike, but for the new point.
    for shape in [id, control] {
        run(&mut s, "shape.resize", json!({"id": shape, "x": 3.0, "y": 3.0, "width": 3.0, "height": 1.2}));
    }
    let resized = geometry(doc(&s), control);
    // The new row holds the values it was given, where the rows copied from the master read the size.
    let mut want = resized.clone();
    want.insert(2, ("LineTo".to_owned(), was[1].1, was[1].2));
    assert_eq!(geometry(doc(&s), id), want, "the copied rows read the shape's size");
    assert_eq!(check_session(&s), Ok(()));
}

/// Connection points a dropped shape gets from its master take an insert in the middle, and the connectors glued to them stay where they are: their formulas and `Connect` rows
/// name the same points by their new numbers. One undo step puts back the connector, the rows and the inheriting.
#[test]
fn an_insert_among_inherited_connection_points_keeps_every_glued_connector_where_it_is() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "process", 2.0, 2.0);
    let b = drop_shape(&mut s, "process", 6.0, 4.0);
    let joined = run(&mut s, "connector.add", json!({"from": {"id": a, "point": 2}, "to": {"id": b, "point": 0}}));
    let connector = id_of(&joined);
    let begin = |s: &mut Session| run(s, "sheet.cell", json!({"id": connector, "cell": "BeginX"}));
    let at = begin(&mut s)["value"].clone();
    assert_eq!(begin(&mut s)["formula"], format!("PAR(PNT(Sheet.{a}!Connections.X3,Sheet.{a}!Connections.Y3))"));
    let connects = |s: &Session| -> Vec<(u32, String, u32, String, u32)> {
        doc(s)
            .page(1)
            .unwrap()
            .connects
            .iter()
            .map(|c| {
                (
                    c.from_sheet,
                    c.from_cell.as_deref().unwrap_or("").to_owned(),
                    c.to_sheet,
                    c.to_cell.as_deref().unwrap_or("").to_owned(),
                    c.to_part.unwrap_or(0),
                )
            })
            .collect()
    };
    let before = state(&s);
    let answer = run(&mut s, "sheet.add_row", json!({"id": a, "section": "Connections", "after": 1}));
    assert_eq!(answer["row"], 2, "{answer}");
    assert_eq!(answer["renumbered"]["rows"], 3, "points 2 to 4 moved up one: {answer}");
    assert_eq!(answer["renumbered"]["copied"]["rows"], 4, "{answer}");
    assert_eq!(answer["renumbered"]["connects"], 1, "{answer}");
    assert_eq!(undo_len(&s), before.1 + 1, "one undo step");
    assert_eq!(begin(&mut s)["formula"], format!("PAR(PNT(Sheet.{a}!Connections.X4,Sheet.{a}!Connections.Y4))"));
    assert_eq!(begin(&mut s)["value"], at, "the connector did not move");
    assert!(connects(&s).contains(&(connector, "BeginX".to_owned(), a, "Connections.X4".to_owned(), 103)), "{:?}", connects(&s));
    assert!(connects(&s).iter().any(|c| c.2 == b && c.4 == 100), "the glue to the other shape is as it was");
    let read = run(&mut s, "sheet.read", json!({"id": a, "sections": ["Connections"], "count": 100}));
    assert_eq!(read["sections"][0]["rows"], 5);
    assert!(own_rows(&s, b, "Connection", None).is_empty(), "the other shape inherits its points still");
    assert_eq!(check_session(&s), Ok(()));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(doc(&s), before.0.as_ref().unwrap(), "the connector, the rows and the inheriting are as they were");
}

/// The stops of a gradient a shape gets from its style take an insert in the middle the same way, and the style is left as it is.
#[test]
fn an_insert_among_the_stops_a_shape_gets_from_its_style_takes_a_copy_of_them() {
    let mut style = StyleBuilder::new().name("Fade");
    for k in 0..3_u32 {
        style = style.row_cell("FillGradient", None, RowKey::Ix(k), "GradientStopPosition", &format!("{}", f64::from(k) / 2.0));
    }
    let built = DocBuilder::new().style(3, style).page(1, PageBuilder::new().shape(ShapeBuilder::new(10).links(None, Some(3), None))).build();
    let mut s = session_of(built);
    assert_eq!(keys(&mut s, "FillGradientStops").len(), 3, "the shape reads its style's stops");
    let answer = add_row(&mut s, "FillGradientStops", json!({"after": 1}));
    assert_eq!((answer["row"].clone(), answer["renumbered"]["copied"]["rows"].clone()), (json!(2), json!(3)), "{answer}");
    assert_eq!(keys(&mut s, "FillGradientStops").len(), 4);
    let position = |s: &mut Session, n: u32| number_of(s, 10, &format!("FillGradientStops.GradientStopPosition[{n}]"));
    assert_eq!(
        [position(&mut s, 1), position(&mut s, 3), position(&mut s, 4)],
        [0.0, 0.5, 1.0],
        "the stops after the new one are the style's, moved"
    );
    assert_eq!(doc(&s).style(3).unwrap().sheet.sections.iter().map(|sec| sec.rows.len()).sum::<usize>(), 3, "the style is as it was");
    // A later edit of the style's stop no longer reaches the shape.
    run(&mut s, "sheet.set_cell", json!({"style": 3, "cell": "FillGradientStops.GradientStopPosition[3]", "formula": "0.9"}));
    assert_eq!(position(&mut s, 4), 1.0);
    assert_eq!(check_session(&s), Ok(()));
}

/// A second insert in the middle of a section the instance already holds a copy of goes as the first did: the copy is taken again as it stands (so nothing changes by it) and the
/// master's cells are still kept out of the rows that moved.
#[test]
fn a_second_insert_into_an_instance_that_holds_its_copy_goes_the_same_way() {
    let mut s = two_instances_of_a_scratch_master();
    add_row(&mut s, "Scratch", json!({"after": 1}));
    let second = add_row(&mut s, "Scratch", json!({"before": 1}));
    assert_eq!(second["row"], 1, "{second}");
    assert_eq!(keys(&mut s, "Scratch").len(), 5);
    let values: Vec<f64> = (1..=5).map(|n| number_of(&mut s, 10, &format!("Scratch.A{n}"))).collect();
    assert_eq!(values, [0.0, 0.0, 0.0, 1.0, 2.0]);
    assert_eq!(own_rows(&s, 10, "Scratch", None).len(), 5);
    assert_eq!(check_session(&s), Ok(()));
}

/// A formula in a row that was copied and that names a row follows it with the rest: the copy is the instance's own formula, found where it is written.
#[test]
fn a_formula_in_a_copied_row_that_names_a_row_follows_it() {
    let master_shape = ShapeBuilder::new(5)
        .row_cell("Scratch", None, RowKey::Ix(0), "A", "5")
        .row_formula("Scratch", None, RowKey::Ix(1), "A", "Scratch.A1*2", "10")
        .row_formula("Scratch", None, RowKey::Ix(2), "B", "Scratch.A2+1", "11");
    let built = DocBuilder::new()
        .master(1, MasterBuilder::new().shape(master_shape))
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).master(1)))
        .build();
    let mut s = session_of(built);
    let answer = add_row(&mut s, "Scratch", json!({"before": 1}));
    assert_eq!(answer["renumbered"]["formulas"], 2, "the two copied formulas: {answer}");
    assert_eq!(formula(&mut s, "Scratch.A3"), "Scratch.A2*2");
    assert_eq!(formula(&mut s, "Scratch.B4"), "Scratch.A3+1");
    assert_eq!((number_of(&mut s, 10, "Scratch.A3"), number_of(&mut s, 10, "Scratch.B4")), (10.0, 11.0));
    // The master's own formulas are as they were.
    let d = doc(&s);
    let master = &d.master(1).unwrap().shapes.get(0).unwrap().sheet;
    let row = master
        .section(&SectionKey::new("Scratch", None))
        .unwrap()
        .rows
        .iter()
        .find(|r| r.ix == Some(1))
        .unwrap()
        .cells
        .get("A")
        .unwrap()
        .formula
        .text()
        .map(str::to_owned);
    assert_eq!(row.as_deref(), Some("Scratch.A1*2"));
}

/// A master's own rows and a style's own rows are still numbered by the instances and the shapes that read them: an insert in the middle of those is refused, and the end is
/// the place.
#[test]
fn a_style_takes_an_insert_in_the_middle_of_its_own_stops_no_more_than_a_master_does_of_its_rows() {
    let mut style = StyleBuilder::new().name("Fade");
    for k in 0..3_u32 {
        style = style.row_cell("FillGradient", None, RowKey::Ix(k), "GradientStopPosition", "0");
    }
    let mut s = session_of(DocBuilder::new().style(3, style).page(1, PageBuilder::new().shape(ShapeBuilder::new(10))).build());
    let anchor = state(&s);
    let e = refused(&mut s, "sheet.add_row", json!({"style": 3, "section": "FillGradientStops", "after": 1}));
    assert!(e.contains("numbered by the shapes that use this style: add the row at the end"), "{e}");
    assert_eq!(state(&s), anchor);
    assert_eq!(run(&mut s, "sheet.add_row", json!({"style": 3, "section": "FillGradientStops"}))["row"], 4);
}

/// The copy holds every cell the master's row at a number holds, so that none of the master's cells shows through a row that moved onto its number: a row that was a line and
/// is now where an arc's row is has the arc's `A` blocked (a cell of its own), and the arc, moved, keeps its bow.
#[test]
fn no_cell_of_the_masters_shows_through_a_row_that_moved_onto_the_number_of_another() {
    let mut master_shape = ShapeBuilder::new(5);
    for (ix, ty, bow) in [(1_u32, "MoveTo", None), (2, "LineTo", None), (3, "ArcTo", Some("0.5"))] {
        master_shape = master_shape
            .row_type("Geometry", Some(0), RowKey::Ix(ix), ty)
            .row_cell("Geometry", Some(0), RowKey::Ix(ix), "X", &ix.to_string())
            .row_cell("Geometry", Some(0), RowKey::Ix(ix), "Y", "0");
        if let Some(bow) = bow {
            master_shape = master_shape.row_cell("Geometry", Some(0), RowKey::Ix(ix), "A", bow);
        }
    }
    let built = DocBuilder::new()
        .master(1, MasterBuilder::new().shape(master_shape))
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).master(1)))
        .build();
    let mut s = session_of(built);
    let answer = add_row(&mut s, "Geometry1", json!({"after": 1}));
    assert_eq!(answer["renumbered"]["copied"]["rows"], 3, "{answer}");
    assert_eq!(
        rows(&mut s, "Geometry1").iter().map(|(_, t)| t.as_str().unwrap().to_owned()).collect::<Vec<_>>(),
        ["MoveTo", "LineTo", "LineTo", "ArcTo"]
    );
    let line_on_the_arcs_number = cell(&mut s, "Geometry1.A3");
    assert_eq!(
        (line_on_the_arcs_number["origin"].clone(), line_on_the_arcs_number["value"].clone()),
        (json!("local"), json!(0.0)),
        "the master's bow does not show: {line_on_the_arcs_number}"
    );
    assert_eq!(num(&mut s, "Geometry1.A4"), 0.5, "the arc kept its bow");
    assert_eq!((num(&mut s, "Geometry1.X3"), num(&mut s, "Geometry1.X4")), (2.0, 3.0));
}

/// A row the instance dropped (a `Del` row) moves with the run it is in, so the number it hid is still hidden, and no row of the master shows through a gap.
#[test]
fn a_row_the_instance_dropped_stays_dropped_through_a_copy_and_an_insert() {
    let mut master_shape = ShapeBuilder::new(5);
    for k in 0..4_u32 {
        master_shape = master_shape.row_cell("Scratch", None, RowKey::Ix(k), "A", &k.to_string());
    }
    let built = DocBuilder::new()
        .master(1, MasterBuilder::new().shape(master_shape))
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).master(1)))
        .build();
    let mut s = session_of(built);
    delete_row(&mut s, "Scratch", json!(2));
    assert_eq!(keys(&mut s, "Scratch"), [json!(1), json!(3), json!(4)]);
    let answer = add_row(&mut s, "Scratch", json!({"after": 1}));
    assert_eq!(answer["row"], 2, "{answer}");
    assert_eq!(keys(&mut s, "Scratch"), [json!(1), json!(2), json!(4), json!(5)], "number 3 is still the dropped row's");
    assert_eq!(
        (
            number_of(&mut s, 10, "Scratch.A1"),
            number_of(&mut s, 10, "Scratch.A2"),
            number_of(&mut s, 10, "Scratch.A4"),
            number_of(&mut s, 10, "Scratch.A5")
        ),
        (0.0, 0.0, 2.0, 3.0)
    );
    assert_eq!(own_rows(&s, 10, "Scratch", None).iter().filter(|(_, del)| *del).count(), 1, "the one Del row it had, moved with its run");
    assert_eq!(check_session(&s), Ok(()));
}

/// A formula of the master's row that names a shape of the master (`Sheet.5!Width`) is read by the instance's own shape, and a copy of it would name another: the insert is
/// refused in words and changes nothing, while the end is still the place.
#[test]
fn a_master_row_that_names_a_shape_of_the_master_cannot_be_copied_and_the_end_is_the_place() {
    let master_shape = ShapeBuilder::new(5)
        .cell_u("Width", "2", "IN")
        .row_formula("Scratch", None, RowKey::Ix(0), "A", "Sheet.5!Width*2", "4")
        .row_cell("Scratch", None, RowKey::Ix(1), "A", "1");
    let built = DocBuilder::new()
        .master(1, MasterBuilder::new().shape(master_shape))
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).master(1)))
        .build();
    let mut s = session_of(built);
    let anchor = state(&s);
    let e = refused(&mut s, "sheet.add_row", json!({"id": 10, "section": "Scratch", "after": 1}));
    assert!(e.contains("names a shape of the master") && e.contains("Sheet.5") && e.contains("add the row at the end"), "{e}");
    assert_eq!(state(&s), anchor);
    assert_eq!(add_row(&mut s, "Scratch", json!({}))["row"], 3);
}

/// An instance of a master of 31,273 rows takes its copy in one pass over the rows: the answer says how many rows and cells were copied.
#[test]
fn an_instance_of_a_master_of_31273_rows_copies_them_once_for_an_insert_in_the_middle() {
    const ROWS: u32 = 31_273;
    let mut master_shape = ShapeBuilder::new(5);
    for k in 0..ROWS {
        master_shape = master_shape.row_cell("Scratch", None, RowKey::Ix(k), "A", &k.to_string());
    }
    let built = DocBuilder::new()
        .master(1, MasterBuilder::new().shape(master_shape))
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).master(1)))
        .build();
    let mut s = session_of(built);
    let answer = add_row(&mut s, "Scratch", json!({"after": 100}));
    assert_eq!(answer["renumbered"]["copied"], json!({"rows": ROWS, "cells": ROWS}), "{answer}");
    assert_eq!(answer["renumbered"]["rows"], ROWS - 100, "{answer}");
    let read = run(&mut s, "sheet.read", json!({"id": 10, "sections": ["Scratch"], "count": 1}));
    assert_eq!(read["sections"][0]["rows"], ROWS + 1, "{read}");
    assert_eq!(
        (number_of(&mut s, 10, "Scratch.A100"), number_of(&mut s, 10, "Scratch.A101"), number_of(&mut s, 10, "Scratch.A102")),
        (99.0, 0.0, 100.0)
    );
    assert_eq!(number_of(&mut s, 10, &format!("Scratch.A{}", ROWS + 1)), f64::from(ROWS - 1));
    assert_eq!(own_rows(&s, 10, "Scratch", None).len(), ROWS as usize + 1);
}

// ----: an Ellipse or an Infinite Line row is alone in its section ----------------------------------------------------------------------

/// A Geometry section that holds an Ellipse or an Infinite Line row holds that row alone (MS-VSDX forbids any other row in the section): a row of any type, before
/// or after it, is refused with the reason and the way out, and nothing changes. A new section made with the variant holds the one row.
#[test]
fn a_section_that_holds_an_ellipse_or_an_infinite_line_takes_no_other_row() {
    let mut s = one_shape(geometry_shape(2));
    let ellipse = run(&mut s, "sheet.add_section", json!({"id": 10, "section": "Geometry", "variant": "Ellipse"}));
    let line = run(&mut s, "sheet.add_section", json!({"id": 10, "section": "Geometry", "variant": "InfiniteLine"}));
    assert_eq!((ellipse["section"].clone(), line["section"].clone()), (json!("Geometry2"), json!("Geometry3")));
    let anchor = state(&s);
    for (section, kind) in [("Geometry2", "Ellipse"), ("Geometry3", "InfiniteLine")] {
        for params in [
            json!({"id": 10, "section": section}),
            json!({"id": 10, "section": section, "type": "MoveTo", "before": 1}),
            json!({"id": 10, "section": section, "type": "LineTo", "after": 1}),
            json!({"id": 10, "section": section, "type": kind}),
        ] {
            let e = refused(&mut s, "sheet.add_row", params.clone());
            assert!(
                e.contains(&format!("{section} holds an {kind} row"))
                    && e.contains("holds that row alone")
                    && e.contains("another Geometry section (sheet.add_section)"),
                "{params}: {e}"
            );
        }
    }
    assert_eq!(state(&s), anchor, "every refusal changes nothing");
    // The row itself can go, and a section it leaves empty takes a row again.
    delete_row(&mut s, "Geometry2", json!(1));
    let again = add_row(&mut s, "Geometry2", json!({"type": "MoveTo"}));
    assert_eq!(again["row"], 1);
}

/// An Ellipse or an Infinite Line row cannot join a section that holds other rows; it can be the row of an empty section, and of a new one.
#[test]
fn an_ellipse_or_an_infinite_line_row_is_added_only_to_an_empty_section() {
    let mut s = one_shape(geometry_shape(2));
    let anchor = state(&s);
    for ty in ["Ellipse", "InfiniteLine"] {
        let e = refused(&mut s, "sheet.add_row", json!({"id": 10, "section": "Geometry1", "type": ty}));
        assert!(e.contains("an Ellipse or an Infinite Line row can only be added to an empty section"), "{e}");
        assert!(e.contains("Geometry1 holds other rows") && e.contains("sheet.add_section"), "the words say why and what to do: {e}");
    }
    assert_eq!(state(&s), anchor);
    // A section whose rows are all deleted is empty.
    let fresh = run(&mut s, "sheet.add_section", json!({"id": 10, "section": "Geometry"}));
    let name = fresh["section"].as_str().unwrap().to_owned();
    delete_row(&mut s, &name, json!(2));
    delete_row(&mut s, &name, json!(1));
    let made = add_row(&mut s, &name, json!({"type": "Ellipse"}));
    assert_eq!((made["row"].clone(), made["type"].clone()), (json!(1), json!("Ellipse")), "{made}");
    let e = refused(&mut s, "sheet.add_row", json!({"id": 10, "section": name}));
    assert!(e.contains("holds that row alone"), "{e}");
    assert_eq!(check_session(&s), Ok(()));
}

/// An Ellipse a master gives an instance is alone in the instance's section too: no row is added to it.
#[test]
fn an_ellipse_an_instance_gets_from_its_master_takes_no_other_row() {
    let master_shape =
        ShapeBuilder::new(5).row_type("Geometry", Some(0), RowKey::Ix(1), "Ellipse").row_cell("Geometry", Some(0), RowKey::Ix(1), "X", "1");
    let built = DocBuilder::new()
        .master(1, MasterBuilder::new().shape(master_shape))
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).master(1)))
        .build();
    let mut s = session_of(built);
    let anchor = state(&s);
    let e = refused(&mut s, "sheet.add_row", json!({"id": 10, "section": "Geometry1", "type": "LineTo"}));
    assert!(e.contains("Geometry1 holds an Ellipse row") && e.contains("holds that row alone"), "{e}");
    assert_eq!(state(&s), anchor);
}

// ---- inherited rows: Del ----------------------------------------------------------------------------------------------------

/// A shape that drops a row its master gives writes the row as a local `Del` row, with its key kept: the numbers of the other rows are the master's and stay (`IX` is a key);
/// the row is gone from the effective rows, and undo brings it back. A second delete of the same row finds nothing.
#[test]
fn a_row_an_instance_drops_is_written_as_a_del_row_and_the_numbers_stay() {
    let mut master_shape = ShapeBuilder::new(5).cell_u("Width", "2", "IN");
    for k in 1..=3_u32 {
        master_shape = master_shape
            .row_type("Geometry", Some(0), RowKey::Ix(k), if k == 1 { "MoveTo" } else { "LineTo" })
            .row_cell("Geometry", Some(0), RowKey::Ix(k), "X", &k.to_string())
            .row_cell("Geometry", Some(0), RowKey::Ix(k), "Y", "0");
    }
    master_shape = master_shape.row_cell("User", None, RowKey::name("Cost"), "Value", "9").row_cell("User", None, RowKey::name("Size"), "Value", "4");
    let built = DocBuilder::new()
        .master(1, MasterBuilder::new().shape(master_shape))
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).master(1)))
        .build();
    let mut s = session_of(built);
    assert_eq!(keys(&mut s, "Geometry1"), [json!(1), json!(2), json!(3)]);
    let before = undo_len(&s);
    let answer = delete_row(&mut s, "Geometry1", json!(2));
    assert_eq!(answer["how"], "marked", "written as a deleted row: {answer}");
    assert_eq!(answer["renumbered"]["rows"], 0);
    assert_eq!(undo_len(&s), before + 1);
    assert_eq!(keys(&mut s, "Geometry1"), [json!(1), json!(3)], "the master's numbers stay");
    // The model holds a local `Del` row with its key and nothing else.
    let d = doc(&s);
    let local = d.shape(&d.find_shape(Owner::Page(1), 10).unwrap()).unwrap();
    let section = local.sheet.section(&SectionKey::new("Geometry", Some(0))).unwrap();
    let row = section.row(&RowKey::Ix(2)).unwrap();
    assert!(row.del && row.cells.is_empty(), "{row:?}");
    assert_eq!(section.rows.len(), 1);
    // The master is as it was.
    assert_eq!(d.master(1).unwrap().shapes.len(), 1);
    let e = refused(&mut s, "sheet.delete_row", json!({"id": 10, "section": "Geometry1", "row": 2}));
    assert!(e.contains("the section has no row 2"), "{e}");
    // A named row of the master goes the same way.
    let named = delete_row(&mut s, "User", json!("Cost"));
    assert_eq!(named["how"], "marked");
    assert_eq!(keys(&mut s, "User"), [json!("Size")]);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(keys(&mut s, "User"), [json!("Cost"), json!("Size")]);
    assert_eq!(check_session(&s), Ok(()));
}

/// A row the shape added itself, beside the master's, is taken out; a row it holds over the master's (an override) is marked deleted, so that the master's does not show through.
#[test]
fn a_row_the_shape_added_is_taken_out_and_one_it_overrides_is_marked_deleted() {
    let master_shape = ShapeBuilder::new(5).row_cell("User", None, RowKey::name("Cost"), "Value", "9");
    let page_shape = ShapeBuilder::new(10).master(1).row_cell("User", None, RowKey::name("Cost"), "Value", "12").row_cell(
        "User",
        None,
        RowKey::name("Mine"),
        "Value",
        "1",
    );
    let built = DocBuilder::new().master(1, MasterBuilder::new().shape(master_shape)).page(1, PageBuilder::new().shape(page_shape)).build();
    let mut s = session_of(built);
    let mine = delete_row(&mut s, "User", json!("Mine"));
    assert_eq!(mine["how"], "removed");
    let cost = delete_row(&mut s, "User", json!("Cost"));
    assert_eq!(cost["how"], "marked");
    assert!(keys(&mut s, "User").is_empty(), "no row shows");
    let d = doc(&s);
    let local = d.shape(&d.find_shape(Owner::Page(1), 10).unwrap()).unwrap();
    let section = local.sheet.section(&SectionKey::new("User", None)).unwrap();
    assert_eq!(section.rows.len(), 1);
    assert!(section.rows[0].del && section.rows[0].cells.is_empty());
}

/// An instance adds a row at the end of what its master gives with no copy, and the number of a row it dropped stays taken: the next row is not given it again. (The insert in
/// the middle is the next test's: since the owner's ruling of 2026-11-26 it takes a copy.)
#[test]
fn an_instance_adds_a_row_at_the_end_of_its_masters_and_the_number_of_a_dropped_row_stays_taken() {
    let mut master_shape = ShapeBuilder::new(5);
    for k in 0..3_u32 {
        master_shape = master_shape.row_cell("Scratch", None, RowKey::Ix(k), "A", &k.to_string());
    }
    let built = DocBuilder::new()
        .master(1, MasterBuilder::new().shape(master_shape))
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).master(1)))
        .build();
    let mut s = session_of(built);
    let end = add_row(&mut s, "Scratch", json!({}));
    assert_eq!(end["row"], 4);
    assert_eq!(keys(&mut s, "Scratch"), [json!(1), json!(2), json!(3), json!(4)]);
    // After a row dropped the number stays taken: the next row is not given it again.
    delete_row(&mut s, "Scratch", json!(2));
    let next = add_row(&mut s, "Scratch", json!({}));
    assert_eq!(next["row"], 5);
    assert_eq!(keys(&mut s, "Scratch"), [json!(1), json!(3), json!(4), json!(5)]);
}

/// A master's own shape: a row at the end is added and its instances inherit it on two pages; a middle insert is refused (its instances hold rows by number); a delete takes
/// the row out and keeps the others' numbers.
#[test]
fn a_masters_row_is_added_at_the_end_and_an_insert_in_the_middle_is_refused() {
    let master_shape = ShapeBuilder::new(5).row_cell("Scratch", None, RowKey::Ix(0), "A", "10").row_cell("Scratch", None, RowKey::Ix(1), "A", "20");
    let built = DocBuilder::new()
        .master(1, MasterBuilder::new().shape(master_shape))
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).master(1)))
        .page(2, PageBuilder::new().shape(ShapeBuilder::new(10).master(1)))
        .build();
    let mut s = session_of(built);
    let e = refused(&mut s, "sheet.add_row", json!({"master": 1, "id": 5, "section": "Scratch", "after": 1}));
    assert!(e.contains("instances") && e.contains("add the row at the end"), "{e}");
    let end = run(&mut s, "sheet.add_row", json!({"master": 1, "id": 5, "section": "Scratch"}));
    assert_eq!((end["row"].clone(), end["sheet"].clone()), (json!(3), json!({"kind": "shape", "master": 1, "id": 5})));
    for page in [1, 2] {
        let read = run(&mut s, "sheet.read", json!({"id": 10, "page": page, "sections": ["Scratch"]}));
        let rows: Vec<&Value> = read["lines"].as_array().unwrap().iter().filter(|l| l["kind"] == "row").collect();
        assert_eq!(rows.len(), 3, "page {page}");
        assert_eq!(rows[2]["row"]["origin"], "inherited");
    }
    let gone = run(&mut s, "sheet.delete_row", json!({"master": 1, "id": 5, "section": "Scratch", "row": 1}));
    assert_eq!(gone["how"], "removed");
    assert_eq!(gone["renumbered"]["rows"], 0, "a master's rows keep their numbers: its instances hold rows by number");
    let read = run(&mut s, "sheet.read", json!({"id": 10, "page": 1, "sections": ["Scratch"]}));
    let numbers: Vec<Value> = read["lines"].as_array().unwrap().iter().filter(|l| l["kind"] == "row").map(|l| l["row"]["key"].clone()).collect();
    assert_eq!(numbers, [json!(2), json!(3)]);
    assert_eq!(check_session(&s), Ok(()));
}

/// A page's sheet, the document's and a style's take rows as a shape's do.
#[test]
fn a_page_the_document_and_a_style_take_rows_in_the_sections_that_allow_them() {
    let built = DocBuilder::new().style(3, StyleBuilder::new().name("Plain")).page(1, PageBuilder::new().shape(ShapeBuilder::new(10))).build();
    let mut s = session_of(built);
    for (sheet, kind) in [(json!({"page": 1}), "page"), (json!({"document": true}), "document"), (json!({"style": 3}), "style")] {
        let mut p = sheet.as_object().unwrap().clone();
        p.insert("section".into(), json!("User"));
        let section = run(&mut s, "sheet.add_section", Value::Object(p.clone()));
        assert_eq!(section["sheet"]["kind"], kind, "{section}");
        let row = run(&mut s, "sheet.add_row", Value::Object(p));
        assert_eq!(row["row"], "Row_2", "{kind}");
    }
    // Geometry, Connection Points and Controls are a shape's only.
    let e = refused(&mut s, "sheet.add_section", json!({"page": 1, "section": "Geometry"}));
    assert!(e.contains("a page has no Geometry sections: only a shape has"), "{e}");
    let e = refused(&mut s, "sheet.add_section", json!({"document": true, "section": "Connections"}));
    assert!(e.contains("the document has no Connection Points sections: only a shape has"), "{e}");
    let e = refused(&mut s, "sheet.add_section", json!({"style": 3, "section": "Controls"}));
    assert!(e.contains("a style has no Controls sections: only a shape has"), "{e}");
    assert_eq!(check_session(&s), Ok(()));
}

// ---- sections ------------------------------------------------------------------------------------------------------------------------------

/// V4 of the window's spec: Insert Section on a rectangle that has User-defined Cells offers Geometry, Ellipse and Infinite Line and greys User-defined Cells; Geometry
/// makes a new `Geometry2` with two rows, a MoveTo then a LineTo; Ellipse and Infinite Line one section each holding one row.
#[test]
fn a_geometry_section_added_holds_a_move_and_a_line_and_an_ellipse_or_an_infinite_line_one_row() {
    let mut s = one_shape(geometry_shape(3).row_cell("User", None, RowKey::name("Rate"), "Value", "5"));
    let offered = run(&mut s, "sheet.sections", json!({"id": 10}));
    let labels: Vec<&str> = offered["can_add"].as_array().unwrap().iter().map(|o| o["label"].as_str().unwrap()).collect();
    assert!(labels.contains(&"Geometry") && labels.contains(&"Ellipse") && labels.contains(&"Infinite Line"), "{labels:?}");
    assert!(!labels.contains(&"User-defined Cells"), "greyed: the shape has it");
    let e = refused(&mut s, "sheet.add_section", json!({"id": 10, "section": "User"}));
    assert!(e.contains("the shape already has User-defined Cells: a section other than Geometry is there once"), "{e}");

    let geometry = run(&mut s, "sheet.add_section", json!({"id": 10, "section": "Geometry"}));
    assert_eq!(
        (geometry["section"].clone(), geometry["label"].clone(), geometry["ix"].clone()),
        (json!("Geometry2"), json!("Geometry 2"), json!(1)),
        "{geometry}"
    );
    assert_eq!(geometry["rows"], json!([1, 2]));
    assert_eq!(rows(&mut s, "Geometry2").iter().map(|(_, t)| t.as_str().unwrap().to_owned()).collect::<Vec<_>>(), ["MoveTo", "LineTo"]);
    assert_eq!(s.history().unwrap().next_undo_label(), Some("Insert section"));

    let ellipse = run(&mut s, "sheet.add_section", json!({"id": 10, "section": "Geometry", "variant": "Ellipse"}));
    assert_eq!(ellipse["section"], "Geometry3");
    assert_eq!(rows(&mut s, "Geometry3").iter().map(|(_, t)| t.as_str().unwrap().to_owned()).collect::<Vec<_>>(), ["Ellipse"]);
    let line = run(&mut s, "sheet.add_section", json!({"id": 10, "section": "Geometry", "variant": "InfiniteLine"}));
    assert_eq!(line["section"], "Geometry4");
    assert_eq!(rows(&mut s, "Geometry4").iter().map(|(_, t)| t.as_str().unwrap().to_owned()).collect::<Vec<_>>(), ["InfiniteLine"]);
    let e = refused(&mut s, "sheet.add_section", json!({"id": 10, "section": "Geometry", "variant": "Spiral"}));
    assert!(e.contains("Spiral is no kind of Geometry section: Ellipse or InfiniteLine"), "{e}");
    let e = refused(&mut s, "sheet.add_section", json!({"id": 10, "section": "User", "variant": "Ellipse"}));
    assert!(e.contains("only a Geometry section has a variant"), "{e}");
    assert_eq!(check_session(&s), Ok(()));
}

/// A section added holds one row made as makes it; a section `sheet.sections` does not offer is refused in words (Shape Transform; the groups that no command
/// inserts; Layers, Reviewer and Annotation, which belong to the page and the comments).
#[test]
fn a_section_is_added_with_one_row_and_one_that_is_not_offered_is_refused() {
    let mut s = one_shape(ShapeBuilder::new(10));
    for (name, rows_first) in [("Prop", "Row_1"), ("Hyperlink", "Row_1"), ("Actions", "Row_1"), ("Controls", "Row_1"), ("SmartTags", "Row_1")] {
        let added = run(&mut s, "sheet.add_section", json!({"id": 10, "section": name}));
        assert_eq!(added["section"], name, "{added}");
        assert_eq!(added["rows"], json!([rows_first]), "{name}: {added}");
    }
    let scratch = run(&mut s, "sheet.add_section", json!({"id": 10, "section": "Scratch"}));
    assert_eq!(scratch["rows"], json!([1]));
    assert_eq!(num(&mut s, "Scratch.A1"), 0.0);
    let points = run(&mut s, "sheet.add_section", json!({"id": 10, "section": "Connection Points"}));
    assert_eq!((points["section"].clone(), points["rows"].clone()), (json!("Connections"), json!([1])), "{points}");
    let anchor = state(&s);
    for (section, words) in [
        ("Shape Transform", "Shape Transform cannot be inserted or deleted"),
        ("Protection", "Protection cannot be inserted or deleted"),
        ("Layers", "Layers cannot be inserted or deleted here: layer.add and layer.delete make them"),
        ("Reviewer", "Reviewer cannot be inserted or deleted here: the comments make them"),
        ("Annotation", "Annotation cannot be inserted or deleted here: the comments make them"),
        ("Char", "Character cannot be inserted or deleted here: the text makes them"),
        ("FillGradientStops", "Fill Gradient Stops belongs to the fill: shape.gradient makes it"),
        ("Nonsense", "no section called Nonsense"),
    ] {
        let e = refused(&mut s, "sheet.add_section", json!({"id": 10, "section": section}));
        assert!(e.contains(words), "{section}: {e}");
    }
    assert_eq!(state(&s), anchor);
    let offered = run(&mut s, "sheet.sections", json!({"id": 10}));
    let labels: Vec<&str> = offered["can_add"].as_array().unwrap().iter().map(|o| o["label"].as_str().unwrap()).collect();
    assert!(!labels.contains(&"Shape Data") && !labels.contains(&"Scratch"), "what was added is not offered again: {labels:?}");
}

/// Insert Section on a shape: Text Transform writes the seven cells with the formulas of and `TxtAngle` 0 and Layer Membership an empty `LayerMember`; a page
/// has neither. Delete Section takes the seven cells out and the block reads those formulas as its defaults again.
#[test]
fn text_transform_is_inserted_with_the_formulas_of_the_block_and_layer_membership_with_an_empty_cell() {
    let mut s = one_shape(ShapeBuilder::new(10).cell_u("Width", "4", "IN").cell_u("Height", "2", "IN"));
    let tt = run(&mut s, "sheet.add_section", json!({"id": 10, "section": "Text Transform"}));
    assert_eq!(tt["section"], "Text Transform", "{tt}");
    for (name, text) in [
        ("TxtWidth", "Width*1"),
        ("TxtHeight", "Height*1"),
        ("TxtPinX", "Width*0.5"),
        ("TxtPinY", "Height*0.5"),
        ("TxtLocPinX", "TxtWidth*0.5"),
        ("TxtLocPinY", "TxtHeight*0.5"),
    ] {
        let c = cell(&mut s, name);
        assert_eq!((c["formula"].as_str(), c["origin"].as_str()), (Some(text), Some("local")), "{name}: {c}");
    }
    assert_eq!(num(&mut s, "TxtWidth"), 4.0);
    assert_eq!(num(&mut s, "TxtPinX"), 2.0);
    assert_eq!(num(&mut s, "TxtLocPinX"), 2.0);
    assert_eq!(cell(&mut s, "TxtAngle")["origin"], "local");
    let lm = run(&mut s, "sheet.add_section", json!({"id": 10, "section": "Layer Membership"}));
    assert_eq!(lm["section"], "Layer Membership");
    let member = cell(&mut s, "LayerMember");
    assert_eq!((member["origin"].clone(), member["shown"].clone()), (json!("local"), json!("")), "{member}");
    // Neither is offered again, nor is either a page's.
    let e = refused(&mut s, "sheet.add_section", json!({"id": 10, "section": "Text Transform"}));
    assert!(e.contains("the shape already has Text Transform"), "{e}");
    let e = refused(&mut s, "sheet.add_section", json!({"page": 1, "section": "Layer Membership"}));
    assert!(e.contains("a page has no Layer Membership sections: only a shape has"), "{e}");
    // Delete Section: the seven go, and the default formulas read again.
    let gone = run(&mut s, "sheet.delete_section", json!({"id": 10, "section": "Text Transform"}));
    assert_eq!(gone["how"], "removed");
    let c = cell(&mut s, "TxtWidth");
    assert_eq!(c["origin"], "default", "{c}");
    assert_eq!(num(&mut s, "TxtWidth"), 4.0, "the block reads Width*1 as its default again");
    let gone = run(&mut s, "sheet.delete_section", json!({"id": 10, "section": "Layer Membership"}));
    assert_eq!(gone["how"], "removed");
    assert_eq!(cell(&mut s, "LayerMember")["origin"], "default");
    assert_eq!(check_session(&s), Ok(()));
}

/// V6 (Shape Transform cannot go) and the rule of what Delete Section offers: `sheet.sections` lists what can be deleted and a refusal says why.
#[test]
fn shape_transform_and_the_other_sections_that_cannot_go_are_refused() {
    let mut s = one_shape(scratch_shape(2).cell_u("Width", "2", "IN"));
    let anchor = state(&s);
    let offered = run(&mut s, "sheet.sections", json!({"id": 10}));
    let deletable: Vec<&str> = offered["can_delete"].as_array().unwrap().iter().map(|d| d["section"].as_str().unwrap()).collect();
    assert!(deletable.contains(&"Scratch") && !deletable.contains(&"Shape Transform"), "{deletable:?}");
    let e = refused(&mut s, "sheet.delete_section", json!({"id": 10, "section": "Shape Transform"}));
    assert!(e.contains("Shape Transform cannot be inserted or deleted: a shape always has one"), "{e}");
    let e = refused(&mut s, "sheet.delete_section", json!({"id": 10, "section": "Protection"}));
    assert!(e.contains("Protection cannot be inserted or deleted"), "{e}");
    let e = refused(&mut s, "sheet.delete_section", json!({"id": 10, "section": "Prop"}));
    assert!(e.contains("the sheet has no section Prop"), "{e}");
    let e = refused(&mut s, "sheet.delete_section", json!({"id": 10}));
    assert!(e.contains("missing `section`"), "{e}");
    assert_eq!(state(&s), anchor);
}

/// A section the shape holds itself is taken out with its rows, and the formulas that read its rows read `#REF!`.
#[test]
fn a_section_the_shape_holds_is_taken_out_and_what_read_its_rows_reads_ref() {
    let shape = scratch_shape(2).row_cell("User", None, RowKey::name("Rate"), "Value", "5").formula("Height", "Scratch.A2*2", "40").formula(
        "Angle",
        "User.Rate.Value+1",
        "6",
    );
    let mut s = one_shape(shape);
    let gone = run(&mut s, "sheet.delete_section", json!({"id": 10, "section": "Scratch"}));
    assert_eq!((gone["section"].clone(), gone["how"].clone()), (json!("Scratch"), json!("removed")), "{gone}");
    assert_eq!(s.history().unwrap().next_undo_label(), Some("Delete section"));
    assert_eq!(formula(&mut s, "Height"), "REF()*2");
    assert_eq!(cell(&mut s, "Height")["error"], "#REF!");
    assert_eq!(formula(&mut s, "Angle"), "User.Rate.Value+1", "another section is not touched");
    let sections = run(&mut s, "sheet.read", json!({"id": 10}));
    assert!(!sections["sections"].as_array().unwrap().iter().any(|sec| sec["section"] == "Scratch"));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(formula(&mut s, "Height"), "Scratch.A2*2", "undo puts the section and the formula back");
}

/// V5 of the window's spec: Geometry1, Geometry2 and Geometry3, Geometry1 deleted: two sections named Geometry1 and Geometry2, the second holding the rows that were
/// Geometry3's; the formulas that name the later sections follow (`Geometry3.X2` is `Geometry2.X2`) and one that named the deleted section reads `#REF!`.
#[test]
fn deleting_a_geometry_section_numbers_the_later_ones_again_and_the_formulas_follow() {
    let mut shape = ShapeBuilder::new(10);
    for (ix, base) in [(0_u32, 100.0_f64), (1, 200.0), (2, 300.0)] {
        for k in 1..=2_u32 {
            shape = shape
                .row_type("Geometry", Some(ix), RowKey::Ix(k), if k == 1 { "MoveTo" } else { "LineTo" })
                .row_cell("Geometry", Some(ix), RowKey::Ix(k), "X", &(base + f64::from(k)).to_string())
                .row_cell("Geometry", Some(ix), RowKey::Ix(k), "Y", "0");
        }
    }
    let shape = shape
        .row_formula("Scratch", None, RowKey::Ix(0), "A", "Geometry3.X2", "302")
        .row_formula("Scratch", None, RowKey::Ix(0), "B", "Geometry2.X1", "201")
        .row_formula("Scratch", None, RowKey::Ix(0), "C", "Geometry1.X1", "101")
        .row_formula("Scratch", None, RowKey::Ix(0), "D", "Geometry1.X2+Geometry3.X1", "401");
    let mut s = one_shape(shape);
    let gone = run(&mut s, "sheet.delete_section", json!({"id": 10, "section": "Geometry1"}));
    assert_eq!(gone["how"], "removed");
    assert_eq!(gone["renumbered"]["sections"], 2, "{gone}");
    let read = run(&mut s, "sheet.read", json!({"id": 10, "all": false}));
    let names: Vec<&str> =
        read["sections"].as_array().unwrap().iter().map(|sec| sec["section"].as_str().unwrap()).filter(|n| n.starts_with("Geometry")).collect();
    assert_eq!(names, ["Geometry1", "Geometry2"]);
    assert_eq!(num(&mut s, "Geometry1.X1"), 201.0);
    assert_eq!(num(&mut s, "Geometry2.X2"), 302.0, "the rows that were Geometry3's");
    assert_eq!(formula(&mut s, "Scratch.A1"), "Geometry2.X2");
    assert_eq!(formula(&mut s, "Scratch.B1"), "Geometry1.X1");
    assert_eq!(formula(&mut s, "Scratch.C1"), "REF()");
    assert_eq!(formula(&mut s, "Scratch.D1"), "REF()+Geometry2.X1");
    assert_eq!(check_session(&s), Ok(()));
}

/// A section the master gives is dropped by a local `Del` section, with its place kept; the instance reads none of it, and undo brings it back.
#[test]
fn a_section_the_master_gives_is_dropped_by_a_local_del_section() {
    let master_shape =
        ShapeBuilder::new(5).row_cell("Scratch", None, RowKey::Ix(0), "A", "10").row_cell("User", None, RowKey::name("Rate"), "Value", "5");
    let built = DocBuilder::new()
        .master(1, MasterBuilder::new().shape(master_shape))
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).master(1)))
        .build();
    let mut s = session_of(built);
    let gone = run(&mut s, "sheet.delete_section", json!({"id": 10, "section": "Scratch"}));
    assert_eq!(gone["how"], "marked", "{gone}");
    let d = doc(&s);
    let local = d.shape(&d.find_shape(Owner::Page(1), 10).unwrap()).unwrap();
    let section = local.sheet.section(&SectionKey::new("Scratch", None)).unwrap();
    assert!(section.del && section.rows.is_empty() && section.cells.is_empty());
    let read = run(&mut s, "sheet.read", json!({"id": 10}));
    assert!(!read["sections"].as_array().unwrap().iter().any(|sec| sec["section"] == "Scratch"));
    assert!(read["sections"].as_array().unwrap().iter().any(|sec| sec["section"] == "User"), "the other section stays");
    run(&mut s, "edit.undo", json!({}));
    let read = run(&mut s, "sheet.read", json!({"id": 10}));
    assert!(read["sections"].as_array().unwrap().iter().any(|sec| sec["section"] == "Scratch"));
    assert_eq!(check_session(&s), Ok(()));
}

// ---- the rules of every command: one step, all or nothing, locks, parameters -----------------------------------------------------------

/// A shape on a locked layer is refused by the registry's rule, in its words, and so is none of a master's.
#[test]
fn a_shape_on_a_locked_layer_takes_no_rows_or_sections() {
    let mut s = blank_session();
    let id = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    run(&mut s, "layer.add", json!({"name": "Walls"}));
    run(&mut s, "layer.assign", json!({"ids": [id], "layers": ["Walls"]}));
    run(&mut s, "layer.set", json!({"layer": "Walls", "lock": true}));
    let anchor = state(&s);
    for (cmd, params) in [
        ("sheet.add_row", json!({"id": id, "section": "Scratch"})),
        ("sheet.delete_row", json!({"id": id, "section": "Scratch", "row": 1})),
        ("sheet.add_section", json!({"id": id, "section": "Scratch"})),
        ("sheet.delete_section", json!({"id": id, "section": "Scratch"})),
    ] {
        let e = refused(&mut s, cmd, params);
        assert!(e.contains("Walls") && e.contains("locked"), "{cmd}: {e}");
    }
    assert_eq!(state(&s), anchor);
}

/// A refused call is no step, a call is one step, and an unknown key is refused by the registry before the command runs; the four commands are journaled.
#[test]
fn a_refusal_is_no_step_and_an_unknown_key_is_refused_by_the_registry() {
    let mut s = one_shape(scratch_shape(2));
    let anchor = state(&s);
    let e = s.execute("sheet.add_row", &json!({"id": 10, "section": "Scratch", "nme": "x"})).unwrap_err();
    assert!(matches!(e, EngineError::BadParams { .. }) && e.to_string().contains("unknown parameter `nme`"), "{e}");
    for (cmd, params) in [
        ("sheet.add_row", json!({"section": "Scratch"})),
        ("sheet.delete_row", json!({"id": 10, "section": "Scratch"})),
        ("sheet.delete_row", json!({"id": 10, "section": "Scratch", "row": 7})),
        ("sheet.add_section", json!({"id": 10, "style": 1, "section": "User"})),
        ("sheet.delete_section", json!({"id": 99, "section": "Scratch"})),
    ] {
        assert!(s.execute(cmd, &params).is_err(), "{cmd} {params}");
    }
    assert_eq!(state(&s), anchor);
    let e = refused(&mut s, "sheet.add_row", json!({"section": "Scratch"}));
    assert!(e.contains("give one sheet: id (with page), page, master (with id), style or document"), "{e}");
    let e = refused(&mut s, "sheet.delete_row", json!({"id": 10, "section": "Scratch"}));
    assert!(e.contains("missing `row`"), "{e}");
    let e = refused(&mut s, "sheet.delete_row", json!({"id": 10, "section": "Scratch", "row": 7}));
    assert!(e.contains("the section has no row 7"), "{e}");
    assert!(!s.journal().iter().any(|(id, _)| id.starts_with("sheet.add_row") || id.starts_with("sheet.delete_row")), "a refusal is not journaled");
    add_row(&mut s, "Scratch", json!({}));
    delete_row(&mut s, "Scratch", json!(1));
    let ids: Vec<&str> = s.journal().iter().map(|(id, _)| id.as_str()).collect();
    assert!(ids.contains(&"sheet.add_row") && ids.contains(&"sheet.delete_row"), "{ids:?}");
}

/// Insert Section's card adds the ticked sections as one undo step (`command.batch`).
#[test]
fn several_sections_added_in_a_batch_are_one_undo_step() {
    let mut s = one_shape(ShapeBuilder::new(10));
    let before = undo_len(&s);
    run(
        &mut s,
        "command.batch",
        json!({"commands": [
            {"command": "sheet.add_section", "params": {"id": 10, "section": "User"}},
            {"command": "sheet.add_section", "params": {"id": 10, "section": "Prop"}},
            {"command": "sheet.add_row", "params": {"id": 10, "section": "Prop"}},
        ]}),
    );
    assert_eq!(undo_len(&s), before + 1);
    assert_eq!(keys(&mut s, "Prop"), [json!("Row_1"), json!("Row_2")]);
    run(&mut s, "edit.undo", json!({}));
    assert!(keys_or_missing(&mut s, "Prop").is_none());
}

fn keys_or_missing(s: &mut Session, section: &str) -> Option<Vec<Value>> {
    let read = s.execute("sheet.read", &json!({"id": 10, "sections": [section], "count": 10})).ok()?;
    Some(read["lines"].as_array()?.iter().filter(|l| l["kind"] == "row").map(|l| l["row"]["key"].clone()).collect())
}

// ---- hostile inputs: counted, never timed -----------------------------------------------------------------------------------------------

/// A Geometry section of 31,273 rows (the most a real file held) takes a row in the middle: 31,173 rows move, and the work is the section's rows once, not a search of the sheet for
/// each row; the cells that name no row of the section cost a glance each.
#[test]
fn a_section_of_31273_rows_is_numbered_again_in_work_that_follows_its_rows() {
    const ROWS: u32 = 31_273;
    let mut shape = ShapeBuilder::new(10).formula("Width", "Geometry1.X31000+Geometry1.X101", "0");
    for k in 1..=ROWS {
        shape = shape
            .row_type("Geometry", Some(0), RowKey::Ix(k), if k == 1 { "MoveTo" } else { "RelLineTo" })
            .row_cell("Geometry", Some(0), RowKey::Ix(k), "X", &k.to_string())
            .row_cell("Geometry", Some(0), RowKey::Ix(k), "Y", "0");
    }
    let mut s = one_shape(shape);
    let answer = add_row(&mut s, "Geometry1", json!({"after": 100, "type": "RelLineTo"}));
    let r = &answer["renumbered"];
    assert_eq!((r["rows"].clone(), r["formulas"].clone()), (json!(ROWS - 100), json!(1)), "{answer}");
    let work = &r["work"];
    assert!(work["visited"].as_u64().unwrap() <= u64::from(3 * ROWS + 100), "every cell of the drawing looked at once at most: {work}");
    assert_eq!(work["parsed"], 1, "only the one formula that names the section is read: {work}");
    assert_eq!(
        formula(&mut s, "Width"),
        "Geometry1.X31001+Geometry1.X102",
        "rows 101 and on moved up one, and the reference to row 101 and the one to row 31000 followed"
    );
    assert_eq!(num(&mut s, "Geometry1.X31274"), f64::from(ROWS));
    assert_eq!(num(&mut s, "Geometry1.X101"), 100.0, "the new row holds the X of the row before it");
    assert_eq!(num(&mut s, "Geometry1.X102"), 101.0, "the row that was 101");
}

/// A renumber that touches 30,000 shapes: each reads a row of the shape that moves through `Sheet.1!`, and each formula is read once and written once. The answer counts the work.
#[test]
fn a_renumber_that_touches_30000_shapes_reads_and_writes_each_formula_once() {
    const READERS: u32 = 30_000;
    let mut page = PageBuilder::new().shape(scratch_shape(4).formula("Height", "1", "1"));
    // The shape that moves is shape 10; its readers are 11 and on.
    for k in 0..READERS {
        page = page.shape(ShapeBuilder::new(11 + k).formula("Width", "Sheet.10!Scratch.A3*1", "30").cell("Height", "1"));
    }
    let mut s = session_of(DocBuilder::new().page(1, page).build());
    let answer = add_row(&mut s, "Scratch", json!({"after": 1}));
    let r = &answer["renumbered"];
    assert_eq!(r["formulas"], READERS, "{r}");
    assert_eq!(r["work"]["parsed"], READERS, "{r}");
    assert_eq!(r["work"]["located"], READERS, "{r}");
    assert!(r["work"]["visited"].as_u64().unwrap() <= u64::from(2 * READERS + 100), "linear in the cells of the page: {r}");
    let d = doc(&s);
    for id in [11_u32, 15_000, 30_010] {
        let at = d.find_shape(Owner::Page(1), id).unwrap();
        assert_eq!(d.shape(&at).unwrap().sheet.cell("Width").unwrap().formula.text(), Some("Sheet.10!Scratch.A4*1"), "shape {id}");
    }
    assert_eq!(run(&mut s, "sheet.cell", json!({"id": 30_010, "cell": "Width"}))["value"], json!(30.0));
    assert_eq!(check_session(&s), Ok(()));
}

/// A chain of 10,000 cells that each read the row before: a row inserted at the top moves them all and each formula is rewritten once, so that the chain still reads the same.
#[test]
fn a_chain_of_10000_rows_that_each_read_the_one_before_is_followed() {
    const ROWS: u32 = 10_000;
    let mut shape = ShapeBuilder::new(10).row_cell("Scratch", None, RowKey::Ix(0), "A", "1");
    for k in 1..ROWS {
        shape = shape.row_formula("Scratch", None, RowKey::Ix(k), "A", &format!("Scratch.A{k}+1"), &(k + 1).to_string());
    }
    let mut s = one_shape(shape);
    let answer = add_row(&mut s, "Scratch", json!({"before": 1}));
    assert_eq!(answer["renumbered"]["rows"], ROWS);
    assert_eq!(answer["renumbered"]["formulas"], ROWS - 1);
    assert_eq!(answer["renumbered"]["work"]["parsed"], ROWS - 1);
    assert_eq!(formula(&mut s, "Scratch.A10001"), "Scratch.A10000+1");
    assert_eq!(num(&mut s, "Scratch.A10001"), f64::from(ROWS));
    assert_eq!(num(&mut s, "Scratch.A2"), 1.0);
    assert_eq!(num(&mut s, "Scratch.A1"), 0.0, "the new row");
    // And the delete that puts them back follows them again.
    delete_row(&mut s, "Scratch", json!(1));
    assert_eq!(formula(&mut s, "Scratch.A10000"), "Scratch.A9999+1");
    assert_eq!(num(&mut s, "Scratch.A10000"), f64::from(ROWS));
}

/// A formula of 65,000 bytes that names the moved row 6,500 times is read once and written once, and a string that holds the same words is not touched.
#[test]
fn a_formula_naming_a_row_thousands_of_times_is_rewritten_once() {
    let body = vec!["Scratch.A3"; 6_500].join(",");
    let text = format!("SUM({body},LEN(\"Scratch.A3 Scratch.A3\"))");
    let mut s = one_shape(scratch_shape(4).formula("Height", &text, "0"));
    let answer = add_row(&mut s, "Scratch", json!({"after": 1}));
    assert_eq!(answer["renumbered"]["formulas"], 1);
    assert_eq!(answer["renumbered"]["work"]["located"], 6_500, "{answer}");
    let now = formula(&mut s, "Height");
    assert_eq!(now.len(), text.len());
    assert_eq!(now, format!("SUM({},LEN(\"Scratch.A3 Scratch.A3\"))", vec!["Scratch.A4"; 6_500].join(",")));
}

/// A section of 320,000 rows (the most the model holds) takes no more, in words.
#[test]
fn a_section_at_the_most_rows_the_model_holds_takes_no_more() {
    const MOST: u32 = 320_000;
    let mut shape = ShapeBuilder::new(10);
    for k in 0..MOST {
        shape = shape.row_cell("Scratch", None, RowKey::Ix(k), "A", "0");
    }
    let mut s = one_shape(shape);
    let anchor = state(&s);
    let e = refused(&mut s, "sheet.add_row", json!({"id": 10, "section": "Scratch"}));
    assert!(e.contains("a section holds at most 320000 rows"), "{e}");
    assert_eq!(state(&s), anchor);
    // A row out of it makes room.
    run(&mut s, "sheet.delete_row", json!({"id": 10, "section": "Scratch", "row": MOST}));
    assert_eq!(add_row(&mut s, "Scratch", json!({}))["row"], MOST);
}

/// An instance of a master of 31,273 rows drops one, merging the section once; the answer's work is counted.
#[test]
fn an_instance_of_a_master_of_31273_rows_drops_one_in_one_merge() {
    const ROWS: u32 = 31_273;
    let mut master_shape = ShapeBuilder::new(5);
    for k in 0..ROWS {
        master_shape = master_shape.row_cell("Scratch", None, RowKey::Ix(k), "A", "1");
    }
    let built = DocBuilder::new()
        .master(1, MasterBuilder::new().shape(master_shape))
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).master(1)))
        .build();
    let mut s = session_of(built);
    let answer = delete_row(&mut s, "Scratch", json!(500));
    assert_eq!(answer["how"], "marked");
    let read = run(&mut s, "sheet.read", json!({"id": 10, "sections": ["Scratch"], "count": 1}));
    assert_eq!(read["sections"][0]["rows"], ROWS - 1, "{read}");
    assert_eq!(check_session(&s), Ok(()));
}

/// The real-time targets of the two biggest renumbers, run by hand on a quiet machine (a target, never a gate: no test asserts a time).
#[test]
#[ignore = "real time: run by hand"]
fn the_renumbers_finish_in_a_few_seconds() {
    let started = std::time::Instant::now();
    a_renumber_that_touches_30000_shapes_reads_and_writes_each_formula_once();
    a_section_of_31273_rows_is_numbered_again_in_work_that_follows_its_rows();
    assert!(started.elapsed().as_secs() < 30, "took {:?}", started.elapsed());
}

/// A drawing a command left must be one every reader takes: the check the engine's own tests make after an edit, held to the Resolver's view of a section.
#[test]
fn the_resolver_reads_the_rows_a_command_wrote() {
    let mut s = one_shape(geometry_shape(3));
    add_row(&mut s, "Geometry1", json!({"after": 3}));
    let d = doc(&s);
    let sheet = SheetRef::Shape(d.find_shape(Owner::Page(1), 10).unwrap());
    let rows = Resolver::new(d).effective_rows(&sheet, &SectionKey::new("Geometry", Some(0))).unwrap();
    assert_eq!(rows.iter().map(|r| r.key.clone()).collect::<Vec<_>>(), [RowKey::Ix(1), RowKey::Ix(2), RowKey::Ix(3), RowKey::Ix(4)]);
}

// ---- the sheets no page works out ------------------------------------------------------------------------------------------------

/// A formula on a sheet no page works out (the document's, a style's, a master's page sheet, the shape of a master no page holds an instance of) that names a row that went reads
/// `#REF!` as one on a page does, with the value it had, and the cell that reads it follows. Nothing worked these sheets out, so the error was never recorded: the property test of
/// found it by a full recalculation that disagreed with the drawing.
#[test]
fn a_formula_on_a_sheet_no_page_works_out_that_named_a_row_that_went_records_ref() {
    let master = MasterBuilder::new().shape(ShapeBuilder::new(5).cell_u("Width", "1", "IN"));
    let page = PageBuilder::new().name("Page-1").shape(ShapeBuilder::new(10).cell_u("Width", "2", "IN"));
    let mut s = session_of(DocBuilder::new().master(1, master).style(1, StyleBuilder::new().name("S1")).page(1, page).build());
    let on = |keys: &Value, extra: Value| {
        let mut params = keys.clone();
        params.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        params
    };
    for (label, keys) in [
        ("the document", json!({"document": true})),
        ("a style", json!({"style": 1})),
        ("a master's page", json!({"master": 1})),
        ("the shape of a master no page holds an instance of", json!({"master": 1, "id": 5})),
    ] {
        run(&mut s, "sheet.add_section", on(&keys, json!({"section": "Scratch"})));
        run(&mut s, "sheet.set_cell", on(&keys, json!({"cell": "Scratch.A1", "formula": "7"})));
        run(&mut s, "sheet.add_section", on(&keys, json!({"section": "User"})));
        run(&mut s, "sheet.add_row", on(&keys, json!({"section": "User", "name": "B"})));
        run(&mut s, "sheet.set_cell", on(&keys, json!({"cell": "User.Row_1.Value", "formula": "Scratch.A1"})));
        run(&mut s, "sheet.set_cell", on(&keys, json!({"cell": "User.B.Value", "formula": "User.Row_1.Value+1"})));
        let cell = |s: &mut Session, name: &str| run(s, "sheet.cell", on(&keys, json!({"cell": name})));
        assert_eq!(cell(&mut s, "User.B.Value")["value"], json!(8.0), "{label}");
        // The row it names goes: the formula reads REF() and the error is recorded, with the value it had; the cell that reads it keeps reading that value.
        run(&mut s, "sheet.delete_row", on(&keys, json!({"section": "Scratch", "row": 1})));
        let a = cell(&mut s, "User.Row_1.Value");
        assert_eq!((a["formula"].clone(), a["error"].clone(), a["value"].clone()), (json!("REF()"), json!("#REF!"), json!(7.0)), "{label}: {a}");
        let b = cell(&mut s, "User.B.Value");
        assert_eq!((b["error"].clone(), b["value"].clone()), (NULL, json!(8.0)), "{label}: {b}");
        let report = diacraft_calc::recalc_all(doc(&s), &diacraft_calc::Budget::default());
        assert!(report.differed.is_empty(), "{label}: a full recalculation differs from the drawing: {:?}", report.differed);
        assert_eq!(check_session(&s), Ok(()));
    }
}

// ---- rows whose names differ only in letter case ---------------------------------------------------------------------------

/// Shape 10 with Shape Data rows as a file from another program may hold them: `Cost`, then `first` and `second`, which differ only in letter case, holding 1 and 2 in `Value`.
fn twin_shape(first: &str, second: &str) -> ShapeBuilder {
    ShapeBuilder::new(10)
        .row_cell("Property", None, RowKey::name("Cost"), "Value", "9")
        .row_cell("Property", None, RowKey::name(first), "Value", "1")
        .row_cell("Property", None, RowKey::name(second), "Value", "2")
        .row_cell("User", None, RowKey::name("Out"), "Value", "0")
}

/// Gives the named rows of shape 10's Shape Data the `IX`es a file may give a row of a named section.
fn give_ix(s: &mut Session, ixes: &[(&str, u32)]) {
    s.edit("ix", |d| {
        let page = Arc::make_mut(d.pages.get_mut(0).unwrap());
        let shape = Arc::make_mut(page.shapes.get_mut(0).unwrap());
        let section = Arc::make_mut(&mut shape.sheet).section_mut(&SectionKey::new("Property", None)).unwrap();
        for at in 0..section.rows.len() {
            let name = section.rows[at].name.clone();
            if let Some((_, ix)) = ixes.iter().find(|(n, _)| name.as_deref() == Some(*n)) {
                Arc::make_mut(section.rows.get_mut(at).unwrap()).ix = Some(*ix);
            }
        }
        Ok(())
    })
    .unwrap();
}

/// What a formula on shape 10 that names `reference` reads, worked out where the formula is written.
fn read_by_formula(s: &mut Session, reference: &str) -> f64 {
    run(s, "sheet.set_cell", json!({"id": 10, "cell": "User.Out.Value", "formula": reference}));
    num(s, "User.Out.Value")
}

/// Both orders of two rows that differ only in case, and each of them as the row the call names.
fn twin_orders() -> [(&'static str, &'static str); 2] {
    [("Row_5", "ROW_5"), ("ROW_5", "Row_5")]
}

/// A delete of a row given by name takes the row that spells the name exactly, and its twin in another case stays: whichever comes first in the section. A formula that names the row
/// reads it, so the commands and the formulas agree on the row (the exact spelling wins).
#[test]
fn a_delete_takes_the_row_of_the_exact_spelling_and_not_its_twin_in_another_case() {
    for (first, second) in twin_orders() {
        for (gone, kept, kept_value) in [(first, second, 2.0), (second, first, 1.0)] {
            let mut s = one_shape(twin_shape(first, second));
            assert_eq!(
                read_by_formula(&mut s, &format!("Prop.{gone}.Value")),
                if gone == first { 1.0 } else { 2.0 },
                "a formula reads {gone} of [{first}, {second}]"
            );
            delete_row(&mut s, "Prop", json!(gone));
            assert_eq!(keys(&mut s, "Prop"), [json!("Cost"), json!(kept)], "deleting {gone} from [{first}, {second}]");
            assert_eq!(num(&mut s, &format!("Prop.{kept}.Value")), kept_value, "the row {kept} is as it was");
            assert_eq!(check_session(&s), Ok(()));
        }
    }
}

/// An insert that goes `after` or `before` a row given by name goes by the row of the exact spelling.
#[test]
fn an_insert_after_or_before_a_row_goes_by_the_row_of_the_exact_spelling() {
    for (first, second) in twin_orders() {
        for anchor in [first, second] {
            for after in [true, false] {
                let mut s = one_shape(twin_shape(first, second));
                add_row(&mut s, "Prop", json!({"name": "Fresh", if after { "after" } else { "before" }: anchor}));
                let mut want = vec![json!("Cost"), json!(first), json!(second)];
                let at = want.iter().position(|k| k == &json!(anchor)).unwrap();
                want.insert(if after { at + 1 } else { at }, json!("Fresh"));
                assert_eq!(keys(&mut s, "Prop"), want, "a row {} {anchor} among [{first}, {second}]", if after { "after" } else { "before" });
                assert_eq!(check_session(&s), Ok(()));
            }
        }
    }
}

/// `sheet.cell` and `sheet.set_cell` address a row by its exact spelling first, and a formula that names the row reads that row.
#[test]
fn a_cell_addressed_through_a_row_name_is_the_exact_rows_whichever_comes_first() {
    for (first, second) in twin_orders() {
        for (named, value) in [(first, 1.0), (second, 2.0)] {
            let mut s = one_shape(twin_shape(first, second));
            assert_eq!(num(&mut s, &format!("Prop.{named}.Value")), value, "sheet.cell {named} of [{first}, {second}]");
            assert_eq!(read_by_formula(&mut s, &format!("Prop.{named}.Value")), value, "a formula naming {named} of [{first}, {second}]");
            run(&mut s, "sheet.set_cell", json!({"id": 10, "cell": format!("Prop.{named}.Value"), "formula": "7"}));
            let other = if named == first { second } else { first };
            assert_eq!(num(&mut s, &format!("Prop.{named}.Value")), 7.0, "sheet.set_cell wrote {named}");
            assert_eq!(num(&mut s, &format!("Prop.{other}.Value")), if other == first { 1.0 } else { 2.0 }, "and left {other} as it was");
        }
    }
}

/// A name that no row spells exactly and that two rows spell in other cases names the row the formulas read: the lower `IX`, and with no `IX` the first in the section.
/// The commands and the formulas read the same row.
#[test]
fn a_name_no_row_spells_exactly_is_the_row_a_formula_reads_in_every_command() {
    for with_ix in [false, true] {
        for (first, second) in twin_orders() {
            // With no `IX` the first in the section wins; with `IX`es the second does.
            let (wins, value) = if with_ix { (second, 2.0) } else { (first, 1.0) };
            let build = || {
                let mut s = one_shape(twin_shape(first, second));
                if with_ix {
                    give_ix(&mut s, &[(first, 7), (second, 2)]);
                }
                s
            };
            let mut s = build();
            assert_eq!(read_by_formula(&mut s, "Prop.row_5.Value"), value, "a formula naming row_5 among [{first}, {second}], ix {with_ix}");
            assert_eq!(num(&mut s, "Prop.row_5.Value"), value, "sheet.cell");
            let mut s = build();
            delete_row(&mut s, "Prop", json!("row_5"));
            let left = if wins == first { second } else { first };
            assert_eq!(keys(&mut s, "Prop"), [json!("Cost"), json!(left)], "a delete of row_5 takes {wins} among [{first}, {second}], ix {with_ix}");
            let mut s = build();
            add_row(&mut s, "Prop", json!({"name": "Fresh", "after": "row_5"}));
            let at = [first, second].iter().position(|n| *n == wins).unwrap();
            let mut want = vec![json!("Cost"), json!(first), json!(second)];
            want.insert(at + 2, json!("Fresh"));
            assert_eq!(keys(&mut s, "Prop"), want, "an insert after row_5 goes after {wins}");
        }
    }
}

/// A row of the exact spelling that is deleted here is the row the name means, and it is not there: the name is not read as its twin in another case (the formulas read the
/// deleted row as nothing, as `Lookup::Deleted` says).
#[test]
fn a_name_spelled_exactly_by_a_deleted_row_is_not_read_as_its_twin() {
    let shape = twin_shape("Row_5", "ROW_5").del_row("Property", None, RowKey::name("Row_5"));
    let mut s = one_shape(shape);
    assert_eq!(keys(&mut s, "Prop"), [json!("Cost"), json!("ROW_5")]);
    let words = refused(&mut s, "sheet.delete_row", json!({"id": 10, "section": "Prop", "row": "Row_5"}));
    assert!(words.contains("no row Row_5"), "{words}");
    assert_eq!(keys(&mut s, "Prop"), [json!("Cost"), json!("ROW_5")], "the twin stays");
    delete_row(&mut s, "Prop", json!("ROW_5"));
    assert_eq!(keys(&mut s, "Prop"), [json!("Cost")]);
}
