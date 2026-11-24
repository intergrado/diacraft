//! Where the end of an `.odg` connector glued to the whole of a shape goes, as LibreOffice puts it.
//!
//! An end of a `draw:connector` that names a shape (`draw:start-shape`, `draw:end-shape`) and no glue point is glued to the whole of the shape. LibreOffice places such an end
//! itself every time it loads the file (the ends and the path the file states for it are not read), so a drawing read from an `.odg` draws the same in both programs only if
//! DiaCraft places the end where LibreOffice will. The rule below is LibreOffice 26.8.2.2's, measured as a black box (the report: 13,272 connectors between two
//! boxes of three shapes on a grid of places, 62,200 with each pair of points forced, and turned, elliptic and custom shapes), and is **not** the VSDX rule (in
//! `glue.rs`, which follows the reference application's dynamic glue and places the end at the connection point nearest the other end): an `.odg` file is drawn by LibreOffice.
//!
//! **The places.** An end goes to one of the shape's **four standard glue points** (the middles of the sides of its frame as it is drawn, or of the box of an outline: the rows 0
//! to 3 the `.odg` reader made), never to a point the shape lists of its own. Each point leaves the shape along a page axis, the way out of its side rounded to the nearest axis
//! (at an exact diagonal, either). LibreOffice numbers them 0 up, 1 right, 2 down and 3 left, and of two equal choices takes the lower numbers, the start shape's first.
//!
//! **Standard and curved connectors** (`standard`, the default, and `curve`, which LibreOffice places alike) take the pair of points whose right-angled route is best. The route
//! of a pair is LibreOffice's: across the gap and back (a middle leg halfway between the two boxes) for points that face each other, one bend for points at right angles, round
//! both shapes for points that face the same way, and nothing simpler for the rest, which LibreOffice never took. A route is worse when it crosses a gap narrower than
//! 1.02 cm (twice [`ESCAPE`]: LibreOffice goes round instead), then when it passes within 0.5 cm of either shape ([`MARGIN`]), then by its length, a route of one bend counted
//! 1.5 cm longer ([`BEND_PENALTY`]) and one that leaves a shape by less than 0.51 cm 1.5 cm longer again ([`SHORT_PENALTY`]). Two routes across that tie (shapes whose
//! widths add up to their heights) go across the wider gap when it is wider by more than 0.3 cm ([`ACROSS_TIE`]), else up or down (LibreOffice's own choice there wavers by
//! a quarter of a centimetre; 96 % of 1,875 such ties measured go so).
//!
//! **Straight connectors** (`line`) take the pair that makes the least of two distances counted along the axes: from the start's point to the middle of the end shape's
//! box, and from the end's point to the start's point. **Three-line connectors** (`lines`) do the same from the points 0.51 cm out along their ways out ([`STUB`]).
//!
//! **One end glued to the whole of a shape, the other not**: a point glued to a glue point is where it is and leaves its shape its own way, and the whole end takes the point the
//! same rules pick for it; for a free other end a standard connector takes the nearest point (counted along the axes) that the free end lies at least 0.51 cm beyond, and the
//! straight ones the nearest point.
//!
//! Measured agreement (LibreOffice 26.8.2.2, the rule as written here run over every measured case): with both ends glued to the whole of a shape, 9,130 of 9,166 standard
//! and curved connectors (the 36 others are ties of the kind above, between two squares), 3,318 of 3,318 straight and 3,306 of 3,318 three-line ones (12 between two squares
//! near each other), 42 of 42 to turned, elliptic and custom shapes, and 1,801 of 1,875 of the ties; with the other end free, 28 of 28 of each kind; with it glued to a point,
//! 28 of 28 straight and three-line ones and 64 of 66 standard and curved ones (the two, one drawing each: a point that faces away from the shape, which LibreOffice reaches by a route
//! round the shape that no rule here makes). Lengths are page inches.

use super::route::P;

/// The least a route leaves a shape by: 0.51 cm (LibreOffice's escape of 0.5 cm and a hundredth of a millimetre past it, measured: a leg of 0.505 cm is short, a gap of 1.015 cm too narrow).
pub(crate) const ESCAPE: f64 = 0.51 / 2.54;

/// The leg a three-line connector, and a route round both shapes, leaves a point by: 0.51 cm.
pub(crate) const STUB: f64 = 0.51 / 2.54;

/// The room round a shape that a route keeps out of: 0.5 cm.
pub(crate) const MARGIN: f64 = 0.5 / 2.54;

/// What a route of one bend counts more than its length: 1.5 cm (any value above 1.0 cm and up to 2.0 cm gives the same picks on every measured pair).
pub(crate) const BEND_PENALTY: f64 = 1.5 / 2.54;

/// What a route that leaves a shape by less than [`ESCAPE`] counts more, besides: 1.5 cm (LibreOffice takes such a bend beside a point glued close to the shape, rather than a
/// route round it, and not where another bend leaves both shapes by enough: any value from 0.6 cm to 2.5 cm gives the same picks on every measured case).
pub(crate) const SHORT_PENALTY: f64 = 1.5 / 2.54;

/// How much wider the gap across must be than the gap up or down for a tie of two routes across to go across: 0.3 cm.
pub(crate) const ACROSS_TIE: f64 = 0.3 / 2.54;

/// Lengths closer than this are equal.
const EPS: f64 = 1.0e-9;

/// The kinds of connector whose whole-shape ends LibreOffice places by a rule of its own: `standard` and `curve` share one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Standard,
    Lines,
    Line,
}

/// One of the four standard glue points of a shape, on the page (y up).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Spot {
    pub at: P,
    /// LibreOffice's number for it: 0 up, 1 right, 2 down, 3 left on the page.
    pub id: u8,
    /// Its ways out, unit vectors on page axes: one, or two for a point on an exact diagonal of its shape.
    pub ways: [Option<P>; 2],
}

/// A shape an end is glued to: its box on the page (lower left, upper right) and its standard points.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Target {
    pub lo: P,
    pub hi: P,
    pub spots: Vec<Spot>,
}

/// What the other end of a connector is, when one end is glued to the whole of a shape and the other is not.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Other {
    /// Glued to a glue point: where it is, the way it leaves its shape (a page axis), and that shape's box.
    Glued { at: P, way: P, lo: P, hi: P },
    /// Not glued.
    Free(P),
}

fn dot(a: P, b: P) -> f64 {
    a.0 * b.0 + a.1 * b.1
}

fn sub(a: P, b: P) -> P {
    (a.0 - b.0, a.1 - b.1)
}

fn along(a: P, way: P, k: f64) -> P {
    (a.0 + way.0 * k, a.1 + way.1 * k)
}

fn manhattan(a: P, b: P) -> f64 {
    (a.0 - b.0).abs() + (a.1 - b.1).abs()
}

fn finite(p: P) -> bool {
    p.0.is_finite() && p.1.is_finite()
}

/// The page axis nearest a direction, and the other one too when it is as near (an exact diagonal); none for no direction.
pub(crate) fn axes_of(v: P) -> [Option<P>; 2] {
    let (ax, ay) = (v.0.abs(), v.1.abs());
    if !finite(v) || ax + ay <= EPS {
        return [None, None];
    }
    let across = (v.0.signum(), 0.0);
    let upright = (0.0, v.1.signum());
    // Halved, so that the sum of two large parts is no infinity, which made every direction of parts past half the largest number a diagonal; halving is exact, so
    // the test is the same as `|ax - ay| <= EPS (ax + ay).max(1)` wherever that sum is a number.
    if (ax - ay).abs() * 0.5 <= EPS * (ax * 0.5 + ay * 0.5).max(0.5) {
        [Some(across), Some(upright)]
    } else if ax > ay {
        [Some(across), None]
    } else {
        [Some(upright), None]
    }
}

/// LibreOffice's number for a point that leaves its shape `way`: 0 up, 1 right, 2 down, 3 left.
pub(crate) fn id_of(way: P) -> u8 {
    if way.1 > 0.5 {
        0
    } else if way.0 > 0.5 {
        1
    } else if way.1 < -0.5 {
        2
    } else {
        3
    }
}

impl Target {
    /// A shape's target from its four standard points on the page and the corners of its frame there: each point leaves along the page axis nearest the way from the middle of the
    /// four to it. The box is the box of the four points when they stand square to the page (a shape square to the page, and an outline, whose points the `.odg` reader puts on
    /// the box of what it draws), and the box of the frame's corners when they do not (a turned frame). `None` for points or corners that are not numbers.
    pub(crate) fn of(points: [P; 4], corners: [P; 4]) -> Option<Target> {
        if !points.iter().chain(corners.iter()).all(|p| finite(*p)) {
            return None;
        }
        let middle = (points.iter().map(|p| p.0).sum::<f64>() / 4.0, points.iter().map(|p| p.1).sum::<f64>() / 4.0);
        // LibreOffice numbers a point by the side of the frame it is on, before the turn: each way from the middle is turned back by the frame's turn, short of a quarter
        // turn (the frame's own axis on the page, `corners[1]` from `corners[0]`), so that a point of a frame turned 45 degrees, on an exact diagonal, has the number
        // of its side and no other point's.
        let axis = sub(corners[1], corners[0]);
        let turn = if axis.0.hypot(axis.1) > EPS { axis.1.atan2(axis.0).rem_euclid(std::f64::consts::FRAC_PI_2) } else { 0.0 };
        let (s, c) = turn.sin_cos();
        let mut spots = Vec::with_capacity(4);
        for at in points {
            let v = sub(at, middle);
            let ways = axes_of(v);
            let first = ways[0]?;
            let unturned = (v.0 * c + v.1 * s, -v.0 * s + v.1 * c);
            let id = axes_of(unturned)[0].map_or(id_of(first), id_of);
            spots.push(Spot { at, id, ways });
        }
        // Square to the page: the points that leave up and down share an x, and those that leave across share a y.
        let near = |a: f64, b: f64| (a - b).abs() <= 1.0e-6 * (1.0 + a.abs().max(b.abs()));
        let single = |upright: bool| -> Vec<&Spot> {
            spots.iter().filter(|s| s.ways[1].is_none() && s.ways[0].is_some_and(|w| (w.1.abs() > 0.5) == upright)).collect()
        };
        let (upright, across) = (single(true), single(false));
        let square = matches!((upright.as_slice(), across.as_slice()), ([a, b], [c, d]) if near(a.at.0, b.at.0) && near(c.at.1, d.at.1));
        let of = |ps: &[P]| {
            let lo = ps.iter().fold((f64::INFINITY, f64::INFINITY), |m, p| (m.0.min(p.0), m.1.min(p.1)));
            let hi = ps.iter().fold((f64::NEG_INFINITY, f64::NEG_INFINITY), |m, p| (m.0.max(p.0), m.1.max(p.1)));
            (lo, hi)
        };
        let (lo, hi) = if square { of(&points) } else { of(&corners) };
        Some(Target { lo, hi, spots })
    }

    fn middle(&self) -> P {
        ((self.lo.0 + self.hi.0) / 2.0, (self.lo.1 + self.hi.1) / 2.0)
    }
}

/// How far a box reaches along a page axis `way`.
fn reach(lo: P, hi: P, way: P) -> f64 {
    (lo.0 * way.0).max(hi.0 * way.0) + (lo.1 * way.1).max(hi.1 * way.1)
}

/// Whether the axis-aligned segment `p`-`q` touches the box `lo`-`hi` grown by `m`.
fn touches(p: P, q: P, lo: P, hi: P, m: f64) -> bool {
    p.0.min(q.0) <= hi.0 + m + EPS && p.0.max(q.0) >= lo.0 - m - EPS && p.1.min(q.1) <= hi.1 + m + EPS && p.1.max(q.1) >= lo.1 - m - EPS
}

/// A route of a standard connector and how good it is: whether it is a route across a gap too narrow for one (LibreOffice goes round instead), whether it passes near a shape,
/// its length counted with the penalties of a bend and of a leg that leaves a shape by too little, and the legs that leave the first shape and reach the second (for a tie
/// of two routes across).
#[derive(Clone, Copy, Debug)]
struct Route {
    narrow: bool,
    near: bool,
    cost: f64,
    ends: f64,
    across: Option<bool>,
}

/// LibreOffice's right-angled route from `pa` leaving shape `a` by `wa` to `pb` reached against `wb` on shape `b`, and its quality; `None` for a route that goes round in more
/// legs (never the one LibreOffice takes when a simpler one is there).
fn route(pa: P, wa: P, a: (P, P), pb: P, wb: P, b: (P, P)) -> Option<Route> {
    let d = sub(pb, pa);
    let (points, narrow, mut penalty): (Vec<P>, bool, f64) = if dot(wa, wb) < -0.5 {
        // Facing each other: out to the middle of the gap between the boxes, across, and in.
        let a_edge = reach(a.0, a.1, wa);
        let b_edge = -reach(b.0, b.1, (-wa.0, -wa.1));
        let gap = b_edge - a_edge;
        if gap <= 0.0 || dot(d, wa) <= 0.0 {
            return None;
        }
        let mid = along(pa, wa, (a_edge + b_edge) / 2.0 - dot(pa, wa));
        let lateral = sub(pb, along(pa, wa, dot(d, wa)));
        (vec![pa, mid, (mid.0 + lateral.0, mid.1 + lateral.1), pb], gap < 2.0 * ESCAPE - EPS, 0.0)
    } else if dot(wa, wb).abs() < 0.5 {
        // At right angles: one bend.
        let g = dot(d, wa);
        let corner = along(pa, wa, g);
        let h = dot(sub(corner, pb), wb);
        if g <= 0.0 || h <= 0.0 {
            return None;
        }
        let short = g < ESCAPE - EPS || h < ESCAPE - EPS;
        (vec![pa, corner, pb], false, BEND_PENALTY + if short { SHORT_PENALTY } else { 0.0 })
    } else {
        // The same way: out past both boxes, across and back.
        let s = reach(a.0, a.1, wa).max(reach(b.0, b.1, wa)) + STUB;
        (vec![pa, along(pa, wa, s - dot(pa, wa)), along(pb, wa, s - dot(pb, wa)), pb], false, 0.0)
    };
    let legs: Vec<(P, P, f64)> =
        points.windows(2).filter_map(|w| Some((*w.first()?, *w.get(1)?))).map(|(p, q)| (p, q, manhattan(p, q))).filter(|l| l.2 > EPS).collect();
    let (first, last) = (legs.first()?, legs.last()?);
    let n = legs.len();
    // A route round both shapes leaves each by its stub; one of a shape that reaches past the other's box by less is short too, and costs the same as a short bend.
    if dot(wa, wb) > 0.5 && (first.2 < ESCAPE - EPS || last.2 < ESCAPE - EPS) {
        penalty += SHORT_PENALTY;
    }
    let near = legs
        .iter()
        .enumerate()
        .any(|(i, (p, q, _))| (i != 0 && touches(*p, *q, a.0, a.1, MARGIN)) || (i + 1 != n && touches(*p, *q, b.0, b.1, MARGIN)));
    let length: f64 = legs.iter().map(|l| l.2).sum();
    let across = (dot(wa, wb) < -0.5 && n == 3).then_some(wa.0.abs() > 0.5);
    Some(Route { narrow, near, cost: length + penalty, ends: first.2 + if n > 1 { last.2 } else { 0.0 }, across })
}

/// The order of two routes: one that is not across too narrow a gap, then one that keeps away from the shapes, then the cheaper; `None` when they are as good.
fn better(x: &Route, y: &Route) -> Option<bool> {
    if x.narrow != y.narrow {
        return Some(!x.narrow);
    }
    if x.near != y.near {
        return Some(!x.near);
    }
    if (x.cost - y.cost).abs() > EPS * (1.0 + x.cost.abs()) {
        return Some(x.cost < y.cost);
    }
    None
}

/// For the Kani harnesses of the crate (`crate::kani_proofs`): [`better`] of two routes given by whether each is across too narrow a gap, whether it passes near a shape, and its cost.
#[cfg(kani)]
pub(crate) fn better_of(x: (bool, bool, f64), y: (bool, bool, f64)) -> Option<bool> {
    let route = |(narrow, near, cost): (bool, bool, f64)| Route { narrow, near, cost, ends: 0.0, across: None };
    better(&route(x), &route(y))
}

/// The best route between a spot of each of two shapes (each spot with each of its ways), as `(route, index of a's spot, index of b's spot)`.
fn best_standard(a: &Target, b: &[(Spot, (P, P))]) -> Option<(usize, usize)> {
    // Every pair's best route over the ways of its points, in LibreOffice's order of the numbers.
    let mut pairs: Vec<(Route, usize, usize, u16)> = Vec::new();
    for (i, sa) in a.spots.iter().enumerate() {
        for (j, (sb, bbox)) in b.iter().enumerate() {
            let mut best: Option<Route> = None;
            for wa in sa.ways.iter().flatten() {
                for wb in sb.ways.iter().flatten() {
                    if let Some(r) = route(sa.at, *wa, (a.lo, a.hi), sb.at, *wb, *bbox)
                        && best.as_ref().is_none_or(|old| better(&r, old) == Some(true))
                    {
                        best = Some(r);
                    }
                }
            }
            if let Some(r) = best {
                pairs.push((r, i, j, u16::from(sa.id) * 4 + u16::from(sb.id)));
            }
        }
    }
    let mut winner: Option<&(Route, usize, usize, u16)> = None;
    for p in &pairs {
        let wins = match winner {
            None => true,
            Some(w) => better(&p.0, &w.0).unwrap_or(p.3 < w.3),
        };
        if wins {
            winner = Some(p);
        }
    }
    let w = winner?;
    // A tie of a route across beside one up or down (shapes whose widths add up to their heights).
    if w.0.across.is_some() {
        let tied: Vec<&(Route, usize, usize, u16)> = pairs.iter().filter(|p| p.0.across.is_some() && better(&p.0, &w.0).is_none()).collect();
        let pick = |across: bool| tied.iter().filter(|p| p.0.across == Some(across)).min_by_key(|p| p.3).copied();
        if let (Some(h), Some(v)) = (pick(true), pick(false)) {
            let chosen = if h.0.ends > v.0.ends + ACROSS_TIE { h } else { v };
            return Some((chosen.1, chosen.2));
        }
    }
    Some((w.1, w.2))
}

/// The point of `spot` a three-line connector's middle leg starts from, for each of its ways: 0.51 cm out.
fn stubs(spot: &Spot) -> impl Iterator<Item = P> + '_ {
    spot.ways.iter().flatten().map(|w| along(spot.at, *w, STUB))
}

/// The `.odg` glue rule for a connector glued to the whole of two shapes, `a` at its start and `b` at its end: the place of each end, on the page.
pub(crate) fn pair(kind: Kind, a: &Target, b: &Target) -> Option<(P, P)> {
    let (i, j) = match kind {
        Kind::Standard => {
            let bs: Vec<(Spot, (P, P))> = b.spots.iter().map(|s| (*s, (b.lo, b.hi))).collect();
            best_standard(a, &bs)?
        }
        Kind::Line | Kind::Lines => {
            let middle = b.middle();
            let mut best: Option<(f64, u16, usize, usize)> = None;
            for (i, sa) in a.spots.iter().enumerate() {
                for (j, sb) in b.spots.iter().enumerate() {
                    let cost = if kind == Kind::Line {
                        manhattan(sa.at, middle) + manhattan(sb.at, sa.at)
                    } else {
                        stubs(sa).flat_map(|pa| stubs(sb).map(move |pb| manhattan(pa, middle) + manhattan(pb, pa))).fold(f64::INFINITY, f64::min)
                    };
                    let order = u16::from(sa.id) * 4 + u16::from(sb.id);
                    let wins = cost.is_finite()
                        && best
                            .is_none_or(|(c, o, _, _)| cost < c - EPS * (1.0 + c.abs()) || ((cost - c).abs() <= EPS * (1.0 + c.abs()) && order < o));
                    if wins {
                        best = Some((cost, order, i, j));
                    }
                }
            }
            let (_, _, i, j) = best?;
            (i, j)
        }
    };
    Some((a.spots.get(i)?.at, b.spots.get(j)?.at))
}

/// The `.odg` glue rule for a connector glued to the whole of `a` at one end and not at the other (`other`): the place of the end on `a`.
pub(crate) fn toward(kind: Kind, a: &Target, other: Other) -> Option<P> {
    let index = match (kind, other) {
        (Kind::Standard, Other::Glued { at, way, lo, hi }) => {
            let spot = Spot { at, id: id_of(way), ways: [Some(way), None] };
            best_standard(a, &[(spot, (lo, hi))])?.0
        }
        (Kind::Standard, Other::Free(p)) => nearest(a, |s| {
            let beyond = s.ways.iter().flatten().any(|w| dot(sub(p, s.at), *w) >= ESCAPE - EPS);
            (u8::from(!beyond), manhattan(s.at, p))
        })?,
        (Kind::Line, Other::Glued { at: p, .. } | Other::Free(p)) => nearest(a, |s| (0, manhattan(s.at, p)))?,
        (Kind::Lines, Other::Glued { at, way, .. }) => {
            let p = along(at, way, STUB);
            nearest(a, |s| (0, stubs(s).map(|q| manhattan(q, p)).fold(f64::INFINITY, f64::min)))?
        }
        (Kind::Lines, Other::Free(p)) => nearest(a, |s| (0, stubs(s).map(|q| manhattan(q, p)).fold(f64::INFINITY, f64::min)))?,
    };
    Some(a.spots.get(index)?.at)
}

/// The spot of `a` with the least `(class, cost)`, the lower number of equals winning.
fn nearest(a: &Target, cost: impl Fn(&Spot) -> (u8, f64)) -> Option<usize> {
    let mut best: Option<(u8, f64, u8, usize)> = None;
    for (i, s) in a.spots.iter().enumerate() {
        let (class, c) = cost(s);
        if !c.is_finite() {
            continue;
        }
        let wins = best.is_none_or(|(bc, b, id, _)| {
            class < bc || (class == bc && (c < b - EPS * (1.0 + b.abs()) || ((c - b).abs() <= EPS * (1.0 + b.abs()) && s.id < id)))
        });
        if wins {
            best = Some((class, c, s.id, i));
        }
    }
    best.map(|(_, _, _, i)| i)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    /// A direction of two large parts leaves by the axis of the larger alone, as a short one does (the two parts' sum passed the largest number, so the tolerance was an
    /// infinity and every such direction was taken for a diagonal); an exact diagonal of large parts still leaves by both.
    #[test]
    fn a_long_direction_leaves_by_the_axis_of_its_larger_part_alone() {
        assert_eq!(axes_of((1.7e308, 0.2e308)), [Some((1.0, 0.0)), None]);
        assert_eq!(axes_of((-1.0e308, 1.5e308)), [Some((0.0, 1.0)), None]);
        assert_eq!(axes_of((1.7e308, -1.7e308)), [Some((1.0, 0.0)), Some((0.0, -1.0))]);
        assert_eq!(axes_of((1.7, 0.2)), [Some((1.0, 0.0)), None]);
    }

    /// A box `w` by `h` centimetres whose middle is at (`x`, `y`) centimetres, **y down** as LibreOffice's page is (the measurements' frame), as a target on the engine's page
    /// (inches, y up from a page 29.7 cm high).
    fn boxed(x: f64, y: f64, w: f64, h: f64) -> Target {
        let page = |px: f64, py: f64| (px / 2.54, (29.7 - py) / 2.54);
        let points = [page(x, y - h / 2.0), page(x + w / 2.0, y), page(x, y + h / 2.0), page(x - w / 2.0, y)];
        let corners =
            [page(x - w / 2.0, y + h / 2.0), page(x + w / 2.0, y + h / 2.0), page(x + w / 2.0, y - h / 2.0), page(x - w / 2.0, y - h / 2.0)];
        Target::of(points, corners).unwrap()
    }

    /// A box `w` by `h` centimetres turned `deg` degrees as LibreOffice's `rotate` turns it (counterclockwise as seen), its middle at (`x`, `y`) centimetres, y down.
    fn turned(x: f64, y: f64, w: f64, h: f64, deg: f64) -> Target {
        let (s, c) = deg.to_radians().sin_cos();
        let page = |u: f64, v: f64| {
            let (px, py) = (x + u * c + v * s, y - u * s + v * c);
            (px / 2.54, (29.7 - py) / 2.54)
        };
        let points = [page(0.0, -h / 2.0), page(w / 2.0, 0.0), page(0.0, h / 2.0), page(-w / 2.0, 0.0)];
        let corners = [page(-w / 2.0, h / 2.0), page(w / 2.0, h / 2.0), page(w / 2.0, -h / 2.0), page(-w / 2.0, -h / 2.0)];
        Target::of(points, corners).unwrap()
    }

    /// The side (T, R, B or L, as LibreOffice numbers 0 to 3) of `t` that `p` is.
    fn side(t: &Target, p: P) -> char {
        let s = t.spots.iter().find(|s| (s.at.0 - p.0).hypot(s.at.1 - p.1) < 1.0e-9).expect("a spot");
        ['T', 'R', 'B', 'L'][usize::from(s.id)]
    }

    fn sides(kind: Kind, a: &Target, b: &Target) -> String {
        let (pa, pb) = pair(kind, a, b).unwrap();
        format!("{}{}", side(a, pa), side(b, pb))
    }

    /// The picks LibreOffice 26.8.2.2 made for a box A 4 cm by 2 cm at (0, 0) and B 3 cm by 2 cm at the offset (cm, y down), with both ends glued to the whole of each
    /// (the grid, `lo/g1.jsonl`): rows of offset, standard, lines, line.
    const WIDE: [((f64, f64), &str, &str, &str); 14] = [
        ((7.0, 0.0), "RL", "RL", "RL"),
        ((-7.0, 0.0), "LR", "LR", "LR"),
        ((0.0, 6.0), "BT", "BT", "BT"),
        ((0.0, -6.0), "TB", "TB", "TB"),
        ((7.0, 3.0), "RL", "RL", "RL"),
        ((5.0, 4.0), "RL", "RL", "RL"),
        ((4.0, 4.0), "BT", "RT", "RL"),
        ((3.0, 5.0), "BT", "RT", "RT"),
        ((2.0, 4.0), "BT", "BL", "RT"),
        ((4.0, 2.0), "RT", "RT", "RL"),
        ((3.5, 2.5), "RT", "RT", "RL"),
        ((-3.0, -5.0), "TB", "LB", "LB"),
        ((6.0, 6.0), "RL", "RL", "RL"),
        ((-4.0, 0.0), "TT", "LR", "LR"),
    ];

    #[test]
    fn the_pick_of_each_kind_is_libreoffices_for_wide_boxes() {
        let a = boxed(0.0, 0.0, 4.0, 2.0);
        for ((dx, dy), standard, lines, line) in WIDE {
            let b = boxed(dx, dy, 3.0, 2.0);
            assert_eq!(sides(Kind::Standard, &a, &b), standard, "standard at ({dx}, {dy})");
            assert_eq!(sides(Kind::Lines, &a, &b), lines, "lines at ({dx}, {dy})");
            assert_eq!(sides(Kind::Line, &a, &b), line, "line at ({dx}, {dy})");
        }
    }

    /// Tall boxes go up and down where wide ones go across: the route across is longer by the difference of the sizes.
    #[test]
    fn tall_boxes_go_up_and_down_where_wide_ones_go_across() {
        let a = boxed(0.0, 0.0, 2.0, 4.0);
        assert_eq!(sides(Kind::Standard, &a, &boxed(9.0, -8.0, 2.0, 3.0)), "TB");
        assert_eq!(sides(Kind::Standard, &a, &boxed(3.5, 0.0, 2.0, 3.0)), "RL");
        assert_eq!(sides(Kind::Standard, &a, &boxed(3.0, 0.0, 2.0, 3.0)), "TT", "a gap of 1 cm is less than the 1.02 cm a route across needs");
    }

    /// The thresholds LibreOffice keeps (`lo/thr`): a gap across of 1.015 cm is too short, 1.02 cm is not; a leg of one bend of 0.505 cm is too short, 0.51 cm is not.
    #[test]
    fn a_route_leaves_a_shape_by_at_least_0_51_cm() {
        let a = boxed(0.0, 0.0, 4.0, 2.0);
        assert_eq!(sides(Kind::Standard, &a, &boxed(3.5 + 1.015, 6.0, 3.0, 2.0)), "BT");
        assert_eq!(sides(Kind::Standard, &a, &boxed(3.5 + 1.02, 6.0, 3.0, 2.0)), "RL");
        assert_eq!(sides(Kind::Standard, &a, &boxed(2.0 + 0.505, 2.5, 3.0, 2.0)), "BL");
        assert_eq!(sides(Kind::Standard, &a, &boxed(2.0 + 0.51, 2.5, 3.0, 2.0)), "RT");
    }

    /// Two squares: the routes across and up or down are as long; LibreOffice goes across where the gap across is the wider by more than about a quarter of a centimetre.
    #[test]
    fn a_tie_goes_across_only_for_a_gap_wider_by_more_than_0_3_cm() {
        let a = boxed(0.0, 0.0, 2.0, 2.0);
        assert_eq!(sides(Kind::Standard, &a, &boxed(8.0, -8.0, 2.0, 2.0)), "TB");
        assert_eq!(sides(Kind::Standard, &a, &boxed(8.5, -8.0, 2.0, 2.0)), "RL");
        assert_eq!(sides(Kind::Standard, &a, &boxed(-6.5, -6.0, 2.0, 2.0)), "LR");
        assert_eq!(sides(Kind::Standard, &a, &boxed(-6.0, -6.0, 2.0, 2.0)), "TB");
    }

    /// A box turned 30 and 45 degrees (`lo/kinds`): its points are the middles of its turned sides, each leaving along the nearest page axis (at 45 degrees, either).
    #[test]
    fn a_turned_box_leaves_by_the_page_axis_nearest_each_side() {
        let b = |dx: f64, dy: f64| boxed(dx, dy, 3.0, 2.0);
        let t30 = turned(0.0, 0.0, 4.0, 2.0, 30.0);
        for ((dx, dy), want) in
            [((7.0, 0.0), "RL"), ((0.0, 6.0), "BT"), ((5.0, 4.0), "BT"), ((-5.0, -4.0), "TB"), ((2.0, 5.0), "BT"), ((6.0, -2.0), "RL")]
        {
            assert_eq!(sides(Kind::Standard, &t30, &b(dx, dy)), want, "30 degrees, ({dx}, {dy})");
        }
        let t45 = turned(0.0, 0.0, 4.0, 2.0, 45.0);
        for ((dx, dy), want) in
            [((7.0, 0.0), "RL"), ((0.0, 6.0), "BT"), ((5.0, 4.0), "BL"), ((4.0, 4.0), "BL"), ((-5.0, -4.0), "TR"), ((6.0, -2.0), "RL")]
        {
            assert_eq!(sides(Kind::Standard, &t45, &b(dx, dy)), want, "45 degrees, ({dx}, {dy})");
        }
    }

    /// One end glued to the whole of A and the other free at the offset (`lo/in1`, the `free` set): a standard connector takes the nearest point the free end lies 0.51 cm
    /// beyond, a straight one the nearest point.
    #[test]
    fn with_the_other_end_free() {
        let a = boxed(0.0, 0.0, 4.0, 2.0);
        let at = |dx: f64, dy: f64| Other::Free((dx / 2.54, (29.7 - dy) / 2.54));
        let side_of = |kind: Kind, dx: f64, dy: f64| side(&a, toward(kind, &a, at(dx, dy)).unwrap());
        assert_eq!(side_of(Kind::Standard, 3.0, 5.0), 'R');
        assert_eq!(side_of(Kind::Standard, 2.0, 4.0), 'B', "the free end is not beyond the right side");
        assert_eq!(side_of(Kind::Line, 2.0, 4.0), 'R');
        assert_eq!(side_of(Kind::Lines, 2.0, 4.0), 'B');
        assert_eq!(side_of(Kind::Standard, -1.0, -6.0), 'T');
    }

    /// Hostile numbers make no target, and a target with nothing to go to gives no place.
    #[test]
    fn numbers_that_are_not_numbers_make_no_target() {
        let p = (0.0, 0.0);
        assert!(Target::of([(f64::NAN, 0.0), p, p, p], [p; 4]).is_none());
        assert!(Target::of([p, p, p, p], [p; 4]).is_none(), "four points at one place have no ways out");
        let empty = Target { lo: p, hi: p, spots: Vec::new() };
        assert!(pair(Kind::Standard, &empty, &boxed(0.0, 0.0, 1.0, 1.0)).is_none());
        assert!(toward(Kind::Line, &empty, Other::Free(p)).is_none());
    }
}
