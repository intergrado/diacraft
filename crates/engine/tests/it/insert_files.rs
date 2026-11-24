//! Hyperlinks, fields and pictures **saved and opened again in the three formats** (`.diacraft`, VSDX and `.odg`): from a new drawing (the model writers) and from a drawing read
//! from a file and edited, saved in its own format (the splice). A VSDX file holds the `Hyperlink` and `Field` sections and a `Foreign` shape with its picture's part; an `.odg` holds what
//! its markup can say (`text:page-number`, `text:page-count`, `text:file-name` and `text:date` for the fields, a frame with a `draw:image` for a picture) and the rest in DiaCraft's own
//! data on the shape; `.diacraft` holds the whole model.

use std::sync::Arc;

use diacraft_doc::{Owner, RowKey, SectionKey, Shape, ShapeType};
use diacraft_engine::Session;
use serde_json::{Value as Json, json};

use crate::odg_build::Dir;
use crate::pictures::real_png;
use crate::{blank_session, run};

/// Opens the file `path` in a session of its own.
fn reopened(path: &str) -> Session {
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": path }));
    s
}

/// The shapes of the first page that is not a background, in drawing order.
fn shapes_of(s: &Session) -> Vec<Arc<Shape>> {
    let page = s.document().unwrap().pages.iter().find(|p| !p.background).unwrap();
    page.shapes.iter().cloned().collect()
}

fn page_id(s: &Session) -> u32 {
    s.document().unwrap().pages.iter().find(|p| !p.background).unwrap().id
}

/// A drawing of three shapes: a rectangle with two hyperlinks, a rectangle whose text is `Page ` and a page number field, `of ` and a page count field, and a picture.
fn made() -> (Session, Vec<u8>) {
    let mut s = blank_session();
    let a = run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 2.0, "y": 3.0}))["id"].as_u64().unwrap();
    run(
        &mut s,
        "hyperlink.set",
        json!({"id": a, "links": [
            {"description": "Site", "address": "https://example.org/start", "default": true},
            {"description": "Overview", "sub_address": "Page-1", "new_window": true}
        ]}),
    );
    let b = run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 5.0, "y": 3.0}))["id"].as_u64().unwrap();
    run(&mut s, "shape.set_text", json!({"id": b, "text": "Page  of "}));
    run(&mut s, "text.insert_field", json!({"id": b, "category": "page", "field": "page_number", "at": 5}));
    run(&mut s, "text.insert_field", json!({"id": b, "category": "page", "field": "page_count"}));
    let png = real_png(24, 12);
    let dir = Dir::new();
    let path = dir.path("logo.png");
    std::fs::write(&path, &png).unwrap();
    run(&mut s, "insert.picture", json!({"path": path, "x": 8.0, "y": 3.0}));
    (s, png)
}

/// What a drawing says of the three, as one value to compare: the links of the first, the text and the rows of the second, and the picture of the third.
fn state(s: &mut Session) -> Json {
    let shapes = shapes_of(s);
    let page = page_id(s);
    let links = run(s, "hyperlink.list", json!({"id": shapes[0].id, "page": page}))["links"].clone();
    let text = diacraft_engine::draw::text_view(s.document().unwrap(), page, shapes[1].id).unwrap();
    let fields: Vec<Json> = text.fields.iter().map(|f| json!({"at": f.at, "ix": f.ix, "shown": f.text})).collect();
    let rows: Vec<Json> = {
        let section = shapes[1].sheet.section(&SectionKey::new("Field", None));
        let cell = |ix: u32, name: &str| -> Json {
            section
                .and_then(|sec| sec.row(&RowKey::Ix(ix)).cloned())
                .and_then(|row| row.cells.get(name).map(|c| json!({"f": c.formula.text(), "v": c.value.as_ref().map(|v| v.text().to_owned())})))
                .unwrap_or(Json::Null)
        };
        text.fields.iter().map(|f| json!({"ix": f.ix, "value": cell(f.ix, "Value"), "type": cell(f.ix, "Type")})).collect()
    };
    let picture = &shapes[2];
    let foreign = picture.foreign.as_ref();
    json!({
        "links": links,
        "text": text.chars,
        "fields": fields,
        "rows": rows,
        "picture": {
            "kind": format!("{:?}", picture.kind),
            "foreign_type": foreign.and_then(|f| f.attr("ForeignType")),
            "bytes": foreign.and_then(|f| f.picture.as_ref()).map(|m| m.bytes.len()),
        },
    })
}

fn picture_bytes(s: &Session) -> Option<Vec<u8>> {
    shapes_of(s).last()?.foreign.as_ref()?.picture.as_ref().map(|m| m.bytes.to_vec())
}

#[test]
fn links_fields_and_a_picture_come_back_from_a_new_drawing_saved_in_each_format() {
    for ext in ["diacraft", "vsdx", "odg"] {
        let (mut s, png) = made();
        let want = state(&mut s);
        assert_eq!(want["text"], "Page  of ");
        assert_eq!(want["picture"]["kind"], "Foreign");
        let dir = Dir::new();
        let path = dir.path(&format!("drawing.{ext}"));
        run(&mut s, "file.save_as", json!({ "path": path }));
        let mut back = reopened(&path);
        let got = state(&mut back);
        assert_eq!(got["picture"], want["picture"], "{ext}: the picture's shape");
        assert_eq!(picture_bytes(&back), Some(png), "{ext}: the picture's bytes are the file's");
        assert_eq!(got["text"], want["text"], "{ext}: the characters");
        assert_eq!(got["fields"], want["fields"], "{ext}: the fields and what they show");
        assert_eq!(got["links"], want["links"], "{ext}: the hyperlinks");
        assert_eq!(got["rows"], want["rows"], "{ext}: the rows of the Field section");
    }
}

#[test]
fn a_drawing_read_from_a_file_and_given_a_link_a_field_and_a_picture_keeps_them_in_its_own_format() {
    for ext in ["diacraft", "vsdx", "odg"] {
        let mut plain = blank_session();
        run(&mut plain, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 2.0, "y": 3.0}));
        run(&mut plain, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 5.0, "y": 3.0}));
        let dir = Dir::new();
        let source = dir.path(&format!("source.{ext}"));
        run(&mut plain, "file.save_as", json!({ "path": source }));
        let mut s = reopened(&source);
        let ids: Vec<u32> = shapes_of(&s).iter().map(|sh| sh.id).collect();
        let page = page_id(&s);
        run(
            &mut s,
            "hyperlink.set",
            json!({"id": ids[0], "page": page, "links": [{"description": "Site", "address": "https://example.org/start", "default": true}]}),
        );
        run(&mut s, "shape.set_text", json!({"id": ids[1], "page": page, "text": "Page "}));
        run(&mut s, "text.insert_field", json!({"id": ids[1], "page": page, "category": "page", "field": "page_number"}));
        let png = real_png(30, 10);
        let file = dir.path("art.png");
        std::fs::write(&file, &png).unwrap();
        run(&mut s, "insert.picture", json!({"path": file, "page": page, "x": 8.0, "y": 3.0}));
        let want_links = run(&mut s, "hyperlink.list", json!({"id": ids[0], "page": page}))["links"].clone();
        // (the save into a VSDX file writes a picture new to a page the file has, as a new page's picture is written: a row of the page part to a part that holds it.
        // refused it, in words.)
        let saved = run(&mut s, "file.save", json!({}));
        if ext == "odg" {
            // An `.odg` edited in place holds a field only where the file had one: the new field is written as the text it shows, and the save says so.
            let said: Vec<&str> = saved["warnings"].as_array().unwrap().iter().filter_map(Json::as_str).collect();
            assert!(said.iter().any(|w| w.contains("1 new field written as the text it shows")), "{saved}");
        }
        let back = reopened(&source);
        let shapes = shapes_of(&back);
        assert_eq!(shapes.len(), 3, "{ext}: the picture is a shape");
        let mut back = back;
        let page = page_id(&back);
        assert_eq!(run(&mut back, "hyperlink.list", json!({"id": shapes[0].id, "page": page}))["links"], want_links, "{ext}: the link");
        let text = diacraft_engine::draw::text_view(back.document().unwrap(), page, shapes[1].id).unwrap();
        if ext == "odg" {
            assert_eq!((text.chars.as_str(), text.fields.len()), ("Page 1", 0), "odg: the field is its text");
        } else {
            assert_eq!(
                (text.chars.as_str(), text.fields.len(), text.fields.first().map(|f| f.text.as_str())),
                ("Page ", 1, Some("1")),
                "{ext}: the text and its field"
            );
        }
        assert_eq!(picture_bytes(&back), Some(png), "{ext}: the picture");
        assert_eq!(shapes[2].kind, ShapeType::Foreign, "{ext}");
    }
    let _ = Owner::Page(1);
}
