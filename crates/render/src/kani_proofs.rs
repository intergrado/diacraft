//! Kani proof harnesses for the renderer's integer code of a frame drawn on threads: how many threads draw the bands of a frame at once
//! (`par::band_threads`), the room the threads of a frame hold (`Parallel::room`), the steps of the evaluator each share of a walk is given (`share::steps_for_each`), and the
//! judge of what the shares spent (`share::within`); and, the frame's budget of work: `share::within` with the work counted, the tally of what the
//! shares spent (`share::Tally::add`), the meter a frame charges its work to (`frame::WorkMeter`, what `Frame::charge`, `work_left` and `add_work` count) and the work of a decode
//! (`picture::decode_work`). Run with `cargo xtask kani` (`docs/development.md`, Proofs).
//!
//! **Not proved here:** `split_rows`, the bands of rows a frame is cut into (the walk goes on until the rows run out, and the solver must show from the division that sizes a band that it
//! ends within the count of threads: past 300 s for every height and a count of 0 to 4, and for a fixed count of 2 and of 3 too); `threads_from` (it is `diacraft_container::par::from_setting`, proved in that crate); `Parallel::usable_threads` (it reads the process's limits from `/proc`,
//! through `diacraft_container::spare::threads_fit`, whose rule `fit` is proved there); `text::Caches::one_of` (a cache of two hash maps, whose caps are private to it: the hash-map
//! trap); `layer_pixels` and the recorder's boxes (many floats). Their unit tests hold them.
//!
//! **Not proved:** `Clock::stepping`'s readings: each reading is `Duration::from_nanos` of the count, a division of 64 bits by a billion, and three readings
//! of a step of any whole seconds ran past 300 s with CaDiCaL, and of a step of up to 255 seconds past 300 s with Kissat (the trap of times to the nanosecond); `decode_work` of 26
//! bytes of any format (past 300 s: the walk of a JPEG's segments beside the BMP's and the PNG's sizes; a BMP's 26 bytes and a PNG's 24 are proved apart); `Frame::image_work`,
//! `pattern::marks`, `hatch_lines` and `dash::plan` (floats through trigonometry or an affine of six). Their unit tests hold them.

use crate::frame::{Counts, WorkMeter};
use crate::limits::{BYTES_PER_WORK, Limits, MAX_BLUR_PIXELS_PER_FRAME, MAX_BLURS_PER_FRAME, MAX_PICTURE_PIXELS, PIXELS_PER_WORK};
use crate::par::{MAX_BAND_SCRATCH_BYTES, MAX_RECORDED_STEPS, MAX_THREADS, Parallel, band_threads};
use crate::picture::decode_work;
use crate::read::THEME_STEPS;
use crate::share::{Tally, steps_for_each, within};

/// The bands' threads of a frame (`band_threads`) for **every** count wanted (up to the 64 a frame is held to), count of bands and scratch budget, and the bytes of a frame of
/// up to 65,535 by 65,535 pixels, with no blur: at least one; no more than were wanted (none wanted is one) nor than there are bands, when there are some; and, **when more than one draws,
/// their contexts of up to the frame's bytes each are within the scratch budget together**.
#[kani::proof]
#[kani::solver(kissat)]
fn the_threads_that_draw_a_frames_bands_hold_their_contexts_within_the_scratch_budget() {
    let (wanted, bands): (usize, usize) = (kani::any(), kani::any());
    kani::assume(wanted <= MAX_THREADS);
    let frame_bytes: u64 = kani::any();
    kani::assume(frame_bytes <= 65_535 * 65_535 * 4);
    let par = Parallel { scratch_bytes: kani::any(), ..Parallel::SINGLE };
    let got = band_threads(wanted, bands, frame_bytes, 0, &par);
    assert!(got >= 1 && got <= wanted.max(1));
    if bands > 0 {
        assert!(got <= bands);
    }
    if got > 1 {
        assert!(got as u128 * u128::from(frame_bytes) <= u128::from(par.scratch_bytes));
    }
    kani::cover!((got > 1) & (got < wanted) & (got < bands), "a frame drawn by fewer threads than it has bands and were wanted, for its scratch");
}

/// `band_threads` for **every** count wanted (up to 64), count of bands and budget of the pixels of blurs, and the pixels of the blur layers of a band of a frame of up to 65,535 by
/// 65,535 pixels, beside a frame of no bytes: when more than one draws, **their blur layers are within the budget of the pixels of blurs together**, and the counts are bound as above.
#[kani::proof]
#[kani::solver(kissat)]
fn the_threads_that_draw_a_frames_bands_hold_their_blur_layers_within_the_frames_budget() {
    let (wanted, bands): (usize, usize) = (kani::any(), kani::any());
    kani::assume(wanted <= MAX_THREADS);
    let layer_pixels: u64 = kani::any();
    kani::assume(layer_pixels <= 65_535 * 65_535);
    let par = Parallel { blur_pixels: kani::any(), ..Parallel::SINGLE };
    let got = band_threads(wanted, bands, 0, layer_pixels, &par);
    assert!(got >= 1 && got <= wanted.max(1));
    if bands > 0 {
        assert!(got <= bands);
    }
    if got > 1 {
        assert!(got as u128 * u128::from(layer_pixels) <= u128::from(par.blur_pixels));
    }
    kani::cover!((got > 1) & (got < wanted) & (got < bands), "a frame drawn by fewer threads than it has bands and were wanted, for its blurs");
}

/// `Parallel::room` (what the threads of a frame hold, beside which they are held to the address space) for **every** budget: the scratch bytes, four bytes for each pixel of
/// the blurs and 560 for each step of the recordings, summed exactly, or the largest number where that passes it; about 400 MB for a default [`Parallel`].
#[kani::proof]
fn the_room_of_a_frames_threads_is_the_sum_of_its_budgets_and_never_wraps() {
    let par = Parallel { scratch_bytes: kani::any(), blur_pixels: kani::any(), max_steps: kani::any(), ..Parallel::SINGLE };
    let exact = u128::from(par.scratch_bytes) + 4 * u128::from(par.blur_pixels) + 560 * par.max_steps as u128;
    assert!(u128::from(par.room()) == exact.min(u128::from(u64::MAX)));
    let default = Parallel::with_threads(4).room();
    assert!(default == MAX_BAND_SCRATCH_BYTES + 4 * MAX_BLUR_PIXELS_PER_FRAME + 560 * MAX_RECORDED_STEPS as u64);
    assert!((390_000_000..410_000_000).contains(&default));
    kani::cover!(par.room() == u64::MAX, "budgets past the largest number");
}

/// `steps_for_each` for **every** count of shares: the frame's steps cut into that many equal parts, rounded up, so that the parts together are the frame's steps or
/// more, and less than one more step for each share (none is one share).
#[kani::proof]
fn the_shares_of_a_walk_take_the_frames_steps_between_them_rounded_up() {
    let shares: usize = kani::any();
    let each = u128::from(steps_for_each(shares));
    let k = shares.max(1) as u128;
    let whole = u128::from(THEME_STEPS);
    assert!(each * k >= whole && (each - 1) * k < whole);
    kani::cover!(each * k == whole, "shares that cut the steps exactly");
    kani::cover!(each == 1, "a share of one step");
}

/// What a share of a walk spent, drawn whole.
fn any_counts() -> Counts {
    Counts {
        drawn: kani::any(),
        rows: kani::any(),
        pushed: kani::any(),
        blurs: kani::any(),
        blur_pixels: kani::any(),
        text_chars: kani::any(),
        label_chars: kani::any(),
        nodes: kani::any(),
        work: kani::any(),
        ..Counts::default()
    }
}

/// Limits of a frame with every count drawn whole, the work's too, and no time.
fn any_limits() -> Limits {
    Limits {
        max_label_chars: kani::any(),
        max_shapes: kani::any(),
        max_rows: kani::any(),
        max_text_chars: kani::any(),
        max_work: kani::any(),
        ..Limits::unlimited_time()
    }
}

/// `share::within` (and the work) is **monotone** for every limits, cap of steps recorded and two sums of what the shares spent, the units of work
/// included: what is within the budgets stays within them for any sum no larger in each count, which is what lets the tally stop the shares on a sum of some of them (the walk in
/// order spends no more than the shares' sum). Past the blurs a frame takes, or a unit of work past its budget, no sum is within.
#[kani::proof]
fn what_is_within_a_frames_budgets_stays_within_them_for_less() {
    let limits = any_limits();
    let max_nodes: usize = kani::any();
    let (more, less) = (any_counts(), any_counts());
    kani::assume(less.drawn <= more.drawn && less.rows <= more.rows && less.pushed <= more.pushed && less.blurs <= more.blurs);
    kani::assume(
        less.blur_pixels <= more.blur_pixels
            && less.text_chars <= more.text_chars
            && less.label_chars <= more.label_chars
            && less.nodes <= more.nodes
            && less.work <= more.work,
    );
    let fits = within(&limits, &more, max_nodes);
    if fits {
        assert!(within(&limits, &less, max_nodes));
    }
    assert!(!within(&limits, &Counts { blurs: MAX_BLURS_PER_FRAME + 1, ..less }, max_nodes));
    if let Some(past) = limits.max_work.checked_add(1) {
        assert!(!within(&limits, &Counts { work: past, ..less }, max_nodes));
    }
    kani::cover!(fits & (more.drawn > 0) & (more.work > 0), "a sum of shares within the budgets");
    kani::cover!(!fits & within(&limits, &less, max_nodes), "a sum past a budget whose part is within it");
    kani::cover!(!fits & (more.work > limits.max_work) & within(&limits, &Counts { work: 0, ..more }, max_nodes), "a sum past the work budget alone");
}

/// The sum of two shares' counts as the tally is to judge it: each count's **true** sum, held at the largest number it holds.
fn held_sum(a: &Counts, b: &Counts) -> Counts {
    Counts {
        drawn: a.drawn.saturating_add(b.drawn),
        rows: a.rows.saturating_add(b.rows),
        pushed: a.pushed.saturating_add(b.pushed),
        blurs: a.blurs.saturating_add(b.blurs),
        blur_pixels: a.blur_pixels.saturating_add(b.blur_pixels),
        text_chars: a.text_chars.saturating_add(b.text_chars),
        label_chars: a.label_chars.saturating_add(b.label_chars),
        nodes: a.nodes.saturating_add(b.nodes),
        work: a.work.saturating_add(b.work),
        ..Counts::default()
    }
}

/// `share::Tally::add` (the work) for **three** shares' counts added in a row, every count of each drawn whole, under any limits: after each add the
/// shares are stopped exactly when the sum of what was added so far, each count held at the largest number it holds, is not within the budgets, and once stopped they stay stopped.
/// A cell of the tally goes round when a count's sum passes the largest number, and the sum it gives is then held there and judged so: a later share's small count is never
/// judged against a cell that went round, since a sum that passes the largest number is past every budget that is not the largest number itself. So the sum of the units of work
/// past a budget of work stops every share (the largest number is no budget: a window's frame, which its time bounds).
#[kani::proof]
fn the_tally_stops_the_shares_exactly_when_what_they_spent_together_passes_a_budget() {
    let limits = any_limits();
    let max_nodes: usize = kani::any();
    let tally = Tally::new(limits, max_nodes);
    let (a, b, c) = (any_counts(), any_counts(), any_counts());
    tally.add(&a);
    let first = tally.aborted();
    assert!(first == !within(&limits, &a, max_nodes));
    tally.add(&b);
    let second = tally.aborted();
    let two = held_sum(&a, &b);
    assert!(second == !within(&limits, &two, max_nodes));
    assert!(second | !first);
    tally.add(&c);
    let third = tally.aborted();
    let three = held_sum(&two, &c);
    assert!(third == !within(&limits, &three, max_nodes));
    assert!(third | !second);
    // A budget of work of the largest number is none (`Limits::default`): only a smaller one stops the shares.
    if limits.max_work < u64::MAX && u128::from(a.work) + u128::from(b.work) + u128::from(c.work) > u128::from(limits.max_work) {
        assert!(third);
    }
    let wrapped = u128::from(a.work) + u128::from(b.work) > u128::from(u64::MAX);
    kani::cover!(!first & second, "a second share that takes the sum past a budget");
    kani::cover!(!second & third & (c.work > 0) & (three.work > limits.max_work), "a third share whose work takes the sum past the work budget");
    kani::cover!(!third & (a.work > 0) & (b.work > 0) & (c.work > 0), "three shares within the budgets");
    kani::cover!(wrapped & (limits.max_work == u64::MAX), "a cell of work that goes round with no budget of work");
}

/// `frame::WorkMeter`, what `Frame::charge`, `Frame::work_left` and `Frame::add_work` count, through **three steps in a row**, each a charge, an add (of work a
/// decode did within what was left) or a cut (a decode its room cut short), of any units within any budget: what is left is the budget less the work; a charge is taken exactly
/// when nothing before it was refused or cut and the units fit what is left, and then adds exactly them; **once a charge is refused or the meter cut, every charge after is
/// refused**; an add adds the units, or what was left when they pass it; and the work never goes down and never passes the budget.
#[kani::proof]
fn a_frames_work_is_charged_before_it_is_done_and_once_refused_every_charge_after_is() {
    let max: u64 = kani::any();
    let mut meter = WorkMeter::default();
    let mut said_no = false;
    let (mut taken_after_add, mut refused_though_fits, mut added_past) = (false, false, false);
    let mut last_was_add = false;
    for _ in 0..3 {
        let before = meter.done();
        let left = meter.left(max);
        assert!(u128::from(before) + u128::from(left) == u128::from(max));
        let units: u64 = kani::any();
        let fits = u128::from(before) + u128::from(units) <= u128::from(max);
        let step: u8 = kani::any();
        match step % 3 {
            0 => {
                let took = meter.charge(units, max);
                assert!(took == (!said_no & fits));
                if took {
                    assert!(meter.done() == before + units);
                    taken_after_add |= last_was_add & (units > 0);
                } else {
                    assert!(meter.done() == before);
                    refused_though_fits |= said_no & fits;
                    said_no = true;
                }
                last_was_add = false;
            }
            1 => {
                meter.add(units, max);
                if units <= left {
                    assert!(meter.done() == before + units);
                } else {
                    assert!(meter.done() == max);
                    added_past = true;
                }
                last_was_add = true;
            }
            _ => {
                meter.cut();
                assert!(meter.done() == before);
                said_no = true;
                last_was_add = false;
            }
        }
        assert!(meter.done() >= before && meter.done() <= max);
    }
    kani::cover!(taken_after_add, "a charge taken after an add");
    kani::cover!(refused_though_fits, "a charge that fits refused after the budget said no");
    kani::cover!(added_past, "an add past what was left, held at the budget");
}

/// `picture::decode_work` of **every** `N` bytes: at least one unit and at least the bytes' own (`BYTES_PER_WORK` to a unit, rounded up), and never more than the bytes'
/// or the most a picture decodes to (`MAX_PICTURE_PIXELS` at `PIXELS_PER_WORK` to a unit), whatever its header says. → what it gave.
fn decode_work_law<const N: usize>() -> u64 {
    let bytes: [u8; N] = kani::any();
    let got = decode_work(&bytes);
    let read = (N as u64).div_ceil(BYTES_PER_WORK);
    assert!(got >= 1 && got >= read);
    assert!(got <= read.max(MAX_PICTURE_PIXELS / PIXELS_PER_WORK).max(1));
    got
}

macro_rules! decode_work_law {
    ($($name:ident: $n:literal, $unwind:literal, $costs_more:tt),* $(,)?) => {
        $(
            #[kani::proof]
            #[kani::unwind($unwind)]
            fn $name() {
                let got = decode_work_law::<$n>();
                kani::cover!(got == 1, "bytes that cost one unit");
                decode_work_law!(@more $costs_more, got);
            }
        )*
    };
    // Bytes long enough for a header of a raster can say more pixels than their own bytes cost.
    (@more true, $got:ident) => {
        kani::cover!($got > 1, "a header whose pixels cost more than its bytes");
    };
    (@more false, $got:ident) => {};
}

decode_work_law! {
    the_work_of_a_decode_of_no_bytes_is_one_unit: 0, 2, false,
    the_work_of_a_decode_of_four_bytes_is_one_unit_and_never_more_than_a_picture_holds: 4, 6, false,
    the_work_of_a_decode_of_eleven_bytes_is_its_header_s_pixels_or_its_bytes_and_never_more_than_a_picture_holds: 11, 13, true,
}

/// The 24 bytes of a PNG's signature and header with the width and height `w` and `h` (the bytes between, the rest of the signature and the header's length, are any).
fn png_of(w: u32, h: u32) -> [u8; 24] {
    let mut b: [u8; 24] = kani::any();
    b[..4].copy_from_slice(&[0x89, b'P', b'N', b'G']);
    b[12..16].copy_from_slice(b"IHDR");
    b[16..20].copy_from_slice(&w.to_be_bytes());
    b[20..24].copy_from_slice(&h.to_be_bytes());
    b
}

/// `decode_work` of **every** 26 bytes that start as a BMP (`BM`), of either size of header, and of every PNG's signature and header of any width and height (24 bytes): at least
/// one unit, and never more than the most a picture decodes to, whatever the header says.
#[kani::proof]
#[kani::unwind(5)]
fn the_work_of_a_decode_of_a_bitmaps_header_is_never_more_than_a_picture_holds() {
    let mut bmp: [u8; 26] = kani::any();
    bmp[..2].copy_from_slice(b"BM");
    let most = MAX_PICTURE_PIXELS / PIXELS_PER_WORK;
    let got = decode_work(&bmp);
    assert!((1..=most).contains(&got));
    let png = decode_work(&png_of(kani::any(), kani::any()));
    assert!((1..=most).contains(&png));
    kani::cover!(got > 1, "a header whose pixels cost more than its bytes");
    kani::cover!(got == most, "the most a picture decodes to");
}

/// `decode_work` of a PNG whose header says **more pixels than a picture decodes to** (both sides past 8,192, so more than `MAX_PICTURE_PIXELS`): it costs its bytes only, one unit
/// for its 24 bytes; and of one a pixel high, any width: its pixels at `PIXELS_PER_WORK` to a unit, rounded up, as far as `MAX_PICTURE_PIXELS`, and its bytes past it.
#[kani::proof]
#[kani::unwind(5)]
fn a_header_that_says_more_pixels_than_a_picture_decodes_to_costs_its_bytes_only() {
    let (w, h): (u32, u32) = (kani::any(), kani::any());
    kani::assume(w > 8_192 && h > 8_192);
    assert!(decode_work(&png_of(w, h)) == 1);
    let wide: u32 = kani::any();
    let got = decode_work(&png_of(wide, 1));
    if u64::from(wide) <= MAX_PICTURE_PIXELS {
        assert!(got == u64::from(wide).div_ceil(PIXELS_PER_WORK).max(1));
    } else {
        assert!(got == 1);
    }
    kani::cover!(got > 1, "a header whose pixels cost more than its bytes");
}
