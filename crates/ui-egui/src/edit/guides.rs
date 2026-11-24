//! Guides in the window: drawn on the drawing area, dragged out of a ruler, moved and deleted with the pointer and the keyboard.
//!
//! **A guide is a page shape** (`docs/editing-commands.md`), and every change to one is a command: a drag out of a ruler is `guide.add` at its
//! release, a drag of a guide is `guide.move`, Delete on a selected guide is `guide.delete`, each one undo step as the engine makes it. The lines drawn
//! are the engine's reading of the page for the snap (`SnapPage::guides`: a guide that lies along an axis, and `SnapPage::slanted`: a guide that lies along
//! none, which the spec documents the reference app to have, R12, and which a turned shape's own side parallel to it snaps to, R13: it is drawn like the others but has no
//! band to take hold of: it is not selected, moved or deleted here), so a guide is drawn where shapes snap to it.
//!
//! What the feature rows give: the View tab's Guides box hides or shows the guides of the
//! page on view; dragging from a ruler places a guide; Delete removes a selected one; a shape dragged to a guide is glued to it and follows it when it
//! moves. The rest is **ours** and listed in the task's report: the line is drawn across the whole drawing area (its geometry is an unbounded line,
//! and a guide can lie off the page); the top ruler gives a horizontal guide and the left one a vertical guide; a
//! guide is not snapped while it is placed or dragged (`guide.add` and `guide.move` take no `snap`); a shape under the pointer is taken before a guide
//! that runs across it; a guide is hidden, and cannot be taken hold of, while the Guides box is off; a drag out of a ruler with the box off shows the
//! guides again, so that the guide made is seen; the new guide is selected.

use diacraft_engine::snap::{Axis, GuideLine, SlantedGuide};
use egui::{CornerRadius, CursorIcon, Painter, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use serde_json::json;

use super::geom::P;
use super::snapping::sent;
use super::{Cx, HIT_REACH, inch};
use crate::app::DiacraftApp;
use crate::canvas::grid::ruler_label;
use crate::theme::{self, Tokens};

/// The width of a guide's line, of a selected one, and of the band under a selected one, in points.
const LINE: f32 = 1.0;
const SELECTED: f32 = 1.5;
const HALO: f32 = 7.0;

/// The `FromPart` numbers of the `Connect` rows that hold a shape to a guide (`AlignLeft` 1 to `AlignTop` 6).
const HELD_PARTS: std::ops::RangeInclusive<u32> = 1..=6;

/// A guide being dragged out of a ruler.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RulerDrag {
    /// The axis of the guide it makes: `Y` (a horizontal guide) from the top ruler, `X` (a vertical one) from the left ruler.
    pub axis: Axis,
    /// Where the guide would go, in paper inches along `axis`, while the pointer is over the drawing area; `None` over the ruler or elsewhere.
    pub at: Option<f64>,
}

/// The orientation `guide.add` takes for a guide of `axis`.
fn orientation(axis: Axis) -> &'static str {
    match axis {
        Axis::X => "vertical",
        Axis::Y => "horizontal",
    }
}

/// The value of `p` along `axis`.
fn along(axis: Axis, p: P) -> f64 {
    match axis {
        Axis::X => p.0,
        Axis::Y => p.1,
    }
}

/// The `ID`s of the shapes held to guide `id` on `page`: the shapes of the page's `Connect` rows from an alignment cell to it.
pub(crate) fn held_by(doc: &diacraft_engine::doc::Document, page: u32, id: u32) -> Vec<u32> {
    let Some(part) = doc.page(page) else { return Vec::new() };
    let mut held: Vec<u32> = part
        .connects
        .iter()
        .take(super::scene::MAX_CONNECTS)
        .filter(|c| c.to_sheet == id && c.from_part.is_some_and(|p| HELD_PARTS.contains(&p)))
        .map(|c| c.from_sheet)
        .collect();
    held.sort_unstable();
    held.dedup();
    held
}

impl DiacraftApp {
    /// The guides of `page` as the snap reads them, if they are read for the drawing as it is now.
    fn guides_now(&self, page: u32) -> &[GuideLine] {
        let key = (self.ui.generation, self.session.revision(), page);
        match &self.ui.edit.snap_page {
            Some(cache) if cache.key() == key => &cache.page.guides,
            _ => &[],
        }
    }

    /// The guides of `page` that lie along no axis, as the snap reads them, if they are read for the drawing as it is now.
    fn slanted_now(&self, page: u32) -> &[SlantedGuide] {
        let key = (self.ui.generation, self.session.revision(), page);
        match &self.ui.edit.snap_page {
            Some(cache) if cache.key() == key => &cache.page.slanted,
            _ => &[],
        }
    }

    /// The guide whose line `p` is within reach of (the nearest), when guides show. Reads the page as the last frame did.
    pub(super) fn guide_hit(&self, cx: &Cx, p: P) -> Option<GuideLine> {
        if !self.ui.show_guides {
            return None;
        }
        let reach = cx.reach(HIT_REACH);
        self.guides_now(cx.page)
            .iter()
            .map(|g| ((along(g.axis, p) - g.at).abs(), *g))
            .filter(|(d, _)| *d <= reach)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, g)| g)
    }

    /// The selected guide's line, if it is one of the page's.
    fn selected_guide(&self, page: u32) -> Option<GuideLine> {
        let id = self.ui.edit.guide?;
        self.guides_now(page).iter().find(|g| g.id == id).copied()
    }

    /// Selects guide `id` alone: the shapes are no longer selected.
    pub(super) fn select_guide(&mut self, page: u32, id: u32) {
        self.ui.edit.selection.clear();
        self.ui.edit.selection_page = Some(page);
        self.ui.edit.guide = Some(id);
    }

    /// Drops the guide selected if it is gone from the page (an undo took it away) or guides are hidden.
    pub(super) fn prune_guide(&mut self, page: u32) {
        if self.ui.edit.guide.is_none() {
            return;
        }
        if !self.ui.show_guides || !self.ensure_snap_page(page) || self.selected_guide(page).is_none() {
            self.ui.edit.guide = None;
        }
    }

    /// The Guides box of the View tab (`view.guides`). Hidden guides cannot be selected or dragged.
    pub(crate) fn set_show_guides(&mut self, on: bool) {
        self.ui.show_guides = on;
        if !on {
            self.ui.edit.guide = None;
            if matches!(self.ui.edit.gesture.as_ref().map(|g| &g.mode), Some(super::gesture::Mode::Guide { .. })) {
                self.ui.edit.gesture = None;
            }
        }
    }

    /// The screen point a guide's line passes through at the middle of the drawing area, and the band along it that takes the pointer.
    fn guide_band(cx: &Cx, axis: Axis, at: f64) -> Rect {
        let reach = HIT_REACH;
        match axis {
            Axis::X => {
                let x = cx.screen_at((at, 0.0)).x;
                Rect::from_min_max(pos2(x - reach, cx.area.top()), pos2(x + reach, cx.area.bottom()))
            }
            Axis::Y => {
                let y = cx.screen_at((0.0, at)).y;
                Rect::from_min_max(pos2(cx.area.left(), y - reach), pos2(cx.area.right(), y + reach))
            }
        }
    }

    /// Paints the guides of the page over its picture, the one selected or dragged marked, and a guide being dragged out of a ruler; and notes each
    /// guide's band as an element (`canvas.guide.<id>`) for the control channel. Nothing while guides are hidden.
    pub(crate) fn paint_guides(&mut self, painter: &Painter, t: &Tokens, cx: &Cx) {
        if !self.ui.show_guides || !self.ensure_snap_page(cx.page) {
            return;
        }
        let dragged = match self.ui.edit.gesture.as_ref() {
            Some(g) if g.active => match &g.mode {
                super::gesture::Mode::Guide { id, .. } => g.landing.guide_at.map(|at| (*id, at)),
                _ => None,
            },
            _ => None,
        };
        let lines: Vec<GuideLine> = self.guides_now(cx.page).to_vec();
        let mut bands = Vec::with_capacity(lines.len().min(64));
        for g in &lines {
            let moved = dragged.filter(|(id, _)| *id == g.id).map(|(_, at)| at);
            let at = moved.unwrap_or(g.at);
            let band = Self::guide_band(cx, g.axis, at);
            if !band.intersects(cx.area) {
                continue;
            }
            let selected = self.ui.edit.guide == Some(g.id) || moved.is_some();
            paint_guide_line(painter, t, cx, g.axis, at, selected);
            if bands.len() < 64 {
                bands.push((g.id, band.intersect(cx.area)));
            }
        }
        for (id, band) in bands {
            self.note(format!("canvas.guide.{id}"), band, true);
        }
        // A guide along no axis is drawn along its own direction: the whole drawing area, as the others, and no band to take hold of.
        let slanted: Vec<SlantedGuide> = self.slanted_now(cx.page).iter().take(MAX_SLANTED_DRAWN).copied().collect();
        for g in &slanted {
            if paint_slanted_guide(painter, t, cx, g) {
                self.note(format!("canvas.slanted_guide.{}", g.id), cx.area, false);
            }
        }
        if let Some((_, at)) = dragged
            && let (Some(g), Some(pointer)) = (lines.iter().find(|g| Some(g.id) == dragged.map(|d| d.0)), self.ui.edit.hover)
        {
            self.paint_guide_tag(painter, t, cx, g.axis, at, pointer);
        }
        if let Some(RulerDrag { axis, at: Some(at) }) = self.ui.edit.ruler {
            paint_guide_line(painter, t, cx, axis, at, true);
            if let Some(pointer) = self.ui.edit.ruler_pointer {
                self.paint_guide_tag(painter, t, cx, axis, at, pointer);
            }
        }
    }

    /// A small label by the pointer with where a guide being placed is, as the ruler reads it (the page's unit, at the drawing's scale).
    fn paint_guide_tag(&self, painter: &Painter, t: &Tokens, cx: &Cx, axis: Axis, at: f64, pointer: P) {
        let s = &cx.settings;
        let per_unit = s.ruler_unit.base_per_unit();
        let (scale, origin) = (s.drawing_scale, if axis == Axis::X { s.ruler_origin.0 } else { s.ruler_origin.1 });
        let value = (at / scale - origin) / per_unit;
        if !value.is_finite() {
            return;
        }
        let words = format!("{} {}", ruler_label(value), s.ruler_unit.code().to_lowercase());
        let galley = painter.layout_no_wrap(words, theme::font(11.0), t.text);
        let p = cx.screen_at(pointer);
        let size = galley.size() + vec2(10.0, 6.0);
        let at_screen = match axis {
            Axis::X => pos2(cx.screen_at((at, 0.0)).x + 8.0, p.y - size.y - 10.0),
            Axis::Y => pos2(p.x + 12.0, cx.screen_at((0.0, at)).y - size.y - 6.0),
        };
        let area = cx.area.shrink(2.0);
        let min = pos2(
            at_screen.x.clamp(area.left(), (area.right() - size.x).max(area.left())),
            at_screen.y.clamp(area.top(), (area.bottom() - size.y).max(area.top())),
        );
        let tag = Rect::from_min_size(min, size);
        painter.rect(tag, CornerRadius::same(3), t.panel_fill, Stroke::new(1.0, t.tile_border), StrokeKind::Inside);
        painter.galley(tag.min + vec2(5.0, 3.0), galley, t.text);
    }

    /// The rulers' part of the pointer: a drag out of the top ruler makes a horizontal guide and one out of the left ruler a vertical guide, one
    /// `guide.add` where it is let go over the drawing area. A drag let go over the ruler, or outside the drawing area, makes nothing. Each ruler is
    /// an element (`canvas.ruler.top`, `canvas.ruler.left`).
    pub(crate) fn ruler_input(&mut self, ui: &Ui, bounds: Rect, cx: &Cx) {
        let area = cx.area;
        if self.is_modal_open() {
            self.ui.edit.ruler = None;
            return;
        }
        let top = Rect::from_min_max(pos2(area.left(), bounds.top()), pos2(area.right(), area.top()));
        // The expanded Shapes panel registers its resize handle at this shared edge before the ruler.
        // Leave its grab band plus one point of clearance out of the ruler's interaction rectangle;
        // ruler painting still uses the full strip, and the folded Shapes strip has no resize handle.
        let left_start = if self.ui.shapes_collapsed {
            bounds.left()
        } else {
            (bounds.left() + ui.style().interaction.resize_grab_radius_side + 1.0).min(area.left())
        };
        let left = Rect::from_min_max(pos2(left_start, area.top()), pos2(area.left(), area.bottom()));
        for (strip, axis, name) in [(top, Axis::Y, "canvas.ruler.top"), (left, Axis::X, "canvas.ruler.left")] {
            if strip.width() <= 0.0 || strip.height() <= 0.0 {
                continue;
            }
            let response = ui.interact(strip, ui.scope_id().with(name), Sense::drag());
            self.note(name, strip, true);
            if response.hovered() || response.dragged() {
                ui.ctx().set_cursor_icon(if axis == Axis::Y { CursorIcon::ResizeVertical } else { CursorIcon::ResizeHorizontal });
            }
            if response.drag_started() && self.ui.edit.text.is_none() {
                self.ui.edit.gesture = None;
                self.ui.edit.ruler = Some(RulerDrag { axis, at: None });
                // A guide made with the guides hidden would not be seen: the drag shows them (ours), as the box would, so the drawing holds it.
                if !self.ui.show_guides {
                    self.turn_view_settings(diacraft_engine::doc::WindowFlags { guides: Some(true), ..Default::default() });
                }
            }
            let ours = self.ui.edit.ruler.is_some_and(|r| r.axis == axis);
            if !ours {
                continue;
            }
            let pointer = response.interact_pointer_pos().or_else(|| ui.input(|i| i.pointer.latest_pos()));
            let place = pointer.filter(|p| area.contains(*p)).map(|p| cx.page_at(p));
            // No repaint is asked for: the pointer's moves bring the frames, and a pointer held still needs none.
            self.ui.edit.ruler = Some(RulerDrag { axis, at: place.map(|p| sent(along(axis, p))) });
            self.ui.edit.ruler_pointer = place;
            if response.drag_stopped() {
                let made = self.ui.edit.ruler.take().and_then(|r| r.at);
                self.ui.edit.ruler_pointer = None;
                if let Some(at) = made {
                    self.add_guide(cx.page, axis, at);
                }
            }
        }
    }

    /// `guide.add` on `page`, and the new guide selected.
    fn add_guide(&mut self, page: u32, axis: Axis, at: f64) {
        let params = json!({"page": page, "orientation": orientation(axis), "at": inch(at)});
        if let Some(answer) = self.run_edit("guide.add", &params)
            && let Some(id) = answer.get("id").and_then(serde_json::Value::as_u64).and_then(|n| u32::try_from(n).ok())
        {
            self.select_guide(page, id);
        }
    }

    /// The drag of guide `id` ended at `to` (paper inches along its axis): `guide.move`, and the shapes held to it are kept painted where they go until
    /// the picture shows them there.
    pub(super) fn finish_guide_move(&mut self, page: u32, (id, from, to): (u32, f64, f64), held: &[u32]) {
        if (to - from).abs() < 1.0e-6 {
            return;
        }
        let params = json!({"id": id, "at": inch(to), "page": page});
        if self.run_edit("guide.move", &params).is_some() && !held.is_empty() {
            self.keep_preview(page, held);
        }
    }

    /// Delete on a selected guide: `guide.delete`. A guide that shapes are glued to is refused by the engine; the window says why
    /// in the status bar and the guide stays, selected.
    pub(crate) fn delete_guide(&mut self, page: u32, id: u32) {
        let glued = self.session.document().is_some_and(|doc| diacraft_engine::snap::hold::held_to(doc, page, id));
        match self.execute("guide.delete", &json!({"id": id, "page": page})) {
            Ok(_) => self.ui.edit.guide = None,
            // The engine's words name a command; the person is told what to do instead.
            Err(_) if glued => {
                self.set_notice("The guide was not deleted: shapes are glued to it. Move them off the guide first, then delete it.");
            }
            Err(e) => self.set_notice(&format!("The guide was not deleted: {e}")),
        }
    }

    /// The cursor over a guide: across its line.
    pub(super) fn guide_cursor(&self, cx: &Cx, p: P) -> Option<CursorIcon> {
        self.guide_hit(cx, p).map(|g| if g.axis == Axis::X { CursorIcon::ResizeHorizontal } else { CursorIcon::ResizeVertical })
    }
}

/// One guide's line across the drawing area, at `at` paper inches along `axis`; a selected one wider, in the selection's colour, over a band.
/// The most guides along no axis drawn in a frame (the snap reads at most 1,000 guides in all).
const MAX_SLANTED_DRAWN: usize = 256;

/// A guide along no axis, drawn across the drawing area through its place along its direction. → whether it was drawn (its numbers are finite).
fn paint_slanted_guide(painter: &Painter, t: &Tokens, cx: &Cx, g: &SlantedGuide) -> bool {
    let p = cx.screen_at(g.at);
    // The page's y runs up, the screen's down; far enough to leave the area on both sides whatever the angle.
    let dir = vec2(g.angle.cos() as f32, -(g.angle.sin() as f32));
    let far = cx.area.size().length() + (p - cx.area.center()).length();
    let ends = [p - dir * far, p + dir * far];
    if !(dir.x.is_finite() && dir.y.is_finite() && far.is_finite() && ends.iter().all(|e| e.x.is_finite() && e.y.is_finite())) {
        return false;
    }
    painter.line_segment(ends, Stroke::new(LINE, t.guide_line));
    true
}

fn paint_guide_line(painter: &Painter, t: &Tokens, cx: &Cx, axis: Axis, at: f64, selected: bool) {
    let ends: [Pos2; 2] = match axis {
        Axis::X => {
            let x = cx.screen_at((at, 0.0)).x;
            [pos2(x, cx.area.top()), pos2(x, cx.area.bottom())]
        }
        Axis::Y => {
            let y = cx.screen_at((0.0, at)).y;
            [pos2(cx.area.left(), y), pos2(cx.area.right(), y)]
        }
    };
    if !(ends[0].x.is_finite() && ends[0].y.is_finite() && ends[1].x.is_finite() && ends[1].y.is_finite()) {
        return;
    }
    if selected {
        painter.line_segment(ends, Stroke::new(HALO, t.guide_halo));
        painter.line_segment(ends, Stroke::new(SELECTED, t.guide_selected));
    } else {
        painter.line_segment(ends, Stroke::new(LINE, t.guide_line));
    }
}
