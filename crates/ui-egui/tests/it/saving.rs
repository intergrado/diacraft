//! Saving from the window: Save and Save As with the dialogs faked, the star in the title, and the question asked before New, Open and
//! closing throw unsaved changes away. The engine's own commands (`file.save`, `file.save_as`) do the writing.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use diacraft_engine::Session;
use diacraft_ui_egui::FilePicker;
use diacraft_ui_egui::SavePicker;
use egui::{Key, Modifiers, ViewportCommand, ViewportEvent, ViewportId};
use serde_json::json;

use crate::editing::{drag_tile_to, edit_window_with, key, key_without_waiting, shape_ids};
use crate::support::{Rest, TempDir, Window, click, commands, press, run_command};

/// A Save dialog that answers `path` and remembers the names it was asked to start with.
fn save_picker(path: PathBuf, asked: &Arc<Mutex<Vec<String>>>) -> SavePicker {
    let asked = Arc::clone(asked);
    Arc::new(move |request| {
        asked.lock().unwrap().push(request.name);
        Some(path.clone())
    })
}

fn open_picker(path: PathBuf) -> FilePicker {
    Arc::new(move || Some(path.clone()))
}

pub fn ctrl() -> Modifiers {
    Modifiers { ctrl: true, command: true, ..Modifiers::NONE }
}

pub fn ctrl_shift() -> Modifiers {
    Modifiers { shift: true, ..ctrl() }
}

pub fn window_with_save(path: &Path, asked: &Arc<Mutex<Vec<String>>>) -> Window {
    let picker = save_picker(path.to_path_buf(), asked);
    edit_window_with([1280.0, 900.0], |app| app.with_save_picker(picker))
}

/// A drawing is unsaved from its first edit, and an edit taken back leaves it saved again; the title carries a star while it is not.
#[test]
fn the_title_shows_unsaved_changes_and_an_undo_to_the_saved_state_clears_them() {
    let mut w = edit_window_with([1280.0, 900.0], |app| app);
    assert_eq!(w.state().window_title(), "Drawing1 - DiaCraft");
    assert!(!w.state().is_dirty(), "a new drawing has nothing to save");
    drag_tile_to(&mut w, "Rectangle", 4.0, 4.0);
    assert!(w.state().is_dirty());
    assert_eq!(w.state().window_title(), "*Drawing1 - DiaCraft");
    assert_eq!(w.state().window_state()["dirty"], true);
    key(&mut w, Key::Z, ctrl());
    assert!(!w.state().is_dirty(), "the edit and its undo leave it clean");
    assert_eq!(w.state().window_title(), "Drawing1 - DiaCraft");
}

/// Ctrl+S on a drawing that has no file asks for one with the Save dialog (starting with the drawing's name), writes it with `file.save_as`
/// and from then on saves in place without asking.
#[test]
fn save_asks_for_a_file_once_and_then_saves_in_place() {
    let dir = TempDir::new("save");
    let target = dir.file("plan.vsdx");
    let asked = Arc::new(Mutex::new(Vec::new()));
    let mut w = window_with_save(&target, &asked);
    drag_tile_to(&mut w, "Rectangle", 4.0, 4.0);
    key(&mut w, Key::S, ctrl());
    assert!(w.state().save_dialog_open() || target.exists(), "the dialog was opened");
    w.rest();
    assert_eq!(asked.lock().unwrap().as_slice(), ["Drawing1.diacraft"], "it started with the drawing's name, and the type of a new drawing");
    assert!(target.exists(), "the file was written");
    assert!(!w.state().is_dirty());
    assert_eq!(w.state().window_title(), "plan.vsdx - DiaCraft", "the drawing is named for its file now");
    assert!(w.state().notice().is_some_and(|n| n.starts_with("Saved plan.vsdx")), "{:?}", w.state().notice());
    // What was written reads back with its one shape.
    let mut other = Session::new();
    other.execute("file.open", &json!({"path": target.to_str().unwrap()})).unwrap();
    assert_eq!(other.document().unwrap().pages.get(0).unwrap().shapes.len(), 1);
    // A second edit, and Ctrl+S saves it to the same file with no dialog.
    drag_tile_to(&mut w, "Diamond", 7.0, 4.0);
    assert!(w.state().is_dirty());
    assert_eq!(w.state().window_title(), "*plan.vsdx - DiaCraft");
    key(&mut w, Key::S, ctrl());
    w.rest();
    assert_eq!(asked.lock().unwrap().len(), 1, "no second dialog");
    assert!(!w.state().is_dirty());
    let mut again = Session::new();
    again.execute("file.open", &json!({"path": target.to_str().unwrap()})).unwrap();
    assert_eq!(again.document().unwrap().pages.get(0).unwrap().shapes.len(), 2);
    assert_eq!(w.state().ui_panics(), 0);
}

/// Ctrl+Shift+S asks again, even for a drawing that has a file, and a name chosen without an extension gets `.vsdx`.
#[test]
fn save_as_asks_again_and_adds_the_extension() {
    let dir = TempDir::new("save-as");
    let first = dir.file("one.vsdx");
    let asked = Arc::new(Mutex::new(Vec::new()));
    let mut w = window_with_save(&first, &asked);
    drag_tile_to(&mut w, "Hexagon", 4.0, 4.0);
    key(&mut w, Key::S, ctrl());
    w.rest();
    assert!(first.exists());
    // The same window, a different answer: the dialog is asked again.
    let second = dir.file("two");
    let picker = save_picker(second.clone(), &asked);
    let mut w2 = edit_window_with([1280.0, 900.0], |app| app.with_save_picker(picker));
    drag_tile_to(&mut w2, "Hexagon", 4.0, 4.0);
    key(&mut w2, Key::S, ctrl_shift());
    w2.rest();
    assert!(dir.file("two.diacraft").exists() && !second.exists(), "`.diacraft`, the type of a new drawing, was added to the name");
    assert_eq!(w2.state().window_title(), "two.diacraft - DiaCraft");
    // Save As on a drawing that already has a file asks again.
    key(&mut w2, Key::S, ctrl_shift());
    w2.rest();
    assert_eq!(asked.lock().unwrap().len(), 3);
}

/// A link where the file should go is refused in words, and neither the link's target nor anything else is written.
#[cfg(unix)]
#[test]
fn a_link_is_refused_in_plain_words() {
    let dir = TempDir::new("save-link");
    let precious = dir.file("precious.vsdx");
    std::fs::write(&precious, b"not to be overwritten").unwrap();
    let link = dir.file("link.vsdx");
    std::os::unix::fs::symlink(&precious, &link).unwrap();
    let asked = Arc::new(Mutex::new(Vec::new()));
    let mut w = window_with_save(&link, &asked);
    drag_tile_to(&mut w, "Rectangle", 4.0, 4.0);
    key(&mut w, Key::S, ctrl());
    w.rest();
    let words = w.state().open_error().expect("the refusal is shown").to_owned();
    assert!(words.contains("not saved") && words.contains("link"), "{words}");
    assert!(!words.contains("file.save_as") && !words.contains("precious"), "no command id or path: {words}");
    assert_eq!(std::fs::read(&precious).unwrap(), b"not to be overwritten");
    assert!(w.state().is_dirty(), "and the drawing is still unsaved");
}

/// A name whose extension is none of the file kinds is refused with the list of them.
#[test]
fn an_unknown_extension_is_refused() {
    let dir = TempDir::new("save-ext");
    let asked = Arc::new(Mutex::new(Vec::new()));
    let mut w = window_with_save(&dir.file("plan.txt"), &asked);
    drag_tile_to(&mut w, "Rectangle", 4.0, 4.0);
    key(&mut w, Key::S, ctrl());
    w.rest();
    let words = w.state().open_error().expect("refused").to_owned();
    assert!(words.contains(".vsdx") && words.contains("not saved"), "{words}");
    assert!(!dir.file("plan.txt").exists());
}

/// New asks first when the drawing has changes that are not saved: Cancel leaves everything, Don't Save goes on, Save saves and goes on. A
/// drawing with nothing to save is not asked about.
#[test]
fn new_asks_before_it_throws_changes_away() {
    let dir = TempDir::new("ask-new");
    let target = dir.file("kept.vsdx");
    let asked = Arc::new(Mutex::new(Vec::new()));
    let mut w = window_with_save(&target, &asked);
    // Nothing to save: no question.
    key(&mut w, Key::N, ctrl());
    assert!(!w.state().prompt_open());
    assert_eq!(w.state().window_title(), "Drawing2 - DiaCraft");
    drag_tile_to(&mut w, "Rectangle", 4.0, 4.0);
    key(&mut w, Key::N, ctrl());
    assert!(w.state().prompt_open());
    w.rest();
    assert!(w.state().element("prompt.save").is_some(), "the question has its three answers");
    // Cancel (and so Escape): the drawing stays.
    press(&mut w, "prompt.cancel");
    assert!(!w.state().prompt_open() && shape_ids(&w).len() == 1);
    key(&mut w, Key::N, ctrl());
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert!(!w.state().prompt_open() && shape_ids(&w).len() == 1, "Escape is Cancel");
    // Save: asks for a file, writes it, then makes the new drawing.
    key(&mut w, Key::N, ctrl());
    press(&mut w, "prompt.save");
    w.rest();
    assert!(target.exists(), "the changes were saved first");
    assert_eq!(w.state().window_title(), "Drawing3 - DiaCraft", "and then the new drawing was made");
    assert!(shape_ids(&w).is_empty());
    // Don't Save: the new drawing replaces it.
    drag_tile_to(&mut w, "Circle", 4.0, 4.0);
    key(&mut w, Key::N, ctrl());
    press(&mut w, "prompt.dont_save");
    assert_eq!(w.state().window_title(), "Drawing4 - DiaCraft");
    // A Save that does not work (the dialog is cancelled) goes no further.
    let mut w2 = edit_window_with([1280.0, 900.0], |app| app.with_save_picker(Arc::new(|_| None)));
    drag_tile_to(&mut w2, "Circle", 4.0, 4.0);
    key(&mut w2, Key::N, ctrl());
    press(&mut w2, "prompt.save");
    w2.rest();
    assert_eq!(w2.state().window_title(), "*Drawing1 - DiaCraft", "nothing was replaced");
}

/// A file chosen with the Open dialog is asked about first too.
#[test]
fn open_asks_once_a_file_is_chosen() {
    let dir = TempDir::new("ask-open");
    let file = dir.file("other.vsdx");
    {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("file.save_as", &json!({"path": file.to_str().unwrap()})).unwrap();
    }
    let mut w = edit_window_with([1280.0, 900.0], |app| app.with_file_picker(open_picker(file.clone())));
    drag_tile_to(&mut w, "Rectangle", 4.0, 4.0);
    key_without_waiting(&mut w, Key::O, ctrl());
    w.rest();
    assert!(w.state().prompt_open(), "the drawing has changes: the chosen file waits for the answer");
    assert_eq!(w.state().window_title(), "*Drawing1 - DiaCraft");
    press(&mut w, "prompt.dont_save");
    w.rest();
    assert_eq!(w.state().window_title(), "other.vsdx - DiaCraft");
    // Opened, and nothing to save: a second Open goes straight through.
    key_without_waiting(&mut w, Key::O, ctrl());
    w.rest();
    assert!(!w.state().prompt_open());
}

/// Closing the window with unsaved changes is held back and asked about; a program's quit is not.
#[test]
fn closing_the_window_asks_first() {
    let mut w = edit_window_with([1280.0, 900.0], |app| app);
    // Clean: the close goes through (it is not cancelled).
    w.input_mut().viewports.entry(ViewportId::ROOT).or_default().events.push(ViewportEvent::Close);
    w.step();
    assert!(!w.state().prompt_open());
    assert!(!commands(&w).iter().any(|c| matches!(c, ViewportCommand::CancelClose)));
    drag_tile_to(&mut w, "Rectangle", 4.0, 4.0);
    w.input_mut().viewports.entry(ViewportId::ROOT).or_default().events.push(ViewportEvent::Close);
    w.step();
    assert!(commands(&w).iter().any(|c| matches!(c, ViewportCommand::CancelClose)), "the close was cancelled");
    assert!(w.state().prompt_open());
    w.rest();
    let at = w.state().element("prompt.dont_save").unwrap().rect.center();
    click(&mut w, at);
    assert!(commands(&w).iter().any(|c| matches!(c, ViewportCommand::Close)), "and given when the person said so");
}

/// The Save controls are enabled because the engine has the commands; Save As needs the app's dialog.
#[test]
fn the_save_controls_follow_the_commands_and_the_dialog() {
    assert!(diacraft_engine::find_command("file.save").is_some() && diacraft_engine::find_command("file.save_as").is_some());
    let w = edit_window_with([1280.0, 900.0], |app| app);
    assert!(w.state().element("qat.Save").unwrap().enabled, "Save on the Quick Access Toolbar");
    let asked = Arc::new(Mutex::new(Vec::new()));
    let mut w = window_with_save(Path::new("/nowhere/x.vsdx"), &asked);
    run_command(&mut w, "window.backstage", json!({"page": "Home"}));
    assert!(w.state().element("backstage.Save").unwrap().enabled && w.state().element("backstage.Save As").unwrap().enabled);
}

/// "Saved plan.vsdx." is about the drawing it saved, and goes when a new drawing replaces it. (Until a later change a notice about the window stayed through
/// `file.new`; now `file.new`, like any command that changes something, takes every notice away: `notices.rs`.)
#[test]
fn what_the_status_bar_said_of_a_saved_drawing_goes_when_another_replaces_it() {
    let dir = TempDir::new("notice");
    let asked = Arc::new(Mutex::new(Vec::new()));
    let mut w = window_with_save(&dir.file("plan.vsdx"), &asked);
    drag_tile_to(&mut w, "Rectangle", 4.0, 4.0);
    key(&mut w, Key::S, ctrl());
    w.rest();
    assert!(w.state().notice().is_some_and(|n| n.starts_with("Saved plan.vsdx")));
    key(&mut w, Key::N, ctrl());
    assert!(w.state().notice().is_none(), "the drawing it spoke of is gone");
}

/// The File view's rail holds eight entries now; in a short window they scroll and never lie over each other.
#[test]
fn the_file_views_rail_never_overlaps_its_entries() {
    let mut w = edit_window_with([480.0, 420.0], |app| app);
    run_command(&mut w, "window.backstage", json!({"page": "Home"}));
    for size in [[480.0, 420.0], [480.0, 330.0], [900.0, 800.0]] {
        w.set_size(egui::vec2(size[0], size[1]));
        w.rest();
        let mut rects: Vec<(String, egui::Rect)> = ["Home", "New", "Open", "Save", "Save As", "Export", "Account", "Options"]
            .iter()
            .map(|n| (n.to_string(), w.state().element(&format!("backstage.{n}")).unwrap_or_else(|| panic!("{n} at {size:?}")).rect))
            .collect();
        rects.sort_by(|a, b| a.1.min.y.total_cmp(&b.1.min.y));
        for pair in rects.windows(2) {
            assert!(pair[0].1.max.y <= pair[1].1.min.y + 0.5, "{} lies over {} at {size:?}: {:?} {:?}", pair[0].0, pair[1].0, pair[0].1, pair[1].1);
        }
    }
}

/// Ctrl+Z undoes, and both Ctrl+Shift+Z and Ctrl+Y redo (the registry's `edit.redo` carries the first; the window adds the second).
#[test]
fn ctrl_y_redoes_as_ctrl_shift_z_does() {
    let mut w = edit_window_with([1280.0, 900.0], |app| app);
    drag_tile_to(&mut w, "Rectangle", 4.0, 4.0);
    assert_eq!(shape_ids(&w).len(), 1);
    for redo in [Modifiers { shift: true, ..ctrl() }, ctrl()] {
        let key_of = if redo.shift { Key::Z } else { Key::Y };
        key(&mut w, Key::Z, ctrl());
        assert!(shape_ids(&w).is_empty(), "Ctrl+Z took the shape back");
        key(&mut w, key_of, redo);
        assert_eq!(shape_ids(&w).len(), 1, "{redo:?} + {key_of:?} brought it back");
        assert!(w.state().is_dirty());
    }
    // With nothing to redo the key does nothing and says nothing.
    key(&mut w, Key::Y, ctrl());
    assert_eq!(shape_ids(&w).len(), 1);
    assert!(w.state().notice().is_none());
}

/// A small `.odg` of one rectangle, built by hand from the standard: `mimetype` first and stored, the parts after it, and a manifest that lists them.
pub fn small_odg() -> Vec<u8> {
    use diacraft_testkit::package::{ZipItem, zip};
    const NS: &str = concat!(
        r#"xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" "#,
        r#"xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" "#,
        r#"xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0""#
    );
    let content = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><office:document-content {NS} office:version=\"1.3\"><office:automatic-styles><style:style style:name=\"gr1\" style:family=\"graphic\"><style:graphic-properties draw:fill=\"solid\" draw:fill-color=\"#3465a4\"/></style:style></office:automatic-styles><office:body><office:drawing><draw:page draw:name=\"page1\" draw:master-page-name=\"Default\"><draw:custom-shape draw:style-name=\"gr1\" draw:layer=\"layout\" draw:name=\"A\" svg:width=\"4cm\" svg:height=\"2cm\" svg:x=\"2cm\" svg:y=\"3cm\"><text:p/><draw:enhanced-geometry svg:viewBox=\"0 0 21600 21600\" draw:type=\"non-primitive\" draw:enhanced-path=\"M 0 0 L 21600 0 21600 21600 0 21600 0 0 Z N\"/></draw:custom-shape></draw:page></office:drawing></office:body></office:document-content>"
    );
    let styles = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><office:document-styles {NS} office:version=\"1.3\"><office:styles/><office:automatic-styles><style:page-layout style:name=\"PM1\"><style:page-layout-properties fo:page-width=\"21cm\" fo:page-height=\"29.7cm\" style:print-orientation=\"portrait\"/></style:page-layout></office:automatic-styles><office:master-styles><draw:layer-set><draw:layer draw:name=\"layout\"/></draw:layer-set><style:master-page style:name=\"Default\" style:page-layout-name=\"PM1\"/></office:master-styles></office:document-styles>"
    );
    let manifest = concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><manifest:manifest xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\" manifest:version=\"1.3\">",
        "<manifest:file-entry manifest:full-path=\"/\" manifest:version=\"1.3\" manifest:media-type=\"application/vnd.oasis.opendocument.graphics\"/>",
        "<manifest:file-entry manifest:full-path=\"content.xml\" manifest:media-type=\"text/xml\"/>",
        "<manifest:file-entry manifest:full-path=\"styles.xml\" manifest:media-type=\"text/xml\"/></manifest:manifest>"
    );
    zip(&[
        ZipItem::stored("mimetype", b"application/vnd.oasis.opendocument.graphics"),
        ZipItem::deflated("content.xml", content.as_bytes()),
        ZipItem::deflated("styles.xml", styles.as_bytes()),
        ZipItem::deflated("META-INF/manifest.xml", manifest.as_bytes()),
    ])
}

/// `small_odg` with a second rectangle, B, and a formula in A that reads it: A's own style carries DiaCraft's data (the cell `Cost`, `Sheet.2!Width`) and B's its `ID`
/// As a save of DiaCraft writes them.
fn odg_with_a_cross_shape_formula() -> Vec<u8> {
    use diacraft_testkit::package::{ZipItem, zip};
    const NS: &str = concat!(
        r#"xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" "#,
        r#"xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" "#,
        r#"xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" "#,
        r#"xmlns:diacraft="urn:app.diacraft:extension""#
    );
    let cells = "&lt;Sheet&gt;&lt;Cell N=&quot;Cost&quot; V=&quot;1&quot; F=&quot;Sheet.2!Width&quot;/&gt;&lt;/Sheet&gt;";
    let style = |name: &str, data: &str| {
        format!(
            "<style:style style:name=\"{name}\" style:family=\"graphic\"><style:graphic-properties draw:fill=\"solid\" draw:fill-color=\"#3465a4\" {data}/></style:style>"
        )
    };
    let shape = |name: &str, style: &str, x: u32| {
        format!(
            "<draw:custom-shape draw:style-name=\"{style}\" draw:layer=\"layout\" draw:name=\"{name}\" svg:width=\"4cm\" svg:height=\"2cm\" svg:x=\"{x}cm\" svg:y=\"3cm\"><text:p/><draw:enhanced-geometry svg:viewBox=\"0 0 21600 21600\" draw:type=\"non-primitive\" draw:enhanced-path=\"M 0 0 L 21600 0 21600 21600 0 21600 0 0 Z N\"/></draw:custom-shape>"
        )
    };
    let content = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><office:document-content {NS} office:version=\"1.3\"><office:automatic-styles>{}{}</office:automatic-styles><office:body><office:drawing><draw:page draw:name=\"page1\" draw:master-page-name=\"Default\">{}{}</draw:page></office:drawing></office:body></office:document-content>",
        style("grA", &format!("diacraft:version=\"1\" diacraft:id=\"1\" diacraft:cells=\"{cells}\"")),
        style("grB", "diacraft:version=\"1\" diacraft:id=\"2\""),
        shape("A", "grA", 2),
        shape("B", "grB", 12)
    );
    let styles = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><office:document-styles {NS} office:version=\"1.3\"><office:styles/><office:automatic-styles><style:page-layout style:name=\"PM1\"><style:page-layout-properties fo:page-width=\"21cm\" fo:page-height=\"29.7cm\" style:print-orientation=\"portrait\"/></style:page-layout></office:automatic-styles><office:master-styles><draw:layer-set><draw:layer draw:name=\"layout\"/></draw:layer-set><style:master-page style:name=\"Default\" style:page-layout-name=\"PM1\"/></office:master-styles></office:document-styles>"
    );
    let manifest = concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><manifest:manifest xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\" manifest:version=\"1.3\">",
        "<manifest:file-entry manifest:full-path=\"/\" manifest:version=\"1.3\" manifest:media-type=\"application/vnd.oasis.opendocument.graphics\"/>",
        "<manifest:file-entry manifest:full-path=\"content.xml\" manifest:media-type=\"text/xml\"/>",
        "<manifest:file-entry manifest:full-path=\"styles.xml\" manifest:media-type=\"text/xml\"/></manifest:manifest>"
    );
    zip(&[
        ZipItem::stored("mimetype", b"application/vnd.oasis.opendocument.graphics"),
        ZipItem::deflated("content.xml", content.as_bytes()),
        ZipItem::deflated("styles.xml", styles.as_bytes()),
        ZipItem::deflated("META-INF/manifest.xml", manifest.as_bytes()),
    ])
}

/// **The save note is shown once in a session, in the banner** (design decision 8): a drawing that a LibreOffice save keeps whole shows none (a shape moved,
/// a shape dropped from a stencil: its link to a master is no trigger), and one that holds a formula that reads another shape shows it with its first
/// save, which a person dismisses, and not again with the next.
#[test]
fn the_save_note_is_shown_once_in_the_banner_and_not_for_a_drawing_a_libreoffice_save_keeps_whole() {
    let dir = TempDir::new("odg-save-note");
    // A drawing with nothing a LibreOffice save would drop: no banner.
    let plain = dir.file("plain.odg");
    std::fs::write(&plain, small_odg()).unwrap();
    let mut w = edit_window_with([1280.0, 900.0], |app| app.with_file_picker(open_picker(plain.clone())));
    key_without_waiting(&mut w, Key::O, ctrl());
    w.rest();
    let first = w.state().session().document().unwrap().pages.get(0).unwrap().shapes.get(0).unwrap().id;
    w.state_mut().execute("shape.move", &json!({"ids": [first], "dx": 0.5, "dy": 0.25})).unwrap();
    key(&mut w, Key::S, ctrl());
    assert!(!w.state().is_dirty(), "{:?}", w.state().open_error());
    assert_eq!(w.state().open_error(), None, "nothing to say of a drawing a LibreOffice save keeps whole");
    // A shape dropped from a stencil is linked to its master, which the file carries in DiaCraft's document file: that is no trigger of the note.
    drag_tile_to(&mut w, "Rectangle", 4.0, 4.0);
    key(&mut w, Key::S, ctrl());
    assert!(!w.state().is_dirty(), "{:?}", w.state().open_error());
    assert_eq!(w.state().open_error(), None, "a link to a master is no trigger of the note");
    // One with a cross-shape formula: the note comes with the first save.
    let held = dir.file("held.odg");
    std::fs::write(&held, odg_with_a_cross_shape_formula()).unwrap();
    let mut w = edit_window_with([1280.0, 900.0], |app| app.with_file_picker(open_picker(held.clone())));
    key_without_waiting(&mut w, Key::O, ctrl());
    w.rest();
    drag_tile_to(&mut w, "Rectangle", 4.0, 4.0);
    key(&mut w, Key::S, ctrl());
    assert!(!w.state().is_dirty(), "{:?}", w.state().open_error());
    let said = w.state().open_error().map(str::to_owned).unwrap_or_default();
    assert!(said.contains("a LibreOffice save would drop") && said.contains("1 shape with a formula that reads another shape"), "{said}");
    assert!(!said.contains("master"), "a link to a master is not said: {said}");
    assert!(w.state().notice().is_some_and(|n| n.starts_with("Saved held.odg.") && !n.contains("LibreOffice")), "{:?}", w.state().notice());
    // Dismissed, it is not said again by the next save, though the drawing still holds the formula.
    w.state_mut().dismiss_open_error();
    drag_tile_to(&mut w, "Rectangle", 7.0, 4.0);
    key(&mut w, Key::S, ctrl());
    assert!(!w.state().is_dirty(), "{:?}", w.state().open_error());
    assert_eq!(w.state().open_error(), None, "once in a session");
    assert_eq!(w.state().ui_panics(), 0);
}

/// An `.odg` opened in the window (the Open dialog's answer) is saved in place, as an `.odg`, with the shape that was dropped on it.
#[test]
fn an_odg_opened_in_the_window_is_saved_in_place_as_an_odg() {
    let dir = TempDir::new("odg-save");
    let file = dir.file("plan.odg");
    std::fs::write(&file, small_odg()).unwrap();
    let mut w = edit_window_with([1280.0, 900.0], |app| app.with_file_picker(open_picker(file.clone())));
    key_without_waiting(&mut w, Key::O, ctrl());
    w.rest();
    assert_eq!(w.state().window_title(), "plan.odg - DiaCraft", "{:?}", w.state().open_error());
    drag_tile_to(&mut w, "Rectangle", 4.0, 4.0);
    assert!(w.state().is_dirty());
    key(&mut w, Key::S, ctrl());
    assert!(!w.state().is_dirty(), "{:?}", w.state().open_error());
    assert!(w.state().notice().is_some_and(|n| n.starts_with("Saved plan.odg")), "{:?}", w.state().notice());
    let mut other = Session::new();
    other.execute("file.open", &json!({"path": file.to_str().unwrap()})).unwrap();
    assert_eq!(other.document().unwrap().pages.get(0).unwrap().shapes.len(), 2, "the shape of the file and the one that was dropped");
    assert_eq!(w.state().ui_panics(), 0);
}

/// A curved connector is saved (the writer writes it as cubics; the window's banner used to show the writer's words of a refusal for it): the save is made, the drawing
/// is clean and the file holds a `draw:connector` with a path.
#[test]
fn a_curved_connector_is_saved_by_the_window_and_the_banner_says_nothing_against_it() {
    let dir = TempDir::new("odg-curved");
    let file = dir.file("plan.odg");
    let original = small_odg();
    std::fs::write(&file, &original).unwrap();
    let mut w = edit_window_with([1280.0, 900.0], |app| app.with_file_picker(open_picker(file.clone())));
    key_without_waiting(&mut w, Key::O, ctrl());
    w.rest();
    run_command(&mut w, "shape.drop", json!({"master": "ellipse", "page": 0, "x": 4.0, "y": 4.0}));
    let ids = shape_ids(&w);
    run_command(&mut w, "connector.add", json!({"from": {"id": ids[0]}, "to": {"id": ids[1]}}));
    let connector = *shape_ids(&w).last().unwrap();
    run_command(&mut w, "connector.style", json!({"style": "curved", "ids": [connector]}));
    key(&mut w, Key::S, ctrl());
    assert!(!w.state().is_dirty(), "the drawing is saved");
    assert!(w.state().open_error().is_none(), "no banner: {:?}", w.state().open_error());
    assert_ne!(std::fs::read(&file).unwrap(), original, "the file was written");
}

/// A window that opens `file` with the Open dialog and saves with the Save dialog answering `answer`, which remembers the names it was asked to start with.
fn window_opening(file: &Path, answer: PathBuf, asked: &Arc<Mutex<Vec<String>>>) -> Window {
    let (open, save) = (open_picker(file.to_path_buf()), save_picker(answer, asked));
    let mut w = edit_window_with([1280.0, 900.0], |app| app.with_file_picker(open).with_save_picker(save));
    key_without_waiting(&mut w, Key::O, ctrl());
    w.rest();
    w
}

/// **Save As follows the drawing, not the file's name** (the review): an `.odg` called `a.vsdx` or `a.dat` is offered an `.odg` name, and a
/// name typed with no type gets `.odg`; a VSDX drawing called `v.odg` is offered a `.vsdx` name and a typed name gets `.vsdx`. The type is the one the engine read
/// (what the package is), so the dialog's filters, which go by the name it starts with, and the extension added, are the type the engine writes.
#[test]
fn save_as_offers_and_adds_the_type_the_drawing_was_read_as_whatever_its_file_is_called() {
    let dir = TempDir::new("save-as-types");
    // An .odg called a.vsdx and one called a.dat.
    for name in ["a.vsdx", "a.dat"] {
        let file = dir.file(name);
        std::fs::write(&file, small_odg()).unwrap();
        let asked = Arc::new(Mutex::new(Vec::new()));
        let typed = dir.file(&format!("typed-{}", name.replace('.', "-")));
        let mut w = window_opening(&file, typed.clone(), &asked);
        assert_eq!(w.state().window_title(), format!("{name} - DiaCraft"), "{:?}", w.state().open_error());
        key(&mut w, Key::S, ctrl_shift());
        w.rest();
        assert_eq!(asked.lock().unwrap().as_slice(), ["a.odg"], "{name}: the dialog starts with an .odg name, from which it offers .odg");
        let written = dir.file(&format!("typed-{}.odg", name.replace('.', "-")));
        assert!(written.exists() && !typed.exists(), "{name}: a name typed with no type gets .odg: {:?}", w.state().open_error());
        let mut other = Session::new();
        other.execute("file.open", &json!({"path": written.to_str().unwrap()})).unwrap();
        assert_eq!(other.document().unwrap().pages.get(0).unwrap().shapes.len(), 1);
    }
    // A VSDX drawing called v.odg.
    let mut vsdx = Session::new();
    vsdx.execute("file.new", &json!({})).unwrap();
    let made = dir.file("made.vsdx");
    vsdx.execute("file.save_as", &json!({"path": made.to_str().unwrap()})).unwrap();
    let file = dir.file("v.odg");
    std::fs::rename(&made, &file).unwrap();
    let asked = Arc::new(Mutex::new(Vec::new()));
    let typed = dir.file("typed-v");
    let mut w = window_opening(&file, typed.clone(), &asked);
    assert_eq!(w.state().window_title(), "v.odg - DiaCraft", "{:?}", w.state().open_error());
    key(&mut w, Key::S, ctrl_shift());
    w.rest();
    assert_eq!(asked.lock().unwrap().as_slice(), ["v.vsdx"], "the dialog starts with a .vsdx name, from which it offers the VSDX kinds");
    assert!(dir.file("typed-v.vsdx").exists() && !typed.exists(), "a name typed with no type gets .vsdx: {:?}", w.state().open_error());
}

/// The status bar says what a save that wrote a new shape without what the file has no place for did not write (the first of it), and a shape of the new
/// stencils, which a gate refused until a later change, is dropped without a word.
#[test]
fn the_status_bar_says_nothing_of_what_an_odg_save_writes_and_a_drop_it_writes() {
    let dir = TempDir::new("odg-notes");
    let file = dir.file("plan.odg");
    std::fs::write(&file, small_odg()).unwrap();
    let asked = Arc::new(Mutex::new(Vec::new()));
    let mut w = window_opening(&file, file.clone(), &asked);
    drag_tile_to(&mut w, "Rectangle", 4.0, 4.0);
    assert!(!w.state().notice().is_some_and(|n| n.contains("cannot be saved")), "a rectangle is written: {:?}", w.state().notice());
    // A text formatted on a new shape is written (carries it: the status bar used to say it was not).
    let id = *shape_ids(&w).last().unwrap();
    run_command(&mut w, "shape.set_text", json!({"id": id, "text": "Hello"}));
    run_command(&mut w, "text.format", json!({"ids": [id], "bold": true}));
    key(&mut w, Key::S, ctrl());
    let said = w.state().notice().map(str::to_owned).unwrap_or_default();
    assert!(said.starts_with("Saved plan.odg") && !said.contains("Text formatting") && !said.contains("in the log"), "nothing of it is said: {said}");
    // (Every master of the built-in stencils is written.)
    run_command(&mut w, "window.stencil", json!({"stencil": "arrows"}));
    drag_tile_to(&mut w, "Right Arrow", 7.0, 4.0);
    assert!(!w.state().notice().is_some_and(|n| n.contains("cannot be saved")), "an arrow is written: {:?}", w.state().notice());
}

/// A name typed with no type that is given `.odg` (the drawing came from an `.odg`) names a file that is there: the person is asked before it is replaced, as for the
/// types of the other kind, and Replace writes the drawing as an `.odg` over it.
#[test]
fn a_name_given_the_odg_type_that_names_a_file_that_is_there_is_asked_about_first() {
    let dir = TempDir::new("odg-replace");
    let file = dir.file("plan.odg");
    std::fs::write(&file, small_odg()).unwrap();
    let existing = dir.file("typed.odg");
    std::fs::write(&existing, b"a file that is already here and must not be replaced without asking").unwrap();
    let asked = Arc::new(Mutex::new(Vec::new()));
    let mut w = window_opening(&file, dir.file("typed"), &asked);
    key(&mut w, Key::S, ctrl_shift());
    w.rest();
    assert!(w.state().replace_prompt_open(), "the person is asked");
    assert_eq!(std::fs::read(&existing).unwrap(), b"a file that is already here and must not be replaced without asking", "nothing is written yet");
    w.rest();
    press(&mut w, "replace.replace");
    w.rest();
    assert!(!w.state().replace_prompt_open());
    let mut other = Session::new();
    other.execute("file.open", &json!({"path": existing.to_str().unwrap()})).unwrap();
    assert_eq!(other.document().unwrap().pages.get(0).unwrap().shapes.len(), 1, "the drawing is an .odg there now");
    assert_eq!(w.state().window_title(), "typed.odg - DiaCraft");
}
