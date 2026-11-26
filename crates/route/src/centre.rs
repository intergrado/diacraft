//! Centring and tidying (§2.7). The search's route lies on the edges of grown boxes: of equal routes it took the first line. Each interior leg whose two neighbours run the
//! same way (the middle leg of a Z, a step of a staircase: a slide does not change the route's length or its bends) is slid across to the middle of the room it has, between
//! the nearest grown boxes. A leg whose neighbours run opposite ways (a U) stays: a slide would make it longer. A slide that would enter a box, or cost more in wires, is not made.

use crate::buckets::Obstacle;
use crate::geom::{EPS, Point, Rect};
use crate::grid::{Sc, Spent};
use crate::work::{Step, Work};

/// Takes out repeated points and the points between two legs that run the same way.
pub(crate) fn tidy(points: &mut Vec<Point>) {
    let mut out: Vec<Point> = Vec::with_capacity(points.len());
    for p in points.iter() {
        if out.last().is_some_and(|q| (q.x - p.x).abs() <= EPS && (q.y - p.y).abs() <= EPS) {
            continue;
        }
        out.push(*p);
        while out.len() >= 3 {
            let n = out.len();
            let (Some(a), Some(b), Some(c)) = (out.get(n - 3).copied(), out.get(n - 2).copied(), out.get(n - 1).copied()) else { break };
            let level = (a.y - b.y).abs() <= EPS && (b.y - c.y).abs() <= EPS && (b.x - a.x) * (c.x - b.x) > 0.0;
            let upright = (a.x - b.x).abs() <= EPS && (b.x - c.x).abs() <= EPS && (b.y - a.y) * (c.y - b.y) > 0.0;
            if !(level || upright) {
                break;
            }
            out.remove(out.len() - 2);
        }
    }
    *points = out;
}

/// A box in the frame of a slide: `x` runs along the neighbouring legs and `y` across them.
#[derive(Clone, Copy)]
struct Zone {
    r: Rect,
    /// The neighbouring legs may run through it, and the middle leg may.
    pass_nb: bool,
    pass_mid: bool,
}

/// The frame of a slide: `u` along the neighbours' way `(dx, dy)`, `v` across it.
#[derive(Clone, Copy)]
struct Frame {
    dx: f64,
    dy: f64,
}

impl Frame {
    fn to(self, p: Point) -> (f64, f64) {
        (p.x * self.dx + p.y * self.dy, -p.x * self.dy + p.y * self.dx)
    }

    fn back(self, u: f64, v: f64) -> Point {
        Point::new(u * self.dx - v * self.dy, u * self.dy + v * self.dx)
    }
}

/// The legs of a route, each as its way: `None` for a leg of no length or neither level nor upright.
fn way(a: Point, b: Point) -> Option<crate::geom::Dir> {
    use crate::geom::Dir;
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    if dy.abs() <= EPS && dx.abs() > EPS {
        Some(if dx > 0.0 { Dir::Right } else { Dir::Left })
    } else if dx.abs() <= EPS && dy.abs() > EPS {
        Some(if dy > 0.0 { Dir::Up } else { Dir::Down })
    } else {
        None
    }
}

/// Slides each interior leg of the route to the middle of its room. `begin_stub` and `end_stub` are the lengths the first and last leg keep at their ends. `Err` when the
/// work ran out (the route is then left as it is).
pub(crate) fn centre(points: &mut [Point], sc: &mut Sc<'_>, hits: &[Obstacle], begin_stub: f64, end_stub: f64, work: &mut Work) -> Result<(), Spent> {
    let n = points.len();
    if n < 4 {
        return Ok(());
    }
    for j in 1..n - 2 {
        let (Some(a), Some(b), Some(c), Some(d)) =
            (points.get(j - 1).copied(), points.get(j).copied(), points.get(j + 1).copied(), points.get(j + 2).copied())
        else {
            continue;
        };
        let (Some(d0), Some(d1), Some(d2)) = (way(a, b), way(b, c), way(c, d)) else { continue };
        if d0 != d2 || d0.is_level() == d1.is_level() {
            continue;
        }
        let (vx, vy) = d0.vector();
        let f = Frame { dx: vx, dy: vy };
        // The boxes in this frame, charged before they are read.
        let charge = hits.len() as u64;
        if !work.charge_for(Step::Line, charge) {
            return Err(Spent);
        }
        let mut zones = Vec::with_capacity(hits.len());
        for o in hits {
            work.walk(Step::Line, 1);
            if sc.is_exempt(o.id) {
                continue;
            }
            let z = sc.zone(o);
            let (p, q) = (f.to(Point::new(z.x0, z.y0)), f.to(Point::new(z.x1, z.y1)));
            let (nb, mid) = if d0.is_level() { (o.pass_level, o.pass_upright) } else { (o.pass_upright, o.pass_level) };
            zones.push(Zone { r: Rect { x0: p.0.min(q.0), y0: p.1.min(q.1), x1: p.0.max(q.0), y1: p.1.max(q.1) }, pass_nb: nb, pass_mid: mid });
        }
        let ((s1, v1), (c0, _), (_, v3), (s2, _)) = (f.to(a), f.to(b), f.to(c), f.to(d));
        let from = s1 + if j == 1 { begin_stub } else { 0.0 };
        let to = s2 - if j + 2 == n - 1 { end_stub } else { 0.0 };
        let (vlo, vhi) = (v1.min(v3), v1.max(v3));
        let (mut lo, mut hi) = (from, to);
        for z in &zones {
            let r = z.r;
            if !z.pass_mid && r.y0 < vhi && r.y1 > vlo {
                if r.x1 <= c0 + EPS {
                    lo = lo.max(r.x1);
                } else if r.x0 >= c0 - EPS {
                    hi = hi.min(r.x0);
                }
            }
            if !z.pass_nb {
                if v1 > r.y0 && v1 < r.y1 && r.x0 >= c0 - EPS && r.x1 > c0 {
                    hi = hi.min(r.x0);
                }
                if v3 > r.y0 && v3 < r.y1 && r.x1 <= c0 + EPS && r.x0 < c0 {
                    lo = lo.max(r.x1);
                }
            }
        }
        if lo > hi {
            continue;
        }
        let to_c = (lo + hi) / 2.0;
        if (to_c - c0).abs() <= 1.0e-12 {
            continue;
        }
        // The slide must enter no box.
        let clear = zones.iter().all(|z| {
            let r = z.r;
            let first = z.pass_nb || !(v1 > r.y0 && v1 < r.y1 && from < r.x1 && to_c > r.x0);
            let middle = z.pass_mid || !(to_c > r.x0 && to_c < r.x1 && vlo < r.y1 && vhi > r.y0);
            let last = z.pass_nb || !(v3 > r.y0 && v3 < r.y1 && to_c < r.x1 && to > r.x0);
            first && middle && last
        });
        if !clear {
            continue;
        }
        let (nb, nc) = (f.back(to_c, v1), f.back(to_c, v3));
        // And it must cost the wires the same.
        if !sc.wires.is_empty() {
            let old = sc.wire_cost(a, b, work)? + sc.wire_cost(b, c, work)? + sc.wire_cost(c, d, work)?;
            let new = sc.wire_cost(a, nb, work)? + sc.wire_cost(nb, nc, work)? + sc.wire_cost(nc, d, work)?;
            if (new - old).abs() > 1.0e-12 {
                continue;
            }
        }
        if let Some(slot) = points.get_mut(j) {
            *slot = nb;
        }
        if let Some(slot) = points.get_mut(j + 1) {
            *slot = nc;
        }
    }
    Ok(())
}
