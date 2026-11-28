//! The cards of the ShapeSheet window: **Show Sections**, **Insert Section** and **Edit Formula**.
//!
//! **Show Sections** is a list of check boxes, one for each section the sheet shows, which hides any of them. Its state is the tab's own and is not saved: all the boxes are ticked at first.
//! A press on a box sends `window.sheetSections` with the names of the sections that are then ticked, so that the window command is the one way the state changes (a person, a program and a
//! test reach the same code). The card stays open while the boxes are ticked and unticked, and closes with a press outside it.
//!
//! **Insert Section** is a card of check boxes, one for each section a sheet can take (the sections of rows, Text Transform and Layer Membership, and Geometry in its three kinds,
//! Ellipse and Infinite Line too), in the window's order, in two columns where the card is wide enough. **The boxes the sheet cannot take are greyed** and say why in their tooltip: the
//! sheet has the section already (a section other than Geometry is there once), or it belongs to another kind of sheet. What is on offer is what `sheet.sections` says (`can_add`); OK adds
//! the ticked ones with **one `command.batch` of `sheet.add_section`**, which is one undo step, and a refusal of the engine is said in the card, which stays. Escape is Cancel and Enter is OK.
//!
//! **Edit Formula** is a card with a text area of several lines (the formula bar shows one), OK and Cancel, for a long formula or one with line breaks. **It is another place the
//! one edit of the cell is held in**, as the bar is: the edit is the tab's, flagged `in_card`, so that the commit is the commit of Enter (one `sheet.set_cell` with `force`, the fault marked and
//! the engine's words in the card when it is refused) and the live value is the bar's. Enter adds a line, **Ctrl+Enter is OK**, Escape is Cancel. Opened over an edit that was open, Cancel gives
//! the cell's editor the text it had.

use std::sync::Arc;

use diacraft_engine::doc::{DISPLAY_ORDER, SheetKind};
use diacraft_engine::sheetview::Place;
use diacraft_engine::{Result, shown};
use egui::text::LayoutJob;
use egui::{
    Align2, Color32, Context, CornerRadius, FontId, Galley, Id, Key, Modifiers, Rect, RichText, Role, Sense, Stroke, StrokeKind, TextBuffer,
    TextEdit, TextFormat, Ui, WidgetInfo, pos2, vec2,
};
use serde_json::{Map, Value, json};

use super::edit::{Begin, MAX_EDIT_CHARS, Then, edit_id, refusal_words};
use super::rows::Op;
use super::view::Sel;
use super::{bad, flag, place_params};
use crate::app::DiacraftApp;
use crate::ribbon::controls::submenu_height;
use crate::theme::{self, Tokens, size};
use crate::widgets::{self, paint_button_bg};

/// The room a row has for its box, in points, and the height of a row.
const ROW_H: f32 = 24.0;

/// The most sections `window.sheetInsertSection` is asked for at once (the sheet has fewer kinds than this).
const MAX_SECTIONS_NAMED: usize = 64;

// ---- Show Sections ------------------------------------------------------------------------------------------------------------------

impl DiacraftApp {
    /// The rows of the card: the name of each section the active sheet shows, its label and whether it is ticked.
    pub(crate) fn sheet_sections_rows(&self) -> Vec<(String, String, bool)> {
        let sheet = &self.ui.sheet;
        let Some(tab) = sheet.tabs.get(sheet.active) else { return Vec::new() };
        let Some(view) = tab.view.as_ref() else { return Vec::new() };
        view.base().sections.iter().map(|s| (s.name.clone(), s.label.clone(), !tab.hidden.contains(&s.name))).collect()
    }
}

/// The body of the Show Sections menu: a box for each section, in the sheet's order.
pub fn sections_menu(app: &mut DiacraftApp, ui: &mut Ui, t: &Tokens) {
    let rows = app.sheet_sections_rows();
    ui.set_min_width(220.0);
    if rows.is_empty() {
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::hover());
        let words = "No ShapeSheet is open";
        resp.widget_info(|| WidgetInfo::labeled(Role::Label, true, words));
        ui.painter().text(pos2(rect.left() + 12.0, rect.center().y), Align2::LEFT_CENTER, words, theme::font(size::LABEL), t.text_dim);
        return;
    }
    let height = submenu_height(ui.ctx().content_rect().height());
    let mut toggled: Option<String> = None;
    egui::ScrollArea::vertical().max_height(height).auto_shrink([true, true]).show(ui, |ui| {
        for (name, label, shown) in &rows {
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width().max(200.0), ROW_H), Sense::click());
            widgets::name_check(&resp, true, *shown, label);
            paint_button_bg(ui, rect, &resp, true, false, t, 3);
            paint_box(ui, t, rect, *shown, true);
            ui.painter().text(pos2(rect.left() + 30.0, rect.center().y), Align2::LEFT_CENTER, label, theme::font(size::LABEL), t.text);
            app.note(format!("sheet.sections.{name}"), rect, true);
            if resp.clicked() {
                toggled = Some(name.clone());
            }
        }
    });
    if let Some(name) = toggled {
        let shown: Vec<&str> = rows.iter().filter(|(n, _, on)| if *n == name { !*on } else { *on }).map(|(n, _, _)| n.as_str()).collect();
        let _ = app.execute("window.sheetSections", &json!({ "shown": shown }));
    }
}

/// A check box at the left of a row: ticked or clear, and quiet when the row cannot be used.
pub(super) fn paint_box(ui: &Ui, t: &Tokens, row: Rect, ticked: bool, enabled: bool) {
    let b = Rect::from_center_size(pos2(row.left() + 12.0, row.center().y), vec2(13.0, 13.0));
    let painter = ui.painter();
    let (fill, edge) = match (ticked, enabled) {
        (true, true) => (t.accent, t.input_border),
        (_, true) => (t.input_fill, t.input_border),
        (_, false) => (t.control_idle_fill, t.separator),
    };
    painter.rect(b, CornerRadius::same(2), fill, Stroke::new(1.0, edge), StrokeKind::Inside);
    if ticked && enabled {
        let (a, m, z) = (b.left_top() + vec2(2.5, 6.5), b.left_top() + vec2(5.5, 9.5), b.left_top() + vec2(10.5, 3.5));
        painter.line_segment([a, m], Stroke::new(1.6, t.accent_text));
        painter.line_segment([m, z], Stroke::new(1.6, t.accent_text));
    }
}

// ---- Insert Section -----------------------------------------------------------------------------------------------------------------

/// Whether a box of the Insert Section card can be ticked, and if not why.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Offer {
    /// `sheet.sections` offers it.
    Offered,
    /// The sheet has the section already (a section other than Geometry is there once).
    Has,
    /// This kind of sheet does not take it (a shape's sections go on a shape's sheet).
    Elsewhere,
}

impl Offer {
    pub fn word(self) -> &'static str {
        match self {
            Offer::Offered => "offered",
            Offer::Has => "has",
            Offer::Elsewhere => "elsewhere",
        }
    }

    fn why(self) -> &'static str {
        match self {
            Offer::Offered => "",
            Offer::Has => "This sheet has it already: a section other than Geometry is there once.",
            Offer::Elsewhere => "This kind of sheet does not take it: it goes on another kind of sheet.",
        }
    }
}

/// A box of the Insert Section card.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Item {
    /// What the card shows (`Scratch`, `Shape Data`, `Ellipse`).
    pub label: String,
    /// The name `sheet.add_section` takes (`Scratch`, `Prop`, `Geometry`), empty for a box that is not on offer.
    pub section: String,
    /// A Geometry section's kind: `Ellipse` or `InfiniteLine`.
    pub variant: Option<String>,
    pub offer: Offer,
    pub ticked: bool,
}

/// The Insert Section card while it is open.
#[derive(Clone, Debug)]
pub(crate) struct InsertCard {
    /// The sheet the sections go on, whatever tab is on top when OK is pressed.
    pub place: Place,
    pub items: Vec<Item>,
    /// What the engine refused the last OK with, said in the card.
    pub error: Option<String>,
}

/// What `sheet.sections` offers: `(label, section, variant)` for each entry of its `can_add`.
fn offers_of(answer: &Value) -> Vec<(String, String, Option<String>)> {
    let text = |o: &Value, key: &str| o.get(key).and_then(Value::as_str).map(str::to_owned);
    answer
        .get("can_add")
        .and_then(Value::as_array)
        .map(|list| list.iter().filter_map(|o| Some((text(o, "label")?, text(o, "section")?, text(o, "variant")))).collect())
        .unwrap_or_default()
}

/// The boxes of the card for a sheet of `kind`, in the window's order: every section some sheet can take, a Geometry section in its three kinds, each on offer when `offers` has it.
pub(crate) fn items_of(kind: SheetKind, offers: &[(String, String, Option<String>)]) -> Vec<Item> {
    let mut items = Vec::new();
    for entry in DISPLAY_ORDER.iter().filter(|d| d.rules.insertable) {
        let labels: &[&str] = if entry.element == Some("Geometry") { &["Geometry", "Ellipse", "Infinite Line"] } else { &[entry.label] };
        for label in labels {
            let item = match offers.iter().find(|(l, _, _)| l == label) {
                Some((_, section, variant)) => {
                    Item { label: (*label).to_owned(), section: section.clone(), variant: variant.clone(), offer: Offer::Offered, ticked: false }
                }
                None => {
                    let offer = if entry.rules.can_insert_on(kind) { Offer::Has } else { Offer::Elsewhere };
                    Item { label: (*label).to_owned(), section: String::new(), variant: None, offer, ticked: false }
                }
            };
            items.push(item);
        }
    }
    items
}

impl DiacraftApp {
    /// Whether a card of the sheet is open: the Insert Section card, or the Edit Formula card over the tab on top. Both are modal.
    pub(crate) fn sheet_card_open(&self) -> bool {
        self.ui.sheet.insert.is_some() || self.formula_card_open()
    }

    /// The window state's account of the card that is open: `null` when none is.
    pub(crate) fn sheet_card_json(&self) -> Value {
        if let Some(card) = &self.ui.sheet.insert {
            let items: Vec<Value> = card
                .items
                .iter()
                .map(|i| json!({"label": i.label, "section": i.section, "variant": i.variant, "state": i.offer.word(), "ticked": i.ticked}))
                .collect();
            return json!({"kind": "insertSection", "items": items, "error": card.error});
        }
        match self.ui.sheet.tabs.get(self.ui.sheet.active).and_then(|t| t.edit.as_ref()).filter(|e| e.in_card) {
            Some(e) => json!({"kind": "editFormula", "cell": e.reference, "text": e.text, "refusal": e.refusal}),
            None => Value::Null,
        }
    }

    /// The boxes of the card for the sheet `place`, from `sheet.sections`, a query: what is on offer is the engine's to say.
    fn sheet_insert_items(&mut self, place: &Place) -> Result<Vec<Item>> {
        let answer = self.session.execute("sheet.sections", &place_params(place))?;
        Ok(items_of(place.kind(), &offers_of(&answer)))
    }

    /// Opens the Insert Section card on the sheet on top.
    fn sheet_open_insert_card(&mut self, id: &str) -> Result<()> {
        // An open card stays as it is, with its boxes ticked.
        if self.ui.sheet.insert.is_some() {
            return Ok(());
        }
        let (place, _) = self.sheet_ready(id, Op::InsertSection)?;
        let items = self.sheet_insert_items(&place)?;
        self.ui.sheet.insert = Some(InsertCard { place, items, error: None });
        Ok(())
    }

    /// Adds the sections `chosen` to the sheet `place` with one `command.batch` of `sheet.add_section`: one undo step, all or nothing. The first section added is selected.
    fn sheet_insert_sections(&mut self, place: &Place, chosen: &[Item]) -> Result<()> {
        let commands: Vec<Value> = chosen
            .iter()
            .map(|item| {
                let mut params = place_params(place);
                if let Some(map) = params.as_object_mut() {
                    map.insert("section".to_owned(), json!(item.section));
                    if let Some(variant) = &item.variant {
                        map.insert("variant".to_owned(), json!(variant));
                    }
                }
                json!({"command": "sheet.add_section", "params": params})
            })
            .collect();
        let label = if chosen.len() == 1 { "Insert section" } else { "Insert sections" };
        let answer = self.sheet_run("command.batch", &json!({"commands": commands, "label": label}))?;
        let first = answer.get("results").and_then(Value::as_array).and_then(|r| r.first()).and_then(|r| r.get("section")).and_then(Value::as_str);
        let names: Vec<&str> = chosen.iter().map(|i| i.label.as_str()).collect();
        let on_top = self.ui.sheet.tabs.get(self.ui.sheet.active).is_some_and(|t| t.place == *place);
        if let (true, Some(first)) = (on_top, first) {
            self.sheet_show(Some(Sel::Header { section: first.to_owned() }), format!("Inserted {}.", names.join(", ")));
        }
        Ok(())
    }

    /// `window.sheetInsertSection`: with `sections` those sections are inserted with no card (a program does what a person does), `close` closes the card, and with neither the card opens.
    pub(crate) fn sheet_insert_section_command(&mut self, id: &str, p: &Map<String, Value>) -> Result<()> {
        if flag(id, p, "close")?.unwrap_or(false) {
            if p.get("sections").is_some_and(|s| !s.is_null()) {
                return Err(bad(id, "`close` goes alone"));
            }
            if self.ui.sheet.insert.take().is_none() {
                return Err(bad(id, "the Insert Section card is not open"));
            }
            return Ok(());
        }
        let names: Vec<&str> = match p.get("sections") {
            None | Some(Value::Null) => return self.sheet_open_insert_card(id),
            Some(Value::Array(list)) => {
                if list.len() > MAX_SECTIONS_NAMED {
                    return Err(bad(id, format!("`sections` names at most {MAX_SECTIONS_NAMED} sections")));
                }
                let mut names = Vec::new();
                for name in list {
                    names.push(name.as_str().ok_or_else(|| bad(id, "`sections` must be an array of section labels"))?);
                }
                names
            }
            Some(_) => return Err(bad(id, "`sections` must be an array of section labels")),
        };
        let (place, _) = self.sheet_ready(id, Op::InsertSection)?;
        let items = self.sheet_insert_items(&place)?;
        let mut chosen: Vec<Item> = Vec::new();
        for name in names {
            let found = items
                .iter()
                .find(|i| i.label.eq_ignore_ascii_case(name.trim()) || (!i.section.is_empty() && i.section.eq_ignore_ascii_case(name.trim())));
            match found {
                Some(item) if item.offer == Offer::Offered => {
                    if !chosen.contains(item) {
                        chosen.push(item.clone());
                    }
                }
                Some(item) => return Err(bad(id, format!("`{}`: {}", shown(name, 48), item.offer.why()))),
                None => {
                    return Err(bad(id, format!("`{}` is not a section this sheet can take: sheet.sections lists them in can_add", shown(name, 48))));
                }
            }
        }
        if chosen.is_empty() {
            return Err(bad(id, "`sections` names no section"));
        }
        self.sheet_insert_sections(&place, &chosen)
    }

    /// OK on the Insert Section card: the ticked boxes are inserted, and the card closes; a refusal is said in the card, which stays.
    fn sheet_apply_insert_card(&mut self) {
        let Some(card) = self.ui.sheet.insert.clone() else { return };
        let chosen: Vec<Item> = card.items.iter().filter(|i| i.ticked && i.offer == Offer::Offered).cloned().collect();
        if chosen.is_empty() {
            return;
        }
        match self.sheet_insert_sections(&card.place, &chosen) {
            Ok(()) => self.ui.sheet.insert = None,
            Err(error) => {
                if let Some(open) = self.ui.sheet.insert.as_mut() {
                    open.error = Some(refusal_words(&error));
                }
            }
        }
    }

    /// The cards of the sheet over the window (called with the other dialogs).
    pub(crate) fn sheet_cards(&mut self, ctx: &Context, t: Tokens) {
        if self.ui.sheet.insert.is_some() {
            self.sheet_insert_card(ctx, t);
        }
        if self.formula_card_open() {
            self.sheet_formula_card(ctx, t);
        }
    }

    fn sheet_insert_card(&mut self, ctx: &Context, t: Tokens) {
        let Some(mut card) = self.ui.sheet.insert.take() else { return };
        let screen = ctx.content_rect();
        let width = (screen.width() - 60.0).clamp(280.0, 460.0);
        let two = width >= 400.0;
        let column = if two { (width - 12.0) / 2.0 } else { width };
        let list_h = (screen.height() - 190.0).clamp(96.0, 520.0);
        let (mut ok, mut cancel) = (false, false);
        let mut seen: Vec<(String, Rect, bool)> = Vec::new();
        let any = card.items.iter().any(|i| i.ticked);
        // Enter is OK when a box is ticked and no text box has the keyboard, as in the window's other dialogs.
        let free = !egui::Popup::is_any_open(ctx) && ctx.memory(|m| m.focused().is_none());
        if !egui::Popup::is_any_open(ctx) {
            ctx.input_mut(|i| {
                cancel |= i.consume_key(Modifiers::NONE, Key::Escape);
                ok |= any && free && i.consume_key(Modifiers::NONE, Key::Enter);
            });
        }
        let modal = egui::Modal::new(Id::unique("sheet_insert_section")).backdrop_color(t.scrim).frame(Self::card_frame(&t)).show(ctx, |ui| {
            ui.set_width(width);
            ui.label(RichText::new("Insert Section").font(theme::font(size::TAB + 1.0)).color(t.text));
            ui.add_space(6.0);
            egui::ScrollArea::vertical().id_salt("sheet_insert_list").max_height(list_h).auto_shrink([false, true]).show(ui, |ui| {
                let per = if two { card.items.len().div_ceil(2) } else { card.items.len() }.max(1);
                ui.horizontal_top(|ui| {
                    for chunk in card.items.chunks_mut(per) {
                        ui.vertical(|ui| {
                            ui.set_width(column);
                            for item in chunk {
                                item_row(ui, &t, item, column, &mut seen);
                            }
                        });
                    }
                });
            });
            if let Some(error) = &card.error {
                ui.add_space(4.0);
                let label = ui.add(egui::Label::new(RichText::new(error.as_str()).font(theme::font(size::LABEL)).color(t.sheet_error)).wrap());
                seen.push(("sheet.card.insert.error".to_owned(), label.rect, false));
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let button = ui.add_enabled(any, egui::Button::new(RichText::new("OK").font(theme::font(size::LABEL))).min_size(vec2(84.0, 26.0)));
                seen.push(("sheet.card.insert.ok".to_owned(), button.rect, any));
                ok |= button.clicked();
                let button = ui.add(egui::Button::new(RichText::new("Cancel").font(theme::font(size::LABEL))).min_size(vec2(84.0, 26.0)));
                seen.push(("sheet.card.insert.cancel".to_owned(), button.rect, true));
                cancel |= button.clicked();
            });
        });
        crate::ask::keep_centred(ctx, modal.response.rect);
        for (name, rect, enabled) in seen {
            self.note(name, rect, enabled);
        }
        self.ui.sheet.insert = Some(card);
        if cancel {
            self.ui.sheet.insert = None;
        } else if ok {
            self.sheet_apply_insert_card();
        }
    }
}

/// A box of the Insert Section card: its tick, its label, greyed with the reason in its tooltip when the sheet cannot take the section. Only a box on view is recorded as drawn.
fn item_row(ui: &mut Ui, t: &Tokens, item: &mut Item, width: f32, seen: &mut Vec<(String, Rect, bool)>) {
    let on = item.offer == Offer::Offered;
    let (rect, resp) = ui.allocate_exact_size(vec2(width, ROW_H), if on { Sense::click() } else { Sense::hover() });
    widgets::name_check(&resp, on, item.ticked, &item.label);
    if on {
        paint_button_bg(ui, rect, &resp, true, false, t, 3);
    }
    paint_box(ui, t, rect, item.ticked, on);
    let galley = super::cell::cut(ui, &item.label, widgets::label_colour(t, on), width - 32.0);
    ui.painter().with_clip_rect(Rect::from_min_max(pos2(rect.left() + 30.0, rect.top()), rect.max)).galley(
        pos2(rect.left() + 30.0, rect.center().y - galley.size().y / 2.0),
        galley,
        widgets::label_colour(t, on),
    );
    if ui.is_rect_visible(rect) {
        seen.push((format!("sheet.card.insert.{}", item.label), rect, on));
    }
    if resp.clicked() && on {
        item.ticked = !item.ticked;
    }
    if !on {
        resp.on_hover_text(item.offer.why());
    }
}

// ---- Edit Formula -------------------------------------------------------------------------------------------------------------------

/// The text laid out in `wrap` points, with the fault underlined in the error colour.
fn fault_layout(ui: &Ui, text: &str, wrap: f32, look: (&FontId, Color32, Color32), fault: Option<(usize, usize)>) -> Arc<Galley> {
    let (font, ink, error) = look;
    let mut job = LayoutJob::default();
    let plain = TextFormat::simple(font.clone(), ink);
    let marked = TextFormat { underline: Stroke::new(1.5, error), ..TextFormat::simple(font.clone(), ink) };
    let range = fault.filter(|(a, b)| a < b && text.is_char_boundary(*a) && text.is_char_boundary(*b) && *b <= text.len());
    match range {
        Some((a, b)) => {
            job.append(text.get(..a).unwrap_or(""), 0.0, plain.clone());
            job.append(text.get(a..b).unwrap_or(""), 0.0, marked);
            job.append(text.get(b..).unwrap_or(""), 0.0, plain);
        }
        None => job.append(text, 0.0, plain),
    }
    job.wrap.max_width = wrap;
    ui.fonts_mut(|f| f.layout_job(job))
}

impl DiacraftApp {
    /// Whether the Edit Formula card is open: the edit of the tab on top is held in it.
    pub(crate) fn formula_card_open(&self) -> bool {
        self.ui.sheet.tabs.get(self.ui.sheet.active).and_then(|t| t.edit.as_ref()).is_some_and(|e| e.in_card)
    }

    /// Opens the Edit Formula card on the selected cell: the edit that is open moves into it (Cancel gives it back), else an edit of the cell's formula begins there.
    pub(crate) fn sheet_open_formula_card(&mut self) -> std::result::Result<(), String> {
        let active = self.ui.sheet.active;
        let began = self.ui.sheet.tabs.get(active).is_some_and(|t| t.edit.is_none());
        if began {
            self.sheet_begin_edit(Begin::Text { in_bar: false })?;
        }
        let edit = self.ui.sheet.tabs.get_mut(active).and_then(|t| t.edit.as_mut()).ok_or_else(|| "select a cell to edit its formula".to_owned())?;
        if !edit.in_card {
            edit.card_from = if began { None } else { Some(edit.text.clone()) };
            edit.in_card = true;
            edit.in_bar = false;
            edit.fresh = true;
        }
        Ok(())
    }

    /// Cancel on the Edit Formula card: an edit the card began is given up; one it moved into goes back to the cell's editor with the text it had.
    pub(crate) fn sheet_card_cancel(&mut self) {
        let Some(tab) = self.ui.sheet.tabs.get_mut(self.ui.sheet.active) else { return };
        let Some(edit) = tab.edit.as_mut() else { return };
        match edit.card_from.take() {
            Some(text) => {
                edit.set_text(text);
                edit.text_changed();
                edit.in_card = false;
                edit.fresh = true;
            }
            None => tab.edit = None,
        }
    }

    /// `window.sheetEditFormula`: opens the card on the selected cell's formula, with `text` in it when given, commits what it holds with `commit` (as OK does), or closes it with `close`.
    pub(crate) fn sheet_edit_formula_command(&mut self, id: &str, p: &Map<String, Value>) -> Result<()> {
        let text = super::word(id, p, "text")?;
        let (commit, close) = (flag(id, p, "commit")?.unwrap_or(false), flag(id, p, "close")?.unwrap_or(false));
        if close {
            if commit || text.is_some() {
                return Err(bad(id, "`close` goes alone"));
            }
            if !self.formula_card_open() {
                return Err(bad(id, "the Edit Formula card is not open"));
            }
            self.sheet_card_cancel();
            return Ok(());
        }
        self.sheet_prepare();
        if !self.formula_card_open() {
            self.sheet_op_verdict(Op::EditFormula).map_err(|words| bad(id, words))?;
            self.sheet_open_formula_card().map_err(|words| bad(id, words))?;
        }
        if let (Some(text), Some(edit)) = (text, self.ui.sheet.tabs.get_mut(self.ui.sheet.active).and_then(|t| t.edit.as_mut())) {
            edit.set_text(text.to_owned());
            edit.fresh = true;
        }
        if commit {
            self.sheet_commit_edit(Then::Stay);
        }
        Ok(())
    }

    /// Ctrl+E: the card opens on the selected cell, or the footer says why it cannot.
    pub(crate) fn sheet_formula_key(&mut self) {
        if let Err(words) = self.sheet_op_verdict(Op::EditFormula).and_then(|()| self.sheet_open_formula_card()) {
            self.sheet_say(words);
        }
    }

    fn sheet_formula_card(&mut self, ctx: &Context, t: Tokens) {
        let active = self.ui.sheet.active;
        let Some(mut edit) = self.ui.sheet.tabs.get_mut(active).and_then(|tab| tab.edit.take()) else { return };
        let screen = ctx.content_rect();
        let width = (screen.width() - 60.0).clamp(280.0, 620.0);
        let text_h = (screen.height() - 240.0).clamp(80.0, 320.0);
        let rows = ((text_h / 16.0) as usize).clamp(3, 10);
        // Ctrl+Enter is OK and Escape is Cancel, taken before the text box can see them: Enter in the box is a line.
        let (mut ok, mut cancel) = (false, false);
        ctx.input_mut(|i| {
            ok = i.consume_key(Modifiers::COMMAND, Key::Enter);
            cancel = i.consume_key(Modifiers::NONE, Key::Escape);
        });
        let mut seen: Vec<(String, Rect, bool)> = Vec::new();
        let font = super::cell::font();
        let modal = egui::Modal::new(Id::unique("sheet_formula_card")).backdrop_color(t.scrim).frame(Self::card_frame(&t)).show(ctx, |ui| {
            ui.set_width(width);
            let title = format!("Edit Formula - {}", shown(&edit.reference, 60));
            ui.label(RichText::new(title).font(theme::font(size::TAB + 1.0)).color(t.text));
            ui.add_space(6.0);
            let id = edit_id();
            if edit.fresh {
                let mut state = egui::text_edit::TextEditState::load(ctx, id).unwrap_or_default();
                let end = edit.text.chars().count();
                state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(end))));
                state.store(ctx, id);
                edit.fresh = false;
                ctx.memory_mut(|m| m.request_focus(id));
            } else if ctx.memory(|m| m.focused().is_none()) {
                // The card is modal: the text box has the keyboard unless a button of the card has it (Tab moves there).
                ctx.memory_mut(|m| m.request_focus(id));
            }
            let fault = edit.fault.as_ref().map(|f| (f.start, f.end));
            let (ink, error) = (t.text, t.sheet_error);
            let layout_font = font.clone();
            let mut layouter =
                move |ui: &Ui, buf: &dyn TextBuffer, wrap: f32| fault_layout(ui, buf.as_str(), wrap, (&layout_font, ink, error), fault);
            egui::ScrollArea::vertical().id_salt("sheet_formula_text").max_height(text_h).auto_shrink([false, true]).show(ui, |ui| {
                let out = TextEdit::multiline(&mut edit.text)
                    .id(id)
                    .font(font.clone())
                    .text_color(ink)
                    .desired_width(width - 8.0)
                    .desired_rows(rows)
                    .char_limit(MAX_EDIT_CHARS)
                    .layouter(&mut layouter)
                    .show(ui);
                out.response.response.widget_info(|| WidgetInfo::labeled(Role::MultilineTextInput, true, "Formula"));
                seen.push(("sheet.card.formula.text".to_owned(), out.response.response.rect.intersect(ui.clip_rect()), true));
                if out.response.response.changed() {
                    edit.text_changed();
                }
            });
            ui.add_space(4.0);
            // What is wrong with a refused text is said once: the value the text works out to waits until the text is right.
            let problem = edit.refusal.as_ref().map(|r| edit.fault.as_ref().map_or_else(|| r.clone(), |f| f.words.clone()));
            if let Some(words) = problem {
                let label = ui.add(egui::Label::new(RichText::new(words).font(theme::font(size::LABEL)).color(t.sheet_error)).wrap());
                seen.push(("sheet.card.formula.words".to_owned(), label.rect, false));
            } else if let Some((words, live)) = super::bar::live_text(&edit) {
                let label = ui.add(egui::Label::new(RichText::new(words).font(theme::font(size::LABEL)).color(live.colour(&t))).truncate());
                seen.push(("sheet.card.formula.value".to_owned(), label.rect, false));
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let button = ui.add(egui::Button::new(RichText::new("OK").font(theme::font(size::LABEL))).min_size(vec2(84.0, 26.0)));
                seen.push(("sheet.card.formula.ok".to_owned(), button.rect, true));
                ok |= button.clicked();
                let button = ui.add(egui::Button::new(RichText::new("Cancel").font(theme::font(size::LABEL))).min_size(vec2(84.0, 26.0)));
                seen.push(("sheet.card.formula.cancel".to_owned(), button.rect, true));
                cancel |= button.clicked();
            });
        });
        crate::ask::keep_centred(ctx, modal.response.rect);
        for (name, rect, enabled) in seen {
            self.note(name, rect, enabled);
        }
        if let Some(tab) = self.ui.sheet.tabs.get_mut(active) {
            tab.edit = Some(edit);
        }
        if cancel {
            self.sheet_card_cancel();
        } else if ok {
            self.sheet_commit_edit(Then::Stay);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offer(label: &str, section: &str, variant: Option<&str>) -> (String, String, Option<String>) {
        (label.to_owned(), section.to_owned(), variant.map(str::to_owned))
    }

    #[test]
    fn the_card_lists_every_section_some_sheet_takes_in_the_windows_order_with_geometry_in_three_kinds() {
        let items = items_of(SheetKind::Shape, &[]);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "User-defined Cells",
                "Shape Data",
                "Hyperlinks",
                "Connection Points",
                "Actions",
                "Controls",
                "Geometry",
                "Ellipse",
                "Infinite Line",
                "Scratch",
                "Text Transform",
                "Layer Membership",
                "Action Tag"
            ]
        );
        assert!(items.iter().all(|i| !i.ticked), "nothing is ticked at first");
    }

    #[test]
    fn a_box_is_on_offer_when_sheet_sections_offers_it_has_when_the_sheet_could_take_it_and_elsewhere_when_it_could_not() {
        let offers = [offer("Scratch", "Scratch", None), offer("Ellipse", "Geometry", Some("Ellipse"))];
        let on_page = items_of(SheetKind::Page, &offers);
        let state = |label: &str| on_page.iter().find(|i| i.label == label).map(|i| (i.offer, i.section.clone(), i.variant.clone()));
        assert_eq!(state("Scratch"), Some((Offer::Offered, "Scratch".to_owned(), None)));
        assert_eq!(
            state("Ellipse"),
            Some((Offer::Offered, "Geometry".to_owned(), Some("Ellipse".to_owned()))),
            "a Geometry kind carries its variant"
        );
        assert_eq!(state("Hyperlinks").map(|s| s.0), Some(Offer::Has), "a page takes Hyperlinks and this one has them");
        assert_eq!(state("Connection Points").map(|s| s.0), Some(Offer::Elsewhere), "only a shape takes Connection Points");
        assert_eq!(state("Geometry").map(|s| s.0), Some(Offer::Elsewhere), "and Geometry");
    }

    #[test]
    fn the_offers_are_read_from_the_answer_and_a_malformed_entry_is_skipped() {
        let answer = json!({"can_add": [
            {"section": "Scratch", "label": "Scratch", "variant": null},
            {"section": "Geometry", "label": "Ellipse", "variant": "Ellipse"},
            {"section": "Prop"},
            "nonsense"
        ]});
        assert_eq!(offers_of(&answer), vec![offer("Scratch", "Scratch", None), offer("Ellipse", "Geometry", Some("Ellipse"))]);
        assert!(offers_of(&json!({})).is_empty());
        assert!(offers_of(&json!({"can_add": 3})).is_empty());
    }
}
