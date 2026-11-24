//! The tab row and the ribbon body.
//!
//! The row holds File first (it opens the File view, not a ribbon body), then the tabs in the order of
//! And at its right end Comments and Share, which do nothing yet. Under it the body
//! shows the open tab's groups in order, each with its label under it. A body wider than the window folds groups
//! rather than clipping them (`fit`): a folded group is one button that opens a card with the group's controls, and every
//! menu in the card opens over it (`controls::fold_card`); the body folds away entirely to the tab row.
//!
//! The tab row and the card behind the body follow the sibling craft apps' ribbons (`docs/provenance.md`); the tables the
//! body is drawn from are in `spec`.

use egui::{Align2, CornerRadius, Frame, Rect, Sense, Stroke, Ui, UiBuilder, pos2, vec2};

use crate::app::DiacraftApp;
use crate::icons::{self, Badge, Base, Icon};
use crate::state::BackstagePage;
use crate::theme::{self, Tokens, size};
use crate::widgets::{self, label_colour, paint_button_bg, text_width, tip};

pub(crate) mod callouts;
pub(crate) mod change_shape;
pub(crate) mod controls;
pub(crate) mod design;
pub mod fit;
pub(crate) mod line_ends;
pub(crate) mod palette;
pub(crate) mod pickers;
pub(crate) mod quick_styles;
pub mod spec;

pub use controls::BODY_H;

/// The height of the row of tabs, in points.
pub const TAB_ROW_H: f32 = 30.0;

/// The width a tab has beyond its text, in points.
const TAB_PAD: f32 = 20.0;

/// The width a tab has beyond its text when the row is short of room, in points.
const TAB_PAD_NARROW: f32 = 10.0;

/// The width of the button that opens the menu of tabs the row has no room for, in points.
const OVERFLOW_W: f32 = 30.0;

/// The width of the row, in points, from which Comments and Share show.
const SHOW_END_FROM: f32 = 760.0;

/// How much of the row's right end the collapse control keeps clear in the body, in points.
const BODY_RIGHT_GUTTER: f32 = 22.0;

/// How a tab is drawn: its name, whether it is the File tab, and whether it is the open one.
#[derive(Clone, Copy)]
struct TabLook<'a> {
    name: &'a str,
    is_file: bool,
    active: bool,
}

impl DiacraftApp {
    /// The panel with the tab row and, unless folded, the ribbon body.
    pub(crate) fn ribbon_panel(&mut self, ui: &mut Ui) {
        let t = Tokens::get(ui.ctx());
        let collapsed = self.ui.ribbon_collapsed;
        // What the controls show of the selection is read once for the frame, before they are drawn; the fonts for the Font box come in
        // whenever the thread that found them is done (`format.rs`).
        self.refresh_format_view();
        self.refresh_page_view();
        self.refresh_design_view();
        self.refresh_container_view();
        self.refresh_swimlane_view();
        self.refresh_org_view();
        // A contextual tab goes when its object is not selected, and the ribbon goes back to the first tab.
        if self.ui.tab == "Container Format" && !self.container_selected() {
            self.ui.tab = crate::state::FIRST_TAB.to_owned();
        }
        if self.ui.tab == crate::ribbon::spec::CROSS_FUNCTIONAL_TAB && !self.swimlane_present() {
            self.ui.tab = crate::state::FIRST_TAB.to_owned();
        }
        // The Org Chart tab goes with the chart.
        if self.ui.tab == "Org Chart" && !self.org_chart_present() {
            self.ui.tab = crate::state::FIRST_TAB.to_owned();
        }
        self.ui.format.fonts.poll(ui.ctx());
        if collapsed {
            self.ui.ribbon_body = None;
        }
        let height = TAB_ROW_H + 1.0 + if collapsed { 0.0 } else { BODY_H + 1.0 };
        let frame = Frame::new().fill(t.top_bar_fill);
        egui::Panel::top("ribbon").exact_size(height).show_separator_line(false).frame(frame).show(ui, |ui| {
            let rect = ui.max_rect();
            let row = Rect::from_min_size(rect.min, vec2(rect.width(), TAB_ROW_H));
            self.tab_row(ui, &t, row);
            if collapsed {
                ui.painter().hline(rect.x_range(), row.bottom(), Stroke::new(1.0, t.ribbon_border));
            } else {
                let body = Rect::from_min_max(pos2(rect.left(), row.bottom()), rect.max);
                self.ribbon_body(ui, &t, body);
            }
        });
    }

    fn tab_row(&mut self, ui: &mut Ui, t: &Tokens, row: Rect) {
        let font = theme::font_strong(size::TAB);
        let names = spec::tab_names(self.tab_context());
        let comments_w = text_width(ui, "Comments", &theme::font(size::LABEL)) + 34.0;
        let share_w = text_width(ui, "Share", &theme::font(size::LABEL)) + 34.0;
        // Comments and Share show when the row is wide enough for them as well as the tabs.
        let wide = row.width() >= SHOW_END_FROM;
        let end_w = if self.ui.ribbon_collapsed { 30.0 } else { 0.0 } + if wide { comments_w + share_w + 12.0 } else { 0.0 };
        let avail = (row.width() - 16.0 - end_w).max(0.0);
        // Tabs lose some of their room first; then those that do not fit go in a menu.
        let widths = |pad: f32| -> Vec<f32> { names.iter().map(|n| text_width(ui, n, &font) + pad).collect() };
        let total = |w: &[f32]| w.iter().map(|w| w + 2.0).sum::<f32>();
        let mut w = widths(TAB_PAD);
        if total(&w) > avail {
            w = widths(TAB_PAD_NARROW);
        }
        let mut shown = names.len();
        if total(&w) > avail {
            let mut used = OVERFLOW_W;
            shown = w
                .iter()
                .take_while(|wi| {
                    used += **wi + 2.0;
                    used <= avail
                })
                .count()
                .max(1);
        }
        let (mut x, y0) = (row.left() + 8.0, row.top() + 4.0);
        for (name, w) in names.iter().zip(&w).take(shown) {
            let r = Rect::from_min_max(pos2(x, y0), pos2(x + w, row.bottom()));
            self.tab_button(ui, t, r, name);
            x += w + 2.0;
        }
        if shown < names.len() {
            let r = Rect::from_min_max(pos2(x, y0), pos2(x + OVERFLOW_W - 2.0, row.bottom()));
            self.tab_overflow(ui, t, r, &names[shown..]);
        }
        self.tab_row_right(ui, t, row, wide);
    }

    /// One tab of the row: draws it and does what a click or a double click on it does.
    fn tab_button(&mut self, ui: &mut Ui, t: &Tokens, r: Rect, name: &str) {
        let is_file = name == spec::FILE_TAB;
        let active = !is_file && self.ui.tab == name && self.ui.backstage.is_none();
        let resp = ui.interact(r, ui.scope_id().with(("tab", name)), Sense::click());
        widgets::name_button(&resp, true, name);
        self.note(format!("tab.{name}"), r, true);
        self.paint_tab(ui, t, r, TabLook { name, is_file, active }, &resp);
        if resp.clicked() {
            if is_file {
                self.ui.backstage = Some(BackstagePage::Home);
            } else {
                self.ui.tab = name.to_owned();
                self.ui.backstage = None;
                self.ui.ribbon_collapsed = false;
            }
        }
        if resp.double_clicked() && !is_file {
            self.ui.ribbon_collapsed = !self.ui.ribbon_collapsed;
        }
    }

    /// The button that opens a menu of the tabs the row has no room for.
    fn tab_overflow(&mut self, ui: &mut Ui, t: &Tokens, r: Rect, hidden: &[&'static str]) {
        let resp = ui.interact(r, ui.scope_id().with("tab_overflow"), Sense::click());
        widgets::name_button(&resp, true, "More tabs");
        paint_button_bg(ui, r, &resp, true, false, t, 4);
        for dx in [-5.0, 0.0, 5.0] {
            ui.painter().circle_filled(r.center() + vec2(dx, 3.0), 1.4, t.text_dim);
        }
        self.note("tab.overflow", r, true);
        let mut chosen = None;
        egui::Popup::menu(&resp.on_hover_text("More tabs")).close_behavior(egui::PopupCloseBehavior::CloseOnClick).show(|ui| {
            ui.set_min_width(110.0);
            for name in hidden {
                let row = ui.selectable_label(self.ui.tab == *name, *name);
                self.note(format!("tab.overflow.{name}"), row.rect, true);
                if row.clicked() {
                    chosen = Some(*name);
                }
            }
        });
        if let Some(name) = chosen {
            self.ui.tab = name.to_owned();
            self.ui.backstage = None;
            self.ui.ribbon_collapsed = false;
        }
    }

    fn paint_tab(&self, ui: &Ui, t: &Tokens, r: Rect, tab: TabLook<'_>, resp: &egui::Response) {
        let TabLook { name, is_file, active } = tab;
        let font = if active { theme::font_strong(size::TAB) } else { theme::font(size::TAB) };
        let painter = ui.painter();
        if is_file {
            let fill = if resp.is_pointer_button_down_on() || resp.hovered() { t.accent_hover } else { t.accent };
            painter.rect_filled(r.shrink2(vec2(0.0, 1.0)), CornerRadius { nw: 4, ne: 4, sw: 0, se: 0 }, fill);
            if resp.has_focus() {
                painter.rect_stroke(r.shrink(2.0), CornerRadius::same(3), Stroke::new(1.5, t.accent_text), egui::StrokeKind::Inside);
            }
            painter.text(r.center(), Align2::CENTER_CENTER, name, font, t.accent_text);
            return;
        }
        if active {
            let card = CornerRadius { nw: 4, ne: 4, sw: 0, se: 0 };
            painter.rect_filled(r, card, t.ribbon_fill);
            let line = Stroke::new(1.0, t.ribbon_border);
            painter.line_segment([r.left_bottom(), r.left_top() + vec2(0.0, 4.0)], line);
            painter.line_segment([r.left_top() + vec2(4.0, 0.0), r.right_top() - vec2(4.0, 0.0)], line);
            painter.line_segment([r.right_top() + vec2(0.0, 4.0), r.right_bottom()], line);
            painter.rect_filled(
                Rect::from_min_size(pos2(r.left() + 6.0, r.bottom() - 3.0), vec2(r.width() - 12.0, 3.0)),
                CornerRadius::same(1),
                t.accent,
            );
            painter.rect_filled(Rect::from_min_size(pos2(r.left() + 1.0, r.bottom() - 1.0), vec2(r.width() - 2.0, 2.0)), 0.0, t.ribbon_fill);
        } else {
            paint_button_bg(ui, r.shrink2(vec2(1.0, 2.0)), resp, true, false, t, 4);
        }
        if active && resp.has_focus() {
            painter.rect_stroke(r.shrink(2.0), CornerRadius::same(3), Stroke::new(1.5, t.accent), egui::StrokeKind::Inside);
        }
        painter.text(
            r.center() - vec2(0.0, if active { 1.0 } else { 0.0 }),
            Align2::CENTER_CENTER,
            name,
            font,
            if active { t.text } else { t.text_dim },
        );
    }

    /// Comments and Share at the right end of the row, and the control that unfolds a folded ribbon.
    fn tab_row_right(&mut self, ui: &mut Ui, t: &Tokens, row: Rect, wide: bool) {
        let font = theme::font(size::LABEL);
        let mut right = row.right() - 8.0;
        if self.ui.ribbon_collapsed {
            let r = Rect::from_min_size(pos2(right - 24.0, row.top() + 5.0), vec2(24.0, 22.0));
            let resp = ui.interact(r, ui.scope_id().with("ribbon_expand"), Sense::click());
            widgets::name_button(&resp, true, "Expand the Ribbon");
            paint_button_bg(ui, r, &resp, true, false, t, 3);
            icons::paint(ui.painter(), r.shrink(3.0), Icon::of(Base::Collapse), t, true);
            // The collapse icon points up; unfolded it points down.
            self.note("ribbon.expand", r, true);
            if resp.on_hover_text("Expand the Ribbon").clicked() {
                self.set_ribbon_collapsed(false);
            }
            right = r.left() - 6.0;
        }
        for (label, icon) in [("Share", Icon::of(Base::Person).with(Badge::Arrow)), ("Comments", Icon::of(Base::Bubble))] {
            let w = text_width(ui, label, &font) + 34.0;
            let r = Rect::from_min_size(pos2(right - w, row.top() + 5.0), vec2(w, 22.0));
            if !wide {
                break;
            }
            let resp = ui.interact(r, ui.scope_id().with(("tab_end", label)), Sense::hover());
            widgets::name_button(&resp, false, label);
            icons::paint(ui.painter(), Rect::from_center_size(pos2(r.left() + 14.0, r.center().y), vec2(15.0, 15.0)), icon, t, false);
            ui.painter().text(pos2(r.left() + 26.0, r.center().y), Align2::LEFT_CENTER, label, font.clone(), label_colour(t, false));
            self.note(format!("tab_end.{label}"), r, false);
            resp.on_hover_text(tip(label, false));
            right = r.left() - 4.0;
        }
    }

    fn set_ribbon_collapsed(&mut self, collapsed: bool) {
        self.ui.ribbon_collapsed = collapsed;
    }

    fn ribbon_body(&mut self, ui: &mut Ui, t: &Tokens, body: Rect) {
        self.ui.ribbon_body = Some(body);
        ui.painter().rect_filled(body, 0.0, t.ribbon_fill);
        ui.painter().hline(body.x_range(), body.top(), Stroke::new(1.0, t.ribbon_border));
        ui.painter().hline(body.x_range(), body.bottom() - 1.0, Stroke::new(1.0, t.ribbon_border));
        // The control at the lower right that folds the ribbon away.
        let collapse = Rect::from_min_size(pos2(body.right() - 20.0, body.bottom() - 22.0), vec2(16.0, 16.0));
        let resp = ui.interact(collapse, ui.scope_id().with("ribbon_collapse"), Sense::click());
        widgets::name_button(&resp, true, "Collapse the Ribbon");
        paint_button_bg(ui, collapse, &resp, true, false, t, 3);
        icons::paint(ui.painter(), collapse.shrink(1.0), Icon::of(Base::Collapse), t, true);
        self.note("ribbon.collapse", collapse, true);
        if resp.on_hover_text("Collapse the Ribbon").clicked() {
            self.set_ribbon_collapsed(true);
        }
        // A contextual tab that is not on the row (its object is not selected any more) shows the first tab's body.
        let on_row = spec::tab_names(self.tab_context()).iter().any(|n| *n == self.ui.tab);
        let Some(tab) = spec::tab_by_label(&self.ui.tab).filter(|_| on_row).or_else(|| spec::tab_by_label(crate::state::FIRST_TAB)) else { return };
        let inner = Rect::from_min_max(pos2(body.left() + 4.0, body.top() + 1.0), pos2(body.right() - BODY_RIGHT_GUTTER, body.bottom() - 1.0));
        let mut child = ui.new_child(UiBuilder::new().max_rect(inner));
        child.set_clip_rect(inner);
        self.draw_tab_groups(&mut child, t, tab, inner);
    }

    fn draw_tab_groups(&mut self, ui: &mut Ui, t: &Tokens, tab: &'static spec::Tab, inner: Rect) {
        let widths: Vec<fit::Widths> = tab.groups.iter().map(|g| controls::widths(ui, g)).collect();
        let levels = fit::plan(&widths, inner.width());
        let total: f32 = widths.iter().zip(&levels).map(|(w, l)| w.at(*l)).sum();
        let cx = controls::Cx { t: *t, tab: tab.label };
        egui::ScrollArea::horizontal()
            .id_salt(("ribbon_scroll", tab.label))
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(total.max(inner.width()), inner.height()), Sense::hover());
                let mut x = rect.left();
                for (group, level) in tab.groups.iter().zip(levels) {
                    x += controls::draw_group(self, ui, cx, group, level, pos2(x, rect.top() + 1.0));
                }
            });
    }
}
