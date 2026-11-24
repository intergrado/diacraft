//! Insert, Links, Link: the Hyperlinks dialog and following a link.
//!
//! **The dialog** (the image): the shape's hyperlinks in a list, the boxes **Address**, **Sub-address** and **Description** of the one chosen, the buttons **New**, **Delete** and **Default**, and
//! **OK** and **Cancel**. It is opened with Ctrl+K or Insert, Links, Link, for the one shape selected (Link is greyed out with none). It works on a draft, as the other dialogs do: nothing is sent
//! until OK, and then **one `hyperlink.set`** (one undo step, "Hyperlinks") with the whole list: a row kept by its name, a row made here with none, and a row left out is taken away.
//! A link with neither an address nor a sub-address is no link and is left out (the engine would refuse it). A **Page** list beside the sub-address fills it with a page's name (the dialog's
//! Browse for a page). **Not in this dialog** (ours, recorded): the "Use relative path for hyperlink" box (a link to a file is not followed from a click, so there is no path to make relative),
//! and the Browse button of the address (the address is a web, FTP or mail address, or empty for this drawing), leave their behaviour open. A refusal of the engine is
//! said in the dialog, which stays open.
//!
//! **Following a link**: a **Ctrl+click** on a shape that holds links, with **no shape selected** (with another shape selected the click only adds to the selection), follows the one marked
//! default, else the first that is not hidden. A link of this drawing goes to its page (and selects its shape); a web, FTP or mail address is handed to the system through egui's `OpenUrl`, which
//! the window does on its next frame. Only those are followed: `hyperlink.follow` refuses a file or another program in words and the refusal is the notice. The window command
//! is `window.followLink {id}`.

use diacraft_engine::{EngineError, Result};
use egui::{Context, CornerRadius, Id, Key, Modifiers, RichText, Sense, Stroke, StrokeKind, TextEdit, Ui, vec2};
use serde_json::{Map, Value, json};

use crate::app::DiacraftApp;
use crate::theme::{self, Tokens, size};

/// The height of a row of a list, in points.
pub(crate) const ROW_H: f32 = 24.0;

/// The most characters each box takes (the engine's own cap).
const MAX_CHARS: usize = 4_096;

/// One link of the draft: a row of the shape as it was (`name`), or one made in the dialog.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Row {
    pub name: Option<String>,
    pub description: String,
    pub address: String,
    pub sub_address: String,
    pub default: bool,
}

impl Row {
    fn made() -> Row {
        Row { name: None, description: String::new(), address: String::new(), sub_address: String::new(), default: false }
    }

    /// What the list says of the row: its description, else where it goes.
    fn caption(&self) -> String {
        let words = [&self.description, &self.address, &self.sub_address].into_iter().find(|t| !t.trim().is_empty());
        let mut text = words.map_or_else(|| "(new hyperlink)".to_owned(), |t| t.trim().to_owned());
        if self.default {
            text.push_str("  (default)");
        }
        text
    }

    /// Whether the row says where it goes: a link with neither an address nor a sub-address is no link.
    fn goes_somewhere(&self) -> bool {
        !self.address.trim().is_empty() || !self.sub_address.trim().is_empty()
    }
}

/// The Hyperlinks dialog while it is open.
#[derive(Clone, Debug)]
pub struct LinksDialog {
    page: u32,
    id: u32,
    rows: Vec<Row>,
    /// The row the boxes show.
    selected: usize,
    /// What the engine refused the last OK with, said in the dialog.
    error: Option<String>,
    /// The names of the pages, for the Page list.
    pages: Vec<String>,
}

impl LinksDialog {
    /// The `links` of the `hyperlink.set` OK sends: each row that goes somewhere, kept rows by name.
    fn links(&self) -> Vec<Value> {
        self.rows
            .iter()
            .filter(|r| r.goes_somewhere())
            .map(|r| {
                let mut map = Map::new();
                if let Some(name) = &r.name {
                    map.insert("name".to_owned(), json!(name));
                }
                map.insert("description".to_owned(), json!(r.description.trim()));
                map.insert("address".to_owned(), json!(r.address.trim()));
                map.insert("sub_address".to_owned(), json!(r.sub_address.trim()));
                map.insert("default".to_owned(), json!(r.default));
                Value::Object(map)
            })
            .collect()
    }
}

/// What a press in the dialog chose, taken after the card is drawn.
#[derive(Default)]
struct Pressed {
    ok: bool,
    cancel: bool,
    new: bool,
    delete: bool,
    default: bool,
}

fn bad(cmd: &str, msg: impl Into<String>) -> EngineError {
    EngineError::BadParams { cmd: cmd.to_owned(), msg: msg.into() }
}

impl DiacraftApp {
    /// The dialog's state for the window state and the tests: `null` when it is closed, else the shape, the rows and the one chosen.
    pub(crate) fn links_state(&self) -> Value {
        let Some(d) = &self.ui.insert.links else { return Value::Null };
        json!({
            "id": d.id,
            "page": d.page,
            "selected": d.selected,
            "error": d.error,
            "rows": d.rows.iter().map(|r| json!({
                "name": r.name,
                "description": r.description,
                "address": r.address,
                "sub_address": r.sub_address,
                "default": r.default,
            })).collect::<Vec<_>>(),
        })
    }

    /// The one shape selected, with the page it is on: what Link works on.
    pub(crate) fn one_shape_selected(&self) -> Option<(u32, u32)> {
        let doc = self.session().document()?;
        let chosen = self.ui.page.filter(|p| doc.page(*p).is_some());
        let page = chosen.or_else(|| crate::page_tabs::tab_order(doc).first().map(|(id, _)| *id))?;
        match self.ui.edit.selection() {
            [only] => Some((page, *only)),
            _ => None,
        }
    }

    /// Whether Link can run now, and if not why.
    pub(crate) fn links_enabled(&self) -> std::result::Result<(), String> {
        self.command_enabled("hyperlink.set")?;
        if self.one_shape_selected().is_none() {
            return Err("select one shape to give it a hyperlink".to_owned());
        }
        Ok(())
    }

    /// Opens the dialog for the one shape selected: its links are asked of the engine (`hyperlink.list`). An open dialog stays as it is.
    pub(crate) fn open_links(&mut self) -> Result<()> {
        const ID: &str = "window.hyperlinks";
        if self.ui.insert.links.is_some() {
            return Ok(());
        }
        let (page, id) = self.one_shape_selected().ok_or_else(|| bad(ID, "select one shape to give it a hyperlink"))?;
        // A text box that is open ends first: what was typed is kept.
        self.commit_pending_text();
        let answer = self.execute("hyperlink.list", &json!({"id": id, "page": page}))?;
        let text = |l: &Value, key: &str| l.get(key).and_then(Value::as_str).unwrap_or("").to_owned();
        let rows: Vec<Row> = answer
            .get("links")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .map(|l| Row {
                        name: l.get("name").and_then(Value::as_str).map(str::to_owned),
                        description: text(l, "description"),
                        address: text(l, "address"),
                        sub_address: text(l, "sub_address"),
                        default: l.get("default").and_then(Value::as_bool).unwrap_or(false),
                    })
                    .collect()
            })
            .unwrap_or_default();
        // A shape with no link starts with one empty row, so that the boxes are there to type in.
        let rows = if rows.is_empty() { vec![Row::made()] } else { rows };
        let pages = self.session().document().map(|doc| doc.pages.iter().map(|p| p.name.to_string()).collect()).unwrap_or_default();
        self.ui.insert.links = Some(LinksDialog { page, id, rows, selected: 0, error: None, pages });
        Ok(())
    }

    pub(crate) fn close_links(&mut self) {
        self.ui.insert.links = None;
    }

    /// OK: one `hyperlink.set` with the draft, and the dialog closes; a refusal is said in the dialog, which stays.
    fn apply_links(&mut self) {
        let Some(dialog) = self.ui.insert.links.clone() else { return };
        let params = json!({"id": dialog.id, "page": dialog.page, "links": dialog.links()});
        match self.execute("hyperlink.set", &params) {
            Ok(_) => self.close_links(),
            Err(e) => {
                if let Some(open) = self.ui.insert.links.as_mut() {
                    open.error = Some(e.to_string());
                }
            }
        }
    }

    pub(crate) fn links_dialog(&mut self, ctx: &Context, t: Tokens) {
        let Some(mut dialog) = self.ui.insert.links.take() else { return };
        let screen = ctx.content_rect();
        let width = (screen.width() - 60.0).clamp(300.0, 560.0);
        let mut pressed = Pressed::default();
        if !egui::Popup::is_any_open(ctx) && ctx.memory(|m| m.focused().is_none()) && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter)) {
            pressed.ok = true;
        }
        let mut seen: Vec<(String, egui::Rect, bool)> = Vec::new();
        let modal = egui::Modal::new(Id::unique("hyperlinks_dialog")).backdrop_color(t.scrim).frame(Self::card_frame(&t)).show(ctx, |ui| {
            ui.set_width(width);
            ui.label(RichText::new("Hyperlinks").font(theme::font(size::TAB + 1.0)).color(t.text));
            ui.add_space(8.0);
            heading(ui, &t, "Hyperlinks of this shape");
            egui::ScrollArea::vertical().id_salt("hyperlinks_list").max_height(112.0).auto_shrink([false, true]).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for (at, row) in dialog.rows.iter().enumerate() {
                    if list_row(ui, &t, &row.caption(), at == dialog.selected, format!("links.row.{at}"), &mut seen) {
                        dialog.selected = at;
                    }
                }
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                for (label, key, set) in [("New", "links.new", 0), ("Delete", "links.delete", 1), ("Default", "links.default", 2)] {
                    let b = ui.add(egui::Button::new(RichText::new(label).font(theme::font(size::LABEL))).min_size(vec2(72.0, 24.0)));
                    seen.push((key.to_owned(), b.rect, true));
                    if b.clicked() {
                        match set {
                            0 => pressed.new = true,
                            1 => pressed.delete = true,
                            _ => pressed.default = true,
                        }
                    }
                }
            });
            ui.add_space(8.0);
            let pages = dialog.pages.clone();
            let selected = dialog.selected;
            if let Some(row) = dialog.rows.get_mut(selected) {
                edit_row(ui, &t, "Address", &mut row.address, "links.address", &mut seen);
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(ui.available_width() - 120.0);
                        edit_row(ui, &t, "Sub-address", &mut row.sub_address, "links.sub_address", &mut seen);
                    });
                    ui.vertical(|ui| {
                        ui.add_space(18.0);
                        let from = egui::ComboBox::from_id_salt("links_page")
                            .selected_text(RichText::new("Page").font(theme::font(size::LABEL)))
                            .width(110.0);
                        let shown = from.show_ui(ui, |ui| {
                            for name in &pages {
                                if ui.selectable_label(false, name).clicked() {
                                    row.sub_address.clone_from(name);
                                }
                            }
                        });
                        shown.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, "Page"));
                        seen.push(("links.page".to_owned(), shown.response.rect, true));
                    });
                });
                edit_row(ui, &t, "Description", &mut row.description, "links.description", &mut seen);
            }
            if let Some(error) = &dialog.error {
                ui.add_space(4.0);
                ui.label(RichText::new(error.as_str()).font(theme::font(size::LABEL)).color(t.notice_text));
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                for (label, key, ok) in [("OK", "links.ok", true), ("Cancel", "links.cancel", false)] {
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
        if pressed.new {
            dialog.rows.push(Row::made());
            dialog.selected = dialog.rows.len() - 1;
            dialog.error = None;
        }
        if pressed.delete && dialog.selected < dialog.rows.len() {
            dialog.rows.remove(dialog.selected);
            dialog.selected = dialog.selected.min(dialog.rows.len().saturating_sub(1));
            dialog.error = None;
        }
        // Default marks the row chosen and clears the others: one link is the default.
        if pressed.default {
            let chosen = dialog.selected;
            for (at, row) in dialog.rows.iter_mut().enumerate() {
                row.default = at == chosen && !row.default;
            }
        }
        self.ui.insert.links = Some(dialog);
        if pressed.cancel {
            self.close_links();
        } else if pressed.ok {
            self.apply_links();
        }
    }

    /// Follows the first link of shape `id` on a page the way a Ctrl+click does. → whether the shape had a link: `false` leaves the click to what it did before (the shape joins the selection).
    /// A link that is refused (a file, another program) is the notice, in the engine's words, and is still a link: the click did what it could.
    pub(crate) fn follow_link_of(&mut self, page: u32, id: u32) -> bool {
        let has = self
            .execute("hyperlink.list", &json!({"id": id, "page": page}))
            .ok()
            .and_then(|a| a.get("links").and_then(Value::as_array).map(|l| !l.is_empty()))
            .unwrap_or(false);
        if !has {
            return false;
        }
        match self.execute("hyperlink.follow", &json!({"id": id, "page": page})) {
            Ok(go) => self.go_to_link(&go),
            Err(e) => self.set_notice(&e.to_string()),
        }
        true
    }

    /// Does what the engine said a link does (`hyperlink.follow`'s answer): shows the page or the shape of this drawing, or has the next frame hand the address to the system.
    fn go_to_link(&mut self, go: &Value) {
        let kind = go.get("kind").and_then(Value::as_str).unwrap_or("");
        let target_page = go.get("target_page").and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok());
        match (kind, target_page) {
            ("page", Some(page)) => {
                if let Err(e) = self.execute("page.select", &json!({"id": page})) {
                    self.set_notice(&e.to_string());
                }
            }
            ("shape", Some(page)) => match self.execute("page.select", &json!({"id": page})) {
                Ok(_) => {
                    if let Some(shape) = go.get("target_shape").and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok()) {
                        self.select(&[shape]);
                    }
                }
                Err(e) => self.set_notice(&e.to_string()),
            },
            ("web" | "mail", _) => {
                if let Some(url) = go.get("url").and_then(Value::as_str) {
                    self.ui.insert.open_url = Some(url.to_owned());
                }
            }
            _ => self.set_notice("The link goes nowhere DiaCraft follows."),
        }
    }

    /// The `window.followLink {id}` command: follows the shape's link as a Ctrl+click does, and refuses in words for a shape with none.
    pub(crate) fn command_follow_link(&mut self, id: u32) -> Result<Value> {
        const ID: &str = "window.followLink";
        let page = self.current_page().ok_or_else(|| bad(ID, "no document open"))?;
        let go = self.execute("hyperlink.follow", &json!({"id": id, "page": page}))?;
        self.go_to_link(&go);
        Ok(go)
    }
}

pub(crate) fn heading(ui: &mut Ui, t: &Tokens, words: &str) {
    ui.label(RichText::new(words).font(theme::font(size::LABEL)).color(t.text_dim));
    ui.add_space(2.0);
}

/// A selectable row of a list, as wide as the list. → whether it was pressed. A caption wider than the row is cut at its edge.
pub(crate) fn list_row(ui: &mut Ui, t: &Tokens, label: &str, on: bool, key: String, seen: &mut Vec<(String, egui::Rect, bool)>) -> bool {
    // An overlay's rows sense the drag as well as the click, so that a press on one never reaches the drawing under the dialog.
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width().max(60.0), ROW_H), Sense::click_and_drag());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::ListItem, true, on, label));
    let painter = ui.painter_at(rect);
    if on {
        painter.rect_filled(rect, CornerRadius::same(3), t.accent_soft);
    } else if resp.hovered() {
        painter.rect_filled(rect, CornerRadius::same(3), t.hover);
    }
    if resp.has_focus() {
        painter.rect_stroke(rect.shrink(0.5), CornerRadius::same(3), Stroke::new(1.0, t.accent), StrokeKind::Inside);
    }
    painter.text(egui::pos2(rect.left() + 8.0, rect.center().y), egui::Align2::LEFT_CENTER, label, theme::font(size::LABEL), t.text);
    seen.push((key, rect, true));
    resp.clicked()
}

/// A labelled single-line box, as wide as the dialog. The box is noted under `key` for the tests and `ui.inspect`.
pub(crate) fn edit_row(ui: &mut Ui, t: &Tokens, label: &str, text: &mut String, key: &str, seen: &mut Vec<(String, egui::Rect, bool)>) {
    heading(ui, t, label);
    let out = TextEdit::singleline(text)
        .font(theme::font(size::LABEL))
        .text_color(t.text)
        .desired_width(ui.available_width())
        .char_limit(MAX_CHARS)
        .show(ui);
    out.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, label));
    seen.push((key.to_owned(), out.response.rect, true));
    ui.add_space(4.0);
}
