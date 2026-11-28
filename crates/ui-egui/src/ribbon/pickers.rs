//! What opens from the Home tab's formatting controls: the lists of fonts and sizes, and the samples a line menu draws (weights, dashes and line ends); and
//! what the colour controls show of the selection. **The palette of colours is `palette.rs`**. **The colour bar** under a colour
//! control's icon shows the selection's colour (the control's own state): a slash through it when the shapes differ or the colour is none. [`palette_colours`] is the
//! grid of the Text dialog's colour buttons, made in code (a row of greys and five rows of ten hues), so no vendor's swatches are copied.
//!
//! **Nothing here is a document colour that came from a theme token**: the colours of a bar are the person's data, painted as they are. Every other colour is a token.

use diacraft_engine::formatting::Rgb;
use egui::{Align2, CornerRadius, Painter, Rect, Sense, Stroke, StrokeKind, TextEdit, Ui, pos2, vec2};
use serde_json::json;

use super::controls::{Cx, chosen};
use super::spec::{Ctl, Sample};
use crate::app::DiacraftApp;
use crate::format::{FONT_SIZES, Mix, thousandths};
use crate::theme::{self, Tokens, size};
use crate::widgets::{self, label_colour, paint_button_bg};

const COLUMNS: usize = 10;
const ROW: f32 = 22.0;
const LIST_HEIGHT: f32 = 300.0;
/// The width of the Font list and of the Size list, in points.
const LIST_WIDTH: f32 = 240.0;
const SIZE_LIST_WIDTH: f32 = 84.0;
/// The width of a menu row's sample of a line, in points.
pub const SAMPLE_W: f32 = 84.0;

/// The lightness of the rows of hues, light to dark, and their saturation.
const LIGHTNESS: [f32; 5] = [0.88, 0.72, 0.55, 0.40, 0.26];
const SATURATION: f32 = 0.65;

fn hsl(h: f32, s: f32, l: f32) -> Rgb {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let byte = |v: f32| ((v + m).clamp(0.0, 1.0) * 255.0).round() as u8;
    Rgb(byte(r), byte(g), byte(b))
}

/// The colours of the palette, row by row: ten greys, then five rows of ten hues.
pub fn palette_colours() -> Vec<Rgb> {
    let mut out: Vec<Rgb> = (0..COLUMNS)
        .map(|i| {
            let v = (255.0 * (1.0 - i as f32 / (COLUMNS - 1) as f32)).round() as u8;
            Rgb(v, v, v)
        })
        .collect();
    for l in LIGHTNESS {
        out.extend((0..COLUMNS).map(|c| hsl(36.0 * c as f32, SATURATION, l)));
    }
    out
}

/// The colour a colour control shows for the selection.
pub(crate) fn colour_of(app: &DiacraftApp, key: &str) -> Mix<Option<Rgb>> {
    match key {
        "fill" => app.fill_mix(),
        "line" => app.line_mix(),
        // The colour between the marks of a pattern, the stop of a gradient the pane edits, the colour of a shadow.
        "fill_background" => app.background_mix(),
        "stop_color" => stop_colour_of(app, diacraft_engine::formatting::GradientTarget::Fill),
        "line_stop_color" => stop_colour_of(app, diacraft_engine::formatting::GradientTarget::Line),
        "shadow_color" => match app.shadow_mix() {
            Mix::One(Some(shadow)) => Mix::One(Some(shadow.colour)),
            Mix::One(None) | Mix::Mixed => Mix::Mixed,
            Mix::Nothing => Mix::Nothing,
        },
        _ => match app.text_colour_mix() {
            Mix::One(rgb) => Mix::One(Some(rgb)),
            Mix::Mixed => Mix::Mixed,
            Mix::Nothing => Mix::Nothing,
        },
    }
}

/// The colour of the stop of the gradient the Format Shape pane has selected, for the shapes selected.
fn stop_colour_of(app: &DiacraftApp, target: diacraft_engine::formatting::GradientTarget) -> Mix<Option<Rgb>> {
    match app.gradient_mix(target) {
        Mix::One(Some(g)) => g.stops.get(app.selected_stop(g.stops.len())).map_or(Mix::Nothing, |s| Mix::One(Some(s.colour))),
        Mix::Mixed => Mix::Mixed,
        _ => Mix::Nothing,
    }
}

/// The colour of the selection for the colour control `ctl`, if it has a palette.
pub fn bar_colour(app: &DiacraftApp, ctl: &Ctl) -> Option<Mix<Option<Rgb>>> {
    ctl.palette.map(|palette| colour_of(app, palette.key))
}

/// Paints the colour bar under a colour control's icon.
pub fn paint_bar(painter: &Painter, rect: Rect, colour: &Mix<Option<Rgb>>, t: &Tokens, enabled: bool) {
    let edge = Stroke::new(1.0, if enabled { t.input_border } else { t.text_disabled });
    match colour {
        Mix::One(Some(Rgb(r, g, b))) => {
            painter.rect_filled(rect, CornerRadius::same(1), theme::document_colour(*r, *g, *b));
            painter.rect_stroke(rect, CornerRadius::same(1), edge, StrokeKind::Outside);
        }
        Mix::One(None) | Mix::Mixed => {
            painter.rect_stroke(rect, CornerRadius::same(1), edge, StrokeKind::Inside);
            let slash = Stroke::new(1.25, if enabled { t.text_dim } else { t.text_disabled });
            painter.line_segment([rect.left_bottom() + vec2(1.0, 0.0), rect.right_top() + vec2(-1.0, 0.0)], slash);
            if matches!(colour, Mix::Mixed) {
                painter.line_segment([rect.left_bottom() + vec2(5.0, 0.0), rect.right_top() + vec2(5.0, 0.0)], slash);
            }
        }
        Mix::Nothing => {
            painter.rect_stroke(rect, CornerRadius::same(1), Stroke::new(1.0, t.text_disabled), StrokeKind::Inside);
        }
    }
}

/// One row of a list: a name, marked when it is the current one. → whether it was pressed.
fn list_row(app: &mut DiacraftApp, ui: &mut Ui, cx: Cx, label: &str, name: String, current: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width().max(120.0), ROW), Sense::click());
    widgets::name_button(&resp, true, label);
    paint_button_bg(ui, rect, &resp, true, current, &cx.t, 3);
    ui.painter().with_clip_rect(rect.shrink2(vec2(6.0, 0.0))).text(
        pos2(rect.left() + 8.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        theme::font(size::LABEL),
        cx.t.text,
    );
    app.note(name, rect, true);
    resp.clicked()
}

/// Whether the menu was not drawn in the frame before: it has just opened, so its box takes the keyboard and starts empty.
fn just_opened(app: &mut DiacraftApp, ctx: &egui::Context, which: usize) -> bool {
    let now = ctx.cumulative_pass_nr();
    let last = std::mem::replace(&mut app.ui.format.menu_seen[which], now);
    now.saturating_sub(last) > 2
}

/// The inside of the Font box's list: a box to find a font by typing, and the machine's families (found on a thread of their own).
pub fn font_menu(app: &mut DiacraftApp, ui: &mut Ui, cx: Cx, ctl: &Ctl) {
    let Some(command) = ctl.command else { return };
    let ctx = ui.ctx().clone();
    app.ui.format.fonts.start(&ctx);
    let opened = just_opened(app, &ctx, 0);
    if opened {
        app.ui.format.filter.clear();
    }
    ui.set_min_width(LIST_WIDTH);
    let mut filter = std::mem::take(&mut app.ui.format.filter);
    let edit = ui.add(TextEdit::singleline(&mut filter).hint_text("Find a font").desired_width(LIST_WIDTH - 8.0).font(theme::font(size::LABEL)));
    edit.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Find a font"));
    app.note("font.filter", edit.rect, true);
    if opened {
        edit.request_focus();
    }
    let typed = filter.trim().to_owned();
    let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    let current = match app.font_mix() {
        Mix::One(Some(name)) => Some(name),
        _ => None,
    };
    let families = app.ui.format.fonts.families();
    let mut picked: Option<String> = None;
    match &families {
        None => {
            ui.add_space(4.0);
            ui.label(egui::RichText::new("Loading fonts").font(theme::font(size::LABEL)).color(cx.t.text_dim));
            app.note("font.loading", ui.min_rect(), false);
        }
        Some(all) => {
            let needle = typed.to_lowercase();
            let mut shown: Vec<&String> = all.iter().filter(|name| needle.is_empty() || name.to_lowercase().contains(&needle)).collect();
            // The face the shapes have is on the list even if the machine lacks it, so that the box can show what it is.
            if let Some(face) = &current
                && !all.contains(face)
                && (needle.is_empty() || face.to_lowercase().contains(&needle))
            {
                shown.insert(0, face);
            }
            ui.add_space(2.0);
            if shown.is_empty() {
                ui.label(egui::RichText::new("No font has that in its name").font(theme::font(size::LABEL)).color(cx.t.text_dim));
            }
            // The list is as tall as its rows, up to LIST_HEIGHT: a menu's first pass is a measuring one, and a scroll area left to shrink to
            // what it saw then stays two rows tall.
            let height = (shown.len() as f32 * (ROW + ui.spacing().item_spacing.y)).clamp(ROW, LIST_HEIGHT);
            egui::ScrollArea::vertical().min_scrolled_height(height).max_height(height).auto_shrink([false, false]).show_rows(
                ui,
                ROW,
                shown.len(),
                |ui, range| {
                    for name in shown.get(range).unwrap_or_default() {
                        let is_current = current.as_deref() == Some(name.as_str());
                        if list_row(app, ui, cx, name, format!("font.item.{name}"), is_current) {
                            picked = Some((*name).clone());
                        }
                    }
                },
            );
            if enter && picked.is_none() {
                // Enter takes the first font found, or the name as it was typed (a face the machine lacks can still be named).
                picked = shown.first().map(|name| (*name).clone()).or_else(|| (!typed.is_empty()).then(|| typed.clone()));
            }
        }
    }
    if enter && picked.is_none() && !typed.is_empty() {
        picked = Some(typed);
    }
    app.ui.format.filter = filter;
    if let Some(name) = picked {
        app.run_ribbon_params(&ctx, command, &json!({"font": name}));
        chosen(app, ui);
    }
}

/// The inside of the Size box's list: a box to type a size in, and the sizes the steps go through.
pub fn size_menu(app: &mut DiacraftApp, ui: &mut Ui, cx: Cx, ctl: &Ctl) {
    let Some(command) = ctl.command else { return };
    let ctx = ui.ctx().clone();
    let opened = just_opened(app, &ctx, 1);
    if opened {
        app.ui.format.filter.clear();
    }
    ui.set_min_width(SIZE_LIST_WIDTH);
    let mut typed = std::mem::take(&mut app.ui.format.filter);
    let edit = ui.add(TextEdit::singleline(&mut typed).hint_text("Size").desired_width(SIZE_LIST_WIDTH - 8.0).font(theme::font(size::LABEL)));
    edit.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Type a size"));
    app.note("size.input", edit.rect, true);
    if opened {
        edit.request_focus();
    }
    let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    let current = match app.size_mix() {
        Mix::One(thousandths_of_a_point) => Some(thousandths_of_a_point),
        _ => None,
    };
    let mut picked: Option<f64> = None;
    ui.add_space(2.0);
    for points in FONT_SIZES {
        let is_current = current == Some((points * 1000.0).round() as i64);
        if list_row(app, ui, cx, &size_text(*points), format!("size.item.{}", size_text(*points)), is_current) {
            picked = Some(*points);
        }
    }
    if enter {
        match typed.trim().parse::<f64>() {
            Ok(points) if points.is_finite() && points > 0.0 && points <= diacraft_engine::formatting::MAX_SIZE_POINTS => picked = Some(points),
            _ => app.set_notice("A size is a number of points from more than 0 to 1000."),
        }
    }
    app.ui.format.filter = typed;
    if let Some(points) = picked {
        app.run_ribbon_params(&ctx, command, &json!({"size": points}));
        chosen(app, ui);
    }
}

/// A size as the Size box writes it: whole points without a fraction.
pub fn size_text(points: f64) -> String {
    let rounded = (points * 100.0).round() / 100.0;
    if (rounded - rounded.round()).abs() < 1e-9 { format!("{}", rounded.round() as i64) } else { format!("{rounded}") }
}

/// What the Size box shows for the selection.
pub fn size_label(app: &DiacraftApp) -> Option<String> {
    match app.size_mix() {
        Mix::One(thousandths_of_a_point) => Some(size_text(thousandths_of_a_point as f64 / 1000.0)),
        _ => None,
    }
}

/// What the Font box shows for the selection.
pub fn font_label(app: &DiacraftApp) -> Option<String> {
    match app.font_mix() {
        Mix::One(Some(name)) => Some(name),
        _ => None,
    }
}

/// Whether the menu row `item` stands for what the whole selection has (a weight, a dash pattern or a line end), so that it is marked.
pub fn is_current(app: &DiacraftApp, item: &Ctl) -> bool {
    match item.sample {
        Sample::Icon => app.mark_is_current(item.mark),
        Sample::Weight(points) => app.weight_mix() == Mix::One(thousandths(f64::from(points))),
        Sample::Dash(n) => app.pattern_mix() == Mix::One(u32::from(n)),
        Sample::Arrow { number, begin: true } => app.begin_arrow_mix() == Mix::One(u32::from(number)),
        Sample::Arrow { number, begin: false } => app.end_arrow_mix() == Mix::One(u32::from(number)),
    }
}

/// The marks and gaps of line patterns 2 to 23, in multiples of the weight.
const DASHES: [&[f32]; 22] = [
    &[7.0, 5.0],
    &[0.0, 5.0],
    &[7.0, 5.0, 0.0, 5.0],
    &[7.0, 5.0, 0.0, 5.0, 0.0, 5.0],
    &[7.0, 5.0, 7.0, 5.0, 0.0, 5.0],
    &[19.0, 5.0, 7.0, 5.0],
    &[19.0, 5.0, 7.0, 5.0, 7.0, 5.0],
    &[3.0, 3.0],
    &[0.0, 3.0],
    &[3.0, 3.0, 0.0, 3.0],
    &[3.0, 3.0, 0.0, 3.0, 0.0, 3.0],
    &[3.0, 3.0, 3.0, 3.0, 0.0, 3.0],
    &[9.0, 3.0, 3.0, 3.0],
    &[9.0, 3.0, 3.0, 3.0, 3.0, 3.0],
    &[15.0, 9.0],
    &[0.0, 9.0],
    &[15.0, 9.0, 0.0, 9.0],
    &[15.0, 9.0, 0.0, 9.0, 0.0, 9.0],
    &[15.0, 9.0, 15.0, 9.0, 0.0, 9.0],
    &[39.0, 9.0, 15.0, 9.0],
    &[39.0, 9.0, 15.0, 9.0, 15.0, 9.0],
    &[1.0, 2.0],
];

/// Paints a menu row's sample in `rect`, in the icon colours.
pub fn paint_sample(painter: &Painter, rect: Rect, sample: Sample, t: &Tokens, enabled: bool) {
    let ink = if enabled { t.icon_ink } else { t.icon_ink_disabled };
    let y = rect.center().y;
    let painter = painter.with_clip_rect(rect.expand2(vec2(0.0, 4.0)));
    match sample {
        Sample::Icon => {}
        Sample::Weight(points) => {
            // Points on paper at 96 to the inch, held to what can be told apart.
            let thick = (points * 96.0 / 72.0).clamp(0.6, 7.0);
            painter.line_segment([pos2(rect.left(), y), pos2(rect.right(), y)], Stroke::new(thick, ink));
        }
        Sample::Dash(1) => {
            painter.line_segment([pos2(rect.left(), y), pos2(rect.right(), y)], Stroke::new(1.6, ink));
        }
        Sample::Dash(n) => {
            let weight = 1.1;
            let list = DASHES.get(usize::from(n).saturating_sub(2)).copied().unwrap_or(&[7.0, 5.0]);
            let mut x = rect.left();
            for (k, length) in list.iter().cycle().enumerate() {
                if x >= rect.right() || k > 200 {
                    break;
                }
                let run = length * weight;
                if k % 2 == 0 {
                    if run <= 0.0 {
                        painter.circle_filled(pos2(x, y), weight * 1.2, ink);
                    } else {
                        painter.line_segment([pos2(x, y), pos2((x + run).min(rect.right()), y)], Stroke::new(1.8, ink));
                    }
                }
                x += run;
            }
        }
        Sample::Arrow { number, begin } => {
            // The mark is drawn as the page's renderer draws it (`line_ends.rs`), its tip on the end of the shaft; the shaft stops where the mark's back is, so that a
            // hollow mark shows nothing through it. A few points are left past the tip for a disc, which is centred on it.
            let unit = 4.0;
            let (tip, back) = if begin { (pos2(rect.left() + 5.0, y), 1.0) } else { (pos2(rect.right() - 5.0, y), -1.0) };
            let art = super::line_ends::art(number);
            let stop = art.as_ref().map_or(0.0, |a| a.stop()) * unit;
            let (shaft_from, shaft_to) = if begin { (tip.x + stop, rect.right()) } else { (rect.left(), tip.x - stop) };
            painter.line_segment([pos2(shaft_from, y), pos2(shaft_to, y)], Stroke::new(1.5, ink));
            if let Some(art) = art {
                super::line_ends::paint(&painter, tip, back, unit, &art, ink, t.stencil_fill);
            }
        }
    }
}

/// The width of the Size menu's list of named sizes, in points.
pub const SIZES_WIDTH: f32 = 230.0;

/// The most a list of named sizes is tall before it scrolls, in points.
const SIZES_HEIGHT: f32 = 300.0;

/// The inside of the Size menu above its fixed rows: the named sizes of `page.presets`, in their groups, with the one the page has marked. A press
/// sends `page.size` with the size's id. **The sizes and their groups are the engine's** (`draw::paper::SIZES`), and the dimensions shown are
/// the portrait ones on paper (inches, or millimetres for the ISO sizes).
pub fn size_gallery(app: &mut DiacraftApp, ui: &mut Ui, cx: Cx) {
    let enabled = app.command_enabled("page.size").is_ok();
    let current = app.page_view().and_then(|s| s.size).map(|s| s.id);
    let height = (ui.ctx().content_rect().height() - 260.0).clamp(120.0, SIZES_HEIGHT);
    let mut picked: Option<&'static str> = None;
    egui::ScrollArea::vertical().id_salt("size_gallery").max_height(height).auto_shrink([false, true]).show(ui, |ui| {
        let mut group = "";
        for preset in diacraft_engine::draw::paper::SIZES {
            if preset.group != group {
                group = preset.group;
                ui.label(egui::RichText::new(group).font(theme::font(size::GROUP)).color(cx.t.text_dim));
            }
            let (rect, resp) = ui
                .allocate_exact_size(vec2(ui.available_width().max(SIZES_WIDTH - 16.0), 24.0), if enabled { Sense::click() } else { Sense::hover() });
            let marked = current == Some(preset.id);
            widgets::name_toggle(&resp, enabled, marked, preset.name);
            paint_button_bg(ui, rect, &resp, enabled, marked, &cx.t, 3);
            let painter = ui.painter();
            let colour = label_colour(&cx.t, enabled);
            painter.text(pos2(rect.left() + 10.0, rect.center().y), Align2::LEFT_CENTER, preset.name, theme::font(size::LABEL), colour);
            let dimension = if preset.group == "ISO" {
                format!("{} x {} mm", (preset.width * 25.4).round(), (preset.height * 25.4).round())
            } else {
                format!("{} x {} in", crate::page_setup::number_text(preset.width), crate::page_setup::number_text(preset.height))
            };
            painter.text(pos2(rect.right() - 8.0, rect.center().y), Align2::RIGHT_CENTER, dimension, theme::font(size::GROUP), cx.t.text_dim);
            app.note(format!("size.item.{}", preset.id), rect, enabled);
            if resp.clicked() && enabled {
                picked = Some(preset.id);
            }
        }
    });
    if let Some(id) = picked {
        app.run_ribbon_params(ui.ctx(), "page.size", &json!({"preset": id}));
        chosen(app, ui);
    }
}
