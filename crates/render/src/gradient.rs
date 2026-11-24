//! Gradients.
//!
//! A gradient is laid over a **box**: the extents of the shape's fillable paths in the gradient's frame. The frame is the shape's
//! own when the gradient turns with it (`RotateGradientWithShape` 1) and the page's when not.
//!
//! * Linear (direction 0): `t = 0.5 + (p - centre) . d / L` with `d = (cos a, SIGN sin a)` and `L = W|cos a| + H|sin a|`, so opposite
//!   corners get 0 and 1 and the bands stay square to the direction. The sign is [`GRADIENT_ANGLE_SIGN`](crate::style::GRADIENT_ANGLE_SIGN).
//! * Radial (1 to 7): the distance in shares of the box from the origin of the table, over the distance to the farthest corner, so the
//!   bands are ellipses on a box that is not square.
//! * Rectangular (8 to 12) is stepped in nested rectangles: the rasteriser has no such gradient.
//! * Along the path (13) paints as direction 10 (the fallback).
//!
//! Colours blend as the straight RGB bytes and alpha, not premultiplied.

use diacraft_color::Rgba;
use kurbo::{Affine, Point, Rect};

use crate::limits::RECT_GRADIENT_BANDS;
use crate::style::{GRADIENT_ANGLE_SIGN, GradientSpec, Stop};
use crate::target::{Gradient, GradientKind, GradientStop, Paint};

/// What a gradient fill is drawn with.
pub(crate) enum GradFill {
    /// A paint, with the transform that takes its geometry to paper inches.
    Paint { paint: Paint, paint_transform: Affine },
    /// Nested rectangles in a unit square, outermost first, each with its colour, and the transform from that square to paper inches.
    Bands { rects: Vec<(Rect, Rgba)>, unit_to_paper: Affine },
    /// One colour: a gradient with a single stop, or a box with no extent.
    Flat(Rgba),
}

/// The origin of a radial or rectangular gradient as shares of the box from its lower left.
fn origin(dir: u8) -> (f64, f64) {
    match dir {
        1 | 8 => (1.0, 0.0),
        2 | 9 => (0.0, 0.0),
        4 => (0.5, 0.0),
        5 => (0.5, 1.0),
        6 | 11 => (1.0, 1.0),
        7 | 12 => (0.0, 1.0),
        // 3 and 10 are the centre, and 13 paints as 10.
        _ => (0.5, 0.5),
    }
}

/// The colour at `t` in 0 to 1: stops hold their colour beyond the ends, and the blend is straight.
pub(crate) fn colour_at(stops: &[Stop], t: f64) -> Rgba {
    let (Some(first), Some(last)) = (stops.first(), stops.last()) else {
        return Rgba::TRANSPARENT;
    };
    if t <= first.pos {
        return first.color;
    }
    if t >= last.pos {
        return last.color;
    }
    for pair in stops.windows(2) {
        if let [a, b] = pair
            && t >= a.pos
            && t <= b.pos
        {
            let span = b.pos - a.pos;
            return if span <= 0.0 { b.color } else { a.color.lerp(b.color, (t - a.pos) / span) };
        }
    }
    last.color
}

/// A gradient over `kind` with these stops. A target holds it beyond the ends and blends the straight bytes, not the premultiplied
/// ones.
fn with_stops(kind: GradientKind, stops: &[Stop]) -> Gradient {
    Gradient { kind, stops: stops.iter().map(|s| GradientStop { offset: s.pos, color: s.color }).collect() }
}

/// The fill for `spec` over the box `frame_box` of a frame that `frame_to_paper` takes to paper inches.
pub(crate) fn build(spec: &GradientSpec, frame_box: Rect, frame_to_paper: Affine) -> GradFill {
    let Some(only) = spec.stops.first() else {
        return GradFill::Flat(Rgba::TRANSPARENT);
    };
    let (w, h) = (frame_box.width(), frame_box.height());
    if spec.stops.len() == 1 || !(w.is_finite() && h.is_finite()) || (spec.dir != 0 && (w <= 0.0 || h <= 0.0)) {
        return GradFill::Flat(only.color);
    }
    match spec.dir {
        0 => {
            let (cos, sin) = (spec.angle.cos(), spec.angle.sin());
            let length = w * cos.abs() + h * sin.abs();
            if !(length.is_finite() && length > 0.0) {
                return GradFill::Flat(only.color);
            }
            // The direction of travel in the frame's own x right, y up.
            let d = (cos, GRADIENT_ANGLE_SIGN * sin);
            let c = frame_box.center();
            let start = Point::new(c.x - d.0 * length / 2.0, c.y - d.1 * length / 2.0);
            let end = Point::new(c.x + d.0 * length / 2.0, c.y + d.1 * length / 2.0);
            let g = with_stops(GradientKind::Linear { start, end }, &spec.stops);
            GradFill::Paint { paint: Paint::Gradient(g), paint_transform: frame_to_paper }
        }
        1..=7 => {
            let (ou, ov) = origin(spec.dir);
            let reach = (ou.max(1.0 - ou)).hypot(ov.max(1.0 - ov));
            let unit_to_frame = Affine::translate((frame_box.x0, frame_box.y0)) * Affine::scale_non_uniform(w, h);
            let g = with_stops(GradientKind::Radial { center: Point::new(ou, ov), radius: reach }, &spec.stops);
            GradFill::Paint { paint: Paint::Gradient(g), paint_transform: frame_to_paper * unit_to_frame }
        }
        _ => {
            let (ou, ov) = origin(spec.dir);
            let (ru, rv) = (ou.max(1.0 - ou), ov.max(1.0 - ov));
            let n = RECT_GRADIENT_BANDS;
            let rects = (0..n)
                .rev()
                .map(|k| {
                    let t_hi = (k + 1) as f64 / n as f64;
                    let t_mid = (k as f64 + 0.5) / n as f64;
                    let rect = Rect::new(ou - t_hi * ru, ov - t_hi * rv, ou + t_hi * ru, ov + t_hi * rv);
                    (rect, colour_at(&spec.stops, t_mid))
                })
                .collect();
            let unit_to_frame = Affine::translate((frame_box.x0, frame_box.y0)) * Affine::scale_non_uniform(w, h);
            GradFill::Bands { rects, unit_to_paper: frame_to_paper * unit_to_frame }
        }
    }
}
