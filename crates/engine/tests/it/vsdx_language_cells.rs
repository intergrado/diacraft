//! **Every route to a `.vsdx` file writes a run's language the way the files of the program that opens it hold one**: the tag alone (`V="fr-CA"`, no `F`, no `U`). A language
//! typed in the formula bar as a string (`"fr-CA"`) is a string formula, which that program reads as an error and replaces by a language of its own, so the run's language was lost (`Char.LangID`
//! read `#VALUE!` and 8192). The `.odg` and `.diacraft` routes keep the language as they did: the `.odg` says it in `fo:language` and `fo:country`, and `.diacraft` holds the cell whole.
//!
//! **Every file is made by code**: the package by the testkit's builder, from the specifications; the readers and writers are the real ones.

use diacraft_doc::{Cell, CellAddr, Document, FormulaSlot, RowKey, Value};
use diacraft_odg::package::Package as OdgPackage;
use diacraft_opc::{Limits, Package};
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};
use diacraft_testkit::package::{PageSpec, VsdxBuilder, shape_with};
use serde_json::{Value as Json, json};

use crate::native_common::{Dir, pinned};
use crate::run;
use crate::stencil_open::box_cells;

const PAGE1: &str = "/visio/pages/page1.xml"; // brand-ok: a part name the format defines

/// The runs of the drawing's one shape and the language each is in.
const RUNS: [(&str, &str); 4] = [("one ", "en-US"), ("deux ", "fr-FR"), ("zxx ", "x-none"), ("dört", "tr-TR")];

/// A drawing whose shape (5) says four runs in four languages, each as the files hold one: a plain tag.
fn drawing() -> Vec<u8> {
    let rows: String = RUNS
        .iter()
        .enumerate()
        .map(|(ix, (_, lang))| format!(r#"<Row IX="{ix}"><Cell N="LangID" V="{lang}"/><Cell N="Size" V="0.2"/></Row>"#))
        .collect();
    let text: String = RUNS.iter().enumerate().map(|(ix, (run, _))| format!(r#"<cp IX="{ix}"/>{run}"#)).collect();
    let shape = shape_with(
        5,
        r#" Type="Shape""#,
        &format!(r##"{}<Cell N="FillPattern" V="1"/><Section N="Character">{rows}</Section><Text>{text}</Text>"##, box_cells(2.0, 1.0)),
    );
    VsdxBuilder::new().page(PageSpec::new(1, "Page-1").sheet(r#"<Cell N="PageWidth" V="8.5"/><Cell N="PageHeight" V="11"/>"#).shapes(&shape)).build()
}

/// The start tags of the `LangID` cells of the first page of the package `bytes`, in order.
fn language_tags(bytes: &[u8]) -> Vec<String> {
    let package = Package::read(bytes, &Limits::default()).unwrap();
    let text = String::from_utf8(package.part(PAGE1).unwrap().to_vec()).unwrap();
    let (mut out, mut rest) = (Vec::new(), text.as_str());
    while let Some(at) = rest.find("<Cell N=\"LangID\"") {
        let end = rest[at..].find('>').map_or(rest.len(), |e| at + e + 1);
        out.push(rest[at..end].to_owned());
        rest = &rest[end..];
    }
    out
}

/// What the tags of the four runs are when the second is `tag` and the others are as `drawing` made them.
fn tags_with_second(tag: &str) -> Vec<String> {
    let mut want: Vec<&str> = RUNS.iter().map(|(_, lang)| *lang).collect();
    want[1] = tag;
    want.iter().map(|lang| format!("<Cell N=\"LangID\" V=\"{lang}\"/>")).collect()
}

/// Types `"<tag>"` in the formula bar of the second run's `LangID` (`Char.LangID[2]`).
fn type_language(s: &mut diacraft_engine::Session, tag: &str) {
    let answer = run(s, "sheet.set_cell", json!({"id": 5, "cell": "Char.LangID[2]", "formula": format!("\"{tag}\"")}));
    assert_eq!(answer["changed"], true, "{answer}");
    assert_eq!(answer["value"], tag, "{answer}");
}

fn opened(dir: &Dir, name: &str) -> diacraft_engine::Session {
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": dir.path(name) }));
    s
}

/// The cell of the second run's `LangID` as the model holds it.
fn second(doc: &Document) -> Cell {
    let shape = doc.pages.get(0).unwrap().shapes.get(0).unwrap();
    let section = shape.sheet.sections.iter().find(|s| s.name.as_str() == "Character").unwrap();
    section.rows.iter().find(|r| r.ix == Some(1)).unwrap().cells.get("LangID").unwrap().clone()
}

/// That the file the save wrote was compared with the drawing it was written from and nothing differed.
fn assert_checked_clean(saved: &Json) {
    let classes = saved["checked"]["classes"].as_array().unwrap_or_else(|| panic!("the save was checked: {saved}"));
    assert!(classes.is_empty(), "the file differs from the drawing: {}", saved["checked"]);
}

// ---- a save into the file ----------------------------------------------------------------------------------------------------------------

#[test]
fn a_language_typed_as_a_string_formula_is_written_into_the_file_as_the_tag_alone() {
    let dir = Dir::new();
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": dir.write("a.vsdx", &drawing()) }));
    type_language(&mut s, "fr-CA");
    // In the session the cell is what was typed: a formula that says a string.
    assert!(matches!(second(s.document().unwrap()).formula, FormulaSlot::Local(_)));
    run(&mut s, "file.save", json!({}));
    assert_eq!(language_tags(&dir.read("a.vsdx")), tags_with_second("fr-CA"));
    // And the next save, with nothing else edited, writes the file as it is.
    let once = dir.read("a.vsdx");
    run(&mut s, "file.save", json!({}));
    assert_eq!(dir.read("a.vsdx"), once);
    // The file reads back in the language, as the tag.
    let back = opened(&dir, "a.vsdx");
    let cell = second(back.document().unwrap());
    assert_eq!(cell.value.as_ref().map(Value::text), Some("fr-CA"));
    assert!(matches!(cell.formula, FormulaSlot::Absent) && cell.unit.is_none(), "{cell:?}");
}

// ---- a drawing written from the model ----------------------------------------------------------------------------------------------------

#[test]
fn save_as_vsdx_from_a_vsdx_session_writes_the_tag_alone() {
    let dir = Dir::new();
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": dir.write("a.vsdx", &drawing()) }));
    type_language(&mut s, "fr-CA");
    run(&mut s, "file.save_as", json!({ "path": dir.path("b.vsdx") }));
    assert_eq!(language_tags(&dir.read("b.vsdx")), tags_with_second("fr-CA"));
    // The file the first save wrote holds what it held: the person has not saved into it.
    assert_eq!(language_tags(&dir.read("a.vsdx")), tags_with_second("fr-FR"));
}

#[test]
fn a_drawing_never_saved_is_written_with_the_tag_alone() {
    let dir = Dir::new();
    let mut s = pinned();
    let shape = ShapeBuilder::new(5)
        .cell("PinX", "1")
        .cell("PinY", "1")
        .cell("Width", "2")
        .cell("Height", "1")
        .set(CellAddr::in_row("Character", None, RowKey::Ix(0), "LangID"), Cell::constant(Value::from_text("en-US")))
        .set(CellAddr::in_row("Character", None, RowKey::Ix(1), "LangID"), Cell::constant(Value::from_text("fr-FR")));
    let doc = DocBuilder::new().page(1, PageBuilder::new().name("Page-1").shape(shape)).build();
    s.edit("build", |d| {
        *d = doc;
        Ok(())
    })
    .unwrap();
    type_language(&mut s, "fr-CA");
    let saved = run(&mut s, "file.save_as", json!({ "path": dir.path("new.vsdx") }));
    assert_eq!(language_tags(&dir.read("new.vsdx")), ["<Cell N=\"LangID\" V=\"en-US\"/>", "<Cell N=\"LangID\" V=\"fr-CA\"/>"]);
    assert_checked_clean(&saved);
}

// ---- the other formats ---------------------------------------------------------------------------------------------------------------------

#[test]
fn a_diacraft_file_holds_the_cell_whole_and_its_save_as_vsdx_writes_the_tag_alone() {
    let dir = Dir::new();
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": dir.write("a.vsdx", &drawing()) }));
    type_language(&mut s, "fr-CA");
    run(&mut s, "file.save_as", json!({ "path": dir.path("b.diacraft") }));
    // `.diacraft` is lossless: the cell comes back as it was typed.
    let mut from_native = opened(&dir, "b.diacraft");
    let cell = second(from_native.document().unwrap());
    assert!(matches!(cell.formula, FormulaSlot::Local(_)), "{cell:?}");
    assert_eq!(cell.value.as_ref().map(Value::text), Some("fr-CA"));
    let saved = run(&mut from_native, "file.save_as", json!({ "path": dir.path("c.vsdx") }));
    assert_eq!(language_tags(&dir.read("c.vsdx")), tags_with_second("fr-CA"));
    assert_checked_clean(&saved);
    // And a drawing read from that file is in the language still.
    let cell = second(opened(&dir, "c.vsdx").document().unwrap());
    assert_eq!(cell.value.as_ref().map(Value::text), Some("fr-CA"));
}

#[test]
fn an_odg_says_the_language_in_its_own_attributes_and_its_save_as_vsdx_writes_the_tag_alone() {
    let dir = Dir::new();
    let mut s = pinned();
    run(&mut s, "file.open", json!({ "path": dir.write("a.vsdx", &drawing()) }));
    type_language(&mut s, "fr-CA");
    run(&mut s, "file.save_as", json!({ "path": dir.path("b.odg") }));
    // The `.odg` keeps the language and the country of the run, where another program reads them.
    let package = OdgPackage::read(&dir.read("b.odg"), &diacraft_odg::Limits::default()).unwrap();
    let content = String::from_utf8(package.part("content.xml").unwrap().to_vec()).unwrap();
    assert!(content.contains(r#"fo:language="fr""#) && content.contains(r#"fo:country="CA""#), "{content}");
    assert!(content.contains(r#"fo:country="TR""#), "the other runs keep theirs: {content}");
    // Across to `.vsdx`: the tag alone, and the language is kept in the drawing it reads as.
    let mut from_odg = opened(&dir, "b.odg");
    run(&mut from_odg, "file.save_as", json!({ "path": dir.path("c.vsdx") }));
    assert_eq!(language_tags(&dir.read("c.vsdx")), tags_with_second("fr-CA"));
    let cell = second(opened(&dir, "c.vsdx").document().unwrap());
    assert_eq!(cell.value.as_ref().map(Value::text), Some("fr-CA"));
}
