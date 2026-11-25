//! The canvas's backing store and the device pixel ratio. eframe sizes the canvas from `ResizeObserverEntry.devicePixelContentBoxSize` and takes the window's size in points to be that
//! many pixels over `devicePixelRatio`. A browser that reports the box in CSS pixels where the ratio is more than 1 (Chrome's device emulation does: DevTools' device mode, Lighthouse's mobile runs and the
//! test tools' `deviceScaleFactor`) gives a canvas that is 1 / ratio of the width in pixels, and so a window laid out for 1 / ratio of the width of the page. [`diacraft_browser::canvas_size::correction`]
//! says what the pixels of the canvas must be when they are not what the page's size and the ratio say; `web_canvas.rs` (compiled by `cargo xtask wasm`) applies it before every frame.

use diacraft_browser::canvas_size::{MAX_SIDE, TOLERANCE, correction, wanted};

use crate::read;

/// The case that was reported: a 1200 by 700 page at a device scale factor of 2, 1.5 and 3, where the browser reported the canvas as 1200 by 700 pixels (its CSS size, not its size in device pixels).
#[test]
fn a_canvas_the_size_of_the_page_in_css_pixels_is_corrected_at_every_ratio_above_one() {
    for (ratio, want) in [(1.5, (1800, 1050)), (2.0, (2400, 1400)), (3.0, (3600, 2100)), (1.25, (1500, 875)), (1.75, (2100, 1225))] {
        assert_eq!(correction((1200, 700), (1200.0, 700.0), ratio), Some(want), "at a ratio of {ratio}");
    }
}

/// The phone-size page of the report: 390 by 844 at a ratio of 3 was laid out as 130 by 281 points.
#[test]
fn a_phone_sized_page_at_a_ratio_of_three_is_corrected() {
    assert_eq!(correction((390, 844), (390.0, 844.0), 3.0), Some((1170, 2532)));
}

/// At a ratio of 1 the box in device pixels and in CSS pixels are one, so there is nothing to correct.
#[test]
fn nothing_is_corrected_at_a_ratio_of_one() {
    assert_eq!(correction((1200, 700), (1200.0, 700.0), 1.0), None);
}

/// A browser that reports the box right is left alone, at each ratio: the sizes a real Chrome reported with a real device scale factor of 1.5, 2 and 3 (measured with `--force-device-scale-factor`).
#[test]
fn a_canvas_the_browser_sized_right_is_left_alone() {
    assert_eq!(correction((1800, 920), (1200.0, 613.328125), 1.5), None);
    assert_eq!(correction((2400, 1226), (1200.0, 613.0), 2.0), None);
    assert_eq!(correction((3600, 1839), (1200.0, 613.0), 3.0), None);
    assert_eq!(correction((1170, 2532), (390.0, 844.0), 3.0), None);
}

/// The box is rounded to whole device pixels by the browser, and the CSS size is fractional (browser zoom): a few pixels of difference are the browser's rounding, and the canvas is not touched
/// (setting a canvas's size clears it, so a correction that fired on a rounding would blank the window every frame).
#[test]
fn a_difference_of_a_few_pixels_is_the_browsers_rounding() {
    assert_eq!(correction((1801, 920), (1200.0, 613.328125), 1.5), None);
    assert_eq!(correction((1799, 919), (1200.0, 613.328125), 1.5), None);
    assert_eq!(correction((1802, 922), (1200.0, 613.328125), 1.5), None);
    // The most that is let go, and one pixel more.
    assert_eq!(TOLERANCE, 3);
    assert_eq!(correction((1803, 923), (1200.0, 613.328125), 1.5), None);
    assert_eq!(correction((1797, 917), (1200.0, 613.328125), 1.5), None);
    assert_eq!(correction((1804, 920), (1200.0, 613.328125), 1.5), Some((1800, 920)));
    assert_eq!(correction((1800, 924), (1200.0, 613.328125), 1.5), Some((1800, 920)));
    assert_eq!(correction((1796, 920), (1200.0, 613.328125), 1.5), Some((1800, 920)));
    // 110% zoom: 1920 device pixels are 1745.4545 CSS pixels.
    assert_eq!(correction((1920, 1080), (1745.4545, 981.8181), 1.1), None);
}

/// A resize that the observer has not delivered yet is a real difference, and is corrected: the page's size is what the next frame is drawn for.
#[test]
fn a_canvas_that_has_not_followed_the_page_yet_is_corrected() {
    assert_eq!(correction((1200, 700), (1000.0, 700.0), 1.0), Some((1000, 700)));
    assert_eq!(correction((2400, 1400), (1000.0, 700.0), 2.0), Some((2000, 1400)));
}

/// A canvas that is too wide or too tall alone is corrected on that side only in value, but both sides are given.
#[test]
fn one_wrong_side_gives_both_sides() {
    assert_eq!(correction((2400, 700), (1200.0, 700.0), 2.0), Some((2400, 1400)));
    assert_eq!(correction((1200, 1400), (1200.0, 700.0), 2.0), Some((2400, 1400)));
}

/// What the browser says is not trusted: a size that is not a size, a ratio that is not a ratio, and a canvas with no room (hidden) are left alone.
#[test]
fn what_is_not_a_size_is_left_alone() {
    for (css, ratio) in [
        ((f64::NAN, 700.0), 2.0),
        ((1200.0, f64::NAN), 2.0),
        ((f64::INFINITY, 700.0), 2.0),
        ((1200.0, f64::NEG_INFINITY), 2.0),
        ((0.0, 700.0), 2.0),
        ((1200.0, 0.0), 2.0),
        ((-5.0, 700.0), 2.0),
        ((1200.0, 700.0), f64::NAN),
        ((1200.0, 700.0), f64::INFINITY),
        ((1200.0, 700.0), 0.0),
        ((1200.0, 700.0), -2.0),
    ] {
        assert_eq!(correction((1200, 700), css, ratio), None, "css {css:?}, ratio {ratio}");
        assert_eq!(wanted(css, ratio), None, "css {css:?}, ratio {ratio}");
    }
}

/// A size past what any graphics surface takes is held to the limit, and a canvas at the limit is left alone.
#[test]
fn a_huge_size_is_held_to_the_limit() {
    assert_eq!(wanted((1.0e9, 700.0), 2.0), Some((MAX_SIDE, 1400)));
    assert_eq!(wanted((1200.0, 1.0e12), 3.0), Some((3600, MAX_SIDE)));
    assert_eq!(correction((MAX_SIDE, 1400), (1.0e9, 700.0), 2.0), None);
}

/// A canvas is never asked to be less than one pixel on a side.
#[test]
fn a_tiny_size_is_one_pixel() {
    assert_eq!(wanted((0.2, 0.2), 1.0), Some((1, 1)));
}

/// The shell applies it: `web.rs` installs the guard that calls [`correction`] before every frame, and the guard is the one place that writes the canvas's size.
#[test]
fn the_shell_installs_the_guard() {
    let web = read("src/web.rs");
    assert!(web.contains("crate::web_canvas::install(&cc.egui_ctx);"), "web.rs does not install the canvas guard");
    let guard = read("src/web_canvas.rs");
    assert!(guard.contains("canvas_size::correction("), "web_canvas.rs does not ask for a correction");
    assert!(guard.contains("fn input_hook"), "the guard is not run before each frame's input is used");
    assert!(guard.contains("set_width(") && guard.contains("set_height("), "the guard does not set the canvas's size");
    let lib = read("src/lib.rs");
    assert!(lib.contains("pub mod canvas_size;") && lib.contains("mod web_canvas;"), "lib.rs does not name the two modules");
}
