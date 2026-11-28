//! Original contract vectors, not observations of another application.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use diacraft_doc::{CellAddr, Owner, Resolver, RowKey, SheetRef};
use serde_json::json;

use super::flowchart_tests::{Dir, blank, id_of, ink_near, instance_geometry, num, px, render, run};
use super::{MasterSpec, Outline, find_master, find_master_in, stencils};
use crate::Session;
use crate::draw::text_view;

/// The stencils of the first batches (the seven a new session opens). The second batch's stencils are shipped after them, and have their own tests (`network/tests.rs` and the like).
fn first_seven() -> impl Iterator<Item = &'static super::StencilSpec> {
    stencils().iter().filter(|s| s.default_open)
}

fn batch() -> impl Iterator<Item = &'static MasterSpec> {
    first_seven().skip(2).flat_map(|stencil| stencil.masters())
}

fn drop_at(s: &mut Session, master: &str, x: f64, y: f64) -> u32 {
    id_of(&run(s, "shape.drop", json!({"master": master, "page": 1, "x": x, "y": y})))
}

fn shape(s: &Session, id: u32) -> &diacraft_doc::Shape {
    let doc = s.document().unwrap();
    doc.shape(&doc.find_shape(Owner::Page(1), id).unwrap()).unwrap()
}

fn geometry_number(s: &Session, id: u32, row: u32, name: &str) -> f64 {
    let doc = s.document().unwrap();
    let sheet = SheetRef::Shape(doc.find_shape(Owner::Page(1), id).unwrap());
    Resolver::new(doc)
        .resolve(&sheet, &CellAddr::in_row("Geometry", Some(0), RowKey::Ix(row), name))
        .unwrap()
        .value
        .cell
        .unwrap()
        .value
        .as_ref()
        .unwrap()
        .number()
        .unwrap()
}

fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-8, "{a} != {b}");
}

#[test]
fn catalogue_identity_and_process_lookup_remain_compatible() {
    assert_eq!(first_seven().map(|s| s.masters().len()).collect::<Vec<_>>(), [12, 18, 8, 8, 4, 2, 6]);
    assert_eq!(first_seven().map(|s| s.masters().len()).sum::<usize>(), 58);
    for (series, stencil) in stencils().iter().take(2).enumerate() {
        for (ordinal, master) in stencil.masters().iter().enumerate() {
            assert_eq!(master.guid(), format!("{{D1AC000{}-0000-4000-8000-{:012X}}}", series + 1, ordinal + 1));
        }
    }
    assert_eq!(find_master("Process").unwrap().id, "process");
    assert_eq!(find_master("gane-sarson.process").unwrap().identity, (6, 2));
    assert_eq!(find_master_in(&stencils()[4], "Process").unwrap().id, "gane-sarson.process");
    // Decorative 4, Arrows 5, Gane-Sarson 6, Cross-Functional Flowchart 7 and Organization Chart 8: each library its own series (the last two were both 7).
    for (namespace, stencil) in (4..=8).zip(stencils().iter().skip(2)) {
        for (ordinal, master) in stencil.masters().iter().enumerate() {
            assert_eq!(master.guid(), format!("{{D1AC000{namespace}-0000-4000-8000-{:012X}}}", ordinal + 1));
        }
    }
}

#[test]
fn polygons_are_bounded_simple_and_have_the_specified_star_radii() {
    let cross = |a: (f64, f64), b: (f64, f64), c: (f64, f64)| (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
    for spec in batch().take(16) {
        let Outline::Polygon(vertices) = spec.outline else { panic!("{} is not a polygon", spec.id) };
        assert!((6..=64).contains(&vertices.len()));
        if spec.id.starts_with("arrows.") {
            assert!(vertices.len() <= 32);
        }
        assert!(vertices.iter().all(|(x, y)| (0.0..=1.0).contains(x) && (0.0..=1.0).contains(y)), "{}", spec.id);
        for i in 0..vertices.len() {
            let (a, b) = (vertices[i], vertices[(i + 1) % vertices.len()]);
            for j in i + 2..vertices.len() {
                if i == 0 && j + 1 == vertices.len() {
                    continue;
                }
                let (c, d) = (vertices[j], vertices[(j + 1) % vertices.len()]);
                assert!(!(cross(a, b, c) * cross(a, b, d) < -1e-12 && cross(c, d, a) * cross(c, d, b) < -1e-12), "{} crosses itself", spec.id);
            }
        }
    }
    for (id, n, inner) in
        [("decorative.star-5", 5, 0.22), ("decorative.star-6", 6, 0.25), ("decorative.star-8", 8, 0.28), ("decorative.burst", 12, 0.39)]
    {
        let Outline::Polygon(points) = find_master(id).unwrap().outline else { panic!("star polygon") };
        assert_eq!(points.len(), n * 2);
        for (i, (x, y)) in points.iter().enumerate() {
            near((x - 0.5).hypot(y - 0.5), if i % 2 == 0 { 0.5 } else { inner });
        }
        assert_eq!(points[0], (0.5, 1.0));
    }
    for (id, tip) in [("arrows.right", (1.0, 0.5)), ("arrows.left", (0.0, 0.5)), ("arrows.up", (0.5, 1.0)), ("arrows.down", (0.5, 0.0))] {
        let Outline::Polygon(points) = find_master(id).unwrap().outline else { panic!("arrow polygon") };
        assert!(points.contains(&tip));
    }
}

#[test]
fn every_new_node_resizes_rotates_copies_and_undoes_with_its_label() {
    for spec in batch().filter(|spec| !spec.is_connector()) {
        let mut s = blank();
        let id = drop_at(&mut s, spec.id, 4.0, 4.0);
        let original = instance_geometry(s.document().unwrap(), id);
        run(&mut s, "shape.set_text", json!({"id":id,"text":spec.name}));
        run(&mut s, "shape.resize", json!({"id":id,"x":2.0,"y":2.0,"width":spec.width*2.0,"height":spec.height*0.5}));
        let resized = instance_geometry(s.document().unwrap(), id);
        if spec.id != "gane-sarson.process" {
            for (a, b) in original.iter().zip(&resized) {
                for (old, new) in a.iter().zip(b) {
                    assert_eq!(old.0, new.0);
                    near(new.1, old.1 * 2.0);
                    near(new.2, old.2 * 0.5);
                }
            }
        }
        run(&mut s, "shape.rotate", json!({"ids":[id],"angle":30}));
        run(&mut s, "shape.flip", json!({"ids":[id],"axis":"horizontal"}));
        let before = s.document().unwrap().clone();
        let copy = run(&mut s, "shape.duplicate", json!({"ids":[id]}));
        let copied = u32::try_from(copy["ids"][0].as_u64().unwrap()).unwrap();
        assert_eq!(text_view(s.document().unwrap(), 1, copied).unwrap().chars, spec.name);
        assert_eq!(shape(&s, id).master, shape(&s, copied).master);
        let after = s.document().unwrap().clone();
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(s.document().unwrap(), &before);
        run(&mut s, "edit.redo", json!({}));
        assert_eq!(s.document().unwrap(), &after);
        let drawn = render(&s);
        assert!(drawn.skipped.is_empty(), "{}: {:?}", spec.id, drawn.skipped);
    }
}

#[test]
fn rounded_process_recomputes_circular_corners_and_store_remains_open() {
    let mut s = blank();
    let legacy = drop_at(&mut s, "Process", 7.0, 4.0);
    let process = drop_at(&mut s, "gane-sarson.process", 3.0, 4.0);
    assert_ne!(shape(&s, legacy).master, shape(&s, process).master);
    near(geometry_number(&s, process, 1, "X"), 0.12);
    run(&mut s, "shape.resize", json!({"id":process,"x":1.0,"y":3.0,"width":3.2,"height":0.5}));
    near(geometry_number(&s, process, 1, "X"), 0.06);
    near(geometry_number(&s, process, 2, "X"), 3.2 - 0.06);
    near(geometry_number(&s, process, 3, "Y"), 0.06);
    near(geometry_number(&s, process, 3, "D"), 1.0);
    let store = drop_at(&mut s, "gane-sarson.data-store", 4.0, 1.0);
    run(&mut s, "shape.resize", json!({"id":store,"x":2.0,"y":1.0,"width":3.6,"height":0.4}));
    let drawn = render(&s);
    assert_eq!(px(&drawn, 5.6, 1.2)[3], 0, "open right side");
    for at in [(2.0, 1.2), (4.0, 1.0), (4.0, 1.4)] {
        assert!(ink_near(&drawn, at) > 0);
    }
    assert_eq!(px(&drawn, 1.005, 3.005)[3], 0, "rounded corner is bare");
}

#[test]
fn concave_contours_leave_their_notches_unfilled() {
    for (id, empty) in [
        ("decorative.star-5", (0.08, 0.85)),
        ("decorative.cross", (0.1, 0.1)),
        ("decorative.ribbon", (0.04, 0.5)),
        ("decorative.heart", (0.5, 0.98)),
        ("arrows.right", (0.1, 0.1)),
        ("arrows.four-way", (0.1, 0.1)),
        ("arrows.bent", (0.4, 0.15)),
    ] {
        let mut s = blank();
        let spec = find_master(id).unwrap();
        drop_at(&mut s, id, 5.0, 4.0);
        let drawn = render(&s);
        assert_eq!(px(&drawn, 5.0 + (empty.0 - 0.5) * spec.width, 4.0 + (empty.1 - 0.5) * spec.height)[3], 0, "{id}: notch");
        assert!(drawn.skipped.is_empty());
    }
}

#[test]
fn mixed_batch_vsdx_roundtrip_retains_masters_geometry_text_and_connector_glue() {
    let mut s = blank();
    let mut ids = Vec::new();
    for (i, spec) in batch().enumerate() {
        let id = drop_at(&mut s, spec.id, 1.0 + (i % 5) as f64 * 2.0, 7.5 - (i / 5) as f64 * 1.7);
        run(&mut s, "shape.set_text", json!({"id":id,"text":if spec.is_connector(){"Order data"}else{spec.name}}));
        ids.push((id, spec));
    }
    let (interface, process, flow) = (ids[16].0, ids[17].0, ids[19].0);
    let doc = s.document().unwrap();
    near(num(doc, flow, "EndX") - num(doc, flow, "BeginX"), 1.5);
    near(num(doc, flow, "Height"), 0.0);
    near(num(doc, flow, "BeginArrow"), 0.0);
    near(num(doc, flow, "EndArrow"), 4.0);
    assert!(s.execute("shape.resize", &json!({"id":flow,"x":1,"y":1,"width":2,"height":1})).is_err());
    for (end, node) in [("begin", interface), ("end", process)] {
        run(&mut s, "connector.reconnect", json!({"id":flow,"end":end,"target":{"id":node}}));
    }
    let before_move = s.document().unwrap().clone();
    let end_y = num(s.document().unwrap(), flow, "EndY");
    run(&mut s, "shape.move", json!({"ids":[process],"dx":0,"dy":0.4}));
    near(num(s.document().unwrap(), flow, "EndY"), end_y + 0.4);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.document().unwrap(), &before_move);
    run(&mut s, "edit.redo", json!({}));
    let before = render(&s);
    assert!(before.skipped.is_empty(), "{:?}", before.skipped);
    let dir = Dir::new();
    let path = dir.path("batch.vsdx");
    run(&mut s, "file.save_as", json!({"path":path}));
    let mut reopened = Session::new();
    run(&mut reopened, "file.open", json!({"path":path}));
    let doc = reopened.document().unwrap();
    assert_eq!(doc.masters.len(), 28);
    assert_eq!(doc.page(1).unwrap().connects.len(), 2);
    for (id, spec) in &ids {
        assert_eq!(instance_geometry(s.document().unwrap(), *id), instance_geometry(doc, *id), "{}", spec.id);
        let master = doc.master(shape(&reopened, *id).master.unwrap()).unwrap();
        assert_eq!(master.unique_id.as_deref(), Some(spec.unique_id().as_str()), "{}: the master reopened keeps its UniqueID", spec.id);
        assert_eq!(master.raw.get("BaseID"), Some(spec.guid().as_str()), "{}: and its BaseID", spec.id);
        assert_eq!(text_view(doc, 1, *id).unwrap().chars, if spec.is_connector() { "Order data" } else { spec.name });
    }
    let after = render(&reopened);
    assert!(after.skipped.is_empty());
    assert_eq!(before.rgba_premul, after.rgba_premul);
    let end_x = num(reopened.document().unwrap(), flow, "EndX");
    run(&mut reopened, "shape.move", json!({"ids":[process],"dx":0.3,"dy":0}));
    near(num(reopened.document().unwrap(), flow, "EndX"), end_x + 0.3);
    near(num(reopened.document().unwrap(), flow, "EndArrow"), 4.0);
    drop_at(&mut reopened, "gane-sarson.process", 4.0, 4.0);
    assert_eq!(reopened.document().unwrap().masters.len(), 28, "reuses the rounded master");
    drop_at(&mut reopened, "Process", 7.0, 4.0);
    assert_eq!(reopened.document().unwrap().masters.len(), 29, "legacy process is distinct");
}
