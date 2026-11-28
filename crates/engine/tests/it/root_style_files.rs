//! The root style of the VSDX files the engine writes: the reference application reads a paragraph with no `HorzAlign` on its way as left-aligned when the root style
//! holds none, where this program centres it, so each file a drawing is saved to as a VSDX type states what the program assumes in its root:
//!
//! * a **new drawing** is saved with the whole of it;
//! * a **`.diacraft` file of an earlier build** and an **`.odg`**, saved as a VSDX type (the model writer), are given the cells whose absence a reader reads as another value than the
//!   assumed one, and the save's check finds nothing to say of it; a save into the file after that keeps them;
//! * a **VSDX file that is saved into itself** keeps the root it has.
//!
//! Every file is made by code and read back through the real reader.

use std::collections::BTreeMap;
use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Document, Row, RowKey, Section, SectionKey, SharedVec, Sheet, Style, Value};
use diacraft_native::{State, WriteOptions};
use diacraft_testkit::package::{PageSpec, VsdxBuilder, shape};
use serde_json::json;

use crate::native_common::{Dir, pinned};
use crate::odg_build::{Drawing, rect};
use crate::run;

/// The cells under the root of the VSDX file `bytes`, and row 0 of its `Paragraph` section when it has one.
fn root_of(bytes: &[u8]) -> (BTreeMap<String, String>, Option<BTreeMap<String, String>>) {
    let (doc, _) = diacraft_vsdx::read(bytes, &diacraft_vsdx::Limits::default()).unwrap_or_else(|e| panic!("{e}"));
    let root = doc.style(0).expect("a root style");
    let text = |cells: &BTreeMap<CellName, Cell>| -> BTreeMap<String, String> {
        cells.iter().map(|(n, c)| (n.as_str().to_owned(), c.value.as_ref().map(|v| v.text().to_owned()).unwrap_or_default())).collect()
    };
    let direct = text(&root.sheet.cells);
    let paragraph =
        root.sheet.section(&SectionKey::new("Paragraph", None)).and_then(|section| section.row(&RowKey::Ix(0)).map(|row| text(&row.cells)));
    (direct, paragraph)
}

/// The root a new drawing held before the text cells: the flags, a fill, a line, the middle, the margins and a `Character` row with a size and a colour.
fn old_root() -> Style {
    let mut sheet = Sheet::default();
    for (name, value) in [
        ("EnableLineProps", "1"),
        ("EnableFillProps", "1"),
        ("EnableTextProps", "1"),
        ("FillPattern", "1"),
        ("LinePattern", "1"),
        ("LineWeight", "0.01"),
        ("VerticalAlign", "1"),
        ("LeftMargin", "0.05555556"),
    ] {
        sheet.cells.insert(CellName::new(name), Cell::constant(Value::from_text(value)));
    }
    let mut row = Row { ix: Some(0), ..Row::default() };
    for (name, value) in [("Size", "0.1666667"), ("Color", "0")] {
        row.cells.insert(CellName::new(name), Cell::constant(Value::from_text(value)));
    }
    let mut character = Section::new("Character", None);
    character.rows.push(Arc::new(row));
    sheet.sections.push(Arc::new(character));
    Style { id: 0, name: Some(Arc::from("No Style")), name_u: Some(Arc::from("No Style")), sheet: Arc::new(sheet), ..Style::default() }
}

/// The bytes of a `.diacraft` file of an earlier build: a blank drawing with the root it had.
fn diacraft_of_an_earlier_build() -> Vec<u8> {
    let mut doc = Document::new_blank();
    doc.styles = SharedVec::from_vec(vec![Arc::new(old_root())]);
    diacraft_native::write(&doc, &State::default(), None, &WriteOptions::default()).unwrap_or_else(|e| panic!("{e}")).bytes
}

/// Asserts that the root of `bytes` holds the cells whose absence reads otherwise, with the value the renderer assumes.
#[track_caller]
fn assert_states_the_reading(bytes: &[u8]) {
    let (direct, paragraph) = root_of(bytes);
    let paragraph = paragraph.expect("the root holds a Paragraph row 0");
    assert_eq!(paragraph.get("HorzAlign").map(String::as_str), Some("1"), "{paragraph:?}");
    assert_eq!(paragraph.get("SpLine").map(String::as_str), Some("-1.2"));
    assert_eq!(paragraph.get("BulletFontSize").map(String::as_str), Some("-1"));
    assert_eq!(direct.get("DefaultTabStop").map(String::as_str), Some("0.5"));
    assert_eq!(direct.get("VerticalAlign").map(String::as_str), Some("1"));
}

#[test]
fn a_new_drawing_saved_as_a_vsdx_type_has_the_whole_paragraph_row_in_its_root() {
    let dir = Dir::new();
    let mut s = pinned();
    run(&mut s, "file.new", json!({}));
    run(&mut s, "file.save_as", json!({ "path": dir.path("a.vsdx") }));
    let (direct, paragraph) = root_of(&dir.read("a.vsdx"));
    let paragraph = paragraph.expect("the root holds a Paragraph row 0");
    assert_eq!(paragraph.len(), 12, "{paragraph:?}");
    assert_eq!(paragraph.get("HorzAlign").map(String::as_str), Some("1"));
    for (name, value) in [("DefaultTabStop", "0.5"), ("TextBkgnd", "0"), ("TextDirection", "0"), ("VerticalAlign", "1")] {
        assert_eq!(direct.get(name).map(String::as_str), Some(value), "{name}");
    }
}

#[test]
fn a_drawing_made_from_a_template_saved_as_a_vsdx_type_has_the_whole_paragraph_row_in_its_root() {
    let dir = Dir::new();
    let mut s = pinned();
    run(&mut s, "file.new", json!({ "template": "basic_diagram" }));
    run(&mut s, "file.save_as", json!({ "path": dir.path("t.vsdx") }));
    let (direct, paragraph) = root_of(&dir.read("t.vsdx"));
    let paragraph = paragraph.expect("the root holds a Paragraph row 0");
    assert_eq!(paragraph.len(), 12, "{paragraph:?}");
    assert_eq!(paragraph.get("HorzAlign").map(String::as_str), Some("1"));
    assert_eq!(direct.get("DefaultTabStop").map(String::as_str), Some("0.5"));
}

#[test]
fn a_diacraft_file_of_an_earlier_build_made_from_a_template_saved_as_vsdx_states_what_a_reader_would_read_otherwise() {
    // The skeleton's package data that a template drawing holds is DiaCraft's own and no file's: the drawing came from no VSDX file.
    let dir = Dir::new();
    let mut s = pinned();
    run(&mut s, "file.new", json!({ "template": "basic_diagram" }));
    let mut doc = s.document().expect("a drawing").clone();
    doc.styles = SharedVec::from_vec(vec![Arc::new(old_root())]);
    let bytes = diacraft_native::write(&doc, &State::default(), None, &WriteOptions::default()).unwrap_or_else(|e| panic!("{e}")).bytes;
    let mut t = pinned();
    run(&mut t, "file.open", json!({ "path": dir.write("t.diacraft", &bytes) }));
    run(&mut t, "file.save_as", json!({ "path": dir.path("t.vsdx") }));
    assert_states_the_reading(&dir.read("t.vsdx"));
}

#[test]
fn a_diacraft_file_of_an_earlier_build_saved_as_vsdx_states_what_a_reader_would_read_otherwise() {
    let dir = Dir::new();
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": dir.write("a.diacraft", &diacraft_of_an_earlier_build()) }));
    let root = s.document().and_then(|doc| doc.style(0)).expect("a root style");
    assert!(root.sheet.section(&SectionKey::new("Paragraph", None)).is_none(), "the file's root holds no Paragraph section");
    let out = run(&mut s, "file.save_as", json!({ "path": dir.path("b.vsdx") }));
    assert_states_the_reading(&dir.read("b.vsdx"));
    // Only the cells whose absence reads otherwise: not the rest of the row.
    let (direct, paragraph) = root_of(&dir.read("b.vsdx"));
    assert_eq!(
        paragraph.map(|p| p.keys().cloned().collect::<Vec<_>>()),
        Some(vec!["BulletFontSize".to_owned(), "HorzAlign".to_owned(), "SpLine".to_owned()])
    );
    assert!(!direct.contains_key("TextDirection"));
    // The check after the save finds nothing to say: the file holds the cells the drawing leaves to the renderer, which are the same.
    assert_eq!(out["checked"]["done"], json!(true), "{out}");
    assert_eq!(out["checked"]["classes"], json!([]), "{out}");
    assert_eq!(out["checked"]["as_values"], json!(false), "{out}");
}

#[test]
fn a_save_into_the_file_a_save_as_wrote_keeps_the_cells_the_save_as_added() {
    let dir = Dir::new();
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": dir.write("a.diacraft", &diacraft_of_an_earlier_build()) }));
    run(&mut s, "file.save_as", json!({ "path": dir.path("b.vsdx") }));
    assert_states_the_reading(&dir.read("b.vsdx"));
    // An edit, and a save into the file: the drawing the session holds is not the file's root (or it is: either way the file keeps what it holds).
    run(&mut s, "page.rename", json!({ "id": 1, "name": "Renamed" }));
    run(&mut s, "file.save", json!({}));
    assert_states_the_reading(&dir.read("b.vsdx"));
    // And the file opens and saves into itself once more.
    let mut t = pinned();
    run(&mut t, "file.open", json!({ "path": dir.path("b.vsdx") }));
    run(&mut t, "page.rename", json!({ "id": 1, "name": "Again" }));
    run(&mut t, "file.save", json!({}));
    assert_states_the_reading(&dir.read("b.vsdx"));
}

#[test]
fn an_odg_drawing_saved_as_vsdx_has_the_root_a_new_drawing_has() {
    let dir = Dir::new();
    let mut s = pinned();
    let odg = Drawing::new().page(&rect("R", (2.0, 2.0, 4.0, 2.0), "hello")).bytes();
    run(&mut s, "file.open", json!({ "path": dir.write("a.odg", &odg) }));
    run(&mut s, "file.save_as", json!({ "path": dir.path("b.vsdx") }));
    let (direct, paragraph) = root_of(&dir.read("b.vsdx"));
    let paragraph = paragraph.expect("the root of a drawing read from an .odg holds a Paragraph row 0");
    assert_eq!(paragraph.get("HorzAlign").map(String::as_str), Some("1"), "{paragraph:?}");
    assert_eq!(paragraph.len(), 12, "the reader makes the root from a new drawing's: the whole row");
    assert_eq!(direct.get("DefaultTabStop").map(String::as_str), Some("0.5"));
}

/// The bytes of the document part of the package `bytes` (the one part whose name ends in `/document.xml`).
fn document_part(bytes: &[u8]) -> Vec<u8> {
    let package = diacraft_opc::Package::read(bytes, &diacraft_opc::Limits::default()).expect("a package");
    let name = package.parts().find(|name| name.ends_with("/document.xml")).expect("a document part").to_owned();
    package.part(&name).expect("the part is listed").to_vec()
}

#[test]
fn a_vsdx_file_an_earlier_build_wrote_keeps_its_root_in_a_save_and_a_save_as_that_splice_into_it() {
    // The file DiaCraft wrote with its own earlier root: no Paragraph section. Opened and saved with no edit, or saved under another name, its document part is the file's own bytes (a save
    // of a file of the old namespace writes its solution part again, without a declaration, and no other part): the cells are the model writer's (a drawing that came through a `.diacraft`
    // file), never a splice's.
    let dir = Dir::new();
    let input = diacraft_testkit::old_namespace::file(diacraft_testkit::old_namespace::DRAWING_VSDX);
    assert!(root_of(&input).1.is_none(), "the file holds no Paragraph section");
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": dir.write("old.vsdx", &input) }));
    run(&mut s, "file.save", json!({}));
    assert!(document_part(&dir.read("old.vsdx")) == document_part(&input), "a save with no edit writes the document part as it read it");
    assert!(root_of(&dir.read("old.vsdx")).1.is_none());
    run(&mut s, "file.save_as", json!({ "path": dir.path("copy.vsdx") }));
    assert!(document_part(&dir.read("copy.vsdx")) == document_part(&input), "a Save As of the file keeps the document part as it is");
    assert!(root_of(&dir.read("copy.vsdx")).1.is_none());
}

#[test]
fn a_vsdx_file_that_is_saved_into_itself_keeps_the_root_it_has() {
    let dir = Dir::new();
    // A file whose root holds the three flags and nothing else: what it lacks, the file lacked.
    let bytes = VsdxBuilder::new()
        .page(
            PageSpec::new(1, "Page-1")
                .shapes(&shape(1, r#"<Cell N="PinX" V="1"/><Cell N="PinY" V="1"/><Cell N="Width" V="1"/><Cell N="Height" V="1"/>"#)),
        )
        .build();
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": dir.write("a.vsdx", &bytes) }));
    run(&mut s, "page.rename", json!({ "id": 1, "name": "Renamed" }));
    run(&mut s, "file.save", json!({}));
    let (direct, paragraph) = root_of(&dir.read("a.vsdx"));
    assert!(paragraph.is_none(), "no Paragraph section is added to a file's own root");
    assert!(!direct.contains_key("DefaultTabStop"), "{direct:?}");
    // And a Save As of it to another VSDX file (the model writer, a drawing that came from a package) leaves the root as it is too.
    run(&mut s, "file.save_as", json!({ "path": dir.path("b.vsdx") }));
    let (direct, paragraph) = root_of(&dir.read("b.vsdx"));
    assert!(paragraph.is_none() && !direct.contains_key("DefaultTabStop"));
}
