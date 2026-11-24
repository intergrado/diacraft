//! A frame drawn on several threads is the frame drawn on one, byte for byte: the pixels and the list of what was left out.
//!
//! Every test draws a scene at one thread (the renderer's own way) and at 2, 7 and 31, with a floor of nothing so that a small page is shared
//! out too, and says the frames are the same **and** that the many-thread frames were drawn on many threads (`Renderer::par_stats`): a test
//! that drew on one thread every time would pass for ever.

use diacraft_doc::{Document, RowKey};
use diacraft_render::print::{PrintColor, plan_job};
use diacraft_render::{Cancel, Limits, ParStats, Parallel, Rendered, Renderer, SkipKind, View};
use diacraft_testkit::build::{PageBuilder, ShapeBuilder};
use kurbo::Affine;

use crate::frame_time::{diagram, diagram_on};
use crate::pictures::{foreign, png_of};
use crate::support::*;

/// The thread counts every scene is drawn at besides one: an even count, an odd prime and the machine's biggest.
const COUNTS: [usize; 3] = [2, 7, 31];

/// A renderer that shares every frame out over `threads` threads, however small.
fn renderer_on(threads: usize) -> Renderer {
    let mut renderer = Renderer::new();
    renderer.set_parallel(Parallel::eager(threads));
    renderer
}

/// Says where the two frames first differ, and by how much.
fn differ(one: &Rendered, many: &Rendered) -> String {
    let count = one.rgba_premul.chunks(4).zip(many.rgba_premul.chunks(4)).filter(|(a, b)| a != b).count();
    let first = one.rgba_premul.chunks(4).zip(many.rgba_premul.chunks(4)).position(|(a, b)| a != b);
    match first {
        Some(i) => {
            let w = one.width as usize;
            let (a, b) = (&one.rgba_premul[i * 4..i * 4 + 4], &many.rgba_premul[i * 4..i * 4 + 4]);
            format!("{count} pixels differ; the first at ({}, {}): {a:?} against {b:?}", i % w, i / w)
        }
        None => "the same pixels".to_owned(),
    }
}

/// What a scene's frames at many threads were drawn by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Way {
    /// The walk shared out and the bands drawn on threads.
    Shared,
    /// The walk done in order (a picture, a budget) onto the canvas, as on one thread: no bands.
    InOrder,
}

/// Draws `doc` at one thread and at each of [`COUNTS`] and says the frames are the same, and drawn the way `way` says.
fn assert_same_at_every_count(what: &str, doc: &Document, view: &View, way: Way) -> Rendered {
    let one = Renderer::new().render(doc, 1, view);
    assert_eq!(one.rgba_premul.len(), view.width as usize * view.height as usize * 4, "{what}: the frame has its pixels");
    for threads in COUNTS {
        let mut renderer = renderer_on(threads);
        let many = renderer.render(doc, 1, view);
        assert_eq!((many.width, many.height), (one.width, one.height));
        assert!(many.rgba_premul == one.rgba_premul, "{what}: at {threads} threads {}", differ(&one, &many));
        assert_eq!(many.skipped, one.skipped, "{what}: at {threads} threads what was left out differs");
        // A frame of 64 rows or fewer is one band, which is no sharing of its pixels.
        let banded = u64::from(view.height > 64);
        let want = match way {
            Way::Shared => (1, 0, banded),
            Way::InOrder => (0, 1, 0),
        };
        assert_eq!(counts_of(&renderer), want, "{what}: at {threads} threads the frame was not drawn the way the test means");
    }
    one
}

/// How many frames of a renderer had their walk shared, walked in order, and their pixels drawn in bands.
fn counts_of(renderer: &Renderer) -> (u64, u64, u64) {
    let stats: ParStats = renderer.par_stats();
    (stats.walks_shared, stats.walks_in_order, stats.frames_banded)
}

fn view_of(width: u32, height: u32, scale: f64) -> View {
    View { to_px: Affine::new([scale, 0.0, 0.0, -scale, 0.0, f64::from(height)]), width, height }
}

/// A small generator of numbers that are the same on every run.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }

    fn unit(&mut self) -> f64 {
        (self.next() % 10_000) as f64 / 10_000.0
    }
}

/// A gradient fill of two stops on a shape, `dir` the layout (0 linear, 3 and others radial and the like), turned by `angle`.
fn with_gradient(mut sb: ShapeBuilder, dir: u32, angle: f64, from: &str, to: &str) -> ShapeBuilder {
    sb = sb.cell("FillGradientEnabled", "1").cell("FillGradientDir", &dir.to_string()).cell("FillGradientAngle", &angle.to_string());
    for (i, (color, pos)) in [(from, 0.0), (to, 1.0)].into_iter().enumerate() {
        let row = RowKey::Ix(i as u32);
        sb = sb
            .row_cell("FillGradient", None, row.clone(), "GradientStopColor", color)
            .row_cell("FillGradient", None, row.clone(), "GradientStopColorTrans", "0")
            .row_cell("FillGradient", None, row, "GradientStopPosition", &pos.to_string());
    }
    sb
}

/// A picture shape with the number `id`, its frame at `(x, y)`, `w` by `h`.
fn picture_at(id: u32, x: f64, y: f64, w: f64, h: f64, image: Vec<u8>) -> ShapeBuilder {
    geometry_of(shape_at(id, x, y, w, h), rect_rows(w, h)).cell("FillPattern", "0").cell("LinePattern", "0").foreign(foreign(
        "Bitmap",
        "image/png",
        image,
    ))
}

/// A page of `columns` by `rows` cells of 1.5 by 1.2 inches, each holding a shape of a kind the seed picks: a solid box, a gradient (linear and
/// radial), a hatch, a turned rounded box with a dashed outline, a line with arrowheads, a group with a labelled member and a labelled box, a
/// blurred or hard shadow on one in three, and, if `pictures`, a picture on one in eleven. Text is of several lengths.
fn scene(seed: u64, columns: u32, rows: u32, pictures: bool) -> Document {
    let mut rng = Lcg(seed);
    let (w, h) = (f64::from(columns) * 1.5 + 0.5, f64::from(rows) * 1.2 + 0.5);
    let mut page =
        PageBuilder::new().cell("PageWidth", &w.to_string()).cell("PageHeight", &h.to_string()).cell("PageScale", "1").cell("DrawingScale", "1");
    let words = ["Start", "Review the request", "Approve", "A longer label that has to wrap in its box", "Done", "Ship"];
    let mut id = 1u32;
    for r in 0..rows {
        for c in 0..columns {
            let (x, y) = (0.25 + f64::from(c) * 1.5, 0.25 + f64::from(r) * 1.2);
            let kind = rng.below(8);
            let color = |rng: &mut Lcg| format!("#{:02X}{:02X}{:02X}", 40 + rng.below(200), 40 + rng.below(200), 40 + rng.below(200));
            let mut sb = rect(id, x, y, 1.2, 0.8).cell("FillForegnd", &color(&mut rng)).cell("LineColor", &color(&mut rng));
            match kind {
                0 => {}
                1 => sb = with_gradient(sb, 0, rng.unit() * std::f64::consts::TAU, &color(&mut rng), &color(&mut rng)),
                2 => sb = with_gradient(sb, [1, 2, 3, 10][rng.below(4) as usize], 0.0, &color(&mut rng), &color(&mut rng)),
                3 => sb = sb.cell("FillPattern", &(2 + rng.below(30)).to_string()).cell("FillBkgnd", &color(&mut rng)),
                4 => {
                    sb = sb
                        .cell("Rounding", "0.15")
                        .cell("LinePattern", &(2 + rng.below(8)).to_string())
                        .cell("LineWeight", &(0.01 + rng.unit() * 0.05).to_string())
                        .cell("PinX", &(x + 0.6).to_string())
                        .cell("PinY", &(y + 0.4).to_string())
                        .cell("LocPinX", "0.6")
                        .cell("LocPinY", "0.4")
                        .cell("Angle", &(rng.unit() * std::f64::consts::TAU).to_string());
                }
                5 => {
                    sb = geometry_of(
                        shape_at(id, x, y, 1.2, 0.0),
                        vec![("MoveTo", vec![("X", 0.0), ("Y", 0.0)]), ("LineTo", vec![("X", 1.2), ("Y", 0.0)])],
                    )
                    .cell("EndArrow", &(1 + rng.below(12)).to_string())
                    .cell("BeginArrow", &(1 + rng.below(12)).to_string())
                    .cell("LineWeight", &(0.01 + rng.unit() * 0.04).to_string());
                }
                6 => {
                    let group = shape_at(id, x, y, 1.2, 0.8).group();
                    id += 1;
                    let a = rect(id, 0.0, 0.0, 0.5, 0.8).cell("FillForegnd", &color(&mut rng));
                    id += 1;
                    let b = rect(id, 0.6, 0.0, 0.6, 0.8).cell("FillForegnd", &color(&mut rng)).text(words[rng.below(6) as usize]);
                    sb = group.child(a).child(b);
                }
                _ => sb = sb.text(words[rng.below(6) as usize]).cell("FillPattern", "1"),
            }
            if rng.below(3) == 0 && kind != 5 {
                let blur = if rng.below(2) == 0 { 0.0 } else { 0.03 + rng.unit() * 0.3 };
                sb = sb
                    .cell("ShdwPattern", "1")
                    .cell("ShapeShdwType", "1")
                    .cell("ShapeShdwOffsetX", "0.06")
                    .cell("ShapeShdwOffsetY", "-0.06")
                    .cell("ShapeShdwBlur", &blur.to_string())
                    .cell("ShdwForegnd", "#505050");
            }
            if pictures && id.is_multiple_of(11) {
                let image = png_of(16, 16, |x, y| [(x * 16) as u8, (y * 16) as u8, 128, 255]);
                sb = picture_at(id, x, y, 1.0, 0.7, image);
            }
            page = page.shape(sb);
            id += 1;
        }
    }
    base().page(1, page).build()
}

#[test]
fn a_page_of_thousands_of_shapes_is_the_same_at_every_thread_count() {
    assert_same_at_every_count("2,130 shapes", &diagram(36, 30, false), &view_of(1800, 1200, 30.0), Way::Shared);
}

#[test]
fn blurred_shadows_are_the_same_at_every_thread_count() {
    // 276 shapes, every one with a blurred shadow: under the frame's cap of blurs, so the walk is shared out and each band draws the layers it meets.
    assert_same_at_every_count("shadows", &diagram(12, 12, true), &view_of(1800, 1200, 30.0), Way::Shared);
}

#[test]
fn past_the_cap_of_blurs_the_walk_is_done_in_order_and_the_frame_is_the_same() {
    // 1,730 of 2,130 shadows are drawn hard-edged because the frame blurs 400: which ones depends on the order, so the walk is in order.
    let one = assert_same_at_every_count("more blurs than a frame draws", &diagram(36, 30, true), &view_of(1800, 1200, 30.0), Way::InOrder);
    assert!(one.count(SkipKind::ShadowApproximate) > 0, "the scene reaches the cap");
}

#[test]
fn a_mixed_page_is_the_same_at_every_thread_count() {
    // Gradients (linear and radial), hatches, dashes, turned shapes, arrowheads, groups, text and shadows, on four seeds, each whole and zoomed in
    // so that shapes are cut by every edge of the view.
    for seed in [1u64, 7, 99, 2024] {
        let doc = scene(seed, 14, 9, false);
        let (pw, ph) = (14.0_f64 * 1.5 + 0.5, 9.0_f64 * 1.2 + 0.5);
        assert_same_at_every_count(&format!("seed {seed}"), &doc, &view_of(1400, 900, (1400.0 / pw).min(900.0 / ph)), Way::Shared);
        let zoomed = View { to_px: Affine::new([90.0, 0.0, 0.0, -90.0, -300.0, 700.0]), width: 1203, height: 797 };
        assert_same_at_every_count(&format!("seed {seed} zoomed"), &doc, &zoomed, Way::Shared);
    }
}

#[test]
fn a_page_seen_turned_and_sheared_is_the_same_at_every_thread_count() {
    let doc = scene(5, 10, 8, false);
    let turned = View { to_px: Affine::new([14.0, 9.0, -9.0, 14.0, 520.0, -60.0]), width: 1001, height: 777 };
    assert_same_at_every_count("turned view", &doc, &turned, Way::Shared);
}

#[test]
fn pictures_make_the_walk_in_order_and_the_frame_is_the_same() {
    // A picture is decoded and kept in order, so a page with one is walked on one thread, and drawn onto the canvas as on one thread.
    let doc = scene(3, 12, 8, true);
    let one = assert_same_at_every_count("pictures", &doc, &view_of(1200, 800, 60.0), Way::InOrder);
    assert!(one.rgba_premul.chunks(4).any(|p| p[2] == 128 && p[3] == 255), "a picture is drawn");
}

#[test]
fn a_frame_of_odd_sizes_is_the_same_at_every_thread_count() {
    // Sizes that are not a multiple of a tile (4), of a band, or of the rasteriser's wide tile (256).
    let doc = scene(11, 6, 5, false);
    for (w, h) in [(257, 129), (1, 1), (3, 701), (999, 2), (513, 67), (1801, 71)] {
        assert_same_at_every_count(&format!("{w} by {h}"), &doc, &view_of(w, h, 40.0), Way::Shared);
    }
}

#[test]
fn a_band_edge_through_a_blurred_shadow_and_through_text_changes_nothing() {
    // A column of labelled boxes with soft shadows, one every 0.45 inch (a band edge falls across a shadow and a line of text at every count).
    let mut page = PageBuilder::new().cell("PageWidth", "4").cell("PageHeight", "30").cell("PageScale", "1").cell("DrawingScale", "1");
    for i in 0..60u32 {
        let y = 0.3 + f64::from(i) * 0.47;
        let sb = rect(i + 1, 1.0, y, 2.0, 0.4)
            .cell("FillForegnd", "#FFD27F")
            .cell("ShdwPattern", "1")
            .cell("ShapeShdwType", "1")
            .cell("ShapeShdwOffsetX", "0.1")
            .cell("ShapeShdwOffsetY", "-0.1")
            .cell("ShapeShdwBlur", "0.25")
            .cell("ShdwForegnd", "#303030")
            .text("Soft edges and sharp letters");
        page = page.shape(sb);
    }
    let doc = base().page(1, page).build();
    assert_same_at_every_count("a column of shadows and text", &doc, &view_of(400, 3000, 100.0), Way::Shared);
}

#[test]
fn a_clipped_picture_frame_and_hairlines_are_the_same_at_every_thread_count() {
    // Clips: a picture is cut to its frame and a rectangular gradient is stepped in bands inside a clip; hairlines are one pixel at any count.
    let mut page = PageBuilder::new().cell("PageWidth", "8").cell("PageHeight", "6").cell("PageScale", "1").cell("DrawingScale", "1");
    for i in 0..20u32 {
        let x = 0.2 + f64::from(i % 5) * 1.5;
        let y = 0.2 + f64::from(i / 5) * 1.4;
        let sb = if i % 2 == 0 {
            let image = png_of(8, 8, |px, py| [(px * 30) as u8, (py * 30) as u8, 200, 255]);
            picture_at(i + 1, x, y, 1.2, 1.0, image)
        } else {
            with_gradient(rect(i + 1, x, y, 1.2, 1.0).cell("LineWeight", "0"), 10, 0.0, "#FF0000", "#0000FF")
        };
        page = page.shape(sb);
    }
    let doc = base().page(1, page).build();
    assert_same_at_every_count("clips", &doc, &view_of(800, 600, 100.0), Way::InOrder);
}

#[test]
fn a_page_with_a_background_page_is_the_same_at_every_thread_count() {
    let mut back = PageBuilder::new().background().cell("PageWidth", "6").cell("PageHeight", "4").cell("PageScale", "1").cell("DrawingScale", "1");
    for i in 0..30u32 {
        back = back.shape(rect(i + 1, 0.1 + f64::from(i % 10) * 0.55, 0.1 + f64::from(i / 10) * 1.2, 0.5, 1.0).cell("FillForegnd", "#B0C4DE"));
    }
    let mut front = PageBuilder::new().back_page(2).cell("PageWidth", "6").cell("PageHeight", "4").cell("PageScale", "1").cell("DrawingScale", "1");
    for i in 0..30u32 {
        front = front.shape(rect(i + 1, 0.3 + f64::from(i % 6), 0.4 + f64::from(i / 6) * 0.7, 0.8, 0.5).cell("FillForegnd", "#CD5C5C").text("front"));
    }
    let doc = base().page(1, front).page(2, back).build();
    assert_same_at_every_count("backgrounds", &doc, &view_of(600, 400, 100.0), Way::Shared);
}

#[test]
fn a_frame_that_reaches_a_count_is_the_same_at_every_thread_count() {
    // Counts that a share cannot see: the frame's shapes, its rows of geometry and its characters of text. The walk is done in order, and the
    // frame says what it left out, the same at every count.
    let doc = scene(21, 12, 9, false);
    let view = view_of(1200, 900, 60.0);
    for (what, limits) in [
        ("shapes", Limits { max_shapes: 40, ..Limits::default() }),
        ("rows", Limits { max_rows: 300, ..Limits::default() }),
        ("characters", Limits { max_text_chars: 60, ..Limits::default() }),
    ] {
        let mut one = Renderer::new();
        one.set_limits(limits);
        let a = one.render(&doc, 1, &view);
        assert!(!a.skipped.is_empty(), "{what}: the scene reaches the limit");
        for threads in COUNTS {
            let mut many = renderer_on(threads);
            many.set_limits(limits);
            let b = many.render(&doc, 1, &view);
            assert!(a.rgba_premul == b.rgba_premul, "{what}: at {threads} threads {}", differ(&a, &b));
            assert_eq!(a.skipped, b.skipped, "{what}: at {threads} threads what was left out differs");
            assert_eq!(counts_of(&many).1, 1, "{what}: at {threads} threads the walk was not done in order");
        }
    }
}

#[test]
fn a_frame_a_newer_request_stopped_is_stopped_at_every_thread_count() {
    use std::sync::Arc;
    use std::sync::atomic::AtomicU64;
    let doc = scene(8, 12, 9, false);
    let view = view_of(1200, 900, 60.0);
    for threads in [1usize, 2, 31] {
        let mut renderer = renderer_on(threads);
        // The newest request is 5 and this frame is number 3: it is stale from the start.
        renderer.set_cancel(Some(Cancel::new(Arc::new(AtomicU64::new(5)), 3)));
        let r = renderer.render(&doc, 1, &view);
        assert!(r.rgba_premul.is_empty(), "at {threads} threads a stopped frame has no pixels");
        assert!(r.count(SkipKind::Cancelled) > 0, "at {threads} threads it says so");
        // A request that is not stale draws.
        renderer.set_cancel(Some(Cancel::new(Arc::new(AtomicU64::new(3)), 3)));
        assert_eq!(renderer.render(&doc, 1, &view).rgba_premul.len(), 1200 * 900 * 4);
    }
}

#[test]
fn a_small_page_is_not_shared_out_under_the_default_floor() {
    let doc = scene(2, 4, 3, false);
    let mut renderer = Renderer::new();
    renderer.set_parallel(Parallel::with_threads(31));
    let r = renderer.render(&doc, 1, &view_of(600, 400, 60.0));
    assert_eq!(counts_of(&renderer), (0, 0, 0), "twelve shapes cost less on one thread than the threads would");
    assert_eq!(r, Renderer::new().render(&doc, 1, &view_of(600, 400, 60.0)));
}

#[test]
fn the_same_renderer_draws_the_same_frame_again_and_after_the_page_changes() {
    // The kept layouts belong to each share: a second frame finds them, a changed page is drawn from its cells.
    let doc = diagram_on(20, 14, false, 40.0, 20.0);
    let view = view_of(1600, 800, 40.0);
    let mut renderer = renderer_on(7);
    let first = renderer.render(&doc, 1, &view);
    let laid = renderer.texts_laid_out();
    let second = renderer.render(&doc, 1, &view);
    assert!(first.rgba_premul == second.rgba_premul, "{}", differ(&first, &second));
    assert_eq!(renderer.texts_laid_out(), laid, "the second frame lays nothing out again");
    assert_eq!(counts_of(&renderer).0, 2);
    let other = diagram_on(21, 14, false, 40.0, 20.0);
    let a = renderer.render(&other, 1, &view);
    assert!(a.rgba_premul == Renderer::new().render(&other, 1, &view).rgba_premul);
}

#[test]
fn the_shares_of_a_page_that_passes_a_budget_stop_together_and_do_not_each_walk_to_the_whole_of_it() {
    // 2,130 shapes in 62 shares of about 34 each. A budget that the page passes as a whole but no share passes alone used to let every share
    // walk to its own end before the sums were added: the work was N times a budget's. Now the shares say what they spend, and stop when the
    // sum passes: a few shapes' work, counted in `share_shapes`, whatever the thread count.
    let view = view_of(1800, 1200, 30.0);
    let plain = diagram(36, 30, false);
    let shadowed = diagram(36, 30, true);
    let shapes = plain.page(1).map_or(0, |p| p.shapes.len());
    assert_eq!(shapes, 2130);
    for (what, doc, limits) in [
        ("shapes", &plain, Limits { max_shapes: 300, ..Limits::default() }),
        ("rows", &plain, Limits { max_rows: 300, ..Limits::default() }),
        ("characters", &plain, Limits { max_text_chars: 60, ..Limits::default() }),
        ("blurred shadows", &shadowed, Limits::default()),
    ] {
        let mut one = Renderer::new();
        one.set_limits(limits);
        let a = one.render(doc, 1, &view);
        for threads in COUNTS {
            let mut many = renderer_on(threads);
            many.set_limits(limits);
            let b = many.render(doc, 1, &view);
            assert!(a.rgba_premul == b.rgba_premul, "{what}: at {threads} threads {}", differ(&a, &b));
            assert_eq!(a.skipped, b.skipped, "{what}: at {threads} threads what was left out differs");
            let stats = many.par_stats();
            assert_eq!(stats.walks_in_order, 1, "{what}: at {threads} threads the page was walked in order");
            assert!(
                stats.share_shapes < 1065,
                "{what}: at {threads} threads the shares walked {} of {shapes} shapes before they stopped",
                stats.share_shapes
            );
        }
    }
}

#[test]
fn the_threads_of_a_frames_bands_are_held_to_the_budgets_of_pixels_and_layers() {
    let view = view_of(1800, 1200, 30.0);
    let plain = diagram(36, 30, false);
    let shadowed = diagram(12, 12, true);
    let frame_bytes = 1800 * 1200 * 4;
    let drawn = |doc: &Document, par: Parallel| {
        let mut renderer = Renderer::new();
        renderer.set_parallel(par);
        let r = renderer.render(doc, 1, &view);
        (r, renderer.par_stats())
    };
    let one = Renderer::new().render(&plain, 1, &view);
    // Pixels of the threads' contexts: a budget of two frames' worth is two threads, of less than one frame is one thread (the frame is drawn
    // whole, not in bands), and the budget the frame is given (256 MiB) is every thread.
    let (r, stats) = drawn(&plain, Parallel { scratch_bytes: 2 * frame_bytes, ..Parallel::eager(31) });
    assert!(r.rgba_premul == one.rgba_premul, "{}", differ(&one, &r));
    assert_eq!(stats.most_band_threads, 2, "two frames' worth of pixels hold two threads");
    let (r, stats) = drawn(&plain, Parallel { scratch_bytes: frame_bytes - 1, ..Parallel::eager(31) });
    assert!(r.rgba_premul == one.rgba_premul);
    assert_eq!((stats.most_band_threads, stats.frames_banded), (1, 0), "less than a frame's worth of pixels is one thread, no bands");
    let (_, stats) = drawn(&plain, Parallel::eager(31));
    assert!(stats.most_band_threads > 8, "with the frame's budget the bands are drawn by many threads: {}", stats.most_band_threads);
    // Pixels of the blur layers: no room for any is one thread, room for all of them every thread.
    let one = Renderer::new().render(&shadowed, 1, &view);
    let (r, stats) = drawn(&shadowed, Parallel { blur_pixels: 1, ..Parallel::eager(31) });
    assert!(r.rgba_premul == one.rgba_premul, "{}", differ(&one, &r));
    assert_eq!(
        (stats.most_band_threads, stats.frames_banded),
        (1, 0),
        "layers that do not fit the budget of the frame two at a time are drawn one at a time"
    );
    let (r, stats) = drawn(&shadowed, Parallel::eager(31));
    assert!(r.rgba_premul == one.rgba_premul);
    assert!(stats.most_band_threads > 8, "layers that fit the budget are drawn by many threads: {}", stats.most_band_threads);
}

#[test]
fn a_frame_with_more_steps_than_the_recordings_hold_is_walked_once_and_is_the_same() {
    // 2,130 shapes of about ten steps each against a cap of 500 steps. The shares stop when their sum passes it (a few shapes' work), and the walk
    // in order is drawn onto the canvas: the shapes are walked once, not once to find out that the recordings are too long and again to draw them.
    let doc = diagram(36, 30, false);
    let view = view_of(1800, 1200, 30.0);
    let shapes = doc.page(1).map_or(0, |p| p.shapes.len()) as u64;
    let one = Renderer::new().render(&doc, 1, &view);
    for threads in COUNTS {
        let mut renderer = Renderer::new();
        renderer.set_parallel(Parallel { max_steps: 500, ..Parallel::eager(threads) });
        let r = renderer.render(&doc, 1, &view);
        assert!(r.rgba_premul == one.rgba_premul, "at {threads} threads {}", differ(&one, &r));
        assert_eq!(r.skipped, one.skipped);
        let stats = renderer.par_stats();
        assert_eq!((stats.frames_banded, stats.walks_in_order), (0, 1), "at {threads} threads: {stats:?}");
        assert_eq!(stats.walked_shapes, shapes, "at {threads} threads the walk in order is the only full walk");
        assert!(
            stats.share_shapes < shapes / 3,
            "at {threads} threads the shares walked {} of {shapes} shapes before they stopped",
            stats.share_shapes
        );
    }
}

#[test]
fn a_frame_with_no_time_left_walks_next_to_nothing_in_the_shares_or_in_order() {
    // The walk in order that follows shares that stopped counts from the frame's own start (`Renderer::walk_started`): a frame with no time left
    // walks nothing more.
    let doc = diagram(36, 30, false);
    let view = view_of(1800, 1200, 30.0);
    let mut renderer = renderer_on(7);
    renderer.set_limits(Limits { max_shapes: 300, frame_time: Some(std::time::Duration::ZERO), ..Limits::default() });
    let r = renderer.render(&doc, 1, &view);
    assert!(r.count(SkipKind::FrameBudget) + r.count(SkipKind::ShapeLimit) > 0, "{:?}", r.skipped);
    let stats = renderer.par_stats();
    assert!(stats.walked_shapes + stats.share_shapes < 300, "no time: {stats:?}");
}

#[test]
fn a_sheet_of_a_print_is_the_same_at_every_thread_count_in_colour_and_in_greys() {
    let doc = scene(13, 5, 6, false);
    let sheet = plan_job(&doc, &[1]).expect("plans").sheet(0).map(|(_, s)| s.clone()).expect("a sheet");
    for color in [PrintColor::Color, PrintColor::Grayscale] {
        let mut one = Renderer::new();
        one.set_print_color(color);
        let a = one.render_sheet(&doc, &sheet, 100.0);
        assert!(a.rgba_premul.chunks(4).any(|p| p[3] > 0), "the sheet has ink");
        for threads in COUNTS {
            let mut many = renderer_on(threads);
            many.set_print_color(color);
            let b = many.render_sheet(&doc, &sheet, 100.0);
            assert!(a.rgba_premul == b.rgba_premul, "{color:?} at {threads} threads {}", differ(&a, &b));
            assert_eq!(a.skipped, b.skipped);
            assert_eq!(counts_of(&many), (1, 0, 1), "{color:?} at {threads} threads: the sheet was shared out");
        }
    }
}
