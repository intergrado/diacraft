//! A file is hostile: formula depth, reference chains, cycles and fan-out come from it (the lessons of M2's reviews). Each test builds the
//! crafted input, runs it under a hang guard (a minute: tests have raced under load, so no cost is held to a time; the steps the evaluation
//! took are the count of its work, `EvalStats::steps`), and checks that the answer is a value, never a panic or a stack overflow. Those that
//! go deep run on a thread with a stack of 512 KiB, half what the browser gives.

use std::time::{Duration, Instant};

use diacraft_calc::{Budget, CellAddress, CellError, EvalCtx, EvalOptions, Evaluator, OperandMode, Value, recalc_all_with};
use diacraft_doc::{CellAddr, Document, Owner, RowKey, ShapeRef, SheetRef};
use diacraft_testkit::build::{DocBuilder, MasterBuilder, PageBuilder, ShapeBuilder};

use crate::{E, mismatch, probe};

/// The bound a hang test gives its work: a hang guard and no measure of cost (the slowest of these takes about a second at load 50).
const BOUND: Duration = Duration::from_secs(60);

/// Checks the time taken against [`BOUND`], and prints it, so that `cargo test hostile -- --nocapture` shows the worst times.
fn within_bound(started: Instant) {
    let took = started.elapsed();
    eprintln!("hostile time: {} {:?}", std::thread::current().name().unwrap_or("(small stack)"), took);
    assert!(took < BOUND, "took {took:?}");
}

/// Runs `f` on a thread with a small stack, so that a native recursion that is not bounded overflows it here and not in the field.
fn small_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new().stack_size(512 * 1024).spawn(f).unwrap().join().unwrap()
}

fn shape_at(i: u32) -> SheetRef {
    SheetRef::Shape(ShapeRef::top(Owner::Page(1), i))
}

fn evaluate(doc: &Document, sheet: SheetRef, addr: CellAddr, options: EvalOptions, budget: Budget) -> (Value, EvalCtx) {
    let mut ctx = EvalCtx::new(options, budget);
    let v = Evaluator::new(doc, &mut ctx).evaluate(&CellAddress::new(sheet, addr));
    (v, ctx)
}

fn scratch(row: u32) -> CellAddr {
    CellAddr::in_row("Scratch", None, RowKey::Ix(row), "A")
}

#[test]
fn a_long_chain_of_references_is_worked_out_without_recursion() {
    // `Scratch.A1` is 1 and each row after reads the one before and adds 1: 5,000 cells deep, read from the last.
    let n = 5_000u32;
    let mut shape = ShapeBuilder::new(1).row_cell("Scratch", None, RowKey::Ix(0), "A", "1");
    for k in 1..n {
        shape = shape.row_formula("Scratch", None, RowKey::Ix(k), "A", &format!("Scratch.A{k}+1"), "0");
    }
    let doc = DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build();
    let started = Instant::now();
    let (v, ctx) = small_stack(move || evaluate(&doc, shape_at(0), scratch(n - 1), EvalOptions::default(), Budget::default()));
    within_bound(started);
    assert_eq!(mismatch(&v, &E::N(f64::from(n))), None, "{v:?}");
    assert_eq!(ctx.stats.depth_limit_hits, 0, "the chain is walked with a stack of its own");
}

#[test]
fn a_cell_read_by_many_cells_is_worked_out_once() {
    // 16,000 cells (a section holds 16,384 rows) read one base cell that is itself a formula; with the memo the work is linear in the cells.
    let n = 16_000u32;
    let mut shape = ShapeBuilder::new(1).row_formula("User", None, RowKey::name("Base"), "Value", "SUM(1,2,3)+PI()", "0");
    for k in 0..n {
        shape = shape.row_formula("Scratch", None, RowKey::Ix(k), "A", "User.Base*2", "0");
    }
    let doc = DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build();
    let started = Instant::now();
    let report = recalc_all_with(&doc, &Budget::default(), EvalOptions { operands: OperandMode::Live, ..EvalOptions::default() });
    within_bound(started);
    assert_eq!(report.cells, u64::from(n) + 1);
    assert!(report.stats.steps < 60 * u64::from(n), "linear in the cells: {} steps", report.stats.steps);
}

#[test]
fn a_cycle_ends_in_an_error_value() {
    let shape = ShapeBuilder::new(1)
        .row_formula("Scratch", None, RowKey::Ix(0), "A", "Scratch.B1+1", "0")
        .row_formula("Scratch", None, RowKey::Ix(0), "B", "Scratch.A1+1", "0")
        .row_formula("Scratch", None, RowKey::Ix(0), "C", "Scratch.C1+1", "0");
    let doc = DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build();
    for cell in ["A", "B", "C"] {
        let addr = CellAddr::in_row("Scratch", None, RowKey::Ix(0), cell);
        let (v, ctx) = evaluate(&doc, shape_at(0), addr, EvalOptions::default(), Budget::default());
        // The class the rule gives a cycle (`rules::LOOP_ERROR`, `#VALUE!`), a row cell as a plain one (it was `#REF!`).
        assert_eq!(v, Value::Error(CellError::Value), "{cell}");
        assert!(ctx.stats.cycles > 0, "{cell}: the cycle is counted");
    }
}

#[test]
fn a_cycle_through_inherited_cells_ends() {
    // The master's `LocPinX` reads `LocPinY` and the reverse; the instance inherits both.
    let master = MasterBuilder::new().shape(ShapeBuilder::new(5).formula("LocPinX", "LocPinY+1", "1").formula("LocPinY", "LocPinX+1", "1"));
    let page = PageBuilder::new().shape(ShapeBuilder::new(10).master(1).inh("LocPinX", None).inh("LocPinY", None));
    let doc = DocBuilder::new().master(1, master).page(1, page).build();
    for options in [EvalOptions::default(), EvalOptions { operands: OperandMode::Cached, ..EvalOptions::default() }] {
        let started = Instant::now();
        let mode = options.operands;
        let (v, _) = evaluate(&doc, shape_at(0), CellAddr::direct("LocPinX"), options, Budget::default());
        within_bound(started);
        // Live: the error of a cycle. Cached: each cell is read as saved, and the instance's has no `V`, so the default stands: no cycle at all.
        match mode {
            OperandMode::Live => assert_eq!(v, Value::Error(CellError::Value)),
            OperandMode::Cached => assert!(matches!(v, Value::Number(_)), "{v:?}"),
        }
    }
}

#[test]
fn a_function_that_runs_its_arguments_again_is_bounded_by_the_budget() {
    // `EVALCELL(User.F,"n",1)+EVALCELL(User.F,"n",2)` doubles the work at each of 64 levels: 2^64 evaluations, which nothing may finish.
    let shape = ShapeBuilder::new(1).row_formula("User", None, RowKey::name("F"), "Value", "EVALCELL(User.F,\"n\",1)+EVALCELL(User.F,\"n\",2)", "0");
    let doc = DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build();
    let started = Instant::now();
    let budget = Budget::with_steps(500_000);
    let (v, ctx) = evaluate(&doc, shape_at(0), CellAddr::in_row("User", None, RowKey::name("F"), "Value"), EvalOptions::default(), budget);
    within_bound(started);
    assert!(ctx.stats.budget_exhausted, "{:?}", ctx.stats);
    assert!(matches!(v, Value::Error(_)), "{v:?}");
    assert!(ctx.stats.steps <= 500_000);
}

#[test]
fn a_string_that_reads_itself_ends() {
    // `User.S` holds the text `User.S`: read as a formula it names itself, to the depth rule.
    let setup = |s: ShapeBuilder| {
        let text = diacraft_doc::Cell::constant(diacraft_doc::Value::from_text("User.S")).with_unit("STR");
        s.set(CellAddr::in_row("User", None, RowKey::name("S"), "Value"), text)
    };
    let (doc, at) = probe(setup, "User.S+1");
    let started = Instant::now();
    let mut ctx = EvalCtx::default();
    let v = Evaluator::new(&doc, &mut ctx).evaluate(&at);
    within_bound(started);
    assert_eq!(mismatch(&v, &E::Err(CellError::Value)), None, "{v:?}");
}

#[test]
fn a_string_that_doubles_at_each_level_is_capped() {
    // `ARG("s","a")&ARG("s","a")` under a doubling `EVALCELL` would build a string of 2^64 bytes.
    let shape =
        ShapeBuilder::new(1).row_formula("User", None, RowKey::name("F"), "Value", "EVALCELL(User.F,\"s\",ARG(\"s\",\"a\")&ARG(\"s\",\"a\"))", "0");
    let doc = DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build();
    let (v, _) = evaluate(&doc, shape_at(0), CellAddr::in_row("User", None, RowKey::name("F"), "Value"), EvalOptions::default(), Budget::default());
    assert!(matches!(v, Value::Error(_)), "{v:?}");
}

#[test]
fn one_formula_at_the_node_limit() {
    // 131,000 arguments of one call is as many nodes as a formula may have (`diacraft_formula::MAX_NODES` is 131,072).
    let args = vec!["1"; 130_000].join(",");
    let formula = format!("SUM({args})");
    let (doc, at) = probe(|s| s, &formula);
    let started = Instant::now();
    let mut ctx = EvalCtx::default();
    let v = Evaluator::new(&doc, &mut ctx).evaluate(&at);
    within_bound(started);
    assert_eq!(mismatch(&v, &E::N(130_000.0)), None, "{v:?}");
    // The same formula with an argument that is an error in the last slot.
    let formula = format!("SUM({args},1/0)");
    let (doc, at) = probe(|s| s, &formula);
    let v = Evaluator::new(&doc, &mut EvalCtx::default()).evaluate(&at);
    assert!(matches!(v, Value::Error(_) | Value::Unsupported(_)), "{v:?}");
}

#[test]
fn a_left_leaning_chain_at_the_depth_limit_and_a_nest_at_the_nesting_limit_run_on_a_small_stack() {
    let chain = vec!["1"; 500].join("+");
    let nest = format!("{}1{}", "ABS(".repeat(95), ")".repeat(95));
    let parens = format!("{}1{}", "(".repeat(95), ")".repeat(95));
    let concat = format!("{}\"a\"{}", "(\"a\"&".repeat(90), ")".repeat(90));
    let (doc, _) = probe(|s| s, "1");
    let formulas = vec![(chain, 500.0), (nest, 1.0), (parens, 1.0)];
    for (formula, want) in formulas {
        let (d, at) = probe(|s| s, &formula);
        let v = small_stack(move || Evaluator::new(&d, &mut EvalCtx::default()).evaluate(&at));
        assert_eq!(mismatch(&v, &E::N(want)), None, "{v:?}");
    }
    let (d, at) = probe(|s| s, &concat);
    let v = small_stack(move || Evaluator::new(&d, &mut EvalCtx::default()).evaluate(&at));
    assert!(matches!(v, Value::Text(_)), "{v:?}");
    let _ = doc;
}

#[test]
fn a_qualified_call_nested_at_the_limit_with_a_chain_below_it() {
    // `Sheet.1!ABS(Sheet.1!ABS(...))` 90 deep in each of 300 cells, each reading the next: the chain is walked with a stack and the nesting
    // is the formula's own, so no frame count passes the limit.
    let nest = |inner: &str| format!("{}{}{}", "Sheet.1!ABS(".repeat(90), inner, ")".repeat(90));
    let n = 300u32;
    let mut shape = ShapeBuilder::new(1).row_cell("Scratch", None, RowKey::Ix(0), "A", "1");
    for k in 1..n {
        shape = shape.row_formula("Scratch", None, RowKey::Ix(k), "A", &nest(&format!("Scratch.A{k}")), "0");
    }
    let doc = DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build();
    let started = Instant::now();
    let (v, _) = small_stack(move || evaluate(&doc, shape_at(0), scratch(n - 1), EvalOptions::default(), Budget::default()));
    within_bound(started);
    assert_eq!(mismatch(&v, &E::N(1.0)), None, "{v:?}");
}

#[test]
fn a_nested_string_formula_and_evalcell_chain_stay_inside_the_frame_limit() {
    // A chain of `EVALCELL`s each nesting 90 `ABS`: the frames are counted across the cells, so the limit holds on a small stack.
    let nest = |inner: &str| format!("{}{}{}", "ABS(".repeat(90), inner, ")".repeat(90));
    let mut shape = ShapeBuilder::new(1).row_cell("User", None, RowKey::name("F0"), "Value", "1");
    for k in 1..=40u32 {
        shape = shape.row_formula("User", None, RowKey::name(&format!("F{k}")), "Value", &nest(&format!("EVALCELL(User.F{},\"x\",1)", k - 1)), "0");
    }
    let doc = DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build();
    let (v, ctx) = small_stack(move || {
        evaluate(&doc, shape_at(0), CellAddr::in_row("User", None, RowKey::name("F40"), "Value"), EvalOptions::default(), Budget::default())
    });
    assert!(matches!(v, Value::Error(_)), "{v:?}");
    assert!(ctx.stats.depth_limit_hits > 0, "the frame limit is what stopped it: {:?}", ctx.stats);
}

#[test]
fn a_page_of_many_shapes_naming_each_other_is_not_quadratic() {
    // 30,000 shapes, each reading the next by `Sheet.<id>` and by name: the lookups go through an index, so the work is linear.
    let n = 30_000u32;
    let mut page = PageBuilder::new();
    for k in 0..n {
        let shape = ShapeBuilder::new(k + 1).named(&format!("S{k}"), &format!("S{k}")).cell("Width", "1").formula(
            "Height",
            &format!("Sheet.{}!Width+S{}!Width", k + 2, (k + 1) % n),
            "2",
        );
        page = page.shape(shape);
    }
    let doc = DocBuilder::new().page(1, page).build();
    let started = Instant::now();
    let report = recalc_all_with(&doc, &Budget::default(), EvalOptions { operands: OperandMode::Cached, ..EvalOptions::default() });
    within_bound(started);
    // The last shape names `Sheet.30002`, which is not there: `#REF!`, and the rest are right.
    assert_eq!(report.cells, u64::from(n));
    assert!(report.agreed >= u64::from(n) - 2, "{} agreed", report.agreed);
}

#[test]
fn a_document_that_points_at_nothing_gives_values_and_no_panic() {
    // A `Master` that names no master, a `MasterShape` with no instance above, shape IDs used twice, a style link to nowhere.
    let page = PageBuilder::new()
        .shape(ShapeBuilder::new(1).master(99).formula("PinX", "Sheet.1!Width+Sheet.2!Width", "0").formula("PinY", "NAME()&MASTERNAME()", "x"))
        .shape(ShapeBuilder::new(1).master_shape(5).formula("PinX", "Width+LocPinX", "0").links(Some(77), Some(77), Some(77)))
        .shape(ShapeBuilder::new(2).formula("Width", "Sheet.1!ID()+PAGENUMBER()", "0"));
    let doc = DocBuilder::new().page(1, page).build();
    let report = recalc_all_with(&doc, &Budget::default(), EvalOptions::default());
    assert!(report.cells >= 4, "{report:?}");
    let report = recalc_all_with(&doc, &Budget::default(), EvalOptions { operands: OperandMode::Cached, ..EvalOptions::default() });
    assert!(report.cells >= 4, "{report:?}");
}

#[test]
fn a_search_in_a_long_text_costs_the_length_and_not_the_product() {
    // A needle of 250,000 `a` and a `b` in a haystack of 500,000 `a`: a search that restarts at each place does 10^11 comparisons.
    let started = Instant::now();
    let v = crate::eval(r#"FIND(REPT("a",250000)&"b",REPT("a",500000))"#);
    within_bound(started);
    assert_eq!(mismatch(&v, &E::Err(diacraft_calc::CellError::Value)), None, "{v:?}");
    let started = Instant::now();
    let v = crate::eval(r#"LEN(SUBSTITUTE(REPT("a",500000),REPT("a",250000)&"b","x"))"#);
    within_bound(started);
    assert_eq!(mismatch(&v, &E::N(500_000.0)), None, "{v:?}");
}

#[test]
fn text_made_by_a_formula_is_capped() {
    // Each of these would make more than 1 MiB: the repeat, the join, the substitution, the replacement, a picture of many `@` over a
    // long text, and a text of tabs that `SHAPETEXT` widens. All give `#VALUE!` and none allocates what it asked for.
    let started = Instant::now();
    for f in [
        r#"REPT("abcdefghij",200000)"#,
        r#"REPT("a",1000000)&REPT("a",1000000)"#,
        r#"SUBSTITUTE(REPT("a",900000),"a","bb")"#,
        r#"REPLACE(REPT("a",900000),1,0,REPT("b",900000))"#,
        r#"FORMAT(REPT("a",1000000),REPT("@",4000))"#,
    ] {
        let v = crate::eval(f);
        assert!(matches!(v, Value::Error(diacraft_calc::CellError::Value)), "{f}: {v:?}");
    }
    // A shape's text of more than 1 MiB is refused whole, flags or none.
    let v = crate::eval_on(|s| s.text(&"a".repeat((1 << 20) + 1)), "SHAPETEXT(TheText)");
    assert!(
        matches!(v, Value::Error(diacraft_calc::CellError::Value)) || matches!(&v, Value::Text(t) if t.len() <= (1 << 20)),
        "{:?}",
        v.kind_name()
    );
    let v = crate::eval_on(|s| s.text(&"\t".repeat(200_000)), "SHAPETEXT(TheText,8)");
    assert!(matches!(v, Value::Error(diacraft_calc::CellError::Value)), "{:?}", v.kind_name());
    within_bound(started);
}

#[test]
fn a_picture_costs_no_more_than_its_length() {
    let started = Instant::now();
    // A picture past the cap is `#VALUE!`; one under it with thousands of placeholders and a huge number is worked in its length.
    let long = "0".repeat(5000);
    let v = crate::eval(&format!("FORMAT(1,\"{long}\")"));
    assert_eq!(mismatch(&v, &E::Err(diacraft_calc::CellError::Value)), None, "{v:?}");
    let places = format!("0.{}", "0".repeat(3000));
    let v = crate::eval(&format!("LEN(FORMAT(1E300,\"{places}\"))"));
    assert!(matches!(v, Value::Number(_)), "{v:?}");
    let v = crate::eval(r#"FORMAT(1E300,"0 #/#######")"#);
    assert!(matches!(v, Value::Text(_)), "{v:?}");
    let v = crate::eval(r#"FORMAT(1E12,"dddd d MMMM yyyy")"#);
    assert_eq!(mismatch(&v, &E::Err(diacraft_calc::CellError::Value)), None, "{v:?}");
    within_bound(started);
}

#[test]
fn a_chain_of_groups_past_the_limit_is_value_and_not_a_hang() {
    // A group chain of 200 levels, a point carried from the innermost to the page: `#VALUE!` past 128 steps.
    let mut shape = ShapeBuilder::new(200).cell("PinX", "1").row_formula("User", None, RowKey::name("Probe"), "Value", "PNTX(PAR(PNT(1,1)))", "0");
    for id in (1..200).rev() {
        shape = ShapeBuilder::new(id).cell("PinX", "1").child(shape);
    }
    let doc = DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build();
    let mut at = ShapeRef::top(Owner::Page(1), 0);
    for _ in 1..200 {
        at = at.child(0);
    }
    let started = Instant::now();
    let (v, _) = evaluate(&doc, SheetRef::Shape(at), crate::probe_addr(), EvalOptions::default(), Budget::default());
    within_bound(started);
    assert_eq!(mismatch(&v, &E::Err(diacraft_calc::CellError::Value)), None, "{v:?}");
}

#[test]
fn the_caps_hold_at_their_exact_size() {
    // Each cap allows its size and refuses one more: an off-by-one at a cap is a hang or an overflow in the field and a `#VALUE!` here.
    let cap = 1usize << 20;
    let n = |f: &str| crate::eval(f);
    // REPT: exactly 1 MiB passes, one byte more does not.
    assert_eq!(mismatch(&n(&format!("LEN(REPT(\"a\",{cap}))")), &E::N(cap as f64)), None);
    assert_eq!(mismatch(&n(&format!("REPT(\"a\",{})", cap + 1)), &E::Err(diacraft_calc::CellError::Value)), None);
    // CAT: two halves make exactly 1 MiB, one byte more does not.
    let half = cap / 2;
    assert_eq!(mismatch(&n(&format!("LEN(REPT(\"a\",{half})&REPT(\"a\",{half}))")), &E::N(cap as f64)), None);
    assert_eq!(mismatch(&n(&format!("REPT(\"a\",{half})&REPT(\"a\",{})", half + 1)), &E::Err(diacraft_calc::CellError::Value)), None);
    // A picture of exactly 4,096 characters is worked, one more is not.
    let pic = |len: usize| format!("FORMAT(1,\"{}\")", "x".repeat(len));
    assert!(matches!(n(&pic(4096)), Value::Text(_)));
    assert_eq!(mismatch(&n(&pic(4097)), &E::Err(diacraft_calc::CellError::Value)), None);
    // A string read as a formula: 4,096 bytes are read as one (`1+1` is 2, so the sum is 3) and 4,097 are not (`#VALUE!`).
    let padded = |len: usize| format!("\"1+1{}\"+1", " ".repeat(len - 3));
    assert_eq!(mismatch(&n(&padded(4096)), &E::N(3.0)), None, "{:?}", n(&padded(4096)));
    assert_eq!(mismatch(&n(&padded(4097)), &E::Err(diacraft_calc::CellError::Value)), None, "{:?}", n(&padded(4097)));
}

/// A chain of `depth` nested shapes, the innermost holding the probe.
fn group_chain(depth: u32) -> Value {
    let mut shape = ShapeBuilder::new(depth).cell("PinX", "1").row_formula("User", None, RowKey::name("Probe"), "Value", "PNTX(PAR(PNT(1,1)))", "0");
    for id in (1..depth).rev() {
        shape = ShapeBuilder::new(id).cell("PinX", "1").child(shape);
    }
    let doc = DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build();
    let mut at = ShapeRef::top(Owner::Page(1), 0);
    for _ in 1..depth {
        at = at.child(0);
    }
    evaluate(&doc, SheetRef::Shape(at), crate::probe_addr(), EvalOptions::default(), Budget::default()).0
}

#[test]
fn a_group_chain_of_exactly_the_limit_is_carried_and_one_more_is_not() {
    let limit = diacraft_calc::rules::MAX_FRAME_STEPS as u32;
    assert!(matches!(group_chain(limit), Value::Number(_)), "{:?}", group_chain(limit));
    assert_eq!(mismatch(&group_chain(limit + 1), &E::Err(diacraft_calc::CellError::Value)), None);
}

#[test]
fn random_pictures_and_values_never_panic() {
    // A deterministic generator (xorshift) joins random tokens of the picture language, casts, unclosed quotes and brackets included, and
    // puts random values through them; every call must come back as a value. Only the absence of a panic or a hang is asserted here.
    let tokens = [
        "0",
        "#",
        ".",
        ",",
        "E+",
        "e-",
        "E+0",
        "u",
        "U",
        "uu",
        "UUU",
        "/",
        "@",
        "@+",
        "@-",
        "d",
        "dd",
        "ddd",
        "dddd",
        "ddddd",
        "M",
        "MM",
        "MMM",
        "MMMM",
        "yy",
        "yyyy",
        "h",
        "hh",
        "H",
        "m",
        "mm",
        "s",
        "ss",
        "t",
        "tt",
        "am/pm",
        "c",
        "C",
        "T",
        "[h]",
        "[mm]",
        "[ss]",
        "[d]",
        "[",
        "]",
        " ",
        "$",
        "esc(0)",
        "esc(15)",
        "esc(99)",
        "{<204>}",
        "{<200>}",
        "{<9>}",
        "{<9999999999>}",
        "<,ft>",
        "<ft,in>",
        "<,deg>",
        "<,FEET/INCH>",
        "<",
        ">",
        "{{",
        "}}",
        "\\",
        "'x'",
        "'",
        "#/#",
        "# #/4",
        "# #/#16",
        "0.####",
        "000",
        "#,##0",
        "{",
        "}",
        "esc(",
        "(",
    ];
    let values = [
        "0",
        "1",
        "-1.5",
        "2.4",
        "1E300",
        "1E-300",
        "-0.0004",
        "47569.5",
        "-3",
        "3 in",
        "5 cm",
        "30 deg",
        "\"txt\"",
        "\"\"",
        "TRUE",
        "DATE(2031,3,7)",
        "1/3",
        "7/0",
        "1E15",
        "0.99999999999999",
        "123456789012345678",
    ];
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    let mut next = move |n: usize| {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state % n as u64) as usize
    };
    let started = Instant::now();
    let mut texts = 0;
    for _ in 0..4000 {
        let picture: String = (0..=next(8)).map(|_| tokens[next(tokens.len())]).collect();
        let value = values[next(values.len())];
        let formula = format!("FORMAT({value},\"{}\")", picture.replace('"', "\"\""));
        let v = crate::eval(&formula);
        assert!(matches!(v, Value::Text(_) | Value::Error(_) | Value::Unsupported(_)), "{formula}: {v:?}");
        texts += usize::from(matches!(v, Value::Text(_)));
        let ex = format!("FORMATEX({value},\"{}\",\"ft\",\"in\")", picture.replace('"', "\"\""));
        let v = crate::eval(&ex);
        assert!(matches!(v, Value::Text(_) | Value::Error(_) | Value::Unsupported(_)), "{ex}: {v:?}");
    }
    within_bound(started);
    // Most random pictures give text, so the loop worked the engine and did not only fail fast.
    assert!(texts > 1500, "{texts} of 4000 gave text");
}

#[test]
fn random_arguments_never_panic_in_any_function() {
    // Every function the registry holds, called with random argument lists from a pool that holds the awkward values (huge, tiny, empty,
    // errors, a point, a colour, a reference to a cell and to the text). Only the absence of a panic or a hang is asserted.
    let pool = [
        "0",
        "1",
        "-1",
        "2.5",
        "1E300",
        "-1E300",
        "1E-300",
        "\"\"",
        "\"abc\"",
        "\"é\"",
        "\"1;2;3\"",
        "TRUE",
        "3 in",
        "30 deg",
        "PNT(1,2)",
        "NA()",
        "REF()",
        "Width",
        "TheText",
        "PinX",
        "7/0",
        "DATE(2031,3,7)",
        "#ff0000",
        "65536",
        "-32769",
        "2147483648",
        "Sheet.1!Width",
        "Geometry1.Path",
        "User.Probe",
    ];
    let mut state = 0x2545_F491_4F6C_DD1Du64;
    let mut next = move |n: usize| {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state % n as u64) as usize
    };
    let names = diacraft_calc::function_names();
    let started = Instant::now();
    let mut calls = 0;
    for name in names {
        for _ in 0..60 {
            let args: Vec<&str> = (0..next(7)).map(|_| pool[next(pool.len())]).collect();
            let formula = format!("{name}({})", args.join(","));
            let v = crate::eval_on(|s| s.text("Oak\n"), &formula);
            calls += 1;
            // Never `Sheet`: no function hands a bare sheet to a cell as its result here.
            assert!(!matches!(v, Value::Sheet(_)) || name.contains("SHEET"), "{formula}: {v:?}");
        }
    }
    within_bound(started);
    assert!(calls > 10_000, "{calls} calls");
}

/// `F_0` is `1`, and `F_{k+1}` is the string `F_k` plus 1: a number read through `k` strings, each read as a formula.
fn nested_strings(levels: usize) -> String {
    let mut f = "1".to_owned();
    for _ in 0..levels {
        f = format!("\"{}\"+1", f.replace('"', "\"\""));
    }
    f
}

#[test]
fn strings_read_as_formulas_nest_to_the_limit_and_no_further() {
    // The innermost `1` is read as a number, and each string above it as a formula: 9 levels are worked (the sum is 10), 10 are not.
    let limit = diacraft_calc::rules::MAX_STRING_FORMULA_DEPTH;
    let at_limit = crate::eval(&nested_strings(limit + 1));
    assert_eq!(mismatch(&at_limit, &E::N((limit + 2) as f64)), None, "{at_limit:?}");
    let past = crate::eval(&nested_strings(limit + 2));
    assert_eq!(mismatch(&past, &E::Err(diacraft_calc::CellError::Value)), None, "{past:?}");
}

#[test]
fn evalcell_nests_to_the_limit_and_no_further() {
    // `User.R{k}` is `EVALCELL(User.R{k+1}, ...) + 1` and the last is 0: the value at `R0` is the number of calls, 64 at most.
    let limit = diacraft_calc::rules::MAX_NESTED_EVALUATIONS;
    let chain = |n: usize| {
        let mut shape = ShapeBuilder::new(1);
        for k in 0..n {
            shape = shape.row_formula("User", None, RowKey::name(&format!("R{k}")), "Value", &format!("EVALCELL(User.R{},\"x\",0)+1", k + 1), "0");
        }
        shape = shape.row_formula("User", None, RowKey::name(&format!("R{n}")), "Value", "0", "0");
        let doc = DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build();
        evaluate(&doc, shape_at(0), CellAddr::in_row("User", None, RowKey::name("R0"), "Value"), EvalOptions::default(), Budget::default()).0
    };
    assert_eq!(mismatch(&chain(limit), &E::N(limit as f64)), None, "{:?}", chain(limit));
    assert!(matches!(chain(limit + 1), Value::Error(_)), "{:?}", chain(limit + 1));
}

/// `n` cells, each reading the next through a sheet-returning call (`SHEETREF()!User.C<k>`), which the walk of names cannot see, so that
/// each is read in place with the native frames of the one above it. The last is 0, and the first is `n`.
fn hidden_chain(n: u32) -> Document {
    let mut shape = ShapeBuilder::new(1);
    for k in 0..n {
        shape = shape.row_formula("User", None, RowKey::name(&format!("C{k}")), "Value", &format!("SHEETREF()!User.C{}+1", k + 1), "0");
    }
    shape = shape.row_formula("User", None, RowKey::name(&format!("C{n}")), "Value", "0", "0");
    DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build()
}

/// `n` top-level shapes, the pin of each carried by a `LOCTOLOC` whose source is the next shape: the carry reads the next shape's `PinX`
/// from inside the function, which the walk of names cannot see either (M1: 170 links of this overflowed 512 KiB in release).
fn carry_chain(n: u32) -> Document {
    let mut page = PageBuilder::new();
    for k in 1..=n {
        let mut shape = ShapeBuilder::new(k);
        shape = if k < n {
            shape.formula("PinX", &format!("PNTX(LOCTOLOC(PNT(1,1),Sheet.{}!Width,ThePage!PageWidth))", k + 1), "0")
        } else {
            shape.cell("PinX", "1")
        };
        page = page.shape(shape);
    }
    DocBuilder::new().page(1, page).build()
}

#[test]
fn chains_the_walk_of_names_cannot_see_stay_inside_a_half_megabyte_stack() {
    // M1: native frames follow the chain, one level for each cell read in place, and the depth limit counts a cell read as many levels as its
    // frames weigh, so a chain of 170 or 400 ends as an error at the limit and never overflows 512 KiB, in this profile.
    for n in [50u32, 170, 400] {
        let doc = hidden_chain(n);
        let started = Instant::now();
        let (v, ctx) = small_stack(move || {
            evaluate(&doc, shape_at(0), CellAddr::in_row("User", None, RowKey::name("C0"), "Value"), EvalOptions::default(), Budget::default())
        });
        within_bound(started);
        assert!(matches!(v, Value::Number(_) | Value::Error(_)), "hidden chain of {n}: {v:?}");
        if n >= 170 {
            assert!(ctx.stats.depth_limit_hits > 0, "hidden chain of {n} met no limit: {:?}", ctx.stats);
        }
        let doc = carry_chain(n);
        let started = Instant::now();
        let (v, _) = small_stack(move || evaluate(&doc, shape_at(0), CellAddr::direct("PinX"), EvalOptions::default(), Budget::default()));
        within_bound(started);
        assert!(matches!(v, Value::Number(_) | Value::Error(_)), "carry chain of {n}: {v:?}");
    }
}
