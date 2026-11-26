//! A value as the sheet shows it: the number in the unit of its cell, and the texts of the two views of the window.
//!
//! * **Values view**: a number with **four decimals, trailing zeros kept**, in the unit of the cell (`44.5000 mm`, `0.0000 deg`, `0.0000%`); a number with no unit that
//!   is a whole number is shown whole (an index or a code, `1`), and one that is not has its four decimals; a true-false cell reads `TRUE` or `FALSE`; text is shown without
//!   its quotation marks; a colour reads `RGB(0, 0, 0)`.
//! * **Formulas view**: a plain value shows with **at most four decimals and the trailing zeros dropped** (`2.5 mm`, `0.6667`); text is in quotation marks.
//!
//! **The unit.** A value is held in the internal unit of its measure (inches, radians, days) and the cell's `U` says the unit it shows in; a cell
//! with no `U` shows a length in inches and an angle in degrees, the implicit unit of its kind. The drawing's own unit (the page's measurement choice) is not read
//! here: a window that shows it converts. A number whose unit the model does not know (`NURBS`, `GUID`) is shown as the number it is.
//!
//! **Limits of the reading.** Which cells hold an index or a code (a fill pattern) is not in the catalogue, which gives them all the kind `scalar`: a whole number with no unit
//! is shown whole, so a real number that happens to be whole (`Transparency` 0) is shown `0` and not `0.0000`.

use diacraft_calc::Value as Calc;
use diacraft_doc::{Reading, Value as Held, ValueKind};
use diacraft_units::{Measure, Quantity, Unit, parse_unit};
use serde_json::{Number, Value};

/// A value of a cell, typed: what a program reads in the `value` of a cell.
#[derive(Clone, Debug, PartialEq)]
pub enum CellValue {
    /// A number in the unit of the cell.
    Number(f64),
    Text(String),
    Bool(bool),
    /// No value: an empty cell, an error, an infinity, a point.
    None,
}

impl CellValue {
    /// The JSON of the value: a number that is not finite is `null`.
    pub fn json(&self) -> Value {
        match self {
            CellValue::Number(n) => Number::from_f64(*n).map_or(Value::Null, Value::Number),
            CellValue::Text(t) => Value::String(t.clone()),
            CellValue::Bool(b) => Value::Bool(*b),
            CellValue::None => Value::Null,
        }
    }
}

/// A value, with the unit it is in and the two texts the window shows.
#[derive(Clone, Debug, PartialEq)]
pub struct Typed {
    pub value: CellValue,
    /// The `U` code of the unit the number is in, `None` for a plain number and for text.
    pub unit: Option<&'static str>,
    /// The Values view.
    pub shown: String,
    /// The Formulas view of a value with no formula.
    pub literal: String,
}

impl Typed {
    fn none() -> Typed {
        Typed { value: CellValue::None, unit: None, shown: String::new(), literal: String::new() }
    }

    fn text(t: &str) -> Typed {
        Typed { value: CellValue::Text(t.to_owned()), unit: None, shown: t.to_owned(), literal: quoted(t) }
    }

    fn boolean(b: bool) -> Typed {
        let word = if b { "TRUE" } else { "FALSE" };
        Typed { value: CellValue::Bool(b), unit: None, shown: word.to_owned(), literal: word.to_owned() }
    }

    fn colour(hex: &str) -> Typed {
        let shown = diacraft_calc::Rgb::from_hex(hex).map_or_else(|| hex.to_owned(), |c| format!("RGB({}, {}, {})", c.r, c.g, c.b));
        Typed { value: CellValue::Text(hex.to_owned()), unit: None, shown, literal: hex.to_owned() }
    }
}

/// Text in the quotation marks a formula writes it in, a quote inside it doubled.
pub(crate) fn quoted(text: &str) -> String {
    format!("\"{}\"", text.replace('"', "\"\""))
}

/// A number with four decimals, trailing zeros kept; a number of 1e15 or more in the exponent form; no `-0`.
pub(crate) fn fixed4(n: f64) -> String {
    if !n.is_finite() {
        return infinite(n);
    }
    if n.abs() >= 1e15 {
        return format!("{n:e}");
    }
    let text = format!("{n:.4}");
    if text == "-0.0000" { "0.0000".to_owned() } else { text }
}

/// A number with at most four decimals, the trailing zeros dropped.
pub(crate) fn trimmed4(n: f64) -> String {
    let text = fixed4(n);
    if !text.contains('.') {
        return text;
    }
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

fn infinite(n: f64) -> String {
    if n.is_nan() {
        "#NUM!".to_owned()
    } else if n > 0.0 {
        "1.#INF".to_owned()
    } else {
        "-1.#INF".to_owned()
    }
}

/// A number with no unit: whole as a whole number, else with four decimals.
fn plain(n: f64, trim: bool) -> String {
    if n.is_finite() && n.fract() == 0.0 && n.abs() < 1e15 {
        // No decimals and no `-0`: the value is whole and fits.
        return format!("{}", n + 0.0);
    }
    if trim { trimmed4(n) } else { fixed4(n) }
}

/// The unit's suffix as a window spells it: lower case, `%` stuck to the number.
fn suffix(unit: Unit) -> String {
    unit.suffix().unwrap_or_else(|| unit.code()).to_ascii_lowercase()
}

/// A number in a unit as text, in either view.
fn with_unit(shown: f64, unit: Unit, trim: bool) -> String {
    let number = if trim { trimmed4(shown) } else { fixed4(shown) };
    match unit.measure() {
        Measure::Percent => format!("{number}%"),
        Measure::None | Measure::Date => number,
        _ => format!("{number} {}", suffix(unit)),
    }
}

/// A number cleaned of the noise of a conversion between units (twelve significant digits), so that `50 mm` held as inches reads 50 and not 50.00000000000001.
pub(crate) fn tidy(n: f64) -> f64 {
    if !n.is_finite() || n == 0.0 {
        return n;
    }
    format!("{n:.11e}").parse().unwrap_or(n)
}

/// What a value is in a cell, as the file holds it: `v` the text of `V`, `unit` the cell's `U`, and `kind` what the catalogue says the cell is.
pub(crate) fn typed(v: &str, unit: Option<&str>, kind: ValueKind) -> Typed {
    let code = unit.map(|u| u.trim().to_ascii_uppercase());
    let reading = Held::from_text(v).reading();
    match code.as_deref() {
        Some("BOOL") => {
            if let Reading::Number(n) = reading {
                return Typed::boolean(n != 0.0);
            }
        }
        Some("COLOR") => return Typed::colour(v),
        Some("STR") => return Typed::text(v),
        _ => {}
    }
    match (reading, kind) {
        (Reading::Empty, _) => Typed::none(),
        (Reading::Themed, _) => Typed::text("Themed"),
        (Reading::Infinite { negative }, _) => Typed {
            value: CellValue::None,
            unit: None,
            shown: infinite(if negative { f64::NEG_INFINITY } else { f64::INFINITY }),
            literal: infinite(if negative { f64::NEG_INFINITY } else { f64::INFINITY }),
        },
        (_, ValueKind::Color) | (Reading::Text, _) if looks_like_colour(v) => Typed::colour(v),
        (_, ValueKind::Text | ValueKind::Font | ValueKind::Color) => Typed::text(v),
        (Reading::Text, _) => match v.trim().to_ascii_uppercase().as_str() {
            "TRUE" => Typed::boolean(true),
            "FALSE" => Typed::boolean(false),
            _ => Typed::text(v),
        },
        (Reading::Number(n), kind) => number(n, unit.and_then(parse_unit), kind),
    }
}

fn looks_like_colour(v: &str) -> bool {
    diacraft_calc::Rgb::from_hex(v).is_some()
}

/// A stored number, shown in `unit`, or in the implicit unit of its kind.
fn number(stored: f64, unit: Option<Unit>, kind: ValueKind) -> Typed {
    let unit = unit.filter(|u| *u != Unit::Unitless).or(match kind {
        ValueKind::Length => Some(Unit::Inch),
        ValueKind::Angle => Some(Unit::Degree),
        _ => None,
    });
    match unit {
        Some(u) => in_unit(Quantity::new(stored, u)),
        None => Typed { value: CellValue::Number(tidy(stored)), unit: None, shown: plain(stored, false), literal: plain(stored, true) },
    }
}

/// A quantity as the unit it carries shows it. A number of another dimension than its unit's own (`IN^2`) is shown as the number it is.
fn in_unit(q: Quantity) -> Typed {
    let unit = q.unit;
    if unit == Unit::Unitless {
        return Typed { value: CellValue::Number(tidy(q.stored)), unit: None, shown: plain(q.stored, false), literal: plain(q.stored, true) };
    }
    let n = if q.is_multidimensional() { q.stored } else { q.shown() };
    Typed { value: CellValue::Number(tidy(n)), unit: Some(unit.code()), shown: with_unit(n, unit, false), literal: with_unit(n, unit, true) }
}

/// What a formula worked out to, typed. An error is no value: the caller reads it with [`error_of`].
pub(crate) fn typed_calc(v: &Calc) -> Typed {
    match v {
        Calc::Number(q) => in_unit(*q),
        Calc::Currency { amount, currency } => {
            let code = currency.map_or("", |c| c.code);
            let literal = format!("{} {code}", trimmed4(*amount)).trim_end().to_owned();
            Typed { value: CellValue::Number(tidy(*amount)), unit: None, shown: format!("{} {code}", fixed4(*amount)).trim_end().to_owned(), literal }
        }
        Calc::Text(t) => Typed::text(t),
        Calc::Bool(b) => Typed::boolean(*b),
        Calc::Color(c) => Typed::colour(&c.hex()),
        Calc::Point(p) => {
            let text = |trim: bool| format!("PNT({}, {})", with_unit(p.x.shown(), p.x.unit, trim), with_unit(p.y.shown(), p.y.unit, trim));
            Typed { value: CellValue::None, unit: None, shown: text(false), literal: text(true) }
        }
        Calc::Constructor(c) => {
            let word = match c.kind {
                diacraft_calc::ConstructorKind::Nurbs => "NURBS",
                diacraft_calc::ConstructorKind::Polyline => "POLYLINE",
            };
            Typed { value: CellValue::None, unit: None, shown: word.to_owned(), literal: word.to_owned() }
        }
        Calc::Sheet(_) => Typed { value: CellValue::None, unit: None, shown: "(a sheet)".to_owned(), literal: "(a sheet)".to_owned() },
        Calc::Error(e) => Typed { value: CellValue::None, unit: None, shown: e.text().to_owned(), literal: e.text().to_owned() },
        Calc::Unsupported(name) => Typed { value: CellValue::None, unit: None, shown: format!("{name}?"), literal: format!("{name}?") },
    }
}

/// The error a value is, as `E` spells it.
pub(crate) fn error_of(v: &Calc) -> Option<&'static str> {
    v.error().map(|e| e.text())
}

/// What a cell the file does not hold reads as, by its kind.
pub(crate) fn default_of(kind: ValueKind) -> Typed {
    match kind {
        ValueKind::Scalar | ValueKind::Days => number(0.0, None, kind),
        ValueKind::Length | ValueKind::Angle => number(0.0, None, kind),
        ValueKind::Color => Typed::colour("#000000"),
        ValueKind::Text => Typed::text(""),
        ValueKind::Font => Typed::text("0"),
        ValueKind::Point => Typed { value: CellValue::None, unit: None, shown: "PNT(0, 0)".to_owned(), literal: "PNT(0, 0)".to_owned() },
    }
}

/// `text` cut to at most `max` characters, and whether it was.
pub(crate) fn cut(text: &str, max: usize) -> (String, bool) {
    match text.char_indices().nth(max) {
        Some((at, _)) => (text.get(..at).unwrap_or(text).to_owned(), true),
        None => (text.to_owned(), false),
    }
}

/// The word for a kind of value, in an answer.
pub(crate) fn kind_word(kind: ValueKind) -> &'static str {
    match kind {
        ValueKind::Scalar => "scalar",
        ValueKind::Length => "length",
        ValueKind::Angle => "angle",
        ValueKind::Days => "days",
        ValueKind::Color => "color",
        ValueKind::Text => "text",
        ValueKind::Font => "font",
        ValueKind::Point => "point",
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    #[test]
    fn numbers_have_four_decimals_in_one_view_and_at_most_four_in_the_other() {
        assert_eq!((fixed4(2.5), trimmed4(2.5)), ("2.5000".to_owned(), "2.5".to_owned()));
        assert_eq!((fixed4(0.666_666), trimmed4(0.666_666)), ("0.6667".to_owned(), "0.6667".to_owned()));
        assert_eq!((fixed4(-0.000_01), trimmed4(-0.000_01)), ("0.0000".to_owned(), "0".to_owned()), "no negative zero");
        assert_eq!((fixed4(3.0), trimmed4(3.0)), ("3.0000".to_owned(), "3".to_owned()));
        assert_eq!(fixed4(f64::INFINITY), "1.#INF");
        assert_eq!(fixed4(f64::NAN), "#NUM!");
        assert!(fixed4(1e300).contains('e'), "a huge number is not written in 300 digits: {}", fixed4(1e300));
    }

    #[test]
    fn a_value_is_typed_by_its_unit_and_its_kind() {
        let mm = typed(&Unit::Millimetre.to_internal(44.5).to_string(), Some("MM"), ValueKind::Length);
        assert_eq!((mm.shown.as_str(), mm.literal.as_str(), mm.unit), ("44.5000 mm", "44.5 mm", Some("MM")));
        assert_eq!(mm.value, CellValue::Number(44.5));
        let inch = typed("2", None, ValueKind::Length);
        assert_eq!((inch.shown.as_str(), inch.unit), ("2.0000 in", Some("IN")), "the implicit unit of a length is the inch");
        let angle = typed("0", None, ValueKind::Angle);
        assert_eq!((angle.shown.as_str(), angle.unit), ("0.0000 deg", Some("DEG")));
        let percent = typed("0.25", Some("PER"), ValueKind::Scalar);
        assert_eq!((percent.shown.as_str(), percent.value), ("25.0000%", CellValue::Number(25.0)));
        assert_eq!(typed("1", None, ValueKind::Scalar).shown, "1");
        assert_eq!(typed("0.5", None, ValueKind::Scalar).shown, "0.5000");
        assert_eq!(typed("1", Some("BOOL"), ValueKind::Scalar).shown, "TRUE");
        assert_eq!(typed("#FF8000", None, ValueKind::Color).shown, "RGB(255, 128, 0)");
        let text = typed("say \"hi\"", None, ValueKind::Text);
        assert_eq!((text.shown.as_str(), text.literal.as_str()), ("say \"hi\"", "\"say \"\"hi\"\"\""));
        assert_eq!(typed("", None, ValueKind::Scalar).value, CellValue::None);
        assert_eq!(typed("1.#INF", None, ValueKind::Length).value, CellValue::None);
        assert_eq!(typed("Themed", None, ValueKind::Color).shown, "Themed");
        // A unit the model does not know leaves the number alone.
        assert_eq!(typed("4", Some("NURBS"), ValueKind::Scalar).shown, "4");
    }

    #[test]
    fn a_text_is_cut_at_a_character_and_never_inside_one() {
        assert_eq!(cut("héllo", 2), ("hé".to_owned(), true));
        assert_eq!(cut("héllo", 5), ("héllo".to_owned(), false));
        assert_eq!(cut("", 0), (String::new(), false));
        assert_eq!(cut("ab", 0), (String::new(), true));
    }

    #[test]
    fn a_number_is_tidied_of_the_noise_of_a_unit_conversion() {
        assert_eq!(tidy(50.000_000_000_000_01), 50.0);
        assert_eq!(tidy(0.0), 0.0);
        assert_eq!(tidy(1.0 / 3.0), 0.333_333_333_333);
        assert!(tidy(f64::NAN).is_nan());
    }
}
