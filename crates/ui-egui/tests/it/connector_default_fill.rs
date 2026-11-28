//! The owner's ruling of 2026-11-26: a connector DiaCraft makes has No Fill, so a hollow end stays see-through on a coloured box until a person picks a fill colour, and
//! Home > Fill then colours its inside. Each way a person makes a connector in the window is driven by the pointer as a person would: the Connector tool, a drag that starts at a shape's
//! connection point, AutoConnect's arrow clicked and dragged. What is checked is the document the commands made and the picture the renderer makes of it.

use egui::Modifiers;
use serde_json::json;

use crate::autoconnect::{arrow, glide, glued, rest_at, show_arrows};
use crate::connector_fill::{hit, look, page_id, pick_swatch, pixel};
use crate::editing::{at, drag_with, edit_window, shape_ids, three_shapes};
use crate::support::{Rest, Window, button, click, run_command};

const BLUE: [u8; 3] = [0x33, 0x66, 0xCC];

fn near(a: [u8; 3], b: [u8; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 12)
}

/// The point 0.08 inch past the end of connector `id` along its last leg: inside the square of a hollow end and inside the shape the connector ends on.
fn into_the_shape(w: &Window, id: u32) -> (f64, f64) {
    let doc = w.state().session().document().unwrap();
    let points = diacraft_engine::draw::connector_path(doc, page_id(w), id).unwrap().points;
    let (last, before) = (points[points.len() - 1], points[points.len() - 2]);
    let length = (last.0 - before.0).hypot(last.1 - before.1).max(1.0e-9);
    (last.0 + 0.08 * (last.0 - before.0) / length, last.1 + 0.08 * (last.1 - before.1) / length)
}

/// The three shapes of [`three_shapes`], every one with a blue fill.
fn blue_shapes(w: &mut Window) -> Vec<u32> {
    let ids = three_shapes(w);
    let page = page_id(w);
    w.state_mut().execute("shape.format", &json!({"ids": ids, "fill": "#3366CC", "page": page})).unwrap();
    w.rest();
    ids
}

/// The connectors on the page: the shapes that are not among `boxes`.
fn connectors_besides(w: &Window, boxes: &[u32]) -> Vec<u32> {
    shape_ids(w).into_iter().filter(|id| !boxes.contains(id)).collect()
}

/// Asserts that connector `id`, made by a gesture, has no fill, then gives it a hollow end and asserts that the box shows through it, then picks a swatch of Home > Fill and asserts that the
/// swatch colours the inside.
#[track_caller]
fn assert_no_fill_then_fill_it(w: &mut Window, id: u32, what: &str) {
    assert_eq!(look(w, id)["fill"], json!("none"), "{what}: a new connector has no fill");
    let page = page_id(w);
    w.state_mut().execute("shape.format", &json!({"ids": [id], "end_arrow": 21, "end_arrow_size": 6, "weight": 1.5, "page": page})).unwrap();
    w.rest();
    let (x, y) = into_the_shape(w, id);
    assert!(near(pixel(w, x, y), BLUE), "{what}: see-through inside the hollow end, over a coloured box: {:?}", pixel(w, x, y));
    // Home > Fill on it.
    run_command(w, "window.tab", json!({"tab": "Home"}));
    w.state_mut().select(&[id]);
    w.rest();
    hit(w, "ribbon.Home.FillColorGallery");
    let hex = pick_swatch(w, 5);
    let want =
        [u8::from_str_radix(&hex[1..3], 16).unwrap(), u8::from_str_radix(&hex[3..5], 16).unwrap(), u8::from_str_radix(&hex[5..7], 16).unwrap()];
    assert!(near(pixel(w, x, y), want), "{what}: the swatch {hex} colours the inside: {:?}", pixel(w, x, y));
    assert_eq!(w.state().ui_panics(), 0);
}

#[test]
fn a_connector_made_with_the_connector_tool_has_no_fill_and_home_fill_colours_its_hollow_end() {
    let mut w = edit_window([1280.0, 900.0]);
    let boxes = blue_shapes(&mut w);
    run_command(&mut w, "tool.connector", json!({}));
    drag_with(&mut w, (3.0, 6.0), (7.0, 6.0), Modifiers::NONE);
    let made = connectors_besides(&w, &boxes);
    assert_eq!(made.len(), 1, "the tool made one connector: {made:?}");
    assert_no_fill_then_fill_it(&mut w, made[0], "the Connector tool");
}

#[test]
fn a_connector_dragged_from_a_connection_point_has_no_fill_and_home_fill_colours_its_hollow_end() {
    let mut w = edit_window([1280.0, 900.0]);
    let boxes = blue_shapes(&mut w);
    // The right side of the rectangle at (3, 6), 1.5 by 1, is a connection point: (3.75, 6). The drag ends on the ellipse.
    run_command(&mut w, "tool.connector", json!({}));
    drag_with(&mut w, (3.75, 6.0), (5.0, 3.0), Modifiers::NONE);
    let made = connectors_besides(&w, &boxes);
    assert_eq!(made.len(), 1, "one connector: {made:?}");
    assert_no_fill_then_fill_it(&mut w, made[0], "a drag from a connection point");
}

#[test]
fn a_connector_autoconnect_makes_has_no_fill_by_a_click_and_by_a_drag() {
    // A click on an arrow with no neighbour makes a copy of the shape beside it and joins it (the copy keeps the shape's blue).
    let mut w = edit_window([1100.0, 800.0]);
    let a = crate::autoconnect::rectangle_at(&mut w, 4.0, 4.0);
    let page = page_id(&w);
    w.state_mut().execute("shape.format", &json!({"ids": [a], "fill": "#3366CC", "page": page})).unwrap();
    w.rest();
    let on = at(&w, 4.0, 4.0);
    show_arrows(&mut w, on);
    let target = arrow(&w, "right");
    click(&mut w, target);
    w.rest();
    let ids = shape_ids(&w);
    assert_eq!(ids.len(), 3, "the copy and the connector: {ids:?}");
    let connector = ids[2];
    assert_eq!(glued(&w, connector).0, Some(a));
    assert_no_fill_then_fill_it(&mut w, connector, "an AutoConnect arrow clicked");
    // A drag from an arrow onto another shape joins the two.
    let mut w = edit_window([1100.0, 800.0]);
    let a = crate::autoconnect::rectangle_at(&mut w, 3.0, 4.0);
    let c = crate::autoconnect::rectangle_at(&mut w, 7.0, 7.0);
    let page = page_id(&w);
    w.state_mut().execute("shape.format", &json!({"ids": [a, c], "fill": "#3366CC", "page": page})).unwrap();
    w.rest();
    let on = at(&w, 3.0, 4.0);
    show_arrows(&mut w, on);
    let from = arrow(&w, "up");
    let onto = at(&w, 7.0, 7.0);
    rest_at(&mut w, from);
    button(&mut w, from, true);
    glide(&mut w, from, onto);
    button(&mut w, onto, false);
    w.rest();
    let made = connectors_besides(&w, &[a, c]);
    assert_eq!(made.len(), 1, "{made:?}");
    assert_eq!(glued(&w, made[0]), (Some(a), Some(c)));
    assert_no_fill_then_fill_it(&mut w, made[0], "an AutoConnect arrow dragged onto a shape");
}
