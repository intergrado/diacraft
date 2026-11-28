//! A shape whose frame cells hold a formula and no saved value (a follow-up of public issue 14): the commands that edit and show a drawing read such a cell
//! as the picture draws it, by what the formula works out to, and not as a cell that holds nothing (0). A `PinX` of `=1+1` and no `V` is drawn at 2 inches; a click, a move,
//! a snap and Size & Position must find the shape there as well.
//!
//! Each test builds the drawing in code: a page of boxes whose `PinX`, `Width` or `Angle` is a formula with `V` left out, as a file may leave it (`Cell::with_formula(.., None)`),
//! and the control, the same box with the answer written as a value.

use diacraft_doc::{Cell, CellAddr, Document, RowKey};
use diacraft_engine::Session;
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};
use serde_json::{Value, json};

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

/// The cells of a box that is 2 by 1 inches, pinned at its middle, at (3, 3): each is a constant unless `formulas` names it, and then it is the formula and no value.
fn boxed(id: u32, formulas: &[(&str, &str)]) -> ShapeBuilder {
    let mut shape = ShapeBuilder::new(id);
    for (name, value) in [("PinX", "3"), ("PinY", "3"), ("Width", "2"), ("Height", "1"), ("LocPinX", "1"), ("LocPinY", "0.5"), ("Angle", "0")] {
        shape = match formulas.iter().find(|(named, _)| named == &name) {
            Some((_, formula)) => shape.set(CellAddr::direct(name), Cell::with_formula(formula, None)),
            None => shape.cell(name, value),
        };
    }
    shape
}

fn page_of(shapes: Vec<ShapeBuilder>) -> Document {
    let mut page = PageBuilder::new().cell("PageWidth", "11").cell("PageHeight", "8.5");
    for shape in shapes {
        page = page.shape(shape);
    }
    DocBuilder::new().page(1, page).build()
}

fn placement(s: &mut Session, id: u32) -> Value {
    run(s, "shape.placement", json!({"ids": [id], "page": 1}))["shapes"][0].clone()
}

fn number(v: &Value) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("a number, not {v}"))
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

// ---- Size & Position -----------------------------------------------------------------------------------------------------------------------------

#[test]
fn size_and_position_show_a_pin_a_size_and_an_angle_that_are_a_formula_and_no_saved_value_as_the_formula_works_out() {
    // The pin `=1+1` is 2 (the shape is at 2, 3), `=1+3` for the width is 4 (it is 4 by 1), and `=PI()/2` for the angle is a quarter turn: 90 degrees.
    for (cell, formula, key, want) in [
        ("PinX", "1+1", "x", 2.0),
        ("PinY", "1+4", "y", 5.0),
        ("Width", "1+3", "width", 4.0),
        ("Height", "1+2", "height", 3.0),
        ("Angle", "PI()/2", "angle", 90.0),
    ] {
        let mut s = session_with(page_of(vec![boxed(1, &[(cell, formula)])]));
        let shown = placement(&mut s, 1);
        assert!(near(number(&shown[key]), want), "{cell} = {formula}: Size & Position shows {key} {} for {want}: {shown}", shown[key]);
    }
}

#[test]
fn a_pin_that_is_a_formula_reads_the_cells_it_names_as_the_picture_does() {
    // `=Width*2` is 4 where the width is 2 and holds a value; and a width that is itself a formula and no value is worked out first: `=Width*2` is 8 with `Width` `=1+3`.
    let mut s = session_with(page_of(vec![boxed(1, &[("PinX", "Width*2")]), boxed(2, &[("PinX", "Width*2"), ("Width", "1+3")])]));
    assert!(near(number(&placement(&mut s, 1)["x"]), 4.0));
    assert!(near(number(&placement(&mut s, 2)["x"]), 8.0), "{}", placement(&mut s, 2));
}

#[test]
fn a_formula_that_gives_no_number_leaves_the_cell_as_it_was_read_before() {
    // A formula that fails is not a place: the pin reads 0, as a cell with no value always did, and nothing panics.
    let mut s = session_with(page_of(vec![boxed(1, &[("PinX", "1/0")]), boxed(2, &[("PinX", "NOSUCHFUNCTION(1)")])]));
    for id in [1, 2] {
        let shown = placement(&mut s, id);
        assert!(near(number(&shown["x"]), 0.0), "shape {id}: {shown}");
    }
}

// ---- move ----------------------------------------------------------------------------------------------------------------------------------------

#[test]
fn a_move_takes_a_shape_whose_pin_is_a_formula_and_no_saved_value_from_where_the_formula_puts_it() {
    let mut s = session_with(page_of(vec![boxed(1, &[("PinX", "1+1"), ("PinY", "1+4")])]));
    let moved = run(&mut s, "shape.move", json!({"ids": [1], "dx": 1.0, "dy": 0.5, "page": 1}));
    assert_eq!(moved["moved"], json!(1));
    let shown = placement(&mut s, 1);
    assert!(near(number(&shown["x"]), 3.0) && near(number(&shown["y"]), 5.5), "from (2, 5) by (1, 0.5): {shown}");
    // The cells hold what the move wrote.
    let doc = s.document().unwrap();
    let shape = doc.page(1).unwrap().shapes.get(0).unwrap();
    let held = |name: &str| shape.sheet.cell(name).and_then(|c| c.value.as_ref()).and_then(|v| v.number());
    assert_eq!((held("PinX"), held("PinY")), (Some(3.0), Some(5.5)));
}

#[test]
fn a_move_of_a_group_member_that_is_a_formula_and_no_saved_value_goes_from_where_it_is() {
    let member = ShapeBuilder::new(2)
        .set(CellAddr::direct("PinX"), Cell::with_formula("1+1", None))
        .cell("PinY", "1")
        .cell("Width", "1")
        .cell("Height", "1")
        .cell("LocPinX", "0.5")
        .cell("LocPinY", "0.5");
    let group = boxed(1, &[]).group().child(member);
    let mut s = session_with(page_of(vec![group]));
    run(&mut s, "shape.move", json!({"ids": [2], "dx": 0.25, "dy": 0.0, "page": 1}));
    let doc = s.document().unwrap();
    let at = doc.find_shape(diacraft_doc::Owner::Page(1), 2).unwrap();
    let pin = doc.shape(&at).unwrap().sheet.cell("PinX").and_then(|c| c.value.as_ref()).and_then(|v| v.number());
    assert_eq!(pin, Some(2.25), "the member was at 2 in its group's frame");
}

// ---- snap ----------------------------------------------------------------------------------------------------------------------------------------

/// Only the connection points are targets.
fn only_connection_points(s: &mut Session) {
    run(
        s,
        "snap.set",
        json!({"ruler": false, "grid": false, "alignment_box": false, "intersections": false, "guides": false, "snap_geometry": false,
               "snap_handles": false, "snap_vertices": false, "snap_connection_points": true, "glue_geometry": false, "glue_handles": false,
               "glue_vertices": false, "glue_connection_points": true, "dynamic_grid": false}),
    );
}

/// A 1 by 1 shape at (x, y) with a connection point `(cx, cy)` in its own frame; its pin is `x` as a constant, or the formula `pin_x` and no value.
fn pointed(id: u32, pin_x: Result<f64, &str>, y: f64, point: (f64, f64)) -> ShapeBuilder {
    let shape = ShapeBuilder::new(id)
        .cell("PinY", &y.to_string())
        .cell("Width", "1")
        .cell("Height", "1")
        .cell("LocPinX", "0.5")
        .cell("LocPinY", "0.5")
        .cell("Angle", "0");
    let shape = match pin_x {
        Ok(x) => shape.cell("PinX", &x.to_string()),
        Err(formula) => shape.set(CellAddr::direct("PinX"), Cell::with_formula(formula, None)),
    };
    shape.row_cell("Connection", None, RowKey::Ix(0), "X", &point.0.to_string()).row_cell(
        "Connection",
        None,
        RowKey::Ix(0),
        "Y",
        &point.1.to_string(),
    )
}

fn pin_of(s: &Session, id: u32) -> (f64, f64) {
    let doc = s.document().unwrap();
    let shape = doc.page(1).unwrap().shapes.iter().find(|sh| sh.id == id).unwrap();
    let n = |name: &str| shape.sheet.cell(name).unwrap().value.as_ref().unwrap().number().unwrap();
    (n("PinX"), n("PinY"))
}

#[test]
fn a_connection_point_of_a_shape_pinned_by_a_formula_and_no_saved_value_is_where_the_shape_is() {
    // A is at x = 3 by the formula `=1+2`: its point, the middle of its right side, is at (3.5, 3). B is at 7 and its point is a quarter in from its left and a tenth up, so
    // 0.05 right of and 0.04 above A's when B is moved to a pin of (3.8, 3.44).
    let page = page_of(vec![pointed(1, Err("1+2"), 3.0, (1.0, 0.5)), pointed(2, Ok(7.0), 3.0, (0.25, 0.1))]);
    let mut s = session_with(page);
    only_connection_points(&mut s);
    let moved = run(&mut s, "shape.move", json!({"ids": [2], "dx": 3.8 - 7.0, "dy": 0.44, "snap": true, "page": 1}));
    assert_eq!(moved["snap"]["hits"][0]["target"], json!("connection_point"), "{moved}");
    assert_eq!(moved["snap"]["hits"][0]["shape"], json!(1));
    let (x, y) = pin_of(&s, 2);
    assert!(near(x, 3.75) && near(y, 3.4), "B's point on A's: {x} {y}");
}

#[test]
fn the_own_points_of_a_moved_shape_pinned_by_a_formula_and_no_saved_value_snap_from_where_it_is() {
    // The shape that moves is B, pinned by `=3+4` and no value: its point goes with it from 7.
    let page = page_of(vec![pointed(1, Ok(3.0), 3.0, (1.0, 0.5)), pointed(2, Err("3+4"), 3.0, (0.25, 0.1))]);
    let mut s = session_with(page);
    only_connection_points(&mut s);
    let moved = run(&mut s, "shape.move", json!({"ids": [2], "dx": 3.8 - 7.0, "dy": 0.44, "snap": true, "page": 1}));
    assert_eq!(moved["snap"]["hits"][0]["target"], json!("connection_point"), "{moved}");
    let (x, y) = pin_of(&s, 2);
    assert!(near(x, 3.75) && near(y, 3.4), "B's point on A's: {x} {y}");
}

#[test]
fn snap_at_reads_the_box_of_a_shape_pinned_by_a_formula_as_it_is() {
    // `snap.at` for the box where B's point would be 0.05 right of and 0.04 above A's: -0.05, -0.04 pulls it on.
    let page = page_of(vec![pointed(1, Err("1+2"), 3.0, (1.0, 0.5)), pointed(2, Ok(7.0), 3.0, (0.25, 0.1))]);
    let mut s = session_with(page);
    only_connection_points(&mut s);
    let at = run(&mut s, "snap.at", json!({"page": 1, "ids": [2], "left": 3.3, "bottom": 2.94, "right": 4.3, "top": 3.94}));
    assert!(near(number(&at["dx"]), -0.05) && near(number(&at["dy"]), -0.04), "{at}");
    assert_eq!(at["hits"][0]["target"], json!("connection_point"));
}

// ---- the box and the outline of a target ----------------------------------------------------------------------------------------------------------

/// The settings: the targets are what `on` names, and no other.
fn only(s: &mut Session, on: &[&str]) {
    let mut settings = json!({"ruler": false, "grid": false, "alignment_box": false, "intersections": false, "guides": false, "snap_geometry": false,
                              "snap_handles": false, "snap_vertices": false, "snap_connection_points": false, "dynamic_grid": false});
    for name in on {
        settings[*name] = json!(true);
    }
    run(s, "snap.set", settings);
}

#[test]
fn the_handles_of_a_shape_pinned_by_a_formula_and_no_saved_value_pull_a_moved_shape_to_where_it_is() {
    // A is 2 wide, pinned at x = 3 by `=1+2`: its right side is at 4. B (2 wide, pinned at 7) is moved so that its left side is 0.04 past it, which the handles of A pull right.
    let drive = |pin: Result<f64, &str>| -> f64 {
        let a = match pin {
            Ok(x) => boxed(1, &[]).cell("PinX", &x.to_string()),
            Err(f) => boxed(1, &[("PinX", f)]),
        };
        let mut s = session_with(page_of(vec![a, boxed(2, &[]).cell("PinX", "7")]));
        only(&mut s, &["snap_handles"]);
        run(&mut s, "shape.move", json!({"ids": [2], "dx": 5.0 - 7.0 + 0.04, "dy": 0.0, "snap": true, "page": 1}));
        pin_of(&s, 2).0
    };
    let control = drive(Ok(3.0));
    assert!((control - 5.0).abs() < 1e-6, "the control, with the pin saved: B's side is pulled to A's: {control}");
    let worked = drive(Err("1+2"));
    assert!((worked - 5.0).abs() < 1e-6, "the pin is a formula and no value: {worked}");
}

/// A box 2 by 1 pinned at its middle (3, 3) whose outline is the four sides, its cells `X` and `Y` being the formulas in `formulas` and no value (the box's size is 2 by 1).
fn outlined(id: u32, as_formulas: bool) -> ShapeBuilder {
    let mut shape = boxed(id, &[]);
    for (ix, (kind, x, y)) in
        [("MoveTo", "0", "0"), ("LineTo", "Width", "0"), ("LineTo", "Width", "Height"), ("LineTo", "0", "Height"), ("LineTo", "0", "0")]
            .into_iter()
            .enumerate()
    {
        let ix = RowKey::Ix(u32::try_from(ix + 1).unwrap());
        shape = shape.row_type("Geometry", Some(0), ix.clone(), kind);
        for (name, formula) in [("X", x), ("Y", y)] {
            let addr = CellAddr::in_row("Geometry", Some(0), ix.clone(), name);
            shape = if as_formulas {
                shape.set(addr, Cell::with_formula(formula, None))
            } else {
                shape.set(addr, Cell::constant(diacraft_doc::Value::from_text(&eval(formula))))
            };
        }
    }
    shape
}

/// What the formulas of `outlined` are for a box 2 by 1.
fn eval(formula: &str) -> String {
    match formula {
        "Width" => "2",
        "Height" => "1",
        _ => formula,
    }
    .to_owned()
}

#[test]
fn the_outline_of_a_shape_whose_geometry_cells_are_formulas_and_no_saved_value_is_where_it_is_drawn_for_the_snap() {
    // Only the geometry of shapes is a target. A's outline has its right side at x = 4 (the box's 2 wide, pinned at 3); B is moved so that its left side is 0.04 past it.
    let drive = |formulas: bool| -> f64 {
        let mut s = session_with(page_of(vec![outlined(1, formulas), boxed(2, &[]).cell("PinX", "7")]));
        only(&mut s, &["snap_geometry"]);
        run(&mut s, "shape.move", json!({"ids": [2], "dx": 5.0 - 7.0 + 0.04, "dy": 0.0, "snap": true, "page": 1}));
        pin_of(&s, 2).0
    };
    let control = drive(false);
    assert!((control - 5.0).abs() < 1e-6, "the control, with every cell saved: B's side is pulled onto A's right side: {control}");
    let worked = drive(true);
    assert!((worked - 5.0).abs() < 1e-6, "the rows are formulas and no value: {worked}");
}
