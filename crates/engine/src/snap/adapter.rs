//! The engine's side of `craft-snap`: what a page gives the shared library as targets, what a query asks of it, and what it gives told in the
//! engine's words ([`Snapped`], [`Hit`]).
//!
//! **The targets** of a page are built once for a gesture or a command and kept on the [`SnapPage`] (`TargetsCache`), keyed by the shapes that move
//! (they are never targets) and whether any shape is read at all: the guides and the page margins as lines, and each indexed top-level shape that does
//! not move as an object framed by its corners on the page, with the box round them as its bounds. A snap that needs no shape reads none.
//!
//! **The policy** is the engine's and is passed as data on each query: the document's [`Strength`] (in `SnapSettings`) as the pulls, as priorities, places before lines, a shape lines
//! up with another's box by any of its edges and middle, the alignment of a shape reaches the larger of [`ALIGN_REACH`](super::ALIGN_REACH) and a pull's worth of inches round the
//! subject, equal spacing as `Beyond`/`Neighbour` (today's rule), and marks that are only what was snapped to. Settings and the kinds a query may use
//! come from [`SnapSettings`] as they did before the library.
//!
//! **What stays here:** the shapes' cells (frames, connection points, outlines under their budgets), `InhibitSnap`, the Snap and Glue switches, and
//! the mapping of a hit to [`Target`], [`Anchor`], [`Glue`] and [`Hold`]. The old function is kept test-only as the oracle (`oracle.rs`) that `equal.rs`
//! holds this one equal to.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use craft_snap as cs;
use diacraft_doc::Document;
use kurbo::{BezPath, Point, Rect as KRect, Vec2};

use super::Nudge;
use super::geometry::{self, outline};
use super::index::{Item, MAX_VISITS};
use super::label;
use super::{
    Anchor, Axis, DEFAULT_ZOOM, Edges, Extras, Glue, Hit, Hold, MAX_CANDIDATES, MAX_CONNECTION_ROWS, MAX_CONNECTION_ROWS_PER_QUERY,
    MAX_OUTLINE_SHAPES, MAX_OWN_POINTS, MAX_PATH_PAIRS, MAX_TARGET_PIECES, MAX_TARGET_POINTS, OwnFrame, P, Query, Rect, SPACING_REACH, SnapMark,
    SnapPage, SnapSettings, Snapped, Strength, Subject, Target, Turn, Use,
};
use crate::draw::Reader;

/// The rules of the design's table that the engine decides besides its pulls, as data: [`Policy::TODAY`] is the snapping as it was before the library (what the
/// harness holds the adapter equal to), and [`Policy::ADOPTED`] is what the engine does now, each field moved by one row of the design's table.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Policy {
    /// Which side of a box meets which line of another shape.
    pub matching: cs::Matching,
    /// Whether a box lines up with every shape of the page, and not only those within reach of it.
    pub align_anywhere: bool,
    /// The row of equal gaps: today's rule is the strip as wide across as the subject, the neighbour on each side, then the next box wholly beyond it.
    pub spacing_row: cs::SpacingRow,
    pub spacing_gaps: cs::SpacingGaps,
    /// A shape that draws no line (`LinePattern` 0) offers no alignment lines (row 15).
    pub no_line_is_no_target: bool,
    /// Which marks are given: only what was snapped to (today), or every target the landed box lies on (row 8).
    pub marks: cs::MarkRule,
    /// How far a line mark runs: across every shape on it (today), or from the box to the nearest (row 8).
    pub line_span: cs::LineSpan,
    /// A resize stops at the width or height of another shape (row 6), looking at the shapes in the view when the call has one.
    pub size_match: bool,
    /// A turned shape lines up by the box round its paths, not round its corners (row 14).
    pub path_boxes: bool,
    /// Places (connection points, vertices, outlines, crossings) pull a move held to a page axis where they lie on the line it travels along (row 1).
    pub locked_places: cs::TravelPlaces,
    /// Places stop an arrow key where they lie on the line it travels along (row 4).
    pub nudge_places: cs::TravelPlaces,
    /// A turned shape lines up by its own sides with the like sides of shapes turned the same way and with slanted guides parallel to them, and a turned or flipped shape's resize
    /// snaps along its own axes (row 3): the objects offer their own lines, the library's own directions are on, and a call that is told its subject's frame
    /// ([`Extras::frame`]) is asked by it.
    pub turned_sides: bool,
}

impl Policy {
    #[cfg(test)]
    pub(super) const TODAY: Policy = Policy {
        matching: cs::Matching::AnyToAny,
        align_anywhere: false,
        spacing_row: cs::SpacingRow::Beyond,
        spacing_gaps: cs::SpacingGaps::Neighbour,
        no_line_is_no_target: false,
        marks: cs::MarkRule::HitsOnly,
        line_span: cs::LineSpan::Every,
        size_match: false,
        path_boxes: false,
        locked_places: cs::TravelPlaces::Ignore,
        nudge_places: cs::TravelPlaces::Ignore,
        turned_sides: false,
    };
    /// Each row of the design's table that is built moves one field. Row 13 (spec): a side lines up with the same side of another shape. Row 12: a box lines up with shapes anywhere on the page. Row 9: a box may repeat any gap of its row. Row 15: a shape with no line is no alignment target.
    pub(super) const ADOPTED: Policy = Policy {
        matching: cs::Matching::SameSide,
        align_anywhere: true,
        // Row 9: a row is the shapes whose boxes share a band across the axis, and the subject may repeat any gap of it. The spec names the band and
        // documents no staircase chain, so the library's `Linked` mode is not used; `Beyond` was today's rule only.
        spacing_row: cs::SpacingRow::Band,
        spacing_gaps: cs::SpacingGaps::Row,
        // Row 15: a shape with no line is no alignment target; it stays in equal-gap rows.
        no_line_is_no_target: true,
        // Row 8: every line the landed box lies on is marked, each from the nearest shape on one side of the box to the nearest on the other.
        marks: cs::MarkRule::Coincident,
        line_span: cs::LineSpan::EachSide,
        // Row 6: a resize stops at the width or height of a shape in the view.
        size_match: true,
        // Row 14: a turned shape lines up by the box round its paths.
        path_boxes: true,
        locked_places: cs::TravelPlaces::OnLine,
        nudge_places: cs::TravelPlaces::OnLine,
        // Row 3: a turned shape lines up by its own sides and snaps to slanted guides parallel to them.
        turned_sides: true,
    };
}

// Extended (the design's table): the order kinds of target win in, strongest first.
const PRIORITY_CONNECTION_POINT: u8 = 0;
const PRIORITY_VERTEX: u8 = 1;
const PRIORITY_HANDLE: u8 = 2;
const PRIORITY_INTERSECTION: u8 = 3;
const PRIORITY_GUIDE: u8 = 4;
const PRIORITY_OUTLINE: u8 = 5;
const PRIORITY_ALIGNMENT: u8 = 6;
const PRIORITY_GRID: u8 = 7;
const PRIORITY_RULER: u8 = 8;

/// How far another shape's angle pulls a turn by the rotation handle, in screen pixels: fixed, since no page gives a turn a strength and the six
/// sliders are the pulls of lines, places and the dynamic grid (6 pixels at the pointer).
const ROTATION_PULL: u16 = 6;

/// The target set a page last built, and what it was built for.
#[derive(Default)]
pub(super) struct TargetsCache {
    held: Mutex<Option<Cached>>,
    /// How many sets were built for this page: a drag's moves are one.
    builds: std::sync::atomic::AtomicUsize,
}

impl TargetsCache {
    /// What the last target set built, for the measurements of `equal.rs`.
    #[cfg(test)]
    pub(super) fn report(&self) -> Option<cs::BuildReport> {
        self.held.lock().ok().and_then(|held| held.as_ref().map(|c| *c.built.targets.report()))
    }

    pub(super) fn builds(&self) -> usize {
        self.builds.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// What the held target set takes in memory, in bytes, **estimated** from the library's count of what it holds (objects, lines, points, paths and their elements) at the
    /// prices below ([`price`]).
    pub(super) fn held_bytes(&self) -> usize {
        let Ok(held) = self.held.lock() else { return 0 };
        held.as_ref().map_or(0, |c| price(c.built.targets.report()))
    }
}

/// What a built set of targets takes in memory for each thing it holds, in bytes: **measured** (of the re-review, which found the first prices 2.4 times too
/// low): the process's resident memory (`RssAnon`) taken page after page, nine pages of 10,000 shapes in one process, the first page's growth of the heap left out
/// (`reads.rs`: `measure_what_a_kept_read_holds`, run with `--ignored --nocapture`). **An object costs 667 to 727 bytes for upright shapes and 1,387 to 1,433 for turned ones** once
/// the first query has made the library's own indexes of its lines (the library holds 122 bytes of it as built, `measure_what_the_library_holds_for_each_part`); the report does not
/// say which are turned, so every object is priced at the dearer, with 12 percent over. A point is 43 bytes as built and a line 65, priced at 160 and 240 (several times the
/// built figure, for the indexes a query makes of them: the adapter gives a page's lines a handful of guides and margins, and points only for a shape whose corners are no
/// rectangle). The adapter makes no path, and a path is priced as it was (a guess); each price is pinned by a test.
pub(super) const BYTES_PER_OBJECT: usize = 1_600;
pub(super) const BYTES_PER_POINT: usize = 160;
pub(super) const BYTES_PER_LINE: usize = 240;
pub(super) const BYTES_PER_PATH: usize = 96;
pub(super) const BYTES_PER_PATH_ELEMENT: usize = 48;

/// What a target set that holds what `report` says takes in memory, in bytes (the prices above).
pub(super) fn price(report: &cs::BuildReport) -> usize {
    let n = |v: u32| usize::try_from(v).unwrap_or(usize::MAX);
    let elements = usize::try_from(report.path_elements).unwrap_or(usize::MAX);
    n(report.objects)
        .saturating_mul(BYTES_PER_OBJECT)
        .saturating_add(n(report.points).saturating_mul(BYTES_PER_POINT))
        .saturating_add(n(report.lines).saturating_mul(BYTES_PER_LINE))
        .saturating_add(n(report.paths).saturating_mul(BYTES_PER_PATH))
        .saturating_add(elements.saturating_mul(BYTES_PER_PATH_ELEMENT))
}

struct Cached {
    /// The `ID`s of the shapes that move, sorted.
    moving: Vec<u32>,
    objects: bool,
    /// Whether the guides that lie along no axis are in the set: a subject with sides of its own turned off the page's axes is told them, and a point is not.
    slanted: bool,
    /// The policy the set was built by: a shape that draws no line has no lines (`no_line_is_no_target`), and a turned shape is bounded by its paths (`path_boxes`).
    policy: Policy,
    built: Built,
}

/// A target set, and how many of its objects are bounded by their corners where their paths were meant, for what the paths cost: turned targets, not moving, that the
/// page's budget could not pay for. A result that lines something up by them is `truncated`.
#[derive(Clone)]
struct Built {
    targets: Arc<cs::Targets>,
    by_corners_for_cost: usize,
    /// The shapes whose layers allow no glue: a connector's end is pulled to none of their connection points or outlines and glued to none of them.
    no_glue: Arc<std::collections::HashSet<u32>>,
}

impl std::fmt::Debug for TargetsCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TargetsCache")
    }
}

fn id_of(id: u32) -> cs::Id {
    cs::Id(u64::from(id))
}

fn axis_of(axis: Axis) -> cs::Axis {
    match axis {
        Axis::X => cs::Axis::X,
        Axis::Y => cs::Axis::Y,
    }
}

/// The frame of a shape from its corners on the page (in the order of its own frame), if they make a rectangle with a size: its size is the shape's
/// width and height as drawn, its angle that of its own x edge.
pub(super) fn frame_of(corners: &[P; 4]) -> Option<cs::TurnedBox> {
    let p = |i: usize| corners.get(i).map(|c| Point::new(c.0, c.1));
    let (c0, c1, c2, c3) = (p(0)?, p(1)?, p(2)?, p(3)?);
    let (u, v) = (c1 - c0, c3 - c0);
    let (w, h) = (u.hypot(), v.hypot());
    let skew = (c2 - c1 - v).hypot().max((c3 + u - c2).hypot());
    // A billionth of the sides and the rounding of the corners (at 1e8 inches a corner is known to 1.5e-8, and the sides of a turned shape were no longer square).
    let noise = super::index::corner_noise(corners);
    let tol = 1.0e-9 * (w + h).max(1.0) + 4.0 * noise;
    if !(w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0) || noise >= w.min(h) || u.dot(v).abs() > tol * w.max(h) || skew > tol {
        return None;
    }
    Some(cs::TurnedBox { centre: c0.midpoint(c2), size: kurbo::Size::new(w, h), angle: u.y.atan2(u.x) })
}

/// [`frame_of`] with the direction of the shape's own x axis taken from its cells (`cells`, in radians) when the corners agree with it within the rounding of their numbers: the
/// corners' `atan2` is known to the rounding of the corners over the shape's size, which is a few steps of the pin's distance from the origin, and a line along a side that is
/// off by that angle is off by it times the distance (a slanted guide, whose angle is exact, then does not meet the shape's side where it is drawn).
pub(super) fn frame_from(corners: &[P; 4], cells: f64) -> Option<cs::TurnedBox> {
    let mut f = frame_of(corners)?;
    let d = (f.angle - cells).rem_euclid(std::f64::consts::TAU);
    let d = d.min(std::f64::consts::TAU - d);
    if cells.is_finite() && d <= 1.0e-6 + 8.0 * super::index::corner_noise(corners) / f.size.width.min(f.size.height) {
        f.angle = cells;
    }
    Some(f)
}

/// The own frame of a shape from its corners on the page (and the direction of its own x axis by its cells, `cells`), when it is turned by other than whole quarter turns and the
/// corners make a rectangle with a size.
pub(super) fn own_frame(corners: &[P; 4], cells: f64) -> Option<OwnFrame> {
    if !super::index::is_turned(corners) {
        return None;
    }
    let f = frame_from(corners, cells)?;
    Some(OwnFrame { centre: (f.centre.x, f.centre.y), width: f.size.width, height: f.size.height, angle: f.angle })
}

fn object_of(item: &Item, policy: Policy) -> (cs::Object, Option<cs::TurnedBox>) {
    let lines = policy.no_line_is_no_target;
    // A turned shape lines up by the box round its paths when it has one (row 14), else by the box round its corners.
    let bounds = match item.path_box.filter(|_| policy.path_boxes) {
        Some(b) => KRect::new(b.x0, b.y0, b.x1, b.y1),
        None => KRect::new(item.bbox.x0, item.bbox.y0, item.bbox.x1, item.bbox.y1),
    };
    let corner_box = cs::TurnedBox::from_rect(KRect::new(item.bbox.x0, item.bbox.y0, item.bbox.x1, item.bbox.y1));
    let framed = frame_from(&item.corners, item.frame_angle);
    let frame = framed.unwrap_or(corner_box);
    let parts = cs::Parts {
        bounds_lines: item.line || !lines,
        // A shape turned alike offers its own sides; one turned a quarter turn from the page's has the page's lines only, and the library stores them once.
        own_lines: policy.turned_sides && framed.is_some() && (item.line || !lines),
        spacing: true,
        size: item.line || !lines,
        angle: true,
        corners: framed.is_some(),
        side_middles: framed.is_some(),
        detail: true,
    };
    (cs::Object { frame, bounds: Some(bounds), angle: Some(item.angle).filter(|a| a.is_finite()), parts }, framed)
}

/// The targets of `page` without the shapes `moving`: the guides and the margins, and the shapes when `objects` is asked for.
fn build(doc: &Document, page: &SnapPage, moving: &[u32], objects: bool, slanted: bool, policy: Policy) -> Built {
    let mut by_corners_for_cost = 0usize;
    let mut b = cs::TargetsBuilder::new(cs::Limits::default());
    // A target the builder refuses (a number that is not finite, a limit) is counted by it and is no target: ignoring the refusal is right.
    if !page.inhibit {
        for g in &page.guides {
            let _ = b.line(Some(id_of(g.id)), axis_of(g.axis), g.at, None, cs::Kind::Guide);
        }
        // A guide along no axis is a line for a subject whose own sides can be parallel to it; its normal is the line's direction turned a quarter, folded by the library.
        if slanted {
            for g in &page.slanted {
                let Some(direction) = cs::Direction::new((g.angle + std::f64::consts::FRAC_PI_2).rem_euclid(std::f64::consts::PI)) else { continue };
                let n = direction.normal();
                let _ = b.line_at(Some(id_of(g.id)), direction, n.x * g.at.0 + n.y * g.at.1, None, cs::Kind::Guide);
            }
        }
        if page.size.0 > 0.0 && page.size.1 > 0.0 {
            let (m, (w, h)) = (page.margins, page.size);
            for at in [m.left, w - m.right] {
                let _ = b.line(None, cs::Axis::X, at, None, cs::Kind::Layout);
            }
            for at in [m.bottom, h - m.top] {
                let _ = b.line(None, cs::Axis::Y, at, None, cs::Kind::Layout);
            }
        }
    }
    let mut no_glue: std::collections::HashSet<u32> = std::collections::HashSet::new();
    if objects {
        // A layer whose Snap box is clear lets nothing snap to its shapes, and a shape on layers that allow it is as any other. A table of too many layers
        // is read as having no such layer: the commands that make layers say so in words. A layer whose Glue box is clear lets no end glue to its shapes.
        let standing = crate::draw::layers::page_standing(doc, page.page).unwrap_or_default();
        let off: std::collections::HashSet<u32> = standing.iter().filter(|(_, st)| st.no_snap).map(|(id, _)| *id).collect();
        no_glue = standing.iter().filter(|(_, st)| st.no_glue).map(|(id, _)| *id).collect();
        for item in page.shapes(doc).items() {
            if moving.binary_search(&item.id).is_ok() || off.contains(&item.id) {
                continue;
            }
            let (object, framed) = object_of(item, policy);
            by_corners_for_cost += usize::from(policy.path_boxes && item.path_cut);
            let _ = b.object(id_of(item.id), object);
            if framed.is_none() {
                // Corners that are no rectangle (a shape in a group scaled unevenly and turned): its handles are still its corners.
                let c = &item.corners;
                for (i, p) in c.iter().enumerate() {
                    let _ = b.point(Some(id_of(item.id)), Point::new(p.0, p.1), cs::Kind::Corner, 0);
                    let q = c.get((i + 1) % 4).copied().unwrap_or(*p);
                    let _ = b.point(Some(id_of(item.id)), Point::new((p.0 + q.0) / 2.0, (p.1 + q.1) / 2.0), cs::Kind::SideMiddle, 0);
                }
            }
        }
    }
    Built { targets: Arc::new(b.build()), by_corners_for_cost, no_glue: Arc::new(no_glue) }
}

/// The targets for a query: the page's last build when it was for the same moving shapes, else a new one (kept).
///
/// **The shapes that move are keyed by the ones the set leaves out**: those in the index, when shapes are read at all (a set with no shapes leaves none out). A shape that is not
/// there (the new shape of a drop, which the page was read without; a guide) changes nothing in the set, and so must not make a set built again: the page the window made ready
/// for a drop's preview, with nothing moving, is the page the command finds.
fn targets_for(doc: &Document, page: &SnapPage, moving: &[u32], objects: bool, slanted: bool, policy: Policy) -> Built {
    let mut key = moving.to_vec();
    key.sort_unstable();
    key.dedup();
    if objects {
        let index = page.shapes(doc);
        key.retain(|id| index.find(*id).is_some());
    } else {
        key.clear();
    }
    if let Ok(held) = page.targets.held.lock()
        && let Some(c) = held.as_ref()
        && c.objects == objects
        && c.slanted == slanted
        && c.policy == policy
        && c.moving == key
    {
        return c.built.clone();
    }
    let built = build(doc, page, &key, objects, slanted, policy);
    page.targets.builds.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    super::reads::count(|c| c.target_builds += 1);
    if let Ok(mut held) = page.targets.held.lock() {
        *held = Some(Cached { moving: key, objects, slanted, policy, built: built.clone() });
    }
    built
}

/// What a query reads of the shapes near the subject that the library asks for: connection points and outlines, under the engine's budgets, which are the
/// call's (`Spent`), not the query's.
struct DocDetail<'a> {
    doc: &'a Document,
    page: &'a SnapPage,
    reader: Reader<'a>,
    points: bool,
    outlines: bool,
    spent: &'a mut Spent,
    /// For a connector's end: the shapes whose layers allow no glue, whose points and outlines are not read.
    no_glue: Option<&'a std::collections::HashSet<u32>>,
}

impl DocDetail<'_> {
    /// Whether the shape `id` is one whose layers allow no glue, for a connector's end.
    fn barred(&self, id: u32) -> bool {
        self.no_glue.is_some_and(|set| set.contains(&id))
    }
}

impl cs::Detail for DocDetail<'_> {
    fn points(&mut self, id: cs::Id, spend: &mut cs::Spend<'_>, out: &mut Vec<(Point, cs::Kind, u32)>) {
        if !self.points {
            return;
        }
        let Some(item) = u32::try_from(id.0).ok().filter(|n| !self.barred(*n)).and_then(|n| self.page.shapes(self.doc).find(n)) else { return };
        // Counted over the layers and charged to the query before any row is merged.
        let rows = self.reader.connection_rows(&item.at);
        if rows > MAX_CONNECTION_ROWS || rows > self.spent.rows_left {
            self.spent.cut = true;
        } else if rows > 0 && spend.detail(rows as u64) {
            self.spent.rows_left -= rows;
            for cp in self.reader.connection_points_within(&item.at, rows).unwrap_or_default() {
                out.push((Point::new(cp.x / self.page.k, cp.y / self.page.k), cs::Kind::Point(0), cp.index));
            }
        }
    }

    fn outline(&mut self, id: cs::Id, _spend: &mut cs::Spend<'_>, out: &mut Vec<BezPath>) {
        if !self.outlines {
            return;
        }
        let Some((n, item)) =
            u32::try_from(id.0).ok().filter(|n| !self.barred(*n)).and_then(|n| self.page.shapes(self.doc).find(n).map(|item| (n, item)))
        else {
            return;
        };
        // Built once a call: a shape a later query asks for again is given the paths of the first, and is counted and charged once (the shapes whose outlines a call
        // reads are the call's distinct shapes, at most `MAX_OUTLINE_SHAPES`, whatever the number of queries it makes).
        if let Some(paths) = self.spent.outlines_read.get(&n) {
            out.extend(paths.iter().cloned());
            return;
        }
        if self.spent.outlined >= MAX_OUTLINE_SHAPES {
            self.spent.cut = true;
            return;
        }
        self.spent.outlined += 1;
        let shape = outline(&self.reader, &item.at, self.page.k, &mut self.spent.geometry);
        if shape.too_big > 0 {
            self.spent.cut = true;
        }
        out.extend(shape.paths.iter().cloned());
        self.spent.outlines_read.insert(n, shape.paths);
    }
}

/// Which of the targets that are places a snap looks for.
#[derive(Clone, Copy, Default)]
struct Kinds {
    connection_points: bool,
    handles: bool,
    vertices: bool,
    outline: bool,
    crossings: bool,
}

impl Kinds {
    fn any(&self) -> bool {
        self.connection_points || self.handles || self.outlines()
    }

    /// Whether the outlines of the shapes near are read.
    fn outlines(&self) -> bool {
        self.vertices || self.outline || self.crossings
    }
}

fn subject_rect(subject: &Subject) -> Rect {
    match subject {
        Subject::Point(p) => Rect { x0: p.0, y0: p.1, x1: p.0, y1: p.1 },
        Subject::Box(r) | Subject::Edges(r, _) => *r,
    }
}

/// The library's box of an own frame.
fn to_turned(f: OwnFrame) -> cs::TurnedBox {
    cs::TurnedBox { centre: Point::new(f.centre.0, f.centre.1), size: kurbo::Size::new(f.width, f.height), angle: f.angle }
}

/// The engine's own frame of the library's box.
fn from_turned(b: &cs::TurnedBox) -> OwnFrame {
    OwnFrame { centre: (b.centre.x, b.centre.y), width: b.size.width, height: b.size.height, angle: b.angle }
}

fn to_frame(r: &Rect) -> cs::TurnedBox {
    cs::TurnedBox::from_rect(KRect::new(r.x0, r.y0, r.x1, r.y1))
}

/// A lattice of the page's grid or ruler, from the marks of each axis; none when neither axis has lines.
fn lattice(x: Option<super::Marks>, y: Option<super::Marks>, kind: cs::Kind) -> Option<cs::Lattice> {
    if x.is_none() && y.is_none() {
        return None;
    }
    let line = |m: &Option<super::Marks>| m.map_or((0.0, 0.0), |m| (m.origin, m.pitch));
    let ((ox, px), (oy, py)) = (line(&x), line(&y));
    Some(cs::Lattice { origin: Point::new(ox, oy), pitch: Vec2::new(px, py), kind })
}

/// The kinds of target a query uses, each with its pull and rank.
struct Rules {
    strength: Strength,
    kinds: Kinds,
    lines_up: bool,
    spacing: bool,
    size: bool,
    /// A turn matches another shape's angle, and falls to a step.
    angle: bool,
    step: bool,
    guides: bool,
    margins: bool,
    grid: bool,
    ruler: bool,
}

impl Rules {
    fn table(&self) -> cs::KindTable {
        let mut t = cs::KindTable::default();
        let s = &self.strength;
        // The pulls are whole screen pixels from 0 to 999 (`Strength`), at the zoom of the query (`Reach::Screen`: a target pulls if and only if its distance on the screen is within).
        let rule = |on: bool, px: u16, priority: u8| cs::KindRule { on, reach: cs::Reach::Screen(f64::from(px)), priority, scope: cs::Scope::All };
        let any = |on: bool, priority: u8| cs::KindRule { on, reach: cs::Reach::Any, priority, scope: cs::Scope::All };
        let off = |priority: u8| cs::KindRule { on: false, reach: cs::Reach::Screen(0.0), priority, scope: cs::Scope::All };
        // The kinds that are places keep their pulls whether they are on or not: the window the places are looked for in is as wide as the widest.
        t.set_all_points(rule(false, s.points, PRIORITY_CONNECTION_POINT));
        t.set(cs::Kind::Point(0), rule(self.kinds.connection_points, s.points, PRIORITY_CONNECTION_POINT));
        t.set(cs::Kind::Vertex, rule(self.kinds.vertices, s.points, PRIORITY_VERTEX));
        t.set(cs::Kind::Corner, rule(self.kinds.handles, s.points, PRIORITY_HANDLE));
        t.set(cs::Kind::SideMiddle, rule(self.kinds.handles, s.points, PRIORITY_HANDLE));
        t.set(cs::Kind::Intersection, rule(self.kinds.crossings, s.geometry, PRIORITY_INTERSECTION));
        t.set(cs::Kind::Nearest, rule(self.kinds.outline, s.geometry, PRIORITY_OUTLINE));
        t.set(cs::Kind::Midpoint, rule(false, s.geometry, PRIORITY_OUTLINE));
        t.set(cs::Kind::Tangent, off(PRIORITY_OUTLINE));
        t.set(cs::Kind::Perpendicular, off(PRIORITY_OUTLINE));
        t.set(cs::Kind::Guide, rule(self.guides, s.guides, PRIORITY_GUIDE));
        t.set(cs::Kind::Edge, rule(self.lines_up, s.extensions, PRIORITY_ALIGNMENT));
        t.set(cs::Kind::Centre, rule(self.lines_up, s.extensions, PRIORITY_ALIGNMENT));
        t.set(cs::Kind::Layout, rule(self.margins, s.extensions, PRIORITY_ALIGNMENT));
        t.set(cs::Kind::Spacing, rule(self.spacing, s.extensions, PRIORITY_ALIGNMENT));
        t.set(cs::Kind::Frame, off(PRIORITY_ALIGNMENT));
        t.set_all_lines(off(PRIORITY_ALIGNMENT));
        t.set(
            cs::Kind::Size,
            cs::KindRule { on: self.size, reach: cs::Reach::Screen(f64::from(s.extensions)), priority: PRIORITY_ALIGNMENT, scope: cs::Scope::View },
        );
        t.set(cs::Kind::Angle, rule(self.angle, ROTATION_PULL, PRIORITY_ALIGNMENT));
        t.set(cs::Kind::AngleStep, any(self.step, PRIORITY_GRID));
        t.set(cs::Kind::Grid, rule(self.grid, s.grid, PRIORITY_GRID));
        t.set(cs::Kind::Ruler, rule(self.ruler, s.ruler, PRIORITY_RULER));
        t
    }
}

/// The library's settings for a query: the engine's policy (as priorities and the ranked choice, places before lines, the spacing reach, and what the policy
/// says of places on a line of travel) and the rules of the query (`rules`). `align_reach` is how far across another shape may be and still be lined up with: `None`
/// anywhere on the page.
fn settings_of(policy: Policy, rules: &Rules, align_reach: Option<cs::Reach>) -> cs::Settings {
    cs::Settings {
        kinds: rules.table(),
        choose: cs::Choose::Ranked,
        places: cs::Places::First,
        matching: policy.matching,
        frames: cs::Frames { page: true, own: policy.turned_sides },
        align_reach,
        spacing_reach: cs::SpacingReach { times: 4.0, at_least: SPACING_REACH },
        spacing_row: policy.spacing_row,
        spacing_gaps: policy.spacing_gaps,
        size_axes: cs::SizeAxes::SameAxis,
        angle_period: std::f64::consts::TAU,
        marks: policy.marks,
        coincide: cs::Coincide::default(),
        line_span: policy.line_span,
        locked_places: policy.locked_places,
        nudge_places: policy.nudge_places,
    }
}

/// What one call of [`snap`] spent over all the library queries it made (a box asked on each axis alone, outlines asked apart, a resize asked side by side):
/// the budgets of the engine are the call's, so a query that asks the library more than once is charged as one.
struct Spent {
    /// The rows of `Connection` sections the call may still merge.
    rows_left: usize,
    /// The shapes whose outlines were read.
    outlined: usize,
    /// What building the outlines cost.
    geometry: geometry::Budget,
    /// The outlines already built in this call, by the shape's `ID`: a later query of the call that asks for the same shape (the outlines are asked apart when the moving
    /// shapes have places of their own, after the vertices and crossings were asked for) is given the paths of the first and is charged nothing, so the budgets above are
    /// for the call's distinct shapes, as they were for the one query the old function made.
    outlines_read: HashMap<u32, Vec<BezPath>>,
    /// Whether the detail source left something out for its cost.
    cut: bool,
    /// The library queries made.
    queries: u32,
    examined: u64,
    visited: u64,
    candidates: u64,
    points: u64,
    pieces: u64,
    flattened: u64,
    pairs: u64,
}

impl Spent {
    fn new() -> Spent {
        Spent {
            rows_left: MAX_CONNECTION_ROWS_PER_QUERY,
            outlined: 0,
            geometry: geometry::Budget::new(),
            outlines_read: HashMap::new(),
            cut: false,
            queries: 0,
            examined: 0,
            visited: 0,
            candidates: 0,
            points: 0,
            pieces: 0,
            flattened: 0,
            pairs: 0,
        }
    }

    /// The library's budget for the next query: the engine's caps less what the call has spent.
    fn budget(&self) -> cs::Budget {
        let left = |cap: usize, spent: u64| u32::try_from((cap as u64).saturating_sub(spent)).unwrap_or(u32::MAX);
        cs::Budget {
            window_visits: MAX_VISITS as u32,
            candidates: MAX_CANDIDATES as u32,
            own: MAX_OWN_POINTS as u32,
            points: left(MAX_TARGET_POINTS, self.points),
            outlines: MAX_CANDIDATES as u32,
            pieces: left(MAX_TARGET_PIECES, self.pieces),
            pairs: left(MAX_PATH_PAIRS, self.pairs),
            row: MAX_CANDIDATES as u32,
            ..cs::Budget::default()
        }
    }

    fn add(&mut self, work: &cs::Work) {
        self.queries = self.queries.saturating_add(1);
        self.examined = self.examined.saturating_add(work.examined);
        self.visited = self.visited.saturating_add(work.visited);
        self.candidates = self.candidates.saturating_add(work.candidates);
        self.points = self.points.saturating_add(work.points);
        self.pieces = self.pieces.saturating_add(work.pieces);
        self.flattened = self.flattened.saturating_add(work.flattened);
        self.pairs = self.pairs.saturating_add(work.pairs);
    }

    /// The counters of the call, told in the engine's words.
    fn tell(&self, out: &mut Snapped) {
        out.examined = to_usize(self.examined);
        out.visited = to_usize(self.visited);
        out.candidates = to_usize(self.candidates);
        out.points = to_usize(self.points);
        out.pieces = to_usize(self.pieces);
        out.flattened = to_usize(self.flattened);
        out.outline = self.geometry.spent();
        out.truncated |= self.cut;
    }
}

/// One call of [`snap`]: what it is asked by, and what it has spent.
struct Call<'a> {
    policy: Policy,
    doc: &'a Document,
    page: &'a SnapPage,
    settings: &'a SnapSettings,
    extras: Extras,
    spent: Spent,
}

/// Snaps the subject of `query` on `page` of `doc`, by `settings` (its options and its [`Strength`]).
pub(super) fn snap(doc: &Document, page: &SnapPage, settings: &SnapSettings, query: &Query<'_>) -> Snapped {
    snap_ext(doc, page, settings, query, Extras::default())
}

/// [`snap`] with the `Extras` of a command.
pub(super) fn snap_ext(doc: &Document, page: &SnapPage, settings: &SnapSettings, query: &Query<'_>, extras: Extras) -> Snapped {
    snap_in(Policy::ADOPTED, extras, doc, page, settings, query)
}

/// [`snap`] by a policy, for the harness.
#[cfg(test)]
pub(super) fn snap_with(policy: Policy, doc: &Document, page: &SnapPage, settings: &SnapSettings, query: &Query<'_>) -> Snapped {
    snap_in(policy, Extras::default(), doc, page, settings, query)
}

fn snap_in(policy: Policy, extras: Extras, doc: &Document, page: &SnapPage, settings: &SnapSettings, query: &Query<'_>) -> Snapped {
    let mut call = Call { policy, doc, page, settings, extras, spent: Spent::new() };
    let mut out = match (query.subject, extras.lock) {
        // An arrow key is one query: the next place ahead on the axis, from the box where it is (the library does the rest: a line, a lattice line, an equal gap or, under
        // `nudge_places`, a place on the line of travel, whichever is nearest).
        (Subject::Box(_), _) if extras.nudge.is_some() => call.run(query, Opts { nudge: extras.nudge, ..Opts::new() }),
        // A move that keeps to an axis is one query with the move bound to it: the library solves inside the lock (a place pulls it where it lies on a line the box travels
        // along, under `locked_places`), so what is drawn is where the box lands.
        (Subject::Box(_) | Subject::Point(_), Some(axis)) => call.run(query, Opts { constraint: cs::Constraint::Axis(axis_of(axis)), ..Opts::new() }),
        (subject, _) => call.free(subject, query),
    };
    call.spent.tell(&mut out);
    out
}

impl Call<'_> {
    /// The subject's own frame, when the policy has own sides and the caller told one that is a box: a frame with a size and numbers, which the library can use.
    fn subject_frame(&self) -> Option<OwnFrame> {
        self.extras.frame.filter(|f| self.policy.turned_sides && to_turned(*f).is_valid())
    }

    fn free(&mut self, subject: Subject, query: &Query<'_>) -> Snapped {
        match subject {
            // A resize that keeps the proportions is solved inside them: the library moves the dragged box along its diagonal, or from a side the other axis with it.
            Subject::Edges(..) if self.extras.keep && self.extras.ratio.is_some_and(|r| r.is_finite() && r > 0.0) => {
                let ratio = self.extras.ratio.unwrap_or(1.0);
                self.run(query, Opts { constraint: cs::Constraint::Aspect { ratio, about: cs::About::Fixed }, ..Opts::new() })
            }
            // A resize that moves both sides of an axis snaps one of them, the stronger kind and then the nearer, as it did: the library snaps every side
            // that moves. (A turned or flipped shape's resize is asked as it is: its sides are its own, and every side that moves snaps.)
            Subject::Edges(r, e) if ((e.left && e.right) || (e.bottom && e.top)) && self.subject_frame().is_none() => {
                self.one_side_of_each_axis(query, r, e)
            }
            Subject::Point(_) | Subject::Box(_) => self.each_axis_alone(query),
            Subject::Edges(..) => self.run(query, Opts::new()),
        }
    }

    /// A box or a point is pulled on each axis by that axis's own best target, and a place pulls both. The library takes the best *pair* of targets for a move,
    /// and a pair whose two pulls together reach farther than the larger of the two reaches is not taken (so two lines nearly parallel cannot throw a box):
    /// for the page's two square axes that is a round reach where the engine's is square. So when the answer holds a line on one axis only, the other axis is
    /// asked alone, with the move locked to it: its own best target is what that axis snapped to.
    fn each_axis_alone(&mut self, query: &Query<'_>) -> Snapped {
        // The outlines are asked apart when the moving shapes have places of their own (see `OutlineMode::Alone`): a place of another kind beats an outline.
        let own = matches!(query.subject, Subject::Box(_)) && query.own_points.iter().any(|p| p.0.is_finite() && p.1.is_finite());
        let split = own && self.settings.snap && !self.page.inhibit && query.using == Use::Place && self.settings.snap_geometry;
        let mut out = if split {
            let without = self.run(query, Opts { outline: OutlineMode::Without, ..Opts::new() });
            if without.hits.iter().any(Hit::is_place) {
                return without;
            }
            let alone = self.run(query, Opts { outline: OutlineMode::Alone, ..Opts::new() });
            if alone.hits.iter().any(Hit::is_place) {
                return Snapped { truncated: alone.truncated || without.truncated, ..alone };
            }
            Snapped { truncated: alone.truncated || without.truncated, ..without }
        } else {
            self.run(query, Opts::new())
        };
        // A subject with sides of its own turned off the page's axes has up to four directions, and the library solves them together (two that are not parallel fix the
        // move, and the combination that snaps the most is taken): it is asked once.
        if self.subject_frame().is_some() {
            return out;
        }
        let lines: Vec<Axis> = out.hits.iter().filter_map(|h| h.axis).collect();
        if out.hits.is_empty() || lines.len() != out.hits.len() || lines.len() >= 2 {
            return out;
        }
        for axis in [Axis::X, Axis::Y] {
            if lines.contains(&axis) {
                continue;
            }
            let outline = if split { OutlineMode::Without } else { OutlineMode::Normal };
            let alone = self.run(query, Opts { constraint: cs::Constraint::Axis(axis_of(axis)), outline, nudge: None });
            out.truncated |= alone.truncated;
            let found = alone.hits.iter().filter(|h| h.axis == Some(axis)).count();
            for h in alone.hits.iter().filter(|h| h.axis == Some(axis)) {
                out.hits.push(*h);
                match axis {
                    Axis::X => out.dx = alone.dx,
                    Axis::Y => out.dy = alone.dy,
                }
            }
            // The lines and gaps drawn on that axis are those of the query that found them: the first query's box had not moved along it.
            if found > 0 {
                out.marks = with_marks_of(std::mem::take(&mut out.marks), &alone.marks, axis);
            }
        }
        out.hits.sort_by_key(|h| h.axis == Some(Axis::Y));
        out.holds = holds_of(&out.hits, self.settings, self.page, query);
        out
    }

    /// A resize of which both sides of an axis move: each side is asked alone (the first side of the other axis stands in while one axis is asked, since an
    /// unturned box's two axes are solved apart), and of each axis the answer with the stronger kind, then the nearer, is the one taken.
    fn one_side_of_each_axis(&mut self, query: &Query<'_>, r: Rect, e: Edges) -> Snapped {
        let xs: Vec<(bool, bool)> = if e.left && e.right { vec![(true, false), (false, true)] } else { vec![(e.left, e.right)] };
        let ys: Vec<(bool, bool)> = if e.bottom && e.top { vec![(true, false), (false, true)] } else { vec![(e.bottom, e.top)] };
        let ask = |call: &mut Call<'_>, x: (bool, bool), y: (bool, bool)| {
            let edges = Edges { left: x.0, right: x.1, bottom: y.0, top: y.1 };
            call.run(&Query { subject: Subject::Edges(r, edges), ..*query }, Opts::new())
        };
        let (x0, y0) = (xs.first().copied().unwrap_or_default(), ys.first().copied().unwrap_or_default());
        let first = ask(self, x0, y0);
        let mut by_x = vec![first.clone()];
        for x in xs.iter().skip(1) {
            by_x.push(ask(self, *x, y0));
        }
        let mut by_y = vec![first.clone()];
        for y in ys.iter().skip(1) {
            by_y.push(ask(self, x0, *y));
        }
        let best = |runs: &[Snapped], axis: Axis| -> Option<(Hit, f64, usize)> {
            runs.iter()
                .enumerate()
                .filter_map(|(i, s)| s.hits.iter().find(|h| h.axis == Some(axis)).map(|h| (*h, if axis == Axis::X { s.dx } else { s.dy }, i)))
                .min_by(|a, b| rank_of(a.0.target).cmp(&rank_of(b.0.target)).then(a.1.abs().total_cmp(&b.1.abs())))
        };
        let mut out = first;
        out.truncated = by_x.iter().chain(&by_y).any(|s| s.truncated);
        out.hits.clear();
        out.holds.clear();
        // The marks are those of the queries that found the hits, each axis from its own; an axis with no hit keeps the first query's marks only if the first did not move it.
        let kept = std::mem::take(&mut out.marks);
        let (moved_x, moved_y) = (out.dx != 0.0, out.dy != 0.0);
        (out.dx, out.dy) = (0.0, 0.0);
        let mut marks: Vec<SnapMark> = kept.iter().filter(|m| mark_axis(m).is_none()).cloned().collect();
        for (axis, runs, moved) in [(Axis::X, &by_x, moved_x), (Axis::Y, &by_y, moved_y)] {
            match best(runs, axis) {
                Some((h, d, i)) => {
                    out.hits.push(h);
                    match axis {
                        Axis::X => out.dx = d,
                        Axis::Y => out.dy = d,
                    }
                    marks.extend(runs.get(i).into_iter().flat_map(|r| r.marks.iter()).filter(|m| mark_axis(m) == Some(axis)).cloned());
                }
                None if !moved => marks.extend(kept.iter().filter(|m| mark_axis(m) == Some(axis)).cloned()),
                None => {}
            }
        }
        out.marks = marks;
        out.holds = holds_of(&out.hits, self.settings, self.page, query);
        out
    }

    /// One query of the library, charged to the call.
    fn run(&mut self, query: &Query<'_>, opts: Opts) -> Snapped {
        let (doc, page, settings, policy) = (self.doc, self.page, self.settings, self.policy);
        let strength = settings.strength;
        let zoom = if super::takes_zoom(query.zoom) { query.zoom } else { DEFAULT_ZOOM };
        let query = Query { zoom, ..*query };
        let mut out = Snapped::default();
        let region = subject_rect(&query.subject);
        if !(region.x0.is_finite() && region.y0.is_finite() && region.x1.is_finite() && region.y1.is_finite()) {
            return out;
        }
        let snapping = settings.snap;
        let gluing = settings.glue && query.using == Use::Connect && matches!(query.subject, Subject::Point(_));
        if !snapping && !gluing {
            return out;
        }
        // The shapes of the page are targets of the Snap switch (and are all off under `InhibitSnap`), and, for the end of a connector, of the Glue switch.
        // A shape being placed is pulled by the options of SnapSettings (snap to handles, vertices, connection points and outlines), and a connector's end by
        // those of GlueSettings: the shape's box and its connection points are what is pulled (ours).
        let shape_snap = snapping && !page.inhibit;
        let placing = shape_snap && query.using == Use::Place;
        let kinds = if matches!(query.subject, Subject::Edges(..)) {
            Kinds::default()
        } else {
            Kinds {
                connection_points: (gluing && settings.glue_connection_points) || (placing && settings.snap_connection_points),
                handles: (gluing && settings.glue_handles) || (placing && settings.snap_handles),
                vertices: (gluing && settings.glue_vertices) || (placing && settings.snap_vertices),
                outline: (gluing && settings.glue_geometry) || (placing && settings.snap_geometry),
                crossings: settings.intersections && shape_snap,
            }
        };
        let kinds = match opts.outline {
            OutlineMode::Normal => kinds,
            OutlineMode::Without => Kinds { outline: false, ..kinds },
            OutlineMode::Alone => Kinds { outline: kinds.outline, ..Kinds::default() },
        };
        let lines_up = shape_snap && (settings.alignment_box || settings.dynamic_grid);
        let objects = lines_up || kinds.any();
        let rules = Rules {
            strength,
            kinds,
            lines_up,
            spacing: shape_snap && settings.dynamic_grid && matches!(query.subject, Subject::Box(_)),
            size: shape_snap && policy.size_match && lines_up && matches!(query.subject, Subject::Edges(..)),
            angle: false,
            step: false,
            guides: (snapping && settings.guides && !page.inhibit) || (gluing && settings.glue_guides && !page.inhibit),
            margins: shape_snap && settings.dynamic_grid && page.size.0 > 0.0 && page.size.1 > 0.0,
            grid: snapping && settings.grid,
            ruler: snapping && settings.ruler,
        };
        // The subject's own frame: told by the caller (the move's or the resize's), and used only where the policy has own sides and the subject has sides (not a point).
        let frame = self.subject_frame().filter(|_| !matches!(query.subject, Subject::Point(_)));
        let built = targets_for(doc, page, query.moving, objects, frame.is_some(), policy);
        let targets = built.targets;
        let (k, zoom) = (page.k, query.zoom);
        let lattices: Vec<cs::Lattice> = [
            rules.grid.then(|| lattice(page.grid.grid(Axis::X, k, zoom), page.grid.grid(Axis::Y, k, zoom), cs::Kind::Grid)).flatten(),
            rules.ruler.then(|| lattice(page.grid.ruler(Axis::X, k, zoom), page.grid.ruler(Axis::Y, k, zoom), cs::Kind::Ruler)).flatten(),
        ]
        .into_iter()
        .flatten()
        .collect();
        let own: Vec<Point> = match query.subject {
            Subject::Box(_) if opts.outline != OutlineMode::Alone => {
                query.own_points.iter().filter(|p| p.0.is_finite() && p.1.is_finite()).take(MAX_OWN_POINTS).map(|p| Point::new(p.0, p.1)).collect()
            }
            _ => Vec::new(),
        };
        // The shapes that line up with a subject are those within the reach of its box and of the places of the shapes that move with it: the reach grows by
        // how far those places lie outside the box.
        let overhang =
            own.iter().map(|p| (region.x0 - p.x).max(p.x - region.x1).max(region.y0 - p.y).max(p.y - region.y1).max(0.0)).fold(0.0, f64::max);
        let align_reach = if policy.align_anywhere { None } else { Some(cs::Reach::Document(super::alignment_reach(&strength, zoom, overhang))) };
        let settings_cs = settings_of(policy, &rules, align_reach);
        let subject = match query.subject {
            Subject::Point(p) => cs::Subject::Point { at: Point::new(p.0, p.1), base: None },
            Subject::Box(r) if opts.nudge.is_some() => {
                let n = opts.nudge.unwrap_or(Nudge { axis: Axis::X, forward: true, tick: 0.0 });
                cs::Subject::Nudge {
                    frame: frame.map_or_else(|| to_frame(&r), to_turned),
                    bounds: frame.map(|_| KRect::new(r.x0, r.y0, r.x1, r.y1)),
                    axis: axis_of(n.axis),
                    forward: n.forward,
                    reach: cs::NudgeReach::Anywhere,
                    fallback: n.tick,
                }
            }
            // A box with a frame of its own aligns by its own sides as well as by the box round its paths, which is the rect it is asked by.
            Subject::Box(r) => cs::Subject::Box {
                frame: frame.map_or_else(|| to_frame(&r), to_turned),
                bounds: frame.map(|_| KRect::new(r.x0, r.y0, r.x1, r.y1)),
                u: cs::Anchors::ALL,
                v: cs::Anchors::ALL,
                own: &own,
            },
            Subject::Edges(r, e) => cs::Subject::Edges {
                frame: frame.map_or_else(|| to_frame(&r), to_turned),
                moving: cs::Edges { u_min: e.left, u_max: e.right, v_min: e.bottom, v_max: e.top },
            },
        };
        // The shapes the person sees, for a size match: one counted scan of the objects' bounds against the view (once for this query).
        let view_index =
            self.extras.view.filter(|v| rules.size && v.iter().all(|c| c.is_finite())).map(|v| targets.in_view(KRect::new(v[0], v[1], v[2], v[3])));
        let q = cs::Query {
            subject,
            zoom,
            settings: &settings_cs,
            constraint: opts.constraint,
            rounding: cs::Rounding::None,
            lattices: &lattices,
            view: view_index.as_ref(),
            budget: self.spent.budget(),
        };
        let no_glue = gluing.then_some(&*built.no_glue);
        let mut detail = DocDetail {
            doc,
            page,
            reader: Reader::new(doc),
            points: kinds.connection_points,
            outlines: kinds.outlines(),
            spent: &mut self.spent,
            no_glue,
        };
        let res = cs::snap(&targets, &q, &mut detail);
        #[cfg(test)]
        LAST.with(|l| *l.borrow_mut() = Some(res.clone()));
        self.spent.add(&res.work);
        // Cut: the page holds more shapes than the index reads, or a query met a cap, or a turned target is lined up by its corners because its paths cost more than the page's
        // budget.
        out.truncated = (objects && (page.shapes(doc).truncated || built.by_corners_for_cost > 0)) || res.work.truncated || self.spent.cut;
        self.map_result(&query, &res, gluing, &mut out);
        // An end is glued to no shape whose layers allow no glue, whatever pulled it there (a handle or a vertex of the box): it stays free.
        if out.glue.is_some_and(|glue| built.no_glue.contains(&glue.shape())) {
            out.glue = None;
        }
        // A key press with no place ahead moves by the tick it was told (the library's fallback), which is no hit: the result is the whole press, whatever the caller (the
        // command, a window's preview) does with it.
        if opts.nudge.is_some() && out.hits.is_empty() && res.delta.x.is_finite() && res.delta.y.is_finite() {
            (out.dx, out.dy) = (res.delta.x, res.delta.y);
        }
        out
    }
}

/// How a query is asked beyond the subject.
#[derive(Clone, Copy)]
struct Opts {
    constraint: cs::Constraint,
    outline: OutlineMode,
    nudge: Option<Nudge>,
}

impl Opts {
    fn new() -> Opts {
        Opts { constraint: cs::Constraint::None, outline: OutlineMode::Normal, nudge: None }
    }
}

/// The page axis a mark of a line or a gap runs along; `None` for a mark of another kind.
fn mark_axis(m: &SnapMark) -> Option<Axis> {
    match m {
        SnapMark::Line { axis, normal: None, .. } | SnapMark::Gap { axis, normal: None, .. } => Some(*axis),
        _ => None,
    }
}

/// `first`'s marks, with those along `axis` taken from `from` instead: a call that asks the library again for one axis draws what that query found there.
fn with_marks_of(first: Vec<SnapMark>, from: &[SnapMark], axis: Axis) -> Vec<SnapMark> {
    let mut marks: Vec<SnapMark> = first.into_iter().filter(|m| mark_axis(m) != Some(axis)).collect();
    marks.extend(from.iter().filter(|m| mark_axis(m) == Some(axis)).cloned());
    marks
}

/// What a query does about the outlines of other shapes (`Kind::Nearest`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum OutlineMode {
    Normal,
    /// The outlines are not pulled to.
    Without,
    /// Only the outlines are pulled to, and only by the nine places of the box: the library offers each point of an outline to every place of the subject, the
    /// moving shapes' own connection points among them, and the engine always paired a point with the place it was found for and left the own places out.
    Alone,
}

/// As a number: the lower wins.
fn rank_of(target: Target) -> u8 {
    match target {
        Target::ConnectionPoint => PRIORITY_CONNECTION_POINT,
        Target::Vertex => PRIORITY_VERTEX,
        Target::Handle => PRIORITY_HANDLE,
        Target::Intersection => PRIORITY_INTERSECTION,
        Target::Guide => PRIORITY_GUIDE,
        Target::Outline => PRIORITY_OUTLINE,
        Target::Alignment | Target::PageMargin | Target::Spacing | Target::Size | Target::Angle => PRIORITY_ALIGNMENT,
        Target::Grid | Target::AngleStep => PRIORITY_GRID,
        Target::Ruler => PRIORITY_RULER,
    }
}

#[cfg(test)]
thread_local! {
    /// What the library gave for the last query of this thread, for the harness to print when the answers differ.
    pub(super) static LAST: std::cell::RefCell<Option<cs::Snapped>> = const { std::cell::RefCell::new(None) };
}

fn to_usize(n: u64) -> usize {
    usize::try_from(n).unwrap_or(usize::MAX)
}

/// The engine's target kind of a library kind.
fn target_of(kind: cs::Kind) -> Option<Target> {
    Some(match kind {
        cs::Kind::Guide => Target::Guide,
        cs::Kind::Edge | cs::Kind::Centre => Target::Alignment,
        cs::Kind::Layout => Target::PageMargin,
        cs::Kind::Grid => Target::Grid,
        cs::Kind::Ruler => Target::Ruler,
        cs::Kind::Spacing => Target::Spacing,
        cs::Kind::Size => Target::Size,
        cs::Kind::Angle => Target::Angle,
        cs::Kind::AngleStep => Target::AngleStep,
        cs::Kind::Point(_) => Target::ConnectionPoint,
        cs::Kind::Corner | cs::Kind::SideMiddle => Target::Handle,
        cs::Kind::Vertex => Target::Vertex,
        cs::Kind::Nearest => Target::Outline,
        cs::Kind::Intersection => Target::Intersection,
        _ => return None,
    })
}

/// The page axis nearer to a direction (the normal of a family of lines): the axis of a line along an axis, and for any other direction the one it is closer to.
fn axis_back(d: cs::Direction) -> Axis {
    let a = d.angle();
    if a < std::f64::consts::FRAC_PI_4 || a >= 3.0 * std::f64::consts::FRAC_PI_4 { Axis::X } else { Axis::Y }
}

/// The page axis a direction is, within the library's tolerance for two directions to be one; `None` for any other (a turned shape's own side, a slanted guide).
fn page_axis(d: cs::Direction) -> Option<Axis> {
    let (a, tol) = (d.angle(), cs::Coincide::default().angle);
    if a <= tol || std::f64::consts::PI - a <= tol {
        Some(Axis::X)
    } else if (a - std::f64::consts::FRAC_PI_2).abs() <= tol {
        Some(Axis::Y)
    } else {
        None
    }
}

/// The angle of the normal of a direction that is not a page axis (the `turn` of a hit or a mark).
fn normal_of(d: cs::Direction) -> Option<f64> {
    page_axis(d).is_none().then(|| d.angle())
}

fn anchor_of(side: Option<(cs::Side, cs::OwnOrBounds)>, subject: &Subject) -> Anchor {
    if matches!(subject, Subject::Point(_)) {
        return Anchor::Point;
    }
    match side {
        Some((cs::Side::Min, _)) => Anchor::Low,
        Some((cs::Side::Mid, _)) => Anchor::Middle,
        Some((cs::Side::Max, _)) => Anchor::High,
        None => Anchor::Point,
    }
}

fn along(r: &Rect, axis: Axis) -> (f64, f64) {
    match axis {
        Axis::X => (r.x0, r.x1),
        Axis::Y => (r.y0, r.y1),
    }
}

fn other(axis: Axis) -> Axis {
    match axis {
        Axis::X => Axis::Y,
        Axis::Y => Axis::X,
    }
}

/// Where each anchor of the subject is on `axis` before the snap.
fn anchor_at(subject: &Subject, axis: Axis, anchor: Anchor) -> f64 {
    match subject {
        Subject::Point(p) => match axis {
            Axis::X => p.0,
            Axis::Y => p.1,
        },
        Subject::Box(r) | Subject::Edges(r, _) => {
            let (lo, hi) = along(r, axis);
            match anchor {
                Anchor::Low => lo,
                Anchor::High => hi,
                _ => (lo + hi) / 2.0,
            }
        }
    }
}

impl Call<'_> {
    fn map_result(&self, query: &Query<'_>, res: &cs::Snapped, gluing: bool, out: &mut Snapped) {
        let (doc, page, settings, policy) = (self.doc, self.page, self.settings, self.policy);
        // A resize of a shape with a frame of its own is told in its own axes.
        if matches!(query.subject, Subject::Edges(..)) && self.subject_frame().is_some() {
            return self.map_turned_edges(query, res, out);
        }
        let region = subject_rect(&query.subject);
        // How far the subject moved on an axis; for a resize the movement of each side, from the box it comes to.
        let after = res.frame.map(|f| f.bounds());
        let moved = |axis: Axis, anchor: Anchor| -> f64 {
            match (&query.subject, after) {
                (Subject::Edges(r, _), Some(f)) => match (axis, anchor) {
                    (Axis::X, Anchor::Low) => f.x0 - r.x0,
                    (Axis::X, _) => f.x1 - r.x1,
                    (Axis::Y, Anchor::Low) => f.y0 - r.y0,
                    (Axis::Y, _) => f.y1 - r.y1,
                },
                _ => match axis {
                    Axis::X => res.delta.x,
                    Axis::Y => res.delta.y,
                },
            }
        };
        let finite = |v: f64| if v.is_finite() { v } else { 0.0 };
        let mut dx = 0.0;
        let mut dy = 0.0;
        for h in &res.hits {
            let Some(target) = target_of(h.kind) else { continue };
            if target == Target::Size {
                // Told below, from its mark: the library's hit says which side was pulled and what it matched, not which of the width and the height.
                continue;
            }
            let anchor = anchor_of(h.side, &query.subject);
            if let Some(direction) = h.direction {
                let Some(axis) = page_axis(direction) else {
                    // A line along no page axis: a side of the turned subject's own, lined up with the like side of a shape turned alike or with a slanted guide parallel to it.
                    if let Some(frame) = self.subject_frame() {
                        out.hits.push(own_hit(target, h, direction, &frame, res, &query.subject));
                    }
                    continue;
                };
                let d = finite(moved(axis, anchor));
                let line = anchor_at(&query.subject, axis, anchor) + d;
                let mut hit = Hit {
                    target,
                    axis: Some(axis),
                    normal: None,
                    line,
                    at: None,
                    anchor,
                    shape: h.id.and_then(|i| u32::try_from(i.0).ok()),
                    point: None,
                    span: None,
                    gap: None,
                };
                match target {
                    Target::Alignment => {
                        // How far the line reaches along its own length: the subject's extent across, as it was dragged, and the shape's.
                        let mine = along(&region, other(axis));
                        let theirs = hit.shape.and_then(|n| page.shapes(doc).find(n)).map(|i| along(&i.bbox, other(axis)));
                        hit.span = theirs.map(|t| (mine.0.min(t.0), mine.1.max(t.1)));
                        // The adoption's line mark runs from the box to the nearest shape on the line, which may not be the shape that pulled.
                        if policy.line_span != cs::LineSpan::Every
                            && let Some(span) = res.marks.iter().find_map(|m| match m {
                                cs::Mark::Line { direction, offset, segment, hit: true, .. }
                                    if page_axis(*direction) == Some(axis) && (*offset - hit.line).abs() <= 1.0e-9 * hit.line.abs().max(1.0) =>
                                {
                                    let (a, b) = match axis {
                                        Axis::X => (segment.p0.y, segment.p1.y),
                                        Axis::Y => (segment.p0.x, segment.p1.x),
                                    };
                                    Some((a.min(b), a.max(b)))
                                }
                                _ => None,
                            })
                        {
                            hit.span = Some(span);
                        }
                    }
                    Target::Spacing => {
                        let mid = (along(&region, axis).0 + along(&region, axis).1) / 2.0;
                        hit.line = mid + d;
                        hit.anchor = Anchor::Middle;
                        let gap = res.marks.iter().find_map(|m| match m {
                            cs::Mark::Gap { direction, segment, value, subject: true, hit: true, .. } if page_axis(*direction) == Some(axis) => {
                                let (a, b) = match axis {
                                    Axis::X => (segment.p0.x, segment.p1.x),
                                    Axis::Y => (segment.p0.y, segment.p1.y),
                                };
                                Some((a.min(b), a.max(b), *value))
                            }
                            _ => None,
                        });
                        if let Some((a, b, value)) = gap {
                            hit.span = Some((a, b));
                            hit.gap = Some(value);
                        }
                    }
                    _ => {}
                }
                match axis {
                    Axis::X => dx = d,
                    Axis::Y => dy = d,
                }
                out.hits.push(hit);
            } else {
                // A place: it pulled both axes at once.
                let Some(at) = res.marks.iter().find_map(|m| match m {
                    cs::Mark::Place { at, kind, id, number, hit: true } if *kind == h.kind && *id == h.id && *number == h.number => Some(*at),
                    _ => None,
                }) else {
                    continue;
                };
                dx = finite(res.delta.x);
                dy = finite(res.delta.y);
                let shape = h.id.and_then(|i| u32::try_from(i.0).ok());
                let point = h.number.filter(|_| target == Target::ConnectionPoint);
                out.hits.push(Hit {
                    target,
                    axis: None,
                    normal: None,
                    line: at.x,
                    at: Some((at.x, at.y)),
                    anchor: Anchor::Point,
                    shape,
                    point,
                    span: None,
                    gap: None,
                });
                if gluing {
                    out.glue = match target {
                        Target::ConnectionPoint => shape.zip(point).map(|(shape, point)| Glue::ConnectionPoint { shape, point }),
                        Target::Vertex => shape.map(|shape| Glue::Vertex { shape }),
                        Target::Handle => shape.map(|shape| Glue::Handle { shape }),
                        Target::Outline => shape.map(|shape| Glue::Outline { shape }),
                        _ => None,
                    };
                }
            }
        }
        // A size match: for each width or height the box was pulled to, the side that moved is the one the resize moves on that axis (one, on each axis, in a query).
        if let Subject::Edges(r, e) = query.subject {
            for m in &res.marks {
                let cs::Mark::Size { value, side, matched, hit: true, .. } = m else { continue };
                let axis = if (side.p0.y - side.p1.y).abs() <= (side.p0.x - side.p1.x).abs() { Axis::X } else { Axis::Y };
                // The side that moved on that axis; and when both sides of it move or neither (a resize that keeps the proportions grows the other axis about its middle), the
                // middle of the box, which is what the length was matched about.
                let sides = match axis {
                    Axis::X => (e.left, e.right),
                    Axis::Y => (e.bottom, e.top),
                };
                let anchor = match sides {
                    (true, false) => Anchor::Low,
                    (false, true) => Anchor::High,
                    _ => Anchor::Middle,
                };
                let d = match (anchor, after) {
                    (Anchor::Middle, Some(f)) => {
                        let (now, then) = (along(&r, axis), along(&Rect { x0: f.x0, y0: f.y0, x1: f.x1, y1: f.y1 }, axis));
                        finite((then.0 + then.1) / 2.0 - (now.0 + now.1) / 2.0)
                    }
                    (Anchor::Middle, None) => 0.0,
                    _ => finite(moved(axis, anchor)),
                };
                let line = anchor_at(&query.subject, axis, anchor) + d;
                out.hits.push(Hit {
                    target: Target::Size,
                    axis: Some(axis),
                    normal: None,
                    line,
                    at: None,
                    anchor,
                    shape: matched.and_then(|(id, _)| u32::try_from(id.0).ok()),
                    point: None,
                    span: None,
                    gap: Some(*value),
                });
                match axis {
                    Axis::X => dx = d,
                    Axis::Y => dy = d,
                }
            }
        }
        // A turned subject is moved by the library's correction, which a line along one of its own sides makes along that line's normal: both numbers are the move.
        if self.subject_frame().is_some() {
            (dx, dy) = (finite(res.delta.x), finite(res.delta.y));
        }
        out.dx = dx;
        out.dy = dy;
        out.hits.sort_by_key(|h| match (h.axis, h.normal) {
            (None, None) => 0,
            (Some(Axis::X), _) => 1,
            (Some(Axis::Y), _) => 2,
            (None, Some(_)) => 3,
        });
        out.holds = holds_of(&out.hits, settings, page, query);
        out.marks = marks_of(res, page);
        if self.extras.keep && matches!(query.subject, Subject::Edges(..)) {
            out.landed = res.frame.map(|f| f.bounds()).map(|b| Rect { x0: b.x0, y0: b.y0, x1: b.x1, y1: b.y1 });
        }
    }
}

/// Whether two directions of the library are one, within its tolerance for it and across the half turn where a direction just under it is the one just over 0.
fn same_direction(a: cs::Direction, b: cs::Direction) -> bool {
    let (d, tol) = ((a.angle() - b.angle()).abs(), cs::Coincide::default().angle);
    d <= tol || std::f64::consts::PI - d <= tol
}

/// Where the own side of a turned subject lies once it has moved by `delta`, along the normal of `direction` (a folded unit normal, as the library has it): the centre of its
/// frame along it, and half the frame's length that way (its width along its own x axis, its height along its own y axis) either side.
fn own_side_line(frame: &OwnFrame, delta: Vec2, direction: cs::Direction, side: Option<cs::Side>) -> f64 {
    let n = direction.normal();
    let u = cs::frame::unit(frame.angle);
    let v = Vec2::new(-u.y, u.x);
    let half = if n.dot(u).abs() >= n.dot(v).abs() { frame.width } else { frame.height } / 2.0;
    let centre = n.dot(Vec2::new(frame.centre.0 + delta.x, frame.centre.1 + delta.y));
    match side {
        Some(cs::Side::Min) => centre - half,
        Some(cs::Side::Max) => centre + half,
        _ => centre,
    }
}

/// The hit on a line along no page axis: a side of a turned subject's own frame (a hit of the move `res`) with the like side of another shape or with a slanted guide. Its
/// `normal` is the line's, its `line` the line's offset along it, and the span and the gap are those of the marks of the same line.
fn own_hit(target: Target, h: &cs::Hit, direction: cs::Direction, frame: &OwnFrame, res: &cs::Snapped, subject: &Subject) -> Hit {
    let delta = Vec2::new(if res.delta.x.is_finite() { res.delta.x } else { 0.0 }, if res.delta.y.is_finite() { res.delta.y } else { 0.0 });
    let n = direction.normal();
    let t = Vec2::new(-n.y, n.x);
    let mut hit = Hit {
        target,
        axis: None,
        normal: Some(direction.angle()),
        line: own_side_line(frame, delta, direction, h.side.map(|(s, _)| s)),
        at: None,
        anchor: anchor_of(h.side, subject),
        shape: h.id.and_then(|i| u32::try_from(i.0).ok()),
        point: None,
        span: None,
        gap: None,
    };
    let sorted = |a: f64, b: f64| (a.min(b), a.max(b));
    match target {
        Target::Alignment => {
            hit.span = res.marks.iter().find_map(|m| match m {
                cs::Mark::Line { direction: d, offset, segment, hit: true, .. }
                    if same_direction(*d, direction) && (*offset - hit.line).abs() <= 1.0e-9 * hit.line.abs().max(1.0) =>
                {
                    Some(sorted(t.dot(segment.p0.to_vec2()), t.dot(segment.p1.to_vec2())))
                }
                _ => None,
            });
        }
        Target::Spacing => {
            hit.line = own_side_line(frame, delta, direction, None);
            hit.anchor = Anchor::Middle;
            if let Some((span, value)) = res.marks.iter().find_map(|m| match m {
                cs::Mark::Gap { direction: d, segment, value, subject: true, hit: true, .. } if same_direction(*d, direction) => {
                    Some((sorted(n.dot(segment.p0.to_vec2()), n.dot(segment.p1.to_vec2())), *value))
                }
                _ => None,
            }) {
                hit.span = Some(span);
                hit.gap = Some(value);
            }
        }
        _ => {}
    }
    hit
}

/// The offset of the line a hit of a resize lies on: that of the mark of a line in its direction that was snapped to, preferring the one that names the shape and the kind the hit
/// names (the moving side carries three places, and which of them met the line is the library's to say).
fn line_of_hit(res: &cs::Snapped, h: &cs::Hit, direction: cs::Direction) -> Option<f64> {
    let marks = || {
        res.marks.iter().filter_map(|m| match m {
            cs::Mark::Line { direction: d, offset, hit: true, first, kinds, .. } if same_direction(*d, direction) => Some((*offset, *first, kinds)),
            _ => None,
        })
    };
    marks().find(|(_, first, kinds)| *first == h.id && kinds.contains(&h.kind)).or_else(|| marks().next()).map(|(offset, ..)| offset)
}

impl Call<'_> {
    /// The result of a resize of a turned or flipped shape (`Extras::frame` and `Subject::Edges`), told in the shape's own axes: the box it comes to (`frame`), a hit for each
    /// side that was pulled (the line it is on, and the side), a hit for each width or height that was matched, and no move along a page axis (`dx` and `dy` are 0).
    fn map_turned_edges(&self, query: &Query<'_>, res: &cs::Snapped, out: &mut Snapped) {
        let (page, settings) = (self.page, self.settings);
        let Subject::Edges(_, e) = query.subject else { return };
        out.frame = res.frame.as_ref().map(from_turned);
        for h in &res.hits {
            let Some(target) = target_of(h.kind) else { continue };
            // A size is told below, from its mark; a resize is pulled by lines and sizes only.
            let Some(direction) = h.direction.filter(|_| target != Target::Size) else { continue };
            let anchor = match h.side {
                Some((cs::Side::Min, _)) => Anchor::Low,
                Some((cs::Side::Max, _)) => Anchor::High,
                Some((cs::Side::Mid, _)) => Anchor::Middle,
                None => Anchor::Point,
            };
            out.hits.push(Hit {
                target,
                axis: page_axis(direction),
                normal: normal_of(direction),
                line: line_of_hit(res, h, direction).filter(|l| l.is_finite()).unwrap_or(0.0),
                at: None,
                anchor,
                shape: h.id.and_then(|i| u32::try_from(i.0).ok()),
                point: None,
                span: None,
                gap: None,
            });
        }
        // A size match: the width (along the shape's own x axis) or the height, whichever the mark's side runs along, pulled by the side that moves on that axis (the middle,
        // for both or neither: the proportions grow the other axis about it).
        if let Some(f) = &res.frame {
            let (u, v) = (f.u(), f.v());
            for m in &res.marks {
                let cs::Mark::Size { value, side, matched, hit: true, .. } = m else { continue };
                let run = side.p1 - side.p0;
                let width = run.dot(u).abs() >= run.dot(v).abs();
                let (axis, sides) = if width { (Axis::X, (e.left, e.right)) } else { (Axis::Y, (e.bottom, e.top)) };
                let anchor = match sides {
                    (true, false) => Anchor::Low,
                    (false, true) => Anchor::High,
                    _ => Anchor::Middle,
                };
                out.hits.push(Hit {
                    target: Target::Size,
                    axis: Some(axis),
                    normal: None,
                    line: *value,
                    at: None,
                    anchor,
                    shape: matched.and_then(|(id, _)| u32::try_from(id.0).ok()),
                    point: None,
                    span: None,
                    gap: Some(*value),
                });
            }
        }
        out.hits.sort_by_key(|h| match (h.axis, h.normal) {
            (Some(Axis::X), _) => 1,
            (Some(Axis::Y), _) => 2,
            (None, Some(_)) => 3,
            (None, None) => 0,
        });
        out.holds = holds_of(&out.hits, settings, page, query);
        out.marks = marks_of(res, page);
    }
}

/// The library's marks in the engine's words. A mark of a kind the engine has no word for is left out; one along a direction that is not a page axis is told with its `normal`.
fn marks_of(res: &cs::Snapped, page: &SnapPage) -> Vec<SnapMark> {
    let unit = page.grid.ruler_unit;
    let p = |p: Point| (p.x, p.y);
    let shape = |id: Option<cs::Id>| id.and_then(|i| u32::try_from(i.0).ok());
    res.marks
        .iter()
        .filter_map(|m| match m {
            cs::Mark::Line { direction, offset, segment, kinds, first, count, hit } => {
                let mut targets: Vec<Target> = Vec::new();
                for t in kinds.iter().filter_map(|k| target_of(*k)) {
                    if !targets.contains(&t) {
                        targets.push(t);
                    }
                }
                Some(SnapMark::Line {
                    axis: axis_back(*direction),
                    normal: normal_of(*direction),
                    at: *offset,
                    from: p(segment.p0),
                    to: p(segment.p1),
                    targets,
                    shape: shape(*first),
                    count: *count,
                    hit: *hit,
                })
            }
            cs::Mark::Gap { direction, segment, value, chain, subject, hit } => Some(SnapMark::Gap {
                axis: axis_back(*direction),
                normal: normal_of(*direction),
                from: p(segment.p0),
                to: p(segment.p1),
                value: *value,
                label: label::length(*value, page.k, unit),
                chain: *chain,
                own: *subject,
                hit: *hit,
            }),
            cs::Mark::Size { value, side, matched, count, hit } => Some(SnapMark::Size {
                value: *value,
                label: label::length(*value, page.k, unit),
                side: (p(side.p0), p(side.p1)),
                matched: matched.and_then(|(id, l)| Some((u32::try_from(id.0).ok()?, (p(l.p0), p(l.p1))))),
                count: *count,
                hit: *hit,
            }),
            cs::Mark::Angle { pivot, angle, matched, step, hit } => Some(SnapMark::Angle {
                pivot: p(*pivot),
                degrees: angle.to_degrees(),
                label: label::angle(angle.to_degrees()),
                matched: shape(*matched),
                step: step.map(f64::to_degrees),
                hit: *hit,
            }),
            cs::Mark::Place { at, kind, id, number, hit } => {
                let target = target_of(*kind)?;
                Some(SnapMark::Place {
                    at: p(*at),
                    target,
                    shape: shape(*id),
                    point: number.filter(|_| target == Target::ConnectionPoint),
                    hit: *hit,
                })
            }
        })
        .collect()
}

/// A box placed onto a guide is held to it when the Glue switch and glue to guides are on (a place takes both axes and holds nothing).
fn holds_of(hits: &[Hit], settings: &SnapSettings, page: &SnapPage, query: &Query<'_>) -> Vec<Hold> {
    if !(query.using == Use::Place && !matches!(query.subject, Subject::Point(_)) && settings.glue && settings.glue_guides && !page.inhibit) {
        return Vec::new();
    }
    hits.iter()
        .filter_map(|h| match (h.target, h.axis, h.shape) {
            (Target::Guide, Some(axis), Some(guide)) => Some(Hold { guide, axis, anchor: h.anchor, line: h.line }),
            _ => None,
        })
        .collect()
}

/// A turn by the rotation handle (`snap_turn`): one library query on the page's angles and the step, charged like any call.
pub(super) fn snap_turn(doc: &Document, page: &SnapPage, settings: &SnapSettings, moving: &[u32], turn: Turn) -> Snapped {
    let mut out = Snapped::default();
    let finite = [turn.pivot.0, turn.pivot.1, turn.angle, turn.radius].iter().all(|v| v.is_finite());
    if !finite {
        return out;
    }
    let zoom = if super::takes_zoom(turn.zoom) { turn.zoom } else { DEFAULT_ZOOM };
    let policy = Policy::ADOPTED;
    // The step is the handle's, and needs no switch; matching another shape's angle is a snap, and needs the Snap switch (and a page that does not inhibit it).
    let matching = settings.snap && !page.inhibit;
    let step = super::rotation_step(turn.radius.max(0.0) * zoom).to_radians();
    let rules = Rules {
        strength: settings.strength,
        kinds: Kinds::default(),
        lines_up: false,
        spacing: false,
        size: false,
        angle: matching,
        step: true,
        guides: false,
        margins: false,
        grid: false,
        ruler: false,
    };
    let call = Call { policy, doc, page, settings, extras: Extras::default(), spent: Spent::new() };
    let targets = targets_for(doc, page, moving, matching, false, policy).targets;
    let settings_cs = settings_of(policy, &rules, None);
    let q = cs::Query {
        subject: cs::Subject::Rotate {
            pivot: Point::new(turn.pivot.0, turn.pivot.1),
            angle: turn.angle,
            radius: turn.radius.max(0.0),
            step: Some((step, 0.0)),
        },
        zoom,
        settings: &settings_cs,
        constraint: cs::Constraint::None,
        rounding: cs::Rounding::None,
        lattices: &[],
        view: None,
        budget: call.spent.budget(),
    };
    let res = cs::snap(&targets, &q, &mut cs::NoDetail);
    out.truncated = res.work.truncated || (matching && page.shapes(doc).truncated);
    out.turned = res.angle.filter(|a| a.is_finite());
    for h in &res.hits {
        let Some(target) = target_of(h.kind) else { continue };
        let degrees = out.turned.map_or(0.0, f64::to_degrees);
        out.hits.push(Hit {
            target,
            axis: None,
            normal: None,
            line: degrees,
            at: None,
            anchor: Anchor::Point,
            shape: h.id.and_then(|i| u32::try_from(i.0).ok()),
            point: None,
            span: None,
            gap: None,
        });
    }
    out.marks = marks_of(&res, page);
    let mut spent = call.spent;
    spent.add(&res.work);
    spent.tell(&mut out);
    out
}

#[cfg(test)]
mod frame_tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    /// The corners, in the order of the shape's own frame, of a `w` by `h` shape whose pin is at its middle at `(5, 5)`, turned `degrees`.
    fn corners(w: f64, h: f64, degrees: f64) -> [P; 4] {
        let (s, c) = degrees.to_radians().sin_cos();
        let at = |x: f64, y: f64| (5.0 + (x - w / 2.0) * c - (y - h / 2.0) * s, 5.0 + (x - w / 2.0) * s + (y - h / 2.0) * c);
        [at(0.0, 0.0), at(w, 0.0), at(w, h), at(0.0, h)]
    }

    #[test]
    fn a_rectangle_is_a_frame_of_its_size_at_the_angle_of_its_own_x_edge() {
        let f = frame_of(&corners(2.0, 1.0, 30.0)).unwrap();
        assert!((f.size.width - 2.0).abs() < 1e-12 && (f.size.height - 1.0).abs() < 1e-12, "{f:?}");
        assert!((f.angle - 30.0_f64.to_radians()).abs() < 1e-12 && (f.centre.x - 5.0).abs() < 1e-12 && (f.centre.y - 5.0).abs() < 1e-12, "{f:?}");
        // A shape flipped in x has its own x edge pointing the other way: the angle is a half turn more, the box the same.
        let mut flipped = corners(2.0, 1.0, 0.0);
        flipped.swap(0, 1);
        flipped.swap(2, 3);
        let f = frame_of(&flipped).unwrap();
        assert!(
            (f.angle.abs() - std::f64::consts::PI).abs() < 1e-12 && (f.size.width - 2.0).abs() < 1e-12 && (f.centre.x - 5.0).abs() < 1e-12,
            "{f:?}"
        );
    }

    #[test]
    fn corners_that_are_no_rectangle_with_a_size_are_no_frame() {
        let p = |a: [(f64, f64); 4]| frame_of(&a);
        // Skewed: the sides are parallel in pairs and not square to each other (the sides are orthogonal in the next one and the quad is no parallelogram).
        assert!(p([(0.0, 0.0), (2.0, 0.0), (2.5, 1.0), (0.5, 1.0)]).is_none());
        assert!(p([(0.0, 0.0), (2.0, 0.0), (2.5, 1.5), (0.0, 1.0)]).is_none());
        // No width, no height, and numbers that are not finite.
        assert!(p([(0.0, 0.0), (0.0, 0.0), (0.0, 1.0), (0.0, 1.0)]).is_none());
        assert!(p([(0.0, 0.0), (2.0, 0.0), (2.0, 0.0), (0.0, 0.0)]).is_none());
        assert!(p([(0.0, 0.0), (f64::NAN, 0.0), (2.0, 1.0), (0.0, 1.0)]).is_none());
        assert!(p([(0.0, 0.0), (f64::INFINITY, 0.0), (f64::INFINITY, 1.0), (0.0, 1.0)]).is_none());
    }

    #[test]
    fn the_sides_of_a_frame_may_be_off_square_by_a_few_billionths_and_not_by_more() {
        // The fourth corner moved along the first side by `e`: the sides' dot is 2 e and the tolerance on it 6 billionths (a billionth of the two sides added, times the
        // longer), and the skew e is held to 3 billionths.
        let shifted = |e: f64| {
            let mut c = corners(2.0, 1.0, 30.0);
            let (s, co) = 30.0_f64.to_radians().sin_cos();
            c[3] = (c[3].0 + e * co, c[3].1 + e * s);
            frame_of(&c)
        };
        assert!(shifted(1.0e-9).is_some() && shifted(2.5e-9).is_some());
        assert!(shifted(1.0e-8).is_none());
    }

    /// The corners moved by `off` in both coordinates: the sums round as a shape's corners do with its pin that far from the origin.
    fn far(c: [P; 4], off: f64) -> [P; 4] {
        c.map(|p| (p.0 + off, p.1 + off))
    }

    #[test]
    fn a_rectangle_far_from_the_origin_is_a_frame_with_the_rounding_of_its_corners_allowed() {
        // The 2 by 0.5 shape turned 30 degrees, with its corners at 1e4 to 1e10 inches: a frame, its size the shape's and its angle within the rounding of its corners.
        for off in [0.0, 1.0e4, 1.0e5, 1.0e6, 1.0e7, 1.0e8, 1.0e9, 1.0e10] {
            let f = frame_of(&far(corners(2.0, 0.5, 30.0), off)).unwrap_or_else(|| panic!("no frame at {off:e}"));
            let slack = 1.0e-9 + 64.0 * f64::EPSILON * off;
            assert!((f.size.width - 2.0).abs() < slack && (f.size.height - 0.5).abs() < slack, "{off:e}: {f:?}");
            assert!((f.angle - 30.0_f64.to_radians()).abs() < slack, "{off:e}: {f:?}");
        }
        // A skew of a thousandth of an inch is no frame there either, and a box smaller than the rounding of its corners has none.
        let mut skewed = far(corners(2.0, 0.5, 30.0), 1.0e8);
        skewed[2].0 += 1.0e-3;
        assert!(frame_of(&skewed).is_none());
        assert!(frame_of(&far(corners(0.001, 0.001, 30.0), 1.0e12)).is_none());
        assert!(frame_of(&far(corners(2.0, 0.5, 30.0), 1.0e300)).is_none());
    }

    #[test]
    fn a_skew_of_the_size_of_a_tolerance_is_a_frame_and_a_larger_one_is_not() {
        // The tolerance is a billionth of the larger of the shape's two sides added and 1 (3 here): 2.5 billionths of a unit is within it, a millionth is not.
        let nudge = |d: f64| frame_of(&[(0.0, 0.0), (2.0, 0.0), (2.0 + d, 1.0), (0.0, 1.0)]);
        assert!(nudge(2.5e-9).is_some() && nudge(0.0).is_some());
        assert!(nudge(1.0e-6).is_none());
    }

    #[test]
    fn the_angle_the_cells_say_is_the_frames_when_the_corners_agree_to_a_millionth_of_a_radian_and_not_otherwise() {
        let a = 30.0_f64.to_radians();
        let c = corners(2.0, 1.0, 30.0);
        // Within a millionth of a radian either way round the corners' angle, and a whole turn more or less: the cells'.
        for cells in [a + 5.0e-7, a - 5.0e-7, a + std::f64::consts::TAU + 5.0e-7, a - std::f64::consts::TAU - 5.0e-7] {
            assert_eq!(frame_from(&c, cells).unwrap().angle, cells, "{cells}");
        }
        // Further off, a half turn off, or no number: the corners' angle, whatever the cells say.
        for cells in [a + 2.0e-6, a - 2.0e-6, a + 0.1, a - 0.1, a + std::f64::consts::PI, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let got = frame_from(&c, cells).unwrap().angle;
            assert!((got - a).abs() < 1.0e-12 && got != cells, "{cells}: {got}");
        }
    }

    #[test]
    fn the_cells_angle_is_taken_far_from_the_origin_by_as_much_as_the_rounding_of_the_corners_over_the_size() {
        // A shape 0.002 inch square turned 30 degrees with its corners at 1e8 inches: the corners are known to 1.5e-8, so their angle is off by more than a millionth of a radian,
        // and the angle of the cells (exact) is taken because the difference is within a few steps of the corners over the size.
        let a = 30.0_f64.to_radians();
        let c = far(corners(0.002, 0.002, 30.0), 1.0e8);
        let from_corners = frame_of(&c).unwrap().angle;
        assert!((from_corners - a).abs() > 1.0e-6, "the corners alone are off: {from_corners} against {a}");
        assert_eq!(frame_from(&c, a).unwrap().angle, a);
        // The cells that are off by more than that rounding allows are not taken, there either.
        assert_eq!(frame_from(&c, a + 0.01).unwrap().angle, from_corners);
    }
}

#[cfg(test)]
mod spent_tests {
    use super::*;

    #[test]
    fn the_first_budget_of_a_call_is_the_engines_caps_and_each_later_one_is_what_the_call_has_left() {
        let mut spent = Spent::new();
        let b = spent.budget();
        assert_eq!(
            (b.window_visits, b.candidates, b.own, b.points, b.outlines, b.pieces, b.pairs, b.row),
            (
                MAX_VISITS as u32,
                MAX_CANDIDATES as u32,
                MAX_OWN_POINTS as u32,
                MAX_TARGET_POINTS as u32,
                MAX_CANDIDATES as u32,
                MAX_TARGET_PIECES as u32,
                MAX_PATH_PAIRS as u32,
                MAX_CANDIDATES as u32
            )
        );
        // The caps the library sets for itself are not the engine's to change.
        let own = cs::Budget::default();
        assert_eq!((b.line_steps, b.solves, b.marks, b.detail), (own.line_steps, own.solves, own.marks, own.detail));
        // A query that spent most of the call's points, pieces and pairs leaves the rest to the next.
        spent.add(&cs::Work { points: 4_000, pieces: 16_000, pairs: 60, ..cs::Work::default() });
        let b = spent.budget();
        assert_eq!((b.points, b.pieces, b.pairs), (96, 384, 4));
        // Spending more than a cap leaves none, and does not wrap.
        spent.add(&cs::Work { points: u64::MAX, pieces: 1 << 40, pairs: 100, ..cs::Work::default() });
        let b = spent.budget();
        assert_eq!((b.points, b.pieces, b.pairs), (0, 0, 0));
    }
}

#[cfg(test)]
mod marks_tests {
    use super::*;

    fn line(axis: Axis, at: f64) -> SnapMark {
        SnapMark::Line { axis, normal: None, at, from: (0.0, 0.0), to: (1.0, 1.0), targets: vec![Target::Guide], shape: None, count: 1, hit: true }
    }

    fn gap(axis: Axis, value: f64) -> SnapMark {
        SnapMark::Gap { axis, normal: None, from: (0.0, 0.0), to: (1.0, 0.0), value, label: String::new(), chain: 1, own: true, hit: true }
    }

    fn place() -> SnapMark {
        SnapMark::Place { at: (1.0, 1.0), target: Target::Handle, shape: None, point: None, hit: true }
    }

    #[test]
    fn the_marks_of_an_axis_come_from_the_query_that_found_them_and_the_others_stay() {
        assert_eq!((mark_axis(&line(Axis::X, 1.0)), mark_axis(&gap(Axis::Y, 1.0)), mark_axis(&place())), (Some(Axis::X), Some(Axis::Y), None));
        let first = vec![line(Axis::X, 1.0), line(Axis::Y, 2.0), gap(Axis::Y, 0.5), place()];
        let from = vec![line(Axis::X, 9.0), line(Axis::Y, 3.0), gap(Axis::Y, 0.7), place()];
        // The marks along y are the later query's, the marks along x and the places the first's.
        assert_eq!(with_marks_of(first.clone(), &from, Axis::Y), vec![line(Axis::X, 1.0), place(), line(Axis::Y, 3.0), gap(Axis::Y, 0.7)]);
        assert_eq!(with_marks_of(first.clone(), &from, Axis::X), vec![line(Axis::Y, 2.0), gap(Axis::Y, 0.5), place(), line(Axis::X, 9.0)]);
        // A later query with nothing along the axis takes the first's marks along it away.
        assert_eq!(with_marks_of(first, &[], Axis::Y), vec![line(Axis::X, 1.0), place()]);
    }
}

#[cfg(test)]
mod direction_tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI};

    use super::*;

    fn d(angle: f64) -> cs::Direction {
        cs::Direction::new(angle).unwrap()
    }

    #[test]
    fn a_direction_is_a_page_axis_within_a_billionth_of_a_radian_across_the_half_turn_too() {
        // The library's tolerance for two directions to be one (1e-9 radian): x is the normal at 0 and, folded, just under a half turn; y is the one a quarter turn from it.
        for (angle, axis) in [
            (0.0, Some(Axis::X)),
            (5.0e-10, Some(Axis::X)),
            (2.0e-9, None),
            (PI - 5.0e-10, Some(Axis::X)),
            (PI - 2.0e-9, None),
            (FRAC_PI_2, Some(Axis::Y)),
            (FRAC_PI_2 + 5.0e-10, Some(Axis::Y)),
            (FRAC_PI_2 - 5.0e-10, Some(Axis::Y)),
            (FRAC_PI_2 + 2.0e-9, None),
            (FRAC_PI_2 - 2.0e-9, None),
            (0.5, None),
        ] {
            assert_eq!(page_axis(d(angle)), axis, "{angle}");
            assert_eq!(normal_of(d(angle)).is_some(), axis.is_none(), "{angle}");
        }
    }

    #[test]
    fn the_axis_nearer_to_a_direction_is_x_up_to_an_eighth_of_a_turn_and_from_three_eighths_of_one() {
        assert_eq!(axis_back(d(0.1)), Axis::X);
        assert_eq!(axis_back(d(FRAC_PI_4 - 1.0e-6)), Axis::X);
        assert_eq!(axis_back(d(FRAC_PI_4)), Axis::Y);
        assert_eq!(axis_back(d(FRAC_PI_2)), Axis::Y);
        assert_eq!(axis_back(d(3.0 * FRAC_PI_4 - 1.0e-6)), Axis::Y);
        assert_eq!(axis_back(d(3.0 * FRAC_PI_4)), Axis::X);
        assert_eq!(axis_back(d(PI - 0.1)), Axis::X);
    }

    #[test]
    fn two_directions_are_one_within_a_billionth_of_a_radian_across_the_half_turn_too() {
        assert!(same_direction(d(1.0), d(1.0)) && same_direction(d(1.0), d(1.0 + 5.0e-10)) && same_direction(d(1.0 + 5.0e-10), d(1.0)));
        assert!(!same_direction(d(1.0), d(1.0 + 2.0e-9)) && !same_direction(d(1.0), d(2.0)) && !same_direction(d(0.0), d(FRAC_PI_2)));
        assert!(same_direction(d(4.0e-10), d(PI - 4.0e-10)) && same_direction(d(PI - 4.0e-10), d(4.0e-10)));
        assert!(!same_direction(d(2.0e-9), d(PI - 2.0e-9)));
    }

    #[test]
    fn the_line_of_an_own_side_is_the_frames_centre_along_the_normal_and_half_its_length_that_way_either_side() {
        // A frame 4 by 2 turned a quarter turn about (3, 5): its x axis runs up the page, so its length along a normal at 0 (x) is its height and along a normal at 90 degrees (y) its width.
        let f = OwnFrame { centre: (3.0, 5.0), width: 4.0, height: 2.0, angle: FRAC_PI_2 };
        let delta = Vec2::new(0.5, 0.25);
        let (x, y) = (d(0.0), d(FRAC_PI_2));
        assert_eq!([cs::Side::Min, cs::Side::Mid, cs::Side::Max].map(|s| own_side_line(&f, delta, x, Some(s))), [2.5, 3.5, 4.5]);
        assert_eq!([cs::Side::Min, cs::Side::Mid, cs::Side::Max].map(|s| own_side_line(&f, delta, y, Some(s))), [3.25, 5.25, 7.25]);
        assert_eq!(own_side_line(&f, delta, x, None), 3.5);
    }

    #[test]
    fn a_frame_turned_an_eighth_of_a_turn_has_its_width_along_a_normal_along_its_x_and_its_height_along_one_along_its_y() {
        // The normal along the frame's own x axis (45 degrees) runs across its width, the one along its own y axis (135 degrees) across its height.
        let f = OwnFrame { centre: (3.0, 5.0), width: 4.0, height: 2.0, angle: FRAC_PI_4 };
        let along = |angle: f64, side: cs::Side| own_side_line(&f, Vec2::new(0.0, 0.0), d(angle), Some(side));
        let r = std::f64::consts::FRAC_1_SQRT_2;
        let (c_x, c_y) = ((3.0 + 5.0) * r, (-3.0 + 5.0) * r);
        assert!((along(FRAC_PI_4, cs::Side::Min) - (c_x - 2.0)).abs() < 1.0e-12 && (along(FRAC_PI_4, cs::Side::Max) - (c_x + 2.0)).abs() < 1.0e-12);
        assert!(
            (along(3.0 * FRAC_PI_4, cs::Side::Min) - (c_y - 1.0)).abs() < 1.0e-12
                && (along(3.0 * FRAC_PI_4, cs::Side::Max) - (c_y + 1.0)).abs() < 1.0e-12
        );
    }
}

#[cfg(test)]
mod policy_tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};

    use super::*;

    fn shape(id: u32, pin: (f64, f64), degrees: f64) -> ShapeBuilder {
        ShapeBuilder::new(id)
            .cell("PinX", &pin.0.to_string())
            .cell("PinY", &pin.1.to_string())
            .cell("Width", "2")
            .cell("Height", "1")
            .cell("LocPinX", "1")
            .cell("LocPinY", "0.5")
            .cell("Angle", &degrees.to_radians().to_string())
    }

    #[test]
    fn the_snapping_of_before_the_library_ignores_the_frame_a_caller_tells_and_the_adopted_policy_uses_it() {
        // A shape turned 30 degrees with a guide 0.06 inch past the right of its box and one 0.06 past the top: 0.085 apart together, past a guide's pull. With the frame told, the
        // adopted policy asks once and takes one guide; the policy of before the library (what the harness holds the adapter to) takes each axis apart, as a box square to the page
        // does, whatever the caller says: two guides.
        let mut doc =
            DocBuilder::new().page(1, PageBuilder::new().cell("PageWidth", "20").cell("PageHeight", "20").shape(shape(1, (5.0, 5.0), 30.0))).build();
        let b = crate::snap::bounds_of(&doc, 1, &[1]).unwrap().rect;
        crate::snap::guides::add_guide(&mut doc, 1, Axis::X, b.x1 + 0.06, 0).unwrap();
        crate::snap::guides::add_guide(&mut doc, 1, Axis::Y, b.y1 + 0.06, 0).unwrap();
        let page = SnapPage::read(&doc, 1).unwrap();
        let mut settings = SnapSettings::none();
        settings.snap = true;
        settings.guides = true;
        // The premise is a guide's pull of 8 points (0.0833 inch), which the two guides together, 0.085 apart, are past (the spec's default is 10).
        settings.strength.guides = 8;
        let query = Query { subject: Subject::Box(b), using: Use::Place, moving: &[1], zoom: 96.0, own_points: &[] };
        let ask = |policy: Policy, extras: Extras| snap_in(policy, extras, &doc, &page, &settings, &query);
        let told = Extras { frame: crate::snap::frame_of_shapes(&doc, 1, &[1]), ..Extras::default() };
        assert!(told.frame.is_some());
        assert_eq!(ask(Policy::ADOPTED, told).hits.len(), 1);
        assert_eq!(ask(Policy::ADOPTED, Extras::default()).hits.len(), 2);
        assert_eq!(format!("{:?}", ask(Policy::TODAY, told)), format!("{:?}", ask(Policy::TODAY, Extras::default())));
        assert_eq!(ask(Policy::TODAY, told).hits.len(), 2);
    }

    /// A page of boxes, each `(id, x0, y0, x1, y1)`, 20 inches square.
    fn boxes(list: &[(u32, f64, f64, f64, f64)]) -> Document {
        let mut page = PageBuilder::new().cell("PageWidth", "20").cell("PageHeight", "20");
        for (id, x0, y0, x1, y1) in list {
            let (w, h) = (x1 - x0, y1 - y0);
            page = page.shape(
                ShapeBuilder::new(*id)
                    .cell("PinX", &((x0 + x1) / 2.0).to_string())
                    .cell("PinY", &((y0 + y1) / 2.0).to_string())
                    .cell("Width", &w.to_string())
                    .cell("Height", &h.to_string())
                    .cell("LocPinX", &(w / 2.0).to_string())
                    .cell("LocPinY", &(h / 2.0).to_string())
                    .cell("Angle", "0"),
            );
        }
        DocBuilder::new().page(1, page).build()
    }

    /// One press of the right arrow on shape 1 (the box (2, 4) to (3, 5)) among `others`: what each policy says, the policy of before the library (an alignment reach of 2 inches
    /// across the box, the rest of the page unseen) and the adopted one (the whole page).
    fn press_right(others: &[(u32, f64, f64, f64, f64)]) -> (Snapped, Snapped) {
        let mut list = vec![(1, 2.0, 4.0, 3.0, 5.0)];
        list.extend_from_slice(others);
        let doc = boxes(&list);
        let page = SnapPage::read(&doc, 1).unwrap();
        let mut settings = SnapSettings::none();
        settings.snap = true;
        settings.alignment_box = true;
        settings.dynamic_grid = true;
        let query = Query {
            subject: Subject::Box(Rect { x0: 2.0, y0: 4.0, x1: 3.0, y1: 5.0 }),
            using: Use::Place,
            moving: &[1],
            zoom: 96.0,
            own_points: &[],
        };
        let extras = Extras { nudge: Some(Nudge { axis: Axis::X, forward: true, tick: 0.125 }), ..Extras::default() };
        let ask = |policy: Policy| snap_in(policy, extras, &doc, &page, &settings, &query);
        (ask(Policy::TODAY), ask(Policy::ADOPTED))
    }

    #[test]
    fn a_key_press_whose_nearest_line_of_a_kind_is_behind_the_box_finds_the_next_line_ahead_and_not_a_farther_stop() {
        // The box (2, 4) to (3, 5); shape 2 (0, 5.5) to (2, 6.5) has its right edge on the box's left edge and its middle behind it; shape 3 (4.5, 5.5) to (6.5, 6.5)
        // is 1.5 inches ahead of the box's right side. For each side the nearest line of its kind is behind or at the side (shape 2's edges at 2 and 0, its middle at 1), and the policy of
        // before the library, which looks at the shapes within 2 inches of the box, read that nearest line, dropped it for not being ahead and never looked at shape 3: the key went on to
        // the page's right margin, 16.75 inches on, a stop that is not limited by the reach (with nothing ahead at all it would have moved the tick). The library now asks for the nearest
        // line strictly past the side, so the press stops at shape 3's left edge, 1.5 inches on (the box's right side onto it: any side meets any line under that policy). The adopted
        // policy, which sees the whole page and has no reach, always found shape 3, and lines a side up with the same side of it: the left side, 2.5 inches on.
        let (today, adopted) = press_right(&[(2, 0.0, 5.5, 2.0, 6.5), (3, 4.5, 5.5, 6.5, 6.5)]);
        assert_eq!((today.dx, today.dy, today.hits.len()), (1.5, 0.0, 1), "{today:?}");
        let h = today.hits[0];
        assert_eq!((h.target, h.axis, h.line, h.anchor, h.shape), (Target::Alignment, Some(Axis::X), 4.5, Anchor::High, Some(3)), "{h:?}");
        assert_eq!((adopted.dx, adopted.dy, adopted.hits.len()), (2.5, 0.0, 1), "{adopted:?}");
        let h = adopted.hits[0];
        assert_eq!((h.target, h.axis, h.line, h.anchor, h.shape), (Target::Alignment, Some(Axis::X), 4.5, Anchor::Low, Some(3)), "{h:?}");
        // Past the reach (shape 3 at 7.5, 4.5 inches ahead of the side and beyond the 2 inches round the box), the old policy does not see it and the press goes to the margin; the
        // adopted policy, which sees the whole page, goes to it (its left side onto the left edge, 5.5 inches).
        let (today, adopted) = press_right(&[(2, 0.0, 5.5, 2.0, 6.5), (3, 7.5, 5.5, 9.5, 6.5)]);
        assert_eq!((today.dx, today.hits.len(), today.hits[0].target), (16.75, 1, Target::PageMargin), "{today:?}");
        assert_eq!((adopted.dx, adopted.hits.len()), (5.5, 1), "{adopted:?}");
    }
}
