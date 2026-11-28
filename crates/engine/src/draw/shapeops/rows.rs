//! Loops to `Geometry` rows, and the box that gives a produced shape its frame (SHAPEOPS-FRAME, SHAPEOPS-ROWS).
//!
//! **The loops.** What the path library gives back is a `BezPath` of closed contours, outer boundaries counter-clockwise and holes clockwise (its README: the
//! result fills the same under either fill rule). [`clean`] reads it as [`Loop`]s, rings of nodes with the edge that leaves each (a line or one cubic), merges
//! vertices closer than [`MERGE`] and takes out the vertices a straight run passes through (the library keeps its inputs' own anchors, so the union of four bars
//! that form a ring would otherwise list a vertex in the middle of each side), drops a region of less area than [`MIN_AREA`], and turns the loops about if
//! the library's outer boundaries were clockwise, so that **outer boundaries run anticlockwise and holes clockwise** whatever it did.
//!
//! **The rows**. Each loop is one section: a `MoveTo`, then a `LineTo` for each straight edge and one clamped cubic `NURBSTo` for each curved one (degree 3,
//! knots 0, 0, 0, 0, 1, weights 1), each coordinate a share of the box (`Width*0.666666666667`, the constant 0 for a share of 0 and plain `Width` or
//! `Height` for 1, the constant 0 where the box is flat), the control points of the constructor in the relative type. A closed loop ends on the formulas
//! its `MoveTo` starts with. The flags are those of R11: `NoFill` 0 for a closed section, `NoLine`, `NoShow`, `NoSnap` and `NoQuickDrag` 0. A share is written to
//! twelve places, so that a coordinate is within 1E-9 of the one worked out (a cell holds nine, `stencil::number_text`, and this is why a share is not one).

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Row, Section, Value};
use kurbo::{BezPath, PathEl, Point, Rect, Shape as _};

use super::runs::{Run, Seg};
use super::{MERGE, MIN_AREA};
use crate::stencil::number_text;

/// How far a vertex may lie from the line through its two neighbours to be taken out of a straight run (cell units), and the share of the coordinates' size added
/// to it for rounding.
const STRAIGHT: f64 = 1.0e-9;
const STRAIGHT_RELATIVE: f64 = 1.0e-13;

/// The edge that leaves a node of a loop for the next.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Edge {
    Line,
    /// One cubic Bezier: its two control points.
    Cubic(Point, Point),
}

/// A vertex of a loop and the edge from it to the next vertex (the first, for the last).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Node {
    pub p: Point,
    pub out: Edge,
}

/// A closed boundary: a ring of at least two nodes.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Loop {
    pub nodes: Vec<Node>,
}

impl Loop {
    /// The loop as a closed path.
    pub(super) fn path(&self) -> BezPath {
        let mut path = BezPath::new();
        let Some(first) = self.nodes.first() else { return path };
        path.move_to(first.p);
        for (i, node) in self.nodes.iter().enumerate() {
            let to = self.nodes.get(i + 1).unwrap_or(first).p;
            match node.out {
                Edge::Line => path.line_to(to),
                Edge::Cubic(a, b) => path.curve_to(a, b, to),
            }
        }
        path.close_path();
        path
    }

    /// The signed area: positive for a loop that runs anticlockwise with y up.
    pub(super) fn area(&self) -> f64 {
        self.path().area()
    }

    /// The loop run the other way round.
    fn reversed(&self) -> Loop {
        let n = self.nodes.len();
        let mut nodes = Vec::with_capacity(n);
        // The node at place `k` of the new ring is the old node `n - k` (mod n), and its edge goes to the old node before it, the old edge turned round.
        for k in 0..n {
            let from = (n - k) % n;
            let to = (n + n - k - 1) % n;
            let old_edge = self.nodes.get(to).map_or(Edge::Line, |node| node.out);
            let out = match old_edge {
                Edge::Line => Edge::Line,
                Edge::Cubic(a, b) => Edge::Cubic(b, a),
            };
            if let Some(node) = self.nodes.get(from) {
                nodes.push(Node { p: node.p, out });
            }
        }
        Loop { nodes }
    }
}

fn cubic_of_quad(from: Point, c: Point, to: Point) -> (Point, Point) {
    (from + (c - from) * (2.0 / 3.0), to + (c - to) * (2.0 / 3.0))
}

/// The contours of `path` as rings. A contour with a point that is not a number is left out.
pub(super) fn rings_of(path: &BezPath) -> Vec<Loop> {
    let mut out: Vec<Loop> = Vec::new();
    let mut pts: Vec<Point> = Vec::new();
    let mut edges: Vec<Edge> = Vec::new();
    let finish = |pts: &mut Vec<Point>, edges: &mut Vec<Edge>, out: &mut Vec<Loop>| {
        let (points, edge_list) = (std::mem::take(pts), std::mem::take(edges));
        if let Some(ring) = close_ring(points, edge_list) {
            out.push(ring);
        }
    };
    for el in path.elements() {
        match *el {
            PathEl::MoveTo(p) => {
                finish(&mut pts, &mut edges, &mut out);
                pts.push(p);
            }
            PathEl::LineTo(p) => {
                if !pts.is_empty() {
                    edges.push(Edge::Line);
                    pts.push(p);
                }
            }
            PathEl::QuadTo(c, p) => {
                if let Some(&from) = pts.last() {
                    let (a, b) = cubic_of_quad(from, c, p);
                    edges.push(Edge::Cubic(a, b));
                    pts.push(p);
                }
            }
            PathEl::CurveTo(a, b, p) => {
                if !pts.is_empty() {
                    edges.push(Edge::Cubic(a, b));
                    pts.push(p);
                }
            }
            PathEl::ClosePath => finish(&mut pts, &mut edges, &mut out),
        }
    }
    finish(&mut pts, &mut edges, &mut out);
    out
}

/// A ring from the points of a contour and the edges between them: the contour's last edge comes back to its first point (it is dropped as a point of its own
/// when it ends within [`MERGE`] of it) or a straight edge is added to close it.
fn close_ring(mut pts: Vec<Point>, mut edges: Vec<Edge>) -> Option<Loop> {
    if pts.len() < 2 || !pts.iter().all(|p| p.is_finite()) {
        return None;
    }
    let first = *pts.first()?;
    let last = *pts.last()?;
    if last.distance(first) <= MERGE {
        pts.pop();
    } else {
        edges.push(Edge::Line);
    }
    if pts.len() != edges.len() || pts.len() < 2 {
        return None;
    }
    let edge_points = edges.iter().filter_map(|e| if let Edge::Cubic(a, b) = e { Some([*a, *b]) } else { None }).flatten();
    if !edge_points.clone().all(|p| p.is_finite()) {
        return None;
    }
    Some(Loop { nodes: pts.into_iter().zip(edges).map(|(p, out)| Node { p, out }).collect() })
}

/// Whether `b` lies on the straight way from `a` to `c`: in the middle of it, within [`STRAIGHT`] of the line, and not a spike that goes back.
fn passes_straight(a: Point, b: Point, c: Point) -> bool {
    let (ab, bc, ac) = (b - a, c - b, c - a);
    let length = ac.hypot();
    if ab.dot(bc) <= 0.0 || length <= 0.0 {
        return false;
    }
    let off = ab.cross(ac).abs() / length;
    let size = a.x.abs() + a.y.abs() + b.x.abs() + b.y.abs() + c.x.abs() + c.y.abs();
    off <= STRAIGHT + STRAIGHT_RELATIVE * size
}

/// The ring with the vertices that add nothing taken out: a straight edge shorter than [`MERGE`] (its end goes) and a vertex a straight run passes through. Two
/// nodes at least are kept. One pass: each removal puts its neighbours back to be looked at again, so the work is a few steps for each node.
fn simplify(ring: &Loop) -> Loop {
    let n = ring.nodes.len();
    if n < 3 {
        return ring.clone();
    }
    let mut nodes = ring.nodes.clone();
    let mut next: Vec<usize> = (0..n).map(|i| (i + 1) % n).collect();
    let mut prev: Vec<usize> = (0..n).map(|i| (i + n - 1) % n).collect();
    let mut alive = vec![true; n];
    let mut count = n;
    let mut stack: Vec<usize> = (0..n).rev().collect();
    while let Some(i) = stack.pop() {
        if count <= 2 || !alive.get(i).copied().unwrap_or(false) {
            continue;
        }
        let (p, q) = (prev.get(i).copied().unwrap_or(i), next.get(i).copied().unwrap_or(i));
        let (Some(here), Some(before), Some(after)) = (nodes.get(i).copied(), nodes.get(p).copied(), nodes.get(q).copied()) else { continue };
        if here.out == Edge::Line && here.p.distance(after.p) <= MERGE {
            // The edge to the next vertex is too short to be one: the next vertex goes, and its edge is this one's.
            let beyond = next.get(q).copied().unwrap_or(q);
            if let Some(node) = nodes.get_mut(i) {
                node.out = after.out;
            }
            set(&mut next, i, beyond);
            set(&mut prev, beyond, i);
            set_flag(&mut alive, q, false);
            count -= 1;
            stack.push(i);
            stack.push(beyond);
        } else if before.out == Edge::Line && here.out == Edge::Line && passes_straight(before.p, here.p, after.p) {
            set(&mut next, p, q);
            set(&mut prev, q, p);
            set_flag(&mut alive, i, false);
            count -= 1;
            stack.push(p);
            stack.push(q);
        }
    }
    let Some(start) = alive.iter().position(|a| *a) else { return ring.clone() };
    let mut out = Vec::with_capacity(count);
    let mut at = start;
    for _ in 0..count {
        if let Some(node) = nodes.get(at) {
            out.push(*node);
        }
        at = next.get(at).copied().unwrap_or(start);
    }
    Loop { nodes: out }
}

fn set(list: &mut [usize], at: usize, to: usize) {
    if let Some(slot) = list.get_mut(at) {
        *slot = to;
    }
}

fn set_flag(list: &mut [bool], at: usize, to: bool) {
    if let Some(slot) = list.get_mut(at) {
        *slot = to;
    }
}

/// The loops of the path the library gave: merged, thinned, the slivers dropped, and turned so that outer boundaries run anticlockwise.
pub(super) fn clean(path: &BezPath) -> Vec<Loop> {
    let mut loops: Vec<Loop> = rings_of(path).iter().map(simplify).filter(|ring| ring.area().abs() >= MIN_AREA).collect();
    // The largest loop is an outer boundary, whatever else is: a hole lies inside another loop and has less area.
    let biggest = loops.iter().map(Loop::area).fold(0.0_f64, |best, a| if a.abs() > best.abs() { a } else { best });
    if biggest < 0.0 {
        loops = loops.iter().map(Loop::reversed).collect();
    }
    loops
}

/// The smallest box with sides along the axes that holds the loops: curves count by their true extent. `None` for no loops.
pub(super) fn bounds(loops: &[Loop]) -> Option<Rect> {
    loops.iter().map(|ring| ring.path().bounding_box()).reduce(|a, b| a.union(b))
}

// ---- cells ----------------------------------------------------------------------------------------------------------------------

fn put(cells: &mut std::collections::BTreeMap<CellName, Cell>, name: &str, cell: Cell) {
    cells.insert(CellName::new(name), cell);
}

/// A plain number constant.
pub(super) fn plain(n: f64) -> Cell {
    Cell::constant(Value::from_text(&number_text(n)))
}

/// A length constant, in inches.
pub(super) fn length(n: f64) -> Cell {
    plain(n).with_unit("IN")
}

/// A length formula with the value it gives.
pub(super) fn length_formula(formula: &str, n: f64) -> Cell {
    Cell::with_formula(formula, Some(Value::from_text(&number_text(n)))).with_unit("IN")
}

/// A share as text: twelve places, the trailing zeros taken off. A share too small to show is 0.
pub(super) fn share_text(k: f64) -> String {
    let text = format!("{k:.12}");
    let trimmed = text.trim_end_matches('0').trim_end_matches('.');
    match trimmed {
        "" | "-" | "-0" => "0".to_owned(),
        other => other.to_owned(),
    }
}

/// A share of the box as it is written: the offset over the side, to the twelve places the text holds, so that what is within half of the last of them of 0 or of 1 is
/// that number. A flat side (nothing to be a share of) and an offset that is no number give 0.
pub(super) fn share_of(offset: f64, size: f64) -> f64 {
    if size <= 0.0 {
        return 0.0;
    }
    let k = offset / size;
    if !k.is_finite() {
        return 0.0;
    }
    share_text(k).parse::<f64>().unwrap_or(0.0)
}

/// A coordinate cell: `Width*k` (or `Height*k`) with its value, the constant 0 for a share of 0 and plain `Width` for 1. `size` is the box's side, which
/// the cell's value is a share of.
pub(super) fn share_cell(of: &str, k: f64, size: f64) -> Cell {
    if k == 0.0 || size <= 0.0 {
        length(0.0)
    } else if k == 1.0 {
        length_formula(of, size)
    } else {
        // What the formula gives: the cell `Width` holds the box's side to nine places, and the share its twelve.
        let side = (size * 1.0e9).round() / 1.0e9;
        length_formula(&format!("{of}*{}", share_text(k)), side * k)
    }
}

/// A box a section's rows are shares of: its lower-left corner in the frame of the loops, and its size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Frame {
    pub min: Point,
    pub width: f64,
    pub height: f64,
}

impl Frame {
    /// The frame of a box, a side under a billionth being flat: the noise of a vertical edge is not a width.
    pub(crate) fn of(rect: Rect) -> Frame {
        let flat = |side: f64| if side <= 1.0e-9 { 0.0 } else { side };
        Frame { min: Point::new(rect.x0, rect.y0), width: flat(rect.width()), height: flat(rect.height()) }
    }

    fn x(&self, p: Point) -> f64 {
        share_of(p.x - self.min.x, self.width)
    }

    fn y(&self, p: Point) -> f64 {
        share_of(p.y - self.min.y, self.height)
    }

    fn x_cell(&self, p: Point) -> Cell {
        share_cell("Width", self.x(p), self.width)
    }

    fn y_cell(&self, p: Point) -> Cell {
        share_cell("Height", self.y(p), self.height)
    }
}

/// A row of a `Geometry` section, keyed by its `IX`.
fn row(ix: u32, row_type: &str, cells: Vec<(&str, Cell)>) -> Arc<Row> {
    let mut row = Row { ix: Some(ix), row_type: Some(Arc::from(row_type)), ..Row::default() };
    for (name, cell) in cells {
        put(&mut row.cells, name, cell);
    }
    Arc::new(row)
}

/// The `NURBSTo` row of one cubic, ending at `to`: the control points between the pen and the end are in the constructor as shares.
fn cubic_row(ix: u32, frame: &Frame, (a, b): (Point, Point), to: Point) -> Arc<Row> {
    let pair = |p: Point| format!("{},{}", share_text(frame.x(p)), share_text(frame.y(p)));
    let constructor = format!("NURBS(1,3,0,0,{},0,1,{},0,1)", pair(a), pair(b));
    row(
        ix,
        "NURBSTo",
        vec![
            ("X", frame.x_cell(to)),
            ("Y", frame.y_cell(to)),
            ("A", plain(0.0)),
            ("B", plain(1.0)),
            ("C", plain(0.0)),
            ("D", plain(1.0)),
            ("E", Cell::constant(Value::from_text(&constructor)).with_unit("NURBS")),
        ],
    )
}

/// A `Geometry` section with its five flags: `no_fill` for a section that only strokes.
pub(crate) fn flagged(ix: u32, no_fill: bool) -> Section {
    let mut section = Section::new("Geometry", Some(ix));
    for flag in ["NoFill", "NoLine", "NoShow", "NoSnap", "NoQuickDrag"] {
        put(&mut section.cells, flag, plain(if flag == "NoFill" && no_fill { 1.0 } else { 0.0 }));
    }
    section
}

/// The section of one loop, its coordinates shares of `frame`, numbered `ix`; the rows start at `IX` 1 as a file's do.
pub(super) fn section_of(ring: &Loop, frame: &Frame, ix: u32) -> Section {
    let mut section = flagged(ix, false);
    let Some(first) = ring.nodes.first() else { return section };
    let mut row_ix = 1_u32;
    section.append_row(row(row_ix, "MoveTo", vec![("X", frame.x_cell(first.p)), ("Y", frame.y_cell(first.p))]));
    for (i, node) in ring.nodes.iter().enumerate() {
        row_ix = row_ix.saturating_add(1);
        // The last edge comes back to the first vertex, whose formulas the row repeats.
        let to = ring.nodes.get(i + 1).map_or(first.p, |next| next.p);
        let new_row = match node.out {
            Edge::Line => row(row_ix, "LineTo", vec![("X", frame.x_cell(to)), ("Y", frame.y_cell(to))]),
            Edge::Cubic(a, b) => cubic_row(row_ix, frame, (a, b), to),
        };
        section.append_row(new_row);
    }
    section
}

/// The flags of a section written from runs: `NoShow` is always 0, a hidden section being read by no operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Flags {
    pub no_fill: bool,
    pub no_line: bool,
    pub no_snap: bool,
    pub no_quick_drag: bool,
}

impl Flags {
    /// The flags of a section that is new, with `NoFill` as given: the other three are 0.
    pub(super) const fn new(no_fill: bool) -> Flags {
        Flags { no_fill, no_line: false, no_snap: false, no_quick_drag: false }
    }
}

/// The section of one run (a path that is not a ring of the area operations: a chain Join made, a stretch Trim cut, a side Offset drew), its coordinates shares of `frame`,
/// numbered `ix`: a `MoveTo`, a `LineTo` for each straight segment and one clamped cubic `NURBSTo` for each curved one. A run whose ends meet ends on the formulas its
/// `MoveTo` starts with, so that it closes exactly. `no_fill` is the section's `NoFill`: 1 for a section that strokes only, which is every open one, and every piece
/// that cutting made even where its ends meet.
pub(super) fn section_of_run(run: &Run, frame: &Frame, ix: u32, no_fill: bool) -> Section {
    section_of_runs(std::slice::from_ref(run), frame, ix, Flags::new(no_fill))
}

/// The section of several runs one after the other, each starting with a `MoveTo` (a section of many paths, as Fit Curve keeps one and Offset's side of a section is), numbered
/// `ix`, with the five flags as `flags` gives them; the rows are numbered on from 1. A segment that is an arc is an `ArcTo` row: `X`, `Y` and `A`, the bow, a length.
pub(super) fn section_of_runs(runs: &[Run], frame: &Frame, ix: u32, flags: Flags) -> Section {
    let mut section = flagged(ix, flags.no_fill);
    for (name, on) in [("NoLine", flags.no_line), ("NoSnap", flags.no_snap), ("NoQuickDrag", flags.no_quick_drag)] {
        put(&mut section.cells, name, plain(if on { 1.0 } else { 0.0 }));
    }
    let mut row_ix = 0_u32;
    let mut next = || {
        row_ix = row_ix.saturating_add(1);
        row_ix
    };
    for run in runs {
        let closes = run.closed();
        section.append_row(row(next(), "MoveTo", vec![("X", frame.x_cell(run.start)), ("Y", frame.y_cell(run.start))]));
        for (i, seg) in run.segs.iter().enumerate() {
            let last = i + 1 == run.segs.len();
            let to = if last && closes { run.start } else { seg.end() };
            let at = next();
            let new_row = match *seg {
                Seg::Line(_) => row(at, "LineTo", vec![("X", frame.x_cell(to)), ("Y", frame.y_cell(to))]),
                Seg::Cubic(a, b, _) => cubic_row(at, frame, (a, b), to),
                Seg::Arc { bow, .. } => row(at, "ArcTo", vec![("X", frame.x_cell(to)), ("Y", frame.y_cell(to)), ("A", length(bow))]),
            };
            section.append_row(new_row);
        }
    }
    section
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn square(x0: f64, y0: f64, x1: f64, y1: f64) -> BezPath {
        let mut p = BezPath::new();
        p.move_to((x0, y0));
        p.line_to((x1, y0));
        p.line_to((x1, y1));
        p.line_to((x0, y1));
        p.close_path();
        p
    }

    fn text(cell: Option<&Cell>) -> (Option<String>, Option<String>) {
        let cell = cell.unwrap();
        (cell.formula.text().map(str::to_owned), cell.value.as_ref().map(|v| v.text().to_owned()))
    }

    #[test]
    fn a_share_is_twelve_places_with_the_zeros_off_and_nothing_too_small_to_show() {
        assert_eq!(share_text(2.0 / 3.0), "0.666666666667");
        assert_eq!(share_text(0.5), "0.5");
        assert_eq!(share_text(1.0 / 3.0), "0.333333333333");
        assert_eq!(share_text(-0.25), "-0.25");
        assert_eq!(share_text(1.0e-14), "0");
        assert_eq!(share_text(-1.0e-14), "0");
    }

    #[test]
    fn a_share_is_the_offset_over_the_side_to_twelve_places_and_a_flat_side_or_a_bad_offset_has_only_zero() {
        assert_eq!(share_of(0.5, 2.0), 0.25);
        assert_eq!(share_of(2.0, 3.0), 0.666666666667);
        // Within half of the last place of 0 or of 1 it is that number.
        assert_eq!(share_of(4.0e-13, 1.0), 0.0);
        assert_eq!(share_of(-4.0e-13, 1.0), 0.0);
        assert_eq!(share_of(1.0 - 4.0e-13, 1.0), 1.0);
        assert_eq!(share_of(1.0 + 4.0e-13, 1.0), 1.0);
        // Past it, it shows.
        assert_eq!(share_of(2.0e-12, 1.0), 2.0e-12);
        assert_eq!(share_of(1.0 + 2.0e-12, 1.0), 1.000000000002);
        // Outside the box a share is negative or above 1, which the box of a curve's control points may be.
        assert_eq!(share_of(-0.25, 1.0), -0.25);
        assert_eq!(share_of(1.5, 1.0), 1.5);
        // A flat side, a side that is no number and an offset that is none or too big for the side give 0.
        assert_eq!(share_of(1.0, 0.0), 0.0);
        assert_eq!(share_of(1.0, -2.0), 0.0);
        assert_eq!(share_of(1.0, f64::NAN), 0.0);
        assert_eq!(share_of(f64::NAN, 1.0), 0.0);
        assert_eq!(share_of(f64::INFINITY, 1.0), 0.0);
        assert_eq!(share_of(f64::MAX, 1.0e-300), 0.0);
    }

    #[test]
    fn the_formulas_a_share_makes_parse_even_when_it_is_negative() {
        for k in [0.5, 2.0 / 3.0, -0.25, 1.75] {
            let cell = share_cell("Width", k, 3.0);
            let source = cell.formula.text().unwrap().to_owned();
            assert!(diacraft_formula::parse(&source).ast.is_ok(), "{source}");
        }
    }

    #[test]
    fn a_share_of_zero_is_the_constant_and_a_share_of_one_is_the_plain_cell_and_a_flat_box_has_only_zero() {
        assert_eq!(text(Some(&share_cell("Width", 0.0, 3.0))), (None, Some("0".into())));
        assert_eq!(text(Some(&share_cell("Height", 1.0, 2.0))), (Some("Height".into()), Some("2".into())));
        assert_eq!(text(Some(&share_cell("Width", 0.5, 0.0))), (None, Some("0".into())));
        assert_eq!(text(Some(&share_cell("Width", 2.0 / 3.0, 3.0))), (Some("Width*0.666666666667".into()), Some("2".into())));
    }

    #[test]
    fn a_ring_of_four_bars_loses_the_vertices_a_straight_side_passes_through() {
        let mut path = BezPath::new();
        for p in [(0.0, 1.0), (0.0, 0.0), (1.0, 0.0), (2.0, 0.0), (3.0, 0.0), (3.0, 1.0), (3.0, 2.0), (3.0, 3.0), (0.0, 3.0), (0.0, 2.0)] {
            if path.elements().is_empty() {
                path.move_to(p);
            } else {
                path.line_to(p);
            }
        }
        path.close_path();
        let loops = clean(&path);
        assert_eq!(loops.len(), 1);
        let corners: Vec<(f64, f64)> = loops[0].nodes.iter().map(|n| (n.p.x, n.p.y)).collect();
        assert_eq!(corners.len(), 4, "{corners:?}");
        for corner in [(0.0, 0.0), (3.0, 0.0), (3.0, 3.0), (0.0, 3.0)] {
            assert!(corners.contains(&corner), "{corner:?} in {corners:?}");
        }
    }

    #[test]
    fn a_loop_run_clockwise_is_turned_so_that_the_outer_boundary_is_anticlockwise_and_a_hole_stays_clockwise() {
        // A clockwise outer boundary with a hole run anticlockwise: the library's convention, flipped.
        let mut path = square(0.0, 0.0, 4.0, 4.0).reverse_subpaths();
        let hole = square(1.0, 1.0, 2.0, 2.0);
        for el in hole.elements() {
            path.push(*el);
        }
        let loops = clean(&path);
        assert_eq!(loops.len(), 2);
        let areas: Vec<f64> = loops.iter().map(Loop::area).collect();
        assert!((areas[0] - 16.0).abs() < 1e-9 && (areas[1] + 1.0).abs() < 1e-9, "{areas:?}");
    }

    #[test]
    fn a_region_under_the_area_floor_is_dropped_and_a_very_short_edge_is_not_one() {
        let tiny = square(0.0, 0.0, 1.0e-5, 1.0e-5);
        assert!(clean(&tiny).is_empty());
        // A vertex a millionth from its neighbour in the top edge.
        let mut path = BezPath::new();
        path.move_to((0.0, 0.0));
        path.line_to((2.0, 0.0));
        path.line_to((2.0, 2.0));
        path.line_to((1.0, 2.0));
        path.line_to((1.0 - 1.0e-6, 2.0));
        path.line_to((0.0, 2.0));
        path.close_path();
        let loops = clean(&path);
        assert_eq!(loops[0].nodes.len(), 4, "the short edge and the vertex it leaves in a straight run are gone");
    }

    #[test]
    fn a_square_is_written_as_a_move_and_four_lines_ending_on_the_first_formulas() {
        let loops = clean(&square(1.0, 1.0, 3.0, 3.0));
        let frame = Frame::of(bounds(&loops).unwrap());
        assert_eq!((frame.width, frame.height), (2.0, 2.0));
        let section = section_of(&loops[0], &frame, 0);
        let kinds: Vec<&str> = section.rows.iter().map(|r| r.row_type.as_deref().unwrap()).collect();
        assert_eq!(kinds, ["MoveTo", "LineTo", "LineTo", "LineTo", "LineTo"]);
        let first = &section.rows[0];
        let last = section.rows.last().unwrap();
        for name in ["X", "Y"] {
            assert_eq!(text(first.cells.get(name)), text(last.cells.get(name)), "the loop ends on the formulas its MoveTo starts with");
        }
        let flag = |name: &str| text(section.cells.get(name)).1.unwrap();
        assert_eq!([flag("NoFill"), flag("NoLine"), flag("NoShow"), flag("NoSnap"), flag("NoQuickDrag")], ["0", "0", "0", "0", "0"]);
        assert_eq!(section.rows.iter().map(|r| r.ix.unwrap()).collect::<Vec<_>>(), [1, 2, 3, 4, 5]);
    }

    #[test]
    fn a_cubic_is_one_clamped_nurbs_row_with_relative_control_points() {
        // Anticlockwise: along the bottom to the right, then the arch back over the top to the start.
        let mut path = BezPath::new();
        path.move_to((0.0, 0.0));
        path.line_to((4.0, 0.0));
        path.curve_to((3.0, 2.0), (1.0, 2.0), (0.0, 0.0));
        path.close_path();
        let loops = clean(&path);
        let frame = Frame { min: Point::new(0.0, 0.0), width: 4.0, height: 2.0 };
        let section = section_of(&loops[0], &frame, 0);
        let nurbs = section.rows.iter().find(|r| r.row_type.as_deref() == Some("NURBSTo")).unwrap();
        assert_eq!(text(nurbs.cells.get("E")).1.unwrap(), "NURBS(1,3,0,0,0.75,1,0,1,0.25,1,0,1)");
        assert_eq!(nurbs.cells.get("E").unwrap().unit.as_deref(), Some("NURBS"));
        let cell = |name: &str| text(nurbs.cells.get(name)).1.unwrap();
        assert_eq!([cell("A"), cell("B"), cell("C"), cell("D")], ["0", "1", "0", "1"]);
        assert_eq!(text(nurbs.cells.get("X")), (None, Some("0".into())), "it ends at the start, whose x is the constant 0");
        assert_eq!(text(nurbs.cells.get("Y")), (None, Some("0".into())));
    }

    #[test]
    fn the_extent_of_a_curve_is_its_true_extent_and_not_its_control_points() {
        let mut path = BezPath::new();
        path.move_to((0.0, 0.0));
        path.curve_to((1.0, 2.0), (3.0, 2.0), (4.0, 0.0));
        path.close_path();
        let loops = clean(&path);
        let rect = bounds(&loops).unwrap();
        assert!((rect.y1 - 1.5).abs() < 1e-9, "the top of the arch is at 1.5 and not 2: {rect:?}");
    }

    #[test]
    fn a_quadratic_is_raised_to_a_cubic_and_a_reversed_loop_runs_the_other_way_round() {
        let mut path = BezPath::new();
        path.move_to((0.0, 0.0));
        path.quad_to((2.0, 2.0), (4.0, 0.0));
        path.close_path();
        let ring = &rings_of(&path)[0];
        assert!(matches!(ring.nodes[0].out, Edge::Cubic(..)));
        let reversed = ring.reversed();
        assert!((ring.area() + reversed.area()).abs() < 1e-12, "{} {}", ring.area(), reversed.area());
        assert_eq!(reversed.reversed().nodes, ring.nodes);
    }

    #[test]
    fn a_quadratic_is_raised_to_the_cubic_with_the_same_curve() {
        let mut path = BezPath::new();
        path.move_to((0.0, 0.0));
        path.quad_to((2.0, 2.0), (4.0, 0.0));
        path.close_path();
        let Edge::Cubic(a, b) = rings_of(&path)[0].nodes[0].out else { panic!("a cubic") };
        // The control points of a quadratic's degree elevation: two thirds of the way from each end to its one.
        assert!((a.x - 4.0 / 3.0).abs() < 1e-12 && (a.y - 4.0 / 3.0).abs() < 1e-12, "{a:?}");
        assert!((b.x - 8.0 / 3.0).abs() < 1e-12 && (b.y - 4.0 / 3.0).abs() < 1e-12, "{b:?}");
    }

    /// A ring of five nodes with curved and straight edges, to turn round.
    fn mixed() -> Loop {
        let p = |x: f64, y: f64| Point::new(x, y);
        Loop {
            nodes: vec![
                Node { p: p(0.0, 0.0), out: Edge::Line },
                Node { p: p(4.0, 0.0), out: Edge::Cubic(p(5.0, 1.0), p(5.0, 2.0)) },
                Node { p: p(4.0, 3.0), out: Edge::Line },
                Node { p: p(2.0, 4.0), out: Edge::Cubic(p(1.0, 5.0), p(0.0, 4.0)) },
                Node { p: p(0.0, 3.0), out: Edge::Line },
            ],
        }
    }

    #[test]
    fn a_loop_turned_round_draws_the_same_outline_the_other_way_and_turned_again_is_the_loop() {
        let ring = mixed();
        let reversed = ring.reversed();
        assert_eq!(reversed.nodes.len(), 5);
        assert!((ring.area() + reversed.area()).abs() < 1e-9, "{} {}", ring.area(), reversed.area());
        assert_eq!(reversed.reversed().nodes, ring.nodes);
        // Every vertex is still a vertex, and the first stays first.
        for node in &ring.nodes {
            assert!(reversed.nodes.iter().any(|r| r.p == node.p), "{:?}", node.p);
        }
        assert_eq!(reversed.nodes[0].p, ring.nodes[0].p);
        // The way round is the other: from the first vertex the reversed loop goes to the one the loop came from, along the same curve turned round.
        assert_eq!(reversed.nodes[1].p, ring.nodes[4].p);
        assert_eq!(reversed.nodes[0].out, Edge::Line, "the straight edge the loop came into the first vertex by");
        assert_eq!(
            reversed.nodes[1].out,
            Edge::Cubic(Point::new(0.0, 4.0), Point::new(1.0, 5.0)),
            "the curve between the last two, its control points swapped"
        );
        assert_eq!(reversed.nodes[2].out, Edge::Line);
        assert_eq!(reversed.nodes[3].out, Edge::Cubic(Point::new(5.0, 2.0), Point::new(5.0, 1.0)));
    }

    #[test]
    fn a_vertex_is_taken_out_of_a_straight_run_by_how_far_it_is_from_the_line_and_a_spike_back_is_not_a_straight_run() {
        let at = |x: f64, y: f64| Point::new(x, y);
        // Within a billionth of the line: straight. A millionth off it: a vertex.
        assert!(passes_straight(at(0.0, 0.0), at(1.0, 1.0e-10), at(2.0, 0.0)));
        assert!(!passes_straight(at(0.0, 0.0), at(1.0, 1.0e-6), at(2.0, 0.0)));
        // The line is the line through the two ends, and the middle must be between them: a vertex that goes out and back is a spike.
        assert!(!passes_straight(at(0.0, 0.0), at(2.0, 0.0), at(1.0, 0.0)));
        assert!(!passes_straight(at(0.0, 0.0), at(1.0, 0.0), at(0.0, 0.0)));
        // Far from the origin the rounding of the coordinates is allowed for: a hundred-thousandth is a vertex near the origin and rounding at 1E8.
        assert!(!passes_straight(at(0.0, 0.0), at(1.0, 1.0e-5), at(2.0, 0.0)));
        assert!(passes_straight(at(1.0e8, 0.0), at(1.0e8 + 1.0, 1.0e-5), at(1.0e8 + 2.0, 0.0)));
        // Not a straight run when the vertex is far from the line, at any distance.
        assert!(!passes_straight(at(1.0e8, 0.0), at(1.0e8 + 1.0, 0.5), at(1.0e8 + 2.0, 0.0)));
        // The distance is from the line, not from the end: a long run with a vertex a hair off in the middle is straight.
        assert!(passes_straight(at(0.0, 0.0), at(500.0, 1.0e-10), at(1000.0, 0.0)));
        assert!(!passes_straight(at(0.0, 0.0), at(500.0, 1.0e-3), at(1000.0, 0.0)));
    }

    #[test]
    fn a_vertex_beside_a_curve_stays_even_when_it_lies_on_the_line_through_its_neighbours() {
        let p = |x: f64, y: f64| Point::new(x, y);
        // A to B along a curve, then straight on to C, with A, B and C in a line: B is where the curve ends and stays.
        let ring = Loop {
            nodes: vec![
                Node { p: p(0.0, 0.0), out: Edge::Cubic(p(0.3, 1.0), p(0.7, 1.0)) },
                Node { p: p(1.0, 0.0), out: Edge::Line },
                Node { p: p(2.0, 0.0), out: Edge::Line },
            ],
        };
        assert_eq!(simplify(&ring).nodes.len(), 3);
        // And the same with the curve after it.
        let ring = Loop {
            nodes: vec![
                Node { p: p(0.0, 0.0), out: Edge::Line },
                Node { p: p(1.0, 0.0), out: Edge::Cubic(p(1.3, 1.0), p(1.7, 1.0)) },
                Node { p: p(2.0, 0.0), out: Edge::Line },
            ],
        };
        assert_eq!(simplify(&ring).nodes.len(), 3);
    }

    #[test]
    fn a_ring_of_three_with_a_vertex_on_the_line_of_the_other_two_loses_it_and_a_square_loses_none() {
        let p = |x: f64, y: f64| Point::new(x, y);
        let line = |x, y| Node { p: p(x, y), out: Edge::Line };
        assert_eq!(simplify(&Loop { nodes: vec![line(0.0, 0.0), line(1.0, 0.0), line(2.0, 0.0)] }).nodes.len(), 2);
        assert_eq!(simplify(&Loop { nodes: vec![line(0.0, 0.0), line(2.0, 0.0), line(2.0, 2.0), line(0.0, 2.0)] }).nodes.len(), 4);
        // A vertex a millionth from the next goes, as an edge of that length is no edge.
        assert_eq!(
            simplify(&Loop { nodes: vec![line(0.0, 0.0), line(2.0, 0.0), line(2.0 + 1.0e-6, 0.0), line(2.0, 2.0), line(0.0, 2.0)] }).nodes.len(),
            4
        );
    }

    #[test]
    fn a_contour_with_a_point_that_is_not_a_number_is_left_out() {
        let mut path = square(0.0, 0.0, 1.0, 1.0);
        path.move_to((f64::NAN, 0.0));
        path.line_to((1.0, 1.0));
        path.line_to((0.0, 1.0));
        path.close_path();
        assert_eq!(clean(&path).len(), 1);
    }
}
