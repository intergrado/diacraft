//! The shapes the review projected to hours (C3) and to gigabytes (C4), each built at a size that is still hostile and run to its end under
//! a hang guard (a minute: no cost is held to a time). Every one of them ends because its work is charged in nodes and bytes before it is done, or because its memory is
//! accounted and capped; the budgets here are small so that a test is quick, and the report of the fix round gives the same shapes at the
//! default limits, measured in a release build.

use std::sync::Arc;
use std::time::{Duration, Instant};

use diacraft_calc::{Budget, EvalOptions, OperandMode, RecalcReport, commit, recalc_all_with};
use diacraft_doc::{Document, RowKey, TextPart};
use diacraft_testkit::build::{DocBuilder, MasterBuilder, PageBuilder, ShapeBuilder, StyleBuilder};

/// A hang guard and no measure of cost: the slowest of these takes about a second at load 50.
const BOUND: Duration = Duration::from_secs(60);

fn accounted(r: &RecalcReport) -> u64 {
    r.agreed + r.differed.len() as u64 + r.differed_unlisted + r.skipped.total() + r.unsupported_cells() + r.unreached
}

/// Runs the oracle, checks the time and that every cell is accounted for, and gives the report.
fn run(doc: &Document, steps: u64, operands: OperandMode) -> RecalcReport {
    let started = Instant::now();
    let r = recalc_all_with(doc, &Budget::with_steps(steps), EvalOptions { operands, ..EvalOptions::default() });
    let took = started.elapsed();
    assert!(took < BOUND, "took {took:?}: {r:?}");
    assert_eq!(r.cells, accounted(&r), "{r:?}");
    assert!(r.stats.steps <= steps, "{} steps of {steps}", r.stats.steps);
    r
}

/// A master of one shape, and `n` instances that hold `Inh` cells for `names`, each with the saved value `saved`.
fn instances(master: ShapeBuilder, n: u32, names: &[&str], saved: &str) -> Document {
    let mut page = PageBuilder::new();
    for i in 0..n {
        let mut s = ShapeBuilder::new(i + 1).master(1);
        for name in names {
            s = s.row_inh("User", None, RowKey::name(name), "Value", Some(saved));
        }
        page = page.shape(s);
    }
    DocBuilder::new().master(1, MasterBuilder::new().shape(master)).page(1, page).build()
}

fn formula_row(name: &str, formula: &str) -> ShapeBuilder {
    ShapeBuilder::new(1).row_formula("User", None, RowKey::name(name), "Value", formula, "0")
}

#[test]
fn five_thousand_function_names_in_a_branch_not_taken() {
    // The names of a formula were collected for each cell by a search of the list met so far (quadratic), uncharged, and scored one by one for
    // each cell: 38 hours at the old budget. One walk per formula, and a step for each name a cell scores.
    let names: Vec<String> = (1..=5000).map(|i| format!("F{i}(1)")).collect();
    let doc = instances(formula_row("X", &format!("IF(1=1,1,SUM({}))", names.join(","))), 600, &["X"], "1");
    let r = run(&doc, 2_000_000, OperandMode::Cached);
    assert!(r.budget_exhausted && r.unreached > 0, "{r:?}");
    assert!(r.agreed > 100 && r.stats.steps >= 1_900_000, "{r:?}");
}

#[test]
fn a_megabyte_of_polyline_read_again_by_each_reader() {
    // A `V` of a megabyte is decoded at each Cached read, and each read is charged its length.
    let numbers: Vec<String> = (0..180_000).map(|i| format!("{}.5", i % 90)).collect();
    let big = format!("POLYLINE({})", numbers.join(", "));
    let mut page = PageBuilder::new().shape(ShapeBuilder::new(1).cell_u("Comment", &big, "POLYLINE"));
    for i in 0..3_000u32 {
        page = page.shape(ShapeBuilder::new(i + 2).row_formula("User", None, RowKey::name("R"), "Value", "ISERROR(Sheet.1!Comment)", "0"));
    }
    let doc = DocBuilder::new().page(1, page).build();
    let r = run(&doc, 1_000_000, OperandMode::Cached);
    assert!(r.budget_exhausted && r.unreached > 2_000, "{r:?}");
}

#[test]
fn a_point_of_sixty_thousand_references() {
    let side = |cell: &str| format!("SUM({})", vec![cell; 25_000].join(","));
    let doc = instances(formula_row("P", &format!("PNTX(PNT({},{}))", side("Width"), side("Height"))), 200, &["P"], "0");
    let r = run(&doc, 2_000_000, OperandMode::Cached);
    assert!(r.budget_exhausted && r.unreached > 100, "{r:?}");
}

#[test]
fn a_megabyte_of_text_cased_in_each_of_many_cells() {
    let doc = instances(formula_row("U", r#"LEN(UPPER(REPT("x",1048576)))"#), 2_000, &["U"], "0");
    let r = run(&doc, 2_000_000, OperandMode::Cached);
    assert!(r.budget_exhausted && r.unreached > 1_000, "{r:?}");
}

#[test]
fn the_text_read_of_a_shape_with_sixteen_thousand_fields_and_as_many_readers() {
    let mut s = ShapeBuilder::new(1).text("abc");
    for k in 0..16_384u32 {
        s = s.row_formula("Field", None, RowKey::Ix(k), "Value", "1+1", "2");
        s = s.row_formula("User", None, RowKey::name(&format!("R{k}")), "Value", "LEN(SHAPETEXT(TheText))", "3");
    }
    let doc = DocBuilder::new().page(1, PageBuilder::new().shape(s)).build();
    // The clock walk is done once for the sheet's text and remembered: linear, and no cell reaches the end of the budget for it.
    let r = run(&doc, Budget::DEFAULT_STEPS, OperandMode::Cached);
    assert!(!r.budget_exhausted && r.stats.steps < 2_000_000, "{r:?}");
}

#[test]
fn a_text_of_two_hundred_thousand_markers_read_by_many_cells() {
    let mut parts = Vec::new();
    for _ in 0..100_000 {
        parts.push(TextPart::Chars(Arc::from("a")));
        parts.push(TextPart::Cp(0));
    }
    let mut reader = ShapeBuilder::new(2);
    for k in 0..400u32 {
        reader = reader.row_formula("User", None, RowKey::name(&format!("R{k}")), "Value", "LEN(SHAPETEXT(Sheet.1!TheText))", "100000");
    }
    let doc = DocBuilder::new().page(1, PageBuilder::new().shape(ShapeBuilder::new(1).text_parts(parts)).shape(reader)).build();
    let r = run(&doc, 2_000_000, OperandMode::Cached);
    assert!(r.budget_exhausted && r.unreached > 300, "each read is 200,000 steps: {r:?}");
}

#[test]
fn id_in_an_instance_of_the_last_of_sixty_five_thousand_masters() {
    // `Document::master` scans: the maps of the resolver do not.
    let mut b = DocBuilder::new();
    for m in 1..=65_535u32 {
        b = b.master(m, MasterBuilder::new().shape(ShapeBuilder::new(1)));
    }
    b = b.master(65_536, MasterBuilder::new().shape(formula_row("I", "ID()")));
    let mut page = PageBuilder::new();
    for i in 0..20_000u32 {
        page = page.shape(ShapeBuilder::new(i + 1).master(65_536).row_inh("User", None, RowKey::name("I"), "Value", Some("0")));
    }
    let doc = b.page(1, page).build();
    let r = run(&doc, Budget::DEFAULT_STEPS, OperandMode::Cached);
    assert_eq!(r.cells, 20_001, "{r:?}");
    // And the commit of the same: it looked each sheet up by a scan.
    let mut doc = doc;
    let started = Instant::now();
    let c = commit(&mut doc, &Budget::default(), EvalOptions::default());
    assert!(started.elapsed() < BOUND, "{c:?}");
    assert_eq!(c.written + c.unchanged + c.skipped, 20_001, "{c:?}");
}

#[test]
fn bound_of_forty_thousand_ranges() {
    let ranges: Vec<String> = (0..40_000).map(|i| format!("FALSE,{},{}", i, i + 2)).collect();
    let doc = instances(formula_row("B", &format!("BOUND(20000.5,1,{})", ranges.join(","))), 200, &["B"], "0");
    let r = run(&doc, 2_000_000, OperandMode::Cached);
    assert!(r.budget_exhausted, "{r:?}");
}

#[test]
fn a_string_of_four_kilobytes_that_does_not_parse_in_each_of_many_cells() {
    let junk: String = (0..4090).map(|i| if i % 2 == 0 { '(' } else { '+' }).collect();
    let names = ["U0", "U1", "U2", "U3"];
    let mut m = ShapeBuilder::new(1);
    for n in names {
        m = m.row_formula("User", None, RowKey::name(n), "Value", &format!("\"{junk}\"+1"), "0");
    }
    let doc = instances(m, 5_000, &names, "0");
    let r = run(&doc, 2_000_000, OperandMode::Cached);
    assert!(r.budget_exhausted && r.unreached > 1_000, "each parse is 65 steps: {r:?}");
}

#[test]
fn sixty_five_thousand_masters_and_styles_that_hold_no_formula() {
    let mut b = DocBuilder::new();
    for m in 1..=65_536u32 {
        b = b.master(m, MasterBuilder::new().shape(ShapeBuilder::new(1)));
    }
    for s in 1..=65_000u32 {
        b = b.style(s, StyleBuilder::new());
    }
    let doc = b.build();
    let r = run(&doc, Budget::DEFAULT_STEPS, OperandMode::Cached);
    assert_eq!(r.cells, 0, "{r:?}");
}

// ---- memory (C4) ---------------------------------------------------------------------------------------------------------------

fn rept_cells(n: u32) -> Document {
    let mut page = PageBuilder::new();
    for i in 0..n {
        page = page.shape(ShapeBuilder::new(i + 1).row_formula("User", None, RowKey::name("A"), "Value", "REPT(\"x\",1048576)", "y"));
    }
    DocBuilder::new().page(1, page).build()
}

#[test]
fn cells_of_a_megabyte_each_end_the_run_at_the_memory_budget_in_every_mode() {
    // 3,000 cells of a megabyte were 3.0 GB with the differences, 6.0 GB live, and 6.0 GB in a commit. At a budget of 64 MiB each ends as
    // `budget_exhausted` with `memory_exhausted`, and holds no more than the budget.
    let doc = rept_cells(300);
    let budget = Budget { steps: Budget::DEFAULT_STEPS, bytes: 64 << 20 };
    for operands in [OperandMode::Cached, OperandMode::Live] {
        let started = Instant::now();
        let r = recalc_all_with(&doc, &budget, EvalOptions { operands, ..EvalOptions::default() });
        assert!(started.elapsed() < BOUND, "{operands:?}");
        if operands == OperandMode::Live {
            assert!(r.memory_exhausted && r.budget_exhausted, "{operands:?}: {r:?}");
            assert!(r.stats.bytes_peak <= 64 << 20, "{} bytes held", r.stats.bytes_peak);
        }
        assert_eq!(r.cells, accounted(&r), "{operands:?}");
        assert!(r.differed.iter().all(|d| d.actual.as_ref().is_none_or(|a| a.len() < 400)), "printed values are cut");
    }
    let mut doc = doc;
    let c = commit(&mut doc, &budget, EvalOptions::default());
    assert!(c.memory_exhausted && c.budget_exhausted && c.stats.bytes_peak <= 64 << 20, "{c:?}");
    assert!(c.written > 0 && c.written < 300, "what was gathered is written: {c:?}");
}

#[test]
fn one_master_formula_with_many_instances_that_each_read_many_cells_ends_at_the_memory_budget() {
    // Each instance holds its own memo of what its copy of the formula reads: 30,000 instances of a master formula of 2,000 references wanted
    // 30 to 60 GB.
    let refs: Vec<String> = (0..2_000).map(|k| format!("User.C{k}")).collect();
    let mut m = formula_row("X", &format!("IF(1=1,1,SUM({}))", refs.join(",")));
    for k in 0..2_000u32 {
        m = m.row_cell("User", None, RowKey::name(&format!("C{k}")), "Value", "1");
    }
    let mut doc = instances(m, 3_000, &["X"], "1");
    let budget = Budget { steps: Budget::DEFAULT_STEPS, bytes: 64 << 20 };
    let started = Instant::now();
    let c = commit(&mut doc, &budget, EvalOptions::default());
    assert!(started.elapsed() < BOUND, "{c:?}");
    assert!(c.stats.bytes_peak <= 64 << 20, "{c:?}");
    let r = recalc_all_with(&doc, &budget, EvalOptions { operands: OperandMode::Live, ..EvalOptions::default() });
    assert!(r.stats.bytes_peak <= 64 << 20 && r.cells == accounted(&r), "{r:?}");
}
