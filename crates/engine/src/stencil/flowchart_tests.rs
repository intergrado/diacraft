//! Basic Flowchart, dropped, drawn, resized and saved (tests first). The pixels are named in paper inches, as in
//! `tests/it/edit_render.rs`; no reference image from another application is used. The page's paper is left transparent so that a pixel
//! outside a shape (alpha 0) differs from the white of its fill.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use diacraft_doc::{CellAddr, Document, Owner, Resolver, ShapeRef, SheetRef};
use diacraft_render::{Rendered, Renderer, View};
use serde_json::{Value, json};

use super::{MasterSpec, all_masters, find_master, stencils};
use crate::Session;
use crate::draw::connection_points;

const PX_PER_INCH: f64 = 200.0;
const PAGE_HEIGHT: f64 = 8.5;
/// Where the one shape of a test is dropped: its centre.
const CENTRE: (f64, f64) = (5.5, 4.25);

fn flowchart() -> &'static [MasterSpec] {
    stencils()[1].masters()
}

pub(super) fn blank() -> Session {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    s
}

pub(super) fn run(s: &mut Session, id: &str, params: Value) -> Value {
    s.execute(id, &params).unwrap_or_else(|e| panic!("{id} {params}: {e}"))
}

pub(super) fn id_of(result: &Value) -> u32 {
    u32::try_from(result["id"].as_u64().unwrap()).unwrap()
}

pub(super) fn render(s: &Session) -> Rendered {
    let doc = s.document().unwrap();
    let view = View::fit_page(doc, 1, PX_PER_INCH).unwrap();
    let mut renderer = Renderer::new();
    renderer.set_paper(None);
    renderer.render(doc, 1, &view)
}

/// The pixel at a place on the page, in inches from its left and bottom edges: premultiplied RGBA.
pub(super) fn px(r: &Rendered, x: f64, y: f64) -> [u8; 4] {
    let (width, height) = (r.width as usize, r.height as usize);
    let col = ((x * PX_PER_INCH) as usize).min(width - 1);
    let row = (((PAGE_HEIGHT - y) * PX_PER_INCH) as usize).min(height - 1);
    let at = (row * width + col) * 4;
    let p = &r.rgba_premul[at..at + 4];
    [p[0], p[1], p[2], p[3]]
}

/// Ink: a pixel clearly darker than white and clearly there.
fn dark(p: [u8; 4]) -> bool {
    p[3] > 128 && u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2]) < 600
}

/// How many dark pixels a rectangle of the page holds: (left, bottom) to (right, top), in inches.
fn ink(r: &Rendered, left: f64, bottom: f64, right: f64, top: f64) -> usize {
    let mut n = 0;
    let (mut y, step) = (bottom, 1.0 / PX_PER_INCH);
    while y < top {
        let mut x = left;
        while x < right {
            n += usize::from(dark(px(r, x, y)));
            x += step;
        }
        y += step;
    }
    n
}

/// Ink within two hundredths of an inch of a place: the width of a line and a margin.
pub(super) fn ink_near(r: &Rendered, (x, y): (f64, f64)) -> usize {
    ink(r, x - 0.02, y - 0.02, x + 0.02, y + 0.02)
}

/// A place in the box of a master dropped at [`CENTRE`], given as shares of its width and height.
fn place(spec: &MasterSpec, (fx, fy): (f64, f64)) -> (f64, f64) {
    (CENTRE.0 - spec.width / 2.0 + fx * spec.width, CENTRE.1 - spec.height / 2.0 + fy * spec.height)
}

/// A session with the master dropped at the centre of the page.
fn dropped(spec: &MasterSpec) -> (Session, u32) {
    let mut s = blank();
    let r = run(&mut s, "shape.drop", json!({"master": spec.id, "page": 1, "x": CENTRE.0, "y": CENTRE.1}));
    let id = id_of(&r);
    (s, id)
}

/// What a person looks for to know a symbol: places that are filled, places that are bare paper, and places on the outline, each as shares
/// of the box. Worked out by hand from the drawing (`flowchart.rs`), at the default size.
struct Probe {
    id: &'static str,
    inside: &'static [(f64, f64)],
    outside: &'static [(f64, f64)],
    on: &'static [(f64, f64)],
}

const PROBES: [Probe; 18] = [
    Probe {
        id: "process",
        inside: &[(0.5, 0.5), (0.05, 0.1)],
        outside: &[(1.05, 0.5), (-0.05, 0.5)],
        on: &[(0.0, 0.5), (1.0, 0.5), (0.5, 0.0), (0.5, 1.0)],
    },
    Probe {
        id: "decision",
        inside: &[(0.5, 0.5), (0.3, 0.5)],
        outside: &[(0.1, 0.1), (0.9, 0.9), (0.1, 0.9), (0.9, 0.1)],
        on: &[(0.25, 0.25), (0.75, 0.75), (0.25, 0.75), (0.75, 0.25), (0.5, 0.0), (0.5, 1.0), (0.0, 0.5), (1.0, 0.5)],
    },
    Probe {
        // Half circles of radius 0.3 at each end: the 45 degree points of the caps are on the outline, and the corners are bare.
        id: "terminator",
        inside: &[(0.5, 0.5), (0.12, 0.5)],
        outside: &[(0.02, 0.05), (0.98, 0.95), (0.02, 0.95), (0.98, 0.05)],
        on: &[(0.0, 0.5), (1.0, 0.5), (0.5, 0.0), (0.5, 1.0), (0.0586, 0.1464), (0.9414, 0.1464), (0.0586, 0.8536), (0.9414, 0.8536)],
    },
    Probe {
        id: "data",
        inside: &[(0.5, 0.5)],
        outside: &[(0.98, 0.05), (0.02, 0.95)],
        on: &[(0.0667, 0.5), (0.9333, 0.5), (0.5667, 1.0), (0.4333, 0.0)],
    },
    Probe {
        // The right half of the bottom dips to the bottom of the box and the left half bulges up to a fifth: under the crest is bare.
        id: "document",
        inside: &[(0.5, 0.5), (0.75, 0.05)],
        outside: &[(0.25, 0.1)],
        on: &[(0.75, 0.0), (0.25, 0.2), (0.5, 0.1), (0.5, 1.0), (0.0, 0.55), (1.0, 0.55)],
    },
    Probe {
        id: "multiple_documents",
        inside: &[(0.3, 0.5), (0.5, 0.88), (0.5, 0.96), (0.9, 0.5), (0.965, 0.6)],
        outside: &[(0.03, 0.95), (0.97, 0.05), (0.9, 0.1)],
        on: &[(0.0, 0.46), (0.5, 1.0), (1.0, 0.62), (0.43, 0.08), (0.5, 0.92), (0.5, 0.84)],
    },
    Probe {
        id: "predefined_process",
        inside: &[(0.5, 0.5), (0.05, 0.5), (0.95, 0.5)],
        outside: &[(1.05, 0.5)],
        on: &[(0.1067, 0.5), (0.8933, 0.5), (0.0, 0.5), (1.0, 0.5), (0.5, 0.0), (0.5, 1.0)],
    },
    Probe {
        id: "manual_input",
        inside: &[(0.5, 0.4), (0.05, 0.3)],
        outside: &[(0.05, 0.95), (0.5, 0.99)],
        on: &[(0.5, 0.9), (0.0, 0.4), (1.0, 0.5), (0.5, 0.0)],
    },
    Probe {
        id: "manual_operation",
        inside: &[(0.5, 0.5), (0.1, 0.95)],
        outside: &[(0.02, 0.05), (0.98, 0.05)],
        on: &[(0.5, 1.0), (0.0667, 0.5), (0.9333, 0.5), (0.5, 0.0)],
    },
    Probe {
        id: "preparation",
        inside: &[(0.5, 0.5), (0.1, 0.5)],
        outside: &[(0.05, 0.05), (0.95, 0.95), (0.05, 0.95), (0.95, 0.05)],
        on: &[(0.0, 0.5), (1.0, 0.5), (0.5, 1.0), (0.5, 0.0), (0.1067, 0.75), (0.8933, 0.75), (0.1067, 0.25), (0.8933, 0.25)],
    },
    Probe {
        id: "on_page_reference",
        inside: &[(0.5, 0.3), (0.3, 0.5)],
        outside: &[(0.05, 0.05), (0.95, 0.95)],
        on: &[(0.0, 0.5), (1.0, 0.5), (0.5, 0.0), (0.5, 1.0), (0.1464, 0.1464), (0.8536, 0.8536)],
    },
    Probe {
        id: "off_page_reference",
        inside: &[(0.5, 0.7), (0.5, 0.3)],
        outside: &[(0.05, 0.05), (0.95, 0.05)],
        on: &[(0.5, 0.0), (0.0, 0.7), (1.0, 0.7), (0.5, 1.0), (0.25, 0.2), (0.75, 0.2)],
    },
    Probe {
        // The top face is filled, and the rim in front of it is a stroke across the fill.
        id: "database",
        inside: &[(0.5, 0.5), (0.5, 0.9), (0.5, 0.1)],
        outside: &[(0.03, 0.98), (0.03, 0.02), (0.97, 0.98), (0.97, 0.02)],
        on: &[(0.5, 1.0), (0.5, 0.0), (0.0, 0.5), (1.0, 0.5), (0.5, 0.76)],
    },
    Probe {
        id: "internal_storage",
        inside: &[(0.07, 0.93), (0.5, 0.5), (0.07, 0.3)],
        outside: &[(1.05, 0.5)],
        on: &[(0.15, 0.5), (0.5, 0.85), (0.0, 0.5), (1.0, 0.5), (0.5, 0.0), (0.5, 1.0)],
    },
    Probe {
        id: "delay",
        inside: &[(0.5, 0.5), (0.02, 0.05), (0.02, 0.95)],
        outside: &[(0.98, 0.95), (0.98, 0.05)],
        on: &[(1.0, 0.5), (0.0, 0.5), (0.3333, 1.0), (0.3333, 0.0), (0.9023, 0.8535), (0.9023, 0.1465)],
    },
    Probe {
        id: "display",
        inside: &[(0.5, 0.5), (0.1, 0.5)],
        outside: &[(0.03, 0.05), (0.03, 0.95), (0.99, 0.97), (0.99, 0.03)],
        on: &[(0.0, 0.5), (1.0, 0.5), (0.4467, 1.0), (0.4467, 0.0), (0.08, 0.75), (0.08, 0.25), (0.9219, 0.8535)],
    },
    Probe {
        id: "summing_junction",
        inside: &[(0.5, 0.2), (0.5, 0.8), (0.2, 0.5), (0.8, 0.5)],
        outside: &[(0.05, 0.05)],
        on: &[(0.5, 0.5), (0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75), (0.0, 0.5), (1.0, 0.5), (0.5, 0.0), (0.5, 1.0)],
    },
    Probe {
        id: "or",
        inside: &[(0.25, 0.25), (0.75, 0.75), (0.25, 0.75), (0.75, 0.25)],
        outside: &[(0.05, 0.05)],
        on: &[(0.5, 0.5), (0.5, 0.25), (0.5, 0.75), (0.25, 0.5), (0.75, 0.5), (0.0, 0.5), (1.0, 0.5), (0.5, 0.0), (0.5, 1.0)],
    },
];

#[test]
fn every_flowchart_master_has_a_probe_row_in_stencil_order() {
    let ids: Vec<&str> = flowchart().iter().map(|m| m.id).collect();
    let probed: Vec<&str> = PROBES.iter().map(|p| p.id).collect();
    assert_eq!(ids, probed);
}

/// The sheet of the shape with this `ID` on page 1, found by its `ID` and not its place.
fn sheet_of(d: &Document, id: u32) -> SheetRef {
    SheetRef::Shape(d.find_shape(Owner::Page(1), id).unwrap_or_else(|| panic!("no shape {id} on page 1")))
}

pub(super) fn num(d: &Document, id: u32, cell: &str) -> f64 {
    let res = Resolver::new(d);
    let sheet = sheet_of(d, id);
    let found = res.resolve(&sheet, &CellAddr::direct(cell)).unwrap_or_else(|| panic!("shape {id} has no {cell}"));
    found.value.cell.and_then(|c| c.value.as_ref()).and_then(|v| v.number()).unwrap_or_else(|| panic!("shape {id}: {cell} holds no number"))
}

/// The resolved rows of a shape's `Geometry` sections: for each section, each row's (type, X, Y).
fn geometry_of(d: &Document, sheet: &SheetRef) -> Vec<Vec<(String, f64, f64)>> {
    let res = Resolver::new(d);
    let keys = res.effective_sections(sheet, "Geometry").expect("Geometry sections");
    keys.iter()
        .map(|key| {
            let rows = res.effective_rows(sheet, key).expect("rows");
            rows.iter()
                .map(|row| {
                    let at = |name: &str| {
                        let addr = CellAddr::in_row("Geometry", key.ix, row.key.clone(), name);
                        let found = res.resolve(sheet, &addr).unwrap_or_else(|| panic!("no {name}"));
                        found.value.cell.and_then(|c| c.value.as_ref()).and_then(|v| v.number()).unwrap_or(0.0)
                    };
                    (row.row_type.as_deref().unwrap_or("").to_owned(), at("X"), at("Y"))
                })
                .collect()
        })
        .collect()
}

/// The geometry of the shape `id` on page 1.
pub(super) fn instance_geometry(d: &Document, id: u32) -> Vec<Vec<(String, f64, f64)>> {
    geometry_of(d, &sheet_of(d, id))
}

#[test]
fn every_flowchart_master_drops_with_its_size_its_geometry_and_its_connection_points() {
    for spec in flowchart() {
        let mut s = blank();
        let r = run(&mut s, "shape.drop", json!({"master": spec.name, "page": 1, "x": 3.0, "y": 4.0}));
        let id = id_of(&r);
        assert_eq!(id, 1, "{}", spec.name);
        let d = s.document().unwrap();
        assert_eq!(d.masters.len(), 1, "{}: the master is copied in", spec.name);
        assert_eq!(d.masters.iter().next().unwrap().name.as_deref(), Some(spec.name));
        assert_eq!((r["width"].as_f64(), r["height"].as_f64()), (Some(spec.width), Some(spec.height)), "{}", spec.name);
        assert!((num(d, id, "Width") - spec.width).abs() < 1e-9 && (num(d, id, "Height") - spec.height).abs() < 1e-9, "{}", spec.name);
        assert!((num(d, id, "PinX") - 3.0).abs() < 1e-9 && (num(d, id, "PinY") - 4.0).abs() < 1e-9, "{}", spec.name);
        // The instance holds only its pin; the geometry resolves through the master, section by section.
        let rows = instance_geometry(d, id);
        let (master_doc, master) = {
            let mut doc = Document::new_blank();
            let master = super::ensure_master(&mut doc, spec).unwrap();
            (doc, master)
        };
        let master_rows = geometry_of(&master_doc, &SheetRef::Shape(ShapeRef::top(Owner::Master(master), 0)));
        assert_eq!(rows, master_rows, "{}: the instance reads the master's geometry", spec.name);
        // Every connection point is on the page inside the box, and leaves along an axis.
        let points = connection_points(d, 1, id);
        assert_eq!(points.len(), spec.point_count(), "{}", spec.name);
        for p in &points {
            assert!((3.0 - spec.width / 2.0 - 1e-9..=3.0 + spec.width / 2.0 + 1e-9).contains(&p.x), "{}: point {} x", spec.name, p.index);
            assert!((4.0 - spec.height / 2.0 - 1e-9..=4.0 + spec.height / 2.0 + 1e-9).contains(&p.y), "{}: point {} y", spec.name, p.index);
            assert!((p.dir_x.abs() + p.dir_y.abs() - 1.0).abs() < 1e-9, "{}: point {} direction", spec.name, p.index);
        }
    }
}

#[test]
fn every_masters_connection_points_are_on_its_drawn_outline() {
    // The strongest reading of "where a connector would meet it": the pixels of the stroke are at the point. Basic Shapes are held to it too.
    for spec in all_masters() {
        let (s, id) = dropped(spec);
        let r = render(&s);
        assert!(r.skipped.is_empty(), "{}: {:?}", spec.name, r.skipped);
        for p in connection_points(s.document().unwrap(), 1, id) {
            assert!(ink_near(&r, (p.x, p.y)) > 0, "{}: connection point {} at ({:.3}, {:.3}) is not on the outline", spec.name, p.index, p.x, p.y);
        }
    }
}

#[test]
fn every_flowchart_master_is_drawn_inside_its_box_and_touches_all_four_sides() {
    for spec in flowchart() {
        let (s, _) = dropped(spec);
        let r = render(&s);
        assert!(r.skipped.is_empty(), "{}: {:?}", spec.name, r.skipped);
        let (left, right) = (CENTRE.0 - spec.width / 2.0, CENTRE.0 + spec.width / 2.0);
        let (bottom, top) = (CENTRE.1 - spec.height / 2.0, CENTRE.1 + spec.height / 2.0);
        let m = 0.03;
        assert!(ink(&r, left - m, bottom - m, right + m, top + m) > 100, "{}: drawn", spec.name);
        assert_eq!(ink(&r, 0.0, 0.0, left - m, PAGE_HEIGHT), 0, "{}: nothing left of the box", spec.name);
        assert_eq!(ink(&r, right + m, 0.0, 11.0, PAGE_HEIGHT), 0, "{}: nothing right of it", spec.name);
        assert_eq!(ink(&r, 0.0, 0.0, 11.0, bottom - m), 0, "{}: nothing below it", spec.name);
        assert_eq!(ink(&r, 0.0, top + m, 11.0, PAGE_HEIGHT), 0, "{}: nothing above it", spec.name);
        assert!(ink(&r, left - 0.01, bottom, left + m, top) > 0, "{}: the left side is touched", spec.name);
        assert!(ink(&r, right - m, bottom, right + 0.01, top) > 0, "{}: the right side", spec.name);
        assert!(ink(&r, left, bottom - 0.01, right, bottom + m) > 0, "{}: the bottom", spec.name);
        assert!(ink(&r, left, top - m, right, top + 0.01) > 0, "{}: the top", spec.name);
    }
}

#[test]
fn each_flowchart_symbol_draws_as_its_shape() {
    for (spec, probe) in flowchart().iter().zip(&PROBES) {
        assert_eq!(spec.id, probe.id);
        let (s, _) = dropped(spec);
        let r = render(&s);
        for &at in probe.inside {
            let (x, y) = place(spec, at);
            assert_eq!(px(&r, x, y), [255, 255, 255, 255], "{}: {at:?} should be the white of the fill", spec.name);
        }
        for &at in probe.outside {
            let (x, y) = place(spec, at);
            assert_eq!(px(&r, x, y)[3], 0, "{}: {at:?} should be bare paper", spec.name);
        }
        for &at in probe.on {
            assert!(ink_near(&r, place(spec, at)) > 0, "{}: {at:?} should be on a line", spec.name);
        }
    }
}

#[test]
fn a_filled_symbol_keeps_its_fill_under_an_inner_stroke() {
    // The rim of a database and the rule of an internal storage cross the fill and do not cut it: a pixel a hair either side of the stroke is
    // white (a path with NoFill 1 takes no part in the crossings).
    let database = find_master("database").unwrap();
    let (s, _) = dropped(database);
    let r = render(&s);
    let rim = place(database, (0.5, 0.76));
    assert!(ink_near(&r, rim) > 0);
    assert_eq!(px(&r, rim.0, rim.1 + 0.05), [255, 255, 255, 255], "above the rim: the top face");
    assert_eq!(px(&r, rim.0, rim.1 - 0.05), [255, 255, 255, 255], "below the rim: the body");
}

#[test]
fn a_resized_terminator_keeps_round_ends_and_a_resized_data_symbol_keeps_its_slant() {
    let mut s = blank();
    let terminator = id_of(&run(&mut s, "shape.drop", json!({"master": "terminator", "page": 1, "x": 3.0, "y": 4.0})));
    let data = id_of(&run(&mut s, "shape.drop", json!({"master": "data", "page": 1, "x": 3.0, "y": 2.0})));
    let database = id_of(&run(&mut s, "shape.drop", json!({"master": "database", "page": 1, "x": 8.0, "y": 4.0})));
    // Widen the terminator from 1.5 to 3.0 inches, the data symbol to 3.0, and the database from 1.0 to 2.0.
    run(&mut s, "shape.resize", json!({"id": terminator, "x": 1.5, "y": 3.7, "width": 3.0, "height": 0.6}));
    run(&mut s, "shape.resize", json!({"id": data, "x": 1.5, "y": 1.6, "width": 3.0, "height": 0.8}));
    run(&mut s, "shape.resize", json!({"id": database, "x": 7.0, "y": 3.4, "width": 2.0, "height": 1.2}));
    let d = s.document().unwrap();
    let near = |a: f64, b: f64, what: &str| assert!((a - b).abs() < 1e-6, "{what}: {a} against {b}");
    // The terminator: the straight run ends 0.3 from the right, whatever the width; the caps are still half circles.
    let t = instance_geometry(d, terminator);
    near(t[0][0].1, 0.3, "the left end of the bottom edge");
    near(t[0][1].1, 2.7, "where the right cap begins");
    near(t[0][2].1, 2.7, "the cap's end");
    // The data symbol: the slant is still 0.2, so the bottom edge ends 0.2 short of the right.
    let g = instance_geometry(d, data);
    near(g[0][1].1, 2.8, "the bottom right corner");
    near(g[0][3].1, 0.2, "the top left corner");
    // The database's rim is still a half-ellipse: the ratio of the semi-axes follows the box, 1.0 wide and 0.144 tall.
    let res = Resolver::new(d);
    let sheet = sheet_of(d, database);
    let keys = res.effective_sections(&sheet, "Geometry").unwrap();
    let rows = res.effective_rows(&sheet, &keys[0]).unwrap();
    let ratio = rows.iter().find(|row| row.row_type.as_deref() == Some("EllipticalArcTo")).map(|row| {
        let addr = CellAddr::in_row("Geometry", keys[0].ix, row.key.clone(), "D");
        res.resolve(&sheet, &addr).unwrap().value.cell.and_then(|c| c.value.as_ref()).and_then(|v| v.number()).unwrap()
    });
    near(ratio.unwrap(), 1.0 / 0.144, "the rim's ratio");
    // Connection points follow: the right point of the data symbol is 0.1 in from the right edge (half the slant).
    let points = connection_points(d, 1, data);
    near(points[3].x, 3.0 + 1.5 - 0.1, "the data symbol's right point");
    // And the drawing: the wide terminator still has its corners bare and its ends round, the wide database its rim.
    let r = render(&s);
    assert_eq!(px(&r, 1.53, 3.73)[3], 0, "the corner of the wide terminator is bare");
    assert!(ink_near(&r, (1.5, 4.0)) > 0 && ink_near(&r, (4.5, 4.0)) > 0, "both ends");
    assert!(ink_near(&r, (3.0, 3.7)) > 0 && ink_near(&r, (3.0, 4.3)) > 0, "the straight edges run the whole way");
    assert!(ink_near(&r, (8.0, 3.4 + 1.2 * 0.76)) > 0, "the rim of the wide database");
}

#[test]
fn the_stencil_list_command_serves_both_stencils_and_shape_drop_takes_a_master_of_either() {
    // With no document open, as the window asks it.
    let mut none = Session::new();
    let listed = none.execute("stencil.list", &json!({})).unwrap();
    assert_eq!(listed, super::list());
    let all = listed["stencils"].as_array().unwrap();
    assert_eq!(
        all.iter().map(|s| s["name"].as_str().unwrap()).collect::<Vec<_>>(),
        [
            "Basic Shapes",
            "Basic Flowchart",
            "Decorative Shapes",
            "Arrow Shapes",
            "Gane-Sarson",
            "Cross-Functional Flowchart Shapes",
            "Organization Chart Shapes"
        ]
    );
    assert_eq!(all[0]["masters"].as_array().unwrap().len(), 12);
    assert_eq!(all[1]["masters"].as_array().unwrap().len(), flowchart().len());
    // Every listed master, of either stencil, can be dropped by the id the list gives.
    let mut s = blank();
    for stencil in all {
        for master in stencil["masters"].as_array().unwrap() {
            let r = run(&mut s, "shape.drop", json!({"master": master["id"], "page": 1, "x": 5.0, "y": 4.0}));
            // The instance is named for its master and numbered (`Rectangle.1`).
            assert!(r["name"].as_str().unwrap().starts_with(master["name"].as_str().unwrap()), "{r}");
        }
    }
}

/// A scratch folder under `TMPDIR`, removed on drop.
pub(super) struct Dir(PathBuf);

impl Dir {
    pub(super) fn new() -> Dir {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("diacraft-flowchart-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        Dir(dir)
    }

    pub(super) fn path(&self, name: &str) -> String {
        self.0.join(name).to_str().unwrap().to_owned()
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_page_with_one_of_each_symbol_is_saved_and_read_back_the_same() {
    let mut s = blank();
    let mut placed = Vec::new();
    for (i, spec) in flowchart().iter().enumerate() {
        let (col, row) = (i % 6, i / 6);
        let (x, y) = (1.0 + 1.8 * col as f64, 7.0 - 2.4 * row as f64);
        placed.push((id_of(&run(&mut s, "shape.drop", json!({"master": spec.id, "page": 1, "x": x, "y": y}))), spec));
    }
    // A connector from the process to the decision beside it, glued to their connection points, and a label.
    let joined = run(&mut s, "connector.add", json!({"from": {"id": placed[0].0}, "to": {"id": placed[1].0}}));
    assert_eq!(joined["begin"]["id"], placed[0].0);
    run(&mut s, "shape.set_text", json!({"id": placed[1].0, "text": "Yes?"}));
    let before = render(&s);
    assert!(before.skipped.is_empty(), "{:?}", before.skipped);
    assert_eq!(s.document().unwrap().masters.len(), flowchart().len());

    let dir = Dir::new();
    let path = dir.path("flowchart.vsdx");
    run(&mut s, "file.save_as", json!({"path": path}));

    let mut reopened = Session::new();
    run(&mut reopened, "file.open", json!({"path": path}));
    let doc = reopened.document().unwrap();
    // The masters came back by name, each once, with their geometry sections and connection points.
    let mut names: Vec<&str> = doc.masters.iter().filter_map(|m| m.name.as_deref()).collect();
    names.sort_unstable();
    let mut wanted: Vec<&str> = flowchart().iter().map(|m| m.name).collect();
    wanted.sort_unstable();
    assert_eq!(names, wanted);
    for (id, spec) in &placed {
        assert!((num(doc, *id, "Width") - spec.width).abs() < 1e-9 && (num(doc, *id, "Height") - spec.height).abs() < 1e-9, "{}", spec.name);
        assert_eq!(connection_points(doc, 1, *id).len(), spec.point_count(), "{}", spec.name);
        let (original, again) = (instance_geometry(s.document().unwrap(), *id), instance_geometry(doc, *id));
        assert_eq!(original, again, "{}: the geometry reads back", spec.name);
    }
    // And the page looks the same, pixel for pixel.
    let after = render(&reopened);
    assert!(after.skipped.is_empty(), "{:?}", after.skipped);
    assert_eq!((after.width, after.height), (before.width, before.height));
    assert!(after.rgba_premul == before.rgba_premul, "the reopened page is drawn as it was");
    // Dropping a symbol that is already in the file adds no second master.
    let mut again = reopened;
    run(&mut again, "shape.drop", json!({"master": "process", "page": 1, "x": 5.0, "y": 1.0}));
    assert_eq!(again.document().unwrap().masters.len(), flowchart().len());
}
