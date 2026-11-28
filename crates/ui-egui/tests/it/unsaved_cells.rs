//! A shape whose frame cells hold a formula and no saved value (a follow-up of public issue 14): the window picks it where the picture draws it.
//!
//! A `PinX` of `=1+1` and no `V` is drawn at 2 inches, and the renderer says so. What a click meets (`Scene`), what a drag moves (`shape.move`), where Size & Position puts the
//! shape and where text typed on a group goes (`IsTextEditTarget`) were read from the saved value alone, which is no value: the shape was picked, moved and shown as if it sat
//! at the origin. The drawings are packages built in code (`tests/it/fixture.rs`): a cell is `F` and no `V`, as a file may leave it.

use diacraft_engine::Session;
use diacraft_testkit::package::{cell, shape, shape_with};
use diacraft_ui_egui::DiacraftApp;
use diacraft_ui_egui::edit::scene::{Kind, Scene};
use diacraft_ui_egui::render_worker::bundled_drawer;
use egui::{Key, Modifiers};
use serde_json::json;

use crate::editing::{click_at, drag_with, frame_of, key, type_text};
use crate::fixture::{self, group, page_with, placed, posed, rectangle};
use crate::support::{Rest, TempDir, Window, fast_window, run_command};

/// A cell that is a formula and has no saved value.
fn formula(name: &str, f: &str) -> String {
    format!(r#"<Cell N="{name}" F="{f}"/>"#)
}

/// A session that has read `bytes` as a file.
fn session_of(dir: &TempDir, bytes: &[u8]) -> Session {
    let path = fixture::write(dir.path(), "unsaved.vsdx", bytes);
    let mut session = Session::new();
    session.execute("file.open", &json!({"path": path.to_str().unwrap()})).unwrap();
    session
}

/// A window at 100% on the drawing `bytes`.
fn window_of(dir: &TempDir, bytes: &[u8]) -> Window {
    let mut w = fast_window(DiacraftApp::new(session_of(dir, bytes), None).with_inline_pictures(bundled_drawer()), [1280.0, 900.0]);
    w.rest();
    run_command(&mut w, "view.zoom", json!({"percent": 100}));
    w
}

fn scene_of(session: &Session) -> Scene {
    let doc = session.document().unwrap();
    Scene::build(doc, doc.pages.get(0).unwrap().id, session.revision()).unwrap()
}

/// Half a second of frames: the next click is not the second of a double click.
fn pause(w: &mut Window) {
    for _ in 0..30 {
        w.step();
    }
}

/// A box 2 by 1 inches whose pin is at the middle of it, at (x, y), the cells in `formulas` being a formula and no value in place of the constant.
fn box_with(id: u32, formulas: &[(&str, &str)], geometry: bool) -> String {
    let mut cells = String::new();
    for (name, value) in [("PinX", "4"), ("PinY", "5"), ("Width", "2"), ("Height", "1"), ("LocPinX", "1"), ("LocPinY", "0.5"), ("Angle", "0")] {
        cells += &match formulas.iter().find(|(named, _)| named == &name) {
            Some((_, f)) => formula(name, f),
            None => cell(name, value),
        };
    }
    if geometry {
        cells += &rectangle(2.0, 1.0);
    }
    shape(id, &cells)
}

// ---- what a click meets ----------------------------------------------------------------------------------------------------------------------------

#[test]
fn the_scene_puts_a_shape_whose_pin_is_a_formula_and_no_saved_value_where_the_formula_puts_it() {
    // Pinned at x = 2 by `=1+1`: its box is 1 to 3 across and 4.5 to 5.5 up.
    let dir = TempDir::new("unsaved-scene-pin");
    let session = session_of(&dir, &page_with(&box_with(1, &[("PinX", "1+1")], true)));
    let scene = scene_of(&session);
    let item = scene.get(1).expect("the shape is on the page");
    assert_eq!(item.kind, Kind::Shape);
    let (lo, hi) = item.bounds;
    assert!(
        (lo.0 - 1.0).abs() < 1e-9 && (hi.0 - 3.0).abs() < 1e-9 && (lo.1 - 4.5).abs() < 1e-9 && (hi.1 - 5.5).abs() < 1e-9,
        "its box: {:?}",
        item.bounds
    );
    assert!((item.pin.0 - 2.0).abs() < 1e-9, "its pin: {:?}", item.pin);
    assert_eq!(scene.hit((2.0, 5.0), 0.05).map(|item| item.id), Some(1), "a click at its middle meets it");
    assert!(scene.hit((4.0, 5.0), 0.05).is_none(), "and none at 4, where a pin of 4 would put it");
}

#[test]
fn the_scene_gives_a_shape_whose_width_is_a_formula_and_no_saved_value_its_width() {
    // `=1+3` is 4 across: from 2 to 6 about the pin at 4. (No geometry: it is the box that is hit.)
    let dir = TempDir::new("unsaved-scene-width");
    let session = session_of(&dir, &page_with(&box_with(1, &[("Width", "1+3"), ("LocPinX", "2")], false)));
    let scene = scene_of(&session);
    assert_eq!(scene.hit((5.5, 5.0), 0.05).map(|item| item.id), Some(1), "the right end of a shape 4 wide");
    let item = scene.get(1).unwrap();
    assert!((item.frame.1.0 - item.frame.0.0 - 4.0).abs() < 1e-9, "its frame is 4 wide: {:?}", item.frame);
}

#[test]
fn the_scene_turns_a_shape_whose_angle_is_a_formula_and_no_saved_value() {
    // A quarter turn about its middle: the box 2 by 1 stands 1 across and 2 up, from 5.5 to 3.5 (x 3.5 to 4.5, y 4 to 6).
    let dir = TempDir::new("unsaved-scene-angle");
    let session = session_of(&dir, &page_with(&box_with(1, &[("Angle", "PI()/2")], true)));
    let scene = scene_of(&session);
    let item = scene.get(1).unwrap();
    assert!(item.turned && (item.degrees - 90.0).abs() < 1e-9, "turned {} degrees", item.degrees);
    assert_eq!(scene.hit((4.0, 5.8), 0.05).map(|item| item.id), Some(1), "its upper end, which it reaches only turned");
    assert!(scene.hit((4.9, 5.0), 0.05).is_none(), "and not the right end it has unturned");
}

#[test]
fn a_click_at_the_drawn_place_of_a_shape_pinned_by_a_formula_and_no_saved_value_selects_it() {
    let dir = TempDir::new("unsaved-click");
    let mut w = window_of(&dir, &page_with(&box_with(1, &[("PinX", "1+1")], true)));
    click_at(&mut w, 2.0, 5.0);
    assert_eq!(w.state().selection(), [1], "the shape is where the picture shows it");
    assert_eq!(w.state().ui_panics(), 0);
}

#[test]
fn a_drag_moves_a_shape_pinned_by_a_formula_and_no_saved_value_from_where_it_is() {
    let dir = TempDir::new("unsaved-drag");
    let mut w = window_of(&dir, &page_with(&box_with(1, &[("PinX", "1+1")], true)));
    click_at(&mut w, 2.0, 5.0);
    pause(&mut w);
    let before = frame_of(&w, 1);
    assert!((before.0 - 1.0).abs() < 1e-9, "the box starts at 1: {before:?}");
    drag_with(&mut w, (2.0, 5.0), (4.0, 5.0), Modifiers::NONE);
    let after = frame_of(&w, 1);
    assert!((after.0 - 3.0).abs() < 1e-6, "two inches along from 1: {after:?}");
    assert_eq!(w.state().ui_panics(), 0);
}

// ---- IsTextEditTarget ------------------------------------------------------------------------------------------------------------------------------

/// A group at (4, 5), 2 by 2, whose `IsTextEditTarget` is `target` as a formula and no value, over two boxes: 11 below and 12 on top.
fn class_with(target: &str) -> Vec<u8> {
    let member = |id: u32, y: f64, words: &str| {
        shape_with(id, "", &[placed(0.0, y, 2.0, 1.0), rectangle(2.0, 1.0), format!(r#"<Text><cp IX="0"/>{words}</Text>"#)].concat())
    };
    let members = [member(11, 0.0, "Operations"), member(12, 1.0, "Name")].concat();
    page_with(&group(10, &placed(4.0, 5.0, 2.0, 2.0), &formula("IsTextEditTarget", target), &members))
}

#[test]
fn typing_on_a_group_whose_is_text_edit_target_is_a_formula_that_gives_false_and_no_saved_value_edits_its_last_member() {
    let dir = TempDir::new("unsaved-text-target");
    let mut w = window_of(&dir, &class_with("FALSE"));
    click_at(&mut w, 5.0, 6.0);
    assert_eq!(w.state().selection(), [10], "the first click takes the group");
    pause(&mut w);
    key(&mut w, Key::F2, Modifiers::NONE);
    assert_eq!(w.state().edited_text_shape(), Some(12), "the cell works out to FALSE: the text is the member on top of the stack's");
    type_text(&mut w, "Zed");
    key(&mut w, Key::Escape, Modifiers::NONE);
    assert_eq!(w.state().ui_panics(), 0);
}

#[test]
fn typing_on_a_group_whose_is_text_edit_target_is_a_formula_that_gives_true_and_no_saved_value_edits_the_group() {
    let dir = TempDir::new("unsaved-text-target-true");
    let mut w = window_of(&dir, &class_with("1"));
    click_at(&mut w, 5.0, 6.0);
    pause(&mut w);
    key(&mut w, Key::F2, Modifiers::NONE);
    assert_eq!(w.state().edited_text_shape(), Some(10), "the cell works out to TRUE: the group takes its own text");
    key(&mut w, Key::Escape, Modifiers::NONE);
}

// ---- a layer that is a formula and no saved value ---------------------------------------------------------------------------------------------------

#[test]
fn a_shape_on_a_layer_that_is_locked_by_a_formula_and_no_saved_value_is_not_selectable() {
    use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};

    // The layer's `Lock` is `=1` and `Visible` `=1`, no value: locked, as the picture's layer is, and the pointer passes over its shape.
    let at = |name: &str| diacraft_engine::doc::CellAddr::in_row("Layer", None, diacraft_engine::doc::RowKey::Ix(0), name);
    let layer = |name: &str, f: &str| (at(name), diacraft_engine::doc::Cell::with_formula(f, None));
    let mut page = PageBuilder::new().cell("PageWidth", "11").cell("PageHeight", "8.5");
    for (name, f) in [("Name", "\"Wiring\""), ("Lock", "1"), ("Visible", "1")] {
        let (addr, cell) = layer(name, f);
        page = page.set(addr, cell);
    }
    let on_it = ShapeBuilder::new(1)
        .cell("PinX", "3")
        .cell("PinY", "3")
        .cell("Width", "2")
        .cell("Height", "1")
        .cell("LocPinX", "1")
        .cell("LocPinY", "0.5")
        .cell("LayerMember", "0");
    let off_it =
        ShapeBuilder::new(2).cell("PinX", "7").cell("PinY", "3").cell("Width", "2").cell("Height", "1").cell("LocPinX", "1").cell("LocPinY", "0.5");
    let built = DocBuilder::new().page(1, page.shape(on_it).shape(off_it)).build();
    let mut session = Session::new();
    session.execute("file.new", &json!({})).unwrap();
    session
        .edit("build", |d| {
            *d = built;
            Ok(())
        })
        .unwrap();
    let scene = scene_of(&session);
    assert!(!scene.selectable(1), "a locked layer's shape is passed over");
    assert!(scene.selectable(2), "and a shape on no layer is picked");
    assert!(scene.hit((3.0, 3.0), 0.05).is_none() && scene.hit((7.0, 3.0), 0.05).map(|item| item.id) == Some(2));
}

// ---- a placed pair of shapes: posed() is read as saved ---------------------------------------------------------------------------------------------

#[test]
fn shapes_that_hold_their_values_are_where_they_were() {
    // The control: the same boxes with every cell saved.
    let dir = TempDir::new("unsaved-control");
    let session = session_of(&dir, &page_with(&shape(1, &[posed(2.0, 5.0, 2.0, 1.0, (1.0, 0.5), 0.0), rectangle(2.0, 1.0)].concat())));
    let scene = scene_of(&session);
    assert_eq!(scene.hit((2.0, 5.0), 0.05).map(|item| item.id), Some(1));
}
