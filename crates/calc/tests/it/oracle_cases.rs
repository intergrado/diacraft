//! The oracle, case by case (C1, C2, I3, I4, I6, I9). The review replaced `values_agree` with `_ => true` and every test
//! still passed, so each arm of the comparison, each reason to skip and each way to be counted is pinned here by a cell that goes the
//! other way if the arm, the skip or the count is wrong. The mutants these were written against are named in `m26-mutants.py` and listed
//! in the report.

use std::sync::Arc;

use diacraft_calc::{Budget, DifferenceKind, EvalOptions, OperandMode, RecalcReport, recalc_all, recalc_all_with};
use diacraft_doc::{Cell, CellAddr, Document, RowKey, Theme, Value as DocValue};
use diacraft_testkit::build::{DocBuilder, MasterBuilder, PageBuilder, ShapeBuilder, StyleBuilder};

use crate::fixed_clock;

fn x() -> CellAddr {
    CellAddr::in_row("User", None, RowKey::name("X"), "Value")
}

fn user(name: &str) -> CellAddr {
    CellAddr::in_row("User", None, RowKey::name(name), "Value")
}

/// A formula cell with its saved value, unit and error.
fn cell(formula: &str, v: &str, u: Option<&str>, e: Option<&str>) -> Cell {
    let mut c = Cell::with_formula(formula, Some(DocValue::from_text(v)));
    if let Some(u) = u {
        c = c.with_unit(u);
    }
    c.error = e.map(Arc::from);
    c
}

fn doc_of(shape: ShapeBuilder) -> Document {
    DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build()
}

/// Every formula cell is in exactly one place of the report.
fn accounted(r: &RecalcReport) -> u64 {
    r.agreed + r.differed.len() as u64 + r.differed_unlisted + r.skipped.total() + r.unsupported_cells() + r.unreached
}

fn score_one(c: Cell) -> RecalcReport {
    let r = recalc_all(&doc_of(ShapeBuilder::new(1).set(x(), c)), &Budget::default());
    assert_eq!(r.cells, accounted(&r), "{r:?}");
    r
}

/// A comparison case: a label, the formula, the saved `V`, `U` and `E`, and what is expected.
type Case<'a> = (&'a str, &'a str, &'a str, Option<&'a str>, Option<&'a str>, Want);

/// What a case expects.
#[derive(Clone, Copy, Debug)]
enum Want {
    Agree,
    Differ(DifferenceKind),
}

#[test]
fn each_arm_of_the_comparison_goes_both_ways() {
    use DifferenceKind::{Both, Error, Value};
    let rows: &[Case] = &[
        // text
        ("text equal", r#""abc""#, "abc", Some("STR"), None, Want::Agree),
        ("text differs", r#""abc""#, "abd", Some("STR"), None, Want::Differ(Value)),
        ("text, case", r#""abc""#, "ABC", Some("STR"), None, Want::Differ(Value)),
        // text against a number
        ("text and number agree", r#""12""#, "12", None, None, Want::Agree),
        ("text and number differ", r#""12""#, "13", None, None, Want::Differ(Value)),
        // Booleans
        ("bool equal", "TRUE", "1", Some("BOOL"), None, Want::Agree),
        ("bool differs", "TRUE", "0", Some("BOOL"), None, Want::Differ(Value)),
        ("bool and number agree", "1=1", "1", None, None, Want::Agree),
        ("bool and number differ", "1=1", "0", None, None, Want::Differ(Value)),
        // colours
        ("colour equal", "RGB(1,2,3)", "#010203", None, None, Want::Agree),
        ("colour differs in one channel", "RGB(1,2,3)", "#010204", None, None, Want::Differ(Value)),
        ("colour and a saved index agree", "RGB(255,0,0)", "2", None, None, Want::Agree),
        ("colour and a saved index differ", "RGB(255,0,0)", "3", None, None, Want::Differ(Value)),
        // currency
        ("currency equal", r#"CY(12.5,"EUR")"#, "12.5000EUR", Some("CY"), None, Want::Agree),
        ("currency, other code", r#"CY(12.5,"EUR")"#, "12.5000USD", Some("CY"), None, Want::Differ(Value)),
        ("currency, other amount", r#"CY(12.5,"EUR")"#, "13.5000EUR", Some("CY"), None, Want::Differ(Value)),
        // points
        ("point equal", "PNT(1,2)", "PNT(1,2)", Some("PNT"), None, Want::Agree),
        ("point differs in y", "PNT(1,2)", "PNT(1,3)", Some("PNT"), None, Want::Differ(Value)),
        ("point differs in x", "PNT(1,2)", "PNT(2,2)", Some("PNT"), None, Want::Differ(Value)),
        // constructors
        ("polyline equal", "POLYLINE(1,1,2,2)", "POLYLINE(1, 1, 2, 2)", Some("POLYLINE"), None, Want::Agree),
        ("polyline differs", "POLYLINE(1,1,2,2)", "POLYLINE(1, 1, 2, 3)", Some("POLYLINE"), None, Want::Differ(Value)),
        ("polyline, shorter", "POLYLINE(1,1,2,2)", "POLYLINE(1, 1)", Some("POLYLINE"), None, Want::Differ(Value)),
        ("nurbs against polyline", "NURBS(2,3,0,0,1,1,0,1)", "POLYLINE(2, 3, 0, 0, 1, 1, 0, 1)", Some("POLYLINE"), None, Want::Differ(Value)),
        // dates
        ("date equal", "DATE(2031,3,7)", "2031-03-07T00:00:00", Some("DATE"), None, Want::Agree),
        ("date a second off", "DATE(2031,3,7)", "2031-03-07T00:00:01", Some("DATE"), None, Want::Agree),
        ("date two seconds off", "DATE(2031,3,7)", "2031-03-07T00:00:02", Some("DATE"), None, Want::Differ(Value)),
        // mixed kinds
        ("number against text", "1+1", "abc", Some("STR"), None, Want::Differ(Value)),
        ("text against number", r#""abc""#, "2", None, None, Want::Differ(Value)),
        // the class of an error
        ("same error", "1/0", "1", None, Some("#DIV/0!"), Want::Agree),
        ("another class", "1/0", "1", None, Some("#VALUE!"), Want::Differ(Error)),
        ("an error the file does not have", "1/0", "1", None, None, Want::Differ(Error)),
        ("an error the file has and the formula does not", "1+1", "2", None, Some("#VALUE!"), Want::Differ(Error)),
        ("a value and an error both off", "1/0", "1", None, Some("#N/A"), Want::Differ(Error)),
        ("a value off and an error on the file", "2+3", "9", None, Some("#N/A"), Want::Differ(Both)),
    ];
    let mut failures = Vec::new();
    for (label, formula, v, u, e, want) in rows {
        let r = score_one(cell(formula, v, *u, *e));
        let got = match (r.agreed, r.differed.first()) {
            (1, None) => Want::Agree,
            (0, Some(d)) => Want::Differ(d.kind),
            _ => {
                failures.push(format!("{label}: neither agreed nor differed: {r:?}"));
                continue;
            }
        };
        let same = match (got, *want) {
            (Want::Agree, Want::Agree) => true,
            (Want::Differ(a), Want::Differ(b)) => a == b,
            _ => false,
        };
        if !same {
            failures.push(format!("{label}: `{formula}` against V {v:?}: got {got:?}, wanted {want:?}"));
        }
    }
    assert!(failures.is_empty(), "{} of {} cases failed:\n{}", failures.len(), rows.len(), failures.join("\n"));
}

#[test]
fn an_inherited_cell_whose_chain_holds_no_formula_is_not_scored() {
    // C1: a master `Width` of 5 with no formula under an instance `Inh` of 999, and a master-less `Inh` of 777, were compared with
    // themselves and counted as agreed. There is nothing to work out: they are skipped, and counted as that.
    let master = MasterBuilder::new().shape(ShapeBuilder::new(1).cell("Width", "5"));
    let page =
        PageBuilder::new().shape(ShapeBuilder::new(10).master(1).inh("Width", Some("999"))).shape(ShapeBuilder::new(11).inh("PinX", Some("777")));
    let doc = DocBuilder::new().master(1, master).page(1, page).build();
    for operands in [OperandMode::Cached, OperandMode::Live] {
        let r = recalc_all_with(&doc, &Budget::default(), EvalOptions { operands, ..EvalOptions::default() });
        assert_eq!((r.cells, r.agreed, r.differed.len()), (2, 0, 0), "{operands:?}: {r:?}");
        assert_eq!(r.skipped.inherited_no_formula, 2, "{operands:?}: {r:?}");
        assert_eq!(r.cells, accounted(&r));
    }
    // And an `Inh` whose master does hold a formula is still scored: 999 is not what `Height*2` gives.
    let master = MasterBuilder::new().shape(ShapeBuilder::new(1).cell("Height", "2").formula("Width", "Height*2", "4"));
    let page = PageBuilder::new().shape(ShapeBuilder::new(10).master(1).inh("Width", Some("999")));
    let doc = DocBuilder::new().master(1, master).page(1, page).build();
    let r = recalc_all(&doc, &Budget::default());
    assert_eq!((r.agreed, r.differed.len(), r.skipped.inherited_no_formula), (1, 1, 0), "{r:?}");
}

#[test]
fn infinity_agrees_only_with_infinity() {
    // C2: `|a - b| <= tol * max(|a|, |b|)` is true when one is infinite, so an overflow agreed with anything.
    let rows: &[(&str, &str, &str, bool)] = &[
        ("overflow against 42", "1E308*10", "42", false),
        ("a power past a double against 1", "10^400", "1", false),
        ("5 against infinity", "5", "1.#INF", false),
        ("infinity against infinity", "1E308*10", "1.#INF", true),
        ("minus infinity against minus infinity", "-1E308*10", "-1.#INF", true),
        ("minus infinity against infinity", "-1E308*10", "1.#INF", false),
    ];
    for (label, formula, v, agree) in rows {
        let r = score_one(cell(formula, v, None, None));
        assert_eq!((r.agreed, r.differed.len()), if *agree { (1, 0) } else { (0, 1) }, "{label}: {r:?}");
    }
}

#[test]
fn a_run_that_ends_counts_every_cell_it_did_not_reach_on_every_sheet() {
    // I3: `unreached` stopped at the sheet it ran out in. Three shapes of 100 cells at a budget of 100 steps: every cell is counted once, the
    // one it ran out on among the unreached, and the sheets after it counted too.
    let mut page = PageBuilder::new();
    for id in 1..=3u32 {
        let mut s = ShapeBuilder::new(id);
        for k in 0..100u32 {
            s = s.row_formula("Scratch", None, RowKey::Ix(k), "A", "1+1", "2");
        }
        page = page.shape(s);
    }
    let doc = DocBuilder::new().page(1, page).build();
    let r = recalc_all(&doc, &Budget::with_steps(100));
    assert!(r.budget_exhausted);
    assert_eq!(r.cells, 300, "{r:?}");
    assert_eq!(r.cells, accounted(&r), "{r:?}");
    assert!(r.unreached > 200, "the two later sheets are in it: {r:?}");
    assert!(r.agreed > 0 && r.agreed + r.unreached == 300, "{r:?}");
    // With room, the same document is counted whole, and nothing is unreached.
    let r = recalc_all(&doc, &Budget::default());
    assert_eq!((r.cells, r.agreed, r.unreached, r.budget_exhausted), (300, 300, 0, false));
}

#[test]
fn a_run_that_passes_its_memory_ends_the_same_way() {
    // C4: the memory budget ends a run as `budget_exhausted`, with every cell counted, and never as an abort.
    let mut s = ShapeBuilder::new(1);
    for k in 0..200u32 {
        s = s.row_formula("Scratch", None, RowKey::Ix(k), "A", "REPT(\"x\",100000)", "y");
    }
    let doc = doc_of(s);
    let budget = Budget { steps: Budget::DEFAULT_STEPS, bytes: 4 << 20 };
    let r = recalc_all_with(&doc, &budget, EvalOptions { operands: OperandMode::Live, ..EvalOptions::default() });
    assert!(r.budget_exhausted && r.memory_exhausted, "{r:?}");
    assert_eq!(r.cells, 200);
    assert_eq!(r.cells, accounted(&r), "{r:?}");
    assert!(r.unreached > 100 && r.stats.bytes_peak <= 5 << 20, "{r:?}");
}

#[test]
fn a_printed_value_is_cut_and_the_list_of_differences_is_capped() {
    // C4: a megabyte of text was printed whole in a difference, twice.
    let r = score_one(cell("REPT(\"x\",1000000)", "y", None, None));
    let d = &r.differed[0];
    assert!(d.actual.as_ref().is_some_and(|a| a.len() < 400 && a.contains("characters)")), "{:?}", d.actual);
    assert_eq!(d.saved.as_deref(), Some("y [STR]"));
    // A value of 256 characters with its unit is printed whole.
    let r = score_one(cell(&format!("REPT(\"x\",{})", 250), "y", None, None));
    assert_eq!(r.differed[0].actual.as_ref().map(String::len), Some(250 + " [STR]".len()));
    // The list of differences is capped, and what is past it is counted: a sum of reports keeps the first 100,000 and counts the rest.
    let cap = diacraft_calc::rules::MAX_DIFFERENCES;
    let mut b = RecalcReport::default();
    for _ in 0..cap + 5 {
        b.differed.push(r.differed[0].clone());
    }
    let mut a = RecalcReport::default();
    a.merge(b);
    assert_eq!((a.differed.len(), a.differed_unlisted, a.scored()), (cap, 5, cap as u64 + 5));
}

// ---- the rules of the specs the oracle follows (I6) --------------------------------------------------------------------------

#[test]
fn a_colour_made_by_msotint_agrees_within_one_step_and_no_other_does() {
    // Single precision leaves 2,173 cells one step off; the oracle compares them with a tolerance of one.
    let computed = match crate::eval("MSOTINT(RGB(100,150,200),30)") {
        diacraft_calc::Value::Color(c) => c,
        other => panic!("{other:?}"),
    };
    let step = |d: i32| format!("#{:02x}{:02x}{:02x}", (i32::from(computed.r) + d) as u8, computed.g, computed.b);
    for (formula, d, agree) in [
        ("MSOTINT(RGB(100,150,200),30)", 0, true),
        ("MSOTINT(RGB(100,150,200),30)", 1, true),
        ("MSOTINT(RGB(100,150,200),30)", -1, true),
        ("MSOTINT(RGB(100,150,200),30)", 2, false),
        ("RGB(100,150,200)", 0, true),
    ] {
        let saved = if formula.starts_with("RGB") { "#6496c8".to_owned() } else { step(d) };
        let r = score_one(cell(formula, &saved, None, None));
        assert_eq!((r.agreed, r.differed.len()), if agree { (1, 0) } else { (0, 1) }, "{formula} at {d}: {r:?}");
    }
    // A cell that is not made by MSOTINT gets no tolerance: one step off is a difference.
    let r = score_one(cell("RGB(100,150,200)", "#6596c8", None, None));
    assert_eq!((r.agreed, r.differed.len()), (0, 1), "{r:?}");
}

#[test]
fn a_date_past_the_year_9999_is_left_out() {
    // Seven cells hold the far date a capture prints (`30177-04-13`), which no reader can use as a date.
    let r = score_one(cell("DATE(1899,12,29)", "30177-04-13T00:00:00", Some("DATE"), None));
    assert_eq!((r.agreed, r.differed.len(), r.skipped.far_year), (0, 0, 1), "{r:?}");
    // A date of four digits is scored as always.
    let r = score_one(cell("DATE(2031,3,7)", "2031-03-07T00:00:00", Some("DATE"), None));
    assert_eq!((r.agreed, r.skipped.far_year), (1, 0), "{r:?}");
    // And a far year on a cell that is not a date is not this.
    let r = score_one(cell(r#""30177-04-13""#, "30177-04-13", Some("STR"), None));
    assert_eq!((r.agreed, r.skipped.far_year), (1, 0), "{r:?}");
}

fn with_dates(shape: ShapeBuilder, created: &str, modified: &str) -> Document {
    let mut doc = doc_of(shape);
    let props = Arc::make_mut(&mut doc.props);
    props.created = Some(Arc::from(created));
    props.modified = Some(Arc::from(modified));
    doc
}

#[test]
fn the_document_dates_are_skipped_and_checked_for_a_whole_quarter_hour() {
    // No score on the cell, and the check passes at a shift of -4 hours with
    // nothing left over; 7 minutes is no whole quarter hour; a `DOCLASTEDIT` may be 180 seconds behind and a tolerance of 2 fails it.
    let modified = "2031-03-07T20:05:09Z";
    let rows: &[(&str, &str, &str, bool)] = &[
        ("DOCLASTSAVE()", "2031-03-07T16:05:09", "ALL", true),
        ("DOCLASTSAVE()", "2031-03-07T16:12:09", ", 7 minutes", false),
        ("DOCLASTSAVE()", "2031-03-07T16:05:11", "2 seconds", true),
        ("DOCLASTSAVE()", "2031-03-07T16:05:12", "3 seconds", false),
        ("DOCLASTEDIT()", "2031-03-07T16:02:41", "148 seconds before", true),
        ("DOCLASTEDIT()", "2031-03-07T16:01:00", "DOCLASTEDIT: 249 seconds before", false),
        ("DOCLASTEDIT()", "2031-03-07T16:06:09", "DOCLASTEDIT: after the property", false),
        ("DOCCREATION()", "2031-03-07T05:05:09", "creation, a different property", true),
    ];
    for (formula, v, label, fits) in rows {
        let doc = with_dates(ShapeBuilder::new(1).set(x(), cell(formula, v, Some("DATE"), None)), "2031-03-07T09:05:09Z", modified);
        let r = recalc_all(&doc, &Budget::default());
        assert_eq!((r.agreed, r.differed.len(), r.skipped.clock_readers), (0, 0, 1), "{label}: no score: {r:?}");
        let got = (r.document_date_checks.fit, r.document_date_checks.not_fit);
        assert_eq!(got, if *fits { (1, 0) } else { (0, 1) }, "{label}: {r:?}");
    }
    // A cell that merely reads one is skipped and not checked: only the whole formula `DOC...()` is.
    let doc = with_dates(
        ShapeBuilder::new(1).set(x(), cell("DOCLASTSAVE()+1", "2031-03-07T16:05:09", Some("DATE"), None)),
        "2031-03-07T09:05:09Z",
        modified,
    );
    let r = recalc_all(&doc, &Budget::default());
    assert_eq!((r.skipped.clock_readers, r.document_date_checks.fit, r.document_date_checks.not_fit), (1, 0, 0), "{r:?}");
}

#[test]
fn a_string_with_no_year_reads_the_clock_and_is_skipped_with_or_without_one() {
    // No year, or no date, is the current year or date; "July 19" scored against a saved year is wrong in any other year.
    for options in [EvalOptions::default(), EvalOptions { clock: fixed_clock("2031-03-07T16:05:09", 0), ..EvalOptions::default() }] {
        let doc = doc_of(
            ShapeBuilder::new(1)
                .set(x(), cell(r#"YEAR("July 19")"#, "1999", None, None))
                .set(user("Y"), cell(r#"YEAR("July 19, 2020")"#, "2020", None, None)),
        );
        let r = recalc_all_with(&doc, &Budget::default(), options);
        assert_eq!((r.skipped.clock_readers, r.agreed, r.unsupported_cells()), (1, 1, 0), "{r:?}");
    }
}

fn clock_cells(shape: ShapeBuilder, functions: &[&str]) -> ShapeBuilder {
    let mut s = shape;
    for (k, f) in functions.iter().enumerate() {
        s = s.set(user(&format!("F{k}")), cell(&format!("{f}()"), "abc", None, None));
        // The reader names the clock cell and nothing else: it can only be found by what it names.
        s = s.set(user(&format!("D{k}")), cell(&format!("LEN(User.F{k})"), "99", None, None));
    }
    s
}

#[test]
fn every_clock_function_makes_its_cell_and_the_cells_that_read_it_clock_readers() {
    let functions = ["FILENAME", "DIRECTORY", "RAND", "NOW", "DOCCREATION", "DOCLASTEDIT", "DOCLASTPRINT", "DOCLASTSAVE"];
    let doc = doc_of(clock_cells(ShapeBuilder::new(1), &functions));
    let r = recalc_all(&doc, &Budget::default());
    assert_eq!((r.skipped.clock_readers, r.agreed, r.differed.len(), r.unsupported_cells()), (16, 0, 0, 0), "{r:?}");
    // And a function that is not a clock one is scored (its reader too): the list is the list.
    let doc = doc_of(clock_cells(ShapeBuilder::new(1), &["PI", "ID"]));
    let r = recalc_all(&doc, &Budget::default());
    assert_eq!((r.skipped.clock_readers, r.differed.len()), (0, 4), "{r:?}");
}

#[test]
fn a_cell_that_reads_the_clock_through_a_call_the_names_do_not_show_is_found_live() {
    // In `Live` mode the clock is found by what the evaluation reached: `SHEETREF()!User.F` hides the link from the walk of names.
    let shape =
        ShapeBuilder::new(1).set(user("F"), cell("NOW()", "abc", None, None)).set(user("D"), cell("LEN(SHEETREF()!User.F)", "99", None, None));
    let doc = doc_of(shape);
    let options = EvalOptions { operands: OperandMode::Live, clock: fixed_clock("2031-03-07T16:05:09", 0), ..EvalOptions::default() };
    let r = recalc_all_with(&doc, &Budget::default(), options);
    assert_eq!((r.skipped.clock_readers, r.agreed, r.differed.len()), (2, 0, 0), "{r:?}");
}

fn field_shape(id: u32, field_formula: &str, readers: &[(&str, &str)]) -> ShapeBuilder {
    let mut s = ShapeBuilder::new(id).text("Plant").row_formula("Field", None, RowKey::Ix(0), "Value", field_formula, "abc");
    for (name, formula) in readers {
        s = s.set(user(name), cell(formula, "99", None, None));
    }
    s
}

#[test]
fn the_text_of_a_shape_whose_field_reads_the_clock_is_read_by_the_cells_that_measure_it() {
    // A field calls NOW() (or FILENAME()), `TxtHeight` reads the text
    // and `TxtLocPinY` reads `TxtHeight`: all three are skipped, though `TEXTHEIGHT` is not worked and was counted unsupported.
    for call in ["NOW()", "FILENAME()"] {
        let s = field_shape(1, call, &[("TxtHeight", "TEXTHEIGHT(TheText,3 in)"), ("TxtLocPinY", "User.TxtHeight*0.375")]);
        let r = recalc_all(&doc_of(s), &Budget::default());
        assert_eq!((r.skipped.clock_readers, r.unsupported_cells(), r.agreed, r.differed.len()), (3, 0, 0, 0), "{call}: {r:?}");
        assert_eq!(r.cells, accounted(&r));
    }
    // The same cells over a field that reads no clock: the measure is worked, and both cells are scored against the 99 they saved.
    let s = field_shape(1, "ID()", &[("TxtHeight", "TEXTHEIGHT(TheText,3 in)"), ("TxtLocPinY", "User.TxtHeight*0.375")]);
    let r = recalc_all(&doc_of(s), &Budget::default());
    assert_eq!((r.skipped.clock_readers, r.unsupported_cells()), (0, 0), "{r:?}");
    assert!(r.differed.iter().any(|d| d.cell == "User.TxtHeight.Value"), "the measure is compared with the saved value: {r:?}");
}

#[test]
fn a_text_read_follows_the_fields_of_the_sheet_it_names_and_not_its_own() {
    // `SHAPETEXT(Sheet.1!TheText)` on a clean shape reads shape 1's text, whose field reads the clock.
    let page = PageBuilder::new().shape(field_shape(1, "NOW()", &[])).shape(field_shape(
        2,
        "ID()",
        &[("A", "LEN(SHAPETEXT(Sheet.1!TheText))"), ("B", "LEN(SHAPETEXT(TheText))")],
    ));
    let doc = DocBuilder::new().page(1, page).build();
    let r = recalc_all(&doc, &Budget::default());
    // The field of shape 1 (it names NOW) and `A` are skipped; `B` reads its own clean text and is scored; shape 2's field is scored.
    assert_eq!(r.skipped.clock_readers, 2, "{r:?}");
    assert_eq!(r.cells, accounted(&r));
    assert_eq!(r.differed.iter().filter(|d| d.cell == "User.A.Value").count(), 0, "{r:?}");
    assert!(r.differed.iter().any(|d| d.cell == "User.B.Value"), "{r:?}");
}

/// A theme part that gave the reader nothing: the bytes are a stub, and no colour, font or scheme number was read from them.
pub(crate) fn bare_theme() -> Theme {
    Theme {
        part: Arc::from("visio/theme/theme1.xml"),
        bytes: Arc::from(&b"<a/>"[..]),
        colors: None,
        slots: [None; 12],
        major_font: None,
        minor_font: None,
        color_scheme_id: None,
        font_scheme_id: None,
        effect_scheme_id: None,
        connector_scheme_id: None,
        theme_scheme_id: None,
        entries: Default::default(),
    }
}

#[test]
fn a_colour_under_a_scheme_the_file_no_longer_selects_is_not_scored() {
    // `ColorSchemeIndex` 65534 with no page cell and a theme part in the file reads no theme, and the
    // cached colours may be stale: not a pass and not a fail.
    // Narrowed the rule: the cell is left out only when the saved colour is the one the file's first theme part gives and not the one the
    // sheet's own reading gives (oracle_two_readings.rs has and the rest). Here the part's `lt1` is the saved colour, and the table's `Light` is white.
    let shape = |saved: &str| ShapeBuilder::new(1).cell("ColorSchemeIndex", "65534").set(x(), cell(r#"THEMEVAL("Light")"#, saved, None, None));
    let mut part = diacraft_testkit::theme::ThemeXml::default();
    part.colours[1] = "112233";
    let mut with_theme = doc_of(shape("#112233"));
    with_theme.themes.push(part.part());
    let r = recalc_all(&with_theme, &Budget::default());
    assert_eq!((r.skipped.stale_theme_colour, r.agreed, r.differed.len()), (1, 0, 0), "{r:?}");
    // Without a theme part the cell is scored: #112233 is not the table's colour.
    let r = recalc_all(&doc_of(shape("#112233")), &Budget::default());
    assert_eq!((r.skipped.stale_theme_colour, r.differed.len()), (0, 1), "{r:?}");
    // A part that cannot answer explains nothing: the saved colour is a difference.
    let mut bare = doc_of(shape("#112233"));
    bare.themes.push(Arc::new(bare_theme()));
    let r = recalc_all(&bare, &Budget::default());
    assert_eq!((r.skipped.stale_theme_colour, r.differed.len()), (0, 1), "{r:?}");
}

#[test]
fn a_cell_that_met_the_native_depth_limit_is_skipped_and_not_scored() {
    // 200 cells read one from another through a call the walk of names cannot see: the first met the limit, and its result is the limit's.
    let mut s = ShapeBuilder::new(1);
    for k in 0..200u32 {
        s = s.row_formula("User", None, RowKey::name(&format!("C{k}")), "Value", &format!("SHEETREF()!User.C{}+1", k + 1), "7");
    }
    s = s.row_formula("User", None, RowKey::name("C200"), "Value", "0", "0");
    let r = recalc_all_with(&doc_of(s), &Budget::default(), EvalOptions { operands: OperandMode::Live, ..EvalOptions::default() });
    assert!(r.skipped.depth_limit > 0, "{r:?}");
    assert_eq!(r.cells, accounted(&r), "{r:?}");
}

// ---- Unsupported is never agreed (I4, I9) ---------------------------------------------------------------------------------

#[test]
fn a_function_this_evaluator_lacks_is_unsupported_in_every_function_that_reads_it() {
    let lack = "_SHAPEMIN(1,2)";
    let wrapped = [
        format!("ISERROR({lack})"),
        format!("ISERR({lack})"),
        format!("ISERRNA({lack})"),
        format!("ISERRVALUE({lack})"),
        format!("IFERROR({lack},1)"),
        format!("STRSAME({lack},\"a\")"),
        format!("IF({lack},1,2)"),
        format!("AND(1,{lack})"),
        format!("OR(0,{lack})"),
        format!("NOT({lack})"),
        format!("{lack}+1"),
        format!("{lack}<1"),
        format!("{lack}&\"a\""),
        format!("RED({lack})"),
        format!("SUM(1,{lack})"),
        format!("LEN({lack})"),
        // read from a string
        "IF(\"_SHAPEMIN(1,2)\",1,2)".to_owned(),
        "AND(1,\"_SHAPEMIN(1,2)\")".to_owned(),
        "NOT(\"_SHAPEMIN(1,2)\")".to_owned(),
        "RED(\"_SHAPEMIN(1,2)\")".to_owned(),
        "\"_SHAPEMIN(1,2)\"<1".to_owned(),
        "\"_SHAPEMIN(1,2)\"+1".to_owned(),
        "IFERROR(\"_SHAPEMIN(1,2)\"+1,5)".to_owned(),
    ];
    for f in &wrapped {
        let v = crate::eval(f);
        assert!(matches!(v, diacraft_calc::Value::Unsupported(_)), "`{f}`: {v:?}");
        // And as a cell: counted under the function's name, whatever was saved, never agreed.
        let r = score_one(cell(f, "0", None, None));
        assert_eq!((r.agreed, r.differed.len(), r.unsupported_cells()), (0, 0, 1), "`{f}`: {r:?}");
    }
}

#[test]
fn a_cell_that_reads_an_unsupported_one_is_scored_on_the_value_the_file_saved_for_it() {
    // `A` is unsupported, so its saved 5 stands for the readers (drawing uses it): `B` is scored against its own saved value.
    for operands in [OperandMode::Cached, OperandMode::Live] {
        let shape = ShapeBuilder::new(1)
            .set(user("A"), cell("_SHAPEMIN(1,2)", "5", None, None))
            .set(user("B"), cell("User.A+1", "6", None, None))
            .set(user("C"), cell("User.A+1", "99", None, None));
        let r = recalc_all_with(&doc_of(shape), &Budget::default(), EvalOptions { operands, ..EvalOptions::default() });
        assert_eq!((r.unsupported_cells(), r.agreed, r.differed.len()), (1, 1, 1), "{operands:?}: {r:?}");
    }
}

#[test]
fn a_cell_whose_formula_does_not_parse_keeps_its_saved_value_for_the_cells_that_read_it() {
    // The saved 7 stands: `B` is 7 + 1, not 0 + 1.
    let shape = || ShapeBuilder::new(1).set(user("P"), cell("1 +", "7", None, None)).set(user("B"), cell("User.P+1", "8", None, None));
    for operands in [OperandMode::Cached, OperandMode::Live] {
        let r = recalc_all_with(&doc_of(shape()), &Budget::default(), EvalOptions { operands, ..EvalOptions::default() });
        assert_eq!((r.skipped.parse_failed, r.agreed, r.differed.len()), (1, 1, 0), "{operands:?}: {r:?}");
    }
}

#[test]
fn the_cells_that_run_on_an_event_are_never_worked_out() {
    let shape = ShapeBuilder::new(1)
        .formula("EventDblClick", "1+1", "99")
        .formula("EventDrop", "1+1", "99")
        .formula("EventMultiDrop", "1+1", "99")
        .formula("EventXFMod", "1+1", "99")
        .formula("TheData", "1+1", "99")
        .row_formula("Actions", None, RowKey::name("Open"), "Action", "1+1", "99");
    let r = recalc_all(&doc_of(shape), &Budget::default());
    assert_eq!((r.skipped.actions, r.agreed, r.differed.len()), (6, 0, 0), "{r:?}");
}

// ---- coverage: every kind of sheet, and the rows that are deleted ---------------------------------------------------------------

#[test]
fn a_formula_on_every_kind_of_sheet_is_scored_and_a_deleted_row_is_not() {
    let mut doc = DocBuilder::new()
        .style(5, StyleBuilder::new().formula("LineWeight", "1+1", "99"))
        .master(
            1,
            MasterBuilder::new().cell("PageWidth", "3").formula("PageHeight", "1+1", "99").shape(ShapeBuilder::new(1).formula("Width", "1+1", "99")),
        )
        .page(
            1,
            PageBuilder::new().formula("PageWidth", "1+1", "99").shape(
                ShapeBuilder::new(10)
                    .formula("Width", "1+1", "99")
                    .row_formula("User", None, RowKey::name("Gone"), "Value", "1+1", "99")
                    .del_row("User", None, RowKey::name("Gone"))
                    .child(ShapeBuilder::new(11).formula("Width", "1+1", "99")),
            ),
        )
        .build();
    Arc::make_mut(&mut doc.sheet).set_cell(&CellAddr::direct("DocLangID"), cell("1+1", "99", None, None)).unwrap();
    let r = recalc_all(&doc, &Budget::default());
    let mut sheets: Vec<&str> = r.differed.iter().map(|d| d.sheet.as_str()).collect();
    sheets.sort_unstable();
    assert_eq!(sheets, vec!["document", "master:1", "master:1/shape:1", "page:1", "page:1/shape:10", "page:1/shape:11", "style:5"], "{r:?}");
    assert_eq!((r.cells, r.agreed), (7, 0), "the deleted row is not a cell: {r:?}");
}

#[test]
fn the_score_of_a_function_counts_the_cells_that_differed() {
    let shape = ShapeBuilder::new(1).formula("PinX", "SUM(1,1)", "2").formula("PinY", "SUM(1,1)", "9").formula("Width", "ABS(-1)", "9");
    let r = recalc_all(&doc_of(shape), &Budget::default());
    let sum = r.by_function.get("SUM").copied().unwrap_or_default();
    let abs = r.by_function.get("ABS").copied().unwrap_or_default();
    assert_eq!((sum.agreed, sum.differed, abs.agreed, abs.differed), (1, 1, 0, 1), "{r:?}");
}

// ---- the survivors of the first mutation pass of the fix round -------------------------------------------------------------------

#[test]
fn a_cell_nothing_holds_a_formula_for_but_a_page_prints_one_is_not_scored() {
    // `TxtWidth` reads `Width*1` where nothing holds it: an `Inh` of 999 over nothing is not compared with 4.
    let shape = ShapeBuilder::new(1).cell("Width", "4").inh("TxtWidth", Some("999"));
    let r = recalc_all(&doc_of(shape), &Budget::default());
    assert_eq!((r.skipped.inherited_no_formula, r.agreed, r.differed.len()), (1, 0, 0), "{r:?}");
}

#[test]
fn a_colour_is_stale_only_under_the_scheme_65534_that_reads_no_theme() {
    let theme = bare_theme;
    // A scheme of 0 with a theme part in the file is the table's colour, which is scored (and #112233 is not it).
    let mut doc = doc_of(ShapeBuilder::new(1).cell("ColorSchemeIndex", "0").set(x(), cell(r#"THEMEVAL("FillForegnd")"#, "#112233", None, None)));
    doc.themes.push(Arc::new(theme()));
    let r = recalc_all(&doc, &Budget::default());
    assert_eq!((r.skipped.stale_theme_colour, r.differed.len()), (0, 1), "{r:?}");
}

#[test]
fn a_deleted_section_holds_no_cell_to_score() {
    let shape = ShapeBuilder::new(1)
        .row_formula("User", None, RowKey::name("Gone"), "Value", "1+1", "99")
        .del_section("User", None)
        .formula("Width", "1+1", "99");
    let r = recalc_all(&doc_of(shape), &Budget::default());
    assert_eq!((r.cells, r.differed.len()), (1, 1), "{r:?}");
}

#[test]
fn a_string_read_into_a_colour_cell_that_reads_an_unknown_function_is_unknown() {
    let shape = ShapeBuilder::new(1).formula("LineColor", r#""_SHAPEMIN(1,2)""#, "#112233");
    let r = recalc_all(&doc_of(shape), &Budget::default());
    assert_eq!((r.unsupported_cells(), r.agreed, r.differed.len()), (1, 0, 0), "{r:?}");
}

#[test]
fn the_list_of_differences_stops_at_its_cap_and_the_rest_are_counted() {
    // 7 shapes of 14,500 rows (a section holds at most 16,384): 101,500 cells that all differ.
    let mut page = PageBuilder::new();
    for id in 1..=7u32 {
        let mut s = ShapeBuilder::new(id);
        for k in 0..14_500u32 {
            s = s.row_formula("Scratch", None, RowKey::Ix(k), "A", "1+1", "9");
        }
        page = page.shape(s);
    }
    let doc = DocBuilder::new().page(1, page).build();
    let r = recalc_all(&doc, &Budget::default());
    let cap = diacraft_calc::rules::MAX_DIFFERENCES;
    assert_eq!((r.differed.len(), r.differed_unlisted, r.scored()), (cap, 101_500 - cap as u64, 101_500), "{:?}", r.stats);
    assert_eq!(r.cells, accounted(&r));
}

#[test]
fn a_forward_chain_of_cells_ending_in_the_clock_costs_its_length_and_not_its_square() {
    // M2: only the start of a walk was remembered as a clock reader, so each of 3,000 cells in a chain walked the cells below it again.
    let n = 3_000u32;
    let mut s = ShapeBuilder::new(1);
    for k in 0..n {
        s = s.row_formula("Scratch", None, RowKey::Ix(k), "A", &format!("Scratch.A{}+1", k + 2), "7");
    }
    s = s.row_formula("Scratch", None, RowKey::Ix(n), "A", "NOW()", "7");
    let r = recalc_all(&doc_of(s), &Budget::with_steps(200_000));
    assert!(!r.budget_exhausted, "{} steps: {r:?}", r.stats.steps);
    assert_eq!((r.skipped.clock_readers, r.cells), (u64::from(n) + 1, u64::from(n) + 1), "{r:?}");
}

#[test]
fn text_is_charged_by_its_length_before_it_is_worked() {
    // 64 bytes a step, charged first. Each formula below holds a text of 100 KB or builds one of 1 MB and costs more than
    // 1,000 steps in the work it does, so a budget of 1,000 ends the run at it (and the other charges of the same formula are small).
    let big = "x".repeat(100_000);
    let formulas = [format!("\"{big}\"&\"b\""), format!("UPPER(\"{big}\")"), "REPT(\"x\",1000000)".to_owned(), format!("LEN(\"{big}\")")];
    for f in formulas {
        let r = recalc_all(&doc_of(ShapeBuilder::new(1).set(x(), cell(&f, "0", None, None))), &Budget::with_steps(1_000));
        assert!(r.budget_exhausted && r.unreached == 1, "`{}`: {r:?}", &f[..f.len().min(40)]);
    }
}

#[test]
fn a_point_charges_its_references_once_to_evaluate_and_once_to_find_their_sheet() {
    // 2,000 references: the whole cell costs 14,018 steps, 2,001 of them for the sheet the references name; 13,000 is enough without them.
    let refs = vec!["Width"; 2_000].join(",");
    let shape = ShapeBuilder::new(1).cell("Width", "1").set(x(), cell(&format!("PNTX(PNT(SUM({refs}),1))"), "0", None, None));
    let r = recalc_all(&doc_of(shape), &Budget::with_steps(13_000));
    assert!(r.budget_exhausted, "{r:?}");
    let shape = ShapeBuilder::new(1).cell("Width", "1").set(x(), cell(&format!("PNTX(PNT(SUM({refs}),1))"), "0", None, None));
    let r = recalc_all(&doc_of(shape), &Budget::with_steps(80_000));
    assert!(!r.budget_exhausted, "{r:?}");
}

#[test]
fn a_text_a_shape_inherits_from_its_master_is_unknown_to_shapetext() {
    // I5: a page shape shows the master's text with its own field values, which are not worked: unsupported, and not empty.
    let master = MasterBuilder::new().shape(ShapeBuilder::new(1).text("Plant"));
    let page = PageBuilder::new().shape(ShapeBuilder::new(10).master(1).set(x(), cell("LEN(SHAPETEXT(TheText))", "5", None, None)));
    let doc = DocBuilder::new().master(1, master).page(1, page).build();
    let r = recalc_all(&doc, &Budget::default());
    assert_eq!((r.unsupported_cells(), r.agreed, r.differed.len()), (1, 0, 0), "{r:?}");
}

#[test]
fn the_map_of_clock_readers_is_held_memory() {
    // 300 cells each naming 200 other cells: the map of what has been found to read no clock holds an entry for each, at 640 bytes.
    let refs: Vec<String> = (0..200).map(|k| format!("User.C{k}")).collect();
    let mut s = ShapeBuilder::new(1);
    for k in 0..200u32 {
        s = s.row_cell("User", None, RowKey::name(&format!("C{k}")), "Value", "1");
    }
    for i in 0..300u32 {
        s = s.row_formula("Scratch", None, RowKey::Ix(i), "A", &format!("IF(1=1,1,SUM({}))", refs.join(",")), "1");
    }
    let doc = doc_of(s);
    let r = recalc_all_with(&doc, &Budget { steps: Budget::DEFAULT_STEPS, bytes: 400 << 10 }, EvalOptions::default());
    assert!(r.memory_exhausted && r.budget_exhausted, "{r:?}");
    assert!(r.stats.bytes_peak <= (400 << 10) + 1024, "{} held", r.stats.bytes_peak);
    let r = recalc_all(&doc, &Budget::default());
    assert!(!r.budget_exhausted && r.agreed == 300, "{r:?}");
}

#[test]
fn the_clock_readers_found_are_held_memory_too() {
    // The first of 200 cells names the next in a branch it does not take, and so on to a last that calls the clock. Working the first cell
    // out reads no clock; the walk by names does, and it remembers each of the 200 cells on its path, 640 bytes each, on top of what the
    // walk itself holds while it runs (the budget is chosen between the two: the walk fits, the map does not).
    let n = 200u32;
    let mut s = ShapeBuilder::new(1);
    for k in 0..n {
        let formula = if k + 1 == n { "NOW()".to_string() } else { format!("IF(1=1,5,Scratch.A{})", k + 2) };
        s = s.row_formula("Scratch", None, RowKey::Ix(k), "A", &formula, "5");
    }
    let doc = doc_of(s);
    let r = recalc_all(&doc, &Budget::default());
    assert!(!r.budget_exhausted && r.skipped.clock_readers == u64::from(n), "{r:?}");
    let r = recalc_all(&doc, &Budget { steps: Budget::DEFAULT_STEPS, bytes: 300_000 });
    assert!(r.memory_exhausted && r.budget_exhausted, "{r:?}");
    assert!(r.stats.bytes_peak <= 301_000, "{} held", r.stats.bytes_peak);
    assert_eq!(accounted(&r), r.cells, "{r:?}");
}

#[test]
fn a_cell_that_reads_one_the_depth_limit_cut_short_is_skipped_by_the_oracle_too() {
    // Round 2, item 1: the oracle listed the readers of a cut-short cell as differences (a saved 0 against an `ISERROR` that is true only
    // because the chain was cut) and counted them scored. Every saved value here is right, so nothing may be listed.
    for readers_first in [false, true] {
        let mut shape = ShapeBuilder::new(1);
        let readers = |mut s: ShapeBuilder| {
            for k in 0..20u32 {
                s = s.row_formula("User", None, RowKey::name(&format!("R{k}")), "Value", "ISERROR(SHEETREF()!Scratch.A18)", "0");
                s = s.row_formula("User", None, RowKey::name(&format!("Q{k}")), "Value", "IFERROR(SHEETREF()!Scratch.A18,7)", "1");
            }
            s
        };
        if readers_first {
            shape = readers(shape);
        }
        for k in 0..100u32 {
            let formula = if k == 99 { "1".to_owned() } else { format!("SHEETREF()!Scratch.A{}", k + 2) };
            shape = shape.row_formula("Scratch", None, RowKey::Ix(k), "A", &formula, "1");
        }
        if !readers_first {
            shape = readers(shape);
        }
        let doc = doc_of(shape);
        let r = recalc_all_with(&doc, &Budget::default(), EvalOptions::default());
        assert_eq!(accounted(&r), r.cells, "{r:?}");
        assert!(r.skipped.depth_limit > 0, "readers first {readers_first}: {r:?}");
        let listed: Vec<String> = r.differed.iter().map(|d| format!("{} {}", d.sheet, d.cell)).collect();
        assert!(listed.is_empty(), "readers first {readers_first}: {listed:?} {r:?}");
    }
}
