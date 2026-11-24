//! Circular and elliptical arcs, and their cubic Bezier form.
//!
//! The signs and ratios are
//! applied only through [`crate::rules`].

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use kurbo::{Point, Size, Vec2};

use crate::rules;
use crate::seg::Seg;

/// The most a curve made of cubic Beziers may leave the exact one, in the shape's units (inches). Arcs and ellipses are cut into as many cubics as keep them
/// within this, by the known error of a cubic against a circle. A spline that a cubic cannot stand for (rational, or over degree 3) is cut until the cubics are
/// within it at three points each, and a warning says where that stopped short.
pub const CURVE_TOLERANCE: f64 = 1e-7;

/// The error of one cubic Bezier of a quarter circle, as a share of the radius (the known figure, 2.7E-4). The error falls with the
/// sixth power of the angle one piece covers.
const QUARTER_ERROR: f64 = 2.7e-4;

/// The smallest angle one piece covers, a degree: it keeps a curve of any size to at most 360 pieces a turn.
const MIN_STEP: f64 = PI / 180.0;

/// The arc of an `ArcTo` row: the circle through the pen and the end whose middle stands `bow` from the chord's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CircularArc {
    pub center: Point,
    pub radius: f64,
    /// The angle of the pen as seen from the centre, in radians, counterclockwise from the x-axis.
    pub start_angle: f64,
    /// Radians: positive is counterclockwise as seen with y up, and `4 atan(2A / c)`.
    pub sweep: f64,
    /// The arc's middle point, the one `bow` from the chord's middle.
    pub middle: Point,
}

/// The arc of an `EllipticalArcTo` row.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EllipseArc {
    pub center: Point,
    /// The half-axes: `x` along the axis at `rotation` (the ratio times the other), `y` across it.
    pub radii: Vec2,
    /// The angle of the axis `radii.x` lies along, in radians: `C` of the row.
    pub rotation: f64,
    /// The circle-space angle of the pen, in radians (the parameter of the ellipse: the point is `center + u cos t + v sin t`).
    pub start_angle: f64,
    /// Radians, positive counterclockwise; may exceed a half turn, and runs the way that passes through the row's point.
    pub sweep: f64,
}

/// Why an arc is drawn as a straight line (or, for `Overflow`, as a straight line with a warning).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArcFallback {
    /// The three points fix no ellipse (all on one line once scaled, or two the same).
    NoEllipse,
    /// The ratio is 0, below 0, above 1000 or not a number.
    Ratio,
    /// No chord, that is an end that is the pen to within `rules::ARC_CHORD_RATIO` of the shape's size: a point.
    NoChord,
    /// A bow too small a share of the chord to leave it (`rules::ARC_FLAT_RATIO`): a line.
    Flat,
    /// The numbers are finite and the arithmetic on them is not (a bow of 1E200, points of 1E300): no arc can be made, and a straight line is drawn with a warning.
    Overflow,
}

/// The circular arc of an `ArcTo` from `start` to `end` with bow `bow` (with the sign rule) for a shape of `size`, or the reason it is a
/// straight line: no chord (a chord at rounding noise is none, `rules::arc_chord_noise`), a bow too small a share of the chord to leave it,
/// or numbers whose arithmetic overflows.
pub fn circular_arc(start: Point, end: Point, bow: f64, size: Size) -> Result<CircularArc, ArcFallback> {
    let a = rules::effective_bow(bow);
    let (dx, dy) = (end.x - start.x, end.y - start.y);
    let chord = dx.hypot(dy);
    let half = chord / 2.0;
    // Overflow is told before noise: a bow whose square is not a number (1E200) is a bow that is not an arc, and it would also count as the scale of the noise and
    // make any chord noise, which is a silent line where the warning is owed.
    if !(chord.is_finite() && a.is_finite() && (half * half).is_finite() && (a * a).is_finite() && (half * half + a * a).is_finite()) {
        return Err(ArcFallback::Overflow);
    }
    if chord <= rules::arc_chord_noise(size.width, size.height, start, end, bow) {
        return Err(ArcFallback::NoChord);
    }
    if a.abs() <= rules::ARC_FLAT_RATIO * chord {
        return Err(ArcFallback::Flat);
    }
    // The unit normal to the right of travel: (dy, -dx) / c.
    let normal = Vec2::new(dy / chord, -dx / chord);
    let middle_of_chord = Point::new((start.x + end.x) / 2.0, (start.y + end.y) / 2.0);
    let radius = (half * half + a * a) / (2.0 * a.abs());
    let center = middle_of_chord + normal * ((a * a - half * half) / (2.0 * a));
    let sweep = 4.0 * (2.0 * a / chord).atan();
    let start_angle = (start.y - center.y).atan2(start.x - center.x);
    let middle = middle_of_chord + normal * a;
    let finite = radius.is_finite() && center.is_finite() && sweep.is_finite() && start_angle.is_finite() && middle.is_finite();
    if finite { Ok(CircularArc { center, radius, start_angle, sweep, middle }) } else { Err(ArcFallback::Overflow) }
}

/// The elliptical arc from `start` through `mid` to `end`, with the axis turned `angle` and the axis ratio `ratio`: turn
/// the three points, divide each x by the ratio, find the circle, and take its arc from the first through the second to the third.
/// A ratio the page calls unreliable, or points that fix no ellipse, give the reason to draw the chord instead.
pub fn elliptical_arc(start: Point, mid: Point, end: Point, angle: f64, ratio: f64) -> Result<EllipseArc, ArcFallback> {
    if !(ratio > 0.0 && ratio <= rules::MAX_ELLIPTICAL_RATIO) {
        return Err(ArcFallback::Ratio);
    }
    let turn = rules::AXIS_TURN_SIGN * angle;
    let (sin, cos) = turn.sin_cos();
    let scale = rules::ratio_scale(ratio);
    let to_circle = |p: Point| Point::new((p.x * cos - p.y * sin) * scale, p.x * sin + p.y * cos);
    let (a, b, c) = (to_circle(start), to_circle(mid), to_circle(end));
    if !(a.is_finite() && b.is_finite() && c.is_finite()) {
        return Err(ArcFallback::Overflow);
    }
    let (center, radius) = circumcircle(a, b, c)?;
    let angle_of = |p: Point| (p.y - center.y).atan2(p.x - center.x);
    let (start_angle, mid_angle, end_angle) = (angle_of(a), angle_of(b), angle_of(c));
    let counterclockwise = (end_angle - start_angle).rem_euclid(TAU);
    let to_mid = (mid_angle - start_angle).rem_euclid(TAU);
    // The arc runs the way that passes through the point, which fixes the direction and may exceed a half turn.
    let sweep = if to_mid <= counterclockwise { counterclockwise } else { counterclockwise - TAU };
    // Multiply x by the ratio and turn back (the inverse of `to_circle`).
    let (sin_back, cos_back) = (-turn).sin_cos();
    let (x, y) = (center.x / scale, center.y);
    let real_center = Point::new(x * cos_back - y * sin_back, x * sin_back + y * cos_back);
    let radii = Vec2::new(radius / scale, radius);
    let rotation = -turn;
    let finite = real_center.is_finite() && radii.is_finite() && rotation.is_finite() && start_angle.is_finite() && sweep.is_finite();
    if finite { Ok(EllipseArc { center: real_center, radii, rotation, start_angle, sweep }) } else { Err(ArcFallback::Overflow) }
}

/// The circle through three points: its centre and radius, `NoEllipse` when the points are on one line, or `Overflow` when the arithmetic is
/// not a number. Lying on one line is judged by the area they span over the square of their longest side, against `rules::COLLINEAR_RATIO`.
fn circumcircle(a: Point, b: Point, c: Point) -> Result<(Point, f64), ArcFallback> {
    let (bx, by) = (b.x - a.x, b.y - a.y);
    let (cx, cy) = (c.x - a.x, c.y - a.y);
    let (b2, c2) = (bx * bx + by * by, cx * cx + cy * cy);
    let bc2 = (b.x - c.x).powi(2) + (b.y - c.y).powi(2);
    let longest2 = b2.max(c2).max(bc2);
    let d = 2.0 * (bx * cy - by * cx);
    // Squares that overflow, or a product that does, are no sign of points on a line.
    if !(longest2.is_finite() && d.is_finite()) {
        return Err(ArcFallback::Overflow);
    }
    if d.abs() <= 2.0 * rules::COLLINEAR_RATIO * longest2 {
        return Err(ArcFallback::NoEllipse);
    }
    let (ux, uy) = ((cy * b2 - by * c2) / d, (bx * c2 - cx * b2) / d);
    let center = Point::new(a.x + ux, a.y + uy);
    let radius = ux.hypot(uy);
    if center.is_finite() && radius.is_finite() { Ok((center, radius)) } else { Err(ArcFallback::Overflow) }
}

/// An ellipse as a centre and two conjugate half-diameters: the point at parameter `t` is `center + u cos t + v sin t`. A circle of
/// radius `r` is `u = (r, 0)` and `v = (0, r)`; an upright or turned ellipse has its half-axes for `u` and `v`; the `Ellipse` row's
/// two points give any pair.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Conjugate {
    pub center: Point,
    pub u: Vec2,
    pub v: Vec2,
}

impl Conjugate {
    pub(crate) fn circle(center: Point, radius: f64) -> Conjugate {
        Conjugate { center, u: Vec2::new(radius, 0.0), v: Vec2::new(0.0, radius) }
    }

    pub(crate) fn ellipse(arc: &EllipseArc) -> Conjugate {
        let (sin, cos) = arc.rotation.sin_cos();
        Conjugate { center: arc.center, u: Vec2::new(cos, sin) * arc.radii.x, v: Vec2::new(-sin, cos) * arc.radii.y }
    }

    fn point(&self, t: f64) -> Point {
        self.center + self.u * t.cos() + self.v * t.sin()
    }

    /// The derivative of [`Conjugate::point`] by `t`.
    fn tangent(&self, t: f64) -> Vec2 {
        self.v * t.cos() - self.u * t.sin()
    }

    fn size(&self) -> f64 {
        self.u.hypot().max(self.v.hypot())
    }
}

/// How many radians one cubic of an arc of this size may cover to stay within [`CURVE_TOLERANCE`].
fn piece_step(size: f64) -> f64 {
    let size = size.max(f64::MIN_POSITIVE);
    let share = (CURVE_TOLERANCE / (QUARTER_ERROR * size)).powf(1.0 / 6.0);
    (FRAC_PI_2 * share).clamp(MIN_STEP, FRAC_PI_2)
}

/// The cubic Beziers of the arc of `shape` from parameter `start_angle` through `sweep` radians. The first piece leaves `first` (the
/// pen) and the last arrives exactly at `last`, so that rows join with no gap. `None` for an arc whose numbers are not finite: the caller draws a line and
/// says so.
pub(crate) fn arc_segments(shape: &Conjugate, start_angle: f64, sweep: f64, first: Point, last: Point) -> Option<Vec<Seg>> {
    if !(sweep.is_finite() && start_angle.is_finite() && shape.size().is_finite()) {
        return None;
    }
    // A finite sweep is at most a few turns and the step is at least a degree, so the count is bounded (an arc is under 2 pi).
    let count = ((sweep.abs() / piece_step(shape.size())).ceil() as usize).clamp(1, 4096);
    let delta = sweep / count as f64;
    let arm = 4.0 / 3.0 * (delta / 4.0).tan();
    let mut out = Vec::with_capacity(count);
    let mut from = first;
    for i in 0..count {
        let t0 = start_angle + delta * i as f64;
        let t1 = t0 + delta;
        let to = if i + 1 == count { last } else { shape.point(t1) };
        let c1 = from + shape.tangent(t0) * arm;
        let c2 = to - shape.tangent(t1) * arm;
        out.push(Seg::Cubic(c1, c2, to));
        from = to;
    }
    Some(out)
}
