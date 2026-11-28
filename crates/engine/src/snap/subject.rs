//! What the editing commands snap, as functions a window calls too: the box of the shapes that move, their own connection
//! points that are pulled with it, the edges a resize moves, and a resize's snap. The commands (`cmd/snap_params.rs`, `cmd/shape.rs`, `cmd/placement.rs`)
//! and the window's previews call these same functions, so that a preview lands where the release does; a window keeps a [`SnapPage`] for the frames of
//! a drag and passes it in, where a command reads its own. Everything is in paper inches, y up.

use std::collections::HashMap;

use diacraft_doc::{Document, Owner, ShapeRef, SheetRef};

use super::geometry::PathBox;
use super::{
    Anchor, Axis, Edges, MAX_CONNECTION_ROWS, MAX_CONNECTION_ROWS_PER_QUERY, MAX_OWN_POINTS, P, Query, Rect, SnapPage, SnapSettings, Snapped,
};
use super::{Extras, OutlineWork, OwnFrame, Subject, Use, snap_ext};
use crate::draw::{Reader, units_per_inch};

/// The most of the moving shapes whose connection points are pulled with the box.
pub const MAX_OWN_SHAPES: usize = 16;

/// The most shapes whose joint box `shape.move` snaps (each one's box is built, and a turned one's paths read, for the snap; the shapes are found in one walk of the page,
/// where a search for each walked the page for each); a move of more with `snap` is refused.
pub const MAX_SNAPPED_SHAPES: usize = 512;

/// A box as `shape.resize` takes it: the place of its lower left corner, then its width and height, in paper inches.
pub type BoxAt = (f64, f64, f64, f64);

/// The places of the shapes `ids` that are pulled with their box (their connection points, in paper inches), moved by `shift`: at most [`MAX_OWN_POINTS`]
/// of them, read from at most [`MAX_OWN_SHAPES`] of the shapes, and a `Connection` section of more rows than [`MAX_CONNECTION_ROWS`] is not read. Each is
/// its place plus `shift`, so the places for a shift of (0, 0) moved by a shift later are these to the last bit.
pub fn own_points(doc: &Document, page: u32, ids: &[u32], shift: P) -> Vec<P> {
    let reader = Reader::new(doc);
    let k = units_per_inch(doc, page);
    let mut out: Vec<P> = Vec::new();
    // The rows of the connection points are counted and charged before they are merged, to one budget for the shapes that move.
    let mut rows_left = MAX_CONNECTION_ROWS_PER_QUERY;
    let ids = ids.get(..MAX_OWN_SHAPES).unwrap_or(ids);
    let found = found_on(doc, page, ids.iter().copied());
    for id in ids {
        let Some(at) = found.get(id) else { continue };
        if !pull(&reader, k, at, shift, &mut rows_left, &mut out) {
            return out;
        }
    }
    out
}

/// [`own_points`] of each of the shapes `moves` names, moved by the shift beside it and **each under a budget of its own**, in that order: what a call of [`own_points`] for each
/// shape gives (`shape.place`, which moves each shape by its own distance), for at most [`MAX_OWN_SHAPES`] of them, with the shapes found in one walk of the page
/// where a call for each searched the page from its top for each.
pub fn own_points_each(doc: &Document, page: u32, moves: &[(u32, P)]) -> Vec<P> {
    let reader = Reader::new(doc);
    let k = units_per_inch(doc, page);
    let moves = moves.get(..MAX_OWN_SHAPES).unwrap_or(moves);
    let found = found_on(doc, page, moves.iter().map(|(id, _)| *id));
    let mut out: Vec<P> = Vec::new();
    for (id, shift) in moves {
        let Some(at) = found.get(id) else { continue };
        let (mut rows_left, mut mine) = (MAX_CONNECTION_ROWS_PER_QUERY, Vec::new());
        pull(&reader, k, at, *shift, &mut rows_left, &mut mine);
        out.extend(mine);
    }
    out
}

/// Adds the connection points of the shape at `at`, moved by `shift`, to `out`, its rows charged to `rows_left` before they are merged: a section of more rows than
/// [`MAX_CONNECTION_ROWS`], or than are left, is not read. `false` when `out` was full ([`MAX_OWN_POINTS`]) and a point was left out.
fn pull(reader: &Reader<'_>, k: f64, at: &ShapeRef, shift: P, rows_left: &mut usize, out: &mut Vec<P>) -> bool {
    let rows = reader.connection_rows(at);
    if rows > MAX_CONNECTION_ROWS || rows > *rows_left {
        return true;
    }
    *rows_left -= rows;
    for cp in reader.connection_points_within(at, rows).unwrap_or_default() {
        if out.len() >= MAX_OWN_POINTS {
            return false;
        }
        out.push((cp.x / k + shift.0, cp.y / k + shift.1));
    }
    true
}

/// Where the shapes `ids` are on the page, found in one walk of it, where a search for each walked the page from its top for each. The callers answer with no box and
/// no points rather than an error, so a page that is not there (or holds more shapes than a page may) is an answer of no shapes, as each search on it found none.
fn found_on(doc: &Document, page: u32, ids: impl IntoIterator<Item = u32>) -> HashMap<u32, ShapeRef> {
    crate::draw::find_named(doc, page, &ids.into_iter().collect()).unwrap_or_default()
}

/// The pin of the shape `id` on the page, in paper inches: where its `LocPinX` and `LocPinY` are carried to the page, the point `shape.rotate` turns the shape about and where a
/// turn's angle mark is drawn. A shape's pin is not the middle of its box, and in a turned group it is not its cells' `PinX` and `PinY` either. `None` for a shape that is not
/// there or whose pin is not a finite number.
pub fn pin_of(doc: &Document, page: u32, id: u32) -> Option<P> {
    let reader = Reader::new(doc);
    let at = doc.find_shape(Owner::Page(page), id)?;
    let sheet = SheetRef::Shape(at.clone());
    let k = units_per_inch(doc, page);
    let p = reader.to_page(&at, (reader.number(&sheet, "LocPinX"), reader.number(&sheet, "LocPinY")));
    (p.0.is_finite() && p.1.is_finite()).then_some((p.0 / k, p.1 / k))
}

/// The box of the shapes `ids` on the page, in paper inches: the corners of each carried to the page. `None` if one is not there.
pub fn box_of(doc: &Document, page: u32, ids: &[u32]) -> Option<Rect> {
    let reader = Reader::new(doc);
    let k = units_per_inch(doc, page);
    let mut points = Vec::with_capacity(ids.len().saturating_mul(4));
    let found = found_on(doc, page, ids.iter().copied());
    for id in ids {
        let at = found.get(id)?;
        points.extend(reader.corners(at).map(|p| (p.0 / k, p.1 / k)));
    }
    Rect::round(&points)
}

/// The own frame of the shape `ids` name, when they name one shape that is turned by other than whole quarter turns and whose corners on the page make a rectangle with a size
/// Its centre, its width and height as drawn and the direction of its own x axis, in paper inches. `None` for a shape that is not turned (its own
/// sides are the page's, a quarter turn and a flip included), for a selection of more than one shape (the box of a selection is square to the page), and for a shape that is not
/// there. The command (`shape.move`, `shape.place`) and the window's preview call this one function with [`bounds_of`], so that the shape's own sides line up where the release does.
pub fn frame_of_shapes(doc: &Document, page: u32, ids: &[u32]) -> Option<OwnFrame> {
    let first = ids.first()?;
    if ids.len() > MAX_SNAPPED_SHAPES || ids.iter().any(|id| id != first) {
        return None;
    }
    let reader = Reader::new(doc);
    let k = units_per_inch(doc, page);
    let at = doc.find_shape(Owner::Page(page), *first)?;
    let corners = reader.corners(&at).map(|p| (p.0 / k, p.1 / k));
    let pose = reader.pose(&SheetRef::Shape(at));
    super::adapter::own_frame(&corners, pose.angle + if pose.flip_x { std::f64::consts::PI } else { 0.0 })
}

/// What [`bounds_of`] made: the box the moving shapes align by, how many turned shapes among them kept the box round their corners, and what building the
/// boxes round the others' paths cost.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SubjectBounds {
    pub rect: Rect,
    /// The turned shapes whose paths the budget could not pay for, that draw none, or that are a group of too many members.
    pub by_corners: usize,
    /// The query's geometry budget, spent (the counts a counted test and a window read).
    pub cost: OutlineWork,
}

/// The box the shapes `ids` align by when they move, in paper inches: for each shape turned by other than whole quarter turns, the box round the
/// paths it draws, else the box round its corners; the box round those. **One budget, the query's, pays for the whole selection** (`geometry::Budget::new`, charged before
/// each section is built), so a selection of many shapes costs the budget and not an outline each; a shape it cannot pay for, or one that draws no path, keeps the box
/// round its corners. → the box, how many turned shapes kept the box round their corners, and the cost; `None` if a shape is not there. The command (`shape.move`) and the
/// window's preview call this one function, so that they get one box.
pub fn bounds_of(doc: &Document, page: u32, ids: &[u32]) -> Option<SubjectBounds> {
    let each = bounds_each(doc, page, ids)?;
    let points: Vec<P> = each.rects.iter().flat_map(|r| [(r.x0, r.y0), (r.x1, r.y1)]).collect();
    Rect::round(&points).map(|rect| SubjectBounds { rect, by_corners: each.by_corners, cost: each.cost })
}

/// What [`bounds_each`] made: the box each of the shapes aligns by (in the order asked), how many turned shapes among them kept the box round their corners, and what building
/// the boxes round the others' paths cost.
#[derive(Clone, Debug, PartialEq)]
pub struct EachBounds {
    pub rects: Vec<Rect>,
    pub by_corners: usize,
    pub cost: OutlineWork,
}

/// The box each of the shapes `ids` aligns by, one at a time and under **one budget for all of them** (the query's, charged before each section is built): the box round the
/// paths of a shape turned by other than whole quarter turns, else the box round its corners ([`bounds_of`] is the box round these). `shape.place` moves each shape by its own
/// distance, so it needs each box and not their joint one. `None` if a shape is not there.
pub fn bounds_each(doc: &Document, page: u32, ids: &[u32]) -> Option<EachBounds> {
    let reader = Reader::new(doc);
    let k = units_per_inch(doc, page);
    let mut budget = super::geometry::Budget::new();
    let mut rects = Vec::with_capacity(ids.len());
    let mut by_corners = 0usize;
    let found = found_on(doc, page, ids.iter().copied());
    for id in ids {
        let at = found.get(id)?;
        let corners = reader.corners(at).map(|p| (p.0 / k, p.1 / k));
        let round = Rect::round(&corners)?;
        if super::index::is_turned(&corners) {
            match super::geometry::path_box(&reader, doc, at, k, &mut budget) {
                PathBox::Box(b) => {
                    rects.push(Rect { x0: b.x0, y0: b.y0, x1: b.x1, y1: b.y1 });
                    continue;
                }
                PathBox::Nothing | PathBox::Cut => by_corners += 1,
            }
        }
        rects.push(round);
    }
    Some(EachBounds { rects, by_corners, cost: budget.spent() })
}

/// The edges a resize moves: those that differ from the shape's present box by more than a millionth of an inch.
pub fn moved_edges(now: &Rect, then: &Rect) -> Edges {
    let differs = |a: f64, b: f64| (a - b).abs() > 1.0e-6;
    Edges { left: differs(now.x0, then.x0), bottom: differs(now.y0, then.y0), right: differs(now.x1, then.x1), top: differs(now.y1, then.y1) }
}

/// Whether a shape's box on the page is square to it (not turned, not flipped, and in no group that is): only such a shape is snapped by a resize.
pub fn is_square(doc: &Document, page: u32, id: u32) -> bool {
    let reader = Reader::new(doc);
    let Some(at) = doc.find_shape(Owner::Page(page), id) else { return false };
    let pose = reader.pose(&SheetRef::Shape(at.clone()));
    !pose.is_turned_or_flipped() && !reader.groups_turn(&at)
}

/// A turned or flipped shape's own axes, as a resize takes them: where the corner at its own (0, 0) is, the unit vectors of its own x and y axes (carried to the page, a flip
/// turning one of them round), and its size and the direction of its x axis. `shape.resize` takes `(x, y, width, height)` of this corner and these axes, and the library's frame
/// has its own y axis a quarter turn from its x axis, which a flip turns the other way: `hand` says which.
#[derive(Clone, Copy, Debug)]
struct OwnAxes {
    id: u32,
    c0: P,
    u: P,
    v: P,
    w: f64,
    h: f64,
    angle: f64,
}

impl OwnAxes {
    /// The axes of a shape from its corners on the page (in the order of its own frame), when they make a rectangle with a size.
    fn of(id: u32, corners: &[P; 4], cells: f64) -> Option<OwnAxes> {
        let f = super::adapter::frame_from(corners, cells)?;
        let (c0, c1, c3) = (*corners.first()?, *corners.get(1)?, *corners.get(3)?);
        let (w, h) = (f.size.width, f.size.height);
        // The own x axis is the direction the shape's cells say; its own y axis is a quarter turn from it, or the other way round for a shape flipped in one axis only (the
        // corners say which). Far from the origin the corners' differences are not as exact as the cells.
        let u = craft_snap::frame::unit(f.angle);
        let cross = (c1.0 - c0.0) * (c3.1 - c0.1) - (c1.1 - c0.1) * (c3.0 - c0.0);
        let v = if cross >= 0.0 { (-u.y, u.x) } else { (u.y, -u.x) };
        Some(OwnAxes { id, c0, u: (u.x, u.y), v, w, h, angle: f.angle })
    }

    /// The frame of the box `(x, y, width, height)`: its corner at (0, 0) at (`x`, `y`), its sizes along these axes.
    fn frame_of(&self, (x, y, w, h): BoxAt) -> OwnFrame {
        OwnFrame {
            centre: (x + (w * self.u.0 + h * self.v.0) / 2.0, y + (w * self.u.1 + h * self.v.1) / 2.0),
            width: w,
            height: h,
            angle: self.angle,
        }
    }

    /// The box `(x, y, width, height)` of a frame: its corner at (0, 0) from its centre.
    fn sized_of(&self, f: &OwnFrame) -> BoxAt {
        (
            f.centre.0 - (f.width * self.u.0 + f.height * self.v.0) / 2.0,
            f.centre.1 - (f.width * self.u.1 + f.height * self.v.1) / 2.0,
            f.width,
            f.height,
        )
    }

    /// The sides of the box `sized` that differ from the shape's now by more than a millionth of an inch, as the frame's own sides: `left` and `right` are the sides at the shape's
    /// own x of 0 and of its width, and `bottom` and `top` its own y of 0 and of its height, **seen from the library's frame** (a shape flipped top to bottom has them the other way).
    fn moved(&self, (x, y, w, h): BoxAt) -> Edges {
        let differs = |a: f64| a.abs() > 1.0e-6;
        let d = (x - self.c0.0, y - self.c0.1);
        let (a0, b0) = (d.0 * self.u.0 + d.1 * self.u.1, d.0 * self.v.0 + d.1 * self.v.1);
        let (x_lo, x_hi, y_lo, y_hi) = (differs(a0), differs(a0 + w - self.w), differs(b0), differs(b0 + h - self.h));
        let right_handed = self.u.0 * self.v.1 - self.u.1 * self.v.0 > 0.0;
        Edges { left: x_lo, right: x_hi, bottom: if right_handed { y_lo } else { y_hi }, top: if right_handed { y_hi } else { y_lo } }
    }
}

/// The own axes of the shape `id` when it is turned or flipped (and in no group that is) and its corners on the page make a rectangle with a size: where a resize of it is snapped
/// along its own axes. `None` for any other shape, which is snapped as a box square to the page or not at all (`is_square`).
fn own_axes(doc: &Document, page: u32, id: u32) -> Option<OwnAxes> {
    let reader = Reader::new(doc);
    let at = doc.find_shape(Owner::Page(page), id)?;
    let pose = reader.pose(&SheetRef::Shape(at.clone()));
    if !pose.is_turned_or_flipped() || reader.groups_turn(&at) {
        return None;
    }
    let k = units_per_inch(doc, page);
    OwnAxes::of(id, &reader.corners(&at).map(|p| (p.0 / k, p.1 / k)), pose.angle + if pose.flip_x { std::f64::consts::PI } else { 0.0 })
}

/// Whether a resize of the shape `id` is snapped along its own axes: it is turned or flipped, and its corners on the page make a rectangle (`shape.resize` and the window's
/// preview ask the same question).
pub fn snaps_in_its_own_axes(doc: &Document, page: u32, id: u32) -> bool {
    own_axes(doc, page, id).is_some()
}

/// The snap of a resize of shape `id` on `page` to the box `sized` (`x`, `y`, width, height, paper inches), by the edges that differ from the shape's box
/// now, at `zoom` screen points to a paper inch, by the document's settings. `kept` is the page as the snap reads it, if the caller keeps one (a window,
/// for the frames of a drag); without, the page is read when an edge moves. → the snap, and the box it comes to; `None` when no edge moves, or the snap
/// would collapse the box.
pub fn snap_resize(doc: &Document, page: u32, kept: Option<&SnapPage>, id: u32, sized: BoxAt, zoom: f64) -> Option<(Snapped, BoxAt)> {
    snap_resize_ext(doc, page, kept, id, sized, zoom, Extras::default())
}

/// [`snap_resize`] with the `Extras` of a command: the window's `view`, for the shapes a size match looks at.
pub fn snap_resize_ext(
    doc: &Document,
    page: u32,
    kept: Option<&SnapPage>,
    id: u32,
    sized: BoxAt,
    zoom: f64,
    extras: Extras,
) -> Option<(Snapped, BoxAt)> {
    if let Some(axes) = own_axes(doc, page, id) {
        return snap_turned_resize(doc, page, kept, &axes, sized, zoom, extras);
    }
    snap_edges_resize(doc, page, kept, &[id], sized, zoom, extras)
}

/// The snap of a resize of several shapes as one: the joint box of the shapes `ids` on `page` made the box `sized` (`x`, `y`, width, height, paper
/// inches), snapped by the edges that differ from the joint box now as the box of one shape is (the box of a selection is square to the page, whatever its shapes are), with
/// every shape of `ids` a shape that moves (never a target). `kept` and `extras` are [`snap_resize_ext`]'s; `keep` keeps the proportions of the joint box now. → the snap, and
/// the box it comes to; `None` when no edge moves, when the snap would collapse the box, or when a shape is not there.
pub fn snap_resize_set(
    doc: &Document,
    page: u32,
    kept: Option<&SnapPage>,
    ids: &[u32],
    sized: BoxAt,
    zoom: f64,
    extras: Extras,
) -> Option<(Snapped, BoxAt)> {
    snap_edges_resize(doc, page, kept, ids, sized, zoom, extras)
}

/// [`snap_resize_ext`] for a box square to the page: the edges of the box of `ids` that `sized` moves, against the page's targets.
fn snap_edges_resize(
    doc: &Document,
    page: u32,
    kept: Option<&SnapPage>,
    ids: &[u32],
    sized: BoxAt,
    zoom: f64,
    extras: Extras,
) -> Option<(Snapped, BoxAt)> {
    let (x, y, w, h) = sized;
    let now = box_of(doc, page, ids)?;
    let target = Rect { x0: x, y0: y, x1: x + w, y1: y + h };
    let edges = moved_edges(&now, &target);
    if !(edges.left || edges.bottom || edges.right || edges.top) {
        return None;
    }
    let read;
    let snap_page = match kept {
        Some(kept) => kept,
        None => {
            read = SnapPage::read(doc, page);
            read.as_ref()?
        }
    };
    let query = Query { subject: Subject::Edges(target, edges), using: Use::Place, moving: ids, zoom, own_points: &[] };
    // The proportions a resize keeps are the shape's own, as it is now.
    let ratio = (now.width() > 0.0 && now.height() > 0.0).then(|| now.width() / now.height());
    let extras = Extras { ratio: ratio.filter(|_| extras.keep), ..extras };
    let snapped = snap_ext(doc, snap_page, &SnapSettings::of(doc), &query, extras);
    let sized = resized_by(sized, &snapped)?;
    Some((snapped, sized))
}

/// [`snap_resize_ext`] for a turned or flipped shape: `sized` is the box `shape.resize` takes, its own (0, 0) corner and its sizes along its own axes, and the sides that move are
/// worked out in those axes ([`OwnAxes::moved`]). The library snaps the frame's sides along its own axes, a corner or a side's middle meeting a page line, and the box it comes to
/// is the answer's `frame`, which is the shape's own box again here: `(x, y, width, height)` of its (0, 0) corner. A snap with no hit leaves `sized` as it was.
fn snap_turned_resize(
    doc: &Document,
    page: u32,
    kept: Option<&SnapPage>,
    axes: &OwnAxes,
    sized: BoxAt,
    zoom: f64,
    extras: Extras,
) -> Option<(Snapped, BoxAt)> {
    let edges = axes.moved(sized);
    if !(edges.left || edges.bottom || edges.right || edges.top) {
        return None;
    }
    let read;
    let snap_page = match kept {
        Some(kept) => kept,
        None => {
            read = SnapPage::read(doc, page);
            read.as_ref()?
        }
    };
    let frame = axes.frame_of(sized);
    // With a frame the box of the query is not read: the frame is the box (its own axes), so the rect is the point it is round the middle of.
    let query =
        Query { subject: Subject::Edges(Rect::of(frame.centre, frame.centre), edges), using: Use::Place, moving: &[axes.id], zoom, own_points: &[] };
    // The proportions a resize keeps are the shape's own, its width over its height.
    let ratio = (axes.w > 0.0 && axes.h > 0.0).then(|| axes.w / axes.h);
    let extras = Extras { ratio: ratio.filter(|_| extras.keep), frame: Some(frame), ..extras };
    let snapped = snap_ext(doc, snap_page, &SnapSettings::of(doc), &query, extras);
    // A snap with no hit leaves `sized` as it was, except that keeping the proportions is the library's own answer, hit or not.
    let sized = match &snapped.frame {
        Some(f) if !snapped.hits.is_empty() || extras.keep => {
            let b = axes.sized_of(f);
            (b.2 > 0.0 && b.3 > 0.0 && [b.0, b.1, b.2, b.3].iter().all(|v| v.is_finite())).then_some(b)?
        }
        _ => sized,
    };
    Some((snapped, sized))
}

/// The box `(x, y, width, height)` a resize to `sized` comes to when `snapped` is applied to it, or `None` when that box would have no size (a snap that would collapse the box
/// is not taken). A resize that keeps the proportions comes to the box the snap gave (`landed`), which must have a size and be made of finite numbers; any other, to `sized` with
/// the pull of each hit added to the side it names. `snap_resize` and `snap.at` both take the box from here.
pub fn resized_by(sized: BoxAt, snapped: &Snapped) -> Option<BoxAt> {
    if let Some(l) = snapped.landed {
        let (w, h) = (l.width(), l.height());
        return (w > 0.0 && h > 0.0 && l.x0.is_finite() && l.y0.is_finite() && w.is_finite() && h.is_finite()).then_some((l.x0, l.y0, w, h));
    }
    let (mut x, mut y, mut w, mut h) = sized;
    for hit in &snapped.hits {
        match (hit.axis, hit.anchor) {
            (Some(Axis::X), Anchor::Low) => {
                x += snapped.dx;
                w -= snapped.dx;
            }
            (Some(Axis::X), Anchor::High) => w += snapped.dx,
            (Some(Axis::Y), Anchor::Low) => {
                y += snapped.dy;
                h -= snapped.dy;
            }
            (Some(Axis::Y), Anchor::High) => h += snapped.dy,
            _ => {}
        }
    }
    (w > 0.0 && h > 0.0).then_some((x, y, w, h))
}
