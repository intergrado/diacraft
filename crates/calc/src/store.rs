//! A value as a cell holds it, and back: `V` and `U` as text, and the typed [`Value`] they read as.
//!
//! What a file holds in `V` for each kind is.
//! A reading is total: any text reads as something, and a text
//! that is not what its unit says is kept as text.

use diacraft_formula::{Currency, currency_by_code};
use diacraft_units::{Measure, Quantity, Unit, parse_unit};
use jiff::Span;
use jiff::civil::{Date, DateTime, Time};

use crate::strnum::{parse_decimal, read_quantity};
use crate::value::{Constructor, ConstructorKind, Point, Rgb, Value};

/// A value as text: `V`, and `U` when the kind has one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Encoded {
    pub v: String,
    pub u: Option<String>,
}

/// Reads `V` and `U` as a value. `default_unit` is the unit a number with no `U` has in a cell that calls for a measure
/// Never fails: a text that does not read as its unit is a string.
pub fn decode(text: &str, unit: Option<&str>, default_unit: Option<Unit>) -> Value {
    let u = unit.map(str::trim).filter(|u| !u.is_empty());
    let upper = u.map(str::to_ascii_uppercase);
    let infinite = match text {
        "1.#INF" => Some(f64::INFINITY),
        "-1.#INF" => Some(f64::NEG_INFINITY),
        _ => None,
    };
    let number = infinite.or_else(|| parse_decimal(text));
    match upper.as_deref() {
        Some("STR") => Value::text(text),
        Some("BOOL") => match number {
            Some(n) => Value::Bool(n != 0.0),
            None if text.trim().eq_ignore_ascii_case("true") => Value::Bool(true),
            None if text.trim().eq_ignore_ascii_case("false") => Value::Bool(false),
            None => Value::text(text),
        },
        Some("COLOR") => match Rgb::from_hex(text) {
            Some(c) => Value::Color(c),
            None => number.map_or_else(|| Value::text(text), Value::num),
        },
        Some("DATE") => match iso_to_serial(text).or(number) {
            Some(serial) => Value::date(serial),
            None => Value::text(text),
        },
        Some("CY") => match parse_currency(text) {
            Some((amount, currency)) => Value::Currency { amount, currency },
            None => Value::text(text),
        },
        Some("PNT") => parse_point(text).map_or_else(|| Value::text(text), |p| Value::Point(Box::new(p))),
        Some("NURBS") => parse_constructor(text, ConstructorKind::Nurbs).map_or_else(|| Value::text(text), |c| Value::Constructor(Box::new(c))),
        Some("POLYLINE") => parse_constructor(text, ConstructorKind::Polyline).map_or_else(|| Value::text(text), |c| Value::Constructor(Box::new(c))),
        Some("MULTIDIM") => read_quantity(text).map_or_else(|| Value::text(text), Value::Number),
        Some(code) => match parse_unit(code) {
            Some(unit) => match number {
                Some(n) => Value::Number(Quantity::new(n, unit)),
                None => Value::text(text),
            },
            // `GUID`, `NUM` and a unit nobody lists: the text decides.
            None => untyped(text, number, None),
        },
        None => untyped(text, number, default_unit),
    }
}

/// A value with no usable `U`: a number is a number, `#RRGGBB` a colour, anything else a string.
fn untyped(text: &str, number: Option<f64>, default_unit: Option<Unit>) -> Value {
    if let Some(n) = number {
        return match default_unit {
            Some(unit) if unit.measure() != Measure::None => Value::Number(Quantity::new(n, unit)),
            _ => Value::num(n),
        };
    }
    match Rgb::from_hex(text) {
        Some(c) => Value::Color(c),
        None => Value::text(text),
    }
}

/// A value as `V` and `U` hold it. A number is its stored number with its unit's code; a value of another dimension than its unit's
/// is `MULTIDIM` with the number shown in the unit named, to its power (`3FT^2`); a currency is the number at four decimals
/// and the code with no space; a date is ISO 8601 with no zone; a point is `PNT(x,y)`.
pub fn encode(value: &Value) -> Option<Encoded> {
    Some(match value {
        Value::Number(q) => {
            if q.unit == Unit::Date {
                let iso = serial_to_iso(q.stored)?;
                return Some(Encoded { v: iso, u: Some("DATE".to_owned()) });
            }
            if q.is_multidimensional() || (q.dim() != 0 && q.unit == Unit::Unitless) {
                let name = q.unit.suffix().unwrap_or("");
                let shown = if q.unit == Unit::Unitless { q.stored } else { q.shown() };
                return Some(Encoded { v: format!("{}{name}^{}", number_text(shown), q.dim()), u: Some("MULTIDIM".to_owned()) });
            }
            let u = (q.unit != Unit::Unitless).then(|| q.unit.code().to_owned());
            Encoded { v: number_text(q.stored), u }
        }
        Value::Currency { amount, currency } => {
            let code = currency.map_or("", |c| c.code);
            Encoded { v: format!("{amount:.4}{code}"), u: Some("CY".to_owned()) }
        }
        Value::Text(s) => Encoded { v: s.to_string(), u: Some("STR".to_owned()) },
        Value::Bool(b) => Encoded { v: if *b { "1" } else { "0" }.to_owned(), u: Some("BOOL".to_owned()) },
        // A colour cell holds no `U`; a `User` cell that held `COLOR` keeps its own.
        Value::Color(c) => Encoded { v: c.hex(), u: None },
        Value::Point(p) => Encoded { v: format!("PNT({},{})", number_text(p.x.stored), number_text(p.y.stored)), u: Some("PNT".to_owned()) },
        Value::Constructor(c) => {
            let (name, u) = match c.kind {
                ConstructorKind::Nurbs => ("NURBS", "NURBS"),
                ConstructorKind::Polyline => ("POLYLINE", "POLYLINE"),
            };
            let numbers: Vec<String> = c.numbers.iter().map(|n| number_text(*n)).collect();
            Encoded { v: format!("{name}({})", numbers.join(", ")), u: Some(u.to_owned()) }
        }
        Value::Sheet(_) | Value::Error(_) | Value::Unsupported(_) => return None,
    })
}

/// A number as `V` holds it: the shortest text that reads back as the same number, and `1.#INF` past a double. A negative zero is a zero.
pub fn number_text(x: f64) -> String {
    let x = if x == 0.0 { 0.0 } else { x };
    diacraft_doc::Value::from_number(x).map_or_else(|| "0".to_owned(), |v| v.text().to_owned())
}

/// The day the serial 0 is: 30 December 1899.
pub(crate) const EPOCH: Date = Date::constant(1899, 12, 30);

/// `YYYY-MM-DDThh:mm:ss`, with a fraction of a second and a trailing zone letter tolerated, as a serial: whole days from the
/// epoch plus the part of the day gone since midnight.
pub fn iso_to_serial(text: &str) -> Option<f64> {
    let t = text.trim();
    let t = t.strip_suffix('Z').unwrap_or(t);
    let dt = DateTime::strptime("%Y-%m-%dT%H:%M:%S%.f", t).ok().or_else(|| DateTime::strptime("%Y-%m-%dT%H:%M:%S", t).ok())?;
    Some(datetime_to_serial(dt))
}

/// A date and time as a serial.
pub fn datetime_to_serial(dt: DateTime) -> f64 {
    // The difference of two dates is a whole number of 86,400-second days.
    let days = dt.date().duration_since(EPOCH).as_secs() / 86_400;
    let time = dt.time();
    let second_of_day = i32::from(time.hour()) * 3600 + i32::from(time.minute()) * 60 + i32::from(time.second());
    let seconds = f64::from(second_of_day) + f64::from(time.subsec_nanosecond()) / 1e9;
    days as f64 + seconds / 86_400.0
}

/// The date and time a serial is, rounded to the nearest second: the sign and whole part give
/// the day and the size of the fraction gives the time, so -1.25 is 6:00 on 29 December 1899. `None` for a serial jiff cannot hold, one
/// past the end of 9999 or before the start of -9999.
pub fn serial_to_datetime(serial: f64) -> Option<DateTime> {
    if !serial.is_finite() || serial.abs() > 3.0e6 {
        return None;
    }
    let whole = serial.trunc();
    let fraction = (serial - whole).abs();
    let seconds = (fraction * 86_400.0).round() as i64;
    let midnight = EPOCH.to_datetime(Time::midnight());
    // Spans of days and then of seconds, not one duration: `DateTime + SignedDuration` refuses a duration of more days than the Unix epoch's range holds, wherever it starts, and
    // the last day of 9999 is 2,958,465 days from this epoch.
    let days = Span::new().try_days(whole as i64).ok()?;
    midnight.checked_add(days).ok()?.checked_add(Span::new().try_seconds(seconds).ok()?).ok()
}

/// The ISO text of a serial: `YYYY-MM-DDThh:mm:ss`. The year is a minus sign and four digits when it is negative, as the text has always been (jiff's `%Y` has no minus-sign
/// padding).
pub fn serial_to_iso(serial: f64) -> Option<String> {
    let dt = serial_to_datetime(serial)?;
    let rest = jiff::fmt::strtime::format("%m-%dT%H:%M:%S", dt).ok()?;
    Some(format!("{}{:04}-{rest}", if dt.year() < 0 { "-" } else { "" }, dt.year().unsigned_abs()))
}

/// `12.5000EUR`: the number and the code that follows it. A number alone is the undefined currency.
fn parse_currency(text: &str) -> Option<(f64, Option<&'static Currency>)> {
    let t = text.trim();
    if let Some(n) = parse_decimal(t) {
        return Some((n, None));
    }
    let at = t.len().checked_sub(3)?;
    let (number, code) = (t.get(..at)?, t.get(at..)?);
    if !code.bytes().all(|b| b.is_ascii_alphabetic()) {
        return None;
    }
    Some((parse_decimal(number.trim_end())?, currency_by_code(&code.to_ascii_uppercase())))
}

/// `PNT(25.4MM,2)`: two coordinates, each a number with an optional unit suffix, and a number with none in inches.
fn parse_point(text: &str) -> Option<Point> {
    let t = text.trim();
    let inner = t.get(..4).filter(|p| p.eq_ignore_ascii_case("PNT(")).and_then(|_| t.get(4..))?.strip_suffix(')')?;
    let (x, y) = inner.split_once(',')?;
    let coordinate = |s: &str| {
        let q = read_quantity(s)?;
        Some(if q.unit == Unit::Unitless { Quantity::new(q.stored, Unit::DrawingDefault) } else { q })
    };
    Some(Point { x: coordinate(x)?, y: coordinate(y)?, frame: None, mixed: false })
}

/// `NURBS(2, 3, 0)` or `POLYLINE(1, 1, 2.5, 0)`: the numbers, in order.
fn parse_constructor(text: &str, kind: ConstructorKind) -> Option<Constructor> {
    let t = text.trim();
    let name = match kind {
        ConstructorKind::Nurbs => "NURBS(",
        ConstructorKind::Polyline => "POLYLINE(",
    };
    let inner = t.get(..name.len()).filter(|p| p.eq_ignore_ascii_case(name)).and_then(|_| t.get(name.len()..))?.strip_suffix(')')?;
    let mut numbers = Vec::new();
    if !inner.trim().is_empty() {
        for part in inner.split(',') {
            numbers.push(read_quantity(part)?.stored);
        }
    }
    Some(Constructor { kind, numbers })
}
