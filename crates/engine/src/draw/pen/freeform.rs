//! The Freeform tool: a stroke becomes one smooth curve, a `NURBSTo` row ("an irregular line"; the brief: "a smoothed spline (NURBS)").
//!
//! **The fit is the sibling apps'.** VectorCraft's pencil and brush fit a freehand stroke by simplifying it with the path library the Craft apps share, `craft-pathops`
//! (`simplify_with`: a least-squares Bezier refit that keeps the corners sharper than a threshold and, with a smoothing, takes the jitter out first). The stroke is made a polyline
//! and handed to it; what comes back is a chain of cubics. **What is ours is the row:** the chain is written as one `NURBSTo` row, degree 3, whose knots are those of a composite
//! Bezier (each join is a whole number repeated three times, the form `Page.DrawBezier` stores), with every weight 1. The geometry crate reads it back as the
//! chain it was made of, so the shape draws as the fit says, and a person can move its vertices.
//!
//! **The smoothing** (0 to 1) is the spread of a Gaussian along the stroke, [`SMOOTH_SPREAD`] inches at 1, applied before the refit; **the tolerance** is how far the curve may stand
//! from the (smoothed) stroke. The corner threshold is 60 degrees: a turn sharper than that stays a corner of the curve (a joint of the composite with no tangent in common).
//! The specifications give no setting for either; they are ours (`docs/editing-commands.md`).

use craft_pathops::{SimplifyOptions, simplify_with};
use kurbo::{BezPath, PathEl, Point};

use super::pencil::distinct;
use super::{Drawn, LEAST_SIDE, P, bounds};
use crate::draw::Problem;
use crate::draw::shapeops::{CRow, NurbsKnots};

/// The spread of the Gaussian that smooths a stroke at a smoothing of 1, in inches.
const SMOOTH_SPREAD: f64 = 0.25;

/// A turn of more than this many degrees is a corner and not smoothed away.
const CORNER_DEGREES: f64 = 60.0;

/// The most cubics one stroke's curve may have (3 control points each): a bound on the row the curve is written as.
pub(crate) const MAX_CUBICS: usize = 2_000;

/// The points of a stroke kept for the fit: the first, the last, and each point at least `gap` from the one kept before it. The hand's jitter between two samples a hair apart
/// would otherwise be a turn of any angle at each, and every one a corner.
fn thinned(points: &[P], gap: f64) -> Vec<P> {
    let mut out: Vec<P> = Vec::with_capacity(points.len().min(1_024));
    for (i, q) in points.iter().enumerate() {
        let is_last = i + 1 == points.len();
        let far = out.last().is_none_or(|k| (k.0 - q.0).hypot(k.1 - q.1) >= gap);
        if far || is_last {
            // The last point replaces a kept one that is nearer than the gap to it, so that it ends the stroke without a hair of a segment before it.
            if is_last && !far && out.len() > 1 {
                out.pop();
            }
            out.push(*q);
        }
    }
    out
}

type Cubic = [Point; 4];

fn lerp(a: Point, b: Point, t: f64) -> Point {
    Point::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t)
}

/// The cubics of a path of one run, a line or a quadratic being raised to a cubic; `None` for a path of several runs or none.
fn cubics_of(path: &BezPath) -> Option<Vec<Cubic>> {
    let mut out: Vec<Cubic> = Vec::new();
    let mut pen: Option<Point> = None;
    for el in path.elements() {
        match *el {
            PathEl::MoveTo(p) => {
                if pen.is_some() {
                    return None;
                }
                pen = Some(p);
            }
            PathEl::LineTo(p) => {
                let a = pen?;
                out.push([a, lerp(a, p, 1.0 / 3.0), lerp(a, p, 2.0 / 3.0), p]);
                pen = Some(p);
            }
            PathEl::QuadTo(c, p) => {
                let a = pen?;
                out.push([a, lerp(a, c, 2.0 / 3.0), lerp(p, c, 2.0 / 3.0), p]);
                pen = Some(p);
            }
            PathEl::CurveTo(c1, c2, p) => {
                let a = pen?;
                out.push([a, c1, c2, p]);
                pen = Some(p);
            }
            PathEl::ClosePath => {}
        }
        if out.len() > MAX_CUBICS {
            return None;
        }
    }
    (!out.is_empty()).then_some(out)
}

/// The knot at place `i` of the list a composite of `pieces` cubics writes: 0 four times, then each whole number up to `pieces` - 1 three times, then `pieces` once (the other
/// three copies of the last knot are implied).
pub(crate) fn knot(i: usize, pieces: usize) -> f64 {
    if i < 4 {
        0.0
    } else if i == 3 * pieces + 1 {
        pieces as f64
    } else {
        ((i - 1) / 3) as f64
    }
}

/// The `NURBSTo` row of a chain of cubics that each begin where the one before ended: degree 3, the control points the cubics' own, the knots of a composite Bezier.
/// `None` for no cubics.
pub(super) fn nurbs_row(cubics: &[Cubic]) -> Option<CRow> {
    let (first, last) = (cubics.first()?, cubics.last()?);
    let pieces = cubics.len();
    // The control points in order: the pen, then three for each cubic; the row holds the ones between the pen and the last.
    let mut controls: Vec<Point> = vec![first[0]];
    for cubic in cubics {
        controls.extend([cubic[1], cubic[2], cubic[3]]);
    }
    let n = controls.len();
    let middle: Vec<(Point, f64, f64)> =
        controls.iter().enumerate().skip(1).take(n.saturating_sub(2)).map(|(i, p)| (*p, knot(i, pieces), 1.0)).collect();
    Some(CRow::Nurbs {
        to: last[3],
        knots: NurbsKnots { second_last_knot: knot(n - 1, pieces), last_weight: 1.0, first_knot: knot(0, pieces), first_weight: 1.0 },
        last_knot: knot(n, pieces),
        degree: 3.0,
        points: middle,
    })
}

/// The curve that stands for the stroke `points` within `tolerance` inches, smoothed by `smoothing`.
pub(super) fn draw(points: &[P], tolerance: f64, smoothing: f64) -> Result<Drawn, Problem> {
    let p = thinned(&distinct(points), tolerance * 0.5);
    let (Some(first), Some(last)) = (p.first().copied(), p.last().copied()) else {
        return Err(Problem::bad("a stroke needs at least two points"));
    };
    let reach = p.iter().map(|q| (q.0 - first.0).hypot(q.1 - first.1)).fold(0.0, f64::max);
    if p.len() < 2 || reach < LEAST_SIDE {
        return Err(Problem::bad(format!("draw a stroke at least {LEAST_SIDE} inch long")));
    }
    let (start, end) = (Point::new(first.0, first.1), Point::new(last.0, last.1));
    let closed = (first.0 - last.0).hypot(first.1 - last.1) <= 1.0e-9 && p.len() >= 3;
    // Two points are a straight segment, which a curve through them would only say in more words.
    if p.len() == 2 {
        let rows = vec![CRow::Move(start), CRow::Line(end)];
        let rect = bounds::bounds(&rows).ok_or_else(|| Problem::failed("the stroke has no extent"))?;
        return Ok(Drawn { rows, closed: false, rect });
    }
    let mut polyline = BezPath::new();
    for (i, q) in p.iter().enumerate() {
        if i == 0 {
            polyline.move_to((q.0, q.1));
        } else {
            polyline.line_to((q.0, q.1));
        }
    }
    let options = SimplifyOptions::default().tolerance(tolerance).corner_angle_deg(CORNER_DEGREES).smooth(smoothing * SMOOTH_SPREAD);
    let fitted = simplify_with(&polyline, &options);
    let mut cubics = cubics_of(&fitted).ok_or_else(|| {
        Problem::bad(format!(
            "the stroke has too many corners to fit with one curve (more than {MAX_CUBICS} pieces): draw it in parts, or give a larger `tolerance`"
        ))
    })?;
    // The curve begins and ends where the stroke does, to the last digit: the ends are the vertices a person joins to.
    if let (Some(head), Some(tail)) = (cubics.first().map(|c| c[0]), cubics.last().map(|c| c[3])) {
        let near = |a: Point, b: Point| (a.x - b.x).hypot(a.y - b.y) <= tolerance.max(1.0e-6) * 2.0;
        if !(near(head, start) && near(tail, end)) {
            return Err(Problem::failed("the fitted curve did not keep the ends of the stroke"));
        }
    }
    if let Some(head) = cubics.first_mut() {
        head[0] = start;
    }
    if let Some(tail) = cubics.last_mut() {
        tail[3] = end;
    }
    let row = nurbs_row(&cubics).ok_or_else(|| Problem::failed("the fit made no curve"))?;
    let rows = vec![CRow::Move(start), row];
    let rect = bounds::bounds(&rows).ok_or_else(|| Problem::failed("the curve has no extent"))?;
    Ok(Drawn { rows, closed, rect })
}
