//! Pictures.

use std::sync::Arc;

use diacraft_doc::{ForeignData, Media, RawAttrs};
use diacraft_render::SkipKind;
use diacraft_testkit::build::ShapeBuilder;

use crate::support::*;

/// A PNG of `w` by `h` pixels with the colour `f(x, y)` for each.
pub fn png_of(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, w, h);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().expect("a header");
        let data: Vec<u8> = (0..h).flat_map(|y| (0..w).map(move |x| (x, y))).flat_map(|(x, y)| f(x, y)).collect();
        writer.write_image_data(&data).expect("pixels");
    }
    out
}

/// Four quadrants: red at the top left, green top right, blue bottom left, yellow bottom right.
fn quadrants() -> Vec<u8> {
    png_of(8, 8, |x, y| match (x < 4, y < 4) {
        (true, true) => [255, 0, 0, 255],
        (false, true) => [0, 255, 0, 255],
        (true, false) => [0, 0, 255, 255],
        (false, false) => [255, 255, 0, 255],
    })
}

pub fn foreign(kind: &str, content_type: &str, bytes: Vec<u8>) -> ForeignData {
    ForeignData {
        attrs: RawAttrs::from_pairs([("ForeignType", kind)]),
        picture: Some(Arc::new(Media { part: Arc::from("/visio/media/image1"), content_type: Arc::from(content_type), bytes: Arc::from(bytes) })),
        ..ForeignData::default()
    }
}

/// A picture shape with its frame at `(x, y)`, `w` by `h`.
pub fn picture(x: f64, y: f64, w: f64, h: f64, data: ForeignData) -> ShapeBuilder {
    geometry_of(shape_at(1, x, y, w, h), rect_rows(w, h)).cell("FillPattern", "0").cell("LinePattern", "0").foreign(data)
}

#[test]
fn a_picture_is_stretched_to_its_frame_with_its_first_row_at_the_top() {
    let pic = draw(&one_page(vec![picture(1.0, 1.0, 2.0, 2.0, foreign("Bitmap", "image/png", quadrants()))]));
    assert!(pic.is_red(1.3, 2.7), "the first row, first column: top left");
    assert!(pic.is(2.7, 2.7, (0, 255, 0), 12), "top right");
    assert!(pic.is(1.3, 1.3, (0, 0, 255), 12), "bottom left");
    assert!(pic.is(2.7, 1.3, (255, 255, 0), 12), "bottom right");
    assert!(pic.is_white(0.9, 2.0) && pic.is_white(3.1, 2.0));
}

#[test]
fn the_decoder_comes_from_the_bytes_and_not_from_the_label() {
    // A part typed GIF that holds PNG.
    let pic = draw(&one_page(vec![picture(1.0, 1.0, 2.0, 2.0, foreign("Bitmap", "image/gif", quadrants()))]));
    assert!(pic.is_red(1.3, 2.7));
}

#[test]
fn a_picture_is_cut_to_its_frame_and_the_box_places_its_lower_left_corner() {
    // The box is 4 by 4 starting 1 left of and 1 below the 2 by 2 frame, so the frame shows the middle of the picture
    // and nothing is drawn outside the frame.
    let sb = picture(1.0, 1.0, 2.0, 2.0, foreign("Bitmap", "image/png", quadrants()))
        .cell("ImgWidth", "4")
        .cell("ImgHeight", "4")
        .cell("ImgOffsetX", "-1")
        .cell("ImgOffsetY", "-1");
    let pic = draw(&one_page(vec![sb]));
    assert!(pic.is_white(0.8, 2.0) && pic.is_white(3.2, 2.0) && pic.is_white(2.0, 3.2) && pic.is_white(2.0, 0.8), "nothing outside the frame");
    // The frame's lower left quarter [1, 2] x [1, 2] is the box's x in [1, 2] and y in [1, 2] of 4 by 4 from (0, 0): the picture's centre-left
    // is its blue/red boundary at y = 2; below it is blue (bottom left of the picture), above red.
    assert!(pic.is(1.5, 1.5, (0, 0, 255), 12), "box position (1.5, 1.5) of 4 is the picture's lower left quadrant");
    assert!(pic.is_red(1.5, 2.5), "and just above the middle is the upper left");
    assert!(pic.is(2.5, 2.5, (0, 255, 0), 12));
}

#[test]
fn a_picture_turns_and_flips_with_its_shape() {
    let flipped = picture(1.0, 1.0, 2.0, 2.0, foreign("Bitmap", "image/png", quadrants()))
        .cell("FlipX", "1")
        .cell("PinX", "2")
        .cell("LocPinX", "1")
        .cell("PinY", "2")
        .cell("LocPinY", "1");
    let pic = draw(&one_page(vec![flipped]));
    assert!(pic.is(1.3, 2.7, (0, 255, 0), 12), "FlipX puts the picture's right column on the left");
    assert!(pic.is_red(2.7, 2.7));
}

#[test]
fn a_zero_box_draws_nothing_and_says_nothing() {
    let pic = draw(&one_page(vec![picture(1.0, 1.0, 2.0, 2.0, foreign("Bitmap", "image/png", quadrants())).cell("ImgWidth", "0")]));
    assert_eq!(pic.ink_pixels(), 0);
    assert!(pic.r.skipped.is_empty(), "{:?}", pic.r.skipped);
}

#[test]
fn a_metafile_is_a_placeholder_that_says_so() {
    // EMF: " EMF" at offset 40. WMF: the placeable header.
    let mut emf = vec![0u8; 100];
    emf[40..44].copy_from_slice(b" EMF");
    let pic = draw(&one_page(vec![picture(1.0, 1.0, 2.0, 2.0, foreign("EnhMetaFile", "image/x-emf", emf))]));
    assert!(pic.ink_pixels() > 0 || !pic.is_white(2.0, 2.0), "a placeholder is drawn: {:?}", pic.at(2.0, 2.0));
    assert_eq!(pic.r.count(SkipKind::MetafilePlaceholder), 1);
    let wmf = vec![0xD7, 0xCD, 0xC6, 0x9A, 0, 0, 0, 0];
    let pic = draw(&one_page(vec![picture(1.0, 1.0, 2.0, 2.0, foreign("EnhMetaFile", "image/x-emf", wmf))]));
    assert_eq!(pic.r.count(SkipKind::MetafilePlaceholder), 1, "a WMF in a part typed EMF is found by its bytes");
}

#[test]
fn bytes_that_do_not_decode_are_a_placeholder_and_a_report_and_never_a_panic() {
    for (name, bytes) in [
        ("truncated PNG", quadrants()[..30].to_vec()),
        ("JPEG magic only", vec![0xFF, 0xD8, 0xFF, 0xE0, 0, 0]),
        ("BMP magic only", b"BM".to_vec()),
        ("noise", (0..300u32).map(|i| (i * 7 + 3) as u8).collect()),
        ("empty", Vec::new()),
    ] {
        let pic = draw(&one_page(vec![picture(1.0, 1.0, 2.0, 2.0, foreign("Bitmap", "image/png", bytes))]));
        assert_eq!(pic.r.count(SkipKind::PictureUndecoded), 1, "{name}: {:?}", pic.r.skipped);
    }
}

#[test]
fn a_bmp_is_read_bottom_up_and_a_huge_one_is_refused_before_its_pixels_are_allocated() {
    // 2 by 2, 24 bits, bottom row first: blue, yellow; then red, green. Rows are padded to four bytes.
    let mut bmp = Vec::new();
    bmp.extend_from_slice(b"BM");
    bmp.extend_from_slice(&(54u32 + 16).to_le_bytes());
    bmp.extend_from_slice(&[0, 0, 0, 0]);
    bmp.extend_from_slice(&54u32.to_le_bytes());
    bmp.extend_from_slice(&40u32.to_le_bytes());
    bmp.extend_from_slice(&2u32.to_le_bytes());
    bmp.extend_from_slice(&2u32.to_le_bytes());
    bmp.extend_from_slice(&1u16.to_le_bytes());
    bmp.extend_from_slice(&24u16.to_le_bytes());
    bmp.extend_from_slice(&[0u8; 24]);
    // Pixels are stored as blue, green, red.
    bmp.extend_from_slice(&[255, 0, 0, 0, 255, 255, 0, 0]);
    bmp.extend_from_slice(&[0, 0, 255, 0, 255, 0, 0, 0]);
    let pic = draw(&one_page(vec![picture(1.0, 1.0, 2.0, 2.0, foreign("Bitmap", "image/bmp", bmp.clone()))]));
    assert!(pic.is_red(1.3, 2.7) && pic.is(2.7, 2.7, (0, 255, 0), 12) && pic.is(1.3, 1.3, (0, 0, 255), 12) && pic.is(2.7, 1.3, (255, 255, 0), 12));
    bmp[18..22].copy_from_slice(&40_000u32.to_le_bytes());
    bmp[22..26].copy_from_slice(&40_000u32.to_le_bytes());
    let pic = draw(&one_page(vec![picture(1.0, 1.0, 2.0, 2.0, foreign("Bitmap", "image/bmp", bmp))]));
    assert_eq!(pic.r.count(SkipKind::PictureUndecoded), 1);
}

#[test]
fn an_ink_shape_draws_its_fallback_picture_or_nothing_with_a_warning() {
    let with = foreign("Ink", "image/png", quadrants());
    let pic = draw(&one_page(vec![picture(1.0, 1.0, 2.0, 2.0, with)]));
    assert!(pic.is_red(1.3, 2.7), "its fallback picture");
    let bare = ForeignData { attrs: RawAttrs::from_pairs([("ForeignType", "Ink")]), ..ForeignData::default() };
    let pic = draw(&one_page(vec![picture(1.0, 1.0, 2.0, 2.0, bare)]));
    assert_eq!(pic.ink_pixels(), 0);
    assert_eq!(pic.r.count(SkipKind::InkNoPicture), 1);
}

#[test]
fn a_clipping_path_cuts_the_picture_instead_of_the_frame() {
    // `Geometry2.Path` is the section with IX 1, which holds NoShow and so draws nothing itself. Here a triangle.
    let mut sb = picture(1.0, 1.0, 2.0, 2.0, foreign("Bitmap", "image/png", quadrants()));
    for (i, (kind, x, y)) in [("MoveTo", 0.0, 0.0), ("LineTo", 2.0, 0.0), ("LineTo", 0.0, 2.0), ("LineTo", 0.0, 0.0)].iter().enumerate() {
        let key = diacraft_doc::RowKey::Ix(i as u32);
        sb = sb.row_type("Geometry", Some(1), key.clone(), kind).row_cell("Geometry", Some(1), key.clone(), "X", &x.to_string()).row_cell(
            "Geometry",
            Some(1),
            key,
            "Y",
            &y.to_string(),
        );
    }
    let sb = sb
        .set(diacraft_doc::CellAddr::in_section("Geometry", Some(1), "NoShow"), diacraft_doc::Cell::constant(diacraft_doc::Value::from_text("1")))
        .cell("ClippingPath", "Geometry2.Path");
    let pic = draw(&one_page(vec![sb]));
    assert!(pic.is(1.3, 1.3, (0, 0, 255), 12), "inside the triangle: the lower left half of the frame shows the picture");
    assert!(pic.is_white(2.7, 2.7), "outside it the picture is cut away, though the frame is there: the path alone cuts");
}

#[test]
fn the_caches_keep_the_parts_of_pictures_alive_and_clearing_them_lets_go() {
    // The renderer keeps what it decoded between frames, and with it the part's bytes, so that the old document's media outlive the
    // document until something drops them. `clear_caches` is that something. A picture that does not decode, a metafile and a raster all
    // hold their part.
    let mut emf = vec![0u8; 100];
    emf[40..44].copy_from_slice(b" EMF");
    for (what, data) in [
        ("undecodable", foreign("Bitmap", "image/png", (0..300u32).map(|i| (i * 7 + 3) as u8).collect())),
        ("metafile", foreign("EnhMetaFile", "image/x-emf", emf)),
        ("raster", foreign("Bitmap", "image/png", quadrants())),
    ] {
        let media = Arc::clone(data.picture.as_ref().expect("a picture"));
        let doc = one_page(vec![picture(1.0, 1.0, 2.0, 2.0, data)]);
        let held_by_the_document = Arc::strong_count(&media);
        let mut renderer = diacraft_render::Renderer::new();
        draw_with(&mut renderer, &doc);
        assert_eq!(Arc::strong_count(&media), held_by_the_document + 1, "{what}: the cache holds the part after a frame");
        draw_with(&mut renderer, &doc);
        assert_eq!(Arc::strong_count(&media), held_by_the_document + 1, "{what}: and holds it once, however many frames");
        renderer.clear_caches();
        assert_eq!(Arc::strong_count(&media), held_by_the_document, "{what}: clearing the caches lets go of it");
        // The next frame draws it again, from nothing.
        let again = draw_with(&mut renderer, &doc);
        assert!(again.ink_pixels() > 0 || !again.is_white(2.0, 2.0), "{what}: still drawn after the caches were cleared");
    }
}
