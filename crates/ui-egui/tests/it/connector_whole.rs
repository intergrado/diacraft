//! The Connector tool and the drag of a connector's end on shapes that have no connection point (a public report: the engine refused such a shape, "shape 1 has no connection
//! points to glue to"). A box of a file with no `Connection` section is glued to as a whole: the tool joins two of them, an end dragged onto one is glued to it,
//! and nothing is refused.

use serde_json::json;

use crate::editing::{drag_with, shape_ids};
use crate::support::{Rest, TempDir, Window, run_command};

/// A drawing of boxes with no `Connection` section, 2 by 1 inches, their lower left corners at (2, 6), (7, 6) and (7, 2).
fn plain_boxes_window(dir: &TempDir) -> Window {
    let shapes =
        [crate::fixture::boxed(1, 2.0, 6.0, 2.0, 1.0), crate::fixture::boxed(2, 7.0, 6.0, 2.0, 1.0), crate::fixture::boxed(3, 7.0, 2.0, 2.0, 1.0)]
            .concat();
    let path = crate::fixture::write(dir.path(), "plain-boxes.vsdx", &crate::fixture::page_with(&shapes));
    let mut session = diacraft_engine::Session::new();
    session.execute("file.open", &json!({"path": path.to_str().unwrap()})).unwrap();
    let mut w = crate::support::fast_window(
        diacraft_ui_egui::DiacraftApp::new(session, None).with_inline_pictures(diacraft_ui_egui::render_worker::bundled_drawer()),
        [1280.0, 900.0],
    );
    w.rest();
    run_command(&mut w, "view.fitPage", json!({}));
    w
}

/// The `Connect` rows of the first page: the connector, the end's cell, the shape and `ToPart`.
fn rows(w: &Window) -> Vec<(u32, String, u32, u32)> {
    let page = w.state().session().document().unwrap().pages.get(0).unwrap();
    page.connects.iter().map(|c| (c.from_sheet, c.from_cell.clone().unwrap_or_default().to_string(), c.to_sheet, c.to_part.unwrap_or(0))).collect()
}

#[test]
fn the_connector_tool_joins_two_shapes_with_no_connection_point_glued_to_the_whole_of_each() {
    let dir = TempDir::new("whole-tool");
    let mut w = plain_boxes_window(&dir);
    assert!(diacraft_engine::draw::connection_points(w.state().session().document().unwrap(), 1, 1).is_empty(), "the boxes have no connection point");
    run_command(&mut w, "tool.connector", json!({}));
    w.rest();
    drag_with(&mut w, (3.0, 6.5), (8.0, 6.5), egui::Modifiers::NONE);
    let ids = shape_ids(&w);
    assert_eq!(ids.len(), 4, "a connector was added: {ids:?}");
    let connector = ids[3];
    assert_eq!(rows(&w), [(connector, "BeginX".to_owned(), 1, 3), (connector, "EndX".to_owned(), 2, 3)], "glued to the whole of each box");
    assert_eq!(w.state().selection(), [connector], "the connector is selected, as after any join");
    assert_eq!(w.state().ui_panics(), 0);
}

#[test]
fn a_drag_of_a_connectors_end_onto_a_shape_with_no_connection_point_glues_it_to_the_whole_of_the_shape() {
    let dir = TempDir::new("whole-end");
    let mut w = plain_boxes_window(&dir);
    run_command(&mut w, "tool.connector", json!({}));
    w.rest();
    drag_with(&mut w, (3.0, 6.5), (8.0, 6.5), egui::Modifiers::NONE);
    let connector = shape_ids(&w)[3];
    run_command(&mut w, "tool.pointer", json!({}));
    run_command(&mut w, "edit.select", json!({"ids": [connector]}));
    w.rest();
    let end = w.state().connector_end_point(connector, true).expect("an end handle");
    let end_on_page = w.state().page_point_at(end).unwrap();
    // The end goes onto the box below.
    drag_with(&mut w, end_on_page, (8.0, 2.5), egui::Modifiers::NONE);
    let joins = rows(&w);
    assert!(joins.contains(&(connector, "EndX".to_owned(), 3, 3)), "the end is glued to the whole of the third box: {joins:?}");
    assert!(joins.contains(&(connector, "BeginX".to_owned(), 1, 3)), "the begin stayed: {joins:?}");
    assert_eq!(w.state().ui_panics(), 0);
}
