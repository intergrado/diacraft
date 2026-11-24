//! What a number field of a pane takes and shows: lengths with units, angles, and the arithmetic between them ("Units and arithmetic
//! in Size & Position").
//!
//! A field **accepts units** (`21 in.` is 21 inches, `1 ft. 5 in.` is 17 inches, `2 cm`, `3'`, `4"`) and **expressions** with `+`, `-`, `*`, `/` and
//! parentheses (`1 ft + 2 in`, `(3 + 4) * 2`, `10 in / 4`). **A comma between values is an error** ("a comma between measurement values is an
//! error", with the value unchanged), so `1 ft, 5 in` is refused in words. A number with no unit takes the unit the field shows (the page's measurement
//! unit for a length, the degree for an angle).
//!
//! **How an expression is worked out** (ours; the page says only that the four operations work): a value is a number of inches (or degrees) with a
//! dimension, 1 for a length and 0 for a plain number. A sum or a difference of a length and a plain number takes the plain number in the field's unit;
//! a product or a quotient of a length and a plain number is a length; a length times a length, and a plain number divided by a length, are refused; a length
//! over a length is a plain number. A quantity written `1 ft. 5 in.` (a number and a unit, then another number and a unit) is the sum of the two.
//! Everything is held in **inches**; the pane turns that into the drawing's units and back (the page's scale).

use diacraft_units::{Measure, Unit, parse_unit};

/// What a field counts: a length, or an angle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dim {
    Length,
    Angle,
}

impl Dim {
    fn measure(self) -> Measure {
        match self {
            Dim::Length => Measure::Length,
            Dim::Angle => Measure::Angle,
        }
    }

    fn words(self) -> &'static str {
        match self {
            Dim::Length => "a length",
            Dim::Angle => "an angle",
        }
    }
}

/// The longest text a field is worked out from, in characters.
pub const MAX_CHARS: usize = 120;

/// The deepest parentheses nest.
const MAX_DEPTH: usize = 16;

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Number(f64),
    Word(String),
    Plus,
    Minus,
    Star,
    Slash,
    Open,
    Close,
}

/// A value of the arithmetic: a number in the base unit (inches, degrees) and its dimension (1 for a length or an angle, 0 for a plain number).
#[derive(Clone, Copy, Debug)]
struct Value {
    number: f64,
    dimension: u8,
}

fn tokens(text: &str) -> Result<Vec<Token>, String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(c) = chars.get(at).copied() {
        match c {
            c if c.is_whitespace() => at += 1,
            ',' | ';' => return Err("A comma between values is not allowed. Write one value, with its unit or units: 1 ft. 5 in.".to_owned()),
            '+' => {
                out.push(Token::Plus);
                at += 1;
            }
            '-' | '\u{2212}' => {
                out.push(Token::Minus);
                at += 1;
            }
            '*' | '\u{d7}' => {
                out.push(Token::Star);
                at += 1;
            }
            '/' | '\u{f7}' => {
                out.push(Token::Slash);
                at += 1;
            }
            '(' => {
                out.push(Token::Open);
                at += 1;
            }
            ')' => {
                out.push(Token::Close);
                at += 1;
            }
            // The prime and the double prime (and the ASCII marks for them) are the foot and the inch.
            '\'' | '\u{2032}' => {
                out.push(Token::Word("ft".to_owned()));
                at += 1;
            }
            '"' | '\u{2033}' => {
                out.push(Token::Word("in".to_owned()));
                at += 1;
            }
            '\u{b0}' => {
                out.push(Token::Word("deg".to_owned()));
                at += 1;
            }
            c if c.is_ascii_digit() || c == '.' => {
                let start = at;
                while chars.get(at).is_some_and(|d| d.is_ascii_digit() || *d == '.') {
                    at += 1;
                }
                let literal: String = chars.get(start..at).unwrap_or_default().iter().collect();
                let n: f64 = literal.parse().map_err(|_| format!("`{literal}` is not a number."))?;
                if !n.is_finite() {
                    return Err(format!("`{literal}` is not a number."));
                }
                out.push(Token::Number(n));
            }
            c if c.is_alphabetic() => {
                let start = at;
                while chars.get(at).is_some_and(|d| d.is_alphabetic()) {
                    at += 1;
                }
                let mut word: String = chars.get(start..at).unwrap_or_default().iter().collect();
                // A unit may end in a full stop (`ft.`, `in.`): that stop is part of it.
                if chars.get(at) == Some(&'.') {
                    at += 1;
                }
                word.make_ascii_lowercase();
                out.push(Token::Word(word));
            }
            other => return Err(format!("`{other}` is not allowed in a measurement.")),
        }
    }
    Ok(out)
}

/// The unit a word names for `dim`, as a factor to the base unit (inches or degrees).
fn unit_factor(word: &str, dim: Dim) -> Option<f64> {
    let long = match word {
        "inch" | "inches" => Some("in"),
        "foot" | "feet" => Some("ft"),
        "yard" | "yards" => Some("yd"),
        "mile" | "miles" => Some("mi"),
        "millimetre" | "millimetres" | "millimeter" | "millimeters" => Some("mm"),
        "centimetre" | "centimetres" | "centimeter" | "centimeters" => Some("cm"),
        "metre" | "metres" | "meter" | "meters" => Some("m"),
        "kilometre" | "kilometres" | "kilometer" | "kilometers" => Some("km"),
        "point" | "points" => Some("pt"),
        "degree" | "degrees" => Some("deg"),
        "radian" | "radians" => Some("rad"),
        other => Some(other),
    }?;
    let unit = parse_unit(long)?;
    (unit.measure() == dim.measure() && !matches!(unit, Unit::DrawingDefault | Unit::PageDefault | Unit::TypeDefault | Unit::AngleDefault))
        .then(|| unit.to_internal(1.0))
        .filter(|f| f.is_finite() && *f > 0.0)
}

struct Parser<'a> {
    tokens: &'a [Token],
    at: usize,
    dim: Dim,
    /// The factor of the field's own unit to the base unit: what a plain number in a sum means.
    default: f64,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at)
    }

    fn expression(&mut self, depth: usize) -> Result<Value, String> {
        let mut left = self.term(depth)?;
        while let Some(op) = self.peek().cloned() {
            match op {
                Token::Plus | Token::Minus => {
                    self.at += 1;
                    let right = self.term(depth)?;
                    left = self.sum(left, right, op == Token::Minus);
                }
                _ => break,
            }
        }
        Ok(left)
    }

    /// A sum or a difference: a length and a plain number meet in the field's unit.
    fn sum(&self, left: Value, right: Value, minus: bool) -> Value {
        let in_base = |v: Value| if v.dimension == 0 && (left.dimension == 1 || right.dimension == 1) { v.number * self.default } else { v.number };
        let (a, b) = (in_base(left), in_base(right));
        let dimension = left.dimension.max(right.dimension);
        Value { number: if minus { a - b } else { a + b }, dimension }
    }

    fn term(&mut self, depth: usize) -> Result<Value, String> {
        let mut left = self.factor(depth)?;
        while let Some(op) = self.peek().cloned() {
            match op {
                Token::Star | Token::Slash => {
                    self.at += 1;
                    let right = self.factor(depth)?;
                    left = self.product(left, right, op == Token::Slash)?;
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn product(&self, left: Value, right: Value, divide: bool) -> Result<Value, String> {
        if divide {
            if right.number == 0.0 {
                return Err("A value cannot be divided by nothing.".to_owned());
            }
            return match (left.dimension, right.dimension) {
                (l, 0) => Ok(Value { number: left.number / right.number, dimension: l }),
                (1, 1) => Ok(Value { number: left.number / right.number, dimension: 0 }),
                _ => Err(format!("A plain number cannot be divided by {}.", self.dim.words())),
            };
        }
        match (left.dimension, right.dimension) {
            (1, 1) => Err(format!("{} cannot be multiplied by {}.", capital(self.dim.words()), self.dim.words())),
            (l, r) => Ok(Value { number: left.number * right.number, dimension: l.max(r) }),
        }
    }

    fn factor(&mut self, depth: usize) -> Result<Value, String> {
        match self.peek() {
            Some(Token::Minus) => {
                self.at += 1;
                let v = self.factor(depth)?;
                Ok(Value { number: -v.number, ..v })
            }
            Some(Token::Plus) => {
                self.at += 1;
                self.factor(depth)
            }
            _ => self.primary(depth),
        }
    }

    fn primary(&mut self, depth: usize) -> Result<Value, String> {
        match self.peek().cloned() {
            Some(Token::Open) => {
                if depth >= MAX_DEPTH {
                    return Err("The parentheses go too deep.".to_owned());
                }
                self.at += 1;
                let v = self.expression(depth + 1)?;
                match self.peek() {
                    Some(Token::Close) => {
                        self.at += 1;
                        Ok(v)
                    }
                    _ => Err("A parenthesis is not closed.".to_owned()),
                }
            }
            Some(Token::Number(_)) => self.quantity(),
            Some(Token::Word(w)) => Err(format!("`{w}` has no number before it.")),
            Some(Token::Close) => Err("A parenthesis is closed that was not opened.".to_owned()),
            Some(_) | None => Err("A value is missing.".to_owned()),
        }
    }

    /// A number, its unit if it has one, and the numbers and units that follow (`1 ft. 5 in.`), which are added.
    fn quantity(&mut self) -> Result<Value, String> {
        let Some(Token::Number(n)) = self.peek().cloned() else { return Err("A value is missing.".to_owned()) };
        self.at += 1;
        let Some(Token::Word(word)) = self.peek().cloned() else { return Ok(Value { number: n, dimension: 0 }) };
        self.at += 1;
        let factor = unit_factor(&word, self.dim).ok_or_else(|| format!("`{word}` is not a unit of {}.", self.dim.words()))?;
        let mut total = n * factor;
        // More of the same: `5 in.` after `1 ft.`.
        while let (Some(Token::Number(m)), Some(Token::Word(w))) = (self.peek().cloned(), self.tokens.get(self.at + 1).cloned()) {
            let f = unit_factor(&w, self.dim).ok_or_else(|| format!("`{w}` is not a unit of {}.", self.dim.words()))?;
            total += m * f;
            self.at += 2;
        }
        Ok(Value { number: total, dimension: 1 })
    }
}

fn capital(words: &str) -> String {
    let mut chars = words.chars();
    chars.next().map(|c| c.to_uppercase().collect::<String>() + chars.as_str()).unwrap_or_default()
}

/// Works out what `text` says, in the base unit of `dim` (inches or degrees); a plain number is in `default`, the unit the field shows. The error is
/// words for the person.
fn evaluate(text: &str, dim: Dim, default: Unit) -> Result<f64, String> {
    if text.chars().count() > MAX_CHARS {
        return Err(format!("A measurement is at most {MAX_CHARS} characters."));
    }
    let tokens = tokens(text)?;
    if tokens.is_empty() {
        return Err("Type a value.".to_owned());
    }
    let default_factor = default.to_internal(1.0);
    let mut parser =
        Parser { tokens: &tokens, at: 0, dim, default: if default_factor.is_finite() && default_factor > 0.0 { default_factor } else { 1.0 } };
    let value = parser.expression(0)?;
    if parser.at < tokens.len() {
        return Err(match tokens.get(parser.at) {
            Some(Token::Close) => "A parenthesis is closed that was not opened.".to_owned(),
            _ => "That is not a measurement. Check the operators and the units.".to_owned(),
        });
    }
    let number = if value.dimension == 0 { value.number * parser.default } else { value.number };
    if number.is_finite() { Ok(number) } else { Err("That value is too large.".to_owned()) }
}

/// A length typed in a field, in inches (the drawing's own, before the page's scale). A plain number is in `unit`, the unit the field shows.
pub fn parse_length(text: &str, unit: Unit) -> Result<f64, String> {
    evaluate(text, Dim::Length, unit)
}

/// An angle typed in a field, in degrees. A plain number is in degrees.
pub fn parse_angle(text: &str) -> Result<f64, String> {
    evaluate(text, Dim::Angle, Unit::Degree).map(|base| Unit::Degree.from_internal(base))
}

/// The magnitude from which a number is shown with a power of ten.
const BIGGEST_PLAIN: f64 = 1.0e12;

/// A number the way a field shows it: at most four places, no trailing zeros.
pub fn number_text(n: f64) -> String {
    if !n.is_finite() {
        return "0".to_owned();
    }
    // A number this large (a coordinate a hostile file holds) is shown rounded, as `1.5e119`, not as a hundred digits.
    if n.abs() >= BIGGEST_PLAIN {
        let text = format!("{n:.4e}");
        return match text.split_once('e') {
            Some((mantissa, power)) => format!("{}e{power}", mantissa.trim_end_matches('0').trim_end_matches('.')),
            None => text,
        };
    }
    let rounded = (n * 1.0e4).round() / 1.0e4;
    let rounded = if rounded == 0.0 { 0.0 } else { rounded };
    let text = format!("{rounded:.4}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// A length in inches as a field shows it, in `unit`: `2.5 in`.
pub fn length_text(inches: f64, unit: Unit) -> String {
    let suffix = unit.suffix().unwrap_or("").to_lowercase();
    let value = number_text(unit.from_internal(inches));
    if suffix.is_empty() { value } else { format!("{value} {suffix}") }
}

/// An angle in degrees as a field shows it, kept from 0 up to 360: `90 deg`. A turn of nothing is `0 deg`.
pub fn angle_text(degrees: f64) -> String {
    let d = if degrees.is_finite() { degrees.rem_euclid(360.0) } else { 0.0 };
    // A turn that is a hair short of a whole one (an angle kept as radians) reads as no turn.
    let d = if (360.0 - d).abs() < 1.0e-6 { 0.0 } else { d };
    format!("{} deg", number_text(d))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn len(text: &str) -> Result<f64, String> {
        parse_length(text, Unit::Inch)
    }

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 1.0e-9
    }

    /// `21 in.` is 21 inches, `1 ft. 5 in.` is 17, and a comma between values is an error.
    #[test]
    fn the_inventorys_examples_are_what_they_say() {
        assert!(near(len("21 in.").unwrap(), 21.0));
        assert!(near(len("1 ft. 5 in.").unwrap(), 17.0));
        let err = len("1 ft, 5 in").unwrap_err();
        assert!(err.contains("comma"), "{err}");
    }

    #[test]
    fn a_plain_number_is_in_the_unit_the_field_shows() {
        assert!(near(parse_length("3", Unit::Inch).unwrap(), 3.0));
        assert!(near(parse_length("3", Unit::Foot).unwrap(), 36.0));
        assert!(near(parse_length("2", Unit::Millimetre).unwrap(), 2.0 / 25.4));
        assert!(near(parse_length("2 cm", Unit::Inch).unwrap(), 2.0 / 2.54));
        assert!(near(parse_length("1 m", Unit::Inch).unwrap(), 1000.0 / 25.4));
    }

    #[test]
    fn the_marks_for_feet_and_inches_and_the_long_names_are_units() {
        assert!(near(len("3'").unwrap(), 36.0));
        assert!(near(len("4\"").unwrap(), 4.0));
        assert!(near(len("1' 5\"").unwrap(), 17.0));
        assert!(near(len("2 feet 3 inches").unwrap(), 27.0));
        assert!(near(len("1 FT 2 IN").unwrap(), 14.0));
    }

    #[test]
    fn the_four_operations_work_with_units_and_plain_numbers() {
        assert!(near(len("1 ft + 2 in").unwrap(), 14.0));
        assert!(near(len("1 ft - 2").unwrap(), 10.0), "a plain number in a sum is in the field's unit");
        assert!(near(len("10 in / 4").unwrap(), 2.5));
        assert!(near(len("2 * 3 in").unwrap(), 6.0));
        assert!(near(len("(3 + 4) * 2").unwrap(), 14.0));
        assert!(near(len("2 ft / 1 ft * 5").unwrap(), 10.0), "a length over a length is a plain number, which is then in the field's unit");
        assert!(near(len("-3 in + 5").unwrap(), 2.0));
        assert!(near(parse_length("1 + 2", Unit::Foot).unwrap(), 36.0));
    }

    #[test]
    fn what_cannot_be_worked_out_is_refused_in_words() {
        for bad in [
            "",
            "   ",
            "abc",
            "3 parsecs",
            "3 in in",
            "in 3",
            "3 +",
            "* 3",
            "(3",
            "3)",
            "3 in * 2 in",
            "3 / 0",
            "3 / 2 in",
            "1 ft, 5 in",
            "1,5",
            "1;5",
            "5 deg",
            "1e400",
            "3 ?",
            "..",
        ] {
            let e = len(bad).unwrap_err();
            assert!(!e.is_empty(), "{bad:?}");
        }
        assert!(len(&"1".repeat(MAX_CHARS + 1)).unwrap_err().contains("characters"));
        let deep = format!("{}1{}", "(".repeat(MAX_DEPTH + 4), ")".repeat(MAX_DEPTH + 4));
        assert!(len(&deep).unwrap_err().contains("deep"));
    }

    #[test]
    fn an_angle_takes_degrees_radians_and_arithmetic() {
        assert!(near(parse_angle("90").unwrap(), 90.0));
        assert!(near(parse_angle("90 deg").unwrap(), 90.0));
        assert!(near(parse_angle("45\u{b0} * 2").unwrap(), 90.0));
        assert!(near(parse_angle("180 + 45").unwrap(), 225.0));
        assert!(near(parse_angle("1 rad").unwrap(), 180.0 / std::f64::consts::PI));
        assert!(parse_angle("3 in").is_err(), "a length is not an angle");
        assert!(parse_angle("1, 2").unwrap_err().contains("comma"));
    }

    #[test]
    fn a_hostile_coordinate_is_shown_rounded_and_not_as_a_hundred_digits() {
        assert_eq!(number_text(1.0e119), "1e119");
        assert_eq!(number_text(-1.5e30), "-1.5e30");
        assert_eq!(number_text(123_456_789_012.5), "123456789012.5");
        assert!(number_text(f64::MAX).len() < 16, "{}", number_text(f64::MAX));
        assert!(length_text(1.0e200, Unit::Inch).len() < 16);
    }

    #[test]
    fn a_field_shows_numbers_with_no_noise() {
        assert_eq!(number_text(2.5), "2.5");
        assert_eq!(number_text(3.0), "3");
        assert_eq!(number_text(1.0 / 3.0), "0.3333");
        assert_eq!(number_text(-0.00001), "0");
        assert_eq!(number_text(f64::NAN), "0");
        assert_eq!(length_text(17.0, Unit::Inch), "17 in");
        assert_eq!(length_text(17.0, Unit::Foot), "1.4167 ft");
        assert_eq!(length_text(25.4 / 25.4 * 10.0, Unit::Millimetre), "254 mm");
        assert_eq!(angle_text(90.0), "90 deg");
        assert_eq!(angle_text(-90.0), "270 deg");
        assert_eq!(angle_text(359.9999999999), "0 deg");
        assert_eq!(angle_text(f64::INFINITY), "0 deg");
    }

    #[test]
    fn what_is_shown_is_read_back_as_it_was() {
        for inches in [0.0, 1.5, 17.0, 0.3333, 120.0] {
            for unit in [Unit::Inch, Unit::Foot, Unit::Millimetre, Unit::Centimetre] {
                let shown = length_text(inches, unit);
                let read = parse_length(&shown, unit).unwrap();
                // Four places of the field's unit.
                let tolerance = 0.5e-4 * unit.to_internal(1.0);
                assert!((read - inches).abs() <= tolerance + 1.0e-12, "{shown} read as {read}, not {inches}");
            }
        }
    }
}
