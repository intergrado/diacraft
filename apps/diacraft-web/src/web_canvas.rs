//! The browser's side of the canvas's size: before every frame, the canvas's pixels are held to the size of the page times the device pixel ratio. It exists on `wasm32` only, and is one of
//! the places that touch the DOM; [`crate::canvas_size`] holds what it decides without a browser, and why (a browser that gives the web runner its canvas size in CSS pixels where the ratio is more
//! than 1 gives a window laid out for 1 / ratio of the page: Chrome's device emulation does).
//!
//! **It is a plugin of the window's context (`Plugin::input_hook`), which runs after the runner has made the frame's input and before the window uses it**, so a canvas that is wrong is put right
//! on the frame that would have been drawn wrong: its pixels are set, and so is the size in points the frame is laid out for (the runner's own formula: the canvas's pixels over the zoom factor
//! and the ratio), and the runner paints with the canvas as it is then. A canvas that is right, which is every canvas of a browser that reports its box right, is not touched: setting a canvas's size
//! clears it, so the check reads and writes nothing in that case. Nothing here may panic (WebAssembly aborts on a panic): every browser call that can fail is matched.

use egui::{Context, Plugin, Pos2, RawInput, Rect, vec2};

use crate::canvas_size;

/// Puts the guard on the window's context. Called once, when the runner makes the window.
pub fn install(ctx: &Context) {
    ctx.add_plugin(CanvasGuard::default());
}

/// Holds the canvas's pixels to the page's size times the device pixel ratio.
#[derive(Default)]
struct CanvasGuard {
    /// The console has been told that the canvas was wrong: once is enough, a page that needs it needs it on every resize.
    told: bool,
}

impl Plugin for CanvasGuard {
    fn debug_name(&self) -> &'static str {
        "diacraft-web canvas size"
    }

    fn input_hook(&mut self, ctx: &Context, input: &mut RawInput) {
        let Some(canvas) = crate::web::canvas() else { return };
        let Some(window) = web_sys::window() else { return };
        let ratio = window.device_pixel_ratio();
        let rect = canvas.get_bounding_client_rect();
        let have = (canvas.width(), canvas.height());
        let Some((width, height)) = canvas_size::correction(have, (rect.width(), rect.height()), ratio) else { return };
        if !self.told {
            self.told = true;
            log::warn!(
                "diacraft-web: the canvas had {}x{} pixels, and the page is {:.1}x{:.1} CSS pixels at {ratio} device pixels each: set to {width}x{height} (the browser gave its size in CSS pixels)",
                have.0,
                have.1,
                rect.width(),
                rect.height()
            );
        }
        canvas.set_width(width);
        canvas.set_height(height);
        // The size of the window in points, as the runner makes it: the canvas's pixels over the zoom factor and the ratio. A zoom factor or a ratio that is no number leaves the runner's own.
        let per_point = ctx.zoom_factor() * ratio as f32;
        if per_point.is_finite() && per_point > 0.0 {
            input.screen_rect = Some(Rect::from_min_size(Pos2::ZERO, vec2(width as f32 / per_point, height as f32 / per_point)));
        }
    }
}
