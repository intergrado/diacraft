//! Trees: a hierarchy (a parent over its children) and a compact tree (an outline).
//!
//! Both take a spanning forest of the graph (`forest::forest`), so a diagram that is not a tree is laid out by the connectors that reach each shape first, and the
//! others are only drawn between the places (the engine routes them again). Every walk is a loop over the forest's lists, so a chain of a hundred thousand shapes
//! needs no more stack than a chain of two.

use crate::forest::{Forest, forest, forest_with, preorder};
use crate::frame::Item;
use crate::model::{Align, Error, Node, Side, Work, size};

/// A hierarchy in the frame: each subtree has the width its own children need (or its own, if that is more), a parent is over its children as `align` says, and a
/// layer is as thick as its thickest shape. → the middle of each shape, `(across, along)`.
pub(crate) fn hierarchy(items: &[Item], edges: &[(u32, u32)], align: Align, gaps: (f64, f64), work: &mut Work) -> Result<Vec<(f64, f64)>, Error> {
    let (gap_c, gap_m) = gaps;
    work.charge(items.len() as u64 * 6 + edges.len() as u64)?;
    let f = forest(items, edges);
    let width = subtree_widths(&f, items, gap_c);
    let start = span_starts(&f, &width, gap_c);
    let mut c = vec![0.0_f64; items.len()];
    for &v in f.order.iter().rev() {
        c[v as usize] = parent_place(&f, items, &c, &width, &start, v, align);
    }
    let along = layer_places(&f, items, gap_m);
    Ok((0..items.len()).map(|v| (c[v], along[v])).collect())
}

/// The width of the strip each shape's subtree takes across the flow: the shape's own, or its children's strips with the space between them, if that is more.
fn subtree_widths(f: &Forest, items: &[Item], gap: f64) -> Vec<f64> {
    let mut width: Vec<f64> = items.iter().map(|i| i.cs).collect();
    for &v in f.order.iter().rev() {
        let kids = f.kids.row(v as usize);
        if kids.is_empty() {
            continue;
        }
        let total: f64 = kids.iter().map(|&k| width[k as usize]).sum::<f64>() + gap * (kids.len() - 1) as f64;
        width[v as usize] = width[v as usize].max(total);
    }
    width
}

/// Where each shape's strip begins: the roots' strips one after another, and a parent's children's one after another across its strip, centred in it.
fn span_starts(f: &Forest, width: &[f64], gap: f64) -> Vec<f64> {
    let mut start = vec![0.0_f64; width.len()];
    let mut at = 0.0;
    for &r in &f.roots {
        start[r as usize] = at;
        at += width[r as usize] + gap;
    }
    for &v in &f.order {
        let kids = f.kids.row(v as usize);
        if kids.is_empty() {
            continue;
        }
        let total: f64 = kids.iter().map(|&k| width[k as usize]).sum::<f64>() + gap * (kids.len() - 1) as f64;
        let mut at = start[v as usize] + (width[v as usize] - total) / 2.0;
        for &k in kids {
            start[k as usize] = at;
            at += width[k as usize] + gap;
        }
    }
    start
}

/// The middle of shape `v` across the flow, given those of its children: in its strip if it has none, else over its children as `align` says, kept inside its strip.
fn parent_place(f: &Forest, items: &[Item], c: &[f64], width: &[f64], start: &[f64], v: u32, align: Align) -> f64 {
    let (s, w, own) = (start[v as usize], width[v as usize], items[v as usize].cs);
    let kids = f.kids.row(v as usize);
    let (Some(&first), Some(&last)) = (kids.first(), kids.last()) else { return s + w / 2.0 };
    let wanted = match align {
        Align::Start => c[first as usize],
        Align::Center => (c[first as usize] + c[last as usize]) / 2.0,
        Align::End => c[last as usize],
    };
    wanted.clamp(s + own / 2.0, (s + w - own / 2.0).max(s + own / 2.0))
}

/// The middle of each shape along the flow: a layer (a depth) is as thick as its thickest shape, with the space between, and its shapes are centred on its line.
fn layer_places(f: &Forest, items: &[Item], gap: f64) -> Vec<f64> {
    let layers = f.depth.iter().max().map_or(0, |&d| d as usize + 1);
    let mut thick = vec![0.0_f64; layers];
    for (v, item) in items.iter().enumerate() {
        let d = f.depth[v] as usize;
        thick[d] = thick[d].max(item.ms);
    }
    let mut middle = vec![0.0_f64; layers];
    let mut top = 0.0;
    for d in 0..layers {
        middle[d] = top + thick[d] / 2.0;
        top += thick[d] + gap;
    }
    (0..items.len()).map(|v| middle[f.depth[v] as usize]).collect()
}

// ---- the compact tree --------------------------------------------------------------------------------------------------

/// A compact tree, which is an outline: the shapes of the whole diagram one after another along `run` in the order a walk down the forest meets them, and each
/// level set off from the one before along `level`. The places are as the page has them (`y` up). → the middle of each shape on the page.
pub(crate) fn compact(
    nodes: &[Node],
    edges: &[(u32, u32)],
    run: Side,
    level: Side,
    gaps: (f64, f64),
    work: &mut Work,
) -> Result<Vec<(f64, f64)>, Error> {
    work.charge(nodes.len() as u64 * 6 + edges.len() as u64)?;
    let (ux, uy) = unit(run);
    let (vx, vy) = unit(level);
    let (gap_x, gap_y) = gaps;
    let (gap_u, gap_v) = if run.is_vertical() { (gap_y, gap_x) } else { (gap_x, gap_y) };
    // Each shape in the outline's own frame: its size along the run and along the levels, and where it is now along each.
    let items: Vec<Item> = nodes
        .iter()
        .map(|n| {
            let (w, h) = (size(n.width), size(n.height));
            let (su, sv) = if run.is_vertical() { (h, w) } else { (w, h) };
            // The place along the levels is no part of the order of a parent's children: they keep their numbers where their places along the run tie.
            Item { cs: su, ms: sv, c0: n.x * ux + n.y * uy, m0: 0.0 }
        })
        .collect();
    // The roots are met in the order of their places along the run.
    let mut reading: Vec<u32> = (0..items.len() as u32).collect();
    reading.sort_by(|&a, &b| {
        let (p, q) = (&items[a as usize], &items[b as usize]);
        p.c0.total_cmp(&q.c0).then(p.m0.total_cmp(&q.m0)).then(a.cmp(&b))
    });
    let f = forest_with(&items, edges, &reading);
    let layers = f.depth.iter().max().map_or(0, |&d| d as usize + 1);
    let mut thick = vec![0.0_f64; layers];
    for (v, item) in items.iter().enumerate() {
        let d = f.depth[v] as usize;
        thick[d] = thick[d].max(item.ms);
    }
    let mut level_at = vec![0.0_f64; layers];
    let mut at = 0.0;
    for d in 0..layers {
        level_at[d] = at + thick[d] / 2.0;
        at += thick[d] + gap_v;
    }
    let mut places = vec![(0.0_f64, 0.0_f64); items.len()];
    let mut next = 0.0;
    for v in preorder(&f) {
        let s = items[v as usize].cs;
        let (u, lv) = (next + s / 2.0, level_at[f.depth[v as usize] as usize]);
        places[v as usize] = (u * ux + lv * vx, u * uy + lv * vy);
        next += s + gap_u;
    }
    Ok(places)
}

/// The way a side of the page points, `y` up.
fn unit(side: Side) -> (f64, f64) {
    match side {
        Side::Up => (0.0, 1.0),
        Side::Down => (0.0, -1.0),
        Side::Left => (-1.0, 0.0),
        Side::Right => (1.0, 0.0),
    }
}
