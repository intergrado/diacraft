//! Radial and circular layouts: shapes on rings, or on one circle.
//!
//! The room a shape needs on a ring is the circle its box fits in (the diagonal of the box), with the space between shapes added. Two shapes next to each other on
//! a ring keep the centres of their circles apart by the sum of their radii and the space, so no two boxes overlap.

use std::f64::consts::{PI, TAU};

use crate::forest::{Forest, forest, preorder};
use crate::frame::Item;
use crate::model::{Error, MAX_SIZE, Work};

/// The least room a shape takes on a ring: a shape of no size (and no space) still has a place of its own, so the angles are all more than 0.
const LEAST: f64 = 1.0e-6;

fn reach(i: &Item) -> f64 {
    i.cs.hypot(i.ms)
}

/// A radial tree: the root at the middle, its children on the first ring, theirs on the next. Each shape has the arc of its ring that its subtree needs, so a large
/// branch has a wide one. A diagram with several roots has them on the first ring. → the middle of each shape, `(across, along)` in the frame of a flow down the page
/// (`y` down), about the point `(0, 0)`.
pub(crate) fn radial(items: &[Item], edges: &[(u32, u32)], gap: f64, work: &mut Work) -> Result<Vec<(f64, f64)>, Error> {
    work.charge(items.len() as u64 * 8 + edges.len() as u64)?;
    let f = forest(items, edges);
    let virtual_root = f.roots.len() > 1;
    let need = arc_needs(&f, items, gap);
    let angle = mid_angles(&f, &need, virtual_root);
    let order = preorder(&f);
    let ring = if virtual_root { 1 } else { 0 };
    let radius = ring_radii(&f, items, &order, &angle, gap, ring);
    let places: Vec<(f64, f64)> = (0..items.len())
        .map(|v| {
            let r = radius.get(f.depth[v] as usize + ring).copied().unwrap_or(0.0);
            (r * angle[v].cos(), -r * angle[v].sin())
        })
        .collect();
    if places.iter().any(|p| !(p.0.is_finite() && p.1.is_finite() && p.0.abs() <= MAX_SIZE * 1.0e3 && p.1.abs() <= MAX_SIZE * 1.0e3)) {
        return Err(Error::TooLarge("the rings would be too large to place: lay out part of the diagram".to_owned()));
    }
    Ok(places)
}

/// The room each shape's subtree needs round a ring: a leaf's own, a parent's the sum of its children's.
fn arc_needs(f: &Forest, items: &[Item], gap: f64) -> Vec<f64> {
    let mut need: Vec<f64> = items.iter().map(|i| (reach(i) + gap).max(LEAST)).collect();
    for &v in f.order.iter().rev() {
        let kids = f.kids.row(v as usize);
        if !kids.is_empty() {
            need[v as usize] = kids.iter().map(|&k| need[k as usize]).sum::<f64>().max(LEAST);
        }
    }
    need
}

/// The angle of the middle of each shape's arc: the whole circle is shared among the roots (or is the one root's), and each shape's arc among its children, each by
/// what its subtree needs.
fn mid_angles(f: &Forest, need: &[f64], virtual_root: bool) -> Vec<f64> {
    let mut lo = vec![0.0_f64; need.len()];
    let mut hi = vec![TAU; need.len()];
    if virtual_root {
        let total: f64 = f.roots.iter().map(|&r| need[r as usize]).sum();
        let mut at = 0.0;
        for &r in &f.roots {
            let span = TAU * need[r as usize] / total;
            (lo[r as usize], hi[r as usize]) = (at, at + span);
            at += span;
        }
    }
    for &v in &f.order {
        let kids = f.kids.row(v as usize);
        let total: f64 = kids.iter().map(|&k| need[k as usize]).sum();
        let (mut at, whole) = (lo[v as usize], hi[v as usize] - lo[v as usize]);
        for &k in kids {
            let span = whole * need[k as usize] / total;
            (lo[k as usize], hi[k as usize]) = (at, at + span);
            at += span;
        }
    }
    (0..need.len()).map(|v| (lo[v] + hi[v]) / 2.0).collect()
}

/// The radius of each ring (ring 0 is the root's, at the middle): at least far enough from the ring before for the shapes of both not to meet, and far enough for
/// the neighbours on the ring itself, in the order of their angles, not to meet.
fn ring_radii(f: &Forest, items: &[Item], order: &[u32], angle: &[f64], gap: f64, first: usize) -> Vec<f64> {
    let rings = f.depth.iter().max().map_or(0, |&d| d as usize + 1) + first;
    let mut thick = vec![0.0_f64; rings];
    let mut members: Vec<Vec<u32>> = vec![Vec::new(); rings];
    for &v in order {
        let d = f.depth[v as usize] as usize + first;
        thick[d] = thick[d].max(reach(&items[v as usize]));
        members[d].push(v);
    }
    let mut radius = vec![0.0_f64; rings];
    for d in 1..rings {
        let clear = radius[d - 1] + thick[d - 1] / 2.0 + thick[d] / 2.0 + gap;
        radius[d] = clear.max(neighbours_need(&members[d], items, angle, gap));
    }
    radius
}

/// The least radius at which the shapes of one ring, in the order of their angles, keep their circles apart: two neighbours at angles `a` apart have centres
/// `2 r sin(a / 2)` apart, which must be the sum of their radii and the space.
fn neighbours_need(ring: &[u32], items: &[Item], angle: &[f64], gap: f64) -> f64 {
    let mut least = 0.0_f64;
    if ring.len() < 2 {
        return least;
    }
    for (i, &v) in ring.iter().enumerate() {
        let w = ring[(i + 1) % ring.len()];
        let mut apart = angle[w as usize] - angle[v as usize];
        if i + 1 == ring.len() {
            apart += TAU;
        }
        let room = (reach(&items[v as usize]) + reach(&items[w as usize])) / 2.0 + gap;
        let s = (apart / 2.0).sin().abs();
        if room > 0.0 && s > 1.0e-12 {
            least = least.max(room / (2.0 * s));
        }
    }
    least
}

/// A circle: every shape on one, in the order a walk down the forest meets them, so shapes joined are near each other. → as [`radial`].
pub(crate) fn circular(items: &[Item], edges: &[(u32, u32)], gap: f64, work: &mut Work) -> Result<Vec<(f64, f64)>, Error> {
    work.charge(items.len() as u64 * 8 + edges.len() as u64)?;
    let f = forest(items, edges);
    let order = preorder(&f);
    let n = order.len();
    if n < 2 {
        return Ok(vec![(0.0, 0.0); items.len()]);
    }
    let room: Vec<f64> = (0..n).map(|i| (reach(&items[order[i] as usize]) + reach(&items[order[(i + 1) % n] as usize])) / 2.0 + gap).collect();
    let r = circle_radius(&room);
    if r <= 0.0 {
        return Ok(vec![(0.0, 0.0); items.len()]);
    }
    let angles: Vec<f64> = room.iter().map(|&k| 2.0 * (k / (2.0 * r)).clamp(0.0, 1.0).asin()).collect();
    let slack = (TAU - angles.iter().sum::<f64>()).max(0.0) / n as f64;
    let mut places = vec![(0.0_f64, 0.0_f64); items.len()];
    // The first shape is at the top, and the rest follow clockwise.
    let mut at = PI / 2.0;
    for (i, &v) in order.iter().enumerate() {
        places[v as usize] = (r * at.cos(), -r * at.sin());
        at -= angles[i] + slack;
    }
    if places.iter().any(|p| !(p.0.is_finite() && p.1.is_finite())) {
        return Err(Error::TooLarge("the circle would be too large to place: lay out part of the diagram".to_owned()));
    }
    Ok(places)
}

/// The radius of the smallest circle that holds the shapes in a ring, shape `i` and shape `i + 1` keeping `room[i]` between their middles: the angles
/// `2 asin(room / 2r)` between neighbours must add up to no more than a turn. They fall as the radius grows, so the radius is found by halving.
fn circle_radius(room: &[f64]) -> f64 {
    let widest = room.iter().fold(0.0_f64, |m, &k| m.max(k));
    let turn = |r: f64| -> f64 { room.iter().map(|&k| 2.0 * (k / (2.0 * r)).clamp(0.0, 1.0).asin()).sum::<f64>() };
    let mut lo = widest / 2.0;
    if lo <= 0.0 || turn(lo) <= TAU {
        return lo.max(0.0);
    }
    let mut hi = room.iter().sum::<f64>().max(lo * 2.0);
    for _ in 0..80 {
        let mid = (lo + hi) / 2.0;
        if turn(mid) <= TAU {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    hi
}
