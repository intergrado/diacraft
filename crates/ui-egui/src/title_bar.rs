//! The title bar: the Quick Access Toolbar, the drawing's name followed by the application's, a search box,
//! the account, and the window buttons when the app draws its own frame.
//!
//! The order is (2024 to 2026) and R5 (the Quick Access Toolbar holds Save, Undo and Redo by
//! default). The search box and the account do nothing yet, so they are drawn quiet.

use egui::{Align, Align2, Frame, Layout, Rect, Sense, Stroke, Ui, UiBuilder, Vec2, pos2, vec2};
use serde_json::json;

use crate::app::{DiacraftApp, TOP_BAR_MARGIN, TOP_BAR_ROW_HEIGHT, drags_sent, handed_to_os, window_button};
use crate::icons::{self, Base, Icon};
use crate::theme::{self, Tokens, size};
use crate::widgets::{self, paint_input, text_width, tip};

/// The application name, as the bar shows it after the drawing's.
pub const APP_NAME: &str = "DiaCraft";

/// The Quick Access Toolbar's default controls, with the engine command each runs (`None`: nothing yet).
pub const QAT: [(&str, Base, Option<&str>); 3] =
    [("Save", Base::Save, Some("file.save")), ("Undo", Base::Undo, Some("edit.undo")), ("Redo", Base::Redo, Some("edit.redo"))];

const QAT_BUTTON: f32 = 28.0;
const SEARCH_MAX: f32 = 300.0;
const SEARCH_MIN: f32 = 110.0;

impl DiacraftApp {
    pub(crate) fn title_bar(&mut self, ui: &mut Ui, t: Tokens) {
        let frame = Frame::new().fill(t.top_bar_fill).inner_margin(TOP_BAR_MARGIN);
        let panel = egui::Panel::top("top_bar").show_separator_line(false).frame(frame).show(ui, |ui| {
            self.run_probe(crate::Stage::Panel("top_bar"));
            let (bar, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), TOP_BAR_ROW_HEIGHT), Sense::hover());
            if self.has_window_buttons() {
                // The whole strip, margin included; registered before the controls, which lie over it and take their own presses.
                let width = ui.ctx().content_rect().x_range();
                let (above, below) = (f32::from(TOP_BAR_MARGIN.top), f32::from(TOP_BAR_MARGIN.bottom));
                let drags_before = drags_sent(ui.ctx());
                craft_window::title_bar::empty_area(ui, Rect::from_x_y_ranges(width, (bar.top() - above)..=(bar.bottom() + below)));
                if drags_sent(ui.ctx()) > drags_before {
                    handed_to_os(ui.ctx());
                }
            }
            self.title_bar_row(ui, &t, bar);
        });
        let rect = panel.response.rect;
        ui.painter().hline(rect.x_range(), rect.bottom(), Stroke::new(1.0, t.separator));
    }

    fn title_bar_row(&mut self, ui: &mut Ui, t: &Tokens, bar: Rect) {
        let mut left = bar.left() + 2.0;
        // The window buttons of a frame the app draws, at the left or the right.
        let buttons = self.window_buttons_layout();
        if let Some(b) = &buttons {
            for button in &b.left {
                let r = Rect::from_min_size(pos2(left, bar.top()), vec2(32.0, bar.height()));
                ui.scope_builder(UiBuilder::new().max_rect(r).layout(Layout::left_to_right(Align::Center)), |ui| window_button(ui, *button, *t));
                left = r.right() + 2.0;
            }
        }
        // The Quick Access Toolbar starts directly after any left-side window buttons.
        left = self.quick_access(ui, t, left, bar);
        // The right end: the window buttons, then the account.
        let mut right = bar.right() - 2.0;
        if let Some(b) = &buttons {
            for button in b.right.iter().rev() {
                let r = Rect::from_min_size(pos2(right - 32.0, bar.top()), vec2(32.0, bar.height()));
                ui.scope_builder(UiBuilder::new().max_rect(r).layout(Layout::left_to_right(Align::Center)), |ui| window_button(ui, *button, *t));
                right = r.left() - 2.0;
            }
        }
        right = self.account(ui, t, right, bar);
        right = self.comments_button(ui, t, right, bar);
        self.name_and_search(ui, t, left + 6.0, right - 6.0, bar);
    }

    fn quick_access(&mut self, ui: &mut Ui, t: &Tokens, mut x: f32, bar: Rect) -> f32 {
        for (label, base, command) in QAT {
            let enabled = command.is_some_and(|c| self.command_enabled(c).is_ok()) && (label != "Save" || self.can_save());
            let r = Rect::from_min_size(pos2(x, bar.center().y - QAT_BUTTON / 2.0), Vec2::splat(QAT_BUTTON));
            let resp = ui.interact(r, ui.scope_id().with(("qat", label)), if enabled { Sense::click() } else { Sense::hover() });
            widgets::name_button(&resp, enabled, label);
            widgets::paint_button_bg(ui, r, &resp, enabled, false, t, 4);
            icons::paint(ui.painter(), r.shrink(5.0), Icon::of(base), t, enabled);
            self.note(format!("qat.{label}"), r, enabled);
            if resp.on_hover_text(tip(label, enabled)).clicked()
                && let Some(c) = command
            {
                if c == "file.save" {
                    // Save asks for a file first when the drawing has none.
                    self.save(ui.ctx());
                } else if let Err(e) = self.execute(c, &json!({})) {
                    self.set_notice(&e.to_string());
                }
            }
            x = r.right() + 1.0;
        }
        // The drop-down that customises the toolbar: nothing yet.
        let r = Rect::from_min_size(pos2(x, bar.center().y - 12.0), vec2(16.0, 24.0));
        let resp = ui.interact(r, ui.scope_id().with("qat_customise"), Sense::hover());
        widgets::name_button(&resp, false, "Customize Quick Access Toolbar");
        icons::chevron(ui.painter(), r.center(), 2.5, t.text_disabled);
        self.note("qat.customize", r, false);
        resp.on_hover_text(tip("Customize Quick Access Toolbar", false));
        r.right() + 4.0
    }

    fn account(&mut self, ui: &mut Ui, t: &Tokens, right: f32, bar: Rect) -> f32 {
        let r = Rect::from_center_size(pos2(right - 14.0, bar.center().y), Vec2::splat(26.0));
        let resp = ui.interact(r, ui.scope_id().with("account"), Sense::hover());
        widgets::name_button(&resp, false, "Account");
        ui.painter().circle(r.center(), 12.0, t.icon_fill_disabled, Stroke::new(1.0, t.input_border));
        icons::paint(ui.painter(), r.shrink(6.0), Icon::of(Base::Person), t, false);
        self.note("titlebar.account", r, false);
        resp.on_hover_text(tip("Account", false));
        r.left() - 8.0
    }

    /// The drawing's name and the application's, and the search box after them, in the room between `from` and `to`.
    fn name_and_search(&mut self, ui: &mut Ui, t: &Tokens, from: f32, to: f32, bar: Rect) {
        let title = self.window_title();
        let font = theme::font(size::BAR + 0.5);
        let want = text_width(ui, &title, &font);
        let room = (to - from).max(0.0);
        // The name keeps what it needs while the search box is given the rest, but never less than its minimum, and the box
        // goes altogether when the window is too narrow for it.
        let search_w = (room - want - 24.0).clamp(0.0, SEARCH_MAX);
        let show_search = search_w >= SEARCH_MIN;
        let name_w = if show_search { want.min(room - search_w - 24.0) } else { want.min(room) }.max(0.0);
        let name_rect = Rect::from_min_size(pos2(from, bar.top()), vec2(name_w, bar.height()));
        let painter = ui.painter().with_clip_rect(name_rect);
        painter.text(pos2(name_rect.left(), bar.center().y), Align2::LEFT_CENTER, &title, font, t.top_bar_text);
        // A label for the accessibility tree and tests, over the same rectangle.
        let resp = ui.interact(name_rect, ui.scope_id().with("window_title"), Sense::hover());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &title));
        self.note("titlebar.name", name_rect, true);
        if show_search {
            let free = Rect::from_min_max(pos2(name_rect.right() + 24.0, bar.top()), pos2(to, bar.bottom()));
            let w = search_w.min(free.width());
            let r = Rect::from_center_size(pos2(free.center().x, bar.center().y), vec2(w, 26.0));
            let resp = ui.interact(r, ui.scope_id().with("title_search"), Sense::hover());
            widgets::name_button(&resp, false, "Search");
            paint_input(ui, r, "Search", true, t, false);
            self.note("titlebar.search", r, false);
            resp.on_hover_text(tip("Search", false));
        }
    }
}
