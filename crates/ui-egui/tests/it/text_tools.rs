//! The Text tool and the Text Block tool: a box of text made where the page is clicked or dragged, a shape's text typed into with the Text
//! tool, and the text block of a shape moved, sized and turned apart from the shape. The pointer and the keyboard are given to egui as a person's would
//! be, and what is checked is the document they changed through commands.

use egui::{Key, Modifiers, vec2};
use serde_json::{Value, json};

use crate::editing::{at, click_at, edit_window, frame_of, key, shape_ids, three_shapes, type_text};
use crate::saving::{ctrl, ctrl_shift};
use crate::support::{Rest, Window, click, drag, press, run_command};

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-3
}

fn undo(w: &mut Window) {
    run_command(w, "edit.undo", json!({}));
}

/// What the engine says of the text block of shape `id` (a query: it changes and journals nothing).
fn block_of(w: &mut Window, id: u32) -> Value {
    w.state_mut().execute("text.block", &json!({"id": id})).unwrap()
}

fn text_of(w: &Window, id: u32) -> String {
    let doc = w.state().session().document().unwrap();
    diacraft_ui_egui::edit::plain_text(doc, doc.pages.get(0).unwrap().id, id)
}

fn number(v: &Value) -> f64 {
    v.as_f64().unwrap()
}

/// The page point of corner `n` of a block answer, in inches.
fn corner(block: &Value, n: usize) -> (f64, f64) {
    (number(&block["corners"][n][0]), number(&block["corners"][n][1]))
}

fn select_tool(w: &mut Window, id: &str) {
    run_command(w, id, json!({}));
}

// ---- the Text tool ---------------------------------------------------------------------------------------------------------------------------

/// Ctrl+2 turns the Text tool on; a click on bare page opens the box where it was clicked and makes no shape; the characters typed go with `text.box`
/// when the edit ends, one undo step; the box is selected, and the Pointer Tool is back.
#[test]
fn the_text_tool_opens_a_box_where_the_page_is_clicked_and_makes_it_when_the_edit_ends() {
    let mut w = edit_window([1280.0, 900.0]);
    key(&mut w, Key::Num2, ctrl());
    assert_eq!(w.state().tool().name(), "text", "Ctrl+2 is the Text tool");
    click_at(&mut w, 3.04, 6.04);
    assert!(w.state().window_state()["editingText"].as_bool().unwrap(), "the box is open for typing");
    assert!(shape_ids(&w).is_empty(), "and no shape has been made yet");
    assert_eq!(w.state().tool().name(), "pointer", "the tool is used up by the box it opened");
    let before = w.state().session().revision();
    type_text(&mut w, "Hello");
    assert_eq!(w.state().session().revision(), before, "nothing is sent while the box is open");
    assert!(w.state().window_title().starts_with('*'), "typed characters are a change a close would lose");
    key(&mut w, Key::Escape, Modifiers::NONE);
    let ids = shape_ids(&w);
    assert_eq!(ids.len(), 1, "one box");
    assert_eq!(w.state().session().revision(), before + 1, "one command");
    assert_eq!(text_of(&w, ids[0]), "Hello");
    let (x, y, width, height) = frame_of(&w, ids[0]);
    assert!(near(x + width / 2.0, 3.0) && near(y + height / 2.0, 6.0), "centred on the grid point nearest the click: {x} {y}");
    assert!(near(width, 1.0) && near(height, 0.25), "the size a text box has: {width} x {height}");
    assert_eq!(w.state().selection(), ids.as_slice(), "the new box is selected");
    undo(&mut w);
    assert!(shape_ids(&w).is_empty(), "the box was one undo step");
    assert_eq!(w.state().ui_panics(), 0);
}

/// A box left empty is not made: no shape, no undo step, and the drawing is not changed.
#[test]
fn a_box_left_empty_leaves_nothing_on_the_page() {
    let mut w = edit_window([1280.0, 900.0]);
    select_tool(&mut w, "tool.text");
    click_at(&mut w, 4.0, 4.0);
    assert!(w.state().window_state()["editingText"].as_bool().unwrap());
    let before = w.state().session().revision();
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert!(shape_ids(&w).is_empty());
    assert_eq!(w.state().session().revision(), before, "no command was sent");
    assert!(!w.state().is_dirty());
    // A click away from a box with something in it keeps it; a press on the page ends the edit and the next click is a Pointer Tool's.
    select_tool(&mut w, "tool.text");
    click_at(&mut w, 4.0, 4.0);
    type_text(&mut w, "Kept");
    click_at(&mut w, 8.0, 2.0);
    assert_eq!(shape_ids(&w).len(), 1, "a click away ends the edit and keeps what was typed");
    assert!(!w.state().window_state()["editingText"].as_bool().unwrap());
}

/// A drag with the Text tool makes a box of the size dragged, on the grid.
#[test]
fn a_drag_with_the_text_tool_makes_a_box_of_the_size_dragged() {
    let mut w = edit_window([1280.0, 900.0]);
    select_tool(&mut w, "tool.text");
    crate::editing::drag_with(&mut w, (2.0, 2.0), (5.0, 3.0), Modifiers::NONE);
    assert!(w.state().window_state()["editingText"].as_bool().unwrap(), "the box is open");
    type_text(&mut w, "A wide note");
    key(&mut w, Key::Enter, ctrl());
    let ids = shape_ids(&w);
    assert_eq!(ids.len(), 1, "Ctrl+Enter ends the edit too");
    let (x, y, width, height) = frame_of(&w, ids[0]);
    assert!(near(x, 2.0) && near(y, 2.0) && near(width, 3.0) && near(height, 1.0), "{x} {y} {width} {height}");
    assert_eq!(text_of(&w, ids[0]), "A wide note");
}

/// A click on a shape with the Text tool types into that shape: `shape.set_text`, no new box.
#[test]
fn a_click_on_a_shape_with_the_text_tool_types_into_the_shape() {
    let mut w = edit_window([1280.0, 900.0]);
    let ids = three_shapes(&mut w);
    select_tool(&mut w, "tool.text");
    click_at(&mut w, 3.0, 6.0);
    assert!(w.state().window_state()["editingText"].as_bool().unwrap());
    assert_eq!(w.state().selection(), &ids[..1], "the shape clicked is selected");
    type_text(&mut w, "Start");
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert_eq!(shape_ids(&w), ids, "no box was made");
    assert_eq!(text_of(&w, ids[0]), "Start");
    assert_eq!(w.state().tool().name(), "pointer");
}

/// The Home tab's Text button and the Insert tab's Horizontal Text Box are the same tool.
#[test]
fn the_text_buttons_turn_the_tool_on_and_show_it() {
    let mut w = edit_window([1280.0, 900.0]);
    let text = w.state().element("ribbon.Home.TextTool").unwrap().clone();
    assert!(text.enabled, "the Text button works");
    click(&mut w, text.rect.center());
    w.rest();
    assert_eq!(w.state().tool().name(), "text");
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert_eq!(w.state().tool().name(), "pointer", "Escape leaves the tool");
    run_command(&mut w, "window.tab", json!({"tab": "Insert"}));
    press(&mut w, "ribbon.Insert.TextBoxInsertMenu");
    let item = w.state().element("menu.TextBoxInsertHorizontal").expect("the menu lists Horizontal Text Box").clone();
    assert!(item.enabled);
    assert!(!w.state().element("menu.TextBoxInsertVertical").unwrap().enabled, "the vertical box has no command yet");
    click(&mut w, item.rect.center());
    w.rest();
    assert_eq!(w.state().tool().name(), "text");
}

/// With Auto Size on, a box put past the edge grows the page, and the window reads the page again.
#[test]
fn a_box_past_the_edge_grows_a_page_with_auto_size() {
    let mut w = edit_window([1280.0, 900.0]);
    run_command(&mut w, "page.auto_size", json!({"on": true}));
    // (Zoomed out, so that the right edge of the page is in the drawing area.)
    run_command(&mut w, "view.zoom", json!({"percent": 60}));
    select_tool(&mut w, "tool.text");
    click_at(&mut w, 10.9, 4.0);
    type_text(&mut w, "Edge");
    key(&mut w, Key::Escape, Modifiers::NONE);
    let setup = w.state_mut().execute("page.setup", &json!({})).unwrap();
    assert!(number(&setup["width"]) > 11.5, "the page grew: {}", setup["width"]);
    assert_eq!(w.state().page_view().map(|s| s.width > 11.5), Some(true), "and the window read it");
}

// ---- the Text Block tool ---------------------------------------------------------------------------------------------------------------------

/// Ctrl+Shift+4 turns the tool on; a click selects a shape and shows its block, over the shape, with the shape's own handles not shown.
#[test]
fn the_text_block_tool_shows_the_block_of_the_shape_clicked() {
    let mut w = edit_window([1280.0, 900.0]);
    let ids = three_shapes(&mut w);
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert!(w.state().selection().is_empty());
    key(&mut w, Key::Num4, ctrl_shift());
    assert_eq!(w.state().tool().name(), "text_block", "Ctrl+Shift+4 is the Text Block tool");
    assert!(w.state().text_block_shown().is_none(), "no shape is selected yet");
    click_at(&mut w, 3.0, 6.0);
    let shown = w.state().text_block_shown();
    let engine = block_of(&mut w, ids[0]);
    let shown = shown.expect("the block of the rectangle is shown");
    for (n, got) in shown.corners.iter().enumerate() {
        let c = corner(&engine, n);
        assert!(near(got.0, c.0) && near(got.1, c.1), "corner {n}: {got:?} against {c:?}");
    }
    assert!(near(shown.direction, 0.0));
    assert_eq!(w.state().selection(), &ids[..1]);
    // Bare page clears it.
    click_at(&mut w, 9.0, 1.0);
    assert!(w.state().text_block_shown().is_none());
    assert_eq!(w.state().tool().name(), "text_block", "the tool stays on");
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert_eq!(w.state().tool().name(), "pointer");
    assert_eq!(w.state().ui_panics(), 0);
}

/// A drag of the block's body moves the block, not the shape: `text.block_move`, one undo step.
#[test]
fn dragging_the_body_of_the_block_moves_the_text_and_not_the_shape() {
    let mut w = edit_window([1280.0, 900.0]);
    let ids = three_shapes(&mut w);
    select_tool(&mut w, "tool.text_block");
    click_at(&mut w, 3.0, 6.0);
    let frame = frame_of(&w, ids[0]);
    let before = block_of(&mut w, ids[0]);
    let revision = w.state().session().revision();
    crate::editing::drag_with(&mut w, (3.0, 6.0), (3.5, 6.25), Modifiers::NONE);
    assert_eq!(w.state().session().revision(), revision + 1, "one command");
    let after = block_of(&mut w, ids[0]);
    let (a, b) = (corner(&before, 0), corner(&after, 0));
    assert!(near(b.0 - a.0, 0.5) && near(b.1 - a.1, 0.25), "the block moved by the drag: {a:?} to {b:?}");
    assert_eq!(frame_of(&w, ids[0]), frame, "the shape did not move");
    let shown = w.state().text_block_shown().expect("the block stays shown");
    assert!(near(shown.corners[0].0, b.0) && near(shown.corners[0].1, b.1), "and is drawn where it is now");
    undo(&mut w);
    let back = block_of(&mut w, ids[0]);
    assert!(near(corner(&back, 0).0, a.0) && near(corner(&back, 0).1, a.1), "undo puts the block back");
}

/// A drag of a handle sizes the block along its own axes, with the opposite corner staying: `text.block_resize`.
#[test]
fn dragging_a_handle_of_the_block_sizes_it() {
    let mut w = edit_window([1280.0, 900.0]);
    let ids = three_shapes(&mut w);
    select_tool(&mut w, "tool.text_block");
    click_at(&mut w, 3.0, 6.0);
    let before = block_of(&mut w, ids[0]);
    // The rectangle is 1.5 x 1 centred at (3, 6): its east handle is at (3.75, 6).
    let revision = w.state().session().revision();
    crate::editing::drag_with(&mut w, (3.75, 6.0), (4.75, 6.0), Modifiers::NONE);
    assert_eq!(w.state().session().revision(), revision + 1, "one command");
    let after = block_of(&mut w, ids[0]);
    assert!(near(number(&after["width"]), number(&before["width"]) + 1.0), "wider by the drag: {} then {}", before["width"], after["width"]);
    assert!(near(number(&after["height"]), number(&before["height"])), "as tall as it was");
    let (a, b) = (corner(&before, 0), corner(&after, 0));
    assert!(near(a.0, b.0) && near(a.1, b.1), "the corner on the other side stays: {a:?} {b:?}");
    undo(&mut w);
    assert!(near(number(&block_of(&mut w, ids[0])["width"]), number(&before["width"])));
}

/// The rotation handle above the block turns it about its pin by the change of the way the text runs: `text.block_rotate`.
#[test]
fn dragging_the_rotation_handle_turns_the_text() {
    let mut w = edit_window([1280.0, 900.0]);
    let ids = three_shapes(&mut w);
    select_tool(&mut w, "tool.text_block");
    click_at(&mut w, 3.0, 6.0);
    let shown = w.state().text_block_shown().unwrap();
    let pin = at(&w, shown.pin.0, shown.pin.1);
    // The handle is 24 points above the top edge of the block (the shape is 1 inch tall: 48 points above the pin at 100%).
    let scale = f64::from(w.state().window_state()["pointsPerInch"].as_f64().unwrap() as f32);
    let grip = pin + vec2(0.0, -(0.5 * scale as f32 + 24.0));
    let revision = w.state().session().revision();
    drag(&mut w, grip, vec2(-(0.5 * scale as f32 + 24.0), 0.5 * scale as f32 + 24.0));
    w.rest();
    assert_eq!(w.state().session().revision(), revision + 1, "one command");
    let block = block_of(&mut w, ids[0]);
    assert!(near(number(&block["direction"]), 90.0), "a quarter turn counterclockwise: {}", block["direction"]);
    assert!(near(number(&block["angle"]) - std::f64::consts::FRAC_PI_2, 0.0) || near(number(&block["angle"]), 90.0), "{}", block["angle"]);
    let pin_after = w.state().text_block_shown().unwrap().pin;
    assert!(near(pin_after.0, shown.pin.0) && near(pin_after.1, shown.pin.1), "the block turns about its pin");
    undo(&mut w);
    assert!(near(number(&block_of(&mut w, ids[0])["direction"]), 0.0));
}

/// Rotate Text on the Home tab turns the text of every shape selected by 90 degrees, as one undo step.
#[test]
fn rotate_text_turns_the_text_of_the_selection_a_quarter() {
    let mut w = edit_window([1280.0, 900.0]);
    let ids = three_shapes(&mut w);
    key(&mut w, Key::Escape, Modifiers::NONE);
    let button = w.state().element("ribbon.Home.TextRotate90Degrees").unwrap().clone();
    assert!(!button.enabled, "nothing is selected");
    w.state_mut().select(&ids[..2]);
    w.rest();
    let button = w.state().element("ribbon.Home.TextRotate90Degrees").unwrap().clone();
    assert!(button.enabled, "two shapes are selected");
    click(&mut w, button.rect.center());
    w.rest();
    for id in &ids[..2] {
        assert!(near(number(&block_of(&mut w, *id)["direction"]), 90.0), "shape {id}");
    }
    assert!(near(number(&block_of(&mut w, ids[2])["direction"]), 0.0), "the third was not selected");
    click(&mut w, button.rect.center());
    w.rest();
    // (Half a turn is 180 or -180 degrees, whichever side of the line the arithmetic lands on.)
    assert!(near(number(&block_of(&mut w, ids[0])["direction"]).abs(), 180.0), "a second press turns it a quarter more");
    // Each press was one undo step for both shapes.
    undo(&mut w);
    for id in &ids[..2] {
        assert!(near(number(&block_of(&mut w, *id)["direction"]), 90.0), "one undo takes back one press, for both shapes: {id}");
    }
    undo(&mut w);
    for id in &ids[..2] {
        assert!(near(number(&block_of(&mut w, *id)["direction"]), 0.0), "shape {id}");
    }
}

/// The block, once moved and turned, is saved with the drawing and read again.
#[test]
fn a_moved_and_turned_block_is_saved_and_reopened() {
    let dir = crate::support::TempDir::new("block-save");
    let path = dir.file("block.vsdx");
    let mut w = edit_window([1280.0, 900.0]);
    let ids = three_shapes(&mut w);
    select_tool(&mut w, "tool.text_block");
    click_at(&mut w, 3.0, 6.0);
    crate::editing::drag_with(&mut w, (3.0, 6.0), (3.5, 6.25), Modifiers::NONE);
    w.state_mut().execute("text.block_rotate", &json!({"id": ids[0], "by": 90})).unwrap();
    let want = block_of(&mut w, ids[0]);
    w.state_mut().execute("file.save_as", &json!({"path": path.to_str().unwrap()})).unwrap();
    let mut again = diacraft_engine::Session::new();
    again.execute("file.open", &json!({"path": path.to_str().unwrap()})).unwrap();
    let page = again.document().unwrap().pages.get(0).unwrap().id;
    let got = again.execute("text.block", &json!({"id": ids[0], "page": page})).unwrap();
    for n in 0..4 {
        let (a, b) = (corner(&want, n), corner(&got, n));
        assert!(near(a.0, b.0) && near(a.1, b.1), "corner {n}: {a:?} {b:?}");
    }
    assert!(near(number(&got["direction"]), 90.0));
}
