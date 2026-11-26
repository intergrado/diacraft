//! The Shapes window: a stencil chooser, a local name filter and a scrollable master gallery.
//! Stencils and master geometry come from the engine. UI filtering never changes the drawing.

use egui::{Align2, CornerRadius, Frame, Rect, Response, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use serde_json::json;

use std::sync::Arc;

use crate::app::DiacraftApp;
use crate::edit::geom::P;
use crate::icon_image;
use crate::icons;
use crate::stencils::{self, Art, Kind, MasterInfo, Scratch, StencilInfo};
use crate::theme::{self, Tokens, size};
use crate::widgets::{self, label_colour, paint_button_bg, tip};

/// The narrowest the window is made, in points; at this width the masters are one to a row.
const MIN_W: f32 = 120.0;

const FILTER_CHARS: usize = 128;
const FILTER_SCAN_CHARS: usize = 4_096;

/// The width of the window folded to a strip.
const STRIP_W: f32 = 26.0;

const PAD: f32 = 12.0;
const GUTTER: f32 = 8.0;
/// The side of a master's tile in points (owner 2026-11-23: a tenth smaller than the 80 it was).
const TILE_SIDE: f32 = 72.0;
const TILE_GAP: f32 = 6.0;
/// The share of a tile's side its picture takes (a 43-point figure in the 72-point card), so the picture keeps its place in the card at any side.
const FIGURE_SHARE: f32 = 0.6;
/// The width the window opens at: it snugly fits two tiles.
const DEFAULT_W: f32 = PAD * 2.0 + TILE_SIDE * 2.0 + TILE_GAP;

/// The side of a tile and the number of columns in a gallery `usable_width` wide (finite and over 0) that holds `total` tiles: the side is [`TILE_SIDE`] unless the
/// gallery is narrower, and as many columns as fit (at least one, at most one for each tile) are made. The width changes the count, never the side.
fn tile_grid(usable_width: f32, total: usize) -> (f32, usize) {
    let side = TILE_SIDE.min(usable_width);
    let columns = ((usable_width + TILE_GAP) / (side + TILE_GAP)).floor().clamp(1.0, total.max(1) as f32) as usize;
    (side, columns)
}

/// One row of the More Shapes menu.
enum MoreRow<'a> {
    /// A stencil the window gathers from the masters of several (the Quick Shapes stencil, the search results): its place in the window's list.
    Gathered(usize),
    /// The name of a category, over its stencils.
    Heading(&'a str),
    /// A stencil that is open: its place in the window's list.
    Open(usize),
    /// A built-in stencil that is not open: its place in `stencil.list`'s `available`.
    Available(usize),
}

/// What a stencil file's rows are filed under.
const FILES_HEADING: &str = "From files";

/// The rows of More Shapes ("a menu of stencil categories"): each category of the built-in stencils in the order it first appears (the open ones lead), with a heading and
/// then its stencils, the open ones before those that are not, and the stencils opened from files under their own heading. A list that names no category (a stencil list
/// the app was given) is the plain list of its stencils, with no heading. **Charged**: one row for each stencil the window holds (at most 64 of each list), and a category
/// is found by a scan of the categories already seen, which are at most the stencils' number.
fn more_rows<'a>(stencils: &'a [StencilInfo], available: &'a [stencils::AvailableStencil]) -> Vec<MoreRow<'a>> {
    let mut categories: Vec<&'a str> = Vec::new();
    for category in stencils
        .iter()
        .filter(|s| s.builtin && !s.is_gathered())
        .filter_map(|s| s.category.as_deref())
        .chain(available.iter().filter_map(|a| a.category.as_deref()))
    {
        if !categories.contains(&category) {
            categories.push(category);
        }
    }
    let mut rows: Vec<MoreRow<'a>> = Vec::with_capacity(stencils.len() + available.len() + categories.len() + 1);
    // The search results lead, then the Quick Shapes stencil: they gather the masters of the others.
    for kind in [Kind::Search, Kind::Quick] {
        rows.extend(stencils.iter().enumerate().filter(|(_, s)| s.kind == kind).map(|(i, _)| MoreRow::Gathered(i)));
    }
    // The stencils with no category lead, with no heading.
    for (i, s) in stencils.iter().enumerate() {
        if s.builtin && s.category.is_none() && !s.is_gathered() {
            rows.push(MoreRow::Open(i));
        }
    }
    for category in &categories {
        rows.push(MoreRow::Heading(category));
        for (i, s) in stencils.iter().enumerate() {
            if s.builtin && !s.is_gathered() && s.category.as_deref() == Some(*category) {
                rows.push(MoreRow::Open(i));
            }
        }
        for (a, s) in available.iter().enumerate() {
            if s.category.as_deref() == Some(*category) {
                rows.push(MoreRow::Available(a));
            }
        }
    }
    // An available stencil with no category (the engine names one for each) is listed too, after the rest.
    for (a, s) in available.iter().enumerate() {
        if s.category.is_none() {
            rows.push(MoreRow::Available(a));
        }
    }
    if stencils.iter().any(|s| !s.builtin && !s.is_gathered()) {
        // The heading goes only where it tells the files from a category; a list of files alone is a plain list.
        if !categories.is_empty() {
            rows.push(MoreRow::Heading(FILES_HEADING));
        }
        for (i, s) in stencils.iter().enumerate() {
            if !s.builtin {
                rows.push(MoreRow::Open(i));
            }
        }
    }
    rows
}

impl DiacraftApp {
    pub(crate) fn shapes_window(&mut self, ui: &mut Ui, t: Tokens) {
        if self.ui.shapes_collapsed {
            let frame = Frame::new().fill(t.panel_header_fill);
            let panel = egui::Panel::left("shapes_strip")
                .exact_size(STRIP_W)
                .show_separator_line(false)
                .frame(frame)
                .show(ui, |ui| self.shapes_strip(ui, &t));
            let rect = panel.response.rect;
            ui.painter().vline(rect.right(), rect.y_range(), Stroke::new(1.0, t.separator));
            return;
        }
        let frame = Frame::new().fill(t.panel_fill);
        let screen = ui.ctx().content_rect().width();
        let default = (screen * 0.24).clamp(MIN_W, DEFAULT_W);
        // A width the person set while the window was large does not stay over a third of a smaller one.
        let max = (screen * 0.34).clamp(MIN_W, 360.0);
        let panel = egui::Panel::left("shapes")
            .resizable(true)
            .default_size(default)
            .size_range(MIN_W..=max)
            .show_separator_line(false)
            .frame(frame)
            .show(ui, |ui| self.shapes_contents(ui, &t));
        let rect = panel.response.rect;
        ui.painter().vline(rect.right(), rect.y_range(), Stroke::new(1.0, t.separator));
    }

    /// The window folded to a strip: an arrow that unfolds it, and the name turned on its side.
    fn shapes_strip(&mut self, ui: &mut Ui, t: &Tokens) {
        let area = ui.max_rect();
        let r = Rect::from_min_size(pos2(area.left() + 2.0, area.top() + 4.0), vec2(STRIP_W - 4.0, 22.0));
        let resp = ui.interact(r, ui.scope_id().with("shapes_expand"), Sense::click());
        widgets::name_button(&resp, true, "Expand the Shapes window");
        paint_button_bg(ui, r, &resp, true, false, t, 3);
        paint_double_chevron(ui, r.center(), false, t.text);
        self.note("shapes.expand", r, true);
        if resp.on_hover_text("Expand the Shapes window").clicked() {
            self.ui.shapes_collapsed = false;
        }
        let galley = ui.painter().layout_no_wrap("Shapes".to_owned(), theme::font(size::LABEL + 1.0), t.text_dim);
        let shape =
            egui::epaint::TextShape::new(pos2(area.center().x - galley.size().y / 2.0, area.top() + 36.0 + galley.size().x), galley, t.text_dim)
                .with_angle(-std::f32::consts::FRAC_PI_2);
        ui.painter().add(egui::Shape::Text(shape));
    }

    fn shapes_contents(&mut self, ui: &mut Ui, t: &Tokens) {
        let width = ui.available_width();
        ui.spacing_mut().item_spacing.y = 0.0;
        let (head, _) = ui.allocate_exact_size(vec2(width, 32.0), Sense::hover());
        ui.painter().rect_filled(head, 0.0, t.panel_header_fill);
        ui.painter().text(pos2(head.left() + PAD, head.center().y), Align2::LEFT_CENTER, "Shapes", theme::font_strong(size::TAB), t.text);
        let arrow = Rect::from_center_size(pos2(head.right() - 16.0, head.center().y), vec2(22.0, 22.0));
        let resp = ui.interact(arrow, ui.scope_id().with("shapes_collapse"), Sense::click());
        widgets::name_button(&resp, true, "Minimize the Shapes window");
        paint_button_bg(ui, arrow, &resp, true, false, t, 3);
        paint_double_chevron(ui, arrow.center(), true, t.text);
        self.note("shapes.collapse", arrow, true);
        if resp.on_hover_text("Minimize the Shapes window").clicked() {
            self.ui.shapes_collapsed = true;
        }
        ui.add_space(PAD);
        let stencils = self.stencil_list();
        self.stencil_chooser(ui, t, width, &stencils);
        ui.add_space(GUTTER);
        self.stencil_filter(ui, t, width);
        // Choosing or closing a stencil may have invalidated the catalogue in this pass.
        let stencils = self.stencil_list();
        let current = self.current_stencil_index();
        if let Some(stencil) = stencils.get(current) {
            self.master_tiles(ui, t, width, stencil);
        }
    }

    /// A compact summary opens the catalogue; imported stencils retain their close controls.
    fn stencil_chooser(&mut self, ui: &mut Ui, t: &Tokens, width: f32, stencils: &[StencilInfo]) {
        let (area, _) = ui.allocate_exact_size(vec2(width, 52.0), Sense::hover());
        let r = area.shrink2(vec2(PAD, 0.0));
        let enabled = !stencils.is_empty() || self.ui.stencils.picker.is_some() || self.has_file_host();
        let resp = ui.interact(r, ui.scope_id().with("shapes_more"), if enabled { Sense::click() } else { Sense::hover() });
        let current = self.current_stencil_index();
        let chosen = stencils.get(current);
        let name = chosen.map_or("Open a stencil", |s| s.name.as_str());
        let count = chosen.map_or(0, |s| s.masters.len());
        widgets::name_button(&resp, enabled, &format!("{name}, {count} shapes. Choose stencil"));
        ui.painter().rect_filled(r, 8.0, t.panel_header_fill);
        paint_button_bg(ui, r, &resp, enabled, false, t, 8);
        single_line(ui, Rect::from_min_max(r.min + vec2(10.0, 7.0), pos2(r.right() - 28.0, r.top() + 28.0)), name, theme::font_strong(13.0), t.text);
        single_line(
            ui,
            Rect::from_min_max(r.min + vec2(10.0, 29.0), r.max - vec2(28.0, 5.0)),
            &format!("{count} shapes"),
            theme::font(11.5),
            t.text_dim,
        );
        icons::chevron(ui.painter(), pos2(r.right() - 14.0, r.center().y), 2.5, label_colour(t, enabled));
        self.note("shapes.more", r, enabled);
        let mut select = None;
        let mut open_builtin: Option<String> = None;
        let mut open_file = false;
        let mut clear_search = false;
        let mut reset_stencil: Option<String> = None;
        let more_available = Arc::clone(&self.ui.stencils.available);
        let rows = more_rows(stencils, &more_available);
        if enabled {
            egui::Popup::menu(&resp.on_hover_text(name)).close_behavior(egui::PopupCloseBehavior::CloseOnClick).show(|ui| {
                let popup_width = 260.0_f32.min((ui.ctx().content_rect().width() - 32.0).max(120.0));
                ui.set_width(popup_width);
                let catalogue = egui::ScrollArea::vertical()
                    .id_salt("stencil_choices")
                    .min_scrolled_height(0.0)
                    .max_height((ui.ctx().content_rect().height() * 0.55).clamp(44.0, 440.0))
                    .show(ui, |ui| {
                        for row in &rows {
                            match row {
                                MoreRow::Heading(category) => {
                                    // A category of the menu: a quiet label above its stencils ("a menu of stencil categories").
                                    let (r, _) = ui.allocate_exact_size(vec2(popup_width, 22.0), Sense::hover());
                                    single_line(
                                        ui,
                                        Rect::from_min_max(r.min + vec2(10.0, 4.0), pos2(r.right() - 8.0, r.bottom() - 1.0)),
                                        category,
                                        theme::font_strong(11.5),
                                        t.text_dim,
                                    );
                                    self.note(format!("shapes.category.{category}"), r, true);
                                }
                                MoreRow::Open(i) => {
                                    let Some(stencil) = stencils.get(*i) else { continue };
                                    let (r, _) = ui.allocate_exact_size(vec2(popup_width, 44.0), Sense::hover());
                                    let hit = r.with_max_x(r.right() - if stencil.builtin { 0.0 } else { 30.0 });
                                    let row = ui.interact(hit, ui.scope_id().with(("stencil_row", &stencil.id)), Sense::click());
                                    paint_button_bg(ui, r, &row, true, *i == current, t, 6);
                                    row.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, *i == current, &stencil.name));
                                    let text_right = r.right() - if stencil.builtin { 10.0 } else { 32.0 };
                                    single_line(
                                        ui,
                                        Rect::from_min_max(r.min + vec2(28.0, 4.0), pos2(text_right, r.top() + 25.0)),
                                        &stencil.name,
                                        theme::font(13.0),
                                        t.text,
                                    );
                                    single_line(
                                        ui,
                                        Rect::from_min_max(r.min + vec2(28.0, 25.0), pos2(text_right, r.bottom() - 3.0)),
                                        &format!("{} shapes", stencil.masters.len()),
                                        theme::font(11.5),
                                        t.text_dim,
                                    );
                                    // The check mark is on every stencil that is open (the row of the one shown is also highlighted).
                                    let c = pos2(r.left() + 14.0, r.center().y);
                                    ui.painter().line_segment([c + vec2(-4.0, 0.0), c + vec2(-1.0, 3.0)], Stroke::new(1.5, t.accent));
                                    ui.painter().line_segment([c + vec2(-1.0, 3.0), c + vec2(5.0, -4.0)], Stroke::new(1.5, t.accent));
                                    self.note(format!("shapes.open.{}", stencil.name), Rect::from_center_size(c, vec2(12.0, 12.0)), true);
                                    self.note(format!("shapes.more.{}", stencil.name), hit, true);
                                    self.note(format!("shapes.stencil.{}", stencil.name), r, true);
                                    if row.on_hover_text(&stencil.name).clicked() {
                                        select = Some(stencil.id.clone());
                                    }
                                    if !stencil.builtin {
                                        self.close_control(ui, t, r, stencil);
                                    }
                                }
                                MoreRow::Gathered(i) => {
                                    // The Quick Shapes stencil and the search results: a row like a stencil's, with no check mark, and a cross on the results that clears them.
                                    let Some(stencil) = stencils.get(*i) else { continue };
                                    let (r, _) = ui.allocate_exact_size(vec2(popup_width, 44.0), Sense::hover());
                                    let results = stencil.kind == Kind::Search;
                                    let hit = r.with_max_x(r.right() - if results { 30.0 } else { 0.0 });
                                    let row = ui.interact(hit, ui.scope_id().with(("stencil_row", &stencil.id)), Sense::click());
                                    paint_button_bg(ui, r, &row, true, *i == current, t, 6);
                                    row.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, *i == current, &stencil.name));
                                    let text_right = r.right() - if results { 32.0 } else { 10.0 };
                                    single_line(
                                        ui,
                                        Rect::from_min_max(r.min + vec2(28.0, 4.0), pos2(text_right, r.top() + 25.0)),
                                        &stencil.name,
                                        theme::font(13.0),
                                        t.text,
                                    );
                                    single_line(
                                        ui,
                                        Rect::from_min_max(r.min + vec2(28.0, 25.0), pos2(text_right, r.bottom() - 3.0)),
                                        &format!("{} shapes", stencil.masters.len()),
                                        theme::font(11.5),
                                        t.text_dim,
                                    );
                                    // A magnifier marks the results, and four small squares the Quick Shapes: the rows are the window's own, not a stencil's.
                                    let c = pos2(r.left() + 14.0, r.center().y);
                                    if results {
                                        icons::paint(
                                            ui.painter(),
                                            Rect::from_center_size(c, vec2(14.0, 14.0)),
                                            icons::Icon::of(icons::Base::Magnifier),
                                            t,
                                            true,
                                        );
                                    } else {
                                        for (dx, dy) in [(-3.5, -3.5), (3.5, -3.5), (-3.5, 3.5), (3.5, 3.5)] {
                                            let square = Rect::from_center_size(c + vec2(dx, dy), vec2(5.5, 5.5));
                                            ui.painter().rect_stroke(square, 1.0, Stroke::new(1.2, t.text_dim), StrokeKind::Inside);
                                        }
                                    }
                                    self.note(format!("shapes.virtual.{}", stencil.name), r, true);
                                    if row.on_hover_text(&stencil.name).clicked() {
                                        select = Some(stencil.id.clone());
                                    }
                                    if results {
                                        let x = Rect::from_center_size(pos2(r.right() - 16.0, r.center().y), vec2(20.0, 20.0));
                                        let resp = ui.interact(x, ui.scope_id().with("search_close"), Sense::click());
                                        widgets::name_button(&resp, true, "Clear the search");
                                        paint_button_bg(ui, x, &resp, true, false, t, 3);
                                        let m = x.center();
                                        let ink = Stroke::new(1.3, t.text);
                                        ui.painter().line_segment([m + vec2(-3.5, -3.5), m + vec2(3.5, 3.5)], ink);
                                        ui.painter().line_segment([m + vec2(-3.5, 3.5), m + vec2(3.5, -3.5)], ink);
                                        self.note("shapes.search.clear", x, true);
                                        if resp.on_hover_text("Clear the search").clicked() {
                                            clear_search = true;
                                        }
                                    }
                                }
                                MoreRow::Available(a) => {
                                    // A built-in stencil that is not open (a template opens the ones its kind needs): no check mark, and a click opens it.
                                    let Some(available) = more_available.get(*a) else { continue };
                                    let (r, _) = ui.allocate_exact_size(vec2(popup_width, 44.0), Sense::hover());
                                    let row = ui.interact(r, ui.scope_id().with(("stencil_row", &available.id)), Sense::click());
                                    paint_button_bg(ui, r, &row, true, false, t, 6);
                                    row.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, false, &available.name));
                                    let text_right = r.right() - 10.0;
                                    single_line(
                                        ui,
                                        Rect::from_min_max(r.min + vec2(28.0, 4.0), pos2(text_right, r.top() + 25.0)),
                                        &available.name,
                                        theme::font(13.0),
                                        t.text,
                                    );
                                    single_line(
                                        ui,
                                        Rect::from_min_max(r.min + vec2(28.0, 25.0), pos2(text_right, r.bottom() - 3.0)),
                                        &format!("{} shapes, not open", available.masters),
                                        theme::font(11.5),
                                        t.text_dim,
                                    );
                                    self.note(format!("shapes.more.{}", available.name), r, true);
                                    if row.on_hover_text(format!("Open {}", available.name)).clicked() {
                                        open_builtin = Some(available.id.clone());
                                    }
                                }
                            }
                        }
                    });
                self.note("shapes.catalogue", catalogue.inner_rect, true);
                ui.separator();
                let can = self.can_pick_stencil();
                let (rect, resp) = ui.allocate_exact_size(vec2(popup_width, 28.0), if can { Sense::click() } else { Sense::hover() });
                widgets::name_button(&resp, can, "Open Stencil");
                paint_button_bg(ui, rect, &resp, can, false, t, 4);
                ui.painter().text(
                    pos2(rect.left() + 10.0, rect.center().y),
                    Align2::LEFT_CENTER,
                    "Open Stencil",
                    theme::font(13.0),
                    label_colour(t, can),
                );
                self.note("shapes.more.open", rect, can);
                if resp.on_hover_text("Open Stencil").clicked() && can {
                    open_file = true;
                }
                // Reset Stencil: the stencil on view has the Quick Shapes it came with. Quiet where there is nothing to give back.
                let shown_stencil = stencils.get(current).filter(|s| !s.is_gathered());
                let why = match shown_stencil {
                    None => Some("this row is not a stencil"),
                    Some(s) if !s.customized => Some("the Quick Shapes are the stencil's own"),
                    Some(_) if self.session.document().is_none() => Some("no drawing is open"),
                    Some(_) => None,
                };
                let (rect, resp) = ui.allocate_exact_size(vec2(popup_width, 28.0), if why.is_none() { Sense::click() } else { Sense::hover() });
                widgets::name_button(&resp, why.is_none(), "Reset Stencil");
                paint_button_bg(ui, rect, &resp, why.is_none(), false, t, 4);
                ui.painter().text(
                    pos2(rect.left() + 10.0, rect.center().y),
                    Align2::LEFT_CENTER,
                    "Reset Stencil",
                    theme::font(13.0),
                    label_colour(t, why.is_none()),
                );
                self.note("shapes.resetStencil", rect, why.is_none());
                let tip_text =
                    why.map_or_else(|| "Reset Stencil: give the stencil its own Quick Shapes back".to_owned(), |w| format!("Reset Stencil ({w})"));
                if resp.on_hover_text(tip_text).clicked()
                    && why.is_none()
                    && let Some(s) = shown_stencil
                {
                    reset_stencil = Some(s.id.clone());
                }
            });
        }
        // A built-in stencil that was not open is opened, and then shown (`stencil.open {id}`, `window.stencil`).
        if let Some(id) = open_builtin {
            match self.execute("stencil.open", &json!({"id": id})) {
                Ok(_) => select = Some(id),
                Err(e) => self.set_notice(&e.to_string()),
            }
        }
        if let Some(id) = select
            && let Err(e) = self.execute("window.stencil", &json!({"stencil": id}))
        {
            self.set_notice(&e.to_string());
        }
        if open_file && let Err(e) = self.execute("shapes.openStencil", &json!({})) {
            self.set_notice(&e.to_string());
        }
        if clear_search && let Err(e) = self.execute("shapes.search", &json!({"text": ""})) {
            self.set_notice(&e.to_string());
        }
        if let Some(id) = reset_stencil
            && let Err(e) = self.execute("stencil.reset", &json!({"stencil": id}))
        {
            self.set_notice(&e.to_string());
        }
    }

    /// The search box: the prompt **Search shapes**, a magnifier at its right, a cross that clears the words. **Typing narrows the masters of the stencil on
    /// view by name** (the filter), **and Enter or the magnifier searches every stencil** (`shapes.search`): the results are the stencil `search`, each stencil's
    /// matches under its name. Clearing the words clears the search.
    fn stencil_filter(&mut self, ui: &mut Ui, t: &Tokens, width: f32) {
        let (area, _) = ui.allocate_exact_size(vec2(width, 28.0), Sense::hover());
        let r = area.shrink2(vec2(PAD, 0.0));
        let id = ui.scope_id().with("shape_filter");
        let mut query = self.ui.stencils.filter.clone();
        let has_query = !query.is_empty();
        ui.painter().rect(r, 5.0, t.input_fill, Stroke::new(1.0, t.input_border), StrokeKind::Inside);
        let magnifier = Rect::from_center_size(pos2(r.right() - 14.0, r.center().y), vec2(22.0, 22.0));
        let clear = Rect::from_center_size(pos2(magnifier.left() - 11.0, r.center().y), vec2(20.0, 22.0));
        let text_rect = r.shrink2(vec2(7.0, 2.0)).with_max_x(r.right() - 28.0 - if has_query { 22.0 } else { 0.0 });
        let mut buffer = ShapeFilterBuffer(&mut query);
        let response = ui.put(
            text_rect,
            egui::TextEdit::singleline(&mut buffer)
                .id(id)
                .font(theme::font(12.0))
                .frame(Frame::NONE)
                .margin(vec2(0.0, 2.0))
                .hint_text("Search shapes"),
        );
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Search shapes"));
        self.ui.stencils.filter_focus = response.has_focus().then_some(id);
        if response.has_focus() {
            ui.painter().rect_stroke(r, 5.0, Stroke::new(1.5, t.accent), StrokeKind::Inside);
        }
        // Enter in the box searches, and the box keeps the keyboard so that the words can be changed and searched again.
        let enter = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if enter {
            ui.memory_mut(|m| m.request_focus(id));
        }
        self.note("shapes.search", r, true);
        self.note("shapes.filter", text_rect, true);
        if has_query {
            let response = ui.interact(clear, id.with("clear"), Sense::click());
            widgets::name_button(&response, true, "Clear shape filter");
            paint_button_bg(ui, clear, &response, true, false, t, 4);
            let c = clear.center();
            ui.painter().line_segment([c + vec2(-3.0, -3.0), c + vec2(3.0, 3.0)], Stroke::new(1.2, t.text_dim));
            ui.painter().line_segment([c + vec2(-3.0, 3.0), c + vec2(3.0, -3.0)], Stroke::new(1.2, t.text_dim));
            self.note("shapes.filter.clear", clear, true);
            if response.on_hover_text("Clear shape filter").clicked() {
                query.clear();
                ui.memory_mut(|m| m.request_focus(id));
            }
        }
        let go = ui.interact(magnifier, id.with("go"), Sense::click());
        widgets::name_button(&go, has_query, "Search every stencil");
        paint_button_bg(ui, magnifier, &go, has_query, false, t, 4);
        icons::paint(
            ui.painter(),
            Rect::from_center_size(magnifier.center(), vec2(14.0, 14.0)),
            icons::Icon::of(icons::Base::Magnifier),
            t,
            has_query,
        );
        self.note("shapes.search.button", magnifier, has_query);
        let searched = go.on_hover_text("Search every stencil for these words (Enter)").clicked() && has_query;
        if query != self.ui.stencils.filter
            && let Err(e) = self.execute("window.shapeFilter", &json!({"query": query}))
        {
            self.set_notice(&e.to_string());
        }
        if (enter || searched) && !self.ui.stencils.filter.trim().is_empty() {
            let words = self.ui.stencils.filter.clone();
            if let Err(e) = self.execute("shapes.search", &json!({"text": words})) {
                self.set_notice(&e.to_string());
            }
        }
    }

    /// The close control at the right of the row of a stencil file: a cross; it sends `stencil.close` and the row goes.
    fn close_control(&mut self, ui: &mut Ui, t: &Tokens, row: Rect, stencil: &StencilInfo) {
        let r = Rect::from_center_size(pos2(row.right() - 16.0, row.center().y), vec2(20.0, 20.0));
        let resp = ui.interact(r, ui.scope_id().with(("stencil_close", &stencil.id)), Sense::click());
        let label = format!("Close {}", stencil.name);
        widgets::name_button(&resp, true, &label);
        paint_button_bg(ui, r, &resp, true, false, t, 3);
        let c = r.center();
        let ink = Stroke::new(1.3, t.text);
        ui.painter().line_segment([c + vec2(-3.5, -3.5), c + vec2(3.5, 3.5)], ink);
        ui.painter().line_segment([c + vec2(-3.5, 3.5), c + vec2(3.5, -3.5)], ink);
        self.note(format!("shapes.stencilClose.{}", stencil.name), r, true);
        if resp.on_hover_text(tip(&label, true)).clicked()
            && let Err(e) = self.execute("shapes.closeStencil", &json!({"id": stencil.id}))
        {
            self.set_notice(&e.to_string());
        }
    }

    /// Only matching actual masters are laid out. Geometry is read only for tiles in view. **A stencil's gallery is its Quick Shapes above a thin divider and the rest below it**
    /// And the Quick Shapes stencil and the search results are one run of tiles for each stencil their masters are in, under its name.
    fn master_tiles(&mut self, ui: &mut Ui, t: &Tokens, width: f32, stencil: &StencilInfo) {
        // The results are not narrowed by name: they matched on words of the names and of the search words.
        let query = if stencil.kind == Kind::Search { String::new() } else { self.ui.stencils.filter.to_lowercase() };
        let parts = sections(stencil, &query);
        let total: usize = parts.iter().map(|p| p.masters.len()).sum();
        let mut names = std::collections::HashMap::new();
        for master in &stencil.masters {
            *names.entry(master.name.as_str()).or_insert(0_usize) += 1;
        }
        let mut scroll = egui::ScrollArea::vertical()
            .id_salt("shapes_masters")
            .auto_shrink([false, false])
            .min_scrolled_height(0.0)
            .max_height(ui.available_height().max(0.0));
        if std::mem::take(&mut self.ui.stencils.reset_scroll) {
            scroll = scroll.vertical_scroll_offset(0.0);
        }
        let notes: Vec<String> = match (stencil.kind, self.ui.stencils.search.as_ref()) {
            (Kind::Search, Some(found)) => found.notes.clone(),
            _ => Vec::new(),
        };
        let mut zones: Option<crate::state::Zones> = None;
        let gallery = scroll.show(ui, |ui| {
            ui.add_space(PAD);
            if self.session.document().is_none() {
                gallery_message(ui, width, "Create or open a drawing to place shapes.", t);
                ui.add_space(GUTTER);
            }
            if parts.iter().all(|p| p.masters.is_empty() && p.hint.is_none()) {
                let message = match (stencil.kind, stencil.masters.is_empty()) {
                    (Kind::Search, _) => "No shapes match these words",
                    (Kind::Quick, true) => "No Quick Shapes yet",
                    (_, true) => "This stencil has no shapes.",
                    (..) => "No matching shapes",
                };
                gallery_message(ui, width, message, t);
                self.note("shapes.empty", ui.min_rect(), false);
                let advice = match (stencil.kind, stencil.masters.is_empty()) {
                    (Kind::Search, _) => Some("Try fewer words, or the other spelling of one."),
                    (Kind::Quick, true) => Some("Add a shape with its right-click menu."),
                    (_, false) => Some("Try another name in this stencil."),
                    _ => None,
                };
                if let Some(advice) = advice {
                    ui.add_space(6.0);
                    gallery_message(ui, width, advice, t);
                }
                for note in &notes {
                    ui.add_space(6.0);
                    gallery_message(ui, width, note, t);
                }
                return;
            }
            let inset = PAD.min(width.max(0.0) * 0.5);
            let usable_width = (width - inset * 2.0).max(0.0);
            if !usable_width.is_finite() || usable_width <= 0.0 {
                return;
            }
            // A real panel fits a 72-point square. Even an artificially narrower surface keeps
            // all cards equally square, instead of stretching or sizing them from their labels.
            let (side, columns) = tile_grid(usable_width, total);
            let top = ui.cursor().top();
            let mut quick_end: Option<f32> = None;
            let mut quick_tiles: Vec<Rect> = Vec::new();
            for part in &parts {
                if part.divider {
                    // The divider between the Quick Shapes and the rest: a thin line, with room above and below.
                    let (area, _) = ui.allocate_exact_size(vec2(width, 13.0), Sense::hover());
                    ui.painter().hline(area.left() + inset..=area.right() - inset, area.center().y, Stroke::new(1.0, t.separator));
                    self.note("shapes.quickDivider", area, true);
                    quick_end = Some(area.center().y);
                }
                if let Some(heading) = part.heading {
                    let (area, _) = ui.allocate_exact_size(vec2(width, 22.0), Sense::hover());
                    single_line(
                        ui,
                        Rect::from_min_max(area.min + vec2(inset, 3.0), pos2(area.right() - inset, area.bottom() - 1.0)),
                        heading,
                        theme::font_strong(11.5),
                        t.text_dim,
                    );
                    self.note(format!("shapes.group.{heading}"), area, true);
                }
                if let Some(hint) = part.hint {
                    let (area, _) = ui.allocate_exact_size(vec2(width, 22.0), Sense::hover());
                    single_line(
                        ui,
                        Rect::from_min_max(area.min + vec2(inset, 3.0), pos2(area.right() - inset, area.bottom() - 1.0)),
                        hint,
                        theme::font(12.0),
                        t.text_dim,
                    );
                    self.note("shapes.quickHint", area, true);
                }
                if part.masters.is_empty() {
                    continue;
                }
                let rows = part.masters.len().div_ceil(columns);
                let (area, _) = ui.allocate_exact_size(vec2(width, rows as f32 * (side + TILE_GAP) - TILE_GAP + PAD), Sense::hover());
                for (i, master) in part.masters.iter().enumerate() {
                    let r = Rect::from_min_size(
                        pos2(area.left() + inset + (i % columns) as f32 * (side + TILE_GAP), area.top() + (i / columns) as f32 * (side + TILE_GAP)),
                        vec2(side, side),
                    );
                    if part.quick {
                        quick_tiles.push(r);
                    }
                    let unique_name = names.get(master.name.as_str()) == Some(&1);
                    self.master_tile(ui, t, r, master, unique_name, part.quick);
                }
                if part.quick {
                    quick_end = Some(area.bottom() - PAD);
                }
            }
            for note in &notes {
                gallery_message(ui, width, note, t);
                ui.add_space(6.0);
            }
            // Where a master let go inside the window changes its group: above the divider it becomes a Quick Shape, below it is one no more.
            if let Some(divider) = quick_end.filter(|_| stencil.kind == Kind::Normal) {
                let clip = ui.clip_rect();
                zones = Some(crate::state::Zones {
                    stencil: stencil.id.clone(),
                    quick: Rect::from_min_max(pos2(clip.left(), top.max(clip.top())), pos2(clip.right(), divider.max(top))),
                    rest: Rect::from_min_max(pos2(clip.left(), divider.max(clip.top())), pos2(clip.right(), clip.bottom().max(divider))),
                    tiles: quick_tiles,
                });
            }
        });
        self.ui.stencils.zones = zones;
        self.note("shapes.gallery", gallery.inner_rect, true);
    }

    /// One tile. `quick`: it is in its stencil's Quick Shapes group (the part above the divider). **Right-click** opens its menu: Add to Quick Shapes,
    /// or Remove from Quick Shapes.
    fn master_tile(&mut self, ui: &mut Ui, t: &Tokens, r: Rect, master: &MasterInfo, unique_name: bool, quick: bool) {
        // Expensive master/bitmap artwork is read only for the uniform cards in view.
        let art = if r.intersects(ui.clip_rect()) { self.art_of(ui.ctx(), master) } else { Art::Box(master.width, master.height) };
        let enabled = self.session.document().is_some();
        let key = master.key();
        let resp = ui.interact(r, ui.scope_id().with(("master", &key)), if enabled { Sense::click_and_drag() } else { Sense::hover() });
        widgets::name_button(&resp, enabled, &master.name);
        let dragging = resp.drag_started() || self.tile_drag_key().as_deref() == Some(key.as_str());
        let painter = ui.painter();
        let fill = if dragging {
            t.accent_soft
        } else if enabled && resp.is_pointer_button_down_on() {
            t.pressed
        } else if enabled && resp.hovered() {
            t.hover
        } else {
            t.panel_header_fill
        };
        painter.rect_filled(r, 6.0, fill);
        if enabled && (dragging || resp.has_focus()) {
            painter.rect_stroke(r, 6.0, Stroke::new(1.5, t.accent), StrokeKind::Inside);
        }
        let preview_side = r.width() * FIGURE_SHARE;
        let figure = Rect::from_center_size(r.center(), vec2(preview_side, preview_side));
        paint_art(painter, figure, &art, t, enabled);
        // One inspection alias per tile keeps the full 2,048-master catalogue inside the UI report budget.
        // Names are retained for compatibility; duplicate names use the stable master key instead.
        self.note(if unique_name { format!("shapes.tile.{}", master.name) } else { format!("shapes.tileId.{key}") }, r, enabled);
        if enabled {
            self.tile_input(ui, &resp, master);
            self.tile_menu(&resp, master, quick);
        }
        resp.on_hover_text(&master.name);
    }

    /// The right-click menu of a tile: **Add to Quick Shapes** for a master below the divider, **Remove from Quick Shapes** for one above it (`stencil.add_quick`,
    /// `stencil.remove_quick`). A master of the search results is in the group of the stencil it was found in.
    fn tile_menu(&mut self, tile: &Response, master: &MasterInfo, quick: bool) {
        let t = Tokens::get(&tile.ctx);
        let stencil = master.home.as_ref().map(|h| h.id.clone()).or_else(|| self.current_stencil());
        let Some(stencil) = stencil else { return };
        let mut chosen: Option<bool> = None;
        egui::Popup::context_menu(tile).close_behavior(egui::PopupCloseBehavior::CloseOnClick).show(|ui| {
            ui.set_width(210.0);
            // In the results a master may or may not be one of its stencil's Quick Shapes: the stencil's own list says.
            let in_group = quick || self.is_quick_shape(&stencil, &master.id);
            let (label, add) = if in_group { ("Remove from Quick Shapes", false) } else { ("Add to Quick Shapes", true) };
            let verdict = self.command_enabled(if add { "stencil.add_quick" } else { "stencil.remove_quick" });
            let enabled = verdict.is_ok();
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), if enabled { Sense::click() } else { Sense::hover() });
            widgets::name_button(&resp, enabled, label);
            paint_button_bg(ui, rect, &resp, enabled, false, &t, 3);
            ui.painter().text(
                pos2(rect.left() + 12.0, rect.center().y),
                Align2::LEFT_CENTER,
                label,
                theme::font(size::LABEL),
                label_colour(&t, enabled),
            );
            self.note(if add { "shapes.tileMenu.add" } else { "shapes.tileMenu.remove" }, rect, enabled);
            if let Err(why) = &verdict {
                resp.clone().on_hover_text(format!("{label} ({why})"));
            }
            if resp.clicked() && enabled {
                chosen = Some(add);
            }
        });
        let Some(add) = chosen else { return };
        let command = if add { "stencil.add_quick" } else { "stencil.remove_quick" };
        if let Err(e) = self.execute(command, &json!({"stencil": stencil, "master": master.id})) {
            self.set_notice(&e.to_string());
        }
    }

    /// Whether the master `master` is one of the Quick Shapes the window lists for the stencil `stencil`.
    fn is_quick_shape(&self, stencil: &str, master: &str) -> bool {
        self.ui
            .stencils
            .list
            .as_ref()
            .and_then(|list| list.iter().find(|s| s.id == stencil && !s.is_gathered()))
            .is_some_and(|s| s.masters.iter().take(s.quick).any(|m| m.id == master))
    }
}

/// A run of masters in the gallery.
#[derive(Debug)]
pub(crate) struct Section<'a> {
    /// The name of the stencil the masters are in, where the gallery gathers several.
    pub heading: Option<&'a str>,
    /// What an empty Quick Shapes group says.
    pub hint: Option<&'static str>,
    pub masters: Vec<&'a MasterInfo>,
    /// A divider stands above it: it follows the Quick Shapes group.
    pub divider: bool,
    /// The masters are the stencil's Quick Shapes.
    pub quick: bool,
}

/// What an empty Quick Shapes group says.
const QUICK_HINT: &str = "Drop Quick Shapes here";

/// The runs the gallery of `stencil` is laid out in, with only the masters whose names hold `query` (lower case). **A stencil**: its Quick Shapes, a divider, the rest (the divider and
/// the group are left out when there is no Quick Shape, and a group the drawing emptied shows its hint). **The Quick Shapes stencil**: a run for each stencil, headed by its name when there are
/// several. **The search results**: a run for each stencil that has a match, headed by its name.
pub(crate) fn sections<'a>(stencil: &'a StencilInfo, query: &str) -> Vec<Section<'a>> {
    let wanted = |m: &&MasterInfo| query.is_empty() || m.name.to_lowercase().contains(query);
    match stencil.kind {
        Kind::Normal => {
            let (group, rest) = stencil.masters.split_at(stencil.quick.min(stencil.masters.len()));
            let group: Vec<&MasterInfo> = group.iter().filter(wanted).collect();
            let rest: Vec<&MasterInfo> = rest.iter().filter(wanted).collect();
            let hint = (stencil.customized && stencil.quick == 0 && query.is_empty()).then_some(QUICK_HINT);
            let mut out = Vec::new();
            let has_group = !group.is_empty() || hint.is_some();
            if has_group {
                out.push(Section { heading: None, hint, masters: group, divider: false, quick: true });
            }
            if !rest.is_empty() {
                out.push(Section { heading: None, hint: None, masters: rest, divider: has_group, quick: false });
            }
            out
        }
        Kind::Quick | Kind::Search => {
            let mut out: Vec<Section<'a>> = Vec::new();
            let mut last: Option<&str> = None;
            for m in stencil.masters.iter().filter(wanted) {
                let home = m.home.as_ref().map(|h| (h.id.as_str(), h.name.as_str()));
                if out.is_empty() || last != home.map(|h| h.0) {
                    out.push(Section { heading: home.map(|h| h.1), hint: None, masters: Vec::new(), divider: false, quick: false });
                    last = home.map(|h| h.0);
                }
                if let Some(section) = out.last_mut() {
                    section.masters.push(m);
                }
            }
            // The Quick Shapes stencil names a stencil's group when there are several: several stencils, whatever the filter leaves.
            let homes = stencil
                .masters
                .iter()
                .filter_map(|m| m.home.as_ref().map(|h| h.id.as_str()))
                .fold((None, 0_usize), |(last, n), id| (Some(id), n + usize::from(last != Some(id))));
            if stencil.kind == Kind::Quick && homes.1 < 2 {
                for section in &mut out {
                    section.heading = None;
                }
            }
            if stencil.kind == Kind::Quick && stencil.masters.is_empty() {
                out.push(Section { heading: None, hint: Some(QUICK_HINT), masters: Vec::new(), divider: false, quick: false });
            }
            out
        }
    }
}

/// A tile's figure as a thin, unfilled graphite outline; bitmap icons keep their own appearance.
pub(crate) fn paint_art(painter: &egui::Painter, rect: Rect, art: &Art, t: &Tokens, enabled: bool) {
    let ink = if enabled { t.stencil_ink } else { t.stencil_ink_disabled };
    let side = rect.width().min(rect.height());
    let pen = Stroke::new(icons::STENCIL_STROKE_WIDTH, ink);
    match art {
        Art::Icon(base) => icons::paint_stencil(painter, rect, *base, t, enabled),
        Art::Image(texture, (w, h)) => {
            // The icon keeps its proportions inside the square, and its own colours (a quiet one is dimmed).
            let fit = side / w.max(*h).max(1.0);
            let shown = Rect::from_center_size(rect.center(), vec2(w * fit, h * fit));
            let tint = if enabled { t.image_tint } else { t.image_tint_disabled };
            painter.image(*texture, shown, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), tint);
        }
        Art::Box(w, h) => {
            let longest = w.max(*h).max(1.0e-6);
            let size = vec2((w / longest) as f32, (h / longest) as f32) * side * 0.76;
            painter.rect(Rect::from_center_size(rect.center(), size), CornerRadius::same(2), t.stencil_fill, pen, StrokeKind::Inside);
        }
        Art::Outline(figure) => {
            // The figure fits a square with a margin; its longer side is that square's side, and y runs up.
            let square = Rect::from_center_size(rect.center(), vec2(side, side)).shrink(side * 0.12);
            let scale = f64::from(square.width());
            let (aw, ah) = figure.aspect;
            let origin = pos2(square.center().x - (aw * scale / 2.0) as f32, square.center().y + (ah * scale / 2.0) as f32);
            let at = |p: &P| pos2(origin.x + (p.0 * scale) as f32, origin.y - (p.1 * scale) as f32);
            for (points, _) in &figure.areas {
                if figure.separate_strokes {
                    // This contour already repeats its first point. Separate edges prevent
                    // acute shoulder joins from meeting across gaps in compact previews.
                    for pair in points.windows(2) {
                        if let [a, b] = pair {
                            painter.line_segment([at(a), at(b)], pen);
                        }
                    }
                } else {
                    painter.add(egui::Shape::closed_line(points.iter().map(at).collect(), pen));
                }
            }
            // The lines as the master's pattern says, and the marks of its line ends (`tile_ends.rs`).
            crate::tile_ends::paint(painter, figure, &at, pen);
        }
    }
}

impl DiacraftApp {
    /// Gives the Shapes window the stencils in `list` (an answer of `stencil.list`) instead of the engine's own: for an app whose engine
    /// has others, and for a test.
    pub fn with_stencil_list(mut self, list: &serde_json::Value) -> DiacraftApp {
        self.ui.stencils.list = Some(Arc::new(stencils::parse(list)));
        self.ui.stencils.fixed = true;
        self
    }

    /// The stencils the window lists: the session's `stencil.list` (its own and the files opened), read again when the stencils opened change.
    pub fn stencil_list(&mut self) -> Arc<Vec<StencilInfo>> {
        if !self.ui.stencils.fixed {
            // The built-in stencils that are open change with the drawing's template, so they are part of what the list was read for.
            let stencils = self.session.stencils();
            let opened: Vec<String> =
                stencils.open_builtins().iter().map(|id| (*id).to_owned()).chain(stencils.iter().map(|s| s.id().to_owned())).collect();
            if self.ui.stencils.list.is_none() || self.ui.stencils.listed_for != opened {
                let answer = self.session.execute("stencil.list", &json!({})).ok();
                let mut read = answer.as_ref().map(stencils::parse).unwrap_or_else(stencils::builtin);
                self.ui.stencils.available = Arc::new(answer.as_ref().map(stencils::parse_available).unwrap_or_default());
                // The Quick Shapes of the drawing: each stencil's group is put first in it, and the Quick Shapes stencil gathers them all (`stencil.quick`). A drawing that is not
                // open has each stencil's own.
                if let Ok(quick) = self.session.execute("stencil.quick", &json!({}))
                    && let Some(gathered) = stencils::apply_quick(&mut read, &quick)
                {
                    read.push(gathered);
                }
                if let Some(found) = &self.ui.stencils.search {
                    read.push(found.stencil.clone());
                }
                self.ui.stencils.list = Some(Arc::new(read));
                self.ui.stencils.listed_for = opened;
            }
        }
        let list = Arc::clone(self.ui.stencils.list.get_or_insert_with(|| Arc::new(stencils::builtin())));
        let selected = self.ui.stencils.current.as_ref().and_then(|id| list.iter().find(|s| &s.id == id)).or(list.first()).map(|s| s.id.clone());
        if selected != self.ui.stencils.filter_stencil {
            // The search results show the words that were searched for, in the box.
            let shows_search = selected.as_deref() == Some(stencils::SEARCH_ID);
            self.ui.stencils.filter_stencil = selected;
            if !shows_search {
                self.ui.stencils.filter.clear();
            }
            self.ui.stencils.reset_scroll = true;
            self.cancel_hidden_tile_drag();
        }
        list
    }

    /// The place in the list of the stencil whose masters are shown: the one chosen, else the first.
    pub(crate) fn current_stencil_index(&mut self) -> usize {
        let list = self.stencil_list();
        self.ui.stencils.current.as_ref().and_then(|id| list.iter().position(|s| &s.id == id)).unwrap_or(0)
    }

    /// The id of the stencil whose masters are shown.
    pub fn current_stencil(&mut self) -> Option<String> {
        let at = self.current_stencil_index();
        self.stencil_list().get(at).map(|s| s.id.clone())
    }

    /// Makes the stencil with this id or name (any case) the one whose masters are shown. → whether there is one.
    pub fn show_stencil(&mut self, which: &str) -> bool {
        let found = self.stencil_list().iter().find(|s| s.id.eq_ignore_ascii_case(which) || s.name.eq_ignore_ascii_case(which)).map(|s| s.id.clone());
        let known = found.is_some();
        if let Some(id) = found {
            self.ui.stencils.current = Some(id);
            self.stencil_list();
        }
        known
    }

    /// UI and API share one bounded normalization path; the document and its undo history are untouched.
    pub(crate) fn set_shape_filter(&mut self, query: &str) {
        self.stencil_list();
        let query = normalized_filter(query, FILTER_CHARS);
        if query != self.ui.stencils.filter {
            // Words taken out of the box until the prompt shows again reset the search.
            let emptied =
                query.trim().is_empty() && self.ui.stencils.search.is_some() && self.current_stencil().as_deref() == Some(stencils::SEARCH_ID);
            self.ui.stencils.filter = query;
            self.ui.stencils.reset_scroll = true;
            self.cancel_hidden_tile_drag();
            if emptied {
                self.reset_search();
            }
        }
    }

    /// The `shapes.search` command: searches every stencil for the words of `text` (`stencil.search`) and shows the results as the stencil `search`, each stencil's matches under its
    /// name. Blank text clears the search (and the stencil shown before it is shown again). `mode` (`all` or `any`) sets whether a master needs every word or one (Options, Advanced,
    /// Shape search: AND is the default) and, alone, searches the words of the last search again. A refusal is the engine's, in words, and the results stay as they were.
    pub(crate) fn command_search(&mut self, text: Option<&str>, mode: Option<&str>) -> diacraft_engine::Result<()> {
        if let Some(mode) = mode {
            self.ui.stencils.search_any = match mode.trim() {
                "all" => false,
                "any" => true,
                other => {
                    return Err(diacraft_engine::EngineError::BadParams {
                        cmd: "shapes.search".to_owned(),
                        msg: format!("`mode` must be `all` or `any`, not `{}`", diacraft_engine::shown(other, 16)),
                    });
                }
            };
        }
        let words = match text {
            Some(text) => normalized_filter(text, FILTER_CHARS),
            None => self.ui.stencils.search.as_ref().map(|f| f.text.clone()).unwrap_or_default(),
        };
        if words.trim().is_empty() {
            self.reset_search();
            return Ok(());
        }
        let any = self.ui.stencils.search_any;
        let answer = self.execute("stencil.search", &json!({"text": words, "mode": if any { "any" } else { "all" }}))?;
        let Some(found) = stencils::parse_search(&answer) else {
            return Err(diacraft_engine::EngineError::Internal { cmd: "shapes.search".to_owned(), msg: "the search answered no results".to_owned() });
        };
        if self.ui.stencils.search.is_none() {
            self.ui.stencils.before_search = self.current_stencil();
        }
        self.ui.stencils.search = Some(found);
        self.ui.stencils.filter = words;
        self.ui.stencils.current = Some(stencils::SEARCH_ID.to_owned());
        self.ui.stencils.forget_list();
        let _ = self.stencil_list();
        self.ui.stencils.reset_scroll = true;
        Ok(())
    }

    /// What `window_state` says of the search for shapes: `null` when none is shown, else the words, the mode, how many masters matched and how many are listed, and whether the
    /// search reached the end.
    pub(crate) fn shape_search_state(&self) -> serde_json::Value {
        let Some(found) = self.ui.stencils.search.as_ref() else { return serde_json::Value::Null };
        json!({
            "text": found.text,
            "mode": if self.ui.stencils.search_any { "any" } else { "all" },
            "matches": found.matches,
            "listed": found.stencil.masters.len(),
            "complete": found.complete,
        })
    }

    /// Clears the search: its results go, and the stencil that was shown before it is shown again.
    fn reset_search(&mut self) {
        if self.ui.stencils.search.take().is_none() {
            return;
        }
        let was_shown = self.ui.stencils.current.as_deref() == Some(stencils::SEARCH_ID);
        let before = self.ui.stencils.before_search.take();
        if was_shown {
            self.ui.stencils.current = before;
            self.ui.stencils.filter.clear();
        }
        self.ui.stencils.forget_list();
        let _ = self.stencil_list();
        self.ui.stencils.reset_scroll = true;
    }

    /// A master of the Shapes window let go **inside the gallery** ("a person adds or removes them by dragging across the divider"): let go above the divider
    /// it becomes a Quick Shape of its stencil, at the tile it was let go on (the end when on none), and let go below it, it is one no more. → whether the window took it (the pointer
    /// was in either part), so that it is not dropped on the page.
    pub(crate) fn drop_in_gallery(&mut self, ctx: &egui::Context, master: &MasterInfo) -> bool {
        let Some(zones) = self.ui.stencils.zones.clone() else { return false };
        let Some(pointer) = ctx.input(|i| i.pointer.latest_pos()) else { return false };
        let result = if zones.quick.contains(pointer) {
            let at = zones.tiles.iter().position(|r| r.contains(pointer));
            let mut params = json!({"stencil": zones.stencil, "master": master.id});
            if let (Some(at), Some(map)) = (at, params.as_object_mut()) {
                map.insert("at".to_owned(), json!(at));
            }
            self.execute("stencil.add_quick", &params)
        } else if zones.rest.contains(pointer) {
            self.execute("stencil.remove_quick", &json!({"stencil": zones.stencil, "master": master.id}))
        } else {
            return false;
        };
        if let Err(e) = result {
            self.set_notice(&e.to_string());
        }
        true
    }

    /// egui releases a text field's focus on Escape before drawing; reserve that frame before canvas shortcuts run.
    pub(crate) fn prepare_shape_filter_keys(&mut self, ctx: &egui::Context) {
        self.ui.stencils.filter_keys_held = false;
        if let Some(id) = self.ui.stencils.filter_focus {
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
                ctx.memory_mut(|m| m.surrender_focus(id));
                self.ui.stencils.filter_focus = None;
                self.ui.stencils.filter_keys_held = true;
            } else {
                self.ui.stencils.filter_keys_held = ctx.memory(|m| m.has_focus(id));
            }
        }
    }

    pub(crate) fn shape_filter_keys_held(&self) -> bool {
        self.ui.stencils.filter_keys_held
    }

    pub(crate) fn shape_filter_count(&self) -> usize {
        self.ui
            .stencils
            .list
            .as_ref()
            .and_then(|list| self.ui.stencils.current.as_ref().and_then(|id| list.iter().find(|s| &s.id == id)).or(list.first()))
            .map_or(0, |s| {
                // The search results are not narrowed by name.
                let query = if s.kind == Kind::Search { String::new() } else { self.ui.stencils.filter.to_lowercase() };
                s.masters.iter().filter(|m| m.name.to_lowercase().contains(&query)).count()
            })
    }

    /// What the tile of the master `master` of the stencil `stencil` (`None`: a built-in one) was last drawn from: `"icon"`, `"outline"` or `"box"`;
    /// `None` when no tile was drawn for it (it was out of view). For a test and for `ui.inspect`.
    pub fn tile_art(&self, stencil: Option<&str>, master: &str) -> Option<&'static str> {
        let key = MasterInfo {
            id: master.to_owned(),
            name: String::new(),
            width: 1.0,
            height: 1.0,
            one_d: false,
            autoconnect: true,
            part: false,
            stencil: stencil.map(str::to_owned),
            icon: false,
            home: None,
            ends: None,
        }
        .key();
        if self.ui.stencils.icons.get(&key).is_some_and(Option::is_some) {
            return Some("icon");
        }
        match self.ui.stencils.figures.get(&key) {
            Some(Some(_)) => Some("outline"),
            Some(None) => Some("box"),
            None => None,
        }
    }

    /// How many times a stencil file was read again to draw its tiles: a stencil that is refused room beside a read that runs is read once, and not in every frame.
    pub fn scratch_opens(&self) -> usize {
        self.ui.stencils.scratch_opens
    }

    /// How many figures and icons have been read for tiles: a stencil of hundreds of masters reads only those in view.
    pub fn tiles_read(&self) -> usize {
        self.ui.stencils.figures.len() + self.ui.stencils.icons.len()
    }

    /// What a master's tile shows: its figure in code (a built-in master), its icon (a master of a stencil file that has one), its own outline (read once),
    /// or a box.
    pub(crate) fn art_of(&mut self, ctx: &egui::Context, master: &MasterInfo) -> Art {
        if master.stencil.is_none()
            && let Some(base) = stencils::drawn_in_code(&master.name)
        {
            return Art::Icon(base);
        }
        let key = master.key();
        if master.icon && !self.ui.stencils.icons.contains_key(&key) {
            let texture = self.icon_texture(ctx, master, &key);
            self.ui.stencils.icons.insert(key.clone(), texture);
        }
        if let Some(Some(texture)) = self.ui.stencils.icons.get(&key) {
            return Art::Image(texture.id(), texture.size_vec2().into());
        }
        if !self.ui.stencils.figures.contains_key(&key) {
            match self.figure_of_master(master) {
                Figured::Made(figure) => {
                    self.ui.stencils.figures.insert(key.clone(), Some(Arc::new(figure)));
                }
                Figured::CannotBe => {
                    self.ui.stencils.figures.insert(key.clone(), None);
                }
                // Drawn as a box for this frame, and asked for again in a later one.
                Figured::Later => return Art::Box(master.width, master.height),
            }
        }
        match self.ui.stencils.figures.get(&key) {
            Some(Some(figure)) => Art::Outline(Arc::clone(figure)),
            _ => Art::Box(master.width, master.height),
        }
    }

    /// A master's own outline, for the preview of where it would be dropped: the figure the tile would show if it were drawn from the master's outline (read once, and kept with
    /// the tiles' figures). `None` where there is none, or it cannot be read yet.
    pub(crate) fn outline_of(&mut self, master: &MasterInfo) -> Option<Arc<stencils::Figure>> {
        let key = master.key();
        if !self.ui.stencils.figures.contains_key(&key) {
            match self.figure_of_master(master) {
                Figured::Made(figure) => {
                    self.ui.stencils.figures.insert(key.clone(), Some(Arc::new(figure)));
                }
                Figured::CannotBe => {
                    self.ui.stencils.figures.insert(key.clone(), None);
                }
                Figured::Later => return None,
            }
        }
        self.ui.stencils.figures.get(&key).cloned().flatten()
    }

    /// The texture of a master's icon, if the engine has one and it is of a kind `icon_image` reads.
    fn icon_texture(&self, ctx: &egui::Context, master: &MasterInfo, key: &str) -> Option<egui::TextureHandle> {
        let stencil = self.session.stencils().get(master.stencil.as_deref()?)?;
        let entry = stencil.find(&master.id)?;
        let (format, bytes) = stencil.icon_bytes(entry).ok()?;
        let pixels = icon_image::decode(format.word(), &bytes)?;
        let image = egui::ColorImage::from_rgba_premultiplied([pixels.width, pixels.height], &pixels.premultiplied());
        Some(ctx.load_texture(format!("master-icon-{key}"), image, egui::TextureOptions::LINEAR))
    }

    /// The figure of a master from its own outline: a built-in master on a scratch drawing, a master of a stencil file in the session of its own that has
    /// the file open ([`Figured::CannotBe`] when the window does not know the file, or it cannot be opened again). **A stencil that could not be read because a file is being read**
    /// (the open thread holds the memory a read may use, and the stencil is past the room it is given beside it: `diacraft_engine::memory`) is [`Figured::Later`]: it is not remembered
    /// as one that cannot be read, **and it is not asked for again while a read holds the budget**: the file is loaded and parsed on the window's thread each time it
    /// is asked for, until the room refuses it, and a frame asks for every tile of it. It is asked for once more when no read holds the budget.
    fn figure_of_master(&mut self, master: &MasterInfo) -> Figured {
        let Some(id) = &master.stencil else { return stencils::figure_of(master).map_or(Figured::CannotBe, Figured::Made) };
        let reading = diacraft_engine::memory::reserved() > 0;
        if !reading {
            self.ui.stencils.deferred.clear();
        }
        if !self.ui.stencils.scratch.contains_key(id) {
            if self.ui.stencils.deferred.contains(id) {
                return Figured::Later;
            }
            let path = self.ui.stencils.files.iter().find(|(known, _)| known == id).map(|(_, path)| path.clone());
            self.ui.stencils.scratch_opens = self.ui.stencils.scratch_opens.saturating_add(1);
            let opened = path.and_then(|p| Scratch::open(&p));
            if opened.is_none() && diacraft_engine::memory::reserved() > 0 {
                self.ui.stencils.deferred.insert(id.clone());
                return Figured::Later;
            }
            self.ui.stencils.scratch.insert(id.clone(), opened);
        }
        match self.ui.stencils.scratch.get_mut(id).and_then(Option::as_mut).and_then(|scratch| scratch.figure(master)) {
            Some(figure) => Figured::Made(figure),
            None => Figured::CannotBe,
        }
    }
}

/// What asking for the figure of a master found: the figure, that there is none to be had, or that it cannot be had yet.
enum Figured {
    Made(stencils::Figure),
    CannotBe,
    Later,
}

/// Inspect at most 4,096 input characters: each CR/LF becomes a space, other controls are removed,
/// then retained Unicode characters are capped. Control-only input cannot make this scan unbounded.
/// This is idempotent after egui's own CR/LF-to-space paste preprocessing.
fn normalized_filter(text: &str, remaining: usize) -> String {
    text.chars().take(FILTER_SCAN_CHARS).map(|c| if matches!(c, '\r' | '\n') { ' ' } else { c }).filter(|c| !c.is_control()).take(remaining).collect()
}

/// Normalize before insertion and layout, so control characters never consume the filter's budget.
/// The API uses the same normalizer; other text fields and their input events are untouched.
struct ShapeFilterBuffer<'a>(&'a mut String);

impl egui::TextBuffer for ShapeFilterBuffer<'_> {
    fn type_id(&self) -> std::any::TypeId {
        std::any::TypeId::of::<ShapeFilterBuffer<'static>>()
    }

    fn is_mutable(&self) -> bool {
        true
    }

    fn as_str(&self) -> &str {
        self.0
    }

    fn insert_text(&mut self, text: &str, char_index: egui::text::CharIndex) -> usize {
        let text = normalized_filter(text, FILTER_CHARS.saturating_sub(self.0.chars().count()));
        egui::TextBuffer::insert_text(self.0, &text, char_index)
    }

    fn delete_char_range(&mut self, char_range: std::ops::Range<egui::text::CharIndex>) {
        egui::TextBuffer::delete_char_range(self.0, char_range);
    }
}

/// Fit names to one row while full names remain in tooltips and accessibility labels.
fn single_line(ui: &Ui, rect: Rect, text: &str, font: egui::FontId, colour: egui::Color32) {
    let mut job = egui::text::LayoutJob::simple(text.to_owned(), font, colour, rect.width().max(1.0));
    job.wrap.max_rows = 1;
    job.wrap.overflow_character = Some('…');
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    ui.painter().with_clip_rect(rect.intersect(ui.clip_rect())).galley(pos2(rect.left(), rect.center().y - galley.size().y * 0.5), galley, colour);
}

fn gallery_message(ui: &mut Ui, width: f32, message: &str, t: &Tokens) {
    let galley = ui.painter().layout(message.to_owned(), theme::font(12.0), t.text_dim, (width - PAD * 2.0).max(1.0));
    let (r, response) = ui.allocate_exact_size(vec2(width, galley.size().y), Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, message));
    ui.painter().galley(r.min + vec2(PAD, 0.0), galley, t.text_dim);
}

/// Two chevrons side by side pointing left (to fold the window) or right (to unfold it).
fn paint_double_chevron(ui: &Ui, c: egui::Pos2, left: bool, colour: egui::Color32) {
    let s = 3.2;
    let dir = if left { -1.0 } else { 1.0 };
    for dx in [-3.0, 2.5] {
        let x = c.x + dx;
        let (a, b, z) = (pos2(x - dir * s * 0.5, c.y - s), pos2(x + dir * s * 0.5, c.y), pos2(x - dir * s * 0.5, c.y + s));
        ui.painter().line_segment([a, b], Stroke::new(1.3, colour));
        ui.painter().line_segment([b, z], Stroke::new(1.3, colour));
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    /// Sample the tessellated stroke at a shoulder gap in a compact 34-point preview.
    /// The scene's contour stays exact; only its acute preview joins change.
    fn shoulder_alpha(separate: bool, dark: bool) -> f32 {
        let master = stencils::builtin().into_iter().flat_map(|s| s.masters).find(|m| m.id == "arrows.four-way").unwrap();
        let mut figure = stencils::figure_of(&master).unwrap();
        assert_eq!(figure.areas[0].0.len(), 25);
        assert!(figure.separate_strokes);
        figure.separate_strokes = separate;
        let ctx = egui::Context::default();
        ctx.set_visuals(if dark { egui::Visuals::dark() } else { egui::Visuals::light() });
        let art = Art::Outline(Arc::new(figure));
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let ctx = ui.ctx();
            let painter = ctx.layer_painter(egui::LayerId::background());
            paint_art(&painter, Rect::from_min_size(pos2(0.0, 0.0), vec2(34.0, 34.0)), &art, &Tokens::get(ctx), true);
        });
        output.textures_delta.clear();
        let point = pos2(4.08 + 0.245 * 25.84, 4.08 + 0.245 * 25.84);
        let cross = |a: egui::Vec2, b: egui::Vec2| a.x * b.y - a.y * b.x;
        let mut alpha: f32 = 0.0;
        for primitive in ctx.tessellate(output.shapes, output.pixels_per_point) {
            let egui::epaint::Primitive::Mesh(mesh) = primitive.primitive else { continue };
            for indices in mesh.indices.as_chunks::<3>().0 {
                let (a, b, c) = (mesh.vertices[indices[0] as usize], mesh.vertices[indices[1] as usize], mesh.vertices[indices[2] as usize]);
                let det = cross(b.pos - a.pos, c.pos - a.pos);
                if det.abs() < 1e-6 {
                    continue;
                }
                let v = cross(point - a.pos, c.pos - a.pos) / det;
                let w = cross(b.pos - a.pos, point - a.pos) / det;
                let u = 1.0 - v - w;
                if u >= 0.0 && v >= 0.0 && w >= 0.0 {
                    alpha = alpha.max(u * f32::from(a.color.a()) + v * f32::from(b.color.a()) + w * f32::from(c.color.a()));
                }
            }
        }
        alpha
    }

    #[test]
    fn four_way_preview_keeps_air_between_its_arrowhead_shoulders_in_both_themes() {
        for dark in [false, true] {
            let joined = shoulder_alpha(false, dark);
            let separate = shoulder_alpha(true, dark);
            assert!(joined > 64.0, "joined miters reproduce the cramped gap: {joined}");
            assert!(separate < 8.0, "separate contour edges leave the gap open: {separate}");
        }
    }

    #[test]
    fn the_tile_is_72_points_and_the_default_window_fits_two_of_them() {
        assert_eq!(TILE_SIDE, 72.0);
        assert_eq!(DEFAULT_W, 174.0, "12 each side, two tiles and the gap");
        // The default window's gallery (its width less the margins) is exactly two columns of the full side.
        assert_eq!(tile_grid(DEFAULT_W - PAD * 2.0, 58), (72.0, 2));
        // The figure keeps its share of the card: 43.2 of 72.
        assert!((TILE_SIDE * FIGURE_SHARE - 43.2).abs() < 1.0e-4);
    }

    #[test]
    fn a_wider_gallery_makes_more_columns_of_the_same_side() {
        // The narrowest and widest the window is made: MIN_W 120 and 360, less the margins. A tile of 80 gave 3 columns at 336; 72 gives 4.
        for (usable, columns) in [(96.0, 1), (149.0, 1), (150.0, 2), (227.0, 2), (228.0, 3), (305.0, 3), (306.0, 4), (336.0, 4)] {
            assert_eq!(tile_grid(usable, 58), (72.0, columns), "{usable} points wide");
        }
        // Never more columns than tiles, and at least one.
        assert_eq!(tile_grid(336.0, 3), (72.0, 3));
        assert_eq!(tile_grid(336.0, 0), (72.0, 1));
    }

    #[test]
    fn a_surface_narrower_than_a_tile_makes_one_column_of_a_square_that_fits() {
        assert_eq!(tile_grid(40.0, 5), (40.0, 1));
        assert_eq!(tile_grid(0.5, 5), (0.5, 1));
    }

    fn master(id: &str, name: &str, home: Option<(&str, &str)>) -> MasterInfo {
        MasterInfo {
            id: id.into(),
            name: name.into(),
            width: 1.0,
            height: 1.0,
            one_d: false,
            autoconnect: true,
            part: false,
            stencil: None,
            icon: false,
            home: home.map(|(id, name)| stencils::Home { id: id.into(), name: name.into() }),
            ends: None,
        }
    }

    fn stencil(kind: Kind, masters: Vec<MasterInfo>, quick: usize, customized: bool) -> StencilInfo {
        StencilInfo { id: "s".into(), name: "S".into(), builtin: true, category: None, masters, kind, quick, customized }
    }

    fn names<'a>(section: &Section<'a>) -> Vec<&'a str> {
        section.masters.iter().map(|m| m.name.as_str()).collect()
    }

    #[test]
    fn a_stencil_is_laid_out_as_its_quick_shapes_a_divider_and_the_rest() {
        let s = stencil(Kind::Normal, vec![master("a", "Alpha", None), master("b", "Beta", None), master("c", "Gamma", None)], 2, true);
        let parts = sections(&s, "");
        assert_eq!(parts.len(), 2);
        assert_eq!((names(&parts[0]), parts[0].quick, parts[0].divider, parts[0].hint), (vec!["Alpha", "Beta"], true, false, None));
        assert_eq!((names(&parts[1]), parts[1].quick, parts[1].divider), (vec!["Gamma"], false, true), "the divider stands above the rest");
        // No Quick Shape: no group, no divider.
        let plain = stencil(Kind::Normal, s.masters.clone(), 0, false);
        let none = sections(&plain, "");
        assert_eq!(none.len(), 1);
        assert!(!none[0].divider && !none[0].quick);
        // A group the drawing emptied shows its hint above the divider; the hint is not shown while the filter narrows the names.
        let empty_group = stencil(Kind::Normal, s.masters.clone(), 0, true);
        let emptied = sections(&empty_group, "");
        assert_eq!((emptied[0].hint, emptied[0].masters.len(), emptied[1].divider), (Some("Drop Quick Shapes here"), 0, true));
        assert_eq!(sections(&empty_group, "gam")[0].hint, None);
        // Every master a group holds: no rest, no divider.
        let full = stencil(Kind::Normal, s.masters.clone(), 3, true);
        assert_eq!(sections(&full, "").len(), 1);
        // The filter narrows each part, and drops a part that has nothing left.
        let filtered = sections(&s, "gam");
        assert_eq!(filtered.len(), 1);
        assert_eq!((names(&filtered[0]), filtered[0].divider), (vec!["Gamma"], false), "a divider between nothing and the rest is not drawn");
        // A group longer than the list is a group of the list.
        let long = stencil(Kind::Normal, s.masters.clone(), 99, true);
        assert_eq!(sections(&long, "").len(), 1);
    }

    #[test]
    fn the_gathering_stencils_are_a_run_for_each_stencil_headed_by_its_name() {
        let masters = vec![
            master("a", "Alpha", Some(("x", "Stencil X"))),
            master("b", "Beta", Some(("x", "Stencil X"))),
            master("c", "Gamma", Some(("y", "Stencil Y"))),
            master("d", "Delta", Some(("x", "Stencil X"))),
        ];
        let found = stencil(Kind::Search, masters.clone(), 0, false);
        let parts = sections(&found, "");
        let heads: Vec<Option<&str>> = parts.iter().map(|p| p.heading).collect();
        assert_eq!(heads, [Some("Stencil X"), Some("Stencil Y"), Some("Stencil X")], "a run for each stretch of one stencil, in order");
        // The Quick Shapes stencil names the groups when there are several stencils, and not when there is one.
        let quick = stencil(Kind::Quick, masters, 0, false);
        assert!(sections(&quick, "").iter().all(|p| p.heading.is_some()));
        let narrowed = sections(&quick, "gam");
        assert_eq!((narrowed.len(), narrowed[0].heading), (1, Some("Stencil Y")), "the filter leaves the group that has a match, still named");
        let one = stencil(Kind::Quick, vec![master("a", "Alpha", Some(("x", "Stencil X")))], 0, false);
        assert_eq!(sections(&one, "")[0].heading, None);
        // No Quick Shapes at all: the hint.
        let empty = stencil(Kind::Quick, vec![], 0, false);
        let none = sections(&empty, "");
        assert_eq!((none.len(), none[0].hint), (1, Some("Drop Quick Shapes here")));
        // The results of a search with nothing found are no run.
        let nothing = stencil(Kind::Search, vec![], 0, false);
        assert!(sections(&nothing, "").is_empty());
    }
}
