//! `THEMEVAL` with no theme selected: a call that names no property, which is what every cell holding the marker `Themed` stands for,
//! gives the root style's cell of the host's name; a call that names one gives MS-VSDX's no-theme table. ,
//! With the vectors.

use diacraft_calc::{Budget, EvalCtx, EvalOptions, Evaluator, commit};
use diacraft_doc::{CellAddr, Document, Lookup, Owner, RowKey, ShapeRef, SheetRef};
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};
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
