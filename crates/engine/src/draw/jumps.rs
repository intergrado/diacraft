//! Line jumps: where two connectors cross, one of them hops over the other (
//! and its sisters).
//!
//! (the rules below are the specifications' them for M3,
//! with the project rules they name). A jump is rows of the connector that carries it, written when connectors are routed, never at open
//! and never drawn by the renderer.
//!
//! * **Who takes part.** Connectors that are top-level shapes with a path of lines: made by DiaCraft or glued at an end, one-dimensional, with
//!   `MoveTo`, `LineTo`, `ArcTo` and `PolylineTo` rows only. A curved connector has none and gets none; a line that is not a connector
//!   takes no part. At most [`MAX_PARTICIPANTS`]; a page with more has no jumps made. **What the pass reads**: the routes of the page's connectors,
//!   and the old routes of those routed, are read within [`MAX_JUMP_ROWS`] rows for the pass, counted before any is merged; a route of more than a route is read for
//!   (4,096 rows) takes no part and is not read, and **a page whose routes are more than the pass may read has no jumps made**, as a page of too many connectors has
//!   none (the pass read every route whole each time a connector was routed: ten thousand routes of 4,000 rows were 16 to 40 million rows for the move of one).
//! * **When.** After a command routes connectors (`settle`, a hand edit) the routed ones are rebuilt, and so is every connector whose legs cross the
//!   old or the new route of one of them (a neighbour whose crossings changed) **that has a hop to make or take away**: one that has no hop and
//!   needs none (its rows and its new path are a `MoveTo` and `LineTo` rows only) keeps the rows it has, however they are written; no other connector is
//!   touched (the `.odg` save relies on it). A command that changes the page's jump cells rebuilds every connector. Each is
//!   rebuilt from the route it has with its jumps taken out (the rows' ends with the points on a leg dropped) and the other connectors' likewise.
//! * **A connector on a locked layer** is never rewritten, a rebuild of every connector included: it takes no jump and loses none, and a
//!   connector that crosses it carries a jump over it only where the code below gives that one the jump.
//! * **Which connector carries it** (the page's `LineJumpCode`, absent as in every file's root: 1): 0 none; 1 the flatter leg of the two, 2 the steeper;
//!   3, 4 the connector higher in the page's drawing order (3, the last one routed, stands in the order); 5 the lower; 6 and the rest none. A tie
//!   in steepness goes to the higher in drawing order. A connector's `ConLineJumpCode` overrides: 1 never carries, 2 carries where the page gives it to
//!   nobody, 3 never carries and the connector it crosses does instead, 4 neither carries.
//! * **Its size.** A chord of `LineToLineX` times `LineJumpFactorX` on a level leg and a slanted one, `LineToLineY` times `LineJumpFactorY` on an
//!   upright one (in drawing units); absent, 0.125 and 2/3, as the root holds them. No jump of no size.
//! * **Room.** A jump lies at least half a chord from both ends of its leg, and chords on a leg do not overlap: the later of two closer than
//!   a chord is not made.
//! * **Its look** (`LineJumpStyle`, `ConLineJumpStyle` overriding when not 0): an arc (0, 1, and 5 to 9 by the project rule), a gap (2: a `MoveTo`),
//!   a square (3: a `PolylineTo` with a control point half a chord above each end of the chord), two sides (4: a `PolylineTo` with one control point half
//!   a chord off the crossing). **Its side**: up on a level or slanted leg and left on an upright one by default; `PageLineJumpDirX` and
//!   `ConLineJumpDirX` 2 put it down, `PageLineJumpDirY` and `ConLineJumpDirY` 2 right. An arc's `A` is the half chord, negative to the left of the
//!   way the leg runs, and carried into the connector's own frame (a level leg running toward -x gets +).

use std::collections::{BTreeSet, HashSet};
use std::sync::Arc;

use diacraft_doc::{CellAddr, Document, Owner, Row, ShapeRef, SheetRef};

use super::Problem;
use super::connectors::{inherited_rows, route_section_rows, write_route};
use super::hand::{MAX_ROWS, route_of_rows};
use super::read::{Pose, Reader};
use super::route::P;
use super::rows::{self, Seg};
use super::scan;
use super::style::{PathStyle, path_style};

/// The most connectors that take part; a page with more has no jumps made, so that the work is the connectors and their crossings and no more.
pub const MAX_PARTICIPANTS: usize = 2_000;

/// The size of a jump when the page's cells are absent, as the root style of every file holds them (0.125 inch, 2/3).
const DEFAULT_SPACING: f64 = 0.125;
pub(super) const DEFAULT_FACTOR: f64 = 2.0 / 3.0;

/// The most legs a connector that takes part has, and the most points all of them have between them: a connector with more is no route a person
/// drew or an application made, and a page of them has no jumps made.
const MAX_LEGS: usize = 256;
const MAX_TOTAL_POINTS: usize = 100_000;

/// The most pairs of legs one rebuild tests for a crossing; past it nothing is written (so that the work is bounded whatever the file holds).
const MAX_WORK: u64 = 4_000_000;

/// The most rows of routes one rebuild reads: the routes of the connectors that take part and the old routes of those routed, counted over each
/// connector and its master before any row is merged. Four rows for each of the [`MAX_TOTAL_POINTS`] points a pass takes, room for the jumps on the legs; past it
/// nothing is written.
pub const MAX_JUMP_ROWS: usize = 4 * MAX_TOTAL_POINTS;

/// Takes `n` pairs of legs from the budget: `false` when it holds fewer.
fn spend(work: &mut u64, n: usize) -> bool {
    let n = u64::try_from(n).unwrap_or(u64::MAX);
    if *work < n {
        return false;
    }
    *work -= n;
    #[cfg(test)]
    PAIRS.with(|c| c.set(c.get().saturating_add(n)));
    true
}

// The pairs of legs the tests of this file have tested, for the tests that hold the work of a rebuild to a count.
#[cfg(test)]
thread_local! {
    static PAIRS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// A crossing nearer than this to an end of a leg is the leg meeting another, not crossing it.
const END: f64 = 1.0e-9;

/// The page's jump settings (and a connector's overrides, read beside them).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Settings {
    pub code: u32,
    pub style: u32,
    pub factor: (f64, f64),
    pub spacing: (f64, f64),
    pub dir: (u32, u32),
}

fn whole(n: f64) -> u32 {
    if n.is_finite() && n >= 0.0 { n.round().min(1000.0) as u32 } else { 0 }
}

impl Settings {
    /// The cells of page `page`; a cell found nowhere is the root's value (every file's root holds `LineJumpCode` 1).
    pub(crate) fn of_page(reader: &Reader<'_>, page: u32) -> Settings {
        let sheet = SheetRef::Page(page);
        let get = |name: &str, default: f64| if reader.is_set(&sheet, &CellAddr::direct(name)) { reader.number(&sheet, name) } else { default };
        Settings {
            code: whole(get("LineJumpCode", 1.0)),
            style: whole(get("LineJumpStyle", 0.0)),
            factor: (get("LineJumpFactorX", DEFAULT_FACTOR), get("LineJumpFactorY", DEFAULT_FACTOR)),
            spacing: (get("LineToLineX", DEFAULT_SPACING), get("LineToLineY", DEFAULT_SPACING)),
            dir: (whole(get("PageLineJumpDirX", 0.0)), whole(get("PageLineJumpDirY", 0.0))),
        }
    }
}

/// One connector that takes part.
struct Part {
    id: u32,
    at: ShapeRef,
    /// Its place in the page's drawing order: higher is nearer the top.
    stack: usize,
    pose: Pose,
    points: Vec<P>,
    bbox: (f64, f64, f64, f64),
    code: u32,
    style: u32,
    dir: (u32, u32),
    follow_width: bool,
}

fn extent(points: &[P]) -> (f64, f64, f64, f64) {
    points
        .iter()
        .fold((f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY), |b, p| (b.0.min(p.0), b.1.min(p.1), b.2.max(p.0), b.3.max(p.1)))
}

fn overlap(a: (f64, f64, f64, f64), b: (f64, f64, f64, f64)) -> bool {
    a.0 <= b.2 && b.0 <= a.2 && a.1 <= b.3 && b.1 <= a.3
}

/// Whether the last of a route's rows has an `X` of `Width*1`, as a fixed route's does (and a hand edit's).
fn follows_width(rows: &[diacraft_doc::EffectiveRow<'_>]) -> bool {
    rows.last()
        .and_then(|row| row.cells.iter().find(|(n, _)| n.as_str() == "X"))
        .and_then(|(_, found)| found.formula.cell?.formula.text())
        .is_some_and(|text| text.replace(' ', "").eq_ignore_ascii_case("Width*1"))
}

/// The route of the connector at `sheet`, as [`base_route`] gives it, and whether its last row follows `Width` ([`follows_width`]), read once and charged to `rows_left`
/// `None` when the rows it holds, counted over the connector and its master before any is merged, are more than are left: the pass is given up.
/// `Some(None)` for a connector that takes no part: no route, more rows than a route is read for (refused before any is read), or rows that are not a path of lines.
fn route_read(reader: &Reader<'_>, sheet: &SheetRef, pose: &Pose, rows_left: &mut usize) -> Option<Option<(Vec<P>, bool)>> {
    let key = diacraft_doc::SectionKey::new("Geometry", Some(0));
    let Some(held) = reader.resolver().rows_held(sheet, &key) else { return Some(None) };
    if held > MAX_ROWS {
        return Some(None);
    }
    *rows_left = rows_left.checked_sub(held)?;
    let Some(rows) = reader.resolver().effective_rows(sheet, &key) else { return Some(None) };
    Some(route_of_rows(&rows, pose).map(|points| (points, follows_width(&rows))))
}

/// The connectors of page `page` that take part, in drawing order, found in one walk of its top-level shapes, their routes read within `rows_left`. `None` when there
/// are more than [`MAX_PARTICIPANTS`], or their routes hold more rows than are left.
fn participants(doc: &Document, reader: &Reader<'_>, page: u32, rows_left: &mut usize) -> Option<Vec<Part>> {
    let p = doc.page(page)?;
    let glued: HashSet<u32> =
        p.connects.iter().filter(|c| matches!(c.from_cell.as_deref(), Some("BeginX" | "BeginY" | "EndX" | "EndY"))).map(|c| c.from_sheet).collect();
    let mut parts = Vec::new();
    let mut total_points = 0_usize;
    for (stack, shape) in p.shapes.iter().enumerate() {
        // A connector is glued at an end or made by DiaCraft (a `User` row): the cheap test before any cell is looked up.
        if !glued.contains(&shape.id) && !shape.sheet.sections.iter().any(|s| s.name.as_str() == "User") {
            continue;
        }
        let at = ShapeRef { owner: Owner::Page(page), path: vec![u32::try_from(stack).ok()?] };
        let sheet = SheetRef::Shape(at.clone());
        let ours = reader.is_routed(&sheet);
        if !reader.is_one_d(&sheet) || !(ours || glued.contains(&shape.id)) {
            continue;
        }
        if matches!(path_style(reader, page, &sheet), PathStyle::Curved) {
            continue;
        }
        let pose = reader.pose(&sheet);
        let Some((points, follow_width)) = route_read(reader, &sheet, &pose, rows_left)? else { continue };
        if points.len() > MAX_LEGS + 1 {
            continue;
        }
        total_points += points.len();
        if parts.len() >= MAX_PARTICIPANTS || total_points > MAX_TOTAL_POINTS {
            return None;
        }
        let con = |name: &str| whole(reader.number(&sheet, name));
        parts.push(Part {
            id: shape.id,
            at,
            stack,
            bbox: extent(&points),
            pose,
            points,
            code: con("ConLineJumpCode"),
            style: con("ConLineJumpStyle"),
            dir: (con("ConLineJumpDirX"), con("ConLineJumpDirY")),
            follow_width,
        });
    }
    Some(parts)
}

/// Where two legs cross, if they cross at a point inside both: the point and how far along each (0 to 1).
fn crossing(a: (P, P), b: (P, P)) -> Option<(P, f64, f64)> {
    let (r, s) = ((a.1.0 - a.0.0, a.1.1 - a.0.1), (b.1.0 - b.0.0, b.1.1 - b.0.1));
    let denominator = r.0 * s.1 - r.1 * s.0;
    let scale = (r.0.hypot(r.1) * s.0.hypot(s.1)).max(1.0e-300);
    if denominator.abs() < 1.0e-9 * scale {
        return None;
    }
    let q = (b.0.0 - a.0.0, b.0.1 - a.0.1);
    let (t, u) = ((q.0 * s.1 - q.1 * s.0) / denominator, (q.0 * r.1 - q.1 * r.0) / denominator);
    let inside = |v: f64| v > END && v < 1.0 - END;
    (inside(t) && inside(u)).then_some(((a.0.0 + r.0 * t, a.0.1 + r.1 * t), t, u))
}

/// How steep a leg is: 0 for a level one, 1 for an upright one.
fn steepness(leg: (P, P)) -> f64 {
    let (dx, dy) = ((leg.1.0 - leg.0.0).abs(), (leg.1.1 - leg.0.1).abs());
    if dx + dy > 0.0 { dy / (dx + dy) } else { 0.0 }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    First,
    Second,
}

/// The connector of the pair that the page's `LineJumpCode` gives the jump to: `a` and `b` are the two legs, `upper` says which of the two
/// connectors is higher in the drawing order.
fn page_choice(code: u32, a: (P, P), b: (P, P), upper: Side) -> Option<Side> {
    let (sa, sb) = (steepness(a), steepness(b));
    let lesser = |less: bool| {
        if (sa - sb).abs() < 1.0e-9 {
            Some(upper)
        } else if (sa < sb) == less {
            Some(Side::First)
        } else {
            Some(Side::Second)
        }
    };
    match code {
        1 => lesser(true),
        2 => lesser(false),
        3 | 4 => Some(upper),
        5 => Some(if upper == Side::First { Side::Second } else { Side::First }),
        _ => None,
    }
}

/// Applies the connectors' own codes to the page's choice (with the page's choice winning a conflict).
fn carrier(choice: Option<Side>, first: u32, second: u32) -> Option<Side> {
    let code = |side: Side| if side == Side::First { first } else { second };
    let other = |side: Side| if side == Side::First { Side::Second } else { Side::First };
    let may = |side: Side| first != 4 && second != 4 && !matches!(code(side), 1 | 3);
    let pushed = |side: Side| code(side) == 2 || code(other(side)) == 3;
    match choice {
        Some(side) if may(side) => Some(side),
        Some(side) => Some(other(side)).filter(|o| may(*o) && pushed(*o)),
        None => [Side::First, Side::Second].into_iter().find(|s| may(*s) && pushed(*s)),
    }
}

/// A jump on a leg: how far along it the crossing is (a length), the leg's look and side.
struct Hop {
    along: f64,
}

/// The unit vector of a leg, from its start to its end.
fn unit(leg: (P, P)) -> Option<(P, f64)> {
    let (dx, dy) = (leg.1.0 - leg.0.0, leg.1.1 - leg.0.1);
    let length = dx.hypot(dy);
    (length > END && length.is_finite()).then(|| ((dx / length, dy / length), length))
}

/// The jumps of every leg of `me`, as a path in the page's frame: its legs with the hops it carries. `None` when the budget of `work`
/// is spent.
fn path_of(me: &Part, others: &[&Part], page: &Settings, work: &mut u64) -> Option<Vec<Seg>> {
    let style = if me.style == 0 { page.style } else { me.style };
    let dir = (if me.dir.0 == 0 { page.dir.0 } else { me.dir.0 }, if me.dir.1 == 0 { page.dir.1 } else { me.dir.1 });
    // The pose carries the page into the connector's frame; a mirrored one swaps left and right.
    let mirrored = me.pose.flip_x != me.pose.flip_y;
    let mut segs: Vec<Seg> = Vec::new();
    let Some(first) = me.points.first() else { return Some(segs) };
    segs.push(Seg::Move(*first));
    // The connectors whose box meets this one's: the only ones a leg can cross.
    let reach = (me.bbox.0 - 1.0e-9, me.bbox.1 - 1.0e-9, me.bbox.2 + 1.0e-9, me.bbox.3 + 1.0e-9);
    let near: Vec<&&Part> = others.iter().filter(|other| overlap(reach, other.bbox)).collect();
    for w in me.points.windows(2) {
        let (from, to) = (w[0], w[1]);
        let leg = (from, to);
        let Some((t, length)) = unit(leg) else {
            segs.push(Seg::Line(to));
            continue;
        };
        let upright = (to.0 - from.0).abs() < END;
        let (spacing, factor) = if upright { (page.spacing.1, page.factor.1) } else { (page.spacing.0, page.factor.0) };
        let chord = spacing * factor;
        let half = chord / 2.0;
        let mut hops: Vec<Hop> = Vec::new();
        if chord.is_finite() && chord > END {
            for other in &near {
                if !spend(work, other.points.len()) {
                    return None;
                }
                let upper = if me.stack > other.stack { Side::First } else { Side::Second };
                for o in other.points.windows(2) {
                    let Some((_, ta, _)) = crossing(leg, (o[0], o[1])) else { continue };
                    let choice = page_choice(page.code, leg, (o[0], o[1]), upper);
                    if carrier(choice, me.code, other.code) == Some(Side::First) {
                        hops.push(Hop { along: ta * length });
                    }
                }
            }
        }
        hops.sort_by(|a, b| a.along.total_cmp(&b.along));
        let mut last_end = f64::NEG_INFINITY;
        for hop in &hops {
            if hop.along < half || length - hop.along < half || hop.along - half < last_end + END {
                continue;
            }
            last_end = hop.along + half;
            let (start, centre, end) = (
                (from.0 + t.0 * (hop.along - half), from.1 + t.1 * (hop.along - half)),
                (from.0 + t.0 * hop.along, from.1 + t.1 * hop.along),
                (from.0 + t.0 * (hop.along + half), from.1 + t.1 * (hop.along + half)),
            );
            // The side the jump bulges to, on the page: up (or down) on a level or slanted leg, left (or right) on an upright one.
            let side: P = if upright {
                if dir.1 == 2 { (1.0, 0.0) } else { (-1.0, 0.0) }
            } else {
                let normal = (-t.1, t.0);
                let up = if normal.1 >= 0.0 { normal } else { (-normal.0, -normal.1) };
                if dir.0 == 2 { (-up.0, -up.1) } else { up }
            };
            segs.push(Seg::Line(start));
            let left = (-t.1, t.0);
            let page_bow = if side.0 * left.0 + side.1 * left.1 > 0.0 { -half } else { half };
            let bow = if mirrored { -page_bow } else { page_bow };
            segs.push(match style {
                2 => Seg::Move(end),
                3 => Seg::Poly {
                    to: end,
                    via: vec![(start.0 + side.0 * half, start.1 + side.1 * half), (end.0 + side.0 * half, end.1 + side.1 * half)],
                },
                4 => Seg::Poly { to: end, via: vec![(centre.0 + side.0 * half, centre.1 + side.1 * half)] },
                _ => Seg::Arc { to: end, bow },
            });
        }
        segs.push(Seg::Line(to));
    }
    Some(segs)
}

/// A path's points carried from the page's frame into the connector's own.
fn local(segs: &[Seg], pose: &Pose) -> Vec<Seg> {
    let down = |p: &P| pose.down(*p);
    segs.iter()
        .map(|seg| match seg {
            Seg::Move(p) => Seg::Move(down(p)),
            Seg::Line(p) => Seg::Line(down(p)),
            Seg::Arc { to, bow } => Seg::Arc { to: down(to), bow: *bow },
            Seg::Poly { to, via } => Seg::Poly { to: down(to), via: via.iter().map(down).collect() },
            Seg::Spline { to, via } => Seg::Spline { to: down(to), via: via.iter().map(down).collect() },
        })
        .collect()
}

/// The rows of `me` now, as the section holds them (with the rows its master gives it and the route does not use marked deleted).
fn current_rows(doc: &Document, at: &ShapeRef) -> Option<Vec<Arc<Row>>> {
    let shape = doc.shape(at)?;
    let section = shape.sheet.sections.iter().find(|s| s.name.as_str() == "Geometry" && s.ix == Some(0))?;
    Some(section.rows.iter().cloned().collect())
}

/// How many rows the route section of the connector at `at` holds, without taking them.
fn route_row_count(doc: &Document, at: &ShapeRef) -> usize {
    doc.shape(at)
        .and_then(|shape| shape.sheet.sections.iter().find(|s| s.name.as_str() == "Geometry" && s.ix == Some(0)))
        .map_or(0, |section| section.rows.len())
}

/// Whether a path is one start and straight legs, with no hop.
fn is_plain(path: &[Seg]) -> bool {
    !path.is_empty() && path.iter().enumerate().all(|(i, seg)| if i == 0 { matches!(seg, Seg::Move(_)) } else { matches!(seg, Seg::Line(_)) })
}

/// Whether rows are a `MoveTo` and `LineTo` rows only: a route with no hop.
fn rows_are_plain(rows: &[Arc<Row>]) -> bool {
    !rows.is_empty() && rows.iter().enumerate().all(|(i, row)| row.row_type.as_deref() == Some(if i == 0 { "MoveTo" } else { "LineTo" }))
}

/// A connector's route to write: where it is, its rows, and the rows its master gives it.
type Rewrite = (Vec<u32>, Vec<Arc<Row>>, Option<Vec<u32>>);

/// The route of connector `id` in `doc` before a command, found with the others in one walk, read within `rows_left`: `None` when they hold more rows than are left.
fn old_routes(before: &Document, page: u32, ids: &[u32], rows_left: &mut usize) -> Option<Vec<Vec<P>>> {
    let wanted: HashSet<u32> = ids.iter().copied().collect();
    let Ok(found) = scan::scan(before, page, &wanted) else { return Some(Vec::new()) };
    let reader = Reader::new(before);
    let mut routes = Vec::new();
    for at in found.found.values().filter(|at| at.path.len() == 1) {
        let sheet = SheetRef::Shape(at.clone());
        if !reader.is_one_d(&sheet) {
            continue;
        }
        if let Some((points, _)) = route_read(&reader, &sheet, &reader.pose(&sheet), rows_left)? {
            routes.push(points);
        }
    }
    Some(routes)
}

/// Whether any leg of `points` crosses any leg of `route`; `None` when the budget of `work` is spent.
fn crosses(points: &[P], route: &[P], work: &mut u64) -> Option<bool> {
    if !spend(work, points.len().saturating_mul(route.len())) {
        return None;
    }
    Some(points.windows(2).any(|a| route.windows(2).any(|b| crossing((a[0], a[1]), (b[0], b[1])).is_some())))
}

/// Rebuilds the jumps after connectors `rerouted` were routed (or removed): theirs, and those of every connector that crosses their old route (in
/// `before`) or their new one. With `all` every connector's are rebuilt (the page's jump cells changed).
pub(crate) fn refresh(doc: &mut Document, before: &Document, page: u32, rerouted: &[u32], all: bool) -> Result<(), Problem> {
    let writes = {
        let reader = Reader::new(doc);
        let settings = Settings::of_page(&reader, page);
        let mut rows_left = MAX_JUMP_ROWS;
        let Some(parts) = participants(doc, &reader, page, &mut rows_left) else { return Ok(()) };
        // With the page's code at 0 only a connector with a code of its own (2, always) can carry a jump, and none does at an edit that changes
        // none: nothing to rebuild unless the page's cells were just changed.
        if !all && settings.code == 0 && !parts.iter().any(|p| p.code != 0) {
            return Ok(());
        }
        let mut work = MAX_WORK;
        let changed: BTreeSet<u32> = rerouted.iter().copied().collect();
        let mut affected: BTreeSet<usize> = BTreeSet::new();
        if all {
            affected.extend(0..parts.len());
        } else {
            let Some(mut routes) = old_routes(before, page, rerouted, &mut rows_left) else { return Ok(()) };
            for (i, part) in parts.iter().enumerate() {
                if changed.contains(&part.id) {
                    affected.insert(i);
                    routes.push(part.points.clone());
                }
            }
            for route in &routes {
                let bbox = extent(route);
                for (i, part) in parts.iter().enumerate() {
                    if affected.contains(&i) || !overlap(bbox, part.bbox) {
                        continue;
                    }
                    match crosses(&part.points, route, &mut work) {
                        Some(true) => {
                            affected.insert(i);
                        }
                        Some(false) => {}
                        None => return Ok(()),
                    }
                }
            }
        }
        let mut writes: Vec<Rewrite> = Vec::new();
        let everyone: Vec<&Part> = parts.iter().collect();
        // A connector on a locked layer takes no jump and loses none: its route is rows of its own, which a locked shape does not have written.
        // It still crosses the others, which carry a jump over it as the page's code gives them one.
        let mut locks = super::layers::Locks::default();
        for &i in &affected {
            let Some(me) = parts.get(i) else { continue };
            if let Some(shape) = doc.shape(&me.at)
                && locks.of(doc, page, shape)?.is_some()
            {
                continue;
            }
            let others: Vec<&Part> = everyone.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, p)| *p).collect();
            let Some(path) = path_of(me, &others, &settings, &mut work) else { return Ok(()) };
            // A connector the command did not route, which the rebuild reached because its legs cross the old or the new route of one that was, is rewritten
            // only when its route changes: a hop is made or taken away. One that has no hop and needs none keeps the rows it has, though they are not the rows
            // this writes (a route a file made, or a point on a leg, which a rebuild takes out): rewriting them changed a connector nobody had touched, and
            // a save of the drawing wrote its path again (class F2).
            if !all && !changed.contains(&me.id) && is_plain(&path) {
                // The look at the rows is charged to the budget of the rebuild, before it is made.
                if !spend(&mut work, route_row_count(doc, &me.at)) {
                    return Ok(());
                }
                if current_rows(doc, &me.at).as_deref().is_some_and(rows_are_plain) {
                    continue;
                }
            }
            let segs = local(&path, &me.pose);
            let inherited = inherited_rows(&reader, &me.at);
            let rows = rows::rows(&segs, me.follow_width);
            if current_rows(doc, &me.at).as_deref() != Some(route_section_rows(rows.clone(), &inherited).as_slice()) {
                writes.push((me.at.path.clone(), rows, inherited));
            }
        }
        writes
    };
    for (path, rows, inherited) in writes {
        write_route(doc, page, &path, rows, &inherited)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(id: u32, stack: usize, points: &[P], pose: Pose) -> Part {
        Part {
            id,
            at: ShapeRef { owner: Owner::Page(1), path: vec![u32::try_from(stack).unwrap()] },
            stack,
            pose,
            points: points.to_vec(),
            bbox: extent(points),
            code: 0,
            style: 0,
            dir: (0, 0),
            follow_width: false,
        }
    }

    fn unturned() -> Pose {
        Pose { pin: (0.0, 0.0), loc: (0.0, 0.0), angle: 0.0, flip_x: false, flip_y: false }
    }

    fn settings() -> Settings {
        Settings { code: 1, style: 0, factor: (0.5, 0.5), spacing: (0.2, 0.2), dir: (0, 0) }
    }

    /// A route row of the type `kind`.
    fn row(kind: &str) -> Arc<Row> {
        Arc::new(Row { row_type: Some(Arc::from(kind)), ..Row::default() })
    }

    /// Class F2: a connector that no command routed is rewritten by a rebuild only when a hop is made or taken away, and the test is what it has and
    /// what it would be given: a path of a start and straight legs, and rows of a `MoveTo` and `LineTo` rows.
    #[test]
    fn a_route_has_a_hop_when_it_is_not_a_start_and_straight_legs_and_its_rows_when_they_are_not_a_move_and_lines() {
        let line = |p: P| Seg::Line(p);
        assert!(is_plain(&[Seg::Move((0.0, 0.0)), line((1.0, 0.0)), line((1.0, 1.0))]));
        assert!(is_plain(&[Seg::Move((0.0, 0.0))]));
        assert!(!is_plain(&[]), "a path of no start is not one");
        assert!(!is_plain(&[line((0.0, 0.0)), line((1.0, 0.0))]), "a path that starts with a leg");
        assert!(!is_plain(&[Seg::Move((0.0, 0.0)), line((1.0, 0.0)), Seg::Arc { to: (2.0, 0.0), bow: 0.1 }, line((3.0, 0.0))]), "an arc is a hop");
        assert!(!is_plain(&[Seg::Move((0.0, 0.0)), line((1.0, 0.0)), Seg::Move((2.0, 0.0)), line((3.0, 0.0))]), "a gap is a hop, and a second start");
        assert!(!is_plain(&[Seg::Move((0.0, 0.0)), Seg::Poly { to: (2.0, 0.0), via: vec![(1.0, 0.5)] }]));
        assert!(rows_are_plain(&[row("MoveTo"), row("LineTo"), row("LineTo")]));
        assert!(rows_are_plain(&[row("MoveTo")]));
        assert!(!rows_are_plain(&[]));
        assert!(!rows_are_plain(&[row("LineTo"), row("LineTo")]), "no `MoveTo` first");
        assert!(!rows_are_plain(&[row("MoveTo"), row("LineTo"), row("ArcTo"), row("LineTo")]), "an arc is a hop");
        assert!(!rows_are_plain(&[row("MoveTo"), row("MoveTo"), row("LineTo")]), "a gap");
        assert!(!rows_are_plain(&[row("MoveTo"), row("PolylineTo")]));
        assert!(!rows_are_plain(&[Arc::new(Row::default()), row("LineTo")]), "a row with no type");
    }

    /// The rows a rebuild looks at before it keeps a connector's rows are counted from the route section (`Geometry` of `ix` 0), and not from another
    /// section of the shape, and a shape that is not there or has no route holds none.
    #[test]
    fn the_rows_of_a_connector_s_route_are_counted_from_its_route_section_and_no_other() {
        let document = |sections: &[(&str, Option<u32>, usize)]| {
            let mut shape = diacraft_doc::Shape::new(7);
            let sheet = Arc::make_mut(&mut shape.sheet);
            for (name, ix, rows) in sections {
                let mut section = diacraft_doc::Section::new(name, *ix);
                for _ in 0..*rows {
                    section.rows.push(Arc::new(Row::default()));
                }
                sheet.sections.push(Arc::new(section));
            }
            let mut page = diacraft_doc::Page::new(1, "P");
            page.shapes = diacraft_doc::ShapeList::from_vec(vec![Arc::new(shape)]);
            let mut doc = Document::empty();
            doc.pages.push(Arc::new(page));
            doc
        };
        let at = ShapeRef { owner: Owner::Page(1), path: vec![0] };
        assert_eq!(route_row_count(&document(&[("Geometry", Some(0), 5)]), &at), 5);
        assert_eq!(route_row_count(&document(&[("Geometry", Some(1), 3), ("Geometry", Some(0), 5)]), &at), 5, "the route is the section of index 0");
        assert_eq!(route_row_count(&document(&[("User", Some(0), 4), ("Geometry", Some(0), 2)]), &at), 2, "and a section of that name");
        assert_eq!(
            route_row_count(&document(&[("Geometry", Some(1), 3), ("User", Some(0), 4)]), &at),
            0,
            "a shape with no route holds no row of one"
        );
        assert_eq!(route_row_count(&document(&[]), &at), 0);
        let elsewhere = ShapeRef { owner: Owner::Page(1), path: vec![3] };
        assert_eq!(route_row_count(&document(&[("Geometry", Some(0), 5)]), &elsewhere), 0, "a shape that is not there");
    }

    /// A neighbour a rebuild reached, with no hop to make or take away, is looked at before it is kept, and the look is charged to the budget a
    /// row at a time. Two connectors, the level `A` routed and the upright `B` it crosses, which `A`'s jump keeps plain: `B` with points on its leg (dropped when
    /// the route is read) costs the pairs of legs of `B` without them and one more for each row, and `B` is as it was.
    #[test]
    fn the_look_at_the_rows_of_a_neighbour_kept_as_it_is_is_charged_a_step_a_row() {
        use super::super::connectors::{End, attach_connector, write_route};
        let run = |extra: u32| -> (u64, Document) {
            let mut doc = Document::new_blank();
            let a = attach_connector(&mut doc, 1, &End::Point { x: 1.0, y: 2.0 }, &End::Point { x: 5.0, y: 2.0 }).unwrap().id;
            attach_connector(&mut doc, 1, &End::Point { x: 3.0, y: 1.0 }, &End::Point { x: 3.0, y: 3.0 }).unwrap();
            let routes: [Vec<P>; 2] = [
                vec![(1.0, 2.0), (5.0, 2.0)],
                std::iter::once((3.0, 1.0)).chain((1..=extra).map(|k| (3.0, 1.0 + 0.02 * f64::from(k)))).chain([(3.0, 3.0)]).collect(),
            ];
            for (stack, route) in routes.iter().enumerate() {
                let at = ShapeRef { owner: Owner::Page(1), path: vec![u32::try_from(stack).unwrap()] };
                let pose = Reader::new(&doc).pose(&SheetRef::Shape(at));
                write_route(&mut doc, 1, &[u32::try_from(stack).unwrap()], rows::rows(&local(&rows::polyline(route), &pose), false), &None).unwrap();
            }
            let before = doc.clone();
            PAIRS.with(|c| c.set(0));
            refresh(&mut doc, &before, 1, &[a], false).unwrap();
            let neighbour = ShapeRef { owner: Owner::Page(1), path: vec![1] };
            assert_eq!(doc.shape(&neighbour), before.shape(&neighbour), "{extra} points on the leg: the neighbour is as it was");
            assert_eq!(
                route_row_count(&doc, &neighbour),
                2 + usize::try_from(extra).unwrap(),
                "{extra} points on the leg: and holds the rows it held"
            );
            (PAIRS.with(std::cell::Cell::get), doc)
        };
        let (none, _) = run(0);
        assert!(none > 0, "the rebuild tested the crossing of the two");
        for extra in [1, 7, 40] {
            let (pairs, _) = run(extra);
            assert_eq!(pairs, none + u64::from(extra), "{extra} rows more: a pair of legs each, and no more");
        }
    }

    #[test]
    fn two_legs_cross_where_each_runs_on_past_the_other_and_nowhere_else() {
        let level = ((0.0, 0.0), (4.0, 0.0));
        assert_eq!(crossing(level, ((2.0, -1.0), (2.0, 1.0))).map(|c| c.0), Some((2.0, 0.0)));
        let (_, t, u) = crossing(level, ((1.0, 1.0), (3.0, -1.0))).unwrap();
        assert!((t - 0.5).abs() < 1e-12 && (u - 0.5).abs() < 1e-12);
        assert_eq!(crossing(level, ((2.0, 0.0), (2.0, 1.0))), None, "a T: the other leg ends on this one");
        assert_eq!(crossing(level, ((0.0, -1.0), (0.0, 1.0))), None, "at an end of this leg");
        assert_eq!(crossing(level, ((0.0, 1.0), (4.0, 1.0))), None, "parallel");
        assert_eq!(crossing(level, ((1.0, 0.0), (3.0, 0.0))), None, "on top of it");
        assert_eq!(crossing(level, ((5.0, -1.0), (5.0, 1.0))), None, "past the end");
        assert_eq!(crossing(level, ((2.0, 0.0), (2.0, 0.0))), None, "a leg of no length");
    }

    #[test]
    fn the_page_gives_the_jump_by_its_code_to_the_flatter_the_steeper_the_upper_or_the_lower() {
        let (flat, steep) = (((0.0, 0.0), (4.0, 0.5)), ((2.0, -2.0), (2.5, 2.0)));
        assert_eq!(page_choice(1, flat, steep, Side::Second), Some(Side::First));
        assert_eq!(page_choice(1, steep, flat, Side::First), Some(Side::Second));
        assert_eq!(page_choice(2, flat, steep, Side::First), Some(Side::Second));
        assert_eq!(page_choice(3, flat, steep, Side::Second), Some(Side::Second), "3 stands in the order");
        assert_eq!(page_choice(4, flat, steep, Side::First), Some(Side::First));
        assert_eq!(page_choice(5, flat, steep, Side::First), Some(Side::Second));
        for none in [0, 6, 7, 99] {
            assert_eq!(page_choice(none, flat, steep, Side::First), None, "code {none}");
        }
        // Equally steep: the upper.
        let (a, b) = (((0.0, 0.0), (2.0, 2.0)), ((0.0, 2.0), (2.0, 0.0)));
        assert_eq!(page_choice(1, a, b, Side::First), Some(Side::First));
        assert_eq!(page_choice(1, a, b, Side::Second), Some(Side::Second));
    }

    #[test]
    fn a_connectors_own_code_overrides_the_page_as_the_specification_has_it() {
        use Side::{First, Second};
        // No code of their own: the page's choice.
        assert_eq!(carrier(Some(First), 0, 0), Some(First));
        assert_eq!(carrier(None, 0, 0), None);
        // 1: never; the page's choice goes nowhere (unless the other asks for it).
        assert_eq!(carrier(Some(First), 1, 0), None);
        assert_eq!(carrier(Some(First), 1, 2), Some(Second));
        // 2: always, where the page would give it to nobody; the page's choice wins a conflict.
        assert_eq!(carrier(None, 2, 0), Some(First));
        assert_eq!(carrier(None, 0, 2), Some(Second));
        assert_eq!(carrier(Some(Second), 2, 0), Some(Second));
        // 3: never itself, and the connector it crosses jumps instead.
        assert_eq!(carrier(Some(First), 3, 0), Some(Second));
        assert_eq!(carrier(None, 0, 3), Some(First));
        // 4: neither.
        for (a, b) in [(4, 0), (0, 4), (4, 2), (2, 4), (4, 3)] {
            assert_eq!(carrier(Some(First), a, b), None, "{a} {b}");
            assert_eq!(carrier(None, a, b), None, "{a} {b}");
        }
    }

    #[test]
    fn a_mirrored_connector_has_the_other_sign_for_the_same_side_on_the_page() {
        let level = [(0.0, 0.0), (4.0, 0.0)];
        let upright = [(2.0, -1.0), (2.0, 1.0)];
        let bow_of = |pose: Pose| {
            let me = part(1, 0, &level, pose);
            let other = part(2, 1, &upright, unturned());
            let mut work = MAX_WORK;
            let segs = path_of(&me, &[&other], &settings(), &mut work).unwrap();
            segs.iter().find_map(|s| if let Seg::Arc { bow, .. } = s { Some(*bow) } else { None }).unwrap()
        };
        let half = 0.2 * 0.5 / 2.0;
        assert!((bow_of(unturned()) + half).abs() < 1e-12, "running toward +x, up is the negative side");
        assert!((bow_of(Pose { flip_x: true, ..unturned() }) - half).abs() < 1e-12, "under one flip the same side on the page is the other sign");
        assert!((bow_of(Pose { flip_x: true, flip_y: true, ..unturned() }) + half).abs() < 1e-12, "under two flips it is the first again");
    }

    #[test]
    fn a_crossing_with_room_is_a_jump_a_line_and_a_chord_and_one_without_is_not() {
        let level = [(0.0, 0.0), (4.0, 0.0)];
        let me = part(1, 0, &level, unturned());
        let near_end = part(2, 1, &[(0.04, -1.0), (0.04, 1.0)], unturned());
        let mid = part(3, 2, &[(2.0, -1.0), (2.0, 1.0)], unturned());
        let mut work = MAX_WORK;
        let kinds = |others: &[&Part], work: &mut u64| path_of(&me, others, &settings(), work).unwrap().len();
        assert_eq!(kinds(&[&near_end], &mut work), 2, "0.04 from the end, with half a chord of 0.05 needed: none");
        assert_eq!(kinds(&[&mid], &mut work), 4, "move, line, arc, line");
        assert_eq!(kinds(&[&near_end, &mid], &mut work), 4);
        // Two crossings closer than a chord: the first only.
        let close = part(4, 3, &[(2.05, -1.0), (2.05, 1.0)], unturned());
        assert_eq!(kinds(&[&mid, &close], &mut work), 4);
    }

    #[test]
    fn the_work_of_a_rebuild_is_bounded_and_a_spent_budget_gives_nothing() {
        let level = [(0.0, 0.0), (4.0, 0.0)];
        let me = part(1, 0, &level, unturned());
        let zigzag: Vec<P> = (0..200).map(|i| (f64::from(i) * 0.02, if i % 2 == 0 { -1.0 } else { 1.0 })).collect();
        let other = part(2, 1, &zigzag, unturned());
        let mut work = 10;
        assert!(path_of(&me, &[&other], &settings(), &mut work).is_none(), "a budget of 10 pairs cannot test 199 legs");
        let mut enough = MAX_WORK;
        let segs = path_of(&me, &[&other], &settings(), &mut enough).unwrap();
        assert!(segs.len() >= 2);
        assert!(MAX_WORK - enough >= 199, "the pairs tested were taken from the budget");
        let mut work = 5;
        assert_eq!(crosses(&level, &zigzag, &mut work), None);
        assert!(!spend(&mut 3, 4) && spend(&mut 4, 4));
    }

    #[test]
    fn a_rebuild_of_connectors_that_all_overlap_tests_no_more_pairs_than_its_budget_and_writes_nothing() {
        use super::super::connectors::{End, attach_connector, write_route};
        // 300 connectors of ours with free ends, each given a zigzag of 200 slanted legs across one box: 60,000 legs and 3.6e9 pairs of them.
        let mut doc = Document::new_blank();
        for _ in 0..300 {
            attach_connector(&mut doc, 1, &End::Point { x: 1.0, y: 1.0 }, &End::Point { x: 5.0, y: 1.0 }).unwrap();
        }
        let zigzag: Vec<P> = (0..=200).map(|i| (f64::from(i) * 0.02, if i % 2 == 0 { 0.0 } else { 1.0 })).collect();
        for i in 0..300_u32 {
            write_route(&mut doc, 1, &[i], rows::rows(&rows::polyline(&zigzag), false), &None).unwrap();
        }
        let before = doc.clone();
        PAIRS.with(|c| c.set(0));
        refresh(&mut doc, &before, 1, &[], true).unwrap();
        let pairs = PAIRS.with(std::cell::Cell::get);
        assert!(pairs <= MAX_WORK, "{pairs} pairs of legs tested, the budget is {MAX_WORK}");
        assert!(pairs > MAX_WORK / 2, "the budget is what stopped it: {pairs}");
        assert_eq!(doc, before, "a rebuild that is given up writes nothing");
        // A page that fits the budget tests every pair its boxes meet in, and no more: two connectors of 200 legs are 200 * 200 pairs each way.
        let mut small = Document::new_blank();
        for _ in 0..2 {
            attach_connector(&mut small, 1, &End::Point { x: 1.0, y: 1.0 }, &End::Point { x: 5.0, y: 1.0 }).unwrap();
        }
        for i in 0..2_u32 {
            write_route(&mut small, 1, &[i], rows::rows(&rows::polyline(&zigzag), false), &None).unwrap();
        }
        let before = small.clone();
        PAIRS.with(|c| c.set(0));
        refresh(&mut small, &before, 1, &[], true).unwrap();
        assert_eq!(
            PAIRS.with(std::cell::Cell::get),
            2 * 200 * 201,
            "each of the two connectors tests the 200 legs it has against the 201 points of the other"
        );
    }

    #[test]
    fn hostile_numbers_make_no_crossing_and_no_jump() {
        for bad in [f64::NAN, f64::INFINITY, 1.0e300, -1.0e300] {
            let _ = crossing(((0.0, 0.0), (bad, 0.0)), ((1.0, -1.0), (1.0, 1.0)));
            let me = part(1, 0, &[(0.0, 0.0), (bad, 0.0)], unturned());
            let other = part(2, 1, &[(1.0, -1.0), (1.0, 1.0)], unturned());
            let mut work = MAX_WORK;
            let segs = path_of(&me, &[&other], &settings(), &mut work).unwrap();
            assert!(segs.len() >= 2 && segs.len() <= 4, "{bad}: {}", segs.len());
        }
    }
}
