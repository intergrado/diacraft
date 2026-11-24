//! A route goes round the shapes it is glued to, and not through them.
//!
//! A connection point faces a way (`DirX`, `DirY`, carried through the shape's turns, flips and groups: [`super::read::ConnectionPoint`]), and a
//! route leaves the point that way. A flip or a turn can carry the point to the far side of its shape from the shape at the other end, and rules 2
//! to 6 of [`super::route`], which know the two ends and nothing about the shapes they stand on, then lay a leg across the shape, or hook over its
//! top by a standoff, which is still across it. Here the two boxes the ends stand on (the begin shape's and the end shape's,
//! them) are known:
//!
//! * a route of rules 1 to 6 that no leg of which enters a box is kept as it is;
//! * otherwise the route is the **shortest** of a family that leaves each end along its direction by a standoff and goes round the boxes by a
//!   standoff past their edges: three middle legs (across, along, across, or the reverse) whose lines are the ends' own, the middle between them, or a
//!   standoff past each box and past both ends. Length and [`route::BEND_COST`] for each bend choose; a tie goes to the first of the candidates, which
//!   is fixed. A candidate with a flaw (a leg that doubles back, a repeated vertex, two legs over each other) is no candidate;
//! * an end inside its own box (a connection point at the middle of a shape) or leaving it inwards has no box to leave: that box is no obstacle;
//! * when no candidate is clear (the shapes overlap, the ends are inside each other's boxes) the route of rules 1 to 6 stands.
//!
//! The boxes are the only obstacles. Full routing (round every shape, spread parallel legs, the other styles) replaces this and nothing else.

use super::route::{self, BEND_COST, Dir, JOG, P, STANDOFF, directions, dot, flaws, length_of, near, step, sub, tidy};

/// A shape's box on the page: its four corners, in order round it (either way).
pub type Quad = [P; 4];

/// A leg that only grazes a box, or touches its edge, is not inside it.
const INSET: f64 = 1.0e-6;

/// How far a middle leg of a way round keeps from the boxes (the specifications' `LineToNode` of a quarter-inch page is 0.125):
/// it lies on a line a [`JOG`] from a box's edge, and a line nearer than this is no candidate.
const CLEARANCE: f64 = 0.125;

/// The points of a route that stays out of the boxes: [`route::route`], or the shortest way round when that enters one.
pub fn route_clear(b: P, bdir: Option<Dir>, e: P, edir: Option<Dir>, begin_box: Option<&Quad>, end_box: Option<&Quad>) -> Vec<P> {
    let plain = route::route(b, bdir, e, edir);
    if !(b.0.is_finite() && b.1.is_finite() && e.0.is_finite() && e.1.is_finite()) || near(b, e) {
        return plain;
    }
    let (u, v) = directions(b, bdir, e, edir);
    let boxes: Vec<&Quad> = [begin_box.filter(|q| leaves(q, b, u)), end_box.filter(|q| leaves(q, e, v))].into_iter().flatten().collect();
    if boxes.is_empty() || clear(&plain, &boxes, 0.0) {
        return plain;
    }
    around(b, u, e, v, &boxes).unwrap_or(plain)
}

/// Whether an end on `quad` can leave it along `out`: it is not inside the box, and a step out is not either.
fn leaves(quad: &Quad, at: P, out: Dir) -> bool {
    !inside(quad, at, INSET) && !inside(quad, step(at, out, 1.0e-3), INSET)
}

/// Whether a leg of `points` goes through a shape the route is glued to: the boxes of the shapes at the begin `b` and the end `e` (leaving out a box its end is
/// inside of, or leaves inwards: that box is no obstacle, as in [`route_clear`]), and the route held to them as [`route_clear`] holds a route it makes.
/// A connector set to reroute on crossover (or as needed) is routed again when this is so.
pub fn crosses(points: &[P], b: P, bdir: Option<Dir>, e: P, edir: Option<Dir>, begin_box: Option<&Quad>, end_box: Option<&Quad>) -> bool {
    if points.len() < 2 || !(b.0.is_finite() && b.1.is_finite() && e.0.is_finite() && e.1.is_finite()) {
        return false;
    }
    let (u, v) = directions(b, bdir, e, edir);
    let boxes: Vec<&Quad> = [begin_box.filter(|q| leaves(q, b, u)), end_box.filter(|q| leaves(q, e, v))].into_iter().flatten().collect();
    !boxes.is_empty() && !clear(points, &boxes, 0.0)
}

/// Whether `p` is inside the box by more than `margin` (a distance).
fn inside(quad: &Quad, p: P, margin: f64) -> bool {
    let Some(sign) = winding(quad) else { return false };
    (0..4).all(|i| {
        let (a, b) = (quad[i], quad[(i + 1) % 4]);
        let edge = sub(b, a);
        let length = edge.0.hypot(edge.1);
        length > 0.0 && sign * (edge.0 * (p.1 - a.1) - edge.1 * (p.0 - a.0)) / length > margin
    })
}

/// 1 for a box whose corners go counterclockwise, -1 clockwise, `None` for one of no area.
fn winding(quad: &Quad) -> Option<f64> {
    let area: f64 = (0..4).map(|i| quad[i].0 * quad[(i + 1) % 4].1 - quad[(i + 1) % 4].0 * quad[i].1).sum();
    (area.is_finite() && area.abs() > 1.0e-12).then(|| area.signum())
}

/// Whether the segment `a` to `b` goes into the box (Cyrus and Beck's clipping against its four sides, each moved in by [`INSET`]).
#[cfg(test)]
fn enters(a: P, b: P, quad: &Quad) -> bool {
    enters_beyond(a, b, quad, 0.0)
}

/// [`enters`] for the box grown by `margin` on every side (a distance).
fn enters_beyond(a: P, b: P, quad: &Quad, margin: f64) -> bool {
    let Some(sign) = winding(quad) else { return false };
    let d = sub(b, a);
    let (mut from, mut to) = (0.0_f64, 1.0_f64);
    for i in 0..4 {
        let (p, q) = (quad[i], quad[(i + 1) % 4]);
        let edge = sub(q, p);
        let length = edge.0.hypot(edge.1);
        if length <= 0.0 {
            continue;
        }
        // The side's normal pointing in, and how far inside the side the segment starts and how fast it goes in.
        let n = (-edge.1 / length * sign, edge.0 / length * sign);
        let start = dot(n, sub(a, p)) + margin - INSET;
        let rate = dot(n, d);
        if rate.abs() < 1.0e-15 {
            if start < 0.0 {
                return false;
            }
            continue;
        }
        let at = -start / rate;
        if rate > 0.0 { from = from.max(at) } else { to = to.min(at) }
    }
    to - from > 1.0e-9
}

/// Whether no leg of the route enters any of the boxes grown by `margin`; the first and the last leg, which stand on the boxes of their ends, are held
/// to the boxes themselves.
fn clear(points: &[P], boxes: &[&Quad], margin: f64) -> bool {
    let last = points.len().saturating_sub(2);
    points.windows(2).enumerate().all(|(i, w)| {
        let margin = if i == 0 || i == last { 0.0 } else { margin };
        boxes.iter().all(|quad| !enters_beyond(w[0], w[1], quad, margin))
    })
}

/// The extent of a box: left, bottom, right, top.
fn extent(quad: &Quad) -> (f64, f64, f64, f64) {
    let xs = quad.iter().map(|p| p.0);
    let ys = quad.iter().map(|p| p.1);
    (
        xs.clone().fold(f64::INFINITY, f64::min),
        ys.clone().fold(f64::INFINITY, f64::min),
        xs.fold(f64::NEG_INFINITY, f64::max),
        ys.fold(f64::NEG_INFINITY, f64::max),
    )
}

/// The lines a middle leg may lie on, in the order they are tried: the middle between the two standoff points, each standoff point's own, a standoff
/// past each box's edges, and past both standoff points. `low` and `high` pick the box edges of one axis.
fn lines(a: f64, c: f64, boxes: &[&Quad], span: impl Fn(&Quad) -> (f64, f64)) -> Vec<f64> {
    let mut out = vec![(a + c) / 2.0, a, c];
    for quad in boxes {
        let (low, high) = span(quad);
        out.push(high + JOG);
        out.push(low - JOG);
    }
    out.push(a.max(c) + JOG);
    out.push(a.min(c) - JOG);
    let mut kept: Vec<f64> = Vec::with_capacity(out.len());
    for line in out {
        if line.is_finite() && !kept.iter().any(|k| (k - line).abs() < 1.0e-9) {
            kept.push(line);
        }
    }
    kept
}

/// The shortest clear route of the family (see the module documentation), trying the full standoff at both ends first and a half one where that
/// finds nothing.
fn around(b: P, u: Dir, e: P, v: Dir, boxes: &[&Quad]) -> Option<Vec<P>> {
    let half = STANDOFF / 2.0;
    [(STANDOFF, STANDOFF), (half, STANDOFF), (STANDOFF, half), (half, half)]
        .into_iter()
        .find_map(|(out, back)| shortest(b, u, e, v, boxes, out, back))
}

fn shortest(b: P, u: Dir, e: P, v: Dir, boxes: &[&Quad], out: f64, back: f64) -> Option<Vec<P>> {
    let (bs, es) = (step(b, u, out), step(e, v, back));
    let xs = lines(bs.0, es.0, boxes, |q| {
        let x = extent(q);
        (x.0, x.2)
    });
    let ys = lines(bs.1, es.1, boxes, |q| {
        let x = extent(q);
        (x.1, x.3)
    });
    let mut best: Option<(f64, Vec<P>)> = None;
    let mut consider = |points: Vec<P>| {
        let points = tidy(points);
        let leaves = points.get(1).is_some_and(|second| dot(sub(*second, b), u.vector()) > 0.0);
        let arrives = points.len().checked_sub(2).and_then(|i| points.get(i)).is_some_and(|before| dot(sub(e, *before), v.vector()) < 0.0);
        if !leaves || !arrives || flaws(&points) > 0 || !clear(&points, boxes, CLEARANCE) {
            return;
        }
        let price = length_of(&points) + BEND_COST * points.len().saturating_sub(2) as f64;
        if best.as_ref().is_none_or(|(least, _)| price < *least - 1.0e-12) {
            best = Some((price, points));
        }
    };
    // Across, along, across: out of `bs` to a line `y`, along it to a line `x`, and across to `es`'s height.
    for &y in &ys {
        for &x in &xs {
            consider(vec![b, bs, (bs.0, y), (x, y), (x, es.1), es, e]);
        }
    }
    // Along, across, along.
    for &x in &xs {
        for &y in &ys {
            consider(vec![b, bs, (x, bs.1), (x, y), (es.0, y), es, e]);
        }
    }
    best.map(|(_, points)| points)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A box from its lower-left corner and size, counterclockwise.
    fn rect(x: f64, y: f64, w: f64, h: f64) -> Quad {
        [(x, y), (x + w, y), (x + w, y + h), (x, y + h)]
    }

    fn leg_enters(points: &[P], quad: &Quad) -> bool {
        points.windows(2).any(|w| enters(w[0], w[1], quad))
    }

    #[test]
    fn a_segment_into_a_box_enters_it_and_one_along_its_edge_or_beside_it_does_not() {
        let q = rect(0.0, 0.0, 2.0, 1.0);
        assert!(enters((-1.0, 0.5), (3.0, 0.5), &q), "through");
        assert!(enters((1.0, 0.5), (1.0, 5.0), &q), "from inside, out");
        assert!(enters((-1.0, 0.99), (0.5, 0.99), &q), "into a corner");
        assert!(!enters((-1.0, 0.0), (3.0, 0.0), &q), "along the lower edge");
        assert!(!enters((0.0, -1.0), (0.0, 3.0), &q), "along the left edge");
        assert!(!enters((-1.0, 1.5), (3.0, 1.5), &q), "above");
        assert!(!enters((2.0, 0.5), (5.0, 0.5), &q), "leaving by the right edge, starting on it");
        assert!(!enters((-1.0, 0.5), (0.0, 0.5), &q), "ending on the left edge");
        // The same box the other way round.
        let reversed = [q[3], q[2], q[1], q[0]];
        assert!(enters((-1.0, 0.5), (3.0, 0.5), &reversed));
        assert!(!enters((-1.0, 1.5), (3.0, 1.5), &reversed));
        // A box of no area is nothing to enter.
        assert!(!enters((-1.0, 0.0), (3.0, 0.0), &[(0.0, 0.0); 4]));
    }

    #[test]
    fn a_turned_box_is_the_diamond_it_is_and_not_the_box_that_holds_it() {
        let diamond = [(1.0, 0.0), (2.0, 1.0), (1.0, 2.0), (0.0, 1.0)];
        assert!(enters((-1.0, 1.0), (1.0, 1.0), &diamond));
        // Past a corner of the holding box, outside the diamond.
        assert!(!enters((-1.0, 0.2), (3.0, 0.2), &[(0.0, 0.0), (0.4, 0.0), (0.4, 0.1), (0.0, 0.1)]));
        assert!(!enters((-1.0, 0.1), (0.5, 0.1), &diamond), "left of the slope at this height");
        assert!(enters((-1.0, 0.1), (3.0, 0.1), &diamond), "across the lower point");
        assert!(!enters((-1.0, -0.1), (3.0, -0.1), &diamond), "under the lower point");
    }

    #[test]
    fn a_route_that_enters_no_box_is_the_plain_route() {
        let (a, b) = (rect(-0.75, -0.5, 1.5, 1.0), rect(3.25, -0.5, 1.5, 1.0));
        let plain = route::route((0.75, 0.0), Some(Dir::Right), (3.25, 0.0), Some(Dir::Left));
        assert_eq!(route_clear((0.75, 0.0), Some(Dir::Right), (3.25, 0.0), Some(Dir::Left), Some(&a), Some(&b)), plain);
        // No boxes at all: the plain route for every pair of ends.
        for bd in [None, Some(Dir::Up), Some(Dir::Left)] {
            for ed in [None, Some(Dir::Right), Some(Dir::Down)] {
                assert_eq!(route_clear((0.0, 0.0), bd, (3.0, 2.0), ed, None, None), route::route((0.0, 0.0), bd, (3.0, 2.0), ed));
            }
        }
    }

    #[test]
    fn two_points_facing_out_of_the_far_side_go_round_the_shape_by_a_standoff_past_it() {
        // The begin shape's right point (0.75, 0) and the end shape's right point (4.75, 0): both face right, the end shape in the way.
        let (a, b) = (rect(-0.75, -0.5, 1.5, 1.0), rect(3.25, -0.5, 1.5, 1.0));
        let r = route_clear((0.75, 0.0), Some(Dir::Right), (4.75, 0.0), Some(Dir::Right), Some(&a), Some(&b));
        assert!(!leg_enters(&r, &a) && !leg_enters(&r, &b), "{r:?}");
        assert_eq!(r.first(), Some(&(0.75, 0.0)));
        assert_eq!(r.last(), Some(&(4.75, 0.0)));
        assert_eq!(r[1], (1.0, 0.0), "it leaves to the right: {r:?}");
        assert_eq!(r[r.len() - 2], (5.0, 0.0), "and arrives from the right: {r:?}");
        let length: f64 = r.windows(2).map(|w| (w[1].0 - w[0].0).abs() + (w[1].1 - w[0].1).abs()).sum();
        assert!((length - 6.0).abs() < 1e-9, "{length}: {r:?}");
        assert!(r.iter().any(|p| (p.1 - 0.75).abs() < 1e-9 || (p.1 + 0.75).abs() < 1e-9), "a standoff past the top or the bottom: {r:?}");
    }

    #[test]
    fn the_shorter_side_is_the_one_taken() {
        let (a, b) = (rect(-0.75, -0.3, 1.5, 1.0), rect(3.25, -0.5, 1.5, 1.0));
        // The begin is at 0.2, above the middle of the end shape: over is shorter than under.
        let over = route_clear((0.75, 0.2), Some(Dir::Right), (4.75, 0.0), Some(Dir::Right), Some(&a), Some(&b));
        assert!(over.iter().any(|p| p.1 > 0.5), "{over:?}");
        let (a, b) = (rect(-0.75, -0.7, 1.5, 1.0), rect(3.25, -0.5, 1.5, 1.0));
        let under = route_clear((0.75, -0.2), Some(Dir::Right), (4.75, 0.0), Some(Dir::Right), Some(&a), Some(&b));
        assert!(under.iter().any(|p| p.1 < -0.5), "{under:?}");
    }

    #[test]
    fn an_end_inside_its_own_box_or_leaving_it_inwards_has_no_box_to_leave() {
        let (a, b) = (rect(-0.75, -0.5, 1.5, 1.0), rect(3.25, -0.5, 1.5, 1.0));
        // A point at the middle of the begin shape: the begin box is no obstacle, the route is the plain one.
        let plain = route::route((0.0, 0.0), Some(Dir::Right), (3.25, 0.0), Some(Dir::Left));
        assert_eq!(route_clear((0.0, 0.0), Some(Dir::Right), (3.25, 0.0), Some(Dir::Left), Some(&a), Some(&b)), plain);
        // A point on the right edge that faces left, into the shape.
        let into = route::route((0.75, 0.0), Some(Dir::Left), (3.25, 0.0), Some(Dir::Left));
        assert_eq!(route_clear((0.75, 0.0), Some(Dir::Left), (3.25, 0.0), Some(Dir::Left), Some(&a), Some(&b)), into);
    }

    #[test]
    fn when_nothing_is_clear_the_plain_route_stands() {
        // The end shape holds the begin point's standoff and the shapes overlap: every candidate enters a box.
        let (a, b) = (rect(0.0, -0.5, 4.0, 1.0), rect(1.0, -0.5, 4.0, 1.0));
        let plain = route::route((4.0, 0.0), Some(Dir::Right), (5.0, 0.0), Some(Dir::Right));
        let r = route_clear((4.0, 0.0), Some(Dir::Right), (5.0, 0.0), Some(Dir::Right), Some(&a), Some(&b));
        assert!(r.len() >= 2 && r.len() <= route::MAX_POINTS);
        assert!(r == plain || (!leg_enters(&r, &a) || !leg_enters(&r, &b)), "{r:?}");
    }

    #[test]
    fn hostile_numbers_do_not_panic_or_grow() {
        let q = rect(0.0, 0.0, 1.0, 1.0);
        for bad in [f64::NAN, f64::INFINITY, -f64::INFINITY, 1.0e300, -1.0e300] {
            let r = route_clear((bad, 0.0), Some(Dir::Right), (0.0, bad), Some(Dir::Up), Some(&q), Some(&q));
            assert!(r.len() <= route::MAX_POINTS);
            let r = route_clear(
                (0.0, 0.5),
                Some(Dir::Right),
                (2.0, 0.5),
                Some(Dir::Right),
                Some(&[(bad, 0.0), (1.0, bad), (0.0, 1.0), (bad, bad)]),
                None,
            );
            assert!(r.len() <= route::MAX_POINTS);
        }
    }

    #[test]
    fn every_candidate_for_every_layout_is_flat_of_flaws_or_the_plain_route() {
        // A grid of ends round two boxes: a clear route is flawless and enters neither box, or no clear route exists and the plain one is returned.
        let (a, b) = (rect(-0.75, -0.5, 1.5, 1.0), rect(3.25, -0.5, 1.5, 1.0));
        let dirs = [Dir::Right, Dir::Up, Dir::Left, Dir::Down];
        let begins = [(0.75, 0.0), (0.0, 0.5), (-0.75, 0.0), (0.0, -0.5)];
        let ends = [(4.75, 0.0), (4.0, 0.5), (3.25, 0.0), (4.0, -0.5)];
        let mut detoured = 0;
        for (i, &bp) in begins.iter().enumerate() {
            for (j, &ep) in ends.iter().enumerate() {
                let r = route_clear(bp, Some(dirs[i]), ep, Some(dirs[j]), Some(&a), Some(&b));
                assert!((2..=route::MAX_POINTS).contains(&r.len()), "{r:?}");
                let plain = route::route(bp, Some(dirs[i]), ep, Some(dirs[j]));
                if r != plain {
                    detoured += 1;
                    assert_eq!(flaws(&r), 0, "{r:?}");
                    assert!(!leg_enters(&r, &a) && !leg_enters(&r, &b), "{r:?}");
                    assert!(dot(sub(r[1], r[0]), dirs[i].vector()) > 0.0, "it leaves along the direction: {r:?}");
                }
            }
        }
        assert!(detoured > 0);
    }

    /// A route crosses a shape it is glued to when a leg goes into the box of the shape at an end, and a route round it does not.
    #[test]
    fn a_route_crosses_a_glued_shape_when_a_leg_goes_into_its_box() {
        let (a, b) = (rect(0.0, 0.0, 2.0, 1.0), rect(5.0, 0.0, 2.0, 1.0));
        let (begin, end) = ((2.0, 0.5), (5.0, 0.5));
        let crosses_by = |points: &[P]| crosses(points, begin, Some(Dir::Right), end, Some(Dir::Left), Some(&a), Some(&b));
        assert!(!crosses_by(&[begin, end]), "a leg between the two boxes");
        assert!(!crosses_by(&[begin, (3.0, 0.5), (3.0, 3.0), (4.5, 3.0), (4.5, 0.5), end]), "round the top of both");
        assert!(crosses_by(&[begin, (3.0, 0.5), (3.0, 3.0), (6.0, 3.0), (6.0, 0.5), end]), "the last leg runs back through the second box");
        assert!(crosses_by(&[begin, (1.0, 0.5), (1.0, 3.0), (5.0, 3.0), (5.0, 0.5), end]), "the first leg goes back into the first box");
        // A box an end is inside of is no obstacle (as in `route_clear`), a route of one point or an end that is not a number has none to cross, and no box is no crossing.
        assert!(!crosses(&[begin, end], (1.0, 0.5), Some(Dir::Right), end, Some(Dir::Left), Some(&a), None));
        assert!(!crosses(&[begin], begin, None, end, None, Some(&a), Some(&b)));
        assert!(!crosses(&[begin, end], (f64::NAN, 0.5), None, end, None, Some(&a), Some(&b)));
        assert!(!crosses(&[begin, (3.0, 0.5), (3.0, 3.0), (6.0, 3.0), (6.0, 0.5), end], begin, None, end, None, None, None));
    }
}
