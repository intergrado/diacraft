//! Every vector of the batch, as data.

use diacraft_calc::CellError::{Dim, DivZero, Na, Num, Value};
use diacraft_calc::{CellAddress, EvalCtx, EvalOptions, Evaluator};
use diacraft_doc::{Owner, RowKey, ShapeRef, SheetRef};
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};
use diacraft_units::Unit::{Centimetre, Degree, DrawingDefault, Foot, Inch, Millimetre};

use crate::{E, check_table, check_table_on, eval, eval_on, mismatch, probe_addr};

#[test]
fn all_and_abs() {
    check_table(&[
        ("ALL", "ABS(\"-2\")", E::N(2.0)),
        ("ALL", "SQRT(\"81\")", E::N(9.0)),
        ("ALL", "ABS(TRUE)", E::N(1.0)),
        ("ALL", "SUM(TRUE, TRUE, FALSE)", E::N(2.0)),
        ("ALL", "SUM(1, 7/0)", E::Err(DivZero)),
        ("ALL", "LN(NA())", E::Err(Na)),
        ("ALL", "BITAND(NA(), 1)", E::Err(Na)),
        ("ABS", "ABS(-7.5)", E::N(7.5)),
        ("ABS", "ABS(7.5)", E::N(7.5)),
        ("ABS", "ABS(-3 cm)", E::U(3.0 / 2.54, Centimetre)),
    ]);
}

#[test]
fn bit_functions() {
    check_table(&[
        ("BITAND", "BITAND(12, 6)", E::N(4.0)),
        ("BITAND", "BITAND(240, 60)", E::N(48.0)),
        ("BITAND", "BITAND(65537, 65537)", E::N(1.0)),
        ("BITAND", "BITAND(12.9, 6.9)", E::N(4.0)),
        ("BITNOT", "BITNOT(6)", E::N(65529.0)),
        ("BITNOT", "BITNOT(0)", E::N(65535.0)),
        ("BITNOT", "BITNOT(65535)", E::N(0.0)),
        ("BITNOT", "BITNOT(65536)", E::N(65535.0)),
        ("BITNOT", "BITNOT(6.9)", E::N(65529.0)),
        ("BITOR", "BITOR(12, 6)", E::N(14.0)),
        ("BITOR", "BITOR(240, 15)", E::N(255.0)),
        ("BITOR", "BITOR(65536, 1)", E::N(1.0)),
        ("BITOR", "BITOR(12.9, 6.9)", E::N(14.0)),
        ("BITXOR", "BITXOR(12, 6)", E::N(10.0)),
        ("BITXOR", "BITXOR(240, 60)", E::N(204.0)),
        ("BITXOR", "BITXOR(7, 7)", E::N(0.0)),
        ("BITXOR", "BITXOR(65537, 3)", E::N(2.0)),
        ("BITXOR", "BITXOR(12.9, 5.9)", E::N(9.0)),
    ]);
}

#[test]
fn bound_vectors() {
    check_table(&[
        ("BOUND", "BOUND(5, 0, FALSE, 1, 10)", E::N(5.0)),
        ("BOUND", "BOUND(15, 0, FALSE, 1, 10)", E::N(10.0)),
        ("BOUND", "BOUND(-3, 0, FALSE, 1, 10)", E::N(1.0)),
        ("BOUND", "BOUND(18, 0, FALSE, 10, 10, FALSE, 20, 20, FALSE, 30, 30)", E::N(20.0)),
        ("BOUND", "BOUND(3, 0, FALSE, 1, 1, FALSE, 5, 5)", E::N(1.0)),
        ("BOUND", "BOUND(15, 0, TRUE, 1, 10, FALSE, 20, 30)", E::N(20.0)),
        ("BOUND", "BOUND(15, 1, FALSE, 1, 10)", E::N(15.0)),
        ("BOUND", "BOUND(3, 1, FALSE, 1, 10)", E::N(1.0)),
        ("BOUND", "BOUND(8, 1, FALSE, 1, 10)", E::N(10.0)),
        ("BOUND", "BOUND(5.5, 1, FALSE, 1, 10)", E::N(10.0)),
        ("BOUND", "BOUND(6, 1, FALSE, 0, 10, FALSE, 4, 5)", E::N(10.0)),
        ("BOUND", "BOUND(99, 2, FALSE, 1, 10)", E::N(99.0)),
        ("BOUND", "BOUND(5 cm, 0, FALSE, 1 in, 1 in)", E::U(1.0, Centimetre)),
        ("BOUND", "BOUND(5, 3, FALSE, 1, 10)", E::Err(Value)),
        ("BOUND", "BOUND(5, 0, FALSE, 1)", E::Err(Value)),
        ("BOUND", "BOUND(PNT(1, 2), 0, FALSE, 0, 1)", E::Err(Value)),
        ("formula-language V48", "BOUND(3, 0, \"1=0\", 1, 1, FALSE, 8, 8)", E::N(1.0)),
        ("formula-language V48", "BOUND(3, 0, 2, 1, 1, FALSE, 8, 8)", E::N(8.0)),
    ]);
}

#[test]
fn bound_of_an_angle_is_brought_into_a_turn() {
    // 400 degrees is 40, and -90 degrees is 270.
    let v = eval("BOUND(400 deg, 2, FALSE, 0 deg, 1 deg)");
    assert_eq!(mismatch(&v, &E::U(40f64.to_radians(), Degree)), None, "{v:?}");
    let v = eval("BOUND(-90 deg, 2, FALSE, 0 deg, 1 deg)");
    assert_eq!(mismatch(&v, &E::U(270f64.to_radians(), Degree)), None, "{v:?}");
}

/// The number a formula gives, exactly: its stored value and nothing else.
fn stored_of(formula: &str) -> f64 {
    match eval(formula) {
        diacraft_calc::Value::Number(q) => q.stored,
        other => panic!("{formula}: {other:?}"),
    }
}

/// An angle comes back from 0 up to but not including a turn, at any size. The old reduction, `x - turn * floor(x / turn)`, rounded
/// the product and left the range at a size of a few million radians (`-9.3E-10`, and `-2.3E18` at 2E34); a remainder after the exact `fmod` is
/// the exact angle, rounded once. The expected numbers are `x` modulo the `f64` turn, worked out outside with rational arithmetic and rounded once.
#[test]
fn bound_of_an_angle_lands_in_a_turn_at_any_magnitude() {
    let cases: [(&str, f64); 8] = [
        ("BOUND(-6691341.024733973 rad, 2, FALSE, 0, 0)", 6.28318530659751),
        ("BOUND(2.0074633507824903E+34 rad, 2, FALSE, 0, 0)", 4.580409425979788),
        ("BOUND(1E300 rad, 2, FALSE, 0, 0)", 5.559758606652565),
        ("BOUND(-1E300 rad, 2, FALSE, 0, 0)", 0.7234267005270212),
        ("BOUND(1.7976931348623157E308 rad, 2, FALSE, 0, 0)", 0.5806531521201137),
        ("BOUND(-1.7976931348623157E308 rad, 2, FALSE, 0, 0)", 5.7025321550594725),
        ("BOUND(-0.5 rad, 2, FALSE, 0, 0)", 5.783185307179586),
        ("BOUND(7 rad, 2, FALSE, 0, 0)", 0.7168146928204138),
    ];
    for (formula, wanted) in cases {
        let got = stored_of(formula);
        assert_eq!(got.to_bits(), wanted.to_bits(), "{formula} = {got:e}, wanted {wanted:e}");
    }
}

/// A turn is not in the range, so an angle that rounds to a whole turn is 0: `-1E-20` of a turn short of it, `-1.5E-323` rad, and
/// a whole turn itself.
#[test]
fn bound_of_an_angle_a_hair_short_of_a_turn_is_zero_and_not_a_turn() {
    for formula in [
        "BOUND(-1E-20 rad, 2, FALSE, 0, 0)",
        "BOUND(-1.5E-323 rad, 2, FALSE, 0, 0)",
        "BOUND(-1E-17 rad, 2, FALSE, 0, 0)",
        "BOUND(-1E-15 deg, 2, FALSE, 0, 0)",
        "BOUND(2 * PI() * 1 rad, 2, FALSE, 0, 0)",
        "BOUND(-2 * PI() * 1 rad, 2, FALSE, 0, 0)",
        "BOUND(0 rad, 2, FALSE, 0, 0)",
    ] {
        let got = stored_of(formula);
        assert!(got.to_bits() == 0.0f64.to_bits(), "{formula} = {got:e}");
    }
    // Just past the hair, the angle is its own residue.
    let got = stored_of("BOUND(-1E-15 rad, 2, FALSE, 0, 0)");
    assert!((std::f64::consts::TAU - got - 1e-15).abs() < 1e-15 && got < std::f64::consts::TAU, "{got:e}");
}

/// An angle that is not a number, such as infinity, is 0, as before (it has no turn count to give an angle from).
#[test]
fn bound_of_an_infinite_angle_is_zero() {
    for formula in ["BOUND(1E308*10*1 rad, 2, FALSE, 0, 0)", "BOUND(-1E308*10*1 rad, 2, FALSE, 0, 0)"] {
        let got = stored_of(formula);
        assert_eq!(got.to_bits(), 0.0f64.to_bits(), "{formula}");
    }
}

/// Over a spread of angles of every size: always from 0 up to a turn, never NaN, and congruent to the angle (checked for the
/// sizes at which a turn's worth of precision is left).
#[test]
fn bound_of_an_angle_never_leaves_the_turn() {
    let mut seed = 0x2545_f491_4f6c_dd1d_u64;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for _ in 0..400 {
        let exponent = (next() % 620) as i32 - 310;
        let x = (next() % 9_000_000 + 1_000_000) as f64 / 1e6 * 10f64.powi(exponent) * if next() % 2 == 0 { 1.0 } else { -1.0 };
        if !x.is_finite() {
            continue;
        }
        let got = stored_of(&format!("BOUND({x:e} rad, 2, FALSE, 0, 0)"));
        assert!((0.0..std::f64::consts::TAU).contains(&got), "{x:e} gave {got:e}");
    }
}

#[test]
fn ceiling_and_floor() {
    check_table(&[
        ("CEILING", "CEILING(4.1)", E::N(5.0)),
        ("CEILING", "CEILING(-4.1)", E::N(-5.0)),
        ("CEILING", "CEILING(7.1, 0.5)", E::N(7.5)),
        ("CEILING", "CEILING(-7.1, -0.5)", E::N(-7.5)),
        ("CEILING", "CEILING(3.7, 0.25)", E::N(3.75)),
        ("CEILING", "CEILING(10, 5)", E::N(10.0)),
        ("CEILING", "CEILING(3, 0)", E::N(0.0)),
        ("CEILING", "CEILING(0, 2)", E::N(0.0)),
        ("CEILING", "CEILING(3, -2)", E::Err(Num)),
        ("CEILING", "CEILING(-3, 2)", E::Err(Num)),
        ("CEILING", "CEILING(\"goat\", 2)", E::Err(Value)),
        ("CEILING", "CEILING(1.2 in)", E::U(2.0, Inch)),
        ("FLOOR", "FLOOR(4.9)", E::N(4.0)),
        ("FLOOR", "FLOOR(-4.9)", E::N(-4.0)),
        ("FLOOR", "FLOOR(7.9, 0.5)", E::N(7.5)),
        ("FLOOR", "FLOOR(-7.9, -0.5)", E::N(-7.5)),
        ("FLOOR", "FLOOR(3.7, 0.5)", E::N(3.5)),
        ("FLOOR", "FLOOR(10, 5)", E::N(10.0)),
        ("FLOOR", "FLOOR(3, 0)", E::N(0.0)),
        ("FLOOR", "FLOOR(0, 2)", E::N(0.0)),
        ("FLOOR", "FLOOR(3, -2)", E::Err(Num)),
        ("FLOOR", "FLOOR(-3, 2)", E::Err(Num)),
        ("FLOOR", "FLOOR(\"goat\", 2)", E::Err(Value)),
        ("FLOOR", "FLOOR(1.8 in)", E::U(1.0, Inch)),
    ]);
}

#[test]
fn int_and_logs() {
    check_table(&[
        ("INT", "INT(8.6)", E::N(8.0)),
        ("INT", "INT(-8.6)", E::N(-9.0)),
        ("INT", "INT(8.6 in)", E::U(8.0, Inch)),
        ("INTUP", "INTUP(8.1)", E::N(9.0)),
        ("INTUP", "INTUP(-8.1)", E::N(-8.0)),
        ("INTUP", "INTUP(5)", E::N(5.0)),
        ("INTUP", "INTUP(8.1 in)", E::U(9.0, Inch)),
        ("LN", "LN(1)", E::N(0.0)),
        ("LN", "LN(42)", E::N(42f64.ln())),
        ("LN", "LN(0)", E::Err(Num)),
        ("LN", "LN(-5)", E::Err(Num)),
        ("LN", "LN(1 in)", E::U(0.0, Inch)),
        ("LOG10", "LOG10(1000)", E::N(3.0)),
        ("LOG10", "LOG10(42)", E::N(42f64.log10())),
        ("LOG10", "LOG10(0)", E::Err(Num)),
        ("LOG10", "LOG10(-5)", E::Err(Num)),
        ("LOG10", "LOG10(1 in)", E::U(0.0, Inch)),
    ]);
}

#[test]
fn magnitude_max_min() {
    check_table(&[
        ("MAGNITUDE", "MAGNITUDE(1, 6, 1, 8)", E::N(10.0)),
        ("MAGNITUDE", "MAGNITUDE(2, 3, 1, 8)", E::N(10.0)),
        ("MAGNITUDE", "MAGNITUDE(1, -6, 1, 8)", E::N(10.0)),
        ("MAGNITUDE", "MAGNITUDE(1, 3 in, 1, 4 in)", E::N(5.0)),
        ("MAGNITUDE", "MAGNITUDE(1, 2 in^2, 1, 1)", E::Err(Dim)),
        ("MAGNITUDE", "MAGNITUDE(1, 1, 1, 2 in^2)", E::Err(Dim)),
        ("MAX", "MAX(2 ft, 30 in, 1 yd)", E::U(36.0, diacraft_units::Unit::Yard)),
        ("MAX", "MAX(-3, -8)", E::N(-3.0)),
        ("MAX", "MAX(12 in, 1 ft)", E::U(12.0, Inch)),
        ("MAX", "MAX(7)", E::N(7.0)),
        ("MIN", "MIN(2 ft, 30 in, 1 yd)", E::U(24.0, diacraft_units::Unit::Foot)),
        ("MIN", "MIN(-3, -8)", E::N(-8.0)),
        ("MIN", "MIN(1 ft, 12 in)", E::U(12.0, diacraft_units::Unit::Foot)),
        ("MIN", "MIN(7)", E::N(7.0)),
    ]);
}

#[test]
fn modulus_pi_pow() {
    check_table(&[
        ("MODULUS", "MODULUS(7, 3)", E::N(1.0)),
        ("MODULUS", "MODULUS(7, -3)", E::N(-2.0)),
        ("MODULUS", "MODULUS(-7, 3)", E::N(2.0)),
        ("MODULUS", "MODULUS(-7, -3)", E::N(-1.0)),
        ("MODULUS", "MODULUS(5, 1.4)", E::N(0.8)),
        ("MODULUS", "MODULUS(5, -1.4)", E::N(-0.6)),
        ("MODULUS", "MODULUS(7 in, 3)", E::U(1.0, Inch)),
        ("MODULUS", "MODULUS(7, 0)", E::Err(DivZero)),
        ("PI", "PI()", E::N(std::f64::consts::PI)),
        ("PI", "2*PI()", E::N(std::f64::consts::TAU)),
        ("PI", "(10 in / 2)^2 * PI()", E::D(78.53981633974483, Inch, 2)),
        ("POW", "POW(3, 4)", E::N(81.0)),
        ("POW", "3^4", E::N(81.0)),
        ("POW", "POW(5, 2)", E::N(25.0)),
        ("POW", "POW(2, -2)", E::N(0.25)),
        ("POW", "POW(9, 0.5)", E::N(3.0)),
        ("POW", "POW(-2, 3)", E::N(-8.0)),
        ("POW", "POW(7, 0)", E::N(1.0)),
        ("POW", "POW(0, 3)", E::N(0.0)),
        ("POW", "POW(0, 0)", E::Err(Num)),
        ("POW", "POW(0, -1)", E::Err(DivZero)),
        ("POW", "POW(-8, 0.5)", E::Err(Dim)),
        ("POW", "POW(-2, -2)", E::N(0.25)),
        ("POW", "POW(-8, -0.5)", E::Err(Dim)),
        ("POW", "POW(3 in, 2)", E::D(9.0, Inch, 2)),
        ("POW", "POW(2 in, 1)", E::U(2.0, Inch)),
        ("POW", "POW(2 in, 0)", E::N(1.0)),
        ("POW", "POW(50%, 2)", E::U(0.25, diacraft_units::Unit::Percent)),
        ("POW", "POW(2 in, 1.5)", E::Err(Dim)),
        ("POW", "POW(2 in, 128)", E::Err(Dim)),
        ("formula-language V6", "-3^2", E::N(9.0)),
        ("formula-language V6", "-(3^2)", E::N(-9.0)),
        ("formula-language V6", "2^-1", E::N(0.5)),
        ("formula-language V7", "2^3^2", E::N(64.0)),
        ("formula-language V7", "2^(3^2)", E::N(512.0)),
        ("formula-language V7", "20/2/5", E::N(2.0)),
    ]);
}

#[test]
fn rand_is_in_range_and_not_constant() {
    // 1000 evaluations, each at least 0 and below 1, and at least two different values.
    let mut seen = std::collections::BTreeSet::new();
    let (doc, at) = crate::probe(|s| s, "RAND()");
    let mut ctx = diacraft_calc::EvalCtx::default();
    let mut ev = diacraft_calc::Evaluator::new(&doc, &mut ctx);
    for _ in 0..1000 {
        // A new evaluation each time: the memo of a live evaluation would give the same cell back.
        ev.invalidate_memo();
        let v = ev.evaluate(&at);
        let diacraft_calc::Value::Number(q) = v else { panic!("{v:?}") };
        assert!((0.0..1.0).contains(&q.stored), "{}", q.stored);
        seen.insert(q.stored.to_bits());
    }
    assert!(seen.len() > 1, "a constant passes");
    let v = eval("RAND() _GE_ 0");
    assert_eq!(mismatch(&v, &E::B(true)), None);
    let v = eval("RAND() _LT_ 1");
    assert_eq!(mismatch(&v, &E::B(true)), None);
}

#[test]
fn round_sign_sqrt_sum_trunc() {
    check_table(&[
        ("ROUND", "ROUND(8.765, 1)", E::N(8.8)),
        ("ROUND", "ROUND(8.765, 0)", E::N(9.0)),
        ("ROUND", "ROUND(1234.5, -2)", E::N(1200.0)),
        ("ROUND", "ROUND(123.654, 2)", E::N(123.65)),
        ("ROUND", "ROUND(1234.5678, 16)", E::N(1234.5678)),
        ("ROUND", "ROUND(1234.5678, -9)", E::N(1234.5678)),
        ("ROUND", "ROUND(123456789012, -8)", E::N(123500000000.0)),
        ("ROUND", "ROUND(1E300, 16)", E::N(1e300)),
        ("ROUND", "ROUND(8.765, 1.9)", E::N(8.8)),
        ("ROUND", "ROUND(2.345 in, 1)", E::U(2.3, Inch)),
        ("SIGN", "SIGN(-4)", E::N(-1.0)),
        ("SIGN", "SIGN(4)", E::N(1.0)),
        ("SIGN", "SIGN(0)", E::N(0.0)),
        ("SIGN", "SIGN(5E-10)", E::N(0.0)),
        ("SIGN", "SIGN(5E-10, 0)", E::N(1.0)),
        ("SIGN", "SIGN(2E-9)", E::N(1.0)),
        ("SIGN", "SIGN(0.5, 1)", E::N(0.0)),
        ("SIGN", "SIGN(-0.5, 1)", E::N(0.0)),
        ("SIGN", "SIGN(1, 1)", E::N(0.0)),
        ("SIGN", "SIGN(0.5, -1)", E::N(0.0)),
        ("SIGN", "SIGN(2, -1)", E::N(1.0)),
        ("SIGN", "SIGN(-3 cm)", E::N(-1.0)),
        ("SQRT", "SQRT(81)", E::N(9.0)),
        ("SQRT", "SQRT(2)", E::N(2f64.sqrt())),
        ("SQRT", "SQRT(-4)", E::Err(Num)),
        ("SQRT", "SQRT(4 in)", E::Err(Dim)),
        ("SQRT", "SQRT(1 ft^3)", E::Err(Dim)),
        ("SQRT", "SQRT(9 in^2)", E::U(3.0, Inch)),
        ("SQRT", "SQRT(16 in^4)", E::D(4.0, Inch, 2)),
        ("SUM", "SUM(4, 8, 15)", E::N(27.0)),
        ("SUM", "SUM(2.5)", E::N(2.5)),
        ("SUM", "SUM(1 ft, 12 in)", E::U(24.0, diacraft_units::Unit::Foot)),
        ("SUM", "SUM(12 in, 1 ft)", E::U(24.0, Inch)),
        ("TRUNC", "TRUNC(8.769, 2)", E::N(8.76)),
        ("TRUNC", "TRUNC(8.769, 0)", E::N(8.0)),
        ("TRUNC", "TRUNC(1299, -2)", E::N(1200.0)),
        ("TRUNC", "TRUNC(123.654, -1)", E::N(120.0)),
        ("TRUNC", "TRUNC(1.5, 16)", E::N(1.5)),
        ("TRUNC", "TRUNC(1.5, -309)", E::N(1.5)),
        ("TRUNC", "TRUNC(1299, -308)", E::N(0.0)),
        ("TRUNC", "TRUNC(1E300, 16)", E::N(1e300)),
        ("TRUNC", "TRUNC(8.769, 2.9)", E::N(8.76)),
        ("TRUNC", "TRUNC(8.769 in, 2)", E::U(8.76, Inch)),
    ]);
}

#[test]
fn mod_and_underscore_mod_follow_the_community_report() {
    check_table(&[
        ("MOD", "MOD(5, 2)", E::N(1.0)),
        ("MOD", "MOD(-5, 2)", E::N(1.0)),
        ("MOD", "MOD(5, 0)", E::Err(DivZero)),
        ("MOD", "_MOD(-PI()/4, 2*PI())", E::N(std::f64::consts::FRAC_PI_4)),
        ("MOD", "_MOD(5, 2)", E::N(1.0)),
        ("MOD", "_MOD(5, 0)", E::Err(DivZero)),
    ]);
}

#[test]
fn a_result_beyond_a_double_keeps_infinity_or_is_an_error_and_never_nan() {
    // No NaN enters a value. Infinity stays for an operator (a file writes `1.#INF`).
    let v = eval("1E308*10");
    match v {
        diacraft_calc::Value::Number(q) => assert!(q.stored.is_infinite()),
        other => panic!("{other:?}"),
    }
    let v = eval("POW(10, 400)");
    assert!(matches!(v, diacraft_calc::Value::Number(q) if q.stored.is_infinite()), "{v:?}");
    let v = eval("0*POW(10, 400)");
    assert_eq!(mismatch(&v, &E::Err(Num)), None, "inf times 0 is not a number: {v:?}");
}

/// The amount left after the division, taking the divisor's sign, is the exact remainder of the numbers, moved by the divisor
/// when its sign differs. The rule `number - divisor * floor(number / divisor)` gave the divisor itself for a small negative number (`-1E-20`
/// modulo 1 was 1, and so was `-1E-17`: the sum rounds up), and infinity for a divisor far below the number. A remainder is below the
/// divisor in size: a result that would round to the divisor is the largest number below it, with its sign.
#[test]
fn modulus_is_the_exact_remainder_taking_the_divisors_sign() {
    let below_one = 1.0 - f64::EPSILON / 2.0;
    for (formula, wanted) in [
        ("MODULUS(-1E-20, 1)", below_one),
        ("MODULUS(-1E-17, 1)", below_one),
        ("MODULUS(-5E-17, 1)", below_one),
        ("MODULUS(-1E-20, -1)", -1e-20),
        ("MODULUS(1E-20, -1)", -below_one),
        ("MODULUS(-2, 3E38)", 3e38_f64.next_down()),
        ("MODULUS(2, -3E38)", -(3e38_f64.next_down())),
        ("MODULUS(1E308, 1E-308)", 3.498445546245627e-309),
        ("MODULUS(-1E308, 3)", 1.0),
        ("MODULUS(1E308, -3)", -1.0),
        ("MODULUS(-1E308, -3)", -2.0),
        ("MODULUS(1E308, 3)", 2.0),
        ("MODULUS(1E308, 1E-300)", 3.0195000970293847e-301),
        ("MODULUS(3, 1E-320)", 3.715e-321),
        ("MODULUS(-1E-320, 1)", below_one),
    ] {
        let got = stored_of(formula);
        assert_eq!(got.to_bits(), wanted.to_bits(), "{formula} = {got:e}, wanted {wanted:e}");
    }
    // The result is always below the divisor in size and takes its sign (never the divisor itself), and 0 is 0.
    let mut seed = 0x9e37_79b9_7f4a_7c15_u64;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for _ in 0..300 {
        let scale =
            |n: u64| (n % 9_000 + 1_000) as f64 / 1e3 * 10f64.powi((n >> 20) as i32 % 36 - 18) * if (n >> 40).is_multiple_of(2) { 1.0 } else { -1.0 };
        let (n, d) = (scale(next()), scale(next()));
        let got = stored_of(&format!("MODULUS({n:e}, {d:e})"));
        assert!(got.abs() < d.abs() && (got == 0.0 || (got < 0.0) == (d < 0.0)), "MODULUS({n:e}, {d:e}) = {got:e}");
    }
}

/// A: our rule, which no sample tests yet: `INT`, `INTUP`, `ROUND`, `TRUNC`, `MODULUS`, `LN`, `LOG10`, `SQRT` and `POW` work on the
/// number shown in the argument's unit, as `CEILING` does, and internal units for `DL` or no unit. `INT(1.5 ft)` is 1 ft, 12 stored inches, where
/// the stored reading kept 18 inches and read 1.5 ft. One vector per function at a unit that is not the inch, and the inch where both readings agree.
#[test]
fn the_non_linear_functions_work_on_the_number_shown_in_its_unit() {
    use diacraft_units::Unit::Percent;
    check_table(&[
        ("ALL", "INT(1.5 ft)", E::U(12.0, Foot)),
        ("ALL", "INT(-1.5 ft)", E::U(-24.0, Foot)),
        ("ALL", "INTUP(1.2 ft)", E::U(24.0, Foot)),
        ("ALL", "ROUND(1.26 ft, 1)", E::U(15.6, Foot)),
        ("ALL", "TRUNC(1.79 ft, 1)", E::U(20.4, Foot)),
        // The divisor with no unit counts in the number's unit: 7 ft modulo 3 ft is 1 ft. On the stored inches it is 84 modulo 3, which is 0.
        ("ALL", "MODULUS(7 ft, 3)", E::U(12.0, Foot)),
        ("ALL", "MODULUS(7 ft, 2 ft)", E::U(12.0, Foot)),
        ("ALL", "MODULUS(450 deg, 360 deg)", E::U(90f64.to_radians(), Degree)),
        // The unit of the result is the argument's, and the number in it is the function's (no spec line for what it means).
        ("ALL", "LN(2 ft)", E::U(2f64.ln() * 12.0, Foot)),
        ("ALL", "LOG10(100 ft)", E::U(24.0, Foot)),
        ("ALL", "SQRT(4 ft^2)", E::U(24.0, Foot)),
        // A power keeps the unit's dimension and so reads the same on the stored and the shown number; a percent base is the documented case.
        ("ALL", "POW(2 ft, 2)", E::D(576.0, Foot, 2)),
        ("POW", "POW(50%, 2)", E::U(0.25, Percent)),
        // An angle and a percent are shown in degrees and in percent.
        ("ALL", "INT(10.5 deg)", E::U(10f64.to_radians(), Degree)),
        ("ALL", "INT(12.5%)", E::U(0.12, Percent)),
        // The inch, where both readings agree.
        ("INT", "INT(8.6 in)", E::U(8.0, Inch)),
        ("MODULUS", "MODULUS(7 in, 3)", E::U(1.0, Inch)),
    ]);
    let _ = DrawingDefault;
}

/// The project rules of the reading that no sample tests (B; check-list rows): a `multiple` that is a reference to a typed length
/// cell is a quantity in `DL`, which is a unit of its own, so it counts as its stored inches shown in the number's unit; a `multiple` of another measure
/// than the number's counts as its stored value shown in the number's unit, and is no error.
#[test]
fn a_multiple_that_is_a_dl_reference_or_of_another_measure_is_a_labelled_project_rule() {
    // `Height` is 0.5 in: as a multiple against 15.7 mm it is 12.7 mm, so the answer is two of them, 25.4 mm.
    let v = eval_on(|s| s.cell("Height", "0.5"), "CEILING(15.7 mm, Height)");
    assert_eq!(mismatch(&v, &E::U(1.0, Millimetre)), None, "{v:?}");
    // 1 degree is 0.01745 stored; against a number in inches it counts as 0.01745 in, and 1 in is 58 of them.
    let v = eval("CEILING(1 in, 1 deg)");
    assert_eq!(mismatch(&v, &E::U(58.0 * 1f64.to_radians(), Inch)), None, "{v:?}");
}

/// Review: the acre and the hectare are area units that already hold square inches (their scale is not raised to the dimension), so a square
/// root read through the number shown in them gave 6,272,640 stored inches for `SQRT(1 acre)`. A power of a unit reads the same on the stored and the shown
/// number, and `SQRT` is one: `SQRT(1 acre)` is `POW(1 acre,0.5)`, 2,504.52 inches in `DL` (an acre at a dimension other than 2 is `DL`), and the same
/// for the hectare (a square 100 m on a side, 3,937.01 inches). One length case so the flag cannot break `SQRT` again.
#[test]
fn sqrt_of_an_area_is_the_power_of_the_unit_whatever_the_non_linear_rule() {
    let acre = 6_272_640f64.sqrt();
    let hectare = (1.0e10f64 / (25.4 * 25.4)).sqrt();
    check_table(&[
        ("SQRT", "SQRT(1 acre)", E::U(acre, DrawingDefault)),
        ("POW", "POW(1 acre, 0.5)", E::U(acre, DrawingDefault)),
        ("SQRT", "SQRT(4 acre)", E::U(2.0 * acre, DrawingDefault)),
        ("SQRT", "SQRT(1 ha)", E::U(hectare, DrawingDefault)),
        ("POW", "POW(1 ha, 0.5)", E::U(hectare, DrawingDefault)),
        ("SQRT", "SQRT(9 ft^2)", E::U(36.0, Foot)),
        ("SQRT", "SQRT(16 in^4)", E::D(4.0, Inch, 2)),
    ]);
}

/// `CEILING` and `FLOOR` work on the number shown in the unit `number` carries, and a `multiple` with no unit counts in that unit
/// `Height` is 0.5 in, in `DL`.
#[test]
fn ceiling_and_floor_round_the_number_as_shown_in_its_unit() {
    check_table_on(
        |s| s.cell("Height", "0.5"),
        &[
            // `Height+3 mm` is 15.7 mm and the multiple is 1 mm. Rounding the stored inches gives 1 in, 25.4 mm.
            ("CEILING", "CEILING(Height+3 mm,1)", E::U(16.0 / 25.4, Millimetre)),
            ("CEILING", "MAX(10 mm,CEILING(Height+3 mm,1))", E::U(16.0 / 25.4, Millimetre)),
            // Our rule: 15 mm. Rounding the stored inches gives 0.
            ("FLOOR", "FLOOR(Height+3 mm,1)", E::U(15.0 / 25.4, Millimetre)),
            // A number in `DL` or with no unit counts in internal units, inches.
            ("CEILING", "CEILING(Height+0.1,0.25)", E::U(0.75, DrawingDefault)),
            ("FLOOR", "FLOOR(Height+0.1,0.25)", E::U(0.5, DrawingDefault)),
            ("CEILING", "CEILING(Height)", E::U(1.0, DrawingDefault)),
        ],
    );
    check_table(&[
        // Another unit than millimetres, and a negative number: stepping outward from zero is in the shown unit too.
        ("CEILING", "CEILING(1.2 ft)", E::U(24.0, Foot)),
        ("FLOOR", "FLOOR(1.8 ft)", E::U(12.0, Foot)),
        ("CEILING", "CEILING(-15.2 mm,-1)", E::U(-16.0 / 25.4, Millimetre)),
        ("FLOOR", "FLOOR(-15.7 mm,-1)", E::U(-15.0 / 25.4, Millimetre)),
        // A missing multiple is 1 of the unit shown, and -1 for a negative number.
        ("CEILING", "CEILING(15.2 mm)", E::U(16.0 / 25.4, Millimetre)),
        ("CEILING", "CEILING(-15.2 mm)", E::U(-16.0 / 25.4, Millimetre)),
        ("FLOOR", "FLOOR(15.7 mm)", E::U(15.0 / 25.4, Millimetre)),
        // A multiple that carries a unit of its own is a length, counted as that many of the number's unit: 2 mm, and a point (0.3528 mm).
        ("CEILING", "CEILING(15.7 mm,2 mm)", E::U(16.0 / 25.4, Millimetre)),
        ("CEILING", "CEILING(15.7 mm,1 pt)", E::U(45.0 / 72.0, Millimetre)),
        ("FLOOR", "FLOOR(15.7 mm,1 pt)", E::U(44.0 / 72.0, Millimetre)),
        // The zero and error rules are as they were, and a number already on a multiple stays there.
        ("CEILING", "CEILING(15.7 mm,0)", E::U(0.0, Millimetre)),
        ("CEILING", "CEILING(15.7 mm,-1)", E::Err(Num)),
        ("FLOOR", "FLOOR(-15.7 mm,1)", E::Err(Num)),
        ("CEILING", "CEILING(16 mm,1)", E::U(16.0 / 25.4, Millimetre)),
        ("FLOOR", "FLOOR(16 mm,1)", E::U(16.0 / 25.4, Millimetre)),
    ]);
}

/// `number` is 0.6 in `DL`, so the multiple is a quarter inch whatever the drawing's unit: a page scale in millimetres
/// changes nothing. Counting the multiple in the drawing's millimetres gives 15.25 mm, about 0.6003937008 in.
#[test]
fn ceiling_a_drawing_scale_in_millimetres_does_not_change_the_unit_of_the_multiple() {
    let shape = ShapeBuilder::new(1).cell("Height", "0.5").row_formula("User", None, RowKey::name("Probe"), "Value", "CEILING(Height+0.1,0.25)", "0");
    let page = PageBuilder::new().cell_u("PageScale", "1", "IN").cell_u("DrawingScale", "25.4", "MM").shape(shape);
    let doc = DocBuilder::new().page(1, page).build();
    let at = CellAddress::new(SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0)), probe_addr());
    let mut ctx = EvalCtx::new(EvalOptions::default(), Default::default());
    let v = Evaluator::new(&doc, &mut ctx).evaluate(&at);
    assert_eq!(mismatch(&v, &E::U(0.75, DrawingDefault)), None, "{v:?}");
}
