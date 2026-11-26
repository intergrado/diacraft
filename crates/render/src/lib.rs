//! DiaCraft rendering: a page of a document drawn to premultiplied RGBA pixels, on `vello_cpu`.
//!
//! [`Renderer::render`] takes a document, a page and a [`View`] and returns a [`Rendered`]: the pixels, and a list of what the frame left
//! out or drew by a stand-in ([`Skipped`]), counted and never dropped. It draws what the file saved: a shape's cells are read through the
//! document's resolver, the saved `V` stands, and nothing is recalculated. The marker `Themed` is asked of the
//! theme by `calc`'s evaluator, which is the one place a formula runs (`read.rs`).
//!
//! **Coordinates.** [`View::to_px`] takes **paper inches** (the page's lower-left corner is the origin, x right and y up, so a window flips
//! y in this transform) to device pixels. A shape's cells are in drawing units; the renderer multiplies them by the page's scale
//! (`PageScale` over `DrawingScale`) on the way, and keeps line weights, text sizes and shadow offsets
//! on paper. [`View::fit_page`] makes the usual view. No grid, ruler or guide is drawn, in this crate or in an export: the
//! window draws them over the result.
//!
//! **What draws.** Pages and their background pages; shapes in document order with groups, masters and omitted members put together;
//! layers (hidden and coloured); geometry rows of all 15 types with the page's arcs, splines and rounded corners; fills (solid, gradient in
//! the four layouts, hatch and stipple), lines (patterns, caps and joins), shadows (simple and oblique), line ends, text (laid out by
//! `text`, drawn as outlines), pictures (clipped to their frame; an EMF, EMF+ or WMF picture drawn from the display list `diacraft-metafile` reads it to).
//!
//! **Thin lines (a project rule).** A stroke narrower than one device pixel comes out of a rasteriser as a grey as light
//! as it is thin, and the pictures the target app itself publishes draw such lines a pixel wide. So a target that rasterises (the window and
//! the PNG: [`Target::min_stroke`] is 1) draws a stroke under a pixel **one pixel wide, in the line's full colour and see-through share**, and a
//! writer of SVG or PDF (0) writes the **true width** and leaves the pixels to the reader. The rule covers a shape's outline, its shadow's
//! outline, the strokes of a hollow line end, the pens of a metafile picture and the stand-in for a picture that is not drawn; it leaves the
//! hatch marks of a fill pattern (our own artwork) and the thickening of a made-up bold alone. The line is not snapped to the pixel grid.
//! **What stays the line's own weight:** the size of a line end and the dashes, whose marks and gaps are laid out in weights
//! of the line, not of the width it is drawn at; the caps' share is taken out of each mark so that a mark is as long as seen as the pattern says.
//! A weight of 0 is one device pixel on a raster at any zoom and a quarter point in a file, with `S` taking weight 0. PDF's own
//! zero-width line (ISO 32000-1 8.4.3.2: the thinnest line the device draws) is not used: its width is the device's, so a file would draw a
//! 0.24 point line at a pixel on a screen and at a fraction of a point on a printer. The one number: `MIN_STROKE_PX` in `canvas.rs`.
//!
//! **Targets and exports.** The walk draws onto a [`Target`], not onto pixels: the raster canvas is one, and the SVG and PDF writers
//! (`diacraft-svg`, `diacraft-pdf`) implement the same trait, so a page is read, placed, dashed, shaded and laid out once, whatever it
//! becomes. [`Renderer::draw_onto`] draws a page onto any target; [`export`] holds what the outputs share (the page's layout, the limits of
//! an export, the errors) and the PNG. A target that writes letters as text says so ([`Target::wants_text`]) and is offered each run; one
//! that cannot write a run as text answers `false` and the walk draws its outlines, counted as [`SkipKind::TextAsOutlines`].
//!
//! **Cost.** A frame has a budget ([`Limits`]): a time for the whole frame and one for text layout, and counts of shapes, rows and
//! characters. A frame that reaches one stops drawing, and [`Rendered::skipped`] says how much it left out. The time is read from the renderer's
//! [`Clock`], which a test can step past every deadline. **A picture that goes into a file** (the preview a save writes) is drawn within
//! [`Limits::preview`]: no time, and a budget of work charged before the work is done ([`Limits::max_work`]: shapes entered and drawn, glyphs
//! painted, dashes, hatch lines, metafile items, pictures decoded and drawn), so the same drawing gives the same pixels however loaded the machine
//! is, cut where the drawing says when it is too big. Glyph outlines and laid-out
//! blocks are kept between frames, so panning over the same text does not pay for it again.
//!
//! **Threads.** A [`Renderer`] is `Send` (a window moves it to a worker thread) but not `Sync`: a frame takes `&mut self`. A
//! `diacraft_doc::Document` is shareable, and a frame borrows it for as long as it runs.
//!
//! **A frame on several threads.** [`Renderer::set_parallel`] says how many threads the frames drawn from then on may use
//! ([`Parallel`]; [`default_threads`] is the machine's cores less one, or `DIACRAFT_THREADS`); one is the default, and **a frame is the same
//! bytes at any count**. The walk of a page is cut into shares of neighbouring top shapes, walked at once onto recordings (`share.rs`,
//! `record.rs`), and the pixels are drawn in bands of rows, each in a context in the frame's own coordinates (`par.rs`). A page with a
//! picture, or one that reaches a budget a share could not see, is walked in order on one thread, and its pixels are still drawn in
//! bands. What holds the threads to the memory of one frame: the shares stop together at a budget (a tally), a recording is capped and the
//! walk goes on onto a canvas past the cap, the bands' contexts and blur layers are budgeted, and the threads are held by the rule an open
//! and a save are held by (`diacraft_container::spare::threads_fit`: the threads whose cost fits in an eighth of what a limited process's
//! address space leaves). [`Renderer::par_stats`] counts what was done. The web build draws on one thread whatever is said.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod canvas;
mod clip;
mod clock;
mod corners;
mod custom_end;
mod dash;
mod ends;
pub mod export;
mod face;
#[cfg(test)]
mod fault;
mod frame;
mod geometry;
mod gradient;
mod gray;
mod layers;
pub mod limits;
mod metafile;
mod par;
mod pattern;
mod picture;
mod placeholder;
pub mod print;
mod read;
mod record;
mod share;
mod skips;
mod style;
mod target;
mod text;

#[cfg(kani)]
mod kani_proofs;

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use diacraft_color::Rgba;
use diacraft_doc::{Document, SheetRef};
use diacraft_fonts::FontDb;
use kurbo::Affine;

pub use clock::Clock;
pub use diacraft_fonts::Face;
pub use face::{FaceReader, with_face_reader};
pub use geometry::geom_rows;
pub use limits::{Cancel, Limits};
pub use par::{MAX_THREADS, ParStats, Parallel, default_threads, threads_from};
pub use skips::{SkipKind, Skipped};
pub use target::{Gradient, GradientKind, GradientStop, Paint, Refused, Target, TextRun, TextSource};
pub use vello_cpu::Pixmap;

use crate::canvas::Canvas;
use crate::frame::Frame;
use crate::limits::{MAX_BACKGROUNDS, MAX_VIEW_PIXELS, MAX_VIEW_SIDE};
use crate::read::{Cx, eval_ctx};
use crate::skips::Skips;

/// A page's `ID`.
pub type PageId = u32;

/// How a frame is seen: where paper goes on the screen, and how big the screen is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    /// Paper inches (origin at the page's lower-left corner, y up) to device pixels (origin at the top-left corner, y down).
    pub to_px: Affine,
    pub width: u32,
    pub height: u32,
}

impl View {
    /// The view of a whole page at `px_per_inch` pixels to a paper inch, the page's top-left corner at the pixel origin: the transform
    /// flips y. `None` for a page the document lacks, or one without a size. The size is the page's, rounded up.
    pub fn fit_page(doc: &Document, page: PageId, px_per_inch: f64) -> Option<View> {
        let (w, h) = page_paper_size(doc, page)?;
        if !(px_per_inch.is_finite() && px_per_inch > 0.0) {
            return None;
        }
        let (width, height) = ((w * px_per_inch).ceil(), (h * px_per_inch).ceil());
        if !(width.is_finite() && height.is_finite() && width <= f64::from(MAX_VIEW_SIDE) && height <= f64::from(MAX_VIEW_SIDE)) {
            return None;
        }
        let to_px = Affine::new([px_per_inch, 0.0, 0.0, -px_per_inch, 0.0, h * px_per_inch]);
        Some(View { to_px, width: width.max(1.0) as u32, height: height.max(1.0) as u32 })
    }
}

/// A page's size on paper, in inches: `PageWidth` and `PageHeight` (in drawing units) times the page's scale.
/// `None` for a page the document lacks or one whose size is not a positive number.
pub fn page_paper_size(doc: &Document, page: PageId) -> Option<(f64, f64)> {
    doc.page(page)?;
    let mut ctx = eval_ctx();
    let cx = Cx::new(doc, &mut ctx);
    let sheet = SheetRef::Page(page);
    let positive = |name: &str| {
        let n = cx.num(&sheet, name);
        if n.is_finite() && n > 0.0 { n } else { 1.0 }
    };
    let s = positive("PageScale") / positive("DrawingScale");
    let (w, h) = (cx.num(&sheet, "PageWidth") * s, cx.num(&sheet, "PageHeight") * s);
    (w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0).then_some((w, h))
}

/// A frame: premultiplied RGBA pixels, row by row from the top, and what was left out.
#[derive(Clone, Debug, PartialEq)]
pub struct Rendered {
    /// `width * height * 4` bytes. Empty (and `width` and `height` 0) when the view was refused ([`SkipKind::ViewRefused`]); empty with the
    /// view's `width` and `height` when a newer request stopped the frame ([`Renderer::set_cancel`], [`SkipKind::Cancelled`]).
    pub rgba_premul: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// What the frame left out or drew by a stand-in, by kind, each with how many and the first of them in words. Empty for a frame that
    /// drew everything as the specification says.
    pub skipped: Vec<Skipped>,
}

impl Rendered {
    fn refused(first: String) -> Rendered {
        let mut skips = Skips::default();
        skips.note(SkipKind::ViewRefused, || first);
        Rendered { rgba_premul: Vec::new(), width: 0, height: 0, skipped: skips.into_list() }
    }

    /// How many were left out or drawn by a stand-in, of one kind.
    pub fn count(&self, kind: SkipKind) -> usize {
        self.skipped.iter().filter(|s| s.kind == kind).map(|s| s.count).sum()
    }
}

/// The renderer: the fonts it lays text out in, the limits it holds a frame to, and what it keeps between frames.
pub struct Renderer {
    fonts: Arc<FontDb>,
    limits: Limits,
    paper: Option<Rgba>,
    caches: text::Caches,
    cancel: Option<Cancel>,
    /// The walk is a print's (`print.rs`): shapes marked non-printing and layers marked unprinted are left off.
    print: bool,
    /// How the sheets of a print show their colours (`print.rs`, `gray.rs`): a page drawn for the screen or an export never reads it.
    print_color: print::PrintColor,
    /// How many threads a frame is drawn on (`par.rs`): one until the caller says.
    par: Parallel,
    /// What each share of a walk shared out keeps between frames (`share.rs`).
    share_caches: Vec<text::Caches>,
    stats: ParStats,
    /// Where every frame reads the time its budgets are spent against (`clock.rs`): the machine's, unless the caller says.
    clock: Clock,
    /// The work the frames drawn so far were charged, in the units of `Limits::max_work`.
    work_done: u64,
}

impl Default for Renderer {
    fn default() -> Renderer {
        Renderer::new()
    }
}

impl Renderer {
    /// A renderer with the bundled fonts only: nothing is read from the machine. A window builds the machine's font database off its UI
    /// thread and hands it to [`Renderer::with_fonts`].
    pub fn new() -> Renderer {
        Renderer::with_fonts(FontDb::bundled())
    }

    /// A renderer that lays text out in `fonts`.
    pub fn with_fonts(fonts: FontDb) -> Renderer {
        Renderer::with_shared_fonts(Arc::new(fonts))
    }

    /// A renderer that lays text out in a database others hold too: the window shares it with the commits that measure text for the blocks it
    /// draws, so that a block is sized in the faces it is drawn in.
    pub fn with_shared_fonts(fonts: Arc<FontDb>) -> Renderer {
        Renderer {
            fonts,
            limits: Limits::default(),
            paper: Some(Rgba::WHITE),
            caches: text::Caches::default(),
            cancel: None,
            print: false,
            print_color: print::PrintColor::Color,
            par: Parallel::SINGLE,
            share_caches: Vec::new(),
            stats: ParStats::default(),
            clock: Clock::system(),
            work_done: 0,
        }
    }

    /// How many threads the frames drawn from now on may use, and how big a frame must be to be shared out ([`Parallel`]). **A frame is the same
    /// bytes at any count**: the threads change how long it takes and nothing else. One thread (the default) draws on the calling thread alone,
    /// as the renderer always did. The web build draws on one whatever is said here. The caller chooses ([`default_threads`] is a fair default).
    pub fn set_parallel(&mut self, par: Parallel) {
        self.par = Parallel { threads: par.threads.clamp(1, MAX_THREADS), ..par };
    }

    /// The threads and floor a frame is drawn with.
    pub fn parallel(&self) -> Parallel {
        self.par
    }

    /// How often frames have been shared out since the renderer was made: a count of work done, for a test to see that a frame it drew on many
    /// threads was drawn on them (and not on one, which gives the same pixels).
    pub fn par_stats(&self) -> ParStats {
        self.stats
    }

    /// Replaces the font database (the kept layouts and outlines belonged to the old one and are dropped).
    pub fn set_fonts(&mut self, fonts: FontDb) {
        self.fonts = Arc::new(fonts);
        self.caches.clear();
        self.share_caches.clear();
    }

    /// Drops everything kept between frames: laid-out text, glyph outlines, and decoded pictures together with the parts of the package they
    /// hold on to. A window calls it when another document opens, so that the old one's pictures do not outlive it; the next frame pays
    /// again for what it draws. Fonts and limits stay as they are.
    pub fn clear_caches(&mut self) {
        self.caches.clear();
        self.share_caches.clear();
    }

    /// Lets a newer request stop the frames drawn from now on, until it is replaced or cleared with `None`: see [`Cancel`]. A frame that was
    /// stopped says [`SkipKind::Cancelled`], and **[`Rendered::rgba_premul`] of a [`Renderer::render`] that was stopped is empty** (its
    /// `width` and `height` are the view's); an export is stopped the same way and its writer is left as far as it got. A frame drawn with no
    /// request pending is as it is without this.
    pub fn set_cancel(&mut self, cancel: Option<Cancel>) {
        self.cancel = cancel;
    }

    /// How many images this renderer has decoded to pixels since it was made: pictures that are rasters, and each bitmap of a metafile
    /// picture. A picture is decoded once and kept, so a frame that draws only pictures already kept leaves this where it was; a frame
    /// out of time decodes at most the pictures it reaches before the time is gone. It is a count of work done, and clearing the caches
    /// ([`Renderer::clear_caches`]) does not put it back to 0.
    pub fn images_decoded(&self) -> u64 {
        self.caches.pictures.decoded()
    }

    /// How many blocks of text this renderer has laid out since it was made. A block is laid out once and kept, so a frame that draws only
    /// text already kept leaves this where it was. Like [`Renderer::images_decoded`], it is a count of work done, and clearing the caches
    /// does not put it back to 0.
    pub fn texts_laid_out(&self) -> u64 {
        self.share_caches.iter().fold(self.caches.laid_out(), |n, c| n.saturating_add(c.laid_out()))
    }

    /// The work the frames this renderer drew were charged since it was made, in the units of [`Limits::max_work`]: what the walk that gave each
    /// picture did, in order or shared out (the shares of a walk that had to be done again in order are not counted, and a tally stops them within a budget's
    /// worth). Like [`Renderer::texts_laid_out`], it is a count of work done for a test, and it never goes back.
    pub fn work_done(&self) -> u64 {
        self.work_done
    }

    /// The limits a frame is held to.
    pub fn limits(&self) -> Limits {
        self.limits
    }

    pub fn set_limits(&mut self, limits: Limits) {
        self.limits = limits;
    }

    /// The clock the frames drawn from now on read the time from ([`Clock`]): the machine's by default. A frame whose limits hold no time
    /// draws the same whatever the clock says; a test steps a clock past every deadline to see what one that has them leaves out.
    pub fn set_clock(&mut self, clock: Clock) {
        self.clock = clock;
    }

    /// The colour of the page's paper, drawn over the page's rectangle before any shape; `None` leaves the page transparent. White by default.
    pub fn set_paper(&mut self, paper: Option<Rgba>) {
        self.paper = paper;
    }

    /// The colour of the page's paper, or `None` for a transparent page.
    pub fn paper(&self) -> Option<Rgba> {
        self.paper
    }

    /// Draws `page` of `doc` through `view`.
    ///
    /// Never panics: a view that is too large, a page the document lacks and a bug in drawing each give a result with the reason in
    /// [`Rendered::skipped`].
    pub fn render(&mut self, doc: &Document, page: PageId, view: &View) -> Rendered {
        let pixels = u64::from(view.width) * u64::from(view.height);
        if view.width == 0 || view.height == 0 || view.width > MAX_VIEW_SIDE || view.height > MAX_VIEW_SIDE || pixels > MAX_VIEW_PIXELS {
            return Rendered::refused(format!("a view of {} by {} pixels", view.width, view.height));
        }
        let coeffs = view.to_px.as_coeffs();
        if !coeffs.iter().all(|c| c.is_finite()) {
            return Rendered::refused("a view transform that is not a number".to_owned());
        }
        let (Ok(w), Ok(h)) = (u16::try_from(view.width), u16::try_from(view.height)) else {
            return Rendered::refused("a view too wide for the rasteriser".to_owned());
        };
        let blank = || vec![0u8; usize::try_from(pixels).unwrap_or(0).saturating_mul(4)];
        if doc.page(page).is_none() {
            let mut skips = Skips::default();
            skips.note(SkipKind::NoSuchPage, || format!("page {page}"));
            return Rendered { rgba_premul: blank(), width: view.width, height: view.height, skipped: skips.into_list() };
        }
        // A bug in drawing must not take the window down: the frame is lost, the kept caches are dropped, and the result says so.
        let drawn = catch_unwind(AssertUnwindSafe(|| self.draw(doc, page, view, w, h)));
        let bug = match drawn {
            Ok(Ok((rgba_premul, skips))) => return Rendered { rgba_premul, width: view.width, height: view.height, skipped: skips.into_list() },
            Ok(Err(words)) => words,
            Err(_) => "a frame that met a bug".to_owned(),
        };
        self.caches.clear();
        self.share_caches.clear();
        let mut skips = Skips::default();
        skips.note(SkipKind::Recovered, || bug);
        Rendered { rgba_premul: blank(), width: view.width, height: view.height, skipped: skips.into_list() }
    }

    /// Draws `page` of `doc` onto any [`Target`]: the walk of [`Renderer::render`] with another thing to draw on. An export is this with a
    /// writer for SVG or PDF as the target (the target's `to_px` and `size` say where paper goes, as a [`View`] does for pixels).
    ///
    /// The page's paper is drawn first ([`Renderer::set_paper`]), then its background pages, then the page. Returns what was left out or
    /// drawn by a stand-in, as [`Rendered::skipped`] does. Never panics: a bug in drawing is caught, the clips and layers it left open on
    /// the target are closed, and [`SkipKind::Recovered`] is in the list; a page the document lacks gives [`SkipKind::NoSuchPage`].
    pub fn draw_onto<T: Target>(&mut self, doc: &Document, page: PageId, target: &mut T) -> Vec<Skipped> {
        if doc.page(page).is_none() {
            let mut skips = Skips::default();
            skips.note(SkipKind::NoSuchPage, || format!("page {page}"));
            return skips.into_list();
        }
        let mark = target.mark();
        match catch_unwind(AssertUnwindSafe(|| self.walk(doc, page, target))) {
            Ok(skips) => skips.into_list(),
            Err(_) => {
                target.rewind(mark);
                self.caches.clear();
                let mut skips = Skips::default();
                skips.note(SkipKind::Recovered, || "a page that met a bug".to_owned());
                skips.into_list()
            }
        }
    }

    fn draw(&mut self, doc: &Document, page: PageId, view: &View, w: u16, h: u16) -> Result<(Vec<u8>, Skips), String> {
        self.rasterise(doc, page, w, h, view.to_px, None)
    }

    /// Walks `page` onto a canvas of `w` by `h` pixels (with `clip`, paper inches, put on first) and gives its pixels, **empty if a newer
    /// request cancelled the frame** (nobody wants them: the layers still waiting in the canvas are dropped, not rasterised). A bug the
    /// rasteriser's threads meet is an `Err` in words.
    ///
    /// On one thread the walk draws straight onto the canvas. On more it is cut into shares walked at once onto recordings ([`record`], `share.rs`)
    /// and the rasteriser draws the bands of the frame on separate threads ([`par`]): the same calls in the same order, so the same pixels. A walk
    /// that cannot be shared out is done in order onto the canvas, as on one thread.
    fn rasterise(
        &mut self,
        doc: &Document,
        page: PageId,
        w: u16,
        h: u16,
        to_px: Affine,
        clip: Option<&kurbo::BezPath>,
    ) -> Result<(Vec<u8>, Skips), String> {
        let threads = self.par.usable_threads();
        // The frame's time is its own, however many times the walk is started: a walk done in order after one that was shared out has what the
        // first left.
        let started = self.clock.now();
        if threads > 1 && self.shapes_of(doc, page) >= self.par.min_shapes {
            let mut recorder = record::Recorder::new(w, h, to_px);
            if let Some(clip) = clip {
                recorder.push_clip(clip);
            }
            let shared = self.walk_shared(doc, page, &mut recorder, threads, started);
            if shared.is_none() {
                self.stats.abandoned_time += self.clock.since(started);
            }
            if let Some(skips) = shared {
                self.stats.walks_shared += 1;
                if skips.has(SkipKind::Cancelled) {
                    return Ok((Vec::new(), skips));
                }
                let walked = self.clock.since(started);
                let recording = recorder.finish();
                let rastered = self.clock.now();
                let (pixels, drawers) = par::raster(&recording, self.par, threads)?;
                self.stats.frames_banded += u64::from(drawers > 1);
                self.stats.most_band_threads = self.stats.most_band_threads.max(drawers);
                self.stats.walk_time += walked;
                self.stats.raster_time += self.clock.since(rastered);
                return Ok((pixels, skips));
            }
            // The walk could not be shared out (a picture, a budget, a bug): it is done in order, **drawn straight onto the canvas as it was before the
            // threads**. Writing it down to draw its pixels in bands costs more than the bands save, on a page whose walk is most of its frame.
            self.stats.walks_in_order += 1;
        }
        let mut canvas = Canvas::new(w, h, to_px);
        if let Some(clip) = clip {
            canvas.push_clip(clip);
        }
        let skips = self.walk_started(doc, page, &mut canvas, started);
        if skips.has(SkipKind::Cancelled) {
            return Ok((Vec::new(), skips));
        }
        Ok((canvas.finish(), skips))
    }

    /// How many shapes at the top of `page` and the backgrounds behind it: what a frame's size is judged by before it is walked.
    fn shapes_of(&self, doc: &Document, page: PageId) -> usize {
        let mut count = doc.page(page).map_or(0, |p| p.shapes.len());
        for background in doc.background_chain(page).iter().take(MAX_BACKGROUNDS) {
            count = count.saturating_add(doc.page(background.id).map_or(0, |p| p.shapes.len()));
        }
        count
    }

    /// The walk of a page and its backgrounds onto `target`.
    fn walk<T: Target>(&mut self, doc: &Document, page: PageId, target: &mut T) -> Skips {
        self.walk_started(doc, page, target, self.clock.now())
    }

    /// As [`Renderer::walk`] for a frame that began at `started` (a reading of the renderer's clock): the frame's time budget counts from there.
    fn walk_started<T: Target>(&mut self, doc: &Document, page: PageId, target: &mut T, started: std::time::Duration) -> Skips {
        let mut eval = eval_ctx();
        let cx = Cx::new(doc, &mut eval);
        let mut frame = Frame::new(&cx, target, &self.fonts, &mut self.caches, self.limits);
        frame.clock = self.clock.clone();
        frame.start = started;
        frame.cancel = self.cancel.clone();
        frame.print = self.print;
        if let Some(paper) = self.paper {
            frame.paper(page, paper);
        }
        // Background pages first, the farthest first, each with its own scale and layers; the page itself last.
        let chain = doc.background_chain(page);
        for background in chain.iter().take(MAX_BACKGROUNDS).rev() {
            frame.draw_page(background.id);
        }
        frame.draw_page(page);
        let counts = frame.counts();
        self.stats.walked_shapes = self.stats.walked_shapes.saturating_add(u64::try_from(counts.drawn).unwrap_or(u64::MAX));
        self.work_done = self.work_done.saturating_add(counts.work);
        cx.take_skips()
    }
}
