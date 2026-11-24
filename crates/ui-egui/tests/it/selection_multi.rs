//! The handles of the combined box of several shapes, and of a group, work: a corner stretches the shapes about the opposite corner in proportion, a side
//! stretches them in one axis, the rotation handle turns them about the middle of their joint box, and a group is resized and turned as one shape with its members following.
//! The pointer is given to egui as a person's would be; what is checked is the document the drag's one command changed, the preview while it goes and the pointer.

use diacraft_engine::draw::shape_corners;
use egui::{CursorIcon, Pos2, Vec2, vec2};
use serde_json::json;

use crate::editing::{at, click_at, frame_of};
use crate::selection_look::{AROUND_TWO, handles, hold_net, three};
use crate::support::{Rest, Window, button, move_to, run_command};

type P = (f64, f64);

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1.0e-6
}

fn cursor(w: &Window) -> CursorIcon {
    w.output().platform_output.cursor_icon
}

/// The boxes of the shapes `ids`, as the document has them: lower left and upper right.
fn boxes(w: &Window, ids: &[u32]) -> Vec<(P, P)> {
    ids.iter()
        .map(|id| {
            let (x, y, width, height) = frame_of(w, *id);
            ((x, y), (x + width, y + height))
        })
        .collect()
}

/// The joint box of some boxes.
fn joint(boxes: &[(P, P)]) -> (P, P) {
    let lo = boxes.iter().fold((f64::INFINITY, f64::INFINITY), |m, b| (m.0.min(b.0.0), m.1.min(b.0.1)));
    let hi = boxes.iter().fold((f64::NEG_INFINITY, f64::NEG_INFINITY), |m, b| (m.0.max(b.1.0), m.1.max(b.1.1)));
    (lo, hi)
}

/// Drops the net over the rectangle and the diamond and lets go: the two are selected, with the combined box and its handles round them.
fn two_selected() -> (Window, Vec<u32>) {
    let (mut w, ids) = three();
    let (_, b) = hold_net(&mut w, AROUND_TWO.0, AROUND_TWO.1);
    button(&mut w, b, false);
    w.rest();
    assert_eq!(w.state().selection(), &ids[..2], "the net took the rectangle and the diamond");
    (w, ids[..2].to_vec())
}

/// Presses at `from` (a screen point), drags by `by` in steps and releases; the drag's state is read by `during` at the end of the last step, before the release.
fn drag_screen(w: &mut Window, from: Pos2, by: Vec2, during: impl FnOnce(&Window)) {
    for _ in 0..3 {
        move_to(w, from);
    }
    button(w, from, true);
    for step in 1..=5 {
        move_to(w, from + by * (step as f32 / 5.0));
    }
    w.step();
    during(w);
    button(w, from + by, false);
    w.rest();
}

/// The handle of the combined box that is furthest toward the screen's upper right.
fn north_east(w: &Window) -> Pos2 {
    handles(w).into_iter().map(|(c, _)| c).max_by(|a, b| (a.x - a.y).total_cmp(&(b.x - b.y))).unwrap()
}

/// The handle on the middle of the right side: of the handles furthest to the right, the one between the other two.
fn east(w: &Window) -> Pos2 {
    let all: Vec<Pos2> = handles(w).into_iter().map(|(c, _)| c).collect();
    let right = all.iter().map(|c| c.x).fold(f32::NEG_INFINITY, f32::max);
    let mut side: Vec<Pos2> = all.into_iter().filter(|c| (c.x - right).abs() < 1.0).collect();
    side.sort_by(|a, b| a.y.total_cmp(&b.y));
    side[side.len() / 2]
}

/// Dragging the corner handle of a net selection stretches both shapes about the opposite corner: each shape's box is the old one carried by the stretch that takes the joint
/// box to the new one, the proportions of the joint box are kept (a corner keeps them with no key), and it is one command and one undo step.
#[test]
fn dragging_a_corner_handle_of_a_net_selection_stretches_both_shapes_about_the_opposite_corner() {
    let (mut w, ids) = two_selected();
    let before = boxes(&w, &ids);
    let (lo, hi) = joint(&before);
    let revision = w.state().session().revision();
    let corner = north_east(&w);
    // Out to the right and up: the lower left corner stays.
    drag_screen(&mut w, corner, vec2(170.0, -60.0), |w| {
        let sized = w.state().drag_preview().and_then(|d| d.sized).expect("the drag has a landing while it goes");
        assert!(sized.2 > hi.0 - lo.0, "the preview is wider than the box now: {sized:?}");
    });
    let after = boxes(&w, &ids);
    let (nlo, nhi) = joint(&after);
    assert!(near(nlo.0, lo.0) && near(nlo.1, lo.1), "the opposite corner stays: {nlo:?} against {lo:?}");
    let (sx, sy) = ((nhi.0 - nlo.0) / (hi.0 - lo.0), (nhi.1 - nlo.1) / (hi.1 - lo.1));
    assert!(sx > 1.2, "the shapes are stretched: {sx}");
    assert!((sx - sy).abs() < 1e-6, "a corner keeps the proportions: {sx} by {sy}");
    for ((old_lo, old_hi), (new_lo, new_hi)) in before.iter().zip(&after) {
        let carried = |p: P| (lo.0 + (p.0 - lo.0) * sx, lo.1 + (p.1 - lo.1) * sy);
        let (want_lo, want_hi) = (carried(*old_lo), carried(*old_hi));
        assert!(
            near(new_lo.0, want_lo.0) && near(new_lo.1, want_lo.1) && near(new_hi.0, want_hi.0) && near(new_hi.1, want_hi.1),
            "{new_lo:?} {new_hi:?} for {want_lo:?} {want_hi:?}"
        );
    }
    assert_eq!(w.state().session().revision(), revision + 1, "one command");
    assert_eq!(w.state().selection(), &ids[..], "the selection is kept");
    // One undo step puts both back.
    run_command(&mut w, "edit.undo", json!({}));
    assert_eq!(boxes(&w, &ids), before);
}

/// A side handle stretches in one axis: the other axis is as it was, and the opposite side stays.
#[test]
fn dragging_a_side_handle_stretches_in_one_axis() {
    let (mut w, ids) = two_selected();
    let before = boxes(&w, &ids);
    let (lo, hi) = joint(&before);
    let handle = east(&w);
    drag_screen(&mut w, handle, vec2(120.0, 40.0), |_| {});
    let after = boxes(&w, &ids);
    let (nlo, nhi) = joint(&after);
    assert!(near(nlo.0, lo.0) && near(nlo.1, lo.1) && near(nhi.1, hi.1), "the other three sides stay: {nlo:?} {nhi:?} against {lo:?} {hi:?}");
    assert!(nhi.0 > hi.0 + 0.5, "the east side moved out: {} to {}", hi.0, nhi.0);
    for ((old_lo, old_hi), (new_lo, new_hi)) in before.iter().zip(&after) {
        assert!(near(new_lo.1, old_lo.1) && near(new_hi.1, old_hi.1), "up the page nothing changed");
    }
}

/// The pointer over a handle of the combined box is the two-headed arrow along its direction, the grabbing hand over the rotation handle, and while a corner is dragged the
/// arrow of that corner.
#[test]
fn the_pointer_over_the_handles_of_the_combined_box_is_the_spec_s_arrows() {
    let (mut w, _) = two_selected();
    let corner = north_east(&w);
    for _ in 0..3 {
        move_to(&mut w, corner);
    }
    assert_eq!(cursor(&w), CursorIcon::ResizeNeSw, "the north-east corner: the arrow from lower left to upper right");
    let side = east(&w);
    move_to(&mut w, side);
    assert_eq!(cursor(&w), CursorIcon::ResizeHorizontal, "the east side");
    let all: Vec<Pos2> = handles(&w).into_iter().map(|(c, _)| c).collect();
    let middle = (all.iter().map(|c| c.x).fold(f32::INFINITY, f32::min) + all.iter().map(|c| c.x).fold(f32::NEG_INFINITY, f32::max)) / 2.0;
    let top = all.into_iter().filter(|c| (c.x - middle).abs() < 1.0).min_by(|a, b| a.y.total_cmp(&b.y)).unwrap();
    let glyph = top - vec2(0.0, diacraft_ui_egui::theme::select::ROTATE_CENTRE);
    move_to(&mut w, glyph);
    assert_eq!(cursor(&w), CursorIcon::Grab, "over the rotation handle");
    drag_screen(&mut w, corner, vec2(60.0, -20.0), |w| assert_eq!(cursor(w), CursorIcon::ResizeNeSw, "while the corner is dragged"));
}

/// Dragging the rotation handle of a net selection turns both shapes as one about the middle of their joint box: a quarter turn clockwise turns each pin a quarter about it and
/// each shape's angle goes to -90 degrees, in one command.
#[test]
fn dragging_the_rotation_handle_of_a_net_selection_turns_both_shapes_about_the_middle_of_their_joint_box() {
    let (mut w, ids) = two_selected();
    let before: Vec<P> = ids.iter().map(|id| centres(&w, *id)).collect();
    let (lo, hi) = joint(&boxes(&w, &ids));
    let centre = ((lo.0 + hi.0) / 2.0, (lo.1 + hi.1) / 2.0);
    let north = handles(&w)
        .into_iter()
        .map(|(c, _)| c)
        .filter(|c| (c.x - at(&w, centre.0, centre.1).x).abs() < 1.0)
        .min_by(|a, b| a.y.total_cmp(&b.y))
        .unwrap();
    let glyph = north - vec2(0.0, diacraft_ui_egui::theme::select::ROTATE_CENTRE);
    let pin = at(&w, centre.0, centre.1);
    // From above the middle round to its right, far enough out for steps of 5 degrees: a quarter turn clockwise on the screen.
    let to = pin + vec2(240.0, 0.0);
    let revision = w.state().session().revision();
    drag_screen(&mut w, glyph, to - glyph, |w| {
        assert_eq!(w.state().drag_preview().and_then(|d| d.turned), Some(-90.0), "the preview is the quarter turn clockwise");
        assert_eq!(cursor(w), CursorIcon::Grabbing);
    });
    assert_eq!(w.state().session().revision(), revision + 1, "one command");
    for (id, was) in ids.iter().zip(&before) {
        let (angle, pin_now) = (angle_of(&w, *id), centres(&w, *id));
        assert!((angle + 90.0).abs() < 1e-6, "shape {id} is turned to {angle} degrees");
        // A quarter turn clockwise about the centre: (dx, dy) goes to (dy, -dx).
        let want = (centre.0 + (was.1 - centre.1), centre.1 - (was.0 - centre.0));
        assert!(near(pin_now.0, want.0) && near(pin_now.1, want.1), "shape {id}: the pin {pin_now:?} should be {want:?}");
    }
    run_command(&mut w, "edit.undo", json!({}));
    assert!(ids.iter().all(|id| angle_of(&w, *id).abs() < 1e-6), "undo turns them back");
}

fn centres(w: &Window, id: u32) -> P {
    let d = w.state().session().document().unwrap();
    let corners = shape_corners(d, d.pages.get(0).unwrap().id, id).unwrap();
    // The middle of the box, which is the pin of these shapes.
    (((corners[0].0 + corners[2].0) / 2.0), ((corners[0].1 + corners[2].1) / 2.0))
}

fn angle_of(w: &Window, id: u32) -> f64 {
    let d = w.state().session().document().unwrap();
    let page = d.pages.get(0).unwrap().id;
    let sheet = diacraft_engine::doc::SheetRef::Shape(d.find_shape(diacraft_engine::doc::Owner::Page(page), id).unwrap());
    let resolver = diacraft_engine::doc::Resolver::new(d);
    let found = resolver.resolve(&sheet, &diacraft_engine::doc::CellAddr::direct("Angle")).unwrap();
    found.value.cell.and_then(|c| c.value.as_ref()).and_then(|v| v.number()).unwrap_or(0.0).to_degrees()
}

/// A group is one shape: its handles are live. Dragging the corner of a selected group stretches it and its members follow; a quarter turn by its rotation handle turns the
/// group and its members with it.
#[test]
fn a_group_is_resized_and_turned_by_its_handles_and_its_members_follow() {
    let (mut w, ids) = three();
    run_command(&mut w, "shape.group", json!({"ids": [ids[0], ids[1]]}));
    w.rest();
    click_at(&mut w, 3.0, 6.0);
    let group = w.state().selection().to_vec();
    assert_eq!(group.len(), 1, "a click on a member selects the group");
    let members: Vec<(P, P)> = boxes(&w, &ids[..2]);
    let (lo, hi) = joint(&members);
    let corner = north_east(&w);
    drag_screen(&mut w, corner, vec2(120.0, -50.0), |_| {});
    let after = boxes(&w, &ids[..2]);
    let (nlo, nhi) = joint(&after);
    assert!(near(nlo.0, lo.0) && near(nlo.1, lo.1), "the opposite corner stays: {nlo:?} against {lo:?}");
    assert!(nhi.0 - nlo.0 > (hi.0 - lo.0) + 0.5, "the group, and so the members, are wider");
    let (sx, sy) = ((nhi.0 - nlo.0) / (hi.0 - lo.0), (nhi.1 - nlo.1) / (hi.1 - lo.1));
    for ((old_lo, _), (new_lo, _)) in members.iter().zip(&after) {
        assert!(
            near(new_lo.0, lo.0 + (old_lo.0 - lo.0) * sx) && near(new_lo.1, lo.1 + (old_lo.1 - lo.1) * sy),
            "a member's place is carried by the stretch: {new_lo:?}"
        );
    }
    assert!(w.state().selection() == &group[..], "the group is still selected");
}

/// Escape in the middle of a drag of a handle drops it: the preview goes and nothing is sent, as for one shape.
#[test]
fn escape_in_the_middle_of_a_drag_of_the_combined_box_sends_nothing() {
    let (mut w, ids) = two_selected();
    let before = boxes(&w, &ids);
    let revision = w.state().session().revision();
    let corner = north_east(&w);
    for _ in 0..3 {
        move_to(&mut w, corner);
    }
    button(&mut w, corner, true);
    for step in 1..=5 {
        move_to(&mut w, corner + vec2(30.0, -10.0) * step as f32);
    }
    w.step();
    assert!(w.state().drag_preview().is_some_and(|d| d.sized.is_some()), "the drag is on");
    crate::editing::key(&mut w, egui::Key::Escape, egui::Modifiers::NONE);
    assert!(w.state().drag_preview().is_none(), "the drag is dropped");
    button(&mut w, corner + vec2(150.0, -50.0), false);
    w.rest();
    assert_eq!(w.state().session().revision(), revision, "no command was sent");
    assert_eq!(boxes(&w, &ids), before);
}
