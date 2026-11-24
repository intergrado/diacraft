//! The status bar: the page number and count, the Snap mark, the language, three buttons, a zoom-out button, the zoom slider, a zoom-in button,
//! the zoom percentage and the fit-to-window button.
//!
//! Width, Height and Angle of the one selected shape follow the page count, and show the size a handle's drag is making. The
//! language and the other buttons do nothing yet, so they are drawn quiet. The slider, the two zoom buttons and the fit button work (`view.zoom`, `view.zoomIn`,
//! `view.zoomOut`, `view.fitPage`), and the percentage opens the Zoom dialog (`window.zoomDialog`: Office-like apps do, and no page says so for the
//! reference application).
//!
//! **The Snap mark is DiaCraft's own** (the reference application's status bar has none, the list): the magnet and "Snap on" or "Snap
//! off", read from the document's own Snap switch each frame, and a click is `snap.set` with the switch the other way, as the View tab's Snap box sends it.

use egui::{Align2, CornerRadius, Frame, Margin, Rect, Sense, Stroke, Ui, pos2, vec2};
use serde_json::json;

use crate::app::DiacraftApp;
use crate::canvas::view::{MAX_ZOOM, MIN_ZOOM};
use crate::findings::{self, Finding};
use crate::icons::{self, Base, Icon};
use crate::theme::{self, Tokens, size};
use crate::widgets::{self, paint_button_bg, text_width, tip};

/// The status bar's row height.
pub const ROW_H: f32 = 26.0;

/// The language the status bar names: the one the document's text is in until the document says otherwise.
pub const LANGUAGE: &str = "English (United States)";

const SLIDER_W: f32 = 120.0;

/// The status bar's width, in points, from which it shows the three buttons, and from which it shows the slider.
const WIDE_BAR: f32 = 760.0;
const MEDIUM_BAR: f32 = 540.0;
const BUTTON: f32 = 22.0;

impl DiacraftApp {
    pub(crate) fn status_bar(&mut self, ui: &mut Ui, t: Tokens) {
        let frame = Frame::new().fill(t.status_bar_fill).inner_margin(Margin::symmetric(8, 2));
        let panel = egui::Panel::bottom("status_bar").show_separator_line(false).frame(frame).show(ui, |ui| {
            self.run_probe(crate::Stage::Panel("status_bar"));
            let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::hover());
            let right_start = self.status_right(ui, &t, bar);
            self.status_left(ui, &t, bar, right_start);
        });
        let rect = panel.response.rect;
        ui.painter().hline(rect.x_range(), rect.top(), Stroke::new(1.0, t.separator));
    }

    /// The page number and count, a notice if there is one, and the language: in the room left of `limit`.
    fn status_left(&mut self, ui: &mut Ui, t: &Tokens, bar: Rect, limit: f32) {
        let font = theme::font(size::BAR);
        let area = Rect::from_min_max(bar.min, pos2(limit - 8.0, bar.max.y));
        let painter = ui.painter().with_clip_rect(area);
        let mut x = bar.left() + 2.0;
        if let Some((n, of)) = self.page_position() {
            let text = format!("Page {n} of {of}");
            let w = text_width(ui, &text, &font);
            painter.text(pos2(x, bar.center().y), Align2::LEFT_CENTER, &text, font.clone(), t.status_bar_text);
            let resp = ui.interact(Rect::from_min_size(pos2(x, bar.top()), vec2(w, bar.height())), ui.scope_id().with("page_count"), Sense::hover());
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &text));
            self.note("status.page", resp.rect, true);
            x += w + 18.0;
        }
        x = self.snap_mark(ui, t, x, bar, area.right());
        x = self.findings_mark(ui, t, x, bar, area.right());
        if let Some(text) = self.selection_summary() {
            let w = text_width(ui, &text, &font);
            if x + w + 8.0 < area.right() {
                painter.text(pos2(x, bar.center().y), Align2::LEFT_CENTER, &text, font.clone(), t.status_bar_text);
                let r = Rect::from_min_size(pos2(x, bar.top()), vec2(w, bar.height()));
                let resp = ui.interact(r, ui.scope_id().with("selection_size"), Sense::hover());
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &text));
                self.note("status.selection", r, true);
                x += w + 18.0;
            }
        }
        if let Some(notice) = self.notice().map(str::to_owned) {
            let w = text_width(ui, &notice, &font).min((area.right() - x).max(0.0));
            painter.text(pos2(x, bar.center().y), Align2::LEFT_CENTER, &notice, font.clone(), t.notice_text);
            let r = Rect::from_min_size(pos2(x, bar.top()), vec2(w, bar.height()));
            let resp = ui.interact(r, ui.scope_id().with("notice"), Sense::hover());
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &notice));
            // Its time runs from here, and it then gives the language its room back (`notice.rs`).
            self.notice_shown();
        } else {
            let w = text_width(ui, LANGUAGE, &font);
            if x + w + BUTTON + 12.0 < area.right() {
                painter.text(pos2(x, bar.center().y), Align2::LEFT_CENTER, LANGUAGE, font.clone(), t.text_disabled);
                let r = Rect::from_min_size(pos2(x, bar.top()), vec2(w, bar.height()));
                let resp = ui.interact(r, ui.scope_id().with("language"), Sense::hover());
                widgets::name_button(&resp, false, LANGUAGE);
                self.note("status.language", r, false);
                resp.on_hover_text(tip("Language", false));
                x += w + 12.0;
                let r = Rect::from_center_size(pos2(x + BUTTON / 2.0, bar.center().y), vec2(BUTTON, BUTTON));
                self.quiet_button(ui, t, r, Icon::of(Base::Play), "Macro Recording", "status.macro");
            }
        }
    }

    /// The Snap mark: a magnet with "Snap on" or "Snap off" (struck through when off), pressed while the document's Snap switch is on; a click turns the switch the
    /// other way (`snap.set`, no undo step). It is as wide for either word, so that the parts after it do not move as it is pressed. Nothing is drawn with no
    /// document, or where the room left of `limit` is too small for it. → the room used, as an x past the mark.
    fn snap_mark(&mut self, ui: &mut Ui, t: &Tokens, x: f32, bar: Rect, limit: f32) -> f32 {
        let Some(on) = self.snap_settings_now().map(|s| s.snap) else { return x };
        let enabled = self.command_enabled("snap.set").is_ok();
        let font = theme::font(size::BAR);
        let words = if on { "Snap on" } else { "Snap off" };
        let text_w = text_width(ui, "Snap on", &font).max(text_width(ui, "Snap off", &font));
        let r = Rect::from_min_size(pos2(x, bar.top() + 2.0), vec2(6.0 + 14.0 + 4.0 + text_w + 6.0, bar.height() - 4.0));
        if r.right() > limit {
            return x;
        }
        let resp = ui.interact(r, ui.scope_id().with("snap_mark"), if enabled { Sense::click() } else { Sense::hover() });
        widgets::name_toggle(&resp, enabled, on, words);
        paint_button_bg(ui, r, &resp, enabled, on, t, 4);
        let icon = Rect::from_center_size(pos2(r.left() + 6.0 + 7.0, r.center().y), vec2(14.0, 14.0));
        icons::paint(ui.painter(), icon, Icon::of(Base::Magnet), t, enabled);
        if !on {
            // Struck through, so that "off" does not rest on a colour alone.
            ui.painter().line_segment([icon.left_bottom() + vec2(1.0, -1.0), icon.right_top() + vec2(-1.0, 1.0)], Stroke::new(1.4, t.text_dim));
        }
        let ink = if !enabled {
            t.text_disabled
        } else if on {
            t.status_bar_text
        } else {
            t.text_dim
        };
        ui.painter().text(pos2(r.left() + 6.0 + 14.0 + 4.0, r.center().y), Align2::LEFT_CENTER, words, font, ink);
        self.note("status.snap", r, enabled);
        let tooltip = if on { "Snap is on: shapes snap to what Snap & Glue names. Click to turn it off" } else { "Snap is off. Click to turn it on" };
        if resp.on_hover_text(tooltip).clicked()
            && enabled
            && let Some(params) = self.snap_box_toggle(r#"{"snap":true}"#)
            && let Err(e) = self.execute("snap.set", &params)
        {
            self.set_notice(&e.to_string());
        }
        r.right() + 10.0
    }

    /// The small mark that says some of the drawing is not shown as the file has it, and the list it opens: what the renderer left out
    /// or drew approximately on this page, and what the reader of the file said. Nothing is drawn when there is nothing to say.
    /// → the room used, as an x past the mark.
    fn findings_mark(&mut self, ui: &mut Ui, t: &Tokens, x: f32, bar: Rect, limit: f32) -> f32 {
        let lines = self.findings();
        if lines.is_empty() {
            return x;
        }
        let font = theme::font(size::BAR);
        let count = lines.len().to_string();
        let w = text_width(ui, &count, &font);
        let r = Rect::from_min_size(pos2(x, bar.top() + 2.0), vec2(22.0 + w + 8.0, bar.height() - 4.0));
        if r.right() > limit {
            return x;
        }
        let label = format!("{} {} about how the drawing is shown", lines.len(), if lines.len() == 1 { "note" } else { "notes" });
        let resp = ui.interact(r, ui.scope_id().with("findings"), Sense::click());
        widgets::name_button(&resp, true, &label);
        paint_button_bg(ui, r, &resp, true, false, t, 4);
        let c = pos2(r.left() + 11.0, r.center().y);
        let ink = Stroke::new(1.3, t.text_dim);
        ui.painter().circle_stroke(c, 6.5, ink);
        ui.painter().line_segment([c + vec2(0.0, -3.0), c + vec2(0.0, 0.8)], ink);
        ui.painter().circle_filled(c + vec2(0.0, 3.2), 0.9, t.text_dim);
        ui.painter().text(pos2(r.left() + 22.0, r.center().y), Align2::LEFT_CENTER, &count, font, t.text_dim);
        self.note("status.findings", r, true);
        let resp = resp.on_hover_text(label);
        egui::Popup::menu(&resp).align(egui::RectAlign::TOP_START).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            ui.set_min_width(300.0);
            ui.set_max_width(460.0);
            egui::Frame::new().inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
                egui::ScrollArea::vertical().max_height(320.0).auto_shrink([true, true]).show(ui, |ui| {
                    let mut group = None;
                    for line in &lines {
                        if group != Some(line.group) {
                            group = Some(line.group);
                            ui.add_space(2.0);
                            ui.label(egui::RichText::new(line.group.heading()).strong().color(t.text));
                        }
                        let shown = ui.label(egui::RichText::new(format!("\u{2022} {}", line.text)).color(t.text_dim));
                        if let Some(detail) = &line.detail {
                            shown.on_hover_text(detail);
                        }
                    }
                });
            });
        });
        r.right() + 10.0
    }

    /// The lines behind the status bar's mark for the page on view.
    pub(crate) fn findings(&self) -> Vec<Finding> {
        let from_page = self.ui.page.map(|p| findings::from_render(self.ui.pictures.skipped(p))).unwrap_or_default();
        let mut lines = from_page;
        lines.extend(self.opening.reading.iter().cloned());
        lines
    }

    /// The list for `ui.inspect`: how many lines, and their words.
    pub(crate) fn findings_summary(&self) -> serde_json::Value {
        let lines = self.findings();
        json!({ "count": lines.len(), "lines": lines.iter().map(|l| l.text.clone()).collect::<Vec<_>>() })
    }

    /// A button that does nothing yet.
    fn quiet_button(&mut self, ui: &mut Ui, t: &Tokens, r: Rect, icon: Icon, label: &str, name: &str) {
        let resp = ui.interact(r, ui.scope_id().with(name), Sense::hover());
        widgets::name_button(&resp, false, label);
        icons::paint(ui.painter(), r.shrink(4.0), icon, t, false);
        self.note(name, r, false);
        resp.on_hover_text(tip(label, false));
    }

    /// The right end: three buttons, zoom out, the slider, zoom in, the percentage, the fit button. → the left edge of it all.
    fn status_right(&mut self, ui: &mut Ui, t: &Tokens, bar: Rect) -> f32 {
        // A narrow bar gives up the three buttons, then the slider and its buttons, before it gives up room for the page count.
        let (buttons, slider) = (bar.width() >= WIDE_BAR, bar.width() >= MEDIUM_BAR);
        let font = theme::font(size::BAR);
        let percent = format!("{:.0}%", self.zoom() * 100.0);
        let pct_w = text_width(ui, "400%", &font).max(text_width(ui, &percent, &font));
        let mut right = bar.right() - 2.0;
        // Fit to window.
        let fit = Rect::from_center_size(pos2(right - BUTTON / 2.0, bar.center().y), vec2(BUTTON, BUTTON));
        self.status_command_button(ui, t, fit, Icon::of(Base::Fit), ("Fit Page to Window", "view.fitPage", "status.fit"));
        right = fit.left() - 6.0;
        // The percentage.
        let pct = Rect::from_min_size(pos2(right - pct_w, bar.top()), vec2(pct_w, bar.height()));
        let zoom_enabled = self.command_enabled("window.zoomDialog").is_ok();
        let resp = ui.interact(pct, ui.scope_id().with("zoom_percent"), if zoom_enabled { Sense::click() } else { Sense::hover() });
        paint_button_bg(ui, pct.expand2(vec2(2.0, -2.0)), &resp, zoom_enabled, false, t, 4);
        ui.painter().text(pct.right_center(), Align2::RIGHT_CENTER, &percent, font.clone(), t.status_bar_text);
        widgets::name_button(&resp, zoom_enabled, &percent);
        self.note("status.zoom_percent", pct, zoom_enabled);
        if resp.on_hover_text(tip("Zoom", zoom_enabled)).clicked()
            && zoom_enabled
            && let Err(e) = self.execute("window.zoomDialog", &json!({}))
        {
            self.set_notice(&e.to_string());
        }
        right = pct.left() - 6.0;
        if !slider {
            return right - 6.0;
        }
        let plus = Rect::from_center_size(pos2(right - BUTTON / 2.0, bar.center().y), vec2(BUTTON, BUTTON));
        self.status_command_button(ui, t, plus, Icon::of(Base::Plus), ("Zoom In", "view.zoomIn", "status.zoom_in"));
        right = plus.left() - 2.0;
        let slider = Rect::from_min_size(pos2(right - SLIDER_W, bar.top()), vec2(SLIDER_W, bar.height()));
        self.zoom_slider(ui, t, slider);
        right = slider.left() - 2.0;
        let minus = Rect::from_center_size(pos2(right - BUTTON / 2.0, bar.center().y), vec2(BUTTON, BUTTON));
        self.status_command_button(ui, t, minus, Icon::of(Base::Minus), ("Zoom Out", "view.zoomOut", "status.zoom_out"));
        right = minus.left() - 10.0;
        if !buttons {
            return right - 6.0;
        }
        // Three buttons before them, the last Presentation Mode; the other two are unnamed in the sources.
        for (label, icon, name) in [
            ("Presentation Mode", Icon::of(Base::Play), "status.presentation"),
            ("View button 2", Icon::of(Base::Window), "status.view2"),
            ("View button 1", Icon::of(Base::Table), "status.view1"),
        ] {
            let r = Rect::from_center_size(pos2(right - BUTTON / 2.0, bar.center().y), vec2(BUTTON, BUTTON));
            self.quiet_button(ui, t, r, icon, label, name);
            right = r.left() - 1.0;
        }
        right - 6.0
    }

    fn status_command_button(&mut self, ui: &mut Ui, t: &Tokens, r: Rect, icon: Icon, what: (&str, &str, &str)) {
        let (label, command, name) = what;
        let enabled = self.command_enabled(command).is_ok();
        let resp = ui.interact(r, ui.scope_id().with(name), if enabled { Sense::click() } else { Sense::hover() });
        widgets::name_button(&resp, enabled, label);
        paint_button_bg(ui, r, &resp, enabled, false, t, 4);
        icons::paint(ui.painter(), r.shrink(4.0), icon, t, enabled);
        self.note(name, r, enabled);
        if resp.on_hover_text(tip(label, enabled)).clicked()
            && let Err(e) = self.execute(command, &json!({}))
        {
            self.set_notice(&e.to_string());
        }
    }

    /// The zoom slider: a track from 10% to 400% and a thumb; a press or a drag on it sets the zoom.
    fn zoom_slider(&mut self, ui: &mut Ui, t: &Tokens, r: Rect) {
        let enabled = self.command_enabled("view.zoom").is_ok();
        let resp = ui.interact(r, ui.scope_id().with("zoom_slider"), if enabled { Sense::click_and_drag() } else { Sense::hover() });
        let percent = self.zoom() * 100.0;
        resp.widget_info(|| egui::WidgetInfo::drag_value(enabled, percent));
        let track = Rect::from_min_max(pos2(r.left() + 7.0, r.center().y - 1.5), pos2(r.right() - 7.0, r.center().y + 1.5));
        let (lo, hi) = (MIN_ZOOM * 100.0, MAX_ZOOM * 100.0);
        if enabled
            && (resp.dragged() || resp.clicked())
            && let Some(p) = resp.interact_pointer_pos()
        {
            let f = f64::from(((p.x - track.left()) / track.width()).clamp(0.0, 1.0));
            let target = lo + f * (hi - lo);
            let _ = self.execute("view.zoom", &json!({ "percent": target }));
        }
        let painter = ui.painter();
        painter.rect_filled(track, CornerRadius::same(2), t.input_border);
        let f = ((self.zoom() * 100.0 - lo) / (hi - lo)).clamp(0.0, 1.0) as f32;
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
        self.note("status.zoom_slider", r, enabled);
        resp.on_hover_text(tip("Zoom", enabled));
    }

    /// Which page of how many the drawing area shows → `(n, of)`, counting from 1.
    pub fn page_position(&mut self) -> Option<(usize, usize)> {
        let current = self.current_page()?;
        let doc = self.session().document()?;
        // Counted in the order of the tabs: foreground pages, then background pages.
        let order = crate::page_tabs::tab_order(doc);
        let n = order.iter().position(|(id, _)| *id == current).map_or(1, |i| i + 1);
        Some((n, order.len()))
    }
}
