//! One frame: the shapes of a page walked in drawing order, each drawn with its shadow, fill, outline and text.
//!
//! **Order**: the document's order is the display order, a later shape in front of an earlier one. A group's
//! own drawing sits behind its members at `DisplayMode` 1 and in front at 2, and is not drawn at 0.
//! Within a shape: shadow, fill, outline, text. The walk is iterative with an explicit stack, so a
//! group nested to the document's limit cannot overflow the stack.
//!
//! **Frames.** A shape's cells place it: minus the local pin, flipped, turned, plus the pin, in its
//! parent's frame; a member's parent is its group, so the steps repeat up the tree. The page's scale (`PageScale` over `DrawingScale`)
//! takes drawing units to paper inches at the end, and the view takes paper inches to pixels.
//! Line weights, text sizes and shadow offsets are on paper already and do not meet the scale.

use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Duration;

use diacraft_color::Rgba;
use diacraft_doc::{Member, Owner, ShapeRef, ShapeType, SheetRef};
use diacraft_fonts::FontDb;
use diacraft_geom::{combined_fill, kurbo::Size};
use kurbo::{Affine, BezPath, ParamCurveArclen, Rect, Shape as _};

use crate::clock::Clock;
use crate::corners;
use crate::custom_end::{self, Refusal, Sizing};
use crate::dash;
use crate::ends;
use crate::geometry::{self, ShapeGeometry};
use crate::gradient::{self, GradFill};
use crate::layers::{LayerColour, LayerTable, Verdict, members_of};
use crate::limits::{
    Cancel, DRAWN_PIXELS_PER_WORK, Limits, MAX_BLUR_PIXELS_PER_FRAME, MAX_BLUR_SPREAD_PX, MAX_BLURS_PER_FRAME, MAX_STROKE_PX, WORK_PER_DASH,
    WORK_PER_DRAWN_SHAPE, WORK_PER_END_ROW, WORK_PER_MARK, WORK_PER_SHAPE,
};
use crate::read::{Cx, sheet_of};
use crate::share::Tally;
use crate::skips::SkipKind;
use crate::style::{FillLook, GradientSpec, LineLook, Looks, SHADOW_BLUR_DEVIATIONS, ShadowKind, ShadowLook};
use crate::target::{Paint, Refused, Target};
use crate::text::Caches;

/// The deepest group the walk enters: `diacraft_doc::MAX_SHAPE_DEPTH`, the limit the document itself holds shapes to.
const MAX_DEPTH: usize = diacraft_doc::MAX_SHAPE_DEPTH;

/// What the walk of a page knows about the page it is on.
struct PageCtx {
    /// The page's sheet: its shadow defaults, its scale.
    sheet: SheetRef,
    /// Paper inches over drawing units (`s`).
    scale: f64,
    layers: LayerTable,
    master_layers: HashMap<u32, LayerTable>,
}

/// The flips of the shapes above a shape, one bit for each axis: a shape flipped on an axis toggles it, so two flips of one axis cancel.
///
/// A shape's own flip is cancelled for its text by its own text transform. A flip of a group above it has
/// no such second place to cancel in, so the walk carries these bits down and the text path cancels them in the block's frame
/// (`text.rs`; text is mirrored by none of the flips above it).
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub(crate) struct Flips {
    pub x: bool,
    pub y: bool,
}

impl Flips {
    /// These flips and then `other`'s.
    pub(crate) fn then(self, other: Flips) -> Flips {
        Flips { x: self.x != other.x, y: self.y != other.y }
    }

    /// The flips a shape's own cells say.
    pub(crate) fn of(cx: &Cx<'_, '_>, at: &SheetRef) -> Flips {
        Flips { x: cx.flag(at, "FlipX", false), y: cx.flag(at, "FlipY", false) }
    }
}

/// The work a frame has done, in the units of `Limits::max_work`, each piece charged before it was done, and whether the budget has said no: **once it
/// has, it says no to every charge after**, so the frame is cut at one point and no smaller piece of work slips in after it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct WorkMeter {
    done: u64,
    out: bool,
}

impl WorkMeter {
    /// The units charged and added so far.
    pub(crate) fn done(self) -> u64 {
        self.done
    }

    /// Charges `units` within a budget of `max` before the work is done: true when they fit what is left, and they are added. False when they do not, or the
    /// budget said no before: nothing is charged, and the meter says no from then on.
    pub(crate) fn charge(&mut self, units: u64, max: u64) -> bool {
        if self.out {
            return false;
        }
        match self.done.checked_add(units) {
            Some(done) if done <= max => {
                self.done = done;
                true
            }
            _ => {
                self.out = true;
                false
            }
        }
    }

    /// The budget has said no for a reason of its own (a decode its room cut short): every charge after says no.
    pub(crate) fn cut(&mut self) {
        self.out = true;
    }

    /// What is left of a budget of `max`.
    pub(crate) fn left(self, max: u64) -> u64 {
        max.saturating_sub(self.done)
    }

    /// Adds `units` of work that was done within what [`WorkMeter::left`] said was left: it never passes the budget, nor goes down.
    pub(crate) fn add(&mut self, units: u64, max: u64) {
        self.done = self.done.saturating_add(units).min(max.max(self.done));
    }
}

/// A step of the walk.
enum Work {
    /// Visit a shape: place it, and queue its members and its own drawing in order. `flips` are those of the groups above it.
    Enter { at: ShapeRef, parent: Affine, flips: Flips, depth: usize },
    /// Draw a shape's own geometry and text. `flips` are those of the groups above it, not its own.
    Draw { at: ShapeRef, affine: Affine, flips: Flips, depth: usize, verdict: Verdict },
}

pub(crate) struct Frame<'f, 'a, 'c, T: Target> {
    pub cx: &'f Cx<'a, 'c>,
    /// What the page is drawn on: the raster canvas, or an export's writer (`target.rs`).
    pub canvas: &'f mut T,
    pub fonts: &'f FontDb,
    pub caches: &'f mut Caches,
    pub limits: Limits,
    /// Where the frame reads the time (the renderer's [`Clock`]), and the reading it began at.
    pub(crate) clock: Clock,
    pub(crate) start: Duration,
    /// Pixels in a paper inch (the square root of the view's area scale).
    pub px_per_inch: f64,
    drawn: usize,
    rows: usize,
    /// Steps of the walk queued so far, nested ones counted: what a parallel walk adds up to know that no count could have stopped the frame.
    pushed: usize,
    /// Blurred shadows drawn so far.
    blurs: usize,
    /// Pixels the layers of those blurs hold, as counted by `Canvas::blur_layer_pixels`.
    blur_pixels: u64,
    pub text_chars: usize,
    pub text_time: Duration,
    /// What the labels of placeholders have used of their own budget (`Limits::max_label_chars`, `Limits::label_time`).
    pub label_chars: usize,
    pub label_time: Duration,
    /// Items of metafile pictures drawn so far in the frame (`limits::MAX_METAFILE_ITEMS_PER_FRAME`).
    pub meta_items: usize,
    /// Whether the frame's one forced decode of a metafile's bitmap is still to be used (`metafile.rs`, `DecodeRoom`).
    pub decode_forced_unused: bool,
    /// The work done so far, in the units of `Limits::max_work`, each charged before it was done ([`Frame::charge`]).
    work: WorkMeter,
    /// Set when the work budget ended the frame's drawing (and what it cut was counted as left out).
    worked_out: bool,
    /// Set when a budget ends the frame's drawing.
    stopped: bool,
    /// A newer request for a picture: the frame stops when it comes (checked with the budget, between shapes).
    pub cancel: Option<Cancel>,
    /// Set when `cancel` ended the frame's drawing.
    cancelled: bool,
    /// The walk is a print's: a shape with `NonPrinting` 1 is left off with its members, and a layer's `Print` counts and its `Visible` does
    /// not (`print.rs`).
    pub print: bool,
    /// This is a share of a parallel walk (`share.rs`): it has the cache and the counters of its own, so what depends on the shapes before it
    /// (a picture, which the frame's one forced decode and the kept pictures serve in order) is not done here. A share that meets such a shape sets
    /// [`Frame::bailed`] and stops, and the whole walk is done again on one thread.
    pub(crate) speculative: bool,
    /// Set by a speculative share that met something only the walk in order can draw.
    pub(crate) bailed: bool,
    /// What the shares of a parallel walk spend together, which each share adds to as it goes and which stops them all when it passes a budget.
    pub(crate) tally: Option<&'f Tally>,
    /// What the share has added to the tally so far, and how many steps ago it did.
    published: Counts,
    since_publish: u32,
}

/// How many steps of the walk a share takes between two times it says what it has spent to the tally.
const PUBLISH_EVERY: u32 = 8;

/// What a frame spent, for a parallel walk to add up and judge (`share.rs`).
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Counts {
    pub drawn: usize,
    pub rows: usize,
    pub pushed: usize,
    pub blurs: usize,
    pub blur_pixels: u64,
    pub text_chars: usize,
    pub label_chars: usize,
    /// Steps recorded, for a target that records (`Target::recorded`).
    pub nodes: usize,
    /// Units of work charged (`Limits::max_work`).
    pub work: u64,
    /// The time budget (or a count) ended the drawing.
    pub stopped: bool,
    /// A newer request ended it.
    pub cancelled: bool,
    pub bailed: bool,
}

impl<'f, 'a, 'c, T: Target> Frame<'f, 'a, 'c, T> {
    pub(crate) fn new(cx: &'f Cx<'a, 'c>, canvas: &'f mut T, fonts: &'f FontDb, caches: &'f mut Caches, limits: Limits) -> Self {
        let px_per_inch = canvas.to_px().determinant().abs().sqrt();
        let clock = Clock::system();
        Frame {
            cx,
            canvas,
            fonts,
            caches,
            limits,
            start: clock.now(),
            clock,
            px_per_inch,
            drawn: 0,
            rows: 0,
            pushed: 0,
            blurs: 0,
            blur_pixels: 0,
            text_chars: 0,
            text_time: Duration::ZERO,
            label_chars: 0,
            label_time: Duration::ZERO,
            meta_items: 0,
            decode_forced_unused: true,
            work: WorkMeter::default(),
            worked_out: false,
            stopped: false,
            cancel: None,
            cancelled: false,
            print: false,
            speculative: false,
            bailed: false,
            tally: None,
            published: Counts::default(),
            since_publish: 0,
        }
    }

    /// Notes that this share met what only the walk in order can draw, and stops the others: the page is walked in order.
    pub(crate) fn bail(&mut self) {
        self.bailed = true;
        if let Some(tally) = self.tally {
            tally.abort();
        }
    }

    /// Adds what the share has spent since it last said to the tally.
    pub(crate) fn publish(&mut self) {
        let Some(tally) = self.tally else { return };
        let now = self.counts();
        let before = self.published;
        tally.add(&Counts {
            drawn: now.drawn.saturating_sub(before.drawn),
            rows: now.rows.saturating_sub(before.rows),
            pushed: now.pushed.saturating_sub(before.pushed),
            blurs: now.blurs.saturating_sub(before.blurs),
            blur_pixels: now.blur_pixels.saturating_sub(before.blur_pixels),
            text_chars: now.text_chars.saturating_sub(before.text_chars),
            label_chars: now.label_chars.saturating_sub(before.label_chars),
            nodes: now.nodes.saturating_sub(before.nodes),
            work: now.work.saturating_sub(before.work),
            ..Counts::default()
        });
        self.published = now;
        self.since_publish = 0;
    }

    /// What the frame has spent.
    pub(crate) fn counts(&self) -> Counts {
        Counts {
            drawn: self.drawn,
            rows: self.rows,
            pushed: self.pushed,
            blurs: self.blurs,
            blur_pixels: self.blur_pixels,
            text_chars: self.text_chars,
            label_chars: self.label_chars,
            nodes: self.canvas.recorded(),
            work: self.work.done(),
            stopped: self.stopped,
            cancelled: self.cancelled,
            bailed: self.bailed,
        }
    }

    /// The narrowest stroke the target draws, in paper inches (0 for a target with none): its minimum in its own units over its units to a
    /// paper inch.
    pub(crate) fn hairline(&self) -> f64 {
        self.canvas.min_stroke() / self.px_per_inch.max(1.0e-9)
    }

    /// `PageScale` over `DrawingScale`, each read as 1 when missing, zero, negative or not a number.
    pub(crate) fn page_scale(&self, page: &SheetRef) -> f64 {
        let positive = |name: &str| {
            let n = self.cx.num(page, name);
            if n.is_finite() && n > 0.0 { n } else { 1.0 }
        };
        let s = positive("PageScale") / positive("DrawingScale");
        if s.is_finite() && s > 0.0 { s } else { 1.0 }
    }

    /// The paper of a page: its rectangle in paper inches filled with `color` (`PageWidth` and `PageHeight` are in drawing units).
    pub(crate) fn paper(&mut self, page_id: u32, color: Rgba) {
        let sheet = SheetRef::Page(page_id);
        let s = self.page_scale(&sheet);
        let (w, h) = (self.cx.num(&sheet, "PageWidth") * s, self.cx.num(&sheet, "PageHeight") * s);
        if !(w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0) {
            return;
        }
        let path = Rect::new(0.0, 0.0, w, h).to_path(0.1);
        if let Ok(true) = self.canvas.check(&path, 0.0) {
            self.canvas.fill(&path, &Paint::Solid(color), false);
        }
    }

    /// Whether the frame's time or count budget is spent.
    /// What the frame has left of its time, for work that is done between two looks at the clock (a metafile's bitmaps are decoded in one
    /// go). `None` is no limit.
    pub(crate) fn time_left(&self) -> Option<Duration> {
        self.limits.frame_time.map(|limit| limit.saturating_sub(self.clock.since(self.start)))
    }

    /// Charges `units` of work **before it is done** (`Limits::max_work`): true when they fit what is left, and the work may be done. When they do not,
    /// nothing is charged and the work is not done: the frame stops drawing at its next look at its budgets, and what was to be done for `who` is counted as left
    /// out. A share of a parallel walk that meets the budget bails, so that the walk in order cuts the frame where it would: the cut is a point the drawing sets.
    pub(crate) fn charge(&mut self, units: u64, who: &dyn Fn() -> String) -> bool {
        // Once the budget has said no, it says no to everything after (the meter): the frame is cut at one point, and no smaller piece of work slips in after it.
        if self.work.charge(units, self.limits.max_work) {
            return true;
        }
        self.out_of_work(who);
        false
    }

    /// The work budget has no room for what was to be done for `who`: it is counted as left out, the frame stops drawing at its next look at its budgets, and
    /// every charge after says no. A share of a parallel walk bails, so that the walk in order cuts the frame where it would.
    pub(crate) fn out_of_work(&mut self, who: &dyn Fn() -> String) {
        if self.worked_out {
            return;
        }
        self.work.cut();
        if self.speculative {
            self.bail();
        }
        self.cx.note(SkipKind::WorkLimit, who);
        self.worked_out = true;
        self.stopped = true;
    }

    /// The work of drawing a picture of `width` by `height` of its own pixels, placed on paper by `place`: the device pixels it can cover, the target's whole
    /// view at most, and 1 for the call.
    pub(crate) fn image_work(&self, width: f64, height: f64, place: Affine) -> u64 {
        let (vw, vh) = self.canvas.size();
        let covers = (self.canvas.to_px() * place).transform_rect_bbox(Rect::new(0.0, 0.0, width, height)).intersect(Rect::new(0.0, 0.0, vw, vh));
        let area = covers.width().max(0.0) * covers.height().max(0.0);
        // A box that is no number covers nothing the target draws (a placement past what it takes is refused before a picture is drawn).
        let area = if area.is_finite() { area } else { 0.0 };
        // At most the view's pixels, which the target caps, so the cast keeps it.
        (area / DRAWN_PIXELS_PER_WORK as f64).ceil() as u64 + 1
    }

    /// The work left before the budget, in its units.
    pub(crate) fn work_left(&self) -> u64 {
        self.work.left(self.limits.max_work)
    }

    /// Adds `units` of work that was done within what [`Frame::work_left`] said was left (a decode that its room held to it): it never passes the budget.
    pub(crate) fn add_work(&mut self, units: u64) {
        self.work.add(units, self.limits.max_work);
    }

    pub(crate) fn over_budget(&mut self) -> bool {
        if self.stopped {
            return true;
        }
        // A share says what it has spent every few steps, and stops when the shares together have spent a budget (or one of them bailed).
        if let Some(tally) = self.tally {
            self.since_publish += 1;
            if self.since_publish >= PUBLISH_EVERY {
                self.publish();
            }
            if tally.aborted() {
                self.stopped = true;
                return true;
            }
        }
        if self.drawn >= self.limits.max_shapes {
            self.stopped = true;
            return true;
        }
        if self.rows >= self.limits.max_rows {
            self.stopped = true;
            return true;
        }
        // A newer request makes the rest of this frame pointless. One atomic read: cheaper than the clock below.
        if self.cancel.as_ref().is_some_and(Cancel::is_stale) {
            self.stopped = true;
            self.cancelled = true;
            return true;
        }
        if let Some(limit) = self.limits.frame_time
            && self.clock.since(self.start) > limit
        {
            self.stopped = true;
            return true;
        }
        false
    }

    /// Draws the shapes of the page `page_id`.
    pub(crate) fn draw_page(&mut self, page_id: u32) {
        self.draw_page_part(page_id, None);
    }

    /// Draws the top shapes `part` of the page `page_id` (all of them for `None`), each with what is in it. A share of a parallel walk is
    /// the shapes of one range, in order, drawn in a frame of its own (`share.rs`).
    pub(crate) fn draw_page_part(&mut self, page_id: u32, part: Option<std::ops::Range<usize>>) {
        self.walk_part(page_id, part);
        // What was spent since the last time the share said, so that the tally holds every share's whole.
        self.publish();
    }

    fn walk_part(&mut self, page_id: u32, part: Option<std::ops::Range<usize>>) {
        let sheet = SheetRef::Page(page_id);
        #[cfg(test)]
        crate::fault::strike(self.cx, 0, crate::fault::At::Page);
        let scale = self.page_scale(&sheet);
        let mut ctx = PageCtx { layers: LayerTable::read(self.cx, &sheet), sheet, scale, master_layers: HashMap::new() };
        let owner = Owner::Page(page_id);
        let Some(top) = self.cx.res.shapes(owner) else {
            return;
        };
        let range = match part {
            Some(r) => r.start.min(top.len())..r.end.min(top.len()),
            None => 0..top.len(),
        };
        let mut stack: Vec<Work> = Vec::new();
        // The walk holds a step for each shape it has yet to visit, so the shapes it will visit are bounded by the frame's count before
        // they are queued (a page may hold a million, and a group of a million members costs 90 ms to list: the Task 4 review).
        let room = self.limits.max_shapes.saturating_sub(self.drawn);
        let wanted = range.len();
        if wanted > room {
            self.cx.note_n(SkipKind::ShapeLimit, wanted - room, || "shapes of the page past the count a frame draws".to_owned());
        }
        for i in (range.start..range.start + wanted.min(room)).rev() {
            if let Ok(i) = u32::try_from(i) {
                stack.push(Work::Enter { at: ShapeRef::top(owner, i), parent: Affine::IDENTITY, flips: Flips::default(), depth: 0 });
            }
        }
        self.pushed = self.pushed.saturating_add(stack.len());
        while let Some(work) = stack.pop() {
            if self.bailed {
                return;
            }
            match work {
                Work::Enter { at, parent, flips, depth } => {
                    if self.over_budget() {
                        self.note_left_out(stack.len() + 1);
                        return;
                    }
                    // A shape entered is work whether or not it is drawn: placing it reads its cells. One the budget has no room for is not
                    // entered, and the next look at the budget ends the walk.
                    let cx = self.cx;
                    if !self.charge(WORK_PER_SHAPE, &|| cx.res.shape(&at).map_or_else(|| "a shape".to_owned(), |s| format!("shape {}", s.id))) {
                        continue;
                    }
                    let place = at.clone();
                    self.contain(&place, |frame| frame.enter(&mut ctx, &mut stack, at, parent, flips, depth));
                }
                Work::Draw { at, affine, flips, depth, verdict } => {
                    if self.over_budget() {
                        self.note_left_out(stack.len() + 1);
                        return;
                    }
                    self.contain(&at, |frame| frame.draw_shape(&ctx, &at, affine, flips, depth, &verdict));
                }
            }
        }
    }

    fn note_left_out(&mut self, count: usize) {
        let kind = if self.worked_out {
            SkipKind::WorkLimit
        } else if self.drawn >= self.limits.max_shapes {
            SkipKind::ShapeLimit
        } else if self.rows >= self.limits.max_rows {
            SkipKind::RowLimit
        } else if self.cancelled {
            SkipKind::Cancelled
        } else {
            SkipKind::FrameBudget
        };
        self.cx.note_n(kind, count, || "the shapes after the last one drawn".to_owned());
    }

    /// Runs one step of one shape's drawing (placing it, or drawing it) so that a bug in it costs that shape and not the page.
    ///
    /// A panic is caught here, the clips and layers it left open on the canvas are closed, and the shape is counted as
    /// [`SkipKind::ShapeFailed`] with the bug's own words; what the shape drew before the panic stays, and the walk goes on with the next
    /// step. Nothing else needs undoing: a `RefCell` guard held by the panicking code is released as it unwinds (a `RefCell` has no
    /// poison), the counters of the frame are plain numbers, and a kept layout or outline is added in one step. The evaluator may have
    /// been cut off mid-cell, which a later read of the same cell asks again.
    fn contain(&mut self, at: &ShapeRef, step: impl FnOnce(&mut Self)) {
        let mark = self.canvas.mark();
        if let Err(payload) = catch_unwind(AssertUnwindSafe(|| step(self))) {
            self.canvas.rewind(mark);
            let bug = panic_message(payload.as_ref());
            // The shape's `ID` is looked up now, outside the step that failed.
            let id = self.cx.res.shape(at).map(|s| s.id);
            self.cx.note(SkipKind::ShapeFailed, || match id {
                Some(id) => format!("shape {id}: {bug}"),
                None => format!("a shape: {bug}"),
            });
        }
    }

    /// Places a shape and queues what is drawn for it, in order.
    fn enter(&mut self, ctx: &mut PageCtx, stack: &mut Vec<Work>, at: ShapeRef, parent: Affine, flips: Flips, depth: usize) {
        let cx = self.cx;
        let Some(shape) = cx.res.shape(&at) else {
            return;
        };
        let id = shape.id;
        #[cfg(test)]
        crate::fault::strike(cx, id, crate::fault::At::Enter);
        if shape.del {
            return;
        }
        // A guide is an editing aid and is not part of a drawing (the spec's list of what an export leaves out).
        if shape.kind == ShapeType::Guide {
            cx.note(SkipKind::Guide, || format!("shape {id}"));
            return;
        }
        if shape.master.is_some() && cx.res.master_step(&at).is_err() {
            cx.note(SkipKind::DanglingLink, || format!("shape {id}: a master that is not there"));
        }
        let sheet = sheet_of(&at);
        // A print leaves a non-printing shape off with everything in it.
        if self.print && cx.flag(&sheet, "NonPrinting", false) {
            return;
        }
        let affine = parent * local_transform(cx, &sheet);
        // What the members see above them: the flips above this shape, and this shape's own.
        let members_flips = flips.then(Flips::of(cx, &sheet));
        let verdict = if ctx.layers.is_empty() {
            Verdict::PLAIN
        } else {
            let members = members_of(cx, &sheet, &ctx.layers, &mut ctx.master_layers);
            ctx.layers.verdict(&members, self.print)
        };
        let mode = cx.num_or(&sheet, "DisplayMode", 2.0);
        // Members: only where there can be some (a group, a shape with a master or standing for a shape of one).
        let members = if !shape.children.is_empty() || shape.master.is_some() || shape.master_shape.is_some() {
            cx.res.effective_members(&at).unwrap_or_default()
        } else {
            Vec::new()
        };
        let mut members = members;
        let room = self.limits.max_shapes.saturating_sub(self.drawn).saturating_sub(stack.len());
        if members.len() > room {
            cx.note_n(SkipKind::ShapeLimit, members.len() - room, || format!("members of shape {id} past the count a frame draws"));
            members.truncate(room);
        }
        let own = || Work::Draw { at: at.clone(), affine, flips, depth, verdict };
        // A path is at most `MAX_DEPTH` long, so a shape at depth `d` has a path of `d + 1` and its members `d + 2`.
        let members_ok = depth + 2 <= MAX_DEPTH;
        if !members.is_empty() && !members_ok {
            cx.note_n(SkipKind::TooDeep, members.len(), || format!("members of shape {id}"));
        }
        // The stack pops the last pushed first: push what draws last first.
        let queued = stack.len();
        let draw_in_front = mode >= 1.5;
        let draw_behind = (0.5..1.5).contains(&mode);
        if draw_in_front {
            stack.push(own());
        }
        if members_ok {
            for member in members.into_iter().rev() {
                let (Member::Local(child) | Member::Omitted(child)) = member;
                stack.push(Work::Enter { at: child, parent: affine, flips: members_flips, depth: depth + 1 });
            }
        }
        if draw_behind {
            stack.push(own());
        }
        self.pushed = self.pushed.saturating_add(stack.len().saturating_sub(queued));
    }

    /// Draws one shape's own geometry and text.
    fn draw_shape(&mut self, page: &PageCtx, at: &ShapeRef, affine: Affine, flips: Flips, depth: usize, verdict: &Verdict) {
        if !verdict.drawn {
            return;
        }
        let cx = self.cx;
        let Some(shape) = cx.res.shape(at) else {
            return;
        };
        let id = shape.id;
        #[cfg(test)]
        crate::fault::strike(cx, id, crate::fault::At::Draw);
        let who = move || format!("shape {id}");
        let sheet = sheet_of(at);
        let (width, height) = (cx.num(&sheet, "Width"), cx.num(&sheet, "Height"));
        let to_paper = Affine::scale(page.scale) * affine;
        if self.is_off_view(to_paper, width, height) {
            return;
        }
        // Drawing it is work beside placing it.
        if !self.charge(WORK_PER_DRAWN_SHAPE, &who) {
            return;
        }
        self.drawn += 1;
        let rows_left = self.limits.max_rows.saturating_sub(self.rows);
        let Some(geom) = geometry::build(cx, &sheet, Size::new(width, height), rows_left, &who) else {
            self.rows = self.limits.max_rows;
            return;
        };
        self.rows += geom.rows;
        let sc = ShapeCtx { page, at: at.clone(), sheet, to_paper, depth, layer: verdict.colour };
        self.draw_geometry(&sc, &geom, &who);
        self.draw_object_placeholder(&sc, flips, page.scale, &who);
        self.draw_text(at, affine, flips, page.scale, sc.layer.as_ref(), &who);
    }

    /// Whether a shape's frame, with a margin, is wholly outside the view (a shortcut before building its geometry; paths that leave their
    /// frame by more than the margin are not drawn when the frame is off the view, which is rare).
    fn is_off_view(&self, to_paper: Affine, width: f64, height: f64) -> bool {
        let frame = Rect::new(0.0, 0.0, width, height).abs();
        let margin_paper = 3.0 + 3.0 * (width.abs().max(height.abs()) * to_paper.determinant().abs().sqrt());
        let det = to_paper.determinant().abs().sqrt().max(1.0e-12);
        let m = (margin_paper / det).min(1.0e12);
        let px = self.canvas.to_px().transform_rect_bbox(to_paper.transform_rect_bbox(frame.inflate(m, m)));
        if !(px.x0.is_finite() && px.y0.is_finite() && px.x1.is_finite() && px.y1.is_finite()) {
            // Not drawable: let `check` report it where the paths are.
            return false;
        }
        let view = self.canvas.pixel_rect();
        px.x1 < view.x0 || px.x0 > view.x1 || px.y1 < view.y0 || px.y0 > view.y1
    }

    fn draw_geometry(&mut self, sc: &ShapeCtx<'_>, geom: &ShapeGeometry, who: &dyn Fn() -> String) {
        let cx = self.cx;
        let layer = sc.layer.as_ref();
        let fill = Looks::fill(cx, &sc.sheet, layer, who);
        let line = Looks::line(cx, &sc.sheet, layer);
        let rounding = cx.num(&sc.sheet, "Rounding").max(0.0);
        // Fill ring and stroke path of the shape, in its own frame, corners rounded.
        let rounded = |p: &BezPath| if rounding > 0.0 { corners::round(p, rounding) } else { p.clone() };
        let no_fill = matches!(fill, FillLook::None);
        let fill_local = rounded(&combined_fill(geom.sections.iter().filter(|s| s.flags.paint(no_fill).fill).map(|s| (&s.fill, s.flags))));
        let mut stroke_local = BezPath::new();
        for section in geom.sections.iter().filter(|s| s.flags.paint(false).stroke) {
            stroke_local.extend(rounded(&section.stroke).elements().iter().copied());
        }
        let paths = Paths { fill: fill_local, stroke: stroke_local };
        // The shadow, behind the shape.
        if let Some(shadow) = Looks::shadow(cx, &sc.sheet, &sc.page.sheet, layer)
            && geom.has_geometry
            && (shadow.show >= 2 || sc.depth == 0)
        {
            self.draw_shadow(sc, &shadow, &paths, (&fill, line.as_ref()), who);
        }
        self.draw_fill(&fill, &paths.fill, sc.to_paper, who);
        self.draw_picture(sc, geom, who);
        if let Some(line) = &line {
            self.draw_line(line, &paths.stroke, (sc.to_paper, sc.page.scale), who);
        }
    }

    fn draw_fill(&mut self, fill: &FillLook, local: &BezPath, to_paper: Affine, who: &dyn Fn() -> String) {
        if local.is_empty() {
            return;
        }
        let paper = match self.canvas.bring_in(to_paper * local.clone(), false, 0.0) {
            Ok(Some((path, _))) => path,
            Ok(None) => return,
            Err(Refused::OutOfRange) => {
                self.cx.note(SkipKind::OutOfRange, || format!("{}: a fill", who()));
                return;
            }
        };
        match fill {
            FillLook::None => {}
            FillLook::Solid(color) => self.canvas.fill(&paper, &Paint::Solid(*color), true),
            FillLook::Gradient(spec) => self.draw_gradient(spec, local, &paper, to_paper, who),
            FillLook::Pattern { number, fore, back } => {
                // The lines of a hatch are work, counted before they are made.
                let marks = crate::pattern::marks(&*self.canvas, *number, &paper);
                if self.charge(marks.saturating_mul(WORK_PER_MARK), who) {
                    crate::pattern::draw(&mut *self.canvas, *number, *fore, *back, &paper);
                }
            }
        }
    }

    fn draw_gradient(&mut self, spec: &GradientSpec, local: &BezPath, paper: &BezPath, to_paper: Affine, who: &dyn Fn() -> String) {
        // The box and the frame: the shape's own when the gradient turns with it, the page's when not.
        let (frame_box, frame_to_paper) =
            if spec.rotate_with_shape { (local.control_box(), to_paper) } else { (paper.control_box(), Affine::IDENTITY) };
        if spec.dir >= 8 {
            self.cx.note(SkipKind::GradientApproximate, || format!("{}: gradient direction {}", who(), spec.dir));
        }
        match gradient::build(spec, frame_box, frame_to_paper) {
            GradFill::Flat(color) => self.canvas.fill(paper, &Paint::Solid(color), true),
            GradFill::Paint { paint, paint_transform } => self.canvas.fill_with_paint_transform(paper, &paint, true, paint_transform),
            GradFill::Bands { rects, unit_to_paper } => {
                self.canvas.push_clip(paper);
                for (rect, color) in rects {
                    let band = unit_to_paper * rect.to_path(0.1);
                    self.canvas.fill(&band, &Paint::Solid(color), false);
                }
                self.canvas.pop_clip();
            }
        }
    }

    /// The stroke of a shape's outline, `weight` wide on paper. `place` is the shape's frame to paper inches and the page's scale (paper inches to a drawing inch: what a scaled custom end
    /// follows).
    fn draw_line(&mut self, line: &LineLook, local: &BezPath, place: (Affine, f64), who: &dyn Fn() -> String) {
        let (to_paper, page_scale) = place;
        if local.is_empty() || line.color.a == 0 && line.gradient.is_none() {
            return;
        }
        let paper_full = to_paper * local.clone();
        let ppi = self.px_per_inch.max(1.0e-9);
        let mut weight = line.weight;
        if weight * ppi > MAX_STROKE_PX {
            weight = MAX_STROKE_PX / ppi;
            self.cx.note(SkipKind::StrokeClamped, || format!("{}: line weight {} inches", who(), line.weight));
        }
        // A line thinner than the target can show is drawn as wide as it can, and a weight of 0 is the thinnest line
        // there is; the weight itself stays what sizes the line ends and the dashes.
        let widths = dash::Widths::of(weight, self.hairline());
        let (paper, clipped) = match self.canvas.bring_in(paper_full, true, widths.drawn * ppi) {
            Ok(Some(brought)) => brought,
            Ok(None) => return,
            Err(Refused::OutOfRange) => {
                self.cx.note(SkipKind::OutOfRange, || format!("{}: an outline", who()));
                return;
            }
        };
        if line.compound != 0 {
            self.cx.note(SkipKind::CompoundApproximate, || format!("{}: compound line {}", who(), line.compound));
        }
        // Line ends: the shaft is shortened under each mark, and the marks are drawn once, solid, whatever the pattern.
        // A path that was cut to the view has its ends somewhere else, so it has no line ends.
        let wants_ends = (line.begin_arrow != 0 || line.end_arrow != 0) && !clipped;
        let ends = if wants_ends { self.plan_ends(line, &paper, (widths, page_scale), who) } else { None };
        let shaft = ends.as_ref().map_or(&paper, |e| &e.shaft);
        let length = if line.pattern > 1 { path_length(shaft) } else { 0.0 };
        let plan = dash::plan(widths, line.pattern, line.cap, length, ppi);
        // The dashes are work, counted before the stroke makes them.
        if !self.charge(plan.dashes.saturating_mul(WORK_PER_DASH), who) {
            return;
        }
        if plan.solid_for_count {
            self.cx.note(SkipKind::DashesTooMany, || format!("{}: pattern {} on a path {length:.1} inches long", who(), line.pattern));
        }
        if line.pattern == 254 {
            self.cx.note(SkipKind::CustomPattern, || format!("{}: line pattern 254", who()));
        }
        let paint = match &line.gradient {
            Some(spec) => match gradient::build(spec, paper.control_box(), Affine::IDENTITY) {
                GradFill::Paint { paint, .. } => paint,
                GradFill::Flat(c) => Paint::Solid(c),
                GradFill::Bands { rects, .. } => Paint::Solid(rects.first().map_or(line.color, |(_, c)| *c)),
            },
            None => Paint::Solid(line.color),
        };
        if !shaft.is_empty() {
            self.canvas.stroke(shaft, &plan.stroke, &paint);
        }
        if let Some(ends) = &ends {
            if !ends.fill.is_empty() {
                self.canvas.fill(&ends.fill, &paint, false);
            }
            if !ends.fill_even_odd.is_empty() {
                self.canvas.fill(&ends.fill_even_odd, &paint, true);
            }
            if !ends.strokes.is_empty() {
                let mark = kurbo::Stroke::new(widths.drawn).with_caps(kurbo::Cap::Round).with_join(kurbo::Join::Round);
                self.canvas.stroke(&ends.strokes, &mark, &paint);
            }
        }
    }

    /// The line ends of a stroke whose path is `paper` (paper inches): the marks of the numbers, and the custom ends drawn from their masters.
    fn plan_ends(&mut self, line: &LineLook, paper: &BezPath, sizes: (dash::Widths, f64), who: &dyn Fn() -> String) -> Option<ends::Ends> {
        let widths = sizes.0;
        // A master as the end that cannot be drawn is number 4 at the cell's size, and says so.
        let custom = |n: i64| if n == 254 { 4 } else { n };
        if [line.begin_arrow, line.end_arrow].iter().any(|n| ends::is_unmeasured(*n)) {
            self.cx.note(SkipKind::LineEndApproximate, || format!("{}: a line end whose box no source measures", who()));
        }
        // A size class outside 0 to 6 is drawn as class 2, and the frame says so. A size with no mark to size is nothing to say: number 0, and any
        // number no art is drawn for, draw nothing (the custom end is drawn as a master, or as 4).
        let bad_size = |number: i64, size: i64| ends::size_is_standin(size) && ends::is_drawn(custom(number));
        if bad_size(line.begin_arrow, line.begin_size) || bad_size(line.end_arrow, line.end_size) {
            self.cx.note(SkipKind::LineEndApproximate, || format!("{}: a line end size class outside 0 to 6, drawn as class 2", who()));
        }
        let begin = self.choose_end(line.begin_arrow, line.begin_use.as_ref(), line.begin_size, sizes, who);
        let end = self.choose_end(line.end_arrow, line.end_use.as_ref(), line.end_size, sizes, who);
        let request = ends::Request {
            begin: begin.number,
            end: end.number,
            begin_size: line.begin_size,
            end_size: line.end_size,
            weight: widths.weight,
            see_through: line.color.a < 255,
            begin_custom: begin.custom,
            end_custom: end.custom,
        };
        let planned = ends::plan(paper, &request);
        // A path with no direction anywhere (every segment of no length) can hold no mark, and the frame says why.
        self.cx.note_n(SkipKind::LineEndNoDirection, planned.directionless, || {
            format!("{}: a line end left off a path with no direction (every segment has no length)", who())
        });
        let mut ends = planned.ends?;
        // A custom end is sized by the master and not by a table: it is held to what a target takes, as any path is.
        let margin = widths.drawn * self.px_per_inch.max(1.0e-9);
        for path in [&mut ends.fill_even_odd, &mut ends.strokes] {
            if !path.is_empty() && matches!(self.canvas.check(path, margin), Err(Refused::OutOfRange)) {
                self.cx.note(SkipKind::OutOfRange, || format!("{}: a line end", who()));
                *path = BezPath::new();
            }
        }
        Some(ends)
    }

    /// What an end of a line is drawn as: its number, or a custom end drawn from its master. A custom end whose master cannot be drawn is number 4 and the frame says so
    /// One whose box has no height draws nothing and says nothing; one the work budget has no room for draws nothing.
    fn choose_end(
        &mut self,
        number: i64,
        used: Option<&std::sync::Arc<str>>,
        size: i64,
        sizes: (dash::Widths, f64),
        who: &dyn Fn() -> String,
    ) -> EndChoice {
        if number != 254 {
            return EndChoice { number, custom: None };
        }
        match self.custom_end(used, size, sizes, who) {
            Ok(custom) => EndChoice { number: 0, custom },
            Err(Refusal::Budget) => EndChoice { number: 0, custom: None },
            Err(_) => {
                self.cx.note(SkipKind::CustomLineEnd, || format!("{}: a master as a line end", who()));
                EndChoice { number: 4, custom: None }
            }
        }
    }

    /// The custom end `used` names, sized for a line of this weight at size class `size`: `Ok(None)` for an end that is not drawn (a weight of 0). The master's rows are counted before
    /// they are built and charged as work (a custom end is built again for each line that holds it), and a master past the caps costs nothing (`custom_end::survey`).
    fn custom_end(
        &mut self,
        used: Option<&std::sync::Arc<str>>,
        size: i64,
        sizes: (dash::Widths, f64),
        who: &dyn Fn() -> String,
    ) -> Result<Option<ends::CustomEnd>, Refusal> {
        let (widths, page_scale) = sizes;
        let master = used.and_then(|name| custom_end::master_named(self.cx.doc, name)).ok_or(Refusal::NotAnEnd)?;
        let survey = custom_end::survey(self.cx, master)?;
        if widths.weight <= 0.0 && !survey.scaled() {
            return Ok(None);
        }
        let units = (survey.shapes as u64).saturating_mul(WORK_PER_DRAWN_SHAPE).saturating_add((survey.rows as u64).saturating_mul(WORK_PER_END_ROW));
        if !self.charge(units, who) {
            return Err(Refusal::Budget);
        }
        let form = custom_end::build(self.cx, &survey, self.limits.max_rows.saturating_sub(self.rows))?;
        self.rows = self.rows.saturating_add(form.rows);
        match form.sizing(size, widths.weight, page_scale)? {
            Sizing::Nothing => Ok(None),
            Sizing::By(factor) => form.sized(factor).map(Some).ok_or(Refusal::Unusable),
        }
    }

    fn draw_shadow(&mut self, sc: &ShapeCtx<'_>, shadow: &ShadowLook, paths: &Paths, look: (&FillLook, Option<&LineLook>), who: &dyn Fn() -> String) {
        let cx = self.cx;
        let (fill, line) = look;
        let pin = (cx.num(&sc.sheet, "LocPinX"), cx.num(&sc.sheet, "LocPinY"));
        let local_shear = match shadow.kind {
            // A shear and squash that leaves the base line of the frame fixed.
            ShadowKind::Oblique => {
                let (sin, cos) = shadow.oblique.sin_cos();
                Affine::new([1.0, 0.0, shadow.scale * sin, shadow.scale * cos, 0.0, 0.0])
            }
            _ => Affine::IDENTITY,
        };
        let scale = if shadow.kind == ShadowKind::Oblique { 1.0 } else { shadow.scale };
        let pin_paper = sc.to_paper * kurbo::Point::new(pin.0, pin.1);
        // Scaled about the pin, then moved along the page's axes. The offset is on paper.
        let about_pin = Affine::translate((shadow.offset.0, shadow.offset.1))
            * Affine::translate(pin_paper.to_vec2())
            * Affine::scale(scale)
            * Affine::translate(-pin_paper.to_vec2());
        let place = about_pin * sc.to_paper * local_shear;
        if shadow.kind == ShadowKind::Inner {
            // Leaves an inner shadow open: it is drawn as a simple one.
            cx.note(SkipKind::ShadowApproximate, || format!("{}: an inner shadow drawn as a simple one", who()));
        }
        let ppi = self.px_per_inch.max(1.0e-9);
        // A blur is a Gaussian whose standard deviation is the cell over 1.5 (a project rule: SHADOW_BLUR_DEVIATIONS). Its reach,
        // three deviations, is what the rasteriser pads the layer by, and so what the layer costs: it is cut to MAX_BLUR_SPREAD_PX pixels.
        // An infinite cell is cut the same way (`f64::min` of an infinity is the other number), and a cell that is not a number was read as 0.
        let wanted = shadow.blur / SHADOW_BLUR_DEVIATIONS;
        let widest = MAX_BLUR_SPREAD_PX / (3.0 * ppi);
        let cut = wanted > widest;
        let sigma = wanted.min(widest);
        let reach_px = 3.0 * sigma * ppi;
        let fill_path = (!matches!(fill, FillLook::None) && !paths.fill.is_empty()).then(|| place * paths.fill.clone());
        let stroke_path = line.filter(|_| !paths.stroke.is_empty()).map(|line| {
            let widths = dash::Widths::of(line.weight.min(MAX_STROKE_PX / ppi), self.hairline());
            (place * paths.stroke.clone(), dash::plan(widths, 1, line.cap, 0.0, ppi).stroke)
        });
        let fill_path = fill_path.filter(|p| matches!(self.canvas.check(p, reach_px), Ok(true)));
        let stroke_path = stroke_path.filter(|(p, s)| matches!(self.canvas.check(p, s.width * ppi + reach_px), Ok(true)));
        if fill_path.is_none() && stroke_path.is_none() {
            return;
        }
        let paint = Paint::Solid(shadow.color);
        let draw = |canvas: &mut T| {
            #[cfg(test)]
            crate::fault::strike(cx, cx.res.shape(&sc.at).map_or(0, |s| s.id), crate::fault::At::InLayer);
            if let Some(p) = &fill_path {
                canvas.fill(p, &paint, true);
            }
            if let Some((p, s)) = &stroke_path {
                canvas.stroke(p, s, &paint);
            }
        };
        // What the layer would cost, counted against the frame's pixels: the rasteriser keeps every layer until the frame is finished. A target
        // that cannot blur (a PDF) pays nothing, and an SVG's filter is a cost to the viewer much as a raster layer is to us.
        let cost = if sigma > 0.0 {
            let boxes = fill_path.iter().map(|p| p.control_box()).chain(stroke_path.iter().map(|(p, s)| p.control_box().inflate(s.width, s.width)));
            let area = boxes.reduce(|a, b| a.union(b));
            area.map_or(0, |a| self.canvas.blur_layer_pixels(a, reach_px))
        } else {
            0
        };
        let room = self.blur_pixels.saturating_add(cost) <= MAX_BLUR_PIXELS_PER_FRAME;
        if sigma > 0.0 && sigma.is_finite() && self.blurs < MAX_BLURS_PER_FRAME && room && self.canvas.can_blur() {
            self.blurs += 1;
            self.blur_pixels = self.blur_pixels.saturating_add(cost);
            if cut {
                cx.note(SkipKind::ShadowApproximate, || {
                    let at = widest * SHADOW_BLUR_DEVIATIONS;
                    format!("{}: a blur of {} inches drawn as one of {at:.3} to bound its cost", who(), shadow.blur)
                });
            }
            self.canvas.blurred(sigma, draw);
        } else {
            if sigma > 0.0 {
                cx.note(SkipKind::ShadowApproximate, || format!("{}: a blurred shadow drawn hard-edged", who()));
            }
            draw(self.canvas);
        }
    }
}

/// What an end of a line is drawn as: the number of a mark, or a custom end drawn from its master in place of one (`plan_ends`).
struct EndChoice {
    number: i64,
    custom: Option<ends::CustomEnd>,
}

/// What the drawing of one shape needs to know about it.
pub(crate) struct ShapeCtx<'p> {
    page: &'p PageCtx,
    pub(crate) at: ShapeRef,
    pub(crate) sheet: SheetRef,
    /// The shape's frame to paper inches.
    pub(crate) to_paper: Affine,
    depth: usize,
    layer: Option<LayerColour>,
}

/// The fill ring and the stroke path of a shape, in its own frame.
struct Paths {
    fill: BezPath,
    stroke: BezPath,
}

/// The length of a path, in its own units.
fn path_length(path: &BezPath) -> f64 {
    path.segments().map(|seg| seg.arclen(1.0e-3)).sum()
}

/// A shape's transform to its parent's frame: minus the local pin, flipped, turned counterclockwise, plus the pin.
pub(crate) fn local_transform(cx: &Cx<'_, '_>, at: &SheetRef) -> Affine {
    let finite = |n: f64| if n.is_finite() { n } else { 0.0 };
    let (pin_x, pin_y) = (finite(cx.num(at, "PinX")), finite(cx.num(at, "PinY")));
    let (loc_x, loc_y) = (finite(cx.num(at, "LocPinX")), finite(cx.num(at, "LocPinY")));
    let angle = finite(cx.num(at, "Angle"));
    let flip_x = if cx.flag(at, "FlipX", false) { -1.0 } else { 1.0 };
    let flip_y = if cx.flag(at, "FlipY", false) { -1.0 } else { 1.0 };
    Affine::translate((pin_x, pin_y)) * Affine::rotate(angle) * Affine::scale_non_uniform(flip_x, flip_y) * Affine::translate((-loc_x, -loc_y))
}

/// The most characters of a panic's own words that a sample keeps.
const MAX_BUG_CHARS: usize = 200;

/// What a caught panic said: its message when it had one, cut short.
pub(crate) fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    let text = payload.downcast_ref::<&str>().map(|s| (*s).to_owned()).or_else(|| payload.downcast_ref::<String>().cloned());
    match text {
        Some(text) => text.chars().take(MAX_BUG_CHARS).collect(),
        None => "a panic with no message".to_owned(),
    }
}
