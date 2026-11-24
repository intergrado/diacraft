//! Hostile input: a drawing is as hostile as the file it came from. Each test is crafted and timed (ten seconds is generous), and checks that
//! the export ends with a PDF that a reader opens, or with an error, never a panic.

use std::time::{Duration, Instant};

use diacraft_doc::{Document, RowKey};
use diacraft_pdf::{PdfOptions, export_page};
use diacraft_render::{Renderer, SkipKind};
use diacraft_testkit::build::ShapeBuilder;

use crate::readback::{Seen, read};
use crate::support::*;

/// A hang guard and no measure of cost: an export of these pages takes a second or two at most on a busy machine, so a minute tells a hang from
/// a slow machine at load 50. What each test claims about the work (every shape in the file, what was left out and said so) is counted.
const BOUND: Duration = Duration::from_secs(60);

/// Exports page 1 and reads the file back with an independent reader; returns what it saw and the list of what was left out.
fn exported(doc: &Document) -> (Seen, Vec<diacraft_render::Skipped>) {
    let started = Instant::now();
    let out = export_page(&mut Renderer::new(), doc, 1, &PdfOptions::default()).expect("exports");
    assert!(started.elapsed() < BOUND, "took {:?}", started.elapsed());
    (read(&out.bytes), out.skipped)
}

#[test]
fn many_shapes_are_all_in_the_file_and_it_is_quick() {
    let shapes: Vec<ShapeBuilder> = (0..20_000u32)
        .map(|i| rect(i + 1, f64::from(i % 150) * 0.026, f64::from(i / 150) * 0.019, 0.02, 0.015).cell("FillForegnd", "#FF0000"))
        .collect();
    let (seen, skipped) = exported(&one_page(shapes));
    assert_eq!(seen.with_colour([255, 0, 0]).len(), 20_000);
    assert!(skipped.is_empty(), "{skipped:?}");
}

#[test]
fn coordinates_that_are_huge_or_not_numbers_make_a_file_a_reader_opens() {
    for value in ["1e300", "-1e300", "1.#INF", "-1.#INF", "NaN", "1e15", "9e18"] {
        let sb = rect(1, 1.0, 1.0, 2.0, 2.0)
            .cell("PinX", value)
            .cell("Width", value)
            .cell("LocPinX", value)
            .cell("Angle", value)
            .cell("FillForegnd", "#FF0000");
        exported(&one_page(vec![sb]));
    }
    // Huge geometry on a sane shape: the segments are cut to the page's neighbourhood and every number stays small.
    let sb = geometry_of(
        shape_at(1, 1.0, 1.0, 2.0, 2.0),
        vec![("MoveTo", vec![("X", 0.0), ("Y", 0.0)]), ("LineTo", vec![("X", 1.0e9), ("Y", 1.0e9)]), ("LineTo", vec![("X", 0.0), ("Y", 1.0e9)])],
    )
    .cell("FillForegnd", "#FF0000");
    let (seen, _) = exported(&one_page(vec![sb]));
    for p in seen.with_colour([255, 0, 0]) {
        assert!(p.bbox.x1.abs() < 1.0e7 && p.bbox.y1.abs() < 1.0e7, "{:?}", p.bbox);
    }
}

#[test]
fn a_huge_line_weight_is_clamped_and_a_fine_dash_on_a_long_line_is_drawn_solid() {
    let heavy = rect(1, 1.0, 1.0, 2.0, 2.0).cell("LineWeight", "1e12").cell("LineColor", "#000000");
    let (_, skipped) = exported(&one_page(vec![heavy]));
    assert!(skipped.iter().any(|s| s.kind == SkipKind::StrokeClamped), "{skipped:?}");
    let fine =
        geometry_of(shape_at(1, 0.0, 2.0, 5.0e4, 0.0), vec![("MoveTo", vec![("X", 0.0), ("Y", 0.0)]), ("LineTo", vec![("X", 5.0e4), ("Y", 0.0)])])
            .cell("FillPattern", "0")
            .cell("LinePattern", "2")
            .cell("LineWeight", "0.001")
            .cell("LineColor", "#112233");
    let (seen, skipped) = exported(&one_page(vec![fine]));
    assert!(skipped.iter().any(|s| s.kind == SkipKind::DashesTooMany), "{skipped:?}");
    assert!(seen.with_colour([0x11, 0x22, 0x33]).iter().all(|p| p.dashes.is_empty()));
}

#[test]
fn a_block_of_a_hundred_thousand_letters_is_cut_where_the_renderer_cuts_it_and_the_file_stays_small() {
    let long = "The quick brown fox jumps over the lazy dog. ".repeat(2_300);
    assert!(long.chars().count() > 100_000);
    let sb = rect(1, 0.5, 0.5, 3.0, 3.0).cell("FillPattern", "0").cell("LinePattern", "0").text(&long).row_cell(
        "Character",
        None,
        RowKey::Ix(0),
        "Size",
        "0.05",
    );
    let started = Instant::now();
    let out = export_page(&mut Renderer::new(), &one_page(vec![sb]), 1, &PdfOptions::default()).expect("exports");
    assert!(started.elapsed() < BOUND, "took {:?}", started.elapsed());
    assert!(out.skipped.iter().any(|s| s.kind == SkipKind::TextNote), "the cut is said: {:?}", out.skipped);
    assert!(out.bytes.len() < 8 * 1024 * 1024, "{} bytes", out.bytes.len());
    let seen = read(&out.bytes);
    assert!(!seen.glyphs.is_empty() && seen.glyphs.len() <= 20_000, "{}", seen.glyphs.len());
}

#[test]
fn a_thousand_labels_are_a_thousand_runs_of_text() {
    let shapes: Vec<ShapeBuilder> = (0..1_000u32)
        .map(|i| {
            rect(i + 1, f64::from(i % 20) * 0.19, f64::from(i / 20) * 0.07, 0.18, 0.06)
                .cell("FillPattern", "0")
                .cell("LinePattern", "0")
                .text(&format!("L{i}"))
                .row_cell("Character", None, RowKey::Ix(0), "Size", "0.04")
        })
        .collect();
    let (seen, skipped) = exported(&one_page(shapes));
    let expected: usize = (0..1_000).map(|i| format!("L{i}").chars().count()).sum();
    assert_eq!(seen.glyphs.len(), expected, "{skipped:?}");
}

#[test]
fn text_with_characters_no_font_has_still_makes_a_file_a_reader_opens() {
    let nasty = "a\u{0}b\u{1}c\u{8}d\u{B}e\u{1F}f\u{FFFE}g\u{FFFF}h <![CDATA[ ]]> &amp; \u{202E}x\u{200B}y \u{1F600} \u{10FFFF}";
    let sb = rect(1, 0.5, 0.5, 3.0, 1.0).cell("FillPattern", "0").cell("LinePattern", "0").text(nasty).row_cell(
        "Character",
        None,
        RowKey::Ix(0),
        "Size",
        "0.2",
    );
    exported(&one_page(vec![sb]));
}

#[test]
fn groups_nested_to_the_limit_and_past_it_end_in_a_file() {
    let nest = |depth: usize| {
        let mut shape = rect(1, 1.0, 1.0, 1.0, 1.0).cell("FillForegnd", "#FF0000");
        for level in 0..depth {
            shape = shape_at(2 + level as u32, 0.0, 0.0, 4.0, 4.0).group().child(shape);
        }
        shape
    };
    let (seen, _) = exported(&one_page(vec![nest(150)]));
    assert_eq!(seen.with_colour([255, 0, 0]).len(), 1);
    exported(&one_page(vec![nest(260)]));
}

#[test]
fn four_hundred_and_fifty_blurred_shadows_are_all_drawn_hard_edged_and_said_so() {
    let shapes: Vec<ShapeBuilder> = (0..450u32)
        .map(|i| {
            rect(i + 1, f64::from(i % 30) * 0.12, f64::from(i / 30) * 0.2, 0.08, 0.08)
                .cell("FillForegnd", "#FF0000")
                .cell("ShdwPattern", "1")
                .cell("ShapeShdwType", "1")
                .cell("ShapeShdwOffsetX", "0.02")
                .cell("ShapeShdwOffsetY", "-0.02")
                .cell("ShdwForegnd", "#808080")
                .cell("ShapeShdwBlur", "0.03")
        })
        .collect();
    let (seen, skipped) = exported(&one_page(shapes));
    assert_eq!(seen.with_colour([128, 128, 128]).iter().filter(|p| !p.stroked).count(), 450);
    let said = skipped.iter().find(|s| s.kind == SkipKind::ShadowApproximate).expect("said so");
    assert_eq!(said.count, 450);
}

#[test]
fn a_page_of_no_size_and_one_past_the_cap_are_errors_and_not_files() {
    let sizeless = base().page(1, page(0.0, 4.0)).build();
    assert!(export_page(&mut Renderer::new(), &sizeless, 1, &PdfOptions::default()).is_err());
    let huge = base().page(1, page(1.0e9, 1.0e9)).build();
    assert!(export_page(&mut Renderer::new(), &huge, 1, &PdfOptions::default()).is_err());
    let nan = base().page(1, page(f64::NAN, 4.0)).build();
    assert!(export_page(&mut Renderer::new(), &nan, 1, &PdfOptions::default()).is_err());
    // A page at the cap's edge is a file: 13,000 inches is 936,000 points.
    let wide = base().page(1, page(13_000.0, 1.0)).build();
    assert!(export_page(&mut Renderer::new(), &wide, 1, &PdfOptions::default()).is_ok());
}

/// Cells of a shape that the drawing reads, and the values a hostile file may put in them.
const SHAPE_CELLS: [&str; 34] = [
    "PinX",
    "PinY",
    "Width",
    "Height",
    "LocPinX",
    "LocPinY",
    "Angle",
    "FlipX",
    "FlipY",
    "LineWeight",
    "LineColor",
    "LinePattern",
    "LineCap",
    "FillForegnd",
    "FillBkgnd",
    "FillPattern",
    "FillForegndTrans",
    "FillGradientEnabled",
    "FillGradientDir",
    "FillGradientAngle",
    "ShdwPattern",
    "ShapeShdwType",
    "ShapeShdwOffsetX",
    "ShapeShdwBlur",
    "ShapeShdwScaleFactor",
    "Rounding",
    "BeginArrow",
    "EndArrow",
    "TxtWidth",
    "TxtAngle",
    "TxtPinX",
    "LeftMargin",
    "VerticalAlign",
    "TextDirection",
];
const CHARACTER_CELLS: [&str; 8] = ["Size", "Color", "FontScale", "Style", "Font", "Letterspace", "Case", "Pos"];
const JUNK: [&str; 14] = ["", "x", "-1", "0", "1e308", "-1e308", "NaN", "1.#INF", "#zzzzzz", "9e18", "0.000000001", "1e-300", "65535", "-0"];

#[test]
fn a_hostile_value_in_any_cell_the_drawing_reads_still_makes_a_file_a_reader_opens() {
    // The sweep draws something: with a harmless value the text is there.
    let baseline = rect(1, 1.0, 1.0, 2.0, 1.0).text("Hi there").row_cell("Character", None, RowKey::Ix(0), "Size", "0.2");
    assert_eq!(exported(&one_page(vec![baseline])).0.text(), "Hi there");
    let started = Instant::now();
    for cell in SHAPE_CELLS {
        for junk in JUNK {
            let sb = rect(1, 1.0, 1.0, 2.0, 1.0).cell("FillForegnd", "#FF0000").cell(cell, junk).text("Hi there").row_cell(
                "Character",
                None,
                RowKey::Ix(0),
                "Size",
                "0.2",
            );
            exported(&one_page(vec![sb]));
        }
    }
    for cell in CHARACTER_CELLS {
        for junk in JUNK {
            let sb = rect(1, 1.0, 1.0, 2.0, 1.0).text("Hi there").row_cell("Character", None, RowKey::Ix(0), cell, junk);
            exported(&one_page(vec![sb]));
        }
    }
    // A hang guard over the whole sweep, not a measure of cost: each export is bounded by `BOUND` as well, and the sweep takes under a second.
    assert!(started.elapsed() < Duration::from_secs(60), "took {:?}", started.elapsed());
}
