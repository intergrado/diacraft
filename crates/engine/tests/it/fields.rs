//! Fields and characters in a shape's text (`text.insert_field`, `text.field_choices`, `text.field_sample`, `text.update_fields`, `text.insert`), through `Session::execute` as
//! the control channel, the CLI and MCP call them. A field is a `fld` of the text and a row of the shape's `Field` section;
//! lists the categories and maps each field to its function. The decisions the specs leave open are in
//! `docs/editing-commands.md`, "Fields".

use std::sync::Arc;

use diacraft_doc::{Cell, Document, Owner, Row, RowKey, Section, SectionKey, Shape, Value};
use diacraft_engine::Session;
use diacraft_engine::draw::text_view;
use serde_json::{Value as Json, json};

use crate::edit::{doc, drop_shape};
use crate::hyperlinks::edit_shape;
use crate::{bad_params_msg, blank_session, check_session, run, snapshot};

/// 2026-01-01T00:00:00Z.
const NEW_YEAR: i64 = 1_767_225_600;

fn refused(s: &mut Session, command: &str, params: Json) -> String {
    let e = s.execute(command, &params).expect_err("the call is refused");
    match &e {
        diacraft_engine::EngineError::BadParams { msg, .. } | diacraft_engine::EngineError::Failed { msg, .. } => msg.clone(),
        other => panic!("expected a refusal in words, got {other:?}"),
    }
}

/// A shape with this text on page 1.
fn labelled(s: &mut Session, text: &str) -> u32 {
    let id = drop_shape(s, "rectangle", 3.0, 3.0);
    if !text.is_empty() {
        run(s, "shape.set_text", json!({"id": id, "text": text}));
    }
    id
}

/// What the shape's text is made of: the characters and the fields (`at`, row number and what each shows).
fn view(s: &Session, id: u32) -> (String, Vec<(usize, u32, String)>) {
    let v = text_view(doc(s), 1, id).expect("the shape");
    (v.chars, v.fields.iter().map(|f| (f.at, f.ix, f.text.clone())).collect())
}

/// The text a shape shows: its characters and the results of its fields, in order.
fn shown(s: &Session, id: u32) -> String {
    let at = doc(s).find_shape(Owner::Page(1), id).unwrap();
    let shape = doc(s).shape(&at).unwrap();
    shape
        .text
        .as_ref()
        .map(|t| {
            t.parts
                .iter()
                .map(|p| match p {
                    diacraft_doc::TextPart::Chars(c) => c.to_string(),
                    diacraft_doc::TextPart::Fld { text, .. } => text.to_string(),
                    _ => String::new(),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// A cell of row `ix` of shape `id`'s `Field` section: its formula and its value as text.
fn field_cell(d: &Document, id: u32, ix: u32, cell: &str) -> Option<(Option<String>, Option<String>)> {
    let at = d.find_shape(Owner::Page(1), id)?;
    let shape = d.shape(&at)?;
    let row = shape.sheet.section(&SectionKey::new("Field", None))?.row(&RowKey::Ix(ix))?.clone();
    let c = row.cells.get(cell)?;
    Some((c.formula.text().map(str::to_owned), c.value.as_ref().map(|v| v.text().to_owned())))
}

fn insert(s: &mut Session, id: u32, category: &str, field: &str) -> Json {
    run(s, "text.insert_field", json!({"id": id, "category": category, "field": field}))
}

/// Gives shape `id` rows in the section `name` (`Property` or `User`): each row's name and cells.
fn with_rows(s: &mut Session, id: u32, name: &str, rows: &[(&str, &[(&str, &str)])]) {
    edit_shape(s, id, |shape| {
        let mut section = Section::new(name, None);
        for (row_name, cells) in rows {
            let mut row = Row { name: Some(Arc::from(*row_name)), ..Row::default() };
            for (cell, text) in *cells {
                row.cells.insert((*cell).into(), Cell::constant(Value::from_text(text)));
            }
            section.rows.push(Arc::new(row));
        }
        Arc::make_mut(&mut shape.sheet).sections.push(Arc::new(section));
    });
}

#[test]
fn a_page_number_field_is_a_fld_of_the_text_and_a_row_of_the_field_section() {
    let mut s = blank_session();
    let id = labelled(&mut s, "Page ");
    let made = insert(&mut s, id, "page", "page_number");
    assert_eq!((made["ix"].as_u64(), made["shown"].as_str(), made["formula"].as_str()), (Some(0), Some("1"), Some("PAGENUMBER()")));
    assert_eq!(view(&s, id), ("Page ".to_owned(), vec![(5, 0, "1".to_owned())]), "the field is at the end of the text, showing the page's number");
    assert_eq!(shown(&s, id), "Page 1");
    assert_eq!(field_cell(doc(&s), id, 0, "Value").unwrap().0.as_deref(), Some("PAGENUMBER()"));
    assert_eq!(field_cell(doc(&s), id, 0, "Value").unwrap().1.as_deref(), Some("1"), "the row holds the value it shows");
    assert_eq!(field_cell(doc(&s), id, 0, "Type").unwrap().1.as_deref(), Some("2"), "a number field has Type 2");
    // A second field is the next row.
    let second = insert(&mut s, id, "page", "page_count");
    assert_eq!(second["ix"], 1);
    assert_eq!(view(&s, id).1.len(), 2);
}

#[test]
fn a_field_goes_where_at_says_and_the_characters_around_it_are_one_run_each() {
    let mut s = blank_session();
    let id = labelled(&mut s, "ab");
    run(&mut s, "text.insert_field", json!({"id": id, "category": "page", "field": "page_number", "at": 1}));
    assert_eq!(view(&s, id), ("ab".to_owned(), vec![(1, 0, "1".to_owned())]));
    assert_eq!(shown(&s, id), "a1b");
    run(&mut s, "text.insert_field", json!({"id": id, "category": "page", "field": "page_number", "at": 0}));
    assert_eq!(shown(&s, id), "1a1b");
    // A shape with no text takes a field as its whole text.
    let empty = labelled(&mut s, "");
    insert(&mut s, empty, "page", "page_number");
    assert_eq!(shown(&s, empty), "1");
    // A place past the text is refused in words.
    let words = refused(&mut s, "text.insert_field", json!({"id": id, "category": "page", "field": "page_number", "at": 9}));
    assert!(words.contains("9") && words.contains("2 characters"), "{words}");
}

#[test]
fn an_insertion_is_one_undo_step_and_undo_takes_the_field_and_its_row_away() {
    let mut s = blank_session();
    let id = labelled(&mut s, "x");
    let steps = s.history().unwrap().undo_len();
    insert(&mut s, id, "page", "name");
    assert_eq!(s.history().unwrap().undo_len(), steps + 1);
    assert!(field_cell(doc(&s), id, 0, "Value").is_some());
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(view(&s, id), ("x".to_owned(), vec![]));
    assert!(field_cell(doc(&s), id, 0, "Value").is_none(), "the row went with the field");
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(view(&s, id).1.len(), 1);
}

#[test]
fn the_date_and_time_fields_read_the_clock_and_the_properties_and_the_format_is_the_pictures() {
    let mut s = blank_session();
    s.pin_clock(Some(NEW_YEAR));
    let id = labelled(&mut s, "");
    let now = run(&mut s, "text.insert_field", json!({"id": id, "category": "date_time", "field": "current", "format": "yyyy-MM-dd HH:mm"}));
    assert_eq!((now["shown"].as_str(), now["formula"].as_str()), (Some("2026-01-01 00:00"), Some("NOW()")));
    assert_eq!(field_cell(doc(&s), id, 0, "Type").unwrap().1.as_deref(), Some("5"), "a date field has Type 5");
    assert_eq!(field_cell(doc(&s), id, 0, "Format").unwrap().1.as_deref(), Some("yyyy-MM-dd HH:mm"));
    // The value of a clock is kept in the row, as a date, so that a save holds what the field shows.
    let value = field_cell(doc(&s), id, 0, "Value").unwrap().1.unwrap();
    assert!(value.starts_with("2026-01-01T00:00:00"), "{value}");
    // With no format a date has the default one, `{<211>}`: the date and the time.
    let default = insert(&mut s, id, "date_time", "current");
    assert_eq!(default["format"], "{<211>}");
    assert!(default["shown"].as_str().unwrap().starts_with("1/1/2026 12:00"), "{default}");
    // The creation date of a drawing that has none yet is when it is made (now), and the last edit's the same; a print date is the one the file holds, none here.
    let created = insert(&mut s, id, "date_time", "creation");
    assert!(created["shown"].as_str().unwrap().starts_with("1/1/2026"), "{created}");
    s.edit("test: dates", |d| {
        let props = Arc::make_mut(&mut d.props);
        props.created = Some(Arc::from("2025-03-04T05:06:07Z"));
        props.last_printed = Some(Arc::from("2025-07-08T09:10:11Z"));
        Ok(())
    })
    .unwrap();
    let created = run(&mut s, "text.insert_field", json!({"id": id, "category": "date_time", "field": "creation", "format": "yyyy-MM-dd"}));
    assert_eq!(created["shown"], "2025-03-04");
    let printed = run(&mut s, "text.insert_field", json!({"id": id, "category": "date_time", "field": "print", "format": "yyyy-MM-dd"}));
    assert_eq!(printed["shown"], "2025-07-08");
}

#[test]
fn the_document_fields_show_the_documents_properties() {
    let mut s = blank_session();
    s.edit("test: properties", |d| {
        let props = Arc::make_mut(&mut d.props);
        props.creator = Some(Arc::from("Ann Author"));
        props.company = Some(Arc::from("Example Co"));
        props.keywords = Some(Arc::from("a, b"));
        Ok(())
    })
    .unwrap();
    let id = labelled(&mut s, "");
    for (field, formula, want) in [
        ("creator", "CREATOR()", "Ann Author"),
        ("company", "COMPANY()", "Example Co"),
        ("keywords", "KEYWORDS()", "a, b"),
        ("description", "DESCRIPTION()", ""),
        ("title", "TITLE()", "Untitled"),
        ("subject", "SUBJECT()", ""),
        ("manager", "MANAGER()", ""),
        ("category", "CATEGORY()", ""),
        ("hyperlink_base", "HYPERLINKBASE()", ""),
        ("directory", "DIRECTORY()", ""),
        ("filename", "FILENAME()", ""),
    ] {
        let made = insert(&mut s, id, "document", field);
        assert_eq!((made["formula"].as_str(), made["shown"].as_str()), (Some(formula), Some(want)), "{field}");
    }
}

#[test]
fn the_geometry_fields_follow_the_shape_when_it_changes() {
    let mut s = blank_session();
    let id = labelled(&mut s, "W ");
    let width = insert(&mut s, id, "geometry", "width");
    assert_eq!(width["formula"], "Width");
    assert!(width["shown"].as_str().unwrap().starts_with("1.50"), "{width}");
    insert(&mut s, id, "geometry", "height");
    assert!(shown(&s, id).starts_with("W 1.50"), "{}", shown(&s, id));
    // A field updates by itself when its source changes.
    run(&mut s, "shape.resize", json!({"id": id, "x": 2.0, "y": 2.0, "width": 3.0, "height": 2.0}));
    let after = shown(&s, id);
    assert!(after.starts_with("W 3.00") && after.contains("2.00"), "{after}");
    run(&mut s, "shape.rotate", json!({"ids": [id], "angle": 90.0}));
    let angle = insert(&mut s, id, "geometry", "angle");
    assert!(angle["shown"].as_str().unwrap().starts_with("90"), "{angle}");
}

#[test]
fn the_page_fields_follow_the_pages() {
    let mut s = blank_session();
    let id = labelled(&mut s, "");
    insert(&mut s, id, "page", "name");
    insert(&mut s, id, "page", "page_count");
    let name = run(&mut s, "document.inspect", json!({}))["pages"][0]["name"].as_str().unwrap().to_owned();
    assert_eq!(shown(&s, id), format!("{name}1"));
    run(&mut s, "page.rename", json!({"id": 1, "name": "Overview"}));
    assert_eq!(shown(&s, id), "Overview1", "a renamed page: the field shows the new name");
    run(&mut s, "page.add", json!({"name": "Second"}));
    assert_eq!(shown(&s, id), "Overview2", "a page added: the count");
}

#[test]
fn the_shape_data_and_user_cell_fields_read_the_rows_of_the_shape() {
    let mut s = blank_session();
    let id = labelled(&mut s, "");
    with_rows(
        &mut s,
        id,
        "Property",
        &[("Cost", &[("Value", "12.5"), ("Type", "2"), ("Format", "0.00")]), ("Owner", &[("Value", "Ann"), ("Type", "0")])],
    );
    with_rows(&mut s, id, "User", &[("Note", &[("Value", "Hello")])]);
    let cost = run(&mut s, "text.insert_field", json!({"id": id, "category": "shape_data", "field": "Cost"}));
    assert_eq!((cost["formula"].as_str(), cost["shown"].as_str()), (Some("Prop.Cost"), Some("12.50")));
    let owner = insert(&mut s, id, "shape_data", "Owner");
    assert_eq!((owner["formula"].as_str(), owner["shown"].as_str()), (Some("Prop.Owner"), Some("Ann")));
    let note = insert(&mut s, id, "user_cell", "Note");
    assert_eq!((note["formula"].as_str(), note["shown"].as_str()), (Some("User.Note"), Some("Hello")));
    // A row the shape does not have is refused, naming it and listing what it has.
    let words = refused(&mut s, "text.insert_field", json!({"id": id, "category": "shape_data", "field": "Nope"}));
    assert!(words.contains("Nope") && words.contains("Cost"), "{words}");
    // The value changes: `text.update_fields` shows the new one.
    edit_shape(&mut s, id, |shape| {
        let sheet = Arc::make_mut(&mut shape.sheet);
        let section = sheet.section_mut(&SectionKey::new("Property", None)).unwrap();
        let row = Arc::make_mut(section.rows.iter_mut().find(|r| r.name.as_deref() == Some("Cost")).unwrap());
        row.cells.insert("Value".into(), Cell::constant(Value::from_text("99")));
    });
    let updated = run(&mut s, "text.update_fields", json!({"id": id}));
    assert_eq!(updated["updated"], 1, "{updated}");
    assert!(shown(&s, id).starts_with("99.00"), "{}", shown(&s, id));
}

#[test]
fn the_object_info_fields_are_the_shapes_own() {
    let mut s = blank_session();
    let id = labelled(&mut s, "");
    let got = insert(&mut s, id, "object", "id");
    assert_eq!((got["formula"].as_str(), got["shown"].as_str()), (Some("ID()"), Some(id.to_string().as_str())));
    for (field, formula) in
        [("name", "NAME()"), ("master", "MASTERNAME()"), ("type", "TYPEDESC()"), ("data1", "DATA1()"), ("data2", "DATA2()"), ("data3", "DATA3()")]
    {
        assert_eq!(insert(&mut s, id, "object", field)["formula"], formula, "{field}");
    }
}

#[test]
fn a_custom_formula_is_a_field_too() {
    let mut s = blank_session();
    let id = labelled(&mut s, "Sum ");
    let made = run(&mut s, "text.insert_field", json!({"id": id, "category": "custom", "formula": "1+2"}));
    assert_eq!((made["formula"].as_str(), made["shown"].as_str()), (Some("1+2"), Some("3")));
    let area = run(&mut s, "text.insert_field", json!({"id": id, "category": "custom", "formula": "Width*Height", "format": "0.0"}));
    assert!(area["shown"].as_str().unwrap().starts_with("1.5"), "{area}");
    let text = run(&mut s, "text.insert_field", json!({"id": id, "category": "custom", "formula": "\"a\"&\"b\""}));
    assert_eq!(text["shown"], "ab");
    assert_eq!(field_cell(doc(&s), id, 2, "Type").unwrap().1.as_deref(), Some("0"), "a text result has Type 0");
    for (formula, words_have) in [("", "formula"), ("1/0", "#DIV/0!"), ("1+", "does not read"), ("NOSUCHFUNCTION(1)", "NOSUCHFUNCTION")] {
        let before = snapshot(&s);
        let words = refused(&mut s, "text.insert_field", json!({"id": id, "category": "custom", "formula": formula}));
        assert!(words.contains(words_have), "{formula}: {words}");
        assert_eq!(snapshot(&s), before, "{formula}: a refusal changes nothing");
    }
}

#[test]
fn what_the_dialog_may_ask_is_checked_in_words() {
    let mut s = blank_session();
    let id = labelled(&mut s, "x");
    let before = snapshot(&s);
    for (params, words_have) in [
        (json!({"id": id, "category": "nonsense", "field": "x"}), "nonsense"),
        (json!({"id": id, "category": "page", "field": "nonsense"}), "nonsense"),
        (json!({"id": id, "category": "page"}), "field"),
        (json!({"id": id, "category": "custom"}), "formula"),
        (json!({"id": id, "category": "page", "field": "name", "formula": "1"}), "formula"),
        (json!({"id": 999, "category": "page", "field": "name"}), "999"),
        (json!({"id": id, "category": "page", "field": "name", "format": "x".repeat(5_000)}), "format"),
        (json!({"id": id, "category": "page", "field": "name", "format": "a\u{7}"}), "format"),
        (json!({"id": id, "category": "custom", "formula": "x".repeat(5_000)}), "formula"),
        (json!({"id": id, "category": "page", "field": "name", "at": -1}), "at"),
    ] {
        let e = s.execute("text.insert_field", &params).expect_err("refused");
        let words = match &e {
            diacraft_engine::EngineError::BadParams { msg, .. } | diacraft_engine::EngineError::Failed { msg, .. } => msg.clone(),
            other => panic!("{params}: {other:?}"),
        };
        assert!(words.contains(words_have), "{params}: {words}");
    }
    assert_eq!(snapshot(&s), before);
    check_session(&s).unwrap();
}

#[test]
fn a_shape_holds_at_most_a_thousand_fields() {
    let mut s = blank_session();
    let id = labelled(&mut s, "x");
    // 1,024 fields are put in the model itself (a command makes one at a time, and each is a step of the history).
    edit_shape(&mut s, id, |shape| {
        let mut parts = vec![diacraft_doc::TextPart::Chars(Arc::from("x"))];
        let mut section = Section::new("Field", None);
        for n in 0..1_024u32 {
            parts.push(diacraft_doc::TextPart::Fld { ix: n, text: Arc::from("1") });
            let mut row = Row { ix: Some(n), ..Row::default() };
            row.cells.insert("Value".into(), Cell::with_formula("1", Some(Value::from_text("1"))));
            section.rows.push(Arc::new(row));
        }
        shape.text = Some(Arc::new(diacraft_doc::Text { parts, ..Default::default() }));
        Arc::make_mut(&mut shape.sheet).sections.push(Arc::new(section));
    });
    let words = refused(&mut s, "text.insert_field", json!({"id": id, "category": "custom", "formula": "2"}));
    assert!(words.contains("1024"), "{words}");
    // They are all refreshed by one call, within the work the call may do.
    let updated = run(&mut s, "text.update_fields", json!({"id": id}));
    assert_eq!(updated["updated"], 0, "they all show what their formulas give already");
}

#[test]
fn the_choices_are_the_categories_of_the_dialog_with_the_fields_of_each() {
    let mut s = blank_session();
    let id = labelled(&mut s, "");
    with_rows(&mut s, id, "Property", &[("Cost", &[("Value", "1")]), ("Owner", &[("Value", "x")])]);
    with_rows(&mut s, id, "User", &[("Note", &[("Value", "x")])]);
    let choices = run(&mut s, "text.field_choices", json!({"id": id}));
    let categories: Vec<&str> = choices["categories"].as_array().unwrap().iter().map(|c| c["id"].as_str().unwrap()).collect();
    assert_eq!(categories, ["custom", "date_time", "document", "geometry", "object", "page", "shape_data", "user_cell"]);
    let fields_of = |category: &str| -> Vec<String> {
        let c = choices["categories"].as_array().unwrap().iter().find(|c| c["id"] == category).unwrap();
        c["fields"].as_array().unwrap().iter().map(|f| f["id"].as_str().unwrap().to_owned()).collect()
    };
    assert_eq!(fields_of("date_time"), ["creation", "current", "last_edit", "print"]);
    assert_eq!(fields_of("page"), ["background", "name", "page_count", "page_number"]);
    assert_eq!(fields_of("geometry"), ["height", "width", "angle"]);
    assert_eq!(fields_of("shape_data"), ["Cost", "Owner"], "the shape's own data rows");
    assert_eq!(fields_of("user_cell"), ["Note"]);
    assert_eq!(fields_of("document").len(), 11);
    assert_eq!(fields_of("object").len(), 7);
    assert!(choices["formats"]["date"].as_array().unwrap().iter().any(|f| f["code"] == 211 && f["picture"] == "{<211>}"));
    // Without a shape the data categories list nothing.
    let none = run(&mut s, "text.field_choices", json!({}));
    let data = none["categories"].as_array().unwrap().iter().find(|c| c["id"] == "shape_data").unwrap();
    assert!(data["fields"].as_array().unwrap().is_empty());
}

#[test]
fn a_sample_shows_what_a_field_would_show_and_changes_nothing() {
    let mut s = blank_session();
    s.pin_clock(Some(NEW_YEAR));
    let id = labelled(&mut s, "x");
    let before = snapshot(&s);
    let sample = run(&mut s, "text.field_sample", json!({"id": id, "category": "date_time", "field": "current", "format": "yyyy"}));
    assert_eq!(sample["shown"], "2026");
    let sample = run(&mut s, "text.field_sample", json!({"id": id, "category": "page", "field": "page_number"}));
    assert_eq!(sample["shown"], "1");
    assert_eq!(snapshot(&s), before, "no step, no journal entry, no revision");
    let words = refused(&mut s, "text.field_sample", json!({"id": id, "category": "custom", "formula": "1/0"}));
    assert!(words.contains("#DIV/0!"), "{words}");
}

#[test]
fn update_fields_shows_the_clock_again_and_says_how_many_changed() {
    let mut s = blank_session();
    s.pin_clock(Some(NEW_YEAR));
    let id = labelled(&mut s, "");
    run(&mut s, "text.insert_field", json!({"id": id, "category": "date_time", "field": "current", "format": "yyyy-MM-dd"}));
    assert_eq!(shown(&s, id), "2026-01-01");
    s.pin_clock(Some(NEW_YEAR + 86_400 * 31));
    let steps = s.history().unwrap().undo_len();
    let updated = run(&mut s, "text.update_fields", json!({}));
    assert_eq!(updated["updated"], 1);
    assert_eq!(shown(&s, id), "2026-02-01");
    assert_eq!(s.history().unwrap().undo_len(), steps + 1, "one undo step");
    // Nothing changed: no step.
    let again = run(&mut s, "text.update_fields", json!({"id": id}));
    assert_eq!(again["updated"], 0);
    assert_eq!(s.history().unwrap().undo_len(), steps + 1);
    let words = refused(&mut s, "text.update_fields", json!({"id": 999}));
    assert!(words.contains("999"), "{words}");
}

#[test]
fn a_field_stays_when_the_text_around_it_is_edited() {
    let mut s = blank_session();
    let id = labelled(&mut s, "ab");
    run(&mut s, "text.insert_field", json!({"id": id, "category": "page", "field": "page_number", "at": 1}));
    run(&mut s, "shape.set_text", json!({"id": id, "text": "aXb", "start": 0, "end": 2}));
    assert_eq!(view(&s, id).1.len(), 1, "the field is still there");
    assert_eq!(shown(&s, id), "aXb1", "the field keeps its place among the parts of the text, here after the characters that were all replaced");
}

#[test]
fn text_insert_puts_characters_at_a_place_and_keeps_the_fields() {
    let mut s = blank_session();
    let id = labelled(&mut s, "ac");
    let made = run(&mut s, "text.insert", json!({"id": id, "text": "b", "at": 1}));
    assert_eq!(made["id"], id);
    assert_eq!(view(&s, id).0, "abc");
    // No place: the end.
    run(&mut s, "text.insert", json!({"id": id, "text": "\u{e9}\u{3a9}"}));
    assert_eq!(view(&s, id).0, "abc\u{e9}\u{3a9}");
    // A shape with no text takes it.
    let empty = labelled(&mut s, "");
    run(&mut s, "text.insert", json!({"id": empty, "text": "\u{a9}"}));
    assert_eq!(view(&s, empty).0, "\u{a9}");
    // A field is not touched.
    run(&mut s, "text.insert_field", json!({"id": id, "category": "page", "field": "page_number", "at": 0}));
    run(&mut s, "text.insert", json!({"id": id, "text": ">", "at": 0}));
    assert_eq!(view(&s, id).1.len(), 1);
    // One step each, and nothing to put is no step.
    let steps = s.history().unwrap().undo_len();
    run(&mut s, "text.insert", json!({"id": id, "text": ""}));
    assert_eq!(s.history().unwrap().undo_len(), steps);
    run(&mut s, "text.insert", json!({"id": id, "text": "!"}));
    assert_eq!(s.history().unwrap().undo_len(), steps + 1);
}

#[test]
fn text_insert_is_refused_in_words_and_changes_nothing() {
    let mut s = blank_session();
    let id = labelled(&mut s, "ab");
    let before = snapshot(&s);
    for (params, words_have) in [
        (json!({"id": id, "text": "x", "at": 9}), "9"),
        (json!({"id": id, "text": "a\u{1b}b"}), "control"),
        (json!({"id": id, "text": "\u{fffe}"}), "U+FFFE"),
        (json!({"id": id, "text": "x".repeat(100_001)}), "100000"),
        (json!({"id": 999, "text": "x"}), "999"),
        (json!({"id": id}), "text"),
        (json!({"id": id, "text": 5}), "text"),
        (json!({"id": id, "text": "x", "at": "1"}), "at"),
    ] {
        let words = refused(&mut s, "text.insert", params.clone());
        assert!(words.contains(words_have), "{params}: {words}");
    }
    assert_eq!(snapshot(&s), before);
    let _ = bad_params_msg;
}

// ---- the cost of working fields out (the review's third round, third finding) -------------------------------------------------------------------------------------------------

/// A shape holding `count` fields, numbered from 0, each showing `stale`; with `rows`, each has a `Field` row whose `Value` is the formula given (and the value 1) and whose `Format` is the picture
/// given (none when it is empty).
fn field_shape(id: u32, count: u32, stale: &str, rows: Option<(&str, &str)>) -> Shape {
    let shown: Arc<str> = Arc::from(stale);
    let mut parts = vec![diacraft_doc::TextPart::Chars(Arc::from("x"))];
    let mut sheet = diacraft_doc::Sheet::default();
    let mut section = Section::new("Field", None);
    for n in 0..count {
        parts.push(diacraft_doc::TextPart::Fld { ix: n, text: Arc::clone(&shown) });
        if let Some((formula, picture)) = rows {
            let mut row = Row { ix: Some(n), ..Row::default() };
            row.cells.insert("Value".into(), Cell::with_formula(formula, Some(Value::from_text("1"))));
            if !picture.is_empty() {
                row.cells.insert("Format".into(), Cell::constant(Value::from_text(picture)));
            }
            section.rows.push(Arc::new(row));
        }
    }
    if rows.is_some() {
        sheet.sections.push(Arc::new(section));
    }
    Shape { sheet: Arc::new(sheet), text: Some(Arc::new(diacraft_doc::Text { parts, ..Default::default() })), ..Shape::new(id) }
}

fn put_shapes(s: &mut Session, shapes: Vec<Shape>) {
    s.edit("test: shapes with fields", |d| {
        let page = Arc::make_mut(d.pages.get_mut(0).unwrap());
        for shape in shapes {
            page.shapes.push(Arc::new(shape));
        }
        Ok(())
    })
    .unwrap();
}

/// The work the model and the calculation did since the counts were last reset, as (sections looked at, rows merged, tables of rows built, steps of the evaluator).
fn work_done() -> (u64, u64, u64, u64) {
    (diacraft_doc::work::sections_examined(), diacraft_doc::work::rows_merged(), diacraft_doc::work::row_tables_built(), diacraft_calc::work::steps())
}

#[test]
fn one_shape_of_a_hundred_thousand_date_fields_is_skipped_and_counted_and_costs_nothing_to_leave() {
    let mut s = blank_session();
    // A twin with the same rows and no text: what moving it costs the model and the calculation is not the fields' to answer for.
    let mut twin = field_shape(6, 100_000, "0", Some(("NOW()", "yyyy")));
    twin.text = None;
    put_shapes(&mut s, vec![field_shape(5, 100_000, "0", Some(("NOW()", "yyyy"))), twin]);
    diacraft_doc::work::reset();
    diacraft_calc::work::reset();
    let done = run(&mut s, "text.update_fields", json!({"id": 5}));
    assert_eq!((done["updated"].clone(), done["examined"].clone(), done["skipped"].clone()), (json!(0), json!(0), json!(100_000)), "{done}");
    let (sections, merged, tables, steps) = work_done();
    assert!(
        sections + merged + tables + steps < 10_000,
        "a shape past the most fields is left at the cost of counting them: {:?}",
        (sections, merged, tables, steps)
    );
    // An edit that changes the shape works none of them out either: it costs what it costs the twin, to within a few thousand.
    diacraft_doc::work::reset();
    diacraft_calc::work::reset();
    run(&mut s, "shape.move", json!({"ids": [6], "dx": 1.0, "dy": 0.0}));
    let twin_work = work_done();
    diacraft_doc::work::reset();
    diacraft_calc::work::reset();
    run(&mut s, "shape.move", json!({"ids": [5], "dx": 1.0, "dy": 0.0}));
    let work = work_done();
    assert!(
        work.0 <= twin_work.0 + 5_000 && work.1 <= twin_work.1 + 5_000 && work.2 <= twin_work.2 + 5 && work.3 <= twin_work.3 + 5_000,
        "{work:?} against the twin's {twin_work:?}"
    );
    check_session(&s).unwrap();
}

#[test]
fn ten_thousand_shapes_of_a_hundred_fields_are_refused_in_words_by_an_update_and_a_page_edit_works_a_bounded_share_out() {
    let mut s = blank_session();
    put_shapes(&mut s, (0..10_000u32).map(|n| field_shape(100 + n, 100, "0", None)).collect());
    diacraft_doc::work::reset();
    diacraft_calc::work::reset();
    let words = refused(&mut s, "text.update_fields", json!({}));
    assert!(words.contains("100000") && words.contains("fields"), "{words}");
    let (sections, merged, tables, steps) = work_done();
    assert!(sections + merged + tables + steps < 10_000, "the refusal is made from the count: {:?}", (sections, merged, tables, steps));
    // A page edit works out the fields of the first shapes only, within the cap an edit has, and leaves the rest as they were.
    diacraft_doc::work::reset();
    diacraft_calc::work::reset();
    run(&mut s, "page.rename", json!({"id": 1, "name": "Renamed"}));
    let (sections, merged, _, steps) = work_done();
    assert!(sections + merged < 200_000 && steps < 1_000_000, "{:?}", (sections, merged, steps));
    check_session(&s).unwrap();
}

#[test]
fn a_thousand_shapes_of_a_hundred_fields_are_all_worked_out_in_work_that_follows_their_number() {
    let mut s = blank_session();
    put_shapes(&mut s, (0..1_000u32).map(|n| field_shape(100 + n, 100, "0", Some(("1", "")))).collect());
    diacraft_doc::work::reset();
    diacraft_calc::work::reset();
    let done = run(&mut s, "text.update_fields", json!({}));
    assert_eq!((done["updated"].clone(), done["examined"].clone(), done["skipped"].clone()), (json!(100_000), json!(100_000), json!(0)), "{done}");
    let (sections, merged, tables, steps) = work_done();
    // Each shape's rows are indexed once (a table of its 100 rows), and no field looks for its row, or for its text, by a scan of the others.
    assert!(tables <= 2_000, "tables of rows built: {tables}");
    assert!(sections <= 100_000 * 8, "sections looked at: {sections} for 100,000 fields");
    assert!(merged <= 100_000 * 4, "rows merged: {merged} for 100,000 fields");
    assert!(steps <= 100_000 * 40, "steps of the evaluator: {steps} for 100,000 fields");
    for id in [100u32, 599, 1_099] {
        assert!(shown(&s, id).ends_with(&"1".repeat(100)), "shape {id} shows what its rows give");
    }
    // Nothing is left to do.
    let again = run(&mut s, "text.update_fields", json!({}));
    assert_eq!(again["updated"], 0);
    check_session(&s).unwrap();
}

// ---- a fld with no row (an opened file's own text) ----------------------------------------------------------------------------------------------------------------------------

#[test]
fn a_fld_that_has_no_row_keeps_the_text_it_was_saved_with_through_an_edit_and_an_update() {
    let mut s = blank_session();
    // Two fields the file saved with a text and no `Field` rows: nothing says how to work them out.
    put_shapes(&mut s, vec![field_shape(5, 2, "3", None)]);
    assert_eq!(shown(&s, 5), "x33");
    let done = run(&mut s, "text.update_fields", json!({"id": 5}));
    assert_eq!((done["updated"].clone(), done["examined"].clone()), (json!(0), json!(0)), "{done}");
    run(&mut s, "shape.move", json!({"ids": [5], "dx": 1.0, "dy": 0.0}));
    run(&mut s, "text.insert", json!({"id": 5, "text": "!", "at": 0}));
    assert_eq!(shown(&s, 5), "!x33", "the text typed is in and the fields show what the file held");
    check_session(&s).unwrap();
}
