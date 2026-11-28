//! The `Property` cells **in the forms files hold them**. A language is a tag such as `en-US`, a date never set is a
//! plain 0 and a set one ISO text with `DATE`, a calendar is one of MS-VSDX's fourteen numbers and a new field holds 0, the Ask on drop cell is `Verify`
//! in a file and in a formula, and a `Type` outside the eight is kept as written and listed as unknown. The vector is a file written here from the spec's forms
//! (`forms.vsdx`): every form is read as the spec says, and a VSDX save in place, a `.diacraft` and the after-save check keep each cell as it was written.

use std::sync::Arc;

use diacraft_calc::{CellAddress, EvalCtx, Value as Calc, evaluate};
use diacraft_doc::{Cell, CellAddr, Owner, Row, RowKey, Section, SectionKey, ShapeRef, SheetRef, Value};
use diacraft_engine::Session;
use diacraft_testkit::package::{PageSpec, VsdxBuilder, cell_u, shape_with};
use serde_json::{Value as Json, json};

use crate::edit::doc;
use crate::hyperlinks::edit_shape;
use crate::odg_build::Dir;
use crate::shapedata::{cell, define, fields, refused, set, shape, value_of, with_rows};
use crate::{blank_session, run};

/// The rows of the vector, each a row of a file's `Property` section as the spec gives its form; the row's name says which.
const FORMS: &[(&str, &str)] = &[
    // The eight numbers, each with a value in its type's form.
    ("T0", r#"<Cell N="Type" V="0"/><Cell N="Value" V="text" U="STR"/>"#),
    ("T1", r#"<Cell N="Type" V="1"/><Cell N="Format" V="Engineering;Sales"/><Cell N="Value" V="Sales" U="STR"/>"#),
    ("T2", r#"<Cell N="Type" V="2"/><Cell N="Value" V="3" U="IN"/>"#),
    ("T3", r#"<Cell N="Type" V="3"/><Cell N="Value" V="1" U="BOOL"/>"#),
    ("T4", r#"<Cell N="Type" V="4"/><Cell N="Format" V="Red;Green"/><Cell N="Value" V="Green" U="STR"/>"#),
    // A date that is set: ISO text with `DATE`, and a formula that calls `DATETIME`.
    (
        "T5",
        r#"<Cell N="Type" V="5"/><Cell N="Format" V="M/d/yyyy"/><Cell N="Value" V="2026-03-04T14:30:00" U="DATE" F="DATETIME(46085.604166666664)"/>"#,
    ),
    ("T6", r#"<Cell N="Type" V="6"/><Cell N="Value" V="1.5" U="ED"/>"#),
    ("T7", r#"<Cell N="Type" V="7"/><Cell N="Format" V="0.00"/><Cell N="Value" V="12.5000USD" U="CY"/>"#),
    // Numbers no row of a file holds, and a text.
    ("Type8", r#"<Cell N="Type" V="8"/><Cell N="Value" V="eight" U="STR"/>"#),
    ("TypeMinus", r#"<Cell N="Type" V="-1"/><Cell N="Value" V="minus"/>"#),
    ("TypeText", r#"<Cell N="Type" V="x"/><Cell N="Value" V="text"/>"#),
    ("TypeHalf", r#"<Cell N="Type" V="2.5"/><Cell N="Value" V="half"/>"#),
    // A date never set is a plain 0, with no unit.
    ("DateUnset", r#"<Cell N="Type" V="5"/><Cell N="Format" V=""/><Cell N="Value" V="0"/><Cell N="Calendar" V="0"/>"#),
    // A tag, and a number no file holds.
    ("LangTag", r#"<Cell N="Type" V="0"/><Cell N="Value" V="a" U="STR"/><Cell N="LangID" V="en-US"/>"#),
    ("LangNumber", r#"<Cell N="Type" V="0"/><Cell N="Value" V="b" U="STR"/><Cell N="LangID" V="1033"/>"#),
    ("Cal4", r#"<Cell N="Type" V="5"/><Cell N="Value" V="0"/><Cell N="Calendar" V="4"/>"#),
    ("Cal4Text", r#"<Cell N="Type" V="0"/><Cell N="Value" V="c" U="STR"/><Cell N="Calendar" V="4"/>"#),
    ("Cal12", r#"<Cell N="Type" V="5"/><Cell N="Value" V="0"/><Cell N="Calendar" V="12"/>"#),
    ("Cal99", r#"<Cell N="Type" V="5"/><Cell N="Value" V="0"/><Cell N="Calendar" V="99"/>"#),
    // The cell is `Verify`; a cell spelled `Ask` is no cell of the section, kept as it is.
    ("Asked", r#"<Cell N="Type" V="0"/><Cell N="Value" V="d" U="STR"/><Cell N="Verify" V="1"/>"#),
    ("AskSpelled", r#"<Cell N="Type" V="0"/><Cell N="Value" V="e" U="STR"/><Cell N="Ask" V="1"/>"#),
];

/// The vector: one shape (ID 1) whose `Property` section holds every row of [`FORMS`], each labelled by its name.
fn forms_vsdx() -> Vec<u8> {
    let rows: String = FORMS.iter().map(|(name, cells)| format!(r#"<Row N="{name}"><Cell N="Label" V="{name}"/>{cells}</Row>"#)).collect();
    let place = [cell_u("PinX", "2", "IN"), cell_u("PinY", "3", "IN"), cell_u("Width", "1", "IN"), cell_u("Height", "1", "IN")].concat();
    let one = shape_with(1, r#" Type="Shape""#, &format!(r#"{place}<Section N="Property">{rows}</Section>"#));
    let page = [cell_u("PageWidth", "8.5", "IN"), cell_u("PageHeight", "11", "IN")].concat();
    VsdxBuilder::new().page(PageSpec::new(1, "Page-1").sheet(&page).shapes(&one)).build()
}

fn opened(path: &str) -> Session {
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": path }));
    s
}

/// The page of the vector's shape and the shape's place.
fn the_shape(s: &Session) -> (u32, ShapeRef) {
    let d = doc(s);
    let page = d.pages.iter().find(|p| !p.background).unwrap().id;
    (page, d.find_shape(Owner::Page(page), 1).unwrap())
}

/// Every cell of every row of the shape's own `Property` section as the model holds it: the row's name, and each cell's name, formula, value and unit, in the order of the rows.
type Held = Vec<(String, Vec<(String, Option<String>, Option<String>, Option<String>)>)>;

fn held(s: &Session) -> Held {
    let (_, at) = the_shape(s);
    let shape = doc(s).shape(&at).unwrap();
    let section = shape.sheet.section(&SectionKey::new("Property", None)).unwrap();
    section
        .rows
        .iter()
        .map(|row| {
            let mut cells: Vec<_> = row
                .cells
                .iter()
                .map(|(n, c)| {
                    (
                        n.to_string(),
                        c.formula.text().map(str::to_owned),
                        c.value.as_ref().map(|v| v.text().to_owned()),
                        c.unit.as_ref().map(|u| u.to_string()),
                    )
                })
                .collect();
            cells.sort();
            (row.name.as_deref().unwrap_or("").to_owned(), cells)
        })
        .collect()
}

/// The fields of the vector's shape, hidden ones too, by row name.
fn listed(s: &mut Session) -> Vec<Json> {
    let (page, _) = the_shape(s);
    run(s, "shapedata.list", json!({"id": 1, "page": page, "hidden": true}))["fields"].as_array().unwrap().clone()
}

fn field<'a>(all: &'a [Json], row: &str) -> &'a Json {
    all.iter().find(|f| f["row"] == row).unwrap_or_else(|| panic!("no field {row}"))
}

/// Each form of the vector is read as the spec says.
#[test]
fn every_form_a_file_holds_is_read_as_the_spec_says() {
    let dir = Dir::new();
    let path = dir.write("forms.vsdx", &forms_vsdx());
    let mut s = opened(&path);
    let all = listed(&mut s);
    assert_eq!(all.len(), FORMS.len());
    // The eight numbers.
    let words = ["string", "fixed_list", "number", "boolean", "variable_list", "date", "duration", "currency"];
    for (n, word) in words.iter().enumerate() {
        let f = field(&all, &format!("T{n}"));
        assert_eq!((f["type"].as_str(), f["type_code"].as_u64()), (Some(*word), Some(n as u64)), "T{n}");
    }
    assert_eq!(field(&all, "T1")["items"], json!(["Engineering", "Sales"]));
    assert_eq!(field(&all, "T3")["shown"], "TRUE");
    assert_eq!(field(&all, "T5")["shown"], "3/4/2026", "a set date, through its picture");
    // A number outside the eight, and a text, are unknown: kept, and the value is a string's.
    for (row, code, shown) in
        [("Type8", json!(8), "eight"), ("TypeMinus", json!(-1), "minus"), ("TypeText", Json::Null, "text"), ("TypeHalf", Json::Null, "half")]
    {
        let f = field(&all, row);
        assert_eq!((f["type"].as_str(), &f["type_code"], f["shown"].as_str()), (Some("unknown"), &code, Some(shown)), "{row}: {f}");
    }
    // A date never set: 0 with no unit, which shows nothing.
    let unset = field(&all, "DateUnset");
    assert_eq!((unset["value"].as_str(), unset["unit"].is_null(), unset["shown"].as_str()), (Some("0"), true, Some("")), "{unset}");
    // The language as the cell holds it: a tag, or a number kept as the text it is.
    assert_eq!(field(&all, "LangTag")["lang_id"], "en-US");
    assert_eq!(field(&all, "LangNumber")["lang_id"], "1033");
    assert!(field(&all, "T0")["lang_id"].is_null(), "a row with no LangID cell");
    // The calendar's number and MS-VSDX's name for it; a number past the table has none.
    for (row, code, name) in [
        ("DateUnset", 0, json!("Western")),
        ("Cal4", 4, json!("Japanese Emperor")),
        ("Cal12", 12, json!("Gregorian Arabic")),
        ("Cal99", 99, Json::Null),
    ] {
        let f = field(&all, row);
        assert_eq!((f["calendar"].as_u64(), &f["calendar_name"]), (Some(code), &name), "{row}");
    }
    assert_eq!(field(&all, "Cal4Text")["shown"], "c", "a calendar counts only for a date");
    // Ask on drop is the `Verify` cell; `Ask` is no cell of the section.
    assert_eq!((field(&all, "Asked")["ask"].as_bool(), field(&all, "AskSpelled")["ask"].as_bool()), (Some(true), Some(false)));
}

/// The vector **kept as written**: a VSDX save in place (with a value entered and fields of each form edited), a `.diacraft` and the after-save check of a save across formats keep each cell's
/// formula, value and unit as the file had them.
#[test]
fn every_form_is_kept_as_written_by_a_vsdx_save_a_diacraft_and_the_after_save_check() {
    let dir = Dir::new();
    let path = dir.write("forms.vsdx", &forms_vsdx());
    let mut s = opened(&path);
    let want = held(&s);
    let fields_before = listed(&mut s);
    // The splice: values entered into a field of an unknown type and a date never set, and labels changed, write only what they change.
    let (page, _) = the_shape(&s);
    run(
        &mut s,
        "shapedata.set",
        json!({"id": 1, "page": page, "values": [{"row": "Type8", "value": "nine"}, {"row": "DateUnset", "value": ""}, {"row": "T0", "value": "edited"}]}),
    );
    let all: Vec<Json> = fields_before.iter().map(|f| json!({"row": f["row"]})).collect();
    let mut list = all.clone();
    for f in &mut list {
        if f["row"] == "TypeText" || f["row"] == "LangNumber" || f["row"] == "Cal99" {
            f["label"] = json!("relabelled");
        }
    }
    run(&mut s, "shapedata.define", json!({"id": 1, "page": page, "fields": list}));
    run(&mut s, "file.save", json!({}));
    let back = opened(&path);
    let edited = |want: &Held| -> Held {
        let mut out = want.clone();
        for (row, cells) in &mut out {
            for (name, _, v, u) in cells.iter_mut() {
                match (row.as_str(), name.as_str()) {
                    ("Type8", "Value") => *v = Some("nine".to_owned()),
                    ("T0", "Value") => *v = Some("edited".to_owned()),
                    ("TypeText" | "LangNumber" | "Cal99", "Label") => *v = Some("relabelled".to_owned()),
                    _ => continue,
                }
                // A text the engine writes says `STR`: the value entered and the label changed (a file's own cells that nothing edited keep what they had).
                if name == "Value" || name == "Label" {
                    *u = Some("STR".to_owned());
                }
            }
        }
        out
    };
    assert_eq!(held(&back), edited(&want), "the VSDX save in place: each cell as written, but the ones edited");
    // The model writers: a `.diacraft`, opened again, and saved from it as a VSDX and an `.odg`, each checked after the save.
    let mut s = opened(&path);
    let native = dir.path("forms.diacraft");
    run(&mut s, "file.save_as", json!({ "path": native }));
    let mut from_native = opened(&native);
    assert_eq!(held(&from_native), edited(&want), "the .diacraft");
    for ext in ["vsdx", "odg"] {
        let across = dir.path(&format!("across.{ext}"));
        let saved = run(&mut from_native, "file.save_as", json!({ "path": across }));
        assert_eq!(
            (saved["checked"]["done"].as_bool(), saved["checked"]["classes"].clone()),
            (Some(true), json!([])),
            "{ext}: the after-save check: {saved}"
        );
        assert_eq!(held(&opened(&across)), edited(&want), "{ext}");
    }
}

/// The commands take and write a language tag; a number a file holds is listed as it is and kept.
#[test]
fn a_language_is_given_and_written_as_a_tag_and_a_number_a_file_holds_is_kept() {
    let mut s = blank_session();
    let id = shape(&mut s);
    define(&mut s, id, json!([{"label": "When", "name": "When", "type": "date", "lang_id": "en-US"}]));
    assert_eq!(cell(&s, id, "When", "LangID"), Some((None, Some("en-US".to_owned()), None)), "the tag, as the files hold it");
    assert_eq!(fields(&mut s, id)[0]["lang_id"], "en-US");
    define(&mut s, id, json!([{"row": "When", "lang_id": "zh-Hant-TW"}]));
    assert_eq!(value_of(&s, id, "When", "LangID").as_deref(), Some("zh-Hant-TW"));
    for bad in [
        json!(1033),
        json!("en US"),
        json!("e"),
        json!("en--US"),
        json!("-en"),
        json!("en_US"),
        json!("1033"),
        json!(""),
        json!("abcdefghi"),
        json!("x".repeat(70)),
    ] {
        let words = refused(&mut s, "shapedata.define", json!({"id": id, "fields": [{"row": "When", "lang_id": bad}]}));
        assert!(words.contains("language tag"), "{bad}: {words}");
    }
    // A number a file holds is read as the text it is, and a list that does not name the cell keeps it.
    with_rows(&mut s, id, &[("Old", &[("Label", "Old"), ("Type", "0"), ("LangID", "1033")])]);
    let all = fields(&mut s, id);
    assert_eq!(field(&all, "Old")["lang_id"], "1033");
    define(&mut s, id, json!([{"row": "When"}, {"row": "Old", "label": "Older"}]));
    assert_eq!(value_of(&s, id, "Old", "LangID").as_deref(), Some("1033"), "kept as it was");
}

/// A calendar is a number MS-VSDX lists (0 to 13), named; a new field holds 0; `shapedata.types` lists the fourteen.
#[test]
fn a_calendar_is_one_of_the_fourteen_numbers_named_and_a_new_field_holds_0() {
    let mut s = blank_session();
    let id = shape(&mut s);
    define(&mut s, id, json!([{"label": "Due", "name": "Due", "type": "date"}, {"label": "Text", "name": "Text"}]));
    assert_eq!(cell(&s, id, "Due", "Calendar"), Some((None, Some("0".to_owned()), None)), "a new field holds 0, as every file's does");
    assert_eq!(value_of(&s, id, "Text", "Calendar").as_deref(), Some("0"));
    let due = field(&fields(&mut s, id), "Due").clone();
    assert_eq!((due["calendar"].as_u64(), due["calendar_name"].as_str()), (Some(0), Some("Western")));
    define(&mut s, id, json!([{"row": "Due", "calendar": 13}, {"row": "Text"}]));
    assert_eq!(value_of(&s, id, "Due", "Calendar").as_deref(), Some("13"));
    assert_eq!(field(&fields(&mut s, id), "Due")["calendar_name"], "Um-al-Qura");
    define(&mut s, id, json!([{"label": "Born", "name": "Born", "type": "date", "calendar": 2}]));
    assert_eq!(value_of(&s, id, "Born", "Calendar").as_deref(), Some("2"));
    for bad in [json!(14), json!(-1), json!(1.5), json!("4"), json!(65_535)] {
        let words = refused(&mut s, "shapedata.define", json!({"id": id, "fields": [{"row": "Born", "calendar": bad}]}));
        assert!(words.contains("0 to 13"), "{bad}: {words}");
    }
    let types = run(&mut s, "shapedata.types", json!({}));
    let calendars = types["calendars"].as_array().unwrap();
    assert_eq!(calendars.len(), 14);
    assert_eq!(calendars[0], json!({"code": 0, "name": "Western"}));
    assert_eq!(calendars[4], json!({"code": 4, "name": "Japanese Emperor"}));
    assert_eq!(calendars[12], json!({"code": 12, "name": "Gregorian Arabic"}));
    assert_eq!(calendars[13], json!({"code": 13, "name": "Um-al-Qura"}));
}

/// A date never set is a plain 0 with no unit, written so and shown as nothing; a set date is ISO text with `DATE`; an empty value is the plain 0 again.
#[test]
fn a_date_never_set_is_a_plain_zero_and_shows_nothing() {
    let mut s = blank_session();
    let id = shape(&mut s);
    define(&mut s, id, json!([{"label": "Due", "name": "Due", "type": "date"}]));
    assert_eq!(cell(&s, id, "Due", "Value"), Some((None, Some("0".to_owned()), None)), "a new date field");
    let due = fields(&mut s, id)[0].clone();
    assert_eq!((due["value"].as_str(), due["unit"].is_null(), due["shown"].as_str()), (Some("0"), true, Some("")));
    set(&mut s, id, "Due", "2026-03-04 14:30");
    assert_eq!(cell(&s, id, "Due", "Value"), Some((None, Some("2026-03-04T14:30:00".to_owned()), Some("DATE".to_owned()))));
    set(&mut s, id, "Due", "");
    assert_eq!(cell(&s, id, "Due", "Value"), Some((None, Some("0".to_owned()), None)), "cleared, it is the plain 0 again");
    // The serial 0 with its unit is a date that is set.
    edit_shape(&mut s, id, |sh| {
        let section = Arc::make_mut(&mut sh.sheet).section_mut(&SectionKey::new("Property", None)).unwrap();
        let mut row = Row { name: Some(Arc::from("Epoch")), ..Row::default() };
        row.cells.insert("Type".into(), Cell::constant(Value::from_text("5")));
        row.cells.insert("Value".into(), Cell::constant(Value::from_text("0")).with_unit("DATE"));
        section.rows.push(Arc::new(row));
    });
    assert_eq!(field(&fields(&mut s, id), "Epoch")["shown"], "12/30/1899");
    // A change of type: a date never set is an empty value of the new type, and an empty value is a date never set.
    define(&mut s, id, json!([{"row": "Due", "type": "number"}, {"row": "Epoch"}]));
    assert_eq!(cell(&s, id, "Due", "Value"), Some((None, Some(String::new()), None)), "not the number 0");
    define(&mut s, id, json!([{"row": "Due", "type": "date"}, {"row": "Epoch"}]));
    assert_eq!(cell(&s, id, "Due", "Value"), Some((None, Some("0".to_owned()), None)));
}

/// Ask on drop is the `Verify` cell: read and written as `Verify`, and `Prop.<name>.Verify` in a formula, where `Prop.<name>.Ask` names no cell.
#[test]
fn ask_on_drop_is_the_verify_cell_in_a_file_and_in_a_formula() {
    let mut s = blank_session();
    let id = shape(&mut s);
    with_rows(
        &mut s,
        id,
        &[("Qty", &[("Label", "Qty"), ("Type", "2"), ("Verify", "1")]), ("Other", &[("Label", "Other"), ("Type", "0"), ("Ask", "1")])],
    );
    let all = fields(&mut s, id);
    assert_eq!((field(&all, "Qty")["ask"].as_bool(), field(&all, "Other")["ask"].as_bool()), (Some(true), Some(false)));
    define(&mut s, id, json!([{"row": "Qty", "ask": false}, {"row": "Other", "ask": true}, {"label": "New", "name": "New", "ask": true}]));
    assert_eq!(value_of(&s, id, "Qty", "Verify").as_deref(), Some("0"));
    assert_eq!(value_of(&s, id, "Other", "Verify").as_deref(), Some("1"));
    assert_eq!(value_of(&s, id, "New", "Verify").as_deref(), Some("1"));
    assert_eq!(value_of(&s, id, "Other", "Ask").as_deref(), Some("1"), "a cell spelled Ask is kept as it was");
    assert_eq!(value_of(&s, id, "New", "Ask"), None, "and none is written");
    // A formula reads the cell as `Prop.<name>.Verify`.
    edit_shape(&mut s, id, |sh| {
        let mut section = Section::new("User", None);
        for (name, formula) in [("ReadsVerify", "Prop.New.Verify"), ("ReadsAsk", "Prop.New.Ask")] {
            let mut row = Row { name: Some(Arc::from(name)), ..Row::default() };
            row.cells.insert("Value".into(), Cell::with_formula(formula, Some(Value::from_text("0"))));
            section.rows.push(Arc::new(row));
        }
        Arc::make_mut(&mut sh.sheet).sections.push(Arc::new(section));
    });
    let d = doc(&s);
    let sheet = SheetRef::Shape(d.find_shape(Owner::Page(1), id).unwrap());
    let worked = |row: &str| {
        evaluate(d, &CellAddress::new(sheet.clone(), CellAddr::in_row("User", None, RowKey::name(row), "Value")), &mut EvalCtx::default())
    };
    assert!(
        matches!(worked("ReadsVerify"), Calc::Bool(true)) || matches!(worked("ReadsVerify"), Calc::Number(q) if q.stored == 1.0),
        "{:?}",
        worked("ReadsVerify")
    );
    assert!(matches!(worked("ReadsAsk"), Calc::Error(_)), "Prop.New.Ask names no cell: {:?}", worked("ReadsAsk"));
}

/// A `Type` outside the eight is kept as written through a value entered and a list that names the row, and a type given to it is written.
#[test]
fn a_type_outside_the_eight_is_kept_as_written_and_listed_as_unknown() {
    let mut s = blank_session();
    let id = shape(&mut s);
    with_rows(&mut s, id, &[("Odd", &[("Label", "Odd"), ("Type", "8")]), ("Half", &[("Label", "Half"), ("Type", "2.5")])]);
    let all = fields(&mut s, id);
    assert_eq!((field(&all, "Odd")["type"].as_str(), field(&all, "Odd")["type_code"].as_i64()), (Some("unknown"), Some(8)));
    assert_eq!(field(&all, "Half")["type"], "unknown", "2.5 is no number of the table, and not the 3 it rounds to");
    set(&mut s, id, "Odd", "a text");
    set(&mut s, id, "Half", "12");
    define(&mut s, id, json!([{"row": "Odd", "label": "Still odd", "prompt": "p"}, {"row": "Half", "sort_key": "1"}]));
    assert_eq!(value_of(&s, id, "Odd", "Type").as_deref(), Some("8"));
    assert_eq!(value_of(&s, id, "Half", "Type").as_deref(), Some("2.5"));
    assert_eq!(cell(&s, id, "Odd", "Value"), Some((None, Some("a text".to_owned()), Some("STR".to_owned()))), "a value is a string's");
    let words = refused(&mut s, "shapedata.define", json!({"id": id, "fields": [{"row": "Odd", "type": "unknown"}, {"row": "Half"}]}));
    assert!(words.contains("unknown") && words.contains("string"), "{words}");
    define(&mut s, id, json!([{"row": "Odd", "type": "number"}, {"row": "Half"}]));
    assert_eq!(value_of(&s, id, "Odd", "Type").as_deref(), Some("2"), "a type given is written");
}
