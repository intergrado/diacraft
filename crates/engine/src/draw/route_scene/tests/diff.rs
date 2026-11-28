//! What a put right reports (`Changed`): the obstacles that came, moved, were shown or hidden, or went, with the box the scene had settled on and the box it has; and that a setting is no
//! change (item 3).

use diacraft_doc::{Cell, CellName, Value};
use diacraft_route::{Rect, Work};

use super::build::{add, add_layers, boxed, glue_both, group, line, on_layers, ours, page, placeable, reglue, remove, set_number, shift, show_layer};
use super::{assert_same, plenty};
use crate::draw::route_scene::{ChangedObstacle, Scene};

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Rect {
    Rect { x0, y0, x1, y1 }
}

fn ids(changed: &[ChangedObstacle]) -> Vec<u32> {
    changed.iter().map(|c| c.id).collect()
}

#[test]
fn changed_gives_old_and_new_boxes() {
    let mut doc = page(
        vec![placeable(1, (0.0, 0.0, 1.0, 1.0)), placeable(2, (5.0, 0.0, 1.0, 1.0)), boxed(3, (9.0, 0.0, 1.0, 1.0)).cell("ObjType", "4")],
        Vec::new(),
    );
    let mut scene = Scene::new(1);
    let first = scene.put_right(&doc, 1, &mut plenty());
    assert_eq!(
        first.obstacles,
        vec![
            ChangedObstacle { id: 1, old: None, new: Some(rect(0.0, 0.0, 1.0, 1.0)) },
            ChangedObstacle { id: 2, old: None, new: Some(rect(5.0, 0.0, 6.0, 1.0)) }
        ],
        "the first put right says every obstacle came, in the order of the IDs, and not the shape that is none"
    );
    // A move: the old box and the new.
    shift(&mut doc, 1, 0.0, 3.0);
    let moved = scene.put_right(&doc, 1, &mut plenty());
    assert_eq!(moved.obstacles, vec![ChangedObstacle { id: 1, old: Some(rect(0.0, 0.0, 1.0, 1.0)), new: Some(rect(0.0, 3.0, 1.0, 4.0)) }]);
    // A move of a shape that is no obstacle says nothing; so do a shape that was not touched.
    shift(&mut doc, 3, 1.0, 0.0);
    assert!(scene.put_right(&doc, 1, &mut plenty()).obstacles.is_empty(), "a shape that is no obstacle moved");
    // A resize and a turn.
    set_number(&mut doc, 2, "Width", 2.0);
    let wider = scene.put_right(&doc, 1, &mut plenty());
    assert_eq!(
        wider.obstacles,
        vec![ChangedObstacle { id: 2, old: Some(rect(5.0, 0.0, 6.0, 1.0)), new: Some(rect(5.0, 0.0, 7.0, 1.0)) }],
        "LocPinX stays: it grows to the right"
    );
    // A new shape comes with no old box; a deleted one goes with no new.
    add(&mut doc, placeable(4, (20.0, 0.0, 1.0, 1.0)));
    remove(&mut doc, 1);
    let both = scene.put_right(&doc, 1, &mut plenty());
    assert_eq!(
        both.obstacles,
        vec![
            ChangedObstacle { id: 1, old: Some(rect(0.0, 3.0, 1.0, 4.0)), new: None },
            ChangedObstacle { id: 4, old: None, new: Some(rect(20.0, 0.0, 21.0, 1.0)) }
        ],
        "a shape that went has no new box and one that came has no old"
    );
    assert_same(&scene, &Scene::read(&doc, 1, &mut plenty()), "changed");
}

/// A layer that is hidden takes an obstacle out and one that is shown brings it back: each is a change of the box it is settled on ("shown by a layer").
#[test]
fn a_shape_shown_or_hidden_by_a_layer_is_reported() {
    let mut doc = page(vec![placeable(1, (0.0, 0.0, 1.0, 1.0)), placeable(2, (4.0, 0.0, 1.0, 1.0))], Vec::new());
    add_layers(&mut doc, &["A"]);
    on_layers(&mut doc, 1, "0");
    let mut scene = Scene::new(1);
    scene.put_right(&doc, 1, &mut plenty());
    show_layer(&mut doc, 0, false);
    let hidden = scene.put_right(&doc, 1, &mut plenty());
    assert_eq!(
        hidden.obstacles,
        vec![ChangedObstacle { id: 1, old: Some(rect(0.0, 0.0, 1.0, 1.0)), new: None }],
        "the page was read whole, and one shape differs"
    );
    assert_eq!(scene.obstacles().len(), 1);
    show_layer(&mut doc, 0, true);
    let shown = scene.put_right(&doc, 1, &mut plenty());
    assert_eq!(shown.obstacles, vec![ChangedObstacle { id: 1, old: None, new: Some(rect(0.0, 0.0, 1.0, 1.0)) }], "shown: it comes");
    assert_eq!(ids(&shown.obstacles), vec![1]);
}

/// Item 3: a shape made placeable by a setting asks nothing at once, **whatever put the scene right in between**; the move after it is the shape's first change, and
/// it is reported from the box the shape was settled on before the setting: none.
#[test]
fn a_setting_is_no_change_and_the_next_move_is_reported_from_before_it() {
    let mut doc = page(vec![boxed(1, (0.0, 0.0, 1.0, 1.0)).cell("ObjType", "4"), placeable(2, (6.0, 0.0, 1.0, 1.0))], Vec::new());
    let mut scene = Scene::new(1);
    scene.put_right(&doc, 1, &mut plenty());
    assert_eq!(scene.obstacles().len(), 1, "the shape that holds 4 is no obstacle");
    // `shape.set_behavior placement route_around`: ObjType 1.
    set_number(&mut doc, 1, "ObjType", 1.0);
    let set = scene.put_right(&doc, 1, &mut plenty());
    assert!(set.obstacles.is_empty(), "the setting asks nothing: {:?}", set.obstacles);
    assert_eq!(scene.obstacles().len(), 2, "but the shape is an obstacle now");
    // The scene is put right again by edits that have nothing to do with it, as many as there are.
    for k in 0..3 {
        shift(&mut doc, 2, 0.0, 1.0 + f64::from(k));
        let other = scene.put_right(&doc, 1, &mut plenty());
        assert_eq!(ids(&other.obstacles), vec![2], "only the shape that moved: {:?}", other.obstacles);
    }
    // The move: the old box is none.
    shift(&mut doc, 1, 0.25, 0.0);
    let moved = scene.put_right(&doc, 1, &mut plenty());
    assert_eq!(
        moved.obstacles,
        vec![ChangedObstacle { id: 1, old: None, new: Some(rect(0.25, 0.0, 1.25, 1.0)) }],
        "reported from the box before the setting: it had none"
    );
    // And once reported, the next move is from the box it has.
    shift(&mut doc, 1, 0.25, 0.0);
    let again = scene.put_right(&doc, 1, &mut plenty());
    assert_eq!(again.obstacles, vec![ChangedObstacle { id: 1, old: Some(rect(0.25, 0.0, 1.25, 1.0)), new: Some(rect(0.5, 0.0, 1.5, 1.0)) }]);
    // The other way: a shape that is an obstacle and is set to none stays settled on its box until it moves.
    set_number(&mut doc, 1, "ObjType", 4.0);
    assert!(scene.put_right(&doc, 1, &mut plenty()).obstacles.is_empty());
    assert_eq!(scene.obstacles().len(), 1);
    shift(&mut doc, 1, 0.0, 5.0);
    let away = scene.put_right(&doc, 1, &mut plenty());
    assert_eq!(away.obstacles, vec![ChangedObstacle { id: 1, old: Some(rect(0.5, 0.0, 1.5, 1.0)), new: None }]);
    assert_same(&scene, &Scene::read(&doc, 1, &mut plenty()), "settings");
}

/// A glue makes a shape that reads 0 placeable and is no change either: the write it makes "routes no connector", and the shape is reported with its next move.
#[test]
fn a_glue_is_no_change_and_the_next_move_is_reported_from_before_it() {
    let mut doc = page(
        vec![
            boxed(1, (0.0, 0.0, 1.0, 1.0)).cell("ObjType", "0"),
            boxed(2, (6.0, 0.0, 1.0, 1.0)).cell("ObjType", "0"),
            ours(line(9, &[(0.0, 5.0), (3.0, 5.0)])),
        ],
        Vec::new(),
    );
    let mut scene = Scene::new(1);
    scene.put_right(&doc, 1, &mut plenty());
    reglue(&mut doc, glue_both(9, 1, 2));
    let glued = scene.put_right(&doc, 1, &mut plenty());
    assert!(glued.obstacles.is_empty() && scene.obstacles().len() == 2, "an obstacle now, and no change");
    shift(&mut doc, 2, 0.0, 0.25);
    let moved = scene.put_right(&doc, 1, &mut plenty());
    assert_eq!(moved.obstacles, vec![ChangedObstacle { id: 2, old: None, new: Some(rect(6.0, 0.25, 7.0, 1.25)) }]);
}

/// The page's sheet changing reads every shape and reports none of them: nothing moved.
#[test]
fn a_page_read_whole_reports_only_what_differs() {
    let mut doc = page((1..=6).map(|k| placeable(k, (f64::from(k) * 3.0, 0.0, 1.0, 1.0))).collect(), Vec::new());
    let mut scene = Scene::new(1);
    scene.put_right(&doc, 1, &mut plenty());
    let page = crate::draw::page_mut(&mut doc, 1).unwrap();
    std::sync::Arc::make_mut(&mut page.sheet).cells.insert(CellName::new("LineToNodeX"), Cell::constant(Value::from_text("0.4")));
    let n = scene.reads().shapes;
    let changed = scene.put_right(&doc, 1, &mut plenty());
    assert_eq!(scene.reads().shapes - n, 6, "read whole");
    assert!(changed.obstacles.is_empty(), "and nothing is reported: {:?}", changed.obstacles);
}

/// Work that runs out in the middle of a put right leaves a scene that says it is not the drawing's, and the next put right, given work, reads the page whole and puts it right.
#[test]
fn work_that_runs_out_leaves_an_incomplete_scene_that_is_read_whole_next() {
    let mut doc = page((1..=40).map(|k| placeable(k, (f64::from(k) * 3.0, 0.0, 1.0, 1.0))).collect(), Vec::new());
    let mut scene = Scene::new(1);
    let mut short = Work::new(10);
    let changed = scene.put_right(&doc, 1, &mut short);
    assert!(changed.incomplete && !scene.is_complete(), "ten steps are not a page of forty shapes");
    assert!(short.spent() <= 10, "no more work than it was given");
    shift(&mut doc, 5, 0.0, 1.0);
    let changed = scene.put_right(&doc, 1, &mut plenty());
    assert!(!changed.incomplete && scene.is_complete());
    assert_same(&scene, &Scene::read(&doc, 1, &mut plenty()), "after work that ran out");
    // Work that is spent before it starts.
    let mut none = Work::new(0);
    shift(&mut doc, 6, 0.0, 1.0);
    assert!(scene.put_right(&doc, 1, &mut none).incomplete);
    assert!(!scene.is_complete());
    scene.put_right(&doc, 1, &mut plenty());
    assert_same(&scene, &Scene::read(&doc, 1, &mut plenty()), "after work that was spent");
}

/// A repeated `ID` that is met after a shape that moved was read, among the members of a group added in the same edit: the pass is made again whole, and the move is reported from the box the scene
/// was settled on before the edit (the retry used to start from the new box, and said nothing of the move).
#[test]
fn a_collision_met_after_a_move_still_reports_the_move() {
    let mut doc = page(vec![placeable(1, (0.0, 0.0, 1.0, 1.0)), placeable(2, (5.0, 0.0, 1.0, 1.0))], Vec::new());
    let mut scene = Scene::new(1);
    scene.put_right(&doc, 1, &mut plenty());
    shift(&mut doc, 1, 0.0, 3.0);
    // A group at the end of the page whose first member repeats the `ID` of shape 2, which is met when the group is read, after shape 1.
    add(&mut doc, group(3, (20.0, 0.0, 4.0, 4.0), "8", vec![placeable(2, (0.0, 0.0, 1.0, 1.0)), placeable(4, (2.0, 0.0, 1.0, 1.0))]));
    let changed = scene.put_right(&doc, 1, &mut plenty());
    assert!(!changed.incomplete);
    assert_eq!(
        changed.obstacles,
        vec![
            ChangedObstacle { id: 1, old: Some(rect(0.0, 0.0, 1.0, 1.0)), new: Some(rect(0.0, 3.0, 1.0, 4.0)) },
            ChangedObstacle { id: 4, old: None, new: Some(rect(22.0, 0.0, 23.0, 1.0)) },
        ],
        "shape 1 moved from the box the scene was settled on, shape 4 came, and the repeat of shape 2 is nothing"
    );
    assert_same(&scene, &Scene::read(&doc, 1, &mut plenty()), "a collision met after a move");
    // The repeat goes (the edit is undone): the scene is put right and the shape that came goes.
    let mut before = doc.clone();
    let at = before.page(1).unwrap().shapes.len() - 1;
    crate::draw::page_mut(&mut before, 1).unwrap().shapes.remove(at).unwrap();
    let gone = scene.put_right(&before, 1, &mut plenty());
    assert_eq!(gone.obstacles, vec![ChangedObstacle { id: 4, old: Some(rect(22.0, 0.0, 23.0, 1.0)), new: None }]);
}

/// Work that ran out in a put right does not take the boxes the scene was settled on away: the next one, given work, reports each shape that changed from the box it was settled on before,
/// whether the shape had been read again when the work ran out (it used to be settled on its new box, and its move was lost) or had not (its old box was lost, and it was said to come).
#[test]
fn work_that_runs_out_keeps_the_boxes_the_next_put_right_reports_from() {
    let mut doc = page((1..=6).map(|k| placeable(k, (f64::from(k) * 3.0, 0.0, 1.0, 1.0))).collect(), Vec::new());
    let mut scene = Scene::new(1);
    scene.put_right(&doc, 1, &mut plenty());
    shift(&mut doc, 1, 0.0, 2.0);
    shift(&mut doc, 4, 0.0, 2.0);
    // The six pointers, and the read of shape 1: shape 4's read is refused.
    let mut short = Work::new(7);
    let cut = scene.put_right(&doc, 1, &mut short);
    assert!(cut.incomplete && cut.obstacles.is_empty() && !scene.is_complete());
    // Another edit, and work that is spent in the walk.
    shift(&mut doc, 2, 0.0, 2.0);
    assert!(scene.put_right(&doc, 1, &mut Work::new(3)).incomplete);
    let all = scene.put_right(&doc, 1, &mut plenty());
    assert!(!all.incomplete && scene.is_complete());
    assert_eq!(
        all.obstacles,
        vec![
            ChangedObstacle { id: 1, old: Some(rect(3.0, 0.0, 4.0, 1.0)), new: Some(rect(3.0, 2.0, 4.0, 3.0)) },
            ChangedObstacle { id: 2, old: Some(rect(6.0, 0.0, 7.0, 1.0)), new: Some(rect(6.0, 2.0, 7.0, 3.0)) },
            ChangedObstacle { id: 4, old: Some(rect(12.0, 0.0, 13.0, 1.0)), new: Some(rect(12.0, 2.0, 13.0, 3.0)) },
        ],
        "each of the three moves, from the box before the first put right that ran out"
    );
    assert_same(&scene, &Scene::read(&doc, 1, &mut plenty()), "after work that ran out twice");
    // A shape that went and one that came while the work ran out.
    remove(&mut doc, 5);
    add(&mut doc, placeable(7, (40.0, 0.0, 1.0, 1.0)));
    // Six pointers walked, and the read of the new shape refused.
    assert!(scene.put_right(&doc, 1, &mut Work::new(6)).incomplete);
    let next = scene.put_right(&doc, 1, &mut plenty());
    assert_eq!(
        next.obstacles,
        vec![
            ChangedObstacle { id: 5, old: Some(rect(15.0, 0.0, 16.0, 1.0)), new: None },
            ChangedObstacle { id: 7, old: None, new: Some(rect(40.0, 0.0, 41.0, 1.0)) },
        ]
    );
}

/// The retry that a repeated `ID` asks for may run out of work too: the next put right still reports from the box the scene was settled on before the edit.
#[test]
fn a_retry_that_runs_out_of_work_keeps_the_boxes_too() {
    let mut doc = page(vec![placeable(1, (0.0, 0.0, 1.0, 1.0)), placeable(2, (5.0, 0.0, 1.0, 1.0))], Vec::new());
    let mut scene = Scene::new(1);
    scene.put_right(&doc, 1, &mut plenty());
    shift(&mut doc, 1, 0.0, 3.0);
    add(&mut doc, group(3, (20.0, 0.0, 4.0, 4.0), "8", vec![placeable(2, (0.0, 0.0, 1.0, 1.0)), placeable(4, (2.0, 0.0, 1.0, 1.0))]));
    // The first pass takes 7 steps (3 pointers, shape 1, the group and its two members) and meets the repeat; the retry has 5 left, and needs more.
    let mut short = Work::new(12);
    assert!(scene.put_right(&doc, 1, &mut short).incomplete);
    let done = scene.put_right(&doc, 1, &mut plenty());
    assert_eq!(
        done.obstacles,
        vec![
            ChangedObstacle { id: 1, old: Some(rect(0.0, 0.0, 1.0, 1.0)), new: Some(rect(0.0, 3.0, 1.0, 4.0)) },
            ChangedObstacle { id: 4, old: None, new: Some(rect(22.0, 0.0, 23.0, 1.0)) },
        ]
    );
    assert_same(&scene, &Scene::read(&doc, 1, &mut plenty()), "a retry that ran out of work");
}
