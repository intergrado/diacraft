//! The SVG a page becomes, read back with an XML parser that knows nothing of the writer and checked by value: where the paths are in pixels,
//! what they are painted with, what the gradients and clips hold, and what the text says and where it stands.
//!
//! Pages here are 4 by 4 inches, so at the default 96 pixels to the inch the file is 384 by 384 and paper inch `(x, y)` (y up) is at pixel
//! `(96 x, 384 - 96 y)`.

use std::sync::Arc;

use diacraft_doc::{ForeignData, Media, RawAttrs, RowKey};
use diacraft_opc::xml::El;
use diacraft_render::{Renderer, SkipKind};
use diacraft_svg::{SvgOptions, TextMode, export_page};
use diacraft_testkit::build::ShapeBuilder;

use crate::support::*;

fn defaults() -> SvgOptions {
    SvgOptions::default()
}

/// The paths of the file that are painted with `fill` (a `fill` attribute equal to it), in order.
fn filled<'a>(root: &'a El, fill: &str) -> Vec<&'a El> {
    named(root, "path").into_iter().filter(|p| p.attr("fill") == Some(fill)).collect()
}

fn red_box() -> ShapeBuilder {
    rect(1, 1.0, 1.0, 2.0, 1.0).cell("FillForegnd", "#FF0000").cell("LinePattern", "0")
}

#[test]
fn the_file_is_the_page_in_pixels_with_a_view_box_of_the_same_size() {
    let (out, root) = export(&one_page(vec![]), &defaults());
    assert_eq!(root.name, "svg");
    assert_eq!(attr(&root, "width"), "384");
    assert_eq!(attr(&root, "height"), "384");
    assert_eq!(attr(&root, "viewBox"), "0 0 384 384");
    assert_eq!((out.width, out.height), (384, 384));
    // The paper is the first thing drawn, white, over the whole page.
    let paper = filled(&root, "#ffffff");
    assert_eq!(paper.len(), 1);
    assert_eq!(bbox(attr(paper[0], "d")), (0.0, 0.0, 384.0, 384.0));
    let text = String::from_utf8(out.bytes).expect("UTF-8");
    assert!(text.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
}

#[test]
fn the_size_follows_the_pixels_to_the_inch_asked_for() {
    let (out, root) = export(&one_page(vec![]), &SvgOptions { px_per_inch: 50.0, ..defaults() });
    assert_eq!((attr(&root, "width"), attr(&root, "height")), ("200", "200"));
    assert_eq!((out.width, out.height), (200, 200));
}

#[test]
fn a_filled_rectangle_is_a_path_at_its_place_in_pixels_with_y_down() {
    let (_, root) = export(&one_page(vec![red_box()]), &defaults());
    let boxes = filled(&root, "#ff0000");
    assert_eq!(boxes.len(), 1);
    // Paper [1, 3] x [1, 2] is pixels x 96 to 288, y 384 - 192 = 192 to 384 - 96 = 288.
    let (x0, y0, x1, y1) = bbox(attr(boxes[0], "d"));
    assert!(close(x0, 96.0) && close(x1, 288.0) && close(y0, 192.0) && close(y1, 288.0), "{:?}", (x0, y0, x1, y1));
    assert_eq!(boxes[0].attr("fill-rule"), Some("evenodd"), "filled by the odd-crossings rule, as the window fills");
    assert!(attr(boxes[0], "d").ends_with('Z'), "the ring is closed");
    assert_eq!(boxes[0].attr("fill-opacity"), None, "an opaque colour needs no opacity");
}

#[test]
fn a_transparent_fill_says_its_opacity_and_a_fully_transparent_one_writes_nothing() {
    let half = red_box().cell("FillForegndTrans", "0.5");
    let (_, root) = export(&one_page(vec![half]), &defaults());
    let boxes = filled(&root, "#ff0000");
    assert_eq!(boxes.len(), 1);
    let o: f64 = attr(boxes[0], "fill-opacity").parse().expect("a number");
    assert!((o - 0.5).abs() < 0.01, "{o}");
    let clear = red_box().cell("FillForegndTrans", "1");
    let (_, root) = export(&one_page(vec![clear]), &defaults());
    assert!(filled(&root, "#ff0000").is_empty());
}

#[test]
fn the_outline_is_a_stroked_path_as_wide_as_the_line_weight_in_pixels() {
    let sb = red_box().cell("LinePattern", "1").cell("LineColor", "#0000FF").cell("LineWeight", "0.05");
    let (_, root) = export(&one_page(vec![sb]), &defaults());
    let lines: Vec<_> = named(&root, "path").into_iter().filter(|p| p.attr("stroke") == Some("#0000ff")).collect();
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].attr("fill"), Some("none"));
    let width: f64 = attr(lines[0], "stroke-width").parse().expect("a number");
    assert!((width - 4.8).abs() < 1e-3, "0.05 inch is 4.8 pixels: {width}");
    assert_eq!(lines[0].attr("stroke-dasharray"), None, "a solid line has no dashes");
    assert!(lines[0].attr("stroke-linecap").is_some() && lines[0].attr("stroke-linejoin").is_some());
}

/// A horizontal line 3 inches long at paper height 2, black-ish green, of this weight (inches) and pattern.
fn thin_line(weight: f64, pattern: u32) -> ShapeBuilder {
    geometry_of(shape_at(1, 0.5, 2.0, 3.0, 0.0), vec![("MoveTo", vec![("X", 0.0), ("Y", 0.0)]), ("LineTo", vec![("X", 3.0), ("Y", 0.0)])])
        .cell("FillPattern", "0")
        .cell("LinePattern", &pattern.to_string())
        .cell("LineWeight", &weight.to_string())
        .cell("LineColor", "#008000")
}

#[test]
fn a_line_thinner_than_a_pixel_is_written_at_its_true_width_for_the_reader_to_draw() {
    // The window and the PNG draw a line under a pixel one pixel wide because they rasterise. An SVG is rasterised by
    // its reader at a resolution of its own, so it holds the weight: 0.24 points is 0.32 CSS pixel, and a pattern is in weights of that.
    let weight = 0.24 / 72.0;
    let (_, root) = export(&one_page(vec![thin_line(weight, 1)]), &defaults());
    let line = named(&root, "path").into_iter().find(|p| p.attr("stroke") == Some("#008000")).expect("the line");
    let width: f64 = attr(line, "stroke-width").parse().expect("a number");
    assert!((width - 0.32).abs() < 1e-3, "0.24 points are 0.32 pixels: {width}");
    let (_, root) = export(&one_page(vec![thin_line(weight, 2)]), &defaults());
    let line = named(&root, "path").into_iter().find(|p| p.attr("stroke") == Some("#008000")).expect("the line");
    let dashes = numbers(attr(line, "stroke-dasharray"));
    assert!(dashes.len() == 2 && (dashes[0] - 7.0 * 0.32).abs() < 2e-3 && (dashes[1] - 5.0 * 0.32).abs() < 2e-3, "7 and 5 weights: {dashes:?}");
}

#[test]
fn a_weight_of_zero_is_a_quarter_point_in_a_file() {
    // One device pixel on a screen, and 0.25 points in a vector export.
    let (_, root) = export(&one_page(vec![thin_line(0.0, 1)]), &defaults());
    let line = named(&root, "path").into_iter().find(|p| p.attr("stroke") == Some("#008000")).expect("the line");
    let width: f64 = attr(line, "stroke-width").parse().expect("a number");
    assert!((width - 0.25 * 96.0 / 72.0).abs() < 1e-3, "{width}");
}

#[test]
fn a_dashed_line_carries_its_dashes_in_pixels_and_a_cap() {
    let sb = geometry_of(shape_at(1, 0.5, 2.0, 3.0, 0.0), vec![("MoveTo", vec![("X", 0.0), ("Y", 0.0)]), ("LineTo", vec![("X", 3.0), ("Y", 0.0)])])
        .cell("FillPattern", "0")
        .cell("LinePattern", "2")
        .cell("LineWeight", "0.02")
        .cell("LineColor", "#008000");
    let (_, root) = export(&one_page(vec![sb]), &defaults());
    let line = named(&root, "path").into_iter().find(|p| p.attr("stroke") == Some("#008000")).expect("the line");
    let dashes = numbers(attr(line, "stroke-dasharray"));
    assert!(dashes.len() >= 2 && dashes.iter().all(|d| *d > 0.0), "{dashes:?}");
    // A dash and its gap are a few line weights long (1.92 pixels), not paper inches: far more than a pixel and far less than the line.
    assert!(dashes.iter().all(|d| *d < 288.0), "{dashes:?}");
    let (x0, y0, x1, y1) = bbox(attr(line, "d"));
    assert!(close(x0, 48.0) && close(x1, 336.0) && close(y0, 192.0) && close(y1, 192.0), "{:?}", (x0, y0, x1, y1));
}

fn stops(mut sb: ShapeBuilder, list: &[(&str, f64)]) -> ShapeBuilder {
    for (i, (color, pos)) in list.iter().enumerate() {
        let row = RowKey::Ix(i as u32);
        sb = sb
            .row_cell("FillGradient", None, row.clone(), "GradientStopColor", color)
            .row_cell("FillGradient", None, row.clone(), "GradientStopColorTrans", "0")
            .row_cell("FillGradient", None, row, "GradientStopPosition", &pos.to_string());
    }
    sb
}

/// The six numbers of a `matrix(..)`.
fn matrix_of(text: &str) -> [f64; 6] {
    let n = numbers(text);
    assert_eq!(n.len(), 6, "{text}");
    [n[0], n[1], n[2], n[3], n[4], n[5]]
}

fn apply(m: [f64; 6], x: f64, y: f64) -> (f64, f64) {
    (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
}

#[test]
fn a_linear_gradient_is_defined_with_its_stops_and_runs_across_the_shape_in_pixels() {
    let sb = stops(
        rect(1, 1.0, 1.0, 2.0, 2.0)
            .cell("LinePattern", "0")
            .cell("FillGradientEnabled", "1")
            .cell("FillGradientDir", "0")
            .cell("FillGradientAngle", "0"),
        &[("#FF0000", 0.0), ("#0000FF", 1.0)],
    );
    let (_, root) = export(&one_page(vec![sb]), &defaults());
    let gradients = named(&root, "linearGradient");
    assert_eq!(gradients.len(), 1);
    let g = gradients[0];
    let colours: Vec<(String, String)> = g.elements().map(|s| (attr(s, "stop-color").to_owned(), attr(s, "offset").to_owned())).collect();
    assert_eq!(colours, vec![("#ff0000".to_owned(), "0".to_owned()), ("#0000ff".to_owned(), "1".to_owned())]);
    // The path that uses it names it.
    let id = attr(g, "id");
    let painted: Vec<_> = named(&root, "path").into_iter().filter(|p| p.attr("fill") == Some(format!("url(#{id})").as_str())).collect();
    assert_eq!(painted.len(), 1);
    // Through its own transform the gradient runs from the left edge to the right edge of the square, at its middle: pixels (96, 192) to (288, 192).
    let m = matrix_of(attr(g, "gradientTransform"));
    let (start, end) = (apply(m, attr_f(g, "x1"), attr_f(g, "y1")), apply(m, attr_f(g, "x2"), attr_f(g, "y2")));
    assert!(close(start.0, 96.0) && close(start.1, 192.0), "{start:?}");
    assert!(close(end.0, 288.0) && close(end.1, 192.0), "{end:?}");
    assert_eq!(attr(g, "gradientUnits"), "userSpaceOnUse");
}

fn attr_f(el: &El, name: &str) -> f64 {
    attr(el, name).parse().unwrap_or_else(|_| panic!("`{name}` of <{}> is not a number: {}", el.name, attr(el, name)))
}

#[test]
fn a_radial_gradient_is_centred_on_the_shape() {
    let sb = stops(
        rect(1, 1.0, 1.0, 2.0, 2.0).cell("LinePattern", "0").cell("FillGradientEnabled", "1").cell("FillGradientDir", "3"),
        &[("#FFFFFF", 0.0), ("#000000", 1.0)],
    );
    let (_, root) = export(&one_page(vec![sb]), &defaults());
    let radials = named(&root, "radialGradient");
    assert_eq!(radials.len(), 1);
    let m = matrix_of(attr(radials[0], "gradientTransform"));
    let centre = apply(m, attr_f(radials[0], "cx"), attr_f(radials[0], "cy"));
    assert!(close(centre.0, 192.0) && close(centre.1, 192.0), "{centre:?}");
    let reach = apply(m, attr_f(radials[0], "cx") + attr_f(radials[0], "r"), attr_f(radials[0], "cy"));
    assert!(reach.0 > 192.0 + 90.0, "the circle reaches the shape's corners: {reach:?}");
}

fn label(text: &str, size: f64) -> ShapeBuilder {
    rect(2, 0.5, 2.5, 3.0, 1.0)
        .cell("FillPattern", "0")
        .cell("LinePattern", "0")
        .text(text)
        .row_cell("Character", None, RowKey::Ix(0), "Size", &size.to_string())
        .row_cell("Character", None, RowKey::Ix(0), "Color", "#00AA00")
}

#[test]
fn letters_are_a_text_element_with_the_characters_a_size_a_colour_and_an_x_for_each() {
    let (out, root) = export(&one_page(vec![label("Hello", 0.3)]), &defaults());
    let texts = named(&root, "text");
    assert_eq!(texts.len(), 1);
    let t = texts[0];
    assert_eq!(t.text(), "Hello");
    assert_eq!(attr(t, "fill"), "#00aa00");
    assert!((attr_f(t, "font-size") - 28.8).abs() < 0.01, "0.3 inch is 28.8 pixels: {}", attr(t, "font-size"));
    assert!(
        attr(t, "font-family").ends_with("sans-serif") || attr(t, "font-family").ends_with("monospace") || attr(t, "font-family").ends_with("serif")
    );
    let xs = numbers(attr(t, "x"));
    assert_eq!(xs.len(), 5, "one x for each letter: {xs:?}");
    assert!(xs.windows(2).all(|w| w[1] > w[0]), "left to right: {xs:?}");
    assert_eq!(out.count(SkipKind::TextAsOutlines), 0);
    // Nothing is also drawn as outlines: the paper is the only white path and there is no fill of the letters' colour.
    assert!(filled(&root, "#00aa00").is_empty());
    // Through the element's own transform the first letter stands inside the text block (paper [0.5, 3.5] x [2.5, 3.5], pixels x 48 to 336, y
    // 48 to 144), upright and not mirrored.
    let m = matrix_of(attr(t, "transform"));
    assert!(m[0] > 0.0 && m[3] > 0.0 && m[1].abs() < 1e-6 && m[2].abs() < 1e-6, "upright: {m:?}");
    let ys = numbers(attr(t, "y"));
    assert_eq!(ys.len(), 1, "one baseline for the run");
    let first = apply(m, xs[0], ys[0]);
    let last = apply(m, xs[4], ys[0]);
    assert!(first.0 >= 48.0 && last.0 <= 336.0 && first.1 > 48.0 && first.1 < 144.0, "{first:?} {last:?}");
    // The text is centred in the block: the middle of the first and last origins is near the middle of the block, 192 pixels across.
    assert!(((first.0 + last.0) / 2.0 - 192.0).abs() < 30.0, "{first:?} {last:?}");
}

#[test]
fn a_right_to_left_paragraph_is_a_text_element_set_against_the_right_of_its_block() {
    // The block is paper x 0.5 to 3.5, pixels 48 to 336. Left to right, alignment 0 puts the first letter at the block's left edge; right to left (`Flags` 1) it
    // puts the line against the right edge, so the last letter ends there and every x of the element has moved with it.
    let first_and_last = |flags: &str| {
        let sb = label("Hello", 0.3).row_cell("Paragraph", None, RowKey::Ix(0), "HorzAlign", "0").row_cell(
            "Paragraph",
            None,
            RowKey::Ix(0),
            "Flags",
            flags,
        );
        let (_, root) = export(&one_page(vec![sb]), &defaults());
        let texts = named(&root, "text");
        assert_eq!(texts.len(), 1);
        let t = texts[0];
        let (xs, ys) = (numbers(attr(t, "x")), numbers(attr(t, "y")));
        assert_eq!(xs.len(), 5);
        let m = matrix_of(attr(t, "transform"));
        (apply(m, xs[0], ys[0]).0, apply(m, xs[4], ys[0]).0)
    };
    let (first, _) = first_and_last("0");
    assert!((48.0..56.0).contains(&first), "left to right: the first letter at {first}");
    let (first, last) = first_and_last("1");
    assert!(last > 290.0 && last < 336.0 && first > 230.0, "right to left: letters from {first} to {last}");
}

#[test]
fn vertical_text_is_outlines_since_its_letters_lie_on_their_sides() {
    // `TextDirection` 1: a letter on its side cannot be a `<text>` element, which a viewer sets upright, so it is drawn as outlines; the
    // same label across is a `<text>` element.
    let (_, across) = export(&one_page(vec![label("Hello", 0.3)]), &defaults());
    assert_eq!(named(&across, "text").len(), 1);
    let (out, down) = export(&one_page(vec![label("Hello", 0.3).cell("TextDirection", "1")]), &defaults());
    assert!(named(&down, "text").is_empty());
    assert_eq!(out.count(SkipKind::TextAsOutlines), 1);
    assert!(!filled(&down, "#00aa00").is_empty(), "the letters are filled paths");
}

#[test]
fn text_turns_with_its_shape() {
    let turned = label("Turn", 0.3)
        .cell("PinX", "2")
        .cell("PinY", "2")
        .cell("LocPinX", "1.5")
        .cell("LocPinY", "0.5")
        .cell("Angle", &std::f64::consts::FRAC_PI_2.to_string());
    let (_, root) = export(&one_page(vec![turned]), &defaults());
    let t = named(&root, "text")[0];
    let m = matrix_of(attr(t, "transform"));
    // A quarter turn counterclockwise on the page: reading runs up the page, which is toward smaller y in pixels.
    assert!(m[0].abs() < 1e-4 && m[3].abs() < 1e-4, "{m:?}");
    assert!(m[1] < 0.0 && m[2] > 0.0, "{m:?}");
    assert!((m[1].abs() - 1.0).abs() < 1e-4, "no scaling beyond the pixels the font size already carries: {m:?}");
}

#[test]
fn a_wider_font_scale_stretches_the_letters_across_and_not_the_spacing_away() {
    let plain = {
        let (_, root) = export(&one_page(vec![label("Wide", 0.3)]), &defaults());
        matrix_of(attr(named(&root, "text")[0], "transform"))
    };
    let wide = label("Wide", 0.3).row_cell("Character", None, RowKey::Ix(0), "FontScale", "2");
    let (_, root) = export(&one_page(vec![wide]), &defaults());
    let t = named(&root, "text")[0];
    let m = matrix_of(attr(t, "transform"));
    assert!((m[0] / plain[0] - 2.0).abs() < 1e-6, "{m:?} against {plain:?}");
    assert!((m[3] - plain[3]).abs() < 1e-6, "the height is unchanged");
    // The letters keep the places the layout gave them: each x is in the stretched frame, so its pixel is twice the x over two.
    let xs = numbers(attr(t, "x"));
    assert_eq!(xs.len(), 4);
}

#[test]
fn text_with_markup_characters_is_escaped_and_reads_back_whole() {
    let (_, root) = export(&one_page(vec![label("a<b & \"c\"", 0.3)]), &defaults());
    let t = named(&root, "text")[0];
    assert_eq!(t.text(), "a<b & \"c\"");
}

#[test]
fn as_outlines_writes_every_letter_as_paths_and_no_text_element() {
    let doc = one_page(vec![label("Hello", 0.3)]);
    let (_, as_text) = export(&doc, &defaults());
    let (out, outlined) = export(&doc, &SvgOptions { text: TextMode::AsOutlines, ..defaults() });
    assert!(named(&outlined, "text").is_empty());
    let glyph_paths = filled(&outlined, "#00aa00");
    assert!(!glyph_paths.is_empty(), "the letters are filled paths in their colour");
    assert!(named(&outlined, "path").len() > named(&as_text, "path").len());
    // Asking for outlines is a choice and not a stand-in: nothing is counted for it.
    assert_eq!(out.count(SkipKind::TextAsOutlines), 0);
    // The outlines lie where the text does.
    let (x0, y0, x1, y1) = glyph_paths
        .iter()
        .map(|p| bbox(attr(p, "d")))
        .fold((f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY), |a, b| (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3)));
    assert!(x0 >= 48.0 && x1 <= 336.0 && y0 >= 48.0 && y1 <= 144.0, "{:?}", (x0, y0, x1, y1));
}

#[test]
fn letters_that_are_not_one_glyph_each_are_outlines_and_counted() {
    // Arabic is joined and runs right to left; the bundled faces have no glyphs for it, so every letter is a box or blank: outlines, not text.
    let (out, root) = export(&one_page(vec![label("\u{0645}\u{0631}\u{062D}\u{0628}\u{0627}", 0.3)]), &defaults());
    assert!(named(&root, "text").is_empty());
    assert_eq!(out.count(SkipKind::TextAsOutlines), 1, "{:?}", out.skipped);
}

fn quadrants_png() -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, 8, 8);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().expect("a header");
        let data: Vec<u8> = (0..8u32)
            .flat_map(|y| (0..8u32).map(move |x| (x, y)))
            .flat_map(|(x, y)| match (x < 4, y < 4) {
                (true, true) => [255, 0, 0, 255],
                (false, true) => [0, 255, 0, 255],
                (true, false) => [0, 0, 255, 255],
                (false, false) => [255, 255, 0, 255],
            })
            .collect();
        writer.write_image_data(&data).expect("pixels");
    }
    out
}

fn foreign(content_type: &str, bytes: Vec<u8>) -> ForeignData {
    ForeignData {
        attrs: RawAttrs::from_pairs([("ForeignType", "Bitmap")]),
        picture: Some(Arc::new(Media { part: Arc::from("/visio/media/image1"), content_type: Arc::from(content_type), bytes: Arc::from(bytes) })),
        ..ForeignData::default()
    }
}

fn picture(data: ForeignData) -> ShapeBuilder {
    geometry_of(shape_at(1, 1.0, 1.0, 2.0, 2.0), rect_rows(2.0, 2.0)).cell("FillPattern", "0").cell("LinePattern", "0").foreign(data)
}

/// The standard base64 alphabet decoded, for the tests only.
fn unbase64(text: &str) -> Vec<u8> {
    let value = |c: char| "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/".find(c);
    let mut out = Vec::new();
    let chars: Vec<char> = text.chars().filter(|c| *c != '=').collect();
    for chunk in chars.chunks(4) {
        let n: Vec<u32> = chunk.iter().map(|c| value(*c).expect("a base64 digit") as u32).collect();
        let acc = n.iter().enumerate().fold(0u32, |a, (i, v)| a | (v << (18 - 6 * i as u32)));
        for i in 0..chunk.len().saturating_sub(1) {
            out.push((acc >> (16 - 8 * i as u32)) as u8);
        }
    }
    out
}

#[test]
fn a_picture_is_an_image_with_its_png_clipped_to_the_frame_and_placed_by_one_matrix() {
    let (out, root) = export(&one_page(vec![picture(foreign("image/png", quadrants_png()))]), &defaults());
    let images = named(&root, "image");
    assert_eq!(images.len(), 1);
    let image = images[0];
    assert_eq!((attr(image, "width"), attr(image, "height")), ("8", "8"));
    let href = image.attr("xlink:href").or_else(|| image.attr_local("href")).expect("a reference to the picture");
    let data = href.strip_prefix("data:image/png;base64,").expect("a PNG in a data URI");
    let mut reader = png::Decoder::new(std::io::Cursor::new(unbase64(data))).read_info().expect("the PNG decodes");
    let mut pixels = vec![0u8; reader.output_buffer_size().expect("a size")];
    let info = reader.next_frame(&mut pixels).expect("pixels");
    assert_eq!((info.width, info.height), (8, 8));
    assert_eq!(&pixels[0..4], &[255, 0, 0, 255], "the first pixel is the top left, red");
    assert_eq!(&pixels[(7 * 8 + 7) * 4..(7 * 8 + 7) * 4 + 4], &[255, 255, 0, 255], "the last is the bottom right, yellow");
    // The matrix takes the picture's pixel space onto the frame, paper [1, 3] x [1, 3]: pixel (0, 0) to (96, 96) and (8, 8) to (288, 288).
    let m = matrix_of(attr(image, "transform"));
    let (a, b) = (apply(m, 0.0, 0.0), apply(m, 8.0, 8.0));
    assert!(close(a.0, 96.0) && close(a.1, 96.0) && close(b.0, 288.0) && close(b.1, 288.0), "{a:?} {b:?}");
    // It sits in a group clipped to the frame.
    let clip = named(&root, "clipPath");
    assert_eq!(clip.len(), 1);
    let clip_box = bbox(attr(clip[0].child("path").expect("a path"), "d"));
    assert!(close(clip_box.0, 96.0) && close(clip_box.2, 288.0), "{clip_box:?}");
    assert!(named(&root, "g").iter().any(|g| g.attr("clip-path") == Some(format!("url(#{})", attr(clip[0], "id")).as_str())));
    assert_eq!(out.skipped.len(), 0, "{:?}", out.skipped);
}

#[test]
fn a_metafile_is_the_placeholder_the_window_draws_and_is_reported_the_same_way() {
    let mut emf = vec![0u8; 44];
    emf[40..44].copy_from_slice(b" EMF");
    let doc = one_page(vec![picture(foreign("image/x-emf", emf))]);
    let (out, root) = export(&doc, &defaults());
    assert_eq!(out.count(SkipKind::MetafilePlaceholder), 1, "{:?}", out.skipped);
    assert!(named(&root, "image").is_empty());
    // The same list the window's frame gives for the same page.
    let view = diacraft_render::View::fit_page(&doc, 1, 96.0).expect("a view");
    let drawn = Renderer::new().render(&doc, 1, &view);
    assert_eq!(drawn.skipped, out.skipped);
}

#[test]
fn a_blurred_shadow_is_a_group_with_a_blur_over_its_own_box() {
    let sb = red_box()
        .cell("ShdwPattern", "1")
        .cell("ShapeShdwType", "1")
        .cell("ShapeShdwOffsetX", "0.3")
        .cell("ShapeShdwOffsetY", "-0.3")
        .cell("ShdwForegnd", "#808080")
        .cell("ShapeShdwBlur", "0.15");
    let (_, root) = export(&one_page(vec![sb]), &defaults());
    let filters = named(&root, "filter");
    assert_eq!(filters.len(), 1);
    let blur = filters[0].child("feGaussianBlur").expect("a blur");
    // 0.15 inch of blur is 1.5 deviations: 0.1 inch, 9.6 pixels.
    assert!((attr_f(blur, "stdDeviation") - 9.6).abs() < 0.01, "{}", attr(blur, "stdDeviation"));
    // The filter's region holds the shadow ([1.3, 3.3] x [0.7, 1.7] inches: pixels x 124.8 to 316.8, y 220.8 to 316.8) and its fade.
    let (x, y, w, h) = (attr_f(filters[0], "x"), attr_f(filters[0], "y"), attr_f(filters[0], "width"), attr_f(filters[0], "height"));
    assert!(x < 124.8 && y < 220.8 && x + w > 316.8 && y + h > 316.8, "{x} {y} {w} {h}");
    // The shadow is inside the group that uses it, and the shape itself is not.
    let group = named(&root, "g").into_iter().find(|g| g.attr("filter").is_some()).expect("the blurred group");
    assert_eq!(attr(group, "filter"), format!("url(#{})", attr(filters[0], "id")));
    let inside: Vec<_> = group.elements().filter_map(|p| p.attr("fill")).collect();
    assert_eq!(inside, vec!["#808080"]);
}

#[test]
fn a_page_without_paper_has_no_white_rectangle() {
    let (_, root) = export(&one_page(vec![red_box()]), &SvgOptions { paper: None, ..defaults() });
    assert!(filled(&root, "#ffffff").is_empty());
    assert_eq!(filled(&root, "#ff0000").len(), 1);
}

#[test]
fn the_title_is_written_escaped() {
    let (_, root) = export(&one_page(vec![]), &SvgOptions { title: Some("Flow <1> & more".to_owned()), ..defaults() });
    assert_eq!(root.child("title").map(El::text).as_deref(), Some("Flow <1> & more"));
}

#[test]
fn the_same_page_exports_to_the_same_bytes() {
    let doc = one_page(vec![red_box(), label("Same", 0.3)]);
    let first = export_page(&mut Renderer::new(), &doc, 1, &defaults()).expect("exports");
    let second = export_page(&mut Renderer::new(), &doc, 1, &defaults()).expect("exports");
    assert_eq!(first.bytes, second.bytes);
}

#[test]
fn the_renderer_is_left_as_it_was() {
    let doc = one_page(vec![red_box()]);
    let mut renderer = Renderer::new();
    renderer.set_paper(Some(diacraft_color::Rgba::rgb(1, 2, 3)));
    let limits = renderer.limits();
    export_page(&mut renderer, &doc, 1, &SvgOptions { paper: None, ..defaults() }).expect("exports");
    assert_eq!(renderer.paper(), Some(diacraft_color::Rgba::rgb(1, 2, 3)));
    assert_eq!(renderer.limits(), limits);
}

#[test]
fn an_export_that_cannot_be_made_says_why() {
    let doc = one_page(vec![]);
    let mut renderer = Renderer::new();
    let mut err = |page: u32, o: &SvgOptions| export_page(&mut renderer, &doc, page, o).unwrap_err().to_string();
    assert_eq!(err(5, &defaults()), "the document has no page 5");
    assert_eq!(err(1, &SvgOptions { px_per_inch: 0.0, ..defaults() }), "the resolution must be a positive number");
    assert!(err(1, &SvgOptions { px_per_inch: 1.0e9, ..defaults() }).starts_with("too large to export"));
}

/// An EMF of the shape the metafile spec's V3 describes, in its plain form: a blue brush, the null pen, and a rectangle over the left half
/// of a frame of 80 by 40 device pixels.
fn blue_left_half_emf() -> Vec<u8> {
    let u32s = |v: &[u32]| -> Vec<u8> { v.iter().flat_map(|x| x.to_le_bytes()).collect() };
    let record = |t: u32, fields: Vec<u8>| -> Vec<u8> {
        let mut out = u32s(&[t, 8 + fields.len() as u32]);
        out.extend(fields);
        out
    };
    let ints = |v: &[i32]| -> Vec<u8> { v.iter().flat_map(|x| x.to_le_bytes()).collect() };
    let body: Vec<u8> = [
        record(39, u32s(&[1, 0, 0x00FF_0000, 0])),
        record(37, u32s(&[1])),
        record(37, u32s(&[0x8000_0008])),
        record(43, ints(&[0, 0, 40, 40])),
        record(14, u32s(&[0, 16, 20])),
    ]
    .concat();
    let mut fields = ints(&[0, 0, 79, 39, 0, 0, 2000, 1000]);
    fields.extend(u32s(&[0x464D_4520, 0x10000, 108 + body.len() as u32, 3, 4, 0, 0, 0]));
    fields.extend(ints(&[320, 160, 80, 40]));
    fields.extend(u32s(&[0, 0, 0, 80_000, 40_000]));
    let mut out = record(1, fields);
    out.extend(body);
    out
}

#[test]
fn a_metafile_is_drawn_as_paths_and_the_page_says_nothing_of_it() {
    // The frame (80 by 40 device pixels) is stretched on the 2 by 2 inch box, so the square over its left half is the box's left half, full
    // height: x from 96 to 192 and y from 96 to 288 in CSS pixels. The colour is a blue COLORREF (0x00FF0000).
    let doc = one_page(vec![picture(foreign("image/x-emf", blue_left_half_emf()))]);
    let (out, root) = export(&doc, &defaults());
    assert!(out.skipped.is_empty(), "a picture drawn in full says nothing: {:?}", out.skipped);
    let blue = filled(&root, "#0000ff");
    assert_eq!(blue.len(), 1, "one filled path");
    let b = bbox(attr(blue[0], "d"));
    assert!(close(b.0, 96.0) && close(b.2, 192.0) && close(b.1, 96.0) && close(b.3, 288.0), "{b:?}");
    assert!(named(&root, "image").is_empty());
}

/// A horizontal line 3 inches long at paper height 2, green, of weight 0.02 inch, with the text of the pattern's cell as given.
fn line_with_pattern_text(pattern: &str) -> ShapeBuilder {
    geometry_of(shape_at(1, 0.5, 2.0, 3.0, 0.0), vec![("MoveTo", vec![("X", 0.0), ("Y", 0.0)]), ("LineTo", vec![("X", 3.0), ("Y", 0.0)])])
        .cell("FillPattern", "0")
        .cell("LinePattern", pattern)
        .cell("LineWeight", "0.02")
        .cell("LineColor", "#008000")
}

#[test]
fn a_line_pattern_outside_the_table_is_written_solid_and_the_file_says_so() {
    // A fraction and a number the table has no row for draw solid, with a warning. A fraction was rounded to the number beside it: 2.5 was written with the dashes of
    // pattern 3, 1.6 with those of 2, and 0.4 as no line at all.
    for text in ["2.5", "1.6", "0.4", "24", "100", "-1"] {
        let (out, root) = export(&one_page(vec![line_with_pattern_text(text)]), &defaults());
        let line =
            named(&root, "path").into_iter().find(|p| p.attr("stroke") == Some("#008000")).unwrap_or_else(|| panic!("{text}: the line is there"));
        assert_eq!(line.attr("stroke-dasharray"), None, "{text}: a solid line has no dashes");
        assert!(out.skipped.iter().any(|s| s.kind == SkipKind::PatternOutsideTable), "{text}: said so, got {:?}", out.skipped);
    }
    // A pattern of the table is dashed, and says nothing.
    let (out, root) = export(&one_page(vec![line_with_pattern_text("2")]), &defaults());
    let line = named(&root, "path").into_iter().find(|p| p.attr("stroke") == Some("#008000")).expect("the line");
    assert!(line.attr("stroke-dasharray").is_some());
    assert!(out.skipped.iter().all(|s| s.kind != SkipKind::PatternOutsideTable), "{:?}", out.skipped);
}
