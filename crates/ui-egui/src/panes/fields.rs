//! The number and text fields of the panes: what is shown while a field is not being typed in, what is typed, and when a value is let go of.
//!
//! **A field sends its value once**, when the person presses Enter or leaves it with the pointer or Tab ("typing a value sends one
//! command on Enter or leaving the field"), and only if something was typed in it and the text differs from what the field shows. Esc leaves the old value
//! in it. A field the selection does not agree on (several shapes) shows nothing; one that cannot be changed is drawn quiet and takes no keys.
//!
//! **The value goes to the shapes the field was showing, not to what is selected when it is let go of.** A press on the page changes the selection before
//! the field lets go of the focus, and a press on another field takes the focus before the first one lets go. So each field keeps its own [`Edit`]: the
//! page and the shape ids it showed when it took the focus, the text, and whether anything was typed. **The text is copied from the box whenever it changes,
//! focused or not**: the keys of a frame that ends in Enter are all in it.
//!
//! **An edit is finished at the top of the frame, before the title bar, the ribbon or the File view act** ([`DiacraftApp::flush_edits`], called by `ui`):
//! when its box has lost the focus, and when a click lands anywhere but in the box (the click ends the edit, now, so that a Save, an Undo or an Open that
//! the same click makes finds the value in the drawing). It sends to the shapes it recorded, or sends nothing when nothing was typed (**a box that was only
//! passed through, by Tab or a click, never sends**: a value that is not the person's must not undo what the ribbon, Undo or a drag on the page changed
//! meanwhile). A drawing that replaces the open one drops the edits that wait ([`DiacraftApp::forget_edits`]). **A box that nothing was typed in follows the
//! drawing**: it shows the new value whenever the drawing changes, with the whole text selected again, and takes the shapes now selected for its target.

use egui::text::{CCursor, CCursorRange};
use egui::{Id, Key, Rect, Ui, vec2};

use crate::app::DiacraftApp;
use crate::measure::MAX_CHARS;
use crate::theme::{self, Tokens, size};

/// The shapes a field shows: what a value typed in it is sent to.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Target<'a> {
    pub page: u32,
    pub ids: &'a [u32],
}

/// A field that has the focus, or has just lost it and has not yet been finished.
#[derive(Clone, Debug)]
pub(crate) struct Edit {
    /// The name of the field (`pane.size.x`).
    pub name: String,
    id: Id,
    /// Where the box was when it was last drawn: a click outside it ends the edit.
    rect: Rect,
    /// What the box holds: typed text, or what the field shows while nothing is typed.
    text: String,
    /// What the field shows now (as long as it shows the shapes the edit is for): typed text equal to it is not sent.
    current: String,
    /// Something was typed in the box: only then is anything sent.
    typed: bool,
    /// The box was seen without the focus by the pane: it was let go of in an earlier frame, not as this one began.
    left: bool,
    /// The shapes it showed, and their page.
    pub page: u32,
    pub ids: Vec<u32>,
}

/// A value to be sent: the text, and the shapes it is for.
pub(crate) struct Typed {
    pub name: String,
    pub text: String,
    pub page: u32,
    pub ids: Vec<u32>,
}

/// Why a typed value was refused, and which field and which shapes it was for.
#[derive(Clone, Debug)]
pub struct Refusal {
    pub field: String,
    pub words: String,
    page: u32,
    ids: Vec<u32>,
}

/// The fields being typed in, and the words about the last value that was refused.
#[derive(Default)]
pub struct FieldState {
    edits: Vec<Edit>,
    /// Why the last value was refused.
    pub error: Option<Refusal>,
    /// An Escape ended an edit in this frame: it was the box's, and the drawing's Escape (which clears the selection) stands aside.
    took_escape: bool,
    /// What each field showed in this frame so far, and in the last: `pane_field_text` answers from the last, as `elements` does.
    now: Vec<(String, String)>,
    last: Vec<(String, String)>,
    /// The field that takes the keyboard when it is next drawn (a variable list row of the Shape Data window turned into a text box).
    pub(crate) focus_next: Option<String>,
}

impl FieldState {
    /// A new frame: what the last one drew is what can be asked about.
    pub fn next_frame(&mut self) {
        self.last = std::mem::take(&mut self.now);
    }

    /// How many fields hold an edit that is not finished. For tests.
    pub fn open_edits(&self) -> usize {
        self.edits.len()
    }
}

/// The width of the labels in front of the boxes of a pane, in points.
pub const LABEL_W: f32 = 80.0;

/// A label in a column of the width every label of a pane has, so that the boxes, swatches and drop-downs of a pane line up.
pub(crate) fn label_column(ui: &mut Ui, t: &Tokens, label: &str, enabled: bool) {
    ui.allocate_ui_with_layout(vec2(LABEL_W, 22.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.set_min_size(vec2(LABEL_W, 22.0));
        ui.label(egui::RichText::new(label).font(theme::font(size::LABEL)).color(if enabled { t.text } else { t.text_disabled }));
    });
}

/// What a field shows and whether it works.
pub struct Field<'a> {
    /// The label in front of it, and its name in `ui.elements` (`pane.size.x`).
    pub label: &'a str,
    pub name: &'a str,
    /// What it shows while it is not being typed in: `None` is blank (the shapes differ).
    pub shown: Option<String>,
    pub enabled: bool,
    /// Why it cannot be changed, for the hover text of a quiet field.
    pub why_not: Option<&'a str>,
    /// The shapes it shows: what a value typed in it is for.
    pub target: Option<Target<'a>>,
}

/// Selects the whole of the text of the box `id`, which holds `chars` characters.
fn select_all(ctx: &egui::Context, id: Id, chars: usize) {
    let mut state = egui::text_edit::TextEditState::load(ctx, id).unwrap_or_default();
    state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(0), CCursor::new(chars))));
    state.store(ctx, id);
}

impl DiacraftApp {
    /// Draws a labelled field and keeps what is typed in it. The value is sent by [`DiacraftApp::flush_edits`], at the top of a later frame.
    pub(crate) fn pane_field(&mut self, ui: &mut Ui, t: &Tokens, field: Field<'_>) {
        self.pane_field_tip(ui, t, field, None);
    }

    /// [`DiacraftApp::pane_field`] with a tip for the box (the prompt of a Shape Data field): the text shown while the pointer rests on it.
    pub(crate) fn pane_field_tip(&mut self, ui: &mut Ui, t: &Tokens, field: Field<'_>, tip: Option<&str>) {
        let Field { label, name, shown, enabled, why_not, target } = field;
        let shown_text = shown.unwrap_or_default();
        let id = Id::unique(("pane_field", name));
        // Read before the box is added: the box takes the key for itself when it has the focus.
        let escape = ui.input(|i| i.key_pressed(Key::Escape));
        let held = self.ui.panes.fields.edits.iter().find(|e| e.name == name && e.typed).map(|e| e.text.clone());
        // A box nothing was typed in follows the drawing.
        let mut buffer = held.unwrap_or_else(|| shown_text.clone());
        ui.horizontal(|ui| {
            label_column(ui, t, label, enabled);
            let width = (ui.available_width() - 6.0).clamp(60.0, 170.0);
            let out = ui.add_enabled(
                enabled,
                egui::TextEdit::singleline(&mut buffer)
                    .id(id)
                    .desired_width(width)
                    // A box that cannot be changed takes no keys, even while it still holds the focus from a frame in which it could (egui's `add_enabled` greys
                    // a box and leaves its keys on: a value typed in the frame the selection stopped being the page on view's reached the shapes it showed).
                    .interactive(enabled)
                    .char_limit(MAX_CHARS)
                    .font(theme::font(size::LABEL))
                    .text_color(t.text),
            );
            out.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, enabled, label));
            self.note(name.to_owned(), out.rect, enabled);
            if self.ui.panes.fields.now.len() < 256 {
                self.ui.panes.fields.now.push((name.to_owned(), buffer.clone()));
            }
            let out = match why_not.filter(|_| !enabled) {
                Some(why) => out.on_disabled_hover_text(why),
                None => out,
            };
            let out = match tip.filter(|_| enabled) {
                Some(tip) => out.on_hover_text(tip),
                None => out,
            };
            // A box turned from a drop-down takes the keyboard at once.
            if self.ui.panes.fields.focus_next.as_deref() == Some(name) {
                self.ui.panes.fields.focus_next = None;
                out.request_focus();
            }
            // A box that has the focus has an edit: one that took the focus now, and one whose edit was finished or dropped while the box kept the focus (a click
            // that egui did not count as leaving it, a drawing that replaced the open one).
            let has_edit = self.ui.panes.fields.edits.iter().any(|e| e.name == name);
            if out.gained_focus() || (out.has_focus() && !has_edit) {
                if out.gained_focus() {
                    select_all(ui.ctx(), id, shown_text.chars().count());
                }
                let (page, ids) = target.map_or((0, Vec::new()), |t| (t.page, t.ids.to_vec()));
                let edits = &mut self.ui.panes.fields.edits;
                edits.retain(|e| e.name != name);
                edits.push(Edit {
                    name: name.to_owned(),
                    id,
                    rect: out.rect,
                    text: shown_text.clone(),
                    current: shown_text.clone(),
                    typed: false,
                    left: false,
                    page,
                    ids,
                });
            }
            let Some(edit) = self.ui.panes.fields.edits.iter_mut().find(|e| e.name == name) else { return };
            edit.rect = out.rect;
            if out.changed() {
                // Everything typed up to now, in whatever frame the box lets go of the focus in.
                edit.typed = true;
                edit.text = buffer.clone();
            }
            let same_shapes = target.is_some_and(|t| t.page == edit.page && t.ids == edit.ids.as_slice());
            if out.has_focus() && !edit.typed && (edit.current != shown_text || !same_shapes) {
                // Nothing typed and the drawing changed under the box (a command, Undo, a drag, a Tab from a field that sent): it shows the new value with
                // all of it selected, and is for the shapes that are shown now.
                edit.current = shown_text.clone();
                edit.text = shown_text.clone();
                if let Some(t) = target {
                    edit.page = t.page;
                    edit.ids = t.ids.to_vec();
                }
                select_all(ui.ctx(), id, shown_text.chars().count());
            } else if same_shapes {
                edit.current = shown_text.clone();
            }
            if out.has_focus() {
                edit.left = false;
                if escape {
                    // Esc leaves the old value in the box.
                    edit.typed = false;
                    ui.memory_mut(|m| m.surrender_focus(id));
                }
            } else {
                // Let go of, by Enter, Tab, a click elsewhere or Esc: it is finished at the top of the next frame.
                edit.left = true;
                ui.ctx().request_repaint();
            }
        });
        if let Some(refusal) = &self.ui.panes.fields.error
            && refusal.field == name
        {
            ui.add(egui::Label::new(egui::RichText::new(&refusal.words).font(theme::font(size::LABEL - 1.0)).color(t.notice_text)).wrap());
        }
    }

    /// Finishes the edits whose box has let go of the focus, and those a click lands outside of: each sends what was typed in it to the shapes it showed, or
    /// sends nothing when nothing was typed. **Run at the top of the frame, before anything else acts**, whether or not a pane is open.
    pub(crate) fn flush_edits(&mut self, ctx: &egui::Context) {
        self.ui.panes.fields.took_escape = false;
        if self.ui.panes.fields.edits.is_empty() {
            return;
        }
        let clicked = ctx.input(|i| if i.pointer.any_click() { i.pointer.interact_pos() } else { None });
        // egui counts a click within its interact radius of a box as a click on it, and the box keeps the focus: so does the edit.
        let reach = ctx.global_style().interaction.interact_radius;
        let escape = ctx.input(|i| i.key_pressed(Key::Escape));
        let (finished, still): (Vec<Edit>, Vec<Edit>) = std::mem::take(&mut self.ui.panes.fields.edits)
            .into_iter()
            .partition(|e| !ctx.memory(|m| m.has_focus(e.id)) || clicked.is_some_and(|p| !e.rect.expand(reach).contains(p)));
        self.ui.panes.fields.edits = still;
        for edit in finished {
            // Esc takes the focus from the box it is pressed in as the frame begins: the box was let go of in this frame, not in an earlier one. A box that
            // was let go of by Tab, Enter or a click in an earlier frame is not the Esc's.
            let by_escape = escape && !edit.left && !ctx.memory(|m| m.has_focus(edit.id));
            if by_escape {
                self.ui.panes.fields.took_escape = true;
            }
            if by_escape || !edit.typed || edit.text.trim() == edit.current.trim() {
                continue;
            }
            let typed = Typed { name: edit.name, text: edit.text, page: edit.page, ids: edit.ids };
            self.send_typed(typed);
        }
    }

    /// Drops the edits that wait: a drawing replaced the open one, and a value typed for the old one is not for the new.
    pub(crate) fn forget_edits(&mut self) {
        self.ui.panes.fields.edits.clear();
        self.ui.panes.fields.error = None;
        self.ui.panes.fields.took_escape = false;
    }

    /// Whether a field holds the keyboard in this frame: it has the focus, or an Escape has just ended its edit.
    pub(crate) fn pane_keys_held(&self) -> bool {
        self.ui.panes.fields.took_escape || !self.ui.panes.fields.edits.is_empty()
    }

    /// Sends a value typed in a pane's field to the shapes it was typed for.
    fn send_typed(&mut self, typed: Typed) {
        match typed.name.as_str() {
            name if name.starts_with("pane.size.") => self.send_size_typed(typed),
            name if name.starts_with("pane.format.") => self.send_format_typed(typed),
            name if name.starts_with("pane.data.") => self.send_data_typed(typed),
            _ => {}
        }
    }

    /// Says why the value typed in field `name` was refused: under the field while the shapes it was for are still selected, and in the status bar.
    pub(crate) fn field_refused(&mut self, name: &str, words: String, page: u32, ids: &[u32]) {
        self.set_notice(&words);
        self.ui.panes.fields.error = Some(Refusal { field: name.to_owned(), words, page, ids: ids.to_vec() });
    }

    /// Forgets a refusal: the field was typed in again and the value was taken.
    pub(crate) fn field_accepted(&mut self) {
        self.ui.panes.fields.error = None;
    }

    /// Forgets a refusal that was about shapes that are not the selection now.
    pub(crate) fn forget_stale_refusal(&mut self) {
        let page = self.ui.page;
        let stale = self.ui.panes.fields.error.as_ref().is_some_and(|r| Some(r.page) != page || r.ids != self.ui.edit.selection());
        if stale {
            self.ui.panes.fields.error = None;
        }
    }

    /// What the field `name` of a pane showed in the last frame: its text, or nothing when it was blank or not drawn. For tests and `ui.inspect`.
    pub fn pane_field_text(&self, name: &str) -> Option<&str> {
        // The frame drawn last: while the dock draws a frame its fields are in `now`, and when the frame is done they are the last drawn.
        let fields = &self.ui.panes.fields;
        let drawn = if fields.now.is_empty() { &fields.last } else { &fields.now };
        drawn.iter().find(|(n, _)| n == name).map(|(_, text)| text.as_str())
    }

    /// The words under the field `name`, if its last value was refused. For tests.
    pub fn pane_field_refusal(&self, name: &str) -> Option<&str> {
        self.ui.panes.fields.error.as_ref().filter(|r| r.field == name).map(|r| r.words.as_str())
    }

    /// How many fields are held in an edit: one has the focus, or has lost it and not been finished. For tests.
    pub fn pane_edits_open(&self) -> usize {
        self.ui.panes.fields.open_edits()
    }
}
