//! The sine family and the angle functions.
//!
//! An angle is radians inside, and a number with no angle unit is radians in all but `RAD`. Nothing is snapped
//! The cosine of 90 degrees is the very small number a double gives.

use std::f64::consts::{PI, TAU};

use diacraft_units::{Measure, Quantity, Unit};

use crate::eval::{Evaluator, Site};
use crate::registry::{FnDef, eager};
use crate::value::{CellError, Value};

type Slots<'a> = &'a [Option<Value>];

/// A function of one number that gives a plain number: the sine family. An infinite result is `#NUM!`, so no infinity enters a document
/// (project rule).
fn plain(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>, f: fn(f64) -> f64) -> Value {
    match ev.arg_double(site, slots, 0) {
        Ok(q) => {
            let r = f(q.stored);
            if r.is_finite() { Value::num(r) } else { Value::Error(CellError::Num) }
        }
        Err(e) => e,
    }
}

fn sin(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    plain(ev, site, slots, f64::sin)
}

fn cos(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    plain(ev, site, slots, f64::cos)
}

fn tan(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    plain(ev, site, slots, f64::tan)
}

fn sinh(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    plain(ev, site, slots, f64::sinh)
}

fn cosh(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    plain(ev, site, slots, f64::cosh)
}

fn tanh(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    plain(ev, site, slots, f64::tanh)
}

/// An angle in the default angle unit, `PtgAngDft`: the radians stored, shown as `DA`.
fn angle(radians: f64) -> Value {
    Value::Number(Quantity::new(radians, Unit::AngleDefault))
}

fn acos(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    match ev.arg_double(site, slots, 0) {
        // Beyond -1 to 1 is `#VALUE!` by MS-VSDX, where the page says `#NUM!`; no clamp for a rounding error.
        Ok(q) if q.stored.abs() > 1.0 || q.stored.is_nan() => Value::Error(CellError::Value),
        Ok(q) => angle(q.stored.acos()),
        Err(e) => e,
    }
}

fn asin(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    match ev.arg_double(site, slots, 0) {
        Ok(q) if q.stored.abs() > 1.0 || q.stored.is_nan() => Value::Error(CellError::Value),
        Ok(q) => angle(q.stored.asin()),
        Err(e) => e,
    }
}

fn atan(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    match ev.arg_double(site, slots, 0) {
        Ok(q) => angle(q.stored.atan()),
        Err(e) => e,
    }
}

/// `ATAN2(y, x)`, `y` first. Plain `atan2` on doubles, a `y` of negative zero being a zero: pi on the negative
/// axis (project rule).
fn atan2(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    let y = match ev.arg_double(site, slots, 0) {
        Ok(q) => q.stored,
        Err(e) => return e,
    };
    let x = match ev.arg_double(site, slots, 1) {
        Ok(q) => q.stored,
        Err(e) => return e,
    };
    let y = if y == 0.0 { 0.0 } else { y };
    angle(y.atan2(x))
}

/// A whole turn added or taken until the angle is at least 0 and below 2 pi, in the argument's unit if it is an angle unit.
fn ang360(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    match ev.arg_double(site, slots, 0) {
        Ok(q) => {
            let mut r = q.stored - TAU * (q.stored / TAU).floor();
            // A tiny negative value rounds up to exactly 2 pi, which is outside the range.
            if r >= TAU || r.is_nan() {
                r = 0.0;
            }
            let unit = if q.unit.measure() == Measure::Angle { q.unit } else { Unit::AngleDefault };
            Value::Number(Quantity::new(r, unit))
        }
        Err(e) => e,
    }
}

/// `DEG(angle)` keeps the stored number and shows it in degrees.
fn deg(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    match ev.arg_double(site, slots, 0) {
        Ok(q) => Value::Number(Quantity::new(q.stored, Unit::Degree)),
        Err(e) => e,
    }
}

/// `RAD(angle)`: an angle unit keeps its stored value, anything else is read as degrees.
fn rad(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: Slots<'_>) -> Value {
    match ev.arg_double(site, slots, 0) {
        Ok(q) if q.unit.measure() == Measure::Angle => Value::Number(Quantity::new(q.stored, Unit::Radian)),
        Ok(q) => Value::Number(Quantity::new(q.stored * PI / 180.0, Unit::Radian)),
        Err(e) => e,
    }
}

pub(crate) static DEFS: &[FnDef] = &[
    eager("ACOS", 1, 1, acos),
    eager("ANG360", 1, 1, ang360),
    eager("ASIN", 1, 1, asin),
    eager("ATAN", 1, 1, atan),
    eager("ATAN2", 2, 2, atan2),
    eager("COS", 1, 1, cos),
    eager("COSH", 1, 1, cosh),
    eager("DEG", 1, 1, deg),
    eager("RAD", 1, 1, rad),
    eager("SIN", 1, 1, sin),
    eager("SINH", 1, 1, sinh),
    eager("TAN", 1, 1, tan),
    eager("TANH", 1, 1, tanh),
];
