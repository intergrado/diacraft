//! Every vector of the batch, as data, and the currency and date rows.

use std::sync::Arc;

use diacraft_calc::CellError::{DivZero, Na, Num, Value};
use diacraft_calc::{EvalCtx, EvalOptions, Evaluator, iso_to_serial};
use diacraft_doc::DocProps;
use diacraft_units::Unit::{DurationDefault, ElapsedDay};

use crate::{E, check_table, eval, eval_with, fixed_clock, mismatch, probe};

/// A date as the spec prints it, by its serial.
fn d(iso: &str) -> E {
    E::Date(iso_to_serial(iso).unwrap())
}

fn clocked(formula: &str, now: &str) -> diacraft_calc::Value {
    eval_with(|s| s, formula, EvalOptions { clock: fixed_clock(now, 0), ..EvalOptions::default() })
}

#[test]
fn all_vectors() {
    check_table(&[
        ("ALL", "DATE(7/0,1,1)", E::Err(DivZero)),
        ("ALL", "YEAR(NA())", E::Err(Na)),
        ("ALL", "CY(7/0,\"USD\")", E::Err(DivZero)),
        ("ALL", "DATE(2031,3,7)+1 ed", d("2031-03-08T00:00:00")),
        ("ALL", "DATE(2031,3,8)-DATE(2031,3,7)", E::U(1.0, DurationDefault)),
    ]);
}

#[test]
fn cy() {
    check_table(&[
        ("CY", "CY(12.5,\"EUR\")", E::Cy(12.5, "EUR")),
        ("CY", "CY(12.5,10)", E::Cy(12.5, "EUR")),
        ("CY", "CY(5,\"ZZZ\")", E::Err(Value)),
        ("CY", "CY(5,2)", E::Err(Value)),
        ("CY", "CY(5,205)", E::Err(Value)),
        ("CY", "CY(\"goat\",\"USD\")", E::Err(Value)),
        ("CY", "CY(1E15,\"USD\")", E::Err(Value)),
        ("CY", "CY(922337203685477,\"USD\")", E::Cy(922337203685477.0, "USD")),
        ("CY", "12.5EUR", E::Cy(12.5, "EUR")),
        ("CY", "12.5DEG", E::U(12.5f64.to_radians(), diacraft_units::Unit::Degree)),
        ("CY", "CY(1,126)", E::Cy(1.0, "ERN")),
        ("CY", "ABS(CY(-3,\"EUR\"))", E::N(3.0)),
        ("CY", "CY(5,\"eur\")", E::Cy(5.0, "EUR")),
        ("CY", "CY()", E::Err(Value)),
    ]);
    let v = eval("CY(12.5,\"EUR\")");
    let e = diacraft_calc::encode(&v).unwrap();
    assert_eq!((e.v.as_str(), e.u.as_deref()), ("12.5000EUR", Some("CY")), "Four decimals and the code with no space");
    let e = diacraft_calc::encode(&eval("CY(1,126)")).unwrap();
    assert_eq!(e.v, "1.0000ERN", "CY");
    assert_eq!(diacraft_calc::decode("12.5000EUR", Some("CY"), None), eval("CY(12.5,\"EUR\")"));
}

#[test]
fn currency_rows_of_the_operators() {
    check_table(&[
        ("ADD", "CY(5,\"USD\")+2", E::Cy(7.0, "USD")),
        ("ADD", "2+CY(5,\"USD\")", E::Cy(7.0, "USD")),
        ("ADD", "CY(5,1)+CY(2,\"USD\")", E::Cy(7.0, "USD")),
        ("ADD", "CY(5,\"USD\")+CY(2,1)", E::Cy(7.0, "USD")),
        ("ADD", "CY(5,\"USD\")+CY(2,\"EUR\")", E::Err(Value)),
        ("SUB", "CY(5,\"USD\")-2", E::Cy(3.0, "USD")),
        ("SUB", "CY(5,\"USD\")-CY(2,\"EUR\")", E::Err(Value)),
        ("MUL", "CY(5,\"USD\")*2", E::Cy(10.0, "USD")),
        ("MUL", "CY(5,\"USD\")*CY(2,\"EUR\")", E::Err(Value)),
        ("DIV", "CY(5,\"USD\")/0", E::Err(DivZero)),
        ("DIV", "CY(6,\"USD\")/2", E::Cy(3.0, "USD")),
        ("DIV", "CY(6,\"USD\")/CY(2,\"USD\")", E::Cy(3.0, "USD")),
        ("DIV", "CY(6,\"USD\")/CY(2,\"EUR\")", E::Err(Value)),
        ("ALL", "CY(5,\"USD\")=CY(5,\"EUR\")", E::B(true)),
        ("logic", "NOT(DATE(2000,1,1))", E::B(false)),
    ]);
}

#[test]
fn date_arithmetic_rows_of_the_operators() {
    check_table(&[
        ("ADD", "DATE(2000,1,1)+12 eh", E::Date(36526.5)),
        ("ADD", "12 eh+DATE(2000,1,1)", E::Date(36526.5)),
        ("ADD", "DATE(2000,1,2)+DATE(2000,1,1)", E::Date(73053.0)),
        ("SUB", "DATE(2000,1,2)-DATE(2000,1,1)", E::U(1.0, DurationDefault)),
        ("SUB", "DATE(2000,1,2)-12 eh", E::Date(36526.5)),
        ("SUB", "1 ed-DATE(2000,1,1)", E::U(1.0 - 36526.0, ElapsedDay)),
        ("formula-language V49", "1 ed+12 eh", E::U(1.5, ElapsedDay)),
        ("formula-language V30", "DATE(1899,12,30)", E::Date(0.0)),
    ]);
}

#[test]
fn date() {
    check_table(&[
        ("DATE", "DATE(2031,3,7)", d("2031-03-07T00:00:00")),
        ("DATE", "DATE(2031,3,7)", E::Date(47914.0)),
        ("DATE", "DATE(1999,6,7)", E::Date(36318.0)),
        ("DATE", "DATE(29,5,2)", d("2029-05-02T00:00:00")),
        ("DATE", "DATE(30,5,2)", d("1930-05-02T00:00:00")),
        ("DATE", "DATE(100,5,2)", d("2000-05-02T00:00:00")),
        ("DATE", "DATE(101,5,2)", E::Err(Num)),
        ("DATE", "DATE(2031,0,5)", d("2030-12-05T00:00:00")),
        ("DATE", "DATE(2031,13,5)", d("2032-01-05T00:00:00")),
        ("DATE", "DATE(2031,-1,5)", d("2030-11-05T00:00:00")),
        ("DATE", "DATE(2031,3,0)", d("2031-02-28T00:00:00")),
        ("DATE", "DATE(2031,3,32)", d("2031-04-01T00:00:00")),
        ("DATE", "DATE(2031,3,-1)", d("2031-02-27T00:00:00")),
        ("DATE", "DATE(2028,2,29)", E::Date(46812.0)),
        ("DATE", "DATE(2027,2,29)", d("2027-03-01T00:00:00")),
        ("DATE", "DATE(2200,2,29)", d("2200-03-01T00:00:00")),
        ("DATE", "DATE(1898,12,31)", E::Err(Num)),
        ("DATE", "DATE(1899,1,1)", E::Date(-363.0)),
        ("DATE", "DATE(2031,40000,1)", E::Err(Value)),
        ("DATE", "DATE(1900,6,15)", E::Date(167.0)),
        ("DATE", "DATE(1900,2,10)", E::Date(42.0)),
        ("DATE", "DATE(2031.9,3.9,7.9)", d("2031-03-07T00:00:00")),
        ("DATE", "DATE(1900,0,1)", E::Date(-29.0)),
        ("DATE", "DATE(10000,1,1)", E::Err(Num)),
        ("SER", "DATE(1900,3,1)-DATE(1900,2,28)", E::U(1.0, DurationDefault)),
    ]);
}

#[test]
fn datetime_and_the_strings_it_reads() {
    check_table(&[
        ("DATETIME", "DATETIME(47914.5)", d("2031-03-07T12:00:00")),
        ("DATETIME", "DATETIME(35580.6337)", d("1997-05-30T15:12:32")),
        ("DATETIME", "DATETIME(\"3/7/2031 16:05\")", d("2031-03-07T16:05:00")),
        ("DATETIME", "DATETIME(\"Mar 7, 2031\")", d("2031-03-07T00:00:00")),
        ("DATETIME", "DATETIME(\"4:05 PM March 7, 2031\")", d("2031-03-07T16:05:00")),
        ("DATETIME", "DATETIME(\"2031-03-07T16:05:09\")", d("2031-03-07T16:05:09")),
        ("DATETIME", "DATETIME(\"goat\")", E::Err(Value)),
        ("DATETIME", "DATETIME(\"\")", E::Err(Value)),
        ("DATETIME", "DATETIME(\"3/7/31\")", d("1931-03-07T00:00:00")),
        ("DATETIME", "DATETIME(\"3/7/29\")", d("2029-03-07T00:00:00")),
        ("PARSE", "DATETIME(\"30 May 1997\")", d("1997-05-30T00:00:00")),
        ("PARSE", "DATETIME(\"May 30, 1997 3:45:24 PM\")", d("1997-05-30T15:45:24")),
        ("PARSE", "DATETIME(\"May 1997\")", d("1997-05-01T00:00:00")),
        ("PARSE", "DATETIME(\"2-28/1999\")", d("1999-02-28T00:00:00")),
        ("PARSE", "DATETIME(\"2/30/2031\")", E::Err(Value)),
        ("DATEVALUE", "DATEVALUE(\"3/7/2031 16:05\")", d("2031-03-07T00:00:00")),
        ("DATEVALUE", "DATEVALUE(35580.6337)", d("1997-05-30T00:00:00")),
        ("DATEVALUE", "DATEVALUE(-1.25)", E::Date(-1.0)),
        ("DATEVALUE", "DATEVALUE(\"May 33, 1997\")", E::Err(Value)),
        ("DATEVALUE", "DATEVALUE(\"Feb 30, 2031\")", E::Err(Value)),
    ]);
    // A string with no year reads the clock.
    let v = clocked("DATETIME(\"1:30 PM July 19\")", "2031-01-01T00:00:00");
    assert_eq!(mismatch(&v, &d("2031-07-19T13:30:00")), None, "{v:?}");
    let v = eval("DATETIME(\"1:30 PM July 19\")");
    assert_eq!(mismatch(&v, &E::Unsupported), None, "with no clock the year is unknown: {v:?}");
}

#[test]
fn the_parts_of_a_date() {
    check_table(&[
        ("DAY", "DAY(\"May 30, 1997 15:45:24\")", E::N(30.0)),
        ("DAY", "DAY(47914.99)", E::N(7.0)),
        ("DAY", "DAY(-1.25)", E::N(29.0)),
        ("DAYOFYEAR", "DAYOFYEAR(35580.6337)", E::N(150.0)),
        ("DAYOFYEAR", "DAYOFYEAR(DATE(2028,12,31))", E::N(366.0)),
        ("DAYOFYEAR", "DAYOFYEAR(DATE(2031,12,31))", E::N(365.0)),
        ("DAYOFYEAR", "DAYOFYEAR(\"goat\")", E::Err(Value)),
        ("HOUR", "HOUR(\"May 30, 1997 3:45:24 PM\")", E::N(15.0)),
        ("HOUR", "HOUR(\"5/30/1997\")", E::N(0.0)),
        ("HOUR", "HOUR(0.5)", E::N(12.0)),
        ("HOUR", "HOUR(-1.25)", E::N(6.0)),
        ("MINUTE", "MINUTE(\"7/7/1999 13:45:24\")", E::N(45.0)),
        ("MINUTE", "MINUTE(0.575)", E::N(48.0)),
        ("MINUTE", "MINUTE(\"7:89\")", E::Err(Value)),
        ("MONTH", "MONTH(\"May 30, 1999 13:45:24\")", E::N(5.0)),
        ("MONTH", "MONTH(35580.6337)", E::N(5.0)),
        ("MONTH", "MONTH(\"3/7/2031\")", E::N(3.0)),
        ("SECOND", "SECOND(\"05/30/1997 13:45:32\")", E::N(32.0)),
        ("SECOND", "SECOND(0.6337)", E::N(32.0)),
        ("SECOND", "SECOND(47914)", E::N(0.0)),
        ("WEEKDAY", "WEEKDAY(\"May 30, 1999\")", E::N(7.0)),
        ("WEEKDAY", "WEEKDAY(35880.6337)", E::N(4.0)),
        ("WEEKDAY", "WEEKDAY(DATE(2031,3,10))", E::N(1.0)),
        ("YEAR", "YEAR(\"10/27/2007 13:45:24\")", E::N(2007.0)),
        ("YEAR", "YEAR(35580.6337)", E::N(1997.0)),
        ("YEAR", "YEAR(DATEVALUE(\"Dec 25, 2031\")+7 ed)", E::N(2032.0)),
        ("YEAR", "YEAR(0)", E::N(1899.0)),
        ("YEAR", "YEAR(2)", E::N(1900.0)),
        ("SER", "YEAR(1E9)", E::Err(Value)),
        ("ALL", "DAY(0.99999999)", E::N(31.0)),
    ]);
}

#[test]
fn time_and_timevalue() {
    check_table(&[
        ("TIME", "TIME(15,30,30)", E::Date(0.6461805555555556)),
        ("TIME", "TIME(15,30,30)+8 eh", E::Date(0.6461805555555556 + 8.0 / 24.0)),
        ("TIME", "TIME(25,0,0)", E::Date(1.0 / 24.0)),
        ("TIME", "TIME(1,90,90)", E::Date(9090.0 / 86400.0)),
        ("TIME", "HOUR(TIME(1,2,3.9))", E::N(1.0)),
        ("TIME", "SECOND(TIME(1,2,3.9))", E::N(3.0)),
        ("TIME", "TIME(-1,0,0)", E::Date(-1.0 / 24.0)),
        ("TIMEVALUE", "TIMEVALUE(\"14:30\")+4 eh+30 em", E::Date(19.0 / 24.0)),
        ("TIMEVALUE", "TIMEVALUE(0.6337)", E::Date(0.6337)),
        ("TIMEVALUE", "TIMEVALUE(\"11 AM, July 1, 1997\")", E::Date(11.0 / 24.0)),
        ("TIMEVALUE", "TIMEVALUE(\"7:89\")", E::Err(Value)),
        ("TIMEVALUE", "TIMEVALUE(-1.25)", E::Date(0.25)),
    ]);
}

#[test]
fn now_and_the_document_dates_take_the_clock_and_zone_as_inputs() {
    let v = clocked("NOW()", "2031-03-07T16:05:09");
    assert_eq!(mismatch(&v, &d("2031-03-07T16:05:09")), None, "{v:?}");
    let diacraft_calc::Value::Number(q) = v else { panic!() };
    assert!((q.stored - 47914.6702).abs() < 1e-4);
    let v = eval("NOW()");
    assert_eq!(mismatch(&v, &E::Unsupported), None, "with no clock, NOW has no value: {v:?}");
    // The DOC* vectors: `modified` 20:05:09 UTC, and a zone `offset` from UTC.
    let props = |stamp: &str| DocProps {
        created: Some(Arc::from(stamp)),
        modified: Some(Arc::from(stamp)),
        last_printed: Some(Arc::from(stamp)),
        ..DocProps::default()
    };
    let doc_value = |formula: &str, offset_seconds: i32| {
        let (mut doc, at) = probe(|s| s, formula);
        doc.props = Arc::new(props("2031-03-07T20:05:09Z"));
        let options = EvalOptions { clock: fixed_clock("2031-03-07T16:05:09", offset_seconds), ..EvalOptions::default() };
        let mut ctx = EvalCtx::new(options, Default::default());
        Evaluator::new(&doc, &mut ctx).evaluate(&at)
    };
    for (formula, offset, want) in [
        ("DOCLASTSAVE()", 0, "2031-03-07T20:05:09"),
        ("DOCLASTSAVE()", -4 * 3600, "2031-03-07T16:05:09"),
        ("DOCCREATION()", 0, "2031-03-07T20:05:09"),
        ("DOCCREATION()", 5 * 3600, "2031-03-08T01:05:09"),
        ("DOCLASTEDIT()", 0, "2031-03-07T20:05:09"),
        ("DOCLASTPRINT()", 0, "2031-03-07T20:05:09"),
        ("DOCLASTSAVE()", 5 * 3600 + 1800, "2031-03-08T01:35:09"),
    ] {
        let v = doc_value(formula, offset);
        assert_eq!(mismatch(&v, &d(want)), None, "{formula} at {offset}: {v:?}");
    }
    // With no `modified` property there is no date to give.
    let v = eval("DOCLASTSAVE()");
    assert_eq!(mismatch(&v, &E::Unsupported), None, "{v:?}");
}

#[test]
fn a_date_is_written_as_iso_text_with_whole_seconds() {
    let e = diacraft_calc::encode(&eval("DATE(2031,3,7)")).unwrap();
    assert_eq!((e.v.as_str(), e.u.as_deref()), ("2031-03-07T00:00:00", Some("DATE")));
    let e = diacraft_calc::encode(&eval("DATETIME(35580.6337)")).unwrap();
    assert_eq!(e.v, "1997-05-30T15:12:32", "rounded to the nearest second, not cut");
    assert_eq!(diacraft_calc::decode("2031-03-07T16:05:09", Some("DATE"), None), eval("DATETIME(\"2031-03-07T16:05:09\")"));
}

/// Chrono to jiff: the text of a serial at the ends of the calendar, pinned before the change (what chrono wrote) and passing after it. A negative year is a minus
/// sign and four digits, and the last day of 9999 is whole.
#[test]
fn the_text_of_a_serial_at_the_ends_of_the_calendar_is_what_it_was() {
    for (serial, want) in [
        (-1_000_000.0, "-0838-02-02T00:00:00"),
        (-693_959.0, "0000-01-01T00:00:00"),
        (-693_960.0, "-0001-12-31T00:00:00"),
        (-693_961.5, "-0001-12-30T12:00:00"),
        (-2_000_000.0, "-3576-03-07T00:00:00"),
        (-693_594.0, "0000-12-31T00:00:00"),
        (-1_000_000.25, "-0838-02-02T06:00:00"),
        (2_958_465.0, "9999-12-31T00:00:00"),
        (2_958_465.999_99, "9999-12-31T23:59:59"),
    ] {
        assert_eq!(diacraft_calc::serial_to_iso(serial).as_deref(), Some(want), "serial {serial}");
    }
    check_table(&[
        ("", "YEAR(2958465)", E::N(9999.0)),
        ("", "MONTH(2958465)", E::N(12.0)),
        ("", "DAY(2958465)", E::N(31.0)),
        ("", "HOUR(2958465.99999)", E::N(23.0)),
        ("", "MINUTE(2958465.99999)", E::N(59.0)),
        ("", "SECOND(2958465.99999)", E::N(59.0)),
        ("", "WEEKDAY(2958465)", E::N(5.0)),
        ("", "DAYOFYEAR(2958465)", E::N(365.0)),
    ]);
}

/// The first day past the end of 9999 is no date a part can give (the project rule: `#VALUE!` and no panic at any value). chrono held years to 262,143, and a serial
/// up to 3,000,000 (the year 10113) had a year; jiff holds 9999, and the spec's rule is what is left.
#[test]
fn a_serial_past_the_end_of_9999_gives_no_part_and_no_text() {
    check_table(&[
        ("SER", "YEAR(2958466)", E::Err(Value)),
        ("SER", "DAY(3000000)", E::Err(Value)),
        ("SER", "HOUR(2999999.5)", E::Err(Value)),
        ("SER", "WEEKDAY(2958466)", E::Err(Value)),
    ]);
    assert_eq!(diacraft_calc::serial_to_iso(2_958_466.0), None);
    assert_eq!(diacraft_calc::serial_to_iso(3_000_000.0), None);
}

/// A year past 9999 is no year. chrono added the offsets of `DATE` to a first of the month in a year it held, so `DATE(10088,-827,-11476)` was a day of 9987;
/// the year argument is checked on its own now, and a string with such a year is not a date.
#[test]
fn a_year_past_9999_is_no_date_in_an_argument_or_a_string() {
    check_table(&[
        ("DATE", "DATE(10088,-827,-11476)", E::Err(Num)),
        ("DATE", "DATE(10173,-1681,-19377)", E::Err(Num)),
        ("PARSE", "DATEVALUE(\"1/1/10000\")", E::Err(Value)),
        ("PARSE", "DATETIME(\"12/31/10000 1:00 PM\")", E::Err(Value)),
        ("PARSE", "DATEVALUE(\"12/31/9999\")", d("9999-12-31T00:00:00")),
    ]);
}

/// A 60th second (a leap second, in a time a person or another program wrote) reads as the last second of its minute: jiff has no leap seconds, as Temporal has none.
/// chrono kept it and counted it, so 23:59:60 was the next midnight.
#[test]
fn a_leap_second_reads_as_the_last_second_of_the_minute() {
    assert_eq!(iso_to_serial("2026-03-07T23:59:60"), iso_to_serial("2026-03-07T23:59:59"));
    assert_eq!(iso_to_serial("2026-03-07T12:00:60.5"), iso_to_serial("2026-03-07T12:00:59.5"));
    assert!(iso_to_serial("2026-03-07T12:00:61").is_none(), "61 is no second");
}
