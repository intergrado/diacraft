//! Arithmetic, rounding and bits: the 26 functions, and `MOD` and `_MOD`.
//!
//! Each gives back a number in the unit of its first argument where the page says `vNum`. **A function that is not linear works on the number shown in
//! the argument's unit** (`INT`, `INTUP`, `ROUND`, `TRUNC`, `MODULUS`, `LN`, `LOG10`: it, our rule, which no sample tests yet;  [`rules::NONLINEAR_ON_STORED`] flips it),
//! as `CEILING` does by the samples and `FLOOR` by our rule: internal units, inches for
//! a length, for `DL` or no unit. `SQRT` and `POW` are powers of the unit and read the same on both numbers, so they work on the stored value whatever the
//! flag says. The linear ones (`ABS`, the bit functions, `MOD`, `MAX`) work on the stored value. An argument
//! that cannot convert is `#VALUE!` (project rule), and an error argument comes back first.
//!
//! **The two remainders are exact.** `MODULUS` and the fold of an angle into a turn (`BOUND` of an angle, `GRAVITY`'s limits) are worked
//! from the exact remainder of the two numbers (`%`, an `fmod`, which loses no digit at any size) and moved by the divisor or the turn with one sum, and
//! not from `n - d * floor(n / d)`, whose product rounds at the extremes. The angle is reduced through the exact remainder by the `f64` turn, not to the
//! nearest representable angle: at a size where the spacing of doubles passes a turn, it is the angle the stored number is, from 0 up to but not
//! including `TAU`. A sum that rounds to the divisor, or to the turn, is not in the range: `MODULUS` gives the largest number below the divisor in size
//! (with its sign), and an angle gives 0. [`fold_modulo`] and [`fold_remainder`] are the steps after the remainder, which Kani proves for every remainder
//! (`kani_proofs.rs`).

use diacraft_units::{Quantity, Unit};

use crate::convert::{float_range, signed_int, unsigned_int};
use crate::eval::{Evaluator, Site};
use crate::ops;
use crate::registry::{FnDef, MANY, eager};
use crate::rules::{BAD_ARGUMENT_ERROR, NONLINEAR_ON_STORED, SIGN_FUZZ};
use crate::value::{CellError, Value};

type Slots<'a> = &'a [Option<Value>];

fn bad() -> Value {
    Value::Error(BAD_ARGUMENT_ERROR)
}

/// The number a non-linear function works on, by [`NONLINEAR_ON_STORED`]: the stored value, or the number shown in the argument's unit.
fn basis(q: Quantity) -> f64 {
    if NONLINEAR_ON_STORED { q.stored } else { q.shown() }
}

/// A result of a function on [`basis`], in the unit and dimension of `like`.
fn unbasis(like: Quantity, x: f64) -> Value {
    if x.is_nan() {
        return Value::Error(CellError::Num);
    }
    if NONLINEAR_ON_STORED {
        Value::Number(Quantity::with_dim(x, like.unit, like.dim()))
    } else {
        Value::Number(Quantity::from_shown(x, like.unit, like.dim()))
    }
}

/// A number the same as `like` in unit and dimension, with this stored value.
fn same_unit(like: Quantity, stored: f64) -> Value {
    if stored.is_nan() { Value::Error(CellError::Num) } else { Value::Number(Quantity::with_dim(stored, like.unit, like.dim())) }
}

/// Runs `f` on one argument by `vDouble`.
fn unary(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>, f: impl FnOnce(Quantity) -> Value) -> Value {
    match ev.arg_double(site, slots, 0) {
        Ok(q) => f(q),
        Err(e) => e,
    }
}

fn abs(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    unary(ev, site, slots, |q| same_unit(q, q.stored.abs()))
}

fn int(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    unary(ev, site, slots, |q| unbasis(q, basis(q).floor()))
}

fn intup(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    unary(ev, site, slots, |q| unbasis(q, basis(q).ceil()))
}

fn ln(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    unary(ev, site, slots, |q| if basis(q) <= 0.0 { Value::Error(CellError::Num) } else { unbasis(q, basis(q).ln()) })
}

fn log10(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    unary(ev, site, slots, |q| if basis(q) <= 0.0 { Value::Error(CellError::Num) } else { unbasis(q, basis(q).log10()) })
}

/// `SQRT`. **It works on the stored value whatever [`NONLINEAR_ON_STORED`] says**, as `POW` does: a root is a power of the unit, so the
/// number shown and the stored number give the same result when the unit's dimension carries the power (`SQRT(9 ft^2)` is 3 ft, 36 stored inches, either way),
/// and reading it through the number shown went wrong for the acre and the hectare, which are `Measure::Area` units that already hold square inches (their
/// scale is not raised to the dimension): `SQRT(1 acre)` came out as 6,272,640 stored inches and not 2,504.52. `ops::finish` is the one place that says what
/// an acre or a hectare becomes at a dimension other than 2 (`DL`), and `POW(1 acre, 0.5)` goes through it too.
fn sqrt(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    unary(ev, site, slots, |q| {
        // `unknown`: a negative value with an odd dimension. Project rule: `#NUM!`, the first of the two the page names.
        if q.stored < 0.0 {
            return Value::Error(CellError::Num);
        }
        let dim = i32::from(q.dim());
        if dim % 2 != 0 {
            return Value::Error(CellError::Dim);
        }
        ops::finish(q.stored.sqrt(), q.unit, dim / 2)
    })
}

fn pi(_: &mut Evaluator<'_, '_>, _: &Site<'_>, _: Slots<'_>) -> Value {
    Value::num(std::f64::consts::PI)
}

fn pow(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    let base = match ev.arg_double(site, slots, 0) {
        Ok(q) => q,
        Err(e) => return e,
    };
    match ev.arg_double(site, slots, 1) {
        Ok(exp) => ops::pow(base, exp),
        Err(e) => e,
    }
}

fn rand(ev: &mut Evaluator<'_, '_>, _: &Site<'_>, _: Slots<'_>) -> Value {
    ev.ctx.clock_touched = true;
    Value::num(ev.ctx.next_random())
}

/// The 16 bits an argument of a bit function is (`vUnsignedInt`).
fn bits(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>, i: usize) -> Result<u16, Value> {
    unsigned_int(ev.arg_double(site, slots, i)?)
}

fn bitand(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    match (bits(ev, site, slots, 0), bits(ev, site, slots, 1)) {
        (Ok(a), Ok(b)) => Value::num(f64::from(a & b)),
        (Err(e), _) | (_, Err(e)) => e,
    }
}

fn bitor(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    match (bits(ev, site, slots, 0), bits(ev, site, slots, 1)) {
        (Ok(a), Ok(b)) => Value::num(f64::from(a | b)),
        (Err(e), _) | (_, Err(e)) => e,
    }
}

fn bitxor(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    match (bits(ev, site, slots, 0), bits(ev, site, slots, 1)) {
        (Ok(a), Ok(b)) => Value::num(f64::from(a ^ b)),
        (Err(e), _) | (_, Err(e)) => e,
    }
}

fn bitnot(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    match bits(ev, site, slots, 0) {
        Ok(a) => Value::num(f64::from(!a)),
        Err(e) => e,
    }
}

/// A number made a whole multiple of `multiple`, by `step`, with a fuzz so that a quotient a rounding error left just off a whole number
/// (`0.3 / 0.1`) counts as that number: "a number already on a multiple stays there". The fuzz, a relative 1E-12, is ours.
fn quotient(n: f64, m: f64) -> f64 {
    let r = n / m;
    let whole = r.round();
    if (r - whole).abs() <= 1e-12 * whole.abs().max(1.0) { whole } else { r }
}

/// The number a result of [`to_multiple`] is: `x` shown in the unit and dimension `like` carries. Not [`unbasis`]: `CEILING` and `FLOOR` do not follow
/// [`NONLINEAR_ON_STORED`].
fn shown_result(like: Quantity, x: f64) -> Value {
    if x.is_nan() { Value::Error(CellError::Num) } else { Value::Number(Quantity::from_shown(x, like.unit, like.dim())) }
}

/// A `multiple` (or a `MODULUS` divisor) as a count of the unit `number` is shown in: a plain number counts as it is, and a `multiple` that
/// carries a unit of its own is a length, so its stored internal value is shown in `number`'s unit (internal units when `number` is in `DL` or has no unit).
///
/// **Two readings here are project rules, which no sample tests (B; check-list rows).** A `multiple` that is a reference to a typed
/// length cell is a quantity in `DL`, which is a unit of its own, so it counts as its stored inches shown in `number`'s unit, and not as a plain number.
/// A `multiple` of another measure than `number`'s (`CEILING(1 in, 1 deg)`) counts as its stored value shown in `number`'s unit, and is no error.
fn multiple_in_unit_of(number: Quantity, multiple: Quantity) -> f64 {
    if multiple.dim() == 0 { multiple.stored } else { Quantity::with_dim(multiple.stored, number.unit, number.dim()).shown() }
}

/// `CEILING` and `FLOOR`: `outward` steps away from zero, and otherwise toward it.
///
/// **They work on the number as shown in `number`'s unit, and a `multiple` with no unit counts in that unit** (`inferred-from-sample`:
/// `CEILING(Height+3 mm,1)` is 16 mm, stored as 0.6299 in, where rounding the stored inches gives 1 in). A `number` in `DL` or with no unit shows
/// internal units, so the multiple is inches. A missing `multiple` is 1 (or -1 for a negative number) of the unit shown. The rule is not
/// [`NONLINEAR_ON_STORED`]'s: the samples settle it for `CEILING`. **`FLOOR` follows `CEILING` by our own rule** (tagged `unknown`:
/// no file holds a `FLOOR` with a `multiple`), the two being worded alike; a public file with `FLOOR` given a unitless multiple of a length in
/// millimetres, and its cached value, would settle it. A `multiple` with a unit of its own, and a missing one, are our reading of the same rule (project
/// rules: see [`multiple_in_unit_of`]; a missing multiple, is 1 of the unit shown).
fn to_multiple(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>, outward: bool) -> Value {
    let number = match ev.arg_double(site, slots, 0) {
        Ok(q) => q,
        Err(e) => return e,
    };
    let n = number.shown();
    let m = if site.len() >= 2 {
        match ev.arg_double(site, slots, 1) {
            Ok(q) => multiple_in_unit_of(number, q),
            Err(e) => return e,
        }
    } else if n >= 0.0 {
        1.0
    } else {
        -1.0
    };
    // A zero in either argument gives 0, before the signs are compared (`unknown`: project rule).
    if n == 0.0 || m == 0.0 {
        return shown_result(number, 0.0);
    }
    if (n > 0.0) != (m > 0.0) {
        return Value::Error(CellError::Num);
    }
    let steps = quotient(n.abs(), m.abs());
    let steps = if outward { steps.ceil() } else { steps.floor() };
    let x = steps * m.abs();
    shown_result(number, if n < 0.0 { -x } else { x })
}

fn ceiling(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    to_multiple(ev, site, slots, true)
}

fn floor(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    to_multiple(ev, site, slots, false)
}

fn modulus(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    let number = match ev.arg_double(site, slots, 0) {
        Ok(q) => q,
        Err(e) => return e,
    };
    // The divisor with no unit counts in the number's unit, as a `CEILING` multiple does (our rule): 7 ft modulo 3 is 1 ft.
    let divisor = match ev.arg_double(site, slots, 1) {
        Ok(q) => match float_range(q) {
            Ok(stored) => {
                if NONLINEAR_ON_STORED {
                    stored
                } else {
                    multiple_in_unit_of(number, q)
                }
            }
            Err(e) => return e,
        },
        Err(e) => return e,
    };
    if divisor == 0.0 {
        return Value::Error(CellError::DivZero);
    }
    // The amount left after division, taking the divisor's sign.
    unbasis(number, floor_modulo(basis(number), divisor))
}

/// `n` modulo `d`, taking the divisor's sign: the amount left after division, from 0 up to but not including `d` in size, with the sign of `d`
/// NaN for a `d` of zero and for a number that is not finite, which `MODULUS` answers as `#NUM!` (a zero divisor it refused first).
///
/// **Through the exact remainder.** The page's rule is `n - d * floor(n / d)`, which agrees with this at the numbers the page works,
/// but rounds at the extremes: a small negative `n` made `d` itself (`-1E-20` modulo 1 was 1, and so was `-1E-17`, up to about
/// `-5.5E-17`), the product `d * floor` lost a divisor far below the number (`1E308` modulo `1E-308` was `-inf`), and `-2` modulo `3E38` was `3E38`.
/// `n % d` is exact (the remainder of two doubles is a double, of the sign of `n`, below `d` in size), and moving it a divisor when its sign
/// differs from `d`'s is one sum. That sum rounds up to the divisor when the remainder is far below it, and a remainder is below the divisor in
/// size, so that result is the largest number below `d` in size, with its sign.
pub(crate) fn floor_modulo(n: f64, d: f64) -> f64 {
    if !n.is_finite() || !d.is_finite() || d == 0.0 {
        return f64::NAN;
    }
    fold_modulo(n % d, d)
}

/// The step of [`floor_modulo`] after the remainder `r` of `n % d` (of `n`'s sign and below `d` in size): moved by `d` when its sign differs, and the
/// largest number below `d` in size when that sum rounds to `d`. Apart from the remainder so that a proof can state it for every `r`.
pub(crate) fn fold_modulo(r: f64, d: f64) -> f64 {
    if r == 0.0 {
        return 0.0;
    }
    if (r < 0.0) == (d < 0.0) {
        return r;
    }
    let moved = r + d;
    if moved.abs() >= d.abs() { d.abs().next_down().copysign(d) } else { moved }
}

/// `MOD` and `_MOD`: the remainder after truncating division, with its sign dropped (`community-report`). The first
/// argument's unit carries (`unknown`: project rule).
fn mod_call(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    let number = match ev.arg_double(site, slots, 0) {
        Ok(q) => q,
        Err(e) => return e,
    };
    let divisor = match ev.arg_double(site, slots, 1) {
        Ok(q) => q,
        Err(e) => return e,
    };
    if divisor.stored == 0.0 {
        return Value::Error(CellError::DivZero);
    }
    same_unit(number, (number.stored % divisor.stored).abs())
}

/// `ROUND` and `TRUNC` on a number: `digits` places right of the point (left when negative). `lowest` is the digit below which the
/// number comes back untouched (-9, -308) and 15 the digit above.
fn round_to(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>, lowest: i64, truncate: bool) -> Value {
    let number = match ev.arg_double(site, slots, 0) {
        Ok(q) => q,
        Err(e) => return e,
    };
    let digits = match ev.arg_double(site, slots, 1).and_then(signed_int) {
        Ok(d) => d,
        Err(e) => return e,
    };
    let x = basis(number);
    // Out of range, the number comes back as it is.
    if digits <= lowest || digits > 15 {
        return unbasis(number, x);
    }
    let apply = |r: f64| if truncate { r.trunc() } else { r.round() };
    let result = if digits >= 0 {
        let scale = 10f64.powi(digits as i32);
        let scaled = x * scale;
        if !scaled.is_finite() {
            // No overflow from scaling: a number that large has no digits to cut.
            x
        } else {
            apply(snap(scaled)) / scale
        }
    } else {
        let scale = 10f64.powi((-digits) as i32);
        if !scale.is_finite() { if truncate { 0.0 } else { x } } else { apply(snap(x / scale)) * scale }
    };
    unbasis(number, result)
}

/// A scaled number that a rounding error left a hair off a whole number is that whole number (`0.29 * 100` is 28.999999999999996).
fn snap(r: f64) -> f64 {
    let whole = r.round();
    if (r - whole).abs() <= 1e-12 * whole.abs().max(1.0) { whole } else { r }
}

fn round(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    round_to(ev, site, slots, -9, false)
}

fn trunc(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    round_to(ev, site, slots, -309, true)
}

fn sign(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    let number = match ev.arg_double(site, slots, 0) {
        Ok(q) => q,
        Err(e) => return e,
    };
    let fuzz = if site.len() >= 2 {
        match ev.arg_double(site, slots, 1) {
            Ok(q) => q.stored.abs(),
            Err(e) => return e,
        }
    } else {
        SIGN_FUZZ
    };
    let n = number.stored;
    Value::num(if n > fuzz {
        1.0
    } else if n < -fuzz {
        -1.0
    } else {
        0.0
    })
}

/// The numbers of every slot by `vDouble`.
fn all_doubles(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Result<Vec<Quantity>, Value> {
    (0..slots.len()).map(|i| ev.arg_double(site, slots, i)).collect()
}

fn max(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    extreme(ev, site, slots, |a, b| a > b)
}

fn min(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    extreme(ev, site, slots, |a, b| a < b)
}

/// The argument nearest an end of the line, comparing stored values; the first of equals wins, keeping its unit.
fn extreme(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>, better: impl Fn(f64, f64) -> bool) -> Value {
    let all = match all_doubles(ev, site, slots) {
        Ok(a) => a,
        Err(e) => return e,
    };
    let mut best: Option<Quantity> = None;
    for q in all {
        match best {
            Some(b) if !better(q.stored, b.stored) => {}
            _ => best = Some(q),
        }
    }
    best.map_or_else(bad, Value::Number)
}

fn sum(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    let all = match all_doubles(ev, site, slots) {
        Ok(a) => a,
        Err(e) => return e,
    };
    let total: f64 = all.iter().map(|q| q.stored).sum();
    // The unit of whichever argument has one first; `unknown`: its dimension too.
    match all.iter().find(|q| q.unit != Unit::Unitless) {
        Some(first) => ops::finish(total, first.unit, i32::from(first.dim())),
        None => ops::finish(total, Unit::Unitless, 0),
    }
}

fn magnitude(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    let all = match all_doubles(ev, site, slots) {
        Ok(a) => a,
        Err(e) => return e,
    };
    let [ca, a, cb, b] = all[..] else { return bad() };
    // `#DIM!` when the rise or the run is multidimensional.
    if a.dim() > 1 || a.dim() < 0 || b.dim() > 1 || b.dim() < 0 {
        return Value::Error(CellError::Dim);
    }
    let x = ca.stored * a.stored;
    let y = cb.stored * b.stored;
    Value::num((x * x + y * y).sqrt())
}

/// `BOUND(value, type, ignore, begin, end [, ignore, begin, end]...)`.
fn bound(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    let count = slots.len();
    // 2 + 3n arguments with n at least 1.
    if count < 5 || !(count - 2).is_multiple_of(3) {
        return bad();
    }
    // An empty first argument, as the page's size-limit example has it, is 0 until a write gives it a number (`unknown`).
    let value = if slots.first().is_some_and(Option::is_none) {
        Quantity::new(0.0, Unit::Unitless)
    } else {
        match ev.arg_double(site, slots, 0) {
            Ok(q) => q,
            Err(e) => return e,
        }
    };
    let kind = match ev.arg_double(site, slots, 1).and_then(unsigned_int) {
        Ok(k) => k,
        Err(e) => return e,
    };
    let mut ranges: Vec<(f64, f64)> = Vec::new();
    let mut at = 2;
    while at + 2 < count {
        let ignore = match ev.arg_bool(site, slots, at) {
            Ok(b) => b,
            Err(e) => return e,
        };
        let begin = match ev.arg_double(site, slots, at + 1) {
            Ok(q) => q.stored,
            Err(e) => return e,
        };
        let end = match ev.arg_double(site, slots, at + 2) {
            Ok(q) => q.stored,
            Err(e) => return e,
        };
        if !ignore {
            // A `begin` above its `end` is swapped (`unknown`: project rule).
            ranges.push(if begin <= end { (begin, end) } else { (end, begin) });
        }
        at += 3;
    }
    let v = value.stored;
    // The ranges are sorted and swept: a step a range, charged first.
    if !ev.steps(ranges.len()) {
        return Value::Error(crate::rules::LOOP_ERROR);
    }
    let bounded = match kind {
        0 => bound_inclusive(v, &ranges),
        1 => bound_exclusive(v, &ranges),
        2 => v,
        _ => return bad(),
    };
    // An angle is brought into 0 up to 2 pi; an infinite one has no turn and is 0.
    let bounded = if value.unit.measure() == diacraft_units::Measure::Angle {
        let turned = wrap_turn(bounded);
        if turned.is_nan() { 0.0 } else { turned }
    } else {
        bounded
    };
    same_unit(value, bounded)
}

/// `x` brought into 0 up to but not including a whole turn, for any finite `x`. NaN for a number that is not finite: it
/// has no turn count, and `BOUND` answers it as 0, as it always has.
///
/// **Through the exact remainder, and not the nearest angle in range.** `x % TAU` is the IEEE remainder of a truncating division, which is exact at
/// every size (the remainder of two doubles is a double): no digit is lost, whatever the size of `x`, where `x - TAU * floor(x / TAU)` rounds its
/// product and left the range from a few million radians up (`-9.3E-10` for `-6691341.02`, `-2.3E18` for `2E34`). So the answer is `x` modulo the
/// `f64` turn, `TAU`, as that number is: at a size where the spacing of doubles passes a turn, an angle has no turn count to keep, and this is the
/// angle the stored number is, nothing nearer or farther. A negative remainder is moved up a turn, and that sum rounds, to a whole turn when the
/// remainder is below half the spacing under `TAU` (4.4E-16): a turn is not in the range, and the angle a hair short of one is next to 0, so that
/// result is 0. A zero, a negative one too, is 0.
pub(crate) fn wrap_turn(x: f64) -> f64 {
    let turn = std::f64::consts::TAU;
    fold_remainder(x % turn, turn)
}

/// The step of [`wrap_turn`] after the remainder `r` (of the sign of the angle, below `turn` in size): `r` from 0 up to `turn`, a sum that rounds to
/// `turn` taken as 0. Apart from the remainder so that a proof can state it for every `r`.
pub(crate) fn fold_remainder(r: f64, turn: f64) -> f64 {
    if r == 0.0 {
        return 0.0;
    }
    if r > 0.0 {
        return r;
    }
    let up = r + turn;
    if up >= turn { 0.0 } else { up }
}

/// Type 0: the value if it lies in an active range, else the closest bound, the earliest of equals. No active range leaves the value
/// (`unknown`: project rule).
fn bound_inclusive(v: f64, ranges: &[(f64, f64)]) -> f64 {
    if ranges.iter().any(|&(b, e)| b <= v && v <= e) {
        return v;
    }
    let mut best: Option<(f64, f64)> = None;
    for &(b, e) in ranges {
        for candidate in [b, e] {
            let d = (candidate - v).abs();
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, candidate));
            }
        }
    }
    best.map_or(v, |(_, c)| c)
}

/// Whether each range lies inside another that is not identical to it, in `n log n`: the ranges sorted by `begin`, then by `end` from the
/// largest, are swept with the largest `end` of the ranges before them, an identical group taken together. (A test of each against all
/// the others is quadratic: 40,000 ranges took a second a cell.)
fn nested_flags(ranges: &[(f64, f64)]) -> Vec<bool> {
    let mut order: Vec<usize> = (0..ranges.len()).collect();
    order.sort_by(|&a, &b| ranges[a].0.total_cmp(&ranges[b].0).then(ranges[b].1.total_cmp(&ranges[a].1)));
    let mut flags = vec![false; ranges.len()];
    let mut max_end = f64::NEG_INFINITY;
    let mut i = 0;
    while i < order.len() {
        let Some(&first) = order.get(i) else { break };
        let range = ranges[first];
        let mut j = i;
        while order.get(j).is_some_and(|&k| ranges[k] == range) {
            j += 1;
        }
        let inside = max_end >= range.1;
        for &k in order.get(i..j).unwrap_or(&[]) {
            flags[k] = inside;
        }
        max_end = max_end.max(range.1);
        i = j;
    }
    flags
}

/// Type 1: the value if it lies in no active range, else the closest bound that is not a bound of a range nested inside another; exactly
/// halfway between a `begin` and an `end` it is the `end`. A value on a bound is outside the range (`unknown`: project rule).
fn bound_exclusive(v: f64, ranges: &[(f64, f64)]) -> f64 {
    if !ranges.iter().any(|&(b, e)| b < v && v < e) {
        return v;
    }
    let nested = nested_flags(ranges);
    let mut best: Option<(f64, bool, f64)> = None;
    for (i, &(b, e)) in ranges.iter().enumerate() {
        if nested.get(i).copied().unwrap_or(false) {
            continue;
        }
        for (candidate, is_end) in [(b, false), (e, true)] {
            let d = (candidate - v).abs();
            let better = match best {
                None => true,
                Some((bd, best_end, _)) => d < bd || (d == bd && is_end && !best_end),
            };
            if better {
                best = Some((d, is_end, candidate));
            }
        }
    }
    best.map_or(v, |(_, _, c)| c)
}

pub(crate) static DEFS: &[FnDef] = &[
    eager("ABS", 1, 1, abs),
    eager("BITAND", 2, 2, bitand),
    eager("BITNOT", 1, 1, bitnot),
    eager("BITOR", 2, 2, bitor),
    eager("BITXOR", 2, 2, bitxor),
    eager("BOUND", 5, MANY, bound),
    eager("CEILING", 1, 2, ceiling),
    eager("FLOOR", 1, 2, floor),
    eager("INT", 1, 1, int),
    eager("INTUP", 1, 1, intup),
    eager("LN", 1, 1, ln),
    eager("LOG10", 1, 1, log10),
    eager("MAGNITUDE", 4, 4, magnitude),
    eager("MAX", 1, MANY, max),
    eager("MIN", 1, MANY, min),
    eager("MOD", 2, 2, mod_call),
    eager("MODULUS", 2, 2, modulus),
    eager("PI", 0, 0, pi),
    eager("POW", 2, 2, pow),
    eager("RAND", 0, 0, rand),
    eager("ROUND", 2, 2, round),
    eager("SIGN", 1, 2, sign),
    eager("SQRT", 1, 1, sqrt),
    eager("SUM", 1, MANY, sum),
    eager("TRUNC", 2, 2, trunc),
    eager("_MOD", 2, 2, mod_call),
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::TAU;

    /// A stream of bit patterns, for numbers of every size.
    fn bits(seed: u64) -> impl FnMut() -> u64 {
        let mut s = seed;
        move || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            s
        }
    }

    /// A finite angle of any size, from the smallest subnormal to the largest number, lands from 0 up to but not including a turn. The old
    /// reduction left the range at sizes where its product rounds (`-9.3E-10` for `-6691341.02`, `-2.3E18` for `2E34`) and the `fmod`-based
    /// `rem_euclid` of `GRAVITY` gave a whole turn for a hair below 0.
    #[test]
    fn wrap_turn_lands_in_a_turn_for_every_finite_angle() {
        let mut next = bits(0x9e37_79b9_7f4a_7c15);
        let mut seen_small_negative = false;
        for _ in 0..400_000 {
            let x = f64::from_bits(next());
            if !x.is_finite() {
                continue;
            }
            let r = wrap_turn(x);
            assert!((0.0..TAU).contains(&r), "wrap_turn({x:e}) = {r:e}");
            assert!(!r.is_sign_negative(), "wrap_turn({x:e}) is a negative zero");
            seen_small_negative |= x < 0.0 && x > -1e-10;
        }
        assert!(seen_small_negative, "the sweep reaches the hair below zero");
        // The edges, by name.
        let hair = [f64::MIN_POSITIVE, 5e-324, 1e-20, 1e-17, 1e-16, 4.4e-16, 5.0e-16, 1e-15, 1e-10, 1.0];
        let multiples = [1.0, 2.0, 3.0, 4.0, 1e3, 1e6, 1e10].map(|k| k * TAU);
        for base in hair.iter().copied().chain(multiples) {
            for x in [base, -base, base.next_up(), -base.next_up(), base.next_down(), -base.next_down()] {
                let r = wrap_turn(x);
                assert!((0.0..TAU).contains(&r), "wrap_turn({x:e}) = {r:e}");
            }
        }
        for x in [f64::MAX, f64::MIN, -0.0, 0.0, TAU, -TAU, TAU.next_down(), -TAU.next_down(), TAU.next_up(), -TAU.next_up()] {
            let r = wrap_turn(x);
            assert!((0.0..TAU).contains(&r), "wrap_turn({x:e}) = {r:e}");
        }
    }

    /// The angle is the same angle: for sizes where `f64` still has room for a turn's worth of digits, the result differs from the angle by a
    /// whole number of turns, to the precision of the angle.
    #[test]
    fn wrap_turn_keeps_the_angle() {
        let mut next = bits(0x2545_f491_4f6c_dd1d);
        for _ in 0..100_000 {
            let x = ((next() % 20_000_000) as f64 - 10_000_000.0) / 1_000.0;
            let r = wrap_turn(x);
            let turns = (x - r) / TAU;
            assert!((turns - turns.round()).abs() < 1e-9, "wrap_turn({x}) = {r}: {turns} turns");
        }
        assert_eq!(wrap_turn(1.0), 1.0);
        assert_eq!(wrap_turn(-1.0), TAU - 1.0);
        assert_eq!(wrap_turn(TAU), 0.0);
        assert_eq!(wrap_turn(-TAU), 0.0);
    }

    /// A number that is not finite has no turn: NaN, which `BOUND` answers as 0 as before and `GRAVITY` takes as no angle between its limits.
    #[test]
    fn wrap_turn_has_no_answer_for_what_is_not_finite() {
        for x in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(wrap_turn(x).is_nan());
        }
    }
}
