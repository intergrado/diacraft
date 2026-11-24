//! A cell's value, `V`.
//!
//! **One source of truth.** The file's text is the value. A [`Value`] holds that text and nothing else, behind an `Arc`, and
//! every reading of it ([`Value::reading`], [`Value::number`]) is derived from the text on the call. So the typed value and
//! the text cannot disagree: there is no second field to fall behind. A number made by a calculation enters through
//! [`Value::from_number`], which writes it as text that reads back as exactly that number, and [`Value::with_number`] keeps
//! the file's own text when an evaluation computes the number it already holds (`1.50` stays
//! `1.50`).

use std::sync::Arc;

use diacraft_units::{Measure, parse_unit};

/// The text of a `V` attribute, as the file gave it. See the [module documentation](self).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Value(Arc<str>);

/// What a value's text reads as.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Reading {
    /// `V=""`, which is common in files.
    Empty,
    /// A finite number.
    Number(f64),
    /// `1.#INF`, which a file writes for a number too large for a double, or `-1.#INF`.
    Infinite { negative: bool },
    /// The theme marker, `Themed` in any case: the value comes from theme inheritance.
    Themed,
    /// Anything else: a colour (`#FF0000`), a string, a date text, a truth word.
    Text,
}

impl Value {
    /// A value holding `text` exactly.
    pub fn from_text(text: &str) -> Value {
        Value(Arc::from(text))
    }

    /// A value made by calculation: a number written so that it reads back as that number. `1.#INF` and `-1.#INF` for the
    /// infinities, `None` for NaN, which no value can be.
    pub fn from_number(n: f64) -> Option<Value> {
        if n.is_nan() {
            return None;
        }
        if n.is_infinite() {
            return Some(Value::from_text(if n > 0.0 { "1.#INF" } else { "-1.#INF" }));
        }
        // Rust prints the shortest text that reads back as the same number. Plain digits suit ordinary sizes; the
        // extremes would run to hundreds of digits, so they take the exponent form, which `number` reads too.
        let magnitude = n.abs();
        let text = if magnitude >= 1e16 || (magnitude != 0.0 && magnitude < 1e-6) { format!("{n:e}") } else { format!("{n}") };
        Some(Value::from_text(&text))
    }

    /// The text, exactly as given.
    pub fn text(&self) -> &str {
        &self.0
    }

    /// The text behind the pointer it is shared by: a copy of a cell shares it, and a count of the memory a model holds counts it once (`Arc::as_ptr`).
    pub fn shared_text(&self) -> &Arc<str> {
        &self.0
    }

    /// What the text reads as.
    pub fn reading(&self) -> Reading {
        let text = self.text();
        if text.is_empty() {
            Reading::Empty
        } else if text == "1.#INF" {
            Reading::Infinite { negative: false }
        } else if text == "-1.#INF" {
            Reading::Infinite { negative: true }
        } else if text.eq_ignore_ascii_case("themed") {
            Reading::Themed
        } else if let Some(n) = parse_number(text) {
            Reading::Number(n)
        } else {
            Reading::Text
        }
    }

    /// The number the text reads as, if it is one (finite).
    pub fn number(&self) -> Option<f64> {
        parse_number(self.text())
    }

    /// Whether the value comes from the theme (`Themed`, in any case).
    pub fn is_themed(&self) -> bool {
        self.reading() == Reading::Themed
    }

    /// The value after an evaluation gave `n`: this very value, text and all, when it already reads as `n` (a zero keeps its sign), and
    /// the number otherwise. A NaN changes nothing, and neither does an evaluation of a value that comes from the theme: `Themed` stays,
    /// in its own spelling, because the evaluation reads the theme in memory and the file goes on saying where the value comes from.
    pub fn with_number(&self, n: f64) -> Value {
        if n.is_nan() || self.is_themed() {
            return self.clone();
        }
        if self.number().is_some_and(|old| old == n && old.is_sign_negative() == n.is_sign_negative()) {
            return self.clone();
        }
        Value::from_number(n).unwrap_or_else(|| self.clone())
    }
}

/// A finite number written with digits, a sign, a point and an exponent, and nothing else: not `inf`, `nan`, `0x10`, `1_000`,
/// or text with blanks around it, which Rust's own reading would take or refuse in ways a file's value must not depend on.
fn parse_number(text: &str) -> Option<f64> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'+' | b'-' | b'e' | b'E')) {
        return None;
    }
    text.parse::<f64>().ok().filter(|n| n.is_finite())
}

/// The kind of value a cell holds, which decides what it is when nothing gives it a value: by the cell's
/// type, and R20 by its unit when the cell is there with no `V`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ValueKind {
    /// A plain number, a truth value or an enumeration: 0.
    Scalar,
    /// A length (`vLength`) or a type size: 0 inches.
    Length,
    /// An angle (`vAngle`): 0 radians.
    Angle,
    /// A date or any value (`vAny`): 0 days. Durations are in days too.
    Days,
    /// A colour: black.
    Color,
    /// A string or a format string: the empty string.
    Text,
    /// A font: the string `"0"`.
    Font,
    /// A point: `PNT(0, 0)`, from the unit `PNT` of R20.
    Point,
}

impl ValueKind {
    /// What a cell with unit `code` and no `V` is: 0 inches for a length or type unit and for `AC`
    /// and `HA`; 0 radians for an angle; 0 days for a duration; 0 for `BOOL`, `CY`, `DATE` and `PER`; black for `COLOR`;
    /// `PNT(0, 0)`; the empty string for `STR`. `None` for no unit and for a unit the spec does not list (`NURBS`,
    /// `POLYLINE`, `GUID`, `NUM`, `MULTIDIM`, anything a file invents): then R19 applies, by the cell's own type.
    pub fn from_unit(code: &str) -> Option<ValueKind> {
        let is = |word: &str| code.trim().eq_ignore_ascii_case(word);
        if ["BOOL", "CY", "DATE", "PER"].iter().any(|w| is(w)) {
            return Some(ValueKind::Scalar);
        }
        if is("COLOR") {
            return Some(ValueKind::Color);
        }
        if is("PNT") {
            return Some(ValueKind::Point);
        }
        if is("STR") {
            return Some(ValueKind::Text);
        }
        match parse_unit(code)?.measure() {
            Measure::Length | Measure::Area => Some(ValueKind::Length),
            Measure::Angle => Some(ValueKind::Angle),
            Measure::Duration => Some(ValueKind::Days),
            Measure::None | Measure::Percent | Measure::Date => None,
        }
    }
}
