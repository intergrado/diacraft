//! Hostile input: a drawing and a view are as hostile as the file they came from. Each test is crafted, run under a hang guard of a minute
//! (no cost is held to a time: the counts and the report of what was left out are what the tests assert), and checks that the frame ends with
//! a picture and a report and never a panic.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use diacraft_doc::Document;
use diacraft_render::{Cancel, Limits, Renderer, SkipKind, View};
use diacraft_testkit::build::ShapeBuilder;
use kurbo::Affine;

use crate::support::*;

/// A hang guard and no measure of cost: the slowest frame here takes a few seconds at load 50.
const BOUND: Duration = Duration::from_secs(60);

fn timed<T>(f: impl FnOnce() -> T) -> T {
    let started = Instant::now();
    let out = f();
    assert!(started.elapsed() < BOUND, "took {:?}", started.elapsed());
    out
}

#[test]
fn many_shapes_end_at_the_count_limit_and_say_how_many_were_left_out() {
    let shapes: Vec<ShapeBuilder> = (0..30_000u32)
        .map(|i| rect(i + 1, f64::from(i % 150) * 0.026, f64::from(i / 150) * 0.019, 0.02, 0.015).cell("FillForegnd", "#FF0000"))
        .collect();
    let doc = one_page(shapes);
    let mut renderer = Renderer::new();
    renderer.set_limits(Limits { max_shapes: 5_000, ..Limits::unlimited_time() });
    let pic = timed(|| draw_with(&mut renderer, &doc));
    assert!(pic.ink_pixels() > 0, "the first shapes are drawn");
    assert!(pic.r.count(SkipKind::ShapeLimit) >= 25_000, "the rest are counted: {:?}", pic.r.skipped);
}

#[test]
fn a_frame_stops_at_its_time_budget_and_reports_what_it_left_out() {
    let shapes: Vec<ShapeBuilder> = (0..3_000u32).map(|i| rect(i + 1, f64::from(i % 60) * 0.06, f64::from(i / 60) * 0.07, 0.05, 0.05)).collect();
    let doc = one_page(shapes);
    let mut renderer = Renderer::new();
    renderer.set_limits(Limits { frame_time: Some(Duration::ZERO), ..Limits::default() });
    let pic = timed(|| draw_with(&mut renderer, &doc));
    assert!(pic.r.count(SkipKind::FrameBudget) > 0, "{:?}", pic.r.skipped);
}

#[test]
fn coordinates_that_are_huge_or_not_numbers_draw_nothing_and_fail_nothing() {
    for value in ["1e300", "-1e300", "1.#INF", "-1.#INF", "NaN", "1e15", "9e18"] {
        let sb = rect(1, 1.0, 1.0, 2.0, 2.0).cell("PinX", value).cell("Width", value).cell("LocPinX", value).cell("Angle", value);
        let pic = timed(|| draw(&one_page(vec![sb])));
        assert_eq!(pic.r.rgba_premul.len(), 400 * 400 * 4, "a frame for {value}");
    }
    // Huge geometry on a sane shape.
    let sb = geometry_of(
        shape_at(1, 1.0, 1.0, 2.0, 2.0),
        vec![("MoveTo", vec![("X", 0.0), ("Y", 0.0)]), ("LineTo", vec![("X", 1.0e300), ("Y", 1.0e300)])],
    );
    timed(|| draw(&one_page(vec![sb])));
}

#[test]
fn a_huge_line_weight_is_clamped_and_reported() {
    let sb = rect(1, 1.0, 1.0, 2.0, 2.0).cell("LineWeight", "1e12").cell("LineColor", "#000000");
    let pic = timed(|| draw(&one_page(vec![sb])));
    assert!(pic.r.count(SkipKind::StrokeClamped) >= 1, "{:?}", pic.r.skipped);
}

#[test]
fn a_dash_pattern_that_would_make_millions_of_dashes_is_drawn_solid() {
    let sb =
        geometry_of(shape_at(1, 0.0, 2.0, 5.0e4, 0.0), vec![("MoveTo", vec![("X", 0.0), ("Y", 0.0)]), ("LineTo", vec![("X", 5.0e4), ("Y", 0.0)])])
            .cell("FillPattern", "0")
            .cell("LinePattern", "2")
            .cell("LineWeight", "0.001");
    let pic = timed(|| draw(&one_page(vec![sb])));
    assert!(pic.r.count(SkipKind::DashesTooMany) >= 1, "{:?}", pic.r.skipped);
}

#[test]
fn groups_nested_to_the_limit_and_beyond_do_not_overflow_the_stack() {
    let nest = |depth: usize| {
        let mut shape = rect(1, 1.0, 1.0, 1.0, 1.0).cell("FillForegnd", "#FF0000");
        for level in 0..depth {
            shape = shape_at(2 + level as u32, 0.0, 0.0, 4.0, 4.0).group().child(shape);
        }
        shape
    };
    let pic = timed(|| draw(&one_page(vec![nest(150)])));
    assert!(pic.is_red(1.5, 1.5), "a member 150 levels down still draws");
    let deep = timed(|| draw(&one_page(vec![nest(260)])));
    assert!(deep.r.count(SkipKind::TooDeep) >= 1 || deep.ink_pixels() > 0, "{:?}", deep.r.skipped);
}

#[test]
fn a_view_that_would_overflow_a_pixel_buffer_is_refused_with_a_reason() {
    let doc = one_page(vec![rect(1, 1.0, 1.0, 2.0, 2.0)]);
    let mut renderer = Renderer::new();
    for (w, h) in [(u32::MAX, u32::MAX), (100_000, 100_000), (16_384, 16_384), (0, 10), (10, 0), (70_000, 1), (16_385, 1), (1, 20_000)] {
        let view = View { to_px: Affine::IDENTITY, width: w, height: h };
        let r = timed(|| renderer.render(&doc, 1, &view));
        assert!(r.rgba_premul.is_empty() && r.width == 0 && r.height == 0, "{w} by {h}");
        assert_eq!(r.count(SkipKind::ViewRefused), 1);
    }
    let bad = View { to_px: Affine::new([f64::NAN, 0.0, 0.0, 1.0, 0.0, 0.0]), width: 10, height: 10 };
    assert_eq!(renderer.render(&doc, 1, &bad).count(SkipKind::ViewRefused), 1, "a view that is not a number");
}

#[test]
fn a_page_of_a_size_no_buffer_could_hold_still_renders_the_part_in_view() {
    // A page of 1e9 inches, seen through a 100 pixel window at the origin.
    let p = diacraft_testkit::build::PageBuilder::new()
        .cell("PageWidth", "1e9")
        .cell("PageHeight", "1e9")
        .cell("PageScale", "1")
        .cell("DrawingScale", "1")
        .shape(rect(1, 0.1, 0.1, 0.5, 0.5).cell("FillForegnd", "#FF0000"));
    let doc = base().page(1, p).build();
    assert!(View::fit_page(&doc, 1, 96.0).is_none(), "no view fits the whole page");
    let view = View { to_px: Affine::new([100.0, 0.0, 0.0, -100.0, 0.0, 100.0]), width: 100, height: 100 };
    let r = timed(|| Renderer::new().render(&doc, 1, &view));
    assert_eq!(r.rgba_premul.len(), 100 * 100 * 4);
    let px = |x: usize, y: usize| &r.rgba_premul[(y * 100 + x) * 4..(y * 100 + x) * 4 + 4];
    assert_eq!(px(30, 70)[0], 255, "the shape at (0.1, 0.1) to (0.6, 0.6) is red at pixel (30, 70)");
}

#[test]
fn many_text_blocks_of_letters_no_face_draws_cost_a_bounded_time_and_are_counted() {
    // The T7 review: 20 blocks of 2,048 uncovered letters took 1.6 seconds with the machine's fonts. A hundred must not take five times that,
    // so a frame counts the characters it lays out, and the time, and leaves the rest out.
    let text: String = (0..2_048u32).map(|i| char::from_u32(0xE000 + i % 512).unwrap_or('x')).collect();
    let blocks = || -> Vec<ShapeBuilder> {
        (0..100u32)
            .map(|i| {
                rect(i + 1, f64::from(i % 10) * 0.39, f64::from(i / 10) * 0.39, 0.38, 0.38).text(&format!("{i:04}{text}")).row_cell(
                    "Character",
                    None,
                    diacraft_doc::RowKey::Ix(0),
                    "Size",
                    "0.05",
                )
            })
            .collect()
    };
    let doc = one_page(blocks());
    // The character budget: 10,000 characters is four blocks of 2,048.
    let mut renderer = Renderer::new();
    renderer.set_limits(Limits { max_text_chars: 10_000, ..Limits::unlimited_time() });
    let pic = timed(|| draw_with(&mut renderer, &doc));
    assert_eq!(pic.r.count(SkipKind::TextCharBudget), 96, "{:?}", pic.r.skipped);
    // The time budget: spent after the first block, the rest are counted.
    let mut renderer = Renderer::new();
    renderer.set_limits(Limits { text_time: Some(Duration::from_nanos(1)), ..Limits::unlimited_time() });
    let pic = timed(|| draw_with(&mut renderer, &doc));
    assert_eq!(pic.r.count(SkipKind::TextBudget), 99, "{:?}", pic.r.skipped);
    // And the defaults end well inside the bound.
    let pic = timed(|| draw(&doc));
    assert_eq!(pic.r.rgba_premul.len(), 400 * 400 * 4);
}

#[test]
fn a_cycle_of_background_pages_a_dangling_master_and_a_style_loop_end() {
    let a = page(4.0, 4.0).back_page(2).shape(rect(1, 1.0, 1.0, 1.0, 1.0).cell("FillForegnd", "#FF0000"));
    let b = page(4.0, 4.0).background().back_page(1);
    let dangling = ShapeBuilder::new(9).master(77).cell("PinX", "2").cell("PinY", "2").cell("Width", "1").cell("Height", "1");
    let looped = rect(10, 2.0, 2.0, 1.0, 1.0).links(Some(5), Some(5), Some(5));
    let doc = base()
        .style(5, diacraft_testkit::build::StyleBuilder::new().links(Some(6), Some(6), Some(6)))
        .style(6, diacraft_testkit::build::StyleBuilder::new().links(Some(5), Some(5), Some(5)))
        .page(1, a.shape(dangling).shape(looped))
        .page(2, b)
        .build();
    let pic = timed(|| draw(&doc));
    assert!(pic.is_red(1.5, 1.5));
}

#[test]
fn a_section_of_tens_of_thousands_of_rows_is_built_within_the_budget() {
    let mut rows: Vec<(&str, Vec<(&str, f64)>)> = vec![("MoveTo", vec![("X", 0.0), ("Y", 0.0)])];
    for i in 0..20_000u32 {
        rows.push(("LineTo", vec![("X", f64::from(i % 2) * 3.0), ("Y", f64::from(i) * 0.0001)]));
    }
    let sb = geometry_of(shape_at(1, 0.5, 0.5, 3.0, 2.0), rows);
    let pic = timed(|| draw(&one_page(vec![sb])));
    assert_eq!(pic.r.rgba_premul.len(), 400 * 400 * 4);
}

#[test]
fn a_page_that_is_not_there_gives_a_blank_frame_and_a_reason() {
    let doc: Document = one_page(vec![]);
    let view = View { to_px: Affine::IDENTITY, width: 10, height: 10 };
    let r = Renderer::new().render(&doc, 99, &view);
    assert_eq!(r.rgba_premul.len(), 400);
    assert_eq!(r.count(SkipKind::NoSuchPage), 1);
}

#[test]
fn text_with_a_hostile_width_and_size_still_ends() {
    for (w, size) in [("1e300", "1e300"), ("-5", "0"), ("0", "-3"), ("NaN", "NaN"), ("1.#INF", "1.#INF")] {
        let sb = rect(1, 1.0, 1.0, 2.0, 2.0).cell("TxtWidth", w).cell("TxtHeight", w).text("Hello world, a label that wraps").row_cell(
            "Character",
            None,
            diacraft_doc::RowKey::Ix(0),
            "Size",
            size,
        );
        timed(|| draw(&one_page(vec![sb])));
    }
}

#[test]
fn a_shape_far_larger_than_the_rasterisers_coordinates_is_cut_to_the_view_and_still_draws() {
    // A rectangle of two billion inches covering the page, and a line a billion inches long across it: at 100 pixels to the inch their
    // corners are 1e11 pixels away. Refusing them leaves the page white; the part in view must be drawn.
    let big = rect(1, -1.0e9, -1.0e9, 2.0e9, 2.0e9).cell("FillForegnd", "#FF0000").cell("LinePattern", "0");
    let pic = timed(|| draw(&one_page(vec![big])));
    assert!(pic.is_red(2.0, 2.0) && pic.is_red(0.1, 3.9), "the fill covers the view");
    assert_eq!(pic.r.count(SkipKind::OutOfRange), 0, "{:?}", pic.r.skipped);
    let long =
        geometry_of(shape_at(1, 0.0, 2.0, 1.0e9, 0.0), vec![("MoveTo", vec![("X", 0.0), ("Y", 0.0)]), ("LineTo", vec![("X", 1.0e9), ("Y", 0.0)])])
            .cell("FillPattern", "0")
            .cell("LineWeight", "0.1");
    let pic = timed(|| draw(&one_page(vec![long])));
    assert!(pic.is_dark(2.0, 2.0) && pic.is_dark(3.9, 2.0), "the line crosses the view");
    assert!(pic.is_white(2.0, 2.2));
}

#[test]
fn a_slanted_line_from_far_away_crosses_the_view_where_it_should() {
    // A line at 45 degrees from (-1e9, -1e9) to (1e9, 1e9) inches passes through (1, 1), (2, 2) and (3, 3). The rasteriser's 32-bit
    // coordinates, given its ends 1e11 pixels away, would put it thousands of pixels off: the cut keeps it exact.
    let slanted = geometry_of(
        shape_at(1, -1.0e9, -1.0e9, 2.0e9, 2.0e9),
        vec![("MoveTo", vec![("X", 0.0), ("Y", 0.0)]), ("LineTo", vec![("X", 2.0e9), ("Y", 2.0e9)])],
    )
    .cell("FillPattern", "0")
    .cell("LineWeight", "0.1");
    let pic = timed(|| draw(&one_page(vec![slanted])));
    assert!(pic.is_dark(1.0, 1.0) && pic.is_dark(2.0, 2.0) && pic.is_dark(3.0, 3.0), "on the diagonal");
    assert!(pic.is_white(1.0, 3.0) && pic.is_white(3.0, 1.0), "and not off it");
}

/// A page of many cheap shapes, none of which reaches the count or time limits.
fn crowd(n: u32) -> Document {
    let shapes: Vec<ShapeBuilder> = (0..n).map(|i| rect(i + 1, f64::from(i % 150) * 0.026, f64::from(i / 150) * 0.019, 0.02, 0.015)).collect();
    one_page(shapes)
}

#[test]
fn a_frame_asked_for_before_a_newer_one_stops_at_once_and_makes_no_pixels() {
    let doc = crowd(6_000);
    let newest = Arc::new(AtomicU64::new(8));
    let mut renderer = Renderer::new();
    renderer.set_limits(Limits::unlimited_time());
    renderer.set_cancel(Some(Cancel::new(Arc::clone(&newest), 7)));
    let pic = timed(|| draw_with(&mut renderer, &doc));
    assert_eq!(pic.r.count(SkipKind::Cancelled), 6_000, "every shape is counted as left out: {:?}", pic.r.skipped);
    assert!(pic.r.rgba_premul.is_empty(), "the pixels of a stopped frame are not made");
    assert_eq!((pic.r.width, pic.r.height), (400, 400));
    // The newest request itself is never stopped, and a renderer with the stop cleared draws as it did.
    renderer.set_cancel(Some(Cancel::new(newest, 8)));
    let whole = draw_with(&mut renderer, &doc);
    assert_eq!(whole.r.count(SkipKind::Cancelled), 0);
    assert!(whole.ink_pixels() > 0);
    renderer.set_cancel(None);
    assert_eq!(draw_with(&mut renderer, &doc).r.rgba_premul, whole.r.rgba_premul, "a stop that never fires changes no pixel");
}

#[test]
fn a_frame_in_progress_stops_between_shapes_when_a_newer_request_is_numbered() {
    let doc = crowd(30_000);
    let mut renderer = Renderer::new();
    renderer.set_limits(Limits::unlimited_time());
    // How long the frame takes whole on this machine, only so that the newer request lands inside the next one: it comes after an eighth
    // of that, and the frame notices it between two shapes. What is asserted is counted (the shapes left out), not measured.
    let started = Instant::now();
    let whole = draw_with(&mut renderer, &doc);
    let whole_time = started.elapsed();
    assert_eq!(whole.r.count(SkipKind::Cancelled), 0);
    let newest = Arc::new(AtomicU64::new(1));
    renderer.set_cancel(Some(Cancel::new(Arc::clone(&newest), 1)));
    let flipper = {
        let newest = Arc::clone(&newest);
        std::thread::spawn(move || {
            std::thread::sleep(whole_time / 8);
            newest.store(2, Ordering::Relaxed);
        })
    };
    let pic = timed(|| draw_with(&mut renderer, &doc));
    flipper.join().unwrap();
    // Counted, not timed: the shapes the frame left out because it stopped (it ended before it would have if it left any out), and not more
    // than the page has.
    let left_out = pic.r.count(SkipKind::Cancelled);
    assert!((1..=30_000).contains(&left_out), "the frame noticed the newer request and left shapes out: {left_out}, {:?}", pic.r.skipped);
    assert!(pic.r.rgba_premul.is_empty());
}
