//! A formula tried on a sheet before it is written: the dry run of the ShapeSheet window's formula bar and of `sheet.evaluate` (the design
//! of the ShapeSheet window, 7.3).
//!
//! [`evaluate_text`] parses a text and works it out **as the formula of a cell would be worked out on a sheet**, and writes nothing: the document is
//! borrowed, never changed, and the work is held in an [`EvalCtx`] made for the call. What it gives is the value, fitted to the cell as a commit would
//! fit it (a bare number takes the unit the cell last held, else its default), or the parse error with its kind and its byte
//! offset **in the text as given**, so that a window can mark the fault where it was typed.
//!
//! **What it matches of a commit and what it does not.**
//!
//! * It reads every other cell as a recalculation would (`OperandMode::Live` by default): the cells the text names are worked out first, with an
//!   explicit stack and not by recursion, so a text that reads the end of a chain of thousands of cells has a value (`Evaluator::prewarm`).
//! * **A text that names the cell it is for is a loop**, as its commit would make it (`rules::LOOP_ERROR`): the cell is held as being worked out while
//!   the text runs, so the text never reads the formula the cell is about to lose. A cycle that goes through other cells is worked out by calc's own
//!   rules (a failing cell reads at its last good value), which may not be the order a commit works the cells out in.
//! * **An action cell is never worked out** (`Action` and its kin): the text is parsed, and its errors are given, but no `SETF` runs and
//!   no value is made ([`EvalResult::ActionCell`]).
//! * A bare number is **fitted as calc fits a formula's result** (the stored number is kept and only the unit changes). The rule a command uses for
//!   a literal typed in a cell (which reads the number in the unit the cell shows) is the command's, and the two agree in inches.
//!
//! **Hostile text.** The parse is charged before it is made, a step for each [`UNITS_PER_STEP`] bytes of the text, as the parse of a string read as a
//! formula is (`convert.rs`); a text past the parser's own limit is refused with no copy; and the run is held to the [`Budget`] given, so that `steps`
//! never passes it and `budget_exhausted` says when it was reached.

use std::sync::Arc;

use diacraft_doc::{CellAddr, Document, SheetRef};
use diacraft_formula::{MAX_SOURCE_BYTES, ParseError, ParseErrorKind};

use crate::cells::{clipping_path_result, is_action_cell};
use crate::eval::{Budget, CellAddress, Env, EvalCtx, EvalOptions, Evaluator, OperandMode};
use crate::rules::{LOOP_ERROR, UNITS_PER_STEP};
use crate::value::Value;

/// What a text came to.
#[derive(Clone, Debug, PartialEq)]
pub enum EvalResult {
    /// The text parsed and worked out to this value. An error the formula makes is a value (`#DIV/0!`), a function the evaluator does not have is
    /// [`Value::Unsupported`], and a run the budget stopped is the loop error with `budget_exhausted` set.
    Value(Value),
    /// The text does not parse: what went wrong, and the byte offset in the text given.
    ParseError(ParseError),
    /// The text parsed, and the cell it is for runs on an event: it is never worked out.
    ActionCell,
}

/// What [`evaluate_text`] gives.
#[derive(Clone, Debug, PartialEq)]
pub struct Evaluated {
    pub result: EvalResult,
    /// The steps taken (`EvalStats::steps`): a count of the work, never more than the budget's.
    pub steps: u64,
    /// The nodes of the tree the text parsed to (`diacraft_formula::Formula::node_count`), 0 when it did not parse. What a writer holds to the
    /// document's total of nodes.
    pub nodes: usize,
    /// Whether the budget ran out, of steps or of memory. The value is then the loop error and says nothing of the formula.
    pub budget_exhausted: bool,
}

/// Tries `text` as the formula of the cell `at` of `sheet` (or, with no cell, on the sheet itself), as a recalculation would work it out, with the
/// default options (operands worked out live, no clock, the bundled faces). See the module's account of what it matches of a commit. Nothing is
/// written. A sheet that is not in the document reads every reference as `#REF!`.
pub fn evaluate_text(doc: &Document, sheet: &SheetRef, at: Option<&CellAddr>, text: &str, budget: &Budget) -> Evaluated {
    evaluate_text_with(doc, sheet, at, text, &EvalOptions::default(), budget)
}

/// [`evaluate_text`] with the options of the session that will commit the text (its clock, its file, its faces), so that a function that reads them
/// gives here what its commit will give.
pub fn evaluate_text_with(doc: &Document, sheet: &SheetRef, at: Option<&CellAddr>, text: &str, options: &EvalOptions, budget: &Budget) -> Evaluated {
    let mut ctx = EvalCtx::new(options.clone(), *budget);
    let (result, nodes) = run(doc, &mut ctx, sheet, at, text);
    Evaluated { result, steps: ctx.stats.steps, nodes, budget_exhausted: ctx.stats.budget_exhausted }
}

fn run(doc: &Document, ctx: &mut EvalCtx, sheet: &SheetRef, at: Option<&CellAddr>, text: &str) -> (EvalResult, usize) {
    // A text past the parser's limit is refused without a copy of it, as the parser refuses it.
    if text.len() > MAX_SOURCE_BYTES {
        return (EvalResult::ParseError(ParseError { kind: ParseErrorKind::TooLong, offset: MAX_SOURCE_BYTES }), 0);
    }
    // The parse costs its length, charged before it is made.
    if !ctx.take_steps((text.len() / UNITS_PER_STEP) as u64 + 1) {
        return (EvalResult::Value(Value::Error(LOOP_ERROR)), 0);
    }
    let formula = Arc::new(diacraft_formula::parse(text));
    let nodes = formula.node_count();
    let expr = match &formula.ast {
        Ok(expr) => expr,
        Err(e) => return (EvalResult::ParseError(*e), 0),
    };
    if at.is_some_and(is_action_cell) {
        return (EvalResult::ActionCell, nodes);
    }
    let live = ctx.opts.operands == OperandMode::Live;
    let mut ev = Evaluator::new(doc, ctx);
    let mut env = Env::new(sheet.clone(), None);
    env.cell = at.cloned();
    if let Some(at) = at {
        // The cell is being given this formula: a read of it, by the text or by a cell the text reads, is a cycle, as in the commit.
        ev.ctx.in_progress.insert((sheet.clone(), at.clone()));
    }
    if live && let Some(facts) = ev.facts_of(&formula) {
        // The cells the text names are worked out first, deepest first and without recursion, as a cell's own formula's would be.
        for r in &facts.refs {
            if ev.exhausted() {
                break;
            }
            if let Ok((on, addr)) = ev.locate(r, &env) {
                // The result is in the memo, where the evaluation of the text finds it.
                let _ = ev.evaluate_cell(&CellAddress::new(on, addr));
            }
        }
    }
    let raw = ev.eval(expr, &env);
    let value = match at.and_then(|a| ev.res.resolve(sheet, a).map(|res| (res, a))) {
        Some((res, a)) => clipping_path_result(ev.fit_to_cell(raw, &res, a), a),
        None => raw,
    };
    (EvalResult::Value(value), nodes)
}
