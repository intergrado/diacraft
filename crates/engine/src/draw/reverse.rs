//! Reverse Ends.
//!
//! The cells and the swap are the specifications'; the path, the label and the words are ours. Reverse Ends swaps the begin and the end of a
//! one-dimensional shape, so that the flow runs the other way, and **keeps the drawing**: the connector is the same line from the same shape to the same shape, walked the
//! other way. For each shape named that has a begin and an end:
//!
//! * **The ends.** `BeginX` and `EndX`, `BeginY` and `EndY` change cells, each with its formula and its cached value and unit, so the glue formulas (`PAR(PNT(Sheet.4!...))`)
//!   travel with the ends they place. A reference in a swapped formula to the shape's own `BeginX`, `BeginY`, `EndX`, `EndY`, `BegTrigger` or `EndTrigger` names the other cell
//!   after (`BeginX` names `BegTrigger` first, so the `_WALKGLUE` formulas keep their text and the triggers swap), and **`BegTrigger` and `EndTrigger` swap**
//!   (each names the shape its end is glued to) when the shape has both; a shape that has one only, as a connector made here with one end glued to the whole of a shape
//!   Has it go to the other end, which holds 0 where it was.
//! * **The glue.** The page's `Connect` rows of the shape swap `FromCell` (`BeginX` and `EndX`, `BeginY` and `EndY`) and `FromPart` (7 and 10, 8 and 11, 9 and 12) and keep
//!   everything else: both ends stay glued to the same shapes and the same points, or the whole shape.
//! * **The frame.** `Width`, `Angle`, `PinX` and `PinY` follow by their formulas (`Angle` turns by a half turn), through one recalculation of what the swap wrote and what reads
//!   it, as the cells of a group moved are worked out: no end is placed by the engine's rule and no route is made again, so nothing about the shape's route or glue is guessed.
//! * **The path.** The rows of every `Geometry` section are carried from the frame the shape had to the frame it has, and **walked the other way**: the first row is the point
//!   the path ended at, and each row goes back to the point the one before it came from. A line stays a line, an arc keeps the side it bows to on the page (`A` changes sign),
//!   an elliptical arc keeps its middle point, a polyline's points and a spline's control points, knots and weights go in the other order. So **a fixed route and a hand-edited
//!   leg are the same line**, `ConFixedCode` stays, and the last row follows `Width` as it did. A path that its own rows already draw the other way (a straight line) is left
//!   as it is, formulas and all. Line ends (`BeginArrow`, `EndArrow`, their sizes and every other format cell) stay on their own cells: the renderer puts them on the path's first
//!   and last vertex, so **the arrow ends up at the other end**, as the documentation says.
//! * **The label.** The text block keeps its place on the page and its direction there: `TxtPinX` and `TxtPinY` are carried through the frame, and `TxtAngle` is worked out
//!   so that the block lies as it did (a label the connector keeps upright stays upright). A cell already right is not written.
//! * **Many shapes** are one edit. A shape that has no begin and end is left alone and listed, one whose `LockBegin` or `LockEnd` is on (acts on the shapes it can, and
//!   lists the locked ones) and one with a cell that `GUARD` protects likewise; a shape on a locked layer is refused by the registry (`cmd/locked.rs`).
//! * **A path this command cannot read** (a row of a kind it does not carry over, a section of more rows than a call reads) is left as it is, and the shape is listed in
//!   [`Reversal::geometry_kept`]: its ends and glue swap, and it is drawn from its new begin as its own rows say.
//!
//! **What it costs.** One walk of the page finds the shapes. The rows read are at most [`MAX_SHAPE_ROWS`] for a shape and [`MAX_REVERSE_ROWS`] for the call, counted before any
//! is merged, and at most [`MAX_SECTIONS`] sections for a shape; past them the path is kept as it is. One batch of cell writes swaps the ends, one pass over the page's `Connect`
//! rows swaps the glue, and one commit works out the cells that read the ends, whatever the number of shapes.

use std::collections::{BTreeSet, HashSet};
use std::sync::Arc;

use diacraft_calc::Changes;
use diacraft_doc::{
    Cell, CellAddr, CellEdit, CellName, CellWrite, Connect, Connects, Document, EffectiveRow, FormulaSlot, Row, RowKey, SectionKey, Shape, ShapeRef,
    SheetRef, Value, WriteOrigin, WriteOutcome,
};
use diacraft_geom::GeomRow;
use kurbo::{Affine, Point};

use super::connectors::write_section;
use super::read::{Pose, Reader};
use super::route::P;
use super::rows::{BOW_UNIT, length, plain, polyline_text, put};
use super::scan;
use super::settle::{recalculate_sections, route_after, settle_kept};
use super::shapeops::{CRow, NurbsKnots, carry};
use super::{Problem, page_mut, require_page};
use crate::stencil::number_text;

/// The cells that swap, each with the one it swaps with: the place of the begin and the end (the two coordinates) and the triggers that name the shapes they are glued to.
const PAIRS: [(&str, &str); 3] = [("BeginX", "EndX"), ("BeginY", "EndY"), ("BegTrigger", "EndTrigger")];

/// The most rows of paths a call reads in all, counted before any is merged: past it the path of a shape is kept as it is.
pub const MAX_REVERSE_ROWS: usize = 200_000;

/// The most rows one section of a shape's path may have to be reversed.
const MAX_SHAPE_ROWS: usize = super::hand::MAX_ROWS;

/// The most `Geometry` sections a shape may have to have its path reversed.
const MAX_SECTIONS: usize = 16;

/// How far two coordinates, knots or ratios may be and be the same.
const SAME: f64 = 1.0e-9;

/// What a call of [`reverse_ends`] did. The shapes of `reversed` are in document order.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Reversal {
    /// The shapes whose begin and end were swapped.
    pub reversed: Vec<u32>,
    /// The shapes left alone because their `LockBegin` or `LockEnd` is on.
    pub locked: Vec<u32>,
    /// The other shapes left alone, each with the reason, in words.
    pub skipped: Vec<NotReversed>,
    /// Shapes of `reversed` whose path was left as their rows draw it, because it could not be read.
    pub geometry_kept: Vec<u32>,
}

/// A shape the call left as it was, and why (a whole sentence that begins with the shape).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotReversed {
    pub id: u32,
    pub why: String,
}

/// Up to five `ids` as words: `3, 4, 9` and, past five, `3, 4, 9, 12, 13 and 40 more`.
fn listed(ids: &[u32]) -> String {
    let shown: Vec<String> = ids.iter().take(5).map(u32::to_string).collect();
    match ids.len().saturating_sub(5) {
        0 => shown.join(", "),
        more => format!("{} and {more} more", shown.join(", ")),
    }
}

fn protected_words(id: u32) -> String {
    format!("shape {id} has its begin or end protected (LockBegin or LockEnd), so it is left as it is")
}

impl Reversal {
    /// Why nothing was reversed, in words: the reason of the one shape named, or the first few of the several.
    pub fn refusal(&self, named: usize) -> String {
        let mut reasons: Vec<String> = self.skipped.iter().map(|k| k.why.clone()).collect();
        reasons.extend(self.locked.iter().map(|id| protected_words(*id)));
        match reasons.as_slice() {
            [only] => only.clone(),
            [] => format!("none of the {named} shapes named has a begin and an end to swap"),
            many => {
                let more = many.len().saturating_sub(3);
                let tail = if more > 0 { format!("; and {more} more") } else { String::new() };
                format!("none of the {named} shapes named could be reversed: {}{tail}", many.iter().take(3).cloned().collect::<Vec<_>>().join("; "))
            }
        }
    }

    /// What the call did, in a sentence or two.
    pub fn says(&self) -> String {
        let mut words = match self.reversed.as_slice() {
            [only] => format!("Reversed shape {only}: its begin and end are swapped, with their glue, and the arrows stay on the ends of the path."),
            many => format!(
                "Reversed {} shapes: their begin and end are swapped, with their glue, and the arrows stay on the ends of the path.",
                many.len()
            ),
        };
        if !self.geometry_kept.is_empty() {
            words.push_str(&format!(
                " The path of shape {} could not be reversed (its rows are of a kind this command does not carry over, or too many), so it is drawn from its new begin as its own rows say.",
                listed(&self.geometry_kept)
            ));
        }
        let left: Vec<u32> = self.locked.iter().copied().chain(self.skipped.iter().map(|k| k.id)).collect();
        if !left.is_empty() {
            let (what, were) = if left.len() == 1 { ("shape", "it was") } else { ("shapes", "they were") };
            words.push_str(&format!(" Left as {were}: {what} {}.", listed(&left)));
        }
        words
    }
}

// ------------------------------------------------------------------ the formulas

/// The cell a reference to `name` names after a swap, if `name` is one of the cells that swap (in any letter case).
fn other_name(name: &str) -> Option<&'static str> {
    PAIRS.iter().find_map(|(a, b)| {
        if name.eq_ignore_ascii_case(a) {
            Some(*b)
        } else if name.eq_ignore_ascii_case(b) {
            Some(*a)
        } else {
            None
        }
    })
}

/// `source` with each reference to the shape's own `BeginX`, `EndX`, `BeginY`, `EndY`, `BegTrigger` or `EndTrigger` (no sheet before it, no index) naming the other one;
/// `None` when it has none, or does not parse (it is then moved as it is). The rest of the text is kept character for character.
fn swapped_refs(source: &str) -> Option<String> {
    let spans = diacraft_formula::reference_spans(source).ok()?;
    let mut out = String::with_capacity(source.len());
    let mut copied = 0_usize;
    for span in &spans {
        let reference = &span.reference;
        let [segment] = reference.path.as_slice() else { continue };
        if !reference.qualifiers.is_empty() || segment.index.is_some() || span.start < copied {
            continue;
        }
        let Some(other) = other_name(&segment.name) else { continue };
        out.push_str(source.get(copied..span.start)?);
        out.push_str(other);
        copied = span.end;
    }
    if copied == 0 {
        return None;
    }
    out.push_str(source.get(copied..)?);
    Some(out)
}

/// The cell `name` of `shape` as a swap moves it: the cell the shape holds, its formula's references to its own ends turned; a formula that is inherited stays with its master and the
/// cell moves as the number it has; a cell the shape does not hold moves as the value it inherits. `None` for a cell with no value anywhere.
fn moved_cell(reader: &Reader<'_>, shape: &Shape, sheet: &SheetRef, name: &str) -> Option<Cell> {
    if let Some(held) = shape.sheet.cells.get(&CellName::new(name)) {
        let mut cell = held.clone();
        match &held.formula {
            FormulaSlot::Local(formula) => {
                if let Some(text) = swapped_refs(&formula.source) {
                    cell.formula = FormulaSlot::Local(Arc::new(diacraft_formula::parse(&text)));
                }
            }
            FormulaSlot::Inherit => cell.formula = FormulaSlot::Absent,
            FormulaSlot::Absent | FormulaSlot::NoFormula => {}
        }
        return cell.value.is_some().then_some(cell);
    }
    let found = reader.resolver().resolve(sheet, &CellAddr::direct(name))?;
    let value = found.value.cell.and_then(|c| c.value.clone())?;
    let unit = found.formula.cell.and_then(|c| c.unit.clone()).or_else(|| found.value.cell.and_then(|c| c.unit.clone()));
    Some(Cell { value: Some(value), unit, ..Cell::default() })
}

// ------------------------------------------------------------------ the path

/// The geometry crate's rows as the path of a shape can be reversed: `Ellipse`, `InfiniteLine`, the spline rows and the relative Bezier rows are not carried over, and a
/// constructor that cannot be read is not guessed.
fn reversible(rows: &[GeomRow]) -> bool {
    rows.iter().all(|row| match row {
        GeomRow::MoveTo { .. }
        | GeomRow::RelMoveTo { .. }
        | GeomRow::LineTo { .. }
        | GeomRow::RelLineTo { .. }
        | GeomRow::ArcTo { .. }
        | GeomRow::EllipticalArcTo { .. }
        | GeomRow::RelEllipticalArcTo { .. } => true,
        GeomRow::PolylineTo { polyline, .. } => polyline.is_some(),
        GeomRow::NurbsTo { nurbs, .. } => nurbs.is_some(),
        GeomRow::SplineStart { .. }
        | GeomRow::SplineKnot { .. }
        | GeomRow::RelCubBezTo { .. }
        | GeomRow::RelQuadBezTo { .. }
        | GeomRow::Ellipse { .. }
        | GeomRow::InfiniteLine { .. } => false,
    })
}

/// The point a row of a path ends at, for the rows [`reverse_rows`] carries over.
fn end_of(row: &CRow) -> Option<Point> {
    match row {
        CRow::Move(to)
        | CRow::Line(to)
        | CRow::Arc { to, .. }
        | CRow::EllipticalArc { to, .. }
        | CRow::Polyline { to, .. }
        | CRow::Nurbs { to, .. } => Some(*to),
        CRow::Ellipse { .. } | CRow::Cubic { .. } | CRow::Quad { .. } | CRow::SplineStart { .. } | CRow::SplineKnot { .. } => None,
    }
}

/// A clamped spline walked the other way: its control points, weights and knots go in the other order, the knots mirrored on the span they cover. `back` is where the reversed
/// row ends (where the row started); `None` for a spline whose knots are not clamped at its start, since its mirror is not a row the file format writes.
fn reverse_nurbs(back: Point, knots: &NurbsKnots, last_knot: f64, degree: f64, via: &[(Point, f64, f64)]) -> Option<CRow> {
    let whole = degree.round();
    if !(degree.is_finite() && (degree - whole).abs() < SAME && (1.0..=25.0).contains(&whole)) {
        return None;
    }
    // The degree is a whole number from 1 to 25.
    let d = whole as usize;
    let n = via.len().checked_add(2)?;
    if n <= d {
        return None;
    }
    let mut listed: Vec<f64> = Vec::with_capacity(n + 1);
    listed.push(knots.first_knot);
    listed.extend(via.iter().map(|v| v.1));
    listed.push(knots.second_last_knot);
    listed.push(last_knot);
    let mut weights: Vec<f64> = Vec::with_capacity(n);
    weights.push(knots.first_weight);
    weights.extend(via.iter().map(|v| v.2));
    weights.push(knots.last_weight);
    if !listed.iter().chain(&weights).all(|v| v.is_finite()) {
        return None;
    }
    // The knots the curve has: the listed ones, then the last one `degree` times.
    let mut full = listed;
    full.extend(std::iter::repeat_n(last_knot, d));
    let total = full.first()? + last_knot;
    let mirrored: Vec<f64> = full.iter().rev().map(|k| total - k).collect();
    let tolerance = SAME * total.abs().max(1.0);
    let sorted = mirrored.windows(2).all(|w| w[0] <= w[1] + tolerance);
    let clamped = mirrored.get(n..).is_some_and(|tail| tail.iter().all(|k| (k - tail[0]).abs() <= tolerance));
    if !sorted || !clamped {
        return None;
    }
    weights.reverse();
    let points: Vec<(Point, f64, f64)> =
        (1..n - 1).map(|i| Some((via.get(n - 2 - i)?.0, *mirrored.get(i)?, *weights.get(i)?))).collect::<Option<Vec<_>>>()?;
    Some(CRow::Nurbs {
        to: back,
        knots: NurbsKnots {
            second_last_knot: *mirrored.get(n - 1)?,
            last_weight: *weights.get(n - 1)?,
            first_knot: *mirrored.first()?,
            first_weight: *weights.first()?,
        },
        last_knot: *mirrored.get(n)?,
        degree,
        points,
    })
}

/// One row of a path walked back to the point it started from (`back`).
fn reverse_row(row: &CRow, back: Point) -> Option<CRow> {
    Some(match row {
        CRow::Line(_) => CRow::Line(back),
        // The same arc seen from its other end bows to the other side of the way of travel.
        CRow::Arc { bow, .. } => CRow::Arc { to: back, bow: -bow },
        CRow::EllipticalArc { mid, angle, ratio, .. } => CRow::EllipticalArc { to: back, mid: *mid, angle: *angle, ratio: *ratio },
        CRow::Polyline { points, .. } => CRow::Polyline { to: back, points: points.iter().rev().copied().collect() },
        CRow::Nurbs { knots, last_knot, degree, points, .. } => reverse_nurbs(back, knots, *last_knot, *degree, points)?,
        CRow::Move(_) | CRow::Ellipse { .. } | CRow::Cubic { .. } | CRow::Quad { .. } | CRow::SplineStart { .. } | CRow::SplineKnot { .. } => {
            return None;
        }
    })
}

/// A path walked the other way: each run of rows (a `MoveTo` and what follows it) goes back from its last point to its first, and the runs go in the other order, so
/// the first row is the point the path ended at. `None` for a path with a row [`end_of`] does not know, or one that does not start with a `MoveTo`.
fn reverse_rows(rows: &[CRow]) -> Option<Vec<CRow>> {
    let mut runs: Vec<(Point, Vec<&CRow>)> = Vec::new();
    for row in rows {
        match row {
            CRow::Move(at) => runs.push((*at, Vec::new())),
            other => runs.last_mut()?.1.push(other),
        }
    }
    let mut out: Vec<CRow> = Vec::with_capacity(rows.len());
    for (start, run) in runs.iter().rev() {
        let mut places: Vec<Point> = Vec::with_capacity(run.len() + 1);
        places.push(*start);
        for row in run {
            places.push(end_of(row)?);
        }
        out.push(CRow::Move(*places.last()?));
        for (i, row) in run.iter().enumerate().rev() {
            // The row `i` went from `places[i]` to `places[i + 1]`: walked back it ends where it began.
            out.push(reverse_row(row, *places.get(i)?)?);
        }
    }
    Some(out)
}

fn near_point(a: Point, b: Point) -> bool {
    (a.x - b.x).abs() < SAME && (a.y - b.y).abs() < SAME
}

/// Whether two rows are the same row, to a billionth.
fn same_row(a: &CRow, b: &CRow) -> bool {
    match (a, b) {
        (CRow::Move(p), CRow::Move(q)) | (CRow::Line(p), CRow::Line(q)) => near_point(*p, *q),
        (CRow::Arc { to, bow }, CRow::Arc { to: to2, bow: bow2 }) => near_point(*to, *to2) && (bow - bow2).abs() < SAME,
        (CRow::EllipticalArc { to, mid, angle, ratio }, CRow::EllipticalArc { to: to2, mid: mid2, angle: angle2, ratio: ratio2 }) => {
            near_point(*to, *to2) && near_point(*mid, *mid2) && (angle - angle2).abs() < SAME && (ratio - ratio2).abs() < SAME
        }
        (CRow::Polyline { to, points }, CRow::Polyline { to: to2, points: points2 }) => {
            near_point(*to, *to2) && points.len() == points2.len() && points.iter().zip(points2).all(|(p, q)| near_point(*p, *q))
        }
        (
            CRow::Nurbs { to, knots, last_knot, degree, points },
            CRow::Nurbs { to: to2, knots: knots2, last_knot: last2, degree: degree2, points: points2 },
        ) => {
            near_point(*to, *to2)
                && (knots.first_knot - knots2.first_knot).abs() < SAME
                && (knots.second_last_knot - knots2.second_last_knot).abs() < SAME
                && (knots.first_weight - knots2.first_weight).abs() < SAME
                && (knots.last_weight - knots2.last_weight).abs() < SAME
                && (last_knot - last2).abs() < SAME
                && (degree - degree2).abs() < SAME
                && points.len() == points2.len()
                && points.iter().zip(points2).all(|(p, q)| near_point(p.0, q.0) && (p.1 - q.1).abs() < SAME && (p.2 - q.2).abs() < SAME)
        }
        _ => false,
    }
}

fn same_rows(a: &[CRow], b: &[CRow]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| same_row(x, y))
}

/// The row `ix` of a path, written as constants in the shape's own frame as a route is (`rows.rs`). `follow_width` makes the `X` of the last row `Width*1`, as the last row
/// of a file's fixed route is. `None` for a row that is not one of the kinds a reversed path holds.
fn emit_row(i: usize, row: &CRow, follow_width: bool, last: usize) -> Option<Arc<Row>> {
    let ix = u32::try_from(i.checked_add(1)?).unwrap_or(u32::MAX);
    let (kind, to) = match row {
        CRow::Move(to) => ("MoveTo", *to),
        CRow::Line(to) => ("LineTo", *to),
        CRow::Arc { to, .. } => ("ArcTo", *to),
        CRow::EllipticalArc { to, .. } => ("EllipticalArcTo", *to),
        CRow::Polyline { to, .. } => ("PolylineTo", *to),
        CRow::Nurbs { to, .. } => ("NURBSTo", *to),
        CRow::Ellipse { .. } | CRow::Cubic { .. } | CRow::Quad { .. } | CRow::SplineStart { .. } | CRow::SplineKnot { .. } => return None,
    };
    let mut out = Row { ix: Some(ix), row_type: Some(Arc::from(kind)), ..Row::default() };
    let x = if follow_width && i == last && i > 0 {
        Cell::with_formula("Width*1", Some(Value::from_text(&number_text(to.x)))).with_unit("IN")
    } else {
        length(to.x)
    };
    put(&mut out, "X", x);
    put(&mut out, "Y", length(to.y));
    match row {
        CRow::Arc { bow, .. } => put(&mut out, "A", plain(*bow).with_unit(BOW_UNIT)),
        CRow::EllipticalArc { mid, angle, ratio, .. } => {
            put(&mut out, "A", length(mid.x));
            put(&mut out, "B", length(mid.y));
            put(&mut out, "C", plain(*angle));
            put(&mut out, "D", plain(*ratio));
        }
        CRow::Polyline { points, .. } => {
            let via: Vec<P> = points.iter().map(|p| (p.x, p.y)).collect();
            put(&mut out, "A", Cell::constant(Value::from_text(&polyline_text(&via))).with_unit("POLYLINE"));
        }
        CRow::Nurbs { knots, last_knot, degree, points, .. } => {
            let mut text = format!("NURBS({},{},1,1", number_text(*last_knot), number_text(*degree));
            for (p, knot, weight) in points {
                text.push_str(&format!(",{},{},{},{}", number_text(p.x), number_text(p.y), number_text(*knot), number_text(*weight)));
            }
            text.push(')');
            put(&mut out, "A", plain(knots.second_last_knot));
            put(&mut out, "B", plain(knots.last_weight));
            put(&mut out, "C", plain(knots.first_knot));
            put(&mut out, "D", plain(knots.first_weight));
            put(&mut out, "E", Cell::constant(Value::from_text(&text)).with_unit("NURBS"));
        }
        CRow::Move(_)
        | CRow::Line(_)
        | CRow::Ellipse { .. }
        | CRow::Cubic { .. }
        | CRow::Quad { .. }
        | CRow::SplineStart { .. }
        | CRow::SplineKnot { .. } => {}
    }
    Some(Arc::new(out))
}

/// The rows of a reversed path, numbered from 1.
fn emit(rows: &[CRow], follow_width: bool) -> Option<Vec<Arc<Row>>> {
    let last = rows.len().saturating_sub(1);
    rows.iter().enumerate().map(|(i, row)| emit_row(i, row, follow_width, last)).collect()
}

fn finite(rows: &[CRow]) -> bool {
    rows.iter().all(|row| match row {
        CRow::Move(p) | CRow::Line(p) => p.is_finite(),
        CRow::Arc { to, bow } => to.is_finite() && bow.is_finite(),
        CRow::EllipticalArc { to, mid, angle, ratio } => to.is_finite() && mid.is_finite() && angle.is_finite() && ratio.is_finite(),
        CRow::Polyline { to, points } => to.is_finite() && points.iter().all(|p| p.is_finite()),
        CRow::Nurbs { to, points, .. } => to.is_finite() && points.iter().all(|p| p.0.is_finite()),
        CRow::Ellipse { .. } | CRow::Cubic { .. } | CRow::Quad { .. } | CRow::SplineStart { .. } | CRow::SplineKnot { .. } => false,
    })
}

/// The map from the frame a shape had to the frame it has: a point of the old frame, carried to the page by the old pose and back by the new one. A half turn about the
/// pin for a connector whose `Angle` the swap turned; whatever the poses are, the shape's picture on the page is the same.
fn transfer(old: &Pose, new: &Pose) -> Affine {
    let at = |p: P| new.down(old.up(p));
    let (origin, x, y) = (at((0.0, 0.0)), at((1.0, 0.0)), at((0.0, 1.0)));
    Affine::new([x.0 - origin.0, x.1 - origin.1, y.0 - origin.0, y.1 - origin.1, origin.0, origin.1])
}

// ------------------------------------------------------------------ the plan

/// One `Geometry` section of a shape as it was, and what it takes to write it again.
struct OldSection {
    key: SectionKey,
    rows: Vec<GeomRow>,
    /// The `IX` of the rows the shape's master gives the section, so that rows the new path does not use do not stay in the merge.
    inherited: Option<Vec<u32>>,
    /// Whether the last row's `X` was a formula (a file's fixed route has `Width*1` there).
    follows_width: bool,
}

/// The path of a shape as it was, or why it is kept.
enum Path {
    /// The sections of the path, in order.
    Sections(Vec<OldSection>),
    /// Left as it is.
    Kept,
}

/// The label of a shape as it was: where the middle of its text block is in the shape's frame, and its `TxtAngle`.
struct Label {
    pin: P,
    angle: f64,
}

/// A shape to reverse, with what was read of it before anything was written.
struct Plan {
    id: u32,
    at: ShapeRef,
    sheet: SheetRef,
    pose: Pose,
    size: P,
    /// The cells that swap: each name and the cell it is given.
    cells: Vec<(&'static str, Cell)>,
    path: Path,
    label: Option<Label>,
}

/// What a shape comes to before anything is written.
enum Verdict {
    Reverse(Box<Plan>),
    Locked,
    Skip(String),
}

fn inherited_ix(reader: &Reader<'_>, at: &ShapeRef, key: &SectionKey) -> Option<Vec<u32>> {
    let step = reader.resolver().master_step(at).ok().flatten()?;
    let rows = reader.resolver().effective_rows(&SheetRef::Shape(step.shape), key)?;
    Some(rows.iter().filter_map(|row| if let RowKey::Ix(ix) = row.key { Some(ix) } else { None }).collect())
}

/// Whether the cell `X` of the last row holds a formula of its own.
fn last_x_is_formula(rows: &[EffectiveRow<'_>]) -> bool {
    rows.last()
        .and_then(|row| row.cells.iter().find(|(name, _)| name.as_str() == "X"))
        .is_some_and(|(_, found)| found.formula.cell.is_some_and(|cell| matches!(cell.formula, FormulaSlot::Local(_))))
}

/// The path of the shape at `at`: its `Geometry` sections, read within what the shape and the call may read (`budget` is what the call has left).
fn read_path(reader: &Reader<'_>, at: &ShapeRef, budget: &mut usize) -> Path {
    let sheet = SheetRef::Shape(at.clone());
    let keys = match reader.resolver().effective_sections_within(&sheet, "Geometry", MAX_SECTIONS) {
        None => return Path::Sections(Vec::new()),
        Some(Err(_)) => return Path::Kept,
        Some(Ok(keys)) => keys,
    };
    let mut sections = Vec::with_capacity(keys.len());
    for key in keys {
        let room = MAX_SHAPE_ROWS.min(*budget);
        let rows = match reader.resolver().effective_rows_within(&sheet, &key, room) {
            None => continue,
            Some(Err(_)) => return Path::Kept,
            Some(Ok(rows)) => rows,
        };
        *budget = budget.saturating_sub(rows.len());
        let (geom, skipped) = diacraft_render::geom_rows(&rows);
        if skipped > 0 || !reversible(&geom) {
            return Path::Kept;
        }
        let inherited = inherited_ix(reader, at, &key);
        sections.push(OldSection { key, rows: geom, inherited, follows_width: last_x_is_formula(&rows) });
    }
    Path::Sections(sections)
}

/// Judges the shape `id` at `at`.
fn judge(reader: &Reader<'_>, doc: &Document, id: u32, at: &ShapeRef, budget: &mut usize) -> Verdict {
    let sheet = SheetRef::Shape(at.clone());
    let Some(shape) = doc.shape(at) else { return Verdict::Skip(format!("shape {id} is not there")) };
    if !reader.is_one_d(&sheet) {
        return Verdict::Skip(format!("shape {id} is not a connector or a line: it has no begin and end to swap"));
    }
    if reader.number(&sheet, "LockBegin") != 0.0 || reader.number(&sheet, "LockEnd") != 0.0 {
        return Verdict::Locked;
    }
    let guarded = PAIRS
        .iter()
        .flat_map(|(a, b)| [*a, *b])
        .any(|name| reader.resolver().resolve(&sheet, &CellAddr::direct(name)).is_some_and(|found| found.formula.cell.is_some_and(Cell::is_guarded)));
    if guarded {
        return Verdict::Skip(format!("shape {id} has a cell of its begin or end that GUARD protects, so its ends cannot be swapped"));
    }
    let mut cells: Vec<(&'static str, Cell)> = Vec::with_capacity(6);
    for (a, b) in PAIRS {
        let (moved_a, moved_b) = (moved_cell(reader, shape, &sheet, a), moved_cell(reader, shape, &sheet, b));
        match (moved_a, moved_b) {
            (Some(x), Some(y)) => {
                if x != y {
                    cells.push((a, y));
                    cells.push((b, x));
                }
            }
            // A trigger on one end only (a connector made here, glued to the whole of a shape at one end) goes to the other end with the end it names, and the end it left
            // holds 0, as a style gives it. A shape that has none is left as it is; an end with no value cannot move.
            (Some(x), None) if a == "BegTrigger" => {
                cells.push((a, Cell::constant(Value::from_text("0"))));
                cells.push((b, x));
            }
            (None, Some(y)) if a == "BegTrigger" => {
                cells.push((a, y));
                cells.push((b, Cell::constant(Value::from_text("0"))));
            }
            _ if a == "BegTrigger" => {}
            _ => return Verdict::Skip(format!("shape {id} has a begin or end cell that holds no value, so its ends cannot be swapped")),
        }
    }
    let size = (reader.number(&sheet, "Width"), reader.number(&sheet, "Height"));
    let label = shape
        .text
        .is_some()
        .then(|| Label { pin: (reader.number(&sheet, "TxtPinX"), reader.number(&sheet, "TxtPinY")), angle: reader.number(&sheet, "TxtAngle") });
    Verdict::Reverse(Box::new(Plan {
        id,
        at: at.clone(),
        sheet: sheet.clone(),
        pose: reader.pose(&sheet),
        size,
        cells,
        path: read_path(reader, at, budget),
        label,
    }))
}

// ------------------------------------------------------------------ the writes

/// An angle brought to the range from just over -pi to pi.
fn wrapped(a: f64) -> f64 {
    let turn = std::f64::consts::TAU;
    let r = a.rem_euclid(turn);
    if r > std::f64::consts::PI { r - turn } else { r }
}

/// The rows a `Geometry` section is given: its `IX`, the rows, and the `IX` of the rows its master gives it.
struct NewSection {
    ix: Option<u32>,
    rows: Vec<Arc<Row>>,
    inherited: Option<Vec<u32>>,
}

/// What the swap writes of one shape after the frame has been worked out again: the rows of each section that its path changes, and the label's cells.
struct Written {
    path: Vec<u32>,
    sections: Vec<NewSection>,
    label: Vec<(SheetRef, CellWrite)>,
    kept: bool,
}

/// The sections of the shape's reversed path, or `None` when a row cannot be carried over.
fn reversed_path(plan: &Plan, sections: &[OldSection], map: Affine) -> Option<Vec<NewSection>> {
    let mut out = Vec::new();
    for section in sections {
        let stored = carry(&section.rows, plan.size, Affine::IDENTITY);
        let carried = carry(&section.rows, plan.size, map);
        if !finite(&stored) || !finite(&carried) {
            return None;
        }
        let want = reverse_rows(&carried)?;
        // A path the rows draw the other way already (a straight line) is left as it is, with its formulas.
        if same_rows(&stored, &want) {
            continue;
        }
        let ends_at_width = match want.last().and_then(end_of) {
            Some(last) => (last.x - plan.size.0).abs() < SAME * plan.size.0.abs().max(1.0),
            None => false,
        };
        out.push(NewSection { ix: section.key.ix, rows: emit(&want, section.follows_width && ends_at_width)?, inherited: section.inherited.clone() });
    }
    Some(out)
}

/// The cells that keep the label where it is on the page and the way it lies there, for the frame the shape has now (`new`) and the one it had (`plan.pose`).
fn label_writes(plan: &Plan, label: &Label, map: Affine, new: &Pose, reader: &Reader<'_>) -> Vec<(SheetRef, CellWrite)> {
    let at = map * Point::new(label.pin.0, label.pin.1);
    let flipped = plan.pose.flip_x != plan.pose.flip_y;
    // The text block lies at the shape's angle with `TxtAngle` added, or taken off under one flip.
    let net = if flipped { plan.pose.angle - label.angle } else { plan.pose.angle + label.angle };
    let turn = if flipped { new.angle - net } else { net - new.angle };
    let mut writes = Vec::new();
    for (name, want) in [("TxtPinX", at.x), ("TxtPinY", at.y), ("TxtAngle", wrapped(turn))] {
        let held = reader.number(&plan.sheet, name);
        let differs = if name == "TxtAngle" { wrapped(held - want).abs() > SAME } else { (held - want).abs() > SAME };
        if want.is_finite() && differs {
            writes.push((
                plan.sheet.clone(),
                CellWrite { addr: CellAddr::direct(name), edit: CellEdit::Value(Value::from_text(&number_text(want))), origin: WriteOrigin::Drawing },
            ));
        }
    }
    writes
}

/// Reverses the begin and the end of the one-dimensional shapes `ids` of page `page`. Each shape that has a begin and an end, whose `LockBegin` and
/// `LockEnd` are off and no cell of whose ends `GUARD` protects, is reversed; the others are left as they are and said in the answer. All or nothing as an edit: an error leaves
/// the document as it was. A page or a shape that is not there is an error, in words.
pub fn reverse_ends(doc: &mut Document, page: u32, ids: &[u32]) -> Result<Reversal, Problem> {
    require_page(doc, page)?;
    let before = doc.clone();
    let (_, named) = scan::named(&before, page, ids)?;
    let mut out = Reversal::default();
    let mut plans: Vec<Plan> = Vec::new();
    {
        let reader = Reader::new(&before);
        let mut budget = MAX_REVERSE_ROWS;
        for (id, at) in &named {
            match judge(&reader, &before, *id, at, &mut budget) {
                Verdict::Reverse(plan) => plans.push(*plan),
                Verdict::Locked => out.locked.push(*id),
                Verdict::Skip(why) => out.skipped.push(NotReversed { id: *id, why }),
            }
        }
    }
    if plans.is_empty() {
        return Ok(out);
    }
    let reversed: HashSet<u32> = plans.iter().map(|p| p.id).collect();
    // The ends, with their formulas and values, swap in one batch of cell writes.
    let writes: Vec<(SheetRef, CellWrite)> = plans
        .iter()
        .flat_map(|plan| {
            plan.cells.iter().map(|(name, cell)| {
                (plan.sheet.clone(), CellWrite { addr: CellAddr::direct(name), edit: CellEdit::Cell(cell.clone()), origin: WriteOrigin::Drawing })
            })
        })
        .collect();
    let changed: Vec<(SheetRef, CellAddr)> = writes.iter().map(|(sheet, write)| (sheet.clone(), write.addr.clone())).collect();
    if !writes.is_empty() {
        let outcomes = doc.write_batch(writes)?;
        if outcomes.contains(&WriteOutcome::KeptGuarded) {
            return Err(Problem::failed("a cell of an end is protected by GUARD and cannot be swapped"));
        }
    }
    swap_connects(doc, page, &reversed)?;
    // What reads the ends is worked out again, and nothing else is: no end is placed by the engine's rule and no route is made again.
    settle_kept(doc, page, &Changes { cells: changed, ..Changes::default() })?;
    // The frame is what the recalculation made it: the path and the label are carried to it.
    let written: Vec<Written> = {
        let reader = Reader::new(doc);
        plans
            .iter()
            .map(|plan| {
                let new = reader.pose(&plan.sheet);
                let map = transfer(&plan.pose, &new);
                let (sections, kept) = match &plan.path {
                    Path::Sections(sections) => match reversed_path(plan, sections, map) {
                        Some(done) => (done, false),
                        None => (Vec::new(), true),
                    },
                    Path::Kept => (Vec::new(), true),
                };
                let label =
                    if kept { Vec::new() } else { plan.label.as_ref().map(|l| label_writes(plan, l, map, &new, &reader)).unwrap_or_default() };
                Written { path: plan.at.path.clone(), sections, label, kept }
            })
            .collect()
    };
    let mut label_cells: Vec<(SheetRef, CellAddr)> = Vec::new();
    let mut label_batch: Vec<(SheetRef, CellWrite)> = Vec::new();
    // The sections of the paths that were written, whole: the cells that read them (a shape tied to the path by `POINTALONGPATH`) are worked out after the write, and not by the
    // recalculation of the swap, which read the path as it was (the same work a leg moved by hand and the jumps are given).
    let mut rewritten: Vec<(SheetRef, SectionKey)> = Vec::new();
    for (plan, w) in plans.iter().zip(written) {
        rewritten.extend(w.sections.iter().map(|section| (plan.sheet.clone(), SectionKey::new("Geometry", section.ix))));
        for section in w.sections {
            write_section(doc, page, &w.path, section.ix, section.rows, &section.inherited)?;
        }
        label_cells.extend(w.label.iter().map(|(sheet, write)| (sheet.clone(), write.addr.clone())));
        label_batch.extend(w.label);
        out.reversed.push(plan.id);
        if w.kept {
            out.geometry_kept.push(plan.id);
        }
    }
    if !label_batch.is_empty() {
        // A label cell a `GUARD` pins stays where it is, and the edit goes on (`label::write`).
        doc.write_batch(label_batch)?;
    }
    if !rewritten.is_empty() || !label_cells.is_empty() {
        // The cells that read the paths written and the label cells written, and what reads those, in one commit that is none when nothing reads them.
        recalculate_sections(doc, page, rewritten, label_cells.clone())?;
        super::fields::refresh_changed(doc, page, &label_cells);
    }
    follow_connectors_glued_to_them(doc, &before, page, &reversed, &plans)?;
    Ok(out)
}

/// The `Connect` rows of the shapes in `ids` after a swap: `FromCell` and `FromPart` name the other end, and everything else is kept.
fn swap_connects(doc: &mut Document, page: u32, ids: &HashSet<u32>) -> Result<(), Problem> {
    let p = page_mut(doc, page).ok_or_else(|| Problem::failed("the page went away"))?;
    if !p.connects.iter().any(|c| ids.contains(&c.from_sheet)) {
        return Ok(());
    }
    let list: Vec<Connect> = p.connects.iter().map(|c| if ids.contains(&c.from_sheet) { swapped_connect(c) } else { c.clone() }).collect();
    p.connects = Connects::from_vec(list);
    Ok(())
}

fn swapped_connect(c: &Connect) -> Connect {
    let mut out = c.clone();
    if let Some(name) = c.from_cell.as_deref() {
        let other = [("BeginX", "EndX"), ("BeginY", "EndY")].iter().find_map(|(a, b)| {
            if name.eq_ignore_ascii_case(a) {
                Some(*b)
            } else if name.eq_ignore_ascii_case(b) {
                Some(*a)
            } else {
                None
            }
        });
        if let Some(other) = other {
            out.from_cell = Some(Arc::from(other));
        }
    }
    out.from_part = c.from_part.map(swapped_part);
    out
}

/// The `FromPart` of a `Connect` row after a swap: 7 and 8 are the begin's x and y, 10 and 11 the end's, 9 and 12 the begin and the end; any other part is as it was. A swap
/// of a swap is the part it began with.
fn swapped_part(part: u32) -> u32 {
    match part {
        7 => 10,
        10 => 7,
        8 => 11,
        11 => 8,
        9 => 12,
        12 => 9,
        other => other,
    }
}

/// A connector glued to a connection point of a shape that was reversed follows the point where the swap put it (the recalculation has moved its end): it is routed again
/// where it moved, as a connector is when the shape it is glued to moves. Only the connectors glued to a reversed shape are looked at, in one pass over the page's rows.
fn follow_connectors_glued_to_them(doc: &mut Document, before: &Document, page: u32, reversed: &HashSet<u32>, plans: &[Plan]) -> Result<(), Problem> {
    let followers: BTreeSet<u32> = {
        let Some(p) = doc.page(page) else { return Ok(()) };
        p.connects.iter().filter(|c| reversed.contains(&c.to_sheet) && c.to_sheet != c.from_sheet).map(|c| c.from_sheet).collect()
    };
    if followers.is_empty() {
        return Ok(());
    }
    // A row that names a shape the page does not hold (a file's) is left out by the walk, which finds the others in one pass for the drawing now and the drawing before.
    let wanted: HashSet<u32> = followers.iter().copied().collect();
    let (now, was) = (scan::find_named(doc, page, &wanted)?, scan::find_named(before, page, &wanted)?);
    let moved: Vec<u32> = {
        let (reader, old) = (Reader::new(doc), Reader::new(before));
        let own: HashSet<u32> = plans.iter().map(|p| p.id).collect();
        followers
            .iter()
            .copied()
            .filter(|id| !own.contains(id))
            .filter(|id| {
                let (Some(at), Some(then)) = (now.get(id), was.get(id)) else { return false };
                let (a, b) = (SheetRef::Shape(at.clone()), SheetRef::Shape(then.clone()));
                ["BeginX", "BeginY", "EndX", "EndY"].iter().any(|name| (reader.number(&a, name) - old.number(&b, name)).abs() > 1.0e-7)
            })
            .collect()
    };
    if moved.is_empty() {
        return Ok(());
    }
    route_after(doc, before, page, &[], &moved, &[])
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use diacraft_geom::NurbsCurve;

    use super::*;

    fn pt(x: f64, y: f64) -> Point {
        Point::new(x, y)
    }

    fn line(x: f64, y: f64) -> CRow {
        CRow::Line(pt(x, y))
    }

    #[test]
    fn a_polyline_walked_back_starts_where_it_ended_and_a_second_reversal_gives_it_back() {
        let path = vec![CRow::Move(pt(0.0, 0.0)), line(2.0, 0.0), line(2.0, 1.0), line(4.0, 1.0)];
        let back = reverse_rows(&path).unwrap();
        assert_eq!(back, vec![CRow::Move(pt(4.0, 1.0)), line(2.0, 1.0), line(2.0, 0.0), line(0.0, 0.0)]);
        assert_eq!(reverse_rows(&back).unwrap(), path);
    }

    #[test]
    fn an_arc_keeps_the_side_it_bows_to_so_its_bow_changes_sign_and_an_elliptical_arc_keeps_its_middle() {
        let arc = vec![CRow::Move(pt(0.0, 0.0)), CRow::Arc { to: pt(2.0, 0.0), bow: 0.25 }];
        assert_eq!(reverse_rows(&arc).unwrap(), vec![CRow::Move(pt(2.0, 0.0)), CRow::Arc { to: pt(0.0, 0.0), bow: -0.25 }]);
        let ellipse = vec![CRow::Move(pt(0.0, 0.0)), CRow::EllipticalArc { to: pt(2.0, 0.0), mid: pt(1.0, 0.5), angle: 0.3, ratio: 2.0 }];
        assert_eq!(
            reverse_rows(&ellipse).unwrap(),
            vec![CRow::Move(pt(2.0, 0.0)), CRow::EllipticalArc { to: pt(0.0, 0.0), mid: pt(1.0, 0.5), angle: 0.3, ratio: 2.0 }]
        );
        let poly = vec![CRow::Move(pt(0.0, 0.0)), CRow::Polyline { to: pt(3.0, 0.0), points: vec![pt(1.0, 0.5), pt(2.0, -0.5)] }];
        assert_eq!(
            reverse_rows(&poly).unwrap(),
            vec![CRow::Move(pt(3.0, 0.0)), CRow::Polyline { to: pt(0.0, 0.0), points: vec![pt(2.0, -0.5), pt(1.0, 0.5)] }]
        );
    }

    #[test]
    fn a_path_in_two_runs_goes_back_run_by_run_with_the_gap_between_them() {
        let gap = vec![CRow::Move(pt(0.0, 0.0)), line(1.0, 0.0), CRow::Move(pt(2.0, 0.0)), line(3.0, 0.0)];
        assert_eq!(reverse_rows(&gap).unwrap(), vec![CRow::Move(pt(3.0, 0.0)), line(2.0, 0.0), CRow::Move(pt(1.0, 0.0)), line(0.0, 0.0)]);
        assert_eq!(reverse_rows(&[]).unwrap(), Vec::new());
        assert_eq!(reverse_rows(&[CRow::Move(pt(1.0, 1.0))]).unwrap(), vec![CRow::Move(pt(1.0, 1.0))]);
    }

    #[test]
    fn a_path_that_starts_with_no_move_or_holds_a_row_that_is_not_carried_over_is_not_reversed() {
        assert_eq!(reverse_rows(&[line(1.0, 0.0)]), None);
        for row in [
            CRow::Cubic { c1: pt(0.0, 1.0), c2: pt(1.0, 1.0), to: pt(1.0, 0.0) },
            CRow::Quad { c: pt(0.5, 1.0), to: pt(1.0, 0.0) },
            CRow::Ellipse { centre: pt(0.0, 0.0), first: pt(1.0, 0.0), second: pt(0.0, 1.0) },
            CRow::SplineStart { to: pt(1.0, 1.0), second_knot: 0.0, first_knot: 0.0, last_knot: 1.0, degree: 3.0 },
            CRow::SplineKnot { to: pt(1.0, 1.0), knot: 1.0 },
        ] {
            assert_eq!(reverse_rows(&[CRow::Move(pt(0.0, 0.0)), row.clone()]), None, "{row:?}");
        }
    }

    /// The curve a `NURBSTo` row after the pen `pen` draws, as the geometry crate reads it.
    fn curve_of(pen: Point, row: &CRow) -> NurbsCurve {
        let CRow::Nurbs { to, knots, last_knot, degree, points } = row else { panic!("not a spline: {row:?}") };
        let mut control = vec![pen];
        let mut weights = vec![knots.first_weight];
        let mut listed = vec![knots.first_knot];
        for (p, knot, weight) in points {
            control.push(*p);
            weights.push(*weight);
            listed.push(*knot);
        }
        control.push(*to);
        weights.push(knots.last_weight);
        listed.push(knots.second_last_knot);
        listed.push(*last_knot);
        NurbsCurve::new(*degree, control, weights, &listed).unwrap()
    }

    fn spline(degree: f64, via: &[((f64, f64), f64, f64)], knots: NurbsKnots, last_knot: f64, to: (f64, f64)) -> CRow {
        CRow::Nurbs { to: pt(to.0, to.1), knots, last_knot, degree, points: via.iter().map(|((x, y), k, w)| (pt(*x, *y), *k, *w)).collect() }
    }

    /// The same curve walked the other way: the reversed spline at the parameter that mirrors `t` is the point of the spline at `t`.
    fn assert_mirrored(pen: Point, row: &CRow, back: &CRow) {
        let (old, new) = (curve_of(pen, row), curve_of(end_of(row).unwrap(), back));
        let ((lo, hi), (lo2, hi2)) = (old.domain(), new.domain());
        assert!((lo - lo2).abs() < 1e-9 && (hi - hi2).abs() < 1e-9, "the span is the same: {:?} {:?}", old.domain(), new.domain());
        for step in 0..=20 {
            let t = lo + (hi - lo) * f64::from(step) / 20.0;
            let (a, b) = (old.eval(t), new.eval(lo + hi - t));
            assert!((a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9, "t {t}: {a:?} {b:?}");
        }
    }

    #[test]
    fn a_clamped_spline_walked_back_draws_the_same_curve_whatever_its_degree_and_weights() {
        let pen = pt(0.0, 0.0);
        // The quadratic spline of a curved connector: four control points, knots 0, 0, 0, 1, 2 and two more 2.
        let quadratic = spline(
            2.0,
            &[((2.0, 0.0), 0.0, 1.0), ((2.0, 2.0), 0.0, 1.0)],
            NurbsKnots { second_last_knot: 1.0, last_weight: 1.0, first_knot: 0.0, first_weight: 1.0 },
            2.0,
            (4.0, 2.0),
        );
        // A cubic of five control points with weights that differ and a knot inside.
        let cubic = spline(
            3.0,
            &[((1.0, 2.0), 0.0, 2.0), ((3.0, -1.0), 0.0, 0.5), ((4.0, 2.0), 0.0, 1.5)],
            NurbsKnots { second_last_knot: 1.0, last_weight: 3.0, first_knot: 0.0, first_weight: 1.0 },
            2.0,
            (6.0, 0.0),
        );
        for row in [quadratic, cubic] {
            let reversed = reverse_rows(&[CRow::Move(pen), row.clone()]).unwrap();
            assert_eq!(reversed.len(), 2);
            assert_eq!(reversed[0], CRow::Move(end_of(&row).unwrap()));
            assert_mirrored(pen, &row, &reversed[1]);
            // And back again.
            let twice = reverse_rows(&reversed).unwrap();
            assert_eq!(twice.len(), 2);
            assert_mirrored(end_of(&row).unwrap(), &reversed[1], &twice[1]);
        }
    }

    #[test]
    fn a_spline_not_clamped_at_its_start_or_with_a_bad_degree_is_not_reversed() {
        let knots = NurbsKnots { second_last_knot: 2.0, last_weight: 1.0, first_knot: 0.0, first_weight: 1.0 };
        // Knots 0, 1, 2, 3, 3, 3: open at the start, so its mirror is not a row a file holds.
        let open = spline(2.0, &[((1.0, 1.0), 1.0, 1.0), ((2.0, 0.0), 2.0, 1.0)], knots, 3.0, (3.0, 1.0));
        assert_eq!(reverse_rows(&[CRow::Move(pt(0.0, 0.0)), open]), None);
        let ok_knots = NurbsKnots { second_last_knot: 0.0, last_weight: 1.0, first_knot: 0.0, first_weight: 1.0 };
        for degree in [0.0, 26.0, f64::NAN, 2.5, -1.0] {
            let bad = spline(degree, &[((1.0, 1.0), 0.0, 1.0)], ok_knots, 1.0, (2.0, 0.0));
            assert_eq!(reverse_rows(&[CRow::Move(pt(0.0, 0.0)), bad]), None, "{degree}");
        }
        // Too few control points for the degree.
        let few = spline(3.0, &[((1.0, 1.0), 0.0, 1.0)], ok_knots, 1.0, (2.0, 0.0));
        assert_eq!(reverse_rows(&[CRow::Move(pt(0.0, 0.0)), few]), None);
    }

    #[test]
    fn rows_that_are_the_same_to_a_billionth_are_one_path_and_others_are_not() {
        let a = vec![CRow::Move(pt(0.0, 0.0)), line(4.0, 0.0)];
        assert!(same_rows(&a, &[CRow::Move(pt(1e-12, 0.0)), line(4.0 - 1e-12, 0.0)]));
        assert!(!same_rows(&a, &[CRow::Move(pt(0.0, 0.0)), line(4.0, 1e-6)]));
        assert!(!same_rows(&a, &a[..1]));
        assert!(!same_rows(&[CRow::Move(pt(0.0, 0.0))], &[line(0.0, 0.0)]));
        // A straight line is the same line walked back in the frame that turned by a half turn: nothing to write.
        let (old, new) = (
            Pose { pin: (3.0, 1.0), loc: (2.0, 0.0), angle: 0.0, flip_x: false, flip_y: false },
            Pose { pin: (3.0, 1.0), loc: (2.0, 0.0), angle: std::f64::consts::PI, flip_x: false, flip_y: false },
        );
        let stored = vec![CRow::Move(pt(0.0, 0.0)), line(4.0, 0.0)];
        let carried: Vec<CRow> = stored
            .iter()
            .map(|row| match row {
                CRow::Move(p) => CRow::Move(transfer(&old, &new) * *p),
                CRow::Line(p) => CRow::Line(transfer(&old, &new) * *p),
                other => other.clone(),
            })
            .collect();
        assert!(same_rows(&stored, &reverse_rows(&carried).unwrap()));
    }

    #[test]
    fn the_map_between_two_frames_is_a_half_turn_about_the_pin_when_the_angle_turns_by_a_half() {
        let (old, new) = (
            Pose { pin: (3.0, 1.0), loc: (2.0, 0.0), angle: 0.0, flip_x: false, flip_y: false },
            Pose { pin: (3.0, 1.0), loc: (2.0, 0.0), angle: std::f64::consts::PI, flip_x: false, flip_y: false },
        );
        let map = transfer(&old, &new);
        let at = |x: f64, y: f64| map * pt(x, y);
        for (x, y, want) in [(0.0, 0.0, (4.0, 0.0)), (4.0, 0.0, (0.0, 0.0)), (1.0, 0.5, (3.0, -0.5)), (2.0, 0.0, (2.0, 0.0))] {
            let got = at(x, y);
            assert!((got.x - want.0).abs() < 1e-9 && (got.y - want.1).abs() < 1e-9, "({x}, {y}): {got:?}");
        }
        // The same pose is the identity, whatever it is.
        let still = transfer(&new, &new);
        let got = still * pt(1.5, 0.25);
        assert!((got.x - 1.5).abs() < 1e-9 && (got.y - 0.25).abs() < 1e-9);
    }

    #[test]
    fn the_references_of_a_swapped_formula_to_its_own_ends_name_the_other_end() {
        assert_eq!(swapped_refs("BeginX").as_deref(), Some("EndX"));
        assert_eq!(swapped_refs("SQRT((EndX-BeginX)^2+(EndY-BeginY)^2)").as_deref(), Some("SQRT((BeginX-EndX)^2+(BeginY-EndY)^2)"));
        // A swap turns the arguments, so the begin's formula names `BegTrigger` first again.
        assert_eq!(
            swapped_refs("_WALKGLUE(EndTrigger,BegTrigger,WalkPreference)").as_deref(),
            Some("_WALKGLUE(BegTrigger,EndTrigger,WalkPreference)")
        );
        // Any letter case, and the text of the formula is otherwise kept.
        assert_eq!(swapped_refs("1 + beginy * 2").as_deref(), Some("1 + EndY * 2"));
        // Another shape's cell, a string, a row's cell and a formula with none are left as they are.
        assert_eq!(swapped_refs("PAR(PNT(Sheet.4!Connections.X1,Sheet.4!Connections.Y1))"), None);
        assert_eq!(swapped_refs("Sheet.4!BeginX"), None);
        assert_eq!(swapped_refs("\"BeginX\"&BeginY").as_deref(), Some("\"BeginX\"&EndY"));
        assert_eq!(swapped_refs("Geometry1.X2"), None);
        assert_eq!(swapped_refs("Width*0.5"), None);
        assert_eq!(swapped_refs("(("), None, "a formula that does not parse is moved as it is");
        assert_eq!(swapped_refs(""), None);
    }

    #[test]
    fn a_connect_row_swaps_its_from_cell_and_from_part_and_keeps_the_rest() {
        let row = |cell: &str, part: u32| Connect {
            from_sheet: 7,
            from_cell: Some(Arc::from(cell)),
            from_part: Some(part),
            to_sheet: 5,
            to_cell: Some(Arc::from("Connections.X1")),
            to_part: Some(100),
            ..Connect::default()
        };
        for (cell, part, want_cell, want_part) in [
            ("BeginX", 9, "EndX", 12),
            ("EndX", 12, "BeginX", 9),
            ("BeginY", 8, "EndY", 11),
            ("EndY", 11, "BeginY", 8),
            ("BeginX", 7, "EndX", 10),
            ("EndX", 10, "BeginX", 7),
        ] {
            let swapped = swapped_connect(&row(cell, part));
            assert_eq!((swapped.from_cell.as_deref(), swapped.from_part), (Some(want_cell), Some(want_part)), "{cell} {part}");
            assert_eq!((swapped.from_sheet, swapped.to_sheet, swapped.to_part), (7, 5, Some(100)));
            assert_eq!(swapped_connect(&swapped), row(cell, part), "twice is once");
        }
        // A row of another kind keeps its cell and its part.
        let guide = row("AlignBottom", 4);
        assert_eq!(swapped_connect(&guide), guide);
    }

    #[test]
    fn a_part_swapped_twice_is_the_part_it_was_and_only_the_six_of_the_ends_change() {
        for part in 0..200_u32 {
            assert_eq!(swapped_part(swapped_part(part)), part, "{part}");
            assert_eq!(swapped_part(part) != part, matches!(part, 7..=12), "{part}");
        }
        assert_eq!([7, 8, 9, 10, 11, 12].map(swapped_part), [10, 11, 12, 7, 8, 9]);
    }

    #[test]
    fn the_words_name_the_shapes_and_cap_the_list() {
        assert_eq!(listed(&[3]), "3");
        assert_eq!(listed(&[3, 4, 9]), "3, 4, 9");
        assert_eq!(listed(&[1, 2, 3, 4, 5, 6, 7]), "1, 2, 3, 4, 5 and 2 more");
        let none = Reversal::default();
        assert!(none.refusal(2).contains("none of the 2 shapes named"));
        let one = Reversal {
            skipped: vec![NotReversed { id: 5, why: "shape 5 is not a connector or a line: it has no begin and end to swap".into() }],
            ..Reversal::default()
        };
        assert_eq!(one.refusal(1), "shape 5 is not a connector or a line: it has no begin and end to swap");
        let locked = Reversal { locked: vec![8], ..Reversal::default() };
        assert!(locked.refusal(1).contains("shape 8") && locked.refusal(1).contains("LockBegin or LockEnd"));
        let many = Reversal { locked: vec![1, 2, 3, 4, 5], ..Reversal::default() };
        assert!(many.refusal(5).contains("and 2 more"), "{}", many.refusal(5));
        let done =
            Reversal { reversed: vec![3, 4], locked: vec![8], skipped: vec![NotReversed { id: 9, why: String::new() }], geometry_kept: vec![4] };
        let says = done.says();
        assert!(says.contains("Reversed 2 shapes") && says.contains("shape 4 could not be reversed") && says.contains("shapes 8, 9"), "{says}");
        assert!(Reversal { reversed: vec![3], ..Reversal::default() }.says().starts_with("Reversed shape 3"));
    }
}
