//! The tiles of the Shapes window: dragged onto the page they drop a shape, clicked they drop one at the middle of what is in view.
//!
//! A tile is enabled when it stands for a master that `stencil.list` gives and a drawing is open; the figures the stencil has no master for
//! stay drawn and quiet. The master is dropped by its id, whatever stencil it is in. The drop is `shape.drop`, one undo step; the new shape
//! is selected. **It is sent with `snap`**: the engine snaps the new shape's box by the document's Snap and Glue settings and holds it to
//! a guide it is pulled to, and the ghost that follows the pointer is shown where the engine's snap puts its box (`snapping.rs`), worked out once for
//! each place of the pointer ([`TileLanding`]), with the master's own connection points pulled with it as `shape.drop` pulls the new shape's. Alt held
//! drops it where it is.
//!
//! **The ghost is the shape the drop makes** (U49). A master is not dropped at the size it is listed at: beyond the range of eight of the page's drawing scale it keeps its size
//! on paper, and a group whose height follows its text has the height of its text. So the engine drops the master on a copy of the document
//! (`snap::drop_preview`) and the window keeps what that made ([`TileShape`]): the box's size on paper, which the ghost is drawn at and the snap's box has, and the places the
//! master brings, moved with the pointer. It is worked out **once for a drag**, and again only when the drawing changes (its revision, which a change of the page's scale is), never for a frame.

use std::sync::Arc;

use diacraft_engine::draw::units_per_inch;
use diacraft_engine::snap::{DropMaster, DropPreview, Extras, Rect as SnapRect, drop_preview, size_on_paper};
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

/// What dropping the master of the tile being dragged would make, on one page: the box's size on paper (the engine's drop on a copy of the document: `preview`, or, when the drop
/// cannot be made there, the size the range of eight gives the master) and the preview, which gives the places the master brings.
#[derive(Clone)]
pub struct TileShape {
    preview: Option<Arc<DropPreview>>,
    /// The size of the new shape's box, in paper inches.
    width: f64,
    height: f64,
}

/// A [`TileShape`] and what it was made for: the master, the page, and the drawing's generation and revision. A frame with all of them as they were makes no drop.
pub struct TileShapeKept {
    key: (String, u32, u64, u64),
    shape: TileShape,
}

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

    /// The screen rectangle the ghost of the tile being dragged was drawn at in the last frame, for the tests. `None` with no tile dragged.
    pub fn tile_ghost_rect(&self) -> Option<Rect> {
        self.ui.edit.tile_ghost
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
            let shape = self.tile_shape(master, over.map(|(_, at)| (sent(at.0), sent(at.1))));
            let landing = over.map(|(page, at)| self.drop_landing(&ctx, page, master, at, shape.as_ref()));
            self.ui.edit.tile_ghost = self.paint_tile_ghost(&ctx, master, landing.as_ref(), shape.as_ref());
        }
        if response.drag_stopped() && self.ui.edit.tile.take().is_some() {
            self.ui.edit.tile_ghost = None;
            self.ui.edit.tile_shape = None;
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

    /// What dropping `master` on the page on view would make, worked out **once for a drag**: the engine drops the master on a copy of the document (`snap::drop_preview`, which
    /// counts it: `snap::counts`), and the box and the places it makes are kept while the master, the page and the drawing (its generation and revision, which a change of the
    /// page's scale is) stay as they are. `centre` is where the drop is made on the copy, the pointer's place over the page and the middle of the page elsewhere: the box and the
    /// places are the same wherever the shape goes, moved by the distance (`DropPreview::rect_at`, `points_at`). `None` with no drawing, or no page.
    fn tile_shape(&mut self, master: &MasterInfo, centre: Option<P>) -> Option<TileShape> {
        let page = self.current_page()?;
        let key = (master.key(), page, self.ui.generation, self.session.revision());
        if let Some(kept) = self.ui.edit.tile_shape.as_ref().filter(|kept| kept.key == key) {
            return Some(kept.shape.clone());
        }
        let doc = self.session.document()?;
        let centre =
            centre.or_else(|| crate::canvas::page_drawing::page_settings(doc, page).map(|s| (sent(s.size.0 / 2.0), sent(s.size.1 / 2.0))))?;
        let preview = match &master.stencil {
            None => diacraft_engine::stencil::find_master(&master.id).and_then(|spec| drop_preview(doc, page, DropMaster::Stock(spec), centre)),
            Some(file) => {
                self.session.stencils().get(file).and_then(|stencil| drop_preview(doc, page, DropMaster::File { stencil, key: &master.id }, centre))
            }
        };
        // A drop that cannot be made on the copy (a page that is full, a master with no shape) is drawn at the size the range of eight gives the master.
        let (width, height) = match &preview {
            Some(p) => (p.width(), p.height()),
            None => size_on_paper(master.width.max(0.0), master.height.max(0.0), units_per_inch(doc, page)),
        };
        let shape = TileShape { preview: preview.map(Arc::new), width, height };
        self.ui.edit.tile_shape = Some(TileShapeKept { key, shape: shape.clone() });
        Some(shape)
    }

    /// Where a master dropped centred at `at` lands: its box (`shape`'s size, which is the size of the shape the drop makes) snapped by the engine's snap on the kept page, as `shape.drop`
    /// snaps it, with the master's own connection points pulled with it (the places the new shape will have, which the command reads off the shape it makes: `shape`'s preview, moved
    /// to `at`). Alt held leaves it where it is. Worked out again only when the pointer, Alt, the zoom, the view or the drawing changed.
    fn drop_landing(&mut self, ctx: &egui::Context, page: u32, master: &MasterInfo, at: P, shape: Option<&TileShape>) -> DropLanding {
        let at = (sent(at.0), sent(at.1));
        let alt = ctx.input(|i| i.modifiers.alt);
        let Some(cx) = self.cx_of(page) else { return (page, at, Vec::new()) };
        let view = view_of(&cx);
        let key = (master.key(), page, at, alt, cx.view.scale(), self.ui.generation, self.session.revision(), view, self.ui.edit.reads_done);
        if let Some(kept) = self.ui.edit.tile_landing.as_ref().filter(|t| t.key == key) {
            return kept.landing.clone();
        }
        let (landing, snapped) = match (alt, shape) {
            (false, Some(shape)) => {
                // The box of the shape the drop makes, on paper, where it will be, and the places it will have there: what the drop on a copy of the document made of the master (beyond
                // the range of eight of the page's scale its size on paper, and for a group of compartments that follow their text the height their text makes it).
                let (rect, own) = match &shape.preview {
                    Some(preview) => (preview.rect_at(at), preview.points_at(at)),
                    None => (
                        SnapRect {
                            x0: at.0 - shape.width / 2.0,
                            y0: at.1 - shape.height / 2.0,
                            x1: at.0 + shape.width / 2.0,
                            y1: at.1 + shape.height / 2.0,
                        },
                        Vec::new(),
                    ),
                };
                match self.snap_box(page, rect, &[], &own, cx.view.scale(), Extras { view: Some(view), ..Extras::default() }) {
                    Some(snapped) => ((page, (at.0 + snapped.dx, at.1 + snapped.dy), snapped.marks), true),
                    None => ((page, at, Vec::new()), false),
                }
            }
            // Alt held leaves it where it is, and a drag with nothing to drop on a copy of (no drawing) has nothing to snap.
            _ => ((page, at, Vec::new()), false),
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

    /// The outline of the shape that would be dropped, at the page's zoom and the size the drop makes it: over the page where the snap puts it, with what it snapped to; elsewhere
    /// following the pointer.
    fn paint_tile_ghost(&self, ctx: &egui::Context, master: &MasterInfo, landing: Option<&DropLanding>, shape: Option<&TileShape>) -> Option<Rect> {
        let (Some(pointer), Some(scale)) = (ctx.input(|i| i.pointer.latest_pos()), self.current_scale()) else { return None };
        let t = Tokens::get(ctx);
        // The size of the shape the drop makes, in paper inches (`tile_shape`): the master's own only when there is no drawing to drop it on.
        let (width, height) = shape.map_or((master.width, master.height), |s| (s.width, s.height));
        let size = vec2(width as f32 * scale as f32, height as f32 * scale as f32);
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
        Some(rect)
    }

    /// Points to an inch on the page on view.
    fn current_scale(&self) -> Option<f64> {
        let page = self.ui.page?;
        self.ui.views.get(&page).map(crate::canvas::view::View::scale)
    }
}
