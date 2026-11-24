//! The Colors dialog: the **More Colors...** row of the Fill, Line and Font Color menus and of the Format Shape pane's colour buttons.
//!
//! **What the specification gives** (`pictured` for the layout, `documented` for the use): a dialog titled Colors with two tabs, **Standard**, a palette of colour cells, and **Custom**, a
//! colour field with a brightness bar, a **Color model** list, the three numbers of the model (Red, Green and Blue in the image), a **Hex** box, a **New** and a **Current** swatch, and OK and
//! Cancel; a colour may be named by its hexadecimal code, by its RGB value or by a pick on the colour field. Whether the list holds more than RGB is not shown (Q), so **ours has two models, RGB and
//! HSL**; the grid of the Standard tab, 12 hues in six lightnesses and a row of greys, is made in code and is ours too, as the Custom tab's field is: its
//! horizontal is the hue, its vertical the saturation, and the bar at its right the lightness.
//!
//! **What OK does**: one `format.shape` (`fill` or `line`), `format.text` (`color`), or `format.gradient`, `format.shadow` for the pane's other colour buttons, with the colour as `#rrggbb`: **a fixed colour**,
//! which no theme changes, and it enters the Recent Colors. Cancel and Escape change nothing; Enter is OK while no box has the keyboard. A hex text that is no colour is refused in words and
//! the dialog stays. The dialog opens on the colour the selection has (the **Current** swatch), and on **New**, the same, until something is picked.
//!
//! The window command is `window.colorsDialog {target, close?}`; `target` is the palette's key (`fill`, `line`, `color`, `stop_color`, `line_stop_color`, `fill_background`, `shadow_color`).

use diacraft_engine::formatting::Rgb;
use diacraft_engine::{EngineError, Result};
use egui::{Context, CornerRadius, Id, Key, Modifiers, Rect, RichText, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use serde_json::{Value, json};

use crate::app::DiacraftApp;
use crate::format::Mix;
use crate::theme::{self, Tokens, size};

/// The text box that was open: the page, the shape and the characters selected in it.
type TextTarget = (u32, u32, Option<(usize, usize)>);

/// The tabs of the dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Standard,
    Custom,
}

/// The colour model of the Custom tab's three numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Model {
    Rgb,
    Hsl,
}

/// The Colors dialog while it is open.
#[derive(Clone, Debug)]
pub struct ColorsDialog {
    /// The key of the palette it was opened from.
    target: &'static str,
    /// The command OK runs, with the colour under `target`.
    command: &'static str,
    tab: Tab,
    model: Model,
    /// What the selection has: the **Current** swatch.
    current: Option<Rgb>,
    /// What is picked: the **New** swatch.
    new: Rgb,
    /// The hue (0 to 360), saturation and lightness (0 to 1) of `new`, kept beside it so that a grey does not lose its hue while the lightness moves.
    hsl: (f64, f64, f64),
    /// What the Hex box holds.
    hex: String,
    error: Option<String>,
    /// The text box that was open when a text colour's dialog was asked for: the page, the shape and the characters selected in it. The box was closed to show the dialog and what was
    /// typed in it sent; OK formats those characters, and the box opens again over them when the dialog ends, whichever way.
    text: Option<TextTarget>,
}

/// The command and the key it takes the colour under, for the palette key `target`.
fn command_for(target: &str) -> Option<(&'static str, &'static str)> {
    Some(match target {
        "fill" => ("format.shape", "fill"),
        "line" => ("format.shape", "line"),
        "fill_background" => ("format.shape", "fill_background"),
        "color" => ("format.text", "color"),
        "stop_color" => ("format.gradient", "stop_color"),
        "line_stop_color" => ("format.gradient", "line_stop_color"),
        "shadow_color" => ("format.shadow", "shadow_color"),
        _ => return None,
    })
}

/// A colour of the model: hue 0 to 360, saturation and lightness 0 to 1 → its red, green and blue.
pub fn hsl_to_rgb(h: f64, s: f64, l: f64) -> Rgb {
    let (s, l) = (s.clamp(0.0, 1.0), l.clamp(0.0, 1.0));
    let h = if h.is_finite() { h.rem_euclid(360.0) } else { 0.0 };
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match (h / 60.0).floor() as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let byte = |v: f64| ((v + m).clamp(0.0, 1.0) * 255.0).round() as u8;
    Rgb(byte(r), byte(g), byte(b))
}

/// A colour's hue (0 to 360), saturation and lightness (0 to 1).
pub fn rgb_to_hsl(Rgb(r, g, b): Rgb) -> (f64, f64, f64) {
    let (r, g, b) = (f64::from(r) / 255.0, f64::from(g) / 255.0, f64::from(b) / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let (l, d) = ((max + min) / 2.0, max - min);
    if d == 0.0 {
        return (0.0, 0.0, l);
    }
    let s = d / (1.0 - (2.0 * l - 1.0).abs());
    let h = if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    (h, s.clamp(0.0, 1.0), l)
}

/// The cells of the Standard tab, row by row: a row of greys from white to black, then six lightnesses of twelve hues.
pub const STANDARD_COLUMNS: usize = 12;
const STANDARD_LIGHTNESS: [f64; 6] = [0.86, 0.72, 0.58, 0.46, 0.34, 0.22];

/// The colours of the Standard tab, row by row.
pub fn standard_cells() -> Vec<Rgb> {
    let mut out: Vec<Rgb> = (0..STANDARD_COLUMNS)
        .map(|i| {
            let v = (255.0 * (1.0 - i as f64 / (STANDARD_COLUMNS - 1) as f64)).round() as u8;
            Rgb(v, v, v)
        })
        .collect();
    for l in STANDARD_LIGHTNESS {
        out.extend((0..STANDARD_COLUMNS).map(|c| hsl_to_rgb(30.0 * c as f64, 0.78, l)));
    }
    out
}

const CELL: f32 = 20.0;
const CELL_GAP: f32 = 2.0;
const FIELD_W: f32 = 200.0;
const FIELD_H: f32 = 150.0;
const BAR_W: f32 = 22.0;

fn bad(cmd: &str, msg: impl Into<String>) -> EngineError {
    EngineError::BadParams { cmd: cmd.to_owned(), msg: msg.into() }
}

impl DiacraftApp {
    /// The dialog's state for the window state and the tests: `null` when it is closed.
    pub(crate) fn colors_dialog_state(&self) -> Value {
        let Some(d) = &self.ui.colors_dialog else { return Value::Null };
        json!({
            "target": d.target,
            "tab": match d.tab { Tab::Standard => "standard", Tab::Custom => "custom" },
            "model": match d.model { Model::Rgb => "rgb", Model::Hsl => "hsl" },
            "new": d.new.hex(),
            "current": d.current.map(Rgb::hex),
            "hex": d.hex,
            "error": d.error,
        })
    }

    /// Opens the dialog for the palette key `target`, on the colour the selection has. One that is open stays as it is.
    pub(crate) fn open_colors_dialog(&mut self, target: &str) -> Result<()> {
        const ID: &str = "window.colorsDialog";
        if self.ui.colors_dialog.is_some() {
            return Ok(());
        }
        let (command, key) = command_for(target).ok_or_else(|| {
            bad(
                ID,
                format!(
                    "`target` must be fill, line, color, stop_color, line_stop_color, fill_background or shadow_color; not `{}`",
                    diacraft_engine::shown(target, 32)
                ),
            )
        })?;
        let current = match crate::ribbon::pickers::colour_of(self, key) {
            Mix::One(rgb) => rgb,
            _ => None,
        };
        let new = current.unwrap_or(Rgb(0x1E, 0x6F, 0xD9));
        // A text box that is open: what the colour is for is the characters selected in it (what `format.text` does with the box the same way).
        let text = if key == "color" { self.open_text_selection().map(|open| (open.page, open.id, open.range)) } else { None };
        if text.is_some() {
            self.commit_pending_text();
        }
        self.ui.colors_dialog = Some(ColorsDialog {
            target: key,
            command,
            tab: Tab::Standard,
            model: Model::Rgb,
            current,
            new,
            hsl: rgb_to_hsl(new),
            hex: new.hex(),
            error: None,
            text,
        });
        Ok(())
    }

    /// Closes the dialog; a text box that was open when it was asked for opens again, over the same characters.
    pub(crate) fn close_colors_dialog(&mut self) {
        if let Some(dialog) = self.ui.colors_dialog.take()
            && let Some((page, id, range)) = dialog.text
        {
            self.reopen_text(page, id, range);
        }
    }

    /// OK: the command with the colour, as a fixed colour, and the colour is the newest of the Recent Colors. A refusal stays in the dialog, in words.
    fn apply_colors_dialog(&mut self, ctx: &Context) {
        let Some((command, key, new, text)) = self.ui.colors_dialog.as_ref().map(|d| (d.command, d.target, d.new, d.text)) else { return };
        let result = match text {
            // The characters that were selected in the box that was open: `text.format` of that range.
            Some((page, id, range)) => {
                let mut params = json!({ "ids": [id], "page": page, key: new.hex() });
                let within = range.filter(|(from, to)| from < to && self.text_chars(page, id).is_some_and(|n| *to <= n));
                if let (Some((from, to)), Some(map)) = (within, params.as_object_mut()) {
                    map.insert("start".to_owned(), json!(from));
                    map.insert("end".to_owned(), json!(to));
                }
                self.execute("text.format", &params)
            }
            None => self.execute(command, &json!({ key: new.hex() })),
        };
        match result {
            Ok(_) => {
                self.remember_colour(new);
                self.close_colors_dialog();
                ctx.request_repaint();
            }
            Err(e) => {
                if let Some(d) = self.ui.colors_dialog.as_mut() {
                    d.error = Some(e.to_string());
                }
            }
        }
    }

    pub(crate) fn colors_dialog(&mut self, ctx: &Context, t: Tokens) {
        let Some(mut dialog) = self.ui.colors_dialog.take() else { return };
        let typing = ctx.text_edit_focused();
        let (mut ok, mut cancel) = (false, false);
        let mut seen: Vec<(String, Rect, bool)> = Vec::new();
        let screen = ctx.content_rect();
        let width = (screen.width() - 40.0).clamp(260.0, 470.0);
        let modal = egui::Modal::new(Id::unique("colors_dialog")).backdrop_color(t.scrim).frame(Self::card_frame(&t)).show(ctx, |ui| {
            ui.set_width(width);
            ui.label(RichText::new("Colors").font(theme::font(size::TAB + 1.0)).color(t.text));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                for (tab, label, key) in [(Tab::Standard, "Standard", "colors.tab.standard"), (Tab::Custom, "Custom", "colors.tab.custom")] {
                    let b = ui.add(
                        egui::Button::new(RichText::new(label).font(theme::font(size::LABEL)).color(t.text))
                            .selected(dialog.tab == tab)
                            .min_size(vec2(84.0, 24.0)),
                    );
                    seen.push((key.to_owned(), b.rect, true));
                    if b.clicked() {
                        dialog.tab = tab;
                        dialog.error = None;
                    }
                }
            });
            ui.add_space(6.0);
            match dialog.tab {
                Tab::Standard => standard_tab(ui, &t, &mut dialog, &mut seen),
                Tab::Custom => custom_tab(ui, &t, &mut dialog, &mut seen),
            }
            ui.add_space(6.0);
            swatches(ui, &t, &dialog, &mut seen);
            if let Some(error) = &dialog.error {
                ui.add_space(4.0);
                let said = ui.add(egui::Label::new(RichText::new(error).font(theme::font(size::LABEL)).color(t.notice_text)).wrap());
                seen.push(("colors.error".to_owned(), said.rect, true));
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                for (label, key, primary) in [("OK", "colors.ok", true), ("Cancel", "colors.cancel", false)] {
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
        self.ui.colors_dialog = Some(dialog);
        if cancel {
            self.close_colors_dialog();
        } else if ok {
            self.apply_colors_dialog(ctx);
        }
    }
}

/// The colour picked, in every place it is held.
fn set_new(dialog: &mut ColorsDialog, rgb: Rgb) {
    dialog.new = rgb;
    dialog.hsl = rgb_to_hsl(rgb);
    dialog.hex = rgb.hex();
    dialog.error = None;
}

/// The colour picked as hue, saturation and lightness (a grey keeps the hue it had).
fn set_hsl(dialog: &mut ColorsDialog, hsl: (f64, f64, f64)) {
    dialog.hsl = hsl;
    dialog.new = hsl_to_rgb(hsl.0, hsl.1, hsl.2);
    dialog.hex = dialog.new.hex();
    dialog.error = None;
}

fn standard_tab(ui: &mut Ui, t: &Tokens, dialog: &mut ColorsDialog, seen: &mut Vec<(String, Rect, bool)>) {
    let cells = standard_cells();
    let rows = cells.len().div_ceil(STANDARD_COLUMNS);
    let (grid, _) = ui.allocate_exact_size(
        vec2(STANDARD_COLUMNS as f32 * (CELL + CELL_GAP) + CELL_GAP, rows as f32 * (CELL + CELL_GAP) + CELL_GAP),
        Sense::hover(),
    );
    let mut picked = None;
    for (n, rgb) in cells.iter().enumerate() {
        let (col, row) = (n % STANDARD_COLUMNS, n / STANDARD_COLUMNS);
        let rect = Rect::from_min_size(
            pos2(grid.left() + CELL_GAP + col as f32 * (CELL + CELL_GAP), grid.top() + CELL_GAP + row as f32 * (CELL + CELL_GAP)),
            vec2(CELL, CELL),
        );
        let resp = ui.interact(rect, ui.scope_id().with(("standard", n)), Sense::click());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, rgb.hex()));
        let painter = ui.painter();
        painter.rect_filled(rect, CornerRadius::same(2), theme::document_colour(rgb.0, rgb.1, rgb.2));
        painter.rect_stroke(rect, CornerRadius::same(2), Stroke::new(1.0, t.input_border), StrokeKind::Inside);
        if *rgb == dialog.new {
            painter.rect_stroke(rect.expand(2.0), CornerRadius::same(3), Stroke::new(2.0, t.accent), StrokeKind::Inside);
        } else if resp.hovered() {
            painter.rect_stroke(rect.expand(1.0), CornerRadius::same(2), Stroke::new(1.5, t.text), StrokeKind::Inside);
        }
        seen.push((format!("colors.standard.{}", rgb.hex()), rect, true));
        if resp.clicked() {
            picked = Some(*rgb);
        }
        resp.on_hover_text(rgb.hex());
    }
    if let Some(rgb) = picked {
        set_new(dialog, rgb);
    }
}

fn custom_tab(ui: &mut Ui, t: &Tokens, dialog: &mut ColorsDialog, seen: &mut Vec<(String, Rect, bool)>) {
    ui.horizontal_top(|ui| {
        // The colour field and the lightness bar.
        let (field, field_resp) = ui.allocate_exact_size(vec2(FIELD_W, FIELD_H), Sense::click_and_drag());
        let (bar, bar_resp) = ui.allocate_exact_size(vec2(BAR_W, FIELD_H), Sense::click_and_drag());
        field_resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Colour field: hue across, saturation down"));
        bar_resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Lightness"));
        let (h, s, l) = dialog.hsl;
        paint_field(ui.painter(), field, l, t);
        paint_bar(ui.painter(), bar, h, s, t);
        // The pointer on the field picks a hue and a saturation, and on the bar a lightness.
        if let Some(at) = field_resp.interact_pointer_pos().filter(|_| field_resp.is_pointer_button_down_on() || field_resp.clicked()) {
            let hue = f64::from(((at.x - field.left()) / field.width()).clamp(0.0, 1.0)) * 360.0;
            let sat = 1.0 - f64::from(((at.y - field.top()) / field.height()).clamp(0.0, 1.0));
            set_hsl(dialog, (hue.min(359.999), sat, l));
        }
        if let Some(at) = bar_resp.interact_pointer_pos().filter(|_| bar_resp.is_pointer_button_down_on() || bar_resp.clicked()) {
            let lum = 1.0 - f64::from(((at.y - bar.top()) / bar.height()).clamp(0.0, 1.0));
            set_hsl(dialog, (h, s, lum));
        }
        // Marks: where the colour is on the field and the bar.
        let mark = pos2(field.left() + field.width() * (h / 360.0) as f32, field.top() + field.height() * (1.0 - s) as f32);
        ui.painter().circle_stroke(mark, 5.0, Stroke::new(2.0, t.text));
        ui.painter().circle_stroke(mark, 6.5, Stroke::new(1.0, t.panel_fill));
        let at = bar.top() + bar.height() * (1.0 - l) as f32;
        ui.painter().line_segment([pos2(bar.left() - 2.0, at), pos2(bar.right() + 2.0, at)], Stroke::new(2.0, t.text));
        seen.push(("colors.field".to_owned(), field, true));
        seen.push(("colors.bar".to_owned(), bar, true));
        ui.add_space(8.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            // The colour model.
            ui.horizontal(|ui| {
                ui.label(RichText::new("Color model").font(theme::font(size::LABEL)).color(t.text));
                for (model, label, key) in [(Model::Rgb, "RGB", "colors.model.rgb"), (Model::Hsl, "HSL", "colors.model.hsl")] {
                    let b = ui.add(
                        egui::Button::new(RichText::new(label).font(theme::font(size::LABEL)).color(t.text))
                            .selected(dialog.model == model)
                            .min_size(vec2(44.0, 22.0)),
                    );
                    seen.push((key.to_owned(), b.rect, true));
                    if b.clicked() {
                        dialog.model = model;
                    }
                }
            });
            match dialog.model {
                Model::Rgb => {
                    let Rgb(r, g, b) = dialog.new;
                    let mut values = [f64::from(r), f64::from(g), f64::from(b)];
                    let mut changed = false;
                    for (n, (label, key)) in [("Red", "colors.red"), ("Green", "colors.green"), ("Blue", "colors.blue")].into_iter().enumerate() {
                        changed |= number_row(ui, t, label, key, &mut values[n], 0.0..=255.0, 1.0, seen);
                    }
                    if changed {
                        set_new(dialog, Rgb(values[0].round() as u8, values[1].round() as u8, values[2].round() as u8));
                    }
                }
                Model::Hsl => {
                    let (h, s, l) = dialog.hsl;
                    let mut values = [h, s * 100.0, l * 100.0];
                    let ranges = [0.0..=359.0, 0.0..=100.0, 0.0..=100.0];
                    let mut changed = false;
                    for (n, (label, key)) in
                        [("Hue", "colors.hue"), ("Saturation", "colors.saturation"), ("Lightness", "colors.lightness")].into_iter().enumerate()
                    {
                        changed |= number_row(ui, t, label, key, &mut values[n], ranges[n].clone(), 1.0, seen);
                    }
                    if changed {
                        set_hsl(dialog, (values[0], values[1] / 100.0, values[2] / 100.0));
                    }
                }
            }
            hex_row(ui, t, dialog, seen);
        });
    });
}

/// A labelled number box: a drag value with its arrows. → whether the person changed it.
#[allow(clippy::too_many_arguments)]
fn number_row(
    ui: &mut Ui,
    t: &Tokens,
    label: &str,
    key: &str,
    value: &mut f64,
    range: std::ops::RangeInclusive<f64>,
    speed: f64,
    seen: &mut Vec<(String, Rect, bool)>,
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.add_sized([70.0, 22.0], egui::Label::new(RichText::new(label).font(theme::font(size::LABEL)).color(t.text)));
        let r = ui.add(egui::DragValue::new(value).range(range).speed(speed).max_decimals(0));
        r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::SpinButton, true, label));
        seen.push((key.to_owned(), r.rect, true));
        changed = r.changed();
    });
    changed
}

/// The Hex box: a colour as `#rrggbb`, taken when the box loses the keyboard or Enter is pressed. What is not a colour is told in words.
fn hex_row(ui: &mut Ui, t: &Tokens, dialog: &mut ColorsDialog, seen: &mut Vec<(String, Rect, bool)>) {
    ui.horizontal(|ui| {
        ui.add_sized([70.0, 22.0], egui::Label::new(RichText::new("Hex").font(theme::font(size::LABEL)).color(t.text)));
        let mut out = egui::TextEdit::singleline(&mut dialog.hex).desired_width(86.0).char_limit(9).font(theme::font(size::LABEL)).show(ui);
        // The box takes the keyboard with its text selected, so that typing a colour replaces the one shown.
        if out.response.gained_focus() {
            let end = egui::text::CCursor::new(dialog.hex.chars().count());
            out.state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), end)));
            out.state.store(ui.ctx(), out.response.id);
        }
        let edit = out.response;
        edit.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Hex"));
        seen.push(("colors.hex".to_owned(), edit.rect, true));
        if edit.lost_focus() && dialog.hex.trim() != dialog.new.hex() {
            match Rgb::parse(dialog.hex.trim()) {
                Some(rgb) => set_new(dialog, rgb),
                None => {
                    dialog.error = Some("That is not a colour. Write it as #RRGGBB, three pairs of hexadecimal digits (like #3C78D8).".to_owned())
                }
            }
        }
    });
}

/// The New and the Current swatch side by side.
fn swatches(ui: &mut Ui, t: &Tokens, dialog: &ColorsDialog, seen: &mut Vec<(String, Rect, bool)>) {
    ui.horizontal(|ui| {
        for (label, key, colour) in [("New", "colors.new", Some(dialog.new)), ("Current", "colors.current", dialog.current)] {
            ui.label(RichText::new(label).font(theme::font(size::LABEL)).color(t.text));
            let (rect, resp) = ui.allocate_exact_size(vec2(44.0, 22.0), Sense::hover());
            resp.widget_info(|| {
                egui::WidgetInfo::labeled(egui::Role::Label, true, format!("{label} {}", colour.map_or_else(|| "none".to_owned(), Rgb::hex)))
            });
            match colour {
                Some(Rgb(r, g, b)) => {
                    ui.painter().rect_filled(rect, CornerRadius::same(2), theme::document_colour(r, g, b));
                }
                None => {
                    ui.painter().line_segment([rect.left_bottom(), rect.right_top()], Stroke::new(1.25, t.text_dim));
                }
            }
            ui.painter().rect_stroke(rect, CornerRadius::same(2), Stroke::new(1.0, t.input_border), StrokeKind::Inside);
            seen.push((key.to_owned(), rect, true));
            ui.add_space(8.0);
        }
    });
}

/// The colour field: the hue across (twelve quads of a mesh, so that the blend is smooth), saturation 1 at the top and 0 at the bottom, at lightness `l`.
fn paint_field(painter: &egui::Painter, rect: Rect, l: f64, t: &Tokens) {
    const STOPS: usize = 12;
    let mut mesh = egui::Mesh::default();
    for i in 0..=STOPS {
        let h = 360.0 * i as f64 / STOPS as f64;
        let x = rect.left() + rect.width() * i as f32 / STOPS as f32;
        for (y, s) in [(rect.top(), 1.0), (rect.bottom(), 0.0)] {
            let Rgb(r, g, b) = hsl_to_rgb(h, s, l);
            mesh.colored_vertex(pos2(x, y), theme::document_colour(r, g, b));
        }
    }
    for i in 0..STOPS as u32 {
        let k = i * 2;
        mesh.add_triangle(k, k + 1, k + 2);
        mesh.add_triangle(k + 1, k + 3, k + 2);
    }
    painter.add(egui::Shape::mesh(mesh));
    painter.rect_stroke(rect, CornerRadius::same(2), Stroke::new(1.0, t.input_border), StrokeKind::Inside);
}

/// The lightness bar: white at the top, the colour in the middle and black at the bottom.
fn paint_bar(painter: &egui::Painter, rect: Rect, h: f64, s: f64, t: &Tokens) {
    let mut mesh = egui::Mesh::default();
    for (y, l) in [(rect.top(), 1.0), (rect.center().y, 0.5), (rect.bottom(), 0.0)] {
        let Rgb(r, g, b) = hsl_to_rgb(h, s, l);
        let colour = theme::document_colour(r, g, b);
        mesh.colored_vertex(pos2(rect.left(), y), colour);
        mesh.colored_vertex(pos2(rect.right(), y), colour);
    }
    for k in [0_u32, 2] {
        mesh.add_triangle(k, k + 1, k + 2);
        mesh.add_triangle(k + 1, k + 3, k + 2);
    }
    painter.add(egui::Shape::mesh(mesh));
    painter.rect_stroke(rect, CornerRadius::same(2), Stroke::new(1.0, t.input_border), StrokeKind::Inside);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_colour_goes_to_hsl_and_back_within_a_step() {
        for rgb in [Rgb(0, 0, 0), Rgb(255, 255, 255), Rgb(255, 0, 0), Rgb(0x3C, 0x78, 0xD8), Rgb(12, 200, 99), Rgb(128, 128, 128), Rgb(1, 2, 3)] {
            let (h, s, l) = rgb_to_hsl(rgb);
            let back = hsl_to_rgb(h, s, l);
            for (a, b) in [(rgb.0, back.0), (rgb.1, back.1), (rgb.2, back.2)] {
                assert!(a.abs_diff(b) <= 1, "{rgb:?} came back {back:?}");
            }
        }
        assert_eq!(hsl_to_rgb(0.0, 1.0, 0.5), Rgb(255, 0, 0));
        assert_eq!(hsl_to_rgb(120.0, 1.0, 0.5), Rgb(0, 255, 0));
        assert_eq!(hsl_to_rgb(240.0, 1.0, 0.5), Rgb(0, 0, 255));
        assert_eq!(hsl_to_rgb(f64::NAN, 2.0, -1.0), Rgb(0, 0, 0), "a number that is none is a colour, and the numbers are held to their ranges");
        assert_eq!(hsl_to_rgb(720.0, 1.0, 0.5), Rgb(255, 0, 0));
    }

    #[test]
    fn the_standard_grid_is_a_row_of_greys_and_six_rows_of_twelve_hues() {
        let cells = standard_cells();
        assert_eq!(cells.len(), STANDARD_COLUMNS * 7);
        assert_eq!(cells[0], Rgb(255, 255, 255));
        assert_eq!(cells[STANDARD_COLUMNS - 1], Rgb(0, 0, 0));
        let mut sorted = cells.clone();
        sorted.sort_by_key(|c| (c.0, c.1, c.2));
        sorted.dedup();
        assert_eq!(sorted.len(), cells.len(), "every cell is a colour of its own");
    }

    #[test]
    fn a_target_names_its_command_and_key() {
        assert_eq!(command_for("fill"), Some(("format.shape", "fill")));
        assert_eq!(command_for("color"), Some(("format.text", "color")));
        assert_eq!(command_for("shadow_color"), Some(("format.shadow", "shadow_color")));
        assert_eq!(command_for("nope"), None);
    }
}
