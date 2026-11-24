//! The values ODF attributes hold, read strictly (ODF 1.4 Part 3 §18.3 and the schema's `length`, `percent`, `color`
//! and `angle` patterns): a value that breaks its pattern reads as nothing, and the caller treats the attribute as absent and keeps it.
//!
//! A length is a signed decimal number with no exponent, directly followed by one of `cm`, `mm`, `in`, `pt`, `pc` or `px`; it reads as
//! inches by 1 in = 2.54 cm = 25.4 mm = 72 pt = 6 pc, and `px` as 1/96 in. The unit becomes the cell's `U` (`CM`, `MM`, `IN`,
//! `PT`, `P`, none for `px`).

/// A length unit of R17.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LenUnit {
    Cm,
    Mm,
    In,
    Pt,
    Pc,
    Px,
}

impl LenUnit {
    /// The cell's `U` for a length read in this unit: `CM`, `MM`, `IN`, `PT`, `P`, and none for `px`.
    pub(crate) fn word(self) -> Option<&'static str> {
        match self {
            LenUnit::Cm => Some("CM"),
            LenUnit::Mm => Some("MM"),
            LenUnit::In => Some("IN"),
            LenUnit::Pt => Some("PT"),
            LenUnit::Pc => Some("P"),
            LenUnit::Px => None,
        }
    }

    /// How many of this unit make an inch.
    fn per_inch(self) -> f64 {
        match self {
            LenUnit::Cm => 2.54,
            LenUnit::Mm => 25.4,
            LenUnit::In => 1.0,
            LenUnit::Pt => 72.0,
            LenUnit::Pc => 6.0,
            LenUnit::Px => 96.0,
        }
    }

    /// One of this unit, in inches.
    pub(crate) fn inches(self) -> f64 {
        1.0 / self.per_inch()
    }
}

/// A length read: its value in inches, and the unit it was written in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Length {
    pub inches: f64,
    pub unit: LenUnit,
}

/// Whether `text` is a decimal number as ODF's `length` and `percent` write one: an optional `-`, then digits with an optional fraction, or
/// a point and digits. No exponent, no `+`, no blanks.
fn is_decimal(text: &str) -> bool {
    let body = text.strip_prefix('-').unwrap_or(text);
    let (whole, fraction) = match body.split_once('.') {
        Some((w, f)) => (w, Some(f)),
        None => (body, None),
    };
    let digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    match fraction {
        None => !whole.is_empty() && digits(whole),
        Some(f) => digits(whole) && digits(f) && !(whole.is_empty() && f.is_empty()),
    }
}

/// A decimal number of ODF's form ([`is_decimal`]), finite.
pub(crate) fn decimal(text: &str) -> Option<f64> {
    if !is_decimal(text) {
        return None;
    }
    text.parse::<f64>().ok().filter(|n| n.is_finite())
}

/// A length, or `None` when the text is not one.
pub(crate) fn length(text: &str) -> Option<Length> {
    let units = [("cm", LenUnit::Cm), ("mm", LenUnit::Mm), ("in", LenUnit::In), ("pt", LenUnit::Pt), ("pc", LenUnit::Pc), ("px", LenUnit::Px)];
    let (number, unit) = units.iter().find_map(|(suffix, unit)| text.strip_suffix(suffix).map(|n| (n, *unit)))?;
    let n = decimal(number)?;
    let inches = n / unit.per_inch();
    inches.is_finite().then_some(Length { inches, unit })
}

/// Hundredths of a millimetre in an inch.
pub(crate) const HUNDREDTHS_PER_INCH: f64 = 2540.0;

/// A coordinate of a translation in `draw:transform` (the arguments of `translate`, and `e` and `f` of `matrix`): a length, or **a plain decimal number, which
/// LibreOffice 26.8.2.2 reads as hundredths of a millimetre** (a black box: `matrix (1 0 0 1 2000 3000)`, `translate (2000 3000)` and `matrix (1 0 0 1 2cm
/// 3000)` drawn with the corner at 2 cm, 3 cm, `matrix (1 0 0 1 2 3)` at 0.002 cm, 0.003 cm). ODF 1.4 asks for lengths with units there. Gives the
/// inches and the unit, `None` for a plain number.
pub(crate) fn translation(text: &str) -> Option<(f64, Option<LenUnit>)> {
    if let Some(l) = length(text) {
        return Some((l.inches, Some(l.unit)));
    }
    let inches = decimal(text)? / HUNDREDTHS_PER_INCH;
    inches.is_finite().then_some((inches, None))
}

/// A percentage: the number before `%` (`60%` is 60), or `None`.
pub(crate) fn percent(text: &str) -> Option<f64> {
    decimal(text.strip_suffix('%')?)
}

/// A colour `#rrggbb` (§18.3.9) as the cell holds it, `#RRGGBB`, or `None`.
pub(crate) fn colour(text: &str) -> Option<String> {
    let hex = text.strip_prefix('#')?;
    (hex.len() == 6 && hex.bytes().all(|b| b.is_ascii_hexdigit())).then(|| format!("#{}", hex.to_ascii_uppercase()))
}

/// A colour's three channels as the cell holds the colour, `#RRGGBB`.
pub(crate) fn hex([red, green, blue]: [u8; 3]) -> String {
    format!("#{red:02X}{green:02X}{blue:02X}")
}

/// A colour's three channels, each 0 to 255.
pub(crate) fn rgb(text: &str) -> Option<[u8; 3]> {
    let hex = colour(text)?;
    let byte = |at: usize| hex.get(at..at + 2).and_then(|h| u8::from_str_radix(h, 16).ok());
    Some([byte(1)?, byte(3)?, byte(5)?])
}

/// An `angle` (§18.3.1): a number in degrees when it carries no unit, or with `deg`, `rad` or `grad`. In degrees.
pub(crate) fn angle_degrees(text: &str) -> Option<f64> {
    let (number, per_degree) = if let Some(n) = text.strip_suffix("deg") {
        (n, 1.0)
    } else if let Some(n) = text.strip_suffix("grad") {
        (n, 0.9)
    } else if let Some(n) = text.strip_suffix("rad") {
        (n, 180.0 / std::f64::consts::PI)
    } else {
        (text, 1.0)
    };
    let d = number_any(number)? * per_degree;
    d.is_finite().then_some(d)
}

/// A number as SVG writes one in path data and transforms: an optional sign, digits with an optional fraction, and an optional exponent.
pub(crate) fn number_any(text: &str) -> Option<f64> {
    let bytes = text.as_bytes();
    let mut at = 0;
    if matches!(bytes.first(), Some(b'+' | b'-')) {
        at += 1;
    }
    let digits_from = |i: usize| {
        let n = bytes.get(i..).map_or(0, |rest| rest.iter().take_while(|b| b.is_ascii_digit()).count());
        (i + n, n > 0)
    };
    let (after_whole, whole) = digits_from(at);
    at = after_whole;
    let mut fraction = false;
    if bytes.get(at) == Some(&b'.') {
        let (after, any) = digits_from(at + 1);
        at = after;
        fraction = any;
    }
    if !whole && !fraction {
        return None;
    }
    if matches!(bytes.get(at), Some(b'e' | b'E')) {
        let mut e = at + 1;
        if matches!(bytes.get(e), Some(b'+' | b'-')) {
            e += 1;
        }
        let (after, any) = digits_from(e);
        if !any {
            return None;
        }
        at = after;
    }
    if at != bytes.len() {
        return None;
    }
    text.parse::<f64>().ok().filter(|n| n.is_finite())
}

/// A whole number that fits an `i64`, in XML Schema's lexical form for the integer types ODF's integer attributes use: digits, with an
/// optional `-` or `+` before them.
pub(crate) fn integer(text: &str) -> Option<i64> {
    let digits = text.strip_prefix(['-', '+']).unwrap_or(text);
    (!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())).then(|| text.parse::<i64>().ok()).flatten()
}

/// `true` or `false` (§18.3.3); anything else is `None`.
pub(crate) fn boolean(text: &str) -> Option<bool> {
    match text {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lengths_read_strictly() {
        assert_eq!(length("29.7cm").map(|l| l.inches), Some(29.7 / 2.54));
        assert_eq!(length("-0.8cm").map(|l| l.inches), Some(-0.8 / 2.54));
        assert_eq!(length(".5in").map(|l| l.inches), Some(0.5));
        assert_eq!(length("12pt").map(|l| (l.inches, l.unit)), Some((12.0 / 72.0, LenUnit::Pt)));
        assert_eq!(length("1pc").map(|l| l.inches), Some(1.0 / 6.0));
        assert_eq!(length("96px").map(|l| l.inches), Some(1.0));
        for bad in ["1", "1e2cm", "+1cm", " 1cm", "1 cm", "cm", "-cm", ".cm", "1.2.3cm", "1CM", "nancm", "infcm", "1.5e3cm", "x.5cm", "1.e2cm"] {
            assert_eq!(length(bad), None, "{bad}");
        }
    }

    #[test]
    fn percents_colours_and_angles() {
        assert_eq!(percent("60%"), Some(60.0));
        assert_eq!(percent("-25.0%"), Some(-25.0));
        assert_eq!(percent("60"), None);
        assert_eq!(colour("#e0ecf8").as_deref(), Some("#E0ECF8"));
        assert_eq!(colour("#e0ecf"), None);
        assert_eq!(colour("e0ecf8"), None);
        assert_eq!(rgb("#ff8001"), Some([255, 128, 1]));
        assert_eq!(angle_degrees("60"), Some(60.0));
        assert_eq!(angle_degrees("90deg"), Some(90.0));
        assert_eq!(angle_degrees("100grad"), Some(90.0));
        assert!((angle_degrees("3.141592653589793rad").unwrap_or(0.0) - 180.0).abs() < 1e-9);
        assert_eq!(number_any("-1.5e3"), Some(-1500.0));
        assert_eq!(number_any("12"), Some(12.0));
        assert_eq!(number_any("1e+5"), Some(100_000.0));
        assert_eq!(number_any("2E-1"), Some(0.2));
        assert_eq!(boolean("false"), Some(false));
        assert_eq!(boolean("true"), Some(true));
        assert_eq!(boolean("1"), None);
        assert_eq!(number_any("1e"), None);
        assert_eq!(number_any("."), None);
        assert_eq!(number_any(".5"), Some(0.5), "a fraction with no whole part");
        assert_eq!(number_any("-.25e1"), Some(-2.5));
        assert_eq!(integer("-12"), Some(-12));
        assert_eq!(integer("+12"), Some(12));
        for bad in ["1.0", "", "-", "+", "1a", "--1", "+-1", " 1"] {
            assert_eq!(integer(bad), None, "{bad}");
        }
    }
}
