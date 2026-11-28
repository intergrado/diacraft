//! The rest of the readers that went round the cells' formulas: the text block of a shape (`TxtWidth` and the like) and the line a guide draws (the `InfiniteLine` row
//! of a shape that is a guide), each read as the picture reads a cell that holds a formula and no saved value, by what the formula works out to.

use diacraft_doc::{Cell, CellAddr, Document, RowKey, ShapeType, Value};
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

/// A box 2 by 1 pinned at its middle at (3, 3).
fn boxed(id: u32) -> ShapeBuilder {
    ShapeBuilder::new(id)
        .cell("PinX", "3")
        .cell("PinY", "3")
        .cell("Width", "2")
        .cell("Height", "1")
        .cell("LocPinX", "1")
        .cell("LocPinY", "0.5")
        .cell("Angle", "0")
}

#[test]
fn the_text_block_of_a_shape_whose_cells_are_a_formula_and_no_saved_value_is_where_the_formula_puts_it() {
    // A text block 3 wide and 0.5 high, its pin at its middle: the cells that say so hold a formula and no value.
    let block = |name: &str, formula: &str| (CellAddr::direct(name), Cell::with_formula(formula, None));
    let mut shape = boxed(1);
    for (name, formula) in [
        ("TxtPinX", "1"),
        ("TxtPinY", "0.5"),
        ("TxtWidth", "1+2"),
        ("TxtHeight", "0.5"),
        ("TxtLocPinX", "1.5"),
        ("TxtLocPinY", "0.25"),
        ("TxtAngle", "0"),
    ] {
        let (addr, cell) = block(name, formula);
        shape = shape.set(addr, cell);
    }
    let s = session_with(DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build());
    let place = diacraft_engine::draw::text_block_place(s.document().unwrap(), 1, 1).unwrap();
    assert!((place.width - 3.0).abs() < 1e-9 && (place.height - 0.5).abs() < 1e-9, "its size: {place:?}");
    assert!(
        (place.pin.0 - 3.0).abs() < 1e-9 && (place.pin.1 - 3.0).abs() < 1e-9,
        "its pin: the shape's at (3, 3) less its own pin and plus the block's (1, 0.5): {place:?}"
    );
}

/// A vertical guide through x = 5, as the `InfiniteLine` row of a shape that is a guide: the point (`X`, `Y`) and the point (`A`, `B`), each a constant or a formula and no value.
fn guide(as_formulas: bool) -> ShapeBuilder {
    let mut shape = ShapeBuilder::new(10).kind(ShapeType::Guide).cell("PinX", "0").cell("PinY", "0").cell("Angle", "0");
    let key = RowKey::Ix(1);
    shape = shape.row_type("Geometry", Some(0), key.clone(), "InfiniteLine");
    for (name, formula, value) in [("X", "2+3", "5"), ("Y", "0+0", "0"), ("A", "2+3", "5"), ("B", "0+1", "1")] {
        let addr = CellAddr::in_row("Geometry", Some(0), key.clone(), name);
        shape =
            if as_formulas { shape.set(addr, Cell::with_formula(formula, None)) } else { shape.set(addr, Cell::constant(Value::from_text(value))) };
    }
    shape
}

#[test]
fn the_line_a_guide_draws_is_where_the_formulas_of_its_row_put_it() {
    // Only guides are targets. The guide is the line x = 5; B (2 wide, pinned at 3) is moved so that its right side is 0.04 short of it.
    let drive = |as_formulas: bool| -> f64 {
        let page = PageBuilder::new().cell("PageWidth", "11").cell("PageHeight", "8.5").shape(guide(as_formulas)).shape(boxed(2));
        let mut s = session_with(DocBuilder::new().page(1, page).build());
        run(
            &mut s,
            "snap.set",
            json!({"ruler": false, "grid": false, "alignment_box": false, "intersections": false, "guides": true, "snap_geometry": false,
                   "snap_handles": false, "snap_vertices": false, "snap_connection_points": false, "dynamic_grid": false}),
        );
        run(&mut s, "shape.move", json!({"ids": [2], "dx": 5.0 - 0.04 - 4.0, "dy": 0.0, "snap": true, "page": 1}));
        let doc = s.document().unwrap();
        let at = doc.find_shape(diacraft_doc::Owner::Page(1), 2).unwrap();
        doc.shape(&at).unwrap().sheet.cell("PinX").and_then(|c| c.value.as_ref()).and_then(|v| v.number()).unwrap()
    };
    let control = drive(false);
    assert!((control - 4.0).abs() < 1e-6, "the control, with the row saved: B's right side comes to the guide at 5: {control}");
    let worked = drive(true);
    assert!((worked - 4.0).abs() < 1e-6, "the row is formulas and no value: {worked}");
}
