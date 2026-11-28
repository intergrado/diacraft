//! Runs: the paths of the shapes a command works on as they are drawn, for Join, Trim, Offset and Fit Curve.
//!
//! A *run* is one path: where it starts and the segments that follow, each a straight line or one cubic Bezier (a quadratic is raised to a cubic, an arc or an ellipse is
//! already the cubics the geometry crate draws it as). It is **closed** when its ends meet within [`MERGE`], the closure tolerance (`Path.Closed`). A section of a
//! shape holds one run for each `MoveTo` in it, so a section can hold several, and a shape's sections hold as many more.
//!
//! **A run read from a shape never holds an arc**: only Fit Curve makes one ([`Seg::Arc`], an `ArcTo` row, for the stretches it finds circular with the circles flag), and
//! the arc is drawn as the cubics that stand for it ([`arc_cubics`]) wherever a run is handed to the path library.
//!
//! Nothing here knows a document: runs are plain kurbo values in the container's coordinates (or the working frame near the origin the library is given).

use kurbo::{Arc as KArc, BezPath, CubicBez, PathEl, Point, Rect, Shape as _, Vec2};

use super::MERGE;

/// One segment of a run: a straight line to a point, or one cubic Bezier with its two control points and its end. Its start is where the segment before it ended.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Seg {
    Line(Point),
    Cubic(Point, Point, Point),
    /// A circular arc to the point (only Fit Curve makes one): `bow` is how far the arc's middle stands from the chord's, positive to the right of the way of travel.
    Arc {
        to: Point,
        bow: f64,
    },
}

impl Seg {
    pub(super) fn end(&self) -> Point {
        match *self {
            Seg::Line(p) | Seg::Cubic(_, _, p) | Seg::Arc { to: p, .. } => p,
        }
    }
}

/// One path: its start and its segments.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Run {
    pub start: Point,
    pub segs: Vec<Seg>,
}

impl Run {
    /// Where the last segment ends (the start for a run of none).
    pub(super) fn end(&self) -> Point {
        self.segs.last().map_or(self.start, Seg::end)
    }

    /// Whether the ends meet within [`MERGE`]: a run of no segment is not closed.
    pub(super) fn closed(&self) -> bool {
        !self.segs.is_empty() && self.start.distance(self.end()) <= MERGE
    }

    /// The run the other way round: it starts where this one ends, each cubic has its control points the other way about, and an arc's bow changes sign (the way of travel
    /// turned round puts the right side on the left).
    pub(super) fn reversed(&self) -> Run {
        let mut points = Vec::with_capacity(self.segs.len() + 1);
        points.push(self.start);
        points.extend(self.segs.iter().map(Seg::end));
        let mut segs = Vec::with_capacity(self.segs.len());
        for (k, seg) in self.segs.iter().enumerate().rev() {
            // The segment `k` goes from `points[k]` to `points[k + 1]`: turned round it goes to `points[k]`.
            let to = points.get(k).copied().unwrap_or(self.start);
            segs.push(match *seg {
                Seg::Line(_) => Seg::Line(to),
                Seg::Cubic(a, b, _) => Seg::Cubic(b, a, to),
                Seg::Arc { bow, .. } => Seg::Arc { to, bow: -bow },
            });
        }
        Run { start: self.end(), segs }
    }

    /// The run with every length multiplied by `k` (a uniform scale about the origin): points, control points and bows.
    pub(super) fn scaled(&self, k: f64) -> Run {
        let to = |p: Point| Point::new(p.x * k, p.y * k);
        let segs = self
            .segs
            .iter()
            .map(|seg| match *seg {
                Seg::Line(p) => Seg::Line(to(p)),
                Seg::Cubic(a, b, p) => Seg::Cubic(to(a), to(b), to(p)),
                Seg::Arc { to: p, bow } => Seg::Arc { to: to(p), bow: bow * k },
            })
            .collect();
        Run { start: to(self.start), segs }
    }

    /// Whether every number of the run is finite: its start, and the points, control points and bows of its segments.
    pub(super) fn is_finite(&self) -> bool {
        self.start.is_finite()
            && self.segs.iter().all(|seg| match *seg {
                Seg::Line(p) => p.is_finite(),
                Seg::Cubic(a, b, p) => a.is_finite() && b.is_finite() && p.is_finite(),
                Seg::Arc { to, bow } => to.is_finite() && bow.is_finite(),
            })
    }

    /// The run as an open path, a `MoveTo` and a stroke for each segment (an arc as the cubics that draw it).
    pub(super) fn path(&self) -> BezPath {
        let mut path = BezPath::new();
        path.move_to(self.start);
        let mut pen = self.start;
        for seg in &self.segs {
            match *seg {
                Seg::Line(p) => path.line_to(p),
                Seg::Cubic(a, b, p) => path.curve_to(a, b, p),
                Seg::Arc { to, bow } => {
                    for cubic in arc_cubics(pen, to, bow) {
                        path.curve_to(cubic.p1, cubic.p2, cubic.p3);
                    }
                }
            }
            pen = seg.end();
        }
        path
    }

    /// The box with sides along the axes that holds the run: curves by their true extent.
    pub(super) fn bounds(&self) -> Rect {
        self.path().bounding_box()
    }

    /// Whether the run draws something: some point of it, a control point too, lies farther than [`MERGE`] from the start in x or in y. A run that does not is a point,
    /// and a line out and back to one is not (it has an end that is far from the start).
    pub(super) fn has_extent(&self) -> bool {
        let far = |p: Point| (p.x - self.start.x).abs() > MERGE || (p.y - self.start.y).abs() > MERGE;
        self.segs.iter().any(|seg| match *seg {
            Seg::Line(p) => far(p),
            Seg::Cubic(a, b, p) => far(a) || far(b) || far(p),
            Seg::Arc { to, bow } => far(to) || bow.abs() > MERGE,
        })
    }
}

/// A cubic's control points for the quadratic from `from` through `c` to `to`: two thirds of the way to the control point from each end.
fn cubic_of_quad(from: Point, c: Point, to: Point) -> (Point, Point) {
    (from + (c - from) * (2.0 / 3.0), to + (c - to) * (2.0 / 3.0))
}

/// The runs of a path, one for each `MoveTo`: a `ClosePath` adds the straight segment back to the start where the path does not end within [`MERGE`] of it, and
/// ends the run. A run that draws nothing (a point, a move alone) is left out. The elements are those of a path the geometry crate built, which a caller has checked
/// for numbers that are not finite.
pub(super) fn runs_of(path: &BezPath) -> Vec<Run> {
    let mut runs: Vec<Run> = Vec::new();
    let mut current: Option<Run> = None;
    let mut pen = Point::ORIGIN;
    let finish = |run: Option<Run>, runs: &mut Vec<Run>| {
        if let Some(run) = run.filter(Run::has_extent) {
            runs.push(run);
        }
    };
    for el in path.elements() {
        match *el {
            PathEl::MoveTo(p) => {
                finish(current.take(), &mut runs);
                current = Some(Run { start: p, segs: Vec::new() });
                pen = p;
            }
            PathEl::LineTo(p) => {
                if let Some(run) = current.as_mut() {
                    run.segs.push(Seg::Line(p));
                    pen = p;
                }
            }
            PathEl::QuadTo(c, p) => {
                if let Some(run) = current.as_mut() {
                    let (a, b) = cubic_of_quad(pen, c, p);
                    run.segs.push(Seg::Cubic(a, b, p));
                    pen = p;
                }
            }
            PathEl::CurveTo(a, b, p) => {
                if let Some(run) = current.as_mut() {
                    run.segs.push(Seg::Cubic(a, b, p));
                    pen = p;
                }
            }
            PathEl::ClosePath => {
                if let Some(mut run) = current.take() {
                    if !run.segs.is_empty() && pen.distance(run.start) > MERGE {
                        run.segs.push(Seg::Line(run.start));
                    }
                    finish(Some(run), &mut runs);
                }
            }
        }
    }
    finish(current, &mut runs);
    runs
}

/// The runs as one path.
pub(super) fn runs_path(runs: &[Run]) -> BezPath {
    let mut all = BezPath::new();
    for run in runs {
        for el in run.path().elements() {
            all.push(*el);
        }
    }
    all
}

// ---- arcs -----------------------------------------------------------------------------------------------------------------------

/// The circle an `ArcTo` from `from` to `to` with this `bow` is an arc of, and the arc's sweep: `(centre, radius, start angle, sweep)`, the sweep positive when the
/// arc runs anticlockwise (a positive bow). `None` for no arc: a chord of no length, a bow that is nothing or no number.
pub(super) fn arc_circle(from: Point, to: Point, bow: f64) -> Option<(Point, f64, f64, f64)> {
    // A chord of no length, a bow of nothing and a number that is not one all come out as a number that is not finite below (a division by zero), and are no arc.
    let chord = to - from;
    let reach = chord.hypot();
    let left = Vec2::new(-chord.y, chord.x) / reach;
    let middle = from.midpoint(to);
    let sag = bow.abs();
    let radius = (reach * reach / 4.0 + sag * sag) / (2.0 * sag);
    // The centre is on the left for a bow to the right and on the right for a bow to the left, `radius - sag` from the chord, which is past it for an arc of more than half a circle.
    let side = bow.signum();
    let centre = middle + left * (side * (radius - sag));
    let half = (reach / 2.0).atan2(radius - sag);
    let start = (from.y - centre.y).atan2(from.x - centre.x);
    (radius.is_finite() && centre.is_finite() && half.is_finite()).then_some((centre, radius, start, side * 2.0 * half))
}

/// The cubics that draw the arc (the straight chord, as one cubic, for no arc: the row draws the line).
pub(super) fn arc_cubics(from: Point, to: Point, bow: f64) -> Vec<CubicBez> {
    let Some((centre, radius, start, sweep)) = arc_circle(from, to, bow) else { return vec![CubicBez::new(from, from, to, to)] };
    let arc = KArc::new(centre, Vec2::new(radius, radius), start, sweep, 0.0);
    let mut cubics: Vec<CubicBez> = Vec::new();
    let mut pen = from;
    for el in arc.append_iter(1.0e-7) {
        if let PathEl::CurveTo(a, b, p) = el {
            cubics.push(CubicBez::new(pen, a, b, p));
            pen = p;
        }
    }
    // The pieces end where the arc does, to within rounding: the last ends exactly on the row's end.
    if let Some(last) = cubics.last_mut() {
        last.p3 = to;
    }
    if cubics.is_empty() { vec![CubicBez::new(from, from, to, to)] } else { cubics }
}

/// The centre of the circle through three points, `None` for three that are in a line (or a number that is not one).
fn circumcentre(a: Point, b: Point, c: Point) -> Option<Point> {
    let (ab, ac) = (b - a, c - a);
    // Three in a line have no circle: `d` is 0 and the centre is not a number (an infinity or 0 over 0), which is no centre.
    let d = 2.0 * ab.cross(ac);
    let (ab2, ac2) = (ab.hypot2(), ac.hypot2());
    let centre = Point::new(a.x + (ac.y * ab2 - ab.y * ac2) / d, a.y + (ab.x * ac2 - ac.x * ab2) / d);
    centre.is_finite().then_some(centre)
}

/// The bow of the arc from `from` to `to` that passes through `through`: how far the middle of that arc stands from the chord's middle, positive on the right of the way of
/// travel (an anticlockwise arc). The point need not be the arc's middle: it is any point of it, and the circle through the three is the arc's. `None` for a chord of no
/// length, three points in a line or a number that is not one.
pub(super) fn bow_through(from: Point, to: Point, through: Point) -> Option<f64> {
    let chord = to - from;
    let reach = chord.hypot();
    // No circle for a chord of no length, three points in a line or a number that is not one.
    let centre = circumcentre(from, to, through)?;
    let radius = (from - centre).hypot();
    let angle = |p: Point| (p.y - centre.y).atan2(p.x - centre.x);
    let (start, end, via) = (angle(from), angle(to), angle(through));
    // How far round, from the start in the given direction (1 anticlockwise, -1 clockwise), to a point: in (0, a whole turn]. The angles are atan2's, in (-pi, pi], so what is
    // turned is in (-2 pi, 2 pi) and a turn added once makes it positive.
    let round = |to_angle: f64, direction: f64| {
        let turned = direction * (to_angle - start);
        if turned <= 0.0 { turned + std::f64::consts::TAU } else { turned }
    };
    // The way round that comes to `through` before it comes to the end is the arc's.
    let direction = if round(via, 1.0) < round(end, 1.0) { 1.0 } else { -1.0 };
    let middle = start + direction * round(end, direction) / 2.0;
    let top = Point::new(centre.x + radius * middle.cos(), centre.y + radius * middle.sin());
    let right = Vec2::new(chord.y, -chord.x) / reach;
    let bow = (top - from.midpoint(to)).dot(right);
    bow.is_finite().then_some(bow)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn p(x: f64, y: f64) -> Point {
        Point::new(x, y)
    }

    #[test]
    fn a_path_is_one_run_for_each_move_and_a_close_adds_the_way_back() {
        let mut path = BezPath::new();
        path.move_to((0.0, 0.0));
        path.line_to((1.0, 0.0));
        path.line_to((1.0, 1.0));
        path.close_path();
        path.move_to((5.0, 5.0));
        path.line_to((6.0, 5.0));
        let runs = runs_of(&path);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].segs, [Seg::Line(p(1.0, 0.0)), Seg::Line(p(1.0, 1.0)), Seg::Line(p(0.0, 0.0))], "the closing segment is added");
        assert!(runs[0].closed() && !runs[1].closed());
    }

    #[test]
    fn a_run_that_ends_where_it_started_needs_no_closing_segment() {
        let mut path = BezPath::new();
        path.move_to((0.0, 0.0));
        path.line_to((1.0, 0.0));
        path.line_to((0.0, 0.000_005));
        path.close_path();
        let runs = runs_of(&path);
        assert_eq!(runs[0].segs.len(), 2, "5E-6 from the start is the start");
        assert!(runs[0].closed());
    }

    #[test]
    fn a_point_and_a_move_alone_draw_nothing_and_are_left_out() {
        let mut path = BezPath::new();
        path.move_to((0.0, 0.0));
        path.move_to((3.0, 3.0));
        path.line_to((3.0, 3.000_001));
        path.move_to((7.0, 7.0));
        path.line_to((8.0, 7.0));
        let runs = runs_of(&path);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].start, p(7.0, 7.0));
        // A line there and back is not a point.
        let mut there_and_back = BezPath::new();
        there_and_back.move_to((0.0, 0.0));
        there_and_back.line_to((2.0, 0.0));
        there_and_back.line_to((0.0, 0.0));
        assert_eq!(runs_of(&there_and_back).len(), 1);
    }

    #[test]
    fn a_quadratic_is_raised_to_the_cubic_with_the_same_curve() {
        let mut path = BezPath::new();
        path.move_to((0.0, 0.0));
        path.quad_to((1.0, 2.0), (2.0, 0.0));
        let runs = runs_of(&path);
        let Seg::Cubic(a, b, to) = runs[0].segs[0] else { panic!("a cubic") };
        assert_eq!(to, p(2.0, 0.0));
        let cubic = kurbo::CubicBez::new(p(0.0, 0.0), a, b, to);
        let quad = kurbo::QuadBez::new(p(0.0, 0.0), p(1.0, 2.0), p(2.0, 0.0));
        for k in 0..=10 {
            let t = f64::from(k) / 10.0;
            assert!(kurbo::ParamCurve::eval(&cubic, t).distance(kurbo::ParamCurve::eval(&quad, t)) < 1e-12);
        }
    }

    #[test]
    fn a_run_reversed_twice_is_the_run_and_each_cubic_turns_its_controls_about() {
        let run =
            Run { start: p(0.0, 0.0), segs: vec![Seg::Line(p(1.0, 0.0)), Seg::Cubic(p(2.0, 1.0), p(3.0, 1.0), p(4.0, 0.0)), Seg::Line(p(4.0, 5.0))] };
        let back = run.reversed();
        assert_eq!(back.start, p(4.0, 5.0));
        assert_eq!(back.segs, [Seg::Line(p(4.0, 0.0)), Seg::Cubic(p(3.0, 1.0), p(2.0, 1.0), p(1.0, 0.0)), Seg::Line(p(0.0, 0.0))]);
        assert_eq!(back.reversed(), run);
    }

    #[test]
    fn a_cubic_that_loops_to_its_own_start_has_extent_through_its_controls() {
        let loop_of_one = Run { start: p(0.0, 0.0), segs: vec![Seg::Cubic(p(2.0, 2.0), p(-2.0, 2.0), p(0.0, 0.0))] };
        assert!(loop_of_one.has_extent() && loop_of_one.closed());
        let nothing = Run { start: p(0.0, 0.0), segs: vec![Seg::Line(p(0.0, 0.0))] };
        assert!(!nothing.has_extent());
    }
}
