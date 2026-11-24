//! The window's AutoConnect.
//!
//! With AutoConnect on, the pointer resting on a shape shows four arrows round it. A click on an arrow joins the nearest neighbour in its direction or makes a
//! copy there and joins it; resting on an arrow shows the flyout of up to four masters of the open stencil, with a preview on the page of the one under the
//! pointer, and a click on one drops it there, joined; a drag from an arrow onto a shape joins the two. Every change is a `connector.autoconnect` command. The
//! window's clock is the test's (`clock.rs`): the delays pass when the test lets them.

use std::time::Duration;

use diacraft_ui_egui::edit::Tool;
use diacraft_ui_egui::edit::autoconnect::{ARROWS_DELAY, FLYOUT_DELAY, FLYOUT_GRACE, MAX_FLYOUT};
use diacraft_ui_egui::theme::Tokens;
use egui::{Color32, Key, Modifiers, Pos2, Rect, vec2};
use egui_kittest::kittest::{NodeT, Queryable};
use serde_json::json;

use crate::editing::{at, edit_window, frame_of, key, shape_ids};
use crate::saving::ctrl;
use crate::support::{Rest, Window, button, click, move_to, run_command};

fn page_id(w: &Window) -> u32 {
    w.state().session().document().unwrap().pages.get(0).unwrap().id
}

/// A rectangle (1.5 by 1) centred at `(x, y)`, dropped by the command. → its `ID`.
fn rectangle_at(w: &mut Window, x: f64, y: f64) -> u32 {
    let page = page_id(w);
    let answer = w.state_mut().execute("shape.drop", &json!({"master": "rectangle", "page": page, "x": x, "y": y})).unwrap();
    w.rest();
    u32::try_from(answer["id"].as_u64().unwrap()).unwrap()
}

/// The arrows the last frame drew, by side, with their rectangles on the screen.
fn arrows(w: &Window) -> Vec<(String, Rect)> {
    let mut found: Vec<(String, Rect)> =
        w.state().elements().iter().filter_map(|e| e.name.strip_prefix("autoconnect.arrow.").map(|side| (side.to_owned(), e.rect))).collect();
    found.sort_by(|a, b| a.0.cmp(&b.0));
    found
}

fn sides(w: &Window) -> Vec<String> {
    arrows(w).into_iter().map(|(side, _)| side).collect()
}

fn arrow(w: &Window, side: &str) -> Pos2 {
    arrows(w).into_iter().find(|(s, _)| s == side).unwrap_or_else(|| panic!("no {side} arrow: {:?}", sides(w))).1.center()
}

/// Every shape the last frame painted, the `Shape::Vec`s opened.
fn painted(w: &Window) -> Vec<egui::Shape> {
    fn open(shape: &egui::Shape, into: &mut Vec<egui::Shape>) {
        match shape {
            egui::Shape::Vec(inner) => inner.iter().for_each(|s| open(s, into)),
            other => into.push(other.clone()),
        }
    }
    let mut found = Vec::new();
    for clipped in &w.output().shapes {
        open(&clipped.shape, &mut found);
    }
    found
}

/// The fills of the triangles of the arrows the last frame painted: the three-point filled paths in one of the two arrow colours of `tokens`.
fn arrow_fills(w: &Window, tokens: &Tokens) -> Vec<Color32> {
    painted(w)
        .into_iter()
        .filter_map(|shape| match shape {
            egui::Shape::Path(path) if path.points.len() == 3 && (path.fill == tokens.auto_arrow || path.fill == tokens.auto_arrow_hot) => {
                Some(path.fill)
            }
            _ => None,
        })
        .collect()
}

/// The circles the last frame painted in one of the two arrow colours of `tokens`: a circle handle would be one.
fn arrow_coloured_circles(w: &Window, tokens: &Tokens) -> usize {
    painted(w).iter().filter(|s| matches!(s, egui::Shape::Circle(c) if c.fill == tokens.auto_arrow || c.fill == tokens.auto_arrow_hot)).count()
}

/// Whether the last frame painted a text that reads `words`.
fn painted_text(w: &Window, words: &str) -> bool {
    painted(w).iter().any(|s| matches!(s, egui::Shape::Text(t) if t.galley.text() == words))
}

/// What the connector's end is glued to: its `Connect` row's cell and part, from the page.
fn end_glue(w: &Window, connector: u32) -> Option<(u32, Option<String>, Option<u32>)> {
    let page = w.state().session().document().unwrap().pages.get(0).unwrap();
    page.connects
        .iter()
        .find(|c| c.from_sheet == connector && c.from_cell.as_deref() == Some("EndX"))
        .map(|c| (c.to_sheet, c.to_cell.as_deref().map(str::to_owned), c.to_part))
}

/// The pointer rests at `pos` for a few frames.
fn rest_at(w: &mut Window, pos: Pos2) {
    for _ in 0..3 {
        move_to(w, pos);
    }
}

/// The pointer rests on the shape at `pos` until the arrows are due, and a frame draws them.
fn show_arrows(w: &mut Window, pos: Pos2) {
    rest_at(w, pos);
    w.advance(ARROWS_DELAY);
    w.step();
}

/// The pointer goes from where it is to `to` in six moves.
fn glide(w: &mut Window, from: Pos2, to: Pos2) {
    for step in 1..=6 {
        move_to(w, from + (to - from) * (step as f32 / 6.0));
    }
}

/// The shapes a connector is glued to, begin then end, from the page's `Connect` rows.
fn glued(w: &Window, connector: u32) -> (Option<u32>, Option<u32>) {
    let page = w.state().session().document().unwrap().pages.get(0).unwrap();
    let to = |cell: &str| page.connects.iter().find(|c| c.from_sheet == connector && c.from_cell.as_deref() == Some(cell)).map(|c| c.to_sheet);
    (to("BeginX"), to("EndX"))
}

fn flyout(w: &Window) -> Option<(String, Vec<String>)> {
    w.state().autoconnect_flyout()
}

fn flyout_tiles(w: &Window) -> Vec<Rect> {
    let mut tiles: Vec<(String, Rect)> =
        w.state().elements().iter().filter_map(|e| e.name.strip_prefix("autoconnect.flyout.").map(|n| (n.to_owned(), e.rect))).collect();
    tiles.sort_by(|a, b| a.0.cmp(&b.0));
    tiles.into_iter().map(|(_, r)| r).collect()
}

#[test]
fn the_arrows_show_after_the_delay_on_the_shape_the_pointer_is_on_and_go_when_it_leaves() {
    let mut w = edit_window([1100.0, 800.0]);
    rectangle_at(&mut w, 4.0, 4.0);
    let middle = at(&w, 4.0, 4.0);
    rest_at(&mut w, middle);
    assert!(arrows(&w).is_empty(), "not at once: the pointer has only just come");
    w.advance(ARROWS_DELAY - Duration::from_millis(1));
    w.step();
    assert!(arrows(&w).is_empty(), "not a millisecond early");
    w.advance(Duration::from_millis(1));
    w.step();
    assert_eq!(sides(&w), ["down", "left", "right", "up"]);
    // Each is outside the shape's box, at the middle of its side.
    let (left, top, right, bottom) = (at(&w, 3.25, 4.5).x, at(&w, 3.25, 4.5).y, at(&w, 4.75, 3.5).x, at(&w, 4.75, 3.5).y);
    let (r, l, u, d) = (arrow(&w, "right"), arrow(&w, "left"), arrow(&w, "up"), arrow(&w, "down"));
    assert!(r.x > right && (r.y - middle.y).abs() < 1.0, "right: {r:?} for the box {left}..{right}");
    assert!(l.x < left && (l.y - middle.y).abs() < 1.0, "left: {l:?}");
    assert!(u.y < top && (u.x - middle.x).abs() < 1.0, "up: {u:?} (screen y runs down)");
    assert!(d.y > bottom && (d.x - middle.x).abs() < 1.0, "down: {d:?}");
    // The pointer goes to bare page: the arrows go with it.
    let away = at(&w, 8.0, 7.0);
    move_to(&mut w, away);
    w.step();
    assert!(arrows(&w).is_empty());
}

#[test]
fn the_arrows_stay_while_the_pointer_goes_from_the_shape_to_one_of_them() {
    let mut w = edit_window([1100.0, 800.0]);
    rectangle_at(&mut w, 4.0, 4.0);
    let middle = at(&w, 4.0, 4.0);
    show_arrows(&mut w, middle);
    let target = arrow(&w, "right");
    for step in 1..=8 {
        move_to(&mut w, middle + (target - middle) * (step as f32 / 8.0));
        assert_eq!(arrows(&w).len(), 4, "step {step}: the arrows are still there");
    }
}

#[test]
fn arrows_show_only_with_autoconnect_on_in_the_drawing_and_in_the_program_with_the_pointer_tool() {
    let mut w = edit_window([1100.0, 800.0]);
    rectangle_at(&mut w, 4.0, 4.0);
    let middle = at(&w, 4.0, 4.0);
    show_arrows(&mut w, middle);
    assert_eq!(arrows(&w).len(), 4);
    // The drawing's box off.
    w.state_mut().execute("autoconnect.set", &json!({"on": false})).unwrap();
    w.rest();
    show_arrows(&mut w, middle);
    assert!(arrows(&w).is_empty(), "the drawing's AutoConnect is off");
    w.state_mut().execute("autoconnect.set", &json!({"on": true})).unwrap();
    w.rest();
    show_arrows(&mut w, middle);
    assert_eq!(arrows(&w).len(), 4);
    // The program-wide setting off (File, Options, Advanced: Enable AutoConnect).
    w.state_mut().execute("window.enableAutoConnect", &json!({"on": false})).unwrap();
    w.rest();
    show_arrows(&mut w, middle);
    assert!(arrows(&w).is_empty(), "the program's setting is off");
    w.state_mut().execute("window.enableAutoConnect", &json!({"on": true})).unwrap();
    w.rest();
    show_arrows(&mut w, middle);
    assert_eq!(arrows(&w).len(), 4);
    // The Connector tool has its own way of joining.
    w.state_mut().set_tool(Tool::Connector);
    w.rest();
    show_arrows(&mut w, middle);
    assert!(arrows(&w).is_empty(), "no arrows with the Connector tool");
}

#[test]
fn a_shape_being_dragged_and_a_connector_and_a_group_show_no_arrows() {
    let mut w = edit_window([1100.0, 800.0]);
    let a = rectangle_at(&mut w, 3.0, 4.0);
    let b = rectangle_at(&mut w, 7.0, 4.0);
    let c = rectangle_at(&mut w, 7.0, 6.5);
    let joined = w.state_mut().execute("connector.add", &json!({"from": {"id": a}, "to": {"id": b}})).unwrap();
    let connector = joined["id"].as_u64().unwrap();
    w.rest();
    // The middle of the connector between the two shapes: no arrows on it.
    let on_connector = at(&w, 5.0, 4.0);
    show_arrows(&mut w, on_connector);
    assert!(arrows(&w).is_empty(), "connector {connector} has none");
    // A drag of a shape in progress.
    let on_a = at(&w, 3.0, 4.0);
    rest_at(&mut w, on_a);
    button(&mut w, on_a, true);
    move_to(&mut w, on_a + vec2(20.0, 10.0));
    w.advance(ARROWS_DELAY);
    move_to(&mut w, on_a + vec2(25.0, 12.0));
    assert!(arrows(&w).is_empty(), "no arrows while a shape is dragged");
    button(&mut w, on_a + vec2(25.0, 12.0), false);
    w.rest();
    // A group has no connection point to glue a connector to.
    let d = rectangle_at(&mut w, 3.0, 6.5);
    w.state_mut().execute("shape.group", &json!({"ids": [c, d]})).unwrap();
    w.rest();
    let on_group = at(&w, 7.0, 6.5);
    show_arrows(&mut w, on_group);
    assert!(arrows(&w).is_empty(), "a group has no arrows");
}

#[test]
fn a_click_on_an_arrow_with_no_neighbour_makes_a_copy_beside_the_shape_and_joins_it() {
    let mut w = edit_window([1100.0, 800.0]);
    let a = rectangle_at(&mut w, 4.0, 4.0);
    let on = at(&w, 4.0, 4.0);
    show_arrows(&mut w, on);
    let target = arrow(&w, "right");
    let revision = w.state().session().revision();
    click(&mut w, target);
    w.rest();
    let ids = shape_ids(&w);
    assert_eq!(ids.len(), 3, "the copy and the connector: {ids:?}");
    let copy = ids[1];
    assert_eq!(w.state().session().revision(), revision + 1, "one command, one undo step");
    // The copy is 0.75 inch beyond the right side, level with the shape and as big.
    let (x, y, width, height) = frame_of(&w, copy);
    assert!(
        (x - 5.5).abs() < 1e-6 && (y - 3.5).abs() < 1e-6 && (width - 1.5).abs() < 1e-6 && (height - 1.0).abs() < 1e-6,
        "{x} {y} {width} {height}"
    );
    assert_eq!(glued(&w, ids[2]), (Some(a), Some(copy)));
    assert_eq!(w.state().selection(), [copy], "the new shape is selected, to be written in");
    // One undo takes the copy and its connector back.
    run_command(&mut w, "edit.undo", json!({}));
    assert_eq!(shape_ids(&w), [a]);
}

#[test]
fn a_click_on_an_arrow_joins_the_nearest_neighbour_in_that_direction_and_makes_no_shape() {
    let mut w = edit_window([1100.0, 800.0]);
    let a = rectangle_at(&mut w, 3.0, 4.0);
    let b = rectangle_at(&mut w, 6.5, 4.0);
    let on = at(&w, 3.0, 4.0);
    show_arrows(&mut w, on);
    let pressed = arrow(&w, "right");
    click(&mut w, pressed);
    w.rest();
    let ids = shape_ids(&w);
    assert_eq!(ids.len(), 3, "the connector alone is new: {ids:?}");
    let connector = ids[2];
    assert_eq!(glued(&w, connector), (Some(a), Some(b)));
    assert_eq!(w.state().selection(), [connector], "the connector is selected, as the Connector tool leaves it");
    // The shapes are where they were.
    assert!((frame_of(&w, b).0 - 5.75).abs() < 1e-6);
}

#[test]
fn a_press_on_an_arrow_and_a_short_slip_is_still_a_click() {
    let mut w = edit_window([1100.0, 800.0]);
    rectangle_at(&mut w, 4.0, 4.0);
    let on = at(&w, 4.0, 4.0);
    show_arrows(&mut w, on);
    let target = arrow(&w, "up");
    rest_at(&mut w, target);
    button(&mut w, target, true);
    move_to(&mut w, target + vec2(1.0, 1.0));
    button(&mut w, target + vec2(1.0, 1.0), false);
    w.rest();
    assert_eq!(shape_ids(&w).len(), 3, "a copy above and its connector");
}

#[test]
fn a_drag_from_an_arrow_onto_a_shape_joins_the_two_and_onto_bare_page_makes_nothing() {
    let mut w = edit_window([1100.0, 800.0]);
    let a = rectangle_at(&mut w, 3.0, 4.0);
    let c = rectangle_at(&mut w, 7.0, 7.0);
    let on = at(&w, 3.0, 4.0);
    show_arrows(&mut w, on);
    let from = arrow(&w, "up");
    let onto = at(&w, 7.0, 7.0);
    rest_at(&mut w, from);
    button(&mut w, from, true);
    glide(&mut w, from, onto);
    // While the button is down the window knows where the end would be glued.
    assert_eq!(w.state().autoconnect_drag_target(), Some(c), "the drag would join c");
    button(&mut w, onto, false);
    w.rest();
    let ids = shape_ids(&w);
    assert_eq!(ids.len(), 3, "{ids:?}");
    assert_eq!(glued(&w, ids[2]), (Some(a), Some(c)));
    // A second drag, let go over nothing: nothing is made.
    run_command(&mut w, "edit.undo", json!({}));
    let on = at(&w, 3.0, 4.0);
    show_arrows(&mut w, on);
    let from = arrow(&w, "left");
    let bare = at(&w, 1.0, 1.5);
    rest_at(&mut w, from);
    button(&mut w, from, true);
    glide(&mut w, from, bare);
    button(&mut w, bare, false);
    w.rest();
    assert_eq!(shape_ids(&w).len(), 2, "nothing made over bare page");
}

#[test]
fn resting_on_an_arrow_shows_the_flyout_of_the_first_four_masters_of_the_open_stencil() {
    let mut w = edit_window([1100.0, 800.0]);
    rectangle_at(&mut w, 4.0, 4.0);
    let middle = at(&w, 4.0, 4.0);
    show_arrows(&mut w, middle);
    let target = arrow(&w, "right");
    glide(&mut w, middle, target);
    assert!(flyout(&w).is_none(), "not at once");
    w.advance(FLYOUT_DELAY);
    w.step();
    let (side, names) = flyout(&w).expect("the flyout is up");
    assert_eq!(side, "right");
    assert_eq!(names.len(), MAX_FLYOUT, "four, however many masters the stencil has: {names:?}");
    // They are the first four of the stencil the Shapes window shows, none of them a connector.
    let stencils = w.state_mut().stencil_list();
    let first: Vec<String> = stencils[0].masters.iter().filter(|m| !m.one_d).take(MAX_FLYOUT).map(|m| m.name.clone()).collect();
    assert_eq!(names, first);
    assert_eq!(flyout_tiles(&w).len(), MAX_FLYOUT);
    // It stands to the right of the arrow, on the page and in the drawing area.
    let tiles = flyout_tiles(&w);
    assert!(tiles.iter().all(|t| t.left() >= target.x), "{tiles:?} for the arrow {target:?}");
    // Another stencil gives its own.
    assert!(w.state_mut().show_stencil("flowchart"));
    w.rest();
    show_arrows(&mut w, middle);
    let target = arrow(&w, "right");
    glide(&mut w, middle, target);
    w.advance(FLYOUT_DELAY);
    w.step();
    let (_, names) = flyout(&w).expect("the flyout is up");
    let stencils = w.state_mut().stencil_list();
    let flow = stencils.iter().find(|s| s.id == "flowchart").unwrap();
    let first: Vec<String> = flow.masters.iter().filter(|m| !m.one_d).take(MAX_FLYOUT).map(|m| m.name.clone()).collect();
    assert_eq!(names, first, "the stencil that is open now");
}

#[test]
fn the_pointer_on_a_tile_previews_the_shape_on_the_page_where_the_engine_would_put_it() {
    let mut w = edit_window([1100.0, 800.0]);
    let a = rectangle_at(&mut w, 4.0, 4.0);
    let middle = at(&w, 4.0, 4.0);
    show_arrows(&mut w, middle);
    let target = arrow(&w, "right");
    glide(&mut w, middle, target);
    w.advance(FLYOUT_DELAY);
    w.step();
    let tile = flyout_tiles(&w)[1];
    assert!(w.state().autoconnect_ghost().is_none(), "no tile is under the pointer yet");
    glide(&mut w, target, tile.center());
    assert_eq!(flyout(&w).map(|f| f.0), Some("right".to_owned()), "the flyout stays while the pointer goes to it");
    let ghost = w.state().autoconnect_ghost().expect("a preview of the second master");
    // It is where the engine's `connector.autoconnect` puts a shape of that size: the same function answers.
    let names = flyout(&w).unwrap().1;
    let stencils = w.state_mut().stencil_list();
    let master = stencils[0].masters.iter().find(|m| m.name == names[1]).unwrap().clone();
    let page = page_id(&w);
    let doc = w.state().session().document().unwrap();
    let k = diacraft_engine::draw::units_per_inch(doc, page);
    let centre =
        diacraft_engine::draw::autoconnect_centre(doc, page, a, diacraft_engine::draw::route::Dir::Right, (master.width, master.height)).unwrap();
    assert!((ghost.0 - centre.0 / k).abs() < 1e-6 && (ghost.1 - centre.1 / k).abs() < 1e-6, "{ghost:?} for {centre:?}");
    // The preview leaves with the pointer.
    glide(&mut w, tile.center(), tile.center() + vec2(0.0, 300.0));
    assert!(w.state().autoconnect_ghost().is_none());
}

#[test]
fn a_click_on_a_tile_drops_that_master_beside_the_shape_joined_to_it() {
    let mut w = edit_window([1100.0, 800.0]);
    let a = rectangle_at(&mut w, 4.0, 4.0);
    let middle = at(&w, 4.0, 4.0);
    show_arrows(&mut w, middle);
    let target = arrow(&w, "down");
    glide(&mut w, middle, target);
    w.advance(FLYOUT_DELAY);
    w.step();
    let names = flyout(&w).unwrap().1;
    let tile = flyout_tiles(&w)[2];
    glide(&mut w, target, tile.center());
    let revision = w.state().session().revision();
    click(&mut w, tile.center());
    w.rest();
    assert_eq!(w.state().session().revision(), revision + 1, "one command: {:?} {names:?} {}", flyout(&w), w.state().status_text());
    let ids = shape_ids(&w);
    assert_eq!(ids.len(), 3, "{ids:?}");
    let new = ids[1];
    assert_eq!(glued(&w, ids[2]), (Some(a), Some(new)));
    assert_eq!(w.state().selection(), [new]);
    let doc = w.state().session().document().unwrap();
    let master_name = doc
        .pages
        .get(0)
        .unwrap()
        .shapes
        .iter()
        .find(|s| s.id == new)
        .and_then(|s| s.master)
        .and_then(|m| doc.masters.iter().find(|x| x.id == m))
        .and_then(|m| m.name.clone());
    assert_eq!(master_name.as_deref(), Some(names[2].as_str()), "the third master of the flyout");
    // Below the shape: its centre is on the shape's vertical, a gap under it.
    let (x, y, width, height) = frame_of(&w, new);
    assert!((x + width / 2.0 - 4.0).abs() < 1e-6 && y + height < 3.5 - 0.7, "{x} {y} {width} {height}");
    assert!(flyout(&w).is_none(), "the flyout goes once a master is chosen");
}

#[test]
fn the_flyout_goes_when_the_pointer_is_off_its_arrow_and_it_for_a_moment() {
    let mut w = edit_window([1100.0, 800.0]);
    rectangle_at(&mut w, 4.0, 4.0);
    let middle = at(&w, 4.0, 4.0);
    show_arrows(&mut w, middle);
    let target = arrow(&w, "right");
    glide(&mut w, middle, target);
    w.advance(FLYOUT_DELAY);
    w.step();
    assert!(flyout(&w).is_some());
    // Back to the shape's middle: the flyout stays for the grace, then goes.
    glide(&mut w, target, middle);
    assert!(flyout(&w).is_some(), "not at once, the pointer may be on its way across");
    w.advance(FLYOUT_GRACE);
    w.step();
    assert!(flyout(&w).is_none());
    assert_eq!(arrows(&w).len(), 4, "the arrows are still there");
    // Escape closes it too.
    glide(&mut w, middle, target);
    w.advance(FLYOUT_DELAY);
    w.step();
    assert!(flyout(&w).is_some());
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert!(flyout(&w).is_none());
}

#[test]
fn the_visual_aids_box_is_wired_to_the_drawings_setting_and_the_programs() {
    let mut w = edit_window([1280.0, 900.0]);
    run_command(&mut w, "window.tab", json!({"tab": "View"}));
    let node = |w: &Window| w.get_by_label("AutoConnect").accesskit_node().toggled();
    assert_eq!(node(&w), Some(egui::accesskit::Toggled::True), "a drawing holds nothing: AutoConnect is on");
    assert!(!w.get_by_label("AutoConnect").accesskit_node().is_disabled());
    assert!(w.state().element("ribbon.View.AutoConnect").unwrap().enabled);
    let revision = w.state().session().revision();
    w.get_by_label("AutoConnect").click();
    w.rest();
    // The box is a View setting, "unsaved, not undo": the drawing is changed, no step is made and no revision moves.
    assert_eq!(w.state().session().revision(), revision, "no revision moves");
    assert_eq!(w.state().session().history().unwrap().undo_len(), 0, "no undo step");
    assert!(w.state().is_dirty(), "the drawing is unsaved");
    assert_eq!(node(&w), Some(egui::accesskit::Toggled::False));
    assert_eq!(w.state_mut().execute("autoconnect.settings", &json!({})).unwrap()["on"], false);
    w.get_by_label("AutoConnect").click();
    w.rest();
    assert_eq!(node(&w), Some(egui::accesskit::Toggled::True));
    w.get_by_label("AutoConnect").click();
    w.rest();
    assert_eq!(node(&w), Some(egui::accesskit::Toggled::False));
    key(&mut w, Key::Z, ctrl());
    assert_eq!(node(&w), Some(egui::accesskit::Toggled::False), "Ctrl+Z has nothing to undo and leaves the box");
    // The program's setting off: the box is greyed, and says so.
    run_command(&mut w, "window.enableAutoConnect", json!({"on": false}));
    assert!(w.get_by_label("AutoConnect").accesskit_node().is_disabled(), "greyed out when the program-wide setting is off");
    assert!(!w.state().element("ribbon.View.AutoConnect").unwrap().enabled);
    run_command(&mut w, "window.enableAutoConnect", json!({"on": true}));
    assert!(!w.get_by_label("AutoConnect").accesskit_node().is_disabled());
}

#[test]
fn alt_with_an_arrow_key_is_a_click_on_that_arrow_for_the_shape_selected() {
    let mut w = edit_window([1100.0, 800.0]);
    let a = rectangle_at(&mut w, 4.0, 4.0);
    w.state_mut().select(&[a]);
    w.rest();
    key(&mut w, Key::ArrowRight, Modifiers::ALT);
    let ids = shape_ids(&w);
    assert_eq!(ids.len(), 3, "a copy on the right and its connector: {ids:?}");
    assert!((frame_of(&w, ids[1]).0 - 5.5).abs() < 1e-6);
    assert_eq!(w.state().selection(), [ids[1]]);
    // With the drawing's box off the key does nothing.
    w.state_mut().execute("autoconnect.set", &json!({"on": false})).unwrap();
    w.state_mut().select(&[a]);
    w.rest();
    key(&mut w, Key::ArrowUp, Modifiers::ALT);
    assert_eq!(shape_ids(&w).len(), 3);
}

#[test]
fn alt_shift_with_an_arrow_key_opens_the_flyout_and_the_arrow_keys_and_enter_choose() {
    let mut w = edit_window([1100.0, 800.0]);
    let a = rectangle_at(&mut w, 4.0, 4.0);
    w.state_mut().select(&[a]);
    w.rest();
    key(&mut w, Key::ArrowRight, Modifiers::ALT | Modifiers::SHIFT);
    let (side, names) = flyout(&w).expect("the flyout is open");
    assert_eq!(side, "right");
    assert_eq!(w.state().autoconnect_current(), Some(0), "the first master has the keyboard");
    key(&mut w, Key::ArrowDown, Modifiers::NONE);
    assert_eq!(w.state().autoconnect_current(), Some(1), "a column beside a right arrow: down goes on");
    key(&mut w, Key::ArrowRight, Modifiers::NONE);
    assert_eq!(w.state().autoconnect_current(), Some(2), "right goes on too");
    key(&mut w, Key::ArrowUp, Modifiers::NONE);
    assert_eq!(w.state().autoconnect_current(), Some(1), "up goes back");
    key(&mut w, Key::ArrowLeft, Modifiers::NONE);
    key(&mut w, Key::ArrowLeft, Modifiers::NONE);
    assert_eq!(w.state().autoconnect_current(), Some(0), "the end stays");
    key(&mut w, Key::ArrowDown, Modifiers::NONE);
    key(&mut w, Key::ArrowDown, Modifiers::NONE);
    assert!(w.state().autoconnect_ghost().is_some(), "the one that has the keyboard is previewed");
    // Escape closes it and leaves the shape selected.
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert!(flyout(&w).is_none());
    assert_eq!(w.state().selection(), [a]);
    // Enter chooses.
    key(&mut w, Key::ArrowUp, Modifiers::ALT | Modifiers::SHIFT);
    key(&mut w, Key::ArrowRight, Modifiers::NONE);
    key(&mut w, Key::Enter, Modifiers::NONE);
    let ids = shape_ids(&w);
    assert_eq!(ids.len(), 3, "{ids:?}");
    let doc = w.state().session().document().unwrap();
    let made = doc
        .pages
        .get(0)
        .unwrap()
        .shapes
        .iter()
        .find(|s| s.id == ids[1])
        .and_then(|s| s.master)
        .and_then(|m| doc.masters.iter().find(|x| x.id == m))
        .and_then(|m| m.name.clone());
    assert_eq!(made.as_deref(), Some(names[1].as_str()));
    assert!(frame_of(&w, ids[1]).1 > 4.5, "above the shape");
}

#[test]
fn a_master_dragged_from_the_shapes_window_onto_an_arrow_is_placed_and_joined() {
    let mut w = edit_window([1100.0, 800.0]);
    let a = rectangle_at(&mut w, 4.0, 4.0);
    // The arrows are showing: the tile is dragged over the shape, and the pointer comes to its arrow.
    let tile = w.state().element("shapes.tile.Ellipse").expect("the tile").clone();
    let start = tile.rect.center();
    let middle = at(&w, 4.0, 4.0);
    rest_at(&mut w, start);
    button(&mut w, start, true);
    glide(&mut w, start, middle);
    w.advance(ARROWS_DELAY);
    w.step();
    let target = arrow(&w, "right");
    glide(&mut w, middle, target);
    button(&mut w, target, false);
    w.rest();
    let ids = shape_ids(&w);
    assert_eq!(ids.len(), 3, "the ellipse and its connector: {ids:?}");
    assert_eq!(glued(&w, ids[2]), (Some(a), Some(ids[1])));
    let doc = w.state().session().document().unwrap();
    let made = doc
        .pages
        .get(0)
        .unwrap()
        .shapes
        .iter()
        .find(|s| s.id == ids[1])
        .and_then(|s| s.master)
        .and_then(|m| doc.masters.iter().find(|x| x.id == m))
        .and_then(|m| m.name.clone());
    assert_eq!(made.as_deref(), Some("Ellipse"));
}

#[test]
fn a_press_on_a_selection_handle_is_still_a_resize_while_the_arrows_are_up() {
    let mut w = edit_window([1100.0, 800.0]);
    let a = rectangle_at(&mut w, 4.0, 4.0);
    w.state_mut().select(&[a]);
    w.rest();
    let middle = at(&w, 4.0, 4.0);
    show_arrows(&mut w, middle);
    // The east handle is on the box's edge; the arrow beyond it is clear of it.
    let edge = at(&w, 4.75, 4.0);
    assert!(arrows(&w).iter().find(|(s, _)| s == "right").unwrap().1.left() > edge.x + 6.0, "the arrow starts beyond the handle's half width");
    let before = frame_of(&w, a);
    rest_at(&mut w, edge);
    button(&mut w, edge, true);
    glide(&mut w, edge, edge + vec2(40.0, 0.0));
    button(&mut w, edge + vec2(40.0, 0.0), false);
    w.rest();
    let after = frame_of(&w, a);
    assert!(after.2 > before.2 + 0.3, "the handle widened the shape: {before:?} {after:?}");
    assert_eq!(shape_ids(&w), [a], "and no copy was made");
}

#[test]
fn a_shape_with_no_connection_point_gets_no_arrows() {
    // The boxes of this file have no `Connection` section: a connector could not be glued to one, so a click on an arrow would only be refused.
    let dir = crate::support::TempDir::new("auto-no-points");
    let path = crate::fixture::write(dir.path(), "plain.vsdx", &crate::fixture::crowded_vsdx(50));
    let mut session = diacraft_engine::Session::new();
    session.execute("file.open", &json!({"path": path.to_str().unwrap()})).unwrap();
    let mut w = crate::support::fast_window(
        diacraft_ui_egui::DiacraftApp::new(session, None).with_inline_pictures(diacraft_ui_egui::render_worker::bundled_drawer()),
        [1280.0, 900.0],
    );
    w.rest();
    run_command(&mut w, "view.fitPage", json!({}));
    let page = page_id(&w);
    let doc = w.state().session().document().unwrap();
    assert!(diacraft_engine::draw::connection_points(doc, page, 1).is_empty(), "the fixture's boxes have no connection point");
    // Shape 1 is the square at (0.2, 0.2).
    let on = at(&w, 0.24, 0.24);
    show_arrows(&mut w, on);
    assert!(arrows(&w).is_empty(), "no arrows on a shape a connector cannot be glued to");
}

#[test]
fn the_arrows_survive_the_least_and_the_most_zoom() {
    let mut w = edit_window([1100.0, 800.0]);
    rectangle_at(&mut w, 4.0, 4.0);
    for percent in [5, 6400] {
        run_command(&mut w, "view.zoom", json!({"percent": percent}));
        let middle = at(&w, 4.0, 4.0);
        show_arrows(&mut w, middle);
        for _ in 0..3 {
            w.step();
        }
        assert!(w.state().ui_panics() == 0, "no panic at {percent}%");
    }
}

#[test]
fn resting_on_a_shape_of_a_crowded_page_looks_its_points_up_once_and_a_frame_that_changes_nothing_looks_up_nothing() {
    let dir = crate::support::TempDir::new("auto-crowded");
    let path = crate::fixture::write(dir.path(), "crowded.vsdx", &crate::fixture::crowded_vsdx(5_000));
    let mut session = diacraft_engine::Session::new();
    session.execute("file.open", &json!({"path": path.to_str().unwrap()})).unwrap();
    let mut w = crate::support::fast_window(
        diacraft_ui_egui::DiacraftApp::new(session, None).with_inline_pictures(diacraft_ui_egui::render_worker::bundled_drawer()),
        [1280.0, 900.0],
    );
    w.rest();
    run_command(&mut w, "view.zoom", json!({"percent": 100}));
    // Shape 4054 is the 0.08 inch square with its lower left at (5.5, 4.2).
    let on = at(&w, 5.54, 4.24);
    rest_at(&mut w, on);
    diacraft_ui_egui::work::reset();
    diacraft_doc::work::reset();
    w.advance(ARROWS_DELAY);
    w.step();
    // Whether the arrows are up depends on the fixture's boxes having a connection point, which this test does not ask: what it counts is the looking up.
    let first = diacraft_doc::work::finds_visited();
    assert!(first <= 5_000 + 64, "one walk of the page for the shape's points, no more: {first}");
    diacraft_doc::work::reset();
    for _ in 0..10 {
        w.step();
    }
    assert_eq!(diacraft_doc::work::finds_visited(), 0, "ten frames of the same shape look nothing up");
}

#[test]
fn the_flyout_opened_by_the_keyboard_is_not_taken_over_by_the_pointer_resting_on_another_shape() {
    let mut w = edit_window([1100.0, 800.0]);
    let a = rectangle_at(&mut w, 3.0, 4.0);
    rectangle_at(&mut w, 8.0, 4.0);
    w.state_mut().select(&[a]);
    w.rest();
    key(&mut w, Key::ArrowRight, Modifiers::ALT | Modifiers::SHIFT);
    assert_eq!(flyout(&w).map(|f| f.0), Some("right".to_owned()));
    // The pointer rests on the other shape and then on its arrow, long enough for a flyout of its own.
    let on_b = at(&w, 8.0, 4.0);
    show_arrows(&mut w, on_b);
    let target = arrow(&w, "right");
    glide(&mut w, on_b, target);
    w.advance(FLYOUT_DELAY);
    w.step();
    let state = w.state().window_state();
    assert_eq!(state["autoConnect"]["flyout"]["from"], a, "still the selected shape's: {state}");
    key(&mut w, Key::ArrowDown, Modifiers::NONE);
    assert_eq!(w.state().autoconnect_current(), Some(1), "and the keys still move in it");
}

// ----: the desktop's arrows (as corrected them): blue, no circle handle, no "Glue to Shape" tip

#[test]
fn the_arrows_are_four_blue_triangles_with_no_circle_handle_beside_them_in_either_theme() {
    for theme in ["light", "dark"] {
        let tokens = if theme == "dark" { Tokens::dark() } else { Tokens::light() };
        let mut w = edit_window([1100.0, 800.0]);
        run_command(&mut w, "window.theme", json!({"theme": theme}));
        rectangle_at(&mut w, 4.0, 4.0);
        let middle = at(&w, 4.0, 4.0);
        show_arrows(&mut w, middle);
        assert_eq!(sides(&w), ["down", "left", "right", "up"], "{theme}");
        // The window has no element for a circle handle, and paints no circle in an arrow's colour.
        let handles: Vec<String> = w.state().elements().iter().filter(|e| e.name.starts_with("autoconnect.knob")).map(|e| e.name.clone()).collect();
        assert!(handles.is_empty(), "{theme}: a circle handle stands beside an arrow: {handles:?}");
        assert_eq!(arrow_coloured_circles(&w, &tokens), 0, "{theme}: a circle is painted in an arrow's colour");
        // The pointer is on the shape and on no arrow: the four triangles are all the colour of an arrow at rest.
        assert_eq!(arrow_fills(&w, &tokens), vec![tokens.auto_arrow; 4], "{theme}");
        // The pointer goes to the right arrow: that one is the deeper blue and the other three stay as they were.
        let target = arrow(&w, "right");
        glide(&mut w, middle, target);
        w.step();
        let mut fills = arrow_fills(&w, &tokens);
        fills.sort_by_key(|c| c.to_array());
        let mut expected = vec![tokens.auto_arrow, tokens.auto_arrow, tokens.auto_arrow, tokens.auto_arrow_hot];
        expected.sort_by_key(|c| c.to_array());
        assert_eq!(fills, expected, "{theme}: one triangle, the one under the pointer, is the deeper blue");
        assert_eq!(arrow_coloured_circles(&w, &tokens), 0, "{theme}: still no circle");
    }
}

#[test]
fn a_drag_that_begins_where_a_circle_handle_used_to_stand_beside_an_arrow_joins_nothing() {
    let mut w = edit_window([1100.0, 800.0]);
    rectangle_at(&mut w, 3.0, 4.0);
    rectangle_at(&mut w, 7.0, 7.0);
    let on = at(&w, 3.0, 4.0);
    show_arrows(&mut w, on);
    // The handle stood 17 points clockwise of the triangle's middle line: below the right arrow, beside the triangle and clear of the arrow's own press area.
    let spot = arrow(&w, "right") + vec2(0.0, 17.0);
    let onto = at(&w, 7.0, 7.0);
    let revision = w.state().session().revision();
    rest_at(&mut w, spot);
    button(&mut w, spot, true);
    glide(&mut w, spot, onto);
    assert!(w.state().autoconnect_drag_target().is_none(), "no drag from an arrow is under way");
    button(&mut w, onto, false);
    w.rest();
    assert_eq!(shape_ids(&w).len(), 2, "no connector was made");
    assert_eq!(w.state().session().revision(), revision, "and the document did not change");
}

#[test]
fn a_drag_from_an_arrow_is_one_undo_step_selects_the_connector_and_onto_the_shape_it_began_on_makes_nothing() {
    let mut w = edit_window([1100.0, 800.0]);
    let a = rectangle_at(&mut w, 3.0, 4.0);
    let c = rectangle_at(&mut w, 7.0, 7.0);
    let on = at(&w, 3.0, 4.0);
    show_arrows(&mut w, on);
    let from = arrow(&w, "up");
    let onto = at(&w, 7.0, 7.0);
    rest_at(&mut w, from);
    button(&mut w, from, true);
    glide(&mut w, from, onto);
    assert_eq!(w.state().autoconnect_drag_target(), Some(c), "the drag would join c");
    let revision = w.state().session().revision();
    button(&mut w, onto, false);
    w.rest();
    let ids = shape_ids(&w);
    assert_eq!(ids.len(), 3, "{ids:?}");
    assert_eq!(glued(&w, ids[2]), (Some(a), Some(c)));
    assert_eq!(w.state().session().revision(), revision + 1, "one command, one undo step");
    assert_eq!(w.state().selection(), [ids[2]], "the connector is selected, as a click on an arrow leaves it");
    run_command(&mut w, "edit.undo", json!({}));
    assert_eq!(shape_ids(&w).len(), 2, "one undo takes it back");
    // A drag let go on the shape it began on makes nothing.
    let on = at(&w, 3.0, 4.0);
    show_arrows(&mut w, on);
    let from = arrow(&w, "right");
    rest_at(&mut w, from);
    button(&mut w, from, true);
    glide(&mut w, from, on);
    assert!(w.state().autoconnect_drag_target().is_none());
    button(&mut w, on, false);
    w.rest();
    assert_eq!(shape_ids(&w).len(), 2, "nothing made on the shape it began on");
}

#[test]
fn a_drag_from_an_arrow_to_a_shapes_edge_joins_at_the_connection_point_there_and_none_shows_a_glue_to_shape_tip() {
    let mut w = edit_window([1100.0, 800.0]);
    let a = rectangle_at(&mut w, 3.0, 4.0);
    let c = rectangle_at(&mut w, 7.0, 7.0);
    // The rectangles are 1.5 by 1 inch: the top point of c is at (7.0, 7.5) and the bottom point at (7.0, 6.5). The pointer is let go on each in turn.
    let mut glue = Vec::new();
    for y in [7.5, 6.5] {
        let on = at(&w, 3.0, 4.0);
        show_arrows(&mut w, on);
        let from = arrow(&w, "up");
        let edge = at(&w, 7.0, y);
        rest_at(&mut w, from);
        button(&mut w, from, true);
        glide(&mut w, from, edge);
        w.step();
        assert_eq!(w.state().autoconnect_drag_target(), Some(c), "the drag would join c at y {y}");
        assert!(!painted_text(&w, "Glue to Shape"), "no tip at the edge");
        button(&mut w, edge, false);
        w.rest();
        let ids = shape_ids(&w);
        assert_eq!(ids.len(), 3, "{ids:?}");
        assert_eq!(glued(&w, ids[2]), (Some(a), Some(c)));
        glue.push(end_glue(&w, ids[2]).expect("the end is glued"));
        run_command(&mut w, "edit.undo", json!({}));
    }
    assert!(glue[0].1 != glue[1].1 || glue[0].2 != glue[1].2, "the two edges are two points of c: {glue:?}");
}

#[test]
fn a_drag_from_an_arrow_into_the_inside_of_a_shape_paints_no_glue_to_shape_tip() {
    // The tip belongs to the reference app's web version: the desktop's drag from an arrow shows the rubber line and the green over the shape, and no words.
    let mut w = edit_window([1100.0, 800.0]);
    rectangle_at(&mut w, 3.0, 4.0);
    let c = rectangle_at(&mut w, 7.0, 7.0);
    let on = at(&w, 3.0, 4.0);
    show_arrows(&mut w, on);
    let from = arrow(&w, "up");
    let inside = at(&w, 7.0, 7.1);
    rest_at(&mut w, from);
    button(&mut w, from, true);
    glide(&mut w, from, inside);
    w.step();
    assert_eq!(w.state().autoconnect_drag_target(), Some(c), "the drag is over c's inside");
    assert!(!painted_text(&w, "Glue to Shape"), "no tip beside the pointer");
    button(&mut w, inside, false);
    w.rest();
}

/// A shape whose layers allow no glue shows no arrows (no connector is glued to it); on a layer that allows it, it does.
#[test]
fn a_shape_whose_layers_allow_no_glue_shows_no_arrows() {
    let mut w = edit_window([1100.0, 800.0]);
    let a = rectangle_at(&mut w, 4.0, 4.0);
    run_command(&mut w, "layer.add", json!({"name": "Bare", "glue": false}));
    run_command(&mut w, "layer.assign", json!({"ids": [a], "layers": ["Bare"]}));
    w.rest();
    let middle = at(&w, 4.0, 4.0);
    show_arrows(&mut w, middle);
    assert!(arrows(&w).is_empty(), "no arrows: {:?}", sides(&w));
    let away = at(&w, 8.0, 7.0);
    move_to(&mut w, away);
    w.step();
    run_command(&mut w, "layer.set", json!({"layer": "Bare", "glue": true}));
    w.rest();
    show_arrows(&mut w, middle);
    assert_eq!(sides(&w), ["down", "left", "right", "up"]);
}

/// A Swimlane let go on an arrow is no shape to join: it is dropped as a drop of it is (on a page with no lanes, the shape it is drawn as), and no
/// connector is made.
#[test]
fn a_swimlane_dragged_onto_an_arrow_is_dropped_as_a_drop_is_and_joined_to_nothing() {
    let mut w = edit_window([1100.0, 800.0]);
    let a = rectangle_at(&mut w, 4.0, 4.0);
    run_command(&mut w, "window.stencil", json!({ "stencil": "cross-functional" }));
    w.rest();
    let tile = w.state().element("shapes.tile.Swimlane").expect("the tile").clone();
    let start = tile.rect.center();
    let middle = at(&w, 4.0, 4.0);
    rest_at(&mut w, start);
    button(&mut w, start, true);
    glide(&mut w, start, middle);
    w.advance(ARROWS_DELAY);
    w.step();
    let target = arrow(&w, "right");
    glide(&mut w, middle, target);
    button(&mut w, target, false);
    w.rest();
    let ids = shape_ids(&w);
    assert_eq!(ids.len(), 2, "the rectangle and the swimlane, and no connector: {ids:?}");
    let doc = w.state().session().document().unwrap();
    let joined = doc.pages.get(0).unwrap().shapes.iter().any(|s| s.sheet.cell("BeginX").is_some());
    assert!(!joined, "no connector was made");
    assert!(ids.contains(&a));
}
