//! File > New: the gallery of templates.
//!
//! The page has the headings across its top, **Personal**, **Templates** and **Sample Diagrams** (the organization's and Office's are left out: DiaCraft has neither), a
//! heading chosen shows its tiles under it. **Templates** holds the Blank drawing tile and a tile for each template of the engine's `template.list`, each showing the
//! template's name and its category. A press on a tile makes the new drawing, as Ctrl+N does for the blank one: it asks first if the drawing has changes that are
//! not saved, and then runs `file.new` with the template's id (`save::Then::Template`). **Personal** holds the person's own templates and **Sample Diagrams** the
//! engine's samples: DiaCraft has neither yet, so Personal is quiet and Sample Diagrams says there are none.
//!
//! **The pictures are drawn here, from what `template.list` says**: a sheet of the page's shape (landscape or portrait, and its proportions), with a small plain
//! picture of the template's category on it (a flowchart, or boxes joined by a line), in the first colours of the template's theme where it has one. There is no
//! rendering of the template: its page is empty (`template.list`). Nothing of another application's is used.
//!
//! **A press on a template's tile opens its detail** ("picking a tile opens a dialog with its name, a description, preview thumbnails, a
//! close button, Metric Units and US Units radio buttons, and a large Create button"): the gallery steps back and the page shows the template's name, its category, its
//! description, a larger picture of its page and **Create**, which asks about unsaved changes as Ctrl+N does and then runs `file.new` with the template's id; **Close** and
//! Escape go back to the gallery and make nothing. **There is no Metric Units or US Units choice**: the engine has no unit choice at the start of a drawing (the P1 row
//! "Choose metric or US units when starting"), so the detail does not offer one it could not honour. The Blank drawing tile makes its drawing at once, as it always has.

use std::sync::Arc;

use egui::{Align2, CornerRadius, Painter, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, UiBuilder, pos2, vec2};
use serde_json::{Value, json};

use crate::app::DiacraftApp;
use crate::theme::{self, Tokens, size};
use crate::widgets::{self, text_width, tip};

use crate::backstage::BLANK_TILE;

/// The most templates the page lists: the list is the engine's, and a gallery of thousands is not one a person scrolls.
const MAX_TEMPLATES: usize = 256;

/// A tile's size, and the gap between tiles, in points.
const TILE_W: f32 = 156.0;
const TILE_H: f32 = 156.0;
const GAP: f32 = 18.0;
/// The height of a row of headings when they wrap, in points.
const HEADING_ROW: f32 = 32.0;

/// The headings of the New page, in order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NewTab {
    Personal,
    #[default]
    Templates,
    Samples,
}

impl NewTab {
    const ALL: [NewTab; 3] = [NewTab::Personal, NewTab::Templates, NewTab::Samples];

    /// The heading's words.
    pub const fn heading(self) -> &'static str {
        match self {
            NewTab::Personal => "Personal",
            NewTab::Templates => "Templates",
            NewTab::Samples => "Sample Diagrams",
        }
    }

    /// Whether the heading has anything behind it: Personal needs a folder of the person's own templates, which the window has no setting for yet.
    const fn works(self) -> bool {
        !matches!(self, NewTab::Personal)
    }
}

/// A template as a tile shows it: what `template.list` says of it, and the first colours of its theme.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Card {
    pub id: String,
    pub name: String,
    pub category: String,
    pub description: String,
    /// The page's width and height in paper inches, as the template starts it (turned to its orientation).
    pub page: (f64, f64),
    /// The first colours of the template's theme as `(r, g, b)`; none for a template with no theme.
    pub accents: Vec<(u8, u8, u8)>,
}

/// What the window remembers of the New page: the heading on view, and the templates the engine listed.
#[derive(Default)]
pub struct NewPageState {
    pub tab: NewTab,
    cards: Option<Arc<Vec<Card>>>,
    /// The id of the template whose detail is open, in place of the gallery.
    pub detail: Option<String>,
}

/// `#rrggbb` as bytes. Six hexadecimal digits and nothing else: `u8::from_str_radix` takes a sign before a digit, so a pair is checked to be two digits first.
pub(crate) fn parse_hex(text: &str) -> Option<(u8, u8, u8)> {
    let digits = text.strip_prefix('#')?;
    if digits.len() != 6 || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |at: usize| digits.get(at..at + 2).and_then(|pair| u8::from_str_radix(pair, 16).ok());
    Some((byte(0)?, byte(2)?, byte(4)?))
}

/// The cards `template.list` answers with, and (from `theme.list`) the first accents of each one's theme. An entry that has no `id` or no `name` is left out.
pub(crate) fn cards_from(list: &Value, themes: Option<&Value>) -> Vec<Card> {
    let Some(entries) = list.get("templates").and_then(Value::as_array) else { return Vec::new() };
    let accents_of = |theme: &str| -> Vec<(u8, u8, u8)> {
        let found = themes
            .and_then(|t| t.get("themes"))
            .and_then(Value::as_array)
            .and_then(|all| all.iter().find(|t| t.get("id").and_then(Value::as_str) == Some(theme)));
        found
            .and_then(|t| t.get("colours"))
            .and_then(|c| c.get("accents"))
            .and_then(Value::as_array)
            .map(|colours| colours.iter().filter_map(Value::as_str).filter_map(parse_hex).take(3).collect())
            .unwrap_or_default()
    };
    entries
        .iter()
        .take(MAX_TEMPLATES)
        .filter_map(|entry| {
            let text = |key: &str| entry.get(key).and_then(Value::as_str).map(str::to_owned);
            let page = entry.get("page");
            let inches = |key: &str| page.and_then(|p| p.get(key)).and_then(Value::as_f64).filter(|v| v.is_finite() && *v > 0.0).unwrap_or(0.0);
            let theme = entry.get("theme").and_then(|t| t.get("id")).and_then(Value::as_str);
            Some(Card {
                id: text("id")?,
                name: text("name")?,
                category: text("category").unwrap_or_default(),
                description: text("description").unwrap_or_default(),
                page: (inches("width"), inches("height")),
                accents: theme.map(accents_of).unwrap_or_default(),
            })
        })
        .collect()
}

/// `text` shortened with an ellipsis so that it is at most `max` points wide.
fn fitted(ui: &Ui, text: &str, font: &egui::FontId, max: f32) -> String {
    if text_width(ui, text, font) <= max {
        return text.to_owned();
    }
    let mut kept: Vec<char> = text.chars().take(64).collect();
    while !kept.is_empty() {
        kept.pop();
        let shorter: String = kept.iter().collect::<String>() + "\u{2026}";
        if text_width(ui, &shorter, font) <= max {
            return shorter;
        }
    }
    "\u{2026}".to_owned()
}

impl DiacraftApp {
    /// The templates the engine lists, read once for the window.
    pub(crate) fn template_cards(&mut self) -> Arc<Vec<Card>> {
        if let Some(cards) = &self.ui.new_page.cards {
            return cards.clone();
        }
        let list = self.execute("template.list", &json!({}));
        // The theme list only colours the pictures: a refusal of it is no reason to show none.
        let themes = self.execute("theme.list", &json!({})).ok();
        let cards = match list {
            Ok(list) => cards_from(&list, themes.as_ref()),
            Err(e) => {
                self.set_notice(&e.to_string());
                Vec::new()
            }
        };
        let cards = Arc::new(cards);
        self.ui.new_page.cards = Some(cards.clone());
        cards
    }

    /// The New page: its headings and the tiles of the one chosen.
    pub(crate) fn new_page(&mut self, ui: &mut Ui, t: &Tokens, body: Rect) {
        ui.painter().text(body.left_top(), Align2::LEFT_TOP, "New", theme::font(26.0), t.text);
        // The headings run along a row and wrap to the next when the window is narrow, so that none lies outside it.
        let mut x = body.left();
        let mut y = body.top() + 56.0;
        for tab in NewTab::ALL {
            let heading = tab.heading();
            let font = theme::font(size::TAB + 1.0);
            let w = text_width(ui, heading, &font);
            if x > body.left() && x + w + 8.0 > body.right() {
                x = body.left();
                y += HEADING_ROW;
            }
            let r = Rect::from_min_size(pos2(x, y), vec2(w + 8.0, 26.0));
            let enabled = tab.works();
            let on = self.ui.new_page.tab == tab;
            let resp = ui.interact(r, ui.scope_id().with(("new_heading", heading)), if enabled { Sense::click() } else { Sense::hover() });
            widgets::name_toggle(&resp, enabled, on, heading);
            let colour = if !enabled {
                t.text_disabled
            } else if on {
                t.text
            } else {
                t.text_dim
            };
            ui.painter().text(pos2(r.left() + 4.0, r.center().y), Align2::LEFT_CENTER, heading, font, colour);
            if on {
                ui.painter().hline(r.left()..=r.right(), y + 29.0, Stroke::new(2.0, t.accent));
            }
            self.note(format!("backstage.heading.{heading}"), r, enabled);
            if resp.on_hover_text(tip(heading, enabled)).clicked() && enabled {
                self.ui.new_page.tab = tab;
            }
            x += w + 36.0;
        }
        ui.painter().hline(body.left()..=body.right(), y + 30.0, Stroke::new(1.0, t.separator));
        let area = Rect::from_min_max(pos2(body.left(), y + 42.0), body.max);
        let tab = self.ui.new_page.tab;
        let cards = self.template_cards();
        // The detail of a template, in place of the gallery (a template the engine no longer lists has none to show).
        let detail = self.ui.new_page.detail.clone().and_then(|id| cards.iter().find(|c| c.id == id).cloned());
        if self.ui.new_page.detail.is_some() && (detail.is_none() || tab != NewTab::Templates) {
            self.ui.new_page.detail = None;
        }
        if let Some(card) = detail.filter(|_| tab == NewTab::Templates) {
            self.template_detail(ui, t, area, &card);
            return;
        }
        ui.scope_builder(UiBuilder::new().max_rect(area), |ui| {
            egui::ScrollArea::vertical().id_salt("new_scroll").auto_shrink([false, false]).show(ui, |ui| match tab {
                NewTab::Templates => self.template_tiles(ui, t, &cards),
                NewTab::Samples => {
                    ui.label(egui::RichText::new("No sample diagrams are built yet.").font(theme::font(size::LABEL + 1.0)).color(t.text_disabled));
                }
                NewTab::Personal => {}
            });
        });
    }

    /// The Templates tiles: Blank drawing first, then one for each template, in as many columns as the width holds.
    fn template_tiles(&mut self, ui: &mut Ui, t: &Tokens, cards: &[Card]) {
        let count = cards.len() + 1;
        let width = ui.available_width().max(TILE_W);
        let columns = (((width + GAP) / (TILE_W + GAP)).floor() as usize).max(1);
        let rows = count.div_ceil(columns);
        let (grid, _) = ui.allocate_exact_size(vec2(width, rows as f32 * (TILE_H + GAP) - GAP), Sense::hover());
        let cell = |n: usize| {
            Rect::from_min_size(grid.min + vec2((n % columns) as f32 * (TILE_W + GAP), (n / columns) as f32 * (TILE_H + GAP)), vec2(TILE_W, TILE_H))
        };
        let enabled = self.command_enabled("file.new").is_ok();
        if self.tile(ui, t, cell(0), "blank", BLANK_TILE, None, "A new drawing with no template", enabled, |painter, pic, t| {
            let page = Rect::from_center_size(pic.center(), vec2(72.0, 54.0));
            painter.rect(page, CornerRadius::ZERO, t.page_fill, Stroke::new(1.0, t.page_border), StrokeKind::Inside);
        }) {
            self.ask_then(ui.ctx(), crate::save::Then::New);
        }
        for (n, card) in cards.iter().enumerate() {
            let key = format!("template.{}", card.id);
            let words = if card.description.is_empty() { card.name.clone() } else { format!("{}: {}", card.name, card.description) };
            let picture = |painter: &Painter, pic: Rect, t: &Tokens| paint_preview(painter, pic, card, t, 70.0);
            if self.tile(ui, t, cell(n + 1), &key, &card.name, Some(&card.category), &words, enabled, picture) {
                self.ui.new_page.detail = Some(card.id.clone());
            }
        }
    }

    /// One tile of the gallery: its picture over its name, and under that its category when it has one. → whether it was pressed.
    #[allow(clippy::too_many_arguments)]
    fn tile(
        &mut self,
        ui: &mut Ui,
        t: &Tokens,
        r: Rect,
        key: &str,
        title: &str,
        category: Option<&str>,
        hover: &str,
        enabled: bool,
        picture: impl FnOnce(&Painter, Rect, &Tokens),
    ) -> bool {
        let resp = ui.interact(r, ui.scope_id().with(("new_tile", key)), if enabled { Sense::click() } else { Sense::hover() });
        widgets::name_button(&resp, enabled, title);
        if resp.gained_focus() {
            resp.scroll_to_me(None);
        }
        let painter = ui.painter();
        let border = if (resp.hovered() || resp.has_focus()) && enabled { Stroke::new(2.0, t.accent) } else { Stroke::new(1.0, t.tile_border) };
        painter.rect(r, CornerRadius::same(6), t.backstage_tile, border, StrokeKind::Inside);
        picture(painter, Rect::from_center_size(pos2(r.center().x, r.top() + 56.0), vec2(TILE_W - 24.0, 88.0)), t);
        let font = theme::font(size::LABEL + 1.0);
        let max = TILE_W - 16.0;
        let name = fitted(ui, title, &font, max);
        ui.painter().text(pos2(r.center().x, r.bottom() - 36.0), Align2::CENTER_CENTER, name, font, t.text);
        if let Some(category) = category {
            let small = theme::font(size::LABEL);
            let words = fitted(ui, category, &small, max);
            ui.painter().text(pos2(r.center().x, r.bottom() - 17.0), Align2::CENTER_CENTER, words, small, t.text_dim);
        }
        self.note(format!("backstage.tile.{key}"), r, enabled);
        resp.on_hover_text(tip(hover, enabled)).clicked() && enabled
    }
}

impl DiacraftApp {
    /// `window.templateDetail`: opens the File view on New with the detail of the template named (an id of `template.list`), or goes back to the gallery with `close`.
    pub(crate) fn template_detail_command(&mut self, p: &serde_json::Map<String, Value>) -> diacraft_engine::Result<()> {
        let bad = |msg: String| diacraft_engine::EngineError::BadParams { cmd: "window.templateDetail".to_owned(), msg };
        if matches!(p.get("close"), Some(Value::Bool(true))) {
            self.ui.new_page.detail = None;
            return Ok(());
        }
        let Some(Value::String(id)) = p.get("template") else { return Err(bad("give `template`, or `close: true`".to_owned())) };
        let cards = self.template_cards();
        let Some(card) = cards.iter().find(|c| c.id.eq_ignore_ascii_case(id)) else {
            let ids: Vec<&str> = cards.iter().map(|c| c.id.as_str()).collect();
            return Err(bad(format!("no template `{}`; the templates are: {}", diacraft_engine::shown(id, 32), ids.join(", "))));
        };
        self.ui.new_page.tab = NewTab::Templates;
        self.ui.new_page.detail = Some(card.id.clone());
        self.ui.backstage = Some(crate::state::BackstagePage::New);
        Ok(())
    }

    /// The detail of one template in `area` (the gallery's place): its name, its category, its description, a picture of its page and the **Create** and **Close** buttons.
    /// A wide area puts the picture at the left of the words and a narrow one over them; either way the buttons are under the words and inside the area, which scrolls
    /// when the window is too short for all of it.
    fn template_detail(&mut self, ui: &mut Ui, t: &Tokens, area: Rect, card: &Card) {
        let enabled = self.command_enabled("file.new").is_ok();
        let mut create = false;
        let mut close = false;
        ui.scope_builder(UiBuilder::new().max_rect(area), |ui| {
            egui::ScrollArea::vertical().id_salt("new_detail_scroll").auto_shrink([false, false]).show(ui, |ui| {
                let width = ui.available_width().max(1.0);
                let wide = width >= 560.0;
                let side = if wide { 220.0_f32 } else { width.min(220.0) };
                let words_w = if wide { (width - side - 28.0).max(160.0) } else { width };
                let (picture, _) = ui.allocate_exact_size(vec2(side, side * 0.8), Sense::hover());
                ui.painter().rect(picture, CornerRadius::same(6), t.backstage_tile, Stroke::new(1.0, t.tile_border), StrokeKind::Inside);
                paint_preview(ui.painter(), picture, card, t, side * 0.62);
                self.note("backstage.detail.picture", picture, true);
                let words_at = if wide { pos2(picture.right() + 28.0, picture.top()) } else { pos2(picture.left(), picture.bottom() + 14.0) };
                let words = Rect::from_min_size(words_at, vec2(words_w, 400.0));
                let inner = ui.scope_builder(UiBuilder::new().max_rect(words), |ui| {
                    ui.set_width(words_w);
                    ui.add(egui::Label::new(egui::RichText::new(&card.name).font(theme::font(size::TAB + 8.0)).color(t.text)).wrap());
                    ui.add_space(2.0);
                    if !card.category.is_empty() {
                        ui.add(egui::Label::new(egui::RichText::new(&card.category).font(theme::font(size::LABEL + 1.0)).color(t.text_dim)).wrap());
                        ui.add_space(8.0);
                    }
                    if !card.description.is_empty() {
                        ui.add(egui::Label::new(egui::RichText::new(&card.description).font(theme::font(size::LABEL + 1.0)).color(t.text)).wrap());
                        ui.add_space(8.0);
                    }
                    let (w, h) = card.page;
                    if w > 0.0 && h > 0.0 {
                        let page = format!("Page: {} by {} inches", number_words(w), number_words(h));
                        ui.add(egui::Label::new(egui::RichText::new(page).font(theme::font(size::LABEL)).color(t.text_dim)).wrap());
                    }
                });
                let below = inner.response.rect.bottom().max(picture.bottom()) + 18.0;
                let buttons = Rect::from_min_size(pos2(picture.left(), below), vec2(width.min(260.0), 34.0));
                for (n, (label, key, primary)) in
                    [("Create", "backstage.detail.create", true), ("Close", "backstage.detail.close", false)].into_iter().enumerate()
                {
                    let r = Rect::from_min_size(buttons.min + vec2(n as f32 * 100.0, 0.0), vec2(92.0, 34.0));
                    let usable = enabled || !primary;
                    let text = egui::RichText::new(label).font(theme::font(size::LABEL + 1.0));
                    let mut button = egui::Button::new(text);
                    if primary {
                        button = button.fill(t.accent_soft).stroke(Stroke::new(1.5, t.accent));
                    }
                    let resp = ui.put(r, button.sense(if usable { Sense::click() } else { Sense::hover() }));
                    widgets::name_button(&resp, usable, label);
                    self.note(key, r, usable);
                    if resp.on_hover_text(tip(label, usable)).clicked() && usable {
                        if primary { create = true } else { close = true }
                    }
                }
                ui.allocate_rect(Rect::from_min_max(picture.min, pos2(picture.left() + width, buttons.bottom() + 8.0)), Sense::hover());
            });
        });
        self.note("backstage.detail", area, true);
        if create {
            // The detail stays open until the drawing is made: a question about unsaved changes can still be cancelled, and Cancel leaves the detail as it was.
            self.ask_then(ui.ctx(), crate::save::Then::Template(card.id.clone()));
        } else if close {
            self.ui.new_page.detail = None;
        }
    }
}

/// A length in inches as words: whole when it is, else to two places with no trailing zero.
fn number_words(v: f64) -> String {
    if (v - v.round()).abs() < 1.0e-9 { format!("{}", v.round() as i64) } else { format!("{}", (v * 100.0).round() / 100.0) }
}

/// A sheet of the template's page, with a plain picture of its category on it. The sheet keeps the page's proportions, the long side `long` points (70 on a tile).
fn paint_preview(painter: &Painter, pic: Rect, card: &Card, t: &Tokens, long: f32) {
    let (w, h) = card.page;
    let ratio = if w > 0.0 && h > 0.0 { (w.min(h) / w.max(h)).clamp(0.4, 1.0) as f32 } else { 0.77 };
    let size = if w >= h { vec2(long, long * ratio) } else { vec2(long * ratio, long) };
    let page = Rect::from_center_size(pic.center(), size);
    painter.rect(page, CornerRadius::ZERO, t.page_fill, Stroke::new(1.0, t.page_border), StrokeKind::Inside);
    // The colours of the template's theme, where it has one: the first for a shape's fill, the second for the second shape's.
    let colour = |n: usize, fallback: egui::Color32| card.accents.get(n).map_or(fallback, |(r, g, b)| theme::document_colour(*r, *g, *b));
    let (first, second) = (colour(0, t.accent_soft), colour(1, t.accent_soft));
    let ink = Stroke::new(1.0, t.icon_ink);
    let inner = page.shrink2(vec2(size.x * 0.18, size.y * 0.12));
    if card.category.eq_ignore_ascii_case("flowchart") {
        flowchart(painter, inner, first, second, ink);
    } else {
        boxes(painter, inner, first, second, ink);
    }
}

/// Three steps down the sheet: a box, a diamond (a decision) and a box, joined by lines.
fn flowchart(painter: &Painter, inner: Rect, first: egui::Color32, second: egui::Color32, ink: Stroke) {
    let (w, step) = (inner.width() * 0.55, inner.height() / 3.0);
    let cx = inner.center().x;
    let band = |n: f32| Rect::from_min_size(pos2(cx - w / 2.0, inner.top() + step * n + step * 0.12), vec2(w, step * 0.62));
    let (top, mid, low) = (band(0.0), band(1.0), band(2.0));
    painter.rect(top, CornerRadius::same(2), first, ink, StrokeKind::Inside);
    let diamond: Vec<Pos2> = vec![mid.center_top(), mid.right_center(), mid.center_bottom(), mid.left_center()];
    painter.add(Shape::convex_polygon(diamond, second, ink));
    painter.rect(low, CornerRadius::same(2), first, ink, StrokeKind::Inside);
    painter.line_segment([top.center_bottom(), mid.center_top()], ink);
    painter.line_segment([mid.center_bottom(), low.center_top()], ink);
}

/// Two boxes side by side joined by a line, and a third under them.
fn boxes(painter: &Painter, inner: Rect, first: egui::Color32, second: egui::Color32, ink: Stroke) {
    let (w, h) = (inner.width() * 0.38, inner.height() * 0.32);
    let left = Rect::from_min_size(inner.left_top(), vec2(w, h));
    let right = Rect::from_min_size(pos2(inner.right() - w, inner.top()), vec2(w, h));
    let low = Rect::from_min_size(pos2(inner.center().x - w / 2.0, inner.bottom() - h), vec2(w, h));
    painter.line_segment([left.right_center(), right.left_center()], ink);
    painter.line_segment([left.center_bottom(), pos2(left.center().x, low.center().y)], ink);
    painter.line_segment([pos2(left.center().x, low.center().y), low.left_center()], ink);
    painter.rect(left, CornerRadius::same(2), first, ink, StrokeKind::Inside);
    painter.rect(right, CornerRadius::same(2), second, ink, StrokeKind::Inside);
    painter.rect(low, CornerRadius::same(2), first, ink, StrokeKind::Inside);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hex_colour_is_read_and_anything_else_is_not() {
        assert_eq!(parse_hex("#336699"), Some((0x33, 0x66, 0x99)));
        assert_eq!(parse_hex("#FFffFF"), Some((255, 255, 255)));
        for bad in ["336699", "#33669", "#3366999", "#gg0000", "", "#", "#33 699"] {
            assert_eq!(parse_hex(bad), None, "{bad:?}");
        }
    }

    /// A pair of digits is two hexadecimal digits, and not a sign and one digit: `u8::from_str_radix` takes `+f` for 15, so `#+f+f+f` was read as a grey (
    /// the counterexample of `a_tiles_colour_is_read_exactly_from_a_hash_and_six_hexadecimal_digits`).
    #[test]
    fn a_sign_among_the_digits_is_no_colour() {
        for bad in ["#+f+f+f", "#+0+0+0", "#33+699", "#3366+9", "#-1-1-1"] {
            assert_eq!(parse_hex(bad), None, "{bad:?}");
        }
    }

    /// The cards follow the engine's list: its order, names, categories and page shapes, with the colours of the template's theme and none for a template with no theme;
    /// an entry without an id or a name is left out and a list that is no list is empty.
    #[test]
    fn the_cards_follow_the_engines_list() {
        let list = json!({"templates": [
            {"id": "a", "name": "Alpha", "category": "General", "description": "d", "page": {"width": 11.0, "height": 8.5}, "theme": null},
            {"id": "b", "name": "Beta", "category": "Flowchart", "description": "e", "page": {"width": 8.5, "height": 11.0}, "theme": {"id": "t1", "name": "T", "variant": 0}},
            {"name": "No id"},
            {"id": "c"},
        ]});
        let themes = json!({"themes": [{"id": "t1", "colours": {"accents": ["#102030", "#405060", "#708090", "#a0b0c0"]}}, {"id": "t2", "colours": {"accents": ["#000000"]}}]});
        let cards = cards_from(&list, Some(&themes));
        assert_eq!(cards.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["a", "b"]);
        assert_eq!((cards[0].name.as_str(), cards[0].category.as_str(), cards[0].page), ("Alpha", "General", (11.0, 8.5)));
        assert!(cards[0].accents.is_empty(), "no theme, no colours");
        assert_eq!(cards[1].accents, [(0x10, 0x20, 0x30), (0x40, 0x50, 0x60), (0x70, 0x80, 0x90)], "the first three accents of the theme");
        assert_eq!(cards_from(&list, None)[1].accents, [], "and none when the themes could not be listed");
        assert!(cards_from(&json!({}), None).is_empty() && cards_from(&json!({"templates": 3}), None).is_empty());
    }

    /// A gallery is capped: a list of thousands is cut at the most the page shows.
    #[test]
    fn the_list_is_capped() {
        let many: Vec<Value> = (0..MAX_TEMPLATES + 40).map(|n| json!({"id": format!("t{n}"), "name": format!("T{n}")})).collect();
        assert_eq!(cards_from(&json!({ "templates": many }), None).len(), MAX_TEMPLATES);
    }
}
