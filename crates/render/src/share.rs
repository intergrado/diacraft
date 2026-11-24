//! The walk of a page shared out among threads.
//!
//! Reading a page's cells to say what to draw costs much more than drawing it: on a page of thousands of shapes the walk is four fifths of
//! a frame. The top shapes of a page (and of the backgrounds behind it) are cut into **shares** of neighbouring shapes; each share is walked
//! on a thread, in a frame of its own, onto a [`Recorder`]; the recordings are joined in order and drawn after ([`crate::par`]).
//!
//! **The same frame as the walk in order.** A shape draws from its own cells, so the walk of one top shape does not depend on the shapes
//! before it except through what the frame counts and keeps: the budgets (shapes, rows, characters of text, blurred shadows, the pixels of
//! their layers, the steps of the evaluator), the kept laid-out text and outlines, the kept pictures, and the list of what was left out. A
//! share walks with every budget to itself, so the joined walk is the walk in order **only if no budget could have been reached**: the
//! counts are added up afterwards, and if any sum reaches a budget (or any share met a picture or a placeholder, whose laying out and
//! decoding are served in order and through caches that shapes share) the shares are thrown away and the page is walked in order on one
//! thread, as it always was. What is kept (a laid-out block is the same whoever laid it out) changes how long a share takes and nothing else.
//! The list of what was left out is joined in order: the counts add up and the first of a kind is the earliest.
//!
//! **The clock is the one thing that cannot be the same**: a frame that reaches its time budget stops at a shape that depends on the speed
//! of the machine at any thread count. A share that reaches it ends the frame there; the shares after it are dropped and counted as left out.
//!
//! A panic in a share is caught and takes the page back to the walk in order, which meets the same bug and is caught by the frame's net.

use std::ops::Range;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use diacraft_doc::{Document, Owner};
use kurbo::Affine;

use crate::frame::{Counts, Frame};
use crate::limits::{Cancel, Limits, MAX_BACKGROUNDS, MAX_BLUR_PIXELS_PER_FRAME, MAX_BLURS_PER_FRAME};
use crate::read::{Cx, THEME_STEPS, eval_ctx, eval_ctx_steps};
use crate::record::{Node, Recorder};
use crate::skips::{SkipKind, Skips};
use crate::text::Caches;
use crate::{PageId, Renderer};

/// Whether what the frame has spent, in all, leaves the walk in order its own course: no count could have ended the frame, nor changed a
/// choice the walk makes by counting (the cap on blurs, the characters of text), nor the room a shape's members are cut to (the frame's count
/// less the shapes drawn and the steps waiting, and the members are steps waiting: twice the steps and the shapes is more than enough).
/// **The walk in order spends no more than the shares' sum, so if the sum is within a budget it is too.**
pub(crate) fn within(limits: &Limits, c: &Counts, max_nodes: usize) -> bool {
    c.drawn.saturating_add(c.pushed.saturating_mul(2)) <= limits.max_shapes
        && c.rows < limits.max_rows
        && c.blurs <= MAX_BLURS_PER_FRAME
        && c.blur_pixels <= MAX_BLUR_PIXELS_PER_FRAME
        && c.text_chars <= limits.max_text_chars
        && c.label_chars <= limits.max_label_chars
        && c.nodes <= max_nodes
        && c.work <= limits.max_work
}

/// What the shares of a walk have spent together. **Each share says what it has spent every few steps, and when the sum passes a budget (or
/// a share bails) every share stops**: the shares do not each walk to the whole of a budget, so a page walked on N threads does at most
/// about one budget's work before it is walked in order, not N. The steps of the evaluator are cut into a part for each share instead.
pub(crate) struct Tally {
    limits: Limits,
    max_nodes: usize,
    drawn: AtomicUsize,
    rows: AtomicUsize,
    pushed: AtomicUsize,
    blurs: AtomicUsize,
    blur_pixels: AtomicU64,
    text_chars: AtomicUsize,
    label_chars: AtomicUsize,
    nodes: AtomicUsize,
    work: AtomicU64,
    stop: AtomicBool,
}

impl Tally {
    pub(crate) fn new(limits: Limits, max_nodes: usize) -> Tally {
        Tally {
            limits,
            max_nodes,
            drawn: AtomicUsize::new(0),
            rows: AtomicUsize::new(0),
            pushed: AtomicUsize::new(0),
            blurs: AtomicUsize::new(0),
            blur_pixels: AtomicU64::new(0),
            text_chars: AtomicUsize::new(0),
            label_chars: AtomicUsize::new(0),
            nodes: AtomicUsize::new(0),
            work: AtomicU64::new(0),
            stop: AtomicBool::new(false),
        }
    }

    /// Adds what a share spent, and stops every share if all of them together have passed a budget.
    pub(crate) fn add(&self, spent: &Counts) {
        let add =
            |cell: &AtomicUsize, n: usize| if n == 0 { cell.load(Ordering::Relaxed) } else { cell.fetch_add(n, Ordering::Relaxed).saturating_add(n) };
        let total = Counts {
            drawn: add(&self.drawn, spent.drawn),
            rows: add(&self.rows, spent.rows),
            pushed: add(&self.pushed, spent.pushed),
            blurs: add(&self.blurs, spent.blurs),
            blur_pixels: if spent.blur_pixels == 0 {
                self.blur_pixels.load(Ordering::Relaxed)
            } else {
                self.blur_pixels.fetch_add(spent.blur_pixels, Ordering::Relaxed).saturating_add(spent.blur_pixels)
            },
            text_chars: add(&self.text_chars, spent.text_chars),
            label_chars: add(&self.label_chars, spent.label_chars),
            nodes: add(&self.nodes, spent.nodes),
            work: if spent.work == 0 {
                self.work.load(Ordering::Relaxed)
            } else {
                self.work.fetch_add(spent.work, Ordering::Relaxed).saturating_add(spent.work)
            },
            ..Counts::default()
        };
        if !within(&self.limits, &total, self.max_nodes) {
            self.abort();
        }
    }

    /// Stops every share.
    pub(crate) fn abort(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    pub(crate) fn aborted(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    /// The shapes the shares have drawn so far.
    fn drawn_total(&self) -> usize {
        self.drawn.load(Ordering::Relaxed)
    }
}

/// The steps of the evaluator that each of `shares` shares may take: the frame's steps cut into equal parts, rounded up.
pub(crate) fn steps_for_each(shares: usize) -> u64 {
    THEME_STEPS.div_ceil(u64::try_from(shares.max(1)).unwrap_or(u64::MAX))
}

/// What a share walked: its recording and what it spent.
struct Done {
    nodes: Vec<Node>,
    skips: Skips,
    counts: Counts,
    /// The steps the share's evaluator took, and whether its budget ran out.
    steps: u64,
    exhausted: bool,
    /// How many top shapes the share had.
    tops: usize,
}

/// One range of one page's top shapes.
struct Unit {
    page: PageId,
    range: Range<usize>,
}

/// What every share needs to walk, which none of them changes.
struct Common<'a> {
    doc: &'a Document,
    fonts: &'a diacraft_fonts::FontDb,
    limits: Limits,
    cancel: Option<Cancel>,
    print: bool,
    /// The frame's clock, and the reading the frame began at.
    clock: crate::clock::Clock,
    start: std::time::Duration,
    width: u16,
    height: u16,
    to_px: Affine,
    /// What the shares have spent together, and the steps of the evaluator each may take.
    tally: &'a Tally,
    eval_steps: u64,
}

/// Walks one unit in a frame of its own.
fn walk_unit(common: &Common<'_>, unit: &Unit, caches: &mut Caches) -> Done {
    #[cfg(test)]
    crate::fault::strike_share(common.width, unit.range.start);
    let mut eval = eval_ctx_steps(common.eval_steps);
    let mut recorder = Recorder::new(common.width, common.height, common.to_px);
    let (skips, counts) = {
        let cx = Cx::new(common.doc, &mut eval);
        let counts = {
            let mut frame = Frame::new(&cx, &mut recorder, common.fonts, caches, common.limits);
            frame.clock = common.clock.clone();
            frame.start = common.start;
            frame.cancel = common.cancel.clone();
            frame.print = common.print;
            frame.speculative = true;
            frame.tally = Some(common.tally);
            frame.draw_page_part(unit.page, Some(unit.range.clone()));
            frame.counts()
        };
        (cx.take_skips(), counts)
    };
    Done { nodes: recorder.into_nodes(), skips, counts, steps: eval.stats.steps, exhausted: eval.stats.budget_exhausted, tops: unit.range.len() }
}

impl Renderer {
    /// The walk of `page` and its backgrounds onto `target`, on `threads` threads, **or `None` with `target` untouched** when the walk must
    /// be done in order (see the module documentation). `Some` is the walk in order's own result: the same steps written to `target`, the same
    /// list of what was left out.
    pub(crate) fn walk_shared(
        &mut self,
        doc: &Document,
        page: PageId,
        target: &mut Recorder,
        threads: usize,
        started: std::time::Duration,
    ) -> Option<Skips> {
        // The pages in the order they are drawn: the farthest background first, the page last.
        let chain = doc.background_chain(page);
        let mut pages: Vec<PageId> = chain.iter().take(MAX_BACKGROUNDS).rev().map(|p| p.id).collect();
        pages.push(page);
        // The top shapes of each, as the frame lists them.
        let units = {
            let mut eval = eval_ctx();
            let cx = Cx::new(doc, &mut eval);
            let mut units = Vec::new();
            for &id in &pages {
                let count = cx.res.shapes(Owner::Page(id)).map_or(0, |top| top.len());
                let shares = (count / self.par.min_share.max(1)).clamp(1, threads.saturating_mul(SHARES_PER_THREAD).max(1));
                for i in 0..shares {
                    // `i * count` cannot overflow: a page holds at most a million shapes and a share count is small.
                    let (from, to) = (i.saturating_mul(count) / shares, (i + 1).saturating_mul(count) / shares);
                    units.push(Unit { page: id, range: from..to });
                }
            }
            units
        };
        if units.len() < 2 {
            return None;
        }
        // The kept text of each share stays with the share, so that the same shapes find it again from one frame to the next.
        if self.share_caches.len() != units.len() {
            self.share_caches = (0..units.len()).map(|_| Caches::one_of(units.len())).collect();
        }
        let tally = Tally::new(self.limits, self.par.max_steps);
        let common = Common {
            doc,
            fonts: &self.fonts,
            limits: self.limits,
            cancel: self.cancel.clone(),
            print: self.print,
            clock: self.clock.clone(),
            start: started,
            width: target.width(),
            height: target.height(),
            to_px: crate::target::Target::to_px(target),
            tally: &tally,
            eval_steps: steps_for_each(units.len()),
        };
        let caches: Vec<Mutex<Caches>> = std::mem::take(&mut self.share_caches).into_iter().map(Mutex::new).collect();
        let slots: Vec<Mutex<Option<Done>>> = units.iter().map(|_| Mutex::new(None)).collect();
        let next = AtomicUsize::new(0);
        let work = || {
            loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= units.len() || tally.aborted() {
                    return;
                }
                let (Some(unit), Some(cache), Some(slot)) = (units.get(i), caches.get(i), slots.get(i)) else {
                    tally.abort();
                    return;
                };
                let mut cache = cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                let walked = catch_unwind(AssertUnwindSafe(|| walk_unit(&common, unit, &mut cache)));
                match walked {
                    Ok(done) => {
                        // A share that could not be walked here, or ended by a count, takes the others' work off: the page is walked in order.
                        if done.counts.bailed {
                            tally.abort();
                        }
                        *slot.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(done);
                    }
                    Err(_) => {
                        cache.clear();
                        tally.abort();
                    }
                }
            }
        };
        std::thread::scope(|scope| {
            let helpers = threads.min(units.len()).saturating_sub(1);
            let mut started = Vec::with_capacity(helpers);
            for _ in 0..helpers {
                // A thread that cannot be started leaves its share of the work to the others.
                if let Ok(handle) = std::thread::Builder::new().name("diacraft-walk".to_owned()).spawn_scoped(scope, work) {
                    started.push(handle);
                }
            }
            work();
            for handle in started {
                // A panic outside a share's own net is as a panic in it: the share's slot stays empty and the page is walked in order.
                if handle.join().is_err() {
                    tally.abort();
                }
            }
        });
        self.share_caches = caches.into_iter().map(|c| c.into_inner().unwrap_or_else(std::sync::PoisonError::into_inner)).collect();
        self.stats.share_shapes = self.stats.share_shapes.saturating_add(u64::try_from(tally.drawn_total()).unwrap_or(u64::MAX));
        // A budget passed, a bail or a bug: what the shares walked is dropped here, before the page is walked in order, so the two are never held together.
        if tally.aborted() {
            return None;
        }
        let mut dones = Vec::with_capacity(units.len());
        for slot in slots {
            dones.push(slot.into_inner().unwrap_or_else(std::sync::PoisonError::into_inner)?);
        }
        let dones = self.joined(dones, doc, page, target)?;
        Some(dones)
    }

    /// Judges the shares and joins them onto `target` (the paper first), or says `None` when the walk in order could have gone another way.
    fn joined(&mut self, dones: Vec<Done>, doc: &Document, page: PageId, target: &mut Recorder) -> Option<Skips> {
        let limits = self.limits;
        let mut total = Counts::default();
        let mut steps = 0u64;
        let mut tops = 0usize;
        for done in &dones {
            let c = &done.counts;
            // A theme answer is kept for the walk's frame, so a cell every shape reads (a page's) is asked once in order and once in each share,
            // and a share that could not get an answer notes it again: the counts would not be the walk in order's.
            if c.bailed || done.exhausted || done.skips.has(SkipKind::ThemeUnanswered) {
                return None;
            }
            total.drawn = total.drawn.saturating_add(c.drawn);
            total.rows = total.rows.saturating_add(c.rows);
            total.pushed = total.pushed.saturating_add(c.pushed);
            total.blurs = total.blurs.saturating_add(c.blurs);
            total.blur_pixels = total.blur_pixels.saturating_add(c.blur_pixels);
            total.text_chars = total.text_chars.saturating_add(c.text_chars);
            total.label_chars = total.label_chars.saturating_add(c.label_chars);
            total.nodes = total.nodes.saturating_add(c.nodes);
            total.work = total.work.saturating_add(c.work);
            steps = steps.saturating_add(done.steps);
            tops = tops.saturating_add(done.tops);
        }
        // A share the clock ended is no reason to walk again: the walk in order meets the clock at about the same shape. But a count is.
        if !(within(&limits, &total, self.par.max_steps) && steps < THEME_STEPS) {
            return None;
        }
        // The frame's first words: the paper, drawn on this thread.
        let mut skips = Skips::default();
        if let Some(paper) = self.paper {
            let mut eval = eval_ctx();
            let cx = Cx::new(doc, &mut eval);
            {
                let mut frame = Frame::new(&cx, target, &self.fonts, &mut self.caches, limits);
                frame.paper(page, paper);
            }
            skips.absorb(cx.take_skips());
        }
        // The shares in order, up to the first that the clock or a newer request ended.
        let ended = dones.iter().position(|d| d.counts.stopped || d.counts.cancelled);
        let mut after = 0usize;
        let mut cancelled = false;
        for (i, done) in dones.into_iter().enumerate() {
            match ended {
                Some(at) if i > at => after = after.saturating_add(done.tops),
                _ => {
                    cancelled |= done.counts.cancelled;
                    skips.absorb(done.skips);
                    target.append(done.nodes);
                }
            }
        }
        if after > 0 {
            let kind = if cancelled { SkipKind::Cancelled } else { SkipKind::FrameBudget };
            skips.note_n(kind, after, || "the shapes after the last one drawn".to_owned());
        }
        self.work_done = self.work_done.saturating_add(total.work);
        Some(skips)
    }
}

/// How many shares there are for each thread, so that a share that costs more does not leave the others waiting.
const SHARES_PER_THREAD: usize = 2;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_evaluators_steps_are_cut_into_parts_that_do_not_add_up_to_more_than_a_part_over_the_frames() {
        for shares in [1usize, 2, 7, 62, 64, 1000] {
            let each = steps_for_each(shares);
            let all = each * shares as u64;
            assert!(all >= THEME_STEPS && all < THEME_STEPS + shares as u64, "{shares} shares: {all} steps in all");
        }
        assert_eq!(steps_for_each(0), THEME_STEPS, "no shares is one");
        assert!(steps_for_each(62) < THEME_STEPS / 60, "a share of 62 does not have the frame's steps");
    }

    fn spent(drawn: usize, pushed: usize, blurs: usize) -> Counts {
        Counts { drawn, pushed, blurs, ..Counts::default() }
    }

    #[test]
    fn the_tally_stops_every_share_when_their_sum_passes_a_budget_and_not_before() {
        let limits = Limits { max_shapes: 100, max_rows: 1000, max_text_chars: 50, max_label_chars: 50, ..Limits::default() };
        let tally = Tally::new(limits, 1000);
        // Ten shares of 3 shapes and 4 steps each: 30 drawn and 40 steps, and `drawn + 2 * steps` is 110 at the last, past 100.
        for share in 0..10 {
            assert!(!tally.aborted(), "share {share}: not yet");
            tally.add(&spent(3, 4, 0));
        }
        assert!(tally.aborted(), "the shares together passed the count of shapes");
        // Blurs: 400 are allowed, the 401st is not.
        let tally = Tally::new(Limits::default(), usize::MAX);
        tally.add(&spent(0, 0, MAX_BLURS_PER_FRAME));
        assert!(!tally.aborted(), "400 blurs are the cap");
        tally.add(&spent(0, 0, 1));
        assert!(tally.aborted());
        // Characters of text, rows, recorded steps.
        let tally = Tally::new(limits, 1000);
        tally.add(&Counts { text_chars: 50, ..Counts::default() });
        assert!(!tally.aborted());
        tally.add(&Counts { text_chars: 1, ..Counts::default() });
        assert!(tally.aborted());
        let tally = Tally::new(limits, 10);
        tally.add(&Counts { nodes: 10, ..Counts::default() });
        assert!(!tally.aborted());
        tally.add(&Counts { nodes: 1, ..Counts::default() });
        assert!(tally.aborted());
        let tally = Tally::new(limits, 1000);
        tally.add(&Counts { rows: 1000, ..Counts::default() });
        assert!(tally.aborted(), "the rows stop the frame at the cap, as the walk's own count does");
    }

    #[test]
    fn a_share_that_cannot_be_walked_here_stops_the_others() {
        let tally = Tally::new(Limits::default(), usize::MAX);
        assert!(!tally.aborted());
        tally.abort();
        assert!(tally.aborted());
    }
}
