//! What the drawing area paints over the picture while editing: the preview of a drag, the connection points, the highlight of what a connector would be glued to.
//! The selection, its handles and the net of a drag from bare page are `select_look.rs`'s. Every colour is a theme token.

use std::sync::Arc;

use diacraft_engine::snap::{Axis, Target as SnapTarget};
use egui::epaint::PathShape;
use egui::{Painter, Pos2, Shape, Stroke, Vec2, pos2, vec2};

use super::geom::{self, P};
use super::gesture::{Gesture, Mode, Target, Which};
use super::scene::{Item, Kind, Scene};
use super::select_look;
use super::snapping::Mark;
use super::{Cx, Tool};
use crate::app::DiacraftApp;
use crate::theme::{self, Tokens};

/// How far, in points, the line of a snap runs past the shapes it joins on either side, the radius of the ring at a place snapped to, and the size of the ticks and the
/// arrowheads that end a gap's arrow.
const MARK_OVERHANG: f32 = 12.0;
const MARK_RADIUS: f32 = 5.5;
const TICK: f32 = 5.0;
const ARROW: f32 = 5.0;
/// How long the ray of an angle mark is, in points.
const ANGLE_RAY: f32 = 56.0;
/// The size of the text of a label, and the room kept round it inside its plate, in points.
const LABEL_TEXT: f32 = 11.0;
const LABEL_PAD: f32 = 3.0;

/// The direction a line mark runs in on the screen: from its segment, or where the segment has no length from its axis (upright at x) or its normal.
fn line_direction(axis: Axis, normal: Option<f64>, p: Pos2, q: Pos2) -> Vec2 {
    let d = q - p;
    if d.length() > 0.5 && d.x.is_finite() && d.y.is_finite() {
        return d.normalized();
    }
    match (normal, axis) {
        // The line's own direction is a quarter turn from its normal; the screen's y runs down.
        (Some(n), _) => vec2(-(n.sin() as f32), -(n.cos() as f32)),
        (None, Axis::X) => vec2(0.0, -1.0),
        (None, Axis::Y) => vec2(1.0, 0.0),
    }
}

/// A label that waits to be placed: its text, the line it belongs to (where its arrow, ray or side is on the screen), and whether the text runs along that line (a gap of a
/// turned row) or stays level.
struct Tag {
    text: String,
    line: (Pos2, Pos2),
    along: bool,
}

/// Paints what a drag snapped to (`snapping.rs`), from the engine's marks, each by its segment: a guide pulled to is lit across the drawing area (a slanted one along its own
/// direction); every other line (an alignment, a margin, a grid or ruler line, a side of a turned shape's own) is a dashed line from the dragged shape to the nearest shape
/// on it, a little past each; an equal gap is a double arrow with end ticks and a label of its length, along the row (a turned row's label turned with it); a place is a
/// ring; a size is the length marked on the side that has it and on the side of the shape it matches, with its label; an angle is a ray from the pin, with its label.
/// Labels are placed so that none overlaps another (`snap::label::place`).
pub(super) fn paint_marks(painter: &Painter, t: &Tokens, cx: &Cx, marks: &[Mark]) {
    let line = Stroke::new(1.0, t.snap_mark);
    let mut tags: Vec<Tag> = Vec::new();
    for mark in marks {
        crate::work::count(|w| w.marks_painted += 1);
        match mark {
            Mark::Line { axis, normal, from, to, targets, .. } => {
                let (p, q) = (cx.screen_at(*from), cx.screen_at(*to));
                if !(p.x.is_finite() && p.y.is_finite() && q.x.is_finite() && q.y.is_finite()) {
                    continue;
                }
                let dir = line_direction(*axis, *normal, p, q);
                if targets.contains(&SnapTarget::Guide) {
                    // A guide is lit across the drawing area, whatever way it runs.
                    let far = cx.area.size().length();
                    painter.line_segment([p - dir * far, q + dir * far], Stroke::new(2.0, t.snap_mark));
                } else {
                    painter.extend(Shape::dashed_line(&[p - dir * MARK_OVERHANG, q + dir * MARK_OVERHANG], line, DASH, GAP));
                }
            }
            Mark::Gap { normal, from, to, label, .. } => {
                let (a, b) = (cx.screen_at(*from), cx.screen_at(*to));
                if !(a.x.is_finite() && a.y.is_finite() && b.x.is_finite() && b.y.is_finite()) {
                    continue;
                }
                paint_dimension(painter, line, a, b);
                tags.push(Tag { text: label.clone(), line: (a, b), along: normal.is_some() });
            }
            Mark::Place { at, .. } => {
                let c = cx.screen_at(*at);
                if c.x.is_finite() && c.y.is_finite() {
                    painter.circle_stroke(c, MARK_RADIUS, Stroke::new(1.5, t.snap_mark));
                    painter.circle_filled(c, 1.5, t.snap_mark);
                }
            }
            Mark::Size { label, side, matched, .. } => {
                let (a, b) = (cx.screen_at(side.0), cx.screen_at(side.1));
                if a.x.is_finite() && a.y.is_finite() && b.x.is_finite() && b.y.is_finite() {
                    paint_dimension(painter, line, a, b);
                    tags.push(Tag { text: label.clone(), line: (a, b), along: false });
                }
                if let Some((_, other)) = matched {
                    let (c, d) = (cx.screen_at(other.0), cx.screen_at(other.1));
                    if c.x.is_finite() && c.y.is_finite() && d.x.is_finite() && d.y.is_finite() {
                        paint_dimension(painter, line, c, d);
                    }
                }
            }
            Mark::Angle { pivot, degrees, label, .. } => {
                let p = cx.screen_at(*pivot);
                let radians = degrees.to_radians();
                if !(p.x.is_finite() && p.y.is_finite() && radians.is_finite()) {
                    continue;
                }
                // The page's y runs up, the screen's down.
                let end = p + vec2(radians.cos() as f32, -(radians.sin() as f32)) * ANGLE_RAY;
                painter.extend(Shape::dashed_line(&[p, end], line, DASH, GAP));
                tags.push(Tag { text: label.clone(), line: (p, end), along: false });
            }
        }
    }
    paint_labels(painter, t, tags);
}

/// A length marked on the screen from `a` to `b`: a line with an arrowhead and a tick across it at each end.
fn paint_dimension(painter: &Painter, stroke: Stroke, a: Pos2, b: Pos2) {
    painter.line_segment([a, b], stroke);
    let d = b - a;
    let length = d.length();
    if !(length.is_finite() && length > 0.5) {
        return;
    }
    let (u, n) = (d / length, vec2(-d.y, d.x) / length);
    for (end, way) in [(a, 1.0_f32), (b, -1.0_f32)] {
        painter.line_segment([end - n * TICK, end + n * TICK], stroke);
        // Heads only where the arrow is longer than they are.
        if length > 3.0 * ARROW {
            let back = end + u * (way * ARROW);
            painter.line_segment([end, back + n * (ARROW * 0.5)], stroke);
            painter.line_segment([end, back - n * (ARROW * 0.5)], stroke);
        }
    }
}

/// Draws the labels of the marks on plates, each where `snap::label::place` puts it: centred on its line when it fits, else beside it, none on another.
fn paint_labels(painter: &Painter, t: &Tokens, tags: Vec<Tag>) {
    if tags.is_empty() {
        return;
    }
    let font = theme::font(LABEL_TEXT);
    let mut shown: Vec<(Arc<egui::Galley>, f32)> = Vec::new();
    let mut lines = Vec::new();
    for tag in &tags {
        let galley = painter.layout_no_wrap(tag.text.clone(), font.clone(), t.text);
        let size = galley.size() + vec2(2.0 * LABEL_PAD, 2.0 * LABEL_PAD);
        // The text of a gap along a turned row runs along it, kept right way up: an angle from just over a quarter turn down to a quarter turn up.
        let d = tag.line.1 - tag.line.0;
        let mut angle = if tag.along && d.length() > 0.5 { d.y.atan2(d.x) } else { 0.0 };
        if angle > std::f32::consts::FRAC_PI_2 {
            angle -= std::f32::consts::PI;
        } else if angle < -std::f32::consts::FRAC_PI_2 {
            angle += std::f32::consts::PI;
        }
        let (s, c) = (angle.sin().abs(), angle.cos().abs());
        let boxed = (f64::from(size.x * c + size.y * s), f64::from(size.x * s + size.y * c));
        lines.push(((f64::from(tag.line.0.x), f64::from(tag.line.0.y)), (f64::from(tag.line.1.x), f64::from(tag.line.1.y)), boxed));
        shown.push((galley, angle));
    }
    let placed = diacraft_engine::snap::label::place(&lines, &[]);
    for ((galley, angle), at) in shown.into_iter().zip(placed) {
        crate::work::count(|w| w.labels_painted += 1);
        let centre = pos2(((at[0] + at[2]) / 2.0) as f32, ((at[1] + at[3]) / 2.0) as f32);
        if !(centre.x.is_finite() && centre.y.is_finite()) {
            continue;
        }
        let (half_w, half_h) = (galley.size().x / 2.0 + LABEL_PAD, galley.size().y / 2.0 + LABEL_PAD);
        let (s, c) = angle.sin_cos();
        let turn = |x: f32, y: f32| centre + vec2(x * c - y * s, x * s + y * c);
        // The plate the text stands on, turned with it.
        painter.add(Shape::convex_polygon(
            vec![turn(-half_w, -half_h), turn(half_w, -half_h), turn(half_w, half_h), turn(-half_w, half_h)],
            t.canvas_fill,
            Stroke::new(1.0, t.snap_mark),
        ));
        // The text's top left corner, where a turn about it puts the text's middle on the plate's.
        let corner = turn(-galley.size().x / 2.0, -galley.size().y / 2.0);
        painter.add(Shape::Text(egui::epaint::TextShape::new(corner, galley, t.text).with_angle(angle)));
    }
}

/// The side of a handle's square of a text block (the Text Block tool), in points. The handles of a selected shape are the selection look's (`select_look.rs`, `theme::select`).
pub(super) const HANDLE: f32 = 7.0;
/// The radius of the mark at a connector's end, in points.
const END_RADIUS: f32 = 5.0;
/// The radius of the rotation handle of a text block, in points.
pub(super) const ROTATION_RADIUS: f32 = 6.5;
/// The reach of a connection point's cross, and the radius of the highlight under the pointer, in points.
const POINT_ARM: f32 = 3.0;
const POINT_HIGHLIGHT: f32 = 6.0;
/// The most shapes whose connection points are marked at once when the Connector tool is on.
const MAX_MARKED: usize = 300;
/// The most connection points marked in a frame, the point under the pointer apart (a file can turn View > Connection Points on,
/// and 300 shapes of 256 points are 76,800 crosses a frame). Two line segments each: measured at 0.29 ms a frame for the 4,096 (a kittest frame of
/// 300 instances of 200 points, 0.46 ms with them and 0.18 ms without, optimised test build). More than a page a person reads shows (a 30 by 30
/// grid of four-point shapes is 3,600).
pub const MAX_POINTS_MARKED: usize = 4_096;
/// Dashes of an outline that is a promise and not yet a fact, in points.
pub(super) const DASH: f32 = 5.0;
pub(super) const GAP: f32 = 3.0;

/// The paths that stand for an item in a preview: what the page paints of its geometry (`Item::painted_paths`: a section the shape hides is not there), or its box when it paints none. A
/// shape whose geometry is all hidden (a marker of a group's member) has no part in the preview.
pub(super) fn preview_paths(item: &Item) -> Vec<Vec<P>> {
    let painted: Vec<Vec<P>> = item.painted_paths().cloned().collect();
    if !painted.is_empty() {
        painted
    } else if item.hidden {
        Vec::new()
    } else {
        vec![item.corners.to_vec()]
    }
}

/// The most shapes a preview of a drag draws: a group of thousands of members is drawn by its box and the first of them, as a page of thousands of shapes is not read at all.
const MAX_PREVIEWED: usize = 2_000;

/// The items that stand for `ids` in a preview: a shape or a connector itself, and for a group the shapes in it (a group draws nothing of its own), to any depth the scene
/// read, at most [`MAX_PREVIEWED`] of them. A group none of whose members were read (it is set to group only) is drawn by its box.
pub(super) fn preview_items<'a>(scene: &'a Scene, ids: &[u32]) -> Vec<&'a Item> {
    let mut out: Vec<&Item> = Vec::new();
    let mut waiting: Vec<&Item> = ids.iter().rev().filter_map(|id| scene.get(*id)).collect();
    let mut looked = 0_usize;
    while let Some(item) = waiting.pop() {
        looked += 1;
        if out.len() >= MAX_PREVIEWED || looked > 4 * MAX_PREVIEWED {
            break;
        }
        if item.kind == Kind::Group {
            let before = waiting.len();
            waiting.extend(scene.children_of(item.id).collect::<Vec<_>>().into_iter().rev());
            if waiting.len() == before {
                out.push(item);
            }
        } else {
            out.push(item);
        }
    }
    out
}

fn screen_path(cx: &Cx, path: &[P]) -> Vec<Pos2> {
    path.iter().map(|p| cx.screen_at(*p)).collect()
}

/// A path as a shape: filled lightly and outlined when it encloses something, a line otherwise.
fn paint_path(painter: &Painter, t: &Tokens, points: Vec<Pos2>, stroke: Stroke) {
    if points.len() >= 3 {
        painter.add(Shape::closed_line(points, stroke));
    } else if points.len() == 2 {
        painter.add(Shape::line(points, stroke));
    }
    let _ = t;
}

/// Fills a closed path with the preview colour (concave outlines are filled as their convex hull would be, which is fine for a preview
/// drawn this lightly).
fn fill_path(painter: &Painter, t: &Tokens, points: &[Pos2]) {
    if points.len() >= 3 {
        painter.add(PathShape::convex_polygon(points.to_vec(), t.edit_fill, Stroke::NONE));
    }
}

fn dashed(painter: &Painter, points: &[Pos2], stroke: Stroke, closed: bool) {
    let mut line = points.to_vec();
    if closed && let Some(first) = points.first() {
        line.push(*first);
    }
    painter.extend(Shape::dashed_line(&line, stroke, DASH, GAP));
}

impl DiacraftApp {
    /// Paints the editing layer of the page `cx.page`, over the picture. The painter is clipped to the drawing area. The net of a drag from empty page is
    /// painted last, over the shapes and every mark; it shows from the first movement.
    pub(crate) fn paint_edit(&self, painter: &Painter, t: &Tokens, cx: &Cx) {
        self.paint_edit_marks(painter, t, cx);
        // AutoConnect's arrows, flyout and the preview of the master under the pointer, over the marks (`autoconnect.rs`).
        self.paint_auto(painter, t, cx);
        // The markers of the comments of the page (`comments_ui.rs`); they are the window's, over the picture, and no part of what a print or an export draws.
        self.paint_comment_markers(painter, t, cx);
        let net = self.ui.edit.gesture.as_ref().filter(|g| matches!(g.mode, Mode::Marquee { .. }) && g.now != g.origin);
        if let Some(gesture) = net.filter(|_| self.ui.edit.scene.as_ref().is_some_and(|s| s.page == cx.page)) {
            select_look::paint_net(painter, t, cx, gesture);
        }
        // The Lasso Select tool's trail: a thin dotted line behind the crosshair, no veil and no box.
        if let Some(Mode::Lasso { trail, .. }) = self.ui.edit.gesture.as_ref().filter(|g| g.active).map(|g| &g.mode) {
            select_look::paint_trail(painter, t, cx, trail);
        }
    }

    fn paint_edit_marks(&self, painter: &Painter, t: &Tokens, cx: &Cx) {
        let edit = &self.ui.edit;
        let (Some(scene), Some(doc)) = (&edit.scene, self.session.document()) else {
            self.paint_pending(painter, t, cx);
            return;
        };
        if scene.page != cx.page {
            return;
        }
        let moving = edit.gesture.as_ref().filter(|g| g.active);
        // The selection stays painted where it was while a move is previewed elsewhere; the preview is what shows the new place.
        // The Text Block tool shows the block's handles and not the shape's.
        let with_handles = moving.is_none() && edit.tool != Tool::TextBlock;
        select_look::paint_selection(painter, t, cx, &self.selected_items(scene), with_handles);
        // The container of a member selected shows (`container_ui.rs`).
        self.paint_container_glow(painter, t, cx);
        // The handles at the middle of the legs of the connector selected (`legs.rs`).
        if with_handles {
            self.paint_leg_handles(painter, t, cx);
            self.paint_tip_handle(painter, t, cx);
        }
        if edit.tool == Tool::TextBlock {
            self.paint_block(painter, t, cx);
        }
        // The connection points are marked while the Connector tool is on or a connector's end is dragged, with the one a release would glue to lit; and
        // with every tool while View > Connection Points is on, with none lit.
        let joining =
            edit.tool == Tool::Connector || moving.is_some_and(|g| matches!(g.mode, Mode::Join { .. } | Mode::Reshape { .. } | Mode::Arrow { .. }));
        if joining {
            let focus = moving.map(|g| g.target).or_else(|| edit.hover.map(|p| self.target_at(cx, p)));
            self.paint_points(painter, t, cx, scene, doc, focus);
        } else if self.ui.show_connection_points {
            self.paint_points(painter, t, cx, scene, doc, None);
        }
        if let Some(gesture) = moving {
            self.paint_gesture(painter, t, cx, scene, gesture);
        }
        // The ends of the path a drawing tool goes on from (`draw.rs`).
        self.paint_selected_vertices(painter, t, cx);
        self.paint_pending(painter, t, cx);
    }

    /// The outlines of an edit that has been made and whose picture has not come yet.
    fn paint_pending(&self, painter: &Painter, t: &Tokens, cx: &Cx) {
        let Some(after) = self.ui.edit.after.as_ref().filter(|a| a.page == cx.page) else { return };
        for path in &after.paths {
            let points = screen_path(cx, path);
            fill_path(painter, t, &points);
            paint_path(painter, t, points, Stroke::new(1.5, t.edit_line));
        }
    }

    /// Crosses at the connection points of the shapes in view, and the one under the pointer lit; the shape a connector would be glued to
    /// is outlined in green.
    fn paint_points(&self, painter: &Painter, t: &Tokens, cx: &Cx, scene: &Scene, doc: &diacraft_engine::doc::Document, focus: Option<Target>) {
        let view = cx.view.visible(cx.area);
        let shown = scene.items.iter().filter(|i| i.kind != Kind::Connector).filter(|i| geom::overlaps(i.bounds, view)).take(MAX_MARKED);
        let lit = match focus {
            Some(Target::Point { id, index, at }) => Some((id, index, at)),
            _ => None,
        };
        // The crosses, at most MAX_POINTS_MARKED of them a frame (the shapes first in the page's order); the point a release would glue to is drawn
        // after them, whatever the count.
        let mut left = MAX_POINTS_MARKED;
        'shapes: for item in shown {
            for point in scene.points(doc, item.id) {
                if lit.is_some_and(|(id, index, _)| (id, index) == (item.id, point.index)) {
                    continue;
                }
                if left == 0 {
                    break 'shapes;
                }
                left -= 1;
                crate::work::count(|w| w.points_marked += 1);
                let at = cx.screen_at(point.at);
                let stroke = Stroke::new(1.25, t.glue_point);
                painter.line_segment([at + vec2(-POINT_ARM, -POINT_ARM), at + vec2(POINT_ARM, POINT_ARM)], stroke);
                painter.line_segment([at + vec2(-POINT_ARM, POINT_ARM), at + vec2(POINT_ARM, -POINT_ARM)], stroke);
            }
        }
        if let Some((.., at)) = lit {
            crate::work::count(|w| w.points_marked += 1);
            let at = cx.screen_at(at);
            painter.circle_filled(at, POINT_HIGHLIGHT + 4.0, t.glue_halo);
            painter.circle(at, POINT_HIGHLIGHT, t.glue_on, Stroke::new(1.5, t.handle_fill));
        }
        if let Some(Target::Shape(id)) = focus
            && let Some(item) = scene.get(id)
        {
            for path in preview_paths(item) {
                let points = screen_path(cx, &path);
                painter.add(PathShape::convex_polygon(points.clone(), t.glue_halo, Stroke::NONE));
                painter.add(Shape::closed_line(points, Stroke::new(2.0, t.glue_on)));
            }
        }
    }

    fn paint_gesture(&self, painter: &Painter, t: &Tokens, cx: &Cx, scene: &Scene, gesture: &Gesture) {
        match &gesture.mode {
            // The net and the lasso's trail are painted over everything, from the first movement, by `paint_edit`.
            Mode::Marquee { .. } | Mode::Lasso { .. } => {}
            Mode::Move { ids, .. } => {
                let (dx, dy) = gesture.move_delta();
                for item in ids.iter().filter_map(|id| scene.get(*id)).filter(|i| i.moves_by_itself()) {
                    for path in preview_paths(item) {
                        let moved: Vec<P> = path.iter().map(|p| (p.0 + dx, p.1 + dy)).collect();
                        let points = screen_path(cx, &moved);
                        fill_path(painter, t, &points);
                        paint_path(painter, t, points, Stroke::new(1.5, t.edit_line));
                    }
                }
                paint_marks(painter, t, cx, &gesture.landing.marks);
            }
            Mode::Guide { axis, at, held, .. } => {
                // The guide is drawn where it goes by `paint_guides`; the shapes glued to it go with it.
                let Some(to) = gesture.landing.guide_at else { return };
                let (dx, dy) = if *axis == Axis::X { (to - at, 0.0) } else { (0.0, to - at) };
                for item in held.iter().filter_map(|id| scene.get(*id)) {
                    for path in preview_paths(item) {
                        let moved: Vec<P> = path.iter().map(|p| (p.0 + dx, p.1 + dy)).collect();
                        let points = screen_path(cx, &moved);
                        fill_path(painter, t, &points);
                        paint_path(painter, t, points, Stroke::new(1.5, t.edit_line));
                    }
                }
            }
            Mode::Resize { id, handle, basis } => {
                // The drag works in the shape's own frame, so a turned shape's preview is turned too.
                let (lo, hi) = gesture.new_frame(*handle, *basis);
                let (w0, h0) = (basis.w.max(1e-9), basis.h.max(1e-9));
                let (sx, sy) = ((hi.0 - lo.0) / w0, (hi.1 - lo.1) / h0);
                for item in preview_items(scene, &[*id]) {
                    for path in preview_paths(item) {
                        let scaled: Vec<P> = path
                            .iter()
                            .map(|p| {
                                let q = basis.to_local(*p);
                                basis.to_page((lo.0 + q.0 * sx, lo.1 + q.1 * sy))
                            })
                            .collect();
                        let points = screen_path(cx, &scaled);
                        fill_path(painter, t, &points);
                        paint_path(painter, t, points, Stroke::new(1.5, t.edit_line));
                    }
                }
                dashed(painter, &screen_path(cx, &basis.corners_of(lo, hi)), Stroke::new(1.0, t.edit_line), true);
                paint_marks(painter, t, cx, &gesture.landing.marks);
            }
            Mode::Rotate { id, pin, grab, start } => {
                let to = gesture.turn_to(cx, *pin, *grab, *start);
                for item in preview_items(scene, &[*id]) {
                    for path in preview_paths(item) {
                        let turned: Vec<P> = path.iter().map(|p| geom::turned_about(*p, *pin, to - *start)).collect();
                        let points = screen_path(cx, &turned);
                        fill_path(painter, t, &points);
                        paint_path(painter, t, points, Stroke::new(1.5, t.edit_line));
                    }
                }
                // The pin the shape turns about, and the line from it to the pointer.
                let (at, pointer) = (cx.screen_at(*pin), cx.screen_at(gesture.now));
                painter.extend(Shape::dashed_line(&[at, pointer], Stroke::new(1.0, t.edit_line), DASH, GAP));
                painter.circle(at, 3.5, t.handle_fill, Stroke::new(1.5, t.edit_line));
                // The angle another shape has, or the step, that the turn comes to.
                paint_marks(painter, t, cx, &gesture.landing.marks);
            }
            Mode::ResizeSet { ids, handle, basis, margin } => {
                // The shapes are stretched as one: every place of them goes where the stretch that takes the joint box to the box the handle makes puts it (a shape turned by
                // other than a quarter keeps its angle in the result, which the preview does not show).
                let lo = basis.origin;
                let (nlo, nhi) = gesture.new_joint(*handle, *basis, *margin);
                let (sx, sy) = ((nhi.0 - nlo.0) / basis.w.max(1e-9), (nhi.1 - nlo.1) / basis.h.max(1e-9));
                for item in preview_items(scene, ids) {
                    for path in preview_paths(item) {
                        let stretched: Vec<P> = path.iter().map(|p| (nlo.0 + (p.0 - lo.0) * sx, nlo.1 + (p.1 - lo.1) * sy)).collect();
                        let points = screen_path(cx, &stretched);
                        fill_path(painter, t, &points);
                        paint_path(painter, t, points, Stroke::new(1.5, t.edit_line));
                    }
                }
                let box_corners = [nlo, (nhi.0, nlo.1), nhi, (nlo.0, nhi.1)];
                dashed(painter, &screen_path(cx, &box_corners), Stroke::new(1.0, t.edit_line), true);
                paint_marks(painter, t, cx, &gesture.landing.marks);
            }
            Mode::RotateSet { ids, pin, .. } => {
                // The shapes turn as one about the middle of their joint box, by the step the preview shows.
                let by = gesture.landing.turn.filter(|_| gesture.landing.worked_out()).map_or(0.0, |turning| turning.to);
                for item in preview_items(scene, ids) {
                    for path in preview_paths(item) {
                        let turned: Vec<P> = path.iter().map(|p| geom::turned_about(*p, *pin, by)).collect();
                        let points = screen_path(cx, &turned);
                        fill_path(painter, t, &points);
                        paint_path(painter, t, points, Stroke::new(1.5, t.edit_line));
                    }
                }
                let (at, pointer) = (cx.screen_at(*pin), cx.screen_at(gesture.now));
                painter.extend(Shape::dashed_line(&[at, pointer], Stroke::new(1.0, t.edit_line), DASH, GAP));
                painter.circle(at, 3.5, t.handle_fill, Stroke::new(1.5, t.edit_line));
            }
            Mode::BlockMove { .. } | Mode::BlockResize { .. } | Mode::BlockRotate { .. } => self.paint_block_gesture(painter, t, cx, gesture),
            Mode::Draw { .. } => self.paint_draw(painter, t, cx, gesture),
            Mode::TextBox { on: None } => self.paint_text_box_drag(painter, t, cx, gesture),
            Mode::TextBox { on: Some(_) } => {}
            Mode::Join { from } => self.paint_join(painter, t, cx, scene, *from, gesture),
            // A drag from an arrow: the line from the middle of its side to where the pointer is, green over a shape it would join.
            Mode::Arrow { anchor, .. } => self.paint_rubber_line(painter, t, cx, *anchor, gesture),
            Mode::Leg { .. } => self.paint_leg_gesture(painter, t, cx, gesture),
            Mode::Tip { .. } => self.paint_tip_gesture(painter, t, cx, gesture),
            Mode::Reshape { id, end } => {
                let Some((begin, last)) = scene.get(*id).and_then(|i| i.ends) else { return };
                let fixed = if *end == Which::Begin { last } else { begin };
                self.paint_rubber_line(painter, t, cx, fixed, gesture);
            }
        }
    }

    fn paint_join(&self, painter: &Painter, t: &Tokens, cx: &Cx, scene: &Scene, from: Target, gesture: &Gesture) {
        // A free begin goes where the snap puts it.
        let start = match gesture.landing.start.unwrap_or(from) {
            Target::Free(p) | Target::Point { at: p, .. } => p,
            Target::Shape(id) => scene.get(id).map_or(gesture.origin, Item::centre),
        };
        self.paint_rubber_line(painter, t, cx, start, gesture);
    }

    /// The straight line from where a connector is fixed to where it would end (a free end where the snap puts it), green when that end would be
    /// glued, and what the snap pulled the end to.
    fn paint_rubber_line(&self, painter: &Painter, t: &Tokens, cx: &Cx, fixed: P, gesture: &Gesture) {
        let target = gesture.landing.end.filter(|_| gesture.landing.worked_out()).unwrap_or(gesture.target);
        let end = match target {
            Target::Point { at, .. } | Target::Free(at) => at,
            Target::Shape(_) => gesture.now,
        };
        let free = matches!(target, Target::Free(_));
        let colour = if free { t.edit_line } else { t.glue_on };
        let points = [cx.screen_at(fixed), cx.screen_at(end)];
        painter.extend(Shape::dashed_line(&points, Stroke::new(1.75, colour), DASH, GAP));
        if free {
            paint_marks(painter, t, cx, &gesture.landing.marks);
        }
        painter.circle(cx.screen_at(end), END_RADIUS, if free { t.glue_off } else { t.glue_on }, Stroke::new(1.5, t.edit_line));
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use serde_json::json;

    use super::*;

    /// What a drag of a group draws is what its members paint: a BPMN gateway is its diamond and the box its name is typed in, and the markers it hides at its default Shape Data are not drawn
    /// (nor their boxes).
    #[test]
    fn the_preview_of_a_group_draws_what_its_members_paint_and_nothing_of_the_markers_they_hide() {
        let mut session = diacraft_engine::Session::new();
        session.execute("file.new", &json!({})).unwrap();
        let page = session.document().unwrap().pages.get(0).unwrap().id;
        let dropped = session.execute("shape.drop", &json!({"master": "bpmn.gateway", "page": page, "x": 4.0, "y": 3.0})).unwrap();
        let id = u32::try_from(dropped["id"].as_u64().unwrap()).unwrap();
        let scene = Scene::build_all(session.document().unwrap(), page, session.revision()).unwrap();
        let items = preview_items(&scene, &[id]);
        assert!(items.iter().any(|item| item.hidden), "the gateway has hidden markers");
        for item in items.iter().filter(|item| item.hidden) {
            assert!(preview_paths(item).is_empty(), "a marker nobody can see has no part in a preview");
        }
        let paths: Vec<Vec<P>> = items.iter().copied().flat_map(preview_paths).collect();
        assert_eq!(paths.len(), 2, "the diamond, and the box of the name: {paths:?}");
        let mut lengths: Vec<usize> = paths.iter().map(Vec::len).collect();
        lengths.sort_unstable();
        assert_eq!(lengths, [4, 5], "a box has four corners and the diamond's outline comes back to its first: {paths:?}");
    }
}
