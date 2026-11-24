//! Pan & Zoom: a small picture of the page with a rectangle for the part of it the drawing area shows.
//!
//! **The rectangle is the view.** Drag it and the page pans (`view.pan`); press the page outside it and the view centres there; drag a corner of it and the
//! view zooms about its middle (`view.zoom`), the rectangle growing as the zoom falls and shrinking as it rises; the slider under the picture sets the zoom
//! (`view.zoom`). The pane changes no drawing: it asks the same window commands the wheel, the middle button and the status bar's slider ask.
//!
//! The picture shows the **outlines of the shapes** (from the scene the editing code keeps), not the rendered page: the rendered picture is made for the
//! drawing area, and a second one for a few dozen pixels would cost the render worker a job each time a shape moves. It reaches a fifth of a page's size
//! past the paper on every side, so the rectangle stays on view when the page is panned a little off. A rectangle larger than the picture (zoomed far out) is
//! cut at its edge.
//!
//! **The work is bounded** (counted by tests, `work.rs`): the lines of the picture are made from the scene once for each revision of the drawing ([`Thumb`]),
//! with at most [`PATH_POINTS`] points to a path, [`PATHS_PER_SHAPE`] paths to a shape and [`POINT_BUDGET`] points in all. **Every shape has its box first**
//! (five points), and what the budget has over is shared equally among the shapes as detail (a path longer than its share is thinned, its ends kept); the
//! picture says under it how many shapes are drawn simplified, or left out when not even their boxes fit. Each frame only moves those points into the
//! picture's place and draws them, and looks the selection up in a set.

use std::collections::HashSet;

use egui::{CornerRadius, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, pos2, vec2};
use serde_json::json;

use crate::app::DiacraftApp;
use crate::canvas::view::{MAX_ZOOM, MIN_ZOOM, POINTS_PER_INCH, View};
use crate::edit::geom::P;
use crate::edit::scene::Scene;
use crate::theme::{self, Tokens, size};
use crate::widgets::tip;
use crate::work;

/// What the picture reaches past the paper, as a share of the page's size, on each side.
const REACH: f64 = 0.2;
/// The tallest the picture is, in points.
const MAX_H: f32 = 190.0;
/// How near a press must be to a corner of the rectangle to take it, in points.
const GRAB: f32 = 8.0;
/// The most points one path of a shape keeps in the picture, the most paths a shape keeps, and the most points the whole picture holds.
pub(crate) const PATH_POINTS: usize = 32;
pub(crate) const PATHS_PER_SHAPE: usize = 8;
pub(crate) const POINT_BUDGET: usize = work::THUMB_POINT_BUDGET;
/// The points of a shape's box: what every shape has in the picture before any shape has detail.
const BOX_POINTS: usize = 5;

/// One line of the picture: a shape's path, in inches of the page.
#[derive(Clone, Debug)]
struct Line {
    id: u32,
    points: Vec<P>,
}

/// The lines of the picture for one revision of the drawing.
#[derive(Debug, Default)]
pub(crate) struct Thumb {
    /// The drawing's generation, the page, the revision of the scene it was made from and how many shapes the scene held.
    key: (u64, u32, u64, usize),
    lines: Vec<Line>,
    /// How many points the lines hold; how many shapes are drawn in less detail than they have (thinned, or only their box); and how many were left out
    /// because even their boxes did not fit the budget.
    pub points: usize,
    pub simplified: usize,
    pub left_out: usize,
}

/// The points of a path thinned to at most `max`, the first and the last kept.
fn thin(path: &[P], max: usize) -> Vec<P> {
    if path.len() <= max || max < 2 {
        return path.iter().take(max.max(1)).copied().collect();
    }
    let stride = (path.len() - 1).div_ceil(max - 1);
    let mut kept: Vec<P> = path.iter().step_by(stride).copied().collect();
    if let Some(last) = path.last()
        && kept.last() != Some(last)
    {
        if kept.len() >= max {
            kept.pop();
        }
        kept.push(*last);
    }
    kept
}

/// The box of a shape as a closed line: its four corners and the first again.
fn box_of(corners: &[P; 4]) -> Vec<P> {
    let mut line: Vec<P> = corners.to_vec();
    line.push(corners[0]);
    line
}

impl Thumb {
    /// Makes the lines of the picture from the scene, in the scene's order. **Every shape has a box first** ([`BOX_POINTS`] points): the budget holds the boxes
    /// of the shapes not yet drawn, and what is over that is shared equally among them as detail, a shape taking less than its share leaving more for the
    /// next. A shape with more detail than its share is drawn thinned (or, when the share is too small for its paths, as its box) and counted in
    /// `simplified`; shapes are left out only when even the boxes do not fit, and counted in `left_out`. The picture says both under it.
    pub(crate) fn build(scene: &Scene, generation: u64) -> Thumb {
        let n = scene.items.len();
        let mut lines = Vec::new();
        let (mut points, mut simplified, mut left_out) = (0usize, 0usize, 0usize);
        for (i, item) in scene.items.iter().enumerate() {
            let left = POINT_BUDGET.saturating_sub(points);
            if left < BOX_POINTS {
                left_out += 1;
                continue;
            }
            let remaining = n - i;
            let allowance = BOX_POINTS + left.saturating_sub(BOX_POINTS * remaining) / remaining;
            let paths: Vec<&Vec<P>> = item.outline.iter().filter(|p| p.len() >= 2).take(PATHS_PER_SHAPE).collect();
            let wanted: usize = paths.iter().map(|p| p.len().min(PATH_POINTS)).sum();
            let shown: Vec<Vec<P>> = if paths.is_empty() {
                vec![box_of(&item.corners)]
            } else if wanted <= allowance {
                paths.iter().map(|p| thin(p, PATH_POINTS)).collect()
            } else {
                let each = allowance / paths.len();
                if each >= 2 { paths.iter().map(|p| thin(p, each)).collect() } else { vec![box_of(&item.corners)] }
            };
            // Simplified: drawn with less than its whole outline, whatever cut it (the budget, the 32 points of a path, the 8 paths of a shape).
            let drawn: usize = shown.iter().map(Vec::len).sum();
            let whole: usize = item.outline.iter().filter(|p| p.len() >= 2).map(Vec::len).sum();
            if !item.outline.is_empty() && drawn < whole {
                simplified += 1;
            }
            for line in shown {
                points += line.len();
                lines.push(Line { id: item.id, points: line });
            }
        }
        Thumb { key: (generation, scene.page, scene.revision, scene.items.len()), lines, points, simplified, left_out }
    }

    /// What the picture says under it when it is not the whole of the drawing, or nothing.
    fn words(&self) -> Option<String> {
        let shapes = |n: usize| if n == 1 { "1 shape is".to_owned() } else { format!("{n} shapes are") };
        match (self.simplified, self.left_out) {
            (0, 0) => None,
            (s, 0) => Some(format!("{} drawn simplified.", shapes(s))),
            (0, l) => Some(format!("{} left out.", shapes(l))),
            (s, l) => Some(format!("{} drawn simplified and {l} left out.", shapes(s))),
        }
    }
}

/// The selection as a set, kept while the selection is the same.
#[derive(Debug, Default)]
struct SelectedSet {
    ids: Vec<u32>,
    set: HashSet<u32>,
}

/// What a drag in the picture has hold of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Grab {
    /// The inside of the rectangle (or the page, which first centres the view there): the view moves.
    Move,
    /// A corner of the rectangle: the view zooms.
    Corner,
}

/// What Pan & Zoom keeps between frames.
#[derive(Default)]
pub struct PanZoomState {
    grab: Option<Grab>,
    /// Where in the rectangle the drag took hold, from its middle: the rectangle follows the pointer by this, so that it does not jump to the pointer and
    /// does not lose the distance the pointer went before the drag began.
    offset: egui::Vec2,
    /// The picture's lines, made again when the drawing changes.
    thumb: Option<Thumb>,
    selected: SelectedSet,
}

impl PanZoomState {
    /// A drawing replaced the open one: the picture of the old one is not shown for the new, and a drag of the old one is over.
    pub(crate) fn forget_picture(&mut self) {
        self.thumb = None;
        self.selected = SelectedSet::default();
        self.grab = None;
    }
}

/// The picture's place on the screen and in the page: inches of the page (y up) on one side, points (y down) on the other.
#[derive(Clone, Copy, Debug)]
pub(super) struct Map {
    pub thumb: Rect,
    /// The lower left corner of what the picture shows, in inches.
    origin: (f64, f64),
    /// Points to one inch in the picture.
    pub k: f64,
}

impl Map {
    /// The map of a page of `page` inches into at most `room` points across and `MAX_H` high, with the reach of the paper round it. `None` when there is no room.
    pub(super) fn new(page: (f64, f64), room: f32, left_top: Pos2) -> Option<Map> {
        let (w, h) = (page.0 * (1.0 + 2.0 * REACH), page.1 * (1.0 + 2.0 * REACH));
        if !(w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0) || room < 8.0 {
            return None;
        }
        let k = (f64::from(room) / w).min(f64::from(MAX_H) / h);
        let thumb = Rect::from_min_size(left_top, vec2((w * k) as f32, (h * k) as f32));
        Some(Map { thumb, origin: (-page.0 * REACH, -page.1 * REACH), k })
    }

    pub(super) fn thumb_point(&self, p: (f64, f64)) -> Pos2 {
        pos2(self.thumb.left() + ((p.0 - self.origin.0) * self.k) as f32, self.thumb.bottom() - ((p.1 - self.origin.1) * self.k) as f32)
    }

    /// The rectangle of the part of the page `view` shows in a drawing area `area`, in the picture (not cut).
    pub(super) fn view_rect(&self, view: &View, area: Rect) -> Rect {
        let ((x0, y0), (x1, y1)) = view.visible(area);
        Rect::from_two_pos(self.thumb_point((x0, y1)), self.thumb_point((x1, y0)))
    }
}

/// What `view.pan` is to be told (screen points, to the right and down) to bring the middle of the view `by` picture points towards the right and down. The page
/// moves the other way, so both are negative (`View::pan_by`).
pub(super) fn pan_for(view: &View, map: &Map, by: egui::Vec2) -> (f64, f64) {
    let s = view.scale() / map.k;
    (-f64::from(by.x) * s, -f64::from(by.y) * s)
}

/// The zoom (a fraction) at which the drawing area `area` shows what a rectangle with half-sizes `half` (picture points) holds, whose shape is the area's:
/// the larger of the two sizes is taken, so the pointer keeps the corner on or outside the rectangle. `None` for a rectangle of no size or no number.
pub(super) fn zoom_for(map: &Map, area: Rect, half: egui::Vec2) -> Option<f64> {
    if !(half.x.is_finite() && half.y.is_finite()) {
        return None;
    }
    let aspect = f64::from(area.width()) / f64::from(area.height().max(1.0));
    let half_w = f64::from(half.x.abs()).max(f64::from(half.y.abs()) * aspect) / map.k;
    let width_in = half_w * 2.0;
    if !(width_in.is_finite() && width_in > 1e-6) {
        return None;
    }
    let zoom = f64::from(area.width()) / (width_in * POINTS_PER_INCH);
    zoom.is_finite().then_some(zoom.clamp(MIN_ZOOM, MAX_ZOOM))
}

impl DiacraftApp {
    pub(super) fn pan_zoom_body(&mut self, ui: &mut Ui, t: &Tokens, _body: Rect) {
        let Some(page) = self.current_page() else {
            ui.add(egui::Label::new(egui::RichText::new("No drawing open.").font(theme::font(size::LABEL)).color(t.text_dim)).wrap());
            return;
        };
        let page_size = self.current_page_size();
        let area = self.ui.canvas_rect;
        let view = self.ui.views.get(&page).copied().or_else(|| area.map(|a| View::fit_page(a, page_size)));
        let room = ui.available_width();
        let Some(map) = Map::new(page_size, room, ui.cursor().min) else {
            ui.label("The pane is too narrow.");
            return;
        };
        let (rect, resp) = ui.allocate_exact_size(map.thumb.size(), Sense::click_and_drag());
        let map = Map { thumb: rect, ..map };
        let enabled = view.is_some() && area.is_some();
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, "Page picture with the view rectangle"));
        self.note("pane.pan_zoom.thumb", rect, enabled);
        let shown = view.zip(area).map(|(view, area)| map.view_rect(&view, area));
        self.paint_thumb(ui, t, &map, page, page_size, shown);
        if let (Some(view), Some(area), Some(shown)) = (view, area, shown) {
            self.paint_view_rect(ui, t, &map, shown);
            self.pan_zoom_drag(ui, &resp, &map, &view, area, shown);
        }
        // When the picture is not the whole of the drawing, it says so under itself.
        let words = self.ui.panes.pan_zoom.thumb.as_ref().and_then(Thumb::words);
        if let Some(words) = words {
            ui.add_space(2.0);
            let r = ui.add(egui::Label::new(egui::RichText::new(&words).font(theme::font(size::LABEL - 1.0)).color(t.text_dim)).wrap());
            self.note("pane.pan_zoom.note", r.rect, false);
        }
        ui.add_space(8.0);
        self.zoom_row(ui, t);
    }

    /// The paper, the part in view tinted under the outlines of the shapes, and the outlines.
    fn paint_thumb(&mut self, ui: &mut Ui, t: &Tokens, map: &Map, page: u32, page_size: (f64, f64), shown: Option<Rect>) {
        self.ensure_scene(page);
        let painter = ui.painter_at(map.thumb);
        painter.rect_filled(map.thumb, CornerRadius::ZERO, t.canvas_fill);
        let paper = Rect::from_two_pos(map.thumb_point((0.0, page_size.1)), map.thumb_point((page_size.0, 0.0)));
        painter.rect_filled(paper, CornerRadius::ZERO, t.page_fill);
        painter.rect_stroke(paper, CornerRadius::ZERO, Stroke::new(1.0, t.page_border), StrokeKind::Outside);
        if let Some(cut) = shown.map(|r| r.intersect(map.thumb)).filter(|r| r.width() > 0.0 && r.height() > 0.0) {
            painter.rect_filled(cut, CornerRadius::ZERO, t.accent_soft);
        }
        // The lines are made again only when the scene is another: another page, another revision of the drawing.
        let generation = self.ui.generation;
        let current = self.ui.edit.scene().map(|s| (generation, s.page, s.revision, s.items.len()));
        if current.is_some() && self.ui.panes.pan_zoom.thumb.as_ref().map(|t| t.key) != current {
            self.ui.panes.pan_zoom.thumb = self.ui.edit.scene().map(|scene| Thumb::build(scene, generation));
            work::count(|w| {
                w.thumb_builds += 1;
                w.thumb_points = self.ui.panes.pan_zoom.thumb.as_ref().map_or(0, |t| t.points);
                w.thumb_simplified = self.ui.panes.pan_zoom.thumb.as_ref().map_or(0, |t| t.simplified);
                w.thumb_left_out = self.ui.panes.pan_zoom.thumb.as_ref().map_or(0, |t| t.left_out);
            });
        }
        let selection = self.ui.edit.selection();
        if self.ui.panes.pan_zoom.selected.ids != selection {
            self.ui.panes.pan_zoom.selected = SelectedSet { ids: selection.to_vec(), set: selection.iter().copied().collect() };
        }
        let mut painted = 0;
        if let Some(thumb) = &self.ui.panes.pan_zoom.thumb {
            let (plain, chosen) = (Stroke::new(1.0, t.text_dim), Stroke::new(1.5, t.accent));
            for line in &thumb.lines {
                let stroke = if self.ui.panes.pan_zoom.selected.set.contains(&line.id) { chosen } else { plain };
                let points: Vec<Pos2> = line.points.iter().map(|p| map.thumb_point(*p)).filter(|p| p.x.is_finite() && p.y.is_finite()).collect();
                painted += points.len();
                if points.len() >= 2 {
                    painter.add(Shape::line(points, stroke));
                }
            }
        }
        work::count(|w| w.thumb_painted = painted);
        painter.rect_stroke(map.thumb, CornerRadius::ZERO, Stroke::new(1.0, t.input_border), StrokeKind::Inside);
    }

    /// The rectangle of the view, with a handle at each corner that is in the picture.
    fn paint_view_rect(&mut self, ui: &mut Ui, t: &Tokens, map: &Map, shown: Rect) {
        let painter = ui.painter_at(map.thumb);
        painter.rect_stroke(shown, CornerRadius::ZERO, Stroke::new(1.5, t.accent), StrokeKind::Inside);
        self.note("pane.pan_zoom.view", shown, true);
        for corner in corners(shown) {
            if map.thumb.contains(corner) {
                let handle = Rect::from_center_size(corner, vec2(6.0, 6.0));
                painter.rect_filled(handle, CornerRadius::ZERO, t.handle_fill);
                painter.rect_stroke(handle, CornerRadius::ZERO, Stroke::new(1.0, t.accent), StrokeKind::Inside);
            }
        }
    }

    /// Reads the drag in the picture and sends `view.pan` or `view.zoom`.
    fn pan_zoom_drag(&mut self, ui: &mut Ui, resp: &egui::Response, map: &Map, view: &View, area: Rect, shown: Rect) {
        let near_corner = |p: Pos2| corners(shown).iter().any(|c| c.distance(p) <= GRAB);
        if resp.drag_started()
            && let Some(p) = ui.input(|i| i.pointer.press_origin())
        {
            let corner = near_corner(p);
            self.ui.panes.pan_zoom.grab = Some(if corner { Grab::Corner } else { Grab::Move });
            // A press inside the rectangle holds it where it was pressed; a press outside brings the middle of the view to the pointer.
            self.ui.panes.pan_zoom.offset = if !corner && shown.contains(p) { p - shown.center() } else { egui::Vec2::ZERO };
        }
        if resp.hovered()
            && let Some(p) = ui.input(|i| i.pointer.hover_pos())
        {
            let icon = if near_corner(p) {
                egui::CursorIcon::ResizeNwSe
            } else if shown.contains(p) {
                egui::CursorIcon::Grab
            } else {
                egui::CursorIcon::PointingHand
            };
            ui.ctx().set_cursor_icon(icon);
        }
        if resp.dragged() {
            match self.ui.panes.pan_zoom.grab {
                Some(Grab::Move) => {
                    if let Some(p) = resp.interact_pointer_pos() {
                        let by = p - self.ui.panes.pan_zoom.offset - shown.center();
                        if by.length() > 0.01 {
                            self.pan_view(pan_for(view, map, by));
                        }
                    }
                }
                Some(Grab::Corner) => {
                    if let Some(p) = resp.interact_pointer_pos()
                        && let Some(zoom) = zoom_for(map, area, p - shown.center())
                        && (zoom - view.zoom).abs() > 1e-6
                    {
                        let _ = self.execute("view.zoom", &json!({ "percent": zoom * 100.0 }));
                    }
                }
                None => {}
            }
        }
        if resp.drag_stopped() {
            self.ui.panes.pan_zoom.grab = None;
        }
        if resp.clicked()
            && let Some(p) = resp.interact_pointer_pos()
            && !near_corner(p)
        {
            self.pan_view(pan_for(view, map, p - shown.center()));
        }
    }

    fn pan_view(&mut self, by: (f64, f64)) {
        if by.0.is_finite() && by.1.is_finite() {
            let _ = self.execute("view.pan", &json!({ "dx": by.0, "dy": by.1 }));
        }
    }

    /// The zoom, in words and as a slider (the same track as the status bar's).
    fn zoom_row(&mut self, ui: &mut Ui, t: &Tokens) {
        let enabled = self.command_enabled("view.zoom").is_ok();
        let zoom = self.zoom();
        ui.horizontal(|ui| {
            ui.allocate_ui_with_layout(vec2(super::fields::LABEL_W, 22.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.set_min_size(vec2(super::fields::LABEL_W, 22.0));
                ui.label(egui::RichText::new("Zoom").font(theme::font(size::LABEL)).color(if enabled { t.text } else { t.text_disabled }));
            });
            ui.label(egui::RichText::new(format!("{:.0}%", zoom * 100.0)).font(theme::font(size::LABEL)).color(t.text));
        });
        let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), if enabled { Sense::click_and_drag() } else { Sense::hover() });
        let percent = zoom * 100.0;
        resp.widget_info(|| egui::WidgetInfo::drag_value(enabled, percent));
        let track = Rect::from_min_max(pos2(r.left() + 7.0, r.center().y - 1.5), pos2(r.right() - 7.0, r.center().y + 1.5));
        let (lo, hi) = (MIN_ZOOM * 100.0, MAX_ZOOM * 100.0);
        if enabled
            && (resp.dragged() || resp.clicked())
            && let Some(p) = resp.interact_pointer_pos()
        {
            let f = f64::from(((p.x - track.left()) / track.width()).clamp(0.0, 1.0));
            let target = lo + f * (hi - lo);
            if (target - percent).abs() > 1e-6 {
                let _ = self.execute("view.zoom", &json!({ "percent": target }));
            }
        }
        let painter = ui.painter();
        painter.rect_filled(track, CornerRadius::same(2), t.input_border);
        let f = ((percent - lo) / (hi - lo)).clamp(0.0, 1.0) as f32;
        let x = track.left() + f * track.width();
        painter.rect_filled(
            Rect::from_min_max(track.min, pos2(x, track.max.y)),
            CornerRadius::same(2),
            if enabled { t.accent } else { t.text_disabled },
        );
        painter.circle(
            pos2(x, r.center().y),
            6.0,
            if enabled { t.page_fill } else { t.icon_fill_disabled },
            Stroke::new(1.5, if enabled { t.accent } else { t.text_disabled }),
        );
        self.note("pane.pan_zoom.slider", r, enabled);
        resp.on_hover_text(tip("Zoom", enabled));
    }
}

fn corners(r: Rect) -> [Pos2; 4] {
    [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()]
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn area() -> Rect {
        Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0))
    }

    #[test]
    fn the_note_under_the_picture_counts_in_the_singular_and_the_plural() {
        let said = |simplified, left_out| Thumb { simplified, left_out, ..Thumb::default() }.words();
        assert_eq!(said(0, 0), None);
        assert_eq!(said(1, 0).as_deref(), Some("1 shape is drawn simplified."));
        assert_eq!(said(2, 0).as_deref(), Some("2 shapes are drawn simplified."));
        assert_eq!(said(0, 1).as_deref(), Some("1 shape is left out."));
        assert_eq!(said(3, 2).as_deref(), Some("3 shapes are drawn simplified and 2 left out."));
        assert_eq!(said(1, 1).as_deref(), Some("1 shape is drawn simplified and 1 left out."));
    }

    #[test]
    fn a_long_path_is_thinned_to_the_most_a_path_keeps_with_its_ends_and_a_short_one_is_whole() {
        for len in [2usize, 5, 32, 33, 64, 1_000, 4_097] {
            let path: Vec<P> = (0..len).map(|i| (i as f64, (i * i % 7) as f64)).collect();
            let kept = thin(&path, PATH_POINTS);
            assert!(kept.len() <= PATH_POINTS && kept.len() >= 2, "{len}: {}", kept.len());
            assert_eq!(kept.first(), path.first());
            assert_eq!(kept.last(), path.last(), "the last point is kept: {len}");
            if len <= PATH_POINTS {
                assert_eq!(kept, path, "a short path is whole");
            }
        }
        assert!(thin(&[], PATH_POINTS).is_empty());
    }

    #[test]
    fn the_map_puts_the_page_in_the_middle_with_a_reach_round_it() {
        let m = Map::new((10.0, 8.0), 240.0, pos2(20.0, 30.0)).unwrap();
        assert!(m.thumb.width() <= 240.5 && m.thumb.height() <= MAX_H + 0.5, "{:?}", m.thumb);
        let low_left = m.thumb_point((0.0, 0.0));
        let high_right = m.thumb_point((10.0, 8.0));
        assert!(low_left.x > m.thumb.left() && high_right.x < m.thumb.right());
        assert!(low_left.y < m.thumb.bottom() && high_right.y > m.thumb.top());
        assert!(Map::new((10.0, 8.0), 4.0, pos2(0.0, 0.0)).is_none(), "no room");
        assert!(Map::new((0.0, 8.0), 200.0, pos2(0.0, 0.0)).is_none());
        assert!(Map::new((f64::NAN, 8.0), 200.0, pos2(0.0, 0.0)).is_none());
    }

    #[test]
    fn the_view_rectangle_is_the_visible_part_of_the_page() {
        let m = Map::new((10.0, 8.0), 240.0, pos2(0.0, 0.0)).unwrap();
        let view = View::fit_page(area(), (10.0, 8.0));
        let r = m.view_rect(&view, area());
        let paper = Rect::from_two_pos(m.thumb_point((0.0, 8.0)), m.thumb_point((10.0, 0.0)));
        assert!(r.contains_rect(paper.shrink(0.5)), "Fit shows the whole page: {r:?} {paper:?}");
        let zoomed = View { zoom: view.zoom * 2.0, ..view };
        let z = m.view_rect(&zoomed, area());
        assert!((z.width() * 2.0 - r.width()).abs() < 0.01, "twice the zoom, half the rectangle");
        assert!((z.center() - r.center()).length() < 0.01, "about the same middle");
    }

    #[test]
    fn dragging_the_rectangle_pans_the_page_the_other_way_by_the_same_inches() {
        let m = Map::new((10.0, 8.0), 240.0, pos2(0.0, 0.0)).unwrap();
        let mut view = View { zoom: 1.0, center: (5.0, 4.0), fitted: false };
        let before = m.view_rect(&view, area());
        let by = vec2(12.0, -7.0);
        let (dx, dy) = pan_for(&view, &m, by);
        view.pan_by(vec2(dx as f32, dy as f32));
        let after = m.view_rect(&view, area());
        let moved = after.center() - before.center();
        assert!((moved.x - by.x).abs() < 0.05 && (moved.y - by.y).abs() < 0.05, "{moved:?} for {by:?}");
    }

    #[test]
    fn dragging_a_corner_sets_the_zoom_that_shows_the_rectangle() {
        let m = Map::new((10.0, 8.0), 240.0, pos2(0.0, 0.0)).unwrap();
        let view = View { zoom: 1.0, center: (5.0, 4.0), fitted: false };
        let rect = m.view_rect(&view, area());
        // A corner of the rectangle itself asks for the zoom it already has.
        let asked = zoom_for(&m, area(), rect.right_bottom() - rect.center()).unwrap();
        assert!((asked - 1.0).abs() < 0.01, "{asked}");
        // A corner half as far out asks for twice the zoom.
        let near = zoom_for(&m, area(), (rect.right_bottom() - rect.center()) * 0.5).unwrap();
        assert!((near - 2.0).abs() < 0.02, "{near}");
        // Far out is held to the zoom range; the middle itself and a number that is none ask for nothing.
        assert_eq!(zoom_for(&m, area(), vec2(1.0e6, 1.0e6)), Some(MIN_ZOOM));
        assert_eq!(zoom_for(&m, area(), vec2(0.0, 0.0)), None);
        assert_eq!(zoom_for(&m, area(), vec2(f32::NAN, 1.0)), None);
    }
}
