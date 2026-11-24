//! Black and white: a print's colours as the greys of the same lightness, for the Print view's **Black and white**.
//!
//! **The grey of a colour** is its luma, `Y' = 0.299 R' + 0.587 G' + 0.114 B'`: the weights of ITU-R BT.601 on the sRGB-encoded values, the
//! ones `ColorTransform::Gray` uses, worked in whole numbers by [`diacraft_color::luma`]. Everything a sheet draws is turned: fills, lines,
//! gradients, patterns, shadows, text and pictures (each pixel), and the colours a theme gives, which reach the drawing as colours like any
//! other. Alpha is kept, so what is see-through stays so. **Ours** (in `docs/editing-commands.md`): no specification says how a
//! print in black and white is drawn (names the choice only), so it is a plain grayscale and not pure black on
//! white.
//!
//! **Two ways, one result.** A sheet drawn as pixels ([`crate::Renderer::render_sheet`], the preview) is drawn in colour and each pixel is
//! turned after: the luma is a weighted mean, so it gives the same as turning each colour before they are blended, premultiplied alpha
//! included, and a shadow keeps its blur. A sheet drawn onto a target ([`crate::print::draw_sheet`], the PDF a printer is given) has its colours
//! turned as they are handed over, by [`Grayed`].

use std::sync::Arc;

use kurbo::{Affine, BezPath, Rect, Stroke};
use vello_cpu::Pixmap;

use crate::target::{Gradient, GradientStop, Paint, Refused, Target, TextRun};

/// `paint` in greys: a colour, or every stop of a gradient.
pub(crate) fn gray_paint(paint: &Paint) -> Paint {
    match paint {
        Paint::Solid(c) => Paint::Solid(c.gray()),
        Paint::Gradient(g) => Paint::Gradient(Gradient {
            kind: g.kind,
            stops: g.stops.iter().map(|s| GradientStop { offset: s.offset, color: s.color.gray() }).collect(),
        }),
    }
}

/// `image` in greys, pixel by pixel, alpha kept (a premultiplied pixel stays one: its grey is never above its alpha).
pub(crate) fn gray_pixmap(image: &Pixmap) -> Pixmap {
    let mut grey = image.clone();
    diacraft_color::gray_pixels(grey.data_as_u8_slice_mut());
    grey
}

/// A target that hands every colour on to `inner` as its grey: what a sheet in black and white is drawn onto when it is not drawn as pixels.
///
/// **It draws no blur** ([`Target::can_blur`] is false): the walk then draws a blurred shadow hard-edged and says so, as it does for a PDF,
/// the one target a print is written to, which cannot blur either. Everything else is `inner`'s: its units, its size, its clips, its text.
pub(crate) struct Grayed<'t, T: Target> {
    inner: &'t mut T,
}

impl<'t, T: Target> Grayed<'t, T> {
    pub(crate) fn new(inner: &'t mut T) -> Grayed<'t, T> {
        Grayed { inner }
    }
}

impl<T: Target> Target for Grayed<'_, T> {
    fn to_px(&self) -> Affine {
        self.inner.to_px()
    }

    fn size(&self) -> (f64, f64) {
        self.inner.size()
    }

    fn fill(&mut self, path: &BezPath, paint: &Paint, even_odd: bool) {
        self.inner.fill(path, &gray_paint(paint), even_odd);
    }

    fn fill_with_paint_transform(&mut self, path: &BezPath, paint: &Paint, even_odd: bool, paint_transform: Affine) {
        self.inner.fill_with_paint_transform(path, &gray_paint(paint), even_odd, paint_transform);
    }

    fn stroke(&mut self, path: &BezPath, stroke: &Stroke, paint: &Paint) {
        self.inner.stroke(path, stroke, &gray_paint(paint));
    }

    fn min_stroke(&self) -> f64 {
        self.inner.min_stroke()
    }

    fn blurred<F: FnOnce(&mut Self)>(&mut self, _sigma: f64, draw: F) {
        // Not called: `can_blur` is false, and the walk asks it first. If it is, the shadow is drawn sharp.
        draw(self);
    }

    fn can_blur(&self) -> bool {
        false
    }

    fn push_clip(&mut self, path: &BezPath) {
        self.inner.push_clip(path);
    }

    fn pop_clip(&mut self) {
        self.inner.pop_clip();
    }

    fn image(&mut self, image: &Arc<Pixmap>, place: Affine) {
        self.inner.image(&Arc::new(gray_pixmap(image)), place);
    }

    fn mark(&self) -> usize {
        self.inner.mark()
    }

    fn rewind(&mut self, mark: usize) {
        self.inner.rewind(mark);
    }

    fn wants_text(&self) -> bool {
        self.inner.wants_text()
    }

    fn text_run(&mut self, run: &TextRun<'_>) -> bool {
        self.inner.text_run(&TextRun { color: run.color.gray(), ..*run })
    }

    fn pixel_rect(&self) -> Rect {
        self.inner.pixel_rect()
    }

    fn blur_layer_pixels(&self, paper: Rect, spread_px: f64) -> u64 {
        self.inner.blur_layer_pixels(paper, spread_px)
    }

    fn paper_view(&self) -> Option<Rect> {
        self.inner.paper_view()
    }

    fn check(&self, path: &BezPath, margin_px: f64) -> Result<bool, Refused> {
        self.inner.check(path, margin_px)
    }

    fn bring_in(&self, path: BezPath, stroke: bool, margin_px: f64) -> Result<Option<(BezPath, bool)>, Refused> {
        self.inner.bring_in(path, stroke, margin_px)
    }
}

#[cfg(test)]
mod tests {
    use diacraft_color::Rgba;
    use kurbo::Point;

    use super::*;
    use crate::target::GradientKind;

    #[test]
    fn a_paint_keeps_its_geometry_and_alpha_and_every_colour_becomes_its_luma() {
        assert_eq!(gray_paint(&Paint::Solid(Rgba::rgba(255, 0, 0, 128))), Paint::Solid(Rgba::rgba(76, 76, 76, 128)));
        let kind = GradientKind::Linear { start: Point::new(0.0, 0.0), end: Point::new(1.0, 0.0) };
        let stops = vec![GradientStop { offset: 0.0, color: Rgba::rgb(0, 0, 255) }, GradientStop { offset: 0.7, color: Rgba::rgba(0, 255, 0, 10) }];
        let Paint::Gradient(g) = gray_paint(&Paint::Gradient(Gradient { kind, stops })) else { panic!("a gradient stays one") };
        assert_eq!(g.kind, kind);
        assert_eq!(
            g.stops,
            vec![GradientStop { offset: 0.0, color: Rgba::rgb(29, 29, 29) }, GradientStop { offset: 0.7, color: Rgba::rgba(150, 150, 150, 10) }]
        );
    }

    #[test]
    fn a_picture_is_turned_pixel_by_pixel_and_the_original_is_left_alone() {
        let mut image = Pixmap::new(2, 1);
        image.data_as_u8_slice_mut().copy_from_slice(&[200, 0, 0, 200, 0, 0, 100, 100]);
        let grey = gray_pixmap(&image);
        // 0.299 * 200 = 59.8, rounded 60, under its alpha 200; 0.114 * 100 = 11.4, rounded 11.
        assert_eq!(grey.data_as_u8_slice(), &[60, 60, 60, 200, 11, 11, 11, 100]);
        assert_eq!(image.data_as_u8_slice(), &[200, 0, 0, 200, 0, 0, 100, 100]);
        assert_eq!((grey.width(), grey.height()), (2, 1));
    }
}
