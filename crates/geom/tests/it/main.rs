//! The geometry crate's integration tests: one binary, each file a module (AGENTS.md: one test binary per crate).
//!
//! Tests may unwrap and panic: a failing assertion is the point of a test. The vectors are those of
//! A test names the vector it comes from, and says
//! which wrong result the vector catches. Numbers compare within 1E-9 as the spec says, except a curve that this crate
//! turns into cubic Beziers (arcs, ellipses, rational splines), which compares within `CURVE_EPS`: those are
//! approximations bounded by the crate's tolerance, and the exact figures (centre, radii, sweep, control points, knots)
//! are compared within 1E-9 where the crate returns them.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![forbid(unsafe_code)]

mod arcs;
mod beziers;
mod ellipse;
mod elliptical;
mod fill;
mod hostile;
mod lines;
mod nurbs;
mod polyline;
mod reading;
mod row_draws;
mod splines;

use diacraft_geom::kurbo::{BezPath, ParamCurve, ParamCurveNearest, PathEl, PathSeg, Point, Rect, Shape, Size};
use diacraft_geom::{BuiltPaths, GeomRow, build_paths};

/// The spec's comparison: "Compare numbers within 1E-9 unless a vector says otherwise".
pub const EPS: f64 = 1e-9;
/// A curve made of cubic Beziers: the crate cuts arcs, ellipses and rational splines so that they stay within `CURVE_TOLERANCE`, 1E-7
/// inches of the exact curve, so 1E-6 is safe for the sizes in the vectors.
pub const CURVE_EPS: f64 = 1e-6;

pub fn p(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

pub fn near(a: f64, b: f64, eps: f64) -> bool {
    (a - b).abs() <= eps
}

pub fn assert_near(actual: f64, expected: f64, eps: f64, what: &str) {
    assert!(near(actual, expected, eps), "{what}: {actual} is not {expected} within {eps}");
}

pub fn assert_pt(actual: Point, x: f64, y: f64, eps: f64, what: &str) {
    assert!(near(actual.x, x, eps) && near(actual.y, y, eps), "{what}: ({}, {}) is not ({x}, {y}) within {eps}", actual.x, actual.y);
}

pub fn size(width: f64, height: f64) -> Size {
    Size::new(width, height)
}

/// Build with a box of the given size, and fail the test on an error of the whole path.
pub fn build(rows: &[GeomRow], width: f64, height: f64) -> BuiltPaths {
    build_paths(rows, size(width, height)).unwrap()
}

/// Build with a 1 by 1 box, for rows that use no share.
pub fn build_plain(rows: &[GeomRow]) -> BuiltPaths {
    build(rows, 1.0, 1.0)
}

pub fn mv(x: f64, y: f64) -> GeomRow {
    GeomRow::MoveTo { x, y }
}

pub fn ln(x: f64, y: f64) -> GeomRow {
    GeomRow::LineTo { x, y }
}

pub fn rel_mv(x: f64, y: f64) -> GeomRow {
    GeomRow::RelMoveTo { x, y }
}

pub fn rel_ln(x: f64, y: f64) -> GeomRow {
    GeomRow::RelLineTo { x, y }
}

pub fn arc_to(x: f64, y: f64, bow: f64) -> GeomRow {
    GeomRow::ArcTo { x, y, bow }
}

pub fn segments(path: &BezPath) -> Vec<PathSeg> {
    path.segments().collect()
}

/// The end points of the drawn segments, in order, after the first segment's start.
pub fn vertices(path: &BezPath) -> Vec<Point> {
    let segs = segments(path);
    let mut out: Vec<Point> = segs.first().map(|s| s.start()).into_iter().collect();
    out.extend(segs.iter().map(|s| s.end()));
    out
}

pub fn extents(path: &BezPath) -> Rect {
    path.bounding_box()
}

/// Every x at which the path crosses the horizontal line `y`, found by sampling each segment finely and interpolating.
pub fn x_at_y(path: &BezPath, y: f64) -> Vec<f64> {
    crossings(path, |pt| pt.y - y, |pt| pt.x)
}

/// Every y at which the path crosses the vertical line `x`.
pub fn y_at_x(path: &BezPath, x: f64) -> Vec<f64> {
    crossings(path, |pt| pt.x - x, |pt| pt.y)
}

fn crossings(path: &BezPath, side: impl Fn(Point) -> f64, value: impl Fn(Point) -> f64) -> Vec<f64> {
    const STEPS: usize = 2000;
    let mut found = Vec::new();
    for seg in path.segments() {
        let mut prev = seg.eval(0.0);
        for i in 1..=STEPS {
            let next = seg.eval(i as f64 / STEPS as f64);
            let (a, b) = (side(prev), side(next));
            if (a < 0.0) != (b < 0.0) {
                let t = a / (a - b);
                found.push(value(prev) + (value(next) - value(prev)) * t);
            }
            prev = next;
        }
    }
    found
}

/// True if the path passes within `eps` of `target`: the nearest point of some segment, found to 1E-12, is that close.
pub fn passes_through(path: &BezPath, target: Point, eps: f64) -> bool {
    path.segments().any(|seg| seg.nearest(target, 1e-12).distance_sq.sqrt() <= eps)
}

pub fn ends_with_close(path: &BezPath) -> bool {
    matches!(path.elements().last(), Some(PathEl::ClosePath))
}
