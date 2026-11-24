//! The drawing surface: a `vello_cpu` context, with the checks every path passes before the rasteriser sees it.
//!
//! Everything is drawn in **paper inches, y up**, through the view's transform to pixels. A stroke's width is in paper inches too, which
//! is why a line weight does not follow the drawing scale while geometry does (the shape's transform
//! carries the page's scale into paper inches before it reaches here).

use std::sync::Arc;

use diacraft_color::Rgba;
use kurbo::{Affine, BezPath, Rect, Stroke};
use vello_cpu::filter_effects::{EdgeMode, Filter, FilterPrimitive};
use vello_cpu::peniko::{self, ColorStop, Extend, Fill};
use vello_cpu::{Pixmap, PixmapMut, RenderContext, RenderSettings, Resources};

use crate::target::{Gradient, GradientKind, Paint, Target};

fn to_vello(paint: &Paint) -> vello_cpu::PaintType {
    match paint {
        Paint::Solid(c) => vello_cpu::PaintType::Solid(color_of(*c)),
        Paint::Gradient(g) => vello_cpu::PaintType::Gradient(gradient_of(g)),
    }
}

/// A colour for the rasteriser. `Rgba` here is straight (not premultiplied) alpha.
pub(crate) fn color_of(c: Rgba) -> peniko::Color {
    peniko::Color::from_rgba8(c.r, c.g, c.b, c.a)
}

/// A gradient for the rasteriser: held beyond its ends, blended as the straight bytes and not the premultiplied ones.
fn gradient_of(g: &Gradient) -> peniko::Gradient {
    let mut out = match g.kind {
        GradientKind::Linear { start, end } => peniko::Gradient::new_linear(start, end),
        GradientKind::Radial { center, radius } => peniko::Gradient::new_radial(center, radius as f32),
    };
    out.extend = Extend::Pad;
    out.interpolation_alpha_space = peniko::InterpolationAlphaSpace::Unpremultiplied;
    let stops: Vec<ColorStop> = g.stops.iter().map(|s| ColorStop::from((s.offset as f32, color_of(s.color)))).collect();
    out.with_stops(stops.as_slice())
}

/// The narrowest stroke the raster draws, in pixels (a project rule). A stroke under a pixel wide comes out as a grey
/// as light as it is thin, so the walk draws one a pixel wide at the line's full colour. **Change it here** if a render disagrees.
pub(crate) const MIN_STROKE_PX: f64 = 1.0;

/// A clip or a layer pushed on the rasteriser and not yet popped.
#[derive(Clone, Copy)]
enum Open {
    Clip,
    Layer,
}

pub(crate) struct Canvas {
    ctx: RenderContext,
    width: u16,
    height: u16,
    to_px: Affine,
    /// The clips and layers in force, innermost last: what [`Canvas::rewind`] pops when a drawing is cut short by a panic.
    open: Vec<Open>,
}

impl Canvas {
    /// A canvas of `width` by `height` pixels (each at most 65,535: the caller checks the size against the limits) drawn through `to_px`.
    pub(crate) fn new(width: u16, height: u16, to_px: Affine) -> Canvas {
        let settings = RenderSettings { num_threads: 0, ..RenderSettings::default() };
        let mut ctx = RenderContext::new_with(width, height, settings);
        ctx.set_transform(to_px);
        Canvas { ctx, width, height, to_px, open: Vec::new() }
    }

    /// The pixels, premultiplied RGBA, row by row from the top.
    pub(crate) fn finish(self) -> Vec<u8> {
        let height = self.height;
        self.finish_rows(height).data_as_u8_slice().to_vec()
    }

    /// The first `rows` rows of the pixels (at most the canvas's height), premultiplied RGBA. The rasteriser cuts what it draws off at
    /// the pixmap's edge, so a band of a frame is drawn without the rows below it (`par.rs`).
    pub(crate) fn finish_rows(mut self, rows: u16) -> Pixmap {
        let mut target = Pixmap::new(self.width, rows.min(self.height));
        let mut resources = Resources::new();
        self.ctx.flush();
        self.ctx.render(&mut target, &mut resources);
        target
    }

    /// As [`Canvas::finish_rows`], into `scratch`, which a band's thread keeps from one band to the next (it grows to the biggest band and is
    /// not cleared: the rasteriser writes every pixel of the rows it is given). The rows are the first bytes of `scratch`; `None` if they
    /// cannot be made (a size that is not what the pixmap takes).
    pub(crate) fn finish_rows_into(mut self, rows: u16, scratch: &mut Vec<u8>) -> Option<usize> {
        let rows = rows.min(self.height);
        let need = usize::from(self.width) * usize::from(rows) * 4;
        if scratch.len() < need {
            scratch.resize(need, 0);
        }
        let target = PixmapMut::new(self.width, rows, scratch.get_mut(..need)?)?;
        let mut resources = Resources::new();
        self.ctx.flush();
        self.ctx.render(target, &mut resources);
        Some(need)
    }
}

impl Target for Canvas {
    fn to_px(&self) -> Affine {
        self.to_px
    }

    fn size(&self) -> (f64, f64) {
        (f64::from(self.width), f64::from(self.height))
    }

    fn min_stroke(&self) -> f64 {
        MIN_STROKE_PX
    }

    fn mark(&self) -> usize {
        self.open.len()
    }

    /// Pops the clips and layers pushed since `mark` and puts the transform and the paint transform back, so that a drawing that was cut
    /// short (a panic caught between a push and its pop) does not clip or blur what is drawn after it.
    fn rewind(&mut self, mark: usize) {
        while self.open.len() > mark {
            match self.open.pop() {
                Some(Open::Clip) => self.ctx.pop_clip(),
                Some(Open::Layer) => self.ctx.pop_layer(),
                None => break,
            }
        }
        self.ctx.reset_paint_transform();
        self.ctx.set_transform(self.to_px);
    }

    fn fill(&mut self, path: &BezPath, paint: &Paint, even_odd: bool) {
        self.ctx.set_transform(self.to_px);
        self.ctx.set_paint(to_vello(paint));
        self.ctx.set_fill_rule(if even_odd { Fill::EvenOdd } else { Fill::NonZero });
        self.ctx.fill_path(path);
    }

    fn fill_with_paint_transform(&mut self, path: &BezPath, paint: &Paint, even_odd: bool, paint_transform: Affine) {
        self.ctx.set_paint_transform(paint_transform);
        self.fill(path, paint, even_odd);
        self.ctx.reset_paint_transform();
    }

    fn stroke(&mut self, path: &BezPath, stroke: &Stroke, paint: &Paint) {
        self.ctx.set_transform(self.to_px);
        self.ctx.set_paint(to_vello(paint));
        self.ctx.set_stroke(stroke.clone());
        self.ctx.stroke_path(path);
    }

    fn blurred<F: FnOnce(&mut Canvas)>(&mut self, sigma: f64, draw: F) {
        // The deviation is in the user units of the transform in force when the layer is pushed (a probe of `vello_cpu` 0.2 and again of 0.3
        // shows it scaled by the transform: 0.2 inch at 100 pixels to the inch blurs as 20 pixels do at 1), and our user unit is the paper inch.
        // In 0.4 each axis has its own deviation, scaled through the transform's linear part (a turned or sheared view blurs per axis, where 0.3
        // used the average scale factor), and `gaussian_blur` gives both axes the one value. `a_blur_reaches_the_same_paper_inches_at_every_pixel_density` holds it.
        let filter = Filter::from_primitive(FilterPrimitive::gaussian_blur(sigma as f32, EdgeMode::None));
        self.ctx.set_transform(self.to_px);
        self.ctx.push_filter_layer(filter);
        self.open.push(Open::Layer);
        draw(self);
        self.open.pop();
        self.ctx.pop_layer();
    }

    fn push_clip(&mut self, path: &BezPath) {
        self.ctx.set_transform(self.to_px);
        // Clipped by the odd-crossings rule, as the SVG and PDF writers clip: the rasteriser takes the rule last set for a fill, which would make
        // a clip depend on the fill before it. The parts of a metafile's clip (an exclusion is a frame with a hole) rely on it.
        self.ctx.set_fill_rule(Fill::EvenOdd);
        self.ctx.push_clip_path(path);
        self.open.push(Open::Clip);
    }

    fn pop_clip(&mut self) {
        if self.open.pop().is_some() {
            self.ctx.pop_clip();
        }
    }

    /// The image is stretched, not tiled, and what is drawn is cut by the clip in force.
    fn image(&mut self, image: &Arc<Pixmap>, place: Affine) {
        let source = vello_cpu::ImageSource::Pixmap(Arc::clone(image));
        let paint = vello_cpu::Image { image: source, sampler: peniko::ImageSampler::default() };
        self.ctx.set_transform(self.to_px * place);
        self.ctx.set_paint(vello_cpu::PaintType::Image(paint));
        self.ctx.set_fill_rule(Fill::NonZero);
        self.ctx.fill_rect(&Rect::new(0.0, 0.0, f64::from(image.width()), f64::from(image.height())));
        self.ctx.set_transform(self.to_px);
    }
}
