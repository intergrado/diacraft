//! Kani proof harnesses for `InfiniteLine::clip` and the helpers it is made of (`infinite.rs`). Run with `cargo xtask kani` (`docs/development.md`, Proofs).
//!
//! Every `f64` from `kani::any()` ranges over all 2^64 bit patterns, NaN and both infinities included, unless a harness assumes otherwise. This crate sets no Kani
//! flags, so Kani's NaN check is on: a harness fails at any operation that makes a NaN on a path it reaches, and so each one here also proves that no NaN is made
//! from its finite inputs.
//!
//! **How far the proofs go.** Kani cannot prove a law of `clip` over its eight numbers: with more than a few symbolic floats and a division a proof does
//! not end (one over all eight ran 3 hours without an answer, and ones over two to four of them ran past 900 s with every solver). So the laws are proved in two
//! narrowed ways:
//! - the helpers on their own, each over one to three symbolic numbers: `exponent`, `power_of_two`, the shift and `scaled_to_unit`, `way`, `nearer` (one axis at a
//!   time), and `along` (its sign and finiteness for any edge, place and step, and the order of two edges for a fixed place and step);
//! - `clip` itself over families of lines with one symbolic number: a level or an upright line from any start or at any height, lines of a fixed slope cut by a window
//!   with one moving side (the harness knows their exact chord), a diagonal through any point of two kinds (any number from 1 to 2, any power of two, in six harnesses
//!), and a line
//!   from a far point at any height through a point of the window.
//!
//! What these leave unproved, and the harnesses dropped for not finishing in 300 s, are named in `docs/development.md` (Proofs). Time a harness before adding it: the
//! ones here take from under a second to about two minutes on a busy machine.

use kurbo::{Line, Point, Rect};

use crate::InfiniteLine;
use crate::infinite::{along, exponent, nearer, power_of_two, scaled_to_unit, unit_shift, way};

/// One unit of the subnormal numbers, 2^-1074 (5E-324).
const UNIT: f64 = 5e-324;

/// The window of the families of lines: x from -1 to 3, y from -2 to 2.
const W: Rect = Rect::new(-1.0, -2.0, 3.0, 2.0);

/// Any finite number.
fn any_finite() -> f64 {
    let v: f64 = kani::any();
    kani::assume(v.is_finite());
    v
}

/// 2^`e` built from its bits, for an `e` from -1074 (the smallest subnormal number) up, and infinity past 1023: the harnesses' own powers, not the code's.
fn two_to(e: i32) -> f64 {
    if e > 1023 {
        f64::INFINITY
    } else if e >= -1022 {
        f64::from_bits(((e + 1023) as u64) << 52)
    } else {
        f64::from_bits(1u64 << (e + 1074))
    }
}

/// A finite number that is not 0 as its sign, its significand as a 53-bit whole number with the top bit set, and its exponent: |x| = significand * 2^(e - 52).
/// A subnormal number's significand is shifted up to the top bit, so two numbers that differ only by a power of two have the same significand.
fn parts(x: f64) -> (bool, u64, i32) {
    let bits = x.to_bits();
    let field = ((bits >> 52) & 0x7ff) as i32;
    let fraction = bits & ((1u64 << 52) - 1);
    if field == 0 {
        let lift = fraction.leading_zeros() as i32 - 11;
        (x.is_sign_negative(), fraction << lift, -1022 - lift)
    } else {
        (x.is_sign_negative(), fraction | (1u64 << 52), field - 1023)
    }
}

// ---- the helpers ------------------------------------------------------------------------------------------------------------

/// `exponent` brackets every finite number over 0 between two powers of two, 2^e <= x < 2^(e + 1), the subnormal numbers too, with `e` from -1074 to 1023.
#[kani::proof]
fn exponent_brackets_every_positive_number() {
    let x: f64 = kani::any();
    kani::assume(x.is_finite() && x > 0.0);
    let e = exponent(x);
    assert!((-1074..=1023).contains(&e));
    assert!(two_to(e) <= x && x < two_to(e + 1));
    kani::cover!(e == -1074, "the smallest subnormal number");
    kani::cover!(e == -1023, "the largest subnormal power");
    kani::cover!(e == -1022, "the smallest normal power");
    kani::cover!(e == 0, "from 1 to 2");
    kani::cover!(e == 1023, "the largest numbers");
}

/// `power_of_two(p)` is a number exactly for a `p` from -1022 to 1023, the powers a normal number holds, and there it is 2^p: a normal number over 0, 1 at 0, and
/// each power twice the one below it, which pins every one of them.
#[kani::proof]
fn power_of_two_is_two_to_the_power_where_a_normal_number_holds_it() {
    let p: i32 = kani::any();
    match power_of_two(p) {
        Some(v) => {
            assert!((-1022..=1023).contains(&p));
            assert!(v.is_normal() && v > 0.0);
            if p == 0 {
                assert!(v == 1.0);
            }
            if p < 1023 {
                assert!(power_of_two(p + 1) == Some(v * 2.0));
            }
            kani::cover!(p == -1022, "the smallest");
            kani::cover!(p == 1023, "the largest");
        }
        None => {
            assert!(!(-1022..=1023).contains(&p));
            kani::cover!(p == -1023, "one under the smallest");
            kani::cover!(p == 1024, "one over the largest");
        }
    }
}

/// The shift of `scaled_to_unit` (`unit_shift`), for every pair of sizes of the larger part (over 0) and the smaller one (0 up to the larger): its two halves are
/// powers `power_of_two` holds, so `scaled_to_unit` is never `None`; the larger part lands from 1 up to under 2^1024 (it does not overflow), in [1, 2) unless the
/// smaller part is under 2^-1022 of it; the smaller part, if not 0, lands on a normal number unless the larger is at the top already; and the scale is raised no
/// higher than that needs.
#[kani::proof]
fn the_shift_puts_the_larger_part_in_one_to_two_and_keeps_the_smaller_part_normal() {
    let (larger, smaller): (f64, f64) = (kani::any(), kani::any());
    kani::assume(larger.is_finite() && larger > 0.0 && smaller >= 0.0 && smaller <= larger);
    let shift = unit_shift(larger, smaller);
    let first = shift / 2;
    assert!(power_of_two(first).is_some() && power_of_two(shift - first).is_some());
    // Where the larger part lands: 2^top <= larger * 2^shift < 2^(top + 1).
    let top = exponent(larger) + shift;
    assert!((0..=1023).contains(&top));
    if smaller == 0.0 {
        assert!(top == 0);
        kani::cover!(true, "no smaller part");
    } else {
        let low = exponent(smaller) + shift;
        if exponent(larger) - exponent(smaller) <= 1022 {
            assert!(top == 0);
        }
        assert!(low >= -1022 || top == 1023);
        assert!(top == 0 || low == -1022 || (top == 1023 && low < -1022));
        kani::cover!(top == 0, "the larger part in [1, 2)");
        kani::cover!(top > 0 && top < 1023, "raised to keep the smaller part normal");
        kani::cover!(top == 1023 && low < -1022, "raised to the top, the smaller part still subnormal");
    }
}

/// `scaled_to_unit` of any finite pair but (0, 0), which every way between two distinct points is (`the_way_is_finite_...`): `Some` and finite; each part keeps
/// its sign and its significand to the last bit, a 0 stays 0, and both move by the same power of two, so the way is the same way; the larger part is at least 1, in
/// [1, 2) unless the smaller part is under 2^-1022 of it; and a smaller part that is not 0 is a normal number unless the larger is at 2^1023 or over.
#[kani::proof]
fn scaled_to_unit_keeps_the_way_to_the_last_bit_and_does_not_overflow() {
    let (x, y) = (any_finite(), any_finite());
    kani::assume(x != 0.0 || y != 0.0);
    match scaled_to_unit(x, y) {
        Some((sx, sy)) => {
            assert!(sx.is_finite() && sy.is_finite());
            assert!((sx == 0.0) == (x == 0.0) && (sy == 0.0) == (y == 0.0));
            let mut moved = [None, None];
            for (i, (v, s)) in [(x, sx), (y, sy)].into_iter().enumerate() {
                if v != 0.0 {
                    let (sign, significand, e) = parts(v);
                    let (scaled_sign, scaled_significand, scaled_e) = parts(s);
                    assert!(sign == scaled_sign && significand == scaled_significand);
                    moved[i] = Some(scaled_e - e);
                }
            }
            if let [Some(a), Some(b)] = moved {
                assert!(a == b);
            }
            let (larger, smaller) = (sx.abs().max(sy.abs()), sx.abs().min(sy.abs()));
            assert!(larger >= 1.0);
            let (big, small) = (x.abs().max(y.abs()), x.abs().min(y.abs()));
            if small == 0.0 || exponent(big) - exponent(small) <= 1022 {
                assert!(larger < 2.0);
            }
            if small != 0.0 {
                assert!(smaller.is_normal() || larger >= two_to(1023));
            }
            kani::cover!(larger < 2.0 && smaller > 0.0, "two parts, the larger in [1, 2)");
            kani::cover!(larger >= 2.0, "raised");
            kani::cover!(!smaller.is_normal() && smaller > 0.0, "raised to the top, the smaller part still subnormal");
        }
        None => {
            assert!(false, "a way is always scaled");
        }
    }
}

/// `way` between any two finite points: finite (no part passes the largest number); never the wrong way along an axis; the difference itself where that is a
/// number, so 0 on an axis exactly where the points are equal on it; where it is halved, 0 on an axis only for points within two of the smallest numbers (5E-324)
/// of each other on it (half of an odd number of them rounds to an even one: limit (b) of `clip`'s doc); and not (0, 0) for two points that differ.
#[kani::proof]
fn the_way_is_finite_never_points_back_and_is_nothing_only_between_equal_points() {
    let (a, b) = (Point::new(any_finite(), any_finite()), Point::new(any_finite(), any_finite()));
    let (dx, dy) = way(a, b);
    assert!(dx.is_finite() && dy.is_finite());
    assert!(!(dx > 0.0) || b.x > a.x);
    assert!(!(dx < 0.0) || b.x < a.x);
    assert!(!(dy > 0.0) || b.y > a.y);
    assert!(!(dy < 0.0) || b.y < a.y);
    let halved = !(b.x - a.x).is_finite() || !(b.y - a.y).is_finite();
    if halved {
        assert!(dx != 0.0 || (b.x - a.x).abs() <= 2.0 * UNIT);
        assert!(dy != 0.0 || (b.y - a.y).abs() <= 2.0 * UNIT);
    } else {
        assert!((dx == 0.0) == (a.x == b.x) && (dy == 0.0) == (a.y == b.y));
    }
    if a != b {
        assert!(dx != 0.0 || dy != 0.0);
    }
    kani::cover!(halved, "halved");
    kani::cover!(!halved && a != b, "the whole difference");
    kani::cover!(halved && dx == 0.0 && a.x != b.x, "an odd unit halved away");
}

/// The distance from a place on one axis to the window's range on it, worked the plain way by clamping.
fn clamped_gap(v: f64, low: f64, high: f64) -> f64 {
    (v - v.clamp(low, high)).abs()
}

/// `nearer` along x: for two points at any places across (and heights in the window), it gives the point nearer the window by the plain distance, worked by
/// clamping, and the first when they are as near.
#[kani::proof]
fn the_point_nearer_the_window_across_is_the_one_worked_from() {
    let (ax, bx) = (any_finite(), any_finite());
    let (a, b) = (Point::new(ax, 0.5), Point::new(bx, -1.5));
    let n = nearer(a, b, W);
    if clamped_gap(bx, W.x0, W.x1) < clamped_gap(ax, W.x0, W.x1) {
        assert!(n == b);
    } else {
        assert!(n == a);
    }
    kani::cover!(ax < W.x0 && bx > W.x1 && n == b, "opposite sides, the second nearer");
    kani::cover!(ax < W.x0 && bx > W.x1 && n == a, "opposite sides, the first nearer");
    kani::cover!((W.x0..=W.x1).contains(&ax) && (W.x0..=W.x1).contains(&bx), "both in the window");
}

/// `nearer` along y, the same.
#[kani::proof]
fn the_point_nearer_the_window_up_and_down_is_the_one_worked_from() {
    let (ay, by) = (any_finite(), any_finite());
    let (a, b) = (Point::new(0.5, ay), Point::new(-0.5, by));
    let n = nearer(a, b, W);
    if clamped_gap(by, W.y0, W.y1) < clamped_gap(ay, W.y0, W.y1) {
        assert!(n == b);
    } else {
        assert!(n == a);
    }
    kani::cover!(ay < W.y0 && by > W.y1 && n == b, "opposite sides, the second nearer");
    kani::cover!(ay > W.y1 && by > W.y1 && n == a, "one side, the first nearer");
}

/// The laws of `along` for any edge, place and step that is not 0, at the scale `down`: a number, never NaN; 0 where the edge is the place, and otherwise of the
/// sign of the distance over the step (or 0, where the quotient is under the smallest number); and finite for a step of 1 or more, which the larger part of a
/// direction is.
fn along_laws(edge: f64, place: f64, step: f64, down: f64) -> f64 {
    let at = along(edge, place, step, down);
    assert!(!at.is_nan());
    if edge == place {
        assert!(at == 0.0);
    } else if (edge > place) == (step > 0.0) {
        assert!(at >= 0.0);
    } else {
        assert!(at <= 0.0);
    }
    if step.abs() >= 1.0 {
        assert!(at.is_finite());
    }
    at
}

/// `along` at full size, as `clip` calls it there (no number over a quarter of the largest), for any edge, place and step: `along_laws`.
#[kani::proof]
fn along_at_full_size_has_the_sign_of_the_distance_and_is_finite_for_the_larger_step() {
    let (edge, place, step) = (any_finite(), any_finite(), any_finite());
    kani::assume(step != 0.0 && edge.abs() <= f64::MAX / 4.0 && place.abs() <= f64::MAX / 4.0);
    let at = along_laws(edge, place, step, 1.0);
    kani::cover!(at.is_infinite(), "past the largest number, for a small step");
    kani::cover!(at.is_finite() && at != 0.0, "a distance");
}

/// `along` at a quarter of the scale, for any edge, place and step, its three ways included (the difference past the largest number, only the quotient past it at
/// full size, neither): `along_laws`.
#[kani::proof]
fn along_at_a_quarter_has_the_sign_of_the_distance_and_is_finite_for_the_larger_step() {
    let (edge, place, step) = (any_finite(), any_finite(), any_finite());
    kani::assume(step != 0.0);
    let at = along_laws(edge, place, step, 0.25);
    kani::cover!(!(edge - place).is_finite(), "a difference past the largest number");
    kani::cover!((edge - place).is_finite() && !((edge - place) / step).is_finite(), "a quotient past it at full size");
    kani::cover!(at.is_infinite(), "past the largest number, for a small step");
}

/// Two edges `low` <= `high` keep their order along a step from `place`: the lower edge is not after the higher one going up the step, nor before it going down,
/// so that a line that crosses the window is not dropped for its ends' being out of order.
fn along_keeps_order(low: f64, high: f64, place: f64, step: f64, down: f64) {
    let (a, b) = (along(low, place, step, down), along(high, place, step, down));
    if step > 0.0 {
        assert!(a <= b);
    } else {
        assert!(a >= b);
    }
}

/// The order of two edges at full size, from the origin along a step of 1.5: any two edges up to a quarter of the largest number.
#[kani::proof]
fn along_keeps_the_order_of_two_edges_at_full_size() {
    let (low, high) = (any_finite(), any_finite());
    kani::assume(low <= high && low.abs() <= f64::MAX / 4.0 && high.abs() <= f64::MAX / 4.0);
    along_keeps_order(low, high, 0.0, 1.5, 1.0);
    kani::cover!(low < 0.0 && high > 0.0, "the two sides of the place");
}

/// The order of two edges at a quarter of the scale, from a place at minus half the largest number along a step of 1: the difference from the place passes the
/// largest number for an edge from about half of it up, so the two edges can take the two ways of `along`. (A step of 1.5 here took 133 s on a busy machine, and 1
/// takes 8: the order of a quotient by 1.5 is the full-size harness's.)
#[kani::proof]
fn along_keeps_the_order_of_two_edges_where_a_difference_passes_the_largest_number() {
    let (low, high) = (any_finite(), any_finite());
    kani::assume(low <= high);
    along_keeps_order(low, high, f64::MIN / 2.0, 1.0, 0.25);
    kani::cover!(low < f64::MAX / 4.0 && high == f64::MAX, "one difference past the largest number and the other not");
}

/// The order of two edges at a quarter of the scale, from the origin along the smallest normal step: the quotient passes the largest number at full size for an edge
/// from about 4 up, so the two edges can take the two ways of `along`.
#[kani::proof]
fn along_keeps_the_order_of_two_edges_where_a_quotient_passes_the_largest_number() {
    let (low, high) = (any_finite(), any_finite());
    kani::assume(low <= high);
    along_keeps_order(low, high, 0.0, f64::MIN_POSITIVE, 0.25);
    kani::cover!(low == 1.0 && high == 8.0, "one quotient past the largest number at full size and the other not");
}

// ---- clip -------------------------------------------------------------------------------------------------------------------

/// INFLINE contract: points or a window that are not finite numbers give `None`. One-sided: a `clip` that always gave `None` would pass (the harnesses below are
/// the other side).
#[kani::proof]
#[kani::unwind(9)] // the harness walks its 8 inputs; `clip` loops twice
fn clip_refuses_non_finite_input() {
    let n: [f64; 8] = kani::any();
    let line = InfiniteLine { p0: Point::new(n[0], n[1]), p1: Point::new(n[2], n[3]) };
    let rect = Rect::new(n[4], n[5], n[6], n[7]);
    kani::assume(!n.iter().all(|v| v.is_finite()));
    assert!(line.clip(rect).is_none());
    kani::cover!(n.iter().any(|v| v.is_nan()), "a NaN");
    kani::cover!(n.iter().any(|v| v.is_infinite()), "an infinity");
    kani::cover!(n[4..].iter().all(|v| v.is_finite()) && !n[..4].iter().all(|v| v.is_finite()), "a window of numbers and a line that is not");
}

/// A level line at height `c` through the places `a` and `b` (not equal) across, cut by `W`: kept exactly when its height is in the window's, then cut exactly at the
/// window's two sides in the order of the points, at its own height to the last bit (no number over a quarter of the largest) or to two of the smallest numbers (at
/// a quarter of the scale, where a subnormal height loses its last bits), in the window.
fn level(a: f64, b: f64, c: f64) -> Option<Line> {
    let cut = InfiniteLine { p0: Point::new(a, c), p1: Point::new(b, c) }.clip(W);
    match cut {
        Some(cut) => {
            assert!((W.y0..=W.y1).contains(&c));
            let (first, second) = if b > a { (W.x0, W.x1) } else { (W.x1, W.x0) };
            assert!(cut.p0.x == first && cut.p1.x == second);
            assert!((cut.p0.y - c).abs() <= 2.0 * UNIT && (cut.p1.y - c).abs() <= 2.0 * UNIT);
            if a.abs().max(b.abs()) <= f64::MAX / 4.0 {
                assert!(cut.p0.y == c && cut.p1.y == c);
            }
            assert!((W.y0..=W.y1).contains(&cut.p0.y) && (W.y0..=W.y1).contains(&cut.p1.y));
        }
        None => {
            assert!(!(W.y0..=W.y1).contains(&c));
        }
    }
    cut
}

/// An upright line at the place `c` through the heights `a` and `b`: the same as `level`, on the other axis.
fn upright(a: f64, b: f64, c: f64) -> Option<Line> {
    let cut = InfiniteLine { p0: Point::new(c, a), p1: Point::new(c, b) }.clip(W);
    match cut {
        Some(cut) => {
            assert!((W.x0..=W.x1).contains(&c));
            let (first, second) = if b > a { (W.y0, W.y1) } else { (W.y1, W.y0) };
            assert!(cut.p0.y == first && cut.p1.y == second);
            assert!((cut.p0.x - c).abs() <= 2.0 * UNIT && (cut.p1.x - c).abs() <= 2.0 * UNIT);
            if a.abs().max(b.abs()) <= f64::MAX / 4.0 {
                assert!(cut.p0.x == c && cut.p1.x == c);
            }
            assert!((W.x0..=W.x1).contains(&cut.p0.x) && (W.x0..=W.x1).contains(&cut.p1.x));
        }
        None => {
            assert!(!(W.x0..=W.x1).contains(&c));
        }
    }
    cut
}

/// A level line from any start across through (0.5, 1), a point of the window: `level`'s laws (the start can be any size, so the scale can be quartered, and the
/// line is worked from either point).
#[kani::proof]
fn a_level_line_from_any_start_is_cut_exactly_at_the_window_sides() {
    let a = any_finite();
    kani::assume(a != 0.5);
    let cut = level(a, 0.5, 1.0);
    kani::cover!(cut.is_some() && a < 0.5, "rightward");
    kani::cover!(cut.is_some() && a > 0.5, "leftward");
    kani::cover!(a.abs() > f64::MAX / 4.0, "at a quarter of the scale");
}

/// A level line at any height up to a quarter of the largest number, through the places 5E-324 and 0: `level`'s laws, kept exactly when its height is in the
/// window's.
#[kani::proof]
fn a_level_line_at_any_height_is_cut_exactly_at_the_window_sides_or_missed() {
    let c = any_finite();
    kani::assume(c.abs() <= f64::MAX / 4.0);
    let cut = level(UNIT, 0.0, c);
    kani::cover!(cut.is_some() && c != 0.0 && c.abs() < f64::MIN_POSITIVE, "a subnormal height");
    kani::cover!(cut.is_none() && c > W.y1, "above the window");
    kani::cover!(cut.is_none() && c < W.y0, "below the window");
}

/// A level line at any height up to a quarter of the largest number, through the two ends of the numbers (the way halved, the scale quartered): `level`'s laws.
#[kani::proof]
fn a_level_line_at_any_height_through_the_ends_of_the_numbers_is_cut_or_missed() {
    let c = any_finite();
    kani::assume(c.abs() <= f64::MAX / 4.0);
    let cut = level(f64::MIN, f64::MAX, c);
    kani::cover!(cut.is_some() && c != 0.0 && c.abs() < f64::MIN_POSITIVE, "a subnormal height that loses its last bits");
    kani::cover!(cut.is_none(), "missed");
}

/// An upright line from any start up or down through (1, 0.5), a point of the window: `upright`'s laws.
#[kani::proof]
fn an_upright_line_from_any_start_is_cut_exactly_at_the_window_top_and_bottom() {
    let a = any_finite();
    kani::assume(a != 0.5);
    let cut = upright(a, 0.5, 1.0);
    kani::cover!(cut.is_some() && a < 0.5, "upward");
    kani::cover!(cut.is_some() && a > 0.5, "downward");
    kani::cover!(a.abs() > f64::MAX / 4.0, "at a quarter of the scale");
}

/// The line through `p0` and `p1`, which is y = k x for a power of two `k` of either sign, cut by `rect` (in order, its edges 0 or of a normal size that can be
/// doubled, so the harness's chord is exact). Kept when its exact chord is longer than `tol`, 4 roundings of the largest of the numbers given (2^-50 of it), and
/// dropped when it misses by more than `tol`; and where it is kept, both ends finite, in the window, each on an edge, on the line to 4 `tol` (across the slope),
/// and, where the chord is longer than `tol`, at the chord's two ends to `tol`, from `p0`'s side to `p1`'s.
fn fixed_line(p0: Point, p1: Point, k: f64, rect: Rect) -> Option<Line> {
    let inv = 1.0 / k;
    let (from_y, to_y) = if k > 0.0 { (rect.y0 * inv, rect.y1 * inv) } else { (rect.y1 * inv, rect.y0 * inv) };
    let (lo, hi) = (rect.x0.max(from_y), rect.x1.min(to_y));
    let largest = [p0.x, p0.y, p1.x, p1.y, rect.x0, rect.y0, rect.x1, rect.y1].iter().fold(0.0f64, |m, v| m.max(v.abs()));
    let tol = largest * (4.0 * f64::EPSILON);
    let cut = InfiniteLine { p0, p1 }.clip(rect);
    match cut {
        Some(cut) => {
            for end in [cut.p0, cut.p1] {
                assert!(end.is_finite());
                assert!(end.x >= rect.x0 && end.x <= rect.x1 && end.y >= rect.y0 && end.y <= rect.y1);
                assert!(end.x == rect.x0 || end.x == rect.x1 || end.y == rect.y0 || end.y == rect.y1);
                assert!((end.y - k * end.x).abs() <= tol * 4.0);
            }
            assert!(lo <= hi + tol);
            if hi - lo > tol {
                let (first, second) = if p1.x > p0.x { (lo, hi) } else { (hi, lo) };
                assert!((cut.p0.x - first).abs() <= tol && (cut.p1.x - second).abs() <= tol);
            }
        }
        None => {
            assert!(hi - lo <= tol);
        }
    }
    kani::cover!(cut.is_some() && hi - lo > tol, "a chord");
    kani::cover!(cut.is_none(), "a miss");
    cut
}

/// An edge 0 or of a normal size that can be doubled (2^-1020 to a quarter of the largest number, either sign).
fn any_edge() -> f64 {
    let w = any_finite();
    kani::assume(w == 0.0 || (w.abs() >= f64::MIN_POSITIVE * 4.0 && w.abs() <= f64::MAX / 4.0));
    w
}

/// The line y = 2x, from the origin up, cut by the window with its left side at any place: `fixed_line`'s laws.
#[kani::proof]
fn a_steep_line_is_cut_at_its_chord_by_a_window_with_a_moving_left_side() {
    let w = any_edge();
    kani::assume(w <= W.x1);
    let cut = fixed_line(Point::new(0.0, 0.0), Point::new(1.0, 2.0), 2.0, Rect::new(w, W.y0, W.x1, W.y1));
    kani::cover!(cut.is_some_and(|c| c.p0.x == w), "entering on the moving side");
}

/// The line y = -x/2, from (1, -1/2) to the origin, cut by the window with its bottom at any height: `fixed_line`'s laws.
#[kani::proof]
fn a_shallow_falling_line_is_cut_at_its_chord_by_a_window_with_a_moving_bottom() {
    let w = any_edge();
    kani::assume(w <= W.y1);
    let cut = fixed_line(Point::new(1.0, -0.5), Point::new(0.0, 0.0), -0.5, Rect::new(W.x0, w, W.x1, W.y1));
    kani::cover!(cut.is_some_and(|c| c.p0.y == w), "entering on the moving side");
}

/// The line y = 4x, from (1, 4) down to the origin, cut by the window with its right side at any place: `fixed_line`'s laws. (It is steeper than the others so
/// that it can miss: y = 2x crosses every such window, and a cover that no input reaches fails the run.)
#[kani::proof]
fn a_steeper_line_going_down_is_cut_at_its_chord_by_a_window_with_a_moving_right_side() {
    let w = any_edge();
    kani::assume(w >= W.x0);
    let cut = fixed_line(Point::new(1.0, 4.0), Point::new(0.0, 0.0), 4.0, Rect::new(W.x0, W.y0, w, W.y1));
    kani::cover!(cut.is_some_and(|c| c.p0.x == w), "entering on the moving side");
}

/// The line y = -x/2, from the origin on, cut by the window with its top at any height: `fixed_line`'s laws.
#[kani::proof]
fn a_shallow_falling_line_is_cut_at_its_chord_by_a_window_with_a_moving_top() {
    let w = any_edge();
    kani::assume(w >= W.y0);
    let cut = fixed_line(Point::new(0.0, 0.0), Point::new(1.0, -0.5), -0.5, Rect::new(W.x0, W.y0, W.x1, w));
    kani::cover!(cut.is_some_and(|c| c.p0.y == w), "entering on the moving side");
}

/// The diagonal y = x through the origin and (1, 1), cut by the window with its left side at any finite place (any size, so the scale can be quartered): kept
/// exactly when the side is at 2 or less, then leaving at (2, 2) exactly, and entering on the moving side at its own height to a rounding where the side is at -2 or
/// over, at (-2, -2) exactly otherwise.
#[kani::proof]
fn a_diagonal_is_cut_where_it_crosses_a_window_with_a_moving_left_side() {
    let w = any_finite();
    let line = InfiniteLine { p0: Point::new(0.0, 0.0), p1: Point::new(1.0, 1.0) };
    match line.clip(Rect::new(w, W.y0, W.x1, W.y1)) {
        Some(cut) => {
            assert!(w <= 2.0);
            assert!(cut.p1 == Point::new(2.0, 2.0));
            if w >= W.y0 {
                assert!(cut.p0.x == w && (cut.p0.y - w).abs() <= f64::EPSILON);
                assert!((W.y0..=W.y1).contains(&cut.p0.y));
            } else {
                assert!(cut.p0 == Point::new(-2.0, -2.0));
            }
            kani::cover!(w > 0.0 && w < 1.0, "worked from the second point");
            kani::cover!(w < W.y0, "entering at the bottom");
            kani::cover!(w < f64::MIN / 4.0, "at a quarter of the scale");
        }
        None => {
            assert!(w > 2.0);
            kani::cover!(w > W.x1, "a window given the wrong way round");
            kani::cover!(w <= W.x1, "a window the line misses");
        }
    }
}

/// The chord of the diagonal y = x in `W` runs from (-1, -1), on the left side, to (2, 2), on the top. The line through (`t`, `t`) and the origin is kept, and cut
/// there from the first point's side to the origin's, each end exactly on its edge and within 16 roundings of 1 (16 `EPSILON`) of the chord's end along it.
fn diagonal_through(t: f64) -> Line {
    let cut = InfiniteLine { p0: Point::new(t, t), p1: Point::new(0.0, 0.0) }.clip(W);
    let tol = 16.0 * f64::EPSILON;
    match cut {
        Some(cut) => {
            let (top, left) = if t > 0.0 { (cut.p0, cut.p1) } else { (cut.p1, cut.p0) };
            assert!(top.y == W.y1 && (top.x - 2.0).abs() <= tol && top.x <= W.x1);
            assert!(left.x == W.x0 && (left.y + 1.0).abs() <= tol && left.y >= W.y0);
            cut
        }
        None => {
            assert!(false, "a line through the window is kept");
            Line::new(Point::new(0.0, 0.0), Point::new(0.0, 0.0))
        }
    }
}

/// A diagonal through (t, t) for every t from 1 up to 2 (points of the window, so the line is worked from it, and every distance and the direction are symbolic):
/// kept, and cut from the top, exactly on it, to the left side, exactly on it, both ends in the window. Not proved here: how near the chord's ends they are (the law of
/// `diagonal_through` ran past 300 s over this family; it is proved for the powers of two below).
#[kani::proof]
fn a_diagonal_through_any_point_from_one_to_two_is_kept_and_cut_on_the_top_and_the_left_side() {
    let t = any_finite();
    kani::assume((1.0..2.0).contains(&t));
    match (InfiniteLine { p0: Point::new(t, t), p1: Point::new(0.0, 0.0) }).clip(W) {
        Some(cut) => {
            assert!(cut.p0.y == W.y1 && cut.p1.x == W.x0);
            assert!((W.x0..=W.x1).contains(&cut.p0.x) && (W.y0..=W.y1).contains(&cut.p1.y));
            kani::cover!(t > 1.5, "the upper half");
        }
        None => {
            assert!(false, "a line through the window is kept");
        }
    }
}

/// A power of two of a normal size and either sign, chosen by its bits.
fn any_power() -> f64 {
    let (field, negative): (u64, bool) = (kani::any(), kani::any());
    kani::assume((1..=2046).contains(&field));
    f64::from_bits((u64::from(negative) << 63) | (field << 52))
}

/// A power of two of a normal size whose exponent field is from `lo` to `hi`, of the sign given.
fn any_power_in(lo: u64, hi: u64, negative: bool) -> f64 {
    let field: u64 = kani::any();
    kani::assume(field >= lo && field <= hi);
    f64::from_bits((u64::from(negative) << 63) | (field << 52))
}

// A diagonal through (t, t) for every power of two t of either sign up to a quarter of the largest number (in the window, so worked from it, and outside it, worked from the
// origin): `diagonal_through`. The family is proved in six harnesses, by the sign of t and three bands of its exponent field: up to 960 (t up to 2^-63),
// 961 to 1024 (2^-62 to 2, in the window), and 1025 to 2044 (4 up to 2^1021). As one harness it took 168 to
// 187 s at load 23 to 36 and timed out at 300 s in a gate at load 40 to 48; the six took 13 to 66 s at load 25.

/// The diagonal through a positive power of two of an exponent field up to 960.
#[kani::proof]
fn a_diagonal_through_a_tiny_positive_power_of_two_is_kept_and_cut_at_its_chord() {
    let cut = diagonal_through(any_power_in(1, 960, false));
    kani::cover!(cut.p0.y == W.y1, "from above");
}

/// The diagonal through a positive power of two from 2^-62 to 2, in the window.
#[kani::proof]
fn a_diagonal_through_a_positive_power_of_two_in_the_window_is_kept_and_cut_at_its_chord() {
    let cut = diagonal_through(any_power_in(961, 1024, false));
    kani::cover!(cut.p0.y == W.y1, "from above");
}

/// The diagonal through a positive power of two from 4 to 2^1021, outside the window.
#[kani::proof]
fn a_diagonal_through_a_large_positive_power_of_two_is_kept_and_cut_at_its_chord() {
    let cut = diagonal_through(any_power_in(1025, 2044, false));
    kani::cover!(cut.p0.y == W.y1, "from above");
}

/// The diagonal through a negative power of two of an exponent field up to 960.
#[kani::proof]
fn a_diagonal_through_a_tiny_negative_power_of_two_is_kept_and_cut_at_its_chord() {
    let cut = diagonal_through(any_power_in(1, 960, true));
    kani::cover!(cut.p0.x == W.x0, "from below");
}

/// The diagonal through a negative power of two from -2^-62 to -2 (in the window up to -1).
#[kani::proof]
fn a_diagonal_through_a_negative_power_of_two_near_the_window_is_kept_and_cut_at_its_chord() {
    let cut = diagonal_through(any_power_in(961, 1024, true));
    kani::cover!(cut.p0.x == W.x0, "from below");
}

/// The diagonal through a negative power of two from -4 to -2^1021, outside the window.
#[kani::proof]
fn a_diagonal_through_a_large_negative_power_of_two_is_kept_and_cut_at_its_chord() {
    let cut = diagonal_through(any_power_in(1025, 2044, true));
    kani::cover!(cut.p0.x == W.x0, "from below");
}

/// A diagonal through (t, t) for every power of two t over a quarter of the largest number, of either sign (the scale quartered): `diagonal_through`.
#[kani::proof]
fn a_diagonal_through_a_power_of_two_past_a_quarter_of_the_largest_number_is_kept() {
    let t = any_power();
    kani::assume(t.abs() > f64::MAX / 4.0);
    let cut = diagonal_through(t);
    kani::cover!(t > 0.0 && cut.p0.y == W.y1, "from above");
    kani::cover!(t < 0.0 && cut.p0.x == W.x0, "from below");
}

/// A line from a far point (1E300, y), at any height, through (0.5, 0.25), a point of the window: never lost, and cut with both ends in the window, each on an edge.
#[kani::proof]
fn a_line_from_a_far_point_at_any_height_through_the_window_is_kept() {
    let y = any_finite();
    let line = InfiniteLine { p0: Point::new(1e300, y), p1: Point::new(0.5, 0.25) };
    match line.clip(W) {
        Some(cut) => {
            for end in [cut.p0, cut.p1] {
                assert!(end.x >= W.x0 && end.x <= W.x1 && end.y >= W.y0 && end.y <= W.y1);
                assert!(end.x == W.x0 || end.x == W.x1 || end.y == W.y0 || end.y == W.y1);
            }
            kani::cover!(cut.p0.x == W.x1, "from the right side");
            kani::cover!(cut.p0.y == W.y1, "from the top");
        }
        None => {
            assert!(false, "a line through the window is kept");
        }
    }
}
