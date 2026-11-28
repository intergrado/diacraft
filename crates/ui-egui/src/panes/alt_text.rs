//! The Alt Text tab of the Format Shape pane: a **Title** and a **Description** for the shapes selected,
//! the texts a screen reader speaks in place of the picture (`docs/editing-commands.md`, "Alt text and the accessibility check").
//!
//! **What it shows.** The boxes read the document through the engine's own lookup (`diacraft_engine::draw::alt_text`, the one `shape.alt_text` and the check use), once for each revision of
//! the drawing, page and selection. A text a shape **inherits from its master** is shown as ghost text in the empty box, with a line that says where it comes from; typing replaces
//! it in the shape's own row, and **Use master's text** takes the own row away again (the cell's formula is cleared, so the shape inherits again: `sheet.set_cell` with an empty formula).
//! A text a **formula** makes is shown as the formula's text, and typing replaces it with the text typed. With **several shapes** a box shows the text they share, or "(multiple values)"
//! where they differ, and an edit writes all of them (ours: the brief of this task over the greyed boxes; the specification says one shape).
//!
//! **What it does.** A value is sent when the Title box is ended with Enter or either box lets go of the keyboard, and Esc puts the old text back (`fields.rs`): **one `shape.alt_text` for
//! one shape, one `command.batch` of them for several, one undo step**. Both boxes take at most **255 characters**, typed or pasted, and say so; the command's own cap is 32,768 and
//! it is the engine's to refuse. A text a file holds that is longer is shown cut, with its length, in a box that takes no typing (it is saved whole). A pasted line break is a space in the
//! Title and stays in the Description. **Clear Alt Text** empties both texts of every shape.
//!
//! **What stops the boxes, in words:** nothing selected; more shapes than one undo step holds (256, the batch's limit); a shape on a locked layer (the engine's words); a text a
//! `GUARD` protects; a text longer than the box takes. The boxes are then drawn quiet, so a screen reader reads them as disabled, and the text is still shown.

use std::sync::Arc;

use diacraft_engine::doc::alt_text::{AltText, Where};
use diacraft_engine::doc::{Document, Resolver, SheetRef};
use diacraft_engine::draw::alt_text::{self as engine, Located, Part, Read, Target as Sheet};
use diacraft_engine::draw::layers;
use egui::{RichText, Ui};
use serde_json::{Value, json};

use super::fields::{BoxLook, Field, Target, Typed};
use crate::app::DiacraftApp;
use crate::comments_ui::text_button;
use crate::theme::{self, Tokens, size};

/// The names of the two boxes in `ui.elements`.
pub(crate) const TITLE_FIELD: &str = "pane.alttext.title";
pub(crate) const DESCRIPTION_FIELD: &str = "pane.alttext.description";

/// The most characters either box takes (the reference application's Description box takes 255, and the Title box's own limit is not published).
pub(crate) const BOX_CHARS: usize = 255;

/// The most shapes one edit is written to: a batch holds at most 256 commands, and the edit is one undo step.
pub(crate) const MAX_SHAPES: usize = 256;

/// The rows the Description box shows.
const DESCRIPTION_ROWS: usize = 5;

/// One of the two texts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Which {
    Title,
    Description,
}

impl Which {
    /// The word the engine's commands and the words of the section use.
    pub(crate) const fn word(self) -> &'static str {
        match self {
            Which::Title => "title",
            Which::Description => "description",
        }
    }

    /// The `User` row that holds the text.
    const fn row(self) -> &'static str {
        match self {
            Which::Title => diacraft_engine::doc::alt_text::TITLE_ROW,
            Which::Description => diacraft_engine::doc::alt_text::DESCRIPTION_ROW,
        }
    }

    const fn field(self) -> &'static str {
        match self {
            Which::Title => TITLE_FIELD,
            Which::Description => DESCRIPTION_FIELD,
        }
    }

    fn part(self, read: &Read) -> &Part {
        match self {
            Which::Title => &read.title,
            Which::Description => &read.description,
        }
    }

    fn master_text<'a>(self, text: &'a AltText<'a>) -> &'a str {
        match self {
            Which::Title => text.title.text,
            Which::Description => text.description.text,
        }
    }
}

/// One of the two texts as the section shows it for the shapes selected.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Side {
    /// What the box holds while nothing is typed in it: the shapes' own text (cut at [`BOX_CHARS`]), a formula's text, or nothing for a text that is inherited.
    pub shown: String,
    /// Ghost text of the empty box: the master's text the shapes inherit, or "(multiple values)".
    pub hint: Option<String>,
    /// The shapes differ.
    pub mixed: bool,
    /// Why the box takes no typing (this text only), in words.
    pub read_only: Option<String>,
    /// A line under the box: where the text comes from.
    pub note: Option<String>,
    /// The shapes that have a row of their own over a text the master has: Use master's text acts on them.
    pub inheritable: Vec<u32>,
    /// A shape has this text (its own or its master's).
    pub has_text: bool,
    /// A `GUARD` protects the text on a shape.
    pub protected: bool,
}

/// What the section shows for one selection at one revision.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct AltView {
    /// The drawing's generation and revision, the page and the shapes it was read for.
    key: Option<(u64, u64, u32)>,
    ids: Vec<u32>,
    pub shapes: usize,
    pub title: Side,
    pub description: Side,
    /// Words that stop both boxes: nothing is selected, too many shapes, a locked layer.
    pub blocked: Option<String>,
}

impl AltView {
    pub(crate) fn side(&self, which: Which) -> &Side {
        match which {
            Which::Title => &self.title,
            Which::Description => &self.description,
        }
    }

    /// Whether Clear Alt Text works: something to clear, and nothing in the way.
    pub(crate) fn can_clear(&self) -> bool {
        self.blocked.is_none() && (self.title.has_text || self.description.has_text) && !self.title.protected && !self.description.protected
    }

    /// What the window state says of the section.
    pub(crate) fn json(&self) -> Value {
        let side = |s: &Side| json!({"shown": s.shown, "hint": s.hint, "mixed": s.mixed, "readOnly": s.read_only, "note": s.note, "canInherit": !s.inheritable.is_empty()});
        json!({"shapes": self.shapes, "blocked": self.blocked, "title": side(&self.title), "description": side(&self.description), "canClear": self.can_clear()})
    }
}

/// What the section keeps between frames: the last reading and for what, and whether it was drawn in the frame.
#[derive(Default)]
pub struct AltCache {
    view: Option<Arc<AltView>>,
    /// The Alt Text section was drawn in this frame (for the window state).
    shown: bool,
}

impl AltCache {
    /// A new frame: the section has not been drawn in it yet.
    pub(crate) fn next_frame(&mut self) {
        self.shown = false;
    }

    /// The reading the section last drew, while it was on the screen.
    pub(crate) fn view_shown(&self) -> Option<&AltView> {
        self.view.as_deref().filter(|_| self.shown)
    }
}

/// The text with the line breaks a box of this kind keeps: one line is one line (a break is a space), several lines hold line feeds (a carriage return is a line break too).
pub(crate) fn normalise(text: &str, one_line: bool) -> String {
    let unified = text.replace("\r\n", "\n").replace('\r', "\n");
    if one_line { unified.replace('\n', " ") } else { unified }
}

/// The first `most` characters of `text`.
pub(crate) fn first_chars(text: &str, most: usize) -> String {
    text.chars().take(most).collect()
}

/// What a refusal says, without the name of the command or of the batch step that gave it: the words a person reads under a box.
pub(crate) fn alt_words(error: &diacraft_engine::EngineError) -> String {
    let said = crate::shape_data::words(error);
    let said = said.split_once(" failed: ").map_or(said.as_str(), |(_, rest)| rest);
    let said = said.strip_prefix("invalid parameters for `").and_then(|rest| rest.split_once("`: ")).map_or(said, |(_, rest)| rest);
    said.strip_prefix('`').and_then(|rest| rest.split_once("` failed: ")).map_or(said, |(_, rest)| rest).to_owned()
}

/// The words of a text the box cannot hold because it is longer than the box takes.
pub(crate) fn long_words(length: usize) -> String {
    format!(
        "The text is {length} characters, more than the box takes ({BOX_CHARS}): the box shows the first {BOX_CHARS} and cannot change it, and a save keeps all of it. Clear Alt Text empties it."
    )
}

pub(crate) const GUARD_WORDS: &str = "This text is protected by a GUARD formula and cannot be changed here.";

/// What one shape has of one text.
struct Entry {
    id: u32,
    boxed: String,
    ghost: Option<String>,
    length: usize,
    long: bool,
    protected: bool,
    own: bool,
    has_text: bool,
    /// The master's text for the shape, when the shape has a master and it has text.
    master_text: bool,
    /// The text comes from the master; its name.
    from_master: Option<String>,
    formula: bool,
}

fn entry(which: Which, id: u32, read: &Read, master: Option<&(Option<String>, AltTextOwned)>) -> Entry {
    let part = which.part(read);
    let inherited = matches!(part.from, Where::Master(_));
    let boxed = match (&part.formula, inherited) {
        (Some(formula), _) => formula.clone(),
        (None, true) => String::new(),
        (None, false) => part.text.clone(),
    };
    let ghost = (inherited && part.formula.is_none() && !part.text.is_empty()).then(|| part.text.clone());
    let boxed_chars = boxed.chars().count();
    let long = !inherited && (boxed_chars > BOX_CHARS || part.length > BOX_CHARS && part.formula.is_none());
    let master_text = master.is_some_and(|(_, text)| !text.of(which).is_empty());
    Entry {
        id,
        boxed: if boxed_chars > BOX_CHARS { first_chars(&boxed, BOX_CHARS) } else { boxed },
        ghost,
        length: part.length.max(boxed_chars),
        long,
        protected: part.protected,
        own: part.from == Where::Own,
        has_text: !part.text.is_empty(),
        master_text,
        from_master: inherited.then(|| master.and_then(|(name, _)| name.clone()).unwrap_or_default()),
        formula: part.formula.is_some(),
    }
}

/// A master shape's two texts, owned.
struct AltTextOwned {
    title: String,
    description: String,
}

impl AltTextOwned {
    fn of(&self, which: Which) -> &str {
        match which {
            Which::Title => &self.title,
            Which::Description => &self.description,
        }
    }
}

/// The master a shape inherits from: its name, and the texts of the master's shape.
fn master_of(doc: &Document, res: &Resolver<'_>, at: &Located) -> Option<(Option<String>, AltTextOwned)> {
    let SheetRef::Shape(shape) = &at.sheet else { return None };
    let step = res.master_step(shape).ok()??;
    let found = diacraft_engine::doc::alt_text::read(res, &SheetRef::Shape(step.shape.clone()));
    let name = doc.master(step.master).and_then(|m| m.name.as_deref()).map(|n| diacraft_engine::shown(n, 40));
    Some((name, AltTextOwned { title: Which::Title.master_text(&found).to_owned(), description: Which::Description.master_text(&found).to_owned() }))
}

fn side_of(entries: &[Entry], one_shape: bool) -> Side {
    let Some(first) = entries.first() else { return Side::default() };
    let same = entries.iter().all(|e| e.boxed == first.boxed && e.ghost == first.ghost);
    let protected = entries.iter().any(|e| e.protected);
    let long = entries.iter().find(|e| e.long);
    let read_only = if protected { Some(GUARD_WORDS.to_owned()) } else { long.map(|e| long_words(e.length)) };
    let inheritable: Vec<u32> = entries.iter().filter(|e| e.own && e.master_text && !e.protected).map(|e| e.id).collect();
    let note = match (&first.from_master, first.formula) {
        (_, _) if !same || read_only.is_some() => None,
        // A master that has no text for it gives nothing to say.
        (Some(_), _) if first.ghost.is_none() => None,
        (Some(name), _) if one_shape && name.is_empty() => Some("Inherited from the master. Type to replace it.".to_owned()),
        (Some(name), _) if one_shape => Some(format!("Inherited from the master \"{name}\". Type to replace it.")),
        (Some(_), _) => Some("Inherited from the master. Type to replace it.".to_owned()),
        (None, true) if one_shape => Some("A formula sets this text. Typing replaces it with the text typed.".to_owned()),
        _ => None,
    };
    Side {
        shown: if same { first.boxed.clone() } else { String::new() },
        hint: if same { first.ghost.clone() } else { Some("(multiple values)".to_owned()) },
        mixed: !same,
        read_only,
        note,
        inheritable,
        has_text: entries.iter().any(|e| e.has_text),
        protected,
    }
}

/// The section's reading of the shapes `ids` of page `page`.
fn read_view(doc: &Document, page: u32, ids: &[u32]) -> AltView {
    let mut view = AltView { shapes: ids.len(), ..AltView::default() };
    if ids.len() > MAX_SHAPES {
        view.blocked = Some(format!(
            "{} shapes are selected: alt text is written to at most {MAX_SHAPES} shapes in one undo step, so select fewer shapes.",
            ids.len()
        ));
        return view;
    }
    match layers::first_locked(doc, page, ids) {
        Ok(Some((id, layer))) => view.blocked = Some(layers::locked_words(id, &layer)),
        Ok(None) => {}
        Err(problem) => view.blocked = Some(problem.message),
    }
    let res = Resolver::new(doc);
    let mut titles = Vec::with_capacity(ids.len());
    let mut descriptions = Vec::with_capacity(ids.len());
    for id in ids {
        let Ok(at) = engine::locate(doc, &Sheet::Shape { page, id: *id }) else { continue };
        let read = engine::read(doc, &at);
        let master = master_of(doc, &res, &at);
        titles.push(entry(Which::Title, *id, &read, master.as_ref()));
        descriptions.push(entry(Which::Description, *id, &read, master.as_ref()));
    }
    view.title = side_of(&titles, ids.len() == 1);
    view.description = side_of(&descriptions, ids.len() == 1);
    view
}

impl DiacraftApp {
    /// The reading of the shapes `ids` of page `page`, made again only when the drawing, the page or the selection is not the last one's.
    fn alt_view(&mut self, page: u32, ids: &[u32]) -> Arc<AltView> {
        let key = (self.ui.generation, self.session.revision(), page);
        let fresh = self.ui.panes.alt.view.as_ref().filter(|v| v.key == Some(key) && v.ids == ids).cloned();
        if let Some(view) = fresh {
            return view;
        }
        let mut view = self.session.document().map(|doc| read_view(doc, page, ids)).unwrap_or_default();
        view.key = Some(key);
        view.ids = ids.to_vec();
        let view = Arc::new(view);
        self.ui.panes.alt.view = Some(Arc::clone(&view));
        view
    }

    /// `window.altText`: **Edit Alt Text** (the shape's menu, a finding's Add Alt Text). For a shape (`of` `shape`, the default): the shape `id` on its page, or the shapes selected when no `id`
    /// is given, are shown and selected and the Format Shape pane opens at its Alt Text section with the Title box ready for typing. For a page (`of` `page`): Page Setup opens on its Alt Text tab.
    pub(crate) fn alt_text_command(&mut self, of: Option<&str>, id: Option<u32>, page: Option<u32>) -> diacraft_engine::Result<()> {
        const ID: &str = "window.altText";
        let bad = |msg: String| diacraft_engine::EngineError::BadParams { cmd: ID.to_owned(), msg };
        match of.unwrap_or("shape") {
            "page" => {
                self.open_page_setup(crate::page_setup::DialogTab::AltText, page)?;
                self.page_setup_focus_alt_title();
                Ok(())
            }
            "shape" => {
                match id {
                    Some(id) => {
                        let doc = self.session.document().ok_or_else(|| bad("no document open".to_owned()))?;
                        let holder = diacraft_engine::draw::page_holding(doc, page, &[id]);
                        let found = holder.filter(|p| doc.find_shape(diacraft_engine::doc::Owner::Page(*p), id).is_some());
                        let page = found.ok_or_else(|| bad(format!("no shape has id {id}")))?;
                        if self.current_page() != Some(page) {
                            self.execute("page.select", &json!({"id": page}))?;
                        }
                        self.select(&[id]);
                    }
                    None => {
                        self.selection_target(ID)?;
                    }
                }
                self.ui.panes.format.tab = super::format_shape::FormatTab::AltText;
                self.pane_command("format_shape", Some(true))?;
                self.ui.panes.raise(super::Pane::FormatShape);
                self.ui.panes.fields.focus_next = Some(TITLE_FIELD.to_owned());
                Ok(())
            }
            other => Err(bad(format!("`of` is `shape` or `page`, not `{}`", diacraft_engine::shown(other, 32)))),
        }
    }

    /// Whether Edit Alt Text can run with the parameters `args` (a control's, as JSON text): a shape needs a shape selected, a page needs a drawing.
    pub(crate) fn alt_text_verdict(&self, args: &str) -> Result<(), String> {
        self.session.document().ok_or_else(|| "no document open".to_owned())?;
        let of_page = serde_json::from_str::<Value>(args).ok().is_some_and(|v| v.get("of").and_then(Value::as_str) == Some("page"));
        if of_page || !self.ui.edit.selection().is_empty() { Ok(()) } else { Err("nothing is selected".to_owned()) }
    }

    /// The section's reading for the window state: `null` unless the Alt Text tab was on the screen in the last frame.
    pub(crate) fn alt_text_state(&self) -> Value {
        self.ui.panes.alt.view_shown().map_or(Value::Null, AltView::json)
    }

    /// Forgets the reading: another drawing replaced the open one.
    pub(super) fn forget_alt_pane(&mut self) {
        self.ui.panes.alt = AltCache::default();
    }

    /// The tab's body: the heading, the two boxes and Clear Alt Text.
    pub(super) fn alt_text_section(&mut self, ui: &mut Ui, t: &Tokens) {
        self.ui.panes.alt.shown = true;
        let selection = self.selection_here();
        let view = match &selection {
            Some((page, ids)) => self.alt_view(*page, ids),
            None => {
                let view = Arc::new(AltView { blocked: Some("Select a shape to edit its alt text.".to_owned()), ..AltView::default() });
                self.ui.panes.alt.view = Some(Arc::clone(&view));
                view
            }
        };
        let (page, ids) = selection.unwrap_or((0, Vec::new()));
        ui.label(RichText::new("Alt Text").font(theme::font(size::TAB)).color(t.text));
        ui.add_space(2.0);
        note_line(ui, "What a screen reader says in place of the shape: a short title first, then a longer description.", t.text_dim);
        if let Some(words) = &view.blocked {
            let colour = if ids.is_empty() { t.text_dim } else { t.notice_text };
            note_line(ui, words, colour);
        }
        ui.add_space(6.0);
        self.alt_box(ui, t, Which::Title, &view, page, &ids);
        ui.add_space(8.0);
        self.alt_box(ui, t, Which::Description, &view, page, &ids);
        ui.add_space(8.0);
        let can_clear = view.can_clear();
        let clear = text_button(ui, t, "Clear Alt Text", can_clear, "pane.alttext.clear");
        self.note("pane.alttext.clear", clear.rect, can_clear);
        if clear.clicked() && can_clear {
            self.alt_write(&ids, page, &json!({"clear": true}), "Clear alt text", TITLE_FIELD);
        }
    }

    /// One of the boxes: its label, the box, how many characters it holds, and what the engine and the shapes say about it.
    fn alt_box(&mut self, ui: &mut Ui, t: &Tokens, which: Which, view: &AltView, page: u32, ids: &[u32]) {
        let side = view.side(which);
        let (label, access, rows) = match which {
            Which::Title => ("Title", "Alt text title", None),
            Which::Description => ("Description", "Alt text description", Some(DESCRIPTION_ROWS)),
        };
        let name = which.field();
        ui.label(RichText::new(label).font(theme::font(size::LABEL)).color(t.text));
        let quiet = view.blocked.as_deref().or(side.read_only.as_deref());
        let enabled = quiet.is_none();
        let field = Field { label: access, name, shown: Some(side.shown.clone()), enabled, why_not: quiet, target: Some(Target { page, ids }) };
        self.pane_box(ui, t, field, None, BoxLook { rows, hint: side.hint.as_deref(), limit: BOX_CHARS, stacked: true });
        if enabled {
            let typed = self.pane_field_text(name).map_or(0, |text| text.chars().count());
            let words = if typed >= BOX_CHARS {
                format!("{typed} of {BOX_CHARS} characters: the box takes no more.")
            } else {
                format!("{typed} of {BOX_CHARS} characters")
            };
            note_line(ui, &words, if typed >= BOX_CHARS { t.notice_text } else { t.text_dim });
        } else if view.blocked.is_none()
            && let Some(words) = side.read_only.as_deref()
        {
            note_line(ui, words, t.notice_text);
        }
        if let Some(note) = &side.note {
            note_line(ui, note, t.text_dim);
        }
        if !side.inheritable.is_empty() && view.blocked.is_none() {
            let words = format!("Use master's {}", which.word());
            let button = text_button(ui, t, &words, true, "");
            self.note(format!("{name}.inherit"), button.rect, true);
            if button.clicked() {
                self.alt_inherit(which, page, &side.inheritable);
            }
        }
    }

    /// A value typed in one of the boxes, sent at the top of a frame (`fields.rs`): `shape.alt_text` for the shape the box showed, a batch of them for several, one undo step.
    pub(super) fn send_alt_typed(&mut self, typed: Typed) {
        let Typed { name, text, page, ids } = typed;
        let which = if name == TITLE_FIELD { Which::Title } else { Which::Description };
        let text = normalise(&text, which == Which::Title);
        self.alt_write(&ids, page, &json!({ which.word(): text }), "Set alt text", which.field());
    }

    /// Writes `params` (`title`, `description` or `clear`) to the shapes `ids` of page `page`: one command for one shape, a batch for several, **one undo step**. A refusal is said under the
    /// box `field` and in the status bar, in the engine's words.
    fn alt_write(&mut self, ids: &[u32], page: u32, params: &Value, label: &str, field: &str) {
        let steps: Vec<(&str, Value)> = ids
            .iter()
            .map(|id| {
                let mut p = params.clone();
                if let Some(map) = p.as_object_mut() {
                    map.insert("id".to_owned(), json!(id));
                    map.insert("page".to_owned(), json!(page));
                }
                ("shape.alt_text", p)
            })
            .collect();
        self.alt_run(steps, label, field, page, ids);
    }

    /// Use master's text: the own row of each shape stops holding a text of its own (the cell's formula is cleared, and the cell inherits again), one undo step.
    fn alt_inherit(&mut self, which: Which, page: u32, ids: &[u32]) {
        let cell = format!("User.{}.Value", which.row());
        let steps: Vec<(&str, Value)> =
            ids.iter().map(|id| ("sheet.set_cell", json!({"id": id, "page": page, "cell": cell, "formula": ""}))).collect();
        self.alt_run(steps, "Use master's alt text", which.field(), page, ids);
    }

    fn alt_run(&mut self, steps: Vec<(&str, Value)>, label: &str, field: &str, page: u32, ids: &[u32]) {
        let result = match steps.as_slice() {
            [] => return,
            _ if steps.len() > MAX_SHAPES => {
                Err(format!("{} shapes are more than one undo step takes ({MAX_SHAPES}): select fewer shapes.", steps.len()))
            }
            [(command, params)] => self.execute(command, params).map(|_| ()).map_err(|e| alt_words(&e)),
            many => {
                let commands: Vec<Value> = many.iter().map(|(command, params)| json!({"command": command, "params": params})).collect();
                self.execute("command.batch", &json!({"commands": commands, "label": label})).map(|_| ()).map_err(|e| alt_words(&e))
            }
        };
        match result {
            Ok(()) => self.field_accepted(),
            Err(words) => self.field_refused(field, words, page, ids),
        }
    }
}

/// A small line of words under a box: wrapped, dim or in the notice colour.
fn note_line(ui: &mut Ui, words: &str, colour: egui::Color32) {
    ui.add(egui::Label::new(RichText::new(words).font(theme::font(size::LABEL - 1.0)).color(colour)).wrap());
}

#[cfg(test)]
mod tests {
    use super::*;
    use diacraft_engine::EngineError;

    #[test]
    fn a_title_is_one_line_and_a_description_keeps_its_line_feeds() {
        assert_eq!(normalise("Pump\nP-101", true), "Pump P-101");
        assert_eq!(normalise("Pump\r\nP-101\rx", true), "Pump P-101 x");
        assert_eq!(normalise("a\r\nb\rc\nd", false), "a\nb\nc\nd");
        assert_eq!(normalise("  spaced  ", true), "  spaced  ", "no trimming");
        assert_eq!(normalise("", false), "");
    }

    #[test]
    fn the_first_characters_are_characters_and_not_bytes() {
        assert_eq!(first_chars("\u{e9}\u{e9}\u{e9}", 2), "\u{e9}\u{e9}");
        assert_eq!(first_chars("e\u{301}x", 2), "e\u{301}", "a combining mark is a character of its own, as the engine counts");
        assert_eq!(first_chars("abc", 10), "abc");
        assert_eq!(first_chars("abc", 0), "");
    }

    #[test]
    fn the_words_of_a_refusal_have_no_command_or_batch_step_in_front() {
        let bad = EngineError::BadParams { cmd: "shape.alt_text".to_owned(), msg: "the title must be at most 32768 characters".to_owned() };
        assert_eq!(alt_words(&bad), "the title must be at most 32768 characters");
        let batch = EngineError::Failed {
            cmd: "command.batch".to_owned(),
            msg: "batch step 2 (`shape.alt_text`) failed: invalid parameters for `shape.alt_text`: shape 4 is on layer 1 (\"Held\"), which is locked"
                .to_owned(),
        };
        assert_eq!(alt_words(&batch), "shape 4 is on layer 1 (\"Held\"), which is locked");
        let failed =
            EngineError::Failed { cmd: "shape.alt_text".to_owned(), msg: "the title is protected by GUARD and cannot be changed here".to_owned() };
        assert_eq!(alt_words(&failed), "the title is protected by GUARD and cannot be changed here");
    }

    fn entry_of(boxed: &str, ghost: Option<&str>) -> Entry {
        Entry {
            id: 1,
            boxed: boxed.to_owned(),
            ghost: ghost.map(str::to_owned),
            length: boxed.chars().count(),
            long: false,
            protected: false,
            own: !boxed.is_empty(),
            has_text: !boxed.is_empty() || ghost.is_some(),
            master_text: false,
            from_master: ghost.map(|_| String::new()),
            formula: false,
        }
    }

    #[test]
    fn shapes_that_agree_show_their_text_and_shapes_that_differ_say_so() {
        let one = side_of(&[entry_of("Pump", None)], true);
        assert_eq!((one.shown.as_str(), one.mixed, one.hint.as_deref()), ("Pump", false, None));
        let agree = side_of(&[entry_of("Same", None), entry_of("Same", None)], false);
        assert_eq!((agree.shown.as_str(), agree.mixed), ("Same", false));
        let differ = side_of(&[entry_of("A", None), entry_of("B", None)], false);
        assert_eq!((differ.shown.as_str(), differ.mixed, differ.hint.as_deref()), ("", true, Some("(multiple values)")));
        // An own text and an inherited one that read alike differ: one is typed in the box, the other is ghost text.
        let own_and_inherited = side_of(&[entry_of("Valve", None), entry_of("", Some("Valve"))], false);
        assert!(own_and_inherited.mixed);
        let inherited = side_of(&[entry_of("", Some("Valve"))], true);
        assert_eq!((inherited.shown.as_str(), inherited.hint.as_deref()), ("", Some("Valve")));
        assert!(inherited.note.as_deref().is_some_and(|n| n.contains("master")));
        assert_eq!(side_of(&[], false), Side::default());
    }

    #[test]
    fn a_protected_or_a_long_text_takes_no_typing_and_says_why() {
        let mut guarded = entry_of("Long note", None);
        guarded.protected = true;
        let side = side_of(&[guarded], true);
        assert!(side.read_only.as_deref().is_some_and(|w| w.contains("GUARD") && w.contains("protected")) && side.protected);
        let mut long = entry_of("x", None);
        long.long = true;
        long.length = 300;
        let side = side_of(&[long], true);
        let words = side.read_only.unwrap();
        assert!(words.contains("300") && words.contains("255"), "{words}");
    }
}
