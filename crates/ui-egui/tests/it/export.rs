//! Export in the window: the File view's Export entry and the View tab's Export button ask for a file with the native Save dialog
//! (faked here), then the engine's `file.export` writes the page on view as a PNG, an SVG or a PDF, off the window's thread. The drawing is
//! never changed, and a refusal is told in words.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use diacraft_ui_egui::ExportPicker;
use egui::Key;
use serde_json::json;

use crate::editing::{drag_tile_to, edit_window_with, key};
use crate::support::{Rest, TempDir, Window, click, press, run_command};

/// A Save dialog that answers `path` and remembers the names it was asked to start with.
fn picker(path: Option<PathBuf>, asked: &Arc<Mutex<Vec<String>>>) -> ExportPicker {
    let asked = Arc::clone(asked);
    Arc::new(move |suggestion| {
        asked.lock().unwrap().push(suggestion);
        path.clone()
    })
}

/// A window on a new drawing with a rectangle on it, whose Export dialog answers `path`.
fn exporting_window(path: Option<PathBuf>, asked: &Arc<Mutex<Vec<String>>>) -> Window {
    let picker = picker(path, asked);
    let mut w = edit_window_with([1280.0, 900.0], |app| app.with_export_picker(picker));
    drag_tile_to(&mut w, "Rectangle", 4.0, 4.0);
    w
}

fn open_file_view(w: &mut Window) {
    run_command(w, "window.backstage", json!({"page": "Home"}));
}

/// Each kind is written from the File view's Export entry, the dialog starts with the drawing's name as a PNG, and nothing in the drawing
/// changes: no undo step, not unsaved, no file of its own.
#[test]
fn export_in_the_file_view_writes_each_kind_and_leaves_the_drawing_alone() {
    let dir = TempDir::new("export");
    for (name, magic) in [("page.png", &b"\x89PNG\r\n\x1a\n"[..]), ("page.svg", b"<?xml"), ("page.pdf", b"%PDF-"), ("Loud.PNG", b"\x89PNG")] {
        let target = dir.file(name);
        let asked = Arc::new(Mutex::new(Vec::new()));
        let mut w = exporting_window(Some(target.clone()), &asked);
        let (revision, dirty) = (w.state().session().revision(), w.state().is_dirty());
        let history = w.state().session().history().map(|h| (h.undo_len(), h.redo_len()));
        open_file_view(&mut w);
        let entry = w.state().element("backstage.Export").expect("the File view has an Export entry");
        assert!(entry.enabled, "Export works where the app has a dialog and a drawing is open");
        press(&mut w, "backstage.Export");
        w.rest();
        assert_eq!(asked.lock().unwrap().as_slice(), ["Drawing1.png"], "it started with the drawing's name as a PNG");
        let file = std::fs::read(&target).unwrap_or_else(|e| panic!("{name} was not written: {e}"));
        assert!(file.starts_with(magic), "{name} is the kind its extension says");
        assert!(w.state().notice().is_some_and(|n| n == format!("Exported {name}.")), "{:?}", w.state().notice());
        assert_eq!(w.state().session().revision(), revision, "an export is not an edit");
        assert_eq!(w.state().is_dirty(), dirty, "and does not mark the drawing saved or unsaved");
        assert_eq!(w.state().session().history().map(|h| (h.undo_len(), h.redo_len())), history, "and is no undo step");
        assert!(w.state().session().path().is_none(), "and gives the drawing no file");
        assert!(w.state().open_error().is_none());
        assert_eq!(w.state().ui_panics(), 0);
    }
}

/// The View tab's Export group has the same button.
#[test]
fn the_view_tabs_export_group_has_the_same_button() {
    let dir = TempDir::new("export-view");
    let target = dir.file("viewed.svg");
    let asked = Arc::new(Mutex::new(Vec::new()));
    let mut w = exporting_window(Some(target.clone()), &asked);
    run_command(&mut w, "window.tab", json!({"tab": "View"}));
    let button = w.state().element("ribbon.View.ExportPicture").expect("the View tab has a Picture button in its Export group").clone();
    assert!(button.enabled);
    click(&mut w, button.rect.center());
    w.rest();
    assert!(std::fs::read(&target).unwrap().starts_with(b"<?xml"));
}

/// A name chosen without an extension gets .png; another extension is refused in words and nothing is written; a cancelled dialog does
/// nothing; a folder that is not there is a refusal in words too.
#[test]
fn a_name_with_no_extension_gets_png_and_a_name_the_engine_would_refuse_is_told_so_in_words() {
    let dir = TempDir::new("export-names");
    let asked = Arc::new(Mutex::new(Vec::new()));
    let mut w = exporting_window(Some(dir.file("plain")), &asked);
    open_file_view(&mut w);
    press(&mut w, "backstage.Export");
    w.rest();
    assert!(std::fs::read(dir.file("plain.png")).unwrap().starts_with(b"\x89PNG"), "plain became plain.png");
    assert!(!dir.file("plain").exists());
    assert_eq!(w.state().notice(), Some("Exported plain.png."));

    let mut w = exporting_window(Some(dir.file("photo.jpg")), &asked);
    open_file_view(&mut w);
    press(&mut w, "backstage.Export");
    w.rest();
    let words = w.state().open_error().expect("a refusal is shown over the drawing").to_owned();
    assert!(words.starts_with("photo.jpg was not exported: a picture of a page is saved as PNG, SVG or PDF"), "{words}");
    assert!(words.contains(".png, .svg or .pdf"), "{words}");
    assert!(!dir.file("photo.jpg").exists());
    assert!(!words.contains("file.export") && !words.contains("::") && !words.contains(dir.path().to_str().unwrap()), "{words}");

    let mut w = exporting_window(Some(dir.file("no such folder").join("x.png")), &asked);
    open_file_view(&mut w);
    press(&mut w, "backstage.Export");
    w.rest();
    let words = w.state().open_error().expect("a failure is shown over the drawing").to_owned();
    assert!(words.starts_with("x.png was not exported. ") && words.ends_with("The drawing is as it was."), "{words}");
    assert!(!words.contains("file.export") && !words.contains(dir.path().to_str().unwrap()), "no command id and no folder: {words}");

    let mut w = exporting_window(None, &asked);
    open_file_view(&mut w);
    press(&mut w, "backstage.Export");
    w.rest();
    assert!(w.state().open_error().is_none() && w.state().notice().is_none(), "a cancelled dialog does nothing");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1, "only plain.png was written");
}

/// The page on view is the page written, and Export is dim where there is no dialog or no drawing.
#[test]
fn the_page_on_view_is_the_page_written_and_the_entry_is_dim_without_a_dialog() {
    let dir = TempDir::new("export-page");
    let asked = Arc::new(Mutex::new(Vec::new()));
    let mut w = exporting_window(Some(dir.file("second.svg")), &asked);
    run_command(&mut w, "page.add", json!({"name": "Second"}));
    let second = w.state().session().document().unwrap().pages.iter().find(|p| &*p.name == "Second").unwrap().id;
    run_command(&mut w, "page.select", json!({"id": second}));
    open_file_view(&mut w);
    press(&mut w, "backstage.Export");
    w.rest();
    let svg = std::fs::read_to_string(dir.file("second.svg")).unwrap();
    assert!(svg.contains("Second"), "the page on view is the one written");
    // No dialog: the entry is quiet, and so is the button.
    let mut bare = edit_window_with([1280.0, 900.0], |app| app);
    open_file_view(&mut bare);
    assert!(!bare.state().element("backstage.Export").unwrap().enabled);
    assert!(!bare.state().can_export());
    bare.state_mut().export_dialog(&egui::Context::default());
    assert!(!bare.state().export_dialog_open(), "nothing opens");
    // Nothing in the engine's command changed: it is the registered one.
    assert!(diacraft_engine::find_command("file.export").is_some());
    key(&mut bare, Key::Escape, egui::Modifiers::NONE);
}
