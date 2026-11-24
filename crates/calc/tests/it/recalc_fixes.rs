//! The fixes the first recalculation score ranked, one test for each, at the values and counts real files hold.
//!
//! `THEMEVAL` with no theme, the comparer's colour index
//! (`values_agree`), `THEMEPROP` on a sheet that reads no theme (the kind), the
//! operands that are markers and no value, and the real-file salt (`salt.rs`).

use std::sync::Arc;

use diacraft_calc::{
    Budget, Calculator, CellAddress, CellEval, CellKind, EvalOptions, Evaluator, OperandMode, RealSalt, SaltClass, SaltPolicy, Value, recalc_all,
    recalc_all_salted,
};
use diacraft_doc::{Cell, CellAddr, Document, Owner, RowKey, ShapeRef, SheetRef, Value as DocValue};
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};
use diacraft_testkit::theme::ThemeXml;

use crate::{E, mismatch};

const FIVE: [&str; 5] = ["ThemeIndex", "ColorSchemeIndex", "EffectSchemeIndex", "ConnectorSchemeIndex", "FontSchemeIndex"];

fn c(hex: &str) -> E {
    let n = u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap();
    E::Col((n >> 16) as u8, (n >> 8) as u8, n as u8)
}

fn user(name: &str) -> CellAddr {
    CellAddr::in_row("User", None, RowKey::name(name), "Value")
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

fn on_schemes(s: ShapeBuilder, n: &str) -> ShapeBuilder {
    FIVE.iter().fold(s, |s, name| s.cell(name, n))
}

fn expect(id: &str, got: Value, want: E) {
    assert_eq!(mismatch(&got, &want), None, "{id}: {got:?}");
}

/// `formula` worked out in `User.Probe` of a shape with the five scheme cells at `schemes` ("-" for none), over a root style holding `root`.
fn with_root(root: &[(&str, &str)], schemes: &str, formula: &str) -> Value {
    let shape = if schemes == "-" { ShapeBuilder::new(1) } else { on_schemes(ShapeBuilder::new(1), schemes) }.row_formula(
        "User",
        None,
        RowKey::name("Probe"),
        "Value",
        formula,
        "0",
    );
    let doc = DocBuilder::new().root(|r| root.iter().fold(r, |r, (n, v)| r.cell(n, v))).page(1, PageBuilder::new().shape(shape)).build();
    let at = CellAddress::new(SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0)), crate::probe_addr());
    Evaluator::new(&doc, &mut diacraft_calc::EvalCtx::default()).evaluate(&at)
}

// ---- item 1: the connector names read the root style's cell they map to -------------------------------------------------------------

/// What a root style holds in every file: a medium arrow size, a 0.75-point weight and a solid line.
const ROOT: [(&str, &str); 4] = [("BeginArrowSize", "2"), ("EndArrowSize", "2"), ("LineWeight", "0.010416666666666666"), ("LinePattern", "1")];

#[test]
fn the_connector_names_with_no_theme_read_the_root_styles_cell_they_map_to() {
    // `ConnectorBeginSize` is `BeginArrowSize`, `ConnectorEndSize` is `EndArrowSize`, `ConnectorWeight` is `LineWeight` and
    // `ConnectorPattern` is `LinePattern`. They gave 0, from a cell that no style has, on 538 cells (162 files, 61 sources).
    for (formula, want) in [
        (r#"THEMEVAL("ConnectorBeginSize")"#, E::N(2.0)),
        (r#"THEMEVAL("ConnectorEndSize")"#, E::N(2.0)),
        (r#"THEMEVAL("ConnectorWeight")"#, E::Any(0.010416666666666666)),
        (r#"THEMEVAL("ConnectorPattern")"#, E::N(1.0)),
        // Letter case does not matter.
        (r#"THEMEVAL("connectorbeginsize")"#, E::N(2.0)),
    ] {
        expect(formula, with_root(&ROOT, "0", formula), want);
    }
    // And they read the root's own number, and not a constant that the files happen to hold.
    let other = [("BeginArrowSize", "5"), ("EndArrowSize", "4"), ("LineWeight", "0.02"), ("LinePattern", "7")];
    expect("size 5", with_root(&other, "0", r#"THEMEVAL("ConnectorBeginSize")"#), E::N(5.0));
    expect("size 4", with_root(&other, "0", r#"THEMEVAL("ConnectorEndSize")"#), E::N(4.0));
    expect("weight", with_root(&other, "0", r#"THEMEVAL("ConnectorWeight")"#), E::Any(0.02));
    expect("pattern", with_root(&other, "0", r#"THEMEVAL("ConnectorPattern")"#), E::N(7.0));
    // The five cells absent are the same as 0.
    expect("no cells", with_root(&other, "-", r#"THEMEVAL("ConnectorBeginSize")"#), E::N(5.0));
}

#[test]
fn the_other_names_of_the_table_read_their_mapped_cell_and_the_plain_ones_as_before() {
    let root = [
        ("BeginArrow", "4"),
        ("EndArrow", "13"),
        ("CompoundType", "0"),
        ("Rounding", "0.25"),
        ("ShdwPattern", "1"),
        ("LineWeight", "0.0104"),
        ("FillPattern", "1"),
    ];
    // Learn's `LineBegin` and MS-VSDX's `ConnectorBegin` are `BeginArrow`; both rounding names are `Rounding`.
    expect("LineBegin", with_root(&root, "0", r#"THEMEVAL("LineBegin")"#), E::N(4.0));
    expect("ConnectorBegin", with_root(&root, "0", r#"THEMEVAL("ConnectorBegin")"#), E::N(4.0));
    expect("ConnectorEnd", with_root(&root, "0", r#"THEMEVAL("ConnectorEnd")"#), E::N(13.0));
    expect("ConnectorRounding", with_root(&root, "0", r#"THEMEVAL("ConnectorRounding")"#), E::Any(0.25));
    expect("ShadowPattern", with_root(&root, "0", r#"THEMEVAL("ShadowPattern")"#), E::N(1.0));
    // A name that is the cell's own is the same as before, and a name the table lacks is still an error.
    expect("LineWeight", with_root(&root, "0", r#"THEMEVAL("LineWeight")"#), E::Any(0.0104));
    expect("FillPattern", with_root(&root, "0", r#"THEMEVAL("FillPattern")"#), E::N(1.0));
    expect("no such name", with_root(&root, "0", r#"THEMEVAL("ConnectorSize")"#), E::Err(diacraft_calc::CellError::Value));
    // A default replaces the root, as it always did.
    expect("default", with_root(&root, "0", r#"THEMEVAL("ConnectorBeginSize",9)"#), E::N(9.0));
}

#[test]
fn a_theme_that_is_selected_does_not_read_the_root_for_a_connector_name() {
    // The root is for the case with no theme: with one selected and no part in the file, the call is unsupported and the
    // saved value stands, as it was.
    expect("selected", with_root(&ROOT, "40", r#"THEMEVAL("ConnectorBeginSize")"#), E::Unsupported);
}

// ---- item 3: VariantColor1 to 7 with no theme ----------------------------------------------------------------------------------------

#[test]
fn a_variant_colour_with_no_theme_is_the_tables_accent_colour() {
    // With R6: 760 cells (44 files, 23 sources) save the no-theme table's accent colours. The first variant colour is `AccentColor`,
    // and the sixth `AccentColor6`.
    for (n, hex) in [(1, "c05046"), (2, "9dbb61"), (3, "ab9ac0"), (4, "4bacc6"), (5, "f59d56"), (6, "ffc000")] {
        let formula = format!(r#"THEMEVAL("VariantColor{n}")"#);
        expect(&formula, with_root(&[], "0", &formula), c(hex));
    }
    // The name is read without regard to case, and the five cells absent are no theme either.
    expect("case", with_root(&[], "-", r#"THEMEVAL("variantcolor2")"#), c("9dbb61"));
    // A default replaces the table.
    expect("default", with_root(&[], "0", r#"THEMEVAL("VariantColor1",RGB(1,2,3))"#), c("010203"));
    // The seventh is the table's `Dark`, black (settled: 39 cells in files with no theme part hold it).
    expect("seventh", with_root(&[], "0", r#"THEMEVAL("VariantColor7")"#), c("000000"));
    // An eighth is no name at all.
    expect("eighth", with_root(&[], "0", r#"THEMEVAL("VariantColor8")"#), E::Err(diacraft_calc::CellError::Value));
}

// ---- item 4: THEMEVAL() in a cell the table does not cover ----------------------------------------------------------------------------

/// A shape whose character row 0 holds a bare `THEMEVAL()` with the marker in `ColorTrans`, a cell the table does not cover.
fn bare_in_uncovered(schemes: &str) -> ShapeBuilder {
    on_schemes(ShapeBuilder::new(1), schemes).row_formula("Character", None, RowKey::Ix(0), "ColorTrans", "THEMEVAL()", "Themed")
}

fn bare_value(root: Option<&str>, shape: ShapeBuilder) -> Value {
    let doc = DocBuilder::new()
        .root(|r| match root {
            Some(v) => r.row_cell("Character", None, RowKey::Ix(0), "ColorTrans", v),
            None => r,
        })
        .page(1, PageBuilder::new().shape(shape))
        .build();
    let at = CellAddress::new(SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0)), CellAddr::in_row("Character", None, RowKey::Ix(0), "ColorTrans"));
    Evaluator::new(&doc, &mut diacraft_calc::EvalCtx::default()).evaluate(&at)
}

#[test]
fn a_bare_call_with_no_theme_in_a_cell_the_table_does_not_cover_is_not_an_error() {
    // "the root style's cell of the host's name", and 0 where the root holds none. The call gave `#VALUE!` (the rule for a
    // case no sample held), and files save 0 on 661 cells (33 files, 18 sources).
    expect("no root cell", bare_value(None, bare_in_uncovered("0")), E::N(0.0));
    // The root's own cell, and not a constant 0.
    expect("root cell", bare_value(Some("0.25"), bare_in_uncovered("0")), E::Any(0.25));
    // A direct cell, in the same way.
    let direct = on_schemes(ShapeBuilder::new(1), "0").formula("ShdwBkgnd", "THEMEVAL()", "Themed");
    let doc = doc_of(direct);
    let at = CellAddress::new(SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0)), CellAddr::direct("ShdwBkgnd"));
    expect("direct", Evaluator::new(&doc, &mut diacraft_calc::EvalCtx::default()).evaluate(&at), E::N(0.0));
}

#[test]
fn a_bare_call_in_an_uncovered_cell_under_a_theme_is_the_root_styles_cell_too() {
    // As rewrote it: "a host in no table gives the root style's cell of the host's own name, with or without a theme, and no
    // call with no argument gives `#VALUE!`". (kept `#VALUE!` under a theme, and the rule has moved.)
    let shape = bare_in_uncovered("40");
    let mut doc = DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build();
    doc.themes.push(ThemeXml::default().part());
    let at = CellAddress::new(SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0)), CellAddr::in_row("Character", None, RowKey::Ix(0), "ColorTrans"));
    expect("theme", Evaluator::new(&doc, &mut diacraft_calc::EvalCtx::default()).evaluate(&at), E::N(0.0));
}

// ---- item 7: a number against a saved colour is an exact index ------------------------------------------------------------------------

/// Whether a cell `User.X` worked out from `formula` agrees with the saved colour `saved`.
fn colour_agrees(formula: &str, saved: &str) -> bool {
    let r = recalc_all(&doc_of(ShapeBuilder::new(1).set(user("X"), cell(formula, saved, None, None))), &Budget::default());
    assert_eq!(r.agreed + r.differed.len() as u64, 1, "{formula} against {saved}: {r:?}");
    r.agreed == 1
}

#[test]
fn a_computed_number_agrees_with_a_saved_colour_only_as_an_exact_palette_index() {
    // 0 is black, 1 white, 2 red. The comparer cut a number to its index, so 1.75 agreed with white (the
    // first recalculation score: 2,198 cells of 431,825 in 5 files, 3 sources).
    assert!(colour_agrees("1", "#ffffff"), "index 1 is white");
    assert!(colour_agrees("2", "#ff0000"), "index 2 is red");
    assert!(colour_agrees("0", "#000000"), "index 0 is black");
    assert!(!colour_agrees("1.75", "#ffffff"), "1.75 is not index 1");
    assert!(!colour_agrees("1.75", "#ff0000"), "and not index 2 either");
    assert!(!colour_agrees("0.75", "#000000"), "0.75 is not index 0");
    assert!(!colour_agrees("0.75", "#ffffff"), "and not index 1");
    assert!(!colour_agrees("-0.5", "#000000"), "a negative fraction is no index");
    // An index within the comparer's tolerance of a whole number is that index (a number a double arithmetic made).
    assert!(colour_agrees("1+1E-12", "#ffffff"));
    assert!(!colour_agrees("2-1E-12", "#ffffff"), "1.999999999999 is index 2, which is red");
    // Past the palette, or not a number a palette holds: no colour is invented.
    assert!(!colour_agrees("1E300", "#ffffff"));
    assert!(!colour_agrees("-1", "#ffffff"));
}

// ---- item 5: THEMEPROP on a sheet that reads no theme ----------------------------------------------------------------------------------

fn scored_with(shape: ShapeBuilder, themes: usize) -> diacraft_calc::RecalcReport {
    let mut doc = doc_of(shape);
    for _ in 0..themes {
        doc.themes.push(ThemeXml::default().part());
    }
    recalc_all(&doc, &Budget::default())
}

/// For, a project rule: on a sheet whose scheme cells read 0 the oracle scores a `THEMEPROP` cell: a saved
/// `Embellishment` of 2 is a pass and a saved 7 a difference (the leave-out it had scored neither), and a `Multiformat` of 0 is a pass and a 1 a difference.
/// Nothing is left out for it (the reason `stale_theme_property` is gone).
#[test]
fn themeprop_on_a_sheet_that_reads_no_theme_is_scored_and_not_left_out() {
    let embellishment = |saved: &str| cell(r#"THEMEPROP("Embellishment")"#, saved, None, None);
    let multiformat = |saved: &str| cell(r#"THEMEPROP("Multiformat")"#, saved, None, None);
    let on = |s: ShapeBuilder| on_schemes(s, "0");
    let r = scored_with(on(ShapeBuilder::new(1)).set(user("E"), embellishment("2")), 0);
    assert_eq!((r.left_out(), r.agreed, r.differed.len()), (0, 1, 0), "a saved 2 is a pass: {r:?}");
    let r = scored_with(on(ShapeBuilder::new(1)).set(user("E"), embellishment("7")), 0);
    assert_eq!((r.left_out(), r.agreed, r.differed.len()), (0, 0, 1), "a saved 7 is a difference: {r:?}");
    assert_eq!(r.accounted(), r.cells, "{r:?}");
    let r = scored_with(on(ShapeBuilder::new(1)).set(user("M"), multiformat("0")), 1);
    assert_eq!((r.left_out(), r.agreed, r.differed.len()), (0, 1, 0), "{r:?}");
    let r = scored_with(on(ShapeBuilder::new(1)).set(user("M"), multiformat("1")), 1);
    assert_eq!((r.left_out(), r.agreed, r.differed.len()), (0, 0, 1), "{r:?}");
    // Any one of the five cells at 0 reads no theme: the effect scheme alone, with a theme part in the file.
    let shape = on_schemes(ShapeBuilder::new(1), "40").cell("EffectSchemeIndex", "0").set(user("E"), embellishment("2"));
    let r = scored_with(shape, 1);
    assert_eq!((r.left_out(), r.agreed, r.differed.len()), (0, 1, 0), "{r:?}");
    // The five cells absent (and 65534 over a page that holds none) read no theme either.
    let r = scored_with(ShapeBuilder::new(1).set(user("E"), embellishment("2")), 0);
    assert_eq!((r.left_out(), r.agreed, r.differed.len()), (0, 1, 0), "{r:?}");
    // A `THEMEPROP` inside a longer formula is scored the same way, with the 2 in it.
    let longer = cell(r#"THEMEPROP("Embellishment")+1"#, "3", None, None);
    let r = scored_with(on(ShapeBuilder::new(1)).set(user("L"), longer), 0);
    assert_eq!((r.left_out(), r.agreed, r.differed.len()), (0, 1, 0), "{r:?}");
}

#[test]
fn themeprop_under_an_explicit_theme_and_other_cells_on_a_sheet_with_no_theme_are_scored() {
    // With a theme selected the call is scored: here the theme part's first style variant says 2, as the saved value does.
    let shape = on_schemes(ShapeBuilder::new(1), "40").set(user("E"), cell(r#"THEMEPROP("Embellishment")"#, "2", None, None));
    let r = scored_with(shape, 1);
    assert_eq!((r.left_out(), r.agreed, r.differed.len()), (0, 1, 0), "{r:?}");
    // A cell with no THEMEPROP on a sheet with no theme is scored as ever: 2 against 1+1, and 3 against it.
    let shape = on_schemes(ShapeBuilder::new(1), "0").set(user("A"), cell("1+1", "2", None, None)).set(user("B"), cell("1+1", "3", None, None));
    let r = scored_with(shape, 0);
    assert_eq!((r.left_out(), r.agreed, r.differed.len()), (0, 1, 1), "{r:?}");
}

// ---- item 6: a marker operand has its own reason ----------------------------------------------------------------------------------------

#[test]
fn a_cell_reading_a_themed_operand_has_a_reason_of_its_own_and_is_no_unsupported_function() {
    // 97,954 of the 97,961 cells the harness called `unsupported_unlisted` read an operand saved as `Themed`; 7 read a date past the year 9999.
    let themed = Cell::with_formula("THEMEVAL()", Some(DocValue::from_text("Themed")));
    let far = cell("DATE(1899,12,29)", "30177-04-13T00:00:00", Some("DATE"), None);
    let shape = ShapeBuilder::new(1)
        .set(user("A"), themed)
        .set(user("B"), cell("User.A", "7", None, None))
        .set(user("C"), cell("User.A+1", "8", None, None))
        .set(user("D"), far)
        .set(user("E"), cell("User.D", "2", None, None));
    let r = scored_with(shape, 0);
    assert_eq!((r.skipped.themed_operand, r.skipped.far_year_operand), (2, 1), "{r:?}");
    assert_eq!(r.unsupported_cells(), 0, "no function is unsupported: {:?}", r.unsupported);
    // The marker cells themselves keep their own reasons.
    assert_eq!((r.skipped.themed, r.skipped.far_year), (1, 1), "{r:?}");
    assert_eq!(r.accounted(), r.cells, "{r:?}");
}

#[test]
fn a_function_a_file_made_up_is_unsupported_even_if_it_is_named_like_a_marker() {
    // The markers were `THEMED` and `FARYEAR`: a call to a function of that name was counted as a cell reading a marker. They are not names now.
    let shape = ShapeBuilder::new(1).set(user("A"), cell("THEMED(1)", "1", None, None)).set(user("B"), cell("FARYEAR(1)", "1", None, None));
    let r = scored_with(shape, 0);
    assert_eq!((r.skipped.themed_operand, r.skipped.far_year_operand), (0, 0), "{r:?}");
    assert_eq!(r.unsupported.get("THEMED"), Some(&1), "{:?}", r.unsupported);
    assert_eq!(r.unsupported.get("FARYEAR"), Some(&1), "{:?}", r.unsupported);
}

// ---- item 8: the real-file salt ---------------------------------------------------------------------------------------------------------

/// A document with an agreeing cell of each kind of value, and one compared by its error alone.
fn every_kind() -> Document {
    doc_of(
        ShapeBuilder::new(1)
            .set(user("Num"), cell("1+1", "2", None, None))
            .set(user("Len"), cell("2.5 in", "2.5", Some("IN"), None))
            .set(user("Date"), cell("DATE(2031,3,7)", "2031-03-07T00:00:00", Some("DATE"), None))
            .set(user("Bool"), cell("2>1", "1", Some("BOOL"), None))
            .set(user("False"), cell("2<1", "0", Some("BOOL"), None))
            .set(user("Text"), cell("\"ab\"&\"cd\"", "abcd", Some("STR"), None))
            .set(user("Colour"), cell("RGB(1,2,3)", "#010203", None, None))
            .set(user("Money"), cell("12.5EUR", "12.5000EUR", Some("CY"), None))
            .set(user("Point"), cell("PNT(1,2)", "PNT(1,2)", Some("PNT"), None))
            .set(user("Poly"), cell("POLYLINE(1,1,2,2)", "POLYLINE(1, 1, 2, 2)", Some("POLYLINE"), None))
            .set(user("Err"), cell("1/0", "1", None, Some("#DIV/0!"))),
    )
}

pub(crate) fn salted(doc: &Document, calc: &mut dyn Calculator, policy: SaltPolicy) -> diacraft_calc::RecalcReport {
    recalc_all_salted(doc, &Budget::default(), EvalOptions { operands: OperandMode::Cached, ..EvalOptions::default() }, calc, Some(policy))
}

/// A calculator that hands back what the file saved: the echo the real-file salt exists to catch.
pub(crate) struct Echo;

impl Calculator for Echo {
    fn calculate(&mut self, ev: &mut Evaluator<'_, '_>, at: &CellAddress) -> Arc<CellEval> {
        let mut e = (*ev.evaluate_cell(at)).clone();
        if e.kind == CellKind::Formula {
            if let Some(saved) = e.saved.clone() {
                e.raw = saved.clone();
                e.view = saved;
            }
            e.error = e.saved_error;
            e.unsupported = None;
        }
        Arc::new(e)
    }
}

#[test]
fn the_real_salt_catches_every_kind_of_value_in_a_file_and_the_evaluator_passes_it() {
    let doc = every_kind();
    let r = salted(&doc, &mut diacraft_calc::Native, SaltPolicy::STANDARD);
    let s: &RealSalt = &r.real_salt;
    assert_eq!(r.agreed, 11, "{r:?}");
    // The first agreed cell of each kind is salted: number, date, bool, text, colour, currency, point, constructor, and an error.
    for class in SaltClass::ALL {
        let t = s.by_class.get(&class).copied().unwrap_or_default();
        assert!(t.salted >= 1, "{class:?} was not salted: {s:?}");
        assert_eq!(t.salted, t.caught, "{class:?} was not caught: {s:?}");
    }
    assert!(s.passed() && s.misses.is_empty(), "{s:?}");
    assert_eq!(s.salted, s.caught + s.by_allowance + s.missed, "{s:?}");
    assert_eq!(s.salted, s.caught, "{s:?}");
    assert_eq!((s.by_allowance, s.not_checked, s.too_large), (0, 0, 0));
    // It changes nothing in the score: the same counts as an unsalted run, the same steps and no budget flag.
    let plain = recalc_all(&doc, &Budget::default());
    assert_eq!((r.agreed, r.differed.len(), r.cells, r.skipped.total()), (plain.agreed, plain.differed.len(), plain.cells, plain.skipped.total()));
    assert_eq!((r.stats, r.budget_exhausted), (plain.stats, plain.budget_exhausted));
}

#[test]
fn an_echo_calculator_fails_the_real_salt_on_a_real_file() {
    // The instrument the first recalculation score could not catch: an echo agrees with every cell of a real file, and the fixed control is one
    // synthetic document. With a wrong value put in a real cell, the echo hands that back and agrees: a miss for every kind of value.
    let r = salted(&every_kind(), &mut Echo, SaltPolicy::STANDARD);
    let s = &r.real_salt;
    assert!(!s.passed(), "{s:?}");
    assert_eq!(s.missed, s.salted, "the echo agrees with every salted cell: {s:?}");
    assert_eq!((s.caught, s.by_allowance), (0, 0));
    // Every kind of value, the error apart: the echo hands back the value the file saved even where the formula fails, so its cell is a number.
    for class in SaltClass::ALL.into_iter().filter(|c| *c != SaltClass::Error) {
        assert!(s.by_class.get(&class).is_some_and(|t| t.salted >= 1 && t.caught == 0), "{class:?}: {s:?}");
    }
    // The first misses are kept, by place and kind, to say where.
    assert!(!s.misses.is_empty() && s.misses.len() <= diacraft_calc::MAX_MISSES_KEPT, "{s:?}");
}

#[test]
fn the_real_salt_of_a_boolean_is_a_flip_that_cannot_agree() {
    // The first run's own salting left Boolean flips agreeing. A saved Boolean is flipped to the other Boolean, which differs from the Boolean
    // the formula gave and from the number that stands for it.
    for (formula, saved) in [("2>1", "1"), ("2<1", "0"), ("1", "1"), ("0", "0")] {
        let doc = doc_of(ShapeBuilder::new(1).set(user("B"), cell(formula, saved, Some("BOOL"), None)));
        let r = salted(&doc, &mut diacraft_calc::Native, SaltPolicy::STANDARD);
        assert_eq!((r.real_salt.salted, r.real_salt.caught), (1, 1), "{formula}: {:?}", r.real_salt);
    }
    // A saved number of a Boolean formula (a file that holds `1` with no unit) is salted as a number, away from both 0 and 1.
    let doc = doc_of(ShapeBuilder::new(1).set(user("B"), cell("2>1", "1", None, None)));
    let r = salted(&doc, &mut diacraft_calc::Native, SaltPolicy::STANDARD);
    assert_eq!((r.real_salt.salted, r.real_salt.caught, r.real_salt.missed), (1, 1, 0), "{:?}", r.real_salt);
}

#[test]
fn the_real_salt_of_a_number_against_a_colour_index_is_no_index() {
    // A cell that gave the colour white and saved the index 1: the salted index is a half further, which names no palette entry.
    let doc = doc_of(ShapeBuilder::new(1).set(user("C"), cell("RGB(255,255,255)", "1", None, None)));
    let r = salted(&doc, &mut diacraft_calc::Native, SaltPolicy::STANDARD);
    assert_eq!((r.agreed, r.real_salt.salted, r.real_salt.caught), (1, 1, 1), "{r:?}");
}

#[test]
fn atan2_s_zero_length_rule_is_counted_as_an_allowance_and_not_as_a_miss() {
    // Both operands under 1E-6 and any saved value agrees. A wrong value agrees there by design.
    let doc = doc_of(ShapeBuilder::new(1).set(user("A"), cell("ATAN2(3E-8,-2E-8)", "0.4", None, None)));
    let r = salted(&doc, &mut diacraft_calc::Native, SaltPolicy::STANDARD);
    let s = &r.real_salt;
    assert_eq!((r.agreed, s.salted, s.caught, s.by_allowance, s.missed), (1, 1, 0, 1, 0), "{r:?}");
    assert!(s.passed());
    // A call with real lengths has no allowance, and the same salt is caught.
    let doc = doc_of(ShapeBuilder::new(1).set(user("A"), cell("ATAN2(1,1)", "0.7853981633974483", None, None)));
    let r = salted(&doc, &mut diacraft_calc::Native, SaltPolicy::STANDARD);
    assert_eq!((r.real_salt.salted, r.real_salt.caught, r.real_salt.by_allowance), (1, 1, 0), "{:?}", r.real_salt);
}

#[test]
fn the_real_salt_is_a_fixed_small_sample_with_at_least_one_cell() {
    // 3,000 agreeing numbers: the first is salted, and about one in a thousand of the rest, from a fixed seed, so a second run salts the same
    // cells and a different seed salts others.
    let mut shape = ShapeBuilder::new(1);
    for i in 0..3_000 {
        shape = shape.set(user(&format!("N{i}")), cell(&format!("{i}+1"), &(i + 1).to_string(), None, None));
    }
    let doc = doc_of(shape);
    let a = salted(&doc, &mut diacraft_calc::Native, SaltPolicy::STANDARD);
    let b = salted(&doc, &mut diacraft_calc::Native, SaltPolicy::STANDARD);
    assert_eq!(a.real_salt, b.real_salt, "a second run salts the same cells");
    let n = a.real_salt.salted;
    assert!((1..=12).contains(&n), "{n} of 3,000 salted");
    assert_eq!((a.real_salt.caught, a.agreed), (n, 3_000));
    // One in 1: every cell is salted. A seed that picks none still salts the first.
    let all = salted(&doc, &mut diacraft_calc::Native, SaltPolicy { seed: 1, one_in: 1 });
    assert_eq!((all.real_salt.salted, all.real_salt.caught), (3_000, 3_000));
    // And an echo is caught on the sample, as on all.
    let echo = salted(&doc, &mut Echo, SaltPolicy::STANDARD);
    assert_eq!((echo.real_salt.missed, echo.real_salt.caught), (n, 0));
    // A document with no agreeing cell has nothing to salt.
    let none = salted(&doc_of(ShapeBuilder::new(1).set(user("W"), cell("1+1", "3", None, None))), &mut diacraft_calc::Native, SaltPolicy::STANDARD);
    assert_eq!((none.agreed, none.real_salt.salted), (0, 0));
}

#[test]
fn the_real_salt_does_not_read_its_wrong_value_into_the_cells_that_read_the_cell() {
    // The cell under test alone has the wrong value; a cell that reads it as an operand reads what the file saved. Here `B` reads `A`, and `A`
    // is salted: `B` is scored (and agrees) from the real `A` in the walk, and the salt only changes `A`'s own comparison.
    let doc = doc_of(ShapeBuilder::new(1).set(user("A"), cell("1+1", "2", None, None)).set(user("B"), cell("User.A+1", "3", None, None)));
    let r = salted(&doc, &mut diacraft_calc::Native, SaltPolicy { seed: 7, one_in: 1 });
    assert_eq!((r.agreed, r.real_salt.salted, r.real_salt.caught, r.real_salt.missed), (2, 2, 2, 0), "{r:?}");
}

#[test]
fn the_real_salt_works_on_the_live_operand_mode_too() {
    // `Live` remembers cells it worked out; the cell under salt is worked out afresh and the memo is not poisoned by it.
    let doc = every_kind();
    let options = EvalOptions { operands: OperandMode::Live, ..EvalOptions::default() };
    let r = recalc_all_salted(&doc, &Budget::default(), options.clone(), &mut diacraft_calc::Native, Some(SaltPolicy { seed: 3, one_in: 1 }));
    assert_eq!(r.agreed, 11, "{r:?}");
    assert_eq!((r.real_salt.salted, r.real_salt.caught), (11, 11), "{:?}", r.real_salt);
    let echo = recalc_all_salted(&doc, &Budget::default(), options, &mut Echo, Some(SaltPolicy { seed: 3, one_in: 1 }));
    assert_eq!((echo.real_salt.salted, echo.real_salt.missed), (11, 11), "{:?}", echo.real_salt);
}

#[test]
fn a_walk_that_spent_its_budget_salts_nothing_and_is_not_marked_by_the_salt() {
    // The salt works on what the walk left. A budget of a few steps leaves none, and the planned cells are counted as not checked.
    let doc = every_kind();
    let used = recalc_all(&doc, &Budget::default()).stats.steps;
    let tight = Budget { steps: used, ..Budget::default() };
    let r = recalc_all_salted(
        &doc,
        &tight,
        EvalOptions { operands: OperandMode::Cached, ..EvalOptions::default() },
        &mut diacraft_calc::Native,
        Some(SaltPolicy::STANDARD),
    );
    // The walk fitted its budget exactly; the salt had nothing left, and says so. The report does not call the budget spent.
    assert!(!r.budget_exhausted && !r.stats.budget_exhausted, "{r:?}");
    assert_eq!(r.real_salt.salted, r.real_salt.caught + r.real_salt.by_allowance + r.real_salt.missed);
    assert!(r.real_salt.not_checked > 0, "{:?}", r.real_salt);
    assert_eq!(r.agreed, 11, "{r:?}");
}

#[test]
fn a_report_merged_sums_the_real_salt_of_both() {
    let mut a = salted(&every_kind(), &mut diacraft_calc::Native, SaltPolicy::STANDARD);
    let b = salted(&every_kind(), &mut Echo, SaltPolicy::STANDARD);
    let (sa, sb) = (a.real_salt.clone(), b.real_salt.clone());
    a.merge(b);
    assert_eq!(a.real_salt.salted, sa.salted + sb.salted);
    assert_eq!((a.real_salt.caught, a.real_salt.missed), (sa.caught + sb.caught, sa.missed + sb.missed));
    assert!(!a.real_salt.passed());
    assert!(a.real_salt.misses.len() <= diacraft_calc::MAX_MISSES_KEPT);
    let t = a.real_salt.by_class[&SaltClass::Number];
    assert_eq!(t.salted, sa.by_class[&SaltClass::Number].salted + sb.by_class[&SaltClass::Number].salted);
}
