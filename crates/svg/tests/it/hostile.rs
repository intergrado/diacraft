//! Hostile input: a drawing is as hostile as the file it came from. Each test is crafted and timed (ten seconds is generous), and checks that
//! the export ends with a well-formed file or an error, never a panic and never a number that is not one.

use std::time::{Duration, Instant};

use diacraft_doc::{Document, RowKey};
use diacraft_render::{Renderer, SkipKind};
use diacraft_svg::{SvgOptions, export_page};
use diacraft_testkit::build::ShapeBuilder;

use crate::support::*;

/// A hang guard and no measure of cost: an export of these pages takes a second or two at most on a busy machine, so a minute tells a hang from
/// a slow machine at load 50. What each test claims about the work (every shape in the file, what was left out and said so) is counted.
const BOUND: Duration = Duration::from_secs(60);

/// Exports page 1 and checks the file is well formed and holds no number that is not one; returns the file and the list of what was left out.
fn exported(doc: &Document) -> (String, Vec<diacraft_render::Skipped>) {
    let started = Instant::now();
    let out = export_page(&mut Renderer::new(), doc, 1, &SvgOptions::default()).expect("exports");
    assert!(started.elapsed() < BOUND, "took {:?}", started.elapsed());
    diacraft_opc::xml::parse(&out.bytes).expect("a well-formed file");
    let text = String::from_utf8(out.bytes).expect("UTF-8");
    for bad in ["NaN", "inf"] {
        assert!(!text.contains(bad), "`{bad}` in the file");
    }
    // No number written with an exponent: a digit, then `e`, then a digit or a sign and a digit.
    let bytes = text.as_bytes();
    for (i, w) in bytes.windows(3).enumerate() {
        let exponent = w[0].is_ascii_digit()
            && w[1] == b'e'
            && (w[2].is_ascii_digit() || (matches!(w[2], b'+' | b'-') && bytes.get(i + 3).is_some_and(u8::is_ascii_digit)));
        assert!(!exponent, "a number with an exponent: {}", &text[i.saturating_sub(10)..(i + 12).min(text.len())]);
    }
    (text, out.skipped)
}

#[test]
fn many_shapes_are_all_in_the_file_and_it_is_quick() {
    let shapes: Vec<ShapeBuilder> = (0..20_000u32)
        .map(|i| rect(i + 1, f64::from(i % 150) * 0.026, f64::from(i / 150) * 0.019, 0.02, 0.015).cell("FillForegnd", "#FF0000"))
        .collect();
    let (text, skipped) = exported(&one_page(shapes));
    assert_eq!(text.matches("fill=\"#ff0000\"").count(), 20_000);
    assert!(skipped.is_empty(), "{skipped:?}");
}

#[test]
fn an_export_uses_the_limits_of_an_export_and_puts_the_renderers_own_back() {
    let mut renderer = Renderer::new();
    let shapes: Vec<ShapeBuilder> = (0..2_000u32).map(|i| rect(i + 1, f64::from(i % 50) * 0.07, f64::from(i / 50) * 0.09, 0.05, 0.05)).collect();
    let doc = one_page(shapes);
    // The renderer's own limits are the window's; an export has limits of its own (`export_limits`), and puts the caller's back afterwards.
    renderer.set_limits(diacraft_render::Limits { max_shapes: 100, ..diacraft_render::Limits::default() });
    let out = export_page(&mut renderer, &doc, 1, &SvgOptions::default()).expect("exports");
    // The export's limits stand in for the renderer's during the export, so all 2,000 are drawn.
    assert_eq!(out.count(SkipKind::ShapeLimit), 0, "{:?}", out.skipped);
    assert_eq!(renderer.limits().max_shapes, 100);
}

#[test]
fn coordinates_that_are_huge_or_not_numbers_make_a_file_with_no_such_number_in_it() {
    for value in ["1e300", "-1e300", "1.#INF", "-1.#INF", "NaN", "1e15", "9e18"] {
        let sb = rect(1, 1.0, 1.0, 2.0, 2.0)
            .cell("PinX", value)
            .cell("Width", value)
            .cell("LocPinX", value)
            .cell("Angle", value)
            .cell("FillForegnd", "#FF0000");
        exported(&one_page(vec![sb]));
    }
    // Huge geometry on a sane shape: the segment is cut to the page's neighbourhood and its numbers stay small.
    let sb = geometry_of(
        shape_at(1, 1.0, 1.0, 2.0, 2.0),
        vec![("MoveTo", vec![("X", 0.0), ("Y", 0.0)]), ("LineTo", vec![("X", 1.0e9), ("Y", 1.0e9)]), ("LineTo", vec![("X", 0.0), ("Y", 1.0e9)])],
    )
    .cell("FillForegnd", "#FF0000");
    let (text, _) = exported(&one_page(vec![sb]));
    for number in numbers(&text.replace(|c: char| c.is_alphabetic() && c != '.', " ")) {
        assert!(number.abs() < 1.0e7, "{number}");
    }
}

#[test]
fn a_huge_line_weight_is_clamped_and_a_fine_dash_on_a_long_line_is_drawn_solid() {
    let heavy = rect(1, 1.0, 1.0, 2.0, 2.0).cell("LineWeight", "1e12").cell("LineColor", "#000000");
    let (text, skipped) = exported(&one_page(vec![heavy]));
    assert!(skipped.iter().any(|s| s.kind == SkipKind::StrokeClamped), "{skipped:?}");
    assert!(numbers(&text).iter().all(|n| n.abs() < 1.0e7));
    let fine =
        geometry_of(shape_at(1, 0.0, 2.0, 5.0e4, 0.0), vec![("MoveTo", vec![("X", 0.0), ("Y", 0.0)]), ("LineTo", vec![("X", 5.0e4), ("Y", 0.0)])])
            .cell("FillPattern", "0")
            .cell("LinePattern", "2")
            .cell("LineWeight", "0.001");
    let (text, skipped) = exported(&one_page(vec![fine]));
    assert!(skipped.iter().any(|s| s.kind == SkipKind::DashesTooMany), "{skipped:?}");
    assert!(!text.contains("stroke-dasharray"));
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
    let (text, skipped) = exported(&one_page(vec![sb]));
    assert!(skipped.iter().any(|s| s.kind == SkipKind::TextNote), "the cut is said: {skipped:?}");
    assert!(text.len() < 8 * 1024 * 1024, "{} bytes", text.len());
}

#[test]
fn a_thousand_labels_are_a_thousand_text_elements() {
    let shapes: Vec<ShapeBuilder> = (0..1_000u32)
        .map(|i| {
            rect(i + 1, f64::from(i % 20) * 0.19, f64::from(i / 20) * 0.07, 0.18, 0.06)
                .cell("FillPattern", "0")
                .cell("LinePattern", "0")
                .text(&format!("L{i}"))
                .row_cell("Character", None, RowKey::Ix(0), "Size", "0.04")
        })
        .collect();
    let (text, skipped) = exported(&one_page(shapes));
    assert_eq!(text.matches("<text ").count(), 1_000, "{skipped:?}");
}

#[test]
fn text_with_characters_xml_forbids_still_makes_a_well_formed_file() {
    let nasty = "a\u{0}b\u{1}c\u{8}d\u{B}e\u{1F}f\u{FFFE}g\u{FFFF}h <![CDATA[ ]]> &amp; \u{202E}x\u{200B}y";
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
    let (text, _) = exported(&one_page(vec![nest(150)]));
    assert_eq!(text.matches("fill=\"#ff0000\"").count(), 1);
    exported(&one_page(vec![nest(260)]));
}

#[test]
fn blurred_shadows_by_the_hundred_are_each_a_filter_and_the_rest_are_hard_edged() {
    let shapes: Vec<ShapeBuilder> = (0..450u32)
        .map(|i| {
            rect(i + 1, f64::from(i % 30) * 0.12, f64::from(i / 30) * 0.2, 0.08, 0.08)
                .cell("FillForegnd", "#FF0000")
                .cell("ShdwPattern", "1")
                .cell("ShapeShdwType", "1")
                .cell("ShapeShdwOffsetX", "0.02")
                .cell("ShapeShdwOffsetY", "-0.02")
                .cell("ShapeShdwBlur", "0.03")
        })
        .collect();
    let (text, skipped) = exported(&one_page(shapes));
    assert_eq!(text.matches("<filter ").count(), 400, "at most 400 blurs a page");
    assert!(skipped.iter().any(|s| s.kind == SkipKind::ShadowApproximate), "{skipped:?}");
}

#[test]
fn a_page_of_no_size_and_one_past_the_cap_are_errors_and_not_files() {
    let sizeless = base().page(1, page(0.0, 4.0)).build();
    assert!(export_page(&mut Renderer::new(), &sizeless, 1, &SvgOptions::default()).is_err());
    let huge = base().page(1, page(1.0e9, 1.0e9)).build();
    assert!(export_page(&mut Renderer::new(), &huge, 1, &SvgOptions::default()).is_err());
    let nan = base().page(1, page(f64::NAN, 4.0)).build();
    assert!(export_page(&mut Renderer::new(), &nan, 1, &SvgOptions::default()).is_err());
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
fn a_hostile_value_in_any_cell_the_drawing_reads_still_makes_a_well_formed_file() {
    // The sweep draws something: with a harmless value the text is there.
    let baseline = rect(1, 1.0, 1.0, 2.0, 1.0).text("Hi there").row_cell("Character", None, RowKey::Ix(0), "Size", "0.2");
    assert!(exported(&one_page(vec![baseline])).0.contains(">Hi there</text>"));
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

/// A square of `size` inches at the page's corner with a grey shadow of `blur` inches of blur, offset by `offset`.
fn blurred(id: u32, size: f64, offset: f64, blur: &str) -> ShapeBuilder {
    rect(id, 0.05, 0.05, size, size)
        .cell("FillForegnd", "#FF0000")
        .cell("LinePattern", "0")
        .cell("ShdwPattern", "1")
        .cell("ShapeShdwType", "1")
        .cell("ShapeShdwOffsetX", &offset.to_string())
        .cell("ShapeShdwOffsetY", &(-offset).to_string())
        .cell("ShdwForegnd", "#808080")
        .cell("ShapeShdwBlur", blur)
}

#[test]
fn a_huge_blur_is_cut_to_the_same_reach_the_window_cuts_it_to_and_the_file_says_so() {
    // 20 inches of blur is a deviation of 13.3 inches (1,280 pixels). The walk cuts a blur's reach to MAX_BLUR_SPREAD_PX (320 device pixels, three
    // deviations): a deviation of 320 / 3 = 106.67 pixels at 96 to the inch, which the filter carries.
    let (text, skipped) = exported(&one_page(vec![blurred(1, 1.0, 0.3, "20")]));
    let root = diacraft_opc::xml::parse(text.as_bytes()).unwrap();
    let deviations: Vec<f64> = named(&root, "feGaussianBlur").iter().map(|b| attr(b, "stdDeviation").parse().unwrap()).collect();
    assert_eq!(deviations.len(), 1);
    assert!((deviations[0] - 320.0 / 3.0).abs() < 0.01, "{deviations:?}");
    let cut = skipped.iter().find(|s| s.kind == SkipKind::ShadowApproximate).expect("said so");
    assert!(cut.first.contains("to bound its cost"), "{cut:?}");
    // An infinite blur is cut the same way.
    let (text, _) = exported(&one_page(vec![blurred(1, 1.0, 0.3, "1.#INF")]));
    let root = diacraft_opc::xml::parse(text.as_bytes()).unwrap();
    let wide: f64 = attr(named(&root, "feGaussianBlur")[0], "stdDeviation").parse().unwrap();
    assert!(wide <= 320.0 / 3.0 + 0.01, "{wide}");
}

#[test]
fn blurred_shadows_as_large_as_the_page_are_counted_together_and_the_rest_are_hard_edged() {
    // Each shadow is the page's size (374 pixels) and its fade (3 deviations of 0.2 inch: 58 pixels) and the rasteriser's tiles on every
    // side, 256,000 pixels or so: 32 Mi pixels a frame (`MAX_BLUR_PIXELS_PER_FRAME`) is 131 of them, whatever the target draws them with.
    let shapes: Vec<ShapeBuilder> = (0..200u32).map(|i| blurred(i + 1, 3.9, 0.0, "0.3")).collect();
    let (text, skipped) = exported(&one_page(shapes));
    let filters = text.matches("<filter ").count();
    assert!((125..=135).contains(&filters), "{filters} filters");
    let hard = skipped.iter().find(|s| s.kind == SkipKind::ShadowApproximate).expect("said so");
    assert_eq!(hard.count, 200 - filters, "{skipped:?}");
}
