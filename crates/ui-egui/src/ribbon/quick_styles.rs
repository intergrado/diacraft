//! Home, Shape Styles, Quick Styles: the gallery.
//!
//! *"Menu as pictured: a heading Variant Styles with four tiles; a heading Theme Styles with a grid of tiles in seven columns and six rows (42), each tile showing "Abc" on a
//! small rounded shape in a different line and fill."* The tiles are **the shape selected, put in each style in a copy of the drawing** (`shape.quick_styles`), drawn here from
//! the fill, the line, the gradient and the shadow the theme gives it, so that a tile shows what a press makes of the shape (and a program reads the same numbers). The tiles
//! are made once for each revision, page and shape (`DiacraftApp::refresh_quick_view`). The tile the first shape has is marked.
//!
//! **No live preview.** Pointing at a tile names it in a tooltip and nothing more: a preview on the page would be an edit and an undo step (as the Themes gallery's,
//! `design.rs`). **A page with no theme has no styles**: the control is quiet and its tooltip says to apply a theme (`Design`, `Themes`), and the menu says so too.

use diacraft_engine::formatting::{GradientLook, QuickTile, Rgb};
use egui::{CornerRadius, Mesh, Painter, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use serde_json::json;

use super::controls::{Cx, chosen};
use crate::app::DiacraftApp;
use crate::theme::{self, Tokens, size};
use crate::widgets;

/// The tiles in a row of the Theme Styles: the seven colours of the page's colour variant.
pub const COLUMNS: usize = 7;

/// A tile's size in the menu and the gap between two, in points.
const TILE: egui::Vec2 = egui::vec2(36.0, 28.0);
const GAP: f32 = 4.0;

/// The width the menu has for its tiles, in points.
pub(super) const WIDTH: f32 = COLUMNS as f32 * TILE.x + (COLUMNS as f32 - 1.0) * GAP + 16.0;

/// The inside of the Quick Styles menu: the Variant Styles and the Theme Styles.
pub(super) fn gallery_menu(app: &mut DiacraftApp, ui: &mut Ui, cx: Cx) {
    app.refresh_quick_view();
    let enabled = app.command_enabled("format.quickStyle").is_ok();
    let Some(look) = app.ui.quick.look.clone() else {
        let words = app.ui.quick.why.clone().or_else(|| app.quick_refusal()).unwrap_or_else(|| "Select a shape to give it a style.".to_owned());
        ui.add(egui::Label::new(egui::RichText::new(words).font(theme::font(size::LABEL)).color(cx.t.text_dim)).wrap());
        return;
    };
    let current = look.current;
    heading(ui, &cx.t, "Variant Styles");
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = GAP;
        for (n, tile) in look.variants.iter().enumerate() {
            let element = format!("menu.quickstyle.variant.{n}");
            let label = format!("Variant Style {}", n + 1);
            if draw_tile(app, ui, &cx.t, tile, &element, &label, current == tile.style, enabled) {
                press(app, ui, json!({"variant": n}));
            }
        }
    });
    ui.add_space(6.0);
    heading(ui, &cx.t, "Theme Styles");
    for (row, tiles) in look.styles.chunks(COLUMNS).enumerate() {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            for (column, tile) in tiles.iter().enumerate() {
                let element = format!("menu.quickstyle.{}.{}", row + 1, column + 1);
                let label = format!("Theme Style, row {} column {}", row + 1, column + 1);
                if draw_tile(app, ui, &cx.t, tile, &element, &label, current == tile.style, enabled) {
                    press(app, ui, json!({"row": row + 1, "column": column + 1}));
                }
            }
        });
        ui.add_space(GAP / 2.0);
    }
}

/// Sends the Quick Style and closes the menu.
fn press(app: &mut DiacraftApp, ui: &mut Ui, params: serde_json::Value) {
    app.run_ribbon_params(ui.ctx(), "format.quickStyle", &params);
    chosen(app, ui);
}

fn heading(ui: &mut Ui, t: &Tokens, words: &str) {
    ui.label(egui::RichText::new(words).font(theme::font(size::LABEL)).color(t.text_dim));
}

/// Draws a tile and senses a press on it. → whether it was pressed.
#[allow(clippy::too_many_arguments)]
fn draw_tile(app: &mut DiacraftApp, ui: &mut Ui, t: &Tokens, tile: &QuickTile, element: &str, label: &str, marked: bool, enabled: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(TILE, if enabled { Sense::click() } else { Sense::hover() });
    widgets::name_toggle(&resp, enabled, marked, label);
    paint_tile(ui.painter(), rect, tile, t, enabled);
    let painter = ui.painter();
    if marked {
        painter.rect_stroke(rect, CornerRadius::same(3), Stroke::new(2.0, t.accent), StrokeKind::Inside);
    } else if resp.hovered() && enabled {
        painter.rect_stroke(rect, CornerRadius::same(3), Stroke::new(1.5, t.accent_hover), StrokeKind::Inside);
    } else {
        painter.rect_stroke(rect, CornerRadius::same(3), Stroke::new(1.0, t.input_border), StrokeKind::Inside);
    }
    app.note(element.to_owned(), rect, enabled);
    resp.clone().on_hover_text(label.to_owned());
    resp.clicked() && enabled
}

/// A tile: "Abc" on a small rounded shape in the fill, line and shadow of the style, on the menu's own ground. The colours are the drawing's (the theme's), and the
/// ground is a token.
fn paint_tile(painter: &Painter, rect: Rect, tile: &QuickTile, t: &Tokens, enabled: bool) {
    painter.rect_filled(rect, CornerRadius::same(3), t.input_fill);
    let body = rect.shrink2(vec2(5.0, 5.0));
    if !enabled {
        painter.rect_stroke(body, CornerRadius::same(4), Stroke::new(1.0, t.icon_ink_disabled), StrokeKind::Inside);
        return;
    }
    if let Some(shadow) = &tile.fx.shadow {
        // A shadow a few points to the lower right, as far as the tile has room for.
        let reach = (shadow.distance as f32).clamp(1.0, 3.0);
        let away = body.translate(vec2(reach, reach));
        painter.rect_filled(
            away,
            CornerRadius::same(4),
            theme::document_colour_alpha(shadow.colour.0, shadow.colour.1, shadow.colour.2, crate::format_fx::alpha_of(shadow.transparency)),
        );
    }
    let round = CornerRadius::same(4);
    match (&tile.fx.gradient, tile.fill) {
        (Some(g), _) => paint_gradient(painter, body, g),
        (None, Some(Rgb(r, g, b))) => {
            painter.rect_filled(body, round, theme::document_colour_alpha(r, g, b, crate::format_fx::alpha_of(tile.fx.fill_transparency)));
        }
        (None, None) => {}
    }
    if let Some(Rgb(r, g, b)) = tile.line {
        let width = (tile.weight as f32).clamp(1.0, 3.0);
        painter.rect_stroke(body, round, Stroke::new(width, theme::document_colour(r, g, b)), StrokeKind::Inside);
    }
    // The text: black or white by the fill it is on.
    let ground = tile.fill.or_else(|| tile.fx.gradient.as_ref().and_then(|g| g.stops.first().map(|s| s.colour)));
    let dark = ground.is_some_and(|Rgb(r, g, b)| 0.299 * f32::from(r) + 0.587 * f32::from(g) + 0.114 * f32::from(b) < 140.0);
    let ink = match ground {
        None => t.text,
        Some(_) if dark => theme::document_colour(255, 255, 255),
        Some(_) => theme::document_colour(0, 0, 0),
    };
    painter.text(body.center(), egui::Align2::CENTER_CENTER, "Abc", theme::font(10.0), ink);
}

/// A gradient in a tile: along its angle for a linear one, from the middle out for the others.
fn paint_gradient(painter: &Painter, rect: Rect, g: &GradientLook) {
    let steps = 24_usize;
    let mut mesh = Mesh::default();
    let colour = |at: f64| {
        let (r, gc, b, a) = crate::format_fx::gradient_colour_at(g, at);
        theme::document_colour_alpha(r, gc, b, a)
    };
    let rad = g.angle.to_radians();
    let vertical = g.dir == 0 && rad.sin().abs() > rad.cos().abs();
    let reversed = g.dir == 0 && if vertical { rad.sin() < 0.0 } else { rad.cos() < 0.0 };
    if g.dir == 0 {
        for i in 0..=steps {
            let at = i as f64 / steps as f64;
            let c = colour(if reversed { 1.0 - at } else { at });
            let k = at as f32;
            let (a, b) = if vertical {
                (pos2(rect.left(), rect.top() + rect.height() * k), pos2(rect.right(), rect.top() + rect.height() * k))
            } else {
                (pos2(rect.left() + rect.width() * k, rect.top()), pos2(rect.left() + rect.width() * k, rect.bottom()))
            };
            mesh.colored_vertex(a, c);
            mesh.colored_vertex(b, c);
            if i > 0 {
                let n = (i * 2) as u32;
                mesh.add_triangle(n - 2, n - 1, n);
                mesh.add_triangle(n - 1, n, n + 1);
            }
        }
        painter.add(egui::Shape::mesh(mesh));
    } else {
        // From the middle out in rounded rectangles, the first stop in the middle and the last at the edge.
        for i in 0..steps {
            let f = i as f32 / steps as f32;
            let inner = rect.shrink2(vec2(f * rect.width() / 2.0, f * rect.height() / 2.0));
            if inner.width() > 0.0 && inner.height() > 0.0 {
                painter.rect_filled(inner, CornerRadius::same(3), colour(1.0 - f64::from(f)));
            }
        }
    }
}
