//! `_ELLIPSE_THETA` and `_ELLIPSE_ECC`: the two reserved functions that fill the `C` and `D` cells of an
//! elliptical arc's row, the axis angle and the axis ratio of the arc's ellipse once its shape is resized. About
//! 418,000 saved cells call them; the rules were fitted on saved values and tested on repositories the fit did not use,
//! and the application describes neither.
//!
//! **The arguments** are the old angle `c` (radians), the old axis ratio `d`, the old width and height `w0` and `h0`, and the new width and height
//! `w1` and `h1`; `_ELLIPSE_ECC` takes a seventh, `a`, the angle to measure along (the row's `C`). Before the resize
//! the arc's ellipse has a half-axis of length `d` along `c` and one of length 1 across it. The resize scales x by `sx = w1 / w0` and y by
//! `sy = h1 / h0`, so the half-axes become `u = (sx·d·cos c, sy·d·sin c)` and `v = (-sx·sin c, sy·cos c)`.
//!
//! **`_ELLIPSE_THETA`** is the direction of the resized ellipse's longer axis, put between -pi and 0 (step 3), after two steps that stop first
//! A width or height collapsed to 0 with the other above 0 gives 0 (step 1), and a ratio of exactly 1, a circle whose angle means nothing, gives
//! 0 (step 2), both before anything is divided. **`_ELLIPSE_ECC`** is the ratio `along / across` of the resized ellipse measured along `a` and
//! across it, 1 for a collapsed size when `a` is 0.
//!
//! **Project rules, as the spec tags them.** An exact 0 from the axis step stays 0 and is not moved to -pi. An argument that is an error
//! comes back as the result (the registry does it), a wrong count is `#VALUE!` (the registry), and every other invalid argument is `#NUM!`: a
//! value that is not finite, a ratio or an old size of 0 or less, a negative new size, both new sizes 0, a collapsed size with a seventh argument
//! other than 0, and a scale that is not finite, and so is a sum of squares of the axes that overflows (the same rule for a finite scale whose
//! square is not: a finite wrong angle must not come out of an infinity). `_ELLIPSE_ECC` with an `across` of 0 outside the collapsed case is `#NUM!`. A seventh
//! argument that is no axis of the resized ellipse is measured as written all the same.
//!
//! **Cost.** Constant: a few dozen multiplications, a step for the call. The comparison of the two cells by the curve they describe together is in
//! `compare.rs` and uses [`theta`] and [`ecc`] as they are.

use std::f64::consts::PI;

use diacraft_units::{Quantity, Unit};

use crate::eval::{Evaluator, Site};
use crate::registry::{FnDef, eager};
use crate::value::{CellError, Value};

type Slots<'a> = &'a [Option<Value>];

/// The six numbers of a resize: `c`, `d`, `w0`, `h0`, `w1`, `h1`.
pub(crate) type Resize = [f64; 6];

/// Whether the arguments name a resize: every one finite, the ratio and the old sizes above 0, the new sizes not below 0 and not both 0.
fn valid(r: &Resize) -> bool {
    let [_, d, w0, h0, w1, h1] = *r;
    r.iter().all(|x| x.is_finite()) && d > 0.0 && w0 > 0.0 && h0 > 0.0 && w1 >= 0.0 && h1 >= 0.0 && !(w1 == 0.0 && h1 == 0.0)
}

/// Whether one new size is 0 and the other above 0 (step 1).
fn collapsed(r: &Resize) -> bool {
    let [_, _, _, _, w1, h1] = *r;
    (w1 == 0.0 && h1 > 0.0) || (h1 == 0.0 && w1 > 0.0)
}

/// The images `u` and `v` of the half-axes under the resize, or `None` when a scale or an image is not finite.
fn images(r: &Resize) -> Option<((f64, f64), (f64, f64))> {
    let [c, d, w0, h0, w1, h1] = *r;
    let (sx, sy) = (w1 / w0, h1 / h0);
    if !(sx.is_finite() && sy.is_finite()) {
        return None;
    }
    let (s, co) = c.sin_cos();
    let u = (sx * d * co, sy * d * s);
    let v = (-sx * s, sy * co);
    [u.0, u.1, v.0, v.1].iter().all(|x| x.is_finite()).then_some((u, v))
}

/// `_ELLIPSE_THETA(c, d, w0, h0, w1, h1)`: the direction of the resized ellipse's longer axis, from -pi to 0.
pub(crate) fn theta(r: &Resize) -> Result<f64, CellError> {
    if !valid(r) {
        return Err(CellError::Num);
    }
    // Steps 1 and 2 come before any division.
    let [_, d, ..] = *r;
    if collapsed(r) || d == 1.0 {
        return Ok(0.0);
    }
    let (u, v) = images(r).ok_or(CellError::Num)?;
    let qxx = u.0 * u.0 + v.0 * v.0;
    let qyy = u.1 * u.1 + v.1 * v.1;
    let qxy = u.0 * u.1 + v.0 * v.1;
    // The squares of a finite scale can overflow, and an infinity in one of them (or in two, whose difference is not a number) would give a finite wrong
    // angle: `_ELLIPSE_THETA(0.5,2,1,1E-307,10,2)` is about -1.96 with `qyy` infinite, and about -1.57 as it is. So none of the three may be infinite.
    if !(qxx.is_finite() && qyy.is_finite() && qxy.is_finite()) {
        return Err(CellError::Num);
    }
    let t = (2.0 * qxy).atan2(qxx - qyy) / 2.0;
    if !t.is_finite() {
        return Err(CellError::Num);
    }
    // A tiny positive `t` gives about -pi; an exact 0 stays 0 (project rule). Adding 0.0 turns a negative zero into a zero.
    Ok(if t > 0.0 { t - PI } else { t + 0.0 })
}

/// `_ELLIPSE_ECC(c, d, w0, h0, w1, h1, a)`: the ratio of the resized ellipse measured along `a` and across it.
pub(crate) fn ecc(r: &Resize, a: f64) -> Result<f64, CellError> {
    if !valid(r) || !a.is_finite() {
        return Err(CellError::Num);
    }
    if collapsed(r) {
        return if a == 0.0 { Ok(1.0) } else { Err(CellError::Num) };
    }
    let (u, v) = images(r).ok_or(CellError::Num)?;
    let (s, c) = a.sin_cos();
    let along = (u.0 * c + u.1 * s).hypot(v.0 * c + v.1 * s);
    let across = (-u.0 * s + u.1 * c).hypot(-v.0 * s + v.1 * c);
    if !(along.is_finite() && across.is_finite()) || across == 0.0 {
        return Err(CellError::Num);
    }
    let ratio = along / across;
    if ratio.is_finite() { Ok(ratio) } else { Err(CellError::Num) }
}

/// The six numbers of a resize from the first six slots of a call.
fn resize(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Result<Resize, Value> {
    let mut out = [0.0; 6];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = ev.arg_double(site, slots, i)?.stored;
    }
    Ok(out)
}

fn theta_function(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    let r = match resize(ev, site, slots) {
        Ok(r) => r,
        Err(e) => return e,
    };
    match theta(&r) {
        Ok(t) => Value::Number(Quantity::new(t, Unit::AngleDefault)),
        Err(e) => Value::Error(e),
    }
}

fn ecc_function(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    let r = match resize(ev, site, slots) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let a = match ev.arg_double(site, slots, 6) {
        Ok(q) => q.stored,
        Err(e) => return e,
    };
    match ecc(&r, a) {
        Ok(x) => Value::num(x),
        Err(e) => Value::Error(e),
    }
}

pub(crate) static DEFS: &[FnDef] = &[eager("_ELLIPSE_ECC", 7, 7, ecc_function), eager("_ELLIPSE_THETA", 6, 6, theta_function)];

#[cfg(test)]
mod tests {
    use super::*;

    /// A value that is not a number comes in as a number that is not finite: every function refuses it as `#NUM!`, never a panic or an infinity out.
    #[test]
    fn arguments_that_are_not_finite_are_invalid() {
        let good: Resize = [0.5, 2.0, 4.0, 2.0, 8.0, 2.0];
        assert!(theta(&good).is_ok() && ecc(&good, 0.0).is_ok());
        for i in 0..6 {
            for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                let mut r = good;
                r[i] = bad;
                assert_eq!(theta(&r), Err(CellError::Num), "argument {i} = {bad}");
                assert_eq!(ecc(&r, 0.0), Err(CellError::Num), "argument {i} = {bad}");
            }
        }
        for bad in [f64::NAN, f64::INFINITY] {
            assert_eq!(ecc(&good, bad), Err(CellError::Num));
        }
    }

    /// Whatever finite arguments come in, the answer is a finite number in range or `#NUM!`: -pi to 0 for the angle, above 0 for the ratio.
    #[test]
    fn finite_arguments_give_an_answer_in_range_or_num() {
        let values = [0.0, 1e-300, 1e-12, 0.3, 1.0, 2.5, 1e12, 1e300, -1.0, -0.0];
        for &c in &values {
            for &d in &values {
                for &w0 in &values {
                    for &w1 in &values {
                        let r: Resize = [c, d, w0, 1.5, w1, 0.7];
                        match theta(&r) {
                            Ok(t) => assert!((-PI..=0.0).contains(&t), "{r:?}: {t}"),
                            Err(e) => assert_eq!(e, CellError::Num, "{r:?}"),
                        }
                        match ecc(&r, c) {
                            Ok(x) => assert!(x.is_finite() && x >= 0.0, "{r:?}: {x}"),
                            Err(e) => assert_eq!(e, CellError::Num, "{r:?}"),
                        }
                    }
                }
            }
        }
    }
}
