//! In the window: Ctrl+Alt+K doubles and Ctrl+Alt+Shift+K halves the connection points of the edge under the pointer (of every edge of the shapes selected when the
//! pointer is on none), the Connection Points submenu of the shape's right-click menu and the menu of Home, Tools, Connection Point do the same, and each is greyed when it would
//! do nothing. The pointer and the keyboard are given to egui as a person's would be; what is checked is the drawing the commands changed, what the window drew, and how often
//! it asked the engine (the answers are counted, not timed).
//!
//! The rules are in and the design "Doubling and halving connection points" §10.

use diacraft_ui_egui::work;
use egui::{Event, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::{NodeT, Queryable};
use serde_json::{Value, json};

use crate::editing::{at, edit_window, frame_of, key, shape_ids, type_text};
use crate::support::{Rest, Window, click, move_to, press, run_command};

/// Ctrl+Alt and Ctrl+Alt+Shift, as egui reports them on Windows and Linux.
const CTRL_ALT: Modifiers = Modifiers { alt: true, ctrl: true, shift: false, mac_cmd: false, command: true };
const CTRL_ALT_SHIFT: Modifiers = Modifiers { alt: true, ctrl: true, shift: true, mac_cmd: false, command: true };

/// A wide window, so that no group of the Home tab is folded.
const WIDE: [f32; 2] = [2400.0, 900.0];

fn drop_at(w: &mut Window, master: &str, x: f64, y: f64) -> u32 {
    w.state_mut().execute("shape.drop", &json!({"master": master, "page": 1, "x": x, "y": y})).unwrap()["id"].as_u64().unwrap() as u32
}

fn select(w: &mut Window, ids: &[u32]) {
    run_command(w, "edit.select", json!({"ids": ids}));
}

/// A window with one Rectangle (1.5 by 1 inch, a point at the middle of each side) selected. → the window and the shape.
fn one_rectangle(size: [f32; 2]) -> (Window, u32) {
    let mut w = edit_window(size);
    let id = drop_at(&mut w, "rectangle", 5.0, 5.0);
    select(&mut w, &[id]);
    (w, id)
}

fn enabled(w: &Window, name: &str) -> bool {
    w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).enabled
}

/// The edges of a shape as the engine's query tells them.
fn edges(w: &mut Window, id: u32) -> Vec<Value> {
    let answer = w.state_mut().execute("shape.connection_edges", &json!({"id": id})).unwrap();
    answer["shapes"][0]["edges"].as_array().unwrap().clone()
}

/// The number of points on each edge, in the edges' order.
fn counts(w: &mut Window, id: u32) -> Vec<usize> {
    edges(w, id).iter().map(|e| e["points"].as_array().unwrap().len()).collect()
}

/// The number of connection points the shape holds in all.
fn total(w: &mut Window, id: u32) -> u64 {
    w.state_mut().execute("shape.connection_edges", &json!({"id": id})).unwrap()["shapes"][0]["points"].as_u64().unwrap()
}

/// The number of the edge along the top of the shape.
fn top_edge(w: &mut Window, id: u32) -> usize {
    let all = edges(w, id);
    let y = |e: &Value, end: &str| e[end]["y"].as_f64().unwrap();
    let top = all.iter().map(|e| y(e, "from").max(y(e, "to"))).fold(f64::MIN, f64::max);
    all.iter().position(|e| (y(e, "from") - top).abs() < 1e-9 && (y(e, "to") - top).abs() < 1e-9).expect("the shape has an edge along its top")
}

/// Moves the pointer to a place of the page, in inches.
fn point_at(w: &mut Window, x: f64, y: f64) {
    let pos = at(w, x, y);
    for _ in 0..3 {
        move_to(w, pos);
    }
}

/// Moves the pointer onto the top edge of a shape, away from its points, and presses a key.
fn press_on_top(w: &mut Window, id: u32, pressed: (Key, Modifiers)) {
    let (x, y, width, height) = frame_of(w, id);
    point_at(w, x + 0.25 * width, y + height);
    key(w, pressed.0, pressed.1);
}

/// Moves the pointer to the middle of the shape and presses a key.
fn press_inside(w: &mut Window, id: u32, pressed: (Key, Modifiers)) {
    let (x, y, width, height) = frame_of(w, id);
    point_at(w, x + width / 2.0, y + height / 2.0);
    key(w, pressed.0, pressed.1);
}

const DOUBLE: (Key, Modifiers) = (Key::K, CTRL_ALT);
const HALVE: (Key, Modifiers) = (Key::K, CTRL_ALT_SHIFT);

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

/// A right click at a place of the page, in inches.
fn right_click_at(w: &mut Window, x: f64, y: f64) {
    let pos = at(w, x, y);
    right_click(w, pos);
}

/// A press on the control called `name`, and the window comes to rest.
fn hit(w: &mut Window, name: &str) {
    let at = w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).rect.center();
    click(w, at);
    w.rest();
}

fn notice(w: &Window) -> String {
    w.state().notice().unwrap_or("").to_owned()
}

fn revision(w: &Window) -> u64 {
    w.state().session().revision()
}

// ---- the key --------------------------------------------------------------------------------------------------------------------------------

/// With the pointer on the top edge of the selected Rectangle, Ctrl+Alt+K gives three points on the top and one on each other side, and the notice says so.
#[test]
fn the_key_with_the_pointer_on_the_top_edge_doubles_that_edge_only() {
    let (mut w, id) = one_rectangle([1400.0, 900.0]);
    let other = drop_at(&mut w, "rectangle", 9.0, 5.0);
    select(&mut w, &[id]);
    assert_eq!(counts(&mut w, id), [1, 1, 1, 1]);
    let top = top_edge(&mut w, id);
    let before: Vec<Value> = edges(&mut w, id).iter().map(|e| e["points"].clone()).collect();
    press_on_top(&mut w, id, DOUBLE);
    let mut want = vec![1, 1, 1, 1];
    want[top] = 3;
    assert_eq!(counts(&mut w, id), want, "three on the top, one on each other side");
    assert_eq!(total(&mut w, id), 6);
    assert_eq!(notice(&w), format!("Connection points doubled on 1 edge of shape {id} (1 to 3)"));
    // Old points stay on their edges, and the master (a shape that is not this one) keeps its own four.
    let after: Vec<Value> = edges(&mut w, id).iter().map(|e| e["points"].clone()).collect();
    for (i, old) in before.iter().enumerate() {
        for ix in old.as_array().unwrap() {
            assert!(after[i].as_array().unwrap().contains(ix), "edge {i} kept the point {ix}");
        }
    }
    assert_eq!(counts(&mut w, other), [1, 1, 1, 1], "a shape of the same master keeps the master's points: the doubled shape took its own copy");
    assert_eq!(w.state().ui_panics(), 0);
}

/// With the pointer inside the shape, or off the drawing, the key acts on every edge of the shapes selected.
#[test]
fn the_key_with_the_pointer_elsewhere_doubles_every_edge_of_the_shapes_selected() {
    let (mut w, id) = one_rectangle([1400.0, 900.0]);
    press_inside(&mut w, id, DOUBLE);
    assert_eq!(counts(&mut w, id), [3, 3, 3, 3], "three on each side");
    assert_eq!(notice(&w), format!("Connection points doubled on 4 edges of shape {id} (4 to 12 points in all)"));
    // Off the drawing (on the ribbon) it is every edge as well.
    let tab = w.state().element("tab.Home").unwrap().rect.center();
    for _ in 0..3 {
        move_to(&mut w, tab);
    }
    key(&mut w, DOUBLE.0, DOUBLE.1);
    assert_eq!(counts(&mut w, id), [7, 7, 7, 7], "the pointer was on none");
}

/// A shape that is not selected is not changed, and a pointer on its edge is no target of the key: the selected shapes' every edge is.
#[test]
fn a_pointer_on_a_shape_that_is_not_selected_targets_every_edge_of_the_selected() {
    let (mut w, id) = one_rectangle([1400.0, 900.0]);
    let other = drop_at(&mut w, "rectangle", 9.0, 5.0);
    select(&mut w, &[id]);
    press_on_top(&mut w, other, DOUBLE);
    assert_eq!(counts(&mut w, id), [3, 3, 3, 3]);
    assert_eq!(counts(&mut w, other), [1, 1, 1, 1], "the shape under the pointer was not selected");
}

/// Ctrl+Alt+Shift+K halves back: the edge under the pointer, else every edge.
#[test]
fn shift_halves_the_latest_doubling_back() {
    let (mut w, id) = one_rectangle([1400.0, 900.0]);
    press_inside(&mut w, id, DOUBLE);
    assert_eq!(counts(&mut w, id), [3, 3, 3, 3]);
    let top = top_edge(&mut w, id);
    press_on_top(&mut w, id, HALVE);
    let mut want = vec![3, 3, 3, 3];
    want[top] = 1;
    assert_eq!(counts(&mut w, id), want, "the edge under the pointer went back to its original point");
    assert_eq!(notice(&w), format!("Connection points halved on 1 edge of shape {id} (3 to 1)"));
    press_inside(&mut w, id, HALVE);
    assert_eq!(counts(&mut w, id), [1, 1, 1, 1], "every edge that had an added point went back; originals are never removed");
    assert_eq!(notice(&w), format!("Connection points halved on 3 edges of shape {id} (10 to 4 points in all)"));
    // The doublings undo one by one: 1 to 3 to 7, then back to 3, then to 1.
    press_inside(&mut w, id, DOUBLE);
    press_inside(&mut w, id, DOUBLE);
    assert_eq!(counts(&mut w, id), [7, 7, 7, 7]);
    press_inside(&mut w, id, HALVE);
    assert_eq!(counts(&mut w, id), [3, 3, 3, 3]);
}

/// With nothing added the halving key does nothing: no change, no undo step, no notice; and both Halve rows are greyed.
#[test]
fn with_nothing_added_the_halving_key_does_nothing_and_both_halve_rows_are_greyed() {
    let (mut w, id) = one_rectangle(WIDE);
    let steps = revision(&w);
    press_inside(&mut w, id, HALVE);
    assert_eq!(counts(&mut w, id), [1, 1, 1, 1]);
    assert_eq!(revision(&w), steps, "no undo step");
    assert_eq!(notice(&w), "", "a disabled key says nothing");
    // The ribbon's menu: Double works, Halve is greyed.
    hit(&mut w, "ribbon.Home.ConnectionPointTool");
    assert!(enabled(&w, "menu.ConnectionPointsDouble"));
    assert!(!enabled(&w, "menu.ConnectionPointsHalve"), "no point a doubling made is on the shape");
    key(&mut w, Key::Escape, Modifiers::NONE);
    // The right-click menu's.
    let (x, y, width, height) = frame_of(&w, id);
    right_click_at(&mut w, x + 0.25 * width, y + height);
    hit(&mut w, "menu.CanvasConnectionPoints");
    assert!(enabled(&w, "menu.CanvasConnectionPointsDouble"));
    assert!(!enabled(&w, "menu.CanvasConnectionPointsHalve"));
    assert_eq!(revision(&w), steps);
}

/// On a Diamond, whose points are its four corners, Double gives one point on the middle of each side and keeps the corners.
#[test]
fn on_a_diamond_double_gives_one_point_on_each_side_and_keeps_the_corners() {
    let mut w = edit_window(WIDE);
    let id = drop_at(&mut w, "diamond", 5.0, 5.0);
    select(&mut w, &[id]);
    assert_eq!(total(&mut w, id), 4);
    assert_eq!(counts(&mut w, id), [0, 0, 0, 0], "the corners are on no edge");
    // The pointer is on the middle of the shape, which is on no side.
    press_inside(&mut w, id, DOUBLE);
    assert_eq!(counts(&mut w, id), [1, 1, 1, 1], "one on each side: no point gives one");
    assert_eq!(total(&mut w, id), 8, "and the four corners are kept");
    press_inside(&mut w, id, HALVE);
    assert_eq!(counts(&mut w, id), [0, 0, 0, 0], "one point gives none");
    assert_eq!(total(&mut w, id), 4, "the corners, which are original points, are kept");
    // With none left, halving stays at none: the key does nothing and the row is greyed.
    let steps = revision(&w);
    press_inside(&mut w, id, HALVE);
    assert_eq!(revision(&w), steps, "no undo step");
    assert_eq!(total(&mut w, id), 4);
    hit(&mut w, "ribbon.Home.ConnectionPointTool");
    assert!(enabled(&w, "menu.ConnectionPointsDouble"));
    assert!(!enabled(&w, "menu.ConnectionPointsHalve"), "nothing a doubling made is left to take");
}

/// The key stands aside while a shape's text is edited and while a menu is open.
#[test]
fn the_key_stands_aside_while_text_is_edited_or_a_menu_is_open() {
    let (mut w, id) = one_rectangle(WIDE);
    let steps = revision(&w);
    // A menu is open.
    hit(&mut w, "ribbon.Home.ConnectionPointTool");
    assert!(w.state().element("menu.ConnectionPointsDouble").is_some(), "the menu is open");
    press_inside(&mut w, id, DOUBLE);
    assert_eq!(revision(&w), steps, "a menu is open: the key is the menu's");
    key(&mut w, Key::Escape, Modifiers::NONE);
    // A text box is open over the shape.
    let (x, y, width, height) = frame_of(&w, id);
    let centre = at(&w, x + width / 2.0, y + height / 2.0);
    click(&mut w, centre);
    click(&mut w, centre);
    w.rest();
    assert!(w.state().window_state()["editingText"].as_bool().unwrap(), "the shape's text is being edited");
    key(&mut w, DOUBLE.0, DOUBLE.1);
    key(&mut w, HALVE.0, HALVE.1);
    assert_eq!(revision(&w), steps, "text is being edited: the keys do no doubling");
    assert_eq!(counts(&mut w, id), [1, 1, 1, 1]);
}

/// No selection leaves the keys quiet and every row greyed, and so does a connector alone.
#[test]
fn no_selection_or_a_connector_alone_leaves_the_rows_greyed_and_the_keys_quiet() {
    let mut w = edit_window(WIDE);
    let a = drop_at(&mut w, "rectangle", 3.0, 5.0);
    let b = drop_at(&mut w, "rectangle", 8.0, 5.0);
    let connector = w.state_mut().execute("connector.add", &json!({"from": {"id": a}, "to": {"id": b}})).unwrap()["id"].as_u64().unwrap() as u32;
    // Nothing selected.
    select(&mut w, &[]);
    assert!(!enabled(&w, "ribbon.Home.ConnectionPointTool"), "nothing is selected: the control is quiet");
    let steps = revision(&w);
    key(&mut w, DOUBLE.0, DOUBLE.1);
    assert_eq!(revision(&w), steps);
    let refused = w.state_mut().execute("window.connectionPointsDo", &json!({"action": "double"})).unwrap_err().to_string();
    assert!(refused.contains("nothing is selected"), "{refused}");
    // A connector alone.
    select(&mut w, &[connector]);
    assert!(!enabled(&w, "ribbon.Home.ConnectionPointTool"), "a connector has no edge to put points on");
    key(&mut w, DOUBLE.0, DOUBLE.1);
    key(&mut w, HALVE.0, HALVE.1);
    assert_eq!(revision(&w), steps);
    assert_eq!(counts(&mut w, a), [1, 1, 1, 1]);
    let refused = w.state_mut().execute("window.connectionPointsDo", &json!({"action": "double", "target": "all"})).unwrap_err().to_string();
    assert!(refused.contains("connector"), "the engine's words for a connector: {refused}");
    // A connector with a rectangle: the rectangle is doubled and the connector is skipped by the engine's words.
    select(&mut w, &[a, connector]);
    assert!(enabled(&w, "ribbon.Home.ConnectionPointTool"));
    press_inside(&mut w, a, DOUBLE);
    assert_eq!(counts(&mut w, a), [3, 3, 3, 3]);
}

// ---- the rows -------------------------------------------------------------------------------------------------------------------------------

/// The ribbon's rows act on every edge of the shapes selected, whatever is under the pointer.
#[test]
fn the_ribbon_menu_rows_act_on_every_edge() {
    let (mut w, id) = one_rectangle(WIDE);
    assert!(enabled(&w, "ribbon.Home.ConnectionPointTool"), "a shape is selected");
    hit(&mut w, "ribbon.Home.ConnectionPointTool");
    let rows: Vec<String> = ["menu.ConnectionPointsDouble", "menu.ConnectionPointsHalve"].iter().map(|s| (*s).to_owned()).collect();
    assert!(rows.iter().all(|r| w.state().element(r).is_some()), "the menu holds Double and Halve");
    assert!(!w.get_by_label("Double Connection Points").accesskit_node().is_disabled());
    assert!(w.get_by_label("Halve Connection Points").accesskit_node().is_disabled());
    hit(&mut w, "menu.ConnectionPointsDouble");
    assert_eq!(counts(&mut w, id), [3, 3, 3, 3]);
    assert_eq!(notice(&w), format!("Connection points doubled on 4 edges of shape {id} (4 to 12 points in all)"));
    // Halve now works, from the same menu.
    hit(&mut w, "ribbon.Home.ConnectionPointTool");
    assert!(enabled(&w, "menu.ConnectionPointsHalve"));
    hit(&mut w, "menu.ConnectionPointsHalve");
    assert_eq!(counts(&mut w, id), [1, 1, 1, 1]);
    // The doubling and the halving are each one undo step.
    run_command(&mut w, "edit.undo", json!({}));
    assert_eq!(counts(&mut w, id), [3, 3, 3, 3]);
    run_command(&mut w, "edit.undo", json!({}));
    assert_eq!(counts(&mut w, id), [1, 1, 1, 1]);
}

/// The ribbon's rows are where the plan puts them: the Connection Point control of Home, Tools holds them, and the Connector button next to it is not changed.
#[test]
fn the_connection_point_control_holds_the_two_rows_in_the_tools_group() {
    let (w, _id) = one_rectangle(WIDE);
    assert!(w.state().element("ribbon.Home.ConnectionPointTool").is_some() && w.state().element("ribbon.Home.ConnectorTool").is_some());
    let tool = diacraft_ui_egui::ribbon::spec::tab_by_label("Home").unwrap().groups.iter().find(|g| g.label == "Tools").unwrap().top_level();
    let control = tool.iter().find(|c| c.id == "ConnectionPointTool").unwrap();
    let ids: Vec<&str> = control.items.iter().map(|c| c.id).collect();
    assert_eq!(ids, ["ConnectionPointsDouble", "ConnectionPointsHalve"]);
    assert!(control.items.iter().all(|c| c.command == Some("window.connectionPointsDo")));
}

/// The right-click menu has a Connection Points submenu after Container and before Format Shape; its rows act on the edge at the click, else on every edge.
#[test]
fn the_right_click_rows_act_on_the_edge_at_the_click_or_on_every_edge() {
    let (mut w, id) = one_rectangle([1400.0, 900.0]);
    let (x, y, width, height) = frame_of(&w, id);
    // A click on the top edge.
    right_click_at(&mut w, x + 0.25 * width, y + height);
    let row = |w: &Window, name: &str| w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).rect.center().y;
    let (container, points, format) = (row(&w, "menu.CanvasContainer"), row(&w, "menu.CanvasConnectionPoints"), row(&w, "menu.CanvasFormatShape"));
    assert!(container < points && points < format, "the submenu is after Container and before Format Shape: {container} {points} {format}");
    hit(&mut w, "menu.CanvasConnectionPoints");
    assert!(enabled(&w, "menu.CanvasConnectionPointsDouble"));
    assert!(!enabled(&w, "menu.CanvasConnectionPointsHalve"));
    assert!(!w.get_by_label("Double Connection Points").accesskit_node().is_disabled());
    hit(&mut w, "menu.CanvasConnectionPointsDouble");
    let top = top_edge(&mut w, id);
    let mut want = vec![1, 1, 1, 1];
    want[top] = 3;
    assert_eq!(counts(&mut w, id), want, "the click was on the top edge");
    // Now Halve is on for that edge, and a click in the middle acts on every edge: it has nothing to halve elsewhere, and Double doubles all four.
    right_click_at(&mut w, x + width / 2.0, y + height / 2.0);
    hit(&mut w, "menu.CanvasConnectionPoints");
    assert!(enabled(&w, "menu.CanvasConnectionPointsHalve"), "the top edge has points a doubling made: every edge is the target");
    hit(&mut w, "menu.CanvasConnectionPointsDouble");
    let mut every = vec![3, 3, 3, 3];
    every[top] = 7;
    assert_eq!(counts(&mut w, id), every);
}

/// A right-click on a shape that is not selected selects it, and the rows act on it. A connector's menu has no such rows.
#[test]
fn a_right_click_selects_the_shape_and_a_connectors_menu_has_no_rows() {
    let mut w = edit_window([1400.0, 900.0]);
    let a = drop_at(&mut w, "rectangle", 3.0, 5.0);
    let b = drop_at(&mut w, "rectangle", 8.0, 5.0);
    select(&mut w, &[a]);
    let (x, y, width, height) = frame_of(&w, b);
    right_click_at(&mut w, x + width / 2.0, y + height / 2.0);
    assert_eq!(w.state().selection(), [b]);
    hit(&mut w, "menu.CanvasConnectionPoints");
    hit(&mut w, "menu.CanvasConnectionPointsDouble");
    assert_eq!(counts(&mut w, b), [3, 3, 3, 3]);
    assert_eq!(counts(&mut w, a), [1, 1, 1, 1]);
    key(&mut w, Key::Escape, Modifiers::NONE);
    // The connector's own menu.
    let connector = w.state_mut().execute("connector.add", &json!({"from": {"id": a}, "to": {"id": b}})).unwrap()["id"].as_u64().unwrap() as u32;
    select(&mut w, &[connector]);
    right_click_at(&mut w, 5.5, 5.0);
    assert!(w.state().element("menu.CanvasConnectionPoints").is_none(), "the connector's menu has no Connection Points rows");
}

/// The member of a group that is selected is the shape the rows act on (design §10.2), and the group itself has no outline of its own to put points on.
#[test]
fn a_member_selected_inside_a_group_is_doubled_and_the_group_itself_is_not() {
    let mut w = edit_window(WIDE);
    let a = drop_at(&mut w, "rectangle", 3.0, 5.0);
    let b = drop_at(&mut w, "rectangle", 8.0, 5.0);
    let group = w.state_mut().execute("shape.group", &json!({"ids": [a, b], "page": 1})).unwrap()["id"].as_u64().unwrap() as u32;
    select(&mut w, &[group]);
    assert!(!enabled(&w, "ribbon.Home.ConnectionPointTool"), "a group is no shape with an edge: the rows are greyed");
    let refused = w.state_mut().execute("window.connectionPointsDo", &json!({"action": "double", "target": "all"})).unwrap_err().to_string();
    assert!(refused.contains("is a group"), "the engine's words for a group: {refused}");
    select(&mut w, &[a]);
    assert!(enabled(&w, "ribbon.Home.ConnectionPointTool"), "the member is the shape selected");
    run_command(&mut w, "window.connectionPointsDo", json!({"action": "double", "target": "all"}));
    assert_eq!(counts(&mut w, a), [3, 3, 3, 3]);
    assert_eq!(counts(&mut w, b), [1, 1, 1, 1], "the other member is not selected");
    assert_eq!(w.state().ui_panics(), 0);
}

/// The rows and the control are reached in a narrow window too: the Tools group folds into a card, and the rows are in the menu over it.
#[test]
fn in_a_narrow_window_the_rows_are_in_the_folded_tools_card() {
    let (mut w, id) = one_rectangle([440.0, 700.0]);
    assert!(w.state().element("ribbon.Home.fold.GroupTools").is_some(), "the Tools group is folded at this width");
    hit(&mut w, "ribbon.Home.fold.GroupTools");
    assert!(enabled(&w, "ribbon.Home.ConnectionPointTool"));
    hit(&mut w, "ribbon.Home.ConnectionPointTool");
    assert!(enabled(&w, "menu.ConnectionPointsDouble"));
    let screen = egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(440.0, 700.0));
    for name in ["menu.ConnectionPointsDouble", "menu.ConnectionPointsHalve"] {
        assert!(screen.contains_rect(w.state().element(name).unwrap().rect), "{name} is inside the window");
    }
    hit(&mut w, "menu.ConnectionPointsDouble");
    assert_eq!(counts(&mut w, id), [3, 3, 3, 3]);
}

// ---- the cache and the pick -----------------------------------------------------------------------------------------------------------------

/// The window asks `shape.connection_edges` once for a selection, and not again while nothing changes: not at each frame, not for a menu opened, not for a key pressed with
/// the pointer elsewhere; again when the selection or the drawing changes (the drawing's own generation too: a file opened starts its revision again).
#[test]
fn the_cache_is_asked_once_for_a_selection_and_not_again_while_nothing_changes() {
    work::reset();
    let (mut w, id) = one_rectangle(WIDE);
    w.rest();
    for _ in 0..12 {
        w.step();
    }
    assert_eq!(
        work::work().connection_edge_queries,
        1,
        "one question for a selection that has not changed in twelve frames, the first it was drawn for"
    );
    // Opening the menu and moving the pointer ask nothing new.
    hit(&mut w, "ribbon.Home.ConnectionPointTool");
    key(&mut w, Key::Escape, Modifiers::NONE);
    let (x, y, _width, height) = frame_of(&w, id);
    for step in 0..8 {
        point_at(&mut w, x + 0.1 * f64::from(step), y + height);
    }
    assert_eq!(work::work().connection_edge_queries, 1, "the selection is read once, and the pointer and a menu ask nothing");
    // A key reads nothing again either: the cache answers whether it can act.
    key(&mut w, DOUBLE.0, DOUBLE.1);
    let after_doubling = work::work().connection_edge_queries;
    assert_eq!(after_doubling, 2, "the doubling moved the revision: one question for the new drawing, and none for the key itself");
    for _ in 0..6 {
        w.step();
    }
    assert_eq!(work::work().connection_edge_queries, after_doubling, "and not again while nothing changes");
    // Another selection is another question; the same one again is not.
    let other = drop_at(&mut w, "rectangle", 9.0, 5.0);
    select(&mut w, &[other]);
    w.rest();
    let after_select = work::work().connection_edge_queries;
    assert!(after_select > after_doubling);
    for _ in 0..6 {
        w.step();
    }
    assert_eq!(work::work().connection_edge_queries, after_select);
}

/// A selection past `MAX_WINDOW_SHAPES` is not read: both rows stay enabled, and the engine's words say what happened.
#[test]
fn a_selection_past_the_windows_cap_leaves_the_rows_enabled_and_the_engines_words_shown() {
    let mut w = edit_window(WIDE);
    for i in 0..257_u32 {
        drop_at(&mut w, "rectangle", 0.5 + f64::from(i % 20) * 0.4, 0.5 + f64::from(i / 20) * 0.4);
    }
    run_command(&mut w, "edit.selectAll", json!({}));
    assert_eq!(w.state().selection().len(), 257);
    work::reset();
    w.step();
    w.step();
    assert!(enabled(&w, "ribbon.Home.ConnectionPointTool"));
    assert_eq!(work::work().connection_edge_queries, 0, "more shapes than the window reads: the engine is not asked");
    hit(&mut w, "ribbon.Home.ConnectionPointTool");
    assert!(enabled(&w, "menu.ConnectionPointsHalve"), "past the cap Halve is left enabled, though nothing was added");
    hit(&mut w, "menu.ConnectionPointsHalve");
    assert!(notice(&w).contains("nothing to halve"), "the engine's words: {}", notice(&w));
    assert_eq!(shape_ids(&w).len(), 257);
}

/// Past the window's cap the key leaves the pick to the engine: it sends the pointer's place as `at` and `reach`, and the one edge near it is the one that changes.
#[test]
fn past_the_cap_the_key_leaves_the_pick_to_the_engine() {
    let mut w = edit_window(WIDE);
    for i in 0..256_u32 {
        drop_at(&mut w, "rectangle", 1.0 + f64::from(i % 16) * 0.5, 5.0 + f64::from(i / 16) * 0.2);
    }
    // The shape the pointer will be on stands alone, in view and a long way from the others' edges.
    let target = drop_at(&mut w, "rectangle", 5.0, 3.0);
    run_command(&mut w, "edit.selectAll", json!({}));
    assert_eq!(w.state().selection().len(), 257);
    work::reset();
    let (x, y, width, height) = frame_of(&w, target);
    point_at(&mut w, x + 0.25 * width, y + height);
    key(&mut w, DOUBLE.0, DOUBLE.1);
    assert_eq!(work::work().connection_edge_queries, 0, "more shapes than the window reads: the engine is not asked what the edges can take");
    let top = top_edge(&mut w, target);
    let mut want = vec![1, 1, 1, 1];
    want[top] = 3;
    assert_eq!(counts(&mut w, target), want, "the engine doubled the one edge near the pointer");
    let first = shape_ids(&w)[0];
    assert_eq!(counts(&mut w, first), [1, 1, 1, 1], "and no other shape");
}

/// The engine picks the edge the window picks, for the window's place and reach (`at` and `reach`, design §10.2).
#[test]
fn the_engine_picks_the_edge_the_window_picks() {
    let (mut w, id) = one_rectangle([1400.0, 900.0]);
    let ellipse = drop_at(&mut w, "ellipse", 10.0, 5.0);
    let ppi = w.state().window_state()["pointsPerInch"].as_f64().unwrap();
    let reach = 5.0 / ppi;
    select(&mut w, &[id, ellipse]);
    w.rest();
    let (x, y, width, height) = frame_of(&w, id);
    let (ex, ey, ew, eh) = frame_of(&w, ellipse);
    let places = [
        ("on the top edge", (x + 0.3 * width, y + height)),
        ("just above the top edge, within reach", (x + 0.3 * width, y + height + 0.5 * reach)),
        ("on the right edge", (x + width, y + 0.3 * height)),
        ("just left of the left edge, within reach", (x - 0.5 * reach, y + 0.7 * height)),
        ("on the top-left corner, where two edges tie", (x, y + height)),
        ("past the reach", (x + 0.3 * width, y + height + 3.0 * reach)),
        ("inside the rectangle", (x + width / 2.0, y + height / 2.0)),
        ("on the ellipse", (ex + ew / 2.0, ey + eh)),
        ("on the ellipse's left", (ex, ey + eh / 2.0)),
        ("far from every shape", (14.0, 8.0)),
    ];
    for (what, (px, py)) in places {
        let window = w.state().connection_edge_at((px, py));
        // The engine, asked the same way a program would: the shapes named and the place and reach.
        let answer = w
            .state_mut()
            .execute("shape.connection_points", &json!({"ids": [id, ellipse], "action": "double", "at": {"x": px, "y": py}, "reach": reach}))
            .unwrap_or_else(|e| panic!("{what}: {e}"));
        let changed: Vec<(u64, u64)> = answer["shapes"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|s| s["edges"].as_array().unwrap().iter().map(|e| (s["id"].as_u64().unwrap(), e["edge"].as_u64().unwrap())))
            .collect();
        run_command(&mut w, "edit.undo", json!({}));
        let engine = if changed.len() == 1 { Some((changed[0].0 as u32, changed[0].1 as u32)) } else { None };
        assert_eq!(window, engine, "{what}: the window picks {window:?} and the engine {engine:?} (changed: {changed:?})");
    }
}

// ---- the minimum distance ---------------------------------------------------------------------------------------------------------------------

/// A window setting, sent as `min_gap` on every call: Double is greyed when the edge has no room under the minimum, and a smaller minimum in Options brings it back with no
/// new selection.
#[test]
fn a_change_of_the_minimum_clears_the_enabled_state_without_a_new_selection() {
    let (mut w, id) = one_rectangle(WIDE);
    assert!((w.state().window_state()["connectionMinGap"].as_f64().unwrap() - 3.0 / 25.4).abs() < 1e-9, "3 mm by default");
    // The top is 1.5 inches long with one point: 1 → 3 → 7 → 15 are the doublings that keep 3 mm (0.118 inch) between points; the next would make 0.094.
    let (x, y, width, height) = frame_of(&w, id);
    for _ in 0..3 {
        point_at(&mut w, x + 0.1 * width, y + height);
        key(&mut w, DOUBLE.0, DOUBLE.1);
    }
    let top = top_edge(&mut w, id);
    assert_eq!(counts(&mut w, id)[top], 7, "1 → 3 → 7, and the third key had no room: the gaps of 15 points would be 0.094 inch");
    let steps = revision(&w);
    key(&mut w, DOUBLE.0, DOUBLE.1);
    assert_eq!(revision(&w), steps, "the key does nothing: Double is greyed for the top edge");
    // With the pointer elsewhere the other edges still have room, so the rows are not greyed for them.
    hit(&mut w, "ribbon.Home.ConnectionPointTool");
    assert!(enabled(&w, "menu.ConnectionPointsDouble"));
    key(&mut w, Key::Escape, Modifiers::NONE);
    // The setting, as the Options dialog sends it: 1.5 mm.
    run_command(&mut w, "window.connectionMinGap", json!({"inches": 1.5 / 25.4}));
    assert!((w.state().window_state()["connectionMinGap"].as_f64().unwrap() - 1.5 / 25.4).abs() < 1e-9);
    point_at(&mut w, x + 0.1 * width, y + height);
    key(&mut w, DOUBLE.0, DOUBLE.1);
    assert_eq!(counts(&mut w, id)[top], 15, "enabled again, with no new selection");
}

/// The command that sets it takes paper inches from 0.01 to 2 and refuses the rest in words.
#[test]
fn the_minimum_is_refused_outside_its_range_in_words() {
    let mut w = edit_window([1400.0, 900.0]);
    for good in [0.01, 2.0, 0.125] {
        run_command(&mut w, "window.connectionMinGap", json!({"inches": good}));
        assert!((w.state().window_state()["connectionMinGap"].as_f64().unwrap() - good).abs() < 1e-12);
    }
    for bad in [json!(0.005), json!(2.5), json!(-1), json!("big"), json!(null)] {
        let refused = w.state_mut().execute("window.connectionMinGap", &json!({"inches": bad})).unwrap_err().to_string();
        assert!(refused.contains("0.01") && refused.contains('2'), "{bad}: {refused}");
    }
    assert!((w.state().window_state()["connectionMinGap"].as_f64().unwrap() - 0.125).abs() < 1e-12, "a refusal changes nothing");
}

/// The row in File, Options, Advanced shows the minimum in millimetres, takes any unit, refuses 0.2 mm and 60 mm in words and sends the setting on OK.
#[test]
fn the_options_row_sets_the_minimum_and_refuses_what_is_out_of_range() {
    let mut w = edit_window([1400.0, 900.0]);
    run_command(&mut w, "window.options", json!({"tab": "Advanced"}));
    let field = w.state().element("options.min_gap").expect("the row's box is drawn").clone();
    assert!(field.enabled);
    assert_eq!(w.state().options_dialog_min_gap_text().as_deref(), Some("3 mm"));
    let type_in = |w: &mut Window, text: &str| {
        hit(w, "options.min_gap");
        key(w, Key::A, Modifiers::COMMAND);
        type_text(w, text);
    };
    // 0.2 mm and 60 mm are out of the range 0.254 mm to 50.8 mm.
    for refused in ["0.2 mm", "60 mm", "nonsense"] {
        type_in(&mut w, refused);
        press(&mut w, "options.ok");
        assert_eq!(w.state().options_dialog_tab(), Some("Advanced"), "{refused}: the dialog stays open");
        let words = w.state().options_dialog_error().unwrap_or_else(|| panic!("{refused}: no words")).to_owned();
        assert!(words.contains("0.254 mm") && words.contains("50.8 mm"), "{refused}: {words}");
        assert!((w.state().window_state()["connectionMinGap"].as_f64().unwrap() - 3.0 / 25.4).abs() < 1e-9, "{refused} changed nothing");
    }
    type_in(&mut w, "1.5 mm");
    press(&mut w, "options.ok");
    assert!(w.state().options_dialog_tab().is_none(), "OK closed the dialog");
    assert!((w.state().window_state()["connectionMinGap"].as_f64().unwrap() - 1.5 / 25.4).abs() < 1e-6);
    // The units it takes: inches, and a plain number is millimetres (the unit it shows).
    run_command(&mut w, "window.options", json!({"tab": "Advanced"}));
    assert_eq!(w.state().options_dialog_min_gap_text().as_deref(), Some("1.5 mm"));
    type_in(&mut w, "0.25 in");
    press(&mut w, "options.ok");
    assert!((w.state().window_state()["connectionMinGap"].as_f64().unwrap() - 0.25).abs() < 1e-9);
    run_command(&mut w, "window.options", json!({"tab": "Advanced"}));
    type_in(&mut w, "4");
    press(&mut w, "options.ok");
    assert!((w.state().window_state()["connectionMinGap"].as_f64().unwrap() - 4.0 / 25.4).abs() < 1e-9);
    // Cancel changes nothing.
    run_command(&mut w, "window.options", json!({"tab": "Advanced"}));
    type_in(&mut w, "10 mm");
    press(&mut w, "options.cancel");
    assert!((w.state().window_state()["connectionMinGap"].as_f64().unwrap() - 4.0 / 25.4).abs() < 1e-9);
    assert_eq!(w.state().ui_panics(), 0);
}

// ---- the window command ---------------------------------------------------------------------------------------------------------------------

/// `window.connectionPointsDo` takes `action` and an optional `target`, and refuses the rest in words.
#[test]
fn the_window_command_refuses_what_it_does_not_take_in_words() {
    let (mut w, id) = one_rectangle([1400.0, 900.0]);
    let refused = |w: &mut Window, params: Value| w.state_mut().execute("window.connectionPointsDo", &params).unwrap_err().to_string();
    assert!(refused(&mut w, json!({})).contains("action"));
    assert!(refused(&mut w, json!({"action": "set_count"})).contains("double or halve"));
    assert!(refused(&mut w, json!({"action": "triple"})).contains("double"));
    assert!(refused(&mut w, json!({"action": "double", "target": "left"})).contains("pointer"));
    assert!(refused(&mut w, json!({"action": "double", "count": 3})).contains("count"));
    assert_eq!(counts(&mut w, id), [1, 1, 1, 1]);
    // Nothing to halve: a disabled command, in the window's words.
    let none = refused(&mut w, json!({"action": "halve", "target": "all"}));
    assert!(none.contains("nothing to halve"), "{none}");
    run_command(&mut w, "window.connectionPointsDo", json!({"action": "double", "target": "all"}));
    assert_eq!(counts(&mut w, id), [3, 3, 3, 3]);
}

/// Every new control has an accessible name, and the Options row's box has the row's caption.
#[test]
fn every_new_control_has_an_accessible_name() {
    let (mut w, _id) = one_rectangle(WIDE);
    hit(&mut w, "ribbon.Home.ConnectionPointTool");
    for label in ["Double Connection Points", "Halve Connection Points"] {
        let _ = w.get_by_label(label);
    }
    key(&mut w, Key::Escape, Modifiers::NONE);
    run_command(&mut w, "window.options", json!({"tab": "Advanced"}));
    // The caption and the box are two nodes of one name: one of them is the box.
    let named: Vec<_> = w.get_all_by_label("Minimum distance between doubled connection points").collect();
    assert!(named.iter().any(|n| n.accesskit_node().role() == egui::accesskit::Role::TextInput), "the box is named by the row's caption");
}
