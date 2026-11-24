//! The Change Shape gallery: the menu of Home > Editing > Change Shape.
//!
//! A gallery of related shapes, with the stencils to switch between to see more of them. The tiles are the masters of every
//! stencil that is open (the Shapes window's own list, `stencil_list`), a heading for each stencil and the masters of the stencil under it, drawn as the Shapes window
//! draws them. **Only the masters that fit the selection are listed**: a connector is changed only to a connector, and any other shape only to a master that is not one
//! (the engine's), so the gallery never offers what the engine would refuse. A press on a tile sends `arrange.changeShape` (the window's command,
//! `shape.change` of the selection) and closes the menu.

use egui::{Align2, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use serde_json::json;

use super::controls::{Cx, chosen};
use crate::app::DiacraftApp;
use crate::shapes_window::paint_art;
use crate::stencils::MasterInfo;
use crate::theme::{self, size};
use crate::widgets::{self, label_colour};

/// The side of a tile, in points.
const TILE: f32 = 56.0;
const GAP: f32 = 4.0;
/// How many tiles a row holds.
const COLUMNS: usize = 5;
const PAD: f32 = 6.0;
/// The height of the row that names a stencil.
const HEADING_H: f32 = 22.0;
/// The most the gallery is tall, in points, before it scrolls.
const MAX_HEIGHT: f32 = 420.0;

/// The width of the gallery: its tiles and a little air round them.
pub(crate) const GALLERY_WIDTH: f32 = COLUMNS as f32 * (TILE + GAP) - GAP + PAD * 2.0;

/// Whether a master is one the shapes selected can be changed to: a one-dimensional master for a connector, any other for any other shape.
fn fits(master: &MasterInfo, connectors: bool, others: bool) -> bool {
    if master.one_d { connectors } else { others }
}

/// The gallery, drawn in the menu of the Change Shape control.
pub(crate) fn gallery_menu(app: &mut DiacraftApp, ui: &mut Ui, cx: Cx) {
    let (connectors, others) = app.selection_kinds();
    let enabled = app.command_enabled("arrange.changeShape").is_ok();
    let list = app.stencil_list();
    let font = theme::font(size::LABEL);
    let height = MAX_HEIGHT.min((ui.ctx().content_rect().height() - 120.0).max(160.0));
    egui::ScrollArea::vertical().max_height(height).auto_shrink([true, true]).show(ui, |ui| {
        ui.set_width(GALLERY_WIDTH);
        let mut shown = 0_usize;
        // The stencils the window gathers from the others (Quick Shapes, the search results) hold masters the stencils list already.
        for stencil in list.iter().filter(|s| !s.is_gathered()) {
            let masters: Vec<&MasterInfo> = stencil.masters.iter().filter(|m| fits(m, connectors, others)).collect();
            if masters.is_empty() {
                continue;
            }
            shown += masters.len();
            let (heading, _) = ui.allocate_exact_size(vec2(GALLERY_WIDTH, HEADING_H), Sense::hover());
            ui.painter().text(heading.left_center() + vec2(PAD, 0.0), Align2::LEFT_CENTER, &stencil.name, font.clone(), cx.t.text_dim);
            let rows = masters.len().div_ceil(COLUMNS);
            let (area, _) = ui.allocate_exact_size(vec2(GALLERY_WIDTH, rows as f32 * (TILE + GAP) - GAP + PAD), Sense::hover());
            for (i, master) in masters.iter().enumerate() {
                let at = pos2(area.left() + PAD + (i % COLUMNS) as f32 * (TILE + GAP), area.top() + (i / COLUMNS) as f32 * (TILE + GAP));
                tile(app, ui, cx, Rect::from_min_size(at, vec2(TILE, TILE)), master, enabled);
            }
        }
        if shown == 0 {
            let (rect, _) = ui.allocate_exact_size(vec2(GALLERY_WIDTH, 40.0), Sense::hover());
            let why = if connectors && !others { "No connector master is open" } else { "No master is open to change to" };
            ui.painter().text(rect.center(), Align2::CENTER_CENTER, why, font, label_colour(&cx.t, false));
        }
    });
}

/// One tile: the master's figure, its name on hover, and a press that changes the selection to it.
fn tile(app: &mut DiacraftApp, ui: &mut Ui, cx: Cx, rect: Rect, master: &MasterInfo, enabled: bool) {
    let art = if rect.intersects(ui.clip_rect()) { app.art_of(ui.ctx(), master) } else { crate::stencils::Art::Box(master.width, master.height) };
    let resp = ui.interact(rect, ui.scope_id().with(("change_shape_tile", master.key())), if enabled { Sense::click() } else { Sense::hover() });
    widgets::name_button(&resp, enabled, &master.name);
    let fill = if enabled && resp.is_pointer_button_down_on() {
        cx.t.pressed
    } else if enabled && resp.hovered() {
        cx.t.hover
    } else {
        cx.t.panel_header_fill
    };
    let painter = ui.painter();
    painter.rect_filled(rect, 6.0, fill);
    if enabled && resp.has_focus() {
        painter.rect_stroke(rect, 6.0, Stroke::new(1.5, cx.t.accent), StrokeKind::Inside);
    }
    let side = 44.0_f32.min(rect.width());
    paint_art(painter, Rect::from_center_size(rect.center(), vec2(side, side)), &art, &cx.t, enabled);
    app.note(format!("ribbon.{}.changeShape.{}", cx.tab, master.key()), rect, enabled);
    let name = master.name.clone();
    let clicked = resp.clicked();
    resp.on_hover_text(name);
    if clicked && enabled {
        app.run_ribbon_params(ui.ctx(), "arrange.changeShape", &json!({"master": master.id, "stencil": master.stencil}));
        chosen(app, ui);
    }
}
