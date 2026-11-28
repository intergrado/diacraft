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

use std::sync::Arc;

use diacraft_doc::{CellAddr, EffectiveRow, FormulaSlot, RowKey, SectionKey, ShapeRef, SheetRef};
use diacraft_geom::kurbo::{ParamCurve, ParamCurveArclen, ParamCurveArea, ParamCurveDeriv, ParamCurveNearest, PathSeg, Point, Rect, Size, Vec2};
use diacraft_geom::{Bound, DrawStep, GeomError, MAX_ROWS, RowDraw, build_row_draws, is_closed};

use super::{MAX_CONNECTION_ROWS_READ, Skip};
use crate::draw::Reader;
use crate::draw::worked::RowView;

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

/// A tangent shorter than this share of the section's extent has no direction (the derivative of a cubic that stops at an end).
const NO_DIRECTION: f64 = 1.0e-12;

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

    pub(super) fn charge_rows(&mut self, rows: usize) -> Result<(), Skip> {
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

/// One piece of an edge: a line, a quadratic or a cubic, with the length before it and its own, and the segment (the drawn row, counted from 1 over the section's drawn rows with
/// moves left out) that made it, as the path functions number it.
#[derive(Clone, Copy, Debug)]
struct Piece {
    seg: PathSeg,
    start: f64,
    len: f64,
    segment: u32,
}

/// A step of a run as drawn, before its length is known; `draw` is the index of the row draw that made it and `step` the step's index in that draw.
#[derive(Clone, Copy)]
struct Raw {
    seg: PathSeg,
    draw: usize,
    step: usize,
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
    /// The `IX` (or place) of the last row of the draw before the one this edge came from: the row that set the pen it starts from.
    pub pen_row: Option<u32>,
    /// The index of the edge's first step in the draw of its row (the piece of a `PolylineTo`).
    pub step: u32,
    /// The signed area of the run the edge is in when the run ends where it began (positive when it turns counterclockwise), `None` for an open run: which side is outside.
    pub run_area: Option<f64>,
    pieces: Vec<Piece>,
    /// The box round every piece's control points.
    bounds: Rect,
    /// The accuracy lengths and places are worked out to: a tiny share of the section's extent.
    accuracy: f64,
}

/// A place on an edge: where, and which way the edge runs there (a unit vector, where it has a direction).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Place {
    pub point: Point,
    pub tangent: Option<Vec2>,
}

/// The nearest place of an edge to a point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Near {
    pub distance: f64,
    /// How far along the edge, in its own length.
    pub along: f64,
}

/// What a section costs a path function that walks it (`fn_path.rs`, "Cost"): a step for each cell the walk reads, three for each step drawn (the draw and the walk's two) and
/// one for each 512 multiplications of spline work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SectionCost {
    pub ix: Option<u32>,
    /// The cells the walk reads (`cells_read`).
    pub cells: usize,
    pub steps: usize,
    pub curve_work: u64,
}

/// How many cells of a row a path function reads, by the row's type: its `X` and `Y` and the cells of the type (`fn_path.rs` `draw_rows`). A row of a type it does not know is skipped.
pub(crate) fn cells_read(row_type: Option<&str>) -> usize {
    match row_type {
        Some("MoveTo" | "RelMoveTo" | "LineTo" | "RelLineTo") => 2,
        Some("ArcTo" | "SplineKnot" | "PolylineTo") => 3,
        Some("RelQuadBezTo" | "InfiniteLine") => 4,
        Some("EllipticalArcTo" | "RelEllipticalArcTo" | "SplineStart" | "RelCubBezTo" | "Ellipse") => 6,
        Some("NURBSTo") => 7,
        _ => 0,
    }
}

impl SectionCost {
    /// What one walk of the section is charged, in calc's steps: what the walk reads and draws and the spline work, a fifth more and 64 for the call itself, so that the charge is
    /// never less than the steps a path cell takes (measured: `tests/it/conn_points_costs.rs`).
    pub(crate) fn walk(&self) -> u64 {
        let cells = u64::try_from(self.cells).unwrap_or(u64::MAX);
        let steps = u64::try_from(self.steps).unwrap_or(u64::MAX);
        let base = cells.saturating_add(steps.saturating_mul(3)).saturating_add(self.curve_work / 512);
        base.saturating_mul(6).div_ceil(5).saturating_add(64)
    }
}

/// What a shape's outline is made of.
#[derive(Clone, Debug, Default)]
pub(crate) struct Outline {
    pub edges: Vec<Edge>,
    /// Whether the shape has more edges than [`MAX_EDGES_PER_SHAPE`]: those listed are the first.
    pub truncated: bool,
    /// The sections read, each with what a walk of it costs.
    pub sections: Vec<SectionCost>,
}

/// A row of the `Connection` section, in the shape's own frame.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PointRow {
    /// The row's `IX` (the first row is 0), or its place in a section keyed by name: the number `Connections.X<index + 1>` names and `ToPart` is 100 plus.
    pub index: u32,
    /// The row's name, in a section keyed by name.
    pub name: Option<Arc<str>>,
    pub x: f64,
    pub y: f64,
    /// The way out: `DirX` and `DirY`.
    pub dir_x: f64,
    pub dir_y: f64,
    /// `Type`: 0 inward, 1 outward, 2 both.
    pub kind: f64,
    /// Whether the row is extended: a formula in `D`.
    pub extended: bool,
    /// Whether the row's `X` or `Y` is guarded (`GUARD(...)`): a halving does not remove it.
    pub guarded: bool,
    /// The `Geometry` section a path formula in the row's `X` or `Y` walks (its `IX`), `u32::MAX` for one that does not say: what the row costs calc each time the section or
    /// the shape's size changes.
    pub path_section: Option<u32>,
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

pub(super) fn cell_number(row: &EffectiveRow<'_>, name: &str) -> f64 {
    row.cells
        .iter()
        .find(|(n, _)| n.as_str() == name)
        .and_then(|(_, found)| found.value.cell)
        .and_then(|c| c.value.as_ref())
        .and_then(|v| v.number())
        .unwrap_or(0.0)
}

/// The number a cell of a connection row holds, 0 for a cell the row lacks. A cell that holds a formula and no saved value is what the formula works out to, as the
/// picture reads it.
fn worked_number(row: &RowView<'_, '_>, name: &str) -> f64 {
    row.number(name).unwrap_or(0.0)
}

pub(super) fn row_number(row: &EffectiveRow<'_>, position: usize) -> u32 {
    match &row.key {
        RowKey::Ix(ix) => *ix,
        RowKey::Name(_) => u32::try_from(position).unwrap_or(u32::MAX),
    }
}

/// The formula text of a cell of the row, as the resolver finds it (through the master).
pub(super) fn formula_text<'r>(row: &'r EffectiveRow<'_>, name: &str) -> Option<&'r str> {
    row.cells.iter().find(|(n, _)| n.as_str() == name).and_then(|(_, found)| found.formula.cell).and_then(|c| c.formula.text())
}

/// The section a `POINTALONGPATH(Geometry<n>.Path, ...)` in a formula walks, as its `IX` (`n` less one): `Some(u32::MAX)` for a call that does not name one a reader can find,
/// `None` for a formula with no such call.
fn path_section(text: &str) -> Option<u32> {
    let upper = text.to_ascii_uppercase();
    let call = upper.find("POINTALONGPATH(")?;
    let rest = upper.get(call..)?;
    let digits: String =
        rest.find("GEOMETRY").and_then(|g| rest.get(g + "GEOMETRY".len()..)).unwrap_or("").chars().take_while(char::is_ascii_digit).collect();
    Some(digits.parse::<u32>().ok().and_then(|n| n.checked_sub(1)).unwrap_or(u32::MAX))
}

/// Whether a formula is a `GUARD(...)`: the cell is protected from a drag.
fn is_guard(text: &str) -> bool {
    text.trim_start().get(..6).is_some_and(|head| head.eq_ignore_ascii_case("GUARD("))
}

/// Whether the row has a formula in `D`: the sign of an extended row.
fn has_d_formula(row: &EffectiveRow<'_>) -> bool {
    row.cells.iter().any(|(n, found)| n.as_str() == "D" && found.formula.cell.is_some_and(|c| matches!(c.formula, FormulaSlot::Local(_))))
}

/// The `Connection` rows of the shape `at` in its own frame, within [`MAX_CONNECTION_ROWS_READ`], counted over the shape, its master and the styles before any is
/// merged: a shape with more is [`Skip::Shape`], and none is merged or charged.
pub(crate) fn point_rows<'a>(reader: &Reader<'a>, at: &ShapeRef, work: &mut OutlineWork) -> Result<Vec<PointRow>, Skip> {
    let held = reader.connection_rows(at);
    if held > MAX_CONNECTION_ROWS_READ {
        let id = reader.doc().shape(at).map_or(0, |s| s.id);
        return Err(Skip::Shape(format!(
            "shape {id} has more than {MAX_CONNECTION_ROWS_READ} connection points, counted over the shape and its master: it is not read"
        )));
    }
    work.point_rows = work.point_rows.saturating_add(held);
    let sheet = SheetRef::Shape(at.clone());
    let key = SectionKey::new("Connection", None);
    let Some(Ok(rows)) = reader.resolver().effective_rows_within(&sheet, &key, held) else {
        return Ok(Vec::new());
    };
    Ok(rows
        .iter()
        .enumerate()
        .map(|(position, effective)| {
            let row = reader.worked().row(&sheet, &key, effective);
            PointRow {
                index: row_number(effective, position),
                name: if let RowKey::Name(name) = &effective.key { Some(Arc::clone(name)) } else { None },
                x: worked_number(&row, "X"),
                y: worked_number(&row, "Y"),
                dir_x: worked_number(&row, "DirX"),
                dir_y: worked_number(&row, "DirY"),
                kind: worked_number(&row, "Type"),
                extended: has_d_formula(effective),
                guarded: ["X", "Y"].iter().any(|name| formula_text(effective, name).is_some_and(is_guard)),
                path_section: ["X", "Y"].iter().find_map(|name| formula_text(effective, name).and_then(path_section)),
            }
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
    /// By draw: the segment number of the row (drawn rows counted from 1, moves left out).
    segments: &'a [u32],
    /// By draw: the number of the last row of the draw before it, which set the pen.
    before: &'a [Option<u32>],
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

/// The unit direction of a piece at the parameter `t`: its derivative there, and where that has no length the direction at the nearer end.
fn tangent_at(seg: &PathSeg, t: f64, floor: f64) -> Option<Vec2> {
    let d = match seg {
        PathSeg::Line(l) => l.p1 - l.p0,
        PathSeg::Quad(q) => q.deriv().eval(t).to_vec2(),
        PathSeg::Cubic(c) => c.deriv().eval(t).to_vec2(),
    };
    let l = d.hypot();
    if l.is_finite() && l > floor { Some(d / l) } else { tangent(seg, t < 0.5, floor) }
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
        run.push(Raw { seg, draw: c.draw, step: c.step.saturating_sub(1) });
        c.pen = Some(to);
    }
    (!run.is_empty()).then_some(run)
}

/// What the edges of a section are collected in.
struct Builder {
    edges: Vec<Edge>,
    truncated: bool,
    sections: Vec<SectionCost>,
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
        // Which way the run turns, when it ends where it began: the side that is outside, for a point's way out.
        let area = match (run.first(), run.last()) {
            (Some(first), Some(last)) if is_closed(first.seg.start(), last.seg.end()) => Some(run.iter().map(|p| p.seg.signed_area()).sum::<f64>()),
            _ => None,
        };
        if Self::closed_curve(run, sect) {
            return self.emit(EdgeKind::Closed, run, sect, work, area);
        }
        let mut i = 0;
        while let Some(piece) = run.get(i) {
            let line = is_line(&piece.seg);
            let mut end = i + 1;
            while !line && run.get(end).is_some_and(|next| !is_line(&next.seg) && next.draw == piece.draw) {
                end += 1;
            }
            let kind = if line { EdgeKind::Straight } else { EdgeKind::Curved };
            if self.emit(kind, run.get(i..end).unwrap_or_default(), sect, work, area)? == Flow::Stop {
                return Ok(Flow::Stop);
            }
            i = end;
        }
        Ok(Flow::Go)
    }

    /// Adds the edge the pieces make, unless it is shorter than [`MIN_EDGE`]; stops at [`MAX_EDGES_PER_SHAPE`] with `truncated` set.
    fn emit(&mut self, kind: EdgeKind, pieces: &[Raw], sect: &Section<'_>, work: &mut OutlineWork, run_area: Option<f64>) -> Result<Flow, Skip> {
        work.charge_arcs(pieces.iter().filter(|p| !is_line(&p.seg)).count())?;
        let mut measured: Vec<Piece> = Vec::with_capacity(pieces.len());
        let mut at = 0.0_f64;
        let mut bounds: Option<Rect> = None;
        for raw in pieces {
            let len = piece_len(&raw.seg, sect.accuracy);
            if !len.is_finite() || len < 0.0 {
                return Err(Skip::Shape(format!("shape {}: a length of its outline is not a number", sect.id)));
            }
            measured.push(Piece { seg: raw.seg, start: at, len, segment: sect.segments.get(raw.draw).copied().unwrap_or(0) });
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
            pen_row: pieces.first().and_then(|p| sect.before.get(p.draw)).copied().flatten(),
            step: pieces.first().map_or(0, |p| u32::try_from(p.step).unwrap_or(u32::MAX)),
            run_area: run_area.filter(|a| a.is_finite()),
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
        let (found, _) = reader.worked().geom_rows(&sheet, key, std::slice::from_ref(row));
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
    // The segment each draw is (drawn rows counted from 1, a draw of nothing but moves none), and the row that set the pen each draw starts from.
    let mut count = 0_u32;
    let segments: Vec<u32> = drawn
        .rows
        .iter()
        .map(|d| {
            if d.steps.iter().any(|step| !matches!(step, DrawStep::Jump(_))) {
                count = count.saturating_add(1);
            }
            count
        })
        .collect();
    let before: Vec<Option<u32>> = (0..drawn.rows.len())
        .map(|i| {
            let last = i.checked_sub(1).and_then(|p| drawn.rows.get(p)).map(|d| d.first.saturating_add(d.rows).saturating_sub(1))?;
            numbers.get(last).copied()
        })
        .collect();
    out.sections.push(SectionCost {
        ix: key.ix,
        cells: rows.iter().map(|r| cells_read(r.row_type.as_deref())).fold(0, usize::saturating_add),
        steps,
        curve_work: drawn.curve_work,
    });
    let sect = Section {
        id,
        ix: key.ix,
        draws: &drawn.rows,
        numbers: &numbers,
        ellipse: &ellipse,
        segments: &segments,
        before: &before,
        extent,
        accuracy: LENGTH_ACCURACY * extent,
    };
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
    let mut out = Builder { edges: Vec::new(), truncated: false, sections: Vec::new() };
    for key in &keys {
        if read_section(reader, at, id, key, size, work, &mut out)? == Flow::Stop {
            break;
        }
    }
    Ok(Outline { edges: out.edges, truncated: out.truncated, sections: out.sections })
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

    /// The place `along` the edge, clamped to it, and which way the edge runs there. A curved piece costs an arc call.
    pub(crate) fn place_at(&self, along: f64, work: &mut OutlineWork) -> Result<Place, Skip> {
        let along = if along.is_finite() { along.clamp(0.0, self.length) } else { 0.0 };
        let index = self.pieces.partition_point(|p| p.start + p.len < along);
        let Some(piece) = self.pieces.get(index).or_else(|| self.pieces.last()) else { return Ok(Place { point: Point::ORIGIN, tangent: None }) };
        let floor = NO_DIRECTION * self.accuracy / LENGTH_ACCURACY;
        let local = (along - piece.start).clamp(0.0, piece.len);
        if piece.len <= 0.0 {
            return Ok(Place { point: piece.seg.start(), tangent: tangent(&piece.seg, true, floor) });
        }
        let t = if is_line(&piece.seg) {
            local / piece.len
        } else {
            work.charge_arcs(1)?;
            piece.seg.inv_arclen(local, self.accuracy).clamp(0.0, 1.0)
        };
        Ok(Place { point: piece.seg.eval(t), tangent: tangent_at(&piece.seg, t, floor) })
    }

    /// The place `along` the edge, clamped to it. A curved piece costs an arc call.
    pub(crate) fn point_at(&self, along: f64, work: &mut OutlineWork) -> Result<Point, Skip> {
        self.place_at(along, work).map(|place| place.point)
    }

    /// Where `along` the edge falls as a path function counts it: the segment (the drawn row, from 1, moves left out) and the share of that row's length, by arc length
    /// `POINTALONGPATH(Geometry<s>.Path, <share>, 0, <segment>)`. A place on the join of two rows is the end of the earlier. `None` for a
    /// segment of no length.
    pub(crate) fn segment_place(&self, along: f64) -> Option<(u32, f64)> {
        let along = if along.is_finite() { along.clamp(0.0, self.length) } else { 0.0 };
        let from = self.pieces.partition_point(|p| p.start + p.len < along);
        // Pieces of no length are never chosen, as the path walk never chooses them.
        let live = |k: &usize| self.pieces.get(*k).is_some_and(|p| p.len > 0.0);
        let at = (from..self.pieces.len()).find(live).or_else(|| (0..from).rev().find(live))?;
        let segment = self.pieces.get(at)?.segment;
        let mut first = at;
        while first > 0 && self.pieces.get(first - 1).is_some_and(|p| p.segment == segment) {
            first -= 1;
        }
        let start = self.pieces.get(first)?.start;
        let len: f64 = self.pieces.get(first..)?.iter().take_while(|p| p.segment == segment).map(|p| p.len).sum();
        (len > 0.0 && len.is_finite()).then(|| (segment, ((along - start) / len).clamp(0.0, 1.0)))
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

    #[test]
    fn a_path_formula_names_its_section_by_the_number_after_geometry() {
        assert_eq!(path_section("PNTX(POINTALONGPATH(Geometry3.Path,0.5,0,1))"), Some(2));
        assert_eq!(path_section("pntx(pointalongpath(geometry1.path,0.125,0,1))"), Some(0));
        assert_eq!(path_section("POINTALONGPATH(Scratch.A1,0.5)"), Some(u32::MAX), "a call that names no section a reader can find");
        assert_eq!(path_section("POINTALONGPATH(Geometry0.Path,0.5)"), Some(u32::MAX), "there is no section 0 to name");
        assert_eq!(path_section("POINTALONGPATH(Geometry99999999999.Path,0.5)"), Some(u32::MAX), "a number past what a section holds");
        assert_eq!(path_section("Width*0.5"), None);
        assert_eq!(path_section(""), None);
    }

    #[test]
    fn only_a_formula_that_begins_with_guard_is_a_guard() {
        assert!(is_guard("GUARD(Width*0.25)") && is_guard("  guard(1)"));
        assert!(!is_guard("Width*GUARD(1)") && !is_guard("GUAR") && !is_guard("GUARDS(1)") && !is_guard(""));
        assert!(!is_guard("GUARD\u{e9}("), "a multi-byte character where the bracket should be is no guard and no panic");
    }

    #[test]
    fn a_walk_is_charged_a_fifth_over_what_it_reads_and_draws_and_the_arithmetic_saturates() {
        // 6 cells and 15 steps: 6 + 3 * 15 = 51, a fifth over is 62 (rounded up), and 64 for the call.
        assert_eq!(SectionCost { ix: Some(0), cells: 6, steps: 15, curve_work: 0 }.walk(), 126);
        assert_eq!(SectionCost { ix: None, cells: 0, steps: 0, curve_work: 0 }.walk(), 64);
        assert_eq!(SectionCost { ix: None, cells: 0, steps: 0, curve_work: 5_120 }.walk(), 12 + 64, "10 steps for 5,120 multiplications of a spline");
        let huge = SectionCost { ix: None, cells: usize::MAX, steps: usize::MAX, curve_work: u64::MAX }.walk();
        assert!(huge >= u64::MAX / 5, "{huge}");
        assert_eq!([Some("MoveTo"), Some("ArcTo"), Some("Ellipse"), Some("NURBSTo"), Some("Nothing"), None].map(cells_read), [2, 3, 6, 7, 0, 0]);
    }

    /// An edge of three pieces: the first two the segment 1 (lengths 1 and 1), then a piece of no length, then the segment 2 (length 2).
    fn three_pieces() -> Edge {
        use diacraft_geom::kurbo::Line;
        let piece = |from: f64, len: f64, segment: u32| Piece {
            seg: PathSeg::Line(Line::new(Point::new(from, 0.0), Point::new(from + len, 0.0))),
            start: from,
            len,
            segment,
        };
        let pieces = vec![piece(0.0, 1.0, 1), piece(1.0, 1.0, 1), piece(2.0, 0.0, 2), piece(2.0, 2.0, 2)];
        Edge {
            number: 0,
            kind: EdgeKind::Curved,
            section_ix: Some(0),
            rows: (1, 2),
            steps: 4,
            length: 4.0,
            pen_row: None,
            step: 0,
            run_area: None,
            pieces,
            bounds: Rect::new(0.0, 0.0, 4.0, 0.0),
            accuracy: 1.0e-9,
        }
    }

    #[test]
    fn a_place_along_an_edge_is_a_segment_and_a_share_of_that_segments_length() {
        let edge = three_pieces();
        let at = |along: f64| edge.segment_place(along).unwrap();
        assert_eq!(at(0.0), (1, 0.0));
        assert_eq!(at(0.5), (1, 0.25), "the segment is both its pieces, 2 long");
        assert_eq!(at(1.0), (1, 0.5), "on the join of two pieces of a segment");
        assert_eq!(at(2.0), (1, 1.0), "on the join of two segments: the end of the earlier");
        assert_eq!(at(3.0), (2, 0.5), "a piece of no length is never chosen");
        assert_eq!(at(4.0), (2, 1.0));
        assert_eq!(at(99.0), (2, 1.0), "past the end is the end");
        assert_eq!(at(f64::NAN), (1, 0.0), "no number is the start");
    }

    #[test]
    fn a_place_has_the_direction_the_edge_runs_there() {
        let edge = three_pieces();
        let mut work = OutlineWork::default();
        let place = edge.place_at(1.5, &mut work).unwrap();
        assert!((place.point.x - 1.5).abs() < 1.0e-12 && place.point.y == 0.0);
        let t = place.tangent.unwrap();
        assert!((t.x - 1.0).abs() < 1.0e-12 && t.y.abs() < 1.0e-12);
        assert_eq!(work.arc_calls, 0, "a straight piece costs no arc call");
        // A cubic's direction is its derivative's; one that stops at an end takes the direction of the control point after it.
        use diacraft_geom::kurbo::CubicBez;
        let c = PathSeg::Cubic(CubicBez::new(Point::new(0.0, 0.0), Point::new(0.0, 0.0), Point::new(0.0, 1.0), Point::new(1.0, 1.0)));
        let start = tangent_at(&c, 0.0, 1.0e-12).unwrap();
        assert!(start.x.abs() < 1.0e-12 && (start.y - 1.0).abs() < 1.0e-12, "{start:?}");
        let end = tangent_at(&c, 1.0, 1.0e-12).unwrap();
        assert!((end.x - 1.0).abs() < 1.0e-12 && end.y.abs() < 1.0e-12, "{end:?}");
    }
}
