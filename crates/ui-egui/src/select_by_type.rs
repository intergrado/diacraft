//! The Select by Type dialog: Home, Editing, Select, Select by Type.
//!
//! **What the specification gives**: a dialog with the types of shape to select, any combination (the page lists "Shapes, Groups, Guides, OLE, Objects, Metafiles, Bitmaps and Ink
//! Objects", read here as seven types), and the layers on which items can be selected, any combination; **OK selects every matching item**. The image's heading names type, role and layer, and
//! the Shape selection tools page says "shapes, connectors or containers", so **the roles Connectors and Containers are a list of their own (ours)**. The captions are ours, from those words.
//!
//! **What the engine says is on offer** (`select.choices`): the seven types and the two roles are always listed, and the layers are those of the page on view. **OK sends one
//! `edit.selectByType`** with what is ticked, which runs the engine's `select.by_type` (the three lists put together by *and*, an empty list asking nothing) and selects the answer; **OK
//! is quiet until something is ticked**, for a dialog that asks nothing selects nothing. Cancel and Escape select nothing, Enter is OK. The window's selection is the one thing it changes.
//!
//! The lists stand side by side when the dialog is wide enough and one under the other when it is not, inside a scrolling area, so that no row is clipped or lies over another.

use diacraft_engine::draw::{SelectKind, SelectRole};
use diacraft_engine::{EngineError, Result};
use egui::{Context, CornerRadius, Id, Key, Modifiers, RichText, Sense, Stroke, StrokeKind, Ui, vec2};
use serde_json::{Map, Value, json};

use crate::app::DiacraftApp;
use crate::theme::{self, Tokens, size};

/// The height of a row of a list, in points.
const ROW_H: f32 = 24.0;

/// The least width, in points, at which the three lists stand side by side.
const BESIDE_FROM: f32 = 470.0;

/// A layer of the page on view, as the dialog lists it.
#[derive(Clone, Debug)]
struct LayerRow {
    ix: u32,
    name: String,
    locked: bool,
    on: bool,
}

/// The Select by Type dialog while it is open.
#[derive(Clone, Debug)]
pub struct SelectByTypeDialog {
    /// The types and the roles, in the order of [`SelectKind::ALL`] and [`SelectRole::ALL`], ticked or not.
    kinds: [bool; 7],
    roles: [bool; 2],
    layers: Vec<LayerRow>,
}

impl SelectByTypeDialog {
    /// Whether anything is ticked: the dialog asks for something.
    fn asks(&self) -> bool {
        self.kinds.iter().chain(self.roles.iter()).any(|on| *on) || self.layers.iter().any(|l| l.on)
    }

    /// The params of the `edit.selectByType` that OK sends.
    fn params(&self) -> Value {
        let types: Vec<&str> = SelectKind::ALL.iter().zip(self.kinds).filter(|(_, on)| *on).map(|(k, _)| k.word()).collect();
        let roles: Vec<&str> = SelectRole::ALL.iter().zip(self.roles).filter(|(_, on)| *on).map(|(r, _)| r.word()).collect();
        let layers: Vec<u32> = self.layers.iter().filter(|l| l.on).map(|l| l.ix).collect();
        json!({"types": types, "roles": roles, "layers": layers})
    }
}

/// What a press in the dialog chose, taken after the card is drawn.
#[derive(Default)]
struct Pressed {
    ok: bool,
    cancel: bool,
}

fn bad(cmd: &str, msg: impl Into<String>) -> EngineError {
    EngineError::BadParams { cmd: cmd.to_owned(), msg: msg.into() }
}

impl DiacraftApp {
    /// The dialog's state for the window state and tests: `null` when it is closed, else what is ticked.
    pub(crate) fn select_by_type_state(&self) -> Value {
        let Some(d) = &self.ui.select_by_type else { return Value::Null };
        let mut map = Map::new();
        let ticked = |words: Vec<&str>| words.into_iter().map(str::to_owned).collect::<Vec<_>>();
        map.insert("types".to_owned(), json!(ticked(SelectKind::ALL.iter().zip(d.kinds).filter(|(_, on)| *on).map(|(k, _)| k.word()).collect())));
        map.insert("roles".to_owned(), json!(ticked(SelectRole::ALL.iter().zip(d.roles).filter(|(_, on)| *on).map(|(r, _)| r.word()).collect())));
        map.insert("layers".to_owned(), json!(d.layers.iter().filter(|l| l.on).map(|l| l.ix).collect::<Vec<_>>()));
        Value::Object(map)
    }

    /// Opens the dialog for the page on view: its layers are asked of the engine (`select.choices`). An open dialog stays as it is.
    pub(crate) fn open_select_by_type(&mut self) -> Result<()> {
        const ID: &str = "window.selectByType";
        if self.ui.select_by_type.is_some() {
            return Ok(());
        }
        let page = self.current_page().ok_or_else(|| bad(ID, "no document open"))?;
        let answer = self.execute("select.choices", &json!({"page": page}))?;
        let layers = answer
            .get("layers")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(|l| {
                        let ix = l.get("ix").and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok())?;
                        let name = l.get("name").and_then(Value::as_str).unwrap_or("").to_owned();
                        Some(LayerRow { ix, name, locked: l.get("locked").and_then(Value::as_bool).unwrap_or(false), on: false })
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.ui.select_by_type = Some(SelectByTypeDialog { kinds: [false; 7], roles: [false; 2], layers });
        Ok(())
    }

    pub(crate) fn close_select_by_type(&mut self) {
        self.ui.select_by_type = None;
    }

    /// `edit.selectByType`: selects the shapes of the page on view that `select.by_type` takes for `p` (the same params), as OK does, with no dialog.
    pub(crate) fn select_by_type_command(&mut self, p: &Map<String, Value>) -> Result<()> {
        let page = self.current_page().ok_or_else(|| bad("edit.selectByType", "no document open"))?;
        let mut params = Value::Object(p.clone());
        if let Some(map) = params.as_object_mut() {
            map.insert("page".to_owned(), json!(page));
        }
        let answer = self.execute("select.by_type", &params)?;
        let ids: Vec<u32> = answer
            .get("ids")
            .and_then(Value::as_array)
            .map(|list| list.iter().filter_map(|v| v.as_u64().and_then(|n| u32::try_from(n).ok())).collect())
            .unwrap_or_default();
        if let Some(unread) = answer.get("unread").and_then(Value::as_u64) {
            self.set_notice(&format!("This page has more shapes than a selection reads; {unread} were left out. The commands still reach them."));
        }
        self.select(&ids);
        Ok(())
    }

    /// OK: one `edit.selectByType` with what is ticked, and the dialog closes.
    fn apply_select_by_type(&mut self) {
        let Some(params) = self.ui.select_by_type.as_ref().filter(|d| d.asks()).map(SelectByTypeDialog::params) else { return };
        self.close_select_by_type();
        if let Err(e) = self.execute("edit.selectByType", &params) {
            self.set_notice(&e.to_string());
        }
    }

    pub(crate) fn select_by_type_dialog(&mut self, ctx: &Context, t: Tokens) {
        let Some(mut dialog) = self.ui.select_by_type.take() else { return };
        let screen = ctx.content_rect();
        let width = (screen.width() - 60.0).clamp(280.0, 600.0);
        let content_h = (screen.height() - 190.0).clamp(140.0, 400.0);
        let mut pressed = Pressed::default();
        let ready = dialog.asks();
        // Enter is OK while no control has the keyboard; one that has it (reached by Tab) takes Enter as a press of its own.
        if !egui::Popup::is_any_open(ctx)
            && ctx.memory(|m| m.focused().is_none())
            && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter))
            && ready
        {
            pressed.ok = true;
        }
        let mut seen: Vec<(String, egui::Rect, bool)> = Vec::new();
        let modal = egui::Modal::new(Id::unique("select_by_type_dialog")).backdrop_color(t.scrim).frame(Self::card_frame(&t)).show(ctx, |ui| {
            ui.set_width(width);
            ui.label(RichText::new("Select by Type").font(theme::font(size::TAB + 1.0)).color(t.text));
            ui.add_space(8.0);
            ui.style_mut().spacing.scroll = egui::style::ScrollStyle::solid();
            egui::ScrollArea::vertical().id_salt("select_by_type_content").max_height(content_h).auto_shrink([false, true]).show(ui, |ui| {
                let inner = ui.available_width();
                ui.set_width(inner);
                lists(ui, &t, &mut dialog, &mut seen);
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                for (label, key, ok) in [("OK", "select_type.ok", true), ("Cancel", "select_type.cancel", false)] {
                    let enabled = !ok || ready;
                    let b =
                        ui.add_enabled(enabled, egui::Button::new(RichText::new(label).font(theme::font(size::LABEL))).min_size(vec2(84.0, 26.0)));
                    seen.push((key.to_owned(), b.rect, enabled));
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
        self.ui.select_by_type = Some(dialog);
        if pressed.cancel {
            self.close_select_by_type();
        } else if pressed.ok {
            self.apply_select_by_type();
        }
    }
}

/// The three lists, side by side when the dialog is wide enough and one under the other when it is not.
fn lists(ui: &mut Ui, t: &Tokens, dialog: &mut SelectByTypeDialog, seen: &mut Vec<(String, egui::Rect, bool)>) {
    let width = ui.available_width();
    if width < BESIDE_FROM {
        ui.vertical(|ui| {
            kinds_list(ui, t, dialog, seen);
            ui.add_space(8.0);
            roles_list(ui, t, dialog, seen);
            ui.add_space(8.0);
            layers_list(ui, t, dialog, seen);
        });
        return;
    }
    let column = (width - 24.0) / 3.0;
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 12.0;
        ui.vertical(|ui| {
            ui.set_width(column);
            kinds_list(ui, t, dialog, seen);
        });
        ui.vertical(|ui| {
            ui.set_width(column);
            roles_list(ui, t, dialog, seen);
        });
        ui.vertical(|ui| {
            ui.set_width(column);
            layers_list(ui, t, dialog, seen);
        });
    });
}

fn heading(ui: &mut Ui, t: &Tokens, words: &str) {
    ui.label(RichText::new(words).font(theme::font(size::LABEL)).color(t.text_dim));
    ui.add_space(2.0);
}

fn kinds_list(ui: &mut Ui, t: &Tokens, dialog: &mut SelectByTypeDialog, seen: &mut Vec<(String, egui::Rect, bool)>) {
    heading(ui, t, "Shape type");
    ui.spacing_mut().item_spacing.y = 2.0;
    for (kind, on) in SelectKind::ALL.iter().zip(dialog.kinds.iter_mut()) {
        if check_row(ui, t, kind.label(), *on, format!("select_type.type.{}", kind.word()), seen) {
            *on = !*on;
        }
    }
}

fn roles_list(ui: &mut Ui, t: &Tokens, dialog: &mut SelectByTypeDialog, seen: &mut Vec<(String, egui::Rect, bool)>) {
    heading(ui, t, "Role");
    ui.spacing_mut().item_spacing.y = 2.0;
    for (role, on) in SelectRole::ALL.iter().zip(dialog.roles.iter_mut()) {
        if check_row(ui, t, role.label(), *on, format!("select_type.role.{}", role.word()), seen) {
            *on = !*on;
        }
    }
}

fn layers_list(ui: &mut Ui, t: &Tokens, dialog: &mut SelectByTypeDialog, seen: &mut Vec<(String, egui::Rect, bool)>) {
    heading(ui, t, "Layer");
    ui.spacing_mut().item_spacing.y = 2.0;
    if dialog.layers.is_empty() {
        ui.label(RichText::new("The page has no layers.").font(theme::font(size::LABEL)).color(t.text_disabled));
        return;
    }
    for layer in &mut dialog.layers {
        let caption = if layer.locked { format!("{} (locked)", layer.name) } else { layer.name.clone() };
        if check_row(ui, t, &caption, layer.on, format!("select_type.layer.{}", layer.ix), seen) {
            layer.on = !layer.on;
        }
    }
}

/// A check box row: ticked or not, as wide as the list. → whether it was pressed. A caption wider than the row is cut at its edge.
fn check_row(ui: &mut Ui, t: &Tokens, label: &str, on: bool, key: String, seen: &mut Vec<(String, egui::Rect, bool)>) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width().max(60.0), ROW_H), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, on, label));
    let painter = ui.painter_at(rect);
    if resp.hovered() {
        painter.rect_filled(rect, CornerRadius::same(3), t.hover);
    }
    if resp.has_focus() {
        painter.rect_stroke(rect.shrink(0.5), CornerRadius::same(3), Stroke::new(1.0, t.accent), StrokeKind::Inside);
    }
    let b = egui::Rect::from_center_size(egui::pos2(rect.left() + 11.0, rect.center().y), vec2(14.0, 14.0));
    painter.rect(b, CornerRadius::same(2), if on { t.accent } else { t.input_fill }, Stroke::new(1.0, t.input_border), StrokeKind::Inside);
    if on {
        let (p, m, q) = (b.left_top() + vec2(3.0, 7.0), b.left_top() + vec2(6.0, 10.0), b.left_top() + vec2(11.0, 4.0));
        painter.line_segment([p, m], Stroke::new(1.6, t.accent_text));
        painter.line_segment([m, q], Stroke::new(1.6, t.accent_text));
    }
    painter.text(egui::pos2(rect.left() + 26.0, rect.center().y), egui::Align2::LEFT_CENTER, label, theme::font(size::LABEL), t.text);
    seen.push((key, rect, true));
    resp.clicked()
}
