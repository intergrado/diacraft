//! The Show Sections card: a list of check boxes, one for each section the sheet shows, which hides any of them. Its state is the tab's own and is not saved: all
//! the boxes are ticked at first.
//!
//! A press on a box sends `window.sheetSections` with the names of the sections that are then ticked, so that the window command is the one way the state changes (a person, a program and
//! a test reach the same code). The card stays open while the boxes are ticked and unticked, and closes with a press outside it.

use egui::{Align2, CornerRadius, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use serde_json::json;

use crate::app::DiacraftApp;
use crate::ribbon::controls::submenu_height;
use crate::theme::{self, Tokens, size};
use crate::widgets::{self, paint_button_bg};

/// The room a row has for its box, in points, and the height of a row.
const ROW_H: f32 = 24.0;

impl DiacraftApp {
    /// The rows of the card: the name of each section the active sheet shows, its label and whether it is ticked.
    pub(crate) fn sheet_sections_rows(&self) -> Vec<(String, String, bool)> {
        let sheet = &self.ui.sheet;
        let Some(tab) = sheet.tabs.get(sheet.active) else { return Vec::new() };
        let Some(view) = tab.view.as_ref() else { return Vec::new() };
        view.base().sections.iter().map(|s| (s.name.clone(), s.label.clone(), !tab.hidden.contains(&s.name))).collect()
    }
}

/// The body of the Show Sections menu: a box for each section, in the sheet's order.
pub fn sections_menu(app: &mut DiacraftApp, ui: &mut Ui, t: &Tokens) {
    let rows = app.sheet_sections_rows();
    ui.set_min_width(220.0);
    if rows.is_empty() {
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::hover());
        let words = "No ShapeSheet is open";
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, words));
        ui.painter().text(pos2(rect.left() + 12.0, rect.center().y), Align2::LEFT_CENTER, words, theme::font(size::LABEL), t.text_dim);
        return;
    }
    let height = submenu_height(ui.ctx().content_rect().height());
    let mut toggled: Option<String> = None;
    egui::ScrollArea::vertical().max_height(height).auto_shrink([true, true]).show(ui, |ui| {
        for (name, label, shown) in &rows {
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width().max(200.0), ROW_H), Sense::click());
            widgets::name_check(&resp, true, *shown, label);
            paint_button_bg(ui, rect, &resp, true, false, t, 3);
            let b = egui::Rect::from_center_size(pos2(rect.left() + 12.0, rect.center().y), vec2(13.0, 13.0));
            let painter = ui.painter();
            painter.rect(
                b,
                CornerRadius::same(2),
                if *shown { t.accent } else { t.input_fill },
                Stroke::new(1.0, t.input_border),
                StrokeKind::Inside,
            );
            if *shown {
                let (a, m, z) = (b.left_top() + vec2(2.5, 6.5), b.left_top() + vec2(5.5, 9.5), b.left_top() + vec2(10.5, 3.5));
                painter.line_segment([a, m], Stroke::new(1.6, t.accent_text));
                painter.line_segment([m, z], Stroke::new(1.6, t.accent_text));
            }
            painter.text(pos2(rect.left() + 30.0, rect.center().y), Align2::LEFT_CENTER, label, theme::font(size::LABEL), t.text);
            app.note(format!("sheet.sections.{name}"), rect, true);
            if resp.clicked() {
                toggled = Some(name.clone());
            }
        }
    });
    if let Some(name) = toggled {
        let shown: Vec<&str> = rows.iter().filter(|(n, _, on)| if *n == name { !*on } else { *on }).map(|(n, _, _)| n.as_str()).collect();
        let _ = app.execute("window.sheetSections", &json!({ "shown": shown }));
    }
}
