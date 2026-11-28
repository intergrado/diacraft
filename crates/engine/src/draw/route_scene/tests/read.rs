//! What a scene reads of a page: the obstacles, the wires, the glue, and that a scene put right equals one read whole after each kind of change.

use diacraft_doc::{Cell, CellName, ShapeType, Value};
use diacraft_route::{Point, Rect};
use diacraft_testkit::build::{DocBuilder, MasterBuilder, ShapeBuilder};

use super::build::{
    add, add_layers, boxed, glue, glue_both, group, line, on_layers, ours, page, page_in, placeable, reglue, remove, row_of_boxes, set_number, shift,
    show_layer,
};
use super::oracle::assert_matches;
use super::{assert_same, plenty};
use crate::draw::reroute::Reroute;
use crate::draw::route_scene::{LegsState, MAX_SCENE_SHAPES, Scene};
use crate::draw::style::PathStyle;

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Rect {
    Rect { x0, y0, x1, y1 }
}

fn obstacle_ids(scene: &Scene) -> Vec<u32> {
    scene.dump().candidates.iter().filter(|c| c.3.is_some()).map(|c| c.0).collect()
}

/// P holds 1, G holds 0 and an end of another connector is glued to it, N holds 0 and is glued to nothing, F holds 5 (neither): P and G are obstacles and N and F are not; a
/// one-dimensional shape, a guide and a box of no width are none; and a group of 8 gives its members while a group of 9 is one.
#[test]
fn a_scene_holds_the_obstacles_the_rules_say() {
    let doc = page(
        vec![
            placeable(1, (0.0, 0.0, 2.0, 1.0)),
            boxed(2, (0.0, 3.0, 2.0, 1.0)).cell("ObjType", "0"),
            boxed(3, (0.0, 6.0, 2.0, 1.0)).cell("ObjType", "0"),
            boxed(4, (0.0, 9.0, 2.0, 1.0)).cell("ObjType", "5"),
            boxed(5, (0.0, 12.0, 2.0, 1.0)).cell("ObjType", "1").kind(ShapeType::Guide),
            line(6, &[(10.0, 0.0), (10.0, 5.0)]).cell("ObjType", "1"),
            boxed(7, (0.0, 15.0, 0.0, 1.0)).cell("ObjType", "1"),
            // A group of 8 gives its two members, whatever their own boxes are inside it.
            group(8, (20.0, 0.0, 6.0, 1.0), "8", vec![placeable(9, (0.0, 0.0, 1.0, 1.0)), placeable(10, (3.0, 0.0, 1.0, 1.0))]),
            // A group of 9 is one obstacle and its members are not looked at.
            group(11, (20.0, 5.0, 6.0, 1.0), "9", vec![placeable(12, (0.0, 0.0, 1.0, 1.0)), placeable(13, (3.0, 0.0, 1.0, 1.0))]),
            // P2 lets level legs through.
            placeable(14, (30.0, 0.0, 2.0, 1.0)).cell("ShapePermeableX", "1"),
            ours(line(20, &[(1.0, 3.5), (1.0, 1.0)])),
        ],
        glue_both(20, 2, 1),
    );
    let scene = Scene::read(&doc, 1, &mut plenty());
    assert_eq!(obstacle_ids(&scene), vec![1, 2, 9, 10, 11, 14], "P, G, the two members of the group of 8, the group of 9 and the permeable box");
    let router = scene.obstacles();
    let p14 = router.get(14).unwrap();
    assert!(p14.pass_level && !p14.pass_upright, "ShapePermeableX 1 lets level legs through and keeps upright ones out");
    assert!(router.get(9).is_some() && router.get(12).is_none(), "the members of 8 are obstacles and the members of 9 are not looked at");
    assert_eq!(router.get(11).unwrap().rect, rect(20.0, 5.0, 26.0, 6.0), "the group of 9 is its own box");
    assert_eq!(router.get(10).unwrap().rect, rect(23.0, 0.0, 24.0, 1.0), "a member's box is carried through its group's frame");
    assert_matches(&scene, &doc, MAX_SCENE_SHAPES, "");
    // A shape that is none now is held all the same, to be one when a connector is glued to it.
    assert!(scene.candidate(3).is_some() && scene.candidate(4).is_some(), "N and F are candidates");
    assert!(
        scene.candidate(5).is_none() && scene.candidate(6).is_none() && scene.candidate(7).is_none(),
        "a guide, a line and a box of no width are no candidate"
    );
}

#[test]
fn a_connector_is_a_wire_when_it_is_ours_or_glued_and_a_plain_line_is_not() {
    let doc = page(
        vec![
            placeable(1, (0.0, 0.0, 2.0, 1.0)),
            placeable(2, (8.0, 0.0, 2.0, 1.0)),
            // A plain line: no glue, not ours.
            line(30, &[(0.0, 5.0), (4.0, 5.0)]),
            // Ours, glued nowhere: a Z of three legs.
            ours(line(31, &[(0.0, 8.0), (2.0, 8.0), (2.0, 9.0), (6.0, 9.0)])).cell("ConFixedCode", "3"),
            // Glued at its begin only, with a route of two legs, an appearance of its own, and the style 2.
            line(32, &[(2.0, 0.5), (5.0, 0.5), (5.0, 3.0)]).cell("ShapeRouteStyle", "2"),
        ],
        vec![glue(32, "BeginX", 1), glue(32, "BeginY", 1)],
    );
    let scene = Scene::read(&doc, 1, &mut plenty());
    let plain = scene.connector(30).unwrap();
    assert!(!plain.wired && plain.state == LegsState::Unread && plain.legs.is_empty(), "a plain line is no wire and its route is not read");
    let z = scene.connector(31).unwrap();
    assert!(z.wired && z.ours && z.state == LegsState::Read, "a connector of ours is a wire");
    assert_eq!(z.legs.len(), 3);
    assert_eq!(z.legs[1], (Point::new(2.0, 8.0), Point::new(2.0, 9.0)), "the legs are the route's, in order, on the page");
    assert_eq!(z.setting, Reroute::OnCrossover, "ConFixedCode 3 is on crossover");
    assert_eq!(z.glue, [None, None]);
    let glued = scene.connector(32).unwrap();
    assert!(glued.wired && !glued.ours, "a glued connector is a wire");
    assert_eq!(glued.glue, [Some(1), None], "its begin is glued to shape 1 and its end to nothing");
    assert_eq!(glued.style, PathStyle::Straight, "ShapeRouteStyle 2 is straight");
    assert_eq!(scene.wires().len(), 2, "the two connectors are wires");
    assert_eq!(scene.wires().leg_count(), 5);
    assert_matches(&scene, &doc, MAX_SCENE_SHAPES, "wires");
}

/// A connector inside a group is none of the page's wires: the scene is the page's top level.
#[test]
fn a_connector_inside_a_group_is_no_wire() {
    let doc = page(
        vec![
            group(1, (0.0, 0.0, 10.0, 10.0), "0", vec![ours(line(2, &[(0.0, 0.0), (4.0, 0.0)])), placeable(3, (6.0, 6.0, 1.0, 1.0))]),
            ours(line(4, &[(0.0, 20.0), (4.0, 20.0)])),
        ],
        Vec::new(),
    );
    let scene = Scene::read(&doc, 1, &mut plenty());
    assert!(scene.connector(2).is_none() && scene.connector(4).is_some());
    assert_eq!(scene.wires().len(), 1);
}

/// A route with a line jump on a leg (a `MoveTo` and points on the leg) is the legs of the route without the jump, as `hand::base_route` has them.
#[test]
fn the_legs_are_the_routes_as_the_hand_edit_reads_them() {
    let doc = page(vec![ours(line(1, &[(0.0, 0.0), (1.0, 0.0), (2.0, 0.0), (3.0, 0.0), (3.0, 4.0)]))], Vec::new());
    let scene = Scene::read(&doc, 1, &mut plenty());
    let legs = scene.connector(1).unwrap().legs;
    assert_eq!(
        legs,
        vec![(Point::new(0.0, 0.0), Point::new(3.0, 0.0)), (Point::new(3.0, 0.0), Point::new(3.0, 4.0))],
        "the points on a leg are dropped"
    );
    assert_matches(&scene, &doc, MAX_SCENE_SHAPES, "legs");
}

/// A page that is the same page costs nothing to put right: no pointer is walked and no shape read.
#[test]
fn a_page_that_has_not_changed_is_put_right_by_nothing() {
    let doc = page(row_of_boxes(1, 40), Vec::new());
    let mut scene = Scene::read(&doc, 1, &mut plenty());
    let first = scene.reads();
    assert_eq!(first.shapes, 40, "the first put right reads every shape");
    assert_eq!(first.whole, 1);
    let changed = scene.put_right(&doc, 1, &mut plenty());
    assert!(changed.obstacles.is_empty() && !changed.incomplete);
    assert_eq!(scene.reads(), first, "the same document: nothing walked, nothing read");
}

/// One move reads one shape and walks the page's pointers; a connector, a `Connect` row and the other shapes are not read.
#[test]
fn a_move_reads_the_one_shape_it_changed() {
    let mut doc = page(
        {
            let mut shapes = row_of_boxes(1, 40);
            shapes.push(ours(line(100, &[(0.0, 5.0), (4.0, 5.0)])));
            shapes
        },
        glue_both(100, 1, 2),
    );
    let mut scene = Scene::read(&doc, 1, &mut plenty());
    let before = scene.reads();
    let poses = crate::draw::read::POSES_READ.with(std::cell::Cell::get);
    shift(&mut doc, 7, 0.0, 3.0);
    scene.put_right(&doc, 1, &mut plenty());
    let after = scene.reads();
    assert_eq!(after.shapes - before.shapes, 1, "one shape read");
    assert_eq!(after.walked - before.walked, 41, "every top-level pointer walked once");
    assert_eq!(
        (after.lines - before.lines, after.connect_rows - before.connect_rows, after.route_rows - before.route_rows),
        (0, 0, 0),
        "no connector, no row"
    );
    assert_eq!(after.whole, before.whole, "not the page whole");
    // The readers' own count: a shape read is the poses of its box's four corners, and no other shape's.
    let poses = crate::draw::read::POSES_READ.with(std::cell::Cell::get) - poses;
    assert!(poses <= 8, "{poses} poses were read for one shape");
    assert_same(&scene, &Scene::read(&doc, 1, &mut plenty()), "a move");
}

/// What differs between two documents is read, whatever happened between: each kind of change on a small page leaves a scene equal to one read whole, and the oracle's.
#[test]
fn a_scene_put_right_equals_a_fresh_one_after_each_kind_of_change() {
    let doc = page(
        vec![
            placeable(1, (0.0, 0.0, 2.0, 1.0)),
            placeable(2, (8.0, 0.0, 2.0, 1.0)),
            boxed(3, (4.0, 0.0, 2.0, 1.0)),
            group(4, (0.0, 10.0, 8.0, 1.0), "8", vec![placeable(5, (0.0, 0.0, 1.0, 1.0)), placeable(6, (4.0, 0.0, 1.0, 1.0))]),
            ours(line(7, &[(2.0, 0.5), (8.0, 0.5)])),
        ],
        glue_both(7, 1, 2),
    );
    type Edit = (&'static str, fn(&mut diacraft_doc::Document));
    let edits: Vec<Edit> = vec![
        ("a move", |d| shift(d, 3, 0.5, 2.0)),
        ("a move of a member's group", |d| shift(d, 4, 1.0, 1.0)),
        ("a delete", |d| remove(d, 2)),
        ("an add", |d| add(d, placeable(8, (3.0, 3.0, 1.0, 1.0)))),
        ("a setting of ObjType", |d| set_number(d, 3, "ObjType", 1.0)),
        ("a width", |d| set_number(d, 1, "Width", 3.0)),
        ("a turn", |d| set_number(d, 1, "Angle", 0.7)),
        ("a permeable box", |d| set_number(d, 1, "ShapePermeableY", 1.0)),
        ("a route", |d| set_number(d, 7, "BeginY", 0.75)),
        ("the glue", |d| reglue(d, vec![glue(7, "BeginX", 3), glue(7, "EndY", 1)])),
        ("no glue", |d| reglue(d, Vec::new())),
        ("the page's clearance", |d| {
            let page = crate::draw::page_mut(d, 1).unwrap();
            let sheet = std::sync::Arc::make_mut(&mut page.sheet);
            sheet.cells.insert(CellName::new("LineToNodeX"), Cell::constant(Value::from_text("0.3")));
        }),
        ("a layer shown or hidden", |d| {
            add_layers(d, &["A"]);
            on_layers(d, 1, "0");
            show_layer(d, 0, false);
        }),
    ];
    for (what, edit) in edits {
        let mut scene = Scene::read(&doc, 1, &mut plenty());
        let mut next = doc.clone();
        edit(&mut next);
        scene.put_right(&next, 1, &mut plenty());
        assert_same(&scene, &Scene::read(&next, 1, &mut plenty()), what);
        assert_matches(&scene, &next, MAX_SCENE_SHAPES, what);
        // And back: an undo is the older document, whose pointers the scene may hold.
        scene.put_right(&doc, 1, &mut plenty());
        assert_same(&scene, &Scene::read(&doc, 1, &mut plenty()), &format!("{what}, undone"));
    }
}

/// A master's cell, a style's cell and the page's sheet are what every shape inherits from: a change of one reads the page whole, and what it reads is the new page.
#[test]
fn a_changed_master_style_or_page_sheet_reads_again_whole() {
    let base = || {
        DocBuilder::new()
            .master(
                7,
                MasterBuilder::new().shape(
                    ShapeBuilder::new(1).cell("Width", "2").cell("Height", "1").cell("LocPinX", "1").cell("LocPinY", "0.5").cell("ObjType", "4"),
                ),
            )
            .root(|r| r.cell("LineToNodeX", "0.125"))
    };
    let mut doc = page_in(
        base(),
        (1..=6).map(|k| ShapeBuilder::new(k).master(7).cell("PinX", &(f64::from(k) * 3.0).to_string()).cell("PinY", "1")).collect(),
        Vec::new(),
    );
    let mut scene = Scene::read(&doc, 1, &mut plenty());
    assert!(obstacle_ids(&scene).is_empty(), "the master says neither: no instance is an obstacle");
    let n = scene.reads().shapes;
    assert_eq!(n, 6);
    // A master's ObjType 4 becomes 1.
    let masters: Vec<_> = doc.masters.iter().cloned().collect();
    let mut master = (*masters[0]).clone();
    let mut top = (*master.shapes.get(0).unwrap().clone()).clone();
    std::sync::Arc::make_mut(&mut top.sheet).cells.insert(CellName::new("ObjType"), Cell::constant(Value::from_text("1")));
    master.shapes = std::iter::once(std::sync::Arc::new(top)).collect();
    doc.masters = std::iter::once(std::sync::Arc::new(master)).collect();
    scene.put_right(&doc, 1, &mut plenty());
    assert_eq!(scene.reads().shapes - n, 6, "every shape read again: they inherit from the master");
    assert_eq!(scene.reads().whole, 2);
    assert_eq!(obstacle_ids(&scene), vec![1, 2, 3, 4, 5, 6], "and they are obstacles now");
    assert_same(&scene, &Scene::read(&doc, 1, &mut plenty()), "a changed master");
    // A style: the root's cell, which every shape inherits where it holds none.
    let styles: Vec<_> = doc.styles.iter().cloned().collect();
    let mut root = (*styles[0]).clone();
    std::sync::Arc::make_mut(&mut root.sheet).cells.insert(CellName::new("LineToNodeX"), Cell::constant(Value::from_text("0.5")));
    doc.styles = std::iter::once(std::sync::Arc::new(root)).collect();
    let before = scene.reads().shapes;
    scene.put_right(&doc, 1, &mut plenty());
    assert_eq!(scene.reads().shapes - before, 6, "a changed style reads the page whole");
    assert_eq!(scene.settings().clearance, (0.5, 0.125), "and the page's clearance is the new one");
    // The page's own sheet.
    let page_mut = crate::draw::page_mut(&mut doc, 1).unwrap();
    std::sync::Arc::make_mut(&mut page_mut.sheet).cells.insert(CellName::new("LineToNodeY"), Cell::constant(Value::from_text("0.25")));
    let before = scene.reads().shapes;
    scene.put_right(&doc, 1, &mut plenty());
    assert_eq!(scene.reads().shapes - before, 6, "a changed page sheet reads the page whole");
    assert_eq!(scene.settings().clearance, (0.5, 0.25));
    assert_same(&scene, &Scene::read(&doc, 1, &mut plenty()), "a changed page sheet");
}

/// A glue changes what is an obstacle without a read of a shape: the shapes that read 0 are obstacles once a connector is glued to them, and the `Connect` rows are passed once.
#[test]
fn a_glue_makes_an_obstacle_with_no_read_of_a_shape() {
    let mut doc = page(
        {
            let mut shapes: Vec<ShapeBuilder> = (1..=20).map(|k| boxed(k, (f64::from(k) * 3.0, 0.0, 1.0, 1.0)).cell("ObjType", "0")).collect();
            shapes.push(ours(line(100, &[(0.0, 5.0), (4.0, 5.0)])));
            shapes
        },
        Vec::new(),
    );
    let mut scene = Scene::read(&doc, 1, &mut plenty());
    assert!(obstacle_ids(&scene).is_empty(), "no connector is glued to any of them");
    let before = scene.reads();
    reglue(&mut doc, glue_both(100, 4, 9));
    let changed = scene.put_right(&doc, 1, &mut plenty());
    let after = scene.reads();
    assert_eq!(obstacle_ids(&scene), vec![4, 9], "the two shapes a connector is glued to are obstacles now");
    assert_eq!(after.shapes, before.shapes, "no shape was read for it");
    assert_eq!(after.connect_rows - before.connect_rows, 4, "the Connect rows were passed once");
    assert!(changed.obstacles.is_empty(), "a glue is no change of a box: cause C asks the next time the shape changes");
    assert_same(&scene, &Scene::read(&doc, 1, &mut plenty()), "a glue");
    assert_matches(&scene, &doc, MAX_SCENE_SHAPES, "a glue");
    // And the glue taken away: a plain line is no connector, a connector of ours stays a wire.
    reglue(&mut doc, Vec::new());
    scene.put_right(&doc, 1, &mut plenty());
    assert!(obstacle_ids(&scene).is_empty());
    assert_same(&scene, &Scene::read(&doc, 1, &mut plenty()), "no glue");
}

/// A one-dimensional shape that is no connector (no glue, not ours) becomes one when a glue is made: it is read then, and not before.
#[test]
fn a_line_that_is_glued_becomes_a_wire_and_is_read_then() {
    let mut doc = page(vec![placeable(1, (0.0, 0.0, 1.0, 1.0)), line(2, &[(0.0, 5.0), (4.0, 5.0), (4.0, 8.0)])], Vec::new());
    let mut scene = Scene::read(&doc, 1, &mut plenty());
    assert_eq!((scene.reads().lines, scene.wires().len()), (0, 0));
    reglue(&mut doc, vec![glue(2, "EndX", 1)]);
    scene.put_right(&doc, 1, &mut plenty());
    assert_eq!((scene.reads().lines, scene.wires().len(), scene.wires().leg_count()), (1, 1, 2), "read when it became a connector");
    assert_eq!(scene.connector(2).unwrap().glue, [None, Some(1)]);
    assert_same(&scene, &Scene::read(&doc, 1, &mut plenty()), "a line glued");
    reglue(&mut doc, Vec::new());
    scene.put_right(&doc, 1, &mut plenty());
    assert_eq!((scene.wires().len(), scene.connector(2).unwrap().state), (0, LegsState::Unread), "and none again with the glue gone");
    assert_same(&scene, &Scene::read(&doc, 1, &mut plenty()), "a line unglued");
}

/// A page that is not there, and a page that is another, give a scene of nothing and of the other.
#[test]
fn a_page_that_is_gone_or_another_page_empties_the_scene() {
    let doc = page(row_of_boxes(1, 5), Vec::new());
    let mut scene = Scene::read(&doc, 1, &mut plenty());
    assert_eq!(scene.obstacles().len(), 5);
    let changed = scene.put_right(&doc, 7, &mut plenty());
    assert_eq!(scene.obstacles().len(), 0, "a page that is not there has no obstacle");
    assert_eq!(scene.dump().tops, 0);
    assert!(changed.obstacles.len() <= 5);
    let again = scene.put_right(&doc, 1, &mut plenty());
    assert_eq!(scene.obstacles().len(), 5, "and the page is read again when it is there");
    assert_eq!(again.obstacles.len(), 5, "every obstacle came");
    assert_eq!(scene.page(), 1);
}

/// A repeated `ID` is the first shape's: the scene is the first's whatever the edits, and is made again whole for as long as the repeat lasts.
#[test]
fn a_repeated_id_is_the_first_shapes_and_the_scene_is_always_right() {
    let mut doc = page(
        vec![
            placeable(1, (0.0, 0.0, 1.0, 1.0)),
            placeable(2, (3.0, 0.0, 1.0, 1.0)),
            placeable(2, (6.0, 0.0, 1.0, 1.0)),
            placeable(3, (9.0, 0.0, 1.0, 1.0)),
        ],
        Vec::new(),
    );
    let mut scene = Scene::read(&doc, 1, &mut plenty());
    assert_eq!(scene.obstacles().get(2).unwrap().rect, rect(3.0, 0.0, 4.0, 1.0), "the first shape with the ID");
    assert_eq!(obstacle_ids(&scene), vec![1, 2, 3]);
    assert_matches(&scene, &doc, MAX_SCENE_SHAPES, "a repeated ID");
    // An edit of a shape elsewhere.
    shift(&mut doc, 3, 0.0, 1.0);
    scene.put_right(&doc, 1, &mut plenty());
    assert_same(&scene, &Scene::read(&doc, 1, &mut plenty()), "an edit beside a repeated ID");
    assert_matches(&scene, &doc, MAX_SCENE_SHAPES, "an edit beside a repeated ID");
    // The repeat taken away: the second shape with the ID goes.
    let at = doc.page(1).unwrap().shapes.iter().rposition(|s| s.id == 2).unwrap();
    crate::draw::page_mut(&mut doc, 1).unwrap().shapes.remove(at).unwrap();
    scene.put_right(&doc, 1, &mut plenty());
    assert_same(&scene, &Scene::read(&doc, 1, &mut plenty()), "the repeat gone");
    assert_matches(&scene, &doc, MAX_SCENE_SHAPES, "the repeat gone");
}

/// The members of a group that holds 8 move between the scene's top-level shapes when it is grouped and ungrouped: the scene follows, with no member held twice.
#[test]
fn a_group_and_an_ungroup_move_obstacles_between_shapes() {
    let doc = page(
        vec![
            group(1, (0.0, 0.0, 8.0, 1.0), "8", vec![placeable(2, (0.0, 0.0, 1.0, 1.0)), placeable(3, (4.0, 0.0, 1.0, 1.0))]),
            placeable(4, (20.0, 0.0, 1.0, 1.0)),
        ],
        Vec::new(),
    );
    let mut scene = Scene::read(&doc, 1, &mut plenty());
    assert_eq!(obstacle_ids(&scene), vec![2, 3, 4]);
    // The group taken apart: its members are top-level shapes of the page, where the group was.
    let mut next = doc.clone();
    let members: Vec<_> = next.page(1).unwrap().shapes.get(0).unwrap().children.iter().cloned().collect();
    {
        let page = crate::draw::page_mut(&mut next, 1).unwrap();
        page.shapes.remove(0).unwrap();
        for (k, member) in members.into_iter().enumerate() {
            page.shapes.insert(k, member).unwrap();
        }
    }
    let reads = scene.reads();
    scene.put_right(&next, 1, &mut plenty());
    assert_same(&scene, &Scene::read(&next, 1, &mut plenty()), "a group taken apart");
    assert_eq!(scene.reads().shapes - reads.shapes, 2, "the two members are read as top-level shapes, and the other shape is not");
    assert_eq!(scene.reads().whole, reads.whole, "not the page whole");
}
