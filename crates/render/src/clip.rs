//! Clipping straight-sided paths to a rectangle, for paths that reach past the coordinates the rasteriser takes.
//!
//! A page of 3,000 inches seen at a high zoom has corners millions of pixels from the view, and the rasteriser's coordinates are 32-bit.
//! A path made only of straight segments can be cut to a rectangle around the view without changing what shows in it: a fill is clipped as
//! a polygon (Sutherland and Hodgman), a stroke segment by segment (Liang and Barsky). A path with a curve in it is not clipped; the
//! caller reports it. A dash pattern restarts where a clipped stroke enters the rectangle, which moves the phase of its dashes.

use kurbo::{BezPath, PathEl, Point, Rect};

/// The subpaths of a path made only of lines: their points, and whether each was closed. `None` if there is a curve in it.
fn polylines(path: &BezPath) -> Option<Vec<(Vec<Point>, bool)>> {
    let mut out: Vec<(Vec<Point>, bool)> = Vec::new();
    for el in path.elements() {
        match *el {
            PathEl::MoveTo(p) => out.push((vec![p], false)),
            PathEl::LineTo(p) => out.last_mut()?.0.push(p),
            PathEl::ClosePath => out.last_mut()?.1 = true,
            PathEl::QuadTo(..) | PathEl::CurveTo(..) => return None,
        }
    }
    Some(out)
}

/// `path` as a fill, cut to `rect`. `None` if it has a curve.
pub(crate) fn polygon(path: &BezPath, rect: Rect) -> Option<BezPath> {
    let mut out = BezPath::new();
    for (points, _) in polylines(path)? {
        let clipped = clip_polygon(points, rect);
        if clipped.len() >= 3 {
            out.move_to(clipped[0]);
            for p in &clipped[1..] {
                out.line_to(*p);
            }
            out.close_path();
        }
    }
    Some(out)
}

/// Sutherland and Hodgman against the four sides of `rect`.
fn clip_polygon(mut points: Vec<Point>, rect: Rect) -> Vec<Point> {
    type Inside = fn(Point, Rect) -> bool;
    type Cross = fn(Point, Point, Rect) -> Point;
    let sides: [(Inside, Cross); 4] = [
        (|p, r| p.x >= r.x0, |a, b, r| Point::new(r.x0, a.y + (b.y - a.y) * (r.x0 - a.x) / (b.x - a.x))),
        (|p, r| p.x <= r.x1, |a, b, r| Point::new(r.x1, a.y + (b.y - a.y) * (r.x1 - a.x) / (b.x - a.x))),
        (|p, r| p.y >= r.y0, |a, b, r| Point::new(a.x + (b.x - a.x) * (r.y0 - a.y) / (b.y - a.y), r.y0)),
        (|p, r| p.y <= r.y1, |a, b, r| Point::new(a.x + (b.x - a.x) * (r.y1 - a.y) / (b.y - a.y), r.y1)),
    ];
    for (inside, cross) in sides {
        if points.is_empty() {
            break;
        }
        let mut next = Vec::with_capacity(points.len() + 4);
        let mut previous = points[points.len() - 1];
        for current in points {
            match (inside(previous, rect), inside(current, rect)) {
                (true, true) => next.push(current),
                (true, false) => next.push(cross(previous, current, rect)),
                (false, true) => {
                    next.push(cross(previous, current, rect));
                    next.push(current);
                }
                (false, false) => {}
            }
            previous = current;
        }
        points = next;
    }
    points
}

/// `path` as a stroke, cut to `rect`. `None` if it has a curve.
pub(crate) fn polyline(path: &BezPath, rect: Rect) -> Option<BezPath> {
    let mut out = BezPath::new();
    for (points, closed) in polylines(path)? {
        let mut all = points;
        if closed && let Some(first) = all.first().copied() {
            all.push(first);
        }
        // Whether the pen of `out` is at the end of the last segment kept.
        let mut pen: Option<Point> = None;
        for pair in all.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            match segment(a, b, rect) {
                Some((c, d)) => {
                    if pen != Some(c) {
                        out.move_to(c);
                    }
                    out.line_to(d);
                    pen = Some(d);
                }
                None => pen = None,
            }
        }
    }
    Some(out)
}

/// The part of the segment `a` to `b` inside `rect` (Liang and Barsky).
fn segment(a: Point, b: Point, rect: Rect) -> Option<(Point, Point)> {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for (p, q) in [(-dx, a.x - rect.x0), (dx, rect.x1 - a.x), (-dy, a.y - rect.y0), (dy, rect.y1 - a.y)] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
        } else {
            let r = q / p;
            if p < 0.0 {
                t0 = t0.max(r);
            } else {
                t1 = t1.min(r);
            }
        }
    }
    (t0 <= t1).then(|| (Point::new(a.x + t0 * dx, a.y + t0 * dy), Point::new(a.x + t1 * dx, a.y + t1 * dy)))
}
