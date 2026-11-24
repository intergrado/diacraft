//! The questions the window asks: before it replaces a file whose name it gave a file type to (M2 of the M3 review), and when the engine says
//! the drawing's file changed on the disk (M3). While a question is on the screen the drawing under it takes neither the keys nor the pointer (L6).

use std::path::PathBuf;
use std::sync::Arc;

use diacraft_ui_egui::{ExportPicker, SavePicker};
use egui::{Key, Modifiers, ViewportCommand, ViewportEvent, ViewportId};
use serde_json::json;

use crate::editing::{click_at, drag_tile_to, drag_with, edit_window_with, frame_of, key, shape_ids, three_shapes};
use crate::fixture;
use crate::saving::{ctrl, ctrl_shift};
use crate::support::{Rest, TempDir, Window, click, commands, press, run_command};
use crate::text_edit::{labelled_window, page_xml};

fn save_picker(path: PathBuf) -> SavePicker {
    Arc::new(move |_| Some(path.clone()))
}

fn export_picker(path: PathBuf) -> ExportPicker {
    Arc::new(move |_| Some(path.clone()))
}

const PRECIOUS: &[u8] = b"a file that is already here and must not be replaced without asking";
const CHANGED: &[u8] = b"changed by something else since the drawing was opened";

// ---- M2: a file type the window added names a file that is there -------------------------------------------------------------------------

/// The Save dialog returned `plan` and checked that `plan` is free; the window makes it `plan.vsdx`, which is there. Cancel leaves it; Replace
/// replaces it.
#[test]
fn save_as_asks_before_it_replaces_a_file_whose_name_it_gave_a_type_to() {
    let dir = TempDir::new("replace");
    let existing = dir.file("plan.vsdx");
    std::fs::write(&existing, PRECIOUS).unwrap();
    let (mut w, _) = labelled_window(&dir, |app| app.with_save_picker(save_picker(dir.file("plan"))));
    key(&mut w, Key::S, ctrl_shift());
    w.rest();
    assert!(w.state().replace_prompt_open(), "the person is asked");
    assert_eq!(std::fs::read(&existing).unwrap(), PRECIOUS, "and nothing is written yet");
    w.rest();
    assert!(w.state().element("replace.replace").is_some() && w.state().element("replace.cancel").is_some());
    // Cancel: still nothing written, the drawing still has its own file.
    press(&mut w, "replace.cancel");
    assert!(!w.state().replace_prompt_open());
    assert_eq!(std::fs::read(&existing).unwrap(), PRECIOUS);
    assert_eq!(w.state().window_title(), "labelled.vsdx - DiaCraft");
    // Replace: the file is the drawing now.
    key(&mut w, Key::S, ctrl_shift());
    w.rest();
    press(&mut w, "replace.replace");
    w.rest();
    assert!(!w.state().replace_prompt_open());
    assert!(page_xml(&existing).contains(fixture::LABEL_TEXT), "{}", page_xml(&existing));
    assert_eq!(w.state().window_title(), "plan.vsdx - DiaCraft");
}

/// A name that had its type already was checked by the dialog, and a name nothing is at has nothing to replace: neither is asked about.
#[test]
fn a_name_with_its_type_or_with_no_file_there_is_not_asked_about() {
    let dir = TempDir::new("replace-not");
    let existing = dir.file("plan.vsdx");
    std::fs::write(&existing, PRECIOUS).unwrap();
    // With the type: the dialog asked, the window does not again.
    let (mut w, _) = labelled_window(&dir, |app| app.with_save_picker(save_picker(existing.clone())));
    key(&mut w, Key::S, ctrl_shift());
    w.rest();
    assert!(!w.state().replace_prompt_open());
    assert!(page_xml(&existing).contains(fixture::LABEL_TEXT));
    // No type, and no `fresh.vsdx`: written at once.
    let (mut w, _) = labelled_window(&dir, |app| app.with_save_picker(save_picker(dir.file("fresh"))));
    key(&mut w, Key::S, ctrl_shift());
    w.rest();
    assert!(!w.state().replace_prompt_open());
    assert!(dir.file("fresh.vsdx").exists());
}

/// Export does the same with `.png`, and its Cancel leaves the file; its Replace writes the picture.
#[test]
fn export_asks_before_it_replaces_a_file_whose_name_it_gave_a_type_to() {
    let dir = TempDir::new("replace-export");
    let existing = dir.file("pic.png");
    std::fs::write(&existing, PRECIOUS).unwrap();
    let (mut w, _) = labelled_window(&dir, |app| app.with_export_picker(export_picker(dir.file("pic"))));
    run_command(&mut w, "window.backstage", json!({"page": "Home"}));
    press(&mut w, "backstage.Export");
    w.rest();
    assert!(w.state().replace_prompt_open(), "the person is asked");
    assert_eq!(std::fs::read(&existing).unwrap(), PRECIOUS);
    w.rest();
    press(&mut w, "replace.cancel");
    assert!(!w.state().replace_prompt_open() && !w.state().exporting_now());
    assert_eq!(std::fs::read(&existing).unwrap(), PRECIOUS, "Cancel writes nothing");
    press(&mut w, "backstage.Export");
    w.rest();
    press(&mut w, "replace.replace");
    w.rest();
    assert!(std::fs::read(&existing).unwrap().starts_with(b"\x89PNG"), "Replace writes the picture");
    // With its type, or with nothing there: no question.
    let (mut w, _) = labelled_window(&dir, |app| app.with_export_picker(export_picker(existing.clone())));
    run_command(&mut w, "window.backstage", json!({"page": "Home"}));
    press(&mut w, "backstage.Export");
    w.rest();
    assert!(!w.state().replace_prompt_open());
    let (mut w, _) = labelled_window(&dir, |app| app.with_export_picker(export_picker(dir.file("new"))));
    run_command(&mut w, "window.backstage", json!({"page": "Home"}));
    press(&mut w, "backstage.Export");
    w.rest();
    assert!(!w.state().replace_prompt_open() && dir.file("new.png").exists());
}

// ---- M3: the drawing's file changed on the disk -------------------------------------------------------------------------------------------

/// Save finds the file changed since the drawing was opened: the person is asked, not told it failed; Cancel keeps the other change and the
/// drawing as it is; Overwrite runs the save again with `force`.
#[test]
fn save_asks_when_the_file_changed_on_the_disk_and_overwrites_only_when_told_to() {
    let dir = TempDir::new("changed");
    let (mut w, path) = labelled_window(&dir, |app| app);
    run_command(&mut w, "shape.move", json!({"ids": [1], "dx": 1.0, "dy": 0.0}));
    std::fs::write(&path, CHANGED).unwrap();
    key(&mut w, Key::S, ctrl());
    w.rest();
    assert!(w.state().changed_prompt_open(), "the person is asked");
    assert!(w.state().open_error().is_none(), "it is a question, not a failure");
    assert_eq!(std::fs::read(&path).unwrap(), CHANGED, "nothing is written yet");
    w.rest();
    for name in ["changed.overwrite", "changed.save_as", "changed.cancel"] {
        assert!(w.state().element(name).is_some(), "{name}");
    }
    // Cancel: the other change stays, the drawing is still unsaved.
    press(&mut w, "changed.cancel");
    assert!(!w.state().changed_prompt_open() && w.state().is_dirty());
    assert_eq!(std::fs::read(&path).unwrap(), CHANGED);
    // Overwrite: the drawing is the file.
    key(&mut w, Key::S, ctrl());
    w.rest();
    press(&mut w, "changed.overwrite");
    w.rest();
    assert!(!w.state().changed_prompt_open() && !w.state().is_dirty());
    assert!(page_xml(&path).contains(fixture::LABEL_TEXT), "the file is the drawing again");
    assert!(w.state().notice().is_some_and(|n| n.starts_with("Saved labelled.vsdx")), "{:?}", w.state().notice());
}

/// Save As from the question saves the drawing as another file and leaves the file that changed as it is.
#[test]
fn save_as_from_the_question_saves_elsewhere_and_leaves_the_changed_file() {
    let dir = TempDir::new("changed-as");
    let elsewhere = dir.file("elsewhere.vsdx");
    let (mut w, path) = labelled_window(&dir, |app| app.with_save_picker(save_picker(elsewhere.clone())));
    run_command(&mut w, "shape.move", json!({"ids": [1], "dx": 1.0, "dy": 0.0}));
    std::fs::write(&path, CHANGED).unwrap();
    key(&mut w, Key::S, ctrl());
    w.rest();
    press(&mut w, "changed.save_as");
    w.rest();
    assert!(!w.state().changed_prompt_open());
    assert!(elsewhere.exists(), "saved as the file the dialog gave");
    assert_eq!(std::fs::read(&path).unwrap(), CHANGED, "and the file that changed is as it was");
    assert_eq!(w.state().window_title(), "elsewhere.vsdx - DiaCraft");
    assert!(!w.state().is_dirty());
}

/// Save As onto the drawing's own file that changed is asked about too, and Overwrite names the same file again with `force`.
#[test]
fn save_as_onto_its_own_changed_file_is_asked_about() {
    let dir = TempDir::new("changed-own");
    // The app's dialog answers the file the drawing came from.
    let (mut w, path) = labelled_window(&dir, |app| app.with_save_picker(save_picker(dir.file("labelled.vsdx"))));
    assert_eq!(path, dir.file("labelled.vsdx"));
    run_command(&mut w, "shape.move", json!({"ids": [1], "dx": 1.0, "dy": 0.0}));
    std::fs::write(&path, CHANGED).unwrap();
    key(&mut w, Key::S, ctrl_shift());
    w.rest();
    assert!(w.state().changed_prompt_open());
    assert_eq!(std::fs::read(&path).unwrap(), CHANGED);
    w.rest();
    press(&mut w, "changed.overwrite");
    w.rest();
    assert!(page_xml(&path).contains(fixture::LABEL_TEXT));
    assert!(!w.state().is_dirty());
}

/// A close that saves goes through the question: the window closes after the overwrite, and not before it.
#[test]
fn a_close_that_saves_waits_for_the_answer_about_a_changed_file() {
    let dir = TempDir::new("changed-close");
    let (mut w, path) = labelled_window(&dir, |app| app);
    run_command(&mut w, "shape.move", json!({"ids": [1], "dx": 1.0, "dy": 0.0}));
    std::fs::write(&path, CHANGED).unwrap();
    w.input_mut().viewports.entry(ViewportId::ROOT).or_default().events.push(ViewportEvent::Close);
    w.step();
    assert!(w.state().prompt_open());
    w.rest();
    press(&mut w, "prompt.save");
    w.rest();
    assert!(w.state().changed_prompt_open(), "the save met the changed file");
    assert!(!commands(&w).iter().any(|c| matches!(c, ViewportCommand::Close)), "and the window has not closed");
    w.rest();
    let overwrite = w.state().element("changed.overwrite").unwrap().rect.center();
    click(&mut w, overwrite);
    assert!(commands(&w).iter().any(|c| matches!(c, ViewportCommand::Close)), "the window closes once the file is saved");
    assert!(page_xml(&path).contains(fixture::LABEL_TEXT));
}

/// A cancelled question ends what was waiting on the save: a New that was to follow does not happen.
#[test]
fn cancelling_the_question_cancels_the_new_that_waited_for_the_save() {
    let dir = TempDir::new("changed-new");
    let (mut w, path) = labelled_window(&dir, |app| app);
    run_command(&mut w, "shape.move", json!({"ids": [1], "dx": 1.0, "dy": 0.0}));
    std::fs::write(&path, CHANGED).unwrap();
    key(&mut w, Key::N, ctrl());
    press(&mut w, "prompt.save");
    w.rest();
    press(&mut w, "changed.cancel");
    assert!(!w.state().changed_prompt_open() && !w.state().prompt_open());
    assert_eq!(w.state().window_title(), "*labelled.vsdx - DiaCraft", "still the drawing, still unsaved");
    assert_eq!(std::fs::read(&path).unwrap(), CHANGED);
}

// ---- the drawing under a save question is replaced ---------------------------------------------------------------------

/// A program that makes a new drawing under the question about a changed file takes the question away: Overwrite would have written the new drawing over
/// the old one's file.
#[test]
fn a_new_drawing_made_by_a_program_closes_the_question_about_a_changed_file() {
    let dir = TempDir::new("changed-replaced");
    let (mut w, path) = labelled_window(&dir, |app| app);
    run_command(&mut w, "shape.move", json!({"ids": [1], "dx": 1.0, "dy": 0.0}));
    std::fs::write(&path, CHANGED).unwrap();
    key(&mut w, Key::S, ctrl());
    w.rest();
    assert!(w.state().changed_prompt_open(), "the save met the changed file");
    run_command(&mut w, "file.new", json!({}));
    assert!(!w.state().changed_prompt_open() && !w.state().is_modal_open(), "the question is about a drawing that is gone");
    assert!(w.state().element("changed.overwrite").is_none(), "and has no buttons");
    assert_eq!(std::fs::read(&path).unwrap(), CHANGED, "nothing was written");
}

/// The same for the question about replacing a file that is there: Replace would have written the new drawing under the name the old one was saved as.
#[test]
fn a_new_drawing_made_by_a_program_closes_the_question_about_replacing_a_file() {
    let dir = TempDir::new("replace-replaced");
    let existing = dir.file("plan.vsdx");
    std::fs::write(&existing, PRECIOUS).unwrap();
    let (mut w, _) = labelled_window(&dir, |app| app.with_save_picker(save_picker(dir.file("plan"))));
    key(&mut w, Key::S, ctrl_shift());
    w.rest();
    assert!(w.state().replace_prompt_open());
    run_command(&mut w, "file.new", json!({}));
    assert!(!w.state().replace_prompt_open() && !w.state().is_modal_open());
    assert!(w.state().element("replace.replace").is_none());
    assert_eq!(std::fs::read(&existing).unwrap(), PRECIOUS, "nothing was written");
}

/// The question about replacing a picture file (Export) is about the file and not about a save of the drawing: a program that replaces the drawing leaves it, and
/// Cancel still ends it.
#[test]
fn the_question_about_replacing_a_picture_is_not_closed_by_a_new_drawing() {
    let dir = TempDir::new("export-replaced");
    let existing = dir.file("pic.png");
    std::fs::write(&existing, PRECIOUS).unwrap();
    let (mut w, _) = labelled_window(&dir, |app| app.with_export_picker(export_picker(dir.file("pic"))));
    run_command(&mut w, "window.backstage", json!({"page": "Home"}));
    press(&mut w, "backstage.Export");
    w.rest();
    assert!(w.state().replace_prompt_open(), "the person is asked");
    run_command(&mut w, "file.new", json!({}));
    assert!(w.state().replace_prompt_open(), "a question about a picture file stays");
    w.rest();
    press(&mut w, "replace.cancel");
    assert!(!w.state().replace_prompt_open());
    assert_eq!(std::fs::read(&existing).unwrap(), PRECIOUS, "Cancel writes nothing");
}

// ---- L6: under a question, or the File view, the drawing takes nothing ----------------------------------------------------------------------

/// Delete, Undo and Select All under the File view change nothing: the drawing is not the one the person is looking at.
#[test]
fn keys_under_the_file_view_do_not_reach_the_drawing() {
    let mut w = edit_window_with([1280.0, 900.0], |app| app);
    let ids = three_shapes(&mut w);
    click_at(&mut w, 3.0, 6.0);
    assert_eq!(w.state().selection(), &ids[..1]);
    let revision = w.state().session().revision();
    run_command(&mut w, "window.backstage", json!({"page": "Home"}));
    key(&mut w, Key::Delete, Modifiers::NONE);
    key(&mut w, Key::Z, ctrl());
    key(&mut w, Key::A, ctrl());
    key(&mut w, Key::Num3, ctrl());
    assert_eq!(w.state().session().revision(), revision, "nothing was deleted or undone");
    assert_eq!(shape_ids(&w), ids);
    run_command(&mut w, "window.backstage", json!({"page": "close"}));
    assert_eq!(w.state().selection(), &ids[..1], "and the selection is as it was");
    assert_eq!(w.state().tool().name(), "pointer");
    // The commands on files are still the File view's keys: New works from it.
    run_command(&mut w, "window.backstage", json!({"page": "Home"}));
    key(&mut w, Key::N, ctrl());
    assert!(w.state().prompt_open(), "Ctrl+N asks from the File view as it does from the drawing");
}

/// With the question about unsaved changes on the screen, a drag on the drawing, a click on a shape, Delete, the toolbar, a tile of the Shapes
/// window and the ribbon do nothing; they work again once it is answered.
#[test]
fn a_question_takes_the_keys_and_the_pointer_from_everything_under_it() {
    let mut w = edit_window_with([1280.0, 900.0], |app| app);
    let ids = three_shapes(&mut w);
    key(&mut w, Key::N, ctrl());
    assert!(w.state().prompt_open());
    w.rest();
    let revision = w.state().session().revision();
    let selection = w.state().selection().to_vec();
    // The ellipse (and the diamond beside the card) are clear of the card in the middle of the window.
    let card = w.state().element("prompt.dont_save").unwrap().rect;
    let ellipse = crate::editing::at(&w, 5.0, 3.0);
    assert!(ellipse.y > card.bottom() + 40.0, "the ellipse {ellipse:?} is under the card {card:?}");
    let centre = |w: &Window, id| {
        let (x, y, width, height) = frame_of(w, id);
        (x + width / 2.0, y + height / 2.0)
    };
    let ellipse_centre = centre(&w, ids[2]);
    drag_with(&mut w, (5.0, 3.0), (5.0, 2.0), Modifiers::NONE);
    click_at(&mut w, 7.0, 6.0);
    key(&mut w, Key::Delete, Modifiers::NONE);
    key(&mut w, Key::Z, ctrl());
    key(&mut w, Key::A, ctrl());
    let undo = w.state().element("qat.Undo").unwrap().rect.center();
    click(&mut w, undo);
    let tile = w.state().element("shapes.tile.Circle").unwrap().rect.center();
    click(&mut w, tile);
    let connector = w.state().element("ribbon.Home.ConnectorTool").unwrap().rect.center();
    click(&mut w, connector);
    w.rest();
    assert!(w.state().prompt_open(), "still waiting for the answer");
    assert_eq!(w.state().session().revision(), revision, "nothing under the card changed the drawing");
    assert_eq!(shape_ids(&w), ids);
    assert_eq!(w.state().selection(), selection.as_slice());
    assert_eq!(w.state().tool().name(), "pointer");
    assert_eq!(centre(&w, ids[2]), ellipse_centre, "the shape did not move");
    // Answered, the drawing works again.
    press(&mut w, "prompt.cancel");
    assert!(!w.state().prompt_open());
    drag_with(&mut w, (5.0, 3.0), (5.0, 2.0), Modifiers::NONE);
    assert!(w.state().session().revision() > revision, "a drag moves the shape now");
    drag_tile_to(&mut w, "Circle", 9.0, 2.0);
    assert_eq!(shape_ids(&w).len(), 4);
}

/// The cards keep every button on the screen and apart from each other at a narrow window and at a wide one.
#[test]
fn the_cards_fit_a_narrow_window_and_a_wide_one() {
    let dir = TempDir::new("cards-fit");
    let existing = dir.file("plan.vsdx");
    std::fs::write(&existing, PRECIOUS).unwrap();
    let (mut w, path) = labelled_window(&dir, |app| app.with_save_picker(save_picker(dir.file("plan"))));
    run_command(&mut w, "shape.move", json!({"ids": [1], "dx": 1.0, "dy": 0.0}));
    // Three cards, one after the other: the replace question, the changed-on-disk question and the unsaved-changes question.
    key(&mut w, Key::S, ctrl_shift());
    w.rest();
    assert!(w.state().replace_prompt_open());
    let check = |w: &mut Window, names: &[&str]| {
        for size in [[300.0, 420.0], [480.0, 520.0], [1280.0, 900.0]] {
            w.set_size(egui::vec2(size[0], size[1]));
            w.rest();
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(size[0], size[1]));
            let rects: Vec<(String, egui::Rect)> =
                names.iter().map(|n| (n.to_string(), w.state().element(n).unwrap_or_else(|| panic!("{n} at {size:?}")).rect)).collect();
            for (name, rect) in &rects {
                assert!(screen.contains_rect(*rect), "{name} {rect:?} is off the screen at {size:?}");
            }
            for (i, (a, ra)) in rects.iter().enumerate() {
                for (b, rb) in rects.iter().skip(i + 1) {
                    assert!(!ra.intersects(*rb), "{a} lies over {b} at {size:?}: {ra:?} {rb:?}");
                }
            }
        }
    };
    check(&mut w, &["replace.replace", "replace.cancel"]);
    press(&mut w, "replace.cancel");
    std::fs::write(&path, CHANGED).unwrap();
    key(&mut w, Key::S, ctrl());
    w.rest();
    assert!(w.state().changed_prompt_open());
    check(&mut w, &["changed.overwrite", "changed.save_as", "changed.cancel"]);
    press(&mut w, "changed.cancel");
    key(&mut w, Key::N, ctrl());
    assert!(w.state().prompt_open());
    w.rest();
    check(&mut w, &["prompt.save", "prompt.dont_save", "prompt.cancel"]);
}
