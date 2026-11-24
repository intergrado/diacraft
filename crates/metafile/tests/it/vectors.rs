//! The test vectors, as data: each builds the bytes the spec composes and asserts what it expects.
//! The assertions are on the display list in frame coordinates (device pixels from the frame's top-left corner).

use diacraft_metafile::{Detail, Error, Format, Paint, Placement, parse, sniff};

use crate::build::*;
use crate::inspect::*;

/// `H`, `HP`, a blue square in EMF+, then the GDI half (a red brush on the right half).
fn records_3(extra_before_fill: Option<Vec<u8>>) -> Vec<Vec<u8>> {
    let blue = argb(0, 0, 255, 255);
    let mut recs = vec![hp()];
    if let Some(e) = extra_before_fill {
        recs.push(e);
    }
    recs.push(plus_fill_rect(blue, [0.0, 0.0, 40.0, 40.0]));
    recs.extend(gdi_red_right_half());
    recs.push(c(&[p(0x4002, 0, &[])]));
    recs
}

#[test]
fn the_decoder_comes_from_the_bytes_and_not_from_a_label() {
    // A WMF under an EMF label, an EMF under a WMF label, a PNG under either: only the bytes are asked.
    let wmf_bytes = [0x01, 0x00, 0x09, 0x00, 0x06, 0x00];
    assert_eq!(sniff(&wmf_bytes), Some(Format::Wmf));
    let mut emf_bytes = vec![0u8; 100];
    emf_bytes[0] = 1;
    emf_bytes[40..44].copy_from_slice(&[0x20, 0x45, 0x4D, 0x46]);
    assert_eq!(sniff(&emf_bytes), Some(Format::Emf));
    let png = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    assert_eq!(sniff(&png), None);
    assert_eq!(parse(&png).unwrap_err(), Error::NotMetafile);
    // Placeable and disk WMF keys.
    assert_eq!(sniff(&[0xD7, 0xCD, 0xC6, 0x9A]), Some(Format::Wmf));
    assert_eq!(sniff(&[0x02, 0x00, 0x09, 0x00]), Some(Format::Wmf));
    assert_eq!(sniff(&[]), None);
}

#[test]
fn the_header_is_not_as_long_as_its_size() {
    // Size 116, nDescription 14, offDescription 88, then 28 bytes of code units 0x0051, 0x0037 repeated: an 88-byte header.
    let mut bytes = emf(&[rect_fill_record()]);
    // Rebuild the header record with Size 116: the description sits in bytes 88 to 116.
    let mut head = bytes[..108].to_vec();
    head[4..8].copy_from_slice(&116u32.to_le_bytes());
    head[60..64].copy_from_slice(&14u32.to_le_bytes());
    head[64..68].copy_from_slice(&88u32.to_le_bytes());
    let desc: Vec<u8> = (0..14).flat_map(|i| if i % 2 == 0 { 0x0051u16 } else { 0x0037 }.to_le_bytes()).collect();
    // The bytes of 88 to 108 of the original header are overwritten by the description, which then runs on to 116.
    head[88..108].copy_from_slice(&desc[..20]);
    head.extend_from_slice(&desc[20..]);
    let rest = bytes.split_off(108);
    let mut file = head;
    file.extend(rest);
    let pic = parse(&file).unwrap();
    assert!((pic.width - 80.0).abs() < 0.01 && (pic.height - 40.0).abs() < 0.01, "{} by {}", pic.width, pic.height);
    // The fields past 88 are description text, and must not be read as a pixel format or micrometres.
    assert_eq!(fills(&pic).len(), 1);
}

fn rect_fill_record() -> Vec<u8> {
    r(43, &i32s(&[0, 0, 40, 40]))
}

#[test]
fn an_emf_plus_file_plays_the_emf_plus_half_even_when_it_is_dual() {
    let pic = parse(&emf(&records_3(None))).unwrap();
    let f = fills(&pic);
    assert_eq!(f.len(), 1, "{f:?}");
    assert!(bbox_is(f[0].0, [0.0, 0.0, 40.0, 40.0]) && f[0].1 == BLUE, "the left half is blue and the right half is empty: {f:?}");
    assert!(pic.report.is_empty(), "{:?}", pic.report);
    assert_eq!(pic.kind, diacraft_metafile::Kind::EmfPlus);
}

#[test]
fn a_getdc_section_is_ordinary_emf() {
    // C(P(0x4004, 0)) just before the brush: the brush, the two selections and the rectangle are one section.
    let blue = argb(0, 0, 255, 255);
    let mut recs = vec![hp(), plus_fill_rect(blue, [0.0, 0.0, 40.0, 40.0]), c(&[p(0x4004, 0, &[])])];
    recs.extend(gdi_red_right_half());
    recs.push(c(&[p(0x4002, 0, &[])]));
    let pic = parse(&emf(&recs)).unwrap();
    let f = fills(&pic);
    assert_eq!(f.len(), 2, "{f:?}");
    assert!(f.iter().any(|(b, c)| bbox_is(*b, [0.0, 0.0, 40.0, 40.0]) && *c == BLUE));
    assert!(f.iter().any(|(b, c)| bbox_is(*b, [40.0, 0.0, 80.0, 40.0]) && *c == RED), "red on the right: {f:?}");
}

#[test]
fn with_only_the_rectangle_in_the_section_it_draws_with_the_starting_brush_and_pen() {
    // The GetDC comment just before the rectangle: the brush and selections before it were skipped and not applied, so the
    // rectangle is filled with the white brush and stroked by its black pen.
    let blue = argb(0, 0, 255, 255);
    let g = gdi_red_right_half();
    let mut recs = vec![hp(), plus_fill_rect(blue, [0.0, 0.0, 40.0, 40.0])];
    recs.extend(g[..3].iter().cloned());
    recs.push(c(&[p(0x4004, 0, &[])]));
    recs.push(g[3].clone());
    recs.push(c(&[p(0x4002, 0, &[])]));
    let pic = parse(&emf(&recs)).unwrap();
    let ink = inks(&pic);
    assert_eq!(ink.len(), 2, "{ink:?}");
    assert_eq!(ink[1].fill, Some([255, 255, 255, 255]), "The white brush, not the skipped red one");
    assert_eq!(ink[1].stroke, Some([0, 0, 0, 255]), "and its black pen");
}

#[test]
fn any_emf_plus_record_ends_a_getdc_section() {
    // As V4, with a SetAntiAliasMode record just before the rectangle: the rectangle falls outside the section.
    let blue = argb(0, 0, 255, 255);
    let g = gdi_red_right_half();
    let mut recs = vec![hp(), plus_fill_rect(blue, [0.0, 0.0, 40.0, 40.0]), c(&[p(0x4004, 0, &[])])];
    recs.extend(g[..3].iter().cloned());
    recs.push(c(&[p(0x401E, 0x0001, &[])]));
    recs.push(g[3].clone());
    recs.push(c(&[p(0x4002, 0, &[])]));
    let pic = parse(&emf(&recs)).unwrap();
    assert_eq!(fills(&pic).len(), 1, "the blue half only");
}

#[test]
fn a_type_no_reader_knows_makes_a_dual_file_play_its_emf_half() {
    let pic = parse(&emf(&records_3(Some(c(&[p(0x40F0, 0, &[])]))))).unwrap();
    let f = fills(&pic);
    assert_eq!(f.len(), 1, "the red half only, not both halves: {f:?}");
    assert!(bbox_is(f[0].0, [40.0, 0.0, 80.0, 40.0]) && f[0].1 == RED);
    assert_eq!(pic.report.first(), Some(Detail::GdiFallback));
    assert_eq!(pic.kind, diacraft_metafile::Kind::Emf);
}

#[test]
fn the_same_in_an_emf_plus_only_file_draws_what_it_can_and_reports_it() {
    let mut recs = records_3(Some(c(&[p(0x40F0, 0, &[])])));
    recs[0] = hp_flags(0x0000);
    let pic = parse(&emf(&recs)).unwrap();
    let f = fills(&pic);
    assert_eq!(f.len(), 1, "{f:?}");
    assert!(bbox_is(f[0].0, [0.0, 0.0, 40.0, 40.0]) && f[0].1 == BLUE, "the blue left half, no red");
    assert_eq!(pic.report.first(), Some(Detail::RecordSkipped));
}

#[test]
fn an_object_in_two_pieces_whose_first_data_word_is_the_total_size() {
    let green = 0xFF00_FF00u32;
    let first = p(0x4008, 0x8103, &[u32s(&[12]), u32s(&[0xDBC0_1002, 0])].concat());
    let second = p(0x4008, 0x8103, &[u32s(&[12]), u32s(&[green])].concat());
    let draw = p(0x400A, 0x0000, &[u32s(&[3, 1]), f32s(&[0.0, 0.0, 80.0, 40.0])].concat());
    let pic = parse(&emf(&[hp(), c(&[first]), c(&[second]), c(&[draw])])).unwrap();
    let f = fills(&pic);
    assert_eq!(f.len(), 1, "{f:?}");
    assert!(bbox_is(f[0].0, [0.0, 0.0, 80.0, 40.0]) && f[0].1 == GREEN, "the whole box is green: {f:?}");
}

/// A probe: a line from the logical point to itself, so that the display list says where the point lands.
fn probe(x: i32, y: i32) -> Vec<Vec<u8>> {
    vec![r(27, &i32s(&[x, y])), r(54, &i32s(&[x, y]))]
}

#[test]
fn the_window_origin_and_extent_with_its_sign_map_a_point() {
    let mut recs = vec![r(17, &u32s(&[8])), r(10, &i32s(&[100, 200])), r(9, &i32s(&[1000, -500])), r(12, &i32s(&[0, 40])), r(11, &i32s(&[80, 40]))];
    recs.extend(probe(600, 450));
    let pic = parse(&emf(&recs)).unwrap();
    let ink = inks(&pic);
    assert_eq!(ink.len(), 1);
    let pt = ink[0].points[0];
    assert!(near(pt[0], 40.0) && near(pt[1], 20.0), "logical (600, 450) is device (40, 20), the frame's centre: {pt:?}");
}

#[test]
fn a_matrix_put_first_by_modify_world_transform_mode_2() {
    // Set translate(10, 0), then mode 2 with scale(2, 1): x = 5 goes to 20 (mode 3 would give 30).
    let set = r(35, &f32s(&[1.0, 0.0, 0.0, 1.0, 10.0, 0.0]));
    let modify = |mode: u32| r(36, &[f32s(&[2.0, 0.0, 0.0, 1.0, 0.0, 0.0]), u32s(&[mode])].concat());
    for (mode, want) in [(2, 20.0), (3, 30.0)] {
        let mut recs = vec![set.clone(), modify(mode)];
        recs.extend(probe(5, 0));
        let pic = parse(&emf(&recs)).unwrap();
        let pt = inks(&pic)[0].points[0];
        assert!(near(pt[0], want), "mode {mode}: {pt:?}");
    }
}

#[test]
fn the_frame_and_not_bounds_is_the_picture() {
    // Bounds (10, 5, 69, 34), and a fill of device (40, 20) to (80, 40) under MM_TEXT.
    let head = Head { bounds: [10, 5, 69, 34], ..Head::default() };
    let pic = parse(&emf_with(&head, &[r(43, &i32s(&[40, 20, 80, 40]))])).unwrap();
    assert!(near(pic.width, 80.0) && near(pic.height, 40.0), "the frame is 80 by 40 device pixels");
    let f = fills(&pic);
    assert!(bbox_is(f[0].0, [40.0, 20.0, 80.0, 40.0]), "{f:?}");
}

#[test]
fn the_millimetre_form_of_the_resolution_is_the_one_used() {
    let head = Head { micrometers: [80_900, 40_000], ..Head::default() };
    let pic = parse(&emf_with(&head, &[rect_fill_record()])).unwrap();
    assert!(near(pic.width, 80.0), "80 device pixels by Millimeters, not 79.1 by the micrometres: {}", pic.width);
}

#[test]
fn emf_plus_coordinates_are_device_pixels_and_the_logical_resolution_does_not_rescale_them() {
    let head = Head { device: [2400, 1200], millimeters: [101, 50], bounds: [0, 0, 475, 239], ..Head::default() };
    let rect = plus_fill_rect(argb(0, 0, 0, 255), [0.0, 0.0, 237.6, 240.0]);
    let pic = parse(&emf_with(&head, &[hp(), rect])).unwrap();
    assert!((pic.width - 475.247).abs() < 0.01 && near(pic.height, 240.0), "{} by {}", pic.width, pic.height);
    let f = fills(&pic);
    assert!(bbox_is(f[0].0, [0.0, 0.0, 237.6, 240.0]), "the left half of the box, not scaled by 600 / 96: {f:?}");
}

#[test]
fn the_world_transform_matrices_apply_first_unless_bit_2000_asks_for_last() {
    let set = p(0x402A, 0, &f32s(&[1.0, 0.0, 0.0, 1.0, 10.0, 0.0]));
    for (flags, want) in [(0x0000u16, 20.0), (0x2000, 30.0)] {
        let mult = p(0x402C, flags, &f32s(&[2.0, 0.0, 0.0, 1.0, 0.0, 0.0]));
        let rect = plus_fill_rect(argb(0, 0, 0, 255), [5.0, 0.0, 1.0, 1.0]);
        let pic = parse(&emf(&[hp(), c(&[set.clone(), mult]), rect])).unwrap();
        let f = fills(&pic);
        assert!(near(f[0].0[0], want), "flags {flags:#x}: x = 5 goes to {}: {f:?}", f[0].0[0]);
    }
}

/// An EMF+ image object of a metafile (`kind` 5 for a Dual one) holding `data`.
fn metafile_image(id: u16, kind: u32, data: &[u8]) -> Vec<u8> {
    let mut d = u32s(&[0xDBC0_1002, 2, kind, data.len() as u32]);
    d.extend(data);
    p(0x4008, 0x0500 | id, &d)
}

#[test]
fn a_metafile_inside_the_image_object_maps_its_pixels_one_to_one_into_the_source_rectangle() {
    // The nested frame is 100 by 50 pixels (Frame 2500 by 1250 at 4 pixels to the millimetre), its Bounds are (0, 0, 59, 29).
    let inner_head = Head { frame: [0, 0, 2500, 1250], bounds: [0, 0, 59, 29], ..Head::default() };
    // The nested picture draws a 1 by 1 square at its pixel (50, 25).
    let inner = emf_with(&inner_head, &[r(43, &i32s(&[50, 25, 51, 26]))]);
    let draw = p(
        0x401B,
        0x0001,
        &[u32s(&[0xFFFF_FFFF, 2]), f32s(&[0.0, 0.0, 100.0, 50.0]), u32s(&[3]), f32s(&[10.0, 10.0, 110.0, 10.0, 10.0, 60.0])].concat(),
    );
    let pic = parse(&emf(&[hp(), c(&[metafile_image(1, 4, &inner)]), c(&[draw])])).unwrap();
    let f = fills(&pic);
    assert_eq!(f.len(), 1, "{f:?}");
    assert!(near(f[0].0[0], 60.0) && near(f[0].0[1], 35.0), "the nested pixel (50, 25) lands at (60, 35): {f:?}");
}

#[test]
fn a_wmf_window_is_its_frame_and_stores_y_before_x() {
    let recs = vec![w(0x020B, &i16s(&[0, 0])), w(0x020C, &i16s(&[100, 200])), w(0x0325, &[i16s(&[2]), i16s(&[0, 0, 200, 100])].concat())];
    let pic = parse(&wmf(&recs)).unwrap();
    assert!(near(pic.width, 200.0) && near(pic.height, 100.0), "a window 200 wide and 100 tall: {} by {}", pic.width, pic.height);
    let ink = inks(&pic);
    assert!(near(ink[0].points[0][0], 0.0) && near(ink[0].points[1][0], 200.0) && near(ink[0].points[1][1], 100.0), "{ink:?}");
}

#[test]
fn a_placeable_wmf_in_an_image_object_has_its_header_24_bytes_in() {
    // The placeable record is 22 bytes, then two zero bytes, then the standard header (type 1, nine words, version 0x0300).
    let mut data = vec![0xD7, 0xCD, 0xC6, 0x9A, 0, 0];
    data.extend(i16s(&[0, 0, 200, 100]));
    data.extend(u16s(&[1440]));
    data.extend(u32s(&[0]));
    data.extend(u16s(&[0]));
    data.extend([0, 0]);
    let wmf_bytes = wmf(&[w(0x0325, &[i16s(&[2]), i16s(&[0, 0, 200, 100])].concat())]);
    data.extend(&wmf_bytes);
    assert_eq!(data.len(), 24 + wmf_bytes.len());
    let draw =
        p(0x401B, 0x0001, &[u32s(&[0xFFFF_FFFF, 2]), f32s(&[0.0, 0.0, 200.0, 100.0]), u32s(&[3]), f32s(&[0.0, 0.0, 80.0, 0.0, 0.0, 40.0])].concat());
    let pic = parse(&emf(&[hp(), c(&[metafile_image(1, 4, &data)]), c(&[draw])])).unwrap();
    assert_eq!(inks(&pic).len(), 1, "the WMF parsed and drew its line: {:?}", pic.report);
}

fn text_record(reference: [i32; 2], chars: &str, advances: &[i32], options: u32) -> Vec<u8> {
    let units: Vec<u16> = chars.encode_utf16().collect();
    let mut f = i32s(&[0, 0, 0, 0]);
    f.extend(u32s(&[1]));
    f.extend(f32s(&[0.0, 0.0]));
    f.extend(i32s(&reference));
    let n = units.len() as u32;
    // The rectangle is present: the string is at 76, and the advances after it padded to four bytes.
    let string_at = 76u32;
    let dx_at = string_at + (n * 2).div_ceil(4) * 4;
    f.extend(u32s(&[n, string_at, options]));
    f.extend(i32s(&[0, 0, 0, 0]));
    f.extend(u32s(&[if advances.is_empty() { 0 } else { dx_at }]));
    let mut strings = u16s(&units);
    while !strings.len().is_multiple_of(4) {
        strings.push(0);
    }
    f.extend(strings);
    f.extend(i32s(advances));
    r(84, &f)
}

fn font_record(index: u32, height: i32, face: &str, charset: u8, escapement: i32) -> Vec<u8> {
    let mut f = u32s(&[index]);
    f.extend(i32s(&[height, 0, escapement, 0, 400]));
    f.extend([0, 0, 0, charset, 0, 0, 0, 0]);
    let mut name: Vec<u16> = face.encode_utf16().collect();
    name.resize(32, 0);
    f.extend(u16s(&name));
    // The 356-byte form: the rest of the extended log font is zeros.
    f.extend(vec![0u8; 356 - 8 - 4 - 92]);
    r(82, &f)
}

#[test]
fn text_with_an_advance_array_is_placed_by_the_file_and_not_by_the_font() {
    // Height -20, alignment baseline and centre (0x1E), reference (100, 50), three characters with advances 10, 20 and 30.
    let recs =
        vec![font_record(1, -20, "Quillmono", 0, 0), r(37, &u32s(&[1])), r(22, &u32s(&[0x1E])), text_record([100, 50], "abc", &[10, 20, 30], 0)];
    let pic = parse(&emf(&recs)).unwrap();
    let t =
        pic.items.iter().find_map(|i| if let diacraft_metafile::Drawn::Text(t) = &i.drawn { Some(t.clone()) } else { None }).expect("a text item");
    let Placement::Origins(o) = &t.placement else { panic!("placed by advances: {:?}", t.placement) };
    let xs: Vec<f64> = o.iter().map(|p| p[0]).collect();
    assert!(near(xs[0], 70.0) && near(xs[1], 80.0) && near(xs[2], 100.0), "{xs:?}");
    assert!(o.iter().all(|p| near(p[1], 50.0)), "baseline y = 50: {o:?}");
    assert_eq!(t.text, "abc");
    assert_eq!(t.v_align, diacraft_metafile::VAlign::Baseline);
    assert!(&*t.font.family == "Quillmono" && !t.cell && near(t.size, 20.0));
}

#[test]
fn a_positive_height_is_the_cell_and_a_negative_one_the_em() {
    for (height, cell) in [(24, true), (-24, false)] {
        let recs = vec![font_record(1, height, "Hack", 0, 0), r(37, &u32s(&[1])), text_record([0, 0], "a", &[], 0)];
        let pic = parse(&emf(&recs)).unwrap();
        let t = pic
            .items
            .iter()
            .find_map(|i| if let diacraft_metafile::Drawn::Text(t) = &i.drawn { Some(t.clone()) } else { None })
            .expect("a text item");
        assert!(t.cell == cell && near(t.size, 24.0), "height {height}: cell {} size {}", t.cell, t.size);
    }
}

#[test]
fn text_of_glyph_indices_is_not_drawn_and_reported() {
    let recs = vec![font_record(1, -20, "Hack", 0, 0), r(37, &u32s(&[1])), text_record([0, 0], "ab", &[], 0x10)];
    let pic = parse(&emf(&recs)).unwrap();
    assert!(pic.items.is_empty());
    assert_eq!(pic.report.first(), Some(Detail::GlyphIndexText));
}

#[test]
fn a_copy_with_no_region_data_resets_the_clip() {
    let recs = vec![r(30, &i32s(&[0, 0, 40, 40])), r(75, &u32s(&[0, 5])), r(43, &i32s(&[0, 0, 80, 40]))];
    let pic = parse(&emf(&recs)).unwrap();
    let ink = inks(&pic);
    assert_eq!(ink.len(), 1, "the whole box is filled: not clipped away, not clipped to the left half");
    assert_eq!(ink[0].clip, 0, "no clip");
}

/// An `EMR_EXTSELECTCLIPRGN` of one rectangle by `mode`.
fn select_clip_rect(rect: [i32; 4], mode: u32) -> Vec<u8> {
    let mut data = u32s(&[32, 1, 1, 16]);
    data.extend(i32s(&rect));
    data.extend(i32s(&rect));
    r(75, &[u32s(&[data.len() as u32, mode]), data].concat())
}

#[test]
fn the_meta_region_still_holds_after_the_clip_is_made_again() {
    let recs = vec![select_clip_rect([0, 0, 40, 40], 5), r(28, &[]), r(30, &i32s(&[20, 0, 80, 40])), r(43, &i32s(&[0, 0, 80, 40]))];
    let pic = parse(&emf(&recs)).unwrap();
    let ink = inks(&pic);
    assert_eq!(ink.len(), 1);
    let clip = clip_bbox(&pic, ink[0].clip).expect("a single rectangle");
    assert!(bbox_is(clip, [20.0, 0.0, 40.0, 40.0]), "only device x 20 to 40 is filled: {clip:?}");
}

#[test]
fn a_restore_deeper_than_the_stack_is_ignored() {
    let recs = vec![r(33, &[]), r(34, &i32s(&[-2])), r(43, &i32s(&[0, 0, 40, 40]))];
    let pic = parse(&emf(&recs)).unwrap();
    assert_eq!(inks(&pic).len(), 1, "drawing goes on");
}

#[test]
fn the_count_of_records_in_the_header_is_not_trusted() {
    let head = Head { records: 3, ..Head::default() };
    let five: Vec<Vec<u8>> = (0..5).map(|i| r(43, &i32s(&[i, 0, i + 1, 4]))).collect();
    let pic = parse(&emf_with(&head, &five)).unwrap();
    assert_eq!(inks(&pic).len(), 5, "all five are drawn");
    assert!(pic.report.is_empty());
}

#[test]
fn a_record_whose_size_is_six_ends_the_walk_and_is_reported() {
    let mut bytes = emf(&[r(43, &i32s(&[0, 0, 10, 10]))]);
    // Replace `END` with a record of size 6.
    bytes.truncate(bytes.len() - 20);
    bytes.extend(u32s(&[14, 6]));
    let pic = parse(&bytes).unwrap();
    assert_eq!(inks(&pic).len(), 1, "the earlier record is kept");
    assert_eq!(pic.report.first(), Some(Detail::CutShort));
}

#[test]
fn a_polygon_whose_count_says_a_million_points_in_a_forty_byte_record_is_skipped() {
    // The record is 8 + 16 (bounds) + 4 (count) + 12 = 40 bytes, and the count says 1,000,000.
    let mut f = i32s(&[0, 0, 10, 10]);
    f.extend(u32s(&[1_000_000]));
    f.extend(i16s(&[0, 0, 1, 1, 2, 2]));
    let rec = r(86, &f);
    assert_eq!(rec.len(), 40);
    let pic = parse(&emf(&[rec, r(43, &i32s(&[0, 0, 10, 10]))])).unwrap();
    assert_eq!(inks(&pic).len(), 1, "the polygon is skipped and the walk goes on");
}

#[test]
fn an_object_that_is_not_there_is_ignored_and_the_pen_and_brush_stay() {
    let recs = vec![r(37, &u32s(&[9])), r(43, &i32s(&[0, 0, 40, 40]))];
    let pic = parse(&emf(&recs)).unwrap();
    let ink = inks(&pic);
    assert_eq!(ink[0].fill, Some([255, 255, 255, 255]));
    assert_eq!(ink[0].stroke, Some([0, 0, 0, 255]));
}

#[test]
fn the_gradient_of_the_emf_plus_half_is_drawn_and_the_masks_of_the_gdi_half_are_not() {
    // A linear gradient brush (object 1) fills a rectangle; the GDI half draws the same area with a 1-bit mask by SRCPAINT and then a
    // colour bitmap by SRCAND.
    let brush = p(
        0x4008,
        0x0101,
        &[u32s(&[0xDBC0_1002, 4, 0, 0]), f32s(&[0.0, 0.0, 80.0, 40.0]), u32s(&[argb(255, 0, 0, 255), argb(0, 0, 255, 255), 0, 0])].concat(),
    );
    let draw = p(0x400A, 0x0000, &[u32s(&[1, 1]), f32s(&[0.0, 0.0, 80.0, 40.0])].concat());
    let mask = stretch_dibits([0, 0, 80, 40], &dib_1bit_mask(), 0x00EE_0086);
    let colour = stretch_dibits([0, 0, 80, 40], &dib_24(2, 2, &[[255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 255]]), 0x0088_00C6);
    let pic = parse(&emf(&[hp(), c(&[brush]), c(&[draw]), mask, colour, c(&[p(0x4002, 0, &[])])])).unwrap();
    let paint = pic.items.iter().find_map(|i| match &i.drawn {
        diacraft_metafile::Drawn::Path(p) => p.fill.as_ref().map(|f| f.paint.clone()),
        _ => None,
    });
    assert!(matches!(paint, Some(Paint::Linear(_))), "{paint:?}");
    assert!(
        pic.items.iter().all(|i| !matches!(i.drawn, diacraft_metafile::Drawn::Bitmap(_))),
        "no bitmap: the EMF+ half is played, not the GDI half"
    );
    assert!(pic.report.is_empty(), "{:?}", pic.report);
}
