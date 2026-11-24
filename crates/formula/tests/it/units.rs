//! Number literals with units.

use diacraft_formula::{Expr, ImplicitMeasure, Number, ParseErrorKind, parse};
use diacraft_units::{Quantity, Unit};

use crate::{bin, close, err, n, ok, shape};

/// The number a literal parses to.
fn number(src: &str) -> Number {
    match ok(src) {
        Expr::Number(n) => n,
        other => panic!("{src}: {other:?}"),
    }
}

/// The stored value and unit of a literal.
fn stored(src: &str) -> (f64, Unit) {
    let q = number(src).quantity().unwrap_or_else(|| panic!("{src} has no quantity"));
    (q.stored, q.unit)
}

#[test]
fn every_suffix_in_any_case() {
    // The table, suffix by suffix: the unit, and the stored value that column gives (among them).
    let table: [(&str, Unit, f64); 25] = [
        ("IN", Unit::Inch, 1.0),
        ("FT", Unit::Foot, 12.0),
        ("YD", Unit::Yard, 36.0),
        ("MI", Unit::Mile, 63360.0),
        ("NM", Unit::NauticalMile, 1_852_000.0 / 25.4),
        ("MM", Unit::Millimetre, 1.0 / 25.4),
        ("CM", Unit::Centimetre, 1.0 / 2.54),
        ("M", Unit::Metre, 1000.0 / 25.4),
        ("KM", Unit::Kilometre, 1_000_000.0 / 25.4),
        ("DL", Unit::DrawingDefault, 1.0),
        ("DP", Unit::PageDefault, 1.0),
        ("PT", Unit::Point, 1.0 / 72.0),
        ("P", Unit::Pica, 1.0 / 6.0),
        ("D", Unit::Didot, 0.3759715104 / 25.4),
        ("C", Unit::Cicero, 12.0 * 0.3759715104 / 25.4),
        ("DEG", Unit::Degree, std::f64::consts::PI / 180.0),
        ("RAD", Unit::Radian, 1.0),
        ("DA", Unit::AngleDefault, 1.0),
        ("ED", Unit::ElapsedDay, 1.0),
        ("EH", Unit::ElapsedHour, 1.0 / 24.0),
        ("EM", Unit::ElapsedMinute, 1.0 / 1440.0),
        ("ES", Unit::ElapsedSecond, 1.0 / 86400.0),
        ("EW", Unit::ElapsedWeek, 7.0),
        ("DE", Unit::DurationDefault, 1.0),
        ("ACRE", Unit::Acre, 6_272_640.0),
    ];
    for (suffix, unit, per) in table {
        for text in [suffix.to_string(), suffix.to_lowercase(), format!("{}{}", &suffix[..1], suffix[1..].to_lowercase())] {
            for src in [format!("2 {text}"), format!("2{text}")] {
                assert_eq!(number(&src), Number::with_unit(2.0, unit), "{src}");
                let (value, u) = stored(&src);
                assert_eq!(u, unit, "{src}");
                assert!(close(value, 2.0 * per), "{src}: {value}");
            }
        }
    }
    // The hectare, which has a dotted spelling too, and the acre and hectare by their other names.
    assert_eq!(number("1 HA"), Number::with_unit(1.0, Unit::Hectare));
}

#[test]
fn stored_values_and_units() {
    for (src, value, unit) in [
        ("1 in", 1.0, Unit::Inch),
        ("2.54 cm", 1.0, Unit::Centimetre),
        ("1 ft", 12.0, Unit::Foot),
        ("36 pt", 0.5, Unit::Point),
        ("180 deg", std::f64::consts::PI, Unit::Degree),
        ("1 ew", 7.0, Unit::ElapsedWeek),
        ("12 eh", 0.5, Unit::ElapsedHour),
        ("7 ed", 7.0, Unit::ElapsedDay),
        ("168 eh", 7.0, Unit::ElapsedHour),
        ("2 DL", 2.0, Unit::DrawingDefault),
        ("1 ACRE", 6_272_640.0, Unit::Acre),
    ] {
        let (v, u) = stored(src);
        assert!(close(v, value) && u == unit, "{src}: {v} {u:?}");
    }
    // A hectare is about 15500031 square inches.
    let (v, u) = stored("1 HA");
    assert!(close(v, 15_500_031.0) || (v - 15_500_031.0).abs() < 1.0, "{v}");
    assert_eq!(u, Unit::Hectare);
    // A plain number has no unit.
    assert_eq!(stored("5"), (5.0, Unit::Unitless));
}

#[test]
fn names_the_areas_by_their_longer_spellings() {
    for (src, unit) in [
        ("3 ACRES", Unit::Acre),
        ("3 acres", Unit::Acre),
        ("3 HECTARE", Unit::Hectare),
        ("3 hectares", Unit::Hectare),
        ("3 HA.", Unit::Hectare),
        ("3 EW.", Unit::ElapsedWeek),
        ("3 ed.", Unit::ElapsedDay),
        ("3 eweek", Unit::ElapsedWeek),
        ("3 eday", Unit::ElapsedDay),
        ("3 ehour", Unit::ElapsedHour),
        ("3 eminute", Unit::ElapsedMinute),
        ("3 esecond", Unit::ElapsedSecond),
    ] {
        assert_eq!(number(src), Number::with_unit(3.0, unit), "{src}");
    }
}

#[test]
fn a_power_straight_after_a_unit_belongs_to_the_unit() {
    // `3 IN^2` is three square inches; `(3 in)^2` is the operator on a literal.
    assert_eq!(number("3 IN^2"), Number::Measured { value: 3.0, unit: Unit::Inch, power: Some(2) });
    assert_eq!(number("3IN^2"), number("3 in^2"));
    assert_eq!(ok("(3 in)^2"), bin(diacraft_formula::BinaryOp::Pow, Expr::Number(Number::with_unit(3.0, Unit::Inch)), n(2.0)));
    assert_ne!(ok("3 IN^2"), ok("(3 IN)^2"));
    // `3 FT^2`; the dimension and the stored value of a power are the evaluator's.
    assert_eq!(number("3 FT^2"), Number::Measured { value: 3.0, unit: Unit::Foot, power: Some(2) });
    assert_eq!(number("3 FT^2").quantity(), None);
    // The other examples of the spec.
    assert_eq!(shape("3 IN^2+1"), "(+ 3IN^2 1)");
    assert_eq!(shape("3 in^2-1 in"), "(- 3IN^2 1IN)");
    assert_eq!(shape("6 IN^2/2 IN"), "(/ 6IN^2 2IN)");
    assert_eq!(shape("MAGNITUDE(1, 2 in^2, 1, 1)"), "(call MAGNITUDE 1 2IN^2 1 1)");
    assert_eq!(shape("SQRT(16 in^4)"), "(call SQRT 16IN^4)");
}

#[test]
fn a_power_out_of_range_or_not_whole_is_not_the_literals() {
    // R28 gives `#DIM!` for both; the tree has them and `calc` decides. 70 and 70 are in range each.
    assert_eq!(shape("1 ft^70 * 1 ft^70"), "(* 1FT^70 1FT^70)");
    assert_eq!(shape("1 IN^100*1 IN^27"), "(* 1IN^100 1IN^27)");
    // A fractional power is no part of the literal: the operator, on a number with a unit.
    assert_eq!(shape("2 cm^1.5"), "(^ 2CM 1.5)");
    assert_eq!(shape("2 cm^.5"), "(^ 2CM 0.5)");
    // Far out of range still parses: 999 is past 127 and is `#DIM!`'s to give.
    assert_eq!(number("2 IN^999"), Number::Measured { value: 2.0, unit: Unit::Inch, power: Some(999) });
    // Past what a power can hold, an error and not a wrapped number.
    assert_eq!(err("2 IN^99999999999").kind, ParseErrorKind::NumberOutOfRange);
}

#[test]
fn a_sign_in_the_power_belongs_to_the_power() {
    // E11 leaves this `unknown`; another reader accepts the sign, and R28 allows negative powers. A bare `^` stays an operator.
    assert_eq!(number("2 IN^-1"), Number::Measured { value: 2.0, unit: Unit::Inch, power: Some(-1) });
    assert_eq!(number("2 IN^+3"), Number::Measured { value: 2.0, unit: Unit::Inch, power: Some(3) });
    assert_eq!(shape("2^-1"), "(^ 2 (neg 1))");
    assert_eq!(shape("2 IN ^2"), "(^ 2IN 2)");
    assert_eq!(shape("2 IN^ 2"), "(^ 2IN 2)");
    assert_eq!(shape("2 IN^x"), "(^ 2IN @x)");
    assert_eq!(shape("2 IN^-x"), "(^ 2IN (neg @x))");
}

#[test]
fn a_literal_takes_the_longest_match() {
    // `1E5` is an exponent; `1EM` has none, so it is a number with a unit; `1E` is a number and a name that is no operator.
    assert_eq!(ok("1E5"), n(100_000.0));
    assert_eq!(ok("1e-5"), n(0.00001));
    assert_eq!(number("1EM"), Number::with_unit(1.0, Unit::ElapsedMinute));
    assert_eq!(number("1 EM"), Number::with_unit(1.0, Unit::ElapsedMinute));
    assert_eq!(number("1EH"), Number::with_unit(1.0, Unit::ElapsedHour));
    assert_eq!(number("2 D"), Number::with_unit(2.0, Unit::Didot));
    assert_eq!(number("2D"), Number::with_unit(2.0, Unit::Didot));
    assert_eq!(err("1E").kind, ParseErrorKind::ExpectedOperator);
    assert_eq!(err("1E+").kind, ParseErrorKind::ExpectedOperator);
    // An exponent and then a unit.
    assert_eq!(number("1E2M"), Number::with_unit(100.0, Unit::Metre));
    // `5"` is no literal anywhere: after a number a quote is not a unit, and a string needs an operator before it.
    assert_eq!(err("5\"").kind, ParseErrorKind::ExpectedOperator);
    // A longer run of letters than any unit is not that unit with something left over.
    assert_eq!(err("3 MINUS").kind, ParseErrorKind::ExpectedOperator);
    assert_eq!(err("3 CMXX").kind, ParseErrorKind::ExpectedOperator);
    assert_eq!(err("3 INCH").kind, ParseErrorKind::ExpectedOperator);
    // A unit is a number's, not a name's: after a reference, a unit is a name that is no operator.
    assert_eq!(err("Width IN").kind, ParseErrorKind::ExpectedOperator);
}

#[test]
fn compound_literals_18() {
    // Feet then inches, `U` is `F_I`, stored in inches: `8FT 4IN` is 100.
    assert_eq!(number("8FT 4IN"), Number::FeetInches { feet: 8.0, inches: 4.0 });
    assert_eq!(number("8 FT 4 IN"), Number::FeetInches { feet: 8.0, inches: 4.0 });
    assert_eq!(number("8ft 4.5in"), Number::FeetInches { feet: 8.0, inches: 4.5 });
    assert_eq!(stored("8FT 4IN"), (100.0, Unit::FeetInches));
    // Ciceros then didots, picas then points.
    assert_eq!(number("2C4"), Number::CiceroDidot { ciceros: 2.0, didots: 4.0 });
    assert_eq!(number("3P6"), Number::PicaPoints { picas: 3.0, points: 6.0 });
    assert_eq!(number("3p6.5"), Number::PicaPoints { picas: 3.0, points: 6.5 });
    let (p, u) = stored("3P6");
    assert!(close(p, 3.0 / 6.0 + 6.0 / 72.0) && u == Unit::PicaPoints, "{p}");
    let (c, u) = stored("2C4");
    assert!(close(c, Quantity::cicero_didot(2.0, 4.0).stored) && u == Unit::CiceroDidot);
    // Without the second number they are the plain units.
    assert_eq!(number("2 C"), Number::with_unit(2.0, Unit::Cicero));
    assert_eq!(number("3 P"), Number::with_unit(3.0, Unit::Pica));
    assert_eq!(number("8 FT"), Number::with_unit(8.0, Unit::Foot));
    // A sum of feet and inches is not the compound: the operator separates them.
    assert_eq!(shape("8 FT + 4 IN"), "(+ 8FT 4IN)");
    assert_eq!(shape("8FT 4IN+1"), "(+ 8FT4IN 1)");
    // A number after feet that is not inches is an error, not a silent reading.
    assert_eq!(err("8FT 4").kind, ParseErrorKind::ExpectedOperator);
    assert_eq!(err("8FT 4INX").kind, ParseErrorKind::ExpectedOperator);
}

#[test]
fn degrees_minutes_seconds() {
    // `10.25°` is the value of 10 degrees 15 minutes (decimal degrees are the reference page's); `U` is `AD`.
    assert_eq!(number("10.25°"), Number::DegreesMinutesSeconds { degrees: 10.25, minutes: 0.0, seconds: 0.0 });
    assert_eq!(number("10°15'"), Number::DegreesMinutesSeconds { degrees: 10.0, minutes: 15.0, seconds: 0.0 });
    assert_eq!(number("10°15'30\""), Number::DegreesMinutesSeconds { degrees: 10.0, minutes: 15.0, seconds: 30.0 });
    assert_eq!(number("10°30\""), Number::DegreesMinutesSeconds { degrees: 10.0, minutes: 0.0, seconds: 30.0 });
    let (a, u) = stored("10.25°");
    let (b, _) = stored("10°15'");
    assert!(close(a, b) && close(a, 10.25_f64.to_radians()) && u == Unit::DegreesMinutesSeconds, "{a} {b}");
    assert_eq!(number("10°").quantity(), Some(Quantity::degrees_minutes_seconds(10.0, 0.0, 0.0)));
    assert_eq!(shape("10°+5°"), "(+ 10deg0min0sec 5deg0min0sec)");
    // A degree sign with nothing before it is no literal.
    assert!(parse("°").ast.is_err());
}

#[test]
fn implicit_units_22() {
    // `number [unit, flag]`; the flag names the measure, in either case, and the unit is read in any case.
    assert_eq!(number("3[cm,D]"), Number::Implicit { value: 3.0, unit: Unit::Centimetre, measure: ImplicitMeasure::Drawing });
    assert_eq!(number("3 [CM, d]"), number("3[cm,D]"));
    for (flag, measure) in [
        ('A', ImplicitMeasure::Angle),
        ('D', ImplicitMeasure::Drawing),
        ('E', ImplicitMeasure::Duration),
        ('P', ImplicitMeasure::Page),
        ('T', ImplicitMeasure::Type),
    ] {
        assert_eq!(number(&format!("1[in,{flag}]")), Number::Implicit { value: 1.0, unit: Unit::Inch, measure }, "{flag}");
        assert_eq!(number(&format!("1[in,{}]", flag.to_ascii_lowercase())), number(&format!("1[in,{flag}]")));
        assert_eq!(measure.flag(), flag);
        assert_eq!(ImplicitMeasure::from_flag(flag), Some(measure));
    }
    // 3 cm is about 1.18110236 inches, to be shown in the drawing unit.
    let (v, _) = stored("3[cm,D]");
    assert!((v - 1.18110236).abs() < 1e-8, "{v}");
    assert_eq!(stored("2 DL"), (2.0, Unit::DrawingDefault));
    assert_eq!(err("3[cm,Q]").kind, ParseErrorKind::ExpectedOperator);
    assert_eq!(err("3[cm]").kind, ParseErrorKind::ExpectedOperator);
    assert_eq!(err("3[cm,D").kind, ParseErrorKind::ExpectedOperator);
}

#[test]
fn m1_ruling_42_a_dt_literal_is_unscaled() {
    // amended give DT the factor 1: `12 DT` stores 12.
    assert_eq!(number("12 DT"), Number::with_unit(12.0, Unit::TypeDefault));
    assert_eq!(stored("12 DT"), (12.0, Unit::TypeDefault));
    assert_eq!(stored("12DT"), (12.0, Unit::TypeDefault));
    assert_eq!(stored("0.5 dt"), (0.5, Unit::TypeDefault));
    // A point is still 1/72 of an inch, and the five defaults are alike.
    assert_eq!(stored("12 PT"), (12.0 / 72.0, Unit::Point));
    for (src, unit) in
        [("12 DL", Unit::DrawingDefault), ("12 DP", Unit::PageDefault), ("12 DA", Unit::AngleDefault), ("12 DE", Unit::DurationDefault)]
    {
        assert_eq!(stored(src), (12.0, unit), "{src}");
    }
    // The units crate says the same, so a literal read any way gives the one answer: stored 12, and shown 12.
    assert_eq!(Unit::TypeDefault.to_internal(12.0), 12.0);
    let q = Quantity::from_value(12.0, Unit::TypeDefault);
    assert_eq!((q.stored, q.shown()), (12.0, 12.0));
    let q = number("12 DT").quantity().unwrap();
    assert_eq!((q.stored, q.shown()), (12.0, 12.0));
    // `2 DT` is 2 in internal units; 1/72 of it fails.
    assert_eq!(stored("2 DT"), (2.0, Unit::TypeDefault));
    // An implicit DT is a literal too.
    assert_eq!(stored("12[dt,T]"), (12.0, Unit::TypeDefault));
}

#[test]
fn the_reference_pages_spellings_of_a_square_unit() {
    // `SQ. IN.`, `SQ IN` and `IN.^2` are a length unit squared.
    let square_inches = Number::Measured { value: 3.0, unit: Unit::Inch, power: Some(2) };
    for src in ["3 SQ. IN.", "3 SQ IN", "3 sq in", "3 sq. in", "3 SQ.IN.", "3 IN.^2"] {
        assert_eq!(number(src), square_inches, "{src}");
    }
    assert_eq!(number("3 SQ. FT."), Number::Measured { value: 3.0, unit: Unit::Foot, power: Some(2) });
    assert_eq!(err("3 SQ").kind, ParseErrorKind::ExpectedOperator);
    assert_eq!(err("3 SQ. XYZ.").kind, ParseErrorKind::ExpectedOperator);
}

#[test]
fn a_mixed_number() {
    // The number may be mixed, `5 1/2`. A plain division is not.
    assert_eq!(ok("5 1/2"), n(5.5));
    assert_eq!(number("5 1/2 in"), Number::with_unit(5.5, Unit::Inch));
    assert_eq!(number("5 1/2[in,D]"), Number::Implicit { value: 5.5, unit: Unit::Inch, measure: ImplicitMeasure::Drawing });
    assert_eq!(ok("2   3/4"), n(2.75));
    assert_eq!(shape("1/2"), "(/ 1 2)");
    assert_eq!(shape("5/2"), "(/ 5 2)");
    assert_eq!(shape("5 / 2"), "(/ 5 2)");
    assert_eq!(shape("5*1/2"), "(/ (* 5 1) 2)");
    // Only a whole number takes a fraction, a zero denominator is an error, and `5 1/2.5` is not a mixed number.
    assert_eq!(err("5 1/0").kind, ParseErrorKind::ExpectedOperator);
    assert!(parse("5.5 1/2").ast.is_err());
    assert!(parse("5 1/2.5").ast.is_err());
    assert_eq!(shape("F(1 1/2, 3)"), "(call F 1.5 3)");
}

#[test]
fn a_currency_literal() {
    // The code is one of the 192, in capitals, with a blank before it or none. The whole table, the
    // blank, the case and are in `currency.rs`; here is what this test held before.
    assert_eq!(ok("12.5USD"), Expr::Currency { value: 12.5, code: "USD".into() });
    assert_eq!(ok("5 USD"), Expr::Currency { value: 5.0, code: "USD".into() });
    assert_eq!(ok("5USD"), Expr::Currency { value: 5.0, code: "USD".into() });
    assert_eq!(shape("SUM(5 USD, 3)"), "(call SUM 5USD 3)");
    // Changed from `shape("5 eur")` being `5eur`, a code "kept as written and checked by whoever knows the codes": the parser knows
    // them now, and a literal does not ignore case, so lower case is no code.
    assert_eq!(err("5 eur").kind, ParseErrorKind::ExpectedOperator);
    assert_eq!(err("5eur").kind, ParseErrorKind::ExpectedOperator);
    // DEG and RAD are units and have three letters. The comment said "a unit wins over a code"; no code is a unit, so
    // there is nothing to win, and the assertion stands.
    assert_eq!(shape("5 DEG"), "5DEG");
    // Two letters or four are no code; and a currency is not an operand of a following operator word.
    assert_eq!(err("5 AB").kind, ParseErrorKind::ExpectedOperator);
    assert_eq!(err("5 ABCD").kind, ParseErrorKind::ExpectedOperator);
    assert_eq!(shape("5 USD _GT_ 2"), "(_GT_ 5USD 2)");
}

#[test]
fn a_number_that_does_not_fit_is_an_error() {
    // R3 puts 1.7976e308 as the largest: beyond a double is no number, never an infinity in the tree.
    assert_eq!(err("1e999").kind, ParseErrorKind::NumberOutOfRange);
    assert_eq!(err("1e309").kind, ParseErrorKind::NumberOutOfRange);
    assert_eq!(ok("1e308"), n(1e308));
    // Underflow is zero, which is finite.
    assert_eq!(ok("1e-999"), n(0.0));
    // A number of 2,000 digits is hostile, not large.
    let long = "9".repeat(2000);
    assert_eq!(err(&long).kind, ParseErrorKind::NumberOutOfRange);
    assert_eq!(err(&format!("0.{long}")).kind, ParseErrorKind::NumberOutOfRange);
    // 300 digits is a number below the cap, and it is finite only up to 309.
    assert!(parse(&"9".repeat(300)).ast.is_ok());
    assert_eq!(err(&"9".repeat(400)).kind, ParseErrorKind::NumberOutOfRange);
}

#[test]
fn units_in_the_spec_vectors_of_the_operators() {
    assert_eq!(shape("1cm/4"), "(/ 1CM 4)");
    assert_eq!(shape("7 ft*3"), "(* 7FT 3)");
    assert_eq!(shape("3+2.54 cm"), "(+ 3 2.54CM)");
    assert_eq!(shape("1 ft=12 in"), "(= 1FT 12IN)");
    assert_eq!(shape("1 DL-2.54 cm"), "(- 1DL 2.54CM)");
    assert_eq!(shape("25 mm&\"x\""), "(& 25MM \"x\")");
    assert_eq!(shape("1 ACRE*1 IN"), "(* 1ACRE 1IN)");
    assert_eq!(shape("DATE(2000,1,1)+12 eh"), "(+ (call DATE 2000 1 1) 12EH)");
    assert_eq!(shape("12 eh+DATE(2000,1,1)"), "(+ 12EH (call DATE 2000 1 1))");
    assert_eq!(shape("1 ed-DATE(2000,1,1)"), "(- 1ED (call DATE 2000 1 1))");
    assert_eq!(shape("3 in*50%"), "(* 3IN (% 50))");
    assert_eq!(shape("BOUND(400 deg, 2, FALSE, 0 deg, 1 deg)"), "(call BOUND 400DEG 2 FALSE 0DEG 1DEG)");
    assert_eq!(shape("IF(4<2,3 cm,2 in)"), "(call IF (< 4 2) 3CM 2IN)");
    assert_eq!(shape("-3 cm"), "(neg 3CM)");
}

#[test]
fn date_is_a_base_when_a_power_follows() {
    // R26 lists `DATE` among the bases of a multidimensional literal. It has no suffix of its own (a date has no literal
    // form), so it is read after a number only with a power straight after it.
    assert_eq!(number("3 DATE^2"), Number::Measured { value: 3.0, unit: Unit::Date, power: Some(2) });
    assert_eq!(number("3DATE^2"), number("3 date^2"));
    assert_eq!(number("3 Date^-1"), Number::Measured { value: 3.0, unit: Unit::Date, power: Some(-1) });
    assert_eq!(shape("3 DATE^2+1"), "(+ 3DATE^2 1)");
    assert_eq!(number("3 DATE^2").quantity(), None);
    // No power, or one that is not part of the literal: no suffix.
    assert_eq!(err("3 DATE").kind, ParseErrorKind::ExpectedOperator);
    assert_eq!(err("3 DATE^1.5").kind, ParseErrorKind::ExpectedOperator);
    assert_eq!(err("3 DATE ^2").kind, ParseErrorKind::ExpectedOperator);
    assert_eq!(err("3 DATE^x").kind, ParseErrorKind::ExpectedOperator);
    // And every other base of the list takes a power.
    for base in [
        "IN", "FT", "YD", "MI", "NM", "MM", "CM", "M", "KM", "DL", "DP", "PT", "DT", "P", "D", "C", "DEG", "RAD", "DA", "DATE", "ED", "EH", "EM",
        "ES", "EW", "DE",
    ] {
        assert!(matches!(number(&format!("2 {base}^2")), Number::Measured { power: Some(2), .. }), "{base}");
    }
}

#[test]
fn the_square_shapes_are_for_length_units() {
    // `SQ. IN.`, `SQ IN` and `IN.^2` are the shapes of the units page "for length units". An angle, a duration, an area or
    // a date takes a power by `^` only.
    for src in [
        "3 SQ DEG",
        "3 SQ. DEG.",
        "3 sq rad",
        "3 SQ DA",
        "3 SQ ED",
        "3 SQ. EW.",
        "3 SQ ACRE",
        "3 SQ HA",
        "3 SQ DATE",
        "3 DEG.^2",
        "3 RAD.^2",
        "3 ACRE.^2",
    ] {
        assert_eq!(err(src).kind, ParseErrorKind::ExpectedOperator, "{src}");
    }
    assert_eq!(number("3 DEG^2"), Number::Measured { value: 3.0, unit: Unit::Degree, power: Some(2) });
    // Every length unit reads them, the implicit defaults among them.
    for (unit, text) in [
        (Unit::Inch, "IN"),
        (Unit::Foot, "FT"),
        (Unit::Yard, "YD"),
        (Unit::Mile, "MI"),
        (Unit::NauticalMile, "NM"),
        (Unit::Millimetre, "MM"),
        (Unit::Centimetre, "CM"),
        (Unit::Metre, "M"),
        (Unit::Kilometre, "KM"),
        (Unit::DrawingDefault, "DL"),
        (Unit::PageDefault, "DP"),
        (Unit::Point, "PT"),
        (Unit::TypeDefault, "DT"),
        (Unit::Pica, "P"),
        (Unit::Didot, "D"),
        (Unit::Cicero, "C"),
    ] {
        let squared = Number::Measured { value: 3.0, unit, power: Some(2) };
        for src in [format!("3 SQ {text}"), format!("3 SQ. {text}."), format!("3 {text}.^2")] {
            assert_eq!(number(&src), squared, "{src}");
        }
    }
}

#[test]
fn the_area_units_are_no_base_of_a_power() {
    // The list of bases (IN to DE, with DATE) leaves out `ACRE` and `HA`, which are areas already: a power straight after one is
    // not a literal's power but the operator `^`, on the literal, in every spelling of the two.
    for (src, shape_of) in [
        ("3 ACRE^2", "(^ 3ACRE 2)"),
        ("3 acre^2", "(^ 3ACRE 2)"),
        ("3 ACRES^2", "(^ 3ACRE 2)"),
        ("3 AC^2", "(^ 3ACRE 2)"),
        ("3 HA^2", "(^ 3HA 2)"),
        ("3 HECTARE^2", "(^ 3HA 2)"),
        ("3 HECTARES^2", "(^ 3HA 2)"),
        ("3 HA.^2", "(^ 3HA 2)"),
        ("3 ACRE^-1", "(^ 3ACRE (neg 1))"),
        ("3 HA^2+1", "(+ (^ 3HA 2) 1)"),
    ] {
        assert_eq!(shape(src), shape_of, "{src}");
    }
    // The acre and the hectare alone are still literals, and a unit that is a base still takes its power.
    assert_eq!(number("3 ACRE"), Number::with_unit(3.0, Unit::Acre));
    assert_eq!(number("3 HA"), Number::with_unit(3.0, Unit::Hectare));
    assert_eq!(number("3 M^2"), Number::Measured { value: 3.0, unit: Unit::Metre, power: Some(2) });
}
