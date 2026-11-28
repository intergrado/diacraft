//! The Alt Text tab of the Page Setup dialog: a one-line **Title** and a several-line
//! **Description** for the page, the texts a screen reader speaks in place of the page.
//!
//! The tab works on the dialog's **draft** like the others: it reads `page.alt_text`'s lookup when the dialog opens and sends nothing until OK, which adds one `page.alt_text` with
//! only the text that was changed to what OK sends, in the dialog's one undo step (the name, the size and the rest go in the same batch). The boxes follow the Alt Text section of Format Shape
//! (`panes/alt_text.rs`): 255 characters at most, typed or pasted, and the count says so; a line break pasted into the Title is a space and the Description keeps its line feeds; a text a
//! formula makes is shown as the formula's text and typing replaces it with the text typed; a text a `GUARD` protects, or a file made longer than the box takes, is shown and takes no typing.
//! A page has no master to inherit from.

use diacraft_engine::doc::Document;
use diacraft_engine::draw::alt_text::{self as engine, Part, Target};
use egui::{RichText, Ui};
use serde_json::{Value, json};

use super::Dialog;
use crate::panes::alt_text::{BOX_CHARS, GUARD_WORDS, first_chars, long_words, normalise};
use crate::theme::{self, Tokens, size};

/// The names of the two boxes in `ui.elements`.
pub(super) const TITLE_BOX: &str = "pagesetup.alttext.title";
pub(super) const DESCRIPTION_BOX: &str = "pagesetup.alttext.description";

/// The rows the Description box shows.
const DESCRIPTION_ROWS: usize = 6;

/// One text of the page: what the box holds, what it held when the dialog opened, and why it takes no typing.
#[derive(Clone, Debug, Default)]
struct Text {
    shown: String,
    was: String,
    read_only: Option<String>,
    /// A formula makes the text: the box shows it.
    formula: bool,
}

impl Text {
    fn of(part: &Part) -> Text {
        let shown = match &part.formula {
            Some(formula) => formula.clone(),
            None => part.text.clone(),
        };
        let long = shown.chars().count() > BOX_CHARS || part.length > BOX_CHARS && part.formula.is_none();
        let read_only = if part.protected {
            Some(GUARD_WORDS.to_owned())
        } else if long {
            Some(long_words(part.length.max(shown.chars().count())))
        } else {
            None
        };
        let shown = first_chars(&shown, BOX_CHARS);
        Text { was: shown.clone(), shown, read_only, formula: part.formula.is_some() }
    }

    /// The text to send, when the person changed it.
    fn changed(&self, one_line: bool) -> Option<String> {
        (self.read_only.is_none() && self.shown != self.was).then(|| normalise(&self.shown, one_line))
    }
}

/// The tab's draft.
#[derive(Clone, Debug, Default)]
pub(super) struct Alt {
    title: Text,
    description: Text,
    /// The Title box takes the keyboard when it is next drawn (Edit Alt Text opened the dialog).
    pub(super) focus_title: bool,
}

impl Alt {
    /// What the page holds now, read when the dialog opens.
    pub(super) fn read(doc: &Document, page: u32) -> Alt {
        let Ok(at) = engine::locate(doc, &Target::Page(page)) else { return Alt::default() };
        let read = engine::read(doc, &at);
        Alt { title: Text::of(&read.title), description: Text::of(&read.description), focus_title: false }
    }

    /// The `page.alt_text` that OK adds: only the texts that were changed. `None` when none was.
    pub(super) fn command(&self, page: u32) -> Option<Value> {
        let (title, description) = (self.title.changed(true), self.description.changed(false));
        if title.is_none() && description.is_none() {
            return None;
        }
        let mut params = serde_json::Map::new();
        params.insert("page".to_owned(), json!(page));
        if let Some(title) = title {
            params.insert("title".to_owned(), json!(title));
        }
        if let Some(description) = description {
            params.insert("description".to_owned(), json!(description));
        }
        Some(Value::Object(params))
    }
}

/// The tab: the two boxes with their counts and what stops them.
pub(super) fn tab(ui: &mut Ui, t: &Tokens, dialog: &mut Dialog, seen: &mut Vec<(String, egui::Rect, bool)>) {
    let small = |ui: &mut Ui, words: &str, colour: egui::Color32| {
        ui.add(egui::Label::new(RichText::new(words).font(theme::font(size::LABEL - 1.0)).color(colour)).wrap());
    };
    small(ui, "What a screen reader says in place of the page. OK writes both texts to the page.", t.text_dim);
    ui.add_space(6.0);
    let alt = &mut dialog.alt;
    let focus = std::mem::take(&mut alt.focus_title);
    for (which, label, access, name, rows) in [
        (0, "Title", "Page alt text title", TITLE_BOX, None),
        (1, "Description", "Page alt text description", DESCRIPTION_BOX, Some(DESCRIPTION_ROWS)),
    ] {
        ui.label(RichText::new(label).font(theme::font(size::LABEL)).color(t.text));
        let text = if which == 0 { &mut alt.title } else { &mut alt.description };
        let enabled = text.read_only.is_none();
        let edit = match rows {
            Some(rows) => egui::TextEdit::multiline(&mut text.shown).desired_rows(rows),
            None => egui::TextEdit::singleline(&mut text.shown),
        };
        let response = ui.add_enabled(
            enabled,
            edit.desired_width(f32::INFINITY).char_limit(BOX_CHARS).interactive(enabled).font(theme::font(size::LABEL)).text_color(t.text),
        );
        let role = if rows.is_some() { egui::Role::MultilineTextInput } else { egui::Role::TextInput };
        response.widget_info(|| egui::WidgetInfo::labeled(role, enabled, access));
        seen.push((name.to_owned(), response.rect, enabled));
        if focus && which == 0 {
            response.request_focus();
        }
        let typed = text.shown.chars().count();
        if enabled {
            let full = typed >= BOX_CHARS;
            let words = if full {
                format!("{typed} of {BOX_CHARS} characters: the box takes no more.")
            } else {
                format!("{typed} of {BOX_CHARS} characters")
            };
            small(ui, &words, if full { t.notice_text } else { t.text_dim });
            if text.formula && text.shown == text.was {
                small(ui, "A formula sets this text. Typing replaces it with the text typed.", t.text_dim);
            }
        } else if let Some(words) = &text.read_only {
            small(ui, words, t.notice_text);
        }
        ui.add_space(6.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(text: &str, length: usize) -> Part {
        Part { text: text.to_owned(), length, from: diacraft_engine::doc::alt_text::Where::Own, formula: None, protected: false }
    }

    #[test]
    fn only_a_text_that_was_changed_is_sent_and_a_page_with_nothing_changed_sends_nothing() {
        let mut alt = Alt { title: Text::of(&part("Plant", 5)), description: Text::of(&part("", 0)), focus_title: false };
        assert!(alt.command(3).is_none());
        alt.description.shown = "Line one\r\nLine two".to_owned();
        assert_eq!(alt.command(3), Some(json!({"page": 3, "description": "Line one\nLine two"})));
        alt.title.shown = "A\nB".to_owned();
        assert_eq!(alt.command(3), Some(json!({"page": 3, "title": "A B", "description": "Line one\nLine two"})));
        alt.title.shown = "Plant".to_owned();
        alt.description.shown.clear();
        assert!(alt.command(3).is_none(), "typed back to what it was");
    }

    #[test]
    fn a_long_or_protected_text_is_shown_and_never_sent() {
        let long = "x".repeat(300);
        let mut text = Text::of(&part(&long, 300));
        assert_eq!((text.shown.chars().count(), text.was.chars().count()), (255, 255));
        assert!(text.read_only.as_deref().is_some_and(|w| w.contains("300") && w.contains("255")));
        text.shown.push('y');
        assert_eq!(text.changed(false), None);
        let guarded = Text::of(&Part { protected: true, ..part("Note", 4) });
        assert!(guarded.read_only.as_deref().is_some_and(|w| w.contains("GUARD")));
        let formula = Text::of(&Part { formula: Some("Prop.Name".to_owned()), ..part("Pump", 4) });
        assert_eq!((formula.shown.as_str(), formula.formula, formula.read_only), ("Prop.Name", true, None));
    }
}
