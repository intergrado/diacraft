//! The compound literals of R29: feet and inches, degrees with minutes and seconds, ciceros and didots, picas and points.
//! Their stored number is the sum, in the internal unit; the unit says how it is shown.

use diacraft_units::{Quantity, Unit};

use crate::close;

/// Given `8FT 4IN`, expect stored 100 and `U` F_I.
#[test]
fn feet_and_inches() {
    let q = Quantity::feet_inches(8.0, 4.0);
    assert!(close(q.stored, 100.0), "{}", q.stored);
    assert_eq!(q.unit, Unit::FeetInches);
    assert_eq!(q.unit.code(), "F_I");
    // Naive: the feet alone (96), or the digits side by side (84), or 8.4 feet.
    for naive in [96.0, 84.0, 8.4 * 12.0] {
        assert!((q.stored - naive).abs() > 0.5, "{naive}");
    }
    assert!(close(Quantity::feet_inches(0.0, 0.0).stored, 0.0), "the samples hold only zero lengths in this form");
    assert!(close(Quantity::feet_inches(1.0, 0.0).stored, 12.0));
}

/// V18 again: given 10.25 with the degree sign, expect the value of 10 degrees 15 minutes; both are `U` AD.
#[test]
fn degrees_minutes_seconds() {
    let decimal = Quantity::from_value(10.25, Unit::DegreesMinutesSeconds);
    let minutes = Quantity::degrees_minutes_seconds(10.0, 15.0, 0.0);
    assert!(close(decimal.stored, minutes.stored), "{} {}", decimal.stored, minutes.stored);
    assert!(close(minutes.stored, 10.25_f64.to_radians()));
    assert_eq!(minutes.unit.code(), "AD");
    // 10 degrees 15 minutes 36 seconds is 10.26 degrees.
    assert!(close(Quantity::degrees_minutes_seconds(10.0, 15.0, 36.0).stored, 10.26_f64.to_radians()));
    // Naive: reading the minutes as tenths of a degree (10.15).
    assert!((minutes.stored - 10.15_f64.to_radians()).abs() > 1e-4);
}

/// `2C4` is two ciceros and four didots, `3P6` three picas and six points.
#[test]
fn ciceros_and_didots_picas_and_points() {
    let didot = 0.3759715104 / 25.4;
    let cd = Quantity::cicero_didot(2.0, 4.0);
    assert!(close(cd.stored, (2.0 * 12.0 + 4.0) * didot), "{}", cd.stored);
    assert_eq!(cd.unit, Unit::CiceroDidot);
    assert_eq!(cd.unit.code(), "C_D");
    let pp = Quantity::pica_points(3.0, 6.0);
    assert!(close(pp.stored, 3.0 / 6.0 + 6.0 / 72.0), "{}", pp.stored);
    assert_eq!(pp.unit.code(), "P_PT");
    // 12 points make a pica.
    assert!(close(Quantity::pica_points(1.0, 0.0).stored, Quantity::pica_points(0.0, 12.0).stored));
    assert!(close(Quantity::cicero_didot(1.0, 0.0).stored, Quantity::cicero_didot(0.0, 12.0).stored));
}

/// A compound unit's own number is in its smallest unit (inches, degrees, didots, points), so a conversion to its measure works.
#[test]
fn a_compound_value_can_be_shown_in_a_plain_unit() {
    let q = Quantity::feet_inches(8.0, 4.0);
    assert!(close(q.value_in(Unit::Foot).unwrap(), 100.0 / 12.0));
    assert!(close(q.value_in(Unit::Inch).unwrap(), 100.0));
    assert!(close(Quantity::degrees_minutes_seconds(180.0, 0.0, 0.0).value_in(Unit::Radian).unwrap(), std::f64::consts::PI));
    assert!(close(Quantity::pica_points(1.0, 0.0).value_in(Unit::Point).unwrap(), 12.0));
}
