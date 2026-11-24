//! Shadows.

use diacraft_render::SkipKind;

use crate::support::*;

fn shadowed(angle: f64) -> diacraft_testkit::build::ShapeBuilder {
    rect(1, 1.0, 1.5, 1.0, 1.0)
        .cell("FillForegnd", "#FF0000")
        .cell("LinePattern", "0")
        .cell("ShdwPattern", "1")
        .cell("ShapeShdwType", "1")
        .cell("ShapeShdwOffsetX", "0.3")
        .cell("ShapeShdwOffsetY", "-0.3")
        .cell("ShdwForegnd", "#808080")
        .cell("PinX", "1.5")
        .cell("PinY", "2.0")
        .cell("LocPinX", "0.5")
        .cell("LocPinY", "0.5")
        .cell("Angle", &angle.to_string())
}

#[test]
fn a_shadow_is_behind_its_shape_offset_right_and_down_by_its_cells() {
    // The shape fills [1, 2] x [1.5, 2.5]; offset (0.3, -0.3) puts the shadow at [1.3, 2.3] x [1.2, 2.2].
    let pic = draw(&one_page(vec![shadowed(0.0)]));
    assert!(pic.is_red(1.5, 2.0), "the shape is in front of its shadow");
    assert!(pic.is(2.15, 1.35, (128, 128, 128), 6), "the shadow shows past the shape's lower right");
    assert!(pic.is_white(1.1, 1.25), "and not where it is not: left of 1.3");
    assert!(pic.is_white(2.4, 1.35), "or right of 2.3");
}

#[test]
fn a_shadow_offset_stays_along_the_pages_axes_when_the_shape_is_turned() {
    // /: a shape turned a quarter turn, with offsets 0.3 right and 0.3 down. The offset does not turn with it:
    // turned with the shape it would be 0.3 right and 0.3 UP.
    let pic = draw(&one_page(vec![shadowed(std::f64::consts::FRAC_PI_2)]));
    // The turned square, 1 by 1 about its pin at (1.5, 2.0), is [1, 2] x [1.5, 2.5] again; the shadow is 0.3 right and 0.3 down.
    assert!(pic.is(2.15, 1.35, (128, 128, 128), 6), "shadow lower right of the shape");
    assert!(pic.is_white(2.15, 2.65), "not upper right");
}

#[test]
fn a_shape_with_shadow_type_zero_takes_the_pages_default() {
    // The page's five cells stand in. A page type of 0 draws a simple shadow.
    let sb = shadowed(0.0).cell("ShapeShdwType", "0").cell("ShapeShdwOffsetX", "0").cell("ShapeShdwOffsetY", "0");
    let p = page(4.0, 4.0)
        .cell("ShdwType", "0")
        .cell("ShdwOffsetX", "0.2")
        .cell("ShdwOffsetY", "-0.2")
        .cell("ShdwScaleFactor", "1")
        .cell("ShdwObliqueAngle", "0")
        .shape(sb);
    let pic = draw(&base().page(1, p).build());
    assert!(pic.is(2.1, 1.35, (128, 128, 128), 6), "{:?}", pic.at(2.1, 1.35));
}

#[test]
fn a_shadow_scale_is_about_the_pin() {
    // A box (0, 0) to (2, 1) with its pin at (1, 0.5), lifted by 2 inches, a factor of 1.5 and offsets 0.1 and -0.1. The shadow is
    // 3 by 1.5 about the pin, then moved: x in [-0.4, 2.6], y in [1.65, 3.15]. Scaled about the shape's corner it would not reach below y = 2.
    let sb = rect(1, 0.0, 0.0, 2.0, 1.0)
        .cell("FillForegnd", "#FF0000")
        .cell("LinePattern", "0")
        .cell("ShdwPattern", "1")
        .cell("ShapeShdwType", "1")
        .cell("ShapeShdwOffsetX", "0.1")
        .cell("ShapeShdwOffsetY", "-0.1")
        .cell("ShapeShdwScaleFactor", "1.5")
        .cell("ShdwForegnd", "#808080")
        .cell("PinX", "1.0")
        .cell("PinY", "2.5")
        .cell("LocPinX", "1.0")
        .cell("LocPinY", "0.5");
    let pic = draw(&one_page(vec![sb]));
    assert!(pic.is_red(1.0, 2.5), "the shape is in front of its shadow");
    assert!(pic.is(1.0, 1.8, (128, 128, 128), 6), "the shadow reaches below the shape: {:?}", pic.at(1.0, 1.8));
    assert!(pic.is_white(1.0, 1.55), "and no further down than 1.65");
    assert!(pic.is(2.4, 2.5, (128, 128, 128), 6), "and past its right side");
}

#[test]
fn a_blurred_shadow_fades_across_its_edge() {
    // The blur cell is 1.5 standard deviations. Shadow edge at x = 2.3 for 0.15 inch of blur (a deviation of 0.1):
    // about half the shadow's darkness at the edge, nearly all of it 0.2 inside, and almost none 0.3 outside. A hard edge is full darkness up
    // to 2.3 and none past it.
    let pic = draw(&one_page(vec![shadowed(0.0).cell("ShapeShdwBlur", "0.15")]));
    let grey = |x: f64| i32::from(pic.at(x, 1.7)[0]);
    let (inside, edge, outside, far) = (grey(2.12), grey(2.3), grey(2.45), grey(2.8));
    assert!(inside < edge && edge < outside && outside <= far, "a profile that lightens outward: {inside} {edge} {outside} {far}");
    assert!(inside < 160 && far > 240, "dark inside and clear far outside: {inside} {far}");
    assert!((130..=215).contains(&edge), "about half way at the edge: {edge}");
}

/// Page 1 of `doc` at `ppi` pixels to the inch, the whole page.
fn draw_at(doc: &diacraft_doc::Document, ppi: f64) -> Pic {
    let view = diacraft_render::View::fit_page(doc, 1, ppi).expect("the page has a size");
    let r = diacraft_render::Renderer::new().render(doc, 1, &view);
    Pic { r, ppi, page_h: f64::from(view.height) / ppi }
}

#[test]
fn a_blur_reaches_the_same_paper_inches_at_every_pixel_density() {
    // The rasteriser takes a blur's deviation in the user units of the transform in force when the layer is pushed, and ours is the paper inch,
    // so the same shadow blurs over the same stretch of paper at 100 and at 300 pixels to the inch. A deviation read as pixels would be a hard
    // edge at both (a tenth of a pixel), and one that was never scaled would be the same at both. So each density is held to the profile of
    // `a_blurred_shadow_fades_across_its_edge` (0.1 inch of deviation, a cell of 0.15: about half the darkness at the edge), and the two to each other.
    let doc = one_page(vec![shadowed(0.0).cell("ShapeShdwBlur", "0.15")]);
    let (coarse, fine) = (draw_at(&doc, 100.0), draw_at(&doc, 300.0));
    for (pic, ppi) in [(&coarse, 100), (&fine, 300)] {
        let grey = |x: f64| i32::from(pic.at(x, 1.7)[0]);
        let (inside, edge, outside, far) = (grey(2.12), grey(2.3), grey(2.45), grey(2.8));
        assert!(
            inside < edge && edge < outside && outside <= far,
            "a profile that lightens outward at {ppi} pixels to the inch: {inside} {edge} {outside} {far}"
        );
        assert!(inside < 160 && far > 240, "dark inside and clear far outside at {ppi} pixels to the inch: {inside} {far}");
        assert!((130..=215).contains(&edge), "about half way at the edge at {ppi} pixels to the inch: {edge}");
    }
    for x in [2.0, 2.12, 2.3, 2.45, 2.6, 2.8] {
        let (a, b) = (i32::from(coarse.at(x, 1.7)[0]), i32::from(fine.at(x, 1.7)[0]));
        assert!((a - b).abs() <= 8, "the shadow at x = {x} inch is {a} at 100 pixels to the inch and {b} at 300");
    }
}

#[test]
fn a_shape_with_a_blurred_shadow_is_still_in_front_of_it() {
    let pic = draw(&one_page(vec![shadowed(0.0).cell("ShapeShdwBlur", "0.15")]));
    assert!(pic.is_red(1.5, 2.0));
}

/// A shape of `size` inches at `(x, y)` with a grey shadow 0.3 right and 0.3 down, blurred by this cell text.
fn soft(id: u32, x: f64, y: f64, size: f64, blur: &str) -> diacraft_testkit::build::ShapeBuilder {
    rect(id, x, y, size, size)
        .cell("FillForegnd", "#FF0000")
        .cell("LinePattern", "0")
        .cell("ShdwPattern", "1")
        .cell("ShapeShdwType", "1")
        .cell("ShapeShdwOffsetX", "0.3")
        .cell("ShapeShdwOffsetY", "-0.3")
        .cell("ShdwForegnd", "#808080")
        .cell("ShapeShdwBlur", blur)
}

/// The most the greatest difference between two frames' bytes.
fn most_different(a: &Pic, b: &Pic) -> u8 {
    assert_eq!(a.r.rgba_premul.len(), b.r.rgba_premul.len());
    a.r.rgba_premul.iter().zip(&b.r.rgba_premul).map(|(x, y)| x.abs_diff(*y)).max().unwrap_or(0)
}

#[test]
fn a_blur_wider_than_the_cap_is_drawn_at_the_cap_and_noted() {
    // A blur's reach is three deviations, and past MAX_BLUR_SPREAD_PX pixels it costs more than the rasteriser holds. At 100 pixels to
    // the inch the cap is a cell of 1.5 * 320 / 300 = 1.6 inches. A cell of 30 draws as 1.6, says so, and still draws a shadow.
    let at_cap = 1.5 * diacraft_render::limits::MAX_BLUR_SPREAD_PX / (3.0 * PPI);
    let draw_blur = |blur: f64| draw(&one_page(vec![soft(1, 1.0, 1.5, 1.0, &blur.to_string())]));
    let (cut, edge, within) = (draw_blur(30.0), draw_blur(at_cap * 0.999), draw_blur(0.15));
    assert_eq!(cut.r.count(SkipKind::ShadowApproximate), 1, "{:?}", cut.r.skipped);
    assert_eq!(within.r.count(SkipKind::ShadowApproximate), 0, "a blur of a real drawing is not cut: {:?}", within.r.skipped);
    assert!(most_different(&cut, &edge) <= 3, "the cut blur is drawn as the blur at the cap, not as no blur and not as a wider one");
    assert!(!cut.is_white(2.15, 1.35), "the shadow is still drawn: {:?}", cut.at(2.15, 1.35));
    assert!(cut.is_red(1.5, 2.0), "and the shape is in front of it");
}

#[test]
fn an_infinite_blur_is_cut_like_any_other_and_still_draws_a_shadow() {
    // `1.#INF` is the text of an infinity, which the cap treats as a cell larger than the cap.
    let draw_blur = |blur: &str| draw(&one_page(vec![soft(1, 1.0, 1.5, 1.0, blur)]));
    let (infinite, huge) = (draw_blur("1.#INF"), draw_blur("1000"));
    assert_eq!(infinite.r.count(SkipKind::ShadowApproximate), 1, "{:?}", infinite.r.skipped);
    assert_eq!(infinite.r.rgba_premul, huge.r.rgba_premul);
    assert!(!infinite.is_white(2.15, 1.35), "{:?}", infinite.at(2.15, 1.35));
}

#[test]
fn shadows_past_the_frames_pixel_budget_are_hard_edged_and_noted() {
    // 90 shapes of 3 inches, each with a blur of 1 inch (a spread of 200 pixels): a layer of about half a million pixels, of which the frame
    // pays for MAX_BLUR_PIXELS_PER_FRAME / 513,000 = 65. The rest are drawn with a hard edge, and the frame says so.
    let shapes: Vec<_> = (1..=90).map(|i| soft(i, 0.2, 0.9, 3.0, "1")).collect();
    let started = std::time::Instant::now();
    let pic = draw(&one_page(shapes));
    let took = started.elapsed();
    let hard = pic.r.count(SkipKind::ShadowApproximate);
    assert!((10..=60).contains(&hard), "some of the 90 are hard-edged, not none and not all: {hard}, {:?}", pic.r.skipped);
    assert!(pic.is(3.3, 1.5, (128, 128, 128), 6), "a hard-edged shadow is still drawn, at its full colour: {:?}", pic.at(3.3, 1.5));
    // A hang guard and no measure of cost: the frame's pixel budget is what bounds the work (the count of hard-edged shadows above).
    assert!(took.as_secs() < 60, "{took:?}");
}
