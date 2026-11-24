//! Which extends the owner's of 2026-11-22, "Unsaved, not undo": the boxes of View > Visual Aids that are the document's (AutoConnect,
//! Dynamic Grid, the Snap box and the Snap & Glue dialog, and the status bar's Snap mark) mark the drawing unsaved and are no undo step, so Ctrl+Z goes on undoing the drawing's
//! edits and leaves the boxes as they were turned. The window follows the engine: a box turned shows at once, and a drawing saved and opened again shows what it held.
//!
//! The Save dialog is stood in for by a picker that answers a path of the test's, and a second window opens the file with Ctrl+O, as a person does.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui::accesskit::Toggled;
use egui::{Key, ViewportEvent, ViewportId};
use egui_kittest::kittest::{NodeT, Queryable};
use serde_json::{Value, json};

use crate::editing::{drag_tile_to, edit_window_with, frame_of, key, key_without_waiting, shape_ids};
use crate::saving::{ctrl, ctrl_shift};
use crate::support::{Rest, TempDir, Window, press, run_command};
use diacraft_ui_egui::{FilePicker, SavePicker};

fn save_picker(answer: PathBuf) -> SavePicker {
    Arc::new(move |_| Some(answer.clone()))
}

/// A window on a new drawing whose Save dialog answers `answer`.
fn new_window(answer: PathBuf) -> Window {
    edit_window_with([1600.0, 900.0], |app| app.with_save_picker(save_picker(answer)))
}

/// A window whose Open dialog answers `file`, with `file` opened by Ctrl+O.
fn window_on(file: &Path) -> Window {
    let path = file.to_path_buf();
    let open: FilePicker = Arc::new(move || Some(path.clone()));
    let mut w = edit_window_with([1600.0, 900.0], |app| app.with_file_picker(open));
    key_without_waiting(&mut w, Key::O, ctrl());
    w.rest();
    assert!(w.state().open_error().is_none(), "{} did not open: {:?}", file.display(), w.state().open_error());
    w
}

fn toggled(w: &Window, label: &str) -> Option<Toggled> {
    w.get_by_label(label).accesskit_node().toggled()
}

fn undo_steps(w: &Window) -> usize {
    w.state().session().history().unwrap().undo_len()
}

/// The document's Snap & Glue settings, as `snap.settings` answers them.
fn snap(w: &mut Window) -> Value {
    w.state_mut().execute("snap.settings", &json!({})).unwrap()
}

fn autoconnect_on(w: &mut Window) -> bool {
    w.state_mut().execute("autoconnect.settings", &json!({})).unwrap()["on"].as_bool().unwrap()
}

/// The boxes under test: how a person turns each one, and what the window then shows.
struct BoxUnderTest {
    name: &'static str,
    /// Turns it the way a person does.
    turn: fn(&mut Window),
    /// Whether it is turned now, by what the document holds.
    turned: fn(&mut Window) -> bool,
    /// What the window's control for it says.
    shows: fn(&Window) -> Option<Toggled>,
}

fn boxes() -> Vec<BoxUnderTest> {
    vec![
        BoxUnderTest {
            name: "AutoConnect",
            turn: |w| {
                run_command(w, "window.tab", json!({"tab": "View"}));
                w.get_by_label("AutoConnect").click();
                w.rest();
            },
            turned: |w| !autoconnect_on(w),
            shows: |w| toggled(w, "AutoConnect"),
        },
        BoxUnderTest {
            name: "Dynamic Grid",
            turn: |w| {
                run_command(w, "window.tab", json!({"tab": "View"}));
                press(w, "ribbon.View.DynamicGrid");
                w.rest();
            },
            turned: |w| snap(w)["dynamic_grid"] == json!(false),
            shows: |w| toggled(w, "Dynamic Grid"),
        },
        BoxUnderTest {
            name: "the Snap box",
            turn: |w| {
                run_command(w, "window.tab", json!({"tab": "View"}));
                press(w, "ribbon.View.SnapToggle");
                w.rest();
            },
            turned: |w| snap(w)["snap"] == json!(false),
            shows: |w| toggled(w, "Snap"),
        },
        BoxUnderTest {
            name: "the status bar's Snap mark",
            turn: |w| {
                press(w, "status.snap");
                w.rest();
            },
            turned: |w| snap(w)["snap"] == json!(false),
            shows: |w| {
                // The mark's name is its state's: "Snap on" or "Snap off".
                ["Snap on", "Snap off"].into_iter().find_map(|label| w.query_by_label(label)).and_then(|node| node.accesskit_node().toggled())
            },
        },
        BoxUnderTest {
            name: "the Snap & Glue dialog",
            turn: |w| {
                run_command(w, "window.snapGlue", json!({"tab": "General"}));
                press(w, "snapglue.option.glue_vertices");
                press(w, "snapglue.ok");
                w.rest();
            },
            turned: |w| snap(w)["glue_vertices"] == json!(true),
            shows: |_| Some(Toggled::False),
        },
    ]
}

/// The box turned marks the drawing unsaved (the title's star, `is_dirty`) and makes no undo step; the window shows it at once, by what the document now holds (a window that kept the
/// settings it read for the old revision would show the old one).
#[test]
fn a_box_turned_is_unsaved_and_no_undo_step_and_the_window_shows_it_at_once() {
    for b in boxes() {
        let dir = TempDir::new("viewundo-box");
        let mut w = new_window(dir.file("x.diacraft"));
        assert_eq!(w.state().window_title(), "Drawing1 - DiaCraft", "{}", b.name);
        run_command(&mut w, "window.tab", json!({"tab": "View"}));
        let shown = (b.shows)(&w);
        (b.turn)(&mut w);
        assert!((b.turned)(&mut w), "{}: the box is turned in the document", b.name);
        assert_eq!(undo_steps(&w), 0, "{}: no undo step", b.name);
        assert_eq!(w.state().window_title(), "*Drawing1 - DiaCraft", "{}: the title's mark", b.name);
        assert!(w.state().is_dirty(), "{}", b.name);
        if b.name != "the Snap & Glue dialog" {
            assert_ne!((b.shows)(&w), shown, "{}: the control shows what it was turned to", b.name);
        }
        // Ctrl+Z has nothing to undo and leaves the box where it was turned.
        key(&mut w, Key::Z, ctrl());
        assert!((b.turned)(&mut w), "{}: Ctrl+Z leaves the box", b.name);
        key(&mut w, Key::Z, ctrl_shift());
        assert!((b.turned)(&mut w), "{}: and so does redo", b.name);
        assert!(w.state().is_dirty(), "{}", b.name);
    }
}

/// The box turned, then a shape moved, then Ctrl+Z: the move is undone and the box stays; Ctrl+Z again takes the drop back, and the box still stays; redo brings the shape back and
/// leaves the box.
#[test]
fn ctrl_z_after_a_box_and_a_move_undoes_only_the_move() {
    for b in boxes() {
        let dir = TempDir::new("viewundo-undo");
        let mut w = new_window(dir.file("x.diacraft"));
        drag_tile_to(&mut w, "Rectangle", 4.0, 4.0);
        let id = shape_ids(&w)[0];
        let dropped = frame_of(&w, id);
        (b.turn)(&mut w);
        assert!((b.turned)(&mut w), "{}", b.name);
        run_command(&mut w, "shape.move", json!({"ids": [id], "dx": 1.0, "dy": 0.0}));
        assert!(frame_of(&w, id).0 > dropped.0 + 0.5);
        key(&mut w, Key::Z, ctrl());
        assert_eq!(frame_of(&w, id), dropped, "{}: the move is undone", b.name);
        assert!((b.turned)(&mut w), "{}: and the box stays", b.name);
        key(&mut w, Key::Z, ctrl());
        assert!(shape_ids(&w).is_empty(), "{}: the drop is undone", b.name);
        assert!((b.turned)(&mut w), "{}", b.name);
        key(&mut w, Key::Z, ctrl());
        assert!((b.turned)(&mut w), "{}: past the start of the history the box is as it was turned", b.name);
        assert!(w.state().is_dirty(), "{}", b.name);
        key(&mut w, Key::Z, ctrl_shift());
        key(&mut w, Key::Z, ctrl_shift());
        assert_eq!(shape_ids(&w), [id], "{}", b.name);
        assert!((b.turned)(&mut w), "{}: redo leaves the box as it is", b.name);
    }
}

/// The close question: a drawing whose box was turned asks first, and Save keeps the box in `.diacraft` and in VSDX, which a second window opens and shows as it was turned.
#[test]
fn a_box_turned_asks_on_close_and_a_save_and_a_reopen_keeps_it() {
    for b in boxes() {
        for name in ["kept.diacraft", "kept.vsdx"] {
            let dir = TempDir::new("viewundo-save");
            let target = dir.file(name);
            let mut w = new_window(target.clone());
            (b.turn)(&mut w);
            w.input_mut().viewports.entry(ViewportId::ROOT).or_default().events.push(ViewportEvent::Close);
            w.step();
            assert!(w.state().prompt_open(), "{} ({name}): closing asks about the box turned", b.name);
            w.rest();
            press(&mut w, "prompt.save");
            w.rest();
            assert!(target.exists() && !w.state().is_dirty(), "{} ({name})", b.name);
            let first = std::fs::read(&target).unwrap();
            key(&mut w, Key::S, ctrl());
            assert_eq!(std::fs::read(&target).unwrap(), first, "{} ({name}): an unedited second save", b.name);
            let mut again = window_on(&target);
            assert!((b.turned)(&mut again), "{} ({name}): the box is as it was turned when the file is opened again", b.name);
            assert!(!again.state().is_dirty(), "{} ({name}): a drawing opened is clean", b.name);
        }
    }
}

/// The AutoConnect box greys out when the program-wide setting is off and follows the drawing's own setting: the box and `autoconnect.set` agree, and no step is made by either.
#[test]
fn the_autoconnect_box_follows_the_drawings_setting_and_no_undo() {
    let dir = TempDir::new("viewundo-ac");
    let mut w = new_window(dir.file("x.diacraft"));
    run_command(&mut w, "window.tab", json!({"tab": "View"}));
    assert_eq!(toggled(&w, "AutoConnect"), Some(Toggled::True));
    w.get_by_label("AutoConnect").click();
    w.rest();
    assert_eq!(toggled(&w, "AutoConnect"), Some(Toggled::False));
    w.get_by_label("AutoConnect").click();
    w.rest();
    assert_eq!(toggled(&w, "AutoConnect"), Some(Toggled::True));
    // A program's setting shows in the box, and neither is an undo step.
    run_command(&mut w, "autoconnect.set", json!({"on": false}));
    assert_eq!(toggled(&w, "AutoConnect"), Some(Toggled::False), "a program's change shows");
    assert_eq!(undo_steps(&w), 0);
}
