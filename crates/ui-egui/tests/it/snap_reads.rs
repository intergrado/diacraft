//! What the window does while the page's reader has not answered yet, and when the drawing changes under it. The reader works on a thread of its
//! own, and a **gate in its hook orders it** (never time): the request `i` waits until the test lets it go, so a test holds the reader back for as many frames as it likes, edits the
//! drawing under it, makes it panic, or lets it answer, and counts what the window's thread did. Where the reader has not answered, a drag goes as the pointer made it and what is sent
//! carries no `snap`, so that **the release is what the preview showed**: a move, a resize, a turn and a drop each have a test of that here. (The reviewer's hook-gated tests of the
//! reader, taken in with the rest.)

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use diacraft_engine::Session;
use diacraft_ui_egui::DiacraftApp;
use diacraft_ui_egui::render_worker::bundled_drawer;
use diacraft_ui_egui::snap_reader::{Hook, SnapReader};
use diacraft_ui_egui::work;
use egui::{Key, Modifiers, Pos2, vec2};
use serde_json::json;

use crate::editing::{at, click_at, edit_window_with, key_without_waiting, shape_ids};
use crate::fixture;
use crate::guides::{bounds, near, page_id};
use crate::support::{Rest, TempDir, Window, button, fast_window, move_to, run_command};

/// Call `i` of the reader's hook waits until `allowed > i`; `panic_at` makes that call panic instead.
#[derive(Clone)]
struct Gate {
    allowed: Arc<(Mutex<usize>, Condvar)>,
    calls: Arc<AtomicUsize>,
}

impl Gate {
    fn new() -> Gate {
        Gate { allowed: Arc::new((Mutex::new(0), Condvar::new())), calls: Arc::new(AtomicUsize::new(0)) }
    }

    fn hook(&self, panic_at: Option<usize>) -> Hook {
        let (allowed, calls) = (Arc::clone(&self.allowed), Arc::clone(&self.calls));
        Arc::new(move || {
            let i = calls.fetch_add(1, Ordering::SeqCst);
            if panic_at == Some(i) {
                panic!("the reader's work broke");
            }
            let (lock, cv) = (&allowed.0, &allowed.1);
            let mut n = lock.lock().unwrap();
            while *n <= i {
                n = cv.wait(n).unwrap();
            }
        })
    }

    /// Lets the first `n` requests go.
    fn allow(&self, n: usize) {
        let (lock, cv) = (&self.allowed.0, &self.allowed.1);
        *lock.lock().unwrap() = n;
        cv.notify_all();
    }

    /// How many requests the thread has begun.
    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

/// A window on a new drawing whose reader waits at `gate`; the grid and the ruler are off, so that only the shapes pull.
fn window(gate: &Gate, panic_at: Option<usize>) -> Window {
    let hook = gate.hook(panic_at);
    let mut w = edit_window_with([1280.0, 900.0], move |app| app.with_snap_reader(SnapReader::threaded_with(hook)));
    run_command(&mut w, "snap.set", json!({"grid": false, "ruler": false}));
    w
}

/// A rectangle dropped by the command, centred at `(x, y)` exactly (no snap). → its `ID`.
fn drop_at(w: &mut Window, x: f64, y: f64) -> u32 {
    let page = page_id(w);
    let answer = exec(w, "shape.drop", json!({"master": "rectangle", "page": page, "x": x, "y": y}));
    u32::try_from(answer["id"].as_u64().unwrap()).unwrap()
}

fn exec(w: &mut Window, command: &str, params: serde_json::Value) -> serde_json::Value {
    let answer = w.state_mut().execute(command, &params).unwrap_or_else(|e| panic!("{command} {params}: {e}"));
    w.rest();
    answer
}

/// The middle of a shape's box, in inches.
fn middle(w: &Window, id: u32) -> (f64, f64) {
    let b = bounds(w, id);
    ((b.0 + b.2) / 2.0, (b.1 + b.3) / 2.0)
}

fn mv(w: &mut Window, x: f64, y: f64) {
    let p = at(w, x, y);
    move_to(w, p);
}

fn btn(w: &mut Window, x: f64, y: f64, down: bool) {
    let p = at(w, x, y);
    button(w, p, down);
}

/// Waits (bounded) until `cond` holds, stepping frames.
fn wait_for(w: &mut Window, what: &str, cond: impl Fn(&Window) -> bool) {
    for _ in 0..5_000 {
        if cond(w) {
            return;
        }
        w.step();
        std::thread::sleep(Duration::from_millis(1));
    }
    panic!("never: {what}");
}

/// A pointer still, three frames, then the button down at `from`.
fn press_at(w: &mut Window, from: Pos2) {
    for _ in 0..3 {
        move_to(w, from);
    }
    button(w, from, true);
}

// ---------------------------------------------------------------------------------------------------------------- a release before the reader answers is what the preview showed

/// A drop released before the reader answers lands where its ghost was, and is sent with no `snap` (of the review): the ghost went as the pointer made it, 0.03 inch under
/// where an alignment would have pulled it, so a drop sent with `snap` would have landed on the alignment and the ghost never was there.
#[test]
fn a_drop_released_before_the_reader_answers_lands_where_its_ghost_was_and_goes_without_snap() {
    let gate = Gate::new();
    let mut w = window(&gate, None);
    let _target = drop_at(&mut w, 7.0, 4.0);
    let before = shape_ids(&w);
    let tile = w.state().element("shapes.tile.Rectangle").unwrap().rect.center();
    let to = at(&w, 4.0, 3.97);
    move_to(&mut w, tile);
    button(&mut w, tile, true);
    for step in 1..=5 {
        move_to(&mut w, tile + (to - tile) * (step as f32 / 5.0));
    }
    let ghost = w.state().tile_preview().expect("the ghost is over the page");
    assert!(near(ghost.0, 4.0, 1e-4) && near(ghost.1, 3.97, 1e-4), "the ghost is where the pointer is: {ghost:?}");
    assert!(gate.calls() <= 1 && w.state().snap_reading(), "the reader is held");
    button(&mut w, to, false);
    w.step();
    w.step();
    let made = *shape_ids(&w).iter().find(|i| !before.contains(i)).expect("dropped");
    let m = middle(&w, made);
    assert!(near(m.0, ghost.0, 1e-9) && near(m.1, ghost.1, 1e-9), "the drop landed at {m:?}, the ghost showed {ghost:?}");
    assert_eq!(w.state().last_snap(), None, "sent without snap");
    gate.allow(100);
    wait_for(&mut w, "the reader answered", |w| !w.state().snap_reading());
}

/// **The drop is the ghost**: the button comes up a few points from where the ghost was last drawn (the pointer moved in the frame of the release), and the shape lands where the
/// ghost was, not where the pointer is. The reader has not answered, so the ghost went as the pointer made it.
#[test]
fn a_drop_released_a_little_off_its_ghost_lands_on_the_ghost() {
    let gate = Gate::new();
    let mut w = window(&gate, None);
    let before = shape_ids(&w);
    let tile = w.state().element("shapes.tile.Rectangle").unwrap().rect.center();
    let to = at(&w, 4.0, 3.97);
    move_to(&mut w, tile);
    button(&mut w, tile, true);
    for step in 1..=5 {
        move_to(&mut w, tile + (to - tile) * (step as f32 / 5.0));
    }
    let ghost = w.state().tile_preview().expect("the ghost is over the page");
    button(&mut w, to + vec2(6.0, 4.0), false);
    w.step();
    w.step();
    let made = *shape_ids(&w).iter().find(|i| !before.contains(i)).expect("dropped");
    let m = middle(&w, made);
    assert!(near(m.0, ghost.0, 1e-9) && near(m.1, ghost.1, 1e-9), "the drop landed at {m:?}, the ghost showed {ghost:?}");
    gate.allow(100);
    wait_for(&mut w, "the reader answered", |w| !w.state().snap_reading());
}

/// **M13 of the re-review**: the release is the ghost only when the ghost was worked out for the page the release is over. The page changes in the frame of the release (the command
/// is run with no frame after it, as a key that turns the page is, and the button comes up in the next frame), so the ghost on the screen is the other page's: the shape is dropped
/// on the page the pointer is over, where the pointer is, and not at the other page's ghost.
#[test]
fn a_drop_released_as_the_page_changes_lands_where_the_pointer_is_and_not_at_the_ghost_of_the_other_page() {
    let gate = Gate::new();
    let mut w = window(&gate, None);
    run_command(&mut w, "page.add", json!({}));
    let first = page_id(&w);
    // The second page has been on the screen (it has the view of its own that a release needs) and the first is current again.
    run_command(&mut w, "page.select", json!({"id": 2}));
    run_command(&mut w, "page.select", json!({"id": first}));
    let tile = w.state().element("shapes.tile.Rectangle").unwrap().rect.center();
    let to = at(&w, 4.0, 3.97);
    move_to(&mut w, tile);
    button(&mut w, tile, true);
    for step in 1..=5 {
        move_to(&mut w, tile + (to - tile) * (step as f32 / 5.0));
    }
    let ghost = w.state().tile_preview().expect("the ghost is over the first page");
    w.state_mut().execute("page.select", &json!({"id": 2})).unwrap();
    button(&mut w, to + vec2(60.0, 40.0), false);
    w.step();
    w.step();
    let doc = w.state().session().document().unwrap();
    assert!(doc.page(first).unwrap().shapes.is_empty(), "nothing was dropped on the first page, whose ghost it was");
    let made = doc.page(2).unwrap().shapes.iter().map(|s| s.id).next().expect("dropped on the page the pointer was over");
    let r = diacraft_engine::snap::box_of(doc, 2, &[made]).unwrap();
    let m = ((r.x0 + r.x1) / 2.0, (r.y0 + r.y1) / 2.0);
    assert!(!near(m.0, ghost.0, 1e-3) && !near(m.1, ghost.1, 1e-3), "the drop landed at {m:?}, where the other page's ghost was ({ghost:?})");
    assert!((m.0 - ghost.0).abs() < 1.5 && (m.1 - ghost.1).abs() < 1.5, "and near it, where the pointer was: {m:?}, the ghost {ghost:?}");
    gate.allow(100);
    wait_for(&mut w, "the reader answered", |w| !w.state().snap_reading());
}

/// A drop whose ghost was snapped (the reader had answered) is sent with `snap` and lands on the ghost, as before.
#[test]
fn a_drop_whose_ghost_was_snapped_is_sent_with_snap_and_lands_on_the_ghost() {
    let gate = Gate::new();
    gate.allow(100);
    let mut w = window(&gate, None);
    let _target = drop_at(&mut w, 7.0, 4.0);
    let before = shape_ids(&w);
    let tile = w.state().element("shapes.tile.Rectangle").unwrap().rect.center();
    let to = at(&w, 4.0, 3.97);
    move_to(&mut w, tile);
    button(&mut w, tile, true);
    for step in 1..=5 {
        move_to(&mut w, tile + (to - tile) * (step as f32 / 5.0));
    }
    wait_for(&mut w, "the reader answered", |w| !w.state().snap_reading());
    w.step();
    w.step();
    let ghost = w.state().tile_preview().expect("the ghost is over the page");
    assert!(near(ghost.1, 4.0, 1e-4), "pulled to the target's middle line: {ghost:?}");
    button(&mut w, to, false);
    w.rest();
    let made = *shape_ids(&w).iter().find(|i| !before.contains(i)).expect("dropped");
    let m = middle(&w, made);
    assert!(near(m.0, ghost.0, 1e-9) && near(m.1, ghost.1, 1e-9), "the drop landed at {m:?}, the ghost showed {ghost:?}");
    assert!(w.state().last_snap().is_some(), "sent with snap");
}

/// A resize released before the reader answers is what the preview showed, with no `snap` (the window keeps the proportions or the side itself): the east side dragged to 1.97
/// inches wide would come to 2 by another shape's width if it were snapped, and it does not.
#[test]
fn a_resize_released_before_the_reader_answers_is_the_preview_and_goes_without_snap() {
    let gate = Gate::new();
    let mut w = window(&gate, None);
    let page = page_id(&w);
    let id = drop_at(&mut w, 3.0, 4.0);
    let wide = drop_at(&mut w, 8.0, 6.0);
    exec(&mut w, "shape.resize", json!({"id": wide, "x": 7.0, "y": 5.5, "width": 2.0, "height": 1.0, "page": page}));
    click_at(&mut w, 3.0, 4.0);
    assert_eq!(w.state().selection(), [id]);
    let (from, to) = (at(&w, 3.75, 4.0), at(&w, 3.75 + 0.47, 4.0));
    press_at(&mut w, from);
    for step in 1..=5 {
        move_to(&mut w, from + (to - from) * (step as f32 / 5.0));
    }
    assert!(w.state().snap_reading(), "the reader is held");
    let preview = w.state().drag_preview().unwrap();
    assert!(!preview.snap && preview.marks.is_empty(), "the resize goes as the pointer made it: {preview:?}");
    let sized = preview.sized.unwrap();
    assert!(near(sized.2, 1.97, 1e-3), "not matched to the other shape's 2 inches: {sized:?}");
    button(&mut w, to, false);
    w.step();
    w.step();
    let after = bounds(&w, id);
    assert!(near(after.2 - after.0, sized.2, 1e-9) && near(after.3 - after.1, sized.3, 1e-9), "the release is the preview: {sized:?} {after:?}");
    assert_eq!(w.state().last_snap(), None, "sent without snap");
    gate.allow(100);
    wait_for(&mut w, "the reader answered", |w| !w.state().snap_reading());
}

/// A turn released before the reader answers is what the preview showed, with no `snap`: at 150 points from the pin the step is 5 degrees, so 34 degrees is the step, 35, where
/// another shape's 33 would have taken it were it snapped.
#[test]
fn a_turn_released_before_the_reader_answers_is_the_preview_and_goes_without_snap() {
    let gate = Gate::new();
    let mut w = window(&gate, None);
    let id = drop_at(&mut w, 4.0, 3.0);
    let other = drop_at(&mut w, 8.0, 6.0);
    let page = page_id(&w);
    exec(&mut w, "shape.rotate", json!({"ids": [other], "angle": 33.0, "page": page}));
    click_at(&mut w, 4.0, 3.0);
    assert_eq!(w.state().selection(), [id]);
    let radius = 150.0 / 96.0;
    let angle = (90.0_f64 + 34.0).to_radians();
    let pointer = (4.0 + radius * angle.cos(), 3.0 + radius * angle.sin());
    let (from, to) = (at(&w, 4.0, 3.875), at(&w, pointer.0, pointer.1));
    press_at(&mut w, from);
    for step in 1..=5 {
        move_to(&mut w, from + (to - from) * (step as f32 / 5.0));
    }
    assert!(w.state().snap_reading(), "the reader is held");
    let preview = w.state().drag_preview().unwrap();
    assert!(!preview.snap && preview.marks.is_empty(), "the turn goes as the pointer made it: {preview:?}");
    let turned = preview.turned.expect("the preview says where the shape comes to");
    assert!(near(turned, 35.0, 1e-6), "the step, not the other shape's angle: {turned}");
    button(&mut w, to, false);
    w.step();
    w.step();
    let degrees = diacraft_engine::draw::shape_frame(w.state().session().document().unwrap(), page_id(&w), id).unwrap().angle.to_degrees();
    assert!(near(degrees.rem_euclid(360.0), 35.0, 1e-6), "the release is the preview: {degrees}");
    assert_eq!(w.state().last_snap(), None, "sent without snap");
    gate.allow(100);
    wait_for(&mut w, "the reader answered", |w| !w.state().snap_reading());
}

/// The reader is asked for a page once, not once a frame: a drag held while it works makes one request however many frames go by (a request for each frame would only be
/// answered by the newest, but each would be work for the thread and an answer to take).
#[test]
fn a_drag_held_while_the_reader_works_asks_for_the_page_once() {
    let gate = Gate::new();
    let mut w = window(&gate, None);
    let id = drop_at(&mut w, 3.0, 4.0);
    let _b = drop_at(&mut w, 7.0, 4.53);
    let from = at(&w, 3.0, 4.0);
    for _ in 0..3 {
        move_to(&mut w, from);
    }
    work::reset();
    button(&mut w, from, true);
    for step in 1..=20 {
        mv(&mut w, 3.0, 4.0 + 0.01 * f64::from(step));
    }
    assert_eq!(w.state().selection(), [id]);
    assert!(w.state().snap_reading(), "the reader is held");
    assert_eq!(work::work().reads_asked, 1, "one request for the drag: {:?}", work::work());
    btn(&mut w, 3.0, 4.2, false);
    w.step();
    w.step();
    gate.allow(100);
    wait_for(&mut w, "the reader answered", |w| !w.state().snap_reading());
}

// ---------------------------------------------------------------------------------------------------------------- the drawing changes under the reader

/// An edit during a held drag: the reader's answer for the page before the edit is not taken, the drag goes unsnapped until the answer for the page after it, and then it snaps to
/// the shapes where the edit put them (never by the stale index); the release equals the preview.
#[test]
fn an_edit_during_a_held_drag_never_uses_the_stale_index() {
    let gate = Gate::new();
    let mut w = window(&gate, None);
    let page = page_id(&w);
    let a = drop_at(&mut w, 3.0, 4.0);
    let b = drop_at(&mut w, 7.0, 6.0);
    let from = at(&w, 3.0, 4.0);
    for _ in 0..3 {
        move_to(&mut w, from);
    }
    button(&mut w, from, true);
    mv(&mut w, 3.0, 4.2);
    mv(&mut w, 3.0, 4.3);
    wait_for(&mut w, "the first request is in the hook", |_| gate.calls() == 1);
    // The edit: B comes to where A will line up with it (top 5.03).
    w.state_mut().execute("shape.move", &json!({"ids": [b], "dx": 0.0, "dy": -1.47, "page": page})).unwrap();
    work::reset();
    mv(&mut w, 3.0, 4.45);
    mv(&mut w, 3.0, 4.5);
    let p = w.state().drag_preview().unwrap();
    assert!(!p.snap, "not ready: {p:?}");
    // The stale request finishes; its answer is for the page before the edit.
    gate.allow(1);
    wait_for(&mut w, "the second request is in the hook", |_| gate.calls() == 2);
    for _ in 0..5 {
        w.step();
    }
    let p = w.state().drag_preview().unwrap();
    let counted = work::work();
    assert!(!p.snap, "the answer for the page before the edit is not taken: {p:?}");
    assert_eq!((counted.snaps, counted.index_built_here, counted.targets_built_here), (0, 0, 0), "{counted:?}");
    gate.allow(2);
    wait_for(&mut w, "the reader answered", |w| !w.state().snap_reading());
    w.step();
    w.step();
    let p = w.state().drag_preview().unwrap();
    assert!(p.snap, "ready now: {p:?}");
    let delta = p.delta.unwrap();
    assert!((delta.1 - 0.53).abs() < 1e-6, "snapped to B where the edit put it: {delta:?} {:?}", p.marks);
    let before = middle(&w, a);
    btn(&mut w, 3.0, 4.5, false);
    w.rest();
    let after = middle(&w, a);
    assert!(((after.1 - before.1) - delta.1).abs() < 1e-9 && ((after.0 - before.0) - delta.0).abs() < 1e-9, "{before:?} {after:?} {delta:?}");
    let counted = work::work();
    assert_eq!((counted.index_built_here, counted.targets_built_here), (0, 0), "the release built nothing on this thread: {counted:?}");
}

/// An undo during a held drag takes B away from where A would line up with it: the drag never snaps to where B was.
#[test]
fn an_undo_during_a_held_drag_never_uses_the_stale_index() {
    let gate = Gate::new();
    let mut w = window(&gate, None);
    let page = page_id(&w);
    let a = drop_at(&mut w, 3.0, 4.0);
    let b = drop_at(&mut w, 7.0, 6.0);
    exec(&mut w, "shape.move", json!({"ids": [b], "dx": 0.0, "dy": -1.47, "page": page}));
    let from = at(&w, 3.0, 4.0);
    for _ in 0..3 {
        move_to(&mut w, from);
    }
    button(&mut w, from, true);
    mv(&mut w, 3.0, 4.3);
    mv(&mut w, 3.0, 4.5);
    wait_for(&mut w, "the first request is in the hook", |_| gate.calls() == 1);
    w.state_mut().execute("edit.undo", &json!({})).unwrap();
    mv(&mut w, 3.0, 4.49);
    mv(&mut w, 3.0, 4.5);
    gate.allow(1);
    wait_for(&mut w, "the second request is in the hook", |_| gate.calls() == 2);
    for _ in 0..5 {
        w.step();
    }
    assert!(!w.state().drag_preview().unwrap().snap, "the answer before the undo is not taken");
    gate.allow(2);
    wait_for(&mut w, "the reader answered", |w| !w.state().snap_reading());
    w.step();
    let p = w.state().drag_preview().unwrap();
    let delta = p.delta.unwrap();
    assert!((delta.1 - 0.5).abs() < 1e-6, "B is back where it was, nothing to line up with: {delta:?} {:?}", p.marks);
    let before = middle(&w, a);
    btn(&mut w, 3.0, 4.5, false);
    w.rest();
    let after = middle(&w, a);
    assert!(((after.1 - before.1) - delta.1).abs() < 1e-9, "{before:?} {after:?} {delta:?}");
}

/// A panic in the reader's work: the page is not snapped (the release is the preview, sent without `snap`), no frame waits, and an edit gives a page that is read again.
#[test]
fn a_panic_in_the_reader_leaves_the_drag_unsnapped_and_an_edit_reads_again() {
    let gate = Gate::new();
    gate.allow(100);
    let mut w = window(&gate, Some(0));
    let a = drop_at(&mut w, 3.0, 4.0);
    let _b = drop_at(&mut w, 7.0, 4.53);
    let from = at(&w, 3.0, 4.0);
    for _ in 0..3 {
        move_to(&mut w, from);
    }
    button(&mut w, from, true);
    mv(&mut w, 3.0, 4.3);
    mv(&mut w, 3.0, 4.5);
    wait_for(&mut w, "the reader answered", |w| !w.state().snap_reading() && gate.calls() >= 1);
    for _ in 0..3 {
        w.step();
    }
    mv(&mut w, 3.0, 4.501);
    mv(&mut w, 3.0, 4.5);
    let p = w.state().drag_preview().unwrap();
    assert!(!p.snap, "a page whose reading broke is not snapped: {p:?}");
    let before = middle(&w, a);
    btn(&mut w, 3.0, 4.5, false);
    w.rest();
    let after = middle(&w, a);
    assert!(((after.1 - before.1) - p.delta.unwrap().1).abs() < 1e-9, "the release is the preview");
    assert_eq!(w.state().last_snap(), None, "sent without snap");
    assert_eq!(w.state().ui_panics(), 0, "the window counts no panic of its own: the reader's net caught it");
    // The release was an edit: the page is read again, and the next drag snaps.
    let from = at(&w, after.0, after.1);
    for _ in 0..3 {
        move_to(&mut w, from);
    }
    button(&mut w, from, true);
    mv(&mut w, after.0, after.1 - 0.2);
    mv(&mut w, after.0, after.1 - 0.47);
    wait_for(&mut w, "the reader answered again", |w| !w.state().snap_reading());
    w.step();
    w.step();
    let p = w.state().drag_preview().unwrap();
    assert!(p.snap, "the next drag snaps: {p:?}");
    btn(&mut w, after.0, after.1 - 0.47, false);
    w.rest();
}

/// The newest request is the one made: a move, a drag of another shape and the move again, each begun while the first request is held; the last drag snaps once the reader
/// answers, and what is released is what was shown.
#[test]
fn the_newest_request_rule_does_not_strand_the_drag() {
    let gate = Gate::new();
    let mut w = window(&gate, None);
    let a = drop_at(&mut w, 3.0, 4.0);
    let _b = drop_at(&mut w, 7.0, 4.53);
    let c = drop_at(&mut w, 5.0, 7.0);
    // Drag 1: move A (held).
    let from = at(&w, 3.0, 4.0);
    for _ in 0..3 {
        move_to(&mut w, from);
    }
    button(&mut w, from, true);
    mv(&mut w, 3.0, 4.3);
    wait_for(&mut w, "held", |_| gate.calls() == 1);
    btn(&mut w, 3.0, 4.3, false);
    for _ in 0..2 {
        w.step();
    }
    // Drag 2: move C (another request, queued).
    let cm = middle(&w, c);
    let from = at(&w, cm.0, cm.1);
    for _ in 0..3 {
        move_to(&mut w, from);
    }
    button(&mut w, from, true);
    mv(&mut w, cm.0 + 0.3, cm.1);
    btn(&mut w, cm.0 + 0.3, cm.1, false);
    for _ in 0..2 {
        w.step();
    }
    // Drag 3: move A again, to where it lines up with B.
    let am = middle(&w, a);
    let from = at(&w, am.0, am.1);
    for _ in 0..3 {
        move_to(&mut w, from);
    }
    button(&mut w, from, true);
    mv(&mut w, am.0, am.1 + 0.2);
    mv(&mut w, am.0, 4.5);
    gate.allow(100);
    wait_for(&mut w, "the reader answered", |w| !w.state().snap_reading());
    w.step();
    w.step();
    let p = w.state().drag_preview().unwrap();
    assert!(p.snap, "the last drag is ready: {p:?}");
    let before = middle(&w, a);
    btn(&mut w, am.0, 4.5, false);
    w.rest();
    let after = middle(&w, a);
    assert!(((after.1 - before.1) - p.delta.unwrap().1).abs() < 1e-9, "the release is the preview");
}

/// The page changed under a held drag (a page added and shown by commands): nothing panics, and no frame is a snap on a page the reader was not asked about.
#[test]
fn the_page_changed_under_a_held_drag_snaps_nothing_stale() {
    let gate = Gate::new();
    let mut w = window(&gate, None);
    let _a = drop_at(&mut w, 3.0, 4.0);
    let _b = drop_at(&mut w, 7.0, 4.53);
    let from = at(&w, 3.0, 4.0);
    for _ in 0..3 {
        move_to(&mut w, from);
    }
    button(&mut w, from, true);
    mv(&mut w, 3.0, 4.3);
    mv(&mut w, 3.0, 4.5);
    wait_for(&mut w, "the first request is in the hook", |_| gate.calls() == 1);
    let added = w.state_mut().execute("page.add", &json!({})).unwrap();
    if let Some(id) = added["id"].as_u64().or_else(|| added["page"].as_u64()) {
        let _ = w.state_mut().execute("page.select", &json!({"page": id}));
    }
    for _ in 0..3 {
        w.step();
    }
    mv(&mut w, 3.0, 4.52);
    work::reset();
    gate.allow(10);
    wait_for(&mut w, "the reader answered", |w| !w.state().snap_reading());
    w.step();
    btn(&mut w, 3.0, 4.52, false);
    w.rest();
    assert_eq!(w.state().ui_panics(), 0);
    assert_eq!(work::work().index_built_here, 0, "no index was made on this thread");
}

// ---------------------------------------------------------------------------------------------------------------- a release and an arrow press on a ready page build nothing here

/// A window on a drawing of `n` small boxes (the crowded page of the review) at 100%, whose reader works on a thread of its own and is let go: the page it makes ready is the
/// session's, and the commands the window sends find it.
fn crowded_window(dir: &TempDir, n: u32) -> Window {
    let path = fixture::write(dir.path(), "drawing.vsdx", &fixture::crowded_vsdx(n));
    let mut session = Session::new();
    session.execute("file.open", &json!({"path": path.to_str().unwrap()})).unwrap();
    let app = DiacraftApp::new(session, None).with_inline_pictures(bundled_drawer()).with_snap_reader(SnapReader::threaded());
    let mut w = fast_window(app, [1280.0, 900.0]);
    w.rest();
    run_command(&mut w, "view.zoom", json!({"percent": 100}));
    w
}

/// Waits (bounded) until the reader has nothing it has not answered.
fn until_ready(w: &mut Window) {
    wait_for(w, "the reader answered", |w| !w.state().snap_reading());
}

/// The page of 10,000 shapes: a drag's release and an arrow key's press, once the reader has made the page ready, **build no index of the shapes and no set of targets on the
/// window's thread, where the command runs**: counted by the engine's own counts (per thread). The press that first meets the page goes as it always did; **after a press the
/// window asks for the page again** for the revision the press made, and the next press, which comes when the reader is done, builds nothing.
#[test]
fn a_release_and_an_arrow_press_on_a_ready_page_of_ten_thousand_shapes_build_nothing_on_the_windows_thread() {
    let dir = TempDir::new("ready-ten-thousand");
    let mut w = crowded_window(&dir, 10_000);
    // Shape 1054 is the 0.08 inch square with its lower left at (5.5, 1.2).
    let id = 1054;
    let from = at(&w, 5.54, 1.24);
    for _ in 0..3 {
        move_to(&mut w, from);
    }
    button(&mut w, from, true);
    move_to(&mut w, from + vec2(30.0, 0.0));
    until_ready(&mut w);
    move_to(&mut w, from + vec2(31.0, 1.0));
    assert_eq!(w.state().selection(), [id]);
    assert!(w.state().drag_preview().unwrap().snap, "the drag snaps");
    // The release: `shape.move` with snap, on a ready page.
    diacraft_engine::snap::reset_counts();
    button(&mut w, from + vec2(31.0, 1.0), false);
    let release = diacraft_engine::snap::counts();
    assert_eq!(
        (release.index_builds, release.target_builds, release.kept_found, release.kept_missing),
        (0, 0, 1, 0),
        "the release found the page: {release:?}"
    );
    assert!(w.state().last_snap().is_some(), "and it snapped");
    // The first press meets the page the release made stale: it asks for it, and goes as it did (the command shares what the reader makes, or makes it).
    w.rest();
    key_without_waiting(&mut w, Key::ArrowRight, Modifiers::NONE);
    until_ready(&mut w);
    w.step();
    // The next press finds it ready.
    diacraft_engine::snap::reset_counts();
    key_without_waiting(&mut w, Key::ArrowRight, Modifiers::NONE);
    let press = diacraft_engine::snap::counts();
    assert_eq!(
        (press.index_builds, press.target_builds, press.kept_found, press.kept_missing),
        (0, 0, 1, 0),
        "an arrow press on a ready page builds nothing on this thread: {press:?}"
    );
    until_ready(&mut w);
}

/// The reviewer's yardstick, kept (run with `--ignored`): a snapped release and an arrow press beside a Shift-arrow, which is a plain move, on crowded pages of 2,500 to
/// 10,000 shapes, **timed**: what the window's thread spends in the frame, and how long the reader works while frames go on. A test never asserts a time.
#[test]
#[ignore = "a measurement: it times, and a test never asserts a time"]
fn yardstick_the_frames_of_a_release_and_an_arrow_press_beside_a_plain_move_on_crowded_pages() {
    for n in [2_500u32, 5_000, 10_000] {
        let dir = TempDir::new("yardstick-frames");
        let mut w = crowded_window(&dir, n);
        let from = at(&w, 5.54, 1.24);
        for _ in 0..3 {
            move_to(&mut w, from);
        }
        button(&mut w, from, true);
        move_to(&mut w, from + vec2(30.0, 0.0));
        let t = Instant::now();
        until_ready(&mut w);
        let reader_ms = t.elapsed().as_secs_f64() * 1e3;
        move_to(&mut w, from + vec2(31.0, 0.0));
        let t = Instant::now();
        move_to(&mut w, from + vec2(32.0, 1.0));
        let drag_ms = t.elapsed().as_secs_f64() * 1e3;
        let t = Instant::now();
        button(&mut w, from + vec2(32.0, 1.0), false);
        let release_ms = t.elapsed().as_secs_f64() * 1e3;
        w.rest();
        let mut presses = Vec::new();
        for _ in 0..3 {
            let t = Instant::now();
            key_without_waiting(&mut w, Key::ArrowRight, Modifiers::NONE);
            presses.push(t.elapsed().as_secs_f64() * 1e3);
            until_ready(&mut w);
            w.step();
        }
        let mut plain = Vec::new();
        for _ in 0..3 {
            let t = Instant::now();
            key_without_waiting(&mut w, Key::ArrowRight, Modifiers::SHIFT);
            plain.push(t.elapsed().as_secs_f64() * 1e3);
            w.rest();
        }
        eprintln!(
            "YARDSTICK n={n}: the reader works {reader_ms:.0} ms while frames go on; a drag frame {drag_ms:.1} ms; the release frame {release_ms:.1} ms; arrow press frames {presses:.1?} ms; Shift+arrow (a plain move) frames {plain:.1?} ms"
        );
    }
}

/// The first arrow press on a page asks for it before it goes: with a reader that does its work where it is asked (the web's, and a test's by default), the page is ready when
/// the command runs, and the command **finds** it (it would have found none, and read the page itself, had the window not asked).
#[test]
fn the_first_arrow_press_makes_the_page_ready_before_it_goes_and_the_command_finds_it() {
    let mut w = edit_window_with([1280.0, 900.0], |app| app);
    run_command(&mut w, "snap.set", json!({"grid": false, "ruler": false}));
    let id = drop_at(&mut w, 3.0, 4.0);
    click_at(&mut w, 3.0, 4.0);
    assert_eq!(w.state().selection(), [id]);
    diacraft_engine::snap::reset_counts();
    work::reset();
    key_without_waiting(&mut w, Key::ArrowRight, Modifiers::NONE);
    let press = diacraft_engine::snap::counts();
    assert_eq!((press.kept_found, press.kept_missing), (1, 0), "the command found the page the window had made ready: {press:?}");
    assert_eq!(press.index_builds, 1, "made once, by the reader, where it was asked: {press:?}");
    // The window asked: the reader did its work inside the frame, once, before the command (and not again after it, where that would be work for a press that may not come).
    assert_eq!(work::work().reads_inline, 1, "{:?}", work::work());
}

// ---------------------------------------------------------------------------------------------------------------- one landing asks the page for two kinds of query

/// The Text tool's drag asks the page for **two kinds** of query in one landing (a place for the corner the press landed at, the moved sides of the box for the other), and a
/// reader on a thread answers each in its own time: a page that was ready only for the last kind it was made ready for would be asked again by every frame for the other, and the
/// preview would flip between snapped and not (the real app showed it: the size mark came and went, and a release got the box the frame before). The page keeps every kind it was made
/// ready for, so that once the reader is idle the preview is the snapped one in every frame, and the box made is the box shown.
#[test]
fn the_text_tools_drag_is_snapped_in_every_frame_once_the_reader_has_answered_both_its_kinds() {
    let gate = Gate::new();
    gate.allow(10_000);
    let mut w = window(&gate, None);
    // A shape 2 inches wide from (3.4, 4) to (5.4, 5.5); the Text tool dragged from (6, 4) to (7.97, 4.5): the box comes to 2 inches wide.
    let page = page_id(&w);
    let a = drop_at(&mut w, 4.4, 4.75);
    exec(&mut w, "shape.resize", json!({"id": a, "x": 3.4, "y": 4.0, "width": 2.0, "height": 1.5, "page": page}));
    run_command(&mut w, "tool.text", json!({}));
    let (from, to) = (at(&w, 6.0, 4.0), at(&w, 7.97, 4.5));
    press_at(&mut w, from);
    for step in 1..=5 {
        move_to(&mut w, from + (to - from) * (step as f32 / 5.0));
    }
    wait_for(&mut w, "the reader is idle", |w| !w.state().snap_reading());
    // Frames go on with the pointer still: every one of them shows the box snapped.
    let mut snapped = 0;
    for _ in 0..60 {
        w.step();
        std::thread::sleep(Duration::from_millis(2));
        let corners = w.state().drag_preview().and_then(|p| p.corners).expect("the drag's corners");
        assert!(near(corners.1.0, 8.0, 1e-9), "the box is 2 inches wide in every frame: {corners:?}");
        snapped += 1;
    }
    assert_eq!(snapped, 60);
    assert!(!w.state().snap_reading(), "and the reader is not asked again by each frame");
    button(&mut w, to, false);
    w.rest();
    let asked = work::work().reads_asked;
    assert!(asked <= 8, "a handful of requests, not one a frame: {asked}");
}

// ---------------------------------------------------------------------------------------------------------------- a page made ready for one drag and then another (of the re-review)

/// One drag of shape `id` that goes through the reader and is cancelled with Escape (no edit: the revision stays): the page is made ready for the shape that moves, the preview
/// snaps, and nothing is sent. → whether the preview snapped.
fn one_drag_cancelled(w: &mut Window, id: u32) -> bool {
    let (mx, my) = middle(w, id);
    let from = at(w, mx, my);
    press_at(w, from);
    mv(w, mx, my + 0.2);
    mv(w, mx, my + 0.3);
    wait_for(w, "the reader is idle", |w| !w.state().snap_reading());
    w.step();
    mv(w, mx, my + 0.31);
    let snapped = w.state().drag_preview().is_some_and(|p| p.snap);
    key_without_waiting(w, Key::Escape, Modifiers::NONE);
    btn(w, mx, my + 0.31, false);
    w.rest();
    snapped
}

/// **Of the re-review.** Three drags at one revision, none of which edits (each is cancelled): A, then B, then A again. The page was made ready for A and then for B, and the
/// engine holds **one** set of targets, the one it built last, for B's shapes: a window that still said the page was ready for A built A's set on its own thread in the first frame
/// of the third drag (14.4 ms on a page of 10,000 shapes). What was made ready for other shapes is not ready any more: the third drag asks the reader again, and no frame of it builds
/// an index or a set of targets here.
#[test]
fn a_third_drag_at_one_revision_builds_nothing_on_the_windows_thread() {
    let gate = Gate::new();
    gate.allow(10_000);
    let mut w = window(&gate, None);
    let a = drop_at(&mut w, 3.0, 4.0);
    let b = drop_at(&mut w, 7.0, 4.53);
    assert!(one_drag_cancelled(&mut w, a), "the first drag snaps");
    assert!(one_drag_cancelled(&mut w, b), "the second drag snaps");
    work::reset();
    let snapped = one_drag_cancelled(&mut w, a);
    let c = work::work();
    assert!(snapped, "the third drag snaps once the reader has answered");
    assert_eq!((c.index_built_here, c.targets_built_here), (0, 0), "the third drag built on the window's thread: {c:?}");
    assert!(c.reads_asked >= 1, "the third drag asked the reader again for the shape it was not made ready for any more: {c:?}");
}

// ---------------------------------------------------------------------------------------------------------------- the re-review's held cases, taken in

/// A move of A (3, 4) held before the reader answers, B at (7, 4.53): the preview is unsnapped (A's middle goes to 4.5), and the release is the preview, sent without `snap`. (The
/// re-review's case: the resize, the turn and the drop had tests of an early release, and the move, the commonest, was held only by the drags' own.)
#[test]
fn a_move_released_before_the_reader_answers_is_the_preview_and_goes_without_snap() {
    let gate = Gate::new();
    let mut w = window(&gate, None);
    let a = drop_at(&mut w, 3.0, 4.0);
    let _b = drop_at(&mut w, 7.0, 4.53);
    let from = at(&w, 3.0, 4.0);
    press_at(&mut w, from);
    mv(&mut w, 3.0, 4.2);
    mv(&mut w, 3.0, 4.4);
    mv(&mut w, 3.0, 4.5);
    assert!(w.state().snap_reading(), "the reader is held");
    let preview = w.state().drag_preview().unwrap();
    assert!(!preview.snap, "{preview:?}");
    let delta = preview.delta.unwrap();
    assert!(near(delta.1, 0.5, 1e-9), "{delta:?}");
    let before = middle(&w, a);
    btn(&mut w, 3.0, 4.5, false);
    w.step();
    w.step();
    let after = middle(&w, a);
    assert!(near(after.1 - before.1, 0.5, 1e-9) && near(after.0 - before.0, 0.0, 1e-9), "{before:?} {after:?}");
    assert_eq!(w.state().last_snap(), None, "sent without snap");
    gate.allow(1000);
    wait_for(&mut w, "the reader is idle", |w| !w.state().snap_reading());
}

/// The Text tool's drag, once the reader has answered both its kinds: no frame builds an index or a set of targets on the window's thread (the first held case of the re-review).
#[test]
fn the_text_tools_frames_build_nothing_on_the_windows_thread_once_the_reader_has_answered() {
    let gate = Gate::new();
    gate.allow(10_000);
    let mut w = window(&gate, None);
    let page = page_id(&w);
    let a = drop_at(&mut w, 4.4, 4.75);
    exec(&mut w, "shape.resize", json!({"id": a, "x": 3.4, "y": 4.0, "width": 2.0, "height": 1.5, "page": page}));
    run_command(&mut w, "tool.text", json!({}));
    let (from, to) = (at(&w, 6.0, 4.0), at(&w, 7.97, 4.5));
    press_at(&mut w, from);
    for step in 1..=5 {
        move_to(&mut w, from + (to - from) * (step as f32 / 5.0));
    }
    wait_for(&mut w, "the reader is idle", |w| !w.state().snap_reading());
    w.step();
    work::reset();
    for _ in 0..30 {
        w.step();
        std::thread::sleep(Duration::from_millis(1));
    }
    let c = work::work();
    assert_eq!((c.index_built_here, c.targets_built_here), (0, 0), "{c:?}");
    assert!(c.marks_painted >= 30, "and the frames did snap, and drew its marks: {c:?}");
    button(&mut w, to, false);
    w.rest();
}
