//! The oracle's two readings of a colour under a scheme the file no longer selects (the "Narrowing the
//! leave-out"), and what keeps the echo control and the real-file salt honest under it.
//!
//! The cell is worked out twice, with no theme (what the sheet reads) and with the file's first theme part selected, and is left out only when the saved
//! value is the second reading and not the first. Every other cell under the rule's condition is scored.

use std::sync::Arc;

use diacraft_calc::{Budget, EvalOptions, OperandMode, RecalcReport, SaltPolicy, recalc_all, recalc_all_with};
use diacraft_doc::{Cell, CellAddr, Document, RowKey, Value as DocValue};
use diacraft_testkit::build::{DocBuilder, MasterBuilder, PageBuilder, ShapeBuilder};
use diacraft_testkit::theme::ThemeXml;

use crate::oracle_cases::bare_theme;
use crate::recalc_fixes::{Echo, salted};

const FIVE: [&str; 5] = ["ThemeIndex", "ColorSchemeIndex", "EffectSchemeIndex", "ConnectorSchemeIndex", "FontSchemeIndex"];

/// A theme part whose first fill colour, `lt1` (`THEMEVAL("Light")`), is `#3a6b9e`.
fn blue_theme() -> ThemeXml {
    let mut x = ThemeXml::default();
    x.colours[1] = "3a6b9e";
    x
}

fn user(name: &str) -> CellAddr {
    CellAddr::in_row("User", None, RowKey::name(name), "Value")
}

fn formula_cell(formula: &str, saved: &str) -> Cell {
    Cell::with_formula(formula, Some(DocValue::from_text(saved)))
}

/// A master shape whose five scheme cells follow a page that holds none.
fn following(shape: ShapeBuilder) -> ShapeBuilder {
    FIVE.iter().fold(shape, |s, name| s.cell(name, "65534"))
}

/// The document: the master shape follows the page, the page holds none, the theme part's first fill colour is `#3a6b9e`, and `FillForegnd` calls
/// `THEMEVAL` whose no-theme reading is `#ffffff`. `saved` is what the file saved.
fn all_7(saved: &str) -> Document {
    let shape = following(ShapeBuilder::new(5)).formula("FillForegnd", r#"THEMEVAL("Light")"#, saved);
    let mut doc = DocBuilder::new().master(1, MasterBuilder::new().shape(shape)).page(1, PageBuilder::new()).build();
    doc.themes.push(blue_theme().part());
    doc
}

fn live() -> EvalOptions {
    EvalOptions { operands: OperandMode::Live, ..EvalOptions::default() }
}

/// What the three counts of are: left out as stale, agreed, differed.
fn counts(r: &RecalcReport) -> (u64, u64, usize) {
    (r.skipped.stale_theme_colour, r.agreed, r.differed.len())
}

/// The cell is left out when it saves `#3a6b9e` (the theme reading), and scored when it saves `#ffffff` (the no-theme reading) as a pass and
/// `#00ff00` as a difference. Leaving out every cell under the condition, which was the rule, scores none of the three.
#[test]
fn all_a_stale_colour_is_left_out_only_when_the_saved_value_is_the_theme_reading() {
    for options in [EvalOptions { operands: OperandMode::Cached, ..EvalOptions::default() }, live()] {
        let mode = options.operands;
        let run = |saved: &str| recalc_all_with(&all_7(saved), &Budget::default(), options.clone());
        let theme = run("#3a6b9e");
        assert_eq!(counts(&theme), (1, 0, 0), "{mode:?}: the theme reading is the stale kind: {theme:?}");
        let no_theme = run("#ffffff");
        assert_eq!(counts(&no_theme), (0, 1, 0), "{mode:?}: the no-theme reading is a pass: {no_theme:?}");
        let misfit = run("#00ff00");
        assert_eq!(counts(&misfit), (0, 0, 1), "{mode:?}: a colour neither reading gives is a difference: {misfit:?}");
        for r in [&theme, &no_theme, &misfit] {
            assert_eq!(r.accounted(), r.cells, "{mode:?}: {r:?}");
        }
        // The second reading is a probe of the cell and not a fallback of the score: the stats carry none of its theme fallbacks.
        assert_eq!(theme.stats.theme_fallbacks, 0, "{mode:?}: {theme:?}");
    }
}

/// The second reading selects the file's first theme part, so a part the reader could not read explains nothing: the cell is scored, and differs.
#[test]
fn a_theme_part_that_cannot_say_leaves_nothing_out() {
    let shape = following(ShapeBuilder::new(5)).formula("FillForegnd", r#"THEMEVAL("Light")"#, "#112233");
    let mut doc = DocBuilder::new().master(1, MasterBuilder::new().shape(shape)).page(1, PageBuilder::new()).build();
    doc.themes.push(Arc::new(bare_theme()));
    let r = recalc_all(&doc, &Budget::default());
    assert_eq!(counts(&r), (0, 0, 1), "the part gives no colour for `Light`, so the saved value is not its reading: {r:?}");
}

/// The rule's condition is as it was: a sheet that selects a theme, a page that holds a scheme, a scheme of 0 and a file with no theme part are not under
/// it, and what they give is scored by the one reading the sheet has.
#[test]
fn a_cell_outside_the_condition_is_scored_by_its_one_reading() {
    let saved = "#3a6b9e";
    let formula = r#"THEMEVAL("Light")"#;
    // The shape selects the theme part itself (40): the reading is the theme's and agrees.
    let own = FIVE.iter().fold(ShapeBuilder::new(5), |s, n| s.cell(n, "40")).formula("FillForegnd", formula, saved);
    let mut doc = DocBuilder::new().master(1, MasterBuilder::new().shape(own)).page(1, PageBuilder::new()).build();
    doc.themes.push(blue_theme().part());
    assert_eq!(counts(&recalc_all(&doc, &Budget::default())), (0, 1, 0));
    // The page selects a theme and the shape follows it: the same.
    let page =
        FIVE.iter().fold(PageBuilder::new(), |p, n| p.cell(n, "40")).shape(following(ShapeBuilder::new(1)).formula("FillForegnd", formula, saved));
    let mut doc = DocBuilder::new().page(1, page).build();
    doc.themes.push(blue_theme().part());
    assert_eq!(counts(&recalc_all(&doc, &Budget::default())), (0, 1, 0));
    // A scheme of 0 on the shape is no theme, said outright: the saved theme colour is a difference and is not left out.
    let zero = FIVE.iter().fold(ShapeBuilder::new(5), |s, n| s.cell(n, "0")).formula("FillForegnd", formula, saved);
    let mut doc = DocBuilder::new().master(1, MasterBuilder::new().shape(zero)).page(1, PageBuilder::new()).build();
    doc.themes.push(blue_theme().part());
    assert_eq!(counts(&recalc_all(&doc, &Budget::default())), (0, 0, 1));
    // No theme part in the file: the rule does not apply, and the saved colour is a difference.
    let mut doc = all_7(saved);
    doc.themes = Default::default();
    assert_eq!(counts(&recalc_all(&doc, &Budget::default())), (0, 0, 1));
}

/// In the live mode the cells a cell reads are worked out again under the second reading: a memo from the first would show the first reading's value.
/// `B` reads `A` only when the sheet reads a theme, so it is the stale kind exactly when `A` is read afresh.
#[test]
fn the_second_reading_works_the_cells_it_reads_out_afresh() {
    let shape = following(ShapeBuilder::new(5))
        .set(user("A"), formula_cell(r#"THEMEVAL("Light")"#, "#3a6b9e"))
        .set(user("B"), formula_cell(r#"IF(ISTHEMED(),User.A,THEMEVAL("Dark"))"#, "#3a6b9e"));
    let mut doc = DocBuilder::new().master(1, MasterBuilder::new().shape(shape)).page(1, PageBuilder::new()).build();
    doc.themes.push(blue_theme().part());
    let r = recalc_all_with(&doc, &Budget::default(), live());
    assert_eq!(counts(&r), (2, 0, 0), "A and B both save the theme reading: {r:?}");
    // Nothing the second reading made stays in the memo of the first: the next reading of A is the first reading's.
    assert_eq!(r.accounted(), r.cells, "{r:?}");
}

/// The echo control: a calculator that hands back the saved values agrees with every cell at the first reading, so the cell is scored and never left out,
/// and the real-file salt, which judges a wrong value put in at the first reading only, finds the echo and not the native calculator.
#[test]
fn an_echo_scores_the_cell_and_the_real_salt_catches_it() {
    let doc = all_7("#ffffff");
    let policy = SaltPolicy { seed: 7, one_in: 1 };
    let native = salted(&doc, &mut diacraft_calc::Native, policy);
    assert_eq!(counts(&native), (0, 1, 0), "{native:?}");
    let s = &native.real_salt;
    assert_eq!((s.salted, s.caught, s.missed), (1, 1, 0), "{s:?}");
    assert!(s.passed());
    let echo = salted(&doc, &mut Echo, policy);
    assert_eq!((echo.real_salt.salted, echo.real_salt.caught, echo.real_salt.missed), (1, 0, 1), "{:?}", echo.real_salt);
    assert!(!echo.real_salt.passed());
    // A cell that saves the theme reading: native leaves it out, and the echo does not (it agrees at the first reading), so the echo scores 100%.
    let stale = all_7("#3a6b9e");
    let native = salted(&stale, &mut diacraft_calc::Native, policy);
    assert_eq!(counts(&native), (1, 0, 0), "{native:?}");
    let echo = salted(&stale, &mut Echo, policy);
    assert_eq!(counts(&echo), (0, 1, 0), "an echo cannot be told from a pass, so it is scored: {echo:?}");
    assert!(!echo.real_salt.passed(), "and the salt finds it: {:?}", echo.real_salt);
}

/// A document of `n` stale cells in one master shape, each calling `THEMEVAL("Light")`, saved as given.
fn many(n: usize, saved: &str) -> Document {
    let mut shape = following(ShapeBuilder::new(5));
    for k in 0..n {
        shape = shape.set(user(&format!("C{k}")), formula_cell(r#"THEMEVAL("Light")"#, saved));
    }
    let mut doc = DocBuilder::new().master(1, MasterBuilder::new().shape(shape)).page(1, PageBuilder::new()).build();
    doc.themes.push(blue_theme().part());
    doc
}

/// The second reading doubles the work of the cells under the rule, and no more: the steps of a document whose cells are left out (both readings run)
/// are at most about twice those of one whose cells agree at the first reading (one reading), and grow in proportion to the cells.
#[test]
fn the_second_reading_costs_about_one_more_evaluation_of_the_cell() {
    let steps = |n: usize, saved: &str, options: &EvalOptions| {
        let r = recalc_all_with(&many(n, saved), &Budget::default(), options.clone());
        assert_eq!(r.accounted(), r.cells, "{r:?}");
        (r.stats.steps, r.skipped.stale_theme_colour, r.agreed)
    };
    for options in [EvalOptions { operands: OperandMode::Cached, ..EvalOptions::default() }, live()] {
        let mode = options.operands;
        let (one_reading, stale_none, agreed) = steps(200, "#ffffff", &options);
        assert_eq!((stale_none, agreed), (0, 200), "{mode:?}");
        let (two_readings, stale, agreed_none) = steps(200, "#3a6b9e", &options);
        assert_eq!((stale, agreed_none), (200, 0), "{mode:?}");
        // Measured: 5,600 and 10,200 steps cached, 4,605 and 9,605 live, so the second reading is about one more evaluation of the cell (23 to 25 steps
        // a cell against 23 to 28 for the first) and the whole is under 2.2 times.
        assert!(two_readings * 10 >= one_reading * 15, "{mode:?}: the second reading is work: {one_reading} and {two_readings}");
        assert!(two_readings * 10 <= one_reading * 22, "{mode:?}: {two_readings} steps for 200 cells with two readings, {one_reading} with one");
        // Linear in the cells: twice the cells, twice the steps.
        let (double, ..) = steps(400, "#3a6b9e", &options);
        eprintln!("{mode:?}: 200 cells, one reading {one_reading} steps, two readings {two_readings}; 400 cells, two readings {double}");
        let ratio = double as f64 / two_readings as f64;
        assert!((1.8..2.2).contains(&ratio), "{mode:?}: {two_readings} steps for 200 cells and {double} for 400");
    }
}

/// A budget that the second reading spends ends the run as any other does: the cell is `unreached` and the report accounts for every cell.
#[test]
fn a_budget_spent_in_the_second_reading_leaves_the_cell_unreached() {
    let doc = many(50, "#3a6b9e");
    let full = recalc_all(&doc, &Budget::default());
    assert_eq!((full.skipped.stale_theme_colour, full.unreached), (50, 0), "{full:?}");
    // Every budget from a few steps to what the full run took: the report accounts for all the cells, and none is counted as a pass or a difference of
    // a reading that did not finish.
    for steps in [40u64, 80, 120, 160, 200, 240, 300, full.stats.steps / 2, full.stats.steps - 1] {
        let r = recalc_all(&doc, &Budget::with_steps(steps));
        assert_eq!(r.accounted(), r.cells, "{steps}: {r:?}");
        assert!(r.budget_exhausted, "{steps}: {r:?}");
        assert_eq!((r.agreed, r.differed.len()), (0, 0), "{steps}: a reading cut short is neither a pass nor a difference: {r:?}");
    }
}

/// The reviewer's chain: `C0` calls `THEMEVAL`, and each `Ck` reads `C(k-1)` behind a branch that is not taken, so that it is a candidate (its formula names
/// `THEMEVAL`) and its second reading needs the whole chain under the second reading. All saved as the theme's colour.
fn chain(n: usize) -> Document {
    let mut shape = following(ShapeBuilder::new(5)).set(user("C0"), formula_cell(r#"THEMEVAL("Light")"#, "#3a6b9e"));
    for k in 1..n {
        shape = shape.set(user(&format!("C{k}")), formula_cell(&format!(r#"IF(FALSE,THEMEVAL("Dark"),User.C{})"#, k - 1), "#3a6b9e"));
    }
    let mut doc = DocBuilder::new().master(1, MasterBuilder::new().shape(shape)).page(1, PageBuilder::new()).build();
    doc.themes.push(blue_theme().part());
    doc
}

/// In the live mode the second reading of a cell needs the cells it reads under the second reading, and a memo made afresh for each cell
/// made a chain of 400 cost 738,600 steps against 10,400 for one reading. One memo of the second reading serves the whole walk, so each cell is worked out
/// once under each reading and the steps are linear in the chain: eight times the cells, about eight times the steps and the cells worked out (not 64).
#[test]
fn in_the_live_mode_the_second_reading_of_a_chain_is_linear_in_the_chain() {
    let run = |n: usize| {
        let r = recalc_all_with(&chain(n), &Budget::default(), live());
        assert_eq!(counts(&r), (n as u64, 0, 0), "every cell of the chain saves the theme's colour: {r:?}");
        assert_eq!(r.accounted(), r.cells, "{r:?}");
        (r.stats.steps, r.stats.cells_worked)
    };
    let ((steps_50, cells_50), (steps_400, cells_400)) = (run(50), run(400));
    eprintln!("chain of 50: {steps_50} steps, {cells_50} cells worked out; chain of 400: {steps_400} steps, {cells_400} cells worked out");
    // Once under each reading: two evaluations of a cell, whatever the length of the chain.
    assert_eq!((cells_50, cells_400), (100, 800), "each cell is worked out once under each reading");
    let steps = steps_400 as f64 / steps_50 as f64;
    assert!((6.0..10.0).contains(&steps), "{steps_50} steps for 50 cells and {steps_400} for 400 is a ratio of {steps}, and 64 is the square");
}

/// A: the rule's condition needs the sheet's own `ColorSchemeIndex` to be 65534. A sheet that holds no scheme cell at all reads no theme
/// and is not under the rule, so a theme colour it saves is a difference and is not left out.
#[test]
fn a_sheet_that_holds_no_scheme_cell_is_not_under_the_condition() {
    let shape = ShapeBuilder::new(5).formula("FillForegnd", r#"THEMEVAL("Light")"#, "#3a6b9e");
    let mut doc = DocBuilder::new().master(1, MasterBuilder::new().shape(shape)).page(1, PageBuilder::new()).build();
    doc.themes.push(blue_theme().part());
    let r = recalc_all(&doc, &Budget::default());
    assert_eq!(counts(&r), (0, 0, 1), "{r:?}");
}

/// A project rule: a scheme cell the sheet holds as an outright 0 stays 0 in the second reading, so `THEMEVAL` reads no theme in both and
/// the saved theme colour is a difference. (`ThemeIndex` 0 with the other four following the page.)
#[test]
fn an_outright_zero_scheme_cell_stays_zero_in_the_second_reading() {
    for zero in FIVE {
        let shape = FIVE.iter().fold(ShapeBuilder::new(5), |s, n| s.cell(n, if *n == zero { "0" } else { "65534" })).formula(
            "FillForegnd",
            r#"THEMEVAL("Light")"#,
            "#3a6b9e",
        );
        let mut doc = DocBuilder::new().master(1, MasterBuilder::new().shape(shape)).page(1, PageBuilder::new()).build();
        doc.themes.push(blue_theme().part());
        let r = recalc_all(&doc, &Budget::default());
        // A named colour reads no theme when `ThemeIndex`, `ColorSchemeIndex` or `FontSchemeIndex` is 0; the effect and connector schemes
        // only matter to the lists, so an outright 0 there leaves the colour to the first part and the cell is the stale kind.
        let reads_no_theme = matches!(zero, "ThemeIndex" | "ColorSchemeIndex" | "FontSchemeIndex");
        let want = if reads_no_theme { (0, 0, 1) } else { (1, 0, 0) };
        assert_eq!(counts(&r), want, "{zero} at 0 outright: {r:?}");
    }
}

/// Only the five scheme cells read the first part in the second reading. `VariationColorIndex`, which is absent here, reads 0 and so the
/// first variant: `VariantColor1` is its first colour, `#112233`. A reading that gave the sentinel to the variant index too would find no variant.
#[test]
fn only_the_five_scheme_cells_read_the_first_part() {
    let shape = following(ShapeBuilder::new(5)).formula("FillForegnd", r#"THEMEVAL("VariantColor1")"#, "#112233");
    let mut doc = DocBuilder::new().master(1, MasterBuilder::new().shape(shape)).page(1, PageBuilder::new()).build();
    doc.themes.push(ThemeXml::default().part());
    let r = recalc_all(&doc, &Budget::default());
    assert_eq!(counts(&r), (1, 0, 0), "{r:?}");
}

/// A page that holds its own 65534 is not a page that holds none. `THEMEVAL` reads the number 65534, which no part holds, and takes the first
/// part: the sheet selects a theme, so the cell is outside the rule and its theme colour is a pass.
#[test]
fn a_page_that_holds_65534_itself_is_not_a_page_that_holds_none() {
    let shape = following(ShapeBuilder::new(1)).formula("FillForegnd", r#"THEMEVAL("Light")"#, "#3a6b9e");
    let page = FIVE.iter().fold(PageBuilder::new(), |p, n| p.cell(n, "65534")).shape(shape);
    let mut doc = DocBuilder::new().page(1, page).build();
    doc.themes.push(blue_theme().part());
    for options in [EvalOptions { operands: OperandMode::Cached, ..EvalOptions::default() }, live()] {
        let r = recalc_all_with(&doc, &Budget::default(), options);
        assert_eq!(counts(&r), (0, 1, 0), "{r:?}");
    }
}

/// B: the first reading's memo is back after the second. `C` reads `A`, which is stale: `A` is left out, and `C` (no `THEMEVAL` in it, so
/// scored) reads `A` as the first reading made it, white, and agrees with the white it saves.
#[test]
fn the_first_readings_memo_is_back_after_the_second() {
    let shape = following(ShapeBuilder::new(5))
        .set(user("A"), formula_cell(r#"THEMEVAL("Light")"#, "#3a6b9e"))
        .set(user("C"), formula_cell("User.A", "#ffffff"));
    let mut doc = DocBuilder::new().master(1, MasterBuilder::new().shape(shape)).page(1, PageBuilder::new()).build();
    doc.themes.push(blue_theme().part());
    let r = recalc_all_with(&doc, &Budget::default(), live());
    assert_eq!(counts(&r), (1, 1, 0), "{r:?}");
}

/// And the brief's "its bytes charged": the cells the second reading memoises are held memory like any other. 100 stale cells in the live mode
/// hold, at the peak, the memo of the first reading and the memo of the second, so the peak is higher by about the second memo (a cell is 512 bytes of entry
/// and 256 of key) than for 100 cells that agree at the first reading and need no second one.
#[test]
fn the_memo_of_the_second_reading_is_charged_to_the_bytes_of_the_run() {
    let peak = |saved: &str| recalc_all_with(&many(100, saved), &Budget::default(), live()).stats.bytes_peak;
    let (one, two) = (peak("#ffffff"), peak("#3a6b9e"));
    assert!(two >= one + 100 * 700, "the peak is {one} bytes with one reading and {two} with two: the second memo is not charged");
}

/// The cells left out as stale are tallied by the functions their formulas call, so that a jump in the count can be traced to a kind of
/// formula (a colour the first part gives that is also the no-theme colour is the case the rule can mask).
#[test]
fn the_cells_left_out_as_stale_are_tallied_by_function() {
    let shape = following(ShapeBuilder::new(5))
        .set(user("A"), formula_cell(r#"THEMEVAL("Light")"#, "#3a6b9e"))
        .set(user("B"), formula_cell(r#"IF(TRUE,THEMEVAL("Light"),0)"#, "#3a6b9e"))
        .set(user("C"), formula_cell(r#"THEMEVAL("Light")"#, "#ffffff"));
    let mut doc = DocBuilder::new().master(1, MasterBuilder::new().shape(shape)).page(1, PageBuilder::new()).build();
    doc.themes.push(blue_theme().part());
    let r = recalc_all(&doc, &Budget::default());
    assert_eq!(counts(&r), (2, 1, 0), "A and B are left out, C agrees with the first reading: {r:?}");
    let tally: Vec<(&str, u64)> = r.stale_theme_colour_by_function.iter().map(|(k, v)| (k.as_str(), *v)).collect();
    assert_eq!(tally, [("IF", 1), ("THEMEVAL", 2)], "{r:?}");
}
