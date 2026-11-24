//! Drawing in the window: tiles dropped on the page, selecting, moving, resizing, joining, text in place, Delete and undo. The pointer and
//! the keyboard are given to egui as a person's would be, and what is checked is the document they changed through commands.

use diacraft_engine::Session;
use diacraft_engine::draw::shape_frame;
use diacraft_ui_egui::DiacraftApp;
use diacraft_ui_egui::render_worker::bundled_drawer;
use egui::{Event, Key, Modifiers, Pos2, vec2};
use serde_json::json;

use crate::support::{Rest, Window, button, click, drag, fast_window, move_to, run_command};

/// A window on a new drawing at 100% zoom, with pictures drawn on the spot. Its frames are 1/60 s apart, so two clicks are a double click.
pub fn edit_window(size: [f32; 2]) -> Window {
    edit_window_with(size, |app| app)
}

pub fn edit_window_with(size: [f32; 2], build: impl FnOnce(DiacraftApp) -> DiacraftApp) -> Window {
    let mut session = Session::new();
    session.execute("file.new", &json!({})).unwrap();
    let mut w = fast_window(build(DiacraftApp::new(session, None).with_inline_pictures(bundled_drawer())), size);
    w.rest();
    run_command(&mut w, "view.zoom", json!({"percent": 100}));
    w
}

/// Holds the keys `modifiers` names from the next frame on (egui learns of them by an event of their own).
pub fn hold(w: &mut Window, modifiers: Modifiers) {
    w.input_mut().events.push(Event::ModifiersChanged(modifiers));
    w.step();
}

/// A click at a point of the page, with keys held.
pub fn click_with(w: &mut Window, (x, y): (f64, f64), modifiers: Modifiers) {
    let pos = at(w, x, y);
    for _ in 0..3 {
        move_to(w, pos);
    }
    hold(w, modifiers);
    button(w, pos, true);
    button(w, pos, false);
    hold(w, Modifiers::NONE);
    w.rest();
}

/// A click at a point of the page.
pub fn click_at(w: &mut Window, x: f64, y: f64) {
    let pos = at(w, x, y);
    click(w, pos);
    w.rest();
}

/// A drag between two points of the page, with keys held from the press to the release.
pub fn drag_with(w: &mut Window, from: (f64, f64), to: (f64, f64), modifiers: Modifiers) {
    let (from, to) = (at(w, from.0, from.1), at(w, to.0, to.1));
    for _ in 0..3 {
        move_to(w, from);
    }
    hold(w, modifiers);
    button(w, from, true);
    for step in 1..=5 {
        move_to(w, from + (to - from) * (step as f32 / 5.0));
    }
    button(w, to, false);
    hold(w, Modifiers::NONE);
    w.rest();
}

/// The screen point of a point of the page, in inches.
pub fn at(w: &Window, x: f64, y: f64) -> Pos2 {
    w.state().screen_point_at((x, y)).expect("the page has been drawn")
}

/// The `ID`s of the shapes on the first page, in order.
pub fn shape_ids(w: &Window) -> Vec<u32> {
    w.state().session().document().unwrap().pages.get(0).unwrap().shapes.iter().map(|s| s.id).collect()
}

fn page_id(w: &Window) -> u32 {
    w.state().session().document().unwrap().pages.get(0).unwrap().id
}

/// The box of a shape in inches: lower left, then size.
pub fn frame_of(w: &Window, id: u32) -> (f64, f64, f64, f64) {
    let f = shape_frame(w.state().session().document().unwrap(), page_id(w), id).unwrap();
    (f.x, f.y, f.width, f.height)
}

/// Drags the tile named `name` onto the page point (x, y).
pub fn drag_tile_to(w: &mut Window, name: &str, x: f64, y: f64) {
    let tile = w.state().element(&format!("shapes.tile.{name}")).expect("the tile is drawn").clone();
    let to = at(w, x, y);
    drag(w, tile.rect.center(), to - tile.rect.center());
    w.rest();
}

/// A key press and release, then the window comes to rest ([`Rest::rest`]): what the key started on another thread (the Save dialog, a file
/// being read) is waited for, whenever the machine lets it answer.
pub fn key(w: &mut Window, key: Key, modifiers: Modifiers) {
    key_without_waiting(w, key, modifiers);
    w.rest();
}

/// A key press and release, and no wait: for a test that looks at the dialog or the file being read before it ends.
pub fn key_without_waiting(w: &mut Window, key: Key, modifiers: Modifiers) {
    w.input_mut().events.push(Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers });
    w.step();
    w.input_mut().events.push(Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers });
    w.step();
}

pub fn type_text(w: &mut Window, text: &str) {
    w.input_mut().events.push(Event::Text(text.to_owned()));
    w.step();
    w.rest();
}

/// Drops three shapes, one a row: (3, 6), (7, 6) and (5, 3).
pub fn three_shapes(w: &mut Window) -> Vec<u32> {
    drag_tile_to(w, "Rectangle", 3.0, 6.0);
    drag_tile_to(w, "Diamond", 7.0, 6.0);
    drag_tile_to(w, "Ellipse", 5.0, 3.0);
    shape_ids(w)
}

/// A drag from a tile in the Shapes window onto the page drops that shape there, snapped to the grid, and selects it.
#[test]
fn dragging_a_tile_onto_the_page_drops_the_shape_where_it_lands() {
    let mut w = edit_window([1280.0, 900.0]);
    assert!(shape_ids(&w).is_empty());
    drag_tile_to(&mut w, "Rectangle", 4.04, 5.04);
    let ids = shape_ids(&w);
    assert_eq!(ids.len(), 1, "one shape was dropped");
    let (x, y, width, height) = frame_of(&w, ids[0]);
    // A rectangle is 1.5 by 1; the grid at 100% is a quarter inch and pulls from 5 pixels, so the centre (4.04, 5.04), 3.8 pixels off the lines, lands on (4.0, 5.0).
    assert!((width - 1.5).abs() < 1e-9 && (height - 1.0).abs() < 1e-9, "{width} x {height}");
    assert!((x + width / 2.0 - 4.0).abs() < 1e-6 && (y + height / 2.0 - 5.0).abs() < 1e-6, "centred at ({}, {})", x + width / 2.0, y + height / 2.0);
    assert_eq!(w.state().selection(), ids.as_slice(), "the new shape is selected");
    assert_eq!(w.state().ui_panics(), 0);
    // A drop is one undo step.
    run_command(&mut w, "edit.undo", json!({}));
    assert!(shape_ids(&w).is_empty());
}

/// Released away from the page area, the drag drops nothing.
#[test]
fn a_tile_released_over_the_ribbon_drops_nothing() {
    let mut w = edit_window([1280.0, 900.0]);
    let tile = w.state().element("shapes.tile.Rectangle").unwrap().clone();
    drag(&mut w, tile.rect.center(), vec2(30.0, -400.0));
    w.rest();
    assert!(shape_ids(&w).is_empty());
}

/// A click on a tile drops one at the middle of what is in view, and the next one a step from it.
#[test]
fn clicking_a_tile_drops_a_shape_at_the_middle_and_the_next_beside_it() {
    let mut w = edit_window([1280.0, 900.0]);
    let tile = w.state().element("shapes.tile.Circle").unwrap().clone();
    click(&mut w, tile.rect.center());
    w.rest();
    click(&mut w, tile.rect.center());
    w.rest();
    let ids = shape_ids(&w);
    assert_eq!(ids.len(), 2);
    let centre = |id| {
        let (x, y, width, height) = frame_of(&w, id);
        (x + width / 2.0, y + height / 2.0)
    };
    let (a, b) = (centre(ids[0]), centre(ids[1]));
    assert!(a.0 > 3.0 && a.0 < 8.0 && a.1 > 2.0 && a.1 < 7.0, "near the middle of the page: {a:?}");
    assert!((a.0 - b.0).abs() > 0.1 || (a.1 - b.1).abs() > 0.1, "the second is not on the first: {a:?} {b:?}");
}

/// The tiles that have a master are enabled, and they are exactly the masters `stencil.list` gives.
#[test]
fn the_enabled_tiles_are_the_masters_of_the_stencil() {
    let w = edit_window([1280.0, 900.0]);
    let mut session = Session::new();
    let listed = session.execute("stencil.list", &json!({})).unwrap();
    let names: Vec<String> = listed["stencils"][0]["masters"].as_array().unwrap().iter().map(|m| m["name"].as_str().unwrap().to_owned()).collect();
    assert_eq!(names.len(), 12);
    let tiles: Vec<(String, bool)> =
        w.state().elements().iter().filter_map(|e| e.name.strip_prefix("shapes.tile.").map(|n| (n.to_owned(), e.enabled))).collect();
    assert_eq!(tiles.len(), 12, "only actual masters are drawn");
    let enabled: Vec<&String> = tiles.iter().filter(|(_, on)| *on).map(|(n, _)| n).collect();
    assert_eq!(enabled.len(), 12);
    for name in &names {
        assert!(enabled.contains(&name), "{name} has a master and should be enabled");
    }
    for quiet in ["Star", "Cross", "Arrow", "Cylinder"] {
        assert!(!tiles.iter().any(|(n, _)| n == quiet), "{quiet} has no master and is omitted");
    }
}

fn centre_of(w: &Window, id: u32) -> (f64, f64) {
    let (x, y, width, height) = frame_of(w, id);
    (x + width / 2.0, y + height / 2.0)
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

fn connects(w: &Window) -> Vec<(u32, Option<u32>, u32)> {
    let page = w.state().session().document().unwrap().pages.get(0).unwrap();
    page.connects.iter().map(|c| (c.from_sheet, c.from_part, c.to_sheet)).collect()
}

fn undo(w: &mut Window) {
    key(w, Key::Z, Modifiers::COMMAND);
}

/// Click selects, Shift-click adds, a click on the bare page clears, and so does Escape. Selecting is not an edit.
#[test]
fn click_selects_shift_click_adds_and_the_bare_page_and_escape_clear() {
    let mut w = edit_window([1280.0, 900.0]);
    let ids = three_shapes(&mut w);
    assert_eq!(ids.len(), 3);
    let revision = w.state().session().revision();
    click_at(&mut w, 3.0, 6.0);
    assert_eq!(w.state().selection(), &ids[..1]);
    click_with(&mut w, (7.0, 6.0), Modifiers::SHIFT);
    assert_eq!(w.state().selection(), &ids[..2], "Shift adds");
    click_with(&mut w, (3.0, 6.0), Modifiers::SHIFT);
    assert_eq!(w.state().selection(), &ids[1..2], "Shift on a selected shape drops it");
    click_at(&mut w, 5.0, 3.0);
    assert_eq!(w.state().selection(), &ids[2..], "a plain click selects only that one");
    // A diamond is not hit at the corner of its box.
    click_at(&mut w, 7.7, 6.45);
    assert!(w.state().selection().is_empty(), "the corner of the diamond's box is bare page");
    click_at(&mut w, 3.0, 6.0);
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert!(w.state().selection().is_empty(), "Escape clears");
    assert_eq!(w.state().session().revision(), revision, "selecting changes nothing in the document");
}

/// A rubber band from empty page takes the shapes wholly inside it.
#[test]
fn a_rubber_band_selects_the_shapes_wholly_inside() {
    let mut w = edit_window([1280.0, 900.0]);
    let ids = three_shapes(&mut w);
    // Around the rectangle and the diamond (the row at 6), not the ellipse at (5, 3); one corner of the ellipse's box is inside, not all.
    drag_with(&mut w, (1.5, 7.2), (8.6, 4.2), Modifiers::NONE);
    assert_eq!(w.state().selection(), &ids[..2]);
    // Partly inside is not inside.
    drag_with(&mut w, (1.5, 7.2), (3.2, 5.5), Modifiers::NONE);
    assert!(w.state().selection().is_empty());
    key(&mut w, Key::A, Modifiers::COMMAND);
    assert_eq!(w.state().selection(), ids.as_slice(), "Ctrl+A selects all");
}

/// A drag moves the selection by the drag, with the shape pressed on landing on the grid; nothing is sent until the button is up, and
/// the whole move is one undo step.
#[test]
fn dragging_a_shape_moves_it_and_the_command_goes_on_release() {
    let mut w = edit_window([1280.0, 900.0]);
    let ids = three_shapes(&mut w);
    let before = w.state().session().revision();
    let from = at(&w, 3.0, 6.0);
    for _ in 0..3 {
        move_to(&mut w, from);
    }
    button(&mut w, from, true);
    for step in 1..=5 {
        move_to(&mut w, from + vec2(1.04 * 96.0, 0.52 * 96.0) * (step as f32 / 5.0));
    }
    assert!(w.state().edit_dragging(), "the drag is on");
    assert_eq!(w.state().session().revision(), before, "nothing is sent while the button is down");
    assert_eq!(centre_of(&w, ids[0]), (3.0, 6.0), "and the shape has not moved");
    button(&mut w, from + vec2(1.04 * 96.0, 0.52 * 96.0), false);
    w.rest();
    // 1.04 right and 0.52 down; the centre lands on the quarter-inch grid at (4, 5.5), 3.8 and 1.9 pixels from the lines, within the grid's 5.
    let (x, y) = centre_of(&w, ids[0]);
    assert!(near(x, 4.0) && near(y, 5.5), "({x}, {y})");
    assert_eq!(w.state().session().revision(), before + 1, "one command");
    assert_eq!(w.state().selection(), &ids[..1]);
    undo(&mut w);
    assert!(near(centre_of(&w, ids[0]).0, 3.0), "one undo puts it back");
}

/// Shift holds a move to one axis, and Escape in the middle of a drag drops it.
#[test]
fn shift_holds_a_move_to_one_axis_and_escape_drops_the_drag() {
    let mut w = edit_window([1280.0, 900.0]);
    let ids = three_shapes(&mut w);
    drag_with(&mut w, (3.0, 6.0), (4.5, 5.0), Modifiers::SHIFT);
    let (x, y) = centre_of(&w, ids[0]);
    assert!(near(x, 4.5) && near(y, 6.0), "({x}, {y}): only the larger move is made");
    let before = w.state().session().revision();
    let from = at(&w, 4.5, 6.0);
    for _ in 0..3 {
        move_to(&mut w, from);
    }
    button(&mut w, from, true);
    move_to(&mut w, from + vec2(120.0, 40.0));
    assert!(w.state().edit_dragging());
    key(&mut w, Key::Escape, Modifiers::NONE);
    button(&mut w, from + vec2(120.0, 40.0), false);
    w.rest();
    assert_eq!(w.state().session().revision(), before, "Escape dropped the drag: no command");
}

/// A handle resizes the one selected shape; Shift on a corner keeps its proportions.
#[test]
fn a_handle_resizes_the_selected_shape() {
    let mut w = edit_window([1280.0, 900.0]);
    let ids = three_shapes(&mut w);
    click_at(&mut w, 3.0, 6.0);
    // The rectangle is 1.5 by 1 about (3, 6): its east handle is at (3.75, 6).
    let before = w.state().session().revision();
    drag_with(&mut w, (3.75, 6.0), (4.8, 6.3), Modifiers::NONE);
    let (x, y, width, height) = frame_of(&w, ids[0]);
    assert!(
        near(x, 2.25) && near(y, 5.5) && near(width, 2.5) && near(height, 1.0),
        "({x}, {y}) {width} x {height}: only the east edge moved, to the grid"
    );
    assert_eq!(w.state().session().revision(), before + 1);
    // The north-east corner with Shift keeps the 2.5 : 1 proportions.
    drag_with(&mut w, (4.75, 6.5), (6.0, 7.0), Modifiers::SHIFT);
    let (_, _, width, height) = frame_of(&w, ids[0]);
    assert!((width / height - 2.5).abs() < 1e-6, "{width} x {height}");
    undo(&mut w);
    undo(&mut w);
    assert!(near(frame_of(&w, ids[0]).2, 1.5), "two undos, two steps");
}

/// The Connector tool joins two shapes by a drag from one to the other, glued at both ends; the tool stays on until Esc.
#[test]
fn the_connector_tool_joins_two_shapes_and_stays_on_until_escape() {
    let mut w = edit_window([1280.0, 900.0]);
    let ids = three_shapes(&mut w);
    let tool = w.state().element("ribbon.Home.ConnectorTool").expect("the tool is on the Home tab").clone();
    assert!(tool.enabled);
    click(&mut w, tool.rect.center());
    w.rest();
    assert_eq!(w.state().tool().name(), "connector");
    drag_with(&mut w, (3.0, 6.0), (7.0, 6.0), Modifiers::NONE);
    let ids_after = shape_ids(&w);
    assert_eq!(ids_after.len(), 4, "a connector was added");
    let joins = connects(&w);
    assert_eq!(joins.len(), 2);
    assert!(
        joins.iter().any(|(from, part, to)| *from == ids_after[3] && *part == Some(9) && *to == ids[0]),
        "begin glued to the rectangle: {joins:?}"
    );
    assert!(joins.iter().any(|(from, part, to)| *from == ids_after[3] && *part == Some(12) && *to == ids[1]), "end glued to the diamond: {joins:?}");
    assert_eq!(w.state().selection(), &ids_after[3..], "the new connector is selected");
    assert_eq!(w.state().tool().name(), "connector", "the tool stays on");
    // A second one from the diamond to the ellipse.
    drag_with(&mut w, (7.0, 6.0), (5.0, 3.0), Modifiers::NONE);
    assert_eq!(shape_ids(&w).len(), 5);
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert_eq!(w.state().tool().name(), "pointer", "Escape returns to the Pointer Tool");
    undo(&mut w);
    undo(&mut w);
    assert_eq!(shape_ids(&w).len(), 3, "each connector was one undo step");
}

/// A drag that ends on nothing, or a click, joins nothing, and one that comes back to its own shape does not either.
#[test]
fn a_join_needs_somewhere_to_go() {
    let mut w = edit_window([1280.0, 900.0]);
    three_shapes(&mut w);
    run_command(&mut w, "tool.connector", json!({}));
    let before = w.state().session().revision();
    click_at(&mut w, 3.0, 6.0);
    drag_with(&mut w, (3.0, 6.0), (3.1, 6.05), Modifiers::NONE);
    assert_eq!(w.state().session().revision(), before, "a click and a very short drag join nothing");
    // From a shape to bare page: a connector with a free end.
    drag_with(&mut w, (3.0, 6.0), (3.0, 4.5), Modifiers::NONE);
    assert_eq!(shape_ids(&w).len(), 4);
    assert_eq!(connects(&w).len(), 1, "glued at the shape's end only");
}

/// Dragging an end of the selected connector to another shape glues it there; moving a shape reroutes the connectors glued to it.
#[test]
fn an_end_is_dragged_to_another_shape_and_a_moved_shape_reroutes_its_connector() {
    let mut w = edit_window([1280.0, 900.0]);
    let ids = three_shapes(&mut w);
    run_command(&mut w, "tool.connector", json!({}));
    drag_with(&mut w, (3.0, 6.0), (7.0, 6.0), Modifiers::NONE);
    let connector = shape_ids(&w)[3];
    run_command(&mut w, "tool.pointer", json!({}));
    let before = frame_of(&w, connector);
    // Move the diamond: the connector's end follows it.
    drag_with(&mut w, (7.0, 6.0), (7.0, 7.0), Modifiers::NONE);
    assert!(near(centre_of(&w, ids[1]).1, 7.0));
    let after = frame_of(&w, connector);
    assert!(after != before, "the connector was rerouted: {before:?} -> {after:?}");
    // Select the connector and drag its end to the ellipse.
    run_command(&mut w, "edit.select", json!({"ids": [connector]}));
    w.rest();
    let end = w.state().connector_end_point(connector, true).expect("an end handle");
    let end_on_page = w.state().page_point_at(end).unwrap();
    drag_with(&mut w, end_on_page, (5.0, 3.5), Modifiers::NONE);
    let joins = connects(&w);
    assert!(
        joins.iter().any(|(from, part, to)| *from == connector && *part == Some(12) && *to == ids[2]),
        "the end is glued to the ellipse now: {joins:?}"
    );
    assert!(joins.iter().any(|(from, part, to)| *from == connector && *part == Some(9) && *to == ids[0]), "the begin stayed: {joins:?}");
}

/// Double-click opens the text box over the shape with its text selected; typing replaces it and Escape keeps it, in one command.
#[test]
fn double_click_edits_text_in_place_and_escape_keeps_it() {
    let mut w = edit_window([1280.0, 900.0]);
    let ids = three_shapes(&mut w);
    let before = w.state().session().revision();
    click_at(&mut w, 3.0, 6.0);
    click_at(&mut w, 3.0, 6.0);
    assert!(w.state().window_state()["editingText"].as_bool().unwrap(), "the text box is open");
    assert!(w.state().element("shape.text").is_some(), "and was drawn over the shape");
    type_text(&mut w, "Start");
    assert_eq!(w.state().session().revision(), before, "nothing is sent while the box is open");
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert!(!w.state().window_state()["editingText"].as_bool().unwrap());
    let doc = w.state().session().document().unwrap();
    assert_eq!(diacraft_ui_egui::edit::plain_text(doc, doc.pages.get(0).unwrap().id, ids[0]), "Start");
    assert_eq!(w.state().session().revision(), before + 1, "one command");
    // A second edit that changes nothing sends nothing.
    key(&mut w, Key::F2, Modifiers::NONE);
    assert!(w.state().window_state()["editingText"].as_bool().unwrap(), "F2 opens it");
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert_eq!(w.state().session().revision(), before + 1);
    undo(&mut w);
    let doc = w.state().session().document().unwrap();
    assert_eq!(diacraft_ui_egui::edit::plain_text(doc, doc.pages.get(0).unwrap().id, ids[0]), "", "undo takes the text away");
}

/// Typing on a selected shape starts the text, and a click away ends it.
#[test]
fn typing_on_a_selected_shape_starts_its_text_and_a_click_away_ends_it() {
    let mut w = edit_window([1280.0, 900.0]);
    let ids = three_shapes(&mut w);
    click_at(&mut w, 7.0, 6.0);
    type_text(&mut w, "Ok?");
    assert!(w.state().window_state()["editingText"].as_bool().unwrap());
    type_text(&mut w, "!");
    click_at(&mut w, 5.0, 3.0);
    let doc = w.state().session().document().unwrap();
    let page = doc.pages.get(0).unwrap().id;
    assert_eq!(diacraft_ui_egui::edit::plain_text(doc, page, ids[1]), "Ok?!");
    assert_eq!(w.state().selection(), &ids[2..], "the click that ended the edit also selected what it was on");
}

/// Delete removes the selection and the connectors glued to it, as one undo step; arrow keys nudge.
#[test]
fn delete_removes_the_selection_with_its_connectors_and_arrows_nudge() {
    let mut w = edit_window([1280.0, 900.0]);
    let ids = three_shapes(&mut w);
    run_command(&mut w, "tool.connector", json!({}));
    drag_with(&mut w, (3.0, 6.0), (7.0, 6.0), Modifiers::NONE);
    run_command(&mut w, "tool.pointer", json!({}));
    click_at(&mut w, 5.0, 3.0);
    key(&mut w, Key::ArrowRight, Modifiers::NONE);
    // An arrow key goes to the next place the shape can snap to: the next tick of the ruler, an eighth of an inch at 100%, and not a grid pitch.
    assert!(near(centre_of(&w, ids[2]).0, 5.125), "a press goes to the next ruler tick: {:?}", centre_of(&w, ids[2]));
    key(&mut w, Key::ArrowUp, Modifiers::SHIFT);
    let up = centre_of(&w, ids[2]).1 - 3.0;
    assert!(up > 0.005 && up < 0.02, "Shift makes it a point ({up})");
    // Delete the rectangle: its connector goes too.
    click_at(&mut w, 3.0, 6.0);
    key(&mut w, Key::Delete, Modifiers::NONE);
    let left = shape_ids(&w);
    assert_eq!(left, ids[1..].to_vec(), "the rectangle and the connector are gone");
    assert!(w.state().selection().is_empty());
    undo(&mut w);
    assert_eq!(shape_ids(&w).len(), 4, "one undo brings both back");
    assert_eq!(connects(&w).len(), 2);
}

/// Nothing happens to a text box's keys: Delete in the box does not delete the shape.
#[test]
fn keys_in_the_text_box_do_not_reach_the_drawing() {
    let mut w = edit_window([1280.0, 900.0]);
    let ids = three_shapes(&mut w);
    click_at(&mut w, 3.0, 6.0);
    key(&mut w, Key::F2, Modifiers::NONE);
    key(&mut w, Key::Delete, Modifiers::NONE);
    key(&mut w, Key::Z, Modifiers::COMMAND);
    assert_eq!(shape_ids(&w), ids, "the shape is still there, and so is the drop that Ctrl+Z would have undone");
    key(&mut w, Key::Escape, Modifiers::NONE);
}

/// A Mac keyboard's delete key sends Backspace, so on a Mac Backspace deletes the selection as Delete does, while no text is being edited; Forward
/// Delete (Fn+Delete) still does. With a shape's text open Backspace deletes a character and the shape stays. On Windows and Linux Backspace does nothing to
/// the shapes selected: the shortcut list names Delete alone.
#[test]
fn on_a_mac_the_delete_key_deletes_the_selection_and_backspace_does_not_elsewhere() {
    let mut w = edit_window([1280.0, 900.0]);
    let ids = three_shapes(&mut w);
    // One click: a second one on the same shape so soon would be a double click, which opens its text.
    click_at(&mut w, 3.0, 6.0);
    for os in [egui::os::OperatingSystem::Windows, egui::os::OperatingSystem::Nix] {
        w.ctx.set_os(os);
        key(&mut w, Key::Backspace, Modifiers::NONE);
        assert_eq!(shape_ids(&w), ids, "{os:?}: Backspace leaves the shapes selected alone");
        assert_eq!(w.state().selection(), &ids[..1]);
    }
    w.ctx.set_os(egui::os::OperatingSystem::Mac);
    assert_eq!(w.state().window_state()["editingText"], false);
    key(&mut w, Key::Backspace, Modifiers::NONE);
    assert_eq!(shape_ids(&w), ids[1..].to_vec(), "the Mac's delete key deletes the rectangle");
    assert!(w.state().selection().is_empty());
    // Forward Delete still deletes.
    click_at(&mut w, 7.0, 6.0);
    key(&mut w, Key::Delete, Modifiers::NONE);
    assert_eq!(shape_ids(&w), ids[2..].to_vec(), "Forward Delete deletes the diamond");
    // With the ellipse's text open, Backspace deletes the character before the cursor.
    click_at(&mut w, 5.0, 3.0);
    key(&mut w, Key::F2, Modifiers::NONE);
    type_text(&mut w, "ab");
    key(&mut w, Key::Backspace, Modifiers::NONE);
    assert_eq!(w.state().edited_text(), Some("a"), "Backspace in the text deletes a character");
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert_eq!(shape_ids(&w), ids[2..].to_vec(), "and the shape stays");
    assert_eq!(diacraft_ui_egui::edit::plain_text(w.state().session().document().unwrap(), page_id(&w), ids[2]), "a");
    // Back on Windows, Delete is the key. (Selected by its command: a click on the ellipse again so soon could be a double click.)
    w.ctx.set_os(egui::os::OperatingSystem::Windows);
    run_command(&mut w, "edit.select", json!({"ids": [ids[2]]}));
    assert_eq!((w.state().selection(), &w.state().window_state()["editingText"]), (&ids[2..], &json!(false)));
    key(&mut w, Key::Backspace, Modifiers::NONE);
    assert_eq!(shape_ids(&w), ids[2..].to_vec());
    key(&mut w, Key::Delete, Modifiers::NONE);
    assert!(shape_ids(&w).is_empty(), "Delete deletes on Windows");
}

/// The tool buttons on the ribbon, Ctrl+1 and Ctrl+3, and Select All all go through the window's commands.
#[test]
fn the_tool_controls_and_their_shortcuts_run_the_tool_commands() {
    let mut w = edit_window([1280.0, 900.0]);
    three_shapes(&mut w);
    key(&mut w, Key::Num3, Modifiers::COMMAND);
    assert_eq!(w.state().tool().name(), "connector");
    key(&mut w, Key::Num1, Modifiers::COMMAND);
    assert_eq!(w.state().tool().name(), "pointer");
    let connector = w.state().element("ribbon.Home.ConnectorTool").unwrap().rect.center();
    click(&mut w, connector);
    w.rest();
    assert_eq!(w.state().tool().name(), "connector");
    let pointer = w.state().element("ribbon.Home.PointerTool").unwrap().rect.center();
    click(&mut w, pointer);
    w.rest();
    assert_eq!(w.state().tool().name(), "pointer");
    assert_eq!(w.state().window_state()["tool"], "pointer");
}

/// A move, a resize, a join, a text and a delete are each undone by the toolbar's Undo, and redone by Redo.
#[test]
fn undo_and_redo_on_the_toolbar_cover_every_edit() {
    let mut w = edit_window([1280.0, 900.0]);
    let ids = three_shapes(&mut w);
    drag_with(&mut w, (3.0, 6.0), (3.0, 7.0), Modifiers::NONE);
    click_at(&mut w, 3.0, 7.0);
    drag_with(&mut w, (3.75, 7.0), (4.25, 7.0), Modifiers::NONE);
    key(&mut w, Key::F2, Modifiers::NONE);
    type_text(&mut w, "A");
    key(&mut w, Key::Escape, Modifiers::NONE);
    key(&mut w, Key::Delete, Modifiers::NONE);
    assert_eq!(shape_ids(&w), ids[1..].to_vec());
    let steps = w.state().session().history().map(|h| h.undo_len()).unwrap();
    let qat_undo = |w: &mut Window| {
        let at = w.state().element("qat.Undo").unwrap().rect.center();
        click(w, at);
        w.rest();
    };
    for _ in 0..4 {
        qat_undo(&mut w);
    }
    assert_eq!(shape_ids(&w), ids, "everything after the three drops is undone");
    assert!(near(centre_of(&w, ids[0]).1, 6.0) && near(frame_of(&w, ids[0]).2, 1.5));
    assert_eq!(w.state().session().history().map(|h| h.undo_len()).unwrap(), steps - 4);
    for _ in 0..4 {
        let at = w.state().element("qat.Redo").unwrap().rect.center();
        click(&mut w, at);
        w.rest();
    }
    assert_eq!(shape_ids(&w), ids[1..].to_vec());
}

/// The status bar names the width, height and angle of the one selected shape, and nothing for none or several.
#[test]
fn the_status_bar_gives_the_size_of_the_one_selected_shape() {
    let mut w = edit_window([1280.0, 900.0]);
    three_shapes(&mut w);
    assert!(w.state().selection_summary().is_some(), "the last drop is selected");
    assert_eq!(w.state().selection_summary().unwrap(), "Width: 1.50 in   Height: 1.00 in   Angle: 0\u{b0}");
    assert!(w.state().element("status.selection").is_some());
    key(&mut w, Key::A, Modifiers::COMMAND);
    assert!(w.state().selection_summary().is_none(), "several shapes: no size");
    assert!(w.state().element("status.selection").is_none());
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert!(w.state().element("status.selection").is_none(), "none selected: none shown");
}

/// A handle's drag changes the shape's size, and the status bar then gives the new one.
#[test]
fn a_resize_changes_the_size_the_status_bar_gives() {
    let mut w = edit_window([1280.0, 900.0]);
    drag_tile_to(&mut w, "Square", 5.0, 5.0);
    let id = shape_ids(&w)[0];
    assert_eq!(w.state().selection_summary().unwrap(), "Width: 1.00 in   Height: 1.00 in   Angle: 0\u{b0}");
    // The south-east corner of the 1 inch square, taken to (4.7, 5.3): a quarter inch square.
    drag_with(&mut w, (5.5, 4.5), (4.7, 5.3), Modifiers::NONE);
    let (_, _, width, height) = frame_of(&w, id);
    assert!(near(width, 0.25) && near(height, 0.25), "{width} x {height}");
    assert_eq!(w.state().selection_summary().unwrap(), "Width: 0.25 in   Height: 0.25 in   Angle: 0\u{b0}");
}
