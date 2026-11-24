//! The Shape Data window: a pane that lists the fields of the one shape selected, a label at the left and an editor
//! for the field's type at the right.
//!
//! **What it shows**: the title is **Shape Data - <the shape's name>**, and **Shape Data - ThePage** with no shape selected (the page's own shape data is not built, so the body says so). The rows
//! are the engine's `shapedata.list` (the fields in the order they are listed, the hidden ones left out), read again only when the drawing, the page or the shape is not the last answer's. A
//! field's **Prompt** is the tip of its editor, with the label. **The editor follows the type**: a string, number, date, duration and currency field is a text box that shows the value through the
//! field's format and sends what is typed when the person presses Enter or leaves the box (the same machinery as Size & Position's boxes, `fields.rs`), and so is a field of an unknown type (a `Type`
//! cell that holds no number of the eight), whose value is a string's; a date never set (a plain 0) shows nothing; a boolean is a drop-down of TRUE and FALSE;
//! a fixed list is a drop-down of its items; a variable list is a drop-down of its items and **New value...**, which turns the row into a text box for a value the list does not have yet.
//! A value typed is **one `shapedata.set`** (one undo step), and a refusal of the engine is said under the field and in the status bar.
//!
//! **The right-click menu of the pane** offers **Define Shape Data...**, which opens the dialog (`shape_data.rs`); **Shape Data Sets** is not built, so it is not offered. A button at the end of
//! the list does the same.
//!
//! Choices no specification settles (ours): with several shapes selected the pane lists nothing and says to select one (R6 says only that "the pane shows only the selected shape's data"); a
//! label is cut at 12 characters in its column and the tip holds all of it; a shape with more than 512 fields, or text more than a call reads, is refused by the engine and the pane says so in the
//! engine's words.

use std::sync::Arc;

use egui::{RichText, Sense, Ui, vec2};
use serde_json::{Value, json};

use super::fields::{Field, Target, label_column};
use crate::app::DiacraftApp;
use crate::theme::{self, Tokens, size};
use crate::work;

/// A field as the pane shows it: what `shapedata.list` says of it that the editors need.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FieldView {
    pub row: String,
    pub label: String,
    /// The type's word: `string`, `fixed_list`, `number`, `boolean`, `variable_list`, `date`, `duration` or `currency`, or `unknown` for a `Type` cell that holds no number of the eight, whose
    /// value is a string's (a text box).
    pub kind: String,
    pub shown: String,
    pub prompt: String,
    pub items: Vec<String>,
}

/// What `shapedata.list` answered for one shape.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Listing {
    pub name: String,
    pub fields: Vec<FieldView>,
    /// The engine's words when it refused to list the shape.
    pub error: Option<String>,
}

/// What the pane keeps: the last answer and for which drawing, revision, page and shape, and the variable list row that is being typed in.
#[derive(Default)]
pub struct DataCache {
    key: Option<(u64, u64, u32, u32)>,
    read: Option<Arc<Listing>>,
    /// A variable list row turned into a text box for a new value: its page, shape and row.
    typing: Option<(u32, u32, String)>,
}

/// What the selection comes to for the pane.
enum Pick {
    Nothing,
    Many(usize),
    One(u32, u32),
}

/// The most characters of a label the column shows.
const LABEL_CHARS: usize = 12;

fn cut(text: &str, most: usize) -> String {
    if text.chars().count() <= most {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(most.saturating_sub(1)).collect();
    out.push('\u{2026}');
    out
}

fn listing_of(answer: &Value) -> Listing {
    let text = |v: &Value, key: &str| v.get(key).and_then(Value::as_str).unwrap_or("").to_owned();
    let fields = answer
        .get("fields")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .map(|f| FieldView {
                    row: text(f, "row"),
                    label: text(f, "label"),
                    kind: text(f, "type"),
                    shown: text(f, "shown"),
                    prompt: text(f, "prompt"),
                    items: f
                        .get("items")
                        .and_then(Value::as_array)
                        .map(|i| i.iter().filter_map(|s| s.as_str().map(str::to_owned)).collect())
                        .unwrap_or_default(),
                })
                .collect()
        })
        .unwrap_or_default();
    Listing { name: text(answer, "name"), fields, error: None }
}

impl DiacraftApp {
    fn data_pick(&mut self) -> Pick {
        match self.selection_here() {
            None => Pick::Nothing,
            Some((page, ids)) => match ids.as_slice() {
                [only] => Pick::One(page, *only),
                other => Pick::Many(other.len()),
            },
        }
    }

    /// `shapedata.list` for the shape, asked again only when the drawing, the page or the shape is not the last answer's.
    fn data_listing(&mut self, page: u32, id: u32) -> Arc<Listing> {
        let key = (self.ui.generation, self.session.revision(), page, id);
        if self.ui.panes.data.key != Some(key) || self.ui.panes.data.read.is_none() {
            work::count(|w| w.data_queries += 1);
            let read = match self.session.execute("shapedata.list", &json!({"id": id, "page": page})) {
                Ok(answer) => listing_of(&answer),
                Err(e) => Listing { error: Some(e.to_string()), ..Listing::default() },
            };
            self.ui.panes.data.read = Some(Arc::new(read));
            self.ui.panes.data.key = Some(key);
        }
        self.ui.panes.data.read.clone().unwrap_or_default()
    }

    /// The title: "Shape Data - <the shape's name>", "Shape Data - ThePage" with no shape selected, and "Shape Data" for several.
    pub(super) fn shape_data_title(&mut self) -> String {
        match self.data_pick() {
            Pick::One(page, id) => {
                let listing = self.data_listing(page, id);
                match listing.name.as_str() {
                    "" => "Shape Data".to_owned(),
                    name => format!("Shape Data - {}", diacraft_engine::shown(name, 40)),
                }
            }
            Pick::Nothing => "Shape Data - ThePage".to_owned(),
            Pick::Many(_) => "Shape Data".to_owned(),
        }
    }

    pub(super) fn shape_data_body(&mut self, ui: &mut Ui, t: &Tokens) {
        let words = |ui: &mut Ui, text: &str| {
            ui.add(egui::Label::new(RichText::new(text).font(theme::font(size::LABEL)).color(t.text_dim)).wrap());
            ui.add_space(6.0);
        };
        match self.data_pick() {
            Pick::Nothing => {
                words(ui, "Select a shape to see its shape data. The page's own shape data is not shown yet.");
                self.data_menu_area(ui, false);
            }
            Pick::Many(count) => {
                words(ui, &format!("{count} shapes are selected: select one shape to see its shape data."));
                self.data_menu_area(ui, false);
            }
            Pick::One(page, id) => {
                let listing = self.data_listing(page, id);
                if let Some(error) = &listing.error {
                    words(ui, error);
                    return;
                }
                if listing.fields.is_empty() {
                    words(ui, "This shape has no shape data. Define Shape Data adds fields to type values in.");
                }
                let can_set = self.command_enabled("shapedata.set").is_ok();
                for field in &listing.fields {
                    self.data_row(ui, t, page, id, field, can_set);
                }
                self.data_menu_area(ui, true);
            }
        }
    }

    /// The button that opens the Define Shape Data dialog, and the rest of the pane as the place the right-click menu opens on (**Define Shape Data...**).
    fn data_menu_area(&mut self, ui: &mut Ui, one_shape: bool) {
        ui.add_space(6.0);
        let enabled = one_shape && self.command_enabled("window.defineShapeData").is_ok();
        let button = ui.add_enabled(enabled, egui::Button::new(RichText::new("Define Shape Data...").font(theme::font(size::LABEL))));
        crate::widgets::name_button(&button, enabled, "Define Shape Data...");
        self.note("pane.shape_data.define", button.rect, enabled);
        if button.clicked() {
            let _ = self.execute("window.defineShapeData", &json!({}));
        }
        let rest = ui.available_height().clamp(24.0, 400.0);
        let area = ui.allocate_response(vec2(ui.available_width(), rest), Sense::click());
        egui::Popup::context_menu(&area).close_behavior(egui::PopupCloseBehavior::CloseOnClick).show(|ui| {
            ui.set_width(190.0);
            let t = Tokens::get(ui.ctx());
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), if enabled { Sense::click() } else { Sense::hover() });
            crate::widgets::name_button(&resp, enabled, "Define Shape Data...");
            crate::widgets::paint_button_bg(ui, rect, &resp, enabled, false, &t, 3);
            ui.painter().text(
                egui::pos2(rect.left() + 12.0, rect.center().y),
                egui::Align2::LEFT_CENTER,
                "Define Shape Data...",
                theme::font(size::LABEL),
                crate::widgets::label_colour(&t, enabled),
            );
            self.note("pane.shape_data.menu.define", rect, enabled);
            if resp.clicked() && enabled {
                let _ = self.execute("window.defineShapeData", &json!({}));
            }
        });
    }

    /// One field: its label and the editor for its type.
    fn data_row(&mut self, ui: &mut Ui, t: &Tokens, page: u32, id: u32, field: &FieldView, can_set: bool) {
        let label = cut(&field.label, LABEL_CHARS);
        let name = format!("pane.data.{}", field.row);
        let tip = if field.prompt.is_empty() { field.label.clone() } else { format!("{}: {}", field.label, field.prompt) };
        let typing_here = matches!(&self.ui.panes.data.typing, Some((p, i, r)) if (*p, *i) == (page, id) && *r == field.row);
        let combo_kind = matches!(field.kind.as_str(), "boolean" | "fixed_list" | "variable_list");
        if combo_kind && !typing_here {
            self.data_combo(ui, t, (page, id), field, &label, &name, &tip, can_set);
            return;
        }
        let ids = [id];
        let target = Some(Target { page, ids: &ids });
        self.pane_field_tip(
            ui,
            t,
            Field {
                label: &label,
                name: &name,
                shown: Some(field.shown.clone()),
                enabled: can_set,
                why_not: Some("Shape data cannot be entered now"),
                target,
            },
            Some(&tip),
        );
    }

    /// A boolean, a fixed list or a variable list: a drop-down of the choices.
    #[allow(clippy::too_many_arguments)]
    fn data_combo(&mut self, ui: &mut Ui, t: &Tokens, shape: (u32, u32), field: &FieldView, label: &str, name: &str, tip: &str, can_set: bool) {
        let (page, id) = shape;
        // The choices: the text to show and the value to send.
        let mut choices: Vec<(String, String)> = match field.kind.as_str() {
            "boolean" => vec![("TRUE".to_owned(), "true".to_owned()), ("FALSE".to_owned(), "false".to_owned())],
            _ => field.items.iter().map(|i| (i.clone(), i.clone())).collect(),
        };
        if field.kind != "boolean" {
            choices.push(("(blank)".to_owned(), String::new()));
        }
        let mut chosen: Option<String> = None;
        let mut new_value = false;
        ui.horizontal(|ui| {
            label_column(ui, t, label, can_set);
            let width = (ui.available_width() - 6.0).clamp(60.0, 170.0);
            let shown = if field.shown.is_empty() { " ".to_owned() } else { field.shown.clone() };
            let combo = egui::ComboBox::from_id_salt(("shape_data_combo", page, id, &field.row))
                .selected_text(RichText::new(shown).font(theme::font(size::LABEL)).color(t.text))
                .width(width);
            let out = ui.add_enabled_ui(can_set, |ui| {
                combo.show_ui(ui, |ui| {
                    for (at, (text, value)) in choices.iter().enumerate() {
                        let on = *value == field.shown || *text == field.shown;
                        let resp = ui.selectable_label(on, text);
                        self.note(format!("{name}.item.{at}"), resp.rect, true);
                        if resp.clicked() {
                            chosen = Some(value.clone());
                        }
                    }
                    if field.kind == "variable_list" {
                        let resp = ui.selectable_label(false, "New value...");
                        self.note(format!("{name}.new"), resp.rect, true);
                        if resp.clicked() {
                            new_value = true;
                        }
                    }
                })
            });
            let response = out.inner.response.on_hover_text(tip);
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, can_set, &field.label));
            self.note(name.to_owned(), response.rect, can_set);
        });
        if let Some(value) = chosen {
            match self.execute("shapedata.set", &json!({"id": id, "page": page, "row": field.row, "value": value})) {
                Ok(_) => self.field_accepted(),
                Err(e) => self.field_refused(name, crate::shape_data::words(&e), page, &[id]),
            }
        }
        if new_value {
            self.ui.panes.data.typing = Some((page, id, field.row.clone()));
            self.ui.panes.fields.focus_next = Some(name.to_owned());
        }
        if let Some(refusal) = self.pane_field_refusal(name).map(str::to_owned) {
            ui.add(egui::Label::new(RichText::new(refusal).font(theme::font(size::LABEL - 1.0)).color(t.notice_text)).wrap());
        }
    }

    /// A value typed in a field of this pane, sent at the top of a frame (`fields.rs`): one `shapedata.set` for the shape the field showed.
    pub(super) fn send_data_typed(&mut self, typed: super::fields::Typed) {
        let Some(row) = typed.name.strip_prefix("pane.data.").map(str::to_owned) else { return };
        let Some(id) = typed.ids.first().copied().filter(|_| typed.ids.len() == 1) else { return };
        let params = json!({"id": id, "page": typed.page, "row": row, "value": typed.text});
        match self.execute("shapedata.set", &params) {
            Ok(_) => self.field_accepted(),
            Err(e) => self.field_refused(&typed.name, crate::shape_data::words(&e), typed.page, &typed.ids),
        }
        // A value that was typed ends the text box of a variable list, whether it was taken or not.
        if matches!(&self.ui.panes.data.typing, Some((p, i, r)) if (*p, *i) == (typed.page, id) && *r == row) {
            self.ui.panes.data.typing = None;
        }
    }

    /// A drawing replaced the open one: the answer and the text box are forgotten.
    pub(super) fn forget_shape_data_pane(&mut self) {
        self.ui.panes.data = DataCache::default();
    }
}
