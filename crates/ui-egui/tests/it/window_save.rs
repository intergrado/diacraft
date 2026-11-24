//! The View settings are the drawing's. A box of the View tab turned writes into the drawing (`window.settings.set`), a save keeps it, and
//! a drawing opened again shows what it holds: a new drawing's settings when it holds no windows part.
//!
//! The Save dialog is stood in for by a picker that answers a path of the test's, and the second window opens the file with Ctrl+O, as a person does. Every file is
//! built in code.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui::{Key, ViewportCommand, ViewportEvent, ViewportId};
use egui_kittest::kittest::Queryable;
use serde_json::{Value, json};

use crate::editing::{drag_tile_to, edit_window_with, frame_of, key, key_without_waiting, shape_ids};
use crate::fixture;
use crate::saving::{ctrl, ctrl_shift};
use crate::support::{Rest, TempDir, Window, click, commands, press, run_command};
use diacraft_ui_egui::{FilePicker, SavePicker};

fn save_picker(answer: PathBuf) -> SavePicker {
    Arc::new(move |_| Some(answer.clone()))
}

/// A window on a new drawing whose Save dialog answers `answer`.
fn new_window(answer: PathBuf) -> Window {
    edit_window_with([1280.0, 900.0], |app| app.with_save_picker(save_picker(answer)))
}

/// A window whose Open dialog answers `file`, with `file` opened by Ctrl+O; its Save dialog answers `answer` when there is one.
fn window_on(file: &Path, answer: Option<PathBuf>) -> Window {
    let path = file.to_path_buf();
    let open: FilePicker = Arc::new(move || Some(path.clone()));
    let mut w = edit_window_with([1280.0, 900.0], |app| {
        let app = app.with_file_picker(open);
        match answer {
            Some(answer) => app.with_save_picker(save_picker(answer)),
            None => app,
        }
    });
    key_without_waiting(&mut w, Key::O, ctrl());
    w.rest();
    assert!(w.state().open_error().is_none(), "{} did not open: {:?}", file.display(), w.state().open_error());
    w
}

/// What the window shows, `(grid, rulers, guides, connection points)`, and that the drawing area draws it.
fn shown(w: &Window) -> (bool, bool, bool, bool) {
    let s: Value = w.state().window_state();
    let b = |k: &str| s[k].as_bool().unwrap_or_else(|| panic!("{k} in {s}"));
    let shown = (b("grid"), b("rulers"), b("guides"), b("connectionPoints"));
    let report = w.state().canvas_report();
    assert_eq!((report.grid_lines.0 > 0, report.rulers), (shown.0, shown.1), "the drawing area draws what the boxes say: {report:?}");
    shown
}

/// A new drawing's settings.
const NEW: (bool, bool, bool, bool) = (true, true, true, false);

/// Test 1 of the brief: a new drawing saved as `.diacraft` comes back with the grid and the rulers on, as it showed them before the save (the concern 1).
#[test]
fn a_new_drawing_saved_as_diacraft_reopens_with_the_grid_and_the_rulers_on() {
    let dir = TempDir::new("winsave-new");
    let target = dir.file("new.diacraft");
    let mut w = new_window(target.clone());
    assert_eq!(shown(&w), NEW);
    key(&mut w, Key::S, ctrl_shift());
    assert!(target.exists());
    let again = window_on(&target, None);
    assert_eq!(shown(&again), NEW, "the grid and the rulers on, as before the save");
}

/// Test 2 of the brief: the grid turned off by its box is off again in a window that opens the saved file, as `.diacraft` and as VSDX; the box turned is an edit of the
/// drawing (the title's star, and closing asks) and no undo step (Ctrl+Z leaves it).
#[test]
fn the_grid_turned_off_is_off_again_after_a_save_and_a_reopen() {
    let dir = TempDir::new("winsave-off");
    for name in ["off.diacraft", "off.vsdx"] {
        let target = dir.file(name);
        let mut w = new_window(target.clone());
        run_command(&mut w, "window.tab", json!({"tab": "View"}));
        w.get_by_label("Grid").click();
        w.rest();
        assert_eq!(shown(&w), (false, true, true, false), "{name}");
        assert!(w.state().is_dirty(), "{name}: a box turned is an edit of the drawing");
        key(&mut w, Key::S, ctrl_shift());
        assert!(target.exists() && !w.state().is_dirty(), "{name}");
        let again = window_on(&target, None);
        assert_eq!(shown(&again), (false, true, true, false), "{name}: the grid is off again");
    }
    // A box turned is no undo step: Ctrl+Z has nothing to undo and leaves it where it was turned, and the drawing stays changed.
    let mut w = new_window(dir.file("undo.diacraft"));
    run_command(&mut w, "view.rulers", json!({"on": false}));
    assert_eq!(shown(&w), (true, false, true, false));
    key(&mut w, Key::Z, ctrl());
    assert_eq!(shown(&w), (true, false, true, false), "Ctrl+Z leaves the rulers off");
    key(&mut w, Key::Z, ctrl_shift());
    assert_eq!(shown(&w), (true, false, true, false), "and so does redo");
    assert!(w.state().is_dirty());
}

/// The owner's ruling, 2026-11-22: "Unsaved, not undo": the Grid box turned, then a shape moved, then Ctrl+Z: the move is undone and the grid stays off. Ctrl+Z
/// again takes the shape's drop back, and once more has nothing to undo: the grid is off all the way, and the drawing is still changed.
#[test]
fn ctrl_z_after_a_box_and_a_move_undoes_only_the_move() {
    let dir = TempDir::new("winsave-undo");
    let mut w = new_window(dir.file("x.diacraft"));
    drag_tile_to(&mut w, "Rectangle", 4.0, 4.0);
    let id = shape_ids(&w)[0];
    let dropped = frame_of(&w, id);
    run_command(&mut w, "window.tab", json!({"tab": "View"}));
    w.get_by_label("Grid").click();
    w.rest();
    assert_eq!(shown(&w), (false, true, true, false));
    run_command(&mut w, "shape.move", json!({"ids": [id], "dx": 1.0, "dy": 0.0}));
    assert!(frame_of(&w, id).0 > dropped.0 + 0.5);
    key(&mut w, Key::Z, ctrl());
    assert_eq!(frame_of(&w, id), dropped, "the move is undone");
    assert_eq!(shown(&w), (false, true, true, false), "and the grid stays off");
    key(&mut w, Key::Z, ctrl());
    assert!(shape_ids(&w).is_empty(), "the drop is undone");
    assert_eq!(shown(&w), (false, true, true, false));
    key(&mut w, Key::Z, ctrl());
    assert_eq!(shown(&w), (false, true, true, false), "past the start of the history the grid is as it was turned");
    assert!(w.state().is_dirty());
    // Redo brings the shape back, and not the grid.
    key(&mut w, Key::Z, ctrl_shift());
    key(&mut w, Key::Z, ctrl_shift());
    assert_eq!(shape_ids(&w), [id]);
    assert!(frame_of(&w, id).0 > dropped.0 + 0.5);
    assert_eq!(shown(&w), (false, true, true, false), "redo leaves the grid as it is");
}

/// A box turned is a change of the drawing and no undo step. The title carries its mark, closing the window asks first, a box turned makes no undo step, and a
/// second save of the drawing, with nothing turned since, is no edit.
#[test]
fn a_box_turned_marks_the_drawing_unsaved_and_the_close_asks() {
    let dir = TempDir::new("winsave-close");
    let target = dir.file("kept.diacraft");
    let mut w = new_window(target.clone());
    assert_eq!(w.state().window_title(), "Drawing1 - DiaCraft");
    // Clean: the close goes through.
    w.input_mut().viewports.entry(ViewportId::ROOT).or_default().events.push(ViewportEvent::Close);
    w.step();
    assert!(!w.state().prompt_open());
    assert!(!commands(&w).iter().any(|c| matches!(c, ViewportCommand::CancelClose)));
    let revision = w.state().session().revision();
    run_command(&mut w, "window.tab", json!({"tab": "View"}));
    w.get_by_label("Grid").click();
    w.rest();
    assert_eq!(shown(&w), (false, true, true, false));
    assert_eq!(w.state().session().history().unwrap().undo_len(), 0, "no undo step");
    assert_eq!(w.state().session().revision(), revision, "no revision moves: the page is not drawn again for the grid");
    assert_eq!(w.state().window_title(), "*Drawing1 - DiaCraft", "the title's mark");
    assert_eq!(w.state().window_state()["dirty"], true);
    // The close is held back and asked about.
    w.input_mut().viewports.entry(ViewportId::ROOT).or_default().events.push(ViewportEvent::Close);
    w.step();
    assert!(commands(&w).iter().any(|c| matches!(c, ViewportCommand::CancelClose)), "the close was cancelled");
    assert!(w.state().prompt_open());
    w.rest();
    // Save: asks for the file through the dialog and writes it.
    press(&mut w, "prompt.save");
    w.rest();
    assert!(target.exists() && !w.state().is_dirty());
    // A second save with nothing turned since is no edit: the same bytes.
    let first = std::fs::read(&target).unwrap();
    key(&mut w, Key::S, ctrl());
    assert_eq!(std::fs::read(&target).unwrap(), first, "an unedited second save");
    assert!(!w.state().is_dirty());
    let again = window_on(&target, None);
    assert_eq!(shown(&again), (false, true, true, false));
}

/// Don't Save on the close question throws the box turned away with the rest of the changes.
#[test]
fn a_box_turned_and_not_saved_is_given_up_when_the_person_says_so() {
    let mut w = edit_window_with([1280.0, 900.0], |app| app);
    run_command(&mut w, "view.grid", json!({"on": false}));
    w.input_mut().viewports.entry(ViewportId::ROOT).or_default().events.push(ViewportEvent::Close);
    w.step();
    assert!(w.state().prompt_open());
    w.rest();
    let at = w.state().element("prompt.dont_save").unwrap().rect.center();
    click(&mut w, at);
    assert!(commands(&w).iter().any(|c| matches!(c, ViewportCommand::Close)), "the close is given when the person said so");
}

/// An opened VSDX file saved into itself (Ctrl+S) keeps the settings turned; a program's `window.settings.set` is shown by the window as a box turned is.
#[test]
fn an_opened_vsdx_saved_in_place_keeps_the_settings_turned() {
    let dir = TempDir::new("winsave-vsdx");
    let file = fixture::write(dir.path(), "plans.vsdx", &fixture::demo_vsdx());
    let mut w = window_on(&file, None);
    assert_eq!(shown(&w), NEW, "the file's own: grid and rulers on");
    run_command(&mut w, "window.settings.set", json!({"grid": false, "connection_points": true}));
    assert_eq!(shown(&w), (false, true, true, true), "the window follows the drawing");
    key(&mut w, Key::S, ctrl());
    assert!(!w.state().is_dirty());
    let again = window_on(&file, None);
    assert_eq!(shown(&again), (false, true, true, true));
}

/// Test 3 of the brief, from the window: a VSDX file opened and saved with its settings untouched keeps its windows part byte for byte.
#[test]
fn an_opened_vsdx_saved_with_its_settings_untouched_keeps_its_windows_part() {
    let dir = TempDir::new("winsave-keep");
    let bytes = fixture::demo_vsdx();
    let file = fixture::write(dir.path(), "plans.vsdx", &bytes);
    let mut w = window_on(&file, None);
    run_command(&mut w, "page.add", json!({"name": "More"}));
    key(&mut w, Key::S, ctrl());
    let read = |b: &[u8]| {
        let pkg = diacraft_opc::Package::read(b, &diacraft_opc::Limits::default()).unwrap();
        pkg.part("/visio/windows.xml").unwrap().to_vec()
    };
    assert_eq!(read(&std::fs::read(&file).unwrap()), read(&bytes));
    assert_eq!(read(&bytes), fixture::WINDOWS_PART.as_bytes());
}
