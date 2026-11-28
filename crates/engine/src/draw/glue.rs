//! Glue to the whole of a shape (a `Connect` row with `ToPart` 3).
//!
//! ours, the one place the rule is named. An end glued to the whole of a shape is at the connection point
//! of the shape nearest the connector's other end; a shape with no connection point has its outline for the place, and the end is at the point
//! of the outline nearest the other end (`nearest_spot`). When both ends are glued to the whole of a shape, the pair of places, one on each,
//! with the least distance between them (`pair`), is taken, which is the rule for the ends of a new connector.
//!
//! Our rule: the outline of a shape, for this, is the boundary of its box (`Width` by `Height` in its own frame, carried to the
//! page through its poses and its groups'). `ShapeFixedCode` is documented as a flag that makes a connector glue to a shape's alignment box in place
//! of its perimeter, so a box is a place the reference app glues to; the drawn curve of a round shape is not followed.

use std::collections::{BTreeMap, BTreeSet};

use diacraft_calc::{Budget, CellAddress, EvalCtx, Value as Calc};
use diacraft_doc::{CellAddr, CellEdit, CellWrite, Document, FormulaSlot, ShapeRef, SheetRef, Value, WriteOrigin};

use super::Problem;
use super::connectors::Which;
use super::read::{ConnectionPoint, GluedPoints, Reader};
use super::reroute::MAX_CROSSING_POINT_ROWS;
use super::route::{Dir, P};
use super::scan::Located;
use crate::stencil::number_text;

/// `ToPart` of a `Connect` row that glues to the whole of a shape.
pub(crate) const WHOLE_SHAPE: u32 = 3;

/// How many connection points of a shape are looked at when an end picks the nearest.
const MAX_SPOTS: usize = 256;

/// Distances squared and cosines closer than this are equal when two sides of a box are compared.
const NEAR_SIDE: f64 = 1.0e-9;

/// How far an end may be from a connection point and still be at it, in drawing units: a saved value is rounded, a point is not.
const AT_POINT: f64 = 1.0e-4;

fn sub(a: P, b: P) -> P {
    (a.0 - b.0, a.1 - b.1)
}

fn dot(a: P, b: P) -> f64 {
    a.0 * b.0 + a.1 * b.1
}

/// The corners of a shape's box on the page, in the order of its frame: lower left, lower right, upper right, upper left.
fn corners(reader: &Reader<'_>, at: &ShapeRef) -> [P; 4] {
    reader.corners(at)
}

/// The point of the segment `a` to `b` nearest `p`.
fn on_segment(a: P, b: P, p: P) -> P {
    let ab = sub(b, a);
    let length_squared = dot(ab, ab);
    if length_squared <= 0.0 || !length_squared.is_finite() {
        return a;
    }
    let t = (dot(sub(p, a), ab) / length_squared).clamp(0.0, 1.0);
    (a.0 + ab.0 * t, a.1 + ab.1 * t)
}

/// The point of the outline of shape `at` nearest `toward`, and the way out of the shape there: the normal of the side it is
/// on, pointing away from the middle of the box. At a corner, where two sides are as near, the side that faces `faces` more is taken (the larger
/// cosine of the angle between its normal and the way from the point to `faces`); the first of equals after that.
pub(crate) fn outline_nearest(reader: &Reader<'_>, at: &ShapeRef, toward: P, faces: P) -> Option<(P, P)> {
    let corners = corners(reader, at);
    if !corners.iter().all(|c| c.0.is_finite() && c.1.is_finite()) {
        return None;
    }
    let middle = ((corners[0].0 + corners[2].0) / 2.0, (corners[0].1 + corners[2].1) / 2.0);
    // The distance squared, the point, the way out and how much that way faces `toward`, for each side.
    let mut sides: Vec<(f64, P, P, f64)> = Vec::with_capacity(4);
    for i in 0..4 {
        let (a, b) = (corners[i], corners[(i + 1) % 4]);
        let q = on_segment(a, b, toward);
        let d = sub(q, toward);
        let side = sub(b, a);
        let normal = (side.1, -side.0);
        let out = if dot(normal, sub(q, middle)) >= 0.0 { normal } else { (-normal.0, -normal.1) };
        let length = out.0.hypot(out.1);
        let facing = if length > 0.0 { dot(out, sub(faces, q)) / length } else { 0.0 };
        sides.push((dot(d, d), q, out, facing));
    }
    let least = sides.iter().map(|s| s.0).fold(f64::INFINITY, f64::min);
    let mut best: Option<(P, P, f64)> = None;
    for (distance, q, out, facing) in &sides {
        if *distance <= least + NEAR_SIDE && best.is_none_or(|(_, _, f)| *facing > f + NEAR_SIDE) {
            best = Some((*q, *out, *facing));
        }
    }
    best.map(|(q, out, _)| (q, out))
}

/// A place an end can be glued.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Spot {
    at: P,
}

/// The places of shape `target`, whose connection points are `points`, for an end whose other end is at `toward`: each connection point (the first [`MAX_SPOTS`]),
/// or, for a shape with none, the one point of its outline nearest `toward`.
fn spots(reader: &Reader<'_>, target: &ShapeRef, points: &[ConnectionPoint], toward: P) -> Vec<Spot> {
    if points.is_empty() {
        return outline_nearest(reader, target, toward, toward).map(|(at, _)| Spot { at }).into_iter().collect();
    }
    points.iter().take(MAX_SPOTS).map(|p| Spot { at: (p.x, p.y) }).collect()
}

fn distance_squared(a: P, b: P) -> f64 {
    dot(sub(a, b), sub(a, b))
}

/// The place of `target` (whose connection points are `points`) nearest `other`: the first of equals.
fn nearest_spot(reader: &Reader<'_>, target: &ShapeRef, points: &[ConnectionPoint], other: P) -> Option<P> {
    let mut best: Option<(f64, P)> = None;
    for spot in spots(reader, target, points, other) {
        let d = distance_squared(spot.at, other);
        if best.is_none_or(|(least, _)| d < least) {
            best = Some((d, spot.at));
        }
    }
    best.map(|(_, at)| at)
}

/// The places for the two ends of a connector glued to the whole of two shapes: the pair, one place of each shape, with
/// the least distance between them, the first of equals. A shape with no connection point gives the point of its outline nearest the middle of the
/// other shape's box. `first_points` and `second_points` are the connection points of each.
fn pair(reader: &Reader<'_>, first: (&ShapeRef, &[ConnectionPoint]), second: (&ShapeRef, &[ConnectionPoint])) -> Option<(P, P)> {
    let middle = |at: &ShapeRef| {
        let c = corners(reader, at);
        ((c[0].0 + c[2].0) / 2.0, (c[0].1 + c[2].1) / 2.0)
    };
    let (a, b) = (spots(reader, first.0, first.1, middle(second.0)), spots(reader, second.0, second.1, middle(first.0)));
    let mut best: Option<(f64, P, P)> = None;
    for pa in &a {
        for pb in &b {
            let d = distance_squared(pa.at, pb.at);
            if best.is_none_or(|(least, _, _)| d < least) {
                best = Some((d, pa.at, pb.at));
            }
        }
    }
    best.map(|(_, x, y)| (x, y))
}

/// The way a connector leaves shape `target` at the place `at` on it, its other end being at `other`: a connection point's direction where `at` is
/// one, and the way out of the outline there where it is not (an end that was glued to the whole of the shape and put on its outline). `points` are the connection
/// points of `target`, read once for a call (the crossing tests and the routings of a call read them once for each shape, within a number of
/// rows, and have none for a shape past it, whose ends then leave by the outline).
pub(crate) fn exit_at_among(reader: &Reader<'_>, target: &ShapeRef, points: &[ConnectionPoint], at: P, other: P) -> Option<Dir> {
    let point = points.iter().find(|p| (p.x - at.0).abs() < AT_POINT && (p.y - at.1).abs() < AT_POINT);
    if let Some(p) = point {
        return Dir::from_vector(p.dir_x, p.dir_y);
    }
    // On the outline: the side `at` is on, and at a corner the side that faces the other end of the connector.
    let (_, out) = outline_nearest(reader, target, at, other)?;
    Dir::from_vector(out.0, out.1)
}

// ---- placing the ends calc cannot ----------------------------------------------------------------------------------------------------------

/// An end of a connector that the engine places: calc cannot work out its cells (`_WALKGLUE`: behaviour `unknown`), or it has no
/// formula at all, and its shape has changed.
#[derive(Clone, Copy, Debug)]
struct Unplaced {
    which: Which,
    target: u32,
    /// The `ToPart` of its `Connect` row: [`WHOLE_SHAPE`] or 100 plus a connection point's `IX`.
    part: u32,
}

/// Whether calc cannot give the end cell of `sheet` a value: its formula calls a function calc does not have, or it has none.
fn calc_cannot(doc: &Document, reader: &Reader<'_>, ctx: &mut EvalCtx, sheet: &SheetRef, which: Which) -> bool {
    let addr = CellAddr::direct(which.x_cell());
    let no_formula = reader
        .resolver()
        .resolve(sheet, &addr)
        .is_some_and(|found| found.formula.cell.is_none_or(|c| matches!(c.formula, FormulaSlot::Absent | FormulaSlot::NoFormula)));
    if no_formula {
        return true;
    }
    matches!(diacraft_calc::evaluate(doc, &CellAddress::new(sheet.clone(), addr), ctx), Calc::Unsupported(_))
}

/// The ends of the connectors glued to a shape in `reach`, or inside a shape the command changed (`inside`: the members of what it named, and not the
/// connector it names itself, whose glued end stays where the file put it, ROTATE-1D-1), that calc cannot work out, by connector: both ends of a
/// connector, whichever of its shapes changed, since the place nearest the other end changes with it. A glue to anything but the whole of a shape or a
/// connection point of it is left to the file.
fn unplaced_ends(
    doc: &Document,
    reader: &Reader<'_>,
    page: u32,
    followers: &BTreeSet<u32>,
    paths: &Located,
    steps: &mut u64,
    strict: bool,
) -> Result<BTreeMap<u32, Vec<Unplaced>>, Problem> {
    let mut found: BTreeMap<u32, Vec<Unplaced>> = BTreeMap::new();
    let Some(p) = doc.page(page) else { return Ok(found) };
    // One budget for the whole command, so that a page of formulas that are slow to work out is not worked out again for each end: `steps` of it, which the
    // caller shares with what else the command works out (a refit of a group routes the connectors glued to it within what its commit left).
    let mut ctx = EvalCtx::new(crate::measure::eval_options(), Budget::with_steps(*steps));
    for c in p.connects.iter().filter(|c| followers.contains(&c.from_sheet)) {
        let which = match c.from_cell.as_deref() {
            Some("BeginX" | "BeginY") => Which::Begin,
            Some("EndX" | "EndY") => Which::End,
            _ => continue,
        };
        let Some(part) = c.to_part.filter(|part| *part == WHOLE_SHAPE || *part >= super::connectors::TO_PART_BASE) else { continue };
        let list = found.entry(c.from_sheet).or_default();
        if list.iter().any(|u| u.which == which) {
            continue;
        }
        let Some(at) = paths.get(&c.from_sheet).cloned() else { continue };
        let sheet = SheetRef::Shape(at);
        // A connector DiaCraft made is glued to a connection point by formulas calc works out, and to the whole of a shape by `_WALKGLUE` formulas, which it cannot (the
        // end is placed here, as a file's is). A connector an `.odg` file holds carries the same marker (`User.DiaCraftRoute`: the reader lets DiaCraft route it again), and an end
        // of it glued to the whole of a shape holds plain values for the engine to place: it is placed as any file's is (it stayed
        // where the file had it).
        if (part == WHOLE_SHAPE || !reader.is_routed(&sheet)) && calc_cannot(doc, reader, &mut ctx, &sheet, which) {
            list.push(Unplaced { which, target: c.to_sheet, part });
        }
    }
    found.retain(|_, ends| !ends.is_empty());
    *steps = steps.saturating_sub(ctx.stats.steps);
    // With `strict`, a budget that ran out is the refusal the other recalculations give; without it the ends are left to calc, as they always were.
    if strict && ctx.stats.budget_exhausted {
        return Err(Problem::failed("the page is too large to recalculate"));
    }
    Ok(found)
}

/// The glue of each end of the connectors in `of`, from one walk of a page's `Connect` rows: by connector, and by end (0 the begin, 1 the end), the shape and the `ToPart` of
/// the end's first row.
fn glue_by_end<'c>(
    connects: impl Iterator<Item = &'c diacraft_doc::Connect>,
    of: &BTreeMap<u32, Vec<Unplaced>>,
) -> BTreeMap<u32, BTreeMap<u8, (u32, u32)>> {
    let mut out: BTreeMap<u32, BTreeMap<u8, (u32, u32)>> = BTreeMap::new();
    for c in connects.filter(|c| of.contains_key(&c.from_sheet)) {
        let key = match c.from_cell.as_deref() {
            Some("BeginX" | "BeginY") => 0u8,
            Some("EndX" | "EndY") => 1u8,
            _ => continue,
        };
        if let Some(part) = c.to_part {
            out.entry(c.from_sheet).or_default().entry(key).or_insert((c.to_sheet, part));
        }
    }
    out
}

/// The kind of an `.odg` connector, from its route cells as the `.odg` reader set them (`ShapeRouteStyle` 2 for `line` and
/// `lines`, `ConLineRouteExt` 2 for `curve`, which is placed as `standard` is) and, for a straight one, the `draw:type` the file wrote, which the reader holds (`lines` and
/// `line` read alike).
fn odg_kind(reader: &Reader<'_>, at: &ShapeRef) -> super::odg_glue::Kind {
    use super::odg_glue::Kind;
    let sheet = SheetRef::Shape(at.clone());
    if reader.number(&sheet, "ShapeRouteStyle") != 2.0 {
        return Kind::Standard;
    }
    let lines =
        reader.doc().shape(at).and_then(|s| s.ext.get()).is_some_and(|ext| {
            ext.odf.held.iter().any(|h| h.group == diacraft_doc::HeldGroup::Geometry && h.attrs.get("draw:type") == Some("lines"))
        });
    if lines { Kind::Lines } else { Kind::Line }
}

/// The target places an end on: the shape's four standard points, rows 0 to 3 of its `Connection` section as the `.odg` reader made them (the middles of
/// the sides of the frame as LibreOffice draws it, or of an outline's box), or the middles of the sides of its frame when it has not all four. `None` for a shape whose
/// section holds more rows than an end looks at, or whose points are no numbers.
fn odg_target(reader: &Reader<'_>, cache: &mut GluedPoints, id: u32, at: &ShapeRef) -> Option<super::odg_glue::Target> {
    let corners = corners(reader, at);
    let points = odg_points(reader, cache, id, at)?;
    let row = |ix: u32| points.iter().find(|p| p.index == ix).map(|p| (p.x, p.y));
    // A custom shape whose rows are its own glue points (`diacraft:glue="own"`) has no standard rows: rows 0 to 3 are its first four points, and the four
    // standard points LibreOffice puts a whole-shape end on are the middles of its frame's sides (CPDOUBLE.ODG).
    let own = reader.doc().shape(at).is_some_and(|shape| diacraft_odg::rows_are_listed_glue(shape));
    let standard = match (own, row(0), row(1), row(2), row(3)) {
        (false, Some(a), Some(b), Some(c), Some(d)) => [a, b, c, d],
        _ => {
            let mid = |p: P, q: P| ((p.0 + q.0) / 2.0, (p.1 + q.1) / 2.0);
            let [c0, c1, c2, c3] = corners;
            [mid(c0, c1), mid(c1, c2), mid(c2, c3), mid(c3, c0)]
        }
    };
    super::odg_glue::Target::of(standard, corners)
}

/// The connection points of shape `id` (at `at`), from the points read once for the call: `None` for a shape whose section holds more rows than an
/// end looks at ([`MAX_SPOTS`]), or more than the call had left.
fn odg_points<'c>(reader: &Reader<'_>, cache: &'c mut GluedPoints, id: u32, at: &ShapeRef) -> Option<&'c [ConnectionPoint]> {
    cache.of(reader, id, at).filter(|(rows, _)| *rows <= MAX_SPOTS).map(|(_, points)| points)
}

/// The way an end glued to connection point `index` of `target` (shape `id`), at `at` on the page, leaves the shape: the point's own direction on the
/// nearest page axis, or, for a point with none, the outward normal of the side of the target's box it is nearest (in shares of the box).
fn odg_way(
    reader: &Reader<'_>,
    cache: &mut GluedPoints,
    id: u32,
    target: &ShapeRef,
    box_of: &super::odg_glue::Target,
    index: u32,
    at: P,
) -> Option<P> {
    let own = odg_points(reader, cache, id, target)?.iter().find(|p| p.index == index).map(|p| (p.dir_x, p.dir_y));
    if let Some(way) = own.and_then(|d| super::odg_glue::axes_of(d)[0]) {
        return Some(way);
    }
    let (lo, hi) = (box_of.lo, box_of.hi);
    let (hw, hh) = ((hi.0 - lo.0) / 2.0, (hi.1 - lo.1) / 2.0);
    let (cx, cy) = ((lo.0 + hi.0) / 2.0, (lo.1 + hi.1) / 2.0);
    let share = |d: f64, half: f64| if half > 0.0 { d / half } else { 0.0 };
    super::odg_glue::axes_of((share(at.0 - cx, hw), share(at.1 - cy, hh)))[0]
}

/// The `.odg` glue rule for connector `at` of an `.odg` file (`User.DiaCraftRoute`, which the `.odg` reader gives every connector it reads): its ends glued to the whole of a shape go
/// where LibreOffice puts them when it loads the file (`odg_glue.rs`), and not where the rule for an end glued to a shape would. `glue` is the glue of each of its ends. `None` when the
/// rule has nothing to say (no end glued to the whole of a shape, both glued to one shape, a shape whose points cannot be read), and the ends are then placed as any file's.
fn odg_place(
    reader: &Reader<'_>,
    paths: &Located,
    cache: &mut GluedPoints,
    at: &ShapeRef,
    ends: &[Unplaced],
    glue: Option<&BTreeMap<u8, (u32, u32)>>,
) -> Option<Vec<(Which, P)>> {
    use super::odg_glue::{Other, pair, toward};
    let sheet = SheetRef::Shape(at.clone());
    let current = |which: Which| reader.parent_to_page(at, (reader.number(&sheet, which.x_cell()), reader.number(&sheet, which.y_cell())));
    let whole: Vec<&Unplaced> = ends.iter().filter(|e| e.part == WHOLE_SHAPE).collect();
    let kind = odg_kind(reader, at);
    match whole.as_slice() {
        [first, second] if first.target != second.target => {
            let (begin, end) = if first.which == Which::Begin { (first, second) } else { (second, first) };
            let a = odg_target(reader, cache, begin.target, paths.get(&begin.target)?)?;
            let b = odg_target(reader, cache, end.target, paths.get(&end.target)?)?;
            let (pa, pb) = pair(kind, &a, &b)?;
            Some(vec![(Which::Begin, pa), (Which::End, pb)])
        }
        [only] => {
            let target = odg_target(reader, cache, only.target, paths.get(&only.target)?)?;
            let other = match only.which {
                Which::Begin => Which::End,
                Which::End => Which::Begin,
            };
            let there = current(other);
            let toward_other = match glue.and_then(|g| g.get(&u8::from(other == Which::End))).copied() {
                Some((to, part)) if part >= super::connectors::TO_PART_BASE => {
                    let shape = paths.get(&to)?;
                    let box_of = odg_target(reader, cache, to, shape)?;
                    let way = odg_way(reader, cache, to, shape, &box_of, part.saturating_sub(super::connectors::TO_PART_BASE), there)?;
                    Other::Glued { at: there, way, lo: box_of.lo, hi: box_of.hi }
                }
                // Glued to the whole of the same shape, or by a glue no rule here knows: left.
                Some(_) => return None,
                None => Other::Free(there),
            };
            Some(vec![(only.which, toward(kind, &target, toward_other)?)])
        }
        _ => None,
    }
}

/// Where the engine puts the unplaced ends of connector `id`: an end glued to a connection point is at that point; ends glued
/// to the whole of a shape are at the places [`pair`] picks, or, with the other end placed or calc's, at the place nearest it ([`nearest_spot`]). The points of the
/// shapes come from `cache`, read once for the call.
fn place(reader: &Reader<'_>, paths: &Located, cache: &mut GluedPoints, at: &ShapeRef, ends: &[Unplaced]) -> Vec<(Which, P)> {
    let sheet = SheetRef::Shape(at.clone());
    // The end cells are in the frame of the connector's parent (a group's, for a member); the places are on the page.
    let current = |which: Which| reader.parent_to_page(at, (reader.number(&sheet, which.x_cell()), reader.number(&sheet, which.y_cell())));
    let mut spots: BTreeMap<u8, P> = BTreeMap::new();
    let key = |which: Which| u8::from(which == Which::End);
    for end in ends.iter().filter(|e| e.part != WHOLE_SHAPE) {
        let Some(target) = paths.get(&end.target) else { continue };
        let index = end.part.saturating_sub(super::connectors::TO_PART_BASE);
        if let Some(point) = cache.points(reader, end.target, target).iter().find(|p| p.index == index) {
            spots.insert(key(end.which), (point.x, point.y));
        }
    }
    let whole: Vec<&Unplaced> = ends.iter().filter(|e| e.part == WHOLE_SHAPE).collect();
    let targets: Vec<Option<ShapeRef>> = whole.iter().map(|e| paths.get(&e.target).cloned()).collect();
    let mut remaining: Vec<(&Unplaced, ShapeRef)> = Vec::new();
    for (end, target) in whole.iter().copied().zip(targets) {
        if let Some(target) = target {
            remaining.push((end, target));
        }
    }
    if let [(first, a), (second, b)] = remaining.as_slice()
        && first.target != second.target
        && let Some((pa, pb)) = {
            // Both are read first, then looked at together.
            cache.of(reader, first.target, a);
            cache.of(reader, second.target, b);
            pair(reader, (a, cache.held(first.target)), (b, cache.held(second.target)))
        }
    {
        spots.insert(key(first.which), pa);
        spots.insert(key(second.which), pb);
        remaining.clear();
    }
    for (end, target) in remaining {
        let other = match end.which {
            Which::Begin => Which::End,
            Which::End => Which::Begin,
        };
        let toward = spots.get(&key(other)).copied().unwrap_or_else(|| current(other));
        if let Some(spot) = nearest_spot(reader, &target, cache.points(reader, end.target, &target), toward) {
            spots.insert(key(end.which), spot);
        }
    }
    [Which::Begin, Which::End].into_iter().filter_map(|which| spots.get(&key(which)).map(|p| (which, *p))).collect()
}

/// Puts the ends of the connectors glued to a shape in `reach` that calc cannot work out where says, as the cached values of their
/// cells under the `Evaluation` origin, so the formula stays as the file had it, within `steps` of the evaluator (taken off as they are used); with
/// `strict`, a budget that runs out is refused in words. → whether any cell was written; the caller then recalculates what reads the ends.
pub(crate) fn place_ends_within(
    doc: &mut Document,
    page: u32,
    reach: &BTreeSet<u32>,
    inside: &BTreeSet<u32>,
    located: &mut Located,
    steps: &mut u64,
    strict: bool,
) -> Result<bool, Problem> {
    place_ends(doc, page, (reach, inside), located, (steps, strict), None)
}

/// Puts the ends `which` of connector `id`, which a command has just glued (a new connector, an end moved to a shape), where says (an `.odg` connector's by
/// `place_ends_within` decides by what the connector carries), as the next change to the shape it is glued to would: a connector made and a connector moved then agree,
/// and a shape with no connection point is glued to by one rule from the first. The other end of the connector is left where it is (ROTATE-1D-1: a command about the connector
/// keeps the ends it does not change). → whether a cell was written; the caller settles after, which works out what reads the ends.
pub(crate) fn place_new_ends(doc: &mut Document, page: u32, id: u32, which: &[Which]) -> Result<bool, Problem> {
    let mut located = Located::default();
    let mut steps = Budget::DEFAULT_STEPS;
    let inside: BTreeSet<u32> = std::iter::once(id).collect();
    place_ends(doc, page, (&BTreeSet::new(), &inside), &mut located, (&mut steps, false), Some(which))
}

/// [`place_ends_within`], for the ends `only` names when it is given. `follow` is `reach` and `inside`, and `budget` the steps and `strict`.
fn place_ends(
    doc: &mut Document,
    page: u32,
    follow: (&BTreeSet<u32>, &BTreeSet<u32>),
    located: &mut Located,
    budget: (&mut u64, bool),
    only: Option<&[Which]>,
) -> Result<bool, Problem> {
    let (reach, inside) = follow;
    let (steps, strict) = budget;
    let writes: Vec<(SheetRef, CellWrite)> = {
        let Some(p) = doc.page(page) else { return Ok(false) };
        let followers: BTreeSet<u32> =
            p.connects.iter().filter(|c| reach.contains(&c.to_sheet) || inside.contains(&c.from_sheet)).map(|c| c.from_sheet).collect();
        if followers.is_empty() {
            return Ok(false);
        }
        // The connectors and the shapes they are glued to, looked up with the rest of the settling (`located`: one walk of the page for those not looked up
        // yet) and not a search each (a guide that carries a thousand shapes puts them all in `reach`).
        let wanted: Vec<u32> = p.connects.iter().filter(|c| followers.contains(&c.from_sheet)).flat_map(|c| [c.from_sheet, c.to_sheet]).collect();
        located.find(doc, page, wanted)?;
        let paths: &Located = located;
        let reader = Reader::new(doc);
        let mut writes = Vec::new();
        // The connection points of the shapes the ends are glued to, each read once for the call (once for each end, ten thousand ends on a shape of
        // ten thousand points were a hundred million rows), within [`MAX_CROSSING_POINT_ROWS`] rows for all of them.
        let mut cache = GluedPoints::new(MAX_CROSSING_POINT_ROWS);
        let unplaced = unplaced_ends(doc, &reader, page, &followers, paths, steps, strict)?;
        // The glue of every end of those connectors, for the other end: one walk of the page's rows.
        let glue = glue_by_end(p.connects.iter(), &unplaced);
        for (id, ends) in unplaced {
            let Some(at) = paths.get(&id) else { continue };
            let sheet = SheetRef::Shape(at.clone());
            // An `.odg` connector's ends glued to the whole of a shape go where LibreOffice puts them; every other end, and one the rule says nothing of.
            let placed = if reader.is_routed(&sheet) { odg_place(&reader, paths, &mut cache, at, &ends, glue.get(&id)) } else { None };
            for (which, page_spot) in placed.unwrap_or_else(|| place(&reader, paths, &mut cache, at, &ends)) {
                if only.is_some_and(|only| !only.contains(&which)) {
                    continue;
                }
                // Into the frame the cells are in (a connector in a group has them in the group's).
                let spot = reader.point_from_page(at, page_spot);
                for (name, value) in [(which.x_cell(), spot.0), (which.y_cell(), spot.1)] {
                    if !value.is_finite() || (reader.number(&sheet, name) - value).abs() <= 1.0e-9 {
                        continue;
                    }
                    let edit = CellEdit::Cached { value: Value::from_text(&number_text(value)), error: None, unit: None };
                    writes.push((sheet.clone(), CellWrite { addr: CellAddr::direct(name), edit, origin: WriteOrigin::Evaluation }));
                }
            }
        }
        writes
    };
    if writes.is_empty() {
        return Ok(false);
    }
    doc.write_batch(writes)?;
    Ok(true)
}
