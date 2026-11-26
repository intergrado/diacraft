//! The search: A\* over (vertex, the way the route arrived) on the Hanan grid of an area, at three clearances, with the limits of every input.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};
use std::sync::OnceLock;

use crate::buckets::{Leg, MAX_AREA_VISITS, Obstacle, Obstacles, Wires};
use crate::centre::{centre, tidy};
use crate::geom::{DIRS, Dir, EPS, Point, Rect, sign};
use crate::grid::{Clean, Edge, Leave, Sc, Spent, find, merged};
use crate::work::{Step, Work};
use crate::{Clearance, Outcome, Request, Settings};

/// The most obstacles in an area, and wires: past either no search is made.
pub const MAX_AREA_OBSTACLES: usize = 2_048;
pub const MAX_AREA_WIRES: usize = 4_096;
/// The most lines on an axis.
pub const MAX_GRID_LINES: usize = 8_192;
/// The most states one route's searches expand.
pub const MAX_EXPANSIONS: u64 = 200_000;
/// The most steps one route takes (and no more than the work it is given).
pub const MAX_ROUTE_WORK: u64 = 2_000_000;
/// The most points of a route.
pub const MAX_ROUTE_POINTS: usize = 48;

/// How many times an area is grown to take in the obstacles that meet it.
const GROWTHS: usize = 3;
/// Costs are compared in units of a billionth, so that sums made in another order are the same cost.
const QUANTUM: f64 = 1.0e9;
/// The leg a first try has to be, at most, to be one the search asked about: nothing here, but the refusal's words.
const BAD_END: &str = "an end is not a finite point within the limit";
const SAME_POINT: &str = "the two ends are at one point";
const TOO_MANY_POINTS: &str = "the route has more than 48 points";

// ------------------------------------------------------------------------------------------------------------------------------------------------------------------
// The two small functions the proofs are about.

/// The number of the state at line `line_x` of the x lines and `line_y` of `lines` y lines, arriving by `way`: `None` for a line or a way that is not one and when the number
/// does not fit. Different states have different numbers.
pub fn state_index(line_x: u32, line_y: u32, way: u8, lines: u32) -> Option<u32> {
    if line_y >= lines || way >= 4 {
        return None;
    }
    let cell = line_x.checked_mul(lines)?.checked_add(line_y)?;
    cell.checked_mul(4)?.checked_add(u32::from(way))
}

/// The most segments the table of forced bends tries.
const MAX_SEGMENTS: u32 = 6;

fn build_table() -> [u8; 144] {
    let mut table = [MAX_SEGMENTS as u8; 144];
    for h in 0..4usize {
        for a in 0..4usize {
            for sx in -1i32..=1 {
                for sy in -1i32..=1 {
                    let mut best = MAX_SEGMENTS as u8;
                    if h == a && sx == 0 && sy == 0 {
                        best = 0;
                    }
                    for m in 1..=MAX_SEGMENTS {
                        for code in 0..3u32.pow(m) {
                            let (mut prev, mut c, mut bends, mut mask) = (h, code, 0u8, 0u8);
                            for _ in 0..m {
                                let rel = (c % 3) as usize;
                                c /= 3;
                                prev = (prev + [0usize, 1, 3].get(rel).copied().unwrap_or(0)) % 4;
                                bends += u8::from(rel != 0);
                                mask |= 1 << prev;
                            }
                            let xok = match (mask & 1 != 0, mask & 4 != 0) {
                                (true, true) => true,
                                (true, false) => sx == 1,
                                (false, true) => sx == -1,
                                (false, false) => sx == 0,
                            };
                            let yok = match (mask & 2 != 0, mask & 8 != 0) {
                                (true, true) => true,
                                (true, false) => sy == 1,
                                (false, true) => sy == -1,
                                (false, false) => sy == 0,
                            };
                            if prev == a && xok && yok {
                                best = best.min(bends);
                            }
                        }
                    }
                    let i = ((h * 4 + a) * 3 + (sx + 1) as usize) * 3 + (sy + 1) as usize;
                    if let Some(slot) = table.get_mut(i) {
                        *slot = best;
                    }
                }
            }
        }
    }
    table
}

/// The fewest bends a route needs from a point where it arrived heading `from` to a point `(dx, dy)` away that it must arrive at heading `arrive`, on a plane with nothing
/// in it: a lower bound on the bends of any route between them, however the boxes lie. It depends on the signs of `dx` and `dy` alone (with a dead band of a billionth).
pub fn forced_bends(from: Dir, arrive: Dir, dx: f64, dy: f64) -> u8 {
    static TABLE: OnceLock<[u8; 144]> = OnceLock::new();
    let table = TABLE.get_or_init(build_table);
    let i = ((from.index() * 4 + arrive.index()) * 3 + (sign(dx) + 1) as usize) * 3 + (sign(dy) + 1) as usize;
    table.get(i).copied().unwrap_or(0)
}

// ------------------------------------------------------------------------------------------------------------------------------------------------------------------
// The first try's test.

/// How far a leg is taken in from a box's edge before it counts as inside, so that a leg along an edge or ending on one does not enter it (the first try's own number).
const INSET: f64 = 1.0e-6;

/// Whether the leg `a` to `b` goes into the inside of `r` taken in by [`INSET`] (Liang and Barsky's clipping).
fn clip_enters(a: Point, b: Point, r: &Rect) -> bool {
    let (x0, x1, y0, y1) = (r.x0 + INSET, r.x1 - INSET, r.y0 + INSET, r.y1 - INSET);
    if x0 >= x1 || y0 >= y1 {
        return false;
    }
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for (p, d, lo, hi) in [(a.x, b.x - a.x, x0, x1), (a.y, b.y - a.y, y0, y1)] {
        if d.abs() < 1.0e-300 {
            if p < lo || p > hi {
                return false;
            }
            continue;
        }
        let (u, v) = ((lo - p) / d, (hi - p) / d);
        let (near, far) = if u < v { (u, v) } else { (v, u) };
        t0 = t0.max(near);
        t1 = t1.min(far);
        if t0 >= t1 {
            return false;
        }
    }
    t0 < t1
}

/// Whether a leg of the route `points` goes into the box of an obstacle that does not let it through: the test the first try is kept by. A box in `exempt` is none. A leg
/// that cannot be tested (a number that is not a point, a query that was cut, work that ran out) counts as entering one, so that the router is asked.
pub fn enters_any(obstacles: &Obstacles, points: &[Point], exempt: &[u32], work: &mut Work) -> bool {
    let mut hits = Vec::new();
    for pair in points.windows(2) {
        let (Some(a), Some(b)) = (pair.first(), pair.get(1)) else { continue };
        if !(a.is_ok() && b.is_ok()) {
            return true;
        }
        let cut = obstacles.query_into(&Rect::of(*a, *b), work, &mut hits);
        if cut || work.is_spent() {
            return true;
        }
        let level = (a.y - b.y).abs() <= EPS;
        let upright = (a.x - b.x).abs() <= EPS;
        for o in &hits {
            if exempt.contains(&o.id) {
                continue;
            }
            let passes = match (level, upright) {
                (true, false) => o.pass_level,
                (false, true) => o.pass_upright,
                _ => o.pass_level && o.pass_upright,
            };
            if !passes && clip_enters(*a, *b, &o.rect) {
                return true;
            }
        }
    }
    false
}

// ------------------------------------------------------------------------------------------------------------------------------------------------------------------
// The route.

/// What an attempt at one clearance and one area found.
enum Attempt {
    Found {
        points: Vec<Point>,
        cut: bool,
    },
    /// No end can leave at this clearance: a larger area would not help.
    NoStart,
    NoRoute,
}

/// Routes `request` round the `obstacles`, keeping off the `wires` as the `settings` say, charging `work`. See the crate's words.
pub fn route(obstacles: &Obstacles, wires: &Wires, request: &Request, settings: &Settings, work: &mut Work) -> Outcome {
    let (b, e) = (request.begin.at, request.end.at);
    if !(b.is_ok() && e.is_ok()) {
        return Outcome::Refused(BAD_END);
    }
    if b == e {
        return Outcome::Refused(SAME_POINT);
    }
    let s = Clean::of(settings);
    // One route takes at most its own share, and the edit's work is charged what it took.
    let share = work.left().min(MAX_ROUTE_WORK);
    let from_edit = share == work.left();
    let mut mine = Work::new(share);
    let outcome = route_levels(obstacles, wires, request, &s, &mut mine);
    let paid = work.absorb(&mine);
    if (mine.is_spent() && from_edit) || !paid {
        work.exhaust();
    }
    outcome
}

fn route_levels(obstacles: &Obstacles, wires: &Wires, request: &Request, s: &Clean, work: &mut Work) -> Outcome {
    let mut expanded = 0u64;
    let flat = s.clearance.0 <= 0.0 && s.clearance.1 <= 0.0;
    let levels: &[(Clearance, f64)] =
        if flat { &[(Clearance::Full, 1.0)] } else { &[(Clearance::Full, 1.0), (Clearance::Half, 0.5), (Clearance::None, 0.0)] };
    for (level, factor) in levels {
        for area_try in 0..2 {
            let mut sc = Sc::new(obstacles, wires, request, s, *factor);
            match attempt(&mut sc, area_try, work, &mut expanded) {
                Err(Spent) => return Outcome::OutOfWork,
                Ok(Attempt::NoStart) => break,
                Ok(Attempt::NoRoute) => {}
                Ok(Attempt::Found { points, cut }) => {
                    if points.len() > MAX_ROUTE_POINTS {
                        return Outcome::Refused(TOO_MANY_POINTS);
                    }
                    return Outcome::Routed { points, clearance: *level, cut };
                }
            }
        }
    }
    Outcome::NoWay
}

/// The box round some points.
fn round(points: &[Point]) -> Rect {
    let first = points.first().copied().unwrap_or_default();
    let mut r = Rect::of(first, first);
    for p in points {
        r = r.union(&Rect::of(*p, *p));
    }
    r
}

fn attempt(sc: &mut Sc<'_>, area_try: usize, work: &mut Work, expanded: &mut u64) -> Result<Attempt, Spent> {
    let req = sc.req;
    let begins = sc.leaves(&req.begin, work)?;
    let ends = sc.leaves(&req.end, work)?;
    if begins.is_empty() || ends.is_empty() {
        return Ok(Attempt::NoStart);
    }
    sc.trim(&begins, &ends);

    // The area: the box round the ends and their stubs, grown, then grown again to take in the obstacles that meet it.
    let mut key: Vec<Point> = vec![req.begin.at, req.end.at];
    key.extend(begins.iter().chain(&ends).map(|l| l.vertex));
    let bbox = round(&key);
    let m = sc.s.margin;
    let mut area = if area_try == 0 { bbox.grown(m, m) } else { bbox.grown(bbox.width() + 3.0 * m, bbox.height() + 3.0 * m) };
    let mut hits: Vec<Obstacle> = Vec::new();
    for round_ in 0..=GROWTHS {
        // The window is the area grown by the clearance: an obstacle whose grown box reaches into the area is one of its obstacles, though its box does not.
        let window = area.grown(sc.cx, sc.cy);
        let cut = sc.obstacles.query_capped(&window, work, &mut hits, MAX_AREA_VISITS);
        if work.is_spent() || cut || hits.len() > MAX_AREA_OBSTACLES {
            return Err(Spent);
        }
        if round_ == GROWTHS {
            break;
        }
        let mut next = area;
        for o in hits.iter().filter(|o| !sc.is_exempt(o.id)) {
            next = next.union(&sc.zone(o));
        }
        if next == area {
            break;
        }
        area = next;
    }
    let mut legs: Vec<Leg> = Vec::new();
    let cut = sc.wires.legs_capped(&area, work, &mut legs, MAX_AREA_VISITS);
    let mut ids: Vec<u32> = legs.iter().map(|l| l.wire).collect();
    ids.sort_unstable();
    ids.dedup();
    if work.is_spent() || cut || ids.len() > MAX_AREA_WIRES {
        return Err(Spent);
    }

    // The lines: those of the ends and their stubs are pinned, so that a route leaves an end exactly.
    let mut xs: Vec<(f64, bool)> = Vec::new();
    let mut ys: Vec<(f64, bool)> = Vec::new();
    let mut walked = 0u64;
    for p in &key {
        xs.push((p.x, true));
        ys.push((p.y, true));
        walked += 2;
    }
    for v in [area.x0, area.x1] {
        xs.push((v, false));
        walked += 1;
    }
    for v in [area.y0, area.y1] {
        ys.push((v, false));
        walked += 1;
    }
    for o in hits.iter().filter(|o| !sc.is_exempt(o.id) && !(o.pass_level && o.pass_upright)) {
        let z = sc.zone(o);
        for v in [z.x0, z.x1].into_iter().filter(|v| *v >= area.x0 && *v <= area.x1) {
            xs.push((v, false));
            walked += 1;
        }
        for v in [z.y0, z.y1].into_iter().filter(|v| *v >= area.y0 && *v <= area.y1) {
            ys.push((v, false));
            walked += 1;
        }
    }
    let (mut wx, mut wy) = (Vec::new(), Vec::new());
    sc.wire_lines(&legs, &mut wx, &mut wy);
    for v in wx.into_iter().filter(|v| *v >= area.x0 && *v <= area.x1) {
        xs.push((v, false));
        walked += 1;
    }
    for v in wy.into_iter().filter(|v| *v >= area.y0 && *v <= area.y1) {
        ys.push((v, false));
        walked += 1;
    }
    let raw = (xs.len() + ys.len()) as u64;
    if !work.charge_for(Step::Line, raw) {
        return Err(Spent);
    }
    work.walk(Step::Line, walked);
    let (xs, ys) = (merged(xs), merged(ys));
    if xs.len() > MAX_GRID_LINES || ys.len() > MAX_GRID_LINES {
        return Err(Spent);
    }

    let mut search = Search::new(xs, ys);
    let Some(found) = search.run(sc, &begins, &ends, work, expanded)? else { return Ok(Attempt::NoRoute) };
    let mut points = search.points(sc, &found);
    let begin_stub = begins.iter().find(|l| l.way == found.begin_way).map_or(0.0, |l| l.stub);
    let end_stub = search.goals.get(found.goal).map_or(0.0, |g| g.leave.stub);
    tidy(&mut points);
    centre(&mut points, sc, &hits, begin_stub, end_stub, work)?;
    tidy(&mut points);
    Ok(Attempt::Found { points, cut: sc.cut })
}

// ------------------------------------------------------------------------------------------------------------------------------------------------------------------
// A* over (vertex, heading).

/// A place the search is done at: an end's stub end (where the end's stub meets the grid), and what it costs to arrive there.
#[derive(Clone, Copy, Debug)]
struct Goal {
    ix: usize,
    iy: usize,
    leave: Leave,
    /// The stub's length and what the wires along it cost.
    base: f64,
}

/// A state reached: its cost so far and the state before it.
#[derive(Clone, Copy, Debug)]
struct Node {
    g: f64,
    parent: u32,
}

/// What the search found: the vertices of the route as line numbers, the begin option it started from and the goal it finished at.
struct Found {
    cells: Vec<(usize, usize)>,
    /// The way the route leaves the begin by.
    begin_way: Dir,
    goal: usize,
}

/// An entry of the heap: the smallest cost first, of equal ones the greatest cost so far (the deepest, nearest the goal), of those the first reached.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Item {
    f: i64,
    g: i64,
    /// A key that turning and mirroring the scene leave as it is (how far the state is from the line between the ends, and from the begin), so that of equal states the
    /// order does not favour a side.
    tie: (i64, i64, i64),
    seq: u64,
    id: u32,
}

impl Ord for Item {
    fn cmp(&self, other: &Item) -> Ordering {
        other.f.cmp(&self.f).then(self.g.cmp(&other.g)).then(other.tie.cmp(&self.tie)).then(other.seq.cmp(&self.seq))
    }
}

impl PartialOrd for Item {
    fn partial_cmp(&self, other: &Item) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn quant(x: f64) -> i64 {
    (x * QUANTUM).round() as i64
}

/// The parent of a state with none.
const NONE: u32 = u32::MAX;
/// The numbers of the goals as states: `NONE - 1 - k`.
fn goal_id(k: usize) -> u32 {
    NONE - 1 - k as u32
}

struct Search {
    xs: Vec<f64>,
    ys: Vec<f64>,
    heap: BinaryHeap<Item>,
    nodes: HashMap<u32, Node>,
    /// The cost of the edge out of a vertex by a way (its length and the wires along it), or `None` when it is blocked.
    edges: HashMap<u32, Option<f64>>,
    seq: u64,
    goals: Vec<Goal>,
    /// The ends, for the tie key.
    ends: (Point, Point),
}

impl Search {
    fn new(xs: Vec<f64>, ys: Vec<f64>) -> Search {
        Search {
            xs,
            ys,
            heap: BinaryHeap::new(),
            nodes: HashMap::new(),
            edges: HashMap::new(),
            seq: 0,
            goals: Vec::new(),
            ends: (Point::default(), Point::default()),
        }
    }

    fn vertex(&self, ix: usize, iy: usize) -> Point {
        Point::new(self.xs.get(ix).copied().unwrap_or(0.0), self.ys.get(iy).copied().unwrap_or(0.0))
    }

    fn id(&self, ix: usize, iy: usize, way: Dir) -> Option<u32> {
        state_index(u32::try_from(ix).ok()?, u32::try_from(iy).ok()?, way.index() as u8, u32::try_from(self.ys.len()).ok()?)
    }

    fn unpack(&self, id: u32) -> (usize, usize, Dir) {
        let ny = self.ys.len().max(1);
        let (cell, way) = ((id / 4) as usize, (id % 4) as usize);
        (cell / ny, cell % ny, Dir::from_index(way))
    }

    fn neighbour(&self, ix: usize, iy: usize, m: Dir) -> Option<(usize, usize)> {
        match m {
            Dir::Right => (ix + 1 < self.xs.len()).then_some((ix + 1, iy)),
            Dir::Left => ix.checked_sub(1).map(|i| (i, iy)),
            Dir::Up => (iy + 1 < self.ys.len()).then_some((ix, iy + 1)),
            Dir::Down => iy.checked_sub(1).map(|i| (ix, i)),
        }
    }

    /// The cost of the edge out of vertex `(ix, iy)` by `m` (not counting the bend), or `None` when it is blocked or there is none.
    fn edge_cost(&mut self, sc: &mut Sc<'_>, ix: usize, iy: usize, m: Dir, work: &mut Work) -> Result<Option<(usize, usize, f64)>, Spent> {
        let Some((nx, ny)) = self.neighbour(ix, iy, m) else { return Ok(None) };
        let Some(key) = self.id(ix, iy, m) else { return Ok(None) };
        let (a, b) = (self.vertex(ix, iy), self.vertex(nx, ny));
        if let Some(c) = self.edges.get(&key) {
            return Ok(c.map(|c| (nx, ny, c)));
        }
        let cost = match sc.edge(a, b, work)? {
            Edge::Blocked => None,
            Edge::Free => Some((b.x - a.x).abs() + (b.y - a.y).abs() + sc.wire_cost(a, b, work)?),
        };
        self.edges.insert(key, cost);
        Ok(cost.map(|c| (nx, ny, c)))
    }

    /// The least a route can still cost from `(ix, iy)` heading `h`: the Manhattan distance to a goal, the bends it forces and what the goal's stub costs.
    fn estimate(&self, sc: &Sc<'_>, ix: usize, iy: usize, h: Dir) -> f64 {
        let p = self.vertex(ix, iy);
        let mut best = f64::INFINITY;
        for g in &self.goals {
            let q = self.vertex(g.ix, g.iy);
            let (dx, dy) = (q.x - p.x, q.y - p.y);
            for a in DIRS {
                let extra = if a == g.leave.way.opposite() {
                    0.0
                } else if !g.leave.straight && a != g.leave.way {
                    1.0
                } else {
                    continue;
                };
                best = best.min(dx.abs() + dy.abs() + sc.s.bend * (f64::from(forced_bends(h, a, dx, dy)) + extra) + g.base);
            }
        }
        if best.is_finite() { best } else { 0.0 }
    }

    /// The tie key of a state: its vertex's distance from the line through the ends and from the begin, each squared, and how far its way points at the end, in billionths.
    fn tie_of(&self, id: u32) -> (i64, i64, i64) {
        if id >= goal_id(3) {
            return (0, 0, 0);
        }
        let (ix, iy, h) = self.unpack(id);
        let (p, (a, b)) = (self.vertex(ix, iy), self.ends);
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let len2 = dx * dx + dy * dy;
        let cross = (p.x - a.x) * dy - (p.y - a.y) * dx;
        let off2 = if len2 > 0.0 { cross * cross / len2 } else { 0.0 };
        let (hx, hy) = h.vector();
        let toward = hx * (b.x - p.x) + hy * (b.y - p.y);
        (quant(off2), quant((p.x - a.x) * (p.x - a.x) + (p.y - a.y) * (p.y - a.y)), quant(-toward))
    }

    /// Reaches state `id` with cost `g`, from `parent`, if that is cheaper than what is known.
    fn relax(&mut self, id: u32, g: f64, parent: u32, h: f64, work: &mut Work) -> Result<(), Spent> {
        if self.nodes.get(&id).is_some_and(|n| n.g <= g + 1.0e-12) {
            return Ok(());
        }
        if !work.charge_for(Step::Push, 1) {
            return Err(Spent);
        }
        work.walk(Step::Push, 1);
        self.nodes.insert(id, Node { g, parent });
        self.seq += 1;
        let tie = self.tie_of(id);
        self.heap.push(Item { f: quant(g + h), g: quant(g), tie, seq: self.seq, id });
        Ok(())
    }

    fn run(&mut self, sc: &mut Sc<'_>, begins: &[Leave], ends: &[Leave], work: &mut Work, expanded: &mut u64) -> Result<Option<Found>, Spent> {
        let req = sc.req;
        self.ends = (req.begin.at, req.end.at);
        for l in ends {
            let (Some(ix), Some(iy)) = (find(&self.xs, l.vertex.x), find(&self.ys, l.vertex.y)) else { continue };
            let base = l.stub + if l.stub > 0.0 { sc.wire_cost(l.vertex, req.end.at, work)? } else { 0.0 };
            self.goals.push(Goal { ix, iy, leave: *l, base });
        }
        for l in begins {
            let (Some(ix), Some(iy)) = (find(&self.xs, l.vertex.x), find(&self.ys, l.vertex.y)) else { continue };
            let g0 = l.stub + if l.stub > 0.0 { sc.wire_cost(req.begin.at, l.vertex, work)? } else { 0.0 } + sc.lean(l.way);
            if l.straight {
                // No stub: the first move goes straight out of the end.
                if let Some((nx, ny, c)) = self.edge_cost(sc, ix, iy, l.way, work)?
                    && let Some(id) = self.id(nx, ny, l.way)
                {
                    let h = self.estimate(sc, nx, ny, l.way);
                    self.relax(id, g0 + c, NONE, h, work)?;
                }
            } else if let Some(id) = self.id(ix, iy, l.way) {
                let h = self.estimate(sc, ix, iy, l.way);
                self.relax(id, g0, NONE, h, work)?;
            }
        }
        while let Some(item) = self.heap.pop() {
            if !work.charge_for(Step::State, 1) {
                return Err(Spent);
            }
            work.walk(Step::State, 1);
            *expanded += 1;
            if *expanded > MAX_EXPANSIONS {
                return Err(Spent);
            }
            let Some(node) = self.nodes.get(&item.id).copied() else { continue };
            if quant(node.g) != item.g {
                continue;
            }
            if item.id >= goal_id(3) {
                return Ok(self.found(item.id, node));
            }
            let (ix, iy, h) = self.unpack(item.id);
            // At a goal's vertex: arriving against the end's way costs nothing more, and a turn onto its stub costs a bend.
            for k in 0..self.goals.len() {
                let Some(goal) = self.goals.get(k).copied() else { continue };
                if goal.ix != ix || goal.iy != iy {
                    continue;
                }
                let extra = if h == goal.leave.way.opposite() {
                    0.0
                } else if !goal.leave.straight && h != goal.leave.way {
                    sc.s.bend
                } else {
                    continue;
                };
                self.relax(goal_id(k), node.g + extra + goal.base, item.id, 0.0, work)?;
            }
            // The moves: straight on, then the turns, the one that ends nearer a goal first.
            let mut moves: Vec<(f64, Dir)> = Vec::new();
            for m in DIRS.into_iter().filter(|m| *m != h.opposite()) {
                let Some((nx, ny)) = self.neighbour(ix, iy, m) else { continue };
                let q = self.vertex(nx, ny);
                let near = self
                    .goals
                    .iter()
                    .map(|g| (self.vertex(g.ix, g.iy), g))
                    .map(|(v, _)| (v.x - q.x).abs() + (v.y - q.y).abs())
                    .fold(f64::INFINITY, f64::min);
                moves.push((if m == h { -1.0 } else { near }, m));
            }
            moves.sort_by(|a, b| a.0.total_cmp(&b.0));
            for (_, m) in moves {
                let Some((nx, ny, cost)) = self.edge_cost(sc, ix, iy, m, work)? else { continue };
                let Some(id) = self.id(nx, ny, m) else { continue };
                let turn = if m == h { 0.0 } else { sc.s.bend };
                let h2 = self.estimate(sc, nx, ny, m);
                self.relax(id, node.g + turn + cost, item.id, h2, work)?;
            }
        }
        Ok(None)
    }

    /// The route to the goal `id`, back along the parents.
    fn found(&self, id: u32, node: Node) -> Option<Found> {
        let goal = (NONE - 1 - id) as usize;
        let mut cells = Vec::new();
        let mut at = node.parent;
        let mut first = at;
        while at != NONE {
            let (ix, iy, _) = self.unpack(at);
            cells.push((ix, iy));
            first = at;
            at = self.nodes.get(&at)?.parent;
            if cells.len() > self.xs.len().saturating_mul(self.ys.len()).saturating_mul(4).saturating_add(4) {
                return None;
            }
        }
        cells.reverse();
        Some(Found { cells, begin_way: self.unpack(first).2, goal })
    }

    /// The corners of the route found, squared: every leg level or upright, the first and last point the ends exactly.
    fn points(&self, sc: &Sc<'_>, found: &Found) -> Vec<Point> {
        let req = sc.req;
        let mut line: Vec<Point> = vec![req.begin.at];
        line.extend(found.cells.iter().map(|(ix, iy)| self.vertex(*ix, *iy)));
        line.push(req.end.at);
        square(&mut line);
        line
    }
}

/// Puts right the one thing the merging of lines can leave: a leg a billionth off level or upright where an end's own line was merged with another's. Legs are classified
/// by which coordinate they keep most nearly, and each run of legs of one kind is given the coordinate of the end it touches, or its first point's.
fn square(points: &mut [Point]) {
    let n = points.len();
    if n < 3 {
        return;
    }
    let level: Vec<bool> = points
        .windows(2)
        .filter_map(|w| match w {
            [a, b] => Some((b.y - a.y).abs() <= (b.x - a.x).abs()),
            _ => None,
        })
        .collect();
    // Forward: each point takes the coordinate its leg keeps from the point before.
    for i in 1..n - 1 {
        let Some(before) = points.get(i - 1).copied() else { continue };
        let Some(here) = points.get_mut(i) else { continue };
        if level.get(i - 1).copied().unwrap_or(true) {
            here.y = before.y;
        } else {
            here.x = before.x;
        }
    }
    // Backward: the last run of legs of one kind lies on the line through the end.
    let Some(end) = points.get(n - 1).copied() else { return };
    let kind = level.get(n - 2).copied().unwrap_or(true);
    let mut i = n - 2;
    while i > 0 {
        if let Some(p) = points.get_mut(i) {
            if kind {
                p.y = end.y;
            } else {
                p.x = end.x;
            }
        }
        if level.get(i - 1).copied() != Some(kind) {
            break;
        }
        i -= 1;
    }
}
