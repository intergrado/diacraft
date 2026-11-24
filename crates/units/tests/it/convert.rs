//! `Quantity`: a number held in the internal unit (inches, radians, days) and shown in another.

use diacraft_units::{Measure, Quantity, Unit};

use crate::close;

/// The last column, as the spec writes it: the internal units (inches, radians or days) in one unit. A row with a
/// formula (`1000 / 25.4`) is written as that formula, so a typo in `ratio` is not repeated here.
fn table() -> Vec<(Unit, f64)> {
    let didot = 0.3759715104 / 25.4;
    vec![
        (Unit::Inch, 1.0),
        (Unit::InchFine, 1.0),
        (Unit::Foot, 12.0),
        (Unit::Yard, 36.0),
        (Unit::Mile, 63360.0),
        (Unit::MileFine, 63360.0),
        (Unit::NauticalMile, 1852000.0 / 25.4),
        (Unit::Millimetre, 1.0 / 25.4),
        (Unit::Centimetre, 1.0 / 2.54),
        (Unit::Metre, 1000.0 / 25.4),
        (Unit::Kilometre, 1000000.0 / 25.4),
        (Unit::DrawingDefault, 1.0),
        (Unit::PageDefault, 1.0),
        (Unit::Point, 1.0 / 72.0),
        // As amended: DT is unscaled, like DL, DP, DA and DE. MS-VSDX's 1/72 is not read.
        (Unit::TypeDefault, 1.0),
        (Unit::Pica, 1.0 / 6.0),
        (Unit::Didot, didot),
        (Unit::Cicero, 12.0 * didot),
        (Unit::Degree, std::f64::consts::PI / 180.0),
        (Unit::Radian, 1.0),
        (Unit::AngleDefault, 1.0),
        (Unit::ElapsedDay, 1.0),
        (Unit::ElapsedHour, 1.0 / 24.0),
        (Unit::ElapsedMinute, 1.0 / 1440.0),
        (Unit::ElapsedSecond, 1.0 / 86400.0),
        (Unit::ElapsedWeek, 7.0),
        (Unit::DurationDefault, 1.0),
        // Percent stores the fraction.
        (Unit::Percent, 0.01),
        (Unit::Date, 1.0),
        (Unit::Unitless, 1.0),
    ]
}

#[test]
fn the_internal_value_of_one_of_each_unit_is_the_specs_column() {
    for (unit, per_unit) in table() {
        let q = Quantity::from_value(1.0, unit);
        assert!(close(q.stored, per_unit), "1 {unit:?} stores {} and the spec says {per_unit}", q.stored);
        assert_eq!(q.unit, unit);
        assert!(close(unit.base_per_unit(), per_unit), "{unit:?}");
        // And back: the number shown for the stored one.
        assert!(close(q.shown(), 1.0), "{unit:?} shows {}", q.shown());
    }
}

/// The units the table above does not reach are the acre and hectare (squares) and the compounds, which have their own tests.
#[test]
fn the_table_covers_every_unit_but_the_areas_and_the_compounds() {
    let in_table: Vec<Unit> = table().iter().map(|(u, _)| *u).collect();
    let others = [Unit::Acre, Unit::Hectare, Unit::FeetInches, Unit::DegreesMinutesSeconds, Unit::CiceroDidot, Unit::PicaPoints];
    for unit in Unit::ALL {
        assert!(in_table.contains(&unit) || others.contains(&unit), "{unit:?} is in neither list");
    }
}

/// Given `1 in`, `2.54 cm`, `1 ft` and `36 pt`, expect stored 1, 1, 12 and 0.5, and `U` IN, CM, FT, PT.
#[test]
fn lengths_are_stored_in_inches() {
    let rows: &[(f64, Unit, f64, &str)] =
        &[(1.0, Unit::Inch, 1.0, "IN"), (2.54, Unit::Centimetre, 1.0, "CM"), (1.0, Unit::Foot, 12.0, "FT"), (36.0, Unit::Point, 0.5, "PT")];
    for (value, unit, stored, code) in rows {
        let q = Quantity::from_value(*value, *unit);
        assert!(close(q.stored, *stored), "{value} {code}: stored {} not {stored}", q.stored);
        assert_eq!(q.unit.code(), *code);
        // The naive result: keeping the number as typed (`2.54 cm` stored as 2.54) is wrong for all but the inch.
        if *unit != Unit::Inch {
            assert!((q.stored - value).abs() > 1e-3, "{value} {code} stored as typed");
        }
    }
    // 2.54 cm is one inch to the last digit, not to within a rounding error: a table of factors that multiplies by 1/2.54 does not
    // give this.
    assert_eq!(Quantity::from_value(2.54, Unit::Centimetre).stored, 1.0);
    assert_eq!(Quantity::from_value(25.4, Unit::Millimetre).stored, 1.0);
}

/// Given `180 deg`, expect stored about 3.14159265 and `U` DEG; given `1 ew` and `12 eh`, expect 7 and 0.5.
#[test]
fn angles_are_stored_in_radians_and_durations_in_days() {
    let deg = Quantity::from_value(180.0, Unit::Degree);
    assert!(close(deg.stored, std::f64::consts::PI) && deg.unit.code() == "DEG");
    assert!((deg.stored - 180.0).abs() > 1.0, "naive: degrees kept as typed");
    assert!(close(Quantity::from_value(1.0, Unit::ElapsedWeek).stored, 7.0));
    assert!(close(Quantity::from_value(12.0, Unit::ElapsedHour).stored, 0.5));
}

/// Given `1 ew`, `7 ed` and `168 eh`, expect one stored value, 7.
#[test]
fn a_week_is_seven_days_is_168_hours() {
    for (value, unit) in [
        (1.0, Unit::ElapsedWeek),
        (7.0, Unit::ElapsedDay),
        (168.0, Unit::ElapsedHour),
        (10080.0, Unit::ElapsedMinute),
        (604800.0, Unit::ElapsedSecond),
    ] {
        let stored = Quantity::from_value(value, unit).stored;
        assert!(close(stored, 7.0), "{value} {unit:?} stored {stored}");
    }
}

/// Given a formula `30 deg` in an angle cell, expect `V` about 0.5235987756 and `U` `DEG`.
/// Storing the number shown gives 30. (The cell types of those two cells are the document's; the stored value and the `U` are this crate's.)
#[test]
fn c_geometry_thirty_degrees_is_stored_in_radians_and_shown_in_degrees() {
    let q = Quantity::from_value(30.0, Unit::Degree);
    // The spec's 0.5235987756 is pi / 6, which is what is compared (the constant has to be written that way for clippy).
    assert!((q.stored - std::f64::consts::FRAC_PI_6).abs() < 1e-12, "{}", q.stored);
    assert_eq!(q.unit.code(), "DEG");
    assert!(close(q.shown(), 30.0));
    assert!((q.stored - 30.0).abs() > 1.0, "naive: the number shown is stored");
}

/// `1 ACRE` is 43560 square feet in square inches, 6272640, `U` AC; `1 HA` about 15500031, `U` HA.
#[test]
fn areas_are_stored_in_square_inches() {
    let acre = Quantity::from_value(1.0, Unit::Acre);
    assert_eq!(acre.stored, 43560.0 * 144.0);
    assert_eq!(acre.stored, 6272640.0);
    assert_eq!(acre.unit.code(), "AC");
    let ha = Quantity::from_value(1.0, Unit::Hectare);
    assert!((ha.stored - 15500031.0).abs() < 1.0, "1 HA stores {}", ha.stored);
    assert!(close(ha.stored, 10_000.0 * (1000.0 / 25.4) * (1000.0 / 25.4)));
    assert_eq!(ha.unit.code(), "HA");
    assert!(close(ha.shown(), 1.0) && close(acre.shown(), 1.0));
    // Naive: the same factor as a length (a hectare as 10 000 metres, or an acre as 1).
    assert!(ha.stored > 1.0e6 && ha.stored < 1.0e8);
}

/// `40%` is 0.4, with `U` PER.
#[test]
fn percent_stores_the_fraction() {
    let q = Quantity::from_value(40.0, Unit::Percent);
    assert!(close(q.stored, 0.4));
    assert_eq!(q.unit.code(), "PER");
    assert!(close(Quantity::from_value(100.0, Unit::Percent).stored, 1.0), "1 means 100 percent");
}

/// `2 DL` with centimetre drawing units is stored 2 and shown as 5.08 cm; `3[cm,D]` with inch drawing
/// units is stored about 1.18110236 and shown in inches.
#[test]
fn a_value_is_shown_in_the_unit_the_context_asks_for() {
    let two_dl = Quantity::from_value(2.0, Unit::DrawingDefault);
    assert!(close(two_dl.stored, 2.0));
    assert!(close(two_dl.value_in(Unit::Centimetre).unwrap(), 5.08));
    // Naive: storing the number shown.
    assert!((two_dl.value_in(Unit::Centimetre).unwrap() - two_dl.stored).abs() > 1.0);

    let three_cm = Quantity::from_value(3.0, Unit::Centimetre);
    assert!((three_cm.stored - 1.18110236).abs() < 1e-8, "{}", three_cm.stored);
    assert!(close(three_cm.value_in(Unit::Inch).unwrap(), 1.1811023622047243));
    assert!(close(three_cm.value_in(Unit::Centimetre).unwrap(), 3.0));
}

/// Of functions-operators (the stored number is what arithmetic works on): `7 ft` is stored 84 and shown 7 ft; and a
/// conversion keeps the stored number and changes the unit.
#[test]
fn a_conversion_changes_the_unit_and_never_the_stored_number() {
    let seven_ft = Quantity::from_value(7.0, Unit::Foot);
    assert!(close(seven_ft.stored, 84.0));
    let shown_in_inches = seven_ft.convert(Unit::Inch).unwrap();
    assert_eq!(shown_in_inches.stored, seven_ft.stored);
    assert_eq!(shown_in_inches.unit, Unit::Inch);
    assert!(close(shown_in_inches.shown(), 84.0));
    // functions-operators `3+2.54 cm`: the unit is the second operand's and the stored number the sum of stored numbers;
    // the shown number is 10.16 cm and not 5.54 cm.
    let sum = Quantity::new(3.0 + Quantity::from_value(2.54, Unit::Centimetre).stored, Unit::Centimetre);
    assert!(close(sum.stored, 4.0) && close(sum.shown(), 10.16), "{}", sum.shown());
}

/// A value cannot be shown in a unit of another measure: a length is not an angle (lengths in inches, angles in radians,
/// durations in days).
#[test]
fn a_unit_of_another_measure_is_refused() {
    let length = Quantity::from_value(1.0, Unit::Inch);
    for unit in [Unit::Degree, Unit::ElapsedDay, Unit::Percent, Unit::Acre, Unit::Date, Unit::Unitless] {
        assert_eq!(length.value_in(unit), None, "{unit:?}");
        assert_eq!(length.convert(unit), None, "{unit:?}");
    }
    assert!(Quantity::from_value(1.0, Unit::Degree).value_in(Unit::Radian).is_some());
    assert!(Quantity::from_value(1.0, Unit::Acre).value_in(Unit::Hectare).is_some());
    assert!(Quantity::from_value(1.0, Unit::Unitless).value_in(Unit::Unitless).is_some());
    assert_eq!(Unit::Acre.measure(), Measure::Area);
    assert_eq!(Unit::DrawingDefault.measure(), Measure::Length, "the default units are implicit lengths");
    assert_eq!(Unit::TypeDefault.measure(), Measure::Length);
    assert_eq!(Unit::AngleDefault.measure(), Measure::Angle);
    assert_eq!(Unit::DurationDefault.measure(), Measure::Duration);
    assert_eq!(Unit::Unitless.measure(), Measure::None);
}

/// Hostile numbers go through unchanged and never panic: the evaluator (not this crate) decides what a NaN or infinity means.
#[test]
fn non_finite_numbers_pass_through() {
    for unit in Unit::ALL {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, f64::MAX, f64::MIN_POSITIVE, 0.0, -0.0] {
            let q = Quantity::from_value(value, unit);
            let _ = q.shown();
            let _ = q.value_in(unit);
        }
    }
    assert!(Quantity::from_value(f64::NAN, Unit::Inch).stored.is_nan());
    assert!(Quantity::from_value(f64::INFINITY, Unit::Foot).stored.is_infinite());
}

/// A conversion does not overflow when the true value fits. `to_internal` and `from_internal` used to multiply by the numerator
/// before they divided by the denominator, so a number a factor below `f64::MAX` came out as infinity in the middle of the sum. The
/// expected values are the exact products, worked out outside (the rational product, rounded once).
#[test]
fn a_conversion_whose_true_value_fits_does_not_overflow() {
    let to_internal = [
        (Unit::Hectare, 2.605578372271685e298, 4.038654554330221e305),
        (Unit::NauticalMile, 1e302, 7.291338582677165e306),
        (Unit::Kilometre, 1e303, 3.937007874015748e307),
        (Unit::Metre, 1e306, 3.937007874015748e307),
    ];
    for (unit, x, wanted) in to_internal {
        let got = unit.to_internal(x);
        assert!(got.is_finite() && (got - wanted).abs() <= 1e-12 * wanted, "{unit:?}.to_internal({x:e}) = {got:e}, wanted {wanted:e}");
        let back = Quantity::from_value(x, unit).stored;
        assert_eq!(back.to_bits(), got.to_bits(), "{unit:?}: `from_value` takes the same route");
    }
    let from_internal = [
        (Unit::Hectare, 4.17e305, 2.6903172000000004e298),
        (Unit::NauticalMile, 1e307, 1.3714902807775378e302),
        (Unit::Kilometre, 1e308, 2.54e303),
        (Unit::Metre, 1e308, 2.54e306),
    ];
    for (unit, x, wanted) in from_internal {
        let got = unit.from_internal(x);
        assert!(got.is_finite() && (got - wanted).abs() <= 1e-12 * wanted, "{unit:?}.from_internal({x:e}) = {got:e}, wanted {wanted:e}");
    }
    // A value that does not fit is still an infinity, with its sign, and not a wrong finite number.
    assert_eq!(Unit::Kilometre.to_internal(f64::MAX), f64::INFINITY);
    assert_eq!(Unit::Kilometre.to_internal(f64::MIN), f64::NEG_INFINITY);
    // Where the factor is below one, the largest number in any unit comes out finite, whichever of its two parts is large.
    assert!(Unit::Kilometre.from_internal(f64::MAX).is_finite());
    assert!(Unit::Degree.to_internal(f64::MAX).is_finite());
    assert!(Unit::Degree.to_internal(f64::MIN).is_finite());
}

/// Over every unit and the whole range of sizes: wherever the product or the quotient with the unit's factor fits (a margin of a
/// thousandth keeps the rounding of that check out of it), the conversion is finite and within 1E-12 of it; and a number that cannot fit
/// is an infinity, never NaN.
#[test]
fn every_unit_converts_every_size_that_fits() {
    let mut checked = 0_u32;
    for unit in Unit::ALL {
        let base = unit.base_per_unit();
        for exponent in -320..=308 {
            for mantissa in [1.0, 1.7976931348623157, 3.3, 9.99] {
                let x = mantissa * 10f64.powi(exponent);
                if !x.is_finite() {
                    continue;
                }
                for sign in [1.0, -1.0] {
                    let x = sign * x;
                    let (up, down) = (x * base, x / base);
                    for (got, direct, up_or_down) in [(unit.to_internal(x), up, "to_internal"), (unit.from_internal(x), down, "from_internal")] {
                        assert!(!got.is_nan(), "{unit:?}.{up_or_down}({x:e})");
                        if direct.is_finite() && direct.abs() <= f64::MAX * 0.999 && direct.abs() >= f64::MIN_POSITIVE * 1e6 {
                            assert!(
                                got.is_finite() && (got - direct).abs() <= 1e-12 * direct.abs(),
                                "{unit:?}.{up_or_down}({x:e}) = {got:e}, wanted {direct:e}"
                            );
                            checked += 1;
                        }
                    }
                }
            }
        }
    }
    assert!(checked > 40_000, "{checked}");
}

/// Zero in any unit and at any dimension shows as zero, and a zero shown in any unit at any dimension is stored as zero. The scale of
/// a unit to a large power (`63360^-128`) underflows to 0 or overflows to infinity, and 0 / 0 and 0 * infinity are NaN.
#[test]
fn zero_is_zero_in_every_unit_and_at_every_dimension() {
    for unit in Unit::ALL {
        for dim in i8::MIN..=i8::MAX {
            for zero in [0.0, -0.0] {
                let q = Quantity::with_dim(zero, unit, dim);
                assert_eq!(q.shown(), 0.0, "{unit:?}^{dim} shows {}", q.shown());
                for target in Unit::ALL {
                    if let Some(shown) = q.value_in(target) {
                        assert_eq!(shown, 0.0, "{unit:?}^{dim} in {target:?} shows {shown}");
                    }
                }
                let stored = Quantity::from_shown(zero, unit, dim).stored;
                assert_eq!(stored, 0.0, "0 {unit:?}^{dim} is stored as {stored}");
            }
        }
    }
    // The example the proofs found.
    assert_eq!(Quantity::with_dim(0.0, Unit::Mile, -128).shown(), 0.0);
}
