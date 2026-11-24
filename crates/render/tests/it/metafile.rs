//! Metafile pictures: each test
//! builds an EMF, EMF+ or WMF in code, draws it in a picture shape, and asserts **pixels** by record class, named in paper inches. The
//! picture box is 2 inches wide and 1 tall at (1, 1), so the 80 by 40 pixel frame has 40 pixels to the inch, and frame pixel (u, v) is at
//! paper (1 + u / 40, 2 - v / 40).

use diacraft_doc::Document;
use diacraft_render::{Renderer, SkipKind};

use crate::meta_build::*;
use crate::pictures::{foreign, picture};
use crate::support::*;

fn doc_of(bytes: Vec<u8>) -> Document {
    one_page(vec![picture(1.0, 1.0, 2.0, 1.0, foreign("EnhMetaFile", "image/x-emf", bytes))])
}

fn draw_meta(bytes: Vec<u8>) -> Pic {
    draw(&doc_of(bytes))
}

/// Paper inches of a frame pixel's centre.
fn at(u: f64, v: f64) -> (f64, f64) {
    (1.0 + u / 40.0, 2.0 - v / 40.0)
}

fn is(pic: &Pic, u: f64, v: f64, rgb: (u8, u8, u8)) -> bool {
    let (x, y) = at(u, v);
    pic.is(x, y, rgb, 14)
}

fn blank(pic: &Pic, u: f64, v: f64) -> bool {
    let (x, y) = at(u, v);
    pic.is_white(x, y)
}

fn dark(pic: &Pic, u: f64, v: f64) -> bool {
    let (x, y) = at(u, v);
    pic.is_dark(x, y)
}

fn gdi_red_right_half() -> Vec<Vec<u8>> {
    let mut v = brush_no_pen(1, rgb(255, 0, 0));
    v.push(rect_record(40, 0, 80, 40));
    v
}

fn dual_blue_left() -> Vec<Vec<u8>> {
    let mut v = vec![hp(), plus_fill_rect(argb(0, 0, 255, 255), [0.0, 0.0, 40.0, 40.0])];
    v.extend(gdi_red_right_half());
    v.push(end_plus());
    v
}

#[test]
fn the_emf_plus_half_of_a_dual_file_is_drawn_and_the_gdi_half_is_not() {
    // The left half is blue and the right half is empty; drawing the GDI half would give a red right half.
    let pic = draw_meta(emf(&dual_blue_left()));
    assert!(is(&pic, 20.0, 20.0, (0, 0, 255)), "blue on the left");
    assert!(blank(&pic, 60.0, 20.0), "nothing on the right: {:?}", pic.at(2.5, 1.5));
    assert!(pic.r.skipped.is_empty(), "a picture drawn in full adds no row: {:?}", pic.r.skipped);
}

#[test]
fn a_record_no_reader_knows_makes_a_dual_file_play_its_gdi_half_and_say_so() {
    let mut recs = dual_blue_left();
    recs.insert(1, c(&[p(0x40F0, 0, &[])]));
    let pic = draw_meta(emf(&recs));
    assert!(blank(&pic, 20.0, 20.0), "no blue: the halves are not mixed");
    assert!(is(&pic, 60.0, 20.0, (255, 0, 0)), "red on the right");
    assert_eq!(pic.r.count(SkipKind::MetafileGdiFallback), 1, "{:?}", pic.r.skipped);
    assert_eq!(pic.r.skipped.len(), 1, "one row for the picture: {:?}", pic.r.skipped);
}

#[test]
fn an_emf_plus_only_file_draws_what_it_can_and_names_the_record_it_skipped() {
    let mut recs = dual_blue_left();
    recs[0] = hp_flags(0);
    recs.insert(1, c(&[p(0x40F0, 0, &[])]));
    let pic = draw_meta(emf(&recs));
    assert!(is(&pic, 20.0, 20.0, (0, 0, 255)) && blank(&pic, 60.0, 20.0));
    assert_eq!(pic.r.count(SkipKind::MetafileRecordSkipped), 1, "{:?}", pic.r.skipped);
}

#[test]
fn a_filled_rectangle_with_an_outline_of_the_pens_width() {
    // Class: SELECTOBJECT, CREATEBRUSHINDIRECT, EXTCREATEPEN-like CREATEPEN and RECTANGLE: a red rectangle with a black outline 4 pixels wide.
    let mut recs = vec![r(38, &[u32s(&[2, 0]), i32s(&[4, 0]), u32s(&[rgb(0, 0, 0)])].concat()), r(37, &u32s(&[2]))];
    recs.push(r(39, &u32s(&[1, 0, rgb(255, 0, 0), 0])));
    recs.push(r(37, &u32s(&[1])));
    recs.push(rect_record(10, 10, 50, 30));
    let pic = draw_meta(emf(&recs));
    assert!(is(&pic, 30.0, 20.0, (255, 0, 0)), "the inside is red");
    assert!(dark(&pic, 10.0, 20.0) && dark(&pic, 50.0, 20.0) && dark(&pic, 30.0, 10.0), "the outline is dark all round");
    assert!(blank(&pic, 5.0, 20.0) && blank(&pic, 60.0, 20.0), "and nothing is outside it");
}

#[test]
fn polygons_polylines_and_poly_polygons_are_drawn_as_16_bit_point_records() {
    let tri = |pts: [i16; 6]| {
        let mut f = i32s(&[0, 0, 80, 40]);
        f.extend(u32s(&[3]));
        f.extend(i16s(&pts));
        f
    };
    // POLYGON16: a red triangle with its right angle at the lower left.
    let mut recs = brush_no_pen(1, rgb(255, 0, 0));
    recs.push(r(86, &tri([0, 0, 0, 40, 40, 40])));
    let pic = draw_meta(emf(&recs));
    assert!(is(&pic, 5.0, 35.0, (255, 0, 0)), "inside the triangle");
    assert!(blank(&pic, 35.0, 5.0), "outside it");
    // POLYLINE16 with a pen of 4: a line along the middle.
    let mut recs = pen_no_brush(1, 4, rgb(0, 0, 255));
    let mut f = i32s(&[0, 0, 80, 40]);
    f.extend(u32s(&[2]));
    f.extend(i16s(&[5, 20, 75, 20]));
    recs.push(r(87, &f));
    let pic = draw_meta(emf(&recs));
    assert!(is(&pic, 40.0, 20.0, (0, 0, 255)) && blank(&pic, 40.0, 10.0) && blank(&pic, 40.0, 30.0));
    // POLYPOLYGON16: two squares, side by side, and the gap between them empty.
    let mut recs = brush_no_pen(1, rgb(0, 160, 0));
    let mut f = i32s(&[0, 0, 80, 40]);
    f.extend(u32s(&[2, 8]));
    f.extend(u32s(&[4, 4]));
    f.extend(i16s(&[0, 0, 20, 0, 20, 40, 0, 40, 60, 0, 80, 0, 80, 40, 60, 40]));
    recs.push(r(91, &f));
    let pic = draw_meta(emf(&recs));
    assert!(is(&pic, 10.0, 20.0, (0, 160, 0)) && is(&pic, 70.0, 20.0, (0, 160, 0)) && blank(&pic, 40.0, 20.0));
}

#[test]
fn a_path_bracket_is_filled_once_with_curves_in_it() {
    // BEGINPATH, MOVETOEX, POLYBEZIERTO16 (a lens), CLOSEFIGURE, ENDPATH, FILLPATH.
    let mut recs = brush_no_pen(1, rgb(255, 0, 255));
    recs.push(r(59, &[]));
    recs.push(r(27, &i32s(&[10, 20])));
    let mut f = i32s(&[0, 0, 80, 40]);
    f.extend(u32s(&[6]));
    f.extend(i16s(&[25, 0, 55, 0, 70, 20, 55, 40, 25, 40, 10, 20]));
    recs.push(r(88, &f));
    recs.push(r(61, &[]));
    recs.push(r(60, &[]));
    recs.push(r(62, &[]));
    let pic = draw_meta(emf(&recs));
    assert!(is(&pic, 40.0, 20.0, (255, 0, 255)), "the inside of the lens");
    assert!(blank(&pic, 40.0, 3.0) && blank(&pic, 3.0, 20.0), "outside the lens");
}

#[test]
fn a_clip_rectangle_cuts_what_follows_and_a_copy_with_no_data_resets_it() {
    // INTERSECTCLIPRECT then a copy with no region data, then a fill of the whole frame: the whole box.
    let mut recs = brush_no_pen(1, rgb(255, 0, 0));
    recs.push(r(30, &i32s(&[0, 0, 40, 40])));
    recs.push(rect_record(0, 0, 80, 40));
    let pic = draw_meta(emf(&recs));
    assert!(is(&pic, 20.0, 20.0, (255, 0, 0)) && blank(&pic, 60.0, 20.0), "cut to the left half");
    let mut recs = brush_no_pen(1, rgb(255, 0, 0));
    recs.push(r(30, &i32s(&[0, 0, 40, 40])));
    recs.push(r(75, &u32s(&[0, 5])));
    recs.push(rect_record(0, 0, 80, 40));
    let pic = draw_meta(emf(&recs));
    assert!(is(&pic, 20.0, 20.0, (255, 0, 0)) && is(&pic, 60.0, 20.0, (255, 0, 0)), "the reset clip lets the whole box through");
}

#[test]
fn a_path_clip_by_exclusion_cuts_a_hole() {
    // The EMF+ clip excludes a square (a frame with a hole): the odd-crossings rule of the clip is what makes the hole.
    let hole = p(0x4032, 0x0400, &f32s(&[20.0, 10.0, 40.0, 20.0]));
    let fill = p(0x400A, 0x8000, &[u32s(&[argb(0, 0, 0, 255), 1]), f32s(&[0.0, 0.0, 80.0, 40.0])].concat());
    let pic = draw_meta(emf(&[hp(), c(&[hole, fill]), end_plus()]));
    assert!(dark(&pic, 5.0, 5.0) && dark(&pic, 70.0, 35.0), "ink where the clip is");
    assert!(blank(&pic, 40.0, 20.0), "and none in the hole: {:?}", pic.at(2.0, 1.5));
}

#[test]
fn a_linear_gradient_of_emf_plus_runs_from_one_colour_to_the_other() {
    // A gradient brush of red to blue across the frame; the GDI half is masks, which are not drawn.
    let brush = p(
        0x4008,
        0x0101,
        &[u32s(&[0xDBC0_1002, 4, 0, 0]), f32s(&[0.0, 0.0, 80.0, 40.0]), u32s(&[argb(255, 0, 0, 255), argb(0, 0, 255, 255), 0, 0])].concat(),
    );
    let draw_it = p(0x400A, 0x0000, &[u32s(&[1, 1]), f32s(&[0.0, 0.0, 80.0, 40.0])].concat());
    let pic = draw_meta(emf(&[hp(), c(&[brush]), c(&[draw_it]), end_plus()]));
    let (xl, yl) = at(4.0, 20.0);
    let (xm, ym) = at(40.0, 20.0);
    let (xr, yr) = at(76.0, 20.0);
    let (l, m, rr) = (pic.at(xl, yl), pic.at(xm, ym), pic.at(xr, yr));
    assert!(l[0] > 200 && l[2] < 60, "red at the left: {l:?}");
    assert!(rr[2] > 200 && rr[0] < 60, "blue at the right: {rr:?}");
    assert!(m[0] > 90 && m[0] < 170 && m[2] > 90 && m[2] < 170, "a mix in the middle: {m:?}");
}

#[test]
fn a_pen_of_emf_plus_strokes_a_path_object() {
    // A path of two lines drawn with a pen of width 4 in blue (DrawPath).
    let mut path = u32s(&[0xDBC0_1002, 3, 0]);
    path.extend(f32s(&[5.0, 5.0, 75.0, 5.0, 75.0, 35.0]));
    path.extend([0, 1, 1]);
    let mut pen = u32s(&[0xDBC0_1002, 0, 0, 0]);
    pen.extend(f32s(&[4.0]));
    pen.extend(u32s(&[0xDBC0_1002, 0, argb(0, 0, 255, 255)]));
    let draw_it = p(0x4015, 0x0001, &u32s(&[2]));
    let pic = draw_meta(emf(&[hp(), c(&[p(0x4008, 0x0301, &path), p(0x4008, 0x0202, &pen), draw_it]), end_plus()]));
    assert!(is(&pic, 40.0, 5.0, (0, 0, 255)) && is(&pic, 75.0, 20.0, (0, 0, 255)), "the two sides are blue");
    assert!(blank(&pic, 40.0, 20.0) && blank(&pic, 10.0, 20.0), "and the middle is empty");
}

#[test]
fn a_bitmap_in_a_gdi_record_is_stretched_to_its_destination() {
    // STRETCHDIBITS of 2 by 2 pixels with SRCCOPY into the whole frame: red, green on the top row; blue, yellow below.
    let dib = dib_24(2, 2, &[[255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 0]]);
    let pic = draw_meta(emf(&[stretch_dibits([0, 0, 80, 40], &dib, 0x00CC_0020)]));
    assert!(is(&pic, 20.0, 10.0, (255, 0, 0)) && is(&pic, 60.0, 10.0, (0, 255, 0)));
    assert!(is(&pic, 20.0, 30.0, (0, 0, 255)) && is(&pic, 60.0, 30.0, (255, 255, 0)));
}

#[test]
fn a_mask_by_srcand_leaves_its_white_pixels_out() {
    // A white and black bitmap by SRCAND over the red fill under it: the white pixels let the red through, the black ones draw black.
    let mut recs = brush_no_pen(1, rgb(255, 0, 0));
    recs.push(rect_record(0, 0, 80, 40));
    recs.push(stretch_dibits([0, 0, 80, 40], &dib_24(2, 1, &[[255, 255, 255], [0, 0, 0]]), 0x0088_00C6));
    let pic = draw_meta(emf(&recs));
    assert!(is(&pic, 20.0, 20.0, (255, 0, 0)), "white leaves the red");
    assert!(dark(&pic, 60.0, 20.0), "black is drawn");
}

#[test]
fn a_metafile_inside_a_metafile_lands_in_the_parallelogram_of_the_draw() {
    // The nested metafile's frame is 100 by 50 pixels; its pixel (50, 25) lands at outer pixel (60, 35). The nested file fills a block
    // there, so the outer picture has the block at (55..65, 30..40) and nothing else.
    let mut inner = emf(&[r(39, &u32s(&[1, 0, rgb(255, 0, 0), 0])), r(37, &u32s(&[1])), r(37, &u32s(&[0x8000_0008])), rect_record(45, 20, 55, 30)]);
    // The nested frame is 100 by 50 pixels: Frame 2500 by 1250 in the header.
    inner[24..40].copy_from_slice(&i32s(&[0, 0, 2500, 1250]));
    let mut image = u32s(&[0xDBC0_1002, 2, 4, inner.len() as u32]);
    image.extend(&inner);
    let draw_it = p(
        0x401B,
        0x0001,
        &[u32s(&[0xFFFF_FFFF, 2]), f32s(&[0.0, 0.0, 100.0, 50.0]), u32s(&[3]), f32s(&[10.0, 10.0, 110.0, 10.0, 10.0, 60.0])].concat(),
    );
    let pic = draw_meta(emf(&[hp(), c(&[p(0x4008, 0x0501, &image)]), c(&[draw_it]), end_plus()]));
    assert!(is(&pic, 60.0, 35.0, (255, 0, 0)), "the block is at (60, 35)");
    assert!(blank(&pic, 30.0, 20.0) && blank(&pic, 80.0 - 1.0, 5.0));
}

#[test]
fn text_is_drawn_where_the_file_says_in_the_family_it_names() {
    // Hack is bundled: a face named by the file in another case is found. Four characters at advances of 20 pixels from x = 5.
    let recs = vec![font_record(1, -20, "HACK", 0), r(37, &u32s(&[1])), text_record([5, 30], "MMMM", &[20, 20, 20, 20], 0)];
    let pic = draw_meta(emf(&recs));
    let (x0, _, x1, _) = pic.ink_box().expect("ink");
    assert!(x0 >= 1.0 + 4.0 / 40.0 && x1 <= 1.0 + 80.0 / 40.0, "within x = 5 to 85: {x0} to {x1}");
    assert!(pic.ink_pixels() > 150, "letters were drawn");
    // The four letters stand 20 pixels apart: a gap between the first and the second, as the advances say.
    assert!(pic.r.skipped.iter().all(|s| s.kind != SkipKind::MetafileFontMissing), "{:?}", pic.r.skipped);
}

#[test]
fn text_in_a_symbol_font_that_cannot_be_found_is_not_drawn_and_is_reported() {
    // A symbol font (character set 2) the machine lacks is not drawn, and the row says font-missing.
    let recs = vec![font_record(1, -20, "Quillsym", 2), r(37, &u32s(&[1])), text_record([5, 30], "ab", &[], 0)];
    let pic = draw_meta(emf(&recs));
    assert_eq!(pic.ink_pixels(), 0);
    assert_eq!(pic.r.count(SkipKind::MetafileFontMissing), 1, "{:?}", pic.r.skipped);
}

#[test]
fn text_of_glyph_indices_is_not_drawn_and_is_reported() {
    // V21.
    let recs = vec![font_record(1, -20, "Hack", 0), r(37, &u32s(&[1])), text_record([5, 30], "ab", &[], 0x10)];
    let pic = draw_meta(emf(&recs));
    assert_eq!(pic.ink_pixels(), 0);
    assert_eq!(pic.r.count(SkipKind::MetafileGlyphIndexText), 1, "{:?}", pic.r.skipped);
}

#[test]
fn a_wmf_is_drawn_in_its_window() {
    // A window of 200 by 100 (y stored first), a red rectangle over its right half.
    let recs = vec![
        w(0x020B, &i16s(&[0, 0])),
        w(0x020C, &i16s(&[100, 200])),
        w(0x02FC, &[u16s(&[0]), u32s(&[rgb(255, 0, 0)]), u16s(&[0])].concat()),
        w(0x012D, &u16s(&[0])),
        w(0x02FA, &[u16s(&[5]), i16s(&[1, 0]), u32s(&[0])].concat()),
        w(0x012D, &u16s(&[1])),
        // bottom, right, top, left
        w(0x041B, &i16s(&[100, 200, 0, 100])),
    ];
    let pic = draw_meta(wmf(&recs));
    assert!(is(&pic, 60.0, 20.0, (255, 0, 0)), "the right half is red");
    assert!(blank(&pic, 20.0, 20.0), "the left is empty");
}

#[test]
fn a_picture_is_stretched_to_its_box_on_each_axis_and_cut_to_its_frame() {
    // The 80 by 40 frame on a 2 by 2 inch box; a fill of the frame's top half covers the box's top half, and a fill that
    // leaves the frame does not leave the box.
    let mut recs = brush_no_pen(1, rgb(255, 0, 0));
    recs.push(rect_record(-200, -200, 280, 20));
    let doc = one_page(vec![picture(1.0, 1.0, 2.0, 2.0, foreign("EnhMetaFile", "image/x-emf", emf(&recs)))]);
    let pic = draw(&doc);
    assert!(pic.is_red(2.0, 2.5) && pic.is_white(2.0, 1.5), "the top half only");
    assert!(pic.is_white(0.8, 2.5) && pic.is_white(3.2, 2.5) && pic.is_white(2.0, 3.2), "and nothing outside the box");
}

#[test]
fn a_picture_flips_with_its_shape() {
    let mut recs = brush_no_pen(1, rgb(255, 0, 0));
    recs.push(rect_record(0, 0, 40, 40));
    let sb = picture(1.0, 1.0, 2.0, 1.0, foreign("EnhMetaFile", "image/x-emf", emf(&recs)))
        .cell("FlipX", "1")
        .cell("PinX", "2")
        .cell("LocPinX", "1")
        .cell("PinY", "1.5")
        .cell("LocPinY", "0.5");
    let pic = draw(&one_page(vec![sb]));
    assert!(is(&pic, 60.0, 20.0, (255, 0, 0)) && blank(&pic, 20.0, 20.0), "FlipX puts the left half on the right");
}

#[test]
fn a_pen_under_a_device_pixel_is_drawn_one_pixel_wide_in_its_own_colour() {
    // For the pens of a picture: a pen of 1 frame pixel is half a pixel at 20 to the inch (the frame has 40), and would
    // come out a grey of half the pen's black. A POLYLINE16 along the middle of the frame, with the pen of 1.
    let mut recs = pen_no_brush(1, 1, rgb(0, 0, 0));
    let mut f = i32s(&[0, 0, 80, 40]);
    f.extend(u32s(&[2]));
    f.extend(i16s(&[5, 20, 75, 20]));
    recs.push(r(87, &f));
    let doc = doc_of(emf(&recs));
    let view = diacraft_render::View::fit_page(&doc, 1, 20.0).expect("a size");
    let rendered = Renderer::new().render(&doc, 1, &view);
    let width = rendered.width as usize;
    // The ink of the column through the middle of the line, in pixels of black.
    let ink: f64 = (0..rendered.height as usize).map(|y| 1.0 - f64::from(rendered.rgba_premul[(y * width + 40) * 4]) / 255.0).sum();
    assert!((ink - 1.0).abs() < 0.08, "a pixel of ink in all, not half of one: {ink}");
}

#[test]
fn a_header_no_reader_takes_keeps_the_placeholder() {
    // A type-1 record with the signature but a header that is cut short.
    let mut bytes = vec![0u8; 60];
    bytes[0] = 1;
    bytes[40..44].copy_from_slice(b" EMF");
    let pic = draw_meta(bytes);
    assert_eq!(pic.r.count(SkipKind::MetafilePlaceholder), 1, "{:?}", pic.r.skipped);
}

#[test]
fn a_record_of_the_wrong_size_cuts_the_picture_short_and_says_so() {
    let mut bytes = emf(&[brush_no_pen(1, rgb(255, 0, 0)), vec![rect_record(0, 0, 40, 40)]].concat());
    bytes.truncate(bytes.len() - 20);
    bytes.extend(u32s(&[14, 6]));
    let pic = draw_meta(bytes);
    assert!(is(&pic, 20.0, 20.0, (255, 0, 0)), "what came before is drawn");
    assert_eq!(pic.r.count(SkipKind::MetafileCutShort), 1, "{:?}", pic.r.skipped);
}

#[test]
fn a_picture_that_reaches_a_cap_is_drawn_as_far_as_it_got_and_says_so() {
    // 2 million SAVEDC records: the depth cap is reached, the fill after them is drawn, and one row says cap-reached.
    let mut recs = brush_no_pen(1, rgb(255, 0, 0));
    recs.extend((0..2_000_000).map(|_| r(33, &[])));
    recs.push(rect_record(0, 0, 80, 40));
    let started = std::time::Instant::now();
    let pic = draw_meta(emf(&recs));
    // A hang guard and no measure of cost: the cap on saved states is what ends the walk (reported below), and a minute is generous for a
    // machine at load 50.
    assert!(started.elapsed().as_secs_f64() < 60.0, "took {:?}", started.elapsed());
    assert!(is(&pic, 20.0, 20.0, (255, 0, 0)));
    assert_eq!(pic.r.count(SkipKind::MetafileCapReached), 1, "{:?}", pic.r.skipped);
}

#[test]
fn a_metafile_is_kept_between_frames_and_drawn_again_without_reading_it_again() {
    let doc = doc_of(emf(&dual_blue_left()));
    let mut renderer = Renderer::new();
    let first = draw_with(&mut renderer, &doc);
    let second = draw_with(&mut renderer, &doc);
    assert_eq!(first.r.rgba_premul, second.r.rgba_premul);
}

// ---- text through the font database ----

/// A renderer whose database also holds faces built in code: box glyphs 0.45 of an em wide and 0.7 high from the origin, 600 units to the
/// advance, `ascent` and `descent` as given.
fn renderer_with(faces: &[(&str, i16, i16)]) -> Renderer {
    let mut db = diacraft_fonts::FontDb::bundled();
    for (name, ascent, descent) in faces {
        let mut face = diacraft_fonts::synth::SynthFont::new(name).ascii(600);
        face.ascent = *ascent;
        face.descent = *descent;
        db.add_font_data(face.build());
    }
    Renderer::with_fonts(db)
}

#[test]
fn the_letters_stand_where_the_advances_say_whatever_the_face_is() {
    // Height -10 (an em of 10 pixels), baseline and centre (0x1E), reference (40, 30), advances 6, 12 and 18: the letters start at 22, 28 and
    // 40 on the baseline y = 30, boxes 4.5 wide and 7 high.
    let recs = vec![font_record(1, -10, "quillmono", 0), r(37, &u32s(&[1])), r(22, &u32s(&[0x1E])), text_record([40, 30], "AAA", &[6, 12, 18], 0)];
    let pic = draw_with(&mut renderer_with(&[("Quillmono", 800, -200)]), &doc_of(emf(&recs)));
    for u in [24.0, 30.0, 42.0] {
        assert!(dark(&pic, u, 27.0), "a letter at {u}");
    }
    assert!(blank(&pic, 35.0, 27.0) && blank(&pic, 18.0, 27.0) && blank(&pic, 47.0, 27.0), "and none between or beside them");
    assert!(blank(&pic, 24.0, 33.0) && blank(&pic, 24.0, 20.0), "sitting on the baseline: nothing below it or above the box");
}

#[test]
fn a_positive_height_is_the_cell_and_a_negative_one_the_em() {
    // A face of 1.2 ems from ascent to descent: height +24 is an em of 20 pixels, height -24 an em of 24. The box is 0.7 of an em high.
    let tall = |height: i32| {
        let recs = vec![font_record(1, height, "Quillsans", 0), r(37, &u32s(&[1])), r(22, &u32s(&[0x18])), text_record([10, 36], "A", &[], 0)];
        let pic = draw_with(&mut renderer_with(&[("Quillsans", 900, -300)]), &doc_of(emf(&recs)));
        let b = pic.ink_box().expect("a letter");
        (b.3 - b.1) * 40.0
    };
    let (cell, em) = (tall(24), tall(-24));
    assert!((cell - 14.0).abs() < 1.0, "an em of 20: a box of 14 pixels, got {cell}");
    assert!((em - 16.8).abs() < 1.0, "an em of 24: a box of 16.8 pixels, got {em}");
}

#[test]
fn a_driver_string_is_found_in_the_family_ignoring_case_and_placed_at_its_positions() {
    // QUILLMONO in the file, Quillmono installed; codes 0x41 and 0x42 (Unicode), positions (10, 20) and (18, 20), an em of 12 pixels.
    let mut font = u32s(&[0xDBC0_1002]);
    font.extend(f32s(&[12.0]));
    font.extend(u32s(&[2, 0, 0, 9]));
    font.extend(u16s(&"QUILLMONO".encode_utf16().collect::<Vec<_>>()));
    let mut text = u32s(&[0xFF00_0000, 1, 0, 2]);
    text.extend(u16s(&[0x41, 0x42]));
    text.extend(f32s(&[10.0, 20.0, 18.0, 20.0]));
    let draw_it = p(0x4036, 0x8001, &text);
    let doc = doc_of(emf(&[hp(), c(&[p(0x4008, 0x0601, &font), draw_it]), end_plus()]));
    let pic = draw_with(&mut renderer_with(&[("Quillmono", 800, -200)]), &doc);
    // Its glyphs are solid boxes: the corner just inside each box is ink, and between the boxes there is none.
    assert!(dark(&pic, 10.7, 12.3) && dark(&pic, 18.7, 12.3), "a box at each position");
    assert!(blank(&pic, 16.5, 16.0), "and a gap between");
    assert!(pic.r.skipped.is_empty(), "{:?}", pic.r.skipped);
}

// ---- bitmaps in EMF+ ----

fn png_2x2() -> Vec<u8> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, 2, 2);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let data: Vec<u8> = [[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255], [255, 255, 0, 255]].concat();
    encoder.write_header().unwrap().write_image_data(&data).unwrap();
    out
}

/// An EMF+ image object of a bitmap (object 1) and a `DrawImagePoints` of its source rectangle `src` into the frame `[0, 0, 80, 40]`.
fn image_file(image: Vec<u8>, src: [f32; 4]) -> Vec<u8> {
    let draw_it = p(0x401B, 0x0001, &[u32s(&[0xFFFF_FFFF, 2]), f32s(&src), u32s(&[3]), f32s(&[0.0, 0.0, 80.0, 0.0, 0.0, 40.0])].concat());
    emf(&[hp(), c(&[p(0x4008, 0x0501, &image)]), c(&[draw_it]), end_plus()])
}

#[test]
fn a_compressed_bitmap_of_emf_plus_is_decoded_by_its_bytes_and_stretched() {
    // A PNG object (compressed, type 1): the format is told by the bytes.
    let mut image = u32s(&[0xDBC0_1002, 1]);
    image.extend(i32s(&[2, 2, 0]));
    image.extend(u32s(&[0x0026_200A, 1]));
    image.extend(png_2x2());
    let pic = draw_meta(image_file(image, [0.0, 0.0, 2.0, 2.0]));
    assert!(is(&pic, 20.0, 10.0, (255, 0, 0)) && is(&pic, 60.0, 10.0, (0, 255, 0)));
    assert!(is(&pic, 20.0, 30.0, (0, 0, 255)) && is(&pic, 60.0, 30.0, (255, 255, 0)));
    assert!(pic.r.skipped.is_empty(), "{:?}", pic.r.skipped);
}

#[test]
fn a_source_rectangle_draws_only_that_part_of_the_bitmap() {
    // The right column of the PNG, 1 by 2 pixels, stretched over the frame: green above, yellow below, and no red or blue where the
    // column's own pixels are (the edges, a pixel's width, are blended with the pixels beside the source rectangle).
    let mut image = u32s(&[0xDBC0_1002, 1]);
    image.extend(i32s(&[2, 2, 0]));
    image.extend(u32s(&[0x0026_200A, 1]));
    image.extend(png_2x2());
    let pic = draw_meta(image_file(image, [1.0, 0.0, 1.0, 2.0]));
    assert!(is(&pic, 40.0, 10.0, (0, 255, 0)) && is(&pic, 40.0, 30.0, (255, 255, 0)));
    assert!(!is(&pic, 40.0, 10.0, (255, 0, 0)) && !is(&pic, 40.0, 30.0, (0, 0, 255)));
}

#[test]
fn raw_pixels_of_emf_plus_are_drawn_with_their_alpha() {
    // 32 bits, ARGB: one opaque red pixel, stretched over the frame; and a half clear one beside it that shows the paper.
    let mut image = u32s(&[0xDBC0_1002, 1]);
    image.extend(i32s(&[2, 1, 8]));
    image.extend(u32s(&[0x0026_200A, 0]));
    image.extend([0, 0, 255, 255, 0, 0, 255, 0]);
    let pic = draw_meta(image_file(image, [0.0, 0.0, 2.0, 1.0]));
    assert!(is(&pic, 20.0, 20.0, (255, 0, 0)), "opaque");
    assert!(blank(&pic, 60.0, 20.0), "clear");
}

#[test]
fn an_image_that_is_not_an_image_makes_a_dual_file_play_its_other_half() {
    // An object that cannot be played, in a Dual file, gives the picture to the EMF records.
    let mut image = u32s(&[0xDBC0_1002, 1]);
    image.extend(i32s(&[2, 2, 0]));
    image.extend(u32s(&[0x0026_200A, 1]));
    image.extend(b"not an image at all");
    let mut recs = vec![hp(), c(&[p(0x4008, 0x0501, &image)])];
    recs.push(c(&[p(
        0x401B,
        0x0001,
        &[u32s(&[0xFFFF_FFFF, 2]), f32s(&[0.0, 0.0, 2.0, 2.0]), u32s(&[3]), f32s(&[0.0, 0.0, 80.0, 0.0, 0.0, 40.0])].concat(),
    )]));
    recs.extend(gdi_red_right_half());
    let pic = draw_meta(emf(&recs));
    assert!(is(&pic, 60.0, 20.0, (255, 0, 0)));
    assert_eq!(pic.r.count(SkipKind::MetafileGdiFallback), 1, "{:?}", pic.r.skipped);
}

#[test]
fn an_emf_plus_header_that_is_not_where_one_belongs_plays_the_file_as_plain_emf_and_says_so() {
    // A record before the EMF+ comment. The plain records draw (red on the right); the comment is skipped.
    let mut recs = vec![r(25, &u32s(&[rgb(255, 255, 255)]))];
    recs.push(hp());
    recs.push(plus_fill_rect(argb(0, 0, 255, 255), [0.0, 0.0, 40.0, 40.0]));
    recs.extend(gdi_red_right_half());
    let pic = draw_meta(emf(&recs));
    assert!(blank(&pic, 20.0, 20.0) && is(&pic, 60.0, 20.0, (255, 0, 0)));
    assert_eq!(pic.r.count(SkipKind::MetafileEmfPlusMisplaced), 1, "{:?}", pic.r.skipped);
}

#[test]
fn a_page_with_a_metafile_exports_to_png_with_the_picture_in_it() {
    let doc = doc_of(emf(&dual_blue_left()));
    let out = diacraft_render::export::export_png(&mut Renderer::new(), &doc, 1, 100.0).expect("exports");
    assert!(out.skipped.is_empty(), "{:?}", out.skipped);
    let decoder = png::Decoder::new(std::io::Cursor::new(out.bytes));
    let mut reader = decoder.read_info().unwrap();
    let mut buf = vec![0u8; reader.output_buffer_size().unwrap()];
    let frame = reader.next_frame(&mut buf).unwrap();
    let width = frame.width as usize;
    let px = |x: f64, y: f64| {
        let (col, row) = ((x * 100.0) as usize, ((4.0 - y) * 100.0) as usize);
        let i = (row * width + col) * frame.color_type.samples();
        [buf[i], buf[i + 1], buf[i + 2]]
    };
    assert_eq!(px(1.5, 1.5), [0, 0, 255], "blue on the left half of the box");
    assert_eq!(px(2.5, 1.5), [255, 255, 255], "paper on the right");
}

// ---- hostile: bitmaps that each pass and together do not ----

/// A PNG of one grey, `side` by `side` pixels: a few tens of kilobytes for tens of millions of pixels.
fn solid_png(side: u32, grey: u8) -> Vec<u8> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, side, side);
    encoder.set_color(png::ColorType::Grayscale);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Fast);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&vec![grey; side as usize * side as usize]).unwrap();
    drop(writer);
    out
}

#[test]
fn many_bitmaps_each_under_the_cap_are_decoded_only_while_the_pictures_total_is_under_it() {
    // Four image objects, each a PNG of 6,300 by 6,300 pixels (39.7 million: under the cap of one bitmap), each drawn into a quarter of the
    // frame. Together they are 159 million pixels, 635 MB once decoded, past the 64 million a picture decodes: the first is decoded and
    // drawn, the others are not decoded, and the one row of the picture says a cap was reached.
    let png_bytes = solid_png(6_300, 40);
    let mut recs = vec![hp()];
    for id in 1..=4u16 {
        let mut image = u32s(&[0xDBC0_1002, 1]);
        image.extend(i32s(&[6_300, 6_300, 0]));
        image.extend(u32s(&[0x0026_200A, 1]));
        image.extend(&png_bytes);
        recs.push(c(&[p(0x4008, 0x0500 | id, &image)]));
    }
    let quarter = |id: u16, x: f32, y: f32| {
        let pts = [x, y, x + 40.0, y, x, y + 20.0];
        c(&[p(0x401B, id, &[u32s(&[0xFFFF_FFFF, 2]), f32s(&[0.0, 0.0, 6_300.0, 6_300.0]), u32s(&[3]), f32s(&pts)].concat())])
    };
    recs.extend([quarter(1, 0.0, 0.0), quarter(2, 40.0, 0.0), quarter(3, 0.0, 20.0), quarter(4, 40.0, 20.0)]);
    recs.push(end_plus());
    let doc = doc_of(emf(&recs));
    // No limit on time: what is asked is the cap on pixels, which holds whatever the time (a loaded machine would cut the frame short).
    let mut renderer = Renderer::new();
    renderer.set_limits(diacraft_render::Limits::unlimited_time());
    let started = std::time::Instant::now();
    let pic = draw_with(&mut renderer, &doc);
    let took = started.elapsed();
    eprintln!("hostile: four bitmaps of 39.7 million pixels drawn in {took:?}");
    assert!(dark(&pic, 20.0, 10.0), "the first bitmap is drawn");
    assert!(blank(&pic, 60.0, 10.0) && blank(&pic, 20.0, 30.0) && blank(&pic, 60.0, 30.0), "the others are not decoded: {:?}", pic.at(2.5, 1.75));
    assert_eq!(pic.r.count(SkipKind::MetafileCapReached), 1, "{:?}", pic.r.skipped);
    // Counted, not timed: "one bitmap's decode and no more" is one image decoded, whatever the load on the machine.
    assert_eq!(renderer.images_decoded(), 1, "one bitmap's decode and no more");
    // A hang guard, not a measure of cost.
    assert!(took.as_secs_f64() < 60.0, "took {took:?}");
}

#[test]
fn a_million_items_are_read_and_drawn_inside_the_frames_time_and_the_picture_says_so() {
    // 1.2 million RECTANGLE records (the item cap is a million): read in full, drawn until the frame's time runs out, and the row says a cap
    // was reached. The page is not held for minutes.
    let mut recs = brush_no_pen(1, rgb(255, 0, 0));
    recs.extend((0..1_200_000).map(|i| rect_record(i % 70, i % 30, i % 70 + 10, i % 30 + 10)));
    let bytes = emf(&recs);
    let started = std::time::Instant::now();
    let items = diacraft_metafile::parse(&bytes).unwrap().items.len();
    let read = started.elapsed();
    let started = std::time::Instant::now();
    let pic = draw_meta(bytes);
    let took = started.elapsed();
    eprintln!("hostile: {items} items read in {read:?}; read and drawn by the renderer in {took:?}");
    assert_eq!(items, 1_000_000, "the item cap");
    // A hang guard, not a measure of cost: the frame's own time limit ends the drawing of a million items (the cap reached is reported
    // below), and a minute is generous for a machine at load 50 that read and drew them in four and a half seconds.
    assert!(took.as_secs_f64() < 60.0, "took {took:?}");
    assert!(pic.r.count(SkipKind::MetafileCapReached) == 1, "{:?}", pic.r.skipped);
}

#[test]
fn a_picture_whose_bitmaps_need_several_frames_time_finishes_within_a_bounded_number_of_frames_and_stays_kept() {
    // Four bitmaps of 3,000 by 3,000 pixels (9 million each) and a frame of 20 milliseconds: each frame decodes the bitmaps it has time for,
    // at least one, from where the last stopped, and the picture is whole in at most four frames more than the last bitmap needs to be
    // drawn in. After that a frame draws it without decoding anything, and it is the same picture.
    let png_bytes = solid_png(3_000, 40);
    let mut recs = vec![hp()];
    for id in 1..=4u16 {
        let mut image = u32s(&[0xDBC0_1002, 1]);
        image.extend(i32s(&[3_000, 3_000, 0]));
        image.extend(u32s(&[0x0026_200A, 1]));
        image.extend(&png_bytes);
        recs.push(c(&[p(0x4008, 0x0500 | id, &image)]));
    }
    let quarter = |id: u16, x: f32, y: f32| {
        let pts = [x, y, x + 40.0, y, x, y + 20.0];
        c(&[p(0x401B, id, &[u32s(&[0xFFFF_FFFF, 2]), f32s(&[0.0, 0.0, 3_000.0, 3_000.0]), u32s(&[3]), f32s(&pts)].concat())])
    };
    recs.extend([quarter(1, 0.0, 0.0), quarter(2, 40.0, 0.0), quarter(3, 0.0, 20.0), quarter(4, 40.0, 20.0)]);
    recs.push(end_plus());
    let doc = doc_of(emf(&recs));
    let mut renderer = Renderer::new();
    renderer.set_limits(diacraft_render::Limits { frame_time: Some(std::time::Duration::from_millis(20)), ..renderer.limits() });
    let mut whole = None;
    for frame in 1..=8 {
        let pic = draw_with(&mut renderer, &doc);
        let all = [(20.0, 10.0), (60.0, 10.0), (20.0, 30.0), (60.0, 30.0)].iter().all(|(u, v)| dark(&pic, *u, *v));
        if all && pic.r.skipped.is_empty() {
            whole = Some((frame, pic));
            break;
        }
    }
    let (frame, pic) = whole.expect("whole within 8 frames");
    eprintln!("hostile: four bitmaps of 9 million pixels, whole in {frame} frames of 20 ms");
    assert!(frame <= 6, "four bitmaps, at least one decoded a frame, and a frame to draw them in: {frame}");
    // Kept: the next frame decodes nothing and draws the same picture. That is a count of the renderer's decodes, not a time: the four
    // bitmaps were each decoded once on the way to a whole picture, and a frame that draws the kept picture leaves the count where it was.
    // The frame has all the time it wants, so that a bitmap left to decode would be decoded in it and move the count (under the 20 ms of
    // the frames above, a frame cut short before it reached a bitmap would leave the count alone and prove nothing). The one time check
    // left is a hang guard in seconds.
    assert_eq!(renderer.images_decoded(), 4, "each of the four bitmaps was decoded once to make the picture whole");
    renderer.set_limits(diacraft_render::Limits::unlimited_time());
    let started = std::time::Instant::now();
    let again = draw_with(&mut renderer, &doc);
    assert!(started.elapsed() < std::time::Duration::from_secs(60), "a frame that only draws kept bitmaps took {:?}", started.elapsed());
    assert_eq!(renderer.images_decoded(), 4, "a kept picture is drawn without decoding");
    assert!(again.r.skipped.is_empty(), "nothing is left out: {:?}", again.r.skipped);
    assert_eq!(again.r.rgba_premul, pic.r.rgba_premul);
}

/// An EMF+ file of one PNG of `side` by `side` pixels (grey `grey`) drawn into the whole frame.
fn one_png_file(side: u32, grey: u8) -> Vec<u8> {
    let mut image = u32s(&[0xDBC0_1002, 1]);
    image.extend(i32s(&[side as i32, side as i32, 0]));
    image.extend(u32s(&[0x0026_200A, 1]));
    image.extend(solid_png(side, grey));
    image_file(image, [0.0, 0.0, side as f32, side as f32])
}

#[test]
fn many_pictures_on_a_page_cost_the_frame_its_time_and_one_decode_and_not_one_decode_each() {
    // Twelve pictures, each its own metafile with a PNG of 3,000 by 3,000 pixels, on frames of 5 ms, which one decode outlasts by far. What
    // is asserted is counted, by the renderer's count of the images it has decoded and by the frames' reports, and not measured in
    // milliseconds: a loaded machine slows the decodes and the frames alike, and the counts do not move.
    //   * A frame decodes the picture it reaches and stops there, so it decodes at most one of the twelve, and says that its time ran out.
    //   * The pictures after it are reported as left out of the frame (the ones that have not had their turn).
    //   * When the twelve are decoded, the frames after decode nothing, and a frame with all the time it wants draws every picture whole.
    const PICTURES: u64 = 12;
    let shapes: Vec<_> = (0..PICTURES as u32)
        .map(|i| {
            let (col, row) = (f64::from(i % 4), f64::from(i / 4));
            let data = foreign("EnhMetaFile", "image/x-emf", one_png_file(3_000, 50 + i as u8));
            geometry_of(shape_at(i + 1, 0.05 + col, 0.2 + row * 1.2, 0.9, 0.9), rect_rows(0.9, 0.9))
                .cell("FillPattern", "0")
                .cell("LinePattern", "0")
                .foreign(data)
        })
        .collect();
    let doc = one_page(shapes);
    let mut renderer = Renderer::new();
    assert_eq!(renderer.images_decoded(), 0, "nothing is decoded before a frame asks");
    renderer.set_limits(diacraft_render::Limits { frame_time: Some(std::time::Duration::from_millis(5)), ..renderer.limits() });
    // A frame that is cut short before it reaches the next picture decodes nothing, and the next one goes on: the number of frames is
    // capped by a count, not by a time, so that a picture that is never reached ends the test and not the run.
    let mut frames = 0u32;
    while renderer.images_decoded() < PICTURES {
        frames += 1;
        assert!(frames <= 400, "{frames} frames and {} of {PICTURES} decoded", renderer.images_decoded());
        let before = renderer.images_decoded();
        let started = std::time::Instant::now();
        let pic = draw_with(&mut renderer, &doc);
        // The one time check left, a hang guard in seconds: a frame is a decode and a few draws, and never this long.
        assert!(started.elapsed() < std::time::Duration::from_secs(60), "frame {frames} took {:?}", started.elapsed());
        let decoded = renderer.images_decoded() - before;
        assert!(decoded <= 1, "frame {frames} decoded {decoded} pictures, and a frame stops at the first: {:?}", pic.r.skipped);
        let waiting = PICTURES - renderer.images_decoded();
        assert!(
            pic.r.count(SkipKind::FrameBudget) as u64 >= waiting,
            "frame {frames}: the {waiting} pictures without a turn are left out and said so: {:?}",
            pic.r.skipped
        );
        if decoded == 1 {
            assert!(pic.r.count(SkipKind::MetafileCapReached) >= 1, "frame {frames} decoded a picture and its time ran out: {:?}", pic.r.skipped);
        }
    }
    // After that every picture is whole and kept: no frame decodes again, short of time or not.
    for _ in 0..3 {
        draw_with(&mut renderer, &doc);
        assert_eq!(renderer.images_decoded(), PICTURES, "a picture kept is not decoded again");
    }
    renderer.set_limits(diacraft_render::Limits::unlimited_time());
    let last = draw_with(&mut renderer, &doc);
    assert_eq!(renderer.images_decoded(), PICTURES, "each picture was decoded once and no more");
    assert!(last.r.skipped.is_empty(), "a frame with all its time draws every picture and leaves out nothing: {:?}", last.r.skipped);
    for i in 0..PICTURES as u8 {
        let (col, row) = (f64::from(i % 4), f64::from(i / 4));
        let grey = 50 + i;
        assert!(
            last.is(0.5 + col, 0.65 + row * 1.2, (grey, grey, grey), 8),
            "picture {i} is drawn in grey {grey}: {:?}",
            last.at(0.5 + col, 0.65 + row * 1.2)
        );
    }
}
