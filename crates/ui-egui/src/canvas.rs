//! The drawing area: the workspace, the page on it, and the rulers and grid over it.
//!
//! Panning and zooming work on the page as it is: the wheel scrolls, Ctrl with the wheel (and a pinch) zooms about the
//! pointer, and a drag with the middle button moves the page. The primary button draws (`edit`). What the page itself draws
//! is asked of one function, [`page_drawing::paint_page_drawing`]; the grid, the rulers and the guides are overlays drawn here, from the
//! View tab's check boxes (the guides are `edit/guides.rs`). The grid's lines are those the engine's
//! snap pulls a drag to (`edit/snapping.rs`), in the drawing's units and at its scale. A drag out of a ruler makes a guide.

use egui::{Align2, CursorIcon, Frame, PointerButton, Rect, Sense, Ui, vec2};

use crate::app::DiacraftApp;
use crate::state::CanvasReport;
use crate::theme::{self, Tokens, size};

pub mod grid;
pub mod page_drawing;
pub mod picture;
pub mod view;

use page_drawing::{PageFrame, PageSettings, page_settings, paint_page_lost, paint_page_paper, paint_page_picture};
use view::View;

/// The least the drawing area is made, in points, so that a hostile window size cannot give a zero or negative area.
const MIN_AREA: f32 = 16.0;

/// Where the drawing is: the whole area with its rulers, the part inside them, and the page shown.
#[derive(Clone, Copy)]
struct Where {
    bounds: Rect,
    area: Rect,
    page: u32,
}

impl DiacraftApp {
    pub(crate) fn canvas(&mut self, ui: &mut Ui, t: Tokens) {
        let frame = Frame::new().fill(t.canvas_fill);
        egui::CentralPanel::default().frame(frame).show(ui, |ui| {
            self.run_probe(crate::Stage::Panel("canvas"));
            let bounds = ui.max_rect();
            let Some(page) = self.current_page() else {
                let (rect, resp) = ui.allocate_exact_size(vec2(bounds.width(), bounds.height()), Sense::hover());
                let message = "No drawing open. Choose File, then New.";
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, message));
                ui.painter().text(rect.center(), Align2::CENTER_CENTER, message, theme::font(size::TAB), t.text_dim);
                return;
            };
            let settings = self.session().document().and_then(|d| page_settings(d, page)).unwrap_or_default();
            let rulers = if self.ui.show_rulers { grid::RULER_THICKNESS } else { 0.0 };
            let area = Rect::from_min_max(bounds.min + vec2(rulers, rulers), bounds.max);
            let area =
                if area.width() < MIN_AREA || area.height() < MIN_AREA { Rect::from_min_size(area.min, vec2(MIN_AREA, MIN_AREA)) } else { area };
            self.ui.canvas_rect = Some(area);
            let mut view = self.ui.views.get(&page).copied().unwrap_or_else(|| View::fit_page(area, settings.size));
            // A view made by Fit follows the window as it is resized.
            if view.fitted {
                view = View::fit_page(area, settings.size);
            }
            let response = ui.interact(area, ui.scope_id().with("canvas"), Sense::click_and_drag());
            self.note("canvas", area, true);
            self.canvas_input(ui, &response, area, &mut view);
            self.ui.views.insert(page, view);
            let cx = crate::edit::Cx { area, view, page, settings };
            // A guide is dragged out of a ruler (`edit/guides.rs`).
            if self.ui.show_rulers {
                self.ruler_input(ui, bounds, &cx);
            } else {
                self.ui.edit.forget_ruler_drag();
            }
            self.edit_canvas(ui, &response, &cx);
            // The menu a right click opens: Cut, Copy, Paste (at the click), Duplicate, Group, Ungroup (`ribbon/spec.rs`, `CANVAS_MENU`).
            egui::Popup::context_menu(&response).show(|ui| self.canvas_menu_rows(ui));
            self.paint_canvas(ui, &t, Where { bounds, area, page }, &view, &settings);
        });
    }

    fn canvas_input(&mut self, ui: &Ui, response: &egui::Response, area: Rect, view: &mut View) {
        // The primary button draws (`edit`): it selects, moves and joins. The middle button moves the page, and so do the wheel and `view.pan`.
        if response.dragged_by(PointerButton::Middle) {
            view.pan_by(response.drag_delta());
            ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        }
        if !response.hovered() {
            return;
        }
        let (scroll, zoom_delta, pointer) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.pointer.hover_pos()));
        if (zoom_delta - 1.0).abs() > f32::EPSILON {
            view.zoom_about(area, pointer.unwrap_or(area.center()), view.zoom * f64::from(zoom_delta));
        }
        if scroll != egui::Vec2::ZERO {
            view.pan_by(scroll);
        }
    }

    fn paint_canvas(&mut self, ui: &Ui, t: &Tokens, at: Where, view: &View, settings: &PageSettings) {
        let Where { bounds, area, page } = at;
        let painter = ui.painter_at(bounds);
        let in_area = painter.with_clip_rect(area);
        let frame = PageFrame { id: page, rect: view.page_rect(area, settings.size), scale: view.scale() };
        // The paper, then the grid over it, then the drawing over the grid (a project rule: `page_drawing.rs`).
        paint_page_paper(&in_area, t, &frame);
        let mut report = CanvasReport { page: Some(frame.rect), ..CanvasReport::default() };
        if self.ui.show_grid {
            // The snap's lines, at the drawing's scale (`edit/snapping.rs`), so that what is drawn is what a drag snaps to.
            let [x, y] = self.grid_marks(page, view.scale());
            let k = self.session().document().map_or(1.0, |doc| diacraft_engine::draw::units_per_inch(doc, page));
            let axis = |m: Option<diacraft_engine::snap::Marks>| m.map(|m| grid::GridAxis { pitch: m.pitch, origin: m.origin });
            let lines = grid::GridLines { x: axis(x), y: axis(y), k };
            report.grid_lines = grid::paint_grid(&painter, t, view, area, &frame, settings, &lines);
        }
        // The back end's own limit on a texture's side: a picture past it panics in egui, and on the web there is nothing to catch that.
        let max_side = ui.ctx().input(|i| i.max_texture_side) as f32;
        let want = picture::Want { page, page_rect: frame.rect, scale: frame.scale, area, ppp: ui.ctx().pixels_per_point(), max_side };
        if let Some(failure) = self.update_picture(ui.ctx(), &want) {
            self.set_notice(&format!(
                "internal error while drawing the page: {failure} (the window carries on with the picture it had; please report this bug)"
            ));
        }
        // A theme or variant under the pointer is shown in place of the page's picture (`theme_preview.rs`); the page's own picture is not touched.
        let previewing = self.update_theme_preview(ui.ctx(), &want);
        let image = if previewing { self.ui.theme_preview.pictures.image(page) } else { self.ui.pictures.image(page) };
        paint_page_picture(&in_area, t, image, &frame);
        report.picture = image.map(|i| crate::state::PictureReport { size: i.size, lit: i.lit, rough: i.rough });
        report.preview = previewing && image.is_some();
        // The page breaks over the picture (View, Show, Page Breaks), dashed, where the page passes the printer paper (`relayout_ui.rs`).
        if let Some(lines) = self.page_break_lines(page) {
            report.page_breaks = crate::relayout_ui::paint_page_breaks(&in_area, t, view, area, &frame, settings.size, &lines);
        }
        let cx = crate::edit::Cx { area, view: *view, page, settings: *settings };
        // The guides over the picture (they are never in it: the renderer leaves them out), and the editing layer over them.
        self.paint_guides(&in_area, t, &cx);
        self.paint_edit(&in_area, t, &cx);
        // A page that lost everything says so on the page, in words: a small mark in the status bar is not enough.
        let lost = self.ui.pictures.lost(page).map(str::to_owned);
        if let Some(words) = &lost {
            let rect = paint_page_lost(&in_area, t, &frame, words);
            self.note("canvas.lost", rect, true);
        }
        if self.ui.show_rulers {
            grid::paint_rulers(&painter, t, view, bounds, area, &frame, settings);
            report.rulers = true;
        }
        self.ui.report = report;
    }

    /// Takes the pictures the render worker has finished, and asks it for one if the view needs one. → the message of a drawing that
    /// failed, if one did.
    fn update_picture(&mut self, ctx: &egui::Context, want: &picture::Want) -> Option<String> {
        let failure = self.ui.pictures.update(ctx, &self.session, want, self.clock.now());
        // A panic the renderer or its worker caught is a panic of this window: `ui.inspect` counts it, so a page that drew blank is never
        // reported as one with no panics.
        self.ui_panics = self.ui_panics.saturating_add(self.ui.pictures.take_panics());
        failure
    }

    /// The view of the page on view as the frame after this one draws it, and the drawing area it is drawn in (the last frame's: only a frame knows a new area).
    /// **The answer of a window command and `ui.inspect` read this, and not what the last frame painted**: a request is answered before the frame
    /// that draws what it changed, so the rectangle the last frame drew (`canvas_report`) is the page as it was before a `view.pan` or `view.zoom` that came in
    /// the same pass. A view made by Fit follows the area, as `canvas` makes it each frame; a page with no view yet is fitted, as the frame makes it.
    pub fn shown_view(&self) -> Option<(View, Rect)> {
        let page = self.ui.page?;
        let area = self.ui.canvas_rect?;
        let size = self.session().document().and_then(|d| page_settings(d, page)).map_or(page_drawing::DEFAULT_PAGE_INCHES, |s| s.size);
        let view = match self.ui.views.get(&page) {
            Some(view) if !view.fitted => *view,
            _ => View::fit_page(area, size),
        };
        Some((view, area))
    }

    /// Where the page on view is on the screen as the frame after this one draws it, in points: `[left, top, width, height]` (see [`DiacraftApp::shown_view`]).
    pub fn shown_page_rect(&self) -> Option<[f32; 4]> {
        let (view, area) = self.shown_view()?;
        let page = self.ui.page?;
        let size = self.session().document().and_then(|d| page_settings(d, page)).map_or(page_drawing::DEFAULT_PAGE_INCHES, |s| s.size);
        let r = view.page_rect(area, size);
        Some([r.min.x, r.min.y, r.width(), r.height()])
    }

    /// The point of the current page, in inches, under the screen point `at`, as the last frame drew the view.
    pub fn page_point_at(&self, at: egui::Pos2) -> Option<(f64, f64)> {
        let area = self.ui.canvas_rect?;
        let view = self.ui.views.get(&self.ui.page?)?;
        Some(view.to_page(area, at))
    }

    /// The screen point of the point `p` of the current page, in inches, as the last frame drew the view: the other way from
    /// [`DiacraftApp::page_point_at`].
    pub fn screen_point_at(&self, p: (f64, f64)) -> Option<egui::Pos2> {
        let area = self.ui.canvas_rect?;
        let view = self.ui.views.get(&self.ui.page?)?;
        Some(view.to_screen(area, p))
    }

    /// The picture of the page on view, for `ui.inspect`: how many pictures were asked for and came, and the one on the screen.
    pub fn picture_state(&self) -> serde_json::Value {
        let p = &self.ui.pictures;
        serde_json::json!({
            "requested": p.requested,
            "received": p.received,
            "busy": p.busy(),
            "tookMs": u64::try_from(p.last_took.as_millis()).unwrap_or(u64::MAX),
            "shown": self.ui.report.picture.map(|r| serde_json::json!({"width": r.size[0], "height": r.size[1], "litPixels": r.lit, "rough": r.rough})),
            // The words the page shows when its drawing failed, or null; and what the window has caught since it began (these also count in
            // `panics.ui`).
            "lost": self.ui.page.and_then(|page| p.lost(page)),
            "caughtPanics": {"framesLost": p.frames_lost, "shapesLost": p.shapes_lost},
        })
    }

    /// What the drawing area drew in the last frame.
    pub fn canvas_report(&self) -> CanvasReport {
        self.ui.report
    }
}
