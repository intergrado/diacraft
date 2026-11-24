//! Insert, Text, Symbol: a character picker.
//!
//! **What the specification gives** is little ("Symbol", an Omega button with an arrow, greyed out with nothing selected or not editing text; by its ids a gallery of symbols and a More
//! Symbols dialog; no page or image shows either). So **the contents are ours**: the Symbol menu lists the symbols a drawing's text most often wants, and **More Symbols...** opens a dialog
//! of six sets (Common, Currency, Arrows, Math, Greek, Shapes), a grid of characters with the one chosen shown larger with its code point, and **Insert** and **Close**. A double-click on a
//! character inserts it. **The sets hold only what the bundled faces draw** (the grid is set in the monospace family, Hack with Ubuntu Light behind it, which has the arrows, the math
//! signs and the geometric shapes the proportional face lacks; a test of the sets holds each character to it), so no character is a box in the dialog.
//!
//! **Where it goes**: with a text box open, the characters selected in the box are replaced (the caret is where the symbol goes), and the box opens again after it; with one shape selected and
//! no box, at the end of its text. The engine's `text.insert` makes the edit (one undo step, "Insert text"), and `shape.set_text` with a range when characters are selected. The window
//! commands are `window.symbols {close?}` (the dialog) and `window.symbol {text}` (a symbol at once, as a menu row does).

use diacraft_engine::{EngineError, Result};
use egui::{Context, CornerRadius, Id, Key, Modifiers, RichText, Sense, Stroke, StrokeKind, vec2};
use serde_json::{Value, json};

use crate::app::DiacraftApp;
use crate::insert_links::{heading, list_row};
use crate::theme::{self, Tokens, size};

/// The sets of the dialog: a name, and the characters.
pub const SETS: [(&str, &str); 6] = [
    ("Common", "©®™°±×÷µ¶§†‡•…‰′″¿¡«»‹›“”‘’„–—¼½¾¹²³№"),
    ("Currency", "$¢£¤¥€₹₩₪₫₴₦"),
    ("Arrows", "←↑→↓↔↕⇐⇑⇒⇓⇔↖↗↘↙↩↪↺↻"),
    ("Math", "±×÷−∓∞≈≠≡≤≥∑∏√∫∂∆∇∈∉∩∪⊂⊃⊆⊇∧∨¬∀∃∅π"),
    ("Greek", "ΑΒΓΔΕΖΗΘΙΚΛΜΝΞΟΠΡΣΤΥΦΧΨΩαβγδεζηθικλμνξοπρστυφχψω"),
    ("Shapes", "■□▲△▶▷▼▽◀◁◆◇●○◎"),
];

/// The side of a character's button, in points.
const CELL: f32 = 30.0;

/// The Symbol dialog while it is open.
#[derive(Clone, Debug)]
pub struct SymbolDialog {
    page: u32,
    id: u32,
    /// The characters selected in the text box that was open (`(start, end)`; a caret is `start == end`), which the symbol replaces.
    caret: Option<(usize, usize)>,
    was_editing: bool,
    set: usize,
    selected: Option<char>,
    error: Option<String>,
}

fn bad(cmd: &str, msg: impl Into<String>) -> EngineError {
    EngineError::BadParams { cmd: cmd.to_owned(), msg: msg.into() }
}

impl DiacraftApp {
    /// The dialog's state for the window state and the tests: `null` when it is closed.
    pub(crate) fn symbol_state(&self) -> Value {
        let Some(d) = &self.ui.insert.symbol else { return Value::Null };
        json!({"id": d.id, "page": d.page, "set": SETS.get(d.set).map(|s| s.0), "selected": d.selected.map(String::from), "error": d.error})
    }

    /// Whether Symbol can run now, and if not why: a text box is open, or one shape is selected.
    pub(crate) fn symbol_enabled(&self) -> std::result::Result<(), String> {
        self.command_enabled("text.insert")?;
        if self.ui.edit.editing_text() || self.one_shape_selected().is_some() {
            Ok(())
        } else {
            Err("select one shape, or open its text, to put a symbol in it".to_owned())
        }
    }

    /// Opens the dialog: a text box that is open ends first (what was typed is kept) and its selection is where the symbol goes.
    pub(crate) fn open_symbols(&mut self) -> Result<()> {
        const ID: &str = "window.symbols";
        if self.ui.insert.symbol.is_some() {
            return Ok(());
        }
        let was_editing = self.ui.edit.editing_text();
        let caret = self.edited_text_selection();
        self.commit_pending_text();
        let (page, id) = self.one_shape_selected().ok_or_else(|| bad(ID, "select one shape, or open its text, to put a symbol in it"))?;
        self.ui.insert.symbol = Some(SymbolDialog { page, id, caret, was_editing, set: 0, selected: None, error: None });
        Ok(())
    }

    pub(crate) fn close_symbols(&mut self) {
        self.ui.insert.symbol = None;
    }

    /// Puts `text` in the text of the one shape selected: in place of the characters selected in the box that was open (`caret`), else at the end. One edit; the box opens again after it when one
    /// was open. → the engine's answer.
    fn put_symbol(&mut self, page: u32, id: u32, caret: Option<(usize, usize)>, was_editing: bool, text: &str) -> Result<Value> {
        let answer = match caret {
            Some((start, end)) if start < end => {
                self.execute("shape.set_text", &json!({"id": id, "page": page, "text": text, "start": start, "end": end}))?
            }
            Some((start, _)) => self.execute("text.insert", &json!({"id": id, "page": page, "text": text, "at": start}))?,
            None => self.execute("text.insert", &json!({"id": id, "page": page, "text": text}))?,
        };
        if was_editing {
            let after = caret.map_or_else(|| self.text_chars(page, id).unwrap_or(0), |(start, _)| start + text.chars().count());
            self.reopen_text(page, id, Some((after, after)));
        }
        Ok(answer)
    }

    /// The dialog's Insert: the character chosen goes in, and the dialog closes; a refusal is said in the dialog.
    fn apply_symbol(&mut self) {
        let Some(d) = self.ui.insert.symbol.clone() else { return };
        let Some(c) = d.selected else { return };
        match self.put_symbol(d.page, d.id, d.caret, d.was_editing, &c.to_string()) {
            Ok(_) => self.close_symbols(),
            Err(e) => {
                if let Some(open) = self.ui.insert.symbol.as_mut() {
                    open.error = Some(e.to_string());
                }
            }
        }
    }

    /// The `window.symbol {text}` command: `text` goes in at once, with no dialog, as a row of the Symbol menu does.
    pub(crate) fn command_symbol(&mut self, text: &str) -> Result<Value> {
        const ID: &str = "window.symbol";
        if text.is_empty() {
            return Err(bad(ID, "`text` must hold the characters to put in"));
        }
        let was_editing = self.ui.edit.editing_text();
        let caret = self.edited_text_selection();
        self.commit_pending_text();
        let (page, id) = self.one_shape_selected().ok_or_else(|| bad(ID, "select one shape, or open its text, to put a symbol in it"))?;
        self.put_symbol(page, id, caret, was_editing, text)
    }

    pub(crate) fn symbol_dialog(&mut self, ctx: &Context, t: Tokens) {
        let Some(mut dialog) = self.ui.insert.symbol.take() else { return };
        let screen = ctx.content_rect();
        let width = (screen.width() - 60.0).clamp(300.0, 520.0);
        let (mut insert, mut close) = (false, false);
        let mut seen: Vec<(String, egui::Rect, bool)> = Vec::new();
        let modal = egui::Modal::new(Id::unique("symbol_dialog")).backdrop_color(t.scrim).frame(Self::card_frame(&t)).show(ctx, |ui| {
            ui.set_width(width);
            ui.label(RichText::new("Symbol").font(theme::font(size::TAB + 1.0)).color(t.text));
            ui.add_space(8.0);
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                ui.vertical(|ui| {
                    ui.set_width(110.0);
                    heading(ui, &t, "Set");
                    ui.spacing_mut().item_spacing.y = 2.0;
                    for (at, (name, _)) in SETS.iter().enumerate() {
                        if list_row(ui, &t, name, at == dialog.set, format!("symbol.set.{}", name.to_lowercase()), &mut seen) && at != dialog.set {
                            dialog.set = at;
                            dialog.selected = None;
                            dialog.error = None;
                        }
                    }
                });
                ui.vertical(|ui| {
                    heading(ui, &t, "Characters");
                    let chars: Vec<char> = SETS.get(dialog.set).map(|s| s.1.chars().collect()).unwrap_or_default();
                    let per_row = (((width - 134.0) / (CELL + 3.0)).floor() as usize).max(4);
                    egui::ScrollArea::vertical().id_salt("symbol_grid").max_height(CELL * 5.0 + 16.0).auto_shrink([false, true]).show(ui, |ui| {
                        ui.spacing_mut().item_spacing = vec2(3.0, 3.0);
                        for line in chars.chunks(per_row) {
                            ui.horizontal(|ui| {
                                for &c in line {
                                    let (rect, resp) = ui.allocate_exact_size(vec2(CELL, CELL), Sense::click_and_drag());
                                    let on = dialog.selected == Some(c);
                                    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, on, c.to_string()));
                                    let painter = ui.painter_at(rect);
                                    painter.rect(
                                        rect,
                                        CornerRadius::same(3),
                                        if on {
                                            t.accent_soft
                                        } else if resp.hovered() {
                                            t.hover
                                        } else {
                                            t.input_fill
                                        },
                                        Stroke::new(1.0, if on { t.accent } else { t.input_border }),
                                        StrokeKind::Inside,
                                    );
                                    painter.text(rect.center(), egui::Align2::CENTER_CENTER, c, egui::FontId::monospace(size::LABEL + 3.0), t.text);
                                    seen.push((format!("symbol.char.{:04X}", c as u32), rect, true));
                                    resp.clone().on_hover_text(format!("U+{:04X}", c as u32));
                                    if resp.clicked() {
                                        dialog.selected = Some(c);
                                        dialog.error = None;
                                    }
                                    if resp.double_clicked() {
                                        dialog.selected = Some(c);
                                        insert = true;
                                    }
                                }
                            });
                        }
                    });
                });
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                match dialog.selected {
                    Some(c) => {
                        ui.label(RichText::new(format!("{c}  U+{:04X}", c as u32)).font(egui::FontId::monospace(size::LABEL + 3.0)).color(t.text))
                    }
                    None => ui.label(RichText::new("Choose a character.").font(theme::font(size::LABEL)).color(t.text_disabled)),
                };
            });
            if let Some(error) = &dialog.error {
                ui.add_space(4.0);
                ui.label(RichText::new(error.as_str()).font(theme::font(size::LABEL)).color(t.notice_text));
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                for (label, key, is_insert) in [("Insert", "symbol.insert", true), ("Close", "symbol.close", false)] {
                    let enabled = !is_insert || dialog.selected.is_some();
                    let b =
                        ui.add_enabled(enabled, egui::Button::new(RichText::new(label).font(theme::font(size::LABEL))).min_size(vec2(84.0, 26.0)));
                    seen.push((key.to_owned(), b.rect, enabled));
                    if b.clicked() {
                        if is_insert { insert = true } else { close = true }
                    }
                }
            });
        });
        crate::ask::keep_centred(ctx, modal.response.rect);
        for (name, rect, enabled) in seen {
            self.note(name, rect, enabled);
        }
        if !egui::Popup::is_any_open(ctx) && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            close = true;
        }
        self.ui.insert.symbol = Some(dialog);
        if close {
            self.close_symbols();
        } else if insert {
            self.apply_symbol();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **No character of a set is a box**: the grid is set in the monospace family, and each character is laid out there under the policy that panics for a character no face has (the
    /// default draws a box, which a test would not see).
    #[test]
    fn every_character_of_every_set_is_drawn_by_the_bundled_monospace_faces() {
        let ctx = egui::Context::default();
        ctx.set_fonts(crate::fonts::font_definitions());
        ctx.set_missing_glyph_policy(egui::MissingGlyphPolicy::Panic);
        let mut missing = String::new();
        let mut out = ctx.run_pass(egui::RawInput::default(), |ctx| {
            for (_, chars) in SETS {
                for c in chars.chars() {
                    let id = egui::FontId::monospace(14.0);
                    let drawn = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        ctx.fonts_mut(|f| f.layout_no_wrap(c.to_string(), id, Default::default()));
                    }));
                    if drawn.is_err() {
                        missing.push(c);
                    }
                }
            }
        });
        // The pass made textures for the glyphs it laid out; they have no window to go to.
        out.textures_delta.clear();
        assert_eq!(missing, "", "characters of the sets that the bundled faces cannot draw");
    }

    /// Every set holds characters, no set repeats one, and no character is a control or a format mark: a symbol is something a person can see.
    #[test]
    fn the_sets_hold_visible_characters_once_each() {
        for (name, chars) in SETS {
            assert!(!chars.is_empty(), "{name}");
            let mut seen: Vec<char> = chars.chars().collect();
            let n = seen.len();
            seen.sort_unstable();
            seen.dedup();
            assert_eq!(seen.len(), n, "{name}: a character twice");
            assert!(chars.chars().all(|c| !c.is_control() && !c.is_whitespace()), "{name}");
        }
    }
}
