//! Item 1: a style sheet the model has that the file lacks, and the shapes that name it.
//!
//! A new style sheet is an empty common `graphic` style at the end of `office:styles` and a `style` child of DiaCraft's document file that holds its whole sheet; a shape whose
//! three links name another style sheet has that style as the parent of its automatic style. Every case saves a model, reads the bytes back with the reader and compares what came
//! back with what went in, and saves again, which must be no edit.

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Links, Row, Section, Sheet, Style, Value};
use diacraft_odg::package::Package;
use diacraft_odg::{Limits, WriteError, read};

use crate::drawing::Drawing;
use crate::write_core::{Open, custom, open, open_bytes, part, real};

const PATH: &str = "DiaCraft/document.xml";

/// A style sheet like the `Theme` style a theme command makes: a number cell, a cell that holds a call and its marker, and a section of rows of such cells.
pub fn style_sheet(id: u32, name: &str, link: u32) -> Style {
    let mut sheet = Sheet { links: Links { line: Some(link), fill: Some(link), text: Some(link) }, ..Sheet::default() };
    sheet.cells.insert(CellName::new("ThemeIndex"), Cell::constant(Value::from_text("65534")));
    sheet.cells.insert(CellName::new("FillForegnd"), Cell::with_formula("THEMEVAL()", Some(Value::from_text("Themed"))));
    let mut gradient = Section::new("FillGradient", None);
    let mut row = Row { ix: Some(0), ..Row::default() };
    row.cells.insert(CellName::new("GradientStopColor"), Cell::with_formula("THEMEVAL()", Some(Value::from_text("Themed"))));
    gradient.rows.push(Arc::new(row));
    sheet.sections.push(Arc::new(gradient));
    Style { id, name: Some(Arc::from(name)), name_u: Some(Arc::from(name)), sheet: Arc::new(sheet), ..Style::default() }
}

fn one_shape() -> Open {
    open(&Drawing::a4().page(&custom("s1", "1cm")))
}

fn push(o: &mut Open, style: Style) {
    o.doc.styles.push(Arc::new(style));
}

fn refusal(o: &Open) -> String {
    match real(&o.doc, &o.bytes, &o.index) {
        Err(WriteError::Unsupported(words)) => words.to_owned(),
        Err(other) => panic!("another refusal than the writer's words: {other}"),
        Ok(saved) => panic!("a save that dropped what the model holds: {} bytes", saved.bytes.len()),
    }
}

/// A style sheet added last is an empty common style of `styles.xml` and an entry of the document file; the file reads back with the sheet the model has, and a second
/// save is no edit (a third, of the model the file reads as, none either).
#[test]
fn a_style_sheet_added_last_is_an_empty_common_style_and_its_sheet_is_in_the_document_file() {
    let mut o = one_shape();
    let n = u32::try_from(o.doc.styles.len()).unwrap();
    push(&mut o, style_sheet(n, "Theme", 0));
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    assert!(saved.edited);
    let styles = part(&saved.bytes, "styles.xml");
    assert!(styles.contains(r#"<style:style style:name="Theme" style:family="graphic"/>"#), "an empty common style: {styles}");
    assert_eq!(styles.matches("style:name=\"Theme\"").count(), 1);
    let file = part(&saved.bytes, PATH);
    assert!(
        file.contains(r#"<diacraft:style name="Theme">"#) && file.contains("THEMEVAL()") && file.contains("FillGradient"),
        "the sheet is in the file: {file}"
    );
    assert!(!file.contains("name-u"), "the NameU is the name: {file}");
    let package = Package::read(&saved.bytes, &Limits::default()).unwrap();
    assert!(package.manifest().entries.iter().any(|e| e.full_path == PATH), "and the file is listed");
    let (back, report) = read(&saved.bytes, &Limits::default()).unwrap();
    assert_eq!(report.warnings, Vec::<String>::new());
    assert_eq!(back.styles.len(), o.doc.styles.len());
    // The style as read holds the `.odg` data of its slot, which the model's does not: the rest is the same.
    let bare = |s: &diacraft_doc::Style| diacraft_doc::Style { ext: Default::default(), ..s.clone() };
    assert_eq!(bare(back.styles.get(n as usize).unwrap()), bare(o.doc.styles.get(n as usize).unwrap()), "the style sheet comes back whole");
    // The part no page changed is copied as it was (a style sheet is `styles.xml`, the document file and the manifest's business).
    let diff = diacraft_odg::diff_packages(&o.bytes, &saved.bytes, &Limits::default()).unwrap();
    let changed: Vec<&str> = diff.parts.iter().map(|p| p.part.as_str()).chain(diff.bytes_only.iter().map(String::as_str)).collect();
    assert!(!changed.contains(&"content.xml"), "content.xml is as it was: {changed:?}");
    assert!(changed.contains(&"styles.xml"), "{changed:?}");
    let again = real(&o.doc, &saved.bytes, &saved.index).unwrap();
    assert!(!again.edited && again.bytes == saved.bytes, "a second save is no edit");
    let reread = open_bytes(saved.bytes.clone());
    let third = real(&reread.doc, &reread.bytes, &reread.index).unwrap();
    assert!(!third.edited && third.bytes == saved.bytes, "nor is a save of the model read from the file");
}

/// The name is the `NameU` with the escapes, the display name is the sheet's `Name` when it is another, the document file carries the `NameU` when the name is not
/// it; a style whose links name an earlier new style names it as its parent.
#[test]
fn a_name_with_a_space_is_written_by_and_comes_back_as_it_was_and_a_parent_is_named() {
    let mut o = one_shape();
    let n = u32::try_from(o.doc.styles.len()).unwrap();
    let mut shown = style_sheet(n, "My Theme", 0);
    shown.name = Some(Arc::from("My Theme (shown)"));
    push(&mut o, shown);
    push(&mut o, style_sheet(n + 1, "Second", n));
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    let styles = part(&saved.bytes, "styles.xml");
    assert!(styles.contains(r#"<style:style style:name="My_20_Theme" style:family="graphic" style:display-name="My Theme (shown)"/>"#), "{styles}");
    assert!(styles.contains(r#"<style:style style:name="Second" style:family="graphic" style:parent-style-name="My_20_Theme"/>"#), "{styles}");
    assert!(styles.find("My_20_Theme").unwrap() < styles.find("\"Second\"").unwrap(), "in the order of the IDs");
    let file = part(&saved.bytes, PATH);
    assert!(file.contains(r#"<diacraft:style name="My_20_Theme" name-u="My Theme">"#), "the NameU the name is not: {file}");
    let (back, report) = read(&saved.bytes, &Limits::default()).unwrap();
    assert_eq!(report.warnings, Vec::<String>::new());
    for at in [n as usize, n as usize + 1] {
        // The style as read holds the `.odg` data of its slot, which the model's does not: the rest is the same.
        let bare = |s: &diacraft_doc::Style| diacraft_doc::Style { ext: Default::default(), ..s.clone() };
        assert_eq!(bare(back.styles.get(at).unwrap()), bare(o.doc.styles.get(at).unwrap()), "style {at}");
    }
    assert!(!real(&o.doc, &saved.bytes, &saved.index).unwrap().edited);
}

/// Every other change of the styles is refused in words that say what is wrong: a style sheet of the file changed or gone, a new one whose ID is not its place, whose
/// links name different styles or a style that is not there, that is named like one the file holds (as written or as R11 writes it).
#[test]
fn a_change_of_a_style_sheet_the_file_holds_and_a_new_one_of_another_form_are_refused_in_words() {
    let base = one_shape();
    let n = u32::try_from(base.doc.styles.len()).unwrap();
    let mut changed = one_shape();
    let root = Arc::make_mut(changed.doc.styles.get_mut(0).unwrap());
    Arc::make_mut(&mut root.sheet).cells.insert(CellName::new("Tag"), Cell::constant(Value::from_text("x")));
    assert!(refusal(&changed).contains("a change of a style sheet the file holds"), "{}", refusal(&changed));
    for bad in [style_sheet(n + 3, "Theme", 0), style_sheet(n, "Theme", n), style_sheet(n, "Theme", 99), {
        let mut s = style_sheet(n, "Theme", 0);
        let sheet = Arc::make_mut(&mut s.sheet);
        sheet.links.fill = Some(1);
        s
    }] {
        let mut o = one_shape();
        push(&mut o, bad);
        assert!(refusal(&o).contains("a new style sheet that is not an empty common style"), "{}", refusal(&o));
    }
    // A name a style of the file has, as it is written (`Base`), and as R11 writes another (`Base 2` is `Base_202`: no, `Base_20_2`, which a file may hold).
    let mut o = open(
        &Drawing::a4()
            .common(r#"<style:style style:name="Base" style:family="graphic"/><style:style style:name="B_20_2" style:family="graphic"/>"#)
            .page(&custom("s1", "1cm")),
    );
    let m = u32::try_from(o.doc.styles.len()).unwrap();
    push(&mut o, style_sheet(m, "Base", 0));
    assert!(refusal(&o).contains("a new style sheet that is not an empty common style"), "a name the file holds");
    let mut o = open(&Drawing::a4().common(r#"<style:style style:name="B_20_2" style:family="graphic"/>"#).page(&custom("s1", "1cm")));
    let m = u32::try_from(o.doc.styles.len()).unwrap();
    push(&mut o, style_sheet(m, "B 2", 0));
    assert!(refusal(&o).contains("a new style sheet that is not an empty common style"), "a name R11 writes as one the file holds");
    // A change to a style of the file is told as it is: the words of the old refusal, with the name of what changed.
}

/// A package with no `styles.xml` has no place for the common style: refused in words, and nothing is written.
#[test]
fn a_package_with_no_styles_part_refuses_a_new_style_sheet_in_words() {
    let mut drawing = Drawing::a4().page(&custom("s1", "1cm"));
    drawing.no_styles = true;
    let mut o = open(&drawing);
    let n = u32::try_from(o.doc.styles.len()).unwrap();
    push(&mut o, style_sheet(n, "Theme", 0));
    assert!(refusal(&o).contains("no styles.xml"), "{}", refusal(&o));
}

fn file_with(entries: &str) -> Vec<u8> {
    let common = r##"<style:style style:name="Base" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#336699"/></style:style>"##;
    let mut drawing = Drawing::a4().common(common).page(&custom("s1", "1cm"));
    drawing.files.push((
        PATH.to_owned(),
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><diacraft:document xmlns:diacraft="urn:app.diacraft:extension" version="2">{entries}</diacraft:document>"#
        )
        .into_bytes(),
    ));
    drawing.bytes()
}

/// What the reader does with the `style` entries of a document file: an entry is put over the sheet of the style of that name (its cells and sections replace the
/// sheet's, a `gone` token takes one the markup gave out, `name-u` gives the `NameU` back); an entry for no style, or for the root, is a warning and nothing else.
#[test]
fn the_entries_of_the_document_file_for_styles_are_put_over_the_style_of_that_name() {
    let base_of = |doc: &diacraft_doc::Document| doc.styles.iter().find(|s| s.name.as_deref() == Some("Base")).expect("the common style").clone();
    // The markup alone: the fill the style's properties give.
    let (doc, _) = read(&file_with(""), &Limits::default()).unwrap();
    assert!(base_of(&doc).sheet.cell("FillForegnd").is_some());
    let entry = r#"<diacraft:style name="Base" name-u="Base style" gone="c.FillForegnd"><diacraft:sheet><diacraft:Cell N="ThemeIndex" V="65534"/><diacraft:Cell N="Tag" V="t" F="1+1"/></diacraft:sheet></diacraft:style>"#;
    let (doc, report) = read(&file_with(entry), &Limits::default()).unwrap();
    let base = base_of(&doc);
    assert_eq!(base.name_u.as_deref(), Some("Base style"), "the NameU the entry gives back");
    assert_eq!(base.sheet.cell("ThemeIndex").and_then(|c| c.value.as_ref()).map(Value::text), Some("65534"));
    assert_eq!(base.sheet.cell("Tag").and_then(|c| c.formula.text()), Some("1+1"));
    assert!(base.sheet.cell("FillForegnd").is_none(), "a cell the entry says is gone is taken out of what the markup gave");
    assert_eq!(report.warnings, Vec::<String>::new());
    // No style of that name, and the root: a warning each, and the root is as it was.
    let nothing = r#"<diacraft:style name="Nothing"/><diacraft:style name="No Style"><diacraft:sheet><diacraft:Cell N="Tag" V="t"/></diacraft:sheet></diacraft:style>"#;
    let (doc, report) = read(&file_with(nothing), &Limits::default()).unwrap();
    assert_eq!(report.warning_counts.get("diacraft_style_unknown"), Some(&2), "{:?}", report.warning_counts);
    assert!(doc.styles.get(0).unwrap().sheet.cell("Tag").is_none(), "the root is as it was");
}
