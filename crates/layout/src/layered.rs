//! The flowchart: shapes in layers along the flow, connectors pointing from a layer to a later one.
//!
//! The steps are the usual ones for a layered drawing, each with its own bound (nothing recurses and nothing is searched):
//!
//! 1. **Cycles are broken** by one walk down the graph, which turns each connector it meets pointing back at a shape still being walked (so a ring of three has one
//!    connector turned, and two shapes joined both ways are one connector).
//! 2. **Layers** are the longest path to each shape from a shape nothing points at; a shape nothing points at then moves down to the layer before its nearest.
//! 3. A connector over `k` layers is a chain of `k - 1` stand-in points, so that its way is ordered with the rest. Their number is charged before any is made
//!    ([`MAX_DUMMIES`]).
//! 4. **Order in the layers** starts from where the shapes are now and is made again by sweeps down and up the layers (each shape to the mean place of its neighbours
//!    in the layer before or after), and the order with the fewest crossings is kept.
//! 5. **Place across** (`place::place_row`): each layer, in turn down and up, goes as near the mean place of its neighbours' as the order and the spaces allow.
//! 6. **Place along**: a layer is as thick as its thickest shape, with the space between layers.

use crate::forest::{Csr, csr, reading_order};
use crate::frame::Item;
use crate::model::{Error, MAX_DUMMIES, Work};
use crate::place::place_row;

/// How many sweeps and passes a diagram of this many shapes and connectors gets: many for a small one, few for the largest.
fn rounds(size: usize) -> usize {
    (4_000_000 / (size as u64 + 1)).clamp(2, 24) as usize
}

/// Shapes and stand-ins, with the layer of each and the links from one layer to the next.
struct Skeleton {
    /// The layer of each point: the shapes first, then the stand-ins.
    layer: Vec<u32>,
    /// The size across the flow and the place across now: 0 and an in-between place for a stand-in.
    cs: Vec<f64>,
    c0: Vec<f64>,
    real: usize,
    /// `(upper, lower, weight)`: a link from a point to one in the next layer. A straight run is worth more the more of it is stand-in.
    links: Vec<(u32, u32, f64)>,
    layers: usize,
    /// The links of each point to the layer above it (the numbers of `links`) and to the one below.
    up: Csr,
    down: Csr,
}

/// Where each shape goes in the frame: `(across, along)` of its middle.
pub(crate) fn flowchart(items: &[Item], edges: &[(u32, u32)], gaps: (f64, f64), work: &mut Work) -> Result<Vec<(f64, f64)>, Error> {
    let (gap_c, gap_m) = gaps;
    let rounds = rounds(items.len() + edges.len());
    work.charge((items.len() + edges.len()) as u64 * 4)?;
    let dag = acyclic(items, edges);
    let layer = layers_of(items.len(), &dag);
    let sk = skeleton(items, &dag, &layer, work)?;
    work.charge((sk.layer.len() + sk.links.len()) as u64 * (rounds as u64 * 4 + 8))?;
    let mut order = Order::new(&sk);
    order.sweep(&sk, rounds);
    let x = across(&sk, &order, gap_c, rounds);
    let along = along(items, &sk, gap_m);
    Ok((0..items.len()).map(|v| (x[v], along[v])).collect())
}

// ---- cycles and layers -------------------------------------------------------------------------------------------------

/// The connectors, each turned if it pointed back along the walk, with none twice. A walk starts at the shapes nothing points at, in reading order, and then at
/// those not reached.
fn acyclic(items: &[Item], edges: &[(u32, u32)]) -> Vec<(u32, u32)> {
    let n = items.len();
    let ids: Vec<(u32, u32)> = edges.iter().enumerate().map(|(i, &(a, _))| (a, i as u32)).collect();
    let out = csr(n, &ids);
    let mut indegree = vec![0_u32; n];
    for &(_, b) in edges {
        indegree[b as usize] += 1;
    }
    let (mut color, mut back) = (vec![0_u8; n], vec![false; edges.len()]);
    let reading = reading_order(items);
    let mut stack: Vec<(u32, usize)> = Vec::new();
    for pass in 0..2 {
        for &s in &reading {
            if color[s as usize] == 0 && (pass == 1 || indegree[s as usize] == 0) {
                walk(s, &out, edges, &mut color, &mut back, &mut stack);
            }
        }
    }
    let mut dag: Vec<(u32, u32)> = edges.iter().zip(&back).map(|(&(a, b), &turned)| if turned { (b, a) } else { (a, b) }).collect();
    dag.sort_unstable();
    dag.dedup();
    dag
}

/// One walk down from `s`: a connector to a shape that is still being walked (grey) is marked as pointing back.
fn walk(s: u32, out: &Csr, edges: &[(u32, u32)], color: &mut [u8], back: &mut [bool], stack: &mut Vec<(u32, usize)>) {
    color[s as usize] = 1;
    stack.push((s, 0));
    while let Some(top) = stack.last_mut() {
        let v = top.0;
        let row = out.row(v as usize);
        let Some(&e) = row.get(top.1) else {
            color[v as usize] = 2;
            stack.pop();
            continue;
        };
        top.1 += 1;
        let w = edges[e as usize].1;
        match color[w as usize] {
            0 => {
                color[w as usize] = 1;
                stack.push((w, 0));
            }
            1 => back[e as usize] = true,
            _ => {}
        }
    }
}

/// The layer of each shape of a graph with no cycle: the longest path to it from a shape nothing points at; then each of those shapes (a source) is brought down to
/// the layer before its nearest successor, so that its connectors are no longer than they need be.
fn layers_of(n: usize, dag: &[(u32, u32)]) -> Vec<u32> {
    let out = csr(n, dag);
    let mut indegree = vec![0_u32; n];
    for &(_, b) in dag {
        indegree[b as usize] += 1;
    }
    let sources: Vec<bool> = indegree.iter().map(|&d| d == 0).collect();
    let mut layer = vec![0_u32; n];
    let mut topo: Vec<u32> = (0..n as u32).filter(|&v| indegree[v as usize] == 0).collect();
    let mut head = 0;
    while let Some(&v) = topo.get(head) {
        head += 1;
        for &w in out.row(v as usize) {
            layer[w as usize] = layer[w as usize].max(layer[v as usize] + 1);
            indegree[w as usize] -= 1;
            if indegree[w as usize] == 0 {
                topo.push(w);
            }
        }
    }
    for &v in topo.iter().rev() {
        if !sources[v as usize] {
            continue;
        }
        if let Some(nearest) = out.row(v as usize).iter().map(|&w| layer[w as usize]).min() {
            layer[v as usize] = nearest.saturating_sub(1);
        }
    }
    layer
}

// ---- stand-in points -----------------------------------------------------------------------------------------------------

fn skeleton(items: &[Item], dag: &[(u32, u32)], layer_of: &[u32], work: &mut Work) -> Result<Skeleton, Error> {
    let real = items.len();
    let mut dummies: u64 = 0;
    for &(a, b) in dag {
        dummies += u64::from(layer_of[b as usize].saturating_sub(layer_of[a as usize]).saturating_sub(1));
        if dummies > MAX_DUMMIES as u64 {
            return Err(Error::TooLarge(format!(
                "the connectors cross more than {MAX_DUMMIES} layers between them: lay out part of the diagram, or give it a layout with no layers"
            )));
        }
    }
    work.charge(dummies + dag.len() as u64)?;
    let mut layer: Vec<u32> = layer_of.to_vec();
    let mut cs: Vec<f64> = items.iter().map(|i| i.cs).collect();
    let mut c0: Vec<f64> = items.iter().map(|i| i.c0).collect();
    let mut links: Vec<(u32, u32, f64)> = Vec::with_capacity(dag.len() + dummies as usize);
    for &(a, b) in dag {
        let (la, lb) = (layer_of[a as usize], layer_of[b as usize]);
        let span = lb.saturating_sub(la);
        let mut prev = a;
        for step in 1..span {
            let id = layer.len() as u32;
            layer.push(la + step);
            cs.push(0.0);
            c0.push(items[a as usize].c0 + (items[b as usize].c0 - items[a as usize].c0) * f64::from(step) / f64::from(span));
            links.push((prev, id, link_weight(prev, id, real)));
            prev = id;
        }
        links.push((prev, b, link_weight(prev, b, real)));
    }
    let layers = layer.iter().max().map_or(0, |&l| l as usize + 1);
    let down_ids: Vec<(u32, u32)> = links.iter().enumerate().map(|(i, l)| (l.0, i as u32)).collect();
    let up_ids: Vec<(u32, u32)> = links.iter().enumerate().map(|(i, l)| (l.1, i as u32)).collect();
    let (up, down) = (csr(layer.len(), &up_ids), csr(layer.len(), &down_ids));
    Ok(Skeleton { layer, cs, c0, real, links, layers, up, down })
}

/// How hard a link pulls: 1 between two shapes, 2 when one end is a stand-in, 8 between two, so that a long connector runs straight.
fn link_weight(a: u32, b: u32, real: usize) -> f64 {
    match (a as usize >= real, b as usize >= real) {
        (false, false) => 1.0,
        (true, true) => 8.0,
        _ => 2.0,
    }
}

// ---- order in the layers -------------------------------------------------------------------------------------------------

/// The order of the points in each layer.
struct Order {
    /// Every point, layer by layer, each layer in its order.
    lay: Vec<u32>,
    /// Where each layer starts in `lay`.
    start: Vec<usize>,
    /// The place of each point in its layer.
    pos: Vec<u32>,
    scratch: Vec<(f64, u32, u32)>,
    slots: Vec<u32>,
}

impl Order {
    /// The layers in the order their points are across now.
    fn new(sk: &Skeleton) -> Order {
        let nv = sk.layer.len();
        let mut lay: Vec<u32> = (0..nv as u32).collect();
        lay.sort_by(|&a, &b| sk.layer[a as usize].cmp(&sk.layer[b as usize]).then(sk.c0[a as usize].total_cmp(&sk.c0[b as usize])).then(a.cmp(&b)));
        let mut start = vec![0_usize; sk.layers + 1];
        for &l in &sk.layer {
            start[l as usize + 1] += 1;
        }
        for i in 0..sk.layers {
            start[i + 1] += start[i];
        }
        let mut order = Order { lay, start, pos: vec![0; nv], scratch: Vec::new(), slots: Vec::new() };
        order.number();
        order
    }

    /// The place of each point in its layer, from the lists.
    fn number(&mut self) {
        for w in self.start.windows(2) {
            for (slot, &v) in self.lay[w[0]..w[1]].iter().enumerate() {
                self.pos[v as usize] = slot as u32;
            }
        }
    }

    /// The sweeps: each down the layers and up them, the order kept for the fewest crossings. It stops after two sweeps that gain nothing, or at none.
    fn sweep(&mut self, sk: &Skeleton, rounds: usize) {
        let mut tree = Vec::new();
        let mut best = self.crossings(sk, &mut tree);
        let mut best_lay = self.lay.clone();
        let mut stalled = 0;
        for _ in 0..rounds {
            if best == 0 || stalled >= 2 {
                break;
            }
            for l in 1..sk.layers {
                self.arrange(sk, l, true);
            }
            for l in (0..sk.layers.saturating_sub(1)).rev() {
                self.arrange(sk, l, false);
            }
            let now = self.crossings(sk, &mut tree);
            if now < best {
                (best, stalled) = (now, 0);
                best_lay.clone_from(&self.lay);
            } else {
                stalled += 1;
            }
        }
        self.lay = best_lay;
        self.number();
    }

    /// Layer `l` put in the order of the mean places of its points' neighbours in the layer above (`above`) or below. A point with no such neighbour keeps its place.
    fn arrange(&mut self, sk: &Skeleton, l: usize, above: bool) {
        let (a, b) = (self.start[l], self.start[l + 1]);
        self.scratch.clear();
        self.slots.clear();
        self.slots.resize(b - a, u32::MAX);
        let rows = if above { &sk.up } else { &sk.down };
        for (slot, &v) in self.lay[a..b].iter().enumerate() {
            let (mut sum, mut count) = (0.0, 0_u32);
            for &id in rows.row(v as usize) {
                let link = sk.links[id as usize];
                sum += f64::from(self.pos[(if above { link.0 } else { link.1 }) as usize]);
                count += 1;
            }
            if count == 0 {
                self.slots[slot] = v;
            } else {
                self.scratch.push((sum / f64::from(count), slot as u32, v));
            }
        }
        self.scratch.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
        let mut moving = self.scratch.iter();
        for (slot, taken) in self.slots.iter_mut().enumerate() {
            if *taken == u32::MAX
                && let Some(m) = moving.next()
            {
                *taken = m.2;
            }
            if let Some(&v) = self.lay.get(a + slot).filter(|_| *taken == u32::MAX) {
                // Cannot happen (as many points move as have a slot to move to); the point keeps the place it had.
                *taken = v;
            }
            self.lay[a + slot] = *taken;
            self.pos[*taken as usize] = slot as u32;
        }
    }

    /// How many pairs of links between neighbouring layers cross: each pair counted with a tree of the places in the lower layer (a link crosses the ones before it
    /// that end further along).
    fn crossings(&self, sk: &Skeleton, tree: &mut Vec<u32>) -> u64 {
        let mut total = 0_u64;
        let mut ends: Vec<u32> = Vec::new();
        for l in 0..sk.layers.saturating_sub(1) {
            let width = self.start[l + 2] - self.start[l + 1];
            tree.clear();
            tree.resize(width + 1, 0);
            let mut inserted = 0_u64;
            for &u in &self.lay[self.start[l]..self.start[l + 1]] {
                ends.clear();
                ends.extend(sk.down.row(u as usize).iter().map(|&id| self.pos[sk.links[id as usize].1 as usize]));
                ends.sort_unstable();
                for &p in &ends {
                    total += inserted - u64::from(prefix(tree, p as usize + 1));
                }
                for &p in &ends {
                    add(tree, p as usize + 1);
                    inserted += 1;
                }
            }
        }
        total
    }
}

/// How many were added at places `1..=i` of the tree.
fn prefix(tree: &[u32], mut i: usize) -> u32 {
    let mut sum = 0;
    while i > 0 {
        sum += tree.get(i).copied().unwrap_or(0);
        i &= i - 1;
    }
    sum
}

/// Adds one at place `i` of the tree.
fn add(tree: &mut [u32], mut i: usize) {
    while i < tree.len() {
        tree[i] += 1;
        i += i & i.wrapping_neg();
    }
}

// ---- places across and along -----------------------------------------------------------------------------------------------

/// The least distance between the middles of two points that are next to each other in a layer.
fn separation(sk: &Skeleton, a: u32, b: u32, gap: f64) -> f64 {
    let stand_in = a as usize >= sk.real || b as usize >= sk.real;
    (sk.cs[a as usize] + sk.cs[b as usize]) / 2.0 + if stand_in { gap / 2.0 } else { gap }
}

/// The middle of each point across the flow: layers packed to the left to start with, then, in turn down and up the layers, each layer as near as it can be to the
/// mean place of its neighbours in the layer before it (after it, going up), weighted by the pull of the links.
fn across(sk: &Skeleton, order: &Order, gap: f64, rounds: usize) -> Vec<f64> {
    let nv = sk.layer.len();
    let mut x = vec![0.0_f64; nv];
    for w in order.start.windows(2) {
        let mut at = 0.0;
        let mut prev: Option<u32> = None;
        for &v in &order.lay[w[0]..w[1]] {
            if let Some(p) = prev {
                at += separation(sk, p, v, gap);
            }
            x[v as usize] = at;
            prev = Some(v);
        }
    }
    let mut row = Row::default();
    for _ in 0..(rounds / 2).clamp(1, 8) {
        for l in 1..sk.layers {
            row.relax(sk, order, l, true, gap, &mut x);
        }
        for l in (0..sk.layers.saturating_sub(1)).rev() {
            row.relax(sk, order, l, false, gap, &mut x);
        }
    }
    for l in 1..sk.layers {
        row.relax(sk, order, l, true, gap, &mut x);
    }
    x
}

/// The vectors one layer's placing uses, kept between layers.
#[derive(Default)]
struct Row {
    want: Vec<f64>,
    weight: Vec<f64>,
    sep: Vec<f64>,
    out: Vec<f64>,
}

impl Row {
    /// Layer `l` moved as near as the order and the spaces allow to the mean place of its neighbours in the layer above (`above`) or below.
    fn relax(&mut self, sk: &Skeleton, order: &Order, l: usize, above: bool, gap: f64, x: &mut [f64]) {
        let (a, b) = (order.start[l], order.start[l + 1]);
        let members = &order.lay[a..b];
        self.want.clear();
        self.weight.clear();
        self.sep.clear();
        let rows = if above { &sk.up } else { &sk.down };
        for (i, &v) in members.iter().enumerate() {
            let (mut pull, mut sum) = (0.0, 0.0);
            for &id in rows.row(v as usize) {
                let link = sk.links[id as usize];
                pull += link.2;
                sum += link.2 * x[(if above { link.0 } else { link.1 }) as usize];
            }
            if pull > 0.0 {
                self.want.push(sum / pull);
                self.weight.push(pull);
            } else {
                self.want.push(x[v as usize]);
                self.weight.push(1.0e-3);
            }
            if i > 0 {
                self.sep.push(separation(sk, members[i - 1], v, gap));
            }
        }
        self.out.clear();
        self.out.resize(members.len(), 0.0);
        place_row(&self.want, &self.weight, &self.sep, &mut self.out);
        for (&v, &p) in members.iter().zip(&self.out) {
            x[v as usize] = p;
        }
    }
}

/// The middle of each shape along the flow: a layer is as thick as its thickest shape, with the space between layers, and its shapes are centred on its line.
fn along(items: &[Item], sk: &Skeleton, gap: f64) -> Vec<f64> {
    let mut thick = vec![0.0_f64; sk.layers];
    for (v, item) in items.iter().enumerate() {
        let l = sk.layer[v] as usize;
        thick[l] = thick[l].max(item.ms);
    }
    let mut middle = vec![0.0_f64; sk.layers];
    let mut top = 0.0;
    for l in 0..sk.layers {
        middle[l] = top + thick[l] / 2.0;
        top += thick[l] + gap;
    }
    (0..items.len()).map(|v| middle[sk.layer[v] as usize]).collect()
}
