//! What a scene reads at most, and that it says so once: top-level shapes and members, rows of routes, a route of more than a route, and `Connect` rows.

use std::sync::Arc;

use diacraft_doc::{Document, Shape};

use super::build::{add, glue, group, line, ours, page, placeable, remove, shift};
use super::oracle::assert_matches;
use super::{assert_same, plenty};
use crate::draw::route_scene::{Caps, LegsState, MAX_SCENE_LINES, MAX_SCENE_ROUTE_ROWS, MAX_SCENE_ROWS_EACH, MAX_SCENE_SHAPES, Scene};

fn small(shapes: usize, route_rows: usize, rows_each: usize, connect_rows: usize) -> Caps {
    Caps { shapes, lines: MAX_SCENE_LINES, route_rows, rows_each, connect_rows }
}

/// The same line with another `ID`, holding the very sheet (rows and all) of the first: how the model holds copies, so that 100 routes of 4,000 rows are one.
fn copy_of(doc: &Document, id: u32, new: u32) -> Arc<Shape> {
    let at = doc.page(1).unwrap().shapes.iter().find(|s| s.id == id).unwrap();
    Arc::new(Shape { id: new, ..Shape::clone(at) })
}

/// A route of `rows` rows along y = `y`: `rows` points, every one on the line.
fn long_route(id: u32, y: f64, rows: usize) -> diacraft_testkit::build::ShapeBuilder {
    let points: Vec<(f64, f64)> = (0..rows).map(|k| (k as f64 * 0.001, y)).collect();
    ours(line(id, &points))
}

/// A shape past the cap is no obstacle, in page order; the cut is said once, again after it was put right and arose again, and the scene is the one read whole at every edit.
#[test]
fn a_page_past_the_shape_cap_is_read_in_part_and_says_so_once() {
    let boxes = |n: u32| (1..=n).map(|k| placeable(k, (f64::from(k) * 3.0, 0.0, 1.0, 1.0))).collect::<Vec<_>>();
    let mut doc = page(boxes(8), Vec::new());
    let mut scene = Scene::with_caps(1, small(5, 400_000, 4_096, 400_000));
    scene.put_right(&doc, 1, &mut plenty());
    assert_eq!(scene.obstacles().len(), 5, "the first five in page order");
    assert!(scene.obstacles().get(5).is_some() && scene.obstacles().get(6).is_none());
    assert!(scene.cut().shapes);
    let said = scene.take_notice().expect("the cut is said the first time");
    assert!(said.shapes && said.words().contains("read in part"), "{}", said.words());
    assert_eq!(scene.take_notice(), None, "and once");
    // A move of a shape inside the cap: still cut, not said again.
    shift(&mut doc, 2, 0.0, 1.0);
    scene.put_right(&doc, 1, &mut plenty());
    assert!(scene.cut().shapes);
    assert_eq!(scene.take_notice(), None, "the second edit says nothing more of it");
    assert_same(&scene, &Scene::read_with(&doc, 1, small(5, 400_000, 4_096, 400_000)), "a move past the cap");
    // Delete three: the page fits, and the shapes that were past the cap are read.
    for id in [1, 2, 3] {
        remove(&mut doc, id);
    }
    scene.put_right(&doc, 1, &mut plenty());
    assert!(!scene.cut().any() && scene.obstacles().len() == 5, "the five that are left are read");
    assert_eq!(scene.take_notice(), None);
    assert_same(&scene, &Scene::read_with(&doc, 1, small(5, 400_000, 4_096, 400_000)), "the page fits");
    assert_matches(&scene, &doc, 5, "the page fits");
    // And past it again: said again.
    add(&mut doc, placeable(20, (60.0, 0.0, 1.0, 1.0)));
    scene.put_right(&doc, 1, &mut plenty());
    assert!(scene.cut().shapes && scene.take_notice().is_some(), "said again when it arose again");
}

/// The members of a group count against the cap as shapes, in page order, and a group read in part is read in part whatever moves before it: the scene is the one read whole at each step.
#[test]
fn the_shape_cap_moves_with_the_edits_and_a_scene_put_right_stays_the_one_read_whole() {
    let caps = small(9, 400_000, 4_096, 400_000);
    let members = |first: u32| {
        vec![placeable(first, (0.0, 0.0, 1.0, 1.0)), placeable(first + 1, (2.0, 0.0, 1.0, 1.0)), placeable(first + 2, (4.0, 0.0, 1.0, 1.0))]
    };
    let mut doc = page(
        vec![
            placeable(1, (0.0, 20.0, 1.0, 1.0)),
            group(2, (0.0, 0.0, 8.0, 1.0), "8", members(10)),
            placeable(3, (3.0, 20.0, 1.0, 1.0)),
            group(4, (0.0, 4.0, 8.0, 1.0), "8", members(20)),
            placeable(5, (6.0, 20.0, 1.0, 1.0)),
            group(6, (0.0, 8.0, 8.0, 1.0), "8", members(30)),
            placeable(7, (9.0, 20.0, 1.0, 1.0)),
        ],
        Vec::new(),
    );
    let mut scene = Scene::with_caps(1, caps);
    scene.put_right(&doc, 1, &mut plenty());
    assert_matches(&scene, &doc, 9, "the groups at the cap");
    assert!(scene.cut().shapes);
    // Each edit moves the boundary: a delete before it, a move of a group whose members are inside it, an add in front.
    remove(&mut doc, 1);
    scene.put_right(&doc, 1, &mut plenty());
    assert_same(&scene, &Scene::read_with(&doc, 1, caps), "a shape before the cap deleted");
    assert_matches(&scene, &doc, 9, "a shape before the cap deleted");
    remove(&mut doc, 3);
    scene.put_right(&doc, 1, &mut plenty());
    assert_same(&scene, &Scene::read_with(&doc, 1, caps), "another");
    assert_matches(&scene, &doc, 9, "another");
    shift(&mut doc, 4, 0.0, 1.0);
    scene.put_right(&doc, 1, &mut plenty());
    assert_same(&scene, &Scene::read_with(&doc, 1, caps), "a group moved");
    add(&mut doc, placeable(40, (12.0, 20.0, 1.0, 1.0)));
    scene.put_right(&doc, 1, &mut plenty());
    assert_same(&scene, &Scene::read_with(&doc, 1, caps), "a shape added");
    assert_matches(&scene, &doc, 9, "a shape added");
}

/// A page whose 10,001st top-level shape in page order is a placeable box across a connector's way: the scene did not read it, and says once that the page was read in part.
#[test]
fn the_10001st_shape_is_not_read_and_the_page_says_so_once() {
    let mut shapes: Vec<_> =
        (1..=MAX_SCENE_SHAPES as u32).map(|k| placeable(k, (f64::from(k % 100) * 3.0, f64::from(k / 100) * 3.0, 1.0, 1.0))).collect();
    shapes.push(placeable(50_000, (1.0, 1.0, 1.0, 1.0)));
    let mut doc = page(shapes, Vec::new());
    let mut scene = Scene::read(&doc, 1, &mut plenty());
    assert_eq!(scene.obstacles().len(), MAX_SCENE_SHAPES, "ten thousand obstacles");
    assert!(scene.obstacles().get(50_000).is_none(), "the one past the cap is not read");
    assert!(scene.candidate(MAX_SCENE_SHAPES as u32).is_some() && scene.candidate(50_000).is_none());
    assert_eq!(scene.reads().shapes, MAX_SCENE_SHAPES as u64, "and no more than ten thousand shapes were looked at");
    assert!(scene.take_notice().is_some_and(|c| c.shapes), "said");
    shift(&mut doc, 5, 0.0, 1.0);
    scene.put_right(&doc, 1, &mut plenty());
    assert_eq!(scene.take_notice(), None, "a second move says nothing more of it");
    // 10,000 shapes exactly are read in full and say nothing.
    remove(&mut doc, 50_000);
    scene.put_right(&doc, 1, &mut plenty());
    assert!(!scene.cut().any() && scene.take_notice().is_none(), "exactly the cap is no cut");
}

/// A route of 4,097 rows has no wire and is said; one of 4,096 is read. A page of routes past 400,000 rows is read as far as the rows go, in page order, and said.
#[test]
fn a_route_past_the_row_cap_has_no_wire_and_the_rows_of_the_page_are_capped() {
    let doc = page(vec![long_route(1, 0.0, MAX_SCENE_ROWS_EACH + 1), long_route(2, 1.0, MAX_SCENE_ROWS_EACH)], Vec::new());
    let mut scene = Scene::read(&doc, 1, &mut plenty());
    assert_eq!(scene.connector(1).unwrap().state, LegsState::Long, "4,097 rows are more than a route");
    assert!(scene.connector(1).unwrap().legs.is_empty());
    assert_eq!(scene.connector(2).unwrap().state, LegsState::Read, "4,096 rows are read");
    assert_eq!(scene.wires().len(), 1);
    assert!(scene.cut().long_routes && !scene.cut().route_rows);
    assert!(scene.take_notice().is_some_and(|c| c.long_routes));
    assert_eq!(scene.reads().route_rows, MAX_SCENE_ROWS_EACH as u64, "the long route was not read, and charged nothing");
    assert_matches(&scene, &doc, MAX_SCENE_SHAPES, "4,097 and 4,096 rows");
    // 100 routes of 4,000 rows are 400,000: all read. A 101st of two rows is past it.
    let mut doc = page(vec![long_route(1, 0.0, 4_000)], Vec::new());
    for k in 2..=100 {
        let copy = copy_of(&doc, 1, k);
        crate::draw::page_mut(&mut doc, 1).unwrap().shapes.push(copy);
    }
    assert_eq!(100 * 4_000, MAX_SCENE_ROUTE_ROWS);
    let mut scene = Scene::read(&doc, 1, &mut plenty());
    assert!(!scene.cut().any(), "exactly the cap is no cut");
    assert_eq!(scene.wires().len(), 100);
    add(&mut doc, ours(line(200, &[(0.0, 50.0), (4.0, 50.0)])));
    scene.put_right(&doc, 1, &mut plenty());
    assert_eq!(scene.connector(200).unwrap().state, LegsState::Cut, "400,002 rows");
    assert!(scene.cut().route_rows && scene.take_notice().is_some_and(|c| c.route_rows));
    assert_eq!(scene.wires().len(), 100, "the 101st has no wire");
    // Delete the first: the budget frees, and the last is read.
    remove(&mut doc, 1);
    scene.put_right(&doc, 1, &mut plenty());
    assert_eq!(scene.connector(200).unwrap().state, LegsState::Read, "the rows it needed are free now");
    assert!(!scene.cut().any());
    assert_eq!(scene.wires().len(), 100);
}

/// The rows of routes cut in page order, with caps small enough to see each step: every connector after the first that does not fit is cut, whatever it needs, and the scene is the one read whole.
#[test]
fn the_row_cap_cuts_in_page_order_and_the_scene_stays_the_one_read_whole() {
    let caps = small(10_000, 10, 6, 400_000);
    let route = |id: u32, rows: usize, y: f64| long_route(id, y, rows);
    let mut doc = page(vec![route(1, 4, 0.0), route(2, 4, 1.0), route(3, 4, 2.0), route(4, 2, 3.0), route(5, 7, 4.0)], Vec::new());
    let mut scene = Scene::with_caps(1, caps);
    scene.put_right(&doc, 1, &mut plenty());
    let states: Vec<LegsState> = (1..=5).map(|id| scene.connector(id).unwrap().state).collect();
    assert_eq!(
        states,
        vec![LegsState::Read, LegsState::Read, LegsState::Cut, LegsState::Cut, LegsState::Long],
        "8 of 10 rows read; the third needs 4; the fourth is after the cut; the fifth is long"
    );
    assert_same(&scene, &Scene::read_with(&doc, 1, caps), "the rows cut");
    remove(&mut doc, 1);
    scene.put_right(&doc, 1, &mut plenty());
    let states: Vec<LegsState> = (2..=5).map(|id| scene.connector(id).unwrap().state).collect();
    assert_eq!(states, vec![LegsState::Read, LegsState::Read, LegsState::Read, LegsState::Long], "the first is gone: 4 + 4 + 2 rows are 10");
    assert_same(&scene, &Scene::read_with(&doc, 1, caps), "a connector gone");
    add(&mut doc, route(9, 2, 9.0));
    scene.put_right(&doc, 1, &mut plenty());
    assert_eq!(scene.connector(9).unwrap().state, LegsState::Cut);
    assert_same(&scene, &Scene::read_with(&doc, 1, caps), "a connector past the rows");
}

/// `Connect` rows past the cap are not passed, and the glue past them is none: said once.
#[test]
fn connect_rows_past_the_cap_are_not_read_and_the_page_says_so() {
    let doc = page(
        vec![
            placeable(1, (0.0, 0.0, 1.0, 1.0)),
            placeable(2, (5.0, 0.0, 1.0, 1.0)),
            ours(line(10, &[(1.0, 0.5), (5.0, 0.5)])),
            ours(line(11, &[(1.0, 3.5), (5.0, 3.5)])),
        ],
        vec![glue(10, "BeginX", 1), glue(10, "EndX", 2), glue(11, "BeginX", 1), glue(11, "EndX", 2)],
    );
    let mut scene = Scene::with_caps(1, small(10_000, 400_000, 4_096, 3));
    scene.put_right(&doc, 1, &mut plenty());
    assert_eq!(scene.reads().connect_rows, 3, "three rows passed");
    assert_eq!(scene.connector(10).unwrap().glue, [Some(1), Some(2)]);
    assert_eq!(scene.connector(11).unwrap().glue, [Some(1), None], "the fourth row is past the cap");
    assert!(scene.cut().connects && scene.take_notice().is_some_and(|c| c.connects));
    assert_same(&scene, &Scene::read_with(&doc, 1, small(10_000, 400_000, 4_096, 3)), "connects cut");
}

/// A one-dimensional shape past the cap on them has no wire, in page order, and is said; two-dimensional shapes are not counted among them and the other way round.
#[test]
fn the_cap_on_connectors_is_separate_from_the_cap_on_shapes() {
    let caps = Caps { lines: 3, ..small(MAX_SCENE_SHAPES, 400_000, 4_096, 400_000) };
    let mut shapes = vec![placeable(1, (0.0, 0.0, 1.0, 1.0))];
    for k in 0..5u32 {
        shapes.push(ours(line(10 + k, &[(0.0, 5.0 + f64::from(k)), (4.0, 5.0 + f64::from(k))])));
        shapes.push(placeable(100 + k, (10.0 + f64::from(k) * 3.0, 0.0, 1.0, 1.0)));
    }
    let mut doc = page(shapes, Vec::new());
    let mut scene = Scene::with_caps(1, caps);
    scene.put_right(&doc, 1, &mut plenty());
    assert_eq!(scene.wires().len(), 3, "the first three connectors in page order");
    assert!(scene.connector(12).is_some() && scene.connector(13).is_none(), "the fourth is not looked at");
    assert_eq!(scene.obstacles().len(), 6, "every box: the connectors past their cap use none of the boxes'");
    assert!(scene.cut().lines && !scene.cut().shapes && scene.take_notice().is_some_and(|c| c.lines));
    assert_same(&scene, &Scene::read_with(&doc, 1, caps), "connectors cut");
    remove(&mut doc, 10);
    scene.put_right(&doc, 1, &mut plenty());
    assert_eq!(scene.wires().len(), 3, "the cap moves on: the fourth is read now");
    assert!(scene.connector(13).is_some());
    assert_same(&scene, &Scene::read_with(&doc, 1, caps), "a connector gone");
}

/// A shape past the cap that did not change costs a compare: a put right after an edit inside the cap peeks at no shape and reads one.
#[test]
fn a_shape_past_the_cap_that_did_not_change_is_not_looked_at_again() {
    let caps = small(5, 400_000, 4_096, 400_000);
    let mut doc = page((1..=8).map(|k| placeable(k, (f64::from(k) * 3.0, 0.0, 1.0, 1.0))).collect(), Vec::new());
    let mut scene = Scene::with_caps(1, caps);
    scene.put_right(&doc, 1, &mut plenty());
    assert_eq!((scene.reads().shapes, scene.reads().peeks), (5, 3), "five read, three told apart by their cells");
    shift(&mut doc, 2, 0.0, 1.0);
    let before = scene.reads();
    scene.put_right(&doc, 1, &mut plenty());
    assert_eq!(
        (scene.reads().shapes - before.shapes, scene.reads().peeks - before.peeks),
        (1, 0),
        "the three past the cap were compared and not looked at"
    );
    // A shape past the cap that changed is told apart again, and read no more.
    shift(&mut doc, 7, 0.0, 1.0);
    let before = scene.reads();
    scene.put_right(&doc, 1, &mut plenty());
    assert_eq!((scene.reads().shapes - before.shapes, scene.reads().peeks - before.peeks), (0, 1));
    assert_same(&scene, &Scene::read_with(&doc, 1, caps), "past the cap");
}

/// Numbers that are not boxes: a shape at 1e300, one of no size, one whose size is 1e-300 and a connector whose route is at 1e300 are left out by the router's own checks, counted, and
/// nothing panics.
#[test]
fn hostile_numbers_are_left_out_and_nothing_panics() {
    let doc = page(
        vec![
            placeable(1, (1.0e300, 1.0e300, 1.0, 1.0)),
            placeable(2, (0.0, 0.0, 1.0e-300, 1.0e-300)),
            placeable(3, (-1.0e7, 0.0, 1.0, 1.0)),
            placeable(4, (0.0, 0.0, 1.0, 1.0)),
            ours(line(5, &[(0.0, 1.0e300), (4.0, 1.0e300)])),
            ours(line(6, &[(0.0, 0.0), (1.0, 1.0)])),
            ours(line(7, &[(0.0, 5.0), (4.0, 5.0)])),
        ],
        Vec::new(),
    );
    let scene = Scene::read(&doc, 1, &mut plenty());
    assert!(scene.obstacles().get(4).is_some() && scene.obstacles().get(1).is_none() && scene.obstacles().get(3).is_none(), "a box past 1e6 is none");
    assert_eq!(scene.wires().len(), 3, "each connector is a wire, and the router keeps the legs it can");
    assert_eq!(scene.wires().leg_count(), 1, "the leg at 1e300 and the slanted leg are left out");
    assert_eq!(scene.wires().skipped_legs(), 2);
    assert_matches(&scene, &doc, MAX_SCENE_SHAPES, "hostile numbers");
}

/// A repeated `ID` among the members of groups, and between a member and a top-level shape: the first holds it, in the order the page and its groups have, and the scene is the one read whole.
#[test]
fn a_repeated_id_among_members_is_the_first_ones() {
    let mut doc = page(
        vec![
            group(1, (0.0, 0.0, 8.0, 1.0), "8", vec![placeable(10, (0.0, 0.0, 1.0, 1.0)), placeable(10, (3.0, 0.0, 1.0, 1.0))]),
            placeable(10, (20.0, 0.0, 1.0, 1.0)),
            placeable(11, (40.0, 0.0, 1.0, 1.0)),
        ],
        Vec::new(),
    );
    let mut scene = Scene::read(&doc, 1, &mut plenty());
    assert_eq!(scene.obstacles().get(10).unwrap().rect.x0, 0.0, "the first member holds the ID");
    assert_matches(&scene, &doc, MAX_SCENE_SHAPES, "a repeated member ID");
    shift(&mut doc, 11, 0.0, 1.0);
    scene.put_right(&doc, 1, &mut plenty());
    assert_same(&scene, &Scene::read(&doc, 1, &mut plenty()), "an edit beside the repeat");
    assert_matches(&scene, &doc, MAX_SCENE_SHAPES, "an edit beside the repeat");
}

/// A kept shape is valid within the budget that is left when it fits the room it took, and one whose members were cut when it has the very budget that cut it.
#[test]
fn a_kept_shape_is_valid_when_a_read_now_would_give_what_it_gave() {
    use crate::draw::route_scene::put::fits;
    assert!(fits(1, None, 1) && fits(1, None, 2) && !fits(1, None, 0), "a plain shape needs one");
    assert!(fits(4, None, 4) && fits(4, None, 9) && !fits(4, None, 3), "a group read whole needs what it took");
    assert!(fits(0, None, 0), "a shape of no cost fits nothing left");
    assert!(fits(7, Some(7), 7) && !fits(7, Some(7), 8) && !fits(7, Some(7), 6), "a group that was cut is valid only at the budget that cut it");
    assert!(!fits(u32::MAX, None, 5) && fits(u32::MAX, None, usize::MAX) && !fits(1, Some(u32::MAX), 5));
}
