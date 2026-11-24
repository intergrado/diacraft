//! Re-Layout Page, Auto Space and the page breaks in the window: Design > Layout > Re-Layout Page and More Layout Options, Home > Arrange > Position > Auto Space,
//! Spacing Options, Avoid Page Breaks and Move off Page Breaks, and View > Show > Page Breaks.
//!
//! The window is thin: every control sends the engine's command (`page.relayout`, `shape.autospace`, `page.spacing`, `page.avoid_page_breaks`, `shape.move_off_page_breaks`,
//! `page_breaks.set`) for the shapes selected on the page on view, or for the whole page when none is (`window_scope`), and shows what the engine answers.
//!
//! * **The gallery** has the headings Flowchart (four tiles: top to bottom, bottom to top, left to right, right to left), Hierarchy (four), Compact Tree (four of the eight: the
//!   others are in the dialog) , Radial and Circular, then More Layout Options. **Each tile is a picture of the layout it makes**, drawn from the engine's own layout of a small
//!   sample diagram (`diacraft_layout`), so a tile cannot show a placement the command does not make. There is **no live preview** while the pointer is over a tile: a preview on the
//!   page would be an edit and an undo step, as the Themes gallery's would (`ribbon/design.rs`); the tile's tooltip is its name.
//! * **The Configure Layout dialog** has Style, Direction (for a flowchart, a hierarchy and a compact tree), Alignment (for a hierarchy), Spacing in inches, the connectors'
//!   Style and Appearance, the boxes **Apply routing style to connectors** and **Enlarge page to fit drawing**, **Apply settings to** Selection (greyed when nothing is selected) or
//!   Current page, a **Preview** (the picture of the chosen style), and Apply, OK and Cancel. It works on a draft: nothing is sent until Apply or OK.
//! * **Spacing Options** is a small dialog of two boxes, the horizontal and the vertical space between shapes in inches (the page's `AvenueSizeX` and `AvenueSizeY`).
//! * **Page Breaks** is a box of the View tab: dashed lines across the page where it passes the printer paper (`canvas.rs`), from the engine's `page.breaks`.

use diacraft_engine::draw::relayout::{Current, current, page_breaks};
use diacraft_layout::{Graph, Node, Settings, Style, layout};
use egui::{Context, CornerRadius, Id, Key, Modifiers, Painter, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Ui, vec2};
use serde_json::{Value, json};

use crate::app::DiacraftApp;
use crate::format::OnView;
use crate::ribbon::controls::{Cx, chosen};
use crate::theme::{self, Tokens, size};
use crate::widgets;

/// A tile of the gallery: the words the command takes for it, its name, and the style it makes.
#[derive(Clone, Copy, Debug)]
pub struct Tile {
    pub style: &'static str,
    pub direction: Option<&'static str>,
    pub name: &'static str,
}

/// The headings of the gallery and their tiles.
pub const GALLERY: [(&str, &[Tile]); 5] = [
    (
        "Flowchart",
        &[
            Tile { style: "flowchart", direction: Some("top_to_bottom"), name: "Flowchart, top to bottom" },
            Tile { style: "flowchart", direction: Some("bottom_to_top"), name: "Flowchart, bottom to top" },
            Tile { style: "flowchart", direction: Some("left_to_right"), name: "Flowchart, left to right" },
            Tile { style: "flowchart", direction: Some("right_to_left"), name: "Flowchart, right to left" },
        ],
    ),
    (
        "Hierarchy",
        &[
            Tile { style: "hierarchy", direction: Some("top_to_bottom"), name: "Hierarchy, top to bottom" },
            Tile { style: "hierarchy", direction: Some("bottom_to_top"), name: "Hierarchy, bottom to top" },
            Tile { style: "hierarchy", direction: Some("left_to_right"), name: "Hierarchy, left to right" },
            Tile { style: "hierarchy", direction: Some("right_to_left"), name: "Hierarchy, right to left" },
        ],
    ),
    (
        "Compact Tree",
        &[
            Tile { style: "compact_tree", direction: Some("down_right"), name: "Compact tree, down and right" },
            Tile { style: "compact_tree", direction: Some("up_right"), name: "Compact tree, up and right" },
            Tile { style: "compact_tree", direction: Some("right_down"), name: "Compact tree, right and down" },
            Tile { style: "compact_tree", direction: Some("left_down"), name: "Compact tree, left and down" },
        ],
    ),
    ("Radial", &[Tile { style: "radial", direction: None, name: "Radial" }]),
    ("Circular", &[Tile { style: "circular", direction: None, name: "Circular" }]),
];

/// The tiles in a row of the gallery, and a tile's size in points.
const COLUMNS: usize = 4;
const TILE: egui::Vec2 = egui::vec2(56.0, 44.0);

/// The width the menu has for its tiles, in points.
pub(crate) const WIDTH: f32 = COLUMNS as f32 * TILE.x + (COLUMNS as f32 - 1.0) * 5.0 + 16.0;

/// The style of the words a command takes: a style, a direction and an alignment as `diacraft_engine::draw::relayout::choose` reads them.
fn style_of(kind: &str, direction: Option<&str>, alignment: Option<&str>) -> Option<Style> {
    diacraft_engine::draw::relayout::choose(Some(kind), direction, alignment, Style::Radial).ok()
}

// ---- pictures ---------------------------------------------------------------------------------------------------------------------

/// The sample diagram a picture lays out: a root with two children, one of which has two of its own and the other one.
fn sample() -> Graph {
    let at = |i: usize| Node { width: 1.5, height: 1.0, x: 2.0 * i as f64, y: 0.0 };
    Graph { nodes: (0..6).map(at).collect(), edges: vec![(0, 1), (0, 2), (1, 3), (1, 4), (2, 5)] }
}

/// Draws the picture of `style` in `rect`: the sample diagram laid out by the engine's layout, shrunk to fit.
pub(crate) fn paint_style(painter: &Painter, rect: Rect, style: Style, t: &Tokens, enabled: bool) {
    let graph = sample();
    let Ok(at) = layout(&graph, &Settings { style, gap_x: 0.7, gap_y: 0.7 }) else { return };
    let (mut lo, mut hi) = ((f64::INFINITY, f64::INFINITY), (f64::NEG_INFINITY, f64::NEG_INFINITY));
    for (n, p) in graph.nodes.iter().zip(&at) {
        lo = (lo.0.min(p.0 - n.width / 2.0), lo.1.min(p.1 - n.height / 2.0));
        hi = (hi.0.max(p.0 + n.width / 2.0), hi.1.max(p.1 + n.height / 2.0));
    }
    let (w, h) = ((hi.0 - lo.0).max(1.0e-6), (hi.1 - lo.1).max(1.0e-6));
    let inside = rect.shrink(5.0);
    let k = (f64::from(inside.width()) / w).min(f64::from(inside.height()) / h);
    let (ink, fill) = if enabled { (t.icon_ink, t.icon_fill) } else { (t.icon_ink_disabled, t.icon_fill_disabled) };
    // Page y runs up, the screen's down; the picture is centred in its rectangle.
    let (used_w, used_h) = ((w * k) as f32, (h * k) as f32);
    let origin = Pos2::new(inside.center().x - used_w / 2.0, inside.center().y - used_h / 2.0);
    let screen = |x: f64, y: f64| Pos2::new(origin.x + ((x - lo.0) * k) as f32, origin.y + ((hi.1 - y) * k) as f32);
    for &(a, b) in &graph.edges {
        if let (Some(p), Some(q)) = (at.get(a as usize), at.get(b as usize)) {
            painter.line_segment([screen(p.0, p.1), screen(q.0, q.1)], Stroke::new(1.0, ink));
        }
    }
    for (n, p) in graph.nodes.iter().zip(&at) {
        let r = Rect::from_two_pos(screen(p.0 - n.width / 2.0, p.1 - n.height / 2.0), screen(p.0 + n.width / 2.0, p.1 + n.height / 2.0));
        painter.rect(r, CornerRadius::same(1), fill, Stroke::new(1.0, ink), StrokeKind::Inside);
    }
}

// ---- the gallery ------------------------------------------------------------------------------------------------------------------

/// The inside of the Re-Layout Page menu: the headings and tiles of the gallery.
pub(crate) fn gallery_menu(app: &mut DiacraftApp, ui: &mut Ui, cx: Cx) {
    let enabled = app.command_enabled("window.relayout").is_ok();
    for (heading, tiles) in GALLERY {
        ui.label(RichText::new(heading).font(theme::font(size::LABEL)).color(cx.t.text_dim));
        for row in tiles.chunks(COLUMNS) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 5.0;
                for tile in row {
                    let (rect, resp) = ui.allocate_exact_size(TILE, if enabled { Sense::click() } else { Sense::hover() });
                    widgets::name_button(&resp, enabled, tile.name);
                    let t = &cx.t;
                    ui.painter().rect_filled(rect, CornerRadius::same(3), t.input_fill);
                    if let Some(style) = style_of(tile.style, tile.direction, None) {
                        paint_style(ui.painter(), rect, style, t, enabled);
                    }
                    let ring = if resp.hovered() && enabled { Stroke::new(1.5, t.accent_hover) } else { Stroke::new(1.0, t.input_border) };
                    ui.painter().rect_stroke(rect, CornerRadius::same(3), ring, StrokeKind::Inside);
                    resp.clone().on_hover_text(if enabled { tile.name.to_owned() } else { format!("{} (nothing to lay out)", tile.name) });
                    app.note(format!("menu.relayout.{}", tile_key(tile)), rect, enabled);
                    if resp.clicked() && enabled {
                        let mut params = json!({"style": tile.style});
                        if let (Some(map), Some(direction)) = (params.as_object_mut(), tile.direction) {
                            map.insert("direction".to_owned(), json!(direction));
                        }
                        app.run_ribbon_params(ui.ctx(), "window.relayout", &params);
                        chosen(app, ui);
                    }
                }
            });
            ui.add_space(4.0);
        }
    }
    ui.separator();
}

/// The name a tile is known by to tests and agents: its style and its direction.
fn tile_key(tile: &Tile) -> String {
    match tile.direction {
        Some(direction) => format!("{}.{direction}", tile.style),
        None => tile.style.to_owned(),
    }
}

// ---- what the window works on --------------------------------------------------------------------------------------------------

impl DiacraftApp {
    /// The page on view and the shapes selected on it: the shapes a command lays out, or `None` for the whole page. An empty selection, and one that is of another page, is the page.
    fn window_scope(&mut self) -> Option<(u32, Option<Vec<u32>>)> {
        match self.selection_on_view() {
            OnView::NoDocument => None,
            OnView::Here(page, ids) => Some((page, Some(ids))),
            OnView::Nothing | OnView::Elsewhere => self.current_page().map(|page| (page, None)),
        }
    }

    /// Whether some shape is selected on the page on view.
    fn relayout_has_selection(&mut self) -> bool {
        matches!(self.selection_on_view(), OnView::Here(..))
    }

    /// `window.relayout`: `page.relayout` of the selection, or of the page when nothing is selected, with the params the window command was given (a style, a direction...).
    pub(crate) fn window_relayout(&mut self, given: &serde_json::Map<String, Value>) -> diacraft_engine::Result<()> {
        let (page, ids) = self
            .window_scope()
            .ok_or_else(|| diacraft_engine::EngineError::BadParams { cmd: "window.relayout".to_owned(), msg: "no document open".to_owned() })?;
        let mut params = given.clone();
        params.insert("page".to_owned(), json!(page));
        if let Some(ids) = &ids {
            params.insert("ids".to_owned(), json!(ids));
        }
        self.commit_pending_text();
        let before = self.session.revision();
        let answer = self.execute("page.relayout", &Value::Object(params))?;
        if self.session.revision() != before {
            self.keep_preview(page, ids.as_deref().unwrap_or(&[]));
        }
        self.relayout_notice(&answer);
        Ok(())
    }

    /// `window.autoSpace`: `shape.autospace` of the selection or of the page.
    pub(crate) fn window_autospace(&mut self) -> diacraft_engine::Result<()> {
        let (page, ids) = self
            .window_scope()
            .ok_or_else(|| diacraft_engine::EngineError::BadParams { cmd: "window.autoSpace".to_owned(), msg: "no document open".to_owned() })?;
        let mut params = json!({"page": page});
        if let (Some(map), Some(ids)) = (params.as_object_mut(), &ids) {
            map.insert("ids".to_owned(), json!(ids));
        }
        self.commit_pending_text();
        let before = self.session.revision();
        let answer = self.execute("shape.autospace", &params)?;
        if self.session.revision() != before {
            self.keep_preview(page, ids.as_deref().unwrap_or(&[]));
        }
        self.relayout_notice(&answer);
        Ok(())
    }

    /// `arrange.moveOffPageBreaks`: `shape.move_off_page_breaks` of the selection or of the page.
    pub(crate) fn window_move_off_breaks(&mut self) -> diacraft_engine::Result<()> {
        let (page, ids) = self.window_scope().ok_or_else(|| diacraft_engine::EngineError::BadParams {
            cmd: "arrange.moveOffPageBreaks".to_owned(),
            msg: "no document open".to_owned(),
        })?;
        let mut params = json!({"page": page});
        if let (Some(map), Some(ids)) = (params.as_object_mut(), &ids) {
            map.insert("ids".to_owned(), json!(ids));
        }
        self.commit_pending_text();
        let answer = self.execute("shape.move_off_page_breaks", &params)?;
        self.relayout_notice(&answer);
        Ok(())
    }

    /// `arrange.avoidPageBreaks`: the page's box, turned (or set to `on`).
    pub(crate) fn window_avoid_breaks(&mut self, on: Option<bool>) -> diacraft_engine::Result<()> {
        let page = self.current_page().ok_or_else(|| diacraft_engine::EngineError::BadParams {
            cmd: "arrange.avoidPageBreaks".to_owned(),
            msg: "no document open".to_owned(),
        })?;
        let on = on.unwrap_or(!self.avoid_breaks_checked());
        self.execute("page.avoid_page_breaks", &json!({"on": on, "page": page})).map(|_| ())
    }

    /// `view.pageBreaks`: the drawing's Page Breaks box, turned (or set to `on`): `page_breaks.set`, which is no undo step.
    pub(crate) fn window_page_breaks(&mut self, on: Option<bool>) -> diacraft_engine::Result<()> {
        let on = on.unwrap_or(!self.page_breaks_box());
        self.execute("page_breaks.set", &json!({"on": on})).map(|_| ())
    }

    /// Whether the drawing shows its page breaks.
    pub(crate) fn page_breaks_box(&self) -> bool {
        self.session().document().is_some_and(diacraft_engine::view::page_breaks_on)
    }

    /// Whether the page on view has Avoid Page Breaks on.
    pub(crate) fn avoid_breaks_checked(&self) -> bool {
        self.layout_now().is_some_and(|c| c.avoid_page_breaks)
    }

    /// What the cells of the page on view say of its layout.
    fn layout_now(&self) -> Option<Current> {
        let doc = self.session().document()?;
        current(doc, self.ui.page?).ok()
    }

    /// Puts what a layout left out in the status bar's notice, in words: the shapes left on a locked layer.
    fn relayout_notice(&mut self, answer: &Value) {
        let locked = answer["locked"].as_array().map_or(0, Vec::len);
        if locked > 0 {
            self.set_notice(&format!(
                "{locked} shape{} on a locked layer {} left where {} is.",
                if locked == 1 { "" } else { "s" },
                if locked == 1 { "was" } else { "were" },
                if locked == 1 { "it" } else { "they" }
            ));
        }
    }

    /// Whether Re-Layout Page, Auto Space and Move off Page Breaks can run: a drawing with shapes.
    pub(crate) fn relayout_enabled(&self) -> std::result::Result<(), String> {
        self.command_enabled("page.relayout")
    }

    /// The page's breaks as the canvas draws them, in paper inches: `None` with the box off, or for a page that cannot be printed as it is. Read again when the document moves.
    pub(crate) fn page_break_lines(&mut self, page: u32) -> Option<BreakLines> {
        if !self.page_breaks_box() {
            return None;
        }
        let revision = self.session.revision();
        if let Some((rev, p, lines)) = &self.ui.relayout.breaks
            && *rev == revision
            && *p == page
        {
            return lines.clone();
        }
        let lines = self.session().document().and_then(|doc| {
            let k = diacraft_engine::draw::units_per_inch(doc, page);
            let b = page_breaks(doc, page).ok()?;
            Some((b.x.iter().map(|v| v / k).collect(), b.y.iter().map(|v| v / k).collect()))
        });
        self.ui.relayout.breaks = Some((revision, page, lines.clone()));
        lines
    }
}

// ---- the Configure Layout dialog ------------------------------------------------------------------------------------------------

/// Which shapes the dialog's Apply works on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Apply {
    Selection,
    Page,
}

/// The Configure Layout dialog while it is open: a draft of what the page's layout cells say.
pub struct RelayoutDialog {
    pub(crate) style: String,
    pub(crate) direction: String,
    pub(crate) alignment: String,
    pub(crate) spacing: String,
    pub(crate) connector_style: String,
    pub(crate) appearance: String,
    pub(crate) apply_routing: bool,
    pub(crate) enlarge: bool,
    pub(crate) apply_to: Apply,
    error: Option<String>,
}

/// The Spacing Options dialog while it is open.
pub struct SpacingDialog {
    pub(crate) horizontal: String,
    pub(crate) vertical: String,
    error: Option<String>,
}

/// The places of the page breaks along x and along y, in paper inches.
pub type BreakLines = (Vec<f64>, Vec<f64>);

/// What the window keeps for this feature.
#[derive(Default)]
pub struct RelayoutState {
    pub dialog: Option<RelayoutDialog>,
    pub spacing: Option<SpacingDialog>,
    /// The breaks last read: the revision, the page and the lines, in paper inches.
    pub breaks: Option<(u64, u32, Option<BreakLines>)>,
}

/// The kinds and what each takes, in the order of the dialog's Style list.
const KINDS: [(&str, &str); 5] =
    [("flowchart", "Flowchart"), ("hierarchy", "Hierarchy"), ("compact_tree", "Compact Tree"), ("radial", "Radial"), ("circular", "Circular")];
const FLOWS: [(&str, &str); 4] =
    [("top_to_bottom", "Top to bottom"), ("bottom_to_top", "Bottom to top"), ("left_to_right", "Left to right"), ("right_to_left", "Right to left")];
const OUTLINES: [(&str, &str); 8] = [
    ("down_right", "Down, then right"),
    ("right_down", "Right, then down"),
    ("right_up", "Right, then up"),
    ("up_right", "Up, then right"),
    ("up_left", "Up, then left"),
    ("left_up", "Left, then up"),
    ("left_down", "Left, then down"),
    ("down_left", "Down, then left"),
];
const ALIGNS: [(&str, &str); 3] = [("start", "Left or top"), ("center", "Center or middle"), ("end", "Right or bottom")];

impl DiacraftApp {
    /// The style the Configure Layout dialog has now (`None`: it is closed). For the window state, tests and agents.
    pub fn relayout_dialog_style(&self) -> Option<String> {
        self.ui.relayout.dialog.as_ref().map(|d| d.style.clone())
    }

    /// Whether the Spacing Options dialog is open.
    pub fn spacing_dialog_open(&self) -> bool {
        self.ui.relayout.spacing.is_some()
    }

    /// Opens the Configure Layout dialog with what the page's cells say (`page.layout`); one that is open stays as it is.
    pub(crate) fn open_relayout_dialog(&mut self) -> diacraft_engine::Result<()> {
        if self.ui.relayout.dialog.is_some() {
            return Ok(());
        }
        let page = self.current_page().ok_or_else(|| diacraft_engine::EngineError::BadParams {
            cmd: "window.relayoutOptions".to_owned(),
            msg: "no document open".to_owned(),
        })?;
        let now = self.execute("page.layout", &json!({"page": page}))?;
        let text = |key: &str, fallback: &str| now[key].as_str().unwrap_or(fallback).to_owned();
        let kind = text("style", "flowchart");
        let direction = text("direction", if kind == "compact_tree" { "down_right" } else { "top_to_bottom" });
        let spacing = format!("{:.3}", now["spacing"]["x"].as_f64().unwrap_or(0.375));
        let apply_to = if self.relayout_has_selection() { Apply::Selection } else { Apply::Page };
        self.ui.relayout.dialog = Some(RelayoutDialog {
            style: kind,
            direction,
            alignment: text("alignment", "center"),
            spacing: spacing.trim_end_matches('0').trim_end_matches('.').to_owned(),
            connector_style: text("connector_style", "right_angle"),
            appearance: text("appearance", "straight"),
            apply_routing: true,
            enlarge: now["enlarge_page"].as_bool().unwrap_or(false),
            apply_to,
            error: None,
        });
        Ok(())
    }

    pub(crate) fn close_relayout_dialog(&mut self) {
        self.ui.relayout.dialog = None;
    }

    /// Opens the Spacing Options dialog with the page's two spaces (`page.layout`).
    pub(crate) fn open_spacing_dialog(&mut self) -> diacraft_engine::Result<()> {
        if self.ui.relayout.spacing.is_some() {
            return Ok(());
        }
        let page = self
            .current_page()
            .ok_or_else(|| diacraft_engine::EngineError::BadParams { cmd: "window.spacingOptions".to_owned(), msg: "no document open".to_owned() })?;
        let now = self.execute("page.layout", &json!({"page": page}))?;
        let show = |v: f64| {
            let t = format!("{v:.3}");
            t.trim_end_matches('0').trim_end_matches('.').to_owned()
        };
        self.ui.relayout.spacing = Some(SpacingDialog {
            horizontal: show(now["spacing"]["x"].as_f64().unwrap_or(0.375)),
            vertical: show(now["spacing"]["y"].as_f64().unwrap_or(0.375)),
            error: None,
        });
        Ok(())
    }

    pub(crate) fn close_spacing_dialog(&mut self) {
        self.ui.relayout.spacing = None;
    }

    /// The parameters of `page.relayout` that the dialog's draft makes, or the words that refuse it.
    fn relayout_params(&mut self) -> Result<(Value, Option<Vec<u32>>), String> {
        let Some(d) = self.ui.relayout.dialog.as_ref() else { return Err("the dialog is closed".to_owned()) };
        let spacing: f64 = d.spacing.trim().parse().map_err(|_| "Type a spacing in inches, a number more than 0.".to_owned())?;
        if !(spacing.is_finite() && spacing > 0.0) {
            return Err("Type a spacing in inches, a number more than 0.".to_owned());
        }
        let mut params = json!({
            "style": d.style,
            "spacing": spacing,
            "connector_style": d.connector_style,
            "appearance": d.appearance,
            "apply_routing": d.apply_routing,
            "enlarge_page": d.enlarge,
        });
        let (kind, direction, alignment, apply_to) = (d.style.clone(), d.direction.clone(), d.alignment.clone(), d.apply_to);
        if let Some(map) = params.as_object_mut() {
            if matches!(kind.as_str(), "flowchart" | "hierarchy" | "compact_tree") {
                map.insert("direction".to_owned(), json!(direction));
            }
            if kind == "hierarchy" {
                map.insert("alignment".to_owned(), json!(alignment));
            }
        }
        let (page, ids) = self.window_scope().ok_or_else(|| "no document open".to_owned())?;
        if let Some(map) = params.as_object_mut() {
            map.insert("page".to_owned(), json!(page));
            if let (Apply::Selection, Some(ids)) = (apply_to, &ids) {
                map.insert("ids".to_owned(), json!(ids));
            }
        }
        Ok((params, if apply_to == Apply::Selection { ids } else { None }))
    }

    /// Apply and OK: sends the draft as one `page.relayout`. A refusal is said in the dialog, which stays open. → whether it went through.
    fn apply_relayout_dialog(&mut self) -> bool {
        let sent = self.relayout_params().and_then(|(params, ids)| {
            let page = params["page"].as_u64().and_then(|p| u32::try_from(p).ok());
            self.commit_pending_text();
            let before = self.session.revision();
            let answer = self.execute("page.relayout", &params).map_err(|e| e.to_string())?;
            if let (true, Some(page)) = (self.session.revision() != before, page) {
                self.keep_preview(page, ids.as_deref().unwrap_or(&[]));
            }
            self.relayout_notice(&answer);
            Ok(())
        });
        match sent {
            Ok(()) => true,
            Err(words) => {
                if let Some(d) = self.ui.relayout.dialog.as_mut() {
                    d.error = Some(words);
                }
                false
            }
        }
    }

    /// OK of the Spacing Options dialog: `page.spacing`.
    fn apply_spacing_dialog(&mut self) {
        let Some(d) = self.ui.relayout.spacing.as_ref() else { return };
        let parse = |t: &str| t.trim().parse::<f64>().ok().filter(|v| v.is_finite() && *v > 0.0);
        let (Some(horizontal), Some(vertical)) = (parse(&d.horizontal), parse(&d.vertical)) else {
            if let Some(d) = self.ui.relayout.spacing.as_mut() {
                d.error = Some("Type each distance in inches, a number more than 0.".to_owned());
            }
            return;
        };
        let page = self.current_page();
        let mut params = json!({"horizontal": horizontal, "vertical": vertical});
        if let (Some(map), Some(page)) = (params.as_object_mut(), page) {
            map.insert("page".to_owned(), json!(page));
        }
        match self.execute("page.spacing", &params) {
            Ok(_) => self.close_spacing_dialog(),
            Err(e) => {
                if let Some(d) = self.ui.relayout.spacing.as_mut() {
                    d.error = Some(e.to_string());
                }
            }
        }
    }

    /// The dialogs of this feature over the window.
    pub(crate) fn relayout_dialogs(&mut self, ctx: &Context, t: Tokens) {
        if self.ui.relayout.dialog.is_some() {
            self.relayout_dialog(ctx, t);
        }
        if self.ui.relayout.spacing.is_some() {
            self.spacing_dialog(ctx, t);
        }
    }

    fn relayout_dialog(&mut self, ctx: &Context, t: Tokens) {
        let has_selection = self.relayout_has_selection();
        let typing = ctx.text_edit_focused();
        let Some(mut d) = self.ui.relayout.dialog.take() else { return };
        if !has_selection && d.apply_to == Apply::Selection {
            d.apply_to = Apply::Page;
        }
        let (mut ok, mut apply, mut cancel) = (false, false, false);
        let mut seen: Vec<(String, Rect, bool)> = Vec::new();
        let modal = egui::Modal::new(Id::unique("relayout_dialog")).backdrop_color(t.scrim).frame(Self::card_frame(&t)).show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 60.0).clamp(320.0, 520.0));
            ui.label(RichText::new("Configure Layout").font(theme::font(size::TAB + 1.0)).color(t.text));
            ui.add_space(6.0);
            placement_group(ui, &mut d, &t, &mut seen);
            ui.add_space(6.0);
            connectors_group(ui, &mut d, &t, &mut seen);
            ui.add_space(6.0);
            apply_group(ui, &mut d, has_selection, &t, &mut seen);
            ui.add_space(6.0);
            if let Some(error) = &d.error {
                let said = ui.add(egui::Label::new(RichText::new(error).font(theme::font(size::LABEL)).color(t.notice_text)).wrap());
                seen.push(("relayout.error".to_owned(), said.rect, true));
                ui.add_space(4.0);
            }
            ui.horizontal(|ui| {
                for (label, key) in [("OK", "relayout.ok"), ("Cancel", "relayout.cancel"), ("Apply", "relayout.apply")] {
                    let b = ui.add(egui::Button::new(RichText::new(label).font(theme::font(size::LABEL))).min_size(vec2(84.0, 26.0)));
                    seen.push((key.to_owned(), b.rect, true));
                    if b.clicked() {
                        match key {
                            "relayout.ok" => ok = true,
                            "relayout.cancel" => cancel = true,
                            _ => apply = true,
                        }
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
        self.ui.relayout.dialog = Some(d);
        if cancel {
            self.close_relayout_dialog();
        } else if ok || apply {
            // Apply keeps the dialog; OK closes it once what it sent went through.
            let sent = self.apply_relayout_dialog();
            if sent && ok {
                self.close_relayout_dialog();
            }
        }
    }

    fn spacing_dialog(&mut self, ctx: &Context, t: Tokens) {
        let typing = ctx.text_edit_focused();
        let Some(mut d) = self.ui.relayout.spacing.take() else { return };
        let (mut ok, mut cancel) = (false, false);
        let mut seen: Vec<(String, Rect, bool)> = Vec::new();
        let modal = egui::Modal::new(Id::unique("spacing_dialog")).backdrop_color(t.scrim).frame(Self::card_frame(&t)).show(ctx, |ui| {
            ui.set_width(300.0);
            ui.label(RichText::new("Spacing Options").font(theme::font(size::TAB + 1.0)).color(t.text));
            ui.add_space(6.0);
            ui.label(
                RichText::new("The space Auto Space and Re-Layout Page leave between shapes, in inches.")
                    .font(theme::font(size::LABEL))
                    .color(t.text_dim),
            );
            ui.add_space(4.0);
            for (label, key, text) in [("Horizontal", "spacing.horizontal", &mut d.horizontal), ("Vertical", "spacing.vertical", &mut d.vertical)] {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(label).font(theme::font(size::LABEL)).color(t.text));
                    let edit = ui.add(egui::TextEdit::singleline(text).desired_width(70.0).char_limit(12).font(theme::font(size::LABEL)));
                    edit.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, key));
                    seen.push((key.to_owned(), edit.rect, true));
                });
            }
            ui.add_space(6.0);
            if let Some(error) = &d.error {
                let said = ui.add(egui::Label::new(RichText::new(error).font(theme::font(size::LABEL)).color(t.notice_text)).wrap());
                seen.push(("spacing.error".to_owned(), said.rect, true));
                ui.add_space(4.0);
            }
            ui.horizontal(|ui| {
                for (label, key, primary) in [("OK", "spacing.ok", true), ("Cancel", "spacing.cancel", false)] {
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
        self.ui.relayout.spacing = Some(d);
        if cancel {
            self.close_spacing_dialog();
        } else if ok {
            self.apply_spacing_dialog();
        }
    }
}

/// A row of choices: each a button that is selected when `value` holds its name; a press sets it. Each is known by `<key>.<name>`.
fn choices(ui: &mut Ui, key: &str, value: &mut String, list: &[(&str, &str)], enabled: bool, seen: &mut Vec<(String, Rect, bool)>, t: &Tokens) {
    ui.horizontal_wrapped(|ui| {
        for (name, words) in list {
            let chosen = value == name;
            let resp = ui.add_enabled(
                enabled,
                egui::Button::selectable(
                    chosen,
                    RichText::new(*words).font(theme::font(size::LABEL)).color(if enabled { t.text } else { t.text_disabled }),
                ),
            );
            widgets::name_toggle(&resp, enabled, chosen, words);
            seen.push((format!("{key}.{name}"), resp.rect, enabled));
            if resp.clicked() && enabled {
                *value = (*name).to_owned();
            }
        }
    });
}

fn placement_group(ui: &mut Ui, d: &mut RelayoutDialog, t: &Tokens, seen: &mut Vec<(String, Rect, bool)>) {
    ui.label(RichText::new("Placement").font(theme::font(size::LABEL)).color(t.text_dim));
    ui.horizontal(|ui| {
        ui.label(RichText::new("Style").font(theme::font(size::LABEL)).color(t.text));
        choices(ui, "relayout.style", &mut d.style, &KINDS, true, seen, t);
    });
    let (has_direction, has_alignment) = (matches!(d.style.as_str(), "flowchart" | "hierarchy" | "compact_tree"), d.style == "hierarchy");
    ui.horizontal(|ui| {
        ui.label(RichText::new("Direction").font(theme::font(size::LABEL)).color(if has_direction { t.text } else { t.text_disabled }));
        // A direction of the other kind is not one of this style's: a change of style gives the style's first.
        let list: &[(&str, &str)] = if d.style == "compact_tree" { &OUTLINES } else { &FLOWS };
        if has_direction && !list.iter().any(|(name, _)| *name == d.direction) {
            d.direction = list.first().map_or_else(String::new, |(name, _)| (*name).to_owned());
        }
        choices(ui, "relayout.direction", &mut d.direction, list, has_direction, seen, t);
    });
    ui.horizontal(|ui| {
        ui.label(RichText::new("Alignment").font(theme::font(size::LABEL)).color(if has_alignment { t.text } else { t.text_disabled }));
        choices(ui, "relayout.alignment", &mut d.alignment, &ALIGNS, has_alignment, seen, t);
    });
    ui.horizontal(|ui| {
        ui.label(RichText::new("Spacing (in)").font(theme::font(size::LABEL)).color(t.text));
        let edit = ui.add(egui::TextEdit::singleline(&mut d.spacing).desired_width(60.0).char_limit(12).font(theme::font(size::LABEL)));
        edit.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "relayout.spacing"));
        seen.push(("relayout.spacing".to_owned(), edit.rect, true));
        // The Preview: the style chosen, laid out.
        let (rect, _) = ui.allocate_exact_size(vec2(64.0, 44.0), Sense::hover());
        ui.painter().rect_filled(rect, CornerRadius::same(3), t.input_fill);
        let alignment = if d.style == "hierarchy" { Some(d.alignment.as_str()) } else { None };
        let direction = if has_direction { Some(d.direction.as_str()) } else { None };
        if let Some(style) = style_of(&d.style, direction, alignment) {
            paint_style(ui.painter(), rect, style, t, true);
        }
        ui.painter().rect_stroke(rect, CornerRadius::same(3), Stroke::new(1.0, t.input_border), StrokeKind::Inside);
        seen.push(("relayout.preview".to_owned(), rect, true));
    });
}

fn connectors_group(ui: &mut Ui, d: &mut RelayoutDialog, t: &Tokens, seen: &mut Vec<(String, Rect, bool)>) {
    ui.label(RichText::new("Connectors").font(theme::font(size::LABEL)).color(t.text_dim));
    ui.horizontal(|ui| {
        ui.label(RichText::new("Style").font(theme::font(size::LABEL)).color(t.text));
        choices(ui, "relayout.connector_style", &mut d.connector_style, &[("right_angle", "Right Angle"), ("straight", "Straight")], true, seen, t);
    });
    ui.horizontal(|ui| {
        ui.label(RichText::new("Appearance").font(theme::font(size::LABEL)).color(t.text));
        choices(ui, "relayout.appearance", &mut d.appearance, &[("straight", "Straight"), ("curved", "Curved")], true, seen, t);
    });
    for (key, label, value) in [
        ("relayout.apply_routing", "Apply routing style to connectors", &mut d.apply_routing),
        ("relayout.enlarge", "Enlarge page to fit drawing", &mut d.enlarge),
    ] {
        let r = ui.checkbox(value, RichText::new(label).font(theme::font(size::LABEL)).color(t.text));
        seen.push((key.to_owned(), r.rect, true));
    }
}

fn apply_group(ui: &mut Ui, d: &mut RelayoutDialog, has_selection: bool, t: &Tokens, seen: &mut Vec<(String, Rect, bool)>) {
    ui.label(RichText::new("Apply settings to").font(theme::font(size::LABEL)).color(t.text_dim));
    ui.horizontal(|ui| {
        for (which, key, label, enabled) in
            [(Apply::Selection, "relayout.to_selection", "Selection", has_selection), (Apply::Page, "relayout.to_page", "Current page", true)]
        {
            let r = ui.add_enabled(
                enabled,
                egui::RadioButton::new(
                    d.apply_to == which,
                    RichText::new(label).font(theme::font(size::LABEL)).color(if enabled { t.text } else { t.text_disabled }),
                ),
            );
            seen.push((key.to_owned(), r.rect, enabled));
            if r.clicked() && enabled {
                d.apply_to = which;
            }
        }
    });
}

// ---- the page breaks on the canvas ---------------------------------------------------------------------------------------------------

/// Paints the page breaks over the page `frame`: a dashed line along each break, across the page. → how many lines went across and how many down.
pub(crate) fn paint_page_breaks(
    painter: &Painter,
    t: &Tokens,
    view: &crate::canvas::view::View,
    area: Rect,
    frame: &crate::canvas::page_drawing::PageFrame,
    size: (f64, f64),
    lines: &(Vec<f64>, Vec<f64>),
) -> (usize, usize) {
    let clip = frame.rect.intersect(area);
    if clip.width() <= 0.0 || clip.height() <= 0.0 {
        return (0, 0);
    }
    let painter = painter.with_clip_rect(clip);
    let stroke = Stroke::new(1.0, t.page_break);
    let mut drawn = (0, 0);
    for x in lines.0.iter().take(MAX_LINES) {
        let (a, b) = (view.to_screen(area, (*x, 0.0)), view.to_screen(area, (*x, size.1)));
        painter.add(egui::Shape::dashed_line(&[a, b], stroke, 6.0, 4.0));
        drawn.0 += 1;
    }
    for y in lines.1.iter().take(MAX_LINES) {
        let (a, b) = (view.to_screen(area, (0.0, *y)), view.to_screen(area, (size.0, *y)));
        painter.add(egui::Shape::dashed_line(&[a, b], stroke, 6.0, 4.0));
        drawn.1 += 1;
    }
    drawn
}

/// The most break lines one axis draws: a page too large for a print is refused by the engine, and this keeps a frame short whatever it says.
const MAX_LINES: usize = 512;
