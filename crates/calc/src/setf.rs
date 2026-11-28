//! `SETF`: the call that writes a formula into another cell.
//!
//! **A `SETF` writes when its cell is worked out, as the pages say** (2026-11-17: documented behaviour wins over earlier rulings of
//! ours; it takes this over, which had a recalculation write nothing). The old reading is kept as a setting, [`SetfMode::Inert`].
//!
//! * **The call** is 0, and asks for the write it names: the cell it names and the formula it gives. The target is `GETREF(cell)` or a string that names
//!   a cell; anything else is an error (`#VALUE!`), a cell found nowhere is `#REF!`, and a formula that does not parse is `#VALUE!`
//!   and leaves the target as it was. The formula is the text of the second argument's result, a number printed in its unit, and a blank one writes nothing.
//! * **A call's write is asked for by the cell that holds it, worked out as itself**. The call keeps its write with the result of the cell it is
//!   worked out in (`CellEval::setf`: the cell's own formula's calls, and those an `EVALCELL` or a string read as a formula in it reaches), and the write is queued
//!   by [`ask`] when that cell is worked out *as itself* ([`Evaluator::evaluate_cell`]): because an input it reads changed, because a pass works every cell out, or
//!   because a dynamic call (`EVALCELL`) works the formula out. A formula that reads the cell's value (0) works the cell out in place for that value and asks for
//!   nothing, so a cell worked out for some other input (`Go + Width` after `Width` changed) does not make `Go` write over a person's value at its target. A cell
//!   that is dirty is worked out once, in place or as itself, and asks once however many cells read it or times it is asked for in one pass.
//! * **The commit makes the writes** ([`settle`]), after the cells it worked out have all been worked out (the queue is performed when the evaluation
//!   ends), in the order the calls were reached, as writes of origin [`WriteOrigin::Setf`] (the formula becomes the cell's own, and no guard stops it). Of
//!   the calls that name one cell the last wins, and a call whose target holds the text already writes nothing, which is how a `SETF` that is worked out again by
//!   the change it made comes to rest. The cells that read what was written are then worked out again, in the same commit, and so on.
//! * **The loop and work budgets.** At most [`MAX_SETF_WRITES`] formulas are written in one commit and at most [`MAX_SETF_ROUNDS`] rounds of working out again are
//!   made, so a `SETF` that sets itself off (a counter, two cells that write each other, a chain of cells that each write the next) ends there, and every
//!   round is charged to the commit's own [`Budget`]; a call past either cap, and one past [`MAX_SETF_QUEUED`] in one pass, is dropped and counted in the report.
//!   A commit whose budget ran out makes no `SETF` write: a run cut short is no result to act on.
//! * **Where it does not write.** The evaluator holds the document and never changes it; only the commits above make writes. The comparison with a file's saved
//!   values ([`crate::recalc_all`]), the dry run of a typed formula ([`crate::evaluate_text`]) and the work of the document before an edit write nothing: the first
//!   and the last take [`SetfMode::Inert`], the dry run drops the queue. An action cell is never worked out, so its `SETF` is not reached here.

use std::collections::HashMap;
use std::sync::Arc;

use diacraft_doc::{CellAddr, CellEdit, Document, FormulaSlot, Lookup, SheetRef, WriteOrigin};
use diacraft_formula::{Expr, Formula, Number};

use crate::cells::CellEval;
use crate::commit::CommitReport;
use crate::eval::{Budget, CellAddress, EvalCtx, Evaluator, SetfMode, Site};
use crate::rules::{BAD_ARGUMENT_ERROR, LOOP_ERROR, MAX_SETF_QUEUED, MAX_SETF_ROUNDS, MAX_SETF_WRITES};
use crate::strnum::format_number;
use crate::value::{CellError, Value};

/// One write a `SETF` call asks for: the cell, and the formula to give it.
#[derive(Clone, Debug)]
pub(crate) struct SetfEffect {
    pub target: CellAddress,
    pub formula: Arc<Formula>,
}

/// What a commit's rerun of the cells a round changed gives back: its report, and the effects it queued.
pub(crate) type Rerun<'a> = dyn FnMut(&mut Document, &Budget, Vec<(SheetRef, CellAddr)>) -> (CommitReport, Vec<SetfEffect>) + 'a;

/// `SETF(target, value)`: 0, and the write queued under [`SetfMode::Perform`]. See the module documentation.
pub(crate) fn setf(ev: &mut Evaluator<'_, '_>, site: &Site<'_>) -> Value {
    let perform = ev.ctx.opts.setf == SetfMode::Perform;
    let mut text: Option<Arc<str>> = None;
    if let Some(value) = site.arg(1) {
        match ev.eval(value, site.env) {
            Value::Text(t) if !ev.work(t.len()) => return Value::Error(LOOP_ERROR),
            Value::Text(t) if !t.trim().is_empty() && !diacraft_formula::parse(&t).is_parsed() => return Value::Error(CellError::Value),
            e @ (Value::Error(_) | Value::Unsupported(_)) => return e,
            other if perform => match formula_text(ev, &other) {
                Ok(t) => text = Some(t),
                Err(e) => return e,
            },
            _ => {}
        }
    }
    if !perform {
        return Value::num(0.0);
    }
    let target = match target_of(ev, site) {
        Ok(t) => t,
        Err(e) => return e,
    };
    // A blank text writes nothing: it is no formula (the call is 0 all the same, as it was).
    let Some(text) = text.filter(|t| !t.trim().is_empty()) else { return Value::num(0.0) };
    // The text is parsed again for the write, and charged by its length as the first parse was: it parsed above, so this is a formula.
    if !ev.work(text.len()) {
        return Value::Error(LOOP_ERROR);
    }
    let formula = Arc::new(diacraft_formula::parse(&text));
    if !formula.is_parsed() {
        return Value::Error(CellError::Value);
    }
    queue(ev, SetfEffect { target, formula });
    Value::num(0.0)
}

/// The text a value gives as the formula a `SETF` writes: a string as it is, a number as a formula that reads back as that number (a bare number where it has no
/// unit or the drawing's own, which a cell reads in the unit it shows; otherwise the number in its unit), and any other value as
/// `vString` makes it (`TRUE`, `RGB(r, g, b)`, a currency's own form).
fn formula_text(ev: &mut Evaluator<'_, '_>, value: &Value) -> Result<Arc<str>, Value> {
    if let Value::Number(q) = value
        && q.unit != diacraft_units::Unit::Date
        && !q.is_multidimensional()
    {
        if (q.unit == diacraft_units::Unit::Unitless && q.dim() == 0) || q.unit == diacraft_units::Unit::DrawingDefault {
            return Ok(Arc::from(format_number(q.stored)));
        }
        let shown = q.value_in(q.unit).unwrap_or(q.stored);
        return Ok(Arc::from(diacraft_formula::print(&Expr::Number(Number::with_unit(shown, q.unit)))));
    }
    ev.read_text(value)
}

/// The cell the first argument names, found as a reference in a formula is: `GETREF(cell)`, or a string that reads as one reference. Any other first
/// argument is `#VALUE!`, a reference that names no cell or a cell that is nowhere is `#REF!`.
fn target_of(ev: &mut Evaluator<'_, '_>, site: &Site<'_>) -> Result<CellAddress, Value> {
    let bad = || Value::Error(BAD_ARGUMENT_ERROR);
    let Some(first) = site.arg(0) else { return Err(bad()) };
    let (sheet, addr) = match first {
        Expr::Call(c) if c.is("GETREF") && c.qualifiers.is_empty() => match c.args.as_slice() {
            [Some(Expr::Ref(r))] => ev.locate(r, site.env)?,
            _ => return Err(bad()),
        },
        other => {
            let named = ev.eval(other, site.env);
            let Value::Text(name) = named else { return Err(if named.is_failure() { named } else { bad() }) };
            if !ev.work(name.len()) {
                return Err(Value::Error(LOOP_ERROR));
            }
            let parsed = diacraft_formula::parse(&name);
            match &parsed.ast {
                Ok(Expr::Ref(r)) => ev.locate(r, site.env)?,
                _ => return Err(bad()),
            }
        }
    };
    // A cell nothing supplies is not made by a write: a row that is not there stays not there. A cell directly under a sheet that the catalogue lists
    // (`Width` where no sheet holds one) is the sheet's own, with its default, and may be written.
    let exists = ev
        .res
        .resolve(&sheet, &addr)
        .is_some_and(|found| found.formula.cell.is_some() || found.value.cell.is_some() || (addr.row.is_none() && found.listed));
    if !exists {
        return Err(Value::Error(CellError::Ref));
    }
    Ok(CellAddress::new(sheet, addr))
}

/// Keeps an effect with the cell being worked out (`EvalCtx::setf_open`), charged to the memory of the working-out; one past [`MAX_SETF_QUEUED`] in that cell is not
/// kept and is counted, to be dropped when the cell asks ([`ask`]). The cell's result carries it, and it is queued only if the cell is worked out as itself.
fn queue(ev: &mut Evaluator<'_, '_>, effect: SetfEffect) {
    if ev.ctx.setf_open.len() >= MAX_SETF_QUEUED {
        ev.ctx.setf_open_over = ev.ctx.setf_open_over.saturating_add(1);
        return;
    }
    let bytes = (effect.formula.source.len() as u64).saturating_add(crate::rules::KEY_BYTES);
    if ev.ctx.hold_bytes(bytes) {
        ev.ctx.setf_open.push(effect);
    }
}

/// The calls of the cell about to be worked out begin empty: the list of the cell being worked out now (an outer one, when this cell is read in the middle of its
/// formula) is set aside, and handed back to [`close_calls`].
pub(crate) fn open_calls(ctx: &mut EvalCtx) -> (Vec<SetfEffect>, u64) {
    (std::mem::take(&mut ctx.setf_open), std::mem::take(&mut ctx.setf_open_over))
}

/// The cell's working-out is over: its calls (and how many were past the most one cell keeps) are what it reached, and the outer cell's list is put back.
pub(crate) fn close_calls(ctx: &mut EvalCtx, outer: (Vec<SetfEffect>, u64)) -> (Vec<SetfEffect>, u64) {
    (std::mem::replace(&mut ctx.setf_open, outer.0), std::mem::replace(&mut ctx.setf_open_over, outer.1))
}

/// **The cell at `at` was worked out as itself, giving `out`: its calls ask for their writes**, in the order they were reached, past [`MAX_SETF_QUEUED`] queued in one
/// pass dropped and counted. A cell asks once in a pass, so a cell asked for twice (a master's cell a pass works out and then writes) queues its calls once. A cell read as
/// an operand never gets here (`Evaluator::read_cell`), which is the point: only the cell that holds a call, worked out for its own reasons, asks for its write.
pub(crate) fn ask(ev: &mut Evaluator<'_, '_>, at: &CellAddress, out: &CellEval) {
    if out.setf.is_empty() && out.setf_over == 0 {
        return;
    }
    let key = (at.sheet.clone(), at.addr.clone());
    if ev.ctx.setf_asked.contains(&key) || !ev.ctx.hold_bytes(crate::rules::KEY_BYTES) {
        return;
    }
    ev.ctx.setf_asked.insert(key);
    let mut dropped = out.setf_over;
    for effect in &out.setf {
        if ev.ctx.setf.len() >= MAX_SETF_QUEUED || !ev.ctx.hold_bytes(crate::rules::KEY_BYTES) {
            dropped = dropped.saturating_add(1);
            continue;
        }
        ev.ctx.setf.push(effect.clone());
    }
    ev.ctx.setf_dropped = ev.ctx.setf_dropped.saturating_add(dropped);
}

/// Whether the cell already holds this formula text of its own.
fn holds(doc: &Document, effect: &SetfEffect) -> bool {
    let Some(sheet) = doc.sheet_of(&effect.target.sheet) else { return false };
    match sheet.lookup(&effect.target.addr) {
        Lookup::Found(cell) => matches!(&cell.formula, FormulaSlot::Local(f) if f.source == effect.formula.source),
        Lookup::Missing | Lookup::Deleted => false,
    }
}

/// Makes the writes of `effects` and counts them in `report`. Of the calls that name one cell the last wins, as the writes are made in the order the calls were
/// reached and the last one stays, so a cell is written once and the calls before it are counted as `setf_same`, as is a call whose target holds its text
/// already: **a round that leaves the cells as it found them has written nothing**, which is how a pair of calls that write one cell two texts, each time they
/// are worked out, comes to rest. A write past `left` is dropped, and one the model refuses (a row marked deleted, a limit) is refused. → the cells that were
/// written, which what reads them is worked out again from.
fn perform(doc: &mut Document, effects: Vec<SetfEffect>, left: &mut usize, report: &mut CommitReport) -> Vec<(SheetRef, CellAddr)> {
    let mut last: HashMap<&CellAddress, usize> = HashMap::with_capacity(effects.len());
    for (at, effect) in effects.iter().enumerate() {
        last.insert(&effect.target, at);
    }
    let keep: Vec<bool> = effects.iter().enumerate().map(|(at, effect)| last.get(&effect.target) == Some(&at)).collect();
    drop(last);
    let mut written = Vec::new();
    for (effect, keep) in effects.into_iter().zip(keep) {
        if !keep || holds(doc, &effect) {
            report.setf_same = report.setf_same.saturating_add(1);
            continue;
        }
        if *left == 0 {
            report.setf_dropped = report.setf_dropped.saturating_add(1);
            continue;
        }
        let SetfEffect { target, formula } = effect;
        match doc.write_cell(&target.sheet, &target.addr, CellEdit::Formula { formula, value: None }, WriteOrigin::Setf) {
            Ok(_) => {
                *left -= 1;
                report.setf_written = report.setf_written.saturating_add(1);
                written.push((target.sheet, target.addr));
            }
            Err(_) => report.setf_refused = report.setf_refused.saturating_add(1),
        }
    }
    written
}

/// Makes the `SETF` writes a commit's working-out queued, works out again what they changed with `rerun`, and goes on while that queues more, within the loop
/// budget ([`MAX_SETF_WRITES`] writes, [`MAX_SETF_ROUNDS`] rounds) and the work budget (`budget`, less what the commit has taken). Everything the rounds do is added to
/// `report`.
pub(crate) fn settle(doc: &mut Document, budget: &Budget, mut effects: Vec<SetfEffect>, report: &mut CommitReport, rerun: &mut Rerun<'_>) {
    let mut writes_left = MAX_SETF_WRITES;
    let mut rounds = 0_usize;
    while !effects.is_empty() {
        // A run cut short is no result to act on, and a loop that keeps going is stopped: the rest is dropped, and counted.
        if report.budget_exhausted || rounds >= MAX_SETF_ROUNDS {
            report.setf_dropped = report.setf_dropped.saturating_add(effects.len() as u64);
            return;
        }
        rounds += 1;
        report.setf_rounds = report.setf_rounds.saturating_add(1);
        let changed = perform(doc, std::mem::take(&mut effects), &mut writes_left, report);
        if changed.is_empty() {
            return;
        }
        let left = Budget { steps: budget.steps.saturating_sub(report.stats.steps), bytes: budget.bytes };
        if left.steps == 0 {
            report.budget_exhausted = true;
            return;
        }
        let (mut next, more) = rerun(doc, &left, changed);
        if writes_left == 0 {
            // Every write is made: what the cells worked out again reach is the calls already dropped, and is not counted a second time.
            next.setf_dropped = 0;
            report.absorb(&next);
            return;
        }
        report.absorb(&next);
        effects = more;
    }
}
