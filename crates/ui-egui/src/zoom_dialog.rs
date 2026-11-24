//! The Zoom dialog: View > Zoom > Zoom (`ZoomDialog`) and the zoom percentage of the status bar.
//!
//! **What the specification gives** (test vector V4): a dialog titled "Zoom" with a group "Magnification" of nine radio buttons in this order, 400%,
//! 200%, 150%, 100%, 75%, 50%, Page width, Whole page and Percentage with a text box, then OK and Cancel; **Whole page is the choice it opens with.** The English
//! captions are ours, read from the one (German) image, and so is the Percentage box's range (the specification leaves it open): the window's own, 10% to
//! 400% (`MIN_ZOOM` and `MAX_ZOOM`), refused in words when a typed number is outside it, as a window that clamps silently would show a zoom the person did not
//! ask for. The dialog has no help button: the window has no help to open.
//!
//! **Nothing is sent until OK.** The dialog changes only the window's view, never the drawing, so it sends no engine command: OK runs the window's own
//! `view.zoom {percent}` for a listed percentage or a typed one, `view.fitWidth` for Page width (the View tab's Page Width button) and `view.fitPage` for Whole page
//! (Fit to Window), so the dialog and the buttons cannot disagree. Cancel and Escape change nothing; Enter is OK while no box has the keyboard, as in the Page Setup
//! dialog. The Percentage box starts with the zoom the view has now, and typing in it, or putting the keyboard in it, chooses Percentage.
//!
//! The status bar's percentage opens the same dialog. **No page says that a click there does so for the reference application** ("in
//! Office-like apps"); it is DiaCraft's reading, and it is one line of the status bar to take out.

use egui::{Context, Id, Key, Modifiers, RichText, Ui, vec2};
use serde_json::json;

use crate::app::DiacraftApp;
use crate::canvas::view::{MAX_ZOOM, MIN_ZOOM};
use crate::theme::{self, Tokens, size};

/// What the dialog's radio buttons choose between.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    /// A listed percentage.
    Preset(u16),
    /// The page's width fills the drawing area (`view.fitWidth`).
    PageWidth,
    /// The whole page is in the drawing area (`view.fitPage`).
    WholePage,
    /// The number typed in the box.
    Percent,
}

/// The nine choices in the order of the specification, each with the name it is known by to tests and agents (`zoom.choice.<name>`, and the dialog's value in
/// the window state) and its caption.
pub const CHOICES: [(Choice, &str, &str); 9] = [
    (Choice::Preset(400), "400", "400%"),
    (Choice::Preset(200), "200", "200%"),
    (Choice::Preset(150), "150", "150%"),
    (Choice::Preset(100), "100", "100%"),
    (Choice::Preset(75), "75", "75%"),
    (Choice::Preset(50), "50", "50%"),
    (Choice::PageWidth, "page_width", "Page width"),
    (Choice::WholePage, "whole_page", "Whole page"),
    (Choice::Percent, "percent", "Percentage"),
];

impl Choice {
    /// The name the choice is known by (`zoom.choice.<name>`).
    pub fn name(self) -> &'static str {
        CHOICES.iter().find(|(c, ..)| *c == self).map_or("", |(_, name, _)| name)
    }
}

/// The height of the list of nine choices, in points: a row is 20 and the space between two is 2.
const LIST_H: f32 = 9.0 * 22.0 + 4.0;

/// The most characters the Percentage box takes: a zoom is three digits and a point, and a longer text is no number the window could use.
const FIELD_CHARS: usize = 12;

/// The zoom a typed text asks for, in percent: a number, with a percent sign or not, inside the window's range. The refusal is the words the dialog says.
pub fn parse_percent(text: &str) -> Result<f64, String> {
    let (lo, hi) = (MIN_ZOOM * 100.0, MAX_ZOOM * 100.0);
    let refuse = || format!("Type a zoom from {lo:.0} to {hi:.0} percent.");
    let number = text.trim().trim_end_matches('%').trim();
    number.parse::<f64>().ok().filter(|p| p.is_finite() && *p >= lo && *p <= hi).ok_or_else(refuse)
}

/// The Zoom dialog while it is open.
pub struct ZoomDialog {
    pub(crate) choice: Choice,
    /// What the Percentage box holds.
    text: String,
    error: Option<String>,
}

impl DiacraftApp {
    /// The name of the choice the Zoom dialog has now (`None`: it is closed). For the window state, tests and agents.
    pub fn zoom_dialog_choice(&self) -> Option<&'static str> {
        self.ui.zoom_dialog.as_ref().map(|d| d.choice.name())
    }

    /// Opens the dialog on Whole page, with the zoom now in the Percentage box; one that is open stays as it is.
    pub(crate) fn open_zoom_dialog(&mut self) {
        if self.ui.zoom_dialog.is_none() {
            let text = format!("{:.0}", self.zoom() * 100.0);
            self.ui.zoom_dialog = Some(ZoomDialog { choice: Choice::WholePage, text, error: None });
        }
    }

    pub(crate) fn close_zoom_dialog(&mut self) {
        self.ui.zoom_dialog = None;
    }

    /// OK: the window's view command for the choice. A typed number outside the range is refused in words and the dialog stays open.
    fn apply_zoom_dialog(&mut self) {
        let Some((choice, text)) = self.ui.zoom_dialog.as_ref().map(|d| (d.choice, d.text.clone())) else { return };
        let result = match choice {
            Choice::Preset(percent) => self.execute("view.zoom", &json!({ "percent": percent })).map(|_| ()).map_err(|e| e.to_string()),
            Choice::PageWidth => self.execute("view.fitWidth", &json!({})).map(|_| ()).map_err(|e| e.to_string()),
            Choice::WholePage => self.execute("view.fitPage", &json!({})).map(|_| ()).map_err(|e| e.to_string()),
            Choice::Percent => match parse_percent(&text) {
                Ok(percent) => self.execute("view.zoom", &json!({ "percent": percent })).map(|_| ()).map_err(|e| e.to_string()),
                Err(words) => Err(words),
            },
        };
        match result {
            Ok(()) => self.close_zoom_dialog(),
            Err(words) => {
                if let Some(dialog) = self.ui.zoom_dialog.as_mut() {
                    dialog.error = Some(words);
                }
            }
        }
    }

    pub(crate) fn zoom_dialog(&mut self, ctx: &Context, t: Tokens) {
        let screen = ctx.content_rect();
        let width = (screen.width() - 60.0).clamp(240.0, 340.0);
        // The list is as tall as its nine choices (a fixed height: a dialog that finds its size as its parts are drawn takes frames to settle in the middle of the
        // window), and in a window too short for that it scrolls.
        let content_h = (screen.height() - 170.0).clamp(110.0, LIST_H);
        // Enter is OK, but not while a box has the keyboard: there Enter ends the typing.
        let typing = ctx.text_edit_focused();
        let Some(mut dialog) = self.ui.zoom_dialog.take() else { return };
        let (mut ok, mut cancel) = (false, false);
        let mut seen: Vec<(String, egui::Rect, bool)> = Vec::new();
        let modal = egui::Modal::new(Id::unique("zoom_dialog")).backdrop_color(t.scrim).frame(Self::card_frame(&t)).show(ctx, |ui| {
            ui.set_width(width);
            ui.label(RichText::new("Zoom").font(theme::font(size::TAB + 1.0)).color(t.text));
            ui.add_space(6.0);
            ui.label(RichText::new("Magnification").font(theme::font(size::LABEL)).color(t.text_dim));
            ui.add_space(2.0);
            ui.style_mut().spacing.scroll = egui::style::ScrollStyle::solid();
            let content = egui::ScrollArea::vertical()
                .id_salt("zoom_dialog_content")
                .min_scrolled_height(content_h)
                .max_height(content_h)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing.y = 2.0;
                    for (choice, name, words) in CHOICES {
                        ui.horizontal(|ui| {
                            let r = ui.radio(dialog.choice == choice, RichText::new(words).font(theme::font(size::LABEL)).color(t.text));
                            seen.push((format!("zoom.choice.{name}"), r.rect, true));
                            if r.clicked() {
                                dialog.choice = choice;
                                dialog.error = None;
                            }
                            if choice == Choice::Percent {
                                percent_box(ui, &mut dialog, &mut seen);
                            }
                        });
                    }
                });
            seen.push(("zoom.content".to_owned(), content.inner_rect, true));
            ui.add_space(6.0);
            if let Some(error) = &dialog.error {
                let said = ui.add(egui::Label::new(RichText::new(error).font(theme::font(size::LABEL)).color(t.notice_text)).wrap());
                seen.push(("zoom.error".to_owned(), said.rect, true));
                ui.add_space(4.0);
            }
            ui.horizontal(|ui| {
                for (label, key, primary) in [("OK", "zoom.ok", true), ("Cancel", "zoom.cancel", false)] {
                    let b = ui.add(egui::Button::new(RichText::new(label).font(theme::font(size::LABEL))).min_size(vec2(84.0, 26.0)));
                    seen.push((key.to_owned(), b.rect, true));
                    if b.clicked() {
                        if primary { ok = true } else { cancel = true }
                    }
                }
            });
        });
        crate::ask::keep_centred(ctx, modal.response.rect);
        for (name, rect, enabled) in seen {
            self.note(name, rect, enabled);
        }
        if !egui::Popup::is_any_open(ctx) {
            if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
                cancel = true;
            }
            if !typing && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter)) {
                ok = true;
            }
        }
        self.ui.zoom_dialog = Some(dialog);
        if cancel {
            self.close_zoom_dialog();
        } else if ok {
            self.apply_zoom_dialog();
        }
    }
}

/// The Percentage box and its percent sign, at the right of the radio button of Percentage: typing in it, or the keyboard coming to it, chooses Percentage.
fn percent_box(ui: &mut Ui, dialog: &mut ZoomDialog, seen: &mut Vec<(String, egui::Rect, bool)>) {
    let edit = ui.add(egui::TextEdit::singleline(&mut dialog.text).desired_width(56.0).char_limit(FIELD_CHARS).font(theme::font(size::LABEL)));
    edit.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "zoom.percent"));
    seen.push(("zoom.percent".to_owned(), edit.rect, true));
    if edit.changed() || edit.gained_focus() {
        dialog.choice = Choice::Percent;
        dialog.error = None;
    }
    ui.label(RichText::new("%").font(theme::font(size::LABEL)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_typed_zoom_is_a_number_in_the_windows_range() {
        assert_eq!(parse_percent("175"), Ok(175.0));
        assert_eq!(parse_percent(" 60 % "), Ok(60.0));
        assert_eq!(parse_percent("10"), Ok(10.0));
        assert_eq!(parse_percent("400"), Ok(400.0));
        assert_eq!(parse_percent("12.5"), Ok(12.5));
        for refused in ["", "abc", "9.99", "400.5", "-20", "NaN", "inf", "%", "1e9", "5 0"] {
            assert_eq!(parse_percent(refused), Err("Type a zoom from 10 to 400 percent.".to_owned()), "{refused:?}");
        }
    }

    #[test]
    fn the_choices_are_the_nine_of_the_specification_in_order_and_each_name_is_its_own() {
        let names: Vec<&str> = CHOICES.iter().map(|(_, name, _)| *name).collect();
        assert_eq!(names, ["400", "200", "150", "100", "75", "50", "page_width", "whole_page", "percent"]);
        for (choice, name, _) in CHOICES {
            assert_eq!(choice.name(), name);
        }
    }
}
