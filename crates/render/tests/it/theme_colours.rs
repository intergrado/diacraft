//! A `Themed` marker asks the theme part and draws the answer: fills, gradients, lines, line patterns, text colour and shadows, with
//! pixel assertions at the values a real file holds.
//! A file with no theme part keeps the fallback.
//!
//! The theme part is the composed one of `diacraft_testkit::theme` (accent1 `#204060`, accent2 `#506070`, accent3 `#214365`, scheme 40), made
//! by the reader a package's theme parts go through. Each test says which entry it reads.

use std::sync::Arc;

use diacraft_doc::{Cell, CellAddr, Document, RowKey, Theme, Value};
use diacraft_render::SkipKind;
use diacraft_testkit::build::{MasterBuilder, ShapeBuilder};
use diacraft_testkit::theme::{ThemeXml, effect, font_props, line_style, ln, solid, srgb_clr};

use crate::support::*;

const FIVE: [&str; 5] = ["ThemeIndex", "ColorSchemeIndex", "EffectSchemeIndex", "ConnectorSchemeIndex", "FontSchemeIndex"];

fn marker() -> Cell {
    Cell::with_formula("THEMEVAL()", Some(Value::from_text("Themed")))
}

/// The cells name the scheme `n` (all five) and the shape's own Quick Style cells.
fn on_scheme(mut sb: ShapeBuilder, n: u32, quick: &[(&str, &str)]) -> ShapeBuilder {
    for name in FIVE {
        sb = sb.cell(name, &n.to_string());
    }
    quick.iter().fold(sb, |sb, (name, value)| sb.cell(name, value))
}

fn themed(sb: ShapeBuilder, names: &[&str]) -> ShapeBuilder {
    names.iter().fold(sb, |sb, name| sb.set(CellAddr::direct(name), marker()))
}

/// A page of the given shapes in a document that holds these theme parts.
fn doc_with(shapes: Vec<ShapeBuilder>, themes: Vec<Arc<Theme>>) -> Document {
    let mut doc = one_page(shapes);
    for t in themes {
        doc.themes.push(t);
    }
    doc
}

fn assert_nothing_lost(pic: &Pic) {
    assert_eq!(pic.r.count(SkipKind::Recovered), 0, "the whole frame was lost: {:?}", pic.r.skipped);
    assert_eq!(pic.r.count(SkipKind::ShapeFailed), 0, "a shape was lost: {:?}", pic.r.skipped);
}

#[test]
fn a_themed_fill_draws_the_themes_accent_and_not_the_fallback_blue() {
    // `FillForegnd` holds `THEMEVAL()` and the marker; `QuickStyleFillColor` 2 is `accent1`, `#204060`, and fill entry 1 is a plain `phClr`.
    let sb = themed(on_scheme(rect(1, 1.0, 1.0, 2.0, 2.0), 40, &[("QuickStyleFillColor", "2"), ("QuickStyleFillMatrix", "1")]), &["FillForegnd"]);
    let pic = draw(&doc_with(vec![sb], vec![ThemeXml::default().part()]));
    assert_nothing_lost(&pic);
    assert!(pic.is(2.0, 2.0, (0x20, 0x40, 0x60), 2), "the accent, not #96AFCF: {:?}", pic.at(2.0, 2.0));
    assert_eq!(pic.r.count(SkipKind::ThemeUnanswered), 0, "{:?}", pic.r.skipped);
}

#[test]
fn an_explicit_theme_number_draws_that_theme_and_not_the_first_part() {
    // Two parts: 40 with accent1 `#204060`, and 41 with accent1 `#aa5500`. A shape on 41 draws the second though the first is listed first.
    let second = ThemeXml {
        scheme: 41,
        colours: ["111111", "fefefe", "202020", "dddddd", "aa5500", "506070", "214365", "708090", "a0b0c0", "c0d0e0", "0563c1", "954f72"],
        ..ThemeXml::default()
    };
    let quick = [("QuickStyleFillColor", "2"), ("QuickStyleFillMatrix", "1")];
    let on_41 = themed(on_scheme(rect(1, 0.5, 1.0, 1.5, 2.0), 41, &quick), &["FillForegnd"]);
    let on_40 = themed(on_scheme(rect(2, 2.25, 1.0, 1.5, 2.0), 40, &quick), &["FillForegnd"]);
    let pic = draw(&doc_with(vec![on_41, on_40], vec![ThemeXml::default().part(), second.part()]));
    assert!(pic.is(1.25, 2.0, (0xaa, 0x55, 0x00), 2), "theme 41: {:?}", pic.at(1.25, 2.0));
    assert!(pic.is(3.0, 2.0, (0x20, 0x40, 0x60), 2), "theme 40: {:?}", pic.at(3.0, 2.0));
}

#[test]
fn a_fill_entry_with_a_shade_darkens_the_accent_by_linear_light() {
    // Entry 2 is a solid `phClr` with `shade` 75000: accent1 `#204060` goes to `#1b3754`, and `FillBkgnd` is `#4d4d4d`.
    let quick = [("QuickStyleFillColor", "2"), ("QuickStyleFillMatrix", "2")];
    let sb = themed(on_scheme(rect(1, 1.0, 1.0, 2.0, 2.0), 40, &quick), &["FillForegnd", "FillBkgnd", "FillPattern"]);
    let pic = draw(&doc_with(vec![sb], vec![ThemeXml::default().part()]));
    assert!(pic.is(2.0, 2.0, (0x1b, 0x37, 0x54), 2), "{:?}", pic.at(2.0, 2.0));
}

#[test]
fn a_quick_style_line_colour_draws_the_outline_in_the_theme() {
    // `QuickStyleLineColor` 3 is `accent2`, `#506070`; line entry 1 is a plain `phClr`.
    let quick = [("QuickStyleLineColor", "3"), ("QuickStyleLineMatrix", "1")];
    let sb = themed(
        on_scheme(rect(1, 1.0, 1.0, 2.0, 2.0), 40, &quick).cell("LineWeight", "0.1").cell("LinePattern", "1").cell("FillPattern", "0"),
        &["LineColor"],
    );
    let pic = draw(&doc_with(vec![sb], vec![ThemeXml::default().part()]));
    assert!(pic.is(1.0, 2.0, (0x50, 0x60, 0x70), 2), "the left edge: {:?}", pic.at(1.0, 2.0));
    assert!(pic.is(2.0, 3.0, (0x50, 0x60, 0x70), 2), "the top edge: {:?}", pic.at(2.0, 3.0));
}

#[test]
fn a_themed_line_weight_and_pattern_follow_the_entry_and_the_lineex_one_position_ahead() {
    // The line list's entry 1 is 4 points and `sysDash`; `vt:lineStyles` position 2 holds pattern 9 (3 marks, 3 gaps), position 1 holds 5.
    let x = ThemeXml {
        lines: vec![ln(50_800, "flat", &solid(&srgb_clr("000000", &[])), "sysDash")],
        line_styles: vec![line_style(0, 0, 2, 0, 2, 5), line_style(0, 0, 2, 0, 2, 9)],
        ..ThemeXml::default()
    };
    let quick = [("QuickStyleLineMatrix", "1"), ("QuickStyleLineColor", "0")];
    let sb =
        themed(on_scheme(rect(1, 0.5, 1.0, 3.0, 2.0), 40, &quick).cell("FillPattern", "0"), &["LineColor", "LineWeight", "LinePattern", "LineCap"]);
    let pic = draw(&doc_with(vec![sb], vec![x.part()]));
    // A dashed top edge: several marks along it, and the stroke is the theme's 4 points (0.0556 inch, 5 or 6 pixels) thick.
    let runs = pic.dark_runs_along_x(3.0, 0.5, 3.5);
    assert!(runs.len() >= 5, "a dashed edge has several marks, got {runs:?}");
    let middle = (runs[0].0 as f64 + runs[0].1 as f64 / 2.0) / pic.ppi;
    let thick = (0..12).filter(|i| pic.is_dark(middle, 3.0 + (f64::from(*i) - 6.0) * 0.01)).count();
    assert!((4..=7).contains(&thick), "the stroke is 4 points wide: {thick} pixels");
}

#[test]
fn a_themed_text_colour_draws_the_letters_in_the_theme() {
    // Font entry 1 is a `vt:color` of `#c00000`; the `Color` of the character row holds the marker.
    let x = ThemeXml { fonts: vec![font_props(0, &srgb_clr("c00000", &[]))], ..ThemeXml::default() };
    let quick = [("QuickStyleFontMatrix", "1")];
    let sb = on_scheme(rect(1, 1.0, 1.0, 2.0, 2.0), 40, &quick)
        .cell("FillPattern", "0")
        .cell("LinePattern", "0")
        .text("W")
        .row_cell("Character", None, RowKey::Ix(0), "Size", "1.5")
        .set(CellAddr::in_row("Character", None, RowKey::Ix(0), "Color"), marker());
    let pic = draw(&doc_with(vec![sb], vec![x.part()]));
    let red = (0..200)
        .flat_map(|i| (0..200).map(move |j| (i, j)))
        .filter(|(i, j)| pic.is(1.0 + f64::from(*i) * 0.01, 1.0 + f64::from(*j) * 0.01, (0xc0, 0, 0), 4))
        .count();
    assert!(red > 150, "the letter is `#c00000`: {red} pixels");
}

#[test]
fn a_themed_gradient_runs_from_the_entrys_first_stop_to_its_last() {
    // Fill entry 3 is a gradient of `phClr` tint 50000 at 0 and shade 50000 at 1, at 90 degrees: top to bottom. Of `#204060` those are
    // `#bdc0c5` and `#142c44`. The third row of the stops is past the entry's list and is dropped.
    let quick = [("QuickStyleFillColor", "2"), ("QuickStyleFillMatrix", "3")];
    let mut sb = themed(
        on_scheme(rect(1, 1.0, 1.0, 2.0, 2.0), 40, &quick).cell("LinePattern", "0"),
        &["FillGradientEnabled", "FillGradientDir", "FillGradientAngle"],
    );
    for ix in 0..3 {
        for name in ["GradientStopColor", "GradientStopColorTrans", "GradientStopPosition"] {
            sb = sb.set(CellAddr::in_row("FillGradient", None, RowKey::Ix(ix), name), marker());
        }
    }
    let pic = draw(&doc_with(vec![sb], vec![ThemeXml::default().part()]));
    assert!(pic.is(2.0, 2.95, (0xbd, 0xc0, 0xc5), 12), "the top is the first stop: {:?}", pic.at(2.0, 2.95));
    assert!(pic.is(2.0, 1.05, (0x14, 0x2c, 0x44), 12), "the bottom is the last: {:?}", pic.at(2.0, 1.05));
    assert_eq!(pic.r.count(SkipKind::ThemeUnanswered), 0, "{:?}", pic.r.skipped);
}

#[test]
fn a_themed_shadow_is_drawn_offset_by_the_entrys_distance_and_bearing() {
    // Effect entry 1 is an outer shadow of 0.5 inch at 90 degrees (down), in `#303030`.
    let x = ThemeXml { effects: vec![effect(Some((0, 457_200, 5_400_000, &srgb_clr("303030", &[]))))], ..ThemeXml::default() };
    let quick = [("QuickStyleEffectsMatrix", "1"), ("QuickStyleFillMatrix", "1"), ("QuickStyleFillColor", "2")];
    let sb = themed(
        on_scheme(rect(1, 1.0, 2.0, 1.0, 1.0), 40, &quick).cell("LinePattern", "0").cell("ShapeShdwShow", "2"),
        &[
            "FillForegnd",
            "ShdwPattern",
            "ShdwForegnd",
            "ShdwForegndTrans",
            "ShapeShdwType",
            "ShapeShdwOffsetX",
            "ShapeShdwOffsetY",
            "ShapeShdwBlur",
            "ShapeShdwScaleFactor",
        ],
    );
    let pic = draw(&doc_with(vec![sb], vec![x.part()]));
    assert!(pic.is(1.5, 2.5, (0x20, 0x40, 0x60), 2), "the shape");
    assert!(pic.is(1.5, 1.75, (0x30, 0x30, 0x30), 4), "the shadow below it: {:?}", pic.at(1.5, 1.75));
    assert!(pic.is_white(1.5, 3.5) && pic.is_white(2.5, 2.5), "and nowhere else");
}

#[test]
fn a_file_with_no_theme_part_reads_the_root_style_for_a_marker_and_not_the_table() {
    // Scheme cells above 0 and no part to answer, so `calc` leaves the marker
    // unanswered. A marker stands for a call that names no property, and that gives the root style's cell: the root's white fill (1) and black
    // line (0), where MS-VSDX's column would give `#96AFCF` and `#1F477D`.
    let sb =
        themed(on_scheme(rect(1, 1.0, 1.0, 2.0, 2.0), 40, &[]).cell("LineWeight", "0.1").cell("LinePattern", "1"), &["FillForegnd", "LineColor"]);
    let pic = draw(&doc_with(vec![sb], vec![]));
    assert_nothing_lost(&pic);
    assert!(pic.is_white(2.0, 2.0), "the fill: {:?}", pic.at(2.0, 2.0));
    assert!(pic.is_dark(1.0, 2.0), "the line: {:?}", pic.at(1.0, 2.0));
    assert!(pic.r.count(SkipKind::ThemeUnanswered) >= 2, "{:?}", pic.r.skipped);
}

/// The root style of `base()` with these cells of its own: a fill and a line that no fixed white or black can match.
fn coloured_root() -> diacraft_testkit::build::DocBuilder {
    base().root(|r| r.cell("FillForegnd", "#3C5A78").cell("FillBkgnd", "#A04000").cell("LineColor", "#007F00"))
}

const NONE_SELECTED: [&str; 5] = ["0", "0", "0", "0", "0"];

/// A rectangle on a page that selects no theme (`cells`: the five scheme cells, as numbers), holding the marker in these cells.
fn on_no_theme(cells: &[&str; 5], marked: &[&str]) -> ShapeBuilder {
    let sb = FIVE.iter().zip(cells).fold(rect(1, 1.0, 1.0, 2.0, 2.0), |sb, (name, n)| sb.cell(name, n));
    themed(sb.cell("LineWeight", "0.1").cell("LinePattern", "1"), marked)
}

#[test]
fn a_themed_rectangle_on_a_page_with_no_theme_draws_white_with_a_black_line() {
    // The five cells are 0; the marker fill is the root's white, the marker line its black.
    let pic = draw(&one_page(vec![on_no_theme(&NONE_SELECTED, &["FillForegnd", "LineColor"])]));
    assert_nothing_lost(&pic);
    assert!(pic.is_white(2.0, 2.0), "white, not #96AFCF: {:?}", pic.at(2.0, 2.0));
    assert!(pic.is_dark(1.0, 2.0) && pic.is_dark(3.0, 2.0) && pic.is_dark(2.0, 1.0), "black, not #1F477D: {:?}", pic.at(1.0, 2.0));
    assert!(!pic.is(1.0, 2.0, (0x1F, 0x47, 0x7D), 30), "not the column's blue: {:?}", pic.at(1.0, 2.0));
    assert_eq!(pic.r.count(SkipKind::ThemeUnanswered), 0, "calc answers where no theme is selected: {:?}", pic.r.skipped);
}

#[test]
fn a_themed_cell_with_no_theme_draws_the_roots_own_colour_and_not_a_fixed_white_or_black() {
    // A root `FillForegnd` of `#3C5A78` is what a marker fill paints; a fixed white fails. The same for the line.
    let doc = coloured_root().page(1, page(4.0, 4.0).shape(on_no_theme(&NONE_SELECTED, &["FillForegnd", "LineColor"]))).build();
    let pic = draw(&doc);
    assert!(pic.is(2.0, 2.0, (0x3C, 0x5A, 0x78), 2), "the root's fill: {:?}", pic.at(2.0, 2.0));
    assert!(pic.is(1.0, 2.0, (0x00, 0x7F, 0x00), 2), "the root's line: {:?}", pic.at(1.0, 2.0));
}

#[test]
fn a_page_with_no_theme_cells_at_all_and_one_whose_cells_say_65534_read_the_root_too() {
    // An absent cell is 0, and 65534 over a page that holds none is no theme. With a theme part in the file the
    // first part's colour fails.
    for (id, cells) in [("absent", None), ("65534", Some(["65534"; 5]))] {
        let sb = match cells {
            Some(c) => on_no_theme(&c, &["FillForegnd", "LineColor"]),
            None => themed(rect(1, 1.0, 1.0, 2.0, 2.0).cell("LineWeight", "0.1").cell("LinePattern", "1"), &["FillForegnd", "LineColor"]),
        };
        let mut doc = coloured_root().page(1, page(4.0, 4.0).shape(sb)).build();
        doc.themes.push(ThemeXml::default().part());
        let pic = draw(&doc);
        assert!(pic.is(2.0, 2.0, (0x3C, 0x5A, 0x78), 2), "{id}, the fill: {:?}", pic.at(2.0, 2.0));
        assert!(pic.is(1.0, 2.0, (0x00, 0x7F, 0x00), 2), "{id}, the line: {:?}", pic.at(1.0, 2.0));
    }
}

#[test]
fn a_call_naming_a_property_still_draws_the_tables_colour_with_no_theme() {
    // a named property: `THEMEVAL("FillColor")` is `#96AFCF` and `THEMEVAL("LineColor")` `#1F477D`, whatever
    // the root holds. This is the other half of the rule: only the call naming none moved.
    let sb = FIVE.iter().fold(rect(1, 1.0, 1.0, 2.0, 2.0), |sb, name| sb.cell(name, "0")).cell("LineWeight", "0.1").cell("LinePattern", "1");
    let sb = sb.formula("FillForegnd", "THEMEVAL(\"FillColor\")", "Themed").formula("LineColor", "THEMEVAL(\"LineColor\")", "Themed");
    let doc = coloured_root().page(1, page(4.0, 4.0).shape(sb)).build();
    let pic = draw(&doc);
    assert_nothing_lost(&pic);
    assert!(pic.is(2.0, 2.0, (0x96, 0xAF, 0xCF), 2), "the table's fill: {:?}", pic.at(2.0, 2.0));
    assert!(pic.is(1.0, 2.0, (0x1F, 0x47, 0x7D), 2), "the table's line: {:?}", pic.at(1.0, 2.0));
}

#[test]
fn a_themed_text_colour_with_no_theme_reads_the_roots_character_row() {
    // The marker in a `Character` row's `Color` reads the root style's row, `#C00000` here, where the table's black fails.
    // Two ways to no theme: the five cells 0, which `calc` answers, and cells above 0 with no theme part, which the renderer reads itself.
    for (id, cells) in [("cells 0", "0"), ("no part", "40")] {
        let sb = on_scheme(rect(1, 1.0, 1.0, 2.0, 2.0), cells.parse().unwrap_or(0), &[])
            .cell("FillPattern", "0")
            .cell("LinePattern", "0")
            .text("W")
            .row_cell("Character", None, RowKey::Ix(0), "Size", "1.5")
            .set(CellAddr::in_row("Character", None, RowKey::Ix(0), "Color"), marker());
        let doc = base().root(|r| r.row_cell("Character", None, RowKey::Ix(0), "Color", "#C00000")).page(1, page(4.0, 4.0).shape(sb)).build();
        let pic = draw(&doc);
        let red = (0..200)
            .flat_map(|i| (0..200).map(move |j| (i, j)))
            .filter(|(i, j)| pic.is(1.0 + f64::from(*i) * 0.01, 1.0 + f64::from(*j) * 0.01, (0xc0, 0, 0), 4))
            .count();
        assert!(red > 150, "{id}: the letter is the root's `#c00000`: {red} pixels");
    }
}

#[test]
fn a_themed_back_colour_with_no_theme_reads_the_roots_cell() {
    // `FillBkgnd` holding the marker reads the root's, and the table's `#BFCEE1` fails. Pattern 9 is a flat half share of the
    // foreground over the ground, so the pixel is the mean of the root's two colours, `#3C5A78` and `#A04000`: `#6E4D3C`. The table's two
    // would mean `#ABBED8`.
    let sb = on_no_theme(&NONE_SELECTED, &["FillForegnd", "FillBkgnd"]).cell("FillPattern", "9").cell("LinePattern", "0");
    let doc = coloured_root().page(1, page(4.0, 4.0).shape(sb)).build();
    let pic = draw(&doc);
    assert!(pic.is(2.0, 2.0, (0x6E, 0x4D, 0x3C), 12), "the mean of the root's two colours: {:?}", pic.at(2.0, 2.0));
}

#[test]
fn an_instance_inherits_a_themed_cell_through_inh_and_the_theme_answers_on_the_instance() {
    // The master's shape holds the marker in `FillForegnd` and the instance `Inh`, with the marker as its saved value; the Quick Style cells
    // are the instance's own, so the entry is the instance's choice. Slot 6 is `accent5`, `#a0b0c0`.
    let master = MasterBuilder::new().shape(
        themed(rect(5, 0.0, 0.0, 1.0, 1.0), &["FillForegnd"]).cell("PinX", "0.5").cell("PinY", "0.5").cell("LocPinX", "0.5").cell("LocPinY", "0.5"),
    );
    let inst = |id: u32, x: f64, slot: &str| {
        on_scheme(ShapeBuilder::new(id).master(1), 40, &[("QuickStyleFillColor", slot), ("QuickStyleFillMatrix", "1")])
            .inh("FillForegnd", Some("Themed"))
            .cell("PinX", &x.to_string())
            .cell("PinY", "1.5")
            .cell("Width", "1")
            .cell("Height", "1")
            .cell("LocPinX", "0.5")
            .cell("LocPinY", "0.5")
    };
    let p = page(4.0, 4.0).shape(inst(10, 1.0, "6")).shape(inst(11, 3.0, "2"));
    let mut doc = base().master(1, master).page(1, p).build();
    doc.themes.push(ThemeXml::default().part());
    let pic = draw(&doc);
    assert_nothing_lost(&pic);
    assert!(pic.is(1.0, 1.5, (0xa0, 0xb0, 0xc0), 2), "accent5: {:?}", pic.at(1.0, 1.5));
    assert!(pic.is(3.0, 1.5, (0x20, 0x40, 0x60), 2), "accent1: {:?}", pic.at(3.0, 1.5));
}

#[test]
fn a_page_of_thousands_of_themed_shapes_is_answered_by_the_theme_within_the_frames_steps_and_in_time() {
    // 3,000 shapes, each asking the theme for its fill, outline, pattern, weight and shadow: the answers are kept per cell, the steps of the
    // frame hold them all, and nothing is left unanswered for want of budget.
    let quick = [
        ("QuickStyleFillColor", "2"),
        ("QuickStyleFillMatrix", "2"),
        ("QuickStyleLineColor", "3"),
        ("QuickStyleLineMatrix", "1"),
        ("QuickStyleEffectsMatrix", "1"),
    ];
    let shapes: Vec<ShapeBuilder> = (0..3_000u32)
        .map(|i| {
            let (col, row) = (f64::from(i % 60), f64::from(i / 60));
            let sb = rect(i + 1, 0.1 + col * 0.06, 0.1 + row * 0.07, 0.05, 0.05);
            themed(on_scheme(sb, 40, &quick), &["FillForegnd", "FillPattern", "LineColor", "LinePattern", "LineWeight", "ShdwPattern"])
        })
        .collect();
    let doc = doc_with(shapes, vec![ThemeXml::default().part()]);
    let started = std::time::Instant::now();
    let pic = draw(&doc);
    let took = started.elapsed();
    eprintln!("[timing] 3,000 themed shapes drawn in {took:?}");
    assert_nothing_lost(&pic);
    assert_eq!(pic.r.count(SkipKind::ThemeUnanswered), 0, "{:?}", pic.r.skipped);
    // A hang guard and no measure of cost: nothing is left unanswered (above), and a minute is generous for a machine at load 50.
    assert!(took < std::time::Duration::from_secs(60), "took {took:?}");
    assert!(pic.is(0.125, 0.125, (0x1b, 0x37, 0x54), 3), "the first shape: {:?}", pic.at(0.125, 0.125));
}

#[test]
fn a_theme_that_holds_the_entry_but_a_matrix_of_zero_reads_the_roots_cell() {
    // Matrix 0 takes the root style's cell: here the root's white fill. The theme is not asked for a colour.
    let sb = themed(on_scheme(rect(1, 1.0, 1.0, 2.0, 2.0), 40, &[("QuickStyleFillColor", "2"), ("QuickStyleFillMatrix", "0")]), &["FillForegnd"]);
    let pic = draw(&doc_with(vec![sb], vec![ThemeXml::default().part()]));
    assert!(pic.is(2.0, 2.0, (255, 255, 255), 2), "{:?}", pic.at(2.0, 2.0));
}
