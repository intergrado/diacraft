//! The tiles of the Shapes window: dragged onto the page they drop a shape, clicked they drop one at the middle of what is in view.
//!
//! A tile is enabled when it stands for a master that `stencil.list` gives and a drawing is open; the figures the stencil has no master for
//! stay drawn and quiet. The master is dropped by its id, whatever stencil it is in. The drop is `shape.drop`, one undo step; the new shape
//! is selected. **It is sent with `snap`**: the engine snaps the new shape's box by the document's Snap and Glue settings and holds it to
//! a guide it is pulled to, and the ghost that follows the pointer is shown where the engine's snap puts its box (`snapping.rs`), worked out once for
//! each place of the pointer ([`TileLanding`]), with the master's own connection points pulled with it as `shape.drop` pulls the new shape's. Alt held
//! drops it where it is.

use diacraft_engine::snap::{Extras, Rect as SnapRect, dropped_points, dropped_points_of_file};
use egui::{CursorIcon, Id, LayerId, Order, Rect, Response, Stroke, Ui, vec2};
use serde_json::json;

use super::geom::{self, P};
use super::inch;
use super::snapping::{Mark, sent, view_of};
use crate::app::DiacraftApp;
use crate::stencils::MasterInfo;
use crate::theme::Tokens;

/// How many steps down and to the right a click on a tile looks for a free place before it settles for the middle.
const FREE_PLACE_TRIES: u32 = 24;
/// How near, in inches, another shape's middle may be for a place to count as taken.
const TAKEN: f64 = 0.2;

/// A tile being dragged from the Shapes window.
#[derive(Clone)]
pub struct TileDrag {
    pub master: MasterInfo,
}

/// Where a master dropped now would land: the page, its centre there, and what the snap pulled it to.
type DropLanding = (u32, P, Vec<Mark>);

/// Where the tile being dragged would land, and what that was worked out for: the master, the page and the place under the pointer, Alt, the zoom, the
/// drawing's generation and revision, the view, and how many answers the page's reader has given. A frame with all of them as they were asks the snap nothing.
pub struct TileLanding {
    key: (String, u32, P, bool, f64, u64, u64, [f64; 4], u64),
    landing: DropLanding,
    /// Whether the landing is the engine's snap, and not the drop as the pointer made it (Alt held, or the page not ready yet): **the release is sent with `snap` only when
    /// the ghost it shows was snapped**, since a release that snapped what the ghost did not would land somewhere the ghost never was.
    snapped: bool,
}

impl DiacraftApp {
    /// Where the tile being dragged would land, if it is over the page: the place of the new shape's middle in paper inches, as the ghost is drawn (and the
    /// release drops it), for the tests. `None` with no tile dragged, or one off the page.
    pub fn tile_preview(&self) -> Option<P> {
        self.ui.edit.tile_landing.as_ref().map(|t| t.landing.1)
    }

    /// The pointer's work on one tile: the drag begins, a ghost of the shape follows the pointer, and the release over the page drops it.
    pub(crate) fn tile_input(&mut self, ui: &Ui, response: &Response, master: &MasterInfo) {
        let ctx = ui.ctx().clone();
        if response.drag_started() {
            self.ui.edit.tile = Some(TileDrag { master: master.clone() });
        }
        if response.dragged() && self.ui.edit.tile.is_some() {
            let over = self.canvas_page_point(&ctx);
            ctx.set_cursor_icon(if over.is_some() { CursorIcon::Copy } else { CursorIcon::NoDrop });
            let landing = over.map(|(page, at)| self.drop_landing(&ctx, page, master, at));
            self.paint_tile_ghost(&ctx, master, landing.as_ref());
        }
        if response.drag_stopped() && self.ui.edit.tile.take().is_some() {
            let shown = self.ui.edit.tile_landing.take();
            // A master let go inside the Shapes window changes its group (across the divider), and is not dropped on the page.
            if self.drop_in_gallery(&ctx, master) {
                return;
            }
            // A master let go on a shape's AutoConnect arrow is placed beside the shape and joined to it.
            if self.auto_drop_master(&ctx, master) {
                return;
            }
            if let Some((page, at)) = self.canvas_page_point(&ctx) {
                let alt = ctx.input(|i| i.modifiers.alt);
                match shown.filter(|t| !alt && t.key.1 == page) {
                    // **The drop is the ghost**: the place the ghost was worked out for, with `snap` only if the ghost was snapped. A ghost that went as the pointer
                    // made it (the reader had not answered yet) is dropped where it was, with no `snap`, whatever the page has become ready for since.
                    Some(t) => self.drop_master(page, master, t.key.2, t.snapped),
                    // No ghost was shown (the button came up before one was), or Alt is held: where the pointer is, snapped unless Alt says not.
                    None => self.drop_master(page, master, (sent(at.0), sent(at.1)), !alt),
                }
            }
        } else if response.clicked() {
            self.drop_at_middle(master);
        }
    }

    /// The page on view and the point of it under the pointer, if the pointer is over the drawing area.
    fn canvas_page_point(&mut self, ctx: &egui::Context) -> Option<(u32, P)> {
        let page = self.current_page()?;
        let cx = self.cx_of(page)?;
        let pointer = ctx.input(|i| i.pointer.latest_pos())?;
        cx.area.contains(pointer).then(|| (page, cx.page_at(pointer)))
    }

    /// Where a master dropped centred at `at` lands: its box snapped by the engine's snap on the kept page, as `shape.drop` snaps it, with the master's own connection
    /// points pulled with it (`dropped_points`, and `dropped_points_of_file` for a master of a stencil file: the places the new shape will have, which the command reads off the
    /// shape it makes). Alt held leaves it where it is. Worked out again only when the pointer, Alt, the zoom, the view or the drawing changed.
    fn drop_landing(&mut self, ctx: &egui::Context, page: u32, master: &MasterInfo, at: P) -> DropLanding {
        let at = (sent(at.0), sent(at.1));
        let alt = ctx.input(|i| i.modifiers.alt);
        let Some(cx) = self.cx_of(page) else { return (page, at, Vec::new()) };
        let view = view_of(&cx);
        let key = (master.key(), page, at, alt, cx.view.scale(), self.ui.generation, self.session.revision(), view, self.ui.edit.reads_done);
        if let Some(kept) = self.ui.edit.tile_landing.as_ref().filter(|t| t.key == key) {
            return kept.landing.clone();
        }
        let (landing, snapped) = if alt {
            ((page, at, Vec::new()), false)
        } else {
            // The master's size is drawing units, which the command divides by the page's scale to the paper inches the box is in.
            let k = self.ui.edit.snap_page.as_ref().filter(|c| c.key().2 == page).map_or(1.0, |c| c.page.k);
            let (w, h) = (master.width.max(0.0) / k, master.height.max(0.0) / k);
            let rect = SnapRect { x0: at.0 - w / 2.0, y0: at.1 - h / 2.0, x1: at.0 + w / 2.0, y1: at.1 + h / 2.0 };
            // The places the new shape will have, where it will be: a stock master's connection points, and a stencil file's master's, which the engine reads off the shape a
            // drop on a copy of the document makes (`dropped_points_of_file`), as `shape.drop` reads them off the shape it makes.
            let own: Vec<P> = match &master.stencil {
                None => diacraft_engine::stencil::find_master(&master.id).map(|spec| dropped_points(spec, at, k)).unwrap_or_default(),
                Some(file) => self
                    .session
                    .document()
                    .zip(self.session.stencils().get(file))
                    .map(|(doc, stencil)| dropped_points_of_file(doc, page, stencil, &master.id, at))
                    .unwrap_or_default(),
            };
            match self.snap_box(page, rect, &[], &own, cx.view.scale(), Extras { view: Some(view), ..Extras::default() }) {
                Some(snapped) => ((page, (at.0 + snapped.dx, at.1 + snapped.dy), snapped.marks), true),
                None => ((page, at, Vec::new()), false),
            }
        };
        self.ui.edit.tile_landing = Some(TileLanding { key, landing: landing.clone(), snapped });
        landing
    }

    /// Drops the master centred at `centre` on `page` and selects it; with `snap`, the engine snaps it by the document's settings, at the view's zoom.
    fn drop_master(&mut self, page: u32, master: &MasterInfo, centre: P, snap: bool) {
        let mut params = json!({"master": master.id, "page": page, "x": inch(centre.0), "y": inch(centre.1)});
        // A master of a stencil file is dropped from that stencil.
        if let (Some(stencil), Some(map)) = (&master.stencil, params.as_object_mut()) {
            map.insert("stencil".to_owned(), json!(stencil));
        }
        // `snap` carries the zoom and the visible part of the page (`view`: a drop has no use for it, and the window sends it with every snapping command).
        let view = self.ui.page.and_then(|p| self.cx_of(p)).map(|cx| view_of(&cx));
        if let (true, Some(zoom), Some(map)) = (snap, self.current_scale(), params.as_object_mut()) {
            map.insert("snap".to_owned(), view.map_or_else(|| json!({"zoom": zoom}), |view| json!({"zoom": zoom, "view": view})));
        }
        let Some(answer) = self.run_edit("shape.drop", &params) else { return };
        self.ui.edit.last_snap = answer.get("snap").cloned();
        if let Some(id) = answer.get("id").and_then(serde_json::Value::as_u64).and_then(|id| u32::try_from(id).ok()) {
            self.keep_preview(page, &[id]);
            self.select(&[id]);
            // A master that holds a field with Ask on drop has the values asked for (`shape_data.rs`).
            self.ask_for_data(page, id);
        }
    }

    /// A click on a tile: **Assumed: it drops the shape at the middle of the part of the page in view** (the pointer is not on the page), a
    /// step of the grid down and to the right of the last one while that place is taken. The place is snapped by the engine, as a drop by the pointer is.
    fn drop_at_middle(&mut self, master: &MasterInfo) {
        let Some(page) = self.current_page() else { return };
        let Some(cx) = self.cx_of(page) else { return };
        self.ensure_scene(page);
        let (w, h) = cx.settings.size;
        let middle = cx.page_at(cx.area.center());
        let base = (sent(middle.0.clamp(0.0, w)), sent(middle.1.clamp(0.0, h)));
        let (gx, gy) = self.grid_pitch(&cx);
        let taken = |p: P| self.ui.edit.scene.as_ref().is_some_and(|s| s.items.iter().any(|i| geom::distance(i.centre(), p) < TAKEN));
        let centre = (0..FREE_PLACE_TRIES)
            .map(|k| (base.0 + f64::from(k) * gx, base.1 - f64::from(k) * gy))
            .find(|p| !taken(*p) && p.0 <= w && p.1 >= 0.0)
            .unwrap_or(base);
        self.drop_master(page, master, centre, true);
    }

    /// The outline of the shape that would be dropped, at the page's zoom: over the page where the snap puts it, with what it snapped to; elsewhere
    /// following the pointer.
    fn paint_tile_ghost(&self, ctx: &egui::Context, master: &MasterInfo, landing: Option<&DropLanding>) {
        let (Some(pointer), Some(scale)) = (ctx.input(|i| i.pointer.latest_pos()), self.current_scale()) else { return };
        let t = Tokens::get(ctx);
        let size = vec2(master.width as f32 * scale as f32, master.height as f32 * scale as f32);
        let over = landing.and_then(|(page, at, marks)| self.cx_of(*page).map(|cx| (cx, *at, marks)));
        let centre = over.as_ref().map_or(pointer, |(cx, at, _)| cx.screen_at(*at));
        let rect = Rect::from_center_size(centre, size);
        let painter = ctx.layer_painter(LayerId::new(Order::Tooltip, Id::unique("tile_ghost")));
        let colour = if over.is_some() { t.edit_line } else { t.text_disabled };
        if master.directed_preview() {
            painter.arrow(rect.left_center(), rect.right_center() - rect.left_center(), Stroke::new(1.5, colour));
        } else {
            painter.rect_filled(rect, 2.0, t.edit_fill);
            painter.add(egui::Shape::closed_line(
                vec![rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom()],
                Stroke::new(1.5, colour),
            ));
        }
        if let Some((cx, _, marks)) = &over {
            super::paint::paint_marks(&painter.with_clip_rect(cx.area), &t, cx, marks);
        }
    }

    /// Points to an inch on the page on view.
    fn current_scale(&self) -> Option<f64> {
        let page = self.ui.page?;
        self.ui.views.get(&page).map(crate::canvas::view::View::scale)
    }
}
