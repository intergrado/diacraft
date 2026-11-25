//! The size the canvas's backing store must have, decided without a browser.
//!
//! **What went wrong.** The web runner sizes the canvas from the `devicePixelContentBoxSize` of the entry its `ResizeObserver` is given, which is the size of the canvas in device pixels,
//! and takes the size of the window in points to be the canvas's width in pixels over `window.devicePixelRatio`. A browser that reports that box in CSS pixels where the ratio is more than 1
//! breaks the second step: Chrome's device emulation does (`Emulation.setDeviceMetricsOverride` with a `deviceScaleFactor`: DevTools' device mode, Lighthouse's mobile runs, and the
//! test tools that set a device scale factor). A 1200 by 700 page at a ratio of 2 got a canvas of 1200 by 700 pixels, and so a window of 600 by 350 points, drawn at twice the size it should be. A real Chrome with a real scale factor of 1.5, 2 or 3 reports the box right (measured with
//! `--force-device-scale-factor`), and so does Firefox 160 (with `layout.css.devPixelsPerPx`), so what people see on a hi-DPI screen was never affected; what the emulation shows is, and a person who tries the app on a phone in a desktop browser's device mode would see it broken.
//!
//! **What this holds.** The canvas is laid out to the size of the page (`page.css`), so its size in CSS pixels times the ratio is its size in device pixels, whatever the browser's box says.
//! [`correction`] compares the two and, when they differ by more than the browser's own rounding ([`TOLERANCE`]), says what the pixels of the canvas must be. `web_canvas.rs` applies it
//! before every frame, so a browser that reports the box right never meets it, and one that does not gets the right canvas on the frame it was wrong.

/// The most pixels the canvas is given on a side: past what any graphics surface takes, so that a size the browser makes up (a page of a million CSS pixels) cannot ask for more than a surface can
/// have. The largest surface of the common GPUs is 16,384 pixels.
pub const MAX_SIDE: u32 = 16_384;

/// The most a side's pixels may differ from the page's size times the ratio before the canvas is thought wrong: the browser rounds its box to whole device pixels (one pixel), the page's size is
/// fractional under browser zoom (a pixel more when it is rounded again), and a margin besides. A canvas that is more than this out is not the browser's rounding.
pub const TOLERANCE: u32 = 3;

/// The pixels the canvas must have for a page of `css` size (width and height, in CSS pixels) at `pixel_ratio` device pixels to the CSS pixel: each side rounded to a whole pixel, at least 1 and at
/// most [`MAX_SIDE`]. `None` for a size that is not one (not a number, not finite, no room) or a ratio that is not one, which are left to the browser.
pub fn wanted(css: (f64, f64), pixel_ratio: f64) -> Option<(u32, u32)> {
    let side = |css: f64| -> Option<u32> {
        let pixels = (css * pixel_ratio).round();
        if !css.is_finite() || css <= 0.0 || !pixel_ratio.is_finite() || pixel_ratio <= 0.0 || !pixels.is_finite() {
            return None;
        }
        // Clamped before the cast, so that a value past a `u32` is the limit and not a wrapped number.
        Some(pixels.clamp(1.0, f64::from(MAX_SIDE)) as u32)
    };
    Some((side(css.0)?, side(css.1)?))
}

/// What the canvas's pixels must be set to, when the canvas has `have` pixels and the page's size is `css` at `pixel_ratio`: `Some` when either side is more than [`TOLERANCE`] pixels from
/// [`wanted`], and then both sides, as [`wanted`] gives them; `None` when the canvas is right, or when nothing can be said ([`wanted`] has no answer).
pub fn correction(have: (u32, u32), css: (f64, f64), pixel_ratio: f64) -> Option<(u32, u32)> {
    let want = wanted(css, pixel_ratio)?;
    let out = |have: u32, want: u32| have.abs_diff(want) > TOLERANCE;
    (out(have.0, want.0) || out(have.1, want.1)).then_some(want)
}
