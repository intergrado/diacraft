//! Logic and errors: `IF`, `AND`, `OR`, `NOT`, `IFERROR`, the four `ISERR` tests, `NA` and `REF`.
//!
//! This batch decides which errors escape a formula. `IF` works out only the branch it takes (so no side effect of the other one
//! runs); `AND` and `OR` work out every argument, so an error after a deciding one still comes back; the
//! tests and `IFERROR` see the error of a cell read through a bare reference, where anything else sees the cell's last good value.

use diacraft_formula::Expr;

use crate::convert::slot;
use crate::eval::{Env, Evaluator, Site};
use crate::registry::{FnDef, MANY, eager, lazy};
use crate::rules::{BAD_ARGUMENT_ERROR, LOOP_ERROR, MISSING_ROW_ERROR};
use crate::value::{CellError, Value};

impl Evaluator<'_, '_> {
    /// An argument as an error-testing function sees it: a bare reference to a failing cell is that cell's error, and
    /// any other expression is its value, a failing cell inside it reading at its last good value.
    pub(crate) fn eval_seeing_errors(&mut self, e: &Expr, env: &Env) -> Value {
        if let Expr::Ref(r) = e {
            if !self.steps(1) {
                return Value::Error(LOOP_ERROR);
            }
            let (sheet, addr) = match self.locate(r, env) {
                Ok(x) => x,
                Err(v) => return v,
            };
            let read = self.read_cell(&sheet, &addr);
            if !read.found && addr.row.is_some() {
                self.note_unholdable(&addr);
                return Value::Error(MISSING_ROW_ERROR);
            }
            if let Some(err) = read.error {
                return Value::Error(err);
            }
            return read.view.clone();
        }
        self.eval(e, env)
    }
}

fn iff(ev: &mut Evaluator<'_, '_>, site: &Site<'_>) -> Value {
    let Some(test_expr) = site.arg(0) else { return Value::Error(BAD_ARGUMENT_ERROR) };
    let test = ev.eval(test_expr, site.env);
    let truth = match ev.read_bool(&test, site.env) {
        Ok(b) => b,
        Err(e) => return e,
    };
    // An empty `then` is TRUE and an empty `else` FALSE (MS-VSDX's grammar).
    match (truth, site.arg(if truth { 1 } else { 2 })) {
        (_, Some(branch)) => ev.eval(branch, site.env),
        (t, None) => Value::Bool(t),
    }
}

/// `AND` and `OR`: every argument converts by `vBoolean`, an empty slot is left out, and the leftmost error comes back.
fn fold(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: &[Option<Value>], is_and: bool) -> Value {
    let mut result = is_and;
    let mut any = false;
    for v in slots.iter().flatten() {
        match ev.read_bool(v, site.env) {
            Ok(b) => {
                any = true;
                if is_and {
                    result &= b;
                } else {
                    result |= b;
                }
            }
            Err(e) => return e,
        }
    }
    if !any {
        // Nothing but empty slots: no source says. Project rule: the error of a bad argument.
        return Value::Error(BAD_ARGUMENT_ERROR);
    }
    Value::Bool(result)
}

fn and(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: &[Option<Value>]) -> Value {
    fold(ev, site, slots, true)
}

fn or(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: &[Option<Value>]) -> Value {
    fold(ev, site, slots, false)
}

fn not(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: &[Option<Value>]) -> Value {
    match slot(slots, 0) {
        Ok(v) => match ev.read_bool(v, site.env) {
            Ok(b) => Value::Bool(!b),
            Err(e) => e,
        },
        Err(e) => e,
    }
}

fn iferror(ev: &mut Evaluator<'_, '_>, site: &Site<'_>) -> Value {
    let Some(value_expr) = site.arg(0) else { return Value::Error(BAD_ARGUMENT_ERROR) };
    let value = ev.eval_seeing_errors(value_expr, site.env);
    match value {
        // Every class of error counts; the alternate is not looked at while the value is healthy.
        Value::Error(_) => match site.arg(1) {
            Some(alternate) => ev.eval(alternate, site.env),
            // An empty `alternate` is FALSE.
            None => Value::Bool(false),
        },
        other => other,
    }
}

/// The four tests (the table): `matches` says which errors answer TRUE. They never give an error back.
fn test_error(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, matches: fn(CellError) -> bool) -> Value {
    let Some(e) = site.arg(0) else { return Value::Error(BAD_ARGUMENT_ERROR) };
    match ev.eval_seeing_errors(e, site.env) {
        Value::Error(err) => Value::Bool(matches(err)),
        // The value is unknown, so the answer is.
        u @ Value::Unsupported(_) => u,
        _ => Value::Bool(false),
    }
}

fn iserror(ev: &mut Evaluator<'_, '_>, site: &Site<'_>) -> Value {
    test_error(ev, site, |_| true)
}

fn iserr(ev: &mut Evaluator<'_, '_>, site: &Site<'_>) -> Value {
    test_error(ev, site, |e| e != CellError::Na)
}

fn iserrna(ev: &mut Evaluator<'_, '_>, site: &Site<'_>) -> Value {
    test_error(ev, site, |e| e == CellError::Na)
}

fn iserrvalue(ev: &mut Evaluator<'_, '_>, site: &Site<'_>) -> Value {
    test_error(ev, site, |e| e == CellError::Value)
}

fn na(_: &mut Evaluator<'_, '_>, _: &Site<'_>, _: &[Option<Value>]) -> Value {
    Value::Error(CellError::Na)
}

/// `REF()`: `#REF!`, and a note that the formula being worked out reached one, so that the oracle can tell a formula that can only fail
/// from one whose lookup failed.
fn ref_error(ev: &mut Evaluator<'_, '_>, _: &Site<'_>, _: &[Option<Value>]) -> Value {
    ev.ctx.must_fail_touched = true;
    Value::Error(CellError::Ref)
}

pub(crate) static DEFS: &[FnDef] = &[
    eager("AND", 1, MANY, and),
    lazy("IF", 3, 3, iff),
    lazy("IFERROR", 2, 2, iferror),
    lazy("ISERR", 1, 1, iserr),
    lazy("ISERRNA", 1, 1, iserrna),
    lazy("ISERROR", 1, 1, iserror),
    lazy("ISERRVALUE", 1, 1, iserrvalue),
    eager("NA", 0, 0, na),
    eager("NOT", 1, 1, not),
    eager("OR", 1, MANY, or),
    eager("REF", 0, 0, ref_error),
];
