//! Straight and curved connectors:
//! `connector.style`.

use diacraft_doc::{CellAddr, Document, Owner, Resolver, SectionKey, SheetRef};
use diacraft_geom::kurbo::{PathEl, Size, flatten};
use diacraft_geom::{GeomRow, Nurbs, build_paths};
use serde_json::json;

use crate::edit::{doc, drop_shape, geometry, id_of, route};
use crate::{Snapshot, blank_session, check_session, run, snapshot};

/// A snapshot as the document and its history have it: a call that changes nothing still has its place in the journal.
fn quiet(s: &diacraft_engine::Session) -> Snapshot {
    Snapshot { journal: Vec::new(), ..snapshot(s) }
}

type P = (f64, f64);

/// The number a connector's cell holds, `None` where nothing sets it.
pub(crate) fn cell(d: &Document, id: u32, name: &str) -> Option<f64> {
    let sheet = SheetRef::Shape(d.find_shape(Owner::Page(1), id).unwrap());
    Resolver::new(d).resolve(&sheet, &CellAddr::direct(name)).and_then(|f| f.value.cell?.value.as_ref()?.number())
}

pub(crate) fn page_cell(d: &Document, name: &str) -> Option<f64> {
    Resolver::new(d).resolve(&SheetRef::Page(1), &CellAddr::direct(name)).and_then(|f| f.value.cell?.value.as_ref()?.number())
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1.0e-6
}

fn near_point(a: P, b: P) -> bool {
    near(a.0, b.0) && near(a.1, b.1)
}

/// Box A at (3, 4) and box B at (9, 6), joined from A's right point to B's left point: a Z of three legs.
pub(crate) fn z_pair() -> (diacraft_engine::Session, u32, u32, u32) {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    let b = drop_shape(&mut s, "rectangle", 9.0, 6.0);
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"id": a, "point": 3}, "to": {"id": b, "point": 1}})));
    (s, a, b, c)
}

fn kinds(d: &Document, id: u32) -> Vec<String> {
    geometry(d, id).into_iter().map(|r| r.0).collect()
}

#[test]
fn a_new_connector_is_right_angle_and_a_page_with_no_style_is_right_angle() {
    let (s, _, _, c) = z_pair();
    assert_eq!(kinds(doc(&s), c), ["MoveTo", "LineTo", "LineTo", "LineTo"], "a Z");
    assert_eq!(cell(doc(&s), c, "ShapeRouteStyle"), None, "a connector writes no style cell until one is chosen");
}

#[test]
fn straight_lines_join_the_two_ends_with_one_leg_and_keep_it_when_a_shape_moves() {
    let (mut s, a, b, c) = z_pair();
    let r = run(&mut s, "connector.style", json!({"style": "straight", "ids": [c]}));
    assert_eq!(r, json!({"page": 1, "style": "straight", "styled": 1, "default_changed": false}));
    let d = doc(&s);
    assert_eq!((cell(d, c, "ShapeRouteStyle"), cell(d, c, "ConLineRouteExt")), (Some(2.0), Some(1.0)), "Straight lines");
    assert_eq!(kinds(d, c), ["MoveTo", "LineTo"]);
    assert!(near_point(route(d, c)[0], (3.75, 4.0)) && near_point(route(d, c)[1], (8.25, 6.0)), "{:?}", route(d, c));
    // The shapes move: the connector stays one straight leg between the points it is glued to.
    for (id, dx, dy) in [(b, 1.0, -3.0), (a, -2.0, 1.5), (b, -7.0, 0.5)] {
        run(&mut s, "shape.move", json!({"ids": [id], "dx": dx, "dy": dy}));
        let d = doc(&s);
        let r = route(d, c);
        assert_eq!(r.len(), 2, "moved {id} by ({dx}, {dy}): {r:?}");
        assert!(near_point(r[0], (cell(d, c, "BeginX").unwrap(), cell(d, c, "BeginY").unwrap())), "{r:?}");
        assert!(near_point(r[1], (cell(d, c, "EndX").unwrap(), cell(d, c, "EndY").unwrap())), "{r:?}");
    }
    assert_eq!(check_session(&s), Ok(()));
}

/// The stroke of the connector's rows as the geometry crate draws them: the points of the flattened path.
fn drawn(d: &Document, id: u32) -> Vec<P> {
    let sheet = SheetRef::Shape(d.find_shape(Owner::Page(1), id).unwrap());
    let res = Resolver::new(d);
    let rows = res.effective_rows(&sheet, &SectionKey::new("Geometry", Some(0))).unwrap();
    let mut geom: Vec<GeomRow> = Vec::new();
    for row in &rows {
        let num =
            |name: &str| row.cells.iter().find(|(n, _)| n.as_str() == name).and_then(|(_, r)| r.value.cell?.value.as_ref()?.number()).unwrap_or(0.0);
        let text = |name: &str| {
            row.cells.iter().find(|(n, _)| n.as_str() == name).and_then(|(_, r)| r.value.cell?.value.as_ref().map(|v| v.text().to_owned()))
        };
        geom.push(match row.row_type.as_deref().unwrap() {
            "MoveTo" => GeomRow::MoveTo { x: num("X"), y: num("Y") },
            "LineTo" => GeomRow::LineTo { x: num("X"), y: num("Y") },
            "NURBSTo" => GeomRow::NurbsTo {
                x: num("X"),
                y: num("Y"),
                second_last_knot: num("A"),
                last_weight: num("B"),
                first_knot: num("C"),
                first_weight: num("D"),
                nurbs: Some(Nurbs::from_text(&text("E").unwrap()).unwrap()),
            },
            other => panic!("a route of {other} rows"),
        });
    }
    let (width, height) = (cell(d, id, "Width").unwrap(), cell(d, id, "Height").unwrap());
    let built = build_paths(&geom, Size { width, height }).unwrap();
    let pose = diacraft_engine::draw::Pose {
        pin: (cell(d, id, "PinX").unwrap(), cell(d, id, "PinY").unwrap()),
        loc: (cell(d, id, "LocPinX").unwrap(), cell(d, id, "LocPinY").unwrap()),
        angle: cell(d, id, "Angle").unwrap(),
        flip_x: false,
        flip_y: false,
    };
    let mut points = Vec::new();
    flatten(built.stroke.elements().iter().copied(), 0.001, |el| match el {
        PathEl::MoveTo(p) | PathEl::LineTo(p) => points.push(pose.up((p.x, p.y))),
        _ => {}
    });
    points
}

fn distance_to(points: &[P], target: P) -> f64 {
    points
        .windows(2)
        .map(|w| {
            let (a, b) = (w[0], w[1]);
            let ab = (b.0 - a.0, b.1 - a.1);
            let t = (((target.0 - a.0) * ab.0 + (target.1 - a.1) * ab.1) / (ab.0 * ab.0 + ab.1 * ab.1).max(1e-18)).clamp(0.0, 1.0);
            ((a.0 + ab.0 * t - target.0).powi(2) + (a.1 + ab.1 * t - target.1).powi(2)).sqrt()
        })
        .fold(f64::INFINITY, f64::min)
}

#[test]
fn curved_lines_are_a_spline_from_the_begin_to_the_end_that_leaves_and_arrives_along_the_legs() {
    let (mut s, _, _, c) = z_pair();
    run(&mut s, "connector.style", json!({"style": "curved", "ids": [c]}));
    let d = doc(&s);
    assert_eq!((cell(d, c, "ShapeRouteStyle"), cell(d, c, "ConLineRouteExt")), (Some(1.0), Some(2.0)), "Curved lines");
    assert_eq!(kinds(d, c), ["MoveTo", "NURBSTo"], "A curve is NURBSTo rows");
    // The Z through (3.75, 4), (6, 4), (6, 6), (8.25, 6): the spline starts and ends there, is level at both ends and passes the middle of the cross leg.
    let curve = drawn(d, c);
    assert!(near_point(curve[0], (3.75, 4.0)) && near_point(*curve.last().unwrap(), (8.25, 6.0)), "{:?} .. {:?}", curve[0], curve.last());
    assert!(distance_to(&curve, (6.0, 5.0)) < 0.01, "the curve passes the middle of the cross leg");
    let (first, last) = (curve[1], curve[curve.len() - 2]);
    assert!((first.1 - 4.0).abs() < 0.01 && first.0 > 3.75, "leaves along the first leg: {first:?}");
    assert!((last.1 - 6.0).abs() < 0.01 && last.0 < 8.25, "arrives along the last leg: {last:?}");
    assert!(curve.iter().all(|p| p.0 >= 3.75 - 1e-6 && p.0 <= 8.25 + 1e-6 && p.1 >= 4.0 - 1e-6 && p.1 <= 6.0 + 1e-6), "inside the box of the route");
    // It follows a shape that moves, and is a curve still.
    run(&mut s, "shape.move", json!({"ids": [2], "dx": 0.0, "dy": -2.0}));
    let d = doc(&s);
    assert_eq!(kinds(d, c), ["MoveTo", "LineTo"], "ends level with each other are a line");
    run(&mut s, "shape.move", json!({"ids": [2], "dx": 1.0, "dy": 3.0}));
    assert_eq!(kinds(doc(&s), c), ["MoveTo", "NURBSTo"]);
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_curve_of_a_route_with_a_detour_is_still_a_clamped_curve_through_its_ends() {
    let mut s = blank_session();
    let a = drop_shape(&mut s, "rectangle", 3.0, 4.0);
    let b = drop_shape(&mut s, "rectangle", 9.0, 4.0);
    // A's right point to B's right point: a hook round B, five legs.
    let c = id_of(&run(&mut s, "connector.add", json!({"from": {"id": a, "point": 3}, "to": {"id": b, "point": 3}})));
    run(&mut s, "connector.style", json!({"style": "curved", "ids": [c]}));
    let d = doc(&s);
    let curve = drawn(d, c);
    assert!(near_point(curve[0], (3.75, 4.0)) && near_point(*curve.last().unwrap(), (9.75, 4.0)), "{:?} .. {:?}", curve[0], curve.last());
    // It rounds the corners of the hook round B, and does not cut into B (its top is 4.5).
    let corners = diacraft_engine::draw::shape_corners(d, 1, b).unwrap();
    let (x0, x1) = (corners.iter().map(|p| p.0).fold(f64::INFINITY, f64::min), corners.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max));
    let (y0, y1) = (corners.iter().map(|p| p.1).fold(f64::INFINITY, f64::min), corners.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max));
    assert!(curve.iter().all(|p| !(p.0 > x0 + 1e-3 && p.0 < x1 - 1e-3 && p.1 > y0 + 1e-3 && p.1 < y1 - 1e-3)), "the curve is outside B: {curve:?}");
    assert!(curve.iter().any(|p| p.1 > 4.5), "and goes over it");
}

#[test]
fn right_angle_takes_a_curved_or_straight_connector_back_to_legs() {
    let (mut s, _, _, c) = z_pair();
    run(&mut s, "connector.style", json!({"style": "straight", "ids": [c]}));
    run(&mut s, "connector.style", json!({"style": "curved", "ids": [c]}));
    run(&mut s, "connector.style", json!({"style": "right_angle", "ids": [c]}));
    let d = doc(&s);
    assert_eq!((cell(d, c, "ShapeRouteStyle"), cell(d, c, "ConLineRouteExt")), (Some(1.0), Some(1.0)));
    assert_eq!(kinds(d, c), ["MoveTo", "LineTo", "LineTo", "LineTo"]);
}

#[test]
fn the_page_default_is_what_a_new_connector_takes_and_it_routes_nothing_again() {
    let (mut s, a, b, c) = z_pair();
    let r = run(&mut s, "connector.style", json!({"style": "curved"}));
    assert_eq!(r, json!({"page": 1, "style": "curved", "styled": 0, "default_changed": true}));
    let d = doc(&s);
    assert_eq!((page_cell(d, "RouteStyle"), page_cell(d, "LineRouteExt")), (Some(1.0), Some(2.0)));
    assert_eq!(
        kinds(d, c),
        ["MoveTo", "LineTo", "LineTo", "LineTo"],
        "the connector there is has the route it had (the reroute is the window's to ask for)"
    );
    let later = id_of(&run(&mut s, "connector.add", json!({"from": {"id": b, "point": 2}, "to": {"id": a, "point": 0}})));
    assert_eq!(kinds(doc(&s), later).last().map(String::as_str), Some("NURBSTo"), "a new connector takes the page's style");
    // The old one takes it at its next route (the project rule).
    run(&mut s, "shape.move", json!({"ids": [b], "dx": 0.0, "dy": 1.0}));
    assert_eq!(kinds(doc(&s), c), ["MoveTo", "NURBSTo"]);
    // The same again changes nothing and is no undo step.
    let was = quiet(&s);
    let r = run(&mut s, "connector.style", json!({"style": "curved"}));
    assert_eq!(r["default_changed"], json!(false));
    assert_eq!(quiet(&s), was);
}

#[test]
fn a_connector_with_a_style_of_its_own_ignores_the_page_and_zero_follows_it() {
    let (mut s, _, b, c) = z_pair();
    run(&mut s, "connector.style", json!({"style": "straight", "ids": [c]}));
    run(&mut s, "connector.style", json!({"style": "curved"}));
    run(&mut s, "shape.move", json!({"ids": [b], "dx": 0.0, "dy": 1.0}));
    assert_eq!(kinds(doc(&s), c), ["MoveTo", "LineTo"], "its own straight wins over the page's curve");
}

#[test]
fn a_style_is_one_undo_step_a_repeat_is_none_and_a_shape_that_is_no_connector_is_refused() {
    let (mut s, a, _, c) = z_pair();
    let was = snapshot(&s);
    run(&mut s, "connector.style", json!({"style": "straight", "ids": [c]}));
    assert_eq!(snapshot(&s).undo_steps, was.undo_steps + 1);
    let once = quiet(&s);
    let r = run(&mut s, "connector.style", json!({"style": "straight", "ids": [c]}));
    assert_eq!(r["styled"], json!(0));
    assert_eq!(quiet(&s), once, "the same style again is no undo step");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(doc(&s), was.document.as_ref().unwrap(), "one undo takes the cells and the route");
    let was = snapshot(&s);
    let e = s.execute("connector.style", &json!({"style": "straight", "ids": [a]})).unwrap_err();
    assert!(e.to_string().contains("not a connector"), "{e}");
    assert_eq!(snapshot(&s), was);
    for bad in [
        json!({"style": "wavy", "ids": [c]}),
        json!({"ids": [c]}),
        json!({"style": "straight", "ids": [999]}),
        json!({"style": "straight", "ids": []}),
    ] {
        let e = s.execute("connector.style", &bad);
        assert!(e.is_err(), "{bad}");
        assert_eq!(snapshot(&s), was, "{bad}");
    }
}
