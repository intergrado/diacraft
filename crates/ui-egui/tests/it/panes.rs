//! The docking panes at the right of the drawing area: the dock, Size & Position, Format Shape, Pan & Zoom and Drawing
//! Explorer. The pointer and the keyboard are given to egui as a person's would be, and what is checked is the document they changed through commands.

use egui::{Key, Modifiers, Rect};
use serde_json::{Value, json};

use diacraft_engine::Session;
use diacraft_ui_egui::DiacraftApp;
use diacraft_ui_egui::render_worker::bundled_drawer;

use crate::editing::{click_at, drag_tile_to, drag_with, edit_window, frame_of, key, shape_ids, type_text};
use crate::fixture;
use crate::saving::ctrl;
use crate::support::{Rest, TempDir, Window, click, drag, elements_named, fast_window, press, run_command};
use std::sync::{Arc, Mutex};

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-3
}

fn rect_of(w: &Window, name: &str) -> Rect {
    w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).rect
}

fn is_enabled(w: &Window, name: &str) -> bool {
    w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).enabled
}

fn click_on(w: &mut Window, name: &str) {
    let at = rect_of(w, name).center();
    click(w, at);
    w.rest();
}

fn open(w: &mut Window, pane: &str) {
    run_command(w, "window.pane", json!({"pane": pane, "shown": true}));
}

fn undo(w: &mut Window) {
    run_command(w, "edit.undo", json!({}));
}

/// Types `text` in the field `name` and ends it with Enter: one value, as a person types it.
fn type_in(w: &mut Window, name: &str, text: &str) {
    click_on(w, name);
    key(w, Key::A, ctrl());
    type_text(w, text);
    key(w, Key::Enter, Modifiers::NONE);
}

/// The text the field `name` showed in the last frame.
fn shown_in(w: &Window, name: &str) -> String {
    w.state().pane_field_text(name).unwrap_or_else(|| panic!("{name} was not drawn")).to_owned()
}

fn panes(w: &Window) -> Value {
    w.state().window_state()["panes"].clone()
}

fn page_id(w: &Window) -> u32 {
    w.state().session().document().unwrap().pages.get(0).unwrap().id
}

/// A rectangle dropped at (4, 5) and selected: 1.5 by 1, pin at its middle.
fn window_with_a_shape() -> (Window, u32) {
    let mut w = edit_window([1280.0, 900.0]);
    drag_tile_to(&mut w, "Rectangle", 4.0, 5.0);
    let id = shape_ids(&w)[0];
    (w, id)
}

fn pin_of(w: &Window, id: u32) -> (f64, f64) {
    let f = frame_of(w, id);
    (f.0 + f.2 / 2.0, f.1 + f.3 / 2.0)
}

// ---- the dock ----------------------------------------------------------------------------------------------------------------------------

/// View, Task Panes lists Size & Position and Pan & Zoom with a check that shows whether each is open; a pane docks at the right of the drawing area and
/// takes room from it, and the close button takes it away again.
#[test]
fn the_task_panes_menu_opens_a_pane_at_the_right_and_its_close_button_closes_it() {
    let mut w = edit_window([1280.0, 900.0]);
    let wide = rect_of(&w, "canvas");
    run_command(&mut w, "window.tab", json!({"tab": "View"}));
    press(&mut w, "ribbon.View.TaskPanesMenu");
    for item in ["menu.ShapesWindow", "menu.PanAndZoomWindow", "menu.SizeAndPositionWindow"] {
        assert!(is_enabled(&w, item), "{item} works");
    }
    click_on(&mut w, "menu.SizeAndPositionWindow");
    assert_eq!(panes(&w), json!([["size_position"]]));
    let pane = rect_of(&w, "pane.size_position");
    let canvas = rect_of(&w, "canvas");
    assert!(pane.left() >= canvas.right() - 1.0, "the pane is at the right of the drawing area: {pane:?} {canvas:?}");
    assert!(canvas.width() < wide.width() - 150.0, "and takes its room from the drawing area");
    let tabs = rect_of(&w, "page-tabs.add");
    assert!(tabs.right() <= pane.left() + 1.0, "the page tabs run under the drawing area only");
    // The title and the close button.
    assert!(w.state().element("pane.size_position.title").is_some());
    click_on(&mut w, "pane.size_position.close");
    assert_eq!(panes(&w), json!([]));
    assert!(w.state().element("pane.size_position").is_none());
    assert!(rect_of(&w, "canvas").width() > wide.width() - 2.0, "the drawing area has its room back");
    assert_eq!(w.state().ui_panics(), 0);
}

/// Several panes stack one above another, each with its own title and close button; two can share a space with tabs along the bottom.
#[test]
fn panes_stack_and_two_can_share_a_space_with_tabs_along_the_bottom() {
    let mut w = edit_window([1280.0, 900.0]);
    open(&mut w, "size_position");
    open(&mut w, "pan_zoom");
    assert_eq!(panes(&w), json!([["size_position"], ["pan_zoom"]]));
    let (a, b) = (rect_of(&w, "pane.size_position"), rect_of(&w, "pane.pan_zoom"));
    assert!(a.bottom() <= b.top() + 1.0 && (a.left() - b.left()).abs() < 1.0, "one above the other: {a:?} {b:?}");
    for pane in ["size_position", "pan_zoom"] {
        assert!(w.state().element(&format!("pane.{pane}.title")).is_some() && w.state().element(&format!("pane.{pane}.close")).is_some());
    }
    // Share a space: through the pane's menu.
    click_on(&mut w, "pane.pan_zoom.menu");
    click_on(&mut w, "pane.menu.share.size_position");
    assert_eq!(panes(&w), json!([["size_position", "pan_zoom"]]));
    assert!(w.state().element("pane.pan_zoom").is_some(), "the one on top is drawn");
    let (tab_a, tab_b) = (rect_of(&w, "pane.tab.size_position"), rect_of(&w, "pane.tab.pan_zoom"));
    let space = rect_of(&w, "pane.pan_zoom");
    assert!(tab_a.top() > space.center().y && (tab_a.bottom() - space.bottom()).abs() < 2.0, "the tabs are along the bottom: {tab_a:?} in {space:?}");
    assert!(tab_a.right() <= tab_b.left() + 1.0);
    click_on(&mut w, "pane.tab.size_position");
    assert!(w.state().element("pane.size.x").is_some(), "the tab brings its pane to the top");
    // The menu takes one out to a space of its own.
    click_on(&mut w, "pane.size_position.menu");
    click_on(&mut w, "pane.menu.own");
    assert_eq!(panes(&w), json!([["pan_zoom"], ["size_position"]]));
    // A pane that is not open cannot be shared with.
    assert!(w.state_mut().execute("window.paneDock", &json!({"pane": "size_position", "with": "drawing_explorer"})).is_err());
    assert!(w.state_mut().execute("window.pane", &json!({"pane": "nothing"})).is_err());
    assert!(w.state_mut().execute("window.pane", &json!({"pane": "pan_zoom", "shown": 3})).is_err());
    run_command(&mut w, "window.paneDock", json!({"pane": "pan_zoom", "with": "size_position"}));
    assert_eq!(panes(&w), json!([["size_position", "pan_zoom"]]));
    run_command(&mut w, "window.pane", json!({"pane": "pan_zoom"}));
    assert_eq!(panes(&w), json!([["size_position"]]), "a command with no `shown` toggles");
}

// ---- Size & Position ---------------------------------------------------------------------------------------------------------------------

/// The title holds the name of the shape selected; the fields hold its X, Y, Width, Height and Angle in the page's unit.
#[test]
fn size_and_position_shows_the_shape_selected_and_its_name_is_in_the_title() {
    use egui_kittest::kittest::Queryable;
    let (mut w, id) = window_with_a_shape();
    open(&mut w, "size_position");
    w.get_by_label(&format!("Size & Position - Rectangle.{id}"));
    for name in ["pane.size.x", "pane.size.y", "pane.size.width", "pane.size.height", "pane.size.angle"] {
        assert!(is_enabled(&w, name), "{name} works");
    }
    assert_eq!(shown_in(&w, "pane.size.x"), "4 in", "the pin is at the middle of the box, 4 inches from the page's left");
    assert_eq!(shown_in(&w, "pane.size.y"), "5 in");
    assert_eq!(shown_in(&w, "pane.size.width"), "1.5 in");
    assert_eq!(shown_in(&w, "pane.size.height"), "1 in");
    assert_eq!(shown_in(&w, "pane.size.angle"), "0 deg");
    // Nothing selected: the title is the pane's, and the fields are quiet and blank.
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert!(w.state().selection().is_empty());
    w.get_by_label("Size & Position");
    assert!(!is_enabled(&w, "pane.size.x") && shown_in(&w, "pane.size.x").is_empty());
}

/// A typed width sends one `shape.resize` about the pin: the pin stays where it is on the page, and the box grows on both sides of it.
#[test]
fn a_typed_width_and_height_resize_the_shape_about_its_pin_in_one_undo_step_each() {
    let (mut w, id) = window_with_a_shape();
    open(&mut w, "size_position");
    let before = w.state().session().revision();
    type_in(&mut w, "pane.size.width", "2 in.");
    assert_eq!(w.state().session().revision(), before + 1, "one command");
    let (x, y, width, height) = frame_of(&w, id);
    assert!(near(width, 2.0) && near(height, 1.0), "{width} x {height}");
    assert!(near(x + width / 2.0, 4.0) && near(y + height / 2.0, 5.0), "the pin stayed at (4, 5)");
    assert_eq!(shown_in(&w, "pane.size.width"), "2 in", "and the field shows what the shape has");
    // `1 ft. 5 in.` is 17 inches.
    type_in(&mut w, "pane.size.height", "1 ft. 5 in.");
    let (_, y, _, height) = frame_of(&w, id);
    assert!(near(height, 17.0) && near(y + height / 2.0, 5.0), "17 inches tall about the same pin: {height}");
    undo(&mut w);
    assert!(near(frame_of(&w, id).3, 1.0), "each was its own undo step");
    undo(&mut w);
    assert!(near(frame_of(&w, id).2, 1.5));
}

/// A comma between values is an error, and the size is unchanged.
#[test]
fn a_comma_between_values_is_refused_in_words_and_changes_nothing() {
    let (mut w, id) = window_with_a_shape();
    open(&mut w, "size_position");
    let before = w.state().session().revision();
    type_in(&mut w, "pane.size.height", "1 ft, 5 in");
    assert_eq!(w.state().session().revision(), before, "nothing was sent");
    assert!(near(frame_of(&w, id).3, 1.0));
    assert!(w.state().notice().is_some_and(|n| n.contains("comma")), "{:?}", w.state().notice());
    assert_eq!(shown_in(&w, "pane.size.height"), "1 in", "the field shows what the shape has again");
    type_in(&mut w, "pane.size.width", "abc");
    type_in(&mut w, "pane.size.width", "0");
    type_in(&mut w, "pane.size.width", "-3 in");
    assert_eq!(w.state().session().revision(), before);
    assert_eq!(w.state().ui_panics(), 0);
}

/// X and Y move the shape so that its pin is at the value; Angle turns it; each is one command.
#[test]
fn x_y_and_angle_move_and_turn_the_shape() {
    let (mut w, id) = window_with_a_shape();
    open(&mut w, "size_position");
    let before = w.state().session().revision();
    type_in(&mut w, "pane.size.x", "6.5");
    type_in(&mut w, "pane.size.y", "2 in + 1 in");
    assert_eq!(w.state().session().revision(), before + 2, "two commands");
    let (px, py) = pin_of(&w, id);
    assert!(near(px, 6.5) && near(py, 3.0), "({px}, {py})");
    type_in(&mut w, "pane.size.angle", "90");
    assert_eq!(shown_in(&w, "pane.size.angle"), "90 deg");
    let f = diacraft_engine::draw::shape_frame(w.state().session().document().unwrap(), page_id(&w), id).unwrap();
    assert!(near(f.angle.to_degrees(), 90.0), "{}", f.angle);
    assert!(near(f.pin_x, 6.5) && near(f.pin_y, 3.0), "it turned about its pin");
    // A turned shape is resized along its own axes, about its pin.
    type_in(&mut w, "pane.size.width", "3");
    let f = diacraft_engine::draw::shape_frame(w.state().session().document().unwrap(), page_id(&w), id).unwrap();
    assert!(near(f.width, 3.0) && near(f.pin_x, 6.5) && near(f.pin_y, 3.0), "{f:?}");
    undo(&mut w);
    undo(&mut w);
    undo(&mut w);
    let (px, py) = pin_of(&w, id);
    assert!(near(px, 6.5) && near(py, 5.0), "three undos back to after the X");
}

/// Lengths are in the page's unit at its scale, and typed the same way.
#[test]
fn lengths_follow_the_pages_drawing_scale() {
    let mut w = edit_window([1280.0, 900.0]);
    run_command(&mut w, "page.scale", json!({"preset": "arch_1_4"}));
    drag_tile_to(&mut w, "Rectangle", 4.0, 5.0);
    let id = shape_ids(&w)[0];
    open(&mut w, "size_position");
    // 4 paper inches at 1/4 inch to a foot are 16 feet; the scale is 48 times the master's, beyond the range of eight, so the master is dropped at the size it has on paper:
    // 1.5 inches, which are 72 drawing inches, 6 feet.
    assert_eq!(shown_in(&w, "pane.size.x"), "16 ft");
    assert_eq!(shown_in(&w, "pane.size.width"), "6 ft");
    type_in(&mut w, "pane.size.x", "20 ft");
    // The engine's frame is in the page's drawing units, inches of the drawing: 20 feet are 240 of them (and 5 inches on the paper).
    let (px, _) = pin_of(&w, id);
    assert!(near(px, 240.0), "{px}");
    assert_eq!(shown_in(&w, "pane.size.x"), "20 ft");
    type_in(&mut w, "pane.size.width", "96");
    assert!(near(frame_of(&w, id).2, 96.0 * 12.0), "96 is feet, the field's unit: {}", frame_of(&w, id).2);
    assert_eq!(shown_in(&w, "pane.size.width"), "96 ft");
}

/// With several shapes selected a field is blank where they differ, a value typed in X puts every shape's pin at it as one undo step, and a size typed is
/// the size of all of them (`shape.size`).
#[test]
fn several_shapes_show_blank_where_they_differ_and_a_typed_value_applies_to_all() {
    let mut w = edit_window([1280.0, 900.0]);
    drag_tile_to(&mut w, "Rectangle", 3.0, 6.0);
    drag_tile_to(&mut w, "Diamond", 7.0, 6.0);
    let ids = shape_ids(&w);
    w.state_mut().select(&ids);
    open(&mut w, "size_position");
    assert_eq!(shown_in(&w, "pane.size.x"), "", "the pins differ across");
    assert_eq!(shown_in(&w, "pane.size.y"), "6 in", "and agree up");
    assert_eq!(shown_in(&w, "pane.size.angle"), "0 deg");
    let before = w.state().session().revision();
    type_in(&mut w, "pane.size.x", "5");
    assert!(w.state().session().revision() > before);
    for id in &ids {
        assert!(near(pin_of(&w, *id).0, 5.0), "shape {id}");
    }
    assert_eq!(shown_in(&w, "pane.size.x"), "5 in");
    undo(&mut w);
    assert!(near(pin_of(&w, ids[0]).0, 3.0) && near(pin_of(&w, ids[1]).0, 7.0), "one undo step took both back");
}

/// A width typed with several shapes selected is the width of every one of them, each keeping its pin, in one undo step; a line among them makes the size
/// quiet, with the reason, since the engine would refuse the whole call.
#[test]
fn a_typed_size_applies_to_every_shape_selected_and_a_line_among_them_makes_it_quiet() {
    let mut w = edit_window([1280.0, 900.0]);
    drag_tile_to(&mut w, "Rectangle", 3.0, 6.0);
    drag_tile_to(&mut w, "Diamond", 7.0, 3.0);
    let ids = shape_ids(&w);
    let pins: Vec<(f64, f64)> = ids.iter().map(|id| pin_of(&w, *id)).collect();
    w.state_mut().select(&ids);
    open(&mut w, "size_position");
    assert!(is_enabled(&w, "pane.size.width") && is_enabled(&w, "pane.size.height"));
    let before = w.state().session().revision();
    type_in(&mut w, "pane.size.width", "2.5");
    let after = w.state().session().revision();
    assert!(after > before);
    for (id, pin) in ids.iter().zip(&pins) {
        let f = frame_of(&w, *id);
        assert!(near(f.2, 2.5), "shape {id} is 2.5 wide: {f:?}");
        assert!(near(pin_of(&w, *id).0, pin.0) && near(pin_of(&w, *id).1, pin.1), "shape {id} keeps its pin");
    }
    assert_eq!(shown_in(&w, "pane.size.width"), "2.5 in");
    undo(&mut w);
    assert!(near(frame_of(&w, ids[0]).2, 1.5), "one undo step took both back");
    // A line in the selection: the engine refuses a size for the whole call, so the fields are quiet.
    let page = page_id(&w);
    let made = w.state_mut().execute("connector.add", &json!({"from": {"x": 1.0, "y": 1.0}, "to": {"x": 2.0, "y": 1.0}, "page": page})).unwrap();
    let line = made["id"].as_u64().unwrap() as u32;
    w.state_mut().select(&[ids[0], line]);
    w.rest();
    assert!(!is_enabled(&w, "pane.size.width") && !is_enabled(&w, "pane.size.height"), "a line is sized by its ends");
}

/// The nine places of Pin Pos: a press moves the pin on the shape and the shape stays where it is on the page; X and Y then count from the new pin, and a size
/// typed keeps it.
#[test]
fn pin_pos_moves_the_pin_on_the_shape_and_the_shape_stays_put() {
    let (mut w, id) = window_with_a_shape();
    open(&mut w, "size_position");
    assert_eq!(shown_in(&w, "pane.size.x"), "4 in");
    for place in ["top-left", "top-center", "top-right", "middle-left", "center", "middle-right", "bottom-left", "bottom-center", "bottom-right"] {
        assert!(is_enabled(&w, &format!("pane.size.pin.{place}")), "{place} works");
    }
    let frame = frame_of(&w, id);
    let before = w.state().session().revision();
    click_on(&mut w, "pane.size.pin.bottom-left");
    assert!(w.state().session().revision() > before, "one command");
    let after = frame_of(&w, id);
    assert!(
        near(after.0, frame.0) && near(after.1, frame.1) && near(after.2, frame.2) && near(after.3, frame.3),
        "the shape did not move: {frame:?} {after:?}"
    );
    assert_eq!(shown_in(&w, "pane.size.x"), "3.25 in", "X is now the corner's");
    assert_eq!(shown_in(&w, "pane.size.y"), "4.5 in");
    // A press on the place the pin is at sends nothing.
    let rev = w.state().session().revision();
    click_on(&mut w, "pane.size.pin.bottom-left");
    assert_eq!(w.state().session().revision(), rev);
    // The pin stays when the size is typed: the left edge does not move.
    type_in(&mut w, "pane.size.width", "3");
    let sized = frame_of(&w, id);
    assert!(near(sized.0, frame.0) && near(sized.2, 3.0), "{sized:?}");
    // And X now moves the corner.
    type_in(&mut w, "pane.size.x", "6");
    assert!(near(frame_of(&w, id).0, 6.0));
    undo(&mut w);
    undo(&mut w);
    undo(&mut w);
    assert!(near(frame_of(&w, id).0, frame.0) && near(frame_of(&w, id).2, frame.2));
    assert_eq!(shown_in(&w, "pane.size.x"), "4 in", "the pin is back at the middle");
}

/// A line shows Begin X, Begin Y, End X, End Y, Length and Angle; a typed end moves that end only, a typed length keeps the direction, and the angle turns it.
#[test]
fn a_line_shows_its_ends_length_and_angle_and_each_typed_value_is_one_command() {
    let mut w = edit_window([1280.0, 900.0]);
    let page = page_id(&w);
    let made = w.state_mut().execute("connector.add", &json!({"from": {"x": 2.0, "y": 2.0}, "to": {"x": 5.0, "y": 6.0}, "page": page})).unwrap();
    let line = made["id"].as_u64().unwrap() as u32;
    w.state_mut().select(&[line]);
    w.rest();
    open(&mut w, "size_position");
    assert_eq!(shown_in(&w, "pane.size.begin_x"), "2 in");
    assert_eq!(shown_in(&w, "pane.size.begin_y"), "2 in");
    assert_eq!(shown_in(&w, "pane.size.end_x"), "5 in");
    assert_eq!(shown_in(&w, "pane.size.end_y"), "6 in");
    assert_eq!(shown_in(&w, "pane.size.length"), "5 in");
    assert_eq!(shown_in(&w, "pane.size.angle"), "53.1301 deg");
    for name in ["begin_x", "begin_y", "end_x", "end_y", "length", "angle"] {
        assert!(is_enabled(&w, &format!("pane.size.{name}")), "{name} works for a line with free ends");
    }
    assert!(w.state().element("pane.size.pin").is_none(), "a line has no pin of its own, so there is no Pin Pos");
    // One end.
    let before = w.state().session().revision();
    type_in(&mut w, "pane.size.end_x", "8");
    assert!(w.state().session().revision() > before);
    assert_eq!(shown_in(&w, "pane.size.end_x"), "8 in");
    assert_eq!(shown_in(&w, "pane.size.end_y"), "6 in", "the other coordinate is kept");
    assert_eq!(shown_in(&w, "pane.size.begin_x"), "2 in", "and the other end");
    // The length keeps the direction.
    let angle = shown_in(&w, "pane.size.angle");
    type_in(&mut w, "pane.size.length", "10");
    assert_eq!(shown_in(&w, "pane.size.length"), "10 in");
    assert_eq!(shown_in(&w, "pane.size.angle"), angle);
    // The angle turns the free end about the begin.
    type_in(&mut w, "pane.size.angle", "0");
    assert_eq!(shown_in(&w, "pane.size.angle"), "0 deg");
    assert_eq!(shown_in(&w, "pane.size.length"), "10 in");
    for _ in 0..3 {
        undo(&mut w);
    }
    assert_eq!(shown_in(&w, "pane.size.end_x"), "5 in", "each was one undo step");
    assert_eq!(shown_in(&w, "pane.size.end_y"), "6 in");
}

/// The fields of an end that is glued to a shape are quiet (the end follows the shape) and say so; with both ends glued the length and the angle are quiet.
#[test]
fn a_glued_end_is_quiet_and_both_glued_ends_settle_the_length() {
    let mut w = edit_window([1280.0, 900.0]);
    drag_tile_to(&mut w, "Rectangle", 3.0, 6.0);
    drag_tile_to(&mut w, "Diamond", 8.0, 6.0);
    let ids = shape_ids(&w);
    let page = page_id(&w);
    let made = w.state_mut().execute("connector.add", &json!({"from": {"id": ids[0]}, "to": {"id": ids[1]}, "page": page})).unwrap();
    let line = made["id"].as_u64().unwrap() as u32;
    w.state_mut().select(&[line]);
    w.rest();
    open(&mut w, "size_position");
    for name in ["begin_x", "begin_y", "end_x", "end_y", "length", "angle"] {
        assert!(!is_enabled(&w, &format!("pane.size.{name}")), "{name} is quiet with both ends glued");
    }
    // Free one end: a connector from a shape to a point.
    let half = w.state_mut().execute("connector.add", &json!({"from": {"id": ids[0]}, "to": {"x": 6.0, "y": 2.0}, "page": page})).unwrap();
    let half = half["id"].as_u64().unwrap() as u32;
    w.state_mut().select(&[half]);
    w.rest();
    assert!(!is_enabled(&w, "pane.size.begin_x") && !is_enabled(&w, "pane.size.begin_y"), "the begin is glued");
    assert!(
        is_enabled(&w, "pane.size.end_x") && is_enabled(&w, "pane.size.end_y") && is_enabled(&w, "pane.size.length"),
        "the free end and the length work"
    );
}

// ---- Pan & Zoom --------------------------------------------------------------------------------------------------------------------------

/// A window with Pan & Zoom open and the page zoomed to 200%, so that the view rectangle is a part of the picture.
fn zoomed_window() -> Window {
    let (mut w, _) = window_with_a_shape();
    run_command(&mut w, "view.zoom", json!({"percent": 200}));
    open(&mut w, "pan_zoom");
    w.rest();
    w
}

/// The rectangle of the view in the picture is the part of the page the drawing area shows: it is inside the picture at 200% and as large as the picture
/// reaches (the page and its margin) when the whole page is on view.
#[test]
fn pan_and_zoom_shows_a_picture_of_the_page_and_the_part_of_it_in_view() {
    let mut w = zoomed_window();
    let thumb = rect_of(&w, "pane.pan_zoom.thumb");
    let view = rect_of(&w, "pane.pan_zoom.view");
    assert!(thumb.contains_rect(view), "the part in view is inside the picture at 200%: {view:?} in {thumb:?}");
    assert!(view.width() < thumb.width() * 0.8, "and smaller than it");
    assert!(is_enabled(&w, "pane.pan_zoom.slider") && is_enabled(&w, "pane.pan_zoom.thumb"));
    // The picture draws the outline of the shape (a rectangle's outline is five points: four corners and the first again).
    let drawn = diacraft_ui_egui::work::work();
    assert!(drawn.thumb_builds >= 1 && drawn.thumb_points >= 5, "the lines of the picture were made from the shape's outline: {drawn:?}");
    assert!(drawn.thumb_painted >= 5, "and drawn: {drawn:?}");
    run_command(&mut w, "view.fitPage", json!({}));
    w.rest();
    let fitted = rect_of(&w, "pane.pan_zoom.view");
    assert!(fitted.width() > view.width() * 1.5, "the whole page on view is a larger rectangle: {fitted:?}");
    assert_eq!(w.state().ui_panics(), 0);
}

/// Dragging the rectangle pans the page by the same part of it, one `view.pan` for each step of the drag, and leaves the zoom alone.
#[test]
fn dragging_the_view_rectangle_pans_the_page() {
    let mut w = zoomed_window();
    let before = rect_of(&w, "pane.pan_zoom.view");
    let zoom = w.state().zoom();
    let page_before = w.state().window_state()["pageRect"].clone();
    drag(&mut w, before.center(), egui::vec2(18.0, 9.0));
    w.rest();
    let after = rect_of(&w, "pane.pan_zoom.view");
    let moved = after.center() - before.center();
    assert!((moved.x - 18.0).abs() < 1.5 && (moved.y - 9.0).abs() < 1.5, "the rectangle follows the pointer: {moved:?}");
    assert!((w.state().zoom() - zoom).abs() < 1e-9, "the zoom is not changed");
    let page_after = w.state().window_state()["pageRect"].clone();
    assert!(page_after[0].as_f64().unwrap() < page_before[0].as_f64().unwrap(), "the page went left under the view as the view went right");
    assert!(page_after[1].as_f64().unwrap() < page_before[1].as_f64().unwrap(), "and up as the view went down");
}

/// A press on the page outside the rectangle brings the middle of the view there.
#[test]
fn a_press_in_the_picture_outside_the_rectangle_centres_the_view_there() {
    let mut w = zoomed_window();
    let view = rect_of(&w, "pane.pan_zoom.view");
    let thumb = rect_of(&w, "pane.pan_zoom.thumb");
    let target = egui::pos2((view.center().x + thumb.right()) / 2.0, view.center().y - 5.0);
    assert!(!view.contains(target) && thumb.contains(target));
    click(&mut w, target);
    w.rest();
    let after = rect_of(&w, "pane.pan_zoom.view");
    assert!((after.center() - target).length() < 2.0, "{:?} is not {target:?}", after.center());
}

/// Dragging a corner of the rectangle outward zooms the view out about its middle, and inward zooms in; the zoom is a `view.zoom` and stays in range.
#[test]
fn dragging_a_corner_of_the_view_rectangle_zooms_about_its_middle() {
    let mut w = zoomed_window();
    let view = rect_of(&w, "pane.pan_zoom.view");
    let zoom = w.state().zoom();
    drag(&mut w, view.right_bottom(), egui::vec2(8.0, 6.0));
    w.rest();
    let wider = rect_of(&w, "pane.pan_zoom.view");
    assert!(w.state().zoom() < zoom - 0.05, "out: {} from {zoom}", w.state().zoom());
    assert!((wider.center() - view.center()).length() < 1.5, "about the same middle: {:?} {:?}", wider.center(), view.center());
    assert!(wider.width() > view.width() + 5.0);
    let zoom = w.state().zoom();
    drag(&mut w, wider.right_bottom(), egui::vec2(-20.0, -15.0));
    w.rest();
    assert!(w.state().zoom() > zoom + 0.1, "in");
    assert!(w.state().zoom() <= 4.0 && w.state().zoom() >= 0.1);
}

/// The slider sets the zoom, from 10% to 400%, as the status bar's does.
#[test]
fn the_slider_in_pan_and_zoom_sets_the_zoom() {
    let mut w = zoomed_window();
    let slider = rect_of(&w, "pane.pan_zoom.slider");
    click(&mut w, egui::pos2(slider.center().x, slider.center().y));
    w.rest();
    assert!((w.state().zoom() - 2.05).abs() < 0.03, "half way along 10% to 400% is 205%: {}", w.state().zoom());
    let slider = rect_of(&w, "pane.pan_zoom.slider");
    click(&mut w, egui::pos2(slider.left() + 7.0, slider.center().y));
    w.rest();
    assert!((w.state().zoom() - 0.10).abs() < 0.01, "{}", w.state().zoom());
}

// ---- Drawing Explorer --------------------------------------------------------------------------------------------------------------------

/// The demo drawing in a window: the pages Overview, Details and Notes, the background page Frame, a master and the root style.
fn demo_window(dir: &TempDir) -> Window {
    let path = fixture::write(dir.path(), "demo.vsdx", &fixture::demo_vsdx());
    let mut session = Session::new();
    session.execute("file.open", &json!({"path": path.to_str().unwrap()})).unwrap();
    let mut w = fast_window(DiacraftApp::new(session, None).with_inline_pictures(bundled_drawer()), [1280.0, 900.0]);
    w.rest();
    w
}

/// The tree lists the pages (foreground, then background), the masters and the styles of the document; a row for a page goes to that page, a branch folds
/// and unfolds, and the rows of masters and styles are for reading.
#[test]
fn drawing_explorer_lists_the_document_and_a_page_row_goes_to_the_page() {
    let dir = TempDir::new("explorer");
    let mut w = demo_window(&dir);
    open(&mut w, "drawing_explorer");
    w.rest();
    for branch in ["foreground_pages", "background_pages", "masters", "styles"] {
        assert!(w.state().element(&format!("explorer.branch.{branch}")).is_some(), "branch {branch}");
    }
    assert!(w.state().element("explorer.branch.fill_patterns").is_none(), "a branch with nothing in it is not listed");
    // Pages 1, 2 and 3 are foreground, page 4 (Frame) is the background.
    for id in 1..=4 {
        assert!(w.state().element(&format!("explorer.page.{id}")).is_some(), "page {id}");
    }
    let order: Vec<f32> = [1, 2, 3, 4].iter().map(|id| rect_of(&w, &format!("explorer.page.{id}")).top()).collect();
    assert!(order.windows(2).all(|p| p[0] < p[1]), "foreground pages first, then the background: {order:?}");
    assert!(rect_of(&w, "explorer.branch.background_pages").top() > rect_of(&w, "explorer.page.3").top());
    assert_eq!(w.state().window_state()["page"], json!(1));
    click_on(&mut w, "explorer.page.2");
    assert_eq!(w.state().window_state()["page"], json!(2), "a row goes to its page");
    // The masters and the styles start folded; a click on a branch unfolds it, and the rows under it are for reading.
    assert!(w.state().window_state()["explorerOpen"].as_array().unwrap().iter().all(|b| b != "masters"));
    click_on(&mut w, "explorer.branch.masters");
    assert!(w.state().window_state()["explorerOpen"].as_array().unwrap().iter().any(|b| b == "masters"));
    let rows_before = elements_named(&w, "explorer.page.").len();
    // Fold the foreground pages: their rows go.
    click_on(&mut w, "explorer.branch.foreground_pages");
    assert!(w.state().element("explorer.page.1").is_none());
    assert_eq!(elements_named(&w, "explorer.page.").len(), rows_before - 3);
    assert_eq!(w.state().ui_panics(), 0);
}

/// Branches fold and unfold by `window.explorerBranch`, by word or by title; a name that is none is refused in words.
#[test]
fn explorer_branches_fold_by_command_and_refuse_a_name_that_is_none() {
    let (mut w, _) = window_with_a_shape();
    open(&mut w, "drawing_explorer");
    run_command(&mut w, "window.explorerBranch", json!({"branch": "styles", "open": true}));
    assert!(w.state().explorer_branch_open(diacraft_ui_egui::Branch::Styles));
    run_command(&mut w, "window.explorerBranch", json!({"branch": "Styles"}));
    assert!(!w.state().explorer_branch_open(diacraft_ui_egui::Branch::Styles), "no `open` toggles");
    let words = w.state_mut().execute("window.explorerBranch", &json!({"branch": "nothing"})).unwrap_err().to_string();
    assert!(words.contains("no branch") && words.contains("foreground_pages"), "{words}");
}

// ---- Format Shape ------------------------------------------------------------------------------------------------------------------------

/// What `shape.formatting` says of shape `id` now.
fn look(w: &mut Window, id: u32) -> Value {
    let page = page_id(w);
    let answer = w.state_mut().execute("shape.formatting", &json!({"ids": [id], "page": page})).unwrap();
    answer["shapes"][0].clone()
}

/// The pane shows the shape's fill and line, and each change sent from it is one `format.shape` and one undo step: the picture of the page is the live
/// preview.
#[test]
fn format_shape_shows_the_selection_and_each_change_is_one_command_and_one_undo_step() {
    let (mut w, id) = window_with_a_shape();
    open(&mut w, "format_shape");
    w.rest();
    assert!(w.state().element("pane.format_shape").is_some());
    let before = look(&mut w, id);
    assert!(before["fill"].as_str().is_some_and(|f| f.starts_with('#')), "a dropped shape has a fill: {before}");
    for name in ["pane.format.fill.none", "pane.format.fill.solid", "pane.format.line.none", "pane.format.line.solid", "pane.format.line.weight"] {
        assert!(is_enabled(&w, name), "{name} works");
    }
    // No fill: one step.
    let revision = w.state().session().revision();
    click_on(&mut w, "pane.format.fill.none");
    assert!(w.state().session().revision() > revision);
    assert_eq!(look(&mut w, id)["fill"], json!("none"), "the fill is gone");
    // Solid fill puts a colour back (the last the shape had).
    click_on(&mut w, "pane.format.fill.solid");
    assert!(look(&mut w, id)["fill"].as_str().is_some_and(|f| f.starts_with('#')));
    undo(&mut w);
    assert_eq!(look(&mut w, id)["fill"], json!("none"), "one undo step took the colour away again");
    undo(&mut w);
    assert_eq!(look(&mut w, id)["fill"], before["fill"], "and one the fill back");
    // The line: none, and the weight typed in points.
    click_on(&mut w, "pane.format.line.none");
    assert_eq!(look(&mut w, id)["line"], json!("none"));
    assert!(!is_enabled(&w, "pane.format.line.weight"), "a line that is not drawn has no weight to set");
    undo(&mut w);
    type_in(&mut w, "pane.format.line.weight", "3 pt");
    assert!((look(&mut w, id)["weight"].as_f64().unwrap() - 3.0).abs() < 1e-6, "{}", look(&mut w, id));
    assert_eq!(shown_in(&w, "pane.format.line.weight"), "3 pt");
    click_on(&mut w, "pane.format.line.weight.up");
    assert!((look(&mut w, id)["weight"].as_f64().unwrap() - 3.25).abs() < 1e-6, "a quarter of a point a step");
    assert_eq!(w.state().ui_panics(), 0);
}

/// With shapes that differ the controls show nothing, and a change goes to all of them.
#[test]
fn format_shape_with_several_shapes_is_blank_where_they_differ_and_a_change_goes_to_all() {
    let mut w = edit_window([1280.0, 900.0]);
    drag_tile_to(&mut w, "Rectangle", 3.0, 6.0);
    drag_tile_to(&mut w, "Diamond", 7.0, 6.0);
    let ids = shape_ids(&w);
    let page = page_id(&w);
    run_command(&mut w, "shape.format", json!({"ids": [ids[0]], "line": "#2f5597", "weight": 2.0, "page": page}));
    w.state_mut().select(&ids);
    open(&mut w, "format_shape");
    w.rest();
    assert_eq!(shown_in(&w, "pane.format.line.weight"), "", "the weights differ");
    click_on(&mut w, "pane.format.fill.none");
    for id in &ids {
        assert_eq!(look(&mut w, *id)["fill"], json!("none"), "shape {id}");
    }
    undo(&mut w);
    assert!(look(&mut w, ids[0])["fill"] != json!("none") && look(&mut w, ids[1])["fill"] != json!("none"), "one step for both");
}

/// What no command sets is drawn quiet: the Effects tab keeps five such rows after the Shadow group (gave the fill its gradient, its pattern and its transparency, and
/// the Effects tab its shadow); with nothing selected every control is quiet.
#[test]
fn format_shape_draws_quiet_what_no_command_sets_and_everything_with_no_selection() {
    let (mut w, _) = window_with_a_shape();
    open(&mut w, "format_shape");
    w.rest();
    for name in ["pane.format.fill.gradient", "pane.format.fill.pattern", "pane.format.fill.transparency"] {
        assert!(is_enabled(&w, name), "{name} works: a command sets it");
    }
    click_on(&mut w, "pane.format.tab.effects");
    assert!(w.state().element("pane.format.effects.0").is_some(), "the Shadow group heads the tab");
    for n in 1..=5 {
        assert!(!is_enabled(&w, &format!("pane.format.effects.{n}")), "effect {n} is listed, and quiet");
    }
    assert!(w.state().element("pane.format.effects.6").is_none(), "and there are five after the Shadow group");
    click_on(&mut w, "pane.format.tab.fill_line");
    w.state_mut().select(&[]);
    w.rest();
    for name in [
        "pane.format.fill.none",
        "pane.format.fill.solid",
        "pane.format.fill.gradient",
        "pane.format.fill.pattern",
        "pane.format.line.none",
        "pane.format.line.solid",
    ] {
        assert!(!is_enabled(&w, name), "{name} is quiet with nothing selected");
    }
}

/// F3 and the launcher of the Shape Styles group open Format Shape.
#[test]
fn and_the_shape_styles_launcher_open_format_shape() {
    let (mut w, _) = window_with_a_shape();
    key(&mut w, Key::F3, Modifiers::NONE);
    assert_eq!(panes(&w), json!([["format_shape"]]));
    key(&mut w, Key::F3, Modifiers::NONE);
    assert_eq!(panes(&w), json!([["format_shape"]]), "F3 opens the pane and does not close it again");
    click_on(&mut w, "pane.format_shape.close");
    assert_eq!(panes(&w), json!([]));
    press(&mut w, "ribbon.Home.ObjectFormatDialog");
    assert_eq!(panes(&w), json!([["format_shape"]]));
}

// ---- the Text dialog ---------------------------------------------------------------------------------------------------------------------

/// The launchers of the Font and Paragraph groups and F11 open the Text dialog on the tab of the group; OK sends what was changed as one `text.format`,
/// Cancel sends nothing.
#[test]
fn the_font_and_paragraph_launchers_and_f11_open_the_text_dialog_and_ok_sends_one_command() {
    let (mut w, id) = window_with_a_shape();
    press(&mut w, "ribbon.Home.FontDialog");
    assert_eq!(w.state().window_state()["textDialog"], json!("Font"));
    assert!(is_enabled(&w, "textdialog.bold") && is_enabled(&w, "textdialog.ok"));
    let revision = w.state().session().revision();
    click_on(&mut w, "textdialog.bold");
    click_on(&mut w, "textdialog.underline");
    assert_eq!(w.state().session().revision(), revision, "nothing is sent until OK");
    click_on(&mut w, "textdialog.ok");
    assert!(w.state().window_state()["textDialog"].is_null(), "OK closes it");
    assert_eq!(w.state().session().revision(), revision + 1, "one command for both changes");
    let after = look(&mut w, id);
    assert!(after["bold"] == json!(true) && after["underline"] == json!(true), "{after}");
    undo(&mut w);
    assert_eq!(look(&mut w, id)["bold"], json!(false), "one undo step");
    // Cancel.
    press(&mut w, "ribbon.Home.ParagraphDialog");
    assert_eq!(w.state().window_state()["textDialog"], json!("Paragraph"));
    let revision = w.state().session().revision();
    click_on(&mut w, "textdialog.align.right");
    click_on(&mut w, "textdialog.cancel");
    assert!(w.state().window_state()["textDialog"].is_null());
    assert_eq!(w.state().session().revision(), revision, "Cancel sends nothing");
    // F11.
    key(&mut w, Key::F11, Modifiers::NONE);
    assert_eq!(w.state().window_state()["textDialog"], json!("Font"));
    click_on(&mut w, "textdialog.tab.Text Block");
    assert_eq!(w.state().window_state()["textDialog"], json!("Text Block"));
    for margin in ["left", "right", "top", "bottom"] {
        assert!(!is_enabled(&w, &format!("textdialog.margin.{margin}")), "the {margin} margin is quiet: no command sets it");
    }
    click_on(&mut w, "textdialog.valign.top");
    click_on(&mut w, "textdialog.ok");
    assert_eq!(look(&mut w, id)["valign"], json!("top"));
    assert_eq!(w.state().ui_panics(), 0);
}

/// The paragraph alignment and a size typed in the Font tab go in the same one command.
#[test]
fn the_text_dialog_sends_a_size_and_an_alignment_together_and_leaves_what_it_did_not_touch() {
    let (mut w, id) = window_with_a_shape();
    let page = page_id(&w);
    run_command(&mut w, "text.format", json!({"ids": [id], "page": page, "italic": true}));
    run_command(&mut w, "window.textDialog", json!({"tab": "Font"}));
    w.rest();
    type_in(&mut w, "textdialog.size", "20");
    click_on(&mut w, "textdialog.ok");
    let after = look(&mut w, id);
    assert!((after["size"].as_f64().unwrap() - 20.0).abs() < 1e-6, "{after}");
    assert_eq!(after["italic"], json!(true), "what was not changed is as it was");
    assert!(w.state_mut().execute("window.textDialog", &json!({"tab": "Nothing"})).is_err());
    run_command(&mut w, "window.textDialog", json!({"close": true}));
}

// ---- the dock's size and the commands behind its buttons ---------------------------------------------------------------------------------

/// The dock opens 280 points wide (what the window was when it first opened does not set it), and at a narrow window it is held to 45% of the width with
/// every control of the pane inside it.
#[test]
fn the_dock_is_280_points_wide_and_narrow_windows_keep_the_controls_inside_it() {
    let mut w = edit_window([1280.0, 900.0]);
    drag_tile_to(&mut w, "Rectangle", 4.0, 5.0);
    open(&mut w, "size_position");
    assert!((rect_of(&w, "panes").width() - 280.0).abs() < 1.0, "{:?}", rect_of(&w, "panes"));
    let mut narrow = edit_window([480.0, 700.0]);
    let page = page_id(&narrow);
    let made = narrow.state_mut().execute("shape.drop", &json!({"master": "rectangle", "page": page, "x": 4.0, "y": 4.0})).unwrap();
    narrow.state_mut().select(&[made["id"].as_u64().unwrap() as u32]);
    open(&mut narrow, "size_position");
    narrow.rest();
    let dock = rect_of(&narrow, "panes");
    assert!(dock.width() <= 480.0 * 0.45 + 1.0 && dock.width() >= 200.0, "{dock:?}");
    for name in [
        "pane.size.x",
        "pane.size.y",
        "pane.size.width",
        "pane.size.height",
        "pane.size.angle",
        "pane.size.pin",
        "pane.size_position.close",
        "pane.size_position.menu",
    ] {
        let r = rect_of(&narrow, name);
        assert!(dock.contains_rect(r), "{name} {r:?} is inside the dock {dock:?}");
    }
    let canvas = rect_of(&narrow, "canvas");
    assert!(canvas.right() <= dock.left() + 1.0, "the pane does not overlap the drawing area");
    assert_eq!(narrow.state().ui_panics(), 0);
}

/// The buttons of the dock are commands: a tab is `window.paneTab` and the menu's moves are `window.paneMove`; a pane that is not open or a name that is none
/// is refused in words.
#[test]
fn the_tabs_and_moves_of_the_dock_are_commands_and_refuse_a_pane_that_is_not_open() {
    let (mut w, _) = window_with_a_shape();
    open(&mut w, "size_position");
    open(&mut w, "pan_zoom");
    open(&mut w, "drawing_explorer");
    assert_eq!(panes(&w), json!([["size_position"], ["pan_zoom"], ["drawing_explorer"]]));
    run_command(&mut w, "window.paneMove", json!({"pane": "drawing_explorer", "up": true}));
    assert_eq!(panes(&w), json!([["size_position"], ["drawing_explorer"], ["pan_zoom"]]));
    run_command(&mut w, "window.paneMove", json!({"pane": "size_position", "up": false}));
    assert_eq!(panes(&w), json!([["drawing_explorer"], ["size_position"], ["pan_zoom"]]));
    run_command(&mut w, "window.paneDock", json!({"pane": "pan_zoom", "with": "size_position"}));
    assert_eq!(panes(&w), json!([["drawing_explorer"], ["size_position", "pan_zoom"]]));
    run_command(&mut w, "window.paneTab", json!({"pane": "size_position"}));
    assert!(w.state().element("pane.size.x").is_some(), "the tab comes to the top");
    let words = w.state_mut().execute("window.paneTab", &json!({"pane": "format_shape"})).unwrap_err().to_string();
    assert!(words.contains("not open"), "{words}");
    assert!(w.state_mut().execute("window.paneMove", &json!({"pane": "format_shape", "up": true})).is_err());
    assert!(w.state_mut().execute("window.paneMove", &json!({"pane": "pan_zoom"})).is_err(), "`up` is needed");
    assert!(w.state_mut().execute("window.paneTab", &json!({"pane": "nothing"})).is_err());
}

// ---- leaving a field (H1, H2 of the review) ---------------------------------------------------------------------------------

/// Gives the field `name` the focus and types `text` in it, with no Enter: the field is let go of in the way the test goes on to do.
fn type_only(w: &mut Window, name: &str, text: &str) {
    click_on(w, name);
    key(w, Key::A, ctrl());
    type_text(w, text);
    w.rest();
}

/// What `shape.placement` says of shape `id` now.
fn placement(w: &mut Window, id: u32) -> Value {
    let page = page_id(w);
    w.state_mut().execute("shape.placement", &json!({"ids": [id], "page": page})).unwrap()["shapes"][0].clone()
}

/// Two shapes, the rectangle (3, 6) and the diamond (7, 6), and the rectangle selected with Size & Position open.
fn two_shapes_and_the_pane() -> (Window, Vec<u32>) {
    let mut w = edit_window([1280.0, 900.0]);
    drag_tile_to(&mut w, "Rectangle", 3.0, 6.0);
    drag_tile_to(&mut w, "Diamond", 7.0, 6.0);
    let ids = shape_ids(&w);
    w.state_mut().select(&[ids[0]]);
    open(&mut w, "size_position");
    w.rest();
    (w, ids)
}

/// A width typed and not ended, then a press on another shape: the width is the first shape's (the one the field showed), the second shape is not changed.
#[test]
fn a_value_typed_and_left_by_a_press_on_another_shape_goes_to_the_shape_it_was_typed_for() {
    let (mut w, ids) = two_shapes_and_the_pane();
    let second = frame_of(&w, ids[1]);
    type_only(&mut w, "pane.size.width", "3");
    click_at(&mut w, 7.0, 6.0);
    w.rest();
    assert_eq!(w.state().selection(), [ids[1]], "the press selected the other shape");
    assert!(near(frame_of(&w, ids[0]).2, 3.0), "the width went to the shape it was typed for: {:?}", frame_of(&w, ids[0]));
    assert_eq!(frame_of(&w, ids[1]), second, "and the shape that was pressed is as it was");
    assert_eq!(shown_in(&w, "pane.size.width"), "1.5 in", "the box shows the shape now selected");
    assert_eq!(w.state().pane_edits_open(), 0);
}

/// A width typed and not ended, then a press on the empty page (which clears the selection): the value is not lost.
#[test]
fn a_value_typed_and_left_by_a_press_on_the_empty_page_is_not_lost() {
    let (mut w, ids) = two_shapes_and_the_pane();
    type_only(&mut w, "pane.size.width", "3");
    click_at(&mut w, 5.0, 2.5);
    w.rest();
    assert!(w.state().selection().is_empty(), "the press cleared the selection");
    assert!(near(frame_of(&w, ids[0]).2, 3.0), "{:?}", frame_of(&w, ids[0]));
    undo(&mut w);
    assert!(near(frame_of(&w, ids[0]).2, 1.5), "one undo step");
}

/// A value typed and not ended, then a press in another field: the first value is sent (the press takes the focus before the first field lets go of it).
#[test]
fn a_value_typed_and_left_by_a_press_in_another_field_is_sent() {
    let (mut w, ids) = two_shapes_and_the_pane();
    type_only(&mut w, "pane.size.x", "6");
    click_on(&mut w, "pane.size.y");
    w.rest();
    assert!(near(pin_of(&w, ids[0]).0, 6.0), "X was sent: {:?}", pin_of(&w, ids[0]));
    assert!(near(pin_of(&w, ids[0]).1, 6.0), "and Y, which was not typed in, is as it was");
    assert_eq!(w.state().pane_edits_open(), 1, "the second field has the focus");
    key(&mut w, Key::Enter, Modifiers::NONE);
    assert!(near(pin_of(&w, ids[0]).1, 6.0), "an Enter in Y with nothing typed sends nothing");
}

/// Tab leaves a field as a click does: the value is sent, and the next field has the focus.
#[test]
fn tab_sends_the_value_typed_and_moves_to_the_next_field() {
    let (mut w, ids) = two_shapes_and_the_pane();
    type_only(&mut w, "pane.size.x", "6");
    key(&mut w, Key::Tab, Modifiers::NONE);
    assert!(near(pin_of(&w, ids[0]).0, 6.0), "{:?}", pin_of(&w, ids[0]));
    assert_eq!(w.state().pane_edits_open(), 1, "the next field has the focus");
    assert_eq!(shown_in(&w, "pane.size.x"), "6 in");
}

/// Esc leaves the old value in the box and sends nothing.
#[test]
fn esc_leaves_the_old_value_and_sends_nothing() {
    let (mut w, ids) = two_shapes_and_the_pane();
    let revision = w.state().session().revision();
    type_only(&mut w, "pane.size.x", "9");
    assert_eq!(w.state().session().revision(), revision, "typing sends nothing by itself");
    assert_eq!(w.state().pane_edits_open(), 1);
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert_eq!(w.state().session().revision(), revision, "nothing was sent");
    assert_eq!(shown_in(&w, "pane.size.x"), "3 in", "the old value is in the box");
    assert!(near(pin_of(&w, ids[0]).0, 3.0));
    assert_eq!(w.state().pane_edits_open(), 0);
    assert_eq!(w.state().selection(), [ids[0]], "the Escape was the box's: the selection is kept");
    key(&mut w, Key::Enter, Modifiers::NONE);
    assert_eq!(w.state().session().revision(), revision);
}

/// A box that was only passed through (a click, a Tab) sends nothing, and shows what the drawing says while it has the focus: a command run meanwhile, here
/// a turn from the ribbon's own command, is not taken back by the box.
#[test]
fn a_box_with_the_focus_and_nothing_typed_follows_the_drawing_and_never_undoes_a_command() {
    let (mut w, ids) = two_shapes_and_the_pane();
    click_on(&mut w, "pane.size.angle");
    assert_eq!(shown_in(&w, "pane.size.angle"), "0 deg");
    run_command(&mut w, "arrange.rotate", json!({"by": 90}));
    w.rest();
    assert_eq!(shown_in(&w, "pane.size.angle"), "90 deg", "the box with the focus shows the new angle");
    key(&mut w, Key::Enter, Modifiers::NONE);
    assert!((placement(&mut w, ids[0])["angle"].as_f64().unwrap() - 90.0).abs() < 1e-6, "the turn is still there after the Enter");
    assert_eq!(shown_in(&w, "pane.size.angle"), "90 deg");
}

/// Undo while a field has the focus with nothing typed: the undo stands, the redo is kept, and the box shows the angle the drawing has.
#[test]
fn undo_with_a_field_focused_and_untouched_does_nothing_more_and_keeps_the_redo() {
    let (mut w, ids) = two_shapes_and_the_pane();
    type_in(&mut w, "pane.size.angle", "30");
    assert!((placement(&mut w, ids[0])["angle"].as_f64().unwrap() - 30.0).abs() < 1e-6);
    click_on(&mut w, "pane.size.angle");
    undo(&mut w);
    w.rest();
    assert_eq!(shown_in(&w, "pane.size.angle"), "0 deg");
    key(&mut w, Key::Enter, Modifiers::NONE);
    assert!(placement(&mut w, ids[0])["angle"].as_f64().unwrap().abs() < 1e-6, "the undo stands");
    assert_eq!(w.state().session().history().unwrap().redo_len(), 1, "the redo was not thrown away by a value the person did not type");
}

/// A drag on the page while a field has the focus with nothing typed: the box does not send its old value back afterwards.
#[test]
fn a_drag_on_the_page_while_a_field_is_focused_and_untouched_is_not_taken_back() {
    let (mut w, ids) = two_shapes_and_the_pane();
    click_on(&mut w, "pane.size.x");
    drag_with(&mut w, (3.0, 6.0), (5.0, 6.0), Modifiers::NONE);
    w.rest();
    key(&mut w, Key::Enter, Modifiers::NONE);
    w.rest();
    assert!(near(pin_of(&w, ids[0]).0, 5.0), "the shape stays where the drag put it: {:?}", pin_of(&w, ids[0]));
    assert_eq!(shown_in(&w, "pane.size.x"), "5 in");
}

/// A Tab through the fields of a line: End Y is typed, Tab goes to Length, and an Enter there sends nothing, so the length the new end made is kept.
#[test]
fn tabbing_into_length_after_an_end_does_not_send_the_old_length_back() {
    let mut w = edit_window([1280.0, 900.0]);
    let page = page_id(&w);
    let made = w.state_mut().execute("connector.add", &json!({"from": {"x": 2.0, "y": 2.0}, "to": {"x": 5.0, "y": 6.0}, "page": page})).unwrap();
    let line = made["id"].as_u64().unwrap() as u32;
    w.state_mut().select(&[line]);
    open(&mut w, "size_position");
    w.rest();
    type_only(&mut w, "pane.size.end_y", "8");
    key(&mut w, Key::Tab, Modifiers::NONE);
    let length = shown_in(&w, "pane.size.length");
    assert_ne!(length, "5 in", "the new end made the line longer: {length}");
    key(&mut w, Key::Enter, Modifiers::NONE);
    assert_eq!(shown_in(&w, "pane.size.length"), length, "an Enter in Length with nothing typed leaves the length as the end made it");
    assert_eq!(shown_in(&w, "pane.size.end_y"), "8 in");
}

/// The weight of Format Shape is a field of the same kind: typed and left by a press on another shape it goes to the shape it showed.
#[test]
fn a_weight_typed_and_left_by_a_press_on_another_shape_goes_to_the_shape_it_showed() {
    let mut w = edit_window([1280.0, 900.0]);
    drag_tile_to(&mut w, "Rectangle", 3.0, 6.0);
    drag_tile_to(&mut w, "Diamond", 7.0, 6.0);
    let ids = shape_ids(&w);
    w.state_mut().select(&[ids[0]]);
    open(&mut w, "format_shape");
    w.rest();
    let before = look(&mut w, ids[1])["weight"].clone();
    type_only(&mut w, "pane.format.line.weight", "4 pt");
    click_at(&mut w, 7.0, 6.0);
    w.rest();
    assert_eq!(w.state().selection(), [ids[1]]);
    assert!((look(&mut w, ids[0])["weight"].as_f64().unwrap() - 4.0).abs() < 1e-6, "the weight went to the first shape");
    assert_eq!(look(&mut w, ids[1])["weight"], before, "and the shape pressed is as it was");
}

// ---- refusals and bounds ---------------------------------------------------------------------------------------------------------------

/// The demo of a drawing of `bytes` in a window.
fn window_of(dir: &TempDir, bytes: &[u8]) -> Window {
    let path = fixture::write(dir.path(), "drawing.vsdx", bytes);
    let mut session = Session::new();
    session.execute("file.open", &json!({"path": path.to_str().unwrap()})).unwrap();
    let mut w = fast_window(DiacraftApp::new(session, None).with_inline_pictures(bundled_drawer()), [1280.0, 900.0]);
    w.rest();
    w
}

/// The engine refuses a size for a shape whose Width is guarded: the words are told under the field and nothing changes; they go when the selection does.
#[test]
fn a_value_the_engine_refuses_is_told_under_the_field_and_the_words_go_with_the_selection() {
    let dir = TempDir::new("guarded");
    let mut w = window_of(&dir, &fixture::guarded_width_vsdx());
    w.state_mut().select(&[1]);
    open(&mut w, "size_position");
    w.rest();
    let revision = w.state().session().revision();
    type_in(&mut w, "pane.size.width", "3");
    let words = w.state().pane_field_refusal("pane.size.width").expect("the refusal is told").to_owned();
    assert!(!words.is_empty() && words.to_lowercase().contains("guard"), "{words}");
    assert_eq!(w.state().session().revision(), revision, "nothing changed");
    assert_eq!(w.state().notice(), Some(words.as_str()), "and the status bar says it too");
    w.state_mut().select(&[]);
    w.rest();
    assert!(
        w.state().pane_field_refusal("pane.size.width").is_none(),
        "the words about shape 1 are not left under the field when it is not selected"
    );
}

/// A number past what the engine takes is refused in words and nothing is sent; a hostile coordinate is shown rounded.
#[test]
fn a_place_or_size_past_the_engines_bound_is_refused_and_a_huge_coordinate_is_shown_rounded() {
    let (mut w, ids) = two_shapes_and_the_pane();
    let revision = w.state().session().revision();
    type_in(&mut w, "pane.size.width", "5000000");
    let words = w.state().pane_field_refusal("pane.size.width").expect("refused").to_owned();
    assert!(words.contains("too large") && words.contains("1000000"), "{words}");
    type_in(&mut w, "pane.size.x", "50000000");
    assert!(w.state().pane_field_refusal("pane.size.x").is_some_and(|w| w.contains("too large")));
    assert_eq!(w.state().session().revision(), revision, "nothing was sent");
    assert_eq!(frame_of(&w, ids[0]).2, 1.5);
    // A coordinate in a file that is far past anything: the box shows it as a short number.
    let page = page_id(&w);
    w.state_mut().execute("shape.move", &json!({"ids": [ids[0]], "dx": 900_000.0, "dy": 0.0, "page": page})).unwrap();
    w.rest();
    assert!(shown_in(&w, "pane.size.x").len() < 20, "{}", shown_in(&w, "pane.size.x"));
}

/// The title is the name that `shape.placement` gives: `NameU` when the shape has no `Name`, `Sheet.<id>` when it has neither.
#[test]
fn the_title_is_the_name_placement_gives() {
    use egui_kittest::kittest::Queryable;
    let (mut w, ids) = two_shapes_and_the_pane();
    let name = placement(&mut w, ids[0])["name"].as_str().unwrap().to_owned();
    w.get_by_label(&format!("Size & Position - {name}"));
}

/// Over the 4,096 shapes one engine command takes, the fields are quiet and say why; nothing is sent in part.
#[test]
fn a_selection_over_what_one_command_takes_is_not_half_applied() {
    let dir = TempDir::new("many");
    let mut w = window_of(&dir, &fixture::detailed_vsdx(4_200, 2));
    let all: Vec<u32> = (1..=4_200).collect();
    w.state_mut().select(&all);
    open(&mut w, "size_position");
    w.rest();
    for name in ["pane.size.x", "pane.size.y", "pane.size.width", "pane.size.height", "pane.size.angle"] {
        assert!(!is_enabled(&w, name), "{name} is quiet with 4,200 shapes selected");
    }
    let few: Vec<u32> = (1..=4_096).collect();
    w.state_mut().select(&few);
    w.rest();
    assert!(is_enabled(&w, "pane.size.x"), "4,096 are sent in one command");
}

// ---- the Text dialog on characters selected in an open text box ------------------------------------------------------------------

/// Opens the label's text box (F2) and selects the characters `from..to`.
fn select_chars(w: &mut Window, from: usize, to: usize) {
    click_at(w, 3.0, 6.0);
    key(w, Key::F2, Modifiers::NONE);
    key(w, Key::ArrowUp, Modifiers::NONE);
    key(w, Key::Home, Modifiers::NONE);
    for _ in 0..from {
        key(w, Key::ArrowRight, Modifiers::NONE);
    }
    for _ in from..to {
        key(w, Key::ArrowRight, Modifiers::SHIFT);
    }
    assert_eq!(w.state().edited_text_selection(), Some((from, to)));
}

/// `shape.formatting` of the characters `a..b` of the label (shape 1).
fn range_look(w: &mut Window, a: usize, b: usize) -> Value {
    let page = page_id(w);
    w.state_mut().execute("shape.formatting", &json!({"ids": [1], "page": page, "start": a, "end": b})).unwrap()["shapes"][0].clone()
}

/// With characters selected in the open box the Text dialog formats those characters, in one `text.format` with their range; the box stays open with the
/// same selection, and the characters outside the range are as they were.
#[test]
fn the_text_dialog_formats_the_characters_selected_in_the_open_box() {
    let dir = TempDir::new("dialog-range");
    let (mut w, _) = crate::text_edit::labelled_window(&dir, |app| app);
    select_chars(&mut w, 2, 4);
    let before_outside = range_look(&mut w, 0, 2);
    let revision = w.state().session().revision();
    key(&mut w, Key::F11, Modifiers::NONE);
    assert_eq!(w.state().window_state()["textDialog"], json!("Font"));
    click_on(&mut w, "textdialog.underline");
    click_on(&mut w, "textdialog.ok");
    assert!(w.state().window_state()["textDialog"].is_null(), "OK closes the dialog");
    assert!(w.state().window_state()["editingText"].as_bool().unwrap(), "and the box stays open");
    assert_eq!(w.state().edited_text_selection(), Some((2, 4)), "with the same characters selected");
    assert_eq!(range_look(&mut w, 2, 4)["underline"], json!(true), "the characters selected are underlined");
    assert_eq!(range_look(&mut w, 0, 2)["underline"], before_outside["underline"], "the others are not");
    assert!(w.state().session().revision() > revision);
    assert_eq!(w.state().ui_panics(), 0);
}

// ----: the keys of the last frame, finishing before the File view acts, a box that follows the drawing -------------------------

/// A key event, pressed or released, and no frame.
fn push_key(w: &mut Window, key: Key, pressed: bool) {
    w.input_mut().events.push(egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE });
}

/// Text and an Enter in one frame, as a fast typist or a paste with a newline gives them, then the frame's release.
fn text_and_enter_in_one_frame(w: &mut Window, text: &str) {
    w.input_mut().events.push(egui::Event::Text(text.to_owned()));
    push_key(w, Key::Enter, true);
    w.step();
    push_key(w, Key::Enter, false);
    w.step();
    w.rest();
}

/// The text of the box and the Enter that ends it come in the same frame: all the text is sent.
#[test]
fn text_and_an_enter_in_one_frame_send_all_of_the_text() {
    let (mut w, ids) = two_shapes_and_the_pane();
    click_on(&mut w, "pane.size.width");
    key(&mut w, Key::A, ctrl());
    text_and_enter_in_one_frame(&mut w, "2");
    assert!(near(frame_of(&w, ids[0]).2, 2.0), "the width typed with the Enter is sent: {:?}", frame_of(&w, ids[0]));
}

/// A value typed in two frames, the second with the Enter: "1" then "2" and Enter is 12, not 1.
#[test]
fn a_value_typed_in_two_frames_the_last_with_the_enter_is_sent_whole() {
    let (mut w, ids) = two_shapes_and_the_pane();
    click_on(&mut w, "pane.size.width");
    key(&mut w, Key::A, ctrl());
    type_text(&mut w, "1");
    text_and_enter_in_one_frame(&mut w, "2");
    assert!(near(frame_of(&w, ids[0]).2, 12.0), "{:?}", frame_of(&w, ids[0]));
}

/// A click on the File tab ends the edit before the File view opens, and a drawing opened from there takes none of it: the value went to the drawing it
/// was typed for, and the new drawing's shape of the same id is as the file made it.
#[test]
fn a_value_typed_then_a_click_on_file_and_an_open_lands_in_the_old_drawing_and_not_in_the_new() {
    let dir = TempDir::new("typed-then-open");
    let (mut w, id) = window_with_a_shape();
    assert_eq!(id, 1, "the new drawing's first shape is shape 1, as the other drawing's");
    open(&mut w, "size_position");
    w.rest();
    type_only(&mut w, "pane.size.width", "3");
    click_on(&mut w, "tab.File");
    assert!(near(frame_of(&w, id).2, 3.0), "the click on File sent the value, before the File view opened: {:?}", frame_of(&w, id));
    let path = fixture::write(dir.path(), "other.vsdx", &fixture::detailed_vsdx(2, 2));
    run_command(&mut w, "file.open", json!({"path": path.to_str().unwrap()}));
    w.rest();
    assert_eq!(w.state().pane_edits_open(), 0, "nothing waits");
    assert!(near(frame_of(&w, 1).2, 0.2), "shape 1 of the new drawing is as its file made it: {:?}", frame_of(&w, 1));
    assert!(!w.state().is_dirty(), "and the new drawing has no change in it");
}

/// The title bar's Save, clicked while a value waits in a field, saves with the value in it.
#[test]
fn a_value_typed_then_a_click_on_save_is_in_the_saved_file() {
    let dir = TempDir::new("typed-then-save");
    let path = dir.file("saved.vsdx");
    let asked = Arc::new(Mutex::new(Vec::new()));
    let mut w = crate::saving::window_with_save(&path, &asked);
    drag_tile_to(&mut w, "Rectangle", 4.0, 5.0);
    let id = shape_ids(&w)[0];
    open(&mut w, "size_position");
    w.rest();
    type_only(&mut w, "pane.size.width", "3");
    click_on(&mut w, "qat.Save");
    w.rest();
    assert!(path.exists(), "the drawing was saved");
    let mut saved = Session::new();
    saved.execute("file.open", &json!({"path": path.to_str().unwrap()})).unwrap();
    let width = saved.execute("shape.placement", &json!({"ids": [id]})).unwrap()["shapes"][0]["width"].as_f64().unwrap();
    assert!((width - 3.0).abs() < 1e-6, "the saved file holds the width that was waiting: {width}");
    assert!(!w.state().is_dirty(), "and the drawing is saved, not saved and then changed");
}

/// The same from the File view: the click on File sends the value, and the Save there writes it.
#[test]
fn a_value_typed_then_a_click_on_file_and_save_is_in_the_saved_file() {
    let dir = TempDir::new("typed-file-save");
    let path = dir.file("saved.vsdx");
    let asked = Arc::new(Mutex::new(Vec::new()));
    let mut w = crate::saving::window_with_save(&path, &asked);
    drag_tile_to(&mut w, "Rectangle", 4.0, 5.0);
    let id = shape_ids(&w)[0];
    open(&mut w, "size_position");
    w.rest();
    type_only(&mut w, "pane.size.width", "3");
    click_on(&mut w, "tab.File");
    click_on(&mut w, "backstage.Save");
    w.rest();
    let mut saved = Session::new();
    saved.execute("file.open", &json!({"path": path.to_str().unwrap()})).unwrap();
    let width = saved.execute("shape.placement", &json!({"ids": [id]})).unwrap()["shapes"][0]["width"].as_f64().unwrap();
    assert!((width - 3.0).abs() < 1e-6, "{width}");
}

/// Undo clicked while a value waits: the value goes in first and the Undo takes it back, so the shape is there and as it was dropped.
#[test]
fn a_value_typed_then_a_click_on_undo_is_taken_back_and_the_step_before_stands() {
    let (mut w, ids) = two_shapes_and_the_pane();
    type_only(&mut w, "pane.size.width", "3");
    click_on(&mut w, "qat.Undo");
    w.rest();
    assert_eq!(shape_ids(&w), ids, "both shapes are still there: the Undo took back the width and nothing else");
    assert!(near(frame_of(&w, ids[0]).2, 1.5), "{:?}", frame_of(&w, ids[0]));
}

/// A Tab from a field that sent its value to the next: the next one shows the drawing's value when it changes (the end made the line longer), all of it
/// selected, so that what is typed replaces it.
#[test]
fn a_box_that_follows_the_drawing_has_all_its_text_selected_again() {
    let mut w = edit_window([1280.0, 900.0]);
    let page = page_id(&w);
    let made = w.state_mut().execute("connector.add", &json!({"from": {"x": 2.0, "y": 2.0}, "to": {"x": 5.0, "y": 6.0}, "page": page})).unwrap();
    let line = made["id"].as_u64().unwrap() as u32;
    w.state_mut().select(&[line]);
    open(&mut w, "size_position");
    w.rest();
    type_only(&mut w, "pane.size.end_y", "8");
    key(&mut w, Key::Tab, Modifiers::NONE);
    assert_ne!(shown_in(&w, "pane.size.length"), "5 in");
    type_text(&mut w, "10");
    key(&mut w, Key::Enter, Modifiers::NONE);
    assert_eq!(shown_in(&w, "pane.size.length"), "10 in", "the typed 10 replaced the whole of the length");
}

/// X typed for shapes at 300 different places (the engine's `shape.place`) puts every one of them at it, in one command and one undo step: X is not
/// quiet for many places (the old rule was 256 steps of a batch), each shape keeps its own Y, and one Undo gives every shape its place back.
#[test]
fn x_typed_for_shapes_at_300_different_places_puts_them_all_at_it_in_one_undo_step() {
    let dir = TempDir::new("scattered");
    let mut w = window_of(&dir, &fixture::scattered_vsdx(300));
    let all: Vec<u32> = (1..=300).collect();
    w.state_mut().select(&all);
    open(&mut w, "size_position");
    w.rest();
    assert!(is_enabled(&w, "pane.size.x") && is_enabled(&w, "pane.size.y"), "300 places across are not too many");
    assert_eq!(shown_in(&w, "pane.size.x"), "", "the shapes differ across");
    let before: Vec<(f64, f64, f64, f64)> = all.iter().map(|id| frame_of(&w, *id)).collect();
    let revision = w.state().session().revision();
    type_in(&mut w, "pane.size.x", "5");
    assert_eq!(w.state().session().revision(), revision + 1, "one command");
    for (id, was) in all.iter().zip(&before) {
        let now = frame_of(&w, *id);
        assert!(near(now.0, 5.0), "shape {id} is at 5 (its pin is at its lower left, so its frame starts there): {now:?}");
        assert!(near(now.1, was.1) && near(now.2, was.2) && near(now.3, was.3), "shape {id} keeps its Y and its size: {now:?} from {was:?}");
    }
    assert_eq!(shown_in(&w, "pane.size.x"), "5 in", "and the shapes agree now");
    undo(&mut w);
    let after: Vec<(f64, f64, f64, f64)> = all.iter().map(|id| frame_of(&w, *id)).collect();
    assert_eq!(after, before, "one Undo gives every shape its place back");
    assert_eq!(w.state().pane_field_refusal("pane.size.x"), None);
}

/// A shape `shape.place` cannot place (a line with an end a million inches out would be carried past the page's reach) is left where it is and counted in the
/// answer; the others are placed; the words are told under the field the way a refusal is, and go when the selection does.
#[test]
fn a_shape_the_engine_could_not_place_is_told_under_the_field_and_the_others_are_placed() {
    let (mut w, ids) = two_shapes_and_the_pane();
    let page = page_id(&w);
    let made =
        w.state_mut().execute("connector.add", &json!({"from": {"x": 0.0, "y": 2.0}, "to": {"x": 999_999.0, "y": 2.0}, "page": page})).unwrap();
    let line = made["id"].as_u64().unwrap() as u32;
    w.state_mut().select(&[ids[0], line]);
    w.rest();
    let box_before = frame_of(&w, ids[0]);
    type_in(&mut w, "pane.size.x", "900000");
    let words = w.state().pane_field_refusal("pane.size.x").expect("the shape that could not be placed is told").to_owned();
    assert!(words.starts_with("1 shape could not be placed"), "{words}");
    assert!(near(frame_of(&w, ids[0]).0 + box_before.2 / 2.0, 900_000.0), "the other shape was placed: {:?}", frame_of(&w, ids[0]));
    assert!(near(frame_of(&w, ids[0]).1, box_before.1), "at its own Y");
    w.state_mut().select(&[ids[1]]);
    w.rest();
    assert_eq!(w.state().pane_field_refusal("pane.size.x"), None, "the words are about shapes that are not selected now");
}

/// Esc in a frame in which a box that was let go of earlier is finished cancels the box it was pressed in and not that one.
#[test]
fn an_escape_in_the_frame_a_box_is_finished_cancels_only_the_box_it_was_pressed_in() {
    let (mut w, ids) = two_shapes_and_the_pane();
    type_only(&mut w, "pane.size.x", "6");
    // The Enter lets the box go in one frame, and the Escape comes in the next, the frame in which the box is finished.
    push_key(&mut w, Key::Enter, true);
    w.step();
    push_key(&mut w, Key::Enter, false);
    push_key(&mut w, Key::Escape, true);
    w.step();
    push_key(&mut w, Key::Escape, false);
    w.step();
    w.rest();
    assert!(near(pin_of(&w, ids[0]).0, 6.0), "the value the Enter ended is sent: {:?}", pin_of(&w, ids[0]));
}

/// Format Shape's weight and rounding are quiet over 4,096 shapes, as Size & Position's fields are, and work at 4,096.
#[test]
fn format_shapes_boxes_are_quiet_over_what_one_command_takes() {
    let dir = TempDir::new("format-many");
    let mut w = window_of(&dir, &fixture::detailed_vsdx(4_200, 2));
    let all: Vec<u32> = (1..=4_200).collect();
    w.state_mut().select(&all);
    open(&mut w, "format_shape");
    w.rest();
    assert!(!is_enabled(&w, "pane.format.line.weight") && !is_enabled(&w, "pane.format.line.rounding"), "4,200 shapes");
    let few: Vec<u32> = (1..=4_096).collect();
    w.state_mut().select(&few);
    w.rest();
    assert!(is_enabled(&w, "pane.format.line.weight") && is_enabled(&w, "pane.format.line.rounding"), "4,096 shapes");
}

/// The Explorer lists the foreground pages, the background pages, the styles, then the masters (the order).
#[test]
fn the_explorer_lists_styles_before_masters() {
    let dir = TempDir::new("explorer-order");
    let mut w = window_of(&dir, &fixture::demo_vsdx());
    open(&mut w, "drawing_explorer");
    w.rest();
    let top = |name: &str| rect_of(&w, name).top();
    assert!(top("explorer.branch.foreground_pages") < top("explorer.branch.background_pages"));
    assert!(top("explorer.branch.background_pages") < top("explorer.branch.styles"));
    assert!(top("explorer.branch.styles") < top("explorer.branch.masters"), "styles come before masters");
}

/// A shape with a `NameU` and no `Name` is named by its `NameU` in the title, as `shape.placement` names it.
#[test]
fn the_title_of_a_shape_with_only_a_nameu_is_that_name() {
    use egui_kittest::kittest::Queryable;
    let dir = TempDir::new("nameu");
    let mut w = window_of(&dir, &fixture::nameu_only_vsdx());
    w.state_mut().select(&[1]);
    open(&mut w, "size_position");
    w.rest();
    w.get_by_label("Size & Position - Only name");
}

// ----: a click just outside a box ------------------------------------------------------------------------------------------------

/// A click within egui's interact radius of a box (a few points outside it) is a click on the box: it keeps the focus, and so does its edit, so what is typed
/// next is not wiped by the frame that finds a focused box with no edit.
#[test]
fn a_click_just_outside_a_box_keeps_the_edit_and_what_is_typed_next_is_sent() {
    let (mut w, ids) = two_shapes_and_the_pane();
    type_only(&mut w, "pane.size.x", "6");
    let r = rect_of(&w, "pane.size.x");
    click(&mut w, egui::pos2(r.left() - 3.0, r.center().y));
    w.rest();
    assert_eq!(w.state().pane_edits_open(), 1, "the box still has its edit");
    key(&mut w, Key::A, ctrl());
    type_text(&mut w, "7");
    key(&mut w, Key::Enter, Modifiers::NONE);
    assert!(near(pin_of(&w, ids[0]).0, 7.0), "what was typed after the click is sent: {:?}", pin_of(&w, ids[0]));
}

/// A click well outside the box ends the edit: the value is sent, the focus goes, and a key typed after it changes nothing.
#[test]
fn a_click_well_outside_a_box_sends_the_value_and_the_box_takes_no_more_keys() {
    let (mut w, ids) = two_shapes_and_the_pane();
    type_only(&mut w, "pane.size.x", "6");
    let r = rect_of(&w, "pane.size.x");
    click(&mut w, egui::pos2(r.left() - 12.0, r.center().y));
    w.rest();
    assert!(near(pin_of(&w, ids[0]).0, 6.0), "{:?}", pin_of(&w, ids[0]));
    assert_eq!(w.state().pane_edits_open(), 0);
    type_text(&mut w, "9");
    key(&mut w, Key::Enter, Modifiers::NONE);
    assert!(near(pin_of(&w, ids[0]).0, 6.0), "nothing was typed in a box: {:?}", pin_of(&w, ids[0]));
}
