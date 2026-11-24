//! The conversions a function argument declares: `vDouble`, `vDoubleEx`, `vBoolean`, `vString` and the
//! whole-number ones.
//!
//! A conversion either gives the converted value or the value the function must return instead: an error that came in, `Unsupported`,
//! or the error an argument that "is not valid" is given (`rules::BAD_ARGUMENT_ERROR`, `rules::UNREADABLE_STRING_IN_ARITHMETIC`).
//! Which of the two a string that cannot be read gets depends on the caller, since the sources disagree: arithmetic is `#VALUE!`
//! A comparison reads 0, a test reads `FALSE`, and a
//! function's argument is `#VALUE!` except the empty string, which is 0 ([`Unreadable::EmptyIsZero`]).

use std::sync::Arc;

use diacraft_formula::Expr;
use diacraft_units::{Quantity, Unit};

use crate::eval::{Env, Evaluator};
use crate::ops::Operand;
use crate::rules::{BAD_ARGUMENT_ERROR, LOOP_ERROR, MAX_STRING_FORMULA_DEPTH, UNREADABLE_STRING_IN_ARITHMETIC};
use crate::store::{encode, serial_to_iso};
use crate::strnum::{format_number, read_quantity};
use crate::value::{CellError, Value};

/// The longest string read as a formula. A project rule: the formula parser limits its own input, and a string is usually a few words.
const MAX_STRING_FORMULA_BYTES: usize = 4096;

/// What a string that does not read as a number is taken for, by who asks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Unreadable {
    /// An error: arithmetic.
    Error,
    /// An error, except the empty string, which is 0: a function's argument. **Project rule (`unknown`):**
    /// an empty cell given to `MAX` raises no error in the files, and whether it counts as 0 or as no value at all is not told (the two cached
    /// values that show it, `MAX(2, empty)` and `ROUND(MAX(empty, 1.5), 0)`, are 2 either way). It is 0, the typed empty value of an operand
    /// that cannot convert (an empty slot). An empty text cell of the data
    /// batch is `""`, where the cell, unlisted, read as the number 0 its kind gives, and the files were scored on that 0.
    /// To flip to "no value", skip such an argument in `fn_math::extreme` and `sum` and read it as 0 elsewhere. A word and a string of
    /// spaces stay errors.
    EmptyIsZero,
    /// 0: a comparison.
    Zero,
}

impl Evaluator<'_, '_> {
    /// A string as a formula, worked out on the context's sheet: `None` for a string that is not one (it does not parse). The
    /// evaluation is one level deeper, so a string that reads itself ends (`MAX_STRING_FORMULA_DEPTH`).
    pub(crate) fn eval_string_formula(&mut self, text: &str, env: &Env) -> Option<Value> {
        if env.string_depth >= MAX_STRING_FORMULA_DEPTH || text.len() > MAX_STRING_FORMULA_BYTES || text.trim().is_empty() {
            return None;
        }
        // The parse costs its length: 4 KB parsed again and again is what a step must not stand for.
        if !self.work(text.len()) {
            return Some(Value::Error(LOOP_ERROR));
        }
        let formula = diacraft_formula::parse(text);
        let expr: Expr = formula.ast.ok()?;
        let mut inner = env.clone();
        inner.string_depth += 1;
        Some(self.eval(&expr, &inner))
    }

    /// `vDoubleEx`: a number with its unit and dimension, or a currency with its code. A string is read as a number, or as the
    /// formula it spells; one that is neither is `unreadable`'s answer.
    fn number_of(&mut self, v: &Value, env: &Env, unreadable: Unreadable) -> Result<Operand, Value> {
        match v {
            Value::Number(q) => Ok(Operand::Num(*q)),
            Value::Currency { amount, currency } => Ok(Operand::Cy { amount: *amount, currency: *currency }),
            Value::Bool(b) => Ok(Operand::Num(Quantity::new(if *b { 1.0 } else { 0.0 }, Unit::Unitless))),
            Value::Text(s) => self.text_to_number(s, env, unreadable),
            // A colour, a point, a path and a sheet are not numbers:
            // project rule, `#VALUE!`.
            Value::Color(_) | Value::Point(_) | Value::Constructor(_) | Value::Sheet(_) => Err(Value::Error(BAD_ARGUMENT_ERROR)),
            Value::Error(_) | Value::Unsupported(_) => Err(v.clone()),
        }
    }

    fn text_to_number(&mut self, s: &str, env: &Env, unreadable: Unreadable) -> Result<Operand, Value> {
        if !self.work(s.len()) {
            return Err(Value::Error(LOOP_ERROR));
        }
        // The empty string, only where a function asks (`Unreadable::EmptyIsZero`): not a string of spaces, which no source works either.
        if s.is_empty() && unreadable == Unreadable::EmptyIsZero {
            return Ok(Operand::Num(Quantity::new(0.0, Unit::Unitless)));
        }
        if let Some(q) = read_quantity(s) {
            return Ok(Operand::Num(q));
        }
        if let Some(inner) = self.eval_string_formula(s, env) {
            match inner {
                // A name that finds nothing: the string is not readable. A function this evaluator does not have is unknown, and so is the
                // number: the call is handed back as it is (the round: `IF("_ELLIPSE_THETA(1,2)",1,2)` is not 2).
                Value::Error(CellError::Ref) => {}
                u @ Value::Unsupported(_) => return Err(u),
                Value::Error(e) => return Err(Value::Error(e)),
                other => {
                    let mut deeper = env.clone();
                    deeper.string_depth += 1;
                    return self.number_of(&other, &deeper, unreadable);
                }
            }
        }
        match unreadable {
            Unreadable::Zero => Ok(Operand::Num(Quantity::new(0.0, Unit::Unitless))),
            Unreadable::Error | Unreadable::EmptyIsZero => Err(Value::Error(UNREADABLE_STRING_IN_ARITHMETIC)),
        }
    }

    /// `vDouble` for a function's argument: a currency loses its code, and the empty string is 0 (`Unreadable::EmptyIsZero`).
    pub(crate) fn read_double(&mut self, v: &Value, env: &Env) -> Result<Quantity, Value> {
        self.number_of(v, env, Unreadable::EmptyIsZero).map(Operand::quantity)
    }

    /// `vDouble` for the operators that are not `+ - * /` and a comparison: `^`, the unary signs and `%`. A string that cannot
    /// convert is an error, the empty string too.
    pub(crate) fn read_double_arith(&mut self, v: &Value, env: &Env) -> Result<Quantity, Value> {
        self.number_of(v, env, Unreadable::Error).map(Operand::quantity)
    }

    /// `vDouble` for a comparison: a string that cannot convert counts as 0.
    pub(crate) fn read_double_lenient(&mut self, v: &Value, env: &Env) -> Result<Quantity, Value> {
        self.number_of(v, env, Unreadable::Zero).map(Operand::quantity)
    }

    /// `vDoubleEx`: keeps the currency. The operands of `+ - * /`.
    pub(crate) fn read_operand(&mut self, v: &Value, env: &Env) -> Result<Operand, Value> {
        self.number_of(v, env, Unreadable::Error)
    }

    /// `vBoolean`: a number is false at exactly zero; a string is read as a formula
    /// (false where it gives false or zero), and one that cannot be read is false.
    pub(crate) fn read_bool(&mut self, v: &Value, env: &Env) -> Result<bool, Value> {
        match v {
            Value::Bool(b) => Ok(*b),
            Value::Number(q) => Ok(q.stored != 0.0),
            Value::Currency { amount, .. } => Ok(*amount != 0.0),
            Value::Text(s) => self.text_to_bool(s, env),
            // `unknown`: project rule, as an unreadable string.
            Value::Color(_) | Value::Point(_) | Value::Constructor(_) | Value::Sheet(_) => Ok(false),
            Value::Error(_) | Value::Unsupported(_) => Err(v.clone()),
        }
    }

    fn text_to_bool(&mut self, s: &str, env: &Env) -> Result<bool, Value> {
        // The length is charged first: trimming a megabyte of white space is a pass over it.
        if !self.work(s.len()) {
            return Err(Value::Error(LOOP_ERROR));
        }
        if s.trim().is_empty() {
            return Ok(false);
        }
        if let Some(q) = read_quantity(s) {
            return Ok(q.stored != 0.0);
        }
        match self.eval_string_formula(s, env) {
            None => Ok(false),
            Some(Value::Bool(b)) => Ok(b),
            Some(Value::Number(q)) => Ok(q.stored != 0.0),
            Some(Value::Currency { amount, .. }) => Ok(amount != 0.0),
            // A formula that fails comes back as its error, except a name that found nothing, which is no formula at all.
            Some(Value::Error(CellError::Ref)) => Ok(false),
            Some(e @ (Value::Error(_) | Value::Unsupported(_))) => Err(e),
            Some(_) => Ok(false),
        }
    }

    /// `vString`: a string stays; a number is its digits, with the unit's mark straight after when
    /// it has one, in the number shown in that unit; a Boolean is `TRUE` or `FALSE`.
    pub(crate) fn read_text(&mut self, v: &Value) -> Result<Arc<str>, Value> {
        match v {
            Value::Text(s) => Ok(Arc::clone(s)),
            Value::Number(q) => Ok(Arc::from(number_to_text(*q))),
            Value::Bool(b) => Ok(Arc::from(if *b { "TRUE" } else { "FALSE" })),
            // `vString` does not list a currency (`unknown`): project rule, the form `V` holds.
            // A colour joined to text reads as `RGB(r, g, b)`.
            Value::Color(c) => Ok(Arc::from(format!("RGB({}, {}, {})", c.r, c.g, c.b))),
            Value::Currency { .. } | Value::Point(_) | Value::Constructor(_) => Ok(Arc::from(encode(v).map(|e| e.v).unwrap_or_default())),
            Value::Sheet(_) => Err(Value::Error(BAD_ARGUMENT_ERROR)),
            Value::Error(_) | Value::Unsupported(_) => Err(v.clone()),
        }
    }
}

/// A number as text: plain digits for a number with no unit, a date as ISO text, and for one with a unit the number shown in it with
/// the unit's mark straight after (`25MM`); a multidimensional value shows its power (`3FT^2`).
pub(crate) fn number_to_text(q: Quantity) -> String {
    if q.unit == Unit::Date {
        return serial_to_iso(q.stored).unwrap_or_else(|| format_number(q.stored));
    }
    if q.unit == Unit::Unitless && q.dim() == 0 {
        return format_number(q.stored);
    }
    // The table: inches take `"`, feet `'`, feet and inches `5' 3"`, and any other unit its capital mark.
    if !q.is_multidimensional() {
        match q.unit {
            Unit::Inch | Unit::InchFine => return format!("{}\"", format_number(q.shown())),
            Unit::Foot => return format!("{}'", format_number(q.shown())),
            Unit::FeetInches => {
                let inches = q.stored;
                let feet = (inches.abs() / 12.0).trunc();
                let rest = inches.abs() - feet * 12.0;
                let sign = if inches < 0.0 { "-" } else { "" };
                return format!("{sign}{}' {}\"", format_number(feet), format_number(rest));
            }
            _ => {}
        }
    }
    let mark = q.unit.suffix().unwrap_or_else(|| q.unit.code());
    let shown = format_number(q.shown());
    if q.is_multidimensional() { format!("{shown}{mark}^{}", q.dim()) } else { format!("{shown}{mark}") }
}

/// `vSignedLong`: a whole number, fractions cut toward zero; beyond -2147483648 to 2147483647 is not valid.
pub(crate) fn signed_long(q: Quantity) -> Result<i64, Value> {
    let t = q.stored.trunc();
    if t.is_nan() || !(-2_147_483_648.0..=2_147_483_647.0).contains(&t) {
        return Err(Value::Error(BAD_ARGUMENT_ERROR));
    }
    Ok(t as i64)
}

/// `vSignedInt`: `vSignedLong` limited to -32768 to 32767.
pub(crate) fn signed_int(q: Quantity) -> Result<i64, Value> {
    let n = signed_long(q)?;
    if !(-32_768..=32_767).contains(&n) {
        return Err(Value::Error(BAD_ARGUMENT_ERROR));
    }
    Ok(n)
}

/// `vUnsignedInt`: a whole number that wraps modulo 65536. **Project rule (`unknown`):** a negative one wraps
/// the same way, as sixteen bits of its two's complement, so `BITNOT(-1)` is 0 and the result is always 0 to 65535.
pub(crate) fn unsigned_int(q: Quantity) -> Result<u16, Value> {
    let n = signed_long(q)?;
    Ok((n & 0xFFFF) as u16)
}

/// `vFloat`: `vDouble` limited to single precision; beyond it is not valid.
pub(crate) fn float_range(q: Quantity) -> Result<f64, Value> {
    if q.stored.is_nan() || q.stored.abs() > f64::from(f32::MAX) {
        return Err(Value::Error(BAD_ARGUMENT_ERROR));
    }
    Ok(q.stored)
}

/// An argument slot as a value: an empty slot, or one past the end, is not a value (`unknown`): project rule,
/// the error of a bad argument.
pub(crate) fn slot(slots: &[Option<Value>], i: usize) -> Result<&Value, Value> {
    slots.get(i).and_then(Option::as_ref).ok_or(Value::Error(BAD_ARGUMENT_ERROR))
}

impl Evaluator<'_, '_> {
    /// Argument `i` by `vDouble`.
    pub(crate) fn arg_double(&mut self, site: &crate::eval::Site<'_>, slots: &[Option<Value>], i: usize) -> Result<Quantity, Value> {
        let v = slot(slots, i)?;
        self.read_double(v, site.env)
    }

    /// Argument `i` by `vBoolean`.
    pub(crate) fn arg_bool(&mut self, site: &crate::eval::Site<'_>, slots: &[Option<Value>], i: usize) -> Result<bool, Value> {
        let v = slot(slots, i)?;
        self.read_bool(v, site.env)
    }

    /// Argument `i` by `vString`.
    pub(crate) fn arg_text(&mut self, slots: &[Option<Value>], i: usize) -> Result<Arc<str>, Value> {
        let v = slot(slots, i)?;
        self.read_text(v)
    }
}
