//! The right-angle route of a connector: **the one place the routing rule is named**.
//!
//! (DiaCraft's own right-angle rule; a right-angle style exists and leaves its
//! legs to the application, and says a direction is a lean and not a rule). Given the two ends and the way a connector leaves
//! each (a connection point's direction, snapped to the nearest axis; none for a free end):
//!
//! 1. A free end is given a direction from the geometry. If the other end's direction points toward it, it faces back along the same
//!    axis; otherwise it leaves sideways, on the side of the other end. Two free ends run along the longer of the two distances.
//! 2. Two ends that face each other with the end ahead take a **Z**: out, across at the middle between them, and in. Aligned, that is
//!    one straight leg.
//! 3. Two ends at right angles whose corner is ahead of both take an **L**: out along one axis to the other's line.
//! 4. Two ends that leave the same way take a **C**: out past both by the standoff and back in.
//! 5. Anything else (an end behind the other, a corner behind) goes out of both ends by the standoff first and bridges between them: a
//!    detour of up to six points.
//!
//! 6. **A route is never degenerate**. Rules 1 to 5 can give a route that doubles back on itself or runs over its own legs when
//!    the ends line up (two ends at one height that face each other from behind, two that leave the same way at one height, a corner
//!    that lands on a standoff point): a leg that goes out and the next that comes straight back along it, drawn as one line with a
//!    second arrowhead on it. Such a route is replaced by the best of a few bridges between the two standoff points: a middle leg across
//!    at the middle, at an end's own line, or one standoff beyond both. The best has no flaw, then fewest bends, then the shortest. Ends
//!    that meet are one point twice, a connector of no length: the only route with a repeated vertex, since no leg is left to point an
//!    arrowhead along.
//!
//! Legs are horizontal or vertical, never slanted. Points that repeat or lie on the line between their neighbours are dropped. Full routing
//! (avoiding shapes, spreading parallel legs, the other route styles) replaces this function and nothing else.

/// A point, in page coordinates (drawing units).
pub type P = (f64, f64);

/// How far a connector goes straight out of an end before it turns, where it has to turn at once otherwise (drawing units).
pub const STANDOFF: f64 = 0.25;

/// Coordinates closer than this are the same.
const EPS: f64 = 1.0e-9;

/// The most points a route has.
pub const MAX_POINTS: usize = 8;

/// The way a connector leaves an end, along an axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Right,
    Up,
    Left,
    Down,
}

impl Dir {
    /// The axis direction nearest to the vector `(dx, dy)`: along x when the two are equally long. `None` for a vector of no length or one
    /// that is not a number.
    pub fn from_vector(dx: f64, dy: f64) -> Option<Dir> {
        if !(dx.is_finite() && dy.is_finite()) || (dx.abs() < EPS && dy.abs() < EPS) {
            return None;
        }
        Some(if dx.abs() >= dy.abs() {
            if dx > 0.0 { Dir::Right } else { Dir::Left }
        } else if dy > 0.0 {
            Dir::Up
        } else {
            Dir::Down
        })
    }

    pub(super) fn horizontal(self) -> bool {
        matches!(self, Dir::Right | Dir::Left)
    }

    pub(super) fn opposite(self) -> Dir {
        match self {
            Dir::Right => Dir::Left,
            Dir::Left => Dir::Right,
            Dir::Up => Dir::Down,
            Dir::Down => Dir::Up,
        }
    }

    pub(super) fn vector(self) -> P {
        match self {
            Dir::Right => (1.0, 0.0),
            Dir::Left => (-1.0, 0.0),
            Dir::Up => (0.0, 1.0),
            Dir::Down => (0.0, -1.0),
        }
    }
}

pub(super) fn dot(a: P, b: P) -> f64 {
    a.0 * b.0 + a.1 * b.1
}

pub(super) fn sub(a: P, b: P) -> P {
    (a.0 - b.0, a.1 - b.1)
}

pub(super) fn step(p: P, d: Dir, by: f64) -> P {
    let v = d.vector();
    (p.0 + v.0 * by, p.1 + v.1 * by)
}

/// The route from `b` (leaving along `bdir`) to `e` (leaving along `edir`, so arriving the other way): the points in order, first `b`
/// and last `e`, at most [`MAX_POINTS`]. Non-finite input gives the two ends joined.
pub fn route(b: P, bdir: Option<Dir>, e: P, edir: Option<Dir>) -> Vec<P> {
    if !(b.0.is_finite() && b.1.is_finite() && e.0.is_finite() && e.1.is_finite()) {
        return vec![b, e];
    }
    // Rule 6: ends that meet are a connector of no length.
    if near(b, e) {
        return vec![b, e];
    }
    let (u, v) = directions(b, bdir, e, edir);
    let mut points = tidy(legs(b, u, e, v));
    if flaws(&points) > 0 {
        let bridged = detour(b, u, e, v);
        if flaws(&bridged) < flaws(&points) {
            points = bridged;
        }
    }
    // Points that were dropped as repeats may differ from the ends by rounding: the route starts and ends exactly on them.
    if let Some(first) = points.first_mut() {
        *first = b;
    }
    if let Some(last) = points.last_mut() {
        *last = e;
    }
    points
}

/// Both directions, the free ones filled in (rule 1).
pub(super) fn directions(b: P, bdir: Option<Dir>, e: P, edir: Option<Dir>) -> (Dir, Dir) {
    let d = sub(e, b);
    match (bdir, edir) {
        (Some(u), Some(v)) => (u, v),
        (Some(u), None) => (u, if dot(d, u.vector()) > EPS { u.opposite() } else { sideways(u, sub(b, e)) }),
        (None, Some(v)) => (if dot(sub(b, e), v.vector()) > EPS { v.opposite() } else { sideways(v, d) }, v),
        (None, None) => {
            let along = Dir::from_vector(d.0, d.1).unwrap_or(Dir::Right);
            (along, along.opposite())
        }
    }
}

/// The direction across `along`'s axis that points the way `toward` does (up or right when it points neither).
fn sideways(along: Dir, toward: P) -> Dir {
    if along.horizontal() {
        if toward.1 >= 0.0 { Dir::Up } else { Dir::Down }
    } else if toward.0 >= 0.0 {
        Dir::Right
    } else {
        Dir::Left
    }
}

/// The corners between the ends, by the rules 2 to 5.
fn legs(b: P, u: Dir, e: P, v: Dir) -> Vec<P> {
    if u == v {
        return c_shape(b, u, e);
    }
    if u == v.opposite() {
        return if dot(sub(e, b), u.vector()) >= -EPS { z_shape(b, u, e) } else { behind_facing(b, u, e, v) };
    }
    // At right angles: the corner is on the line of `b` along `u` and the line of `e` along `v`.
    let corner = if u.horizontal() { (e.0, b.1) } else { (b.0, e.1) };
    if dot(sub(corner, b), u.vector()) >= -EPS && dot(sub(corner, e), v.vector()) >= -EPS { vec![b, corner, e] } else { behind_corner(b, u, e, v) }
}

/// Out, across at the middle, in (rule 2).
fn z_shape(b: P, u: Dir, e: P) -> Vec<P> {
    if u.horizontal() {
        let mx = (b.0 + e.0) / 2.0;
        vec![b, (mx, b.1), (mx, e.1), e]
    } else {
        let my = (b.1 + e.1) / 2.0;
        vec![b, (b.0, my), (e.0, my), e]
    }
}

/// Out past both ends and back in (rule 4).
fn c_shape(b: P, u: Dir, e: P) -> Vec<P> {
    let v = u.vector();
    let reach = dot(b, v).max(dot(e, v)) + STANDOFF;
    if u.horizontal() {
        let x = reach * v.0;
        vec![b, (x, b.1), (x, e.1), e]
    } else {
        let y = reach * v.1;
        vec![b, (b.0, y), (e.0, y), e]
    }
}

/// Ends that face each other with `e` behind `b`: out of both by the standoff, and across at the middle of the other axis (rule 5).
fn behind_facing(b: P, u: Dir, e: P, v: Dir) -> Vec<P> {
    let (bs, es) = (step(b, u, STANDOFF), step(e, v, STANDOFF));
    if u.horizontal() {
        let my = (b.1 + e.1) / 2.0;
        vec![b, bs, (bs.0, my), (es.0, my), es, e]
    } else {
        let mx = (b.0 + e.0) / 2.0;
        vec![b, bs, (mx, bs.1), (mx, es.1), es, e]
    }
}

/// Ends at right angles whose corner is behind one of them: out of both by the standoff and across (rule 5).
fn behind_corner(b: P, u: Dir, e: P, v: Dir) -> Vec<P> {
    let (bs, es) = (step(b, u, STANDOFF), step(e, v, STANDOFF));
    if u.horizontal() { vec![b, bs, (bs.0, es.1), es, e] } else { vec![b, bs, (es.0, bs.1), es, e] }
}

/// How far a middle leg of a bridge goes past both standoff points when it has to go around (rule 6).
pub(super) const JOG: f64 = STANDOFF;

/// What a bend costs in a bridge, in the units of length: fewer bends win among bridges of about one length.
pub(super) const BEND_COST: f64 = 0.5;

pub(super) fn near(a: P, b: P) -> bool {
    (a.0 - b.0).abs() < EPS && (a.1 - b.1).abs() < EPS
}

/// How many things are wrong with a route: a zero-length leg, a leg that doubles back on the one before it, and a leg that runs over another
/// on the same line. A route with none is clean.
pub(super) fn flaws(points: &[P]) -> usize {
    let mut count = points.windows(2).filter(|w| near(w[0], w[1])).count();
    count += points
        .windows(3)
        .filter(|w| {
            let (a, b) = (sub(w[1], w[0]), sub(w[2], w[1]));
            (a.0 * b.1 - a.1 * b.0).abs() < EPS && dot(a, b) < 0.0
        })
        .count();
    let legs: Vec<(P, P)> = points.windows(2).map(|w| (w[0], w[1])).collect();
    for (i, a) in legs.iter().enumerate() {
        for b in legs.iter().skip(i + 2) {
            if runs_over(*a, *b) {
                count += 1;
            }
        }
    }
    count
}

/// Whether two legs lie on one horizontal or one vertical line and share a stretch of it.
fn runs_over(a: (P, P), b: (P, P)) -> bool {
    let shared = |lo1: f64, hi1: f64, lo2: f64, hi2: f64| lo1.max(lo2) < hi1.min(hi2) - EPS;
    let flat = |l: (P, P)| (l.0.1 - l.1.1).abs() < EPS;
    let upright = |l: (P, P)| (l.0.0 - l.1.0).abs() < EPS;
    if flat(a) && flat(b) && (a.0.1 - b.0.1).abs() < EPS {
        return shared(a.0.0.min(a.1.0), a.0.0.max(a.1.0), b.0.0.min(b.1.0), b.0.0.max(b.1.0));
    }
    if upright(a) && upright(b) && (a.0.0 - b.0.0).abs() < EPS {
        return shared(a.0.1.min(a.1.1), a.0.1.max(a.1.1), b.0.1.min(b.1.1), b.0.1.max(b.1.1));
    }
    false
}

pub(super) fn length_of(points: &[P]) -> f64 {
    points.windows(2).map(|w| (w[0].0 - w[1].0).abs() + (w[0].1 - w[1].1).abs()).sum()
}

/// Rule 6: out of both ends by a standoff, and a bridge between the two standoff points that is a middle leg across, along a vertical line (an
/// `x`) or a horizontal one (a `y`) chosen from the middle, each standoff point's own and one [`JOG`] past both. A standoff is
/// [`STANDOFF`], or shorter where two ends are so close that the full one would run over the other's (half the gap between them in `x` or
/// in `y`), or none. The best has the fewest flaws (a route that does not leave along an end's direction counts one, any other flaw two),
/// then the least of length and [`BEND_COST`] for each bend.
fn detour(b: P, u: Dir, e: P, v: Dir) -> Vec<P> {
    let across = |a: f64, c: f64| [(a + c) / 2.0, a, c, a.max(c) + JOG, a.min(c) - JOG];
    let standoffs: Vec<f64> = [STANDOFF, (b.0 - e.0).abs() / 2.0, (b.1 - e.1).abs() / 2.0, 0.0]
        .into_iter()
        .filter(|s| s.is_finite() && *s >= 0.0 && *s <= STANDOFF)
        .collect();
    let leaves = |points: &[P]| match (points.first(), points.get(1), points.last(), points.len().checked_sub(2).and_then(|i| points.get(i))) {
        (Some(&a), Some(&a2), Some(&z), Some(&z1)) => dot(sub(a2, a), u.vector()) > 0.0 && dot(sub(z, z1), v.vector()) < 0.0,
        _ => false,
    };
    let mut best: Option<(usize, f64, Vec<P>)> = None;
    let mut consider = |points: Vec<P>| {
        let flawed = 2 * flaws(&points) + usize::from(!leaves(&points));
        let price = length_of(&points) + BEND_COST * points.len().saturating_sub(2) as f64;
        if best.as_ref().is_none_or(|(f, c, _)| flawed < *f || (flawed == *f && price < *c)) {
            best = Some((flawed, price, points));
        }
    };
    for &out in &standoffs {
        for &back in &standoffs {
            let (bs, es) = (step(b, u, out), step(e, v, back));
            for x in across(bs.0, es.0) {
                consider(tidy(vec![b, bs, (x, bs.1), (x, es.1), es, e]));
            }
            for y in across(bs.1, es.1) {
                consider(tidy(vec![b, bs, (bs.0, y), (es.0, y), es, e]));
            }
        }
    }
    best.map_or_else(|| vec![b, e], |(_, _, points)| points)
}

/// Drops repeated points and points on the line between their neighbours, and keeps at most [`MAX_POINTS`].
pub(super) fn tidy(points: Vec<P>) -> Vec<P> {
    let mut out = tidy_all(points);
    out.truncate(MAX_POINTS);
    out
}

/// [`tidy`] without the limit on the points: for a route read from rows, or edited by hand.
pub(super) fn tidy_all(points: Vec<P>) -> Vec<P> {
    let mut out: Vec<P> = Vec::with_capacity(points.len());
    for p in points {
        if out.last().is_some_and(|last| (last.0 - p.0).abs() < EPS && (last.1 - p.1).abs() < EPS) {
            continue;
        }
        while let [.., a, b] = out[..] {
            let (ab, bp) = (sub(b, a), sub(p, b));
            if (ab.0 * bp.1 - ab.1 * bp.0).abs() < EPS && dot(ab, bp) > 0.0 {
                out.pop();
            } else {
                break;
            }
        }
        out.push(p);
    }
    // A route whose ends meet is still two points: a connector of no length.
    if out.len() == 1 {
        let only = out[0];
        out.push(only);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: &[P], b: &[P]) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(p, q)| (p.0 - q.0).abs() < 1e-9 && (p.1 - q.1).abs() < 1e-9)
    }

    #[test]
    fn facing_ends_on_a_line_take_one_straight_leg() {
        let r = route((0.0, 0.0), Some(Dir::Right), (4.0, 0.0), Some(Dir::Left));
        assert!(near(&r, &[(0.0, 0.0), (4.0, 0.0)]), "{r:?}");
    }

    #[test]
    fn facing_ends_offset_take_a_z_with_its_cross_leg_in_the_middle() {
        let r = route((0.0, 0.0), Some(Dir::Right), (4.0, 2.0), Some(Dir::Left));
        assert!(near(&r, &[(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (4.0, 2.0)]), "{r:?}");
        let up = route((0.0, 0.0), Some(Dir::Up), (1.0, 5.0), Some(Dir::Down));
        assert!(near(&up, &[(0.0, 0.0), (0.0, 2.5), (1.0, 2.5), (1.0, 5.0)]), "{up:?}");
    }

    #[test]
    fn ends_at_right_angles_take_an_l_when_the_corner_is_ahead_of_both() {
        let r = route((0.0, 0.0), Some(Dir::Right), (3.0, 2.0), Some(Dir::Down));
        assert!(near(&r, &[(0.0, 0.0), (3.0, 0.0), (3.0, 2.0)]), "{r:?}");
    }

    #[test]
    fn a_corner_behind_one_end_takes_a_detour_that_leaves_by_the_standoff() {
        let r = route((0.0, 0.0), Some(Dir::Right), (-2.0, 2.0), Some(Dir::Up));
        assert!(near(&r, &[(0.0, 0.0), (0.25, 0.0), (0.25, 2.25), (-2.0, 2.25), (-2.0, 2.0)]), "{r:?}");
    }

    #[test]
    fn ends_that_leave_the_same_way_take_a_c() {
        let r = route((0.0, 0.0), Some(Dir::Right), (0.0, 2.0), Some(Dir::Right));
        assert!(near(&r, &[(0.0, 0.0), (0.25, 0.0), (0.25, 2.0), (0.0, 2.0)]), "{r:?}");
    }

    #[test]
    fn an_end_behind_a_facing_end_takes_an_s_through_the_middle_height() {
        let r = route((0.0, 0.0), Some(Dir::Right), (-2.0, 1.0), Some(Dir::Left));
        assert!(near(&r, &[(0.0, 0.0), (0.25, 0.0), (0.25, 0.5), (-2.25, 0.5), (-2.25, 1.0), (-2.0, 1.0)]), "{r:?}");
    }

    #[test]
    fn a_free_end_arrives_facing_the_other_when_it_is_ahead() {
        let r = route((0.0, 0.0), Some(Dir::Right), (3.0, 2.0), None);
        assert!(near(&r, &[(0.0, 0.0), (1.5, 0.0), (1.5, 2.0), (3.0, 2.0)]), "{r:?}");
        let both = route((0.0, 0.0), None, (3.0, 2.0), None);
        assert!(near(&both, &[(0.0, 0.0), (1.5, 0.0), (1.5, 2.0), (3.0, 2.0)]), "{both:?}");
        let tall = route((0.0, 0.0), None, (1.0, 5.0), None);
        assert!(near(&tall, &[(0.0, 0.0), (0.0, 2.5), (1.0, 2.5), (1.0, 5.0)]), "{tall:?}");
    }

    #[test]
    fn the_ends_meeting_is_a_connector_of_no_length() {
        let r = route((1.0, 1.0), Some(Dir::Right), (1.0, 1.0), Some(Dir::Left));
        assert!(near(&r, &[(1.0, 1.0), (1.0, 1.0)]), "{r:?}");
    }

    #[test]
    fn a_direction_is_the_nearest_axis() {
        assert_eq!(Dir::from_vector(0.9, 0.3), Some(Dir::Right));
        assert_eq!(Dir::from_vector(-0.2, 0.7), Some(Dir::Up));
        assert_eq!(Dir::from_vector(-1.0, -1.0), Some(Dir::Left), "a tie goes to x");
        assert_eq!(Dir::from_vector(0.0, 0.0), None);
        assert_eq!(Dir::from_vector(f64::NAN, 1.0), None);
    }

    #[test]
    fn every_route_is_made_of_horizontal_and_vertical_legs_between_the_ends() {
        let dirs = [None, Some(Dir::Right), Some(Dir::Up), Some(Dir::Left), Some(Dir::Down)];
        let coords = [-3.0, -0.5, 0.0, 0.25, 1.0, 2.5];
        for bd in dirs {
            for ed in dirs {
                for &bx in &coords {
                    for &by in &coords {
                        for &ex in &coords {
                            for &ey in &coords {
                                let r = route((bx, by), bd, (ex, ey), ed);
                                assert!((2..=MAX_POINTS).contains(&r.len()), "{bd:?} {ed:?}: {r:?}");
                                assert_eq!(r.first(), Some(&(bx, by)));
                                assert_eq!(r.last(), Some(&(ex, ey)));
                                for w in r.windows(2) {
                                    let straight = (w[0].0 - w[1].0).abs() < 1e-9 || (w[0].1 - w[1].1).abs() < 1e-9;
                                    assert!(straight, "a slanted leg in {bd:?} {ed:?} ({bx},{by}) to ({ex},{ey}): {r:?}");
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// What is wrong with a route, if anything: a repeated vertex or a zero-length leg, a leg that doubles back on the one before it, or two
    /// legs on one line that overlap.
    fn flaw(points: &[P]) -> Option<String> {
        let len = |a: P, b: P| ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt();
        for w in points.windows(2) {
            if len(w[0], w[1]) < 1e-9 {
                return Some(format!("a zero-length leg at {:?}", w[0]));
            }
        }
        for w in points.windows(3) {
            let (a, b) = (sub(w[1], w[0]), sub(w[2], w[1]));
            if (a.0 * b.1 - a.1 * b.0).abs() < 1e-9 && dot(a, b) < 0.0 {
                return Some(format!("doubles back at {:?}", w[1]));
            }
        }
        let legs: Vec<(P, P)> = points.windows(2).map(|w| (w[0], w[1])).collect();
        for (i, a) in legs.iter().enumerate() {
            for b in legs.iter().skip(i + 2) {
                let overlap = |lo1: f64, hi1: f64, lo2: f64, hi2: f64| lo1.max(lo2) < hi1.min(hi2) - 1e-9;
                let same_y = (a.0.1 - a.1.1).abs() < 1e-9 && (b.0.1 - b.1.1).abs() < 1e-9 && (a.0.1 - b.0.1).abs() < 1e-9;
                let same_x = (a.0.0 - a.1.0).abs() < 1e-9 && (b.0.0 - b.1.0).abs() < 1e-9 && (a.0.0 - b.0.0).abs() < 1e-9;
                if same_y && overlap(a.0.0.min(a.1.0), a.0.0.max(a.1.0), b.0.0.min(b.1.0), b.0.0.max(b.1.0)) {
                    return Some(format!("overlapping legs on y {}", a.0.1));
                }
                if same_x && overlap(a.0.1.min(a.1.1), a.0.1.max(a.1.1), b.0.1.min(b.1.1), b.0.1.max(b.1.1)) {
                    return Some(format!("overlapping legs on x {}", a.0.0));
                }
            }
        }
        None
    }

    #[test]
    fn no_route_repeats_a_vertex_doubles_back_or_runs_over_itself() {
        let dirs = [None, Some(Dir::Right), Some(Dir::Up), Some(Dir::Left), Some(Dir::Down)];
        let mut failures: Vec<String> = Vec::new();
        let mut total = 0_u32;
        // Quarter steps, so that the standoff of 0.25 lands exactly on the other end's coordinates and legs line up; eighths, so that ends are
        // closer than a standoff; tenths and twentieths, which land on nothing.
        for step in [0.25, 0.125, 0.1, 0.05] {
            let coords: Vec<f64> = (-4..=6).map(|i| f64::from(i) * step).collect();
            for bd in dirs {
                for ed in dirs {
                    for &bx in &coords {
                        for &by in &coords {
                            for &ex in &coords {
                                for &ey in &coords {
                                    if bx == ex && by == ey {
                                        continue;
                                    }
                                    total += 1;
                                    let r = route((bx, by), bd, (ex, ey), ed);
                                    if let Some(why) = flaw(&r) {
                                        failures.push(format!("{bd:?} ({bx},{by}) -> {ed:?} ({ex},{ey}): {why}: {r:?}"));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        let shown: Vec<String> = failures.iter().take(12).cloned().collect();
        assert!(failures.is_empty(), "{} of {total} routes are flawed, for example:\n{}", failures.len(), shown.join("\n"));
    }

    #[test]
    fn a_detour_around_ends_that_line_up_leaves_and_arrives_along_the_ends_directions() {
        // Two right edges at one height, one ahead of the other: the route that went out and straight back is a hook above them.
        let r = route((0.0, 0.0), Some(Dir::Right), (2.0, 0.0), Some(Dir::Right));
        assert!(flaw(&r).is_none(), "{r:?}");
        assert_eq!(r.first(), Some(&(0.0, 0.0)));
        assert_eq!(r.last(), Some(&(2.0, 0.0)));
        assert!(r[1].0 > 0.0 && r[1].1 == 0.0, "it leaves to the right: {r:?}");
        let n = r.len();
        assert!(r[n - 2].0 > 2.0 && r[n - 2].1 == 0.0, "and arrives from the right: {r:?}");
        // Facing ends at one height, the target behind: an S with a jog, not a line that goes out and back through both.
        let s = route((0.0, 0.0), Some(Dir::Right), (-2.0, 0.0), Some(Dir::Left));
        assert!(flaw(&s).is_none(), "{s:?}");
        assert_eq!((s[1], s[s.len() - 2]), ((0.25, 0.0), (-2.25, 0.0)));
        // Ends that meet are a connector of no length, the one route with a repeated vertex.
        assert_eq!(route((1.0, 1.0), Some(Dir::Right), (1.0, 1.0), Some(Dir::Right)), vec![(1.0, 1.0), (1.0, 1.0)]);
    }

    #[test]
    fn hostile_numbers_do_not_panic_or_grow() {
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1.0e300, -1.0e300] {
            let r = route((bad, 0.0), Some(Dir::Right), (0.0, bad), Some(Dir::Up));
            assert!(r.len() <= MAX_POINTS);
        }
    }
}
