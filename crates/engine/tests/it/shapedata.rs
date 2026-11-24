//! The Shape Data of a shape (`shapedata.list`, `shapedata.set`, `shapedata.define`, `shapedata.types`), through `Session::execute` as the control channel, the CLI and
//! MCP call them. A field is a row of the shape's `Property` section ("one row per data item on a shape") with the cells `Label`, `Prompt`, `Type`, `Format`, `Value`,
//! `SortKey`, `Invisible`, `Verify` (the Ask on drop cell), `LangID` and `Calendar`; has the rows this follows. The decisions the specs leave open are in
//! `docs/editing-commands.md`, "Shape Data".

use std::sync::Arc;

use diacraft_doc::{Cell, Document, Master, Owner, Row, RowKey, Section, SectionKey, Shape, ShapeList, ShapeType, Value};
use diacraft_engine::Session;
use serde_json::{Value as Json, json};

use crate::edit::{doc, drop_shape};
use crate::hyperlinks::edit_shape;
use crate::{blank_session, check_session, run, snapshot};

/// The message of a refusal in words.
pub(crate) fn refused(s: &mut Session, command: &str, params: Json) -> String {
    let before = snapshot(s);
    let e = s.execute(command, &params).expect_err("the call is refused");
    assert_eq!(snapshot(s), before, "{command}: a refusal changes nothing");
    match &e {
        diacraft_engine::EngineError::BadParams { msg, .. } | diacraft_engine::EngineError::Failed { msg, .. } => msg.clone(),
        other => panic!("expected a refusal in words, got {other:?}"),
    }
}

pub(crate) fn shape(s: &mut Session) -> u32 {
    drop_shape(s, "rectangle", 3.0, 3.0)
}

/// The fields `shapedata.list` gives, each as the answer holds it.
pub(crate) fn fields(s: &mut Session, id: u32) -> Vec<Json> {
    run(s, "shapedata.list", json!({"id": id}))["fields"].as_array().unwrap().clone()
}

/// The rows of the fields listed, in the order listed.
fn rows(s: &mut Session, id: u32) -> Vec<String> {
    fields(s, id).iter().map(|f| f["row"].as_str().unwrap().to_owned()).collect()
}

/// A cell of a row of shape `id`'s `Property` section: its formula, its value and its unit.
pub(crate) fn cell(s: &Session, id: u32, row: &str, name: &str) -> Option<(Option<String>, Option<String>, Option<String>)> {
    let d = doc(s);
    let at = d.find_shape(Owner::Page(1), id)?;
    let shape = d.shape(&at)?;
    let r = shape.sheet.section(&SectionKey::new("Property", None))?.row(&RowKey::name(row))?.clone();
    let c = r.cells.get(name)?;
    Some((c.formula.text().map(str::to_owned), c.value.as_ref().map(|v| v.text().to_owned()), c.unit.as_ref().map(|u| u.to_string())))
}

pub(crate) fn value_of(s: &Session, id: u32, row: &str, name: &str) -> Option<String> {
    cell(s, id, row, name).and_then(|c| c.1)
}

/// Gives shape `id` rows in the `Property` section, as a file holds them: each row's name and the cells it has (a value, and a formula when the text starts with `=`, whose value is 0).
pub(crate) fn with_rows(s: &mut Session, id: u32, rows: &[(&str, &[(&str, &str)])]) {
    edit_shape(s, id, |shape| {
        let section = Arc::make_mut(&mut shape.sheet).section_mut(&SectionKey::new("Property", None)).unwrap();
        for (row_name, cells) in rows {
            let mut row = Row { name: Some(Arc::from(*row_name)), ..Row::default() };
            for (name, text) in *cells {
                let c = match text.strip_prefix('=') {
                    Some(formula) => Cell::with_formula(formula, Some(Value::from_text("0"))),
                    None => Cell::constant(Value::from_text(text)),
                };
                row.cells.insert((*name).into(), c);
            }
            section.rows.push(Arc::new(row));
        }
    });
}

pub(crate) fn define(s: &mut Session, id: u32, list: Json) -> Json {
    run(s, "shapedata.define", json!({"id": id, "fields": list}))
}

pub(crate) fn set(s: &mut Session, id: u32, row: &str, value: &str) -> Json {
    run(s, "shapedata.set", json!({"id": id, "row": row, "value": value}))
}

fn undo_steps(s: &Session) -> usize {
    snapshot(s).undo_steps
}

/// A shape with one field of each type, named by the type.
fn typed() -> (Session, u32) {
    let mut s = blank_session();
    let id = shape(&mut s);
    define(
        &mut s,
        id,
        json!([
            {"label": "Text", "name": "Text", "type": "string"},
            {"label": "Pick", "name": "Pick", "type": "fixed_list", "format": "Red;Green;Blue"},
            {"label": "Count", "name": "Count", "type": "number"},
            {"label": "Done", "name": "Done", "type": "boolean"},
            {"label": "Tag", "name": "Tag", "type": "variable_list", "format": "Red;Green"},
            {"label": "When", "name": "When", "type": "date"},
            {"label": "Span", "name": "Span", "type": "duration"},
            {"label": "Cost", "name": "Cost", "type": "currency"},
        ]),
    );
    (s, id)
}

#[test]
fn the_types_are_the_eight_of_the_define_dialog_with_the_formats_each_offers() {
    let mut s = blank_session();
    let answer = run(&mut s, "shapedata.types", json!({}));
    let types = answer["types"].as_array().unwrap();
    let words: Vec<&str> = types.iter().map(|t| t["id"].as_str().unwrap()).collect();
    assert_eq!(words, ["string", "fixed_list", "number", "boolean", "variable_list", "date", "duration", "currency"]);
    let codes: Vec<u64> = types.iter().map(|t| t["code"].as_u64().unwrap()).collect();
    assert_eq!(codes, [0, 1, 2, 3, 4, 5, 6, 7], "the Type cell's number");
    let formats = |id: &str| -> Vec<String> {
        types.iter().find(|t| t["id"] == id).unwrap()["formats"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["picture"].as_str().unwrap().to_owned())
            .collect()
    };
    assert!(formats("date").len() > 3 && formats("number").iter().any(|p| p == "0.00"), "the formats change with the type");
    assert!(formats("boolean").is_empty() && formats("fixed_list").is_empty(), "a list's Format is its items, and a truth value has no picture");
    assert!(types.iter().all(|t| t["label"].as_str().is_some_and(|l| !l.is_empty())));
}

#[test]
fn a_new_field_is_a_row_with_the_cells_the_dialog_shows_and_is_one_undo_step() {
    let mut s = blank_session();
    let id = shape(&mut s);
    let before = undo_steps(&s);
    let answer = define(&mut s, id, json!([{"label": "Unit cost", "type": "currency", "prompt": "Cost of one", "sort_key": "2"}]));
    assert_eq!(undo_steps(&s), before + 1, "one undo step");
    let first = &answer["fields"][0];
    assert_eq!(
        (first["row"].as_str(), first["label"].as_str(), first["type"].as_str(), first["type_code"].as_u64()),
        (Some("Unit_cost"), Some("Unit cost"), Some("currency"), Some(7))
    );
    assert_eq!(value_of(&s, id, "Unit_cost", "Label").as_deref(), Some("Unit cost"));
    assert_eq!(value_of(&s, id, "Unit_cost", "Type").as_deref(), Some("7"));
    assert_eq!(value_of(&s, id, "Unit_cost", "Prompt").as_deref(), Some("Cost of one"));
    assert_eq!(value_of(&s, id, "Unit_cost", "SortKey").as_deref(), Some("2"));
    assert_eq!(value_of(&s, id, "Unit_cost", "Invisible").as_deref(), Some("0"));
    assert_eq!(value_of(&s, id, "Unit_cost", "Verify").as_deref(), Some("0"), "the cell Ask on drop is stored in");
    assert_eq!(value_of(&s, id, "Unit_cost", "Value").as_deref(), Some(""), "a new field is blank");
    run(&mut s, "edit.undo", json!({}));
    assert!(fields(&mut s, id).is_empty(), "undo takes the field away");
    assert!(cell(&s, id, "Unit_cost", "Label").is_none());
    check_session(&s).unwrap();
}

#[test]
fn a_new_field_with_no_label_is_called_property_n_and_a_name_is_made_of_the_label() {
    let mut s = blank_session();
    let id = shape(&mut s);
    define(&mut s, id, json!([{}, {}, {"label": "  Cost / unit (EUR) "}, {"label": "Cost / unit (EUR)"}, {"label": "7 days"}]));
    assert_eq!(rows(&mut s, id), ["Property1", "Property2", "Cost_unit_EUR", "Cost_unit_EUR_2", "_7_days"]);
    let all = fields(&mut s, id);
    assert_eq!(all[0]["label"], "Property1");
    assert_eq!(all[2]["label"], "Cost / unit (EUR)", "a label is kept as it is, trimmed");
    // `name` is the row's name as given, for a field that is new.
    define(&mut s, id, json!([{"row": "Property1"}, {"label": "Own", "name": "Mine"}]));
    assert_eq!(rows(&mut s, id), ["Property1", "Mine"], "a row the list leaves out goes");
    let words = refused(&mut s, "shapedata.define", json!({"id": id, "fields": [{"name": "has space"}]}));
    assert!(words.contains("has space") && words.contains("name"), "{words}");
    let words = refused(&mut s, "shapedata.define", json!({"id": id, "fields": [{"name": "A"}, {"name": "a"}]}));
    assert!(words.contains("twice") || words.contains("already"), "{words}");
    let words = refused(&mut s, "shapedata.define", json!({"id": id, "fields": [{"row": "Property1", "name": "Other"}]}));
    assert!(words.contains("renamed") || words.contains("rename"), "a row is not renamed, formulas read it by name: {words}");
}

#[test]
fn a_value_is_typed_by_the_type_of_its_field_and_written_with_the_unit_of_that_type() {
    let (mut s, id) = typed();
    let shown =
        |s: &mut Session, row: &str| -> String { fields(s, id).iter().find(|f| f["row"] == row).unwrap()["shown"].as_str().unwrap().to_owned() };
    // String: any text, kept as text.
    set(&mut s, id, "Text", "Ann Lee");
    assert_eq!(cell(&s, id, "Text", "Value"), Some((None, Some("Ann Lee".into()), Some("STR".into()))));
    // Number: digits, or a number with a unit.
    set(&mut s, id, "Count", "12.5");
    assert_eq!(cell(&s, id, "Count", "Value"), Some((None, Some("12.5".into()), None)));
    set(&mut s, id, "Count", "3 in");
    assert_eq!(cell(&s, id, "Count", "Value"), Some((None, Some("3".into()), Some("IN".into()))));
    set(&mut s, id, "Count", "25.4 mm");
    assert_eq!(value_of(&s, id, "Count", "Value").as_deref(), Some("1"), "the value is held in inches");
    // Boolean.
    set(&mut s, id, "Done", "yes");
    assert_eq!(cell(&s, id, "Done", "Value"), Some((None, Some("1".into()), Some("BOOL".into()))));
    assert_eq!(shown(&mut s, "Done"), "TRUE");
    set(&mut s, id, "Done", "FALSE");
    assert_eq!(value_of(&s, id, "Done", "Value").as_deref(), Some("0"));
    assert_eq!(shown(&mut s, "Done"), "FALSE");
    // Date, in the two forms a person types.
    set(&mut s, id, "When", "2026-03-04");
    assert_eq!(cell(&s, id, "When", "Value"), Some((None, Some("2026-03-04T00:00:00".into()), Some("DATE".into()))));
    set(&mut s, id, "When", "3/4/2026 2:30 pm");
    assert_eq!(value_of(&s, id, "When", "Value").as_deref(), Some("2026-03-04T14:30:00"));
    assert!(shown(&mut s, "When").starts_with("3/4/2026"), "{}", shown(&mut s, "When"));
    // Duration: a number and a unit, days with none.
    set(&mut s, id, "Span", "1.5");
    assert_eq!(cell(&s, id, "Span", "Value"), Some((None, Some("1.5".into()), Some("ED".into()))));
    set(&mut s, id, "Span", "36 hours");
    assert_eq!(value_of(&s, id, "Span", "Value").as_deref(), Some("1.5"), "held in days");
    assert_eq!(cell(&s, id, "Span", "Value").unwrap().2.as_deref(), Some("EH"));
    // Currency: a symbol and thousands marks are read.
    set(&mut s, id, "Cost", "$1,234.5");
    assert_eq!(cell(&s, id, "Cost", "Value"), Some((None, Some("1234.5000".into()), Some("CY".into()))));
    assert_eq!(shown(&mut s, "Cost"), "$1,234.50");
    // A number typed in a cell that has a unit takes that unit.
    set(&mut s, id, "Count", "2 mm");
    set(&mut s, id, "Count", "5");
    assert_eq!(cell(&s, id, "Count", "Value").unwrap().2.as_deref(), Some("MM"));
    check_session(&s).unwrap();
}

#[test]
fn a_value_that_is_no_value_of_the_type_is_refused_in_words_and_changes_nothing() {
    let (mut s, id) = typed();
    for (row, text, word) in [
        ("Count", "twelve", "number"),
        ("Done", "maybe", "true or false"),
        ("When", "2026-02-30", "date"),
        ("When", "someday", "date"),
        ("Span", "3 fortnights", "duration"),
        ("Cost", "lots", "currency"),
        ("Pick", "Purple", "Red, Green, Blue"),
    ] {
        let words = refused(&mut s, "shapedata.set", json!({"id": id, "row": row, "value": text}));
        assert!(words.contains(word), "{row} {text}: {words}");
        assert!(words.contains(text), "the text refused is named: {words}");
    }
    let words = refused(&mut s, "shapedata.set", json!({"id": id, "row": "Nope", "value": "x"}));
    assert!(words.contains("Nope") && words.contains("Text"), "a row the shape lacks is named and what it has is listed: {words}");
}

#[test]
fn a_fixed_list_takes_only_its_items_and_a_variable_list_adds_what_is_typed() {
    let (mut s, id) = typed();
    set(&mut s, id, "Pick", "Green");
    assert_eq!(value_of(&s, id, "Pick", "Value").as_deref(), Some("Green"));
    let pick = fields(&mut s, id).into_iter().find(|f| f["row"] == "Pick").unwrap();
    assert_eq!(pick["items"], json!(["Red", "Green", "Blue"]), "the items are the Format's, apart at the semicolons");
    set(&mut s, id, "Pick", "");
    assert_eq!(value_of(&s, id, "Pick", "Value").as_deref(), Some(""), "a field can be cleared");
    // A variable list: an item it has joins nothing; a new one joins the Format, in the same undo step.
    let before = undo_steps(&s);
    set(&mut s, id, "Tag", "Green");
    assert_eq!(value_of(&s, id, "Tag", "Format").as_deref(), Some("Red;Green"));
    set(&mut s, id, "Tag", "Blue");
    assert_eq!(value_of(&s, id, "Tag", "Format").as_deref(), Some("Red;Green;Blue"), "the page says only that it joins the list: it goes last");
    assert_eq!(undo_steps(&s), before + 2);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(
        (value_of(&s, id, "Tag", "Value").as_deref(), value_of(&s, id, "Tag", "Format").as_deref()),
        (Some("Green"), Some("Red;Green")),
        "one step takes both back"
    );
}

#[test]
fn an_entered_value_replaces_a_formula_even_one_under_guard() {
    let mut s = blank_session();
    let id = shape(&mut s);
    with_rows(
        &mut s,
        id,
        &[("Cost", &[("Label", "Cost"), ("Type", "2"), ("Value", "=GUARD(5)")]), ("Open", &[("Label", "Open"), ("Type", "2"), ("Value", "=2+2")])],
    );
    set(&mut s, id, "Cost", "9");
    assert_eq!(
        cell(&s, id, "Cost", "Value"),
        Some((None, Some("9".into()), None)),
        "the Value cell page: an entered value replaces any formula, even under GUARD"
    );
    set(&mut s, id, "Open", "7");
    assert_eq!(cell(&s, id, "Open", "Value"), Some((None, Some("7".into()), None)));
}

#[test]
fn a_formula_that_reads_a_field_is_worked_out_again_when_the_value_is_entered() {
    let mut s = blank_session();
    let id = shape(&mut s);
    define(&mut s, id, json!([{"label": "Cost", "name": "Cost", "type": "number"}]));
    edit_shape(&mut s, id, |shape| {
        let mut section = Section::new("User", None);
        let mut row = Row { name: Some(Arc::from("Double")), ..Row::default() };
        row.cells.insert("Value".into(), Cell::with_formula("Prop.Cost*2", Some(Value::from_text("0"))));
        section.rows.push(Arc::new(row));
        Arc::make_mut(&mut shape.sheet).sections.push(Arc::new(section));
    });
    set(&mut s, id, "Cost", "21");
    let d = doc(&s);
    let at = d.find_shape(Owner::Page(1), id).unwrap();
    let shape = d.shape(&at).unwrap();
    let double = shape
        .sheet
        .section(&SectionKey::new("User", None))
        .unwrap()
        .row(&RowKey::name("Double"))
        .unwrap()
        .cells
        .get("Value")
        .unwrap()
        .value
        .as_ref()
        .unwrap()
        .text()
        .to_owned();
    assert_eq!(double, "42", "User.Double is Prop.Cost*2");
}

#[test]
fn a_text_field_that_shows_a_value_follows_it() {
    let mut s = blank_session();
    let id = shape(&mut s);
    define(&mut s, id, json!([{"label": "Owner", "name": "Owner"}]));
    set(&mut s, id, "Owner", "Ann");
    run(&mut s, "shape.set_text", json!({"id": id, "text": "Owner: "}));
    run(&mut s, "text.insert_field", json!({"id": id, "category": "shape_data", "field": "Owner"}));
    set(&mut s, id, "Owner", "Bob");
    let text = diacraft_engine::draw::text_view(doc(&s), 1, id).unwrap();
    assert_eq!(text.fields.first().map(|f| f.text.as_str()), Some("Bob"), "the field of the text shows the new value");
}

#[test]
fn a_hidden_field_keeps_its_value_and_leaves_the_list() {
    let mut s = blank_session();
    let id = shape(&mut s);
    define(&mut s, id, json!([{"label": "Seen", "name": "Seen"}, {"label": "Secret", "name": "Secret", "invisible": true}]));
    let listed = run(&mut s, "shapedata.list", json!({"id": id}));
    assert_eq!(rows(&mut s, id), ["Seen"]);
    assert_eq!(listed["hidden"], 1, "the answer counts what it left out");
    let all = run(&mut s, "shapedata.list", json!({"id": id, "hidden": true}));
    assert_eq!(all["fields"].as_array().unwrap().len(), 2);
    assert_eq!(all["fields"][1]["invisible"], true);
    assert_eq!(all["hidden"], 0);
    // A hidden field can be given a value, and keeps it.
    set(&mut s, id, "Secret", "kept");
    assert_eq!(value_of(&s, id, "Secret", "Value").as_deref(), Some("kept"));
    // `Invisible` may be a formula: it is read as the formula's value.
    with_rows(&mut s, id, &[("Calc", &[("Label", "Calc"), ("Type", "0"), ("Invisible", "=1")])]);
    assert!(rows(&mut s, id).contains(&"Calc".to_owned()), "a formula whose value is 0 leaves the field in");
    edit_shape(&mut s, id, |shape| {
        let section = Arc::make_mut(&mut shape.sheet).section_mut(&SectionKey::new("Property", None)).unwrap();
        for row in section.rows.iter_mut().filter(|r| r.name.as_deref() == Some("Calc")) {
            Arc::make_mut(row).cells.insert("Invisible".into(), Cell::with_formula("1", Some(Value::from_text("1"))));
        }
    });
    assert!(!rows(&mut s, id).contains(&"Calc".to_owned()), "a formula that gives 1 hides the field");
    assert_eq!(run(&mut s, "shapedata.list", json!({"id": id}))["hidden"], 2);
}

#[test]
fn fields_are_listed_by_sort_key_then_row_order_and_the_unkeyed_come_last() {
    let mut s = blank_session();
    let id = shape(&mut s);
    // V2 of the spec: keys 1, 3, 2 on Item, Price, Quantity, and a fourth field with no key.
    define(
        &mut s,
        id,
        json!([
            {"name": "Item", "label": "Item", "sort_key": "1"},
            {"name": "Price", "label": "Price", "sort_key": "3"},
            {"name": "Quantity", "label": "Quantity", "sort_key": "2"},
            {"name": "Fourth", "label": "Fourth"},
        ]),
    );
    assert_eq!(rows(&mut s, id), ["Item", "Quantity", "Price", "Fourth"]);
    // Compared as text and ignoring case; equal keys keep the row order.
    define(
        &mut s,
        id,
        json!([
            {"row": "Item", "sort_key": "b"},
            {"row": "Price", "sort_key": "A"},
            {"row": "Quantity", "sort_key": "B"},
            {"row": "Fourth", "sort_key": ""},
            {"name": "Fifth", "label": "Fifth", "sort_key": "a"},
        ]),
    );
    assert_eq!(rows(&mut s, id), ["Price", "Fifth", "Item", "Quantity", "Fourth"]);
    // The listed order is what `define` returns too.
    let answer = define(&mut s, id, json!([{"row": "Fourth"}, {"row": "Fifth"}]));
    assert_eq!(answer["fields"].as_array().unwrap().iter().map(|f| f["row"].as_str().unwrap()).collect::<Vec<_>>(), ["Fifth", "Fourth"]);
}

#[test]
fn a_field_is_deleted_by_leaving_it_out_of_the_list_in_one_undo_step() {
    let (mut s, id) = typed();
    set(&mut s, id, "Text", "kept");
    let before = snapshot(&s).document.unwrap();
    let steps = undo_steps(&s);
    let answer = define(&mut s, id, json!([{"row": "Text"}, {"row": "Count"}]));
    assert_eq!(rows(&mut s, id), ["Text", "Count"]);
    assert_eq!(answer["fields"].as_array().unwrap().len(), 2);
    assert_eq!(undo_steps(&s), steps + 1);
    assert_eq!(value_of(&s, id, "Text", "Value").as_deref(), Some("kept"), "a field that stays keeps its value");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(rows(&mut s, id).len(), 8);
    assert_eq!(snapshot(&s).document.unwrap(), before, "undo restores the shape as it was");
    // Every field taken away leaves no section.
    define(&mut s, id, json!([]));
    assert!(fields(&mut s, id).is_empty());
    let d = doc(&s);
    let at = d.find_shape(Owner::Page(1), id).unwrap();
    assert!(d.shape(&at).unwrap().sheet.section(&SectionKey::new("Property", None)).is_none(), "a section left with no row is no section");
}

#[test]
fn a_list_that_changes_nothing_is_no_undo_step() {
    let (mut s, id) = typed();
    let before = snapshot(&s);
    let steps = undo_steps(&s);
    let all: Vec<Json> = run(&mut s, "shapedata.list", json!({"id": id, "hidden": true}))["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| json!({"row": f["row"]}))
        .collect();
    define(&mut s, id, Json::Array(all));
    assert_eq!(undo_steps(&s), steps, "no step for a list that is the list the shape has");
    assert_eq!(snapshot(&s).document, before.document);
    // The same value again is no step either.
    set(&mut s, id, "Text", "x");
    let steps = undo_steps(&s);
    set(&mut s, id, "Text", "x");
    assert_eq!(undo_steps(&s), steps);
}

#[test]
fn a_type_change_keeps_a_value_that_reads_as_the_new_type_and_otherwise_keeps_the_text() {
    let mut s = blank_session();
    let id = shape(&mut s);
    define(&mut s, id, json!([{"label": "Qty", "name": "Qty"}]));
    set(&mut s, id, "Qty", "12");
    define(&mut s, id, json!([{"row": "Qty", "type": "number"}]));
    assert_eq!(cell(&s, id, "Qty", "Value"), Some((None, Some("12".into()), None)), "12 reads as a number: it is one now");
    assert_eq!(value_of(&s, id, "Qty", "Type").as_deref(), Some("2"));
    define(&mut s, id, json!([{"row": "Qty", "type": "boolean"}]));
    assert_eq!(value_of(&s, id, "Qty", "Value").as_deref(), Some("12"), "12 is no truth value: the value is kept as it was, and the answer says so");
    let answer = define(&mut s, id, json!([{"row": "Qty", "type": "date"}]));
    assert!(answer["notes"].as_array().is_some_and(|n| n.iter().any(|w| w.as_str().unwrap().contains("Qty"))), "{answer}");
    // A value given with a new type is the new type's.
    define(&mut s, id, json!([{"row": "Qty", "type": "boolean", "value": "no"}]));
    assert_eq!(cell(&s, id, "Qty", "Value"), Some((None, Some("0".into()), Some("BOOL".into()))));
}

#[test]
fn ask_language_calendar_and_prompt_are_kept_in_their_cells() {
    let mut s = blank_session();
    let id = shape(&mut s);
    define(
        &mut s,
        id,
        json!([{"label": "When", "name": "When", "type": "date", "ask": true, "lang_id": "en-US", "calendar": 1, "prompt": "Pick a day"}]),
    );
    assert_eq!(value_of(&s, id, "When", "Verify").as_deref(), Some("1"));
    assert_eq!(value_of(&s, id, "When", "LangID").as_deref(), Some("en-US"));
    assert_eq!(value_of(&s, id, "When", "Calendar").as_deref(), Some("1"));
    let field = &fields(&mut s, id)[0];
    assert_eq!(
        (field["ask"].as_bool(), field["lang_id"].as_str(), field["calendar"].as_u64(), field["prompt"].as_str()),
        (Some(true), Some("en-US"), Some(1), Some("Pick a day"))
    );
    define(&mut s, id, json!([{"row": "When", "ask": false}]));
    assert_eq!(value_of(&s, id, "When", "Verify").as_deref(), Some("0"));
    assert_eq!(value_of(&s, id, "When", "LangID").as_deref(), Some("en-US"), "a cell the list does not name stays");
}

#[test]
fn a_display_format_decides_how_a_value_is_shown_and_not_what_it_is() {
    let mut s = blank_session();
    let id = shape(&mut s);
    define(
        &mut s,
        id,
        json!([{"label": "Cost", "name": "Cost", "type": "number", "format": "0.00"}, {"label": "Name", "name": "Name", "type": "string", "format": "@+"}]),
    );
    set(&mut s, id, "Cost", "12.5");
    let cost = fields(&mut s, id).into_iter().find(|f| f["row"] == "Cost").unwrap();
    assert_eq!((cost["value"].as_str(), cost["shown"].as_str(), cost["format"].as_str()), (Some("12.5"), Some("12.50"), Some("0.00")));
    define(&mut s, id, json!([{"row": "Cost", "format": "0.0"}, {"row": "Name"}]));
    assert_eq!(fields(&mut s, id)[0]["shown"], "12.5");
}

#[test]
fn the_rows_a_shape_takes_from_its_master_are_listed_set_and_deleted() {
    let mut s = blank_session();
    let mut master_shape = Shape::new(1);
    master_shape.kind = ShapeType::Shape;
    let mut section = Section::new("Property", None);
    for (name, label, ty) in [("Cost", "Cost", "2"), ("Owner", "Owner", "0")] {
        let mut row = Row { name: Some(Arc::from(name)), ..Row::default() };
        for (c, v) in [("Label", label), ("Type", ty), ("Value", ""), ("Format", ""), ("Invisible", "0")] {
            row.cells.insert(c.into(), Cell::constant(Value::from_text(v)));
        }
        section.rows.push(Arc::new(row));
    }
    Arc::make_mut(&mut master_shape.sheet).sections.push(Arc::new(section));
    s.edit("a master", |d| {
        d.add_master(Master { id: 5, shapes: ShapeList::from_vec(vec![Arc::new(master_shape)]), ..Master::default() }).unwrap();
        Ok(())
    })
    .unwrap();
    let id = shape(&mut s);
    edit_shape(&mut s, id, |sh| sh.master = Some(5));
    assert_eq!(rows(&mut s, id), ["Cost", "Owner"], "the instance has no Property section of its own: the master's rows are its fields");
    set(&mut s, id, "Cost", "9");
    assert_eq!(value_of(&s, id, "Cost", "Value").as_deref(), Some("9"));
    assert_eq!(fields(&mut s, id)[0]["value"], "9");
    // The row the list leaves out goes from this shape and stays on the master.
    define(&mut s, id, json!([{"row": "Cost"}]));
    assert_eq!(rows(&mut s, id), ["Cost"]);
    let d: &Document = doc(&s);
    assert_eq!(
        d.masters.iter().next().unwrap().shapes.iter().next().unwrap().sheet.section(&SectionKey::new("Property", None)).unwrap().rows.len(),
        2,
        "the master is as it was"
    );
    check_session(&s).unwrap();
}

#[test]
fn a_command_with_hostile_parameters_is_refused_in_words() {
    let mut s = blank_session();
    let id = shape(&mut s);
    for (command, params, word) in [
        ("shapedata.list", json!({}), "id"),
        ("shapedata.list", json!({"id": 99}), "99"),
        ("shapedata.list", json!({"id": id, "hidden": 3}), "hidden"),
        ("shapedata.set", json!({"id": id}), "row"),
        ("shapedata.set", json!({"id": id, "row": "X"}), "value"),
        ("shapedata.set", json!({"id": id, "row": "X", "value": [1]}), "value"),
        ("shapedata.set", json!({"id": id, "row": "X", "value": "a", "values": []}), "values"),
        ("shapedata.define", json!({"id": id}), "fields"),
        ("shapedata.define", json!({"id": id, "fields": {}}), "fields"),
        ("shapedata.define", json!({"id": id, "fields": [3]}), "object"),
        ("shapedata.define", json!({"id": id, "fields": [{"colour": "red"}]}), "colour"),
        ("shapedata.define", json!({"id": id, "fields": [{"type": "purple"}]}), "purple"),
        ("shapedata.define", json!({"id": id, "fields": [{"label": 3}]}), "label"),
        ("shapedata.define", json!({"id": id, "fields": [{"invisible": "yes"}]}), "invisible"),
        ("shapedata.define", json!({"id": id, "fields": [{"lang_id": -1}]}), "lang_id"),
        ("shapedata.define", json!({"id": id, "fields": [{"calendar": 1.5}]}), "calendar"),
        ("shapedata.define", json!({"id": id, "fields": [{"row": "Nope"}]}), "Nope"),
        ("shapedata.define", json!({"id": id, "fields": [{"label": "a\u{0}b"}]}), "label"),
        ("shapedata.define", json!({"id": id, "fields": [{"label": "x".repeat(5000)}]}), "label"),
        ("shapedata.define", json!({"id": id, "fields": [{"type": "fixed_list", "format": "A;B", "value": "C"}]}), "A, B"),
    ] {
        let words = refused(&mut s, command, params.clone());
        assert!(words.contains(word), "{command} {params}: {words}");
    }
    let many: Vec<Json> = (0..600).map(|_| json!({})).collect();
    let words = refused(&mut s, "shapedata.define", json!({"id": id, "fields": many}));
    assert!(words.contains("512"), "{words}");
    check_session(&s).unwrap();
}

#[test]
fn a_shape_with_more_rows_than_are_read_is_refused_before_any_is_read() {
    let mut s = blank_session();
    let id = shape(&mut s);
    edit_shape(&mut s, id, |shape| {
        let mut section = Section::new("Property", None);
        for n in 0..4_200u32 {
            section.rows.push(Arc::new(Row { name: Some(Arc::from(format!("R{n}"))), ..Row::default() }));
        }
        Arc::make_mut(&mut shape.sheet).sections.push(Arc::new(section));
    });
    for (command, params) in [
        ("shapedata.list", json!({"id": id})),
        ("shapedata.set", json!({"id": id, "row": "R1", "value": "x"})),
        ("shapedata.define", json!({"id": id, "fields": []})),
    ] {
        let words = refused(&mut s, command, params);
        assert!(words.contains("4200") || words.contains("4,200"), "{command}: {words}");
    }
}

#[test]
fn a_file_with_a_text_past_the_cap_is_cut_and_a_call_that_reads_too_much_is_refused() {
    let mut s = blank_session();
    let id = shape(&mut s);
    let huge = "x".repeat(200_000);
    with_rows(&mut s, id, &[("Big", &[("Label", huge.as_str()), ("Value", huge.as_str()), ("Type", "0")])]);
    let listed = fields(&mut s, id);
    assert!(listed[0]["label"].as_str().unwrap().chars().count() <= 4_096, "a label is cut at the cap when it is read");
    assert!(listed[0]["value"].as_str().unwrap().chars().count() <= 32_768, "so is a value");
    // Many such rows are more than one call reads.
    let rows_of: Vec<(String, Vec<(&str, &str)>)> =
        (0..400).map(|n| (format!("B{n}"), vec![("Label", huge.as_str()), ("Value", huge.as_str())])).collect();
    edit_shape(&mut s, id, |shape| {
        let mut section = Section::new("Property", None);
        for (name, cells) in &rows_of {
            let mut row = Row { name: Some(Arc::from(name.as_str())), ..Row::default() };
            for (c, v) in cells {
                row.cells.insert((*c).into(), Cell::constant(Value::from_text(v)));
            }
            section.rows.push(Arc::new(row));
        }
        Arc::make_mut(&mut shape.sheet).sections.clear();
        Arc::make_mut(&mut shape.sheet).sections.push(Arc::new(section));
    });
    let words = refused(&mut s, "shapedata.list", json!({"id": id}));
    assert!(words.contains("more text than is read"), "{words}");
}

#[test]
fn a_locked_layer_refuses_the_commands_that_change_a_shape_on_it() {
    let mut s = blank_session();
    let id = shape(&mut s);
    define(&mut s, id, json!([{"label": "Cost", "name": "Cost"}]));
    run(&mut s, "layer.add", json!({"name": "Notes", "page": 1}));
    run(&mut s, "layer.assign", json!({"ids": [id], "layers": ["Notes"], "page": 1}));
    run(&mut s, "layer.set", json!({"layer": "Notes", "lock": true, "page": 1}));
    for (command, params) in
        [("shapedata.set", json!({"id": id, "row": "Cost", "value": "1"})), ("shapedata.define", json!({"id": id, "fields": []}))]
    {
        let words = refused(&mut s, command, params);
        assert!(words.contains("Notes"), "the layer is named: {words}");
    }
    // A query still reads it.
    assert_eq!(rows(&mut s, id), ["Cost"]);
}

#[test]
fn several_values_are_set_in_one_undo_step() {
    let (mut s, id) = typed();
    let steps = undo_steps(&s);
    let answer = run(
        &mut s,
        "shapedata.set",
        json!({"id": id, "values": [{"row": "Text", "value": "Ann"}, {"row": "Count", "value": "4"}, {"row": "Done", "value": "true"}]}),
    );
    assert_eq!(undo_steps(&s), steps + 1);
    assert_eq!(value_of(&s, id, "Text", "Value").as_deref(), Some("Ann"));
    assert_eq!(value_of(&s, id, "Count", "Value").as_deref(), Some("4"));
    assert_eq!((answer["changed"].as_bool(), answer["values"].as_u64()), (Some(true), Some(3)));
    // One of them wrong: none is set.
    let words = refused(&mut s, "shapedata.set", json!({"id": id, "values": [{"row": "Text", "value": "Bob"}, {"row": "Count", "value": "many"}]}));
    assert!(words.contains("many"), "{words}");
    assert_eq!(value_of(&s, id, "Text", "Value").as_deref(), Some("Ann"));
}

/// Every cell of a row holds each junk value a file can hold (`junk_values`: numbers past a double, text with controls and NULs, a character outside the BMP): the fields are listed or refused
/// in words, a value is entered or refused in words, and a list of them is defined, and no call answers with an internal error or leaves an invalid document.
#[test]
fn a_row_whose_cells_hold_junk_is_listed_and_edited_without_a_failure_of_the_engine() {
    use diacraft_testkit::strategies::junk_values;
    let texts: Vec<String> = junk_values().iter().map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_owned)).collect();
    let mut s = blank_session();
    let id = shape(&mut s);
    for cell in ["Label", "Type", "Format", "Value", "Prompt", "SortKey", "Invisible", "Verify", "LangID", "Calendar"] {
        for (n, text) in texts.iter().enumerate() {
            let row = format!("{cell}_{n}");
            with_rows(&mut s, id, &[(row.as_str(), &[(cell, text.as_str())])]);
        }
    }
    let internal = |r: &Result<Json, diacraft_engine::EngineError>| matches!(r, Err(diacraft_engine::EngineError::Internal { .. }));
    let listed = s.execute("shapedata.list", &json!({"id": id, "hidden": true}));
    assert!(!internal(&listed), "{listed:?}");
    if let Ok(answer) = &listed {
        for field in answer["fields"].as_array().unwrap() {
            let row = field["row"].as_str().unwrap().to_owned();
            for text in &texts {
                let r = s.execute("shapedata.set", &json!({"id": id, "row": row, "value": text}));
                assert!(!internal(&r), "{row} {text:?}: {r:?}");
            }
            let r = s
                .execute("shapedata.define", &json!({"id": id, "fields": [{"row": row, "type": "date", "value": "2026-01-01"}, {"label": "after"}]}));
            assert!(!internal(&r), "{row}: {r:?}");
        }
    }
    for value in junk_values() {
        for key in ["label", "type", "format", "value", "prompt", "sort_key", "invisible", "ask", "lang_id", "calendar", "name", "row"] {
            let r = s.execute("shapedata.define", &json!({"id": id, "fields": [{ key: value }]}));
            assert!(!internal(&r), "{key} {value}: {r:?}");
        }
    }
    check_session(&s).unwrap();
}
