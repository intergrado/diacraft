//! The picture of a sheet in the Print view: what is asked of the render worker, and what is shown meanwhile.
//!
//! The preview shows **a sheet of the print as the printer would be given it**: the same plan (`diacraft_render::print`) and the same walk as the
//! PDF, drawn as pixels (`Renderer::render_sheet`), in the colours chosen: black and white draws it in greys, and choosing it asks for the sheet
//! again at once (the colours are part of what a picture is of). The window asks for the sheet it shows at a resolution that serves its size on the screen, and a
//! worker thread draws it: **a frame never waits for a sheet**. The sheet that was last drawn stays on the screen, scaled to where the sheet is now,
//! until the next arrives; one of another sheet is not shown in its place.
//!
//! The worker is the page pictures' own kind (`render_worker.rs`): it keeps the newest request and stops one in progress that a newer makes
//! pointless (a sheet turned over, a zoom). Where a thread cannot be had, and in a test, the same drawer runs inline.

use std::sync::Arc;

use diacraft_engine::doc::Document;
use diacraft_render::print::{PrintColor, Sheet};
use diacraft_render::{Limits, SkipKind, View};
use egui::{Context, TextureHandle, TextureOptions};
use kurbo::Affine;

use crate::render_worker::{Done, Job, MakeDrawer, RenderWorker, bundled_drawer, system_drawer};

/// The resolutions a sheet is drawn at, in pixels to the inch. A sheet is drawn at the least of them that serves its size on the screen, so that a
/// small change of the window's size or of the zoom asks for nothing.
const RESOLUTIONS: [f64; 9] = [24.0, 36.0, 48.0, 72.0, 96.0, 144.0, 192.0, 288.0, 384.0];

/// The longest side of a sheet's picture, in pixels, whatever the screen asks: a sheet of 44 inches at 384 would be 17,000.
pub const MAX_SIDE_PX: f64 = 4096.0;

/// What a picture is of: the document, the revision, the sheet of the print, the resolution and the colours.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Key {
    generation: u64,
    revision: u64,
    pub sheet: usize,
    /// Pixels to the inch, a whole number from [`RESOLUTIONS`].
    ppi: u32,
    color: PrintColor,
}

/// A sheet's picture on the screen.
pub(crate) struct Shown {
    pub texture: TextureHandle,
    pub key: Key,
}

enum Mode {
    Thread(Option<MakeDrawer>),
    Inline(Option<MakeDrawer>),
}

/// The preview's worker and what it has drawn.
pub(crate) struct Preview {
    mode: Mode,
    worker: Option<RenderWorker>,
    serial: u64,
    /// The document as the worker was last given it, with the generation and revision it is of.
    doc: Option<(u64, u64, Arc<Document>)>,
    /// The request on its way: its serial, and what it is of.
    pending: Option<(u64, Key)>,
    shown: Option<Shown>,
    /// Why the last sheet has no picture: drawing it failed.
    lost: Option<String>,
    /// How many sheets have been asked for, and how many have arrived.
    pub requested: u64,
    pub received: u64,
}

impl Preview {
    /// Sheets drawn on a thread of their own, over the machine's faces and the bundled ones.
    pub(crate) fn threaded() -> Preview {
        Preview::with_mode(Mode::Thread(Some(system_drawer())))
    }

    /// Sheets drawn on a thread of their own by `make`'s drawer: over the database the application shares (`DiacraftApp::with_fonts`).
    pub(crate) fn threaded_with(make: MakeDrawer) -> Preview {
        Preview::with_mode(Mode::Thread(Some(make)))
    }

    /// Sheets drawn on the calling thread, at once, over the bundled faces: for a test, which then sees a picture in the frame that asked.
    pub(crate) fn inline_bundled() -> Preview {
        Preview::with_mode(Mode::Inline(Some(bundled_drawer())))
    }

    fn with_mode(mode: Mode) -> Preview {
        Preview { mode, worker: None, serial: 0, doc: None, pending: None, shown: None, lost: None, requested: 0, received: 0 }
    }

    /// Whether a sheet is being drawn.
    pub(crate) fn busy(&self) -> bool {
        self.pending.is_some()
    }

    /// Why the sheet has no picture, in words, if drawing it failed.
    pub(crate) fn lost(&self) -> Option<&str> {
        self.lost.as_deref()
    }

    /// The picture last drawn, if it is of `sheet` of this document: whatever its revision and resolution.
    pub(crate) fn shown_for(&self, generation: u64, sheet: usize) -> Option<&Shown> {
        self.shown.as_ref().filter(|s| s.key.generation == generation && s.key.sheet == sheet)
    }

    /// Forgets what was drawn: another document is open.
    pub(crate) fn reset(&mut self) {
        self.doc = None;
        self.pending = None;
        self.shown = None;
        self.lost = None;
        if let Some(worker) = &mut self.worker {
            worker.clear_caches();
        }
    }

    /// Asks for `sheet` (the `number`th of the print) at `px_per_inch` in `color` if it is not the picture shown or already on its way.
    pub(crate) fn want(
        &mut self,
        ctx: &Context,
        doc: &Document,
        (generation, revision): (u64, u64),
        number: usize,
        sheet: &Sheet,
        (px_per_inch, color): (f64, PrintColor),
    ) {
        let key = Key { generation, revision, sheet: number, ppi: px_per_inch.round().max(1.0) as u32, color };
        if self.shown.as_ref().is_some_and(|s| s.key == key) || self.pending.is_some_and(|(_, k)| k == key) {
            return;
        }
        if self.worker.is_none() {
            self.worker = self.start(ctx);
        }
        let shared = match &self.doc {
            Some((g, r, shared)) if *g == generation && *r == revision => Arc::clone(shared),
            _ => {
                let shared = Arc::new(doc.clone());
                self.doc = Some((generation, revision, Arc::clone(&shared)));
                shared
            }
        };
        let Some(worker) = self.worker.as_mut() else { return };
        self.serial += 1;
        let serial = self.serial;
        let (width, height) = ((sheet.width * px_per_inch).ceil().max(1.0), (sheet.height * px_per_inch).ceil().max(1.0));
        let view = View { to_px: Affine::IDENTITY, width: width as u32, height: height as u32 };
        self.pending = Some((serial, key));
        self.requested += 1;
        worker.submit(Job {
            serial,
            doc: shared,
            page: sheet.page,
            view,
            limits: Limits::default(),
            supersedes: true,
            cancel: None,
            sheet: Some(Box::new((sheet.clone(), px_per_inch, color))),
        });
    }

    fn start(&mut self, ctx: &Context) -> Option<RenderWorker> {
        match &mut self.mode {
            Mode::Thread(make) => {
                let make = make.take()?;
                RenderWorker::spawn(ctx.clone(), make).or_else(|| Some(RenderWorker::inline(bundled_drawer())))
            }
            Mode::Inline(make) => Some(RenderWorker::inline(make.take()?)),
        }
    }

    /// Takes in the pictures that have arrived.
    pub(crate) fn poll(&mut self, ctx: &Context) {
        let done = self.worker.as_mut().map(RenderWorker::poll).unwrap_or_default();
        for answer in done {
            self.accept(ctx, answer);
        }
    }

    fn accept(&mut self, ctx: &Context, done: Done) {
        // Serial 0 is the worker's own: it could not start with the machine's faces. It draws on with the bundled ones.
        if done.serial == 0 {
            return;
        }
        let Some((serial, key)) = self.pending else { return };
        if done.serial != serial {
            return;
        }
        self.pending = None;
        let rendered = match done.result {
            Ok(rendered) => rendered,
            Err(_) => {
                self.lost = Some(LOST_SHEET.to_owned());
                return;
            }
        };
        // A sheet that a newer request stopped has no picture, and says nothing.
        if rendered.count(SkipKind::Cancelled) > 0 {
            return;
        }
        if rendered.count(SkipKind::Recovered) > 0 {
            self.lost = Some(LOST_SHEET.to_owned());
            return;
        }
        self.received += 1;
        let Some(image) = done.image else { return };
        self.lost = None;
        let texture = ctx.load_texture(format!("print-sheet-{serial}"), image, TextureOptions::LINEAR);
        self.shown = Some(Shown { texture, key });
        ctx.request_repaint();
    }
}

/// What the preview says when a sheet could not be drawn.
pub const LOST_SHEET: &str =
    "This sheet could not be drawn. DiaCraft met an internal problem while drawing it. Your drawing is not changed. Please report this bug.";

/// The resolution to draw a sheet at, in pixels to the inch: it shows `inches_wide` across `points` screen points of `pixels_per_point` pixels each,
/// and the least resolution of [`RESOLUTIONS`] that serves that is chosen, held to [`MAX_SIDE_PX`] on the sheet's longer side (and to `max_side`, what
/// the graphics back end takes of a texture).
pub(crate) fn resolution(points: f64, pixels_per_point: f64, sheet: (f64, f64), max_side: f64) -> f64 {
    let longer = sheet.0.max(sheet.1).max(1.0e-6);
    let across = sheet.0.max(1.0e-6);
    let wanted = if points.is_finite() && pixels_per_point.is_finite() && points > 0.0 && pixels_per_point > 0.0 {
        points * pixels_per_point / across
    } else {
        RESOLUTIONS[0]
    };
    let cap = (MAX_SIDE_PX.min(if max_side.is_finite() && max_side > 16.0 { max_side } else { MAX_SIDE_PX }) / longer).max(RESOLUTIONS[0]);
    let chosen = RESOLUTIONS.iter().copied().find(|r| *r >= wanted).unwrap_or(RESOLUTIONS[RESOLUTIONS.len() - 1]);
    // The least resolution is the floor even for a sheet too large for it: a smaller picture is not asked for.
    chosen.min(RESOLUTIONS.iter().copied().filter(|r| *r <= cap).fold(RESOLUTIONS[0], f64::max))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sheet_is_drawn_at_the_least_resolution_that_serves_its_size_on_the_screen() {
        // Letter sideways, 11 inches across: 528 points at 1 pixel to the point is 48 to the inch; 600 points needs 72.
        assert_eq!(resolution(528.0, 1.0, (11.0, 8.5), 8192.0), 48.0);
        assert_eq!(resolution(600.0, 1.0, (11.0, 8.5), 8192.0), 72.0);
        assert_eq!(resolution(600.0, 2.0, (11.0, 8.5), 8192.0), 144.0, "a screen of twice the pixels");
        assert_eq!(resolution(1.0, 1.0, (11.0, 8.5), 8192.0), 24.0, "never below the least");
    }

    #[test]
    fn a_resolution_is_held_to_the_longest_side_a_picture_may_have() {
        // A 44 inch sheet at 4096 pixels is 93 to the inch: 72 is the most of the list that does it.
        assert_eq!(resolution(100_000.0, 2.0, (34.0, 44.0), 8192.0), 72.0);
        // A graphics back end that takes 2048 pixels a side: 46 to the inch for a 44 inch sheet, and 36 is the most of the list that does it.
        assert_eq!(resolution(100_000.0, 2.0, (34.0, 44.0), 2048.0), 36.0);
        assert_eq!(resolution(f64::NAN, 1.0, (11.0, 8.5), 8192.0), 24.0, "numbers that are not numbers ask for the least");
        assert_eq!(resolution(500.0, 1.0, (0.0, 0.0), f64::NAN), 384.0, "a sheet with no size is drawn at what the screen asks");
    }
}
