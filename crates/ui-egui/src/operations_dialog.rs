//! The Offset and Fit Curve dialogs: the two operations that take a number, asked for in the page's
//! measurement unit.
//!
//! **What the specification gives, and what is ours.** R40 says the window asks for one positive distance in the page's measurement units, defaulting to 0.25 inch (6 mm on a
//! metric page), with OK and Cancel, and that a distance of 0 or less, or one that is not finite, is refused. R43 says Fit Curve's dialog asks for the tolerance in the page's units
//! and has three boxes for the flags (periodic splines, circular arcs, break at abrupt changes of direction). **No public source describes either dialog**: the
//! captions, the order and the defaults are ours, as the Zoom dialog's captions are. **Fit Curve's default tolerance is ours**, a hundredth of an inch (a quarter of a millimetre on
//! a metric page), and its three boxes start unticked, the flags being 0 then.
//!
//! **Nothing is sent until OK.** OK works out the number in the cells' own units: the typed length is read in the page's unit as the panes read one (a number with no unit takes the
//! page's, `measure::parse_length`) and goes through the page's drawing scale as Size & Position's fields do, so a distance typed on a page drawn at a scale is what the person sees on
//! the page, and the engine gets what its rows hold (`docs/editing-commands.md`, "Offset and Fit Curve"). Then OK runs the window's own `operations.offset {distance}` or
//! `operations.fitCurve {tolerance, flags}`, which send the engine's `shape.offset` and `shape.fit_curve` on the shapes selected. **A refusal stays in the dialog, in the engine's
//! words, and the dialog stays open**; Cancel and Escape change nothing, and Enter is OK while no box has the keyboard.

use diacraft_units::Unit;
use egui::{Context, Id, Key, Modifiers, RichText, vec2};
use serde_json::json;

use crate::app::DiacraftApp;
use crate::canvas::page_drawing::{PageSettings, page_settings};
use crate::measure::{length_text, parse_length};
use crate::theme::{self, Tokens, size};

/// The most characters a number box takes: a length with its unit, and no more than the text of one the window could use.
const FIELD_CHARS: usize = 40;

/// The distance an Offset starts with, in inches, and in millimetres on a metric page.
const DEFAULT_DISTANCE_INCHES: f64 = 0.25;
const DEFAULT_DISTANCE_MM: f64 = 6.0;

/// The tolerance a Fit Curve starts with (ours): a hundredth of an inch, a quarter of a millimetre on a metric page.
const DEFAULT_TOLERANCE_INCHES: f64 = 0.01;
const DEFAULT_TOLERANCE_MM: f64 = 0.25;

/// Whether the unit is a metric length, for the defaults.
fn is_metric(unit: Unit) -> bool {
    matches!(unit, Unit::Millimetre | Unit::Centimetre | Unit::Metre | Unit::Kilometre)
}

/// What a length typed in `text` is, in the cells' own units on a page with `settings`: the text read in the page's unit (a number with no unit takes it), as the drawing's length,
/// and carried through the drawing scale as the panes' fields do. The refusal is the measure's own words, or says the number is not one the command can use.
pub fn cells_of(text: &str, settings: &PageSettings) -> Result<f64, String> {
    let drawing = parse_length(text, settings.ruler_unit)?;
    let cells = drawing * settings.drawing_scale;
    if cells.is_finite() { Ok(cells) } else { Err("That is too large to be a length on this page.".to_owned()) }
}

/// The Offset dialog while it is open.
pub struct OffsetDialog {
    /// What the box holds.
    text: String,
    /// The page's settings the box is read with, taken when the dialog opened.
    settings: PageSettings,
    error: Option<String>,
}

/// The Fit Curve dialog while it is open.
pub struct FitCurveDialog {
    text: String,
    settings: PageSettings,
    /// `visSplinePeriodic`, `visSplineDoCircles` and `visSplineAbrupt`.
    periodic: bool,
    circles: bool,
    abrupt: bool,
    error: Option<String>,
}

impl FitCurveDialog {
    /// The flags the boxes say: a sum of 1, 2 and 4.
    fn flags(&self) -> u32 {
        u32::from(self.periodic) + 2 * u32::from(self.circles) + 4 * u32::from(self.abrupt)
    }
}

/// The text a length of `inches` is shown as in the page's unit.
fn shown(inches: f64, unit: Unit) -> String {
    length_text(inches, unit)
}

impl DiacraftApp {
    /// The settings of the page the shapes selected are on (the page's own defaults when there is none).
    fn selection_page_settings(&self) -> PageSettings {
        let page = self.ui.edit.selection_page().or(self.ui.page);
        self.session.document().zip(page).and_then(|(doc, page)| page_settings(doc, page)).unwrap_or_default()
    }

    /// The distance the Offset dialog is showing (`None`: it is closed). For the window state, tests and agents.
    pub fn offset_dialog_text(&self) -> Option<&str> {
        self.ui.offset_dialog.as_ref().map(|d| d.text.as_str())
    }

    /// The tolerance and the flags the Fit Curve dialog is showing (`None`: it is closed).
    pub fn fit_curve_dialog_state(&self) -> Option<(&str, u32)> {
        self.ui.fit_dialog.as_ref().map(|d| (d.text.as_str(), d.flags()))
    }

    /// Opens the Offset dialog with the distance of R40 in the box; one that is open stays as it is.
    pub(crate) fn open_offset_dialog(&mut self) {
        if self.ui.offset_dialog.is_none() {
            let settings = self.selection_page_settings();
            let inches = if is_metric(settings.ruler_unit) { DEFAULT_DISTANCE_MM / 25.4 } else { DEFAULT_DISTANCE_INCHES };
            self.ui.offset_dialog = Some(OffsetDialog { text: shown(inches, settings.ruler_unit), settings, error: None });
        }
    }

    pub(crate) fn close_offset_dialog(&mut self) {
        self.ui.offset_dialog = None;
    }

    /// Opens the Fit Curve dialog with the default tolerance and no flag; one that is open stays as it is.
    pub(crate) fn open_fit_curve_dialog(&mut self) {
        if self.ui.fit_dialog.is_none() {
            let settings = self.selection_page_settings();
            let inches = if is_metric(settings.ruler_unit) { DEFAULT_TOLERANCE_MM / 25.4 } else { DEFAULT_TOLERANCE_INCHES };
            self.ui.fit_dialog = Some(FitCurveDialog {
                text: shown(inches, settings.ruler_unit),
                settings,
                periodic: false,
                circles: false,
                abrupt: false,
                error: None,
            });
        }
    }

    pub(crate) fn close_fit_curve_dialog(&mut self) {
        self.ui.fit_dialog = None;
    }

    /// OK of the Offset dialog: the window's `operations.offset` with the distance in the cells' own units. A refusal is shown in the dialog, which stays open.
    fn apply_offset_dialog(&mut self) {
        let Some((text, settings)) = self.ui.offset_dialog.as_ref().map(|d| (d.text.clone(), d.settings)) else { return };
        let result = cells_of(&text, &settings)
            .and_then(|distance| self.execute("operations.offset", &json!({ "distance": distance })).map(|_| ()).map_err(|e| e.to_string()));
        match result {
            Ok(()) => self.close_offset_dialog(),
            Err(words) => {
                if let Some(dialog) = self.ui.offset_dialog.as_mut() {
                    dialog.error = Some(words);
                }
            }
        }
    }

    /// OK of the Fit Curve dialog: the window's `operations.fitCurve` with the tolerance in the cells' own units and the flags the boxes say.
    fn apply_fit_curve_dialog(&mut self) {
        let Some((text, settings, flags)) = self.ui.fit_dialog.as_ref().map(|d| (d.text.clone(), d.settings, d.flags())) else { return };
        let result = cells_of(&text, &settings).and_then(|tolerance| {
            self.execute("operations.fitCurve", &json!({ "tolerance": tolerance, "flags": flags })).map(|_| ()).map_err(|e| e.to_string())
        });
        match result {
            Ok(()) => self.close_fit_curve_dialog(),
            Err(words) => {
                if let Some(dialog) = self.ui.fit_dialog.as_mut() {
                    dialog.error = Some(words);
                }
            }
        }
    }

    /// The Offset dialog over the window: a number box with the page's unit, OK and Cancel.
    pub(crate) fn offset_dialog(&mut self, ctx: &Context, t: Tokens) {
        let width = (ctx.content_rect().width() - 60.0).clamp(240.0, 340.0);
        let typing = ctx.text_edit_focused();
        let Some(mut dialog) = self.ui.offset_dialog.take() else { return };
        let (mut ok, mut cancel) = (false, false);
        let mut seen: Vec<(String, egui::Rect, bool)> = Vec::new();
        let modal = egui::Modal::new(Id::unique("offset_dialog")).backdrop_color(t.scrim).frame(Self::card_frame(&t)).show(ctx, |ui| {
            ui.set_width(width);
            ui.label(RichText::new("Offset").font(theme::font(size::TAB + 1.0)).color(t.text));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Distance:").font(theme::font(size::LABEL)).color(t.text));
                let edit =
                    ui.add(egui::TextEdit::singleline(&mut dialog.text).desired_width(120.0).char_limit(FIELD_CHARS).font(theme::font(size::LABEL)));
                edit.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "offset.distance"));
                seen.push(("offset.distance".to_owned(), edit.rect, true));
                if edit.changed() {
                    dialog.error = None;
                }
            });
            ui.add_space(6.0);
            buttons_and_error(ui, &t, "offset", dialog.error.as_deref(), &mut seen, &mut ok, &mut cancel);
        });
        self.finish_dialog(ctx, modal.response.rect, seen, typing, &mut ok, &mut cancel);
        self.ui.offset_dialog = Some(dialog);
        if cancel {
            self.close_offset_dialog();
        } else if ok {
            self.apply_offset_dialog();
        }
    }

    /// The Fit Curve dialog over the window: a tolerance box with the page's unit, the three boxes of the flags, OK and Cancel.
    pub(crate) fn fit_curve_dialog(&mut self, ctx: &Context, t: Tokens) {
        let width = (ctx.content_rect().width() - 60.0).clamp(260.0, 360.0);
        let typing = ctx.text_edit_focused();
        let Some(mut dialog) = self.ui.fit_dialog.take() else { return };
        let (mut ok, mut cancel) = (false, false);
        let mut seen: Vec<(String, egui::Rect, bool)> = Vec::new();
        let modal = egui::Modal::new(Id::unique("fit_curve_dialog")).backdrop_color(t.scrim).frame(Self::card_frame(&t)).show(ctx, |ui| {
            ui.set_width(width);
            ui.label(RichText::new("Fit Curve").font(theme::font(size::TAB + 1.0)).color(t.text));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Tolerance:").font(theme::font(size::LABEL)).color(t.text));
                let edit =
                    ui.add(egui::TextEdit::singleline(&mut dialog.text).desired_width(120.0).char_limit(FIELD_CHARS).font(theme::font(size::LABEL)));
                edit.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "fit.tolerance"));
                seen.push(("fit.tolerance".to_owned(), edit.rect, true));
                if edit.changed() {
                    dialog.error = None;
                }
            });
            ui.add_space(4.0);
            for (name, caption, flag) in [
                ("fit.periodic", "Periodic splines", &mut dialog.periodic),
                ("fit.circles", "Circular arcs", &mut dialog.circles),
                ("fit.abrupt", "Break at abrupt changes", &mut dialog.abrupt),
            ] {
                let r = ui.checkbox(flag, RichText::new(caption).font(theme::font(size::LABEL)).color(t.text));
                seen.push((name.to_owned(), r.rect, true));
            }
            ui.add_space(6.0);
            buttons_and_error(ui, &t, "fit", dialog.error.as_deref(), &mut seen, &mut ok, &mut cancel);
        });
        self.finish_dialog(ctx, modal.response.rect, seen, typing, &mut ok, &mut cancel);
        self.ui.fit_dialog = Some(dialog);
        if cancel {
            self.close_fit_curve_dialog();
        } else if ok {
            self.apply_fit_curve_dialog();
        }
    }

    /// What every dialog of these two does after its card is drawn: keeps it centred, notes the controls, and takes Escape (Cancel) and Enter (OK, while no box has the keyboard).
    fn finish_dialog(
        &mut self,
        ctx: &Context,
        card: egui::Rect,
        seen: Vec<(String, egui::Rect, bool)>,
        typing: bool,
        ok: &mut bool,
        cancel: &mut bool,
    ) {
        crate::ask::keep_centred(ctx, card);
        for (name, rect, enabled) in seen {
            self.note(name, rect, enabled);
        }
        if !egui::Popup::is_any_open(ctx) {
            if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
                *cancel = true;
            }
            if !typing && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter)) {
                *ok = true;
            }
        }
    }
}

/// The engine's words when the command was refused, then OK and Cancel, each noted as `<prefix>.ok` and `<prefix>.cancel`.
fn buttons_and_error(
    ui: &mut egui::Ui,
    t: &Tokens,
    prefix: &str,
    error: Option<&str>,
    seen: &mut Vec<(String, egui::Rect, bool)>,
    ok: &mut bool,
    cancel: &mut bool,
) {
    if let Some(error) = error {
        let said = ui.add(egui::Label::new(RichText::new(error).font(theme::font(size::LABEL)).color(t.notice_text)).wrap());
        seen.push((format!("{prefix}.error"), said.rect, true));
        ui.add_space(4.0);
    }
    ui.horizontal(|ui| {
        for (label, key, primary) in [("OK", "ok", true), ("Cancel", "cancel", false)] {
            let b = ui.add(egui::Button::new(RichText::new(label).font(theme::font(size::LABEL))).min_size(vec2(84.0, 26.0)));
            seen.push((format!("{prefix}.{key}"), b.rect, true));
            if b.clicked() {
                if primary {
                    *ok = true;
                } else {
                    *cancel = true;
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(unit: Unit, drawing_scale: f64) -> PageSettings {
        PageSettings { ruler_unit: unit, drawing_scale, ..PageSettings::default() }
    }

    #[test]
    fn a_typed_length_is_read_in_the_pages_unit_and_carried_through_the_drawing_scale() {
        // Inches, scale 1: the number as it is; a unit typed is honoured.
        assert_eq!(cells_of("0.25", &page(Unit::Inch, 1.0)), Ok(0.25));
        assert_eq!(cells_of("0.25 in", &page(Unit::Inch, 1.0)), Ok(0.25));
        assert!((cells_of("1 ft", &page(Unit::Inch, 1.0)).unwrap() - 12.0).abs() < 1e-12);
        // A metric page: a number with no unit is in millimetres, and 25.4 of them are an inch.
        assert!((cells_of("25.4", &page(Unit::Millimetre, 1.0)).unwrap() - 1.0).abs() < 1e-12);
        assert!((cells_of("2.54 cm", &page(Unit::Millimetre, 1.0)).unwrap() - 1.0).abs() < 1e-12);
        // At a scale of 1 paper inch to 48 drawing inches the drawing scale is 1/48: 12 drawing inches are a quarter of a paper inch, the cells' own units.
        assert!((cells_of("12", &page(Unit::Inch, 1.0 / 48.0)).unwrap() - 0.25).abs() < 1e-12);
        // Words for what is not a length.
        for bad in ["", "abc", "1,5", "NaN", "inf", "1 ft, 5 in"] {
            assert!(cells_of(bad, &page(Unit::Inch, 1.0)).is_err(), "{bad:?}");
        }
        assert!(cells_of("1e308 ft", &page(Unit::Inch, 1.0e10)).is_err(), "an overflow is refused in words");
    }

    #[test]
    fn the_flags_the_boxes_say_are_the_sum_of_one_two_and_four() {
        let dialog = |periodic, circles, abrupt| FitCurveDialog {
            text: String::new(),
            settings: PageSettings::default(),
            periodic,
            circles,
            abrupt,
            error: None,
        };
        assert_eq!(dialog(false, false, false).flags(), 0);
        assert_eq!(dialog(true, false, false).flags(), 1);
        assert_eq!(dialog(false, true, false).flags(), 2);
        assert_eq!(dialog(false, false, true).flags(), 4);
        assert_eq!(dialog(true, true, true).flags(), 7);
    }

    #[test]
    fn only_the_four_metric_lengths_make_a_page_metric() {
        for unit in [Unit::Millimetre, Unit::Centimetre, Unit::Metre, Unit::Kilometre] {
            assert!(is_metric(unit));
        }
        for unit in [Unit::Inch, Unit::Foot, Unit::Point, Unit::Yard] {
            assert!(!is_metric(unit));
        }
    }
}
