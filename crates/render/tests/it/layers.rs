//! Layers.

use diacraft_doc::RowKey;
use diacraft_testkit::build::PageBuilder;

use crate::support::*;

fn layer_row(p: PageBuilder, ix: u32, visible: &str) -> PageBuilder {
    let key = RowKey::Ix(ix);
    p.row_cell("Layer", None, key.clone(), "Visible", visible)
        .row_cell("Layer", None, key.clone(), "Name", &format!("Layer{ix}"))
        .row_cell("Layer", None, key.clone(), "NameUniv", &format!("Layer{ix}"))
        .row_cell("Layer", None, key, "Status", "0")
}

fn on(layers: &str, x: f64, color: &str) -> diacraft_testkit::build::ShapeBuilder {
    rect(10 + x as u32, x, 1.0, 0.8, 0.8).cell("FillForegnd", color).cell("LayerMember", layers)
}

#[test]
fn a_shape_is_hidden_only_when_every_layer_it_is_on_is_hidden() {
    // Layer 0 is hidden and layer 1 shown.
    let p = layer_row(layer_row(page(4.0, 4.0), 0, "0"), 1, "1")
        .shape(on("0", 0.0, "#FF0000"))
        .shape(on("1", 1.0, "#FF0000"))
        .shape(on("0;1", 2.0, "#FF0000"))
        .shape(on("", 3.0, "#FF0000"));
    let pic = draw(&base().page(1, p).build());
    assert!(pic.is_white(0.4, 1.4), "on the hidden layer only: not drawn");
    assert!(pic.is_red(1.4, 1.4), "on a shown layer");
    assert!(pic.is_red(2.4, 1.4), "on a hidden and a shown layer: drawn");
    assert!(pic.is_red(3.4, 1.4), "on no layer: drawn");
}

#[test]
fn a_member_of_a_hidden_group_stays_unless_its_own_layers_hide_it() {
    // Each shape goes by its own list. The group's own drawing is hidden with it.
    let group = on("0", 0.0, "#0000FF")
        .cell("PinX", "0.5")
        .cell("PinY", "0.5")
        .cell("Width", "3")
        .cell("Height", "3")
        .group()
        .child(rect(20, 0.5, 0.5, 0.5, 0.5).cell("FillForegnd", "#FF0000"))
        .child(rect(21, 1.5, 0.5, 0.5, 0.5).cell("FillForegnd", "#FF0000").cell("LayerMember", "0"));
    let group = geometry_of(group, rect_rows(3.0, 3.0));
    let p = layer_row(page(4.0, 4.0), 0, "0").shape(group);
    let pic = draw(&base().page(1, p).build());
    assert!(pic.is_white(0.6, 0.6), "the group's own drawing is on the hidden layer");
    assert!(pic.is_red(1.2, 1.2), "the member on no layer is drawn");
    assert!(pic.is_white(2.2, 1.2), "the member on the hidden layer is not");
}

#[test]
fn a_list_is_read_by_row_number_and_a_vacant_row_keeps_its_number() {
    // Row 1 is vacant (status 2, no name) and row 2 hidden; a shape on 2 is hidden. Counting named rows only would not find a
    // third row and would draw it.
    let mut p = layer_row(page(4.0, 4.0), 0, "1");
    p = p.row_cell("Layer", None, RowKey::Ix(1), "Visible", "0").row_cell("Layer", None, RowKey::Ix(1), "Status", "2").row_cell(
        "Layer",
        None,
        RowKey::Ix(1),
        "Name",
        "",
    );
    p = layer_row(p, 2, "0");
    let p = p.shape(on("2", 0.0, "#FF0000")).shape(on("1", 1.0, "#FF0000")).shape(on("9", 2.0, "#FF0000"));
    let pic = draw(&base().page(1, p).build());
    assert!(pic.is_white(0.4, 1.4), "on row 2, hidden");
    assert!(pic.is_red(1.4, 1.4), "a list that names only a vacant row names no layer");
    assert!(pic.is_red(2.4, 1.4), "an index past the table names no row: the shape is drawn");
}

#[test]
fn a_background_page_goes_by_its_own_layer_table() {
    // The foreground hides layer 0, and the background's layer 0 is shown.
    let back = layer_row(page(4.0, 4.0), 0, "1").background().shape(on("0", 0.0, "#00FF00"));
    let front = layer_row(page(4.0, 4.0), 0, "0").back_page(2).shape(on("0", 2.0, "#FF0000"));
    let pic = draw(&base().page(1, front).page(2, back).build());
    assert!(pic.is(0.4, 1.4, (0, 255, 0), 6), "the background's shape on its own shown layer");
    assert!(pic.is_white(2.4, 1.4), "the foreground's on a layer it hides");
}

#[test]
fn a_layer_colour_goes_over_a_shape_on_exactly_that_one_layer() {
    // The line takes the layer's colour and the fill turns white. A shape on two layers keeps its own colours.
    let mut p = layer_row(layer_row(page(4.0, 4.0), 0, "1"), 1, "1");
    p = p.row_cell("Layer", None, RowKey::Ix(0), "Color", "#00FF00").row_cell("Layer", None, RowKey::Ix(0), "ColorTrans", "0");
    p = p.row_cell("Layer", None, RowKey::Ix(1), "Color", "#FF00FF").row_cell("Layer", None, RowKey::Ix(1), "ColorTrans", "0");
    let one = on("0", 0.0, "#FF0000").cell("LineWeight", "0.1").cell("LineColor", "#000000");
    let two = on("0;1", 2.0, "#FF0000").cell("LineWeight", "0.1").cell("LineColor", "#000000");
    let pic = draw(&base().page(1, p.shape(one).shape(two)).build());
    assert!(pic.is(0.0, 1.4, (0, 255, 0), 8) || pic.is(0.02, 1.4, (0, 255, 0), 8), "the line is the layer's green, got {:?}", pic.at(0.02, 1.4));
    assert!(pic.is_white(0.4, 1.4), "the fill is white in place of the shape's own red");
    assert!(pic.is_red(2.4, 1.4), "on two layers: the shape's own fill");
    assert!(pic.is_dark(2.0, 1.4), "and its own line");
}
