//! Two searches written apart from the router's (they share the crate's geometry types and nothing of `search.rs`, `grid.rs` or `buckets.rs`): a Dijkstra over the whole
//! materialized Hanan grid of a scene, with every blocked edge found by looking at every obstacle, and a Dijkstra over the unit lattice for scenes of whole numbers with no
//! clearance. The router's cost must equal theirs. The rules are the lib's words (what a stub is, what the three clearances are, what a wire costs), written out again here
//! from those words and not from the router's code.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use diacraft_route::{Clearance, DIRS, Dir, End, Obstacle, Overlap, Point, Rect, Request, Separate, Settings, Wire};

const EPS: f64 = 1.0e-9;

#[derive(Clone, Debug)]
pub struct Case {
    pub obstacles: Vec<Obstacle>,
    pub wires: Vec<Wire>,
    pub request: Request,
    pub settings: Settings,
}

fn level_factor(level: Clearance) -> f64 {
    match level {
        Clearance::Full => 1.0,
        Clearance::Half => 0.5,
        Clearance::None => 0.0,
    }
}

fn exempt(case: &Case, id: u32) -> bool {
    case.request.exempt.contains(&id)
}

fn box_of(case: &Case, id: u32) -> Option<&Obstacle> {
    case.obstacles.iter().find(|o| o.id == id && !exempt(case, id))
}

/// The distance along `way` from `at` to the point where the ray enters the open box `g`: 0 when `at` is already inside it, and infinity when it never does.
fn entry(at: Point, way: Dir, g: &Rect) -> f64 {
    let (across, along, lo, hi, a0, a1) = if way.is_level() { (at.y, at.x, g.x0, g.x1, g.y0, g.y1) } else { (at.x, at.y, g.y0, g.y1, g.x0, g.x1) };
    if !(across > a0 && across < a1) {
        return f64::INFINITY;
    }
    let sign = match way {
        Dir::Right | Dir::Up => 1.0,
        Dir::Left | Dir::Down => -1.0,
    };
    // Along the axis in the direction of travel, the box spans (s_lo, s_hi) as a distance from `at`.
    let (d0, d1) = if sign > 0.0 { (lo - along, hi - along) } else { (along - hi, along - lo) };
    if d1 <= 0.0 {
        f64::INFINITY
    } else if d0 < 0.0 {
        0.0
    } else {
        d0
    }
}

/// Whether the straight run from `a` to `b` goes into the inside of the box `g` (a run along its edge or past its corner does not).
fn enters_open(a: Point, b: Point, g: &Rect) -> bool {
    let (x0, x1, y0, y1) = (a.x.min(b.x), a.x.max(b.x), a.y.min(b.y), a.y.max(b.y));
    if (a.y - b.y).abs() <= EPS {
        a.y > g.y0 && a.y < g.y1 && x0.max(g.x0) < x1.min(g.x1)
    } else {
        a.x > g.x0 && a.x < g.x1 && y0.max(g.y0) < y1.min(g.y1)
    }
}

/// An end's way out: the first vertex the search starts from, the stub's length, whether the first move must go straight.
#[derive(Clone, Copy, Debug)]
struct Leave {
    way: Dir,
    stub: f64,
    vertex: Point,
    straight: bool,
}

struct Setup {
    zones: Vec<(Rect, bool, bool)>,
    begins: Vec<Leave>,
    ends: Vec<Leave>,
}

fn clamp(v: f64) -> f64 {
    v.max(0.0)
}

fn options(case: &Case, e: &End, cx: f64, cy: f64) -> Vec<Leave> {
    let single = e.ways.single().is_some();
    let mut out = Vec::new();
    for w in DIRS {
        if !e.ways.contains(w) {
            continue;
        }
        let c = if w.is_level() { cx } else { cy };
        let on = e.stands_on.and_then(|s| box_of(case, s));
        let mut stub = if single { c } else { 0.0 };
        if let Some(s) = on {
            let far = e.at.moved(w, c);
            if enters_open(e.at, far, &s.rect) {
                continue;
            }
            if !single {
                let zone = s.rect.grown(cx, cy);
                if far.x > zone.x0 && far.x < zone.x1 && far.y > zone.y0 && far.y < zone.y1 {
                    continue;
                }
            }
            stub = c;
        }
        let mut free = f64::INFINITY;
        for o in &case.obstacles {
            if exempt(case, o.id) || Some(o.id) == e.stands_on {
                continue;
            }
            if (w.is_level() && o.pass_level) || (!w.is_level() && o.pass_upright) {
                continue;
            }
            free = free.min(entry(e.at, w, &o.rect.grown(cx, cy)));
        }
        let stub = stub.min(free);
        let straight = stub <= EPS;
        let stub = if straight { 0.0 } else { stub };
        out.push(Leave { way: w, stub, vertex: e.at.moved(w, stub), straight });
    }
    out
}

fn setup(case: &Case, level: Clearance) -> Setup {
    let f = level_factor(level);
    let (cx, cy) = (clamp(case.settings.clearance.0) * f, clamp(case.settings.clearance.1) * f);
    let begins = options(case, &case.request.begin, cx, cy);
    let ends = options(case, &case.request.end, cx, cy);
    let mut zones = Vec::new();
    for o in &case.obstacles {
        if exempt(case, o.id) {
            continue;
        }
        // The growth on each side: the clearance, trimmed on the side an end's stub leaves by.
        let mut g = [cx, cx, cy, cy]; // left, right, bottom, top
        for (e, opts) in [(&case.request.begin, &begins), (&case.request.end, &ends)] {
            if e.stands_on == Some(o.id) {
                for opt in opts {
                    let side = match opt.way {
                        Dir::Left => 0,
                        Dir::Right => 1,
                        Dir::Down => 2,
                        Dir::Up => 3,
                    };
                    if let Some(slot) = g.get_mut(side) {
                        *slot = slot.min(opt.stub);
                    }
                }
            }
        }
        zones.push((Rect { x0: o.rect.x0 - g[0], y0: o.rect.y0 - g[2], x1: o.rect.x1 + g[1], y1: o.rect.y1 + g[3] }, o.pass_level, o.pass_upright));
    }
    Setup { zones, begins, ends }
}

fn related(case: &Case, w: &Wire) -> bool {
    w.glued.iter().flatten().any(|g| case.request.glued.iter().flatten().any(|h| h == g))
}

fn pays(case: &Case, w: &Wire) -> bool {
    match case.settings.overlap {
        Overlap::Any => false,
        Overlap::Never => true,
        Overlap::Related => !related(case, w) && case.settings.separate != Separate::None,
    }
}

/// The cost of the wires along the straight leg `a` to `b` (level or upright): crossings and near length.
fn wire_cost(case: &Case, a: Point, b: Point) -> f64 {
    let s = &case.settings;
    let level = (a.y - b.y).abs() <= EPS;
    let (lo, hi) = if level { (a.x.min(b.x), a.x.max(b.x)) } else { (a.y.min(b.y), a.y.max(b.y)) };
    let across = if level { a.y } else { a.x };
    let mut cost = 0.0;
    for w in &case.wires {
        for (p, q) in &w.legs {
            let wire_level = (p.y - q.y).abs() <= EPS;
            let (w_lo, w_hi) = if wire_level { (p.x.min(q.x), p.x.max(q.x)) } else { (p.y.min(q.y), p.y.max(q.y)) };
            let w_across = if wire_level { p.y } else { p.x };
            if wire_level != level {
                // Crossing: the wire's leg is across ours; ours is across the wire's line strictly inside its span, and the wire's line is in our half-open span.
                if across > w_lo && across < w_hi && hi - lo > EPS {
                    if w_across > lo + EPS && w_across < hi - EPS {
                        cost += s.cross;
                    } else if (w_across - lo).abs() <= EPS || (w_across - hi).abs() <= EPS {
                        cost += s.cross / 2.0;
                    }
                }
            } else if pays(case, w) {
                let gap = if level { s.spacing.1 } else { s.spacing.0 };
                if gap > 0.0 && (across - w_across).abs() < gap - EPS {
                    cost += s.near * (hi.min(w_hi) - lo.max(w_lo)).max(0.0);
                }
            }
        }
    }
    cost
}

fn lean_cost(case: &Case, way: Dir) -> f64 {
    let Some((flow, c)) = case.settings.lean else { return 0.0 };
    let (vx, vy) = flow.vector();
    let along = (case.request.end.at.x - case.request.begin.at.x) * vx + (case.request.end.at.y - case.request.begin.at.y) * vy;
    if way == flow.opposite() && along > EPS { c } else { 0.0 }
}

/// The cost of a route given as its corners: its length, a bend for each turn, the wires along each leg and the lean of its first leg.
pub fn cost_of(case: &Case, points: &[Point]) -> f64 {
    let mut cost = 0.0;
    let mut last: Option<Dir> = None;
    for pair in points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        cost += (b.x - a.x).abs() + (b.y - a.y).abs() + wire_cost(case, a, b);
        let d = if (b.x - a.x).abs() > (b.y - a.y).abs() {
            if b.x > a.x { Dir::Right } else { Dir::Left }
        } else if b.y > a.y {
            Dir::Up
        } else {
            Dir::Down
        };
        match last {
            None => cost += lean_cost(case, d),
            Some(l) if l != d => cost += case.settings.bend,
            _ => {}
        }
        last = Some(d);
    }
    cost
}

#[derive(PartialEq)]
struct Item(f64, usize);

impl Eq for Item {}

impl PartialOrd for Item {
    fn partial_cmp(&self, other: &Item) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Item {
    fn cmp(&self, other: &Item) -> Ordering {
        other.0.total_cmp(&self.0).then(other.1.cmp(&self.1))
    }
}

fn lines(mut v: Vec<f64>) -> Vec<f64> {
    v.sort_by(f64::total_cmp);
    v.dedup();
    v
}

/// The cheapest route at this clearance over the whole Hanan grid, or `None` when there is none.
pub fn best(case: &Case, level: Clearance) -> Option<f64> {
    let su = setup(case, level);
    if su.begins.is_empty() || su.ends.is_empty() {
        return None;
    }
    let s = &case.settings;
    let mut xs = vec![case.request.begin.at.x, case.request.end.at.x];
    let mut ys = vec![case.request.begin.at.y, case.request.end.at.y];
    for o in su.begins.iter().chain(&su.ends) {
        xs.push(o.vertex.x);
        ys.push(o.vertex.y);
    }
    for (z, pl, pu) in &su.zones {
        if !(*pl && *pu) {
            xs.extend([z.x0, z.x1]);
            ys.extend([z.y0, z.y1]);
        }
    }
    for w in case.wires.iter().filter(|w| pays(case, w)) {
        for (p, q) in &w.legs {
            if (p.y - q.y).abs() <= EPS {
                if s.spacing.1 > 0.0 {
                    ys.extend([p.y - s.spacing.1, p.y + s.spacing.1]);
                }
            } else if s.spacing.0 > 0.0 {
                xs.extend([p.x - s.spacing.0, p.x + s.spacing.0]);
            }
        }
    }
    // The area's edges are lines too: the box round the ends and their stubs, grown by the margin (the scenes of the tests lie well inside it).
    let (kx, ky): (Vec<f64>, Vec<f64>) = (xs.clone(), ys.clone());
    let (x_lo, x_hi) = (kx.iter().copied().fold(f64::INFINITY, f64::min), kx.iter().copied().fold(f64::NEG_INFINITY, f64::max));
    let (y_lo, y_hi) = (ky.iter().copied().fold(f64::INFINITY, f64::min), ky.iter().copied().fold(f64::NEG_INFINITY, f64::max));
    let key_x: Vec<f64> =
        [case.request.begin.at.x, case.request.end.at.x].into_iter().chain(su.begins.iter().chain(&su.ends).map(|o| o.vertex.x)).collect();
    let key_y: Vec<f64> =
        [case.request.begin.at.y, case.request.end.at.y].into_iter().chain(su.begins.iter().chain(&su.ends).map(|o| o.vertex.y)).collect();
    let _ = (x_lo, x_hi, y_lo, y_hi);
    let m = s.margin;
    xs.extend([key_x.iter().copied().fold(f64::INFINITY, f64::min) - m, key_x.iter().copied().fold(f64::NEG_INFINITY, f64::max) + m]);
    ys.extend([key_y.iter().copied().fold(f64::INFINITY, f64::min) - m, key_y.iter().copied().fold(f64::NEG_INFINITY, f64::max) + m]);
    let (xs, ys) = (lines(xs), lines(ys));
    let (nx, ny) = (xs.len(), ys.len());
    let at = |i: usize, j: usize| Point::new(xs[i], ys[j]);
    let index_of = |p: Point| (xs.iter().position(|v| *v == p.x), ys.iter().position(|v| *v == p.y));
    let id = |i: usize, j: usize, h: Dir| (i * ny + j) * 4 + h.index();
    let blocked = |a: Point, b: Point| {
        let level = a.y == b.y;
        su.zones.iter().any(|(z, pl, pu)| {
            if level {
                !pl && a.y > z.y0 && a.y < z.y1 && a.x.min(b.x) < z.x1 && a.x.max(b.x) > z.x0
            } else {
                !pu && a.x > z.x0 && a.x < z.x1 && a.y.min(b.y) < z.y1 && a.y.max(b.y) > z.y0
            }
        })
    };
    let mut dist = vec![f64::INFINITY; nx * ny * 4];
    let mut heap = BinaryHeap::new();
    let push = |dist: &mut Vec<f64>, heap: &mut BinaryHeap<Item>, node: usize, d: f64| {
        if d < dist[node] {
            dist[node] = d;
            heap.push(Item(d, node));
        }
    };
    let step = |i: usize, j: usize, m: Dir| -> Option<(usize, usize)> {
        match m {
            Dir::Right => (i + 1 < nx).then(|| (i + 1, j)),
            Dir::Left => i.checked_sub(1).map(|i| (i, j)),
            Dir::Up => (j + 1 < ny).then(|| (i, j + 1)),
            Dir::Down => j.checked_sub(1).map(|j| (i, j)),
        }
    };
    for b in &su.begins {
        let (Some(i), Some(j)) = index_of(b.vertex) else { continue };
        let g0 = b.stub + wire_cost(case, case.request.begin.at, b.vertex) + lean_cost(case, b.way);
        if b.straight {
            // The first move goes straight out of the end.
            if let Some((ni, nj)) = step(i, j, b.way) {
                let (from, to) = (at(i, j), at(ni, nj));
                if !blocked(from, to) {
                    let d = g0 + (to.x - from.x).abs() + (to.y - from.y).abs() + wire_cost(case, from, to);
                    push(&mut dist, &mut heap, id(ni, nj, b.way), d);
                }
            }
        } else {
            push(&mut dist, &mut heap, id(i, j, b.way), g0);
        }
    }
    while let Some(Item(d, node)) = heap.pop() {
        if d > dist[node] {
            continue;
        }
        let (h, cell) = (DIRS[node % 4], node / 4);
        let (i, j) = (cell / ny, cell % ny);
        for m in DIRS {
            if m == h.opposite() {
                continue;
            }
            let Some((ni, nj)) = step(i, j, m) else { continue };
            let (from, to) = (at(i, j), at(ni, nj));
            if blocked(from, to) {
                continue;
            }
            let turn = if m == h { 0.0 } else { s.bend };
            let nd = d + turn + (to.x - from.x).abs() + (to.y - from.y).abs() + wire_cost(case, from, to);
            push(&mut dist, &mut heap, id(ni, nj, m), nd);
        }
    }
    let mut answer: Option<f64> = None;
    for goal in &su.ends {
        let (Some(i), Some(j)) = index_of(goal.vertex) else { continue };
        let base = goal.stub + wire_cost(case, goal.vertex, case.request.end.at);
        for a in DIRS {
            let extra = if a == goal.way.opposite() {
                0.0
            } else if !goal.straight && a != goal.way {
                s.bend
            } else {
                continue;
            };
            let d = dist[id(i, j, a)];
            if d.is_finite() {
                let total = d + extra + base;
                answer = Some(answer.map_or(total, |x| x.min(total)));
            }
        }
    }
    answer
}

/// The cheapest route over the unit lattice from `-margin` to `size + margin`, for scenes of whole numbers with no clearance and no wires: the length plus a bend for each
/// turn, every lattice point a vertex. `None` when there is none.
pub fn lattice(case: &Case, size: i32, margin: i32) -> Option<f64> {
    let lo = -margin;
    let w = (size + 2 * margin + 1) as usize;
    let cell = |x: i32, y: i32| ((x - lo) as usize) * w + (y - lo) as usize;
    let inside = |x: f64, y: f64| x >= f64::from(lo) && x <= f64::from(size + margin) && y >= f64::from(lo) && y <= f64::from(size + margin);
    let blocked = |x0: i32, y0: i32, x1: i32, y1: i32| {
        case.obstacles.iter().filter(|o| !exempt(case, o.id)).any(|o| {
            let level = y0 == y1;
            if level {
                !o.pass_level
                    && f64::from(y0) > o.rect.y0
                    && f64::from(y0) < o.rect.y1
                    && f64::from(x0.min(x1)) < o.rect.x1
                    && f64::from(x0.max(x1)) > o.rect.x0
            } else {
                !o.pass_upright
                    && f64::from(x0) > o.rect.x0
                    && f64::from(x0) < o.rect.x1
                    && f64::from(y0.min(y1)) < o.rect.y1
                    && f64::from(y0.max(y1)) > o.rect.y0
            }
        })
    };
    let (b, e) = (&case.request.begin, &case.request.end);
    if !(inside(b.at.x, b.at.y) && inside(e.at.x, e.at.y)) {
        return None;
    }
    let mut dist = vec![f64::INFINITY; w * w * 4];
    let mut heap = BinaryHeap::new();
    let (bx, by) = (b.at.x as i32, b.at.y as i32);
    for way in DIRS.into_iter().filter(|d| b.ways.contains(*d)) {
        let (dx, dy) = way.vector();
        let (nx, ny) = (bx + dx as i32, by + dy as i32);
        if nx < lo || ny < lo || nx > size + margin || ny > size + margin || blocked(bx, by, nx, ny) {
            continue;
        }
        let node = cell(nx, ny) * 4 + way.index();
        if 1.0 < dist[node] {
            dist[node] = 1.0;
            heap.push(Item(1.0, node));
        }
    }
    while let Some(Item(d, node)) = heap.pop() {
        if d > dist[node] {
            continue;
        }
        let (h, c) = (DIRS[node % 4], node / 4);
        let (x, y) = ((c / w) as i32 + lo, (c % w) as i32 + lo);
        for m in DIRS {
            if m == h.opposite() {
                continue;
            }
            let (dx, dy) = m.vector();
            let (nx, ny) = (x + dx as i32, y + dy as i32);
            if nx < lo || ny < lo || nx > size + margin || ny > size + margin || blocked(x, y, nx, ny) {
                continue;
            }
            let nd = d + 1.0 + if m == h { 0.0 } else { case.settings.bend };
            let to = cell(nx, ny) * 4 + m.index();
            if nd < dist[to] {
                dist[to] = nd;
                heap.push(Item(nd, to));
            }
        }
    }
    let (ex, ey) = (e.at.x as i32, e.at.y as i32);
    DIRS.into_iter()
        .filter(|a| e.ways.contains(a.opposite()))
        .map(|a| dist[cell(ex, ey) * 4 + a.index()])
        .filter(|d| d.is_finite())
        .min_by(f64::total_cmp)
}
