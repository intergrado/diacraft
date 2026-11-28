//! **`IX` on a section appears only on `Geometry`, and always there.** In the corpus 89,008 `Connection`
//! sections, in 1,412 files, none carry it. So a writer writes no `IX` on any other section, a `Connection` section in particular, also on an instance whose rows are a part of its master's
//! and merge with them by their own row `IX`; the rows keep theirs.
//!
//! A section a save does not reach keeps whatever the file has (a plain save changes nothing that was not edited, and a visit gives back what the file held: the model keeps every attribute it has
//! no field for); a section it writes, a new one or one an edit reached, has none.
//! Every input is built in code.

use std::sync::Arc;

use diacraft_doc::{CellAddr, CellEdit, Document, Owner, RowKey, SheetRef, Value, WriteOrigin};
use diacraft_testkit::package::{MasterSpec, PageSpec, VsdxBuilder, cell, shape, shape_with};
use diacraft_vsdx::{OriginalBytes, WriteOptions, write_with};

use crate::model_writer::{model_write, text_of};
use crate::read_ok;

const PAGE: &str = "/visio/pages/page1.xml"; // brand-ok: a part name the format defines

fn connection_rows(rows: &[(u32, &str)]) -> String {
    rows.iter().map(|(ix, x)| format!("<Row IX=\"{ix}\">{}{}</Row>", cell("X", x), cell("Y", "0"))).collect()
}

fn geometry() -> String {
    format!(
        "<Section N=\"Geometry\" IX=\"0\"><Row T=\"MoveTo\" IX=\"1\">{}{}</Row><Row T=\"LineTo\" IX=\"2\">{}{}</Row></Section>",
        cell("X", "0"),
        cell("Y", "0"),
        cell("X", "1"),
        cell("Y", "1")
    )
}

/// A drawing whose master shape holds a `Connection` section of four rows, and a page with two instances of it: `ID` 1 holds two of the rows (`IX` 1 and 3) changed, in a section written as
/// `section_attrs` says (nothing, or ` IX="0"` the way a file of another writer might); `ID` 2 holds its own `Connection` section too, which no test edits.
fn package(section_attrs: &str) -> Vec<u8> {
    let master_shape =
        shape(4, &format!("{}<Section N=\"Connection\">{}</Section>", geometry(), connection_rows(&[(0, "0"), (1, "1"), (2, "2"), (3, "3")])));
    let own =
        |x1: &str, x3: &str| format!("{}<Section N=\"Connection\"{section_attrs}>{}</Section>", geometry(), connection_rows(&[(1, x1), (3, x3)]));
    let page = format!("{}{}", shape_with(1, " Master=\"1\"", &own("5", "6")), shape_with(2, " Master=\"1\"", &own("7", "8")));
    VsdxBuilder::new().master(MasterSpec::new(1, "Box").shapes(&master_shape)).page(PageSpec::new(1, "Page-1").shapes(&page)).build()
}

/// Writes `x` into the `X` cell of the row `ix` of shape `shape_id`'s `Connection` section.
fn set_x(doc: &mut Document, shape_id: u32, ix: u32, x: &str) {
    let at = doc.find_shape(Owner::Page(1), shape_id).unwrap_or_else(|| panic!("no shape {shape_id}"));
    doc.write_cell(
        &SheetRef::Shape(at),
        &CellAddr::in_row("Connection", None, RowKey::Ix(ix), "X"),
        CellEdit::Value(Value::from_text(x)),
        WriteOrigin::Drawing,
    )
    .unwrap_or_else(|e| panic!("{e}"));
}

fn save_into(doc: &Document, original: &[u8], baseline: &Document) -> Vec<u8> {
    let original = OriginalBytes { bytes: original, baseline: Some(baseline) };
    write_with(doc, Some(&original), &WriteOptions { now: Some(1_700_000_000), ..WriteOptions::default() }).unwrap_or_else(|e| panic!("{e}")).bytes
}

/// The text of each `<Section N="Connection" ...>` start tag of a page part.
fn connection_tags(xml: &str) -> Vec<String> {
    xml.match_indices("<Section N=\"Connection\"").map(|(at, _)| xml[at..].split('>').next().unwrap_or("").to_owned()).collect()
}

/// Every `Geometry` section start tag holds an `IX`.
fn geometry_keeps_ix(xml: &str) -> bool {
    let all = xml.matches("<Section N=\"Geometry\"").count();
    all > 0 && all == xml.matches("<Section N=\"Geometry\" IX=\"").count()
}

#[test]
fn a_section_the_model_writer_makes_has_no_ix_but_geometry_and_the_rows_keep_theirs() {
    let (doc, _) = read_ok(&package(""));
    let written = model_write(&doc, false);
    let xml = text_of(&written.bytes, PAGE);
    let tags = connection_tags(&xml);
    assert_eq!(tags, ["<Section N=\"Connection\"", "<Section N=\"Connection\""], "{xml}");
    assert!(xml.contains("<Row IX=\"1\">") && xml.contains("<Row IX=\"3\">"), "{xml}");
    assert!(!xml.contains("<Row IX=\"0\">") && !xml.contains("<Row IX=\"2\">"), "the rows the master holds are not copied: {xml}");
    assert!(geometry_keeps_ix(&xml), "{xml}");
}

#[test]
fn a_visit_of_a_file_that_wrote_an_ix_on_a_section_gives_back_what_the_file_had() {
    // The file's instance writes `<Section N="Connection" IX="0">`. The model keeps every attribute it has no field for (a lossless read), and the model writer, which has no baseline to say
    // what an edit reached, gives it back, as every visit gives back what the file held; only a save that rewrites the section takes it off (the next tests). A section DiaCraft made holds
    // no such attribute (the first test).
    let original = package(" IX=\"0\"");
    let (doc, _) = read_ok(&original);
    let written = model_write(&doc, true);
    let xml = text_of(&written.bytes, PAGE);
    assert_eq!(connection_tags(&xml), ["<Section N=\"Connection\" IX=\"0\"", "<Section N=\"Connection\" IX=\"0\""], "{xml}");
    assert!(xml.contains("<Row IX=\"1\">") && xml.contains("<Row IX=\"3\">"), "{xml}");
    assert!(geometry_keeps_ix(&xml), "{xml}");
}

#[test]
fn a_section_a_save_into_the_file_reaches_loses_the_ix_and_a_section_it_does_not_reach_keeps_it() {
    let original = package(" IX=\"0\"");
    let (doc, _) = read_ok(&original);
    // A row's `X` changed on shape 1: its section is patched. Shape 2 is not edited.
    let mut edited = doc.clone();
    set_x(&mut edited, 1, 1, "9");
    let bytes = save_into(&edited, &original, &doc);
    let xml = text_of(&bytes, PAGE);
    let tags = connection_tags(&xml);
    assert_eq!(
        tags,
        ["<Section N=\"Connection\"", "<Section N=\"Connection\" IX=\"0\""],
        "the edited section has none, the one no edit reached is the file's: {xml}"
    );
    assert!(xml.contains(r#"<Cell N="X" V="9"/>"#), "{xml}");
    assert!(geometry_keeps_ix(&xml), "{xml}");
    // And the page reads back as edited, the rows by their own `IX`.
    let (back, _) = read_ok(&bytes);
    let at = back.find_shape(Owner::Page(1), 1).unwrap();
    let section = back.shape(&at).unwrap().sheet.sections.iter().find(|s| s.name.as_str() == "Connection").cloned().unwrap();
    assert_eq!(section.rows.iter().map(|r| r.ix).collect::<Vec<_>>(), [Some(1), Some(3)]);
}

#[test]
fn a_save_that_edits_a_section_a_file_wrote_without_ix_writes_none() {
    let original = package("");
    let (doc, _) = read_ok(&original);
    let mut edited = doc.clone();
    set_x(&mut edited, 1, 3, "4");
    let bytes = save_into(&edited, &original, &doc);
    let xml = text_of(&bytes, PAGE);
    assert_eq!(connection_tags(&xml), ["<Section N=\"Connection\"", "<Section N=\"Connection\""], "{xml}");
    assert!(xml.contains("<Row IX=\"1\">") && xml.contains("<Row IX=\"3\">"), "{xml}");
    assert!(geometry_keeps_ix(&xml), "{xml}");
}

#[test]
fn a_save_that_edits_nothing_changes_no_section_of_the_file() {
    let original = package(" IX=\"0\"");
    let (doc, _) = read_ok(&original);
    let bytes = save_into(&doc, &original, &doc);
    assert_eq!(text_of(&bytes, PAGE), text_of(&original, PAGE), "a plain save changes nothing that was not edited");
    assert!(connection_tags(&text_of(&bytes, PAGE)).iter().all(|tag| tag.contains("IX=\"0\"")));
}

#[test]
fn a_connection_section_the_model_holds_with_an_ix_of_its_own_is_not_written_with_it() {
    // A model that holds the number in its field (a section made in code with an `IX`, or read from a file by an older reader) writes none on a section that is not `Geometry`.
    let (mut doc, _) = read_ok(&package(""));
    let shape = Arc::make_mut(Arc::make_mut(doc.pages.get_mut(0).unwrap()).shapes.get_mut(0).unwrap());
    let sheet = Arc::make_mut(&mut shape.sheet);
    for section in sheet.sections.iter_mut() {
        let section = Arc::make_mut(section);
        if section.name.as_str() == "Connection" {
            section.ix = Some(2);
        }
    }
    let xml = text_of(&model_write(&doc, false).bytes, PAGE);
    assert!(!connection_tags(&xml).iter().any(|tag| tag.contains("IX")), "{xml}");
    assert!(geometry_keeps_ix(&xml), "{xml}");
}
