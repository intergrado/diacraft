//! A new drawing's window and font table, which the acceptance test found wrong: our own saved files reopened with the grid and the
//! rulers off, because `windows.xml` held no `Window`, and `document.xml` held an empty `FaceNames`, which no file in the sample files have.
//!
//! Every input is built in code. The window is read here the way the interface reads it (`window_settings.rs` of `ui-egui`: the drawing
//! window, or the first, and a flag as a child element or an attribute), so that "the grid and the rulers are on" is what a reopen shows.

use std::collections::BTreeMap;
use std::sync::Arc;

use diacraft_doc::{CellAddr, CellEdit, Document, FaceName, Owner, RawAttrs, RowKey, ShapeRef, SheetRef, Value, WriteOrigin};
use diacraft_opc::xml::{self, El};
use diacraft_opc::{Limits, Package};
use diacraft_testkit::build::{DocBuilder, MasterBuilder, PageBuilder, ShapeBuilder, StyleBuilder};
use diacraft_testkit::package::{PageSpec, Pkg, VsdxBuilder, shape};
use diacraft_vsdx::{OriginalBytes, WriteOptions, Written, read, write_with};

use crate::read_ok;

const NOW: i64 = 1_700_000_000;
const WINDOWS_TYPE: &str = "http://schemas.microsoft.com/visio/2010/relationships/windows";
const WINDOWS_CT: &str = "application/vnd.ms-visio.windows+xml";

fn write_new(doc: &Document) -> Written {
    write_with(doc, None, &WriteOptions { now: Some(NOW), ..WriteOptions::default() }).unwrap_or_else(|e| panic!("{e}"))
}

fn save(doc: &Document, original: &[u8], baseline: &Document) -> Written {
    let original = OriginalBytes { bytes: original, baseline: Some(baseline) };
    write_with(doc, Some(&original), &WriteOptions { now: Some(NOW), ..WriteOptions::default() }).unwrap_or_else(|e| panic!("{e}"))
}

fn parts(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let pkg = Package::read(bytes, &Limits::default()).unwrap();
    pkg.parts().map(|p| (p.to_owned(), pkg.part(p).unwrap().to_vec())).collect()
}

fn part(bytes: &[u8], name: &str) -> Vec<u8> {
    parts(bytes).get(name).unwrap_or_else(|| panic!("no part {name}")).clone()
}

// ----------------------------------------------------------------------------------------------------------------------- the window

/// The root of the windows part, found through the relationships as a reader does.
fn windows_root(bytes: &[u8]) -> El {
    let package = Package::read(bytes, &Limits::default()).unwrap();
    let top = package.rels("/").unwrap();
    let document = top.items.iter().find(|r| r.ty.ends_with("/document")).unwrap().target.clone().unwrap();
    let rels = package.rels(&document).unwrap();
    let windows = rels.items.iter().find(|r| r.ty.ends_with("/windows")).unwrap().target.clone().unwrap();
    xml::parse(package.part(&windows).unwrap()).unwrap()
}

/// `(ShowGrid, ShowRulers)` as `window_settings` of `ui-egui` reads them: the window marked `Drawing`, else the first; a flag is a child's
/// text or an attribute, `1` or `true` for on and `0` or `false` for off.
fn flags(bytes: &[u8]) -> (Option<bool>, Option<bool>) {
    fn flag(window: &El, name: &str) -> Option<bool> {
        let text = window.child(name).map(El::text).or_else(|| window.attr(name).map(str::to_owned))?;
        match text.trim() {
            "1" | "true" => Some(true),
            "0" | "false" => Some(false),
            _ => None,
        }
    }
    let root = windows_root(bytes);
    let drawing = root.children_named("Window").find(|w| w.attr("WindowType").is_some_and(|t| t.eq_ignore_ascii_case("Drawing")));
    match drawing.or_else(|| root.children_named("Window").next()) {
        Some(window) => (flag(window, "ShowGrid"), flag(window, "ShowRulers")),
        None => (None, None),
    }
}

#[test]
fn a_new_drawing_reopens_with_the_grid_and_the_rulers_on() {
    let written = write_new(&Document::new_blank());
    assert_eq!(flags(&written.bytes), (Some(true), Some(true)));
    // The drawing window opens the first page: one window, of the type `Drawing`, for a `Page` container.
    let root = windows_root(&written.bytes);
    assert_eq!((root.attr("ClientWidth"), root.attr("ClientHeight")), (Some("1024"), Some("768")));
    let windows: Vec<&El> = root.children_named("Window").collect();
    assert_eq!(windows.len(), 1);
    let window = windows[0];
    assert_eq!(window.attr("WindowType"), Some("Drawing"));
    assert_eq!(window.attr("ContainerType"), Some("Page"));
    assert_eq!(window.attr("Page"), Some("1"), "page 1 of a new drawing");
    assert_eq!(window.attr("ID"), Some("0"));
    // The whole page, around its middle (US Letter landscape: 11 by 8.5 inches).
    assert_eq!(window.attr("ViewScale"), Some("-1"));
    assert_eq!((window.attr("ViewCenterX"), window.attr("ViewCenterY")), (Some("5.5"), Some("4.25")));
    // `Document` holds a file path, so a new window carries none; nothing names a master or a stencil.
    for absent in ["Document", "Master", "ParentWindow", "Container", "Sheet", "ReadOnly"] {
        assert_eq!(window.attr(absent), None, "{absent}");
    }
    assert!(written.read_back.warnings.is_empty(), "{:?}", written.read_back.warnings);
    // The part reads back as the same drawing, and a reader that looks at the window finds the same flags in the saved bytes of a second save.
    assert_eq!(read_ok(&written.bytes).0.pages.len(), 1);
}

#[test]
fn the_window_is_for_the_first_page_and_stands_in_the_middle_of_its_sheet() {
    // The first page in order, not the lowest `ID`; a sheet with a size gives the centre in drawing units; page `ID` 0 is an `ID` like any.
    let doc = DocBuilder::new()
        .page(7, PageBuilder::new().cell("PageWidth", "8").cell("PageHeight", "6"))
        .page(2, PageBuilder::new().cell("PageWidth", "30").cell("PageHeight", "20"))
        .build();
    let root = windows_root(&write_new(&doc).bytes);
    let window = root.children_named("Window").next().unwrap();
    assert_eq!((window.attr("Page"), window.attr("ViewCenterX"), window.attr("ViewCenterY")), (Some("7"), Some("4"), Some("3")));

    // No size on the sheet: no centre is written (the whole page fits, and a reader takes the middle).
    let doc = DocBuilder::new().page(0, PageBuilder::new()).build();
    let root = windows_root(&write_new(&doc).bytes);
    let window = root.children_named("Window").next().unwrap();
    assert_eq!(window.attr("Page"), Some("0"));
    assert_eq!((window.attr("ViewCenterX"), window.attr("ViewCenterY")), (None, None));
    assert_eq!(flags(&write_new(&doc).bytes), (Some(true), Some(true)));
}

/// A new drawing keeps its Page Breaks box in DiaCraft's own data, and its VSDX file says it where the format has a place for it, the drawing
/// window's `ShowPageBreaks`, after `ShowGrid`. A box that is off writes nothing, and the file reads back with the box as the drawing had it.
#[test]
fn a_new_drawing_whose_page_breaks_box_is_on_writes_show_page_breaks_in_its_window() {
    let mut doc = DocBuilder::new().page(0, PageBuilder::new()).build();
    let names =
        |bytes: &[u8]| -> Vec<String> { windows_root(bytes).children_named("Window").next().unwrap().elements().map(|e| e.name.clone()).collect() };
    assert_eq!(names(&write_new(&doc).bytes), ["ShowRulers", "ShowGrid"], "off: nothing is written");
    assert!(doc.set_diacraft_child(diacraft_doc::PAGE_BREAKS_CHILD, Some(&[("on", "1")])));
    let written = write_new(&doc);
    assert_eq!(names(&written.bytes), ["ShowRulers", "ShowGrid", "ShowPageBreaks"]);
    let (back, _) = read_ok(&written.bytes);
    assert_eq!(back.window_flags().and_then(|f| f.page_breaks), Some(true), "the window says it");
    assert_eq!(flags(&written.bytes), (Some(true), Some(true)), "and the grid and the rulers are as they were");
}

#[test]
fn a_drawing_with_no_page_has_a_windows_part_with_no_window() {
    // E1 of the document spec: a `Windows` with no `Window` is normal, and there is no page for a window to show.
    let written = write_new(&DocBuilder::new().build());
    assert_eq!(windows_root(&written.bytes).children_named("Window").count(), 0);
    assert_eq!(flags(&written.bytes), (None, None));
}

#[test]
fn a_file_that_was_read_keeps_its_windows_part_byte_for_byte() {
    // A file whose window has the grid and the rulers off, saved with an edit: the part is the original's, and the flags are still off.
    let windows = r#"<Windows ClientWidth="1" ClientHeight="1"><Window ID="0" WindowType="Drawing" Page="1"><ShowRulers>0</ShowRulers><ShowGrid>0</ShowGrid></Window></Windows>"#;
    let original = VsdxBuilder::new()
        .page(PageSpec::new(1, "First").shapes(&shape(5, r#"<Cell N="PinX" V="1"/>"#)))
        .with(Pkg::new().part("/visio/windows.xml", WINDOWS_CT, windows).rel("/visio/document.xml", "rId90", WINDOWS_TYPE, "windows.xml"))
        .build();
    let (mut doc, _) = read_ok(&original);
    let baseline = doc.clone();
    let sheet = SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0));
    doc.write_cell(&sheet, &CellAddr::direct("PinX"), CellEdit::Value(Value::from_text("7")), WriteOrigin::Drawing).unwrap();
    let written = save(&doc, &original, &baseline);
    assert_ne!(part(&written.bytes, "/visio/pages/page1.xml"), part(&original, "/visio/pages/page1.xml"), "the edit was written");
    assert_eq!(part(&written.bytes, "/visio/windows.xml"), part(&original, "/visio/windows.xml"));
    assert_eq!(flags(&written.bytes), (Some(false), Some(false)));
    assert!(!written.rewritten.iter().any(|p| p.contains("windows")) && !written.added.iter().any(|p| p.contains("windows")));

    // A file with no windows part at all is not given one by a save.
    let bare = VsdxBuilder::new().page(PageSpec::new(1, "First").shapes(&shape(5, r#"<Cell N="PinX" V="1"/>"#))).build();
    let (mut doc, _) = read_ok(&bare);
    let baseline = doc.clone();
    doc.write_cell(&sheet, &CellAddr::direct("PinX"), CellEdit::Value(Value::from_text("7")), WriteOrigin::Drawing).unwrap();
    let written = save(&doc, &bare, &baseline);
    assert_eq!(parts(&written.bytes).contains_key("/visio/windows.xml"), parts(&bare).contains_key("/visio/windows.xml"));
}

#[test]
fn a_new_drawing_saved_again_keeps_the_window_it_was_first_saved_with() {
    // The engine's second save: the session's own document, the file just written, and the baseline the first save returned.
    let doc = Document::new_blank();
    let first = write_new(&doc);
    let mut edited = doc.clone();
    edited.add_page(Some("Second")).unwrap();
    let second = save(&edited, &first.bytes, &first.document);
    assert_eq!(part(&second.bytes, "/visio/windows.xml"), part(&first.bytes, "/visio/windows.xml"));
    assert_eq!(flags(&second.bytes), (Some(true), Some(true)));
    assert_eq!(read_ok(&second.bytes).0.pages.len(), 2);
}

// -------------------------------------------------------------------------------------------------------------------- the font table

/// The `FaceNames` of a `document.xml`, after the check a schema would make of it: the part's children come in the order of
/// `FaceNames` is there once with at least one `FaceName` (MS-VSDX asks for one), and every `FaceName` has a `NameU`
/// that is not empty. Returns the names.
fn font_table(document_xml: &[u8]) -> Vec<String> {
    const ORDER: [&str; 9] = [
        "DocumentSettings",
        "Colors",
        "FaceNames",
        "StyleSheets",
        "DocumentSheet",
        "EventList",
        "HeaderFooter",
        "DataTransferInfo",
        "PublishSettings",
    ];
    let root = xml::parse(document_xml).unwrap();
    let children: Vec<&El> = root.elements().collect();
    let mut at = 0;
    for child in &children {
        let name = child.name.rsplit(':').next().unwrap();
        let place = ORDER.iter().position(|n| *n == name).unwrap_or_else(|| panic!("an element R8 does not list: {name}"));
        assert!(place >= at, "{name} is out of order");
        at = place + 1;
    }
    let tables: Vec<&&El> = children.iter().filter(|c| c.name.ends_with("FaceNames")).collect();
    assert_eq!(tables.len(), 1, "FaceNames, once");
    let faces: Vec<&El> = tables[0].elements().collect();
    assert!(!faces.is_empty(), "an empty FaceNames fails the schema: MS-VSDX wants at least one FaceName");
    faces
        .iter()
        .map(|f| {
            assert!(f.name.ends_with("FaceName"), "{}", f.name);
            let name = f.attr("NameU").unwrap_or_else(|| panic!("a FaceName without NameU"));
            assert!(!name.is_empty());
            name.to_owned()
        })
        .collect()
}

fn character_row(i: u32, cell: &str, name: &str) -> impl FnOnce(ShapeBuilder) -> ShapeBuilder {
    let (cell, name) = (cell.to_owned(), name.to_owned());
    move |s| s.row_cell("Character", None, RowKey::Ix(i), &cell, &name)
}

#[test]
fn a_new_drawings_font_table_is_not_empty_and_the_schema_like_check_holds_for_the_whole_part() {
    let written = write_new(&Document::new_blank());
    let names = font_table(&part(&written.bytes, "/visio/document.xml"));
    assert_eq!(names.len(), 1, "no cell names a face, so the one default face");
    // The reader sees the table; the model the writer hands back is the caller's own (an empty table), as it is for the unknown elements.
    assert_eq!(read_ok(&written.bytes).0.fonts.iter().map(|f| f.name_u.to_string()).collect::<Vec<_>>(), names);
    assert!(written.document.fonts.is_empty());
    assert!(written.read_back.warnings.is_empty(), "{:?}", written.read_back.warnings);
}

#[test]
fn the_faces_the_cells_name_are_in_the_table_and_the_markers_are_not() {
    let shape_with_faces = character_row(0, "Font", "Alpha Face")(ShapeBuilder::new(1));
    let shape_with_faces = character_row(0, "AsianFont", "0")(shape_with_faces);
    let shape_with_faces = character_row(1, "ComplexScriptFont", "Themed")(shape_with_faces);
    let shape_with_faces = character_row(2, "Font", "")(shape_with_faces);
    let member = character_row(0, "Font", "Gamma Face")(ShapeBuilder::new(3));
    let group = ShapeBuilder::new(2).group().child(member);
    let bullet = ShapeBuilder::new(4).row_cell("Paragraph", None, RowKey::Ix(0), "BulletFont", "Delta Face");
    let master = MasterBuilder::new().name("M").shape(character_row(0, "Font", "Beta Face")(ShapeBuilder::new(1)));
    let style = StyleBuilder::new().name("S").row_cell("Character", None, RowKey::Ix(0), "Font", "Zeta Face");
    let doc =
        DocBuilder::new().style(3, style).master(1, master).page(1, PageBuilder::new().shape(shape_with_faces).shape(group).shape(bullet)).build();
    let written = write_new(&doc);
    let names = font_table(&part(&written.bytes, "/visio/document.xml"));
    assert_eq!(
        names,
        ["Alpha Face", "Beta Face", "Delta Face", "Gamma Face", "Zeta Face"],
        "in name order; no `0`, `Themed` or empty name; no default face"
    );
    // The model's table was empty and stays so in the model the writer returns (the next save must not take the file's longer table for an edit).
    assert!(written.document.fonts.is_empty());
}

#[test]
fn the_models_own_table_comes_first_as_it_is_and_a_name_in_it_is_not_added_twice() {
    let alpha = FaceName {
        name_u: Arc::from("Alpha Face"),
        raw: RawAttrs::from_pairs([("Panose", "2 15 5 2 2 2 4 3 2 4"), ("Flags", "1")]),
        ext: Default::default(),
    };
    let mut doc = DocBuilder::new().page(1, PageBuilder::new().shape(character_row(0, "Font", "Alpha Face")(ShapeBuilder::new(1)))).build();
    doc.fonts = Arc::new(vec![alpha.clone()]);
    let written = write_new(&doc);
    assert_eq!(font_table(&part(&written.bytes, "/visio/document.xml")), ["Alpha Face"]);
    let read_back = read_ok(&written.bytes).0;
    assert_eq!(read_back.fonts.as_ref(), &vec![alpha]);

    // With a cell that names another face: the model's row first, then the added one.
    let mut doc = DocBuilder::new().page(1, PageBuilder::new().shape(character_row(0, "Font", "Beta Face")(ShapeBuilder::new(1)))).build();
    doc.fonts = Arc::new(vec![FaceName { name_u: Arc::from("Zed Face"), raw: RawAttrs::new(), ext: Default::default() }]);
    let written = write_new(&doc);
    assert_eq!(font_table(&part(&written.bytes, "/visio/document.xml")), ["Zed Face", "Beta Face"]);
    assert_eq!(written.document.fonts, doc.fonts, "the baseline is the caller's table");
}

#[test]
fn a_saved_new_drawing_saved_again_does_not_empty_or_shrink_its_font_table() {
    // The engine's flow: the session's document never changes to the file's, and the baseline is what the first save returned.
    let mut doc = DocBuilder::new().page(1, PageBuilder::new().shape(character_row(0, "Font", "Beta Face")(ShapeBuilder::new(1)))).build();
    let first = write_new(&doc);
    let table = font_table(&part(&first.bytes, "/visio/document.xml"));
    assert_eq!(table, ["Beta Face"]);
    // No edit: the document part is the file's own bytes.
    let again = save(&doc, &first.bytes, &first.document);
    assert_eq!(part(&again.bytes, "/visio/document.xml"), part(&first.bytes, "/visio/document.xml"));
    // An edit elsewhere: the same.
    doc.add_page(None).unwrap();
    let again = save(&doc, &first.bytes, &first.document);
    assert_eq!(part(&again.bytes, "/visio/document.xml"), part(&first.bytes, "/visio/document.xml"));
    assert_eq!(font_table(&part(&again.bytes, "/visio/document.xml")), table);
    // A table the session then gives itself replaces the file's, as an edit of the table does for a file that was read.
    doc.fonts = Arc::new(vec![FaceName { name_u: Arc::from("Late Face"), raw: RawAttrs::new(), ext: Default::default() }]);
    let later = save(&doc, &first.bytes, &first.document);
    assert_eq!(font_table(&part(&later.bytes, "/visio/document.xml")), ["Late Face"]);

    // A new drawing with nothing that names a face, saved twice.
    let blank = Document::new_blank();
    let first = write_new(&blank);
    let again = save(&blank, &first.bytes, &first.document);
    assert_eq!(part(&again.bytes, "/visio/document.xml"), part(&first.bytes, "/visio/document.xml"));
    assert_eq!(font_table(&part(&again.bytes, "/visio/document.xml")).len(), 1);
}

#[test]
fn a_file_that_was_read_keeps_its_font_table_even_when_it_is_empty() {
    // A file whose `FaceNames` has no row is the file's, and a save does not give it one.
    let original = VsdxBuilder::new().page(PageSpec::new(1, "P").shapes(&shape(5, r#"<Cell N="PinX" V="1"/>"#))).build();
    let before = part(&original, "/visio/document.xml");
    let (mut doc, _) = read_ok(&original);
    let baseline = doc.clone();
    let sheet = SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0));
    doc.write_cell(&sheet, &CellAddr::direct("PinX"), CellEdit::Value(Value::from_text("7")), WriteOrigin::Drawing).unwrap();
    let written = save(&doc, &original, &baseline);
    assert_eq!(part(&written.bytes, "/visio/document.xml"), before);
}

#[test]
fn the_names_a_drawing_adds_to_the_table_are_capped() {
    // 1,500 faces named by the rows of one shape: the table holds the model's rows, none, and at most 1,024 of these.
    let mut shape = ShapeBuilder::new(1);
    for i in 0..1_500u32 {
        shape = shape.row_cell("Character", None, RowKey::Ix(i), "Font", &format!("Face {i:04}"));
    }
    let doc = DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build();
    let written = write_new(&doc);
    let names = font_table(&part(&written.bytes, "/visio/document.xml"));
    assert_eq!(names.len(), 1_024);
    assert_eq!(names.first().map(String::as_str), Some("Face 0000"));
    // And the drawing reads back whole.
    assert_eq!(read(&written.bytes, &diacraft_vsdx::Limits::default()).unwrap().0.pages.len(), 1);
}
