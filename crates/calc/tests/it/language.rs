//! The vectors that need an evaluator (literals, ranks, units, conversions, errors), as data.

use diacraft_calc::CellError::{Dim, DivZero, Na, Num, Value};
use diacraft_units::Unit::{Acre, Centimetre, Degree, DrawingDefault, ElapsedWeek, FeetInches, Foot, Hectare, Inch, Percent, Point, TypeDefault};

use crate::{E, check_table, eval, mismatch};

#[test]
fn ranks_and_associativity() {
    check_table(&[
        ("V6", "-3^2", E::N(9.0)),
        ("V6", "-(3^2)", E::N(-9.0)),
        ("V6", "2^-1", E::N(0.5)),
        ("V7", "2^3^2", E::N(64.0)),
        ("V7", "2^(3^2)", E::N(512.0)),
        ("V7", "20/2/5", E::N(2.0)),
        ("V8", "2+3*4", E::N(14.0)),
        ("V8", "(2+3)*4", E::N(20.0)),
        ("V9", "3*4 & \" eggs\"", E::T("12 eggs")),
        ("V9", "1+2 & \"x\"", E::T("3x")),
        ("V10", "1<2=TRUE", E::B(true)),
        ("V11", "40%", E::U(0.4, Percent)),
        ("V11", "2^50%", E::N(2f64.powf(0.5))),
        ("V38", "5 GT 2", E::B(true)),
        ("V38", "5 _GT_ 2", E::B(true)),
        ("V38", "1 + 1 _EQ_ 2", E::B(true)),
        ("V41", "5*2", E::N(10.0)),
        ("V41", "5 * 2", E::N(10.0)),
    ]);
}

#[test]
fn literals() {
    check_table(&[
        ("V12", ".5", E::N(0.5)),
        ("V12", "5.", E::N(5.0)),
        ("V12", "1.5E3", E::N(1500.0)),
        ("V12", "1.5e-1", E::N(0.15)),
        ("V13", "true", E::B(true)),
        ("V13", "FALSE", E::B(false)),
        ("V14", "1 in", E::U(1.0, Inch)),
        ("V14", "2.54 cm", E::U(1.0, Centimetre)),
        ("V14", "1 ft", E::U(12.0, Foot)),
        ("V14", "36 pt", E::U(0.5, Point)),
        ("V15", "180 deg", E::U(std::f64::consts::PI, Degree)),
        ("V15", "1 ew", E::U(7.0, ElapsedWeek)),
        ("V15", "12 eh", E::U(0.5, diacraft_units::Unit::ElapsedHour)),
        ("V31", "7 ed", E::U(7.0, diacraft_units::Unit::ElapsedDay)),
        ("V31", "168 eh", E::U(7.0, diacraft_units::Unit::ElapsedHour)),
        ("V16", "3 IN^2", E::D(3.0, Inch, 2)),
        ("V16", "(3 in)^2", E::D(9.0, Inch, 2)),
        ("V17", "1 ft^70 * 1 ft^70", E::Err(Dim)),
        ("V17", "2 cm^1.5", E::Err(Dim)),
        ("V18", "8FT 4IN", E::U(100.0, FeetInches)),
        ("V19", "4*\"3\"", E::N(12.0)),
        ("V19", "6*2 & \" cents\"", E::T("12 cents")),
        ("V20", "4*\"goat\"", E::Err(Value)),
        ("V20", "4+\"goat\"", E::Err(Value)),
        ("V21", "\"cat\"=\"dog\"", E::B(true)),
        ("V21", "\"2\"=\"2.0\"", E::B(true)),
        ("V22", "2 DL", E::U(2.0, DrawingDefault)),
        ("V22", "3[cm,D]", E::U(3.0 / 2.54, DrawingDefault)),
        ("V44", "1 ACRE", E::U(6272640.0, Acre)),
        ("V44", "1 HA", E::U(10_000.0 * 1000.0 * 1000.0 / (25.4 * 25.4), Hectare)),
        ("V44", "3 FT^2", E::D(432.0, Foot, 2)),
        ("V42", "\"plain\"", E::T("plain")),
        ("V42", "#FF8000", E::Col(255, 128, 0)),
        ("V42", "12.5USD", E::Cy(12.5, "USD")),
        ("V47", "ABS(\"3 in\")", E::U(3.0, Inch)),
        ("V47", "ABS(\"tRuE\")", E::N(1.0)),
        ("V52", "2 DT", E::U(2.0, TypeDefault)),
    ]);
    let v = eval("3 FT^2");
    let encoded = diacraft_calc::encode(&v).unwrap();
    assert_eq!(encoded.u.as_deref(), Some("MULTIDIM"), "U MULTIDIM");
    assert_eq!(encoded.v, "3FT^2", "V holds the number, the unit and the power");
}

#[test]
fn errors() {
    check_table(&[
        ("V32", "7/0", E::Err(DivZero)),
        ("V32", "SQRT(-4)", E::Err(Num)),
        ("V32", "NA()", E::Err(Na)),
        ("V33", "ABS(NA())", E::Err(Na)),
        ("V33", "1+7/0", E::Err(DivZero)),
        ("V33", "LN(7/0)", E::Err(DivZero)),
    ]);
}

#[test]
fn a_string_with_a_unit_reads_as_that_value() {
    // R44 `vDouble`: "A string may be a number or a number joined to a unit, and then the string's unit applies."
    let v = eval("ABS(\"3 in\")");
    assert_eq!(mismatch(&v, &E::U(3.0, Inch)), None, "{v:?}");
}

#[test]
fn a_formula_that_is_not_a_literal_takes_a_leading_equals_sign_off_in_the_parser_not_here() {
    // Typed `=1+2` and file text `1+2` are one parse; the evaluator sees the same tree.
    let a = eval("=1+2");
    let b = eval("1+2");
    assert_eq!(a, b);
}

#[test]
fn an_unknown_function_is_unsupported_and_a_declared_one_is_too() {
    // No value, so the saved one stands. The `FunctionDef` names are not in the model yet, so they are unknown names here too.
    let v = eval("NOT_A_REAL_NAME1(1,2)");
    assert_eq!(mismatch(&v, &E::Unsupported), None);
    let v = eval("_SHAPEMIN(1)");
    assert_eq!(mismatch(&v, &E::Unsupported), None);
    let v = eval("TODAY()");
    assert_eq!(mismatch(&v, &E::Unsupported), None, "date-time: TODAY is an unknown name");
}

#[test]
fn function_names_match_in_any_case() {
    for f in ["abs(-1)", "ABS(-1)", "Abs(-1)"] {
        let v = eval(f);
        assert_eq!(mismatch(&v, &E::N(1.0)), None, "{f}");
    }
}

#[test]
fn unsupported_poisons_what_is_built_on_it() {
    let v = eval("1+FOO(2)");
    assert_eq!(mismatch(&v, &E::Unsupported), None);
    // But a branch that is not taken is never looked at.
    let v = eval("IF(1,5,FOO(2))");
    assert_eq!(mismatch(&v, &E::N(5.0)), None);
}
