//! The editing commands: drop, move, resize, delete, label and connect shapes, through `Session::execute` exactly as the control
//! channel's `engine.execute` calls it. Numbers are paper inches on pages of the default scale, where they are the cells' own.

use diacraft_doc::{CellAddr, CellEdit, Document, FormulaSlot, Lookup, Origin, Owner, Resolver, SectionKey, SheetRef, Value, WriteOrigin};
use diacraft_engine::draw::{Pose, connection_points, shape_frame};
use diacraft_engine::stencil;
use diacraft_engine::{EngineError, Session};
use serde_json::{Value as Json, json};

use crate::{bad_params_msg, blank_session, check_session, run, snapshot};

const EPS: f64 = 1.0e-9;

pub(crate) fn id_of(result: &Json) -> u32 {
    u32::try_from(result["id"].as_u64().unwrap()).unwrap()
}

pub(crate) fn drop_shape(s: &mut Session, master: &str, x: f64, y: f64) -> u32 {
    id_of(&run(s, "shape.drop", json!({"master": master, "page": 1, "x": x, "y": y})))
}

pub(crate) fn doc(s: &Session) -> &Document {
    s.document().unwrap()
}

/// The `SheetRef` of the shape with this `ID` on page 1, found by its `ID` and not its place.
fn sheet_of(d: &Document, id: u32) -> SheetRef {
    SheetRef::Shape(d.find_shape(Owner::Page(1), id).unwrap_or_else(|| panic!("no shape {id} on page 1")))
}

/// What a cell directly under shape `id` is now, by inheritance.
pub(crate) fn num(d: &Document, id: u32, cell: &str) -> f64 {
    let res = Resolver::new(d);
    let found = res.resolve(&sheet_of(d, id), &CellAddr::direct(cell)).unwrap_or_else(|| panic!("shape {id} has no {cell}"));
    found.value.cell.and_then(|c| c.value.as_ref()).and_then(|v| v.number()).unwrap_or_else(|| panic!("shape {id}: {cell} holds no number"))
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1.0e-6
}

fn assert_near(what: &str, got: f64, want: f64) {
    assert!(near(got, want), "{what}: got {got}, want {want}");
}

/// The resolved `Geometry` rows of shape `id`: (row type, X, Y); the first `Geometry` section.
pub(crate) fn geometry(d: &Document, id: u32) -> Vec<(String, f64, f64)> {
    let res = Resolver::new(d);
    let rows = res.effective_rows(&sheet_of(d, id), &SectionKey::new("Geometry", Some(0))).expect("a Geometry section");
    rows.iter()
        .map(|row| {
            let at = |name: &str| {
                row.cells.iter().find(|(n, _)| n.as_str() == name).and_then(|(_, r)| r.value.cell?.value.as_ref()?.number()).unwrap_or(0.0)
            };
            (row.row_type.as_deref().unwrap_or("").to_owned(), at("X"), at("Y"))
        })
        .collect()
}

/// The route of connector `id` on the page: its rows carried from its own frame to the page.
pub(crate) fn route(d: &Document, id: u32) -> Vec<(f64, f64)> {
    let pose = Pose {
        pin: (num(d, id, "PinX"), num(d, id, "PinY")),
        loc: (num(d, id, "LocPinX"), num(d, id, "LocPinY")),
        angle: num(d, id, "Angle"),
        flip_x: false,
        flip_y: false,
    };
    geometry(d, id).iter().map(|(_, x, y)| pose.up((*x, *y))).collect()
}

fn assert_route(d: &Document, id: u32, want: &[(f64, f64)]) {
    let got = route(d, id);
    let ok = got.len() == want.len() && got.iter().zip(want).all(|(g, w)| near(g.0, w.0) && near(g.1, w.1));
    assert!(ok, "route of {id}: got {got:?}, want {want:?}");
}

/// The `Connect` rows of page 1 as (from shape, from cell, from part, to shape, to cell, to part).
fn connects(d: &Document) -> Vec<(u32, String, u32, u32, String, u32)> {
    d.page(1)
        .unwrap()
        .connects
        .iter()
        .map(|c| {
            (
                c.from_sheet,
                c.from_cell.as_deref().unwrap_or("").to_owned(),
                c.from_part.unwrap_or(0),
                c.to_sheet,
                c.to_cell.as_deref().unwrap_or("").to_owned(),
                c.to_part.unwrap_or(0),
            )
        })
        .collect()
}

fn formula_of(d: &Document, id: u32, cell: &str) -> Option<String> {
    let shape = d.shape(&d.find_shape(Owner::Page(1), id)?)?;
    match shape.sheet.lookup(&CellAddr::direct(cell)) {
        Lookup::Found(c) => c.formula.text().map(str::to_owned),
        _ => None,
    }
}

/// Rectangle 1 at (2, 2) and rectangle 2 at (6, 4), joined by connector 3. The default rectangle is 1.5 by 1.
fn two_joined() -> Session {
    let mut s = blank_session();
    assert_eq!(drop_shape(&mut s, "rectangle", 2.0, 2.0), 1);
    assert_eq!(drop_shape(&mut s, "Rectangle", 6.0, 4.0), 2);
    let r = run(&mut s, "connector.add", json!({"from": {"id": 1}, "to": {"id": 2}}));
    assert_eq!(id_of(&r), 3);
    s
}

// ---- stencil.list and shape.drop ------------------------------------------------------------------------------------------------

#[test]
fn the_stencil_lists_its_twelve_masters_with_sizes() {
    let mut s = diacraft_engine::Session::new();
    let r = run(&mut s, "stencil.list", json!({}));
    let masters = r["stencils"][0]["masters"].as_array().unwrap();
    assert_eq!(r["stencils"][0]["name"], "Basic Shapes");
    assert_eq!(masters.len(), 12);
    assert_eq!(masters[0], json!({"id": "rectangle", "name": "Rectangle", "width": 1.5, "height": 1.0, "connection_points": 4}));
    assert!(masters.iter().all(|m| m["width"].as_f64().unwrap() > 0.0 && m["connection_points"].as_u64().unwrap() >= 4));
}

#[test]
fn every_master_drops_with_its_geometry_size_and_connection_points() {
    for spec in stencil::masters() {
        let mut s = blank_session();
        let r = run(&mut s, "shape.drop", json!({"master": spec.name, "page": 1, "x": 3.0, "y": 4.0}));
        let id = id_of(&r);
        assert_eq!(id, 1, "{}", spec.name);
        let d = doc(&s);
        assert_eq!(d.masters.len(), 1, "{}: the master is copied in", spec.name);
        assert_eq!(r["master"].as_u64().unwrap(), u64::from(d.masters.iter().next().unwrap().id));
        assert_eq!((r["width"].as_f64().unwrap(), r["height"].as_f64().unwrap()), (spec.width, spec.height));
        // What the instance reads is the master's, with its own pin.
        assert_near(spec.name, num(d, id, "Width"), spec.width);
        assert_near(spec.name, num(d, id, "Height"), spec.height);
        assert_near(spec.name, num(d, id, "PinX"), 3.0);
        assert_near(spec.name, num(d, id, "PinY"), 4.0);
        assert_near(spec.name, num(d, id, "LocPinX"), spec.width / 2.0);
        let frame = shape_frame(d, 1, id).unwrap();
        assert_near(spec.name, frame.x, 3.0 - spec.width / 2.0);
        assert_near(spec.name, frame.y, 4.0 - spec.height / 2.0);
        // The geometry resolves through the master, and stays in the box.
        let rows = geometry(d, id);
        assert!(!rows.is_empty(), "{}", spec.name);
        for (_, x, y) in &rows {
            assert!(*x > -EPS && *x < spec.width + EPS && *y > -EPS && *y < spec.height + EPS, "{}: ({x}, {y})", spec.name);
        }
        let points = connection_points(d, 1, id);
        assert!(points.len() >= 4, "{}", spec.name);
        for p in &points {
            assert!(
                p.x >= frame.x - EPS && p.x <= frame.x + frame.width + EPS && p.y >= frame.y - EPS && p.y <= frame.y + frame.height + EPS,
                "{}",
                spec.name
            );
        }
        // The instance holds its pin and nothing it can inherit.
        let shape = d.shape(&d.find_shape(Owner::Page(1), id).unwrap()).unwrap();
        assert_eq!(shape.master, Some(d.masters.iter().next().unwrap().id));
        assert!(
            shape.sheet.cells.keys().map(|k| k.as_str()).eq(["PinX", "PinY"]),
            "{}: {:?}",
            spec.name,
            shape.sheet.cells.keys().collect::<Vec<_>>()
        );
        assert!(shape.sheet.sections.is_empty());
        assert_eq!(check_session(&s), Ok(()));
    }
}

#[test]
fn a_master_is_copied_in_once_and_shapes_get_the_next_ids() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "square", 1.0, 1.0);
    let b = drop_shape(&mut s, "Square", 3.0, 1.0);
    let c = drop_shape(&mut s, "circle", 5.0, 1.0);
    assert_eq!((a, b, c), (1, 2, 3));
    let d = doc(&s);
    assert_eq!(d.masters.len(), 2);
    let master_of = |id: u32| d.shape(&d.find_shape(Owner::Page(1), id).unwrap()).unwrap().master;
    assert_eq!(master_of(1), master_of(2));
    assert_ne!(master_of(1), master_of(3));
}

#[test]
fn a_drop_names_a_master_page_and_place_that_exist() {
    let mut s = blank_session();
    let before = snapshot(&s);
    for params in [
        json!({"master": "heptagon", "page": 1, "x": 1, "y": 1}),
        json!({"master": "rectangle", "page": 99, "x": 1, "y": 1}),
        json!({"master": "rectangle", "page": 1, "x": "1", "y": 1}),
        json!({"master": "rectangle", "page": 1, "x": 1e300, "y": 1}),
        json!({"master": "rectangle", "page": 1, "x": 1}),
        json!({"master": 5, "page": 1, "x": 1, "y": 1}),
    ] {
        let e = s.execute("shape.drop", &params).unwrap_err();
        assert!(matches!(e, EngineError::BadParams { .. }), "{params}: {e}");
    }
    assert_eq!(snapshot(&s), before, "a refused drop changes nothing");
    let mut none = Session::new();
    assert!(matches!(none.execute("shape.drop", &json!({"master": "rectangle", "page": 1, "x": 1, "y": 1})), Err(EngineError::Disabled { .. })));
}

#[test]
fn a_drop_is_one_undo_step_that_brings_the_document_back() {
    let mut s = blank_session();
    let before = doc(&s).clone();
    drop_shape(&mut s, "hexagon", 2.0, 2.0);
    let after = doc(&s).clone();
    assert_eq!(s.history().unwrap().undo_len(), 1);
    assert_eq!(s.history().unwrap().next_undo_label(), Some("Drop shape"));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(doc(&s), &before);
    assert!(doc(&s).masters.is_empty());
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(doc(&s), &after);
}

#[test]
fn the_engine_execute_request_of_the_control_channel_drops_a_shape() {
    // The control channel parses a line like this and calls `Session::execute(command, params)`; the reply is the result as JSON.
    let line = r#"{"id":7,"method":"engine.execute","params":{"command":"shape.drop","params":{"master":"Circle","page":1,"x":1.5,"y":2.5}}}"#;
    let request: Json = serde_json::from_str(line).unwrap();
    let call = &request["params"];
    let mut s = blank_session();
    let reply = s.execute(call["command"].as_str().unwrap(), &call["params"]).unwrap();
    assert_eq!(reply["id"], 1);
    assert_eq!(reply["name"], "Circle.1");
    assert!(serde_json::to_string(&reply).unwrap().contains("\"master\""));
    assert_eq!(s.journal().last().map(|(c, _)| c.as_str()), Some("shape.drop"));
}

#[test]
fn lengths_are_paper_inches_whatever_the_pages_scale() {
    let mut s = blank_session();
    // Two drawing units to a paper inch.
    s.edit("scale", |d| {
        d.write_cell(&SheetRef::Page(1), &CellAddr::direct("DrawingScale"), CellEdit::Value(Value::from_text("2")), WriteOrigin::Drawing)
            .map(|_| ())
            .map_err(|e| diacraft_engine::EngineError::Failed { cmd: "test".into(), msg: e.to_string() })
    })
    .unwrap();
    let r = run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 3, "y": 4}));
    let id = id_of(&r);
    assert_near("PinX in drawing units", num(doc(&s), id, "PinX"), 6.0);
    assert_near("PinY in drawing units", num(doc(&s), id, "PinY"), 8.0);
    assert_near("the size in paper inches", r["width"].as_f64().unwrap(), 0.75);
    run(&mut s, "shape.move", json!({"ids": [id], "dx": 1, "dy": -1}));
    assert_near("moved PinX", num(doc(&s), id, "PinX"), 8.0);
    assert_near("moved PinY", num(doc(&s), id, "PinY"), 6.0);
}

// ---- connectors -----------------------------------------------------------------------------------------------------------------

#[test]
fn a_connector_is_glued_at_the_nearest_points_with_a_z_route() {
    let s = two_joined();
    let d = doc(&s);
    // Rectangle 1's right edge (point 3) faces rectangle 2's left edge (point 1).
    assert_eq!(
        connects(d),
        [(3, "BeginX".to_owned(), 9, 1, "Connections.X4".to_owned(), 103), (3, "EndX".to_owned(), 12, 2, "Connections.X2".to_owned(), 101)]
    );
    assert_eq!(formula_of(d, 3, "BeginX").as_deref(), Some("PAR(PNT(Sheet.1!Connections.X4,Sheet.1!Connections.Y4))"));
    assert_eq!(formula_of(d, 3, "EndY").as_deref(), Some("PAR(PNT(Sheet.2!Connections.X2,Sheet.2!Connections.Y2))"));
    // The ends are where the points are; the creation formulas give the rest.
    assert_near("BeginX", num(d, 3, "BeginX"), 2.75);
    assert_near("BeginY", num(d, 3, "BeginY"), 2.0);
    assert_near("EndX", num(d, 3, "EndX"), 5.25);
    assert_near("EndY", num(d, 3, "EndY"), 4.0);
    assert_near("Width", num(d, 3, "Width"), (2.5f64 * 2.5 + 4.0).sqrt());
    assert_near("Angle", num(d, 3, "Angle"), 2.0f64.atan2(2.5));
    assert_near("PinX", num(d, 3, "PinX"), 4.0);
    assert_near("PinY", num(d, 3, "PinY"), 3.0);
    assert_near("LocPinX", num(d, 3, "LocPinX"), num(d, 3, "Width") / 2.0);
    // The route, in the connector's own frame: it starts at (0, LocPinY) and ends at (Width, LocPinY).
    let rows = geometry(d, 3);
    assert_eq!(rows.iter().map(|r| r.0.as_str()).collect::<Vec<_>>(), ["MoveTo", "LineTo", "LineTo", "LineTo"]);
    assert_near("begin x", rows[0].1, 0.0);
    assert_near("begin y", rows[0].2, num(d, 3, "LocPinY"));
    assert_near("end x", rows[3].1, num(d, 3, "Width"));
    assert_near("end y", rows[3].2, num(d, 3, "LocPinY"));
    // And on the page it is a Z with its middle leg halfway between the shapes.
    assert_route(d, 3, &[(2.75, 2.0), (4.0, 2.0), (4.0, 4.0), (5.25, 4.0)]);
    // A solid arrowhead at the end, no fill.
    assert_eq!(num(d, 3, "EndArrow"), 4.0);
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_connector_names_its_connection_points_or_ends_free() {
    let mut s = blank_session();
    drop_shape(&mut s, "rectangle", 2.0, 2.0);
    drop_shape(&mut s, "rectangle", 6.0, 2.0);
    let r = run(&mut s, "connector.add", json!({"from": {"id": 1, "point": 0}, "to": {"id": 2, "point": 0}}));
    assert_eq!((r["begin"].clone(), r["end"].clone()), (json!({"id": 1, "point": 0}), json!({"id": 2, "point": 0})));
    // Both tops: out of both by the standoff and across (a C).
    assert_route(doc(&s), id_of(&r), &[(2.0, 2.5), (2.0, 2.75), (6.0, 2.75), (6.0, 2.5)]);
    let free = run(&mut s, "connector.add", json!({"from": {"x": 1, "y": 6}, "to": {"x": 5, "y": 6}}));
    let free_id = id_of(&free);
    assert_eq!((free["begin"].clone(), free["end"].clone()), (Json::Null, Json::Null));
    assert_eq!(formula_of(doc(&s), free_id, "BeginX"), None, "a free end holds a plain value");
    assert_route(doc(&s), free_id, &[(1.0, 6.0), (5.0, 6.0)]);
    assert_eq!(connects(doc(&s)).len(), 2, "a free connector has no Connect row");
    // One end glued, one free: the nearest point to the free end.
    let half = run(&mut s, "connector.add", json!({"from": {"id": 2}, "to": {"x": 9, "y": 2}}));
    assert_eq!(half["begin"], json!({"id": 2, "point": 3}));
    assert_eq!(half["end"], Json::Null);
    assert_route(doc(&s), id_of(&half), &[(6.75, 2.0), (9.0, 2.0)]);
}

#[test]
fn a_connector_between_shapes_that_do_not_exist_or_points_that_do_not_is_refused() {
    let mut s = two_joined();
    let before = snapshot(&s);
    for params in [
        json!({"from": {"id": 1}, "to": {"id": 99}}),
        json!({"from": {"id": 1, "point": 9}, "to": {"id": 2}}),
        json!({"from": {"id": 1, "x": 1}, "to": {"id": 2}}),
        json!({"from": {"id": 1}, "to": {"y": 1}}),
        json!({"from": {"id": 1}, "to": {"id": 2, "colour": "red"}}),
        json!({"from": {"id": 1}}),
        json!({"from": {"id": 1}, "to": {"id": 2}, "page": 5}),
    ] {
        let e = s.execute("connector.add", &params).unwrap_err();
        assert!(matches!(e, EngineError::BadParams { .. }), "{params}: {e}");
    }
    assert_eq!(snapshot(&s), before);
}

#[test]
fn moving_a_shape_moves_the_glued_end_and_reroutes() {
    let mut s = two_joined();
    let steps = s.history().unwrap().undo_len();
    let r = run(&mut s, "shape.move", json!({"ids": [2], "dx": 1.0, "dy": -1.0}));
    assert_eq!(r["moved"], 1);
    assert_eq!(s.history().unwrap().undo_len(), steps + 1, "one undo step");
    let d = doc(&s);
    assert_near("PinX", num(d, 2, "PinX"), 7.0);
    assert_near("PinY", num(d, 2, "PinY"), 3.0);
    assert_near("EndX", num(d, 3, "EndX"), 6.25);
    assert_near("EndY", num(d, 3, "EndY"), 3.0);
    assert_near("Width", num(d, 3, "Width"), (3.5f64 * 3.5 + 1.0).sqrt());
    assert_near("Angle", num(d, 3, "Angle"), 1.0f64.atan2(3.5));
    assert_near("PinX of the connector", num(d, 3, "PinX"), 4.5);
    assert_route(d, 3, &[(2.75, 2.0), (4.5, 2.0), (4.5, 3.0), (6.25, 3.0)]);
    // The begin is still on rectangle 1, and the Connect rows did not change.
    assert_near("BeginX", num(d, 3, "BeginX"), 2.75);
    assert_eq!(connects(d).len(), 2);
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn moving_both_shapes_and_the_connector_moves_everything_by_the_same_amount() {
    let mut s = two_joined();
    let before = route(doc(&s), 3);
    run(&mut s, "shape.move", json!({"ids": [1, 2, 3], "dx": 2.0, "dy": 1.0}));
    let after = route(doc(&s), 3);
    assert_eq!(before.len(), after.len());
    for (b, a) in before.iter().zip(&after) {
        assert!(near(a.0, b.0 + 2.0) && near(a.1, b.1 + 1.0), "{b:?} -> {a:?}");
    }
    assert_near("PinX of 1", num(doc(&s), 1, "PinX"), 4.0);
}

#[test]
fn a_connector_with_free_ends_moves_by_its_ends_and_a_glued_one_stays_with_its_shapes() {
    let mut s = blank_session();
    let free = id_of(&run(&mut s, "connector.add", json!({"from": {"x": 1, "y": 1}, "to": {"x": 3, "y": 1}})));
    run(&mut s, "shape.move", json!({"ids": [free], "dx": 0.0, "dy": 2.0}));
    assert_route(doc(&s), free, &[(1.0, 3.0), (3.0, 3.0)]);
    assert_near("EndY", num(doc(&s), free, "EndY"), 3.0);

    let mut s = two_joined();
    let before = route(doc(&s), 3);
    run(&mut s, "shape.move", json!({"ids": [3], "dx": 5.0, "dy": 5.0}));
    assert_eq!(route(doc(&s), 3), before, "both ends are glued, so the connector does not leave its shapes");
    assert!(formula_of(doc(&s), 3, "BeginX").is_some());
}

#[test]
fn resizing_a_shape_scales_its_geometry_and_the_glued_connector_follows() {
    let mut s = two_joined();
    let steps = s.history().unwrap().undo_len();
    run(&mut s, "shape.resize", json!({"id": 1, "x": 1.0, "y": 1.0, "width": 3.0, "height": 2.0}));
    assert_eq!(s.history().unwrap().undo_len(), steps + 1);
    let d = doc(&s);
    assert_near("Width", num(d, 1, "Width"), 3.0);
    assert_near("Height", num(d, 1, "Height"), 2.0);
    assert_near("PinX", num(d, 1, "PinX"), 2.5);
    assert_near("PinY", num(d, 1, "PinY"), 2.0);
    assert_near("LocPinX", num(d, 1, "LocPinX"), 1.5);
    let frame = shape_frame(d, 1, 1).unwrap();
    assert_eq!((frame.x, frame.y, frame.width, frame.height), (1.0, 1.0, 3.0, 2.0));
    // The vertices are shares of the new box: the master's own are untouched.
    let rows = geometry(d, 1);
    let corners: Vec<(f64, f64)> = rows.iter().map(|r| (r.1, r.2)).collect();
    assert_eq!(corners.len(), 5);
    for (got, want) in corners.iter().zip([(0.0, 0.0), (3.0, 0.0), (3.0, 2.0), (0.0, 2.0), (0.0, 0.0)]) {
        assert!(near(got.0, want.0) && near(got.1, want.1), "{corners:?}");
    }
    let master = d.master(d.shape(&d.find_shape(Owner::Page(1), 1).unwrap()).unwrap().master.unwrap()).unwrap();
    let master_width = master.shapes.iter().next().map(|m| m.sheet.cell("Width").and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned()));
    assert_eq!(master_width, Some(Some("1.5".to_owned())), "the master is not edited");
    // What the instance inherits and now gives another result is held as `Inh` with its result.
    let shape = d.shape(&d.find_shape(Owner::Page(1), 1).unwrap()).unwrap();
    let held = match shape.sheet.lookup(&CellAddr::direct("LocPinX")) {
        Lookup::Found(c) => c.clone(),
        other => panic!("{other:?}"),
    };
    assert_eq!(held.formula, FormulaSlot::Inherit);
    assert_eq!(held.origin(), Origin::InheritedWithCachedValue);
    // The connection point moved, and the connector with it.
    let right = connection_points(d, 1, 1)[3];
    assert_eq!((right.x, right.y), (4.0, 2.0));
    assert_near("BeginX", num(d, 3, "BeginX"), 4.0);
    assert_near("BeginY", num(d, 3, "BeginY"), 2.0);
    assert_route(d, 3, &[(4.0, 2.0), (4.625, 2.0), (4.625, 4.0), (5.25, 4.0)]);
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_resize_that_cannot_be_made_is_refused_and_changes_nothing() {
    let mut s = two_joined();
    let before = snapshot(&s);
    for params in [
        json!({"id": 3, "x": 1, "y": 1, "width": 3, "height": 2}),
        json!({"id": 1, "x": 1, "y": 1, "width": 0, "height": 2}),
        json!({"id": 1, "x": 1, "y": 1, "width": -3, "height": 2}),
        json!({"id": 1, "x": 1, "y": 1, "width": 1e300, "height": 2}),
        json!({"id": 99, "x": 1, "y": 1, "width": 3, "height": 2}),
        json!({"id": 1, "x": 1, "y": 1, "width": 3}),
    ] {
        assert!(s.execute("shape.resize", &params).is_err(), "{params}");
        assert_eq!(snapshot(&s), before, "{params}");
    }
}

/// With `connectors: false` the connector stays with its end where it was (by default it goes with the shape, `delete_connectors.rs`).
#[test]
fn deleting_a_shape_with_connectors_false_frees_the_end_of_the_connector_glued_to_it() {
    let mut s = two_joined();
    let steps = s.history().unwrap().undo_len();
    let r = run(&mut s, "shape.delete", json!({"ids": [2], "connectors": false}));
    assert_eq!(r["deleted"], 1);
    assert_eq!(s.history().unwrap().undo_len(), steps + 1);
    let d = doc(&s);
    assert!(d.find_shape(Owner::Page(1), 2).is_none());
    assert_eq!(d.masters.len(), 1, "the master stays");
    // The connector stays, with its end where it was and no formula.
    assert_near("EndX", num(d, 3, "EndX"), 5.25);
    assert_near("EndY", num(d, 3, "EndY"), 4.0);
    assert_eq!(formula_of(d, 3, "EndX"), None);
    assert_eq!(connects(d), [(3, "BeginX".to_owned(), 9, 1, "Connections.X4".to_owned(), 103)]);
    assert_route(d, 3, &[(2.75, 2.0), (4.0, 2.0), (4.0, 4.0), (5.25, 4.0)]);
    // The begin is still glued: moving rectangle 1 still moves it.
    run(&mut s, "shape.move", json!({"ids": [1], "dx": 0.0, "dy": 1.0}));
    assert_near("BeginY", num(doc(&s), 3, "BeginY"), 3.0);
    assert_near("EndY stays", num(doc(&s), 3, "EndY"), 4.0);
    // Undo brings the shape, the glue and the formula back.
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    assert!(doc(&s).find_shape(Owner::Page(1), 2).is_some());
    assert_eq!(connects(doc(&s)).len(), 2);
    assert!(formula_of(doc(&s), 3, "EndX").is_some());
}

#[test]
fn deleting_the_connector_takes_its_connect_rows_and_deleting_everything_empties_the_page() {
    let mut s = two_joined();
    run(&mut s, "shape.delete", json!({"ids": [3]}));
    assert!(connects(doc(&s)).is_empty());
    assert!(doc(&s).find_shape(Owner::Page(1), 3).is_none());
    assert!(doc(&s).find_shape(Owner::Page(1), 1).is_some());

    let mut s = two_joined();
    run(&mut s, "shape.delete", json!({"ids": [1, 2, 3, 3]}));
    assert!(doc(&s).page(1).unwrap().shapes.is_empty());
    assert!(connects(doc(&s)).is_empty());
    // Both shapes deleted and the connector kept (`connectors: false`): both ends are free where they were.
    let mut s = two_joined();
    run(&mut s, "shape.delete", json!({"ids": [1, 2], "connectors": false}));
    assert!(connects(doc(&s)).is_empty());
    assert_near("BeginX", num(doc(&s), 3, "BeginX"), 2.75);
    assert_near("EndX", num(doc(&s), 3, "EndX"), 5.25);
    assert_route(doc(&s), 3, &[(2.75, 2.0), (4.0, 2.0), (4.0, 4.0), (5.25, 4.0)]);
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn deleting_what_is_not_there_is_refused() {
    let mut s = two_joined();
    let before = snapshot(&s);
    for params in [json!({"ids": [99]}), json!({"ids": [1, 99]}), json!({"ids": []}), json!({"ids": "1"}), json!({"ids": [-1]}), json!({})] {
        assert!(matches!(s.execute("shape.delete", &params), Err(EngineError::BadParams { .. })), "{params}");
    }
    let many: Vec<u32> = (0..5_000).collect();
    assert!(matches!(s.execute("shape.delete", &json!({"ids": many})), Err(EngineError::BadParams { .. })));
    assert_eq!(snapshot(&s), before);
}

#[test]
fn a_shapes_text_is_replaced_and_taken_away_and_undone() {
    let mut s = two_joined();
    let before = doc(&s).clone();
    run(&mut s, "shape.set_text", json!({"id": 1, "text": "Start\nhere"}));
    let text_of = |s: &Session| {
        let d = doc(s);
        d.shape(&d.find_shape(Owner::Page(1), 1).unwrap()).unwrap().text.clone()
    };
    let text = text_of(&s).unwrap();
    assert_eq!(text.parts, [diacraft_doc::TextPart::Chars("Start\nhere".into())]);
    run(&mut s, "shape.set_text", json!({"id": 1, "text": "Second"}));
    assert_eq!(text_of(&s).unwrap().parts, [diacraft_doc::TextPart::Chars("Second".into())]);
    run(&mut s, "shape.set_text", json!({"id": 1, "text": ""}));
    assert!(text_of(&s).is_none());
    for _ in 0..3 {
        run(&mut s, "edit.undo", json!({}));
    }
    assert_eq!(doc(&s), &before);
    // A connector takes text too.
    run(&mut s, "shape.set_text", json!({"id": 3, "text": "yes"}));
    assert!(doc(&s).shape(&doc(&s).find_shape(Owner::Page(1), 3).unwrap()).unwrap().text.is_some());
}

#[test]
fn hostile_text_is_refused() {
    let mut s = two_joined();
    let before = snapshot(&s);
    for text in ["bell\u{7}", "escape\u{1b}[2J", "nul\0", &"x".repeat(100_001)] {
        let e = s.execute("shape.set_text", &json!({"id": 1, "text": text})).unwrap_err();
        assert!(matches!(e, EngineError::BadParams { .. }));
        assert!(!bad_params_msg(&e).chars().any(char::is_control));
    }
    assert!(s.execute("shape.set_text", &json!({"id": 99, "text": "x"})).is_err());
    assert_eq!(snapshot(&s), before);
    run(&mut s, "shape.set_text", json!({"id": 1, "text": "tab\there"}));
}

#[test]
fn a_guarded_pin_is_not_moved_and_nothing_changes() {
    let mut s = two_joined();
    s.edit("guard", |d| {
        let sheet = sheet_of(d, 1);
        d.write_cell(
            &sheet,
            &CellAddr::direct("PinX"),
            CellEdit::Cell(diacraft_doc::Cell::with_formula("GUARD(2)", Some(Value::from_text("2")))),
            WriteOrigin::Automation,
        )
        .map(|_| ())
        .map_err(|e| diacraft_engine::EngineError::Failed { cmd: "test".into(), msg: e.to_string() })
    })
    .unwrap();
    let before = snapshot(&s);
    let e = s.execute("shape.move", &json!({"ids": [1], "dx": 1, "dy": 0})).unwrap_err();
    assert!(matches!(e, EngineError::Failed { .. }), "{e}");
    assert_eq!(snapshot(&s), before);
}

#[test]
fn reconnecting_an_end_glues_it_to_another_point_a_shape_or_a_place() {
    let mut s = two_joined();
    let c = id_of(&run(&mut s, "shape.drop", json!({"master": "ellipse", "page": 1, "x": 6, "y": 0})));
    assert_eq!(c, 4);
    // The end moves to the ellipse's left point (1).
    let r = run(&mut s, "connector.reconnect", json!({"id": 3, "end": "end", "target": {"id": 4, "point": 1}}));
    assert_eq!(r["end"], json!({"id": 4, "point": 1}));
    assert_eq!(r["begin"], json!({"id": 1, "point": 3}));
    let d = doc(&s);
    assert_eq!(
        connects(d),
        [(3, "BeginX".to_owned(), 9, 1, "Connections.X4".to_owned(), 103), (3, "EndX".to_owned(), 12, 4, "Connections.X2".to_owned(), 101)]
    );
    assert_near("EndX", num(d, 3, "EndX"), 5.25);
    assert_near("EndY", num(d, 3, "EndY"), 0.0);
    assert_route(d, 3, &[(2.75, 2.0), (4.0, 2.0), (4.0, 0.0), (5.25, 0.0)]);
    // To a place: the end is a plain point, with no Connect row.
    run(&mut s, "connector.reconnect", json!({"id": 3, "end": "end", "target": {"x": 8, "y": 1}}));
    let d = doc(&s);
    assert_eq!(connects(d).len(), 1);
    assert_eq!(formula_of(d, 3, "EndX"), None);
    assert_near("EndX", num(d, 3, "EndX"), 8.0);
    assert_near("EndY", num(d, 3, "EndY"), 1.0);
    // The begin to rectangle 2, the point nearest the end.
    let r = run(&mut s, "connector.reconnect", json!({"id": 3, "end": "begin", "target": {"id": 2}}));
    // Rectangle 2 is at (6, 4); its bottom point (6, 3.5) is the nearest to (8, 1).
    assert_eq!(r["begin"], json!({"id": 2, "point": 2}));
    assert_eq!(connects(doc(&s)).len(), 1);
    assert_near("BeginX", num(doc(&s), 3, "BeginX"), 6.0);
    assert_near("BeginY", num(doc(&s), 3, "BeginY"), 3.5);
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn reconnecting_something_that_is_not_a_connector_or_an_end_that_is_not_one_is_refused() {
    let mut s = two_joined();
    let before = snapshot(&s);
    for params in [
        json!({"id": 1, "end": "end", "target": {"id": 2}}),
        json!({"id": 3, "end": "middle", "target": {"id": 2}}),
        json!({"id": 3, "end": "end", "target": {"id": 99}}),
        json!({"id": 3, "end": "end", "target": {"id": 2, "point": 17}}),
        json!({"id": 3, "end": "end"}),
        json!({"id": 99, "end": "end", "target": {"id": 2}}),
    ] {
        assert!(matches!(s.execute("connector.reconnect", &params), Err(EngineError::BadParams { .. })), "{params}");
    }
    assert_eq!(snapshot(&s), before);
}

#[test]
fn each_editing_command_is_one_undo_step_and_redo_restores_it() {
    let commands: Vec<(&str, Json)> = vec![
        ("shape.drop", json!({"master": "octagon", "page": 1, "x": 9, "y": 6})),
        ("shape.move", json!({"ids": [1, 2], "dx": 0.5, "dy": 0.5})),
        ("shape.resize", json!({"id": 2, "x": 5, "y": 3, "width": 2, "height": 2})),
        ("shape.set_text", json!({"id": 1, "text": "A"})),
        ("connector.add", json!({"from": {"id": 2, "point": 0}, "to": {"id": 1, "point": 2}})),
        ("connector.reconnect", json!({"id": 3, "end": "begin", "target": {"x": 0.5, "y": 0.5}})),
        ("shape.delete", json!({"ids": [1]})),
    ];
    for (id, params) in commands {
        let mut s = two_joined();
        let (before, steps) = (doc(&s).clone(), s.history().unwrap().undo_len());
        let journaled = |s: &Session| s.journal().iter().filter(|(c, _)| c == id).count();
        let entries = journaled(&s);
        run(&mut s, id, params.clone());
        let after = doc(&s).clone();
        assert_ne!(after, before, "{id}");
        assert_eq!(s.history().unwrap().undo_len(), steps + 1, "{id}: one step");
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(doc(&s), &before, "{id}: undo");
        run(&mut s, "edit.redo", json!({}));
        assert_eq!(doc(&s), &after, "{id}: redo");
        assert_eq!(journaled(&s), entries + 1, "{id}: journaled once");
    }
}

#[test]
fn a_batch_of_drawing_commands_is_one_undo_step() {
    let mut s = blank_session();
    let before = doc(&s).clone();
    run(
        &mut s,
        "command.batch",
        json!({"label": "Draw", "commands": [
            {"command": "shape.drop", "params": {"master": "rectangle", "page": 1, "x": 2, "y": 2}},
            {"command": "shape.drop", "params": {"master": "circle", "page": 1, "x": 6, "y": 2}},
            {"command": "connector.add", "params": {"from": {"id": 1}, "to": {"id": 2}}},
            {"command": "shape.set_text", "params": {"id": 1, "text": "Start"}},
        ]}),
    );
    assert_eq!(s.history().unwrap().undo_len(), 1);
    assert!(connects(doc(&s)).len() == 2);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(doc(&s), &before);
}

#[test]
fn a_page_other_than_the_first_is_named_or_found_by_its_shapes() {
    let mut s = blank_session();
    run(&mut s, "page.add", json!({"name": "Second"}));
    let r = run(&mut s, "shape.drop", json!({"master": "square", "page": 2, "x": 1, "y": 1}));
    assert_eq!(id_of(&r), 1);
    // Shape 1 is on page 2 only: no `page` finds it there.
    run(&mut s, "shape.move", json!({"ids": [1], "dx": 1, "dy": 0}));
    let d = doc(&s);
    let sheet = SheetRef::Shape(d.find_shape(Owner::Page(2), 1).unwrap());
    let pin = Resolver::new(d).resolve(&sheet, &CellAddr::direct("PinX")).unwrap().value.cell.unwrap().value.as_ref().unwrap().number().unwrap();
    assert_near("PinX", pin, 2.0);
    assert!(d.page(1).unwrap().shapes.is_empty());
}
