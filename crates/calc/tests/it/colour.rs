//! Every vector of the batch that the evaluator works, as data.
//!
//! **Run in `theme.rs`, not here:** `THEMEVAL` under an explicit theme (the THEMEVAL vectors 1, 3 and 5 to 13), `THEMECBV` and
//! `THEMEPROP`, which read a theme part's lists. What stays `Unsupported` under an explicit theme is a file with no theme
//! part, a custom colour scheme and the properties not worked out (a test below pins the first): the saved value then stands, and the oracle
//! counts the cell under `unsupported` and never as agreed.

use diacraft_calc::CellError::{DivZero, Value};
use diacraft_calc::{EvalCtx, Evaluator};
use diacraft_doc::{ColorEntry, RowKey};
use diacraft_testkit::build::ShapeBuilder;
use diacraft_units::Unit;

use crate::{E, check_table, check_table_on, eval, eval_on, mismatch, probe};

/// A colour as the spec prints it.
fn c(hex: &str) -> E {
    let n = u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap();
    E::Col((n >> 16) as u8, (n >> 8) as u8, n as u8)
}

#[test]
fn rgb_and_the_conversions() {
    check_table(&[
        ("ALL", "RGB(255,0,0)", c("#ff0000")),
        ("ALL", "\"x\"&RGB(1,2,3)", E::T("xRGB(1, 2, 3)")),
        ("ALL", "RGB(1,2,3)+1", c("#010203")),
        ("RGB", "RGB(1,2,3)", c("#010203")),
        ("RGB", "RGB(257,2,3)", c("#010203")),
        ("RGB", "RGB(1.9,2.9,3.9)", c("#010203")),
        ("RGB", "RGB(-1,2,3)", c("#ff0203")),
        ("CONV", "HUE(RGB(200,100,50))", E::N(13.0)),
        ("CONV", "LUM(RGB(200,100,50))", E::N(118.0)),
        ("CONV", "SAT(RGB(200,100,50))", E::N(144.0)),
        ("CONV", "SAT(RGB(33,71,223))", E::N(178.0)),
        ("CONV", "HUE(RGB(128,128,128))", E::N(160.0)),
        ("CONV", "LUM(RGB(128,128,128))", E::N(120.0)),
        ("CONV", "SAT(RGB(128,128,128))", E::N(0.0)),
        ("CONV", "HUE(RGB(255,0,0))", E::N(0.0)),
        ("CONV", "LUM(RGB(255,0,0))", E::N(120.0)),
        ("CONV", "SAT(RGB(255,0,0))", E::N(240.0)),
        ("CONV", "HSL(70,90,100)", c("#569342")),
        ("CONV", "HSL(0,0,120)", c("#808080")),
        ("HSL", "HSL(160,240,120)", c("#0000ff")),
        ("HSL", "HSL(70.9,90.9,100.9)", c("#569342")),
        ("HSL", "HSL(300,240,120)", c("#e6ff00")),
        ("HSL", "HSL(250,240,120)", c("#ff0000")),
        ("HSL", "HSL(0,0,120)", c("#808080")),
    ]);
}

#[test]
fn the_parts_of_a_colour() {
    check_table(&[
        ("BLUE", "BLUE(13)", E::N(128.0)),
        ("BLUE", "BLUE(RGB(10,20,30))", E::N(30.0)),
        ("BLUE", "BLUE(\"word\")", E::N(0.0)),
        ("GREEN", "GREEN(11)", E::N(128.0)),
        ("GREEN", "GREEN(RGB(10,20,30))", E::N(20.0)),
        ("GREEN", "GREEN(\"word\")", E::N(0.0)),
        ("RED", "RED(22)", E::N(51.0)),
        ("RED", "RED(RGB(10,20,30))", E::N(10.0)),
        ("RED", "RED(\"word\")", E::N(0.0)),
        ("HUE", "HUE(7)", E::N(120.0)),
        ("HUE", "HUE(RGB(134,147,67))", E::N(47.0)),
        ("HUE", "HUE(HSL(10,20,30))", E::N(10.0)),
        ("LUM", "LUM(6)", E::N(120.0)),
        ("LUM", "LUM(HSL(10,20,30))", E::N(30.0)),
        ("LUM", "LUM(RGB(200,100,50))", E::N(118.0)),
        ("SAT", "SAT(8)", E::N(240.0)),
        ("SAT", "SAT(HSL(10,20,30))", E::N(20.0)),
        ("SAT", "SAT(RGB(33,71,223))", E::N(178.0)),
        ("HUEDIFF", "HUEDIFF(RGB(0,0,255),RGB(255,0,0))", E::N(160.0)),
        ("HUEDIFF", "HUEDIFF(RGB(255,0,0),RGB(0,0,255))", E::N(-160.0)),
        ("LUMDIFF", "LUMDIFF(RGB(150,175,207),RGB(191,206,225))", E::N(-28.0)),
        ("LUMDIFF", "LUMDIFF(RGB(191,206,225),RGB(150,175,207))", E::N(28.0)),
        ("LUMDIFF", "LUMDIFF(RGB(10,20,30),RGB(10,20,30))", E::N(0.0)),
        ("SATDIFF", "SATDIFF(RGB(200,100,50),RGB(128,128,128))", E::N(144.0)),
        ("SATDIFF", "SATDIFF(RGB(128,128,128),RGB(200,100,50))", E::N(-144.0)),
    ]);
}

#[test]
fn a_part_of_a_colour_cell_goes_through_the_rgb_it_holds() {
    // A cell holding `HSL(10,20,30)` has `V` `#231f1d`, so the hue of that cell is 13 and not 10.
    let setup = |s: ShapeBuilder| s.row_formula("User", None, RowKey::name("Q"), "Value", "HSL(10,20,30)", "#231f1d");
    check_table_on(setup, &[("HUE", "HUE(User.Q)", E::N(13.0)), ("HUE", "HUE(HSL(10,20,30))", E::N(10.0))]);
    // The colour `HSL(10,20,30)` is, the one the spec saved.
    let v = eval("HSL(10,20,30)");
    assert_eq!(mismatch(&v, &c("#231f1d")), None, "{v:?}");
}

#[test]
fn a_colour_table_entry_and_a_missing_index() {
    // An index beyond the palette names a `ColorEntry` by `IX`, and one in neither is colour 0.
    for (formula, expected) in [("GREEN(300)", 0.0), ("BLUE(30)", 86.0)] {
        let (mut doc, at) = probe(|s| s, formula);
        doc.colors = std::sync::Arc::new(vec![ColorEntry { ix: 30, rgb: "#123456".into() }]);
        let mut ctx = EvalCtx::default();
        let v = Evaluator::new(&doc, &mut ctx).evaluate(&at);
        assert_eq!(mismatch(&v, &E::N(expected)), None, "{formula}: {v:?}");
    }
}

#[test]
fn tint_shade_tone_and_blend() {
    check_table(&[
        ("BLEND", "BLEND(RGB(0,0,0),RGB(200,100,40),0.25)", c("#32190a")),
        ("BLEND", "BLEND(RGB(0,0,0),RGB(200,100,40),1.5)", E::Err(Value)),
        ("BLEND", "BLEND(RGB(0,0,0),RGB(200,100,40),-0.1)", E::Err(Value)),
        ("BLEND", "BLEND(RGB(10,20,30),RGB(200,100,40),0)", c("#0a141e")),
        ("BLEND", "BLEND(RGB(10,20,30),RGB(200,100,40),1)", c("#c86428")),
        ("TINT", "TINT(RGB(10,20,30),37)", c("#1e3c59")),
        ("TINT", "TINT(RGB(254,254,254),37)", c("#fefefe")),
        ("TINT", "TINT(RGB(153,153,153),28)", c("#b5b5b5")),
        ("TINT", "TINT(RGB(200,100,50),-50)", c("#74391d")),
        ("TINT", "TINT(RGB(10,20,30),300)", c("#ffffff")),
        ("SHADE", "SHADE(RGB(64,88,102),53)", c("#1f2930")),
        ("SHADE", "SHADE(RGB(255,193,1),-28)", c("#fece3d")),
        ("SHADE", "SHADE(RGB(10,20,30),300)", c("#000000")),
        ("TONE", "TONE(RGB(200,100,50),30)", c("#b96842")),
        ("TONE", "TONE(RGB(200,100,50),-30)", c("#d85e23")),
        ("TONE", "TONE(RGB(120,110,100),30)", c("#766f67")),
        ("MSOTINT", "MSOTINT(RGB(0,0,0),25)", c("#3f3f3f")),
        ("MSOTINT", "MSOTINT(RGB(71,115,197),40)", c("#90abdc")),
        ("MSOTINT", "MSOTINT(RGB(255,255,255),-35)", c("#a5a5a5")),
        ("MSOTINT", "MSOTINT(RGB(200,100,50),150)", c("#ffffff")),
        ("MSOTINT", "MSOTINT(RGB(200,100,50),-150)", c("#000000")),
        ("MSOSHADE", "MSOSHADE(RGB(200,100,50),30)", c("#8c4522")),
        ("MSOSHADE", "MSOSHADE(RGB(200,100,50),-30)", c("#da916d")),
        ("MSOSHADE", "MSOSHADE(RGB(200,100,50),150)", c("#000000")),
    ]);
}

#[test]
fn theme_wrappers() {
    check_table(&[
        ("THEMEGUARD", "THEMEGUARD(2+3)", E::N(5.0)),
        ("THEMEGUARD", "THEMEGUARD(3 cm)", E::U(3.0 / 2.54, Unit::Centimetre)),
        ("THEMEGUARD", "THEMEGUARD(7/0)", E::Err(DivZero)),
        ("THEMERESTORE", "THEMERESTORE(5)", E::N(0.0)),
        ("THEMERESTORE", "RGB(1,2,3)+THEMERESTORE(9)", c("#010203")),
        ("THEMERESTORE", "THEMERESTORE(1/0)", E::N(0.0)),
        ("CELLISTHEMED", "CELLISTHEMED(TRUE)", E::B(true)),
        ("CELLISTHEMED", "CELLISTHEMED(FALSE)", E::B(false)),
        ("CELLISTHEMED", "CELLISTHEMED(1/0)", E::Err(DivZero)),
        ("CELLISTHEMED", "CELLISTHEMED(2)", E::B(true)),
        ("USE", "USE(\"Some Master\")", E::N(254.0)),
        ("USE", "USE({00000000-1111-2222-3333-444444444444})", E::N(254.0)),
        ("formula-language V53", "USE(\"{00000000-1111-2222-3333-444444444444}\")", E::N(254.0)),
    ]);
}

#[test]
fn isthemed_follows_the_colour_scheme_cell() {
    check_table_on(|s| s.cell("ColorSchemeIndex", "33"), &[("ISTHEMED", "ISTHEMED()", E::B(true))]);
    check_table_on(|s| s.cell("ColorSchemeIndex", "0"), &[("ISTHEMED", "ISTHEMED()", E::B(false))]);
    check_table_on(|s| s.cell("ColorSchemeIndex", "33").cell("EffectSchemeIndex", "0"), &[("ISTHEMED", "ISTHEMED()", E::B(true))]);
    // 65534 reads the page, and a page holding none is no theme.
    check_table_on(|s| s.cell("ColorSchemeIndex", "65534"), &[("ISTHEMED", "ISTHEMED()", E::B(false))]);
}

#[test]
fn the_fixed_theme() {
    let setup = |s: ShapeBuilder| s.row_cell("User", None, RowKey::name("msvThemeColors"), "Value", "5");
    check_table_on(setup, &[("THEME", "THEME(\"FillColor\")", c("#6fbbce"))]);
    check_table(&[("THEME", "THEME(\"FillColor\")", c("#96afcf")), ("THEME", "THEME(\"NoSuchProperty\")", E::Err(Value))]);
    let setup = |s: ShapeBuilder| s.row_cell("User", None, RowKey::name("msvThemeEffects"), "Value", "3");
    check_table_on(setup, &[("THEME", "THEME(\"LineWeight\")", E::Any(0.016666667))]);
}

#[test]
fn themeval_with_no_theme() {
    // All five cells 0.
    let none = |s: ShapeBuilder| {
        s.cell("ThemeIndex", "0")
            .cell("ColorSchemeIndex", "0")
            .cell("EffectSchemeIndex", "0")
            .cell("ConnectorSchemeIndex", "0")
            .cell("FontSchemeIndex", "0")
    };
    check_table_on(
        none,
        &[
            ("THEMEVAL", "THEMEVAL(\"FillColor\")", c("#96afcf")),
            ("THEMEVAL", "THEMEVAL(\"FillColor\",RGB(1,2,3))", c("#010203")),
            ("THEMEVAL", "THEMEVAL(\"FillPattern\",7)", E::N(7.0)),
            ("THEMEVAL", "THEMEVAL(\"AccentColor\")", c("#c05046")),
            ("THEMEVAL", "THEMEVAL(\"accentcolor3\")", c("#ab9ac0")),
            ("THEMEVAL", "THEMEVAL(\"LineColor\")", c("#1f477d")),
            ("THEMEVAL", "THEMEVAL(\"NoSuchProperty\")", E::Err(Value)),
            ("THEMEVAL", "THEMEVAL(2)", c("#c05046")),
        ],
    );
    // 65534 and a page that holds no scheme cell is no theme either.
    let by_page = |s: ShapeBuilder| s.cell("ThemeIndex", "65534").cell("ColorSchemeIndex", "65534").cell("FontSchemeIndex", "65534");
    check_table_on(by_page, &[("THEMEVAL", "THEMEVAL(\"FillColor\")", c("#96afcf"))]);
    // No cell at all: the page holds none, which is 0.
    check_table(&[("THEMEVAL", "THEMEVAL(\"FillColor\")", c("#96afcf"))]);
}

#[test]
fn a_theme_with_no_part_to_answer_is_unsupported() {
    // With a theme selected the answer is in the theme part, and this document has none: no value, so the saved one stands (`theme.rs` has
    // the same call answered by a part).
    let themed = |s: ShapeBuilder| {
        s.cell("ThemeIndex", "40")
            .cell("ColorSchemeIndex", "40")
            .cell("EffectSchemeIndex", "40")
            .cell("ConnectorSchemeIndex", "40")
            .cell("FontSchemeIndex", "40")
    };
    let v = eval_on(themed, "THEMEVAL(\"AccentColor3\")");
    assert_eq!(mismatch(&v, &E::Unsupported), None, "{v:?}");
    // THEMECBV: the stop is checked first, and no theme leaves the colour as it is.
    let none = |s: ShapeBuilder| s.cell("ThemeIndex", "0").cell("ColorSchemeIndex", "0");
    check_table_on(
        none,
        &[
            ("THEMECBV", "THEMECBV(RGB(0,255,0),2)", c("#00ff00")),
            ("THEMECBV", "THEMECBV(RGB(0,255,0),11)", E::Err(Value)),
            ("THEMECBV", "THEMECBV(RGB(0,255,0),0)", E::Err(Value)),
        ],
    );
}

#[test]
fn a_string_spelling_a_colour_is_read_into_a_colour_cell() {
    // `FillForegnd` holding a reference to a string cell that spells an `RGB` call is that colour.
    let setup = |s: ShapeBuilder| {
        let text = diacraft_doc::Cell::constant(diacraft_doc::Value::from_text("RGB(10,20,30)")).with_unit("STR");
        s.set(diacraft_doc::CellAddr::in_row("User", None, RowKey::name("Q"), "Value"), text).formula("FillForegnd", "GUARD(User.Q)", "#0A141E")
    };
    let (doc, _) = probe(setup, "1");
    let at = diacraft_calc::CellAddress::new(
        diacraft_doc::SheetRef::Shape(diacraft_doc::ShapeRef::top(diacraft_doc::Owner::Page(1), 0)),
        diacraft_doc::CellAddr::direct("FillForegnd"),
    );
    let mut ctx = EvalCtx::default();
    let v = Evaluator::new(&doc, &mut ctx).evaluate(&at);
    assert_eq!(mismatch(&v, &c("#0a141e")), None, "{v:?}");
}

#[test]
fn colours_are_written_in_lower_case_without_a_palette_index() {
    let v = eval("RGB(255,0,0)");
    let encoded = diacraft_calc::encode(&v).unwrap();
    assert_eq!(encoded.v, "#ff0000");
    // Read back, a colour is a colour whatever the case.
    assert_eq!(diacraft_calc::decode("#FF0000", None, None), v);
    assert_eq!(diacraft_calc::decode("#ff0000", None, None), v);
}

#[test]
fn the_theme_tables_match_the_spec() {
    // Spot checks of the generated table against the spec's rows (the generator read the same table).
    let check = |scheme: u32, property: &str, want: &str| {
        let setup = move |s: ShapeBuilder| s.row_cell("User", None, RowKey::name("msvThemeColors"), "Value", &scheme.to_string());
        let v = eval_on(setup, &format!("THEME(\"{property}\")"));
        assert_eq!(mismatch(&v, &c(want)), None, "scheme {scheme} {property}: {v:?}");
    };
    check(0, "BackgroundColor", "#ffffff");
    check(0, "AccentColor5", "#f59d56");
    check(22, "BackgroundColor", "#eeece1");
    check(23, "TextColor", "#ffffff");
    check(56, "AccentColor5", "#05e0db");
    check(56, "FillColor", "#31b6fd");
    check(36, "LineColor", "#404040");
}
