//! The shapes of a page, indexed by area, for the snaps that reach other shapes.
//!
//! **Why an index.** A drag asks "what is near the pointer" at every move, and a page may hold 100,000 shapes (`MAX_SHAPES_PER_PAGE` is a
//! million). A scan of the page for each ask is what the reviews found, in other places, to be the cost to avoid. Here the page is read **once**
//! into a [`ShapeIndex`]: the box of each top-level shape on the page, in paper inches, put into the buckets of a uniform grid it covers. A query is
//! the buckets its window touches, and the shapes in them: its cost is the shapes near the window, and not the shapes of the page.
//!
//! **What is read, and the bound.** At most [`MAX_INDEXED`] top-level shapes, in page order: reading a shape's frame is a handful of cells
//! resolved through its master and styles (8 to 12 microseconds each: 10,000 shapes cost about 100 milliseconds, which the window pays on a thread of its own,
//! `ui-egui` `snap_reader.rs`; a command pays it on the thread that runs it). A page with more has
//! [`ShapeIndex::truncated`] set, and the shapes after those are not snap targets. A shape that is deleted from its master, a guide, a
//! one-dimensional shape (a connector) and one whose box is not a finite number is not indexed. The members of a group are not indexed (a group
//! is one box, as if its `IsSnapTarget` were 0). The counts are plain fields, so that a test holds the work to
//! a number and not to a time ([`ShapeIndex::read`], and [`Gathered::examined`] for a query).
//!
//! **Hostile pages.** A box past [`COORD_LIMIT`] paper inches from the origin is indexed in the farthest bucket it can name; a box that covers more
//! than [`MAX_BUCKETS_PER_SHAPE`] buckets goes to the list of big shapes, which every query looks at first. **None is left out**: the list has no size of its
//! own, and each of its entries is a visit counted against the same [`MAX_VISITS`] as a bucket entry is, so a page of thousands of huge shapes costs the
//! cap and a query that reaches it is cut (and says so), as it is in a crowded bucket. **The buckets are visited from the middle of the window outward**
//! (the ring of buckets at each distance from the middle one in turn), so that when the visits run out it is the shapes farthest from the pointer that are
//! not looked at, and not the ones under it.

use std::collections::HashMap;

use diacraft_doc::{CellAddr, Document, Owner, ShapeRef, ShapeType, SheetRef};

use super::geometry::{OutlineWork, PathBox};
use crate::draw::Reader;

/// A point in paper inches, x right and y up.
pub type P = (f64, f64);

/// The most top-level shapes of a page that are read into the index: 10,000 (it was 5,000 until the window made the page's index ready off its own thread).
pub const MAX_INDEXED: usize = 10_000;

/// The most buckets one shape may cover before it is a big shape, one of a list every query looks at.
pub const MAX_BUCKETS_PER_SHAPE: usize = 16;

/// The most bucket entries one query looks at (a shape in several buckets is an entry in each): the cap that bounds a gather, whatever sits in the
/// buckets it meets. A query that reaches it is cut, and says so. A shape is in at most [`MAX_BUCKETS_PER_SHAPE`] buckets, so a window of a few
/// inches on a crowded page is far under it (2,500 squares over one spot made a gather push and sort a million ids).
pub const MAX_VISITS: usize = 4_096;

/// The farthest a coordinate may be from the origin, in paper inches, to name a bucket: a box beyond it is put in the farthest one.
pub const COORD_LIMIT: f64 = 1.0e6;

/// The most buckets in each direction a query window may cover: a window wider than this many buckets is cut to the part round its middle.
pub const MAX_WINDOW_BUCKETS: i64 = 48;

/// An axis-aligned rectangle in paper inches, `x0 <= x1` and `y0 <= y1`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

impl Rect {
    /// The rectangle through two corners, whichever order they come in.
    pub fn of(a: P, b: P) -> Rect {
        Rect { x0: a.0.min(b.0), y0: a.1.min(b.1), x1: a.0.max(b.0), y1: a.1.max(b.1) }
    }

    /// The rectangle round these points, if there is one and every number is finite.
    pub fn round(points: &[P]) -> Option<Rect> {
        let first = points.first()?;
        let mut r = Rect { x0: first.0, y0: first.1, x1: first.0, y1: first.1 };
        for p in points {
            if !(p.0.is_finite() && p.1.is_finite()) {
                return None;
            }
            r = Rect { x0: r.x0.min(p.0), y0: r.y0.min(p.1), x1: r.x1.max(p.0), y1: r.y1.max(p.1) };
        }
        Some(r)
    }

    pub fn centre(&self) -> P {
        ((self.x0 + self.x1) / 2.0, (self.y0 + self.y1) / 2.0)
    }

    pub fn width(&self) -> f64 {
        self.x1 - self.x0
    }

    pub fn height(&self) -> f64 {
        self.y1 - self.y0
    }

    /// The rectangle grown by `d` on every side.
    pub fn grown(&self, d: f64) -> Rect {
        Rect { x0: self.x0 - d, y0: self.y0 - d, x1: self.x1 + d, y1: self.y1 + d }
    }

    pub fn meets(&self, other: &Rect) -> bool {
        self.x0 <= other.x1 && other.x0 <= self.x1 && self.y0 <= other.y1 && other.y0 <= self.y1
    }

    /// How far the point is from the rectangle: 0 inside it.
    pub fn distance_to(&self, p: P) -> f64 {
        let dx = (self.x0 - p.0).max(0.0).max(p.0 - self.x1);
        let dy = (self.y0 - p.1).max(0.0).max(p.1 - self.y1);
        dx.hypot(dy)
    }
}

/// What an index of `items` shapes holds in memory, in bytes: each item (its size and 16 bytes for the heap block of its path), each of the `buckets` (64 bytes), each of the
/// `in_buckets` entries of the buckets (4), each of the `big` shapes (4) and each of the `ids` of the table of `ID`s (16), and **half again** over the sum.
pub(super) fn price_index(items: usize, buckets: usize, in_buckets: usize, big: usize, ids: usize) -> usize {
    let item = std::mem::size_of::<Item>().saturating_add(ITEM_HEAP);
    let bytes = items
        .saturating_mul(item)
        .saturating_add(buckets.saturating_mul(BUCKET))
        .saturating_add(in_buckets.saturating_mul(BUCKET_ENTRY))
        .saturating_add(big.saturating_mul(BIG_ENTRY))
        .saturating_add(ids.saturating_mul(ID_ENTRY));
    bytes.saturating_add(bytes / 2)
}

/// The heap block of an item's path, a bucket's table entry and entries, a big shape's entry and a table entry of an `ID`, in bytes ([`price_index`]).
pub(super) const ITEM_HEAP: usize = 16;
pub(super) const BUCKET: usize = 64;
pub(super) const BUCKET_ENTRY: usize = 4;
pub(super) const BIG_ENTRY: usize = 4;
pub(super) const ID_ENTRY: usize = 16;

/// One indexed shape.
#[derive(Clone, Debug)]
pub struct Item {
    pub id: u32,
    /// The corners of the shape's box on the page, in the order of its own frame: (0, 0) first.
    pub corners: [P; 4],
    /// The axis-aligned box round the corners: the shape's alignment box.
    pub bbox: Rect,
    /// Where the shape is, to read what a query needs of it (connection points, geometry).
    pub at: ShapeRef,
    /// Whether the shape draws a line: its `LinePattern`, read through its master and styles, is not 0 (spec R48). A cell found nowhere is no
    /// pattern set, and the shape keeps its lines: only a pattern of 0 takes them.
    pub line: bool,
    /// The shape's `Angle` cell, in radians: what a turn matches (row 7). It is the shape's own turn, a flipped shape's included.
    pub angle: f64,
    /// The direction of the shape's own x axis on the page, from the cells: its `Angle`, and a half turn more for a flip in x. The corners say it too (`atan2` of the x edge), but far
    /// from the origin only to the rounding of the corners, and a line along the shape's own side is that angle times its distance from the origin off.
    pub frame_angle: f64,
    /// For a shape turned by other than whole quarter turns: the level and upright box round every path it draws, built when the index is
    /// made under the page's budget. `None` for a shape that is not turned, one that draws no path, and one the budget could not pay for (`bounds_by_corners`).
    pub path_box: Option<Rect>,
    /// Whether the shape is turned and its box round its paths was left out for what it costs: the page's budget could not pay for a section, or the tree is of more than
    /// [`super::geometry::MAX_BOX_MEMBERS`] shapes. A shape that draws no path is not cut: it has nothing to pay for.
    pub path_cut: bool,
}

/// The indexed shapes of one page.
#[derive(Debug)]
pub struct ShapeIndex {
    cell: f64,
    items: Vec<Item>,
    buckets: HashMap<(i64, i64), Vec<u32>>,
    big: Vec<u32>,
    /// Where each indexed shape is in `items`, by its `ID` (the adapter asks for a shape by the `Id` the targets name).
    by_id: HashMap<u32, u32>,
    /// How many top-level shapes of the page were read to make the index: at most [`MAX_INDEXED`], whatever the page holds.
    pub read: usize,
    /// How many top-level shapes the page holds.
    pub total: usize,
    /// Whether some shape of the page is not in the index: the page holds more than [`MAX_INDEXED`].
    pub truncated: bool,
    /// How many turned shapes have the box round their paths, and how many turned shapes keep the box round their corners because the page's budget could not pay for
    /// their paths, they draw none, or they are a group of too many members.
    pub bounds_by_paths: usize,
    pub bounds_by_corners: usize,
    /// How many of the turned shapes kept the box round their corners for what their paths cost (`Item::path_cut`): the page budget spent, and not a shape that draws
    /// nothing. A result that lines up by these boxes says so (`truncated`).
    pub bounds_cut: usize,
    /// What the page's boxes cost: the page's budget, spent.
    pub box_cost: OutlineWork,
}

/// What a query found near a window.
#[derive(Debug, Default)]
pub struct Gathered<'a> {
    /// The shapes whose box meets the window, the nearest to the window's middle first, at most the limit asked for.
    pub items: Vec<&'a Item>,
    /// How many shapes the query looked at, each once: the work it did, whatever the page holds.
    pub examined: usize,
    /// How many bucket entries it visited, a shape in several buckets once for each: at most [`MAX_VISITS`].
    pub visited: usize,
    /// Whether the window held more than the limit, or the visits ran out before the buckets did.
    pub cut: bool,
}

/// The buckets at the distance `r` (the larger of the two differences in x and y) from the bucket `(cx, cy)`: that one alone for 0, else the sides of the square
/// round it, each bucket once. At most `8 r` of them.
fn ring(cx: i64, cy: i64, r: i64) -> impl Iterator<Item = (i64, i64)> {
    let across = (cx - r..=cx + r).flat_map(move |bx| if r == 0 { vec![(bx, cy)] } else { vec![(bx, cy - r), (bx, cy + r)] });
    let down = (cy - r + 1..=cy + r - 1).flat_map(move |by| [(cx - r, by), (cx + r, by)]);
    across.chain(down)
}

fn bucket_of(v: f64, cell: f64) -> i64 {
    let v = v.clamp(-COORD_LIMIT, COORD_LIMIT);
    (v / cell).floor() as i64
}

impl ShapeIndex {
    /// Reads page `page`: the box of each top-level shape (up to `limit`, which is [`MAX_INDEXED`] for a page the engine reads), in paper inches, `k` being the drawing
    /// units to a paper inch.
    /// The size of a bucket follows the shapes: twice the median of their longer sides, between a quarter of an inch and eight, so that a bucket holds a
    /// few shapes on a crowded page and a few inches of an empty one.
    pub(crate) fn build(doc: &Document, page: u32, k: f64, limit: usize) -> ShapeIndex {
        let mut index = ShapeIndex {
            cell: 1.0,
            items: Vec::new(),
            buckets: HashMap::new(),
            big: Vec::new(),
            by_id: HashMap::new(),
            read: 0,
            total: 0,
            truncated: false,
            bounds_by_paths: 0,
            bounds_by_corners: 0,
            bounds_cut: 0,
            box_cost: OutlineWork::default(),
        };
        let Some(page_ref) = doc.page(page) else { return index };
        index.total = page_ref.shapes.len();
        let reader = Reader::new(doc);
        let mut found: Vec<Item> = Vec::new();
        for (i, shape) in page_ref.shapes.iter().enumerate() {
            if i >= limit {
                index.truncated = true;
                break;
            }
            index.read += 1;
            if shape.del || shape.kind == ShapeType::Guide {
                continue;
            }
            let Ok(position) = u32::try_from(i) else { break };
            let at = ShapeRef { owner: Owner::Page(page), path: vec![position] };
            if reader.is_one_d(&SheetRef::Shape(at.clone())) {
                continue;
            }
            let corners = reader.corners(&at).map(|p| (p.0 / k, p.1 / k));
            let Some(bbox) = Rect::round(&corners) else { continue };
            let sheet = SheetRef::Shape(at.clone());
            let pattern = CellAddr::direct("LinePattern");
            let line = !(reader.is_set(&sheet, &pattern) && reader.number(&sheet, "LinePattern") == 0.0);
            let angle = reader.number(&sheet, "Angle");
            let pose = reader.pose(&sheet);
            let frame_angle = pose.angle + if pose.flip_x { std::f64::consts::PI } else { 0.0 };
            found.push(Item { id: shape.id, corners, bbox, at, line, angle, frame_angle, path_box: None, path_cut: false });
        }
        // The box round the paths of each turned shape, under one budget for the page, charged before each section is built.
        let mut budget = super::geometry::Budget::page();
        for item in &mut found {
            if !is_turned(&item.corners) {
                continue;
            }
            match super::geometry::path_box(&reader, doc, &item.at, k, &mut budget) {
                PathBox::Box(b) => {
                    item.path_box = Some(Rect { x0: b.x0, y0: b.y0, x1: b.x1, y1: b.y1 });
                    index.bounds_by_paths += 1;
                }
                PathBox::Nothing => index.bounds_by_corners += 1,
                PathBox::Cut => {
                    item.path_cut = true;
                    index.bounds_by_corners += 1;
                    index.bounds_cut += 1;
                }
            }
        }
        index.box_cost = budget.spent();
        let mut sides: Vec<f64> = found.iter().map(|i| i.bbox.width().max(i.bbox.height())).filter(|s| s.is_finite()).collect();
        sides.sort_by(f64::total_cmp);
        let median = sides.get(sides.len() / 2).copied().unwrap_or(1.0);
        index.cell = (2.0 * median).clamp(0.25, 8.0);
        for item in found {
            index.insert(item);
        }
        index
    }

    fn insert(&mut self, item: Item) {
        let Ok(number) = u32::try_from(self.items.len()) else {
            self.truncated = true;
            return;
        };
        self.by_id.entry(item.id).or_insert(number);
        let (x0, x1) = (bucket_of(item.bbox.x0, self.cell), bucket_of(item.bbox.x1, self.cell));
        let (y0, y1) = (bucket_of(item.bbox.y0, self.cell), bucket_of(item.bbox.y1, self.cell));
        let count = (x1 - x0 + 1).saturating_mul(y1 - y0 + 1);
        if count > MAX_BUCKETS_PER_SHAPE as i64 {
            self.big.push(number);
        } else {
            for bx in x0..=x1 {
                for by in y0..=y1 {
                    self.buckets.entry((bx, by)).or_default().push(number);
                }
            }
        }
        self.items.push(item);
    }

    /// The shapes whose box meets `window`, nearest to the window's middle first, at most `limit` of them. `skip` says which `ID`s never count (the
    /// shapes being moved).
    pub fn gather(&self, window: &Rect, limit: usize, skip: &dyn Fn(u32) -> bool) -> Gathered<'_> {
        let mut out = Gathered::default();
        // Which shapes were looked at: a bit for each, so that a shape in many buckets is looked at once and nothing is sorted.
        let mut seen = vec![0u64; self.items.len().div_ceil(64)];
        let mut hits: Vec<(f64, &Item)> = Vec::new();
        // The big shapes first: every query meets them. Each is a visit.
        let mut more = self.big.iter().all(|number| self.visit(*number, window, skip, &mut seen, &mut out, &mut hits));
        if more && window.x0.is_finite() && window.y0.is_finite() && window.x1.is_finite() && window.y1.is_finite() {
            let (mut x0, mut x1) = (bucket_of(window.x0, self.cell), bucket_of(window.x1, self.cell));
            let (mut y0, mut y1) = (bucket_of(window.y0, self.cell), bucket_of(window.y1, self.cell));
            // A window wider than the bound is cut to the part round its middle.
            for (lo, hi) in [(&mut x0, &mut x1), (&mut y0, &mut y1)] {
                if *hi - *lo + 1 > MAX_WINDOW_BUCKETS {
                    let mid = *lo + (*hi - *lo) / 2;
                    *lo = mid - MAX_WINDOW_BUCKETS / 2;
                    *hi = mid + MAX_WINDOW_BUCKETS / 2 - 1;
                }
            }
            // The buckets in rings round the middle one: the middle, then the eight round it, then the sixteen round those, and so on to the window's edge.
            let (cx, cy) = (x0 + (x1 - x0) / 2, y0 + (y1 - y0) / 2);
            let rings = (cx - x0).max(x1 - cx).max(cy - y0).max(y1 - cy);
            'rings: for r in 0..=rings {
                for (bx, by) in ring(cx, cy, r) {
                    if bx < x0 || bx > x1 || by < y0 || by > y1 {
                        continue;
                    }
                    let Some(list) = self.buckets.get(&(bx, by)) else { continue };
                    more = list.iter().all(|number| self.visit(*number, window, skip, &mut seen, &mut out, &mut hits));
                    if !more {
                        break 'rings;
                    }
                }
            }
        }
        hits.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.id.cmp(&b.1.id)));
        out.cut |= hits.len() > limit;
        hits.truncate(limit);
        out.items = hits.into_iter().map(|(_, item)| item).collect();
        out
    }

    /// One bucket entry of a gather: the shape `number`, looked at once however many buckets it is in. `false` when the visits have run out.
    fn visit<'a>(
        &'a self,
        number: u32,
        window: &Rect,
        skip: &dyn Fn(u32) -> bool,
        seen: &mut [u64],
        out: &mut Gathered<'a>,
        hits: &mut Vec<(f64, &'a Item)>,
    ) -> bool {
        if out.visited >= MAX_VISITS {
            out.cut = true;
            return false;
        }
        out.visited += 1;
        let (word, bit) = (number as usize / 64, number % 64);
        let Some(slot) = seen.get_mut(word) else { return true };
        if *slot >> bit & 1 == 1 {
            return true;
        }
        *slot |= 1 << bit;
        let Some(item) = self.items.get(number as usize) else { return true };
        out.examined += 1;
        if !skip(item.id) && item.bbox.meets(window) {
            hits.push((item.bbox.distance_to(window.centre()), item));
        }
        true
    }

    /// What the index holds in memory, in bytes, **estimated** from the number of its entries ([`price_index`]): each item with the heap block of its path, each bucket with its
    /// entries, the list of big shapes and the table of `ID`s. Measured 111 to 230 bytes a shape on pages of 10,000 (`reads.rs`: `measure_what_a_kept_read_holds`),
    /// against the 368 to 404 this says: generous.
    pub fn held_bytes(&self) -> usize {
        let in_buckets: usize = self.buckets.values().map(Vec::len).sum();
        price_index(self.items.len(), self.buckets.len(), in_buckets, self.big.len(), self.by_id.len())
    }

    /// The indexed shapes, in page order.
    pub fn items(&self) -> &[Item] {
        &self.items
    }

    /// The indexed shape with `ID` `id`.
    pub fn find(&self, id: u32) -> Option<&Item> {
        self.by_id.get(&id).and_then(|n| self.items.get(*n as usize))
    }

    /// How many shapes are in the index.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether no shape is in the index.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// How many steps of the largest coordinate a corner may lie off from where its shape's own sides put it, for the rounding of the numbers it was made of (a corner is a sum
/// of a pin as far from the origin as the page goes and a product by the shape's turn, through a group's transform or several).
const CORNER_STEPS: f64 = 16.0;

/// The rounding noise on the corners of a shape's box, in paper inches: [`CORNER_STEPS`] steps of the largest coordinate, a step being `f64::EPSILON` times
/// the number. A box held square to itself and to the page by a fixed tolerance in its own length is no box at 1e8 inches, where a corner is only known to 1.5e-8: the
/// tolerance is that and the noise.
pub(super) fn corner_noise(c: &[P; 4]) -> f64 {
    CORNER_STEPS * f64::EPSILON * c.iter().map(|p| p.0.abs().max(p.1.abs())).fold(0.0, f64::max)
}

/// Whether the box a shape's corners make is turned by other than whole quarter turns (a box that is not a rectangle is not turned: it is framed by its bounds). Its sides are
/// square to each other within a billionth of the longer one's length and the noise of the corners ([`corner_noise`]), and it is turned when its angle is off a quarter turn by more
/// than a billionth of a radian and the noise over its side. A box smaller than the noise of its corners has no angle to read: it is not turned.
pub(super) fn is_turned(c: &[P; 4]) -> bool {
    let (u, v) = ((c[1].0 - c[0].0, c[1].1 - c[0].1), (c[3].0 - c[0].0, c[3].1 - c[0].1));
    let (lu, lv) = (u.0.hypot(u.1), v.0.hypot(v.1));
    if !(lu.is_finite() && lv.is_finite() && lu > 0.0 && lv > 0.0) {
        return false;
    }
    let (noise, longest) = (corner_noise(c), lu.max(lv));
    if noise >= lu.min(lv) || (u.0 * v.0 + u.1 * v.1).abs() > 1.0e-9 * longest * longest + 4.0 * noise * longest {
        return false;
    }
    let tolerance = 1.0e-9 + 2.0 * noise / lu;
    let q = u.1.atan2(u.0).rem_euclid(std::f64::consts::FRAC_PI_2);
    q > tolerance && std::f64::consts::FRAC_PI_2 - q > tolerance
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    /// The corners, in the order of the shape's own frame, of a `w` by `h` shape with its pin in its middle at (5, 5), turned `degrees`.
    fn turned_corners(w: f64, h: f64, degrees: f64) -> [P; 4] {
        let (s, c) = degrees.to_radians().sin_cos();
        let at = |x: f64, y: f64| (5.0 + (x - w / 2.0) * c - (y - h / 2.0) * s, 5.0 + (x - w / 2.0) * s + (y - h / 2.0) * c);
        [at(0.0, 0.0), at(w, 0.0), at(w, h), at(0.0, h)]
    }

    #[test]
    fn a_box_is_turned_when_its_sides_are_square_to_each_other_and_not_square_to_the_page_by_more_than_a_tolerance() {
        assert!(
            is_turned(&turned_corners(2.0, 1.0, 30.0)) && is_turned(&turned_corners(2.0, 1.0, -30.0)) && is_turned(&turned_corners(1.0, 2.0, 135.0))
        );
        for degrees in [0.0, 90.0, 180.0, 270.0, 360.0, -90.0] {
            assert!(!is_turned(&turned_corners(2.0, 1.0, degrees)), "{degrees}");
        }
        // A box whose sides are not square to each other is no box that is turned (it is framed by its bounds): skewed, with the same sides.
        assert!(!is_turned(&[(0.0, 0.0), (2.0, 0.0), (2.5, 1.0), (0.5, 1.0)]));
        assert!(!is_turned(&[(0.0, 0.0), (2.0, 1.0), (2.5, 1.5), (0.5, 0.5)]));
        // No size, and numbers that are not finite.
        assert!(!is_turned(&[(1.0, 1.0), (1.0, 1.0), (0.0, 2.0), (0.0, 2.0)]));
        assert!(!is_turned(&[(1.0, 1.0), (2.0, 2.0), (2.0, 2.0), (1.0, 1.0)]));
        let mut nan = turned_corners(2.0, 1.0, 30.0);
        nan[1].0 = f64::NAN;
        assert!(!is_turned(&nan));
        let mut inf = turned_corners(2.0, 1.0, 30.0);
        inf[1].1 = f64::INFINITY;
        assert!(!is_turned(&inf));
    }

    /// The corners moved by `off` in both coordinates: the sums round as the corners of a shape with its pin that far from the origin do.
    fn far(c: [P; 4], off: f64) -> [P; 4] {
        c.map(|p| (p.0 + off, p.1 + off))
    }

    #[test]
    fn a_box_far_from_the_origin_is_turned_by_the_same_rule_with_the_rounding_of_its_corners_allowed() {
        // A 2 by 0.5 box turned 30 degrees is turned at every distance (at 1e8 the corners are rounded to 1.5e-8 inch, and the sides were held square to a billionth of
        // their length); a box at a quarter turn is not, whatever its corners were rounded to.
        for off in [0.0, 1.0e4, 1.0e5, 1.0e6, 1.0e7, 1.0e8, 1.0e9, 1.0e10] {
            for degrees in [30.0, 135.0, -60.0, 0.01] {
                assert!(is_turned(&far(turned_corners(2.0, 0.5, degrees), off)), "{degrees} degrees at {off:e}");
            }
            for degrees in [0.0, 90.0, 180.0, 270.0, -90.0, 360.0] {
                assert!(!is_turned(&far(turned_corners(2.1, 0.47, degrees), off)), "{degrees} degrees at {off:e}");
            }
        }
        // A real turn of 1e-4 radian is one at 1e8, and a box really skewed by a thousandth of an inch is no box there either.
        assert!(is_turned(&far(turned_corners(2.0, 0.5, 1.0e-4_f64.to_degrees()), 1.0e8)));
        let skewed = far([(0.0, 0.0), (2.0, 0.0), (2.5, 1.0), (0.5, 1.0)], 1.0e8);
        assert!(!is_turned(&skewed));
        let mut off_square = far(turned_corners(2.0, 0.5, 30.0), 1.0e8);
        off_square[3].0 += 1.0e-3;
        assert!(!is_turned(&off_square));
    }

    #[test]
    fn the_rounding_of_a_far_corner_moves_the_angle_of_a_quarter_turn_by_no_more_than_the_noise_over_its_side() {
        // A box at a quarter turn whose corners were rounded at 1e8 (a step of 1.5e-8 inch) has its angle off by that over its side. 5e-8 radian is within the noise of a corner
        // there (3.4e-7 over a side of 2.1), 1e-5 radian is a turn; near the origin 5e-8 radian is a turn too.
        let off_by = |radians: f64| far(turned_corners(2.1, 0.47, 90.0 + radians.to_degrees()), 1.0e8);
        assert!(!is_turned(&off_by(5.0e-8)));
        assert!(!is_turned(&off_by(2.0e-7)));
        // The noise over the side is 3.4e-7 there: 5e-7 and 8e-7 radian are turns (a tolerance that was the noise itself, 7e-7, or the noise times the side, 1.5e-6, was too wide).
        assert!(is_turned(&off_by(5.0e-7)) && is_turned(&off_by(8.0e-7)));
        assert!(is_turned(&off_by(1.0e-5)));
        assert!(is_turned(&turned_corners(2.1, 0.47, 90.0 + 5.0e-8_f64.to_degrees())));
    }

    #[test]
    fn a_long_box_far_from_the_origin_is_square_to_itself_by_the_noise_times_its_length() {
        // The error of a dot product of two sides is the noise times the longer side: a box 100 inches long at 1e10 is square within 3.5e-5 times 100, which a tolerance
        // that took the noise over the length (3.5e-7) would not hold.
        assert!(is_turned(&far(turned_corners(100.0, 30.0, 30.0), 1.0e10)));
        assert!(is_turned(&far(turned_corners(100.0, 30.0, 30.0), 1.0e8)));
    }

    #[test]
    fn the_noise_is_that_of_the_largest_coordinate_of_either_axis() {
        // A shape far out in y only: its corners are rounded to 1.5e-8 there, and are still square to each other and turned.
        let far_up = |c: [P; 4]| c.map(|p| (p.0, p.1 + 1.0e8));
        assert!(is_turned(&far_up(turned_corners(2.0, 0.5, 30.0))));
        assert!(!is_turned(&far_up(turned_corners(2.1, 0.47, 90.0))));
        let far_right = |c: [P; 4]| c.map(|p| (p.0 + 1.0e8, p.1));
        assert!(is_turned(&far_right(turned_corners(2.0, 0.5, 30.0))));
    }

    #[test]
    fn a_box_with_one_side_smaller_than_the_rounding_of_its_corners_is_no_turned_box() {
        // At 1e12 the noise of a corner is 3.5e-3 inch: a box 2 inches long and a thousandth thick has a side to read an angle from and another that is lost in the noise.
        assert!(!is_turned(&far(turned_corners(2.0, 0.001, 30.0), 1.0e12)));
        assert!(is_turned(&far(turned_corners(2.0, 0.5, 30.0), 1.0e12)));
    }

    #[test]
    fn a_box_smaller_than_the_rounding_of_its_corners_is_no_turned_box() {
        // At 1e12 a step of a number is 1.2e-4 inch and the noise of a corner 3.5e-3: a box a thousandth of an inch across has no sides to read an angle from.
        assert!(!is_turned(&far(turned_corners(0.001, 0.001, 30.0), 1.0e12)));
        assert!(is_turned(&far(turned_corners(2.0, 0.5, 30.0), 1.0e12)));
        // Numbers too big to hold a box at all.
        assert!(!is_turned(&far(turned_corners(2.0, 0.5, 30.0), 1.0e300)));
    }

    #[test]
    fn the_sides_of_a_turned_box_may_be_off_square_by_a_billionth_and_not_by_a_millionth() {
        // The fourth corner moved along the first side by `e`: the dot of the sides is 2 e, and the tolerance a billionth of the longer side squared (4 billionths).
        let skewed = |e: f64| {
            let mut c = turned_corners(2.0, 1.0, 30.0);
            let (s, co) = 30.0_f64.to_radians().sin_cos();
            c[3] = (c[3].0 + e * co, c[3].1 + e * s);
            c
        };
        assert!(is_turned(&skewed(1.0e-9)) && is_turned(&skewed(0.0)));
        assert!(!is_turned(&skewed(1.0e-6)));
    }

    fn item(id: u32, x: f64, y: f64, w: f64, h: f64) -> Item {
        let corners = [(x, y), (x + w, y), (x + w, y + h), (x, y + h)];
        Item {
            id,
            corners,
            bbox: Rect::round(&corners).unwrap(),
            at: ShapeRef { owner: Owner::Page(1), path: vec![id] },
            line: true,
            angle: 0.0,
            frame_angle: 0.0,
            path_box: None,
            path_cut: false,
        }
    }

    fn index_of(items: Vec<Item>) -> ShapeIndex {
        let mut index = ShapeIndex {
            cell: 1.0,
            items: Vec::new(),
            buckets: HashMap::new(),
            big: Vec::new(),
            by_id: HashMap::new(),
            read: 0,
            total: 0,
            truncated: false,
            bounds_by_paths: 0,
            bounds_by_corners: 0,
            bounds_cut: 0,
            box_cost: OutlineWork::default(),
        };
        for item in items {
            index.insert(item);
        }
        index
    }

    #[test]
    fn a_query_finds_the_shapes_near_the_window_and_not_the_others() {
        let index = index_of(vec![item(1, 0.0, 0.0, 1.0, 1.0), item(2, 5.0, 5.0, 1.0, 1.0), item(3, 0.5, 0.5, 0.2, 0.2)]);
        let found = index.gather(&Rect { x0: 0.4, y0: 0.4, x1: 0.6, y1: 0.6 }, 10, &|_| false);
        let ids: Vec<u32> = found.items.iter().map(|i| i.id).collect();
        assert!(ids.contains(&1) && ids.contains(&3) && !ids.contains(&2));
        // The shape far away was not even looked at.
        assert!(found.examined <= 2, "{}", found.examined);
    }

    #[test]
    fn the_shapes_being_moved_are_not_found() {
        let index = index_of(vec![item(1, 0.0, 0.0, 1.0, 1.0), item(2, 0.5, 0.5, 1.0, 1.0)]);
        let found = index.gather(&Rect { x0: 0.0, y0: 0.0, x1: 2.0, y1: 2.0 }, 10, &|id| id == 1);
        assert_eq!(found.items.iter().map(|i| i.id).collect::<Vec<_>>(), [2]);
    }

    #[test]
    fn a_window_with_more_shapes_than_the_limit_keeps_the_nearest() {
        let items: Vec<Item> = (0..40).map(|i| item(i, f64::from(i) * 0.05, 0.0, 0.01, 0.01)).collect();
        let index = index_of(items);
        let found = index.gather(&Rect { x0: 0.0, y0: -0.5, x1: 3.0, y1: 0.5 }, 5, &|_| false);
        assert_eq!(found.items.len(), 5);
        assert!(found.cut);
        // Nearest to the window's middle (x 1.5): shapes 30 (x 1.5) first.
        assert_eq!(found.items.first().map(|i| i.id), Some(30));
    }

    #[test]
    fn a_huge_shape_is_a_big_one_and_found_by_every_query() {
        let index = index_of(vec![item(1, -1000.0, -1000.0, 2000.0, 2000.0), item(2, 5.0, 5.0, 1.0, 1.0)]);
        assert_eq!(index.big.len(), 1);
        let found = index.gather(&Rect { x0: 100.0, y0: 100.0, x1: 101.0, y1: 101.0 }, 10, &|_| false);
        assert_eq!(found.items.iter().map(|i| i.id).collect::<Vec<_>>(), [1]);
    }

    #[test]
    fn many_big_shapes_are_all_in_the_index_and_found_by_the_window_they_meet() {
        // L-c: 256 was the most big shapes, and the ones past it were left out of the index. None is, now.
        let n = 300u32;
        let items: Vec<Item> = (0..n).map(|i| item(i, -1000.0, -1000.0, 2000.0 + f64::from(i), 2000.0)).collect();
        let index = index_of(items);
        assert_eq!((index.big.len(), index.len()), (n as usize, n as usize));
        assert!(!index.truncated);
        let found = index.gather(&Rect { x0: 100.0, y0: 100.0, x1: 101.0, y1: 101.0 }, 400, &|_| false);
        assert_eq!(found.items.len(), n as usize, "the last one is found too");
        assert!(found.items.iter().any(|i| i.id == n - 1));
        // They cost visits, like any bucket entry, so thousands of them are cut by the cap and say so.
        let items: Vec<Item> = (0..5_000u32).map(|i| item(i, -1000.0, -1000.0, 2000.0 + f64::from(i), 2000.0)).collect();
        let crowded = index_of(items);
        let found = crowded.gather(&Rect { x0: 100.0, y0: 100.0, x1: 101.0, y1: 101.0 }, 64, &|_| false);
        assert!(found.visited <= MAX_VISITS && found.cut, "{found:?}");
    }

    #[test]
    fn a_crowded_corner_does_not_hide_the_shape_under_the_pointer() {
        // L-c: the window is 5 buckets across. 4,300 tiny shapes sit in the bucket at its low corner, which was visited first and used up the visits; the one
        // shape at its middle was never looked at.
        let mut items: Vec<Item> = (0..4_300u32).map(|i| item(i, -1.9 + f64::from(i % 10) * 0.001, -1.9, 0.01, 0.01)).collect();
        items.push(item(9_999, 0.0, 0.0, 0.2, 0.2));
        let index = index_of(items);
        let found = index.gather(&Rect { x0: -2.0, y0: -2.0, x1: 2.0, y1: 2.0 }, 64, &|_| false);
        assert!(found.cut && found.visited <= MAX_VISITS, "{}: {}", found.visited, found.cut);
        assert_eq!(found.items.first().map(|i| i.id), Some(9_999), "the nearest to the middle of the window is found, and first");
    }

    #[test]
    fn the_rings_round_a_bucket_are_each_bucket_once() {
        let mut seen = std::collections::HashSet::new();
        let mut order = Vec::new();
        for r in 0..=6 {
            for cell in ring(10, -3, r) {
                assert_eq!((cell.0 - 10).abs().max((cell.1 + 3).abs()), r, "{cell:?}");
                assert!(seen.insert(cell), "{cell:?} twice");
                order.push(r);
            }
        }
        assert_eq!(seen.len(), 13 * 13);
        assert!(order.windows(2).all(|w| w[0] <= w[1]), "nearest rings first");
    }

    #[test]
    fn hostile_boxes_and_windows_do_not_overflow_or_loop() {
        let index = index_of(vec![item(1, 1.0e300, 1.0e300, 1.0, 1.0), item(2, -1.0e300, 0.0, 5.0, 1.0)]);
        assert_eq!(index.len(), 2);
        let found = index.gather(&Rect { x0: -1.0e300, y0: -1.0e300, x1: 1.0e300, y1: 1.0e300 }, 10, &|_| false);
        assert!(found.examined <= 2);
        let found = index.gather(&Rect { x0: f64::NAN, y0: 0.0, x1: 1.0, y1: 1.0 }, 10, &|_| false);
        assert!(found.items.len() <= 2);
        assert!(Rect::round(&[(f64::NAN, 0.0)]).is_none() && Rect::round(&[]).is_none());
    }

    #[test]
    fn a_window_wider_than_the_bound_costs_the_bound_not_the_width() {
        // 10,000 shapes spread along a line a kilometre long; a window across all of it looks at the buckets round its middle only.
        let items: Vec<Item> = (0..10_000u32).map(|i| item(i, f64::from(i) * 0.1, 0.0, 0.05, 0.05)).collect();
        let index = index_of(items);
        let found = index.gather(&Rect { x0: 0.0, y0: -1.0, x1: 1000.0, y1: 1.0 }, 64, &|_| false);
        assert!(found.examined <= 48 * 10 + 1, "examined {}", found.examined);
        assert!(found.items.len() <= 64);
    }
}
