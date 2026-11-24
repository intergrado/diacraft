//! The oracle's scores and `commit`'s kind check, case by case (items 3 and 4): the places where a changed comparison, a
//! dropped name or a dropped field changed a score and no test said so.

use std::sync::Arc;

use diacraft_calc::rules::MAX_DIFFERENCES;
use diacraft_calc::{
    Budget, Difference, DifferenceKind, EvalOptions, EvalStats, FunctionScore, NotFormulas, OperandMode, RecalcReport, SheetPlace, Skipped,
    ValueClass, commit, recalc_all, recalc_all_with,
};
use diacraft_doc::{Cell, CellAddr, Document, Owner, RowKey, ShapeRef, Value as DocValue};
use diacraft_testkit::build::{DocBuilder, MasterBuilder, PageBuilder, ShapeBuilder};

fn x() -> CellAddr {
    CellAddr::in_row("User", None, RowKey::name("X"), "Value")
}

fn cell(formula: &str, v: &str, unit: Option<&str>, e: Option<&str>) -> Cell {
    let mut c = Cell::with_formula(formula, Some(DocValue::from_text(v)));
    if let Some(u) = unit {
        c = c.with_unit(u);
    }
    c.error = e.map(Arc::from);
    c
}

fn doc_of(shape: ShapeBuilder) -> Document {
    DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build()
}

fn accounted(r: &RecalcReport) -> u64 {
    r.agreed + r.differed.len() as u64 + r.differed_unlisted + r.skipped.total() + r.unsupported_cells() + r.unreached
}

/// The score of one cell `User.X` with `formula` and the saved `v`: `true` when it agreed.
fn agrees(formula: &str, v: &str, unit: Option<&str>) -> bool {
    let r = recalc_all(&doc_of(ShapeBuilder::new(1).set(x(), cell(formula, v, unit, None))), &Budget::default());
    assert_eq!(r.cells, accounted(&r), "{formula} against {v}: {r:?}");
    assert_eq!(r.agreed + r.differed.len() as u64, 1, "{formula} against {v}: {r:?}");
    r.agreed == 1
}

#[test]
fn the_atan2_allowances_are_atan2s_and_each_check_of_them_counts() {
    // Gives `ATAN2` two allowances and no other call any. A pi checked within 1E-10 and a length under 1E-6.
    let pi = "3.141592653589793";
    // The allowance itself: a whole turn from the saved value, both of the size of pi.
    assert!(agrees("ATAN2(0,-1)", &format!("-{pi}"), None), "the turn");
    assert!(agrees("ATAN2(3E-8,-2E-8)", "0.4", None), "no length");
    // Another call with two arguments gets neither: the same numbers, a `MIN` and a `MAX`.
    assert!(!agrees(&format!("MIN({pi},4)"), &format!("-{pi}"), None), "a turn is no allowance of MIN");
    assert!(!agrees("MAX(1E-8,2E-8)", "0.4", None), "no length is no allowance of MAX");
    // Each size check alone: the computed value 5E-10 from pi (ATAN2 of a tiny y and -1), the saved one 5E-10 from -pi, three pi.
    assert!(!agrees("ATAN2(5E-10,-1)", &format!("-{pi}"), None), "the computed value is not within 1E-10 of pi");
    assert!(!agrees("ATAN2(0,-1)", "-3.141592653089793", None), "the saved value is not within 1E-10 of pi");
    assert!(!agrees("ATAN2(0,-1)", "9.42477796076938", None), "three pi is a whole turn from pi and not of the size of pi");
    // The operands' sizes: both must be under 1E-6, each alone.
    assert!(!agrees("ATAN2(3E-8,-2)", "0.4", None), "x is not small");
    assert!(!agrees("ATAN2(2,-3E-8)", "0.4", None), "y is not small");
    assert!(!agrees("ATAN2(1E-7,-1)", "-3.1415926", None), "x is not small, and 1E-7 is not zero");
}

#[test]
fn a_merged_report_is_the_sum_of_every_count_of_both() {
    // `RecalcReport::merge` sums a source's files; it dropped no field in the first round, and nothing said that it would not.
    let diff = |cell: &str| Difference {
        sheet: "page:1".into(),
        cell: cell.into(),
        place: SheetPlace::Page(1),
        address: CellAddr::direct(cell),
        inherited: false,
        functions: vec!["SUM".into()],
        kind: DifferenceKind::Value,
        saved: Some("1".into()),
        saved_class: Some(ValueClass::Number),
        saved_error: None,
        actual: Some("2".into()),
        actual_class: Some(ValueClass::Number),
        actual_error: None,
    };
    let mut a = RecalcReport {
        agreed: 1,
        differed: vec![diff("a")],
        differed_unlisted: 2,
        unsupported: [("PATHLENGTH".to_owned(), 3), ("TEXTWIDTH".to_owned(), 4)].into(),
        skipped: Skipped {
            actions: 1,
            parse_failed: 2,
            clock_readers: 3,
            themed: 4,
            nothing_saved: 5,
            inherited_no_formula: 6,
            far_year: 7,
            stale_theme_colour: 8,
            machine_font_number: 34,
            text_stand_in: 37,
            theme_no_shadow: 35,
            formula_must_fail: 36,
            themed_operand: 32,
            far_year_operand: 33,
            depth_limit: 9,
            ambiguous_sheet: 21,
        },
        not_formulas: NotFormulas { value_only: 22, no_formula: 23, deleted: 24, with_saved_error: 25 },
        by_cell: [(("User".to_owned(), "Value".to_owned()), FunctionScore { agreed: 26, differed: 27 })].into(),
        agreed_by_allowance: 28,
        by_function: [("SUM".to_owned(), FunctionScore { agreed: 10, differed: 11 })].into(),
        cells: 12,
        unreached: 13,
        stats: EvalStats { steps: 16, cycles: 17, depth_limit_hits: 18, bytes_peak: 19, ..EvalStats::default() },
        ..RecalcReport::default()
    };
    a.document_date_checks.fit = 14;
    a.document_date_checks.not_fit = 15;
    let mut b = RecalcReport {
        agreed: 100,
        differed: vec![diff("b")],
        differed_unlisted: 200,
        unsupported: [("PATHLENGTH".to_owned(), 300), ("THEMECBV".to_owned(), 400)].into(),
        skipped: Skipped {
            actions: 100,
            parse_failed: 200,
            clock_readers: 300,
            themed: 400,
            nothing_saved: 500,
            inherited_no_formula: 600,
            far_year: 700,
            stale_theme_colour: 800,
            machine_font_number: 3400,
            text_stand_in: 3700,
            theme_no_shadow: 3500,
            formula_must_fail: 3600,
            themed_operand: 3200,
            far_year_operand: 3300,
            depth_limit: 900,
            ambiguous_sheet: 2100,
        },
        by_function: [
            ("SUM".to_owned(), FunctionScore { agreed: 1000, differed: 1100 }),
            ("MAX".to_owned(), FunctionScore { agreed: 5, differed: 6 }),
        ]
        .into(),
        not_formulas: NotFormulas { value_only: 2200, no_formula: 2300, deleted: 2400, with_saved_error: 2500 },
        by_cell: [
            (("User".to_owned(), "Value".to_owned()), FunctionScore { agreed: 2600, differed: 2700 }),
            (("".to_owned(), "Width".to_owned()), FunctionScore { agreed: 5, differed: 6 }),
        ]
        .into(),
        agreed_by_allowance: 2800,
        cells: 1200,
        unreached: 1300,
        budget_exhausted: true,
        memory_exhausted: true,
        stats: EvalStats { steps: 1600, cycles: 1700, depth_limit_hits: 1800, bytes_peak: 1900, ..EvalStats::default() },
        ..RecalcReport::default()
    };
    b.document_date_checks.fit = 1400;
    b.document_date_checks.not_fit = 1500;
    a.merge(b);
    assert_eq!((a.agreed, a.differed_unlisted, a.cells, a.unreached), (101, 202, 1212, 1313));
    assert_eq!(a.differed.iter().map(|d| d.cell.as_str()).collect::<Vec<_>>(), ["a", "b"]);
    assert_eq!(a.unsupported.get("PATHLENGTH"), Some(&303));
    assert_eq!((a.unsupported.get("TEXTWIDTH"), a.unsupported.get("THEMECBV")), (Some(&4), Some(&400)));
    let s = a.skipped;
    assert_eq!(
        (
            s.actions,
            s.parse_failed,
            s.clock_readers,
            s.themed,
            s.nothing_saved,
            s.inherited_no_formula,
            s.far_year,
            s.stale_theme_colour,
            s.depth_limit,
            s.ambiguous_sheet
        ),
        (101, 202, 303, 404, 505, 606, 707, 808, 909, 2121)
    );
    // The reasons added are summed too.
    assert_eq!((s.themed_operand, s.far_year_operand), (3232, 3333));
    // And the three added.
    assert_eq!((s.machine_font_number, s.theme_no_shadow, s.formula_must_fail), (3434, 3535, 3636));
    // And the one added: cells measured in a stand-in face.
    assert_eq!(s.text_stand_in, 3737);
    let n = a.not_formulas;
    assert_eq!((n.value_only, n.no_formula, n.deleted, n.with_saved_error), (2222, 2323, 2424, 2525));
    assert_eq!(a.by_cell.get(&("User".to_owned(), "Value".to_owned())), Some(&FunctionScore { agreed: 2626, differed: 2727 }));
    assert_eq!(a.by_cell.get(&(String::new(), "Width".to_owned())), Some(&FunctionScore { agreed: 5, differed: 6 }));
    assert_eq!(a.agreed_by_allowance, 2828);
    assert_eq!(a.by_function.get("SUM"), Some(&FunctionScore { agreed: 1010, differed: 1111 }));
    assert_eq!(a.by_function.get("MAX"), Some(&FunctionScore { agreed: 5, differed: 6 }));
    assert!(a.budget_exhausted && a.memory_exhausted);
    assert_eq!((a.document_date_checks.fit, a.document_date_checks.not_fit), (1414, 1515));
    assert_eq!((a.stats.steps, a.stats.cycles, a.stats.depth_limit_hits, a.stats.bytes_peak), (1616, 1717, 1818, 1900));
}

#[test]
fn a_merge_keeps_the_list_of_differences_to_its_cap_and_counts_the_rest() {
    let diff = |n: usize| Difference {
        sheet: "page:1".into(),
        cell: format!("c{n}"),
        place: SheetPlace::Page(1),
        address: CellAddr::direct(&format!("c{n}")),
        inherited: false,
        functions: Vec::new(),
        kind: DifferenceKind::Error,
        saved: None,
        saved_class: None,
        saved_error: None,
        actual: None,
        actual_class: None,
        actual_error: None,
    };
    let mut a = RecalcReport { differed: (0..MAX_DIFFERENCES - 1).map(diff).collect(), ..RecalcReport::default() };
    let b = RecalcReport { differed: (0..3).map(diff).collect(), differed_unlisted: 5, ..RecalcReport::default() };
    a.merge(b);
    assert_eq!((a.differed.len(), a.differed_unlisted), (MAX_DIFFERENCES, 5 + 2));
}

fn field_shape(id: u32, field_formula: &str, readers: &[(&str, &str)]) -> ShapeBuilder {
    let mut s = ShapeBuilder::new(id).text("Plant").row_formula("Field", None, RowKey::Ix(0), "Value", field_formula, "abc");
    for (name, formula) in readers {
        s = s.set(CellAddr::in_row("User", None, RowKey::name(name), "Value"), cell(formula, "0", None, None));
    }
    s
}

#[test]
fn a_bare_reference_to_a_text_follows_the_fields_of_that_text_for_the_clock() {
    // `ISERROR(Sheet.1!TheText)` names the text with no `SHAPETEXT` around it: it is still a read of the text, whose field calls `NOW()`.
    let page = PageBuilder::new().shape(field_shape(1, "NOW()", &[])).shape(field_shape(
        2,
        "ID()",
        &[("A", "ISERROR(Sheet.1!TheText)"), ("B", "ISERROR(TheText)")],
    ));
    let doc = DocBuilder::new().page(1, page).build();
    let r = recalc_all(&doc, &Budget::default());
    // Shape 1's field (it names NOW) and `A`, which names its text; `B` names shape 2's own, clean text and is scored.
    assert_eq!(r.skipped.clock_readers, 2, "{r:?}");
    assert_eq!(r.cells, accounted(&r), "{r:?}");
    assert_eq!(r.agreed + r.differed.len() as u64, 2, "shape 2's field and B: {r:?}");
}

#[test]
fn a_cell_that_reads_the_clock_by_a_hidden_read_taints_each_cell_that_reads_it_even_from_the_memo() {
    // `SHEETREF()!User.B` hides the read from the walk of names. B reads C, which calls the clock; A reads B first, and R reads B after it
    // has been worked out and remembered: the clock is carried with the cell, as the depth limit is.
    let shape = ShapeBuilder::new(1)
        .set(CellAddr::in_row("User", None, RowKey::name("A"), "Value"), cell("SHEETREF()!User.B+1", "1", None, None))
        .set(CellAddr::in_row("User", None, RowKey::name("B"), "Value"), cell("SHEETREF()!User.C+1", "1", None, None))
        .set(CellAddr::in_row("User", None, RowKey::name("C"), "Value"), cell("NOW()", "1", None, None))
        .set(CellAddr::in_row("User", None, RowKey::name("R"), "Value"), cell("SHEETREF()!User.B+2", "1", None, None));
    let r = recalc_all_with(&doc_of(shape), &Budget::default(), EvalOptions { operands: OperandMode::Live, ..EvalOptions::default() });
    assert_eq!((r.skipped.clock_readers, r.agreed, r.differed.len()), (4, 0, 0), "{r:?}");
}

// ---- commit's kind check (item 3) and a stale error (C05b) -------------------------------------------------------------------

fn text_of(doc: &Document, addr: &CellAddr) -> (Option<String>, Option<String>) {
    let sheet = &doc.shape(&ShapeRef::top(Owner::Page(1), 0)).expect("shape").sheet;
    match sheet.lookup(addr) {
        diacraft_doc::Lookup::Found(c) => (c.value.as_ref().map(|v| v.text().to_owned()), c.error.as_deref().map(str::to_owned)),
        other => panic!("{other:?}"),
    }
}

#[test]
fn commit_writes_nothing_for_a_cell_whose_chain_holds_no_formula() {
    // An `Inh` cell that nothing supplies a formula for is not a formula cell: it was a constant the default of its kind could replace
    // (`0` over a saved 777), and the kind check is what keeps it. It is skipped, and counted so.
    let mut doc = doc_of(ShapeBuilder::new(1).inh("PinX", Some("777")).inh("Width", Some("5")));
    let r = commit(&mut doc, &Budget::default(), EvalOptions::default());
    assert_eq!((r.written, r.unchanged, r.skipped, r.added), (0, 0, 2, 0), "{r:?}");
    assert_eq!(text_of(&doc, &CellAddr::direct("PinX")), (Some("777".into()), None));
    // And over a master that holds the constant.
    let master = MasterBuilder::new().shape(ShapeBuilder::new(1).cell("Width", "5"));
    let mut doc =
        DocBuilder::new().master(1, master).page(1, PageBuilder::new().shape(ShapeBuilder::new(10).master(1).inh("Width", Some("999")))).build();
    let r = commit(&mut doc, &Budget::default(), EvalOptions::default());
    assert_eq!((r.written, r.unchanged, r.skipped), (0, 0, 1), "{r:?}");
    assert_eq!(text_of(&doc, &CellAddr::direct("Width")), (Some("999".into()), None));
}

#[test]
fn a_new_value_does_not_keep_the_stale_error_of_the_old() {
    // C05b: the file says `#REF!` for a cell that works out to 4 now, and holds 5 as its last good value. The value written is 4 and the error
    // is gone. (The value that agrees keeps its text and loses only the error: `commit_cases`.)
    let mut doc = doc_of(ShapeBuilder::new(1).set(x(), cell("2+2", "5", None, Some("#REF!"))));
    let r = commit(&mut doc, &Budget::default(), EvalOptions::default());
    assert_eq!((r.written, r.skipped), (1, 0), "{r:?}");
    assert_eq!(text_of(&doc, &x()), (Some("4".into()), None));
    // The same, the value agreeing with the one saved but the error stale: the text of the value stays, the error goes.
    let mut doc = doc_of(ShapeBuilder::new(1).set(x(), cell("2+2", "4.0", None, Some("#REF!"))));
    let r = commit(&mut doc, &Budget::default(), EvalOptions::default());
    assert_eq!(r.written, 1, "{r:?}");
    assert_eq!(text_of(&doc, &x()), (Some("4.0".into()), None));
}

// ---- the comparison of two values of kinds that never agree (item 4) -----------------------------------------------------------

#[test]
fn two_values_of_kinds_that_do_not_compare_never_agree_in_the_oracle() {
    // `values_agree` ends in an arm for every pair no rule above it compares: that arm is `false`. A `true` there passed the whole oracle
    // suite and was caught only by a commit test. Each pair is a result of one kind against a saved value of another.
    let rows: &[(&str, &str, Option<&str>)] = &[
        // A truth value against a text.
        ("1=1", "yes", Some("STR")),
        // A text against a truth value.
        ("\"yes\"", "1", Some("BOOL")),
        // A point against a number, and a number against a point.
        ("PNT(1,2)", "1", None),
        ("1", "PNT(1,2)", Some("PNT")),
        // A constructor against a number.
        ("POLYLINE(1,1,2,2)", "0", None),
        // A colour against a text that spells it (a colour is not a string), and a text against a colour.
        ("RGB(1,2,3)", "#010203", Some("STR")),
        // A currency against a text.
        ("CY(12.5,\"EUR\")", "twelve", Some("STR")),
        // A point against a constructor.
        ("PNT(1,2)", "POLYLINE(1, 1, 2, 2)", Some("POLYLINE")),
    ];
    for (formula, saved, unit) in rows {
        assert!(!agrees(formula, saved, *unit), "`{formula}` against `{saved}` {unit:?} agreed");
    }
}
