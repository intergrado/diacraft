//! **Background pages come first in the pages index.** In the 70 files of the corpus that hold both kinds, every
//! background page comes before every foreground one. So a VSDX writer that makes the pages index from the model (a drawing never saved, and a drawing written from the model alone) writes
//! the background pages first, the order inside each kind as the model has it; the page `ID`s, the names, `BackPage` and what each page holds go with their pages. **A save into a file
//! changes only what an edit changed**, so it keeps the order the file has, whatever it is, and a reader keeps the order it finds.
//!
//! Every input is built in code.

use std::sync::Arc;

use diacraft_doc::{Document, Page};
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};
use diacraft_testkit::package::{PageSpec, VsdxBuilder, cell, shape};
use diacraft_vsdx::{OriginalBytes, WriteOptions, Written, write_with};

use crate::model_writer::{model_write, package, page_cell, parts, text_of};
use crate::read_ok;

const NOW: i64 = 1_700_000_000;
/// The part of the pages index, and what every part of a page begins with (names the format defines).
const PAGES_INDEX: &str = "/visio/pages/pages.xml"; // brand-ok: a part name the format defines
const PAGE_PART: &str = "/visio/pages/page"; // brand-ok: a part name the format defines

/// A page as a reader finds it: its `ID`, name, `Background`, `BackPage` and the `ID`s of its top-level shapes.
type PageRow = (u32, String, bool, Option<u32>, Vec<u32>);

/// The pages as a reader finds them, in order.
fn pages_of(bytes: &[u8]) -> Vec<PageRow> {
    let (doc, _) = read_ok(bytes);
    doc.pages.iter().map(|p| (p.id, p.name.to_string(), p.background, p.back_page, p.shapes.iter().map(|s| s.id).collect())).collect()
}

fn names(bytes: &[u8]) -> Vec<String> {
    pages_of(bytes).into_iter().map(|p| p.1).collect()
}

/// A drawing whose pages run foreground, foreground, background, as a drawing made in a session and a file of another program may: page 1 `Front` (a shape 11), page 2 `Second`
/// (a shape 12) and page 3 `Back`, a background page (a shape 13) that `Front` names as its background.
fn out_of_order() -> Document {
    DocBuilder::new()
        .page(1, PageBuilder::new().name("Front").back_page(3).shape(ShapeBuilder::new(11)))
        .page(2, PageBuilder::new().name("Second").shape(ShapeBuilder::new(12)))
        .page(3, PageBuilder::new().name("Back").background().shape(ShapeBuilder::new(13)))
        .build()
}

/// The same drawing as a file whose pages index runs `Front`, `Back` (a background page `Front` names), the order a file of another program may have.
fn out_of_order_package() -> Vec<u8> {
    VsdxBuilder::new()
        .page(PageSpec::new(1, "Front").attrs(r#" BackPage="2""#).shapes(&shape(11, &cell("PinX", "1"))))
        .page(PageSpec::new(2, "Back").attrs(r#" Background="1""#).shapes(&shape(12, &cell("PinX", "2"))))
        .page(PageSpec::new(3, "Last").shapes(&shape(13, &cell("PinX", "3"))))
        .build()
}

/// The package the builder makes, with the first page written first and a background page after it.
fn foreground_then_background() -> Vec<u8> {
    VsdxBuilder::new()
        .page(PageSpec::new(1, "Front").attrs(r#" BackPage="2""#).shapes(&shape(11, &cell("PinX", "1"))))
        .page(PageSpec::new(2, "Back").attrs(r#" Background="1""#).shapes(&shape(12, &cell("PinX", "2"))))
        .build()
}

fn save_into(doc: &Document, original: &[u8], baseline: &Document) -> Written {
    let original = OriginalBytes { bytes: original, baseline: Some(baseline) };
    write_with(doc, Some(&original), &WriteOptions { now: Some(NOW), ..WriteOptions::default() }).unwrap_or_else(|e| panic!("{e}"))
}

// ---- a drawing never saved ---------------------------------------------------------------------------------------------------------------

#[test]
fn a_new_drawing_writes_its_background_pages_before_the_foreground_ones() {
    let doc = out_of_order();
    let bytes = write_with(&doc, None, &WriteOptions { now: Some(NOW), ..WriteOptions::default() }).unwrap_or_else(|e| panic!("{e}")).bytes;
    let pages = pages_of(&bytes);
    // The background page first, the foreground pages as the model has them; every `ID`, name, reference and shape stays with its page.
    assert_eq!(
        pages,
        [
            (3, "Back".to_owned(), true, None, vec![13]),
            (1, "Front".to_owned(), false, Some(3), vec![11]),
            (2, "Second".to_owned(), false, None, vec![12]),
        ],
        "{pages:?}"
    );
}

#[test]
fn the_page_names_of_the_document_properties_follow_the_pages_index() {
    let doc = out_of_order();
    let bytes = write_with(&doc, None, &WriteOptions { now: Some(NOW), ..WriteOptions::default() }).unwrap_or_else(|e| panic!("{e}")).bytes;
    let app = text_of(&bytes, "/docProps/app.xml");
    let at = |name: &str| app.find(&format!(">{name}<")).unwrap_or_else(|| panic!("{name} is not in {app}"));
    assert!(at("Back") < at("Front") && at("Front") < at("Second"), "{app}");
}

#[test]
fn the_drawing_handed_back_is_the_one_the_file_reads_as() {
    let doc = out_of_order();
    let written = write_with(&doc, None, &WriteOptions { now: Some(NOW), ..WriteOptions::default() }).unwrap_or_else(|e| panic!("{e}"));
    let order: Vec<u32> = written.document.pages.iter().map(|p| p.id).collect();
    assert_eq!(order, [3, 1, 2], "the baseline of the next save is what the file holds");
}

#[test]
fn a_drawing_that_is_in_order_is_written_as_it_was_and_is_not_copied() {
    let doc = DocBuilder::new()
        .page(1, PageBuilder::new().name("Back").background())
        .page(2, PageBuilder::new().name("Front").back_page(1))
        .page(3, PageBuilder::new().name("Second"))
        .build();
    assert!(doc.pages_background_first().is_none(), "a drawing in order has nothing to put in order");
    let bytes = write_with(&doc, None, &WriteOptions { now: Some(NOW), ..WriteOptions::default() }).unwrap_or_else(|e| panic!("{e}")).bytes;
    assert_eq!(names(&bytes), ["Back", "Front", "Second"]);
    // Two backgrounds keep their order among themselves, and so do the foreground pages.
    let doc = DocBuilder::new()
        .page(1, PageBuilder::new().name("F1"))
        .page(2, PageBuilder::new().name("B1").background())
        .page(3, PageBuilder::new().name("F2"))
        .page(4, PageBuilder::new().name("B2").background())
        .build();
    let ordered = doc.pages_background_first().expect("a foreground page comes before a background one");
    let order: Vec<&str> = ordered.pages.iter().map(|p| &*p.name).collect();
    assert_eq!(order, ["B1", "B2", "F1", "F2"]);
    assert!(Arc::ptr_eq(ordered.pages.get(2).unwrap(), doc.pages.get(0).unwrap()), "a page is moved and not copied");
}

// ---- a drawing written from the model alone ----------------------------------------------------------------------------------------------

#[test]
fn a_drawing_written_from_the_model_alone_writes_its_background_pages_first() {
    let (doc, _) = read_ok(&out_of_order_package());
    assert_eq!(doc.pages.iter().map(|p| &*p.name).collect::<Vec<_>>(), ["Front", "Back", "Last"], "the model keeps the order the file has");
    let written = model_write(&doc, false);
    let pages = pages_of(&written.bytes);
    assert_eq!(
        pages,
        [
            (2, "Back".to_owned(), true, None, vec![12]),
            (1, "Front".to_owned(), false, Some(2), vec![11]),
            (3, "Last".to_owned(), false, None, vec![13]),
        ],
        "{pages:?}"
    );
    // Each page keeps its own part and the rows that lead to it: the background page's part still holds its shape, and `Front` still reaches it.
    let names: Vec<(String, String)> = parts(&written.bytes)
        .into_iter()
        .filter(|(n, _)| n.starts_with(PAGE_PART) && n.ends_with(".xml") && n != PAGES_INDEX)
        .map(|(n, b)| (n, String::from_utf8_lossy(&b).into_owned()))
        .collect();
    assert_eq!(names.len(), 3, "{names:?}");
    for (id, part) in [(11, "Front"), (12, "Back"), (13, "Last")] {
        let holds: Vec<&String> = names.iter().filter(|(_, xml)| xml.contains(&format!("ID=\"{id}\""))).map(|(n, _)| n).collect();
        assert_eq!(holds.len(), 1, "shape {id} of {part} is in one part: {holds:?}");
    }
}

#[test]
fn the_model_writer_puts_the_pages_in_order_on_an_unedited_visit_too_and_leaves_a_drawing_in_order_as_it_is() {
    let (doc, _) = read_ok(&out_of_order_package());
    let written = model_write(&doc, true);
    assert_eq!(names(&written.bytes), ["Back", "Front", "Last"]);
    let in_order = VsdxBuilder::new()
        .page(PageSpec::new(2, "Back").attrs(r#" Background="1""#).shapes(&shape(12, &cell("PinX", "2"))))
        .page(PageSpec::new(1, "Front").attrs(r#" BackPage="2""#).shapes(&shape(11, &cell("PinX", "1"))))
        .build();
    let (doc, _) = read_ok(&in_order);
    assert!(doc.pages_background_first().is_none());
    assert_eq!(names(&model_write(&doc, true).bytes), ["Back", "Front"], "a drawing in order is written as it is");
}

// ---- a save into a file -----------------------------------------------------------------------------------------------------------------

#[test]
fn a_save_into_a_file_keeps_the_order_the_file_has_and_the_pages_index_byte_for_byte() {
    let original = foreground_then_background();
    let (doc, _) = read_ok(&original);
    assert_eq!(names(&original), ["Front", "Back"]);
    // An edit of a shape on the first page.
    let mut edited = doc.clone();
    page_cell(&mut edited, 1, 0, "PinX", "5");
    let written = save_into(&edited, &original, &doc);
    assert_eq!(names(&written.bytes), ["Front", "Back"], "a plain save changes nothing that was not edited");
    let before = parts(&original);
    let after = parts(&written.bytes);
    assert_eq!(before.get(PAGES_INDEX), after.get(PAGES_INDEX), "the pages index is the file's own bytes");
    // And with no edit at all, every part is the file's.
    let same = save_into(&doc, &original, &doc);
    assert_eq!(names(&same.bytes), ["Front", "Back"]);
    assert_eq!(parts(&same.bytes).get(PAGES_INDEX), before.get(PAGES_INDEX));
}

#[test]
fn a_page_taken_to_another_place_in_the_model_is_written_to_that_place_in_the_file() {
    // The splice follows the order of the model: two foreground pages that changed places are in the file's pages index in that order, with no other edit.
    let original = VsdxBuilder::new()
        .page(PageSpec::new(1, "One").shapes(&shape(11, &cell("PinX", "1"))))
        .page(PageSpec::new(2, "Two").shapes(&shape(12, &cell("PinX", "2"))))
        .build();
    let (doc, _) = read_ok(&original);
    let mut moved = doc.clone();
    let first = moved.pages.remove(0).unwrap();
    moved.pages.insert(1, first).unwrap();
    let written = save_into(&moved, &original, &doc);
    assert_eq!(names(&written.bytes), ["Two", "One"], "the order of the pages is part of what a save writes");
    let pages = pages_of(&written.bytes);
    assert_eq!(pages.iter().map(|p| (p.0, p.4.clone())).collect::<Vec<_>>(), [(2, vec![12]), (1, vec![11])]);
}

#[test]
fn a_reader_keeps_the_order_of_the_pages_index_and_names_the_background_pages() {
    // Two background pages followed by three foreground pages, and an index with a background page last.
    let mut builder = VsdxBuilder::new();
    for (id, name, back) in [(1, "B1", true), (2, "B2", true), (3, "F1", false), (4, "F2", false), (5, "F3", false)] {
        let spec = PageSpec::new(id, name);
        builder = builder.page(if back { spec.attrs(r#" Background="1""#) } else { spec });
    }
    let pages = pages_of(&builder.build());
    assert_eq!(
        pages.iter().map(|p| (p.1.as_str(), p.2)).collect::<Vec<_>>(),
        [("B1", true), ("B2", true), ("F1", false), ("F2", false), ("F3", false)]
    );
    let last =
        VsdxBuilder::new().page(PageSpec::new(1, "F1")).page(PageSpec::new(2, "F2")).page(PageSpec::new(3, "B1").attrs(r#" Background="1""#)).build();
    let pages = pages_of(&last);
    assert_eq!(
        pages.iter().map(|p| (p.1.as_str(), p.2)).collect::<Vec<_>>(),
        [("F1", false), ("F2", false), ("B1", true)],
        "read too, and the order kept"
    );
}

#[test]
fn the_helper_moves_pages_and_nothing_else() {
    let doc = out_of_order();
    let ordered = doc.pages_background_first().expect("page 3 is a background page after two others");
    assert_eq!(ordered.pages.iter().map(|p| p.id).collect::<Vec<u32>>(), [3, 1, 2]);
    // Same pages, shared: nothing of a page is copied, and nothing else of the drawing differs.
    let back: &Arc<Page> = ordered.pages.get(0).unwrap();
    assert!(Arc::ptr_eq(back, doc.pages.get(2).unwrap()));
    let mut restored = ordered.clone();
    restored.pages = doc.pages.clone();
    assert_eq!(restored, doc);
}

// ---- the page a drawing opens on ---------------------------------------------------------------------------------------------------------

/// The root of the document part, found through the relationships as a reader does.
fn document_root(bytes: &[u8]) -> diacraft_opc::xml::El {
    let package = package(bytes);
    let top = package.rels("/").unwrap();
    let document = top.items.iter().find(|r| r.ty.ends_with("/document")).unwrap().target.clone().unwrap();
    diacraft_opc::xml::parse(package.part(&document).unwrap()).unwrap()
}

/// The root of the windows part, found through the relationships of the document part.
fn windows_root(bytes: &[u8]) -> diacraft_opc::xml::El {
    let package = package(bytes);
    let top = package.rels("/").unwrap();
    let document = top.items.iter().find(|r| r.ty.ends_with("/document")).unwrap().target.clone().unwrap();
    let windows = package.rels(&document).unwrap().items.iter().find(|r| r.ty.ends_with("/windows")).unwrap().target.clone().unwrap();
    diacraft_opc::xml::parse(package.part(&windows).unwrap()).unwrap()
}

/// The page the drawing window opens (`Page` of the `Window` marked `Drawing`) and the `TopPage` of the document settings, as the file says them.
fn start_page(bytes: &[u8]) -> (Option<String>, Option<String>) {
    let windows = windows_root(bytes);
    let page = windows.children_named("Window").find(|w| w.attr("WindowType") == Some("Drawing")).and_then(|w| w.attr("Page")).map(str::to_owned);
    let top = document_root(bytes).child("DocumentSettings").and_then(|s| s.attr("TopPage")).map(str::to_owned);
    (page, top)
}

fn written_new(doc: &Document) -> Vec<u8> {
    write_with(doc, None, &WriteOptions { now: Some(NOW), ..WriteOptions::default() }).unwrap_or_else(|e| panic!("{e}")).bytes
}

#[test]
fn a_new_drawing_opens_on_its_first_foreground_page_whatever_the_order_of_its_pages() {
    let some = |id: &str| (Some(id.to_owned()), Some(id.to_owned()));
    // Foreground, foreground, background: written with the background first, and the window and `TopPage` name the first page that is not a background.
    assert_eq!(start_page(&written_new(&out_of_order())), some("1"), "the window opens `Front`, and not `Back`, which the file lists first");
    // Background first, as a drawing made in a session has them.
    let in_order = DocBuilder::new()
        .page(1, PageBuilder::new().name("Back").background())
        .page(2, PageBuilder::new().name("Front").back_page(1))
        .page(3, PageBuilder::new().name("Second"))
        .build();
    assert!(in_order.pages_background_first().is_none());
    assert_eq!(start_page(&written_new(&in_order)), some("2"));
    // Two backgrounds before the pages: still the first page that is not one.
    let two = DocBuilder::new()
        .page(1, PageBuilder::new().name("B1").background())
        .page(2, PageBuilder::new().name("B2").background())
        .page(3, PageBuilder::new().name("F1"))
        .build();
    assert_eq!(start_page(&written_new(&two)), some("3"));
    // A drawing of backgrounds only has no other page to open: the first there is.
    let only = DocBuilder::new().page(1, PageBuilder::new().name("B1").background()).page(2, PageBuilder::new().name("B2").background()).build();
    assert_eq!(start_page(&written_new(&only)), some("1"));
}

#[test]
fn the_windows_part_made_for_a_drawing_names_its_first_foreground_page_and_the_model_writer_writes_it() {
    // A drawing whose window settings were set holds a windows part of its own, made for the drawing as it is then (`new_windows_part`), and the model writer writes it: the page of its
    // drawing window is the first page that is not a background, where the model lists a background page first.
    let in_order = DocBuilder::new()
        .page(1, PageBuilder::new().name("Back").background())
        .page(2, PageBuilder::new().name("Front").back_page(1))
        .page(3, PageBuilder::new().name("Second"))
        .build();
    let part = diacraft_vsdx::new_windows_part(&in_order);
    let window = part
        .children
        .iter()
        .find_map(|n| match n {
            diacraft_doc::RawNode::Element(e) if e.attrs.get("WindowType") == Some("Drawing") => Some(e),
            _ => None,
        })
        .expect("the drawing window");
    assert_eq!(window.attrs.get("Page"), Some("2"), "`Front`, and not `Back`");
    let mut held = in_order.clone();
    held.vsdx = Some(Arc::new(diacraft_doc::VsdxHeld { windows: Some(Arc::new(part)), ..diacraft_doc::VsdxHeld::default() }));
    let written = model_write(&held, false);
    assert_eq!(start_page(&written.bytes).0.as_deref(), Some("2"));
    // A drawing of backgrounds only has no other page to open.
    let only = DocBuilder::new().page(1, PageBuilder::new().name("B1").background()).page(2, PageBuilder::new().name("B2").background()).build();
    let part = diacraft_vsdx::new_windows_part(&only);
    assert!(part.children.iter().any(|n| matches!(n, diacraft_doc::RawNode::Element(e) if e.attrs.get("Page") == Some("1"))));
}
