//! A save into the file after an edit of rows and sections (`sheet.add_row`, `sheet.delete_row`, `sheet.add_section`, `sheet.delete_section`): **a row or a section that was
//! numbered again goes on being the element that held it** (its unknown attribute, element and comment with it), **and a second save in the session is no refusal** when the first left
//! the count of typed children before an unknown node as it was while the file counts again.
//!
//! Every input is built in code, and every edit is made on the model the way the commands make it.

use std::sync::Arc;

use diacraft_doc::{Cell, CellAddr, CellEdit, Document, Owner, Row, SheetRef, Value, WriteOrigin};
use diacraft_testkit::package::{MasterSpec, PageSpec, VsdxBuilder, shape};
use diacraft_vsdx::{OriginalBytes, WriteOptions, Written, write_with};

use crate::read_ok;

const PAGE: &str = "/visio/pages/page1.xml"; // brand-ok: a part name the format defines
const DOCUMENT: &str = "/visio/document.xml"; // brand-ok: a part name the format defines

fn options() -> WriteOptions {
    WriteOptions { now: Some(1_700_000_000), ..WriteOptions::default() }
}

/// The text of a part of a package.
fn text_of(bytes: &[u8], name: &str) -> String {
    let package = diacraft_opc::Package::read(bytes, &diacraft_opc::Limits::default()).unwrap();
    String::from_utf8(package.part(name).unwrap_or_else(|| panic!("no part {name}")).to_vec()).unwrap()
}

/// A save into the file `original`, from the model `edited`, with `baseline` as the model the file was read as.
fn save_into(edited: &Document, original: &[u8], baseline: &Document) -> Result<Written, String> {
    let original = OriginalBytes { bytes: original, baseline: Some(baseline) };
    write_with(edited, Some(&original), &options()).map_err(|e| e.to_string())
}

fn shape_sheet_mut(doc: &mut Document, id: u32) -> &mut diacraft_doc::Sheet {
    let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
    let found = page.shapes.iter().position(|s| s.id == id).unwrap_or_else(|| panic!("no shape {id}"));
    let shape = Arc::make_mut(page.shapes.get_mut(found).unwrap());
    Arc::make_mut(&mut shape.sheet)
}

fn row_with(ix: Option<u32>, name: Option<&str>, cell: &str, value: &str) -> Row {
    let mut row = Row { ix, name: name.map(Arc::from), ..Row::default() };
    row.cells.insert(cell.into(), Cell::read(None, Some(value), None, None, &mut 0));
    row
}

/// A page whose shape 7 holds a Scratch section of three rows, the last with an unknown attribute and an unknown element, a User section whose rows have a comment between them, and, after
/// its sections, an unknown element of the shape.
fn package() -> Vec<u8> {
    let inner = concat!(
        r#"<Cell N="Width" V="2"/>"#,
        r#"<Section N="Scratch">"#,
        r#"<Row IX="0"><Cell N="A" V="1"/></Row>"#,
        r#"<Row IX="1"><Cell N="A" V="2"/></Row>"#,
        r#"<Row IX="2" Zip="kept"><Cell N="A" V="3"/><x:Mark xmlns:x="urn:z" a="1"/></Row>"#,
        r#"</Section>"#,
        r#"<Section N="User"><Row N="a"><Cell N="Value" V="1"/></Row><!-- between --><Row N="b"><Cell N="Value" V="2"/></Row></Section>"#,
        r#"<x:Shape-level xmlns:x="urn:z" at="the end"/>"#
    );
    VsdxBuilder::new().page(PageSpec::new(1, "Page-1").shapes(&shape(7, inner))).build()
}

/// The Scratch row `ix` of shape 7 numbered `to`, by the way an insert numbers the rows after it again.
fn renumber(doc: &mut Document, from: u32, to: u32) {
    let sheet = shape_sheet_mut(doc, 7);
    let section = sheet.sections.iter_mut().find(|s| s.name.as_str() == "Scratch").map(Arc::make_mut).unwrap();
    for row in section.rows.iter_mut() {
        if row.ix == Some(from) {
            Arc::make_mut(row).ix = Some(to);
        }
    }
}

#[test]
fn a_row_numbered_again_by_an_insert_goes_on_holding_its_unknown_attribute_and_element() {
    let original = package();
    let (base, _) = read_ok(&original);
    let mut edited = base.clone();
    // An insert at 1: the rows 1 and 2 become 2 and 3, and a new row takes 1.
    renumber(&mut edited, 2, 3);
    renumber(&mut edited, 1, 2);
    let sheet = shape_sheet_mut(&mut edited, 7);
    let section = sheet.sections.iter_mut().find(|s| s.name.as_str() == "Scratch").map(Arc::make_mut).unwrap();
    section.rows.insert(1, Arc::new(row_with(Some(1), None, "A", "9")));
    let written = save_into(&edited, &original, &base).unwrap_or_else(|e| panic!("{e}"));
    let xml = text_of(&written.bytes, PAGE);
    assert!(xml.contains(r#"<Row IX="3" Zip="kept"><Cell N="A" V="3"/><x:Mark xmlns:x="urn:z" a="1"/></Row>"#), "{xml}");
    assert!(xml.contains(r#"<Row IX="1"><Cell N="A" V="9"/></Row>"#), "{xml}");
    // The file reads back the rows as they were numbered.
    let (back, _) = read_ok(&written.bytes);
    let page = back.pages.get(0).unwrap();
    let section = page.shapes.get(0).unwrap().sheet.sections.iter().find(|s| s.name.as_str() == "Scratch").cloned().unwrap();
    assert_eq!(section.rows.iter().map(|r| r.ix).collect::<Vec<_>>(), [Some(0), Some(1), Some(2), Some(3)]);
    assert_eq!(section.rows[3].passthrough.len(), 1, "the unknown element is in the row that held it");
}

#[test]
fn a_delete_then_an_insert_at_the_same_number_leaves_the_row_that_went_with_its_unknown_nodes_out_of_the_file() {
    let original = package();
    let (base, _) = read_ok(&original);
    let mut edited = base.clone();
    let sheet = shape_sheet_mut(&mut edited, 7);
    let section = sheet.sections.iter_mut().find(|s| s.name.as_str() == "Scratch").map(Arc::make_mut).unwrap();
    section.rows.pop();
    section.rows.push(Arc::new(row_with(Some(2), None, "A", "8")));
    let written = save_into(&edited, &original, &base).unwrap_or_else(|e| panic!("{e}"));
    let xml = text_of(&written.bytes, PAGE);
    assert!(xml.contains(r#"<Row IX="2"><Cell N="A" V="8"/></Row>"#), "{xml}");
    assert!(!xml.contains("Zip") && !xml.contains("x:Mark"), "{xml}");
}

#[test]
fn a_second_save_after_an_insert_before_an_unknown_node_is_no_refusal_and_keeps_the_node_where_the_file_has_it() {
    let original = package();
    let (base, _) = read_ok(&original);
    // First edit: a section and a row (typed children before the unknown element of the shape, and before the comment of the User section).
    let mut first = base.clone();
    let sheet = shape_sheet_mut(&mut first, 7);
    sheet.sections.push(Arc::new(diacraft_doc::Section::new("Hyperlink", None)));
    let user = sheet.sections.iter_mut().find(|s| s.name.as_str() == "User").map(Arc::make_mut).unwrap();
    user.rows.insert(0, Arc::new(row_with(None, Some("first"), "Value", "0")));
    let one = save_into(&first, &original, &base).unwrap_or_else(|e| panic!("{e}"));
    let xml = text_of(&one.bytes, PAGE);
    assert!(xml.contains("<!-- between -->") && xml.contains(r#"<x:Shape-level xmlns:x="urn:z" at="the end"/>"#), "{xml}");

    // The session holds the model it edited and the file read back is the baseline of the next save: they hold the same unknown nodes, counted after different numbers of typed children.
    let mut second = first.clone();
    let at = SheetRef::Shape(second.find_shape(Owner::Page(1), 7).unwrap());
    second
        .write_cell(&at, &CellAddr::direct("Width"), CellEdit::Value(Value::from_text("5")), WriteOrigin::Drawing)
        .unwrap_or_else(|e| panic!("{e}"));
    let two = save_into(&second, &one.bytes, &one.document).unwrap_or_else(|e| panic!("the second save: {e}"));
    let xml = text_of(&two.bytes, PAGE);
    assert!(xml.contains(r#"<Cell N="Width" V="5"/>"#), "{xml}");
    assert!(xml.contains("<!-- between -->") && xml.contains(r#"<x:Shape-level xmlns:x="urn:z" at="the end"/>"#), "{xml}");
    let (back, _) = read_ok(&two.bytes);
    assert_eq!(back.pages.get(0).unwrap().shapes.get(0).unwrap().sheet.sections.len(), 3);
}

#[test]
fn a_second_save_after_a_row_and_a_cell_of_a_style_and_a_master_is_no_refusal() {
    let style = concat!(
        r#"<StyleSheet ID="3" NameU="Plain" Name="Plain" LineStyle="0" FillStyle="0" TextStyle="0">"#,
        r#"<Cell N="LineWeight" V="0.01"/><x:Style-level xmlns:x="urn:z"/>"#,
        r#"<Section N="User"><Row N="a"><Cell N="Value" V="1"/></Row></Section>"#,
        r#"</StyleSheet>"#
    );
    let master =
        shape(5, r#"<Cell N="Width" V="2"/><Section N="User"><Row N="a"><Cell N="Value" V="1"/></Row></Section><x:Master-level xmlns:x="urn:z"/>"#);
    let original = VsdxBuilder::new()
        .more_styles(style)
        .master(MasterSpec::new(1, "Box").shapes(&master))
        .page(PageSpec::new(1, "Page-1").shapes(&shape(7, r#"<Cell N="Width" V="2"/>"#)))
        .build();
    let (base, _) = read_ok(&original);
    let mut first = base.clone();
    // A new named row in the style's User section and in the master shape's, and a new cell in each.
    let style_at = first.styles.iter().position(|s| s.id == 3).unwrap();
    let style = Arc::make_mut(first.styles.get_mut(style_at).unwrap());
    let sheet = Arc::make_mut(&mut style.sheet);
    sheet.cells.insert("LineColor".into(), Cell::read(None, Some("2"), None, None, &mut 0));
    let user = sheet.sections.iter_mut().find(|s| s.name.as_str() == "User").map(Arc::make_mut).unwrap();
    user.rows.push(Arc::new(row_with(None, Some("b"), "Value", "2")));
    let m = Arc::make_mut(first.masters.get_mut(0).unwrap());
    let shape = Arc::make_mut(m.shapes.get_mut(0).unwrap());
    let sheet = Arc::make_mut(&mut shape.sheet);
    sheet.cells.insert("Height".into(), Cell::read(None, Some("1"), None, None, &mut 0));
    let user = sheet.sections.iter_mut().find(|s| s.name.as_str() == "User").map(Arc::make_mut).unwrap();
    user.rows.push(Arc::new(row_with(None, Some("b"), "Value", "2")));
    let one = save_into(&first, &original, &base).unwrap_or_else(|e| panic!("the first save: {e}"));
    assert!(text_of(&one.bytes, DOCUMENT).contains(r#"<Row N="b">"#));
    // The second edit: another cell of each.
    let mut second = first.clone();
    let style_at = second.styles.iter().position(|s| s.id == 3).unwrap();
    let style = Arc::make_mut(second.styles.get_mut(style_at).unwrap());
    Arc::make_mut(&mut style.sheet).cells.insert("FillForegnd".into(), Cell::read(None, Some("3"), None, None, &mut 0));
    let m = Arc::make_mut(second.masters.get_mut(0).unwrap());
    let shape = Arc::make_mut(m.shapes.get_mut(0).unwrap());
    Arc::make_mut(&mut shape.sheet).cells.insert("LocPinX".into(), Cell::read(None, Some("1"), None, None, &mut 0));
    let two = save_into(&second, &one.bytes, &one.document).unwrap_or_else(|e| panic!("the second save: {e}"));
    assert!(text_of(&two.bytes, DOCUMENT).contains("FillForegnd"));
    assert!(text_of(&two.bytes, DOCUMENT).contains("x:Style-level"), "the unknown element of the style is kept");
}
