//! Test only (`#[cfg(test)]`, never compiled into the library): panics put into a frame's drawing at chosen places, to show that a shape
//! whose drawing panics costs that shape and not the page.
//!
//! A test arms a fault for one shape's `ID` ([`arm`]), draws a page with `Renderer::render`, and reads the result. The drawing calls
//! [`strike`] at four places (placing a shape, drawing it, inside a shadow's blur layer, and the start of a page), and the armed fault
//! panics there, holding one of the frame's `RefCell`s open if the test said so, as the bug of did.

use std::cell::RefCell;

use crate::read::Cx;

/// Where in the drawing a fault strikes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum At {
    /// The start of a page's walk, outside any shape: the whole frame's net catches it.
    Page,
    /// Placing a shape (`Frame::enter`).
    Enter,
    /// Drawing a shape's own geometry and text (`Frame::draw_shape`).
    Draw,
    /// Inside the layer a blurred shadow is drawn in.
    InLayer,
}

/// Which cell of the frame's reading context the panicking code holds open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Holding {
    Nothing,
    Skips,
    Themed,
    Eval,
}

thread_local! {
    static ARMED: RefCell<Vec<(u32, At, Holding)>> = const { RefCell::new(Vec::new()) };
}

/// Arms a fault for shape `shape` (0 for [`At::Page`]). It strikes every time the drawing passes that place, on this thread.
pub(crate) fn arm(shape: u32, at: At, holding: Holding) {
    ARMED.with(|armed| armed.borrow_mut().push((shape, at, holding)));
}

/// The band a test has armed to panic, as the frame's width and the band's first row (the pair is the test's own: no other frame matches it).
static BAND_FAULT: std::sync::Mutex<Option<(u16, u16)>> = std::sync::Mutex::new(None);

/// Arms a panic in the band of a frame `width` pixels wide that starts at row `first`, on whatever thread draws it (a band is drawn on a
/// thread of its own, which the thread-local list above cannot reach).
pub(crate) fn arm_band(width: u16, first: u16) {
    *BAND_FAULT.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some((width, first));
}

/// The share a test has armed to panic, as the frame's width and the share's first top shape.
static SHARE_FAULT: std::sync::Mutex<Option<(u16, usize)>> = std::sync::Mutex::new(None);

/// Arms a panic in the share of the walk of a frame `width` pixels wide that starts at top shape `first`.
pub(crate) fn arm_share(width: u16, first: usize) {
    *SHARE_FAULT.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some((width, first));
}

/// Takes the share's fault away.
pub(crate) fn disarm_share() {
    *SHARE_FAULT.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = None;
}

/// Panics if a fault is armed for this share.
#[allow(clippy::panic)]
pub(crate) fn strike_share(width: u16, first: usize) {
    let hit = *SHARE_FAULT.lock().unwrap_or_else(std::sync::PoisonError::into_inner) == Some((width, first));
    if hit {
        panic!("a test panic in a share of the walk");
    }
}

/// Takes the band's fault away.
pub(crate) fn disarm_band() {
    *BAND_FAULT.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = None;
}

/// Panics if a fault is armed for this band.
#[allow(clippy::panic)]
pub(crate) fn strike_band(width: u16, first: u16) {
    let hit = *BAND_FAULT.lock().unwrap_or_else(std::sync::PoisonError::into_inner) == Some((width, first));
    if hit {
        panic!("a test panic in a band of the frame");
    }
}

/// Takes every fault away.
#[cfg(test)]
pub(crate) fn disarm() {
    ARMED.with(|armed| armed.borrow_mut().clear());
}

/// Panics if a fault is armed for this shape at this place.
pub(crate) fn strike(cx: &Cx<'_, '_>, shape: u32, at: At) {
    let hit = ARMED.with(|armed| armed.borrow().iter().find(|(s, a, _)| *s == shape && *a == at).map(|(_, _, holding)| *holding));
    if let Some(holding) = hit {
        cx.panic_holding(holding);
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use diacraft_color::Rgba;
    use diacraft_doc::{Document, RowKey};
    use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder, StyleBuilder};
    use kurbo::{Affine, BezPath, Rect, Shape as _};

    use super::{At, Holding, arm, arm_band, arm_share, disarm, disarm_band, disarm_share};
    use crate::canvas::Canvas;
    use crate::target::{Paint, Target};
    use crate::{Parallel, Rendered, Renderer, SkipKind, View};

    const PPI: f64 = 50.0;

    /// A 4 by 4 inch page of squares side by side along y = 1 to 2: 1 red, 2 blue, 3 green, the middle one the victim. Each is on a theme
    /// and holds a themed cell no theme answers (so each reads the evaluator, the kept answers and the list of notes). `change` is applied
    /// to every square, with its `ID`.
    fn page_with(change: impl Fn(u32, ShapeBuilder) -> ShapeBuilder) -> Document {
        let mut page = PageBuilder::new().cell("PageWidth", "4").cell("PageHeight", "4").cell("PageScale", "1").cell("DrawingScale", "1");
        for (id, x, rgb) in [(1, 0.5, "#FF0000"), (2, 1.75, "#0000FF"), (3, 3.0, "#00FF00")] {
            let mut sb = ShapeBuilder::new(id)
                .cell("PinX", &x.to_string())
                .cell("PinY", "1")
                .cell("Width", "1")
                .cell("Height", "1")
                .cell("LocPinX", "0")
                .cell("LocPinY", "0")
                .cell("Angle", "0")
                .cell("FillForegnd", rgb)
                .cell("LinePattern", "0")
                .formula("FillBkgnd", "THEMEVAL()", "Themed");
            for name in ["ThemeIndex", "ColorSchemeIndex", "EffectSchemeIndex", "ConnectorSchemeIndex", "FontSchemeIndex"] {
                sb = sb.cell(name, "1");
            }
            let outline = [("MoveTo", 0.0, 0.0), ("LineTo", 1.0, 0.0), ("LineTo", 1.0, 1.0), ("LineTo", 0.0, 1.0), ("LineTo", 0.0, 0.0)];
            for (i, (kind, px, py)) in outline.into_iter().enumerate() {
                let key = RowKey::Ix(i as u32);
                sb = sb.row_type("Geometry", Some(0), key.clone(), kind).row_cell("Geometry", Some(0), key.clone(), "X", &px.to_string());
                sb = sb.row_cell("Geometry", Some(0), key, "Y", &py.to_string());
            }
            page = page.shape(change(id, sb));
        }
        let root = |r: StyleBuilder| {
            r.cell("FillPattern", "1").cell("FillForegnd", "1").cell("FillBkgnd", "0").cell("LinePattern", "1").cell("ShdwPattern", "0")
        };
        DocBuilder::new().root(root).page(1, page).build()
    }

    fn page_of_three() -> Document {
        page_with(|_, sb| sb)
    }

    /// The same three squares with no themed cell: the shares of a walk can take it (an unanswered theme sends the walk back to one thread).
    fn plain_page_of_three() -> Document {
        let mut page = PageBuilder::new().cell("PageWidth", "4").cell("PageHeight", "4").cell("PageScale", "1").cell("DrawingScale", "1");
        for (id, x, rgb) in [(1, 0.5, "#FF0000"), (2, 1.75, "#0000FF"), (3, 3.0, "#00FF00")] {
            let mut sb = ShapeBuilder::new(id)
                .cell("PinX", &x.to_string())
                .cell("PinY", "1")
                .cell("Width", "1")
                .cell("Height", "1")
                .cell("LocPinX", "0")
                .cell("LocPinY", "0")
                .cell("Angle", "0")
                .cell("FillForegnd", rgb)
                .cell("FillPattern", "1")
                .cell("LinePattern", "0");
            let outline = [("MoveTo", 0.0, 0.0), ("LineTo", 1.0, 0.0), ("LineTo", 1.0, 1.0), ("LineTo", 0.0, 1.0), ("LineTo", 0.0, 0.0)];
            for (i, (kind, px, py)) in outline.into_iter().enumerate() {
                let key = RowKey::Ix(i as u32);
                sb = sb.row_type("Geometry", Some(0), key.clone(), kind).row_cell("Geometry", Some(0), key.clone(), "X", &px.to_string());
                sb = sb.row_cell("Geometry", Some(0), key, "Y", &py.to_string());
            }
            page = page.shape(sb);
        }
        DocBuilder::new()
            .root(|r: StyleBuilder| r.cell("FillPattern", "1").cell("FillForegnd", "1").cell("LinePattern", "1").cell("ShdwPattern", "0"))
            .page(1, page)
            .build()
    }

    /// Draws page 1 and gives the result and a reader of its pixels at a point in paper inches, y up.
    fn draw(renderer: &mut Renderer, doc: &Document) -> (Rendered, impl Fn(f64, f64) -> [u8; 4] + use<>) {
        let view = View::fit_page(doc, 1, PPI).unwrap();
        let r = renderer.render(doc, 1, &view);
        let (w, h) = (r.width as usize, r.height as usize);
        let pixels = r.rgba_premul.clone();
        let at = move |x: f64, y: f64| {
            let (px, py) = ((x * PPI) as usize, ((h as f64 / PPI - y) * PPI) as usize);
            let i = (py.min(h - 1) * w + px.min(w - 1)) * 4;
            [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
        };
        (r, at)
    }

    fn is(p: [u8; 4], rgb: (u8, u8, u8)) -> bool {
        p[3] > 250 && p[0].abs_diff(rgb.0) < 8 && p[1].abs_diff(rgb.1) < 8 && p[2].abs_diff(rgb.2) < 8
    }

    /// Every way the panicking code may hold a cell of the frame open when it panics.
    const HOLDINGS: [Holding; 4] = [Holding::Nothing, Holding::Skips, Holding::Themed, Holding::Eval];

    #[test]
    fn a_panic_in_one_shapes_drawing_loses_that_shape_and_the_others_draw() {
        for holding in HOLDINGS {
            disarm();
            let mut renderer = Renderer::new();
            arm(2, At::Draw, holding);
            let (r, at) = draw(&mut renderer, &page_of_three());
            assert!(is(at(1.0, 1.5), (255, 0, 0)), "{holding:?}: the shape before it is drawn: {:?}", at(1.0, 1.5));
            assert!(
                is(at(3.5, 1.5), (0, 255, 0)),
                "{holding:?}: the shape after it is drawn, after a guard was held through the panic: {:?}",
                at(3.5, 1.5)
            );
            assert!(!is(at(2.25, 1.5), (0, 0, 255)), "{holding:?}: the shape that panicked is not drawn");
            assert_eq!(r.count(SkipKind::ShapeFailed), 1, "{holding:?}: {:?}", r.skipped);
            assert_eq!(r.count(SkipKind::Recovered), 0, "{holding:?}: the frame is not lost: {:?}", r.skipped);
            let failed = r.skipped.iter().find(|s| s.kind == SkipKind::ShapeFailed).unwrap();
            assert!(failed.first.starts_with("shape 2: a test panic"), "{holding:?}: {}", failed.first);
            // Shapes 1 and 3 each asked the evaluator for a themed cell, one before the unwind and one after.
            assert_eq!(r.count(SkipKind::ThemeUnanswered), 2, "{holding:?}: {:?}", r.skipped);
        }
        disarm();
    }

    #[test]
    fn a_panic_in_placing_a_shape_loses_that_shape_and_the_others_draw() {
        disarm();
        let mut renderer = Renderer::new();
        arm(2, At::Enter, Holding::Skips);
        let (r, at) = draw(&mut renderer, &page_of_three());
        assert!(is(at(1.0, 1.5), (255, 0, 0)) && is(at(3.5, 1.5), (0, 255, 0)), "the shapes beside it are drawn");
        assert!(!is(at(2.25, 1.5), (0, 0, 255)));
        assert_eq!(r.count(SkipKind::ShapeFailed), 1, "{:?}", r.skipped);
        disarm();
    }

    #[test]
    fn two_shapes_that_panic_are_both_counted_and_the_first_is_the_sample() {
        disarm();
        let mut renderer = Renderer::new();
        arm(3, At::Draw, Holding::Nothing);
        arm(1, At::Draw, Holding::Eval);
        let (r, at) = draw(&mut renderer, &page_of_three());
        assert!(is(at(2.25, 1.5), (0, 0, 255)), "the one that did not panic is drawn");
        assert_eq!(r.count(SkipKind::ShapeFailed), 2, "{:?}", r.skipped);
        let failed = r.skipped.iter().find(|s| s.kind == SkipKind::ShapeFailed).unwrap();
        assert!(failed.first.starts_with("shape 1:"), "the first one met: {}", failed.first);
        disarm();
    }

    #[test]
    fn a_panic_inside_a_blur_layer_does_not_blur_or_clip_what_is_drawn_after() {
        disarm();
        // Shape 2 has a blurred shadow, and panics inside the layer it is drawn in. If the layer stayed open, shape 3 would be drawn into it.
        let doc = page_with(|id, sb| {
            if id != 2 {
                return sb;
            }
            sb.cell("ShdwPattern", "1")
                .cell("ShapeShdwType", "1")
                .cell("ShapeShdwOffsetX", "0.1")
                .cell("ShapeShdwOffsetY", "-0.1")
                .cell("ShapeShdwBlur", "0.15")
                .cell("ShdwForegnd", "#808080")
        });
        let mut renderer = Renderer::new();
        arm(2, At::InLayer, Holding::Skips);
        let (r, at) = draw(&mut renderer, &doc);
        assert_eq!(r.count(SkipKind::ShapeFailed), 1, "{:?}", r.skipped);
        assert!(is(at(1.0, 1.5), (255, 0, 0)), "before: {:?}", at(1.0, 1.5));
        assert!(is(at(3.5, 1.5), (0, 255, 0)), "after, sharp and whole: {:?}", at(3.5, 1.5));
        assert!(is(at(3.05, 1.05), (0, 255, 0)) && is(at(3.95, 1.95), (0, 255, 0)), "to its corners");
        // The renderer is as good as new: the next frame, with no fault, draws all three.
        disarm();
        let (r, at) = draw(&mut renderer, &doc);
        assert_eq!(r.count(SkipKind::ShapeFailed), 0, "{:?}", r.skipped);
        assert!(is(at(2.25, 1.5), (0, 0, 255)));
    }

    #[test]
    fn a_panic_outside_any_shape_is_caught_by_the_frames_net_as_before() {
        disarm();
        let mut renderer = Renderer::new();
        arm(0, At::Page, Holding::Skips);
        let (r, at) = draw(&mut renderer, &page_of_three());
        assert_eq!(r.count(SkipKind::Recovered), 1, "{:?}", r.skipped);
        assert_eq!(r.count(SkipKind::ShapeFailed), 0);
        assert_eq!(at(1.0, 1.5)[3], 0, "the frame that was lost is clear");
        // The renderer goes on: the same renderer draws the next frame whole.
        disarm();
        let (r, at) = draw(&mut renderer, &page_of_three());
        assert!(r.skipped.iter().all(|s| s.kind == SkipKind::ThemeUnanswered), "{:?}", r.skipped);
        assert!(is(at(1.0, 1.5), (255, 0, 0)) && is(at(2.25, 1.5), (0, 0, 255)) && is(at(3.5, 1.5), (0, 255, 0)));
    }

    #[test]
    fn rewinding_a_canvas_closes_the_clips_and_layers_a_cut_short_drawing_left_open() {
        let mut canvas = Canvas::new(40, 40, Affine::scale(10.0));
        let mark = canvas.mark();
        // A clip to the top left quarter, and another inside it, never popped (the drawing that opened them panicked).
        canvas.push_clip(&Rect::new(0.0, 0.0, 2.0, 2.0).to_path(0.1));
        canvas.push_clip(&Rect::new(0.0, 0.0, 1.0, 1.0).to_path(0.1));
        assert_eq!(canvas.mark(), mark + 2);
        // A blur layer whose drawing panics: the closure's panic leaves the layer open.
        let cut = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| canvas.blurred(0.01, |_| panic!("cut short"))));
        assert!(cut.is_err());
        assert_eq!(canvas.mark(), mark + 3, "two clips and the layer are open");
        canvas.rewind(mark);
        assert_eq!(canvas.mark(), mark);
        let all: BezPath = Rect::new(0.0, 0.0, 4.0, 4.0).to_path(0.1);
        canvas.fill(&all, &Paint::Solid(Rgba::rgb(255, 0, 0)), false);
        let pixels = canvas.finish();
        let px = |x: usize, y: usize| &pixels[(y * 40 + x) * 4..(y * 40 + x) * 4 + 4];
        assert_eq!(px(35, 35), [255, 0, 0, 255], "the far corner is filled: no clip is left in force");
        assert_eq!(px(5, 5), [255, 0, 0, 255]);
    }

    #[test]
    fn a_panics_words_are_kept_whatever_it_carried() {
        use crate::frame::panic_message;
        let caught = |f: fn()| std::panic::catch_unwind(f).unwrap_err();
        assert_eq!(panic_message(caught(|| panic!("a plain str")).as_ref()), "a plain str");
        assert_eq!(panic_message(caught(|| panic!("{}", "formatted".to_owned())).as_ref()), "formatted");
        assert_eq!(panic_message(caught(|| std::panic::panic_any(7_u8)).as_ref()), "a panic with no message");
        let long = "x".repeat(1000);
        assert_eq!(panic_message(std::panic::catch_unwind(|| panic!("{}", long)).unwrap_err().as_ref()).chars().count(), 200);
    }

    /// A view of the page of three that is `width` pixels wide, a width no other test draws, so that a fault armed for it meets only its own test.
    fn odd_view(doc: &Document, width: u32) -> View {
        let mut view = View::fit_page(doc, 1, PPI).unwrap();
        view.width = width;
        view
    }

    #[test]
    fn a_panic_in_one_band_is_an_error_the_frame_is_cut_short_as_by_the_guard_and_the_next_frame_draws() {
        disarm_band();
        let doc = plain_page_of_three();
        let view = odd_view(&doc, 203);
        let mut renderer = Renderer::new();
        renderer.set_parallel(Parallel::eager(4));
        // 200 rows on 4 threads: four bands of 50 rows made 64 (a band has at least 64), so there are four; the band that starts at row 64 is the
        // second from the top.
        assert!(crate::par::split_rows(200, 4).iter().any(|&(first, _)| first == 64), "{:?}", crate::par::split_rows(200, 4));
        arm_band(203, 64);
        let r = renderer.render(&doc, 1, &view);
        disarm_band();
        assert_eq!(r.count(SkipKind::Recovered), 1, "the frame is lost as the guard loses one: {:?}", r.skipped);
        let words = &r.skipped.iter().find(|s| s.kind == SkipKind::Recovered).unwrap().first;
        assert!(words.contains("a band of the frame met a bug") && words.contains("a test panic in a band"), "in words: {words}");
        assert_eq!((r.width, r.height), (203, 200));
        assert!(r.rgba_premul.len() == 203 * 200 * 4 && r.rgba_premul.iter().all(|&b| b == 0), "a blank frame of the right size");
        // The renderer is as good as new: the same view, with no fault, draws all three, the same as one thread draws it.
        let again = renderer.render(&doc, 1, &view);
        assert_eq!(again.count(SkipKind::Recovered), 0, "{:?}", again.skipped);
        assert_eq!(again, Renderer::new().render(&doc, 1, &view));
    }

    #[test]
    fn a_panic_in_a_share_of_the_walk_takes_the_page_back_to_the_walk_in_order_and_the_frame_is_the_same() {
        disarm_share();
        let doc = plain_page_of_three();
        let view = odd_view(&doc, 205);
        let mut renderer = Renderer::new();
        renderer.set_parallel(Parallel::eager(4));
        // Three shares of one top shape each; the second panics.
        arm_share(205, 1);
        let r = renderer.render(&doc, 1, &view);
        disarm_share();
        assert_eq!(r.count(SkipKind::Recovered), 0, "the walk in order does not meet the fault: {:?}", r.skipped);
        assert_eq!(r, Renderer::new().render(&doc, 1, &view), "the frame is the one thread's");
        let stats = renderer.par_stats();
        assert_eq!((stats.walks_shared, stats.walks_in_order), (0, 1), "the page was walked in order after the share failed");
    }
}
