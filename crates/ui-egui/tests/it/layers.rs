//! In the window: a locked or hidden layer's shapes are not picked by a click, a rubber band or Select All, the Layers menu opens the Layer Properties and Assign to
//! Layer dialogs, and OK sends one `command.batch` that is one undo step. The pointer is given to egui as a person's would be, and what is checked is the document the commands
//! changed.

use serde_json::{Value, json};

use crate::editing::{click_at, drag_with, edit_window, frame_of, three_shapes, type_text};
use crate::support::{Rest, Window, press, run_command};

fn centre(w: &Window, id: u32) -> (f64, f64) {
    let (x, y, width, height) = frame_of(w, id);
    (x + width / 2.0, y + height / 2.0)
}

/// A click on the middle of shape `id`.
fn click_on(w: &mut Window, id: u32) {
    let (x, y) = centre(w, id);
    click_at(w, x, y);
}

fn selection(w: &Window) -> Vec<u32> {
    w.state().window_state()["selection"].as_array().unwrap().iter().map(|v| u32::try_from(v.as_u64().unwrap()).unwrap()).collect()
}

fn layers(w: &mut Window) -> Vec<Value> {
    w.state_mut().execute("layer.list", &json!({})).unwrap()["layers"].as_array().unwrap().clone()
}

/// The text of the `LayerMember` cell of shape `id` of the first page.
fn members(w: &Window, id: u32) -> Option<String> {
    let doc = w.state().session().document().unwrap();
    let page = doc.pages.get(0).unwrap().id;
    let at = doc.find_shape(diacraft_engine::doc::Owner::Page(page), id)?;
    doc.shape(&at)?.sheet.cell("LayerMember").and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned())
}

#[test]
fn a_shape_on_a_locked_or_hidden_layer_cannot_be_picked() {
    let mut w = edit_window([1280.0, 900.0]);
    let ids = three_shapes(&mut w);
    run_command(&mut w, "layer.add", json!({"name": "Held"}));
    run_command(&mut w, "layer.add", json!({"name": "Away"}));
    run_command(&mut w, "layer.assign", json!({"ids": [ids[0]], "layers": ["Held"]}));
    run_command(&mut w, "layer.assign", json!({"ids": [ids[1]], "layers": ["Away"]}));
    // Plain layers: a click picks the shape.
    click_on(&mut w, ids[0]);
    assert_eq!(selection(&w), [ids[0]]);
    // Locked: the click passes over it, and the rubber band and Select All leave it.
    run_command(&mut w, "layer.set", json!({"layer": "Held", "lock": true}));
    assert!(selection(&w).is_empty(), "a shape picked and then locked is no longer selected: {:?}", selection(&w));
    click_at(&mut w, 0.5, 0.5);
    click_on(&mut w, ids[0]);
    assert!(selection(&w).is_empty(), "a locked layer's shape is not selectable: {:?}", selection(&w));
    run_command(&mut w, "edit.selectAll", json!({}));
    assert_eq!(selection(&w), [ids[1], ids[2]], "Select All leaves the locked shape");
    // Hidden: the same, and it is not drawn either.
    run_command(&mut w, "layer.set", json!({"layer": "Away", "visible": false}));
    click_at(&mut w, 0.5, 0.5);
    click_on(&mut w, ids[1]);
    assert!(selection(&w).is_empty(), "a hidden layer's shape is not selectable: {:?}", selection(&w));
    drag_with(&mut w, (1.0, 7.5), (8.4, 2.0), egui::Modifiers::NONE);
    assert_eq!(selection(&w), [ids[2]], "a rubber band over the whole page picks the plain shape alone");
    // Unlocked and shown again, they are picked again.
    run_command(&mut w, "layer.set", json!({"layer": "Held", "lock": false}));
    run_command(&mut w, "layer.set", json!({"layer": "Away", "visible": true}));
    run_command(&mut w, "edit.selectAll", json!({}));
    assert_eq!(selection(&w), [ids[0], ids[1], ids[2]]);
}

#[test]
fn the_layers_menu_opens_the_two_dialogs_and_assign_needs_a_selection() {
    let mut w = edit_window([1280.0, 900.0]);
    let ids = three_shapes(&mut w);
    let properties = w.state().command_enabled("window.layerProperties");
    assert_eq!(properties, Ok(()));
    run_command(&mut w, "edit.select", json!({"ids": []}));
    assert!(w.state().command_enabled("window.assignLayer").is_err(), "nothing is selected");
    run_command(&mut w, "edit.select", json!({"ids": [ids[0]]}));
    assert_eq!(w.state().command_enabled("window.assignLayer"), Ok(()));
    run_command(&mut w, "window.layerProperties", json!({}));
    assert_eq!(w.state().window_state()["layerProperties"], true);
    run_command(&mut w, "window.layerProperties", json!({"close": true}));
    assert_eq!(w.state().window_state()["layerProperties"], false);
    run_command(&mut w, "window.assignLayer", json!({}));
    assert_eq!(w.state().window_state()["assignLayer"], true);
    run_command(&mut w, "window.assignLayer", json!({"close": true}));
    assert_eq!(w.state().window_state()["assignLayer"], false);
}

#[test]
fn layer_properties_makes_changes_and_removes_layers_in_one_undo_step() {
    let mut w = edit_window([1280.0, 900.0]);
    let ids = three_shapes(&mut w);
    run_command(&mut w, "layer.add", json!({"name": "Old"}));
    run_command(&mut w, "layer.assign", json!({"ids": [ids[0]], "layers": ["Old"]}));
    let undo_before = w.state().session().history().unwrap().undo_len();
    run_command(&mut w, "window.layerProperties", json!({}));
    assert_eq!(w.state().layer_properties_names(), ["Old"]);
    // New...: a name is typed and added; the new row's boxes are the new layer's.
    press(&mut w, "layers.new");
    type_text(&mut w, "Notes");
    press(&mut w, "layers.add");
    assert_eq!(w.state().layer_properties_names(), ["Old", "Notes"]);
    // Hide Old, lock Notes, then OK.
    press(&mut w, "layers.row.0.visible");
    press(&mut w, "layers.row.1.lock");
    press(&mut w, "layers.ok");
    w.rest();
    assert!(!w.state().layer_properties_open(), "OK closes the dialog");
    let now = layers(&mut w);
    assert_eq!(now.len(), 2);
    assert_eq!((now[0]["name"].clone(), now[0]["visible"].clone()), (json!("Old"), json!(false)));
    assert_eq!((now[1]["name"].clone(), now[1]["lock"].clone(), now[1]["active"].clone()), (json!("Notes"), json!(true), json!(false)));
    let history = w.state().session().history().unwrap();
    assert_eq!(history.undo_len(), undo_before + 1, "the whole dialog is one undo step");
    assert_eq!(history.next_undo_label(), Some("Layer properties"));
    // Remove asks first for a layer that has shapes: keeping them leaves them on no layer.
    run_command(&mut w, "window.layerProperties", json!({}));
    press(&mut w, "layers.row.0.name");
    press(&mut w, "layers.remove");
    assert!(w.state().element("layers.remove_keep").is_some(), "the question is asked");
    press(&mut w, "layers.remove_keep");
    press(&mut w, "layers.ok");
    w.rest();
    assert_eq!(layers(&mut w).len(), 1);
    assert_eq!(members(&w, ids[0]).as_deref(), Some(""));
    assert!(w.state().session().document().unwrap().pages.get(0).unwrap().shapes.iter().any(|s| s.id == ids[0]), "the shape was kept");
}

#[test]
fn a_dialog_that_is_cancelled_sends_nothing_and_a_refused_name_is_said_in_it() {
    let mut w = edit_window([1280.0, 900.0]);
    three_shapes(&mut w);
    run_command(&mut w, "layer.add", json!({"name": "Old"}));
    let revision = w.state().session().revision();
    run_command(&mut w, "window.layerProperties", json!({}));
    press(&mut w, "layers.row.0.print");
    press(&mut w, "layers.cancel");
    w.rest();
    assert!(!w.state().layer_properties_open());
    assert_eq!(w.state().session().revision(), revision, "Cancel sends nothing");
    // A repeated name is refused by the engine and said in the dialog, which stays open.
    run_command(&mut w, "window.layerProperties", json!({}));
    press(&mut w, "layers.new");
    type_text(&mut w, "Old");
    press(&mut w, "layers.add");
    press(&mut w, "layers.ok");
    w.rest();
    assert!(w.state().layer_properties_open(), "the dialog stays open");
    assert!(w.state().layers_dialog_error().is_some_and(|e| e.contains("already")), "{:?}", w.state().layers_dialog_error());
    assert_eq!(layers(&mut w).len(), 1, "and nothing was made");
}

#[test]
fn assign_to_layer_ticks_the_layers_of_the_selection_and_sends_what_changed() {
    let mut w = edit_window([1280.0, 900.0]);
    let ids = three_shapes(&mut w);
    for name in ["Zero", "One"] {
        run_command(&mut w, "layer.add", json!({"name": name}));
    }
    run_command(&mut w, "layer.assign", json!({"ids": [ids[0]], "layers": ["Zero"]}));
    run_command(&mut w, "edit.select", json!({"ids": [ids[0], ids[1]]}));
    run_command(&mut w, "window.assignLayer", json!({}));
    let boxes = w.state().assign_layer_boxes();
    // Zero holds one of the two shapes: mixed; One holds none.
    assert_eq!(boxes.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), ["Zero", "One"]);
    assert_eq!(format!("{:?}", boxes.iter().map(|(_, t)| *t).collect::<Vec<_>>()), "[Mixed, Off]");
    press(&mut w, "assign.layer.1");
    press(&mut w, "assign.ok");
    w.rest();
    assert!(!w.state().assign_layer_open());
    assert_eq!(members(&w, ids[0]).as_deref(), Some("0;1"), "Zero is kept on the shape that was on it, and One is added");
    assert_eq!(members(&w, ids[1]).as_deref(), Some("1"), "a mixed box left alone changes nothing");
    // New... adds a layer and puts the shapes on it.
    run_command(&mut w, "window.assignLayer", json!({}));
    press(&mut w, "assign.new");
    type_text(&mut w, "Fresh");
    press(&mut w, "assign.add");
    press(&mut w, "assign.ok");
    w.rest();
    assert_eq!(layers(&mut w).len(), 3);
    assert_eq!(members(&w, ids[1]).as_deref(), Some("1;2"));
}

/// The page's `Connect` rows: the connector, the end (9 begin, 12 end) and the shape.
fn connects(w: &Window) -> Vec<(u32, Option<u32>, u32)> {
    let page = w.state().session().document().unwrap().pages.get(0).unwrap();
    page.connects.iter().map(|c| (c.from_sheet, c.from_part, c.to_sheet)).collect()
}

/// The Connector tool's drag onto a shape whose layers allow no glue leaves that end free; the other end, on a shape on no layer, is glued.
#[test]
fn the_connector_tool_leaves_an_end_free_on_a_shape_whose_layers_allow_no_glue() {
    let mut w = edit_window([1280.0, 900.0]);
    let ids = three_shapes(&mut w);
    run_command(&mut w, "layer.add", json!({"name": "Bare", "glue": false}));
    run_command(&mut w, "layer.assign", json!({"ids": [ids[1]], "layers": ["Bare"]}));
    run_command(&mut w, "tool.connector", json!({}));
    w.rest();
    drag_with(&mut w, (3.0, 6.0), (7.0, 6.0), egui::Modifiers::NONE);
    let after = crate::editing::shape_ids(&w);
    assert_eq!(after.len(), 4, "a connector was added: {after:?}");
    let joins = connects(&w);
    assert_eq!(joins, [(after[3], Some(9), ids[0])], "the begin is glued and the end, on the shape whose layer takes no glue, is free: {joins:?}");
}

/// Deleting a shape deletes the connectors glued to it that can be picked, and leaves one on a locked layer (the engine would refuse a delete
/// that named it); the shape goes.
#[test]
fn a_shape_deleted_leaves_a_connector_glued_to_it_that_is_on_a_locked_layer() {
    let mut w = edit_window([1280.0, 900.0]);
    let ids = three_shapes(&mut w);
    let joined = w.state_mut().execute("connector.add", &json!({"from": {"id": ids[0]}, "to": {"id": ids[1]}})).unwrap();
    let connector = u32::try_from(joined["id"].as_u64().unwrap()).unwrap();
    run_command(&mut w, "layer.add", json!({"name": "Held"}));
    run_command(&mut w, "layer.assign", json!({"ids": [connector], "layers": ["Held"]}));
    run_command(&mut w, "layer.set", json!({"layer": "Held", "lock": true}));
    w.rest();
    click_on(&mut w, ids[0]);
    assert_eq!(selection(&w), [ids[0]]);
    crate::editing::key(&mut w, egui::Key::Delete, egui::Modifiers::NONE);
    let left = crate::editing::shape_ids(&w);
    assert_eq!(left, [ids[1], ids[2], connector], "the shape is gone and the locked connector stays");
}
