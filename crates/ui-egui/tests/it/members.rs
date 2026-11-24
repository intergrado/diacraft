//! The members of groups: what the scene reads of them, a second click and the press that starts a drag, `SelectMode`
//! (0 group only, 1 group first, 2 members first, a cell that is absent reads 1), and input that is awkward: groups nested past
//! the depth the scene reads, more members than it reads, an `ID` a top-level shape also has, and the members of a turned group that is not at
//! the origin.

use std::time::{Duration, Instant};

use diacraft_engine::Session;
use diacraft_ui_egui::DiacraftApp;
use diacraft_ui_egui::edit::scene::{Kind, MAX_GROUP_DEPTH, MAX_MEMBERS, Scene};
use diacraft_ui_egui::render_worker::bundled_drawer;
use egui::Modifiers;
use serde_json::json;

use crate::editing::{click_at, drag_with, frame_of};
use crate::fixture::{self, boxed, group, page_with, placed};
use crate::support::{Rest, TempDir, Window, fast_window, run_command};

/// A session that has read `bytes` as a file.
fn session_of(dir: &TempDir, bytes: &[u8]) -> Session {
    let path = fixture::write(dir.path(), "groups.vsdx", bytes);
    let mut session = Session::new();
    session.execute("file.open", &json!({"path": path.to_str().unwrap()})).unwrap();
    session
}

fn scene_of(session: &Session) -> Scene {
    let doc = session.document().unwrap();
    Scene::build(doc, doc.pages.get(0).unwrap().id, session.revision()).unwrap()
}

/// A window at 100% on the drawing `bytes`.
fn window_of(dir: &TempDir, bytes: &[u8]) -> Window {
    let mut w = fast_window(DiacraftApp::new(session_of(dir, bytes), None).with_inline_pictures(bundled_drawer()), [1280.0, 900.0]);
    w.rest();
    run_command(&mut w, "view.zoom", json!({"percent": 100}));
    w
}

/// Half a second of frames: the next click is not the second of a double click.
fn pause(w: &mut Window) {
    for _ in 0..30 {
        w.step();
    }
}

// ---- SelectMode -------------------------------------------------------------------------------------------------------------------------------

/// A group set to group only has no members in the scene, and none of the groups inside it has any either, whatever their own mode;
/// a group with no cell reads 1; a group set to members first is read as 2.
#[test]
fn a_group_set_to_group_only_offers_no_members_and_neither_do_the_groups_inside_it() {
    let dir = TempDir::new("select-mode");
    let session = session_of(&dir, &fixture::groups_vsdx());
    let scene = scene_of(&session);
    let mode = |id| scene.get(id).map(|item| item.select_mode);
    assert_eq!((mode(10), mode(20), mode(30), mode(40)), (Some(0), Some(1), Some(2), Some(1)));
    for id in [11, 12, 13] {
        assert!(scene.get(id).is_none(), "{id} is under a group set to group only");
    }
    for id in [21, 31, 32, 41, 42] {
        assert!(scene.get(id).is_some(), "{id} is a member of a group that can be entered");
    }
    assert_eq!(scene.children_of(10).count(), 0);
    assert_eq!(scene.items.len(), 4, "the four groups are what a click meets");
}

/// With the mouse: group only is never entered, group first is entered by a second click, members first by the first.
#[test]
fn a_click_enters_a_group_by_its_select_mode() {
    let dir = TempDir::new("select-mode-click");
    let mut w = window_of(&dir, &fixture::groups_vsdx());
    // Group only (10, with a group set to members first inside it, over a box): a second click picks nothing more.
    click_at(&mut w, 2.0, 6.0);
    assert_eq!(w.state().selection(), [10]);
    pause(&mut w);
    click_at(&mut w, 2.0, 6.0);
    assert_eq!(w.state().selection(), [10], "the member is unselectable, under an outer group set to 0");
    // Group first (20, no cell): the first click takes the group, the second the member.
    pause(&mut w);
    click_at(&mut w, 5.0, 6.0);
    assert_eq!(w.state().selection(), [20]);
    pause(&mut w);
    click_at(&mut w, 5.0, 6.0);
    assert_eq!(w.state().selection(), [21]);
    // Members first (30): the first click takes the member under the pointer, and one on its sibling the sibling.
    pause(&mut w);
    click_at(&mut w, 7.5, 6.0);
    assert_eq!(w.state().selection(), [31], "members first: the first click");
    pause(&mut w);
    click_at(&mut w, 8.5, 6.0);
    assert_eq!(w.state().selection(), [32]);
    assert_eq!(w.state().ui_panics(), 0);
}

// ---- a press on a selected group starts the move of the group ---------------------------------------------------------------------------------

/// Click a group, pause, drag it by a member: the group moves whole, and the selection stays the group; click, pause, click selects the member; a
/// member that is selected moves alone.
#[test]
fn a_drag_that_begins_on_a_member_of_the_selected_group_moves_the_group() {
    let dir = TempDir::new("member-drag");
    let mut w = window_of(&dir, &fixture::groups_vsdx());
    click_at(&mut w, 5.0, 6.0);
    assert_eq!(w.state().selection(), [20]);
    pause(&mut w);
    let (group_before, member_before) = (frame_of(&w, 20), frame_of(&w, 21));
    let revision = w.state().session().revision();
    // The press is on the box inside the group, which would be the member of a second click.
    drag_with(&mut w, (5.5, 6.0), (6.5, 6.0), Modifiers::NONE);
    let (group_after, member_after) = (frame_of(&w, 20), frame_of(&w, 21));
    assert!((group_after.0 - group_before.0 - 1.0).abs() < 1e-6, "the group moved a whole inch: {group_before:?} {group_after:?}");
    assert_eq!(member_after, member_before, "and the member did not leave it (its frame is the group's)");
    assert_eq!(w.state().selection(), [20], "the selection is still the group");
    assert_eq!(w.state().session().revision(), revision + 1, "one move");
    // A click, and not a drag, on the group now at (5..7, 5..7) selects the member.
    pause(&mut w);
    click_at(&mut w, 6.0, 6.0);
    assert_eq!(w.state().selection(), [21]);
    // The member, selected, is dragged alone.
    pause(&mut w);
    let group_before = frame_of(&w, 20);
    drag_with(&mut w, (6.0, 6.0), (6.0, 7.0), Modifiers::NONE);
    assert_eq!(frame_of(&w, 20), group_before, "the group stays");
    assert!((frame_of(&w, 21).1 - member_before.1 - 1.0).abs() < 1e-6, "the member went up an inch: {:?}", frame_of(&w, 21));
    assert_eq!(w.state().selection(), [21]);
}

/// A group set to members first gives the member the first press: dragging it moves that member alone.
#[test]
fn a_drag_on_a_group_of_members_first_moves_the_member() {
    let dir = TempDir::new("members-first-drag");
    let mut w = window_of(&dir, &fixture::groups_vsdx());
    let group_before = frame_of(&w, 30);
    drag_with(&mut w, (7.5, 6.0), (7.5, 7.0), Modifiers::NONE);
    assert_eq!(w.state().selection(), [31]);
    assert_eq!(frame_of(&w, 30), group_before);
    assert!((frame_of(&w, 31).1 - 1.0).abs() < 1e-6, "{:?}", frame_of(&w, 31));
}

// ---- awkward input ----------------------------------------------------------------------------------------------------------------------------

/// The members of a turned group that is not at the origin are where the page has them: carried through the group's turn about its pin.
#[test]
fn the_members_of_a_turned_and_offset_group_are_at_their_places_on_the_page() {
    let dir = TempDir::new("turned-group");
    let session = session_of(&dir, &fixture::groups_vsdx());
    let scene = scene_of(&session);
    let small = scene.get(42).expect("the small box is a member");
    assert_eq!(small.parent, Some(40));
    assert!(small.in_turned_group && !small.turned, "it is in a turned group and is not turned itself");
    assert!(!small.resizable() && !small.rotatable(), "the engine refuses to resize, turn or flip it");
    // The group is 2 by 1 with its pin in the middle at (6, 3), turned a quarter to the left: the lower left of its frame, (0, 0), lands at
    // (6.5, 2), and the box that is 0.5 square there lies from x 6 to 6.5 and y 2 to 2.5.
    let expected = [(6.5, 2.0), (6.5, 2.5), (6.0, 2.5), (6.0, 2.0)];
    for (got, want) in small.corners.iter().zip(expected) {
        assert!((got.0 - want.0).abs() < 1e-9 && (got.1 - want.1).abs() < 1e-9, "{:?} is not {expected:?}", small.corners);
    }
    // The group's own box is a vertical one: 1 wide, 2 tall, about (6, 3).
    let outer = scene.get(40).unwrap();
    assert!((outer.bounds.0.0 - 5.5).abs() < 1e-9 && (outer.bounds.1.0 - 6.5).abs() < 1e-9, "{:?}", outer.bounds);
    assert!((outer.bounds.0.1 - 2.0).abs() < 1e-9 && (outer.bounds.1.1 - 4.0).abs() < 1e-9, "{:?}", outer.bounds);
    assert_eq!(outer.kind, Kind::Group);
    // A click on the small box's place, after a click on the group, selects it.
    let mut w = window_of(&dir, &fixture::groups_vsdx());
    click_at(&mut w, 6.25, 2.25);
    assert_eq!(w.state().selection(), [40]);
    pause(&mut w);
    click_at(&mut w, 6.25, 2.25);
    assert_eq!(w.state().selection(), [42], "the second click reaches the member at its place on the page");
}

/// `n` groups nested one in the next, the outermost `first`, the innermost holding a box.
fn nested(first: u32, n: u32) -> String {
    let inner =
        (first + 1..first + n).rev().fold(boxed(first + n, 0.0, 0.0, 1.0, 1.0), |inside, id| group(id, &placed(0.0, 0.0, 1.0, 1.0), "", &inside));
    group(first, &placed(3.0, 3.0, 1.0, 1.0), "", &inner)
}

/// Groups nested past the depth the scene reads (eight levels of members) stop there, with no panic and no loop; the shapes above that are as
/// they are, and the click that would have reached further reaches the deepest member read.
#[test]
fn groups_nested_past_the_depth_the_scene_reads_stop_there() {
    let dir = TempDir::new("deep");
    // Twelve groups (101 to 112) and a box (113) in the innermost: the outermost is the top-level shape, the rest are 11 levels of members.
    let session = session_of(&dir, &page_with(&nested(101, 12)));
    let started = Instant::now();
    let scene = scene_of(&session);
    // A hang guard and no measure of cost: the depth the scene reads is counted below, and a minute is generous for a machine at load 50.
    assert!(started.elapsed() < Duration::from_secs(60), "{:?}", started.elapsed());
    assert_eq!(scene.items.len(), 1);
    let depth = u32::try_from(MAX_GROUP_DEPTH).unwrap();
    assert!(scene.get(101 + depth).is_some(), "a member at depth {depth} is read");
    assert!(scene.get(101 + depth + 1).is_none(), "one level further is not");
    assert_eq!(scene.members.len(), MAX_GROUP_DEPTH);
    assert_eq!(scene.root_of(101 + depth), Some(101), "and the chain of parents leads to the top");
    assert_eq!(scene.root_of(101 + depth + 1), None);
    // The window opens it and a click on it does not loop.
    let mut w = window_of(&dir, &page_with(&nested(101, 12)));
    click_at(&mut w, 3.5, 3.5);
    assert_eq!(w.state().selection(), [101]);
    for _ in 0..3 {
        pause(&mut w);
        click_at(&mut w, 3.5, 3.5);
    }
    assert_eq!(w.state().ui_panics(), 0);
    assert!(!w.state().selection().is_empty());
}

/// A group with more members than the scene reads: it reads `MAX_MEMBERS` of them, the first ones, and no more.
#[test]
fn a_group_of_more_members_than_the_scene_reads_gives_the_first_of_them() {
    let dir = TempDir::new("many");
    let n = MAX_MEMBERS + 100;
    let members: String = (0..n)
        .map(|i| {
            let (x, y) = (f64::from(u32::try_from(i % 100).unwrap()) * 0.05, f64::from(u32::try_from(i / 100 % 60).unwrap()) * 0.05);
            boxed(1000 + u32::try_from(i).unwrap(), x, y, 0.04, 0.04)
        })
        .collect();
    let session = session_of(&dir, &page_with(&group(1, &placed(1.0, 1.0, 5.0, 3.0), "", &members)));
    let started = Instant::now();
    let scene = scene_of(&session);
    // A hang guard and no measure of cost: the members the scene reads are counted below, and a minute is generous for a machine at load 50.
    assert!(started.elapsed() < Duration::from_secs(60), "{:?}", started.elapsed());
    assert_eq!(scene.items.len(), 1);
    assert_eq!(scene.members.len(), MAX_MEMBERS, "no more than the cap");
    assert!(scene.get(1000).is_some() && scene.get(1000 + u32::try_from(MAX_MEMBERS).unwrap() - 1).is_some(), "the first are read");
    assert!(scene.get(1000 + u32::try_from(MAX_MEMBERS).unwrap()).is_none(), "the rest are left to the commands");
    assert_eq!(scene.children_of(1).count(), MAX_MEMBERS);
}

/// A member whose `ID` a top-level shape has too is reached through the top-level one (`get`, `root_of`); the member is still in the group.
#[test]
fn a_member_with_the_id_of_a_top_level_shape_leaves_the_top_level_one_found() {
    let dir = TempDir::new("duplicate");
    let shapes = [
        boxed(5, 1.0, 1.0, 1.0, 1.0),
        group(6, &placed(4.0, 4.0, 2.0, 2.0), "", &[boxed(5, 0.0, 0.0, 1.0, 1.0), boxed(7, 1.0, 1.0, 1.0, 1.0)].concat()),
    ]
    .concat();
    let session = session_of(&dir, &page_with(&shapes));
    let scene = scene_of(&session);
    assert_eq!(scene.get(5).map(|item| item.parent), Some(None), "the top-level shape is the one found");
    assert_eq!(scene.root_of(5), Some(5));
    assert_eq!(scene.get(7).map(|item| item.parent), Some(Some(6)));
    let in_group: Vec<u32> = scene.children_of(6).map(|item| item.id).collect();
    assert_eq!(in_group, [5, 7], "the member is still one of the group's");
    assert_eq!(scene.hit_child(6, (4.5, 4.5), 0.01).map(|item| item.id), Some(5), "and a click in it finds it");
}
