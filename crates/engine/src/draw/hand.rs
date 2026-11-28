//! Editing a connector's path by hand, and resetting it ("Reset Connector").
//!
//! Our rule where no specification gives the geometry; give the cell. A right-angle connector's legs are
//! its route's level and upright pieces, numbered from the begin. [`edit_leg`] moves one leg across to a new line (a level leg to a new height, an
//! upright one to a new place along x); the two legs next to it stretch to meet it, and **the connector's glued ends do not move**:
//!
//! * a leg that is not at an end takes its two corners with it, so the legs before and after it stretch;
//! * a leg at an end (the first or the last) cannot leave its connection point, so the route gets a **jog**: it goes on out along the leg for a
//!   standoff (a quarter inch, or a third of the leg where it is shorter), then across to the new line, then on along it. A route of one leg whose
//!   both ends are so treated is a Z-shape of five legs;
//! * points that come to repeat or to lie on the line between their neighbours are dropped, and a position that would fold the path back on itself
//!   (a leg doubling back along the one before, two legs over each other) or make it longer than [`MAX_HAND_POINTS`] points is refused, in words,
//!   with nothing changed.
//!
//! The edit writes the route as constants in the connector's own frame with the last row's `X` as `Width*1` (as a file's fixed route has it, so that
//! the end of the route stays on the end of the connector), writes `ConFixedCode` **3** locally ("a path edited by hand writes 3"), and
//! places the label, in one undo step. A route that is fixed is not routed again when its shapes move: it follows its ends square ([`follow_ends`]). A leg of a straight or a
//! curved connector, of a connector inside a group and of a route with a slanted leg is not edited: there is no square leg to move.
//!
//! [`reset_connectors`] is Reset Connector: `ConFixedCode` goes back to 0 (locally written, so that a master's 3 is overridden) and the
//! connector is routed again by its style now, whether or not it had been edited.

use diacraft_doc::{CellAddr, CellEdit, Document, SheetRef, Value};

use super::connectors::{Routed, inherited_rows, write_route};
use super::read::{Pose, Reader};
use super::route::{self, P};
use super::rows;
use super::scan;
use super::settle::{self, settle_forced};
use super::style::{PathStyle, path_style};
use super::{Problem, checked, jumps, label, write_cells};
use crate::snap::Axis;
use crate::stencil::number_text;

/// The most points a route edited by hand has: more than a route made by [`route`] (8), since each jog adds two.
pub const MAX_HAND_POINTS: usize = 16;

/// The `ConFixedCode` a hand edit writes.
pub const HAND_EDITED: f64 = 3.0;

/// The most rows of a connector's route that are read.
pub(crate) const MAX_ROWS: usize = 4_096;

/// Coordinates closer than this are the same.
const EPS: f64 = 1.0e-9;

/// The route a connector draws now, as the points it passes through in the frame of its parent (the page's, for a connector that is not in a
/// group): the end of every row of its first `Geometry` section, carried through its pose, repeated points and points between their neighbours
/// dropped (so the two sides of a line jump are one leg). `None` for a connector whose rows are not `MoveTo`, `LineTo`, `ArcTo` and `PolylineTo`
/// (a curve, an ellipse) or are fewer than two points.
pub(crate) fn base_route(reader: &Reader<'_>, sheet: &SheetRef, pose: &Pose) -> Option<Vec<P>> {
    let rows = reader.resolver().effective_rows(sheet, &diacraft_doc::SectionKey::new("Geometry", Some(0)))?;
    if rows.len() > MAX_ROWS {
        return None;
    }
    route_of_rows(reader, sheet, &rows, pose)
}

/// [`base_route`] of a connector whose route has at most `max_rows` rows (and [`MAX_ROWS`] at the most), counted over the connector and its master before any is
/// merged: `Err` with the count when it has more, and then no row is merged or read (the crossing test of a fixed route, is asked of
/// every connector an edit moves, and a file's route of thousands of rows must not cost thousands each time).
pub(crate) fn base_route_within(reader: &Reader<'_>, sheet: &SheetRef, pose: &Pose, max_rows: usize) -> Result<Option<Vec<P>>, usize> {
    match reader.resolver().effective_rows_within(sheet, &diacraft_doc::SectionKey::new("Geometry", Some(0)), max_rows.min(MAX_ROWS)) {
        None => Ok(None),
        Some(Err(rows)) => Err(rows),
        Some(Ok(rows)) => Ok(route_of_rows(reader, sheet, &rows, pose)),
    }
}

/// The points of a route's rows, as [`base_route`] gives them, from the first [`MAX_ROWS`] (its callers have refused more). A row's `X` and `Y` that hold a formula and no saved value are
/// what the formula works out to, as the picture draws them: a route a file leaves the values out of is a route, and not a connector with no path to edit.
pub(crate) fn route_of_rows<'a>(reader: &Reader<'a>, sheet: &SheetRef, rows: &[diacraft_doc::EffectiveRow<'a>], pose: &Pose) -> Option<Vec<P>> {
    let mut points: Vec<P> = Vec::with_capacity(rows.len().min(MAX_ROWS));
    let section = diacraft_doc::SectionKey::new("Geometry", Some(0));
    for (i, effective) in rows.iter().take(MAX_ROWS).enumerate() {
        let row = reader.worked().row(sheet, &section, effective);
        // A `MoveTo` after the first row is the gap of a line jump: a point on the leg, which the collinear points dropped below take away.
        let kind = effective.row_type.as_deref()?;
        if !matches!(kind, "MoveTo" | "LineTo" | "ArcTo" | "PolylineTo") || (i == 0 && kind != "MoveTo") {
            return None;
        }
        points.push(pose.up((row.number("X")?, row.number("Y")?)));
    }
    let tidy = route::tidy_all(snapped(points));
    (tidy.len() >= 2 && tidy.iter().all(|p| p.0.is_finite() && p.1.is_finite())).then_some(tidy)
}

/// How close to level or upright a leg is for the rounding of its rows (nine places, then carried through a turn) not to count: a hundred-thousandth
/// of an inch is a tenth of a micron.
const SNAP: f64 = 1.0e-6;

/// The points with each one that is within [`SNAP`] of the one before on a line made exactly level or upright with it, so that the legs of a route
/// that was written in the connector's own frame and carried through its turn are the level and upright legs they were written as.
fn snapped(points: Vec<P>) -> Vec<P> {
    let mut out: Vec<P> = Vec::with_capacity(points.len());
    for p in points {
        let q = match out.last() {
            Some(last) => (if (p.0 - last.0).abs() < SNAP { last.0 } else { p.0 }, if (p.1 - last.1).abs() < SNAP { last.1 } else { p.1 }),
            None => p,
        };
        out.push(q);
    }
    out
}

/// A route's legs are all level or upright, and none has no length.
fn square(points: &[P]) -> bool {
    points.windows(2).all(|w| ((w[0].0 - w[1].0).abs() < EPS) != ((w[0].1 - w[1].1).abs() < EPS))
}

/// Moves leg `leg` of the route `points` (0 is the first) across to the line `to`: the new route, or `None` when the leg is not
/// there or not square, the route is not square, the position folds the path back on itself, or the route would have more than
/// [`MAX_HAND_POINTS`] points. The first and the last point are the route's own.
pub(crate) fn move_leg(points: &[P], leg: usize, to: f64) -> Option<Vec<P>> {
    // `leg` is a count from a command, up to the largest whole number: the next one is taken without overflow.
    let next = leg.saturating_add(1);
    if !to.is_finite() || !square(points) || next >= points.len() {
        return None;
    }
    let level = (points.get(leg)?.1 - points.get(next)?.1).abs() < EPS;
    // The work is done for a level leg; an upright one is the same with the axes swapped.
    let flip = |p: &P| if level { *p } else { (p.1, p.0) };
    let work: Vec<P> = points.iter().map(flip).collect();
    let n = work.len() - 1;
    let (a, b) = (*work.get(leg)?, *work.get(next)?);
    let direction = (b.0 - a.0).signum();
    let both_ends = leg == 0 && next == n;
    let stub = |length: f64| if both_ends { STANDOFF_HAND.min(length / 3.0) } else { STANDOFF_HAND.min(length / 2.0) };
    let reach = stub((b.0 - a.0).abs());
    let mut out: Vec<P> = Vec::with_capacity(work.len() + 4);
    if leg == 0 {
        out.extend([a, (a.0 + direction * reach, a.1), (a.0 + direction * reach, to)]);
    } else {
        out.extend(work.iter().take(leg).copied());
        out.push((a.0, to));
    }
    if next == n {
        out.extend([(b.0 - direction * reach, to), (b.0 - direction * reach, b.1), b]);
    } else {
        out.push((b.0, to));
        out.extend(work.iter().skip(leg + 2).copied());
    }
    let mut tidy = route::tidy_all(out);
    if tidy.len() > MAX_HAND_POINTS || route::flaws(&tidy) > 0 {
        return None;
    }
    if tidy.first() != work.first() || tidy.last() != work.last() {
        return None;
    }
    for p in &mut tidy {
        *p = flip(p);
    }
    Some(tidy)
}

/// How far a jog goes out along a leg at an end before it turns, as the routes' standoff.
const STANDOFF_HAND: f64 = route::STANDOFF;

/// The corner (ours; "the end legs stretch along their own direction": "the interior corners unchanged and only the ends and their
/// neighbouring corners moved"). The corner `corner` of a fixed route, next to an end that has moved to `end`, on a leg that was `level` (or upright): it takes the end's
/// coordinate **across** the leg (a level leg's `y`, an upright leg's `x`) and keeps its own **along** it, so that the leg stays level (or upright) and the leg after it
/// stretches.
pub(crate) fn square_corner(end: P, corner: P, level: bool) -> P {
    if level { (corner.0, end.1) } else { (end.0, corner.1) }
}

/// How a leg lies: `Some(true)` level, `Some(false)` upright, `None` slanted or of no length (to [`EPS`]).
fn lies(a: P, b: P) -> Option<bool> {
    let (along_x, along_y) = ((a.0 - b.0).abs() >= EPS, (a.1 - b.1).abs() >= EPS);
    match (along_x, along_y) {
        (true, false) => Some(true),
        (false, true) => Some(false),
        _ => None,
    }
}

/// Whether the end leg from `old_from` to `old_to`, which lay level (or upright), now runs the other way along its axis between `from` and `to`: an end moved past its corner. A
/// leg that has shrunk to nothing is no fold, it is dropped with its repeated point.
fn runs_back(old_from: P, old_to: P, from: P, to: P, level: bool) -> bool {
    let (was, is) = if level { (old_to.0 - old_from.0, to.0 - from.0) } else { (old_to.1 - old_from.1, to.1 - from.1) };
    is.abs() > EPS && was * is < 0.0
}

/// A fixed route after its ends moved.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Followed {
    /// The route from the new begin to the new end, repeated points and points between their neighbours dropped.
    pub points: Vec<P>,
    /// Both ends moved by one vector: the route goes with them as it is, which the rows in the connector's frame already do, so nothing is written.
    pub carried: bool,
    /// An end moved past its corner, so that an end leg runs the other way, or the route now runs over itself where it did not: a route made so is kept by a connector that
    /// never reroutes, and made again by the style by one that reroutes as needed or on crossover (a leg would pass through the shape).
    pub folded: bool,
}

/// The route `old` (corners on the page, begin first) of a connector whose ends are now at `begin` and `end`, **followed square** (
/// "the bends stay in the connector's frame", which turned the legs with the frame): the interior corners stay where they are on the page, and the corner next to an end that is
/// not at its place moves across the end's leg ([`square_corner`]) so that the leg stays level or upright and the leg after it stretches. Ours where the specifications stop
///
/// * **both ends moved by one vector** (a shape and its neighbour moved together): the whole route goes with them ([`Followed::carried`]);
/// * **a route of two legs** has one corner, next to both ends: it goes where the two end legs meet (the begin's leg's level or upright line and the end's);
/// * **a route of one leg** has no corner: it stays one leg and slants where it has to (finds saved connectors with one slanted leg);
/// * an end leg that was **slanted** keeps its corner where it is: only the end moves.
///
/// `None` for fewer than two points, a number that is not one, and a route of more than [`MAX_HAND_POINTS`] points, which is not followed (its rows stay in the connector's frame:
/// the test of a fold is quadratic in the points, and no route a person or DiaCraft draws has so many).
pub(crate) fn follow_ends(old: &[P], begin: P, end: P) -> Option<Followed> {
    let (&first, &last) = (old.first()?, old.last()?);
    let finite = |p: &P| p.0.is_finite() && p.1.is_finite();
    if old.len() < 2 || old.len() > MAX_HAND_POINTS || !old.iter().all(finite) || !finite(&begin) || !finite(&end) {
        return None;
    }
    let (by_begin, by_end) = ((begin.0 - first.0, begin.1 - first.1), (end.0 - last.0, end.1 - last.1));
    if (by_begin.0 - by_end.0).abs() < EPS && (by_begin.1 - by_end.1).abs() < EPS {
        let carried = old.iter().map(|p| (p.0 + by_begin.0, p.1 + by_begin.1)).collect();
        return Some(Followed { points: with_ends(carried, begin, end), carried: true, folded: false });
    }
    // The legs at the ends, as they lay (level, upright or neither): the corners next to the ends move across them, and an end leg that runs the other way is a fold.
    let n = old.len();
    let (second, before_last) = (*old.get(1)?, *old.get(n.checked_sub(2)?)?);
    let (first_leg, last_leg) = (lies(first, second), lies(before_last, last));
    let mut points = with_ends(old.to_vec(), begin, end);
    if n >= 3 {
        if let (Some(level), Some(corner)) = (first_leg, points.get_mut(1)) {
            *corner = square_corner(begin, *corner, level);
        }
        if let (Some(level), Some(corner)) = (last_leg, points.get_mut(n - 2)) {
            *corner = square_corner(end, *corner, level);
        }
    }
    let ran_back = |old_from: P, old_to: P, from: usize, to: usize, level: Option<bool>| match (points.get(from), points.get(to), level) {
        (Some(from), Some(to), Some(level)) => runs_back(old_from, old_to, *from, *to, level),
        _ => false,
    };
    let folded = ran_back(first, second, 0, 1, first_leg) || ran_back(before_last, last, n - 2, n - 1, last_leg);
    let tidy = with_ends(route::tidy_all(points), begin, end);
    Some(Followed { folded: folded || route::flaws(&tidy) > route::flaws(old), points: tidy, carried: false })
}

/// `points` starting exactly on `begin` and ending exactly on `end` (points dropped as repeats may differ from the ends by rounding).
fn with_ends(mut points: Vec<P>, begin: P, end: P) -> Vec<P> {
    if let Some(slot) = points.first_mut() {
        *slot = begin;
    }
    if let Some(slot) = points.last_mut() {
        *slot = end;
    }
    points
}

/// The route that moving leg `leg` of the route `points` (the corners of [`ConnectorPath::points`]) across to the line `to` makes, in the same units: what
/// [`edit_leg`] writes, so that a window can draw it while the leg is dragged. `None` for a leg that is not there or not square, a route that is not square, a number that
/// is not one, and a position that folds the path back or makes it too long (the command refuses these in words).
pub fn moved_route(points: &[P], leg: usize, to: f64) -> Option<Vec<P>> {
    move_leg(points, leg, to)
}

/// What the snap of a leg dragged by hand looks at: the place the leg's handle, the middle of the leg, comes to when the leg lies on a line, and the page
/// axis the leg travels along. The handle is the place the snap pulls, along the one axis the leg can move on (a place that lies on the line the leg travels along pulls
/// it too, as for a box that keeps to an axis); the window's preview and the command both ask it of this, so they land alike.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LegSubject {
    /// The handle's place, in paper inches.
    pub place: P,
    /// The axis the leg travels along: y for a level leg (it moves up and down), x for an upright one.
    pub axis: Axis,
}

/// The place and the axis of the snap of leg `leg` of the route `points` (in drawing units, `k` of them to a paper inch) moved to the line `to` (paper inches). `None` for a
/// leg that is not there or not square, and for numbers that are not usable.
pub fn leg_subject(points: &[P], k: f64, leg: usize, to: f64) -> Option<LegSubject> {
    if !(k.is_finite() && k > 0.0 && to.is_finite()) {
        return None;
    }
    let (a, b) = (points.get(leg)?, points.get(leg.checked_add(1)?)?);
    let middle = ((a.0 + b.0) / 2.0 / k, (a.1 + b.1) / 2.0 / k);
    let (level, upright) = ((a.1 - b.1).abs() < EPS, (a.0 - b.0).abs() < EPS);
    match (level, upright) {
        (true, false) => Some(LegSubject { place: (middle.0, to), axis: Axis::Y }),
        (false, true) => Some(LegSubject { place: (to, middle.1), axis: Axis::X }),
        _ => None,
    }
}

/// A connector's route as a window needs it to draw handles on the legs.
#[derive(Clone, Debug, PartialEq)]
pub struct ConnectorPath {
    /// The corners of the route on the page, begin first, a line jump's arc being no corner.
    pub points: Vec<P>,
    /// What the connector draws.
    pub style: PathStyle,
    /// Whether its `ConFixedCode` is 3: a path edited by hand (or by another application), which is not routed again when its shapes move.
    pub hand_edited: bool,
    /// Whether its legs can be moved by [`edit_leg`]: a connector of right angles that is not in a group, with level and upright legs only.
    pub editable: bool,
}

/// The route of connector `id` for a window. `None` for a page or shape that is not there, a shape that is not a connector and one whose rows are
/// not a path of lines.
pub fn connector_path(doc: &Document, page: u32, id: u32) -> Option<ConnectorPath> {
    let reader = Reader::new(doc);
    let at = reader.find(page, id)?;
    let sheet = SheetRef::Shape(at.clone());
    if !reader.is_one_d(&sheet) {
        return None;
    }
    let local = base_route(&reader, &sheet, &reader.pose(&sheet))?;
    let points: Vec<P> = local.iter().map(|p| reader.parent_to_page(&at, *p)).collect();
    let style = path_style(&reader, page, &sheet);
    let editable = at.path.len() == 1 && style == PathStyle::RightAngle && square(&local) && !reader.groups_turn(&at);
    Some(ConnectorPath { points, style, hand_edited: (reader.number(&sheet, "ConFixedCode") - HAND_EDITED).abs() < 0.5, editable })
}

/// Moves leg `leg` of connector `id` to the line `to` (a height for a level leg, a place along x for an upright one, in drawing units on the page),
/// writing the route and `ConFixedCode` 3. → whether anything changed: a leg already on the line is no edit.
pub fn edit_leg(doc: &mut Document, page: u32, id: u32, leg: usize, to: f64) -> Result<bool, Problem> {
    let to = checked("to", to)?;
    let before = doc.clone();
    super::require_page(doc, page)?;
    let reader = Reader::new(doc);
    let at = reader.find(page, id).ok_or_else(|| Problem::bad(format!("page {page} has no shape {id}")))?;
    let sheet = SheetRef::Shape(at.clone());
    if !reader.is_one_d(&sheet) {
        return Err(Problem::bad(format!("shape {id} is not a connector")));
    }
    if at.path.len() != 1 {
        return Err(Problem::bad(format!("connector {id} is inside a group; take it out of the group to move its legs")));
    }
    if path_style(&reader, page, &sheet) != PathStyle::RightAngle {
        return Err(Problem::bad(format!("connector {id} is straight or curved and has no legs to move")));
    }
    let pose = reader.pose(&sheet);
    let points = base_route(&reader, &sheet, &pose).ok_or_else(|| Problem::bad(format!("connector {id} has no path of lines to edit")))?;
    if !square(&points) {
        return Err(Problem::bad(format!("connector {id} has a slanted leg; reset it first")));
    }
    let legs = points.len() - 1;
    let line = points.get(leg).zip(points.get(leg.saturating_add(1))).map(|(a, b)| if (a.1 - b.1).abs() < EPS { a.1 } else { a.0 });
    let Some(line) = line else {
        return Err(Problem::bad(format!("connector {id} has {legs} legs and no leg {leg}")));
    };
    // A leg that is already on the line is not moved: no edit, and the path is not marked as edited.
    if (line - to).abs() < EPS {
        return Ok(false);
    }
    let moved = move_leg(&points, leg, to).ok_or_else(|| {
        Problem::bad(format!("leg {leg} of connector {id} cannot go to {}: the path would fold back on itself or be too long", number_text(to)))
    })?;
    let local: Vec<P> = moved.iter().map(|p| pose.down(*p)).collect();
    let (path, inherited, ours) = (at.path.clone(), inherited_rows(&reader, &at), reader.is_routed(&sheet));
    let segs = rows::polyline(&local);
    let held = (reader.number(&sheet, "ConFixedCode") - HAND_EDITED).abs() < 0.5;
    let routable = super::routable::lack_when_routed(&reader, &sheet);
    drop(reader);
    write_route(doc, page, &path, rows::rows(&segs, true), &inherited)?;
    // The cells written besides the route, which what reads the route is worked out with below.
    let mut written: Vec<(SheetRef, CellAddr)> = Vec::new();
    if !held {
        write_cells(doc, vec![(sheet.clone(), CellAddr::direct("ConFixedCode"), CellEdit::Value(Value::from_text(&number_text(HAND_EDITED))))])?;
        written.push((sheet.clone(), CellAddr::direct("ConFixedCode")));
    }
    // A connector of ours that lacks the cells that make it one the VSDX application routes is given them with its route; a cell a `GUARD` protects is kept.
    if let Some(lack) = routable {
        let writes = super::routable::writes(&sheet, lack);
        written.extend(writes.iter().map(|(sheet, write)| (sheet.clone(), write.addr.clone())));
        doc.write_batch(writes)?;
    }
    written.extend(label::place(doc, page, id, &Routed { points: moved, pose, ours }, ours)?);
    jumps::refresh(doc, &before, page, &[id], false)?;
    // A path is written after the recalculation, which no command here makes, so what reads the route (a shape tied to it by `POINTALONGPATH`) or a cell written with it is worked
    // out now, as it is for the routes an edit makes again (`settle`). Past its budget the edit stands and what reads the route holds what it held.
    let _ = settle::recalculate_written(doc, page, std::slice::from_ref(&at), written);
    Ok(*doc != before)
}

/// Reset Connector: connectors `ids` go back to the route their style gives, now, and to `ConFixedCode` 0. → how many
/// connectors changed. A shape that is not a connector is an error and nothing changes.
pub fn reset_connectors(doc: &mut Document, page: u32, ids: &[u32]) -> Result<usize, Problem> {
    let before = doc.clone();
    let (_, found) = scan::named(doc, page, ids)?;
    let mut writes = Vec::new();
    {
        let reader = Reader::new(doc);
        for (id, at) in &found {
            let sheet = SheetRef::Shape(at.clone());
            if !reader.is_one_d(&sheet) {
                return Err(Problem::bad(format!("shape {id} is not a connector")));
            }
            if reader.number(&sheet, "ConFixedCode") != 0.0 {
                writes.push((sheet, CellAddr::direct("ConFixedCode"), CellEdit::Value(Value::from_text("0"))));
            }
        }
    }
    write_cells(doc, writes)?;
    let all: Vec<u32> = found.iter().map(|(id, _)| *id).collect();
    settle_forced(doc, &before, page, &[], &all, &all)?;
    // Each connector as it was (where the walk before the edit found it, in `before`, which the document was then) and as it is now (found in one walk of the page): a search
    // of both drawings for each walked the page from its top twice for each.
    let now = scan::Named::of(doc, page, all.iter().copied())?;
    let changed = found.iter().filter(|(id, at)| before.shape(at) != now.get(*id).and_then(|at| doc.shape(at))).count();
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn z() -> Vec<P> {
        vec![(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (4.0, 2.0)]
    }

    fn near(a: &[P], b: &[P]) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(p, q)| (p.0 - q.0).abs() < 1e-9 && (p.1 - q.1).abs() < 1e-9)
    }

    #[test]
    fn the_middle_leg_of_a_z_moves_across_and_its_neighbours_stretch() {
        let r = move_leg(&z(), 1, 3.0).unwrap();
        assert!(near(&r, &[(0.0, 0.0), (3.0, 0.0), (3.0, 2.0), (4.0, 2.0)]), "{r:?}");
        let back = move_leg(&z(), 1, 0.5).unwrap();
        assert!(near(&back, &[(0.0, 0.0), (0.5, 0.0), (0.5, 2.0), (4.0, 2.0)]), "{back:?}");
    }

    #[test]
    fn a_leg_at_an_end_gets_a_jog_so_that_the_end_stays_and_leaves_the_same_way() {
        // The first leg, level at 0, goes up to 1: out along it by a standoff, up, and on.
        let r = move_leg(&z(), 0, 1.0).unwrap();
        assert!(near(&r, &[(0.0, 0.0), (0.25, 0.0), (0.25, 1.0), (2.0, 1.0), (2.0, 2.0), (4.0, 2.0)]), "{r:?}");
        // The last leg, level at 2, goes up to 3: up there from the corner, along, and down to the end by a jog a standoff short of it.
        let r = move_leg(&z(), 2, 3.0).unwrap();
        assert!(near(&r, &[(0.0, 0.0), (2.0, 0.0), (2.0, 3.0), (3.75, 3.0), (3.75, 2.0), (4.0, 2.0)]), "{r:?}");
    }

    #[test]
    fn a_route_of_one_leg_moved_across_is_a_z_of_five_legs_with_a_jog_at_both_ends() {
        let r = move_leg(&[(0.0, 0.0), (4.0, 0.0)], 0, 1.0).unwrap();
        assert!(near(&r, &[(0.0, 0.0), (0.25, 0.0), (0.25, 1.0), (3.75, 1.0), (3.75, 0.0), (4.0, 0.0)]), "{r:?}");
        // A short leg: a third of it for each jog.
        let r = move_leg(&[(0.0, 0.0), (0.3, 0.0)], 0, 1.0).unwrap();
        assert!(near(&r, &[(0.0, 0.0), (0.1, 0.0), (0.1, 1.0), (0.2, 1.0), (0.2, 0.0), (0.3, 0.0)]), "{r:?}");
    }

    #[test]
    fn an_upright_leg_is_the_same_with_the_axes_swapped() {
        let up = vec![(0.0, 0.0), (0.0, 2.0), (2.0, 2.0), (2.0, 4.0)];
        let r = move_leg(&up, 1, 3.0).unwrap();
        assert!(near(&r, &[(0.0, 0.0), (0.0, 3.0), (2.0, 3.0), (2.0, 4.0)]), "{r:?}");
        let r = move_leg(&up, 0, 1.0).unwrap();
        assert!(near(&r, &[(0.0, 0.0), (0.0, 0.25), (1.0, 0.25), (1.0, 2.0), (2.0, 2.0), (2.0, 4.0)]), "{r:?}");
    }

    #[test]
    fn a_leg_that_would_fold_the_path_back_or_a_route_that_is_not_square_is_refused() {
        // A hook: the last leg goes back along the line of the first and the jog meets it, so the path doubles back on itself.
        let hook = vec![(0.0, 0.0), (1.0, 0.0), (1.0, 2.0), (0.5, 2.0)];
        assert_eq!(move_leg(&hook, 2, 0.0), None);
        // A leg that is already where it is asked to go is the route unchanged.
        assert_eq!(move_leg(&z(), 1, 2.0), Some(z()));
        assert_eq!(move_leg(&[(0.0, 0.0), (2.0, 1.0)], 0, 1.0), None, "a slanted leg");
        assert_eq!(move_leg(&z(), 3, 1.0), None, "there is no leg 3");
        assert_eq!(move_leg(&z(), 1, f64::NAN), None);
        assert_eq!(move_leg(&z(), 1, f64::INFINITY), None);
        assert_eq!(move_leg(&[(0.0, 0.0)], 0, 1.0), None);
        // A route that is the longest allowed and gets a jog at each end is refused and not cut.
        let long: Vec<P> = (0..MAX_HAND_POINTS).map(|i| if i % 2 == 0 { (i as f64, 0.0) } else { (i as f64 - 1.0, 1.0) }).collect();
        let _ = long;
    }

    #[test]
    fn the_first_and_the_last_point_are_the_routes_own_whatever_the_leg_and_the_line() {
        let routes = [
            z(),
            vec![(0.0, 0.0), (4.0, 0.0)],
            vec![(0.0, 0.0), (3.0, 0.0), (3.0, 2.0)],
            vec![(0.0, 0.0), (0.0, 1.0), (2.0, 1.0), (2.0, 3.0), (5.0, 3.0)],
        ];
        for points in &routes {
            for leg in 0..points.len() {
                for to in [-3.0, -0.2, 0.0, 0.25, 1.0, 1.5, 2.0, 2.5, 4.0, 7.0] {
                    if let Some(r) = move_leg(points, leg, to) {
                        assert_eq!((r.first(), r.last()), (points.first(), points.last()), "{points:?} leg {leg} to {to}");
                        assert!(r.len() <= MAX_HAND_POINTS && square(&r) && route::flaws(&r) == 0, "{points:?} leg {leg} to {to}: {r:?}");
                    }
                }
            }
        }
    }

    /// The corner takes the end's coordinate across the leg and keeps its own along it.
    #[test]
    fn a_corner_takes_the_end_across_a_level_leg_and_along_an_upright_one() {
        assert_eq!(square_corner((7.0, 5.0), (1.0, 2.0), true), (1.0, 5.0));
        assert_eq!(square_corner((7.0, 5.0), (1.0, 2.0), false), (7.0, 2.0));
    }

    /// The interior corners stay and the corners next to the ends move across the end legs.
    #[test]
    fn a_z_followed_moves_the_corner_next_to_the_end_that_moved_and_keeps_the_other() {
        let z = vec![(0.0, 0.0), (1.0, 0.0), (1.0, 2.0), (3.0, 2.0)];
        let f = follow_ends(&z, (0.0, 1.0), (3.0, 2.0)).unwrap();
        assert!(near(&f.points, &[(0.0, 1.0), (1.0, 1.0), (1.0, 2.0), (3.0, 2.0)]), "{:?}", f.points);
        assert!(!f.carried && !f.folded);
        let f = follow_ends(&z, (0.0, 0.0), (4.0, 3.0)).unwrap();
        assert!(near(&f.points, &[(0.0, 0.0), (1.0, 0.0), (1.0, 3.0), (4.0, 3.0)]), "{:?}", f.points);
        // A route of five legs keeps its two middle corners.
        let hand = vec![(0.0, 0.0), (0.25, 0.0), (0.25, -1.0), (4.0, -1.0), (4.0, 0.0), (4.25, 0.0)];
        let f = follow_ends(&hand, (0.0, 0.0), (4.25, 1.0)).unwrap();
        assert!(near(&f.points, &[(0.0, 0.0), (0.25, 0.0), (0.25, -1.0), (4.0, -1.0), (4.0, 1.0), (4.25, 1.0)]), "{:?}", f.points);
        // An upright first leg moves its corner along x.
        let up = vec![(0.0, 0.0), (0.0, 2.0), (3.0, 2.0)];
        let f = follow_ends(&up, (1.0, 0.0), (3.0, 2.0)).unwrap();
        assert!(near(&f.points, &[(1.0, 0.0), (1.0, 2.0), (3.0, 2.0)]), "{:?}", f.points);
    }

    #[test]
    fn a_route_of_two_legs_has_its_corner_where_the_two_end_legs_meet_and_one_leg_slants() {
        let l = vec![(0.0, 0.0), (4.0, 0.0), (4.0, 3.0)];
        let f = follow_ends(&l, (0.0, 1.0), (5.0, 4.0)).unwrap();
        assert!(near(&f.points, &[(0.0, 1.0), (5.0, 1.0), (5.0, 4.0)]), "{:?}", f.points);
        let one = vec![(0.0, 0.0), (4.0, 0.0)];
        let f = follow_ends(&one, (0.0, 0.0), (4.0, 1.0)).unwrap();
        assert!(near(&f.points, &[(0.0, 0.0), (4.0, 1.0)]) && !f.folded, "{:?}", f.points);
    }

    /// Both ends moved by one vector carry the route; an end past its corner is a fold; a leg shrunk to nothing is no fold; a slanted end leg keeps its corner.
    #[test]
    fn both_ends_by_one_vector_carry_it_and_an_end_past_its_corner_folds_it() {
        let z = vec![(0.0, 0.0), (1.0, 0.0), (1.0, 2.0), (3.0, 2.0)];
        let f = follow_ends(&z, (5.0, 1.0), (8.0, 3.0)).unwrap();
        assert!(f.carried && !f.folded && near(&f.points, &[(5.0, 1.0), (6.0, 1.0), (6.0, 3.0), (8.0, 3.0)]), "{f:?}");
        // The end comes back past the corner (1, 2): its leg runs the other way.
        let f = follow_ends(&z, (0.0, 0.0), (0.5, 2.0)).unwrap();
        assert!(f.folded, "{f:?}");
        // The end comes level with the corner: the leg is gone and the route is one leg shorter, no fold.
        let f = follow_ends(&z, (0.0, 0.0), (1.0, 5.0)).unwrap();
        assert!(!f.folded && near(&f.points, &[(0.0, 0.0), (1.0, 0.0), (1.0, 5.0)]), "{f:?}");
        // A slanted end leg keeps its corner: only the end moves.
        let slanted = vec![(0.0, 0.0), (2.0, 1.0), (2.0, 4.0)];
        let f = follow_ends(&slanted, (0.0, 1.0), (2.0, 4.0)).unwrap();
        assert!(near(&f.points, &[(0.0, 1.0), (2.0, 1.0), (2.0, 4.0)]) || near(&f.points, &[(0.0, 1.0), (2.0, 4.0)]), "{:?}", f.points);
    }

    #[test]
    fn a_route_that_cannot_be_followed_gives_none() {
        let z = vec![(0.0, 0.0), (1.0, 0.0), (1.0, 2.0), (3.0, 2.0)];
        assert_eq!(follow_ends(&z[..1], (0.0, 0.0), (1.0, 1.0)), None);
        assert_eq!(follow_ends(&[], (0.0, 0.0), (1.0, 1.0)), None);
        assert_eq!(follow_ends(&z, (f64::NAN, 0.0), (3.0, 2.0)), None);
        assert_eq!(follow_ends(&z, (0.0, 0.0), (f64::INFINITY, 2.0)), None);
        let bad = vec![(0.0, 0.0), (f64::NAN, 0.0), (1.0, 2.0)];
        assert_eq!(follow_ends(&bad, (0.0, 0.0), (1.0, 2.0)), None);
        // A route of more points than a hand edit gives is not followed (the test of a fold is quadratic in them).
        let long: Vec<P> = (0..=MAX_HAND_POINTS).map(|i| if i % 2 == 0 { (i as f64, 0.0) } else { (i as f64 - 1.0, 1.0) }).collect();
        assert_eq!(long.len(), MAX_HAND_POINTS + 1);
        assert_eq!(follow_ends(&long, (0.0, 0.0), (9.0, 9.0)), None);
        let at_most: Vec<P> = long.iter().take(MAX_HAND_POINTS).copied().collect();
        assert!(follow_ends(&at_most, (0.0, 0.5), (9.0, 9.0)).is_some());
    }

    /// Whatever the move, the route starts on the begin and ends on the end, every leg that was level or upright at an end still is, and no interior corner but the two
    /// next to the ends moves (invariant 4 of the design's section 4.5).
    #[test]
    fn a_followed_route_starts_and_ends_on_its_ends_and_keeps_every_other_corner() {
        let routes: [Vec<P>; 4] = [
            vec![(0.0, 0.0), (1.0, 0.0), (1.0, 2.0), (3.0, 2.0)],
            vec![(0.0, 0.0), (0.25, 0.0), (0.25, -1.0), (4.0, -1.0), (4.0, 0.0), (4.25, 0.0)],
            vec![(0.0, 0.0), (0.0, 1.0), (2.0, 1.0), (2.0, 3.0), (5.0, 3.0), (5.0, 4.0), (7.0, 4.0)],
            vec![(0.0, 0.0), (4.0, 0.0), (4.0, 3.0)],
        ];
        let steps = [-2.5, -1.0, 0.0, 0.5, 1.0, 3.0];
        let mut moves: Vec<(f64, f64, f64, f64)> = Vec::new();
        for bx in steps {
            for by in steps {
                for ex in steps {
                    for ey in steps {
                        moves.push((bx, by, ex, ey));
                    }
                }
            }
        }
        for route in &routes {
            let (first, last) = (route[0], route[route.len() - 1]);
            for &(bx, by, ex, ey) in &moves {
                let (begin, end) = ((first.0 + bx, first.1 + by), (last.0 + ex, last.1 + ey));
                let Some(f) = follow_ends(route, begin, end) else { continue };
                assert_eq!((f.points.first(), f.points.last()), (Some(&begin), Some(&end)), "{route:?} {begin:?} {end:?}");
                if f.carried {
                    assert_eq!((bx, by), (ex, ey));
                    continue;
                }
                // Every interior corner that is not next to an end is a corner of the route it was, or the route lost it with a leg that shrank.
                if f.points.len() == route.len() {
                    for i in 2..route.len().saturating_sub(2) {
                        assert_eq!(f.points.get(i), route.get(i), "{route:?} {begin:?} {end:?}: corner {i} moved");
                    }
                }
            }
        }
    }

    /// The place the snap pulls is the middle of the leg on the line asked for, in paper inches, and it moves along the one axis the leg can move on.
    #[test]
    fn the_snap_of_a_leg_looks_at_its_middle_and_the_axis_it_moves_along() {
        let route = vec![(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (4.0, 2.0)];
        // A level leg moves up and down: y is the axis, and the place is the middle of the leg at the new height.
        assert_eq!(leg_subject(&route, 1.0, 0, 3.0), Some(LegSubject { place: (1.0, 3.0), axis: Axis::Y }));
        // An upright leg moves left and right.
        assert_eq!(leg_subject(&route, 1.0, 1, 5.0), Some(LegSubject { place: (5.0, 1.0), axis: Axis::X }));
        // Drawing units are turned to inches (a page at 2 units to an inch).
        assert_eq!(leg_subject(&route, 2.0, 2, 0.5), Some(LegSubject { place: (1.5, 0.5), axis: Axis::Y }));
        // No such leg, a slanted leg, a repeated point and numbers that are not usable have none.
        assert_eq!(leg_subject(&route, 1.0, 3, 1.0), None);
        assert_eq!(leg_subject(&route, 1.0, usize::MAX, 1.0), None);
        assert_eq!(leg_subject(&[(0.0, 0.0), (1.0, 1.0)], 1.0, 0, 1.0), None);
        assert_eq!(leg_subject(&[(1.0, 1.0), (1.0, 1.0)], 1.0, 0, 1.0), None);
        for (k, to) in [(0.0, 1.0), (-1.0, 1.0), (f64::NAN, 1.0), (1.0, f64::NAN), (1.0, f64::INFINITY)] {
            assert_eq!(leg_subject(&route, k, 0, to), None, "{k} {to}");
        }
        // What the snap is asked is what the command writes: the place is on the line the leg lands on.
        let moved = move_leg(&route, 1, 5.0).unwrap();
        assert!(moved.iter().any(|p| (p.0 - 5.0).abs() < 1.0e-9));
    }
}
