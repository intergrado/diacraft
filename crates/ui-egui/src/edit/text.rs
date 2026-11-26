//! Text edited in place: an egui text box over the shape, holding the shape's characters, sent with `shape.set_text` when the edit ends.
//!
//! Double-click, F2, or typing on a selected shape opens the box with all of the shape's characters selected (so what is typed replaces
//! them). **On a group whose `IsTextEditTarget` is FALSE the box opens over the member on top of its stack** (`group_text.rs`), and
//! the group stays selected. **Escape, Ctrl+Enter, or a click away ends the edit and keeps what was typed** ("Esc or a click away ends
//! it"); Enter is a line break. Nothing is sent if the text did not change, so an edit that changes nothing is no undo step.
//!
//! **The box edits the characters and never the fields** (of the M3 review). A shape's text is kept in parts: the characters, the
//! markers that start a run, and the fields with the text they show. The box holds the characters of
//! [`diacraft_engine::draw::text_view`]. Its fields are drawn under them as chips that take no keys and no pointer: shown, not typed into.
//! When the edit ends only the characters that changed are sent, as a range (`docs/editing-commands.md`, "Text"): the longest common prefix
//! and suffix of the old characters and the new are left alone ([`changed_range`]), so a run's formatting and a field survive one changed
//! letter.
//!
//! **A text edit that is open is ended before anything else changes the drawing or leaves it**: a page change, Undo, Redo, Save,
//! New, Open, the File view, an Export and the window's close all call [`DiacraftApp::commit_pending_text`] first, so a label being typed is
//! never lost without asking.

use diacraft_engine::doc::{Document, Owner, TextPart};
use diacraft_engine::draw::{MAX_TEXT_CHARS, TextField, text_view};
use diacraft_engine::shown;
use egui::text::{CCursor, CCursorRange};
use egui::{Align2, Context, CornerRadius, Frame, Id, Key, Margin, Modifiers, Order, Pos2, Rect, RichText, Sense, Stroke, TextEdit, Ui, vec2};
use serde_json::json;

use super::geom::{self, P};
use super::gesture::Gesture;
use super::group_text;
use super::{Cx, LEAST_SIZE, Tool, inch};
use crate::app::DiacraftApp;
use crate::theme::{self, Tokens, size};

/// The narrowest and the widest the box is made, in points.
const LEAST_WIDTH: f32 = 96.0;
const MOST_WIDTH: f32 = 420.0;
/// The size of the text in the box, in points, at the zoom where the shape's own text (a sixth of an inch) is that size, held to what can be read.
const LEAST_FONT: f32 = 10.0;
const MOST_FONT: f32 = 40.0;
/// The most characters of a field's result a chip shows.
const FIELD_CHARS: usize = 40;

/// A text box the Text tool is making, which is not a shape yet: its middle on the page and its size, in paper inches. A click gives no size (the
/// engine's `text.box` has one of its own, 1 inch by 0.25), a drag gives the size dragged.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct NewBox {
    pub x: f64,
    pub y: f64,
    pub width: Option<f64>,
    pub height: Option<f64>,
}

/// The width of the box `text.box` makes when it is given none, in inches (`docs/editing-commands.md`, "Text boxes and the text block"): the width
/// the editor of a new box has.
const BOX_WIDTH: f64 = 1.0;

/// A text box open over a shape, or over the place a new box will be.
pub(super) struct TextEditing {
    /// The shape being edited; 0 while the box is a new one (`new`): no shape has the `ID` 0. **It may be a member of the shape that is selected**: typing
    /// on a group goes to the member that holds its text (`group_text.rs`).
    pub id: u32,
    /// Where the shape is on the page, lower left and upper right in inches, **when the scene does not read it**: a member of a group set to group
    /// only has no place in the scene, and typing on such a group still reaches its member, so the box is drawn from the place the
    /// engine finds for it. `None` for a shape the scene reads (its place is then the scene's, which follows every edit).
    unread: Option<(P, P)>,
    /// The box the Text tool is making. **Nothing is sent until the edit ends with characters in it** (`text.box` goes with its text), so that no
    /// empty box is left on the page to delete.
    pub new: Option<NewBox>,
    /// The page the shape is on: the edit is sent there, whatever page is on view by then.
    pub page: u32,
    /// The characters being edited (the `chars` of `text_view`).
    pub text: String,
    original: String,
    /// The shape's fields, shown under the characters and never edited.
    fields: Vec<TextField>,
    /// The first frame: the box takes the keyboard and selects all.
    fresh: bool,
    /// Where the box was drawn in the last frame, to tell a click on it from one away from it.
    rect: Option<Rect>,
    /// The characters selected in the box as the last frame left them, `(start, end)` counted in characters with `start` not above `end`.
    /// A caret with nothing selected is `start == end`. `None` before the box has been drawn once. Formatting reads it (`format.rs`).
    selection: Option<(usize, usize)>,
    /// What is selected when the box opens: all of its characters when `None`.
    initial: Option<(usize, usize)>,
}

impl TextEditing {
    /// Whether the characters in the box differ from the ones the shape had when the box opened.
    pub(super) fn changed(&self) -> bool {
        self.text != self.original
    }

    /// Whether the shape is one the scene does not read (`unread`) and the document still has: its box stays open though the scene lacks it.
    pub(super) fn is_unread_member_still_there(&self, doc: Option<&Document>) -> bool {
        self.unread.is_some() && doc.is_some_and(|doc| doc.find_shape(Owner::Page(self.page), self.id).is_some())
    }
}

/// The id of the text field of the box over shape `shape`: one id for the box and for whoever asks whether it has the keyboard.
fn text_field_id(shape: u32) -> Id {
    Id::unique(("shape_text_box", shape))
}

/// The box that is open: the shape it edits, the page the shape is on and the characters selected in it, as the last frame left them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpenText {
    pub page: u32,
    pub id: u32,
    /// `(start, end)` in characters, `start` not above `end`; a caret is `start == end`; `None` before the box has been drawn.
    pub range: Option<(usize, usize)>,
}

/// The text a shape shows: its characters and its fields' text in order, markers left out.
pub fn plain_text(doc: &Document, page: u32, id: u32) -> String {
    let Some(at) = doc.find_shape(Owner::Page(page), id) else { return String::new() };
    let Some(shape) = doc.shape(&at) else { return String::new() };
    shape_text(shape)
}

/// The text `shape` shows: its characters and its fields' text in order, markers left out.
pub fn shape_text(shape: &diacraft_engine::doc::Shape) -> String {
    let Some(text) = &shape.text else { return String::new() };
    text.parts
        .iter()
        .map(|part| match part {
            TextPart::Chars(chars) => chars.as_ref(),
            TextPart::Fld { text, .. } => text.as_ref(),
            _ => "",
        })
        .collect()
}

/// What replaces what, to turn the characters `old` into `new`: the range `start..end` of `old` (counted in characters) and the characters
/// that go in its place. The longest common prefix is left alone, and then the longest common suffix of what is left, so the range is as
/// short as it can be (`docs/editing-commands.md`, "Text"). `None`: the two are the same.
pub fn changed_range(old: &str, new: &str) -> Option<(usize, usize, String)> {
    if old == new {
        return None;
    }
    let (old, new): (Vec<char>, Vec<char>) = (old.chars().collect(), new.chars().collect());
    let prefix = old.iter().zip(new.iter()).take_while(|(a, b)| a == b).count();
    let (old_rest, new_rest) = (old.get(prefix..).unwrap_or_default(), new.get(prefix..).unwrap_or_default());
    let suffix = old_rest.iter().rev().zip(new_rest.iter().rev()).take_while(|(a, b)| a == b).count();
    let inserted = new_rest.get(..new_rest.len().saturating_sub(suffix)).unwrap_or_default();
    Some((prefix, old.len().saturating_sub(suffix), inserted.iter().collect()))
}

/// `text` as a shape's text can hold it: no carriage returns, and none of the characters the file format cannot carry (the controls other than
/// a line break and a tab, U+FFFE and U+FFFF), which a paste can bring in. The engine refuses them, and a refusal would lose the label.
fn storable(text: &str) -> String {
    text.chars().filter(|c| !(c.is_control() && !matches!(c, '\n' | '\t')) && !matches!(c, '\u{fffe}' | '\u{ffff}')).collect()
}

/// Where the box goes on the screen for a shape whose box on the page is `lo` to `hi`: over the shape, at least [`LEAST_WIDTH`] wide.
fn box_centre_and_width(cx: &Cx, (lo, hi): (P, P)) -> (Pos2, f32) {
    let (a, b) = (cx.screen_at((lo.0, hi.1)), cx.screen_at((hi.0, lo.1)));
    let rect = Rect::from_two_pos(a, b);
    (rect.center(), rect.width().clamp(LEAST_WIDTH, MOST_WIDTH))
}

/// Where the box of a new text box goes on the screen: over the place it will have, at least [`LEAST_WIDTH`] wide.
fn new_box_place(cx: &Cx, new: NewBox) -> (Pos2, f32) {
    let width = (new.width.unwrap_or(BOX_WIDTH) * cx.view.scale()) as f32;
    (cx.screen_at((new.x, new.y)), width.clamp(LEAST_WIDTH, MOST_WIDTH))
}

/// The two corners of the box a drag with the Text tool has made so far: the press and the pointer, each where the engine's snap puts a place (the
/// drag's landing, `snapping.rs`) unless Alt is held.
fn drag_corners(gesture: &Gesture) -> (P, P) {
    gesture.landing.corners.filter(|_| gesture.landing.worked_out()).unwrap_or((gesture.origin, gesture.now))
}

impl DiacraftApp {
    /// The Text tool's release: a click on a shape types into it, a click on bare page opens a box there, a drag opens one of the size dragged. The
    /// tool is used up by it, so that the next click is a Pointer Tool's. **Nothing is sent: `text.box` goes when the edit ends with characters in it.**
    pub(super) fn finish_text_box(&mut self, cx: &Cx, gesture: &Gesture, on: Option<u32>) {
        self.set_tool(Tool::Pointer);
        if !gesture.active {
            if let Some(id) = on {
                self.select(&[id]);
                self.start_text(cx.page, id);
            } else {
                let at = self.land_place(cx, gesture.origin, gesture.alt, &[], &mut Vec::new());
                self.open_new_text_box(cx.page, NewBox { x: at.0, y: at.1, width: None, height: None });
            }
            return;
        }
        let (first, second) = drag_corners(gesture);
        let (lo, hi) = geom::ordered(first, second);
        let (w, h) = (hi.0 - lo.0, hi.1 - lo.1);
        // A side the grid made nothing of is left to the engine's own.
        let side = |n: f64| (n >= LEAST_SIZE).then_some(n);
        let new = NewBox { x: (lo.0 + hi.0) / 2.0, y: (lo.1 + hi.1) / 2.0, width: side(w), height: side(h) };
        self.open_new_text_box(cx.page, new);
    }

    /// Opens the editor over the place a new text box will have. The box is made when the edit ends with characters in it.
    fn open_new_text_box(&mut self, page: u32, new: NewBox) {
        self.commit_pending_text();
        self.ui.edit.gesture = None;
        self.set_selection(page, Vec::new());
        self.ui.edit.text = Some(TextEditing {
            id: 0,
            unread: None,
            new: Some(new),
            page,
            original: String::new(),
            text: String::new(),
            fields: Vec::new(),
            fresh: true,
            rect: None,
            selection: None,
            initial: None,
        });
    }

    /// The edit of a new box ended: `text.box` with the characters typed, if there are any, and the box is selected. One command, one undo step.
    fn commit_new_text_box(&mut self, page: u32, new: NewBox, typed: &str) {
        let text = storable(typed);
        if text.is_empty() {
            return;
        }
        let mut params = json!({"page": page, "x": inch(new.x), "y": inch(new.y), "text": text});
        if let (Some(width), Some(object)) = (new.width, params.as_object_mut()) {
            object.insert("width".to_owned(), inch(width));
        }
        if let (Some(height), Some(object)) = (new.height, params.as_object_mut()) {
            object.insert("height".to_owned(), inch(height));
        }
        let made = self
            .run_edit("text.box", &params)
            .and_then(|answer| answer.get("id").and_then(serde_json::Value::as_u64))
            .and_then(|id| u32::try_from(id).ok());
        if let Some(id) = made {
            self.select(&[id]);
            self.keep_preview(page, &[id]);
        }
    }

    /// Paints the box the Text tool's drag has made so far.
    pub(super) fn paint_text_box_drag(&self, painter: &egui::Painter, t: &Tokens, cx: &Cx, gesture: &Gesture) {
        if !gesture.active {
            return;
        }
        let (a, b) = drag_corners(gesture);
        let rect = egui::Rect::from_two_pos(cx.screen_at(a), cx.screen_at(b));
        painter.rect(rect, CornerRadius::ZERO, t.edit_fill, Stroke::new(1.0, t.edit_line), egui::StrokeKind::Inside);
        super::paint::paint_marks(painter, t, cx, &gesture.landing.marks);
    }

    /// Opens the text box over the shape under `over`, if the pointer is on one. A double-click.
    ///
    /// **A member that is selected on its own, with the pointer on it, is the shape double clicked**, and not the group it is in: its text is edited, whatever
    /// the group's `IsTextEditTarget` says. (A group selected is not a member: the second click of the pair did not go in to the member under it,
    /// `gesture.rs`, and the group sends the text where its cell says, `start_text`.)
    pub(super) fn edit_text_at(&mut self, cx: &Cx, over: Option<Pos2>) {
        let Some(at) = over else { return };
        self.ensure_scene(cx.page);
        let (p, reach) = (cx.page_at(at), cx.reach(super::HIT_REACH));
        let member = self.single().filter(|item| item.parent.is_some() && item.hit(p, reach)).map(|item| item.id);
        let hit = member.or_else(|| self.ui.edit.scene.as_ref().and_then(|s| s.hit(p, reach)).map(|i| i.id));
        if let Some(id) = hit {
            self.select(&[id]);
            self.start_text(cx.page, id);
        }
    }

    /// Opens the text box over the one selected shape (F2, or typing on it). `typed` is only a sign that the box is opened by typing: the
    /// box sees the same key event this frame and takes it.
    pub(super) fn edit_selected_text(&mut self, _typed: Option<String>) {
        let Some(page) = self.current_page() else { return };
        if let [only] = self.ui.edit.selection.as_slice() {
            let id = *only;
            self.start_text(page, id);
        }
    }

    /// Opens the box where text typed on shape `id` goes: the shape itself, or for a group whose `IsTextEditTarget` is FALSE the member on top of its stack,
    /// the last in document order (`group_text.rs`). The selection stays as it is.
    pub(super) fn start_text(&mut self, page: u32, id: u32) {
        let target = self.session.document().map_or(id, |doc| group_text::text_target(doc, page, id));
        self.open_text_box(page, target, None);
    }

    /// Opens the box over shape `id` with `initial` selected, or all of its characters.
    fn open_text_box(&mut self, page: u32, id: u32, initial: Option<(usize, usize)>) {
        // A box that is open on another shape ends first: what was typed in it is kept.
        self.commit_pending_text();
        let Some(view) = self.session.document().and_then(|doc| text_view(doc, page, id)) else { return };
        self.ui.edit.gesture = None;
        let unread = self.place_not_in_scene(page, id);
        self.ui.edit.text = Some(TextEditing {
            id,
            unread,
            page,
            original: view.chars.clone(),
            text: view.chars,
            fields: view.fields,
            new: None,
            fresh: true,
            rect: None,
            selection: None,
            initial,
        });
    }

    /// Where shape `id` is on the page, in inches, when the scene does not read it (a member of a group set to group only): the corners the
    /// engine gives it, carried up through its groups. `None` when the scene reads it, and for a shape the engine cannot place either (the box then ends at
    /// once, as it does for any shape that is not there).
    fn place_not_in_scene(&mut self, page: u32, id: u32) -> Option<(P, P)> {
        self.ensure_scene(page);
        if self.ui.edit.scene.as_ref().is_some_and(|scene| scene.get(id).is_some()) {
            return None;
        }
        let doc = self.session.document()?;
        let per_inch = diacraft_engine::draw::units_per_inch(doc, page);
        let corners = diacraft_engine::draw::shape_corners(doc, page, id)?.map(|(x, y)| (x / per_inch, y / per_inch));
        geom::bounds_of(&corners).filter(|(lo, hi)| [lo.0, lo.1, hi.0, hi.1].iter().all(|n| n.is_finite()))
    }

    /// The box that is open, if one is, and the characters selected in it: what formatting needs before it sends what was typed.
    pub(crate) fn open_text_selection(&self) -> Option<OpenText> {
        let editing = self.ui.edit.text.as_ref().filter(|editing| editing.new.is_none())?;
        Some(OpenText { page: editing.page, id: editing.id, range: editing.selection })
    }

    /// The id of the text field of the box that is open: how to tell the shape's box from any other text field that has the keyboard.
    pub(crate) fn text_box_field(&self) -> Option<Id> {
        self.ui.edit.text.as_ref().map(|editing| text_field_id(editing.id))
    }

    /// The characters selected in the text box that is open, `(start, end)` counted in characters (a caret is `start == end`).
    pub fn edited_text_selection(&self) -> Option<(usize, usize)> {
        self.ui.edit.text.as_ref().and_then(|t| t.selection)
    }

    /// How many characters the editable text of shape `id` has now.
    pub(crate) fn text_chars(&self, page: u32, id: u32) -> Option<usize> {
        let view = self.session.document().and_then(|doc| text_view(doc, page, id))?;
        Some(view.chars.chars().count())
    }

    /// Opens the box again over shape `id` after a format, with the same characters selected, so that the person goes on typing.
    pub(crate) fn reopen_text(&mut self, page: u32, id: u32, selected: Option<(usize, usize)>) {
        let n = self.text_chars(page, id).unwrap_or(0);
        self.open_text_box(page, id, selected.map(|(from, to)| (from.min(n), to.min(n))));
    }

    /// The shape the open text box edits, if one is open over a shape (a new text box has none until its edit ends). **It may be a member of the
    /// shape that is selected**: typing on a group reaches the member that holds its text.
    pub fn edited_text_shape(&self) -> Option<u32> {
        self.ui.edit.text.as_ref().filter(|t| t.new.is_none()).map(|t| t.id)
    }

    /// The characters in the text box, if one is open.
    pub fn edited_text(&self) -> Option<&str> {
        self.ui.edit.text.as_ref().map(|t| t.text.as_str())
    }

    /// The results of the fields the open text box shows and does not edit, in order.
    pub fn edited_text_fields(&self) -> Vec<String> {
        self.ui.edit.text.iter().flat_map(|t| t.fields.iter().map(|f| f.text.clone())).collect()
    }

    /// Whether a text box is open with characters in it that the drawing does not have yet.
    pub fn has_unsent_text(&self) -> bool {
        self.ui.edit.text.as_ref().is_some_and(TextEditing::changed)
    }

    /// Ends the text edit that is open, if one is, and sends what was typed. Called before anything that changes the drawing, leaves it, or
    /// asks whether it has changes: a page change, Undo and Redo, Save, New, Open, a close, so that a label being typed is not lost.
    ///
    /// **Only the characters that changed are sent**, as a range, against the characters the shape has now (the shape may have been changed
    /// by a program while the box was open): the markers and the fields are not touched. A failure is told in the status bar.
    pub fn commit_pending_text(&mut self) {
        let Some(editing) = self.ui.edit.text.take() else { return };
        if let Some(new) = editing.new {
            self.commit_new_text_box(editing.page, new, &editing.text);
            return;
        }
        let Some(view) = self.session.document().and_then(|doc| text_view(doc, editing.page, editing.id)) else { return };
        let Some((start, end, text)) = changed_range(&view.chars, &storable(&editing.text)) else { return };
        let params = json!({"id": editing.id, "text": text, "start": start, "end": end, "page": editing.page});
        self.run_edit("shape.set_text", &params);
    }

    /// One frame of the open text box: ends the edit on Escape, Ctrl+Enter or a press away from the box, and draws it.
    pub(super) fn text_box_input(&mut self, ctx: &Context, cx: &Cx, pressed: bool, _over: Option<Pos2>) {
        let Some(editing) = &self.ui.edit.text else { return };
        let (id, unread, previous, new) = (editing.id, editing.unread, editing.rect, editing.new);
        let place = match new {
            Some(new) => new_box_place(cx, new),
            None => {
                let Some(bounds) = self.ui.edit.scene.as_ref().and_then(|s| s.get(id)).map(|item| item.bounds).or(unread) else {
                    self.ui.edit.text = None;
                    return;
                };
                box_centre_and_width(cx, bounds)
            }
        };
        let escape = ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape));
        let ctrl_enter = ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::Enter));
        let press_at = ctx.input(|i| i.pointer.latest_pos());
        // A press away from the box ends the edit, **but not one on the ribbon's body or on something floating over the drawing (a menu, a
        // palette, a list)**: a person selects characters and then presses Bold, Font Color or a size, and goes on typing.
        let on_float = press_at.and_then(|p| ctx.layer_id_at(p)).is_some_and(|layer| layer.order != Order::Background);
        let on_ribbon = press_at.is_some_and(|p| self.ui.ribbon_body.is_some_and(|r| r.contains(p)));
        let away = pressed && !on_float && !on_ribbon && press_at.is_some_and(|p| previous.is_some_and(|r| !r.contains(p)));
        if escape || ctrl_enter || away {
            self.commit_pending_text();
            return;
        }
        self.draw_text_box(ctx, cx, place);
    }

    fn draw_text_box(&mut self, ctx: &Context, cx: &Cx, (centre, width): (Pos2, f32)) {
        let Some(editing) = self.ui.edit.text.as_mut() else { return };
        let t = Tokens::get(ctx);
        let font = egui::FontId::proportional((cx.view.scale() as f32 / 6.0).clamp(LEAST_FONT, MOST_FONT));
        let field = text_field_id(editing.id);
        // The box has the keyboard for as long as it is open (an area's first frame is a sizing pass that cannot take it).
        // (Unless another text field has it: the list of fonts has a box to type a name in, and the box would take the keyboard back.)
        let other_text_field = ctx.text_edit_focused() && ctx.memory(|m| m.focused()) != Some(field);
        ctx.memory_mut(|m| {
            if m.focused() != Some(field) && !other_text_field {
                m.request_focus(field);
            }
        });
        if editing.fresh {
            let mut state = egui::text_edit::TextEditState::load(ctx, field).unwrap_or_default();
            let range = match editing.initial {
                Some((from, to)) => CCursorRange::two(CCursor::new(from), CCursor::new(to)),
                None => CCursorRange::select_all_str(&editing.text),
            };
            state.cursor.set_char_range(Some(range));
            state.store(ctx, field);
            editing.fresh = false;
        }
        let mut shown_rect = None;
        let mut chips = Vec::new();
        egui::Area::new(Id::unique("shape_text_area")).order(Order::Foreground).pivot(Align2::CENTER_CENTER).fixed_pos(centre).show(ctx, |ui| {
            Frame::new().fill(t.input_fill).stroke(Stroke::new(1.5, t.edit_line)).inner_margin(Margin::symmetric(4, 2)).show(ui, |ui| {
                let out = TextEdit::multiline(&mut editing.text)
                    .id(field)
                    .font(font)
                    .text_color(t.text)
                    .frame(Frame::NONE)
                    .desired_width(width - 8.0)
                    .desired_rows(1)
                    .char_limit(MAX_TEXT_CHARS)
                    .align(Align2::CENTER_TOP)
                    .show(ui);
                out.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::MultilineTextInput, true, "Shape text"));
                // (Kept when the box is not focused for a frame, as it is in the one where a press on the ribbon takes the keyboard.)
                if let Some(range) = out.cursor_range {
                    let [from, to] = range.sorted_cursors();
                    editing.selection = Some((from.index.0, to.index.0));
                }
                chips = field_chips(ui, &t, &editing.fields);
            });
            shown_rect = Some(ui.min_rect().expand2(vec2(4.0, 2.0)));
        });
        editing.rect = shown_rect;
        if let Some(rect) = shown_rect {
            self.note("shape.text", rect, true);
        }
        for (n, rect) in chips.into_iter().enumerate() {
            self.note(format!("shape.text.field.{n}"), rect, false);
        }
    }
}

/// The fields of the shape under its characters: one chip each, with what it shows. They are labels that sense nothing but the pointer's
/// rest, so the box's keys and clicks never reach them; each says in its hover text that it is not typed over. → where each was drawn.
fn field_chips(ui: &mut Ui, t: &Tokens, fields: &[TextField]) -> Vec<Rect> {
    if fields.is_empty() {
        return Vec::new();
    }
    let mut drawn = Vec::with_capacity(fields.len());
    ui.add_space(2.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(4.0, 2.0);
        ui.label(RichText::new("Fields:").font(theme::font(size::LABEL - 1.0)).color(t.text_dim));
        for field in fields {
            let result = shown(&field.text, FIELD_CHARS);
            let words = if result.is_empty() {
                "A field. It shows nothing, and is not typed over.".to_owned()
            } else {
                format!("A field. It shows {result}, and is not typed over.")
            };
            let chip = Frame::new()
                .fill(t.accent_soft)
                .stroke(Stroke::new(1.0, t.input_border))
                .corner_radius(CornerRadius::same(3))
                .inner_margin(Margin::symmetric(5, 1))
                .show(ui, |ui| {
                    let text = if result.is_empty() { "(empty)".to_owned() } else { result.clone() };
                    ui.add(egui::Label::new(RichText::new(text).font(theme::font(size::LABEL)).color(t.text)).selectable(false).sense(Sense::hover()))
                });
            chip.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, format!("Field {result}")));
            drawn.push(chip.response.rect);
            chip.response.on_hover_text(words);
        }
    });
    drawn
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use serde_json::json;

    use super::*;

    #[test]
    fn a_shapes_text_is_its_characters_without_markers() {
        let mut s = diacraft_engine::Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let page = s.document().unwrap().pages.get(0).unwrap().id;
        let id = s.execute("shape.drop", &json!({"master": "rectangle", "page": page, "x": 3.0, "y": 3.0})).unwrap()["id"].as_u64().unwrap() as u32;
        assert_eq!(plain_text(s.document().unwrap(), page, id), "");
        s.execute("shape.set_text", &json!({"id": id, "text": "Start\nHere", "page": page})).unwrap();
        assert_eq!(plain_text(s.document().unwrap(), page, id), "Start\nHere");
        assert_eq!(plain_text(s.document().unwrap(), page, 999), "");
    }

    #[test]
    fn the_changed_range_leaves_the_common_prefix_and_suffix_alone() {
        assert_eq!(changed_range("abcd", "abcd"), None);
        // One letter changed: the range is that letter.
        assert_eq!(changed_range("abcd", "abce"), Some((3, 4, "e".to_owned())));
        assert_eq!(changed_range("abcd", "aXcd"), Some((1, 2, "X".to_owned())));
        // Characters added and taken away, at the ends and in the middle.
        assert_eq!(changed_range("abcd", "abcde"), Some((4, 4, "e".to_owned())));
        assert_eq!(changed_range("abcd", "Xabcd"), Some((0, 0, "X".to_owned())));
        assert_eq!(changed_range("abcd", "ad"), Some((1, 3, String::new())));
        assert_eq!(changed_range("abcd", ""), Some((0, 4, String::new())));
        assert_eq!(changed_range("", "hi"), Some((0, 0, "hi".to_owned())));
        // The prefix and the suffix never overlap: one more of a repeated letter is one insertion.
        assert_eq!(changed_range("aa", "aaa"), Some((2, 2, "a".to_owned())));
        assert_eq!(changed_range("aaa", "aa"), Some((2, 3, String::new())));
        assert_eq!(changed_range("abab", "ababab"), Some((4, 4, "ab".to_owned())));
        // Characters are counted as characters, not bytes.
        assert_eq!(changed_range("h\u{e9}llo \u{1f600}!", "hello \u{1f600}!"), Some((1, 2, "e".to_owned())));
        assert_eq!(changed_range("\u{1f600}", "\u{1f601}"), Some((0, 1, "\u{1f601}".to_owned())));
    }

    #[test]
    fn applying_the_range_to_the_old_characters_gives_the_new_ones() {
        let cases = [("abcd", "abce"), ("", "x"), ("xyz", ""), ("aXa", "aa"), ("one two", "one  two"), ("caf\u{e9}", "cafe\u{301}!"), ("aaaa", "aa")];
        for (old, new) in cases {
            let (start, end, text) = changed_range(old, new).unwrap();
            let chars: Vec<char> = old.chars().collect();
            let rebuilt: String = chars[..start].iter().collect::<String>() + &text + &chars[end..].iter().collect::<String>();
            assert_eq!(rebuilt, new, "{old:?} to {new:?} by {start}..{end} {text:?}");
        }
    }

    #[test]
    fn what_a_file_cannot_carry_is_left_out_of_a_label_before_it_is_sent() {
        assert_eq!(storable("a\r\nb\u{1b}c\td\u{fffe}\u{ffff}"), "a\nbc\td");
        assert_eq!(storable("plain, \u{e9}\u{1f600}"), "plain, \u{e9}\u{1f600}");
    }
}
