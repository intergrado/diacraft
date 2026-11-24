//! The oracle: `recalc_all` scores each formula cell against the saved `V` and `E`, and leaves out what it must.

use std::sync::Arc;

use diacraft_calc::{Budget, DifferenceKind, EvalOptions, OperandMode, RecalcReport, recalc_all, recalc_all_with};
use diacraft_doc::{Cell, CellAddr, Document, RowKey};
use diacraft_testkit::build::{DocBuilder, MasterBuilder, PageBuilder, ShapeBuilder};

use crate::fixed_clock;

fn doc_with(shape: ShapeBuilder) -> Document {
    DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build()
}

fn score(shape: ShapeBuilder) -> RecalcReport {
    recalc_all(&doc_with(shape), &Budget::default())
}

fn with_error(mut cell: Cell, e: &str) -> Cell {
    cell.error = Some(Arc::from(e));
    cell
}

#[test]
fn a_cell_that_agrees_counts_and_one_that_differs_is_listed_by_name() {
    let report = score(ShapeBuilder::new(1).cell("Width", "2").formula("Height", "Width*2", "4").formula("PinX", "Width*2", "5").formula(
        "PinY",
        "SUM(Width,Width)",
        "4.0000000001",
    ));
    assert_eq!(report.agreed, 2, "{report:?}");
    assert_eq!(report.differed.len(), 1);
    let d = &report.differed[0];
    assert_eq!((d.cell.as_str(), d.kind), ("PinX", DifferenceKind::Value));
    assert_eq!(d.functions, Vec::<String>::new());
    assert_eq!((d.saved.as_deref(), d.actual.as_deref()), (Some("5 [DL]"), Some("4 [DL]")), "expected and actual values cross");
    assert_eq!(report.by_function.get("SUM").map(|s| (s.agreed, s.differed)), Some((1, 0)));
}

#[test]
fn a_difference_holds_no_formula_text() {
    // AGENTS.md: a sample-file report crosses with names, counts and values, never the text of a formula.
    let secret = "Width*77";
    let report = score(ShapeBuilder::new(1).cell("Width", "2").formula("PinX", secret, "5"));
    let printed = format!("{report:?}");
    assert!(!printed.contains(secret), "{printed}");
    assert_eq!(report.differed.len(), 1);
}

#[test]
fn the_error_is_compared_as_well_as_the_value() {
    // A formula that fails and holds no `E` differs by its error; an `E` that is right agrees. (A formula that can only
    // fail, a reached `REF()`, has been left out; it is `Sheet.9321!`, a shape the page lacks, that differs here, as a
    // lookup that is wrong would look the same.)
    let report = score(
        ShapeBuilder::new(1)
            .formula("PinX", "Sheet.9321!Width*3", "6")
            .set(CellAddr::direct("PinY"), with_error(Cell::with_formula("SQRT(-4)", Some(diacraft_doc::Value::from_text("5"))), "#NUM!"))
            .set(CellAddr::direct("Width"), with_error(Cell::with_formula("2", Some(diacraft_doc::Value::from_text("2"))), "#N/A")),
    );
    assert_eq!(report.agreed, 1, "the #NUM! cell agrees: {report:?}");
    assert_eq!(report.differed.len(), 2);
    let pin_x = report.differed.iter().find(|d| d.cell == "PinX").unwrap();
    assert_eq!((pin_x.kind, pin_x.actual_error.as_deref(), pin_x.saved_error.as_deref()), (DifferenceKind::Error, Some("#REF!"), None));
    let width = report.differed.iter().find(|d| d.cell == "Width").unwrap();
    assert_eq!((width.kind, width.actual_error.as_deref(), width.saved_error.as_deref()), (DifferenceKind::Error, None, Some("#N/A")));
}

#[test]
fn a_function_that_is_not_here_is_counted_by_name_and_not_scored() {
    let report = score(ShapeBuilder::new(1).formula("PinX", "FOO(1)+1", "7").formula("PinY", "bar(2)", "7").formula("Width", "FOO(3)", "7"));
    assert_eq!(report.agreed, 0);
    assert!(report.differed.is_empty());
    assert_eq!(report.unsupported.get("FOO"), Some(&2));
    assert_eq!(report.unsupported.get("BAR"), Some(&1));
}

#[test]
fn what_is_left_out_is_counted() {
    let setup = ShapeBuilder::new(1)
        // An action cell is never worked out.
        .row_formula("Actions", None, RowKey::name("Go"), "Action", "1+1", "7")
        // A formula that does not parse keeps its saved value.
        .formula("PinX", "1 +* 2", "5")
        // Nothing saved.
        .set(CellAddr::direct("PinY"), Cell::with_formula("1+1", None))
        // The theme marker.
        .formula("Width", "2", "Themed");
    let report = score(setup);
    assert_eq!((report.agreed, report.differed.len()), (0, 0), "{report:?}");
    assert_eq!(report.skipped.actions, 1);
    assert_eq!(report.skipped.parse_failed, 1);
    assert_eq!(report.skipped.nothing_saved, 1);
    assert_eq!(report.skipped.themed, 1);
}

#[test]
fn a_cell_that_reads_the_clock_is_skipped_directly_and_through_what_it_reads() {
    // And their vectors: the clock-reader is the cell whose evaluation reaches NOW, and each cell that names it.
    let shape = ShapeBuilder::new(1)
        .row_formula("User", None, RowKey::name("A"), "Value", "NOW()", "2031-03-07T16:05:09")
        .row_formula("User", None, RowKey::name("B"), "Value", "User.A+1", "1")
        .row_formula("User", None, RowKey::name("C"), "Value", "User.B*2", "2")
        // A branch not taken never reaches the clock, so the cell is scored.
        .formula("PinX", "IF(1=1,5,NOW())", "5")
        .formula("PinY", "IF(1=2,5,NOW()+1 ed)", "99")
        .formula("Width", "RAND()", "0.5");
    let doc = doc_with(shape);
    let options = EvalOptions { clock: fixed_clock("2031-03-07T16:05:09", 0), operands: OperandMode::Cached, ..EvalOptions::default() };
    let report = recalc_all_with(&doc, &Budget::default(), options);
    assert_eq!(report.skipped.clock_readers, 5, "{report:?}");
    assert_eq!(report.agreed, 1, "the cell that does not reach the clock is scored against 5: {report:?}");
}

#[test]
fn atan2_gets_its_two_allowances_and_nothing_else_does() {
    // A whole turn at plus or minus pi agrees; a wider allowance does not exist; a line of no length agrees with any value.
    let ok =
        score(ShapeBuilder::new(1).formula("Angle", "ATAN2(-(3-3),-1)", "-3.141592653589793").formula("PinX", "ATAN2(3E-8,-2E-8)", "0.4").formula(
            "PinY",
            "ATAN2(0,-1)",
            "3.141592653589793",
        ));
    assert_eq!((ok.agreed, ok.differed.len()), (3, 0), "{ok:?}");
    let bad = score(ShapeBuilder::new(1).formula("Angle", "ATAN2(1E-7,-1)", "-3.1415926").formula("PinX", "ATAN2(3E-5,-2E-5)", "0.4"));
    assert_eq!((bad.agreed, bad.differed.len()), (0, 2), "{bad:?}");
    // Another function with the same operands gets no allowance.
    let other = score(ShapeBuilder::new(1).formula("Angle", "ATAN(0)+ATAN2(0,-1)", "-3.141592653589793"));
    assert_eq!(other.differed.len(), 1, "{other:?}");
}

#[test]
fn a_cell_through_a_master_is_scored_on_the_instance() {
    // R24 and the specs' "through a master" counts: an `Inh` cell is worked out by the master's formula on the instance.
    let doc = DocBuilder::new()
        .master(1, MasterBuilder::new().shape(ShapeBuilder::new(5).cell("Width", "2").formula("LocPinX", "Width*0.5", "1")))
        .page(
            1,
            PageBuilder::new()
                .shape(ShapeBuilder::new(10).master(1).cell("Width", "6").inh("LocPinX", Some("3")))
                .shape(ShapeBuilder::new(11).master(1).cell("Width", "6").inh("LocPinX", Some("1"))),
        )
        .build();
    let report = recalc_all(&doc, &Budget::default());
    // The master's own cell agrees; the first instance's `Inh` agrees at 3; the second holds the master's 1 and differs.
    assert_eq!(report.agreed, 2, "{report:?}");
    assert_eq!(report.differed.len(), 1);
    assert!(report.differed[0].inherited);
    assert_eq!(report.differed[0].sheet, "page:1/shape:11");
}

#[test]
fn live_operands_show_a_wrong_cell_in_every_cell_that_reads_it() {
    // Cached: each cell is tested alone against what its operands were saved as. Live: B and C follow A's new value.
    let shape = ShapeBuilder::new(1)
        .row_formula("User", None, RowKey::name("A"), "Value", "2+2", "5")
        .row_formula("User", None, RowKey::name("B"), "Value", "User.A+1", "6")
        .row_formula("User", None, RowKey::name("C"), "Value", "User.B+1", "7");
    let doc = doc_with(shape);
    let cached = recalc_all_with(&doc, &Budget::default(), EvalOptions { operands: OperandMode::Cached, ..EvalOptions::default() });
    assert_eq!((cached.agreed, cached.differed.len()), (2, 1), "{cached:?}");
    let live = recalc_all_with(&doc, &Budget::default(), EvalOptions { operands: OperandMode::Live, ..EvalOptions::default() });
    assert_eq!((live.agreed, live.differed.len()), (0, 3), "{live:?}");
}

#[test]
fn a_report_is_a_sum_of_reports() {
    let a = score(ShapeBuilder::new(1).cell("Width", "2").formula("Height", "Width*2", "4").formula("PinX", "FOO(1)", "1"));
    let b = score(ShapeBuilder::new(1).cell("Width", "2").formula("Height", "Width*3", "4").formula("PinX", "FOO(1)", "1"));
    let mut sum = RecalcReport::default();
    sum.merge(a);
    sum.merge(b);
    assert_eq!((sum.agreed, sum.differed.len(), sum.cells), (1, 1, 4));
    assert_eq!(sum.unsupported.get("FOO"), Some(&2));
}

#[test]
fn the_budget_bounds_the_run() {
    // A document of 2,000 formula cells and a budget of 2,000 steps: the run stops, says so, and counts what it did not reach.
    let mut shape = ShapeBuilder::new(1).cell("Width", "2");
    for i in 0..2_000u32 {
        shape = shape.row_formula("Scratch", None, RowKey::Ix(i), "A", "Width*2+Width", "6");
    }
    let doc = doc_with(shape);
    let report = recalc_all(&doc, &Budget::with_steps(2_000));
    assert!(report.budget_exhausted, "{report:?}");
    assert!(report.unreached > 0 && report.agreed + (report.differed.len() as u64) < 2_000, "{report:?}");
    assert!(report.stats.steps <= 2_000, "no more steps than the budget: {}", report.stats.steps);
    // With the budget it needs, all are scored.
    let report = recalc_all(&doc, &Budget::default());
    assert_eq!((report.agreed, report.unreached, report.budget_exhausted), (2_000, 0, false), "{report:?}");
}

#[test]
fn a_dates_saved_text_is_compared_to_the_second() {
    let saved = diacraft_doc::Value::from_text("2031-03-07T16:05:09");
    let report = score(
        ShapeBuilder::new(1).set(CellAddr::direct("Width"), Cell::with_formula("DATE(2031,3,7)+16 eh+5 em+9 es", Some(saved)).with_unit("DATE")),
    );
    assert_eq!((report.agreed, report.differed.len()), (1, 0), "{report:?}");
}

#[test]
fn the_number_tolerance_is_a_billionth_and_no_wider() {
    // "Numbers agree within 1E-9 of the larger of 1 and their sizes": 4.0000000001 agrees with 4, 4.000001 and 4.00001 do not.
    let report = score(
        ShapeBuilder::new(1)
            .cell("Width", "2")
            .formula("Height", "Width*2", "4.0000000001")
            .formula("PinX", "Width*2", "4.000001")
            .formula("PinY", "Width*2", "3.99999"),
    );
    assert_eq!(report.agreed, 1, "{report:?}");
    let mut names: Vec<&str> = report.differed.iter().map(|d| d.cell.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, vec!["PinX", "PinY"]);
}

#[test]
fn the_geometry_functions_are_compared_within_a_millionth() {
    // "compare within 1E-6". 3.0000001 is 3E-8 away from 3 (relative 1E-8): it agrees for `PNTX`, and not for the
    // same number made by a plain product.
    let report = score(
        ShapeBuilder::new(1).cell("Width", "1.5").formula("PinX", "PNTX(PNT(3,4))", "3.0000001").formula("PinY", "Width*2", "3.0000001").formula(
            "LocPinX",
            "INTERSECTX(0,0,30 deg,4,0,90 deg)",
            "4.00001",
        ),
    );
    assert_eq!(report.agreed, 1, "{report:?}");
    let mut names: Vec<&str> = report.differed.iter().map(|d| d.cell.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, vec!["LocPinX", "PinY"], "a relative error of 2.5E-6 is outside the geometry tolerance too");
}
