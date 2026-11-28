//! A shape whose cell reads another shape's cell (`Sheet.2!PinX`): the scene reads the values a cell holds, and an edit that moves the shape that is read works out the one that reads it, which
//! is then another allocation, so the scene reads it again by the compare it makes for every shape (CodeRabbit asked whether a dependent shape keeps a stale box; it does not, and these tests
//! hold it to that: the scene put right is the one read whole, and the dependents are reported with their old and new boxes).

use serde_json::{Value, json};

use super::plenty;
use crate::Session;
use crate::draw::route_scene::{ChangedObstacle, Scene};

fn drop_box(s: &mut Session, x: f64, y: f64) -> u32 {
    let made = s.execute("shape.drop", &json!({"master": "process", "stencil": "flowchart", "page": 1, "x": x, "y": y})).unwrap();
    u32::try_from(made["id"].as_u64().unwrap()).unwrap()
}

fn link(s: &mut Session, id: u32, cell: &str, formula: &str) {
    s.execute("sheet.set_cell", &json!({"id": id, "page": 1, "cell": cell, "formula": formula})).unwrap();
}

fn put_right(s: &Session, scene: &mut Scene) -> crate::draw::route_scene::Changed {
    scene.put_right(s.document().unwrap(), 1, &mut plenty())
}

fn same_as_fresh(s: &Session, scene: &Scene, context: &str) {
    super::assert_same(scene, &Scene::read(s.document().unwrap(), 1, &mut plenty()), context);
}

fn ids(changed: &[ChangedObstacle]) -> Vec<u32> {
    changed.iter().map(|c| c.id).collect()
}

fn run(s: &mut Session, id: &str, params: Value) {
    s.execute(id, &params).unwrap();
}

/// The shape that reads `Sheet.<a>!PinX` moves with `a`: both are read again, and both are reported.
#[test]
fn a_shape_whose_pin_reads_another_follows_it() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let a = drop_box(&mut s, 2.0, 2.0);
    let b = drop_box(&mut s, 6.0, 2.0);
    let c = drop_box(&mut s, 4.0, 6.0);
    link(&mut s, b, "PinX", &format!("Sheet.{a}!PinX+4"));
    link(&mut s, b, "PinY", &format!("Sheet.{a}!PinY"));
    let mut scene = Scene::new(1);
    put_right(&s, &mut scene);
    assert_eq!(scene.obstacles().len(), 3, "three boxes, each an obstacle");
    let before_b = scene.obstacles().get(b).unwrap().rect;
    // The shape that is read moves: the one that reads it moves with it, and the one that does neither does not.
    run(&mut s, "shape.move", json!({"ids": [a], "dx": 1.5, "dy": 0.75}));
    let reads = scene.reads();
    let changed = put_right(&s, &mut scene);
    assert_eq!(ids(&changed.obstacles), vec![a, b], "the shape that was moved and the one that reads it: {:?}", changed.obstacles);
    let moved_b = changed.obstacles.iter().find(|c| c.id == b).unwrap();
    assert_eq!(moved_b.old, Some(before_b));
    let now_b = scene.obstacles().get(b).unwrap().rect;
    let (dx, dy) = (now_b.x0 - before_b.x0, now_b.y0 - before_b.y0);
    assert!((dx - 1.5).abs() < 1e-9 && (dy - 0.75).abs() < 1e-9, "the dependent moved by what the shape it reads moved: {dx}, {dy}");
    assert_eq!(moved_b.new, Some(now_b));
    assert_eq!(scene.reads().shapes - reads.shapes, 2, "two shapes read, and not the third (shape {c})");
    same_as_fresh(&s, &scene, "a shape that reads another moved");
    // A chain: c reads b, which reads a.
    link(&mut s, c, "PinX", &format!("Sheet.{b}!PinX-1"));
    put_right(&s, &mut scene);
    run(&mut s, "shape.move", json!({"ids": [a], "dx": -0.5, "dy": 0.0}));
    let chained = put_right(&s, &mut scene);
    assert_eq!(ids(&chained.obstacles), vec![a, b, c], "a chain of three: {:?}", chained.obstacles);
    same_as_fresh(&s, &scene, "a chain of shapes that read each other");
    // Size is read the same way.
    link(&mut s, c, "Width", &format!("Sheet.{a}!Width*2"));
    put_right(&s, &mut scene);
    run(&mut s, "shape.resize", json!({"id": a, "x": 1.0, "y": 1.0, "width": 2.0, "height": 1.0}));
    let resized = put_right(&s, &mut scene);
    assert!(ids(&resized.obstacles).contains(&c), "the shape whose width reads the other's: {:?}", resized.obstacles);
    same_as_fresh(&s, &scene, "a shape that reads another's width");
    // An undo and a redo bring the old boxes and the new, and each put right is the one read whole.
    run(&mut s, "edit.undo", json!({}));
    put_right(&s, &mut scene);
    same_as_fresh(&s, &scene, "undo");
    run(&mut s, "edit.redo", json!({}));
    put_right(&s, &mut scene);
    same_as_fresh(&s, &scene, "redo");
}

/// A member of a group whose cell reads a shape outside the group: that shape moves, the member's cached values are worked out, and the group (the top-level shape the scene holds) is another
/// allocation, so it is read again and the member's box is reported.
#[test]
fn a_member_of_a_group_that_reads_a_shape_outside_it_follows_it() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let a = drop_box(&mut s, 2.0, 2.0);
    let b = drop_box(&mut s, 6.0, 2.0);
    let outside = drop_box(&mut s, 4.0, 8.0);
    let grouped = s.execute("shape.group", &json!({"ids": [a, b]})).unwrap();
    // A group that holds 8 and not 1 is no obstacle itself, and its members are (a group of 9 is one obstacle).
    let group = u32::try_from(grouped["id"].as_u64().unwrap()).unwrap();
    s.edit("members", |doc| {
        super::build::set_number(doc, group, "ObjType", 8.0);
        Ok(())
    })
    .unwrap();
    link(&mut s, b, "PinY", &format!("Sheet.{outside}!PinY"));
    let mut scene = Scene::new(1);
    put_right(&s, &mut scene);
    assert!(scene.obstacles().get(b).is_some(), "the member is an obstacle");
    let before = scene.obstacles().get(b).unwrap().rect;
    run(&mut s, "shape.move", json!({"ids": [outside], "dx": 0.0, "dy": 1.25}));
    let changed = put_right(&s, &mut scene);
    let mut want = vec![b, outside];
    want.sort_unstable();
    assert_eq!(ids(&changed.obstacles), want, "the shape that moved and the member that reads it: {:?}", changed.obstacles);
    let moved = changed.obstacles.iter().find(|c| c.id == b).unwrap();
    assert_eq!(moved.old, Some(before));
    assert!(((moved.new.unwrap().y0 - before.y0) - 1.25).abs() < 1e-9, "the member moved by what the shape it reads moved: {moved:?}");
    same_as_fresh(&s, &scene, "a member that reads a shape outside its group");
}
