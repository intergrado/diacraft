//! In the window: Home, Editing, Change Shape is quiet until a shape is selected, opens a gallery of the masters that fit the selection, and a press on
//! a tile sends `arrange.changeShape` (`shape.change` of the selection, one undo step). The pointer is given to egui as a person's would be, and what is checked is the
//! document the command changed and the selection the window kept.

use serde_json::json;

use crate::editing::{edit_window, three_shapes};
use crate::support::{Rest, Window, elements_named, press, run_command};

const CONTROL: &str = "ribbon.Home.ReplaceShape";

fn selection(w: &Window) -> Vec<u32> {
    w.state().selection().to_vec()
}

/// The name of the master shape `id` of the first page is an instance of.
fn master_of(w: &Window, id: u32) -> Option<String> {
    let doc = w.state().session().document()?;
    let page = doc.pages.get(0)?.id;
    let at = doc.find_shape(diacraft_engine::doc::Owner::Page(page), id)?;
    let master = doc.shape(&at)?.master?;
    doc.master(master)?.name.as_deref().map(str::to_owned)
}

fn undo_steps(w: &Window) -> usize {
    w.state().session().history().map_or(0, |h| h.undo_len())
}

#[test]
fn change_shape_is_quiet_until_a_shape_is_selected() {
    let mut w = edit_window([1280.0, 900.0]);
    three_shapes(&mut w);
    run_command(&mut w, "edit.select", json!({"ids": []}));
    w.rest();
    assert!(!w.state().element(CONTROL).expect("the control is drawn").enabled, "nothing is selected");
    assert_eq!(w.state().command_enabled("arrange.changeShape"), Err("nothing is selected".to_owned()));
    let refused = w.state_mut().execute("arrange.changeShape", &json!({"master": "hexagon"})).unwrap_err().to_string();
    assert!(refused.contains("nothing is selected"), "{refused}");
    run_command(&mut w, "edit.select", json!({"ids": [1]}));
    w.rest();
    assert!(w.state().element(CONTROL).unwrap().enabled);
}

#[test]
fn the_gallery_lists_the_masters_that_fit_and_a_press_on_a_tile_changes_the_selection_in_one_step() {
    let mut w = edit_window([1280.0, 900.0]);
    let ids = three_shapes(&mut w);
    run_command(&mut w, "edit.select", json!({"ids": [ids[0], ids[2]]}));
    w.rest();
    press(&mut w, CONTROL);
    let tiles = elements_named(&w, "ribbon.Home.changeShape.");
    assert!(tiles.iter().any(|t| t.0.ends_with(".hexagon")), "the built-in masters are tiles: {} of them", tiles.len());
    assert!(tiles.iter().all(|t| !t.0.ends_with("gane-sarson.data-flow")), "a connector master is not offered to shapes that are not connectors");
    let steps = undo_steps(&w);
    press(&mut w, "ribbon.Home.changeShape.hexagon");
    assert_eq!(master_of(&w, ids[0]).as_deref(), Some("Hexagon"));
    assert_eq!(master_of(&w, ids[2]).as_deref(), Some("Hexagon"));
    assert_eq!(master_of(&w, ids[1]).as_deref(), Some("Diamond"), "a shape not selected is left alone");
    assert_eq!(undo_steps(&w), steps + 1, "one undo step");
    assert_eq!(selection(&w), vec![ids[0], ids[2]], "the shapes keep their IDs, so the selection stays");
    assert_eq!(w.state().ui_panics(), 0);
    run_command(&mut w, "edit.undo", json!({}));
    assert_eq!(master_of(&w, ids[0]).as_deref(), Some("Rectangle"));
}

#[test]
fn a_connector_is_offered_connector_masters_only_and_a_mixed_selection_both() {
    let mut w = edit_window([1280.0, 900.0]);
    let ids = three_shapes(&mut w);
    let c = w.state_mut().execute("connector.add", &json!({"from": {"id": ids[0]}, "to": {"id": ids[1]}})).unwrap();
    let c = u32::try_from(c["id"].as_u64().unwrap()).unwrap();
    run_command(&mut w, "edit.select", json!({"ids": [c]}));
    w.rest();
    press(&mut w, CONTROL);
    let tiles = elements_named(&w, "ribbon.Home.changeShape.");
    assert!(tiles.iter().any(|t| t.0.ends_with("gane-sarson.data-flow")), "the connector master is offered: {tiles:?}");
    assert!(tiles.iter().all(|t| !t.0.ends_with(".hexagon")), "a shape master is not offered to a connector");
    press(&mut w, "ribbon.Home.changeShape.gane-sarson.data-flow");
    assert_eq!(master_of(&w, c).as_deref(), Some("Data Flow"));
    // A selection of a shape and a connector is offered both kinds, and the one that does not fit is said in the notice.
    run_command(&mut w, "edit.select", json!({"ids": [ids[0], c]}));
    w.rest();
    press(&mut w, CONTROL);
    let tiles = elements_named(&w, "ribbon.Home.changeShape.");
    assert!(tiles.iter().any(|t| t.0.ends_with(".hexagon")) && tiles.iter().any(|t| t.0.ends_with("gane-sarson.data-flow")));
    press(&mut w, "ribbon.Home.changeShape.hexagon");
    assert_eq!(master_of(&w, ids[0]).as_deref(), Some("Hexagon"));
    assert_eq!(master_of(&w, c).as_deref(), Some("Data Flow"), "the connector is left as it was");
    assert!(w.state().notice().is_some_and(|n| n.contains("one-dimensional")), "{:?}", w.state().notice());
}

#[test]
fn the_gallery_opens_in_both_themes() {
    for theme in ["light", "dark"] {
        let mut w = edit_window([1280.0, 900.0]);
        let ids = three_shapes(&mut w);
        run_command(&mut w, "window.theme", json!({"theme": theme}));
        run_command(&mut w, "edit.select", json!({"ids": [ids[0]]}));
        w.rest();
        press(&mut w, CONTROL);
        assert!(!elements_named(&w, "ribbon.Home.changeShape.").is_empty(), "the gallery opens in the {theme} theme");
        assert_eq!(w.state().ui_panics(), 0);
    }
}
