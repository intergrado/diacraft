//! A connector's path edited by hand in the window.
//!
//! A selected right-angle connector shows a handle at the middle of each leg. A drag of one moves the leg, the route keeps its right angles, the drag lands where the
//! engine's snap puts it, and the release is one `connector.edit_path`: one undo step. The connector's context menu has Reset Connector and the Reroute choices.

use diacraft_engine::draw::{Reroute, connector_path, connector_reroute};
use diacraft_ui_egui::edit::Tool;
use egui::{Key, Modifiers, Pos2, Rect};
use serde_json::{Value, json};

use crate::editing::{at, edit_window, hold, key, shape_ids};
use crate::support::{Rest, Window, button, move_to, run_command};

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

/// A window with a rectangle at (2, 3) and one at (6, 5), joined from the first's right point to the second's left point: a Z of three legs, (2.75, 3), (4, 3), (4, 5),
/// (5.25, 5). The connector is selected. → the window and the connector.
fn z_window() -> (Window, u32) {
    let mut w = edit_window([1280.0, 900.0]);
    let (a, b) = (rectangle_at(&mut w, 2.0, 3.0), rectangle_at(&mut w, 6.0, 5.0));
    let page = page_id(&w);
    let answer = w.state_mut().execute("connector.add", &json!({"from": {"id": a, "point": 3}, "to": {"id": b, "point": 1}, "page": page})).unwrap();
    let c = u32::try_from(answer["id"].as_u64().unwrap()).unwrap();
    w.state_mut().select(&[c]);
    w.rest();
    (w, c)
}

type P = (f64, f64);

fn route(w: &Window, id: u32) -> Vec<P> {
    connector_path(w.state().session().document().unwrap(), page_id(w), id).unwrap().points
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1.0e-6
}

fn assert_route(got: &[P], want: &[P]) {
    assert!(got.len() == want.len() && got.iter().zip(want).all(|(g, w)| near(g.0, w.0) && near(g.1, w.1)), "got {got:?}, want {want:?}");
}

/// The handles the last frame drew: leg number and rectangle on the screen.
fn handles(w: &Window) -> Vec<(usize, Rect)> {
    let mut found: Vec<(usize, Rect)> = w
        .state()
        .elements()
        .iter()
        .filter_map(|e| e.name.strip_prefix("connector.leg.").and_then(|n| n.parse::<usize>().ok()).map(|n| (n, e.rect)))
        .collect();
    found.sort_by_key(|(n, _)| *n);
    found
}

fn code_of(w: &Window, id: u32) -> Option<Reroute> {
    connector_reroute(w.state().session().document().unwrap(), page_id(w), id)
}

fn undo_steps(w: &Window) -> usize {
    w.state().session().history().map_or(0, |h| h.undo_len())
}

/// Presses at the handle of leg `leg`, moves to `to` (a page point) in steps and stops there, the button still down.
fn press_and_move(w: &mut Window, leg: usize, to: P) -> Pos2 {
    let from = w.state().connector_leg_point(connector_of(w), leg).expect("the leg has a handle");
    for _ in 0..3 {
        move_to(w, from);
    }
    button(w, from, true);
    let target = at(w, to.0, to.1);
    for step in 1..=5 {
        move_to(w, from + (target - from) * (step as f32 / 5.0));
    }
    target
}

fn connector_of(w: &Window) -> u32 {
    *w.state().selection().first().expect("a connector is selected")
}

fn release(w: &mut Window, pos: Pos2) {
    button(w, pos, false);
    w.rest();
}

#[test]
fn a_selected_right_angle_connector_shows_a_handle_at_the_middle_of_each_leg() {
    let (w, c) = z_window();
    assert_route(&route(&w, c), &[(2.75, 3.0), (4.0, 3.0), (4.0, 5.0), (5.25, 5.0)]);
    let shown = handles(&w);
    assert_eq!(shown.iter().map(|(n, _)| *n).collect::<Vec<_>>(), [0, 1, 2]);
    for ((_, rect), middle) in shown.iter().zip([(3.375, 3.0), (4.0, 4.0), (4.625, 5.0)]) {
        let want = at(&w, middle.0, middle.1);
        assert!((rect.center() - want).length() < 1.0, "{rect:?} for {middle:?} at {want:?}");
        assert!(rect.width() >= 12.0, "a handle is easy to hit: {rect:?}");
    }
    // `connector_leg_point` is where a client takes hold.
    for (n, rect) in &shown {
        let p = w.state().connector_leg_point(c, *n).expect("the leg has a handle");
        assert!((p - rect.center()).length() < 0.01, "leg {n}: {p:?} {rect:?}");
    }
    assert_eq!(w.state().connector_leg_point(c, 3), None, "there is no fourth leg");
}

#[test]
fn nothing_is_shown_for_an_unselected_connector_another_tool_several_shapes_or_a_straight_or_curved_connector() {
    let (mut w, c) = z_window();
    assert_eq!(handles(&w).len(), 3);
    // Another tool: the Connector tool joins, and a handle is not its to take.
    w.state_mut().set_tool(Tool::Connector);
    w.rest();
    assert!(handles(&w).is_empty(), "the Connector tool");
    w.state_mut().set_tool(Tool::Pointer);
    w.rest();
    assert_eq!(handles(&w).len(), 3);
    // Several shapes selected.
    let ids = shape_ids(&w);
    w.state_mut().select(&ids);
    w.rest();
    assert!(handles(&w).is_empty(), "several shapes");
    // Nothing selected.
    w.state_mut().select(&[]);
    w.rest();
    assert!(handles(&w).is_empty(), "nothing selected");
    // A straight connector, and a curved one, have no legs to take hold of (the specifications say nothing of a handle on them: report Q1).
    for style in ["straight", "curved"] {
        let page = page_id(&w);
        run_command(&mut w, "connector.style", json!({"style": style, "ids": [c], "page": page}));
        w.state_mut().select(&[c]);
        w.rest();
        assert!(handles(&w).is_empty(), "{style}");
    }
}

#[test]
fn dragging_the_middle_leg_moves_it_and_the_route_keeps_its_right_angles_in_one_undo_step() {
    let (mut w, c) = z_window();
    let steps = undo_steps(&w);
    let target = press_and_move(&mut w, 1, (5.0, 4.0));
    // While the button is down nothing is written, and the preview says where the leg goes.
    assert_eq!(undo_steps(&w), steps);
    assert_route(&route(&w, c), &[(2.75, 3.0), (4.0, 3.0), (4.0, 5.0), (5.25, 5.0)]);
    let preview = w.state().drag_preview().expect("a leg is being dragged");
    assert!(near(preview.leg_to.expect("the line the leg goes to"), 5.0), "{preview:?}");
    assert!(preview.snap, "it asks the engine's snap, as every drag does");
    release(&mut w, target);
    assert_route(&route(&w, c), &[(2.75, 3.0), (5.0, 3.0), (5.0, 5.0), (5.25, 5.0)]);
    assert_eq!(undo_steps(&w), steps + 1, "one undo step");
    assert_eq!(code_of(&w, c), Some(Reroute::OnCrossover), "A path edited by hand writes 3");
    assert!(connector_path(w.state().session().document().unwrap(), page_id(&w), c).unwrap().hand_edited);
    // The connector is still the one selected, with handles on its new legs.
    assert_eq!(w.state().selection(), [c]);
    assert_eq!(handles(&w).len(), 3);
    // One undo takes the leg and the setting back.
    run_command(&mut w, "edit.undo", json!({}));
    assert_route(&route(&w, c), &[(2.75, 3.0), (4.0, 3.0), (4.0, 5.0), (5.25, 5.0)]);
    assert_eq!(code_of(&w, c), Some(Reroute::Freely));
}

#[test]
fn a_leg_at_an_end_is_dragged_with_a_jog_so_that_the_ends_stay_on_their_points() {
    let (mut w, c) = z_window();
    let target = press_and_move(&mut w, 0, (3.375, 2.0));
    release(&mut w, target);
    assert_route(&route(&w, c), &[(2.75, 3.0), (3.0, 3.0), (3.0, 2.0), (4.0, 2.0), (4.0, 5.0), (5.25, 5.0)]);
}

#[test]
fn the_drag_lands_where_the_engines_snap_puts_it_and_the_release_is_what_the_preview_showed() {
    let (mut w, c) = z_window();
    // 0.04 inch (3.84 points) past the grid line at 5: within the pull of the grid.
    let target = press_and_move(&mut w, 1, (5.04, 4.0));
    let preview = w.state().drag_preview().unwrap();
    assert!(near(preview.leg_to.unwrap(), 5.0), "the preview is already on the line: {preview:?}");
    release(&mut w, target);
    assert_route(&route(&w, c), &[(2.75, 3.0), (5.0, 3.0), (5.0, 5.0), (5.25, 5.0)]);
    let told = w.state().last_snap().expect("the answer carried the snap");
    assert_eq!(told["hits"][0]["axis"], json!("x"), "{told}");
}

#[test]
fn alt_turns_the_snap_off_for_the_drag_and_the_leg_lands_where_the_pointer_is() {
    let (mut w, c) = z_window();
    let from = w.state().connector_leg_point(c, 1).unwrap();
    for _ in 0..3 {
        move_to(&mut w, from);
    }
    hold(&mut w, Modifiers::ALT);
    button(&mut w, from, true);
    let target = at(&w, 5.04, 4.0);
    for step in 1..=5 {
        move_to(&mut w, from + (target - from) * (step as f32 / 5.0));
    }
    let preview = w.state().drag_preview().unwrap();
    assert!(!preview.snap && near(preview.leg_to.unwrap(), 5.04), "{preview:?}");
    release(&mut w, target);
    hold(&mut w, Modifiers::NONE);
    assert_route(&route(&w, c), &[(2.75, 3.0), (5.04, 3.0), (5.04, 5.0), (5.25, 5.0)]);
}

#[test]
fn escape_in_the_middle_of_the_drag_changes_nothing_and_so_does_a_drag_back_to_where_it_began() {
    let (mut w, c) = z_window();
    let steps = undo_steps(&w);
    let target = press_and_move(&mut w, 1, (5.0, 4.0));
    key(&mut w, Key::Escape, Modifiers::NONE);
    release(&mut w, target);
    assert_eq!(undo_steps(&w), steps, "Escape drops the drag");
    assert_route(&route(&w, c), &[(2.75, 3.0), (4.0, 3.0), (4.0, 5.0), (5.25, 5.0)]);
    // Out and back: the leg is where it was, and nothing is sent.
    let from = w.state().connector_leg_point(c, 1).unwrap();
    for _ in 0..3 {
        move_to(&mut w, from);
    }
    button(&mut w, from, true);
    move_to(&mut w, from + egui::vec2(40.0, 0.0));
    move_to(&mut w, from);
    release(&mut w, from);
    assert_eq!(undo_steps(&w), steps);
    assert_eq!(code_of(&w, c), Some(Reroute::Freely), "and the path is not marked as edited");
}

#[test]
fn the_glue_of_the_ends_and_the_shapes_stay_as_they_were_when_a_leg_moves() {
    let (mut w, c) = z_window();
    let before = w.state().session().document().unwrap().pages.get(0).unwrap().connects.clone();
    let shapes: Vec<_> = shape_ids(&w);
    let target = press_and_move(&mut w, 1, (5.0, 4.0));
    release(&mut w, target);
    let doc = w.state().session().document().unwrap();
    assert_eq!(doc.pages.get(0).unwrap().connects, before, "both ends are still glued to the same points");
    assert_eq!(shape_ids(&w), shapes, "no shape was made or lost");
    // The ends are where they were: the first leg leaves the first shape's right point and the last arrives at the second's left point.
    let r = route(&w, c);
    assert!(near(r[0].0, 2.75) && near(r[0].1, 3.0) && near(r.last().unwrap().0, 5.25) && near(r.last().unwrap().1, 5.0), "{r:?}");
}

#[test]
fn the_pointer_over_a_handle_is_a_resize_arrow_across_its_leg() {
    let (mut w, c) = z_window();
    for (leg, icon) in [(0, egui::CursorIcon::ResizeVertical), (1, egui::CursorIcon::ResizeHorizontal), (2, egui::CursorIcon::ResizeVertical)] {
        let p = w.state().connector_leg_point(c, leg).unwrap();
        for _ in 0..3 {
            move_to(&mut w, p);
        }
        assert_eq!(w.output().platform_output.cursor_icon, icon, "leg {leg}");
    }
}

#[test]
fn the_context_menu_of_a_connector_has_the_connector_rows_and_runs_reset_and_the_reroute_choices() {
    let (mut w, c) = z_window();
    // Hand-edit the middle leg first, so that Reset Connector has something to take back.
    let target = press_and_move(&mut w, 1, (5.0, 4.0));
    release(&mut w, target);
    assert_eq!(code_of(&w, c), Some(Reroute::OnCrossover));
    right_click_at(&mut w, 3.0, 3.0);
    for name in ["Cut", "Copy", "Paste", "Delete", "BringToFront", "SendToBack", "ResetConnector", "Reroute"] {
        assert!(w.state().element(&format!("menu.Connector{name}")).is_some(), "{name} is in the connector's menu");
    }
    assert!(w.state().element("menu.CanvasGroup").is_none(), "the shape menu's own rows are not the connector's");
    let enabled = |w: &Window, name: &str| w.state().element(&format!("menu.Connector{name}")).is_some_and(|e| e.enabled);
    assert!(enabled(&w, "ResetConnector") && enabled(&w, "Reroute") && enabled(&w, "Cut") && enabled(&w, "Delete"));
    // Reset Connector: the route is the routed one, the code is 0, one undo step.
    let steps = undo_steps(&w);
    press_row(&mut w, "menu.ConnectorResetConnector");
    assert_route(&route(&w, c), &[(2.75, 3.0), (4.0, 3.0), (4.0, 5.0), (5.25, 5.0)]);
    assert_eq!(code_of(&w, c), Some(Reroute::Freely));
    assert_eq!(undo_steps(&w), steps + 1);
    // The Reroute rows are a sub-menu of four, the one the connector has marked; each writes its code in one undo step and leaves the path alone.
    for (row, want, code) in [
        ("Never", Reroute::Never, "menu.ConnectorRerouteNever"),
        ("On Crossover", Reroute::OnCrossover, "menu.ConnectorRerouteOnCrossover"),
        ("As Needed", Reroute::AsNeeded, "menu.ConnectorRerouteAsNeeded"),
        ("Freely", Reroute::Freely, "menu.ConnectorRerouteFreely"),
    ] {
        let steps = undo_steps(&w);
        right_click_at(&mut w, 3.0, 3.0);
        press_row(&mut w, "menu.ConnectorReroute");
        assert!(w.state().element(code).is_some(), "{row}: {:?}", w.state().elements().iter().map(|e| &e.name).collect::<Vec<_>>());
        press_row(&mut w, code);
        assert_eq!(code_of(&w, c), Some(want), "{row}");
        assert_eq!(undo_steps(&w), steps + 1, "{row}: one undo step (the reroute setting is an ordinary edit)");
        assert_route(&route(&w, c), &[(2.75, 3.0), (4.0, 3.0), (4.0, 5.0), (5.25, 5.0)]);
    }
}

#[test]
fn delete_in_the_connectors_menu_takes_the_connector_away_in_one_undo_step_and_cut_and_copy_are_the_clipboards() {
    let (mut w, c) = z_window();
    let shapes = shape_ids(&w);
    assert!(shapes.contains(&c));
    // Copy leaves the connector and fills the clipboard.
    right_click_at(&mut w, 3.0, 3.0);
    press_row(&mut w, "menu.ConnectorCopy");
    assert!(w.state().session().clipboard().is_some_and(|clip| !clip.is_empty()), "the connector was copied");
    assert_eq!(shape_ids(&w), shapes);
    // Delete takes it away, the two shapes stay, and one undo brings it back.
    let steps = undo_steps(&w);
    right_click_at(&mut w, 3.0, 3.0);
    press_row(&mut w, "menu.ConnectorDelete");
    assert!(!shape_ids(&w).contains(&c), "the connector is gone");
    assert_eq!(shape_ids(&w).len(), 2, "the shapes it joined stay");
    assert_eq!(undo_steps(&w), steps + 1);
    run_command(&mut w, "edit.undo", json!({}));
    assert!(shape_ids(&w).contains(&c));
}

#[test]
fn a_shape_that_is_no_connector_has_the_shape_menu_and_a_mixed_selection_too() {
    let (mut w, c) = z_window();
    let rect = shape_ids(&w)[0];
    w.state_mut().select(&[rect]);
    w.rest();
    right_click_at(&mut w, 2.0, 3.0);
    assert!(w.state().element("menu.CanvasGroup").is_some() && w.state().element("menu.ConnectorResetConnector").is_none());
    key(&mut w, Key::Escape, Modifiers::NONE);
    w.state_mut().select(&[rect, c]);
    w.rest();
    right_click_at(&mut w, 2.0, 3.0);
    assert!(w.state().element("menu.CanvasGroup").is_some() && w.state().element("menu.ConnectorResetConnector").is_none(), "a shape among them");
}

#[test]
fn the_window_reports_the_legs_and_the_drag_to_a_client() {
    let (mut w, c) = z_window();
    let state: Value = w.state().window_state();
    assert_eq!(state["legs"]["connector"], json!(c));
    assert_eq!(state["legs"]["handles"].as_array().unwrap().len(), 3);
    assert_eq!(state["legs"]["handles"][1]["leg"], json!(1));
    assert_eq!(state["legs"]["handles"][1]["level"], json!(false));
    assert_eq!(state["legs"]["reroute"], json!("freely"));
    let target = press_and_move(&mut w, 1, (5.0, 4.0));
    let state: Value = w.state().window_state();
    assert!(near(state["legs"]["dragging"]["to"].as_f64().unwrap(), 5.0), "{state}");
    release(&mut w, target);
    assert_eq!(w.state().window_state()["legs"]["reroute"], json!("on_crossover"));
}

fn right_click_at(w: &mut Window, x: f64, y: f64) {
    let pos = at(w, x, y);
    for _ in 0..3 {
        move_to(w, pos);
    }
    for pressed in [true, false] {
        w.input_mut().events.push(egui::Event::PointerButton { pos, button: egui::PointerButton::Secondary, pressed, modifiers: Modifiers::NONE });
        w.step();
    }
    w.rest();
}

/// A click on a row of the open menu.
fn press_row(w: &mut Window, name: &str) {
    let row = w.state().element(name).unwrap_or_else(|| panic!("no row {name}")).rect;
    let pos = row.center();
    for _ in 0..3 {
        move_to(w, pos);
    }
    button(w, pos, true);
    button(w, pos, false);
    w.rest();
}

// ---- the keys: Shift with an arrow key moves a leg a pixel at a time --------------------------------------------------------

/// How far one screen point is on the page, in inches.
fn one_point(w: &Window) -> f64 {
    1.0 / f64::from((at(w, 5.0, 3.0) - at(w, 4.0, 3.0)).x)
}

fn last_command(w: &Window) -> String {
    w.state().session().journal().last().map(|(id, _)| id.clone()).unwrap_or_default()
}

/// "dragging a midpoint to a new place reroutes it, keeping that path until reset, and **Shift with arrow keys moves
/// it a pixel at a time**". With one connector selected whose ends are both glued (a nudge of the shape would move nothing), Shift with an arrow key moves the leg that key moves
/// across, the one nearest the middle of the route, by one screen point, with no snap: one `connector.edit_path` and one undo step each. Left and Right move an upright leg, Up and
/// Down a level one. **Assumed (the specifications say which leg and no more):** the leg is the one nearest the middle among those the key moves.
#[test]
fn shift_with_an_arrow_key_moves_the_leg_across_the_route_a_screen_point() {
    let (mut w, c) = z_window();
    let before = route(&w, c);
    let step = one_point(&w);
    assert!(step > 0.0 && step < 0.05, "a screen point is a small part of an inch: {step}");
    let steps = undo_steps(&w);
    // The one upright leg, at x = 4.
    key(&mut w, Key::ArrowRight, Modifiers::SHIFT);
    assert_eq!(last_command(&w), "connector.edit_path");
    let moved = route(&w, c);
    assert_eq!(moved.len(), before.len(), "the route keeps its legs: {moved:?}");
    assert!(
        near_to(moved[1].0, before[1].0 + step) && near_to(moved[2].0, before[2].0 + step),
        "the upright leg went right by a point: {moved:?} from {before:?}"
    );
    assert!(near_to(moved[1].1, before[1].1) && near_to(moved[2].1, before[2].1), "and only across");
    assert_eq!(undo_steps(&w), steps + 1, "one undo step");
    key(&mut w, Key::ArrowLeft, Modifiers::SHIFT);
    key(&mut w, Key::ArrowLeft, Modifiers::SHIFT);
    let left = route(&w, c);
    assert!(near_to(left[1].0, before[1].0 - step), "Left twice from there is one point left of where it began: {left:?}");
    // Nothing moved the shapes the connector is glued to.
    assert_eq!(shape_ids(&w).len(), 3, "a connector and its two shapes");
    // The plain arrow of a connector that moves nothing does nothing.
    let now = undo_steps(&w);
    key(&mut w, Key::ArrowRight, Modifiers::NONE);
    assert_eq!(undo_steps(&w), now, "a plain arrow key leaves a connector with both ends glued alone");
}

/// A level leg moves with Up and Down. The route from the top of one shape to the bottom of another is vertical, horizontal, vertical, and its middle leg is level.
#[test]
fn shift_with_up_and_down_moves_a_level_middle_leg() {
    let mut w = edit_window([1280.0, 900.0]);
    let (a, b) = (rectangle_at(&mut w, 2.0, 3.0), rectangle_at(&mut w, 6.0, 6.0));
    let page = page_id(&w);
    let answer = w.state_mut().execute("connector.add", &json!({"from": {"id": a, "point": 0}, "to": {"id": b, "point": 2}, "page": page})).unwrap();
    let c = u32::try_from(answer["id"].as_u64().unwrap()).unwrap();
    w.state_mut().select(&[c]);
    w.rest();
    let before = route(&w, c);
    assert_eq!(before.len(), 4, "up, across, up: {before:?}");
    let step = one_point(&w);
    key(&mut w, Key::ArrowUp, Modifiers::SHIFT);
    let up = route(&w, c);
    assert!(near_to(up[1].1, before[1].1 + step) && near_to(up[2].1, before[2].1 + step), "the level leg went up a point: {up:?} from {before:?}");
    key(&mut w, Key::ArrowDown, Modifiers::SHIFT);
    key(&mut w, Key::ArrowDown, Modifiers::SHIFT);
    assert!(near_to(route(&w, c)[1].1, before[1].1 - step));
}

/// A connector with a free end is nudged as a shape is, as before: the keys move the connector, and no leg is edited.
#[test]
fn a_connector_with_a_free_end_is_nudged_as_before() {
    let mut w = edit_window([1280.0, 900.0]);
    let a = rectangle_at(&mut w, 2.0, 3.0);
    let page = page_id(&w);
    let answer = w.state_mut().execute("connector.add", &json!({"from": {"id": a, "point": 3}, "to": {"x": 7.0, "y": 5.0}, "page": page})).unwrap();
    let c = u32::try_from(answer["id"].as_u64().unwrap()).unwrap();
    w.state_mut().select(&[c]);
    w.rest();
    key(&mut w, Key::ArrowRight, Modifiers::SHIFT);
    assert_ne!(last_command(&w), "connector.edit_path", "a connector that moves by itself is nudged as a shape");
}

fn near_to(a: f64, b: f64) -> bool {
    (a - b).abs() < 1.0e-4
}
