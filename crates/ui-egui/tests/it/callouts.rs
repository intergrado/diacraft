//! Insert, Diagram Parts, Callout in the window. The gallery has fourteen tiles in rows of four, four, four and two; a press on one adds a
//! callout tied to the shape selected, or one tied to nothing at the middle of the view; a callout selected shows a yellow handle at the end of its leader, and a drag of it onto a shape ties it and onto
//! bare page unties it. The pointer and the keyboard are given to egui as a person's would be; what is checked is the drawing the commands changed and what the window drew.

use egui::{Key, Modifiers, Pos2};
use serde_json::{Value as Json, json};

use crate::editing::{at, edit_window, key};
use crate::support::{Rest, Window, button, click, move_to, press, run_command};

fn doc_shapes(w: &Window) -> Vec<u32> {
    w.state().session().document().unwrap().pages.get(0).unwrap().shapes.iter().map(|s| s.id).collect()
}

fn undo_steps(w: &Window) -> usize {
    w.state().session().history().map_or(0, |h| h.undo_len())
}

/// A window on a new drawing with rectangles at (3, 4) and (7, 4), on the Insert tab. → the window and the two shapes.
fn window_with_two() -> (Window, u32, u32) {
    let mut w = edit_window([1400.0, 900.0]);
    let mut drop = |x: f64, y: f64| {
        w.state_mut().execute("shape.drop", &json!({"master": "rectangle", "page": 1, "x": x, "y": y})).unwrap()["id"].as_u64().unwrap() as u32
    };
    let (a, b) = (drop(3.0, 4.0), drop(7.0, 4.0));
    run_command(&mut w, "window.tab", json!({"tab": "Insert"}));
    (w, a, b)
}

fn list(w: &mut Window, id: u32) -> Json {
    w.state_mut().execute("callout.list", &json!({"id": id})).unwrap()
}

fn enabled(w: &Window, name: &str) -> bool {
    w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).enabled
}

fn click_on(w: &mut Window, name: &str) {
    let pos = w.state().element(name).unwrap_or_else(|| panic!("{name} is not drawn")).rect.center();
    click(w, pos);
    w.rest();
}

#[test]
fn the_callout_control_is_live_with_a_drawing_and_opens_fourteen_tiles_in_rows_of_four() {
    let (mut w, _, _) = window_with_two();
    assert!(enabled(&w, "ribbon.Insert.CalloutGallery"), "the Callout control works");
    press(&mut w, "ribbon.Insert.CalloutGallery");
    w.rest();
    let styles = w.state_mut().execute("callout.styles", &json!({})).unwrap();
    let ids: Vec<String> = styles["styles"].as_array().unwrap().iter().map(|t| t["id"].as_str().unwrap().to_owned()).collect();
    assert_eq!(ids.len(), 14);
    let rects: Vec<egui::Rect> =
        ids.iter().map(|id| w.state().element(&format!("menu.callout.{id}")).unwrap_or_else(|| panic!("tile {id} is not drawn")).rect).collect();
    // Rows of four, four, four and two: tiles of a row share a top and a row is below the last.
    for row in rects.chunks(4) {
        assert!(row.iter().all(|r| (r.top() - row[0].top()).abs() < 0.5), "a row shares a top: {row:?}");
        assert!(row.windows(2).all(|p| p[1].left() > p[0].right() - 0.5), "left to right");
    }
    for (a, b) in [(0, 4), (4, 8), (8, 12)] {
        assert!(rects[b].top() > rects[a].bottom() - 0.5, "the next row is below");
    }
    assert!(ids.iter().all(|id| enabled(&w, &format!("menu.callout.{id}"))), "every tile works");
}

#[test]
fn a_tile_adds_a_callout_for_the_shape_selected_and_it_becomes_the_selection_with_its_handle() {
    let (mut w, a, _) = window_with_two();
    w.state_mut().select(&[a]);
    w.rest();
    press(&mut w, "ribbon.Insert.CalloutGallery");
    w.rest();
    let steps = undo_steps(&w);
    click_on(&mut w, "menu.callout.speech_round_box");
    let shapes = doc_shapes(&w);
    assert_eq!(shapes.len(), 3, "a callout was added");
    let c = *shapes.last().unwrap();
    assert_eq!(undo_steps(&w), steps + 1, "one undo step");
    assert_eq!(list(&mut w, c)["target"], a);
    assert_eq!(list(&mut w, c)["style"], "speech_round_box");
    assert_eq!(w.state().selection(), [c], "the callout is the selection");
    // Its yellow handle is drawn at the end of its leader, where `callout_tip_point` says, and on the target's edge.
    let handle = w.state().callout_tip_point(c).expect("the selected callout has a handle");
    let element = w.state().element("callout.tip").expect("the handle is a named element");
    assert!((element.rect.center() - handle).length() < 0.5);
    let tip = list(&mut w, c)["tip"].clone();
    let want = at(&w, tip[0].as_f64().unwrap(), tip[1].as_f64().unwrap());
    assert!((handle - want).length() < 0.5, "{handle:?} {want:?}");
    // With another shape selected there is no handle.
    w.state_mut().select(&[a]);
    w.rest();
    assert!(w.state().callout_tip_point(c).is_none());
    assert!(w.state().element("callout.tip").is_none());
}

#[test]
fn with_nothing_selected_a_tile_adds_one_callout_tied_to_nothing_at_the_middle_of_the_view() {
    let (mut w, _, _) = window_with_two();
    w.state_mut().select(&[]);
    w.rest();
    run_command(&mut w, "window.callout", json!({"style": "oval_line"}));
    let shapes = doc_shapes(&w);
    let c = *shapes.last().unwrap();
    assert_eq!(shapes.len(), 3);
    assert_eq!(list(&mut w, c)["target"], Json::Null);
    assert_eq!(list(&mut w, c)["style"], "oval_line");
    // Its pin is the page point under the middle of the drawing area.
    let area = w.state().element("canvas").expect("the drawing area has been drawn").rect;
    let (x, y) = w.state().page_point_at(area.center()).expect("the page has been drawn");
    let pin = diacraft_engine::draw::shape_frame(w.state().session().document().unwrap(), 1, c).unwrap();
    assert!((pin.pin_x - x).abs() < 0.05 && (pin.pin_y - y).abs() < 0.05, "({}, {}) at ({x}, {y})", pin.pin_x, pin.pin_y);
    assert_eq!(w.state().selection(), [c]);
}

#[test]
fn a_style_that_is_not_one_is_refused_in_words_and_changes_nothing() {
    let (mut w, a, _) = window_with_two();
    w.state_mut().select(&[a]);
    let steps = undo_steps(&w);
    let e = w.state_mut().execute("window.callout", &json!({"style": "nothing"})).expect_err("a style that is not a tile");
    assert!(e.to_string().contains("no callout style"), "{e}");
    assert_eq!(undo_steps(&w), steps);
    assert_eq!(doc_shapes(&w).len(), 2);
}

/// Presses the handle of callout `c`, moves to the page point `to` in steps and stops there with the button still down.
fn drag_handle(w: &mut Window, c: u32, to: (f64, f64)) -> Pos2 {
    let from = w.state().callout_tip_point(c).expect("the callout has a handle");
    for _ in 0..3 {
        move_to(w, from);
    }
    button(w, from, true);
    let target = at(w, to.0, to.1);
    for step in 1..=6 {
        move_to(w, from + (target - from) * (step as f32 / 6.0));
    }
    target
}

fn release(w: &mut Window, pos: Pos2) {
    button(w, pos, false);
    w.rest();
}

#[test]
fn dropping_the_handle_on_another_shape_ties_the_callout_to_it_in_one_undo_step() {
    let (mut w, a, b) = window_with_two();
    w.state_mut().select(&[a]);
    w.rest();
    run_command(&mut w, "window.callout", json!({"style": "box_line"}));
    let c = *doc_shapes(&w).last().unwrap();
    assert_eq!(list(&mut w, c)["target"], a);
    let steps = undo_steps(&w);
    let target = drag_handle(&mut w, c, (7.0, 4.0));
    release(&mut w, target);
    assert_eq!(list(&mut w, c)["target"], b, "tied to the shape the handle was dropped on");
    assert!(list(&mut w, a)["callouts"].as_array().unwrap().is_empty(), "and not to the first");
    assert_eq!(list(&mut w, b)["callouts"], json!([c]));
    assert_eq!(undo_steps(&w), steps + 1, "one undo step");
    // The callout is still the selection and its handle is on the new target's edge.
    assert_eq!(w.state().selection(), [c]);
    run_command(&mut w, "edit.undo", json!({}));
    assert_eq!(list(&mut w, c)["target"], a, "undo ties it back");
}

#[test]
fn dropping_the_handle_on_bare_page_unties_the_callout_and_leaves_its_end_there() {
    let (mut w, a, _) = window_with_two();
    w.state_mut().select(&[a]);
    w.rest();
    run_command(&mut w, "window.callout", json!({}));
    let c = *doc_shapes(&w).last().unwrap();
    let steps = undo_steps(&w);
    let target = drag_handle(&mut w, c, (5.0, 8.0));
    release(&mut w, target);
    assert_eq!(list(&mut w, c)["target"], Json::Null, "tied to nothing");
    assert!(list(&mut w, a)["callouts"].as_array().unwrap().is_empty());
    let tip = list(&mut w, c)["tip"].clone();
    assert!((tip[0].as_f64().unwrap() - 5.0).abs() < 0.02 && (tip[1].as_f64().unwrap() - 8.0).abs() < 0.02, "the end is where it was let go: {tip}");
    assert_eq!(undo_steps(&w), steps + 1);
}

#[test]
fn escape_in_the_middle_of_the_drag_and_a_press_that_goes_nowhere_change_nothing() {
    let (mut w, a, b) = window_with_two();
    w.state_mut().select(&[a]);
    w.rest();
    run_command(&mut w, "window.callout", json!({}));
    let c = *doc_shapes(&w).last().unwrap();
    let steps = undo_steps(&w);
    // Escape while the button is down drops the drag: nothing is sent when the button comes up.
    let target = drag_handle(&mut w, c, (7.0, 4.0));
    key(&mut w, Key::Escape, Modifiers::NONE);
    release(&mut w, target);
    assert_eq!(list(&mut w, c)["target"], a);
    assert_eq!(undo_steps(&w), steps);
    // A click on the handle with no drag sends nothing.
    let handle = w.state().callout_tip_point(c).expect("still selected");
    click(&mut w, handle);
    w.rest();
    assert_eq!(undo_steps(&w), steps);
    assert_eq!(list(&mut w, b)["callouts"], json!([]));
}

#[test]
fn the_gallery_and_the_handle_are_drawn_in_the_light_and_the_dark_theme() {
    for theme in ["light", "dark"] {
        let (mut w, a, _) = window_with_two();
        run_command(&mut w, "window.theme", json!({"theme": theme}));
        w.state_mut().select(&[a]);
        w.rest();
        press(&mut w, "ribbon.Insert.CalloutGallery");
        w.rest();
        assert!(w.state().element("menu.callout.speech_box").is_some(), "{theme}: the gallery is drawn");
        click_on(&mut w, "menu.callout.speech_box");
        let c = *doc_shapes(&w).last().unwrap();
        assert!(w.state().callout_tip_point(c).is_some(), "{theme}: the handle is drawn");
    }
}
