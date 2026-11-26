//! The scene of one search: what the clearance is, which obstacles are none, where each end leaves from, the grid's lines, and the two questions the search asks of the
//! buckets when it reaches an edge: whether it is blocked, and what the wires along it cost. The grid itself is never built.

use crate::buckets::{Leg, Obstacle, Obstacles, Wires};
use crate::geom::{DIRS, Dir, EPS, MAX_COORD, Point, Rect, clean};
use crate::work::Work;
use crate::{End, Overlap, Request, Separate, Settings};

/// The settings after they are clamped: no number is negative, not finite, or past what a page can hold.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Clean {
    pub clearance: (f64, f64),
    pub spacing: (f64, f64),
    pub bend: f64,
    pub cross: f64,
    pub near: f64,
    pub lean: Option<(Dir, f64)>,
    pub separate: Separate,
    pub overlap: Overlap,
    pub margin: f64,
}

/// The largest cost: far more than any route's length and far from overflow.
const COST_CAP: f64 = 1.0e12;

impl Clean {
    pub(crate) fn of(s: &Settings) -> Clean {
        Clean {
            clearance: (clean(s.clearance.0, MAX_COORD), clean(s.clearance.1, MAX_COORD)),
            spacing: (clean(s.spacing.0, MAX_COORD), clean(s.spacing.1, MAX_COORD)),
            bend: clean(s.bend, COST_CAP),
            cross: clean(s.cross, COST_CAP),
            near: clean(s.near, COST_CAP),
            lean: s.lean.map(|(d, c)| (d, clean(c, COST_CAP))),
            separate: s.separate,
            overlap: s.overlap,
            margin: clean(s.margin, 2.0 * MAX_COORD),
        }
    }
}

/// How an end leaves: by which way, how long its stub is, where the stub ends, and whether the first move out must go straight on (a stub of no length).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Leave {
    pub way: Dir,
    pub stub: f64,
    pub vertex: Point,
    pub straight: bool,
}

/// What a grid edge is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Edge {
    Free,
    Blocked,
}

/// The run out of work, as an error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Spent;

/// The growth of an obstacle's box on its left, right, bottom and top, trimmed on the sides an end's stub leaves by.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Trim {
    pub id: u32,
    pub g: [f64; 4],
}

/// Whether the straight run `a` to `b` goes into the inside of the box `g`.
fn enters_open(a: Point, b: Point, g: &Rect) -> bool {
    let (x0, x1, y0, y1) = (a.x.min(b.x), a.x.max(b.x), a.y.min(b.y), a.y.max(b.y));
    if (a.y - b.y).abs() <= EPS {
        a.y > g.y0 && a.y < g.y1 && x0.max(g.x0) < x1.min(g.x1)
    } else {
        a.x > g.x0 && a.x < g.x1 && y0.max(g.y0) < y1.min(g.y1)
    }
}

/// How far from `at`, along `way`, a ray enters the inside of `g`: 0 when it starts inside, infinity when it never does.
fn entry(at: Point, way: Dir, g: &Rect) -> f64 {
    let (across, along, lo, hi, a0, a1) = if way.is_level() { (at.y, at.x, g.x0, g.x1, g.y0, g.y1) } else { (at.x, at.y, g.y0, g.y1, g.x0, g.x1) };
    if !(across > a0 && across < a1) {
        return f64::INFINITY;
    }
    let (d0, d1) = if matches!(way, Dir::Right | Dir::Up) { (lo - along, hi - along) } else { (along - hi, along - lo) };
    if d1 <= 0.0 {
        f64::INFINITY
    } else if d0 < 0.0 {
        0.0
    } else {
        d0
    }
}

/// One search's view of the scene at one clearance.
pub(crate) struct Sc<'a> {
    pub obstacles: &'a Obstacles,
    pub wires: &'a Wires,
    pub req: &'a Request,
    pub s: &'a Clean,
    /// The clearance of this search: across level legs `cy`, across upright legs `cx`.
    pub cx: f64,
    pub cy: f64,
    pub exempt: Vec<u32>,
    pub trims: Vec<Trim>,
    /// A query was cut: a box or a leg may have been missed, so an edge asked about counted as blocked.
    pub cut: bool,
    scratch: Vec<Obstacle>,
    legs: Vec<Leg>,
}

impl<'a> Sc<'a> {
    pub(crate) fn new(obstacles: &'a Obstacles, wires: &'a Wires, req: &'a Request, s: &'a Clean, factor: f64) -> Sc<'a> {
        let mut exempt = req.exempt.clone();
        exempt.sort_unstable();
        exempt.dedup();
        Sc {
            obstacles,
            wires,
            req,
            s,
            cx: s.clearance.0 * factor,
            cy: s.clearance.1 * factor,
            exempt,
            trims: Vec::new(),
            cut: false,
            scratch: Vec::new(),
            legs: Vec::new(),
        }
    }

    pub(crate) fn is_exempt(&self, id: u32) -> bool {
        self.exempt.binary_search(&id).is_ok()
    }

    /// The obstacle an end stands on, when it is one and is not exempt.
    fn stands_on(&self, e: &End) -> Option<Obstacle> {
        e.stands_on.filter(|id| !self.is_exempt(*id)).and_then(|id| self.obstacles.get(id).copied())
    }

    /// The box of an obstacle grown by the clearance, trimmed where an end's stub leaves it.
    pub(crate) fn zone(&self, o: &Obstacle) -> Rect {
        let mut g = [self.cx, self.cx, self.cy, self.cy];
        if let Some(t) = self.trims.iter().find(|t| t.id == o.id) {
            for (slot, trim) in g.iter_mut().zip(t.g) {
                *slot = slot.min(trim);
            }
        }
        let [left, right, bottom, top] = g;
        Rect { x0: o.rect.x0 - left, y0: o.rect.y0 - bottom, x1: o.rect.x1 + right, y1: o.rect.y1 + top }
    }

    /// The ways `e` can leave at this clearance. `Err` when the work ran out.
    pub(crate) fn leaves(&mut self, e: &End, work: &mut Work) -> Result<Vec<Leave>, Spent> {
        let single = e.ways.single().is_some();
        let on = self.stands_on(e);
        let mut out = Vec::new();
        for w in DIRS.into_iter().filter(|w| e.ways.contains(*w)) {
            let c = if w.is_level() { self.cx } else { self.cy };
            let mut stub = if single || on.is_some() { c } else { 0.0 };
            let far = e.at.moved(w, c);
            if let Some(s) = &on {
                if enters_open(e.at, far, &s.rect) {
                    continue;
                }
                let zone = s.rect.grown(self.cx, self.cy);
                if !single && far.x > zone.x0 && far.x < zone.x1 && far.y > zone.y0 && far.y < zone.y1 {
                    continue;
                }
            }
            if stub > 0.0 {
                // Shortened to what is free when another obstacle is nearer.
                let window = Rect::of(e.at, far).grown(self.cx, self.cy);
                let mut free = f64::INFINITY;
                if self.obstacles.query_into(&window, work, &mut self.scratch) {
                    self.cut = true;
                    free = 0.0;
                }
                if work.is_spent() {
                    return Err(Spent);
                }
                for o in &self.scratch {
                    let passes = if w.is_level() { o.pass_level } else { o.pass_upright };
                    if passes || Some(o.id) == e.stands_on || self.exempt.binary_search(&o.id).is_ok() {
                        continue;
                    }
                    free = free.min(entry(e.at, w, &o.rect.grown(self.cx, self.cy)));
                }
                stub = stub.min(free);
            }
            let straight = stub <= EPS;
            let stub = if straight { 0.0 } else { stub };
            out.push(Leave { way: w, stub, vertex: e.at.moved(w, stub), straight });
        }
        Ok(out)
    }

    /// The trims the stubs of both ends make on the obstacles they stand on.
    pub(crate) fn trim(&mut self, begin: &[Leave], end: &[Leave]) {
        let req = self.req;
        for (e, leaves) in [(&req.begin, begin), (&req.end, end)] {
            let Some(s) = self.stands_on(e) else { continue };
            let mut g = [f64::INFINITY; 4];
            for l in leaves {
                let side = match l.way {
                    Dir::Left => 0,
                    Dir::Right => 1,
                    Dir::Down => 2,
                    Dir::Up => 3,
                };
                if let Some(slot) = g.get_mut(side) {
                    *slot = slot.min(l.stub);
                }
            }
            match self.trims.iter_mut().find(|t| t.id == s.id) {
                Some(t) => {
                    for (slot, v) in t.g.iter_mut().zip(g) {
                        *slot = slot.min(v);
                    }
                }
                None => self.trims.push(Trim { id: s.id, g }),
            }
        }
    }

    /// Whether the edge `a` to `b` (level or upright) runs through the inside of an obstacle's grown box that does not let it through. A query that was cut counts as blocked.
    pub(crate) fn edge(&mut self, a: Point, b: Point, work: &mut Work) -> Result<Edge, Spent> {
        let window = Rect::of(a, b).grown(self.cx, self.cy);
        let cut = self.obstacles.query_into(&window, work, &mut self.scratch);
        if work.is_spent() {
            return Err(Spent);
        }
        if cut {
            self.cut = true;
            return Ok(Edge::Blocked);
        }
        let level = (a.y - b.y).abs() <= EPS;
        let (x0, x1, y0, y1) = (a.x.min(b.x), a.x.max(b.x), a.y.min(b.y), a.y.max(b.y));
        for o in &self.scratch {
            if self.exempt.binary_search(&o.id).is_ok() {
                continue;
            }
            let z = self.zone(o);
            let hit = if level {
                !o.pass_level && a.y > z.y0 && a.y < z.y1 && x0 < z.x1 && x1 > z.x0
            } else {
                !o.pass_upright && a.x > z.x0 && a.x < z.x1 && y0 < z.y1 && y1 > z.y0
            };
            if hit {
                return Ok(Edge::Blocked);
            }
        }
        Ok(Edge::Free)
    }

    /// Whether a wire's leg is one the route keeps its spacing from.
    fn pays(&self, leg: &Leg) -> bool {
        let related = leg.glued.iter().flatten().any(|g| self.req.glued.iter().flatten().any(|h| h == g));
        match self.s.overlap {
            Overlap::Any => false,
            Overlap::Never => true,
            Overlap::Related => !related && self.s.separate != Separate::None,
        }
    }

    /// The lines the wires add near the area: the spacing on each side of each leg the route keeps apart from.
    pub(crate) fn wire_lines(&self, legs: &[Leg], xs: &mut Vec<f64>, ys: &mut Vec<f64>) {
        for leg in legs.iter().filter(|l| self.pays(l)) {
            if leg.level {
                if self.s.spacing.1 > 0.0 {
                    ys.extend([leg.a.y - self.s.spacing.1, leg.a.y + self.s.spacing.1]);
                }
            } else if self.s.spacing.0 > 0.0 {
                xs.extend([leg.a.x - self.s.spacing.0, leg.a.x + self.s.spacing.0]);
            }
        }
    }

    /// The cost of the wires along the straight run `a` to `b`: a crossing costs `cross` (a leg across ours, our line strictly inside its span, its line strictly inside our
    /// run; half of it where its line is at an end of our run, so that a route that goes straight through a wire at a vertex pays it once, in two halves, and one that turns
    /// on a wire pays half, the same in every image of the scene) and each unit run closer than the spacing to a leg that is kept apart from costs `near`.
    pub(crate) fn wire_cost(&mut self, a: Point, b: Point, work: &mut Work) -> Result<f64, Spent> {
        if self.wires.is_empty() {
            return Ok(0.0);
        }
        let window = Rect::of(a, b).grown(self.s.spacing.0, self.s.spacing.1);
        if self.wires.legs_meeting(&window, work, &mut self.legs) {
            self.cut = true;
        }
        if work.is_spent() {
            return Err(Spent);
        }
        let level = (a.y - b.y).abs() <= EPS;
        let (lo, hi, across) = if level { (a.x.min(b.x), a.x.max(b.x), a.y) } else { (a.y.min(b.y), a.y.max(b.y), a.x) };
        let mut cost = 0.0;
        for leg in &self.legs {
            let (w_lo, w_hi, w_across) = if leg.level { (leg.a.x, leg.b.x, leg.a.y) } else { (leg.a.y, leg.b.y, leg.a.x) };
            if leg.level != level {
                if across > w_lo && across < w_hi && hi - lo > EPS {
                    if w_across > lo + EPS && w_across < hi - EPS {
                        cost += self.s.cross;
                    } else if (w_across - lo).abs() <= EPS || (w_across - hi).abs() <= EPS {
                        cost += self.s.cross / 2.0;
                    }
                }
            } else if self.pays(leg) {
                let gap = if level { self.s.spacing.1 } else { self.s.spacing.0 };
                if gap > 0.0 && (across - w_across).abs() < gap - EPS {
                    cost += self.s.near * (hi.min(w_hi) - lo.max(w_lo)).max(0.0);
                }
            }
        }
        Ok(cost)
    }

    /// The lean cost of leaving along `way`: it is against the flow and the other end lies along the flow.
    pub(crate) fn lean(&self, way: Dir) -> f64 {
        let Some((flow, c)) = self.s.lean else { return 0.0 };
        let (vx, vy) = flow.vector();
        let along = (self.req.end.at.x - self.req.begin.at.x) * vx + (self.req.end.at.y - self.req.begin.at.y) * vy;
        if way == flow.opposite() && along > EPS { c } else { 0.0 }
    }
}

/// The lines of an axis: sorted, and those closer than [`EPS`] merged into one, whose place is the first pinned line of the group (an end's own) or else the first.
pub(crate) fn merged(mut raw: Vec<(f64, bool)>) -> Vec<f64> {
    raw.retain(|(v, _)| v.is_finite());
    raw.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(&(first, _)) = raw.get(i) {
        let mut pinned = None;
        let mut j = i;
        while let Some(&(v, p)) = raw.get(j) {
            if v - first > EPS {
                break;
            }
            if p && pinned.is_none() {
                pinned = Some(v);
            }
            j += 1;
        }
        out.push(pinned.unwrap_or(first));
        i = j;
    }
    out
}

/// The index of the line at `v` (within [`EPS`]), when there is one.
pub(crate) fn find(lines: &[f64], v: f64) -> Option<usize> {
    let i = lines.partition_point(|l| *l < v - EPS);
    lines.get(i).filter(|l| (**l - v).abs() <= EPS).map(|_| i)
}
