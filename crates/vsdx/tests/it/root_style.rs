//! The root style a written file holds. The reference application reads a paragraph with no `HorzAlign` on its way as left-aligned when the root style holds none, and
//! this program centres it, so every file a drawing is written to states what the program assumes in its root:
//!
//! * a **new drawing** is written with the whole of it;
//! * the **model writer** (a drawing that came from no VSDX file: a `.diacraft` file of an earlier build, an `.odg`) adds to a root that lacks them **only the cells whose absence reads as another
//!   value than the assumed one**, and gives back the model as the caller had it, so that the next save into the file does not take the cells out;
//! * a **drawing that was read from a package**, and a **save into a file**, leave the root the file has alone, byte for byte.
//!
//! Every input is built in code.

use std::collections::BTreeMap;
use std::sync::Arc;

use diacraft_doc::{
    Cell, CellAddr, CellEdit, CellName, Document, Owner, Passthrough, Placed, RawElement, RawNode, Row, Section, ShapeRef, SharedVec, Sheet,
    SheetRef, Style, Value, WriteOrigin,
};
use diacraft_opc::xml::{self, El};
use diacraft_opc::{Limits, Package};
use diacraft_testkit::package::{PageSpec, VsdxBuilder, shape};
use diacraft_vsdx::{OriginalBytes, WriteOptions, Written, read, write_with};

use crate::read_ok;

const NOW: i64 = 1_700_000_000;

fn write_new(doc: &Document) -> Written {
    write_with(doc, None, &WriteOptions { now: Some(NOW), ..WriteOptions::default() }).unwrap_or_else(|e| panic!("{e}"))
}

/// The bytes of the document part of the package `package` (the one part whose name ends in `/document.xml`).
fn document_of(package: &Package) -> Vec<u8> {
    let name = package.parts().find(|name| name.ends_with("/document.xml")).expect("a document part").to_owned();
    package.part(&name).expect("the part is listed").to_vec()
}

/// The style 0 element of the document part of `bytes`.
fn root_of(bytes: &[u8]) -> El {
    let package = Package::read(bytes, &Limits::default()).unwrap();
    let document = xml::parse(&document_of(&package)).unwrap();
    let styles = document.child("StyleSheets").expect("a StyleSheets element");
    styles.children_named("StyleSheet").find(|s| s.attr("ID") == Some("0")).expect("a style 0").clone()
}

/// The cells of row 0 of the section `section` of `style`: `N` to `V`; `None` when the style has no such section or row.
fn row0(style: &El, section: &str) -> Option<BTreeMap<String, String>> {
    let section = style.children_named("Section").find(|s| s.attr("N") == Some(section))?;
    let row = section.children_named("Row").find(|r| r.attr("IX") == Some("0"))?;
    Some(row.children_named("Cell").map(|c| (c.attr("N").unwrap_or("").to_owned(), c.attr("V").unwrap_or("").to_owned())).collect())
}

/// The cells directly under `style`: `N` to `V`.
fn direct(style: &El) -> BTreeMap<String, String> {
    style.children_named("Cell").map(|c| (c.attr("N").unwrap_or("").to_owned(), c.attr("V").unwrap_or("").to_owned())).collect()
}

/// The bytes of the document part.
fn document_part(bytes: &[u8]) -> Vec<u8> {
    document_of(&Package::read(bytes, &Limits::default()).unwrap())
}

fn has_section(style: &El, name: &str) -> bool {
    style.children_named("Section").any(|s| s.attr("N") == Some(name))
}

/// The root a new drawing held before a change: the flags, a fill, a line, the middle, the margins and a `Character` row with a size and a colour.
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
        ("RightMargin", "0.05555556"),
        ("TopMargin", "0.05555556"),
        ("BottomMargin", "0.05555556"),
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

/// A drawing made by an earlier build: the blank drawing with the old root.
fn drawing_with_old_root() -> Document {
    let mut doc = Document::new_blank();
    doc.styles = SharedVec::from_vec(vec![Arc::new(old_root())]);
    doc
}

/// The cells of the `Paragraph` row 0 of the root style of a new drawing, them but for `TextPosAfterBullet`, which a reader takes as 0 when it is missing.
const PARAGRAPH: [(&str, &str); 12] = [
    ("Bullet", "0"),
    ("BulletFont", "0"),
    ("BulletFontSize", "-1"),
    ("BulletStr", ""),
    ("Flags", "0"),
    ("HorzAlign", "1"),
    ("IndFirst", "0"),
    ("IndLeft", "0"),
    ("IndRight", "0"),
    ("SpAfter", "0"),
    ("SpBefore", "0"),
    ("SpLine", "-1.2"),
];

/// The cells whose absence a reader takes as 0 where the program assumes another value, with that value.
const READING: [(&str, &str, &str); 6] = [
    ("Paragraph", "HorzAlign", "1"),
    ("Paragraph", "SpLine", "-1.2"),
    ("Paragraph", "BulletFontSize", "-1"),
    ("Character", "FontScale", "1"),
    ("Character", "ComplexScriptSize", "-1"),
    ("", "DefaultTabStop", "0.5"),
];

fn reading_cell(root: &El, section: &str, name: &str) -> Option<String> {
    if section.is_empty() { direct(root).get(name).cloned() } else { row0(root, section)?.get(name).cloned() }
}

// ----------------------------------------------------------------------------------------------------------------------- a new drawing

#[test]
fn a_new_drawing_is_written_with_a_paragraph_row_in_its_root_style() {
    let root = root_of(&write_new(&Document::new_blank()).bytes);
    let paragraph = row0(&root, "Paragraph").expect("style 0 holds a Paragraph section with a row IX 0");
    for (name, value) in PARAGRAPH {
        assert_eq!(paragraph.get(name).map(String::as_str), Some(value), "Paragraph row 0, {name}");
    }
}

#[test]
fn a_new_drawings_root_style_holds_the_whole_text_block_and_the_cells_of_the_first_run() {
    let root = root_of(&write_new(&Document::new_blank()).bytes);
    let block = direct(&root);
    for (name, value) in [
        ("LeftMargin", "0.05555556"),
        ("RightMargin", "0.05555556"),
        ("TopMargin", "0.05555556"),
        ("BottomMargin", "0.05555556"),
        ("VerticalAlign", "1"),
        ("DefaultTabStop", "0.5"),
        ("TextBkgnd", "0"),
        ("TextBkgndTrans", "0"),
        ("TextDirection", "0"),
    ] {
        assert_eq!(block.get(name).map(String::as_str), Some(value), "{name}");
    }
    let character = row0(&root, "Character").expect("a Character row 0");
    for (name, value) in [("Size", "0.1666667"), ("Color", "0"), ("FontScale", "1"), ("ComplexScriptSize", "-1"), ("Strikethru", "0")] {
        assert_eq!(character.get(name).map(String::as_str), Some(value), "Character row 0, {name}");
    }
    // The root names no face and no language: a face is the font database's and a language the document's.
    for name in ["Font", "AsianFont", "ComplexScriptFont", "LangID"] {
        assert!(!character.contains_key(name), "{name}");
    }
}

#[test]
fn a_new_drawings_root_style_reads_back_as_the_model_it_was_written_from() {
    let written = write_new(&Document::new_blank());
    let (read_back, _) = read_ok(&written.bytes);
    let doc = Document::new_blank();
    assert_eq!(read_back.style(0).map(|s| s.sheet.sections.len()), doc.style(0).map(|s| s.sheet.sections.len()));
    let paragraph = |d: &Document| {
        let style = d.style(0).expect("a root");
        let section = style.sheet.section(&diacraft_doc::SectionKey::new("Paragraph", None)).expect("a Paragraph section");
        section
            .row(&diacraft_doc::RowKey::Ix(0))
            .expect("row 0")
            .cells
            .iter()
            .map(|(n, c)| (n.as_str().to_owned(), c.value.as_ref().map(|v| v.text().to_owned())))
            .collect::<BTreeMap<_, _>>()
    };
    assert_eq!(paragraph(&read_back), paragraph(&doc));
}

// ----------------------------------------------------------------------------------------------------------------------- the model writer

#[test]
fn a_drawing_of_an_earlier_build_gets_the_cells_whose_absence_reads_otherwise_when_it_is_written_from_the_model() {
    let written = write_new(&drawing_with_old_root());
    let root = root_of(&written.bytes);
    for (section, name, value) in READING {
        assert_eq!(reading_cell(&root, section, name).as_deref(), Some(value), "{section} {name}");
    }
    // Only those: a cell that reads as assumed when it is left out is not written.
    let paragraph = row0(&root, "Paragraph").expect("a Paragraph row 0");
    assert_eq!(paragraph.keys().map(String::as_str).collect::<Vec<_>>(), ["BulletFontSize", "HorzAlign", "SpLine"]);
    let character = row0(&root, "Character").expect("a Character row 0");
    assert_eq!(character.keys().map(String::as_str).collect::<Vec<_>>(), ["Color", "ComplexScriptSize", "FontScale", "Size"]);
    assert!(!direct(&root).contains_key("TextDirection") && !direct(&root).contains_key("TextBkgnd"));
    // What the root held is as it was.
    assert_eq!(direct(&root).get("LeftMargin").map(String::as_str), Some("0.05555556"));
    assert_eq!(character.get("Size").map(String::as_str), Some("0.1666667"));
}

/// A drawing that takes the model writer's own path, with its root: a drawing that holds an element of its document that no field reads (`held::wanted`).
fn held_drawing_with_old_root() -> Document {
    let mut doc = drawing_with_old_root();
    let element = RawElement { name: Arc::from("EventList"), attrs: Default::default(), children: Vec::new() };
    doc.passthrough = Passthrough::from_nodes(vec![Placed { after: 0, node: RawNode::Element(element) }]);
    assert!(doc.holds_more_than_snap_words(), "the drawing takes the model writer's plan");
    doc
}

#[test]
fn the_model_writers_own_plan_gives_the_same_cells_and_gives_the_callers_root_back() {
    let doc = held_drawing_with_old_root();
    let written = write_new(&doc);
    let root = root_of(&written.bytes);
    for (section, name, value) in READING {
        assert_eq!(reading_cell(&root, section, name).as_deref(), Some(value), "{section} {name}");
    }
    assert_eq!(written.document.style(0).map(|s| &s.sheet), doc.style(0).map(|s| &s.sheet), "the baseline's root is the caller's own");
}

#[test]
fn the_model_given_back_keeps_the_root_the_caller_had_so_the_next_save_into_the_file_keeps_the_cells() {
    let mut doc = drawing_with_old_root();
    let page = doc.add_page(None).unwrap();
    let written = write_new(&doc);
    // The file holds the cells and the model that is the baseline of the next save does not: the file's root is not an edit of the caller's.
    assert!(row0(&root_of(&written.bytes), "Paragraph").is_some());
    assert_eq!(written.document.style(0).map(|s| &s.sheet), doc.style(0).map(|s| &s.sheet), "the baseline's root is the caller's own");
    // The session's own drawing, edited and saved into the file the save wrote, with that baseline: the document part, and so style 0, is the file's own bytes.
    let mut edited = Document::clone(&doc);
    let width = CellEdit::Cell(Cell::constant(Value::from_text("5")));
    edited.write_cell(&SheetRef::Page(page), &CellAddr::direct("PageWidth"), width, WriteOrigin::Automation).unwrap();
    let original = OriginalBytes { bytes: &written.bytes, baseline: Some(&written.document) };
    let saved = write_with(&edited, Some(&original), &WriteOptions { now: Some(NOW), ..WriteOptions::default() }).unwrap_or_else(|e| panic!("{e}"));
    let (before, after) = (root_of(&written.bytes), root_of(&saved.bytes));
    assert_eq!(row0(&before, "Paragraph"), row0(&after, "Paragraph"), "the Paragraph row survives a later save into the file");
    assert_eq!(row0(&after, "Paragraph").and_then(|r| r.get("HorzAlign").cloned()).as_deref(), Some("1"));
    assert_eq!(document_part(&saved.bytes), document_part(&written.bytes), "and the part is the file's bytes");
}

#[test]
fn a_cell_the_root_holds_is_not_changed_by_the_model_writer() {
    // A drawing the person left-aligned on purpose: the root's own `HorzAlign` 0 and a spacing of one line.
    let mut doc = drawing_with_old_root();
    let mut style = old_root();
    let mut sheet = Sheet::clone(&style.sheet);
    let mut row = Row { ix: Some(0), ..Row::default() };
    row.cells.insert(CellName::new("HorzAlign"), Cell::constant(Value::from_text("0")));
    row.cells.insert(CellName::new("SpLine"), Cell::constant(Value::from_text("-1")));
    let mut paragraph = Section::new("Paragraph", None);
    paragraph.rows.push(Arc::new(row));
    sheet.sections.push(Arc::new(paragraph));
    style.sheet = Arc::new(sheet);
    doc.styles = SharedVec::from_vec(vec![Arc::new(style)]);
    let root = root_of(&write_new(&doc).bytes);
    let paragraph = row0(&root, "Paragraph").expect("a Paragraph row 0");
    assert_eq!(paragraph.get("HorzAlign").map(String::as_str), Some("0"));
    assert_eq!(paragraph.get("SpLine").map(String::as_str), Some("-1"));
    assert_eq!(paragraph.get("BulletFontSize").map(String::as_str), Some("-1"), "a cell the row lacks is added");
}

// ----------------------------------------------------------------------------------------------------------------------- what came from a file

/// A package of one page and one shape whose root style is the file's own: the three flags and nothing else.
fn sparse_package() -> Vec<u8> {
    VsdxBuilder::new()
        .page(
            PageSpec::new(1, "Page-1")
                .shapes(&shape(1, r#"<Cell N="PinX" V="1"/><Cell N="PinY" V="1"/><Cell N="Width" V="1"/><Cell N="Height" V="1"/>"#)),
        )
        .build()
}

#[test]
fn a_drawing_read_from_a_package_is_written_from_the_model_with_the_root_the_file_has() {
    let bytes = sparse_package();
    assert!(!has_section(&root_of(&bytes), "Paragraph"), "the file's own root holds no Paragraph section");
    let (doc, _) = read_ok(&bytes);
    let written = write_new(&doc);
    let root = root_of(&written.bytes);
    assert!(!has_section(&root, "Paragraph"), "a root a file holds is the file's: the model writer adds nothing to it");
    assert!(!direct(&root).contains_key("DefaultTabStop"));
}

#[test]
fn a_drawing_read_from_a_package_whose_root_is_the_one_diacraft_made_before_is_given_the_cells_too() {
    // A VSDX file a build before this one wrote, or a drawing made from a template that build had (it holds the package data of DiaCraft's own skeleton): a package behind it, and DiaCraft's own
    // earlier root, which no file of another program holds. What that root lacks is DiaCraft's to state, as for a drawing that came from no package.
    let (mut doc, _) = read_ok(&sparse_package());
    doc.styles = SharedVec::from_vec(vec![Arc::new(old_root())]);
    assert!(doc.vsdx.as_deref().is_some_and(|held| !held.holds_windows_alone()), "the drawing holds a package");
    let written = write_new(&doc);
    let root = root_of(&written.bytes);
    for (section, name, value) in READING {
        assert_eq!(reading_cell(&root, section, name).as_deref(), Some(value), "{section} {name}");
    }
    assert_eq!(written.document.style(0).map(|s| &s.sheet), doc.style(0).map(|s| &s.sheet), "the baseline's root is the caller's own");
}

#[test]
fn a_save_into_a_file_leaves_the_document_part_with_its_root_byte_for_byte() {
    let bytes = sparse_package();
    let (mut doc, _) = read_ok(&bytes);
    let baseline = doc.clone();
    // An edit of the shape's width: the page part is patched, and the document part is the file's own.
    let edit = CellEdit::Cell(Cell::constant(Value::from_text("2")));
    let sheet = SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0));
    doc.write_cell(&sheet, &CellAddr::direct("Width"), edit, WriteOrigin::Automation).unwrap();
    let original = OriginalBytes { bytes: &bytes, baseline: Some(&baseline) };
    let saved = write_with(&doc, Some(&original), &WriteOptions { now: Some(NOW), ..WriteOptions::default() }).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(document_part(&saved.bytes), document_part(&bytes), "the document part is the file's own bytes");
    assert!(!has_section(&root_of(&saved.bytes), "Paragraph"));
    // And it still reads.
    assert!(read(&saved.bytes, &diacraft_vsdx::Limits::default()).is_ok());
}
