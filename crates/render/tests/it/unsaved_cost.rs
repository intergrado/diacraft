//! What a cell with a formula and no saved value costs a page, counted: a page of thousands of shapes that follow a style whose cells are the
//! call `THEMEVAL()` and no value is drawn themed **whole**, and what was worked out for a page is not worked out again for the next frame of it.
//!
//! The answer to a cell is the formula run on the shape that asked, so each shape has its own: a shape of this page reads seven cells, at 25 steps each, and a page of
//! 30,000 shapes is five million steps, past the 4,000,000 that a frame's themed cells have. They were spent on the first 23,000 shapes while the rest were drawn from
//! the defaults.

use std::sync::Arc;

use diacraft_calc::{CellAddress, EvalCtx, EvalOptions, Evaluator, Value};
use diacraft_doc::{Cell, CellAddr, CellEdit, Document, Owner, ShapeRef, SheetRef, WriteOrigin};
use diacraft_render::{Limits, Parallel, Renderer, SkipKind, WorkedCache};
use diacraft_testkit::build::{ShapeBuilder, StyleBuilder};
use diacraft_testkit::theme::ThemeXml;

use crate::support::*;

/// The cells of a fill and a line that the style gives the theme to answer: a dozen, of which the renderer reads seven for a shape it draws.
const THEMED: [&str; 12] = [
    "FillPattern",
    "FillForegnd",
    "FillBkgnd",
    "FillForegndTrans",
    "FillBkgndTrans",
    "FillGradientEnabled",
    "LinePattern",
    "LineColor",
    "LineWeight",
    "LineColorTrans",
    "LineCap",
    "ShdwPattern",
];

/// The columns of the grid the shapes stand in; the pitch is 0.05 inch and a shape 0.04.
const COLUMNS: u32 = 200;

/// A shape of the grid: a small square that links the style `Theme` for its line, fill and text, and holds the cells that select a theme and a Quick Style, as the shapes
/// of a themed drawing do (the scheme cells are read from the shape itself and from the root, never from a style).
fn themed_square(i: u32) -> ShapeBuilder {
    let (col, row) = (f64::from(i % COLUMNS), f64::from(i / COLUMNS));
    let mut sb = rect(i + 1, col * 0.05, row * 0.05, 0.04, 0.04).links(Some(3), Some(3), Some(3));
    for name in ["ThemeIndex", "ColorSchemeIndex", "EffectSchemeIndex", "ConnectorSchemeIndex", "FontSchemeIndex"] {
        sb = sb.cell(name, "40");
    }
    sb.cell("QuickStyleFillColor", "2").cell("QuickStyleFillMatrix", "1").cell("QuickStyleLineColor", "2").cell("QuickStyleLineMatrix", "1")
}

/// A page of `n` small squares that all link the style `Theme`, whose twelve cells hold `THEMEVAL()` and no value, and a document that holds a theme part.
fn themed_by_a_style(n: u32) -> Document {
    let mut style = StyleBuilder::new().name("Theme");
    for name in THEMED {
        style = style.set(CellAddr::direct(name), Cell::with_formula("THEMEVAL()", None));
    }
    let rows = n.div_ceil(COLUMNS);
    let mut p = page(f64::from(COLUMNS) * 0.05, f64::from(rows) * 0.05);
    for i in 0..n {
        p = p.shape(themed_square(i));
    }
    let mut doc = base().style(3, style).page(1, p).build();
    let colours = ["111111", "fefefe", "44546a", "e7e6e6", "5b9bd5", "ed7d31", "a5a5a5", "ffc000", "4472c4", "70ae47", "0563c1", "954f72"];
    doc.themes.push(ThemeXml { colours, ..ThemeXml::default() }.part());
    doc
}

/// What the theme gives the fill of the first shape: the colour the whole page must be drawn in.
fn themed_fill(doc: &Document) -> (u8, u8, u8) {
    let mut ctx = EvalCtx::new(EvalOptions::default(), diacraft_calc::Budget::with_steps(1_000_000));
    let mut ev = Evaluator::new(doc, &mut ctx);
    let cell = CellAddress::new(SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0)), CellAddr::direct("FillForegnd"));
    match ev.evaluate_worked_out(&cell) {
        Some(Value::Color(c)) => (c.r, c.g, c.b),
        other => panic!("the theme answers the fill with a colour, not {other:?}"),
    }
}

/// Where the middle of shape number `i` (from 0) is, in paper inches.
fn centre(i: u32) -> (f64, f64) {
    (f64::from(i % COLUMNS) * 0.05 + 0.02, f64::from(i / COLUMNS) * 0.05 + 0.02)
}

fn draw_whole(renderer: &mut Renderer, doc: &Document) -> Pic {
    renderer.set_limits(Limits::unlimited_time());
    draw_with(renderer, doc)
}

fn fill_at(pic: &Pic, i: u32, want: (u8, u8, u8)) -> bool {
    let (x, y) = centre(i);
    pic.is(x, y, want, 3)
}

#[test]
fn a_page_of_thirty_thousand_shapes_that_follow_a_style_of_unsaved_theme_cells_is_drawn_themed_whole() {
    let doc = themed_by_a_style(30_000);
    let want = themed_fill(&doc);
    assert_ne!(want, (255, 255, 255), "the theme's fill is not the root's white, or the test says nothing");
    let pic = draw_whole(&mut Renderer::new(), &doc);
    assert_eq!(pic.r.count(SkipKind::Recovered), 0, "{:?}", pic.r.skipped);
    assert_eq!(pic.r.count(SkipKind::FrameBudget), 0, "the frame was whole: {:?}", pic.r.skipped);
    assert_eq!(pic.r.count(SkipKind::UnsavedUnanswered), 0, "no cell was left to its default: {:?}", pic.r.skipped);
    // The first, ones in the middle and the last: a frame whose steps ran out on the way draws the later ones from the root's white.
    for i in [0, 1, 9_999, 22_999, 23_001, 29_999] {
        let (x, y) = centre(i);
        assert!(fill_at(&pic, i, want), "shape {i} is drawn in the theme's fill {want:?}: {:?}", pic.at(x, y));
    }
}

/// The document as the frames of a window draw it: one value, behind an `Arc`, and a cache for it that the renderer keeps its answers in.
fn drawn_with_a_cache(renderer: &mut Renderer, doc: &Arc<Document>) -> (Arc<WorkedCache>, Pic) {
    let cache = WorkedCache::new(Arc::clone(doc));
    renderer.set_worked_cache(Some(Arc::clone(&cache)));
    let pic = draw_whole(renderer, doc);
    (cache, pic)
}

#[test]
fn a_second_frame_of_the_same_document_finds_every_answer_the_first_kept_and_works_none_out() {
    // Counted: the cells the frames worked out are the answers the cache was given, once each, and a frame that finds them all gives no more.
    let doc = Arc::new(themed_by_a_style(3_000));
    let want = themed_fill(&doc);
    let mut renderer = Renderer::new();
    let (cache, first) = drawn_with_a_cache(&mut renderer, &doc);
    let worked = cache.evaluated();
    assert!(worked >= 3_000, "at least a cell for each shape was worked out: {worked}");
    assert_eq!(cache.answers() as u64, worked, "each answer was kept once");
    assert!(fill_at(&first, 2_999, want));
    let second = draw_whole(&mut renderer, &doc);
    assert_eq!(cache.evaluated(), worked, "the second frame worked nothing out");
    assert_eq!(second.r.rgba_premul, first.r.rgba_premul, "and drew what the first did");
}

#[test]
fn the_cache_of_one_document_is_not_used_for_a_document_that_was_changed() {
    // An edit makes a new document. A frame of it must not read the answers of the one before: shape 0 now asks the theme for another colour of its list, and the
    // answer kept for it is of the old.
    let before = Arc::new(themed_by_a_style(200));
    let mut renderer = Renderer::new();
    let (cache, first) = drawn_with_a_cache(&mut renderer, &before);
    let old = themed_fill(&before);
    assert!(fill_at(&first, 0, old));
    let mut changed = Document::clone(&before);
    let top = SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0));
    let edit = CellEdit::Value(diacraft_doc::Value::from_text("3"));
    changed.write_cell(&top, &CellAddr::direct("QuickStyleFillColor"), edit, WriteOrigin::Drawing).unwrap();
    let now = themed_fill(&changed);
    assert_ne!(now, old, "the edit moves the theme's answer for the shape");
    let kept = cache.evaluated();
    let second = draw_whole(&mut renderer, &changed);
    assert!(fill_at(&second, 0, now), "shape 0 is drawn from the changed document: {:?}", second.at(centre(0).0, centre(0).1));
    assert!(fill_at(&second, 1, old), "and the shape beside it is as it was");
    assert_eq!(cache.evaluated(), kept, "the cache of the document before was not added to");
}

#[test]
fn a_frame_shared_out_among_threads_is_the_frame_drawn_on_one_with_a_cache_and_without_one() {
    // The shares of a walk each work cells out in a context of their own and put the answers in the one cache; the frame is the same bytes, whoever worked a cell out.
    let doc = Arc::new(themed_by_a_style(3_000));
    let one = draw_whole(&mut Renderer::new(), &doc);
    for threads in [2, 7] {
        for cached in [false, true] {
            let mut renderer = Renderer::new();
            renderer.set_parallel(Parallel::eager(threads));
            let (cache, many) = if cached {
                drawn_with_a_cache(&mut renderer, &doc)
            } else {
                let pic = draw_whole(&mut renderer, &doc);
                (WorkedCache::new(Arc::clone(&doc)), pic)
            };
            assert!(many.r.rgba_premul == one.r.rgba_premul, "{threads} threads, cache {cached}: the frame is not the one drawn on one thread");
            assert_eq!(many.r.skipped, one.r.skipped, "{threads} threads, cache {cached}");
            assert!(renderer.par_stats().walks_shared > 0, "{threads} threads, cache {cached}: the walk was shared out");
            if cached {
                let worked = cache.evaluated();
                assert!(worked >= 3_000, "{worked} answers kept");
                let again = draw_whole(&mut renderer, &doc);
                assert_eq!(cache.evaluated(), worked, "{threads} threads: the next frame worked nothing out");
                assert!(again.r.rgba_premul == one.r.rgba_premul);
            }
        }
    }
}

#[test]
fn the_frames_of_a_document_work_out_at_most_the_steps_its_cache_has_in_all() {
    // The steps for the cells with no saved value are the document's, and not each frame's: a page that holds more formulas than they reach is worked out as far as they
    // go by the first frame, and the frames after it (a pan, a zoom) find them spent and work nothing more out. A crafted file is paid for once, not once for each frame.
    let doc = Arc::new(themed_by_a_style(3_000));
    let total = 20_000;
    let cache = WorkedCache::with_steps(Arc::clone(&doc), total);
    let mut renderer = Renderer::new();
    renderer.set_worked_cache(Some(Arc::clone(&cache)));
    let first = draw_whole(&mut renderer, &doc);
    let spent = cache.steps_spent();
    assert!(spent > total / 2 && spent <= total, "the first frame spent the document's steps: {spent} of {total}");
    assert_eq!(cache.steps_left(), total - spent);
    assert!(
        first.r.count(SkipKind::UnsavedUnanswered) > 0,
        "the cells past the steps were left to their defaults, and the frame said so: {:?}",
        first.r.skipped
    );
    let kept = cache.answers();
    for frame in 0..4 {
        let again = draw_whole(&mut renderer, &doc);
        assert_eq!(cache.steps_spent(), spent, "frame {frame}: nothing more was worked out");
        assert_eq!(cache.answers(), kept, "frame {frame}: and nothing more was kept");
        assert!(again.r.count(SkipKind::UnsavedUnanswered) > 0);
    }
}

#[test]
fn a_cache_is_for_one_document_by_address_and_not_by_equal_content() {
    let doc = Arc::new(themed_by_a_style(10));
    let cache = WorkedCache::new(Arc::clone(&doc));
    assert!(cache.is_for(&doc));
    let equal = Document::clone(&doc);
    assert_eq!(equal, *doc, "equal content");
    assert!(!cache.is_for(&equal), "another value in memory");
}

#[test]
#[ignore = "a clock, which the counted tests above stand for: run it by hand on a quiet machine"]
fn timed_a_second_frame_of_thirty_thousand_shapes_is_much_faster_than_the_first() {
    let doc = Arc::new(themed_by_a_style(30_000));
    let mut renderer = Renderer::new();
    let started = std::time::Instant::now();
    let (_cache, _first) = drawn_with_a_cache(&mut renderer, &doc);
    let cold = started.elapsed();
    let started = std::time::Instant::now();
    let _second = draw_whole(&mut renderer, &doc);
    let warm = started.elapsed();
    eprintln!("cold {cold:?}, warm {warm:?}");
    assert!(warm < cold, "the warm frame ({warm:?}) is quicker than the cold one ({cold:?})");
}
