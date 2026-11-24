//! Rows to paths: the pen, the stroke, the fill ring, and what is done with a row that cannot be drawn.
//!
//! The pen is the current point: a row other than a move or an `Ellipse` or `InfiniteLine` draws from it. A section's rows give two
//! paths. The *stroke* path has a gap at each move, and a closing segment is added to a run only where its end and start coincide
//! The *fill ring* takes each move as a straight edge and closes the whole section once.

use std::f64::consts::TAU;

use kurbo::{BezPath, PathEl, Point, Size};

use crate::arc::{ArcFallback, Conjugate, arc_segments, circular_arc, elliptical_arc};
use crate::error::{GeomError, Warning, WarningKind};
use crate::fill::is_closed;
use crate::infinite::InfiniteLine;
use crate::nurbs::{CurveProblem, Fitted, MAX_CURVE_WORK, NurbsCurve};
use crate::row::{GeomRow, MAX_CONSTRUCTOR_POINTS, Polyline};
use crate::rules::{self, RunJoin};
use crate::seg::{Seg, Step};

/// The most rows of one section that are drawn. The most in files is 31,273.
pub const MAX_ROWS: usize = 200_000;

/// The most elements the stroke path, or the fill ring, may hold.
pub const MAX_PATH_ELEMENTS: usize = 4_000_000;

/// What a section's rows draw.
#[derive(Clone, Debug, Default)]
pub struct BuiltPaths {
    /// The path to stroke: the rows as drawn, with a gap at each move. Empty for no rows.
    pub stroke: BezPath,
    /// The ring to fill: every move is a straight edge, and the ring is closed. Empty when the section has no segment.
    pub fill: BezPath,
    /// The unbounded lines of `InfiniteLine` rows, which are no part of either path; clip them with [`InfiniteLine::clip`].
    pub infinite_lines: Vec<InfiniteLine>,
    /// Each row that was skipped or drawn by a fallback rule.
    pub warnings: Vec<Warning>,
    /// The spline work the section spent, in the units of [`Bound::curve_work`].
    pub curve_work: u64,
}

/// What a caller allows one section to cost, where it allows less than the renderer does (a snap builds the outlines of up to 16 shapes at each
/// pointer move, and a hostile file's rows may hold constructors of tens of thousands of points).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bound {
    /// The work the section may spend on splines, in multiplications (one evaluation of a degree 3 curve costs about 32).
    pub curve_work: u64,
    /// The most elements the stroke path, or the fill ring, may hold.
    pub path_elements: usize,
}

impl Default for Bound {
    fn default() -> Bound {
        Bound { curve_work: MAX_CURVE_WORK, path_elements: MAX_PATH_ELEMENTS }
    }
}

impl Bound {
    /// The bound held to the renderer's: neither limit is more than [`Bound::default`]'s, so that no caller can lift the caps of [`build_paths`] (
    /// re-review): [`build_paths_bounded`] takes the bound through this.
    #[must_use]
    pub fn clamped(self) -> Bound {
        let renderer = Bound::default();
        Bound { curve_work: self.curve_work.min(renderer.curve_work), path_elements: self.path_elements.min(renderer.path_elements) }
    }
}

/// The path to stroke of a section's rows, for a shape of `size` (`Width` and `Height`, which the relative rows need). See [`build_paths`].
pub fn build_path(rows: &[GeomRow], size: Size) -> Result<BezPath, GeomError> {
    Ok(build_paths(rows, size)?.stroke)
}

/// The paths of one `Geometry` section's rows, in rising `IX` order and merged, for a shape of `size`. A row that cannot be
/// drawn is skipped or drawn by the spec's fallback, and a warning says which; only a whole section that is too big, or a size that is not a
/// number, is an error.
pub fn build_paths(rows: &[GeomRow], size: Size) -> Result<BuiltPaths, GeomError> {
    build_paths_with(rows, size, rules::FILL_RUN_JOIN)
}

/// As [`build_paths`], with the fill following `join` and not `rules::FILL_RUN_JOIN`: the page's reading or another reader's. M2's render check
/// compares the two, and the stroke is the same under both.
pub fn build_paths_with(rows: &[GeomRow], size: Size, join: RunJoin) -> Result<BuiltPaths, GeomError> {
    build_paths_bounded(rows, size, join, Bound::default())
}

/// As [`build_paths_with`], with the spline work and the path size held to `bound`, which may be smaller than the renderer's and never larger ([`Bound::clamped`]) (a section that goes past it
/// is [`GeomError::PathTooLarge`], or its splines are skipped with a warning as when the work runs out: `CurveProblem::TooMuchWork`).
pub fn build_paths_bounded(rows: &[GeomRow], size: Size, join: RunJoin, bound: Bound) -> Result<BuiltPaths, GeomError> {
    let bound = bound.clamped();
    if !(size.width.is_finite() && size.height.is_finite()) {
        return Err(GeomError::BadSize);
    }
    if rows.len() > MAX_ROWS {
        return Err(GeomError::TooManyRows { rows: rows.len(), max: MAX_ROWS });
    }
    let mut builder = Builder::new(size, join, bound);
    let mut i = 0;
    while i < rows.len() {
        i += builder.row(rows, i);
        builder.check_size()?;
    }
    builder.end_path();
    let curve_work = bound.curve_work.saturating_sub(builder.work_left);
    Ok(BuiltPaths { stroke: builder.stroke, fill: builder.fill, infinite_lines: builder.lines, warnings: builder.warnings, curve_work })
}

/// One step of what a row drew, in the order drawn: the pen jumps, or a segment goes from the pen to its end.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DrawStep {
    /// The pen goes there with no mark: a move, the start of an `Ellipse`, or a spline that does not start where the pen is.
    Jump(Point),
    /// A straight segment from the pen to the point.
    Line(Point),
    /// A quadratic Bezier from the pen: its control point, then its end.
    Quad(Point, Point),
    /// A cubic Bezier from the pen: its two control points, then its end.
    Cubic(Point, Point, Point),
}

/// What one row drew, or for a spline run (a `SplineStart` and the `SplineKnot` rows after it) what the run drew.
#[derive(Clone, Debug, PartialEq)]
pub struct RowDraw {
    /// The index of the row, among the rows given, that this starts at.
    pub first: usize,
    /// How many rows it used: more than one for a spline run.
    pub rows: usize,
    /// The steps it made, none for a row that was skipped (no current point; a number that is not finite).
    pub steps: Vec<DrawStep>,
}

/// What a section's rows drew, row by row, with the work its splines cost.
#[derive(Clone, Debug, Default)]
pub struct RowDraws {
    pub rows: Vec<RowDraw>,
    /// The spline work the section spent, in the units of [`Bound::curve_work`] (as [`BuiltPaths::curve_work`]).
    pub curve_work: u64,
}

/// The rows of one `Geometry` section drawn one row at a time, for a caller that walks the path by its rows (the path functions of the
/// formula engine: a segment is a drawn row). The same steps as [`build_paths`] makes, in the same order, and the same fallbacks.
///
/// **The section is held to `bound`, the one [`build_paths_bounded`] takes**, through the same [`Bound::clamped`] (so that no caller lifts the
/// renderer's caps, and a snap's and a formula's caps are one rule): `bound.curve_work` is the most multiplications the section's splines may
/// spend, and `curve_work` of the answer says how much was used; `bound.path_elements` is the most elements either path may hold *and* the
/// most steps all the rows may make: past it the section is [`GeomError::PathTooLarge`], checked after each row, so a row that makes many (an
/// arc is at most 4,096) may pass it by that many. Where [`build_paths_bounded`] skips a spline that the work does not cover, with a warning,
/// this is [`GeomError::SplineWorkSpent`]: a path short of a curve is a wrong path.
pub fn build_row_draws(rows: &[GeomRow], size: Size, bound: Bound) -> Result<RowDraws, GeomError> {
    let bound = bound.clamped();
    if !(size.width.is_finite() && size.height.is_finite()) {
        return Err(GeomError::BadSize);
    }
    if rows.len() > MAX_ROWS {
        return Err(GeomError::TooManyRows { rows: rows.len(), max: MAX_ROWS });
    }
    let max_steps = bound.path_elements;
    let mut builder = Builder::new(size, rules::FILL_RUN_JOIN, bound);
    builder.trace = Some(Vec::new());
    let mut out: Vec<RowDraw> = Vec::new();
    let mut steps_total = 0usize;
    let mut warned = 0usize;
    let mut i = 0;
    while i < rows.len() {
        let used = builder.row(rows, i);
        builder.check_size()?;
        let spent = CurveProblem::TooMuchWork.reason();
        if builder.warnings.get(warned..).unwrap_or_default().iter().any(|w| matches!(w.kind, WarningKind::NurbsUnusable(r) if r == spent)) {
            return Err(GeomError::SplineWorkSpent);
        }
        warned = builder.warnings.len();
        let steps = builder.trace.as_mut().map(std::mem::take).unwrap_or_default();
        steps_total = steps_total.saturating_add(steps.len());
        if steps_total > max_steps {
            return Err(GeomError::PathTooLarge { max: max_steps });
        }
        out.push(RowDraw { first: i, rows: used, steps });
        i += used;
    }
    Ok(RowDraws { rows: out, curve_work: bound.curve_work.saturating_sub(builder.work_left) })
}

/// What a row does from the pen: its steps, and a warning for each fallback it made.
#[derive(Default)]
struct Drawn {
    steps: Vec<Step>,
    warnings: Vec<WarningKind>,
}

impl Drawn {
    fn line(end: Point) -> Drawn {
        Drawn::seg(Seg::Line(end))
    }

    fn seg(seg: Seg) -> Drawn {
        Drawn { steps: vec![Step::Seg(seg)], warnings: Vec::new() }
    }

    fn segs(segs: Vec<Seg>) -> Drawn {
        Drawn { steps: segs.into_iter().map(Step::Seg).collect(), warnings: Vec::new() }
    }

    fn warned(mut self, kind: WarningKind) -> Drawn {
        self.warnings.push(kind);
        self
    }
}

struct Builder {
    size: Size,
    join: RunJoin,
    stroke: BezPath,
    fill: BezPath,
    lines: Vec<InfiniteLine>,
    warnings: Vec<Warning>,
    /// The current point, or `None` before the first row and after an `Ellipse` or `InfiniteLine`.
    pen: Option<Point>,
    /// Where the current run of the stroke began, to tell whether it closes.
    run_start: Point,
    /// Whether the current run has drawn a segment.
    run_drawn: bool,
    /// Whether the fill ring is open (it has a start and has not been closed).
    ring_open: bool,
    /// What is left of the work this section may spend on splines.
    work_left: u64,
    /// The most elements either path may hold.
    max_elements: usize,
    /// The steps of the row under way, when the caller asked for them row by row ([`build_row_draws`]).
    trace: Option<Vec<DrawStep>>,
}

impl Builder {
    fn new(size: Size, join: RunJoin, bound: Bound) -> Builder {
        Builder {
            size,
            join,
            stroke: BezPath::new(),
            fill: BezPath::new(),
            lines: Vec::new(),
            warnings: Vec::new(),
            pen: None,
            run_start: Point::ORIGIN,
            run_drawn: false,
            ring_open: false,
            work_left: bound.curve_work,
            max_elements: bound.path_elements,
            trace: None,
        }
    }

    /// Notes a step of the row under way, if the steps are asked for.
    fn note(&mut self, step: DrawStep) {
        if let Some(trace) = self.trace.as_mut() {
            trace.push(step);
        }
    }

    fn warn(&mut self, row: usize, kind: WarningKind) {
        self.warnings.push(Warning { row, kind });
    }

    fn check_size(&self) -> Result<(), GeomError> {
        if self.stroke.elements().len() > self.max_elements || self.fill.elements().len() > self.max_elements {
            return Err(GeomError::PathTooLarge { max: self.max_elements });
        }
        Ok(())
    }

    /// A move that nothing follows draws nothing: it is not in the stroke path.
    fn drop_trailing_move(&mut self) {
        if matches!(self.stroke.elements().last(), Some(PathEl::MoveTo(_))) {
            self.stroke.pop();
        }
    }

    /// Ends the stroke's current run, closing it where its end and start coincide.
    fn finish_run(&mut self) {
        if let Some(pen) = self.pen
            && self.run_drawn
            && is_closed(self.run_start, pen)
        {
            self.stroke.close_path();
        }
        self.run_drawn = false;
    }

    /// Ends the fill ring: closed, or dropped when it holds a move and nothing else.
    fn close_ring(&mut self) {
        if !self.ring_open {
            return;
        }
        if matches!(self.fill.elements().last(), Some(PathEl::MoveTo(_))) {
            self.fill.pop();
        } else {
            self.fill.close_path();
        }
        self.ring_open = false;
    }

    /// Ends whatever path is under way: the run, the ring and the pen. A section's end, an `Ellipse` and an `InfiniteLine` do this.
    fn end_path(&mut self) {
        self.finish_run();
        self.close_ring();
        self.drop_trailing_move();
        self.pen = None;
    }

    /// A move: a gap in the stroke and, after the first row, a straight edge in the fill, or under the reading the end of the ring and the
    /// start of another.
    fn begin_run(&mut self, p: Point) {
        self.finish_run();
        self.drop_trailing_move();
        self.stroke.move_to(p);
        if self.ring_open && self.join == RunJoin::CloseEachRun {
            self.close_ring();
        }
        if self.ring_open {
            self.fill.line_to(p);
        } else {
            self.fill.move_to(p);
            self.ring_open = true;
        }
        self.pen = Some(p);
        self.run_start = p;
        self.note(DrawStep::Jump(p));
    }

    /// The pen goes to `p` with no mark: a gap in the stroke and a straight edge in the fill, the ring going on. A spline that does not end where
    /// the row says, and a row that cannot be drawn, do this.
    fn jump_to(&mut self, p: Point) {
        self.finish_run();
        self.drop_trailing_move();
        self.stroke.move_to(p);
        self.fill.line_to(p);
        self.pen = Some(p);
        self.run_start = p;
        self.note(DrawStep::Jump(p));
    }

    fn push_seg(&mut self, seg: Seg) {
        match seg {
            Seg::Line(p) => {
                self.stroke.line_to(p);
                self.fill.line_to(p);
                self.note(DrawStep::Line(p));
            }
            Seg::Quad(c, p) => {
                self.stroke.quad_to(c, p);
                self.fill.quad_to(c, p);
                self.note(DrawStep::Quad(c, p));
            }
            Seg::Cubic(a, b, c) => {
                self.stroke.curve_to(a, b, c);
                self.fill.curve_to(a, b, c);
                self.note(DrawStep::Cubic(a, b, c));
            }
        }
    }

    fn apply(&mut self, steps: &[Step]) {
        for step in steps {
            match *step {
                Step::Seg(seg) => {
                    self.push_seg(seg);
                    self.run_drawn = true;
                    self.pen = Some(seg.end());
                }
                Step::Jump(p) => self.jump_to(p),
            }
        }
    }

    /// The point a row's shares name.
    fn scaled(&self, x: f64, y: f64) -> Point {
        Point::new(x * self.size.width, y * self.size.height)
    }

    /// Draws row `i` and returns how many rows it used: a spline run uses several, any other row one.
    fn row(&mut self, rows: &[GeomRow], i: usize) -> usize {
        let Some(row) = rows.get(i) else { return 1 };
        // A `SplineStart` is judged with the knots after it: a number that is not finite in any of them skips the whole run.
        if matches!(row, GeomRow::SplineStart { .. }) {
            return self.spline_run(rows, i);
        }
        if !row_is_finite(row) {
            self.warn(i, WarningKind::NonFinite);
            return 1;
        }
        match row {
            GeomRow::MoveTo { x, y } => self.start(i, Point::new(*x, *y)),
            GeomRow::RelMoveTo { x, y } => self.start(i, self.scaled(*x, *y)),
            GeomRow::Ellipse { .. } => self.ellipse(i, row),
            GeomRow::InfiniteLine { x, y, other_x, other_y } => {
                self.end_path();
                self.lines.push(InfiniteLine { p0: Point::new(*x, *y), p1: Point::new(*other_x, *other_y) });
            }
            _ => self.draw_from_pen(i, row),
        }
        1
    }

    fn start(&mut self, i: usize, p: Point) {
        if p.is_finite() {
            self.begin_run(p);
        } else {
            self.warn(i, WarningKind::NonFinite);
        }
    }

    /// A row that draws from the pen: skipped, with a warning, when there is none.
    fn draw_from_pen(&mut self, i: usize, row: &GeomRow) {
        let Some(pen) = self.pen else {
            self.warn(i, WarningKind::NoPen);
            return;
        };
        let drawn = from_pen(row, pen, self.size, &mut self.work_left);
        self.commit(i, drawn);
    }

    /// Puts what a row drew on the paths, unless a number of it overflowed.
    fn commit(&mut self, i: usize, drawn: Drawn) {
        if !drawn.steps.iter().all(|s| s.is_finite()) {
            self.warn(i, WarningKind::NonFinite);
            return;
        }
        for kind in drawn.warnings {
            self.warn(i, kind);
        }
        self.apply(&drawn.steps);
    }

    /// An `Ellipse` is a path of its own, closed, and leaves no pen.
    fn ellipse(&mut self, i: usize, row: &GeomRow) {
        let GeomRow::Ellipse { x, y, first_x, first_y, second_x, second_y } = row else { return };
        let center = Point::new(*x, *y);
        let (first, second) = (Point::new(*first_x, *first_y), Point::new(*second_x, *second_y));
        let (u, v) = (first - center, second - center);
        if !(u.is_finite() && v.is_finite()) {
            self.warn(i, WarningKind::NonFinite);
            return;
        }
        // From (A, B) toward (C, D), a full turn.
        let Some(segs) = arc_segments(&Conjugate { center, u, v }, 0.0, TAU, first, first).filter(|segs| segs.iter().all(|s| s.is_finite())) else {
            self.warn(i, WarningKind::Overflow);
            return;
        };
        // The two points are read as conjugate half-diameters; flag the ones that are not on perpendicular axes.
        let (lu, lv) = (u.hypot(), v.hypot());
        if lu > 0.0 && lv > 0.0 && (u.dot(v) / (lu * lv)).abs() > rules::OBLIQUE_COSINE {
            self.warn(i, WarningKind::ObliqueEllipse);
        }
        self.end_path();
        self.note(DrawStep::Jump(first));
        for seg in &segs {
            self.note(match *seg {
                Seg::Line(p) => DrawStep::Line(p),
                Seg::Quad(c, p) => DrawStep::Quad(c, p),
                Seg::Cubic(a, b, c) => DrawStep::Cubic(a, b, c),
            });
        }
        for path in [&mut self.stroke, &mut self.fill] {
            path.move_to(first);
            for seg in &segs {
                match *seg {
                    Seg::Line(p) => path.line_to(p),
                    Seg::Quad(c, p) => path.quad_to(c, p),
                    Seg::Cubic(a, b, c) => path.curve_to(a, b, c),
                }
            }
            path.close_path();
        }
    }

    /// A `SplineStart` and the `SplineKnot` rows after it are one spline. Returns the rows used.
    fn spline_run(&mut self, rows: &[GeomRow], i: usize) -> usize {
        let mut end = i + 1;
        while matches!(rows.get(end), Some(GeomRow::SplineKnot { .. })) {
            end += 1;
        }
        let run = rows.get(i..end).unwrap_or_default();
        let Some(pen) = self.pen else {
            for k in i..end {
                self.warn(k, WarningKind::NoPen);
            }
            return end - i;
        };
        if let Some(bad) = run.iter().position(|r| !row_is_finite(r)) {
            self.warn(i + bad, WarningKind::NonFinite);
            return end - i;
        }
        let drawn = spline(pen, run, &mut self.work_left);
        self.commit(i, drawn);
        end - i
    }
}

/// The steps of a spline run from `pen`.
fn spline(pen: Point, run: &[GeomRow], budget: &mut u64) -> Drawn {
    let end = match run.last() {
        Some(GeomRow::SplineKnot { x, y, .. } | GeomRow::SplineStart { x, y, .. }) => Point::new(*x, *y),
        _ => return Drawn::default(),
    };
    if run.len() == 1 {
        // A `SplineStart` with no knot after it: a straight line to its point.
        return Drawn::line(end).warned(WarningKind::SplineStray);
    }
    match NurbsCurve::from_spline(pen, run) {
        Ok(curve) => curve_steps(&curve, pen, end, budget),
        Err(problem) => unusable(end, problem),
    }
}

/// A spline that cannot be drawn: skipped, with the pen going to `end`.
fn unusable(end: Point, problem: CurveProblem) -> Drawn {
    Drawn { steps: vec![Step::Jump(end)], warnings: vec![WarningKind::NurbsUnusable(problem.reason())] }
}

/// True if two points are the same to the rounding of the arithmetic that made them.
fn same_point(a: Point, b: Point) -> bool {
    a.distance(b) <= 1e-9 * (1.0 + a.to_vec2().hypot().max(b.to_vec2().hypot()))
}

/// A spline as steps from `pen`, ending at `end` (the row's `X`, `Y`). A clamped spline starts on the pen and ends on `end`; where it does not,
/// the pen jumps to where the curve starts, and to `end` after it (the periodic spline does not start on the pen).
fn curve_steps(curve: &NurbsCurve, pen: Point, end: Point, budget: &mut u64) -> Drawn {
    let Fitted { mut segs, within_tolerance } = match curve.segments(budget) {
        Ok(fitted) => fitted,
        Err(problem) => return unusable(end, problem),
    };
    let mut steps = Vec::new();
    let start = curve.start();
    if !same_point(start, pen) {
        steps.push(Step::Jump(start));
    }
    let ends_at_end = segs.last().is_some_and(|s| same_point(s.end(), end));
    if ends_at_end && let Some(last) = segs.pop() {
        segs.push(last.with_end(end));
    }
    steps.extend(segs.into_iter().map(Step::Seg));
    if !ends_at_end {
        steps.push(Step::Jump(end));
    }
    let mut drawn = Drawn { steps, warnings: Vec::new() };
    if !curve.is_clamped() {
        drawn = drawn.warned(WarningKind::PeriodicSpline);
    }
    if !within_tolerance {
        drawn = drawn.warned(WarningKind::CurveApproximate);
    }
    drawn
}

/// What a row draws from the pen `pen`, for a shape of `size`.
fn from_pen(row: &GeomRow, pen: Point, size: Size, budget: &mut u64) -> Drawn {
    let scale = |x: f64, y: f64| Point::new(x * size.width, y * size.height);
    match row {
        GeomRow::LineTo { x, y } => Drawn::line(Point::new(*x, *y)),
        GeomRow::RelLineTo { x, y } => Drawn::line(scale(*x, *y)),
        GeomRow::ArcTo { x, y, bow } => arc_to(pen, Point::new(*x, *y), *bow, size),
        GeomRow::EllipticalArcTo { x, y, mid_x, mid_y, angle, ratio } => {
            elliptical(pen, Point::new(*x, *y), Point::new(*mid_x, *mid_y), (*angle, *ratio), size)
        }
        GeomRow::RelEllipticalArcTo { x, y, mid_x, mid_y, angle, ratio } => {
            elliptical(pen, scale(*x, *y), scale(*mid_x, *mid_y), (*angle, *ratio), size)
        }
        GeomRow::PolylineTo { x, y, polyline } => polyline_to(Point::new(*x, *y), polyline.as_ref(), size),
        GeomRow::NurbsTo { x, y, .. } => match NurbsCurve::from_nurbs_to(pen, size, row) {
            Ok(curve) => curve_steps(&curve, pen, Point::new(*x, *y), budget),
            Err(problem) => unusable(Point::new(*x, *y), problem),
        },
        GeomRow::SplineKnot { x, y, .. } => Drawn::line(Point::new(*x, *y)).warned(WarningKind::SplineStray),
        GeomRow::RelCubBezTo { x, y, c1_x, c1_y, c2_x, c2_y } => Drawn::seg(Seg::Cubic(scale(*c1_x, *c1_y), scale(*c2_x, *c2_y), scale(*x, *y))),
        GeomRow::RelQuadBezTo { x, y, c_x, c_y } => Drawn::seg(Seg::Quad(scale(*c_x, *c_y), scale(*x, *y))),
        _ => Drawn::default(),
    }
}

/// An arc whose arithmetic overflowed: the chord, and a warning (the other fallbacks of an arc say nothing: they are the spec's rules for rows that exist).
fn overflow(end: Point) -> Drawn {
    Drawn::line(end).warned(WarningKind::Overflow)
}

/// `ArcTo`: the circular arc, or a line to the end where there is no arc, with a warning where the arithmetic overflows.
fn arc_to(pen: Point, end: Point, bow: f64, size: Size) -> Drawn {
    match circular_arc(pen, end, bow, size) {
        Ok(arc) => match arc_segments(&Conjugate::circle(arc.center, arc.radius), arc.start_angle, arc.sweep, pen, end) {
            Some(segs) => Drawn::segs(segs),
            None => overflow(end),
        },
        Err(ArcFallback::Overflow) => overflow(end),
        Err(_) => Drawn::line(end),
    }
}

/// `EllipticalArcTo` and `RelEllipticalArcTo` with their points already in the shape's frame: the arc, or the chord where the
/// points fix no ellipse or the ratio is unreliable; and on a ratio of 0 the chord, or on a collapsed shape the point on the arc
/// first.
fn elliptical(pen: Point, end: Point, mid: Point, (angle, ratio): (f64, f64), size: Size) -> Drawn {
    match elliptical_arc(pen, mid, end, angle, ratio) {
        Ok(arc) => match arc_segments(&Conjugate::ellipse(&arc), arc.start_angle, arc.sweep, pen, end) {
            Some(segs) => Drawn::segs(segs),
            None => overflow(end),
        },
        Err(ArcFallback::Overflow) => overflow(end),
        Err(ArcFallback::Ratio) if ratio == 0.0 => {
            let collapsed = size.width == 0.0 || size.height == 0.0;
            if collapsed { Drawn::segs(vec![Seg::Line(mid), Seg::Line(end)]) } else { Drawn::line(end) }
        }
        Err(ArcFallback::Ratio) => Drawn::line(end).warned(WarningKind::EllipticalRatio),
        Err(_) => Drawn::line(end),
    }
}

/// `PolylineTo`: straight segments through each pair, then to the end. A constructor that cannot be read is a line to the end.
fn polyline_to(end: Point, polyline: Option<&Polyline>, size: Size) -> Drawn {
    let Some(polyline) = polyline.filter(|p| p.points.len() <= MAX_CONSTRUCTOR_POINTS) else {
        return Drawn::line(end).warned(WarningKind::PolylineUnreadable);
    };
    let point = |&(x, y): &(f64, f64)| {
        let x = if polyline.x_is_share { x * size.width } else { x };
        let y = if polyline.y_is_share { y * size.height } else { y };
        Point::new(x, y)
    };
    Drawn::segs(polyline.points.iter().map(point).chain(std::iter::once(end)).map(Seg::Line).collect())
}

/// True if every number the row holds is finite, constructors included.
fn row_is_finite(row: &GeomRow) -> bool {
    let numbers: Vec<f64> = match row {
        GeomRow::MoveTo { x, y } | GeomRow::RelMoveTo { x, y } | GeomRow::LineTo { x, y } | GeomRow::RelLineTo { x, y } => vec![*x, *y],
        GeomRow::ArcTo { x, y, bow } => vec![*x, *y, *bow],
        GeomRow::EllipticalArcTo { x, y, mid_x, mid_y, angle, ratio } | GeomRow::RelEllipticalArcTo { x, y, mid_x, mid_y, angle, ratio } => {
            vec![*x, *y, *mid_x, *mid_y, *angle, *ratio]
        }
        GeomRow::PolylineTo { x, y, polyline } => {
            let mut v = vec![*x, *y];
            v.extend(polyline.iter().flat_map(|p| p.points.iter().flat_map(|(a, b)| [*a, *b])));
            v
        }
        GeomRow::NurbsTo { x, y, second_last_knot, last_weight, first_knot, first_weight, nurbs } => {
            let mut v = vec![*x, *y, *second_last_knot, *last_weight, *first_knot, *first_weight];
            if let Some(n) = nurbs {
                v.extend([n.last_knot, n.degree]);
                v.extend(n.points.iter().flat_map(|p| [p.x, p.y, p.knot, p.weight]));
            }
            v
        }
        GeomRow::SplineStart { x, y, second_knot, first_knot, last_knot, degree } => vec![*x, *y, *second_knot, *first_knot, *last_knot, *degree],
        GeomRow::SplineKnot { x, y, knot } => vec![*x, *y, *knot],
        GeomRow::RelCubBezTo { x, y, c1_x, c1_y, c2_x, c2_y } => vec![*x, *y, *c1_x, *c1_y, *c2_x, *c2_y],
        GeomRow::RelQuadBezTo { x, y, c_x, c_y } => vec![*x, *y, *c_x, *c_y],
        GeomRow::Ellipse { x, y, first_x, first_y, second_x, second_y } => vec![*x, *y, *first_x, *first_y, *second_x, *second_y],
        GeomRow::InfiniteLine { x, y, other_x, other_y } => vec![*x, *y, *other_x, *other_y],
    };
    numbers.iter().all(|n| n.is_finite())
}
