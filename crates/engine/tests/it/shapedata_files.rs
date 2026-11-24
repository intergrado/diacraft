//! The Shape Data of a shape **saved and opened again in the three formats** (`.diacraft`, VSDX and `.odg`): from a new drawing (the model writers) and from a drawing read from a file
//! and edited, saved in its own format (the splice). A VSDX file holds the `Property` section as it is; an `.odg` holds it in DiaCraft's own data on the shape (LibreOffice has no shape data, and
//! the save note says so for a file that is saved in place); `.diacraft` holds the whole model.

use serde_json::{Value as Json, json};

use diacraft_engine::Session;

use crate::odg_build::Dir;
use crate::{blank_session, run};

/// Opens the file `path` in a session of its own.
fn reopened(path: &str) -> Session {
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": path }));
    s
}

fn first_page(s: &Session) -> u32 {
    s.document().unwrap().pages.iter().find(|p| !p.background).unwrap().id
}

/// The ids of the shapes of the first page that is not a background, in drawing order.
fn ids(s: &Session) -> Vec<u32> {
    let page = s.document().unwrap().pages.iter().find(|p| !p.background).unwrap();
    page.shapes.iter().map(|sh| sh.id).collect()
}

/// Every field of shape `id`, hidden ones too, as `shapedata.list` gives them.
fn listed(s: &mut Session, id: u32) -> Json {
    let page = first_page(s);
    let answer = run(s, "shapedata.list", json!({"id": id, "page": page, "hidden": true}));
    answer["fields"].clone()
}

/// The fields of the `n`th shape of the page.
fn fields_at(s: &mut Session, n: usize) -> Json {
    let id = ids(s)[n];
    listed(s, id)
}

/// The fields a person defines on a shape: one of each type, a hidden one, one that asks on drop, and prompts, sort keys and a language and a calendar.
fn define_all(s: &mut Session, id: u32, page: u32) {
    run(
        s,
        "shapedata.define",
        json!({"id": id, "page": page, "fields": [
            {"label": "Owner", "name": "Owner", "type": "string", "value": "Ann Lee", "prompt": "Who owns it", "sort_key": "2"},
            {"label": "Status", "name": "Status", "type": "fixed_list", "format": "Open;Closed;Held", "value": "Closed", "sort_key": "1", "ask": true},
            {"label": "Qty", "name": "Qty", "type": "number", "format": "0.00", "value": "12.5"},
            {"label": "Billable", "name": "Billable", "type": "boolean", "value": "yes"},
            {"label": "Tag", "name": "Tag", "type": "variable_list", "format": "Red;Green", "value": "Blue"},
            {"label": "Due", "name": "Due", "type": "date", "value": "2026-03-04 14:30", "lang_id": "en-US", "calendar": 1},
            {"label": "Span", "name": "Span", "type": "duration", "value": "36 hours"},
            {"label": "Cost", "name": "Cost", "type": "currency", "value": "$1,234.50", "invisible": true},
        ]}),
    );
}

/// A drawing of two shapes, the first with the fields.
fn made() -> Session {
    let mut s = blank_session();
    let a = run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 2.0, "y": 3.0}))["id"].as_u64().unwrap() as u32;
    run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 6.0, "y": 3.0}));
    define_all(&mut s, a, 1);
    s
}

#[test]
fn fields_come_back_from_a_new_drawing_saved_in_each_format() {
    for ext in ["diacraft", "vsdx", "odg"] {
        let mut s = made();
        let want = fields_at(&mut s, 0);
        assert_eq!(want.as_array().unwrap().len(), 8, "{ext}");
        let dir = Dir::new();
        let path = dir.path(&format!("drawing.{ext}"));
        run(&mut s, "file.save_as", json!({ "path": path }));
        let mut back = reopened(&path);
        let got = fields_at(&mut back, 0);
        assert_eq!(got, want, "{ext}: the fields, their values, formats, prompts, order and flags");
        // The second shape has none.
        assert_eq!(fields_at(&mut back, 1), json!([]), "{ext}");
        // What a field shows is the value through its format, after the reopen as before.
        let page = first_page(&back);
        let first_id = ids(&back)[0];
        let shown: Vec<String> = run(&mut back, "shapedata.list", json!({"id": first_id, "page": page}))["fields"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["shown"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(
            shown,
            ["Closed", "Ann Lee", "12.50", "TRUE", "Blue", "3/4/2026 2:30 PM", "36 h"],
            "{ext}: the visible fields in sort key order, the hidden one left out"
        );
    }
}

#[test]
fn a_value_entered_in_a_drawing_read_from_a_file_is_kept_in_its_own_format() {
    for ext in ["diacraft", "vsdx", "odg"] {
        let mut plain = blank_session();
        run(&mut plain, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 2.0, "y": 3.0}));
        let dir = Dir::new();
        let source = dir.path(&format!("source.{ext}"));
        run(&mut plain, "file.save_as", json!({ "path": source }));
        let mut s = reopened(&source);
        let id = ids(&s)[0];
        let page = first_page(&s);
        define_all(&mut s, id, page);
        run(
            &mut s,
            "shapedata.set",
            json!({"id": id, "page": page, "values": [{"row": "Owner", "value": "Bob"}, {"row": "Qty", "value": 3}, {"row": "Tag", "value": "Violet"}]}),
        );
        let want = listed(&mut s, id);
        let saved = run(&mut s, "file.save", json!({}));
        let mut back = reopened(&source);
        let got = fields_at(&mut back, 0);
        assert_eq!(got, want, "{ext}: the splice keeps the fields and the values entered");
        assert_eq!(
            got.as_array().unwrap().iter().find(|f| f["row"] == "Tag").unwrap()["format"],
            "Red;Green;Blue;Violet",
            "{ext}: the variable list's new items are kept"
        );
        if ext == "odg" {
            // LibreOffice has no shape data: the save says what it would not show, in words.
            let notes = saved["notes"].as_array().cloned().unwrap_or_default();
            assert!(notes.iter().any(|n| n["words"].as_str().is_some_and(|w| w.contains("shape with data rows"))), "{saved}");
        }
    }
}

#[test]
fn a_drawing_carried_across_formats_keeps_its_fields() {
    for (from, to) in [("vsdx", "odg"), ("odg", "vsdx"), ("diacraft", "vsdx"), ("vsdx", "diacraft"), ("odg", "diacraft")] {
        let mut s = made();
        let dir = Dir::new();
        let first_path = dir.path(&format!("one.{from}"));
        run(&mut s, "file.save_as", json!({ "path": first_path }));
        let mut one = reopened(&first_path);
        let want = fields_at(&mut one, 0);
        let second_path = dir.path(&format!("two.{to}"));
        run(&mut one, "file.save_as", json!({ "path": second_path }));
        let mut two = reopened(&second_path);
        assert_eq!(fields_at(&mut two, 0), want, "{from} to {to}");
    }
}

#[test]
fn a_field_taken_away_or_a_value_entered_is_one_undo_step_in_a_drawing_read_from_a_file() {
    let mut plain = blank_session();
    run(&mut plain, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 2.0, "y": 3.0}));
    let dir = Dir::new();
    let source = dir.path("source.vsdx");
    run(&mut plain, "file.save_as", json!({ "path": source }));
    let mut s = reopened(&source);
    let id = ids(&s)[0];
    let page = first_page(&s);
    define_all(&mut s, id, page);
    run(&mut s, "file.save", json!({}));
    let before = listed(&mut s, id);
    run(&mut s, "shapedata.define", json!({"id": id, "page": page, "fields": [{"row": "Owner"}]}));
    assert_eq!(listed(&mut s, id).as_array().unwrap().len(), 1);
    run(&mut s, "file.save", json!({}));
    assert_eq!(listed(&mut reopened(&source), id).as_array().unwrap().len(), 1, "the rows left out are taken from the file");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(listed(&mut s, id), before);
}

/// A drawing with fields that is read from a `.diacraft` file and saved as a VSDX type or an `.odg` is **read back and compared**, and Shape Data is in what is compared (a `Property` section
/// that went missing is the class "user cells", `check/tests.rs`): a trip that keeps every field says nothing.
#[test]
fn a_drawing_with_fields_saved_across_formats_is_checked_and_loses_none() {
    let mut s = made();
    let dir = Dir::new();
    let native = dir.path("fields.diacraft");
    run(&mut s, "file.save_as", json!({ "path": native }));
    let mut from_native = reopened(&native);
    for ext in ["vsdx", "odg"] {
        let saved = run(&mut from_native, "file.save_as", json!({ "path": dir.path(&format!("across.{ext}")) }));
        // An `.odg` holds a length or a colour in other forms, which the check names once as "values" and never counts: no class differs, and the check was made.
        assert_eq!((saved["checked"]["done"].as_bool(), saved["checked"]["classes"].clone()), (Some(true), json!([])), "{ext}: {saved}");
    }
}
