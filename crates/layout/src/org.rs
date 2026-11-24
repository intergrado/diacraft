//! The org chart: a tree in which each manager says how its reports are set out (ours: the reference application publishes no placement
//! algorithm).
//!
//! **A chart is a tree of boxes.** Each shape owns a *block*: its own box and, below it along the flow, the blocks of its reports. A block is a rectangle and the blocks of
//! two shapes are never one in the other, so **no two boxes overlap by construction**. A manager's [`Spread`] decides how the blocks of its reports sit:
//!
//! * [`Spread::Across`]: side by side in a row under the manager, a space apart, and the manager over the middle of the row (the first and last reports' middles). The shapes
//!   of a row are on one line, each in the middle of the row's thickness, and the row under them starts after the thickest, so that the rows of one family line up;
//! * [`Spread::After`] and [`Spread::Before`]: in a column under the manager, one under the other with the space along the flow between them, the left edges (right edges for
//!   `Before`) of their shapes on one line a space to the right (left) of the manager's middle. A report may have reports of its own, set out as its own spread says, inside
//!   the report's block.
//!
//! Reports keep the order they have on the page (across the flow for a row, along it for a column); the forest is the one of the other tree styles (`forest.rs`): a shape
//! nothing points at is a root, each shape is taken once, and a connector back to a shape already reached is ignored, so a ring or a second manager is harmless. Roots are
//! set in one row. Every walk is a loop over the forest's lists in order, so a chain of a hundred thousand shapes needs no more stack than a chain of two, and each pass
//! is charged to the [`Work`] before it runs.

use std::cmp::Ordering;

use crate::forest::{Forest, forest};
use crate::frame::Item;
use crate::model::{Error, Spread, Work};

/// A block's measure and where its parts are: shape `v`'s middle is `left[v]` from the block's left edge and `right[v]` from its right edge; the block is `height[v]` along the flow;
/// each report `k` of `v` has its middle `rel_c[k]` across from `v`'s middle and its block's top `rel_top[k]` along from `v`'s block's top.
struct Blocks {
    left: Vec<f64>,
    right: Vec<f64>,
    height: Vec<f64>,
    rel_c: Vec<f64>,
    rel_top: Vec<f64>,
}

/// The middle of each shape of one part, `(across, along)`, laid out as an org chart: `spreads[v]` is how shape `v`'s reports are set out, and `gaps` the spaces across and along
/// the flow.
pub(crate) fn org(items: &[Item], edges: &[(u32, u32)], spreads: &[Spread], gaps: (f64, f64), work: &mut Work) -> Result<Vec<(f64, f64)>, Error> {
    let n = items.len();
    work.charge(n as u64 * 12 + edges.len() as u64)?;
    let (gap_c, gap_m) = gaps;
    let mut f = forest(items, edges);
    let spread = |v: usize| spreads.get(v).copied().unwrap_or_default();
    sort_columns(&mut f, items, &spread);
    let thick = row_thickness(&f, items, &spread);
    let blocks = measure(&f, items, &thick, &spread, gap_c, gap_m);
    // The roots stand in a row, their blocks' tops level.
    let mut at_c = vec![0.0_f64; n];
    let mut at_top = vec![0.0_f64; n];
    let mut x = 0.0_f64;
    for &r in &f.roots {
        at_c[r as usize] = x + blocks.left[r as usize];
        x += blocks.left[r as usize] + blocks.right[r as usize] + gap_c;
    }
    for &v in &f.order {
        for &k in f.kids.row(v as usize) {
            at_c[k as usize] = at_c[v as usize] + blocks.rel_c[k as usize];
            at_top[k as usize] = at_top[v as usize] + blocks.rel_top[k as usize];
        }
    }
    Ok((0..n).map(|v| (at_c[v], at_top[v] + thick[v] / 2.0)).collect())
}

/// The reports of a manager that hangs them are taken in the order they stand along the flow now (then across it): they are a column, and a person reads a column from the
/// top. Every other manager's reports stay in the order across the flow that the forest gave them.
fn sort_columns(f: &mut Forest, items: &[Item], spread: &dyn Fn(usize) -> Spread) {
    let (start, list) = f.kids.rows_mut();
    for (v, w) in start.windows(2).enumerate() {
        if spread(v) == Spread::Across {
            continue;
        }
        let (Some(&a), Some(&b)) = (w.first(), w.get(1)) else { continue };
        if let Some(row) = list.get_mut(a..b) {
            row.sort_by(|&x, &y| along_then_across(items, x, y));
        }
    }
}

fn along_then_across(items: &[Item], x: u32, y: u32) -> Ordering {
    let (p, q) = (&items[x as usize], &items[y as usize]);
    p.m0.total_cmp(&q.m0).then(p.c0.total_cmp(&q.c0)).then(x.cmp(&y))
}

/// How thick the row each shape stands in is along the flow: the thickest of the shapes in a row (the roots; the reports of a manager that sets them side by side), or the
/// shape's own (a report in a column).
fn row_thickness(f: &Forest, items: &[Item], spread: &dyn Fn(usize) -> Spread) -> Vec<f64> {
    let mut thick: Vec<f64> = items.iter().map(|i| i.ms).collect();
    let roots = f.roots.iter().map(|&r| items[r as usize].ms).fold(0.0_f64, f64::max);
    for &r in &f.roots {
        thick[r as usize] = roots;
    }
    for &v in &f.order {
        if spread(v as usize) != Spread::Across {
            continue;
        }
        let kids = f.kids.row(v as usize);
        let row = kids.iter().map(|&k| items[k as usize].ms).fold(0.0_f64, f64::max);
        for &k in kids {
            thick[k as usize] = row;
        }
    }
    thick
}

/// The blocks, from the leaves up: the forest's order has each shape before its reports, so its reverse has them first.
fn measure(f: &Forest, items: &[Item], thick: &[f64], spread: &dyn Fn(usize) -> Spread, gap_c: f64, gap_m: f64) -> Blocks {
    let n = items.len();
    let mut b = Blocks { left: vec![0.0; n], right: vec![0.0; n], height: vec![0.0; n], rel_c: vec![0.0; n], rel_top: vec![0.0; n] };
    for &v in f.order.iter().rev() {
        let v = v as usize;
        let (half, t) = (items[v].cs / 2.0, thick[v]);
        let kids = f.kids.row(v);
        if kids.is_empty() {
            (b.left[v], b.right[v], b.height[v]) = (half, half, t);
            continue;
        }
        let kids_top = t + gap_m;
        match spread(v) {
            Spread::Across => {
                // A row of blocks from 0; each report's middle in it.
                let mut x = 0.0_f64;
                let (mut first, mut last, mut deepest) = (0.0_f64, 0.0_f64, 0.0_f64);
                for (i, &k) in kids.iter().enumerate() {
                    let k = k as usize;
                    let middle = x + b.left[k];
                    if i == 0 {
                        first = middle;
                    }
                    last = middle;
                    b.rel_c[k] = middle;
                    b.rel_top[k] = kids_top;
                    deepest = deepest.max(b.height[k]);
                    x += b.left[k] + b.right[k] + gap_c;
                }
                let total = x - gap_c;
                let centre = (first + last) / 2.0;
                for &k in kids {
                    b.rel_c[k as usize] -= centre;
                }
                b.left[v] = centre.max(half);
                b.right[v] = (total - centre).max(half);
                b.height[v] = kids_top + deepest;
            }
            side => {
                // A column: the reports' left (or right) edges a space from the manager's middle line.
                let sign = if side == Spread::After { 1.0 } else { -1.0 };
                let (mut left, mut right) = (half, half);
                let mut y = kids_top;
                for &k in kids {
                    let k = k as usize;
                    let middle = sign * (gap_c + items[k].cs / 2.0);
                    b.rel_c[k] = middle;
                    b.rel_top[k] = y;
                    left = left.max(b.left[k] - middle);
                    right = right.max(middle + b.right[k]);
                    y += b.height[k] + gap_m;
                }
                b.left[v] = left;
                b.right[v] = right;
                b.height[v] = y - gap_m;
            }
        }
    }
    b
}
