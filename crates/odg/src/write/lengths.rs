//! Lengths as the writer writes them (design, "Lengths"): the shortest decimal that reads back to the cell's value, in
//! the attribute's own unit, never with an exponent.
//!
//! The reader turns `n` of a unit into inches by one division (`read::units::length`), so the writer asks the reader's own function whether a
//! candidate reads back, and does not repeat its arithmetic. A candidate is the value in the unit, rounded to 1, 2, 3, ... significant digits;
//! the first that reads back wins. **A value that came from a file is reached exactly** (`2.5cm` is 2.5 divided by 2.54, and 2.5 is the
//! first candidate that gives that number back), which is what a round trip needs. A value a calculation made (a pin and half a width, taken
//! away again) can differ from a decimal's reading by the last bit or two, and then no short decimal gives it back to the bit and a long one
//! is not what a person wrote: up to 11 digits a candidate that reads back within [`TOLERANCE`] of the value is taken as the value (a
//! millionth of a millimetre at the size of a page, below what any producer writes), and beyond them only an exact reading is, up to the
//! 17 digits that always give a double back. A value with no decimal that reads back to the bit within those, and the last 17 digits of it
//! are the nearest there is, is written as the 17.

use crate::read::units::{LenUnit, length};

/// How far a candidate's reading may be from the value, in inches, at most 11 digits in: 1E-12 of an inch and a millionth of that for
/// each inch of the value's size, a small part of a nanometre at the size of a page.
pub(crate) const TOLERANCE: f64 = 1e-12;

/// The most digits at which a candidate within [`TOLERANCE`] is taken for the value.
const SHORT: usize = 11;

/// The largest size, in the attribute's own unit, the writer writes: a length of more digits than this is no drawing's, and writing it
/// would give an attribute of hundreds of digits.
const LARGEST: f64 = 1e15;

/// Whether two lengths in inches are the same length for a save: within [`TOLERANCE`] scaled by their size, which keeps a value a
/// calculation made as the file's own and tells a move of a hundredth of a millimetre from none.
pub(crate) fn same(a: f64, b: f64) -> bool {
    (a - b).abs() <= TOLERANCE * (1.0 + a.abs().max(b.abs()))
}

/// `inches` as the text of a length in `unit`, with the unit after it, or `None` for a value that is not finite or is past [`LARGEST`] in
/// the unit.
pub(crate) fn length_text(inches: f64, unit: LenUnit) -> Option<String> {
    let number = number_text(inches, unit)?;
    Some(format!("{number}{}", unit_suffix(unit)))
}

/// The unit a length attribute's two-letter suffix names (`cm`, `mm`, `in`, `pt`, `pc`, `px`), as a held unit records it.
pub(crate) fn unit_of_suffix(suffix: &str) -> Option<LenUnit> {
    Some(match suffix {
        "cm" => LenUnit::Cm,
        "mm" => LenUnit::Mm,
        "in" => LenUnit::In,
        "pt" => LenUnit::Pt,
        "pc" => LenUnit::Pc,
        "px" => LenUnit::Px,
        _ => return None,
    })
}

/// The unit a cell's `U` names (`CM`, `MM`, `IN`, `PT`, `P`: [`LenUnit::word`]); a cell with none shows no unit a length attribute can spell.
pub(crate) fn unit_of_word(word: &str) -> Option<LenUnit> {
    Some(match word {
        "CM" => LenUnit::Cm,
        "MM" => LenUnit::Mm,
        "IN" => LenUnit::In,
        "PT" => LenUnit::Pt,
        "P" => LenUnit::Pc,
        _ => return None,
    })
}

/// The unit's suffix as it is written.
pub(crate) fn unit_suffix(unit: LenUnit) -> &'static str {
    match unit {
        LenUnit::Cm => "cm",
        LenUnit::Mm => "mm",
        LenUnit::In => "in",
        LenUnit::Pt => "pt",
        LenUnit::Pc => "pc",
        LenUnit::Px => "px",
    }
}

/// The number part of [`length_text`]: no exponent, no unit, no `-0`.
fn number_text(inches: f64, unit: LenUnit) -> Option<String> {
    if !inches.is_finite() {
        return None;
    }
    let in_unit = inches / unit.inches();
    if !in_unit.is_finite() || in_unit.abs() > LARGEST {
        return None;
    }
    if in_unit == 0.0 {
        return Some("0".to_owned());
    }
    let suffix = unit_suffix(unit);
    let reads = |text: &str| length(&format!("{text}{suffix}")).map(|l| l.inches);
    for digits in 1..=17usize {
        let Some(candidate) = round_to(in_unit, digits) else { continue };
        let Some(back) = reads(&candidate) else { continue };
        if back == inches || (digits <= SHORT && (back - inches).abs() <= TOLERANCE * (1.0 + inches.abs())) {
            return Some(candidate);
        }
    }
    // No decimal of at most 17 digits gives the value back: the nearest of 17 is what there is.
    round_to(in_unit, 17)
}

/// `value` rounded to `digits` significant digits, as a plain decimal with no exponent and no trailing zeros (`-0` is `0`).
fn round_to(value: f64, digits: usize) -> Option<String> {
    let scientific = format!("{:.*e}", digits.saturating_sub(1), value);
    let (mantissa, exponent) = scientific.split_once('e')?;
    let exponent: i32 = exponent.parse().ok()?;
    let (sign, mantissa) = match mantissa.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", mantissa),
    };
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    // The value is 0.d1d2d3... times 10^(exponent + 1).
    let point = i64::from(exponent) + 1;
    let (whole, fraction) = if point <= 0 {
        let zeros = usize::try_from(-point).ok()?;
        (String::new(), format!("{}{digits}", "0".repeat(zeros)))
    } else {
        let at = usize::try_from(point).ok()?;
        if at >= digits.len() {
            (format!("{digits}{}", "0".repeat(at - digits.len())), String::new())
        } else {
            (digits.get(..at)?.to_owned(), digits.get(at..)?.to_owned())
        }
    };
    let fraction = fraction.trim_end_matches('0');
    let whole = if whole.is_empty() { "0" } else { whole.as_str() };
    let text = if fraction.is_empty() { whole.to_owned() } else { format!("{whole}.{fraction}") };
    // A magnitude that rounds to nothing has no sign.
    Some(if text == "0" { text } else { format!("{sign}{text}") })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounding_writes_plain_decimals() {
        assert_eq!(round_to(2.5, 1).as_deref(), Some("2"), "2.5 to one digit is 2 (round half to even)");
        assert_eq!(round_to(2.5, 2).as_deref(), Some("2.5"));
        assert_eq!(round_to(1234.5678, 6).as_deref(), Some("1234.57"));
        assert_eq!(round_to(0.000_123_4, 3).as_deref(), Some("0.000123"));
        assert_eq!(round_to(-0.5, 1).as_deref(), Some("-0.5"));
        assert_eq!(round_to(1.0e10, 1).as_deref(), Some("10000000000"));
        assert_eq!(round_to(0.0, 3).as_deref(), Some("0"));
        assert_eq!(round_to(-0.0, 3).as_deref(), Some("0"));
    }

    #[test]
    fn lengths_a_file_holds_come_back_in_their_own_text() {
        for (text, unit) in [
            ("2.5", LenUnit::Cm),
            ("29.7", LenUnit::Cm),
            ("0.35", LenUnit::Cm),
            ("-5.7", LenUnit::Cm),
            ("25.4", LenUnit::Mm),
            ("8.5", LenUnit::In),
            ("10.5", LenUnit::Pt),
        ] {
            let inches = length(&format!("{text}{}", unit_suffix(unit))).map(|l| l.inches).unwrap_or(f64::NAN);
            assert_eq!(length_text(inches, unit).as_deref(), Some(format!("{text}{}", unit_suffix(unit)).as_str()), "{text}");
        }
    }

    #[test]
    fn a_value_a_calculation_made_is_written_as_the_decimal_within_the_tolerance_of_it() {
        let base = length("2.5cm").map(|l| l.inches).unwrap_or(f64::NAN);
        // A few units in the last place away, and the tolerance's own distance (1E-12 of an inch, and a part for each inch of the size).
        assert_eq!(length_text(base + 3e-16, LenUnit::Cm).as_deref(), Some("2.5cm"));
        assert_eq!(length_text(base + 1e-12, LenUnit::Cm).as_deref(), Some("2.5cm"));
        assert_eq!(length_text(base - 1e-12, LenUnit::Cm).as_deref(), Some("2.5cm"));
        // In a small unit the tolerance is many digits of the value, and it is still the short decimal: 2.5pt a trillionth of an inch away.
        let pt = length("2.5pt").map(|l| l.inches).unwrap_or(f64::NAN);
        assert_eq!(length_text(pt + 1e-12, LenUnit::Pt).as_deref(), Some("2.5pt"));
        assert_eq!(length_text(pt - 1e-12, LenUnit::Pt).as_deref(), Some("2.5pt"));
        // Past it the value is not that decimal.
        assert_ne!(length_text(base + 1e-9, LenUnit::Cm).as_deref(), Some("2.5cm"));
        // Two lengths are the same length within the tolerance scaled by their size.
        assert!(same(1.0, 1.0));
        assert!(same(1.0, 1.0 + 1.5e-12) && same(1.0 + 1.5e-12, 1.0));
        assert!(!same(1.0, 1.0 + 3e-12));
        assert!(same(1000.0, 1000.0 + 5e-10));
        assert!(!same(1000.0, 1000.0 + 2e-9));
        assert!(!same(0.0, 5e-12));
    }

    #[test]
    fn the_largest_length_written_is_a_quadrillion_of_the_unit() {
        assert_eq!(length_text(1e15, LenUnit::In).as_deref(), Some("1000000000000000in"));
        assert!(length_text(1.0001e15, LenUnit::In).is_none());
        assert!(length_text(-1.0001e15, LenUnit::In).is_none());
        assert_eq!(length_text(-1e15, LenUnit::In).as_deref(), Some("-1000000000000000in"));
    }

    #[test]
    fn what_cannot_be_written_is_none() {
        assert_eq!(length_text(f64::NAN, LenUnit::Cm), None);
        assert_eq!(length_text(f64::INFINITY, LenUnit::Cm), None);
        assert_eq!(length_text(1e300, LenUnit::In), None);
        assert_eq!(length_text(0.0, LenUnit::Cm).as_deref(), Some("0cm"));
        assert_eq!(length_text(-0.0, LenUnit::Mm).as_deref(), Some("0mm"));
    }
}
