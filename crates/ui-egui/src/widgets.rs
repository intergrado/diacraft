//! Small widgets the window's parts share: a button's background, an icon button, a quiet input box.
//!
//! The look of a button follows the ribbon button of the sibling craft apps (an icon over a label, a tinted fill under
//! the pointer, the quiet colours of a control that cannot be used); the code is written for this window and the
//! tokens are DiaCraft's own (`docs/provenance.md`).

use egui::{Align2, Color32, CornerRadius, FontId, Rect, Response, Role, Stroke, StrokeKind, Ui, WidgetInfo, pos2, vec2};

use crate::icons::{self, Icon};
use crate::theme::{self, Tokens, size};

/// The width of `text` in `font`, in points.
pub fn text_width(ui: &Ui, text: &str, font: &FontId) -> f32 {
    let colour = ui.visuals().text_color();
    ui.fonts_mut(|f| f.layout_no_wrap(text.to_owned(), font.clone(), colour).size().x)
}

/// The colour of a control's text.
pub fn label_colour(t: &Tokens, enabled: bool) -> Color32 {
    if enabled { t.text } else { t.text_disabled }
}

/// Paints the fill behind a button: tinted under the pointer, darker while pressed, the accent's tint while `selected`.
/// Nothing for a control that cannot be used, unless it is `selected`.
pub fn paint_button_bg(ui: &Ui, rect: Rect, response: &Response, enabled: bool, selected: bool, t: &Tokens, radius: u8) {
    let fill = if enabled && response.is_pointer_button_down_on() {
        Some(t.pressed)
    } else if selected {
        Some(t.accent_soft)
    } else if enabled && response.hovered() {
        Some(t.hover)
    } else {
        None
    };
    if let Some(fill) = fill {
        ui.painter().rect_filled(rect, CornerRadius::same(radius), fill);
    }
    if enabled && (selected || response.has_focus()) {
        let weight = if response.has_focus() { 1.5 } else { 1.0 };
        ui.painter().rect_stroke(rect, CornerRadius::same(radius), Stroke::new(weight, t.accent), StrokeKind::Inside);
    }
}

/// Names a response for assistive technology and for tests: a button called `label`, usable or not.
pub fn name_button(response: &Response, enabled: bool, label: &str) {
    response.widget_info(|| WidgetInfo::labeled(Role::Button, enabled, label));
}

/// Names a response as a button that is pressed or not (Bold, Align Left): `selected` is whether it is pressed.
pub fn name_toggle(response: &Response, enabled: bool, selected: bool, label: &str) {
    response.widget_info(|| WidgetInfo::selected(Role::Button, enabled, selected, label));
}

/// Names a response as a check box.
pub fn name_check(response: &Response, enabled: bool, checked: bool, label: &str) {
    response.widget_info(|| WidgetInfo::selected(Role::CheckBox, enabled, checked, label));
}

/// The text a tooltip shows: the label, with why a control that cannot be used is dimmed.
pub fn tip(label: &str, enabled: bool) -> String {
    if enabled { label.to_owned() } else { format!("{label} (not available yet)") }
}

/// A quiet input box drawn in `rect`: the hint in it, a magnifier at its left when `search`. For an input that does nothing yet
/// (so it is not a text edit).
pub fn paint_input(ui: &Ui, rect: Rect, hint: &str, search: bool, t: &Tokens, enabled: bool) {
    let painter = ui.painter();
    painter.rect(rect, CornerRadius::same(4), t.input_fill, Stroke::new(1.0, t.input_border), StrokeKind::Inside);
    let mut x = rect.left() + 8.0;
    if search {
        icons::paint(painter, Rect::from_center_size(pos2(x + 6.0, rect.center().y), vec2(14.0, 14.0)), Icon::of(icons::Base::Magnifier), t, enabled);
        x += 20.0;
    }
    painter.text(pos2(x, rect.center().y), Align2::LEFT_CENTER, hint, theme::font(size::LABEL), t.text_disabled);
}

/// Breaks `label` over at most two lines at the space nearest its middle, when it is long enough to need it.
pub fn two_lines(label: &str) -> (String, Option<String>) {
    if label.chars().count() <= 9 || !label.contains(' ') {
        return (label.to_owned(), None);
    }
    let middle = label.chars().count() / 2;
    let best = label
        .char_indices()
        .filter(|(_, c)| *c == ' ')
        .min_by_key(|(i, _)| label.get(..*i).map_or(usize::MAX, |s| s.chars().count().abs_diff(middle)));
    match best {
        Some((i, _)) => (label.get(..i).unwrap_or(label).to_owned(), label.get(i + 1..).map(str::to_owned)),
        None => (label.to_owned(), None),
    }
}

/// A straight divider line from `a` to `b`.
pub fn divider(ui: &Ui, a: egui::Pos2, b: egui::Pos2, t: &Tokens) {
    ui.painter().line_segment([a, b], Stroke::new(1.0, t.group_divider));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_labels_break_at_the_middle_space() {
        assert_eq!(two_lines("Paste"), ("Paste".to_owned(), None));
        assert_eq!(two_lines("Format Painter"), ("Format".to_owned(), Some("Painter".to_owned())));
        assert_eq!(two_lines("Apply Theme to New Shapes"), ("Apply Theme".to_owned(), Some("to New Shapes".to_owned())));
        assert_eq!(two_lines("Supercalifragilistic"), ("Supercalifragilistic".to_owned(), None));
    }
}
