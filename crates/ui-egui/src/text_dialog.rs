//! The Text dialog ("F11 or the group's launcher opens the Text dialog for further settings";
//! `FontDialog` and `ParagraphDialog`).
//!
//! Three tabs: **Font** (the face, the size, bold, italic, underline, strikethrough and the colour), **Paragraph** (the horizontal alignment) and
//! **Text Block** (the vertical alignment; the margins and the background are drawn quiet, since no command sets them: `docs/editing-commands.md`, "What is left").
//! **The dialog works on a draft and applies on OK** (ours, as Page Setup does): what was changed goes in **one `text.format`** (the window's `format.text`), so
//! one undo step, and nothing for what was left alone. **With a text box open the dialog formats the characters selected in it**: `format.text` sends
//! what was typed, formats the range and opens the box again with the same selection. What a control shows is what the selection has (a style that some
//! of the shapes or characters have is half pressed, a value they differ in is blank), read when the dialog opens.

use diacraft_engine::formatting::Rgb;
use egui::{Context, Id, Key, Modifiers, RichText, Sense, Stroke, StrokeKind, Ui, vec2};
use serde_json::{Map, Value, json};

use crate::app::DiacraftApp;
use crate::format::{Mix, Pressed};
use crate::measure::number_text;
use crate::ribbon::pickers::palette_colours;
use crate::ribbon::spec::Press;
use crate::theme::{self, Tokens, size};

/// The tabs of the dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextTab {
    Font,
    Paragraph,
    TextBlock,
}

impl TextTab {
    pub const ALL: [TextTab; 3] = [TextTab::Font, TextTab::Paragraph, TextTab::TextBlock];

    pub const fn name(self) -> &'static str {
        match self {
            TextTab::Font => "Font",
            TextTab::Paragraph => "Paragraph",
            TextTab::TextBlock => "Text Block",
        }
    }

    pub fn from_name(name: &str) -> Option<TextTab> {
        TextTab::ALL.into_iter().find(|t| t.name().eq_ignore_ascii_case(name.trim()))
    }
}

/// What the selection has, read when the dialog opens: what each control starts from.
#[derive(Clone, Debug)]
struct Start {
    font: Option<String>,
    size: Option<f64>,
    colour: Option<Rgb>,
    bold: Pressed,
    italic: Pressed,
    underline: Pressed,
    strike: Pressed,
    align: Option<&'static str>,
    valign: Option<&'static str>,
}

/// What the person changed: `None` is left alone.
#[derive(Clone, Debug, Default)]
struct Draft {
    font: Option<String>,
    size: Option<String>,
    colour: Option<String>,
    bold: Option<bool>,
    italic: Option<bool>,
    underline: Option<bool>,
    strike: Option<bool>,
    align: Option<&'static str>,
    valign: Option<&'static str>,
}

/// The Text dialog while it is open.
pub struct TextDialog {
    pub(crate) tab: TextTab,
    start: Start,
    draft: Draft,
    filter: String,
    error: Option<String>,
}

/// How a tri-state style shows and what a press makes of it: half pressed (the shapes differ) goes on, on goes off, off goes on.
fn next(state: Pressed) -> bool {
    state != Pressed::On
}

impl DiacraftApp {
    /// Whether the Text dialog is open, and on which tab.
    pub fn text_dialog_tab(&self) -> Option<&'static str> {
        self.ui.text_dialog.as_ref().map(|d| d.tab.name())
    }

    /// Opens the Text dialog on `tab`; one that is open changes tab.
    pub(crate) fn open_text_dialog(&mut self, tab: TextTab) -> diacraft_engine::Result<()> {
        if let Some(dialog) = self.ui.text_dialog.as_mut() {
            dialog.tab = tab;
            return Ok(());
        }
        self.command_enabled("format.text")
            .map_err(|reason| diacraft_engine::EngineError::Disabled { cmd: "window.textDialog".to_owned(), reason })?;
        self.refresh_format_view();
        let rgb = match self.text_colour_mix() {
            Mix::One(c) => Some(c),
            _ => None,
        };
        let font = match self.font_mix() {
            Mix::One(f) => f,
            _ => None,
        };
        let size = match self.size_mix() {
            Mix::One(t) => Some(t as f64 / 1000.0),
            _ => None,
        };
        let one = |p: Press| self.press_state(p);
        let align = [
            (Press::Left, "left"),
            (Press::Center, "center"),
            (Press::Right, "right"),
            (Press::Justify, "justify"),
            (Press::Distributed, "distributed"),
        ]
        .into_iter()
        .find(|(p, _)| self.press_state(*p) == Pressed::On)
        .map(|(_, w)| w);
        let valign = [(Press::Top, "top"), (Press::Middle, "middle"), (Press::Bottom, "bottom")]
            .into_iter()
            .find(|(p, _)| self.press_state(*p) == Pressed::On)
            .map(|(_, w)| w);
        let start = Start {
            font,
            size,
            colour: rgb,
            bold: one(Press::Bold),
            italic: one(Press::Italic),
            underline: one(Press::Underline),
            strike: one(Press::Strike),
            align,
            valign,
        };
        self.ui.text_dialog = Some(TextDialog { tab, start, draft: Draft::default(), filter: String::new(), error: None });
        Ok(())
    }

    pub(crate) fn close_text_dialog(&mut self) {
        self.ui.text_dialog = None;
    }

    /// The `format.text` parameters of what was changed. Words for a size that is not a number.
    fn text_dialog_params(&self) -> Result<Map<String, Value>, String> {
        let Some(dialog) = &self.ui.text_dialog else { return Ok(Map::new()) };
        let d = &dialog.draft;
        let mut p = Map::new();
        if let Some(font) = d.font.as_deref().map(str::trim).filter(|f| !f.is_empty()) {
            p.insert("font".to_owned(), json!(font));
        }
        if let Some(text) = &d.size {
            let points: f64 = text.trim().parse().map_err(|_| "A size is a number of points, from more than 0 to 1000.".to_owned())?;
            if !(points.is_finite() && points > 0.0 && points <= diacraft_engine::formatting::MAX_SIZE_POINTS) {
                return Err("A size is a number of points, from more than 0 to 1000.".to_owned());
            }
            p.insert("size".to_owned(), json!(points));
        }
        if let Some(colour) = &d.colour {
            let rgb = Rgb::parse(colour.trim())
                .ok_or_else(|| "A colour is written #RRGGBB: three pairs of hexadecimal digits (like #3C78D8).".to_owned())?;
            p.insert("color".to_owned(), json!(rgb.hex()));
        }
        for (key, value) in [("bold", d.bold), ("italic", d.italic), ("underline", d.underline), ("strike", d.strike)] {
            if let Some(on) = value {
                p.insert(key.to_owned(), json!(on));
            }
        }
        if let Some(a) = d.align {
            p.insert("align".to_owned(), json!(a));
        }
        if let Some(v) = d.valign {
            p.insert("valign".to_owned(), json!(v));
        }
        Ok(p)
    }

    fn apply_text_dialog(&mut self) {
        let params = match self.text_dialog_params() {
            Ok(p) => p,
            Err(words) => {
                if let Some(d) = self.ui.text_dialog.as_mut() {
                    d.error = Some(words);
                }
                return;
            }
        };
        if params.is_empty() {
            self.close_text_dialog();
            return;
        }
        match self.execute("format.text", &Value::Object(params)) {
            Ok(_) => self.close_text_dialog(),
            Err(e) => {
                if let Some(d) = self.ui.text_dialog.as_mut() {
                    d.error = Some(e.to_string());
                }
            }
        }
    }

    pub(crate) fn text_dialog(&mut self, ctx: &Context, t: Tokens) {
        let screen = ctx.content_rect();
        let width = (screen.width() - 60.0).clamp(280.0, 460.0);
        // The Font tab (the face, its list, the size, the styles, the whole palette and Other) is about 420 points tall and the card's frame about 130 more: it
        // shows whole in a window 620 points tall or more. In a shorter window the tab scrolls, and the scroll bar is drawn (not the one that shows only when
        // the pointer is over it), so that nothing is hidden without a sign.
        let content_h = (screen.height() - 170.0).clamp(150.0, 440.0);
        let typing = ctx.text_edit_focused();
        // The machine's faces are found on a thread of their own the first time they are wanted (`font_list.rs`).
        self.ui.format.fonts.start(ctx);
        let Some(mut dialog) = self.ui.text_dialog.take() else { return };
        // The face the shapes are drawn in is read again when the machine's faces come in while the dialog is open (the view's key holds whether they are in
        // hand), unless the person has chosen a face: the box names what is drawn, not what it named when the dialog opened.
        if dialog.draft.font.is_none()
            && let Mix::One(font) = self.font_mix()
        {
            dialog.start.font = font;
        }
        let (mut ok, mut cancel) = (false, false);
        let mut seen: Vec<(String, egui::Rect, bool)> = Vec::new();
        let fonts = self.ui.format.fonts.families();
        let modal = egui::Modal::new(Id::unique("text_dialog")).backdrop_color(t.scrim).frame(Self::card_frame(&t)).show(ctx, |ui| {
            ui.set_width(width);
            ui.label(RichText::new("Text").font(theme::font(size::TAB + 1.0)).color(t.text));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                for tab in TextTab::ALL {
                    let r =
                        ui.add(egui::Button::selectable(dialog.tab == tab, RichText::new(tab.name()).font(theme::font(size::LABEL)).color(t.text)));
                    seen.push((format!("textdialog.tab.{}", tab.name()), r.rect, true));
                    if r.clicked() {
                        dialog.tab = tab;
                    }
                }
            });
            ui.add_space(6.0);
            ui.style_mut().spacing.scroll = egui::style::ScrollStyle::solid();
            let content = egui::ScrollArea::vertical()
                .id_salt("text_dialog_content")
                .min_scrolled_height(content_h)
                .max_height(content_h)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_width(width - 14.0);
                    match dialog.tab {
                        TextTab::Font => font_tab(ui, &t, &mut dialog, fonts.as_deref().map(Vec::as_slice), &mut seen),
                        TextTab::Paragraph => paragraph_tab(ui, &t, &mut dialog, &mut seen),
                        TextTab::TextBlock => block_tab(ui, &t, &mut dialog, &mut seen),
                    }
                });
            seen.push(("textdialog.content".to_owned(), content.inner_rect, true));
            ui.add_space(6.0);
            if let Some(error) = &dialog.error {
                ui.add(egui::Label::new(RichText::new(error).font(theme::font(size::LABEL)).color(t.notice_text)).wrap());
                ui.add_space(4.0);
            }
            ui.horizontal(|ui| {
                for (label, key, primary) in [("OK", "textdialog.ok", true), ("Cancel", "textdialog.cancel", false)] {
                    let b = ui.add(egui::Button::new(RichText::new(label).font(theme::font(size::LABEL))).min_size(vec2(84.0, 26.0)));
                    seen.push((key.to_owned(), b.rect, true));
                    if b.clicked() {
                        if primary { ok = true } else { cancel = true }
                    }
                }
            });
        });
        crate::ask::keep_centred(ctx, modal.response.rect);
        for (name, rect, enabled) in seen {
            self.note(name, rect, enabled);
        }
        if !egui::Popup::is_any_open(ctx) {
            if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
                cancel = true;
            }
            if !typing && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter)) {
                ok = true;
            }
        }
        self.ui.text_dialog = Some(dialog);
        if cancel {
            self.close_text_dialog();
        } else if ok {
            self.apply_text_dialog();
        }
    }
}

fn heading(ui: &mut Ui, t: &Tokens, words: &str) {
    ui.label(RichText::new(words).font(theme::font(size::LABEL)).color(t.text_dim));
}

/// A style box that has three states: pressed, not pressed and half pressed (some of the shapes have it). A press makes it on, or off when it was on.
fn style_box(ui: &mut Ui, t: &Tokens, label: &str, name: &str, start: Pressed, draft: &mut Option<bool>, seen: &mut Vec<(String, egui::Rect, bool)>) {
    let state = match draft {
        Some(true) => Pressed::On,
        Some(false) => Pressed::Off,
        None => start,
    };
    let (rect, resp) = ui.allocate_exact_size(vec2(122.0, 22.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, state == Pressed::On, label));
    let b = egui::Rect::from_center_size(egui::pos2(rect.left() + 9.0, rect.center().y), vec2(14.0, 14.0));
    ui.painter().rect(b, 2.0, if state == Pressed::On { t.accent } else { t.input_fill }, Stroke::new(1.0, t.input_border), StrokeKind::Inside);
    match state {
        Pressed::On => {
            let (a, m, z) = (b.left_top() + vec2(3.0, 7.0), b.left_top() + vec2(6.0, 10.0), b.left_top() + vec2(11.0, 4.0));
            ui.painter().line_segment([a, m], Stroke::new(1.6, t.accent_text));
            ui.painter().line_segment([m, z], Stroke::new(1.6, t.accent_text));
        }
        Pressed::Part => {
            ui.painter().line_segment([b.left_center() + vec2(3.0, 0.0), b.right_center() - vec2(3.0, 0.0)], Stroke::new(2.0, t.text_dim));
        }
        Pressed::Off => {}
    }
    ui.painter().text(egui::pos2(rect.left() + 24.0, rect.center().y), egui::Align2::LEFT_CENTER, label, theme::font(size::LABEL), t.text);
    seen.push((name.to_owned(), rect, true));
    if resp.clicked() {
        *draft = Some(next(state));
    }
}

fn font_tab(ui: &mut Ui, t: &Tokens, d: &mut TextDialog, fonts: Option<&[String]>, seen: &mut Vec<(String, egui::Rect, bool)>) {
    heading(ui, t, "Font");
    let shown = d.draft.font.clone().or_else(|| d.start.font.clone()).unwrap_or_default();
    let mut face = if d.filter.is_empty() && d.draft.font.is_none() { shown.clone() } else { d.draft.font.clone().unwrap_or_default() };
    let r = ui.add(egui::TextEdit::singleline(&mut face).desired_width(240.0).hint_text("Face").font(theme::font(size::LABEL)));
    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Font face"));
    seen.push(("textdialog.font".to_owned(), r.rect, true));
    if r.gained_focus() {
        // What is typed replaces the face the box names.
        let mut state = egui::text_edit::TextEditState::load(ui.ctx(), r.id).unwrap_or_default();
        state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(face.chars().count()))));
        state.store(ui.ctx(), r.id);
    }
    if r.changed() {
        d.filter = face.clone();
        d.draft.font = Some(face.clone());
    }
    // The list is narrowed by what the person typed, never by the face the box names (which is the face the shapes are drawn in, always, and would leave a list of
    // the one face).
    let needle = d.filter.trim().to_lowercase();
    let list = egui::ScrollArea::vertical().id_salt("text_dialog_fonts").min_scrolled_height(90.0).max_height(90.0).auto_shrink([false, false]).show(
        ui,
        |ui| match fonts {
            None => {
                ui.label(RichText::new("Loading fonts").font(theme::font(size::LABEL)).color(t.text_dim));
            }
            Some(all) => {
                let matching: Vec<&String> = all.iter().filter(|name| needle.is_empty() || name.to_lowercase().contains(&needle)).take(200).collect();
                if matching.is_empty() {
                    ui.label(RichText::new("No font has that in its name").font(theme::font(size::LABEL)).color(t.text_dim));
                }
                for name in matching {
                    let selected =
                        d.draft.font.as_deref() == Some(name.as_str()) || (d.draft.font.is_none() && d.start.font.as_deref() == Some(name.as_str()));
                    let row = ui.add(
                        egui::Button::selectable(selected, RichText::new(name).font(theme::font(size::LABEL)).color(t.text))
                            .min_size(vec2(ui.available_width(), 20.0)),
                    );
                    seen.push((format!("textdialog.font.item.{name}"), row.rect, true));
                    if row.clicked() {
                        d.draft.font = Some(name.clone());
                        d.filter = name.clone();
                    }
                }
            }
        },
    );
    seen.push(("textdialog.font.list".to_owned(), list.inner_rect, true));
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        heading(ui, t, "Size (points)");
        let mut text = d.draft.size.clone().unwrap_or_else(|| d.start.size.map(number_text).unwrap_or_default());
        let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(70.0).font(theme::font(size::LABEL)));
        r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Font size"));
        seen.push(("textdialog.size".to_owned(), r.rect, true));
        if r.changed() {
            d.draft.size = Some(text);
        }
    });
    ui.add_space(6.0);
    heading(ui, t, "Style");
    ui.horizontal(|ui| {
        style_box(ui, t, "Bold", "textdialog.bold", d.start.bold, &mut d.draft.bold, seen);
        style_box(ui, t, "Italic", "textdialog.italic", d.start.italic, &mut d.draft.italic, seen);
    });
    ui.horizontal(|ui| {
        style_box(ui, t, "Underline", "textdialog.underline", d.start.underline, &mut d.draft.underline, seen);
        style_box(ui, t, "Strikethrough", "textdialog.strike", d.start.strike, &mut d.draft.strike, seen);
    });
    ui.add_space(6.0);
    heading(ui, t, "Color");
    let current = d.draft.colour.clone().or_else(|| d.start.colour.map(|c| c.hex()));
    let colours = palette_colours();
    egui::Grid::new("text_dialog_palette").spacing(vec2(2.0, 2.0)).show(ui, |ui| {
        for (n, rgb) in colours.iter().enumerate() {
            let (rect, resp) = ui.allocate_exact_size(vec2(15.0, 15.0), Sense::click());
            let hex = rgb.hex();
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &hex));
            ui.painter().rect_filled(rect, 2.0, theme::document_colour(rgb.0, rgb.1, rgb.2));
            ui.painter().rect_stroke(rect, 2.0, Stroke::new(1.0, t.input_border), StrokeKind::Inside);
            if current.as_deref() == Some(hex.as_str()) {
                ui.painter().rect_stroke(rect.expand(2.0), 3.0, Stroke::new(2.0, t.accent), StrokeKind::Inside);
            }
            seen.push((format!("textdialog.colour.{hex}"), rect, true));
            if resp.clicked() {
                d.draft.colour = Some(hex);
            }
            if (n + 1) % 10 == 0 {
                ui.end_row();
            }
        }
    });
    ui.horizontal(|ui| {
        heading(ui, t, "Other");
        let mut text = d.draft.colour.clone().unwrap_or_default();
        let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(86.0).hint_text("#RRGGBB").font(theme::font(size::LABEL)));
        r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Other colour"));
        seen.push(("textdialog.colour.hex".to_owned(), r.rect, true));
        if r.changed() {
            d.draft.colour = if text.trim().is_empty() { None } else { Some(text) };
        }
    });
}

fn paragraph_tab(ui: &mut Ui, t: &Tokens, d: &mut TextDialog, seen: &mut Vec<(String, egui::Rect, bool)>) {
    heading(ui, t, "Horizontal alignment");
    let now = d.draft.align.or(d.start.align);
    for (word, label) in [("left", "Left"), ("center", "Center"), ("right", "Right"), ("justify", "Justify"), ("distributed", "Distributed")] {
        let r = ui.radio(now == Some(word), RichText::new(label).font(theme::font(size::LABEL)).color(t.text));
        seen.push((format!("textdialog.align.{word}"), r.rect, true));
        if r.clicked() {
            d.draft.align = Some(word);
        }
    }
}

fn block_tab(ui: &mut Ui, t: &Tokens, d: &mut TextDialog, seen: &mut Vec<(String, egui::Rect, bool)>) {
    heading(ui, t, "Vertical alignment");
    let now = d.draft.valign.or(d.start.valign);
    for (word, label) in [("top", "Top"), ("middle", "Middle"), ("bottom", "Bottom")] {
        let r = ui.radio(now == Some(word), RichText::new(label).font(theme::font(size::LABEL)).color(t.text));
        seen.push((format!("textdialog.valign.{word}"), r.rect, true));
        if r.clicked() {
            d.draft.valign = Some(word);
        }
    }
    ui.add_space(8.0);
    heading(ui, t, "Margins and background: no command sets them yet");
    ui.add_enabled_ui(false, |ui| {
        for (label, name) in [("Left margin", "left"), ("Right margin", "right"), ("Top margin", "top"), ("Bottom margin", "bottom")] {
            ui.horizontal(|ui| {
                ui.allocate_ui_with_layout(vec2(96.0, 22.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.set_min_size(vec2(96.0, 22.0));
                    ui.label(label);
                });
                let mut shown = String::new();
                let r = ui.add(egui::TextEdit::singleline(&mut shown).desired_width(70.0));
                r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, false, label));
                seen.push((format!("textdialog.margin.{name}"), r.rect, false));
            });
        }
    });
}
