//! The Layer Properties and Assign to Layer dialogs: Home > Editing > Layers (ribbon map `LayersMenu`,
//! `LayerPropertiesDialog` and `LayerDialog`).
//!
//! **Layer Properties**: a list of the page's layers with the columns **Name**, **#** (the shapes on the layer), **Visible**, **Print**, **Active**, **Lock**, **Snap**,
//! **Glue** and **Color** (the last seven check boxes), the buttons **New...**, **Remove** and **Rename...**, a **Layer color** row of swatches (the first sixteen colours of
//! the document's table, and None) with a **Transparency** slider and percent box, a box **Remove unreferenced layers**, and **Apply**, **OK** and **Cancel**.
//!
//! * **The dialog works on a draft**, as the Page Setup and Snap & Glue dialogs do: nothing is sent until Apply or OK, and then **one `command.batch`** (one undo step, "Layer
//!   properties") runs, in this order, `layer.delete` for each layer removed, `layer.set` for each layer changed, `layer.add` for each layer made, and `layer.delete
//!   {unreferenced}` when the box is ticked. Layers are named by their names, which are unique on a page, so the removals do not move the numbers the rest are found by.
//!   Cancel and Escape send nothing.
//! * **Remove** on a layer that has shapes asks first (R22 says the desktop deletes them; the project asks, the project rule): *Delete the layer and its shapes*
//!   or *Delete the layer, keep the shapes*. A layer with no shape goes at once. **New...** opens a name box under the list; **Rename...** opens the selected row's name for
//!   typing. A name the engine refuses (empty, repeated, too long, one of the five names the OpenDocument format reserves) is said in the dialog, which stays open.
//! * **A locked layer cannot be active**: ticking Lock clears Active in the draft, and the Active box of a locked row is greyed out.
//! * **The Color box** is on when the layer has a colour: ticking it gives the layer the colour of the swatch last chosen (the table's second colour, by default), clearing
//!   it is None (255). **Transparency** is 0 to 100 percent and is kept as the cell's share (`ColorTrans`).
//!
//! **Assign to Layer**: a list **On layer(s):** of the page's layers, a box for each, ticked for a layer every shape selected is on, clear for one none is on, and
//! **mixed** (a dash) for a layer only some are on; a press on a box turns it on, or off when it was on. Buttons **All**, **None** and **New...**, a box **Preserve group
//! member layers** (ticked: a group's own layers change and its members keep theirs), **OK** and **Cancel**. OK sends one `command.batch` (one undo step, "Assign to layer"):
//! `layer.add` for each layer made, then `layer.assign` (`mode: add`) for the boxes turned on and (`mode: remove`) for those turned off, so a mixed box left alone changes
//! nothing. A locked layer's box is greyed out: a locked layer takes no new shapes.

use diacraft_engine::doc::Owner;
use diacraft_engine::draw::layers::{Layer, MAX_NAME_CHARS, page_layers, shape_layers};
use egui::{Context, CornerRadius, Id, Key, Modifiers, RichText, Sense, Stroke, StrokeKind, Ui, vec2};
use serde_json::{Map, Value, json};

use crate::app::DiacraftApp;
use crate::theme::{self, Tokens, size};

/// The swatches the Layer color row offers: the document's colour table from 0 up, this many.
const SWATCHES: i64 = 16;

/// The colour a layer is given when its Color box is ticked and no swatch was chosen: the table's second colour is white, the third red.
const DEFAULT_SWATCH: i64 = 2;

/// The state of both dialogs.
#[derive(Default)]
pub struct LayersState {
    pub properties: Option<PropertiesDialog>,
    pub assign: Option<AssignDialog>,
}

/// A row of the Layer Properties list: a layer of the page as it was, or one made in the dialog.
#[derive(Clone, Debug, PartialEq)]
struct Row {
    was: Option<Layer>,
    name: String,
    visible: bool,
    print: bool,
    active: bool,
    lock: bool,
    snap: bool,
    glue: bool,
    colour: Option<(u8, u8, u8)>,
    transparency: f64,
}

impl Row {
    fn of(layer: &Layer) -> Row {
        Row {
            was: Some(layer.clone()),
            name: layer.name.clone(),
            visible: layer.visible,
            print: layer.print,
            active: layer.active,
            lock: layer.lock,
            snap: layer.snap,
            glue: layer.glue,
            colour: layer.colour,
            transparency: layer.transparency,
        }
    }

    fn made(name: &str) -> Row {
        Row {
            was: None,
            name: name.to_owned(),
            visible: true,
            print: true,
            active: false,
            lock: false,
            snap: true,
            glue: true,
            colour: None,
            transparency: 0.0,
        }
    }

    /// The shapes on the layer (0 for a layer made here).
    fn shapes(&self) -> usize {
        self.was.as_ref().map_or(0, |l| l.shapes)
    }

    fn colour_text(colour: Option<(u8, u8, u8)>) -> String {
        colour.map_or_else(|| "none".to_owned(), |(r, g, b)| format!("#{r:02x}{g:02x}{b:02x}"))
    }

    /// The `layer.set` this row needs, or `None` when nothing of it changed.
    fn set_params(&self, page: u32) -> Option<Value> {
        let was = self.was.as_ref()?;
        let mut out = Map::new();
        if self.name != was.name {
            out.insert("name".into(), json!(self.name));
        }
        for (key, now, before) in [
            ("visible", self.visible, was.visible),
            ("print", self.print, was.print),
            ("active", self.active, was.active),
            ("lock", self.lock, was.lock),
            ("snap", self.snap, was.snap),
            ("glue", self.glue, was.glue),
        ] {
            if now != before {
                out.insert(key.into(), json!(now));
            }
        }
        if self.colour != was.colour {
            out.insert("color".into(), json!(Row::colour_text(self.colour)));
        }
        if (self.transparency - was.transparency).abs() > 1.0e-9 {
            out.insert("transparency".into(), json!(self.transparency));
        }
        if out.is_empty() {
            return None;
        }
        out.insert("page".into(), json!(page));
        out.insert("layer".into(), json!(was.name));
        Some(Value::Object(out))
    }

    /// The `layer.add` of a layer made in the dialog.
    fn add_params(&self, page: u32) -> Value {
        let mut out = Map::new();
        out.insert("page".into(), json!(page));
        out.insert("name".into(), json!(self.name));
        for (key, on, default) in [
            ("visible", self.visible, true),
            ("print", self.print, true),
            ("active", self.active, false),
            ("lock", self.lock, false),
            ("snap", self.snap, true),
            ("glue", self.glue, true),
        ] {
            if on != default {
                out.insert(key.into(), json!(on));
            }
        }
        if self.colour.is_some() {
            out.insert("color".into(), json!(Row::colour_text(self.colour)));
        }
        if self.transparency > 0.0 {
            out.insert("transparency".into(), json!(self.transparency));
        }
        Value::Object(out)
    }
}

/// The Layer Properties dialog while it is open.
pub struct PropertiesDialog {
    page: u32,
    rows: Vec<Row>,
    /// The layers of the page taken out in the dialog: their names, and whether their shapes go too.
    removed: Vec<(String, bool)>,
    selected: Option<usize>,
    renaming: Option<usize>,
    /// The name box of New..., when it is open.
    adding: Option<String>,
    /// The row Remove asked about, while its question is on.
    confirm: Option<usize>,
    unreferenced: bool,
    /// The swatch last chosen, for the Color box of a row that has none.
    swatch: Option<(u8, u8, u8)>,
    error: Option<String>,
}

impl PropertiesDialog {
    fn new(page: u32, layers: &[Layer]) -> PropertiesDialog {
        PropertiesDialog {
            page,
            rows: layers.iter().map(Row::of).collect(),
            removed: Vec::new(),
            selected: None,
            renaming: None,
            adding: None,
            confirm: None,
            unreferenced: false,
            swatch: None,
            error: None,
        }
    }

    /// The commands OK sends, in order (see the module documentation).
    fn commands(&self) -> Vec<(&'static str, Value)> {
        let mut out: Vec<(&'static str, Value)> = Vec::new();
        for (name, shapes) in &self.removed {
            out.push(("layer.delete", json!({"page": self.page, "layer": name, "shapes": if *shapes { "delete" } else { "keep" }})));
        }
        for row in &self.rows {
            if let Some(params) = row.set_params(self.page) {
                out.push(("layer.set", params));
            }
        }
        for row in self.rows.iter().filter(|r| r.was.is_none()) {
            out.push(("layer.add", row.add_params(self.page)));
        }
        if self.unreferenced {
            out.push(("layer.delete", json!({"page": self.page, "unreferenced": true})));
        }
        out
    }

    /// Takes row `at` out, with its shapes or without them.
    fn remove(&mut self, at: usize, with_shapes: bool) {
        if at >= self.rows.len() {
            return;
        }
        let row = self.rows.remove(at);
        if let Some(was) = row.was {
            self.removed.push((was.name, with_shapes));
        }
        self.selected = None;
        self.renaming = None;
        self.confirm = None;
    }

    fn add(&mut self, name: &str) {
        self.rows.push(Row::made(name.trim()));
        self.selected = Some(self.rows.len().saturating_sub(1));
        self.adding = None;
    }

    /// The first colour of the table that the swatch row would choose, for a Color box that is ticked with none chosen.
    fn colour_for_box(&self, table: &[(u8, u8, u8)]) -> Option<(u8, u8, u8)> {
        self.swatch.or_else(|| table.get(usize::try_from(DEFAULT_SWATCH).unwrap_or(0)).copied())
    }
}

/// The tri-state of a box of Assign to Layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tri {
    Off,
    On,
    Mixed,
}

#[derive(Clone, Debug, PartialEq)]
struct AssignRow {
    /// The layer's number, or `None` for one made in the dialog.
    ix: Option<u32>,
    name: String,
    locked: bool,
    start: Tri,
    now: Tri,
}

/// The Assign to Layer dialog while it is open.
pub struct AssignDialog {
    page: u32,
    ids: Vec<u32>,
    rows: Vec<AssignRow>,
    adding: Option<String>,
    preserve: bool,
    error: Option<String>,
}

impl AssignDialog {
    /// The commands OK sends: the layers made, then the assignment that turns boxes on and the one that turns them off.
    fn commands(&self) -> Vec<(&'static str, Value)> {
        let mut out: Vec<(&'static str, Value)> = Vec::new();
        for row in self.rows.iter().filter(|r| r.ix.is_none()) {
            out.push(("layer.add", json!({"page": self.page, "name": row.name})));
        }
        let named = |want: Tri| -> Vec<String> { self.rows.iter().filter(|r| r.now == want && r.now != r.start).map(|r| r.name.clone()).collect() };
        for (mode, layers) in [("add", named(Tri::On)), ("remove", named(Tri::Off))] {
            if !layers.is_empty() {
                out.push((
                    "layer.assign",
                    json!({"page": self.page, "ids": self.ids, "layers": layers, "mode": mode, "preserve_members": self.preserve}),
                ));
            }
        }
        out
    }

    fn cycle(&mut self, at: usize) {
        if let Some(row) = self.rows.get_mut(at) {
            row.now = if row.now == Tri::On { Tri::Off } else { Tri::On };
        }
    }
}

impl DiacraftApp {
    /// Whether the Layer Properties dialog is open.
    pub fn layer_properties_open(&self) -> bool {
        self.ui.layers.properties.is_some()
    }

    /// Whether the Assign to Layer dialog is open.
    pub fn assign_layer_open(&self) -> bool {
        self.ui.layers.assign.is_some()
    }

    /// The names of the rows the Layer Properties dialog shows now, in order (for tests and agents).
    pub fn layer_properties_names(&self) -> Vec<String> {
        self.ui.layers.properties.as_ref().map(|d| d.rows.iter().map(|r| r.name.clone()).collect()).unwrap_or_default()
    }

    /// What the Assign to Layer dialog shows now: each layer's name and box (for tests and agents).
    pub fn assign_layer_boxes(&self) -> Vec<(String, Tri)> {
        self.ui.layers.assign.as_ref().map(|d| d.rows.iter().map(|r| (r.name.clone(), r.now)).collect()).unwrap_or_default()
    }

    /// The text OK was refused with, while a layers dialog is open.
    pub fn layers_dialog_error(&self) -> Option<&str> {
        self.ui
            .layers
            .properties
            .as_ref()
            .and_then(|d| d.error.as_deref())
            .or_else(|| self.ui.layers.assign.as_ref().and_then(|d| d.error.as_deref()))
    }

    /// Opens Layer Properties on the page on view.
    pub(crate) fn open_layer_properties(&mut self) -> diacraft_engine::Result<()> {
        if self.ui.layers.properties.is_some() {
            return Ok(());
        }
        let Some(page) = self.current_page() else {
            return Err(diacraft_engine::EngineError::Failed { cmd: "window.layerProperties".to_owned(), msg: "no document open".to_owned() });
        };
        let layers = self.layers_of(page)?;
        self.ui.layers.properties = Some(PropertiesDialog::new(page, &layers));
        Ok(())
    }

    fn layers_of(&self, page: u32) -> diacraft_engine::Result<Vec<Layer>> {
        let doc = self
            .session()
            .document()
            .ok_or_else(|| diacraft_engine::EngineError::Failed { cmd: "layer.list".to_owned(), msg: "no document open".to_owned() })?;
        page_layers(doc, page).map_err(|p| p.into_engine("layer.list"))
    }

    /// Opens Assign to Layer for the shapes selected.
    pub(crate) fn open_assign_layer(&mut self) -> diacraft_engine::Result<()> {
        if self.ui.layers.assign.is_some() {
            return Ok(());
        }
        let Some(page) = self.current_page() else {
            return Err(diacraft_engine::EngineError::Failed { cmd: "window.assignLayer".to_owned(), msg: "no document open".to_owned() });
        };
        let ids: Vec<u32> = self.ui.edit.selection().to_vec();
        let layers = self.layers_of(page)?;
        let doc = self
            .session()
            .document()
            .ok_or_else(|| diacraft_engine::EngineError::Failed { cmd: "window.assignLayer".to_owned(), msg: "no document open".to_owned() })?;
        let lists: Vec<Vec<u32>> = ids
            .iter()
            .filter_map(|id| doc.find_shape(Owner::Page(page), *id).and_then(|at| doc.shape(&at)).map(|shape| shape_layers(shape)))
            .collect();
        let rows = layers
            .iter()
            .map(|l| {
                let on = lists.iter().filter(|list| list.contains(&l.ix)).count();
                let start = if on == 0 {
                    Tri::Off
                } else if on == lists.len() {
                    Tri::On
                } else {
                    Tri::Mixed
                };
                AssignRow { ix: Some(l.ix), name: l.name.clone(), locked: l.lock, start, now: start }
            })
            .collect();
        self.ui.layers.assign = Some(AssignDialog { page, ids, rows, adding: None, preserve: true, error: None });
        Ok(())
    }

    pub(crate) fn close_layer_properties(&mut self) {
        self.ui.layers.properties = None;
    }

    pub(crate) fn close_assign_layer(&mut self) {
        self.ui.layers.assign = None;
    }

    /// Sends what the Layer Properties dialog changed. `close` ends the dialog after (OK), else it starts again from the page as it is (Apply). A refusal is said in the dialog.
    fn apply_layer_properties(&mut self, close: bool) {
        let Some(dialog) = self.ui.layers.properties.as_ref() else { return };
        let (page, commands) = (dialog.page, dialog.commands());
        let result = match commands.as_slice() {
            [] => Ok(()),
            [(id, params)] => self.execute(id, params).map(|_| ()),
            many => {
                let steps: Vec<Value> = many.iter().map(|(id, params)| json!({"command": id, "params": params})).collect();
                self.execute("command.batch", &json!({"commands": steps, "label": "Layer properties"})).map(|_| ())
            }
        };
        match result {
            Ok(()) if close => self.close_layer_properties(),
            Ok(()) => {
                if let Ok(layers) = self.layers_of(page) {
                    self.ui.layers.properties = Some(PropertiesDialog::new(page, &layers));
                }
            }
            Err(e) => {
                if let Some(d) = self.ui.layers.properties.as_mut() {
                    d.error = Some(e.to_string());
                }
            }
        }
    }

    fn apply_assign_layer(&mut self) {
        let Some(dialog) = self.ui.layers.assign.as_ref() else { return };
        let commands = dialog.commands();
        let result = match commands.as_slice() {
            [] => Ok(()),
            [(id, params)] => self.execute(id, params).map(|_| ()),
            many => {
                let steps: Vec<Value> = many.iter().map(|(id, params)| json!({"command": id, "params": params})).collect();
                self.execute("command.batch", &json!({"commands": steps, "label": "Assign to layer"})).map(|_| ())
            }
        };
        match result {
            Ok(()) => self.close_assign_layer(),
            Err(e) => {
                if let Some(d) = self.ui.layers.assign.as_mut() {
                    d.error = Some(e.to_string());
                }
            }
        }
    }

    /// The first [`SWATCHES`] colours of the document's table.
    fn swatch_table(&self) -> Vec<(u8, u8, u8)> {
        let Some(doc) = self.session().document() else { return Vec::new() };
        (0..SWATCHES).filter_map(|i| doc.color(i)).map(|c| (c.r, c.g, c.b)).collect()
    }

    /// The dialogs of the Layers menu over the window.
    pub(crate) fn layers_dialogs(&mut self, ctx: &Context, t: Tokens) {
        if self.ui.layers.properties.is_some() {
            self.properties_dialog(ctx, t);
        }
        if self.ui.layers.assign.is_some() {
            self.assign_dialog(ctx, t);
        }
    }

    fn properties_dialog(&mut self, ctx: &Context, t: Tokens) {
        let screen = ctx.content_rect();
        let width = (screen.width() - 60.0).clamp(320.0, 680.0);
        let list_h = (screen.height() - 330.0).clamp(90.0, 230.0);
        let table = self.swatch_table();
        let Some(mut d) = self.ui.layers.properties.take() else { return };
        let (mut ok, mut apply, mut cancel) = (false, false, false);
        let typing = ctx.text_edit_focused();
        let mut seen: Vec<(String, egui::Rect, bool)> = Vec::new();
        let modal = egui::Modal::new(Id::unique("layer_properties_dialog")).backdrop_color(t.scrim).frame(Self::card_frame(&t)).show(ctx, |ui| {
            ui.set_width(width);
            ui.label(RichText::new("Layer Properties").font(theme::font(size::TAB + 1.0)).color(t.text));
            ui.add_space(6.0);
            let scroll = egui::ScrollArea::vertical()
                .id_salt("layer_properties_list")
                .min_scrolled_height(list_h)
                .max_height(list_h)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    properties_list(ui, &t, &mut d, &table, &mut seen);
                });
            seen.push(("layers.list".to_owned(), scroll.inner_rect, true));
            ui.add_space(6.0);
            properties_controls(ui, &t, &mut d, &table, &mut seen);
            if let Some(error) = &d.error {
                ui.add_space(4.0);
                let said = ui.add(egui::Label::new(RichText::new(error).font(theme::font(size::LABEL)).color(t.notice_text)).wrap());
                seen.push(("layers.error".to_owned(), said.rect, true));
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                for (label, key) in [("OK", "layers.ok"), ("Apply", "layers.apply"), ("Cancel", "layers.cancel")] {
                    let b = ui.add(egui::Button::new(RichText::new(label).font(theme::font(size::LABEL))).min_size(vec2(84.0, 26.0)));
                    seen.push((key.to_owned(), b.rect, true));
                    if b.clicked() {
                        match key {
                            "layers.ok" => ok = true,
                            "layers.apply" => apply = true,
                            _ => cancel = true,
                        }
                    }
                }
            });
        });
        crate::ask::keep_centred(ctx, modal.response.rect);
        for (name, rect, enabled) in seen {
            self.note(name, rect, enabled);
        }
        if !egui::Popup::is_any_open(ctx) && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            // Escape ends the question or the typing before it ends the dialog.
            if d.confirm.is_some() {
                d.confirm = None;
            } else if d.adding.is_some() {
                d.adding = None;
            } else if d.renaming.is_some() {
                d.renaming = None;
            } else {
                cancel = true;
            }
        }
        if !typing && !egui::Popup::is_any_open(ctx) && d.confirm.is_none() && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter)) {
            ok = true;
        }
        self.ui.layers.properties = Some(d);
        if cancel {
            self.close_layer_properties();
        } else if ok {
            self.apply_layer_properties(true);
        } else if apply {
            self.apply_layer_properties(false);
        }
    }

    fn assign_dialog(&mut self, ctx: &Context, t: Tokens) {
        let screen = ctx.content_rect();
        let width = (screen.width() - 60.0).clamp(280.0, 400.0);
        let list_h = (screen.height() - 300.0).clamp(90.0, 240.0);
        let Some(mut d) = self.ui.layers.assign.take() else { return };
        let (mut ok, mut cancel) = (false, false);
        let typing = ctx.text_edit_focused();
        let mut seen: Vec<(String, egui::Rect, bool)> = Vec::new();
        let modal = egui::Modal::new(Id::unique("assign_layer_dialog")).backdrop_color(t.scrim).frame(Self::card_frame(&t)).show(ctx, |ui| {
            ui.set_width(width);
            ui.label(RichText::new("Layer").font(theme::font(size::TAB + 1.0)).color(t.text));
            ui.add_space(6.0);
            ui.label(RichText::new("On layer(s):").font(theme::font(size::LABEL)).color(t.text_dim));
            let scroll = egui::ScrollArea::vertical()
                .id_salt("assign_layer_list")
                .min_scrolled_height(list_h)
                .max_height(list_h)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing.y = 2.0;
                    for at in 0..d.rows.len() {
                        let Some(row) = d.rows.get(at).cloned() else { continue };
                        let name = format!("assign.layer.{at}");
                        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
                        let enabled = !row.locked;
                        resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, enabled, row.now == Tri::On, &row.name));
                        let painter = ui.painter_at(rect);
                        if resp.hovered() && enabled {
                            painter.rect_filled(rect, CornerRadius::same(3), t.hover);
                        }
                        paint_tick(
                            &painter,
                            &t,
                            egui::Rect::from_center_size(egui::pos2(rect.left() + 11.0, rect.center().y), vec2(14.0, 14.0)),
                            tri_of(row.now),
                            enabled,
                        );
                        let label = if row.locked { format!("{} (locked)", row.name) } else { row.name.clone() };
                        painter.text(
                            egui::pos2(rect.left() + 26.0, rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            label,
                            theme::font(size::LABEL),
                            if enabled { t.text } else { t.text_dim },
                        );
                        seen.push((name, rect, enabled));
                        if resp.clicked() && enabled {
                            d.cycle(at);
                        }
                    }
                });
            seen.push(("assign.list".to_owned(), scroll.inner_rect, true));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                for (label, key) in [("All", "assign.all"), ("None", "assign.none"), ("New...", "assign.new")] {
                    let b = ui.add(egui::Button::new(RichText::new(label).font(theme::font(size::LABEL))).min_size(vec2(64.0, 24.0)));
                    seen.push((key.to_owned(), b.rect, true));
                    if b.clicked() {
                        match key {
                            "assign.all" => d.rows.iter_mut().filter(|r| !r.locked).for_each(|r| r.now = Tri::On),
                            "assign.none" => d.rows.iter_mut().filter(|r| !r.locked || r.start != Tri::Off).for_each(|r| r.now = Tri::Off),
                            _ => d.adding = Some(String::new()),
                        }
                    }
                }
            });
            let mut made: Option<String> = None;
            if let Some(name) = d.adding.as_mut() {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Layer name").font(theme::font(size::LABEL)).color(t.text_dim));
                    let edit =
                        ui.add(egui::TextEdit::singleline(name).desired_width(150.0).char_limit(MAX_NAME_CHARS).font(theme::font(size::LABEL)));
                    let edit = edit.accessible_name("New layer name");
                    seen.push(("assign.new_name".to_owned(), edit.rect, true));
                    edit.request_focus();
                    let add = ui.add(egui::Button::new(RichText::new("Add").font(theme::font(size::LABEL))));
                    seen.push(("assign.add".to_owned(), add.rect, true));
                    if add.clicked() || (edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter))) {
                        made = Some(name.clone());
                    }
                });
            }
            if let Some(name) = made {
                d.adding = None;
                if !name.trim().is_empty() {
                    d.rows.push(AssignRow { ix: None, name: name.trim().to_owned(), locked: false, start: Tri::Off, now: Tri::On });
                }
            }
            ui.add_space(4.0);
            let r = ui.checkbox(&mut d.preserve, RichText::new("Preserve group member layers").font(theme::font(size::LABEL)).color(t.text));
            seen.push(("assign.preserve".to_owned(), r.rect, true));
            if let Some(error) = &d.error {
                ui.add_space(4.0);
                let said = ui.add(egui::Label::new(RichText::new(error).font(theme::font(size::LABEL)).color(t.notice_text)).wrap());
                seen.push(("assign.error".to_owned(), said.rect, true));
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                for (label, key) in [("OK", "assign.ok"), ("Cancel", "assign.cancel")] {
                    let b = ui.add(egui::Button::new(RichText::new(label).font(theme::font(size::LABEL))).min_size(vec2(84.0, 26.0)));
                    seen.push((key.to_owned(), b.rect, true));
                    if b.clicked() {
                        if key == "assign.ok" { ok = true } else { cancel = true }
                    }
                }
            });
        });
        crate::ask::keep_centred(ctx, modal.response.rect);
        for (name, rect, enabled) in seen {
            self.note(name, rect, enabled);
        }
        if !egui::Popup::is_any_open(ctx) && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            if d.adding.is_some() {
                d.adding = None;
            } else {
                cancel = true;
            }
        }
        if !typing && !egui::Popup::is_any_open(ctx) && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter)) {
            ok = true;
        }
        self.ui.layers.assign = Some(d);
        if cancel {
            self.close_assign_layer();
        } else if ok {
            self.apply_assign_layer();
        }
    }
}

fn tri_of(t: Tri) -> Option<bool> {
    match t {
        Tri::On => Some(true),
        Tri::Off => Some(false),
        Tri::Mixed => None,
    }
}

/// A check box drawn as the other dialogs' are: filled with a tick when on, a dash when mixed (`None`).
fn paint_tick(painter: &egui::Painter, t: &Tokens, b: egui::Rect, on: Option<bool>, enabled: bool) {
    let filled = on != Some(false);
    let fill = if filled && enabled { t.accent } else { t.input_fill };
    painter.rect(b, CornerRadius::same(2), fill, Stroke::new(1.0, t.input_border), StrokeKind::Inside);
    let mark = if enabled { t.accent_text } else { t.text_dim };
    match on {
        Some(true) => {
            let (a, m, z) = (b.left_top() + vec2(3.0, 7.0), b.left_top() + vec2(6.0, 10.0), b.left_top() + vec2(11.0, 4.0));
            painter.line_segment([a, m], Stroke::new(1.6, mark));
            painter.line_segment([m, z], Stroke::new(1.6, mark));
        }
        None => {
            painter.line_segment([b.left_center() + vec2(3.0, 0.0), b.right_center() - vec2(3.0, 0.0)], Stroke::new(1.6, mark));
        }
        Some(false) => {}
    }
}

/// A box of a row: a press turns it, unless it is greyed out. → whether it was pressed.
fn tick(ui: &mut Ui, t: &Tokens, on: bool, enabled: bool, label: &str, name: String, seen: &mut Vec<(String, egui::Rect, bool)>) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, enabled, on, label));
    let painter = ui.painter_at(rect);
    if resp.hovered() && enabled {
        painter.rect_filled(rect, CornerRadius::same(3), t.hover);
    }
    paint_tick(&painter, t, egui::Rect::from_center_size(rect.center(), vec2(14.0, 14.0)), Some(on), enabled);
    seen.push((name, rect, enabled));
    enabled && resp.clicked()
}

fn swatch(colour: (u8, u8, u8)) -> egui::Color32 {
    theme::document_colour(colour.0, colour.1, colour.2)
}

/// The columns of the list: the row is the name (a press selects it), the number of shapes, and the seven boxes.
fn properties_list(ui: &mut Ui, t: &Tokens, d: &mut PropertiesDialog, table: &[(u8, u8, u8)], seen: &mut Vec<(String, egui::Rect, bool)>) {
    ui.spacing_mut().item_spacing = vec2(4.0, 2.0);
    egui::Grid::new("layer_properties_grid").num_columns(9).min_col_width(26.0).spacing(vec2(6.0, 2.0)).show(ui, |ui| {
        for head in ["Name", "#", "Visible", "Print", "Active", "Lock", "Snap", "Glue", "Color"] {
            ui.label(RichText::new(head).font(theme::font(size::LABEL)).color(t.text_dim));
        }
        ui.end_row();
        for at in 0..d.rows.len() {
            let Some(mut row) = d.rows.get(at).cloned() else { continue };
            let selected = d.selected == Some(at);
            if d.renaming == Some(at) {
                let edit =
                    ui.add(egui::TextEdit::singleline(&mut row.name).desired_width(140.0).char_limit(MAX_NAME_CHARS).font(theme::font(size::LABEL)));
                let edit = edit.accessible_name("Layer name");
                seen.push((format!("layers.row.{at}.name_edit"), edit.rect, true));
                edit.request_focus();
                if edit.lost_focus() {
                    d.renaming = None;
                }
            } else {
                let r = ui.add_sized(
                    [140.0, 22.0],
                    egui::Button::selectable(selected, RichText::new(&row.name).font(theme::font(size::LABEL)).color(t.text)),
                );
                seen.push((format!("layers.row.{at}.name"), r.rect, true));
                if r.clicked() {
                    d.selected = Some(at);
                }
                if r.double_clicked() {
                    d.selected = Some(at);
                    d.renaming = Some(at);
                }
            }
            ui.label(RichText::new(row.shapes().to_string()).font(theme::font(size::LABEL)).color(t.text_dim));
            let mut flags = [row.visible, row.print, row.active, row.lock, row.snap, row.glue];
            for (k, key) in ["visible", "print", "active", "lock", "snap", "glue"].into_iter().enumerate() {
                let enabled = !(key == "active" && row.lock);
                let on = flags.get(k).copied().unwrap_or(false);
                if tick(ui, t, on, enabled, &format!("{} {key}", row.name), format!("layers.row.{at}.{key}"), seen)
                    && let Some(flag) = flags.get_mut(k)
                {
                    *flag = !*flag;
                    d.selected = Some(at);
                }
            }
            // A locked layer cannot be active: locking clears Active.
            let [visible, print, active, lock, snap, glue] = flags;
            row.visible = visible;
            row.print = print;
            row.snap = snap;
            row.glue = glue;
            if lock && !row.lock {
                row.active = false;
            } else {
                row.active = active;
            }
            row.lock = lock;
            let coloured = row.colour.is_some();
            if tick(ui, t, coloured, true, &format!("{} color", row.name), format!("layers.row.{at}.color"), seen) {
                row.colour = if coloured { None } else { d.colour_for_box(table) };
                d.selected = Some(at);
            }
            // The row as the dialog now has it; a rename in the box is kept as it is typed.
            if let Some(slot) = d.rows.get_mut(at) {
                *slot = row;
            }
            ui.end_row();
        }
    });
}

/// New..., Remove, Rename..., the Layer color row, Transparency, the unreferenced box, and the question Remove asks.
fn properties_controls(ui: &mut Ui, t: &Tokens, d: &mut PropertiesDialog, table: &[(u8, u8, u8)], seen: &mut Vec<(String, egui::Rect, bool)>) {
    ui.horizontal(|ui| {
        let has_selection = d.selected.is_some_and(|i| i < d.rows.len());
        for (label, key, enabled) in
            [("New...", "layers.new", true), ("Remove", "layers.remove", has_selection), ("Rename...", "layers.rename", has_selection)]
        {
            let b = ui.add_enabled(enabled, egui::Button::new(RichText::new(label).font(theme::font(size::LABEL))).min_size(vec2(84.0, 24.0)));
            seen.push((key.to_owned(), b.rect, enabled));
            if !b.clicked() {
                continue;
            }
            match key {
                "layers.new" => d.adding = Some(String::new()),
                "layers.rename" => d.renaming = d.selected,
                _ => {
                    if let Some(at) = d.selected {
                        match d.rows.get(at).map(Row::shapes) {
                            Some(0) | None => d.remove(at, false),
                            Some(_) => d.confirm = Some(at),
                        }
                    }
                }
            }
        }
    });
    if let Some(at) = d.confirm {
        let (name, shapes) = d.rows.get(at).map(|r| (r.name.clone(), r.shapes())).unwrap_or_default();
        ui.add_space(4.0);
        ui.add(
            egui::Label::new(
                RichText::new(format!("Layer \"{name}\" has {shapes} shape(s) on it.")).font(theme::font(size::LABEL)).color(t.notice_text),
            )
            .wrap(),
        );
        ui.horizontal(|ui| {
            for (label, key) in [
                ("Delete the layer and its shapes", "layers.remove_with_shapes"),
                ("Delete the layer, keep the shapes", "layers.remove_keep"),
                ("Cancel", "layers.remove_cancel"),
            ] {
                let b = ui.add(egui::Button::new(RichText::new(label).font(theme::font(size::LABEL))));
                seen.push((key.to_owned(), b.rect, true));
                if b.clicked() {
                    match key {
                        "layers.remove_with_shapes" => d.remove(at, true),
                        "layers.remove_keep" => d.remove(at, false),
                        _ => d.confirm = None,
                    }
                }
            }
        });
    }
    let mut made: Option<String> = None;
    if let Some(name) = d.adding.as_mut() {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Layer name").font(theme::font(size::LABEL)).color(t.text_dim));
            let edit = ui.add(egui::TextEdit::singleline(name).desired_width(170.0).char_limit(MAX_NAME_CHARS).font(theme::font(size::LABEL)));
            let edit = edit.accessible_name("New layer name");
            seen.push(("layers.new_name".to_owned(), edit.rect, true));
            edit.request_focus();
            let add = ui.add(egui::Button::new(RichText::new("Add").font(theme::font(size::LABEL))));
            seen.push(("layers.add".to_owned(), add.rect, true));
            if add.clicked() || (edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter))) {
                made = Some(name.clone());
            }
        });
    }
    if let Some(name) = made {
        if name.trim().is_empty() {
            d.adding = None;
        } else {
            d.add(&name);
        }
    }
    ui.add_space(6.0);
    let selected = d.selected.filter(|i| *i < d.rows.len());
    ui.horizontal(|ui| {
        ui.label(RichText::new("Layer color").font(theme::font(size::LABEL)).color(if selected.is_some() { t.text } else { t.text_dim }));
        let current = selected.and_then(|i| d.rows.get(i)).and_then(|r| r.colour);
        let (rect, resp) = ui.allocate_exact_size(vec2(36.0, 20.0), Sense::click());
        let none = ui.painter_at(rect);
        none.rect(
            rect,
            CornerRadius::same(3),
            t.input_fill,
            Stroke::new(1.0, if current.is_none() { t.accent } else { t.input_border }),
            StrokeKind::Inside,
        );
        none.text(rect.center(), egui::Align2::CENTER_CENTER, "None", theme::font(size::LABEL - 2.0), t.text_dim);
        seen.push(("layers.colour.none".to_owned(), rect, selected.is_some()));
        if resp.clicked()
            && let Some(row) = selected.and_then(|i| d.rows.get_mut(i))
        {
            row.colour = None;
        }
        for (k, c) in table.iter().enumerate() {
            let (rect, resp) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
            let p = ui.painter_at(rect);
            let chosen = current == Some(*c);
            p.rect(
                rect.shrink(1.0),
                CornerRadius::same(2),
                swatch(*c),
                Stroke::new(if chosen { 2.0 } else { 1.0 }, if chosen { t.accent } else { t.input_border }),
                StrokeKind::Inside,
            );
            seen.push((format!("layers.colour.{k}"), rect, selected.is_some()));
            if resp.clicked()
                && let Some(row) = selected.and_then(|i| d.rows.get_mut(i))
            {
                row.colour = Some(*c);
                d.swatch = Some(*c);
            }
        }
    });
    ui.horizontal(|ui| {
        ui.label(RichText::new("Transparency").font(theme::font(size::LABEL)).color(if selected.is_some() { t.text } else { t.text_dim }));
        let mut percent = selected.and_then(|i| d.rows.get(i)).map_or(0.0, |r| r.transparency * 100.0);
        let enabled = selected.is_some();
        let slider = ui.add_enabled(enabled, egui::Slider::new(&mut percent, 0.0..=100.0).show_value(false).integer());
        slider.widget_info(|| egui::WidgetInfo::slider(enabled, percent, "Layer transparency".to_owned()));
        let slider = slider.accessible_name("Layer transparency");
        seen.push(("layers.transparency".to_owned(), slider.rect, enabled));
        let mut changed = slider.changed();
        let typed = ui.add_enabled(enabled, egui::DragValue::new(&mut percent).range(0.0..=100.0).suffix("%").fixed_decimals(0));
        let typed = typed.accessible_name("Layer transparency in percent");
        seen.push(("layers.transparency_box".to_owned(), typed.rect, enabled));
        changed |= typed.changed();
        if changed && let Some(row) = selected.and_then(|i| d.rows.get_mut(i)) {
            row.transparency = (percent / 100.0).clamp(0.0, 1.0);
        }
    });
    let r = ui.checkbox(&mut d.unreferenced, RichText::new("Remove unreferenced layers").font(theme::font(size::LABEL)).color(t.text));
    seen.push(("layers.unreferenced".to_owned(), r.rect, true));
}
