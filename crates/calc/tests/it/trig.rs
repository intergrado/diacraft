//! Every vector of the batch, as data. An expected number is computed from the angle in the vector, so that the
//! row says what it expects; the "page" values the spec prints to four digits are the same numbers.

use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI, TAU};

use diacraft_calc::CellError::{DivZero, Na, Num, Value};
use diacraft_units::Unit::{AngleDefault, Degree, Radian};

use crate::{E, check_table, eval, mismatch};

fn deg(x: f64) -> f64 {
    x.to_radians()
}

#[test]
fn all_vectors() {
    check_table(&[
        ("ALL", "SIN(7/0)", E::Err(DivZero)),
        ("ALL", "ATAN2(NA(),1)", E::Err(Na)),
        ("ALL", "DEG(7/0)", E::Err(DivZero)),
        ("ALL", "SIN(30 deg)", E::N(0.5)),
        ("ALL", "SIN(30)", E::N(-0.9880316240928618)),
    ]);
}

#[test]
fn special_angles_are_not_snapped() {
    // Within 1E-20 of what a double gives, which no snap to 0 meets; and the tangent of 90 degrees is a huge finite number.
    let near = |f: &str, want: f64, within: f64| {
        let v = eval(f);
        let diacraft_calc::Value::Number(q) = v else { panic!("{f}: {v:?}") };
        assert!((q.stored - want).abs() <= within, "{f}: {} vs {want}", q.stored);
        assert!(q.stored != 0.0 || want == 0.0, "{f} snapped to zero");
    };
    near("COS(90 deg)", 6.123233995736766e-17, 1e-20);
    near("SIN(180 deg)", 1.2246467991473532e-16, 1e-20);
    near("TAN(90 deg)", 1.633123935319537e16, 1e3);
}

#[test]
fn inverse_functions() {
    check_table(&[
        ("ACOS", "ACOS(0)", E::U(FRAC_PI_2, AngleDefault)),
        ("ACOS", "ACOS(-1)", E::U(PI, AngleDefault)),
        ("ACOS", "ACOS(0.5)", E::U(1.0471975511965976, AngleDefault)),
        ("ACOS", "ACOS(1.5)", E::Err(Value)),
        ("ACOS", "ACOS(-1.5)", E::Err(Value)),
        ("ACOS", "ACOS(1)", E::U(0.0, AngleDefault)),
        ("ASIN", "ASIN(1)", E::U(FRAC_PI_2, AngleDefault)),
        ("ASIN", "ASIN(-0.5)", E::U(-std::f64::consts::FRAC_PI_6, AngleDefault)),
        ("ASIN", "ASIN(2)", E::Err(Value)),
        ("ATAN", "ATAN(1)", E::U(FRAC_PI_4, AngleDefault)),
        ("ATAN", "ATAN(-1)", E::U(-FRAC_PI_4, AngleDefault)),
        ("ATAN", "ATAN(1E300)", E::U(FRAC_PI_2, AngleDefault)),
        ("ATAN2", "ATAN2(2,0)", E::U(FRAC_PI_2, AngleDefault)),
        ("ATAN2", "ATAN2(0,2)", E::U(0.0, AngleDefault)),
        ("ATAN2", "ATAN2(1,1)", E::U(FRAC_PI_4, AngleDefault)),
        ("ATAN2", "ATAN2(1,SQRT(3))", E::U(std::f64::consts::FRAC_PI_6, AngleDefault)),
        ("ATAN2", "ATAN2(-3,-3)", E::U(-2.356194490192345, AngleDefault)),
        ("ATAN2", "ATAN2(3,-3)", E::U(2.356194490192345, AngleDefault)),
        ("ATAN2", "ATAN2(0,0)", E::U(0.0, AngleDefault)),
        ("ATAN2", "ATAN2(0,-2)", E::U(PI, AngleDefault)),
        ("ATAN2", "ATAN2(1 ft,12 in)", E::U(FRAC_PI_4, AngleDefault)),
        ("ATAN2", "ATAN2(-1E-30,-2)", E::U(-PI, AngleDefault)),
        ("ATAN2", "ATAN2(1.25,2.25)", E::U(1.25f64.atan2(2.25), AngleDefault)),
        ("ATAN2", "ATAN2(-(3-3),-2)", E::U(PI, AngleDefault)),
    ]);
    // The printed number: 0.5070985 stored, and 29.0546 degrees shown.
    let v = eval("ATAN2(1.25,2.25)");
    let diacraft_calc::Value::Number(q) = v else { panic!() };
    assert!((q.stored - 0.5070985).abs() < 1e-7, "{}", q.stored);
    assert!((q.shown().to_degrees() - 29.0546).abs() < 1e-4);
}

#[test]
fn ang360_units_and_ranges() {
    check_table(&[
        ("ANG360", "ANG360(395 deg)", E::U(deg(35.0), Degree)),
        ("ANG360", "ANG360(-9.8 rad)", E::U(2.7663706143591725, Radian)),
        ("ANG360", "ANG360(-90 deg)", E::U(deg(270.0), Degree)),
        ("ANG360", "ANG360(45)", E::U(45.0 - 7.0 * TAU, AngleDefault)),
        ("ANG360", "ANG360(400 deg)", E::U(deg(40.0), Degree)),
        ("ANG360", "ANG360(1.5 rad)", E::U(1.5, Radian)),
        ("ANG360", "ANG360(1.5)", E::U(1.5, AngleDefault)),
        ("ANG360", "ANG360(-1E-20)", E::U(0.0, AngleDefault)),
        ("ANG360", "ANG360(360 deg)", E::U(0.0, Degree)),
        ("ANG360", "ANG360(\"goat\")", E::Err(Value)),
    ]);
}

#[test]
fn the_sine_family() {
    check_table(&[
        ("COS", "COS(45 deg)", E::N(deg(45.0).cos())),
        ("COS", "COS(60 deg)", E::N(0.5)),
        ("COS", "COS(1)", E::N(0.5403023058681398)),
        ("COS", "COS(0 deg)", E::N(1.0)),
        ("COSH", "COSH(45 deg)", E::N(deg(45.0).cosh())),
        ("COSH", "COSH(1)", E::N(1.5430806348152437)),
        ("COSH", "COSH(0)", E::N(1.0)),
        ("COSH", "COSH(-1)", E::N(1.5430806348152437)),
        ("COSH", "COSH(1000)", E::Err(Num)),
        ("SIN", "SIN(45 deg)", E::N(deg(45.0).sin())),
        ("SIN", "SIN(1)", E::N(0.8414709848078965)),
        ("SIN", "SIN(1 rad)", E::N(0.8414709848078965)),
        ("SINH", "SINH(45 deg)", E::N(deg(45.0).sinh())),
        ("SINH", "SINH(1)", E::N(1.1752011936438014)),
        ("SINH", "SINH(0)", E::N(0.0)),
        ("SINH", "SINH(-1)", E::N(-1.1752011936438014)),
        ("SINH", "SINH(1000)", E::Err(Num)),
        ("TAN", "TAN(45 deg)", E::N(1.0)),
        ("TAN", "TAN(1)", E::N(1.5574077246549023)),
        ("TANH", "TANH(-45 deg)", E::N((-deg(45.0)).tanh())),
        ("TANH", "TANH(-45)", E::N(-1.0)),
        ("TANH", "TANH(0)", E::N(0.0)),
        ("TANH", "TANH(1)", E::N(0.7615941559557649)),
    ]);
    // The spec's printed values, to the digits it prints.
    let printed = |f: &str, want: f64| {
        let diacraft_calc::Value::Number(q) = eval(f) else { panic!("{f}") };
        assert!((q.stored - want).abs() < 5e-5, "{f}: {} vs {want}", q.stored);
    };
    printed("COS(45 deg)", std::f64::consts::FRAC_1_SQRT_2);
    printed("SIN(45 deg)", std::f64::consts::FRAC_1_SQRT_2);
    printed("COSH(45 deg)", 1.3246);
    printed("SINH(45 deg)", 0.8687);
    printed("TANH(-45 deg)", -0.6558);
}

#[test]
fn tan_at_a_quarter_turn_is_finite_and_huge() {
    let v = eval("TAN(90 deg)");
    let diacraft_calc::Value::Number(q) = v else { panic!("{v:?}") };
    assert!(q.stored.is_finite() && q.stored > 1e15);
}

#[test]
fn deg_and_rad() {
    check_table(&[
        ("DEG", "DEG(PI()/4)", E::U(FRAC_PI_4, Degree)),
        ("DEG", "DEG(30 deg)", E::U(deg(30.0), Degree)),
        ("DEG", "DEG(1.5 rad)", E::U(1.5, Degree)),
        ("DEG", "SIN(DEG(PI()/2))", E::N(1.0)),
        ("RAD", "RAD(45)", E::U(FRAC_PI_4, Radian)),
        ("RAD", "RAD(180)", E::U(PI, Radian)),
        ("RAD", "RAD(45 deg)", E::U(FRAC_PI_4, Radian)),
        ("RAD", "RAD(1.5 rad)", E::U(1.5, Radian)),
        ("RAD", "RAD(ATAN2(1,1))", E::U(FRAC_PI_4, Radian)),
    ]);
    let _ = mismatch;
}
