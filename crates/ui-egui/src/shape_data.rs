//! The Define Shape Data dialog and the question a drop asks: Data, Show/Hide, Shape Data Window, and the shape's right-click menu, Data, Define Shape Data
//! ("Fields are added with right-click, Data, **Define Shape Data** (dialog: New, then Label, Type, Format, Prompt, and OK; the Format choices follow the chosen type; the
//! Prompt shows when the pointer rests on a field)").
//!
//! **The dialog works on a draft**, as the Hyperlinks and Layer Properties dialogs do: nothing is sent until OK, and then **one `shapedata.define`** (one undo step, "Define shape data") with the
//! whole list: a field kept by its row name, a field made here with none, and a field left out taken away. The list on the left holds every field of the one shape selected, **hidden ones too**
//! (the dialog is where a hidden field is shown again), in the order the Shape Data window lists them; the buttons **New**, **Delete**, **Move Up** and **Move Down** change it. The boxes at the right
//! are the chosen field's: **Label**, **Name** (the row's name, formulas read it as `Prop.<name>`: typed for a field made here, and read-only for one the shape has, since a row is never renamed),
//! **Type** (the engine's eight types; a row whose `Type` cell holds another number says **Unknown (n)**), **Format** (the picture the value is shown through, with the presets of the type; for a
//! list its items, apart at semicolons), **Value**, **Prompt**, **Sort key**, the boxes **Hidden** and **Ask on drop** (the `Verify` cell), the **Language** (a tag such as `en-US`) and, for a date, the
//! **Calendar**. A refusal of the engine is said in the dialog, which stays open. **A box that is not changed sends nothing**: a type, a value, a language and a calendar the person
//! leaves are kept as the file wrote them, so a `Type` the dialog does not know or a language a file holds as a number survives an OK.
//!
//! * **Move Up and Move Down** change the order by the sort key ("fields with a SortKey list first in that order... Rows themselves cannot be moved"): the two fields change places
//!   in the list, and **every field's sort key is written as its place, `0001`, `0002` and so on** (ours: the keys are compared as text, so they are padded to four digits; a list of more than 9,999
//!   fields cannot be held, and the engine holds at most 512).
//! * **Language and Calendar**: the language is typed as a tag and the engine checks it; the calendar is a drop-down of the
//!   fourteen calendars `shapedata.types` names, offered for a date field only, since only a date reads it. A field that holds no calendar shows the default, Western.
//!
//! **Ask on drop** ("with Ask on drop set on a field, creating, duplicating or copying the shape prompts for its value"): after a master is dropped by the Shapes window, the
//! window asks `shapedata.list` for the new shape, and when a field has the box ticked a small **Shape Data** card lists those fields with an editor of their type. **OK** sends one `shapedata.set`
//! of the values changed (its own undo step, after the drop's), and **Cancel** leaves the shape as it was dropped. Creating by a paste or a duplicate does not ask yet: the specifications
//! name them, and the window has no place yet where they end.

use std::sync::Arc;

use diacraft_engine::draw::shapedata::MAX_FIELDS;
use diacraft_engine::{EngineError, Result};
use egui::{Context, Id, Key, Modifiers, RichText, TextEdit, Ui, vec2};
use serde_json::{Value, json};

use crate::app::DiacraftApp;
use crate::insert_links::{edit_row, heading, list_row};
use crate::theme::{self, Tokens, size};

/// What the dialogs of Shape Data keep.
#[derive(Default)]
pub struct DataState {
    pub define: Option<DefineDialog>,
    pub ask: Option<AskDialog>,
    /// The types and their formats, as the engine lists them (`shapedata.types`), asked the first time they are wanted.
    types: Option<Arc<Vec<TypeInfo>>>,
    /// The calendars a date field may name, `(number, name)`, from the same answer.
    calendars: Option<Calendars>,
}

/// The calendars a date field may name, `(number, name)`, as `shapedata.types` lists them.
type Calendars = Arc<Vec<(u32, String)>>;

/// A type of the Define dialog's list, as the engine gives it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TypeInfo {
    id: String,
    label: String,
    list: bool,
    /// The pictures offered for the type: the picture and its words.
    formats: Vec<(String, String)>,
}

/// A field of the draft: a row of the shape as it was (`row`), or one made in the dialog.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Draft {
    pub row: Option<String>,
    /// The name of a field made here.
    pub name: String,
    pub label: String,
    /// The type's word (`unknown` for a `Type` cell that holds no number of the eight), and the word it had when the dialog opened: a type that is not changed is not sent.
    pub kind: String,
    pub was_kind: String,
    /// The whole number the row's `Type` cell holds, for a type the dialog does not know.
    pub type_code: Option<i64>,
    pub format: String,
    pub value: String,
    /// The value the row had when the dialog opened: a value that is not changed is not sent.
    pub was_value: String,
    pub prompt: String,
    pub sort_key: String,
    pub invisible: bool,
    pub ask: bool,
    /// The `LangID` cell's text, and what it was when the dialog opened.
    pub lang_id: String,
    pub was_lang_id: String,
    /// The calendar's number, and what it was when the dialog opened.
    pub calendar: Option<u32>,
    pub was_calendar: Option<u32>,
}

impl Draft {
    fn made() -> Draft {
        Draft {
            row: None,
            name: String::new(),
            label: String::new(),
            kind: "string".to_owned(),
            was_kind: "string".to_owned(),
            type_code: Some(0),
            format: String::new(),
            value: String::new(),
            was_value: String::new(),
            prompt: String::new(),
            sort_key: String::new(),
            invisible: false,
            ask: false,
            lang_id: String::new(),
            was_lang_id: String::new(),
            calendar: None,
            was_calendar: None,
        }
    }

    fn of(field: &Value) -> Draft {
        let text = |key: &str| field.get(key).and_then(Value::as_str).unwrap_or("").to_owned();
        let calendar = field.get("calendar").and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok());
        // A value that shows nothing is typed as nothing (a date never set holds a plain 0); a truth value is typed as its words, the others as the engine stores them (which a person can type
        // back whatever the format shows).
        let value = if text("shown").is_empty() {
            String::new()
        } else if text("type") == "boolean" {
            text("shown")
        } else {
            text("value")
        };
        Draft {
            row: Some(text("row")),
            name: text("row"),
            label: text("label"),
            kind: text("type"),
            was_kind: text("type"),
            type_code: field.get("type_code").and_then(Value::as_i64),
            format: text("format"),
            was_value: value.clone(),
            value,
            prompt: text("prompt"),
            sort_key: text("sort_key"),
            invisible: field.get("invisible").and_then(Value::as_bool).unwrap_or(false),
            ask: field.get("ask").and_then(Value::as_bool).unwrap_or(false),
            lang_id: text("lang_id"),
            was_lang_id: text("lang_id"),
            calendar,
            was_calendar: calendar,
        }
    }

    /// What the list says of the field.
    fn caption(&self) -> String {
        let mut text = if self.label.trim().is_empty() { "(new field)".to_owned() } else { self.label.trim().to_owned() };
        if self.invisible {
            text.push_str("  (hidden)");
        }
        text
    }

    /// The field as `shapedata.define` takes it: a box of a field the shape has is sent only when it was changed, and the engine checks what is sent.
    fn json(&self) -> Value {
        let new = self.row.is_none();
        let mut map = serde_json::Map::new();
        match &self.row {
            Some(row) => {
                map.insert("row".to_owned(), json!(row));
                // A value that was not typed over is left as it is.
                if self.value != self.was_value {
                    map.insert("value".to_owned(), json!(self.value));
                }
            }
            None => {
                if !self.name.trim().is_empty() {
                    map.insert("name".to_owned(), json!(self.name.trim()));
                }
                map.insert("value".to_owned(), json!(self.value));
            }
        }
        if !self.label.trim().is_empty() || self.row.is_some() {
            map.insert("label".to_owned(), json!(self.label.trim()));
        }
        if new || self.kind != self.was_kind {
            map.insert("type".to_owned(), json!(self.kind));
        }
        map.insert("format".to_owned(), json!(self.format));
        map.insert("prompt".to_owned(), json!(self.prompt));
        map.insert("sort_key".to_owned(), json!(self.sort_key));
        map.insert("invisible".to_owned(), json!(self.invisible));
        map.insert("ask".to_owned(), json!(self.ask));
        if self.lang_id != self.was_lang_id || (new && !self.lang_id.trim().is_empty()) {
            map.insert("lang_id".to_owned(), json!(self.lang_id.trim()));
        }
        if self.calendar != self.was_calendar
            && let Some(n) = self.calendar
        {
            map.insert("calendar".to_owned(), json!(n));
        }
        Value::Object(map)
    }
}

/// The Define Shape Data dialog while it is open.
#[derive(Clone, Debug)]
pub struct DefineDialog {
    page: u32,
    id: u32,
    /// The shape's name, for the title.
    shape: String,
    drafts: Vec<Draft>,
    /// The field the boxes show.
    selected: usize,
    types: Arc<Vec<TypeInfo>>,
    calendars: Calendars,
    /// What the engine refused the last OK with, said in the dialog.
    error: Option<String>,
}

/// A field the drop's question asks for.
#[derive(Clone, Debug, PartialEq)]
struct AskRow {
    row: String,
    label: String,
    kind: String,
    items: Vec<String>,
    prompt: String,
    value: String,
    start: String,
}

/// The question a drop asks, while it is open.
#[derive(Clone, Debug)]
pub struct AskDialog {
    page: u32,
    id: u32,
    shape: String,
    rows: Vec<AskRow>,
    error: Option<String>,
}

/// What a press in the Define dialog chose, taken after the card is drawn.
#[derive(Default)]
struct Pressed {
    ok: bool,
    cancel: bool,
    new: bool,
    delete: bool,
    up: bool,
    down: bool,
}

/// What a refusal says, without the name of the command that gave it: the words the person reads in the pane and the dialogs.
pub(crate) fn words(error: &EngineError) -> String {
    match error {
        EngineError::BadParams { msg, .. } | EngineError::Failed { msg, .. } => msg.clone(),
        other => other.to_string(),
    }
}

fn bad(cmd: &str, msg: impl Into<String>) -> EngineError {
    EngineError::BadParams { cmd: cmd.to_owned(), msg: msg.into() }
}

impl DiacraftApp {
    /// The dialogs' state for the window state and the tests: `null` when closed, else the shape, the fields and the one chosen.
    pub(crate) fn shape_data_state(&self) -> Value {
        let define = self.ui.shape_data.define.as_ref().map(|d| {
            json!({
                "id": d.id,
                "page": d.page,
                "selected": d.selected,
                "error": d.error,
                "fields": d.drafts.iter().map(|f| json!({
                    "row": f.row, "label": f.label, "type": f.kind, "format": f.format, "value": f.value, "prompt": f.prompt,
                    "sort_key": f.sort_key, "invisible": f.invisible, "ask": f.ask, "lang_id": f.lang_id, "calendar": f.calendar,
                })).collect::<Vec<_>>(),
            })
        });
        let ask = self.ui.shape_data.ask.as_ref().map(|d| {
            json!({
                "id": d.id,
                "page": d.page,
                "error": d.error,
                "fields": d.rows.iter().map(|r| json!({"row": r.row, "label": r.label, "type": r.kind, "value": r.value})).collect::<Vec<_>>(),
            })
        });
        json!({ "define": define, "ask": ask })
    }

    /// Whether Define Shape Data can run now, and if not why.
    pub(crate) fn define_data_enabled(&self) -> std::result::Result<(), String> {
        self.command_enabled("shapedata.define")?;
        if self.one_shape_selected().is_none() {
            return Err("select one shape to define its shape data".to_owned());
        }
        Ok(())
    }

    /// The types and the calendars the engine lists, asked once.
    fn data_types(&mut self) -> (Arc<Vec<TypeInfo>>, Calendars) {
        if let (Some(types), Some(calendars)) = (&self.ui.shape_data.types, &self.ui.shape_data.calendars) {
            return (Arc::clone(types), Arc::clone(calendars));
        }
        let text = |v: &Value, key: &str| v.get(key).and_then(Value::as_str).unwrap_or("").to_owned();
        let answer = self.session.execute("shapedata.types", &json!({})).ok();
        let calendars: Vec<(u32, String)> = answer
            .as_ref()
            .and_then(|a| a.get("calendars").and_then(Value::as_array))
            .map(|list| list.iter().filter_map(|c| Some((u32::try_from(c.get("code").and_then(Value::as_u64)?).ok()?, text(c, "name")))).collect())
            .unwrap_or_default();
        let list: Vec<TypeInfo> = answer
            .and_then(|a| a.get("types").and_then(Value::as_array).cloned())
            .unwrap_or_default()
            .iter()
            .map(|t| TypeInfo {
                id: text(t, "id"),
                label: text(t, "label"),
                list: t.get("list").and_then(Value::as_bool).unwrap_or(false),
                formats: t
                    .get("formats")
                    .and_then(Value::as_array)
                    .map(|f| f.iter().map(|p| (text(p, "picture"), text(p, "label"))).collect())
                    .unwrap_or_default(),
            })
            .collect();
        let (types, calendars) = (Arc::new(list), Arc::new(calendars));
        self.ui.shape_data.types = Some(Arc::clone(&types));
        self.ui.shape_data.calendars = Some(Arc::clone(&calendars));
        (types, calendars)
    }

    /// Opens the dialog for the one shape selected: its fields are asked of the engine, hidden ones too. An open dialog stays as it is.
    pub(crate) fn open_define_shape_data(&mut self) -> Result<()> {
        const ID: &str = "window.defineShapeData";
        if self.ui.shape_data.define.is_some() {
            return Ok(());
        }
        let (page, id) = self.one_shape_selected().ok_or_else(|| bad(ID, "select one shape to define its shape data"))?;
        self.commit_pending_text();
        let answer = self.execute("shapedata.list", &json!({"id": id, "page": page, "hidden": true}))?;
        let drafts: Vec<Draft> = answer.get("fields").and_then(Value::as_array).map(|list| list.iter().map(Draft::of).collect()).unwrap_or_default();
        let shape = answer.get("name").and_then(Value::as_str).unwrap_or("").to_owned();
        let (types, calendars) = self.data_types();
        self.ui.shape_data.define = Some(DefineDialog { page, id, shape, drafts, selected: 0, types, calendars, error: None });
        Ok(())
    }

    pub(crate) fn close_define_shape_data(&mut self) {
        self.ui.shape_data.define = None;
    }

    /// OK: one `shapedata.define` with the draft, and the dialog closes; a refusal is said in the dialog, which stays.
    fn apply_define_shape_data(&mut self) {
        let Some(dialog) = self.ui.shape_data.define.clone() else { return };
        let fields: Vec<Value> = dialog.drafts.iter().map(Draft::json).collect();
        match self.execute("shapedata.define", &json!({"id": dialog.id, "page": dialog.page, "fields": fields})) {
            Ok(_) => self.close_define_shape_data(),
            Err(e) => {
                if let Some(open) = self.ui.shape_data.define.as_mut() {
                    open.error = Some(words(&e));
                }
            }
        }
    }

    pub(crate) fn define_data_dialog(&mut self, ctx: &Context, t: Tokens) {
        let Some(mut d) = self.ui.shape_data.define.take() else { return };
        let screen = ctx.content_rect();
        let width = (screen.width() - 60.0).clamp(340.0, 700.0);
        let list_w = (width * 0.34).clamp(130.0, 210.0);
        // The boxes of a field are tall: the dialog takes the room the window has, and scrolls where it has less.
        let boxes_h = (screen.height() - 230.0).clamp(220.0, 580.0);
        let mut pressed = Pressed::default();
        if !egui::Popup::is_any_open(ctx) && ctx.memory(|m| m.focused().is_none()) && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter)) {
            pressed.ok = true;
        }
        let mut seen: Vec<(String, egui::Rect, bool)> = Vec::new();
        let modal = egui::Modal::new(Id::unique("define_shape_data_dialog")).backdrop_color(t.scrim).frame(Self::card_frame(&t)).show(ctx, |ui| {
            ui.set_width(width);
            let title = if d.shape.is_empty() {
                "Define Shape Data".to_owned()
            } else {
                format!("Define Shape Data - {}", diacraft_engine::shown(&d.shape, 40))
            };
            ui.label(RichText::new(title).font(theme::font(size::TAB + 1.0)).color(t.text));
            ui.add_space(8.0);
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(list_w);
                    heading(ui, &t, "Fields");
                    egui::ScrollArea::vertical()
                        .id_salt("define_data_list")
                        .min_scrolled_height((boxes_h - 60.0).min(200.0))
                        .max_height(boxes_h - 60.0)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.set_width(list_w - 8.0);
                            ui.spacing_mut().item_spacing.y = 2.0;
                            for (at, draft) in d.drafts.iter().enumerate() {
                                if list_row(ui, &t, &draft.caption(), at == d.selected, format!("data.field.{at}"), &mut seen) {
                                    d.selected = at;
                                }
                            }
                        });
                    ui.add_space(6.0);
                    // Two rows of two buttons, as wide as half the list.
                    let each = ((list_w - 8.0) / 2.0).max(60.0);
                    for row in [[("New", "data.new", 0), ("Delete", "data.delete", 1)], [("Move Up", "data.up", 2), ("Move Down", "data.down", 3)]] {
                        ui.horizontal(|ui| {
                            for (label, key, press) in row {
                                let enabled = match press {
                                    0 => d.drafts.len() < MAX_FIELDS,
                                    1 => at_least_one(&d),
                                    2 => d.selected > 0,
                                    _ => d.selected + 1 < d.drafts.len(),
                                };
                                let b = ui.add_enabled(
                                    enabled,
                                    egui::Button::new(RichText::new(label).font(theme::font(size::LABEL))).min_size(vec2(each, 24.0)),
                                );
                                seen.push((key.to_owned(), b.rect, enabled));
                                if b.clicked() {
                                    match press {
                                        0 => pressed.new = true,
                                        1 => pressed.delete = true,
                                        2 => pressed.up = true,
                                        _ => pressed.down = true,
                                    }
                                }
                            }
                        });
                    }
                });
                ui.add_space(10.0);
                ui.vertical(|ui| {
                    ui.set_width(width - list_w - 24.0);
                    let types = Arc::clone(&d.types);
                    let calendars = Arc::clone(&d.calendars);
                    let selected = d.selected;
                    egui::ScrollArea::vertical().id_salt("define_data_boxes").max_height(boxes_h).auto_shrink([false, true]).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        match d.drafts.get_mut(selected) {
                            Some(draft) => field_boxes(ui, &t, draft, (&types, &calendars), &mut seen),
                            None => {
                                ui.label(
                                    RichText::new("This shape has no shape data. New adds a field.").font(theme::font(size::LABEL)).color(t.text_dim),
                                );
                            }
                        }
                    });
                });
            });
            if let Some(error) = &d.error {
                ui.add_space(4.0);
                let said = ui.add(egui::Label::new(RichText::new(error.as_str()).font(theme::font(size::LABEL)).color(t.notice_text)).wrap());
                seen.push(("data.error".to_owned(), said.rect, true));
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                for (label, key, ok) in [("OK", "data.ok", true), ("Cancel", "data.cancel", false)] {
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
        // The engine holds at most 512 fields: so does the draft.
        if pressed.new && d.drafts.len() < MAX_FIELDS {
            d.drafts.push(Draft::made());
            d.selected = d.drafts.len() - 1;
            d.error = None;
        }
        if pressed.delete && d.selected < d.drafts.len() {
            d.drafts.remove(d.selected);
            d.selected = d.selected.min(d.drafts.len().saturating_sub(1));
            d.error = None;
        }
        if (pressed.up && d.selected > 0) || (pressed.down && d.selected + 1 < d.drafts.len()) {
            let to = if pressed.up { d.selected - 1 } else { d.selected + 1 };
            d.drafts.swap(d.selected, to);
            d.selected = to;
            // Every field's sort key is its place: the keys are text, so they are padded to four digits (the dialog's own rule).
            for (n, draft) in d.drafts.iter_mut().enumerate() {
                draft.sort_key = format!("{:04}", n + 1);
            }
            d.error = None;
        }
        self.ui.shape_data.define = Some(d);
        if pressed.cancel {
            self.close_define_shape_data();
        } else if pressed.ok {
            self.apply_define_shape_data();
        }
    }

    /// Asks for the values of the fields of shape `id` that have Ask on drop, when it has any (`shapedata.list`): the card opens over the window.
    pub(crate) fn ask_for_data(&mut self, page: u32, id: u32) {
        if self.ui.shape_data.ask.is_some() {
            return;
        }
        let Ok(answer) = self.session.execute("shapedata.list", &json!({"id": id, "page": page})) else { return };
        let text = |v: &Value, key: &str| v.get(key).and_then(Value::as_str).unwrap_or("").to_owned();
        let rows: Vec<AskRow> = answer
            .get("fields")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter(|f| f.get("ask").and_then(Value::as_bool).unwrap_or(false))
                    .map(|f| AskRow {
                        row: text(f, "row"),
                        label: text(f, "label"),
                        kind: text(f, "type"),
                        items: f
                            .get("items")
                            .and_then(Value::as_array)
                            .map(|i| i.iter().filter_map(|s| s.as_str().map(str::to_owned)).collect())
                            .unwrap_or_default(),
                        prompt: text(f, "prompt"),
                        value: text(f, "shown"),
                        start: text(f, "shown"),
                    })
                    .collect()
            })
            .unwrap_or_default();
        if rows.is_empty() {
            return;
        }
        self.ui.shape_data.ask = Some(AskDialog { page, id, shape: text(&answer, "name"), rows, error: None });
    }

    pub(crate) fn close_ask_data(&mut self) {
        self.ui.shape_data.ask = None;
    }

    /// OK: one `shapedata.set` of the values changed; a refusal is said in the card, which stays.
    fn apply_ask_data(&mut self) {
        let Some(dialog) = self.ui.shape_data.ask.clone() else { return };
        let values: Vec<Value> = dialog.rows.iter().filter(|r| r.value != r.start).map(|r| json!({"row": r.row, "value": r.value})).collect();
        if values.is_empty() {
            self.close_ask_data();
            return;
        }
        match self.execute("shapedata.set", &json!({"id": dialog.id, "page": dialog.page, "values": values})) {
            Ok(_) => self.close_ask_data(),
            Err(e) => {
                if let Some(open) = self.ui.shape_data.ask.as_mut() {
                    open.error = Some(words(&e));
                }
            }
        }
    }

    pub(crate) fn ask_data_dialog(&mut self, ctx: &Context, t: Tokens) {
        let Some(mut d) = self.ui.shape_data.ask.take() else { return };
        let screen = ctx.content_rect();
        let width = (screen.width() - 60.0).clamp(280.0, 420.0);
        let (mut ok, mut cancel) = (false, false);
        let typing = ctx.text_edit_focused();
        let mut seen: Vec<(String, egui::Rect, bool)> = Vec::new();
        let modal = egui::Modal::new(Id::unique("ask_shape_data_dialog")).backdrop_color(t.scrim).frame(Self::card_frame(&t)).show(ctx, |ui| {
            ui.set_width(width);
            let title = if d.shape.is_empty() { "Shape Data".to_owned() } else { format!("Shape Data - {}", diacraft_engine::shown(&d.shape, 40)) };
            ui.label(RichText::new(title).font(theme::font(size::TAB + 1.0)).color(t.text));
            ui.add_space(8.0);
            egui::ScrollArea::vertical().id_salt("ask_data_rows").max_height(280.0).auto_shrink([false, true]).show(ui, |ui| {
                ui.set_width(ui.available_width());
                for row in d.rows.iter_mut() {
                    heading(ui, &t, &row.label);
                    let key = format!("ask.{}", row.row);
                    let choices: Vec<String> = match row.kind.as_str() {
                        "boolean" => vec!["TRUE".to_owned(), "FALSE".to_owned()],
                        "fixed_list" | "variable_list" => row.items.clone(),
                        _ => Vec::new(),
                    };
                    if choices.is_empty() {
                        let out = TextEdit::singleline(&mut row.value)
                            .font(theme::font(size::LABEL))
                            .text_color(t.text)
                            .desired_width(ui.available_width())
                            .char_limit(4_096)
                            .show(ui);
                        out.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, &row.label));
                        let rect = out.response.rect;
                        if !row.prompt.is_empty() {
                            let response: &egui::Response = &out.response;
                            response.clone().on_hover_text(&row.prompt);
                        }
                        seen.push((key, rect, true));
                    } else {
                        let combo = egui::ComboBox::from_id_salt(("ask_data_combo", &row.row))
                            .selected_text(RichText::new(if row.value.is_empty() { " " } else { row.value.as_str() }).font(theme::font(size::LABEL)))
                            .width(ui.available_width() - 8.0);
                        let shown = combo.show_ui(ui, |ui| {
                            for (at, choice) in choices.iter().enumerate() {
                                let resp = ui.selectable_label(*choice == row.value, choice);
                                seen.push((format!("{key}.item.{at}"), resp.rect, true));
                                if resp.clicked() {
                                    row.value.clone_from(choice);
                                }
                            }
                        });
                        shown.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, &row.label));
                        seen.push((key, shown.response.rect, true));
                    }
                    ui.add_space(4.0);
                }
            });
            if let Some(error) = &d.error {
                ui.add_space(4.0);
                let said = ui.add(egui::Label::new(RichText::new(error.as_str()).font(theme::font(size::LABEL)).color(t.notice_text)).wrap());
                seen.push(("ask.error".to_owned(), said.rect, true));
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                for (label, key, is_ok) in [("OK", "ask.ok", true), ("Cancel", "ask.cancel", false)] {
                    let b = ui.add(egui::Button::new(RichText::new(label).font(theme::font(size::LABEL))).min_size(vec2(84.0, 26.0)));
                    seen.push((key.to_owned(), b.rect, true));
                    if b.clicked() {
                        if is_ok { ok = true } else { cancel = true }
                    }
                }
            });
        });
        crate::ask::keep_centred(ctx, modal.response.rect);
        for (name, rect, enabled) in seen {
            self.note(name, rect, enabled);
        }
        if !egui::Popup::is_any_open(ctx) && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            cancel = true;
        }
        if !typing && !egui::Popup::is_any_open(ctx) && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter)) {
            ok = true;
        }
        self.ui.shape_data.ask = Some(d);
        if cancel {
            self.close_ask_data();
        } else if ok {
            self.apply_ask_data();
        }
    }

    /// The dialogs of Shape Data over the window.
    pub(crate) fn shape_data_dialogs(&mut self, ctx: &Context, t: Tokens) {
        if self.ui.shape_data.define.is_some() {
            self.define_data_dialog(ctx, t);
        }
        if self.ui.shape_data.ask.is_some() {
            self.ask_data_dialog(ctx, t);
        }
    }
}

fn at_least_one(d: &DefineDialog) -> bool {
    d.selected < d.drafts.len()
}

/// The boxes of the field the list has chosen, with the types and the calendars the engine lists.
fn field_boxes(ui: &mut Ui, t: &Tokens, draft: &mut Draft, lists: (&[TypeInfo], &[(u32, String)]), seen: &mut Vec<(String, egui::Rect, bool)>) {
    let (types, calendars) = lists;
    edit_row(ui, t, "Label", &mut draft.label, "data.label", seen);
    if draft.row.is_none() {
        edit_row(ui, t, "Name (formulas read it as Prop.<name>; empty makes one of the label)", &mut draft.name, "data.name", seen);
    } else {
        heading(ui, t, "Name");
        let name = format!("Prop.{}", draft.row.as_deref().unwrap_or(""));
        ui.label(RichText::new(name).font(theme::font(size::LABEL)).color(t.text_dim));
        ui.add_space(4.0);
    }
    heading(ui, t, "Type");
    let current = types.iter().find(|ty| ty.id == draft.kind);
    // A type the engine does not list is a `Type` cell that holds another number, or none.
    let unknown = draft.type_code.map_or_else(|| "Unknown".to_owned(), |n| format!("Unknown ({n})"));
    let combo = egui::ComboBox::from_id_salt("define_data_type")
        .selected_text(RichText::new(current.map_or(unknown.as_str(), |ty| ty.label.as_str())).font(theme::font(size::LABEL)))
        .width(ui.available_width() - 8.0);
    let shown = combo.show_ui(ui, |ui| {
        for ty in types {
            let resp = ui.selectable_label(ty.id == draft.kind, &ty.label);
            seen.push((format!("data.type.{}", ty.id), resp.rect, true));
            if resp.clicked() {
                draft.kind.clone_from(&ty.id);
            }
        }
    });
    shown.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, "Type"));
    seen.push(("data.type".to_owned(), shown.response.rect, true));
    ui.add_space(4.0);
    let list = current.is_some_and(|ty| ty.list);
    edit_row(ui, t, if list { "List items, apart at semicolons" } else { "Format" }, &mut draft.format, "data.format", seen);
    if let Some(ty) = current.filter(|ty| !ty.formats.is_empty()) {
        let presets = egui::ComboBox::from_id_salt("define_data_formats")
            .selected_text(RichText::new("Format choices").font(theme::font(size::LABEL)))
            .width(ui.available_width() - 8.0);
        let shown = presets.show_ui(ui, |ui| {
            for (at, (picture, label)) in ty.formats.iter().enumerate() {
                let resp = ui.selectable_label(*picture == draft.format, label);
                seen.push((format!("data.format.{at}"), resp.rect, true));
                if resp.clicked() {
                    draft.format.clone_from(picture);
                }
            }
        });
        shown.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, "Format choices"));
        seen.push(("data.formats".to_owned(), shown.response.rect, true));
        ui.add_space(4.0);
    }
    edit_row(ui, t, "Value", &mut draft.value, "data.value", seen);
    edit_row(ui, t, "Prompt", &mut draft.prompt, "data.prompt", seen);
    edit_row(ui, t, "Sort key", &mut draft.sort_key, "data.sort_key", seen);
    let hidden = ui.checkbox(&mut draft.invisible, RichText::new("Hidden").font(theme::font(size::LABEL)).color(t.text));
    seen.push(("data.hidden".to_owned(), hidden.rect, true));
    let asked = ui.checkbox(&mut draft.ask, RichText::new("Ask on drop").font(theme::font(size::LABEL)).color(t.text));
    seen.push(("data.ask".to_owned(), asked.rect, true));
    ui.add_space(4.0);
    edit_row(ui, t, "Language (a tag such as en-US)", &mut draft.lang_id, "data.lang_id", seen);
    if draft.kind == "date" {
        calendar_box(ui, t, draft, calendars, seen);
    }
}

/// A date field's calendar: a drop-down of the calendars by name; a number the engine does not list says Unknown, and a field with none shows the default.
fn calendar_box(ui: &mut Ui, t: &Tokens, draft: &mut Draft, calendars: &[(u32, String)], seen: &mut Vec<(String, egui::Rect, bool)>) {
    heading(ui, t, "Calendar");
    let named = |n: u32| calendars.iter().find(|(code, _)| *code == n).map(|(_, name)| name.clone());
    let shown = named(draft.calendar.unwrap_or(0)).unwrap_or_else(|| format!("Unknown ({})", draft.calendar.unwrap_or(0)));
    let combo = egui::ComboBox::from_id_salt("define_data_calendar")
        .selected_text(RichText::new(shown).font(theme::font(size::LABEL)))
        .width(ui.available_width() - 8.0);
    let out = combo.show_ui(ui, |ui| {
        for (code, name) in calendars {
            let resp = ui.selectable_label(draft.calendar.unwrap_or(0) == *code, name);
            seen.push((format!("data.calendar.{code}"), resp.rect, true));
            if resp.clicked() {
                draft.calendar = Some(*code);
            }
        }
    });
    out.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, "Calendar"));
    seen.push(("data.calendar".to_owned(), out.response.rect, true));
    ui.add_space(4.0);
}
