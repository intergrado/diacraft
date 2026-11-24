//! A flat OpenDocument drawing (`.fodg`) in the window. The Open dialog takes it and the engine reads it; Save writes a drawing
//! that came from one back as a flat file, in place, and says in the banner, in words, what the flat writer refuses; Save As starts from an `.odg` name and never offers
//! `.fodg`. Save As is usable for a drawing that came from a flat file, as for any other (it was greyed out with a note), and writes the types the
//! engine allows. The file is written by hand from the standard.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use diacraft_engine::Session;
use diacraft_ui_egui::{FilePicker, SavePicker};
use egui::Key;
use serde_json::json;

use crate::editing::{drag_tile_to, edit_window_with, key, key_without_waiting, shape_ids};
use crate::saving::{ctrl, ctrl_shift};
use crate::support::{Rest, TempDir, Window, press, run_command};

/// A flat drawing of one rectangle, `A`: one `office:document` with the media type on its root and the children in the order the standard gives.
fn small_fodg() -> Vec<u8> {
    const NS: &str = concat!(
        r#"xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" "#,
        r#"xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" "#,
        r#"xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0""#
    );
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document {NS} office:mimetype=\"application/vnd.oasis.opendocument.graphics\" office:version=\"1.3\"><office:automatic-styles><style:style style:name=\"gr1\" style:family=\"graphic\"><style:graphic-properties draw:fill=\"solid\" draw:fill-color=\"#3465a4\"/></style:style><style:page-layout style:name=\"PM1\"><style:page-layout-properties fo:page-width=\"21cm\" fo:page-height=\"29.7cm\" style:print-orientation=\"portrait\"/></style:page-layout></office:automatic-styles><office:master-styles><draw:layer-set><draw:layer draw:name=\"layout\"/></draw:layer-set><style:master-page style:name=\"Default\" style:page-layout-name=\"PM1\"/></office:master-styles><office:body><office:drawing><draw:page draw:name=\"page1\" draw:master-page-name=\"Default\"><draw:custom-shape draw:style-name=\"gr1\" draw:layer=\"layout\" draw:name=\"A\" svg:width=\"4cm\" svg:height=\"2cm\" svg:x=\"2cm\" svg:y=\"3cm\"><text:p/><draw:enhanced-geometry svg:viewBox=\"0 0 21600 21600\" draw:type=\"non-primitive\" draw:enhanced-path=\"M 0 0 L 21600 0 21600 21600 0 21600 0 0 Z N\"/></draw:custom-shape></draw:page></office:drawing></office:body></office:document>"
    )
    .into_bytes()
}

fn open_picker(path: PathBuf) -> FilePicker {
    Arc::new(move || Some(path.clone()))
}

fn save_picker(path: PathBuf, asked: &Arc<Mutex<Vec<String>>>) -> SavePicker {
    let asked = Arc::clone(asked);
    Arc::new(move |request| {
        asked.lock().unwrap().push(request.name);
        Some(path.clone())
    })
}

/// A window that has opened `file` with the Open dialog, and saves with a Save dialog that answers `answer` and remembers the names it was asked to start with.
fn window_opening(file: &Path, answer: PathBuf, asked: &Arc<Mutex<Vec<String>>>) -> Window {
    let (open, save) = (open_picker(file.to_path_buf()), save_picker(answer, asked));
    let mut w = edit_window_with([1280.0, 900.0], |app| app.with_file_picker(open).with_save_picker(save));
    key_without_waiting(&mut w, Key::O, ctrl());
    w.rest();
    w
}

/// The Open dialog's answer, a `.fodg`, is opened as any drawing is: the window is named for the file and shows its shape, with no word of trouble.
#[test]
fn odg_package_a_fodg_chosen_in_the_open_dialog_is_opened_in_the_window() {
    let dir = TempDir::new("fodg-open");
    for name in ["plan.fodg", "plan.odg", "plan"] {
        let file = dir.file(name);
        std::fs::write(&file, small_fodg()).unwrap();
        let asked = Arc::new(Mutex::new(Vec::new()));
        let w = window_opening(&file, dir.file("unused"), &asked);
        assert_eq!(w.state().window_title(), format!("{name} - DiaCraft"), "{name}: {:?}", w.state().open_error());
        assert_eq!(shape_ids(&w).len(), 1, "{name}: its one rectangle");
        assert!(w.state().open_error().is_none(), "{name}: {:?}", w.state().open_error());
        assert!(!w.state().is_dirty(), "{name}");
        assert_eq!(w.state().ui_panics(), 0);
    }
}

/// A file that is not a drawing is refused in the window's words, and the words name the kinds of file that open, the OpenDocument ones among them.
#[test]
fn odg_package_a_the_words_of_an_open_that_failed_name_the_odg_kinds() {
    let dir = TempDir::new("fodg-not-a-drawing");
    let file = dir.file("notes.txt");
    std::fs::write(&file, b"nothing of a drawing").unwrap();
    let asked = Arc::new(Mutex::new(Vec::new()));
    let w = window_opening(&file, dir.file("unused"), &asked);
    let said = w.state().open_error().expect("the refusal is shown").to_owned();
    assert!(said.contains("notes.txt") && said.contains(".vsdx") && said.contains(".odg") && said.contains(".fodg"), "{said}");
    assert!(!said.contains("not_zip") && !said.contains("::"), "{said}");
}

/// Save of a drawing that came from a flat file writes it back as a flat file, in place: the file stays one flat document, it holds the change, and
/// the window says it is saved.
#[test]
fn odg_package_save_of_a_drawing_from_a_flat_file_writes_the_flat_file_in_place() {
    let dir = TempDir::new("fodg-save");
    let file = dir.file("plan.fodg");
    let original = small_fodg();
    std::fs::write(&file, &original).unwrap();
    let asked = Arc::new(Mutex::new(Vec::new()));
    let mut w = window_opening(&file, dir.file("unused"), &asked);
    let id = shape_ids(&w)[0];
    run_command(&mut w, "shape.move", json!({ "ids": [id], "dx": 1.0, "dy": 0.0 }));
    assert!(w.state().is_dirty());
    key(&mut w, Key::S, ctrl());
    w.rest();
    assert!(!w.state().is_dirty(), "{:?}", w.state().open_error());
    assert!(w.state().open_error().is_none(), "{:?}", w.state().open_error());
    assert!(w.state().notice().is_some_and(|n| n.starts_with("Saved plan.fodg")), "{:?}", w.state().notice());
    assert!(asked.lock().unwrap().is_empty(), "Save in place asks for no name");
    let written = std::fs::read(&file).unwrap();
    assert_ne!(written, original, "the change is in the file");
    let text = String::from_utf8(written).expect("a flat document is text, not a package");
    assert!(text.starts_with("<?xml") && text.matches("<office:document ").count() == 1, "and the file is still one flat document");
    let mut other = Session::new();
    other.execute("file.open", &json!({"path": file.to_str().unwrap()})).unwrap();
    assert_eq!(other.document().unwrap().pages.get(0).unwrap().shapes.len(), 1);
    assert!(!dir.file("plan.odg").exists());
    assert_eq!(w.state().ui_panics(), 0);
}

/// What the flat writer does not write yet (a master a stencil shape brings) is a refused save, said in the banner with the way out of a flat file,
/// and the file and the drawing stay as they were.
#[test]
fn odg_package_a_change_the_flat_writer_refuses_is_the_banner_with_the_way_out_of_a_flat_file() {
    let dir = TempDir::new("fodg-refused");
    let file = dir.file("plan.fodg");
    let original = small_fodg();
    std::fs::write(&file, &original).unwrap();
    let asked = Arc::new(Mutex::new(Vec::new()));
    let mut w = window_opening(&file, dir.file("unused"), &asked);
    drag_tile_to(&mut w, "Rectangle", 4.0, 4.0);
    assert!(w.state().is_dirty());
    key(&mut w, Key::S, ctrl());
    w.rest();
    let said = w.state().open_error().map(str::to_owned).unwrap_or_default();
    assert!(said.starts_with("plan.fodg was not saved."), "{said}");
    assert!(said.contains("masters") && said.contains("saved only as a flat file"), "{said}");
    assert!(said.contains("the file on the disk was not changed"), "{said}");
    assert!(w.state().is_dirty(), "the drawing still has the change");
    assert_eq!(std::fs::read(&file).unwrap(), original, "the file is as it was");
}

/// A Save dialog that answers `answer` and remembers the name it was asked to start with and the extensions of the kinds it was offered.
fn recording_picker(answer: PathBuf, asked: &Arc<Mutex<Vec<String>>>, offered: &Arc<Mutex<Vec<Vec<String>>>>) -> SavePicker {
    let (asked, offered) = (Arc::clone(asked), Arc::clone(offered));
    Arc::new(move |request| {
        asked.lock().unwrap().push(request.name);
        offered.lock().unwrap().push(request.kinds.iter().map(|kind| kind.extension.to_owned()).collect());
        Some(answer.clone())
    })
}

/// A window that has opened `file` with the Open dialog and saves with `picker`.
fn window_with_picker(file: &Path, picker: SavePicker) -> Window {
    let open = open_picker(file.to_path_buf());
    let mut w = edit_window_with([1280.0, 900.0], |app| app.with_file_picker(open).with_save_picker(picker));
    key_without_waiting(&mut w, Key::O, ctrl());
    w.rest();
    w
}

/// Greyed it out with a note: Save As is usable for a drawing that came from a flat file, as for any other. The File view's entry is
/// enabled with no note, the key asks for a name with the `.odg` the drawing is saved as first and `.fodg` never among the kinds, and the answer writes an `.odg` package, with the flat
/// file as it was; the window is the new file's.
#[test]
fn odg_package_save_as_is_usable_for_a_drawing_from_a_flat_file_and_writes_an_odg() {
    let dir = TempDir::new("fodg-save-as");
    let file = dir.file("plan.fodg");
    let original = small_fodg();
    std::fs::write(&file, &original).unwrap();
    let (asked, offered) = (Arc::new(Mutex::new(Vec::new())), Arc::new(Mutex::new(Vec::new())));
    let answer = dir.file("other.odg");
    let mut w = window_with_picker(&file, recording_picker(answer.clone(), &asked, &offered));
    assert_eq!(w.state().command_enabled("file.save_as"), Ok(()));
    assert!(w.state().session().commands().into_iter().find(|c| c.id == "file.save_as").unwrap().disabled_reason.is_none());
    assert!(w.state().can_save_as() && w.state().can_save(), "Save As and Save are usable");
    run_command(&mut w, "window.backstage", json!({"page": "Home"}));
    assert!(w.state().element("backstage.Save").unwrap().enabled, "Save");
    assert!(w.state().element("backstage.Save As").unwrap().enabled, "Save As is usable");
    run_command(&mut w, "window.backstage", json!({"page": "close"}));
    key(&mut w, Key::S, ctrl_shift());
    w.rest();
    assert_eq!(*asked.lock().unwrap(), ["plan.odg"], "the dialog starts from the type the drawing is saved as");
    let kinds = offered.lock().unwrap()[0].clone();
    assert_eq!(kinds.first().map(String::as_str), Some("odg"), "the drawing's own type first: {kinds:?}");
    for wanted in ["odg", "diacraft", "vsdx"] {
        assert!(kinds.iter().any(|k| k == wanted), "{wanted} is offered: {kinds:?}");
    }
    assert!(!kinds.iter().any(|k| k.eq_ignore_ascii_case("fodg")), "a flat file is never a kind: {kinds:?}");
    assert!(w.state().open_error().is_none(), "{:?}", w.state().open_error());
    assert!(!w.state().is_dirty(), "the drawing is saved");
    assert_eq!(std::fs::read(&file).unwrap(), original, "the flat file is as it was");
    let written = std::fs::read(&answer).unwrap();
    assert!(written.starts_with(b"PK"), "the file written is a package, not a flat document");
    assert_eq!(w.state().window_title(), "other.odg - DiaCraft");
    assert_eq!(w.state().ui_panics(), 0);
}

/// The question a save puts when something else changed the file on the disk has Overwrite, Save As and Cancel for a drawing from a flat file as for any other (Save
/// As is usable for it), and Overwrite saves the flat file in place again.
#[test]
fn odg_package_the_question_about_a_changed_flat_file_has_save_as_and_overwrite_saves_it_flat() {
    let dir = TempDir::new("fodg-changed");
    let file = dir.file("plan.fodg");
    std::fs::write(&file, small_fodg()).unwrap();
    let asked = Arc::new(Mutex::new(Vec::new()));
    let mut w = window_opening(&file, dir.file("other.odg"), &asked);
    let id = shape_ids(&w)[0];
    run_command(&mut w, "shape.move", json!({ "ids": [id], "dx": 1.0, "dy": 0.0 }));
    let changed = String::from_utf8(small_fodg()).unwrap().replace("draw:name=\"A\"", "draw:name=\"Changed\"");
    std::fs::write(&file, &changed).unwrap();
    key(&mut w, Key::S, ctrl());
    w.rest();
    assert!(w.state().changed_prompt_open(), "the person is asked");
    w.rest();
    assert!(w.state().element("changed.overwrite").unwrap().enabled, "Overwrite");
    assert!(w.state().element("changed.save_as").unwrap().enabled, "Save As is there for a drawing from a flat file");
    assert!(w.state().element("changed.cancel").unwrap().enabled, "Cancel");
    press(&mut w, "changed.overwrite");
    w.rest();
    assert!(!w.state().changed_prompt_open() && !w.state().is_dirty(), "{:?}", w.state().open_error());
    let written = String::from_utf8(std::fs::read(&file).unwrap()).unwrap();
    assert!(
        written.starts_with("<?xml") && written.matches("<office:document ").count() == 1 && !written.contains("Changed"),
        "the drawing is the flat file again"
    );
    assert!(asked.lock().unwrap().is_empty(), "no dialog was asked for a name");
}

/// What `command_enabled` says no to is quiet at the key, in a drawing from a flat file as in any other: Ctrl+Z with nothing to undo says nothing, and so does every key of a
/// command that cannot run. The key of Save As, which can run for it, opens the dialog and says nothing against it (item 5 had it say why it was out).
#[test]
fn odg_package_a_key_of_a_command_that_cannot_run_is_quiet_in_a_flat_drawing_and_save_as_asks_for_a_name() {
    let dir = TempDir::new("fodg-quiet-keys");
    let file = dir.file("plan.fodg");
    std::fs::write(&file, small_fodg()).unwrap();
    let asked = Arc::new(Mutex::new(Vec::new()));
    let mut w = window_opening(&file, dir.file("other.odg"), &asked);
    assert!(w.state().notice().is_none(), "{:?}", w.state().notice());
    assert!(w.state().session().history().is_some_and(|h| h.undo_len() == 0), "nothing to undo");
    assert!(w.state().command_enabled("edit.undo").is_err());
    key(&mut w, Key::Z, ctrl());
    assert!(w.state().notice().is_none(), "a key of a command that cannot run is quiet: {:?}", w.state().notice());
    assert!(w.state().open_error().is_none());
    key(&mut w, Key::S, ctrl_shift());
    w.rest();
    assert_eq!(*asked.lock().unwrap(), ["plan.odg"], "the key of Save As asks for a name");
    assert!(w.state().notice().is_none_or(|n| !n.contains("flat")), "and says nothing against it: {:?}", w.state().notice());
}

/// While the Save dialog is open, a second Ctrl+Shift+S asks for no second dialog: the key is quiet (the first is waiting for its answer), and the answer of the first is
/// what saves.
#[test]
fn odg_package_a_second_save_as_key_while_the_dialog_is_open_asks_for_no_second_dialog() {
    use std::sync::mpsc::channel;
    use std::time::Duration;
    let dir = TempDir::new("fodg-two-dialogs");
    let file = dir.file("plan.odg");
    std::fs::write(&file, crate::saving::small_odg()).unwrap();
    let answer = dir.file("other.odg");
    let asked = Arc::new(Mutex::new(Vec::new()));
    let (release, gate) = channel::<()>();
    let gate = Arc::new(Mutex::new(gate));
    let picker: SavePicker = {
        let (asked, gate, answer) = (Arc::clone(&asked), Arc::clone(&gate), answer.clone());
        Arc::new(move |request| {
            asked.lock().unwrap().push(request.name);
            // Held until the test lets it answer (bounded: a picker nobody releases answers after five seconds).
            let _ = gate.lock().unwrap().recv_timeout(Duration::from_secs(5));
            Some(answer.clone())
        })
    };
    let open = open_picker(file.clone());
    let mut w = edit_window_with([1280.0, 900.0], |app| app.with_file_picker(open).with_save_picker(picker));
    key_without_waiting(&mut w, Key::O, ctrl());
    w.rest();
    assert_eq!(w.state().window_title(), "plan.odg - DiaCraft", "{:?}", w.state().open_error());
    key_without_waiting(&mut w, Key::S, ctrl_shift());
    for _ in 0..400 {
        if asked.lock().unwrap().len() == 1 {
            break;
        }
        w.step();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(w.state().save_dialog_open(), "the dialog is open");
    assert_eq!(asked.lock().unwrap().len(), 1, "the first key asked for a dialog");
    key_without_waiting(&mut w, Key::S, ctrl_shift());
    // Time for a thread that a second key would have started to ask (it asks before it waits).
    for _ in 0..40 {
        w.step();
        std::thread::sleep(Duration::from_millis(5));
    }
    // Counted first and the pickers released (two answers: a second dialog, if there is one, must not be left waiting) before the test says what it found.
    let asked_by_now = asked.lock().unwrap().clone();
    let _ = release.send(());
    let _ = release.send(());
    assert_eq!(asked_by_now.len(), 1, "a second key asked for no second dialog: {asked_by_now:?}");
    w.rest();
    assert!(!w.state().save_dialog_open());
    assert!(answer.exists(), "the answer of the first dialog saved the drawing: {:?}", w.state().open_error());
    assert_eq!(asked.lock().unwrap().len(), 1);
}
