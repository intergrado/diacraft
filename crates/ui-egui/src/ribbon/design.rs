//! The Design tab's own controls: the Themes and Variants galleries, and what the Layout group's Connectors menu shows and sends.
//!
//! **Themes and variants.** The galleries are filled from `theme.list` (the engine's catalogue and what the page on view has), read once for each
//! revision of the drawing, each page and each selection ([`DiacraftApp::refresh_design_view`]). A tile is drawn here from the theme's own colours (the
//! ground, the ink, six accents), so no picture of another application is copied. A press on a tile sends `theme.apply` (a right click offers the
//! current page and all pages) and a press on a variant sends `theme.variant`. Both name the page on view.
//!
//! **Live preview** (`theme_preview.rs`). A tile or a row under the pointer asks for its theme, variant or colours to be shown on the page, which is drawn from a copy of the
//! drawing with the theme applied while the pointer is there (a preview is no edit: no undo step, no unsaved mark, the redo stack kept).
//!
//! **Variants and Colors.** The engine keeps four variants of each theme (`theme.variant`): the strip's four tiles pick one for the colours and the
//! styles together, and the Colors menu picks one for the colours alone. That is our reading of the Colors menu: the engine cannot give a page the
//! colours of another theme, so the reference application's list of colour sets is the theme's four colour variants here. Effects, Connectors and
//! Embellishments have no engine command and stay quiet.
//!
//! **Connectors.** The rows are static controls that name `connector.style` and `connector.line_jumps`. A press adds what only the window knows: the page
//! on view, and the connectors among the shapes selected (`design_params`). With none selected the style is the page's default for new connectors
//! Show Line Jumps is the page's check box, and shows the page's `LineJumpCode`.

use std::sync::Arc;

use diacraft_engine::draw::{StyleChoice, line_jump_settings, page_style, placements};
use diacraft_engine::formatting::Rgb;
use diacraft_engine::theme::PreviewRequest;
use egui::{Align2, CornerRadius, Painter, Rect, Response, RichText, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use serde_json::{Value, json};

use super::controls::{Cx, chosen};
use super::spec::{Body, Ctl, Mark};
use crate::app::DiacraftApp;
use crate::format::MAX_LOOKED_AT;
use crate::theme::{self, Tokens, size};
use crate::widgets::{self, label_colour, paint_button_bg};

/// The most shapes of a selection a press asks the engine about when it looks for connectors.
const MAX_SELECTED: usize = 65_536;

/// The width of a tile of the strip, its gap, and the room the strip's arrow takes, in points (as `controls::draw_gallery` lays them out).
const STRIP_TILE_W: f32 = 42.0;
const STRIP_STEP: f32 = 46.0;
/// The size of a tile in the gallery's menu, and the room its caption takes under it.
const MENU_TILE: egui::Vec2 = egui::vec2(72.0, 52.0);
const CAPTION_H: f32 = 16.0;
/// The tiles of the Themes menu in a row.
const THEME_COLUMNS: usize = 3;
/// The width the Themes menu has for its tiles in a row, and the Variants menu for its four, in points.
const THEMES_WIDTH: f32 = THEME_COLUMNS as f32 * MENU_TILE.x + (THEME_COLUMNS as f32 - 1.0) * 6.0 + 16.0;
const VARIANTS_WIDTH: f32 = 4.0 * MENU_TILE.x + 3.0 * 6.0 + 16.0;

/// How many tiles a strip shows: the Themes strip three, the Variants strip the four variants, and either one in a narrow window.
pub const fn strip_tiles(body: Body, compact: bool) -> usize {
    match (body, compact) {
        (_, true) => 1,
        (Body::Variants, false) => 4,
        _ => 3,
    }
}

/// The colours a tile is drawn in.
#[derive(Clone, Copy, Debug)]
struct Look {
    ground: Rgb,
    ink: Rgb,
    accents: [Rgb; 6],
}

/// A theme of `theme.list`, as a tile shows it.
#[derive(Debug)]
struct ThemeTile {
    id: String,
    name: String,
    look: Look,
    /// The six colours of each of the theme's variants.
    variants: Vec<[Rgb; 6]>,
}

/// What the Design tab shows of the page on view.
#[derive(Debug, Default)]
pub struct DesignState {
    /// The drawing's generation and revision, the page and the shapes selected on it that this was read for.
    key: Option<(u64, u64, u32, Vec<u32>)>,
    themes: Arc<Vec<ThemeTile>>,
    /// The names of the four variants, as `theme.list` gives them.
    variant_names: Arc<Vec<String>>,
    /// The theme the page has: its id, `none`, or `custom` (a theme of a file that the catalogue does not hold).
    page_theme: String,
    /// The places (0 to 3) of the colour variant and the style variant the page has.
    colours: u32,
    styles: u32,
    /// How many of the shapes selected on the page are connectors (of the first [`MAX_LOOKED_AT`]).
    connectors: usize,
    /// The page's default connector style, as `connector.style` spells it.
    default_style: Option<&'static str>,
    /// Whether the page shows line jumps (`LineJumpCode` above 0).
    pub(crate) jumps: bool,
    /// The theme of the Themes menu that was right clicked, whose rows (the current page, all pages) show under the tiles until the menu closes. A
    /// context menu cannot open from inside a menu in egui (it takes the menu's place), so the menu holds the rows itself.
    pub(crate) picked: Option<String>,
}

fn colour(v: &Value) -> Option<Rgb> {
    v.as_str().and_then(Rgb::parse)
}

/// Six colours of a JSON list of hex colours (a variant lists a seventh, which a tile does not draw).
fn six(list: &Value) -> Option<[Rgb; 6]> {
    let mut out = [Rgb(0, 0, 0); 6];
    let items = list.as_array()?;
    if items.len() < 6 {
        return None;
    }
    for (slot, v) in out.iter_mut().zip(items) {
        *slot = colour(v)?;
    }
    Some(out)
}

fn parse_theme(t: &Value) -> Option<ThemeTile> {
    let c = t.get("colours")?;
    let ground = c.get("background").and_then(colour).or_else(|| c.get("light1").and_then(colour))?;
    let look = Look { ground, ink: c.get("dark1").and_then(colour)?, accents: six(c.get("accents")?)? };
    let variants = t.get("variants")?.as_array()?.iter().filter_map(|v| six(v.get("colours")?)).collect();
    Some(ThemeTile { id: t.get("id")?.as_str()?.to_owned(), name: t.get("name")?.as_str()?.to_owned(), look, variants })
}

/// The name `connector.style` gives a choice.
fn style_word(choice: StyleChoice) -> &'static str {
    match choice {
        StyleChoice::RightAngle => "right_angle",
        StyleChoice::Straight => "straight",
        StyleChoice::Curved => "curved",
    }
}

impl DiacraftApp {
    /// Reads what the Design tab shows of the page on view, if the drawing, the page or the selection has changed since it was read. Called before the
    /// ribbon is drawn, and only while the Design tab is open.
    pub(crate) fn refresh_design_view(&mut self) {
        if self.ui.tab != "Design" {
            return;
        }
        let Some(page) = self.current_page() else {
            self.ui.design = DesignState::default();
            return;
        };
        let selection = self.selection_here().map(|(_, ids)| ids).unwrap_or_default();
        let key = (self.ui.generation, self.session.revision(), page, selection);
        if self.ui.design.key.as_ref() == Some(&key) {
            return;
        }
        let mut state = DesignState::default();
        if let Some(doc) = self.session.document() {
            let looked_at = &key.3[..key.3.len().min(MAX_LOOKED_AT)];
            state.connectors =
                if looked_at.is_empty() { 0 } else { placements(doc, page, looked_at).map_or(0, |found| found.iter().filter(|s| s.one_d).count()) };
            state.default_style = Some(style_word(page_style(doc, page)));
            state.jumps = line_jump_settings(doc, page).code != 0;
        }
        if let Ok(list) = self.session.execute("theme.list", &json!({"page": page})) {
            state.themes =
                Arc::new(list.get("themes").and_then(Value::as_array).map_or_else(Vec::new, |all| all.iter().filter_map(parse_theme).collect()));
            state.variant_names = Arc::new(
                list.get("variants")
                    .and_then(Value::as_array)
                    .map_or_else(Vec::new, |all| all.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect()),
            );
            let held = list.get("page").cloned().unwrap_or(Value::Null);
            state.page_theme = held.get("theme").and_then(Value::as_str).unwrap_or("none").to_owned();
            let place = |name: &str| held.get(name).and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok()).unwrap_or(0);
            (state.colours, state.styles) = (place("colors"), place("styles"));
        }
        state.key = Some(key);
        self.ui.design = state;
    }

    /// The page on view, found without changing anything (`current_page` remembers the first tab it falls back on).
    pub(crate) fn page_on_view(&self) -> Option<u32> {
        let doc = self.session.document()?;
        self.ui.page.filter(|p| doc.page(*p).is_some()).or_else(|| crate::page_tabs::tab_order(doc).first().map(|(id, _)| *id))
    }

    /// Why Variants cannot be used now: the page on view has no theme, and a variant is a variant of one (`theme.variant` would refuse).
    pub(crate) fn variant_refusal(&self) -> Option<String> {
        let (doc, page) = (self.session.document()?, self.page_on_view()?);
        let themed = diacraft_engine::theme::selection_of(doc, page).is_some_and(|s| s.is_themed());
        (!themed).then(|| "the page has no theme: apply one first".to_owned())
    }

    /// Whether the row `mark` of the Design tab's menus stands for what the page on view has.
    pub(crate) fn design_mark_is_current(&self, mark: Mark) -> bool {
        let d = &self.ui.design;
        match mark {
            // The page's default is what a press sets when no connector is selected; with some selected the menu is about them, and the engine has no
            // way to tell their style (the style of a connector is read by `connector_path` only for a path of lines), so nothing is marked.
            Mark::ConnectorStyle(word) => d.connectors == 0 && d.default_style == Some(word),
            Mark::NoTheme => d.page_theme == "none",
            Mark::None
            | Mark::Orientation(_)
            | Mark::Case(_)
            | Mark::Bullet(_)
            | Mark::ContainerResize(_)
            | Mark::ContainerHeading(_)
            | Mark::ContainerMargin(_)
            | Mark::SwimlaneOrientation(_)
            | Mark::OrgLayout(_)
            | Mark::OrgArrangement(_) => false,
        }
    }

    /// The connectors among the shapes selected on the page on view, for a command that styles them.
    pub(crate) fn selected_connectors(&mut self) -> Vec<u32> {
        let Some((page, ids)) = self.selection_here() else { return Vec::new() };
        let ids = &ids[..ids.len().min(MAX_SELECTED)];
        let Some(doc) = self.session.document() else { return Vec::new() };
        placements(doc, page, ids).map_or_else(|_| Vec::new(), |found| found.iter().filter(|s| s.one_d).map(|s| s.id).collect())
    }

    /// What a press on a control that names `connector.style`, `connector.line_jumps` or `theme.apply` sends: its own parameters, the page on view, the
    /// connectors selected (a style goes to them, and to the page's default when none is selected), and for Show Line Jumps the state the page is not in.
    pub(crate) fn design_params(&mut self, ctl: &Ctl) -> Result<Value, String> {
        let internal = |what: &str| format!("internal error: `{}` {what}; please report this bug", ctl.id);
        let mut params: Value = if ctl.args.is_empty() {
            json!({})
        } else {
            serde_json::from_str(ctl.args).map_err(|e| internal(&format!("has parameters that are not JSON ({e})")))?
        };
        let page = self.current_page().ok_or_else(|| "no document open".to_owned())?;
        let connectors = if matches!(ctl.command, Some("connector.style" | "connector.reroute" | "connector.reset")) {
            self.selected_connectors()
        } else {
            Vec::new()
        };
        let jumps_on = self.session.document().is_some_and(|doc| line_jump_settings(doc, page).code != 0);
        let Some(map) = params.as_object_mut() else { return Err(internal("has parameters that are not an object")) };
        map.insert("page".to_owned(), json!(page));
        if !connectors.is_empty() {
            map.insert("ids".to_owned(), json!(connectors));
        }
        if ctl.command == Some("connector.line_jumps") {
            map.insert("show".to_owned(), json!(!jumps_on));
        }
        Ok(params)
    }

    /// Gives the page on view (or every page) the theme `id`.
    fn apply_theme(&mut self, ctx: &egui::Context, id: &str, all: bool) {
        let params = match (all, self.current_page()) {
            (true, _) => json!({"theme": id, "scope": "all"}),
            (false, Some(page)) => json!({"theme": id, "page": page}),
            (false, None) => return,
        };
        self.run_ribbon_params(ctx, "theme.apply", &params);
    }

    /// Gives the page on view a variant of its theme: both the colours and the styles, or the colours alone.
    fn apply_variant(&mut self, ctx: &egui::Context, place: usize, colours_only: bool) {
        let Some(page) = self.current_page() else { return };
        let key = if colours_only { "colors" } else { "variant" };
        self.run_ribbon_params(ctx, "theme.variant", &json!({ key: place, "page": page }));
    }
}

// ---- tiles -----------------------------------------------------------------------------------------------------------------------------------

/// What the page shows while the tile of theme `id` is under the pointer: the theme with its first variant, and **nothing** when the page has that theme and variant already (a press
/// would change nothing, and the preview would be the page).
fn theme_preview_of(app: &DiacraftApp, id: &str) -> Option<PreviewRequest> {
    let d = &app.ui.design;
    (!(d.page_theme == id && d.colours == 0 && d.styles == 0)).then(|| PreviewRequest::Theme { id: id.to_owned(), variant: 0 })
}

/// A tile to draw: the theme's ground, ink and accents in a small "Aa" over two overlapping shapes and a row of the six accents.
struct Tile<'a> {
    /// The element's name, for tests and agents.
    element: String,
    /// The label assistive technology and the tooltip give.
    label: &'a str,
    look: Look,
    /// Whether it stands for what the page has now.
    current: bool,
    enabled: bool,
    /// Why it is quiet, for its tooltip.
    why_quiet: &'static str,
    /// What the page shows while the tile is under the pointer (`theme_preview.rs`).
    preview: Option<PreviewRequest>,
}

/// Draws `tile` in `rect`, senses a press on it, and says whether it was pressed. The colours are the theme's own (document colours); the outline,
/// hover and quiet colours are tokens.
fn draw_tile(app: &mut DiacraftApp, ui: &mut Ui, cx: Cx, rect: Rect, tile: &Tile<'_>) -> Response {
    let id = ui.scope_id().with(("design.tile", &tile.element));
    let resp = ui.interact(rect, id, if tile.enabled { Sense::click() } else { Sense::hover() });
    widgets::name_toggle(&resp, tile.enabled, tile.current, tile.label);
    paint_tile(ui.painter(), rect, tile, &cx.t, &resp);
    if tile.enabled
        && resp.hovered()
        && let Some(request) = &tile.preview
    {
        app.ask_theme_preview(request.clone());
    }
    app.note(&tile.element, rect, tile.enabled);
    resp.clone().on_hover_text(if tile.enabled { tile.label.to_owned() } else { format!("{} ({})", tile.label, tile.why_quiet) });
    resp
}

fn paint_tile(painter: &Painter, rect: Rect, tile: &Tile<'_>, t: &Tokens, resp: &Response) {
    let on = tile.enabled;
    let (ground, ink) = if on {
        (
            theme::document_colour(tile.look.ground.0, tile.look.ground.1, tile.look.ground.2),
            theme::document_colour(tile.look.ink.0, tile.look.ink.1, tile.look.ink.2),
        )
    } else {
        (t.icon_fill_disabled, t.icon_ink_disabled)
    };
    let accent = |n: usize| match (on, tile.look.accents.get(n)) {
        (true, Some(Rgb(r, g, b))) => theme::document_colour(*r, *g, *b),
        _ => t.icon_ink_disabled,
    };
    painter.rect(rect, CornerRadius::same(3), ground, Stroke::new(1.0, t.input_border), StrokeKind::Inside);
    let at = |fx: f32, fy: f32| pos2(rect.left() + rect.width() * fx, rect.top() + rect.height() * fy);
    painter.text(at(0.5, 0.07), Align2::CENTER_TOP, "Aa", theme::font((rect.height() * 0.24).clamp(9.0, 15.0)), ink);
    // Two overlapping shapes, the second in front with an edge in the ground's colour to set it off.
    let (back, front) = (Rect::from_min_max(at(0.12, 0.40), at(0.60, 0.68)), Rect::from_min_max(at(0.38, 0.50), at(0.88, 0.78)));
    painter.rect_filled(back, CornerRadius::same(2), accent(0));
    painter.rect_filled(front, CornerRadius::same(2), accent(1));
    painter.rect_stroke(front, CornerRadius::same(2), Stroke::new(1.0, ground), StrokeKind::Outside);
    // The six accents in a row.
    let radius = (rect.width() / 22.0).clamp(1.6, 3.2);
    for n in 0..6 {
        painter.circle_filled(at(0.14 + 0.72 * (n as f32 + 0.5) / 6.0, 0.9), radius, accent(n));
    }
    if tile.current {
        painter.rect_stroke(rect, CornerRadius::same(3), Stroke::new(2.0, t.accent), StrokeKind::Inside);
    } else if on && resp.hovered() {
        painter.rect_stroke(rect, CornerRadius::same(3), Stroke::new(1.5, t.accent_hover), StrokeKind::Inside);
    }
}

/// The rows a right click on a theme offers: the current page and all pages.
fn apply_menu(app: &mut DiacraftApp, ui: &mut Ui, cx: Cx, id: &str) {
    ui.set_min_width(150.0);
    let enabled = app.command_enabled("theme.apply").is_ok();
    for (element, label, all) in [("menu.theme.apply_page", "Apply to Current Page", false), ("menu.theme.apply_all", "Apply to All Pages", true)] {
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), if enabled { Sense::click() } else { Sense::hover() });
        widgets::name_button(&resp, enabled, label);
        paint_button_bg(ui, rect, &resp, enabled, false, &cx.t, 3);
        ui.painter().text(
            pos2(rect.left() + 10.0, rect.center().y),
            Align2::LEFT_CENTER,
            label,
            theme::font(size::LABEL),
            label_colour(&cx.t, enabled),
        );
        app.note(element, rect, enabled);
        if resp.clicked() && enabled {
            app.apply_theme(ui.ctx(), id, all);
            chosen(app, ui);
        }
    }
}

// ---- the strip -------------------------------------------------------------------------------------------------------------------------------

/// The tiles in the strip of the Themes or the Variants gallery, inside `area`. A press on a theme applies it to the page, and a press on a variant
/// picks it; the arrow at the strip's end (drawn by the caller) opens the whole gallery.
pub(super) fn strip(app: &mut DiacraftApp, ui: &mut Ui, cx: Cx, ctl: &Ctl, area: Rect, compact: bool) {
    let n = strip_tiles(ctl.body, compact);
    let size = vec2(STRIP_TILE_W, area.height() - 8.0);
    let rect = |i: usize| Rect::from_min_size(pos2(area.left() + 4.0 + i as f32 * STRIP_STEP, area.top() + 4.0), size);
    match ctl.body {
        Body::Themes => {
            let (themes, enabled) = (app.ui.design.themes.clone(), app.command_enabled("theme.apply").is_ok());
            for (i, t) in themes.iter().take(n).enumerate() {
                let current = app.ui.design.page_theme == t.id;
                let spec = Tile {
                    element: format!("ribbon.{}.theme.{}", cx.tab, t.id),
                    label: &t.name,
                    look: t.look,
                    current,
                    enabled,
                    why_quiet: "no document open",
                    preview: theme_preview_of(app, &t.id),
                };
                let resp = draw_tile(app, ui, cx, rect(i), &spec);
                theme_press(app, ui, cx, &resp, &t.id, false);
            }
        }
        Body::Variants => {
            let (themes, names) = (app.ui.design.themes.clone(), app.ui.design.variant_names.clone());
            for i in 0..n {
                let Some(spec) = variant_tile(app, &themes, &names, i, format!("ribbon.{}.variant.{i}", cx.tab)) else { continue };
                let resp = draw_tile(app, ui, cx, rect(i), &spec);
                if resp.clicked() && spec.enabled {
                    app.apply_variant(ui.ctx(), i, false);
                }
            }
        }
        Body::None
        | Body::PageSizes
        | Body::VariantColours
        | Body::Callouts
        | Body::Layouts
        | Body::Masters
        | Body::ContainerStyles
        | Body::SheetSections
        | Body::QuickStyles => {}
    }
}

/// What a press and a right click on a theme's tile do. A right click on a tile of the strip opens a context menu; in the Themes menu it picks the tile,
/// and its rows show under the tiles (`DesignState::picked`).
fn theme_press(app: &mut DiacraftApp, ui: &mut Ui, cx: Cx, resp: &Response, id: &str, in_menu: bool) {
    if resp.clicked() {
        app.apply_theme(ui.ctx(), id, false);
        if in_menu {
            chosen(app, ui);
        }
    }
    if in_menu {
        if resp.secondary_clicked() {
            app.ui.design.picked = Some(id.to_owned());
        }
    } else {
        resp.context_menu(|ui| apply_menu(app, ui, cx, id));
    }
}

/// The tile of variant `place` of the theme the page has (the page's own colours in the Variants strip), or `None` when the list does not hold it. Quiet
/// when the page has no theme.
fn variant_tile<'a>(app: &DiacraftApp, themes: &'a [ThemeTile], names: &'a [String], place: usize, element: String) -> Option<Tile<'a>> {
    let held = themes.iter().find(|t| t.id == app.ui.design.page_theme);
    let enabled = held.is_some() && app.command_enabled("theme.variant").is_ok();
    // With no theme there is nothing to vary: the first theme's look stands in, drawn quiet.
    let base = held.or_else(|| themes.first())?;
    let colours = *base.variants.get(place)?;
    let current = enabled && app.ui.design.colours as usize == place;
    Some(Tile {
        element,
        label: names.get(place).map_or("Variant", String::as_str),
        look: Look { accents: colours, ..base.look },
        current,
        enabled,
        why_quiet: "apply a theme first",
        // A press on the tile that is the page's variant already changes nothing, and nothing is previewed.
        preview: u8::try_from(place)
            .ok()
            .filter(|_| !(app.ui.design.colours as usize == place && app.ui.design.styles as usize == place))
            .map(|n| PreviewRequest::Variants { colours: Some(n), styles: Some(n) }),
    })
}

// ---- the menus -------------------------------------------------------------------------------------------------------------------------------

/// A tile with its caption under it, in the gallery's menu: the rectangle of the tile and what was pressed.
fn captioned(app: &mut DiacraftApp, ui: &mut Ui, cx: Cx, spec: &Tile<'_>) -> Response {
    let (cell, _) = ui.allocate_exact_size(vec2(MENU_TILE.x, MENU_TILE.y + CAPTION_H), Sense::hover());
    let resp = draw_tile(app, ui, cx, Rect::from_min_size(cell.min, MENU_TILE), spec);
    let colour = label_colour(&cx.t, spec.enabled);
    let caption = Rect::from_min_size(pos2(cell.left(), cell.top() + MENU_TILE.y), vec2(MENU_TILE.x, CAPTION_H));
    ui.painter().with_clip_rect(caption).text(
        pos2(caption.center().x, caption.center().y + 1.0),
        Align2::CENTER_CENTER,
        spec.label,
        theme::font(size::GROUP),
        colour,
    );
    resp
}

/// The inside of the Themes or the Variants menu above its fixed rows: the tiles of the whole gallery.
pub(super) fn gallery_menu(app: &mut DiacraftApp, ui: &mut Ui, cx: Cx, ctl: &Ctl) {
    match ctl.body {
        Body::Themes => {
            let (themes, enabled) = (app.ui.design.themes.clone(), app.command_enabled("theme.apply").is_ok());
            ui.label(RichText::new("Built-In").font(theme::font(size::GROUP)).color(cx.t.text_dim));
            for row in themes.chunks(THEME_COLUMNS) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    for t in row {
                        let current = app.ui.design.page_theme == t.id;
                        let spec = Tile {
                            element: format!("menu.theme.{}", t.id),
                            label: &t.name,
                            look: t.look,
                            current,
                            enabled,
                            why_quiet: "no document open",
                            preview: theme_preview_of(app, &t.id),
                        };
                        let resp = captioned(app, ui, cx, &spec);
                        theme_press(app, ui, cx, &resp, &t.id, true);
                    }
                });
            }
            if let Some(picked) = app.ui.design.picked.clone().and_then(|id| themes.iter().find(|t| t.id == id)) {
                ui.add_space(4.0);
                ui.label(RichText::new(format!("{}:", picked.name)).font(theme::font(size::GROUP)).color(cx.t.text_dim));
                apply_menu(app, ui, cx, &picked.id);
            }
        }
        Body::Variants => {
            let (themes, names) = (app.ui.design.themes.clone(), app.ui.design.variant_names.clone());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                for i in 0..4 {
                    let Some(spec) = variant_tile(app, &themes, &names, i, format!("menu.variant.{i}")) else { continue };
                    let resp = captioned(app, ui, cx, &spec);
                    if resp.clicked() && spec.enabled {
                        app.apply_variant(ui.ctx(), i, false);
                        chosen(app, ui);
                    }
                }
            });
        }
        Body::None
        | Body::PageSizes
        | Body::VariantColours
        | Body::Callouts
        | Body::Layouts
        | Body::Masters
        | Body::ContainerStyles
        | Body::SheetSections
        | Body::QuickStyles => {}
    }
}

/// The inside of the Colors menu above its fixed rows: the four colour variants of the page's theme, each as a row of its six colours and its name.
/// A press sets the colours' variant and leaves the styles as they are (`theme.variant` with `colors`).
pub(super) fn colour_rows(app: &mut DiacraftApp, ui: &mut Ui, cx: Cx) {
    let (themes, names) = (app.ui.design.themes.clone(), app.ui.design.variant_names.clone());
    let held = themes.iter().find(|t| t.id == app.ui.design.page_theme);
    let enabled = held.is_some() && app.command_enabled("theme.variant").is_ok();
    for place in 0..4 {
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), if enabled { Sense::click() } else { Sense::hover() });
        let label = names.get(place).map_or("Variant", String::as_str);
        let current = enabled && app.ui.design.colours as usize == place;
        widgets::name_toggle(&resp, enabled, current, label);
        paint_button_bg(ui, rect, &resp, enabled, current, &cx.t, 3);
        let painter = ui.painter();
        let (swatch, mut x) = (13.0, rect.left() + 8.0);
        for n in 0..6 {
            let fill = match (enabled, held.and_then(|t| t.variants.get(place)).and_then(|v| v.get(n))) {
                (true, Some(Rgb(r, g, b))) => theme::document_colour(*r, *g, *b),
                _ => cx.t.icon_ink_disabled,
            };
            let r = Rect::from_center_size(pos2(x + swatch / 2.0, rect.center().y), vec2(swatch, swatch));
            painter.rect_filled(r, CornerRadius::same(2), fill);
            x += swatch + 2.0;
        }
        painter.text(pos2(x + 8.0, rect.center().y), Align2::LEFT_CENTER, label, theme::font(size::LABEL), label_colour(&cx.t, enabled));
        app.note(format!("menu.variant.colors.{place}"), rect, enabled);
        if enabled
            && !current
            && resp.hovered()
            && let Ok(n) = u8::try_from(place)
        {
            app.ask_theme_preview(PreviewRequest::Variants { colours: Some(n), styles: None });
        }
        if !enabled {
            resp.clone().on_hover_text("the page has no theme: apply one first");
        }
        if resp.clicked() && enabled {
            app.apply_variant(ui.ctx(), place, true);
            chosen(app, ui);
        }
    }
}

/// The width the menu of a control with this body is at least (the Size menu's is the pickers').
pub(super) fn body_width(body: Body) -> f32 {
    match body {
        Body::Themes => THEMES_WIDTH,
        Body::Variants => VARIANTS_WIDTH,
        Body::Callouts => super::callouts::WIDTH,
        Body::Layouts => crate::relayout_ui::WIDTH,
        Body::Masters => super::change_shape::GALLERY_WIDTH,
        Body::QuickStyles => super::quick_styles::WIDTH,
        Body::None | Body::PageSizes | Body::VariantColours | Body::SheetSections => 0.0,
        Body::ContainerStyles => crate::container_ui::menu_width(),
    }
}
