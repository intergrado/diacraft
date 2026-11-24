//! `THEMEVAL` under an explicit theme, `THEMECBV`, `THEMEPROP` and the Quick Style cells they read (with
//!): the vectors, and the cells a real file holds them in.
//!
//! The theme part is the composed one of `diacraft_testkit::theme`: schemes all 40, `dk1` `#111111`, `lt1` `#fefefe`, `accent1` `#204060`,
//! `accent3` `#214365`, background `#ffffff`, and a value at each list position that no neighbour holds.

use std::sync::Arc;

use diacraft_calc::CellError::{Na, Value as BadValue};
use diacraft_calc::{Budget, CellAddress, EvalCtx, EvalOptions, Evaluator};
use diacraft_doc::{CellAddr, Document, Owner, RowKey, ShapeRef, SheetRef, Theme};
use diacraft_testkit::build::{DocBuilder, MasterBuilder, PageBuilder, ShapeBuilder};
use diacraft_testkit::theme::{ThemeXml, colour_variant, font_props, gradient, ln, scheme_clr, solid, srgb_clr};

use crate::{E, mismatch, probe, probe_addr};

const FIVE: [&str; 5] = ["ThemeIndex", "ColorSchemeIndex", "EffectSchemeIndex", "ConnectorSchemeIndex", "FontSchemeIndex"];

/// The shape selects the composed theme: every scheme cell 40.
fn on_theme(mut sb: ShapeBuilder) -> ShapeBuilder {
    for name in FIVE {
        sb = sb.cell(name, "40");
    }
    sb
}

fn eval_doc(doc: &Document, at: &CellAddress) -> diacraft_calc::Value {
    let mut ctx = EvalCtx::new(EvalOptions::default(), Budget::default());
    Evaluator::new(doc, &mut ctx).evaluate(at)
}

/// The probe formula worked out on the shape `setup` makes, in a document that holds `x` as its one theme part.
fn eval_in(x: &ThemeXml, setup: impl FnOnce(ShapeBuilder) -> ShapeBuilder, formula: &str) -> diacraft_calc::Value {
    let (mut doc, at) = probe(setup, formula);
    doc.themes.push(x.part());
    eval_doc(&doc, &at)
}

fn expect(id: &str, got: diacraft_calc::Value, want: E) {
    if let Some(why) = mismatch(&got, &want) {
        panic!("{id}: {why}");
    }
}

fn col(hex: &str) -> E {
    let byte = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).unwrap();
    E::Col(byte(0), byte(2), byte(4))
}

/// The cell `name` of the one shape of page 1, as `setup` makes it, worked out.
fn eval_cell(x: &ThemeXml, setup: impl FnOnce(ShapeBuilder) -> ShapeBuilder, name: &str) -> diacraft_calc::Value {
    let doc = DocBuilder::new().page(1, PageBuilder::new().shape(setup(ShapeBuilder::new(1)))).build();
    let mut doc = doc;
    doc.themes.push(x.part());
    let at = CellAddress::new(SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0)), CellAddr::direct(name));
    eval_doc(&doc, &at)
}

#[test]
fn themeval_a_scheme_colour_by_name_from_the_part_the_scheme_number_selects() {
    // The page sheet holds 40 in all five cells, the shape reads the page (65534), and a part numbered 41 listed first must not answer.
    let other = ThemeXml { scheme: 41, colours: ["000000"; 12], ..ThemeXml::default() };
    let shape = FIVE.iter().fold(ShapeBuilder::new(1), |s, n| s.cell(n, "65534")).row_formula(
        "User",
        None,
        RowKey::name("Probe"),
        "Value",
        "THEMEVAL(\"AccentColor3\")",
        "0",
    );
    let page = FIVE.iter().fold(PageBuilder::new(), |p, n| p.cell(n, "40")).shape(shape);
    let mut doc = DocBuilder::new().page(1, page).build();
    doc.themes.push(other.part());
    doc.themes.push(ThemeXml::default().part());
    let at = CellAddress::new(SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0)), probe_addr());
    expect("THEMEVAL", eval_doc(&doc, &at), col("214365"));
    for (formula, want) in [
        ("THEMEVAL(\"accentcolor3\")", "214365"),
        ("THEMEVAL(\"Dark\")", "111111"),
        ("THEMEVAL(\"Light\")", "fefefe"),
        ("THEMEVAL(\"AccentColor\")", "204060"),
        ("THEMEVAL(\"BackgroundColor\")", "ffffff"),
    ] {
        let at = CellAddress::new(SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0)), probe_addr());
        let shape =
            FIVE.iter().fold(ShapeBuilder::new(1), |s, n| s.cell(n, "65534")).row_formula("User", None, RowKey::name("Probe"), "Value", formula, "0");
        let page = FIVE.iter().fold(PageBuilder::new(), |p, n| p.cell(n, "40")).shape(shape);
        let mut doc = DocBuilder::new().page(1, page).build();
        doc.themes.push(other.part());
        doc.themes.push(ThemeXml::default().part());
        expect(formula, eval_doc(&doc, &at), col(want));
    }
}

#[test]
fn themeval_the_theme_answers_and_the_default_does_not() {
    let x = ThemeXml::default();
    expect("THEMEVAL", eval_in(&x, on_theme, "THEMEVAL(\"AccentColor3\",RGB(1,2,3))"), col("214365"));
    // A default stands where a scheme cell is 0.
    expect("THEMEVAL", eval_in(&x, |s| on_theme(s).cell("ColorSchemeIndex", "0"), "THEMEVAL(\"AccentColor3\",RGB(1,2,3))"), col("010203"));
}

#[test]
fn themeval_65534_with_no_page_cell_reads_no_theme_and_a_variant_colour_reads_the_first_list() {
    let x = ThemeXml::default();
    expect(
        "THEMEVAL",
        eval_in(&x, |s| s.cell("ColorSchemeIndex", "65534").cell("ThemeIndex", "65534").cell("FontSchemeIndex", "65534"), "THEMEVAL(\"FillColor\")"),
        col("96afcf"),
    );
    // `QuickStyleFillColor` 101 with scheme cells and no `VariationColorIndex`: the second colour of the first list.
    expect("THEMEVAL", eval_in(&x, |s| on_theme(s).cell("QuickStyleFillColor", "101"), "THEMEVAL(QuickStyleFillColor)"), col("223344"));
}

#[test]
fn themeval_a_monotone_variant_sends_100_to_106_to_its_first_colour_and_leaves_200_to_206() {
    let x = ThemeXml::default();
    let monotone = |s: ShapeBuilder| on_theme(s).cell("VariationColorIndex", "1");
    expect("THEMEVAL", eval_in(&x, monotone, "THEMEVAL(103)"), col("a1a1a1"));
    expect("THEMEVAL", eval_in(&x, monotone, "THEMEVAL(203)"), col("d4d4d4"));
    expect("THEMEVAL", eval_in(&x, monotone, "THEMEVAL(\"VariantColor4\")"), col("d4d4d4"));
    // Variant 2 is not monotone: 103 is its fourth.
    expect("THEMEVAL", eval_in(&x, |s| on_theme(s).cell("VariationColorIndex", "2"), "THEMEVAL(103)"), col("0a0b0c"));
}

#[test]
fn themeval_a_fill_entry_by_the_matrix_with_its_modifiers() {
    let x = ThemeXml::default();
    // Entry 2 is a solid `phClr` with `shade` 75000: accent1 `#204060` darker by linear light, `#1b3754`, where plain scaling gives `#183048`.
    let solid_entry = |s: ShapeBuilder| on_theme(s).cell("QuickStyleFillColor", "2").cell("QuickStyleFillMatrix", "2");
    expect("THEMEVAL", eval_in(&x, solid_entry, "THEMEVAL(\"FillColor\")"), col("1b3754"));
    // A gradient entry gives the selected colour plain.
    let gradient_entry = |s: ShapeBuilder| on_theme(s).cell("QuickStyleFillColor", "2").cell("QuickStyleFillMatrix", "3");
    expect("THEMEVAL", eval_in(&x, gradient_entry, "THEMEVAL(\"FillColor\")"), col("204060"));
    // `FillColor2`: a solid with `phClr` gives `#4d4d4d`, a gradient the selected colour, a solid naming `lt1` that colour.
    expect("THEMEVAL", eval_in(&x, solid_entry, "THEMEVAL(\"FillColor2\")"), col("4d4d4d"));
    expect("THEMEVAL", eval_in(&x, gradient_entry, "THEMEVAL(\"FillColor2\")"), col("204060"));
    expect("THEMEVAL", eval_in(&x, |s| on_theme(s).cell("QuickStyleFillMatrix", "6"), "THEMEVAL(\"FillColor2\")"), col("fefefe"));
    // Matrix 101 reads the second `vt:varStyle` of the variant `VariationStyleIndex` (here 0) picks, whose `fillIdx` is 2.
    expect(
        "SEL",
        eval_in(&x, |s| on_theme(s).cell("QuickStyleFillColor", "2").cell("QuickStyleFillMatrix", "101"), "THEMEVAL(\"FillColor\")"),
        col("1b3754"),
    );
    // A number past the list takes the table.
    expect("SEL", eval_in(&x, |s| on_theme(s).cell("QuickStyleFillMatrix", "255"), "THEMEVAL(\"FillColor\")"), col("96afcf"));
}

#[test]
fn themeval_a_line_entry_and_a_font_entry() {
    let x = ThemeXml::default();
    let line = |s: ShapeBuilder| on_theme(s).cell("QuickStyleLineColor", "2").cell("QuickStyleLineMatrix", "2");
    expect("THEMEVAL", eval_in(&x, line, "THEMEVAL(\"LineColor\")"), col("1d3c5a"));
    expect("THEMEVAL", eval_in(&x, line, "THEMEVAL(\"ConnectorColor\")"), col("1d3c5a"));
    // A font entry whose colour is `lt1`: `#fefefe`, whatever `QuickStyleFontColor` holds.
    for font_colour in ["0", "2", "105"] {
        let text = |s: ShapeBuilder| on_theme(s).cell("QuickStyleFontColor", font_colour).cell("QuickStyleFontMatrix", "4");
        expect("THEMEVAL", eval_in(&x, text, "THEMEVAL(\"TextColor\")"), col("fefefe"));
    }
    // Entry 1's `phClr` is the colour `QuickStyleFontColor` picks.
    let ph = |s: ShapeBuilder| on_theme(s).cell("QuickStyleFontColor", "2").cell("QuickStyleFontMatrix", "1");
    expect("THEME", eval_in(&x, ph, "THEMEVAL(\"TextColor\")"), col("204060"));
}

#[test]
fn themeval_the_contrast_step() {
    let x = ThemeXml::default();
    // Text `#fefefe` on a white background, fill `#1b3754`, line `#1d3c5a`: bit 2 takes the candidate that differs most from the text, the fill.
    let step = |bits: &'static str| {
        move |s: ShapeBuilder| {
            on_theme(s)
                .cell("QuickStyleVariation", bits)
                .cell("QuickStyleFontMatrix", "4")
                .cell("QuickStyleFillColor", "2")
                .cell("QuickStyleFillMatrix", "2")
                .cell("QuickStyleLineColor", "2")
                .cell("QuickStyleLineMatrix", "2")
        }
    };
    expect("THEMEVAL", eval_in(&x, step("2"), "THEMEVAL(\"TextColor\")"), col("1b3754"));
    expect("THEMEVAL", eval_in(&x, step("0"), "THEMEVAL(\"TextColor\")"), col("fefefe"));
    // Bit 1 does nothing, and bit 4 is the line's, not the text's.
    expect("THEMEVAL", eval_in(&x, step("1"), "THEMEVAL(\"TextColor\")"), col("fefefe"));
    expect("THEMEVAL", eval_in(&x, step("4"), "THEMEVAL(\"TextColor\")"), col("fefefe"));
    // A wide gap keeps the colour: `dk1` text on white.
    let wide = |s: ShapeBuilder| on_theme(s).cell("QuickStyleVariation", "2").cell("QuickStyleFontMatrix", "1").cell("QuickStyleFontColor", "0");
    expect("THEMEVAL", eval_in(&x, wide, "THEMEVAL(\"TextColor\")"), col("111111"));
    // On a background `#404040` a text of `#505050` is too near, and the background is dark: white.
    let dark = ThemeXml { background: "404040", fonts: vec![font_props(0, &srgb_clr("505050", &[]))], ..ThemeXml::default() };
    let text = |bits: &'static str| move |s: ShapeBuilder| on_theme(s).cell("QuickStyleVariation", bits).cell("QuickStyleFontMatrix", "1");
    expect("THEMEVAL", eval_in(&dark, text("2"), "THEMEVAL(\"TextColor\")"), col("ffffff"));
    expect("THEMEVAL", eval_in(&dark, text("4"), "THEMEVAL(\"TextColor\")"), col("505050"));
    // The line colour has its own bit: 4.
    let line = ThemeXml { background: "404040", lines: vec![ln(9525, "flat", &solid(&srgb_clr("505050", &[])), "solid")], ..ThemeXml::default() };
    let on_line = |bits: &'static str| move |s: ShapeBuilder| on_theme(s).cell("QuickStyleVariation", bits).cell("QuickStyleLineMatrix", "1");
    expect("SEL", eval_in(&line, on_line("4"), "THEMEVAL(\"LineColor\")"), col("ffffff"));
    expect("SEL", eval_in(&line, on_line("2"), "THEMEVAL(\"LineColor\")"), col("505050"));
}

#[test]
fn themeval_the_modifiers_by_the_spec_s_worked_examples() {
    let green = |mods: &[(&str, i32)]| ThemeXml { fills: vec![solid(&srgb_clr("00ff00", mods))], ..ThemeXml::default() };
    let fill = |s: ShapeBuilder| on_theme(s).cell("QuickStyleFillMatrix", "1");
    expect("THEMEVAL", eval_in(&green(&[("satMod", 20000)]), fill, "THEMEVAL(\"FillColor\")"), col("669966"));
    expect("THEMEVAL", eval_in(&green(&[("lumMod", 50000)]), fill, "THEMEVAL(\"FillColor\")"), col("008000"));
    expect("THEMEVAL", eval_in(&green(&[("tint", 50000)]), fill, "THEMEVAL(\"FillColor\")"), col("bcffbc"));
    expect("THEMEVAL", eval_in(&green(&[("shade", 50000)]), fill, "THEMEVAL(\"FillColor\")"), col("00bc00"));
    // A chain applies in the order it stands, and `alpha` leaves the colour.
    expect("THEMEVAL", eval_in(&green(&[("alpha", 50000), ("shade", 50000)]), fill, "THEMEVAL(\"FillColor\")"), col("00bc00"));
}

#[test]
fn themeval_a_number_as_the_property() {
    let x = ThemeXml::default();
    // MS-VSDX's numbering: 2 is the first accent; the page's numbering would give `lt1`'s `#fefefe`.
    expect("THEMEVAL", eval_in(&x, on_theme, "THEMEVAL(2)"), col("204060"));
    expect("THEMEVAL", eval_in(&x, on_theme, "THEMEVAL(0)"), col("111111"));
    expect("THEMEVAL", eval_in(&x, on_theme, "THEMEVAL(8)"), col("ffffff"));
    // A Quick Style cell as the argument reads like the colour cell does: 101 is the colour of a plain solid entry.
    let plain = |s: ShapeBuilder| on_theme(s).cell("QuickStyleFillColor", "101").cell("QuickStyleFillMatrix", "1");
    let by_number = eval_in(&x, plain, "THEMEVAL(QuickStyleFillColor)");
    let by_name = eval_in(&x, plain, "THEMEVAL(\"FillColor\")");
    expect("THEMEVAL", by_number, col("223344"));
    expect("THEMEVAL", by_name, col("223344"));
}

#[test]
fn themeval_a_name_not_in_the_table() {
    expect("THEMEVAL", eval_in(&ThemeXml::default(), on_theme, "THEMEVAL(\"NoSuchProperty\")"), E::Err(BadValue));
}

#[test]
fn themecbv_applies_the_modifiers_of_the_stop_to_the_colour() {
    let x = ThemeXml::default();
    // The entry selected is a gradient whose second stop is `phClr` with `shade` 50000.
    let entry = |s: ShapeBuilder| on_theme(s).cell("QuickStyleFillMatrix", "3");
    expect("THEMECBV", eval_in(&x, entry, "THEMECBV(RGB(0,255,0),2)"), col("00bc00"));
    expect("THEMECBV", eval_in(&x, entry, "THEMECBV(RGB(0,255,0),1)"), col("bcffbc"));
    // Entry 4 has three stops, and its second holds `satMod` (every modifier a stop holds is applied).
    let three = |s: ShapeBuilder| on_theme(s).cell("QuickStyleFillMatrix", "4");
    expect("THEMECBV", eval_in(&x, three, "THEMECBV(RGB(0,255,0),2)"), col("00ff00"));
    // A solid entry, or no theme, leaves the colour.
    let solid_entry = |s: ShapeBuilder| on_theme(s).cell("QuickStyleFillMatrix", "1");
    expect("THEMECBV", eval_in(&x, solid_entry, "THEMECBV(RGB(0,255,0),2)"), col("00ff00"));
    expect("THEMECBV", eval_in(&x, |s| entry(s).cell("ColorSchemeIndex", "0"), "THEMECBV(RGB(0,255,0),2)"), col("00ff00"));
    // A stop of 11 or 0 is `#VALUE!`; 10 with three stops leaves the colour.
    expect("THEMECBV", eval_in(&x, three, "THEMECBV(RGB(0,255,0),11)"), E::Err(BadValue));
    expect("THEMECBV", eval_in(&x, three, "THEMECBV(RGB(0,255,0),0)"), E::Err(BadValue));
    expect("THEMECBV", eval_in(&x, three, "THEMECBV(RGB(0,255,0),10)"), col("00ff00"));
    // A gradient whose stops carry `satMod`, `lumMod` and `tint`, in turn.
    let stops = [
        (0, scheme_clr("phClr", &[("satMod", 20000)])),
        (50_000, scheme_clr("phClr", &[("lumMod", 50000)])),
        (100_000, scheme_clr("phClr", &[("tint", 50000)])),
    ];
    let mods = ThemeXml { fills: vec![gradient(&stops, 0)], ..ThemeXml::default() };
    let first = |s: ShapeBuilder| on_theme(s).cell("QuickStyleFillMatrix", "1");
    expect("THEMECBV", eval_in(&mods, first, "THEMECBV(RGB(0,255,0),1)"), col("669966"));
    expect("THEMECBV", eval_in(&mods, first, "THEMECBV(RGB(0,255,0),2)"), col("008000"));
    expect("THEMECBV", eval_in(&mods, first, "THEMECBV(RGB(0,255,0),3)"), col("bcffbc"));
}

/// A sheet whose `ColorSchemeIndex` reads 0 and a file with no theme part: `Embellishment` 2 and `Multiformat` 0.
/// Naive: 0 for both, as MS-VSDX says, which fails the 503 `Embellishment` cells that save a number. The same with the five cells absent, with the
/// cell 65534 over a page that holds none, and with a theme part in the file.
#[test]
fn themeprop_embellishment_is_2_and_multiformat_0_where_the_sheet_reads_no_theme() {
    let eval_plain = |setup: &dyn Fn(ShapeBuilder) -> ShapeBuilder, formula: &str| {
        let (doc, at) = probe(setup, formula);
        eval_doc(&doc, &at)
    };
    let zero = |s: ShapeBuilder| s.cell("ColorSchemeIndex", "0");
    expect("THEMEPROP", eval_plain(&zero, "THEMEPROP(\"Embellishment\")"), E::N(2.0));
    expect("THEMEPROP", eval_plain(&zero, "THEMEPROP(\"Multiformat\")"), E::N(0.0));
    expect("THEMEPROP", eval_plain(&zero, "THEMEPROP(\"embellishment\")"), E::N(2.0));
    expect("THEMEPROP", eval_plain(&zero, "THEMEPROP(\"Other\")"), E::Err(BadValue));
    // The five cells absent, and 65534 over a page that holds none, read no theme either.
    expect("THEMEPROP", eval_plain(&|s| s, "THEMEPROP(\"Embellishment\")"), E::N(2.0));
    expect("THEMEPROP", eval_plain(&|s| s.cell("ColorSchemeIndex", "65534"), "THEMEPROP(\"Embellishment\")"), E::N(2.0));
    expect("THEMEPROP", eval_plain(&|s| s.cell("ColorSchemeIndex", "65534"), "THEMEPROP(\"Multiformat\")"), E::N(0.0));
    // With a theme part in the file the sheet that reads no theme still gets 2 and 0: the sheet decides, and not the part.
    let x = ThemeXml::default();
    expect("THEMEPROP", eval_in(&x, zero, "THEMEPROP(\"Embellishment\")"), E::N(2.0));
    expect("THEMEPROP", eval_in(&x, zero, "THEMEPROP(\"Multiformat\")"), E::N(0.0));
}

#[test]
fn themeprop_embellishment_and_multiformat() {
    let x = ThemeXml::default();
    // `VariationStyleIndex` 2 and the third style variant's `embellishment` 3.
    expect("THEMEPROP", eval_in(&x, |s| on_theme(s).cell("VariationStyleIndex", "2"), "THEMEPROP(\"Embellishment\")"), E::N(3.0));
    expect(
        "THEMEPROP",
        eval_in(&x, |s| on_theme(s).cell("VariationStyleIndex", "2").cell("EmbellishmentIndex", "5"), "THEMEPROP(\"Embellishment\")"),
        E::N(5.0),
    );
    // Any one of the five scheme cells at 0 reads no theme: `Embellishment` is 2 (the samples against MS-VSDX's 0)
    // and `Multiformat` 0.
    for cell in FIVE {
        let none = |s: ShapeBuilder| on_theme(s).cell("VariationStyleIndex", "2").cell(cell, "0");
        expect("THEMEPROP", eval_in(&x, none, "THEMEPROP(\"Embellishment\")"), E::N(2.0));
        expect("THEMEPROP", eval_in(&x, none, "THEMEPROP(\"Multiformat\")"), E::N(0.0));
    }
    // The page's variant is the first when the shape says 65534 and the page says nothing: `embellishment` 2.
    expect("SEL", eval_in(&x, |s| on_theme(s).cell("VariationStyleIndex", "65534"), "THEMEPROP(\"Embellishment\")"), E::N(2.0));
    // No `monotone` is multiformat 1; `monotone` true is 0 (our reasoning).
    expect("THEMEPROP", eval_in(&x, on_theme, "THEMEPROP(\"Multiformat\")"), E::N(1.0));
    expect("THEMEPROP", eval_in(&x, |s| on_theme(s).cell("VariationColorIndex", "1"), "THEMEPROP(\"Multiformat\")"), E::N(0.0));
    expect("THEMEPROP", eval_in(&x, |s| on_theme(s).cell("VariationColorIndex", "3"), "THEMEPROP(\"Multiformat\")"), E::N(1.0));
    // Any other name is `#VALUE!`, and the case does not matter.
    expect("THEMEPROP", eval_in(&x, on_theme, "THEMEPROP(\"Other\")"), E::Err(BadValue));
    expect("THEMEPROP", eval_in(&x, |s| on_theme(s).cell("VariationStyleIndex", "2"), "THEMEPROP(\"embellishment\")"), E::N(3.0));
    // No variant list: the theme cannot say, so the saved value stands.
    let bare = ThemeXml { colour_variants: None, style_variants: None, ..ThemeXml::default() };
    expect("THEMEPROP", eval_in(&bare, on_theme, "THEMEPROP(\"Multiformat\")"), E::Unsupported);
}

#[test]
fn a_file_with_no_theme_part_cannot_answer_and_a_number_that_matches_no_part_takes_the_first() {
    // No part at all: the marker's answer is the renderer's fallback, so the call is unsupported.
    let (doc, at) = probe(on_theme, "THEMEVAL(\"AccentColor3\")");
    expect("V7", eval_doc(&doc, &at), E::Unsupported);
    // A number no part holds takes the first part.
    expect("E4", eval_in(&ThemeXml { scheme: 41, ..ThemeXml::default() }, on_theme, "THEMEVAL(\"AccentColor3\")"), col("214365"));
    // A custom colour scheme (65535) is a master's, which is not followed here.
    expect(
        "THEMEVAL",
        eval_in(&ThemeXml::default(), |s| on_theme(s).cell("ColorSchemeIndex", "65535"), "THEMEVAL(\"AccentColor3\")"),
        E::Unsupported,
    );
}

#[test]
fn the_cells_of_a_real_shape_read_the_theme_by_their_own_names() {
    let x = ThemeXml::default();
    let themed = |names: &[&str]| {
        let names: Vec<String> = names.iter().map(|n| (*n).to_owned()).collect();
        move |s: ShapeBuilder| names.iter().fold(on_theme(s), |s, n| s.formula(n, "THEMEVAL()", "Themed"))
    };
    let base = |s: ShapeBuilder| {
        s.cell("QuickStyleFillColor", "2")
            .cell("QuickStyleFillMatrix", "2")
            .cell("QuickStyleLineColor", "2")
            .cell("QuickStyleLineMatrix", "2")
            .cell("QuickStyleEffectsMatrix", "2")
            .cell("QuickStyleShadowColor", "2")
    };
    let setup = |names: &[&str]| {
        let f = themed(names);
        move |s: ShapeBuilder| f(base(s))
    };
    let all = [
        "FillForegnd",
        "FillBkgnd",
        "FillPattern",
        "FillForegndTrans",
        "FillGradientEnabled",
        "LineColor",
        "LinePattern",
        "LineWeight",
        "LineCap",
        "Rounding",
        "BeginArrow",
        "EndArrow",
        "BeginArrowSize",
        "EndArrowSize",
        "LineColorTrans",
        "ShdwForegnd",
        "ShdwPattern",
        "ShapeShdwOffsetX",
        "ShapeShdwOffsetY",
        "ShapeShdwBlur",
        "ShapeShdwType",
    ];
    expect("FillForegnd", eval_cell(&x, setup(&all), "FillForegnd"), col("1b3754"));
    expect("FillBkgnd", eval_cell(&x, setup(&all), "FillBkgnd"), col("4d4d4d"));
    // Entry 2 of `vt:fillStyles` is `pattern` 1 (position n); matrix 3 is a gradient, which `vt:fillProps` gives 25.
    expect("FillPattern", eval_cell(&x, setup(&all), "FillPattern"), E::Any(1.0));
    expect("FillPattern", eval_cell(&x, |s| setup(&all)(s).cell("QuickStyleFillMatrix", "3"), "FillPattern"), E::Any(25.0));
    expect("FillGradientEnabled", eval_cell(&x, setup(&all), "FillGradientEnabled"), E::Any(0.0));
    expect("FillGradientEnabled", eval_cell(&x, |s| setup(&all)(s).cell("QuickStyleFillMatrix", "3"), "FillGradientEnabled"), E::Any(1.0));
    expect("LineColor", eval_cell(&x, setup(&all), "LineColor"), col("1d3c5a"));
    // The line list entry 2: 12700 EMU is 1/72 inch; `vt:lineStyle` at position 3 gives the pattern 9, rounding 114300 EMU, and the ends.
    let weight = eval_cell(&x, setup(&all), "LineWeight");
    expect("LineWeight", weight, E::Any(12700.0 / 914_400.0));
    expect("LinePattern", eval_cell(&x, setup(&all), "LinePattern"), E::Any(9.0));
    expect("LineCap", eval_cell(&x, setup(&all), "LineCap"), E::Any(0.0));
    expect("Rounding", eval_cell(&x, setup(&all), "Rounding"), E::Any(0.125));
    expect("BeginArrow", eval_cell(&x, setup(&all), "BeginArrow"), E::Any(1.0));
    expect("EndArrow", eval_cell(&x, setup(&all), "EndArrow"), E::Any(2.0));
    expect("BeginArrowSize", eval_cell(&x, setup(&all), "BeginArrowSize"), E::Any(3.0));
    expect("EndArrowSize", eval_cell(&x, setup(&all), "EndArrowSize"), E::Any(4.0));
    expect("LineColorTrans", eval_cell(&x, setup(&all), "LineColorTrans"), E::Any(0.0));
    // The effect entry 2: an outer shadow of 12700 EMU at 90 degrees, 38 percent black.
    expect("ShdwPattern", eval_cell(&x, setup(&all), "ShdwPattern"), E::Any(1.0));
    expect("ShdwForegnd", eval_cell(&x, setup(&all), "ShdwForegnd"), col("000000"));
    expect("ShapeShdwOffsetX", eval_cell(&x, setup(&all), "ShapeShdwOffsetX"), E::Any(0.0));
    expect("ShapeShdwOffsetY", eval_cell(&x, setup(&all), "ShapeShdwOffsetY"), E::Any(-12700.0 / 914_400.0));
    // The first effect entry has no shadow: `ShdwPattern` 0 and `#000000`.
    let none = |s: ShapeBuilder| setup(&all)(s).cell("QuickStyleEffectsMatrix", "1");
    expect("ShdwPattern", eval_cell(&x, none, "ShdwPattern"), E::Any(0.0));
    expect("ShdwForegnd", eval_cell(&x, none, "ShdwForegnd"), col("000000"));
}

#[test]
fn a_one_dimensional_shape_reads_the_connector_scheme_unless_the_type_says_otherwise() {
    let x = ThemeXml::default();
    let line = |s: ShapeBuilder| on_theme(s).cell("BeginX", "0").cell("QuickStyleLineMatrix", "2").formula("LineWeight", "THEMEVAL()", "Themed");
    // Type 0 and a 1-D shape: the connector list, entry 2 of `vt:fmtConnectorScheme`, 31750 EMU.
    expect("SEL", eval_cell(&x, line, "LineWeight"), E::Any(31750.0 / 914_400.0));
    // Type 2 on the same shape: the effect scheme, entry 2, 12700 EMU.
    expect("SEL", eval_cell(&x, |s| line(s).cell("QuickStyleType", "2"), "LineWeight"), E::Any(12700.0 / 914_400.0));
    // A 2-D shape at type 0: the effect scheme. Type 3: the connector scheme.
    let plain = |s: ShapeBuilder| on_theme(s).cell("QuickStyleLineMatrix", "2").formula("LineWeight", "THEMEVAL()", "Themed");
    expect("SEL", eval_cell(&x, plain, "LineWeight"), E::Any(12700.0 / 914_400.0));
    expect("SEL", eval_cell(&x, |s| plain(s).cell("QuickStyleType", "3"), "LineWeight"), E::Any(31750.0 / 914_400.0));
    // The connector lists hold three entries, and entry 4 is past them: no entry, so the root style's cell, not entry 3's.
    let past = |s: ShapeBuilder| on_theme(s).cell("BeginX", "0").cell("QuickStyleLineMatrix", "4").formula("LineWeight", "THEMEVAL()", "Themed");
    let v = eval_cell(&x, past, "LineWeight");
    assert!(!matches!(v, diacraft_calc::Value::Number(q) if (q.stored - 31750.0 / 914_400.0).abs() < 1e-9), "{v:?}");
}

#[test]
fn the_stops_of_a_themed_gradient_by_row_and_a_row_past_the_list_has_no_answer() {
    let x = ThemeXml::default();
    let row = |ix: u32, name: &str| CellAddr::in_row("FillGradient", None, RowKey::Ix(ix), name);
    let setup = |s: ShapeBuilder| {
        let s = on_theme(s).cell("QuickStyleFillColor", "2").cell("QuickStyleFillMatrix", "3");
        (0..3).fold(s, |s, ix| {
            ["GradientStopColor", "GradientStopColorTrans", "GradientStopPosition"]
                .iter()
                .fold(s, |s, n| s.set(row(ix, n), diacraft_doc::Cell::with_formula("THEMEVAL()", Some(diacraft_doc::Value::from_text("Themed")))))
        })
    };
    let at = |ix: u32, name: &str| {
        let doc = DocBuilder::new().page(1, PageBuilder::new().shape(setup(ShapeBuilder::new(1)))).build();
        let mut doc = doc;
        doc.themes.push(x.part());
        let addr = CellAddress::new(SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0)), row(ix, name));
        eval_doc(&doc, &addr)
    };
    // Entry 3: two stops, `phClr` tint 50000 at 0, `phClr` shade 50000 at 1: of `#204060`.
    expect("stop 1", at(0, "GradientStopColor"), col("bdc0c5"));
    expect("stop 2", at(1, "GradientStopColor"), col("142c44"));
    expect("THEME", at(1, "GradientStopPosition"), E::Any(1.0));
    expect("stop 1", at(0, "GradientStopColorTrans"), E::Any(0.0));
    // The third row is past the entry's list: no value.
    expect("THEME", at(2, "GradientStopColor"), E::Err(Na));
}

#[test]
fn a_master_instance_inherits_a_themed_cell_and_the_theme_answers_on_the_instance() {
    // The master's shape holds `THEMEVAL()` in `LineColor`, the instance holds `Inh`, and the Quick Style cells are the instance's own:
    // the formula runs on the inheritor, so it is the instance's selectors that choose the entry.
    let x = ThemeXml::default();
    let master = MasterBuilder::new().shape(ShapeBuilder::new(1).formula("LineColor", "THEMEVAL()", "Themed"));
    let instance = on_theme(ShapeBuilder::new(2).master(1)).inh("LineColor", None).cell("QuickStyleLineColor", "6").cell("QuickStyleLineMatrix", "1");
    let mut doc = DocBuilder::new().master(1, master).page(1, PageBuilder::new().shape(instance)).build();
    doc.themes.push(x.part());
    let at = CellAddress::new(SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0)), CellAddr::direct("LineColor"));
    // Slot 6 is `accent5`, `#a0b0c0`; entry 1 is a plain `phClr`.
    expect("Inh", eval_doc(&doc, &at), col("a0b0c0"));
}

#[test]
fn a_theme_call_is_charged_by_the_work_it_does_and_a_spent_budget_ends_it() {
    let x = ThemeXml::default();
    let steps = |parts: usize| {
        let (mut doc, at) = probe(on_theme, "THEMEVAL(\"AccentColor3\")");
        for _ in 0..parts {
            doc.themes.push(Arc::new(Theme { theme_scheme_id: None, ..(*x.part()).clone() }));
        }
        doc.themes.push(x.part());
        let mut ctx = EvalCtx::new(EvalOptions::default(), Budget::default());
        let v = Evaluator::new(&doc, &mut ctx).evaluate(&at);
        expect("charged", v, col("214365"));
        ctx.stats.steps
    };
    let (few, many) = (steps(1), steps(200));
    assert!(many >= few + 150, "walking 200 more parts costs steps: {few} against {many}");
    // A budget of three steps cannot reach the answer: the loop error, and the run is marked spent.
    let (mut doc, at) = probe(on_theme, "THEMEVAL(\"AccentColor3\")");
    doc.themes.push(x.part());
    let mut ctx = EvalCtx::new(EvalOptions::default(), Budget::with_steps(3));
    let v = Evaluator::new(&doc, &mut ctx).evaluate(&at);
    assert!(matches!(v, diacraft_calc::Value::Error(_)) && ctx.stats.budget_exhausted, "{v:?}");
}

#[test]
fn a_variant_index_far_past_the_lists_is_no_entry_and_costs_no_walk() {
    let x = ThemeXml { colour_variants: Some((0..4).map(|_| colour_variant(None, ["010203"; 7])).collect()), ..ThemeXml::default() };
    expect("past", eval_in(&x, |s| on_theme(s).cell("VariationColorIndex", "60000"), "THEMEVAL(101)"), E::Unsupported);
}

#[test]
fn a_quoted_digit_is_a_number_of_ms_vsdx_s_numbering() {
    // The page prints a number as a quoted digit; MS-VSDX's numbering is the one followed, so "5" is `AccentColor4`.
    expect("D4", eval_in(&ThemeXml::default(), on_theme, "THEMEVAL(\"5\")"), col("708090"));
}

#[test]
fn a_scheme_number_no_part_holds_and_an_entry_past_its_list_are_counted() {
    // To warn: the evaluator counts, and a report can say so.
    let run = |x: &ThemeXml, setup: &dyn Fn(ShapeBuilder) -> ShapeBuilder, formula: &str| {
        let (mut doc, at) = probe(setup, formula);
        doc.themes.push(x.part());
        let mut ctx = EvalCtx::new(EvalOptions::default(), Budget::default());
        let v = Evaluator::new(&doc, &mut ctx).evaluate(&at);
        (v, ctx.stats.theme_fallbacks)
    };
    let (_, none) = run(&ThemeXml::default(), &on_theme, "THEMEVAL(\"AccentColor3\")");
    assert_eq!(none, 0, "a part that is found is no fallback");
    let (_, missed) = run(&ThemeXml { scheme: 41, ..ThemeXml::default() }, &on_theme, "THEMEVAL(\"AccentColor3\")");
    assert!(missed >= 1, "{missed}");
    let (_, past) = run(&ThemeXml::default(), &|s| on_theme(s).cell("QuickStyleFillMatrix", "50"), "THEMEVAL(\"FillColor\")");
    assert_eq!(past, 1);
}

#[test]
fn a_hostile_part_and_the_most_parts_a_file_may_hold_answer_within_the_budget_and_in_time() {
    // A part with a variant list of thousands (the reader keeps the first 256) among 256 parts, the most `vsdx::MAX_THEMES` allows: the call
    // walks the part list once for each scheme and no more, and answers from the last part.
    let many = ThemeXml {
        colour_variants: Some((0..9_000).map(|i| colour_variant(None, [if i % 2 == 0 { "010203" } else { "0a0b0c" }; 7])).collect()),
        ..ThemeXml::default()
    };
    let hostile = many.part();
    assert_eq!(hostile.entries.colour_variants.len(), diacraft_doc::MAX_THEME_ENTRIES);
    let (mut doc, at) = probe(|s| on_theme(s).cell("VariationColorIndex", "255"), "THEMEVAL(101)");
    for _ in 0..255 {
        doc.themes.push(Arc::new(Theme {
            color_scheme_id: None,
            effect_scheme_id: None,
            connector_scheme_id: None,
            font_scheme_id: None,
            ..(*hostile).clone()
        }));
    }
    doc.themes.push(hostile.clone());
    let started = std::time::Instant::now();
    let mut ctx = EvalCtx::new(EvalOptions::default(), Budget::default());
    let v = Evaluator::new(&doc, &mut ctx).evaluate(&at);
    let took = started.elapsed();
    expect("last variant", v, col("0a0b0c"));
    eprintln!("[timing] THEMEVAL over 256 parts: {took:?}, {} steps", ctx.stats.steps);
    // A hang guard and no measure of cost (it takes about a hundredth of a second); the cost is the steps, asserted next.
    assert!(took < std::time::Duration::from_secs(60), "took {took:?}");
    assert!(ctx.stats.steps >= 768 && ctx.stats.steps < 5_000, "the walk is charged: {} steps", ctx.stats.steps);
}
