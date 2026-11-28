//! **Background pages come first in the pages index** of a VSDX file, as in all 70 files of the corpus that hold both
//! kinds. A drawing a session makes keeps its background pages before its foreground ones, so a new drawing is saved in that order. A drawing opened from a file that has
//! another order keeps the file's order in a plain save, which changes only what was edited; a **Save As** to a VSDX type, and a drawing written from the model alone, write the background
//! pages first, and the drawing in the session becomes what the file holds (no undo step), so the next save compares the same order. `.diacraft` keeps the model's page order, as it keeps
//! the whole model.

use diacraft_engine::Session;
use diacraft_testkit::package::{PageSpec, VsdxBuilder, cell, shape};
use serde_json::json;

use crate::edit::doc;
use crate::native_common::Dir;
use crate::{blank_session, check_session, page_names, run};

/// A file whose pages index runs `Front`, `Back` (a background page that `Front` names), `Last`: foreground first, as a file of another program may have it.
fn other_order() -> Vec<u8> {
    VsdxBuilder::new()
        .page(PageSpec::new(1, "Front").attrs(r#" BackPage="2""#).shapes(&shape(11, &cell("PinX", "1"))))
        .page(PageSpec::new(2, "Back").attrs(r#" Background="1""#).shapes(&shape(12, &cell("PinX", "2"))))
        .page(PageSpec::new(3, "Last").shapes(&shape(13, &cell("PinX", "3"))))
        .build()
}

fn opened(dir: &Dir) -> (Session, String) {
    let path = dir.write("source.vsdx", &other_order());
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": path }));
    (s, path)
}

fn reopened(path: &str) -> Session {
    let mut s = Session::new();
    run(&mut s, "file.open", json!({ "path": path }));
    s
}

/// `(id, name, background, back page)` of each page, in order.
fn pages(s: &Session) -> Vec<(u32, String, bool, Option<u32>)> {
    doc(s).pages.iter().map(|p| (p.id, p.name.to_string(), p.background, p.back_page)).collect()
}

const IN_ORDER: [&str; 3] = ["Back", "Front", "Last"];

#[test]
fn a_new_drawing_is_saved_with_its_background_pages_first() {
    let mut s = blank_session();
    run(&mut s, "page.add", json!({"name": "Second"}));
    run(&mut s, "page.add", json!({"name": "Plan", "background": true}));
    run(&mut s, "page.background", json!({"background": 3, "page": 1}));
    assert_eq!(page_names(&s), ["Plan", "Page-1", "Second"], "a background page is put before the others as it is made");
    let dir = Dir::new();
    let path = dir.path("new.vsdx");
    run(&mut s, "file.save_as", json!({ "path": path }));
    let back = reopened(&path);
    assert_eq!(page_names(&back), ["Plan", "Page-1", "Second"]);
    assert_eq!(pages(&back), pages(&s), "ids, names and what each page names as its background are the same");
}

#[test]
fn a_plain_save_keeps_the_order_the_file_has_and_the_session_keeps_it_too() {
    let dir = Dir::new();
    let (mut s, path) = opened(&dir);
    assert_eq!(page_names(&s), ["Front", "Back", "Last"]);
    run(&mut s, "page.rename", json!({"id": 3, "name": "Final"}));
    run(&mut s, "file.save", json!({}));
    assert_eq!(page_names(&s), ["Front", "Back", "Final"], "the drawing in the session is not put in order by a plain save");
    let back = reopened(&path);
    assert_eq!(page_names(&back), ["Front", "Back", "Final"], "a plain save changes nothing that was not edited");
    assert_eq!(pages(&back)[0], (1, "Front".to_owned(), false, Some(2)));
    check_session(&s).unwrap();
}

#[test]
fn a_save_as_to_a_vsdx_type_writes_the_background_pages_first_and_the_drawing_becomes_what_the_file_holds() {
    let dir = Dir::new();
    let (mut s, source) = opened(&dir);
    let source_bytes = std::fs::read(&source).unwrap();
    run(&mut s, "page.rename", json!({"id": 3, "name": "Final"}));
    let copy = dir.path("copy.vsdx");
    let saved = run(&mut s, "file.save_as", json!({ "path": copy }));
    assert_eq!(saved["dirty"], false, "{saved}");
    let back = reopened(&copy);
    assert_eq!(page_names(&back), ["Back", "Front", "Final"]);
    assert_eq!(
        pages(&back),
        [(2, "Back".to_owned(), true, None), (1, "Front".to_owned(), false, Some(2)), (3, "Final".to_owned(), false, None)],
        "ids, names and the page `Front` names as its background go with their pages"
    );
    // The drawing in the session is what the file holds, as no undo step, and it is not unsaved.
    assert_eq!(page_names(&s), ["Back", "Front", "Final"]);
    assert!(!s.is_dirty());
    // The next save compares the same order: with no edit it writes the file it wrote, and with an edit the order stays.
    let first = std::fs::read(&copy).unwrap();
    run(&mut s, "file.save", json!({}));
    assert_eq!(std::fs::read(&copy).unwrap(), first, "a save with no edit after a Save As writes the same bytes");
    run(&mut s, "page.rename", json!({"id": 1, "name": "Start"}));
    run(&mut s, "file.save", json!({}));
    assert_eq!(page_names(&reopened(&copy)), ["Back", "Start", "Final"]);
    // The file the drawing was opened from is as it was.
    assert_eq!(std::fs::read(&source).unwrap(), source_bytes);
    check_session(&s).unwrap();
}

#[test]
fn a_save_as_of_a_drawing_already_in_order_changes_no_page_of_the_session() {
    let mut s = blank_session();
    run(&mut s, "page.add", json!({"name": "Plan", "background": true}));
    let before = doc(&s).clone();
    let dir = Dir::new();
    let path = dir.path("again.vsdx");
    run(&mut s, "file.save_as", json!({ "path": path }));
    assert_eq!(doc(&s).pages.len(), before.pages.len());
    for (a, b) in doc(&s).pages.iter().zip(before.pages.iter()) {
        assert!(std::sync::Arc::ptr_eq(a, b), "page {} is the very page it was", a.id);
    }
}

#[test]
fn a_drawing_written_from_the_model_alone_has_its_background_pages_first_and_the_check_names_nothing() {
    let dir = Dir::new();
    let (mut s, _) = opened(&dir);
    // The drawing as a `.diacraft` file keeps the model's order, as it keeps the whole model.
    let native = dir.path("copy.diacraft");
    run(&mut s, "file.save_as", json!({ "path": native }));
    let held = reopened(&native);
    assert_eq!(page_names(&held), ["Front", "Back", "Last"], "the native file is the model, in the model's order");
    // Saved from it as a VSDX type, the file is written from the model alone: backgrounds first, and the check finds the drawing and the file the same.
    let mut from_native = reopened(&native);
    let out = dir.path("from-native.vsdx");
    let saved = run(&mut from_native, "file.save_as", json!({ "path": out }));
    assert_eq!(saved["checked"]["classes"], json!([]), "nothing the check could name differs: {saved}");
    assert_eq!(page_names(&reopened(&out)), IN_ORDER);
    assert_eq!(page_names(&from_native), IN_ORDER, "the drawing becomes what the file reads as");
    assert_eq!(pages(&reopened(&out)), pages(&from_native));
    check_session(&from_native).unwrap();
}

#[test]
fn a_vsdx_file_read_after_a_native_visit_holds_the_same_content_in_either_order() {
    // The content of the pages is the same whatever their order: each shape stays on its own page, with the page's own `ID`.
    let dir = Dir::new();
    let (mut s, _) = opened(&dir);
    let copy = dir.path("copy.vsdx");
    run(&mut s, "file.save_as", json!({ "path": copy }));
    let back = reopened(&copy);
    for (name, shape_id) in [("Front", 11), ("Back", 12), ("Last", 13)] {
        let page = doc(&back).pages.iter().find(|p| &*p.name == name).cloned().unwrap();
        assert_eq!(page.shapes.iter().map(|x| x.id).collect::<Vec<_>>(), [shape_id], "{name}");
    }
}

// ---- the page a drawing opens on ------------------------------------------------------------------------------------------------------------

/// The page the drawing window of a saved VSDX file opens and the `TopPage` of its document settings, as the file says them.
fn start_page(path: &str) -> (Option<String>, Option<String>) {
    let bytes = std::fs::read(path).unwrap();
    let package = diacraft_opc::Package::read(&bytes, &diacraft_opc::Limits::default()).unwrap();
    let top = package.rels("/").unwrap();
    let document = top.items.iter().find(|r| r.ty.ends_with("/document")).unwrap().target.clone().unwrap();
    let windows = package.rels(&document).unwrap().items.iter().find(|r| r.ty.ends_with("/windows")).unwrap().target.clone().unwrap();
    let windows = diacraft_opc::xml::parse(package.part(&windows).unwrap()).unwrap();
    let page = windows.children_named("Window").find(|w| w.attr("WindowType") == Some("Drawing")).and_then(|w| w.attr("Page")).map(str::to_owned);
    let settings = diacraft_opc::xml::parse(package.part(&document).unwrap()).unwrap();
    let top_page = settings.child("DocumentSettings").and_then(|s| s.attr("TopPage")).map(str::to_owned);
    (page, top_page)
}

/// A new drawing with a background page, which the model lists first: `VBackground-1` (id 2), `Page-1` (id 1) and `Second` (id 3).
fn with_a_background_page() -> Session {
    let mut s = blank_session();
    run(&mut s, "page.add", json!({"background": true}));
    run(&mut s, "page.add", json!({"name": "Second"}));
    assert_eq!(page_names(&s), ["VBackground-1", "Page-1", "Second"]);
    s
}

#[test]
fn a_drawing_with_a_background_page_is_saved_with_its_window_on_a_foreground_page() {
    // The model's first page is `VBackground-1`; the window of the file and its `TopPage` name `Page-1`.
    let mut s = with_a_background_page();
    let dir = Dir::new();
    let path = dir.path("start.vsdx");
    run(&mut s, "file.save_as", json!({ "path": path }));
    let (page, top) = start_page(&path);
    assert_eq!((page.as_deref(), top.as_deref()), (Some("1"), Some("1")), "the window opens `Page-1` (id 1), and not `VBackground-1` (id 2)");
    assert_eq!(page_names(&reopened(&path)), ["VBackground-1", "Page-1", "Second"], "and the file lists the background page first");
}

#[test]
fn the_window_a_drawing_gets_when_its_settings_are_set_opens_a_foreground_page_too() {
    // A drawing whose window settings were set holds a windows part of its own, made for the drawing as it is then and written from the model alone.
    let mut s = with_a_background_page();
    run(&mut s, "window.settings.set", json!({"grid": false}));
    let dir = Dir::new();
    let path = dir.path("settings.vsdx");
    run(&mut s, "file.save_as", json!({ "path": path }));
    assert_eq!(start_page(&path).0.as_deref(), Some("1"));
    let mut back = reopened(&path);
    assert_eq!(page_names(&back), ["VBackground-1", "Page-1", "Second"]);
    assert_eq!(run(&mut back, "window.settings", json!({}))["grid"], false, "the setting is the file's");
}
