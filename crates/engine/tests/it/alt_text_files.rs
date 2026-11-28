//! Alt text **saved and opened again in the three formats** (`.diacraft`, VSDX and `.odg`): from a new drawing (the model writers) and from a drawing read from a file and edited, saved in
//! its own format (the splice). A VSDX file and a `.diacraft` file hold the two rows of the `User` section as they are; an `.odg` file holds them as `svg:title` and
//! `svg:desc`, which `alt_text_odg.rs` looks into.

use std::sync::Arc;

use diacraft_doc::{Cell, Master, Row, RowKey, Section, SectionKey, Shape, ShapeList, Value};
use diacraft_engine::Session;
use serde_json::{Value as Json, json};

use crate::edit::{doc, drop_shape};
use crate::hyperlinks::edit_shape;
use crate::odg_build::Dir;
use crate::{blank_session, run};

/// Opens the file `path` in a session of its own.
pub(crate) fn reopened(path: &str) -> Session {
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": path }));
    s
}

pub(crate) fn first_page(s: &Session) -> u32 {
    s.document().unwrap().pages.iter().find(|p| !p.background).unwrap().id
}

/// The ids of the top-level shapes of the first page that is not a background, in drawing order.
pub(crate) fn ids(s: &Session) -> Vec<u32> {
    let page = s.document().unwrap().pages.iter().find(|p| !p.background).unwrap();
    page.shapes.iter().map(|sh| sh.id).collect()
}

/// The title and the description of the `n`th shape of the page, as the command answers them.
pub(crate) fn texts_at(s: &mut Session, n: usize) -> (String, String) {
    let id = ids(s)[n];
    let page = first_page(s);
    let a = run(s, "shape.alt_text", json!({"id": id, "page": page}));
    (a["title"].as_str().unwrap().to_owned(), a["description"].as_str().unwrap().to_owned())
}

pub(crate) fn page_texts(s: &mut Session) -> (String, String) {
    let page = first_page(s);
    let a = run(s, "page.alt_text", json!({"page": page}));
    (a["title"].as_str().unwrap().to_owned(), a["description"].as_str().unwrap().to_owned())
}

/// A drawing of four shapes: the first with a title and a description, the second with the white space and the line breaks of a text kept as it is, the third with a title only, the fourth with
/// none; and a title and a description on the page.
pub(crate) fn made() -> Session {
    let mut s = blank_session();
    for (i, x) in [2.0, 4.0, 6.0, 8.0].into_iter().enumerate() {
        let id = drop_shape(&mut s, "rectangle", x, 3.0 + i as f64 * 0.01);
        let _ = id;
    }
    let all = ids(&s);
    run(&mut s, "shape.alt_text", json!({"id": all[0], "title": "Pump P-101", "description": "Feeds the boiler"}));
    run(&mut s, "shape.alt_text", json!({"id": all[1], "title": "  Pump  ", "description": "line one\nline two & <more>\ttabbed "}));
    run(&mut s, "shape.alt_text", json!({"id": all[2], "title": "Only a title"}));
    run(&mut s, "page.alt_text", json!({"title": "Plant overview", "description": "All the units"}));
    s
}

fn expected() -> Vec<(String, String)> {
    vec![
        ("Pump P-101".to_owned(), "Feeds the boiler".to_owned()),
        ("  Pump  ".to_owned(), "line one\nline two & <more>\ttabbed ".to_owned()),
        ("Only a title".to_owned(), String::new()),
        (String::new(), String::new()),
    ]
}

fn all_texts(s: &mut Session) -> Vec<(String, String)> {
    (0..4).map(|n| texts_at(s, n)).collect()
}

#[test]
fn alt_text_comes_back_from_a_new_drawing_saved_in_each_format() {
    for ext in ["diacraft", "vsdx", "odg"] {
        let mut s = made();
        let dir = Dir::new();
        let path = dir.path(&format!("drawing.{ext}"));
        run(&mut s, "file.save_as", json!({ "path": path }));
        let mut back = reopened(&path);
        assert_eq!(all_texts(&mut back), expected(), "{ext}: the texts of the shapes, character for character");
        assert_eq!(page_texts(&mut back), ("Plant overview".to_owned(), "All the units".to_owned()), "{ext}: the page's");
        // The check reads the same drawing the same way.
        let before = run(&mut s, "review.check_accessibility", json!({}))["findings"].as_array().unwrap().len();
        let after = run(&mut back, "review.check_accessibility", json!({}))["findings"].as_array().unwrap().len();
        assert_eq!(before, after, "{ext}: the same findings");
    }
}

/// Saves `made()` as `ext`, opens it and edits it, and saves it in place: what the splice writes.
fn edited_in_place(ext: &str) -> (Dir, String) {
    let mut s = made();
    let dir = Dir::new();
    let path = dir.path(&format!("drawing.{ext}"));
    run(&mut s, "file.save_as", json!({ "path": path }));
    let mut open = reopened(&path);
    let all = ids(&open);
    // The fourth shape gets texts, the first loses its description and the second its title; the page is retitled.
    run(&mut open, "shape.alt_text", json!({"id": all[3], "title": "Valve V-7", "description": "Gate valve"}));
    run(&mut open, "shape.alt_text", json!({"id": all[0], "description": ""}));
    run(&mut open, "shape.alt_text", json!({"id": all[1], "title": ""}));
    run(&mut open, "page.alt_text", json!({"title": "Plant, east wing"}));
    run(&mut open, "file.save", json!({}));
    (dir, path)
}

#[test]
fn alt_text_edited_in_a_file_is_saved_in_place_in_each_format() {
    for ext in ["diacraft", "vsdx", "odg"] {
        let (_dir, path) = edited_in_place(ext);
        let mut back = reopened(&path);
        let got = all_texts(&mut back);
        assert_eq!(got[0], ("Pump P-101".to_owned(), String::new()), "{ext}");
        assert_eq!(got[1], (String::new(), "line one\nline two & <more>\ttabbed ".to_owned()), "{ext}");
        assert_eq!(got[2], ("Only a title".to_owned(), String::new()), "{ext}");
        assert_eq!(got[3], ("Valve V-7".to_owned(), "Gate valve".to_owned()), "{ext}");
        assert_eq!(page_texts(&mut back), ("Plant, east wing".to_owned(), "All the units".to_owned()), "{ext}");
    }
}

#[test]
fn a_save_with_no_edit_keeps_the_alt_text_and_the_file() {
    for ext in ["diacraft", "vsdx", "odg"] {
        let mut s = made();
        let dir = Dir::new();
        let path = dir.path(&format!("drawing.{ext}"));
        run(&mut s, "file.save_as", json!({ "path": path }));
        let first = std::fs::read(&path).unwrap();
        let mut open = reopened(&path);
        run(&mut open, "file.save", json!({}));
        let second = std::fs::read(&path).unwrap();
        // A save with no edit is the file as it was, in the formats that splice; `.diacraft` writes the model again.
        if ext != "diacraft" {
            assert_eq!(first, second, "{ext}: an unedited save keeps every byte");
        }
        let mut back = reopened(&path);
        assert_eq!(all_texts(&mut back), expected(), "{ext}");
    }
}

#[test]
fn a_texts_of_a_file_longer_than_the_boxes_are_kept_by_a_save_of_the_other_text() {
    for ext in ["diacraft", "vsdx", "odg"] {
        let mut s = blank_session();
        let id = drop_shape(&mut s, "rectangle", 3.0, 3.0);
        let long = "d".repeat(300);
        run(&mut s, "shape.alt_text", json!({"id": id, "description": long}));
        let dir = Dir::new();
        let path = dir.path(&format!("drawing.{ext}"));
        run(&mut s, "file.save_as", json!({ "path": path }));
        let mut open = reopened(&path);
        let id = ids(&open)[0];
        run(&mut open, "shape.alt_text", json!({"id": id, "title": "Short"}));
        run(&mut open, "file.save", json!({}));
        let mut back = reopened(&path);
        assert_eq!(texts_at(&mut back, 0), ("Short".to_owned(), long), "{ext}: E4, 300 characters saved unchanged after the title is edited");
    }
}

/// A master with a title and a description on its shape, and an instance of it that inherits both.
pub(crate) fn with_master_and_instance() -> Session {
    let mut s = blank_session();
    let mut shape = Shape::new(1);
    let mut user = Section::new("User", None);
    for (name, text) in [("visAltText", "Valve"), ("visAltDescription", "A gate valve")] {
        let mut row = Row { name: Some(Arc::from(name)), ..Row::default() };
        row.cells.insert("Value".into(), Cell::constant(Value::from_text(text)).with_unit("STR"));
        row.cells.insert("Prompt".into(), Cell::constant(Value::from_text("")));
        user.rows.push(Arc::new(row));
    }
    Arc::make_mut(&mut shape.sheet).sections.push(Arc::new(user));
    s.edit("test: a master", |d| {
        d.add_master(Master { id: 5, shapes: ShapeList::from_vec(vec![Arc::new(shape)]), ..Master::default() }).unwrap();
        Ok(())
    })
    .unwrap();
    let id = drop_shape(&mut s, "rectangle", 3.0, 3.0);
    edit_shape(&mut s, id, |sh| {
        sh.master = Some(5);
        Arc::make_mut(&mut sh.sheet).sections.retain(|sec| sec.name.as_str() != "User");
    });
    s
}

#[test]
fn an_instances_inherited_text_is_the_text_of_its_shape_in_every_format() {
    for ext in ["diacraft", "vsdx", "odg"] {
        let mut s = with_master_and_instance();
        assert_eq!(texts_at(&mut s, 0), ("Valve".to_owned(), "A gate valve".to_owned()));
        let dir = Dir::new();
        let path = dir.path(&format!("drawing.{ext}"));
        run(&mut s, "file.save_as", json!({ "path": path }));
        let mut back = reopened(&path);
        assert_eq!(texts_at(&mut back, 0), ("Valve".to_owned(), "A gate valve".to_owned()), "{ext}");
        let d = doc(&back);
        let at = d.find_shape(diacraft_doc::Owner::Page(first_page(&back)), ids(&back)[0]).unwrap();
        let shape = d.shape(&at).unwrap();
        if ext != "odg" {
            // These two formats keep masters: the instance still holds none of the rows, and its master still has them.
            assert!(shape.sheet.section(&SectionKey::new("User", None)).is_none(), "{ext}: an instance does not copy its master's rows");
            assert_eq!(shape.master, Some(5), "{ext}");
        }
    }
}

#[test]
fn a_one_row_alone_and_the_row_form_survive_a_visit_through_each_format() {
    // A title row alone (the usual form of the files: 480 of 564 sheets), and a description row alone.
    for ext in ["diacraft", "vsdx", "odg"] {
        let mut s = blank_session();
        let a = drop_shape(&mut s, "rectangle", 2.0, 3.0);
        let b = drop_shape(&mut s, "rectangle", 5.0, 3.0);
        run(&mut s, "shape.alt_text", json!({"id": a, "title": "T only"}));
        run(&mut s, "shape.alt_text", json!({"id": b, "description": "D only"}));
        let dir = Dir::new();
        let path = dir.path(&format!("drawing.{ext}"));
        run(&mut s, "file.save_as", json!({ "path": path }));
        let mut back = reopened(&path);
        let got: Vec<Json> = (0..2)
            .map(|n| {
                let id = ids(&back)[n];
                let page = first_page(&back);
                run(&mut back, "shape.alt_text", json!({"id": id, "page": page}))
            })
            .collect();
        assert_eq!((got[0]["title"].as_str(), got[0]["description"].as_str()), (Some("T only"), Some("")), "{ext}");
        assert_eq!((got[1]["title"].as_str(), got[1]["description"].as_str()), (Some(""), Some("D only")), "{ext}");
        let rows = |n: usize| -> Vec<String> {
            let d = doc(&back);
            let at = d.find_shape(diacraft_doc::Owner::Page(first_page(&back)), ids(&back)[n]).unwrap();
            d.shape(&at)
                .unwrap()
                .sheet
                .section(&SectionKey::new("User", None))
                .map(|sec| sec.rows.iter().filter_map(|r| r.name.as_deref().map(str::to_owned)).collect())
                .unwrap_or_default()
        };
        assert!(rows(0).contains(&"visAltText".to_owned()) && !rows(0).contains(&"visAltDescription".to_owned()), "{ext}: {:?}", rows(0));
        assert!(rows(1).contains(&"visAltDescription".to_owned()) && !rows(1).contains(&"visAltText".to_owned()), "{ext}: {:?}", rows(1));
        let _ = RowKey::name("x");
    }
}
