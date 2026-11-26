//! The tab strip of the sheet area (design 5.3): one tab a sheet, named by the title of its object, each with a close button, and at the right a menu with Below,
//! Beside and Fill. In Fill the drawing has a tab of its own at the left of the sheet tabs, to go back to it.
//!
//! A title longer than its tab is cut with an ellipsis and whole in the tooltip. A strip with more tabs than it has room for scrolls sideways (the tab brought forward is scrolled into
//! view) and never lets two tabs overlap.

use egui::{Align, Align2, CornerRadius, Rect, Role, Sense, Stroke, Ui, UiBuilder, WidgetInfo, pos2, vec2};

use super::Dock;
use super::cell;
use super::grid::Note;
use crate::theme::{self, Tokens, size};
use crate::widgets::{self, paint_button_bg, text_width};

/// The height of the strip, in points.
pub const TAB_H: f32 = 28.0;

/// The narrowest and the widest a tab is, in points.
const TAB_MIN_W: f32 = 96.0;
const TAB_MAX_W: f32 = 220.0;

/// The width of a tab's close button and of the strip's menu, in points.
const CLOSE_W: f32 = 18.0;
const MENU_W: f32 = 28.0;

/// What the strip is given.
pub struct TabsIn<'a> {
    pub titles: &'a [String],
    pub active: usize,
    pub dock: Dock,
    /// In Fill, the name of the drawing's page, shown as a tab at the left.
    pub drawing: Option<&'a str>,
    /// The active tab was brought forward since the strip last scrolled to it.
    pub scroll_to_active: bool,
}

/// What a press on the strip asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TabAct {
    Activate(usize),
    Close(usize),
    Dock(Dock),
    /// Back to the drawing (Fill's own tab).
    Drawing,
}

/// The widths of `n` tabs whose titles are `wanted` points wide, in a strip `room` points wide: each as wide as its title wants (between the bounds), all of them narrower when the
/// strip lacks the room, and never narrower than the least. Finite for any input.
pub fn tab_widths(wanted: &[f32], room: f32) -> Vec<f32> {
    let n = wanted.len().max(1) as f32;
    let room = if room.is_finite() { room.max(0.0) } else { 0.0 };
    let each = (room / n).clamp(TAB_MIN_W, TAB_MAX_W);
    let sum: f32 = wanted.iter().map(|w| w.clamp(TAB_MIN_W, TAB_MAX_W)).sum();
    wanted.iter().map(|w| if sum <= room { w.clamp(TAB_MIN_W, TAB_MAX_W) } else { w.clamp(TAB_MIN_W, TAB_MAX_W).min(each) }).collect()
}

/// Draws the strip in `strip`. → the presses and the controls drawn.
pub fn paint(ui: &mut Ui, strip: Rect, t: &Tokens, input: &TabsIn<'_>) -> (Vec<TabAct>, Vec<Note>) {
    let mut acts = Vec::new();
    let mut notes = vec![Note { name: "sheet.tabs".to_owned(), rect: strip, enabled: true }];
    let painter = ui.painter_at(strip);
    painter.rect_filled(strip, 0.0, t.panel_header_fill);
    painter.hline(strip.x_range(), strip.bottom() - 0.5, Stroke::new(1.0, t.separator));
    let font = theme::font(size::LABEL);

    // The menu at the right.
    let menu = Rect::from_min_size(pos2(strip.right() - MENU_W, strip.top()), vec2(MENU_W, strip.height()));
    let resp = ui.interact(menu, ui.scope_id().with("sheet_dock_menu"), Sense::click());
    widgets::name_button(&resp, true, "ShapeSheet window position");
    paint_button_bg(ui, menu.shrink2(vec2(2.0, 3.0)), &resp, true, false, t, 3);
    for dx in [-4.5, 0.0, 4.5] {
        painter.circle_filled(menu.center() + vec2(dx, 0.0), 1.4, t.text_dim);
    }
    notes.push(Note { name: "sheet.dock".to_owned(), rect: menu, enabled: true });
    let current = input.dock;
    egui::Popup::menu(&resp.on_hover_text("ShapeSheet window position")).close_behavior(egui::PopupCloseBehavior::CloseOnClick).show(|ui| {
        ui.set_min_width(120.0);
        for (dock, label) in [(Dock::Below, "Below"), (Dock::Beside, "Beside"), (Dock::Fill, "Fill")] {
            let row = ui.selectable_label(current == dock, label);
            notes.push(Note { name: format!("sheet.dock.{}", dock.word()), rect: row.rect, enabled: true });
            if row.clicked() && current != dock {
                acts.push(TabAct::Dock(dock));
            }
        }
    });

    // The drawing's own tab in Fill, then the sheets'.
    let mut x = strip.left() + 4.0;
    if let Some(name) = input.drawing {
        let w = (text_width(ui, name, &font) + 24.0).clamp(TAB_MIN_W, TAB_MAX_W);
        let rect = Rect::from_min_size(pos2(x, strip.top() + 3.0), vec2(w, strip.height() - 3.0));
        let resp = ui.interact(rect, ui.scope_id().with("sheet_drawing_tab"), Sense::click());
        widgets::name_toggle(&resp, true, false, name);
        paint_tab(ui, t, rect, false, &resp);
        let clip = painter.with_clip_rect(rect.shrink2(vec2(8.0, 0.0)));
        clip.text(pos2(rect.left() + 10.0, rect.center().y), Align2::LEFT_CENTER, name, font.clone(), t.text_dim);
        notes.push(Note { name: "sheet.tab.drawing".to_owned(), rect, enabled: true });
        if resp.clicked() {
            acts.push(TabAct::Drawing);
        }
        x = rect.right() + 4.0;
    }
    let room = (menu.left() - x - 4.0).max(0.0);
    let area = Rect::from_min_max(pos2(x, strip.top()), pos2(x + room, strip.bottom()));
    let wanted: Vec<f32> = input.titles.iter().map(|title| text_width(ui, title, &font) + 14.0 + CLOSE_W + 6.0).collect();
    let widths = tab_widths(&wanted, room);
    let total: f32 = widths.iter().sum::<f32>() + 2.0 * widths.len() as f32;
    let mut child = ui.new_child(UiBuilder::new().max_rect(area));
    child.set_clip_rect(area);
    egui::ScrollArea::horizontal()
        .id_salt("sheet_tab_scroll")
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .auto_shrink([false, false])
        .show(&mut child, |ui| {
            let (content, _) = ui.allocate_exact_size(vec2(total.max(area.width()), area.height()), Sense::hover());
            let mut tx = content.left();
            for (i, (title, w)) in input.titles.iter().zip(&widths).enumerate() {
                let rect = Rect::from_min_size(pos2(tx, content.top() + 3.0), vec2(*w, content.height() - 3.0));
                tx += w + 2.0;
                let active = i == input.active;
                let resp = ui.interact(rect, ui.scope_id().with(("sheet_tab", i)), Sense::click());
                resp.widget_info(|| WidgetInfo::selected(Role::Tab, true, active, title.as_str()));
                paint_tab(ui, t, rect, active, &resp);
                let text_room = (rect.width() - 14.0 - CLOSE_W - 4.0).max(8.0);
                let galley = cell::elided(ui, title, if active { t.text } else { t.text_dim }, text_room);
                ui.painter().with_clip_rect(rect).galley(pos2(rect.left() + 10.0, rect.center().y - galley.size().y / 2.0), galley, t.text);
                let close = Rect::from_center_size(pos2(rect.right() - 4.0 - CLOSE_W / 2.0, rect.center().y), vec2(CLOSE_W, CLOSE_W));
                let cresp = ui.interact(close, ui.scope_id().with(("sheet_tab_close", i)), Sense::click());
                let words = format!("Close {title}");
                widgets::name_button(&cresp, true, &words);
                paint_button_bg(ui, close, &cresp, true, false, t, 3);
                let (c, r) = (close.center(), 3.5);
                let stroke = Stroke::new(1.3, t.text);
                ui.painter().line_segment([c + vec2(-r, -r), c + vec2(r, r)], stroke);
                ui.painter().line_segment([c + vec2(-r, r), c + vec2(r, -r)], stroke);
                if ui.is_rect_visible(rect) && rect.intersects(area) {
                    notes.push(Note { name: format!("sheet.tab.{i}"), rect, enabled: true });
                    notes.push(Note { name: format!("sheet.tab.{i}.close"), rect: close, enabled: true });
                }
                if active && input.scroll_to_active {
                    ui.scroll_to_rect(rect, Some(Align::Center));
                }
                let (closed, pressed) = (cresp.clicked(), resp.clicked());
                cresp.on_hover_text(words);
                resp.on_hover_text(title);
                if closed {
                    acts.push(TabAct::Close(i));
                } else if pressed && !active {
                    acts.push(TabAct::Activate(i));
                }
            }
        });
    (acts, notes)
}

/// The shape of a tab: the active one takes the sheet's ground and an accent line, the others the strip's.
fn paint_tab(ui: &Ui, t: &Tokens, rect: Rect, active: bool, resp: &egui::Response) {
    let round = CornerRadius { nw: 4, ne: 4, sw: 0, se: 0 };
    let painter = ui.painter();
    if active {
        painter.rect_filled(rect, round, t.panel_fill);
        painter.rect_filled(
            Rect::from_min_size(pos2(rect.left() + 4.0, rect.bottom() - 2.5), vec2(rect.width() - 8.0, 2.5)),
            CornerRadius::same(1),
            t.accent,
        );
    } else if resp.hovered() {
        painter.rect_filled(rect, round, t.hover);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabs_are_as_wide_as_their_titles_between_the_bounds_and_narrower_when_the_strip_lacks_room() {
        assert_eq!(tab_widths(&[50.0, 150.0, 400.0], 1000.0), [TAB_MIN_W, 150.0, TAB_MAX_W]);
        // Three tabs of 220 in a strip of 450: each 150.
        assert_eq!(tab_widths(&[300.0, 300.0, 300.0], 450.0), [150.0, 150.0, 150.0]);
        // More tabs than the strip holds at their least: each the least, and the strip scrolls.
        assert_eq!(tab_widths(&[300.0; 10], 300.0), [TAB_MIN_W; 10]);
        assert_eq!(tab_widths(&[], 100.0), Vec::<f32>::new());
        for odd in [f32::NAN, f32::INFINITY, -5.0, 0.0] {
            let w = tab_widths(&[120.0, 120.0], odd);
            assert!(w.iter().all(|w| w.is_finite() && (TAB_MIN_W..=TAB_MAX_W).contains(w)), "{odd}: {w:?}");
        }
        // Never two tabs over one another: the sum of the widths and gaps is what the content holds.
        let w = tab_widths(&[200.0, 200.0, 200.0], 400.0);
        assert!(w.iter().all(|w| *w >= TAB_MIN_W));
    }
}
