//! Cells and sheets: how a formula guards a cell, redirects a write, reaches another cell, asks what holds a shape, and calls a cell as a
//! function.
//!
//! **Recalculation only.** Most of these do their real work when someone edits. This is the half M2 builds: `GUARD` and its kin
//! give their argument, `SETATREF` reads its target and never evaluates its second argument, `DEPENDSON` is `FALSE`, and no function writes
//! a cell or changes a formula (`SETF` is 0 outside an action cell). The edit half, a write through `BOUND` and `SETATREF`
//! Is in `commit.rs`.
//!
//! **The lists, containers and callouts** read a shape's `Relationships` cell by walking its formula, not by evaluating it: a
//! `SUM` of `DEPENDSON` calls, each a code and the shapes after it by `Sheet.<id>!SheetRef()`, a lost one being `REF()`.

use std::collections::HashSet;
use std::sync::Arc;

use diacraft_doc::{CellAddr, FormulaSlot, Lookup, RowKey, SheetRef};
use diacraft_formula::Expr;

use crate::convert::{signed_long, slot};
use crate::eval::{Env, Evaluator, Site};
use crate::registry::{FnDef, MANY, eager, lazy};
use crate::rules::{BAD_ARGUMENT_ERROR, LOOP_ERROR, MAX_NESTED_EVALUATIONS};
use crate::value::{CellError, Value};

/// A shape's ties: each `DEPENDSON` call of its `Relationships` cell, in formula order: the code, and the shapes after it, `None` for a
/// lost one (`REF()`, or a name that finds nothing).
pub(crate) type Ties = Vec<(u32, Vec<Option<SheetRef>>)>;

impl Evaluator<'_, '_> {
    /// The ties of a shape: `None` when it has no `Relationships` cell with a formula of its own or a master's.
    pub(crate) fn ties_of(&mut self, sheet: &SheetRef) -> Option<Arc<Ties>> {
        if let Some(known) = self.ties.get(sheet) {
            return known.clone();
        }
        let ties = self.read_ties(sheet).map(Arc::new);
        self.ties.insert(sheet.clone(), ties.clone());
        ties
    }

    fn read_ties(&mut self, sheet: &SheetRef) -> Option<Ties> {
        let res = self.res.resolve(sheet, &CellAddr::direct("Relationships"))?;
        let Some(FormulaSlot::Local(formula)) = res.formula.cell.map(|c| &c.formula) else { return None };
        let Ok(expr) = &formula.ast else { return None };
        let env = Env::new(res.on.clone(), res.rebind.clone());
        let mut ties: Ties = Vec::new();
        let mut stack: Vec<&Expr> = vec![expr];
        while let Some(e) = stack.pop() {
            if !self.steps(1) {
                break;
            }
            match e {
                Expr::Call(c) if c.is("DEPENDSON") && c.qualifiers.is_empty() => {
                    let code = match c.args.first() {
                        Some(Some(Expr::Number(n))) => n.quantity().map(|q| q.stored),
                        _ => None,
                    };
                    let Some(code) = code.filter(|c| *c >= 0.0 && *c < 1e6) else { continue };
                    let mut targets = Vec::new();
                    for arg in c.args.iter().skip(1).flatten() {
                        match arg {
                            Expr::Call(t) if t.is("SHEETREF") => {
                                let target = match self.qualified(&t.qualifiers, &env) {
                                    Ok(Some(inner)) => Some(inner.sheet),
                                    Ok(None) => Some(env.sheet.clone()),
                                    Err(_) => None,
                                };
                                targets.push(target);
                            }
                            Expr::Call(t) if t.is("REF") => targets.push(None),
                            _ => {}
                        }
                    }
                    ties.push((code as u32, targets));
                }
                Expr::Call(c) => stack.extend(c.args.iter().rev().flatten()),
                Expr::Unary(_, x) | Expr::Percent(x) => stack.push(x),
                Expr::Binary(_, a, b) => {
                    stack.push(b);
                    stack.push(a);
                }
                _ => {}
            }
        }
        Some(ties)
    }

    /// The live targets under a code, in formula order. A lost target is no target.
    pub(crate) fn live_targets(&mut self, sheet: &SheetRef, code: u32) -> Vec<SheetRef> {
        let Some(ties) = self.ties_of(sheet) else { return Vec::new() };
        // The ties are walked (64 to a step) and each target under the code is copied, a step for each: a shape's `Relationships` cell may
        // name thousands, and each call copies them.
        let wanted: usize = ties.iter().filter(|(c, _)| *c == code).map(|(_, targets)| targets.iter().flatten().count()).sum();
        if !self.work(ties.len()) || !self.steps(wanted) {
            return Vec::new();
        }
        ties.iter().filter(|(c, _)| *c == code).flat_map(|(_, targets)| targets.iter().flatten().cloned()).collect()
    }

    fn has_code(&mut self, sheet: &SheetRef, code: u32) -> bool {
        self.ties_of(sheet).is_some_and(|t| t.iter().any(|(c, _)| *c == code))
    }

    /// A user cell's value as text: `User.msvStructureType`, `User.msvShapeCategories`.
    fn user_text(&mut self, sheet: &SheetRef, row: &str) -> Option<Arc<str>> {
        let read = self.read_cell(sheet, &CellAddr::in_row("User", None, RowKey::name(row), "Value"));
        match &read.view {
            Value::Text(t) => Some(Arc::clone(t)),
            _ => None,
        }
    }
}

fn bad() -> Value {
    Value::Error(BAD_ARGUMENT_ERROR)
}

fn sheet_value(s: &SheetRef) -> Value {
    Value::Sheet(Box::new(s.clone()))
}

fn sheetref(_: &mut Evaluator<'_, '_>, site: &Site<'_>, _: &[Option<Value>]) -> Value {
    sheet_value(&site.target.sheet)
}

fn first_target_or_ref(ev: &mut Evaluator<'_, '_>, sheet: &SheetRef, code: u32) -> Value {
    match ev.live_targets(sheet, code).first() {
        Some(t) => sheet_value(t),
        None => Value::Error(CellError::Ref),
    }
}

/// `LISTSHEETREF()`: the list holding the shape, code 5's target. A 1 is taken and any other number gives `#REF!`, as the
/// page's example passes one (project rule), and two slots are `#VALUE!` (the registry's arity).
fn listsheetref(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: &[Option<Value>]) -> Value {
    if let Some(Some(v)) = slots.first() {
        match ev.read_double(v, site.env) {
            Ok(q) if q.stored == 1.0 => {}
            Ok(_) => return Value::Error(CellError::Ref),
            Err(e) => return e,
        }
    }
    first_target_or_ref(ev, &site.target.sheet, 5)
}

fn calloutsheetref(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, _: &[Option<Value>]) -> Value {
    first_target_or_ref(ev, &site.target.sheet, 6)
}

fn listmembercount(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, _: &[Option<Value>]) -> Value {
    Value::num(ev.live_targets(&site.target.sheet, 2).len() as f64)
}

fn calloutcount(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, _: &[Option<Value>]) -> Value {
    Value::num(ev.live_targets(&site.target.sheet, 3).len() as f64)
}

/// `LISTORDER()`: the shape's place among the list's code 2 targets, counting from 1; -1 outside any list.
fn listorder(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, _: &[Option<Value>]) -> Value {
    let me = &site.target.sheet;
    let Some(list) = ev.live_targets(me, 5).into_iter().next() else { return Value::num(-1.0) };
    let members = ev.live_targets(&list, 2);
    match members.iter().position(|m| m == me) {
        Some(at) => Value::num((at + 1) as f64),
        None => Value::num(-1.0),
    }
}

/// `CONTAINERMEMBERCOUNT()`: the live targets under code 1; -1 for a shape that is no container.
fn containermembercount(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, _: &[Option<Value>]) -> Value {
    let me = site.target.sheet.clone();
    let typed = ev.user_text(&me, "msvStructureType").is_some_and(|t| &*t == "Container" || &*t == "List");
    if !typed && !ev.has_code(&me, 1) {
        return Value::num(-1.0);
    }
    Value::num(ev.live_targets(&me, 1).len() as f64)
}

/// `CONTAINERCOUNT()`: the containers holding the shape, those holding it at second hand too, each once.
fn containercount(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, _: &[Option<Value>]) -> Value {
    let me = site.target.sheet.clone();
    let mut seen: HashSet<SheetRef> = HashSet::new();
    let mut queue = ev.live_targets(&me, 4);
    while let Some(next) = queue.pop() {
        if !ev.steps(1) {
            return Value::Error(LOOP_ERROR);
        }
        if next == me || !seen.insert(next.clone()) {
            continue;
        }
        queue.extend(ev.live_targets(&next, 4));
    }
    Value::num(seen.len() as f64)
}

/// `CONTAINERSHEETREF([sheet,] index [, category])`: the container at place `index`, counted down from the frontmost (the latest in the
/// page part), among those whose `User.msvShapeCategories` holds `category`.
fn containersheetref(ev: &mut Evaluator<'_, '_>, site: &Site<'_>, slots: &[Option<Value>]) -> Value {
    let (shape, at) = match slots.first() {
        Some(Some(Value::Sheet(s))) => ((**s).clone(), 1),
        _ => (site.target.sheet.clone(), 0),
    };
    let index = match slot(slots, at).and_then(|v| ev.read_double(v, site.env)).and_then(signed_long) {
        Ok(i) => i,
        Err(e) => return e,
    };
    let category = match slots.get(at + 1) {
        Some(Some(v)) => match ev.read_text(v) {
            Ok(t) if !t.is_empty() => Some(t),
            Ok(_) => None,
            Err(e) => return e,
        },
        _ => None,
    };
    let mut containers = ev.live_targets(&shape, 4);
    if let Some(category) = category {
        // Each container's list of categories is read through, at 64 bytes a step.
        containers.retain(|c| {
            ev.user_text(c, "msvShapeCategories").is_some_and(|list| ev.work(list.len()) && list.split(';').any(|name| name == &*category))
        });
    }
    // Frontmost first: a later shape in the part is in front. The sort is a step for each container.
    if !ev.steps(containers.len()) {
        return Value::Error(LOOP_ERROR);
    }
    containers.sort_by(|a, b| match (a, b) {
        (SheetRef::Shape(x), SheetRef::Shape(y)) => y.path.cmp(&x.path),
        _ => std::cmp::Ordering::Equal,
    });
    match usize::try_from(index.saturating_sub(1)).ok().filter(|_| index >= 1).and_then(|i| containers.get(i)) {
        Some(c) => sheet_value(c),
        None => Value::Error(CellError::Ref),
    }
}

fn dependson(_: &mut Evaluator<'_, '_>, _: &Site<'_>, _: &[Option<Value>]) -> Value {
    // Its arguments were worked out (an error among them came back); the call adds a link and no value.
    Value::Bool(false)
}

/// `GUARD(x)`, `GETREF(cell)`, `GETVAL(cell)` and `SETATREFEVAL(x)` on recalculation: their argument, in its kind and unit.
fn identity(_: &mut Evaluator<'_, '_>, _: &Site<'_>, slots: &[Option<Value>]) -> Value {
    slot(slots, 0).cloned().unwrap_or_else(|e| e)
}

/// `SETATREFEXPR([x])`: `x`, or 0 with no argument.
fn setatrefexpr(_: &mut Evaluator<'_, '_>, _: &Site<'_>, slots: &[Option<Value>]) -> Value {
    match slots.first() {
        Some(Some(v)) => v.clone(),
        _ => Value::num(0.0),
    }
}

/// `SETATREF(target [, set_expression [, ignore]])` on recalculation: `target`'s value, or 0 when `ignore` is true, with the second
/// argument never worked out. A true `ignore` reads no target.
fn setatref(ev: &mut Evaluator<'_, '_>, site: &Site<'_>) -> Value {
    if let Some(ignore) = site.arg(2) {
        let v = ev.eval(ignore, site.env);
        match ev.read_bool(&v, site.env) {
            Ok(true) => return Value::num(0.0),
            Ok(false) => {}
            Err(e) => return e,
        }
    }
    match site.arg(0) {
        Some(target) => ev.eval(target, site.env),
        None => bad(),
    }
}

/// `SETF(target, value)` on recalculation: 0, writing nothing. A value that is a string
/// holding a formula with a syntax error is `#VALUE!`.
fn setf(ev: &mut Evaluator<'_, '_>, site: &Site<'_>) -> Value {
    if let Some(value) = site.arg(1) {
        match ev.eval(value, site.env) {
            Value::Text(t) if !ev.work(t.len()) => return Value::Error(LOOP_ERROR),
            Value::Text(t) if !t.trim().is_empty() && !diacraft_formula::parse(&t).is_parsed() => return Value::Error(CellError::Value),
            e @ (Value::Error(_) | Value::Unsupported(_)) => return e,
            _ => {}
        }
    }
    Value::num(0.0)
}

/// `FORMULAEXISTS(cell)`: whether the cell has a formula on its chain; a value with no `F` has one, `No Formula` has not,
/// and a cell that is not there has none.
fn formulaexists(ev: &mut Evaluator<'_, '_>, site: &Site<'_>) -> Value {
    let Some(Expr::Ref(r)) = site.arg(0) else {
        // Not a reference: a literal such as `FALSE`.
        return Value::Bool(false);
    };
    let (sheet, addr) = match ev.locate(r, site.env) {
        Ok(x) => x,
        Err(v) => return v,
    };
    let Some(res) = ev.res.resolve(&sheet, &addr) else { return Value::Bool(false) };
    Value::Bool(match res.formula.cell.map(|c| &c.formula) {
        Some(FormulaSlot::Local(_)) | Some(FormulaSlot::Absent) => true,
        Some(FormulaSlot::NoFormula) | Some(FormulaSlot::Inherit) | None => false,
    })
}

/// `LOCALFORMULAEXISTS(cell)`: whether the cell holds a formula of its own; `Inh` and an absent cell do not, and a local value with no `F`
/// does.
fn localformulaexists(ev: &mut Evaluator<'_, '_>, site: &Site<'_>) -> Value {
    let Some(Expr::Ref(r)) = site.arg(0) else { return Value::Bool(false) };
    let (sheet, addr) = match ev.locate(r, site.env) {
        Ok(x) => x,
        Err(v) => return v,
    };
    let Some(target) = ev.res.sheet(&sheet) else { return Value::Bool(false) };
    // Through the resolver, which finds a row of a large section and a section of a sheet that has many by an index.
    Value::Bool(match ev.res.lookup(target, &addr) {
        Lookup::Found(cell) => match &cell.formula {
            FormulaSlot::Local(_) => true,
            FormulaSlot::Absent => cell.value.is_some(),
            FormulaSlot::NoFormula | FormulaSlot::Inherit => false,
        },
        Lookup::Missing | Lookup::Deleted => false,
    })
}

/// `USERUI(state, default, user)`: `default` for 0 and `user` otherwise, working out only the one chosen.
fn userui(ev: &mut Evaluator<'_, '_>, site: &Site<'_>) -> Value {
    let Some(state) = site.arg(0) else { return bad() };
    let v = ev.eval(state, site.env);
    let user = match ev.read_bool(&v, site.env) {
        Ok(b) => b,
        Err(e) => return e,
    };
    match site.arg(if user { 2 } else { 1 }) {
        Some(e) => ev.eval(e, site.env),
        None => bad(),
    }
}

/// `ARG(name [, default])`: what the `EVALCELL` that runs the formula bound to `name`, else `default`, else 0 with no error.
fn arg(ev: &mut Evaluator<'_, '_>, site: &Site<'_>) -> Value {
    let Some(name_expr) = site.arg(0) else { return bad() };
    let name_value = ev.eval(name_expr, site.env);
    let name = match ev.read_text(&name_value) {
        Ok(n) => n,
        Err(e) => return e,
    };
    // Names match exactly (project rule). The bindings are searched in order, 64 to a step: an `EVALCELL` may bind thousands.
    if !ev.work(site.env.bindings.as_ref().map_or(0, |b| b.len())) {
        return Value::Error(LOOP_ERROR);
    }
    if let Some(bound) = site.env.bindings.as_ref().and_then(|b| b.iter().find(|(n, _)| n.as_str() == &*name)) {
        return bound.1.clone();
    }
    match site.arg(1) {
        Some(default) => ev.eval(default, site.env),
        None => Value::num(0.0),
    }
}

/// `EVALCELL(cell [, name1, value1, ...])`: the named cell's formula worked out afresh with the pairs as `ARG`'s bindings, values read where
/// the call sits and references in the formula read on the named cell's sheet; the cell's own value is neither used nor changed
/// A loop stops at [`MAX_NESTED_EVALUATIONS`] with `#VALUE!`.
fn evalcell(ev: &mut Evaluator<'_, '_>, site: &Site<'_>) -> Value {
    if site.len().is_multiple_of(2) {
        // A name with no value.
        return bad();
    }
    let Some(Expr::Ref(target)) = site.arg(0) else { return bad() };
    let (sheet, addr) = match ev.locate(target, site.env) {
        Ok(x) => x,
        Err(v) => return v,
    };
    let mut pairs: Vec<(String, Value)> = Vec::new();
    let mut i = 1;
    while i + 1 < site.len() {
        let (Some(name_expr), Some(value_expr)) = (site.arg(i), site.arg(i + 1)) else { return bad() };
        let name_value = ev.eval(name_expr, site.env);
        let name = match ev.read_text(&name_value) {
            Ok(n) => n,
            Err(e) => return e,
        };
        let value = ev.eval(value_expr, site.env);
        if value.is_failure() {
            return value;
        }
        if !ev.work(name.len()) {
            return Value::Error(LOOP_ERROR);
        }
        pairs.push((name.to_string(), value));
        i += 2;
    }
    if site.env.nested + 1 > MAX_NESTED_EVALUATIONS {
        return Value::Error(LOOP_ERROR);
    }
    let Some(res) = ev.res.resolve(&sheet, &addr) else { return Value::Error(CellError::Ref) };
    match res.formula.cell.map(|c| &c.formula) {
        Some(FormulaSlot::Local(formula)) => match &formula.ast {
            Ok(expr) => {
                let mut inner = Env::new(res.on.clone(), res.rebind.clone());
                inner.nested = site.env.nested + 1;
                inner.bindings = Some(Arc::new(pairs));
                ev.eval(expr, &inner)
            }
            Err(_) => ev.read_cell(&sheet, &addr).view.clone(),
        },
        Some(_) => ev.read_cell(&sheet, &addr).view.clone(),
        None => Value::Error(CellError::Ref),
    }
}

/// `EVALTEXT(sheet!TheText)`: the shape's text worked out as a formula on that sheet; empty text is 0 and text that cannot be
/// read is `#VALUE!` (project rule).
fn evaltext(ev: &mut Evaluator<'_, '_>, site: &Site<'_>) -> Value {
    let sheet = match site.arg(0) {
        Some(Expr::Ref(r)) => match ev.locate(r, site.env) {
            Ok((sheet, _)) => sheet,
            Err(v) => return v,
        },
        Some(_) => return bad(),
        None => site.target.sheet.clone(),
    };
    let SheetRef::Shape(at) = &sheet else { return bad() };
    // A text with a field in it, and one inherited from the master, are not worked out here (see `fn_text::own_text`).
    let text = match crate::fn_text::own_text(ev, at, false, "EVALTEXT") {
        Ok(t) => t,
        Err(v) => return v,
    };
    if text.trim().is_empty() {
        return Value::num(0.0);
    }
    if site.env.nested + 1 > MAX_NESTED_EVALUATIONS {
        return Value::Error(LOOP_ERROR);
    }
    let mut inner = Env::new(sheet, None);
    inner.nested = site.env.nested + 1;
    match ev.eval_string_formula(&text, &inner) {
        None | Some(Value::Error(CellError::Ref)) => Value::Error(CellError::Value),
        Some(v) => v,
    }
}

/// `BLOB(text)`: the formula is kept and an evaluation gives `#VALUE!` (project rule): a stored `V` stands.
fn blob(_: &mut Evaluator<'_, '_>, _: &Site<'_>, _: &[Option<Value>]) -> Value {
    Value::Error(CellError::Value)
}

pub(crate) static DEFS: &[FnDef] = &[
    lazy("ARG", 1, 2, arg),
    eager("BLOB", 1, 1, blob),
    eager("CALLOUTCOUNT", 0, 0, calloutcount),
    eager("CALLOUTTARGETREF", 0, 0, calloutsheetref),
    eager("CONTAINERCOUNT", 0, 0, containercount),
    eager("CONTAINERMEMBERCOUNT", 0, 0, containermembercount),
    eager("CONTAINERSHEETREF", 1, 3, containersheetref),
    eager("DEPENDSON", 1, MANY, dependson),
    lazy("EVALCELL", 1, MANY, evalcell),
    lazy("EVALTEXT", 0, 1, evaltext),
    lazy("FORMULAEXISTS", 1, 1, formulaexists),
    eager("GETREF", 1, 1, identity),
    eager("GETVAL", 1, 1, identity),
    eager("GUARD", 1, 1, identity),
    eager("LISTMEMBERCOUNT", 0, 0, listmembercount),
    eager("LISTORDER", 0, 0, listorder),
    eager("LISTSHEETREF", 0, 1, listsheetref),
    lazy("LOCALFORMULAEXISTS", 1, 1, localformulaexists),
    lazy("SETATREF", 1, 3, setatref),
    eager("SETATREFEVAL", 1, 1, identity),
    eager("SETATREFEXPR", 0, 1, setatrefexpr),
    lazy("SETF", 2, 4, setf),
    eager("SHEETREF", 0, 0, sheetref),
    lazy("USERUI", 3, 3, userui),
];
