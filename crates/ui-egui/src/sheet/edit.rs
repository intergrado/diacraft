//! Editing a cell (design 5.8): the edit state, the one-line editor the cell and the formula bar share, and what a commit does.
//!
//! The editor and its keys are adapted from GridCraft's cell editor (`docs/provenance.md`): an edit begun by **typing** (the arrows then commit and move) against one begun by **F2**
//! or a double click (the arrows move the caret), the text the cell and the bar both edit, and **`original`: an edit committed unchanged sends nothing and makes no undo step**.
//! What GridCraft's editor does for a spreadsheet (point mode, array formulas, column completion) is not taken.
//!
//! **The window edits only through the engine's commands.** A commit is one `sheet.set_cell` with the tab's object, the cell's reference, the text and `force: true` (
//! a formula typed in the sheet replaces a guarded one), one undo step. The window shows the command's words when it refuses (in the footer, and for a formula that does not parse
//! the fault is marked from the byte the command's dry run gives), and what the command answered it keeps: `recalculated` gives the cells to mark as changed **until the next commit**
//! (no timer, so no repaint loop), and the footer says how many were worked out again. Each change of the text asks `sheet.evaluate` once, so that the bar shows the value the text
//! works out to before it is committed; nothing is written by it.

use std::sync::Arc;

use diacraft_engine::{EngineError, shown};
use egui::text::LayoutJob;
use egui::{
    Align, Align2, EventFilter, Frame, Galley, Id, Layout, Margin, Rect, Response, Role, Stroke, StrokeKind, TextBuffer, TextFormat, Ui, UiBuilder,
    WidgetInfo,
};
use serde_json::{Value, json};

use super::view::Sel;
use super::{SheetState, place_params};
use crate::app::DiacraftApp;
use crate::theme::Tokens;

/// The most characters the editor takes: the most a write may hold.
pub const MAX_EDIT_CHARS: usize = 65_536;

/// The most bytes a formula may be to be edited here (`sheet.set_cell` refuses more).
pub const MAX_EDIT_BYTES: usize = 65_536;

/// The most cells a commit's answer is read for errors in.
const MAX_ERROR_PROBES: usize = 8;

/// The id of the editor's text field: one at a time, in the cell or in the bar.
pub fn edit_id() -> Id {
    Id::unique("sheet_edit_text")
}

/// The fault in a text that does not parse: a byte range of the text and the words that say what is wrong.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fault {
    pub start: usize,
    pub end: usize,
    pub words: String,
}

/// What the text works out to, as `sheet.evaluate` says it: the value or the error's word, or the fault in words when the text does not parse.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Live {
    pub value: String,
    pub error: bool,
    pub fault: Option<String>,
    /// The work passed the evaluator's budget: the bar shows `...`.
    pub exhausted: bool,
}

/// An edit that is open.
#[derive(Clone, Debug)]
pub struct EditState {
    /// The cell being edited, by what names it.
    pub sel: Sel,
    /// The reference a formula uses for it (`Geometry1.X2`).
    pub reference: String,
    pub text: String,
    /// The text the edit began with: committed unchanged, it sends nothing.
    pub original: String,
    /// Begun by typing: the arrows commit and move. Begun by F2, a double click or a press in the bar: they move the caret.
    pub typing: bool,
    /// Edited in the bar (a press there) and not in the cell.
    pub in_bar: bool,
    /// Edited in the Edit Formula card (Ctrl+E, the Editing group's Edit Formula): the card draws the text, and the cell and the bar show it.
    pub in_card: bool,
    /// What the text was when the card opened over an edit that was already open: Cancel gives it back. `None` when the card began the edit (Cancel then gives the edit up).
    pub card_from: Option<String>,
    /// What the cell holds, from `sheet.cell` (`text`, `length`, ...): a text cell takes typed text as a string.
    pub kind: String,
    pub fault: Option<Fault>,
    /// The command's own words when it refused the text: they stay until the text changes.
    pub refusal: Option<String>,
    pub live: Option<Live>,
    /// The text `live` was asked for.
    pub live_for: Option<String>,
    /// The editor takes the keyboard and puts its caret at the end of the text in the next frame.
    pub fresh: bool,
    /// The widget changed the text: the live value is asked for again.
    pub dirty: bool,
}

impl EditState {
    pub fn new(sel: Sel, reference: String, kind: String, text: String, typing: bool) -> EditState {
        EditState {
            sel,
            reference,
            original: String::new(),
            text,
            typing,
            in_bar: false,
            in_card: false,
            card_from: None,
            kind,
            fault: None,
            refusal: None,
            live: None,
            live_for: None,
            fresh: true,
            dirty: true,
        }
    }

    /// Puts `text` in the editor: the fault and the command's refusal belong to the text that was, so they go.
    pub fn set_text(&mut self, text: String) {
        if text != self.text {
            self.text = text;
            self.text_changed();
        }
    }

    /// The text changed: its fault and refusal go, and its live value is asked for again.
    pub fn text_changed(&mut self) {
        self.fault = None;
        self.refusal = None;
        self.dirty = true;
    }

    /// The edit as the window state gives it.
    pub fn json(&self) -> Value {
        json!({
            "cell": self.reference,
            "section": self.sel.section(),
            "text": self.text,
            "typing": self.typing,
            "inBar": self.in_bar,
            "inCard": self.in_card,
            "kind": self.kind,
            "fault": self.fault.as_ref().map(|f| json!({"start": f.start, "end": f.end, "words": f.words})),
            "refusal": self.refusal,
            "live": self.live.as_ref().map(|l| json!({"value": l.value, "error": l.error, "fault": l.fault, "exhausted": l.exhausted})),
        })
    }
}

/// The range of bytes of `text` to mark for a fault at byte `offset`: from there to the end of the token it is in (a run of letters, digits, `_` and `.`, or the one character),
/// and **the last character when the text ended early** (the offset is at its end). Always inside the text and on character boundaries, never empty for a text that is not empty.
pub fn fault_span(text: &str, offset: usize) -> (usize, usize) {
    if text.is_empty() {
        return (0, 0);
    }
    let mut at = offset.min(text.len());
    while at > 0 && !text.is_char_boundary(at) {
        at -= 1;
    }
    let Some(rest) = text.get(at..).filter(|r| !r.is_empty()) else {
        let start = text.char_indices().next_back().map_or(0, |(i, _)| i);
        return (start, text.len());
    };
    let word = |c: char| c.is_alphanumeric() || c == '_' || c == '.';
    let mut end = at;
    let mut chars = rest.chars();
    if let Some(first) = chars.next() {
        end = end.saturating_add(first.len_utf8());
        if word(first) {
            for c in chars {
                if !word(c) {
                    break;
                }
                end = end.saturating_add(c.len_utf8());
            }
        }
    }
    (at, end.min(text.len()))
}

/// The text of a formula as the editor holds it: a cell that reads `No Formula` is edited from an empty text, so that what is typed over it is a formula and not an appendix to those
/// words.
pub fn edit_text_of(formula: &str) -> String {
    if formula == "No Formula" { String::new() } else { formula.to_owned() }
}

/// What the person is told when a formula is too long to be edited here.
pub const TOO_LONG: &str = "This formula is longer than 65,536 characters: it is kept as it is and cannot be edited here.";

/// How an edit begins.
#[derive(Clone, Debug)]
pub enum Begin {
    /// By typing: this text replaces the formula (`Delete` and `Backspace` begin with an empty one).
    Typing(String),
    /// By F2, a double click or a press in the bar: the formula's own text, the caret at its end.
    Text { in_bar: bool },
}

/// What a commit moves the selection to once it is done.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Then {
    Stay,
    Down,
    Up,
    Left,
    Right,
}

/// The words of a refusal for a person: what the command said, without the command's own name.
pub fn refusal_words(error: &EngineError) -> String {
    match error {
        EngineError::BadParams { msg, .. } | EngineError::Failed { msg, .. } | EngineError::Disabled { reason: msg, .. } => msg.clone(),
        other => other.to_string(),
    }
}

/// The footer's words for what a commit did, from the command's answer: whether it changed, the cell that now reads an error, and how many cells were worked out again.
pub fn commit_words(answer: &Value, on_this_sheet: usize, errors: &[String]) -> String {
    let cell = answer.get("cell").and_then(Value::as_str).unwrap_or("The cell");
    if answer.get("changed").and_then(Value::as_bool) == Some(false) {
        return format!("Nothing changed: {cell} already holds this text.");
    }
    let mut out = String::new();
    for e in errors {
        out.push_str(e);
        out.push_str(". ");
    }
    let cells = answer.get("recalculated").and_then(|r| r.get("cells")).and_then(Value::as_u64).unwrap_or(0);
    let more = answer.get("recalculated").and_then(|r| r.get("more")).and_then(Value::as_bool).unwrap_or(false);
    if cells == 0 {
        out.push_str(&format!("{cell} changed. No other cell was worked out again."));
    } else {
        let noun = if cells == 1 { "cell" } else { "cells" };
        out.push_str(&format!(
            "Worked out again: {cells} {noun} changed, {on_this_sheet} on this sheet{}.",
            if more { " (not all are listed)" } else { "" }
        ));
    }
    if answer.get("worked_out").and_then(Value::as_bool) == Some(false) {
        out.push_str(" DiaCraft cannot work this function out yet: the value is the one saved before.");
    }
    out
}

/// Shows the one-line editor in `rect`: a box over the cell or the bar's formula, the fault underlined in the error colour. → the editor's response.
pub fn show_editor(ui: &mut Ui, rect: Rect, edit: &mut EditState, t: &Tokens, name: &str) -> Response {
    let ctx = ui.ctx().clone();
    let id = edit_id();
    let font = super::cell::font();
    let (ink, error) = (t.text, t.sheet_error);
    ui.painter().rect_filled(rect, 0.0, t.input_fill);
    // The editor takes the keyboard for as long as it is open, unless another text field has it (the Font list's box, a pane's field).
    let other_field = ctx.text_edit_focused() && ctx.memory(|m| m.focused()) != Some(id);
    if edit.fresh {
        let mut state = egui::text_edit::TextEditState::load(&ctx, id).unwrap_or_default();
        let end = edit.text.chars().count();
        state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(end))));
        state.store(&ctx, id);
        edit.fresh = false;
        ctx.memory_mut(|m| m.request_focus(id));
    } else if !other_field && ctx.memory(|m| m.focused()) != Some(id) {
        ctx.memory_mut(|m| m.request_focus(id));
    }
    let fault = edit.fault.as_ref().map(|f| (f.start, f.end));
    let layout_font = font.clone();
    let mut layouter = move |ui: &Ui, buf: &dyn TextBuffer, _wrap: f32| -> Arc<Galley> {
        let text = buf.as_str();
        let mut job = LayoutJob::default();
        let plain = TextFormat::simple(layout_font.clone(), ink);
        let marked = TextFormat { underline: Stroke::new(1.5, error), ..TextFormat::simple(layout_font.clone(), ink) };
        let range = fault.filter(|(a, b)| a < b && text.is_char_boundary(*a) && text.is_char_boundary(*b) && *b <= text.len());
        match range {
            Some((a, b)) => {
                job.append(text.get(..a).unwrap_or(""), 0.0, plain.clone());
                job.append(text.get(a..b).unwrap_or(""), 0.0, marked);
                job.append(text.get(b..).unwrap_or(""), 0.0, plain);
            }
            None => job.append(text, 0.0, plain),
        }
        job.wrap.max_width = f32::INFINITY;
        ui.fonts_mut(|f| f.layout_job(job))
    };
    let mut child = ui.new_child(UiBuilder::new().max_rect(rect).layout(Layout::left_to_right(Align::Center)));
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    let out = egui::TextEdit::singleline(&mut edit.text)
        .id(id)
        .frame(Frame::NONE)
        .margin(Margin::symmetric(super::cell::CELL_PAD as i8, 0))
        .font(font)
        .text_color(ink)
        .desired_width(rect.width())
        .min_size(rect.size())
        .align(Align2::LEFT_CENTER)
        .char_limit(MAX_EDIT_CHARS)
        .layouter(&mut layouter)
        .lock_focus(true)
        .return_key(None)
        .event_filter(EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true })
        .show(&mut child);
    ui.painter().rect_stroke(rect, 0.0, Stroke::new(2.0, t.accent), StrokeKind::Inside);
    let name = name.to_owned();
    out.response.response.widget_info(|| WidgetInfo::labeled(Role::TextInput, true, name.clone()));
    if out.response.response.changed() {
        edit.text_changed();
        ctx.request_repaint();
    }
    out.response.response
}

impl SheetState {
    /// The cells the last commit worked out again on the sheet `place` names (`Place::json`), by reference.
    pub(crate) fn changed_on(&self, sheet: &Value) -> std::collections::HashSet<String> {
        self.changed.iter().filter(|c| &c.sheet == sheet).filter_map(|c| c.cell.clone()).collect()
    }
}

impl DiacraftApp {
    /// Begins an edit of the selected cell of the tab on view, as `how` says. → the words of a refusal when it cannot begin: no cell is selected, the cell has no name a formula uses,
    /// or its formula is too long to edit.
    pub(crate) fn sheet_begin_edit(&mut self, how: Begin) -> Result<(), String> {
        let active = self.ui.sheet.active;
        let reference_of = {
            let doc = self.session.document().ok_or_else(|| "no document open".to_owned())?;
            let tab = self.ui.sheet.tabs.get_mut(active).ok_or_else(|| "no ShapeSheet is open".to_owned())?;
            if tab.edit.is_some() {
                return Ok(());
            }
            let sel = tab.selected.clone().ok_or_else(|| "no cell is selected".to_owned())?;
            let view = tab.view.as_mut().ok_or_else(|| "the sheet is not in the drawing".to_owned())?;
            let (_, cell) = view.cell_of(doc, &sel).ok_or_else(|| "no cell is selected: select one first".to_owned())?;
            let reference =
                cell.reference.clone().ok_or_else(|| "this cell has no name a formula can use, so it cannot be edited here".to_owned())?;
            (sel, reference)
        };
        let (sel, reference) = reference_of;
        // The cell whole (a formula the reading cut is read whole here) and what it holds.
        let place = self.ui.sheet.tabs.get(active).map(|t| t.place.clone()).ok_or_else(|| "no ShapeSheet is open".to_owned())?;
        let mut params = place_params(&place);
        if let Some(map) = params.as_object_mut() {
            map.insert("cell".to_owned(), json!(reference));
        }
        let answer = self.session.execute("sheet.cell", &params).map_err(|e| refusal_words(&e))?;
        let formula = answer.get("formula").and_then(Value::as_str).unwrap_or("").to_owned();
        if formula.len() > MAX_EDIT_BYTES {
            return Err(TOO_LONG.to_owned());
        }
        let kind = answer.get("kind").and_then(Value::as_str).unwrap_or("scalar").to_owned();
        let original = edit_text_of(&formula);
        let (text, typing, in_bar) = match how {
            Begin::Typing(typed) => (typed, true, false),
            Begin::Text { in_bar } => (original.clone(), false, in_bar),
        };
        let mut edit = EditState::new(sel, reference, kind, text, typing);
        edit.original = original;
        edit.in_bar = in_bar;
        if let Some(tab) = self.ui.sheet.tabs.get_mut(active) {
            tab.edit = Some(edit);
            tab.note = None;
            tab.ensure_visible = true;
        }
        self.ui.sheet.focus = true;
        Ok(())
    }

    /// Gives up the edit of the tab on view: nothing is sent.
    pub(crate) fn sheet_cancel_edit(&mut self) {
        let active = self.ui.sheet.active;
        if let Some(tab) = self.ui.sheet.tabs.get_mut(active) {
            tab.edit = None;
        }
    }

    /// Whether the tab on view has an edit open.
    pub(crate) fn sheet_editing(&self) -> bool {
        self.ui.sheet.tabs.get(self.ui.sheet.active).is_some_and(|t| t.edit.is_some())
    }

    /// Asks `sheet.evaluate` for the value of the text in the editor when it has changed since it was asked: a query, so nothing is written.
    pub(crate) fn sheet_edit_live(&mut self) {
        let active = self.ui.sheet.active;
        let Some((place, edit)) = self.ui.sheet.tabs.get(active).and_then(|t| t.edit.as_ref().map(|e| (t.place.clone(), e.clone()))) else { return };
        if !edit.dirty && edit.live_for.as_deref() == Some(edit.text.as_str()) {
            return;
        }
        let live = if edit.text.trim().is_empty() {
            None
        } else if edit.kind == "text" && !edit.text.trim_start().starts_with('=') {
            // A text cell takes what is typed as a string: the text is its own value.
            Some(Live { value: shown(&edit.text, 120), error: false, fault: None, exhausted: false })
        } else {
            let mut params = place_params(&place);
            if let Some(map) = params.as_object_mut() {
                map.insert("formula".to_owned(), json!(edit.text));
                map.insert("cell".to_owned(), json!(edit.reference));
            }
            self.ui.sheet.work.evaluations += 1;
            match self.session.execute("sheet.evaluate", &params) {
                Ok(answer) => {
                    let fault = answer.get("parse_error").and_then(|p| p.get("words")).and_then(Value::as_str).map(str::to_owned);
                    let value = answer.get("shown").and_then(Value::as_str).unwrap_or("").to_owned();
                    let error = answer.get("error").is_some_and(|e| !e.is_null());
                    let exhausted = answer.get("budget_exhausted").and_then(Value::as_bool).unwrap_or(false);
                    Some(Live { value, error, fault, exhausted })
                }
                Err(e) => Some(Live { value: String::new(), error: false, fault: Some(refusal_words(&e)), exhausted: false }),
            }
        };
        if let Some(e) = self.ui.sheet.tabs.get_mut(active).and_then(|t| t.edit.as_mut()) {
            e.live = live;
            e.live_for = Some(e.text.clone());
            e.dirty = false;
        }
    }

    /// Commits the edit of the tab on view as one `sheet.set_cell`, and moves the selection as `then` says. An edit committed unchanged sends nothing. → whether the edit closed: a
    /// refusal keeps it open, with the command's words and, for a formula that does not parse, the fault marked.
    pub(crate) fn sheet_commit_edit(&mut self, then: Then) -> bool {
        let active = self.ui.sheet.active;
        let Some((place, edit)) = self.ui.sheet.tabs.get(active).and_then(|t| t.edit.as_ref().map(|e| (t.place.clone(), e.clone()))) else {
            return true;
        };
        if edit.text == edit.original {
            self.sheet_close_edit(active, then, None);
            return true;
        }
        let mut params = place_params(&place);
        if let Some(map) = params.as_object_mut() {
            map.insert("cell".to_owned(), json!(edit.reference));
            map.insert("formula".to_owned(), json!(edit.text));
            map.insert("force".to_owned(), json!(true));
        }
        match self.execute("sheet.set_cell", &params) {
            Ok(answer) => {
                let words = self.sheet_remember_commit(&place, &answer);
                self.ui.sheet.last = Some(super::Last::Done(answer));
                self.sheet_close_edit(active, then, Some(words));
                true
            }
            Err(error) => {
                let words = refusal_words(&error);
                let fault = self.sheet_fault_of(&place, &edit, &words);
                self.ui.sheet.last = Some(super::Last::Refused(words.clone()));
                if let Some(e) = self.ui.sheet.tabs.get_mut(active).and_then(|t| t.edit.as_mut()) {
                    e.fault = fault;
                    e.refusal = Some(words);
                }
                false
            }
        }
    }

    /// The fault to mark for a text the command refused as no formula: from the byte its dry run gives. `None` for any other refusal.
    fn sheet_fault_of(&mut self, place: &super::Place, edit: &EditState, words: &str) -> Option<Fault> {
        if !words.starts_with("the formula has an error") {
            return None;
        }
        let mut params = place_params(place);
        if let Some(map) = params.as_object_mut() {
            map.insert("formula".to_owned(), json!(edit.text));
            map.insert("cell".to_owned(), json!(edit.reference));
        }
        self.ui.sheet.work.evaluations += 1;
        let answer = self.session.execute("sheet.evaluate", &params).ok()?;
        let parse = answer.get("parse_error").filter(|p| !p.is_null())?;
        let offset = usize::try_from(parse.get("offset").and_then(Value::as_u64)?).ok()?;
        let (start, end) = fault_span(&edit.text, offset);
        Some(Fault { start, end, words: parse.get("words").and_then(Value::as_str).unwrap_or(words).to_owned() })
    }

    /// Keeps what a commit's answer says: the cells it worked out again (marked until the next commit), and the words for the footer, with the cells of this sheet that now read an
    /// error named. → the footer's words.
    fn sheet_remember_commit(&mut self, place: &super::Place, answer: &Value) -> String {
        let sheet_json = place.json();
        let listed: Vec<super::Changed> = answer
            .get("recalculated")
            .and_then(|r| r.get("listed"))
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .map(|item| super::Changed {
                        sheet: item.get("sheet").cloned().unwrap_or(Value::Null),
                        cell: item.get("cell").and_then(Value::as_str).map(str::to_owned),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let here: Vec<String> = listed.iter().filter(|c| c.sheet == sheet_json).filter_map(|c| c.cell.clone()).collect();
        let mut errors: Vec<String> = Vec::new();
        if let (Some(cell), Some(code)) = (answer.get("cell").and_then(Value::as_str), answer.get("error").and_then(Value::as_str)) {
            errors.push(format!("{cell} now reads {code}"));
        }
        for reference in here.iter().take(MAX_ERROR_PROBES) {
            let mut params = place_params(place);
            if let Some(map) = params.as_object_mut() {
                map.insert("cell".to_owned(), json!(reference));
            }
            if let Ok(read) = self.session.execute("sheet.cell", &params)
                && let Some(code) = read.get("error").and_then(Value::as_str)
            {
                errors.push(format!("{reference} now reads {code}"));
            }
        }
        let words = commit_words(answer, here.len(), &errors);
        self.ui.sheet.changed = listed;
        self.ui.sheet.changed_at = Some(self.session.revision());
        words
    }

    /// Closes the edit of tab `i` after a commit (or an unchanged one), moves the selection and leaves the footer's `words` for as long as the selection stays where it is put.
    fn sheet_close_edit(&mut self, i: usize, then: Then, words: Option<String>) {
        if let Some(tab) = self.ui.sheet.tabs.get_mut(i) {
            tab.edit = None;
        }
        if then != Then::Stay {
            self.sheet_move_selection(then);
        }
        if let (Some(words), Some(tab)) = (words, self.ui.sheet.tabs.get_mut(i)) {
            tab.note = Some((tab.selected.clone(), words));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fault_is_marked_from_its_byte_to_the_end_of_its_token_and_the_last_character_when_the_text_ended_early() {
        assert_eq!(fault_span("1/", 2), (1, 2), "the text ended early: the last character");
        assert_eq!(fault_span("1+*2", 2), (2, 3), "a symbol is one character");
        assert_eq!(fault_span("Width*foo.bar+1", 6), (6, 13), "a name runs to its end: letters, digits, `_` and `.`");
        assert_eq!(fault_span("1+abc", 2), (2, 5));
        assert_eq!(fault_span("", 0), (0, 0));
        assert_eq!(fault_span("é+", 3), (2, 3), "the last character, whole: the byte before it is no boundary");
        assert_eq!(fault_span("é+", 1), (0, 2), "an offset inside a character is taken back to its start");
        assert_eq!(fault_span("ab", 99), (1, 2), "an offset past the end is the end");
    }

    #[test]
    fn a_fault_stays_inside_the_text_on_boundaries_and_is_never_empty_for_a_text_that_is_not() {
        for text in ["", "a", "é", "1/", "Width*2", "日本語+", "a.b_c d", "😀+😀"] {
            for offset in 0..text.len() + 3 {
                let (a, b) = fault_span(text, offset);
                assert!(a <= b && b <= text.len(), "{text:?} at {offset}: {a} {b}");
                assert!(text.is_char_boundary(a) && text.is_char_boundary(b), "{text:?} at {offset}: {a} {b}");
                assert!(text.is_empty() || a < b, "{text:?} at {offset}: {a} {b}");
            }
        }
    }

    #[test]
    fn a_cell_that_reads_no_formula_is_edited_from_an_empty_text() {
        assert_eq!(edit_text_of("No Formula"), "");
        assert_eq!(edit_text_of("Width*2"), "Width*2");
        assert_eq!(edit_text_of(""), "");
    }

    #[test]
    fn the_footers_words_say_what_a_commit_did() {
        let answer = json!({"cell": "Width", "changed": true, "recalculated": {"cells": 3, "listed": [], "more": false}, "worked_out": true});
        assert_eq!(commit_words(&answer, 2, &[]), "Worked out again: 3 cells changed, 2 on this sheet.");
        let one = json!({"cell": "Width", "changed": true, "recalculated": {"cells": 1, "listed": [], "more": false}});
        assert_eq!(commit_words(&one, 1, &[]), "Worked out again: 1 cell changed, 1 on this sheet.");
        let none = json!({"cell": "Height", "changed": true, "recalculated": {"cells": 0, "listed": [], "more": false}});
        assert_eq!(commit_words(&none, 0, &[]), "Height changed. No other cell was worked out again.");
        let same = json!({"cell": "Height", "changed": false});
        assert_eq!(commit_words(&same, 0, &[]), "Nothing changed: Height already holds this text.");
        let error = commit_words(&answer, 2, &["Width now reads #DIV/0!".to_owned()]);
        assert!(error.starts_with("Width now reads #DIV/0!. Worked out again"), "{error}");
        let more = json!({"cell": "Width", "changed": true, "recalculated": {"cells": 300, "listed": [], "more": true}});
        assert!(commit_words(&more, 256, &[]).contains("(not all are listed)"));
        let unknown = json!({"cell": "Width", "changed": true, "worked_out": false});
        assert!(commit_words(&unknown, 0, &[]).contains("cannot work this function out yet"));
    }

    #[test]
    fn a_refusal_is_said_without_the_commands_name() {
        let bad = EngineError::BadParams { cmd: "sheet.set_cell".to_owned(), msg: "the cell Width is guarded".to_owned() };
        assert_eq!(refusal_words(&bad), "the cell Width is guarded");
        let failed = EngineError::Failed { cmd: "x".to_owned(), msg: "no document open".to_owned() };
        assert_eq!(refusal_words(&failed), "no document open");
    }

    #[test]
    fn a_change_of_the_text_takes_the_fault_and_the_refusal_away() {
        let mut edit = EditState::new(Sel::Header { section: "A".to_owned() }, "Width".to_owned(), "length".to_owned(), "1/".to_owned(), true);
        edit.dirty = false;
        edit.fault = Some(Fault { start: 1, end: 2, words: "x".to_owned() });
        edit.refusal = Some("words".to_owned());
        edit.set_text("1/".to_owned());
        assert!(edit.fault.is_some() && edit.refusal.is_some() && !edit.dirty, "the same text is no change");
        edit.set_text("1/2".to_owned());
        assert!(edit.fault.is_none() && edit.refusal.is_none() && edit.dirty);
    }
}
