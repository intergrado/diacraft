//! Insert, Text, Field: the Field dialog.
//!
//! **The dialog** (the list): a **Category** list (Custom Formula, Date/Time, Document Info, Geometry, Object Info, Page Info, Shape Data, User-defined Cells), the **Field name** list of the category
//! chosen, a **Custom formula** box for the first category, a **Sample data format** that says how the field will look, a **Data Format** list (the pictures `FORMAT` reads, by the kind of
//! the field: dates, numbers or text, and the field's own first) and **OK** and **Cancel**. What the lists hold is the engine's (`text.field_choices`), and the sample is its answer to
//! `text.field_sample`, which changes nothing; **OK sends one `text.insert_field`** (one undo step, "Insert field"). A refusal of the engine (a formula that gives an error, say) is said in the
//! dialog, which stays open.
//!
//! **Where the field goes** ("double-click the shape to open its text and put the cursor where the field goes, then Field"): with a text box open, a text edit ends first and what was typed
//! is kept, the field goes where the caret was, and the box opens again after it; with one shape selected and no box, at the end of its text. Field is greyed out with neither. The dialog is the
//! window command `window.field {close?}`.

use diacraft_engine::{EngineError, Result};
use egui::{Context, Id, Key, Modifiers, RichText, TextEdit, vec2};
use serde_json::{Map, Value, json};

use crate::app::DiacraftApp;
use crate::insert_links::{ROW_H, heading, list_row};
use crate::theme::{self, Tokens, size};

/// A field of a category, as `text.field_choices` lists it.
#[derive(Clone, Debug, PartialEq)]
struct FieldChoice {
    id: String,
    label: String,
    /// `text`, `number`, `date`, `duration` or `currency`: which formats the Data Format list offers.
    kind: String,
    formula: String,
}

/// A category of the dialog.
#[derive(Clone, Debug, PartialEq)]
struct Category {
    id: String,
    label: String,
    fields: Vec<FieldChoice>,
}

/// One format of the Data Format list: the text a `Format` cell holds, and how the list says it.
#[derive(Clone, Debug, PartialEq)]
struct Format {
    picture: String,
    label: String,
}

/// The Field dialog while it is open.
#[derive(Clone, Debug)]
pub struct FieldDialog {
    page: u32,
    id: u32,
    /// The character of the shape's text the field goes before (where the caret was), or the end.
    at: Option<usize>,
    /// A text box was open when the dialog was: it opens again after the field is in.
    was_editing: bool,
    categories: Vec<Category>,
    dates: Vec<Format>,
    numbers: Vec<Format>,
    texts: Vec<Format>,
    cat: usize,
    field: usize,
    /// The Custom formula box.
    formula: String,
    /// The Data Format list's choice: 0 is the field's own format, the others the formats of its kind.
    format: usize,
    /// How the field would look, or why it cannot.
    sample: std::result::Result<String, String>,
    /// What the last OK was refused with.
    error: Option<String>,
    /// The sample is out of date: asked again before the next frame draws.
    stale: bool,
}

impl FieldDialog {
    fn category(&self) -> Option<&Category> {
        self.categories.get(self.cat)
    }

    fn choice(&self) -> Option<&FieldChoice> {
        self.category().and_then(|c| c.fields.get(self.field))
    }

    /// The formats the Data Format list offers for the field chosen.
    fn formats(&self) -> &[Format] {
        match self.choice().map(|c| c.kind.as_str()) {
            Some("date") => &self.dates,
            Some("number" | "currency") => &self.numbers,
            Some("duration") => &[],
            _ => &self.texts,
        }
    }

    /// The params that make the field (and its sample): the category, the field or the formula, and the format when one of the list is chosen.
    fn params(&self) -> Option<Value> {
        let category = self.category()?;
        let mut map = Map::new();
        map.insert("id".to_owned(), json!(self.id));
        map.insert("page".to_owned(), json!(self.page));
        map.insert("category".to_owned(), json!(category.id));
        if category.id == "custom" {
            map.insert("formula".to_owned(), json!(self.formula.trim()));
        } else {
            map.insert("field".to_owned(), json!(self.choice()?.id));
        }
        if self.format > 0
            && let Some(f) = self.formats().get(self.format - 1)
        {
            map.insert("format".to_owned(), json!(f.picture));
        }
        Some(Value::Object(map))
    }
}

fn bad(cmd: &str, msg: impl Into<String>) -> EngineError {
    EngineError::BadParams { cmd: cmd.to_owned(), msg: msg.into() }
}

fn formats_of(list: Option<&Value>) -> Vec<Format> {
    list.and_then(Value::as_array)
        .map(|l| {
            l.iter()
                .filter_map(|f| {
                    Some(Format {
                        picture: f.get("picture")?.as_str()?.to_owned(),
                        label: f.get("label").and_then(Value::as_str).unwrap_or("").to_owned(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

impl DiacraftApp {
    /// The dialog's state for the window state and the tests: `null` when it is closed.
    pub(crate) fn field_state(&self) -> Value {
        let Some(d) = &self.ui.insert.field else { return Value::Null };
        json!({
            "id": d.id,
            "page": d.page,
            "at": d.at,
            "category": d.category().map(|c| c.id.clone()),
            "field": d.choice().map(|c| c.id.clone()),
            "formula": d.formula,
            "format": d.format,
            "sample": d.sample.clone().unwrap_or_default(),
            "sampleError": d.sample.clone().err(),
            "error": d.error,
        })
    }

    /// Whether Field can run now, and if not why: a text box is open, or one shape is selected.
    pub(crate) fn field_enabled(&self) -> std::result::Result<(), String> {
        self.command_enabled("text.insert_field")?;
        if self.ui.edit.editing_text() || self.one_shape_selected().is_some() {
            Ok(())
        } else {
            Err("select one shape, or open its text, to put a field in it".to_owned())
        }
    }

    /// Opens the dialog. A text box that is open ends first and its caret is where the field goes; the engine is asked what the dialog lists (`text.field_choices`).
    pub(crate) fn open_field(&mut self) -> Result<()> {
        const ID: &str = "window.field";
        if self.ui.insert.field.is_some() {
            return Ok(());
        }
        let was_editing = self.ui.edit.editing_text();
        let caret = self.edited_text_selection().map(|(start, _)| start);
        // What was typed is kept: the field goes into the text the engine has.
        self.commit_pending_text();
        let (page, id) = self.one_shape_selected().ok_or_else(|| bad(ID, "select one shape, or open its text, to put a field in it"))?;
        let answer = self.execute("text.field_choices", &json!({"id": id, "page": page}))?;
        let categories: Vec<Category> = answer
            .get("categories")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(|c| {
                        let text = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).map(str::to_owned);
                        Some(Category {
                            id: text(c, "id")?,
                            label: text(c, "label")?,
                            fields: c
                                .get("fields")?
                                .as_array()?
                                .iter()
                                .filter_map(|f| {
                                    Some(FieldChoice {
                                        id: text(f, "id")?,
                                        label: text(f, "label")?,
                                        kind: text(f, "type")?,
                                        formula: text(f, "formula").unwrap_or_default(),
                                    })
                                })
                                .collect(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        if categories.is_empty() {
            return Err(bad(ID, "the engine lists no field categories"));
        }
        let formats = answer.get("formats");
        // Date/Time is the category a person most often wants after the text.
        let cat = categories.iter().position(|c| c.id == "date_time").unwrap_or(0);
        self.ui.insert.field = Some(FieldDialog {
            page,
            id,
            at: caret,
            was_editing,
            categories,
            dates: formats_of(formats.and_then(|f| f.get("date"))),
            numbers: formats_of(formats.and_then(|f| f.get("number"))),
            texts: formats_of(formats.and_then(|f| f.get("text"))),
            cat,
            field: 0,
            formula: String::new(),
            format: 0,
            sample: Err(String::new()),
            error: None,
            stale: true,
        });
        Ok(())
    }

    pub(crate) fn close_field(&mut self) {
        self.ui.insert.field = None;
    }

    /// Asks the engine how the field chosen would look.
    fn refresh_field_sample(&mut self) {
        let Some(params) = self.ui.insert.field.as_ref().and_then(FieldDialog::params) else {
            if let Some(d) = self.ui.insert.field.as_mut() {
                d.sample = Err("this category has no fields on this shape".to_owned());
                d.stale = false;
            }
            return;
        };
        let custom_empty = params.get("formula").and_then(Value::as_str).is_some_and(str::is_empty);
        let sample = if custom_empty {
            Err("type a formula".to_owned())
        } else {
            self.execute("text.field_sample", &params)
                .map(|a| a.get("shown").and_then(Value::as_str).unwrap_or("").to_owned())
                .map_err(|e| e.to_string())
        };
        if let Some(d) = self.ui.insert.field.as_mut() {
            d.sample = sample;
            d.stale = false;
        }
    }

    /// OK: one `text.insert_field`, and the dialog closes; a refusal is said in the dialog, which stays.
    fn apply_field(&mut self) {
        let Some(dialog) = self.ui.insert.field.clone() else { return };
        let Some(mut params) = dialog.params() else { return };
        if let (Some(at), Some(map)) = (dialog.at, params.as_object_mut()) {
            map.insert("at".to_owned(), json!(at));
        }
        match self.execute("text.insert_field", &params) {
            Ok(_) => {
                self.close_field();
                if dialog.was_editing {
                    // The box opens again over the shape, with the caret where it was.
                    let caret = dialog.at.map(|n| (n, n));
                    self.reopen_text(dialog.page, dialog.id, caret);
                }
            }
            Err(e) => {
                if let Some(open) = self.ui.insert.field.as_mut() {
                    open.error = Some(e.to_string());
                }
            }
        }
    }

    pub(crate) fn field_dialog(&mut self, ctx: &Context, t: Tokens) {
        if self.ui.insert.field.as_ref().is_some_and(|d| d.stale) {
            self.refresh_field_sample();
        }
        let Some(mut dialog) = self.ui.insert.field.take() else { return };
        let screen = ctx.content_rect();
        let width = (screen.width() - 60.0).clamp(320.0, 580.0);
        let (mut ok, mut cancel) = (false, false);
        if !egui::Popup::is_any_open(ctx) && ctx.memory(|m| m.focused().is_none()) && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter)) {
            ok = dialog.sample.is_ok();
        }
        let mut seen: Vec<(String, egui::Rect, bool)> = Vec::new();
        let mut changed = false;
        let modal = egui::Modal::new(Id::unique("field_dialog")).backdrop_color(t.scrim).frame(Self::card_frame(&t)).show(ctx, |ui| {
            ui.set_width(width);
            ui.label(RichText::new("Field").font(theme::font(size::TAB + 1.0)).color(t.text));
            ui.add_space(8.0);
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                let column = (width - 12.0) / 2.0;
                ui.vertical(|ui| {
                    ui.set_width(column);
                    heading(ui, &t, "Category");
                    egui::ScrollArea::vertical().id_salt("field_categories").max_height(ROW_H * 8.0 + 8.0).auto_shrink([false, true]).show(
                        ui,
                        |ui| {
                            ui.spacing_mut().item_spacing.y = 2.0;
                            for at in 0..dialog.categories.len() {
                                let (label, key) = (dialog.categories[at].label.clone(), format!("field.category.{}", dialog.categories[at].id));
                                if list_row(ui, &t, &label, at == dialog.cat, key, &mut seen) && at != dialog.cat {
                                    dialog.cat = at;
                                    dialog.field = 0;
                                    dialog.format = 0;
                                    changed = true;
                                }
                            }
                        },
                    );
                });
                ui.vertical(|ui| {
                    ui.set_width(column);
                    heading(ui, &t, "Field name");
                    let listed = dialog.category().map(|c| c.fields.clone()).unwrap_or_default();
                    egui::ScrollArea::vertical().id_salt("field_names").max_height(ROW_H * 8.0 + 8.0).auto_shrink([false, true]).show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        if dialog.category().is_some_and(|c| c.id == "custom") {
                            ui.label(RichText::new("Type a ShapeSheet formula below.").font(theme::font(size::LABEL)).color(t.text_disabled));
                        } else if listed.is_empty() {
                            ui.label(RichText::new("This shape has none.").font(theme::font(size::LABEL)).color(t.text_disabled));
                        }
                        for (at, f) in listed.iter().enumerate() {
                            if list_row(ui, &t, &f.label, at == dialog.field, format!("field.field.{}", f.id), &mut seen) && at != dialog.field {
                                dialog.field = at;
                                dialog.format = 0;
                                changed = true;
                            }
                        }
                    });
                });
            });
            ui.add_space(8.0);
            if dialog.category().is_some_and(|c| c.id == "custom") {
                heading(ui, &t, "Custom formula");
                let out = TextEdit::singleline(&mut dialog.formula)
                    .font(theme::font(size::LABEL))
                    .text_color(t.text)
                    .desired_width(ui.available_width())
                    .char_limit(4_096)
                    .show(ui);
                out.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Custom formula"));
                seen.push(("field.formula".to_owned(), out.response.rect, true));
                changed |= out.response.changed();
                ui.add_space(6.0);
            }
            ui.horizontal(|ui| {
                ui.label(RichText::new("Data Format").font(theme::font(size::LABEL)).color(t.text_dim));
                let formats = dialog.formats().to_vec();
                let shown = if dialog.format == 0 {
                    "Default".to_owned()
                } else {
                    formats.get(dialog.format - 1).map_or_else(String::new, |f| f.label.clone())
                };
                let combo =
                    egui::ComboBox::from_id_salt("field_format").selected_text(RichText::new(shown).font(theme::font(size::LABEL))).width(200.0);
                let out = combo.show_ui(ui, |ui| {
                    if ui.selectable_label(dialog.format == 0, "Default").clicked() {
                        dialog.format = 0;
                        changed = true;
                    }
                    for (at, f) in formats.iter().enumerate() {
                        if ui.selectable_label(dialog.format == at + 1, &f.label).clicked() {
                            dialog.format = at + 1;
                            changed = true;
                        }
                    }
                });
                out.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, "Data Format"));
                seen.push(("field.format".to_owned(), out.response.rect, true));
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Sample data format").font(theme::font(size::LABEL)).color(t.text_dim));
                match &dialog.sample {
                    Ok(text) if text.is_empty() => ui.label(RichText::new("(nothing)").font(theme::font(size::LABEL)).color(t.text_disabled)),
                    Ok(text) => ui.label(RichText::new(text.as_str()).font(theme::font(size::LABEL)).color(t.text)),
                    Err(why) => ui.label(RichText::new(why.as_str()).font(theme::font(size::LABEL)).color(t.notice_text)),
                };
            });
            if let Some(error) = &dialog.error {
                ui.add_space(4.0);
                ui.label(RichText::new(error.as_str()).font(theme::font(size::LABEL)).color(t.notice_text));
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                for (label, key, is_ok) in [("OK", "field.ok", true), ("Cancel", "field.cancel", false)] {
                    let enabled = !is_ok || dialog.sample.is_ok();
                    let b =
                        ui.add_enabled(enabled, egui::Button::new(RichText::new(label).font(theme::font(size::LABEL))).min_size(vec2(84.0, 26.0)));
                    seen.push((key.to_owned(), b.rect, enabled));
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
        if changed {
            dialog.stale = true;
            dialog.error = None;
        }
        self.ui.insert.field = Some(dialog);
        if cancel {
            self.close_field();
        } else if ok {
            self.apply_field();
        }
    }
}
