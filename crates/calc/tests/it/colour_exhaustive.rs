//! The colour conversions of `fn_color.rs` (`to_hls`, `from_hls`, `step`) run over every input, through the functions that
//! reach them (`HUE`, `SAT`, `LUM`, `HSL`, `TINT`, `SHADE`, `TONE`), because the three are private and the task changes no product code.
//!
//! **The laws**:
//!
//! 1. Every output stays on its scale: `H`, `L` and `S` in 0 to 240, a channel in 0 to 255, and a stepped component in 0 to 240.
//! 2. Nothing divides by zero. The variable divisors are `both`, `room` and `span`; the rest divide by constants.
//! 3. RGB, to HLS, to RGB comes back within a bound (measured below).
//!
//! **How each is shown.** The product's outputs for the 8-bit channels are bytes, so a result that was clamped into range looks the same as
//! one that was never out of it. So each law is shown twice. The *model* below is written out from the spec,
//! in wide integers and with the divisions checked, and the tests run it over its whole domain (every colour, every triple, every value with
//! every amount): there the ranges, the divisors and the round trip are *measured*, and the clamp the product applies to a channel is shown
//! never to fire. The *sweeps* then drive the product over the same domain and compare each result with the model's, so what is proved of the
//! model holds of the product. The two together are a complete proof for these functions, without a change to them.
//!
//! **Domains.** `to_hls`: all 2^24 colours. `from_hls`: all 241^3 = 13,997,521 triples in range, inside the 256^3 triples `HSL` accepts (its
//! arguments keep their low byte, a hue above 240 is held at 240 and `L` and `S` above 240 are held at 240). `step`: every
//! component 0 to 240 with every amount of `vSignedInt`, -32768 to 32767 (`signed_int` refuses all others: pinned below), which is 241 *
//! 65,536 = 15,794,176 pairs through `TINT`. A grey carries `L` and the map from `L` to a grey is one to one, so what comes back says which
//! stepped value the product reached.
//!
//! **Run time (measured in this worktree's `cargo test`, 32 threads, opt-level 1).** The model tests run whole, in 0.03 to 0.4 seconds each. The
//! product sweeps cost 10 to 14 microseconds an input, because each input is a formula to read and work out: the six full sweeps take 13.0,
//! 5.8, 12.6, 7.0, 6.5 and 7.1 seconds on 32 threads (231, 105, 225, 119, 119 and 124 seconds of cores), which is far past 10 seconds on a
//! small machine. Under a quota of 8 cores, the six here and the literal sweep of `colour_hex.rs` together take 89 seconds (707 seconds of cores). So each sweep has a strided sample in the normal suite (every 97th input, and the extremes and edges: about 175,000
//! inputs, 0.1 to 0.3 seconds on 32 threads, 1 to 2.4 seconds of cores) and a full run marked `#[ignore]`, which splits its work across
//! `std::thread::scope`. Run the full sweeps one at a time, with plain `cargo test` and not nextest, which ends a test at 120 seconds:
//!
//! ```text
//! cargo test -p diacraft-calc --test it colour_exhaustive -- --ignored --test-threads=1
//! ```
//!
//! **Results** are in the `RESULT` lines of the doc comments of the tests; the bound of law 3 is [`ROUND_TRIP_MAX_ERROR`], which a test asserts.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use diacraft_calc::rules::BAD_ARGUMENT_ERROR;
use diacraft_calc::{Budget, CellAddress, EvalCtx, EvalOptions, Evaluator, Rgb, Value};
use diacraft_doc::{CellAddr, Owner, RowKey, ShapeRef, SheetRef};
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};

// `eval` is the formula evaluator of this test binary (a ShapeSheet formula worked out as a cell), not code execution.
use crate::eval;

// ---- the model: the spec's arithmetic, in wide integers ------------------------------------------------------------------------

/// `floor(n / d)` for a positive `d`: the spec's `//`.
fn fl(n: i64, d: i64) -> i64 {
    n.div_euclid(d)
}

/// `(H, L, S)` of a colour. `None` if a division by zero was asked for, which a test turns into a failure.
fn model_to_hls(r: u8, g: u8, b: u8) -> Option<(i64, i64, i64)> {
    let (r, g, b) = (i64::from(r), i64::from(g), i64::from(b));
    let top = r.max(g).max(b);
    let low = r.min(g).min(b);
    let both = top + low;
    let span = top - low;
    let l = fl(both * 240 + 255, 510);
    if span == 0 {
        return Some((160, l, 0));
    }
    let s = if l <= 120 {
        checked_fl(span * 240 + fl(both, 2), both)?
    } else {
        let room = 510 - both;
        checked_fl(span * 240 + fl(room, 2), room)?
    };
    let d = |v: i64| checked_fl((top - v) * 40 + fl(span, 2), span);
    let (dr, dg, db) = (d(r)?, d(g)?, d(b)?);
    let mut h = if r == top {
        db - dg
    } else if g == top {
        80 + dr - db
    } else {
        160 + dg - dr
    };
    if h < 0 {
        h += 240;
    }
    if h > 240 {
        h -= 240;
    }
    Some((h, l, s))
}

/// `floor(n / d)`, or `None` for `d` 0.
fn checked_fl(n: i64, d: i64) -> Option<i64> {
    (d != 0).then(|| fl(n, d))
}

/// What works out, before any clamp: the two ends, the three `part` values, and the three channels.
struct Raw {
    lo: i64,
    hi: i64,
    parts: [i64; 3],
    chan: [i64; 3],
}

impl Raw {
    /// The colour, each channel clamped to 0 to 255 ("our reasoning clamps each").
    fn rgb(&self) -> Rgb {
        let c = |v: i64| v.clamp(0, 255) as u8;
        Rgb { r: c(self.chan[0]), g: c(self.chan[1]), b: c(self.chan[2]) }
    }

    /// Whether the clamp changed nothing, and every `part` and end is a component on 0 to 240.
    fn in_range(&self) -> bool {
        let comp = |v: &i64| (0..=240).contains(v);
        comp(&self.lo) && comp(&self.hi) && self.parts.iter().all(comp) && self.chan.iter().all(|c| (0..=255).contains(c))
    }
}

/// For `H`, `L` and `S` as given (no hold: see [`model_hsl_call`]).
fn model_from_hls(h: i64, l: i64, s: i64) -> Raw {
    if s == 0 {
        let v = fl(l * 255 + 120, 240);
        return Raw { lo: l, hi: l, parts: [l; 3], chan: [v; 3] };
    }
    let hi = if l <= 120 { fl(l * (240 + s) + 120, 240) } else { l + s - fl(l * s + 120, 240) };
    let lo = 2 * l - hi;
    let part = |mut x: i64| {
        if x < 0 {
            x += 240;
        }
        if x > 240 {
            x -= 240;
        }
        if x < 40 {
            lo + fl((hi - lo) * x + 20, 40)
        } else if x < 120 {
            hi
        } else if x < 160 {
            lo + fl((hi - lo) * (160 - x) + 20, 40)
        } else {
            lo
        }
    };
    let parts = [part(h + 80), part(h), part(h - 80)];
    Raw { lo, hi, parts, chan: parts.map(|p| fl(p * 255 + 120, 240)) }
}

/// What `HSL(h, s, l)` is: a hue above 240 held at 240, `L` and `S` above 240 held at 240; the arguments are bytes.
fn model_hsl_call(h: u8, s: u8, l: u8) -> Raw {
    model_from_hls(i64::from(h).min(240), i64::from(l).min(240), i64::from(s).min(240))
}

fn model_step(v: i64, d: i64) -> i64 {
    if d >= 0 { (v + d.min(fl((240 - v) * d, 100))).min(240) } else { (v - d.abs().min(fl(v * d.abs(), 100))).max(0) }
}

/// Which component a step moves, and by which sign of the amount.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    /// `TINT`: `L` by `d`.
    Tint,
    /// `SHADE`: `L` by `-d`.
    Shade,
    /// `TONE`: `S` by `-d`.
    Tone,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Tint => "TINT",
            Kind::Shade => "SHADE",
            Kind::Tone => "TONE",
        }
    }

    /// What `TINT`, `SHADE` or `TONE` of a colour by `d` is.
    fn model(self, c: [u8; 3], d: i64) -> Rgb {
        let (h, l, s) = model_to_hls(c[0], c[1], c[2]).unwrap();
        match self {
            Kind::Tint => model_from_hls(h, model_step(l, d), s).rgb(),
            Kind::Shade => model_from_hls(h, model_step(l, -d), s).rgb(),
            Kind::Tone => model_from_hls(h, l, model_step(s, -d)).rgb(),
        }
    }
}

// ---- the evaluator, a batch at a time -------------------------------------------------------------------------------------------

/// Works out each formula as one row of a `User` section of one shape, in one document, and gives the values in order.
pub(crate) fn eval_batch(formulas: &[String]) -> Vec<Value> {
    let mut shape = ShapeBuilder::new(1);
    for (i, f) in formulas.iter().enumerate() {
        shape = shape.row_formula("User", None, RowKey::name(&format!("P{i}")), "Value", f, "0");
    }
    let doc = DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build();
    let mut ctx = EvalCtx::new(EvalOptions::default(), Budget { steps: u64::MAX / 2, bytes: u64::MAX / 2 });
    let mut ev = Evaluator::new(&doc, &mut ctx);
    let sheet = SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0));
    (0..formulas.len())
        .map(|i| ev.evaluate(&CellAddress::new(sheet.clone(), CellAddr::in_row("User", None, RowKey::name(&format!("P{i}")), "Value"))))
        .collect()
}

// ---- the driver: a stride or the whole domain, split across threads --------------------------------------------------------------

/// What a sweep found: how many inputs it worked out, how many broke the law with the first few of them, and the worst error seen.
#[derive(Default)]
pub(crate) struct Tally {
    pub(crate) checked: u64,
    pub(crate) bad: u64,
    pub(crate) examples: Vec<String>,
    pub(crate) worst: u64,
}

impl Tally {
    pub(crate) fn miss(&mut self, what: impl FnOnce() -> String) {
        self.bad += 1;
        if self.examples.len() < 8 {
            self.examples.push(what());
        }
    }

    pub(crate) fn merge(&mut self, other: Tally) {
        self.checked += other.checked;
        self.bad += other.bad;
        self.worst = self.worst.max(other.worst);
        for e in other.examples {
            if self.examples.len() < 8 {
                self.examples.push(e);
            }
        }
    }

    pub(crate) fn assert_clean(&self, law: &str) {
        assert!(self.bad == 0, "{law}: {} of {} inputs broke it, first:\n{}", self.bad, self.checked, self.examples.join("\n"));
    }
}

/// The inputs per unit of work: a document of this many rows.
const CHUNK: u64 = 2048;

/// Works `work` over the inputs `0..n` at `stride` (1 for all of them), plus `extras` and the last one, on every available core. A panic in
/// `work` fails the test (the scope joins its threads and re-raises it).
pub(crate) fn drive(n: u64, stride: u64, extras: &[u64], work: impl Fn(&[u64]) -> Tally + Sync) -> Tally {
    let main = n.div_ceil(stride);
    let main_chunks = main.div_ceil(CHUNK);
    let mut tail: Vec<u64> = extras.to_vec();
    if !(n - 1).is_multiple_of(stride) {
        tail.push(n - 1);
    }
    let total_chunks = main_chunks + (tail.len() as u64).div_ceil(CHUNK);
    let next = AtomicU64::new(0);
    let total = Mutex::new(Tally::default());
    let threads = std::thread::available_parallelism().map_or(1, |t| t.get());
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let k = next.fetch_add(1, Ordering::Relaxed);
                    if k >= total_chunks {
                        break;
                    }
                    let idx: Vec<u64> = if k < main_chunks {
                        (k * CHUNK..((k + 1) * CHUNK).min(main)).map(|j| j * stride).collect()
                    } else {
                        tail.iter().skip(((k - main_chunks) * CHUNK) as usize).take(CHUNK as usize).copied().collect()
                    };
                    let found = work(&idx);
                    total.lock().unwrap().merge(found);
                }
            });
        }
    });
    total.into_inner().unwrap()
}

/// Every 97th input: prime, so it takes every residue of each channel in a colour, and every value and every amount of a step.
pub(crate) const SAMPLE_STRIDE: u64 = 97;
pub(crate) const ALL: u64 = 1;

// ---- the inputs -----------------------------------------------------------------------------------------------------------------

pub(crate) fn rgb_at(i: u64) -> [u8; 3] {
    [(i >> 16) as u8, (i >> 8) as u8, i as u8]
}

pub(crate) fn rgb_index(c: [u8; 3]) -> u64 {
    (u64::from(c[0]) << 16) | (u64::from(c[1]) << 8) | u64::from(c[2])
}

/// The colours a stride might miss: every channel at or next to an end or the middle, and every grey.
pub(crate) fn colour_extras() -> Vec<u64> {
    let edge = [0u8, 1, 2, 3, 126, 127, 128, 129, 252, 253, 254, 255];
    let mut out = Vec::new();
    for r in edge {
        for g in edge {
            for b in edge {
                out.push(rgb_index([r, g, b]));
            }
        }
    }
    out.extend((0..=255u8).map(|v| rgb_index([v, v, v])));
    out
}

const TRIPLES: u64 = 256 * 256 * 256;

/// A triple of `HSL(h, s, l)` arguments, as bytes: `h` first.
fn triple_at(i: u64) -> [u8; 3] {
    [(i >> 16) as u8, (i >> 8) as u8, i as u8]
}

fn triple_extras() -> Vec<u64> {
    let edge = [0u8, 1, 2, 39, 40, 41, 119, 120, 121, 159, 160, 161, 239, 240, 241, 254, 255];
    let mut out = Vec::new();
    for h in edge {
        for s in edge {
            for l in edge {
                out.push(rgb_index([h, s, l]));
            }
        }
    }
    out
}

/// The amounts a stride might miss: around 0, the 100 percent, the 240 scale, the powers of two, and the two ends of `vSignedInt`.
fn amount_extras() -> Vec<i64> {
    let mut d: Vec<i64> = (-3..=3).collect();
    for m in [48, 49, 99, 100, 101, 119, 120, 121, 239, 240, 241, 255, 256, 1000, 4096, 16384, 32766, 32767, 32768] {
        d.extend([m, -m]);
    }
    d.retain(|v| (-32768..=32767).contains(v));
    d.sort_unstable();
    d.dedup();
    d
}

/// Steps are swept over `(value, amount)`: the index is `value * 65536 + (amount + 32768)`.
const AMOUNTS: u64 = 65_536;
const VALUES: u64 = 241;

fn step_at(i: u64) -> (i64, i64) {
    ((i / AMOUNTS) as i64, (i % AMOUNTS) as i64 - 32_768)
}

fn step_extras() -> Vec<u64> {
    let mut out = Vec::new();
    for v in 0..VALUES {
        for d in amount_extras() {
            out.push(v * AMOUNTS + (d + 32_768) as u64);
        }
    }
    out
}

/// A grey for each `L` (the smallest), and for each `S` the colour whose `S` it is and whose `L` is nearest 120 (where a step of `S` shows most).
struct Carriers {
    grey_for_l: [Option<u8>; 241],
    colour_for_s: [Option<[u8; 3]>; 241],
    /// The `S` values no 8-bit colour has.
    sat_unreached: Vec<usize>,
}

fn carriers() -> &'static Carriers {
    static CARRIERS: OnceLock<Carriers> = OnceLock::new();
    CARRIERS.get_or_init(|| {
        let mut grey_for_l = [None; 241];
        for g in 0..=255u8 {
            let l = model_to_hls(g, g, g).unwrap().1 as usize;
            grey_for_l[l].get_or_insert(g);
        }
        let mut best: [Option<(i64, u64)>; 241] = [None; 241];
        for i in 0..(1u64 << 24) {
            let [r, g, b] = rgb_at(i);
            let (_, l, s) = model_to_hls(r, g, b).unwrap();
            let near = (l - 120).abs();
            let slot = &mut best[s as usize];
            if slot.is_none_or(|(n, _)| near < n) {
                *slot = Some((near, i));
            }
        }
        let colour_for_s = best.map(|b| b.map(|(_, i)| rgb_at(i)));
        let sat_unreached = (0..241).filter(|s| colour_for_s[*s].is_none()).collect();
        Carriers { grey_for_l, colour_for_s, sat_unreached }
    })
}

// ---- model tests: the laws over the whole domain, in a second or two each ----------------------------------------------------

/// The model reproduces the spec's own vectors, so the model is the spec and not a copy of the product's mistakes.
#[test]
fn the_model_reproduces_the_specs_vectors() {
    assert_eq!(model_to_hls(200, 100, 50), Some((13, 118, 144)));
    assert_eq!(model_to_hls(33, 71, 223).map(|t| t.2), Some(178));
    assert_eq!(model_to_hls(128, 128, 128), Some((160, 120, 0)));
    assert_eq!(model_to_hls(255, 0, 0), Some((0, 120, 240)));
    assert_eq!(model_to_hls(134, 147, 67).map(|t| t.0), Some(47));
    assert_eq!(model_to_hls(0, 0, 255).map(|t| t.0), Some(160));
    assert_eq!(model_to_hls(150, 175, 207).unwrap().1 - model_to_hls(191, 206, 225).unwrap().1, -28);
    assert_eq!(model_hsl_call(70, 90, 100).rgb(), Rgb { r: 0x56, g: 0x93, b: 0x42 });
    assert_eq!(model_hsl_call(160, 240, 120).rgb(), Rgb { r: 0, g: 0, b: 255 });
    assert_eq!(model_hsl_call(0, 0, 120).rgb(), Rgb { r: 0x80, g: 0x80, b: 0x80 });
    // "at L 239 an amount of 48 moves nothing"; and the two caps.
    assert_eq!(model_step(239, 48), 239);
    assert_eq!(model_step(100, 10), 110);
    assert_eq!(model_step(100, 1000), 240);
    assert_eq!(model_step(0, -5), 0);
    assert_eq!(model_step(200, -1000), 0);
}

/// Law 1 and 2 for `to_hls`, over all 2^24 colours: `H`, `L` and `S` stay in 0 to 240 and no divisor is ever zero.
///
/// RESULT (measured): `H` 0 to 239 (240 is never reached, which allows), `L` 0 to 240, `S` 0 to 240; 0 zero divisors in 16,777,216
/// colours; every one of `both`, `room` and `span` that is divided by is at least 1.
#[test]
fn to_hls_stays_in_range_and_never_divides_by_zero() {
    let (mut hmax, mut lmax, mut smax) = (0, 0, 0);
    let (mut hmin, mut lmin, mut smin) = (i64::MAX, i64::MAX, i64::MAX);
    for i in 0..(1u64 << 24) {
        let [r, g, b] = rgb_at(i);
        let Some((h, l, s)) = model_to_hls(r, g, b) else { panic!("RGB({r},{g},{b}): divides by zero") };
        (hmax, lmax, smax) = (hmax.max(h), lmax.max(l), smax.max(s));
        (hmin, lmin, smin) = (hmin.min(h), lmin.min(l), smin.min(s));
    }
    eprintln!("to_hls: H {hmin}..={hmax}, L {lmin}..={lmax}, S {smin}..={smax}");
    assert_eq!((hmin, hmax), (0, 239));
    assert_eq!((lmin, lmax), (0, 240));
    assert_eq!((smin, smax), (0, 240));
}

/// Law 1 for `from_hls`, over all 241^3 triples: every `part`, both ends and every channel is in range *before* the clamp, so the clamp of
/// never fires. (And `S` of 0 takes the grey branch, whose channel is `(L * 255 + 120) // 240`, at most 255.)
///
/// RESULT (measured): 13,997,521 triples, 0 out of range; `lo` 0 to 240, `hi` 0 to 240, channels 0 to 255.
#[test]
fn from_hls_never_leaves_its_ranges_so_its_clamp_never_fires() {
    let (mut lo_max, mut hi_max, mut chan_max, mut chan_min) = (0, 0, 0, i64::MAX);
    let mut triples = 0u64;
    for h in 0..=240i64 {
        for l in 0..=240i64 {
            for s in 0..=240i64 {
                let raw = model_from_hls(h, l, s);
                triples += 1;
                assert!(raw.in_range(), "HLS({h},{l},{s}): lo {} hi {} parts {:?} channels {:?}", raw.lo, raw.hi, raw.parts, raw.chan);
                (lo_max, hi_max) = (lo_max.max(raw.lo), hi_max.max(raw.hi));
                chan_max = chan_max.max(raw.chan.into_iter().max().unwrap());
                chan_min = chan_min.min(raw.chan.into_iter().min().unwrap());
            }
        }
    }
    eprintln!("from_hls: {triples} triples, lo<={lo_max}, hi<={hi_max}, channels {chan_min}..={chan_max}");
    assert_eq!(triples, 241 * 241 * 241);
    assert_eq!((lo_max, hi_max, chan_min, chan_max), (240, 240, 0, 255));
}

/// Law 1 for `step`, over every value 0 to 240 and every amount of `vSignedInt`: the result is in 0 to 240; a positive amount never lowers
/// the value, a negative one never raises it; and the result moves by at most the amount. `signed_int` refuses an amount outside 16 bits,
/// so these are all the pairs a formula can reach.
///
/// RESULT (measured): 15,794,176 pairs, 0 outside 0 to 240, 0 moves the wrong way, 0 moves past the amount; and the result is monotone in the
/// amount for every value.
#[test]
fn step_stays_in_range_and_moves_the_right_way() {
    for v in 0..=240i64 {
        let mut last = None;
        for d in -32_768..=32_767i64 {
            let out = model_step(v, d);
            assert!((0..=240).contains(&out), "step({v}, {d}) = {out}");
            assert!(if d >= 0 { out >= v } else { out <= v }, "step({v}, {d}) = {out} moves the wrong way");
            assert!((out - v).abs() <= d.abs(), "step({v}, {d}) = {out} moves past the amount");
            if let Some(before) = last {
                assert!(out >= before, "step({v}, {d}) = {out} is below step({v}, {}) = {before}", d - 1);
            }
            last = Some(out);
        }
    }
}

/// Law 3: RGB to HLS to RGB, over all 2^24 colours, by composing the model. The 0 to 240 scale has 241 levels where a byte has 256, and
/// `H` has 240 where the round of a hue is 1/240 of a turn, so the colour cannot come back exactly: the bound is measured.
///
/// RESULT (measured, whole domain, 16,777,216 colours): the largest error in any channel is 5 levels of 255 ([`ROUND_TRIP_MAX_ERROR`]). By the
/// worst channel of a colour: 0 off, 2,644,327 colours; 1 off, 10,964,553; 2 off, 2,723,862; 3 off, 428,184; 4 off, 16,236; 5 off, 54. The first
/// colour at the maximum in colour order is (5, 228, 249), which comes back as (4, 233, 251). The greys are within 1.
#[test]
fn rgb_to_hls_to_rgb_error_is_bounded() {
    let mut histogram = [0u64; 256];
    let (mut grey_worst, mut first_at_max) = (0usize, None);
    for i in 0..(1u64 << 24) {
        let c = rgb_at(i);
        let (h, l, s) = model_to_hls(c[0], c[1], c[2]).unwrap();
        let back = model_from_hls(h, l, s).rgb();
        let err = c.iter().zip([back.r, back.g, back.b]).map(|(a, b)| usize::from(a.abs_diff(b))).max().unwrap();
        if err == ROUND_TRIP_MAX_ERROR && first_at_max.is_none() {
            first_at_max = Some((c, back));
        }
        if c[0] == c[1] && c[1] == c[2] {
            grey_worst = grey_worst.max(err);
        }
        histogram[err] += 1;
    }
    let worst = (0..256).rev().find(|e| histogram[*e] > 0).unwrap();
    eprintln!(
        "round trip: max {worst}; by worst channel error {:?}; greys within {grey_worst}; first at the max {first_at_max:?}",
        &histogram[..=worst]
    );
    assert_eq!(worst, ROUND_TRIP_MAX_ERROR, "the bound moved: it is the measured one");
    assert_eq!(histogram[..=worst], [2_644_327, 10_964_553, 2_723_862, 428_184, 16_236, 54]);
    assert_eq!(grey_worst, 1, "the greys come back within one level");
    assert_eq!(first_at_max.map(|(c, back)| (c, [back.r, back.g, back.b])), Some(([5, 228, 249], [4, 233, 251])));
}

/// The measured bound of law 3: no channel of RGB to HLS to RGB is more than this many levels from where it began (of 255).
const ROUND_TRIP_MAX_ERROR: usize = 5;

// ---- sweeps: the product against the model ---------------------------------------------------------------------------------------

/// `H`, `L` and `S` of a colour in one number, a million apart, so one formula gives all three exactly.
fn hls_formula(c: [u8; 3]) -> String {
    let rgb = format!("RGB({},{},{})", c[0], c[1], c[2]);
    format!("HUE({rgb})*1000000000000+LUM({rgb})*1000000+SAT({rgb})")
}

fn unpack(n: f64) -> (i64, i64, i64) {
    let n = n.round() as i64;
    (n / 1_000_000_000_000, (n / 1_000_000) % 1_000_000, n % 1_000_000)
}

fn to_hls_sweep(stride: u64) -> Tally {
    drive(1 << 24, stride, &colour_extras(), |idx| {
        let cols: Vec<[u8; 3]> = idx.iter().map(|&i| rgb_at(i)).collect();
        let values = eval_batch(&cols.iter().map(|c| hls_formula(*c)).collect::<Vec<_>>());
        let mut t = Tally::default();
        for (c, v) in cols.iter().zip(&values) {
            t.checked += 1;
            let want = model_to_hls(c[0], c[1], c[2]).unwrap();
            match v {
                Value::Number(q) if unpack(q.stored) == want && q.stored == (want.0 * 1_000_000_000_000 + want.1 * 1_000_000 + want.2) as f64 => {}
                other => t.miss(|| format!("RGB{c:?}: (H, L, S) is {other:?}, gives {want:?}")),
            }
        }
        t
    })
}

fn hsl_sweep(stride: u64) -> Tally {
    drive(TRIPLES, stride, &triple_extras(), |idx| {
        let triples: Vec<[u8; 3]> = idx.iter().map(|&i| triple_at(i)).collect();
        let values = eval_batch(&triples.iter().map(|[h, s, l]| format!("HSL({h},{s},{l})")).collect::<Vec<_>>());
        let mut t = Tally::default();
        for ([h, s, l], v) in triples.iter().zip(&values) {
            t.checked += 1;
            let raw = model_hsl_call(*h, *s, *l);
            if !raw.in_range() {
                t.miss(|| format!("HSL({h},{s},{l}): the model's own value leaves its range: {:?} {:?}", raw.parts, raw.chan));
            }
            let want = Value::Color(raw.rgb());
            if *v != want {
                t.miss(|| format!("HSL({h},{s},{l}): got {v:?}, gives {want:?}"));
            }
        }
        t
    })
}

fn round_trip_formula(c: [u8; 3]) -> String {
    let rgb = format!("RGB({},{},{})", c[0], c[1], c[2]);
    format!("HSL(HUE({rgb}),SAT({rgb}),LUM({rgb}))")
}

fn round_trip_sweep(stride: u64) -> Tally {
    drive(1 << 24, stride, &colour_extras(), |idx| {
        let cols: Vec<[u8; 3]> = idx.iter().map(|&i| rgb_at(i)).collect();
        let values = eval_batch(&cols.iter().map(|c| round_trip_formula(*c)).collect::<Vec<_>>());
        let mut t = Tally::default();
        for (c, v) in cols.iter().zip(&values) {
            t.checked += 1;
            let (h, l, s) = model_to_hls(c[0], c[1], c[2]).unwrap();
            let want = model_from_hls(h, l, s).rgb();
            match v {
                Value::Color(got) if *got == want => {
                    let err = [(got.r, c[0]), (got.g, c[1]), (got.b, c[2])].map(|(a, b)| u64::from(a.abs_diff(b)));
                    t.worst = t.worst.max(err.into_iter().max().unwrap());
                }
                other => t.miss(|| format!("RGB{c:?}: HSL(HUE, SAT, LUM) is {other:?}, the spec gives {want:?}")),
            }
        }
        t
    })
}

/// The carrier of a step: a grey for `L`, a colour with the given `S` for `S`; `None` for an `S` no colour has.
fn carrier(kind: Kind, v: i64) -> Option<[u8; 3]> {
    let c = carriers();
    match kind {
        Kind::Tint | Kind::Shade => c.grey_for_l[v as usize].map(|g| [g, g, g]),
        Kind::Tone => c.colour_for_s[v as usize],
    }
}

fn step_sweep(kind: Kind, stride: u64) -> Tally {
    drive(VALUES * AMOUNTS, stride, &step_extras(), |idx| {
        let work: Vec<([u8; 3], i64, i64)> = idx.iter().map(|&i| step_at(i)).filter_map(|(v, d)| carrier(kind, v).map(|c| (c, v, d))).collect();
        let formulas: Vec<String> = work.iter().map(|(c, _, d)| format!("{}(RGB({},{},{}),{d})", kind.name(), c[0], c[1], c[2])).collect();
        let values = eval_batch(&formulas);
        let mut t = Tally::default();
        for ((c, v, d), got) in work.iter().zip(&values) {
            t.checked += 1;
            let want = Value::Color(kind.model(*c, *d));
            if *got != want {
                t.miss(|| format!("{}(RGB{c:?}, {d}) with the component at {v}: got {got:?}, gives {want:?}", kind.name()));
            }
        }
        t
    })
}

/// The grey of a step is one to one with `L`, so a grey that comes back says which stepped value the product reached, and equality with the
/// model's grey is equality of the stepped `L`.
#[test]
fn a_grey_says_which_lightness_it_carries() {
    let mut seen = std::collections::HashSet::new();
    for l in 0..=240i64 {
        assert!(seen.insert(model_from_hls(160, l, 0).rgb()), "two lightnesses make the grey of {l}");
    }
    let c = carriers();
    assert!(c.grey_for_l.iter().all(Option::is_some), "an L no grey has");
    eprintln!("S values no 8-bit colour has: {:?}", c.sat_unreached);
}

/// `HUE`, `SAT` and `LUM` (`to_hls`) against, in the normal suite: every 97th colour and the edges.
#[test]
fn hue_lum_sat_match_conv_on_a_sample() {
    let t = to_hls_sweep(SAMPLE_STRIDE);
    eprintln!("HUE/LUM/SAT sample: {} colours", t.checked);
    t.assert_clean("HUE, LUM and SAT against");
}

/// The same over all 2^24 colours.
#[test]
#[ignore = "full sweep: with the other 6 sweeps, 89 s together on 8 cores: cargo test -p diacraft-calc --test it colour_exhaustive -- --ignored --test-threads=1"]
fn hue_lum_sat_match_conv_on_every_colour() {
    let t = to_hls_sweep(ALL);
    assert_eq!(t.checked, (1 << 24) + colour_extras().len() as u64);
    t.assert_clean("HUE, LUM and SAT against");
}

/// `HSL(h, s, l)` (`from_hls`) against, in the normal suite.
#[test]
fn hsl_matches_conv_on_a_sample() {
    let t = hsl_sweep(SAMPLE_STRIDE);
    eprintln!("HSL sample: {} triples", t.checked);
    t.assert_clean("HSL against");
}

/// The same over all 256^3 argument triples, which holds the 241^3 in range.
#[test]
#[ignore = "full sweep: with the other 6 sweeps, 89 s together on 8 cores: cargo test -p diacraft-calc --test it colour_exhaustive -- --ignored --test-threads=1"]
fn hsl_matches_conv_on_every_triple() {
    let t = hsl_sweep(ALL);
    assert_eq!(t.checked, TRIPLES + triple_extras().len() as u64);
    t.assert_clean("HSL against");
}

/// The round trip `HSL(HUE(c), SAT(c), LUM(c))` through the product, in the normal suite: it is the model's, and so within the bound.
#[test]
fn the_round_trip_through_the_product_is_the_models_on_a_sample() {
    let t = round_trip_sweep(SAMPLE_STRIDE);
    t.assert_clean("HSL(HUE, SAT, LUM) against then");
    assert!(t.worst <= ROUND_TRIP_MAX_ERROR as u64, "a round trip is {} off, past the measured bound {ROUND_TRIP_MAX_ERROR}", t.worst);
}

/// The same over all 2^24 colours.
#[test]
#[ignore = "full sweep: with the other 6 sweeps, 89 s together on 8 cores: cargo test -p diacraft-calc --test it colour_exhaustive -- --ignored --test-threads=1"]
fn the_round_trip_through_the_product_is_the_models_on_every_colour() {
    let t = round_trip_sweep(ALL);
    t.assert_clean("HSL(HUE, SAT, LUM) against then");
    assert_eq!(t.worst, ROUND_TRIP_MAX_ERROR as u64, "the worst error through the product is the model's");
}

/// `TINT` (`step` on `L`) over every value with every amount, in the normal suite: every 97th pair and the edges.
#[test]
fn tint_steps_l_by_step_on_a_sample() {
    let t = step_sweep(Kind::Tint, SAMPLE_STRIDE);
    eprintln!("TINT sample: {} pairs", t.checked);
    t.assert_clean("TINT against");
}

/// `TINT` over all 241 * 65,536 pairs of an `L` and an amount.
#[test]
#[ignore = "full sweep: with the other 6 sweeps, 89 s together on 8 cores: cargo test -p diacraft-calc --test it colour_exhaustive -- --ignored --test-threads=1"]
fn tint_steps_l_by_step_for_every_value_and_amount() {
    let t = step_sweep(Kind::Tint, ALL);
    assert!(t.checked >= VALUES * AMOUNTS);
    t.assert_clean("TINT against");
}

/// `SHADE` (`L` by the amount's negative) in the normal suite.
#[test]
fn shade_steps_l_by_the_negative_amount_on_a_sample() {
    step_sweep(Kind::Shade, SAMPLE_STRIDE).assert_clean("SHADE against");
}

/// `SHADE` over all pairs: its amount of -32768 is a step of 32768, which `TINT` cannot reach.
#[test]
#[ignore = "full sweep: with the other 6 sweeps, 89 s together on 8 cores: cargo test -p diacraft-calc --test it colour_exhaustive -- --ignored --test-threads=1"]
fn shade_steps_l_by_the_negative_amount_for_every_value_and_amount() {
    step_sweep(Kind::Shade, ALL).assert_clean("SHADE against");
}

/// `TONE` (`S` by the amount's negative) in the normal suite.
#[test]
fn tone_steps_s_by_the_negative_amount_on_a_sample() {
    step_sweep(Kind::Tone, SAMPLE_STRIDE).assert_clean("TONE against");
}

/// `TONE` over every `S` a colour has and every amount.
#[test]
#[ignore = "full sweep: with the other 6 sweeps, 89 s together on 8 cores: cargo test -p diacraft-calc --test it colour_exhaustive -- --ignored --test-threads=1"]
fn tone_steps_s_by_the_negative_amount_for_every_value_and_amount() {
    step_sweep(Kind::Tone, ALL).assert_clean("TONE against");
}

/// An amount outside `vSignedInt` is an error and not a step: the edge of the domain above is real.
#[test]
fn an_amount_past_16_bits_is_an_error_not_a_step() {
    for d in [32_768, -32_769, 65_536, 2_147_483_647] {
        for f in ["TINT", "SHADE", "TONE"] {
            assert_eq!(eval(&format!("{f}(RGB(10,20,30),{d})")), Value::Error(BAD_ARGUMENT_ERROR), "{f} {d}");
        }
    }
    assert!(matches!(eval("TINT(RGB(10,20,30),32767)"), Value::Color(_)));
    assert!(matches!(eval("TINT(RGB(10,20,30),-32768)"), Value::Color(_)));
}
