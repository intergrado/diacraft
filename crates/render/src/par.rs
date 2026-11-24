//! Drawing a frame on more than one thread.
//!
//! **Same pixels at any thread count.** A frame drawn on N threads is the frame drawn on one, byte for byte. The walk of a page is read once
//! and written down as a [`Recording`]; the rasteriser then draws bands of rows of it on separate threads, each in a context of its own
//! (`std::thread::scope`; nothing is shared but the recording, which is read only). **A band is not moved.** Every band is a context
//! of the frame's own coordinates, so a path is flattened to the very numbers the whole frame flattens it to (a context moved up by
//! whole pixels rounds the numbers differently, and 25 pixels of a 2,000-shape frame came out one step off in the probe that tried it); the
//! context is cut off below the band, and what it draws above the band is thrown away. A step that cannot reach the band is not drawn in it.
//! A blur is drawn whole in every band it reaches, in a context that goes on past the band by the blur's reach, so that its layer is the
//! one the frame makes.
//!
//! **The thread count is the caller's** ([`Parallel`]); [`default_threads`] is what an app takes when nobody says. On the web build
//! (`wasm32`) the count is always 1 and no thread is started.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use kurbo::Rect;

use crate::canvas::Canvas;
use crate::frame::panic_message;
use crate::limits::MAX_BLUR_PIXELS_PER_FRAME;
use crate::record::{Recording, replay, replay_within, touches};

/// The most threads a frame is shared out to, whatever the caller asks: the bound a save's and an open's count have (`diacraft_container::par::MAX_THREADS`).
pub const MAX_THREADS: usize = diacraft_container::par::MAX_THREADS;

/// The bytes a step of a recording holds, about (`record.rs`; [`MAX_RECORDED_STEPS`] of them are some 140 MB).
const STEP_BYTES: u64 = 560;

/// The height of a tile of the rasteriser, in rows: a band starts at a multiple of it.
const TILE_ROWS: u32 = 4;

/// The fewest rows a band has: a narrower one spends more on drawing what it shares with the bands above it than it saves.
const MIN_BAND_ROWS: u32 = 64;

/// How many bands there are for each thread, so that a band that costs more does not leave the others waiting.
const BANDS_PER_THREAD: usize = 1;

/// How a frame may be shared out among threads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Parallel {
    /// The threads that draw a frame, the calling thread among them, from 1 to [`MAX_THREADS`]. **1 draws as the renderer always did, on the
    /// calling thread alone.** The web build always draws on one.
    pub threads: usize,
    /// The fewest shapes (of a page and the backgrounds it shows, the top ones) a frame must have to be shared out: below it the threads cost
    /// more than they save.
    pub min_shapes: usize,
    /// The fewest top shapes a share of the walk has (`share.rs`): a smaller share costs more to start than it saves.
    pub min_share: usize,
    /// The fewest pixels a frame must have for its rasterising to be shared out in bands: a smaller one is drawn on the thread that walked it.
    pub min_pixels: u64,
    /// The most bytes of pixels the threads that draw the bands of a frame hold between them, besides the frame's own: each holds the rows
    /// of its band and all the rows above it. A frame so large that two would not fit is drawn by one thread.
    pub scratch_bytes: u64,
    /// The most pixels the blur layers of the bands drawn at once may hold together (`vello_cpu` keeps a context's layers until it has
    /// drawn): the cap one frame's layers are held to on one thread ([`MAX_BLUR_PIXELS_PER_FRAME`]).
    pub blur_pixels: u64,
    /// The most steps the recordings of the shares of a frame may hold together (`record.rs`): past it the shares stop and the walk is done in
    /// order onto the canvas, which holds no recording.
    pub max_steps: usize,
}

impl Parallel {
    /// One thread: the renderer's own way.
    pub const SINGLE: Parallel = Parallel {
        threads: 1,
        min_shapes: 0,
        min_share: 1,
        min_pixels: 0,
        scratch_bytes: MAX_BAND_SCRATCH_BYTES,
        blur_pixels: MAX_BLUR_PIXELS_PER_FRAME,
        max_steps: MAX_RECORDED_STEPS,
    };

    /// `threads` threads (cut to 1 and [`MAX_THREADS`]), for the frames big enough to be worth it.
    pub fn with_threads(threads: usize) -> Parallel {
        Parallel {
            threads: threads.clamp(1, MAX_THREADS),
            min_shapes: DEFAULT_MIN_SHAPES,
            min_share: DEFAULT_MIN_SHARE,
            min_pixels: DEFAULT_MIN_PIXELS,
            ..Parallel::SINGLE
        }
    }

    /// `threads` threads for every frame, however small: for a test that must see the shared-out drawing on a small page.
    pub fn eager(threads: usize) -> Parallel {
        Parallel { threads: threads.clamp(1, MAX_THREADS), ..Parallel::SINGLE }
    }

    /// The threads a frame uses now: `threads` cut to [`MAX_THREADS`], to 1 on the web build, and to what the process's address space has room for
    /// when it is limited (`ulimit -v`: each thread takes the address space of its stack, of its allocator's arena and of the heaps the arena keeps after the thread
    /// is gone, and a thread that cannot have it is an abort, not a refusal, and so is a save after the frame that finds the space gone). **The rule a save and an open
    /// are held by** (`diacraft_container::spare::threads_fit`): the threads whose cost fits in an eighth of what the limit leaves beside the address space
    /// in use and what the threads of the frame may hold ([`Parallel::room`]).
    pub fn usable_threads(&self) -> usize {
        if cfg!(target_arch = "wasm32") {
            return 1;
        }
        let wanted = self.threads.clamp(1, MAX_THREADS);
        if wanted == 1 { 1 } else { diacraft_container::spare::threads_fit(wanted, self.room()) }
    }

    /// The most bytes the threads of a frame hold between them, which [`Parallel::usable_threads`] leaves room for: the rows of the bands' contexts
    /// ([`Parallel::scratch_bytes`]), the layers of their blurs ([`Parallel::blur_pixels`], four bytes a pixel) and the recordings of the shares
    /// ([`Parallel::max_steps`], about 560 bytes a step). About 400 MB for a default [`Parallel`].
    pub fn room(&self) -> u64 {
        let steps = u64::try_from(self.max_steps).unwrap_or(u64::MAX);
        self.scratch_bytes.saturating_add(self.blur_pixels.saturating_mul(4)).saturating_add(steps.saturating_mul(STEP_BYTES))
    }
}

/// How often frames have been shared out ([`Renderer::par_stats`](crate::Renderer::par_stats)): counts of work done.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ParStats {
    /// Frames whose walk was cut into shares and walked on several threads.
    pub walks_shared: u64,
    /// Frames drawn on several threads whose walk had to be done in order on one, onto the canvas as on one thread: a page that holds a picture
    /// or a placeholder, one that reached a budget or a cap on steps, a share that met a bug.
    pub walks_in_order: u64,
    /// Frames whose walk was shared out and whose pixels were drawn in bands (a frame walked in order has no bands).
    pub frames_banded: u64,
    /// The most threads that drew the bands of one frame at once (1 for a frame drawn by one): fewer than were asked for when the pixels of the
    /// bands' contexts or of their blur layers would not fit the budgets of the frame.
    pub most_band_threads: usize,
    /// The shapes the shares of walks have drawn, all frames added, **those of a walk that was thrown away included**: a count of work done, for a
    /// test to see that the shares of a page that passes a budget stop together and do not each walk to the whole of it.
    pub share_shapes: u64,
    /// The shapes the walks done in order have drawn, all frames added (those on one thread included): with `share_shapes`, what a frame's walks
    /// cost, for a test to see that a frame is walked once past a cap and not again.
    pub walked_shapes: u64,
    /// The time the walks of those frames took, and the time their pixels took to draw, all frames added: for a status line, and for a person
    /// who measures. Never compared by a test.
    pub walk_time: std::time::Duration,
    pub raster_time: std::time::Duration,
    /// The time the shares of walks that were given up spent before the walk was done in order (the cost of trying to share a page that cannot be).
    pub abandoned_time: std::time::Duration,
}

impl Default for Parallel {
    fn default() -> Parallel {
        Parallel::SINGLE
    }
}

/// The most bytes of pixels the threads of a frame's bands hold between them (a default [`Parallel`]): 128 MiB, so a window's frame (9 MiB) is
/// drawn by fifteen threads, a 4K screen's (33 MiB) by three, and a frame past 64 MiB by one.
pub const MAX_BAND_SCRATCH_BYTES: u64 = 128 * 1024 * 1024;

/// The most steps a default [`Parallel`] writes down for one frame: 250,000, about 140 MB (a step is some 550 bytes), against the kilobyte the
/// rasteriser keeps for each fill it is given (`crate::limits::MAX_METAFILE_ITEMS_PER_FRAME`). A frame of thousands of shapes is well within it (a
/// page of 12,000 shapes writes some 80,000 steps); one past it is drawn onto the canvas from the step that passes it, which frees the recording.
pub const MAX_RECORDED_STEPS: usize = 250_000;

/// The fewest top shapes a default [`Parallel`] shares a frame out for.
pub const DEFAULT_MIN_SHAPES: usize = 200;

/// The fewest top shapes in a share of the walk of a default [`Parallel`].
pub const DEFAULT_MIN_SHARE: usize = 16;

/// The fewest pixels a frame has for a default [`Parallel`] to draw them in bands: 512 by 512.
pub const DEFAULT_MIN_PIXELS: u64 = 512 * 512;

/// The threads to use when the caller does not say: the environment's `DIACRAFT_THREADS` when it is a whole number (cut to 1 and
/// [`MAX_THREADS`]; 1 is the single-threaded way), else the machine's cores less one, which leaves one for the window. 1 on the web build.
/// **The count a save and an open take** (`diacraft_container::par::default_threads`, which this is).
pub fn default_threads() -> usize {
    diacraft_container::par::default_threads()
}

/// [`default_threads`] from what it reads: the environment's text, if it is set, and the machine's core count (`diacraft_container::par::from_setting`, which this is).
pub fn threads_from(env: Option<&str>, cores: usize) -> usize {
    diacraft_container::par::from_setting(env, cores)
}

/// The rows of a frame of `height` rows, cut into bands for `threads` threads: `(first, end)` pairs in order that cover every row once.
/// Each band but the last starts at a multiple of the rasteriser's tile height and has at least [`MIN_BAND_ROWS`] rows; a frame too
/// short for two bands is one band.
pub(crate) fn split_rows(height: u16, threads: usize) -> Vec<(u16, u16)> {
    let height = u32::from(height);
    let wanted = u32::try_from(threads.saturating_mul(BANDS_PER_THREAD)).unwrap_or(u32::MAX).max(1);
    let each = height.div_ceil(wanted).max(MIN_BAND_ROWS).div_ceil(TILE_ROWS) * TILE_ROWS;
    let mut bands = Vec::new();
    let mut first = 0u32;
    while first < height {
        let end = (first + each).min(height);
        // `end` is at most `height`, which came from a `u16`.
        if let (Ok(a), Ok(b)) = (u16::try_from(first), u16::try_from(end)) {
            bands.push((a, b));
        }
        first = end;
    }
    bands
}

/// Draws a recording to pixels, on `threads` threads. The bytes are premultiplied RGBA, row by row from the top, and they are the bytes
/// the recording gives when it is replayed onto one canvas ([`replay`]), on `usable` threads at most; the count is how many threads drew it at once (1 for a frame that
/// was not drawn in bands: a small one, or one whose bands would not fit the budgets of [`Parallel`] at more than one at a time). A panic in a
/// band is returned as words.
pub(crate) fn raster(rec: &Recording, par: Parallel, usable: usize) -> Result<(Vec<u8>, usize), String> {
    let small = u64::from(rec.width) * u64::from(rec.height) < par.min_pixels;
    let wanted = if small { 1 } else { usable };
    let bands = split_rows(rec.height, wanted);
    // The reach of the blurs, in rows, past which a band's context need not go.
    let reach = rec.blur_reach();
    let reach_rows = if reach.is_finite() { (reach.min(f64::from(u16::MAX)).ceil() as u32).div_ceil(TILE_ROWS) * TILE_ROWS } else { 0 };
    // What the threads may hold at once: the rows of each band's context, and the layers of the blurs each band meets.
    let boxes = rec.blur_boxes();
    let most_layer_pixels = bands.iter().map(|&band| layer_pixels(&boxes, band, reach_rows, rec.width, rec.height)).max().unwrap_or(0);
    let frame_bytes = u64::from(rec.width) * u64::from(rec.height) * 4;
    let threads = band_threads(wanted, bands.len(), frame_bytes, most_layer_pixels, &par);
    if threads <= 1 || bands.len() < 2 {
        let mut canvas = Canvas::new(rec.width, rec.height, rec.to_px);
        replay(&rec.nodes, &mut canvas);
        return catch_unwind(AssertUnwindSafe(|| canvas.finish())).map(|pixels| (pixels, 1)).map_err(|p| band_words(p.as_ref()));
    }
    let row_bytes = usize::from(rec.width) * 4;
    let mut out = vec![0u8; row_bytes.saturating_mul(usize::from(rec.height))];
    // The band's own rows of the result, each behind a lock a worker takes once.
    let mut slots: Vec<Mutex<&mut [u8]>> = Vec::with_capacity(bands.len());
    let mut rest = out.as_mut_slice();
    for &(first, end) in &bands {
        let (mine, after) = rest.split_at_mut(row_bytes.saturating_mul(usize::from(end - first)));
        slots.push(Mutex::new(mine));
        rest = after;
    }
    let next = AtomicUsize::new(0);
    let failed = AtomicBool::new(false);
    let work = || -> Result<(), String> {
        // The pixels of a band's context, kept from band to band by the thread that draws them: the first is the biggest.
        let mut scratch: Vec<u8> = Vec::new();
        loop {
            let i = next.fetch_add(1, Ordering::Relaxed);
            if i >= bands.len() || failed.load(Ordering::Relaxed) {
                return Ok(());
            }
            // The lowest band first: the context of a band reaches up to the top, so the lower the band the more it costs.
            let at = bands.len() - 1 - i;
            let done = band(rec, bands.get(at).copied(), reach_rows, slots.get(at), &mut scratch);
            if let Err(words) = done {
                failed.store(true, Ordering::Relaxed);
                return Err(words);
            }
        }
    };
    let results: Vec<Result<(), String>> = std::thread::scope(|scope| {
        let helpers = threads.min(bands.len()).saturating_sub(1);
        let mut started = Vec::with_capacity(helpers);
        for _ in 0..helpers {
            // A thread that cannot be started leaves its share to the others.
            if let Ok(handle) = std::thread::Builder::new().name("diacraft-band".to_owned()).spawn_scoped(scope, work) {
                started.push(handle);
            }
        }
        let mut results = vec![work()];
        for handle in started {
            results.push(handle.join().unwrap_or_else(|p| Err(band_words(p.as_ref()))));
        }
        results
    });
    if let Some(words) = results.into_iter().find_map(Result::err) {
        return Err(words);
    }
    drop(slots);
    Ok((out, threads))
}

/// The pixels of the blur layers a band's context holds: those of the blurs that reach the band, each cut to the context (which is the rows
/// from the top to the band's end and the blurs' reach past it).
fn layer_pixels(boxes: &[Rect], band: (u16, u16), reach_rows: u32, width: u16, height: u16) -> u64 {
    let (first, end) = band;
    let rows = (u32::from(end) + reach_rows).min(u32::from(height));
    let reaches = Rect::new(f64::NEG_INFINITY, f64::from(first), f64::INFINITY, f64::from(end));
    let context = Rect::new(0.0, 0.0, f64::from(width), f64::from(rows));
    boxes
        .iter()
        .filter(|b| touches(b, &reaches))
        .map(|b| {
            let r = b.intersect(context);
            // A box with a side that is not a number, or no area, holds no pixel (a float cast saturates, and a NaN is 0).
            (r.width().max(0.0) * r.height().max(0.0)) as u64
        })
        .fold(0u64, u64::saturating_add)
}

/// How many threads draw the bands of a frame at once: the `wanted` count, cut to the bands there are, and so that **the threads together hold
/// no more than the frame is allowed**. Each holds the pixels of its context, up to the whole frame's (`frame_bytes`), against
/// [`Parallel::scratch_bytes`]; each holds the layers of the blurs its band meets (`layer_pixels` the most any band's), against
/// [`Parallel::blur_pixels`], the cap one frame's layers are held to on one thread. At least 1.
pub(crate) fn band_threads(wanted: usize, bands: usize, frame_bytes: u64, layer_pixels: u64, par: &Parallel) -> usize {
    let fit = |budget: u64, each: u64| budget.checked_div(each).map_or(usize::MAX, |n| usize::try_from(n.max(1)).unwrap_or(usize::MAX));
    wanted.min(bands).min(fit(par.scratch_bytes, frame_bytes)).min(fit(par.blur_pixels, layer_pixels)).max(1)
}

/// Draws one band of a recording into its rows of the result.
fn band(rec: &Recording, rows: Option<(u16, u16)>, reach_rows: u32, slot: Option<&Mutex<&mut [u8]>>, scratch: &mut Vec<u8>) -> Result<(), String> {
    let (Some((first, end)), Some(slot)) = (rows, slot) else {
        return Err("a band of the frame that is not there".to_owned());
    };
    let drawn = catch_unwind(AssertUnwindSafe(|| {
        #[cfg(test)]
        crate::fault::strike_band(rec.width, first);
        // The context goes on past the band by the blurs' reach and stops there.
        let context = u16::try_from((u32::from(end) + reach_rows).min(u32::from(rec.height))).unwrap_or(rec.height);
        let mut canvas = Canvas::new(rec.width, context, rec.to_px);
        let within = Rect::new(f64::NEG_INFINITY, f64::from(first), f64::INFINITY, f64::from(end));
        replay_within(&rec.nodes, Some(within), &mut canvas);
        canvas.finish_rows_into(end, scratch)
    }));
    let made = drawn.map_err(|p| band_words(p.as_ref()))?.ok_or_else(|| "a band of the frame that could not be drawn".to_owned())?;
    let row_bytes = usize::from(rec.width) * 4;
    let from = row_bytes.saturating_mul(usize::from(first));
    let want = row_bytes.saturating_mul(usize::from(end - first));
    let mut mine = slot.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    match scratch.get(..made).and_then(|pixels| pixels.get(from..from.saturating_add(want))) {
        Some(rows) if rows.len() == mine.len() => {
            mine.copy_from_slice(rows);
            Ok(())
        }
        _ => Err("a band of the frame that came out the wrong size".to_owned()),
    }
}

/// A panic in a band, in words.
fn band_words(payload: &(dyn std::any::Any + Send)) -> String {
    format!("a band of the frame met a bug: {}", panic_message(payload))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_threads_come_from_the_environment_first_then_the_cores_less_one() {
        assert_eq!(threads_from(None, 32), 31);
        assert_eq!(threads_from(None, 1), 1, "a single core still draws");
        assert_eq!(threads_from(None, 0), 1);
        assert_eq!(threads_from(None, 500), MAX_THREADS);
        assert_eq!(threads_from(Some("1"), 32), 1);
        assert_eq!(threads_from(Some(" 7 "), 32), 7);
        assert_eq!(threads_from(Some("0"), 32), 1, "0 is not a count: the single-threaded way");
        assert_eq!(threads_from(Some("9999"), 32), MAX_THREADS);
        assert_eq!(threads_from(Some("many"), 4), 3, "text that is not a number is not an override");
        assert_eq!(threads_from(Some("-2"), 4), 3);
    }

    #[test]
    fn the_bands_cover_every_row_once_from_tile_edges() {
        for height in [1u16, 3, 4, 31, 32, 33, 63, 64, 65, 100, 1199, 1200, 4097, 16_384] {
            for threads in [1usize, 2, 3, 7, 31, 64] {
                let bands = split_rows(height, threads);
                let mut at = 0u16;
                for (i, &(first, end)) in bands.iter().enumerate() {
                    assert_eq!(first, at, "height {height}, {threads} threads: a gap or an overlap at band {i}");
                    assert!(end > first);
                    if i + 1 < bands.len() {
                        assert_eq!(first % 4, 0);
                        assert!(end - first >= 64, "a band of {} rows", end - first);
                        assert_eq!(end % 4, 0, "a band ends on a tile edge");
                    }
                    at = end;
                }
                assert_eq!(at, height, "height {height}, {threads} threads: rows left over");
            }
        }
        assert!(split_rows(0, 4).is_empty());
    }

    #[test]
    fn the_threads_of_a_frames_bands_are_held_to_the_scratch_and_the_blur_budgets() {
        let par = Parallel { scratch_bytes: 256 << 20, blur_pixels: 32 << 20, ..Parallel::eager(31) };
        let mib = |n: u64| n << 20;
        assert_eq!(band_threads(31, 62, mib(8), 0, &par), 31, "a window's frame and no blur: every thread asked for");
        assert_eq!(band_threads(31, 10, mib(8), 0, &par), 10, "never more threads than bands");
        assert_eq!(band_threads(31, 62, mib(100), 0, &par), 2, "256 MiB holds two frames of 100 MiB");
        assert_eq!(band_threads(31, 62, mib(256), 0, &par), 1, "a frame of the biggest size is one thread's");
        assert_eq!(band_threads(31, 62, mib(300), 0, &par), 1, "and one past it is still drawn");
        assert_eq!(band_threads(31, 62, mib(8), 10 << 20, &par), 3, "32 million pixels of layers hold three bands' of 10 million");
        assert_eq!(band_threads(31, 62, mib(8), 33 << 20, &par), 1, "a band whose layers pass the cap on their own is drawn alone");
        assert_eq!(band_threads(31, 62, mib(8), 1, &par), 31, "a few pixels of layers leave every thread");
        assert_eq!(band_threads(0, 0, 0, 0, &par), 1, "at least one");
        assert_eq!(band_threads(31, 62, 0, 0, &par), 31, "an empty frame holds nothing");
        let tight = Parallel { scratch_bytes: 0, blur_pixels: 0, ..par };
        assert_eq!(band_threads(31, 62, mib(8), mib(9), &tight), 1, "a budget of nothing is one thread, not none");
    }

    #[test]
    fn the_pixels_of_a_bands_blur_layers_are_the_boxes_that_reach_it_cut_to_its_context() {
        let boxes = [Rect::new(0.0, 100.0, 50.0, 200.0), Rect::new(0.0, 900.0, 80.0, 1000.0), Rect::new(-500.0, -500.0, 5000.0, 5000.0)];
        // A band of rows 120 to 160 on a frame 100 wide and 1,200 tall, with a reach of 8 rows: the context is the top 168 rows.
        // The first box (50 by 100, cut to the context: 50 by 68) and the third (cut to 100 by 168) reach it; the second does not.
        assert_eq!(layer_pixels(&boxes, (120, 160), 8, 100, 1200), 50 * 68 + 100 * 168);
        // At the bottom the context is the whole frame: the second and the third.
        assert_eq!(layer_pixels(&boxes, (1100, 1200), 8, 100, 1200), 100 * 1200);
        assert_eq!(layer_pixels(&[], (0, 100), 0, 100, 1200), 0);
        assert_eq!(layer_pixels(&[Rect::new(f64::NAN, 0.0, 10.0, 10.0)], (0, 100), 0, 100, 1200), 0, "a box that is not a number holds no pixel");
    }

    #[test]
    fn a_short_frame_is_one_band_and_a_tall_one_has_enough_to_go_round() {
        assert_eq!(split_rows(70, 31).len(), 2);
        assert_eq!(split_rows(60, 31), vec![(0, 60)]);
        assert_eq!(split_rows(20, 31), vec![(0, 20)]);
        assert!(split_rows(1200, 31).len() >= 18, "{:?}", split_rows(1200, 31));
        assert_eq!(split_rows(1200, 4).len(), 4, "a band for each thread");
    }

    #[test]
    fn the_threads_of_a_frame_leave_room_for_what_they_hold_between_them() {
        // 128 MiB of band rows, 32 Mi pixels of blur layers at four bytes, 250,000 steps of recordings at 560 bytes.
        assert_eq!(Parallel::SINGLE.room(), (128 << 20) + (32 << 20) * 4 + 250_000 * 560);
        assert_eq!(Parallel::with_threads(31).room(), Parallel::SINGLE.room(), "the budgets of a frame are the same at any count");
        let all = Parallel { scratch_bytes: u64::MAX, blur_pixels: u64::MAX, max_steps: usize::MAX, ..Parallel::SINGLE };
        assert_eq!(all.room(), u64::MAX, "no overflow");
    }

    #[test]
    fn the_usable_threads_are_what_was_asked_for_on_a_machine_with_no_limit_and_one_for_one() {
        assert_eq!(Parallel::SINGLE.usable_threads(), 1);
        assert_eq!(Parallel::eager(0).usable_threads(), 1);
        assert!(Parallel::eager(500).usable_threads() <= MAX_THREADS);
    }
}
