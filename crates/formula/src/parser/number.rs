//! Number literals: a number, then what may follow it directly: a unit, a power of the unit, the compound forms
//! The currency code (one of the 192, with a blank before it or none), and the forms of the reference
//! pages.
//!
//! Every form after the number is read by looking ahead and, when it is not there, putting the position back: `5 GT 2` is a
//! number and then an operator, and `5 IN` is one literal. A run of letters is taken whole, so the longest match wins:
//! `1EM` is a number with the unit `EM`, and `1E5` is a number with an exponent.

use std::sync::Arc;

use diacraft_units::{Measure, Unit, parse_unit};

use super::{Node, Parser, Res};
use crate::{Expr, ImplicitMeasure, Number, ParseErrorKind, currency_by_code};

/// The longest text of one number, in bytes: far more than the 309 digits a double can hold.
const MAX_NUMBER_TEXT: usize = 1024;
/// The most digits of a power written after a unit, which fits an `i32`. The limit of 127 is the evaluator's.
const MAX_POWER_DIGITS: usize = 9;
/// The most digits in the numerator or the denominator of a mixed number: both are exact in a double.
const MAX_FRACTION_DIGITS: usize = 15;

/// The unit a suffix names in a formula: any spelling `parse_unit` knows but the `U` values that are not suffixes (the percent
/// is the postfix operator, and `AD` and the like are not written after a number). `DATE` is read only with a power (`lettered`).
fn literal_unit(text: &str) -> Option<Unit> {
    let unit = parse_unit(text)?;
    (unit.suffix().is_some() && unit != Unit::Percent).then_some(unit)
}

/// The index after the ASCII digits that start at `from`.
fn digits(bytes: &[u8], from: usize) -> usize {
    from + bytes.get(from..).map_or(0, |b| b.iter().take_while(|c| c.is_ascii_digit()).count())
}

impl<'a> Parser<'a> {
    /// A number literal, with whatever unit follows it. The next character is a digit, or a point and a digit.
    pub(super) fn number(&mut self) -> Res<Node> {
        let (mut value, whole) = self.scan_number()?;
        if whole && let Some(mixed) = self.mixed_fraction(value) {
            value = mixed;
        }
        let expr = self.with_suffix(value)?;
        self.leaf(expr)
    }

    /// The number at the position, by R16: digits, a point and digits (`5.` and `.5` too), and an exponent. Also whether it was
    /// only digits.
    fn scan_number(&mut self) -> Res<(f64, bool)> {
        let rest = self.rest();
        let bytes = rest.as_bytes();
        let mut end = digits(bytes, 0);
        let mut whole = true;
        if bytes.get(end) == Some(&b'.') {
            end = digits(bytes, end + 1);
            whole = false;
        }
        if matches!(bytes.get(end), Some(b'e' | b'E')) {
            // An exponent has digits; without them the letter is not part of the number (`1EM` is a unit).
            let sign = usize::from(matches!(bytes.get(end + 1), Some(b'+' | b'-')));
            let exponent_end = digits(bytes, end + 1 + sign);
            if exponent_end > end + 1 + sign {
                end = exponent_end;
                whole = false;
            }
        }
        if end > MAX_NUMBER_TEXT {
            return self.fail(ParseErrorKind::NumberOutOfRange);
        }
        let Some(value) = rest.get(..end).and_then(|t| t.parse::<f64>().ok()) else { return self.fail(ParseErrorKind::BadNumber) };
        if !value.is_finite() {
            return self.fail(ParseErrorKind::NumberOutOfRange);
        }
        self.pos += end;
        Ok((value, whole))
    }

    /// Digits, and a point and digits: the number of a compound literal's second part. No sign, no exponent.
    fn plain_number(&mut self) -> Option<f64> {
        let rest = self.rest();
        let bytes = rest.as_bytes();
        let mut end = digits(bytes, 0);
        if end == 0 {
            return None;
        }
        if bytes.get(end) == Some(&b'.') {
            end = digits(bytes, end + 1);
        }
        if end > MAX_NUMBER_TEXT {
            return None;
        }
        let value = rest.get(..end)?.parse::<f64>().ok().filter(|v| v.is_finite())?;
        self.pos += end;
        Some(value)
    }

    /// `5 1/2`: a whole number, blanks, and a fraction of whole numbers with no blanks inside. `1/2.5` and a zero
    /// denominator are not one.
    fn mixed_fraction(&mut self, whole: f64) -> Option<f64> {
        let rest = self.rest();
        let after = rest.trim_start_matches([' ', '\t']);
        let blanks = rest.len() - after.len();
        if blanks == 0 {
            return None;
        }
        let bytes = after.as_bytes();
        let numerator_end = digits(bytes, 0);
        if numerator_end == 0 || bytes.get(numerator_end) != Some(&b'/') {
            return None;
        }
        let denominator_end = digits(bytes, numerator_end + 1);
        if denominator_end == numerator_end + 1 {
            return None;
        }
        if bytes.get(denominator_end) == Some(&b'.') && bytes.get(denominator_end + 1).is_some_and(u8::is_ascii_digit) {
            return None;
        }
        if numerator_end > MAX_FRACTION_DIGITS || denominator_end - numerator_end - 1 > MAX_FRACTION_DIGITS {
            return None;
        }
        let numerator = after.get(..numerator_end)?.parse::<f64>().ok()?;
        let denominator = after.get(numerator_end + 1..denominator_end)?.parse::<f64>().ok()?;
        if denominator == 0.0 {
            return None;
        }
        self.pos += blanks + denominator_end;
        Some(whole + numerator / denominator)
    }

    /// What follows the number `value`: a unit, a currency code, a compound or an implicit form; or nothing, and the number
    /// stands alone.
    fn with_suffix(&mut self, value: f64) -> Res<Expr> {
        let after_number = self.pos;
        self.skip_ws();
        let found = match self.peek() {
            Some('\u{b0}') => self.degrees(value),
            Some('[') => self.implicit(value),
            Some(c) if c.is_ascii_alphabetic() => self.lettered(value)?,
            _ => None,
        };
        Ok(found.unwrap_or_else(|| {
            self.pos = after_number;
            Expr::Number(Number::plain(value))
        }))
    }

    /// The run of ASCII letters at the position, taken whole.
    fn take_letters(&mut self) -> &'a str {
        let rest = self.rest();
        let len = rest.bytes().take_while(u8::is_ascii_alphabetic).count();
        self.pos += len;
        rest.get(..len).unwrap_or("")
    }

    /// A number followed by letters: `SQ. IN.`, `2C4`, `3P6`, a unit, or a currency code. `None` if the letters are none of those.
    fn lettered(&mut self, value: f64) -> Res<Option<Expr>> {
        let run = self.take_letters();
        if run.eq_ignore_ascii_case("SQ") {
            return Ok(self.square(value));
        }
        if (run.eq_ignore_ascii_case("C") || run.eq_ignore_ascii_case("P")) && self.peek().is_some_and(|c| c.is_ascii_digit()) {
            return Ok(self.second_part(value, run.eq_ignore_ascii_case("C")));
        }
        if let Some(unit) = self.unit_named(run) {
            return self.with_unit(value, unit);
        }
        // R26 lists `DATE` among the bases of a power, and R21 gives a date no literal of its own: so it is read after a number only
        // with a power straight after it (`3 DATE^2`).
        if parse_unit(run) == Some(Unit::Date)
            && let Some(power) = self.unit_power()?
        {
            return Ok(Some(Expr::Number(Number::Measured { value, unit: Unit::Date, power: Some(power) })));
        }
        // A number and a code of the 192 of after it, with a blank or none (`12.5USD`, `5 USD`, as a
        // unit takes one). The letters are taken whole and in capitals (a literal does not ignore case), so `12.5EURO` and
        // `12.5eur` are not one. No code is a unit (checked by a test), so a unit read first never hides a code. Any other three
        // letters are no literal, and the number stands alone with a name after it, which is no operator.
        if let Some(entry) = currency_by_code(run) {
            return Ok(Some(Expr::Currency { value, code: Arc::from(entry.code) }));
        }
        Ok(None)
    }

    /// The unit the letters name, with a dot where the spelling has one (`HA.`, `EW.`).
    fn unit_named(&mut self, run: &str) -> Option<Unit> {
        if self.peek() == Some('.')
            && let Some(unit) = literal_unit(&format!("{run}."))
        {
            self.pos += 1;
            return Some(unit);
        }
        literal_unit(run)
    }

    /// A number with the unit `unit`: feet and inches (`8FT 4IN`), or a power (`3 IN^2`; not for an area), or nothing more.
    fn with_unit(&mut self, value: f64, unit: Unit) -> Res<Option<Expr>> {
        if unit == Unit::Foot
            && let Some(inches) = self.inches_after_feet()
        {
            return Ok(Some(Expr::Number(Number::FeetInches { feet: value, inches })));
        }
        // The list of bases leaves out `ACRE` and `HA`, which are areas already: after one, `^` is the operator.
        if unit.measure() == Measure::Area {
            return Ok(Some(Expr::Number(Number::with_unit(value, unit))));
        }
        let before_dot = self.pos;
        // `IN.^2`: a dot after a length unit that a power follows (the shapes are for length units).
        if unit.measure() == Measure::Length && self.peek() == Some('.') && self.peek2() == Some('^') {
            self.pos += 1;
        }
        let power = self.unit_power()?;
        if power.is_none() {
            self.pos = before_dot;
        }
        Ok(Some(Expr::Number(Number::Measured { value, unit, power })))
    }

    /// A whole power straight after a unit (`^2`, `^-1`): it belongs to the unit. A power that is not a whole number
    /// (`^1.5`), or has a blank before it or after the `^`, or is not a number at all, is the operator `^`.
    fn unit_power(&mut self) -> Res<Option<i32>> {
        let rest = self.rest();
        let bytes = rest.as_bytes();
        if bytes.first() != Some(&b'^') {
            return Ok(None);
        }
        let sign = bytes.get(1).copied();
        // E11 leaves a sign `unknown`; it belongs to the power, as the negative powers need.
        let start = if matches!(sign, Some(b'+' | b'-')) { 2 } else { 1 };
        let end = digits(bytes, start);
        if end == start || (bytes.get(end) == Some(&b'.') && bytes.get(end + 1).is_some_and(u8::is_ascii_digit)) {
            return Ok(None);
        }
        if end - start > MAX_POWER_DIGITS {
            return self.fail(ParseErrorKind::NumberOutOfRange);
        }
        let Some(magnitude) = rest.get(start..end).and_then(|t| t.parse::<i32>().ok()) else { return self.fail(ParseErrorKind::BadNumber) };
        self.pos += end;
        Ok(Some(if sign == Some(b'-') { -magnitude } else { magnitude }))
    }

    /// `8FT 4IN`: after feet, a number and `IN`. Puts the position back and says `None` if it is not there.
    fn inches_after_feet(&mut self) -> Option<f64> {
        let before = self.pos;
        self.skip_ws();
        if let Some(inches) = self.plain_number() {
            self.skip_ws();
            if self.take_letters().eq_ignore_ascii_case("IN") {
                return Some(inches);
            }
        }
        self.pos = before;
        None
    }

    /// `2C4` and `3P6`: after the letter, the second number, with no blank.
    fn second_part(&mut self, first: f64, ciceros: bool) -> Option<Expr> {
        let second = self.plain_number()?;
        Some(Expr::Number(if ciceros {
            Number::CiceroDidot { ciceros: first, didots: second }
        } else {
            Number::PicaPoints { picas: first, points: second }
        }))
    }

    /// `SQ. IN.` and `SQ IN` after a number: a length unit squared. An angle, a duration or an area is no length.
    fn square(&mut self, value: f64) -> Option<Expr> {
        self.eat('.');
        self.skip_ws();
        let unit = literal_unit(self.take_letters()).filter(|u| u.measure() == Measure::Length)?;
        self.eat('.');
        Some(Expr::Number(Number::Measured { value, unit, power: Some(2) }))
    }

    /// `10°`, `10°15'`, `10°15'30"`. The position is at the degree sign.
    fn degrees(&mut self, degrees: f64) -> Option<Expr> {
        self.pos += '\u{b0}'.len_utf8();
        let minutes = self.marked('\'');
        let seconds = self.marked('"');
        Some(Expr::Number(Number::DegreesMinutesSeconds { degrees, minutes: minutes.unwrap_or(0.0), seconds: seconds.unwrap_or(0.0) }))
    }

    /// A plain number and then `mark`; puts the position back if either is missing.
    fn marked(&mut self, mark: char) -> Option<f64> {
        let before = self.pos;
        if let Some(value) = self.plain_number()
            && self.eat(mark)
        {
            return Some(value);
        }
        self.pos = before;
        None
    }

    /// `3[cm,D]`: a unit and a flag for the measure whose default applies. The position is at the `[`.
    fn implicit(&mut self, value: f64) -> Option<Expr> {
        self.pos += 1;
        self.skip_ws();
        let unit = literal_unit(self.take_letters())?;
        self.skip_ws();
        if !self.eat(',') {
            return None;
        }
        self.skip_ws();
        let measure = ImplicitMeasure::from_flag(self.peek()?)?;
        self.pos += 1;
        self.skip_ws();
        if !self.eat(']') {
            return None;
        }
        Some(Expr::Number(Number::Implicit { value, unit, measure }))
    }
}
