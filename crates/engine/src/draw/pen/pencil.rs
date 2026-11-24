//! The Pencil: a stroke becomes lines and arcs (it "draws lines"; the brief: "lines and arcs from the stroke").
//!
//! **Why not the sibling apps' fit.** VectorCraft's pencil and brush fit a chain of cubic Beziers (`craft-pathops`' simplify, which the Freeform tool uses, `freeform.rs`). The Pencil
//! here makes the segments of a path the way the Line and Arc tools do, `LineTo` and `ArcTo` rows, which a person can then edit vertex by vertex, so it has its own, small fit.
//!
//! **The fit is greedy and longest-first.** From the start of a run it finds the farthest point of the stroke a straight line fits to within the tolerance, and the farthest one a
//! circular arc fits to ([`longest`]); the longer of the two is the next segment, a line when they tie, and the next run begins where it ended. A line fits when no point of the run
//! is farther than the tolerance from the segment. An arc fits when the circle through its two ends and the point farthest from the chord (the apex) has no point of the run
//! farther than the tolerance from it, the points go round it one way and do not turn back, and the sweep is at most [`MAX_SWEEP`]. The bow of the arc row is worked out from the
//! circle it: positive to the right of the way the pencil went.
//!
//! **The work is bounded**: each fit looks at the points of its run, a run is found by doubling and halving, and at most [`super::MAX_PIECES`] segments are made; a stroke that
//! would need more (a scribble) is refused in words.

use kurbo::Point;

use super::{Drawn, LEAST_SIDE, MAX_PIECES, P, bounds};
use crate::draw::Problem;
use crate::draw::shapeops::CRow;

/// The most an arc may turn, in degrees: a whole circle is then two arcs, and the chord of an arc never shrinks to nothing as the sweep nears a turn.
const MAX_SWEEP: f64 = 300.0;

/// The bow of an arc as a share of its chord at the largest sweep: `2 bow / chord = tan(sweep / 4)`.
fn largest_bow_share() -> f64 {
    (MAX_SWEEP.to_radians() / 4.0).tan() / 2.0
}

/// The points with each one that is the one before it left out, so that a stroke is a run of different places.
pub(super) fn distinct(points: &[P]) -> Vec<P> {
    let mut out: Vec<P> = Vec::with_capacity(points.len());
    for p in points {
        if out.last().is_none_or(|q| (q.0 - p.0).hypot(q.1 - p.1) > 1.0e-9) {
            out.push(*p);
        }
    }
    out
}

fn dist(a: P, b: P) -> f64 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

/// How far `p` is from the segment `a` to `b`.
fn from_segment(p: P, a: P, b: P) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let l2 = dx * dx + dy * dy;
    if l2 == 0.0 {
        return dist(p, a);
    }
    let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / l2).clamp(0.0, 1.0);
    dist(p, (a.0 + t * dx, a.1 + t * dy))
}

/// Whether the points `i` to `j` of `p` are all within `tol` of the segment from the first to the last.
fn line_fits(p: &[P], i: usize, j: usize, tol: f64) -> bool {
    let (Some(a), Some(b)) = (p.get(i).copied(), p.get(j).copied()) else { return false };
    p.get(i + 1..j).is_some_and(|inner| inner.iter().all(|q| from_segment(*q, a, b) <= tol))
}

/// The circle through three points: its centre and radius, or `None` when they are in a line (or one is not a number).
fn circle_through(a: P, b: P, c: P) -> Option<(P, f64)> {
    let d = 2.0 * (a.0 * (b.1 - c.1) + b.0 * (c.1 - a.1) + c.0 * (a.1 - b.1));
    if d == 0.0 || !d.is_finite() {
        return None;
    }
    let (a2, b2, c2) = (a.0 * a.0 + a.1 * a.1, b.0 * b.0 + b.1 * b.1, c.0 * c.0 + c.1 * c.1);
    let ux = (a2 * (b.1 - c.1) + b2 * (c.1 - a.1) + c2 * (a.1 - b.1)) / d;
    let uy = (a2 * (c.0 - b.0) + b2 * (a.0 - c.0) + c2 * (b.0 - a.0)) / d;
    let r = dist((ux, uy), a);
    (ux.is_finite() && uy.is_finite() && r.is_finite() && r > 0.0).then_some(((ux, uy), r))
}

/// The bow of the circular arc from `a` to `b` that goes round the circle of centre `c` and radius `r` on the side of the chord `side` is on: the signed distance of
/// the arc's middle from the chord's, positive to the right of the way from `a` to `b`. `side` is the signed distance of a point of the arc from the chord line along the right-hand normal.
pub(super) fn bow_of(a: P, b: P, c: P, r: f64, side: f64) -> Option<f64> {
    let chord = dist(a, b);
    if !(chord > 0.0 && chord.is_finite()) {
        return None;
    }
    // The unit normal to the right of travel: (dy, -dx) / c.
    let n = ((b.1 - a.1) / chord, -(b.0 - a.0) / chord);
    let middle = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
    let t = (c.0 - middle.0) * n.0 + (c.1 - middle.1) * n.1;
    let sigma = if side >= 0.0 { 1.0 } else { -1.0 };
    let bow = t + sigma * r;
    bow.is_finite().then_some(bow)
}

/// The bow of an arc that fits the points `i` to `j` of `p` within `tol`, if one does.
fn arc_fit(p: &[P], i: usize, j: usize, tol: f64) -> Option<f64> {
    if j < i + 2 {
        return None;
    }
    let (a, b) = (p.get(i).copied()?, p.get(j).copied()?);
    let chord = dist(a, b);
    if chord < LEAST_SIDE {
        return None;
    }
    let n = ((b.1 - a.1) / chord, -(b.0 - a.0) / chord);
    let inner = p.get(i + 1..j)?;
    // The apex: the point farthest from the chord line, and on which side of it.
    let (mut apex, mut far) = (None, 0.0_f64);
    for q in inner {
        let s = (q.0 - a.0) * n.0 + (q.1 - a.1) * n.1;
        if s.abs() > far.abs() {
            (apex, far) = (Some(*q), s);
        }
    }
    let apex = apex?;
    // A stroke that never leaves the tolerance of the chord is a line's.
    if far.abs() <= tol {
        return None;
    }
    let (c, r) = circle_through(a, apex, b)?;
    let bow = bow_of(a, b, c, r, far)?;
    if bow.abs() > largest_bow_share() * chord {
        return None;
    }
    // Every point is within the tolerance of the circle, and they go round it one way: counterclockwise for a bow to the right.
    let way = if bow > 0.0 { 1.0 } else { -1.0 };
    let mut angle = 0.0_f64;
    let mut last = (a.1 - c.1).atan2(a.0 - c.0);
    for q in p.get(i..=j)? {
        if (dist(*q, c) - r).abs() > tol {
            return None;
        }
        let now = (q.1 - c.1).atan2(q.0 - c.0);
        // The turn since the last point, kept from just over -180 to 180 degrees, and counted in the way the arc goes.
        let mut step = (now - last).rem_euclid(std::f64::consts::TAU);
        if step > std::f64::consts::PI {
            step -= std::f64::consts::TAU;
        }
        let along = step * way;
        // A step back is noise if it is within the tolerance of the circle's length, else the stroke turns back on itself.
        if along < 0.0 && -along * r > tol {
            return None;
        }
        angle += along;
        last = now;
    }
    (angle >= 0.0 && angle <= MAX_SWEEP.to_radians() * 1.05).then_some(bow)
}

/// The last index in `i + 1..=end` that `fits(i, j)` holds for, found by doubling and then halving from `i + 1` (which is assumed to fit: two points always do). A fit that holds at a
/// point past a place where it did not is found if the doubling lands there.
fn longest(i: usize, end: usize, fits: impl Fn(usize, usize) -> bool) -> usize {
    let (mut good, mut step) = (i + 1, 1_usize);
    while good < end {
        let try_at = (good + step).min(end);
        if fits(i, try_at) {
            good = try_at;
            step = step.saturating_mul(2);
        } else {
            // Between the last that fit and the first that did not.
            let (mut low, mut high) = (good, try_at);
            while high - low > 1 {
                let mid = low + (high - low) / 2;
                if fits(i, mid) {
                    low = mid;
                } else {
                    high = mid;
                }
            }
            return low;
        }
    }
    good
}

/// The lines and arcs that stand for the stroke `points` within `tolerance` inches.
pub(super) fn draw(points: &[P], tolerance: f64) -> Result<Drawn, Problem> {
    let p = distinct(points);
    let (Some(first), Some(last)) = (p.first().copied(), p.last().copied()) else {
        return Err(Problem::bad("a stroke needs at least two points"));
    };
    let reach = p.iter().map(|q| dist(*q, first)).fold(0.0, f64::max);
    if p.len() < 2 || reach < LEAST_SIDE {
        return Err(Problem::bad(format!("draw a stroke at least {LEAST_SIDE} inch long")));
    }
    let end = p.len() - 1;
    let to = |k: usize| p.get(k).map_or(Point::new(last.0, last.1), |q| Point::new(q.0, q.1));
    let mut rows = vec![CRow::Move(Point::new(first.0, first.1))];
    let mut i = 0;
    while i < end {
        if rows.len() > MAX_PIECES {
            return Err(Problem::bad(format!(
                "the stroke has too many corners and curves to draw as lines and arcs (more than {MAX_PIECES}): draw it in parts, or with the Freeform tool"
            )));
        }
        let by_line = longest(i, end, |a, b| line_fits(&p, a, b, tolerance));
        // A run an arc covers is one a line covers too (an arc of a bow within the tolerance is a line) or one an arc of more bow fits: so the arcs reach as far as the lines do, and on.
        let by_arc = longest(i, end, |a, b| line_fits(&p, a, b, tolerance) || arc_fit(&p, a, b, tolerance).is_some());
        // An arc only counts where a line does not reach as far.
        match arc_fit(&p, i, by_arc, tolerance) {
            Some(bow) if by_arc > by_line => {
                rows.push(CRow::Arc { to: to(by_arc), bow });
                i = by_arc;
            }
            _ => {
                rows.push(CRow::Line(to(by_line)));
                i = by_line;
            }
        }
    }
    let closed = dist(first, last) <= 1.0e-9 && p.len() >= 3;
    let rect = bounds::bounds(&rows).ok_or_else(|| Problem::failed("the stroke has no extent"))?;
    Ok(Drawn { rows, closed, rect })
}
