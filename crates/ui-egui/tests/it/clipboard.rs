//! Copy, Cut, Paste, Duplicate, Group and Ungroup in the window: the Clipboard group and the Group menu follow the selection and the clipboard,
//! the keys, the events a desktop sends for Ctrl+C, X and V, the copy-drag, the context menu, and the cut-and-undo and paste-and-undo cycles. The pointer and
//! the keyboard are given to egui as a person's would be, and what is checked is the document the commands changed.

use diacraft_engine::draw::shape_frame;
use diacraft_ui_egui::clipboard::labels_of;
use egui::{Event, Key, Modifiers, PointerButton, Pos2};
use serde_json::json;

use crate::editing::{click_at, click_with, drag_with, edit_window, edit_window_with, frame_of, hold, key, shape_ids, three_shapes, type_text};
use crate::support::{Rest, Window, button, click, move_to, press, run_command};

const CTRL: Modifiers = Modifiers::COMMAND;
const CTRL_SHIFT: Modifiers = Modifiers { alt: false, ctrl: false, shift: true, mac_cmd: false, command: true };

fn enabled(w: &Window, name: &str) -> bool {
    w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).enabled
}

/// A click on the control called `name`, and the window comes to rest.
fn hit(w: &mut Window, name: &str) {
    let at = w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).rect.center();
    click(w, at);
    w.rest();
}

fn page_id(w: &Window) -> u32 {
    w.state().session().document().unwrap().pages.get(0).unwrap().id
}

fn centre_of(w: &Window, id: u32) -> (f64, f64) {
    let (x, y, width, height) = frame_of(w, id);
    (x + width / 2.0, y + height / 2.0)
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

fn undo(w: &mut Window) {
    key(w, Key::Z, CTRL);
}

/// A right click at `pos`: the pointer comes, the button goes down and up.
fn right_click(w: &mut Window, pos: Pos2) {
    for _ in 0..3 {
        move_to(w, pos);
    }
    for pressed in [true, false] {
        w.input_mut().events.push(Event::PointerButton { pos, button: PointerButton::Secondary, pressed, modifiers: Modifiers::NONE });
        w.step();
    }
    w.rest();
}

/// The texts egui was asked to put on the system clipboard in the last frame.
fn system_copies(w: &Window) -> Vec<String> {
    w.output()
        .platform_output
        .commands
        .iter()
        .filter_map(|c| match c {
            egui::OutputCommand::CopyText(text) => Some(text.clone()),
            _ => None,
        })
        .collect()
}

/// The three shapes of [`three_shapes`], a rectangle at (3, 6), a diamond at (7, 6) and an ellipse at (5, 3), with the rectangle selected.
fn three() -> (Window, Vec<u32>) {
    let mut w = edit_window([1280.0, 900.0]);
    let ids = three_shapes(&mut w);
    click_at(&mut w, 3.0, 6.0);
    assert_eq!(w.state().selection(), &ids[..1]);
    (w, ids)
}

// ---- the controls ----------------------------------------------------------------------------------------------------------------------------

/// Cut and Copy follow the selection and Paste the clipboard; Paste's menu holds what applies; Format Painter follows the selection.
#[test]
fn the_clipboard_group_follows_the_selection_and_the_clipboard() {
    let mut w = edit_window([1280.0, 900.0]);
    for name in ["Cut", "Copy", "PasteMenu", "FormatPainter"] {
        assert!(!enabled(&w, &format!("ribbon.Home.{name}")), "{name} has nothing to act on");
    }
    let ids = three_shapes(&mut w);
    assert!(enabled(&w, "ribbon.Home.Cut") && enabled(&w, "ribbon.Home.Copy"), "a shape is selected");
    assert!(!enabled(&w, "ribbon.Home.PasteMenu.main"), "the clipboard is empty: Paste does nothing");
    assert!(enabled(&w, "ribbon.Home.PasteMenu.menu"), "but its menu holds Paste Duplicate, which needs only a selection");
    assert!(enabled(&w, "ribbon.Home.FormatPainter"), "Format Painter has a shape to copy the formatting of");
    let revision = w.state().session().revision();
    press(&mut w, "ribbon.Home.Copy");
    w.rest();
    assert!(enabled(&w, "ribbon.Home.PasteMenu.main"), "after a copy Paste can run");
    assert!(w.state().session().clipboard().is_some_and(|c| c.len() == 1));
    assert_eq!(w.state().session().revision(), revision, "a copy changes nothing in the drawing");
    assert_eq!(shape_ids(&w), ids);
    // Escape: nothing is selected, and Cut and Copy are quiet again; Paste is not.
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert!(!enabled(&w, "ribbon.Home.Cut") && !enabled(&w, "ribbon.Home.Copy"));
    assert!(enabled(&w, "ribbon.Home.PasteMenu.main"));
}

/// Copy and Paste from the ribbon: the copy lands a step right and down, is selected, and one undo takes it away; a second paste is a
/// step on.
#[test]
fn copy_and_paste_from_the_ribbon_select_the_copy_and_undo_takes_it_back() {
    let (mut w, ids) = three();
    press(&mut w, "ribbon.Home.Copy");
    w.rest();
    press(&mut w, "ribbon.Home.PasteMenu.main");
    w.rest();
    let now = shape_ids(&w);
    assert_eq!(now.len(), 4, "one shape was added");
    let copy = *now.last().unwrap();
    assert!(!ids.contains(&copy));
    assert_eq!(w.state().selection(), [copy], "the new shape is selected");
    let (x, y) = centre_of(&w, copy);
    assert!(near(x, 3.25) && near(y, 5.75), "a quarter inch right and down of the original at (3, 6): ({x}, {y})");
    assert_eq!(centre_of(&w, ids[0]), (3.0, 6.0), "the original did not move");
    // The next paste is one step more.
    press(&mut w, "ribbon.Home.PasteMenu.main");
    w.rest();
    let second = *shape_ids(&w).last().unwrap();
    let (x, y) = centre_of(&w, second);
    assert!(near(x, 3.5) && near(y, 5.5), "({x}, {y})");
    assert_eq!(w.state().selection(), [second]);
    // One undo for each paste.
    undo(&mut w);
    assert_eq!(shape_ids(&w).len(), 4);
    undo(&mut w);
    assert_eq!(shape_ids(&w), ids, "both pastes are undone");
    assert!(w.state().selection().is_empty(), "and what was selected is gone");
    assert!(w.state().session().clipboard().is_some(), "an undo leaves the clipboard");
}

/// Cut from the ribbon: the shape goes, nothing is selected, one undo brings it back, and Paste puts it again.
#[test]
fn cut_from_the_ribbon_removes_the_shape_and_undo_and_paste_bring_it_back() {
    let (mut w, ids) = three();
    press(&mut w, "ribbon.Home.Cut");
    w.rest();
    assert_eq!(shape_ids(&w), ids[1..], "the rectangle was cut");
    assert!(w.state().selection().is_empty());
    assert!(w.state().session().clipboard().is_some());
    // Paste after a cut: the shapes land a step from where they were.
    press(&mut w, "ribbon.Home.PasteMenu.main");
    w.rest();
    assert_eq!(shape_ids(&w).len(), 3);
    let pasted = *shape_ids(&w).last().unwrap();
    assert!(near(centre_of(&w, pasted).0, 3.25) && near(centre_of(&w, pasted).1, 5.75), "{:?}", centre_of(&w, pasted));
    undo(&mut w);
    assert_eq!(shape_ids(&w), ids[1..], "the paste is undone");
    undo(&mut w);
    assert_eq!(shape_ids(&w), ids, "and the cut: the rectangle is back, where it was");
    assert_eq!(centre_of(&w, ids[0]), (3.0, 6.0));
    assert!(w.state().session().clipboard().is_some_and(|c| !c.is_empty()), "the clipboard is kept through undo");
}

/// Paste Duplicate, in the Paste menu, duplicates the selection with no clipboard: the copy is selected and a quarter inch right and down.
#[test]
fn paste_duplicate_in_the_menu_duplicates_the_selection() {
    let (mut w, ids) = three();
    hit(&mut w, "ribbon.Home.PasteMenu.menu");
    assert!(!enabled(&w, "menu.Paste"), "the clipboard is empty");
    assert!(enabled(&w, "menu.PasteDuplicate"));
    assert!(!enabled(&w, "menu.PasteSpecialDialog") && !enabled(&w, "menu.PasteAsHyperlink"), "those wait for their commands");
    hit(&mut w, "menu.PasteDuplicate");
    assert_eq!(shape_ids(&w).len(), 4);
    let copy = *shape_ids(&w).last().unwrap();
    assert_eq!(w.state().selection(), [copy]);
    assert!(near(centre_of(&w, copy).0, 3.25) && near(centre_of(&w, copy).1, 5.75));
    assert!(w.state().session().clipboard().is_none(), "the clipboard is not used");
    undo(&mut w);
    assert_eq!(shape_ids(&w), ids);
}

// ---- the keys --------------------------------------------------------------------------------------------------------------------------------

/// Ctrl+C, Ctrl+V, Ctrl+X, Ctrl+D as keys (the control channel and the tests give keys): each does what its control does.
#[test]
fn the_clipboard_keys_do_what_the_controls_do() {
    let (mut w, ids) = three();
    key(&mut w, Key::C, CTRL);
    assert!(w.state().session().clipboard().is_some(), "Ctrl+C");
    key(&mut w, Key::V, CTRL);
    assert_eq!(shape_ids(&w).len(), 4, "Ctrl+V");
    let pasted = *shape_ids(&w).last().unwrap();
    assert_eq!(w.state().selection(), [pasted]);
    // Ctrl+D duplicates what is selected, and the copy is selected, so the next Ctrl+D steps on.
    key(&mut w, Key::D, CTRL);
    let duplicate = *shape_ids(&w).last().unwrap();
    assert_eq!(shape_ids(&w).len(), 5, "Ctrl+D");
    assert_eq!(w.state().selection(), [duplicate]);
    assert!(near(centre_of(&w, duplicate).0, centre_of(&w, pasted).0 + 0.25) && near(centre_of(&w, duplicate).1, centre_of(&w, pasted).1 - 0.25));
    key(&mut w, Key::D, CTRL);
    assert_eq!(shape_ids(&w).len(), 6, "and again");
    // Ctrl+X cuts what is selected.
    let last = *shape_ids(&w).last().unwrap();
    key(&mut w, Key::X, CTRL);
    assert_eq!(shape_ids(&w).len(), 5, "Ctrl+X");
    assert!(!shape_ids(&w).contains(&last));
    assert!(w.state().selection().is_empty());
    undo(&mut w);
    assert!(shape_ids(&w).contains(&last), "undo brings the cut back");
    assert!(shape_ids(&w).starts_with(&ids));
}

/// On a desktop egui-winit turns Ctrl+C, X and V into events and sends no key: the window takes the events when no text field has the keyboard.
#[test]
fn the_events_a_desktop_sends_for_copy_cut_and_paste_run_the_commands() {
    let (mut w, ids) = three();
    w.input_mut().events.push(Event::Copy);
    w.step();
    w.rest();
    assert!(w.state().session().clipboard().is_some_and(|c| c.len() == 1), "Event::Copy");
    w.input_mut().events.push(Event::Paste("text from another program".to_owned()));
    w.step();
    w.rest();
    assert_eq!(shape_ids(&w).len(), 4, "Event::Paste pastes the session's clipboard and not the text");
    w.input_mut().events.push(Event::Cut);
    w.step();
    w.rest();
    assert_eq!(shape_ids(&w).len(), 3, "Event::Cut cut the pasted shape");
    assert_eq!(&shape_ids(&w)[..], &ids[..], "and the original is where it was");
}

/// Group and Ungroup, from the keys and the Group menu: the group is selected after grouping, the members after ungrouping, and each is one undo step.
#[test]
fn group_and_ungroup_from_the_keys_and_the_menu() {
    let (mut w, ids) = three();
    assert!(
        !enabled(&w, "ribbon.Home.ObjectsGroupMenu") || {
            // One shape selected: the menu may be open to Ungroup's quiet row only if some item applies, and none does.
            false
        }
    );
    click_with(&mut w, (7.0, 6.0), Modifiers::SHIFT);
    assert_eq!(w.state().selection(), &ids[..2]);
    key(&mut w, Key::G, CTRL_SHIFT);
    let after = shape_ids(&w);
    assert_eq!(after.len(), 2, "two shapes became one group: the group and the ellipse");
    // The group stands where the topmost of its members stood, and is what is selected.
    assert_eq!(w.state().selection().len(), 1, "the group is selected");
    let group = w.state().selection()[0];
    assert!(after.contains(&group) && !ids.contains(&group), "a new shape");
    let before = w.state().session().revision();
    // The menu: Ungroup applies, Group does not (one shape selected), and the rest wait.
    hit(&mut w, "ribbon.Home.ObjectsGroupMenu");
    assert!(enabled(&w, "menu.ObjectsUngroup"));
    assert!(!enabled(&w, "menu.ObjectsGroup"), "a group alone is not grouped again");
    for quiet in ["menu.ObjectsAddToGroup", "menu.ObjectsRemoveFromGroup", "menu.ObjectsConvertToGroup"] {
        assert!(!enabled(&w, quiet), "{quiet} waits for its command");
    }
    hit(&mut w, "menu.ObjectsUngroup");
    assert_eq!(shape_ids(&w).len(), 3, "the group is taken apart");
    let mut selected = w.state().selection().to_vec();
    selected.sort_unstable();
    assert_eq!(selected, ids[..2], "the members are selected");
    assert_eq!(w.state().session().revision(), before + 1, "one command");
    // Ctrl+Shift+G again with the two selected, then Ctrl+Shift+U.
    key(&mut w, Key::G, CTRL_SHIFT);
    assert_eq!(shape_ids(&w).len(), 2);
    key(&mut w, Key::U, CTRL_SHIFT);
    assert_eq!(shape_ids(&w).len(), 3, "Ctrl+Shift+U");
    // Undo, undo, undo, undo walks back to three plain shapes at their places.
    for _ in 0..4 {
        undo(&mut w);
    }
    assert_eq!(shape_ids(&w), ids);
    assert_eq!(centre_of(&w, ids[0]), (3.0, 6.0));
    assert_eq!(centre_of(&w, ids[1]), (7.0, 6.0));
    assert_eq!(w.state().ui_panics(), 0);
}

/// A window command that cannot run says why: Group wants two shapes in one list, Ungroup a group.
#[test]
fn group_and_ungroup_say_why_they_cannot_run() {
    let (w, _) = three();
    assert_eq!(w.state().command_enabled("arrange.group"), Err("select two shapes or more to group".to_owned()));
    assert_eq!(w.state().command_enabled("arrange.ungroup"), Err("no shape selected is a group".to_owned()));
    assert!(w.state().command_enabled("clipboard.copy").is_ok());
    assert_eq!(w.state().command_enabled("clipboard.paste"), Err("the clipboard is empty".to_owned()));
    assert!(w.state().command_enabled("clipboard.duplicate").is_ok());
}

// ---- Ctrl-drag ----------------------------------------------------------------------------------------------------------------------------------

/// A drag that begins with Ctrl held leaves the shape and makes a copy the drag's distance away, and selects the copy; one undo takes it away.
#[test]
fn a_ctrl_drag_copies_the_shape_and_leaves_the_original() {
    let (mut w, ids) = three();
    let revision = w.state().session().revision();
    drag_with(&mut w, (3.0, 6.0), (4.5, 5.0), CTRL);
    assert_eq!(shape_ids(&w).len(), 4, "a copy was made");
    assert_eq!(centre_of(&w, ids[0]), (3.0, 6.0), "the original did not move");
    let copy = *shape_ids(&w).last().unwrap();
    let (x, y) = centre_of(&w, copy);
    assert!(near(x, 4.5) && near(y, 5.0), "the copy is where a move would have put the shape, on the grid: ({x}, {y})");
    assert_eq!(w.state().selection(), [copy], "the copy is selected");
    assert_eq!(w.state().session().revision(), revision + 1, "one command");
    undo(&mut w);
    assert_eq!(shape_ids(&w), ids);
    // The clipboard was not used.
    assert!(w.state().session().clipboard().is_none());
}

/// Ctrl is looked at once, at the press: Ctrl pressed in the middle of a drag does not make it a copy, and Ctrl let go in the middle of
/// a copy-drag does not make it a move.
#[test]
fn ctrl_gained_or_lost_in_the_middle_of_a_drag_changes_nothing() {
    let (mut w, ids) = three();
    // Begun without Ctrl, Ctrl held for the second half: a move.
    let from = crate::editing::at(&w, 3.0, 6.0);
    let to = crate::editing::at(&w, 4.5, 5.0);
    for _ in 0..3 {
        move_to(&mut w, from);
    }
    button(&mut w, from, true);
    for step in 1..=3 {
        move_to(&mut w, from + (to - from) * (step as f32 / 6.0));
    }
    hold(&mut w, CTRL);
    for step in 4..=6 {
        move_to(&mut w, from + (to - from) * (step as f32 / 6.0));
    }
    button(&mut w, to, false);
    hold(&mut w, Modifiers::NONE);
    w.rest();
    assert_eq!(shape_ids(&w), ids, "no copy: Ctrl was gained in the middle");
    assert!(near(centre_of(&w, ids[0]).0, 4.5) && near(centre_of(&w, ids[0]).1, 5.0), "the shape moved: {:?}", centre_of(&w, ids[0]));
    // Begun with Ctrl, let go for the second half: a copy.
    hold(&mut w, CTRL);
    let (from, to) = (crate::editing::at(&w, 4.5, 5.0), crate::editing::at(&w, 6.0, 4.0));
    for _ in 0..3 {
        move_to(&mut w, from);
    }
    button(&mut w, from, true);
    for step in 1..=3 {
        move_to(&mut w, from + (to - from) * (step as f32 / 6.0));
    }
    hold(&mut w, Modifiers::NONE);
    for step in 4..=6 {
        move_to(&mut w, from + (to - from) * (step as f32 / 6.0));
    }
    button(&mut w, to, false);
    w.rest();
    assert_eq!(shape_ids(&w).len(), 4, "a copy: Ctrl was held at the press");
    assert!(near(centre_of(&w, ids[0]).0, 4.5) && near(centre_of(&w, ids[0]).1, 5.0), "the original stayed");
}

/// A Ctrl-click, with no drag, still adds a shape to the selection and drops one from it; a Ctrl-drag of a shape that is selected with others copies them all.
#[test]
fn a_ctrl_click_still_toggles_and_a_ctrl_drag_copies_the_whole_selection() {
    let (mut w, ids) = three();
    click_with(&mut w, (7.0, 6.0), CTRL);
    assert_eq!(w.state().selection(), &ids[..2], "Ctrl-click adds");
    click_with(&mut w, (7.0, 6.0), CTRL);
    assert_eq!(w.state().selection(), &ids[..1], "and drops what is selected");
    click_with(&mut w, (7.0, 6.0), CTRL);
    assert_eq!(shape_ids(&w).len(), 3, "no copy was made by clicks");
    // Both selected: a Ctrl-drag by one of them copies both, a whole step away.
    drag_with(&mut w, (3.0, 6.0), (3.0, 4.0), CTRL);
    assert_eq!(shape_ids(&w).len(), 5, "two copies");
    let selected = w.state().selection().to_vec();
    assert_eq!(selected.len(), 2, "both copies are selected");
    assert!(
        near(centre_of(&w, selected[0]).1, 4.0) && near(centre_of(&w, selected[1]).1, 4.0),
        "{:?}",
        selected.iter().map(|id| centre_of(&w, *id)).collect::<Vec<_>>()
    );
    assert_eq!(centre_of(&w, ids[0]), (3.0, 6.0));
}

// ---- keys that belong to a text ----------------------------------------------------------------------------------------------------------------------

/// With the shape's text box open the clipboard keys, Ctrl+D and Ctrl+Shift+G are the box's: Ctrl+C and the copy event copy its text and not the
/// shape, and nothing runs on the drawing.
#[test]
fn the_clipboard_keys_leave_a_text_box_its_own_keys() {
    let (mut w, ids) = three();
    key(&mut w, Key::F2, Modifiers::NONE);
    assert!(w.state().window_state()["editingText"].as_bool().unwrap());
    type_text(&mut w, "abc");
    let revision = w.state().session().revision();
    for (k, mods) in [
        (Key::C, CTRL),
        (Key::X, CTRL),
        (Key::V, CTRL),
        (Key::D, CTRL),
        (Key::G, CTRL_SHIFT),
        (Key::U, CTRL_SHIFT),
        (Key::B, CTRL_SHIFT),
        (Key::I, CTRL_SHIFT),
    ] {
        key(&mut w, k, mods);
    }
    for event in [Event::Copy, Event::Cut, Event::Paste("zz".to_owned())] {
        w.input_mut().events.push(event);
        w.step();
        w.rest();
    }
    assert!(w.state().session().clipboard().is_none(), "the shape was not copied");
    assert_eq!(w.state().session().revision(), revision, "nothing ran on the drawing");
    assert_eq!(shape_ids(&w), ids);
    assert!(w.state().window_state()["editingText"].as_bool().unwrap(), "the box is open");
    // The ribbon says why: Copy, Cut and Paste are quiet while the box is open.
    for name in ["Cut", "Copy"] {
        assert!(!enabled(&w, &format!("ribbon.Home.{name}")), "{name}");
    }
    assert_eq!(w.state().command_enabled("clipboard.copy"), Err("a text box is open: its text is copied and cut by its own keys".to_owned()));
}

/// A text field that is not the shape's box keeps its keys as well: with the Font list's box focused, Ctrl+C, Ctrl+D and the copy event leave the drawing alone.
#[test]
fn another_text_field_keeps_the_clipboard_keys_too() {
    let scan: diacraft_ui_egui::font_list::Scan = std::sync::Arc::new(|| vec!["Test Sans".to_owned()]);
    let mut w = edit_window_with([1280.0, 900.0], |app| app.with_font_scan(scan));
    let ids = three_shapes(&mut w);
    click_at(&mut w, 3.0, 6.0);
    hit(&mut w, "ribbon.Home.Font");
    assert!(w.state().element("font.filter").is_some(), "the Font list is open");
    let revision = w.state().session().revision();
    key(&mut w, Key::C, CTRL);
    key(&mut w, Key::D, CTRL);
    key(&mut w, Key::G, CTRL_SHIFT);
    w.input_mut().events.push(Event::Copy);
    w.step();
    w.rest();
    assert!(w.state().session().clipboard().is_none());
    assert_eq!(w.state().session().revision(), revision);
    assert_eq!(shape_ids(&w), ids);
}

// ---- the system clipboard -----------------------------------------------------------------------------------------------------------------------------

/// Copy puts the text of the shapes' labels on the system clipboard (egui carries text or an image, one of them): one shape to a line; a copy of shapes
/// with no text leaves the system clipboard alone.
#[test]
fn copy_puts_the_text_of_the_labels_on_the_system_clipboard() {
    let (mut w, ids) = three();
    // No text on the rectangle: nothing goes to the system clipboard.
    w.input_mut().events.push(Event::Key { key: Key::C, physical_key: None, pressed: true, repeat: false, modifiers: CTRL });
    w.step();
    assert!(system_copies(&w).is_empty(), "no label: {:?}", system_copies(&w));
    w.rest();
    // Two labelled shapes: the text of both, in the order selected.
    let page = page_id(&w);
    run_command(&mut w, "shape.set_text", json!({"id": ids[0], "text": "Start", "page": page}));
    run_command(&mut w, "shape.set_text", json!({"id": ids[1], "text": "Is it ready?", "page": page}));
    click_at(&mut w, 7.0, 6.0);
    click_with(&mut w, (3.0, 6.0), Modifiers::SHIFT);
    assert_eq!(w.state().selection(), [ids[1], ids[0]]);
    w.input_mut().events.push(Event::Key { key: Key::C, physical_key: None, pressed: true, repeat: false, modifiers: CTRL });
    w.step();
    assert_eq!(system_copies(&w), ["Is it ready?\nStart"]);
    w.rest();
    // Cut sends the text too, and the shapes go.
    w.input_mut().events.push(Event::Key { key: Key::X, physical_key: None, pressed: true, repeat: false, modifiers: CTRL });
    w.step();
    assert_eq!(system_copies(&w), ["Is it ready?\nStart"]);
    w.rest();
    assert_eq!(shape_ids(&w), ids[2..]);
    // The text of a group's members comes after the group's own.
    let doc = w.state().session().document().unwrap();
    assert_eq!(labels_of(doc, page_id(&w), &[ids[2]]), "", "no text on the ellipse");
    assert_eq!(labels_of(doc, page_id(&w), &[999]), "", "no such shape");
}

// ---- the context menu ---------------------------------------------------------------------------------------------------------------------------------

/// A right click selects the shape under it when it is not selected, opens the menu, and Paste there puts the middle of the copies at the click.
#[test]
fn the_context_menu_pastes_at_the_click() {
    let (mut w, ids) = three();
    key(&mut w, Key::C, CTRL);
    // Right click on the diamond: it becomes the selection, and the menu opens.
    let on_diamond = crate::editing::at(&w, 7.0, 6.0);
    right_click(&mut w, on_diamond);
    assert_eq!(w.state().selection(), [ids[1]], "the shape under the click is selected");
    for name in ["Cut", "Copy", "Paste", "Duplicate", "Group", "Ungroup"] {
        assert!(w.state().element(&format!("menu.Canvas{name}")).is_some(), "{name} is in the menu");
    }
    assert!(enabled(&w, "menu.CanvasPaste") && enabled(&w, "menu.CanvasCopy") && enabled(&w, "menu.CanvasDuplicate"));
    assert!(!enabled(&w, "menu.CanvasGroup") && !enabled(&w, "menu.CanvasUngroup"), "one shape that is no group");
    // Close it, and right click on bare page: Paste there.
    key(&mut w, Key::Escape, Modifiers::NONE);
    let bare = crate::editing::at(&w, 8.5, 2.5);
    right_click(&mut w, bare);
    hit(&mut w, "menu.CanvasPaste");
    assert_eq!(shape_ids(&w).len(), 4);
    let pasted = *shape_ids(&w).last().unwrap();
    let (x, y) = centre_of(&w, pasted);
    assert!(near(x, 8.5) && near(y, 2.5) || (x - 8.5).abs() < 0.02 && (y - 2.5).abs() < 0.02, "the middle of the copy is at the click: ({x}, {y})");
    assert_eq!(w.state().selection(), [pasted]);
    undo(&mut w);
    assert_eq!(shape_ids(&w), ids);
}

// ---- another page -------------------------------------------------------------------------------------------------------------------------------

/// Paste goes onto the page on view: the first paste on another page is where the shapes were.
#[test]
fn paste_goes_onto_the_page_on_view() {
    let (mut w, ids) = three();
    key(&mut w, Key::C, CTRL);
    run_command(&mut w, "page.add", json!({}));
    let pages: Vec<u32> = w.state().session().document().unwrap().pages.iter().map(|p| p.id).collect();
    assert_eq!(pages.len(), 2);
    run_command(&mut w, "page.select", json!({"id": pages[1]}));
    key(&mut w, Key::V, CTRL);
    let doc = w.state().session().document().unwrap();
    assert_eq!(doc.page(pages[1]).unwrap().shapes.len(), 1, "the copy is on the second page");
    assert_eq!(doc.page(pages[0]).unwrap().shapes.len(), 3, "and the first is as it was");
    let id = doc.page(pages[1]).unwrap().shapes.iter().next().unwrap().id;
    assert_eq!(w.state().selection(), [id]);
    let f = shape_frame(doc, pages[1], id).unwrap();
    assert!(near(f.x + f.width / 2.0, 3.0) && near(f.y + f.height / 2.0, 6.0), "where the original was: {f:?}");
    let _ = ids;
}
