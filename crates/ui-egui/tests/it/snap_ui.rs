//! The window sends what the engine's commands use, so that the preview of a drag is its release, and draws the marks the snap gives. Each test
//! drives the window with the pointer and the keys a person has, and checks the document the commands changed against the preview. The grid and the ruler are switched off
//! where they would pull as well as what the test is about; the other settings are a new drawing's. Every window has the manual clock, and the reader of pages works on the
//! test's thread (`support.rs`) unless a test gives it a thread of its own.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use diacraft_engine::Session;
use diacraft_engine::draw::{shape_corners, shape_frame};
use diacraft_engine::snap::Axis;
use diacraft_ui_egui::DiacraftApp;
use diacraft_ui_egui::edit::snapping::Mark;
use diacraft_ui_egui::render_worker::bundled_drawer;
use diacraft_ui_egui::snap_reader::SnapReader;
use diacraft_ui_egui::work;
use egui::{Key, Modifiers, Pos2, vec2};
use serde_json::json;

use crate::editing::{at, click_at, edit_window, hold, key, type_text};
use crate::fixture;
use crate::guides::{add_guide, bounds, held_to, near, page_id, press_and_drag};
use crate::support::{Rest, TempDir, Window, button, fast_window, move_to, run_command};

// ---------------------------------------------------------------------------------------------------------------- helpers

/// A shape of the stock stencil dropped by the command, centred at `(x, y)` exactly (no snap). → its `ID`.
fn drop_at(w: &mut Window, master: &str, x: f64, y: f64) -> u32 {
    let page = page_id(w);
    let answer = w.state_mut().execute("shape.drop", &json!({"master": master, "page": page, "x": x, "y": y})).unwrap();
    w.rest();
    u32::try_from(answer["id"].as_u64().unwrap()).unwrap()
}

/// The shape `id` turned to `degrees` about its pin, by the command.
fn turn(w: &mut Window, id: u32, degrees: f64) {
    let page = page_id(w);
    run_command(w, "shape.rotate", json!({"ids": [id], "angle": degrees, "page": page}));
}

/// The shape `id` flipped left to right, by the command.
fn flip(w: &mut Window, id: u32) {
    let page = page_id(w);
    run_command(w, "shape.flip", json!({"ids": [id], "axis": "horizontal", "page": page}));
}

/// The grid and the ruler off: only the other shapes, the guides and the margins pull.
fn no_grid(w: &mut Window) {
    run_command(w, "snap.set", json!({"grid": false, "ruler": false}));
}

fn corners(w: &Window, id: u32) -> [(f64, f64); 4] {
    shape_corners(w.state().session().document().unwrap(), page_id(w), id).unwrap()
}

/// The middle of a shape's box on the page (its pin, for the shapes of these tests).
fn pin_of(w: &Window, id: u32) -> (f64, f64) {
    let c = corners(w, id);
    ((c[0].0 + c[2].0) / 2.0, (c[0].1 + c[2].1) / 2.0)
}

/// The angle of a shape in degrees from just over -180 up to 180.
fn degrees_of(w: &Window, id: u32) -> f64 {
    let a = shape_frame(w.state().session().document().unwrap(), page_id(w), id).unwrap().angle.to_degrees().rem_euclid(360.0);
    if a > 180.0 { a - 360.0 } else { a }
}

/// Lets go of the button at `at` and lets the window rest.
fn release(w: &mut Window, at: Pos2) {
    button(w, at, false);
    w.rest();
}

fn at_p(w: &Window, p: (f64, f64)) -> Pos2 {
    at(w, p.0, p.1)
}

/// A pointer drag from the page place `from` to `to`, button still down.
fn drag_page(w: &mut Window, from: (f64, f64), to: (f64, f64)) -> (Pos2, Pos2) {
    let (a, b) = (at_p(w, from), at_p(w, to));
    press_and_drag(w, a, b);
    (a, b)
}

// ---------------------------------------------------------------------------------------------------------------- 1. a move through `snap_ext`, with the shape's own frame

/// A move of one shape turned by other than whole quarter turns lines up its own sides with the like sides of a shape turned alike, flipped or not, in
/// the preview as in the release: the two rectangles are turned `degrees`, and the one dragged ends 0.03 inch off the other's own sides, across them. The preview shows the
/// pull and the line along the shape's own direction, and the release is the preview to the bit. 90 degrees is a quarter turn, whose own sides are the page's: the control.
#[test]
fn a_move_of_a_turned_or_flipped_shape_lines_up_its_own_sides_where_the_release_does() {
    for (degrees, flipped) in [(30.0, false), (210.0, false), (30.0, true), (90.0, false)] {
        let mut w = edit_window([1280.0, 900.0]);
        no_grid(&mut w);
        let (end, e) = ((5.5, 4.0), 0.03);
        let start = (end.0 - 0.8, end.1 - 0.6);
        let dragged = drop_at(&mut w, "rectangle", start.0, start.1);
        let shape = |w: &mut Window, id: u32| {
            turn(w, id, degrees);
            if flipped {
                flip(w, id);
            }
        };
        shape(&mut w, dragged);
        // The way its sides run on the page, whatever the turn and the flip made of its angle: along its first edge.
        let c = corners(&w, dragged);
        let len = (c[1].0 - c[0].0).hypot(c[1].1 - c[0].1);
        let u = ((c[1].0 - c[0].0) / len, (c[1].1 - c[0].1) / len);
        let n = (-u.1, u.0);
        // The other shape is 3 inches along the dragged one's sides, and 0.03 inch off across them.
        let other = (end.0 + 3.0 * u.0 - e * n.0, end.1 + 3.0 * u.1 - e * n.1);
        let fixed = drop_at(&mut w, "rectangle", other.0, other.1);
        shape(&mut w, fixed);
        let (_, to) = drag_page(&mut w, start, end);
        let preview = w.state().drag_preview().unwrap();
        let tag = format!("{degrees} degrees, flipped {flipped}");
        assert!(preview.snap, "{tag}: the release is sent with snap: {preview:?}");
        let delta = preview.delta.unwrap();
        let pulled = ((delta.0 - (end.0 - start.0)).hypot(delta.1 - (end.1 - start.1)), delta);
        assert!((0.02..0.04).contains(&pulled.0), "{tag}: the own sides pull it by the 0.03 they are off: {pulled:?} {:?}", preview.marks);
        let own_line = |m: &Mark| matches!(m, Mark::Line { normal: Some(_), .. });
        let page_line = |m: &Mark| matches!(m, Mark::Line { normal: None, axis: Axis::X, .. });
        if degrees == 90.0 {
            assert!(preview.marks.iter().any(page_line), "{tag}: a quarter turn's sides are the page's: {:?}", preview.marks);
        } else {
            assert!(preview.marks.iter().any(own_line), "{tag}: a line along the shape's own side is drawn: {:?}", preview.marks);
        }
        release(&mut w, to);
        let (after, there) = (pin_of(&w, dragged), pin_of(&w, fixed));
        let off = (after.0 - there.0) * n.0 + (after.1 - there.1) * n.1;
        assert!(off.abs() < 1e-6, "{tag}: the own sides are on the other's: {off}");
        let moved = (after.0 - start.0, after.1 - start.1);
        assert!(near(moved.0, delta.0, 1e-9) && near(moved.1, delta.1, 1e-9), "{tag}: the release is the preview: {moved:?} {delta:?}");
        assert!(w.state().last_snap().is_some(), "{tag}: it was sent with snap");
    }
}

// ---------------------------------------------------------------------------------------------------------------- 2. a turned or flipped resize through `snap_resize_ext`

/// A turned shape's resize is snapped along its own axes: the east handle dragged an inch along the shape's own x axis comes to a guide 0.03 inch
/// farther, by the corner of its width-height box that lies farthest right; the angle is unchanged; and the preview's box is the one the release makes.
#[test]
fn a_resize_of_a_turned_shape_comes_to_a_guide_by_its_corner_and_the_preview_is_the_release() {
    let mut w = edit_window([1280.0, 900.0]);
    no_grid(&mut w);
    let id = drop_at(&mut w, "rectangle", 4.0, 4.0);
    turn(&mut w, id, 30.0);
    let (s, c) = f64::to_radians(30.0).sin_cos();
    let (u, n) = ((c, s), (-s, c));
    // The corner at the shape's own (0, 0), which stays; the corner at (width, 0) lies farthest right.
    let origin = (4.0 - 0.75 * u.0 - 0.5 * n.0, 4.0 - 0.75 * u.1 - 0.5 * n.1);
    let guide_x = origin.0 + 2.5 * u.0 + 0.03;
    add_guide(&mut w, "vertical", guide_x);
    click_at(&mut w, 4.0, 4.0);
    assert_eq!(w.state().selection(), [id]);
    let handle = (4.0 + 0.75 * u.0, 4.0 + 0.75 * u.1);
    let target = (4.0 + 1.75 * u.0, 4.0 + 1.75 * u.1);
    let (_, to) = drag_page(&mut w, handle, target);
    let preview = w.state().drag_preview().unwrap();
    assert!(preview.snap, "a turned shape's resize is sent with snap: {preview:?}");
    let sized = preview.sized.unwrap();
    assert!(!near(sized.2, 2.5, 1e-3), "the box is not the one the handle made: {sized:?}");
    release(&mut w, to);
    let after = corners(&w, id);
    let right = after.iter().map(|p| p.0).fold(f64::MIN, f64::max);
    assert!(near(right, guide_x, 1e-9), "the corner is on the guide: {after:?}");
    let (width, height) =
        (f64::hypot(after[1].0 - after[0].0, after[1].1 - after[0].1), f64::hypot(after[3].0 - after[0].0, after[3].1 - after[0].1));
    assert!(
        near(after[0].0, sized.0, 1e-9) && near(after[0].1, sized.1, 1e-9) && near(width, sized.2, 1e-9) && near(height, sized.3, 1e-9),
        "{sized:?} {after:?}"
    );
    assert!(near(height, 1.0, 1e-9), "the other axis is as it was: {height}");
    assert!(near(degrees_of(&w, id), 30.0, 1e-9), "the angle is unchanged: {}", degrees_of(&w, id));
}

/// A flipped shape's handles are the sides they are on: the middle handle on the page's left side of a shape flipped left to right is the one the shape's own x axis ends
/// at, and dragged left it comes to a guide 0.03 inch farther (the window sent no `snap` for such a shape before the preview called `snap_resize_ext`).
#[test]
fn a_resize_of_a_flipped_shape_comes_to_a_guide_by_the_side_it_is_on_and_the_preview_is_the_release() {
    let mut w = edit_window([1280.0, 900.0]);
    no_grid(&mut w);
    let id = drop_at(&mut w, "rectangle", 7.0, 4.0);
    flip(&mut w, id);
    add_guide(&mut w, "vertical", 5.28);
    click_at(&mut w, 7.0, 4.0);
    assert_eq!(w.state().selection(), [id]);
    let (_, to) = drag_page(&mut w, (6.25, 4.0), (5.25, 4.0));
    let preview = w.state().drag_preview().unwrap();
    assert!(preview.snap, "a flipped shape's resize is sent with snap: {preview:?}");
    let sized = preview.sized.unwrap();
    release(&mut w, to);
    let after = bounds(&w, id);
    assert!(near(after.0, 5.28, 1e-9) && near(after.2, 7.75, 1e-9), "the left side is on the guide, the right side stays: {after:?}");
    let c = corners(&w, id);
    let (width, height) = (f64::hypot(c[1].0 - c[0].0, c[1].1 - c[0].1), f64::hypot(c[3].0 - c[0].0, c[3].1 - c[0].1));
    assert!(
        near(c[0].0, sized.0, 1e-9) && near(c[0].1, sized.1, 1e-9) && near(width, sized.2, 1e-9) && near(height, sized.3, 1e-9),
        "{sized:?} {c:?}"
    );
}

// ---------------------------------------------------------------------------------------------------------------- `keep` on a resize

/// A corner keeps the shape's proportions with no key, and a side only with Shift: the engine does it from the box the handle made, and the release is the preview.
/// A side without Shift changes one axis only; with Shift the other axis grows about its middle.
#[test]
fn a_corner_keeps_the_proportions_with_no_key_and_a_side_only_with_shift() {
    // The north-east corner, no key: 1.5 to 1 kept.
    let mut w = edit_window([1280.0, 900.0]);
    no_grid(&mut w);
    let id = drop_at(&mut w, "rectangle", 3.0, 4.0);
    click_at(&mut w, 3.0, 4.0);
    let (_, to) = drag_page(&mut w, (3.75, 4.5), (4.9, 5.37));
    let preview = w.state().drag_preview().unwrap();
    assert!(preview.snap, "{preview:?}");
    let sized = preview.sized.unwrap();
    release(&mut w, to);
    let after = bounds(&w, id);
    let (width, height) = (after.2 - after.0, after.3 - after.1);
    assert!(near(width / height, 1.5, 1e-6) && near(after.0, 2.25, 1e-9) && near(after.1, 3.5, 1e-9), "{after:?}");
    assert!(near(sized.2, width, 1e-9) && near(sized.3, height, 1e-9), "the release is the preview: {sized:?} {after:?}");
    // The east side, no key: the width only.
    let id = drop_at(&mut w, "rectangle", 3.0, 7.0);
    click_at(&mut w, 3.0, 7.0);
    let (_, to) = drag_page(&mut w, (3.75, 7.0), (4.75, 7.0));
    assert!(w.state().drag_preview().unwrap().snap);
    release(&mut w, to);
    let after = bounds(&w, id);
    assert!(near(after.2 - after.0, 2.5, 1e-6) && near(after.3 - after.1, 1.0, 1e-9), "a side alone changes one axis: {after:?}");
    // The east side with Shift: the proportions are kept and the height grows about its middle.
    let id = drop_at(&mut w, "rectangle", 7.0, 2.0);
    click_at(&mut w, 7.0, 2.0);
    hold(&mut w, Modifiers::SHIFT);
    let (_, to) = drag_page(&mut w, (7.75, 2.0), (9.25, 2.0));
    release(&mut w, to);
    hold(&mut w, Modifiers::NONE);
    let after = bounds(&w, id);
    let (width, height) = (after.2 - after.0, after.3 - after.1);
    assert!(near(width / height, 1.5, 1e-6) && near((after.1 + after.3) / 2.0, 2.0, 1e-9), "grows about its middle: {after:?}");
}

/// With Alt held no `snap` is sent, and a corner still keeps the proportions: the window makes the proportional box itself and sends it.
#[test]
fn a_corner_dragged_with_alt_still_keeps_its_proportions_and_is_sent_without_snap() {
    let mut w = edit_window([1280.0, 900.0]);
    let id = drop_at(&mut w, "rectangle", 3.0, 4.0);
    click_at(&mut w, 3.0, 4.0);
    hold(&mut w, Modifiers::ALT);
    let (_, to) = drag_page(&mut w, (3.75, 4.5), (4.9, 5.37));
    let preview = w.state().drag_preview().unwrap();
    assert!(!preview.snap, "Alt: nothing is sent with snap: {preview:?}");
    let sized = preview.sized.unwrap();
    assert!(near(sized.2 / sized.3, 1.5, 1e-6), "kept all the same: {sized:?}");
    release(&mut w, to);
    hold(&mut w, Modifiers::NONE);
    let after = bounds(&w, id);
    assert!(near((after.2 - after.0) / (after.3 - after.1), 1.5, 1e-6) && near(after.2 - after.0, sized.2, 1e-9), "{sized:?} {after:?}");
    assert_eq!(w.state().last_snap(), None);
}

// ---------------------------------------------------------------------------------------------------------------- `view`

/// A resize stops at the width of a shape in the view and not at that of a shape out of it: the window sends its view with the resize and asks the engine with it. A shape
/// 2 inches wide is 40 inches off to the right, out of the view, and another is in it; the east side dragged to 1.97 comes to 2 only by the one in view.
#[test]
fn a_resize_matches_the_width_of_a_shape_in_the_view_and_not_one_out_of_it() {
    // Out of the view alone: nothing to match.
    let mut w = edit_window([1280.0, 900.0]);
    no_grid(&mut w);
    let page = page_id(&w);
    let far = drop_at(&mut w, "rectangle", 40.0, 6.0);
    run_command(&mut w, "shape.resize", json!({"id": far, "x": 39.0, "y": 5.5, "width": 2.0, "height": 1.0, "page": page}));
    let id = drop_at(&mut w, "rectangle", 3.0, 4.0);
    click_at(&mut w, 3.0, 4.0);
    let (_, to) = drag_page(&mut w, (3.75, 4.0), (3.75 + 0.47, 4.0));
    let preview = w.state().drag_preview().unwrap();
    let sized = preview.sized.unwrap();
    assert!(near(sized.2, 1.97, 1e-3), "not matched, the shape is out of view: {sized:?}");
    assert!(preview.marks.iter().all(|m| !matches!(m, Mark::Size { .. })), "{:?}", preview.marks);
    release(&mut w, to);
    assert!(near(bounds(&w, id).2 - bounds(&w, id).0, sized.2, 1e-9), "the release is the preview");
    run_command(&mut w, "shape.delete", json!({"ids": [id], "page": page}));
    // In the view: matched, with the mark and its label.
    let near_one = drop_at(&mut w, "rectangle", 8.0, 6.0);
    run_command(&mut w, "shape.resize", json!({"id": near_one, "x": 7.0, "y": 5.5, "width": 2.0, "height": 1.0, "page": page}));
    let id = drop_at(&mut w, "rectangle", 3.0, 2.0);
    click_at(&mut w, 3.0, 2.0);
    let (_, to) = drag_page(&mut w, (3.75, 2.0), (3.75 + 0.47, 2.0));
    let preview = w.state().drag_preview().unwrap();
    let sized = preview.sized.unwrap();
    assert!(near(sized.2, 2.0, 1e-6), "a shape in view is matched: {sized:?} {:?} {}", preview.marks, w.state().window_state()["pageRect"]);
    assert!(preview.marks.iter().any(|m| matches!(m, Mark::Size { label, .. } if label == "2 in")), "the size mark says 2 in: {:?}", preview.marks);
    release(&mut w, to);
    let after = bounds(&w, id);
    assert!(near(after.2 - after.0, 2.0, 1e-9), "{after:?}");
}

// ---------------------------------------------------------------------------------------------------------------- 3. the marks, painted by segment with their labels

/// Equal gaps are marked with their length, in the preview and in what `ui.inspect` tells of the drag: a shape dragged to repeat the gap of two others shows both gaps, each
/// with its label (three places at most, the page's unit), and the window paints them.
#[test]
fn equal_gaps_are_drawn_with_their_values_and_inspect_lists_them() {
    let mut w = edit_window([1280.0, 900.0]);
    no_grid(&mut w);
    drop_at(&mut w, "rectangle", 2.0, 4.0);
    drop_at(&mut w, "rectangle", 5.0, 4.0);
    // 8.0 would leave 1.5 inches, the gap between the first two; 8.03 is within the pull.
    let dragged = drop_at(&mut w, "rectangle", 8.2, 4.0);
    work::reset();
    let (_, to) = drag_page(&mut w, (8.2, 4.0), (8.03, 4.0));
    let preview = w.state().drag_preview().unwrap();
    assert!(near(preview.delta.unwrap().0, -0.2, 1e-5), "the box lands at 8.0: {:?}", preview.delta);
    let gaps: Vec<&Mark> = preview.marks.iter().filter(|m| matches!(m, Mark::Gap { .. })).collect();
    assert_eq!(gaps.len(), 2, "both gaps of the row are marked: {:?}", preview.marks);
    for gap in &gaps {
        assert!(matches!(gap, Mark::Gap { label, value, .. } if label == "1.5 in" && near(*value, 1.5, 1e-9)), "{gap:?}");
    }
    // What `ui.inspect` lists is what the window draws.
    let told = w.state().window_state()["drag"]["marks"].clone();
    let told_gaps: Vec<_> = told.as_array().unwrap().iter().filter(|m| m["mark"] == json!("gap")).collect();
    assert_eq!(told_gaps.len(), 2, "{told}");
    assert!(told_gaps.iter().all(|m| m["label"] == json!("1.5 in") && m["chain"] == gaps_chain(&gaps)), "{told}");
    let painted = work::work();
    assert!(painted.labels_painted >= 2 && painted.marks_painted >= 2, "the marks and their labels are painted: {painted:?}");
    assert_eq!(w.state().ui_panics(), 0);
    release(&mut w, to);
    assert!(near(bounds(&w, dragged).0, 7.25, 1e-9), "{:?}", bounds(&w, dragged));
}

fn gaps_chain(gaps: &[&Mark]) -> serde_json::Value {
    match gaps.first() {
        Some(Mark::Gap { chain, .. }) => json!(chain),
        _ => json!(null),
    }
}

/// A row turned 30 degrees is measured and marked along its own direction: the gaps run along the row, the marks say which way, and the labels are the values.
#[test]
fn the_gaps_of_a_turned_row_are_marked_along_the_row() {
    let mut w = edit_window([1280.0, 900.0]);
    no_grid(&mut w);
    let (s, c) = f64::to_radians(30.0).sin_cos();
    let u = (c, s);
    let first = (3.0, 2.5);
    let second = (first.0 + 3.0 * u.0, first.1 + 3.0 * u.1);
    let end = (first.0 + 6.0 * u.0 + 0.03 * u.0, first.1 + 6.0 * u.1 + 0.03 * u.1);
    let start = (end.0 + 0.5, end.1 - 0.4);
    let ids = [
        drop_at(&mut w, "rectangle", first.0, first.1),
        drop_at(&mut w, "rectangle", second.0, second.1),
        drop_at(&mut w, "rectangle", start.0, start.1),
    ];
    for id in ids {
        turn(&mut w, id, 30.0);
    }
    work::reset();
    let (_, to) = drag_page(&mut w, start, end);
    let preview = w.state().drag_preview().unwrap();
    let gaps: Vec<&Mark> = preview.marks.iter().filter(|m| matches!(m, Mark::Gap { normal: Some(_), .. })).collect();
    assert_eq!(gaps.len(), 2, "both gaps are marked along the row: {:?}", preview.marks);
    for gap in gaps {
        let Mark::Gap { from, to, label, value, .. } = gap else { panic!("a gap: {gap:?}") };
        assert!(label == "1.5 in" && near(*value, 1.5, 1e-9), "{gap:?}");
        // The segment runs along the row: the cross product with its direction is nothing.
        let d = (to.0 - from.0, to.1 - from.1);
        assert!((d.0 * u.1 - d.1 * u.0).abs() < 1e-9 && near(d.0.hypot(d.1), 1.5, 1e-9), "{gap:?}");
    }
    assert!(work::work().labels_painted >= 2);
    assert_eq!(w.state().ui_panics(), 0);
    release(&mut w, to);
}

// ---------------------------------------------------------------------------------------------------------------- 4. arrow keys

/// An arrow key is one press of `shape.move` with `snap: {nudge: {tick}}`: the selection goes to the next place it can snap to however far; a connection point of the
/// shape's own on the line it travels along, which meets another's, stops it; the next press moves off the place it stopped on; Shift with an arrow is one screen point.
#[test]
fn an_arrow_key_goes_to_the_next_place_and_stops_on_a_connection_point_in_its_way() {
    let mut w = edit_window([1280.0, 900.0]);
    no_grid(&mut w);
    let moving = drop_at(&mut w, "rectangle", 3.0, 4.0);
    drop_at(&mut w, "rectangle", 7.0, 4.0);
    click_at(&mut w, 3.0, 4.0);
    assert_eq!(w.state().selection(), [moving]);
    // Its right-middle connection point (3.75, 4) meets the other's left-middle (6.25, 4): 2.5 inches, the nearest place of any kind.
    key(&mut w, Key::ArrowRight, Modifiers::NONE);
    let after = bounds(&w, moving);
    assert!(near(after.0, 4.75, 1e-9), "a press goes to the next place however far: {after:?} {:?}", w.state().last_snap());
    let said = w.state().last_snap().cloned().expect("a press is sent with snap");
    assert!(said["hits"].as_array().unwrap().iter().any(|h| h["target"] == json!("connection_point")), "{said}");
    // The next press moves off the place it stopped on, to the next one: its top middle point comes to the other's top left corner, three quarters of an inch on.
    key(&mut w, Key::ArrowRight, Modifiers::NONE);
    let second = bounds(&w, moving).0;
    assert!(near(second, 5.5, 1e-9), "{:?} {:?}", bounds(&w, moving), w.state().last_snap());
    // Shift with an arrow is one screen point, with no stops.
    key(&mut w, Key::ArrowRight, Modifiers::SHIFT);
    assert!(near(bounds(&w, moving).0, second + 1.0 / 96.0, 1e-5), "{:?}", bounds(&w, moving));
    assert_eq!(w.state().last_snap(), None, "and no snap is sent with it");
}

/// With nothing ahead a press moves one tick of the ruler as the window draws it at the zoom: an eighth of an inch at 100% and a quarter at 50%.
#[test]
fn with_nothing_ahead_an_arrow_key_moves_one_ruler_tick_at_the_zoom() {
    for (percent, tick) in [(100, 0.125), (50, 0.25)] {
        let mut w = edit_window([1280.0, 900.0]);
        run_command(&mut w, "snap.set", json!({"grid": false, "ruler": false, "dynamic_grid": false}));
        run_command(&mut w, "view.zoom", json!({"percent": percent}));
        let id = drop_at(&mut w, "rectangle", 5.0, 4.0);
        click_at(&mut w, 5.0, 4.0);
        let before = bounds(&w, id);
        key(&mut w, Key::ArrowUp, Modifiers::NONE);
        let after = bounds(&w, id);
        assert!(near(after.1 - before.1, tick, 1e-9) && near(after.0, before.0, 1e-12), "{percent}%: {before:?} {after:?}");
    }
}

// ---------------------------------------------------------------------------------------------------------------- 7. the drop preview with the master's own connection points

/// A shape dropped near another's connection point lands where the ghost showed, the master's own connection points pulled with it as `shape.drop` pulls the new
/// shape's: the master's last connection point is 0.03 inch right of and 0.04 inch above the target's left-middle point, and the ghost and the new shape's
/// middle are where that point is on it.
#[test]
fn a_drop_by_a_connection_point_lands_where_its_ghost_showed() {
    for (master, tile) in [("rectangle", "Rectangle"), ("triangle", "Triangle"), ("parallelogram", "Parallelogram")] {
        let mut w = edit_window([1280.0, 900.0]);
        no_grid(&mut w);
        // The point the pull is taken to: the left-middle of a rectangle at (7, 4), its box from 6.25 to 7.75.
        drop_at(&mut w, "rectangle", 7.0, 4.0);
        let spec = diacraft_engine::stencil::find_master(master).unwrap();
        let (width, height) = (spec.width, spec.height);
        let offsets = spec.connection_offsets();
        let (px, py) = *offsets.last().unwrap();
        // The dropped shape's last connection point 0.03 right of and 0.04 above (6.25, 4): its middle is where that puts it.
        let centre = (6.25 + 0.03 - (px - width / 2.0), 4.0 + 0.04 - (py - height / 2.0));
        let tile = w.state().element(&format!("shapes.tile.{tile}")).unwrap().rect.center();
        let to = at_p(&w, centre);
        move_to(&mut w, tile);
        button(&mut w, tile, true);
        for step in 1..=5 {
            move_to(&mut w, tile + (to - tile) * (step as f32 / 5.0));
        }
        let ghost = w.state().tile_preview().expect("the ghost is over the page");
        assert!(
            near(ghost.0, centre.0 - 0.03, 1e-4) && near(ghost.1, centre.1 - 0.04, 1e-4),
            "{master}: the ghost is pulled to the point: {ghost:?} from {centre:?}"
        );
        release(&mut w, to);
        let ids = crate::editing::shape_ids(&w);
        let made = *ids.last().unwrap();
        let placed = bounds(&w, made);
        let middle = ((placed.0 + placed.2) / 2.0, (placed.1 + placed.3) / 2.0);
        assert!(near(middle.0, ghost.0, 1e-9) && near(middle.1, ghost.1, 1e-9), "{master}: the release is the ghost: {ghost:?} {placed:?}");
    }
}

/// On a page drawn at 2 to 1 a master's size is drawing units: the new shape's box is half as many paper inches, and the ghost is snapped as that box (the command divides the
/// size by the page's scale). The target is 2 inches high on paper with its top at 5; the dropped rectangle (0.5 inch high on paper) is held 0.03 inch over where its top
/// meets that line, and lands with its top on it, which a ghost taken for a box of 1 inch would not (its middle would come to the target's near line instead).
#[test]
fn a_drop_on_a_page_at_another_scale_lands_where_its_ghost_showed() {
    let mut w = edit_window([1280.0, 900.0]);
    no_grid(&mut w);
    let page = page_id(&w);
    run_command(&mut w, "page.scale", json!({"page": page, "paper": 1.0, "drawing": 2.0}));
    let target = drop_at(&mut w, "rectangle", 7.0, 4.0);
    run_command(&mut w, "shape.resize", json!({"id": target, "x": 5.5, "y": 3.0, "width": 3.0, "height": 2.0, "page": page}));
    assert!(near(bounds(&w, target).3, 5.0, 1e-9), "{:?}", bounds(&w, target));
    let tile = w.state().element("shapes.tile.Rectangle").unwrap().rect.center();
    let to = at(&w, 4.0, 4.78);
    move_to(&mut w, tile);
    button(&mut w, tile, true);
    for step in 1..=5 {
        move_to(&mut w, tile + (to - tile) * (step as f32 / 5.0));
    }
    let ghost = w.state().tile_preview().expect("the ghost is over the page");
    assert!(near(ghost.0, 4.0, 1e-4) && near(ghost.1, 4.75, 1e-4), "the ghost's top is on the target's top: {ghost:?}");
    release(&mut w, to);
    let made = *crate::editing::shape_ids(&w).last().unwrap();
    let placed = bounds(&w, made);
    assert!(near(placed.3 - placed.1, 0.5, 1e-9), "the new shape is half the size on paper: {placed:?}");
    let middle = ((placed.0 + placed.2) / 2.0, (placed.1 + placed.3) / 2.0);
    assert!(near(middle.0, ghost.0, 1e-9) && near(middle.1, ghost.1, 1e-9), "the release is the ghost: {ghost:?} {placed:?}");
}

/// The window shows the built-in stencil `id` (it is opened, and then shown, as a click on its row of More Shapes does).
fn show_stencil(w: &mut Window, id: &str) {
    run_command(w, "stencil.open", json!({"id": id}));
    run_command(w, "window.stencil", json!({"stencil": id}));
}

/// Drags the tile `tile` to the page place `to` and holds the button there, the frames of a person's drag.
fn drag_tile(w: &mut Window, tile: &str, to: (f64, f64)) -> Pos2 {
    let drawn = |w: &Window| w.state().elements().iter().filter(|e| e.name.starts_with("shapes.")).map(|e| e.name.clone()).collect::<Vec<_>>();
    let from = w.state().element(tile).unwrap_or_else(|| panic!("{tile} is not drawn: {:?}", drawn(w))).rect.center();
    let to = at_p(w, to);
    move_to(w, from);
    button(w, from, true);
    for step in 1..=5 {
        move_to(w, from + (to - from) * (step as f32 / 5.0));
    }
    to
}

/// What dragging a tile onto the page showed and made: the ghost's rectangle on the screen and its place on the page (inches), and the box of the shape the release made, on the
/// screen and on the page (left, bottom, right, top, inches).
struct Dragged {
    ghost: egui::Rect,
    at_ghost: (f64, f64),
    made: egui::Rect,
    placed: (f64, f64, f64, f64),
}

/// Drags the tile `tile` to the page place (5, 6), reads the ghost with the button still down, lets go, and reads the shape the drop made.
fn drag_and_drop(w: &mut Window, tile: &str) -> Dragged {
    let before = crate::editing::shape_ids(w);
    let to = drag_tile(w, tile, (5.0, 6.0));
    let ghost = w.state().tile_ghost_rect().expect("the ghost is drawn");
    let at_ghost = w.state().tile_preview().expect("the ghost is over the page");
    release(w, to);
    let made = *crate::editing::shape_ids(w).iter().find(|i| !before.contains(i)).expect("dropped");
    let placed = bounds(w, made);
    // The box the drop made, on the screen: its top left and its bottom right.
    let made = egui::Rect::from_min_max(at(w, placed.0, placed.3), at(w, placed.2, placed.1));
    Dragged { ghost, at_ghost, made, placed }
}

impl Dragged {
    /// The ghost is the box of the shape the drop made, to a tenth of a point, and its place is the release's.
    fn assert_ghost_is_the_drop(&self, what: &str) {
        let (ghost, want, placed) = (self.ghost, self.made, self.placed);
        assert!(
            (ghost.min - want.min).abs().max_elem() < 0.1 && (ghost.max - want.max).abs().max_elem() < 0.1,
            "{what}: the ghost {ghost:?} is the box of the shape the drop made {want:?} ({} by {} points)",
            want.width(),
            want.height()
        );
        let middle = ((placed.0 + placed.2) / 2.0, (placed.1 + placed.3) / 2.0);
        assert!(
            near(middle.0, self.at_ghost.0, 1e-9) && near(middle.1, self.at_ghost.1, 1e-9),
            "{what}: the release is the ghost: {:?} {placed:?}",
            self.at_ghost
        );
    }
}

/// U49, the ghost of a master that follows its text: the ghost drawn while dragging a UML class is the box its drop makes, on a page drawn at `k` to 1. A class is a
/// group of compartments whose height follows their text: beyond the range of eight of the page's scale (100 to 1) the drop keeps the master's size on paper, 1.6 inches wide and 1.2 tall
/// (the compartments' least heights follow the page's drop scale, where they were constants a drop does not scale and left the class shorter than the master); within it
/// (1 or 2 to 1) it is the master's drawing size over `k`. The ghost, the snap box it was pulled by and the shape the release makes are one box: the ghost's corners are the made shape's,
/// to a tenth of a point, and its middle is the release's.
fn the_ghost_of_a_class_is_the_box_its_drop_makes(k: f64) {
    let mut w = edit_window([1280.0, 900.0]);
    no_grid(&mut w);
    let page = page_id(&w);
    if k != 1.0 {
        run_command(&mut w, "page.scale", json!({"page": page, "paper": 1.0, "drawing": k}));
    }
    show_stencil(&mut w, "uml-class");
    let dragged = drag_and_drop(&mut w, "shapes.tile.Class");
    dragged.assert_ghost_is_the_drop(&format!("scale {k}"));
    // What the box is, so that the test says what it holds: the master's width over the ratio, or beyond the range its own, and beyond it the master's height too.
    let placed = dragged.placed;
    let width = placed.2 - placed.0;
    assert!(near(width, if k > 8.0 { 1.6 } else { 1.6 / k }, 1e-9), "scale {k}: the width of the drop: {width}");
    if k > 8.0 {
        assert!(
            near(placed.3 - placed.1, 1.2, 1e-6),
            "scale {k}: the compartments' least heights follow the scale, so the drop is the master's height: {placed:?}"
        );
    }
}

#[test]
fn the_ghost_of_a_class_is_the_box_its_drop_makes_at_1_to_1() {
    the_ghost_of_a_class_is_the_box_its_drop_makes(1.0);
}

#[test]
fn the_ghost_of_a_class_is_the_box_its_drop_makes_at_2_to_1() {
    the_ghost_of_a_class_is_the_box_its_drop_makes(2.0);
}

/// The case of U49: beyond the range of eight, a group whose height follows its text.
#[test]
fn the_ghost_of_a_class_is_the_box_its_drop_makes_at_100_to_1() {
    the_ghost_of_a_class_is_the_box_its_drop_makes(100.0);
}

/// The ghost of a master of a stencil file is the box its drop makes, beyond the range of eight too (the engine drops it on a copy of the document): the "Pointed" master is 2 by 1
/// inches drawn at 1:1, and on a page at 100 to 1 the drop keeps that size on paper.
#[test]
fn the_ghost_of_a_stencil_file_master_is_the_box_its_drop_makes_beyond_the_range_of_eight() {
    let dir = TempDir::new("pointed-ghost");
    let path = fixture::write(dir.path(), "Pointed.vssx", &pointed_stencil());
    let mut w = crate::editing::edit_window_with([1280.0, 900.0], |app| app.with_stencil_picker(crate::stencil_files::picker(path.clone())));
    no_grid(&mut w);
    let page = page_id(&w);
    run_command(&mut w, "page.scale", json!({"page": page, "paper": 1.0, "drawing": 100.0}));
    w.state_mut().execute("shapes.openStencil", &json!({})).unwrap();
    w.rest();
    let dragged = drag_and_drop(&mut w, "shapes.tile.Pointed");
    dragged.assert_ghost_is_the_drop("a stencil file's master at 100 to 1");
    let placed = dragged.placed;
    assert!(near(placed.2 - placed.0, 2.0, 1e-9) && near(placed.3 - placed.1, 1.0, 1e-9), "the drop keeps the master's size on paper: {placed:?}");
}

/// U49, what it costs: the shape a drop would make is worked out **once for a drag**, whatever the pointer does over the page for dozens of frames, moving or still, and once more when the
/// drawing changes under the drag (a change of the page's scale: the ghost is worked out again); a second drag makes its own. Counted (`snap::counts().drops`: the drops made on a copy of the
/// document), not timed.
#[test]
fn the_shape_a_drop_would_make_is_worked_out_once_for_a_drag_and_again_when_the_scale_changes() {
    let mut w = edit_window([1280.0, 900.0]);
    no_grid(&mut w);
    let page = page_id(&w);
    show_stencil(&mut w, "uml-class");
    diacraft_engine::snap::reset_counts();
    let drops = || diacraft_engine::snap::counts().drops;
    drag_tile(&mut w, "shapes.tile.Class", (5.0, 6.0));
    assert_eq!(drops(), 1, "the first frames of the drag work the shape out once");
    // Forty frames over the page: the pointer moving across it, then standing still.
    for i in 0..20 {
        let p = at_p(&w, (5.0 + 0.1 * f64::from(i), 6.0));
        move_to(&mut w, p);
    }
    let still = at_p(&w, (7.0, 6.0));
    for _ in 0..20 {
        move_to(&mut w, still);
    }
    assert_eq!(drops(), 1, "forty more frames, moving and still, work nothing out");
    let one_to_one = w.state().tile_ghost_rect().expect("the ghost is drawn");
    // The page's scale changes under the drag: the drawing's revision is another, so the shape is worked out again, once, and the ghost is the box of 100 to 1: the master's size on paper (the compartments' least heights follow the scale, where the class was the shorter box).
    w.state_mut().execute("page.scale", &json!({"page": page, "paper": 1.0, "drawing": 100.0})).unwrap();
    for _ in 0..10 {
        move_to(&mut w, still);
    }
    assert_eq!(drops(), 2, "a change of the page's scale works it out again, once");
    let beyond = w.state().tile_ghost_rect().expect("the ghost is drawn");
    assert!(
        near(f64::from(beyond.height()), f64::from(one_to_one.height()), 1e-3)
            && near(f64::from(beyond.width()), f64::from(one_to_one.width()), 1e-3),
        "the least heights follow the scale, so the class keeps its size on paper: {one_to_one:?} then {beyond:?}"
    );
    release(&mut w, still);
    assert_eq!(drops(), 2, "the release works nothing out");
    // A second drag makes its own.
    let to = drag_tile(&mut w, "shapes.tile.Class", (5.0, 3.0));
    assert_eq!(drops(), 3, "another drag works its own shape out");
    release(&mut w, to);
}

// ---------------------------------------------------------------------------------------------------------------- 8. the rotation handle through `snap_turn`

/// The rotation handle turns a shape to another shape's angle within reach, which beats the step: at 150 points from the pin the step is 5 degrees and the
/// reach 2.29, so 34 degrees comes to the 33 of another shape and 36.5 to the step, 35. The preview is the release, and the preview says the angle with its label.
#[test]
fn the_rotation_handle_comes_to_another_shapes_angle_and_the_preview_is_the_release() {
    for (raw, wants_match, comes_to) in [(34.0_f64, true, 33.0_f64), (36.5, false, 35.0)] {
        let mut w = edit_window([1280.0, 900.0]);
        no_grid(&mut w);
        let id = drop_at(&mut w, "rectangle", 4.0, 3.0);
        let other = drop_at(&mut w, "rectangle", 8.0, 6.0);
        turn(&mut w, other, 33.0);
        click_at(&mut w, 4.0, 3.0);
        assert_eq!(w.state().selection(), [id]);
        // The handle is 0.375 inch above the top side (36 points), which is 0.5 above the pin: straight up. The pointer goes round the pin to 90 + raw degrees, 150 points out.
        let radius = 150.0 / 96.0;
        let angle = (90.0 + raw).to_radians();
        let pointer = (4.0 + radius * angle.cos(), 3.0 + radius * angle.sin());
        let (_, to) = drag_page(&mut w, (4.0, 3.875), pointer);
        let preview = w.state().drag_preview().unwrap();
        assert!(preview.snap, "{raw}: sent with snap: {preview:?}");
        let turned = preview.turned.expect("the preview says where the shape comes to");
        assert!(near(turned, comes_to, 1e-6), "{raw}: comes to {comes_to}: {turned}");
        let angle_mark = preview
            .marks
            .iter()
            .any(|m| matches!(m, Mark::Angle { label, matched, .. } if matched.is_some() == wants_match && label == &format!("{comes_to} deg")));
        assert!(angle_mark, "{raw}: the angle mark and its label: {:?}", preview.marks);
        assert!(near(w.state().window_state()["drag"]["turned"].as_f64().unwrap(), comes_to, 1e-6), "{}", w.state().window_state()["drag"]);
        release(&mut w, to);
        assert!(near(degrees_of(&w, id), comes_to, 1e-6), "{raw}: the release is the preview: {}", degrees_of(&w, id));
        let said = w.state().last_snap().cloned().expect("sent with snap");
        assert!(near(said["angle"].as_f64().unwrap(), comes_to, 1e-6), "{said}");
    }
}

// ---------------------------------------------------------------------------------------------------------------- 9. the page read off the interface thread

/// A window on the drawing `bytes` (read from a file in `dir`) at 100%, whose reader of pages has a thread of its own that waits for `hook` before it works.
fn gated_window(dir: &TempDir, bytes: &[u8], hook: diacraft_ui_egui::snap_reader::Hook) -> Window {
    let path = fixture::write(dir.path(), "drawing.vsdx", bytes);
    let mut session = Session::new();
    session.execute("file.open", &json!({"path": path.to_str().unwrap()})).unwrap();
    let app = DiacraftApp::new(session, None).with_inline_pictures(bundled_drawer()).with_snap_reader(SnapReader::threaded_with(hook));
    let mut w = fast_window(app, [1280.0, 900.0]);
    w.rest();
    run_command(&mut w, "view.zoom", json!({"percent": 100}));
    w
}

/// The reader holds the index of 5,000 shapes back, and no frame waits for it: the drag goes as the pointer made it, **counted** (no snap asked, no index made here, no
/// work of the reader done in a frame, the window's clock let run for seconds with nothing changed), and a release then is what the preview showed (sent without `snap`).
/// When the reader answers, the next frames snap, and the index was never made on this thread.
#[test]
fn no_frame_waits_for_the_index_of_a_crowded_page_which_a_thread_makes() {
    let dir = TempDir::new("reader-gated");
    let (let_go, held) = channel::<()>();
    let held = Mutex::new(held);
    // The first request waits to be let go, the later ones do not.
    let first = AtomicBool::new(true);
    let hook: diacraft_ui_egui::snap_reader::Hook = Arc::new(move || {
        if first.swap(false, Ordering::SeqCst) {
            let _ = held.lock().map(|rx| rx.recv());
        }
    });
    let mut w = gated_window(&dir, &fixture::crowded_vsdx(5_000), hook);
    // Shape 4054 is the 0.08 inch square with its lower left at (5.5, 4.2).
    let id = 4054;
    let from = at(&w, 5.54, 4.24);
    for _ in 0..3 {
        move_to(&mut w, from);
    }
    work::reset();
    button(&mut w, from, true);
    for step in 1..=20 {
        move_to(&mut w, from + vec2(2.0 * step as f32, 0.7 * step as f32));
        // Seconds of the window's own time pass; nothing the reader does depends on them.
        w.advance(Duration::from_secs(1));
    }
    assert_eq!(w.state().selection(), [id]);
    let held_back = work::work();
    assert!(w.state().snap_reading(), "the reader has not answered: {held_back:?}");
    assert_eq!(
        (held_back.snaps, held_back.index_built_here, held_back.reads_inline),
        (0, 0, 0),
        "no snap is asked and no read is made on this thread: {held_back:?}"
    );
    let preview = w.state().drag_preview().unwrap();
    assert!(!preview.snap && preview.marks.is_empty(), "the drag goes as the pointer made it, and is sent so: {preview:?}");
    let raw = preview.delta.unwrap();
    assert!(near(raw.0, 40.0 / 96.0, 1e-4) && near(raw.1, -14.0 / 96.0, 1e-4), "{raw:?}");
    // A release now is what the preview showed. (The window does not come to rest here: the reader is held back.)
    let before = bounds(&w, id);
    button(&mut w, from + vec2(40.0, 14.0), false);
    w.step();
    w.step();
    let after = bounds(&w, id);
    assert!(near(after.0 - before.0, raw.0, 1e-9) && near(after.1 - before.1, raw.1, 1e-9), "{raw:?} {before:?} {after:?}");
    assert_eq!(w.state().last_snap(), None, "sent without snap");
    // The reader answers; the next drag snaps from its first frames, and no index was made on this thread.
    let_go.send(()).unwrap();
    for _ in 0..5_000 {
        if !w.state().snap_reading() {
            break;
        }
        w.step();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(!w.state().snap_reading(), "the reader answered");
    let again = at(&w, after.0 + 0.04, after.1 + 0.04);
    for _ in 0..3 {
        move_to(&mut w, again);
    }
    work::reset();
    button(&mut w, again, true);
    move_to(&mut w, again + vec2(30.0, 3.0));
    for _ in 0..5_000 {
        if !w.state().snap_reading() {
            break;
        }
        w.step();
        std::thread::sleep(Duration::from_millis(2));
    }
    move_to(&mut w, again + vec2(31.0, 3.0));
    let ready = work::work();
    assert!(ready.snaps >= 1 && ready.index_built_here == 0 && ready.reads_inline == 0, "{ready:?}");
    assert!(w.state().drag_preview().unwrap().snap, "the drag snaps now");
    release(&mut w, again + vec2(31.0, 3.0));
}

/// The page is read once for a gesture on the reader's thread and not again by each move: three moves of a drag that began after the reader answered ask three snaps and
/// no reading.
#[test]
fn a_reader_with_a_thread_makes_the_page_ready_once_for_a_gesture() {
    let dir = TempDir::new("reader-thread");
    let hook: diacraft_ui_egui::snap_reader::Hook = Arc::new(|| {});
    let mut w = gated_window(&dir, &fixture::crowded_vsdx(2_000), hook);
    let id = 1054;
    let from = at(&w, 5.54, 1.24);
    for _ in 0..3 {
        move_to(&mut w, from);
    }
    button(&mut w, from, true);
    move_to(&mut w, from + vec2(30.0, 0.0));
    for _ in 0..5_000 {
        if !w.state().snap_reading() {
            break;
        }
        w.step();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(w.state().selection(), [id]);
    work::reset();
    for step in 1..=3 {
        move_to(&mut w, from + vec2(30.0 + 5.0 * step as f32, 2.0 * step as f32));
    }
    let moves = work::work();
    assert_eq!((moves.snaps, moves.snap_page_reads, moves.index_built_here, moves.reads_inline), (3, 0, 0, 0), "{moves:?}");
    release(&mut w, from + vec2(45.0, 6.0));
}

/// A drag held still takes the snap as soon as the reader answers: the preview is worked out again when the page is ready, with the pointer where it was (the landing's key
/// holds the answers the reader has given), and it is the snapped one, not the drag as the pointer made it.
#[test]
fn a_drag_held_still_snaps_as_soon_as_the_reader_answers() {
    let dir = TempDir::new("reader-still");
    let (let_go, held) = channel::<()>();
    let held = Mutex::new(held);
    let first = AtomicBool::new(true);
    let hook: diacraft_ui_egui::snap_reader::Hook = Arc::new(move || {
        if first.swap(false, Ordering::SeqCst) {
            let _ = held.lock().map(|rx| rx.recv());
        }
    });
    let mut w = gated_window(&dir, &fixture::crowded_vsdx(2_000), hook);
    // Shape 1054 is the 0.08 inch square with its lower left at (5.5, 1.2); its neighbours are a tenth of an inch apart, so a drag of 0.3125 inch is pulled.
    let from = at(&w, 5.54, 1.24);
    for _ in 0..3 {
        move_to(&mut w, from);
    }
    button(&mut w, from, true);
    move_to(&mut w, from + vec2(30.0, 0.0));
    let waiting = w.state().drag_preview().unwrap();
    assert!(!waiting.snap && waiting.marks.is_empty() && near(waiting.delta.unwrap().0, 30.0 / 96.0, 1e-4), "{waiting:?}");
    let_go.send(()).unwrap();
    for _ in 0..5_000 {
        if !w.state().snap_reading() {
            break;
        }
        w.step();
        std::thread::sleep(Duration::from_millis(2));
    }
    // The pointer has not moved: only frames.
    w.step();
    w.step();
    let ready = w.state().drag_preview().unwrap();
    assert!(ready.snap && !ready.marks.is_empty(), "the snap came with the reader's answer: {ready:?}");
    assert!(!near(ready.delta.unwrap().0, waiting.delta.unwrap().0, 1e-6), "and moved the drag: {:?} {:?}", waiting.delta, ready.delta);
    release(&mut w, from + vec2(30.0, 0.0));
}

/// A guide along no axis is drawn and a turned shape's own side parallel to it comes to it, in the preview as in the release, and the snap holds nothing: the
/// guide's line is at 120 degrees, and the rectangle turned 30 degrees has its left side along it, 0.03 inch off.
#[test]
fn a_guide_along_no_axis_is_drawn_and_a_turned_shape_snaps_its_own_side_to_it() {
    let mut w = edit_window([1280.0, 900.0]);
    no_grid(&mut w);
    let page = page_id(&w);
    let guide = add_guide(&mut w, "vertical", 6.0);
    // The guide's line runs at the angle its shape is turned to.
    turn(&mut w, guide, 120.0);
    let slanted = diacraft_engine::snap::SnapPage::read(w.state().session().document().unwrap(), page).unwrap().slanted.clone();
    assert_eq!(slanted.len(), 1, "the guide is along no axis now: {slanted:?} {}", slanted.first().map_or(0.0, |g| g.angle.to_degrees()));
    assert!(w.state().element(&format!("canvas.slanted_guide.{guide}")).is_some(), "and it is drawn");
    let g = slanted[0];
    assert!((g.angle.to_degrees() - 120.0).abs() < 1e-9, "the guide runs at 120 degrees: {}", g.angle.to_degrees());
    // The line's normal, and where along it the line is.
    let n = (g.angle.sin(), -g.angle.cos());
    let c = n.0 * g.at.0 + n.1 * g.at.1;
    // The rectangle's pin at the end of the drag: its left side (0.75 inch from its pin along its own x axis, which is the normal) 0.03 inch past the line.
    let u = (f64::to_radians(30.0).cos(), f64::to_radians(30.0).sin());
    let pin0 = (4.0, 3.0);
    let t = c + 0.75 + 0.03 - (u.0 * pin0.0 + u.1 * pin0.1);
    let end = (pin0.0 + u.0 * t, pin0.1 + u.1 * t);
    let start = (end.0 - 0.6, end.1 + 0.5);
    let id = drop_at(&mut w, "rectangle", start.0, start.1);
    turn(&mut w, id, 30.0);
    let (_, to) = drag_page(&mut w, start, end);
    let preview = w.state().drag_preview().unwrap();
    assert!(preview.snap, "{preview:?}");
    assert!(
        preview.marks.iter().any(|m| matches!(m, Mark::Line { normal: Some(_), targets, shape: Some(s), .. } if *s == guide && targets.contains(&diacraft_engine::snap::Target::Guide))),
        "the guide is lit along its own direction: {:?}",
        preview.marks
    );
    let delta = preview.delta.unwrap();
    release(&mut w, to);
    let pin = pin_of(&w, id);
    assert!((n.0 * pin.0 + n.1 * pin.1 - 0.75 - c).abs() < 1e-6, "the side is on the guide: {pin:?}");
    assert!(near(pin.0 - start.0, delta.0, 1e-9) && near(pin.1 - start.1, delta.1, 1e-9), "the release is the preview");
    assert!(held_to(&w, guide).is_empty(), "a snap to a slanted guide holds nothing");
}

// ---------------------------------------------------------------------------------------------------------------- a change under a millionth is sent

/// A shape B whose box is 2 inches wide with its right side 5e-7 inch off A's: the target of the three tests below, which land a drag a hair from where it began.
fn hair_apart_target(w: &mut Window) -> u32 {
    let page = page_id(w);
    let b = drop_at(w, "rectangle", 2.7500005, 7.0);
    run_command(w, "shape.resize", json!({"id": b, "x": 1.7500005, "y": 6.5, "width": 2.0, "height": 1.0, "page": page}));
    assert!(near(bounds(w, b).2, 3.7500005, 1e-9), "{:?}", bounds(w, b));
    b
}

/// A move whose snap pulls it back to a place 5e-7 inch from where it began is sent, as its preview showed it: the release was left unsent under a millionth, and the shape
/// stayed where it was while the preview and the marks said it had moved.
#[test]
fn a_move_that_lands_a_hair_from_where_it_began_is_sent_and_is_the_preview() {
    let mut w = edit_window([1280.0, 900.0]);
    no_grid(&mut w);
    let a = drop_at(&mut w, "rectangle", 3.0, 4.0);
    hair_apart_target(&mut w);
    // A's right side (3.75) dragged 0.045 right comes back to B's right side (3.7500005).
    let (_, to) = drag_page(&mut w, (3.0, 4.0), (3.045, 4.0));
    let preview = w.state().drag_preview().unwrap();
    assert!(preview.snap, "{preview:?}");
    let delta = preview.delta.unwrap();
    assert!(near(delta.0, 5.0e-7, 1e-9) && near(delta.1, 0.0, 1e-9), "the preview moves it 5e-7 inch: {delta:?} {:?}", preview.marks);
    release(&mut w, to);
    let after = bounds(&w, a);
    assert!(near(after.0, 2.25 + delta.0, 1e-9) && near(after.2, 3.75 + delta.0, 1e-9), "the release is the preview: {after:?}");
    assert!(w.state().last_snap().is_some(), "it was sent, with snap");
}

/// A resize whose snap pulls a side back to 5e-7 inch from where it began is sent, as its preview showed it.
#[test]
fn a_resize_that_lands_a_hair_from_where_it_began_is_sent_and_is_the_preview() {
    let mut w = edit_window([1280.0, 900.0]);
    no_grid(&mut w);
    let a = drop_at(&mut w, "rectangle", 3.0, 4.0);
    hair_apart_target(&mut w);
    click_at(&mut w, 3.0, 4.0);
    assert_eq!(w.state().selection(), [a]);
    // The east side (3.75) dragged 0.045 right comes back to B's right side.
    let (_, to) = drag_page(&mut w, (3.75, 4.0), (3.795, 4.0));
    let preview = w.state().drag_preview().unwrap();
    assert!(preview.snap, "{preview:?}");
    let sized = preview.sized.unwrap();
    assert!(near(sized.2, 1.5000005, 1e-9), "the preview widens it by 5e-7 inch: {sized:?} {:?}", preview.marks);
    release(&mut w, to);
    let after = bounds(&w, a);
    assert!(near(after.2 - after.0, sized.2, 1e-9) && near(after.2, 3.7500005, 1e-9), "the release is the preview: {sized:?} {after:?}");
}

/// A turn that comes to another shape's angle 1e-7 degree from where it began is sent, as its preview showed it (with its angle mark).
#[test]
fn a_turn_that_lands_a_hair_from_where_it_began_is_sent_and_is_the_preview() {
    let mut w = edit_window([1280.0, 900.0]);
    no_grid(&mut w);
    let id = drop_at(&mut w, "rectangle", 4.0, 3.0);
    let other = drop_at(&mut w, "rectangle", 8.0, 6.0);
    turn(&mut w, other, 1.0e-7);
    click_at(&mut w, 4.0, 3.0);
    assert_eq!(w.state().selection(), [id]);
    // 150 points from the pin the reach of another shape's angle is 2.29 degrees: the pointer 2 degrees round comes to the other's 1e-7.
    let radius = 150.0 / 96.0;
    let angle = (90.0_f64 + 2.0).to_radians();
    let pointer = (4.0 + radius * angle.cos(), 3.0 + radius * angle.sin());
    let (_, to) = drag_page(&mut w, (4.0, 3.875), pointer);
    let preview = w.state().drag_preview().unwrap();
    assert!(preview.snap, "{preview:?}");
    let turned = preview.turned.expect("the preview says where the shape comes to");
    assert!(near(turned, 1.0e-7, 1e-9), "the other shape's angle: {turned}");
    assert!(preview.marks.iter().any(|m| matches!(m, Mark::Angle { matched: Some(s), .. } if *s == other)), "the angle mark: {:?}", preview.marks);
    release(&mut w, to);
    assert!(near(degrees_of(&w, id), turned, 1e-9), "the release is the preview: {} {turned}", degrees_of(&w, id));
}

// ---------------------------------------------------------------------------------------------------------------- the angle mark is drawn at the pin

/// The angle mark of a turn is at the pin, which the shape turns about, and not at the middle of its box: the shape's pin is moved to the top left of its box, and the preview's
/// mark, `shape.rotate`'s answer and `snap.at`'s all say the pin.
#[test]
fn the_angle_mark_of_a_turn_is_at_the_pin_and_not_the_middle_of_the_box() {
    let mut w = edit_window([1280.0, 900.0]);
    no_grid(&mut w);
    let page = page_id(&w);
    let id = drop_at(&mut w, "rectangle", 4.0, 3.0);
    let other = drop_at(&mut w, "rectangle", 8.0, 6.0);
    turn(&mut w, other, 33.0);
    run_command(&mut w, "shape.pin", json!({"ids": [id], "at": "top-left", "page": page}));
    // The box is (3.25, 2.5) to (4.75, 3.5): the pin is its top left, (3.25, 3.5), and the middle is (4, 3).
    click_at(&mut w, 4.0, 3.0);
    assert_eq!(w.state().selection(), [id]);
    let pin: (f64, f64) = (3.25, 3.5);
    let (grab, radius): ((f64, f64), f64) = ((4.0, 3.875), 150.0 / 96.0);
    // The pointer goes round the pin, 34 degrees from where the handle is.
    let start = (grab.1 - pin.1).atan2(grab.0 - pin.0);
    let angle = start + 34.0_f64.to_radians();
    let pointer = (pin.0 + radius * angle.cos(), pin.1 + radius * angle.sin());
    let (_, to) = drag_page(&mut w, grab, pointer);
    let preview = w.state().drag_preview().unwrap();
    let mark = preview.marks.iter().find_map(|m| if let Mark::Angle { pivot, .. } = m { Some(*pivot) } else { None }).expect("an angle mark");
    assert!(near(mark.0, pin.0, 1e-6) && near(mark.1, pin.1, 1e-6), "the mark is at the pin: {mark:?} (the middle of the box is (4, 3))");
    release(&mut w, to);
    let said = w.state().last_snap().cloned().expect("sent with snap");
    let sent = said["marks"].as_array().unwrap().iter().find(|m| m["mark"] == "angle").expect("the release's angle mark").clone();
    assert!(near(sent["pivot"][0].as_f64().unwrap(), pin.0, 1e-6) && near(sent["pivot"][1].as_f64().unwrap(), pin.1, 1e-6), "{sent}");
}

// ---------------------------------------------------------------------------------------------------------------- the Text tool's drag matches sizes

/// A shape A 2 inches wide, from (3.4, 2) to (5.4, 3.5).
fn wide_shape(w: &mut Window) -> u32 {
    let page = page_id(w);
    let a = drop_at(w, "rectangle", 4.4, 2.75);
    run_command(w, "shape.resize", json!({"id": a, "x": 3.4, "y": 2.0, "width": 2.0, "height": 1.5, "page": page}));
    let b = bounds(w, a);
    assert!(near(b.0, 3.4, 1e-9) && near(b.1, 2.0, 1e-9) && near(b.2, 5.4, 1e-9) && near(b.3, 3.5, 1e-9), "{b:?}");
    a
}

/// The Text tool dragged from (6, 2) to (7.97, 2.5) comes to a box of 2 inches (A's width, which is in the view) with a size mark on both boxes labelled "2 in" and naming A
/// (the Text case): the box the preview shows, the corners it says, are the box the text box is made with, and Alt held leaves the drag as the pointer made it.
#[test]
fn the_text_tools_drag_stops_at_the_width_of_a_shape_in_view_and_the_box_made_is_the_preview() {
    let mut w = edit_window([1280.0, 900.0]);
    no_grid(&mut w);
    let a = wide_shape(&mut w);
    run_command(&mut w, "tool.text", json!({}));
    let (_, to) = drag_page(&mut w, (6.0, 2.0), (7.97, 2.5));
    let preview = w.state().drag_preview().unwrap();
    let ((x0, y0), (x1, y1)) = preview.corners.expect("the drag's corners");
    assert!(
        near(x0, 6.0, 1e-9) && near(y0, 2.0, 1e-9) && near(x1, 8.0, 1e-9) && near(y1, 2.5, 1e-9),
        "the box is (6, 2) to (8, 2.5): {:?}",
        preview.corners
    );
    assert!(
        preview.marks.iter().any(|m| matches!(m, Mark::Size { label, matched: Some((s, _)), .. } if label == "2 in" && *s == a)),
        "a size mark labelled 2 in that names A: {:?}",
        preview.marks
    );
    assert!(w.state().window_state()["drag"]["corners"].is_array(), "ui.inspect tells the corners");
    button(&mut w, to, false);
    w.rest();
    type_text(&mut w, "Wide");
    key(&mut w, Key::Enter, crate::saving::ctrl());
    let made = *crate::editing::shape_ids(&w).iter().find(|i| **i != a).expect("the text box was made");
    let b = bounds(&w, made);
    assert!(near(b.0, 6.0, 1e-6) && near(b.1, 2.0, 1e-6) && near(b.2, 8.0, 1e-6) && near(b.3, 2.5, 1e-6), "the box made is the box shown: {b:?}");
}

/// The same drag the other way, from the right and the top to the left and the bottom: the side the pointer moves is the left one and the bottom one, and the box is the
/// one whose far corner is the pointer's: from (8, 2.5) to (6.03, 2) the box is (6, 2) to (8, 2.5).
#[test]
fn the_text_tools_drag_the_other_way_moves_the_left_and_the_bottom_side() {
    let mut w = edit_window([1280.0, 900.0]);
    no_grid(&mut w);
    let a = wide_shape(&mut w);
    run_command(&mut w, "tool.text", json!({}));
    let (_, to) = drag_page(&mut w, (8.0, 2.5), (6.03, 2.0));
    let preview = w.state().drag_preview().unwrap();
    let ((x0, y0), (x1, y1)) = preview.corners.expect("the drag's corners");
    assert!(
        near(x0, 8.0, 1e-9) && near(y0, 2.5, 1e-9) && near(x1, 6.0, 1e-9) && near(y1, 2.0, 1e-9),
        "the pointer's corner is (6, 2): {:?}",
        preview.corners
    );
    assert!(
        preview.marks.iter().any(|m| matches!(m, Mark::Size { label, matched: Some((s, _)), .. } if label == "2 in" && *s == a)),
        "{:?}",
        preview.marks
    );
    button(&mut w, to, false);
    w.rest();
    type_text(&mut w, "Back");
    key(&mut w, Key::Enter, crate::saving::ctrl());
    let made = *crate::editing::shape_ids(&w).iter().find(|i| **i != a).expect("the text box was made");
    let b = bounds(&w, made);
    assert!(near(b.0, 6.0, 1e-6) && near(b.1, 2.0, 1e-6) && near(b.2, 8.0, 1e-6) && near(b.3, 2.5, 1e-6), "the box made is the box shown: {b:?}");
}

/// With Alt, and with no shape of that width in the view, the Text tool's drag is as the pointer made it, and shows no size mark.
#[test]
fn the_text_tools_drag_matches_no_width_under_alt_or_when_no_shape_is_that_wide() {
    let mut w = edit_window([1280.0, 900.0]);
    no_grid(&mut w);
    wide_shape(&mut w);
    run_command(&mut w, "tool.text", json!({}));
    hold(&mut w, Modifiers::ALT);
    let (_, to) = drag_page(&mut w, (6.0, 2.0), (7.97, 2.5));
    let preview = w.state().drag_preview().unwrap();
    let ((_, _), (x1, y1)) = preview.corners.expect("the drag's corners");
    assert!(near(x1, 7.97, 1e-6) && near(y1, 2.5, 1e-6), "Alt: as the pointer made it: {:?}", preview.corners);
    assert!(preview.marks.iter().all(|m| !matches!(m, Mark::Size { .. })), "{:?}", preview.marks);
    button(&mut w, to, false);
    hold(&mut w, Modifiers::NONE);
    key(&mut w, Key::Escape, Modifiers::NONE);
    // Nothing is 1.4 inches wide: the box is the one dragged.
    run_command(&mut w, "tool.text", json!({}));
    let (_, to) = drag_page(&mut w, (6.0, 5.0), (7.4, 5.6));
    let preview = w.state().drag_preview().unwrap();
    let ((_, _), (x1, y1)) = preview.corners.expect("the drag's corners");
    assert!(near(x1, 7.4, 1e-6) && near(y1, 5.6, 1e-6), "no shape that wide: {:?}", preview.corners);
    assert!(preview.marks.iter().all(|m| !matches!(m, Mark::Size { .. })), "{:?}", preview.marks);
    button(&mut w, to, false);
    key(&mut w, Key::Escape, Modifiers::NONE);
}

// ---------------------------------------------------------------------------------------------------------------- the drop preview with a stencil file's master

/// A stencil of one master "Pointed", 2 by 1 with one connection point 0.6 inch from its left and 0.9 from its bottom: where no corner and no middle of its box is.
fn pointed_stencil() -> Vec<u8> {
    use diacraft_testkit::package::{MasterSpec, VsdxBuilder, shape};
    let cells = format!(
        "{}{}{}",
        fixture::placed(0.0, 0.0, 2.0, 1.0),
        fixture::rectangle(2.0, 1.0),
        r#"<Section N="Connection"><Row IX="0"><Cell N="X" V="0.6" F="Width*0.3"/><Cell N="Y" V="0.9" F="Height*0.9"/></Row></Section>"#
    );
    VsdxBuilder::new()
        .document_type("application/vnd.ms-visio.stencil.main+xml") // brand-ok: a content type the format defines
        .master(MasterSpec::new(1, "Pointed").shapes(&shape(5, &cells)))
        .build()
}

/// A master of a stencil file dropped by its own connection point, which no corner and no middle of its box is, lands where its ghost showed: the preview pulls the master's
/// points (the engine reads them off the shape a drop on a copy of the document makes), as `shape.drop` pulls the new shape's. The point is 0.4 left of and 0.4 above the box's
/// middle; the target's left-middle point is (6.25, 4); the box's middle is held 0.03 right of and 0.02 below where that puts the point on it.
#[test]
fn a_stencil_file_master_dropped_by_a_point_off_its_places_lands_where_its_ghost_showed() {
    let dir = TempDir::new("pointed");
    let path = fixture::write(dir.path(), "Pointed.vssx", &pointed_stencil());
    let mut w = crate::editing::edit_window_with([1280.0, 900.0], |app| app.with_stencil_picker(crate::stencil_files::picker(path.clone())));
    no_grid(&mut w);
    drop_at(&mut w, "rectangle", 7.0, 4.0);
    w.state_mut().execute("shapes.openStencil", &json!({})).unwrap();
    w.rest();
    let tile = w.state().element("shapes.tile.Pointed").expect("the file's master has a tile").rect.center();
    let before = crate::editing::shape_ids(&w);
    let centre = (6.25 + 0.4 + 0.03, 4.0 - 0.4 - 0.02);
    let to = at_p(&w, centre);
    move_to(&mut w, tile);
    button(&mut w, tile, true);
    for step in 1..=5 {
        move_to(&mut w, tile + (to - tile) * (step as f32 / 5.0));
    }
    let ghost = w.state().tile_preview().expect("the ghost is over the page");
    assert!(
        near(ghost.0, centre.0 - 0.03, 1e-4) && near(ghost.1, centre.1 + 0.02, 1e-4),
        "the ghost is pulled by the master's own point: {ghost:?} from {centre:?}"
    );
    release(&mut w, to);
    let made = *crate::editing::shape_ids(&w).iter().find(|i| !before.contains(i)).expect("dropped");
    let placed = bounds(&w, made);
    let middle = ((placed.0 + placed.2) / 2.0, (placed.1 + placed.3) / 2.0);
    assert!(near(middle.0, ghost.0, 1e-9) && near(middle.1, ghost.1, 1e-9), "the release is the ghost: {ghost:?} {placed:?}");
}
