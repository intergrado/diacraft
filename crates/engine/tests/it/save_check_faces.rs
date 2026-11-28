//! DiaCraft's own drawings said "1 font is missing" after a Save As `.odg`. The face was `Arial`, the one row the VSDX writer puts in the font table of a drawing whose table is empty
//! (MS-VSDX asks for a row, and nothing names it): a drawing made from a built-in template holds it (the template is made as the bytes of a `.vstx` and read back), and so does every VSDX file DiaCraft
//! writes from a new drawing. An `.odg` declares the faces its texts use, and DiaCraft's document file inside it carries the faces that hold more than a name, so the face was in no part of the file,
//! and the check after the save counted it. **A face that is its name and nothing more is looked for in the file only where a cell names it** (the check's rule since this task): such a row says
//! nothing a cell does not, and a face a text uses, or one that holds more than its name, is still a missing font when the file lacks it.
//!
//! Red on `d2605d1c`: each built-in template saved as `.odg`, a VSDX file DiaCraft wrote reopened and saved as `.odg`, and a drawing with a text in a font of its own, said "1 font is missing".

use std::sync::Arc;

use diacraft_engine::check;
use diacraft_testkit::package::{PageSpec, VsdxBuilder};
use serde_json::{Value, json};

use crate::native_common::{Dir, opened, pinned};
use crate::run;

/// The words of the classes a save's `checked` field names, or `null` for a save that was not checked.
fn classes(said: &Value) -> Value {
    match said["checked"]["classes"].as_array() {
        Some(list) => Value::Array(list.iter().map(|c| c["words"].clone()).collect()),
        None => Value::Null,
    }
}

/// Whether a save's `checked` field names the fonts class.
fn names_fonts(said: &Value) -> bool {
    said["checked"]["classes"].as_array().is_some_and(|list| list.iter().any(|c| c["class"] == "fonts"))
}

/// A session with a drawing started from `template` (`None`: a blank drawing), as File > New makes it.
fn started(template: Option<&str>) -> diacraft_engine::Session {
    let mut s = pinned();
    if let Some(id) = template {
        run(&mut s, "file.new", json!({ "template": id }));
    }
    s
}

/// **No drawing made new, blank or from a built-in template, names a font after a save in any format**: `.odg`, a VSDX type and `.diacraft`. Each failure is collected before the test asserts, so
/// the message lists every template and format that named one.
#[test]
fn a_new_drawing_from_any_built_in_template_saved_in_any_format_names_no_font() {
    let dir = Dir::new();
    let mut ids: Vec<Option<&str>> = vec![None];
    ids.extend(diacraft_engine::template::templates().iter().map(|t| Some(t.id)));
    assert!(ids.len() >= 14, "the blank drawing and the shipped templates: {ids:?}");
    let mut failures = Vec::new();
    for id in &ids {
        for ext in ["odg", "vsdx", "diacraft"] {
            let mut s = started(*id);
            let name = format!("{}.{ext}", id.unwrap_or("blank"));
            let said = run(&mut s, "file.save_as", json!({ "path": dir.path(&name) }));
            if names_fonts(&said) {
                failures.push(format!("{name}: {}", classes(&said)));
            }
        }
    }
    assert!(failures.is_empty(), "these saves name a font: {failures:#?}");
}

/// **A VSDX file DiaCraft wrote, opened and saved as `.odg`, names no font** (the demo's `office-network.vsdx`, made from Basic Network, converted to `.odg`): its table holds the one row of a new
/// drawing, which nothing names.
#[test]
fn a_vsdx_file_diacraft_wrote_names_no_font_when_saved_as_an_odg() {
    let dir = Dir::new();
    for (template, name) in [(None, "blank"), (Some("basic_network"), "network")] {
        let mut s = started(template);
        run(&mut s, "file.save_as", json!({ "path": dir.path(&format!("{name}.vsdx")) }));
        let mut reopened = opened(&dir.path(&format!("{name}.vsdx")));
        let faces: Vec<String> = reopened.document().unwrap().fonts.iter().map(|f| f.name_u.to_string()).collect();
        assert_eq!(faces, ["Arial"], "{name}: the table of a VSDX file DiaCraft wrote from a new drawing");
        let said = run(&mut reopened, "file.save_as", json!({ "path": dir.path(&format!("{name}.odg")) }));
        assert_eq!(said["checked"]["done"], json!(true), "{name}: {said}");
        assert!(!names_fonts(&said), "{name}: {}", classes(&said));
    }
}

/// A drawing started from Basic Flowchart with a Process shape whose text is in `face`.
fn with_a_text_in(face: &str) -> diacraft_engine::Session {
    let mut s = started(Some("basic_flowchart"));
    run(&mut s, "shape.drop", json!({ "master": "process", "page": 1, "x": 3.0, "y": 4.0 }));
    run(&mut s, "shape.set_text", json!({ "id": 1, "text": "Hello" }));
    run(&mut s, "text.format", json!({ "ids": [1], "font": face }));
    s
}

/// **The face a text uses comes back in the `.odg`, and the row nothing names is no loss**: a template's drawing with a text in a face of its own holds that face and the template's row; the file
/// declares the face of the text, the reopened drawing's text is in it, and the save names no font.
#[test]
fn the_face_a_text_uses_comes_back_and_the_row_nothing_names_is_no_loss() {
    let dir = Dir::new();
    let mut s = with_a_text_in("Liberation Serif");
    let faces: Vec<String> = s.document().unwrap().fonts.iter().map(|f| f.name_u.to_string()).collect();
    assert_eq!(faces, ["Arial", "Liberation Serif"], "the template's row and the face the text was given");
    let said = run(&mut s, "file.save_as", json!({ "path": dir.path("text.odg") }));
    assert_eq!(said["checked"]["done"], json!(true), "{said}");
    assert!(!names_fonts(&said), "{}", classes(&said));
    let content = crate::odg_build::part_of(&dir.read("text.odg"), "content.xml");
    assert!(content.contains("svg:font-family=\"'Liberation Serif'\""), "the file declares the face of the text: {content}");
    let reopened = opened(&dir.path("text.odg"));
    let faces: Vec<String> = reopened.document().unwrap().fonts.iter().map(|f| f.name_u.to_string()).collect();
    assert!(faces.iter().any(|f| f == "Liberation Serif"), "{faces:?}");
}

/// **Control: a drawing that loses a face it uses still names a missing font**, in the class's words: the face of the text taken out of the drawing the file reads back as.
#[test]
fn a_face_a_text_uses_that_the_file_lacks_is_still_a_missing_font() {
    let dir = Dir::new();
    let mut s = with_a_text_in("Liberation Serif");
    let lose = |doc: &mut diacraft_doc::Document| Arc::make_mut(&mut doc.fonts).retain(|f| &*f.name_u != "Liberation Serif");
    let said = check::with_tamper(lose, || run(&mut s, "file.save_as", json!({ "path": dir.path("lost.odg") })));
    assert_eq!(said["checked"]["classes"], json!([{ "class": "fonts", "words": "1 font is missing", "count": 1 }]), "{said}");
}

/// **Control: a face that holds more than its name is a missing font when the file lacks it, though nothing names it**: a VSDX file's face with the attributes R13 lists rides inside the `.odg`
/// and comes back (no finding), and the same save with the face taken out of what the file reads back as names it.
#[test]
fn a_face_with_attributes_that_the_file_lacks_is_a_missing_font_though_nothing_names_it() {
    let dir = Dir::new();
    let bytes = VsdxBuilder::new()
        .face_names(r#"<FaceName NameU="Noto Sans" UnicodeRanges="1 2 3 4" CharSets="1 0" Panose="2 11 5 2 4 5 4 2 2 4" Flags="325"/>"#)
        .page(PageSpec::new(1, "Page-1").sheet(r#"<Cell N="PageWidth" V="8.5"/><Cell N="PageHeight" V="11"/>"#))
        .build();
    let path = dir.write("faces.vsdx", &bytes);
    let mut s = opened(&path);
    let said = run(&mut s, "file.save_as", json!({ "path": dir.path("kept.odg") }));
    assert_eq!(said["checked"]["done"], json!(true), "{said}");
    assert!(!names_fonts(&said), "the face rides and comes back: {}", classes(&said));
    let mut s = opened(&path);
    let lose = |doc: &mut diacraft_doc::Document| Arc::make_mut(&mut doc.fonts).clear();
    let said = check::with_tamper(lose, || run(&mut s, "file.save_as", json!({ "path": dir.path("lost.odg") })));
    assert_eq!(said["checked"]["classes"], json!([{ "class": "fonts", "words": "1 font is missing", "count": 1 }]), "{said}");
}
