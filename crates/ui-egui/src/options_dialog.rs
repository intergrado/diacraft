//! The Options dialog: File, Options, **Customize Ribbon**, and in it the box that turns the Developer tab on.
//!
//! "the Developer tab is hidden until turned on under File, Options, Customize Ribbon": the
//! Options dialog has a list of ten sections (General, Proofing, Save, Language, Accessibility, Advanced, Customize Ribbon, Quick Access Toolbar, Add-ins,
//! Trust Center), and its Main Tabs list orders the tabs as they sit on the row, with Developer unticked. **The Developer tab is off in a new window**, and
//! until this dialog the only way to turn it on was the window command `window.developer` (the control channel): the Operations menu is on it.
//!
//! **What works, and what is drawn quiet.** Customize Ribbon and Advanced are the two sections with anything behind them, so they are the two that are not quiet and that a click
//! chooses; the other eight are listed and say in a tooltip that they are not available yet. In Customize Ribbon's Main Tabs list the Developer tab's box is the one that can be
//! changed; the other tabs are ticked, because they always show, and quiet, because the window cannot hide them yet. **Advanced holds one box**, under the group Editing options:
//! "Select shapes partially within area" (off at first, and the window's own, not saved); the rest of that page is not built.
//! **OK sends one `window.developer`** when the Developer box was changed and one `window.selectPartially` when the Advanced box was, and nothing when neither was; Cancel and Escape send
//! nothing. The dialog opens from the File view's Options (the File view closes behind it, as the Office
//! apps' does) and from `window.options`.
//!
//! **The minimum distance between doubled connection points** is a box on the Advanced page, under Editing options: a length with a unit, in millimetres
//! at first (3 mm, the owner's default), refused in words outside 0.254 mm to 50.8 mm (0.01 in to 2 in); OK sends one `window.connectionMinGap` when it was changed, and a distance that is refused keeps the dialog open.
//!
//! **The window setting is not saved** (a tab shown is the window's, as the View tab's toggles are: `state.rs`); a new window starts with the tab off. The
//! captions are ours, from the words of no source read gives the dialog's own. **Ours:** the dialog's layout, and that the other tabs are listed
//! at all (R32 says only that the Options dialog's list orders the tabs as the row does).

use egui::{Context, CornerRadius, Id, Key, Modifiers, RichText, Sense, Stroke, StrokeKind, Ui, vec2};
use serde_json::json;

use diacraft_engine::draw::conn_points::{MAX_MIN_GAP, MIN_MIN_GAP};
use diacraft_units::Unit;

use crate::app::DiacraftApp;
use crate::measure::{length_text, parse_length};
use crate::theme::{self, Tokens, size};
use crate::widgets::tip;

/// The sections of the dialog's list, in the order.
pub const SECTIONS: [&str; 10] =
    ["General", "Proofing", "Save", "Language", "Accessibility", "Advanced", "Customize Ribbon", "Quick Access Toolbar", "Add-ins", "Trust Center"];

/// The section the dialog opens on.
const WORKING: &str = "Customize Ribbon";

/// The tabs of the dialog: the sections that have something behind them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OptionsTab {
    CustomizeRibbon,
    Advanced,
}

impl OptionsTab {
    /// Every tab that works, in the order of the list of sections.
    pub const ALL: [OptionsTab; 2] = [OptionsTab::Advanced, OptionsTab::CustomizeRibbon];

    pub const fn name(self) -> &'static str {
        match self {
            OptionsTab::CustomizeRibbon => WORKING,
            OptionsTab::Advanced => "Advanced",
        }
    }

    pub fn from_name(name: &str) -> Option<OptionsTab> {
        OptionsTab::ALL.into_iter().find(|t| t.name().eq_ignore_ascii_case(name.trim()))
    }
}

/// The Options dialog while it is open.
pub struct OptionsDialog {
    tab: OptionsTab,
    /// Whether the Developer tab was on when the dialog opened.
    start: bool,
    /// What the Developer box shows now.
    developer: bool,
    /// Whether "Select shapes partially within area" was on when the dialog opened, and what its box shows now.
    partial_start: bool,
    partial: bool,
    /// The minimum distance between doubled connection points as the box showed it when the dialog opened, and as it shows now (a length with a unit, millimetres at first).
    min_gap_start: String,
    min_gap: String,
    /// Why OK was refused: the box holds no distance in the range. Said under the box, and the dialog stays open.
    error: Option<String>,
}

/// What a press in the dialog chose, taken after the card is drawn.
#[derive(Default)]
struct Pressed {
    ok: bool,
    cancel: bool,
}

impl DiacraftApp {
    /// Whether the Options dialog is open, and on which tab.
    pub fn options_dialog_tab(&self) -> Option<&'static str> {
        self.ui.options_dialog.as_ref().map(|d| d.tab.name())
    }

    /// What the dialog's Developer box shows now (`None`: the dialog is closed). For tests and agents.
    pub fn options_dialog_shows_developer(&self) -> Option<bool> {
        self.ui.options_dialog.as_ref().map(|d| d.developer)
    }

    /// What the dialog's box "Select shapes partially within area" shows now (`None`: the dialog is closed). For tests and agents.
    pub fn options_dialog_shows_partial(&self) -> Option<bool> {
        self.ui.options_dialog.as_ref().map(|d| d.partial)
    }

    /// What the dialog's box "Minimum distance between doubled connection points" shows now (`None`: the dialog is closed). For tests and agents.
    pub fn options_dialog_min_gap_text(&self) -> Option<String> {
        self.ui.options_dialog.as_ref().map(|d| d.min_gap.clone())
    }

    /// The words OK was refused with, if it was (`None`: it was not, or the dialog is closed). For tests and agents.
    pub fn options_dialog_error(&self) -> Option<&str> {
        self.ui.options_dialog.as_ref().and_then(|d| d.error.as_deref())
    }

    /// Opens the dialog on `tab`, with the boxes as the window has them. One that is open keeps what was chosen. The File view closes behind it.
    pub(crate) fn open_options_dialog(&mut self, tab: OptionsTab) {
        self.ui.backstage = None;
        match self.ui.options_dialog.as_mut() {
            Some(dialog) => dialog.tab = tab,
            None => {
                let (on, partial) = (self.ui.developer_tab, self.ui.select_partial);
                let gap = length_text(self.connection_min_gap(), Unit::Millimetre);
                self.ui.options_dialog = Some(OptionsDialog {
                    tab,
                    start: on,
                    developer: on,
                    partial_start: partial,
                    partial,
                    min_gap_start: gap.clone(),
                    min_gap: gap,
                    error: None,
                });
            }
        }
    }

    pub(crate) fn close_options_dialog(&mut self) {
        self.ui.options_dialog = None;
    }

    /// OK: one `window.developer` if the Developer box was changed, one `window.selectPartially` if the Advanced box was and one `window.connectionMinGap` if the distance was, and the dialog closes. **A
    /// distance that is no length in the range keeps the dialog open** with the words under the box, and sends nothing.
    fn apply_options_dialog(&mut self) {
        let Some((start, now, partial_start, partial)) = self.ui.options_dialog.as_ref().map(|d| (d.start, d.developer, d.partial_start, d.partial))
        else {
            return;
        };
        let typed = self.ui.options_dialog.as_ref().filter(|d| d.min_gap.trim() != d.min_gap_start.trim()).map(|d| parse_min_gap(&d.min_gap));
        let gap = match typed {
            Some(Err(words)) => {
                if let Some(dialog) = self.ui.options_dialog.as_mut() {
                    dialog.error = Some(words);
                }
                return;
            }
            Some(Ok(inches)) => Some(inches),
            None => None,
        };
        self.close_options_dialog();
        if start != now
            && let Err(e) = self.execute("window.developer", &json!({"on": now}))
        {
            self.set_notice(&e.to_string());
        }
        if partial_start != partial
            && let Err(e) = self.execute("window.selectPartially", &json!({"on": partial}))
        {
            self.set_notice(&e.to_string());
        }
        if let Some(inches) = gap
            && let Err(e) = self.execute("window.connectionMinGap", &json!({"inches": inches}))
        {
            self.set_notice(&e.to_string());
        }
    }

    pub(crate) fn options_dialog(&mut self, ctx: &Context, t: Tokens) {
        let Some(mut dialog) = self.ui.options_dialog.take() else { return };
        let screen = ctx.content_rect();
        let width = (screen.width() - 60.0).clamp(280.0, 560.0);
        let content_h = (screen.height() - 190.0).clamp(140.0, 400.0);
        let mut pressed = Pressed::default();
        // Enter is OK while no control has the keyboard; one that has it (reached by Tab) takes Enter as a press of its own.
        if !egui::Popup::is_any_open(ctx) && ctx.memory(|m| m.focused().is_none()) && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter)) {
            pressed.ok = true;
        }
        let mut seen: Vec<(String, egui::Rect, bool)> = Vec::new();
        let modal = egui::Modal::new(Id::unique("options_dialog")).backdrop_color(t.scrim).frame(Self::card_frame(&t)).show(ctx, |ui| {
            ui.set_width(width);
            ui.label(RichText::new("Options").font(theme::font(size::TAB + 1.0)).color(t.text));
            ui.add_space(8.0);
            ui.style_mut().spacing.scroll = egui::style::ScrollStyle::solid();
            egui::ScrollArea::vertical()
                .id_salt("options_content")
                .min_scrolled_height(content_h)
                .max_height(content_h)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let inner = ui.available_width();
                    ui.set_width(inner);
                    body(ui, &t, &mut dialog, &mut seen);
                });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                for (label, key, ok) in [("OK", "options.ok", true), ("Cancel", "options.cancel", false)] {
                    let b = ui.add(egui::Button::new(RichText::new(label).font(theme::font(size::LABEL))).min_size(vec2(84.0, 26.0)));
                    seen.push((key.to_owned(), b.rect, true));
                    if b.clicked() {
                        if ok { pressed.ok = true } else { pressed.cancel = true }
                    }
                }
            });
        });
        crate::ask::keep_centred(ctx, modal.response.rect);
        for (name, rect, enabled) in seen {
            self.note(name, rect, enabled);
        }
        if !egui::Popup::is_any_open(ctx) && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            pressed.cancel = true;
        }
        self.ui.options_dialog = Some(dialog);
        if pressed.cancel {
            self.close_options_dialog();
        } else if pressed.ok {
            self.apply_options_dialog();
        }
    }
}

/// The least width the list of sections and the page beside it need, in points; a narrower dialog shows the page alone.
const BESIDE_FROM: f32 = 440.0;

/// The width of the list of sections, in points.
const LIST_W: f32 = 150.0;

/// The height of a row of a list, in points.
const ROW_H: f32 = 24.0;

/// The list of sections beside the page of the one open, or the page alone in a narrow window.
fn body(ui: &mut Ui, t: &Tokens, dialog: &mut OptionsDialog, seen: &mut Vec<(String, egui::Rect, bool)>) {
    let width = ui.available_width();
    if width < BESIDE_FROM {
        // The list of sections does not fit beside the page: the two that work are a row of buttons over it, so that a narrow window reaches both (`options.page.<name>`: the list itself, `options.section.<name>`, is left out there).
        ui.horizontal_wrapped(|ui| {
            for tab in OptionsTab::ALL {
                let button = ui.selectable_label(dialog.tab == tab, RichText::new(tab.name()).font(theme::font(size::LABEL)).color(t.text));
                seen.push((format!("options.page.{}", tab.name()), button.rect, true));
                if button.clicked() {
                    dialog.tab = tab;
                }
            }
        });
        ui.add_space(6.0);
        page(ui, t, dialog, seen);
        return;
    }
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.vertical(|ui| {
            ui.set_width(LIST_W);
            if let Some(chosen) = sections(ui, t, dialog.tab, seen) {
                dialog.tab = chosen;
            }
        });
        ui.add_space(12.0);
        ui.vertical(|ui| {
            ui.set_width(width - LIST_W - 12.0);
            page(ui, t, dialog, seen);
        });
    });
}

/// The list of the ten sections: the two that work can be chosen (the open one is shaded), and the rest are quiet and say they are not available yet. → the section a click chose.
fn sections(ui: &mut Ui, t: &Tokens, open: OptionsTab, seen: &mut Vec<(String, egui::Rect, bool)>) -> Option<OptionsTab> {
    ui.spacing_mut().item_spacing.y = 2.0;
    let mut chosen = None;
    for name in SECTIONS {
        let tab = OptionsTab::from_name(name);
        let works = tab.is_some();
        let (rect, resp) = ui.allocate_exact_size(vec2(LIST_W, ROW_H), if works { Sense::click() } else { Sense::hover() });
        resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, works, tab == Some(open), name));
        let painter = ui.painter_at(rect);
        if tab == Some(open) {
            painter.rect_filled(rect, CornerRadius::same(3), t.hover);
        }
        let colour = if works { t.text } else { t.text_disabled };
        painter.text(egui::pos2(rect.left() + 8.0, rect.center().y), egui::Align2::LEFT_CENTER, name, theme::font(size::LABEL), colour);
        seen.push((format!("options.section.{name}"), rect, works));
        if !works {
            resp.on_hover_text(tip(name, false));
        } else if resp.clicked() {
            chosen = tab;
        }
    }
    chosen
}

/// The page of the section that is open.
fn page(ui: &mut Ui, t: &Tokens, dialog: &mut OptionsDialog, seen: &mut Vec<(String, egui::Rect, bool)>) {
    match dialog.tab {
        OptionsTab::CustomizeRibbon => ribbon_page(ui, t, dialog, seen),
        OptionsTab::Advanced => advanced_page(ui, t, dialog, seen),
    }
}

/// The page of Advanced: the group Editing options, with the one box that works.
fn advanced_page(ui: &mut Ui, t: &Tokens, dialog: &mut OptionsDialog, seen: &mut Vec<(String, egui::Rect, bool)>) {
    ui.label(RichText::new("Advanced").font(theme::font(size::TAB)).color(t.text));
    ui.add_space(4.0);
    ui.label(RichText::new("Editing options").font(theme::font(size::LABEL)).color(t.text_dim));
    ui.spacing_mut().item_spacing.y = 2.0;
    if check_row(ui, t, "Select shapes partially within area", dialog.partial, true, "options.partial".to_owned(), "", seen) {
        dialog.partial = !dialog.partial;
    }
    ui.add_space(6.0);
    min_gap_row(ui, t, dialog, seen);
    ui.add_space(8.0);
    let words = "On, a net, Area Select and Lasso Select also select the shapes they touch, and not only the shapes wholly inside them. Doubling connection points (Ctrl+Alt+K) puts no point closer to another than the distance in the box, and leaves an edge with no room as it is. The choices are for this window only and are not saved. The rest of this page is not built yet.";
    let said = ui.add(egui::Label::new(RichText::new(words).font(theme::font(size::LABEL)).color(t.text_dim)).wrap());
    seen.push(("options.note".to_owned(), said.rect, true));
}

/// The longest text the box of the minimum distance takes, in characters.
const MIN_GAP_CHARS: usize = 32;

/// The row "Minimum distance between doubled connection points": a box that takes a length with a unit (a plain number is in millimetres, the unit the box
/// shows), and the words that refused it. **Ours:** the box's place under Editing options and its caption; the box shows millimetres at first, since the dialog has no choice of units yet.
fn min_gap_row(ui: &mut Ui, t: &Tokens, dialog: &mut OptionsDialog, seen: &mut Vec<(String, egui::Rect, bool)>) {
    const CAPTION: &str = "Minimum distance between doubled connection points";
    ui.label(RichText::new(CAPTION).font(theme::font(size::LABEL)).color(t.text));
    let edit = ui.add(egui::TextEdit::singleline(&mut dialog.min_gap).desired_width(120.0).char_limit(MIN_GAP_CHARS).font(theme::font(size::LABEL)));
    edit.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, CAPTION));
    seen.push(("options.min_gap".to_owned(), edit.rect, true));
    if edit.changed() {
        dialog.error = None;
    }
    if let Some(error) = &dialog.error {
        let said = ui.add(egui::Label::new(RichText::new(error).font(theme::font(size::LABEL)).color(t.notice_text)).wrap());
        seen.push(("options.error".to_owned(), said.rect, true));
    }
}

/// The distance the box holds, in paper inches: a length in the range of the setting (a plain number is in millimetres), else the words that say what the box takes.
fn parse_min_gap(text: &str) -> Result<f64, String> {
    let range = format!("Type a distance {}.", crate::edit::min_gap_range());
    // A hair of room at both ends: 0.254 mm typed is the least distance, and its inches come out a rounding away from 0.01.
    const SLACK: f64 = 1.0e-9;
    match parse_length(text, Unit::Millimetre) {
        Ok(inches) if inches.is_finite() && (MIN_MIN_GAP - SLACK..=MAX_MIN_GAP + SLACK).contains(&inches) => {
            Ok(inches.clamp(MIN_MIN_GAP, MAX_MIN_GAP))
        }
        Ok(_) => Err(range),
        Err(words) => Err(format!("{words} {range}")),
    }
}

/// The page of Customize Ribbon: the Main Tabs, with the Developer tab's box.
fn ribbon_page(ui: &mut Ui, t: &Tokens, dialog: &mut OptionsDialog, seen: &mut Vec<(String, egui::Rect, bool)>) {
    ui.label(RichText::new("Customize the ribbon").font(theme::font(size::TAB)).color(t.text));
    ui.add_space(4.0);
    ui.label(RichText::new("Main Tabs").font(theme::font(size::LABEL)).color(t.text_dim));
    ui.spacing_mut().item_spacing.y = 2.0;
    let tabs = crate::ribbon::spec::visible_tabs(crate::ribbon::spec::TabContext {
        developer: true,
        org_chart: false,
        ..crate::ribbon::spec::TabContext::default()
    });
    for tab in tabs {
        let working = tab.label == "Developer";
        let on = if working { dialog.developer } else { true };
        let quiet = format!("{} always shows: the window cannot hide it yet", tab.label);
        if check_row(ui, t, tab.label, on, working, format!("options.tab.{}", tab.label), &quiet, seen) {
            dialog.developer = !dialog.developer;
        }
    }
    ui.add_space(8.0);
    let words = "Tick Developer to show its tab on the ribbon, for this window only: the choice is not saved, so the next window starts with the tab off. The other tabs always show: the window cannot hide them yet.";
    let said = ui.add(egui::Label::new(RichText::new(words).font(theme::font(size::LABEL)).color(t.text_dim)).wrap());
    seen.push(("options.note".to_owned(), said.rect, true));
}

/// A check box row for `label`, known to tests as `key`: ticked or not, and whether a press can change it (when it cannot, `quiet` is its tooltip). → whether it was pressed.
#[allow(clippy::too_many_arguments)]
fn check_row(
    ui: &mut Ui,
    t: &Tokens,
    label: &str,
    on: bool,
    works: bool,
    key: String,
    quiet: &str,
    seen: &mut Vec<(String, egui::Rect, bool)>,
) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width().max(60.0), ROW_H), if works { Sense::click() } else { Sense::hover() });
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, works, on, label));
    let painter = ui.painter_at(rect);
    if works && resp.hovered() {
        painter.rect_filled(rect, CornerRadius::same(3), t.hover);
    }
    if resp.has_focus() {
        painter.rect_stroke(rect.shrink(0.5), CornerRadius::same(3), Stroke::new(1.0, t.accent), StrokeKind::Inside);
    }
    let b = egui::Rect::from_center_size(egui::pos2(rect.left() + 11.0, rect.center().y), vec2(14.0, 14.0));
    let (fill, edge) = if works { (if on { t.accent } else { t.input_fill }, t.input_border) } else { (t.input_fill, t.text_disabled) };
    painter.rect(b, CornerRadius::same(2), fill, Stroke::new(1.0, edge), StrokeKind::Inside);
    if on {
        let ink = if works { t.accent_text } else { t.text_disabled };
        let (p, m, q) = (b.left_top() + vec2(3.0, 7.0), b.left_top() + vec2(6.0, 10.0), b.left_top() + vec2(11.0, 4.0));
        painter.line_segment([p, m], Stroke::new(1.6, ink));
        painter.line_segment([m, q], Stroke::new(1.6, ink));
    }
    let colour = if works { t.text } else { t.text_disabled };
    painter.text(egui::pos2(rect.left() + 26.0, rect.center().y), egui::Align2::LEFT_CENTER, label, theme::font(size::LABEL), colour);
    seen.push((key, rect, works));
    if !works {
        resp.on_hover_text(quiet);
        return false;
    }
    resp.clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_is_the_one_the_specification_gives_and_one_section_works() {
        assert_eq!(SECTIONS.len(), 10);
        assert!(SECTIONS.contains(&WORKING));
        assert_eq!(OptionsTab::from_name(" customize ribbon "), Some(OptionsTab::CustomizeRibbon));
        assert_eq!(OptionsTab::from_name("advanced"), Some(OptionsTab::Advanced));
        assert_eq!(OptionsTab::from_name("General"), None);
    }
}
