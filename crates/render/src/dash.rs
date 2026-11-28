//! Line patterns, caps and joins.
//!
//! **A dash is a multiple of the line's weight.** The pattern's marks and gaps are whole numbers of weights, so a 3-point line
//! dashes three times as long as a 1-point one, on paper, whatever the drawing's scale. A renderer that keeps the lengths in points draws
//! every weight's dashes alike.
//!
//! **A line thinner than its target can show** is drawn at the narrowest width the target has, and its dashes stay in units of its own
//! weight ([`Widths`]).
//!
//! **A compound line** (`CompoundType` 1 to 4: two or three bands sharing the weight) is drawn as a stroke for each band, along the path offset to the band's side of it
//! ([`bands`], [`offset_path`]).

use kurbo::{BezPath, Cap, CubicBez, Join, PathEl, Point, QuadBez, Stroke, Vec2};

use crate::limits::{MAX_DASHES, MIN_DASH_PERIOD_PX};

/// The mitre limit: the length of a mitre over the weight, past which a corner is bevelled. Files give 3 on every stroke.
pub(crate) const MITRE_LIMIT: f64 = 3.0;

/// Marks and gaps of `LinePattern` 2 to 23, in weights, alternating mark, gap. A mark of 0 is a dot.
pub(crate) fn pattern_list(number: i64) -> Option<&'static [f64]> {
    Some(match number {
        2 => &[7.0, 5.0],
        3 => &[0.0, 5.0],
        4 => &[7.0, 5.0, 0.0, 5.0],
        5 => &[7.0, 5.0, 0.0, 5.0, 0.0, 5.0],
        6 => &[7.0, 5.0, 7.0, 5.0, 0.0, 5.0],
        7 => &[19.0, 5.0, 7.0, 5.0],
        8 => &[19.0, 5.0, 7.0, 5.0, 7.0, 5.0],
        9 => &[3.0, 3.0],
        10 => &[0.0, 3.0],
        11 => &[3.0, 3.0, 0.0, 3.0],
        12 => &[3.0, 3.0, 0.0, 3.0, 0.0, 3.0],
        13 => &[3.0, 3.0, 3.0, 3.0, 0.0, 3.0],
        14 => &[9.0, 3.0, 3.0, 3.0],
        15 => &[9.0, 3.0, 3.0, 3.0, 3.0, 3.0],
        16 => &[15.0, 9.0],
        17 => &[0.0, 9.0],
        18 => &[15.0, 9.0, 0.0, 9.0],
        19 => &[15.0, 9.0, 0.0, 9.0, 0.0, 9.0],
        20 => &[15.0, 9.0, 15.0, 9.0, 0.0, 9.0],
        21 => &[39.0, 9.0, 15.0, 9.0],
        22 => &[39.0, 9.0, 15.0, 9.0, 15.0, 9.0],
        23 => &[1.0, 2.0],
        _ => return None,
    })
}

/// The cap and join of a `LineCap`: 0 is a round cap with a round join; 1 is a flat cap and 2 a projecting square one,
/// both with a mitre join. (The names the sources give 1 and 2, square and extended, are the other way round.)
pub(crate) fn cap_and_join(line_cap: u8) -> (Cap, Join) {
    match line_cap {
        1 => (Cap::Butt, Join::Miter),
        2 => (Cap::Square, Join::Miter),
        _ => (Cap::Round, Join::Round),
    }
}

/// How wide a line is on paper, in each sense a drawing needs.
///
/// **A line is drawn at least as wide as its target can show.** A raster target cannot draw a stroke narrower than a pixel at its weight:
/// the stroke comes out as a faint grey a third as dark as the line's colour. So it draws one a pixel wide, in the line's full colour, and
/// the weight stays what it was for everything else: the size of a line end and the unit of the dashes are the line's
/// own weight, not the width it is drawn at. A writer of SVG or PDF draws the weight as it is and leaves the pixels to the reader.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Widths {
    /// The weight the cell holds, in paper inches, or 0 for a weight of 0 (the unit `S` of a line end takes weight 0).
    pub weight: f64,
    /// The unit of the dash lists, in paper inches: the weight, or for a weight of 0 a quarter point (dash lists take 0.25 points).
    pub unit: f64,
    /// The width the stroke is drawn at, in paper inches.
    pub drawn: f64,
}

/// A quarter point, in paper inches: what a weight of 0 stands for in a vector export, and in the dash lists of a raster.
const ZERO_WEIGHT: f64 = 0.25 / 72.0;

impl Widths {
    /// The widths of a line of `weight` paper inches (not negative) on a target whose narrowest stroke is `hairline` paper inches (0 for none).
    ///
    /// A weight of 0 draws one device pixel on a target that has one, at any zoom, and a quarter point on one that has not.
    pub(crate) fn of(weight: f64, hairline: f64) -> Widths {
        let weight = if weight.is_finite() && weight > 0.0 { weight } else { 0.0 };
        let hairline = if hairline.is_finite() && hairline > 0.0 { hairline } else { 0.0 };
        if weight > 0.0 {
            Widths { weight, unit: weight, drawn: weight.max(hairline) }
        } else {
            Widths { weight: 0.0, unit: ZERO_WEIGHT, drawn: if hairline > 0.0 { hairline } else { ZERO_WEIGHT } }
        }
    }
}

/// How a line is stroked.
pub(crate) struct StrokePlan {
    pub stroke: Stroke,
    /// Whether the pattern was dropped because it would make too many dashes.
    pub solid_for_count: bool,
    /// How many dashes the stroke makes, rounded up: 0 for a solid line (the work of the stroke).
    pub dashes: u64,
}

/// The stroke of a line of `widths` with `pattern`, for a path `length` long (paper inches) drawn at `px_per_inch`.
///
/// The table's lists are for a round or projecting cap, which adds half a weight past each end of a mark; under a flat cap each mark is a
/// weight longer and each gap a weight shorter, so that the mark as seen is the same length under all three caps.
///
/// **A line drawn wider than its unit keeps the marks and gaps the pattern gives it, as seen**. The lists are in units of
/// the line's own weight, not of the width it is drawn at: the same marks fall at the same places on paper in the window, in a PNG, and in an
/// SVG or a PDF, and widening the ink changes how thick the line is and not where its marks are. A cap adds half the drawn width at each end
/// of a mark, so each mark is cut by the width the cap adds beyond the one the list counted, and the gap that follows is lengthened by the
/// same, which keeps the period, and the mark as seen, what the pattern says.
pub(crate) fn plan(widths: Widths, pattern: i64, line_cap: u8, length: f64, px_per_inch: f64) -> StrokePlan {
    let (cap, join) = cap_and_join(line_cap);
    let mut stroke = Stroke::new(widths.drawn).with_caps(cap).with_join(join).with_miter_limit(MITRE_LIMIT);
    let Some(list) = pattern_list(pattern) else {
        return StrokePlan { stroke, solid_for_count: false, dashes: 0 };
    };
    let unit = widths.unit;
    if !unit.is_finite() || unit <= 0.0 {
        return StrokePlan { stroke, solid_for_count: false, dashes: 0 };
    }
    let flat = cap == Cap::Butt;
    // What a round or projecting cap adds to a mark beyond the weight the list counted on: nothing when the line is drawn at its weight.
    let extra = if flat { 0.0 } else { (widths.drawn - unit).max(0.0) };
    // A zero-length mark is a dot; a mark of a thousandth of a weight draws one under a round cap, and under a flat cap the table's own
    // longer form (one weight) is a square dot.
    let floor = unit * 1.0e-3;
    let mut carry = 0.0;
    let dashes: Vec<f64> = list
        .iter()
        .enumerate()
        .map(|(i, &n)| {
            let len = if i % 2 == 0 {
                if flat {
                    (n + 1.0) * unit
                } else {
                    let mark = (n * unit - extra).max(floor);
                    carry = (n * unit - mark).max(0.0);
                    mark
                }
            } else if flat {
                (n - 1.0).max(0.0) * unit
            } else {
                n * unit + std::mem::take(&mut carry)
            };
            len.max(floor)
        })
        .collect();
    let period: f64 = dashes.iter().sum();
    if !(period.is_finite() && period > 0.0) {
        return StrokePlan { stroke, solid_for_count: true, dashes: 0 };
    }
    let count = length / period * dashes.len() as f64;
    if period * px_per_inch < MIN_DASH_PERIOD_PX || !count.is_finite() || count > MAX_DASHES {
        return StrokePlan { stroke, solid_for_count: true, dashes: 0 };
    }
    stroke = stroke.with_dashes(0.0, dashes);
    // At most `MAX_DASHES` and not negative (a length is not), so the cast keeps it.
    StrokePlan { stroke, solid_for_count: false, dashes: count.max(0.0).ceil() as u64 }
}

// ---- compound lines --------------------------------------------------------------------------------------------------

/// One band of a compound line: where its middle lies across the path and how wide it is, in paper inches.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Band {
    /// The distance of the band's middle from the path, positive on the **left** of the direction of travel (above a line that runs to the right, in a drawing whose y runs up).
    pub offset: f64,
    pub width: f64,
}

/// The layout of a compound type across the weight, from the left edge of the line to the right: whether each part is a band (and not a gap) and its share of the weight. Type 1 is
/// a third each, a gap between two bands; 2 is a thick band (0.6), a gap and a thin one (0.2 each), the thick one first, and 3 its mirror; 4 is thin, thick, thin (a sixth, a
/// sixth, a third, a sixth and a sixth). Any other type, and 0, is one line: no layout.
fn layout(compound: u8) -> &'static [(bool, f64)] {
    const THIRD: f64 = 1.0 / 3.0;
    const SIXTH: f64 = 1.0 / 6.0;
    match compound {
        1 => &[(true, THIRD), (false, THIRD), (true, THIRD)],
        2 => &[(true, 0.6), (false, 0.2), (true, 0.2)],
        3 => &[(true, 0.2), (false, 0.2), (true, 0.6)],
        4 => &[(true, SIXTH), (false, SIXTH), (true, THIRD), (false, SIXTH), (true, SIXTH)],
        _ => &[],
    }
}

/// The bands of a compound line `total` paper inches wide, from the left of the direction of travel: none for a single line (type 0, or one that is none of the five), whose stroke is drawn
/// as it is. The bands and the gaps between them add up to `total`, so the line is as wide as its weight.
pub(crate) fn bands(compound: u8, total: f64) -> Vec<Band> {
    if !(total.is_finite() && total > 0.0) {
        return Vec::new();
    }
    let mut from_left = 0.0;
    let mut out = Vec::new();
    for (is_band, share) in layout(compound) {
        let width = share * total;
        if *is_band {
            out.push(Band { offset: total / 2.0 - (from_left + width / 2.0), width });
        }
        from_left += width;
    }
    out
}

/// The work of offsetting a segment that is a line, a move or a close, in the units of the frame's budget.
const WORK_PER_LINE_OFFSET: u64 = 1;
/// The work of offsetting a curve: the offset may cut it into a few hundred pieces (`kurbo::offset` stops at a depth of 8), so it is charged more than a line is.
const WORK_PER_CURVE_OFFSET: u64 = 32;

/// The work of offsetting `path` once for each of `bands` bands, charged before it is done.
pub(crate) fn offset_work(path: &BezPath, bands: usize) -> u64 {
    let once = path
        .elements()
        .iter()
        .map(|el| match el {
            PathEl::QuadTo(..) | PathEl::CurveTo(..) => WORK_PER_CURVE_OFFSET,
            PathEl::MoveTo(_) | PathEl::LineTo(_) | PathEl::ClosePath => WORK_PER_LINE_OFFSET,
        })
        .fold(0_u64, u64::saturating_add);
    once.saturating_mul(u64::try_from(bands).unwrap_or(u64::MAX))
}

/// A piece of an offset path: a line between two points, or a curve with the elements that draw it.
enum Piece {
    Line { a: Point, b: Point },
    Curve { start: Point, end: Point, els: Vec<PathEl> },
}

impl Piece {
    fn start(&self) -> Point {
        match self {
            Piece::Line { a, .. } => *a,
            Piece::Curve { start, .. } => *start,
        }
    }

    fn end(&self) -> Point {
        match self {
            Piece::Line { b, .. } => *b,
            Piece::Curve { end, .. } => *end,
        }
    }
}

/// A length below which a segment has no direction to offset by (paper inches).
const NO_LENGTH: f64 = 1.0e-9;

/// The left of `direction` (a vector of any length but 0), `d` long.
fn left_of(direction: Vec2, d: f64) -> Vec2 {
    let length = direction.hypot();
    if length > 0.0 { direction.turn_90() * (d / length) } else { Vec2::ZERO }
}

/// The offset of the segment from `p0` to `p1`, `d` to its left: `None` for one with no length.
fn offset_line(p0: Point, p1: Point, d: f64) -> Option<Piece> {
    let direction = p1 - p0;
    (direction.hypot() > NO_LENGTH).then(|| {
        let shift = left_of(direction, d);
        Piece::Line { a: p0 + shift, b: p1 + shift }
    })
}

/// The offset of a cubic, `d` to its left, to within `tolerance`. A cubic whose control points lie on its chord is a line, and one that is a point has no offset.
fn offset_curve(c: CubicBez, d: f64, tolerance: f64) -> Option<Piece> {
    let chord = c.p3 - c.p0;
    let reach = chord.hypot();
    let off_chord = |p: Point| if reach > NO_LENGTH { (chord.cross(p - c.p0) / reach).abs() } else { (p - c.p0).hypot() };
    if off_chord(c.p1) <= NO_LENGTH && off_chord(c.p2) <= NO_LENGTH {
        return offset_line(c.p0, c.p3, d);
    }
    // A tangent at an end that is a point of the curve held in the control points: the first that is away from the end.
    let tangent = |from: Point, towards: [Point; 3]| towards.into_iter().map(|p| p - from).find(|v| v.hypot() > NO_LENGTH);
    let start_tangent = tangent(c.p0, [c.p1, c.p2, c.p3])?;
    let start = c.p0 + left_of(start_tangent, d);
    let mut offset = BezPath::new();
    kurbo::offset::offset_cubic(c, d, tolerance, &mut offset);
    let mut els = offset.elements().to_vec();
    if matches!(els.first(), Some(PathEl::MoveTo(_))) {
        els.remove(0);
    }
    let end = els.iter().rev().find_map(|el| match el {
        PathEl::MoveTo(p) | PathEl::LineTo(p) | PathEl::QuadTo(_, p) | PathEl::CurveTo(_, _, p) => Some(*p),
        PathEl::ClosePath => None,
    })?;
    Some(Piece::Curve { start, end, els })
}

/// The miter of two lines that meet at `corner`: both are moved to meet at the point where their offsets cross, when that point is within `limit` offsets of the corner and neither line is
/// turned round by it. Two lines that do not meet so (parallel, or an acute corner past the limit) are left as they are, and the join between them is a bevel.
fn miter(first: &mut Piece, second: &mut Piece, corner: Point, d: f64, limit: f64) {
    let (Piece::Line { a: a0, b: b0 }, Piece::Line { a: a1, b: b1 }) = (&mut *first, &mut *second) else {
        return;
    };
    let (u0, u1) = (*b0 - *a0, *b1 - *a1);
    let turn = u0.cross(u1);
    let scale = u0.hypot() * u1.hypot();
    if !scale.is_finite() || scale <= 0.0 || turn.abs() < 1.0e-9 * scale {
        return;
    }
    let t = (*a1 - *b0).cross(u1) / turn;
    let at = *b0 + u0 * t;
    let (before, after) = (at - *a0, *b1 - at);
    let within = (at - corner).hypot() <= limit * d.abs();
    if at.x.is_finite() && at.y.is_finite() && within && before.dot(u0) >= 0.0 && after.dot(u1) >= 0.0 {
        *b0 = at;
        *a1 = at;
    }
}

/// The pieces of one subpath, offset: the segments of the path from `start`, each moved `d` to its left, with the corners between lines mitred. `closed` joins the last to the first.
fn offset_pieces(segments: &[PathEl], start: Point, closed: bool, d: f64, tolerance: f64) -> Vec<Piece> {
    let mut pieces: Vec<Piece> = Vec::with_capacity(segments.len());
    // Where each piece began on the path itself: the corner a join is made at.
    let mut corners: Vec<Point> = Vec::with_capacity(segments.len());
    let mut at = start;
    for el in segments {
        let (piece, to) = match *el {
            PathEl::LineTo(p) => (offset_line(at, p, d), p),
            PathEl::QuadTo(c, p) => (offset_curve(QuadBez::new(at, c, p).raise(), d, tolerance), p),
            PathEl::CurveTo(c1, c2, p) => (offset_curve(CubicBez::new(at, c1, c2, p), d, tolerance), p),
            PathEl::MoveTo(_) | PathEl::ClosePath => continue,
        };
        if let Some(piece) = piece {
            corners.push(at);
            pieces.push(piece);
        }
        at = to;
    }
    for k in 1..pieces.len() {
        let (before, after) = pieces.split_at_mut(k);
        if let (Some(head), Some(tail), Some(corner)) = (before.last_mut(), after.first_mut(), corners.get(k)) {
            miter(head, tail, *corner, d, MITRE_LIMIT);
        }
    }
    if closed && pieces.len() > 1 {
        let last = pieces.len() - 1;
        let (before, after) = pieces.split_at_mut(last);
        if let (Some(first), Some(end), Some(corner)) = (before.first_mut(), after.first_mut(), corners.first()) {
            miter(end, first, *corner, d, MITRE_LIMIT);
        }
    }
    pieces
}

/// Draws `pieces` into `out` as one subpath: a move to the first, a line over a gap between two that do not meet (a bevel), and each piece. `closed` closes it.
fn emit(out: &mut BezPath, pieces: &[Piece], closed: bool) {
    let Some(first) = pieces.first() else { return };
    out.move_to(first.start());
    let mut pen = first.start();
    for piece in pieces {
        if (piece.start() - pen).hypot() > NO_LENGTH {
            out.line_to(piece.start());
        }
        match piece {
            Piece::Line { b, .. } => out.line_to(*b),
            Piece::Curve { els, .. } => els.iter().for_each(|el| out.push(*el)),
        }
        pen = piece.end();
    }
    if closed {
        out.close_path();
    }
}

/// Whether every coordinate of the path is a number.
fn is_finite_path(path: &BezPath) -> bool {
    let finite = |p: &Point| p.x.is_finite() && p.y.is_finite();
    path.elements().iter().all(|el| match el {
        PathEl::MoveTo(p) | PathEl::LineTo(p) => finite(p),
        PathEl::QuadTo(a, b) => finite(a) && finite(b),
        PathEl::CurveTo(a, b, c) => finite(a) && finite(b) && finite(c),
        PathEl::ClosePath => true,
    })
}

/// The path `d` paper inches to the left of `path`, to within `tolerance`: every subpath offset on its own, its corners between lines mitred (to the limit of a stroke's miter, [`MITRE_LIMIT`])
/// and bevelled past it, a curve offset by `kurbo`. A segment with no length has no direction to offset by and is left out; a path with any coordinate that is no number has no offset.
///
/// The bands of a compound line are strokes of this path at the band's width.
pub(crate) fn offset_path(path: &BezPath, d: f64, tolerance: f64) -> BezPath {
    let mut out = BezPath::new();
    if !d.is_finite() || !tolerance.is_finite() || tolerance <= 0.0 || !is_finite_path(path) {
        return out;
    }
    let (mut start, mut at) = (Point::ZERO, Point::ZERO);
    let mut segments: Vec<PathEl> = Vec::new();
    // A subpath that comes back to where it began is closed, as a drawing's outline is whether or not it says so: its first corner is mitred like the others.
    let closes = |segments: &[PathEl], start: Point, at: Point| segments.len() > 1 && (at - start).hypot() <= NO_LENGTH;
    for el in path.elements() {
        match *el {
            PathEl::MoveTo(p) => {
                let closed = closes(&segments, start, at);
                emit(&mut out, &offset_pieces(&segments, start, closed, d, tolerance), closed);
                segments.clear();
                (start, at) = (p, p);
            }
            PathEl::LineTo(p) | PathEl::QuadTo(_, p) | PathEl::CurveTo(_, _, p) => {
                segments.push(*el);
                at = p;
            }
            PathEl::ClosePath => {
                if (at - start).hypot() > NO_LENGTH {
                    segments.push(PathEl::LineTo(start));
                }
                emit(&mut out, &offset_pieces(&segments, start, true, d, tolerance), true);
                segments.clear();
                at = start;
            }
        }
    }
    let closed = closes(&segments, start, at);
    emit(&mut out, &offset_pieces(&segments, start, closed, d, tolerance), closed);
    out
}

#[cfg(test)]
mod tests {
    use kurbo::ParamCurve;

    use super::*;

    /// The points a path's elements end at, in order.
    fn points(path: &BezPath) -> Vec<Point> {
        path.elements()
            .iter()
            .filter_map(|el| match el {
                PathEl::MoveTo(p) | PathEl::LineTo(p) | PathEl::QuadTo(_, p) | PathEl::CurveTo(_, _, p) => Some(*p),
                PathEl::ClosePath => None,
            })
            .collect()
    }

    fn polyline(through: &[(f64, f64)]) -> BezPath {
        let mut path = BezPath::new();
        for (k, (x, y)) in through.iter().enumerate() {
            if k == 0 {
                path.move_to((*x, *y));
            } else {
                path.line_to((*x, *y));
            }
        }
        path
    }

    fn near(a: Point, x: f64, y: f64) -> bool {
        (a.x - x).abs() < 1e-9 && (a.y - y).abs() < 1e-9
    }

    #[test]
    fn the_bands_of_each_type_share_the_weight_and_a_single_line_has_none() {
        // Type 1 is two bands of a third with a third between; 2 is 0.6, 0.2, 0.2; 3 is its mirror; 4 is a sixth, a sixth, a third, a sixth and a sixth.
        for (kind, widths) in [(1, vec![1.0 / 3.0; 2]), (2, vec![0.6, 0.2]), (3, vec![0.2, 0.6]), (4, vec![1.0 / 6.0, 1.0 / 3.0, 1.0 / 6.0])] {
            let found = bands(kind, 3.0);
            assert_eq!(found.iter().map(|b| b.width).collect::<Vec<_>>().len(), widths.len(), "type {kind}");
            for (band, share) in found.iter().zip(&widths) {
                assert!((band.width - share * 3.0).abs() < 1e-12, "type {kind}: {band:?}");
                // Every band lies inside the weight, between its edges.
                assert!(band.offset - band.width / 2.0 >= -1.5 - 1e-12 && band.offset + band.width / 2.0 <= 1.5 + 1e-12, "type {kind}: {band:?}");
            }
            // The first is on the left (a positive offset) and the last on the right, and bands do not overlap.
            assert!(found.first().is_some_and(|b| b.offset > 0.0) && found.last().is_some_and(|b| b.offset < 0.0), "type {kind}");
            assert!(found.windows(2).all(|w| w[0].offset - w[0].width / 2.0 >= w[1].offset + w[1].width / 2.0 - 1e-12), "type {kind}");
        }
        for kind in [0, 5, 6, 255] {
            assert!(bands(kind, 3.0).is_empty(), "type {kind} is one line");
        }
        for total in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(bands(1, total).is_empty(), "a weight of {total} has no bands");
        }
    }

    #[test]
    fn a_line_offset_to_the_left_runs_parallel_at_the_distance_and_to_the_right_for_a_negative_one() {
        let path = polyline(&[(0.0, 0.0), (4.0, 0.0)]);
        assert_eq!(points(&offset_path(&path, 0.5, 0.01)).len(), 2);
        let left = points(&offset_path(&path, 0.5, 0.01));
        assert!(near(left[0], 0.0, 0.5) && near(left[1], 4.0, 0.5), "{left:?}");
        let right = points(&offset_path(&path, -0.5, 0.01));
        assert!(near(right[0], 0.0, -0.5) && near(right[1], 4.0, -0.5), "{right:?}");
        // Up the page the left is toward -x.
        let up = points(&offset_path(&polyline(&[(0.0, 0.0), (0.0, 4.0)]), 0.5, 0.01));
        assert!(near(up[0], -0.5, 0.0) && near(up[1], -0.5, 4.0), "{up:?}");
    }

    #[test]
    fn a_corner_is_mitred_to_where_the_offsets_cross_on_both_sides() {
        // East then north, a left turn: the left offset is the inside of the corner, the right the outside.
        let path = polyline(&[(0.0, 0.0), (3.0, 0.0), (3.0, 3.0)]);
        let inside = points(&offset_path(&path, 0.25, 0.01));
        assert_eq!(inside.len(), 3, "{inside:?}");
        assert!(near(inside[0], 0.0, 0.25) && near(inside[1], 2.75, 0.25) && near(inside[2], 2.75, 3.0), "{inside:?}");
        let outside = points(&offset_path(&path, -0.25, 0.01));
        assert!(near(outside[0], 0.0, -0.25) && near(outside[1], 3.25, -0.25) && near(outside[2], 3.25, 3.0), "{outside:?}");
    }

    #[test]
    fn a_corner_sharper_than_the_miter_limit_is_bevelled_and_not_spiked() {
        // A turn of about 161 degrees: the outer miter would reach several offsets from the corner. It is bevelled: the two ends stay apart and a line joins them.
        let path = polyline(&[(0.0, 0.0), (4.0, 0.0), (0.0, 0.5)]);
        let found = points(&offset_path(&path, -0.1, 0.01));
        assert!(found.len() >= 4, "bevelled, with a point on each side of the corner: {found:?}");
        assert!(found.iter().all(|p| p.x < 4.0 + 0.1 * MITRE_LIMIT + 1e-9), "no spike past the limit: {found:?}");
    }

    #[test]
    fn a_closed_outline_is_offset_whole_and_stays_closed() {
        let mut square = polyline(&[(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0)]);
        square.close_path();
        let inward = offset_path(&square, 0.25, 0.01);
        assert!(matches!(inward.elements().last(), Some(PathEl::ClosePath)), "{:?}", inward.elements());
        let p = points(&inward);
        assert_eq!(p.len(), 5, "a move and four sides, the last back at the start: {p:?}");
        assert!(
            near(p[0], 0.25, 0.25) && near(p[1], 1.75, 0.25) && near(p[2], 1.75, 1.75) && near(p[3], 0.25, 1.75) && near(p[4], 0.25, 0.25),
            "the corner at the start is mitred too: {p:?}"
        );
        // The same outline drawn back to its first point with no close is closed all the same.
        let open = polyline(&[(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0), (0.0, 0.0)]);
        let q = points(&offset_path(&open, 0.25, 0.01));
        assert!(near(q[0], 0.25, 0.25) && matches!(offset_path(&open, 0.25, 0.01).elements().last(), Some(PathEl::ClosePath)), "{q:?}");
    }

    #[test]
    fn a_curve_is_offset_by_the_distance_and_a_curve_on_a_line_is_a_line() {
        let mut arc = BezPath::new();
        arc.move_to((0.0, 0.0));
        arc.curve_to((1.0, 1.0), (3.0, 1.0), (4.0, 0.0));
        let found = offset_path(&arc, 0.2, 0.001);
        let p = points(&found);
        assert!(found.elements().iter().any(|el| matches!(el, PathEl::CurveTo(..))), "the offset of a bend is a curve: {:?}", found.elements());
        // The curve is symmetric about x = 2 and 0.75 high there, so the offset passes 0.2 above that, at (2, 0.95): the pieces of it, however many, come within a hundredth of it.
        let nearest = found
            .segments()
            .map(|seg| (0..=100).map(|k| seg.eval(f64::from(k) / 100.0).distance(Point::new(2.0, 0.95))).fold(f64::INFINITY, f64::min))
            .fold(f64::INFINITY, f64::min);
        assert!(nearest < 0.01, "the offset passes through (2, 0.95): {nearest}");
        // Its ends are 0.2 to the left of the curve's ends (the tangents there are not along x, so only the distance is checked).
        assert!(((p[0] - Point::new(0.0, 0.0)).hypot() - 0.2).abs() < 1e-9, "{:?}", p[0]);
        assert!(((p[p.len() - 1] - Point::new(4.0, 0.0)).hypot() - 0.2).abs() < 1e-3, "{:?}", p[p.len() - 1]);
        // Control points on the chord are a line.
        let mut flat = BezPath::new();
        flat.move_to((0.0, 0.0));
        flat.curve_to((1.0, 0.0), (2.0, 0.0), (3.0, 0.0));
        let line = points(&offset_path(&flat, 0.5, 0.001));
        assert_eq!(line.len(), 2, "{line:?}");
        assert!(near(line[0], 0.0, 0.5) && near(line[1], 3.0, 0.5), "{line:?}");
        // A quadratic goes the same way.
        let mut quad = BezPath::new();
        quad.move_to((0.0, 0.0));
        quad.quad_to((2.0, 2.0), (4.0, 0.0));
        assert!(offset_path(&quad, 0.2, 0.001).elements().iter().any(|el| matches!(el, PathEl::CurveTo(..))));
    }

    #[test]
    fn what_has_no_direction_or_no_number_has_no_offset_and_costs_nothing_beyond_its_size() {
        // Points that are no numbers, a distance or a tolerance that is none, and a tolerance of 0 or less: nothing, with no panic.
        let good = polyline(&[(0.0, 0.0), (1.0, 0.0)]);
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(offset_path(&polyline(&[(0.0, 0.0), (bad, 1.0)]), 0.1, 0.01).is_empty());
            assert!(offset_path(&polyline(&[(bad, 0.0), (1.0, 1.0)]), 0.1, 0.01).is_empty());
            assert!(offset_path(&good, bad, 0.01).is_empty());
            assert!(offset_path(&good, 0.1, bad).is_empty());
        }
        assert!(offset_path(&good, 0.1, 0.0).is_empty() && offset_path(&good, 0.1, -1.0).is_empty());
        // Repeated points, a lone move, an empty path and a path of nothing but closes: no direction anywhere.
        assert!(offset_path(&polyline(&[(2.0, 2.0), (2.0, 2.0), (2.0, 2.0)]), 0.1, 0.01).is_empty());
        assert!(offset_path(&BezPath::new(), 0.1, 0.01).is_empty());
        let mut lone = BezPath::new();
        lone.move_to((1.0, 1.0));
        lone.close_path();
        lone.close_path();
        assert!(offset_path(&lone, 0.1, 0.01).is_empty());
        // A cubic that is a point.
        let mut dot = BezPath::new();
        dot.move_to((1.0, 1.0));
        dot.curve_to((1.0, 1.0), (1.0, 1.0), (1.0, 1.0));
        assert!(offset_path(&dot, 0.1, 0.01).is_empty());
        // A repeated point inside a line changes nothing about it.
        let twice = points(&offset_path(&polyline(&[(0.0, 0.0), (0.0, 0.0), (2.0, 0.0), (2.0, 0.0)]), 0.5, 0.01));
        assert_eq!(twice.len(), 2, "{twice:?}");
    }

    #[test]
    fn a_huge_coordinate_and_a_long_path_give_a_finite_answer_in_counted_work() {
        // Coordinates near the largest a path may hold are offset without overflow into a number that is none.
        let huge = polyline(&[(-1.0e12, 0.0), (1.0e12, 0.0), (1.0e12, 1.0e12)]);
        let found = offset_path(&huge, 0.5, 0.01);
        assert!(points(&found).iter().all(|p| p.x.is_finite() && p.y.is_finite()));
        // A hundred thousand segments make a hundred thousand pieces and no more; the work counted for them is as many units as the path has elements, for each band.
        let long: Vec<(f64, f64)> = (0..100_000).map(|k| (f64::from(k) * 0.001, if k % 2 == 0 { 0.0 } else { 0.5 })).collect();
        let path = polyline(&long);
        assert_eq!(offset_work(&path, 2), 2 * 100_000, "a unit for each element of a path of lines, and each band");
        let found = offset_path(&path, 0.01, 0.01);
        // A turn of nearly half a circle is bevelled, so there may be a point more at a corner; there are never fewer than the path's own.
        assert!(points(&found).len() >= 99_999, "{} points", points(&found).len());
        assert!(points(&found).len() <= 2 * 100_000, "and at most a bevel at each");
        // A curve is charged more than a line is, and the work of no bands is nothing.
        let mut bend = BezPath::new();
        bend.move_to((0.0, 0.0));
        bend.curve_to((1.0, 1.0), (3.0, 1.0), (4.0, 0.0));
        assert!(offset_work(&bend, 1) > offset_work(&polyline(&[(0.0, 0.0), (4.0, 0.0)]), 1));
        assert_eq!(offset_work(&bend, 0), 0);
    }
}
