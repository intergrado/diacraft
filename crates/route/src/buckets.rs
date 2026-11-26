//! The obstacles and the wires, in the buckets of a uniform grid (the layout of the engine's snap index: uniform cells, a list of big boxes every query looks at, and
//! each visit counted), and the bounded queries the search asks of them.
//!
//! Nothing here iterates a hash map: the buckets are a `BTreeMap`, so every answer is the same on every machine.

use std::collections::BTreeMap;

use crate::geom::{EPS, Point, Rect};
use crate::work::{Step, Work};

/// The most bucket entries one query visits (a box in several buckets is an entry in each). A query that reaches it is cut, and says so.
pub const MAX_QUERY_VISITS: usize = 4_096;
/// The most buckets one box may cover before it is a big box, one of a list every query looks at.
pub const MAX_BUCKETS_PER_BOX: i64 = 16;
/// The most entries the search's area query visits: it reads every box and leg of an area, at most 2,048 boxes and 4,096 legs, a few buckets each.
pub(crate) const MAX_AREA_VISITS: usize = 16_384;
/// A window of more columns than this is answered by one scan of the buckets in its columns instead of one probe for each column.
const MAX_WINDOW_COLUMNS: i64 = 64;
/// The most legs one wire may have, and the most legs all the wires may have: a wire past either is left out and counted.
pub const MAX_WIRE_LEGS: usize = 4_096;
pub const MAX_WIRES_LEGS: usize = 400_000;

/// The bucket a coordinate falls in: no overflow for any input, and a number that is not finite goes to the outermost bucket on its side (NaN to the middle one).
pub fn bucket_of(v: f64, cell: f64) -> i64 {
    if v.is_nan() || cell.is_nan() || cell <= 0.0 || !cell.is_finite() {
        return 0;
    }
    let v = v.clamp(-crate::geom::MAX_COORD, crate::geom::MAX_COORD);
    (v / cell).floor() as i64
}

/// The size of a bucket for boxes with these longer sides: twice the median, and never below a millionth.
fn cell_for(mut sides: Vec<f64>) -> f64 {
    sides.retain(|s| s.is_finite() && *s > 0.0);
    sides.sort_by(f64::total_cmp);
    let median = sides.get(sides.len() / 2).copied().unwrap_or(1.0);
    (2.0 * median).clamp(1.0e-6, 1.0e6)
}

/// The buckets of one kind of box: a slot number for each box, in the cells it covers.
#[derive(Clone, Debug)]
struct Index {
    cell: f64,
    map: BTreeMap<(i64, i64), Vec<u32>>,
    big: Vec<u32>,
}

impl Index {
    fn new(cell: f64) -> Index {
        Index { cell, map: BTreeMap::new(), big: Vec::new() }
    }

    fn span(&self, r: &Rect) -> (i64, i64, i64, i64) {
        (bucket_of(r.x0, self.cell), bucket_of(r.x1, self.cell), bucket_of(r.y0, self.cell), bucket_of(r.y1, self.cell))
    }

    fn insert(&mut self, slot: u32, r: &Rect) {
        let (x0, x1, y0, y1) = self.span(r);
        let count = (x1 - x0).saturating_add(1).saturating_mul((y1 - y0).saturating_add(1));
        if count > MAX_BUCKETS_PER_BOX {
            self.big.push(slot);
            return;
        }
        for bx in x0..=x1 {
            for by in y0..=y1 {
                self.map.entry((bx, by)).or_default().push(slot);
            }
        }
    }

    fn remove(&mut self, slot: u32, r: &Rect) {
        let (x0, x1, y0, y1) = self.span(r);
        let count = (x1 - x0).saturating_add(1).saturating_mul((y1 - y0).saturating_add(1));
        if count > MAX_BUCKETS_PER_BOX {
            self.big.retain(|s| *s != slot);
            return;
        }
        for bx in x0..=x1 {
            for by in y0..=y1 {
                if let Some(list) = self.map.get_mut(&(bx, by)) {
                    list.retain(|s| *s != slot);
                    if list.is_empty() {
                        self.map.remove(&(bx, by));
                    }
                }
            }
        }
    }

    /// The slots in the buckets the window touches, each once and in order, at most [`MAX_QUERY_VISITS`] entries looked at and each charged. `true` when the query was cut.
    fn gather(&self, window: &Rect, work: &mut Work, out: &mut Vec<u32>, cap: usize) -> bool {
        out.clear();
        if !(window.x0.is_finite() && window.y0.is_finite() && window.x1.is_finite() && window.y1.is_finite()) {
            return true;
        }
        let mut visits = 0usize;
        let mut cut = false;
        // One visit of the query: charged, counted, and refused past the cap or the work.
        let mut visit = |work: &mut Work| -> bool {
            if visits >= cap || !work.charge_for(Step::Visit, 1) {
                return false;
            }
            visits += 1;
            work.walk(Step::Visit, 1);
            true
        };
        'all: {
            for slot in &self.big {
                if !visit(work) {
                    cut = true;
                    break 'all;
                }
                out.push(*slot);
            }
            let (x0, x1, y0, y1) = self.span(window);
            let columns = (x1 - x0).saturating_add(1);
            if columns <= MAX_WINDOW_COLUMNS {
                for bx in x0..=x1 {
                    if !visit(work) {
                        cut = true;
                        break 'all;
                    }
                    for (_, list) in self.map.range((bx, y0)..=(bx, y1)) {
                        if !visit(work) {
                            cut = true;
                            break 'all;
                        }
                        for slot in list {
                            if !visit(work) {
                                cut = true;
                                break 'all;
                            }
                            out.push(*slot);
                        }
                    }
                }
            } else {
                for ((_, by), list) in self.map.range((x0, i64::MIN)..=(x1, i64::MAX)) {
                    if !visit(work) {
                        cut = true;
                        break 'all;
                    }
                    if *by < y0 || *by > y1 {
                        continue;
                    }
                    for slot in list {
                        if !visit(work) {
                            cut = true;
                            break 'all;
                        }
                        out.push(*slot);
                    }
                }
            }
        }
        out.sort_unstable();
        out.dedup();
        cut
    }
}

/// A box a route goes round, with what may pass through it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Obstacle {
    pub id: u32,
    pub rect: Rect,
    /// Level legs may run through it (`ShapePermeableX`).
    pub pass_level: bool,
    /// Upright legs may run through it (`ShapePermeableY`).
    pub pass_upright: bool,
}

/// What a query found.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Query {
    /// The obstacles whose box meets the window, by `id`.
    pub hits: Vec<Obstacle>,
    /// The query was cut (the visits or the work ran out): there may be more.
    pub cut: bool,
}

/// The obstacles of a page, in buckets.
#[derive(Clone, Debug)]
pub struct Obstacles {
    slots: Vec<Option<Obstacle>>,
    free: Vec<u32>,
    by_id: BTreeMap<u32, u32>,
    index: Index,
    rejected: usize,
    built_for: usize,
}

/// The obstacle as it is kept, or `None` for a box that is no obstacle (not finite, beyond the limit, or of no area).
fn kept(o: &Obstacle) -> Option<Obstacle> {
    let rect = Rect::of(Point::new(o.rect.x0, o.rect.y0), Point::new(o.rect.x1, o.rect.y1));
    let ok = rect.is_ok()
        && rect.width() > 0.0
        && rect.height() > 0.0
        && o.rect.x0.is_finite()
        && o.rect.y0.is_finite()
        && o.rect.x1.is_finite()
        && o.rect.y1.is_finite();
    ok.then_some(Obstacle { rect, ..*o })
}

impl Obstacles {
    /// The obstacles of `list`, bucketed. A box that is not finite, lies beyond the limit or has no area is left out and counted ([`Obstacles::rejected`]); a repeated `id`
    /// keeps its first box.
    pub fn new(list: Vec<Obstacle>) -> Obstacles {
        let mut me = Obstacles { slots: Vec::new(), free: Vec::new(), by_id: BTreeMap::new(), index: Index::new(1.0), rejected: 0, built_for: 0 };
        for o in list {
            match kept(&o) {
                Some(k) if !me.by_id.contains_key(&k.id) => {
                    let Ok(slot) = u32::try_from(me.slots.len()) else {
                        me.rejected += 1;
                        continue;
                    };
                    me.by_id.insert(k.id, slot);
                    me.slots.push(Some(k));
                }
                _ => me.rejected += 1,
            }
        }
        me.rebuild();
        me
    }

    fn rebuild(&mut self) {
        let sides: Vec<f64> = self.slots.iter().flatten().map(|o| o.rect.width().max(o.rect.height())).collect();
        self.index = Index::new(cell_for(sides));
        for (slot, o) in self.slots.iter().enumerate() {
            if let (Some(o), Ok(slot)) = (o, u32::try_from(slot)) {
                self.index.insert(slot, &o.rect);
            }
        }
        self.built_for = self.by_id.len();
    }

    /// How many boxes were left out.
    pub fn rejected(&self) -> usize {
        self.rejected
    }

    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    /// The obstacle with this `id`.
    pub fn get(&self, id: u32) -> Option<&Obstacle> {
        self.by_id.get(&id).and_then(|s| self.slots.get(*s as usize)).and_then(|o| o.as_ref())
    }

    /// Puts `new` in the place of the obstacle `id` (taking it out when `None`). A box that is no obstacle takes the old one out and is counted.
    pub fn replace(&mut self, id: u32, new: Option<Obstacle>) {
        if let Some(slot) = self.by_id.remove(&id) {
            if let Some(Some(old)) = self.slots.get_mut(slot as usize).map(Option::take) {
                self.index.remove(slot, &old.rect);
            }
            self.free.push(slot);
        }
        if let Some(o) = new {
            match kept(&Obstacle { id, ..o }) {
                Some(k) => {
                    let slot = match self.free.pop() {
                        Some(s) => s,
                        None => match u32::try_from(self.slots.len()) {
                            Ok(s) => {
                                self.slots.push(None);
                                s
                            }
                            Err(_) => {
                                self.rejected += 1;
                                return;
                            }
                        },
                    };
                    if let Some(place) = self.slots.get_mut(slot as usize) {
                        *place = Some(k);
                        self.by_id.insert(id, slot);
                        self.index.insert(slot, &k.rect);
                    }
                }
                None => self.rejected += 1,
            }
        }
        let n = self.by_id.len();
        if n > self.built_for.saturating_mul(4).saturating_add(64) || (self.built_for > 64 && n.saturating_mul(4) < self.built_for) {
            self.compact();
        }
    }

    /// Closes the gaps the replaced slots left and fits the buckets to what is there now.
    fn compact(&mut self) {
        let live: Vec<Obstacle> = self.slots.iter().flatten().copied().collect();
        self.slots = live.into_iter().map(Some).collect();
        self.free.clear();
        self.by_id = self.slots.iter().enumerate().filter_map(|(i, o)| Some((o.as_ref()?.id, u32::try_from(i).ok()?))).collect();
        self.rebuild();
    }

    /// The obstacles whose box meets `window`, by `id`, every visit charged to `work`.
    pub fn query(&self, window: &Rect, work: &mut Work) -> Query {
        let mut hits = Vec::new();
        let cut = self.query_into(window, work, &mut hits);
        Query { hits, cut }
    }

    /// [`Obstacles::query`] into a vector the caller keeps, which it empties first. `true` when the query was cut.
    pub fn query_into(&self, window: &Rect, work: &mut Work, hits: &mut Vec<Obstacle>) -> bool {
        self.query_capped(window, work, hits, MAX_QUERY_VISITS)
    }

    /// [`Obstacles::query_into`] with a visit cap of the caller's (the search's area query reads a whole area, and has a larger one).
    pub(crate) fn query_capped(&self, window: &Rect, work: &mut Work, hits: &mut Vec<Obstacle>, cap: usize) -> bool {
        let mut slots = Vec::new();
        let cut = self.index.gather(window, work, &mut slots, cap);
        hits.clear();
        for slot in slots {
            if let Some(Some(o)) = self.slots.get(slot as usize)
                && o.rect.meets(window)
            {
                hits.push(*o);
            }
        }
        hits.sort_by_key(|o| o.id);
        cut
    }
}

/// One leg of a wire, level or upright, with its ends in order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Leg {
    pub wire: u32,
    pub a: Point,
    pub b: Point,
    pub level: bool,
    pub glued: [Option<u32>; 2],
}

impl Leg {
    pub(crate) fn rect(&self) -> Rect {
        Rect { x0: self.a.x, y0: self.a.y, x1: self.b.x, y1: self.b.y }
    }
}

/// The legs of the other connectors of a page: a route keeps its spacing from them and pays for crossing them.
#[derive(Clone, Debug)]
pub struct Wire {
    pub id: u32,
    /// The legs, each from one point to another: level or upright (a leg that is neither is left out and counted).
    pub legs: Vec<(Point, Point)>,
    /// The shapes its ends are glued to: two wires that share one are related.
    pub glued: [Option<u32>; 2],
}

/// The wires of a page, in buckets.
#[derive(Clone, Debug)]
pub struct Wires {
    legs: Vec<Option<Leg>>,
    free: Vec<u32>,
    by_wire: BTreeMap<u32, Vec<u32>>,
    index: Index,
    rejected: usize,
    skipped: usize,
    live: usize,
    built_for: usize,
}

/// The legs of a wire as they are kept: each level or upright and ordered; the count of those left out (not finite or beyond the limit, a slant, or no length).
fn kept_legs(w: &Wire) -> (Vec<Leg>, usize) {
    let mut out = Vec::new();
    let mut skipped = 0usize;
    for (p, q) in &w.legs {
        let (a, b) = (Point::new(p.x.min(q.x), p.y.min(q.y)), Point::new(p.x.max(q.x), p.y.max(q.y)));
        let level = (q.y - p.y).abs() <= EPS;
        let upright = (q.x - p.x).abs() <= EPS;
        if !(p.is_ok() && q.is_ok()) || level == upright {
            skipped += 1;
            continue;
        }
        // A level leg lies on one `y`, an upright one on one `x`.
        let (a, b) = if level { (Point::new(a.x, p.y), Point::new(b.x, p.y)) } else { (Point::new(p.x, a.y), Point::new(p.x, b.y)) };
        out.push(Leg { wire: w.id, a, b, level, glued: w.glued });
    }
    (out, skipped)
}

impl Wires {
    /// The wires of `list`, bucketed. A wire of more than [`MAX_WIRE_LEGS`] legs, or past [`MAX_WIRES_LEGS`] legs in all, is left out and counted, and so is a repeated `id`.
    pub fn new(list: Vec<Wire>) -> Wires {
        let mut me = Wires {
            legs: Vec::new(),
            free: Vec::new(),
            by_wire: BTreeMap::new(),
            index: Index::new(1.0),
            rejected: 0,
            skipped: 0,
            live: 0,
            built_for: 0,
        };
        for w in &list {
            me.add(w);
        }
        me.rebuild();
        me
    }

    /// Adds the legs of `w` to the lists (not to the buckets).
    fn add(&mut self, w: &Wire) -> Vec<u32> {
        let mut slots = Vec::new();
        if w.legs.len() > MAX_WIRE_LEGS || self.by_wire.contains_key(&w.id) || self.live.saturating_add(w.legs.len()) > MAX_WIRES_LEGS {
            self.rejected += 1;
            return slots;
        }
        let (legs, skipped) = kept_legs(w);
        self.skipped += skipped;
        for leg in legs {
            let slot = match self.free.pop() {
                Some(s) => s,
                None => match u32::try_from(self.legs.len()) {
                    Ok(s) => {
                        self.legs.push(None);
                        s
                    }
                    Err(_) => break,
                },
            };
            if let Some(place) = self.legs.get_mut(slot as usize) {
                *place = Some(leg);
                slots.push(slot);
                self.live += 1;
            }
        }
        self.by_wire.insert(w.id, slots.clone());
        slots
    }

    fn rebuild(&mut self) {
        let sides: Vec<f64> = self.legs.iter().flatten().map(|l| (l.b.x - l.a.x).max(l.b.y - l.a.y)).collect();
        self.index = Index::new(cell_for(sides));
        for (slot, leg) in self.legs.iter().enumerate() {
            if let (Some(leg), Ok(slot)) = (leg, u32::try_from(slot)) {
                self.index.insert(slot, &leg.rect());
            }
        }
        self.built_for = self.live;
    }

    /// How many wires were left out.
    pub fn rejected(&self) -> usize {
        self.rejected
    }

    /// How many legs were left out for being neither level nor upright, not finite or beyond the limit, or of no length.
    pub fn skipped_legs(&self) -> usize {
        self.skipped
    }

    /// How many wires are kept.
    pub fn len(&self) -> usize {
        self.by_wire.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_wire.is_empty()
    }

    /// How many legs are kept.
    pub fn leg_count(&self) -> usize {
        self.live
    }

    /// Puts `new` in the place of the wire `id` (taking it out when `None`).
    pub fn replace(&mut self, id: u32, new: Option<Wire>) {
        if let Some(slots) = self.by_wire.remove(&id) {
            for slot in slots {
                if let Some(Some(old)) = self.legs.get_mut(slot as usize).map(Option::take) {
                    self.index.remove(slot, &old.rect());
                    self.live = self.live.saturating_sub(1);
                }
                self.free.push(slot);
            }
        }
        if let Some(w) = new {
            let slots = self.add(&Wire { id, ..w });
            for slot in slots {
                if let Some(Some(leg)) = self.legs.get(slot as usize) {
                    let r = leg.rect();
                    self.index.insert(slot, &r);
                }
            }
        }
        if self.live > self.built_for.saturating_mul(4).saturating_add(64) || (self.built_for > 64 && self.live.saturating_mul(4) < self.built_for) {
            self.compact();
        }
    }

    fn compact(&mut self) {
        let mut remap: BTreeMap<u32, u32> = BTreeMap::new();
        let mut legs = Vec::new();
        for (slot, leg) in self.legs.iter().enumerate() {
            if let (Some(leg), Ok(old), Ok(new)) = (leg, u32::try_from(slot), u32::try_from(legs.len())) {
                remap.insert(old, new);
                legs.push(Some(*leg));
            }
        }
        for slots in self.by_wire.values_mut() {
            *slots = slots.iter().filter_map(|s| remap.get(s).copied()).collect();
        }
        self.legs = legs;
        self.free.clear();
        self.rebuild();
    }

    /// The `id`s of the wires with a leg that meets `window`, in order, every visit charged to `work`.
    pub fn meeting(&self, window: &Rect, work: &mut Work) -> Vec<u32> {
        let mut legs = Vec::new();
        self.legs_meeting(window, work, &mut legs);
        let mut ids: Vec<u32> = legs.iter().map(|l| l.wire).collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    /// The legs that meet `window` into `out`, which is emptied first. `true` when the query was cut.
    pub(crate) fn legs_meeting(&self, window: &Rect, work: &mut Work, out: &mut Vec<Leg>) -> bool {
        self.legs_capped(window, work, out, MAX_QUERY_VISITS)
    }

    /// [`Wires::legs_meeting`] with a visit cap of the caller's.
    pub(crate) fn legs_capped(&self, window: &Rect, work: &mut Work, out: &mut Vec<Leg>, cap: usize) -> bool {
        let mut slots = Vec::new();
        let cut = self.index.gather(window, work, &mut slots, cap);
        out.clear();
        for slot in slots {
            if let Some(Some(leg)) = self.legs.get(slot as usize)
                && leg.rect().meets(window)
            {
                out.push(*leg);
            }
        }
        cut
    }
}
