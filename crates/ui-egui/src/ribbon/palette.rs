//! The colour palette: what opens from the Fill, Line and Font Color buttons.
//!
//! **The sections, top to bottom**: **Use Default**; **Theme Colors**, the page's theme colours with a top row and five shade rows under it, a tooltip such as "Turquoise,
//! Accent 6, Darker 25%"; **Variant Colors**, seven swatches; **Standard Colors**, ten; **Recent Colors**, ten; then **No Fill** (or **No Line**) and **More Colors...**, which opens
//! the Colors dialog (`colors_dialog.rs`). A menu of the control's own items (Gradient, Pattern, Transparency, Fill Options...) follows under a rule (`controls.rs`).
//!
//! **A theme swatch writes a formula that follows the theme** (`theme/palette.rs`): the press sends the control's command with `theme:accent2:40`. A Standard Colors,
//! Recent Colors or Colors dialog colour is a fixed colour, which no theme changes. **Use Default** sends `default`, which hands the cells to the theme.
//! A palette of a control whose command takes no theme colour (a gradient stop, the colour between the marks of a pattern, a shadow) shows only Standard Colors, Recent Colors and More Colors.
//!
//! **The colours are read from the engine**: `theme.colors` gives the swatches of the page on view, read once for each drawing, revision and page. The Standard Colors are the
//! engine's list too, and Recent Colors are the window's (`UiState::recent_colours`, ten at most, newest first, kept while the window is open: the application stores no settings yet).
//! **The colours of the swatches are the drawing's data**, painted as they are; every other colour is a token.

use std::sync::Arc;

use diacraft_engine::formatting::Rgb;
use egui::{Align2, CornerRadius, Painter, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use serde_json::{Value, json};

use super::controls::{Cx, chosen};
use super::pickers::{colour_of, paint_bar};
use super::spec::{Caption, Ctl, Palette};
use crate::app::DiacraftApp;
use crate::format::Mix;
use crate::theme::{self, size};
use crate::widgets::{self, label_colour, paint_button_bg};

/// The most Recent Colors kept.
pub const MAX_RECENT: usize = 10;

const COLUMNS: usize = 10;
const SWATCH: f32 = 17.0;
const GAP: f32 = 3.0;
const PAD: f32 = 4.0;
/// The width of the palette, in points: ten swatches and the gaps between them.
pub const WIDTH: f32 = COLUMNS as f32 * (SWATCH + GAP) + GAP + 2.0 * PAD;
/// The height of a row of the menu that is not a swatch (Use Default, No Fill, More Colors...).
const ROW: f32 = 24.0;

/// A swatch of the page's theme palette, as the window reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct SwatchView {
    /// What a press sends: `theme:accent2:40`.
    pub pick: String,
    pub tooltip: String,
    /// The colour the formula comes to on the page; `None` where the theme cannot answer.
    pub colour: Option<Rgb>,
}

/// What the palette of a page shows: the Theme Colors columns (the colour, then its five shades), the seven Variant Colors and the ten Standard Colors.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PaletteView {
    pub columns: Vec<Vec<SwatchView>>,
    pub variants: Vec<SwatchView>,
    pub standard: Vec<(String, Rgb)>,
}

fn swatch_of(v: &Value) -> Option<SwatchView> {
    Some(SwatchView {
        pick: v.get("pick")?.as_str()?.to_owned(),
        tooltip: v.get("tooltip")?.as_str()?.to_owned(),
        colour: v.get("hex").and_then(Value::as_str).and_then(Rgb::parse),
    })
}

/// The palette `theme.colors` answered, or `None` where it is not the shape the window expects.
pub fn parse(answer: &Value) -> Option<PaletteView> {
    let columns = answer
        .get("theme_colors")?
        .as_array()?
        .iter()
        .map(|c| c.get("swatches")?.as_array()?.iter().map(swatch_of).collect::<Option<Vec<_>>>())
        .collect::<Option<Vec<_>>>()?;
    let variants = answer.get("variant_colors")?.as_array()?.iter().map(swatch_of).collect::<Option<Vec<_>>>()?;
    let standard = answer
        .get("standard_colors")?
        .as_array()?
        .iter()
        .map(|c| Some((c.get("label")?.as_str()?.to_owned(), Rgb::parse(c.get("hex")?.as_str()?)?)))
        .collect::<Option<Vec<_>>>()?;
    Some(PaletteView { columns, variants, standard })
}

/// What the palette is read for: the drawing's generation and revision, and the page.
type Key = (u64, u64, u32);

impl DiacraftApp {
    /// The palette of the page on view, read from the engine once for each drawing, revision and page.
    pub(crate) fn palette_view(&mut self) -> Option<Arc<PaletteView>> {
        let page = self.page_on_view()?;
        let key: Key = (self.ui.generation, self.session.revision(), page);
        if let Some((held, view)) = &self.ui.palette
            && *held == key
        {
            return Some(Arc::clone(view));
        }
        let answer = self.session.execute("theme.colors", &json!({ "page": page })).ok()?;
        let view = Arc::new(parse(&answer)?);
        self.ui.palette = Some((key, Arc::clone(&view)));
        Some(view)
    }

    /// Puts `colour` first among the Recent Colors (once, whatever was there), and drops the oldest past [`MAX_RECENT`].
    pub(crate) fn remember_colour(&mut self, colour: Rgb) {
        self.ui.recent_colours.retain(|c| *c != colour);
        self.ui.recent_colours.insert(0, colour);
        self.ui.recent_colours.truncate(MAX_RECENT);
    }

    /// The Recent Colors, newest first.
    pub fn recent_colours(&self) -> &[Rgb] {
        &self.ui.recent_colours
    }
}

/// What a press on the palette chose.
enum Pick {
    /// The word sent as the control's parameter: `#rrggbb`, `theme:accent2:40`, `default` or `none`.
    Word(String),
    /// A fixed colour (it enters the Recent Colors).
    Recent(Rgb),
    More,
}

/// A heading over a section.
fn heading(ui: &mut Ui, cx: Cx, text: &str) {
    ui.add_space(3.0);
    ui.label(egui::RichText::new(text).font(theme::font(size::GROUP)).color(cx.t.text_dim));
}

/// One swatch in `rect`. → whether it was pressed. A swatch the theme cannot answer is drawn empty and quiet.
#[allow(clippy::too_many_arguments)]
fn swatch(
    app: &mut DiacraftApp,
    ui: &mut Ui,
    cx: Cx,
    rect: Rect,
    name: String,
    colour: Option<Rgb>,
    tooltip: &str,
    marked: bool,
    enabled: bool,
) -> bool {
    let usable = enabled && colour.is_some();
    let resp = ui.interact(rect, ui.scope_id().with(("swatch", &name)), if usable { Sense::click() } else { Sense::hover() });
    widgets::name_button(&resp, usable, tooltip);
    let painter = ui.painter();
    match colour {
        Some(Rgb(r, g, b)) => {
            painter.rect_filled(rect, CornerRadius::same(2), theme::document_colour(r, g, b));
            painter.rect_stroke(rect, CornerRadius::same(2), Stroke::new(1.0, cx.t.input_border), StrokeKind::Inside);
        }
        None => {
            painter.rect_stroke(rect, CornerRadius::same(2), Stroke::new(1.0, cx.t.text_disabled), StrokeKind::Inside);
        }
    }
    if marked {
        painter.rect_stroke(rect.expand(2.0), CornerRadius::same(3), Stroke::new(2.0, cx.t.accent), StrokeKind::Inside);
    } else if resp.hovered() && usable {
        painter.rect_stroke(rect.expand(1.0), CornerRadius::same(2), Stroke::new(1.5, cx.t.text), StrokeKind::Inside);
    }
    app.note(name, rect, usable);
    let pressed = resp.clicked() && usable;
    resp.on_hover_text(tooltip.to_owned());
    pressed
}

/// A row of the menu with a mark at its left (Use Default, No Fill) or none (More Colors...). → whether it was pressed.
#[allow(clippy::too_many_arguments)]
fn row(
    app: &mut DiacraftApp,
    ui: &mut Ui,
    cx: Cx,
    element: String,
    label: &str,
    enabled: bool,
    marked: bool,
    mark: impl FnOnce(&Painter, Rect),
) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(WIDTH, ROW), if enabled { Sense::click() } else { Sense::hover() });
    widgets::name_button(&resp, enabled, label);
    paint_button_bg(ui, rect, &resp, enabled, marked, &cx.t, 3);
    mark(ui.painter(), Rect::from_center_size(pos2(rect.left() + 14.0, rect.center().y), vec2(14.0, 14.0)));
    ui.painter().text(pos2(rect.left() + 30.0, rect.center().y), Align2::LEFT_CENTER, label, theme::font(size::LABEL), label_colour(&cx.t, enabled));
    app.note(element, rect, enabled);
    resp.clicked() && enabled
}

/// The inside of the menu of a colour control: Use Default, the theme's colours with their shades, the variant colours, the standard colours, the recent ones, No Fill (or No Line)
/// and More Colors.... A press sends the control's command with the colour and closes the menu.
pub fn colour_menu(app: &mut DiacraftApp, ui: &mut Ui, cx: Cx, ctl: &Ctl, palette: Palette) {
    let Some(command) = ctl.command else { return };
    let enabled = app.command_enabled(command).is_ok();
    let current = colour_of(app, palette.key);
    let current_rgb = match &current {
        Mix::One(rgb) => Some(*rgb),
        _ => None,
    };
    let key = palette.key;
    let view = if palette.themed { app.palette_view() } else { None };
    let mut picked: Option<Pick> = None;
    ui.set_min_width(WIDTH);
    if palette.themed {
        // Use Default: the colour the theme gives, shown as a swatch of it.
        let default_colour = view.as_ref().and_then(|v| default_swatch(v, key));
        let use_default =
            row(app, ui, cx, format!("palette.{key}.default"), palette.default.text, enabled, false, |painter, r| match default_colour {
                Some(Rgb(red, green, blue)) => {
                    painter.rect_filled(r, CornerRadius::same(2), theme::document_colour(red, green, blue));
                    painter.rect_stroke(r, CornerRadius::same(2), Stroke::new(1.0, cx.t.input_border), StrokeKind::Inside);
                }
                None => {
                    painter.rect_stroke(r, CornerRadius::same(2), Stroke::new(1.0, cx.t.input_border), StrokeKind::Inside);
                }
            });
        if use_default {
            picked = Some(Pick::Word("default".to_owned()));
        }
        if let Some(view) = &view {
            theme_colours(app, ui, cx, key, view, enabled, current_rgb, &mut picked);
            variant_colours(app, ui, cx, key, view, enabled, current_rgb, &mut picked);
        }
    }
    standard_colours(app, ui, cx, key, view.as_deref(), enabled, current_rgb, &mut picked);
    recent_colours(app, ui, cx, key, enabled, current_rgb, &mut picked);
    ui.add_space(3.0);
    if let Some(Caption { text: label, .. }) = palette.none
        && row(app, ui, cx, format!("palette.{key}.none"), label, enabled, current_rgb == Some(None), |painter, r| {
            paint_bar(painter, r.shrink2(vec2(0.0, 2.0)), &Mix::One(None), &cx.t, enabled)
        })
    {
        picked = Some(Pick::Word("none".to_owned()));
    }
    if row(app, ui, cx, format!("palette.{key}.more"), palette.more.text, enabled, false, |_, _| {}) {
        picked = Some(Pick::More);
    }
    match picked {
        None => {}
        Some(Pick::More) => {
            if let Err(e) = app.execute("window.colorsDialog", &json!({ "target": key })) {
                app.set_notice(&e.to_string());
            }
            chosen(app, ui);
        }
        Some(Pick::Word(word)) => {
            app.run_ribbon_params(ui.ctx(), command, &json!({ key: word }));
            chosen(app, ui);
        }
        Some(Pick::Recent(rgb)) => {
            app.remember_colour(rgb);
            app.run_ribbon_params(ui.ctx(), command, &json!({ key: rgb.hex() }));
            chosen(app, ui);
        }
    }
}

/// The colour a control's Use Default row shows: the theme's first accent for a fill, its dark colour for a line and for text. It is the top swatch of the column that picks that colour,
/// found by what the swatch picks and not by where the column stands (the ten columns are Light, Dark, Light, Dark and the six accents).
fn default_swatch(view: &PaletteView, key: &str) -> Option<Rgb> {
    let pick = if key == "fill" { "theme:accent1" } else { "theme:dark" };
    view.columns.iter().filter_map(|column| column.first()).find(|top| top.pick == pick)?.colour
}

/// The name of the element of a swatch of the Theme Colors section: `palette.fill.theme:accent2:40`. The third and fourth columns repeat the first two, so a swatch whose pick an
/// earlier column has already is named with its column's place after an `@`, `palette.fill.theme:light:-25@3`: no two elements of a menu share a name or an egui id.
fn theme_swatch_name(key: &str, columns: &[Vec<SwatchView>], column: usize, row: usize) -> String {
    let pick = columns.get(column).and_then(|c| c.get(row)).map(|sw| sw.pick.as_str()).unwrap_or_default();
    let repeat = columns.iter().take(column).any(|earlier| earlier.get(row).is_some_and(|sw| sw.pick == pick));
    if repeat { format!("palette.{key}.{pick}@{}", column.saturating_add(1)) } else { format!("palette.{key}.{pick}") }
}

/// The Theme Colors section: the columns left to right, the colour on top and its shades under it.
#[allow(clippy::too_many_arguments)]
fn theme_colours(
    app: &mut DiacraftApp,
    ui: &mut Ui,
    cx: Cx,
    key: &str,
    view: &PaletteView,
    enabled: bool,
    current: Option<Option<Rgb>>,
    picked: &mut Option<Pick>,
) {
    heading(ui, cx, "Theme Colors");
    let columns = view.columns.len().max(1);
    let rows = view.columns.iter().map(Vec::len).max().unwrap_or(0);
    let pitch = (WIDTH - 2.0 * PAD) / columns as f32;
    let cell_w = (pitch - GAP).max(6.0);
    // The colour itself, a little room, and the shades.
    let height = rows as f32 * (SWATCH + GAP) + GAP + 3.0;
    let (grid, _) = ui.allocate_exact_size(vec2(WIDTH, height), Sense::hover());
    for (c, column) in view.columns.iter().enumerate() {
        for (r, sw) in column.iter().enumerate() {
            let y = grid.top() + GAP + r as f32 * (SWATCH + GAP) + if r > 0 { 3.0 } else { 0.0 };
            let rect = Rect::from_min_size(pos2(grid.left() + PAD + c as f32 * pitch + GAP / 2.0, y), vec2(cell_w, SWATCH));
            let marked = current == Some(sw.colour) && sw.colour.is_some();
            if swatch(app, ui, cx, rect, theme_swatch_name(key, &view.columns, c, r), sw.colour, &sw.tooltip, marked, enabled) {
                *picked = Some(Pick::Word(sw.pick.clone()));
            }
        }
    }
}

/// The Variant Colors section: the seven colours of the page's colour variant.
#[allow(clippy::too_many_arguments)]
fn variant_colours(
    app: &mut DiacraftApp,
    ui: &mut Ui,
    cx: Cx,
    key: &str,
    view: &PaletteView,
    enabled: bool,
    current: Option<Option<Rgb>>,
    picked: &mut Option<Pick>,
) {
    heading(ui, cx, "Variant Colors");
    let (grid, _) = ui.allocate_exact_size(vec2(WIDTH, SWATCH + 2.0 * GAP), Sense::hover());
    let count = view.variants.len().max(1);
    let pitch = (WIDTH - 2.0 * PAD) / count as f32;
    for (n, sw) in view.variants.iter().enumerate() {
        let rect =
            Rect::from_min_size(pos2(grid.left() + PAD + n as f32 * pitch + GAP / 2.0, grid.top() + GAP), vec2((pitch - GAP).max(6.0), SWATCH));
        let marked = current == Some(sw.colour) && sw.colour.is_some();
        if swatch(app, ui, cx, rect, format!("palette.{key}.{}", sw.pick), sw.colour, &sw.tooltip, marked, enabled) {
            *picked = Some(Pick::Word(sw.pick.clone()));
        }
    }
}

/// A row of up to [`COLUMNS`] fixed colours at the palette's own pitch.
#[allow(clippy::too_many_arguments)]
fn fixed_row(
    app: &mut DiacraftApp,
    ui: &mut Ui,
    cx: Cx,
    key: &str,
    group: &str,
    colours: &[(String, Rgb)],
    enabled: bool,
    current: Option<Option<Rgb>>,
    picked: &mut Option<Pick>,
) -> Option<Rgb> {
    let (grid, _) = ui.allocate_exact_size(vec2(WIDTH, SWATCH + 2.0 * GAP), Sense::hover());
    let mut chosen_colour = None;
    for n in 0..COLUMNS {
        let rect = Rect::from_min_size(pos2(grid.left() + PAD + GAP + n as f32 * (SWATCH + GAP), grid.top() + GAP), vec2(SWATCH, SWATCH));
        match colours.get(n) {
            Some((name, rgb)) => {
                let marked = current == Some(Some(*rgb));
                if swatch(
                    app,
                    ui,
                    cx,
                    rect,
                    format!("palette.{key}.{group}.{}", rgb.hex()),
                    Some(*rgb),
                    &format!("{name} ({})", rgb.hex()),
                    marked,
                    enabled,
                ) {
                    *picked = Some(Pick::Word(rgb.hex()));
                    chosen_colour = Some(*rgb);
                }
            }
            // An empty place of the Recent Colors: a faint outline, nothing to press.
            None => {
                ui.painter().rect_filled(rect, CornerRadius::same(2), cx.t.hover);
            }
        }
    }
    chosen_colour
}

/// The Standard Colors section. A press sends the colour and is no Recent Color (it is always on the menu).
#[allow(clippy::too_many_arguments)]
fn standard_colours(
    app: &mut DiacraftApp,
    ui: &mut Ui,
    cx: Cx,
    key: &str,
    view: Option<&PaletteView>,
    enabled: bool,
    current: Option<Option<Rgb>>,
    picked: &mut Option<Pick>,
) {
    heading(ui, cx, "Standard Colors");
    // The engine's list when the page was read, else the same list as the engine gives it (a window with no drawing has none to ask).
    let colours: Vec<(String, Rgb)> = match view {
        Some(v) => v.standard.clone(),
        None => diacraft_engine::theme::STANDARD_COLOURS.iter().map(|(n, c)| ((*n).to_owned(), *c)).collect(),
    };
    fixed_row(app, ui, cx, key, "standard", &colours, enabled, current, picked);
}

/// The Recent Colors section: the colours chosen from More Colors and the ones chosen from here, newest first.
fn recent_colours(app: &mut DiacraftApp, ui: &mut Ui, cx: Cx, key: &str, enabled: bool, current: Option<Option<Rgb>>, picked: &mut Option<Pick>) {
    heading(ui, cx, "Recent Colors");
    let colours: Vec<(String, Rgb)> = app.ui.recent_colours.iter().map(|c| ("Recent".to_owned(), *c)).collect();
    if let Some(rgb) = fixed_row(app, ui, cx, key, "recent", &colours, enabled, current, picked) {
        *picked = Some(Pick::Recent(rgb));
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn view_of(picks: &[&str]) -> Vec<Vec<SwatchView>> {
        // One top swatch and one shade for each column, the colour of the top swatch a grey of its place so that the swatches differ.
        picks
            .iter()
            .enumerate()
            .map(|(n, pick)| {
                let grey = u8::try_from(n * 20).unwrap();
                vec![
                    SwatchView { pick: (*pick).to_owned(), tooltip: String::new(), colour: Some(Rgb(grey, grey, grey)) },
                    SwatchView { pick: format!("{pick}:-25"), tooltip: String::new(), colour: Some(Rgb(grey, grey, grey)) },
                ]
            })
            .collect()
    }

    const TEN: [&str; 10] = [
        "theme:light",
        "theme:dark",
        "theme:light",
        "theme:dark",
        "theme:accent1",
        "theme:accent2",
        "theme:accent3",
        "theme:accent4",
        "theme:accent5",
        "theme:accent6",
    ];

    #[test]
    fn a_swatch_a_column_repeats_is_named_with_its_place_and_no_two_swatches_of_the_menu_share_a_name() {
        let columns = view_of(&TEN);
        let names: Vec<String> = (0..10).flat_map(|c| (0..2).map(move |r| (c, r))).map(|(c, r)| theme_swatch_name("fill", &columns, c, r)).collect();
        assert_eq!(names[0], "palette.fill.theme:light");
        assert_eq!(names[1], "palette.fill.theme:light:-25");
        assert_eq!(names[4], "palette.fill.theme:light@3", "the third column repeats the first");
        assert_eq!(names[5], "palette.fill.theme:light:-25@3");
        assert_eq!(names[6], "palette.fill.theme:dark@4");
        assert_eq!(names[8], "palette.fill.theme:accent1");
        let mut seen = names.clone();
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), names.len(), "every swatch has a name of its own: {names:?}");
        // A palette with no repeat names every swatch as it always did.
        let accents = view_of(&TEN[4..]);
        assert!((0..accents.len()).all(|c| !theme_swatch_name("line", &accents, c, 0).contains('@')));
    }

    #[test]
    fn use_default_shows_the_first_accent_for_a_fill_and_the_dark_colour_for_a_line_and_for_text_whichever_column_they_are_in() {
        let view = PaletteView { columns: view_of(&TEN), variants: Vec::new(), standard: Vec::new() };
        assert_eq!(default_swatch(&view, "fill"), Some(Rgb(80, 80, 80)), "Accent 1 is the fifth column");
        assert_eq!(default_swatch(&view, "line"), Some(Rgb(20, 20, 20)), "Dark is the second column, the first of the two");
        assert_eq!(default_swatch(&view, "color"), Some(Rgb(20, 20, 20)));
        // A palette with no such column has none to show.
        let none = PaletteView { columns: view_of(&["theme:light"]), variants: Vec::new(), standard: Vec::new() };
        assert_eq!(default_swatch(&none, "fill"), None);
    }
}
