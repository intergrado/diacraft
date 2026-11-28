//! The look of a selection: the net a drag from empty page draws, the marks of one shape selected, and the marks of
//! several.
//!
//! **The net** is a translucent grey veil over the shapes, outlined by one solid pixel, from the press to the pointer, whichever way it is dragged; it shows from
//! the first movement and goes at the release, and nothing is marked on a shape under it. **One shape** selected has a thin grey box on its own box, eight
//! round handles and the rotation handle, which is a circular arrow above the top middle on a stem. **Several shapes** have a dark blue frame on each
//! shape's box (twice as thick on the primary, the one selected first), a connector's own path drawn again in the frame colour, and one thin grey box round them all with
//! the same handles. **A group** is one shape. **One connector** has marks at its two ends: a green disc where it is glued, a square where it is free.
//!
//! Every colour is a theme token (`theme::Tokens`) and every size is in `theme::select`, in screen points: the marks keep their size whatever the zoom. **The handles of
//! several shapes work**: a press on one of the eight starts stretching them as one (`Mode::ResizeSet`, `shape.resize` with `ids`) and a press on the rotation
//! handle starts turning them about the middle of their joint box (`Mode::RotateSet`, `shape.rotate` with `by` and `about`). The box drawn is the joint box the engine
//! stretches from, grown by `select::BOX_MARGIN` (a connector counting by its two ends), so what is drawn is what is resized. A group's handles are a shape's own.

use egui::{CornerRadius, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, vec2};

use super::geom::{self, Basis, Handle, P};
use super::gesture::{Gesture, Mode, page_cursor, rotation_handle};
use super::scene::{Item, Kind, Scene};
use super::{Cx, HANDLE_REACH};
use crate::app::DiacraftApp;
use crate::theme::{Tokens, select};

/// How far the arc of the rotation glyph runs, in degrees, and where it starts: round most of a circle, from just right of the top, clockwise on the screen, to just left of
/// it, where the arrowhead is.
const ARC_START: f32 = -60.0;
const ARC_SWEEP: f32 = 285.0;
/// How many straight pieces the arc is cut into, and the arrowhead's length and half-width, in points.
const ARC_PIECES: usize = 24;
const HEAD_LENGTH: f32 = 4.5;
const HEAD_HALF_WIDTH: f32 = 4.0;

/// The places that make a shape's part of the joint box of a selection, as the engine takes them (`shape.resize` with `ids`, `shape.rotate`): the four corners of its box
/// on the page, and for a connector its two ends and nothing else.
fn joint_points(item: &Item) -> Vec<P> {
    match (item.kind, item.ends) {
        (Kind::Connector, Some((begin, end))) => vec![begin, end],
        _ => item.corners.to_vec(),
    }
}

/// The joint box of `items`: the smallest box on the page that holds their boxes (a connector counting by its two ends), lower left and upper right. `None` for no item or a
/// box that is not a number.
pub(super) fn joint_box(items: &[&Item]) -> Option<(P, P)> {
    let points: Vec<P> = items.iter().flat_map(|item| joint_points(item)).collect();
    geom::bounds_of(&points)
}

/// The box that takes in all of `items`, grown by `margin` inches on every side: lower left and upper right, on the page. `None` for no item or a box that is not a number.
pub(super) fn combined_box(items: &[&Item], margin: f64) -> Option<(P, P)> {
    let (lo, hi) = joint_box(items)?;
    let (lo, hi) = ((lo.0 - margin, lo.1 - margin), (hi.0 + margin, hi.1 + margin));
    [lo.0, lo.1, hi.0, hi.1].iter().all(|v| v.is_finite()).then_some((lo, hi))
}

/// Where the marks of a combined box are on the page: the box, its handles that are shown (`geom::visible_handles` at the handle size) and where the rotation handle's
/// glyph is, above the top middle. `joint` is the joint box inside it, which the handles stand `margin` inches off.
pub(super) struct Combined {
    pub lo: P,
    pub hi: P,
    pub joint: (P, P),
    pub margin: f64,
    pub handles: Vec<(Handle, P)>,
    pub glyph: P,
}

impl Combined {
    pub(super) fn of(items: &[&Item], cx: &Cx) -> Option<Combined> {
        let margin = cx.reach(select::BOX_MARGIN);
        let joint = joint_box(items)?;
        let (lo, hi) = combined_box(items, margin)?;
        let handles = geom::visible_handles(lo, hi, cx.view.scale(), f64::from(select::HANDLE)).into_iter().map(|h| (h, h.at(lo, hi))).collect();
        let glyph = ((lo.0 + hi.0) / 2.0, hi.1 + cx.reach(select::ROTATE_CENTRE));
        Some(Combined { lo, hi, joint, margin, handles, glyph })
    }

    /// The joint box laid on the page's own axes, the frame a stretch of it works in.
    fn basis(&self) -> Basis {
        let (lo, hi) = self.joint;
        Basis { origin: lo, ex: (1.0, 0.0), ey: (0.0, 1.0), w: hi.0 - lo.0, h: hi.1 - lo.1 }
    }
}

/// What a press on the combined box of several shapes comes to.
pub(super) enum OnBox {
    /// The press is on none of its handles.
    Off,
    /// It is on a handle that has nothing to do (a shape of the set is in a turned group, or the joint box has no size): the press keeps the selection and starts nothing.
    Dead,
    /// It starts this drag.
    Start(Mode),
}

/// The screen points of a shape's four corners, if they are all numbers.
fn screen_corners(cx: &Cx, item: &Item) -> Option<[Pos2; 4]> {
    let points = item.corners.map(|p| cx.screen_at(p));
    points.iter().all(|p| p.x.is_finite() && p.y.is_finite()).then_some(points)
}

/// The place the four points cover if they are the corners of a rectangle with its sides along the screen's.
fn upright(points: &[Pos2]) -> Option<Rect> {
    let r = Rect::from_points(points);
    let near = |a: f32, b: f32| (a - b).abs() < 0.01;
    points.iter().all(|p| (near(p.x, r.min.x) || near(p.x, r.max.x)) && (near(p.y, r.min.y) || near(p.y, r.max.y))).then_some(r)
}

/// A line of `stroke` round a shape's corners: a rectangle (which the painter lays on whole pixels, so that it is crisp) when they are upright, else the turned outline.
fn paint_outline(painter: &Painter, points: [Pos2; 4], stroke: Stroke) {
    match upright(&points) {
        Some(rect) => {
            painter.rect_stroke(rect, CornerRadius::ZERO, stroke, StrokeKind::Middle);
        }
        None => {
            painter.add(Shape::closed_line(points.to_vec(), stroke));
        }
    }
}

/// The net of a drag from empty page, from the press to the pointer: the veil, over the picture of the shapes and whatever the editing layer painted, and its outline.
/// The pointer's own crosshair is the window's cursor (`canvas_cursor`).
pub(super) fn paint_net(painter: &Painter, t: &Tokens, cx: &Cx, gesture: &Gesture) {
    let (lo, hi) = geom::ordered(gesture.origin, gesture.now);
    let rect = Rect::from_two_pos(cx.screen_at(lo), cx.screen_at(hi));
    if !(rect.min.x.is_finite() && rect.min.y.is_finite() && rect.max.x.is_finite() && rect.max.y.is_finite()) {
        return;
    }
    painter.rect(rect, CornerRadius::ZERO, t.net_fill, Stroke::new(select::NET_LINE, t.net_line), StrokeKind::Inside);
}

/// The distance between the dots of the lasso's trail, and a dot's radius, in screen points ("a thin grey dotted trail"; the dots are still).
const TRAIL_SPACING: f32 = 3.0;
const TRAIL_DOT: f32 = 0.75;

/// The trail the Lasso Select tool's pointer leaves, from the press to where it is: a thin dotted line in the net's outline grey. It is not closed (the loop closes itself when
/// the button comes up), and nothing moves in it: every dot stays where it was put.
pub(super) fn paint_trail(painter: &Painter, t: &Tokens, cx: &Cx, trail: &[P]) {
    let line: Vec<Pos2> = trail.iter().map(|p| cx.screen_at(*p)).filter(|p| p.x.is_finite() && p.y.is_finite()).collect();
    if line.len() >= 2 {
        painter.extend(Shape::dotted_line(&line, t.net_line, TRAIL_SPACING, TRAIL_DOT));
    }
}

/// The marks of the shapes `items` (the selection, **the primary first**), on the page `cx`: with `handles` off (a drag is on, the Text Block tool shows its own) the
/// frames and the box only.
pub(super) fn paint_selection(painter: &Painter, t: &Tokens, cx: &Cx, items: &[&Item], handles: bool) {
    match items {
        [] => {}
        [item] => paint_single(painter, t, cx, item, handles),
        several => paint_several(painter, t, cx, several, handles),
    }
}

/// One shape: no frame and no primary mark; the box on its own box, the eight handles and the rotation handle. A connector has its two ends.
fn paint_single(painter: &Painter, t: &Tokens, cx: &Cx, item: &Item, handles: bool) {
    if item.kind == Kind::Connector {
        if handles {
            paint_ends(painter, t, cx, item);
        }
        return;
    }
    let Some(corners) = screen_corners(cx, item) else { return };
    paint_outline(painter, corners, Stroke::new(select::BOX_LINE, t.select_box));
    if !handles || !item.rotatable() {
        return;
    }
    // The eight handles sit on the shape's own box where its turn and its flips carry it (gap U9).
    let mut north = false;
    if let Some(basis) = item.basis() {
        let (lo, hi) = basis.local_box();
        for handle in geom::visible_handles(lo, hi, cx.view.scale(), f64::from(select::HANDLE)) {
            north |= handle == Handle::North;
            paint_handle(painter, t, cx.screen_at(basis.to_page(handle.at(lo, hi))));
        }
    }
    if let Some((top, glyph)) = rotation_handle(item, cx) {
        paint_rotation(painter, t, cx.screen_at(top), cx.screen_at(glyph), north);
    }
}

/// Several shapes: a frame on each (the primary's twice as thick, drawn last so that it lies over the others), and with `handles` the one box round them all.
fn paint_several(painter: &Painter, t: &Tokens, cx: &Cx, items: &[&Item], handles: bool) {
    for (n, item) in items.iter().enumerate().rev() {
        paint_member(painter, t, cx, item, if n == 0 { select::FRAME_PRIMARY } else { select::FRAME });
    }
    if !handles {
        return;
    }
    let Some(combined) = Combined::of(items, cx) else { return };
    let corners = [(combined.lo.0, combined.lo.1), (combined.hi.0, combined.lo.1), (combined.hi.0, combined.hi.1), (combined.lo.0, combined.hi.1)]
        .map(|p| cx.screen_at(p));
    if corners.iter().any(|p| !(p.x.is_finite() && p.y.is_finite())) {
        return;
    }
    paint_outline(painter, corners, Stroke::new(select::BOX_LINE, t.select_box));
    let mut north = false;
    for (handle, at) in &combined.handles {
        north |= *handle == Handle::North;
        paint_handle(painter, t, cx.screen_at(*at));
    }
    let top = ((combined.lo.0 + combined.hi.0) / 2.0, combined.hi.1);
    paint_rotation(painter, t, cx.screen_at(top), cx.screen_at(combined.glyph), north);
}

/// The frame of a member of several shapes: its own box, `width` points thick, with no fill; a connector's path drawn again in the same colour.
fn paint_member(painter: &Painter, t: &Tokens, cx: &Cx, item: &Item, width: f32) {
    let stroke = Stroke::new(width, t.select_frame);
    if item.kind == Kind::Connector {
        for path in &item.outline {
            let points: Vec<Pos2> = path.iter().map(|p| cx.screen_at(*p)).collect();
            if points.len() >= 2 && points.iter().all(|p| p.x.is_finite() && p.y.is_finite()) {
                painter.add(Shape::line(points, stroke));
            }
        }
        return;
    }
    if let Some(corners) = screen_corners(cx, item) {
        paint_outline(painter, corners, stroke);
    }
}

/// A selection handle: a circle filled white, with a ring of the box's grey, `select::HANDLE` across all told.
fn paint_handle(painter: &Painter, t: &Tokens, at: Pos2) {
    if at.x.is_finite() && at.y.is_finite() {
        painter.circle(at, (select::HANDLE - select::HANDLE_RING) / 2.0, t.handle_fill, Stroke::new(select::HANDLE_RING, t.select_box));
    }
}

/// The rotation handle: a circular arrow (most of a ring with an arrowhead) centred on `glyph`, on a stem that leaves the box line at `top` (the top middle handle's edge when
/// that handle is shown, `handle`) and ends at the ring.
fn paint_rotation(painter: &Painter, t: &Tokens, top: Pos2, glyph: Pos2, handle: bool) {
    let way = glyph - top;
    let length = way.length();
    if !(length.is_finite() && length > 0.5) {
        return;
    }
    let up = way / length;
    let (from, to) = (top + up * if handle { select::HANDLE / 2.0 } else { 0.0 }, glyph - up * (select::ROTATE_GLYPH / 2.0));
    painter.line_segment([from, to], Stroke::new(select::ROTATE_STEM, t.select_box));
    // The arc is drawn turned with the stem, so that the glyph of a turned shape is turned with it and its arrowhead stays by the stem.
    let turn = up.y.atan2(up.x) + std::f32::consts::FRAC_PI_2;
    let radius = (select::ROTATE_GLYPH - select::ROTATE_RING) / 2.0;
    let at = |degrees: f32| {
        let angle = degrees.to_radians() + turn;
        glyph + vec2(angle.cos(), angle.sin()) * radius
    };
    let arc: Vec<Pos2> = (0..=ARC_PIECES).map(|i| at(ARC_START + ARC_SWEEP * i as f32 / ARC_PIECES as f32)).collect();
    painter.add(Shape::line(arc, Stroke::new(select::ROTATE_RING, t.select_box)));
    // The head at the end of the arc, pointing along it.
    let end = ARC_START + ARC_SWEEP;
    let tip_at = at(end);
    let angle = end.to_radians() + turn;
    let (outward, along) = (vec2(angle.cos(), angle.sin()), vec2(-angle.sin(), angle.cos()));
    painter.add(Shape::convex_polygon(
        vec![tip_at + along * HEAD_LENGTH, tip_at - along * 0.5 + outward * HEAD_HALF_WIDTH, tip_at - along * 0.5 - outward * HEAD_HALF_WIDTH],
        t.select_box,
        Stroke::NONE,
    ));
}

/// A connector's two ends: a green disc with a white halo where it is glued, a square where it is free (white with a grey ring at the begin, grey at the end).
fn paint_ends(painter: &Painter, t: &Tokens, cx: &Cx, item: &Item) {
    let Some((begin, end)) = item.ends else { return };
    for (n, (at, glue)) in [(begin, item.glue[0]), (end, item.glue[1])].into_iter().enumerate() {
        let at = cx.screen_at(at);
        if !(at.x.is_finite() && at.y.is_finite()) {
            continue;
        }
        if glue.is_some() {
            painter.circle_filled(at, select::END_DISC / 2.0 + select::END_HALO, t.handle_fill);
            painter.circle_filled(at, select::END_DISC / 2.0, t.glue_on);
        } else {
            let fill = if n == 0 { t.handle_fill } else { t.select_box };
            painter.rect(
                Rect::from_center_size(at, vec2(select::END_SQUARE, select::END_SQUARE)),
                CornerRadius::ZERO,
                fill,
                Stroke::new(select::BOX_LINE, t.select_box),
                StrokeKind::Inside,
            );
        }
    }
}

impl DiacraftApp {
    /// The shapes of the selection on the page of `scene`, the primary first.
    pub(super) fn selected_items<'a>(&self, scene: &'a Scene) -> Vec<&'a Item> {
        self.ui.edit.selection.iter().filter_map(|id| scene.get(*id)).collect()
    }

    /// What a press at the page point `p` does on the combined box of the shapes selected: a handle stretches them as one, the rotation handle turns them as one about the
    /// middle of their joint box. Handles are looked for before the rotation handle, as a single shape's are. A set with a shape in a turned or flipped group, or whose joint
    /// box has no width or no height, has nothing to start (the engine refuses to carry a place of the page into such a group's frame).
    pub(super) fn press_combined(&self, cx: &Cx, p: P) -> OnBox {
        let Some(scene) = self.ui.edit.scene.as_ref() else { return OnBox::Off };
        let items = self.selected_items(scene);
        if items.len() < 2 {
            return OnBox::Off;
        }
        let Some(combined) = Combined::of(&items, cx) else { return OnBox::Off };
        let reach = cx.reach(HANDLE_REACH);
        let handle = combined
            .handles
            .iter()
            .map(|(handle, at)| (geom::distance(*at, p), *handle))
            .filter(|(d, _)| *d <= reach)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, handle)| handle);
        let on_glyph = geom::distance(combined.glyph, p) <= reach;
        if handle.is_none() && !on_glyph {
            return OnBox::Off;
        }
        let (lo, hi) = combined.joint;
        if items.iter().any(|item| item.in_turned_group) || !((hi.0 - lo.0) > MIN_JOINT && (hi.1 - lo.1) > MIN_JOINT) {
            return OnBox::Dead;
        }
        let ids: Vec<u32> = items.iter().map(|item| item.id).collect();
        match handle {
            Some(handle) => OnBox::Start(Mode::ResizeSet { ids, handle, basis: combined.basis(), margin: combined.margin }),
            None => OnBox::Start(Mode::RotateSet { ids, pin: ((lo.0 + hi.0) / 2.0, (lo.1 + hi.1) / 2.0), grab: combined.glyph }),
        }
    }

    /// The pointer over the handles of the combined box of several shapes: the two-headed arrow along a handle's direction, the grabbing hand over the rotation handle.
    pub(super) fn combined_cursor(&self, cx: &Cx, p: P) -> Option<egui::CursorIcon> {
        let scene = self.ui.edit.scene.as_ref()?;
        let items = self.selected_items(scene);
        if items.len() < 2 {
            return None;
        }
        let combined = Combined::of(&items, cx)?;
        let reach = cx.reach(HANDLE_REACH);
        if let Some((handle, _)) =
            combined.handles.iter().map(|(h, at)| (*h, geom::distance(*at, p))).filter(|(_, d)| *d <= reach).min_by(|a, b| a.1.total_cmp(&b.1))
        {
            return Some(page_cursor(handle));
        }
        (geom::distance(combined.glyph, p) <= reach).then_some(egui::CursorIcon::Grab)
    }
}

/// The least a side of a joint box may be, in inches, for its handles to work: under a millionth there is nothing to stretch from.
const MIN_JOINT: f64 = 1.0e-6;

#[cfg(test)]
mod tests {
    use egui::pos2;

    use super::*;

    fn item(lo: P, hi: P) -> Item {
        Item {
            id: 1,
            kind: Kind::Shape,
            corners: [lo, (hi.0, lo.1), hi, (lo.0, hi.1)],
            frame: (lo, hi),
            turned: false,
            in_turned_group: false,
            pin: ((lo.0 + hi.0) / 2.0, (lo.1 + hi.1) / 2.0),
            parent: None,
            select_mode: 1,
            degrees: 0.0,
            outline: Vec::new(),
            painted: Vec::new(),
            hidden: false,
            text_blocks: Vec::new(),
            ends: None,
            glue: [None, None],
            bounds: (lo, hi),
        }
    }

    /// The combined box takes in every item and is grown by the margin on each side; nothing selected, or a box that is not a number, has none.
    #[test]
    fn the_combined_box_takes_in_every_item_and_grows_by_the_margin() {
        let (a, b) = (item((1.0, 2.0), (2.0, 3.0)), item((4.0, 1.0), (5.0, 2.5)));
        assert_eq!(combined_box(&[&a, &b], 0.0), Some(((1.0, 1.0), (5.0, 3.0))));
        assert_eq!(combined_box(&[&a, &b], 0.25), Some(((0.75, 0.75), (5.25, 3.25))));
        assert_eq!(combined_box(&[&a], 0.0), Some(a.bounds), "one item is its own box");
        assert_eq!(combined_box(&[], 0.0), None);
        let broken = item((f64::NAN, f64::NAN), (f64::NAN, f64::NAN));
        assert_eq!(combined_box(&[&broken], 0.0), None, "a box that is not a number is not drawn");
    }

    /// Four points are a rectangle with its sides along the screen's only when each is at a corner of their bounds, whichever way round they are given.
    #[test]
    fn four_points_are_upright_only_when_they_are_the_corners_of_their_bounds() {
        let square = [pos2(0.0, 0.0), pos2(4.0, 0.0), pos2(4.0, 3.0), pos2(0.0, 3.0)];
        assert_eq!(upright(&square), Some(Rect::from_min_max(pos2(0.0, 0.0), pos2(4.0, 3.0))));
        let flipped = [square[1], square[0], square[3], square[2]];
        assert!(upright(&flipped).is_some(), "a flipped box is still upright");
        let turned = [pos2(2.0, 0.0), pos2(4.0, 1.5), pos2(2.0, 3.0), pos2(0.0, 1.5)];
        assert_eq!(upright(&turned), None, "a box turned 45 degrees is not");
    }
}
