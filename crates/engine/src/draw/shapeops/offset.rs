//! Offset's sides (SHAPEOPS-OFFSET): the paths a distance from a path, on each side of it.
//!
//! **What the library gives, and what is built of it.** `craft_pathops` has the two pieces and neither is Offset:
//!
//! * `try_offset_path` grows a **closed** path by a distance (a positive one: its fill together with the stroke of twice the distance) or shrinks it (a negative one). It
//!   is the outer path and the inner path of R38, and an inner side that vanishes is an empty result, which is not made. Corners are mitred up to the stroker's limit and
//!   bevelled past it (kurbo's miter limit, the ratio of the mitre's length to the stroke's width: the limit [`MITRE_LIMIT`] of 4 bevels a corner whose tip would stand
//!   more than 4 times the distance from it);
//! * `try_outline_stroke` gives the region a stroke of twice the distance covers round an **open** path, with butt caps, which are the lines through the ends square to the
//!   path: one ring. It is not two paths. The two sides are the ring cut at its two caps, which are found as the straight edges that join the points `distance` to
//!   each side of the path's two ends; the ring is walked from there. A ring that has no such caps (the path crosses itself, or bends so hard that the two sides run into
//!   each other) cannot be cut into its sides and is refused in words. Loops that a tight bend would form on its inner side are already gone from the ring (the library
//!   unites the overlaps), which is R38's "loops are removed".
//!
//! **Accuracy.** Neither call takes a precision: they refit to the library's default (0.01) and drop slivers thinner than it, which is a hundred times what R46 allows (a circle
//! of radius 1 offset by 0.25 is 6E-3 off the exact one, and an inner side 0.01 wide is dropped). The work is done at [`SCALE`] times the size, where that default is
//! [`SCALE`] times smaller (1E-5, the precision the other operations are given and the least two vertices are apart), and the sides are scaled back.
//!
//! **The library's work for the command is one `MAX_SWEEP_WORK`.** The library refuses a call whose sweep would take more work than `Limits::sweep_work` (its dry sweep, which
//! counts the pieces, the crossings and the rest before the real one runs, and is not run below 192 pieces), and each call has the limit to itself: a command that makes many
//! calls (a section of a hundred open paths, each a star of 900 vertices, which the library takes in about 3 s each) would be as many limits. So [`Meter`] gives each call its
//! **share** of the one limit, in proportion to the segments it is given, and a call is refused in words past its share; the shares of all the calls of a command add up to the
//! limit of one call, and a command with a single call takes all of it. The share is also a share of the places where lines cross (3.5 microseconds each), which the library
//! counts before it sweeps, and the pairs of segments are held to what an offset is held to.
//!
//! **What comes out** is, for each section of an operand, the runs of its outer or left sides together as one shape and the runs of its inner or right sides as another
//! The closed runs of a section are offset together, as one fill (a ring with a hole keeps the hole), and each open run on its own. A side's runs are in the
//! order the library gave the closed ones and then the open ones in path order.

use craft_pathops::{Cap, FillRule, Join, Limits, MAX_SWEEP_WORK, try_normalize, try_offset_path, try_outline_stroke, with_limits};
use kurbo::{BezPath, PathEl, Point, Shape as _, Vec2};

use super::rows::{Edge, Loop, rings_of};
use super::runs::{Run, Seg, runs_of, runs_path};
use super::{MIN_AREA, ops};
use crate::draw::Problem;

/// The limit past which a mitre is bevelled: kurbo's, the ratio of the mitre's length to the stroke's width. A corner of interior angle `a` has a mitre that long as
/// `1 / sin (a / 2)`, so the tip is at `distance / sin (a / 2)` from the corner, and with a limit of 4 a corner sharper than about 29 degrees is bevelled.
pub(super) const MITRE_LIMIT: f64 = 4.0;

/// How much bigger than the shape the library works: the refit precision of the two calls is 0.01 and cannot be changed, which at this scale is 1E-5 (R46 asks for 1E-4).
pub(super) const SCALE: f64 = 1000.0;

/// The runs on each side of a section: the outer or left ones, the inner or right ones.
#[derive(Debug, Default, PartialEq)]
pub(super) struct Sides {
    pub first: Vec<Run>,
    pub second: Vec<Run>,
}

/// The most the library's sweep may be asked to do for the calls of one command: its own limit for one call.
const WORK: u64 = MAX_SWEEP_WORK as u64;

/// The least a call is given of the work, whatever its share: about a thousand crossings of two lines (4 ms), so that a small call among very big ones is not refused for
/// being a small part of the whole, and no share is left at nothing. The calls that have the floor are the ones under a thousandth of the segments of the command, at most
/// 2,700 of them above the 192 pieces the library does not meter below, which is at most 11 s of work in all.
const LEAST_SHARE: usize = 1 << 22;

/// The most places where the outline of the stroke round an open path may cross itself: 65,536. A path whose stroke crosses itself more than that is no path that has two sides
/// (a path that crosses itself has none, and is refused in words after its outline is made, which for a star of 2,000 vertices took 17 s), and a polyline of thirty thousand
/// sharp bends, whose stroke crosses itself twice at each, is under it. The library counts the places before it sweeps.
const OPEN_CROSSINGS: usize = 1 << 16;

/// What the library's sweep takes for one place where two lines cross: 3.5 microseconds, its README's figure. A share of the work is also a share of crossings
/// (`Limits::real_crossings`), counted before the sweep, so that a path that crosses itself in more places than the share allows is refused at once and not after the
/// dry sweep has been run up to the share.
const CROSSING_NS: usize = 3_500;

/// How the library's work for a command is shared among its calls: each call is given [`WORK`] in proportion to the segments it is given, out of the segments
/// of every call of the command together.
#[derive(Clone, Copy, Debug)]
pub(super) struct Meter {
    /// The segments of every call of the command, together: a closed section is three calls (the fill is made of its rings, then grown, then shrunk) and an open run is one.
    weight: u64,
}

impl Meter {
    /// The meter of a command whose sections are these, each a list of runs.
    pub(super) fn of<'a>(sections: impl Iterator<Item = &'a [Run]>) -> Meter {
        let mut weight = 0_u64;
        for runs in sections {
            for run in runs.iter().filter(|r| !r.segs.is_empty()) {
                let calls = if run.closed() { 3 } else { 1 };
                weight = weight.saturating_add((run.segs.len() as u64).saturating_mul(calls));
            }
        }
        Meter { weight }
    }

    /// The meter of one section alone: all of the work is its.
    #[cfg(test)]
    pub(super) fn alone(runs: &[Run]) -> Meter {
        Meter::of(std::iter::once(runs))
    }

    /// The limits for a call that is given `segments` segments: the library's defaults, with the sweep work of this call's share, the crossings that share pays for, and the
    /// pairs of segments an offset is held to (a stroke's outline and the fill of the rings are held to the same).
    fn limits(&self, segments: usize) -> Limits {
        let mut limits = Limits::default();
        let share = u128::from(WORK) * segments as u128 / u128::from(self.weight.max(1));
        let work = usize::try_from(share).unwrap_or(usize::MAX).clamp(LEAST_SHARE, MAX_SWEEP_WORK);
        limits.sweep_work = work;
        limits.real_crossings = (work / CROSSING_NS).clamp(1, limits.real_crossings);
        limits.segment_pairs = limits.offset_segment_pairs;
        limits
    }
}

/// The sides of the runs of one section, `distance` away, the library's work shared as `meter` says.
pub(super) fn sides(runs: &[Run], distance: f64, meter: &Meter) -> Result<Sides, Problem> {
    let scaled: Vec<Run> = runs.iter().filter(|r| !r.segs.is_empty()).map(|r| r.scaled(SCALE)).collect();
    let d = distance * SCALE;
    let mut out = Sides::default();
    let closed: Vec<Run> = scaled.iter().filter(|r| r.closed()).cloned().collect();
    if !closed.is_empty() {
        // The rings are one fill by the odd-crossings rule (a ring inside another is a hole, whichever way it runs, and the middle of a pentagram drawn in one ring
        // is one), and the library's offset reads the non-zero rule: the fill is made of boundaries that run the way each needs first, for one ring as for several.
        let segments = closed.iter().fold(0_usize, |sum, r| sum.saturating_add(r.segs.len()));
        let limits = meter.limits(segments);
        let ring = with_limits(limits, || try_normalize(&runs_path(&closed), FillRule::EvenOdd)).map_err(ops::refusal)?;
        let outer = with_limits(limits, || try_offset_path(&ring, d, Join::Miter, MITRE_LIMIT)).map_err(ops::refusal)?;
        let inner = with_limits(limits, || try_offset_path(&ring, -d, Join::Miter, MITRE_LIMIT)).map_err(ops::refusal)?;
        out.first.extend(kept(&outer));
        out.second.extend(kept(&inner));
    }
    for run in scaled.iter().filter(|r| !r.closed()) {
        let (left, right) = open_sides(run, d, meter)?;
        out.first.push(left);
        out.second.push(right);
    }
    for run in out.first.iter_mut().chain(out.second.iter_mut()) {
        *run = run.scaled(1.0 / SCALE);
    }
    Ok(out)
}

/// The closed runs of a result that are more than a sliver, as the library drew them.
fn kept(path: &BezPath) -> Vec<Run> {
    runs_of(path).into_iter().filter(|run| !run.closed() || run.path().area().abs() * (1.0 / (SCALE * SCALE)) >= MIN_AREA).collect()
}

/// The unit direction a path leaves its start in and arrives at its end in, `None` for a path that goes nowhere. (A run's path has lines and cubics only.)
fn end_directions(path: &BezPath) -> Option<(Vec2, Vec2)> {
    let els = path.elements();
    let start = match els.first()? {
        PathEl::MoveTo(p) => *p,
        _ => return None,
    };
    let unit = |candidates: &[Vec2]| candidates.iter().find(|v| v.hypot() > 0.0 && v.is_finite()).map(|v| *v / v.hypot());
    let first = match els.get(1)? {
        PathEl::LineTo(p) => unit(&[*p - start]),
        PathEl::CurveTo(a, b, p) => unit(&[*a - start, *b - start, *p - start]),
        _ => None,
    }?;
    let before = match els.get(els.len().checked_sub(2)?)? {
        PathEl::MoveTo(p) | PathEl::LineTo(p) | PathEl::QuadTo(_, p) | PathEl::CurveTo(_, _, p) => *p,
        PathEl::ClosePath => return None,
    };
    let last = match els.last()? {
        PathEl::LineTo(p) => unit(&[*p - before]),
        PathEl::CurveTo(a, b, p) => unit(&[*p - *b, *p - *a, *p - before]),
        _ => None,
    }?;
    Some((first, last))
}

/// The node of the ring nearest `at`, if it is within `tol` of it.
fn node_at(ring: &Loop, at: Point, tol: f64) -> Option<usize> {
    let (k, nearest) = ring.nodes.iter().enumerate().map(|(k, node)| (k, node.p.distance(at))).min_by(|a, b| a.1.total_cmp(&b.1))?;
    (nearest <= tol).then_some(k)
}

/// The edges from node `from` to node `to`, walked forward or back round the ring, as the segments of a path from one to the other. `None` when the walk meets one of
/// `avoid` first, or does not come to `to`.
fn walk(ring: &Loop, from: usize, to: usize, forward: bool, avoid: [usize; 2]) -> Option<Run> {
    let n = ring.nodes.len();
    let mut segs = Vec::new();
    let mut at = from;
    for _ in 0..n {
        if at == to {
            return Some(Run { start: ring.nodes.get(from)?.p, segs });
        }
        let next = if forward { (at + 1) % n } else { (at + n - 1) % n };
        // The edge between `at` and `next` is the one the earlier of them holds.
        let edge = ring.nodes.get(if forward { at } else { next })?.out;
        let end = ring.nodes.get(next)?.p;
        segs.push(match (edge, forward) {
            (Edge::Line, _) => Seg::Line(end),
            (Edge::Cubic(a, b), true) => Seg::Cubic(a, b, end),
            (Edge::Cubic(a, b), false) => Seg::Cubic(b, a, end),
        });
        at = next;
        if avoid.contains(&at) && at != to {
            return None;
        }
    }
    None
}

fn cannot_cut(why: &str) -> Problem {
    Problem::failed(format!(
        "Offset cannot make the two sides of a path that {why}: the outline of its stroke has no caps to cut it at (the path crosses itself, or bends so hard that its sides meet)"
    ))
}

/// The two sides of one open run, `d` away on its left and right (in the library's frame): the stroke's ring cut at its caps.
fn open_sides(run: &Run, d: f64, meter: &Meter) -> Result<(Run, Run), Problem> {
    let path = run.path();
    let mut limits = meter.limits(run.segs.len());
    limits.real_crossings = limits.real_crossings.min(OPEN_CROSSINGS);
    let ring = with_limits(limits, || try_outline_stroke(&path, 2.0 * d, Cap::Butt, Join::Miter, MITRE_LIMIT)).map_err(ops::refusal)?;
    let mut rings = rings_of(&ring);
    let (Some(ring), true) = (rings.pop(), rings.is_empty()) else { return Err(cannot_cut("crosses itself or doubles back on itself")) };
    let (first, last) = end_directions(&path).ok_or_else(|| cannot_cut("goes nowhere"))?;
    let left = |v: Vec2| Vec2::new(-v.y, v.x);
    let (start, end) = (run.start, run.end());
    let tol = (d * 1.0e-3).max(1.0e-3);
    let corner = |at: Point| node_at(&ring, at, tol);
    let (Some(a0), Some(b0), Some(a1), Some(b1)) =
        (corner(start + left(first) * d), corner(start - left(first) * d), corner(end + left(last) * d), corner(end - left(last) * d))
    else {
        return Err(cannot_cut("bends so hard that its two sides meet"));
    };
    cut_at_caps(&ring, [a0, b0, a1, b1])
}

/// The ring cut at its caps: the nodes at the left and right points of the start (`a0`, `b0`) and of the end (`a1`, `b1`). The ring runs one way round, the start cap being
/// `a0` then `b0` (an anticlockwise ring, which is what the library gives) or `b0` then `a0`, and each cap is one straight edge between the two nodes of its end. The left
/// side is the ring from `a0` to `a1` away from the caps, the right side from `b0` to `b1`.
fn cut_at_caps(ring: &Loop, [a0, b0, a1, b1]: [usize; 4]) -> Result<(Run, Run), Problem> {
    let n = ring.nodes.len();
    if [a0, b0, a1, b1].iter().enumerate().any(|(i, k)| [a0, b0, a1, b1].iter().skip(i + 1).any(|other| other == k)) {
        return Err(cannot_cut("is shorter than the distance is wide"));
    }
    // Whether the edge that leaves node `from` is a straight one.
    let straight = |from: usize| ring.nodes.get(from).is_some_and(|node| node.out == Edge::Line);
    let (left_run, right_run) = if b0 == (a0 + 1) % n {
        // A0 to B0 is the start cap: the left side is back from A0, the right forward from B0.
        if !(straight(a0) && b1 == (a1 + n - 1) % n && straight(b1)) {
            return Err(cannot_cut("is capped by anything but straight edges"));
        }
        (walk(ring, a0, a1, false, [b0, b1]), walk(ring, b0, b1, true, [a0, a1]))
    } else if a0 == (b0 + 1) % n {
        if !(straight(b0) && a1 == (b1 + n - 1) % n && straight(a1)) {
            return Err(cannot_cut("is capped by anything but straight edges"));
        }
        (walk(ring, a0, a1, true, [b0, b1]), walk(ring, b0, b1, false, [a0, a1]))
    } else {
        return Err(cannot_cut("has caps that do not meet its sides"));
    };
    match (left_run, right_run) {
        // The four nodes are four, so each walk is a step at least.
        (Some(l), Some(r)) => Ok((l, r)),
        _ => Err(cannot_cut("has sides that run into each other")),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn p(x: f64, y: f64) -> Point {
        Point::new(x, y)
    }

    /// The sides of one section's runs, all the library's work for them.
    fn sides(runs: &[Run], distance: f64) -> Result<Sides, Problem> {
        super::sides(runs, distance, &Meter::alone(runs))
    }

    fn line(a: (f64, f64), b: (f64, f64)) -> Run {
        Run { start: p(a.0, a.1), segs: vec![Seg::Line(p(b.0, b.1))] }
    }

    fn square(x0: f64, y0: f64, x1: f64, y1: f64) -> Run {
        Run { start: p(x0, y0), segs: vec![Seg::Line(p(x1, y0)), Seg::Line(p(x1, y1)), Seg::Line(p(x0, y1)), Seg::Line(p(x0, y0))] }
    }

    fn corners(run: &Run) -> Vec<(f64, f64)> {
        let mut out = vec![(run.start.x, run.start.y)];
        out.extend(run.segs.iter().map(|s| (s.end().x, s.end().y)));
        out
    }

    fn near(got: &[(f64, f64)], want: &[(f64, f64)]) -> bool {
        got.len() == want.len() && got.iter().zip(want).all(|(g, w)| (g.0 - w.0).abs() < 1e-6 && (g.1 - w.1).abs() < 1e-6)
    }

    #[test]
    fn a_line_has_a_side_on_its_left_and_one_on_its_right_each_running_the_way_it_runs() {
        // The line (3, 3) to (6, 6) at 2.
        let both = sides(&[line((3.0, 3.0), (6.0, 6.0))], 2.0).unwrap();
        assert_eq!((both.first.len(), both.second.len()), (1, 1));
        // Two along the normal of a line at 45 degrees is 1.4142... in each axis.
        let a = std::f64::consts::SQRT_2;
        assert!(near(&corners(&both.first[0]), &[(3.0 - a, 3.0 + a), (6.0 - a, 6.0 + a)]), "{:?}", corners(&both.first[0]));
        assert!(near(&corners(&both.second[0]), &[(3.0 + a, 3.0 - a), (6.0 + a, 6.0 - a)]), "{:?}", corners(&both.second[0]));
        // The other way along the line, the left is the other side.
        let back = sides(&[line((6.0, 6.0), (3.0, 3.0))], 2.0).unwrap();
        assert!(near(&corners(&back.first[0]), &[(6.0 + a, 6.0 - a), (3.0 + a, 3.0 - a)]), "{:?}", corners(&back.first[0]));
    }

    #[test]
    fn a_square_has_a_square_outside_it_and_one_inside_and_an_inner_side_that_vanishes_is_not_made() {
        // V22.
        let s = sides(&[square(0.0, 0.0, 2.0, 2.0)], 0.5).unwrap();
        assert_eq!((s.first.len(), s.second.len()), (1, 1));
        let bounds = |r: &Run| r.bounds();
        let (outer, inner) = (bounds(&s.first[0]), bounds(&s.second[0]));
        assert!((outer.x0 + 0.5).abs() < 1e-6 && (outer.y1 - 2.5).abs() < 1e-6, "{outer:?}");
        assert!((inner.x0 - 0.5).abs() < 1e-6 && (inner.x1 - 1.5).abs() < 1e-6, "{inner:?}");
        assert_eq!(s.first[0].segs.len(), 4, "sharp corners: four edges");
        let s = sides(&[square(0.0, 0.0, 2.0, 2.0)], 1.0).unwrap();
        assert_eq!((s.first.len(), s.second.len()), (1, 0), "at 1 the inner square is a point");
        let s = sides(&[square(0.0, 0.0, 2.0, 2.0)], 5.0).unwrap();
        assert_eq!((s.first.len(), s.second.len()), (1, 0), "a distance larger than the shape: the outer side only");
    }

    #[test]
    fn a_corner_is_mitred_up_to_four_times_the_distance_and_bevelled_past_it() {
        // An open path with a corner of interior angle `a`: its two sides, the one on the outside of the turn has the mitre (or the bevel).
        let corner_of = |degrees: f64| {
            let a = degrees.to_radians();
            Run { start: p(0.0, 0.0), segs: vec![Seg::Line(p(10.0, 0.0)), Seg::Line(p(10.0 - 10.0 * a.cos(), 10.0 * a.sin()))] }
        };
        let vertices_of_sides = |degrees: f64| {
            let s = sides(&[corner_of(degrees)], 0.5).unwrap();
            s.first[0].segs.len() + s.second[0].segs.len()
        };
        // Interior angle 40 degrees: tip at 0.5 / sin 20 = 1.46 from the corner, under 4 times the distance (2): mitred, so each side has two segments.
        assert_eq!(vertices_of_sides(40.0), 4);
        // 20 degrees: the tip would be at 0.5 / sin 10 = 2.88, which is past 4 times 0.5 = 2: bevelled, one more segment on the outer side.
        assert_eq!(vertices_of_sides(20.0), 5);
        // The limit is at sin (a / 2) = 1/4, about 28.96 degrees.
        assert_eq!(vertices_of_sides(28.0), 5);
        assert_eq!(vertices_of_sides(30.0), 4);
    }

    #[test]
    fn a_curve_has_sides_that_are_curves_a_distance_from_it_within_a_ten_thousandth() {
        let curve = Run { start: p(0.0, 0.0), segs: vec![Seg::Cubic(p(1.0, 2.0), p(3.0, 2.0), p(4.0, 0.0))] };
        let s = sides(std::slice::from_ref(&curve), 0.25).unwrap();
        assert_eq!((s.first.len(), s.second.len()), (1, 1));
        let spine = curve.path();
        for side in [&s.first[0], &s.second[0]] {
            assert!(side.segs.iter().any(|seg| matches!(seg, Seg::Cubic(..))), "a curve stays a curve");
            let drawn = side.path();
            for seg in drawn.segments() {
                for k in 0..=20 {
                    let at = kurbo::ParamCurve::eval(&seg, f64::from(k) / 20.0);
                    let nearest =
                        spine.segments().map(|sp| kurbo::ParamCurveNearest::nearest(&sp, at, 1e-9).distance_sq.sqrt()).fold(f64::MAX, f64::min);
                    assert!((nearest - 0.25).abs() < 1.0e-4, "{nearest} at {at:?}");
                }
            }
        }
    }

    #[test]
    fn a_path_that_crosses_itself_is_refused_in_words_for_want_of_a_ring_to_cut() {
        let eight = Run { start: p(0.0, 0.0), segs: vec![Seg::Line(p(2.0, 2.0)), Seg::Line(p(2.0, 0.0)), Seg::Line(p(0.0, 2.0))] };
        let error = sides(&[eight], 0.1).unwrap_err();
        assert!(!error.bad_params && error.message.contains("cannot make the two sides"), "{error:?}");
    }

    /// A star of `n` vertices on a circle of radius 5, each joined to the one `step` places on, as a closed run.
    fn star(n: usize, step: usize) -> Run {
        let at = |i: usize| {
            let a = std::f64::consts::TAU * ((step * i) % n) as f64 / n as f64;
            p(5.0 * a.cos(), 5.0 * a.sin())
        };
        Run { start: at(0), segs: (1..=n).map(|i| Seg::Line(at(i))).collect() }
    }

    #[test]
    fn the_librarys_work_for_a_command_is_one_limit_shared_among_its_calls_in_proportion_to_their_segments() {
        let open = |n: usize| Run { start: p(0.0, 0.0), segs: (1..=n).map(|i| Seg::Line(p(i as f64, 0.0))).collect() };
        let sweep_work = |m: &Meter, segments: usize| m.limits(segments).sweep_work;
        // One call takes all of it, whatever its size.
        let one = Meter::alone(&[open(1_000)]);
        assert_eq!(sweep_work(&one, 1_000), MAX_SWEEP_WORK);
        // Two open runs of 1,000 and 3,000 segments: a quarter and three quarters.
        let two = Meter::of([&[open(1_000)][..], &[open(3_000)][..]].into_iter());
        assert_eq!(sweep_work(&two, 1_000), MAX_SWEEP_WORK / 4);
        assert_eq!(sweep_work(&two, 3_000), MAX_SWEEP_WORK / 4 * 3);
        // A closed section is three calls (the rings together, grown, shrunk): rings of 1,002 segments in all weigh 3,006 beside an open run of 1,002, and each of the four
        // calls has a quarter.
        let ring = Run { start: p(0.0, 0.0), segs: vec![Seg::Line(p(1.0, 0.0)), Seg::Line(p(1.0, 1.0)), Seg::Line(p(0.0, 0.0))] };
        let rings: Vec<Run> = (0..334).map(|_| ring.clone()).collect();
        let mixed = Meter::of([&rings[..], &[open(1_002)][..]].into_iter());
        assert_eq!(mixed.weight, 334 * 3 * 3 + 1_002);
        assert_eq!(sweep_work(&mixed, 1_002), MAX_SWEEP_WORK / 4, "the open run's call: 1,002 of 4,008");
        assert_eq!(sweep_work(&mixed, 334 * 3), MAX_SWEEP_WORK / 4, "each of the three calls of the rings: 1,002 of 4,008");
        // The shares of all the calls come to no more than the limit of one call (up to the floor of each).
        let total: usize = [1_000_usize, 3_000].iter().map(|n| sweep_work(&two, *n)).sum();
        assert!(total <= MAX_SWEEP_WORK, "{total}");
        // A call that is nothing beside the others is given the floor, which the library does not meter below: never nothing, and never more than a call's limit.
        let tiny = Meter::of([&[open(1)][..], &[open(500_000)][..]].into_iter());
        assert_eq!(sweep_work(&tiny, 1), LEAST_SHARE);
        assert!(sweep_work(&tiny, 500_000) <= MAX_SWEEP_WORK);
        assert_eq!(sweep_work(&Meter { weight: 0 }, 7), MAX_SWEEP_WORK, "no segments at all is a call alone");
    }

    #[test]
    fn a_star_that_crosses_itself_in_very_many_places_is_refused_in_words_and_one_that_does_not_cross_is_not() {
        // A star of 1,001 vertices crosses itself in 500,000 places: it is under the library's pre-count of the pairs of segments whose bounds overlap and over what a sweep of
        // it may do, so it is refused by the dry sweep, in words. The same path open has the same crossings.
        let closed = star(1_001, 500);
        let error = sides(std::slice::from_ref(&closed), 0.25).unwrap_err();
        assert!(!error.bad_params && error.message.contains("too many path elements"), "{error:?}");
        let mut open = closed.clone();
        open.segs.pop();
        assert!(sides(&[open], 0.25).is_err());
        // A path that does not cross itself is no matter how long it is.
        assert!(sides(&[star(1_000, 1)], 0.25).is_ok(), "a polygon of 1,000 vertices crosses nowhere");
    }

    #[test]
    fn a_single_ring_that_crosses_itself_is_one_fill_by_the_odd_crossings_rule_with_its_middle_a_hole() {
        // A pentagram drawn in one ring: the pentagon in its middle is crossed twice, so by it is a hole, and the offset outwards has the star's outline and the
        // hole, which the non-zero rule would have filled (one boundary).
        let star = |k: usize| {
            let at = |i: usize| {
                let a = std::f64::consts::FRAC_PI_2 + std::f64::consts::TAU * (2 * i % 5) as f64 / 5.0;
                p(a.cos(), a.sin())
            };
            Run { start: at(0), segs: (1..=k).map(|i| Seg::Line(at(i))).collect() }
        };
        let pentagram = star(5);
        assert!(pentagram.closed());
        let sides = sides(&[pentagram], 0.02).unwrap();
        assert_eq!(
            sides.first.len(),
            2,
            "the outline and the hole in the middle: {:?}",
            sides.first.iter().map(|r| r.segs.len()).collect::<Vec<_>>()
        );
        let holes = sides.first.iter().filter(|r| r.bounds().width() < 1.0).count();
        assert_eq!(holes, 1, "one of the two is the small pentagon of the middle");
    }

    #[test]
    fn a_hairpin_whose_sides_meet_is_refused_and_one_that_is_wide_enough_is_not() {
        let hairpin = |gap: f64| Run { start: p(0.0, 0.0), segs: vec![Seg::Line(p(4.0, 0.0)), Seg::Line(p(4.0, gap)), Seg::Line(p(0.0, gap))] };
        // Refused because the caps' corners are not in the ring (worked at 1,000 times, the nearest node to one is 300 away and a corner is within half of one), and not for
        // want of caps to cut.
        let error = sides(&[hairpin(0.3)], 0.5).unwrap_err();
        assert!(!error.bad_params && error.message.contains("path that bends so hard that its two sides meet"), "{error:?}");
        let ok = sides(&[hairpin(3.0)], 0.5).unwrap();
        assert_eq!((ok.first.len(), ok.second.len()), (1, 1));
    }

    #[test]
    fn closed_runs_of_one_section_are_offset_together_so_a_hole_shrinks_and_each_open_run_is_its_own() {
        // A square with a hole: the outer side grows, and the hole shrinks (together one fill), where each ring offset alone would grow.
        let ring_with_hole = [square(0.0, 0.0, 6.0, 6.0), square(2.0, 2.0, 4.0, 4.0)];
        let s = sides(&ring_with_hole, 0.5).unwrap();
        assert_eq!(s.first.len(), 2, "the outer ring and the hole, as one fill's boundary");
        let hole = s.first.iter().map(Run::bounds).find(|b| b.width() < 3.0).unwrap();
        assert!((hole.width() - 1.0).abs() < 1e-6, "the hole is 2 across less 0.5 each side: {hole:?}");
        let open_and_closed = sides(&[square(0.0, 0.0, 2.0, 2.0), line((5.0, 0.0), (8.0, 0.0))], 0.25).unwrap();
        assert_eq!((open_and_closed.first.len(), open_and_closed.second.len()), (2, 2));
        assert!(open_and_closed.first[0].closed() && !open_and_closed.first[1].closed());
    }

    /// A ring of the nodes at these points, every edge a straight one.
    fn ring_of(points: &[(f64, f64)]) -> Loop {
        Loop { nodes: points.iter().map(|(x, y)| super::super::rows::Node { p: p(*x, *y), out: Edge::Line }).collect() }
    }

    #[test]
    fn a_ring_is_cut_at_its_caps_whichever_way_round_it_runs() {
        // A strip along the x axis from the start (0, 0) to the end (4, 0), 1 each side: A0 (0, 1), B0 (0, -1), B1 (4, -1), A1 (4, 1), with one vertex in the middle of each
        // long side. Anticlockwise: A0, B0, (2, -1), B1, A1, (2, 1): the start cap is A0 to B0.
        let anticlockwise = ring_of(&[(0.0, 1.0), (0.0, -1.0), (2.0, -1.0), (4.0, -1.0), (4.0, 1.0), (2.0, 1.0)]);
        let (left, right) = cut_at_caps(&anticlockwise, [0, 1, 4, 3]).unwrap();
        assert_eq!(corners(&left), [(0.0, 1.0), (2.0, 1.0), (4.0, 1.0)], "the left side runs the way the path runs");
        assert_eq!(corners(&right), [(0.0, -1.0), (2.0, -1.0), (4.0, -1.0)]);
        // The same strip clockwise: B0, A0 are the start cap, and the sides are walked the other way.
        let clockwise = ring_of(&[(0.0, -1.0), (0.0, 1.0), (2.0, 1.0), (4.0, 1.0), (4.0, -1.0), (2.0, -1.0)]);
        let (left, right) = cut_at_caps(&clockwise, [1, 0, 3, 4]).unwrap();
        assert_eq!(corners(&left), [(0.0, 1.0), (2.0, 1.0), (4.0, 1.0)]);
        assert_eq!(corners(&right), [(0.0, -1.0), (2.0, -1.0), (4.0, -1.0)]);
        // Where the caps are not where the ring turns (a cap node is not next to its other), or a node is two of the four, the ring is not cut.
        assert!(cut_at_caps(&anticlockwise, [0, 2, 4, 3]).is_err(), "A0 and B0 are not next to each other");
        assert!(cut_at_caps(&anticlockwise, [0, 1, 1, 3]).is_err(), "one node for two corners");
        assert!(cut_at_caps(&anticlockwise, [0, 1, 4, 2]).is_err(), "B1 and A1 are not next to each other");
        let mut curved = anticlockwise.clone();
        curved.nodes[0].out = Edge::Cubic(p(0.0, 0.3), p(0.0, -0.3));
        assert!(cut_at_caps(&curved, [0, 1, 4, 3]).is_err(), "a cap is a straight edge");
        let mut curved_end = anticlockwise.clone();
        curved_end.nodes[3].out = Edge::Cubic(p(4.2, -0.5), p(4.2, 0.5));
        assert!(cut_at_caps(&curved_end, [0, 1, 4, 3]).is_err(), "the cap at the end too");
        // The same four refusals for the ring that runs the other way: a cap that is not straight, at the start or the end, and caps that are not each other's neighbours.
        let mut curved = clockwise.clone();
        curved.nodes[0].out = Edge::Cubic(p(-0.2, -0.5), p(-0.2, 0.5));
        assert!(cut_at_caps(&curved, [1, 0, 3, 4]).is_err(), "a cap is a straight edge, clockwise");
        let mut curved_end = clockwise.clone();
        curved_end.nodes[3].out = Edge::Cubic(p(4.2, 0.5), p(4.2, -0.5));
        assert!(cut_at_caps(&curved_end, [1, 0, 3, 4]).is_err(), "the cap at the end too, clockwise");
        assert!(cut_at_caps(&clockwise, [1, 0, 3, 2]).is_err(), "B1 and A1 are not next to each other, clockwise");
        assert!(cut_at_caps(&clockwise, [1, 3, 2, 4]).is_err(), "A0 and B0 are not next to each other, clockwise");
    }

    #[test]
    fn a_walk_round_a_ring_stops_at_its_end_refuses_to_pass_the_nodes_it_must_avoid_and_may_end_on_one() {
        let ring = ring_of(&[(0.0, 0.0), (1.0, 0.0), (2.0, 0.0), (3.0, 0.0), (4.0, 0.0), (5.0, 0.0)]);
        let forward = walk(&ring, 0, 3, true, [4, 5]).unwrap();
        assert_eq!(corners(&forward), [(0.0, 0.0), (1.0, 0.0), (2.0, 0.0), (3.0, 0.0)]);
        let back = walk(&ring, 0, 4, false, [1, 2]).unwrap();
        assert_eq!(corners(&back), [(0.0, 0.0), (5.0, 0.0), (4.0, 0.0)], "back from 0 by 5 to 4");
        assert!(walk(&ring, 0, 3, true, [2, 5]).is_none(), "it passes 2");
        assert!(walk(&ring, 0, 3, false, [5, 1]).is_none(), "back from 0 it meets 5 first");
        assert!(walk(&ring, 0, 3, true, [3, 5]).is_some(), "the end may be a node that is avoided: it is where the walk comes to");
        assert_eq!(walk(&ring, 2, 2, true, [0, 1]).map(|run| run.segs.len()), Some(0), "from a node to itself is no step");
        // A curved edge is walked as its cubic, and back as the cubic turned round.
        let mut curved = ring.clone();
        curved.nodes[1].out = Edge::Cubic(p(1.2, 1.0), p(1.8, 1.0));
        assert_eq!(walk(&curved, 0, 2, true, [4, 5]).unwrap().segs[1], Seg::Cubic(p(1.2, 1.0), p(1.8, 1.0), p(2.0, 0.0)));
        assert_eq!(walk(&curved, 2, 0, false, [4, 5]).unwrap().segs[0], Seg::Cubic(p(1.8, 1.0), p(1.2, 1.0), p(1.0, 0.0)));
    }

    #[test]
    fn a_region_of_less_than_a_hundred_millionth_is_not_made_and_one_of_more_is() {
        // A square inset by 1 on a side of 2.00005 leaves a square 0.00005 across (area 2.5E-9) and on 2.0002 one of 0.0002 across (area 4E-8): R46 drops the first.
        let tiny = sides(&[square(0.0, 0.0, 2.000_05, 2.000_05)], 1.0).unwrap();
        assert_eq!((tiny.first.len(), tiny.second.len()), (1, 0), "{:?}", tiny.second);
        let small = sides(&[square(0.0, 0.0, 2.000_2, 2.000_2)], 1.0).unwrap();
        assert_eq!((small.first.len(), small.second.len()), (1, 1));
        let area = small.second[0].path().area().abs();
        assert!((area - 4.0e-8).abs() < 1e-9, "{area}");
    }

    #[test]
    fn a_run_that_goes_nowhere_has_no_sides() {
        let none = sides(&[Run { start: p(1.0, 1.0), segs: vec![] }], 0.5).unwrap();
        assert_eq!((none.first.len(), none.second.len()), (0, 0));
    }

    #[test]
    fn the_directions_a_path_leaves_and_arrives_in_are_the_tangents_and_a_zero_handle_looks_to_the_next_point() {
        let mut path = BezPath::new();
        path.move_to((0.0, 0.0));
        path.curve_to((0.0, 0.0), (3.0, 0.0), (3.0, 3.0));
        let (first, last) = end_directions(&path).unwrap();
        assert!((first.x - 1.0).abs() < 1e-12 && first.y.abs() < 1e-12, "{first:?}: the first handle is nothing, so the second point");
        assert!(last.x.abs() < 1e-12 && (last.y - 1.0).abs() < 1e-12, "{last:?}");
        let mut point = BezPath::new();
        point.move_to((1.0, 1.0));
        point.line_to((1.0, 1.0));
        assert!(end_directions(&point).is_none());
        assert!(end_directions(&BezPath::new()).is_none());
    }

    #[test]
    fn a_distance_is_in_the_runs_own_units_whatever_the_scale_the_library_works_at() {
        // The same square at a hundredth of the size and a hundred times it, offset by a share of its side.
        for k in [0.01, 1.0, 100.0] {
            let s = sides(&[square(0.0, 0.0, 2.0 * k, 2.0 * k)], 0.5 * k).unwrap();
            let outer = s.first[0].bounds();
            assert!((outer.x0 + 0.5 * k).abs() < 1e-6 * k.max(1.0), "{k}: {outer:?}");
        }
    }
}
