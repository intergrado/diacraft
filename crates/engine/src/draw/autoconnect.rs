//! AutoConnect: drop a connected copy of a shape, or a chosen master, beside it in one command ("AutoConnect arrows and
//! flyout", "Drop a master on an AutoConnect arrow").
//!
//! Our rule where no specification gives the geometry. A shape has four arrows, one to a side. [`autoconnect`] takes one of them:
//!
//! * the new shape is made by the caller's `make` (a copy of the shape, or an instance of a master, by the very functions `shape.duplicate` and
//!   `shape.drop` use) and then moved into place, so that whatever size it comes out is the size it is placed by;
//! * it goes **in the direction of the arrow**, its centre in line with the source's centre, with a **gap** between the two boxes (a shape's box on the
//!   page, a turned shape's being the box that holds it) of the page's `AvenueSizeX` (left and right) or `AvenueSizeY` (up and down), which a published
//!   account has AutoConnect use (`community-report`), and [`DEFAULT_GAP`] where the page holds none;
//! * a connector is glued from the source's connection point that **faces that way** (its direction through the shape's turns and flips, the nearest to
//!   the middle of that side if several face it) to the new shape's point that faces back; where a shape has no point facing so, the nearest pair is
//!   taken. The connector is the page's default style, routed as `connector.add` routes it.
//!
//! Our rule: a place that another shape of the page already holds is slid along, to the next free one on the perpendicular line:
//! a box's width (or height) and a gap further, alternately to one side and the other, up to [`MAX_SLIDES`] times. So a second arrow on the same side
//! makes a second shape beside the first and not on top of it. Only the first [`MAX_OBSTACLES`] shapes of the page are looked at; when the
//! place stays taken, the shape goes to the first.
//!
//! (ours; no specification says which shape an arrow that is clicked joins, has the arrows and the
//! drag only): a click on an arrow joins the **nearest neighbour** in its direction when there is one, and makes a copy when there is none. The
//! neighbour of shape `from` on the arrow `dir` ([`neighbour`]) is the top-level shape of the page, other than a connector, a guide or `from`, that lies **beyond that
//! side of `from`'s box** (its near side no nearer than the side) **and shares some of its extent across** (the boxes' ranges along the other axis overlap by more
//! than nothing, so a shape that is only diagonal is not "to the right"), the nearest first (a tie goes to the one whose centre is nearer the line through `from`'s
//! centre, then to the lower `ID`), **that has a connection point** to be glued to. A group has none (`connector.add` refuses it) and is passed over. **If the nearest
//! such shape is joined to `from` already** by a connector (either way round) there is no neighbour: the click makes a copy, as an arrow does that has nothing to join,
//! and never a second connector between the same two shapes; the shapes beyond are not looked at. [`autoconnect_join`] joins `from` to a shape that is there (a click's
//! neighbour, or the shape a drag from an arrow was let go on) with the connector, from `from`'s point that faces the arrow to the nearest point of
//! the other shape, or to the one a drag named.
//!
//! One undo step: the shape, its move and the connector.

use std::collections::HashMap;

use diacraft_doc::Document;

use super::connectors::{ConnectorInfo, End, attach_connector_above};
use super::read::Reader;
use super::route::{Dir, P};
use super::settle::settle;
use super::shapes::move_shapes;
use super::{Problem, require_page};

/// The gap between a shape and the one AutoConnect drops beside it, in inches of paper, where the page has no `AvenueSizeX` or `AvenueSizeY`.
pub const DEFAULT_GAP: f64 = 0.75;

/// How many places beside the first AutoConnect tries when the first is taken.
pub const MAX_SLIDES: usize = 8;

/// How many shapes of the page are looked at for a place that is taken, and for a neighbour.
pub const MAX_OBSTACLES: usize = 5_000;

/// How many `Connect` rows of the page are looked at to know whether two shapes are joined, and how many connectors that touch either shape are
/// kept for it: past either the shapes are taken as not joined, which a click answers with a copy.
pub const MAX_CONNECTS_LOOKED_AT: usize = 200_000;
const MAX_JOINED_KEPT: usize = 4_096;

/// How many rows of the `Connection` section a shape may hold to be a neighbour: the snap's own limit for the points it reads of the shapes round a pointer.
const MAX_POINT_ROWS: usize = 256;

/// What AutoConnect made.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AutoConnected {
    /// The new shape's `ID`.
    pub shape: u32,
    pub connector: ConnectorInfo,
    /// Where the new shape's centre is, on the page.
    pub centre: P,
}

/// A box: left, bottom, right, top.
type Rect = (f64, f64, f64, f64);

fn bounds(corners: &[P; 4]) -> Rect {
    corners
        .iter()
        .fold((f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY), |b, p| (b.0.min(p.0), b.1.min(p.1), b.2.max(p.0), b.3.max(p.1)))
}

fn centre_of(b: Rect) -> P {
    ((b.0 + b.2) / 2.0, (b.1 + b.3) / 2.0)
}

/// Whether two boxes share an area.
fn overlap(a: Rect, b: Rect) -> bool {
    a.0 < b.2 - 1.0e-9 && b.0 < a.2 - 1.0e-9 && a.1 < b.3 - 1.0e-9 && b.1 < a.3 - 1.0e-9
}

/// The gap AutoConnect leaves on a page for an arrow `dir`, in drawing units.
fn gap(reader: &Reader<'_>, page: u32, dir: Dir) -> f64 {
    let sheet = diacraft_doc::SheetRef::Page(page);
    let cell = if dir.horizontal() { "AvenueSizeX" } else { "AvenueSizeY" };
    let held = reader.number(&sheet, cell);
    if held.is_finite() && held > 0.0 { held } else { DEFAULT_GAP * super::units_per_inch(reader.doc(), page) }
}

/// The centre a new shape of `size` has beside `source` on the arrow `dir`, `gap` away, and the perpendicular step to the next place.
fn centre_for(source: Rect, dir: Dir, gap: f64, size: (f64, f64), slide: f64) -> P {
    let c = centre_of(source);
    match dir {
        Dir::Right => (source.2 + gap + size.0 / 2.0, c.1 + slide),
        Dir::Left => (source.0 - gap - size.0 / 2.0, c.1 + slide),
        Dir::Up => (c.0 + slide, source.3 + gap + size.1 / 2.0),
        Dir::Down => (c.0 + slide, source.1 - gap - size.1 / 2.0),
    }
}

/// Where a shape of `size` would have its centre beside shape `from` on the arrow `dir`: the first place, before any slide. For a
/// window to draw the preview of what an arrow offers. `None` for a shape that is not there or is a connector.
pub fn autoconnect_centre(doc: &Document, page: u32, from: u32, dir: Dir, size: (f64, f64)) -> Option<P> {
    let reader = Reader::new(doc);
    let at = reader.find(page, from)?;
    if reader.is_one_d(&diacraft_doc::SheetRef::Shape(at.clone())) {
        return None;
    }
    let source = bounds(&reader.corners(&at));
    Some(centre_for(source, dir, gap(&reader, page, dir), size, 0.0))
}

/// The middle of the side of `r` that faces `out`.
fn side_of(r: Rect, out: Dir) -> P {
    let c = centre_of(r);
    match out {
        Dir::Right => (r.2, c.1),
        Dir::Left => (r.0, c.1),
        Dir::Up => (c.0, r.3),
        Dir::Down => (c.0, r.1),
    }
}

/// The connection point of shape `at` that faces `dir` and is nearest `near`: its `IX`.
fn facing(reader: &Reader<'_>, at: &diacraft_doc::ShapeRef, dir: Dir, near: P) -> Option<u32> {
    reader
        .connection_points(at)
        .into_iter()
        .filter(|p| Dir::from_vector(p.dir_x, p.dir_y) == Some(dir))
        .min_by(|a, b| {
            let d = |p: &super::read::ConnectionPoint| (p.x - near.0).powi(2) + (p.y - near.1).powi(2);
            d(a).total_cmp(&d(b))
        })
        .map(|p| p.index)
}

/// The boxes of the top-level shapes of the page other than the connectors, guides, the source and the new shape: what a place is taken by.
fn obstacles(reader: &Reader<'_>, doc: &Document, page: u32, except: [u32; 2]) -> Vec<Rect> {
    let Some(p) = doc.page(page) else { return Vec::new() };
    p.shapes
        .iter()
        .take(MAX_OBSTACLES)
        .enumerate()
        // A guide takes no place: it is an aid that holds `ShapePermeable*` 1, and its box, if a file gives it one, is not a shape.
        .filter(|(_, s)| !except.contains(&s.id) && s.kind != diacraft_doc::ShapeType::Guide)
        .filter_map(|(i, _)| {
            let at = diacraft_doc::ShapeRef { owner: diacraft_doc::Owner::Page(page), path: vec![u32::try_from(i).ok()?] };
            if reader.is_one_d(&diacraft_doc::SheetRef::Shape(at.clone())) {
                return None;
            }
            Some(bounds(&reader.corners(&at)))
        })
        .collect()
}

/// AutoConnect: makes a shape with `make` (given the source's centre, where it is made, and returning its `ID`), moves it beside shape `from` on the
/// arrow `dir`, and glues a connector from `from` to it. `above` is the session's `ID` floor.
pub fn autoconnect(
    doc: &mut Document,
    page: u32,
    from: u32,
    dir: Dir,
    above: u32,
    make: impl FnOnce(&mut Document, P) -> Result<u32, Problem>,
) -> Result<AutoConnected, Problem> {
    require_page(doc, page)?;
    let before = doc.clone();
    let (source, source_centre, gap) = {
        let reader = Reader::new(doc);
        let at = reader.find(page, from).ok_or_else(|| Problem::bad(format!("page {page} has no shape {from}")))?;
        if reader.is_one_d(&diacraft_doc::SheetRef::Shape(at.clone())) {
            return Err(Problem::bad(format!("shape {from} is a connector and has no AutoConnect arrows")));
        }
        let source = bounds(&reader.corners(&at));
        if !(source.0.is_finite() && source.1.is_finite() && source.2.is_finite() && source.3.is_finite()) {
            return Err(Problem::failed(format!("shape {from} has no box to place a shape beside")));
        }
        (source, centre_of(source), gap(&reader, page, dir))
    };
    let shape = make(doc, source_centre)?;
    let (now, size, blocked) = {
        let reader = Reader::new(doc);
        let at = reader.find(page, shape).ok_or_else(|| Problem::failed("the shape that was made is not on the page"))?;
        let box_now = bounds(&reader.corners(&at));
        let blocked = obstacles(&reader, doc, page, [from, shape]);
        (centre_of(box_now), (box_now.2 - box_now.0, box_now.3 - box_now.1), blocked)
    };
    // The first free place along the perpendicular: this one, then a box and a gap to one side, to the other, and so on.
    let across = if dir.horizontal() { size.1 } else { size.0 };
    let step = across + gap;
    let free = |centre: P| {
        let mine = (centre.0 - size.0 / 2.0, centre.1 - size.1 / 2.0, centre.0 + size.0 / 2.0, centre.1 + size.1 / 2.0);
        !blocked.iter().any(|b| overlap(mine, *b))
    };
    let mut target = centre_for(source, dir, gap, size, 0.0);
    if !free(target) {
        for n in 1..=MAX_SLIDES {
            let k = n.div_ceil(2) as f64;
            let next = centre_for(source, dir, gap, size, if n % 2 == 1 { step * k } else { -step * k });
            if free(next) {
                target = next;
                break;
            }
        }
    }
    if !(target.0.is_finite() && target.1.is_finite()) {
        return Err(Problem::failed("the place beside the shape is not a place on the page"));
    }
    move_shapes(doc, page, &[shape], target.0 - now.0, target.1 - now.1)?;
    let (begin, end) = {
        let reader = Reader::new(doc);
        let from_at = reader.find(page, from).ok_or_else(|| Problem::failed("the shape went away"))?;
        let to_at = reader.find(page, shape).ok_or_else(|| Problem::failed("the shape went away"))?;
        let (a, b) = (bounds(&reader.corners(&from_at)), bounds(&reader.corners(&to_at)));
        (facing(&reader, &from_at, dir, side_of(a, dir)), facing(&reader, &to_at, dir.opposite(), side_of(b, dir.opposite())))
    };
    let info = attach_connector_above(doc, page, &End::Shape { id: from, point: begin }, &End::Shape { id: shape, point: end }, above.max(shape))?;
    settle(doc, &before, page, &[info.id], &[])?;
    let centre = {
        let reader = Reader::new(doc);
        reader.find(page, shape).map_or(target, |at| centre_of(bounds(&reader.corners(&at))))
    };
    Ok(AutoConnected { shape, connector: info, centre })
}

/// Whether a box has finite sides.
fn is_finite(b: Rect) -> bool {
    b.0.is_finite() && b.1.is_finite() && b.2.is_finite() && b.3.is_finite()
}

/// How far the box `b` is beyond the side of `source` that faces `dir` (0 where it touches the side or the two overlap along the arrow), and how far its centre is from
/// the line through the middle of `source` along the arrow. `None` when `b` is not beyond that side at all, or shares no extent with `source` across it.
fn lane(source: Rect, b: Rect, dir: Dir) -> Option<(f64, f64)> {
    const EPS: f64 = 1.0e-9;
    let (beyond, overlap, offset) = match dir {
        Dir::Right => (b.0 - source.2, b.3.min(source.3) - b.1.max(source.1), (centre_of(b).1 - centre_of(source).1).abs()),
        Dir::Left => (source.0 - b.2, b.3.min(source.3) - b.1.max(source.1), (centre_of(b).1 - centre_of(source).1).abs()),
        Dir::Up => (b.1 - source.3, b.2.min(source.2) - b.0.max(source.0), (centre_of(b).0 - centre_of(source).0).abs()),
        Dir::Down => (source.1 - b.3, b.2.min(source.2) - b.0.max(source.0), (centre_of(b).0 - centre_of(source).0).abs()),
    };
    (beyond >= -EPS && overlap > EPS && beyond.is_finite() && offset.is_finite()).then(|| (beyond.max(0.0), offset))
}

/// Whether a connector of the page is glued to shape `a` at one end and shape `b` at the other.
fn joined(doc: &Document, page: u32, a: u32, b: u32) -> bool {
    let Some(p) = doc.page(page) else { return false };
    let mut touching: HashMap<u32, u8> = HashMap::new();
    for c in p.connects.iter().take(MAX_CONNECTS_LOOKED_AT) {
        let bit = if c.to_sheet == a {
            1
        } else if c.to_sheet == b {
            2
        } else {
            continue;
        };
        if !c.from_cell.as_deref().is_some_and(|name| matches!(name, "BeginX" | "BeginY" | "EndX" | "EndY")) {
            continue;
        }
        if touching.len() >= MAX_JOINED_KEPT && !touching.contains_key(&c.from_sheet) {
            continue;
        }
        let held = touching.entry(c.from_sheet).or_insert(0);
        *held |= bit;
        if *held == 3 {
            return true;
        }
    }
    false
}

/// The shape a click on the arrow `dir` of shape `from` joins on `page`: the nearest neighbour in that direction, or `None` when there is none, when
/// the nearest is joined to `from` already, or when `from` is not a shape of the page that has arrows (a connector, a shape that is not there). A shape whose layers allow no
/// glue is passed over, as a group is.
pub fn neighbour(doc: &Document, page: u32, from: u32, dir: Dir) -> Option<u32> {
    let reader = Reader::new(doc);
    let at = reader.find(page, from)?;
    if reader.is_one_d(&diacraft_doc::SheetRef::Shape(at.clone())) {
        return None;
    }
    let source = bounds(&reader.corners(&at));
    if !is_finite(source) {
        return None;
    }
    let p = doc.page(page)?;
    // A shape whose layers allow no glue is not offered: no connector is glued to it. The layers' standing costs a walk of the page only when a layer is not plain.
    let no_glue: std::collections::HashSet<u32> = super::layers::page_standing(doc, page)
        .map(|standing| standing.into_iter().filter(|(_, st)| st.no_glue).map(|(id, _)| id).collect())
        .unwrap_or_default();
    // The shapes in the lane, as (distance, offset across, ID, place in the page's list).
    let mut found: Vec<(f64, f64, u32, usize)> = Vec::new();
    for (i, s) in p.shapes.iter().take(MAX_OBSTACLES).enumerate() {
        if s.id == from || s.del || s.kind == diacraft_doc::ShapeType::Guide || no_glue.contains(&s.id) {
            continue;
        }
        let Some(path) = u32::try_from(i).ok() else { break };
        let candidate = diacraft_doc::ShapeRef { owner: diacraft_doc::Owner::Page(page), path: vec![path] };
        if reader.is_one_d(&diacraft_doc::SheetRef::Shape(candidate.clone())) {
            continue;
        }
        let b = bounds(&reader.corners(&candidate));
        if let Some((distance, offset)) = lane(source, b, dir) {
            found.push((distance, offset, s.id, i));
        }
    }
    found.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.total_cmp(&y.1)).then(x.2.cmp(&y.2)));
    // The nearest that a connector can be glued to; a group has no point and is passed over.
    let nearest = found.into_iter().find(|(.., i)| {
        let candidate = diacraft_doc::ShapeRef { owner: diacraft_doc::Owner::Page(page), path: vec![u32::try_from(*i).unwrap_or(u32::MAX)] };
        reader.connection_points_within(&candidate, MAX_POINT_ROWS).is_some_and(|points| !points.is_empty())
    })?;
    (!joined(doc, page, from, nearest.2)).then_some(nearest.2)
}

/// AutoConnect to a shape that is there: glues a connector from the connection point of shape `from` that faces the arrow `dir` to shape `to`, at its point `to_point` or
/// at the one nearest the other end, and moves nothing. `above` is the session's `ID` floor. The result's `shape` is `to` and its `centre` the middle
/// of `to`'s box.
pub fn autoconnect_join(doc: &mut Document, page: u32, from: u32, dir: Dir, to: (u32, Option<u32>), above: u32) -> Result<AutoConnected, Problem> {
    require_page(doc, page)?;
    let (to, to_point) = to;
    if to == from {
        return Err(Problem::bad(format!("shape {from} cannot be joined to itself: `to` names another shape")));
    }
    let before = doc.clone();
    let begin = {
        let reader = Reader::new(doc);
        let from_at = reader.find(page, from).ok_or_else(|| Problem::bad(format!("page {page} has no shape {from}")))?;
        let to_at = reader.find(page, to).ok_or_else(|| Problem::bad(format!("page {page} has no shape {to}")))?;
        for (id, at) in [(from, &from_at), (to, &to_at)] {
            if reader.is_one_d(&diacraft_doc::SheetRef::Shape(at.clone())) {
                return Err(Problem::bad(format!("shape {id} is a connector: AutoConnect joins shapes")));
            }
        }
        let source = bounds(&reader.corners(&from_at));
        if !is_finite(source) {
            return Err(Problem::failed(format!("shape {from} has no box to join from")));
        }
        facing(&reader, &from_at, dir, side_of(source, dir))
    };
    let info = attach_connector_above(doc, page, &End::Shape { id: from, point: begin }, &End::Shape { id: to, point: to_point }, above)?;
    settle(doc, &before, page, &[info.id], &[])?;
    let centre = {
        let reader = Reader::new(doc);
        let at = reader.find(page, to).ok_or_else(|| Problem::failed("the shape went away"))?;
        centre_of(bounds(&reader.corners(&at)))
    };
    Ok(AutoConnected { shape: to, connector: info, centre })
}
