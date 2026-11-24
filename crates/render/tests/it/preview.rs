//! A picture that goes into a file: drawn within a budget of **work**, never of time, so that the same drawing gives the same pixels however
//! loaded the machine is. A clock that a test steps ([`Clock::stepping`]) stands for a machine so slow that every deadline has passed by the time a frame
//! looks: within the window's limits a frame drawn against it leaves out what each deadline guards, and within [`Limits::preview`] it is the frame the
//! machine's clock gives. What the work budget charges is counted here, by kind, and what passes it is cut at a point the drawing sets.

use std::time::Duration;

use diacraft_doc::{Document, ForeignData, RowKey, ShapeType};
use diacraft_render::limits::{
    MAX_METAFILE_ITEMS_PER_FRAME, PIXELS_PER_WORK, PREVIEW_MAX_WORK, WORK_PER_DRAWN_SHAPE, WORK_PER_GLYPH, WORK_PER_SHAPE,
};
use diacraft_render::{Clock, Limits, Parallel, Renderer, SkipKind};
use diacraft_testkit::build::ShapeBuilder;

use crate::meta_build::*;
use crate::pictures::foreign;
use crate::support::*;

/// A clock for which every deadline has passed by the time a frame looks: each reading is an hour after the one before.
fn late() -> Clock {
    Clock::stepping(Duration::from_secs(3_600))
}

/// A frame of `doc` within `limits`, reading the time from `clock`, on `threads` threads, and the work it was charged.
fn drawn_on(limits: Limits, clock: Clock, threads: usize, doc: &Document) -> (Pic, u64, Renderer) {
    let mut renderer = Renderer::new();
    renderer.set_limits(limits);
    renderer.set_clock(clock);
    renderer.set_parallel(Parallel::eager(threads));
    let pic = draw_with(&mut renderer, doc);
    let work = renderer.work_done();
    (pic, work, renderer)
}

fn drawn(limits: Limits, clock: Clock, doc: &Document) -> (Pic, u64, Renderer) {
    drawn_on(limits, clock, 1, doc)
}

/// A box with a label of its own, `Step k`.
fn labelled(id: u32, x: f64, y: f64, text: &str) -> ShapeBuilder {
    rect(id, x, y, 0.35, 0.3).cell("FillForegnd", "#FF0000").text(text).row_cell("Character", None, RowKey::Ix(0), "Size", "0.08")
}

/// An object DiaCraft cannot draw, shown as a box with its label (a placeholder, whose label has a budget of its own).
fn object(id: u32, x: f64, y: f64, label: &str) -> ShapeBuilder {
    shape_at(id, x, y, 0.6, 0.4).kind(ShapeType::Foreign).row_cell("User", None, RowKey::name("ODFObject"), "Value", label)
}

/// Twenty labelled boxes, then five objects with labels of their own, on a page of 4 by 4 inches.
fn labels_and_objects() -> Vec<ShapeBuilder> {
    let mut shapes: Vec<ShapeBuilder> =
        (0..20u32).map(|k| labelled(k + 1, 0.1 + f64::from(k % 10) * 0.38, 0.2 + f64::from(k / 10) * 0.5, &format!("Step {k}"))).collect();
    shapes.extend((0..5u32).map(|k| object(21 + k, 0.1 + f64::from(k) * 0.75, 2.0, &format!("Object kind {k}"))));
    shapes
}

/// A PNG of `side` by `side` grey pixels, compressed: a picture far larger decoded than stored.
fn grey_png(side: u32) -> Vec<u8> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, side, side);
    encoder.set_color(png::ColorType::Grayscale);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Fast);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&vec![90u8; side as usize * side as usize]).unwrap();
    drop(writer);
    out
}

/// `count` picture shapes of `side` inches in rows on the page, each showing `data`: the same picture (one part, as a master's shapes share it) when `shared`, and
/// a copy of its bytes of its own (another part) when not.
fn pictures(count: u32, data: &ForeignData, shared: bool, side: f64) -> Vec<ShapeBuilder> {
    (0..count)
        .map(|i| {
            let (x, y) = (f64::from(i % 40) * 0.09, f64::from(i / 40 % 40) * 0.09);
            let data = if shared {
                data.clone()
            } else {
                let media = data.picture.as_ref().unwrap();
                foreign("Bitmap", &media.content_type, media.bytes.to_vec())
            };
            geometry_of(shape_at(i + 1, x, y, side, side), rect_rows(side, side)).cell("FillPattern", "0").cell("LinePattern", "0").foreign(data)
        })
        .collect()
}

/// The work of a shape entered and drawn.
const DRAWN: u64 = WORK_PER_SHAPE + WORK_PER_DRAWN_SHAPE;

/// The kinds that say a frame was cut by a time.
const TIMED: [SkipKind; 2] = [SkipKind::FrameBudget, SkipKind::TextBudget];

#[test]
fn a_stepped_clock_passes_every_deadline_of_the_windows_limits() {
    let doc = one_page(labels_and_objects());
    // The frame's time: it has passed at the first look, before any shape.
    let (pic, _, _) = drawn(Limits::default(), late(), &doc);
    assert_eq!(pic.r.count(SkipKind::FrameBudget), 25, "{:?}", pic.r.skipped);
    // With no time for the frame, the times of text and of labels: the first block and the first label are laid out (no time was spent yet), and nothing after.
    let (pic, _, renderer) = drawn(Limits { frame_time: None, ..Limits::default() }, late(), &doc);
    assert_eq!(pic.r.count(SkipKind::TextBudget), 19, "{:?}", pic.r.skipped);
    assert_eq!(renderer.texts_laid_out(), 2, "one block of text and one label");
    // The machine's clock, on a page this small: everything is drawn.
    let (pic, _, renderer) = drawn(Limits::default(), Clock::system(), &doc);
    assert!(TIMED.iter().all(|k| pic.r.count(*k) == 0), "{:?}", pic.r.skipped);
    assert_eq!(renderer.texts_laid_out(), 25);
}

#[test]
fn a_preview_is_the_same_frame_whatever_the_clock_says() {
    let preview = Limits::preview();
    assert_eq!((preview.frame_time, preview.text_time, preview.label_time), (None, None, None), "a preview holds no time");
    assert_eq!(preview.max_work, PREVIEW_MAX_WORK);
    assert_eq!(Limits::default().max_work, u64::MAX, "a window's frame has no work budget");
    let mut shapes = labels_and_objects();
    // A dashed line, a hatch, a picture and a metafile with a bitmap and text: every kind of work, and the decodes the clock once bounded.
    shapes.push(
        geometry_of(shape_at(30, 0.2, 3.0, 3.5, 0.0), vec![("MoveTo", vec![("X", 0.0), ("Y", 0.0)]), ("LineTo", vec![("X", 3.5), ("Y", 0.0)])])
            .cell("LinePattern", "2")
            .cell("LineWeight", "0.02"),
    );
    shapes.push(rect(31, 0.2, 3.2, 1.0, 0.6).cell("FillPattern", "23").cell("FillForegnd", "#0000FF"));
    shapes.push(geometry_of(shape_at(32, 1.4, 3.2, 0.6, 0.6), rect_rows(0.6, 0.6)).foreign(foreign("Bitmap", "image/png", grey_png(300))));
    shapes.push(geometry_of(shape_at(33, 2.2, 3.2, 1.2, 0.6), rect_rows(1.2, 0.6)).foreign(foreign(
        "EnhMetaFile",
        "image/x-emf",
        metafile_with_bitmap(200),
    )));
    let doc = one_page(shapes);
    let (plain, plain_work, _) = drawn(preview, Clock::system(), &doc);
    let (stepped, stepped_work, _) = drawn(preview, late(), &doc);
    assert_eq!(plain.r, stepped.r, "the same pixels and the same report");
    assert_eq!(plain_work, stepped_work);
    assert!(TIMED.iter().all(|k| plain.r.count(*k) == 0) && plain.r.count(SkipKind::WorkLimit) == 0, "{:?}", plain.r.skipped);
    // On several threads, the same.
    let (shared, shared_work, _) = drawn_on(preview, late(), 4, &doc);
    assert_eq!(plain.r, shared.r);
    assert_eq!(plain_work, shared_work);
}

/// An EMF+ file of one PNG of `side` by `side` pixels drawn into the whole frame, with a line of text beside it.
fn metafile_with_bitmap(side: u32) -> Vec<u8> {
    let mut image = u32s(&[0xDBC0_1002, 1]);
    image.extend(i32s(&[side as i32, side as i32, 0]));
    image.extend(u32s(&[0x0026_200A, 1]));
    image.extend(grey_png(side));
    let draw_it = p(
        0x401B,
        0x0001,
        &[u32s(&[0xFFFF_FFFF, 2]), f32s(&[0.0, 0.0, side as f32, side as f32]), u32s(&[3]), f32s(&[0.0, 0.0, 80.0, 0.0, 0.0, 40.0])].concat(),
    );
    emf(&[hp(), c(&[p(0x4008, 0x0501, &image)]), c(&[draw_it]), end_plus()])
}

#[test]
fn the_work_of_a_frame_is_charged_before_it_is_done_and_what_has_no_room_is_cut_where_the_drawing_says() {
    let plain = |n: u32| -> Vec<ShapeBuilder> {
        (0..n).map(|k| rect(k + 1, 0.1 + f64::from(k) * 0.35, 1.0, 0.3, 0.3).cell("FillForegnd", "#FF0000")).collect()
    };
    let doc = one_page(plain(10));
    let (pic, work, _) = drawn(Limits::preview(), Clock::system(), &doc);
    assert_eq!(work, 10 * DRAWN, "a shape entered and drawn, and nothing else: {:?}", pic.r.skipped);
    // A text of three letters is three glyphs painted.
    let (_, work, _) = drawn(Limits::preview(), Clock::system(), &one_page(vec![labelled(1, 1.0, 1.0, "abc")]));
    assert_eq!(work, DRAWN + 3 * WORK_PER_GLYPH);
    // A budget of a unit less: the tenth is entered and not drawn, and it alone is left out.
    let tight = |max_work: u64| Limits { max_work, ..Limits::preview() };
    let (pic, work, _) = drawn(tight(10 * DRAWN - 1), Clock::system(), &doc);
    assert_eq!((work, pic.r.count(SkipKind::WorkLimit)), (9 * DRAWN + WORK_PER_SHAPE, 1), "{:?}", pic.r.skipped);
    // Room for six and a bit: the seventh is not entered, the seventh to the tenth are left out, and the frame is the frame of the first six.
    let (pic, work, _) = drawn(tight(6 * DRAWN + WORK_PER_SHAPE - 1), Clock::system(), &doc);
    assert_eq!((work, pic.r.count(SkipKind::WorkLimit)), (6 * DRAWN, 4), "{:?}", pic.r.skipped);
    let (six, _, _) = drawn(Limits::preview(), Clock::system(), &one_page(plain(6)));
    assert_eq!(pic.r.rgba_premul, six.r.rgba_premul, "the cut is at the seventh shape");
    // Shapes outside the view are entered and cost their placing all the same: a thousand of them are a thousand shapes' work.
    let away: Vec<ShapeBuilder> = (0..1_000u32).map(|k| rect(k + 1, 50.0 + f64::from(k), 50.0, 0.3, 0.3)).collect();
    let (pic, work, _) = drawn(tight(100 * WORK_PER_SHAPE), Clock::system(), &one_page(away));
    assert_eq!((work, pic.r.count(SkipKind::WorkLimit)), (100 * WORK_PER_SHAPE, 900), "{:?}", pic.r.skipped);
}

#[test]
fn dashes_hatches_metafile_items_and_decodes_are_charged_and_cut_by_the_work_budget() {
    // Each kind alone in a shape: what the shape costs past its entering is that kind's work, and a budget a unit short of it draws nothing of it.
    let tight = |max_work: u64| Limits { max_work, ..Limits::preview() };
    let dashed =
        geometry_of(shape_at(1, 0.2, 2.0, 3.5, 0.0), vec![("MoveTo", vec![("X", 0.0), ("Y", 0.0)]), ("LineTo", vec![("X", 3.5), ("Y", 0.0)])])
            .cell("LinePattern", "2")
            .cell("LineWeight", "0.02");
    let hatched = rect(1, 0.2, 0.2, 3.5, 3.5).cell("FillPattern", "23").cell("FillForegnd", "#0000FF").cell("LinePattern", "0");
    let picture = geometry_of(shape_at(1, 0.5, 0.5, 2.0, 2.0), rect_rows(2.0, 2.0)).cell("LinePattern", "0").foreign(foreign(
        "Bitmap",
        "image/png",
        grey_png(400),
    ));
    let blank = one_page(Vec::new());
    for (what, shape, least) in [("dashes", dashed, 20), ("a hatch", hatched, 100), ("a picture", picture, 400 * 400 / PIXELS_PER_WORK)] {
        let doc = one_page(vec![shape]);
        let (_, work, _) = drawn(Limits::preview(), Clock::system(), &doc);
        let own = work - DRAWN;
        assert!(own >= least, "{what}: {own} units");
        let (pic, cut, _) = drawn(tight(work - 1), Clock::system(), &doc);
        assert_eq!(pic.r.count(SkipKind::WorkLimit), 1, "{what}: {:?}", pic.r.skipped);
        assert!(cut < work, "{what}");
        let (empty, _, _) = drawn(Limits::preview(), Clock::system(), &blank);
        assert_eq!(pic.r.rgba_premul, empty.r.rgba_premul, "{what}: nothing of it is drawn");
    }
    // Copies of a picture, each its own part, each decoded: a budget with room for four decodes and a little decodes four of twenty, and the frame says it was
    // cut.
    let copy = foreign("Bitmap", "image/png", grey_png(1_000));
    let doc = one_page(pictures(20, &copy, false, 0.5));
    let decode = 1_000 * 1_000 / PIXELS_PER_WORK;
    let (pic, work, renderer) = drawn(tight(4 * decode + 20 * (DRAWN + 200)), Clock::system(), &doc);
    assert_eq!(renderer.images_decoded(), 4, "{work}: {:?}", pic.r.skipped);
    assert!(pic.r.count(SkipKind::WorkLimit) >= 1, "{:?}", pic.r.skipped);
    // A metafile of fifty thousand items, shown by forty shapes: the items are counted, a frame's cap of them holds, and a budget cuts them where it runs out.
    let mut records = brush_no_pen(1, rgb(200, 0, 0));
    records.extend((0..50_000i32).map(|k| rect_record(k % 200, k / 200, k % 200 + 3, k / 200 + 3)));
    let meta = foreign("EnhMetaFile", "image/x-emf", emf(&records));
    let doc = one_page(pictures(40, &meta, true, 0.5));
    let (pic, work, _) = drawn(Limits::preview(), Clock::system(), &doc);
    assert!(work >= MAX_METAFILE_ITEMS_PER_FRAME as u64, "{work}");
    assert!(pic.r.count(SkipKind::MetafileCapReached) >= 1, "{:?}", pic.r.skipped);
    let (pic, work, _) = drawn(tight(100_000), Clock::system(), &doc);
    assert!(work <= 100_000 && pic.r.count(SkipKind::WorkLimit) >= 1, "{work}: {:?}", pic.r.skipped);
    // The bitmap of a metafile is decoded within the work left: a room a unit short of its pixels leaves it waiting, and decodes nothing.
    let doc = one_page(vec![geometry_of(shape_at(1, 0.5, 0.5, 2.0, 1.0), rect_rows(2.0, 1.0)).foreign(foreign(
        "EnhMetaFile",
        "image/x-emf",
        metafile_with_bitmap(1_000),
    ))]);
    let (_, work, renderer) = drawn(Limits::preview(), Clock::system(), &doc);
    assert_eq!(renderer.images_decoded(), 1);
    assert!(work >= 1_000 * 1_000 / PIXELS_PER_WORK, "{work}");
    let (pic, _, renderer) = drawn(tight(work - 1_000_000 / PIXELS_PER_WORK), Clock::system(), &doc);
    assert_eq!(renderer.images_decoded(), 0, "{:?}", pic.r.skipped);
    assert!(pic.r.count(SkipKind::WorkLimit) >= 1, "{:?}", pic.r.skipped);
}

/// The hostile pages a clock once bounded: the same long text in many shapes (laid out once, painted for every one), a page of the preview's own count of
/// shapes that show one picture that decodes to 16 million pixels, and the same page with a metafile of many items in each (whose frame's cap of items holds
/// first). **Each preview is cut by a count, is the same frame on two runs, with every deadline passed and on several threads, and is never cut by a time.**
/// (Copies of a big picture, each decoded, are the engine's timed test: their decodes fill the renderer's cache of pictures, half a gigabyte.)
#[test]
fn a_preview_of_a_hostile_page_is_cut_by_its_work_and_is_the_same_on_every_run() {
    let long = "The quick brown fox jumps over the lazy dog. ".repeat(360);
    let texts: Vec<ShapeBuilder> = (0..300u32).map(|k| labelled(k + 1, f64::from(k % 20) * 0.19, f64::from(k / 20) * 0.25, &long)).collect();
    let big = foreign("Bitmap", "image/png", grey_png(4_000));
    let mut records = brush_no_pen(1, rgb(200, 0, 0));
    records.extend((0..50_000i32).map(|k| rect_record(k % 200, k / 200, k % 200 + 3, k / 200 + 3)));
    let meta = foreign("EnhMetaFile", "image/x-emf", emf(&records));
    // Twenty thousand shapes: more than a picture of 256 pixels shows, and fewer than the preview's work draws of plain boxes.
    let count = 20_000;
    let pages = [
        ("the same long text in 300 shapes", one_page(texts)),
        ("one big picture in every shape", one_page(pictures(count, &big, true, 0.5))),
        ("a metafile of 50,000 items in every shape", one_page(pictures(count, &meta, true, 0.5))),
    ];
    for (what, doc) in &pages {
        let (first, work, renderer) = drawn(Limits::preview(), Clock::system(), doc);
        assert!(work <= PREVIEW_MAX_WORK, "{what}: {work}");
        let cut_by = if what.contains("metafile") { SkipKind::MetafileCapReached } else { SkipKind::WorkLimit };
        assert!(first.r.count(cut_by) >= 1, "{what}: cut by a count: {:?}", first.r.skipped);
        assert!(TIMED.iter().all(|k| first.r.count(*k) == 0), "{what}: {:?}", first.r.skipped);
        if what.contains("picture") {
            assert_eq!(renderer.images_decoded(), 1, "{what}: the picture is decoded once");
        }
        let (again, again_work, _) = drawn(Limits::preview(), late(), doc);
        assert_eq!(first.r, again.r, "{what}: the same frame with every deadline passed");
        assert_eq!(work, again_work, "{what}");
        let (shared, _, _) = drawn_on(Limits::preview(), Clock::system(), 4, doc);
        assert_eq!(first.r, shared.r, "{what}: the same frame on four threads");
    }
    // The text is laid out once, however many shapes show it: what the budget cut is the painting.
    let (_, _, renderer) = drawn(Limits::preview(), Clock::system(), &pages[0].1);
    assert_eq!(renderer.texts_laid_out(), 1);
}
