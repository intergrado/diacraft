//! Builders every test file uses: obstacles, ends, requests, and the route taken out of an outcome.

use diacraft_route::{Dir, DirSet, End, Obstacle, Obstacles, Outcome, Point, Rect, Request, Settings, Wires, Work, route};

pub fn p(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

/// An obstacle of box `(x0, y0)` to `(x1, y1)`.
pub fn obstacle(id: u32, x0: f64, y0: f64, x1: f64, y1: f64) -> Obstacle {
    Obstacle { id, rect: Rect { x0, y0, x1, y1 }, pass_level: false, pass_upright: false }
}

/// An end at `(x, y)` leaving by `way`, standing on `on`.
pub fn end(x: f64, y: f64, way: Dir, on: Option<u32>) -> End {
    End { at: p(x, y), ways: DirSet::only(way), stands_on: on }
}

/// An end at `(x, y)` that may leave any way and stands on nothing.
pub fn free(x: f64, y: f64) -> End {
    End { at: p(x, y), ways: DirSet::ANY, stands_on: None }
}

pub fn request(begin: End, end: End) -> Request {
    Request { begin, end, exempt: Vec::new(), glued: [None, None] }
}

/// Settings with one clearance in both directions, no spacing, and the engine's costs in inches.
pub fn settings(clearance: f64) -> Settings {
    Settings { clearance: (clearance, clearance), spacing: (0.0, 0.0), margin: 1000.0, ..Settings::default() }
}

/// Routes with a budget of ten million steps.
pub fn go(obstacles: Vec<Obstacle>, request: &Request, settings: &Settings) -> Outcome {
    route(&Obstacles::new(obstacles), &Wires::new(Vec::new()), request, settings, &mut Work::new(10_000_000))
}

/// The points of a route, or a panic that says what came instead.
pub fn points(outcome: &Outcome) -> Vec<Point> {
    match outcome {
        Outcome::Routed { points, .. } => points.clone(),
        other => panic!("expected a route, got {other:?}"),
    }
}

/// The length of a polyline.
pub fn length(points: &[Point]) -> f64 {
    points.windows(2).map(|w| (w[1].x - w[0].x).abs() + (w[1].y - w[0].y).abs()).sum()
}

/// The turns of a polyline (every leg is level or upright).
pub fn bends(points: &[Point]) -> usize {
    let dir = |a: Point, b: Point| (b.x > a.x + 1e-12, b.x < a.x - 1e-12, b.y > a.y + 1e-12, b.y < a.y - 1e-12);
    points.windows(3).filter(|w| dir(w[0], w[1]) != dir(w[1], w[2])).count()
}
