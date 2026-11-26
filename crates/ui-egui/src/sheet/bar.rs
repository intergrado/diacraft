//! The formula bar (design 5.2, 5.8): directly under the ribbon body and above the sheet, window-wide, with the reference name of the selected cell, the cancel and
//! accept buttons **before the text**, the formula and the value it works out to, and at the right end the `fx` button.
//!
//! It shows while a sheet tab is open and acts on the active tab; it goes when the last tab closes (a bar that came and went with the keyboard focus would move the whole window each
//! time the focus moved). **It edits**: a press in its text begins an edit of the selected cell's formula there (the cell and the bar edit one text), the **cancel and accept
//! buttons** work while an edit is open and are greyed while none is, and at the right of the text is the **value the text works out to** as it is typed (`= 3.25 in`), its error
//! in the error colour, or the fault in words. A refused text has a line under the bar that says what is wrong (`fault_line`).

use egui::{Align2, CornerRadius, Rect, Role, Sense, Stroke, StrokeKind, Ui, WidgetInfo, pos2, vec2};

use super::cell::{self, CELL_PAD, Ink};
use super::edit::{self, EditState};
use super::grid::Note;
use crate::theme::{self, Tokens, size};
use crate::widgets::{self, label_colour, text_width};

/// The height of the bar, in points.
pub const BAR_H: f32 = 30.0;

/// The width of the cancel, accept and `fx` buttons, in points.
const BUTTON_W: f32 = 26.0;

/// The room at the sides and between the parts, in points.
const GAP: f32 = 6.0;

/// The least the formula's own box is wide, in points (design 10: the bar keeps its two buttons and 120 points of text).
pub const TEXT_MIN: f32 = 120.0;

/// What the bar shows of the selected cell.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BarText {
    /// The reference name (`Geometry1.X2`), or empty with no cell selected.
    pub reference: String,
    /// The formula, whole.
    pub formula: String,
    /// The value it works out to, or its error.
    pub value: String,
    pub error: bool,
}

/// The width of the box of the reference name in a bar `width` points wide: a fifth of it, between 90 and 180.
pub fn name_width(width: f32) -> f32 {
    if width.is_finite() { (width * 0.2).clamp(90.0, 180.0) } else { 90.0 }
}

/// What a press in the bar asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Press {
    /// The cancel button: the edit is given up.
    Cancel,
    /// The accept button: the edit is committed, and the selection stays.
    Accept,
    /// A press in the text, with no edit in the bar: an edit begins there (or the one in the cell moves to the bar).
    Begin,
}

/// A button drawn in the bar: an icon in a box, greyed while nothing is being edited. → whether it was pressed.
fn button(ui: &Ui, rect: Rect, name: &str, enabled: bool, t: &Tokens, draw: impl FnOnce(&egui::Painter, Rect, egui::Color32)) -> bool {
    let resp = ui.interact(rect, ui.scope_id().with(("sheet_bar_button", name)), if enabled { Sense::click() } else { Sense::hover() });
    widgets::name_button(&resp, enabled, name);
    let painter = ui.painter();
    let fill = if enabled && resp.hovered() { t.accent_soft } else { t.control_idle_fill };
    painter.rect(rect.shrink2(vec2(0.0, 2.0)), CornerRadius::same(3), fill, Stroke::new(1.0, t.input_border), StrokeKind::Inside);
    draw(painter, rect.shrink2(vec2(5.0, 7.0)), if enabled { t.text } else { t.text_disabled });
    let clicked = enabled && resp.clicked();
    resp.on_hover_text(if enabled { name.to_owned() } else { format!("{name} (nothing is being edited)") });
    clicked
}

/// What the bar shows beside the text while an edit is open: the value the text works out to, the error it works out to, or the fault in words; or nothing.
fn live_text(edit: &EditState) -> Option<(String, Ink)> {
    let live = edit.live.as_ref()?;
    if live.exhausted {
        return Some(("= ...".to_owned(), Ink::Dim));
    }
    if let Some(fault) = &live.fault {
        return Some((fault.clone(), Ink::Error));
    }
    if live.error {
        return Some((format!("= {}", live.value), Ink::Error));
    }
    Some((format!("= {}", live.value), Ink::Dim))
}

/// What the bar is given: the selected cell's text, and the edit if one is open in this tab.
pub struct BarIn<'a> {
    pub text: &'a BarText,
    pub edit: Option<&'a mut EditState>,
}

/// What the bar did in the frame.
#[derive(Default)]
pub struct BarOut {
    pub notes: Vec<Note>,
    pub press: Option<Press>,
    /// Where the bar's formula box is, for the line under it.
    pub formula: Option<Rect>,
}

/// The line under the bar that says what is wrong with a refused text, drawn over the top of what lies below (no layout moves when it comes and goes). → where it was drawn.
pub fn fault_line(ctx: &egui::Context, bar: Rect, t: &Tokens, words: &str) -> Rect {
    let rect = Rect::from_min_size(bar.left_bottom(), vec2(bar.width(), 22.0));
    egui::Area::new(egui::Id::unique("sheet_bar_fault")).order(egui::Order::Foreground).fixed_pos(rect.min).interactable(false).show(ctx, |ui| {
        let (rect, resp) = ui.allocate_exact_size(rect.size(), Sense::hover());
        resp.widget_info(|| WidgetInfo::labeled(Role::Label, true, format!("Problem: {words}")));
        let painter = ui.painter();
        painter.rect(rect, 0.0, t.panel_header_fill, Stroke::new(1.0, t.sheet_error), StrokeKind::Inside);
        let galley = cell::cut(ui, words, t.sheet_error, rect.width() - 2.0 * CELL_PAD);
        painter.with_clip_rect(rect.shrink2(vec2(CELL_PAD, 0.0))).galley(
            pos2(rect.left() + CELL_PAD, rect.center().y - galley.size().y / 2.0),
            galley,
            t.sheet_error,
        );
    });
    rect
}

/// Draws the bar in `rect`. → the controls drawn, and what a press in it asked for.
pub fn paint(ui: &mut Ui, rect: Rect, t: &Tokens, input: BarIn<'_>) -> BarOut {
    let BarIn { text, mut edit } = input;
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, t.panel_fill);
    painter.hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0, t.separator));
    let mut out = BarOut::default();
    out.notes.push(Note { name: "sheet.bar".to_owned(), rect, enabled: true });
    let (top, h) = (rect.top() + 3.0, rect.height() - 6.0);
    let font = cell::font();
    let editing = edit.is_some();

    // The reference name, then the two buttons.
    let name = Rect::from_min_size(pos2(rect.left() + GAP, top), vec2(name_width(rect.width()), h));
    let cancel = Rect::from_min_size(pos2(name.right() + GAP, rect.top()), vec2(BUTTON_W, rect.height()));
    let accept = Rect::from_min_size(pos2(cancel.right() + 2.0, rect.top()), vec2(BUTTON_W, rect.height()));
    let fx = Rect::from_min_size(pos2(rect.right() - GAP - BUTTON_W, rect.top()), vec2(BUTTON_W, rect.height()));
    let formula = Rect::from_min_max(pos2(accept.right() + GAP, top), pos2((fx.left() - GAP).max(accept.right() + GAP + 1.0), top + h));
    out.formula = Some(formula);

    let boxed = |r: Rect| {
        painter.rect(r, CornerRadius::same(3), t.input_fill, Stroke::new(1.0, t.input_border), StrokeKind::Inside);
    };
    boxed(name);
    let reference = edit.as_ref().map_or(text.reference.as_str(), |e| e.reference.as_str());
    let resp = ui.interact(name, ui.scope_id().with("sheet_bar_name"), Sense::hover());
    resp.widget_info(|| WidgetInfo::labeled(Role::Label, true, format!("Reference: {reference}")));
    let galley = cell::cut(ui, reference, t.text, name.width() - 2.0 * CELL_PAD);
    painter.with_clip_rect(name.shrink2(vec2(CELL_PAD, 0.0))).galley(
        pos2(name.left() + CELL_PAD, name.center().y - galley.size().y / 2.0),
        galley,
        t.text,
    );
    out.notes.push(Note { name: "sheet.bar.name".to_owned(), rect: name, enabled: true });

    if button(ui, cancel, "Cancel", editing, t, |p, r, ink| {
        let stroke = Stroke::new(1.4, ink);
        p.line_segment([r.left_top() + vec2(2.0, 1.0), r.right_bottom() - vec2(2.0, 1.0)], stroke);
        p.line_segment([r.left_bottom() + vec2(2.0, -1.0), r.right_top() + vec2(-2.0, 1.0)], stroke);
    }) {
        out.press = Some(Press::Cancel);
    }
    if button(ui, accept, "Accept", editing, t, |p, r, ink| {
        let stroke = Stroke::new(1.5, ink);
        let (a, m, z) = (r.left_top() + vec2(1.0, 6.0), r.left_top() + vec2(5.0, 10.0), r.left_top() + vec2(12.0, 2.0));
        p.line_segment([a, m], stroke);
        p.line_segment([m, z], stroke);
    }) {
        out.press = Some(Press::Accept);
    }
    out.notes.push(Note { name: "sheet.bar.cancel".to_owned(), rect: cancel, enabled: editing });
    out.notes.push(Note { name: "sheet.bar.accept".to_owned(), rect: accept, enabled: editing });

    // The formula, and at the right of its box the value it works out to when there is room for both.
    boxed(formula);
    let inner = Rect::from_min_max(pos2(formula.left() + CELL_PAD, formula.top()), pos2(formula.right() - CELL_PAD, formula.bottom()));
    let beside = edit
        .as_deref()
        .and_then(live_text)
        .or_else(|| (!text.value.is_empty()).then(|| (format!("= {}", text.value), if text.error { Ink::Error } else { Ink::Dim })));
    // The value keeps at most two fifths of the box: a long one is cut at the box's edge, as a cell's text is, and the formula keeps the rest.
    let value_w = beside.as_ref().map_or(0.0, |(v, _)| text_width(ui, &v.chars().take(80).collect::<String>(), &font)).min(inner.width() * 0.4);
    let show_value = beside.is_some() && inner.width() - value_w - GAP >= TEXT_MIN;
    let text_room = if show_value { inner.width() - value_w - GAP } else { inner.width() };
    let text_rect = Rect::from_min_size(formula.min, vec2(text_room + 2.0 * CELL_PAD, formula.height()));
    let in_bar = edit.as_ref().is_some_and(|e| e.in_bar);
    if let (true, Some(e)) = (in_bar, edit.as_deref_mut()) {
        edit::show_editor(ui, text_rect, e, t, "Formula bar editor");
        out.notes.push(Note { name: "sheet.bar.formula".to_owned(), rect: formula, enabled: true });
    } else {
        let shown = edit.as_ref().map_or(text.formula.as_str(), |e| e.text.as_str());
        let resp = ui.interact(formula, ui.scope_id().with("sheet_bar_formula"), Sense::click());
        resp.widget_info(|| WidgetInfo::labeled(Role::Label, true, format!("Formula: {shown}")));
        let galley = cell::cut(ui, shown, t.text, text_room);
        painter.with_clip_rect(Rect::from_min_size(inner.min, vec2(text_room, inner.height()))).galley(
            pos2(inner.left(), inner.center().y - galley.size().y / 2.0),
            galley,
            t.text,
        );
        if resp.clicked() && (editing || !text.reference.is_empty()) {
            out.press = Some(Press::Begin);
        }
        resp.on_hover_ui(|ui| {
            ui.set_max_width(480.0);
            ui.add(egui::Label::new(if shown.is_empty() { "No cell is selected." } else { shown }).wrap());
        });
        out.notes.push(Note { name: "sheet.bar.formula".to_owned(), rect: formula, enabled: true });
    }
    if let (true, Some((words, ink))) = (show_value, beside) {
        // Left-aligned in its slot at the right end: a value too long for it is cut at the right, as a cell's text is.
        let slot = Rect::from_min_size(pos2(inner.right() - value_w, inner.top()), vec2(value_w, inner.height()));
        let galley = cell::cut(ui, &words, ink.colour(t), value_w);
        painter.with_clip_rect(slot).galley(pos2(slot.left(), inner.center().y - galley.size().y / 2.0), galley, ink.colour(t));
        out.notes.push(Note {
            name: "sheet.bar.value".to_owned(),
            rect: Rect::from_min_max(pos2(inner.right() - value_w, inner.top()), inner.max),
            enabled: false,
        });
    }

    // `fx` (its use is Insert Function, which a later task builds).
    let resp = ui.interact(fx, ui.scope_id().with("sheet_bar_fx"), Sense::hover());
    widgets::name_button(&resp, false, "Insert function");
    painter.text(fx.center(), Align2::CENTER_CENTER, "fx", theme::font_strong(size::LABEL), label_colour(t, false));
    resp.on_hover_text("Insert function (not available yet)");
    out.notes.push(Note { name: "sheet.bar.fx".to_owned(), rect: fx, enabled: false });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_name_box_is_a_fifth_of_the_bar_between_90_and_180_points() {
        assert_eq!(name_width(640.0), 128.0);
        assert_eq!(name_width(200.0), 90.0);
        assert_eq!(name_width(1920.0), 180.0);
        assert_eq!(name_width(f32::NAN), 90.0);
    }

    #[test]
    fn the_bar_keeps_the_room_for_its_text_at_the_narrowest_the_window_is_drawn() {
        // At 640 points: the margins, the name box, the two buttons, `fx` and the gaps leave the formula more than the least.
        let width = 640.0;
        let used = GAP + name_width(width) + GAP + BUTTON_W + 2.0 + BUTTON_W + GAP + GAP + BUTTON_W + GAP;
        assert!(width - used >= TEXT_MIN, "{}", width - used);
    }
}
