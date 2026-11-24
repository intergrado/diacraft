//! Object snaps on kurbo paths: the points a pointer or a dragged shape can be pulled to on someone else's outline.
//!
//! **Where it comes from.** Ported from the object snaps of CADCraft, `storytold/cadcraft` at commit
//! `ba3196e3f737f2fed0d4d8ae9c065b20773dec16` (`crates/engine/src/snap.rs` and `crates/engine/src/snap/curves.rs`; MIT OR Apache-2.0, the owner's own organization; the
//! row is in `docs/provenance.md`). CADCraft's snaps are on its own `Vec2`, `Segment` (lines and arcs), `Ellipse` and `Spline`; here every curve is a
//! kurbo [`BezPath`], which `diacraft_geom` builds a shape's rows into, so the same functions serve a line, an arc, an ellipse and a NURBS.
//!
//! * **Endpoint** (`END`): the start and the end of every segment ([`endpoints`]).
//! * **Midpoint** (`MID`): the point halfway along a segment's length ([`midpoints`]; CADCraft's `Param::mid`, by arc length).
//! * **Intersection** (`INT`): where two outlines cross, on the pieces near the cursor ([`intersections`]; CADCraft's pass over `near_prims` with the finite-line
//!   slack of `intersect_ext` and the probe that leaves out pieces that cannot reach the aperture). Curves are flattened first, as CADCraft flattens an
//!   ellipse or a spline.
//! * **Nearest** (`NEA`): the closest point of an outline to the cursor ([`nearest`]).
//! * **Tangent** (`TAN`) and **perpendicular** (`PER`) from a base point ([`tangents`], [`perpendiculars`]; CADCraft's `Param::solve`: the samples of the
//!   condition, each sign change narrowed by bisection, kept when the normalised condition is nearly zero there). They need a base point, which a drag of a shape
//!   has not, so nothing in DiaCraft asks for them yet.
//! * [`best`]: the nearest candidate within the aperture, where an endpoint or an intersection wins a near tie by 15% of the aperture (CADCraft's `snap_score`).
//!
//! Not ported: quadrant, centre, node, insertion, extension and parallel (they need a circle, an ellipse or an object kind a kurbo path has lost; the reference app's equivalents
//! are extension lines, P2), hatches, blocks and the deferred snaps. **This module knows nothing of DiaCraft's document**: its only types are kurbo's, and it
//! reads no cell, so it can move into a shared crate. The reference app's rules (which options, which shapes, `NoSnap`, `InhibitSnap`) are in `snap/geometry.rs` and
//! `snap/mod.rs`.
//!
//! Every function is bounded: a path is read to at most [`MAX_SEGMENTS`] segments, a flattened side to [`MAX_FLAT`] pieces, and a result to a few dozen points.
//!
//! The engine's object snaps are `craft-snap`'s, and this file is kept only for the oracle (`oracle.rs`) that the adapter is held equal to:
//! what the oracle does not call is dead.
#![allow(dead_code)]

use kurbo::{
    BezPath, CubicBez, Line, ParamCurve, ParamCurveArclen, ParamCurveDeriv, ParamCurveNearest, PathEl, PathSeg, Point, QuadBez, Rect, Vec2, flatten,
};

/// The most segments of one path any function here reads.
pub const MAX_SEGMENTS: usize = 4_096;

/// The most straight pieces of a flattened path that [`intersections`] works with, for each side.
pub const MAX_FLAT: usize = 256;

/// The most points a function returns.
pub const MAX_POINTS: usize = 64;

/// The farthest from the origin, in the path's own units, that a control point may lie for its segment to be read (paper inches in DiaCraft: a million is
/// fifteen miles). A segment with a control point past it, or that is not a number, is left out, and the answer says so (`Pieces::cut`).
pub const COORD_LIMIT: f64 = 1.0e6;

/// The most straight pieces one curved segment may be estimated to make (see [`estimated_pieces`]); a segment that would make more is left out before it is
/// flattened.
pub const MAX_SEGMENT_PIECES: usize = 4_096;

/// The most straight pieces, estimated, that one call to [`pieces_near`] may flatten curves into; the segments after that are left out.
pub const MAX_FLATTEN_PIECES: usize = 16_384;

/// How far a flattened curve may leave the curve, in the path's own units (paper inches in DiaCraft: a thousandth).
pub const FLATNESS: f64 = 1.0e-3;

/// The accuracy of arc lengths and nearest points, in the path's own units.
const ACCURACY: f64 = 1.0e-6;

/// Samples of a segment for [`tangents`] and [`perpendiculars`] (CADCraft uses 512 for a whole curve), bisection steps per root, and roots per segment.
const SAMPLES: usize = 64;
const BISECT: usize = 60;
const MAX_ROOTS: usize = 8;

/// The kind of a snap candidate, with CADCraft's names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Endpoint,
    Midpoint,
    Intersection,
    Nearest,
    Tangent,
    Perpendicular,
}

/// A point a pointer may be pulled to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Candidate {
    pub kind: Kind,
    pub at: Point,
}

/// The start and the end of every segment, a point once where one segment ends and the next begins.
pub fn endpoints(path: &BezPath) -> Vec<Point> {
    let mut out: Vec<Point> = Vec::new();
    for seg in path.segments().take(MAX_SEGMENTS) {
        for p in [seg.start(), seg.end()] {
            if p.is_finite() && out.last().is_none_or(|q| q.distance(p) > 1.0e-12) {
                out.push(p);
            }
        }
    }
    out
}

/// The point halfway along each segment, by length.
pub fn midpoints(path: &BezPath) -> Vec<Point> {
    let mut out = Vec::new();
    for seg in path.segments().take(MAX_SEGMENTS) {
        let t = match seg {
            PathSeg::Line(_) => 0.5,
            _ => {
                let length = seg.arclen(ACCURACY);
                if !(length.is_finite() && length > 0.0) {
                    continue;
                }
                seg.inv_arclen(length / 2.0, ACCURACY)
            }
        };
        let p = seg.eval(t);
        if p.is_finite() {
            out.push(p);
        }
    }
    out
}

/// The point of the path nearest `p`, and how far it is; `None` for a path with no segment or a `p` that is not a number.
pub fn nearest(path: &BezPath, p: Point) -> Option<(Point, f64)> {
    if !p.is_finite() {
        return None;
    }
    let mut best: Option<(Point, f64)> = None;
    for seg in path.segments().take(MAX_SEGMENTS) {
        // A segment with a control point past the limit is no drawing's (and a nearest point by iteration on it is no cheaper).
        let ends = match seg {
            PathSeg::Line(l) => vec![l.p0, l.p1],
            PathSeg::Quad(q) => vec![q.p0, q.p1, q.p2],
            PathSeg::Cubic(c) => vec![c.p0, c.p1, c.p2, c.p3],
        };
        if !control_box(&ends).1 {
            continue;
        }
        let n = seg.nearest(p, ACCURACY);
        if !n.distance_sq.is_finite() {
            continue;
        }
        if best.is_none_or(|(_, d2)| n.distance_sq < d2) {
            best = Some((seg.eval(n.t), n.distance_sq));
        }
    }
    best.map(|(q, d2)| (q, d2.sqrt()))
}

/// The straight pieces of a path that can reach a probe.
#[derive(Clone, Debug, Default)]
pub struct Pieces {
    /// The pieces, at most [`MAX_FLAT`].
    pub lines: Vec<Line>,
    /// Whether something was left out for the work it would cost, and not because it is out of reach: a segment past [`COORD_LIMIT`], a curve that would make
    /// more than [`MAX_SEGMENT_PIECES`] pieces, the estimated pieces past [`MAX_FLATTEN_PIECES`], or more pieces than [`MAX_FLAT`].
    pub cut: bool,
    /// The pieces the curves were estimated to make, which is what the call was charged: the work counted before any curve was flattened.
    pub charged: usize,
    /// The pieces the curves did make, before the probe left any out (never more than `charged`).
    pub made: usize,
}

/// An upper bound on the pieces [`pieces_near`] flattens a curve into, from the diagonal of the box its control points lie in, worked out before the curve is
/// flattened. A cubic or a quadratic differs from its chords by at most an eighth of the square of the step of its parameter times its second derivative,
/// which a control box of diagonal `size` bounds by `12 size`: within [`FLATNESS`] that is at most `1.23 sqrt(size / FLATNESS)` pieces, and `kurbo`'s flattening
/// makes up to about twice the fewest, so the estimate is `2 sqrt(size / FLATNESS) + 2`. A `size` that is not a number gives `usize::MAX`.
pub fn estimated_pieces(size: f64) -> usize {
    if !size.is_finite() || size < 0.0 {
        return usize::MAX;
    }
    let n = 2.0 * (size / FLATNESS).sqrt() + 2.0;
    if n >= usize::MAX as f64 { usize::MAX } else { n.ceil() as usize }
}

/// The box round some points, and whether every number in them is a finite number within [`COORD_LIMIT`].
fn control_box(points: &[Point]) -> (Rect, bool) {
    let mut r = Rect::new(f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    let mut sane = true;
    for p in points {
        sane &= p.x.is_finite() && p.y.is_finite() && p.x.abs() <= COORD_LIMIT && p.y.abs() <= COORD_LIMIT;
        r = Rect::new(r.x0.min(p.x), r.y0.min(p.y), r.x1.max(p.x), r.y1.max(p.y));
    }
    (r, sane)
}

/// Whether a box padded by `slack` can meet the probe (no probe: always).
fn reaches(probe: Option<&Rect>, x0: f64, y0: f64, x1: f64, y1: f64, slack: f64) -> bool {
    probe.is_none_or(|probe| !(x1 + slack < probe.x0 || probe.x1 < x0 - slack || y1 + slack < probe.y0 || probe.y1 < y0 - slack))
}

/// The straight pieces of a path, curves flattened to within [`FLATNESS`]; the pieces whose box (padded by the slack of a finite crossing) cannot meet
/// `probe` are left out, and at most [`MAX_FLAT`] are kept.
///
/// **The work is bounded before it is done.** A curve is flattened into a number of pieces that grows with its size (`sqrt(size / FLATNESS)`), so for each
/// curved segment the box of its control points is looked at first, and the segment is left out, without being flattened, when a control point is past
/// [`COORD_LIMIT`] or not a number, when the box cannot meet the probe (a curve lies inside its control box), or when [`estimated_pieces`] of the box is over
/// [`MAX_SEGMENT_PIECES`] or would take the estimates over `limit` (the caller's: what is left of its own total, and never more than [`MAX_FLATTEN_PIECES`]). What was left out for its cost is `cut`; what was charged is `charged`.
pub fn pieces_near(path: &BezPath, probe: Option<&Rect>, limit: usize) -> Pieces {
    let mut out = Pieces::default();
    let limit = limit.min(MAX_FLATTEN_PIECES);
    let (mut start, mut at) = (None::<Point>, None::<Point>);
    for el in path.elements().iter().take(MAX_SEGMENTS * 3).copied() {
        match el {
            PathEl::MoveTo(p) => {
                start = Some(p);
                at = Some(p);
            }
            PathEl::LineTo(p) => {
                if let Some(a) = at {
                    push_line(a, p, probe, &mut out);
                }
                at = Some(p);
            }
            PathEl::ClosePath => {
                if let (Some(a), Some(s)) = (at, start) {
                    push_line(a, s, probe, &mut out);
                }
                at = start;
            }
            PathEl::QuadTo(c, p) => {
                if let Some(a) = at {
                    flatten_segment(&[a, c, p], el, probe, limit, &mut out);
                }
                at = Some(p);
            }
            PathEl::CurveTo(c1, c2, p) => {
                if let Some(a) = at {
                    flatten_segment(&[a, c1, c2, p], el, probe, limit, &mut out);
                }
                at = Some(p);
            }
        }
    }
    out
}

/// One straight piece, kept if it can reach the probe and there is room.
fn push_line(a: Point, b: Point, probe: Option<&Rect>, out: &mut Pieces) {
    if !(a.is_finite() && b.is_finite()) {
        return;
    }
    if a.x.abs().max(a.y.abs()).max(b.x.abs()).max(b.y.abs()) > COORD_LIMIT {
        out.cut = true;
        return;
    }
    let slack = 1.0e-9 * (1.0 + a.x.abs().max(a.y.abs()).max(b.x.abs()).max(b.y.abs()));
    if !reaches(probe, a.x.min(b.x), a.y.min(b.y), a.x.max(b.x), a.y.max(b.y), slack) {
        return;
    }
    if out.lines.len() >= MAX_FLAT {
        out.cut = true;
        return;
    }
    out.lines.push(Line::new(a, b));
}

/// One curved segment with the control points `points` (`el` is its element, from the first point): flattened if its box is sane, can reach the probe and
/// is not too costly (see [`pieces_near`]).
fn flatten_segment(points: &[Point], el: PathEl, probe: Option<&Rect>, limit: usize, out: &mut Pieces) {
    let (bounds, sane) = control_box(points);
    if !sane {
        out.cut = true;
        return;
    }
    if !reaches(probe, bounds.x0, bounds.y0, bounds.x1, bounds.y1, 1.0e-9 * (1.0 + bounds.x1.abs().max(bounds.y1.abs()))) {
        return;
    }
    let estimate = estimated_pieces(bounds.width().hypot(bounds.height()));
    if estimate > MAX_SEGMENT_PIECES || out.charged.saturating_add(estimate) > limit {
        out.cut = true;
        return;
    }
    out.charged += estimate;
    let Some(first) = points.first().copied() else { return };
    let mut last = first;
    let mut lines: Vec<(Point, Point)> = Vec::new();
    flatten([PathEl::MoveTo(first), el], FLATNESS, |piece| match piece {
        PathEl::MoveTo(p) => last = p,
        PathEl::LineTo(p) => {
            out.made += 1;
            // Pieces beyond the estimate are not kept: the estimate is an upper bound, and what is over it is not looked at.
            if out.made <= out.charged && lines.len() < MAX_FLAT {
                lines.push((last, p));
            }
            last = p;
        }
        _ => {}
    });
    for (a, b) in lines {
        push_line(a, b, probe, out);
    }
}

/// Where two finite straight pieces cross, ends included, with the slack of a ten-billionth that CADCraft's `intersect_ext` gives a finite line.
fn crossing(a: &Line, b: &Line) -> Option<Point> {
    let (r, s) = (a.p1 - a.p0, b.p1 - b.p0);
    let denom = r.cross(s);
    let scale = (r.hypot() * s.hypot()).max(f64::MIN_POSITIVE);
    if !denom.is_finite() || denom.abs() < 1.0e-12 * scale {
        return None;
    }
    let q = b.p0 - a.p0;
    let (t, u) = (q.cross(s) / denom, q.cross(r) / denom);
    let slack = 1.0e-9;
    ((-slack..=1.0 + slack).contains(&t) && (-slack..=1.0 + slack).contains(&u)).then(|| a.p0 + r * t)
}

/// The points where the outline `a` crosses the outline `b`. With a `probe` only the pieces that can reach it are looked at, and only crossings inside it
/// are given (the cursor's aperture, so that a long outline costs what is near the cursor and not its length).
pub fn intersections(a: &BezPath, b: &BezPath, probe: Option<Rect>) -> Vec<Point> {
    crossings(&pieces_near(a, probe.as_ref(), MAX_FLATTEN_PIECES).lines, &pieces_near(b, probe.as_ref(), MAX_FLATTEN_PIECES).lines, probe)
}

/// The points where the pieces `pa` cross the pieces `pb` ([`pieces_near`] of two outlines, flattened once for as many pairs as are asked about); with a
/// `probe`, only those inside it.
pub fn crossings(pa: &[Line], pb: &[Line], probe: Option<Rect>) -> Vec<Point> {
    let mut out: Vec<Point> = Vec::new();
    for x in pa {
        for y in pb {
            let Some(p) = crossing(x, y) else { continue };
            if probe.is_some_and(|r| p.x < r.x0 || p.x > r.x1 || p.y < r.y0 || p.y > r.y1) {
                continue;
            }
            if out.iter().all(|q| q.distance(p) > 1.0e-9) {
                out.push(p);
                if out.len() >= MAX_POINTS {
                    return out;
                }
            }
        }
    }
    out
}

/// The derivative of a segment at `t`.
fn derivative(seg: &PathSeg, t: f64) -> Vec2 {
    match seg {
        PathSeg::Line(l) => l.p1 - l.p0,
        PathSeg::Quad(q) => QuadBez::deriv(q).eval(t).to_vec2(),
        PathSeg::Cubic(c) => CubicBez::deriv(c).eval(t).to_vec2(),
    }
}

/// The points of the path where `cond(point - base, derivative)` is zero: the sign changes of the samples of each segment, narrowed by bisection, kept
/// when the normalised condition is nearly zero there, so that a flip at a kink or a pole is not taken for a solution (CADCraft's `Param::solve`).
fn solve(path: &BezPath, base: Point, cond: fn(Vec2, Vec2) -> f64, skip_lines: bool) -> Vec<Point> {
    let mut out: Vec<Point> = Vec::new();
    if !base.is_finite() {
        return out;
    }
    for seg in path.segments().take(MAX_SEGMENTS) {
        if skip_lines && matches!(seg, PathSeg::Line(_)) {
            continue;
        }
        let at = |t: f64| seg.eval(t);
        let g = |t: f64| cond(at(t) - base, derivative(&seg, t));
        let vals: Vec<f64> = (0..=SAMPLES).map(|i| g(i as f64 / SAMPLES as f64)).collect();
        // A segment on which every point qualifies (a circle seen from its centre: here a cubic that approximates it, within a thousandth of the
        // product of the lengths) has no particular solution.
        let scale = (0..=SAMPLES)
            .map(|i| {
                let t = i as f64 / SAMPLES as f64;
                (at(t) - base).hypot() * derivative(&seg, t).hypot()
            })
            .fold(0.0, f64::max);
        if !(scale.is_finite() && scale > 0.0) || vals.iter().all(|v| v.abs() <= scale * 1.0e-3) {
            continue;
        }
        let mut roots = 0;
        for i in 0..SAMPLES {
            let (Some(&ga), Some(&gb)) = (vals.get(i), vals.get(i + 1)) else { break };
            if !(ga.is_finite() && gb.is_finite()) || ((ga > 0.0) == (gb > 0.0) && ga != 0.0) {
                continue;
            }
            let (mut lo, mut hi, mut fa) = (i as f64 / SAMPLES as f64, (i + 1) as f64 / SAMPLES as f64, ga);
            for _ in 0..BISECT {
                let m = 0.5 * (lo + hi);
                let fm = g(m);
                if fm == 0.0 {
                    (lo, hi) = (m, m);
                    break;
                }
                if (fm > 0.0) == (fa > 0.0) {
                    (lo, fa) = (m, fm);
                } else {
                    hi = m;
                }
            }
            let t = 0.5 * (lo + hi);
            let (p, d) = (at(t), derivative(&seg, t));
            let (r, dl) = ((p - base).hypot(), d.hypot());
            if !(p.is_finite() && r > 1.0e-12 && dl > 0.0) || cond(p - base, d).abs() > 1.0e-6 * r * dl {
                continue;
            }
            if out.iter().all(|q| q.distance(p) > 1.0e-9 * (1.0 + r)) {
                out.push(p);
                roots += 1;
                if roots >= MAX_ROOTS || out.len() >= MAX_POINTS {
                    break;
                }
            }
        }
        if out.len() >= MAX_POINTS {
            break;
        }
    }
    out
}

/// The points of the path where the line from `base` touches it (the tangent through `base`). A straight segment has none.
pub fn tangents(path: &BezPath, base: Point) -> Vec<Point> {
    solve(path, base, |r, d| r.cross(d), true)
}

/// The feet of the perpendiculars from `base` to the path.
pub fn perpendiculars(path: &BezPath, base: Point) -> Vec<Point> {
    solve(path, base, |r, d| r.dot(d), false)
}

/// The score CADCraft ranks candidates by: the distance to the cursor, less 15% of the aperture for an endpoint or an intersection, so that the more
/// specific snap wins where two are nearly as near.
fn score(c: &Candidate, cursor: Point, aperture: f64) -> f64 {
    c.at.distance(cursor) - if matches!(c.kind, Kind::Intersection | Kind::Endpoint) { aperture * 0.15 } else { 0.0 }
}

/// The best candidate within `aperture` of `cursor`: the lowest [`score`] among the finite ones near enough.
pub fn best(candidates: &[Candidate], cursor: Point, aperture: f64) -> Option<Candidate> {
    candidates
        .iter()
        .filter(|c| c.at.is_finite() && c.at.distance(cursor) <= aperture)
        .min_by(|a, b| score(a, cursor, aperture).total_cmp(&score(b, cursor, aperture)))
        .copied()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use kurbo::{Circle, Shape};

    use super::*;

    fn square(x: f64, y: f64, side: f64) -> BezPath {
        let mut p = BezPath::new();
        p.move_to((x, y));
        p.line_to((x + side, y));
        p.line_to((x + side, y + side));
        p.line_to((x, y + side));
        p.close_path();
        p
    }

    fn circle(cx: f64, cy: f64, r: f64) -> BezPath {
        Circle::new((cx, cy), r).to_path(1.0e-6)
    }

    fn near(a: Point, b: (f64, f64)) -> bool {
        (a.x - b.0).abs() < 1.0e-6 && (a.y - b.1).abs() < 1.0e-6
    }

    /// One cubic from (0, 0) to (0, side) whose control points are `side` away, to the right and above: a box of `side` by `side`.
    fn cubic(side: f64) -> BezPath {
        let mut p = BezPath::new();
        p.move_to((0.0, 0.0));
        p.curve_to((side, 0.0), (side, side), (0.0, side));
        p
    }

    #[test]
    fn a_cubic_with_control_points_around_1e12_is_left_out_before_it_is_flattened() {
        // Flattened within a thousandth, such a curve is some 30 million pieces; it is refused from the box of its control points, and nothing is charged.
        let huge = cubic(1.0e12);
        for probe in [None, Some(Rect::new(4.0e11, 4.0e11, 6.0e11, 6.0e11))] {
            let found = pieces_near(&huge, probe.as_ref(), MAX_FLATTEN_PIECES);
            assert!(found.lines.is_empty() && found.cut, "{found:?}");
            assert_eq!((found.charged, found.made), (0, 0), "nothing was estimated or flattened");
        }
        assert!(intersections(&huge, &huge, None).is_empty());
        assert!(nearest(&huge, Point::new(5.0e11, 5.0e11)).is_none());
        // One that is under the limit of coordinates and over the pieces a segment may make is refused on its estimate.
        let big = cubic(COORD_LIMIT / 2.0);
        let found = pieces_near(&big, None, MAX_FLATTEN_PIECES);
        assert!(found.lines.is_empty() && found.cut && found.charged == 0 && found.made == 0, "{found:?}");
        // A quadratic and a line out of range, and a curve that is not a number.
        let mut mixed = BezPath::new();
        mixed.move_to((0.0, 0.0));
        mixed.quad_to((1.0e12, 1.0e12), (1.0, 1.0));
        mixed.line_to((2.0e12, 0.0));
        mixed.curve_to((f64::NAN, 0.0), (1.0, 1.0), (2.0, 2.0));
        let found = pieces_near(&mixed, None, MAX_FLATTEN_PIECES);
        assert!(found.lines.is_empty() && found.cut && found.charged == 0 && found.made == 0, "{found:?}");
    }

    #[test]
    fn a_curve_that_cannot_reach_the_probe_is_not_flattened() {
        // Its control box is a long way from the cursor's aperture: not cut (it is out of reach, not too costly), and not charged.
        let probe = Rect::new(900.0, 900.0, 901.0, 901.0);
        let found = pieces_near(&cubic(100.0), Some(&probe), MAX_FLATTEN_PIECES);
        assert!(found.lines.is_empty() && !found.cut && found.charged == 0 && found.made == 0, "{found:?}");
        // Near it, it is.
        // (The curve reaches x = 75 at the middle of its parameter, at y = 50.)
        let near_it = Rect::new(74.0, 49.0, 76.0, 51.0);
        let found = pieces_near(&cubic(100.0), Some(&near_it), MAX_FLATTEN_PIECES);
        assert!(!found.lines.is_empty() && found.charged > 0 && found.made > 0, "{found:?}");
    }

    #[test]
    fn the_estimate_of_the_pieces_of_a_curve_is_never_less_than_what_flattening_makes() {
        // Counted against kurbo's own flattening, over sizes from a thousandth to the limit of a segment.
        for side in [0.001, 0.01, 0.5, 1.0, 10.0, 100.0, 1_000.0, 2_500.0] {
            let found = pieces_near(&cubic(side), None, MAX_FLATTEN_PIECES);
            let estimate = estimated_pieces(side * std::f64::consts::SQRT_2);
            assert!(found.made > 0 && found.made <= found.charged && found.charged == estimate, "{side}: {found:?}");
            assert!(found.charged <= MAX_SEGMENT_PIECES, "{side}: {found:?}");
            // And the same for a loop with the controls crossed over, which curves harder.
            let mut p = BezPath::new();
            p.move_to((0.0, 0.0));
            p.curve_to((side, side), (0.0, side), (side, 0.0));
            let found = pieces_near(&p, None, MAX_FLATTEN_PIECES);
            assert!(found.made <= found.charged, "{side}: {found:?}");
        }
        // Past what a segment may make, it is left out and says so.
        let too_big = pieces_near(&cubic(4_000.0), None, MAX_FLATTEN_PIECES);
        assert!(too_big.cut && too_big.lines.is_empty() && too_big.charged == 0 && too_big.made == 0, "{too_big:?}");
        assert_eq!(estimated_pieces(f64::NAN), usize::MAX);
        assert_eq!(estimated_pieces(f64::INFINITY), usize::MAX);
        assert_eq!(estimated_pieces(-1.0), usize::MAX);
    }

    #[test]
    fn the_estimates_of_many_curves_are_held_to_one_total() {
        // 100 curves of 400 across, 1,300 pieces each by the estimate: the 13th that would take the total past the cap is the first left out.
        let mut p = BezPath::new();
        for i in 0..100 {
            p.move_to((f64::from(i), 0.0));
            p.curve_to((f64::from(i) + 400.0, 0.0), (f64::from(i) + 400.0, 400.0), (f64::from(i), 400.0));
        }
        let found = pieces_near(&p, None, MAX_FLATTEN_PIECES);
        assert!(found.cut, "{found:?}");
        assert!(found.charged <= MAX_FLATTEN_PIECES && found.made <= found.charged, "{found:?}");
        assert!(found.charged > MAX_FLATTEN_PIECES - 2 * estimated_pieces(400.0 * std::f64::consts::SQRT_2), "{found:?}");
    }

    #[test]
    fn endpoints_are_the_corners_once_each() {
        let e = endpoints(&square(0.0, 0.0, 2.0));
        // Start of the first piece, then each end: (0,0) (2,0) (2,2) (0,2) and the closing piece back to (0,0).
        assert_eq!(e.len(), 5);
        assert!(near(e[0], (0.0, 0.0)) && near(e[2], (2.0, 2.0)) && near(e[4], (0.0, 0.0)));
        assert!(endpoints(&BezPath::new()).is_empty());
    }

    #[test]
    fn midpoints_are_halfway_by_length() {
        let m = midpoints(&square(0.0, 0.0, 2.0));
        assert!(near(m[0], (1.0, 0.0)) && near(m[1], (2.0, 1.0)) && near(m[2], (1.0, 2.0)) && near(m[3], (0.0, 1.0)));
        // Halfway along a curve's length is on the curve, not at the middle of its control polygon.
        let arc = circle(0.0, 0.0, 1.0);
        let on = midpoints(&arc);
        assert!(!on.is_empty());
        for p in &on {
            assert!(((p.x * p.x + p.y * p.y).sqrt() - 1.0).abs() < 1.0e-3, "on the circle: {p:?}");
        }
    }

    #[test]
    fn the_nearest_point_is_on_the_outline() {
        let (p, d) = nearest(&square(0.0, 0.0, 2.0), Point::new(1.0, -0.5)).unwrap();
        assert!(near(p, (1.0, 0.0)) && (d - 0.5).abs() < 1.0e-9);
        let (p, d) = nearest(&circle(0.0, 0.0, 1.0), Point::new(3.0, 0.0)).unwrap();
        assert!((p.x - 1.0).abs() < 1.0e-4 && p.y.abs() < 1.0e-4 && (d - 2.0).abs() < 1.0e-4, "{p:?} {d}");
        assert!(nearest(&BezPath::new(), Point::new(0.0, 0.0)).is_none());
        assert!(nearest(&square(0.0, 0.0, 1.0), Point::new(f64::NAN, 0.0)).is_none());
    }

    #[test]
    fn intersections_are_where_outlines_cross() {
        // Two squares overlapping by a quarter: the sides cross at (1, 2) and (2, 1).
        let x = intersections(&square(0.0, 0.0, 2.0), &square(1.0, 1.0, 2.0), None);
        assert_eq!(x.len(), 2, "{x:?}");
        assert!(x.iter().any(|p| near(*p, (2.0, 1.0))) && x.iter().any(|p| near(*p, (1.0, 2.0))));
        // A circle wider than the square's sides crosses each side twice, curves flattened.
        let c = intersections(&square(0.0, 0.0, 2.0), &circle(1.0, 1.0, 1.2), None);
        assert_eq!(c.len(), 8, "{c:?}");
        for p in &c {
            assert!((((p.x - 1.0).powi(2) + (p.y - 1.0).powi(2)).sqrt() - 1.2).abs() < 2.0 * FLATNESS);
        }
        // The probe keeps what is near the cursor only.
        let near_one = intersections(&square(0.0, 0.0, 2.0), &square(1.0, 1.0, 2.0), Some(Rect::new(1.9, 0.9, 2.1, 1.1)));
        assert_eq!(near_one.len(), 1);
        // Outlines that do not meet, and a path with nothing.
        assert!(intersections(&square(0.0, 0.0, 1.0), &square(5.0, 5.0, 1.0), None).is_empty());
        assert!(intersections(&BezPath::new(), &square(0.0, 0.0, 1.0), None).is_empty());
    }

    #[test]
    fn tangents_touch_and_perpendiculars_are_normals() {
        let c = circle(0.0, 0.0, 1.0);
        let base = Point::new(0.0, 2.0);
        let t = tangents(&c, base);
        assert_eq!(t.len(), 2, "{t:?}");
        for p in &t {
            // On the circle, and the radius is perpendicular to the line from the base.
            assert!(((p.x * p.x + p.y * p.y).sqrt() - 1.0).abs() < 1.0e-4);
            assert!((p.to_vec2().dot(*p - base)).abs() < 1.0e-4);
        }
        // From inside there is none.
        assert!(tangents(&c, Point::new(0.1, 0.1)).is_empty());
        let feet = perpendiculars(&c, Point::new(3.0, 0.0));
        assert!(feet.iter().any(|p| near_loose(*p, (1.0, 0.0))) && feet.iter().any(|p| near_loose(*p, (-1.0, 0.0))), "{feet:?}");
        // A circle seen from its centre has no particular perpendicular; a straight side has the foot of the base on it.
        assert!(perpendiculars(&c, Point::new(0.0, 0.0)).is_empty());
        let sq = perpendiculars(&square(0.0, 0.0, 2.0), Point::new(1.0, 3.0));
        assert!(sq.iter().any(|p| near(*p, (1.0, 2.0))), "{sq:?}");
        assert!(tangents(&square(0.0, 0.0, 2.0), Point::new(5.0, 5.0)).is_empty(), "a straight side has no tangent through a point");
    }

    fn near_loose(a: Point, b: (f64, f64)) -> bool {
        (a.x - b.0).abs() < 1.0e-3 && (a.y - b.1).abs() < 1.0e-3
    }

    #[test]
    fn the_best_candidate_is_the_nearest_and_an_endpoint_wins_a_near_tie() {
        let cursor = Point::new(0.0, 0.0);
        let cands = [
            Candidate { kind: Kind::Nearest, at: Point::new(0.5, 0.0) },
            Candidate { kind: Kind::Endpoint, at: Point::new(0.0, 0.6) },
            Candidate { kind: Kind::Midpoint, at: Point::new(9.0, 9.0) },
        ];
        // The endpoint is 0.1 farther, and 15% of an aperture of 1 is 0.15: it wins.
        assert_eq!(best(&cands, cursor, 1.0).unwrap().kind, Kind::Endpoint);
        // With a smaller aperture the bonus is smaller than the gap: the nearest wins.
        assert_eq!(best(&cands, cursor, 0.65).unwrap().kind, Kind::Nearest);
        assert!(best(&cands, cursor, 0.1).is_none());
        let bad = [Candidate { kind: Kind::Nearest, at: Point::new(f64::NAN, 0.0) }];
        assert!(best(&bad, cursor, 1.0).is_none());
    }

    #[test]
    fn hostile_paths_cost_what_the_caps_say() {
        // A path of 20,000 segments is read to the cap, not to its end.
        let mut p = BezPath::new();
        p.move_to((0.0, 0.0));
        for i in 0..20_000 {
            p.line_to((f64::from(i), f64::from(i % 7)));
        }
        assert!(endpoints(&p).len() <= 2 * MAX_SEGMENTS);
        assert!(midpoints(&p).len() <= MAX_SEGMENTS);
        assert!(intersections(&p, &p, None).len() <= MAX_POINTS);
        let mut nan = BezPath::new();
        nan.move_to((f64::NAN, 0.0));
        nan.line_to((1.0, f64::INFINITY));
        assert!(endpoints(&nan).is_empty() && midpoints(&nan).is_empty() && intersections(&nan, &nan, None).is_empty());
        assert!(nearest(&nan, Point::new(0.0, 0.0)).is_none());
    }
}
