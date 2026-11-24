//! Hostile inputs: files crafted so that each cap in is within reach of a small
//! input. Each test builds one such file in code, reads it, and asserts that the work stayed inside the caps: the report (`cap-reached`),
//! the shape of what came out, and, where a cost leaves no other trace, a count of the work (`diacraft_metafile::work`). A count does not
//! move with the load on the machine, as a time does; the only bound on time is the hang guard of `timed`.
//!
//! Several of these were found by a review of the crate: a clip that grows by a part for each record and is copied whole each time, a path
//! clip transformed without being charged, a metafile inside a metafile copied whole for each draw, and a table scanned from its start.
//! Each fails on the code before its fix.

use std::time::{Duration, Instant};

use diacraft_metafile::limits::{MAX_CLIP_PARTS, MAX_CLIP_POINTS, MAX_POINTS, MAX_WORK};
use diacraft_metafile::{Detail, Paint, Picture, parse};

use crate::build::*;

/// A hang guard and no measure of cost: every cost these files aim at is charged to the picture's budget before it is paid, and each test
/// asserts that (the cap is reported, the clips and items that came out are within it). A read here takes a second at most at load 50, and
/// the unbounded readings these files were made against take minutes to hours, so half a minute tells a hang from a slow machine.
const HANG_GUARD: Duration = Duration::from_secs(30);

/// Reads `bytes`, and fails if that takes long enough to be a hang.
fn timed(bytes: &[u8]) -> (Picture, Duration) {
    let start = Instant::now();
    let pic = parse(bytes).expect("a header that reads");
    let took = start.elapsed();
    eprintln!("hostile: {} bytes read in {took:?}, {} items, {} clips", bytes.len(), pic.items.len(), pic.clips.len());
    assert!(took < HANG_GUARD, "{} bytes took {took:?}: a hang", bytes.len());
    (pic, took)
}

/// A triangle as a `POLYGON16`: not a rectangle, so a clip made of it cannot fold into a rectangle.
fn triangle_polygon() -> Vec<u8> {
    let mut f = i32s(&[0, 0, 80, 40]);
    f.extend(u32s(&[3]));
    f.extend(i16s(&[0, 0, 80, 0, 40, 40]));
    r(86, &f)
}

/// An EMF+ path object of the triangle, as object `id`.
fn triangle_path_object(id: u16) -> Vec<u8> {
    let mut d = u32s(&[0xDBC0_1002, 3, 0]);
    d.extend(f32s(&[0.0, 0.0, 80.0, 0.0, 40.0, 40.0]));
    d.extend([0, 1, 1 | 0x80]);
    p(0x4008, 0x0300 | id, &d)
}

/// An EMF+ path object of `n` points zigzagging over the frame.
fn zigzag_path_object(id: u16, n: usize) -> Vec<u8> {
    let mut d = u32s(&[0xDBC0_1002, n as u32, 0]);
    let pts: Vec<f32> = (0..n).flat_map(|i| [(i % 80) as f32, if i % 2 == 0 { 0.0 } else { 40.0 }]).collect();
    d.extend(f32s(&pts));
    d.extend((0..n).map(|i| if i == 0 { 0u8 } else { 1u8 }));
    p(0x4008, 0x0300 | id, &d)
}

fn plus_fill_all() -> Vec<u8> {
    plus_fill_rect(argb(0, 0, 0, 255), [0.0, 0.0, 80.0, 40.0])
}

/// Counted, not timed: `combinations` clips were combined, and the parts copied to make them are at most the cap on a clip's parts (plus
/// the one the combination adds) for each. A clip that grew by a part each time would copy half the square of the count.
#[track_caller]
fn assert_clip_copies_are_small(combinations: u64) {
    let copied = diacraft_metafile::work::clip_parts_copied();
    assert!(copied <= combinations * (MAX_CLIP_PARTS as u64 + 1), "{copied} parts copied by {combinations} combinations of clips");
}

fn assert_clips_are_small(pic: &Picture) {
    for (i, c) in pic.clips.iter().enumerate() {
        assert!(c.parts.len() <= MAX_CLIP_PARTS, "clip {i} has {} parts, more than {MAX_CLIP_PARTS}", c.parts.len());
        let points: usize = c.parts.iter().map(|p| p.path.segs.len()).sum();
        assert!((points as u64) <= MAX_CLIP_POINTS, "clip {i} holds {points} points");
    }
}

#[test]
fn a_path_clip_after_a_path_clip_does_not_grow_a_clip_by_a_part_each_time() {
    // 20,000 times: a path bracket round a triangle, then SELECTCLIPPATH by intersection. Unbounded, the clip gains a part each time and
    // every combine copies them all: 200 million parts copied.
    let mut recs: Vec<Vec<u8>> = Vec::new();
    for _ in 0..20_000 {
        recs.extend([r(59, &[]), triangle_polygon(), r(60, &[]), r(67, &u32s(&[1]))]);
    }
    recs.push(r(43, &i32s(&[0, 0, 80, 40])));
    diacraft_metafile::work::reset();
    let (pic, _) = timed(&emf(&recs));
    assert_clips_are_small(&pic);
    assert_clip_copies_are_small(20_000);
    assert!(pic.report.count(Detail::CapReached) > 0, "the cap is reported: {:?}", pic.report);
    assert!(!pic.items.is_empty(), "the fill is still drawn, under the clip that was kept");
}

#[test]
fn an_emf_plus_path_clip_after_a_path_clip_does_not_grow_a_clip_by_a_part_each_time() {
    let mut recs = vec![triangle_path_object(1)];
    recs.extend((0..20_000).map(|_| p(0x4033, 0x0101, &[])));
    diacraft_metafile::work::reset();
    let (pic, _) = timed(&emf(&[hp(), c(&recs), plus_fill_all(), c(&[p(0x4002, 0, &[])])]));
    assert_clips_are_small(&pic);
    assert_clip_copies_are_small(20_000);
    assert!(pic.report.count(Detail::CapReached) > 0, "the cap is reported: {:?}", pic.report);
}

#[test]
fn a_path_clip_is_charged_for_the_points_it_transforms() {
    // One path of 10,000 points set as the clip 30,000 times: 300 million points transformed if nothing is charged.
    let mut recs = vec![zigzag_path_object(1, 10_000)];
    recs.extend((0..30_000).map(|_| p(0x4033, 0x0001, &[])));
    diacraft_metafile::work::reset();
    let (pic, _) = timed(&emf(&[hp(), c(&recs), plus_fill_all(), c(&[p(0x4002, 0, &[])])]));
    assert!(pic.report.count(Detail::CapReached) > 0, "the work is capped and reported: {:?}", pic.report);
    // Counted, not timed: the points moved into the frame to be a clip are charged as work, so they are at most the units of work a picture
    // has (the 300 million of 30,000 sets of 10,000 points are not moved).
    let moved = diacraft_metafile::work::clip_points_transformed();
    assert!((1..=MAX_WORK).contains(&moved), "{moved} points of a path clip moved, of 300 million asked for");
}

/// An EMF+ image object of a metafile (`kind` 4) holding `data`, as the one record it is.
fn metafile_image(id: u16, data: &[u8]) -> Vec<u8> {
    let mut d = u32s(&[0xDBC0_1002, 2, 4, data.len() as u32]);
    d.extend(data);
    p(0x4008, 0x0500 | id, &d)
}

fn draw_image_1() -> Vec<u8> {
    p(0x401B, 0x0001, &[u32s(&[0xFFFF_FFFF, 2]), f32s(&[0.0, 0.0, 80.0, 40.0]), u32s(&[3]), f32s(&[0.0, 0.0, 80.0, 0.0, 0.0, 40.0])].concat())
}

#[test]
fn a_metafile_in_an_image_object_is_not_copied_for_each_draw_and_each_draw_is_charged_its_size() {
    // The inner metafile is an EMF whose first record after the header is the end: 4 MB of bytes follow it that no record reads. Drawn
    // 5,000 times, copying it each time is 20 GB.
    let mut inner = emf(&[]);
    inner.resize(inner.len() + 4_000_000, 0xAB);
    let mut recs = vec![metafile_image(1, &inner)];
    recs.extend((0..5_000).map(|_| draw_image_1()));
    let (pic, _) = timed(&emf(&[hp(), c(&recs), c(&[p(0x4002, 0, &[])])]));
    assert!(pic.report.count(Detail::CapReached) > 0, "each draw is charged the metafile's size, so the work is capped: {:?}", pic.report);
}

/// A WMF `META_CREATEPENINDIRECT`, and `META_DELETEOBJECT` of an index.
fn wmf_pen() -> Vec<u8> {
    w(0x02FA, &[u16s(&[0]), i16s(&[1, 0]), u32s(&[0])].concat())
}

fn wmf_delete(i: u16) -> Vec<u8> {
    w(0x01F0, &u16s(&[i]))
}

#[test]
fn a_wmf_object_table_is_not_scanned_from_its_start_for_each_new_object() {
    // 30,000 pens fill the table; then 150,000 times the last is deleted and another created, which takes the one free slot at the end.
    let n = 30_000u16;
    let mut recs: Vec<Vec<u8>> = (0..n).map(|_| wmf_pen()).collect();
    for _ in 0..150_000 {
        recs.push(wmf_delete(n - 1));
        recs.push(wmf_pen());
    }
    diacraft_metafile::work::reset();
    let _ = timed(&wmf(&recs));
    // Counted, not timed: a new object takes the slot the table remembers as the first free, and passes no occupied slot to find it (a search
    // from the start of the table would pass 30,000 of them for each of the 180,000 objects).
    let passed = diacraft_metafile::work::slots_passed();
    assert!(passed <= 180_000, "{passed} occupied slots passed by 180,000 new objects");
}

#[test]
fn a_restore_that_names_no_state_is_charged_for_looking_through_the_saved_ones() {
    // 1,000 saves, then 3 million restores of an index nobody saved: each looks through 1,000 states.
    let mut recs = vec![hp()];
    let saves: Vec<Vec<u8>> = (0..1_000u32).map(|i| p(0x4025, 0, &u32s(&[i]))).collect();
    recs.push(c(&saves));
    let restores: Vec<Vec<u8>> = (0..3_000_000).map(|_| p(0x4026, 0, &u32s(&[999_999]))).collect();
    for chunk in restores.chunks(100_000) {
        recs.push(c(chunk));
    }
    let (pic, _) = timed(&emf(&recs));
    assert!(pic.report.count(Detail::CapReached) > 0, "{:?}", pic.report);
}

#[test]
fn a_pen_with_thousands_of_dashes_drawn_many_times_is_charged_for_the_dashes_each_item_keeps() {
    // A pen of 4,096 custom dashes (32 KB for each item that keeps them), stroked 20,000 times.
    let mut pen = u32s(&[0xDBC0_1002, 0]);
    pen.extend(u32s(&[0x20 | 0x100, 0]));
    pen.extend(f32s(&[1.0]));
    pen.extend(i32s(&[5]));
    pen.extend(u32s(&[4_096]));
    pen.extend(f32s(&vec![1.0; 4_096]));
    pen.extend(u32s(&[0xDBC0_1002, 0, argb(0, 0, 0, 255)]));
    let mut recs = vec![triangle_path_object(1), p(0x4008, 0x0202, &pen)];
    recs.extend((0..20_000).map(|_| p(0x4015, 0x0001, &u32s(&[2]))));
    let (pic, _) = timed(&emf(&[hp(), c(&recs), c(&[p(0x4002, 0, &[])])]));
    assert!(pic.items.len() < 4_000, "the dashes are charged for each item that keeps them: {} items", pic.items.len());
    assert!(pic.report.count(Detail::CapReached) > 0, "{:?}", pic.report);
}

#[test]
fn the_stops_of_a_gradient_are_shared_by_the_items_that_use_it() {
    // A linear gradient of 4,096 preset colours, filled twice: the stops are held once.
    let n = 4_096u32;
    let mut b = u32s(&[0xDBC0_1002, 4]);
    b.extend(u32s(&[0x4, 0]));
    b.extend(f32s(&[0.0, 0.0, 80.0, 40.0]));
    b.extend(u32s(&[argb(255, 0, 0, 255), argb(0, 0, 255, 255), 0, 0]));
    b.extend(u32s(&[n]));
    b.extend(f32s(&(0..n).map(|i| i as f32 / (n - 1) as f32).collect::<Vec<_>>()));
    b.extend(u32s(&(0..n).map(|i| argb((i % 256) as u8, 0, 0, 255)).collect::<Vec<_>>()));
    let brush = p(0x4008, 0x0101, &b);
    let fill = || p(0x400A, 0x0000, &[u32s(&[1, 1]), f32s(&[0.0, 0.0, 10.0, 10.0])].concat());
    let (pic, _) = timed(&emf(&[hp(), c(&[brush, fill(), fill()]), c(&[p(0x4002, 0, &[])])]));
    let stops: Vec<_> = pic
        .items
        .iter()
        .filter_map(|i| match &i.drawn {
            diacraft_metafile::Drawn::Path(p) => match p.fill.as_ref().map(|f| &f.paint) {
                Some(Paint::Linear(l)) => Some(std::sync::Arc::clone(&l.stops)),
                _ => None,
            },
            _ => None,
        })
        .collect();
    assert_eq!(stops.len(), 2);
    assert!(std::sync::Arc::ptr_eq(&stops[0], &stops[1]), "the same list, not a copy for each item");
}

#[test]
fn a_polygon_of_more_points_than_are_left_to_keep_is_not_built() {
    // A POLYGON16 of 10 million points in a record of 40 MB: more than the budget keeps, so its points are not even gathered.
    let n = 10_000_000usize;
    let mut f = i32s(&[0, 0, 80, 40]);
    f.extend(u32s(&[n as u32]));
    f.extend(vec![0u8; n * 4]);
    diacraft_metafile::work::reset();
    let (pic, _) = timed(&emf(&[r(86, &f), r(43, &i32s(&[0, 0, 80, 40]))]));
    assert!(pic.report.count(Detail::CapReached) > 0, "{:?}", pic.report);
    // Counted, not timed: "not even gathered" is none of the 10 million points made into a vector.
    assert_eq!(diacraft_metafile::work::points_gathered(), 0, "the points of a record the budget cannot keep were gathered");
}

#[test]
fn a_region_of_more_rectangles_than_a_clip_may_hold_is_refused_before_it_is_built() {
    // EXTSELECTCLIPRGN with 100,000 rectangles (500,000 points as a path).
    let n = 100_000usize;
    let mut data = u32s(&[32, 1, n as u32, (n * 16) as u32]);
    data.extend(i32s(&[0, 0, 80, 40]));
    for i in 0..n {
        data.extend(i32s(&[(i % 80) as i32, 0, (i % 80) as i32 + 1, 40]));
    }
    let rec = r(75, &[u32s(&[data.len() as u32, 5]), data].concat());
    diacraft_metafile::work::reset();
    let (pic, _) = timed(&emf(&[rec, r(43, &i32s(&[0, 0, 80, 40]))]));
    assert_clips_are_small(&pic);
    // Counted, not timed: refused "before it is built" is none of the 100,000 rectangles gathered.
    assert_eq!(diacraft_metafile::work::region_rects_gathered(), 0, "the rectangles of a region too big for a clip were gathered");
    assert!(pic.report.count(Detail::CapReached) > 0, "{:?}", pic.report);
}

/// A file whose innermost metafile fills a square, wrapped `levels` times: each level is an EMF+ file whose image object is the next.
fn nested_chain(levels: usize) -> Vec<u8> {
    let mut inner = emf(&[r(43, &i32s(&[0, 0, 10, 10]))]);
    for _ in 0..levels {
        inner = emf(&[hp(), c(&[metafile_image(1, &inner), draw_image_1()]), c(&[p(0x4002, 0, &[])])]);
    }
    inner
}

#[test]
fn metafiles_nest_four_deep_and_no_deeper() {
    let (four, _) = timed(&nested_chain(4));
    assert_eq!(four.items.len(), 1, "four levels inside the file are drawn: {:?}", four.report);
    assert_eq!(four.report.count(Detail::CapReached), 0);
    let (five, _) = timed(&nested_chain(5));
    assert_eq!(five.items.len(), 0, "the fifth is not drawn");
    assert!(five.report.count(Detail::CapReached) > 0, "and the cap is reported: {:?}", five.report);
}

#[test]
fn a_file_of_a_very_long_chain_of_nested_metafiles_stops_at_the_cap_without_recursing_further() {
    let (pic, _) = timed(&nested_chain(60));
    assert!(pic.items.is_empty());
    assert!(pic.report.count(Detail::CapReached) > 0);
}

#[test]
fn two_hundred_thousand_saves_stop_at_the_depth_cap() {
    // 200,000 `Save` records, each with a new index: the pushes stop at the cap (1,024), and memory is the cap's, not the count's.
    let saves: Vec<Vec<u8>> = (0..200_000u32).map(|i| p(0x4025, 0, &u32s(&[i]))).collect();
    let mut recs = vec![hp()];
    for chunk in saves.chunks(20_000) {
        recs.push(c(chunk));
    }
    recs.push(plus_fill_all());
    let (pic, _) = timed(&emf(&recs));
    assert!(pic.report.count(Detail::CapReached) > 0, "{:?}", pic.report);
    assert_eq!(pic.items.len(), 1);
}

#[test]
fn two_million_savedc_records_stop_at_the_depth_cap() {
    let recs: Vec<Vec<u8>> = (0..2_000_000).map(|_| r(33, &[])).chain([r(43, &i32s(&[0, 0, 80, 40]))]).collect();
    let (pic, _) = timed(&emf(&recs));
    assert!(pic.report.count(Detail::CapReached) > 0, "{:?}", pic.report);
    assert_eq!(pic.items.len(), 1);
}

#[test]
fn a_path_bracket_built_from_many_records_and_never_painted_is_charged_for_its_points() {
    // 24 POLYLINE16 records of 1 million points inside one bracket, which is never used: 24 million points held, each 24 bytes, unless the
    // points added to the bracket are charged.
    let n = 1_000_000usize;
    let mut f = i32s(&[0, 0, 80, 40]);
    f.extend(u32s(&[n as u32]));
    f.extend((0..n).flat_map(|i| i16s(&[(i % 80) as i16, (i % 40) as i16])));
    let poly = r(87, &f);
    let mut recs = vec![r(59, &[])];
    recs.extend((0..24).map(|_| poly.clone()));
    recs.push(r(60, &[]));
    diacraft_metafile::work::reset();
    let (pic, _) = timed(&emf(&recs));
    assert!(pic.report.count(Detail::CapReached) > 0, "the bracket is capped and reported: {:?}", pic.report);
    // Counted, not timed: the bracket never held more points than a picture keeps (24 million, at 24 bytes each, are not held).
    let held = diacraft_metafile::work::peak_bracket_points();
    assert!((1..=MAX_POINTS).contains(&held), "the bracket held {held} points");
}

#[test]
fn path_objects_redefined_again_and_again_are_charged_for_the_points_they_hold() {
    // 64 path objects of 150,000 points each: 9.6 million points, more than a picture keeps.
    let recs: Vec<Vec<u8>> = (0..64u16).map(|id| zigzag_path_object(id, 150_000)).collect();
    diacraft_metafile::work::reset();
    let (pic, _) = timed(&emf(&[hp(), c(&recs), plus_fill_all(), c(&[p(0x4002, 0, &[])])]));
    assert!(pic.report.count(Detail::CapReached) > 0, "{:?}", pic.report);
    // Counted, not timed: the path objects that were kept hold no more points than a picture keeps.
    let made = diacraft_metafile::work::object_points_made();
    assert!((150_000..=MAX_POINTS).contains(&made), "{made} points kept in path objects, of 9.6 million asked for");
}

#[test]
fn a_million_and_a_fifth_rectangles_stop_at_the_item_cap_and_say_so() {
    // 1.2 million RECTANGLE records (29 MB): the item cap (a million) ends the walk, and the picture says a cap was reached.
    let mut recs = vec![r(39, &u32s(&[1, 0, 0x0000_00FF, 0])), r(37, &u32s(&[1])), r(37, &u32s(&[0x8000_0008]))];
    recs.extend((0..1_200_000).map(|i| r(43, &i32s(&[i % 70, i % 30, i % 70 + 10, i % 30 + 10]))));
    let (pic, _) = timed(&emf(&recs));
    assert_eq!(pic.items.len(), 1_000_000);
    assert!(pic.report.count(Detail::CapReached) > 0, "{:?}", pic.report);
}
