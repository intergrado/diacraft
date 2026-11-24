//! The view of a page: how far it is zoomed and which point of it is in the middle of the drawing area.
//!
//! Page coordinates are inches with y up, as the drawing's page sheet has them. The screen has y down and
//! counts points. At 100% one inch is [`POINTS_PER_INCH`] points (a CSS inch, the reference density display
//! scaling is set against), whatever the scale factor of the display.
//!
//! The zoom-about-a-point and fit arithmetic is the shape of the sibling craft apps' canvas views
//! (`docs/provenance.md`), rewritten for a page measured in inches.

use egui::{Pos2, Rect, Vec2, pos2};

/// Points to one inch at 100%.
pub const POINTS_PER_INCH: f64 = 96.0;

/// The smallest zoom, as a fraction (10%). A project rule: the status bar's slider spans the same range.
pub const MIN_ZOOM: f64 = 0.10;

/// The largest zoom, as a fraction (400%). A project rule.
pub const MAX_ZOOM: f64 = 4.0;

/// How far the middle of the view may be from the page's origin, in inches: a hostile pan cannot send it to a
/// number the arithmetic would lose precision at.
const MAX_CENTER: f64 = 100_000.0;

/// Empty room left round a page by Fit, in points.
const FIT_MARGIN: f64 = 28.0;

/// What a page is zoomed to and where it is looked at.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    /// 1.0 is 100%.
    pub zoom: f64,
    /// The page point (inches, y up) in the middle of the drawing area.
    pub center: (f64, f64),
    /// The view was made by Fit and has not been changed by hand since: a resize of the window fits it again.
    pub fitted: bool,
}

impl Default for View {
    fn default() -> Self {
        View { zoom: 1.0, center: (0.0, 0.0), fitted: false }
    }
}

impl View {
    /// Points to one inch at this zoom.
    pub fn scale(&self) -> f64 {
        self.zoom * POINTS_PER_INCH
    }

    /// The screen point of the page point `p` when the drawing area is `rect`.
    pub fn to_screen(&self, rect: Rect, p: (f64, f64)) -> Pos2 {
        let s = self.scale();
        let c = rect.center();
        pos2(c.x + ((p.0 - self.center.0) * s) as f32, c.y - ((p.1 - self.center.1) * s) as f32)
    }

    /// The page point under the screen point `s` when the drawing area is `rect`.
    pub fn to_page(&self, rect: Rect, s: Pos2) -> (f64, f64) {
        let k = self.scale();
        let c = rect.center();
        (self.center.0 + f64::from(s.x - c.x) / k, self.center.1 - f64::from(s.y - c.y) / k)
    }

    /// The part of the page the drawing area `rect` shows, in inches: lower left, then upper right.
    pub fn visible(&self, rect: Rect) -> ((f64, f64), (f64, f64)) {
        let (a, b) = (self.to_page(rect, rect.left_bottom()), self.to_page(rect, rect.right_top()));
        ((a.0.min(b.0), a.1.min(b.1)), (a.0.max(b.0), a.1.max(b.1)))
    }

    /// The screen rectangle of a page of `size` inches whose lower left corner is the page origin.
    pub fn page_rect(&self, rect: Rect, size: (f64, f64)) -> Rect {
        let a = self.to_screen(rect, (0.0, size.1));
        let b = self.to_screen(rect, (size.0, 0.0));
        Rect::from_min_max(a, b)
    }

    /// Sets the zoom (clamped to the range) keeping the page point under the screen point `anchor` where it is.
    pub fn zoom_about(&mut self, rect: Rect, anchor: Pos2, zoom: f64) {
        if !zoom.is_finite() || !(anchor.x.is_finite() && anchor.y.is_finite()) {
            return;
        }
        let before = self.to_page(rect, anchor);
        self.zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        let after = self.to_page(rect, anchor);
        self.set_center((self.center.0 + before.0 - after.0, self.center.1 + before.1 - after.1));
        self.fitted = false;
    }

    /// Moves the content by `by` screen points (the page follows a drag by the same amount).
    pub fn pan_by(&mut self, by: Vec2) {
        if !(by.x.is_finite() && by.y.is_finite()) {
            return;
        }
        let s = self.scale();
        self.set_center((self.center.0 - f64::from(by.x) / s, self.center.1 + f64::from(by.y) / s));
        self.fitted = false;
    }

    fn set_center(&mut self, c: (f64, f64)) {
        if c.0.is_finite() && c.1.is_finite() {
            self.center = (c.0.clamp(-MAX_CENTER, MAX_CENTER), c.1.clamp(-MAX_CENTER, MAX_CENTER));
        }
    }

    /// The view that shows the whole page of `size` inches in the drawing area `rect`, centred.
    pub fn fit_page(rect: Rect, size: (f64, f64)) -> View {
        let (w, h) = (size.0.max(1e-6), size.1.max(1e-6));
        let zx = (f64::from(rect.width()) - 2.0 * FIT_MARGIN) / (w * POINTS_PER_INCH);
        let zy = (f64::from(rect.height()) - 2.0 * FIT_MARGIN) / (h * POINTS_PER_INCH);
        let zoom = zx.min(zy);
        View { zoom: if zoom.is_finite() { zoom.clamp(MIN_ZOOM, MAX_ZOOM) } else { 1.0 }, center: (size.0 / 2.0, size.1 / 2.0), fitted: true }
    }

    /// The view that makes the page as wide as the drawing area, at the top of the page.
    pub fn fit_width(rect: Rect, size: (f64, f64)) -> View {
        let zx = (f64::from(rect.width()) - 2.0 * FIT_MARGIN) / (size.0.max(1e-6) * POINTS_PER_INCH);
        let zoom = if zx.is_finite() { zx.clamp(MIN_ZOOM, MAX_ZOOM) } else { 1.0 };
        // The top of the page at the top of the area, with the margin above it.
        let visible_h = f64::from(rect.height()) / (zoom * POINTS_PER_INCH);
        let cy = size.1 + FIT_MARGIN / (zoom * POINTS_PER_INCH) - visible_h / 2.0;
        View { zoom, center: (size.0 / 2.0, cy), fitted: false }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area() -> Rect {
        Rect::from_min_size(pos2(100.0, 50.0), egui::vec2(800.0, 600.0))
    }

    #[test]
    fn zoom_keeps_the_point_under_the_pointer() {
        let mut v = View::fit_page(area(), (11.0, 8.5));
        let anchor = pos2(300.0, 200.0);
        let before = v.to_page(area(), anchor);
        v.zoom_about(area(), anchor, v.zoom * 1.7);
        let after = v.to_page(area(), anchor);
        assert!((before.0 - after.0).abs() < 1e-6 && (before.1 - after.1).abs() < 1e-6, "{before:?} {after:?}");
        assert!(!v.fitted, "a hand zoom ends Fit");
    }

    #[test]
    fn zoom_is_clamped_and_hostile_numbers_change_nothing() {
        let mut v = View::default();
        v.zoom_about(area(), pos2(0.0, 0.0), 1e9);
        assert_eq!(v.zoom, MAX_ZOOM);
        v.zoom_about(area(), pos2(0.0, 0.0), 1e-9);
        assert_eq!(v.zoom, MIN_ZOOM);
        let kept = v;
        v.zoom_about(area(), pos2(0.0, 0.0), f64::NAN);
        v.zoom_about(area(), pos2(f32::NAN, 0.0), 2.0);
        v.pan_by(Vec2::new(f32::INFINITY, 0.0));
        assert_eq!(v, kept);
    }

    #[test]
    fn fit_centres_the_page_and_leaves_a_margin() {
        let v = View::fit_page(area(), (11.0, 8.5));
        let page = v.page_rect(area(), (11.0, 8.5));
        assert!((page.center().x - area().center().x).abs() < 0.5 && (page.center().y - area().center().y).abs() < 0.5);
        assert!(area().contains_rect(page), "{page:?} in {:?}", area());
        assert!(v.fitted);
    }

    #[test]
    fn panning_moves_the_page_with_the_drag() {
        let mut v = View::fit_page(area(), (11.0, 8.5));
        let before = v.page_rect(area(), (11.0, 8.5));
        v.pan_by(Vec2::new(30.0, -12.0));
        let after = v.page_rect(area(), (11.0, 8.5));
        assert!((after.min.x - before.min.x - 30.0).abs() < 0.01 && (after.min.y - before.min.y + 12.0).abs() < 0.01);
    }
}
