//! Rounded corners.
//!
//! A `Rounding` above 0 turns each corner where two straight segments meet into an arc of that radius, tangent to both (a circular
//! arc, its ends the radius from the corner along each edge). It applies to the outline for the stroke and the fill alike. The radius is
//! in drawing units, so it is applied in the shape's own frame, before the page's scale.
//!
//! A corner whose edges are shorter than the radius needs is rounded as far as half of the shorter edge allows, so neighbouring corners
//! never overlap. Curves are left as they are; only a corner between two lines is touched.

use kurbo::{BezPath, PathEl, Point};

/// `path` with every corner between two straight segments rounded to `radius`.
pub(crate) fn round(path: &BezPath, radius: f64) -> BezPath {
    if !(radius.is_finite() && radius > 0.0) {
        return path.clone();
    }
    let mut out = BezPath::new();
    let mut run: Vec<PathEl> = Vec::new();
    for el in path.elements() {
        match el {
            PathEl::MoveTo(_) => {
                flush(&mut run, radius, &mut out);
                run.push(*el);
            }
            _ => run.push(*el),
        }
    }
    flush(&mut run, radius, &mut out);
    out
}

/// A subpath: its elements from a `MoveTo` to the next.
fn flush(run: &mut Vec<PathEl>, radius: f64, out: &mut BezPath) {
    if run.is_empty() {
        return;
    }
    match lines_of(run) {
        Some((points, closed)) => emit_rounded(&points, closed, radius, out),
        None => out.extend(run.iter().copied()),
    }
    run.clear();
}

/// The vertices of a subpath that is only straight segments, and whether it is closed (a `ClosePath`, or the last point back on the first).
fn lines_of(run: &[PathEl]) -> Option<(Vec<Point>, bool)> {
    let mut points = Vec::with_capacity(run.len());
    let mut closed = false;
    for (i, el) in run.iter().enumerate() {
        match (i, el) {
            (0, PathEl::MoveTo(p)) => points.push(*p),
            (_, PathEl::LineTo(p)) if i > 0 => points.push(*p),
            (_, PathEl::ClosePath) if i + 1 == run.len() => closed = true,
            _ => return None,
        }
    }
    // A repeated point is no corner; a path that ends where it began is closed.
    points.dedup_by(|a, b| a.distance(*b) < 1.0e-12);
    if points.len() > 1 && points.first().zip(points.last()).is_some_and(|(a, b)| a.distance(*b) < 1.0e-12) {
        points.pop();
        closed = true;
    }
    if points.len() < 3 {
        return None;
    }
    Some((points, closed))
}

fn emit_rounded(points: &[Point], closed: bool, radius: f64, out: &mut BezPath) {
    let n = points.len();
    // For each vertex: the point where the edge into it is left and the point where the edge out of it is joined, with the arc between.
    let corner = |i: usize| -> Option<(Point, Point, Point, Point)> {
        let (prev, here, next) = (points.get((i + n - 1) % n)?, points.get(i)?, points.get((i + 1) % n)?);
        let (to_prev, to_next) = (*prev - *here, *next - *here);
        let (len_prev, len_next) = (to_prev.hypot(), to_next.hypot());
        if len_prev < 1.0e-12 || len_next < 1.0e-12 {
            return None;
        }
        let (u0, u1) = (to_prev / len_prev, to_next / len_next);
        let cos_theta = (u0.dot(u1)).clamp(-1.0, 1.0);
        let theta = cos_theta.acos();
        // A straight run, or a fold back on itself, has no corner to round.
        if theta < 1.0e-6 || (std::f64::consts::PI - theta) < 1.0e-6 {
            return None;
        }
        let tangent = radius / (theta / 2.0).tan();
        let d = tangent.min(len_prev / 2.0).min(len_next / 2.0);
        if !(d.is_finite() && d > 0.0) {
            return None;
        }
        let r = d * (theta / 2.0).tan();
        let turn = std::f64::consts::PI - theta;
        let k = 4.0 / 3.0 * (turn / 4.0).tan() * r;
        let (a, b) = (*here + u0 * d, *here + u1 * d);
        // The control points run along the edges, toward the corner.
        Some((a, a - u0 * k, b - u1 * k, b))
    };
    let mut first = true;
    let range: Vec<usize> = if closed { (0..n).collect() } else { (1..n.saturating_sub(1)).collect() };
    if !closed && let Some(p) = points.first() {
        out.move_to(*p);
        first = false;
    }
    for i in range {
        match corner(i) {
            Some((a, ca, cb, b)) => {
                if first {
                    out.move_to(a);
                    first = false;
                } else {
                    out.line_to(a);
                }
                out.curve_to(ca, cb, b);
            }
            None => {
                if let Some(p) = points.get(i) {
                    if first {
                        out.move_to(*p);
                        first = false;
                    } else {
                        out.line_to(*p);
                    }
                }
            }
        }
    }
    if !closed {
        if let Some(p) = points.last() {
            out.line_to(*p);
        }
    } else {
        out.close_path();
    }
}
