//! An absolute place for many shapes: the Size & Position window's X and Y.
//!
//! (ours; X and Y are where the pin is on the page; the
//! rules of [`shape.move`](super::move_shapes), MOVE-1D). [`place_shapes`] puts the pin of each shape named at `x` and/or `y` **on
//! the page**: the place `shape.placement` shows and `shape.pin` chose. It is a move of each shape by the distance from where its pin is to the place, so
//! it does what `shape.move` does for each shape, and the shapes need not share a distance:
//!
//! * a shape moves by its pin, and only the coordinate asked for is written (`x` alone writes `PinX` and leaves `PinY` and its formula alone, unless the
//!   shape sits in a turned group, where one axis of the page is both of the group's); **a shape at the top of the page gets the number asked for in the
//!   cell itself**, not the sum of its old pin and a distance, so a pin of 1.0 placed at 0.3 is 0.3 and not 0.30000000000000004;
//! * a line or a connector moves the ends that are not glued (a glued end stays where its target holds it, MOVE-1D), by the same distance, so its pin
//!   follows by its formula: **a line with one end glued moves its free end by the distance the pin was to go, so its pin goes half as far** (a line from
//!   0 to 2 with its begin glued, placed at x = 10, runs from 0 to 11 and its pin is at 5.5); a connector with both ends glued has nothing of its own to move;
//! * a member of a group has its pin in the group's frame: the place is a point of the page, carried down through the groups the shape is in, the
//!   outermost first, so the pin the window shows is the place asked for;
//! * **a member named with its group is placed too, and ends at the place as the group does**: the group goes first (document order), and the member's
//!   distance is what is left once the group has taken it along (`Ancestry` carries the page shift of each group named down to the shapes in it), so every
//!   pin named is at the place and no member is moved twice;
//! * connectors glued to a shape that moved follow it and are routed again, one recalculation and one routing for the call (`settle`);
//! * a shape whose pin is at the place already is skipped and not written, and a shape with nothing to move is skipped: the answer counts the
//!   shapes that moved, and when none did the document is not touched;
//! * **a shape that has no place to move from or to** is skipped and not written, and the answer counts it as `unplaced`; the other shapes are placed,
//!   so one such shape does not refuse the call. That is a shape whose pin, carried through its groups, is not a finite number (the numbers of its
//!   groups overflow: a cell holds only finite numbers, but a sum of them need not be), and one that a write would put past the reach of the page
//!   (`MAX_COORD`, a million drawing units: a line with an end that far out, or a member of a group that far away);
//! * a shape named twice is placed once; one that is not on the page is refused, and nothing changes.
//!
//! **Cost.** The shapes are found with **one walk** of the page, whatever their number. The frame a shape's groups give it is composed from the poses of
//! the groups above it, **each group's pose read once** for all the shapes in it (the shapes are in document order, so the groups above one are the
//! groups above the last, less those it left, plus those it entered), not once for each shape and each group above it. The ends glued are found with
//! one pass over the page's `Connect` rows. Only the shapes that a connector can follow, or that are routed connectors themselves, or hold such a
//! shape, are handed to the routing, which looks each up from the top of the page; **which groups hold such a shape is found by one more walk of the
//! page** that passes the answer up from the members to their groups ([`groups_holding`]; none when no connector is on the page or no group is named),
//! and of a chain of touched groups only the outermost is handed on, since its subtree is theirs.

use std::collections::{HashMap, HashSet};

use diacraft_doc::{Document, MAX_SHAPE_DEPTH, MAX_SHAPES_PER_PAGE, ShapeList, ShapeRef, SheetRef};

use super::read::{Pose, Reader};
use super::route::P;
use super::scan;
use super::settle::settle;
use super::{MAX_COORD, Problem, checked, write_length};

/// How far a pin may be from the place asked for and count as there (the arithmetic of a turned group is not exact).
const SAME: f64 = 1.0e-9;

// How many `Connect` rows were read to find the ends glued, for the test that holds a call to one pass over them.
#[cfg(test)]
thread_local! {
    pub(crate) static CONNECT_ROWS_READ: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// A move of the plane that keeps lengths: a turn or a reflection, then a shift. The frame of a shape's parent in the page's is one, being the poses
/// of the groups above it composed.
#[derive(Clone, Copy, Debug)]
struct Iso {
    /// `x' = a x + c y + t.0`, `y' = b x + d y + t.1`.
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    t: P,
}

impl Iso {
    const IDENTITY: Iso = Iso { a: 1.0, b: 0.0, c: 0.0, d: 1.0, t: (0.0, 0.0) };

    /// What a group's pose does to a point of its frame ([`Pose::up`]).
    fn of_pose(pose: &Pose) -> Iso {
        let (a, b) = pose.up_vector((1.0, 0.0));
        let (c, d) = pose.up_vector((0.0, 1.0));
        Iso { a, b, c, d, t: pose.up((0.0, 0.0)) }
    }

    fn point(&self, p: P) -> P {
        (self.a * p.0 + self.c * p.1 + self.t.0, self.b * p.0 + self.d * p.1 + self.t.1)
    }

    /// `self` applied after `inner`.
    fn after(&self, inner: &Iso) -> Iso {
        Iso {
            a: self.a * inner.a + self.c * inner.b,
            b: self.b * inner.a + self.d * inner.b,
            c: self.a * inner.c + self.c * inner.d,
            d: self.b * inner.c + self.d * inner.d,
            t: self.point(inner.t),
        }
    }

    /// A distance of the page as the frame has it: the turn undone. The matrix is a turn or a reflection, so its inverse is its transpose.
    fn vector_in(&self, v: P) -> P {
        (self.a * v.0 + self.b * v.1, self.c * v.0 + self.d * v.1)
    }
}

/// Nothing taken along.
const NO_SHIFT: P = (0.0, 0.0);

/// The frames of the groups above shapes taken in document order, composed once for each group (the module documentation's cost), and how far the groups
/// named have taken what is in them along the page.
struct Ancestry {
    /// For each group above the last shape: its place in the list that holds it, the frame of its members in the page's, and how far the groups named
    /// among it and the groups above it take their members along the page.
    stack: Vec<(u32, Iso, P)>,
    /// How far each group that was named has gone on the page, by its path. Only the groups that moved are here.
    shifts: HashMap<Vec<u32>, P>,
}

impl Ancestry {
    fn new() -> Ancestry {
        Ancestry { stack: Vec::new(), shifts: HashMap::new() }
    }

    /// The frame of the parent of the shape at `at` in the page's, and how far the groups above it have taken it along the page: the identity and no shift
    /// for a shape at the top of the page. `at` follows, in document order, the shapes this was asked about before, and so follows its groups, which
    /// were placed (and [`Ancestry::went`]) before it.
    fn parent_frame(&mut self, reader: &Reader<'_>, at: &ShapeRef) -> (Iso, P) {
        let groups = at.path.len().saturating_sub(1);
        // The groups above this shape that were above the last one too stay; the others were left, and are never entered again.
        let kept = self.stack.iter().zip(&at.path).take(groups).take_while(|(entry, here)| entry.0 == **here).count();
        self.stack.truncate(kept);
        for depth in kept..groups {
            let Some(ix) = at.path.get(depth) else { break };
            let (above, carried) = self.stack.last().map_or((Iso::IDENTITY, NO_SHIFT), |(_, iso, shift)| (*iso, *shift));
            let path = at.path.get(..=depth).map(<[u32]>::to_vec).unwrap_or_default();
            let own = self.shifts.get(&path).copied().unwrap_or(NO_SHIFT);
            let frame = above.after(&Iso::of_pose(&reader.pose(&SheetRef::Shape(ShapeRef { owner: at.owner, path }))));
            self.stack.push((*ix, frame, (carried.0 + own.0, carried.1 + own.1)));
        }
        self.stack.last().map_or((Iso::IDENTITY, NO_SHIFT), |(_, iso, shift)| (*iso, *shift))
    }

    /// The group at `at` goes `shift` along the page: what is in it goes with it.
    fn went(&mut self, at: &ShapeRef, shift: P) {
        if shift != NO_SHIFT {
            self.shifts.insert(at.path.clone(), shift);
        }
    }
}

/// Which ends of which lines a `Connect` row of the page glues: by the line's `ID`, 0 for the begin and 1 for the end, as `plan_ends` of `shape.move`
/// tells them (the row names the end's x cell). And the `ID`s that any row names, as a line or as the shape it is glued to. One pass over the rows.
fn glue_of(doc: &Document, page: u32) -> (HashSet<(u32, u8)>, HashSet<u32>) {
    let (mut ends, mut named) = (HashSet::new(), HashSet::new());
    let Some(p) = doc.page(page) else { return (ends, named) };
    for c in p.connects.iter() {
        #[cfg(test)]
        CONNECT_ROWS_READ.with(|n| n.set(n.get().saturating_add(1)));
        named.insert(c.from_sheet);
        named.insert(c.to_sheet);
        match c.from_cell.as_deref() {
            Some("BeginX") => ends.insert((c.from_sheet, 0)),
            Some("EndX") => ends.insert((c.from_sheet, 1)),
            _ => false,
        };
    }
    (ends, named)
}

/// The `ID`s of the groups among `groups` (shapes named, with members) that hold a shape that a `Connect` row names (`connected`) among their members, at
/// any depth: **one walk of the page** that passes "a shape below is connected" up from each list of members to its group when the list is done, and
/// stops where the page's limits do (`scan`'s). A group counts once, by the first shape of its `ID` in document order, as `scan` finds shapes. Nothing is
/// walked when no row names a shape or no group is named.
fn groups_holding(doc: &Document, page: u32, groups: &HashSet<u32>, connected: &HashSet<u32>) -> HashSet<u32> {
    /// A list of shapes being walked: where it is, whether a shape in it or below it is connected, and the group it is the members of, if one that is
    /// named and is to be told.
    struct Walk<'a> {
        list: &'a ShapeList,
        at: usize,
        holds: bool,
        tell: Option<u32>,
    }
    let mut holding = HashSet::new();
    let Some(top) = doc.page(page).map(|p| &p.shapes) else { return holding };
    if groups.is_empty() || connected.is_empty() {
        return holding;
    }
    let (mut first, mut visited) = (HashSet::new(), 0_usize);
    let mut stack = vec![Walk { list: top, at: 0, holds: false, tell: None }];
    while let Some(walk) = stack.last_mut() {
        let Some(shape) = walk.list.get(walk.at) else {
            // The list is done: its group holds a connected shape if one in it did, and so does the list above if this group is itself connected or holds one.
            if let Some(done) = stack.pop() {
                if let Some(id) = done.tell.filter(|_| done.holds) {
                    holding.insert(id);
                }
                if let Some(above) = stack.last_mut() {
                    above.holds |= done.holds;
                }
            }
            continue;
        };
        walk.at = walk.at.saturating_add(1);
        visited = visited.saturating_add(1);
        #[cfg(test)]
        scan::NODES_WALKED.with(|c| c.set(c.get() + 1));
        if visited > MAX_SHAPES_PER_PAGE {
            break;
        }
        walk.holds |= connected.contains(&shape.id);
        let tell = (groups.contains(&shape.id) && first.insert(shape.id)).then_some(shape.id);
        if stack.len() < MAX_SHAPE_DEPTH && !shape.children.is_empty() {
            stack.push(Walk { list: &shape.children, at: 0, holds: false, tell });
        }
    }
    holding
}

/// What placing one shape comes to.
enum Plan {
    /// Its pin is at the place already, or it has nothing of its own to move.
    Stays,
    /// Its pin is not a finite number, or a cell would be written past the page's reach: there is no place to move it from or to.
    Unplaced,
    /// The cells to write, whether the shape is a line, and how far its pin goes on the page.
    Goes { writes: Vec<(&'static str, f64)>, one_d: bool, shift: P },
}

/// What a call asks, and what it found out about the page before it began.
struct Want<'a> {
    x: Option<f64>,
    y: Option<f64>,
    /// The ends glued, by line and end (`glue_of`).
    glued_ends: &'a HashSet<(u32, u8)>,
}

/// What it takes to put the pin of the shape `id` at `at` at the place asked for. `frame` is its parent's in the page's, and `carried` how far the groups
/// above it that are placed too have already taken it along the page.
fn plan_shape(reader: &Reader<'_>, id: u32, at: &ShapeRef, (frame, carried): (Iso, P), want: &Want<'_>) -> Plan {
    let sheet = SheetRef::Shape(at.clone());
    let pin = reader.pose(&sheet).pin;
    let on_page = frame.point(pin);
    let now = (on_page.0 + carried.0, on_page.1 + carried.1);
    if !(now.0.is_finite() && now.1.is_finite()) {
        return Plan::Unplaced;
    }
    // Where each axis asked for goes, if it has to move.
    let (to_x, to_y) = (want.x.filter(|x| (x - now.0).abs() > SAME), want.y.filter(|y| (y - now.1).abs() > SAME));
    if to_x.is_none() && to_y.is_none() {
        return Plan::Stays;
    }
    let by_page = (to_x.map_or(0.0, |x| x - now.0), to_y.map_or(0.0, |y| y - now.1));
    let (dx, dy) = frame.vector_in(by_page);
    let (move_x, move_y) = (dx.abs() > SAME, dy.abs() > SAME);
    let one_d = reader.is_one_d(&sheet);
    let mut writes: Vec<(&'static str, f64)> = Vec::new();
    if one_d {
        // The ends that are not glued move; `PinX` and `PinY` follow by their formulas (MOVE-1D).
        for (which, x_cell, y_cell) in [(0_u8, "BeginX", "BeginY"), (1_u8, "EndX", "EndY")] {
            if want.glued_ends.contains(&(id, which)) {
                continue;
            }
            if move_x {
                writes.push((x_cell, reader.number(&sheet, x_cell) + dx));
            }
            if move_y {
                writes.push((y_cell, reader.number(&sheet, y_cell) + dy));
            }
        }
    } else {
        // A shape at the top of the page has the number asked for, and not the sum of its pin and a distance, which is not that number to the last digit.
        let top = at.path.len() == 1;
        if move_x {
            writes.push(("PinX", to_x.filter(|_| top).unwrap_or(pin.0 + dx)));
        }
        if move_y {
            writes.push(("PinY", to_y.filter(|_| top).unwrap_or(pin.1 + dy)));
        }
    }
    if writes.iter().any(|(_, value)| !(value.is_finite() && value.abs() <= MAX_COORD)) {
        return Plan::Unplaced;
    }
    if writes.is_empty() {
        return Plan::Stays;
    }
    Plan::Goes { writes, one_d, shift: if one_d { NO_SHIFT } else { by_page } }
}

/// What a call of [`place_shapes`] did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Placed {
    /// The shapes that moved.
    pub moved: usize,
    /// The shapes that were left because their pin is not a finite number (the module documentation).
    pub unplaced: usize,
}

/// Puts the pin of each shape named at `x` and/or `y` on the page, in the page's drawing units. → how many shapes moved: a shape that is
/// there already, or has nothing of its own to move, is not counted and not written, and when none moves the document is not touched; and how many were
/// left because their pin is not a number.
pub fn place_shapes(doc: &mut Document, page: u32, ids: &[u32], x: Option<f64>, y: Option<f64>) -> Result<Placed, Problem> {
    if x.is_none() && y.is_none() {
        return Err(Problem::bad("give `x`, `y` or both"));
    }
    for (name, value) in [("x", x), ("y", y)] {
        if let Some(value) = value {
            checked(name, value)?;
        }
    }
    let before = doc.clone();
    let (_, found) = scan::named(doc, page, ids)?;
    let (glued_ends, connected) = glue_of(doc, page);
    let groups: HashSet<u32> = found.iter().filter(|(_, at)| doc.shape(at).is_some_and(|s| !s.children.is_empty())).map(|(id, _)| *id).collect();
    let holding = groups_holding(doc, page, &groups, &connected);
    let want = Want { x, y, glued_ends: &glued_ends };
    let mut plan: Vec<(SheetRef, &'static str, f64)> = Vec::new();
    let mut touched: Vec<u32> = Vec::new();
    // How far each shape that went goes on the page: the callouts tied to it follow.
    let mut shifts: Vec<(u32, ShapeRef, P)> = Vec::new();
    let mut placed = Placed::default();
    {
        let reader = Reader::new(doc);
        let mut above = Ancestry::new();
        // The last group handed to the routing: the groups in it that are handed on after it are in its subtree, which is theirs.
        let mut last_touched: Option<&ShapeRef> = None;
        for (id, at) in &found {
            let frame = above.parent_frame(&reader, at);
            let (writes, one_d, shift) = match plan_shape(&reader, *id, at, frame, &want) {
                Plan::Stays => continue,
                Plan::Unplaced => {
                    placed.unplaced = placed.unplaced.saturating_add(1);
                    continue;
                }
                Plan::Goes { writes, one_d, shift } => (writes, one_d, shift),
            };
            let sheet = SheetRef::Shape(at.clone());
            plan.extend(writes.into_iter().map(|(name, value)| (sheet.clone(), name, value)));
            placed.moved = placed.moved.saturating_add(1);
            if !one_d {
                shifts.push((*id, at.clone(), shift));
            }
            let is_group = groups.contains(id);
            if is_group {
                above.went(at, shift);
            }
            // The routing is handed the shapes it can have work for: a shape a connector is glued to or that is one (a connector, or a group that holds
            // one or that holds a shape one is glued to). It looks each up from the top of the page.
            let follows = connected.contains(id) || holding.contains(id) || (one_d && reader.is_routed(&sheet));
            let inside = is_group && last_touched.is_some_and(|g| at.path.len() > g.path.len() && at.path.starts_with(&g.path));
            if follows && !inside {
                touched.push(*id);
                if is_group {
                    last_touched = Some(at);
                }
            }
        }
    }
    if plan.is_empty() {
        return Ok(placed);
    }
    for (sheet, name, value) in plan {
        write_length(doc, &sheet, name, checked(name, value)?)?;
    }
    settle(doc, &before, page, &touched, &[])?;
    // The callouts tied to the shapes placed go the same way, in the same edit.
    let follow = super::callouts::followers(doc, page, &shifts)?;
    if !follow.is_empty() {
        super::shapes::move_planned(doc, page, &follow, 0)?;
    }
    Ok(placed)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use std::sync::Arc;

    use diacraft_doc::{Cell, CellName, Connect, Connects, Owner, Shape, ShapeList, Value};

    use super::*;
    use crate::draw::page_mut;
    use crate::draw::read::{FINDS, POSES_READ};
    use crate::draw::scan::NODES_WALKED;

    fn cells(shape: &mut Shape, list: &[(&str, &str)]) {
        let mut sheet = diacraft_doc::Sheet::default();
        for (name, value) in list {
            sheet.cells.insert(CellName::new(name), Cell::constant(Value::from_text(value)).with_unit("IN"));
        }
        shape.sheet = Arc::new(sheet);
    }

    /// A box 2 by 1 with its local pin at the corner and its pin at (`x`, `y`).
    fn plain(id: u32, x: f64, y: f64) -> Shape {
        let mut shape = Shape::new(id);
        cells(&mut shape, &[("PinX", &x.to_string()), ("PinY", &y.to_string()), ("Width", "2"), ("Height", "1"), ("LocPinX", "0"), ("LocPinY", "0")]);
        shape
    }

    /// A line from (0, 0) to (2, 0) with its pin where its ends put it.
    fn line(id: u32) -> Shape {
        let mut line = Shape::new(id);
        cells(&mut line, &[("BeginX", "0"), ("BeginY", "0"), ("EndX", "2"), ("EndY", "0"), ("PinX", "1"), ("PinY", "0")]);
        line
    }

    fn doc_of(shapes: Vec<Shape>) -> Document {
        let mut doc = Document::new_blank();
        page_mut(&mut doc, 1).unwrap().shapes = ShapeList::from_vec(shapes.into_iter().map(Arc::new).collect());
        doc
    }

    /// Where the pin of shape `id` is on the page.
    fn pin_of(doc: &Document, id: u32) -> P {
        let reader = Reader::new(doc);
        let at = doc.find_shape(Owner::Page(1), id).unwrap();
        let sheet = SheetRef::Shape(at.clone());
        reader.parent_to_page(&at, (reader.number(&sheet, "PinX"), reader.number(&sheet, "PinY")))
    }

    fn number(doc: &Document, id: u32, name: &str) -> f64 {
        Reader::new(doc).number(&SheetRef::Shape(doc.find_shape(Owner::Page(1), id).unwrap()), name)
    }

    fn counters_reset() {
        NODES_WALKED.with(|c| c.set(0));
        FINDS.with(|c| c.set(0));
        POSES_READ.with(|c| c.set(0));
        CONNECT_ROWS_READ.with(|c| c.set(0));
    }

    fn count(counter: &'static std::thread::LocalKey<std::cell::Cell<usize>>) -> usize {
        counter.with(std::cell::Cell::get)
    }

    #[test]
    fn nothing_given_a_number_that_is_not_one_or_one_too_big_is_refused_and_nothing_changes() {
        let mut doc = doc_of(vec![plain(1, 1.0, 1.0)]);
        let before = doc.clone();
        for (x, y) in [(None, None), (Some(f64::NAN), None), (None, Some(f64::INFINITY)), (Some(f64::NEG_INFINITY), Some(1.0)), (Some(1.0e300), None)]
        {
            let err = place_shapes(&mut doc, 1, &[1], x, y).unwrap_err();
            assert!(err.bad_params, "{x:?} {y:?}: {err:?}");
            assert_eq!(doc, before, "{x:?} {y:?}");
        }
        // The same bound as `shape.move`'s: a million units of the page.
        assert!(place_shapes(&mut doc, 1, &[1], Some(1.0e6 + 1.0), None).is_err());
        assert_eq!(place_shapes(&mut doc, 1, &[1], Some(1.0e6), None).unwrap().moved, 1);
        // A shape not on the page, or a page that is not there, is refused, with the shapes that are.
        let before = doc.clone();
        let err = place_shapes(&mut doc, 1, &[1, 99], Some(2.0), None).unwrap_err();
        assert!(err.bad_params && err.message.contains("no shape 99"), "{err:?}");
        assert_eq!(doc, before);
        assert!(place_shapes(&mut doc, 7, &[1], Some(2.0), None).is_err());
    }

    #[test]
    fn a_shape_that_is_there_already_is_not_counted_and_not_written() {
        let mut doc = doc_of(vec![plain(1, 4.0, 1.0), plain(2, 3.0, 2.0)]);
        assert_eq!(place_shapes(&mut doc, 1, &[1, 2], Some(4.0), None).unwrap().moved, 1, "shape 1 is at x = 4 already");
        let again = doc.clone();
        assert_eq!(place_shapes(&mut doc, 1, &[1, 2], Some(4.0), None).unwrap().moved, 0);
        assert_eq!(doc, again, "nothing is written when nothing moves");
        assert_eq!(pin_of(&doc, 2), (4.0, 2.0), "and y is where it was");
    }

    #[test]
    fn the_place_of_4096_shapes_on_a_page_of_20000_is_one_walk_with_no_search_and_one_pose_each() {
        // Counted, not timed: the shapes are found by one walk of the page, none is looked for from the top by its `ID`, and each pose is read once.
        let n = 20_000_u32;
        let mut doc = doc_of((1..=n).map(|id| plain(id, f64::from(id % 7), f64::from(id % 11))).collect());
        let ids: Vec<u32> = (1..=4_096).collect();
        counters_reset();
        let moved = place_shapes(&mut doc, 1, &ids, Some(50.0), None).unwrap().moved;
        assert_eq!(moved, ids.len());
        assert_eq!(count(&NODES_WALKED), n as usize, "one visit to each shape of the page");
        assert_eq!(count(&FINDS), 0, "no search from the top of the page for a shape");
        assert_eq!(count(&POSES_READ), ids.len(), "the pose of each shape named, once");
        assert!(ids.iter().all(|id| pin_of(&doc, *id).0 == 50.0));
        assert_eq!(pin_of(&doc, 5), (50.0, 5.0), "y is where it was");
        assert_eq!(pin_of(&doc, 4_097), (f64::from(4_097 % 7), f64::from(4_097 % 11)), "a shape not named is where it was");
    }

    #[test]
    fn four_thousand_shapes_in_the_deepest_of_a_chain_of_150_groups_read_each_group_once() {
        // The worst case for the frames: every shape has 150 groups above it, and carrying each through all 150 reads 150 poses for each of 4,096
        // shapes. The groups are read once for all the shapes in them.
        let (depth, leaves) = (150_u32, 4_096_u32);
        let mut inner: Vec<Shape> = (0..leaves).map(|i| plain(1_000 + i, 1.0, 1.0)).collect();
        for level in (1..=depth).rev() {
            let mut group = plain(level, 0.5, 0.25);
            group.children = ShapeList::from_vec(inner.into_iter().map(Arc::new).collect());
            inner = vec![group];
        }
        let mut doc = doc_of(inner);
        let ids: Vec<u32> = (0..leaves).map(|i| 1_000 + i).collect();
        counters_reset();
        let moved = place_shapes(&mut doc, 1, &ids, Some(200.0), Some(100.0)).unwrap().moved;
        assert_eq!(moved, ids.len());
        assert_eq!(count(&POSES_READ), ids.len() + depth as usize, "each leaf once and each group once");
        assert_eq!(count(&FINDS), 0);
        // The groups shift their members by (0.5, 0.25) each, 150 of them: the pin is on the page where it was asked to be.
        for id in [1_000, 2_048, 5_095] {
            let (x, y) = pin_of(&doc, id);
            assert!((x - 200.0).abs() < 1e-9 && (y - 100.0).abs() < 1e-9, "{id}: {x} {y}");
        }
    }

    #[test]
    fn the_ends_glued_are_found_with_one_pass_over_the_connect_rows_whatever_the_number_of_lines_named() {
        // 4,096 lines with nothing glued and 40,000 rows of connectors between other shapes: each row is read once, and not once for each line.
        let lines = 4_096_u32;
        let mut shapes: Vec<Shape> = (1..=lines).map(line).collect();
        shapes.push(plain(60_000, 1.0, 1.0));
        let mut doc = doc_of(shapes);
        let rows: Vec<Connect> = (0..40_000_u32)
            .map(|i| Connect { from_sheet: 70_000 + i, from_cell: Some(Arc::from("BeginX")), to_sheet: 60_000, ..Connect::default() })
            .collect();
        let n = rows.len();
        page_mut(&mut doc, 1).unwrap().connects = Connects::from_vec(rows);
        let ids: Vec<u32> = (1..=lines).collect();
        counters_reset();
        let moved = place_shapes(&mut doc, 1, &ids, None, Some(3.0)).unwrap().moved;
        assert_eq!(moved, ids.len());
        assert_eq!(count(&CONNECT_ROWS_READ), n, "one pass over the {n} rows");
        assert_eq!((number(&doc, 7, "BeginY"), number(&doc, 7, "EndY")), (3.0, 3.0), "both ends of a free line moved");
        assert_eq!((number(&doc, 7, "BeginX"), number(&doc, 7, "EndX")), (0.0, 2.0), "and x was left alone");
    }

    #[test]
    fn a_glued_end_stays_and_a_line_with_both_ends_glued_is_not_counted() {
        let mut doc = doc_of(vec![line(1), line(2), plain(9, 5.0, 5.0)]);
        let glue = |from: u32, cell: &str| Connect { from_sheet: from, from_cell: Some(Arc::from(cell)), to_sheet: 9, ..Connect::default() };
        page_mut(&mut doc, 1).unwrap().connects = Connects::from_vec(vec![glue(1, "BeginX"), glue(2, "BeginX"), glue(2, "EndX")]);
        let moved = place_shapes(&mut doc, 1, &[1, 2], Some(10.0), None).unwrap().moved;
        assert_eq!(moved, 1, "line 2 has both ends glued and nothing of its own to move");
        assert_eq!(
            (number(&doc, 1, "BeginX"), number(&doc, 1, "EndX")),
            (0.0, 11.0),
            "the begin is glued and stays; the end goes the distance of the pin"
        );
        assert_eq!((number(&doc, 2, "BeginX"), number(&doc, 2, "EndX")), (0.0, 2.0));
    }

    #[test]
    fn only_shapes_a_connector_can_follow_are_handed_to_the_routing() {
        // 4,097 plain shapes, the last of which a connector (not on the page: a row alone) is glued to.
        let mut doc = doc_of((1..=4_097).map(|id| plain(id, 1.0, 1.0)).collect());
        page_mut(&mut doc, 1).unwrap().connects =
            Connects::from_vec(vec![Connect { from_sheet: 90_000, from_cell: Some(Arc::from("BeginX")), to_sheet: 4_097, ..Connect::default() }]);
        counters_reset();
        let ids: Vec<u32> = (1..=4_097).collect();
        assert_eq!(place_shapes(&mut doc, 1, &ids, Some(5.0), None).unwrap().moved, 4_097);
        // The routing looks up the shape glued to (and, for a connector that is there, the connector): a handful, and not one for each shape named.
        assert!(count(&FINDS) <= 8, "searches from the top of the page: {}", count(&FINDS));
    }

    /// A group at (`x`, `y`) of its parent, turned by `angle` radians, holding `members`.
    fn group_of(id: u32, (x, y): P, angle: &str, members: Vec<Shape>) -> Shape {
        let mut group = Shape::new(id);
        cells(
            &mut group,
            &[
                ("PinX", &x.to_string()),
                ("PinY", &y.to_string()),
                ("Width", "4"),
                ("Height", "2"),
                ("LocPinX", "1"),
                ("LocPinY", "0.5"),
                ("Angle", angle),
            ],
        );
        group.children = ShapeList::from_vec(members.into_iter().map(Arc::new).collect());
        group
    }

    fn near(a: P, b: P) -> bool {
        (a.0 - b.0).abs() < 1.0e-9 && (a.1 - b.1).abs() < 1.0e-9
    }

    #[test]
    fn a_member_named_with_its_group_and_every_group_above_it_ends_at_the_place_and_is_not_moved_twice() {
        // Group 1 holds member 2 and group 3, which holds member 4; the groups are turned (a quarter turn and 30 degrees), so a distance on the page is
        // not one in the frames below. Named together, in any order, every pin is at the place on the page: the group goes first, and what a member
        // is moved by is what is left once the group has taken it along.
        let build = || {
            let inner = group_of(3, (2.0, 1.0), "0.5235987755983", vec![plain(4, 0.5, 0.25)]);
            doc_of(vec![group_of(1, (6.0, 5.0), "1.5707963267949", vec![plain(2, 1.0, 1.0), inner])])
        };
        for ids in [[1, 2, 3, 4], [4, 3, 2, 1], [2, 4, 1, 3]] {
            let mut doc = build();
            let placed = place_shapes(&mut doc, 1, &ids, Some(10.0), Some(8.0)).unwrap();
            assert_eq!(placed, Placed { moved: 4, unplaced: 0 }, "{ids:?}");
            for id in ids {
                assert!(near(pin_of(&doc, id), (10.0, 8.0)), "{ids:?}: shape {id} is at {:?}", pin_of(&doc, id));
            }
        }
        // x alone, for the member and its group: both at that x on the page, each keeping the y it had.
        let mut doc = build();
        let before: Vec<P> = (1..=4).map(|id| pin_of(&doc, id)).collect();
        assert_eq!(place_shapes(&mut doc, 1, &[1, 2, 3, 4], Some(10.0), None).unwrap().moved, 4);
        for (id, was) in (1..=4).zip(&before) {
            assert!(near(pin_of(&doc, id), (10.0, was.1)), "shape {id} from {was:?} is at {:?}", pin_of(&doc, id));
        }
        // The member alone is placed as it was: the group is not moved by being above it.
        let mut doc = build();
        let group = pin_of(&doc, 1);
        place_shapes(&mut doc, 1, &[2], Some(10.0), Some(8.0)).unwrap();
        assert!(near(pin_of(&doc, 2), (10.0, 8.0)) && near(pin_of(&doc, 1), group));
    }

    #[test]
    fn a_top_level_pin_is_the_number_asked_for_to_the_last_digit() {
        // `1.0 + (0.3 - 1.0)` is `0.30000000000000004`: the place is written, not worked out from the old pin. Three-decimal pairs, as a window sends.
        let mut doc = doc_of(vec![plain(1, 1.0, 1.0)]);
        place_shapes(&mut doc, 1, &[1], Some(0.3), Some(0.7)).unwrap();
        assert_eq!((number(&doc, 1, "PinX"), number(&doc, 1, "PinY")), (0.3, 0.7));
        let mut inexact = 0;
        for i in 0..1_000_u32 {
            let (was, to) = (f64::from(i) / 1_000.0, f64::from((i * 7 + 13) % 1_000) / 1_000.0);
            let mut doc = doc_of(vec![plain(1, was, 2.0)]);
            place_shapes(&mut doc, 1, &[1], Some(to), None).unwrap();
            inexact += usize::from(number(&doc, 1, "PinX") != to && (was - to).abs() > 1.0e-9);
            assert_eq!(number(&doc, 1, "PinY"), 2.0);
        }
        assert_eq!(inexact, 0, "every place is the number asked for");
    }

    #[test]
    fn a_shape_with_no_place_to_move_from_or_to_is_left_and_counted_and_the_others_are_placed() {
        // Group 10 and the group 11 in it have pins and local pins of 1.7e308 (each cell a finite number): the member's place on the page overflows, so
        // there is no place to move it from. Line 20 has its ends a trillion units out and its pin near the origin: moving it by the distance of its pin would write a cell past the page's reach.
        let huge = [("PinX", "1.7e308"), ("PinY", "0"), ("Width", "1"), ("Height", "1"), ("LocPinX", "-1.7e308"), ("LocPinY", "0")];
        let mut inner = group_of(11, (0.0, 0.0), "0", vec![plain(12, 1.0, 1.0)]);
        cells(&mut inner, &huge);
        let mut outer = group_of(10, (0.0, 0.0), "0", vec![inner]);
        cells(&mut outer, &huge);
        let mut far = line(20);
        cells(&mut far, &[("BeginX", "1.0e12"), ("BeginY", "0"), ("EndX", "1.0e12"), ("EndY", "1"), ("PinX", "1"), ("PinY", "0.5")]);
        let mut doc = doc_of(vec![plain(1, 1.0, 1.0), outer, far]);
        let placed = place_shapes(&mut doc, 1, &[1, 12, 20], Some(3.0), None).unwrap();
        assert_eq!(placed, Placed { moved: 1, unplaced: 2 }, "the shape that can be placed is, and the call is not refused for the others");
        assert_eq!(number(&doc, 1, "PinX"), 3.0);
        assert_eq!(number(&doc, 12, "PinX"), 1.0, "the member is as it was");
        assert_eq!(number(&doc, 20, "BeginX"), 1.0e12, "and so is the line");
        // Nothing but such shapes: the answer says so, and the document is not touched.
        let before = doc.clone();
        assert_eq!(place_shapes(&mut doc, 1, &[12, 20], Some(3.0), None).unwrap(), Placed { moved: 0, unplaced: 2 });
        assert_eq!(doc, before);
    }

    #[test]
    fn a_chain_of_190_nested_groups_that_are_all_named_over_20000_shapes_is_one_walk_for_the_groups_and_one_subtree_for_the_routing() {
        // The worst case for finding the groups that hold a shape a connector is glued to: every group of the chain holds the connected shape, and every
        // group is named. A walk of the subtree of each group is 190 times 20,000 shapes; one walk of the page that passes the answer up is 20,000.
        let (depth, leaves) = (190_u32, 20_000_u32);
        let build = |connect: bool| {
            let mut inner: Vec<Shape> = (0..leaves).map(|i| plain(1_000 + i, 1.0, 1.0)).collect();
            for level in (1..=depth).rev() {
                let mut group = plain(level, 0.5, 0.25);
                group.children = ShapeList::from_vec(inner.into_iter().map(Arc::new).collect());
                inner = vec![group];
            }
            let mut doc = doc_of(inner);
            if connect {
                // A connector (not on the page) is glued to the last leaf, the deepest and the last in the page.
                page_mut(&mut doc, 1).unwrap().connects = Connects::from_vec(vec![Connect {
                    from_sheet: 90_000,
                    from_cell: Some(Arc::from("BeginX")),
                    to_sheet: 1_000 + leaves - 1,
                    ..Connect::default()
                }]);
            }
            doc
        };
        let total = (depth + leaves) as usize;
        let ids: Vec<u32> = (1..=depth).collect();
        let reset = || {
            counters_reset();
            crate::draw::shapes::SUBTREE_NODES.with(|c| c.set(0));
        };
        let subtree_nodes = || count(&crate::draw::shapes::SUBTREE_NODES);
        // No connector on the page: no walk to find the groups that hold one, and no subtree walked.
        let mut doc = build(false);
        reset();
        assert_eq!(place_shapes(&mut doc, 1, &ids, Some(7.0), None).unwrap().moved, depth as usize);
        assert_eq!((count(&NODES_WALKED), subtree_nodes()), (total, 0));
        // A connector glued to the deepest shape: one more walk of the page, and one subtree (the outermost group's, which is the others') for the routing.
        let mut doc = build(true);
        reset();
        diacraft_doc::work::reset();
        assert_eq!(place_shapes(&mut doc, 1, &ids, Some(7.0), None).unwrap().moved, depth as usize);
        // The settling walks the page twice more: once for the shape handed to it (the outermost group), and once for the connector glued to the leaf and
        // the leaf (the connector is not on the page, so that walk goes to the end). Before a change those were searches of `find_shape`,
        // which this counter does not see, and the connector was searched for to the end of the page by each step of the settling that wanted it.
        assert_eq!(
            count(&NODES_WALKED),
            4 * total,
            "the page is walked to find the shapes and once more to find the groups that hold a connected one, and twice by the settling"
        );
        assert!(diacraft_doc::work::finds_visited() <= 2 * total as u64, "the searches by `ID` left: {}", diacraft_doc::work::finds_visited());
        assert!(subtree_nodes() <= total, "one subtree of the page for the whole chain of groups: {}", subtree_nodes());
        for id in [1, 2, 100, depth] {
            assert!(near(pin_of(&doc, id), (7.0, pin_of(&doc, id).1)), "group {id} is at the place: {:?}", pin_of(&doc, id));
        }
    }
}
