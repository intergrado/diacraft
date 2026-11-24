//! A turned shape's drag and its release (the preview of a drag is its release). The engine aligns a shape turned by other than whole
//! quarter turns by the box round the paths it draws (`snap::bounds_of`) when `shape.move` snaps it, so the window's preview of a move has
//! to align the same box: with the box round the corners it shows the shape nowhere near where the release puts it. One case each of a move, a resize and a
//! drop, with a shape that is turned and one that is not. The only snap targets are the other shape's alignment lines (the grid and the ruler are off).

use diacraft_engine::draw::shape_corners;
use diacraft_engine::snap::{Axis, Target};
use diacraft_ui_egui::edit::snapping::Mark;
use egui::Pos2;
use serde_json::json;

use crate::editing::{at, click_at, edit_window, shape_ids};
use crate::guides::{bounds, near, page_id, press_and_drag};
use crate::support::{Rest, Window, button, move_to, run_command};

/// Lets go of the button at `at` and lets the window rest.
fn release(w: &mut Window, at: Pos2) {
    button(w, at, false);
    w.rest();
}

/// A shape dropped by the command from the built-in stencil, centred at `(x, y)` exactly (no snap). → its `ID`, width and height.
fn dropped(w: &mut Window, master: &str, x: f64, y: f64) -> (u32, f64, f64) {
    let page = page_id(w);
    let answer = w.state_mut().execute("shape.drop", &json!({"master": master, "page": page, "x": x, "y": y})).unwrap();
    w.rest();
    (u32::try_from(answer["id"].as_u64().unwrap()).unwrap(), answer["width"].as_f64().unwrap(), answer["height"].as_f64().unwrap())
}

/// The top of the curve of an ellipse of `size` (width, height) turned `degrees` about its pin at height `y`: the extreme of the box round its paths.
fn curve_top(y: f64, (width, height): (f64, f64), degrees: f64) -> f64 {
    let (s, c) = degrees.to_radians().sin_cos();
    y + (width * width / 4.0 * s * s + height * height / 4.0 * c * c).sqrt()
}

/// The stencil's ellipse with its pin at (4, 4), turned `degrees` (not at all for 0). → its `ID`, the top of its curve and its width. The grid and the ruler are
/// switched off, so that only the other shapes pull a drag.
fn turned_ellipse(w: &mut Window, degrees: f64) -> (u32, f64, f64) {
    run_command(w, "snap.set", json!({"grid": false, "ruler": false}));
    let (ellipse, width, height) = dropped(w, "ellipse", 4.0, 4.0);
    if degrees != 0.0 {
        let page = page_id(w);
        run_command(w, "shape.rotate", json!({"ids": [ellipse], "angle": degrees, "page": page}));
    }
    (ellipse, curve_top(4.0, (width, height), degrees), width)
}

/// A move of a shape turned by `degrees` (30 and 210 are turned, 0 and 90 are not: a quarter turn is not) lands where the preview showed it. A rectangle's top is
/// 0.03 inch above the top of the ellipse's curve, and the ellipse is dragged to the right, not up: its curve's top comes to the rectangle's top, as the
/// engine's `shape.move` puts it. (Aligned by its corners, a turned ellipse is 0.27 inch from the line and nothing pulls it: the preview shows no move up, and
/// the release moves it 0.03.)
#[test]
fn a_move_of_a_shape_turned_or_not_lands_where_its_preview_showed() {
    for degrees in [0.0, 30.0, 90.0, 210.0] {
        let mut w = edit_window([1280.0, 900.0]);
        let (ellipse, top, _) = turned_ellipse(&mut w, degrees);
        let (rectangle, _, rectangle_height) = dropped(&mut w, "rectangle", 9.5, top + 0.03 - 0.5);
        assert!(near(rectangle_height, 1.0, 1e-9) && near(bounds(&w, rectangle).3, top + 0.03, 1e-9), "{:?}", bounds(&w, rectangle));
        let before = bounds(&w, ellipse);
        let (from, to) = (at(&w, 4.0, 4.0), at(&w, 4.5, 4.0));
        press_and_drag(&mut w, from, to);
        let preview = w.state().drag_preview().unwrap();
        assert!(preview.snap, "{degrees} degrees: the release is sent with snap: {preview:?}");
        let delta = preview.delta.unwrap();
        assert!(near(delta.0, 0.5, 1e-6), "{degrees} degrees: nothing pulls it along x: {delta:?}");
        assert!(near(delta.1, 0.03, 0.004), "{degrees} degrees: the curve's top comes to the rectangle's top: {delta:?}");
        assert!(
            preview.marks.iter().any(|m| matches!(m, Mark::Line { axis: Axis::Y, targets, .. } if targets.contains(&Target::Alignment))),
            "{degrees} degrees: the line it lined up with is drawn: {:?}",
            preview.marks
        );
        release(&mut w, to);
        let after = bounds(&w, ellipse);
        assert!(
            near(after.0 - before.0, delta.0, 1e-9) && near(after.1 - before.1, delta.1, 1e-9),
            "{degrees} degrees: the release is the preview: {delta:?} {before:?} {after:?}"
        );
        let said = w.state().last_snap().cloned().unwrap();
        assert!(near(said["dy"].as_f64().unwrap(), delta.1, 1e-6), "{degrees} degrees: the command snapped by the same box: {said}");
    }
}

/// A move of two shapes lines up by the box round both, as the command makes it: an unturned rectangle (selected first) and the ellipse, turned or not, whose
/// curve's top is above the rectangle's. The rectangle at the right has its top 0.03 inch above that curve, and the box of the two comes down on it.
#[test]
fn a_move_of_two_shapes_lands_where_its_preview_showed_by_the_box_round_both() {
    for degrees in [0.0, 30.0] {
        let mut w = edit_window([1280.0, 900.0]);
        let (ellipse, top, _) = turned_ellipse(&mut w, degrees);
        let (beside, ..) = dropped(&mut w, "rectangle", 1.5, 3.5);
        dropped(&mut w, "rectangle", 9.5, top + 0.03 - 0.5);
        w.state_mut().select(&[beside, ellipse]);
        w.rest();
        assert_eq!(w.state().selection(), [beside, ellipse]);
        let (before_beside, before_ellipse) = (bounds(&w, beside), bounds(&w, ellipse));
        let (from, to) = (at(&w, 4.0, 4.0), at(&w, 4.5, 4.0));
        press_and_drag(&mut w, from, to);
        let delta = w.state().drag_preview().unwrap().delta.unwrap();
        assert!(
            near(delta.0, 0.5, 1e-6) && near(delta.1, 0.03, 0.004),
            "{degrees} degrees: the box round both comes to the rectangle's top: {delta:?}"
        );
        release(&mut w, to);
        for (id, before) in [(beside, before_beside), (ellipse, before_ellipse)] {
            let after = bounds(&w, id);
            assert!(
                near(after.0 - before.0, delta.0, 1e-9) && near(after.1 - before.1, delta.1, 1e-9),
                "{degrees} degrees: shape {id}, the release is the preview: {delta:?} {before:?} {after:?}"
            );
        }
    }
}

/// A resize of an ellipse: not turned, its east side dragged an inch comes 0.03 farther, where a rectangle's east side is, and the release is the preview; turned
/// 30 degrees, it is snapped along its own axes as `shape.resize` snaps it: the corner of its width-height box that lies farthest right comes to the
/// rectangle's east side, a page line, and the box the preview shows is the one the release makes (the window's preview calls `snap_resize_ext`).
#[test]
fn a_resize_of_a_shape_turned_or_not_lands_where_its_preview_showed() {
    for degrees in [0.0, 30.0] {
        let mut w = edit_window([1280.0, 900.0]);
        let (ellipse, _, width) = turned_ellipse(&mut w, degrees);
        let right = 4.0 + width / 2.0;
        // A rectangle (1.5 wide) above, whose east side is 0.03 inch past where the ellipse's goes (a side lines up with the same side of another shape).
        dropped(&mut w, "rectangle", right + 1.0 + 0.03 - 0.75, 7.0);
        click_at(&mut w, 4.0, 4.0);
        assert_eq!(w.state().selection(), [ellipse]);
        // The east handle is half the width from the pin along the shape's own x axis; the pointer takes it an inch farther.
        let (s, c) = f64::to_radians(degrees).sin_cos();
        let (near_pin, far_pin) = (width / 2.0, width / 2.0 + 1.0);
        let (from, to) = (at(&w, 4.0 + near_pin * c, 4.0 + near_pin * s), at(&w, 4.0 + far_pin * c, 4.0 + far_pin * s));
        press_and_drag(&mut w, from, to);
        let preview = w.state().drag_preview().unwrap();
        let sized = preview.sized.unwrap();
        if degrees == 0.0 {
            assert!(preview.snap && near(sized.2, width + 1.03, 1e-6), "the east side comes to the rectangle's: {preview:?}");
        } else {
            assert!(
                preview.snap
                    && preview.marks.iter().any(|m| matches!(m, Mark::Line { axis: Axis::X, targets, .. } if targets.contains(&Target::Alignment))),
                "{degrees} degrees: a turned shape's resize is snapped, and the line is drawn: {preview:?}"
            );
            assert!(!near(sized.2, width + 1.0, 1e-4), "{degrees} degrees: the box is not the one the handle made: {preview:?}");
        }
        release(&mut w, to);
        let corners = shape_corners(w.state().session().document().unwrap(), page_id(&w), ellipse).unwrap();
        let ellipse_width = width;
        let (width, height) = (
            f64::hypot(corners[1].0 - corners[0].0, corners[1].1 - corners[0].1),
            f64::hypot(corners[3].0 - corners[0].0, corners[3].1 - corners[0].1),
        );
        assert!(
            near(corners[0].0, sized.0, 1e-9) && near(corners[0].1, sized.1, 1e-9) && near(width, sized.2, 1e-9) && near(height, sized.3, 1e-9),
            "{degrees} degrees: the release is the preview: {sized:?} {corners:?}"
        );
        // A corner of the turned box is on the line: the one that lies farthest right is on the rectangle's east side, at x = 5.78 (the rectangle is 1.5 wide).
        if degrees != 0.0 {
            let right = corners.iter().map(|c| c.0).fold(f64::MIN, f64::max);
            assert!(
                near(right, right_of_rectangle(ellipse_width), 1e-9),
                "{degrees} degrees: a corner of the turned box is on the line: {corners:?}"
            );
        }
    }
}

/// Where the east side of the rectangle that `a_resize_of_a_shape_turned_or_not_lands_where_its_preview_showed` puts above the ellipse is: its centre is 0.03 inch
/// past the ellipse's east side dragged an inch, and it is 1.5 wide.
fn right_of_rectangle(ellipse_width: f64) -> f64 {
    let right = 4.0 + ellipse_width / 2.0;
    right + 1.0 + 0.03 - 0.75 + 0.75
}

/// A drop of the rectangle tile by a turned ellipse: the ghost's top comes to the top of the ellipse's curve (0.03 inch off), as the box round the paths of a
/// turned target is made once by the page, and the release puts the new shape where the ghost was. The same by an ellipse that is not turned.
#[test]
fn a_drop_by_a_shape_turned_or_not_lands_where_its_ghost_showed() {
    for degrees in [0.0, 30.0] {
        let mut w = edit_window([1280.0, 900.0]);
        let (_, top, _) = turned_ellipse(&mut w, degrees);
        let tile = w.state().element("shapes.tile.Rectangle").unwrap().rect.center();
        let to = at(&w, 8.0, top + 0.03 - 0.5);
        move_to(&mut w, tile);
        button(&mut w, tile, true);
        for step in 1..=5 {
            move_to(&mut w, tile + (to - tile) * (step as f32 / 5.0));
        }
        let ghost = w.state().tile_preview().expect("the ghost is over the page");
        assert!(near(ghost.1, top - 0.5, 0.004), "{degrees} degrees: the ghost's top comes to the curve's top: {ghost:?}");
        release(&mut w, to);
        let made = *shape_ids(&w).last().unwrap();
        let placed = bounds(&w, made);
        let (x, y) = ((placed.0 + placed.2) / 2.0, (placed.1 + placed.3) / 2.0);
        assert!(near(x, ghost.0, 1e-9) && near(y, ghost.1, 1e-9), "{degrees} degrees: the release is the ghost: {ghost:?} {placed:?}");
    }
}
