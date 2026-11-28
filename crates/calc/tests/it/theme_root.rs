//! `THEMEVAL` with no theme selected: a call that names no property, which is what every cell holding the marker `Themed` stands for,
//! gives the root style's cell of the host's name; a call that names one gives MS-VSDX's no-theme table. ,
//! With the vectors.

use diacraft_calc::{Budget, EvalCtx, EvalOptions, Evaluator, commit, recalc_all};
use diacraft_doc::{CellAddr, Document, Lookup, Owner, RowKey, ShapeRef, SheetRef};
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder, StyleBuilder};
use diacraft_testkit::theme::ThemeXml;

use crate::{E, mismatch};

const FIVE: [&str; 5] = ["ThemeIndex", "ColorSchemeIndex", "EffectSchemeIndex", "ConnectorSchemeIndex", "FontSchemeIndex"];

/// A colour as the spec prints it.
fn c(hex: &str) -> E {
    let n = u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap();
    E::Col((n >> 16) as u8, (n >> 8) as u8, n as u8)
}

/// A shape whose five scheme cells hold `n`.
fn on_schemes(s: ShapeBuilder, n: &str) -> ShapeBuilder {
    FIVE.iter().fold(s, |s, name| s.cell(name, n))
}

/// A document of one shape (page 1) and the root style's `root` cells, holding `themes` composed theme parts.
fn doc_of(root: &[(&str, &str)], shape: ShapeBuilder, themes: usize) -> Document {
    let mut doc = DocBuilder::new().root(|r| root.iter().fold(r, |r, (n, v)| r.cell(n, v))).page(1, PageBuilder::new().shape(shape)).build();
    for _ in 0..themes {
        doc.themes.push(ThemeXml::default().part());
    }
    doc
}

fn eval_at(doc: &Document, addr: CellAddr) -> diacraft_calc::Value {
    let at = diacraft_calc::CellAddress::new(SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0)), addr);
    Evaluator::new(doc, &mut EvalCtx::default()).evaluate(&at)
}

fn expect(id: &str, got: diacraft_calc::Value, want: E) {
    assert_eq!(mismatch(&got, &want), None, "{id}: {got:?}");
}

/// The cell `name` of a shape with no theme (`n` in the five cells; "-" for none at all) holding `formula` and the marker.
fn marker(root: &[(&str, &str)], n: &str, themes: usize, name: &str, formula: &str) -> diacraft_calc::Value {
    let shape = if n == "-" { ShapeBuilder::new(1) } else { on_schemes(ShapeBuilder::new(1), n) }.formula(name, formula, "Themed");
    eval_at(&doc_of(root, shape, themes), CellAddr::direct(name))
}

const ROOT: [(&str, &str); 5] = [("FillForegnd", "1"), ("FillBkgnd", "0"), ("LineColor", "0"), ("ShdwForegnd", "0"), ("LineCap", "0")];

#[test]
fn a_bare_call_with_no_theme_over_a_root_that_holds_no_such_cell_gives_no_value() {
    // A root that is not whole (a new drawing's holds no `BeginArrowSize` or `EndArrowSize`) gives `#N/A`, which each reader takes as a cell it finds nowhere, so that a shape
    // that follows the `Theme` style on a page with no theme reads what a shape that follows no style reads: the medium line end, and not the 0, the smallest.
    for name in ["EndArrowSize", "BeginArrowSize", "Rounding", "ShdwPattern"] {
        expect(name, marker(&ROOT, "0", 0, name, "THEMEVAL()"), E::Err(diacraft_calc::CellError::Na));
        expect(&format!("{name}, no scheme cells"), marker(&ROOT, "-", 1, name, "THEMEVAL()"), E::Err(diacraft_calc::CellError::Na));
    }
    // A root that holds the cell gives it, as before; a call that names its property gives the root's cell or the default, and a `default` is the default.
    expect("EndArrowSize held", marker(&[("EndArrowSize", "2")], "0", 0, "EndArrowSize", "THEMEVAL()"), E::N(2.0));
    expect("ConnectorEndSize", marker(&ROOT, "0", 0, "EndArrowSize", r#"THEMEVAL("ConnectorEndSize")"#), E::N(0.0));
    expect("a default", marker(&ROOT, "0", 0, "EndArrowSize", r#"THEMEVAL("ConnectorEndSize",3)"#), E::N(3.0));
    // The colour cells of `ROOT` are held: the calls give them.
    expect("held colour", marker(&ROOT, "0", 0, "LineColor", "THEMEVAL()"), c("#000000"));
}

/// A document of one shape holding `rows` (`THEMEVAL()` with the saved value 0, whatever the cell's own kind) in the stop rows `rows` of `section`,
/// no scheme cell and no theme part, over a root style that holds `root` as it is built.
fn stops(section: &str, rows: &[u32], cells: &[&str], root: impl FnOnce(StyleBuilder) -> StyleBuilder) -> Document {
    let shape = rows
        .iter()
        .fold(ShapeBuilder::new(1), |s, ix| cells.iter().fold(s, |s, name| s.row_formula(section, None, RowKey::Ix(*ix), name, "THEMEVAL()", "0")));
    DocBuilder::new().root(root).page(1, PageBuilder::new().shape(shape)).build()
}

fn at_row(doc: &Document, section: &str, ix: u32, name: &str) -> diacraft_calc::Value {
    eval_at(doc, CellAddr::in_row(section, None, RowKey::Ix(ix), name))
}

/// A root style that holds row `IX` 0 of `section` only, as the root of every file does: white, 0 and 0.
fn first_row_only(section: &'static str) -> impl FnOnce(StyleBuilder) -> StyleBuilder {
    move |r| {
        r.row_cell(section, None, RowKey::Ix(0), "GradientStopColor", "1")
            .row_cell(section, None, RowKey::Ix(0), "GradientStopColorTrans", "0.25")
            .row_cell(section, None, RowKey::Ix(0), "GradientStopPosition", "0.5")
    }
}

const STOP_CELLS: [&str; 3] = ["GradientStopColor", "GradientStopColorTrans", "GradientStopPosition"];

#[test]
fn a_stop_row_past_the_first_with_no_theme_takes_its_default_where_the_root_holds_only_row_0() {
    // The corpus' own shape (R1 of the gate vector): the root style holds row `IX` 0 only, and a shape's rows 0 and 1 hold a bare call with the saved value 0.
    // Row 0 reads the root's; a row past it has no root value, and takes the cell's own default:
    // 0, 0, and black. The root's row 0 is not the row's default (0.25, 0.5 and the
    // index 1, which a colour cell holds as the root's own cells do, here).
    for section in ["FillGradient", "LineGradient"] {
        let doc = stops(section, &[0, 1, 2], &STOP_CELLS, first_row_only(section));
        expect(&format!("{section} row 0, trans"), at_row(&doc, section, 0, "GradientStopColorTrans"), E::Any(0.25));
        expect(&format!("{section} row 0, position"), at_row(&doc, section, 0, "GradientStopPosition"), E::Any(0.5));
        expect(&format!("{section} row 0, colour"), at_row(&doc, section, 0, "GradientStopColor"), E::Any(1.0));
        for ix in [1, 2] {
            expect(&format!("{section} row {ix}, trans"), at_row(&doc, section, ix, "GradientStopColorTrans"), E::Any(0.0));
            expect(&format!("{section} row {ix}, position"), at_row(&doc, section, ix, "GradientStopPosition"), E::Any(0.0));
            expect(&format!("{section} row {ix}, colour"), at_row(&doc, section, ix, "GradientStopColor"), c("#000000"));
        }
    }
}

#[test]
fn the_oracle_agrees_with_the_saved_zero_of_both_rows_of_the_gate_vector() {
    // The gate's vector as the oracle takes it: the root holds row `IX` 0 (white, 0, 0), a rectangle's rows 0 and 1 hold `THEMEVAL()` in `GradientStopColorTrans`
    // with the saved value 0, and the page selects no theme and the file holds no part. Both rows compute 0: agreed 2, differed 0 (row 1 was `#N/A`).
    for section in ["FillGradient", "LineGradient"] {
        let doc = stops(section, &[0, 1], &["GradientStopColorTrans"], |r| {
            r.row_cell(section, None, RowKey::Ix(0), "GradientStopColor", "1")
                .row_cell(section, None, RowKey::Ix(0), "GradientStopColorTrans", "0")
                .row_cell(section, None, RowKey::Ix(0), "GradientStopPosition", "0")
        });
        let r = recalc_all(&doc, &Budget::default());
        assert_eq!((r.agreed, r.differed.len()), (2, 0), "{section}: {r:?}");
    }
}

#[test]
fn a_stop_row_the_root_holds_reads_the_roots_row() {
    // The control of the gate vector: the root holds rows 0 and 1, and each row reads its own.
    for section in ["FillGradient", "LineGradient"] {
        let doc = stops(section, &[0, 1], &STOP_CELLS, |r| {
            first_row_only(section)(r)
                .row_cell(section, None, RowKey::Ix(1), "GradientStopColor", "2")
                .row_cell(section, None, RowKey::Ix(1), "GradientStopColorTrans", "0.75")
                .row_cell(section, None, RowKey::Ix(1), "GradientStopPosition", "1")
        });
        expect(&format!("{section} row 0"), at_row(&doc, section, 0, "GradientStopColorTrans"), E::Any(0.25));
        expect(&format!("{section} row 1, trans"), at_row(&doc, section, 1, "GradientStopColorTrans"), E::Any(0.75));
        expect(&format!("{section} row 1, position"), at_row(&doc, section, 1, "GradientStopPosition"), E::Any(1.0));
        expect(&format!("{section} row 1, colour"), at_row(&doc, section, 1, "GradientStopColor"), E::Any(2.0));
    }
}

#[test]
fn a_root_that_holds_no_stop_section_gives_no_value_for_row_0_and_the_default_past_it() {
    // The third variant of the gate vector. Every root holds row `IX` 0 ("the root holds 1, `IX` 0", in every file), so a root without it is not whole,
    // like one that lacks a cell directly under the sheet: row 0 gives no value (`#N/A`). A row past the first has no root value in any root
    // So with the section or without it the cell takes its own default.
    for section in ["FillGradient", "LineGradient"] {
        let doc = stops(section, &[0, 1, 2], &STOP_CELLS, |r| r.cell("FillForegnd", "1"));
        for name in STOP_CELLS {
            expect(&format!("{section} row 0, {name}"), at_row(&doc, section, 0, name), E::Err(diacraft_calc::CellError::Na));
        }
        for ix in [1, 2] {
            expect(&format!("{section} row {ix}, trans"), at_row(&doc, section, ix, "GradientStopColorTrans"), E::Any(0.0));
            expect(&format!("{section} row {ix}, position"), at_row(&doc, section, ix, "GradientStopPosition"), E::Any(0.0));
            expect(&format!("{section} row {ix}, colour"), at_row(&doc, section, ix, "GradientStopColor"), c("#000000"));
        }
    }
}

#[test]
fn a_colour_function_over_a_bare_call_in_a_stop_row_past_the_first_is_a_colour() {
    // The gate's three files with a `GradientStopColor` of `SHADE`, `HSL`, `HUE`, `SAT` or `LUM` over the call: a function of the default colour, not the `#N/A`
    // carried through. Each is compared with the same function over black written out, which is what the default is.
    let over = |call: &str| {
        let shape = ShapeBuilder::new(1).row_formula("FillGradient", None, RowKey::Ix(1), "GradientStopColor", call, "#000000");
        let doc = DocBuilder::new().root(first_row_only("FillGradient")).page(1, PageBuilder::new().shape(shape)).build();
        at_row(&doc, "FillGradient", 1, "GradientStopColor")
    };
    for (call, black) in [
        ("SHADE(THEMEVAL(),20)", "SHADE(RGB(0,0,0),20)"),
        ("TINT(THEMEVAL(),50)", "TINT(RGB(0,0,0),50)"),
        ("HSL(HUE(THEMEVAL()),SAT(THEMEVAL()),LUM(THEMEVAL())+40)", "HSL(HUE(RGB(0,0,0)),SAT(RGB(0,0,0)),LUM(RGB(0,0,0))+40)"),
    ] {
        let (got, want) = (over(call), over(black));
        assert!(matches!(got, diacraft_calc::Value::Color(_)), "{call}: {got:?}");
        assert_eq!(got, want, "{call}");
    }
}

#[test]
fn a_bare_call_with_no_theme_gives_the_root_styles_cell_of_the_hosts_name() {
    // A root `FillForegnd` of 1 gives white, a root `LineColor` of 0 black. The table's `#96afcf` and `#1f477d` fail both.
    expect("Fill", marker(&ROOT, "0", 0, "FillForegnd", "THEMEVAL()"), c("#ffffff"));
    expect("Line", marker(&ROOT, "0", 0, "LineColor", "THEMEVAL()"), c("#000000"));
    // `FillBkgnd` with a root of 0 is black; the table's `#bfcee1` fails.
    expect("Back", marker(&ROOT, "0", 0, "FillBkgnd", "THEMEVAL()"), c("#000000"));
    // Our reasoning only, as no sample tests it. The table's `#464646` is for a call naming `ShadowColor`.
    expect("SHDWFOREGND", marker(&ROOT, "0", 0, "ShdwForegnd", "THEMEVAL()"), c("#000000"));
    // No theme part at all.
    expect("V7 line", marker(&ROOT, "0", 0, "LineColor", "THEMEVAL()"), c("#000000"));
    expect("V7 fill", marker(&ROOT, "0", 0, "FillForegnd", "THEMEVAL()"), c("#ffffff"));
    // The five cells absent is the same as 0.
    expect("no cells", marker(&ROOT, "-", 0, "FillForegnd", "THEMEVAL()"), c("#ffffff"));
}

#[test]
fn the_root_gives_its_own_colour_and_not_a_fixed_white_or_black() {
    // A root `FillForegnd` of `#3c5a78` gives that colour, which a fixed white fails.
    expect("hex", marker(&[("FillForegnd", "#3c5a78")], "0", 0, "FillForegnd", "THEMEVAL()"), c("#3c5a78"));
    // An index reads the palette: 2 is red.
    expect("index", marker(&[("LineColor", "2")], "0", 0, "LineColor", "THEMEVAL()"), c("#ff0000"));
}

#[test]
fn a_cell_the_table_does_not_cover_reads_its_root_cell_as_before() {
    // `LineCap` holding the marker with a root of 0 gives 0.
    expect("SEL", marker(&ROOT, "0", 0, "LineCap", "THEMEVAL()"), E::N(0.0));
}

#[test]
fn the_text_colour_of_a_character_row_reads_the_roots_row() {
    let shape = on_schemes(ShapeBuilder::new(1), "0").row_formula("Character", None, RowKey::Ix(0), "Color", "THEMEVAL()", "Themed");
    // The root style holds the character row (a style can): its `Color` is what the marker reads.
    let doc =
        DocBuilder::new().root(|r| r.row_cell("Character", None, RowKey::Ix(0), "Color", "#102030")).page(1, PageBuilder::new().shape(shape)).build();
    expect("text", eval_at(&doc, CellAddr::in_row("Character", None, RowKey::Ix(0), "Color")), c("#102030"));
}

#[test]
fn a_call_naming_a_property_keeps_the_table_in_the_cell_of_that_property() {
    // Naming the property is what asks for the column, in whatever cell the call sits.
    for (cell, formula, want) in [
        ("FillForegnd", "THEMEVAL(\"FillColor\")", "#96afcf"),
        ("FillBkgnd", "THEMEVAL(\"FillColor2\")", "#bfcee1"),
        ("LineColor", "THEMEVAL(\"LineColor\")", "#1f477d"),
        ("LineColor", "THEMEVAL(\"ConnectorColor\")", "#1f477d"),
        ("ShdwForegnd", "THEMEVAL(\"ShadowColor\")", "#464646"),
        // A number of MS-VSDX's numbering names a property too: 2 is `AccentColor`.
        ("FillForegnd", "THEMEVAL(2)", "#c05046"),
    ] {
        expect(formula, marker(&ROOT, "0", 0, cell, formula), c(want));
    }
}

#[test]
fn a_default_replaces_the_root_as_it_replaces_the_table() {
    expect("named", marker(&ROOT, "0", 0, "FillForegnd", "THEMEVAL(\"FillColor\",RGB(1,2,3))"), c("#010203"));
}

#[test]
fn a_page_that_selects_no_theme_over_a_theme_part_reads_the_root_for_a_call_naming_none() {
    // 65534 and a page holding no scheme cell is no theme though a part exists. `THEMEVAL("FillColor")` gives the table, and a
    // `FillForegnd` holding `THEMEVAL()` the root's: the first part's colour fails both, and the table fails the second.
    expect("named", marker(&ROOT, "65534", 1, "FillForegnd", "THEMEVAL(\"FillColor\")"), c("#96afcf"));
    expect("bare", marker(&ROOT, "65534", 1, "FillForegnd", "THEMEVAL()"), c("#ffffff"));
}

#[test]
fn a_theme_that_is_selected_still_answers_a_bare_call_from_the_part() {
    // The root is for the case with no theme: under an explicit theme the part answers, with accent1 `#204060`.
    let shape = on_schemes(ShapeBuilder::new(1), "40").cell("QuickStyleFillColor", "2").cell("QuickStyleFillMatrix", "1").formula(
        "FillForegnd",
        "THEMEVAL()",
        "Themed",
    );
    expect("part", eval_at(&doc_of(&ROOT, shape, 1), CellAddr::direct("FillForegnd")), c("#204060"));
}

#[test]
fn a_marker_cell_that_now_has_an_answer_is_written_back_unchanged() {
    // `LineColor` holding `THEMEVAL()` and the marker, with no theme part, is black, and a commit leaves the cell as the file
    // had it: the marker stays its `V`, and the formula its `F`. (`Themed` is not a value to replace with the colour that stands for it.)
    let shape = on_schemes(ShapeBuilder::new(1), "0").formula("LineColor", "THEMEVAL()", "Themed").formula("FillForegnd", "THEMEVAL()", "Themed");
    let mut doc = doc_of(&ROOT, shape, 0);
    let report = commit(&mut doc, &Budget::default(), EvalOptions::default());
    assert_eq!(report.written, 0, "{report:?}");
    let sheet = &doc.shape(&ShapeRef::top(Owner::Page(1), 0)).expect("the shape").sheet;
    for name in ["LineColor", "FillForegnd"] {
        let Lookup::Found(cell) = sheet.lookup(&CellAddr::direct(name)) else { panic!("{name} is gone") };
        assert_eq!(cell.value.as_ref().map(|v| v.text().to_owned()).as_deref(), Some("Themed"), "{name}");
        assert_eq!(cell.formula.text(), Some("THEMEVAL()"), "{name}");
    }
}
