//! The table, and the three reasons the oracle leaves a cell out under: a font number, the
//! no-shadow branch of a theme and a formula that must fail. Project rules of the oracle, labelled so in the specs.

use std::sync::Arc;

use diacraft_calc::{Budget, CellError, EvalOptions, Native, OperandMode, RecalcReport, SaltPolicy, Value, recalc_all, recalc_all_with};
use diacraft_doc::{Cell, CellAddr, Document, RowKey, Value as DocValue};
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder, StyleBuilder};
use diacraft_testkit::theme::ThemeXml;

use crate::recalc_fixes::{Echo, salted};
use crate::{E, mismatch};

const FIVE: [&str; 5] = ["ThemeIndex", "ColorSchemeIndex", "EffectSchemeIndex", "ConnectorSchemeIndex", "FontSchemeIndex"];

fn c(hex: &str) -> E {
    let n = u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap();
    E::Col((n >> 16) as u8, (n >> 8) as u8, n as u8)
}

fn expect(id: &str, got: Value, want: E) {
    assert_eq!(mismatch(&got, &want), None, "{id}: {got:?}");
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

fn schemes(s: ShapeBuilder, n: &str) -> ShapeBuilder {
    FIVE.iter().fold(s, |s, name| s.cell(name, n))
}

fn doc_of(shape: ShapeBuilder, themes: usize) -> Document {
    let mut doc = DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build();
    for _ in 0..themes {
        doc.themes.push(ThemeXml::default().part());
    }
    doc
}

fn scored(shape: ShapeBuilder, themes: usize) -> RecalcReport {
    recalc_all(&doc_of(shape, themes), &Budget::default())
}

// ---- item 7: the table ------------------------------------------------------------------------------------------------------------

/// A root style holding a different number in each cell a name maps to.
fn root(r: StyleBuilder) -> StyleBuilder {
    let direct: [(&str, &str); 38] = [
        ("LineWeight", "0.0104"),
        ("FillPattern", "7"),
        ("LinePattern", "9"),
        ("LineCap", "2"),
        ("FillGradientEnabled", "1"),
        ("Rounding", "0.25"),
        ("FillForegndTrans", "0.4"),
        ("FillBkgndTrans", "0.45"),
        ("ShdwPattern", "5"),
        ("ShapeShdwOffsetX", "0.11"),
        ("ShapeShdwOffsetY", "0.12"),
        ("ShdwForegndTrans", "0.78"),
        ("ShapeShdwObliqueAngle", "1.5"),
        ("ShapeShdwScaleFactor", "1.25"),
        ("UseGroupGradient", "1"),
        ("CompoundType", "3"),
        ("FillGradientAngle", "0.5"),
        ("FillGradientDir", "13"),
        ("ShapeShdwBlur", "0.07"),
        ("RotateGradientWithShape", "1"),
        ("BeginArrow", "4"),
        ("BeginArrowSize", "5"),
        ("EndArrow", "13"),
        ("EndArrowSize", "6"),
        ("LineGradientEnabled", "1"),
        ("LineGradientDir", "10"),
        ("LineGradientAngle", "0.3"),
        ("SketchEnabled", "1"),
        ("GlowSize", "0.2"),
        ("BevelTopType", "3"),
        ("BevelTopWidth", "0.3"),
        ("BevelTopHeight", "0.31"),
        ("BevelContourSize", "0.32"),
        ("BevelMaterialType", "8"),
        ("ReflectionSize", "0.4"),
        ("ReflectionTrans", "0.41"),
        ("ReflectionDist", "0.42"),
        ("ReflectionBlur", "0.43"),
    ];
    let more: [(&str, &str); 7] = [
        ("SoftEdgesSize", "0.5"),
        ("LineColorTrans", "0.6"),
        ("ShapeShdwType", "2"),
        ("BevelLightingType", "9"),
        ("GlowColorTrans", "0.7"),
        ("SketchAmount", "0.8"),
        ("BevelLightingAngle", "0.9"),
    ];
    let r = direct.iter().chain(more.iter()).fold(r, |r, (n, v)| r.cell(n, v));
    r.row_cell("Character", None, RowKey::Ix(0), "Font", "Root Face")
        .row_cell("Character", None, RowKey::Ix(0), "AsianFont", "Asian Face")
        .row_cell("Character", None, RowKey::Ix(0), "ComplexScriptFont", "Complex Face")
        .row_cell("Character", None, RowKey::Ix(0), "Style", "17")
        .row_cell("FillGradient", None, RowKey::Ix(0), "GradientStopColor", "#112233")
        .row_cell("FillGradient", None, RowKey::Ix(2), "GradientStopPosition", "0.75")
        .row_cell("FillGradient", None, RowKey::Ix(2), "GradientStopColorTrans", "0.2")
        .row_cell("LineGradient", None, RowKey::Ix(1), "GradientStopPosition", "0.6")
}

fn named(formula: &str) -> Value {
    let shape = schemes(ShapeBuilder::new(1), "0").row_formula("User", None, RowKey::name("Probe"), "Value", formula, "0");
    let doc = DocBuilder::new().root(root).page(1, PageBuilder::new().shape(shape)).build();
    crate::evaluate_at_probe(&doc)
}

#[test]
fn themeval_every_name_with_no_theme_reads_the_root_styles_cell_it_maps_to() {
    // The names of the table's third row, by the cell each reads: a different number in each cell of the root.
    let cases: &[(&str, f64)] = &[
        ("LineWeight", 0.0104),
        ("ConnectorWeight", 0.0104),
        ("FillPattern", 7.0),
        ("LinePattern", 9.0),
        ("ConnectorPattern", 9.0),
        ("LineCap", 2.0),
        ("FillGradientEnabled", 1.0),
        ("LineRounding", 0.25),
        ("ConnectorRounding", 0.25),
        ("FillTransparency", 0.4),
        ("FillTransparencyBkgnd", 0.45),
        ("ShadowPattern", 5.0),
        ("ShadowXOffset", 0.11),
        ("ShadowYOffset", 0.12),
        ("ShadowTransparency", 0.78),
        ("ShadowDirection", 1.5),
        ("ShadowMagnification", 1.25),
        ("UseGroupGradient", 1.0),
        ("LineCompoundType", 3.0),
        ("LineCompoundtype", 3.0),
        ("FillGradientAngle", 0.5),
        ("FillGradientDir", 13.0),
        ("ShadowBlur", 0.07),
        ("RotateGradientWithShape", 1.0),
        ("ConnectorBegin", 4.0),
        ("LineBegin", 4.0),
        ("ConnectorBeginSize", 5.0),
        ("LineBeginSize", 5.0),
        ("ConnectorEnd", 13.0),
        ("LineEnd", 13.0),
        ("ConnectorEndSize", 6.0),
        ("LineEndSize", 6.0),
        ("LineGradientEnabled", 1.0),
        ("LineGradientDir", 10.0),
        ("LineGradientAngle", 0.3),
        ("SketchEnabled", 1.0),
        ("GlowSize", 0.2),
        ("BevelTopType", 3.0),
        ("BevelTopWidth", 0.3),
        ("BevelTopHeight", 0.31),
        ("BevelContourSize", 0.32),
        ("BevelMaterial", 8.0),
        ("ReflectionSize", 0.4),
        ("ReflectionTrans", 0.41),
        ("ReflectionDist", 0.42),
        ("ReflectionBlur", 0.43),
        ("SoftEdgesSize", 0.5),
        ("SketchAmount", 0.8),
        ("BevelLightingAngle", 0.9),
        // The Learn names and their MS-VSDX twins (the table's fifth row).
        ("LineColorTrans", 0.6),
        ("ConnectorTransparency", 0.6),
        ("ShadowType", 2.0),
        ("BevelLighting", 9.0),
        ("BevelLightingType", 9.0),
        ("GlowTransparency", 0.7),
        ("GlowColorTrans", 0.7),
        ("TextStyle", 17.0),
    ];
    for (name, want) in cases {
        for spelled in [(*name).to_owned(), name.to_ascii_lowercase(), name.to_ascii_uppercase()] {
            expect(&spelled, named(&format!("THEMEVAL(\"{spelled}\")")), E::Any(*want));
        }
    }
    // The fonts read the root style's `Font`, `ComplexScriptFont` and `AsianFont` (the table's second row).
    expect("LatinFont", named(r#"THEMEVAL("LatinFont")"#), E::T("Root Face"));
    expect("ComplexFont", named(r#"THEMEVAL("ComplexFont")"#), E::T("Complex Face"));
    expect("AsianFont", named(r#"THEMEVAL("AsianFont")"#), E::T("Asian Face"));
    // A default replaces the root for every one of them.
    expect("default", named(r#"THEMEVAL("BevelTopType",99)"#), E::N(99.0));
}

#[test]
fn themeval_the_stops_read_the_roots_row_and_the_cells_own_default_where_the_root_has_none() {
    expect("FillStop1Color", named(r#"THEMEVAL("FillStop1Color")"#), c("112233"));
    expect("FillStop3Position", named(r#"THEMEVAL("FillStop3Position")"#), E::Any(0.75));
    expect("FillStop3Transparency", named(r#"THEMEVAL("FillStop3Transparency")"#), E::Any(0.2));
    expect("LineStop2Position", named(r#"THEMEVAL("linestop2position")"#), E::Any(0.6));
    // No row in the root: the cell's own default, 0 for a position and a transparency.
    expect("FillStop5Transparency", named(r#"THEMEVAL("FillStop5Transparency")"#), E::Any(0.0));
    expect("LineStop10Position", named(r#"THEMEVAL("LineStop10Position")"#), E::Any(0.0));
    // A stop past ten is no name MS-VSDX lists: `#VALUE!`.
    expect("LineStop11Color", named(r#"THEMEVAL("LineStop11Color")"#), E::Err(CellError::Value));
    expect("FillStop0Color", named(r#"THEMEVAL("FillStop0Color")"#), E::Err(CellError::Value));
}

#[test]
fn themeval_the_names_that_give_value_error_and_the_one_that_gives_4() {
    // The five that files save `#VALUE!` for in every cell, and the one every sample reads as 4 (`ConnectorEnd2`, root `EndArrow` 13).
    for name in ["ShadowStyle", "LineTransparency", "FillGradStop1Position", "FillGradStop2Position", "FillGradStop3Position"] {
        expect(name, named(&format!("THEMEVAL(\"{name}\")")), E::Err(CellError::Value));
        expect(&format!("{name}+0"), named(&format!("THEMEVAL(\"{name}\")+0")), E::Err(CellError::Value));
        // The name test comes first: a `default` does not save an unknown name (our rule).
        expect(&format!("{name}, default"), named(&format!("THEMEVAL(\"{name}\",5)")), E::Err(CellError::Value));
    }
    expect("AccentColor1", named(r#"THEMEVAL("AccentColor1")"#), E::Err(CellError::Value));
    expect("NoSuchName", named(r#"THEMEVAL("NoSuchName",5)"#), E::Err(CellError::Value));
    expect("ConnectorEnd2", named(r#"THEMEVAL("ConnectorEnd2")"#), E::N(4.0));
    expect("connectorend2", named(r#"THEMEVAL("connectorend2")"#), E::N(4.0));
    expect("ConnectorEnd2, default", named(r#"THEMEVAL("ConnectorEnd2",9)"#), E::N(9.0));
}

#[test]
fn themeval_the_vectors_of_the_table() {
    // A root holding `CompoundType` 3 and `ShapeShdwType` 2.
    let v16 = |f: &str| {
        let shape = schemes(ShapeBuilder::new(1), "0").row_formula("User", None, RowKey::name("Probe"), "Value", f, "0");
        let doc = DocBuilder::new().root(|r| r.cell("CompoundType", "3").cell("ShapeShdwType", "2")).page(1, PageBuilder::new().shape(shape)).build();
        crate::evaluate_at_probe(&doc)
    };
    expect("shadowstyle", v16(r#"THEMEVAL("shadowstyle")+0"#), E::Err(CellError::Value));
    expect("shadowtype", v16(r#"THEMEVAL("shadowtype")+0"#), E::Any(2.0));
    expect("linecompoundtype", v16(r#"THEMEVAL("linecompoundtype")+0"#), E::Any(3.0));
    // A root holding `BeginArrowSize` 2 and `LineWeight` 0.01 inches.
    let v19 = |f: &str| {
        let shape = schemes(ShapeBuilder::new(1), "0").row_formula("User", None, RowKey::name("Probe"), "Value", f, "0");
        let doc = DocBuilder::new()
            .root(|r| r.cell("BeginArrowSize", "2").cell_u("LineWeight", "0.01", "IN"))
            .page(1, PageBuilder::new().shape(shape))
            .build();
        crate::evaluate_at_probe(&doc)
    };
    expect("connectorbeginsize", v19(r#"3*THEMEVAL("connectorbeginsize")"#), E::Any(6.0));
    expect("connectorweight", v19(r#"THEMEVAL("connectorweight")*2"#), E::Any(0.02));
}

#[test]
fn an_unknown_name_is_value_error_under_a_theme_too_and_connector_end_2_reads_connector_end() {
    // The name test comes first under a theme as well; `ConnectorEnd2` is read as `ConnectorEnd` there (our rule), whose list entry 2
    // gives the line end 2.
    let at = |f: &str| {
        let shape = schemes(ShapeBuilder::new(1), "40").cell("QuickStyleLineMatrix", "2").cell("QuickStyleEffectsMatrix", "2").row_formula(
            "User",
            None,
            RowKey::name("Probe"),
            "Value",
            f,
            "0",
        );
        crate::evaluate_at_probe(&doc_of(shape, 1))
    };
    expect("ShadowStyle", at(r#"THEMEVAL("ShadowStyle")"#), E::Err(CellError::Value));
    expect("ShadowStyle, default", at(r#"THEMEVAL("ShadowStyle",5)"#), E::Err(CellError::Value));
    expect("LineTransparency", at(r#"THEMEVAL("LineTransparency")"#), E::Err(CellError::Value));
    expect("AccentColor1", at(r#"THEMEVAL("AccentColor1")"#), E::Err(CellError::Value));
    expect("ShadowType still answers", at(r#"THEMEVAL("ShadowType")"#), E::Any(1.0));
    expect("ConnectorEnd", at(r#"THEMEVAL("ConnectorEnd")"#), E::Any(2.0));
    expect("ConnectorEnd2", at(r#"THEMEVAL("ConnectorEnd2")"#), E::Any(2.0));
}

// ---- item 8: a font number is the machine's -----------------------------------------------------------------------------

fn font_cell(name: &str, formula: &str, saved: &str) -> ShapeBuilder {
    ShapeBuilder::new(1).set(CellAddr::in_row("Character", None, RowKey::Ix(0), name), cell(formula, saved, None, None))
}

#[test]
fn a_font_cell_that_evaluates_to_a_number_other_than_0_is_left_out_as_the_machines() {
    // 26,627 cells (493 files, 120 sources) save a face name where `THEMEVAL("LatinFont", n)` gives n above 0 with no theme.
    for (cell_name, call) in [("Font", "latinfont"), ("ComplexScriptFont", "ComplexFont"), ("AsianFont", "AsianFont")] {
        let r = scored(font_cell(cell_name, &format!("THEMEVAL(\"{call}\",17)"), "Some Face"), 0);
        assert_eq!((r.skipped.machine_font_number, r.agreed, r.differed.len()), (1, 0, 0), "{cell_name}: {r:?}");
        assert_eq!(r.accounted(), r.cells, "{r:?}");
    }
    // The value saved does not matter: a number saved is left out as well, and an error saved (5 cells hold `#NAME?`).
    let r = scored(font_cell("Font", r#"THEMEVAL("LatinFont",17)"#, "17"), 0);
    assert_eq!((r.skipped.machine_font_number, r.agreed, r.differed.len()), (1, 0, 0), "{r:?}");
    let shape = ShapeBuilder::new(1)
        .set(CellAddr::in_row("Character", None, RowKey::Ix(0), "Font"), cell(r#"THEMEVAL("LatinFont",17)"#, "Some Face", None, Some("#NAME?")));
    assert_eq!(scored(shape, 0).skipped.machine_font_number, 1);
    // A number 0 is not machine-dependent: the `17-17` with a saved 0 is scored, and agrees (6,270 cells).
    let r = scored(font_cell("Font", r#"THEMEVAL("latinfont",17-17)"#, "0"), 0);
    assert_eq!((r.skipped.machine_font_number, r.agreed, r.differed.len()), (0, 1, 0), "{r:?}");
    // A face name given by the formula is scored too: the root's `Font`, with no default.
    let shape = font_cell("Font", r#"THEMEVAL("LatinFont")"#, "Root Face");
    let doc = DocBuilder::new().root(root).page(1, PageBuilder::new().shape(shape)).build();
    let r = recalc_all(&doc, &Budget::default());
    assert_eq!((r.skipped.machine_font_number, r.agreed, r.differed.len()), (0, 1, 0), "{r:?}");
    // Any other cell holding a number is scored as ever.
    let shape = ShapeBuilder::new(1).set(user("X"), cell("17", "Some Face", None, None));
    let r = scored(shape, 0);
    assert_eq!((r.skipped.machine_font_number, r.differed.len()), (0, 1), "{r:?}");
}

// ---- item 9: the no-shadow branch of a theme ---------------------------------------------------------------------------------------

/// A shape selecting the composed theme, over the effect entry `entry` (1 holds no shadow, 2 holds an outer shadow), with `cells` holding `THEMEVAL()`
/// beside a value saved for each.
fn shadowed(entry: &str, cells: &[(&str, &str)]) -> ShapeBuilder {
    let base = schemes(ShapeBuilder::new(1), "40").cell("QuickStyleEffectsMatrix", entry).cell("QuickStyleShadowColor", "2");
    cells.iter().fold(base, |s, (name, saved)| s.formula(name, "THEMEVAL()", saved))
}

const SHADOW_CELLS: [(&str, &str); 9] = [
    ("ShdwForegnd", "#aabbcc"),
    ("ShdwForegndTrans", "0.78"),
    ("ShapeShdwOffsetX", "0.11"),
    ("ShapeShdwOffsetY", "0.12"),
    ("ShapeShdwType", "3"),
    ("ShdwPattern", "1"),
    ("ShapeShdwBlur", "0.07"),
    ("ShapeShdwScaleFactor", "1.5"),
    ("ShapeShdwObliqueAngle", "1.5"),
];

#[test]
fn every_shadow_property_a_theme_answers_on_the_no_shadow_branch_is_left_out() {
    // 72,458 cells in the class 2: the theme part holds no outer shadow, and the files save a colour and 0.78.
    let r = scored(shadowed("1", &SHADOW_CELLS), 1);
    assert_eq!((r.skipped.theme_no_shadow, r.agreed, r.differed.len()), (9, 0, 0), "{r:?}");
    assert_eq!(r.accounted(), r.cells, "{r:?}");
    // The entry decides and not the saved value: a saved `#000000`, which agrees with what the branch gives, is left out too.
    let r = scored(shadowed("1", &[("ShdwForegnd", "#000000"), ("ShdwPattern", "0")]), 1);
    assert_eq!((r.skipped.theme_no_shadow, r.agreed, r.differed.len()), (2, 0, 0), "{r:?}");
    // The properties by their own names, in a cell of another kind.
    for (name, saved) in [
        ("ShadowColor", "#112233"),
        ("ShadowTransparency", "0.78"),
        ("ShadowXOffset", "0.3"),
        ("ShadowYOffset", "0.3"),
        ("ShadowType", "2"),
        ("ShadowPattern", "1"),
        ("ShadowBlur", "0.2"),
        ("ShadowMagnification", "1.5"),
        ("ShadowDirection", "1.5"),
    ] {
        let shape = shadowed("1", &[]).set(user("S"), cell(&format!("THEMEVAL(\"{name}\")"), saved, None, None));
        let r = scored(shape, 1);
        assert_eq!((r.skipped.theme_no_shadow, r.agreed + r.differed.len() as u64), (1, 0), "{name}: {r:?}");
    }
}

#[test]
fn the_same_properties_are_scored_when_the_entry_holds_an_outer_shadow_and_other_properties_always_are() {
    // Effect entry 2 holds an outer shadow (12700 EMU at 90 degrees, `#000000` at 38 percent): nothing is left out, and the colour agrees.
    let r = scored(shadowed("2", &[("ShdwForegnd", "#000000"), ("ShdwPattern", "1"), ("ShapeShdwOffsetX", "0")]), 1);
    assert_eq!((r.skipped.theme_no_shadow, r.agreed, r.differed.len()), (0, 3, 0), "{r:?}");
    // And a wrong saved colour is a difference there.
    let r = scored(shadowed("2", &[("ShdwForegnd", "#aabbcc")]), 1);
    assert_eq!((r.skipped.theme_no_shadow, r.agreed, r.differed.len()), (0, 0, 1), "{r:?}");
    // On the no-shadow entry a cell that is no shadow property is scored: the fill colour of fill entry 2 (`#1b3754`).
    let shape = shadowed("1", &[]).cell("QuickStyleFillColor", "2").cell("QuickStyleFillMatrix", "2").formula("FillForegnd", "THEMEVAL()", "#1b3754");
    let r = scored(shape, 1);
    assert_eq!((r.skipped.theme_no_shadow, r.agreed, r.differed.len()), (0, 1, 0), "{r:?}");
    // With no theme selected a shadow call is not this branch (the root answers): scored.
    let shape = schemes(ShapeBuilder::new(1), "0").formula("ShdwForegnd", "THEMEVAL()", "#000000");
    let r = scored(shape, 1);
    assert_eq!((r.skipped.theme_no_shadow, r.agreed, r.differed.len()), (0, 1, 0), "{r:?}");
}

// ---- item 10: a cell whose own formula can only fail ------------------------------------------------------------------------------------

fn failing(formula: &str, saved: &str, e: Option<&str>) -> RecalcReport {
    scored(ShapeBuilder::new(1).set(user("X"), cell(formula, saved, None, e)), 0)
}

#[test]
fn a_reached_ref_whose_error_is_the_result_is_left_out_when_the_file_saves_a_value_and_no_error() {
    // 527 cells (57 files): a `REF()` in the formula, `V` kept and no `E`.
    for formula in ["REF()", "REF()+1", "1+REF()", "SUM(REF(),2)", "GUARD(REF())", "IF(1<2,REF(),3)", "MAX(REF(),1)"] {
        let r = failing(formula, "5", None);
        assert_eq!((r.skipped.formula_must_fail, r.agreed, r.differed.len()), (1, 0, 0), "{formula}: {r:?}");
        assert_eq!(r.accounted(), r.cells, "{formula}: {r:?}");
    }
    // A cell that saves `E` `#REF!` is scored as usual, and agrees.
    let r = failing("REF()+1", "5", Some("#REF!"));
    assert_eq!((r.skipped.formula_must_fail, r.agreed, r.differed.len()), (0, 1, 0), "{r:?}");
    // Another error saved is a difference, not a left-out cell.
    let r = failing("REF()+1", "5", Some("#VALUE!"));
    assert_eq!((r.skipped.formula_must_fail, r.differed.len()), (0, 1), "{r:?}");
}

#[test]
fn a_ref_the_formula_does_not_reach_or_does_not_pass_on_is_scored() {
    // Caught by `IFERROR` or by a test: the value is the other branch's, and compared (28 cells).
    let r = failing("IFERROR(REF(),3)", "3", None);
    assert_eq!((r.skipped.formula_must_fail, r.agreed, r.differed.len()), (0, 1, 0), "{r:?}");
    let r = failing("IFERROR(REF(),3)", "4", None);
    assert_eq!((r.skipped.formula_must_fail, r.agreed, r.differed.len()), (0, 0, 1), "{r:?}");
    let r = failing("IF(ISERROR(REF()),8,9)", "8", None);
    assert_eq!((r.skipped.formula_must_fail, r.agreed), (0, 1), "{r:?}");
    // In a branch not taken, the token is not reached.
    let r = failing("IF(1>2,REF(),5)", "5", None);
    assert_eq!((r.skipped.formula_must_fail, r.agreed, r.differed.len()), (0, 1, 0), "{r:?}");
    // The error that reaches the result is another: the leftmost comes back.
    let r = failing("1/0+REF()", "5", None);
    assert_eq!((r.skipped.formula_must_fail, r.differed.len()), (0, 1), "{r:?}");
}

#[test]
fn a_ref_from_a_row_or_a_shape_the_file_lacks_stays_scored() {
    // The 79 other `#REF!` cells that save no `E`: a lookup bug would look the same.
    let r = failing("User.Lacking", "5", None);
    assert_eq!((r.skipped.formula_must_fail, r.agreed, r.differed.len()), (0, 0, 1), "{r:?}");
    let r = failing("Sheet.9321!Width", "5", None);
    assert_eq!((r.skipped.formula_must_fail, r.differed.len()), (0, 1), "{r:?}");
    let r = failing("Prop.Cost", "5", None);
    assert_eq!((r.skipped.formula_must_fail, r.differed.len()), (0, 1), "{r:?}");
    // Naming a cell the section does hold, in a row that is not there.
    let r = failing("Scratch.A9", "5", None);
    assert_eq!((r.skipped.formula_must_fail, r.differed.len()), (0, 1), "{r:?}");
    let r = failing("Geometry1.X9", "5", None);
    assert_eq!((r.skipped.formula_must_fail, r.differed.len()), (0, 1), "{r:?}");
}

#[test]
fn a_reference_to_a_cell_its_section_can_never_hold_is_left_out() {
    // 576 cells (2 files): a `Geometry` cell naming a `Tabs` cell `Q`, where a `Tabs` row holds only `Position` and `Alignment` pairs.
    let geometry = |formula: &str| {
        let shape = ShapeBuilder::new(1).set(CellAddr::in_row("Geometry", Some(0), RowKey::Ix(1), "X"), cell(formula, "3", None, None));
        scored(shape, 0)
    };
    let r = geometry("Tabs.Q1+1");
    assert_eq!((r.skipped.formula_must_fail, r.agreed, r.differed.len()), (1, 0, 0), "{r:?}");
    let r = geometry("Tabs.Q12");
    assert_eq!((r.skipped.formula_must_fail, r.differed.len()), (1, 0), "{r:?}");
    // The cells a `Tabs` row can hold are not: `Position`, `Alignment`, and `X1`, with a stop's number run on, and the `Tabs.12` form
    // names no cell by letter.
    for formula in ["Tabs.Position1", "Tabs.Alignment2", "Tabs.X1", "Tabs.12"] {
        let r = geometry(formula);
        assert_eq!((r.skipped.formula_must_fail, r.differed.len()), (0, 1), "{formula}: {r:?}");
    }
    // The same of another section whose cells the catalogue lists in full.
    let r = failing("Scratch.Q1", "5", None);
    assert_eq!((r.skipped.formula_must_fail, r.differed.len()), (1, 0), "{r:?}");
    // `User` is catalogued and its rows hold `Value` and `Prompt` only: judged as `Scratch` is.
    let r = failing("User.Row.Zork", "5", None);
    assert_eq!((r.skipped.formula_must_fail, r.differed.len()), (1, 0), "{r:?}");
    // A section the specs have not catalogued yet is not judged.
    let r = failing("Reviewer.Zork[1]", "5", None);
    assert_eq!((r.skipped.formula_must_fail, r.differed.len()), (0, 1), "{r:?}");
    // A cell that saves `E` `#REF!`, as the files with these formulas mostly do, agrees.
    let r = scored(ShapeBuilder::new(1).set(user("X"), cell("Scratch.Q1", "5", None, Some("#REF!"))), 0);
    assert_eq!((r.skipped.formula_must_fail, r.agreed), (0, 1), "{r:?}");
}

#[test]
fn only_a_cells_own_formula_counts_and_not_a_cell_it_reads() {
    // In Live mode the reader of a failing cell reads its last good value: B is `A` plus 1, and neither its formula nor its result is the failure.
    let shape = ShapeBuilder::new(1).set(user("A"), cell("REF()", "5", None, None)).set(user("B"), cell("User.A+1", "6", None, None));
    let live = EvalOptions { operands: OperandMode::Live, ..EvalOptions::default() };
    let r = recalc_all_with(&doc_of(shape, 0), &Budget::default(), live);
    assert_eq!((r.skipped.formula_must_fail, r.agreed, r.differed.len()), (1, 1, 0), "{r:?}");
}

// ---- the salt still catches an echo over the cells of the new rules --------------------------------------------------------------------------------

#[test]
fn an_echo_still_fails_the_real_salt_over_cells_that_only_the_new_rules_make_agree() {
    // Each cell here differed or was unanswered before a change (a case-blind row, a call before a colour, a seventh variant colour, a bare call in a
    // cell no table covers, a colour of a group's theme): the salt puts a wrong value in and the evaluator catches it where an echo does not.
    let member = ShapeBuilder::new(2)
        .links(Some(2), Some(2), Some(2))
        .set(user("Group"), cell(r#"THEMEVAL("accentcolor3")"#, "#214365", None, None))
        .set(user("Row"), cell("IFERROR(LISTSHEETREF()!User.TALLY9,-1)", "7", None, None))
        .formula("Relationships", &crate::relationships(&[(5, &[3])]), "0");
    let group = (1..=5).fold(ShapeBuilder::new(1).group(), |s, i| s.cell(FIVE[i - 1], "40")).child(member);
    let list = ShapeBuilder::new(3).formula("Relationships", &crate::relationships(&[(2, &[2])]), "0").row_cell(
        "User",
        None,
        RowKey::name("Tally9"),
        "Value",
        "7",
    );
    let loose = ShapeBuilder::new(4)
        .row_cell("Property", None, RowKey::name("Width"), "Value", "3")
        .set(user("Sum"), cell("SETATREF(Prop.Width,,1)+RGB(1,2,3)", "#010203", None, None))
        .set(user("Variant"), cell(r#"THEMEVAL("VariantColor7")"#, "#000000", None, None))
        .formula("TextBkgnd", "THEMEVAL()", "0");
    let style = |n: &str| EIGHT.iter().fold(StyleBuilder::new(), |s, name| s.cell(name, n));
    let mut doc = DocBuilder::new().style(2, style("65534")).page(1, PageBuilder::new().shape(group).shape(list).shape(loose)).build();
    doc.themes.push(ThemeXml::default().part());
    let policy = SaltPolicy { seed: 11, one_in: 1 };
    let honest = salted(&doc, &mut Native, policy);
    assert!(honest.differed.is_empty(), "{honest:?}");
    assert!(honest.agreed >= 5, "the new rules agree: {honest:?}");
    assert!(honest.real_salt.passed() && honest.real_salt.salted >= 5, "{:?}", honest.real_salt);
    let echo = salted(&doc, &mut Echo, policy);
    assert!(!echo.real_salt.passed() && echo.real_salt.missed > 0, "an echo agrees with every salted cell: {:?}", echo.real_salt);
}

const EIGHT: [&str; 8] = [
    "ThemeIndex",
    "ColorSchemeIndex",
    "EffectSchemeIndex",
    "ConnectorSchemeIndex",
    "FontSchemeIndex",
    "VariationColorIndex",
    "VariationStyleIndex",
    "EmbellishmentIndex",
];
