//! A number in text, and a number written as text: what `vDouble` and `vString` do with a string.
//!
//! **Culture.** `vDouble` names .NET's `Convert.ToDouble`, whose plain string form reads digits by the current culture
//! **Project rule:** the invariant culture, so a document reads the same on every machine: a sign,
//! digits, one point, an exponent, and blanks round them; no digit group separator. To flip, change `parse_decimal` and nothing else.

use diacraft_formula::Expr;
use diacraft_units::{Quantity, Unit, parse_unit};

/// A finite decimal number, as `Convert.ToDouble` reads it in the invariant culture: blanks, a sign, digits with at most one point, and
/// an exponent. `None` for anything else, `inf`, `nan` and hexadecimal included, and for a number too large for a double.
pub fn parse_decimal(text: &str) -> Option<f64> {
    let t = text.trim_matches(|c: char| c.is_ascii_whitespace());
    if t.is_empty() {
        return None;
    }
    let bytes = t.as_bytes();
    let mut i = 0;
    if matches!(bytes.get(i), Some(b'+' | b'-')) {
        i += 1;
    }
    let mut digits = 0;
    while bytes.get(i).is_some_and(u8::is_ascii_digit) {
        i += 1;
        digits += 1;
    }
    if bytes.get(i) == Some(&b'.') {
        i += 1;
        while bytes.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
            digits += 1;
        }
    }
    if digits == 0 {
        return None;
    }
    if matches!(bytes.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(bytes.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        let start = i;
        while bytes.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        if i == start {
            return None;
        }
    }
    if i != bytes.len() {
        return None;
    }
    t.parse::<f64>().ok().filter(|x| x.is_finite())
}

/// The longest text a string is read as a number or a unit from. A string longer than this is no number, which keeps the work of a
/// hostile cell bounded.
const MAX_NUMBER_TEXT: usize = 256;

/// What a string says as a number with an optional unit (`vDouble`): `3`, `3 in`, `1/8 in`, `5 1/2`,
/// `25%`, `2FT 3IN`, `3 sq. ft.`, and `TRUE` and `FALSE` in any case. `None` for anything else. A string that names cells or
/// holds an expression is not read here (the last row: the evaluator reads it as a formula).
pub fn read_quantity(text: &str) -> Option<Quantity> {
    let t = text.trim_matches(|c: char| c.is_ascii_whitespace());
    if t.is_empty() || t.len() > MAX_NUMBER_TEXT {
        return None;
    }
    if t.eq_ignore_ascii_case("true") {
        return Some(Quantity::new(1.0, Unit::Unitless));
    }
    if t.eq_ignore_ascii_case("false") {
        return Some(Quantity::new(0.0, Unit::Unitless));
    }
    if let Some(q) = read_simple(t) {
        return Some(q);
    }
    read_as_literal(t)
}

/// A number, a fraction or a mixed number, and then a unit.
fn read_simple(t: &str) -> Option<Quantity> {
    let (value, rest) = split_number(t)?;
    let rest = rest.trim_start();
    let unit_text = rest;
    if unit_text.is_empty() {
        return Some(Quantity::new(value, Unit::Unitless));
    }
    read_unit_text(value, unit_text)
}

/// `value` in the unit the text names: `in`, `%`, `sq. ft.`, `in.^2`, `FT^3`.
fn read_unit_text(value: f64, text: &str) -> Option<Quantity> {
    let t = text.trim();
    let lower = t.to_ascii_lowercase();
    let (square, body) = match lower.strip_prefix("sq") {
        Some(after) if after.starts_with('.') || after.starts_with(' ') || after.is_empty() => (true, after.trim_start_matches(['.', ' '])),
        Some(after) if after.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) => (true, after),
        _ => (false, lower.as_str()),
    };
    // A power straight after the unit: `ft^3`, `in.^2`.
    let (name, power) = match body.split_once('^') {
        Some((name, power)) => (name, Some(power.trim().parse::<i32>().ok()?)),
        None => (body, None),
    };
    let name = name.trim().trim_end_matches('.').trim();
    let unit = parse_unit(name)?;
    let power = match (square, power) {
        (true, None) => Some(2),
        (true, Some(p)) => Some(p),
        (false, p) => p,
    };
    match power {
        None => Some(Quantity::from_value(value, unit)),
        Some(p) => {
            let p = i8::try_from(p).ok()?;
            multidimensional(value, unit, p)
        }
    }
}

/// `value` of `unit` to the power `p`, as `3 IN^2` is: the stored number is `value` times the unit's factor to that power.
/// `None` for a unit that cannot have a power (the base list leaves out acre, hectare, percent and the compound units) and for a
/// power out of range (the caller makes `#DIM!` of it).
pub fn multidimensional(value: f64, unit: Unit, power: i8) -> Option<Quantity> {
    use diacraft_units::Measure;
    match unit.measure() {
        Measure::Length | Measure::Angle | Measure::Duration | Measure::Date => {}
        Measure::None | Measure::Area | Measure::Percent => return None,
    }
    if matches!(unit, Unit::FeetInches | Unit::DegreesMinutesSeconds | Unit::CiceroDidot | Unit::PicaPoints) {
        return None;
    }
    let stored = value * unit.base_per_unit().powi(i32::from(power));
    Some(Quantity::with_dim(stored, unit, power))
}

/// A leading number (with a sign), a fraction after it, or a mixed number: `3`, `-2.5e3`, `1/8`, `5 1/2`. Returns the value and the text
/// after it.
fn split_number(t: &str) -> Option<(f64, &str)> {
    let (sign, body) = match t.as_bytes().first() {
        Some(b'-') => (-1.0, t.get(1..)?),
        Some(b'+') => (1.0, t.get(1..)?),
        _ => (1.0, t),
    };
    let (first, whole, rest) = scan_decimal(body)?;
    // `1/8`: a number, a slash and a whole number.
    if let Some(after_slash) = rest.trim_start().strip_prefix('/') {
        let (denominator, after) = scan_uint(after_slash.trim_start())?;
        if denominator == 0.0 {
            return None;
        }
        return Some((sign * first / denominator, after));
    }
    // `5 1/2`: a whole number, a blank, and a fraction.
    if whole && rest.starts_with(|c: char| c.is_ascii_whitespace()) {
        let candidate = rest.trim_start();
        if let Some((numerator, after_n)) = scan_uint(candidate)
            && let Some(after_slash) = after_n.trim_start().strip_prefix('/')
            && let Some((denominator, after)) = scan_uint(after_slash.trim_start())
            && denominator != 0.0
        {
            return Some((sign * (first + numerator / denominator), after));
        }
    }
    Some((sign * first, rest))
}

/// An unsigned decimal at the start of `t`, with an exponent only when digits follow it: the value, whether it was written as whole
/// digits (no point, no exponent), and the text after it.
fn scan_decimal(t: &str) -> Option<(f64, bool, &str)> {
    let bytes = t.as_bytes();
    let mut i = 0;
    while bytes.get(i).is_some_and(u8::is_ascii_digit) {
        i += 1;
    }
    let int_digits = i;
    let mut whole = true;
    let mut frac_digits = 0;
    if bytes.get(i) == Some(&b'.') {
        whole = false;
        i += 1;
        while bytes.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
            frac_digits += 1;
        }
    }
    if int_digits + frac_digits == 0 {
        return None;
    }
    if matches!(bytes.get(i), Some(b'e' | b'E')) {
        let mut j = i + 1;
        if matches!(bytes.get(j), Some(b'+' | b'-')) {
            j += 1;
        }
        let exp_start = j;
        while bytes.get(j).is_some_and(u8::is_ascii_digit) {
            j += 1;
        }
        if j > exp_start {
            whole = false;
            i = j;
        }
    }
    let value: f64 = t.get(..i)?.parse().ok()?;
    Some((value, whole, t.get(i..)?))
}

/// Whole digits at the start of `t`, as a number, and the text after them.
fn scan_uint(t: &str) -> Option<(f64, &str)> {
    let end = t.bytes().take_while(u8::is_ascii_digit).count();
    if end == 0 {
        return None;
    }
    Some((t.get(..end)?.parse().ok()?, t.get(end..)?))
}

/// A literal the formula grammar reads: `8FT 4IN`, `10°15'30"`, `2C4`, `3P6`. `None` for text that is not one literal.
fn read_as_literal(t: &str) -> Option<Quantity> {
    let formula = diacraft_formula::parse(t);
    let expr = formula.ast.ok()?;
    let (negative, expr) = match expr {
        Expr::Unary(diacraft_formula::UnaryOp::Minus, inner) => (true, *inner),
        Expr::Unary(diacraft_formula::UnaryOp::Plus, inner) => (false, *inner),
        other => (false, other),
    };
    let Expr::Number(n) = expr else { return None };
    let q = match n.quantity() {
        Some(q) => q,
        None => match n {
            diacraft_formula::Number::Measured { value, unit, power: Some(p) } => multidimensional(value, unit, i8::try_from(p).ok()?)?,
            _ => return None,
        },
    };
    Some(if negative { Quantity::with_dim(-q.stored, q.unit, q.dim()) } else { q })
}

/// A number as a string holds it (`vString`): the value to 15 significant digits with no trailing zeros, in plain digits when
/// the number is not very large or very small. **Project rule (`unknown`):** samples hold whole numbers only.
pub fn format_number(x: f64) -> String {
    if x == 0.0 {
        return "0".to_owned();
    }
    if !x.is_finite() {
        return if x > 0.0 { "1.#INF".to_owned() } else { "-1.#INF".to_owned() };
    }
    let magnitude = x.abs();
    if !(1e-5..1e15).contains(&magnitude) {
        let s = format!("{x:.14e}");
        return trim_exponent_form(&s);
    }
    // 15 significant digits: format with enough decimals, then trim.
    let exp10 = magnitude.log10().floor() as i32;
    let decimals = (14 - exp10).clamp(0, 30) as usize;
    let mut s = format!("{x:.decimals$}");
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    if s == "-0" { "0".to_owned() } else { s }
}

/// `1.50000000000000e3` as `1.5E3`.
fn trim_exponent_form(s: &str) -> String {
    let Some((mantissa, exp)) = s.split_once('e') else { return s.to_owned() };
    let mut m = mantissa.to_owned();
    if m.contains('.') {
        while m.ends_with('0') {
            m.pop();
        }
        if m.ends_with('.') {
            m.pop();
        }
    }
    format!("{m}E{exp}")
}
