//! Kani proof harnesses for the unit conversions: `to_internal` and `from_internal` of a [`Unit`], and `from_shown`, `shown` and `value_in` of
//! a [`Quantity`]. Run with `cargo xtask kani` (`docs/development.md`, Proofs).
//!
//! Every `f64` from `kani::any()` ranges over all 2^64 bit patterns, NaN and both infinities included, unless a harness assumes otherwise.
//!
//! **What is not proved here, and why.**
//! - Kani over-approximates `powi`: only `x.powi(0) == 1.0` is exact (`x.powi(1) == x` is not provable). Nothing can be proved through
//!   `Quantity::scale` for a length, an angle, a duration, a percent or a date, whose scale is `base_per_unit().powi(dim)`: the harnesses for
//!   `from_shown`, `shown` and `value_in` take the units whose scale is not a power, the acre, the hectare and no unit (`Measure::Area` and
//!   `Measure::None`). A harness that went through `powi` would also fail Kani's NaN check at the division, since the scale it returns may be 0.
//! - A round trip within a few units of the last place (`from_internal(to_internal(x))`, `shown(from_shown(x))`) does not finish: Kani
//!   must show a chain of multiplications and divisions of a symbolic number to be within a bound, and a SAT solver cannot (the hectare's
//!   alone ran past 120 s).
//! - The scale of a unit to a power (`63360^-128` underflows to 0, `63360^127` overflows to infinity) is not proved, as its `powi` is not: what is
//!   proved of `shown`, `value_in` and `from_shown` through it is that a zero and an infinity are themselves at any dimension, whatever `powi` gives.

use crate::{Measure, Quantity, Unit};

/// Any unit, by its place in `Unit::ALL`.
fn any_unit() -> Unit {
    let i: usize = kani::any();
    kani::assume(i < Unit::ALL.len());
    Unit::ALL[i]
}

/// Any unit whose scale needs no `powi`.
fn any_unit_without_power() -> Unit {
    let unit = any_unit();
    kani::assume(matches!(unit.measure(), Measure::Area | Measure::None));
    unit
}

/// Neither conversion makes a NaN of a number that is not one: the factors are finite and not zero, so an infinity stays an infinity.
#[kani::proof]
fn conversions_make_no_nan() {
    let (unit, x): (Unit, f64) = (any_unit(), kani::any());
    kani::assume(!x.is_nan());
    assert!(!unit.to_internal(x).is_nan());
    assert!(!unit.from_internal(x).is_nan());
    kani::cover!(x.is_infinite(), "an infinity");
}

/// A finite number whose true value fits a double gives a finite result, in either direction. "Fits" is stated with a hundredth to spare for
/// the rounding of the limit: `x` times the unit's factor, or over it, is at most 0.99 of the largest number, and the limit is a constant the compiler
/// works out (a unit's factor is a constant: one harness for each unit of the list, as a symbolic choice of unit makes the multiplier symbolic, which a
/// solver cannot do in time, and a loop over all 36 units timed out at 300 s). `to_internal` multiplied by the numerator before it divided, and
/// `from_internal` the other way round, and lost the result to an infinity in the middle (`Hectare.to_internal(2.6e298)` was infinity, the value
/// is 4.0e305). The list takes the factors of every kind: above and below 1, an exact one and one that is a quotient of two large numbers.
fn conversions_stay_finite_when_the_true_value_fits(unit: Unit) {
    let x: f64 = kani::any();
    kani::assume(x.is_finite());
    let factor = unit.base_per_unit();
    let (reach_to, reach_from) = (f64::MAX * 0.99 / factor, f64::MAX * 0.99 * factor);
    if x.abs() <= reach_to {
        assert!(unit.to_internal(x).is_finite());
    }
    if x.abs() <= reach_from {
        assert!(unit.from_internal(x).is_finite());
    }
    kani::cover!(x.abs() > 1e300 && x.abs() <= reach_to, "a number past 1e300 inside the reach of `to_internal`");
    kani::cover!(x.abs() > 1e300 && x.abs() <= reach_from, "a number past 1e300 inside the reach of `from_internal`");
}

macro_rules! finite_when_it_fits {
    ($($name:ident: $unit:expr),* $(,)?) => {
        $(
            #[kani::proof]
            fn $name() {
                conversions_stay_finite_when_the_true_value_fits($unit);
            }
        )*
    };
}

finite_when_it_fits! {
    hectares_stay_finite_when_the_true_value_fits: Unit::Hectare,
    nautical_miles_stay_finite_when_the_true_value_fits: Unit::NauticalMile,
    kilometres_stay_finite_when_the_true_value_fits: Unit::Kilometre,
    metres_stay_finite_when_the_true_value_fits: Unit::Metre,
    miles_stay_finite_when_the_true_value_fits: Unit::Mile,
    millimetres_stay_finite_when_the_true_value_fits: Unit::Millimetre,
    ciceros_stay_finite_when_the_true_value_fits: Unit::Cicero,
    didots_stay_finite_when_the_true_value_fits: Unit::Didot,
    degrees_stay_finite_when_the_true_value_fits: Unit::Degree,
    elapsed_seconds_stay_finite_when_the_true_value_fits: Unit::ElapsedSecond,
}

/// Zero is zero and a sign is kept (an underflow may end in a zero, never in the other sign), in either direction.
#[kani::proof]
fn conversions_keep_zero_and_the_sign() {
    let (unit, x): (Unit, f64) = (any_unit(), kani::any());
    kani::assume(x.is_finite());
    for r in [unit.to_internal(x), unit.from_internal(x)] {
        assert!(x != 0.0 || r == 0.0);
        assert!(x < 0.0 || r >= 0.0);
        assert!(x > 0.0 || r <= 0.0);
    }
    kani::cover!(x > 0.0, "a positive number");
    kani::cover!(x < 0.0, "a negative number");
}

/// A number shown in a unit is stored by the scale of that unit and shown again by the same scale: finite in, finite out, for a unit whose
/// scale is not a power (the acre, the hectare, no unit) and a number of a size below 1e290.
#[kani::proof]
fn from_shown_and_shown_stay_finite_for_an_area_unit_or_none() {
    let (unit, x): (Unit, f64) = (any_unit_without_power(), kani::any());
    kani::assume(x.is_finite() && x.abs() <= 1e290);
    let q = Quantity::from_shown(x, unit, unit.natural_dim());
    assert!(q.stored.is_finite());
    assert!(q.shown().is_finite());
    kani::cover!(unit == Unit::Hectare && x.abs() > 1e280, "a very large number of hectares");
}

/// `value_in`: a number shown in a unit of another measure is `None` (a length is not an angle), and in a unit of its own measure is a
/// finite number, for a quantity whose scale is not a power.
#[kani::proof]
fn value_in_is_none_for_another_measure_and_finite_for_its_own() {
    let (unit, x): (Unit, f64) = (any_unit_without_power(), kani::any());
    let target = any_unit();
    kani::assume(x.is_finite() && x.abs() <= 1e290);
    let q = Quantity::new(x, unit);
    let same_measure = target.measure() == unit.measure();
    match q.value_in(target) {
        Some(shown) => {
            assert!(same_measure && shown.is_finite());
            kani::cover!(unit == Unit::Acre && target == Unit::Hectare, "acres shown in hectares");
            kani::cover!(x != 0.0 && unit == Unit::Unitless, "a plain number");
        }
        None => {
            assert!(!same_measure);
            kani::cover!(target == Unit::Inch, "a length asked of an area");
        }
    }
}

/// A zero and an infinity are themselves, shown or stored, in any unit and at any dimension: the scale of a unit to a large power is 0 or
/// infinity, and `0 / 0`, `0 * inf` and `inf / inf` are NaN. `powi` is Kani's over-approximation, so what is shown is whatever the scale is, and
/// the proof says it does not matter.
#[kani::proof]
fn zero_and_infinity_are_themselves_in_any_unit_at_any_dimension() {
    let (unit, target, dim): (Unit, Unit, i8) = (any_unit(), any_unit(), kani::any());
    for x in [0.0, -0.0, f64::INFINITY, f64::NEG_INFINITY] {
        let q = Quantity::with_dim(x, unit, dim);
        assert!(q.shown().to_bits() == x.to_bits());
        if let Some(shown) = q.value_in(target) {
            assert!(shown.to_bits() == x.to_bits());
        }
        assert!(Quantity::from_shown(x, unit, dim).stored.to_bits() == x.to_bits());
    }
    kani::cover!(dim == -128 && unit == Unit::Mile, "a mile at the smallest dimension, whose scale underflows");
    kani::cover!(dim == 127 && unit == Unit::Mile, "a mile at the largest dimension, whose scale is infinity");
}
