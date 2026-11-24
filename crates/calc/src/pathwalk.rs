//! Walking a path: the segments a section's rows draw, their lengths, the place a share of
//! the length along, the direction there, and the place nearest a point.
//!
//! **What a walk is made of.** The rows of a `Geometry` section, drawn one row at a time by `diacraft_geom::build_row_draws`, become *pieces*:
//! each piece is a straight line, a quadratic or a cubic Bezier, and belongs to the **segment** (the drawn row, numbered from 1) that made it.
//! A move, or a spline that does not start where the pen is, is a gap: it has no length and no place. Nothing closes a path.
//!
//! **`Rounding`.** Under a positive radius `r`, a corner between two straight pieces is cut at each side by `r / tan(a / 2)`, with `a`
//! the angle between the sides, and the walk crosses from one cut to the other by a straight chord, not the arc a renderer draws. Each cut stops at
//! half the shorter of the two sides. A corner touching a curve stays sharp and a run's open ends stay sharp; a closed run's closing corner
//! rounds, and the walk then starts where the first side's cut ends (project rule). The chord belongs to the segment that arrives at the
//! corner (project rule: nothing settles which of the two segments it is part of).
//!
//! **Cost.** Building a walk and finding a nearest place are linear in the pieces; finding the place a length along is a binary search and one
//! curve inversion. The caller charges the work before it asks (`fn_path.rs`).

use diacraft_geom::kurbo::{
    CubicBez, Line, ParamCurve, ParamCurveArclen, ParamCurveDeriv, ParamCurveExtrema, ParamCurveNearest, PathSeg, Point, QuadBez, Rect, Vec2,
};
use diacraft_geom::{DrawStep, RowDraw};

/// A tangent shorter than this share of the path's extent has no direction (the derivative of a cubic that stops at an end).
const NO_DIRECTION: f64 = 1e-12;

/// How close two distances must be to tie, as a share of the larger and of 1: the earlier place wins a tie.
const TIE: f64 = 1e-12;

/// The accuracy of a curve's length, as a share of the path's extent: far below the 2E-5 the spec's curve counts agree within.
const LENGTH_ACCURACY: f64 = 1e-11;

/// One piece of a walk.
#[derive(Clone, Copy, Debug)]
struct Piece {
    seg: PathSeg,
    /// The segment (drawn row) that made it, from 1.
    row: u32,
    /// The length before it, and its own.
    start: f64,
    len: f64,
}

/// A piece before its length is known.
#[derive(Clone, Copy)]
struct Raw {
    seg: PathSeg,
    row: u32,
}

/// Why a walk could not be made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Stop {
    /// A length is not a number, or too big to add.
    NotFinite,
}

/// Where a length along the path falls.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Place {
    pub point: Point,
    /// The direction of travel there, a unit vector.
    pub tangent: Vec2,
}

/// The place nearest a point.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Closest {
    pub distance: f64,
    /// How far along the path it is, in the path's length.
    pub along: f64,
}

/// A path as the pieces its rows draw.
#[derive(Debug)]
pub(crate) struct Walk {
    pieces: Vec<Piece>,
    /// The length of the whole path.
    pub total: f64,
    /// The drawn rows, which are the path's segments.
    pub segments: u32,
    /// The path's extent, for the accuracy of curves and the size of an angle that counts.
    scale: f64,
}

fn straight(p: Point, q: Point) -> PathSeg {
    PathSeg::Line(Line::new(p, q))
}

fn seg_of(pen: Point, step: &DrawStep) -> Option<(PathSeg, Point)> {
    match *step {
        DrawStep::Jump(_) => None,
        DrawStep::Line(e) => Some((straight(pen, e), e)),
        DrawStep::Quad(c, e) => Some((PathSeg::Quad(QuadBez::new(pen, c, e)), e)),
        DrawStep::Cubic(a, b, e) => Some((PathSeg::Cubic(CubicBez::new(pen, a, b, e)), e)),
    }
}

fn is_line(seg: &PathSeg) -> bool {
    matches!(seg, PathSeg::Line(_))
}

fn line_len(seg: &PathSeg) -> f64 {
    seg.end().distance(seg.start())
}

/// The length of a piece. A curve whose control polygon has no length counts 0: kurbo's `arclen` of a quadratic whose three points coincide is
/// not a number, which would make the whole section an error (a `RelQuadBezTo` on a shape of no size). A curve is never longer than its control
/// polygon, so one whose polygon is within `accuracy` is within `accuracy` of 0 whatever else it does.
fn piece_len(seg: &PathSeg, accuracy: f64) -> f64 {
    let polygon = match seg {
        PathSeg::Line(_) => return line_len(seg),
        PathSeg::Quad(q) => q.p0.distance(q.p1) + q.p1.distance(q.p2),
        PathSeg::Cubic(c) => c.p0.distance(c.p1) + c.p1.distance(c.p2) + c.p2.distance(c.p3),
    };
    if polygon <= accuracy { 0.0 } else { seg.arclen(accuracy) }
}

/// The unit direction of a straight piece.
fn unit(seg: &PathSeg) -> Vec2 {
    let v = seg.end() - seg.start();
    let l = v.hypot();
    if l > 0.0 { v / l } else { Vec2::ZERO }
}

/// Whether two points are one: the run's end and start, in the shape's frame (as `geom` closes a run).
fn closed(start: Point, end: Point) -> bool {
    diacraft_geom::is_closed(start, end)
}

/// Cuts the corners of a run: the radius `r` is above 0.
fn round_run(run: Vec<Raw>, r: f64, scale: f64) -> Vec<Raw> {
    let n = run.len();
    let tiny = NO_DIRECTION.max(1e-13 * scale);
    // The pieces that have a length or are curves: the ones corners are between.
    let live: Vec<usize> = (0..n).filter(|&i| run.get(i).is_some_and(|p| !is_line(&p.seg) || line_len(&p.seg) > tiny)).collect();
    let mut cut_start = vec![0.0f64; n];
    let mut cut_end = vec![0.0f64; n];
    let mut chord: Vec<Option<(Point, Point)>> = vec![None; n];
    let corner = |a: usize, b: usize, cut_end: &mut [f64], cut_start: &mut [f64], chord: &mut [Option<(Point, Point)>]| {
        let (Some(pa), Some(pb)) = (run.get(a), run.get(b)) else { return };
        if !(is_line(&pa.seg) && is_line(&pb.seg)) {
            return;
        }
        let (d1, d2) = (unit(&pa.seg), unit(&pb.seg));
        let dot = d1.dot(d2).clamp(-1.0, 1.0);
        // The turn from one side to the next is `theta`, and the angle between the sides `pi - theta`: r / tan(a / 2) is r * tan(theta / 2).
        let raw = r * ((1.0 - dot) / (1.0 + dot)).sqrt();
        let limit = 0.5 * line_len(&pa.seg).min(line_len(&pb.seg));
        let t = if raw.is_nan() { 0.0 } else { raw.min(limit) };
        if t > tiny
            && let (Some(ce), Some(cs), Some(ch)) = (cut_end.get_mut(a), cut_start.get_mut(b), chord.get_mut(a))
        {
            *ce = t;
            *cs = t;
            *ch = Some((pa.seg.end() - d1 * t, pb.seg.start() + d2 * t));
        }
    };
    for pair in live.windows(2) {
        if let [a, b] = *pair {
            corner(a, b, &mut cut_end, &mut cut_start, &mut chord);
        }
    }
    // A closed run's closing corner rounds.
    if let (Some(&first), Some(&last)) = (live.first(), live.last())
        && first != last
        && let (Some(f), Some(l)) = (run.get(first), run.get(last))
        && closed(f.seg.start(), l.seg.end())
    {
        corner(last, first, &mut cut_end, &mut cut_start, &mut chord);
    }
    let mut out = Vec::with_capacity(n + live.len());
    for (i, piece) in run.iter().enumerate() {
        let seg = match piece.seg {
            PathSeg::Line(_) => {
                let d = unit(&piece.seg);
                let from = piece.seg.start() + d * cut_start.get(i).copied().unwrap_or(0.0);
                let to = piece.seg.end() - d * cut_end.get(i).copied().unwrap_or(0.0);
                straight(from, to)
            }
            other => other,
        };
        out.push(Raw { seg, row: piece.row });
        if let Some(Some((from, to))) = chord.get(i) {
            out.push(Raw { seg: straight(*from, *to), row: piece.row });
        }
    }
    out
}

impl Walk {
    /// The walk of the rows `draws` drew, with `Rounding` of `rounding` (a value that is not above 0 or not a number rounds nothing). `charge` is
    /// asked for the steps of the work before it is done, and says no when the budget is spent.
    pub(crate) fn build(draws: &[RowDraw], rounding: f64, mut charge: impl FnMut(usize) -> bool) -> Result<Option<Walk>, Stop> {
        let steps: usize = draws.iter().map(|d| d.steps.len()).sum();
        if !charge(steps) {
            return Ok(None);
        }
        let mut runs: Vec<Vec<Raw>> = Vec::new();
        let mut current: Vec<Raw> = Vec::new();
        let mut pen: Option<Point> = None;
        let mut segments = 0u32;
        let (mut lo, mut hi) = (Point::new(f64::INFINITY, f64::INFINITY), Point::new(f64::NEG_INFINITY, f64::NEG_INFINITY));
        let mut grow = |p: Point| {
            lo = Point::new(lo.x.min(p.x), lo.y.min(p.y));
            hi = Point::new(hi.x.max(p.x), hi.y.max(p.y));
        };
        for draw in draws {
            if draw.steps.iter().any(|s| !matches!(s, DrawStep::Jump(_))) {
                segments = segments.saturating_add(1);
            }
            for step in &draw.steps {
                match *step {
                    DrawStep::Jump(p) => {
                        if !current.is_empty() {
                            runs.push(std::mem::take(&mut current));
                        }
                        pen = Some(p);
                        grow(p);
                    }
                    _ => {
                        let Some(from) = pen else { continue };
                        let Some((seg, to)) = seg_of(from, step) else { continue };
                        if !(seg.start().is_finite() && to.is_finite()) {
                            return Err(Stop::NotFinite);
                        }
                        grow(to);
                        match step {
                            DrawStep::Quad(c, _) => grow(*c),
                            DrawStep::Cubic(a, b, _) => {
                                grow(*a);
                                grow(*b);
                            }
                            _ => {}
                        }
                        current.push(Raw { seg, row: segments });
                        pen = Some(to);
                    }
                }
            }
        }
        if !current.is_empty() {
            runs.push(current);
        }
        let scale = {
            let size = (hi.x - lo.x).max(hi.y - lo.y);
            let reach = lo.x.abs().max(lo.y.abs()).max(hi.x.abs()).max(hi.y.abs());
            let s = size.max(reach);
            if s.is_finite() { s.max(1.0) } else { 1.0 }
        };
        let r = if rounding.is_finite() && rounding > 0.0 { rounding } else { 0.0 };
        let mut pieces: Vec<Piece> = Vec::new();
        let mut at = 0.0f64;
        let accuracy = LENGTH_ACCURACY * scale;
        for run in runs {
            let run = if r > 0.0 { round_run(run, r, scale) } else { run };
            for raw in run {
                let len = piece_len(&raw.seg, accuracy);
                if !len.is_finite() || len < 0.0 {
                    return Err(Stop::NotFinite);
                }
                pieces.push(Piece { seg: raw.seg, row: raw.row, start: at, len });
                at += len;
            }
        }
        if !at.is_finite() {
            return Err(Stop::NotFinite);
        }
        Ok(Some(Walk { pieces, total: at, segments, scale }))
    }

    /// The pieces, for the caller to charge a walk over them.
    pub(crate) fn piece_count(&self) -> usize {
        self.pieces.len()
    }

    /// Where the length of segment `row` begins and how long it is, or `None` for a segment with no piece.
    pub(crate) fn segment_range(&self, row: u32) -> Option<(f64, f64)> {
        let mut found: Option<(f64, f64)> = None;
        for p in self.pieces.iter().filter(|p| p.row == row) {
            found = Some(match found {
                None => (p.start, p.len),
                Some((s, l)) => (s, l + p.len),
            });
        }
        found
    }

    fn eps(&self) -> f64 {
        1e-12 * self.total.max(1.0)
    }

    /// The piece a length falls in: with `leaving` the one that starts there when two meet, else the one that ends there
    /// Pieces of no length are never chosen. With a `row` only that segment's pieces are, and a length at its end is in
    /// its last piece. `None` for a path of no length.
    fn piece_at(&self, d: f64, leaving: bool, row: Option<u32>) -> Option<&Piece> {
        let eps = self.eps();
        let fit = |p: &&Piece| p.len > 0.0 && row.is_none_or(|r| p.row == r);
        let from = if leaving {
            // The first piece that ends past `d`; past the end, the last piece that fits.
            self.pieces.partition_point(|p| p.start + p.len <= d + eps)
        } else {
            // The first piece that ends at or after `d`.
            self.pieces.partition_point(|p| p.start + p.len < d - eps)
        };
        self.pieces.get(from..)?.iter().find(fit).or_else(|| self.pieces.iter().rev().find(fit))
    }

    /// The segment a length along the path falls in; on the join of two, the earlier one.
    pub(crate) fn segment_at(&self, d: f64) -> Option<u32> {
        self.piece_at(d, false, None).map(|p| p.row)
    }

    fn param_at(&self, piece: &Piece, local: f64) -> f64 {
        if piece.len <= 0.0 {
            return 0.0;
        }
        let local = local.clamp(0.0, piece.len);
        match piece.seg {
            PathSeg::Line(_) => local / piece.len,
            curve => curve.inv_arclen(local, LENGTH_ACCURACY * self.scale).clamp(0.0, 1.0),
        }
    }

    /// The direction of a piece at `t`, a unit vector: the derivative, or where that has no length the direction of the piece's chord.
    fn tangent_of(&self, piece: &Piece, t: f64) -> Vec2 {
        let floor = NO_DIRECTION * self.scale;
        let derivative = |t: f64| match piece.seg {
            PathSeg::Line(l) => l.p1 - l.p0,
            PathSeg::Quad(q) => q.deriv().eval(t).to_vec2(),
            PathSeg::Cubic(c) => c.deriv().eval(t).to_vec2(),
        };
        for at in [t, t.clamp(1e-4, 1.0 - 1e-4), t.clamp(1e-2, 1.0 - 1e-2)] {
            let v = derivative(at);
            let l = v.hypot();
            if l.is_finite() && l > floor {
                return v / l;
            }
        }
        let chord = piece.seg.end() - piece.seg.start();
        let l = chord.hypot();
        if l.is_finite() && l > floor { chord / l } else { Vec2::new(1.0, 0.0) }
    }

    /// The place `d` along the path, clamped to the path, or to segment `row` alone when one is given; where two pieces meet the one leaving
    /// it, and at the very end the one arriving. `None` for a path of no length.
    pub(crate) fn place_at(&self, d: f64, row: Option<u32>) -> Option<Place> {
        let d = d.clamp(0.0, self.total);
        let piece = self.piece_at(d, true, row)?;
        let t = self.param_at(piece, d - piece.start);
        Some(Place { point: piece.seg.eval(t), tangent: self.tangent_of(piece, t) })
    }

    /// The place on the path nearest `p`, the earlier of two equally near. `None` for a path of no length.
    pub(crate) fn nearest(&self, p: Point) -> Option<Closest> {
        let accuracy = LENGTH_ACCURACY * self.scale;
        let mut best: Option<(f64, &Piece, f64)> = None;
        for piece in self.pieces.iter().filter(|q| q.len > 0.0) {
            let near = piece.seg.nearest(p, accuracy);
            let distance = near.distance_sq.max(0.0).sqrt();
            if !distance.is_finite() {
                continue;
            }
            let better = match best {
                None => true,
                Some((d, _, _)) => distance < d - TIE * d.max(1.0),
            };
            if better {
                best = Some((distance, piece, near.t.clamp(0.0, 1.0)));
            }
        }
        let (distance, piece, t) = best?;
        let along = piece.start
            + match piece.seg {
                PathSeg::Line(_) => t * piece.len,
                curve => piece_len(&curve.subsegment(0.0..t), accuracy).min(piece.len),
            };
        Some(Closest { distance, along })
    }

    /// The extent of all the pieces.
    pub(crate) fn extent(&self) -> Option<Rect> {
        let mut out: Option<Rect> = None;
        for p in &self.pieces {
            let b = p.seg.bounding_box();
            out = Some(out.map_or(b, |r| r.union(b)));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use diacraft_geom::{Bound, GeomRow, build_row_draws, kurbo::Size};

    use super::*;

    fn walk(rows: &[GeomRow], rounding: f64) -> Walk {
        let draws = build_row_draws(rows, Size::new(1.0, 1.0), Bound::default()).unwrap();
        Walk::build(&draws.rows, rounding, |_| true).unwrap().unwrap()
    }

    fn mv(x: f64, y: f64) -> GeomRow {
        GeomRow::MoveTo { x, y }
    }

    fn ln(x: f64, y: f64) -> GeomRow {
        GeomRow::LineTo { x, y }
    }

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    /// A move to (0, 0), lines to (4, 0) and (4, 3), `Rounding` 1: a corner cut from (3, 0) to (4, 1), length 6.41421, and travel
    /// 0.8 at (4, 1.71716). Naive: an arc gives 6.57080, sharp corners 7.
    #[test]
    fn a_corner_is_cut_by_a_chord() {
        let w = walk(&[mv(0.0, 0.0), ln(4.0, 0.0), ln(4.0, 3.0)], 1.0);
        assert!(near(w.total, 3.0 + 2.0f64.sqrt() + 2.0), "{}", w.total);
        let at = w.place_at(0.8 * w.total, None).unwrap();
        assert!(near(at.point.x, 4.0) && (at.point.y - 1.71716).abs() < 1e-5, "{:?}", at.point);
        let sharp = walk(&[mv(0.0, 0.0), ln(4.0, 0.0), ln(4.0, 3.0)], 0.0);
        assert!(near(sharp.total, 7.0));
    }

    /// `Rounding` 5 on that path: each side cut by 1.5, half the shorter, length 6.12132, travel 0.8 at (4, 1.77574).
    #[test]
    fn a_cut_stops_at_half_the_shorter_side() {
        let w = walk(&[mv(0.0, 0.0), ln(4.0, 0.0), ln(4.0, 3.0)], 5.0);
        assert!((w.total - 6.12132).abs() < 1e-5, "{}", w.total);
        let at = w.place_at(0.8 * w.total, None).unwrap();
        assert!(near(at.point.x, 4.0) && (at.point.y - 1.77574).abs() < 1e-5, "{:?}", at.point);
    }

    /// The nearest place to (4, 0) is the chord's middle, 0.57795 of the way.
    #[test]
    fn the_nearest_place_to_a_cut_corner_is_the_middle_of_the_chord() {
        let w = walk(&[mv(0.0, 0.0), ln(4.0, 0.0), ln(4.0, 3.0)], 1.0);
        let c = w.nearest(Point::new(4.0, 0.0)).unwrap();
        assert!((c.along / w.total - 0.57795).abs() < 1e-5, "{}", c.along / w.total);
        assert!((c.distance - 0.5f64.sqrt()).abs() < 1e-9, "the chord is {} from the corner", c.distance);
    }

    /// A corner that touches a curve stays sharp, and so do the ends of an open run.
    #[test]
    fn a_corner_that_touches_a_curve_stays_sharp() {
        let w = walk(&[mv(0.0, 0.0), ln(2.0, 0.0), GeomRow::ArcTo { x: 4.0, y: 0.0, bow: 1.0 }], 1.0);
        let sharp = walk(&[mv(0.0, 0.0), ln(2.0, 0.0), GeomRow::ArcTo { x: 4.0, y: 0.0, bow: 1.0 }], 0.0);
        assert!(near(w.total, sharp.total));
        let open = walk(&[mv(0.0, 0.0), ln(4.0, 0.0)], 1.0);
        assert!(near(open.total, 4.0));
    }

    /// A closed run's closing corner rounds, so a rectangle loses four corners' worth.
    #[test]
    fn a_closed_run_rounds_its_closing_corner() {
        let rect = [mv(0.0, 0.0), ln(4.0, 0.0), ln(4.0, 2.0), ln(0.0, 2.0), ln(0.0, 0.0)];
        let w = walk(&rect, 0.5);
        let saved = 4.0 * (2.0 * 0.5 - 2.0f64.sqrt() * 0.5);
        assert!(near(w.total, 12.0 - saved), "{} against {}", w.total, 12.0 - saved);
    }

    /// At a join the place and the direction are of the row leaving it; at the very end, of the row arriving; the
    /// segment is the earlier.
    #[test]
    fn a_join_belongs_to_the_row_leaving_it_for_a_place_and_the_earlier_for_a_segment() {
        let w = walk(&[mv(0.0, 0.0), ln(3.0, 0.0), ln(3.0, 5.0)], 0.0);
        let join = w.place_at(3.0, None).unwrap();
        assert!(near(join.tangent.y, 1.0) && near(join.point.x, 3.0), "{join:?}");
        let end = w.place_at(8.0, None).unwrap();
        assert!(near(end.tangent.y, 1.0) && near(end.point.y, 5.0));
        assert_eq!((w.segment_at(3.0), w.segment_at(0.0), w.segment_at(8.0), w.segments), (Some(1), Some(1), Some(2), 2));
    }

    /// A nearest place at the very start of a curve is 0 along it: kurbo's `arclen` of a quadratic whose three points coincide (the subsegment
    /// from 0 to 0) is not a number, and `f64::min` then answers the piece's whole length.
    #[test]
    fn the_nearest_place_at_the_start_of_a_curve_is_no_way_along_it() {
        let quad = [mv(0.0, 0.0), GeomRow::RelQuadBezTo { x: 4.0, y: 0.0, c_x: 2.0, c_y: 2.0 }];
        let w = walk(&quad, 0.0);
        let c = w.nearest(Point::new(-1.0, 0.0)).unwrap();
        assert!(c.along.abs() < 1e-9, "the quad's start is {} along", c.along);
        assert!((c.distance - 1.0).abs() < 1e-9, "{}", c.distance);
        let cubic = [mv(0.0, 0.0), GeomRow::RelCubBezTo { x: 4.0, y: 0.0, c1_x: 1.0, c1_y: 2.0, c2_x: 3.0, c2_y: 2.0 }];
        let c = walk(&cubic, 0.0).nearest(Point::new(-1.0, 0.0)).unwrap();
        assert!(c.along.abs() < 1e-9, "the cubic's start is {} along", c.along);
    }

    /// A path of no length has no place, and its extent is a point.
    #[test]
    fn a_path_with_no_length_has_no_place() {
        let w = walk(&[mv(1.0, 1.0), ln(1.0, 1.0)], 0.0);
        assert!(w.total == 0.0 && w.place_at(0.0, None).is_none() && w.nearest(Point::new(0.0, 0.0)).is_none());
        assert_eq!(w.segments, 1, "a drawn row of no length is still a row");
    }

    /// Hostile: a rounding that is not a number, negative or infinite rounds nothing, and coordinates that overflow the length stop the walk.
    #[test]
    fn a_rounding_that_is_no_number_rounds_nothing_and_an_overflow_stops_the_walk() {
        for r in [f64::NAN, -1.0, f64::NEG_INFINITY, 0.0] {
            assert!(near(walk(&[mv(0.0, 0.0), ln(4.0, 0.0), ln(4.0, 3.0)], r).total, 7.0));
        }
        let no_splines = Bound { curve_work: 0, ..Bound::default() };
        let draws = build_row_draws(&[mv(-1e308, 0.0), ln(1e308, 0.0)], Size::new(1.0, 1.0), no_splines).unwrap();
        assert_eq!(Walk::build(&draws.rows, 0.0, |_| true).err(), Some(Stop::NotFinite));
        let draws = build_row_draws(&[mv(0.0, 0.0), ln(4.0, 0.0)], Size::new(1.0, 1.0), no_splines).unwrap();
        assert!(Walk::build(&draws.rows, 0.0, |_| false).unwrap().is_none(), "a spent budget stops it before the work");
    }
}
