//! The scenes a session keeps: one for each page it has routed on, entered for the time of an edit, counted in the session's hold, let go with the drawing, and put right when an undo, a redo or
//! a failed command puts another document back.

use std::panic::AssertUnwindSafe;

use serde_json::json;

use super::build::{placeable, row_of_boxes};
use super::{assert_same, plenty};
use crate::Session;
use crate::draw::route_scene::{InUse, KeptScenes, Reads, Scene, with_scene};

fn put_right(doc: &diacraft_doc::Document, page: u32) -> crate::draw::route_scene::Changed {
    with_scene(page, |scene| scene.put_right(doc, page, &mut plenty()))
}

fn reads_of(page: u32) -> Reads {
    with_scene(page, |scene| scene.reads())
}

fn session() -> Session {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    s
}

fn drop_box(s: &mut Session, x: f64, y: f64) -> u32 {
    let made = s.execute("shape.drop", &json!({"master": "rectangle", "page": 1, "x": x, "y": y})).unwrap();
    u32::try_from(made["id"].as_u64().unwrap()).unwrap()
}

/// An entry that ends restores what was there, so a call that runs a call is not left with none; with none entered a call makes a scene of its own.
#[test]
fn entering_inside_an_entry_gives_the_outer_one_back_and_no_entry_makes_a_scene_for_the_call() {
    let doc = super::build::page(row_of_boxes(1, 6), Vec::new());
    let (outer, inner) = (KeptScenes::default(), KeptScenes::default());
    // Outside any entry: a scene for the call, kept by nobody.
    assert_eq!(put_right(&doc, 1).obstacles.len(), 6);
    assert_eq!(reads_of(1), Reads::default(), "the next call finds a scene that has read nothing");
    let first = InUse::enter(&outer);
    put_right(&doc, 1);
    assert_eq!((outer.len(), inner.len()), (1, 0));
    {
        let _second = InUse::enter(&inner);
        put_right(&doc, 1);
        assert_eq!((outer.len(), inner.len()), (1, 1), "the inner entry's scenes are the ones used");
    }
    assert_eq!(reads_of(1).whole, 1, "the outer entry's scene again, which read the page once");
    drop(first);
    assert_eq!(reads_of(1), Reads::default(), "and none when the last entry ends");
}

/// A session keeps a scene for each page between its edits: the second edit puts it right by what changed, and its bytes are in the session's hold.
#[test]
fn a_session_keeps_the_scene_between_edits_and_counts_its_bytes() {
    let mut s = session();
    let a = drop_box(&mut s, 2.0, 2.0);
    drop_box(&mut s, 6.0, 2.0);
    assert_eq!(s.route_scene_held(), 0, "nothing routes yet, so nothing is kept");
    s.edit("read the page", |doc| {
        put_right(doc, 1);
        Ok(())
    })
    .unwrap();
    let held = s.route_scene_held();
    assert!(held > 0, "a scene is kept, and charged");
    let first = s.edit("again", |doc| Ok(reads_of_after(doc))).unwrap();
    assert_eq!(first.whole, 1, "the second edit finds the scene and reads the page no more");
    s.execute("shape.move", &json!({"ids": [a], "dx": 0.5, "dy": 0.0})).unwrap();
    let after_move = s
        .edit("a move seen", |doc| {
            let changed = put_right(doc, 1);
            Ok((changed, reads_of(1)))
        })
        .unwrap();
    assert_eq!(after_move.1.shapes - first.shapes, 1, "the one shape that moved");
    assert_eq!(after_move.0.obstacles.iter().map(|c| c.id).collect::<Vec<_>>(), vec![a]);
    assert_eq!(s.route_scene_held(), held, "the same page, the same bytes");
    // Another page has a scene of its own.
    s.execute("page.add", &json!({})).unwrap();
    s.edit("the other page", |doc| {
        let other = doc.pages.iter().map(|p| p.id).find(|id| *id != 1).unwrap();
        put_right(doc, other);
        put_right(doc, 1);
        Ok(())
    })
    .unwrap();
    assert_eq!(s.kept_scenes(), 2, "one for each page");
    // A drawing that is replaced lets every scene go.
    s.execute("file.new", &json!({})).unwrap();
    assert_eq!(s.route_scene_held(), 0, "the scenes of a drawing that is gone are no use to the next");
    assert_eq!(s.kept_scenes(), 0);
}

fn reads_of_after(doc: &diacraft_doc::Document) -> Reads {
    put_right(doc, 1);
    reads_of(1)
}

/// The scene is the session's alone: two sessions editing keep their own.
#[test]
fn two_sessions_keep_scenes_of_their_own() {
    let (mut one, mut two) = (session(), session());
    drop_box(&mut one, 2.0, 2.0);
    one.edit("a", |doc| {
        put_right(doc, 1);
        Ok(())
    })
    .unwrap();
    assert!(one.route_scene_held() > 0 && two.route_scene_held() == 0);
    two.edit("b", |_| Ok(())).unwrap();
    assert_eq!(two.kept_scenes(), 0);
}

/// An undo and a redo put the scene right with the documents they bring, and **report nothing**: a shape put back by an undo is no shape that "came to cross" a leg at the edit after it.
#[test]
fn an_undo_and_a_redo_put_the_scene_right_and_the_next_edit_is_not_told_of_them() {
    let mut s = session();
    let a = drop_box(&mut s, 2.0, 2.0);
    drop_box(&mut s, 8.0, 2.0);
    // An edit that changes nothing is no step of the history, so that a redo is still there after it.
    let see = |s: &mut Session| s.edit_if("see", |doc| Ok((put_right(doc, 1), false))).unwrap();
    see(&mut s);
    // A move that the scene sees (as the routing of an edit puts it right).
    s.edit("move a", |doc| {
        crate::draw::move_shapes(doc, 1, &[a], 1.0, 0.0).map_err(|p| p.into_engine("test"))?;
        put_right(doc, 1);
        Ok(())
    })
    .unwrap();
    let held_box = |s: &Session, id: u32| {
        let doc = s.document().unwrap();
        crate::draw::shape_corners(doc, 1, id).unwrap()
    };
    let moved = held_box(&s, a);
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(held_box(&s, a) != moved, "the move is undone");
    let told = see(&mut s);
    assert!(
        told.obstacles.is_empty(),
        "the undo is put right by the session, and the next edit is not told that shape {a} came back: {:?}",
        told.obstacles
    );
    s.execute("edit.redo", &json!({})).unwrap();
    let told = see(&mut s);
    assert!(told.obstacles.is_empty(), "nor the redo: {:?}", told.obstacles);
    // The scene is the drawing's after each.
    s.edit("check", |doc| {
        let fresh = Scene::read(doc, 1, &mut plenty());
        with_scene(1, |scene| {
            scene.put_right(doc, 1, &mut plenty());
            assert_same(scene, &fresh, "after an undo and a redo");
        });
        Ok(())
    })
    .unwrap();
}

/// A command that fails is put back, and the scene with it: the next edit is not told of what the failed one did.
#[test]
fn a_command_that_failed_leaves_the_scene_the_drawings() {
    let mut s = session();
    let a = drop_box(&mut s, 2.0, 2.0);
    let seen = s.edit("see", |doc| Ok(put_right(doc, 1))).unwrap();
    assert_eq!(seen.obstacles.iter().map(|c| c.id).collect::<Vec<_>>(), vec![a], "a box dropped from the Basic Shapes stencil is an obstacle");
    // An edit that moves a shape, lets the scene see it, and then fails: the document is put back.
    let failed = s.edit("fails after the scene saw it", |doc| -> crate::error::Result<()> {
        crate::draw::move_shapes(doc, 1, &[a], 3.0, 0.0).map_err(|p| p.into_engine("test"))?;
        put_right(doc, 1);
        Err(crate::error::failed("test", "it fails"))
    });
    assert!(failed.is_err());
    let told = s.edit("after", |doc| Ok(put_right(doc, 1))).unwrap();
    assert!(told.obstacles.is_empty(), "the failed edit's move is no change: {:?}", told.obstacles);
}

/// A call that panics while it holds a scene leaves it not trusted: the scenes are let go, and the next call makes its own.
#[test]
fn a_scene_a_panic_left_is_not_trusted() {
    let doc = super::build::page(vec![placeable(1, (0.0, 0.0, 1.0, 1.0))], Vec::new());
    let kept = KeptScenes::default();
    let _in_use = InUse::enter(&kept);
    put_right(&doc, 1);
    assert_eq!(reads_of(1).whole, 1);
    let caught = std::panic::catch_unwind(AssertUnwindSafe(|| with_scene(1, |_| panic!("a bug in the middle of a put right"))));
    assert!(caught.is_err());
    assert_eq!(reads_of(1), Reads::default(), "the scene a panic left is gone: the next call has a scene that read nothing");
    put_right(&doc, 1);
    assert_eq!(reads_of(1).whole, 1, "and the scenes work again");
}

/// The scenes of a session are kept within a number of bytes: past it the page used longest ago is let go, and the scene in use never is.
#[test]
fn scenes_over_the_cap_let_the_page_used_longest_ago_go() {
    let doc = {
        let mut doc = super::build::page(row_of_boxes(1, 40), Vec::new());
        for id in [2, 3] {
            let mut page = (**doc.pages.get(0).unwrap()).clone();
            page.id = id;
            doc.pages.push(std::sync::Arc::new(page));
        }
        doc
    };
    let one = Scene::read(&doc, 1, &mut plenty()).held_bytes();
    assert!(one > 0);
    let kept = KeptScenes::with_cap(one * 5 / 2);
    let _in_use = InUse::enter(&kept);
    for page in [1, 2, 3] {
        put_right(&doc, page);
    }
    assert_eq!(kept.len(), 2, "two and a half scenes fit two");
    assert_eq!(kept.pages(), vec![2, 3], "page 1 was used longest ago");
    put_right(&doc, 2);
    put_right(&doc, 1);
    assert_eq!(kept.pages(), vec![1, 2], "page 3 is the one used longest ago now");
    // One scene alone over the cap is kept all the same: it is the one in use.
    let tiny = KeptScenes::with_cap(1);
    let _again = InUse::enter(&tiny);
    put_right(&doc, 1);
    assert_eq!(tiny.pages(), vec![1]);
    assert!(tiny.held_bytes() > 1);
}

/// A call inside a call finds the scenes held and is given a scene of its own, rather than waiting for itself.
#[test]
fn a_call_inside_a_call_gets_a_scene_of_its_own() {
    let doc = super::build::page(row_of_boxes(1, 4), Vec::new());
    let kept = KeptScenes::default();
    let _in_use = InUse::enter(&kept);
    let inner = with_scene(1, |outer| {
        outer.put_right(&doc, 1, &mut plenty());
        with_scene(1, |inner| {
            assert_eq!(inner.reads(), Reads::default(), "a scene that read nothing");
            inner.put_right(&doc, 1, &mut plenty()).obstacles.len()
        })
    });
    assert_eq!(inner, 4, "it works, and what it made is nobody's");
    assert_eq!(kept.len(), 1);
    assert_eq!(reads_of(1).whole, 1, "the outer scene is the one kept");
}
