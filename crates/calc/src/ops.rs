//! The arithmetic of numbers with units, dimensions and currencies: `+ - * /`, `^`, the comparisons and the signs.
//!
//! Everything here works on operands already converted (the evaluator does `vDouble` and `vDoubleEx`), and on stored internal values
//! The unit of a result decides only how its stored number shows. Each function is the steps of its
//! section, in order, first match wins.

use diacraft_formula::{BinaryOp, Currency};
use diacraft_units::{Quantity, Unit};

use crate::rules::COMPARE_TOLERANCE;
use crate::value::{CellError, Value};

/// An operand of `vDoubleEx`: a number, or a currency with its code.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Operand {
    Num(Quantity),
    Cy { amount: f64, currency: Option<&'static Currency> },
}

impl Operand {
    fn stored(self) -> f64 {
        match self {
            Operand::Num(q) => q.stored,
            Operand::Cy { amount, .. } => amount,
        }
    }

    /// The number as `vDouble` has it: a currency loses its code and is a plain number.
    pub(crate) fn quantity(self) -> Quantity {
        match self {
            Operand::Num(q) => q,
            Operand::Cy { amount, .. } => Quantity::new(amount, Unit::Unitless),
        }
    }
}

fn has_unit(q: Quantity) -> bool {
    q.unit != Unit::Unitless
}

/// A result's number, unit and dimension, finished: a dimension outside -128 to 127 is `#DIM!`, a dimension of 0 is a plain
/// number, an acre or a hectare at a dimension other than 2 is a multidimensional value in `DL`, and a number that is not a
/// number is `#NUM!`.
pub(crate) fn finish(stored: f64, unit: Unit, dim: i32) -> Value {
    if stored.is_nan() {
        return Value::Error(CellError::Num);
    }
    let Ok(dim) = i8::try_from(dim) else { return Value::Error(CellError::Dim) };
    let unit = if matches!(unit, Unit::Acre | Unit::Hectare) && dim != 2 { Unit::DrawingDefault } else { unit };
    Value::Number(Quantity::with_dim(stored, unit, dim))
}

/// The currency rows, which repeat: `Some(Ok(code))` when a currency is the result, `Some(Err(..))` for
/// two that do not fit, `None` when no operand is a currency.
fn currency_rule(a: Operand, b: Operand) -> Option<Result<Option<&'static Currency>, CellError>> {
    match (a, b) {
        (Operand::Cy { currency: ca, .. }, Operand::Cy { currency: cb, .. }) => {
            let id_of = |c: Option<&'static Currency>| c.map(|c| c.id);
            Some(if ca == cb || id_of(cb) == Some(1) {
                Ok(ca)
            } else if id_of(ca) == Some(1) {
                Ok(cb)
            } else {
                Err(CellError::Value)
            })
        }
        (Operand::Cy { currency, .. }, _) | (_, Operand::Cy { currency, .. }) => Some(Ok(currency)),
        _ => None,
    }
}

fn currency_result(amount: f64, rule: Result<Option<&'static Currency>, CellError>) -> Value {
    match rule {
        Ok(currency) if amount.is_nan() => {
            let _ = currency;
            Value::Error(CellError::Num)
        }
        Ok(currency) => Value::Currency { amount, currency },
        Err(e) => Value::Error(e),
    }
}

/// `a + b` and `a - b`.
pub(crate) fn add_sub(a: Operand, b: Operand, subtract: bool) -> Value {
    let stored = if subtract { a.stored() - b.stored() } else { a.stored() + b.stored() };
    if let Some(rule) = currency_rule(a, b) {
        return currency_result(stored, rule);
    }
    let (qa, qb) = (a.quantity(), b.quantity());
    let (da, db) = (i32::from(qa.dim()), i32::from(qb.dim()));
    if subtract {
        if qa.unit == Unit::Date && qb.unit == Unit::Date {
            // Both dates: a duration in DE, dimension 1.
            return finish(stored, Unit::DurationDefault, 1);
        }
    } else if qa.unit == Unit::Date || qb.unit == Unit::Date {
        return finish(stored, Unit::Date, 1);
    }
    if qa.unit == Unit::DrawingDefault && has_unit(qb) {
        // The shared dimension survives where MS-VSDX's step would set 1.
        let dim = if da == db { da } else { 1 };
        return finish(stored, qb.unit, dim);
    }
    if has_unit(qa) {
        let dim = if db == 0 || db == da { da } else { 1 };
        return finish(stored, qa.unit, dim);
    }
    if has_unit(qb) {
        let dim = if da == 0 || da == db { db } else { 1 };
        return finish(stored, qb.unit, dim);
    }
    finish(stored, Unit::Unitless, 0)
}

/// `a * b`.
pub(crate) fn mul(a: Operand, b: Operand) -> Value {
    let stored = a.stored() * b.stored();
    if let Some(rule) = currency_rule(a, b) {
        return currency_result(stored, rule);
    }
    let (qa, qb) = (a.quantity(), b.quantity());
    let (da, db) = (i32::from(qa.dim()), i32::from(qb.dim()));
    let (pa, pb) = (qa.unit == Unit::Percent, qb.unit == Unit::Percent);
    if pa && pb {
        return finish(stored, Unit::Percent, 1);
    }
    if pa {
        return finish(stored, Unit::Percent, db);
    }
    if pb && has_unit(qa) {
        return finish(stored, qa.unit, da);
    }
    if pb {
        return finish(stored, Unit::Unitless, da);
    }
    if has_unit(qa) {
        return finish(stored, qa.unit, da + db);
    }
    if has_unit(qb) {
        return finish(stored, qb.unit, da + db);
    }
    finish(stored, Unit::Unitless, 0)
}

/// `a / b`. `call` is true for `_DIV(a, b)`, which differs from the operator in one row: a number with a unit as the divisor
/// of a bare one has dimension minus its own.
pub(crate) fn div(a: Operand, b: Operand, call: bool) -> Value {
    if b.stored() == 0.0 {
        // Exactly zero, before anything else, with no tolerance, in any unit and for a currency too.
        return Value::Error(CellError::DivZero);
    }
    let stored = a.stored() / b.stored();
    if let Some(rule) = currency_rule(a, b) {
        return currency_result(stored, rule);
    }
    let (qa, qb) = (a.quantity(), b.quantity());
    let (da, db) = (i32::from(qa.dim()), i32::from(qb.dim()));
    let (pa, pb) = (qa.unit == Unit::Percent, qb.unit == Unit::Percent);
    if has_unit(qa) && !has_unit(qb) {
        return finish(stored, qa.unit, da);
    }
    if has_unit(qb) && !has_unit(qa) {
        return finish(stored, qb.unit, if call { -db } else { db });
    }
    if pa && pb {
        return finish(stored, Unit::Unitless, 0);
    }
    if pa && has_unit(qb) {
        // A negative dimension with no unit to name.
        return finish(stored, Unit::Unitless, -db);
    }
    if pb && has_unit(qa) {
        // A length over a percent loses its unit.
        return finish(stored, Unit::Unitless, 0);
    }
    let unit = if has_unit(qa) { qa.unit } else { qb.unit };
    finish(stored, unit, da - db)
}

/// `POW(base, exponent)` and `base ^ exponent`.
pub(crate) fn pow(base: Quantity, exponent: Quantity) -> Value {
    let (b, e) = (base.stored, exponent.stored);
    if b == 0.0 {
        return if e == 0.0 {
            Value::Error(CellError::Num)
        } else if e < 0.0 {
            Value::Error(CellError::DivZero)
        } else {
            Value::num(0.0)
        };
    }
    if b < 0.0 && e.fract() != 0.0 {
        return Value::Error(CellError::Dim);
    }
    let value = b.powf(e);
    if value.is_nan() {
        return Value::Error(CellError::Num);
    }
    if base.unit == Unit::Percent {
        return finish(value, Unit::Percent, 1);
    }
    let dim = f64::from(base.dim()) * e;
    if dim.fract() != 0.0 || !(-128.0..=127.0).contains(&dim) {
        return Value::Error(CellError::Dim);
    }
    finish(value, base.unit, dim as i32)
}

/// The symbol and exact comparisons on two stored numbers (and the exact family).
///
/// **Two infinities of one sign are equal** (our ruling: leaves infinity open, so the comparisons follow IEEE for an infinite number): `=`,
/// `>=` and `<=` hold and `<>`, `>` and `<` do not. The gap `inf - inf` is NaN, which made every symbol comparison of two equal infinities false, `<>` too,
/// so `(1E308*10) = (1E308*10)` was FALSE. An infinity against a number, or against the other infinity, is the plain order, which the gap gives
/// (`inf - 5` is `inf`). Two finite numbers equal to the last bit are as before (their gap is 0), and **NaN, which no cell holds (`finish` makes `#NUM!` of
/// one), is as before: every symbol comparison of it is false, `<>` too**, as its gap is NaN.
pub(crate) fn compare(op: BinaryOp, a: f64, b: f64) -> bool {
    let t = COMPARE_TOLERANCE;
    let same = a == b;
    match op {
        BinaryOp::Eq => same || (a - b).abs() <= t,
        BinaryOp::Ne => !same && (a - b).abs() > t,
        BinaryOp::Ge => same || a - b >= -t,
        BinaryOp::Gt => a - b > t,
        BinaryOp::Le => same || a - b <= t,
        BinaryOp::Lt => b - a > t,
        BinaryOp::ExactEq => a == b,
        BinaryOp::ExactNe => a != b,
        BinaryOp::ExactGe => a >= b,
        BinaryOp::ExactGt => a > b,
        BinaryOp::ExactLe => a <= b,
        BinaryOp::ExactLt => a < b,
        BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Pow | BinaryOp::Cat => false,
    }
}

/// `- a` and `+ a`: the stored number with its unit and dimension, negated or not.
pub(crate) fn sign(q: Quantity, negate: bool) -> Value {
    finish(if negate { -q.stored } else { q.stored }, q.unit, i32::from(q.dim()))
}

/// `a %`: the stored value over 100, as a percent.
pub(crate) fn percent(q: Quantity) -> Value {
    finish(q.stored / 100.0, Unit::Percent, 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SYMBOL_OPS: [BinaryOp; 6] = [BinaryOp::Eq, BinaryOp::Ne, BinaryOp::Ge, BinaryOp::Gt, BinaryOp::Le, BinaryOp::Lt];

    /// The symbol comparisons follow IEEE whenever a number is infinite: an infinity equals itself and is neither above nor below itself,
    /// and the other cases (an infinity against a number, or against the other one) are the plain order. The gap `inf - inf` is NaN, which made
    /// `=`, `<>`, `>=` and `<=` of two equal infinities all false.
    #[test]
    fn an_infinite_number_compares_as_ieee() {
        let numbers = [f64::NEG_INFINITY, f64::MIN, -1.0, -0.0, 0.0, 1e-300, 1.0, f64::MAX, f64::INFINITY];
        for a in numbers {
            for b in numbers {
                if !(a.is_infinite() || b.is_infinite()) {
                    continue;
                }
                let ieee = [a == b, a != b, a >= b, a > b, a <= b, a < b];
                for (op, want) in SYMBOL_OPS.into_iter().zip(ieee) {
                    assert_eq!(compare(op, a, b), want, "{a} {op:?} {b}");
                }
            }
        }
    }

    /// A finite number is as before: the tolerance of 1E-9 (a number equal to itself, with it, is the same answer as the gap of zero gave).
    #[test]
    fn two_finite_numbers_keep_the_tolerance() {
        assert!(compare(BinaryOp::Eq, 3.0, 3.0 + 5e-10));
        assert!(!compare(BinaryOp::Eq, 3.0, 3.0 + 2e-9));
        assert!(compare(BinaryOp::Ge, 3.0, 3.0 + 5e-10) && compare(BinaryOp::Le, 3.0 + 5e-10, 3.0));
        assert!(!compare(BinaryOp::Gt, 3.0 + 5e-10, 3.0) && !compare(BinaryOp::Lt, 3.0, 3.0 + 5e-10));
        for x in [0.0, -0.0, 1.0, -7.5, f64::MAX, f64::MIN, 1e-320] {
            assert_eq!(SYMBOL_OPS.map(|op| compare(op, x, x)), [true, false, true, false, true, false], "{x}");
        }
        // A pair whose gap overflows is far apart, not equal.
        assert!(!compare(BinaryOp::Eq, f64::MAX, f64::MIN) && compare(BinaryOp::Ne, f64::MAX, f64::MIN));
        assert!(compare(BinaryOp::Gt, f64::MAX, f64::MIN) && compare(BinaryOp::Lt, f64::MIN, f64::MAX));
        assert!(!compare(BinaryOp::Le, f64::MAX, f64::MIN) && !compare(BinaryOp::Ge, f64::MIN, f64::MAX));
    }

    /// NaN, which no cell holds (`finish` makes `#NUM!` of one), is unchanged: every symbol comparison is false, `<>` too, because its gap is NaN.
    #[test]
    fn nan_compares_false_in_every_symbol_comparison_as_before() {
        for other in [f64::NAN, 0.0, 1.0, f64::INFINITY, f64::NEG_INFINITY] {
            for (a, b) in [(f64::NAN, other), (other, f64::NAN)] {
                assert_eq!(SYMBOL_OPS.map(|op| compare(op, a, b)), [false; 6], "{a} against {b}");
            }
        }
    }
}
