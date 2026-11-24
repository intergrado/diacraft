//! The operators spelled as calls: `_ADD(a, b)` for `a + b`, `_FGT(a, b)` for `a > b`, `_UMINUS(a)`, `_PCT(a)`.
//!
//! A call and its operator mean the same, with one exception: `_DIV` gives a bare number over a number with a unit the dimension minus
//! the divisor's, where `/` gives plus. Calls are taken in any case, and a call with the wrong number of arguments or an
//! empty slot is an error (project rule `#VALUE!`).

use diacraft_formula::{BinaryOp, UnaryOp};

use crate::convert::slot;
use crate::eval::{Evaluator, Site};
use crate::registry::{FnDef, eager};
use crate::value::Value;

fn binary(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: &[Option<Value>], op: BinaryOp, call: bool) -> Value {
    let (a, b) = match (slot(slots, 0), slot(slots, 1)) {
        (Ok(a), Ok(b)) => (a.clone(), b.clone()),
        (Err(e), _) | (_, Err(e)) => return e,
    };
    ev.binary(op, a, b, site.env, call)
}

macro_rules! binary_calls {
    ($($f:ident => $op:expr, $call:expr;)*) => {
        $(fn $f(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: &[Option<Value>]) -> Value {
            binary(ev, site, slots, $op, $call)
        })*
    };
}

binary_calls! {
    add => BinaryOp::Add, false;
    sub => BinaryOp::Sub, false;
    mul => BinaryOp::Mul, false;
    div => BinaryOp::Div, true;
    cat => BinaryOp::Cat, false;
    flt => BinaryOp::Lt, false;
    fle => BinaryOp::Le, false;
    fgt => BinaryOp::Gt, false;
    fge => BinaryOp::Ge, false;
    feq => BinaryOp::Eq, false;
    fne => BinaryOp::Ne, false;
    elt => BinaryOp::ExactLt, false;
    ele => BinaryOp::ExactLe, false;
    egt => BinaryOp::ExactGt, false;
    ege => BinaryOp::ExactGe, false;
    eeq => BinaryOp::ExactEq, false;
    ene => BinaryOp::ExactNe, false;
}

fn uminus(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: &[Option<Value>]) -> Value {
    match slot(slots, 0) {
        Ok(v) => ev.unary(UnaryOp::Minus, v.clone(), site.env),
        Err(e) => e,
    }
}

fn uplus(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: &[Option<Value>]) -> Value {
    match slot(slots, 0) {
        Ok(v) => ev.unary(UnaryOp::Plus, v.clone(), site.env),
        Err(e) => e,
    }
}

fn pct(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: &[Option<Value>]) -> Value {
    match slot(slots, 0) {
        Ok(v) => ev.percent(v.clone(), site.env),
        Err(e) => e,
    }
}

pub(crate) static DEFS: &[FnDef] = &[
    eager("_ADD", 2, 2, add),
    eager("_CAT", 2, 2, cat),
    eager("_DIV", 2, 2, div),
    eager("_EEQ", 2, 2, eeq),
    eager("_EGE", 2, 2, ege),
    eager("_EGT", 2, 2, egt),
    eager("_ELE", 2, 2, ele),
    eager("_ELT", 2, 2, elt),
    eager("_ENE", 2, 2, ene),
    eager("_FEQ", 2, 2, feq),
    eager("_FGE", 2, 2, fge),
    eager("_FGT", 2, 2, fgt),
    eager("_FLE", 2, 2, fle),
    eager("_FLT", 2, 2, flt),
    eager("_FNE", 2, 2, fne),
    eager("_MUL", 2, 2, mul),
    eager("_PCT", 1, 1, pct),
    eager("_SUB", 2, 2, sub),
    eager("_UMINUS", 1, 1, uminus),
    eager("_UPLUS", 1, 1, uplus),
];
