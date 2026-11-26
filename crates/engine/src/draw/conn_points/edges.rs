//! The edges of a shape's outline and the connection points on each (of the connection-point doubling design, §1 and §2).
//!
//! **The outline** is what the shape's `Geometry` sections draw, read as the renderer and the path functions read it: the effective rows of each section (inherited
//! from the master), turned into `diacraft_geom` rows by `diacraft_render::geom_rows` and drawn one row at a time by
//! `diacraft_geom::build_row_draws`, in the shape's own frame. A section whose `NoShow` is 1 draws nothing and gives no edge; `NoFill` and `NoLine` do not matter.
//!
//! **The edges** (§1.2): a straight step is an edge of its own; the curved steps of one row are one curved edge; a run of curved rows from one move that ends where it
//! began and turns nowhere (every join within [`CORNER_TURN`] degrees, the closing one too) is one closed edge, and so is an `Ellipse` row. An edge shorter than
//! [`MIN_EDGE`] is no edge. Edges are numbered from 0, the sections in `IX` order and then in the order drawn.
//!
//! **The points** (§2): a row of the `Connection` section is a point of an edge when it lies within [`ON_EDGE`] of it (and, on an open edge, farther than that from
//! both its ends), is an ordinary row (no formula in `D`) and takes connectors (`Type` 0 or 2). A row near two edges is the nearer's, and the lower-numbered's of two
//! as near. Every other row is kept and on no edge: a centre point, a point on a corner, an outward point, an extended row.
//!
//! **What it costs.** Everything is charged to the call's [`OutlineWork`] before it is done, by the units the loops walk: the rows of a section (counted over the
//! shape, its master and the styles before any is merged), the steps drawn, the spline work of `diacraft_geom::Bound`, and the pieces a point is tried against or a
//! length is worked out for (arc calls). A call that spends one of them stops there: the shape that met the limit and every shape after it are refused in words.

use diacraft_doc::{CellAddr, EffectiveRow, FormulaSlot, RowKey, SectionKey, ShapeRef, SheetRef};
use diacraft_geom::kurbo::{ParamCurve, ParamCurveArclen, ParamCurveNearest, PathSeg, Point, Rect, Size, Vec2};
use diacraft_geom::{Bound, DrawStep, GeomError, MAX_ROWS, RowDraw, build_row_draws, is_closed};

use super::{MAX_CONNECTION_ROWS_READ, Skip};
use crate::draw::Reader;

/// How far a connection point may lie from an edge and belong to it: a thousandth of an inch in the shape's own frame, the tolerance within which the files' point-glued
/// ends lie on their rows (26,649 of 26,649).
pub const ON_EDGE: f64 = 0.001;

/// The slack the comparison with [`ON_EDGE`] allows for the floating-point places a distance is worked out from: a point exactly a thousandth off is on.
const ON_EDGE_SLACK: f64 = 1.0e-12;

/// An edge shorter than this, in inches of the shape's own frame, is no edge.
pub const MIN_EDGE: f64 = 1.0e-6;

/// How far, in degrees, two curved pieces may turn at their join for the join to be smooth: a run of curved rows is one closed edge only when every join is.
pub const CORNER_TURN: f64 = 1.0;

/// The most edges one shape lists or changes: past it the query says `truncated`.
pub const MAX_EDGES_PER_SHAPE: usize = 4_096;

/// The most `Geometry` sections of one shape that are read, counted over the shape, its master and the styles before any is merged.
pub const MAX_OUTLINE_SECTIONS: usize = 64;

/// The most rows of `Geometry` sections that one call merges, over all its shapes (**ours**: the plan's table bounds steps, and a row that draws nothing costs a merge
/// and no step).
pub const MAX_OUTLINE_ROWS: usize = 1_000_000;

/// The most steps one call draws, over all its shapes (`diacraft_geom::Bound::path_elements`).
pub const MAX_OUTLINE_STEPS: usize = 1_000_000;

/// The most spline work, in multiplications, one call spends, over all its shapes (`diacraft_geom::Bound::curve_work`).
pub const MAX_OUTLINE_WORK: u64 = 50_000_000;

/// The most arc calls one call makes, over all its shapes: a length worked out, a place found by length and a piece tried against a point, each one.
pub const MAX_ARC_CALLS: usize = 1_000_000;

/// The most points a [`Edge::sample`] gives, for a window to pick and draw an edge by.
pub const MAX_PATH_POINTS: usize = 32;

/// How close two distances must be to tie, in inches: the earlier edge or piece wins a tie.
const TIE: f64 = 1.0e-12;

/// A length as a share of the section's extent that counts as nothing (a curve's accuracy), as `calc`'s path walk takes it, so that this reading and the path functions'
/// agree far inside the 1E-6 the doubling's tests hold them to.
const LENGTH_ACCURACY: f64 = 1.0e-11;

/// A closed edge's place within this many inches of its length is its start (a place a hair short of the end is the start of the curve).
const CLOSE_SLACK: f64 = 1.0e-9;

/// What an edge is (§1.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeKind {
    /// A straight step: a `LineTo`, an `ArcTo` whose bow is 0, a piece of a `PolylineTo` or of a degree 1 `NURBSTo`.
    Straight,
    /// The curved steps of one row: an arc, a spline, a Bézier.
    Curved,
    /// A whole closed curve: an `Ellipse` row, or curved rows that end where they began and turn nowhere.
    Closed,
}

/// The limit of a call that was spent (see [`OutlineWork`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Spent {
    Rows,
    Steps,
    Work,
    ArcCalls,
}

impl Spent {
    /// What the call ran out of, in words.
    pub fn words(self) -> String {
        match self {
            Spent::Rows => format!("the call has read the most rows of outline it may ({MAX_OUTLINE_ROWS}): ask for fewer shapes"),
            Spent::Steps => format!("the call has drawn the most steps of outline it may ({MAX_OUTLINE_STEPS}): ask for fewer shapes"),
            Spent::Work => format!("the call has spent the most spline work on outlines it may ({MAX_OUTLINE_WORK}): ask for fewer shapes"),
            Spent::ArcCalls => {
                format!("the call has made the most arc length and nearest place calls on outlines it may ({MAX_ARC_CALLS}): ask for fewer shapes")
            }
        }
    }
}

/// What a call has spent reading outlines, over all its shapes: the counts its limits cap, which tests read and which stop a hostile shape. A limit spent is
/// [`OutlineWork::stopped`], and every later charge of the call is refused with it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OutlineWork {
    /// Rows of `Geometry` sections merged, counted over the shape, its master and the styles and charged before any was merged.
    pub rows: usize,
    /// Steps drawn.
    pub steps: usize,
    /// Spline multiplications spent.
    pub curve_work: u64,
    /// Arc calls: a length worked out for a curved piece, a place found by length, a piece tried against a point.
    pub arc_calls: usize,
    /// `Connection` rows read, counted over the shape, its master and the styles and charged before any was merged.
    pub point_rows: usize,
    /// The limit that was spent, if one was.
    pub stopped: Option<Spent>,
}

impl OutlineWork {
    fn check(&self) -> Result<(), Skip> {
        self.stopped.map_or(Ok(()), |what| Err(Skip::Call(what)))
    }

    /// The call is out of `what`: what was left is gone, and so is every later charge.
    fn stop(&mut self, what: Spent) -> Skip {
        match what {
            Spent::Rows => self.rows = MAX_OUTLINE_ROWS,
            Spent::Steps => self.steps = MAX_OUTLINE_STEPS,
            Spent::Work => self.curve_work = MAX_OUTLINE_WORK,
            Spent::ArcCalls => self.arc_calls = MAX_ARC_CALLS,
        }
        self.stopped = Some(what);
        Skip::Call(what)
    }

    fn charge_rows(&mut self, rows: usize) -> Result<(), Skip> {
        self.check()?;
        if self.rows.saturating_add(rows) > MAX_OUTLINE_ROWS {
            return Err(self.stop(Spent::Rows));
        }
        self.rows += rows;
        Ok(())
    }

    fn charge_arcs(&mut self, calls: usize) -> Result<(), Skip> {
        self.check()?;
        if self.arc_calls.saturating_add(calls) > MAX_ARC_CALLS {
            return Err(self.stop(Spent::ArcCalls));
        }
        self.arc_calls += calls;
        Ok(())
    }
}

/// One piece of an edge: a line, a quadratic or a cubic, with the length before it and its own.
#[derive(Clone, Copy, Debug)]
struct Piece {
    seg: PathSeg,
    start: f64,
    len: f64,
}

/// A step of a run as drawn, before its length is known; `draw` is the index of the row draw that made it.
#[derive(Clone, Copy)]
struct Raw {
    seg: PathSeg,
    draw: usize,
}

/// An edge of a shape's outline, in the shape's own frame.
#[derive(Clone, Debug)]
pub(crate) struct Edge {
    /// Its number, from 0.
    pub number: u32,
    pub kind: EdgeKind,
    /// The `IX` of the `Geometry` section it comes from (`Geometry<IX + 1>` is the reference's spelling), if the section has one.
    pub section_ix: Option<u32>,
    /// The first and last row (their `IX`, or their place in a section keyed by name) that drew it.
    pub rows: (u32, u32),
    /// How many steps it draws.
    pub steps: usize,
    /// Its length in the shape's own units.
    pub length: f64,
    pieces: Vec<Piece>,
    /// The box round every piece's control points.
    bounds: Rect,
    /// The accuracy lengths and places are worked out to: a tiny share of the section's extent.
    accuracy: f64,
}

/// The nearest place of an edge to a point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Near {
    pub distance: f64,
    /// How far along the edge, in its own length.
    pub along: f64,
}

/// What a shape's outline is made of.
#[derive(Clone, Debug, Default)]
pub(crate) struct Outline {
    pub edges: Vec<Edge>,
    /// Whether the shape has more edges than [`MAX_EDGES_PER_SHAPE`]: those listed are the first.
    pub truncated: bool,
}

/// A row of the `Connection` section, in the shape's own frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PointRow {
    /// The row's `IX` (the first row is 0), or its place in a section keyed by name: the number `Connections.X<index + 1>` names and `ToPart` is 100 plus.
    pub index: u32,
    pub x: f64,
    pub y: f64,
    /// `Type`: 0 inward, 1 outward, 2 both.
    pub kind: f64,
    /// Whether the row is extended: a formula in `D`.
    pub extended: bool,
}

/// A point of an edge, and where along it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct OnEdge {
    pub index: u32,
    pub along: f64,
}

/// The rows of the `Connection` section sorted by the edge each belongs to.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct OnEdges {
    /// For each edge of the outline, its points in their order along it (the lower row first at one place).
    pub edges: Vec<Vec<OnEdge>>,
    /// The rows on no edge, in row order: kept as they are.
    pub other: Vec<u32>,
}

// ---- the rows -------------------------------------------------------------------------------------------------------------------------

fn cell_number(row: &EffectiveRow<'_>, name: &str) -> f64 {
    row.cells
        .iter()
        .find(|(n, _)| n.as_str() == name)
        .and_then(|(_, found)| found.value.cell)
        .and_then(|c| c.value.as_ref())
        .and_then(|v| v.number())
        .unwrap_or(0.0)
}

fn row_number(row: &EffectiveRow<'_>, position: usize) -> u32 {
    match &row.key {
        RowKey::Ix(ix) => *ix,
        RowKey::Name(_) => u32::try_from(position).unwrap_or(u32::MAX),
    }
}

/// Whether the row has a formula in `D`: the sign of an extended row.
fn has_d_formula(row: &EffectiveRow<'_>) -> bool {
    row.cells.iter().any(|(n, found)| n.as_str() == "D" && found.formula.cell.is_some_and(|c| matches!(c.formula, FormulaSlot::Local(_))))
}

/// The `Connection` rows of the shape `at` in its own frame, within [`MAX_CONNECTION_ROWS_READ`], counted over the shape, its master and the styles before any is
/// merged: a shape with more is [`Skip::Shape`], and none is merged or charged.
pub(crate) fn point_rows(reader: &Reader<'_>, at: &ShapeRef, work: &mut OutlineWork) -> Result<Vec<PointRow>, Skip> {
    let held = reader.connection_rows(at);
    if held > MAX_CONNECTION_ROWS_READ {
        let id = reader.doc().shape(at).map_or(0, |s| s.id);
        return Err(Skip::Shape(format!(
            "shape {id} has more than {MAX_CONNECTION_ROWS_READ} connection points, counted over the shape and its master: it is not read"
        )));
    }
    work.point_rows = work.point_rows.saturating_add(held);
    let sheet = SheetRef::Shape(at.clone());
    let Some(Ok(rows)) = reader.resolver().effective_rows_within(&sheet, &SectionKey::new("Connection", None), held) else {
        return Ok(Vec::new());
    };
    Ok(rows
        .iter()
        .enumerate()
        .map(|(position, row)| PointRow {
            index: row_number(row, position),
            x: cell_number(row, "X"),
            y: cell_number(row, "Y"),
            kind: cell_number(row, "Type"),
            extended: has_d_formula(row),
        })
        .collect())
}

// ---- the outline ----------------------------------------------------------------------------------------------------------------------

/// What one section is read with.
struct Section<'a> {
    id: u32,
    ix: Option<u32>,
    draws: &'a [RowDraw],
    /// The number of each geometry row (their `IX`), by the row's place among the rows drawn.
    numbers: &'a [u32],
    /// Whether each geometry row is an `Ellipse`.
    ellipse: &'a [bool],
    /// The section's extent, and the accuracy its curves' lengths are worked out to (a tiny share of it).
    extent: f64,
    accuracy: f64,
}

/// Whether to go on with the next run.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Flow {
    Go,
    Stop,
}

fn seg_of(pen: Point, step: &DrawStep) -> Option<(PathSeg, Point)> {
    use diacraft_geom::kurbo::{CubicBez, Line, QuadBez};
    let (seg, end) = match *step {
        DrawStep::Jump(_) => return None,
        DrawStep::Line(e) => (PathSeg::Line(Line::new(pen, e)), e),
        DrawStep::Quad(c, e) => (PathSeg::Quad(QuadBez::new(pen, c, e)), e),
        DrawStep::Cubic(a, b, e) => (PathSeg::Cubic(CubicBez::new(pen, a, b, e)), e),
    };
    (pen.is_finite() && end.is_finite()).then_some((seg, end))
}

fn is_line(seg: &PathSeg) -> bool {
    matches!(seg, PathSeg::Line(_))
}

/// The length of a piece. A curve whose control polygon has no length is 0: kurbo's `arclen` of a quadratic whose three points coincide is not a number (the same
/// rule as the path walk's).
fn piece_len(seg: &PathSeg, accuracy: f64) -> f64 {
    let polygon = match seg {
        PathSeg::Line(l) => return l.p0.distance(l.p1),
        PathSeg::Quad(q) => q.p0.distance(q.p1) + q.p1.distance(q.p2),
        PathSeg::Cubic(c) => c.p0.distance(c.p1) + c.p1.distance(c.p2) + c.p2.distance(c.p3),
    };
    if polygon <= accuracy { 0.0 } else { seg.arclen(accuracy) }
}

/// The box round a piece's control points: a curve lies inside it.
fn seg_box(seg: &PathSeg) -> Rect {
    let points: [Option<Point>; 4] = match seg {
        PathSeg::Line(l) => [Some(l.p0), Some(l.p1), None, None],
        PathSeg::Quad(q) => [Some(q.p0), Some(q.p1), Some(q.p2), None],
        PathSeg::Cubic(c) => [Some(c.p0), Some(c.p1), Some(c.p2), Some(c.p3)],
    };
    let mut found = points.into_iter().flatten();
    let first = found.next().unwrap_or(Point::ORIGIN);
    found.fold(Rect::from_points(first, first), |r, p| r.union_pt(p))
}

/// Whether `p` is within `reach` of the box (the box grown by `reach` on every side, its edges included).
fn near_box(r: &Rect, p: Point, reach: f64) -> bool {
    p.x >= r.x0 - reach && p.x <= r.x1 + reach && p.y >= r.y0 - reach && p.y <= r.y1 + reach
}

/// The place on a piece nearest `p`: its parameter and its distance.
fn nearest_on(seg: &PathSeg, p: Point, accuracy: f64) -> (f64, f64) {
    match seg {
        PathSeg::Line(l) => {
            let d = l.p1 - l.p0;
            let len2 = d.hypot2();
            let t = if len2 > 0.0 { ((p - l.p0).dot(d) / len2).clamp(0.0, 1.0) } else { 0.0 };
            (t, p.distance(l.p0 + d * t))
        }
        curve => {
            let near = curve.nearest(p, accuracy);
            (near.t.clamp(0.0, 1.0), near.distance_sq.max(0.0).sqrt())
        }
    }
}

/// The unit direction of a piece at its start or its end, where the piece has one: the first control point that is not on the end, as the path walk takes it.
fn tangent(seg: &PathSeg, at_start: bool, floor: f64) -> Option<Vec2> {
    let candidates: [Option<Vec2>; 3] = match (seg, at_start) {
        (PathSeg::Line(l), _) => [Some(l.p1 - l.p0), None, None],
        (PathSeg::Quad(q), true) => [Some(q.p1 - q.p0), Some(q.p2 - q.p0), None],
        (PathSeg::Quad(q), false) => [Some(q.p2 - q.p1), Some(q.p2 - q.p0), None],
        (PathSeg::Cubic(c), true) => [Some(c.p1 - c.p0), Some(c.p2 - c.p0), Some(c.p3 - c.p0)],
        (PathSeg::Cubic(c), false) => [Some(c.p3 - c.p2), Some(c.p3 - c.p1), Some(c.p3 - c.p0)],
    };
    candidates.into_iter().flatten().find_map(|v| {
        let l = v.hypot();
        (l.is_finite() && l > floor).then(|| v / l)
    })
}

/// Whether every join of a run of curved pieces, the closing one too, turns by no more than [`CORNER_TURN`]. A piece with no direction (a curve that stops at a point)
/// is passed over; a run of such pieces only is not smooth.
fn smooth_all_round(run: &[Raw], floor: f64) -> bool {
    let ends: Vec<(Vec2, Vec2)> = run.iter().filter_map(|p| Some((tangent(&p.seg, true, floor)?, tangent(&p.seg, false, floor)?))).collect();
    let cos_turn = CORNER_TURN.to_radians().cos();
    !ends.is_empty() && ends.iter().zip(ends.iter().cycle().skip(1)).all(|((_, end), (start, _))| end.dot(*start) >= cos_turn)
}

/// The section's extent, for the accuracy of its curves: the larger of its size and its reach from the origin, and at least 1 (as the path walk takes it).
fn extent_of(draws: &[RowDraw]) -> f64 {
    let (mut lo, mut hi) = (Point::new(f64::INFINITY, f64::INFINITY), Point::new(f64::NEG_INFINITY, f64::NEG_INFINITY));
    let mut grow = |p: Point| {
        lo = Point::new(lo.x.min(p.x), lo.y.min(p.y));
        hi = Point::new(hi.x.max(p.x), hi.y.max(p.y));
    };
    for step in draws.iter().flat_map(|d| d.steps.iter()) {
        match *step {
            DrawStep::Jump(p) | DrawStep::Line(p) => grow(p),
            DrawStep::Quad(c, e) => {
                grow(c);
                grow(e);
            }
            DrawStep::Cubic(a, b, e) => {
                grow(a);
                grow(b);
                grow(e);
            }
        }
    }
    let size = (hi.x - lo.x).max(hi.y - lo.y);
    let reach = lo.x.abs().max(lo.y.abs()).max(hi.x.abs()).max(hi.y.abs());
    let s = size.max(reach);
    if s.is_finite() { s.max(1.0) } else { 1.0 }
}

/// Where the next run of the draws is: the run is pen-continuous, from one move (or the start of an `Ellipse`) to the next.
#[derive(Default)]
struct Cursor {
    draw: usize,
    step: usize,
    pen: Option<Point>,
}

/// The next run of the section's pieces, or `None` at its end. A step that needs a pen when there is none is passed over, as the drawing passes it over.
fn next_run(draws: &[RowDraw], c: &mut Cursor) -> Option<Vec<Raw>> {
    let mut run: Vec<Raw> = Vec::new();
    while let Some(draw) = draws.get(c.draw) {
        let Some(step) = draw.steps.get(c.step) else {
            c.draw += 1;
            c.step = 0;
            continue;
        };
        c.step += 1;
        if let DrawStep::Jump(p) = *step {
            c.pen = Some(p);
            if !run.is_empty() {
                return Some(run);
            }
            continue;
        }
        let Some((seg, to)) = c.pen.and_then(|pen| seg_of(pen, step)) else { continue };
        run.push(Raw { seg, draw: c.draw });
        c.pen = Some(to);
    }
    (!run.is_empty()).then_some(run)
}

/// What the edges of a section are collected in.
struct Builder {
    edges: Vec<Edge>,
    truncated: bool,
}

impl Builder {
    /// The first and last row of the draws the pieces came from.
    fn rows_of(pieces: &[Raw], sect: &Section<'_>) -> (u32, u32) {
        let number = |index: Option<usize>| index.and_then(|i| sect.numbers.get(i)).copied().unwrap_or(0);
        let first = pieces.first().and_then(|p| sect.draws.get(p.draw)).map(|d| d.first);
        let last = pieces.last().and_then(|p| sect.draws.get(p.draw)).map(|d| d.first.saturating_add(d.rows).saturating_sub(1));
        (number(first), number(last))
    }

    /// Whether the run is one closed edge (§1.2 rule 3): an `Ellipse` row, or only curved pieces that end where they began and turn nowhere.
    fn closed_curve(run: &[Raw], sect: &Section<'_>) -> bool {
        let (Some(first), Some(last)) = (run.first(), run.last()) else { return false };
        if run.iter().any(|p| is_line(&p.seg)) || !is_closed(first.seg.start(), last.seg.end()) {
            return false;
        }
        let is_ellipse = |p: &Raw| sect.draws.get(p.draw).and_then(|d| sect.ellipse.get(d.first)).copied().unwrap_or(false);
        run.iter().all(is_ellipse) || smooth_all_round(run, 1.0e-12 * sect.extent)
    }

    /// Makes the edges of one run: a closed curve, or each straight step and each row's curved steps.
    fn emit_run(&mut self, run: &[Raw], sect: &Section<'_>, work: &mut OutlineWork) -> Result<Flow, Skip> {
        if Self::closed_curve(run, sect) {
            return self.emit(EdgeKind::Closed, run, sect, work);
        }
        let mut i = 0;
        while let Some(piece) = run.get(i) {
            let line = is_line(&piece.seg);
            let mut end = i + 1;
            while !line && run.get(end).is_some_and(|next| !is_line(&next.seg) && next.draw == piece.draw) {
                end += 1;
            }
            let kind = if line { EdgeKind::Straight } else { EdgeKind::Curved };
            if self.emit(kind, run.get(i..end).unwrap_or_default(), sect, work)? == Flow::Stop {
                return Ok(Flow::Stop);
            }
            i = end;
        }
        Ok(Flow::Go)
    }

    /// Adds the edge the pieces make, unless it is shorter than [`MIN_EDGE`]; stops at [`MAX_EDGES_PER_SHAPE`] with `truncated` set.
    fn emit(&mut self, kind: EdgeKind, pieces: &[Raw], sect: &Section<'_>, work: &mut OutlineWork) -> Result<Flow, Skip> {
        work.charge_arcs(pieces.iter().filter(|p| !is_line(&p.seg)).count())?;
        let mut measured: Vec<Piece> = Vec::with_capacity(pieces.len());
        let mut at = 0.0_f64;
        let mut bounds: Option<Rect> = None;
        for raw in pieces {
            let len = piece_len(&raw.seg, sect.accuracy);
            if !len.is_finite() || len < 0.0 {
                return Err(Skip::Shape(format!("shape {}: a length of its outline is not a number", sect.id)));
            }
            measured.push(Piece { seg: raw.seg, start: at, len });
            at += len;
            let b = seg_box(&raw.seg);
            bounds = Some(bounds.map_or(b, |u| u.union(b)));
        }
        if !at.is_finite() {
            return Err(Skip::Shape(format!("shape {}: a length of its outline is not a number", sect.id)));
        }
        if at < MIN_EDGE {
            return Ok(Flow::Go);
        }
        if self.edges.len() >= MAX_EDGES_PER_SHAPE {
            self.truncated = true;
            return Ok(Flow::Stop);
        }
        self.edges.push(Edge {
            number: u32::try_from(self.edges.len()).unwrap_or(u32::MAX),
            kind,
            section_ix: sect.ix,
            rows: Self::rows_of(pieces, sect),
            steps: pieces.len(),
            length: at,
            pieces: measured,
            bounds: bounds.unwrap_or_else(|| Rect::from_points(Point::ORIGIN, Point::ORIGIN)),
            accuracy: sect.accuracy,
        });
        Ok(Flow::Go)
    }
}

/// Reads one `Geometry` section of the shape and adds its edges.
fn read_section(
    reader: &Reader<'_>,
    at: &ShapeRef,
    id: u32,
    key: &SectionKey,
    size: Size,
    work: &mut OutlineWork,
    out: &mut Builder,
) -> Result<Flow, Skip> {
    let sheet = SheetRef::Shape(at.clone());
    let res = reader.resolver();
    if reader.addr_number(&sheet, &CellAddr::in_section("Geometry", key.ix, "NoShow")) != 0.0 {
        return Ok(Flow::Go);
    }
    // The rows are counted over the layers and charged before any is merged.
    let raw = res.rows_held(&sheet, key).unwrap_or(0);
    if raw == 0 {
        return Ok(Flow::Go);
    }
    if raw > MAX_ROWS {
        return Err(Skip::Shape(format!("shape {id} has a Geometry section of {raw} rows, more than the {MAX_ROWS} that are read")));
    }
    work.charge_rows(raw)?;
    let Some(Ok(rows)) = res.effective_rows_within(&sheet, key, raw) else { return Ok(Flow::Go) };
    let (mut geom, mut numbers, mut ellipse) = (Vec::with_capacity(rows.len()), Vec::with_capacity(rows.len()), Vec::with_capacity(rows.len()));
    for (position, row) in rows.iter().enumerate() {
        // A row of a type nobody lists draws nothing and has no place among the rows drawn.
        let (found, _) = diacraft_render::geom_rows(std::slice::from_ref(row));
        if let Some(g) = found.into_iter().next() {
            geom.push(g);
            numbers.push(row_number(row, position));
            ellipse.push(row.row_type.as_deref() == Some("Ellipse"));
        }
    }
    let bound = Bound { curve_work: MAX_OUTLINE_WORK.saturating_sub(work.curve_work), path_elements: MAX_OUTLINE_STEPS.saturating_sub(work.steps) };
    let drawn = match build_row_draws(&geom, size, bound) {
        Ok(drawn) => drawn,
        Err(GeomError::PathTooLarge { .. }) => return Err(work.stop(Spent::Steps)),
        Err(GeomError::SplineWorkSpent) => return Err(work.stop(Spent::Work)),
        Err(other) => return Err(Skip::Shape(format!("shape {id}: its outline cannot be drawn: {other}"))),
    };
    let steps: usize = drawn.rows.iter().map(|r| r.steps.len()).fold(0, usize::saturating_add);
    work.steps = work.steps.saturating_add(steps);
    work.curve_work = work.curve_work.saturating_add(drawn.curve_work);
    let extent = extent_of(&drawn.rows);
    let sect = Section { id, ix: key.ix, draws: &drawn.rows, numbers: &numbers, ellipse: &ellipse, extent, accuracy: LENGTH_ACCURACY * extent };
    let mut cursor = Cursor::default();
    while let Some(run) = next_run(sect.draws, &mut cursor) {
        if out.emit_run(&run, &sect, work)? == Flow::Stop {
            return Ok(Flow::Stop);
        }
    }
    Ok(Flow::Go)
}

/// The outline of the shape `at`: its edges, in the shape's own frame. A shape with more sections, rows or steps than the caps allow is [`Skip::Shape`] or, where the
/// call's own limit was spent, [`Skip::Call`].
pub(crate) fn outline_of(reader: &Reader<'_>, at: &ShapeRef, work: &mut OutlineWork) -> Result<Outline, Skip> {
    work.check()?;
    let sheet = SheetRef::Shape(at.clone());
    let id = reader.doc().shape(at).map_or(0, |s| s.id);
    let keys = match reader.resolver().effective_sections_within(&sheet, "Geometry", MAX_OUTLINE_SECTIONS) {
        Some(Ok(keys)) => keys,
        Some(Err(count)) => {
            return Err(Skip::Shape(format!(
                "shape {id} has more than {MAX_OUTLINE_SECTIONS} Geometry sections ({count}, counted over the shape and its master): it is not read"
            )));
        }
        None => return Ok(Outline::default()),
    };
    let size = Size::new(reader.number(&sheet, "Width"), reader.number(&sheet, "Height"));
    let mut out = Builder { edges: Vec::new(), truncated: false };
    for key in &keys {
        if read_section(reader, at, id, key, size, work, &mut out)? == Flow::Stop {
            break;
        }
    }
    Ok(Outline { edges: out.edges, truncated: out.truncated })
}

// ---- places on an edge ----------------------------------------------------------------------------------------------------------------

impl Edge {
    pub(crate) fn is_closed(&self) -> bool {
        self.kind == EdgeKind::Closed
    }

    /// Where the edge starts: the first point drawn.
    pub(crate) fn start(&self) -> Point {
        self.pieces.first().map_or(Point::ORIGIN, |p| p.seg.start())
    }

    /// Where the edge ends: its start again for a closed edge.
    pub(crate) fn end(&self) -> Point {
        self.pieces.last().map_or(Point::ORIGIN, |p| p.seg.end())
    }

    /// The place `along` the edge, clamped to it. A curved piece costs an arc call.
    pub(crate) fn point_at(&self, along: f64, work: &mut OutlineWork) -> Result<Point, Skip> {
        let along = if along.is_finite() { along.clamp(0.0, self.length) } else { 0.0 };
        let index = self.pieces.partition_point(|p| p.start + p.len < along);
        let Some(piece) = self.pieces.get(index).or_else(|| self.pieces.last()) else { return Ok(Point::ORIGIN) };
        let local = (along - piece.start).clamp(0.0, piece.len);
        if piece.len <= 0.0 {
            return Ok(piece.seg.start());
        }
        let t = if is_line(&piece.seg) {
            local / piece.len
        } else {
            work.charge_arcs(1)?;
            piece.seg.inv_arclen(local, self.accuracy).clamp(0.0, 1.0)
        };
        Ok(piece.seg.eval(t))
    }

    /// `count` places along the edge at equal lengths, for a window to pick and draw the edge by: both ends of an open edge, and the places `i / count` of a closed
    /// one (which the window joins to the first). A straight edge gives its two ends.
    pub(crate) fn sample(&self, count: usize, work: &mut OutlineWork) -> Result<Vec<Point>, Skip> {
        let count = count.clamp(2, MAX_PATH_POINTS);
        if self.kind == EdgeKind::Straight {
            return Ok(vec![self.start(), self.end()]);
        }
        let divisor = if self.is_closed() { count } else { count - 1 };
        (0..count).map(|i| self.point_at(self.length * (i as f64) / (divisor as f64), work)).collect()
    }

    /// The place of the edge nearest `p`, when it is within `within` of it. The edge's box and each piece's are tried first; every piece visited is an arc call.
    pub(crate) fn nearest_within(&self, p: Point, within: f64, work: &mut OutlineWork) -> Result<Option<Near>, Skip> {
        if !near_box(&self.bounds, p, within) {
            return Ok(None);
        }
        let mut best: Option<(f64, usize, f64)> = None;
        for (i, piece) in self.pieces.iter().enumerate() {
            work.charge_arcs(1)?;
            let reach = best.map_or(within, |(d, _, _)| d.min(within));
            if !near_box(&seg_box(&piece.seg), p, reach) {
                continue;
            }
            let (t, distance) = nearest_on(&piece.seg, p, self.accuracy);
            if !distance.is_finite() || distance > within {
                continue;
            }
            if best.is_none_or(|(d, _, _)| distance < d - TIE) {
                best = Some((distance, i, t));
            }
        }
        let Some((distance, i, t)) = best else { return Ok(None) };
        let Some(piece) = self.pieces.get(i) else { return Ok(None) };
        let along = if is_line(&piece.seg) {
            piece.start + t * piece.len
        } else {
            work.charge_arcs(1)?;
            piece.start + piece_len(&piece.seg.subsegment(0.0..t), self.accuracy).min(piece.len)
        };
        Ok(Some(Near { distance, along }))
    }
}

/// The edge a row belongs to and how far along it (§2): the nearest edge within [`ON_EDGE`], the lower-numbered of two as near, and on an open edge not within
/// [`ON_EDGE`] of either end.
fn owner_of(outline: &Outline, row: &PointRow, work: &mut OutlineWork) -> Result<Option<(usize, f64)>, Skip> {
    // An extended row is a point of no direction that is kept; an outward point glues to other shapes' inward ones: neither takes connectors.
    if row.extended || !(row.kind == 0.0 || row.kind == 2.0) {
        return Ok(None);
    }
    let p = Point::new(row.x, row.y);
    if !p.is_finite() {
        return Ok(None);
    }
    let reach = ON_EDGE + ON_EDGE_SLACK;
    let mut best: Option<(f64, usize, f64)> = None;
    for (i, edge) in outline.edges.iter().enumerate() {
        let Some(near) = edge.nearest_within(p, reach, work)? else { continue };
        if !edge.is_closed() && (p.distance(edge.start()) <= reach || p.distance(edge.end()) <= reach) {
            continue;
        }
        let along = if edge.is_closed() && edge.length - near.along <= CLOSE_SLACK * edge.length.max(1.0) { 0.0 } else { near.along };
        if best.is_none_or(|(d, _, _)| near.distance < d - TIE) {
            best = Some((near.distance, i, along));
        }
    }
    Ok(best.map(|(_, i, along)| (i, along)))
}

/// The rows sorted by the edge each is a point of, each edge's in their order along it, and the rows on no edge.
pub(crate) fn points_on(outline: &Outline, rows: &[PointRow], work: &mut OutlineWork) -> Result<OnEdges, Skip> {
    let mut edges: Vec<Vec<OnEdge>> = outline.edges.iter().map(|_| Vec::new()).collect();
    let mut other = Vec::new();
    for row in rows {
        match owner_of(outline, row, work)? {
            Some((edge, along)) => {
                if let Some(list) = edges.get_mut(edge) {
                    list.push(OnEdge { index: row.index, along });
                }
            }
            None => other.push(row.index),
        }
    }
    for list in &mut edges {
        list.sort_by(|a, b| a.along.total_cmp(&b.along).then(a.index.cmp(&b.index)));
    }
    Ok(OnEdges { edges, other })
}

/// The shortest portion of an edge with points at `alongs` (in order along it): the stretches between one point and the next and, on an open edge, from the start to
/// the first point and from the last to the end; on a closed edge the last stretch runs from the last point past the start to the first. With no point the one portion is the
/// whole edge.
fn least_portion(length: f64, closed: bool, alongs: &[f64]) -> f64 {
    let (Some(&first), Some(&last)) = (alongs.first(), alongs.last()) else { return length };
    let between = alongs.windows(2).filter_map(|pair| match pair {
        [a, b] => Some(b - a),
        _ => None,
    });
    let ends: [f64; 2] = if closed { [length - last + first, f64::INFINITY] } else { [first, length - last] };
    between.chain(ends).fold(f64::INFINITY, f64::min).max(0.0)
}

/// The smallest gap a doubling would make on an edge with points at `alongs`, in the shape's own units: half of its shortest portion (a doubling puts a new point in the
/// middle of every portion, §3.2).
pub(crate) fn smallest_new_gap(edge: &Edge, alongs: &[f64]) -> f64 {
    least_portion(edge.length, edge.is_closed(), alongs) / 2.0
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    #[test]
    fn the_least_portion_of_an_open_edge_counts_both_ends_and_a_closed_edge_wraps() {
        // No point: the whole edge. One point in the middle of 10: two portions of 5.
        assert_eq!(least_portion(10.0, false, &[]), 10.0);
        assert_eq!(least_portion(10.0, false, &[5.0]), 5.0);
        // Open: the stretch from the start to the first point counts, and so does the one from the last to the end.
        assert_eq!(least_portion(10.0, false, &[1.0, 6.0]), 1.0);
        assert_eq!(least_portion(10.0, false, &[4.0, 9.5]), 0.5);
        assert_eq!(least_portion(10.0, false, &[4.0, 6.0]), 2.0);
        // Closed: one point has one gap round the whole curve, and the wrap past the start is a gap.
        assert_eq!(least_portion(10.0, true, &[3.0]), 10.0);
        assert_eq!(least_portion(10.0, true, &[1.0, 9.0]), 2.0);
        assert_eq!(least_portion(10.0, true, &[2.0, 5.0, 8.0]), 3.0);
    }

    #[test]
    fn a_portion_never_goes_below_zero() {
        assert_eq!(least_portion(10.0, false, &[6.0, 4.0]), 0.0, "out of order places are not a negative gap");
    }

    #[test]
    fn a_tangent_falls_back_to_the_next_control_point_and_a_point_has_none() {
        use diacraft_geom::kurbo::{CubicBez, QuadBez};
        let c = PathSeg::Cubic(CubicBez::new(Point::new(0.0, 0.0), Point::new(0.0, 0.0), Point::new(0.0, 1.0), Point::new(1.0, 1.0)));
        let start = tangent(&c, true, 1e-12).unwrap();
        assert!((start.x).abs() < 1e-12 && (start.y - 1.0).abs() < 1e-12, "{start:?}");
        let q = PathSeg::Quad(QuadBez::new(Point::new(2.0, 2.0), Point::new(2.0, 2.0), Point::new(2.0, 2.0)));
        assert_eq!(tangent(&q, true, 1e-12), None);
        assert_eq!(tangent(&q, false, 1e-12), None);
    }
}
