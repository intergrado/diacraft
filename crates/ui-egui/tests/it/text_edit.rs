//! The text box and the label it edits (and M1 of the M3 review): the box holds a shape's characters, its fields are shown and not typed
//! into, only the characters that changed are sent, and a label being typed is sent before anything changes or leaves the drawing.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use diacraft_engine::Session;
use diacraft_engine::draw::text_view;
use diacraft_ui_egui::render_worker::bundled_drawer;
use diacraft_ui_egui::{DiacraftApp, FilePicker, SavePicker};
use egui::{Event, Key, Modifiers, PointerButton, Pos2, ViewportCommand, ViewportEvent, ViewportId};
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::editing::{click_at, frame_of, key, key_without_waiting, shape_ids, type_text};
use crate::fixture;
use crate::saving::{ctrl, ctrl_shift};
use crate::support::{Rest, TempDir, Window, commands, fast_window, move_to, press, run_command};

/// The label of the drawing of `labelled_vsdx`: shape 1 of page 1, centred at (3, 6).
const LABEL: u32 = 1;
const AT: (f64, f64) = (3.0, 6.0);

/// Writes the labelled drawing to `dir` and opens it in a window at 100% zoom, with `build` giving the app its options.
pub fn labelled_window(dir: &TempDir, build: impl FnOnce(DiacraftApp) -> DiacraftApp) -> (Window, PathBuf) {
    let path = fixture::write(dir.path(), "labelled.vsdx", &fixture::labelled_vsdx());
    let mut session = Session::new();
    session.execute("file.open", &json!({"path": path.to_str().unwrap()})).unwrap();
    let mut w = fast_window(build(DiacraftApp::new(session, None).with_inline_pictures(bundled_drawer())), [1280.0, 900.0]);
    w.rest();
    run_command(&mut w, "view.zoom", json!({"percent": 100}));
    (w, path)
}

/// The text of shape `LABEL` on page `page`, as an editor sees it.
fn view(w: &Window, page: u32) -> diacraft_engine::draw::TextView {
    text_view(w.state().session().document().unwrap(), page, LABEL).unwrap()
}

/// The first page's XML as it was saved to `path`.
pub fn page_xml(path: &Path) -> String {
    let bytes = std::fs::read(path).unwrap();
    let package = diacraft_opc::Package::read(&bytes, &diacraft_opc::Limits::default()).unwrap();
    String::from_utf8(package.part("/visio/pages/page1.xml").unwrap().to_vec()).unwrap()
}

/// Selects the label and opens its text box with F2.
fn open_box(w: &mut Window) {
    click_at(w, AT.0, AT.1);
    key(w, Key::F2, Modifiers::NONE);
    assert!(w.state().window_state()["editingText"].as_bool().unwrap(), "the text box is open");
}

/// A click whose press and release come in one frame, as a quick click or a touch gives them: the control under the pointer is clicked in the
/// same frame the press is seen, before anything later in the frame (the drawing area's text box) has seen the press.
pub fn tap(w: &mut Window, pos: Pos2) {
    for _ in 0..3 {
        move_to(w, pos);
    }
    for pressed in [true, false] {
        w.input_mut().events.push(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
    }
    w.step();
    w.step();
}

fn save_picker(path: PathBuf) -> SavePicker {
    Arc::new(move |_| Some(path.clone()))
}

fn close_asked(w: &mut Window) -> Vec<ViewportCommand> {
    w.input_mut().viewports.entry(ViewportId::ROOT).or_default().events.push(ViewportEvent::Close);
    w.step();
    commands(w)
}

// ---- H2: the box edits the characters, and fields are shown apart --------------------------------------------------------------------------

/// The box holds `abcd`, not `abcd3`; the field is on the screen as a piece that cannot be typed into; one changed letter sends one letter, and
/// the runs and the field are in the saved file as they were, and in the file read back.
#[test]
fn one_changed_letter_keeps_the_runs_and_the_field_saved_and_reopened() {
    let dir = TempDir::new("label");
    let (mut w, path) = labelled_window(&dir, |app| app);
    assert_eq!(view(&w, 1).chars, "abcd");
    let before = w.state().session().revision();
    open_box(&mut w);
    assert_eq!(w.state().edited_text(), Some("abcd"), "the box holds the characters, and not the field's result");
    assert_eq!(w.state().edited_text_fields(), ["3"]);
    let chip = w.state().element("shape.text.field.0").expect("the field is drawn").clone();
    assert!(!chip.enabled, "and it takes no keys and no pointer");
    w.get_by_label("Field 3");
    // Clicking the field does not end the edit or put the cursor in it.
    click_at(&mut w, 0.0, 0.0);
    assert!(!w.state().window_state()["editingText"].as_bool().unwrap(), "a click away ends the edit");
    open_box(&mut w);
    let chip = w.state().element("shape.text.field.0").expect("the field is drawn again").clone();
    tap(&mut w, chip.rect.center());
    assert!(w.state().window_state()["editingText"].as_bool().unwrap(), "a click on the field is a click on the box");
    assert_eq!(w.state().edited_text(), Some("abcd"));
    // One letter: End, Backspace, and a new letter.
    key(&mut w, Key::End, Modifiers::NONE);
    key(&mut w, Key::Backspace, Modifiers::NONE);
    type_text(&mut w, "e");
    assert_eq!(w.state().edited_text(), Some("abce"));
    assert_eq!(w.state().session().revision(), before, "nothing is sent while the box is open");
    key(&mut w, Key::Escape, Modifiers::NONE);
    let edited = view(&w, 1);
    assert_eq!((edited.chars.as_str(), edited.fields.len()), ("abce", 1), "the letter changed, and the field is still there");
    assert_eq!(w.state().session().revision(), before + 1, "one command");
    // Saved: the markers are where they were, the rows they name too, and the field is the one the file had.
    key(&mut w, Key::S, ctrl());
    w.rest();
    assert!(!w.state().is_dirty());
    let xml = page_xml(&path);
    assert!(xml.contains(fixture::LABEL_TEXT_AFTER), "{xml}");
    assert!(xml.contains(r#"<Row IX="0"><Cell N="Style" V="1""#) && xml.contains(r#"<Row IX="1"><Cell N="Style" V="2""#), "the runs' rows: {xml}");
    assert!(xml.contains(r#"<Section N="Field">"#), "the field's row: {xml}");
    // Reopened: the same, in a new window.
    let mut again = Session::new();
    again.execute("file.open", &json!({"path": path.to_str().unwrap()})).unwrap();
    let reread = text_view(again.document().unwrap(), 1, LABEL).unwrap();
    assert_eq!((reread.chars.as_str(), reread.fields.len(), reread.fields[0].text.as_str()), ("abce", 1, "3"));
    assert_eq!(w.state().ui_panics(), 0);
}

/// The box that is closed with nothing changed sends nothing, and a label with a field is no exception.
#[test]
fn a_box_closed_with_nothing_changed_sends_nothing() {
    let dir = TempDir::new("label-same");
    let (mut w, _) = labelled_window(&dir, |app| app);
    let before = w.state().session().revision();
    open_box(&mut w);
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert_eq!(w.state().session().revision(), before);
    assert!(!w.state().is_dirty());
}

/// Typing over all the characters replaces them and keeps the field and the run markers.
#[test]
fn typing_over_all_the_characters_keeps_the_field() {
    let dir = TempDir::new("label-all");
    let (mut w, path) = labelled_window(&dir, |app| app);
    open_box(&mut w);
    type_text(&mut w, "XY");
    key(&mut w, Key::Escape, Modifiers::NONE);
    let edited = view(&w, 1);
    assert_eq!((edited.chars.as_str(), edited.fields.len()), ("XY", 1));
    key(&mut w, Key::S, ctrl());
    w.rest();
    let xml = page_xml(&path);
    assert!(xml.contains(r#"<Text><cp IX="0"/>XY<cp IX="1"/><fld IX="0">3</fld></Text>"#), "{xml}");
}

/// A paste that carries characters a file cannot hold does not lose the label: they are left out. (Not U+FFFE: the test window's fonts panic on a
/// character they have no glyph for, where the real window draws a box; the unit test of `storable` covers it.)
#[test]
fn a_paste_with_control_characters_is_kept_without_them() {
    let dir = TempDir::new("label-paste");
    let (mut w, _) = labelled_window(&dir, |app| app);
    open_box(&mut w);
    w.input_mut().events.push(Event::Paste("a\u{1b}b\u{7}c".to_owned()));
    w.step();
    w.rest();
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert_eq!(view(&w, 1).chars, "abc", "{:?}", w.state().notice());
    assert!(w.state().notice().is_none(), "{:?}", w.state().notice());
}

// ---- M1: a label being typed is sent before the drawing changes or is left ---------------------------------------------------------------

/// A page tab clicked in the frame it is pressed: the tab is drawn before the drawing area, which would end the edit on the press. The label is
/// sent to the page it is on, and then the page is shown.
#[test]
fn a_page_tab_clicked_while_typing_sends_the_label_to_its_page() {
    let dir = TempDir::new("label-tab");
    let (mut w, _) = labelled_window(&dir, |app| app);
    open_box(&mut w);
    type_text(&mut w, "Hi");
    let tab = w.state().element("page-tab.2").unwrap().rect.center();
    tap(&mut w, tab);
    assert_eq!(w.state().window_state()["page"], 2, "the second page is shown");
    assert_eq!(view(&w, 1).chars, "Hi", "and the label went to the first, where it was typed");
    assert!(!w.state().window_state()["editingText"].as_bool().unwrap());
    // The All menu and page.select do the same.
    run_command(&mut w, "page.select", json!({"id": 1}));
    open_box(&mut w);
    type_text(&mut w, "Ho");
    run_command(&mut w, "page.select", json!({"id": 2}));
    assert_eq!(view(&w, 1).chars, "Ho");
}

/// Undo clicked in the frame it is pressed takes back the label that was being typed, and not the step before it.
#[test]
fn undo_clicked_while_typing_takes_back_the_label_and_not_the_step_before() {
    let dir = TempDir::new("label-undo");
    let (mut w, _) = labelled_window(&dir, |app| app);
    run_command(&mut w, "shape.move", json!({"ids": [LABEL], "dx": 1.0, "dy": 0.0}));
    let moved = frame_of(&w, LABEL).0;
    open_box(&mut w);
    type_text(&mut w, "Hi");
    let undo = w.state().element("qat.Undo").unwrap().rect.center();
    tap(&mut w, undo);
    w.rest();
    assert_eq!(frame_of(&w, LABEL).0, moved, "the move is not undone");
    assert_eq!(view(&w, 1).chars, "abcd", "the label was sent, and then taken back");
    assert!(!w.state().window_state()["editingText"].as_bool().unwrap());
    // Redo brings the label back.
    let redo = w.state().element("qat.Redo").unwrap().rect.center();
    tap(&mut w, redo);
    w.rest();
    assert_eq!(view(&w, 1).chars, "Hi");
}

/// Save clicked in the frame it is pressed writes the label that was being typed.
#[test]
fn save_clicked_while_typing_writes_the_label() {
    let dir = TempDir::new("label-save");
    let (mut w, path) = labelled_window(&dir, |app| app);
    open_box(&mut w);
    type_text(&mut w, "Hi");
    assert!(w.state().window_title().starts_with('*'), "a label being typed is a change the title shows: {}", w.state().window_title());
    let save = w.state().element("qat.Save").unwrap().rect.center();
    tap(&mut w, save);
    w.rest();
    let xml = page_xml(&path);
    assert!(xml.contains(r#"<Text><cp IX="0"/>Hi<cp IX="1"/><fld IX="0">3</fld></Text>"#), "{xml}");
    assert!(!w.state().is_dirty(), "and the drawing is saved with it");
}

/// Ctrl+S in the text box saves too, with the label.
#[test]
fn ctrl_s_in_the_text_box_saves_the_label() {
    let dir = TempDir::new("label-ctrl-s");
    let (mut w, path) = labelled_window(&dir, |app| app);
    open_box(&mut w);
    type_text(&mut w, "Hi");
    key(&mut w, Key::S, ctrl());
    w.rest();
    assert!(page_xml(&path).contains("Hi<cp IX=\"1\"/>"), "{}", page_xml(&path));
}

/// Closing from the desktop's frame or Alt+F4 with a label being typed asks about unsaved changes: the label is one.
#[test]
fn closing_with_a_label_being_typed_asks() {
    let dir = TempDir::new("label-close");
    let (mut w, _) = labelled_window(&dir, |app| app);
    assert!(!w.state().is_dirty(), "the drawing as opened has nothing to save");
    open_box(&mut w);
    type_text(&mut w, "Hi");
    let sent = close_asked(&mut w);
    assert!(sent.iter().any(|c| matches!(c, ViewportCommand::CancelClose)), "the close was held back");
    assert!(w.state().prompt_open(), "and the person is asked");
    assert_eq!(view(&w, 1).chars, "Hi", "the label is in the drawing the question is about");
    w.rest();
    let at = w.state().element("prompt.dont_save").unwrap().rect.center();
    crate::support::click(&mut w, at);
    assert!(commands(&w).iter().any(|c| matches!(c, ViewportCommand::Close)));
}

/// New and Open with a label being typed ask first, whatever the drawing was before the label: the label is a change.
#[test]
fn new_and_open_with_a_label_being_typed_ask_first() {
    let dir = TempDir::new("label-new");
    let other = fixture::write(dir.path(), "other.vsdx", &fixture::labelled_vsdx());
    let picker: FilePicker = Arc::new(move || Some(other.clone()));
    let (mut w, _) = labelled_window(&dir, |app| app.with_file_picker(picker));
    open_box(&mut w);
    type_text(&mut w, "Hi");
    key(&mut w, Key::N, ctrl());
    assert!(w.state().prompt_open(), "New asks");
    assert_eq!(view(&w, 1).chars, "Hi");
    press(&mut w, "prompt.cancel");
    assert!(!w.state().prompt_open() && !shape_ids(&w).is_empty(), "Cancel leaves the drawing, with the label in it");
    // Open, from a clean start: undo the label so the drawing is clean, type again, and ask for a file.
    run_command(&mut w, "edit.undo", json!({}));
    assert!(!w.state().is_dirty());
    open_box(&mut w);
    type_text(&mut w, "Ho");
    key_without_waiting(&mut w, Key::O, ctrl());
    w.rest();
    assert!(w.state().prompt_open(), "Open asks once the file is chosen");
    assert_eq!(view(&w, 1).chars, "Ho");
}

/// Opening the File view with a label being typed sends it; so does Save As.
#[test]
fn the_file_view_and_save_as_send_the_label() {
    let dir = TempDir::new("label-backstage");
    let target = dir.file("copy.vsdx");
    let (mut w, _) = labelled_window(&dir, |app| app.with_save_picker(save_picker(target.clone())));
    open_box(&mut w);
    type_text(&mut w, "Hi");
    run_command(&mut w, "window.backstage", json!({"page": "Home"}));
    assert_eq!(view(&w, 1).chars, "Hi");
    run_command(&mut w, "window.backstage", json!({"page": "close"}));
    open_box(&mut w);
    type_text(&mut w, "Ho");
    key(&mut w, Key::S, ctrl_shift());
    w.rest();
    assert!(page_xml(&target).contains("Ho<cp IX=\"1\"/>"), "{}", page_xml(&target));
}

/// Zoom, a panel and the theme leave the box open; a query a program polls with does not close it.
#[test]
fn view_commands_and_queries_leave_the_box_open() {
    let dir = TempDir::new("label-view");
    let (mut w, _) = labelled_window(&dir, |app| app);
    open_box(&mut w);
    type_text(&mut w, "Hi");
    for (id, params) in [
        ("view.zoomIn", json!({})),
        ("window.theme", json!({"theme": "dark"})),
        ("window.shapes", json!({})),
        ("document.inspect", json!({})),
        ("file.status", json!({})),
    ] {
        w.state_mut().execute(id, &params).unwrap();
        w.rest();
        assert!(w.state().window_state()["editingText"].as_bool().unwrap(), "{id} closed the box");
    }
    assert_eq!(w.state().edited_text(), Some("Hi"));
}

/// An export is made from the drawing as it is when the Export dialog answers, and a label being typed meanwhile is part of it: the label is
/// sent before the copy the export draws is taken (the dialog runs on a thread of its own and the person goes on typing while it is open).
#[test]
fn an_export_is_of_the_drawing_with_the_label_typed_while_its_dialog_was_open() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let dir = TempDir::new("label-export");
    let (during, after) = (dir.file("during.svg"), dir.file("after.svg"));
    let gate = Arc::new(AtomicBool::new(false));
    let answers = Arc::new(Mutex::new(vec![after.clone(), during.clone()]));
    let picker = {
        let (gate, answers) = (Arc::clone(&gate), Arc::clone(&answers));
        Arc::new(move |_: String| {
            let path = answers.lock().unwrap().pop();
            // The first dialog answers at once; the second waits until the test lets it go (and for thirty seconds at most).
            if path.as_ref().is_some_and(|p| p.ends_with("during.svg")) {
                for _ in 0..3000 {
                    if gate.load(Ordering::SeqCst) {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            }
            path
        })
    };
    let (mut w, _) = labelled_window(&dir, |app| app.with_export_picker(picker));
    run_command(&mut w, "window.tab", json!({"tab": "View"}));
    let button = |w: &Window| w.state().element("ribbon.View.ExportPicture").expect("the View tab has a Picture button").rect.center();
    // The first dialog answers `during.svg` (the answers are taken from the end), and waits. The label is typed while it is open.
    let at = button(&w);
    crate::support::click(&mut w, at);
    assert!(w.state().export_dialog_open(), "the dialog is open");
    // The dialog is held open on purpose, so nothing here waits for its thread: the window is rested as it is (`rest_holding`).
    let label = crate::editing::at(&w, AT.0, AT.1);
    crate::support::click(&mut w, label);
    w.rest_holding();
    key_without_waiting(&mut w, Key::F2, Modifiers::NONE);
    w.rest_holding();
    assert!(w.state().window_state()["editingText"].as_bool().unwrap(), "the text box is open");
    w.input_mut().events.push(Event::Text("Ho".to_owned()));
    w.step();
    w.rest_holding();
    assert!(w.state().has_unsent_text() && w.state().export_dialog_open());
    gate.store(true, Ordering::SeqCst);
    w.rest();
    assert_eq!(view(&w, 1).chars, "Ho", "the label was sent when the dialog answered");
    // The same drawing, exported again with the label in it and no box open.
    let at = button(&w);
    crate::support::click(&mut w, at);
    w.rest();
    assert_eq!(std::fs::read(&during).unwrap(), std::fs::read(&after).unwrap(), "the picture is of the drawing with the label");
}
