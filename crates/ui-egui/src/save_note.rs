//! The save note in the window: what a save across the VSDX and OpenDocument formats tells the person about the data the file carries (the native-format design's
//! 5.6, `docs/diacraft-format.md` 7.7).
//!
//! **The words are the engine's.** A save result has a `carried` field (`{classes, document_level, macros, objects, signatures_stale}`, never a warning), and
//! [`diacraft_engine::across::words`] says it: the design's text, exactly, one string for each part that applies (the carried data, the macro project and objects, the signatures),
//! in the design's order. The window keeps no copy of the words and never reads a warning: it reads the field with `Carried::from_json`, asks which format the file written is
//! (the engine's answer for the working file, [`diacraft_engine::file::OpenFile::format`]), and shows what `words` gives. With nothing to say there is no note, and a file written
//! as `.diacraft` has none to say (it holds the whole model).
//!
//! **Where it shows, and why.** The specifications describe one way a reference application treats a save that loses something: the Compatibility Checker, a dialog that comes
//! **before** a save into an earlier version's type, lists what the type cannot hold, and offers Continue or Cancel. It does not fit this note, for
//! three reasons. Nothing is lost: what the type has no place for is kept inside the file and read back, and the note says that other programs may leave it out, which is a warning about
//! later and not a loss now. The engine knows what a save carries only from the save (the result's `carried`; no command asks it beforehand), so a gate before the save would have to
//! write the file to ask. And a command never opens a dialog, so a program that runs the same `file.save_as` must get its answer in the result, not in a question. So the note is **a
//! card over the drawing, which the person reads in full and dismisses, and which never blocks the save**: the file is written, the status bar says so, and the card tells what the file
//! carries, in the room the drawing area has (it scrolls when the window is too short for it). It is no modal, so the window keeps taking keys and the pointer. The one question the window
//! does ask, because the engine refuses the save until it is answered, is the macro project that the type chosen cannot hold (`ask.rs`).
//!
//! **The check after the save**. A save through a model writer is read back by the engine and compared with the drawing, class by class, and the result is the save
//! result's `checked` field. **The card says it after the note's sentences**, in the engine's words (`diacraft_engine::check::words`): a heading, a line for each class that differs with its count, the
//! group "written as values the file format holds", or, for a check that could not finish, "the file was written; DiaCraft could not check it" and why. A card made of the check alone is the card of a
//! `.diacraft` file written from a drawing of another format. The Compatibility Checker before a save (Continue or Cancel) is still 0.3's: this reads the file after.
//!
//! **Once for each save.** A save that carries something sets the note, replacing the last; a save that carries nothing takes the last away; another drawing (a New, an Open) takes it
//! away. A frame draws the card that is there and adds none. The ODF decision-8 note (what a LibreOffice save would drop, once in a session, in the banner: `save.rs`) is the engine's `notes`
//! and stays as it was; the card is drawn under the banner when both are up.

use diacraft_engine::across::{Carried, Target, words};
use diacraft_engine::check::Outcome;
use diacraft_engine::file::FileFormat;
use egui::{Align2, Color32, Context, CornerRadius, FontId, Frame, Id, Margin, Order, Rect, ScrollArea, Sense, Stroke, Ui, vec2};
use serde_json::{Value, json};

use crate::app::DiacraftApp;
use crate::theme::{self, Tokens, size};
use crate::widgets;

/// The most characters of one part the window keeps: the engine's longest note is far below it (its classes are a fixed vocabulary), so this only holds a result that is not the engine's to a limit.
const PART_CHARS: usize = 2000;

/// What the window keeps of a save note while it is on the screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SaveNote {
    /// The file the save wrote, as the window names it.
    pub(crate) name: String,
    /// The parts of the note, the engine's words: the carried data, the macro project and objects, the signatures. Never empty.
    pub(crate) parts: Vec<String>,
    /// Which note of the window's this is (the first is 1): a note that scrolls keeps its place under this, and the next note starts at the top.
    pub(crate) serial: u64,
}

/// The space between two parts, in points.
const GAP: f32 = 6.0;

/// The parts as paragraphs, one under the other, each wrapped to the width the column has.
fn paragraphs(ui: &mut Ui, parts: &[String], t: &Tokens) {
    for (at, part) in parts.iter().enumerate() {
        if at > 0 {
            ui.add_space(GAP);
        }
        let label = ui.add(egui::Label::new(egui::RichText::new(part).font(theme::font(size::TAB)).color(t.text)).wrap());
        label.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, part));
    }
}

/// The height the parts take wrapped to `width`: what each paragraph lays out to, and the gaps. **Measured, so that the card is the height of its words in the frame it is first drawn**: a
/// scroll area in a row takes the row's height to be the room it has (a card of four lines for a note of three parts, which the first screenshot of it showed), and it is used only when the
/// words need more than the drawing area gives.
fn paragraphs_height(ui: &Ui, parts: &[String], font: &FontId, colour: Color32, width: f32) -> f32 {
    let lines: f32 = ui.fonts_mut(|fonts| parts.iter().map(|part| fonts.layout(part.clone(), font.clone(), colour, width).size().y).sum());
    lines + GAP * parts.len().saturating_sub(1) as f32
}

/// The parts of the note for a save that wrote a file of `format`, from the save's result: **the engine's words for its `carried` field** (nothing for a file written as `.diacraft`, for a drawing with no
/// file of a format, for a result with no `carried` field or one that is none, and for a field with nothing in it), and **after them the words of its `checked` field**: what the
/// file read back differs in from the drawing, the values group, or why the check could not finish (`diacraft_engine::check::words`). A save that was not checked (a save into the drawing's own
/// file) has `checked` null and adds nothing. **Empty** when both have nothing to say.
pub(crate) fn parts_of(format: Option<FileFormat>, result: &Value) -> Vec<String> {
    let mut parts = carried_parts(format, result);
    if let Some(outcome) = result.get("checked").and_then(Outcome::from_json) {
        parts.extend(diacraft_engine::check::words(&outcome).iter().map(|part| diacraft_engine::shown(part, PART_CHARS)));
    }
    parts
}

/// The parts of the note for the data the file carries (the engine's words for the `carried` field).
fn carried_parts(format: Option<FileFormat>, result: &Value) -> Vec<String> {
    let target = match format {
        Some(FileFormat::Vsdx) => Target::Vsdx,
        Some(FileFormat::Odg) => Target::Odg,
        Some(FileFormat::Diacraft) | None => return Vec::new(),
    };
    let Some(carried) = result.get("carried").and_then(Carried::from_json) else { return Vec::new() };
    words(target, &carried).iter().map(|part| diacraft_engine::shown(part, PART_CHARS)).collect()
}

impl DiacraftApp {
    /// The parts of the save note on the screen, in the engine's words, or `None` when there is none.
    pub fn save_note(&self) -> Option<&[String]> {
        self.saving.note.as_ref().map(|note| note.parts.as_slice())
    }

    /// Takes the save note away (the person dismissed it).
    pub fn dismiss_save_note(&mut self) {
        self.saving.note = None;
    }

    /// A save worked: the note it leaves is the one for what it carried, which replaces the last note, and is none when it carried nothing.
    pub(crate) fn set_save_note(&mut self, done: &Value) {
        let format = self.session.source().and_then(|file| file.format());
        let parts = parts_of(format, done);
        if parts.is_empty() {
            self.saving.note = None;
            return;
        }
        self.saving.notes_made = self.saving.notes_made.saturating_add(1);
        self.saving.note = Some(SaveNote { name: self.display_name(), parts, serial: self.saving.notes_made });
    }

    /// The window state's word for the note: its parts, or null.
    pub(crate) fn save_note_state(&self) -> Value {
        json!(self.save_note())
    }

    /// Draws the note over the drawing area `area`, its top at `top` (below the banner when there is one): a card with a heading, each part as a paragraph, and a button that dismisses it.
    pub(crate) fn save_note_overlay(&mut self, ctx: &Context, t: Tokens, area: Rect, top: f32) {
        let Some(note) = self.saving.note.clone() else { return };
        let mut dismissed = false;
        // The words wrap inside the room the area has, whatever its width, and the button stays beside them; what does not fit the height scrolls.
        let words_width = (area.width() - 40.0 - 24.0 - 30.0).clamp(120.0, 520.0);
        let room = (area.bottom() - top - 16.0 - 16.0 - 24.0).max(80.0);
        let heading = format!("A note about {}", note.name);
        let shown = egui::Area::new(Id::unique("save_note"))
            .order(Order::Foreground)
            .pivot(Align2::CENTER_TOP)
            .fixed_pos(egui::pos2(area.center().x, top))
            .show(ctx, |ui| {
                // The words that need more than the drawing area has scroll, in a box that height; every other note is as tall as its words.
                let needs = paragraphs_height(ui, &note.parts, &theme::font(size::TAB), t.text, words_width);
                Frame::new()
                    .fill(t.ribbon_fill)
                    .stroke(Stroke::new(1.0, t.notice_text))
                    .corner_radius(CornerRadius::same(6))
                    .inner_margin(Margin::symmetric(12, 8))
                    .show(ui, |ui| {
                        ui.horizontal_top(|ui| {
                            ui.scope(|ui| {
                                ui.set_max_width(words_width);
                                ui.vertical(|ui| {
                                    let label = ui.add(
                                        egui::Label::new(egui::RichText::new(&heading).font(theme::font_strong(size::TAB)).color(t.text)).wrap(),
                                    );
                                    label.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &heading));
                                    ui.add_space(4.0);
                                    if needs > room {
                                        ScrollArea::vertical()
                                            .id_salt(("save_note", note.serial))
                                            .min_scrolled_height(room)
                                            .max_height(room)
                                            .auto_shrink([false, false])
                                            .scroll_bar_visibility(egui::containers::scroll_area::ScrollBarVisibility::AlwaysVisible)
                                            .show(ui, |ui| paragraphs(ui, &note.parts, &t));
                                    } else {
                                        paragraphs(ui, &note.parts, &t);
                                    }
                                });
                            });
                            let (rect, resp) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::click());
                            widgets::name_button(&resp, true, "Dismiss note");
                            widgets::paint_button_bg(ui, rect, &resp, true, false, &t, 4);
                            let c = rect.center();
                            let ink = Stroke::new(1.4, t.text);
                            ui.painter().line_segment([c + vec2(-4.0, -4.0), c + vec2(4.0, 4.0)], ink);
                            ui.painter().line_segment([c + vec2(-4.0, 4.0), c + vec2(4.0, -4.0)], ink);
                            if resp.on_hover_text("Dismiss").clicked() {
                                dismissed = true;
                            }
                            self.note("save-note.dismiss", rect, true);
                        });
                    })
                    .response
                    .rect
            });
        self.note("save-note", shown.inner, true);
        if dismissed {
            self.saving.note = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The classes of the engine's vocabulary (`across::classes::VOCABULARY`, which is not public): the design's twelve, and the nine more that name what they do not.
    const CLASSES: [&str; 21] = [
        "line ends",
        "gradients",
        "hatches",
        "glue directions",
        "layer settings",
        "style names",
        "document properties",
        "settings",
        "macro project",
        "embedded objects",
        "window settings",
        "unknown parts",
        "dash patterns",
        "fill images",
        "page layouts",
        "package details",
        "digital signatures",
        "shape data",
        "page data",
        "style data",
        "document data",
    ];

    fn field(classes: &[&str], macros: bool, objects: bool, signatures: u32) -> Value {
        json!({ "carried": { "classes": classes, "document_level": classes.iter().any(|c| *c != "shape data"), "macros": macros, "objects": objects, "signatures_stale": signatures } })
    }

    /// **Each class is shown** in the words of both formats: the part names the class once in the list, in plain words, and the `.odg`'s sentence of what another program may do is the same
    /// for every class.
    #[test]
    fn each_class_is_named_in_the_words_of_both_formats() {
        for class in CLASSES {
            let result = field(&[class], false, false, 0);
            let vsdx = parts_of(Some(FileFormat::Vsdx), &result);
            assert_eq!(vsdx.len(), 1, "{class}");
            assert_eq!(
                vsdx[0],
                format!(
                    "This drawing has data a VSDX file has no place for ({class}). DiaCraft keeps it inside the file and reads it back, but another program that saves the file may leave it out."
                )
            );
            let odg = parts_of(Some(FileFormat::Odg), &result);
            assert_eq!(odg.len(), 1, "{class}");
            assert_eq!(
                odg[0],
                format!(
                    "This drawing has data an OpenDocument drawing has no place for ({class}). DiaCraft keeps it inside the file and reads it back. Another program that saves the file may keep this data or leave it out, and what that program changes in the drawing wins where DiaCraft can tell."
                )
            );
        }
    }

    /// All the classes at once, in the order the engine gives them, and the two parts that follow.
    #[test]
    fn every_class_together_and_the_other_parts_follow() {
        let result = field(&CLASSES, true, true, 2);
        let parts = parts_of(Some(FileFormat::Vsdx), &result);
        assert_eq!(parts.len(), 3);
        assert!(parts[0].contains(&format!("({})", CLASSES.join(", "))), "{}", parts[0]);
        assert!(parts[1].starts_with("This file carries a macro project and embedded objects that VSDX programs may not show."), "{}", parts[1]);
        assert_eq!(parts[2], "2 digital signature(s) in this file no longer match it.");
        let odg = parts_of(Some(FileFormat::Odg), &result);
        assert!(odg[1].contains("that OpenDocument programs may not show"), "{}", odg[1]);
    }

    /// With nothing to say there is no note, a `.diacraft` has none to say, and a result that is not a save result of the engine's says nothing and does not stop the window.
    #[test]
    fn nothing_to_say_is_no_note() {
        assert!(parts_of(Some(FileFormat::Vsdx), &field(&[], false, false, 0)).is_empty());
        assert!(parts_of(Some(FileFormat::Odg), &field(&[], false, false, 0)).is_empty());
        assert!(parts_of(Some(FileFormat::Diacraft), &field(&["line ends"], true, true, 3)).is_empty(), ".diacraft holds everything");
        assert!(parts_of(None, &field(&["line ends"], true, true, 3)).is_empty(), "a drawing with no file of a format");
        for odd in [
            json!({}),
            json!({"carried": null}),
            json!({"carried": 3}),
            json!({"carried": {"classes": [1]}}),
            json!({"carried": {"classes": []}}),
            json!(null),
        ] {
            assert!(parts_of(Some(FileFormat::Vsdx), &odd).is_empty(), "{odd}");
        }
    }

    /// Only the `carried` field is read: a warning that holds the design's words is not a note (the field is never parsed from warnings).
    #[test]
    fn a_warning_is_not_a_note() {
        let result = json!({
            "warnings": ["This drawing has data a VSDX file has no place for (line ends). DiaCraft keeps it inside the file and reads it back, but another program that saves the file may leave it out."],
            "warning_count": 1,
        });
        assert!(parts_of(Some(FileFormat::Vsdx), &result).is_empty());
    }

    /// **The words of the check follow the note's own**: the engine's words for the `checked` field, for every format a file is written as, `.diacraft` among them; a check that found
    /// nothing, and a save that was not checked (`null`), add nothing; one that could not finish says so in words.
    #[test]
    fn the_words_of_the_check_follow_the_notes_and_say_a_check_that_could_not_finish() {
        let found = json!({ "done": true, "classes": [{ "class": "pictures", "words": "x", "count": 2 }], "as_values": true, "reason": null });
        let mut result = field(&["line ends"], false, false, 0);
        result["checked"] = found;
        let parts = parts_of(Some(FileFormat::Odg), &result);
        assert_eq!(parts.len(), 4, "{parts:?}");
        assert!(parts[0].starts_with("This drawing has data an OpenDocument drawing has no place for"), "{}", parts[0]);
        assert_eq!(parts[2], "pictures: 2 pictures are missing or differ");
        assert!(parts[3].starts_with("Written as values the file format holds:"), "{}", parts[3]);
        // A `.diacraft` file carries nothing, and the check alone makes the card.
        assert_eq!(parts_of(Some(FileFormat::Diacraft), &result).len(), 3, "the heading, the class, the values group");
        // Nothing found, and no check made, are no words.
        let mut clean = field(&[], false, false, 0);
        clean["checked"] = json!({ "done": true, "classes": [], "as_values": false, "reason": null });
        assert!(parts_of(Some(FileFormat::Vsdx), &clean).is_empty());
        clean["checked"] = Value::Null;
        assert!(parts_of(Some(FileFormat::Vsdx), &clean).is_empty());
        // A check that could not finish.
        let mut unchecked = field(&[], false, false, 0);
        unchecked["checked"] = json!({ "done": false, "classes": [], "as_values": false, "reason": "the file does not read back (zip_bad)" });
        assert_eq!(
            parts_of(Some(FileFormat::Vsdx), &unchecked),
            vec!["The file was written; DiaCraft could not check it: the file does not read back (zip_bad)".to_owned()]
        );
        // A field that is not the engine's adds nothing and does not stop the window.
        for odd in
            [json!(3), json!({ "done": true }), json!({ "done": true, "classes": [{ "class": "no such class", "count": 1 }], "as_values": false })]
        {
            let mut result = field(&[], false, false, 0);
            result["checked"] = odd;
            assert!(parts_of(Some(FileFormat::Vsdx), &result).is_empty());
        }
    }

    /// A part is held to a limit, so a result that is not the engine's cannot put a page of text over the drawing.
    #[test]
    fn a_part_is_cut_at_the_limit() {
        let long = "x".repeat(PART_CHARS * 3);
        let parts = parts_of(Some(FileFormat::Vsdx), &field(&[long.as_str()], false, false, 0));
        assert_eq!(parts.len(), 1);
        assert!(parts[0].chars().count() <= PART_CHARS + 8, "{}", parts[0].chars().count());
    }
}
