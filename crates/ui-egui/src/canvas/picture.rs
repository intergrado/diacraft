//! The picture of the page on view: what is asked of the render worker, what is kept, and what is shown meanwhile.
//!
//! The drawing area asks for the part of the page it shows, and a margin round it, as one image at the screen's pixel density. The
//! image is kept with the part of the page (in inches) it covers, so it can be laid over the page again under any later view:
//! while a new picture is being drawn the last one stays on the screen, moved and scaled to where the page is now. A pan or a zoom
//! that leaves the part wanted inside the image asks for nothing. While the view is moving the worker draws within a short frame
//! time ([`MOVING_FRAME_TIME`]), and once it has been still for [`REST_AFTER`] the picture is drawn again within the full limits.
//!
//! Which page, which revision of the document and which document (the **generation**: it changes whenever a file or a new drawing
//! replaces the old one) an image was drawn for are kept with it, and an image is shown only for the page and the document it is of.
//! The grid and the rulers are not in the image: neither is page content.
//!
//! **A picture at rest that left shapes or labels out for want of time** (the renderer's frame budget, or its budget for laying out text)
//! is asked for again, a bounded number of times ([`MAX_REASKS`]) and with more time each: the text laid out the first time is kept by the
//! renderer, so the next picture reaches further. It is asked again only while the picture before it left fewer out than the one before
//! that, and only once the view is still. A newer request stops a picture in progress (`render_worker.rs`).

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use diacraft_engine::Session;
use diacraft_engine::doc::Document;
use diacraft_render::{Limits, Rendered, SkipKind, Skipped, View};
use egui::{Context, Rect, TextureHandle, TextureOptions, pos2, vec2};
use kurbo::Affine;

use crate::render_worker::{Done, Job, MakeDrawer, RenderWorker, bundled_drawer, system_drawer};

/// How far past the drawing area the image reaches on each side, as a share of the area: a small pan stays inside it.
pub const MARGIN: f32 = 0.25;

/// The longest side of an image, in pixels, whatever the graphics back end takes: the image is also held to the back end's own limit
/// ([`Want::max_side`]), which a WebGL context or an old adapter can set far below this.
pub const MAX_SIDE_PX: f32 = 8192.0;

/// The most pixels of an image with its margin; past it the margin is dropped. 24 million is 96 MB of pixels.
pub const MAX_PIXELS: f32 = 24.0e6;

/// How long the view must stand still before the picture is drawn again within the full limits.
pub const REST_AFTER: Duration = Duration::from_millis(150);

/// The frame time and the text time of a picture drawn while the view moves. A project rule: a short frame draws what it can, and the
/// picture at rest completes it.
pub const MOVING_FRAME_TIME: Duration = Duration::from_millis(120);
pub const MOVING_TEXT_TIME: Duration = Duration::from_millis(40);

/// How many times a picture at rest that ran out of time is asked for again. Each asks for twice the time of the one before
/// ([`limits_for`]), so the three pictures together take at most seven times the renderer's defaults (about 10 s of a worker thread that
/// a newer request stops at once).
pub const MAX_REASKS: u8 = 2;

/// The frame time of a picture drawn on the window's own thread, where a slow frame is a frozen window (the web).
const INLINE_FRAME_TIME: Duration = Duration::from_millis(200);

/// How long the window waits after a drawing that failed before it asks again, so a failure that repeats is not a busy loop.
const FAILURE_BACKOFF: Duration = Duration::from_secs(1);

/// The most requests whose answers are still awaited. An answer for a request no longer in the list is dropped.
const MAX_PENDING: usize = 8;

/// What the page says when the drawing of it failed and no picture came: the renderer met a bug and the frame is lost.
pub const LOST_FRAME: &str = "This page could not be drawn. DiaCraft met an internal problem while drawing it. \
    Your drawing is not changed. Please report this bug.";

/// A part of a page, in inches from its lower left corner, y up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Coverage {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

impl Coverage {
    /// Whether `other` lies inside this, to within `tolerance` inches.
    pub fn holds(&self, other: &Coverage, tolerance: f64) -> bool {
        self.x0 <= other.x0 + tolerance && self.y0 <= other.y0 + tolerance && self.x1 >= other.x1 - tolerance && self.y1 >= other.y1 - tolerance
    }
}

/// What the drawing area needs a picture of.
#[derive(Clone, Copy, Debug)]
pub struct Want {
    pub page: u32,
    /// Where the page is on the screen, in points, and how many points make an inch.
    pub page_rect: Rect,
    pub scale: f64,
    /// The drawing area, in points.
    pub area: Rect,
    /// Device pixels to a point.
    pub ppp: f32,
    /// The longest side of a texture the graphics back end takes, in pixels (`egui`'s `max_texture_side`): **a texture past it makes egui panic, and
    /// on the web nothing catches that**, so no image is planned with a side over it. It is held to [`MAX_SIDE_PX`] as well.
    pub max_side: f32,
}

impl Want {
    /// The longest side an image may have, in pixels: the back end's limit, and never more than [`MAX_SIDE_PX`]. A limit that is not a number
    /// above 2 plans no image at all.
    pub fn side_limit(&self) -> f32 {
        if self.max_side.is_finite() { self.max_side.min(MAX_SIDE_PX) } else { MAX_SIDE_PX }
    }
}

/// The image to ask for and the part of the page it covers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plan {
    pub width: u32,
    pub height: u32,
    /// The transform of [`View::to_px`], as `Affine` coefficients.
    pub to_px: [f64; 6],
    /// What the image covers.
    pub covers: Coverage,
    /// The part of the page the drawing area shows: what the image must cover.
    pub needed: Coverage,
}

fn finite_rect(r: Rect) -> bool {
    r.min.x.is_finite() && r.min.y.is_finite() && r.max.x.is_finite() && r.max.y.is_finite()
}

/// The image that serves `want`: the drawing area and a margin, as pixels at the screen's density. `None` when there is nothing to
/// draw into (no area, no scale, no pixel density).
pub fn plan(want: &Want) -> Option<Plan> {
    let (scale, ppp) = (want.scale, f64::from(want.ppp));
    if !(scale.is_finite() && scale > 0.0 && ppp.is_finite() && ppp > 0.0) || !finite_rect(want.page_rect) || !finite_rect(want.area) {
        return None;
    }
    // Two pixels short of the limit: the image starts on a whole pixel and ends on one.
    let limit = want.side_limit();
    let max_pts = (limit - 2.0) / want.ppp;
    // What must be covered: the drawing area, cut to what an image can hold, about its middle.
    let centre = want.area.center();
    let (w, h) = (want.area.width().min(max_pts), want.area.height().min(max_pts));
    if w < 1.0 || h < 1.0 {
        return None;
    }
    let needed = Rect::from_center_size(centre, vec2(w, h));
    // With the margin, unless that is too large an image.
    let wide = needed.expand2(vec2(w * MARGIN, h * MARGIN));
    let big = wide.width() > max_pts || wide.height() > max_pts || wide.width() * wide.height() * want.ppp * want.ppp > MAX_PIXELS;
    let region = if big { needed } else { wide };
    // The image starts on a device pixel, so it lies on the screen's pixels at rest.
    let left = (f64::from(region.min.x) * ppp).floor() / ppp;
    let top = (f64::from(region.min.y) * ppp).floor() / ppp;
    let width = ((f64::from(region.max.x) - left) * ppp).ceil().max(1.0);
    let height = ((f64::from(region.max.y) - top) * ppp).ceil().max(1.0);
    if width > f64::from(limit) || height > f64::from(limit) {
        return None;
    }
    let (px, py) = (f64::from(want.page_rect.min.x), f64::from(want.page_rect.max.y));
    // Inches of the page at a screen point.
    let inch_x = |sx: f64| (sx - px) / scale;
    let inch_y = |sy: f64| (py - sy) / scale;
    let covers = Coverage { x0: inch_x(left), y0: inch_y(top + height / ppp), x1: inch_x(left + width / ppp), y1: inch_y(top) };
    let needed_in = Coverage {
        x0: inch_x(f64::from(needed.min.x)),
        y0: inch_y(f64::from(needed.max.y)),
        x1: inch_x(f64::from(needed.max.x)),
        y1: inch_y(f64::from(needed.min.y)),
    };
    // x' = sx x + tx, y' = -sy y + ty: paper inches (y up) to device pixels (y down) from the image's top left.
    let to_px = [scale * ppp, 0.0, 0.0, -scale * ppp, (px - left) * ppp, (py - top) * ppp];
    Some(Plan { width: width as u32, height: height as u32, to_px, covers, needed: needed_in })
}

/// A picture of a page, drawn and uploaded.
pub struct PageImage {
    texture: TextureHandle,
    pub page: u32,
    pub revision: u64,
    pub generation: u64,
    /// Device pixels to a paper inch.
    pub px_per_inch: f64,
    pub covers: Coverage,
    /// It was drawn within the moving limits and is to be drawn again at rest.
    pub rough: bool,
    pub size: [usize; 2],
    /// How many of its pixels are not clear: a picture of nothing has none.
    pub lit: usize,
    /// How many shapes and text blocks it left out for want of time (the frame's time, the text's time, or the characters of text).
    pub left_out: usize,
    /// It was drawn at rest and left something out for want of time, and asking again may reach further.
    pub short: bool,
    /// Which asking it was: 0 for the first, then the times it was asked for again.
    pub attempt: u8,
}

impl PageImage {
    /// The texture to paint.
    pub fn texture_id(&self) -> egui::TextureId {
        self.texture.id()
    }

    /// Where the image lies on the screen when the page is at `page_rect` and `scale` points make an inch.
    pub fn screen_rect(&self, page_rect: Rect, scale: f64) -> Rect {
        let at = |x: f64, y: f64| pos2((f64::from(page_rect.min.x) + x * scale) as f32, (f64::from(page_rect.max.y) - y * scale) as f32);
        Rect::from_min_max(at(self.covers.x0, self.covers.y1), at(self.covers.x1, self.covers.y0))
    }
}

/// A request sent and not yet answered.
struct Pending {
    serial: u64,
    page: u32,
    revision: u64,
    generation: u64,
    px_per_inch: f64,
    covers: Coverage,
    rough: bool,
    attempt: u8,
}

impl Pending {
    fn serves(&self, page: u32, generation: u64, revision: u64, px_per_inch: f64, needed: &Coverage) -> bool {
        serves(self.page, self.generation, self.revision, self.px_per_inch, &self.covers, (page, generation, revision, px_per_inch), needed)
    }
}

/// Whether a picture of `page`, `generation` and `revision` at `px_per_inch`, covering `covers`, serves what is wanted.
fn serves(page: u32, generation: u64, revision: u64, ppi: f64, covers: &Coverage, wanted: (u32, u64, u64, f64), needed: &Coverage) -> bool {
    let (w_page, w_generation, w_revision, w_ppi) = wanted;
    page == w_page && generation == w_generation && revision == w_revision && (ppi - w_ppi).abs() <= 1e-3 * w_ppi && covers.holds(needed, 0.5 / w_ppi)
}

/// How pictures are drawn: on a thread started when the first page with something on it is shown, or on the window's own.
enum Mode {
    Thread(Option<MakeDrawer>),
    Inline(Option<MakeDrawer>),
}

/// The pictures of the pages: the worker, the picture on the screen and the requests in flight.
pub struct Pictures {
    mode: Mode,
    worker: Option<RenderWorker>,
    generation: u64,
    serial: u64,
    /// The document as the worker was last given it, with the revision and the generation it is of.
    doc: Option<(u64, u64, Arc<Document>)>,
    image: Option<PageImage>,
    pending: VecDeque<Pending>,
    /// Where the page was on the last frame: scale, then the top left of the page.
    last_view: Option<(f64, f32, f32)>,
    /// When the view last moved and when a drawing last failed, by the window's clock ([`crate::clock::Clock`]) and not the machine's.
    moved_at: Option<Duration>,
    failed_at: Option<Duration>,
    /// The window's clock at the frame being drawn: what a picture that fails is dated by.
    now: Duration,
    /// What the last picture of full quality left out, with the page and revision it is of.
    skipped: Option<(u32, u64, u64, Vec<Skipped>)>,
    /// Why the last picture of a page has nothing on it though the page has shapes: the drawing failed (a panic the worker or the renderer
    /// caught), with the page and the generation it is of. The page shows it in words.
    lost: Option<(u32, u64, String)>,
    /// Frames lost whole to a caught panic, and shapes lost to one, since the window began: what `ui.inspect` says.
    pub frames_lost: u64,
    pub shapes_lost: u64,
    /// Caught panics the window has not yet added to its own count ([`Pictures::take_panics`]).
    unreported: u64,
    /// Jobs sent, and pictures received, since the window began.
    pub requested: u64,
    pub received: u64,
    /// How long the last picture took to draw.
    pub last_took: Duration,
}

impl Default for Pictures {
    fn default() -> Pictures {
        Pictures::new()
    }
}

impl Pictures {
    /// Pictures drawn on a worker thread over the machine's fonts.
    pub fn new() -> Pictures {
        Pictures::with_mode(Mode::Thread(Some(system_drawer())))
    }

    /// Pictures drawn by `make`'s drawer on a worker thread.
    pub fn threaded(make: MakeDrawer) -> Pictures {
        Pictures::with_mode(Mode::Thread(Some(make)))
    }

    /// Pictures drawn by `make`'s drawer on the calling thread, at once.
    pub fn inline(make: MakeDrawer) -> Pictures {
        Pictures::with_mode(Mode::Inline(Some(make)))
    }

    fn with_mode(mode: Mode) -> Pictures {
        Pictures {
            mode,
            worker: None,
            generation: 1,
            serial: 0,
            doc: None,
            image: None,
            pending: VecDeque::new(),
            last_view: None,
            moved_at: None,
            failed_at: None,
            now: Duration::ZERO,
            skipped: None,
            lost: None,
            frames_lost: 0,
            shapes_lost: 0,
            unreported: 0,
            requested: 0,
            received: 0,
            last_took: Duration::ZERO,
        }
    }

    /// A different document replaced the old one: nothing drawn for the old one is shown or accepted again.
    pub fn reset(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.doc = None;
        self.image = None;
        self.pending.clear();
        self.skipped = None;
        self.lost = None;
        self.failed_at = None;
        // The view of the new document is a new view: its first picture is drawn at once, within the short limits.
        self.last_view = None;
        self.moved_at = None;
        // The old document's laid-out text and decoded pictures (and the parts of its file they hold on to) must not outlive it, and a job
        // still drawing it is stopped.
        if let Some(worker) = &mut self.worker {
            worker.clear_caches();
        }
    }

    /// Which document the pictures are of: it changes whenever a file or a new drawing replaces the old one ([`Pictures::reset`]).
    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    /// The picture that is shown for `page`, if there is one.
    pub fn image(&self, page: u32) -> Option<&PageImage> {
        self.image.as_ref().filter(|i| i.page == page && i.generation == self.generation)
    }

    /// What the last full picture of `page` left out or drew by a stand-in.
    pub fn skipped(&self, page: u32) -> &[Skipped] {
        match &self.skipped {
            Some((p, generation, _, list)) if *p == page && *generation == self.generation => list,
            _ => &[],
        }
    }

    /// Why the picture of `page` is missing or empty, in words for the page, if the drawing of it failed.
    pub fn lost(&self, page: u32) -> Option<&str> {
        match &self.lost {
            Some((p, generation, words)) if *p == page && *generation == self.generation => Some(words),
            _ => None,
        }
    }

    /// The caught panics (a frame lost whole, a shape lost) since the last call: the window adds them to the panics it reports, so that a
    /// check of `ui.inspect` sees a page that failed to draw.
    pub fn take_panics(&mut self) -> u64 {
        std::mem::take(&mut self.unreported)
    }

    /// Whether a request is in flight.
    pub fn busy(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Takes the answers that have arrived, and asks for a picture if `want` needs one. → the message of a drawing that failed.
    pub fn update(&mut self, ctx: &Context, session: &Session, want: &Want, now: Duration) -> Option<String> {
        let Some(doc) = session.document() else {
            self.now = now;
            return self.receive(ctx);
        };
        self.update_doc(ctx, doc, session.revision(), want, now)
    }

    /// [`Pictures::update`] for a document that is not the session's: the copy a theme preview draws (`theme_preview.rs`). `revision` says which version of it this is: a picture of
    /// another revision is drawn again, and one that arrives late for an older revision does not replace a newer one.
    pub fn update_doc(&mut self, ctx: &Context, doc: &Document, revision: u64, want: &Want, now: Duration) -> Option<String> {
        self.now = now;
        let mut failure = self.receive(ctx);
        if !has_content(doc, want.page) {
            // A page with nothing on it needs no picture, and no thread.
            self.image = self.image.take().filter(|i| i.page != want.page);
            self.lost = self.lost.take().filter(|(page, ..)| *page != want.page);
            self.pending.retain(|p| p.page != want.page);
            return failure;
        }
        let view = (want.scale, want.page_rect.min.x, want.page_rect.min.y);
        let moved =
            self.last_view.is_none_or(|(s, x, y)| (s - view.0).abs() > 1e-6 * s.max(1.0) || (x - view.1).abs() > 0.01 || (y - view.2).abs() > 0.01);
        if moved {
            self.moved_at = Some(now);
        }
        self.last_view = Some(view);
        let moving = self.moved_at.is_some_and(|t| now.saturating_sub(t) < REST_AFTER);
        if moving {
            // Wake up once the view has stood still, to draw the picture again in full.
            ctx.request_repaint_after(REST_AFTER);
        }
        if self.failed_at.is_some_and(|t| now.saturating_sub(t) < FAILURE_BACKOFF) {
            ctx.request_repaint_after(FAILURE_BACKOFF);
            return failure;
        }
        let Some(plan) = plan(want) else { return failure };
        let generation = self.generation;
        let ppi = plan.to_px[0];
        let wanted = (want.page, generation, revision, ppi);
        let from_image = self.image.as_ref().filter(|i| serves(i.page, i.generation, i.revision, i.px_per_inch, &i.covers, wanted, &plan.needed));
        let from_pending = self.pending.iter().rev().find(|p| p.serves(want.page, generation, revision, ppi, &plan.needed));
        // A picture of full quality that ran out of time asks again, once the view is still, a bounded number of times (never on the web's
        // own thread, where each asking would freeze the window).
        let reasks = if self.drawn_on_the_ui_thread() { 0 } else { MAX_REASKS };
        let again = from_image.filter(|i| !i.rough && i.short && i.attempt < reasks).map(|i| i.attempt.saturating_add(1));
        let image_full = from_image.is_some_and(|i| !i.rough) && again.is_none();
        let full = image_full || from_pending.is_some_and(|p| !p.rough);
        let rough_only = !full && (from_image.is_some() || from_pending.is_some());
        let submit = !full && (!rough_only || !moving);
        if submit {
            let rough = moving;
            let attempt = if rough { 0 } else { again.unwrap_or(0) };
            // A picture of the same view that is already on its way (a rough one, with the full one now asked for) is not stopped.
            let supersedes = from_pending.is_none();
            if let Some(error) = self.request(ctx, doc, (want.page, revision), &plan, Ask { rough, attempt, supersedes }) {
                failure = failure.or(Some(error));
            }
            // A drawer on this thread has answered already.
            failure = failure.or(self.receive(ctx));
        }
        failure
    }

    /// Whether a picture is drawn on the window's own thread where that freezes the window: the web, whose worker is inline.
    fn drawn_on_the_ui_thread(&self) -> bool {
        inline_limits_apply(self.worker.as_ref().is_some_and(RenderWorker::is_inline))
    }

    /// Sends the request for `plan`, the `attempt`th asking for it.
    fn request(&mut self, ctx: &Context, doc: &Document, (page, revision): (u32, u64), plan: &Plan, ask: Ask) -> Option<String> {
        let Ask { rough, attempt, supersedes } = ask;
        let shared = match &self.doc {
            Some((generation, rev, shared)) if *generation == self.generation && *rev == revision => Arc::clone(shared),
            _ => {
                let shared = Arc::new(doc.clone());
                self.doc = Some((self.generation, revision, Arc::clone(&shared)));
                shared
            }
        };
        if self.worker.is_none() {
            self.worker = self.start(ctx);
        }
        let worker = self.worker.as_mut()?;
        // The worker says where a picture is drawn: a thread that could not be had (the web) leaves a worker that draws inline, whatever mode
        // was asked for (the report said the inline limit applied there; it never did).
        let limits = limits_for(rough, inline_limits_apply(worker.is_inline()), attempt);
        self.serial += 1;
        let serial = self.serial;
        let view = View { to_px: Affine::new(plan.to_px), width: plan.width, height: plan.height };
        self.pending.push_back(Pending {
            serial,
            page,
            revision,
            generation: self.generation,
            px_per_inch: plan.to_px[0],
            covers: plan.covers,
            rough,
            attempt,
        });
        while self.pending.len() > MAX_PENDING {
            self.pending.pop_front();
        }
        self.requested += 1;
        worker.submit(Job { serial, doc: shared, page, view, limits, supersedes, cancel: None, sheet: None });
        None
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

    /// Takes in the answers that have arrived. → the message of the first failure among them.
    fn receive(&mut self, ctx: &Context) -> Option<String> {
        let done = self.worker.as_mut().map(RenderWorker::poll).unwrap_or_default();
        let mut failure = None;
        for answer in done {
            if let Some(msg) = self.accept(ctx, answer) {
                failure = failure.or(Some(msg));
            }
        }
        failure
    }

    fn accept(&mut self, ctx: &Context, done: Done) -> Option<String> {
        // Serial 0 is the worker's own: it could not start with the machine's fonts.
        if done.serial == 0 {
            let failure = done.result.err();
            if failure.is_some() {
                // The machine's fonts could not be read, which a panic in the font scan caused: caught, and counted as one.
                self.frames_lost += 1;
                self.unreported += 1;
            }
            return failure;
        }
        let at = self.pending.iter().position(|p| p.serial == done.serial)?;
        let pending = self.pending.remove(at)?;
        // Requests older than the answer that came are not going to matter.
        self.pending.retain(|p| p.serial > done.serial);
        if pending.generation != self.generation {
            return None;
        }
        self.last_took = done.took;
        let rendered = match done.result {
            Ok(rendered) => rendered,
            Err(msg) => {
                // The worker caught a panic in drawing and no picture came: the page says so, in words, over what it showed.
                self.failed_at = Some(self.now);
                self.frames_lost += 1;
                self.unreported += 1;
                self.lost = Some((pending.page, pending.generation, LOST_FRAME.to_owned()));
                return Some(msg);
            }
        };
        // A frame that a newer request stopped has no picture, and says nothing about the page.
        if rendered.count(SkipKind::Cancelled) > 0 {
            return None;
        }
        self.received += 1;
        // The worker made the image and counted its lit pixels; the window's thread has only to upload it.
        let lit = done.lit;
        self.account(&rendered, pending.page, pending.generation, lit);
        if !pending.rough {
            self.skipped = Some((pending.page, pending.generation, pending.revision, rendered.skipped.clone()));
        }
        let image = done.image?;
        let size = image.size;
        // A newer picture of the same page is not replaced by an older one that arrives late.
        let previous = self.image.as_ref().filter(|i| i.page == pending.page && i.generation == pending.generation);
        if previous.is_some_and(|i| i.revision > pending.revision) {
            return None;
        }
        // A picture at rest that ran out of time asks again, but only while each asking leaves fewer out than the one before it.
        let left_out = [SkipKind::FrameBudget, SkipKind::TextBudget, SkipKind::TextCharBudget].iter().map(|k| rendered.count(*k)).sum::<usize>();
        let progress = pending.attempt == 0 || previous.is_none_or(|i| i.revision != pending.revision || left_out < i.left_out);
        let short = !pending.rough && left_out > 0 && progress;
        let texture = ctx.load_texture(format!("page-{}", pending.serial), image, TextureOptions::LINEAR);
        self.image = Some(PageImage {
            texture,
            page: pending.page,
            revision: pending.revision,
            generation: pending.generation,
            px_per_inch: pending.px_per_inch,
            covers: pending.covers,
            rough: pending.rough,
            size,
            lit,
            left_out,
            short,
            attempt: pending.attempt,
        });
        ctx.request_repaint();
        None
    }
}

impl Pictures {
    /// Counts what a picture says it lost to a caught panic, and keeps the words for the page when it lost all it had: the renderer's own
    /// net lost the frame ([`SkipKind::Recovered`]), or shapes failed and nothing at all is on the picture.
    fn account(&mut self, rendered: &Rendered, page: u32, generation: u64, lit: usize) {
        let (recovered, shapes) = (rendered.count(SkipKind::Recovered), rendered.count(SkipKind::ShapeFailed));
        let shapes_u64 = u64::try_from(shapes).unwrap_or(u64::MAX);
        if recovered > 0 {
            self.frames_lost = self.frames_lost.saturating_add(1);
        }
        self.shapes_lost = self.shapes_lost.saturating_add(shapes_u64);
        self.unreported = self.unreported.saturating_add(u64::from(recovered > 0)).saturating_add(shapes_u64);
        if recovered > 0 {
            self.lost = Some((page, generation, LOST_FRAME.to_owned()));
        } else if shapes > 0 && lit == 0 {
            let which = if shapes == 1 { "the shape on it" } else { "any of its shapes" };
            let words = format!(
                "This page could not be drawn: DiaCraft could not draw {which}, because of an internal problem. \
                 Your drawing is not changed. Please report this bug."
            );
            self.lost = Some((page, generation, words));
        } else if self.lost.as_ref().is_some_and(|(p, g, _)| *p == page && *g == generation) {
            // A picture that drew is the answer to an earlier failure of the same page.
            self.lost = None;
        }
    }
}

/// Whether a picture drawn by a worker that `is_inline` is drawn on the window's own thread where that freezes the window: only on the web,
/// where the worker is inline because no thread could be had. (On other systems an inline worker is a test's, and draws within the
/// defaults, so that a slow machine does not change what a test sees.)
fn inline_limits_apply(is_inline: bool) -> bool {
    is_inline && cfg!(target_arch = "wasm32")
}

/// The limits a picture is drawn within. The picture at rest has the renderer's defaults; one asked for again (`attempt` above 0) has twice
/// the time for each asking before it, for the frame and for its text; one drawn on the window's own thread has [`INLINE_FRAME_TIME`] and
/// is not given more.
fn limits_for(rough: bool, on_the_ui_thread: bool, attempt: u8) -> Limits {
    let mut limits = Limits::default();
    if rough {
        limits.frame_time = Some(MOVING_FRAME_TIME);
        limits.text_time = Some(MOVING_TEXT_TIME);
    } else if on_the_ui_thread {
        limits.frame_time = Some(INLINE_FRAME_TIME);
    } else if attempt > 0 {
        let times = 1u32.checked_shl(u32::from(attempt)).unwrap_or(u32::MAX);
        limits.frame_time = limits.frame_time.map(|t| t.saturating_mul(times));
        limits.text_time = limits.text_time.map(|t| t.saturating_mul(times));
    }
    limits
}

/// What a request is: a picture while the view moves (`rough`), or one at rest, and which asking of the picture at rest it is.
#[derive(Clone, Copy)]
struct Ask {
    rough: bool,
    attempt: u8,
    /// Whether it makes the pictures still being drawn pointless: no picture on its way serves the view it is for.
    supersedes: bool,
}

/// Whether page `id` or a background page behind it has a shape.
pub fn has_content(doc: &Document, id: u32) -> bool {
    doc.page(id).is_some_and(|p| !p.shapes.is_empty()) || doc.background_chain(id).iter().any(|p| !p.shapes.is_empty())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn want() -> Want {
        Want {
            page: 1,
            page_rect: Rect::from_min_size(pos2(100.0, 80.0), vec2(1056.0, 816.0)),
            scale: 96.0,
            area: Rect::from_min_size(pos2(40.0, 60.0), vec2(800.0, 600.0)),
            ppp: 1.0,
            max_side: MAX_SIDE_PX,
        }
    }

    #[test]
    fn the_image_is_the_area_and_a_margin_on_whole_pixels() {
        let p = plan(&want()).unwrap();
        // 800 and 600 points, a quarter more on each side.
        assert_eq!((p.width, p.height), (1200, 900));
        assert!(p.covers.holds(&p.needed, 0.0), "the image covers what is needed: {p:?}");
        // The page's top left corner is at (100, 80) on the screen and the image starts at (-160, -90).
        assert_eq!(p.to_px[4], 100.0 + 160.0);
        assert_eq!(p.to_px[5], (80.0 + 816.0) + 90.0);
        // Paper y runs up and pixel y runs down.
        assert!(p.to_px[0] > 0.0 && p.to_px[3] < 0.0);
    }

    #[test]
    fn the_density_of_the_screen_makes_more_pixels_not_more_inches() {
        let one = plan(&want()).unwrap();
        let two = plan(&Want { ppp: 2.0, ..want() }).unwrap();
        assert_eq!((two.width, two.height), (one.width * 2, one.height * 2));
        assert!((two.covers.x1 - one.covers.x1).abs() < 1e-9 && (two.covers.y0 - one.covers.y0).abs() < 1e-9);
    }

    #[test]
    fn a_small_pan_stays_inside_the_image_and_a_large_one_does_not() {
        let p = plan(&want()).unwrap();
        let panned = |dx: f32| {
            let w = Want { page_rect: want().page_rect.translate(vec2(dx, 0.0)), ..want() };
            plan(&w).unwrap().needed
        };
        assert!(p.covers.holds(&panned(-150.0), 0.0), "a pan of 150 points is within the margin of 200");
        assert!(!p.covers.holds(&panned(-260.0), 0.0), "a pan of 260 points is not");
    }

    #[test]
    fn an_image_too_large_for_a_texture_gives_up_its_margin_and_then_its_edge() {
        // 6000 points at 1.5: 9000 pixels with no margin is past 8192, so the area is cut about its middle.
        let w = Want { area: Rect::from_min_size(pos2(0.0, 0.0), vec2(6000.0, 1000.0)), ppp: 1.5, ..want() };
        let p = plan(&w).unwrap();
        assert!(p.width as f32 <= MAX_SIDE_PX + 1.0 && p.height as f32 <= MAX_SIDE_PX);
        let w = Want { area: Rect::from_min_size(pos2(0.0, 0.0), vec2(5000.0, 3000.0)), ppp: 1.0, ..want() };
        let p = plan(&w).unwrap();
        assert_eq!((p.width, p.height), (5000, 3000), "past the pixel budget the margin goes");
    }

    /// A graphics back end whose textures are at most 2,048 pixels on a side (egui's default, a WebGL context, an old adapter) gets no image past it:
    /// the margin goes, then the edge of what is shown, and an area that cannot be cut to fit gets none.
    #[test]
    fn no_image_is_planned_past_the_back_ends_texture_limit() {
        for max_side in [2048.0_f32, 1024.0, 512.0] {
            for (width, height, ppp) in
                [(2200.0, 800.0, 1.0), (1000.0, 1000.0, 1.5), (3000.0, 3000.0, 1.0), (800.0, 600.0, 2.0), (1700.0, 700.0, 1.0)]
            {
                let w = Want { area: Rect::from_min_size(pos2(0.0, 0.0), vec2(width, height)), ppp, max_side, ..want() };
                if let Some(p) = plan(&w) {
                    assert!(
                        p.width as f32 <= max_side && p.height as f32 <= max_side,
                        "{max_side}: {width}x{height} at {ppp} gave {}x{}",
                        p.width,
                        p.height
                    );
                    assert!(p.covers.holds(&p.needed, 1e-9), "and it covers what it was cut to: {p:?}");
                }
            }
        }
        // A 1,700 point wide area at 1: past 2,048 with its margin, within it without: the margin goes and the area is covered whole.
        let w = Want { area: Rect::from_min_size(pos2(0.0, 0.0), vec2(1700.0, 700.0)), max_side: 2048.0, ..want() };
        let p = plan(&w).unwrap();
        assert_eq!((p.width, p.height), (1700, 700));
        // A limit that is no number, or under what two pixels of margin leave, plans nothing and never panics.
        assert!(plan(&Want { max_side: 1.0, ..want() }).is_none());
        assert!(plan(&Want { max_side: 0.0, ..want() }).is_none());
        assert!(plan(&Want { max_side: f32::NAN, ..want() }).is_some(), "an unknown limit leaves the fixed cap");
    }

    #[test]
    fn hostile_numbers_make_no_plan() {
        assert!(plan(&Want { scale: f64::NAN, ..want() }).is_none());
        assert!(plan(&Want { scale: 0.0, ..want() }).is_none());
        assert!(plan(&Want { ppp: 0.0, ..want() }).is_none());
        assert!(plan(&Want { page_rect: Rect::from_min_size(pos2(f32::NAN, 0.0), vec2(1.0, 1.0)), ..want() }).is_none());
        assert!(plan(&Want { area: Rect::from_min_size(pos2(0.0, 0.0), vec2(0.5, 0.5)), ..want() }).is_none());
    }

    #[test]
    fn coverage_holds_within_a_tolerance() {
        let a = Coverage { x0: 0.0, y0: 0.0, x1: 10.0, y1: 8.0 };
        assert!(a.holds(&Coverage { x0: 1.0, y0: 1.0, x1: 9.0, y1: 7.0 }, 0.0));
        assert!(!a.holds(&Coverage { x0: -0.1, y0: 1.0, x1: 9.0, y1: 7.0 }, 0.0));
        assert!(a.holds(&Coverage { x0: -0.1, y0: 1.0, x1: 9.0, y1: 7.0 }, 0.2));
    }

    #[test]
    fn a_picture_asked_for_again_has_twice_the_time_for_each_asking_and_the_web_thread_has_neither() {
        let at_rest = limits_for(false, false, 0);
        assert_eq!((at_rest.frame_time, at_rest.text_time), (Limits::default().frame_time, Limits::default().text_time));
        let (first, second) = (limits_for(false, false, 1), limits_for(false, false, 2));
        assert_eq!(first.frame_time, Some(Duration::from_millis(3_000)));
        assert_eq!(second.frame_time, Some(Duration::from_millis(6_000)));
        assert_eq!(second.text_time, Some(Duration::from_millis(1_600)));
        // A view in motion is rough whatever the asking, and the window's own thread is held to its short frame and given no more.
        assert_eq!(limits_for(true, false, 2).frame_time, Some(MOVING_FRAME_TIME));
        assert_eq!(limits_for(false, true, 2).frame_time, Some(INLINE_FRAME_TIME));
        assert_eq!(limits_for(false, true, 0).frame_time, Some(INLINE_FRAME_TIME));
    }

    /// The report said the web's inline limit applied, but the web is built in thread mode and falls back to an inline worker, so the
    /// mode never said inline. The worker is what says.
    #[test]
    fn the_inline_limit_follows_the_worker_that_was_got_not_the_mode_that_was_asked_for() {
        use crate::render_worker::{RenderWorker, bundled_drawer};
        let inline = RenderWorker::inline(bundled_drawer());
        let threaded = RenderWorker::spawn(Context::default(), bundled_drawer()).unwrap();
        assert!(inline.is_inline() && !threaded.is_inline());
        // Off the web an inline worker is a test's and keeps the defaults; on it, an inline worker is held to the short frame.
        assert_eq!(inline_limits_apply(inline.is_inline()), cfg!(target_arch = "wasm32"));
        assert!(!inline_limits_apply(threaded.is_inline()));
        // A Pictures started in thread mode whose thread could not be had has an inline worker, and says so.
        let mut pictures = Pictures::new();
        pictures.worker = Some(RenderWorker::inline(bundled_drawer()));
        assert_eq!(pictures.drawn_on_the_ui_thread(), cfg!(target_arch = "wasm32"));
    }
}
