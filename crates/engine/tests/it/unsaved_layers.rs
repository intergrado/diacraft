//! The flags of a layer that are a formula and no saved value: a layer whose `Visible` is the formula `0` is hidden, as the picture has it, and its shapes are not
//! picked; one whose `Lock` is `1` is locked. The table of layers (`layer.list`, and the layers the window's pointer reads through `page_standing`) read such a cell by what the
//! formula works out to, and not as a cell that holds nothing (the default of its flag: visible, unlocked).

use diacraft_doc::{Cell, CellAddr, Document, RowKey};
use diacraft_engine::Session;
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};
use serde_json::json;

use crate::{blank_session, run};

fn session_with(built: Document) -> Session {
    let mut s = blank_session();
    s.edit("build", |d| {
        *d = built;
        Ok(())
    })
    .unwrap();
    s
}

/// A page with one layer, 0, whose cells are the formulas in `cells` and no value, and shape 1 on it.
fn page_with_a_layer(cells: &[(&str, &str)]) -> Document {
    let at = |name: &str| CellAddr::in_row("Layer", None, RowKey::Ix(0), name);
    let mut page = PageBuilder::new().cell("PageWidth", "11").cell("PageHeight", "8.5");
    for (name, formula) in cells {
        page = page.set(at(name), Cell::with_formula(formula, None));
    }
    let shape = ShapeBuilder::new(1)
        .cell("PinX", "3")
        .cell("PinY", "3")
        .cell("Width", "2")
        .cell("Height", "1")
        .cell("LocPinX", "1")
        .cell("LocPinY", "0.5")
        .cell("LayerMember", "0");
    DocBuilder::new().page(1, page.shape(shape)).build()
}

#[test]
fn the_flags_and_the_name_of_a_layer_that_are_a_formula_and_no_saved_value_are_what_the_formula_works_out_to() {
    let mut s = session_with(page_with_a_layer(&[("Name", "\"Wiring\""), ("Visible", "0"), ("Lock", "1"), ("Print", "1-1"), ("Snap", "1")]));
    let listed = run(&mut s, "layer.list", json!({"page": 1}));
    let layer = &listed["layers"][0];
    assert_eq!(layer["name"], json!("Wiring"), "{listed}");
    assert_eq!(
        (layer["visible"].as_bool(), layer["lock"].as_bool(), layer["print"].as_bool(), layer["snap"].as_bool()),
        (Some(false), Some(true), Some(false), Some(true)),
        "{listed}"
    );
}

#[test]
fn a_layer_with_no_cells_at_all_keeps_the_defaults_of_its_flags() {
    // The control: a cell that is nowhere reads its default (visible, unlocked), as ever.
    let mut s = session_with(page_with_a_layer(&[("Name", "\"Plain\"")]));
    let layer = run(&mut s, "layer.list", json!({"page": 1}))["layers"][0].clone();
    assert_eq!((layer["visible"].as_bool(), layer["lock"].as_bool()), (Some(true), Some(false)));
}

// ----: a layer's colour and a shape's list of layers -------------------------------------------------------------------------------

/// A page with one layer, 0, named and with `colour` as its `Color` cell.
fn page_with_a_coloured_layer(colour: Cell) -> Document {
    let at = |name: &str| CellAddr::in_row("Layer", None, RowKey::Ix(0), name);
    let page = PageBuilder::new()
        .cell("PageWidth", "11")
        .cell("PageHeight", "8.5")
        .set(at("Name"), Cell::constant(diacraft_doc::Value::from_text("Wiring")))
        .set(at("Color"), colour);
    DocBuilder::new().page(1, page).build()
}

fn colour_of_layer_0(colour: Cell) -> serde_json::Value {
    let mut s = session_with(page_with_a_coloured_layer(colour));
    run(&mut s, "layer.list", json!({"page": 1}))["layers"][0]["color"].clone()
}

#[test]
fn the_colour_of_a_layer_that_is_a_formula_and_no_saved_value_is_what_the_formula_works_out_to() {
    // A colour function, and an index of the colour table as a number: each as a cell with a value saved would be.
    assert_eq!(colour_of_layer_0(Cell::with_formula("RGB(255,128,0)", None)), json!("#ff8000"));
    let saved = colour_of_layer_0(Cell::constant(diacraft_doc::Value::from_text("2")));
    assert!(saved.is_string(), "the control has a colour: {saved}");
    assert_eq!(colour_of_layer_0(Cell::with_formula("1+1", None)), saved, "the index 2 of a formula is the index 2 of a value");
}

/// A page with layers 0 and 1 and a shape whose `LayerMember` is `formula` and no value.
fn page_with_a_member_list(formula: &str) -> Document {
    let at = |ix: u32, name: &str| CellAddr::in_row("Layer", None, RowKey::Ix(ix), name);
    let page = PageBuilder::new()
        .cell("PageWidth", "11")
        .cell("PageHeight", "8.5")
        .set(at(0, "Name"), Cell::constant(diacraft_doc::Value::from_text("Back")))
        .set(at(1, "Name"), Cell::constant(diacraft_doc::Value::from_text("Front")));
    let shape = ShapeBuilder::new(1)
        .cell("PinX", "3")
        .cell("PinY", "3")
        .cell("Width", "2")
        .cell("Height", "1")
        .cell("LocPinX", "1")
        .cell("LocPinY", "0.5")
        .set(CellAddr::direct("LayerMember"), Cell::with_formula(formula, None));
    DocBuilder::new().page(1, page.shape(shape)).build()
}

#[test]
fn a_list_of_layers_that_is_a_formula_and_no_saved_value_puts_the_shape_on_the_layers_it_works_out_to() {
    let mut s = session_with(page_with_a_member_list("\"1\""));
    let layers = run(&mut s, "layer.list", json!({"page": 1}))["layers"].clone();
    assert_eq!((layers[0]["shapes"].as_u64(), layers[1]["shapes"].as_u64()), (Some(0), Some(1)), "the formula says layer 1: {layers}");
    // A formula that fails gives no list: the shape is on no layer.
    let mut s = session_with(page_with_a_member_list("1/0"));
    let layers = run(&mut s, "layer.list", json!({"page": 1}))["layers"].clone();
    assert_eq!((layers[0]["shapes"].as_u64(), layers[1]["shapes"].as_u64()), (Some(0), Some(0)), "{layers}");
}
