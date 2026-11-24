//! The graph as lists, its separate parts, and a tree taken from it.

use std::cmp::Ordering;

use crate::frame::Item;

/// Nothing: a shape with no parent.
pub(crate) const NONE: u32 = u32::MAX;

/// Lists of numbers, one for each shape, kept in one vector (so that a diagram of a hundred thousand shapes is three allocations and not a hundred thousand).
#[derive(Clone, Debug, Default)]
pub(crate) struct Csr {
    start: Vec<usize>,
    list: Vec<u32>,
}

impl Csr {
    /// The list of shape `v`: empty for one that is not there.
    pub(crate) fn row(&self, v: usize) -> &[u32] {
        match (self.start.get(v), self.start.get(v + 1)) {
            (Some(&a), Some(&b)) => self.list.get(a..b).unwrap_or(&[]),
            _ => &[],
        }
    }

    pub(crate) fn rows_mut(&mut self) -> (&[usize], &mut [u32]) {
        (&self.start, &mut self.list)
    }
}

/// The lists made from `pairs`: `(v, w)` puts `w` in the list of `v`, after the ones put there before it.
pub(crate) fn csr(n: usize, pairs: &[(u32, u32)]) -> Csr {
    let mut start = vec![0_usize; n + 1];
    for &(v, _) in pairs {
        if let Some(slot) = start.get_mut(v as usize + 1) {
            *slot += 1;
        }
    }
    for i in 0..n {
        let before = start.get(i).copied().unwrap_or(0);
        if let Some(slot) = start.get_mut(i + 1) {
            *slot += before;
        }
    }
    let mut next = start.clone();
    let mut list = vec![0_u32; pairs.len()];
    for &(v, w) in pairs {
        if let Some(at) = next.get_mut(v as usize) {
            if let Some(slot) = list.get_mut(*at) {
                *slot = w;
            }
            *at += 1;
        }
    }
    Csr { start, list }
}

/// The edges without a shape joined to itself and without one repeated, in order.
pub(crate) fn clean_edges(edges: &[(u32, u32)]) -> Vec<(u32, u32)> {
    let mut out: Vec<(u32, u32)> = edges.iter().copied().filter(|(a, b)| a != b).collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// The shapes in the order they read on the page: by their place along the flow, then across it, then their number.
pub(crate) fn reading_order(items: &[Item]) -> Vec<u32> {
    let mut order: Vec<u32> = (0..items.len() as u32).collect();
    order.sort_by(|&a, &b| {
        let (x, y) = (&items[a as usize], &items[b as usize]);
        x.m0.total_cmp(&y.m0).then(x.c0.total_cmp(&y.c0)).then(a.cmp(&b))
    });
    order
}

/// The separate parts of the graph, each a list of shape numbers in order, and the parts in the order their first shapes read on the page.
pub(crate) fn components(items: &[Item], edges: &[(u32, u32)]) -> Vec<Vec<u32>> {
    let n = items.len();
    let mut parent: Vec<u32> = (0..n as u32).collect();
    let mut size = vec![1_u32; n];
    for &(a, b) in edges {
        let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
        if ra == rb {
            continue;
        }
        let (big, small) = if size[ra as usize] >= size[rb as usize] { (ra, rb) } else { (rb, ra) };
        parent[small as usize] = big;
        size[big as usize] += size[small as usize];
    }
    // Parts in the order of the first of their shapes to read on the page.
    let mut part_of: Vec<u32> = vec![NONE; n];
    let mut parts: Vec<Vec<u32>> = Vec::new();
    for v in reading_order(items) {
        let root = find(&mut parent, v) as usize;
        if part_of[root] == NONE {
            part_of[root] = parts.len() as u32;
            parts.push(Vec::new());
        }
        parts[part_of[root] as usize].push(v);
    }
    for part in &mut parts {
        part.sort_unstable();
    }
    parts
}

/// The root of the set `v` is in, with the path to it halved on the way.
fn find(parent: &mut [u32], mut v: u32) -> u32 {
    loop {
        let p = parent[v as usize];
        if p == v {
            return v;
        }
        let up = parent[p as usize];
        parent[v as usize] = up;
        v = up;
    }
}

/// A tree (or trees) taken from a graph: each shape reached once, by the first connector that came to it.
#[derive(Clone, Debug, Default)]
pub(crate) struct Forest {
    /// The parent of each shape, [`NONE`] for a root.
    pub parent: Vec<u32>,
    /// How far each shape is below its root.
    pub depth: Vec<u32>,
    /// The children of each shape, in the order they were reached (the order of their places across the flow).
    pub kids: Csr,
    /// The roots, in reading order.
    pub roots: Vec<u32>,
    /// Every shape, each after its parent (breadth first, one root after another).
    pub order: Vec<u32>,
}

/// The tree of the shapes of `items` joined by `edges` (no shape joined to itself and none twice): a shape that no connector points at is a root, and so is, when the
/// connectors form a ring with none, the first shape of it to read on the page. Connectors are followed the way they point, to the shapes they point at in the order
/// of their places across the flow, so a connector to a shape that was reached already is not part of the tree.
pub(crate) fn forest(items: &[Item], edges: &[(u32, u32)]) -> Forest {
    forest_with(items, edges, &reading_order(items))
}

/// [`forest`] with the roots taken in the order `reading` gives (every shape once).
pub(crate) fn forest_with(items: &[Item], edges: &[(u32, u32)], reading: &[u32]) -> Forest {
    let n = items.len();
    let mut indegree = vec![0_u32; n];
    for &(_, b) in edges {
        indegree[b as usize] += 1;
    }
    let mut out = csr(n, edges);
    sort_rows(&mut out, items);
    let mut f = Forest { parent: vec![NONE; n], depth: vec![0; n], kids: Csr::default(), roots: Vec::new(), order: Vec::with_capacity(n) };
    let mut seen = vec![false; n];
    let mut head = 0;
    // The shapes nothing points at first, then, for what is left (a ring), the first of each in reading order.
    for pass in 0..2 {
        for &r in reading {
            let wanted = if pass == 0 { indegree[r as usize] == 0 } else { true };
            if !wanted || seen[r as usize] {
                continue;
            }
            seen[r as usize] = true;
            f.roots.push(r);
            f.order.push(r);
            walk_breadth(&out, &mut f, &mut seen, &mut head);
        }
    }
    let pairs: Vec<(u32, u32)> = f.order.iter().filter(|&&v| f.parent[v as usize] != NONE).map(|&v| (f.parent[v as usize], v)).collect();
    f.kids = csr(n, &pairs);
    f
}

/// Takes the shapes the queue (`f.order` from `head` on) reaches into the tree.
fn walk_breadth(out: &Csr, f: &mut Forest, seen: &mut [bool], head: &mut usize) {
    while let Some(&v) = f.order.get(*head) {
        *head += 1;
        for &w in out.row(v as usize) {
            if !seen[w as usize] {
                seen[w as usize] = true;
                f.parent[w as usize] = v;
                f.depth[w as usize] = f.depth[v as usize] + 1;
                f.order.push(w);
            }
        }
    }
}

/// Sorts each list of `out` by the places of the shapes in it: across the flow, then along it, then their numbers.
fn sort_rows(out: &mut Csr, items: &[Item]) {
    let (start, list) = out.rows_mut();
    for w in start.windows(2) {
        let (Some(&a), Some(&b)) = (w.first(), w.get(1)) else { continue };
        if let Some(row) = list.get_mut(a..b) {
            row.sort_by(|&x, &y| compare_places(items, x, y));
        }
    }
}

fn compare_places(items: &[Item], x: u32, y: u32) -> Ordering {
    let (p, q) = (&items[x as usize], &items[y as usize]);
    p.c0.total_cmp(&q.c0).then(p.m0.total_cmp(&q.m0)).then(x.cmp(&y))
}

/// The shapes of the forest in the order a walk down it meets them, each parent before its children and each child's whole subtree before the next child: roots in
/// order, children in order. The walk is made with a stack, so a chain of a hundred thousand is no deeper than a short one.
pub(crate) fn preorder(f: &Forest) -> Vec<u32> {
    let mut out = Vec::with_capacity(f.order.len());
    let mut stack: Vec<u32> = f.roots.iter().rev().copied().collect();
    while let Some(v) = stack.pop() {
        out.push(v);
        stack.extend(f.kids.row(v as usize).iter().rev().copied());
    }
    out
}
