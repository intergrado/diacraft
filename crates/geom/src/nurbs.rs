//! Non-uniform rational B-splines: the `NURBSTo` row and the `SplineStart` run.
//!
//! A spline with `n` control points lists `n` + 1 knots where the usual form has `n` + `degree` + 1, and the missing `degree` knots are
//! the last one repeated. The curve is the textbook one over the knots from `u[degree]` to `u[n]`. It is turned into cubic
//! Beziers span by span: exactly for a degree of 2 or 3 with equal weights, and otherwise by Hermite pieces that are cut in two until
//! they are within [`crate::CURVE_TOLERANCE`] of the curve at the middle of their parameter range and at `degree - 3` more points
//! spread over it (`degree` for a rational curve). That count is the least that cannot be fooled: a cubic through a piece's ends and
//! their slopes differs from a polynomial curve of degree `d` by `u^2 (1-u)^2 q(u)` with `q` of degree `d - 4`, which has at most
//! `d - 4` roots, so a nonzero error is seen at one of `d - 3` points; a rational curve's numerator has degree `d + 3`, so `d` points.
//! It is a check at points and not a bound: an error that is under the tolerance at every point can be over it between them. Where the
//! cutting stops (`MAX_FIT_DEPTH`) short of the tolerance, the curve is still drawn and `segments` says it did not reach it, so the
//! row can carry a warning.

use kurbo::{Point, Size, Vec2};

use crate::row::{GeomRow, MAX_CONSTRUCTOR_POINTS};
use crate::rules;
use crate::seg::Seg;

/// The most control points a spline may hold: a constructor's tuples, with the pen and the end.
const MAX_CURVE_POINTS: usize = MAX_CONSTRUCTOR_POINTS + 2;

/// A span of a rational spline, or of one over degree 3, is cut in two at most this many times in a row: 1,024 Hermite cubics. (The error of a Hermite cubic
/// falls with the fourth power of the piece, so a rational cubic with weights of 4 needs pieces down to a 512th of the span to be within 1E-7 where it curves most,
/// and a degree 5 curve with controls 50 apart about a 256th. The pieces that are not needed are not made: a piece is cut only where it strays.)
const MAX_FIT_DEPTH: u32 = 10;

/// What `segments` made: the cubics, and whether every piece was within [`crate::CURVE_TOLERANCE`] of the curve where it was checked.
pub(crate) struct Fitted {
    pub(crate) segs: Vec<Seg>,
    /// False where `MAX_FIT_DEPTH` stopped the cutting short of the tolerance (a degree 25 curve with wild controls, say).
    pub(crate) within_tolerance: bool,
}

/// The state of one `segments` call: the work left, the cubics so far, whether they are all within tolerance, and how many points besides the middle
/// each piece is checked at.
struct Fit<'a> {
    budget: &'a mut u64,
    segs: Vec<Seg>,
    within_tolerance: bool,
    checks: usize,
}

/// How many interior points a Hermite cubic is checked at, besides the middle, for a spline of this degree (see the module docs): `degree - 3` for a polynomial curve
/// (at least 1) and `degree` for a rational one.
fn checks_for(degree: usize, rational: bool) -> usize {
    if rational { degree } else { degree.saturating_sub(3).max(1) }
}

/// The work one section may spend on splines, in multiplications (one evaluation of a degree 3 curve costs about 32). It is far past a
/// section's real splines and bounds a hostile one.
pub(crate) const MAX_CURVE_WORK: u64 = 200_000_000;

/// A point of a spline with its derivative by the parameter `t`.
#[derive(Clone, Copy)]
struct Sample {
    t: f64,
    point: Point,
    slope: Vec2,
}

/// Why a spline cannot be drawn (no usable constructor, a weight of 0 or less, knots that fall, or control points no more
/// than the degree; and the hostile cases).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CurveProblem {
    /// The rows given are not a `NURBSTo` row, or not a `SplineStart` followed by `SplineKnot` rows.
    NotASplineRow,
    /// The `NURBSTo` row has no readable `E`.
    NoConstructor,
    /// The degree is not a whole number from 1 to 25 (a degree of 0 is no curve).
    BadDegree,
    /// The control points are no more than the degree.
    TooFewPoints,
    /// The weights or knots are not as many as the points need.
    CountMismatch,
    /// A weight is not a number above 0.
    BadWeight,
    /// The knots fall by more than rounding.
    KnotsFall,
    /// The first and last knots of the domain are equal: nothing to draw.
    EmptyDomain,
    /// More control points than a constructor may hold.
    TooLarge,
    /// A point or a knot is not a finite number.
    NotFinite,
    /// The section has spent the work it may on splines.
    TooMuchWork,
}

impl CurveProblem {
    /// The reason in words, for a warning.
    pub fn reason(self) -> &'static str {
        match self {
            CurveProblem::NotASplineRow => "the rows are not a spline",
            CurveProblem::NoConstructor => "the row has no readable NURBS constructor",
            CurveProblem::BadDegree => "the degree is not a whole number from 1 to 25",
            CurveProblem::TooFewPoints => "there are no more control points than the degree",
            CurveProblem::CountMismatch => "the weights or knots are not as many as the points need",
            CurveProblem::BadWeight => "a weight is not a number above 0",
            CurveProblem::KnotsFall => "the knots fall",
            CurveProblem::EmptyDomain => "the knots span nothing",
            CurveProblem::TooLarge => "there are too many control points",
            CurveProblem::NotFinite => "a point or a knot is not a finite number",
            CurveProblem::TooMuchWork => "the section has spent the work it may on splines",
        }
    }
}

/// A spline: its degree, control points, weights and the full knot vector of `points.len() + degree + 1` knots. The fields are private: only
/// [`NurbsCurve::new`] and the two constructors that go through it make one, so the knots never fall and the counts always agree (review finding 2: with public
/// fields a caller could build a curve whose domain has its low end above its high end).
#[derive(Clone, Debug, PartialEq)]
pub struct NurbsCurve {
    degree: usize,
    points: Vec<Point>,
    weights: Vec<f64>,
    /// The listed knots, then the last one repeated `degree` times.
    knots: Vec<f64>,
}

/// `v[i]`, or 0 when there is none: the callers keep `i` in range, and an index that is not cannot panic.
fn at(v: &[f64], i: usize) -> f64 {
    v.get(i).copied().unwrap_or(0.0)
}

fn set(v: &mut [f64], i: usize, value: f64) {
    if let Some(slot) = v.get_mut(i) {
        *slot = value;
    }
}

/// `a / d`, or 0 where the knots at both ends of the span are the same (the term of a repeated knot is 0).
fn ratio(a: f64, d: f64) -> f64 {
    if d > 0.0 { a / d } else { 0.0 }
}

/// The degree as a whole number from 1 to `rules::MAX_DEGREE`.
fn degree_of(degree: f64) -> Result<usize, CurveProblem> {
    let rounded = degree.round();
    let whole = degree.is_finite() && (degree - rounded).abs() <= 1e-9;
    if whole && rounded >= 1.0 && rounded <= rules::MAX_DEGREE as f64 { Ok(rounded as usize) } else { Err(CurveProblem::BadDegree) }
}

/// The listed knots with the last repeated `degree` times. A knot that falls by no more than rounding is taken as equal to the one
/// before (repeated knots are written with different digits in two cells).
fn padded_knots(listed: &[f64], degree: usize) -> Result<Vec<f64>, CurveProblem> {
    let mut knots: Vec<f64> = Vec::with_capacity(listed.len() + degree);
    for &k in listed {
        if !k.is_finite() {
            return Err(CurveProblem::NotFinite);
        }
        match knots.last().copied() {
            Some(prev) if k < prev => {
                if prev - k <= rules::KNOT_ROUNDING * prev.abs().max(1.0) {
                    knots.push(prev);
                } else {
                    return Err(CurveProblem::KnotsFall);
                }
            }
            _ => knots.push(k),
        }
    }
    let last = knots.last().copied().ok_or(CurveProblem::CountMismatch)?;
    knots.extend(std::iter::repeat_n(last, degree));
    Ok(knots)
}

/// The values of the `deg` + 1 basis functions of degree `deg` that are not 0 on the span `s`, at `t` (the triangular scheme). `s` must be
/// at least `deg` and the knots must reach `s` + `deg`; an index out of range reads 0, so a wrong span gives wrong numbers and no panic.
fn basis(knots: &[f64], s: usize, deg: usize, t: f64) -> Vec<f64> {
    let mut n = vec![0.0; deg + 1];
    let mut left = vec![0.0; deg + 1];
    let mut right = vec![0.0; deg + 1];
    set(&mut n, 0, 1.0);
    for j in 1..=deg {
        set(&mut left, j, t - at(knots, (s + 1).saturating_sub(j)));
        set(&mut right, j, at(knots, s + j) - t);
        let mut saved = 0.0;
        for r in 0..j {
            let denominator = at(&right, r + 1) + at(&left, j - r);
            let temp = if denominator != 0.0 { at(&n, r) / denominator } else { 0.0 };
            set(&mut n, r, saved + at(&right, r + 1) * temp);
            saved = at(&left, j - r) * temp;
        }
        set(&mut n, j, saved);
    }
    n
}

impl NurbsCurve {
    /// A spline from its degree (as a number: it must be whole, 1 to 25), control points, weights and the listed knots (one more than the
    /// points). The last knot is repeated `degree` times and a fall within rounding is equal.
    pub fn new(degree: f64, points: Vec<Point>, weights: Vec<f64>, listed_knots: &[f64]) -> Result<NurbsCurve, CurveProblem> {
        let degree = degree_of(degree)?;
        let n = points.len();
        if n > MAX_CURVE_POINTS || listed_knots.len() > MAX_CURVE_POINTS + 1 {
            return Err(CurveProblem::TooLarge);
        }
        if weights.len() != n || listed_knots.len() != n + 1 {
            return Err(CurveProblem::CountMismatch);
        }
        if n <= degree {
            return Err(CurveProblem::TooFewPoints);
        }
        if !points.iter().all(|p| p.is_finite()) {
            return Err(CurveProblem::NotFinite);
        }
        if !weights.iter().all(|w| w.is_finite() && *w > 0.0) {
            return Err(CurveProblem::BadWeight);
        }
        let knots = padded_knots(listed_knots, degree)?;
        let curve = NurbsCurve { degree, points, weights, knots };
        let (low, high) = curve.domain();
        if high > low { Ok(curve) } else { Err(CurveProblem::EmptyDomain) }
    }

    /// The spline of a `NURBSTo` row after the pen `pen`: the control points are the pen, each tuple's point (scaled by
    /// `size` where its type is 0), then (`X`, `Y`); the weights `D`, the tuples', `B`; the knots `C`, the tuples', `A` and the
    /// constructor's last knot.
    pub fn from_nurbs_to(pen: Point, size: Size, row: &GeomRow) -> Result<NurbsCurve, CurveProblem> {
        let GeomRow::NurbsTo { x, y, second_last_knot, last_weight, first_knot, first_weight, nurbs } = row else {
            return Err(CurveProblem::NotASplineRow);
        };
        let nurbs = nurbs.as_ref().ok_or(CurveProblem::NoConstructor)?;
        if nurbs.points.len() > MAX_CONSTRUCTOR_POINTS {
            return Err(CurveProblem::TooLarge);
        }
        let mut points = vec![pen];
        let mut weights = vec![*first_weight];
        let mut listed = vec![*first_knot];
        for tuple in &nurbs.points {
            let px = if nurbs.x_is_share { tuple.x * size.width } else { tuple.x };
            let py = if nurbs.y_is_share { tuple.y * size.height } else { tuple.y };
            points.push(Point::new(px, py));
            weights.push(tuple.weight);
            listed.push(tuple.knot);
        }
        points.push(Point::new(*x, *y));
        weights.push(*last_weight);
        listed.push(*second_last_knot);
        listed.push(nurbs.last_knot);
        NurbsCurve::new(nurbs.degree, points, weights, &listed)
    }

    /// The spline of a run of rows after the pen `pen`: the first row is a `SplineStart` and the rest are `SplineKnot`
    /// rows. The control points are the pen, the start's (`X`, `Y`) and each knot's; the listed knots are `B`, `A`, each knot's `A` and `C`;
    /// the degree is `D`; the weights are all 1.
    pub fn from_spline(pen: Point, rows: &[GeomRow]) -> Result<NurbsCurve, CurveProblem> {
        let (first, rest) = rows.split_first().ok_or(CurveProblem::NotASplineRow)?;
        let GeomRow::SplineStart { x, y, second_knot, first_knot, last_knot, degree } = first else { return Err(CurveProblem::NotASplineRow) };
        if rest.len() > MAX_CURVE_POINTS {
            return Err(CurveProblem::TooLarge);
        }
        let mut points = vec![pen, Point::new(*x, *y)];
        let mut listed = vec![*first_knot, *second_knot];
        for row in rest {
            let GeomRow::SplineKnot { x, y, knot } = row else { return Err(CurveProblem::NotASplineRow) };
            points.push(Point::new(*x, *y));
            listed.push(*knot);
        }
        listed.push(*last_knot);
        let weights = vec![1.0; points.len()];
        NurbsCurve::new(*degree, points, weights, &listed)
    }

    /// The degree, from 1 to 25.
    pub fn degree(&self) -> usize {
        self.degree
    }

    /// The control points: the pen, the middle ones, and the end.
    pub fn points(&self) -> &[Point] {
        &self.points
    }

    /// The weights, one for each control point, all above 0.
    pub fn weights(&self) -> &[f64] {
        &self.weights
    }

    /// The full knot vector: the listed knots, then the last one repeated `degree` times. It never falls.
    pub fn knots(&self) -> &[f64] {
        &self.knots
    }

    /// True if the structure holds together: the degree is at least 1, there are more points than the degree, and the weights and
    /// knots are as many as they need. [`NurbsCurve::new`] makes only such curves; the methods check all the same, as a defence against
    /// a bug in this module (the test in this file builds a curve that does not hold together).
    fn is_consistent(&self) -> bool {
        let n = self.points.len();
        self.degree >= 1 && n > self.degree && self.weights.len() == n && self.knots.len() == n + self.degree + 1
    }

    /// The first and last parameter the curve is defined over: `u[degree]` and `u[n]`. (0, 0) for a curve that is not consistent.
    pub fn domain(&self) -> (f64, f64) {
        if self.is_consistent() { (at(&self.knots, self.degree), at(&self.knots, self.points.len())) } else { (0.0, 0.0) }
    }

    /// True if the first knot is repeated `degree` + 1 times, so the curve starts on its first control point. Otherwise the
    /// spline is periodic and does not start on the pen.
    pub fn is_clamped(&self) -> bool {
        let first = at(&self.knots, 0);
        self.is_consistent() && (at(&self.knots, self.degree) - first).abs() <= rules::KNOT_ROUNDING * first.abs().max(1.0)
    }

    /// The spans `s` of the knot vector (`degree` to `n` - 1) that cover some parameter: where `u[s]` is below `u[s + 1]`.
    fn spans(&self) -> Vec<usize> {
        if !self.is_consistent() {
            return Vec::new();
        }
        (self.degree..self.points.len()).filter(|s| at(&self.knots, *s) < at(&self.knots, *s + 1)).collect()
    }

    /// The point of the curve at parameter `t`, clamped to the domain. The origin for a curve that is not consistent.
    pub fn eval(&self, t: f64) -> Point {
        let (low, high) = self.domain();
        // Not `f64::clamp`, which panics when `low` is above `high` or either is NaN: `max` and `min` give a number for any input.
        let t = if t.is_finite() { t.max(low).min(high) } else { low };
        let spans = self.spans();
        // The first span that ends after `t`, or the last one when `t` is the end of the domain.
        let span = spans.iter().copied().find(|s| t < at(&self.knots, *s + 1)).or_else(|| spans.last().copied());
        span.and_then(|s| self.eval_span(s, t)).map_or(Point::ORIGIN, |(point, _)| point)
    }

    /// The point and the derivative by `t` of the curve at `t`, using the polynomial piece of span `s` (so a span's two ends are both
    /// reached from inside it). `None` if the weights make no sense or an index is missing.
    fn eval_span(&self, s: usize, t: f64) -> Option<(Point, Vec2)> {
        let p = self.degree;
        let high = basis(&self.knots, s, p, t);
        let low = basis(&self.knots, s, p.saturating_sub(1), t);
        let (mut numerator, mut denominator) = (Vec2::ZERO, 0.0);
        let (mut d_numerator, mut d_denominator) = (Vec2::ZERO, 0.0);
        for j in 0..=p {
            let i = (s + j).checked_sub(p)?;
            let weight = *self.weights.get(i)?;
            let point = self.points.get(i)?.to_vec2();
            let n = at(&high, j) * weight;
            numerator += point * n;
            denominator += n;
            // N'(i, p) = p (N(i, p-1) / (u[i+p] - u[i]) - N(i+1, p-1) / (u[i+p+1] - u[i+1])).
            let from_left = if j >= 1 { at(&low, j - 1) } else { 0.0 };
            let from_right = if j < p { at(&low, j) } else { 0.0 };
            let derivative = p as f64
                * (ratio(from_left, at(&self.knots, i + p) - at(&self.knots, i))
                    - ratio(from_right, at(&self.knots, i + p + 1) - at(&self.knots, i + 1)));
            d_numerator += point * (derivative * weight);
            d_denominator += derivative * weight;
        }
        if !(denominator > 0.0 && denominator.is_finite()) {
            return None;
        }
        let curve = numerator / denominator;
        let slope = (d_numerator - curve * d_denominator) / denominator;
        Some((curve.to_point(), slope))
    }

    /// The curve as segments from its start: each span is one cubic Bezier (exact for a degree of 2 or 3 with equal weights) or, for a
    /// rational spline or a degree above 3, Hermite cubics cut until they are within [`crate::CURVE_TOLERANCE`] of the curve at three
    /// points each; a degree 1 span is a straight line. The first segment starts at the curve's start, which is the pen for a clamped
    /// spline. `Fitted::within_tolerance` is false where the cutting stopped short. `budget` is the work left to the caller, in units
    /// of one multiplication; the curve takes what it uses and gives up with `TooMuchWork` when it is not enough.
    pub(crate) fn segments(&self, budget: &mut u64) -> Result<Fitted, CurveProblem> {
        let first_weight = self.weights.first().copied().unwrap_or(1.0);
        let rational = self.weights.iter().any(|w| (w - first_weight).abs() > 1e-12 * first_weight.abs());
        let exact = !rational && self.degree <= 3;
        let mut fit = Fit { budget, segs: Vec::new(), within_tolerance: true, checks: checks_for(self.degree, rational) };
        for s in self.spans() {
            let start = self.sample(s, at(&self.knots, s), fit.budget)?;
            let end = self.sample(s, at(&self.knots, s + 1), fit.budget)?;
            if self.degree == 1 {
                fit.segs.push(Seg::Line(end.point));
            } else {
                self.fit(s, start, end, if exact { None } else { Some(0) }, &mut fit)?;
            }
        }
        Ok(Fitted { segs: fit.segs, within_tolerance: fit.within_tolerance })
    }

    /// The point and derivative of the curve at `t` in span `s`, paid for out of `budget`.
    fn sample(&self, s: usize, t: f64, budget: &mut u64) -> Result<Sample, CurveProblem> {
        let cost = 2 * (self.degree as u64 + 1).pow(2);
        *budget = budget.checked_sub(cost).ok_or(CurveProblem::TooMuchWork)?;
        let (point, slope) = self.eval_span(s, t).ok_or(CurveProblem::NotFinite)?;
        Ok(Sample { t, point, slope })
    }

    /// Adds the Hermite cubic through two samples of span `s`: its controls lie a third of the parameter step along each slope, which is
    /// the curve itself for a polynomial of degree 3 or less. With `depth` given, the cubic is checked against the curve at the middle of
    /// its parameter range, and then at `fit.checks` points `k / (checks + 1)` (an error that is antisymmetric about the middle is 0 there,
    /// and a degree 5 S curve passed a check at the middle alone; the count is in the module docs), and cut in two where it strays more
    /// than the tolerance, down to `MAX_FIT_DEPTH` cuts. A piece that is kept past that depth, still too far off, clears `within_tolerance`.
    fn fit(&self, s: usize, from: Sample, to: Sample, depth: Option<u32>, fit: &mut Fit<'_>) -> Result<(), CurveProblem> {
        let third = (to.t - from.t) / 3.0;
        let (c1, c2) = (from.point + from.slope * third, to.point - to.slope * third);
        let Some(depth) = depth else {
            fit.segs.push(Seg::Cubic(c1, c2, to.point));
            return Ok(());
        };
        // The cubic at its parameter `u`, by the Bernstein weights.
        let cubic = |u: f64| -> Point {
            let v = 1.0 - u;
            (from.point.to_vec2() * (v * v * v)
                + c1.to_vec2() * (3.0 * u * v * v)
                + c2.to_vec2() * (3.0 * u * u * v)
                + to.point.to_vec2() * (u * u * u))
                .to_point()
        };
        let middle = self.sample(s, (from.t + to.t) / 2.0, fit.budget)?;
        let mut error = cubic(0.5).distance(middle.point);
        for k in 1..=fit.checks {
            if error > crate::CURVE_TOLERANCE {
                break;
            }
            let u = k as f64 / (fit.checks + 1) as f64;
            if (u - 0.5).abs() < 1e-12 {
                continue; // the middle, which is sampled already
            }
            let at_u = self.sample(s, from.t + (to.t - from.t) * u, fit.budget)?;
            error = error.max(cubic(u).distance(at_u.point));
        }
        if error <= crate::CURVE_TOLERANCE || depth >= MAX_FIT_DEPTH {
            fit.within_tolerance &= error <= crate::CURVE_TOLERANCE;
            fit.segs.push(Seg::Cubic(c1, c2, to.point));
            return Ok(());
        }
        self.fit(s, from, middle, Some(depth + 1), fit)?;
        self.fit(s, middle, to, Some(depth + 1), fit)
    }

    /// Where the curve starts: the point at the start of its domain.
    pub(crate) fn start(&self) -> Point {
        self.eval(self.domain().0)
    }
}

/// The one test that has to live in the crate: it builds a `NurbsCurve` that `NurbsCurve::new` would refuse (knots that fall, or are not numbers), as a defence
/// against a bug in this module, and the fields are private outside it. (Review finding 2: `t.clamp(low, high)` panics when `low` is above `high` or either
/// is NaN, in shipped code, and WebAssembly has no guard.)
#[cfg(test)]
mod tests {
    use super::*;

    fn curve_with_knots(knots: &[f64]) -> NurbsCurve {
        NurbsCurve {
            degree: 2,
            points: vec![Point::new(0.0, 0.0), Point::new(1.0, 2.0), Point::new(2.0, 0.0)],
            weights: vec![1.0; 3],
            knots: knots.to_vec(),
        }
    }

    #[test]
    fn a_curve_with_knots_that_fall_or_are_not_numbers_does_not_panic() {
        for knots in [
            [3.0, 2.0, 1.0, 0.0, -1.0, -2.0],
            [0.0, 0.0, 5.0, 4.0, 4.0, 4.0],
            [f64::NAN, 0.0, 0.0, 1.0, 1.0, 1.0],
            [0.0, 0.0, f64::NAN, f64::NAN, 1.0, 1.0],
            [0.0, 0.0, 0.0, f64::INFINITY, f64::INFINITY, f64::INFINITY],
            [f64::NEG_INFINITY, 0.0, 1.0, f64::INFINITY, 1.0, 0.0],
        ] {
            let curve = curve_with_knots(&knots);
            for t in [0.0, 0.5, 1.0, -7.0, 1e300, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                let _ = curve.eval(t);
            }
            let _ = curve.domain();
            let _ = curve.start();
            let _ = curve.is_clamped();
            let mut budget = MAX_CURVE_WORK;
            let _ = curve.segments(&mut budget);
        }
    }
}
