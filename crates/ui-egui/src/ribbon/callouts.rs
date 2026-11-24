//! Insert, Diagram Parts, Callout: the gallery of callout tiles.
//!
//! *"Gallery: 14 tiles in rows of four, four, four and two ... A hover previews; a click inserts."* The menu holds the engine's tiles (`callout.styles`) as pictures **of the callouts themselves**, drawn
//! by the renderer from the callout the command makes (`diacraft_engine::draw::callout_preview`), so a tile cannot show a figure the command does not make. The pictures are made the first time the
//! menu opens and kept, as textures; they are on white paper in either theme, as a page is. A press on a tile sends the window command `window.callout {style}` (`insert_callout.rs`), which adds a
//! callout tied to the shape selected, or one at the middle of the view when none is, and closes the menu.
//!
//! **No live preview.** Pointing at a tile names the callout in a tooltip and nothing more: a preview on the page would be an edit and an undo step (as the Themes gallery's, `design.rs`).

use diacraft_engine::draw::callout_preview::preview;
use diacraft_engine::stencil::CALLOUT_STYLES;
use egui::{ColorImage, CornerRadius, Sense, Stroke, StrokeKind, TextureHandle, TextureOptions, Ui};
use serde_json::json;

use super::controls::{Cx, chosen};
use crate::app::DiacraftApp;
use crate::widgets;

/// The tiles in a row of the menu: four, four, four and two for the fourteen.
pub const COLUMNS: usize = 4;

/// A tile's size in the menu, in points: the picture is 120 by 70 pixels, shown at this size.
const TILE: egui::Vec2 = egui::vec2(66.0, 40.0);

/// The width the menu has for its tiles, in points.
pub(super) const WIDTH: f32 = COLUMNS as f32 * TILE.x + (COLUMNS as f32 - 1.0) * 5.0 + 16.0;

/// The pixels to an inch the pictures are drawn at: the page of a picture is 2.4 by 1.4 inches, a whole number of pixels at any multiple of 5.
const PICTURE_PPI: f64 = 50.0;

/// A tile: the id the command takes, its name, and its picture once made.
pub struct CalloutTile {
    pub id: &'static str,
    pub name: &'static str,
    pub texture: Option<TextureHandle>,
}

/// What the window keeps of the gallery: the tiles, made once.
#[derive(Default)]
pub struct CalloutsState {
    pub tiles: Vec<CalloutTile>,
    /// The pictures were asked for: a tile that could not be drawn stays without one, and the menu does not try again.
    made: bool,
}

impl CalloutsState {
    /// Makes the pictures the first time, as textures of `ctx`.
    pub fn ensure(&mut self, ctx: &egui::Context) {
        if self.made {
            return;
        }
        self.made = true;
        self.tiles = CALLOUT_STYLES
            .iter()
            .map(|style| {
                let texture = preview(style, PICTURE_PPI).ok().map(|picture| {
                    // The paper is opaque, so the straight bytes are the premultiplied ones.
                    let image = ColorImage::from_rgba_premultiplied([picture.width as usize, picture.height as usize], &picture.rgba);
                    ctx.load_texture(format!("callout-tile-{}", style.id), image, TextureOptions::LINEAR)
                });
                CalloutTile { id: style.id, name: style.name, texture }
            })
            .collect();
    }
}

/// The inside of the Callout menu: the tiles of the gallery in rows of four.
pub(super) fn gallery_menu(app: &mut DiacraftApp, ui: &mut Ui, cx: Cx) {
    app.ui.callouts.ensure(ui.ctx());
    let enabled = app.command_enabled("callout.insert").is_ok();
    let shown: Vec<(&'static str, &'static str, Option<egui::TextureId>)> =
        app.ui.callouts.tiles.iter().map(|t| (t.id, t.name, t.texture.as_ref().map(TextureHandle::id))).collect();
    for row in shown.chunks(COLUMNS) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 5.0;
            for (id, name, texture) in row {
                let (rect, resp) = ui.allocate_exact_size(TILE, if enabled { Sense::click() } else { Sense::hover() });
                let label = (*name).to_owned();
                widgets::name_button(&resp, enabled, &label);
                let t = &cx.t;
                ui.painter().rect_filled(rect, CornerRadius::same(3), t.input_fill);
                // A tile that cannot be used is the bare tile: its picture is not drawn.
                if let Some(texture) = texture.filter(|_| enabled) {
                    let inside = rect.shrink(1.0);
                    egui::Image::new(egui::load::SizedTexture::new(texture, inside.size())).paint_at(ui, inside);
                }
                let painter = ui.painter();
                let ring = if resp.hovered() && enabled { Stroke::new(1.5, t.accent_hover) } else { Stroke::new(1.0, t.input_border) };
                painter.rect_stroke(rect, CornerRadius::same(3), ring, StrokeKind::Inside);
                resp.clone().on_hover_text(if enabled { label.clone() } else { format!("{label} (no document open)") });
                app.note(format!("menu.callout.{id}"), rect, enabled);
                if resp.clicked() && enabled {
                    app.run_ribbon_params(ui.ctx(), "window.callout", &json!({"style": id}));
                    chosen(app, ui);
                }
            }
        });
        ui.add_space(5.0);
    }
}
