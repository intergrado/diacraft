//! `Evaluator::evaluate_worked_out`: a cell the file saved no value for is worked out with the cells it reads worked out as well, whatever the
//! options say, and the options are as they were afterwards. A formula that did not run is no answer.
//!
//! A renderer draws what a file saved; for a cell with a formula and no `V` the formula is all there is, and the cells it reads may have no `V` either. Read
//! as saved (`OperandMode::Cached`) such a cell reads as 0, and worked out (`Live`) it reads as its formula says.

use diacraft_calc::{CellAddress, EvalCtx, EvalOptions, Evaluator, OperandMode, Value};
use diacraft_doc::{Cell, CellAddr, Document, Owner, RowKey, ShapeRef, SheetRef};
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};

fn user(name: &str) -> CellAddr {
    CellAddr::in_row("User", None, RowKey::name(name), "Value")
}

fn unsaved(formula: &str) -> Cell {
    Cell::with_formula(formula, None)
}

fn doc_of(shape: ShapeBuilder) -> Document {
    DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build()
}

fn at(addr: CellAddr) -> CellAddress {
    CellAddress::new(SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0)), addr)
}

fn number(v: &Value) -> Option<f64> {
    match v {
        Value::Number(q) => Some(q.stored),
        _ => None,
    }
}

/// Two cells with no value: `User.A` is `2+3`, `User.B` reads it and adds one.
fn chain() -> Document {
    doc_of(ShapeBuilder::new(1).set(user("A"), unsaved("2+3")).set(user("B"), unsaved("User.A+1")))
}

#[test]
fn a_cell_that_reads_a_cell_with_no_value_is_worked_out_with_it() {
    let doc = chain();
    let mut ctx = EvalCtx::new(EvalOptions { operands: OperandMode::Cached, ..EvalOptions::default() }, diacraft_calc::Budget::with_steps(100_000));
    let mut ev = Evaluator::new(&doc, &mut ctx);
    // Read as saved, the operand is nothing, and the cell is 0 + 1.
    assert_eq!(number(&ev.evaluate(&at(user("B")))), Some(1.0));
    // Worked out, it is what the formulas say.
    assert_eq!(ev.evaluate_worked_out(&at(user("B"))).as_ref().and_then(number), Some(6.0));
}

#[test]
fn the_options_are_as_they_were_after_the_call() {
    let doc = chain();
    let mut ctx = EvalCtx::new(EvalOptions { operands: OperandMode::Cached, ..EvalOptions::default() }, diacraft_calc::Budget::with_steps(100_000));
    {
        let mut ev = Evaluator::new(&doc, &mut ctx);
        let _ = ev.evaluate_worked_out(&at(user("B")));
    }
    assert_eq!(ctx.opts.operands, OperandMode::Cached);
    let mut ev = Evaluator::new(&doc, &mut ctx);
    assert_eq!(number(&ev.evaluate(&at(user("B")))), Some(1.0), "a later read as saved is still a read as saved");
}

#[test]
fn a_formula_that_does_not_parse_is_no_answer() {
    let doc = doc_of(ShapeBuilder::new(1).set(user("A"), unsaved("2+")).set(user("B"), unsaved("(((")));
    let mut ctx = EvalCtx::default();
    let mut ev = Evaluator::new(&doc, &mut ctx);
    assert_eq!(ev.evaluate_worked_out(&at(user("A"))), None);
    assert_eq!(ev.evaluate_worked_out(&at(user("B"))), None);
}

#[test]
fn a_cell_with_no_formula_is_no_answer() {
    let doc = doc_of(ShapeBuilder::new(1).cell("Width", "2"));
    let mut ctx = EvalCtx::default();
    let mut ev = Evaluator::new(&doc, &mut ctx);
    assert_eq!(ev.evaluate_worked_out(&at(CellAddr::direct("Width"))), None, "a constant is read as saved");
    assert_eq!(ev.evaluate_worked_out(&at(CellAddr::direct("Height"))), None, "a cell that is not there has nothing to run");
}

#[test]
fn a_cycle_of_cells_with_no_value_ends_as_an_error_value_and_not_a_hang() {
    let doc = doc_of(ShapeBuilder::new(1).set(user("A"), unsaved("User.B+1")).set(user("B"), unsaved("User.A+1")));
    let mut ctx = EvalCtx::new(EvalOptions::default(), diacraft_calc::Budget::with_steps(100_000));
    let mut ev = Evaluator::new(&doc, &mut ctx);
    let worked = ev.evaluate_worked_out(&at(user("A")));
    assert!(matches!(worked, Some(Value::Error(_))), "{worked:?}");
    assert!(ctx.stats.cycles >= 1, "the cycle was met and counted: {:?}", ctx.stats);
}

#[test]
fn a_long_chain_is_walked_on_a_stack_and_the_steps_bound_it() {
    // 3,000 cells each reading the one before: worked out whole when the steps allow, and an error value when they do not, in either case with no native recursion
    // through the chain (the walk keeps its own stack, `cycle.rs`) and no more steps than the budget.
    let mut shape = ShapeBuilder::new(1).set(user("R0"), unsaved("1"));
    for i in 1..3_000u32 {
        shape = shape.set(user(&format!("R{i}")), unsaved(&format!("User.R{}+1", i - 1)));
    }
    let doc = doc_of(shape);
    let last = at(user("R2999"));
    let mut roomy = EvalCtx::new(EvalOptions::default(), diacraft_calc::Budget::with_steps(10_000_000));
    assert_eq!(Evaluator::new(&doc, &mut roomy).evaluate_worked_out(&last).as_ref().and_then(number), Some(3_000.0));
    let mut tight = EvalCtx::new(EvalOptions::default(), diacraft_calc::Budget::with_steps(500));
    let worked = Evaluator::new(&doc, &mut tight).evaluate_worked_out(&last);
    assert!(!matches!(worked.as_ref().and_then(number), Some(n) if n == 3_000.0), "500 steps do not reach the end of the chain: {worked:?}");
    assert!(tight.stats.budget_exhausted, "{:?}", tight.stats);
}

/// A page of `n` shapes on a theme, each holding `THEMEVAL()` as a formula and no value in its fill: what a style gives every shape that follows it.
fn themed_page(n: u32) -> Document {
    let mut page = PageBuilder::new();
    for id in 1..=n {
        let mut shape = ShapeBuilder::new(id);
        for name in ["ThemeIndex", "ColorSchemeIndex", "EffectSchemeIndex", "ConnectorSchemeIndex", "FontSchemeIndex"] {
            shape = shape.cell(name, "40");
        }
        shape = shape.cell("QuickStyleFillColor", "2").cell("QuickStyleFillMatrix", "1");
        page = page.shape(shape.set(CellAddr::direct("FillForegnd"), unsaved("THEMEVAL()")));
    }
    let mut doc = DocBuilder::new().page(1, page).build();
    doc.themes.push(diacraft_testkit::theme::ThemeXml::default().part());
    doc
}

#[test]
fn an_unsaved_theme_call_costs_a_few_steps_a_cell_and_the_cost_does_not_grow_with_the_page() {
    // Counted, as time is no measure on a machine at load: the steps a cell of each of 100 and 1,000 shapes takes are the same few, the work of one formula
    // and the reads it makes, and not a walk of the page for each cell. A shape of the first page answers as a shape of the second does.
    let steps_a_cell = |n: u32| {
        let doc = themed_page(n);
        let mut ctx = EvalCtx::new(EvalOptions::default(), diacraft_calc::Budget::with_steps(50_000_000));
        let mut ev = Evaluator::new(&doc, &mut ctx);
        for at_shape in 0..n {
            let cell = CellAddress::new(SheetRef::Shape(ShapeRef::top(Owner::Page(1), at_shape)), CellAddr::direct("FillForegnd"));
            let answer = ev.evaluate_worked_out(&cell);
            assert!(matches!(answer, Some(Value::Color(_))), "shape {at_shape}: {answer:?}");
        }
        assert!(!ctx.stats.budget_exhausted);
        ctx.stats.steps / u64::from(n)
    };
    let (small, large) = (steps_a_cell(100), steps_a_cell(1_000));
    assert!(large <= small + small / 10 + 1, "{small} steps a cell at 100 shapes, {large} at 1,000");
    assert!(large <= STEPS_A_THEME_CELL, "{large} steps a cell against {STEPS_A_THEME_CELL}");
}

/// The most steps a cell holding `THEMEVAL()` and no value takes: twice the 25 it took where this was written, at 100 and at 1,000 shapes.
const STEPS_A_THEME_CELL: u64 = 50;

// ---- a context of its own -------------------------------------------------------------------------------------------------------

#[test]
fn a_cell_worked_out_in_a_context_of_its_own_spends_that_contexts_steps_and_leaves_the_evaluators_alone() {
    // A frame that works cells out for a drawing keeps the steps of its themed cells apart from the steps of the cells it was given no value for: neither can spend the
    // other's, and the budget of one running out is not the other's end.
    let doc = chain();
    let mut ctx = EvalCtx::new(EvalOptions { operands: OperandMode::Cached, ..EvalOptions::default() }, diacraft_calc::Budget::with_steps(1_000));
    let mut own = EvalCtx::new(EvalOptions::default(), diacraft_calc::Budget::with_steps(100_000));
    {
        let mut ev = Evaluator::new(&doc, &mut ctx);
        let worked = ev.evaluate_worked_out_in(&mut own, &at(user("B")));
        assert_eq!(worked.as_ref().and_then(number), Some(6.0));
    }
    assert!(own.stats.steps > 0, "the steps were the context's own: {:?}", own.stats);
    assert_eq!(ctx.stats.steps, 0, "none was the evaluator's: {:?}", ctx.stats);
    assert_eq!(ctx.steps_left(), 1_000);
    assert_eq!(ctx.opts.operands, OperandMode::Cached, "the evaluator's options are as they were");
}

#[test]
fn the_budget_of_a_context_of_its_own_running_out_does_not_end_the_evaluators() {
    let doc = chain();
    let mut ctx = EvalCtx::new(EvalOptions::default(), diacraft_calc::Budget::with_steps(1_000));
    let mut own = EvalCtx::new(EvalOptions::default(), diacraft_calc::Budget::with_steps(2));
    {
        let mut ev = Evaluator::new(&doc, &mut ctx);
        let worked = ev.evaluate_worked_out_in(&mut own, &at(user("B")));
        assert!(!matches!(worked.as_ref().and_then(number), Some(n) if n == 6.0), "two steps do not reach the end: {worked:?}");
        assert!(own.stats.budget_exhausted, "{:?}", own.stats);
        // The evaluator goes on: a cell it works out in its own context is whole.
        assert_eq!(number(&ev.evaluate(&at(user("A")))), Some(5.0));
    }
    assert!(!ctx.stats.budget_exhausted, "{:?}", ctx.stats);
}

// ---- an evaluator that holds its context ---------------------------------------------------------------------------------------

#[test]
fn an_evaluator_that_holds_its_context_works_cells_out_and_keeps_its_memo_from_one_cell_to_the_next() {
    // A reader that works the cells of a page out one after another keeps one evaluator for them all: what it built for the first is there for the next.
    let doc = chain();
    let mut ev = Evaluator::owning(&doc, EvalCtx::new(EvalOptions::default(), diacraft_calc::Budget::with_steps(100_000)));
    assert_eq!(ev.evaluate_worked_out(&at(user("B"))).as_ref().and_then(number), Some(6.0));
    let after_b = ev.stats().steps;
    assert!(after_b > 0);
    // `User.A` was worked out on the way to B and is in the memo: asking for it again costs a lookup and not the formula.
    assert_eq!(ev.evaluate_worked_out(&at(user("A"))).as_ref().and_then(number), Some(5.0));
    assert!(ev.stats().steps - after_b < after_b, "the second answer was found, not worked out again: {after_b} then {}", ev.stats().steps);
}

#[test]
fn steps_granted_to_an_evaluator_are_the_steps_it_may_spend_and_a_budget_that_ran_out_stays_out() {
    let doc = chain();
    let mut ev = Evaluator::owning(&doc, EvalCtx::new(EvalOptions::default(), diacraft_calc::Budget::with_steps(1_000)));
    assert_eq!(ev.steps_left(), 1_000);
    ev.grant_steps(500);
    assert_eq!(ev.steps_left(), 1_500);
    ev.grant_steps(u64::MAX);
    assert_eq!(ev.steps_left(), u64::MAX, "the count stops at the most there is");
    // A budget with too little for the formula ends the run for good: more steps given after do not bring it back.
    let mut tight = Evaluator::owning(&doc, EvalCtx::new(EvalOptions::default(), diacraft_calc::Budget::with_steps(2)));
    let worked = tight.evaluate_worked_out(&at(user("B")));
    assert!(!matches!(worked.as_ref().and_then(number), Some(n) if n == 6.0), "two steps do not reach the end: {worked:?}");
    assert!(tight.stats().budget_exhausted);
    tight.grant_steps(100_000);
    assert!(tight.stats().budget_exhausted, "it is out for good");
    assert!(!matches!(tight.evaluate_worked_out(&at(user("B"))).as_ref().and_then(number), Some(n) if n == 6.0));
}
