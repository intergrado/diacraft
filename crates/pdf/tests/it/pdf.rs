//! The PDF a page becomes, read back with the shared PDF reader and interpreter (which know nothing of the writer) and checked by value:
//! where each path is in points, what it is painted with, what text each glyph stands for, and where it stands.
//!
//! Pages here are 4 by 4 inches, so the file is 288 by 288 points and paper inch `(x, y)` (y up) is at point `(72 x, 288 - 72 y)`.

use std::sync::Arc;

use diacraft_doc::{ForeignData, Media, RawAttrs, RowKey};
use diacraft_fonts::FontDb;
use diacraft_fonts::synth::SynthFont;
use diacraft_pdf::{PdfOptions, TextMode, export_page};
use diacraft_render::{Renderer, SkipKind};
use diacraft_testkit::build::ShapeBuilder;

use crate::support::*;

fn red_box() -> ShapeBuilder {
    rect(1, 1.0, 1.0, 2.0, 1.0).cell("FillForegnd", "#FF0000").cell("LinePattern", "0")
}

#[test]
fn the_file_is_one_page_of_the_pages_size_in_points() {
    let (out, seen) = export(&one_page(vec![]), &plain());
    assert!(out.bytes.starts_with(b"%PDF-"));
    assert_eq!(seen.pages, 1);
    assert_eq!(seen.size, (288.0, 288.0));
    assert_eq!((out.width, out.height), (288, 288));
    // The paper: white, the whole page.
    let paper = seen.with_colour([255, 255, 255]);
    assert_eq!(paper.len(), 1);
    assert_eq!((paper[0].bbox.x0, paper[0].bbox.y0, paper[0].bbox.x1, paper[0].bbox.y1), (0.0, 0.0, 288.0, 288.0));
}

#[test]
fn a_filled_rectangle_is_a_path_at_its_place_in_points_with_y_down() {
    let (_, seen) = export(&one_page(vec![red_box()]), &plain());
    let boxes = seen.with_colour([255, 0, 0]);
    assert_eq!(boxes.len(), 1);
    // Paper [1, 3] x [1, 2] is points x 72 to 216, y 288 - 144 = 144 to 288 - 72 = 216.
    let b = boxes[0].bbox;
    assert!(close(b.x0, 72.0) && close(b.x1, 216.0) && close(b.y0, 144.0) && close(b.y1, 216.0), "{b:?}");
    assert!(!boxes[0].stroked);
    assert!(boxes[0].even_odd, "filled by the odd-crossings rule, as the window fills");
}

#[test]
fn a_transparent_fill_says_its_opacity_and_a_fully_transparent_one_writes_nothing() {
    let (_, seen) = export(&one_page(vec![red_box().cell("FillForegndTrans", "0.5")]), &plain());
    let boxes = seen.with_colour([255, 0, 0]);
    assert_eq!(boxes.len(), 1);
    let alpha = boxes[0].rgba.expect("a colour")[3];
    assert!((126..=129).contains(&alpha), "{alpha}");
    let (_, seen) = export(&one_page(vec![red_box().cell("FillForegndTrans", "1")]), &plain());
    assert!(seen.with_colour([255, 0, 0]).is_empty());
}

#[test]
fn the_outline_is_a_stroked_path_as_wide_as_the_line_weight_in_points() {
    let sb = red_box().cell("LinePattern", "1").cell("LineColor", "#0000FF").cell("LineWeight", "0.05");
    let (_, seen) = export(&one_page(vec![sb]), &plain());
    let lines: Vec<_> = seen.with_colour([0, 0, 255]);
    assert_eq!(lines.len(), 1);
    assert!(lines[0].stroked);
    assert!((lines[0].width - 3.6).abs() < 1e-3, "0.05 inch is 3.6 points: {}", lines[0].width);
    assert!(lines[0].dashes.is_empty(), "a solid line has no dashes");
}

/// A horizontal line 3 inches long at paper height 2, green, of this weight (inches) and pattern.
fn thin_line(weight: f64, pattern: u32) -> ShapeBuilder {
    geometry_of(shape_at(1, 0.5, 2.0, 3.0, 0.0), vec![("MoveTo", vec![("X", 0.0), ("Y", 0.0)]), ("LineTo", vec![("X", 3.0), ("Y", 0.0)])])
        .cell("FillPattern", "0")
        .cell("LinePattern", &pattern.to_string())
        .cell("LineWeight", &weight.to_string())
        .cell("LineColor", "#008000")
}

#[test]
fn a_line_thinner_than_a_pixel_is_written_at_its_true_width_and_not_as_the_zero_width_line() {
    // The window and the PNG draw a line under a pixel one pixel wide because they rasterise. A PDF is rasterised by its
    // reader at a resolution of its own, so it holds the weight: 0.24 points. The width 0, which PDF defines as the thinnest line the device
    // draws (ISO 32000-1 8.4.3.2), is not used: its result is the device's, a pixel on a screen and a fraction of a point on a printer.
    let (_, seen) = export(&one_page(vec![thin_line(0.24 / 72.0, 1)]), &plain());
    let line = seen.with_colour([0, 128, 0]);
    assert_eq!(line.len(), 1);
    assert!((line[0].width - 0.24).abs() < 1e-3, "{}", line[0].width);
    let (_, seen) = export(&one_page(vec![thin_line(0.24 / 72.0, 2)]), &plain());
    let line = seen.with_colour([0, 128, 0]);
    assert!(
        line[0].dashes.len() == 2 && (line[0].dashes[0] - 7.0 * 0.24).abs() < 2e-3 && (line[0].dashes[1] - 5.0 * 0.24).abs() < 2e-3,
        "7 and 5 weights: {:?}",
        line[0].dashes
    );
}

#[test]
fn a_weight_of_zero_is_a_quarter_point_in_a_file() {
    // One device pixel on a screen, and 0.25 points in a vector export.
    let (_, seen) = export(&one_page(vec![thin_line(0.0, 1)]), &plain());
    let line = seen.with_colour([0, 128, 0]);
    assert_eq!(line.len(), 1);
    assert!((line[0].width - 0.25).abs() < 1e-3, "{}", line[0].width);
}

#[test]
fn a_dashed_line_carries_its_dashes_in_points() {
    let sb = geometry_of(shape_at(1, 0.5, 2.0, 3.0, 0.0), vec![("MoveTo", vec![("X", 0.0), ("Y", 0.0)]), ("LineTo", vec![("X", 3.0), ("Y", 0.0)])])
        .cell("FillPattern", "0")
        .cell("LinePattern", "2")
        .cell("LineWeight", "0.02")
        .cell("LineColor", "#008000");
    let (_, seen) = export(&one_page(vec![sb]), &plain());
    let line = seen.with_colour([0, 128, 0]);
    assert_eq!(line.len(), 1);
    assert!(line[0].dashes.len() >= 2 && line[0].dashes.iter().all(|d| *d > 0.0 && *d < 216.0), "{:?}", line[0].dashes);
    assert!((line[0].width - 1.44).abs() < 1e-3, "{}", line[0].width);
    let b = line[0].bbox;
    assert!(close(b.x0, 36.0) && close(b.x1, 252.0) && close(b.y0, 144.0), "{b:?}");
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

#[test]
fn a_gradient_fills_the_shape_with_a_shading_and_not_a_colour() {
    let sb = stops(
        rect(1, 1.0, 1.0, 2.0, 2.0)
            .cell("LinePattern", "0")
            .cell("FillGradientEnabled", "1")
            .cell("FillGradientDir", "0")
            .cell("FillGradientAngle", "0"),
        &[("#FF0000", 0.0), ("#0000FF", 1.0)],
    );
    let (_, seen) = export(&one_page(vec![sb]), &plain());
    let shaded: Vec<_> = seen.paths.iter().filter(|p| p.rgba.is_none()).collect();
    assert_eq!(shaded.len(), 1, "{:?}", seen.paths);
    let b = shaded[0].bbox;
    assert!(close(b.x0, 72.0) && close(b.x1, 216.0) && close(b.y0, 72.0) && close(b.y1, 216.0), "{b:?}");
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
fn letters_are_text_in_an_embedded_font_with_the_characters_they_stand_for() {
    let (out, seen) = export(&one_page(vec![label("Hello", 0.3)]), &plain());
    assert_eq!(seen.text(), "Hello");
    assert_eq!(seen.glyphs.len(), 5);
    assert!(seen.glyphs.iter().all(|g| g.embedded && g.drawn && g.gid != 0), "{:?}", seen.glyphs);
    assert!(seen.glyphs.iter().all(|g| g.rgba == [0, 170, 0, 255]));
    assert_eq!(out.count(SkipKind::TextAsOutlines), 0);
    // Not also drawn as outlines.
    assert!(seen.with_colour([0, 170, 0]).is_empty());
    // The glyphs stand left to right on one baseline inside the text block (paper [0.5, 3.5] x [2.5, 3.5]: points x 36 to 252, y 36 to 108).
    let xs: Vec<f64> = seen.glyphs.iter().map(|g| g.origin.x).collect();
    assert!(xs.windows(2).all(|w| w[1] > w[0]), "{xs:?}");
    assert!(seen.glyphs.iter().all(|g| close(g.origin.y, seen.glyphs[0].origin.y)), "one baseline");
    let (first, last) = (seen.glyphs[0].origin, seen.glyphs[4].origin);
    assert!(first.x >= 36.0 && last.x <= 252.0 && first.y > 36.0 && first.y < 108.0, "{first:?} {last:?}");
    // Centred in the block: the middle of the first and last origins is near the block's middle, 144 points across.
    assert!(((first.x + last.x) / 2.0 - 144.0).abs() < 25.0, "{first:?} {last:?}");
    // An em of 0.3 inch is 21.6 points: the outline's unit is that over the face's units per em, the same along x and y (not stretched).
    let (sx, sy) = seen.glyphs[0].scale;
    assert!((sx - sy).abs() < 1e-9 && sx > 0.0, "{sx} {sy}");
}

#[test]
fn the_size_of_the_letters_follows_the_size_cell() {
    let at = |size: f64| export(&one_page(vec![label("Aa", size)]), &plain()).1.glyphs[0].scale.0;
    assert!((at(0.6) / at(0.3) - 2.0).abs() < 1e-6);
}

#[test]
fn a_wider_font_scale_stretches_the_letters_across_and_not_up() {
    let plain_scale = export(&one_page(vec![label("Wide", 0.3)]), &plain()).1.glyphs[0].scale;
    let wide = label("Wide", 0.3).row_cell("Character", None, RowKey::Ix(0), "FontScale", "2");
    let scale = export(&one_page(vec![wide]), &plain()).1.glyphs[0].scale;
    assert!((scale.0 / plain_scale.0 - 2.0).abs() < 1e-6, "{scale:?} against {plain_scale:?}");
    assert!((scale.1 - plain_scale.1).abs() < 1e-9);
}

#[test]
fn text_turns_with_its_shape() {
    let turned = label("Turn", 0.3)
        .cell("PinX", "2")
        .cell("PinY", "2")
        .cell("LocPinX", "1.5")
        .cell("LocPinY", "0.5")
        .cell("Angle", &std::f64::consts::FRAC_PI_2.to_string());
    let (_, seen) = export(&one_page(vec![turned]), &plain());
    assert_eq!(seen.text(), "Turn");
    // A quarter turn counterclockwise on the page: reading runs up the page, toward smaller y in points, and not across.
    let (a, b) = (seen.glyphs[0].origin, seen.glyphs[3].origin);
    assert!(b.y < a.y - 10.0 && (b.x - a.x).abs() < 1.0, "{a:?} {b:?}");
}

#[test]
fn as_outlines_writes_every_letter_as_paths_and_no_text() {
    let (out, seen) = export(&one_page(vec![label("Hello", 0.3)]), &PdfOptions { text: TextMode::AsOutlines, ..plain() });
    assert!(seen.glyphs.is_empty());
    let shapes = seen.with_colour([0, 170, 0]);
    assert!(!shapes.is_empty(), "the letters are filled paths in their colour");
    let b = shapes.iter().fold(kurbo::Rect::new(1e9, 1e9, -1e9, -1e9), |a, p| a.union(p.bbox));
    assert!(b.x0 >= 36.0 && b.x1 <= 252.0 && b.y0 >= 36.0 && b.y1 <= 108.0, "{b:?}");
    assert_eq!(out.count(SkipKind::TextAsOutlines), 0, "a choice and not a stand-in");
}

#[test]
fn letters_the_face_has_no_glyph_for_are_outlines_and_counted() {
    let (out, seen) = export(&one_page(vec![label("\u{0645}\u{0631}\u{062D}\u{0628}\u{0627}", 0.3)]), &plain());
    assert!(seen.glyphs.is_empty());
    assert_eq!(out.count(SkipKind::TextAsOutlines), 1, "{:?}", out.skipped);
}

/// A renderer whose only face is a synthetic one that says `fs_type` about embedding.
fn renderer_with(fs_type: u16) -> Renderer {
    let mut font = SynthFont::new("Licensed Sans").ascii(500);
    font.fs_type = fs_type;
    let mut db = FontDb::bundled();
    assert_eq!(db.add_font_data(font.build()).added, 1);
    Renderer::with_fonts(db)
}

fn in_licensed(text: &str) -> ShapeBuilder {
    label(text, 0.3).row_cell("Character", None, RowKey::Ix(0), "Font", "Licensed Sans")
}

#[test]
fn a_face_whose_licence_forbids_embedding_is_written_as_outlines_and_counted() {
    let doc = one_page(vec![in_licensed("Hello")]);
    let (out, seen) = export_with(&mut renderer_with(2), &doc, &plain());
    assert!(seen.glyphs.is_empty(), "no text in a face that may not be embedded: {:?}", seen.glyphs);
    assert!(!seen.with_colour([0, 170, 0]).is_empty());
    assert_eq!(out.count(SkipKind::TextAsOutlines), 1, "{:?}", out.skipped);
    // The same text in a face that may be embedded is text.
    let (out, seen) = export_with(&mut renderer_with(0), &doc, &plain());
    assert_eq!(seen.text(), "Hello");
    assert_eq!(out.count(SkipKind::TextAsOutlines), 0);
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

#[test]
fn a_picture_is_an_image_placed_on_its_frame_and_clipped_to_it() {
    let (out, seen) = export(&one_page(vec![picture(foreign("image/png", quadrants_png()))]), &plain());
    assert_eq!(seen.images.len(), 1);
    // The image's matrix takes its pixels (8 by 8, y down) to the frame, paper [1, 3] x [1, 3]: points 72 to 216 each way.
    let m = seen.images[0];
    let corners = [(0.0, 0.0), (8.0, 0.0), (8.0, 8.0), (0.0, 8.0)].map(|(x, y)| m * kurbo::Point::new(x, y));
    let b = corners.iter().fold(kurbo::Rect::new(1e9, 1e9, -1e9, -1e9), |a, p| a.union(kurbo::Rect::from_points(*p, *p)));
    assert!(close(b.x0, 72.0) && close(b.x1, 216.0) && close(b.y0, 72.0) && close(b.y1, 216.0), "{b:?}");
    assert_eq!(seen.clips.len(), 1);
    assert!(close(seen.clips[0].x0, 72.0) && close(seen.clips[0].x1, 216.0), "{:?}", seen.clips);
    assert_eq!(seen.open_clips, 0, "every clip is closed");
    assert!(out.skipped.is_empty(), "{:?}", out.skipped);
}

#[test]
fn a_metafile_is_the_placeholder_the_window_draws_and_is_reported_the_same_way() {
    let mut emf = vec![0u8; 44];
    emf[40..44].copy_from_slice(b" EMF");
    let doc = one_page(vec![picture(foreign("image/x-emf", emf))]);
    let (out, seen) = export(&doc, &plain());
    assert_eq!(out.count(SkipKind::MetafilePlaceholder), 1, "{:?}", out.skipped);
    assert!(seen.images.is_empty());
    let view = diacraft_render::View::fit_page(&doc, 1, 72.0).expect("a view");
    let drawn = Renderer::new().render(&doc, 1, &view);
    assert_eq!(drawn.skipped, out.skipped);
}

#[test]
fn a_blurred_shadow_is_drawn_hard_edged_and_said_so() {
    let sb = red_box()
        .cell("ShdwPattern", "1")
        .cell("ShapeShdwType", "1")
        .cell("ShapeShdwOffsetX", "0.3")
        .cell("ShapeShdwOffsetY", "-0.3")
        .cell("ShdwForegnd", "#808080")
        .cell("ShapeShdwBlur", "0.15");
    let (out, seen) = export(&one_page(vec![sb]), &plain());
    assert_eq!(out.count(SkipKind::ShadowApproximate), 1, "{:?}", out.skipped);
    let shadow: Vec<_> = seen.with_colour([128, 128, 128]).into_iter().filter(|p| !p.stroked).collect();
    assert_eq!(shadow.len(), 1, "the shadow's fill");
    // The shadow, [1.3, 3.3] x [0.7, 1.7] inches (points x 93.6 to 237.6, y 165.6 to 237.6), comes before the shape that stands in front of it.
    assert!(close(shadow[0].bbox.x0, 93.6) && close(shadow[0].bbox.y1, 237.6), "{:?}", shadow[0].bbox);
    let place = |rgb: [u8; 3]| seen.paths.iter().position(|p| p.rgba.is_some_and(|c| c[..3] == rgb && !p.stroked));
    assert!(place([128, 128, 128]) < place([255, 0, 0]) && place([255, 0, 0]).is_some(), "the shadow is drawn first");
}

#[test]
fn a_page_without_paper_has_no_white_rectangle() {
    let (_, seen) = export(&one_page(vec![red_box()]), &PdfOptions { paper: None, ..plain() });
    assert!(seen.with_colour([255, 255, 255]).is_empty());
    assert_eq!(seen.with_colour([255, 0, 0]).len(), 1);
}

#[test]
fn the_renderer_is_left_as_it_was() {
    let doc = one_page(vec![red_box()]);
    let mut renderer = Renderer::new();
    renderer.set_paper(Some(diacraft_color::Rgba::rgb(1, 2, 3)));
    let limits = renderer.limits();
    export_page(&mut renderer, &doc, 1, &PdfOptions { paper: None, ..plain() }).expect("exports");
    assert_eq!(renderer.paper(), Some(diacraft_color::Rgba::rgb(1, 2, 3)));
    assert_eq!(renderer.limits(), limits);
}

#[test]
fn an_export_that_cannot_be_made_says_why() {
    let doc = one_page(vec![]);
    let mut renderer = Renderer::new();
    let mut err = |page: u32| export_page(&mut renderer, &doc, page, &plain()).unwrap_err().to_string();
    assert_eq!(err(5), "the document has no page 5");
    let sizeless = base().page(1, page(0.0, 4.0)).build();
    assert_eq!(
        export_page(&mut Renderer::new(), &sizeless, 1, &plain()).unwrap_err().to_string(),
        "the page has no size, so there is nothing to export"
    );
}

#[test]
fn compressed_content_is_smaller_and_reads_the_same() {
    let doc = one_page(
        (0..200u32).map(|i| rect(i + 1, f64::from(i % 20) * 0.19, f64::from(i / 20) * 0.3, 0.18, 0.2).cell("FillForegnd", "#FF0000")).collect(),
    );
    let big = export_page(&mut Renderer::new(), &doc, 1, &plain()).expect("exports");
    let small = export_page(&mut Renderer::new(), &doc, 1, &PdfOptions::default()).expect("exports");
    assert!(small.bytes.len() < big.bytes.len() / 2, "{} against {}", small.bytes.len(), big.bytes.len());
    assert_eq!(read(&small.bytes).with_colour([255, 0, 0]).len(), 200);
}

use crate::readback::read;

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
fn a_metafile_is_painted_as_paths_in_the_pdf() {
    // The square over the frame's left half is the box's left half: points 72 to 144 across and 72 to 216 up on a page of 4 inches.
    let doc = one_page(vec![picture(foreign("image/x-emf", blue_left_half_emf()))]);
    let (out, seen) = export(&doc, &plain());
    assert!(out.skipped.is_empty(), "a picture drawn in full says nothing: {:?}", out.skipped);
    let blue = seen.with_colour([0, 0, 255]);
    assert_eq!(blue.len(), 1, "{:?}", seen.paths);
    let b = blue[0].bbox;
    assert!(close(b.x0, 72.0) && close(b.x1, 144.0) && close(b.y0, 72.0) && close(b.y1, 216.0), "{b:?}");
    assert!(seen.images.is_empty());
    assert_eq!(seen.open_clips, 0, "every clip is closed");
}
