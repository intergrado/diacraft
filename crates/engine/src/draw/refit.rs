//! A group's frame after an edit of its members.
//!
//! `shape.group` makes a group whose frame is the box that holds its members, and a drawing read from a `.odg` has its groups framed so
//! (ODF gives a group no frame, and the reader makes it from the members). An edit of a member left that frame as it was, so the
//! session's group went on being the box of the members *as they were*: its selection box was the old one, a move of the group took it from there, and
//! a new group saved to a `.odg` was refused ("a new group whose frame is not the box of its members"). Reading the saved file again gave the frame
//! the session ought to have had.
//!
//! **A group fits its members, as the reference application does.** That application recalculates a group's selection rectangle when its members change,
//! unless the group's `LockCalcWH` is set (its documented meaning): so the refit is the default for every group, and a group with `LockCalcWH` set is left as it is.
//! A margin someone gave a group (a frame wider than its members) is lost once the members fill the frame by an edit; that is accepted.
//!
//! **The rule.** After an edit, every group any of whose members the edit changed (a cell written, a member added or taken away, at any depth) that
//! **was the box of its members before the edit** is that box again: its `Width` and `Height` are the box's, its pin is the middle of the box and its
//! `LocPinX` and `LocPinY` half the size, and the members stay where they are on the page: their cells are carried into the new frame by a move of
//! minus the box's lower-left corner in the group's frame (as `shape.group` does), a member's tie to the group's size
//! is written again for the group's new size, so that the member keeps its own, and **the group's own constant content** (the constant
//! points of its `Geometry` rows, its `TxtPinX` and `TxtPinY`, the `X` and `Y` of its `Connection` and `Controls` rows) is moved by the same amount, so that it
//! stays where it was on the page (a formula cell, which follows `Width` and `Height`, is left as it is: **an assumption to check**). A turned or flipped group
//! is refitted the same way: the box is in the group's own frame, and the new pin is where the group's pose carries the middle of the new box. Groups are taken
//! innermost first, so that a group holding a group sees its member's new frame. **The box is one rule: a one-dimensional member counts by its two ends and
//! nothing else** (`group::parent_points`), here, in the gate that tells whether a group was the box, in `shape.group` and in the `.odg` writer, so that the session's
//! frame is the one a reopen gives. The connectors glued to a group whose frame moved are **routed again** ([`super::settle::route_after_within`]), at most
//! [`MAX_REROUTED`] tries for the whole edit (a try that comes back as no route is a try); the others are said.
//!
//! **What is left alone** (the cells always win; none of these is reported):
//! * a group whose frame was **not** the box of its members before the edit (a frame a file chose, or one a person set), so a frame is kept up and never
//!   imposed (`tests/it/group_frame.rs`);
//! * a group with `LockCalcWH` set;
//! * a group none of whose members changed: its own cells were edited (a move, a turn, a flip, a size) and its frame is what the edit made it;
//! * a group whose `Width`, `Height`, `PinX` or `PinY` has a formula, or whose `LocPinX` or `LocPinY` has one that is not half of the size, and a group
//!   with a member whose pin has a formula of its own that is no tie to the group (a formula is data, as it is for [`super::nested`]), or a connector
//!   member with an end that is not glued and has one;
//! * a group with no member left, and a box that is not a number or is past [`super::MAX_COORD`].
//!
//! **What is left as it was, and said (revised).** A group the rule asks to refit that cannot be: it has more members than what is left of
//! [`MAX_REFIT_MEMBERS`] for the edit (a file's group of 175,000 members is not rewritten for the edit of one of them), its own points are more than what is left of
//! [`MAX_OWN_POINT_ROWS`] for the edit (counted before a row is read: a row weighs the points it holds), the refit of the group **failed** (a cell a `GUARD` protects, a number that cannot be written, a connector that cannot be routed), or the
//! evaluator's budget for the edit (`calc::Budget`, shared by the pages) does not cover the commit of the page's refits (the page's groups are then kept whole or
//! not at all). The edit stands and the group keeps the frame it had, **and the command's answer lists it** (`groups_left`: the page, the group's `ID` and why;
//! `groups_left_unlisted` counts the ones past the first [`MAX_LEFT_LISTED`]). **A group the session made and the file has never held is left and said as any other is**
//! (the edit was refused before, in words, because the `.odg` writer refused such a group at every save while its frame was not its members' box:
//! the writer writes a group whose frame is not the box as its members, and says so in a note, so that reason is gone and the refusal only stopped the edit; and a group of
//! the file that is left and then saved is told in the same way). **There is no cap on the groups an edit changes**
//! (a cap of 10,000 was reached by a move of 4,096 members each in three groups): each group the rule applies to reads at least one member of
//! the [`MAX_REFIT_MEMBERS`] an edit may read, so the allowance bounds the groups written, and the walk keeps no list of them. **Every cap is one for the edit, summed
//! over its groups and its pages** ([`Allowance`]): a cap for each group would be a cap times the number of groups (the second scan's). What is said is
//! kept to [`MAX_LEFT_LISTED`] entries as it is said, the others only counted.
//!
//! **Where.** `Session::edit_if` calls [`refit_groups`] once on the document an edit made, with the document as it was: every edit of every command, and
//! so undo and redo (they restore documents) hold the group and its members in one step. Which groups changed is found by comparing the two documents'
//! lists of shapes by the pointer each shape is behind, so an edit that touches no member of a group costs a walk of the changed lists and nothing
//! more; a list whose length changed is matched by `ID`. A member counts as changed for its group only when a cell that gives its box differs (a pin, a size,
//! a turn, a flip, the ends of a line, the rows of its geometry): a new colour or a new text that leaves the size as it was refits nothing.
//!
//! **What it costs** (counted by the tests of this file, and measured by the ignored timing test of `tests/it/group_frame.rs`). A group the walk names costs a
//! pass over its members twice (the box it had, and the box it has): no more than the shapes of the page, each group looked at once, the groups at most
//! [`MAX_SHAPE_DEPTH`] deep. The glued ends of the page are found **once for the page**, in one pass over its `Connect` rows. Each refit gives its group's frame
//! and its members' cells the numbers they are to have **as it goes** (a group holding a group reads its member's new frame at once), and the cells that read them
//! are **worked out in one commit for the page**, within one budget for the whole edit; only the cells the refit wrote are listed for it (a translation leaves the
//! other pose cells as they were: listing all seven was 60% of the commit). **The most work, measured on beast (2026-11-20, load 31 to 35, a group of small
//! rectangles read from a `.odg`):** a member moved out past the group's left edge, which lifts every member, takes 0.19 to 0.32 s with 5,000 members and **0.5 s
//! with 10,000** (the refit 0.41 to 0.45 s of it: the two boxes 0.11 s, the lift 0.11 s, the commit 0.14 to 0.17 s, which is the page's dependency index, 42 steps
//! for each of 10,000 cells), against 26 to 50 ms for a shape in no group, and **1.2 to 1.4 s with 20,000**: so the allowance of an edit is **10,000 members**, which
//! keeps one edit under about a second. The history is kept in bytes ([`crate::history::History`]) so that moves like that do not pile up.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use diacraft_calc::{Budget, Changes};
use diacraft_doc::{
    Cell, CellAddr, CellEdit, CellWrite, Document, FormulaSlot, MAX_SHAPE_DEPTH, Owner, Page, Shape, ShapeList, ShapeRef, Sheet, SheetRef, Value,
    WriteOrigin,
};

use super::arrange::{Rect, SAME_PLACE};
use super::group::parent_points;
use super::lift::{Affine, Lift, frame_cells, lift};
use super::nested::{retie, ties_to};
use super::read::{Pose, Reader};
use super::route::P;
use super::settle::{Rerouted, Routing, route_after_within, settle_kept_within};
use super::{MAX_COORD, Problem, write_length};
use diacraft_units::{Quantity, Unit};

/// How near the frame a group had must be to the box of its members for it to be that box: a millionth of an inch, which is more than the nine places a
/// cell holds and less than anything a person sees.
const FRAME_TOLERANCE: f64 = 1.0e-6;

/// The most members whose boxes one edit's refits read: 10,000, the larger of the members a group had and has counted for each group the edit changed. A group
/// of more members than what is left of it is **left as it was, and said**: a file may hold a group of 175,000 members (the tests of `odg_hold.rs` do), and
/// rewriting every member of it for each edit of one costs the memory of a whole write and a time the module's words measure (1.2 to 1.4 s at 20,000 members,
/// 0.5 s at 10,000).
pub const MAX_REFIT_MEMBERS: usize = 10_000;

/// The most groups left as they were (and connectors not routed again) that an answer lists one by one: the others are counted.
pub const MAX_LEFT_LISTED: usize = 100;

/// The most rows of the groups' own `Geometry`, `Connection` and `Controls` sections that **one edit** carries, **in all**: 20,000. A group a `.odg` is read into
/// has four of them, so this is the own points of 5,000 groups (a group with a shape of its own has a few dozen). A row counts for the points it holds (a line for
/// one, an ellipse for three), because a point is two writes: the most an edit writes for them is 40,000 cells, about half a second at the rate measured (13 us a
/// write). A group with more than what is left of the allowance is left as it was, and said, **before any of its rows is read or written**: a sheet of 100,000
/// rows would be 200,000 writes for the edit of one member, and 10,000 groups of 999 rows ten million.
pub const MAX_OWN_POINT_ROWS: usize = 20_000;

/// The most connectors that **one edit** tries to route again, **in all** (the connectors glued to the groups it refitted, whatever the number of groups and of
/// pages): 500. A route is made from the shapes the connector's ends are glued to and costs more than a cell, and a try costs the same whatever comes of it, so
/// it is the tries that are counted. Past the most the ends are where the commit put them (they follow the frame), the route is the one the connector had, and
/// the connector is said.
pub const MAX_REROUTED: usize = 500;

/// What one edit may spend, **in all** (a cap for each group, or for each page, would be a cap times their number): the members whose boxes it reads
/// ([`MAX_REFIT_MEMBERS`]), the rows of their own points it carries ([`MAX_OWN_POINT_ROWS`]) and the connectors it tries to route again ([`MAX_REROUTED`]).
struct Allowance {
    members: usize,
    own_points: usize,
    rerouted: usize,
}

impl Allowance {
    fn new() -> Allowance {
        Allowance { members: MAX_REFIT_MEMBERS, own_points: MAX_OWN_POINT_ROWS, rerouted: MAX_REROUTED }
    }
}

/// A group that the rule asked to refit and that was left as it was, or a connector glued to a refitted group that was not routed again, with the reason, in words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Left {
    pub page: u32,
    /// The `ID` of the group, or of the connector.
    pub id: u32,
    /// `"group"` or `"connector"`.
    pub kind: &'static str,
    pub why: String,
}

impl Left {
    fn group(page: u32, id: u32, why: String) -> Left {
        Left { page, id, kind: "group", why }
    }

    fn connector(page: u32, id: u32, why: String) -> Left {
        Left { page, id, kind: "connector", why: format!("it was not routed again ({why}): its ends follow the frame, its route is the one it had") }
    }

    /// A connector a plain edit (a move, a resize, a new connector) did not route again, past the tries of the edit ([`super::MAX_ROUTED_PER_EDIT`]).
    pub(super) fn not_routed(page: u32, id: u32) -> Left {
        Left {
            page,
            id,
            kind: "connector",
            why: format!(
                "it was not routed again (an edit routes {} connectors again at the most): its ends follow the shapes they are glued to, its route is the one it had",
                super::MAX_ROUTED_PER_EDIT
            ),
        }
    }
}

/// What a call of [`refit_groups`] did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Refit {
    /// How many groups were written.
    pub written: usize,
    /// The groups left as they were, the first [`MAX_LEFT_LISTED`] of them.
    pub left: Vec<Left>,
    /// How many were left besides those in `left`.
    pub unlisted: usize,
}

impl Refit {
    /// Says one group or connector left as it was: listed while fewer than [`MAX_LEFT_LISTED`] are, and only counted after that, so that what is said holds
    /// no more than the list however many are left.
    fn say(&mut self, entry: impl FnOnce() -> Left) {
        if self.left.len() < MAX_LEFT_LISTED {
            self.left.push(entry());
        } else {
            self.unlisted = self.unlisted.saturating_add(1);
        }
    }
}

// What a call costs, for the tests that hold it to a count. Not kept outside tests: one step for each group looked at and each member whose box is read; the
// `Connect` rows passed over to find the glued ends; the commits made; and the evaluator's steps they took.
#[cfg(test)]
thread_local! {
    pub(crate) static WORK: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    pub(crate) static CONNECTS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    pub(crate) static COMMITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    pub(crate) static STEPS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    pub(crate) static OWN_POINTS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// Makes the next group's refit fail, for the test of what a failure comes to.
    pub(crate) static FAIL_NEXT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
fn note_work(n: usize) {
    WORK.with(|w| w.set(w.get().saturating_add(n)));
}

#[cfg(not(test))]
fn note_work(_: usize) {}

#[cfg(test)]
fn note_commit(connects_passed: usize, steps: u64) {
    CONNECTS.with(|c| c.set(c.get().saturating_add(connects_passed)));
    COMMITS.with(|c| c.set(c.get().saturating_add(1)));
    STEPS.with(|c| c.set(c.get().saturating_add(steps)));
}

#[cfg(not(test))]
fn note_commit(_: usize, _: u64) {}

/// The cells that give the box of a shape in its parent's frame, besides the rows of its geometry: what a change of must make the group round it fit again.
const BOX_CELLS: [&str; 13] =
    ["PinX", "PinY", "Width", "Height", "LocPinX", "LocPinY", "Angle", "FlipX", "FlipY", "BeginX", "BeginY", "EndX", "EndY"];

/// Whether the edit that made `new` of `old` (one shape, an `ID` in both documents) can have moved the box the shape gives its group: a cell that gives it
/// differs, or a section of geometry (a line's route), or it is another shape (another master, deleted from its master). A new colour, a new text that leaves
/// the size as it was, a new name: none does.
fn might_move(old: &Shape, new: &Shape) -> bool {
    if old.del != new.del || old.master != new.master || old.master_shape != new.master_shape {
        return true;
    }
    if Arc::ptr_eq(&old.sheet, &new.sheet) {
        return false;
    }
    if BOX_CELLS.iter().any(|name| old.sheet.cell(name) != new.sheet.cell(name)) {
        return true;
    }
    let geometry = |shape: &Shape| shape.sheet.sections.iter().filter(|s| s.name.as_str() == "Geometry").cloned().collect::<Vec<_>>();
    let (was, now) = (geometry(old), geometry(new));
    was.len() != now.len() || was.iter().zip(&now).any(|(a, b)| !Arc::ptr_eq(a, b) && a != b)
}

/// A group whose members the edit changed: where it is in the document as it was and in the document as it is.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Candidate {
    was: Vec<u32>,
    now: Vec<u32>,
}

/// How two lists of shapes differ: the shapes that are not the same one in both (the place in the new list, and in the old) and whether a shape was
/// added or taken away.
struct Difference {
    changed: Vec<(usize, usize)>,
    added_or_taken: bool,
}

/// Pairs the shapes of two lists. The same place in two lists of one length, unless a shape that is not the very one has another `ID` there (the order was
/// changed): then by `ID`. `None` when an `ID` is in a list twice, which a hostile file can do and which pairs nothing for certain.
fn compare(old: &ShapeList, new: &ShapeList) -> Option<Difference> {
    if old.len() == new.len() {
        let mut changed = Vec::new();
        let mut at = 0_usize;
        let mut same_places = true;
        'chunks: for (a, b) in old.chunk_arcs().iter().zip(new.chunk_arcs()) {
            if !Arc::ptr_eq(a, b) {
                for (i, (o, n)) in a.iter().zip(b.iter()).enumerate() {
                    if Arc::ptr_eq(o, n) {
                        continue;
                    }
                    if o.id != n.id {
                        same_places = false;
                        break 'chunks;
                    }
                    changed.push((at.saturating_add(i), at.saturating_add(i)));
                }
            }
            at = at.saturating_add(a.len());
        }
        if same_places {
            return Some(Difference { changed, added_or_taken: false });
        }
    }
    let mut places: HashMap<u32, usize> = HashMap::with_capacity(old.len());
    for (i, shape) in old.iter().enumerate() {
        if places.insert(shape.id, i).is_some() {
            return None;
        }
    }
    let (mut changed, mut seen, mut matched) = (Vec::new(), HashSet::with_capacity(new.len()), 0_usize);
    for (n, shape) in new.iter().enumerate() {
        if !seen.insert(shape.id) {
            return None;
        }
        if let Some(&o) = places.get(&shape.id) {
            matched = matched.saturating_add(1);
            if old.get(o).is_some_and(|was| !Arc::ptr_eq(was, shape)) {
                changed.push((n, o));
            }
        }
    }
    Some(Difference { changed, added_or_taken: matched != new.len() || matched != old.len() })
}

/// The groups among the shapes of `new` whose members differ from those of the shape in `old`, each handed to `visit` as the walk finds it (the groups inside
/// them first), and **kept nowhere**: the walk holds a path and no list of groups, however many there are (a page may hold hundreds of thousands). Returns whether
/// the lists differ in what gives a box: a shape added or taken away, or one that [`might_move`]. `was` and `now` are the paths of the lists' owner in the two
/// documents.
fn walk(old: &ShapeList, new: &ShapeList, was: &mut Vec<u32>, now: &mut Vec<u32>, depth: usize, visit: &mut dyn FnMut(&Candidate)) -> bool {
    if old.is_same_list_as(new) {
        return false;
    }
    let Some(difference) = compare(old, new) else { return true };
    let mut differs = difference.added_or_taken;
    for (n, o) in difference.changed {
        let (Some(was_shape), Some(now_shape)) = (old.get(o), new.get(n)) else { continue };
        differs |= might_move(was_shape, now_shape);
        if (was_shape.children.is_empty() && now_shape.children.is_empty()) || depth >= MAX_SHAPE_DEPTH {
            continue;
        }
        let (Ok(o), Ok(n)) = (u32::try_from(o), u32::try_from(n)) else { continue };
        was.push(o);
        now.push(n);
        let inside = walk(&was_shape.children, &now_shape.children, was, now, depth + 1, visit);
        if inside && !was_shape.children.is_empty() && !now_shape.children.is_empty() {
            visit(&Candidate { was: was.clone(), now: now.clone() });
        }
        was.pop();
        now.pop();
        differs |= inside;
    }
    differs
}

/// Whether `text` holds `needle` in any case, without a copy.
fn contains_ci(text: &str, needle: &str) -> bool {
    // A window of no length is a panic of `windows`: the empty word is in every text.
    needle.is_empty() || text.as_bytes().windows(needle.len()).any(|w| w.eq_ignore_ascii_case(needle.as_bytes()))
}

/// Whether a cell of the sheet (a cell, or one of a row) has a formula that carries a point between frames (`PAR(`, `LOC(`, `LOCTOLOC(`, ...): the cells whose
/// value depends on the frame the shape is in.
fn has_frame_formula(sheet: &Sheet) -> bool {
    let carries = |cell: &Cell| matches!(&cell.formula, FormulaSlot::Local(f) if contains_ci(&f.source, "PAR(") || contains_ci(&f.source, "LOC("));
    sheet.cells.iter().any(|(_, cell)| carries(cell))
        || sheet
            .sections
            .iter()
            .any(|s| s.cells.iter().any(|(_, cell)| carries(cell)) || s.rows.iter().any(|r| r.cells.iter().any(|(_, cell)| carries(cell))))
}

/// The cells of the members a refit carried that are to be worked out with what reads them: the cells it wrote (`written`, the names under each member: a pin, an
/// end) and the cells of the member that carry a point between frames, which are in a frame that moved. **Not the other pose cells**: a translation leaves a
/// local pin, an angle and a flip as they were, and listing them put 5 cells in 7 of the commit for nothing (measured: the commit was 60% of a refit).
fn member_changes(doc: &Document, members: &[(u32, ShapeRef)], written: &[Vec<&'static str>]) -> Vec<(SheetRef, CellAddr)> {
    let mut cells: Vec<(SheetRef, CellAddr)> = Vec::new();
    for ((_, at), names) in members.iter().zip(written) {
        let sheet = SheetRef::Shape(at.clone());
        cells.extend(names.iter().map(|name| (sheet.clone(), CellAddr::direct(name))));
        if let Some(shape) = doc.shape(at)
            && has_frame_formula(&shape.sheet)
        {
            cells.extend(frame_cells(&shape.sheet).into_iter().map(|addr| (sheet.clone(), addr)));
        }
    }
    cells
}

/// The text of the formula a cell of the sheet has, its own or the one it inherits: `None` when it has none.
fn formula_of(reader: &Reader<'_>, sheet: &SheetRef, name: &str) -> Option<String> {
    let found = reader.resolver().resolve(sheet, &CellAddr::direct(name))?;
    match &found.formula.cell?.formula {
        FormulaSlot::Local(f) => Some(f.source.to_string()),
        _ => None,
    }
}

/// Whether `formula` is `<of>*0.5`, the half the reader and `shape.group` give a local pin.
fn is_half_of(formula: &str, of: &str) -> bool {
    let squeezed: String = formula.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_ascii_lowercase();
    let of = of.to_ascii_lowercase();
    squeezed == format!("{of}*0.5") || squeezed == format!("0.5*{of}")
}

/// A group's frame in its own parent's: the size, the pin and the local pin, and the pose that carries a point of the group's frame to its parent's.
struct Frame {
    size: P,
    pose: Pose,
}

impl Frame {
    fn of(reader: &Reader<'_>, sheet: &SheetRef) -> Frame {
        Frame { size: (reader.number(sheet, "Width"), reader.number(sheet, "Height")), pose: reader.pose(sheet) }
    }

    /// Whether the frame is the box `rect` of its members, which are in the group's own frame, whose pin is at the middle.
    fn is_the_box(&self, rect: &Rect) -> bool {
        let near = |a: f64, b: f64| (a - b).abs() <= FRAME_TOLERANCE;
        near(rect.x0, 0.0)
            && near(rect.y0, 0.0)
            && near(rect.x1, self.size.0)
            && near(rect.y1, self.size.1)
            && near(self.pose.loc.0, self.size.0 / 2.0)
            && near(self.pose.loc.1, self.size.1 / 2.0)
    }
}

/// The box of the members of the group at `at`, in the group's own frame: the same points as the box. `None` when there is no member or a
/// point is not a number.
fn members_box(reader: &Reader<'_>, at: &ShapeRef, members: usize) -> Option<Rect> {
    let mut points: Vec<P> = Vec::new();
    for k in 0..members {
        note_work(1);
        let member = at.child(u32::try_from(k).ok()?);
        points.extend(parent_points(reader, &member));
    }
    Rect::of(points).ok()
}

/// Whether a member's cells can be carried into a new frame without losing a formula that is data: its pin has no formula but a tie to the group, and a
/// connector's ends that are not glued have none. The glued ends are carried by their cached values, and a formula that gives the pin from the ends is
/// worked out again.
fn can_be_carried(reader: &Reader<'_>, member: &ShapeRef, group_id: u32, glued: &HashSet<(u32, bool)>, id: u32) -> bool {
    let sheet = SheetRef::Shape(member.clone());
    let is_tie = |name: &str| formula_of(reader, &sheet, name).is_none_or(|f| is_tie_text(&f, group_id));
    if reader.is_one_d(&sheet) {
        let free = |end: bool, x: &str, y: &str| {
            glued.contains(&(id, end)) || (formula_of(reader, &sheet, x).is_none() && formula_of(reader, &sheet, y).is_none())
        };
        return free(false, "BeginX", "BeginY") && free(true, "EndX", "EndY");
    }
    is_tie("PinX") && is_tie("PinY")
}

/// Whether the formula is a tie to the group `group_id` (the form [`super::nested`] writes).
fn is_tie_text(formula: &str, group_id: u32) -> bool {
    let text = formula.trim();
    ["Width", "Height"]
        .into_iter()
        .any(|cell| text.strip_prefix(&format!("Sheet.{group_id}!{cell}*")).is_some_and(|rest| rest.trim().parse::<f64>().is_ok()))
}

/// The ends of the connectors of the page that are glued, found in one pass over its `Connect` rows: `(ID, false)` for the begin and `(ID, true)` for the end.
fn glued_on(doc: &Document, page: u32) -> (HashSet<(u32, bool)>, usize) {
    let Some(p) = doc.page(page) else { return (HashSet::new(), 0) };
    let ends = p
        .connects
        .iter()
        .filter_map(|c| match c.from_cell.as_deref() {
            Some("BeginX" | "BeginY") => Some((c.from_sheet, false)),
            Some("EndX" | "EndY") => Some((c.from_sheet, true)),
            _ => None,
        })
        .collect();
    (ends, p.connects.len())
}

/// What the refit of one group came to.
enum Outcome {
    /// Not a group the rule applies to (it was not the box, it is locked, a formula is data in it): nothing to say.
    Skipped,
    /// Written, with the cells to work out.
    Written { group: u32, cells: Vec<(SheetRef, CellAddr)> },
    /// The rule applies and the group cannot be refitted: it is left as it was, for this reason.
    Left { group: u32, why: String },
}

/// The pairs of cells of a row that are points in the frame of the group that holds the section: by the section, and for a `Geometry` row by its type. A
/// relative row (`RelMoveTo`, ...) has no point of its own and moves with what it is relative to.
fn point_cells(section: &str, row_type: &str) -> &'static [(&'static str, &'static str)] {
    match section {
        "Connection" | "Controls" => &[("X", "Y")],
        "Geometry" => match row_type {
            "MoveTo" | "LineTo" | "ArcTo" | "PolylineTo" | "SplineStart" | "SplineKnot" | "NURBSTo" => &[("X", "Y")],
            "EllipticalArcTo" | "InfiniteLine" => &[("X", "Y"), ("A", "B")],
            "Ellipse" => &[("X", "Y"), ("A", "B"), ("C", "D")],
            _ => &[],
        },
        _ => &[],
    }
}

/// How many rows the sections of a group's own sheet that hold points (`Geometry`, `Connection`, `Controls`) have: a count, with no row read.
fn own_point_rows(group: &Shape) -> usize {
    group
        .sheet
        .sections
        .iter()
        .filter(|s| !s.del && matches!(s.name.as_str(), "Geometry" | "Connection" | "Controls"))
        .fold(0_usize, |sum, s| sum.saturating_add(s.rows.len()))
}

/// What carrying the group's own points weighs: each non-deleted row counts for the points it holds (one for a line, three for an ellipse, at least one), which is
/// what writing it comes to (two cells for each point). The caller has counted the rows against what is left of the edit's allowance, so this reads that many.
fn own_point_weight(group: &Shape) -> usize {
    group.sheet.sections.iter().filter(|s| !s.del && matches!(s.name.as_str(), "Geometry" | "Connection" | "Controls")).fold(
        0_usize,
        |sum, section| {
            section
                .rows
                .iter()
                .filter(|r| !r.del)
                .fold(sum, |sum, row| sum.saturating_add(point_cells(section.name.as_str(), row.row_type.as_deref().unwrap_or("")).len().max(1)))
        },
    )
}

/// The group's own constant points, carried by `by`: the cells of its own sheet that say where something is in its frame (the constant
/// `X`, `Y` and the other points of `Geometry` rows, `TxtPinX`, `TxtPinY`, the `X` and `Y` of `Connection` and `Controls` rows) are moved by the amount the
/// members were, so that they stay where they were on the page. A cell with a formula (it follows `Width` and `Height`) is left, and a cell the group inherits is
/// left. → the cells written.
fn carry_own_points(doc: &mut Document, group: &SheetRef, by: P) -> Result<Vec<(SheetRef, CellAddr)>, Problem> {
    let SheetRef::Shape(at) = group else { return Ok(Vec::new()) };
    let mut wanted: Vec<(CellAddr, f64)> = Vec::new();
    {
        let Some(shape) = doc.shape(at) else { return Ok(Vec::new()) };
        let reader = Reader::new(doc);
        let constant = |cell: &Cell| matches!(cell.formula, FormulaSlot::Absent) && cell.value.as_ref().and_then(Value::number).is_some();
        let along = |name: &str| if matches!(name, "X" | "A" | "C" | "TxtPinX") { by.0 } else { by.1 };
        for name in ["TxtPinX", "TxtPinY"] {
            if shape.sheet.cell(name).is_some_and(constant) {
                let addr = CellAddr::direct(name);
                wanted.push((addr.clone(), reader.addr_number(group, &addr) + along(name)));
            }
        }
        for section in shape.sheet.sections.iter().filter(|s| !s.del) {
            for row in section.rows.iter().filter(|r| !r.del) {
                let Some(key) = row.key(section.name.as_str()) else { continue };
                for (x, y) in point_cells(section.name.as_str(), row.row_type.as_deref().unwrap_or("")) {
                    for name in [*x, *y] {
                        if row.cells.get(name).is_some_and(constant) {
                            let addr = CellAddr::in_row(section.name.as_str(), section.ix, key.clone(), name);
                            wanted.push((addr.clone(), reader.addr_number(group, &addr) + along(name)));
                        }
                    }
                }
            }
        }
    }
    #[cfg(test)]
    OWN_POINTS.with(|c| c.set(c.get().saturating_add(wanted.len())));
    let mut written = Vec::with_capacity(wanted.len());
    for (addr, value) in wanted {
        diacraft_calc::write_value(doc, group, &addr, &Quantity::new(value, Unit::Inch), WriteOrigin::Drawing)?;
        written.push((group.clone(), addr));
    }
    Ok(written)
}

/// Refits the group the candidate names in `doc`, if the rule says to, so that the frame and the members' cells hold their new numbers. → what came of it: the
/// cells to work out (the group's, its own points' and the members'), the group left as it was and why, or nothing to do. The work is done on a copy of the
/// document and kept only when it all went through, so a refit that fails leaves the document as it was. `glued` is the page's ([`glued_on`]).
fn refit_one(
    before: &Document,
    doc: &mut Document,
    page: u32,
    candidate: &Candidate,
    glued: &HashSet<(u32, bool)>,
    allowance: &mut Allowance,
) -> Result<Outcome, Problem> {
    note_work(1);
    let owner = Owner::Page(page);
    let was_at = ShapeRef { owner, path: candidate.was.clone() };
    let at = ShapeRef { owner, path: candidate.now.clone() };
    let (Some(was), Some(now)) = (before.shape(&was_at), doc.shape(&at)) else { return Ok(Outcome::Skipped) };
    if was.id != now.id || was.children.is_empty() || now.children.is_empty() {
        return Ok(Outcome::Skipped);
    }
    let (group_id, members) = (now.id, now.children.len());
    let group = SheetRef::Shape(at.clone());
    // A group whose `LockCalcWH` is set keeps the frame it has: the reference application's own cell for "do not recalculate this group's size".
    if Reader::new(doc).number(&group, "LockCalcWH") != 0.0 {
        return Ok(Outcome::Skipped);
    }
    // A group of more members than what is left of the edit's allowance is left as it was, and said.
    let reads = was.children.len().max(members);
    if reads > allowance.members {
        let why = format!("it has {reads} members and an edit may read {MAX_REFIT_MEMBERS} in all, of which {} are left", allowance.members);
        return Ok(Outcome::Left { group: group_id, why });
    }
    allowance.members -= reads;
    // The frame the group had was the box of the members it had.
    {
        let reader = Reader::new(before);
        let Some(rect) = members_box(&reader, &was_at, was.children.len()) else { return Ok(Outcome::Skipped) };
        if !Frame::of(&reader, &SheetRef::Shape(was_at.clone())).is_the_box(&rect) {
            return Ok(Outcome::Skipped);
        }
    }
    // The box the members have now, and whether the group's cells and the members' are ones a refit may write.
    let member_refs: Vec<ShapeRef> = (0..members).filter_map(|k| u32::try_from(k).ok().map(|k| at.child(k))).collect();
    let (rect, frame, constant_pin) = {
        let reader = Reader::new(doc);
        let plain = ["Width", "Height", "PinX", "PinY"].iter().all(|name| formula_of(&reader, &group, name).is_none())
            && [("LocPinX", "Width"), ("LocPinY", "Height")]
                .iter()
                .all(|(name, of)| formula_of(&reader, &group, name).is_none_or(|f| is_half_of(&f, of)));
        let carried = member_refs.iter().zip(now.children.iter()).all(|(member, shape)| can_be_carried(&reader, member, group_id, glued, shape.id));
        if !plain || !carried {
            return Ok(Outcome::Skipped);
        }
        let Some(rect) = members_box(&reader, &at, members) else { return Ok(Outcome::Skipped) };
        let constant_pin = ["LocPinX", "LocPinY"].map(|name| formula_of(&reader, &group, name).is_none());
        (rect, Frame::of(&reader, &group), constant_pin)
    };
    let size = (rect.x1 - rect.x0, rect.y1 - rect.y0);
    let middle = rect.middle();
    let pin = frame.pose.up(middle);
    if ![rect.x0, rect.y0, size.0, size.1, pin.0, pin.1].iter().all(|v| v.is_finite() && v.abs() <= MAX_COORD) {
        return Ok(Outcome::Skipped);
    }
    let moved = rect.x0.abs() > SAME_PLACE || rect.y0.abs() > SAME_PLACE;
    // The group's own points are carried with the members, a write for each cell: a group of more rows than what is left of the edit's allowance is left as it was,
    // before a row is read, and one whose rows weigh more than what is left (a row of three points counts for three) is left as it was before a cell is written.
    if moved {
        let (rows, free) = (own_point_rows(now), allowance.own_points);
        if rows > free {
            return Ok(Outcome::Left {
                group: group_id,
                why: format!(
                    "it has {rows} rows of its own geometry, connection points and controls, and an edit may carry {MAX_OWN_POINT_ROWS} of them in all, of which {free} remain"
                ),
            });
        }
        let weight = own_point_weight(now);
        if weight > free {
            return Ok(Outcome::Left {
                group: group_id,
                why: format!(
                    "its {rows} rows of its own geometry, connection points and controls weigh {weight} (a row of two points counts for two), and an edit may carry {MAX_OWN_POINT_ROWS} in all, of which {free} remain"
                ),
            });
        }
        allowance.own_points = free - weight;
    }
    #[cfg(test)]
    if FAIL_NEXT.with(|c| c.replace(false)) {
        return Err(Problem::failed("a failure the test made"));
    }
    let local_pin = (size.0 / 2.0, size.1 / 2.0);
    let wants: [(&'static str, f64, f64, bool); 6] = [
        ("Width", frame.size.0, size.0, true),
        ("Height", frame.size.1, size.1, true),
        ("PinX", frame.pose.pin.0, pin.0, true),
        ("PinY", frame.pose.pin.1, pin.1, true),
        ("LocPinX", frame.pose.loc.0, local_pin.0, constant_pin[0]),
        ("LocPinY", frame.pose.loc.1, local_pin.1, constant_pin[1]),
    ];
    let writes: Vec<(&'static str, f64)> =
        wants.iter().filter(|(_, was, now, written)| *written && (now - was).abs() > SAME_PLACE).map(|(name, _, now, _)| (*name, *now)).collect();
    // A pin given by `Width*0.5` keeps its formula and gets its number, as a glued end does in a carry: a group round this one reads it before the commit.
    let stale: [bool; 2] = [
        !constant_pin[0] && (local_pin.0 - frame.pose.loc.0).abs() > SAME_PLACE,
        !constant_pin[1] && (local_pin.1 - frame.pose.loc.1).abs() > SAME_PLACE,
    ];
    if !moved && writes.is_empty() && !stale.iter().any(|s| *s) {
        return Ok(Outcome::Skipped);
    }
    // The work, on a copy.
    let mut trial = Document::clone(doc);
    let ties = ties_to(&trial, group_id, &member_refs);
    let mut written: Vec<Vec<&'static str>> = Vec::new();
    if moved {
        let items: Vec<Lift> = member_refs
            .iter()
            .zip(now.children.iter())
            .map(|(at, m)| Lift { id: m.id, at: at.clone(), by: Affine::translation(-rect.x0, -rect.y0) })
            .collect();
        written = lift(&mut trial, &items, glued)?;
    }
    for (name, value) in &writes {
        write_length(&mut trial, &group, name, *value)?;
    }
    // The size as the cells hold it (a cell holds nine places): half of it is what `Width*0.5` gives, and the factor of a tie is over it.
    let held = {
        let reader = Reader::new(&trial);
        (reader.number(&group, "Width"), reader.number(&group, "Height"))
    };
    // Written as the evaluator writes a result: the shortest text that reads back as the number, so that the commit finds nothing to change in it.
    let given: Vec<(SheetRef, CellWrite)> = [("LocPinX", held.0 / 2.0, stale[0]), ("LocPinY", held.1 / 2.0, stale[1])]
        .into_iter()
        .filter(|(_, _, stale)| *stale)
        .filter_map(|(name, half, _)| {
            let edit = CellEdit::Cached { value: Value::from_number(half)?, error: None, unit: None };
            Some((group.clone(), CellWrite { addr: CellAddr::direct(name), edit, origin: WriteOrigin::Evaluation }))
        })
        .collect();
    if !given.is_empty() {
        trial.write_batch(given)?;
    }
    retie(&mut trial, group_id, &ties, held)?;
    // The group's own constant points stay where they were on the page.
    let own = if moved { carry_own_points(&mut trial, &group, (-rect.x0, -rect.y0))? } else { Vec::new() };
    // The members' cells are worked out only when they were carried; a tie is written with the number it holds, and a group's size is a cell of the group.
    let carried: Vec<(u32, ShapeRef)> = if moved { now.children.iter().map(|m| m.id).zip(member_refs.iter().cloned()).collect() } else { Vec::new() };
    let mut cells = member_changes(&trial, &carried, &written);
    cells.extend(["PinX", "PinY", "Width", "Height", "LocPinX", "LocPinY"].map(|name| (group.clone(), CellAddr::direct(name))));
    cells.extend(own);
    *doc = trial;
    Ok(Outcome::Written { group: group_id, cells })
}

/// The `ID` of the group a candidate names in `doc`.
fn group_at(doc: &Document, page: u32, candidate: &Candidate) -> Option<u32> {
    doc.shape(&ShapeRef { owner: Owner::Page(page), path: candidate.now.clone() }).map(|shape| shape.id)
}

/// The `ID`s of the connectors of page `page` of `doc` with an end glued to one of `groups`, by one pass over the page's `Connect` rows.
fn glued_to(doc: &Document, page: u32, groups: &[u32]) -> Vec<u32> {
    let Some(p) = doc.page(page) else { return Vec::new() };
    let wanted: HashSet<u32> = groups.iter().copied().collect();
    let found: std::collections::BTreeSet<u32> = p.connects.iter().filter(|c| wanted.contains(&c.to_sheet)).map(|c| c.from_sheet).collect();
    found.into_iter().collect()
}

/// Refits the groups an edit left with another box of members: `before` is the document as it was and `doc` the document the edit made,
/// which is changed. → what was written and what was left as it was, with the reasons.
pub fn refit_groups(before: &Document, doc: &mut Document) -> Result<Refit, Problem> {
    refit_within(before, doc, Budget::DEFAULT_STEPS)
}

/// [`refit_groups`] with `steps` for the evaluator, one budget for all the groups of the edit, whatever the pages: each commit takes what it uses from it.
/// The groups of a page are kept together with the commit that works out what reads them, or not at all: a commit the budget does not cover leaves the page's
/// groups as they were (and the budget spent, so that the pages after it are left too), and they are said.
fn refit_within(before: &Document, doc: &mut Document, mut steps: u64) -> Result<Refit, Problem> {
    let mut report = Refit::default();
    let mut allowance = Allowance::new();
    let pages: Vec<Arc<Page>> = doc.pages.iter().cloned().collect();
    for new in pages {
        let Some(old) = before.pages.iter().find(|p| p.id == new.id) else { continue };
        if Arc::ptr_eq(old, &new) {
            continue;
        }
        let (glued, connects) = glued_on(doc, new.id);
        let post_edit = Document::clone(doc);
        let mut cells: Vec<(SheetRef, CellAddr)> = Vec::new();
        let mut done: Vec<u32> = Vec::new();
        let page = new.id;
        {
            let mut visit = |candidate: &Candidate| {
                let id = group_at(doc, page, candidate).unwrap_or_default();
                let said = match refit_one(before, doc, page, candidate, &glued, &mut allowance) {
                    Ok(Outcome::Skipped) => None,
                    Ok(Outcome::Written { group, cells: more }) => {
                        done.push(group);
                        cells.extend(more);
                        None
                    }
                    Ok(Outcome::Left { group, why }) => Some(Left::group(page, group, why)),
                    // A failure is the group's own and is said, not swallowed: the edit stands.
                    Err(problem) => Some(Left::group(page, id, format!("its refit failed: {}", problem.message))),
                };
                if let Some(said) = said {
                    report.say(|| said);
                }
            };
            walk(&old.shapes, &new.shapes, &mut Vec::new(), &mut Vec::new(), 0, &mut visit);
        }
        if !cells.is_empty() {
            // What reads the cells the refits wrote is worked out in the budget the edit has left; a commit it does not cover leaves the page's groups as they
            // were, whole.
            let mut remaining = steps;
            match settle_kept_within(doc, new.id, &Changes { cells, ..Changes::default() }, remaining) {
                Err(problem) => {
                    *doc = post_edit;
                    steps = 0;
                    let why = format!("what reads its cells could not be worked out: {}", problem.message);
                    for group in done {
                        report.say(|| Left::group(new.id, group, why.clone()));
                    }
                }
                Ok(used) => {
                    note_commit(connects, used);
                    remaining = remaining.saturating_sub(used);
                    report.written = report.written.saturating_add(done.len());
                    // The connectors glued to the groups whose frame moved are routed again, **in what the commit left of the budget** (the routing takes its
                    // recalculations' steps off it) and in what is left of the edit's tries ([`MAX_REROUTED`] in all, whatever the pages). It is done on a copy: a
                    // routing that cannot be finished leaves the refits as they are (the frames are right and the ends follow them), and says every connector it
                    // did not route.
                    let mut routed = Document::clone(doc);
                    let routing = Routing { steps: &mut remaining, limit: allowance.rerouted, strict: true };
                    match route_after_within(&mut routed, &post_edit, new.id, &done, &[], &[], routing) {
                        Ok(Rerouted { left: not_routed, attempted }) => {
                            *doc = routed;
                            allowance.rerouted = allowance.rerouted.saturating_sub(attempted);
                            for id in not_routed {
                                report.say(|| Left::connector(new.id, id, format!("an edit routes {MAX_REROUTED} connectors again at the most")));
                            }
                        }
                        Err(problem) => {
                            remaining = 0;
                            allowance.rerouted = 0;
                            let why = format!("the edit's budget did not cover it: {}", problem.message);
                            for id in glued_to(doc, new.id, &done) {
                                report.say(|| Left::connector(new.id, id, why.clone()));
                            }
                        }
                    }
                    steps = remaining;
                }
            }
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use diacraft_doc::{Cell, CellEdit, CellWrite, Connect, Shape, Value, WriteOrigin};
    use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};

    use super::*;
    use crate::draw::{drop_shape, group_shapes, move_shapes, page_mut};
    use crate::stencil::find_master;

    /// What a walk of two lists finds: whether they differ, and the groups it hands out, in the order it hands them.
    pub(super) fn collected(old: &ShapeList, new: &ShapeList) -> (bool, Vec<Candidate>) {
        let mut found = Vec::new();
        let differs = walk(old, new, &mut Vec::new(), &mut Vec::new(), 0, &mut |c: &Candidate| found.push(c.clone()));
        (differs, found)
    }

    /// How many groups a refit wrote, with no file to hold a group to.
    pub(super) fn refit_n(before: &Document, after: &mut Document) -> usize {
        refit_groups(before, after).unwrap().written
    }

    /// As [`refit_n`], within `steps` of the evaluator.
    pub(super) fn refit_in(before: &Document, after: &mut Document, steps: u64) -> usize {
        refit_within(before, after, steps).unwrap().written
    }

    fn list(shapes: &[&Arc<Shape>]) -> ShapeList {
        ShapeList::from_vec(shapes.iter().copied().map(Arc::clone).collect())
    }

    fn shape(id: u32) -> Arc<Shape> {
        Arc::new(Shape::new(id))
    }

    /// A shape with `ID` `id` whose `PinX` is `x`: another `x` is a shape that can have moved the box of its group.
    fn pinned(id: u32, x: &str) -> Arc<Shape> {
        let mut sheet = diacraft_doc::Sheet::default();
        sheet.cells.insert(diacraft_doc::CellName::new("PinX"), Cell::constant(Value::from_text(x)));
        Arc::new(Shape { sheet: Arc::new(sheet), ..Shape::new(id) })
    }

    #[test]
    fn two_lists_are_paired_by_place_and_a_shape_that_is_not_the_very_one_is_a_change() {
        let (a, b, c) = (shape(1), shape(2), shape(3));
        let before = list(&[&a, &b, &c]);
        assert!(compare(&before, &before.clone()).is_some_and(|d| d.changed.is_empty() && !d.added_or_taken));
        let b2 = shape(2);
        let after = list(&[&a, &b2, &c]);
        let d = compare(&before, &after).unwrap();
        assert_eq!((d.changed, d.added_or_taken), (vec![(1, 1)], false), "the same `ID` in the same place, another shape");
    }

    #[test]
    fn a_list_in_another_order_pairs_by_id_and_changes_nothing() {
        let (a, b, c) = (shape(1), shape(2), shape(3));
        let d = compare(&list(&[&a, &b, &c]), &list(&[&c, &a, &b])).unwrap();
        assert_eq!((d.changed, d.added_or_taken), (Vec::new(), false), "the same shapes in another order are no change of what a group holds");
        // And one of them changed on the way: found at its place in each list.
        let b2 = shape(2);
        let d = compare(&list(&[&a, &b, &c]), &list(&[&c, &b2, &a])).unwrap();
        assert_eq!((d.changed, d.added_or_taken), (vec![(1, 1)], false));
        let d = compare(&list(&[&a, &b, &c]), &list(&[&b2, &c, &a])).unwrap();
        assert_eq!((d.changed, d.added_or_taken), (vec![(0, 1)], false), "its place in the new list and in the old");
    }

    #[test]
    fn a_shape_added_or_taken_away_is_told_and_one_in_another_place_is_not() {
        let (a, b, c, d) = (shape(1), shape(2), shape(3), shape(4));
        let taken = compare(&list(&[&a, &b, &c]), &list(&[&a, &c])).unwrap();
        assert_eq!((taken.changed.len(), taken.added_or_taken), (0, true));
        let added = compare(&list(&[&a, &b]), &list(&[&a, &b, &d])).unwrap();
        assert_eq!((added.changed.len(), added.added_or_taken), (0, true));
        // One taken and one added: the same number, and not the same shapes.
        let swapped = compare(&list(&[&a, &b, &c]), &list(&[&a, &b, &d])).unwrap();
        assert_eq!((swapped.changed.len(), swapped.added_or_taken), (0, true));
    }

    #[test]
    fn an_id_in_a_list_twice_pairs_nothing_for_certain() {
        let (a, a2, b) = (shape(1), shape(1), shape(2));
        assert!(compare(&list(&[&a, &b]), &list(&[&b, &a2, &a])).is_none(), "twice in the new list");
        assert!(compare(&list(&[&a, &a2, &b]), &list(&[&b, &a])).is_none(), "twice in the old list");
        // By place the pairing needs no `ID` to be single, and the place is the same.
        let b2 = shape(2);
        assert!(compare(&list(&[&a, &a2, &b]), &list(&[&a, &a2, &b2])).is_some());
    }

    /// A blank drawing with `n` rectangles in a row on page 1 and the group of them.
    fn grouped(n: u32) -> (Document, Vec<u32>, u32) {
        let mut doc = Document::new_blank();
        let rectangle = find_master("rectangle").unwrap();
        let ids: Vec<u32> =
            (0..n).map(|k| drop_shape(&mut doc, 1, rectangle, 1.0 + f64::from(k % 40) * 0.2, 1.0 + f64::from(k / 40)).unwrap().id).collect();
        let group = group_shapes(&mut doc, 1, &ids, 0).unwrap().id;
        (doc, ids, group)
    }

    /// Whether the group's frame is the box of its members in the document.
    fn is_framed(doc: &Document, group: u32) -> bool {
        let at = doc.find_shape(Owner::Page(1), group).unwrap();
        let reader = Reader::new(doc);
        let count = doc.shape(&at).unwrap().children.len();
        let rect = members_box(&reader, &at, count).unwrap();
        Frame::of(&reader, &SheetRef::Shape(at)).is_the_box(&rect)
    }

    /// The document an edit made on a copy of `doc` and refitted, and the number of groups written.
    fn edited(doc: &Document, edit: impl FnOnce(&mut Document)) -> (usize, Document) {
        let mut after = doc.clone();
        edit(&mut after);
        let written = refit_n(doc, &mut after);
        (written, after)
    }

    #[test]
    fn a_group_whose_member_moved_is_written_and_is_the_box_of_its_members_again() {
        let (doc, ids, group) = grouped(3);
        assert!(is_framed(&doc, group));
        let (written, after) = edited(&doc, |d| {
            move_shapes(d, 1, &[ids[0]], -2.0, 1.5).unwrap();
        });
        assert_eq!(written, 1);
        assert!(is_framed(&after, group));
        // The same edit with no refit leaves the group as it was: the test of the test.
        let mut stale = doc.clone();
        move_shapes(&mut stale, 1, &[ids[0]], -2.0, 1.5).unwrap();
        assert!(!is_framed(&stale, group));
    }

    #[test]
    fn an_edit_that_changes_no_member_writes_no_group_and_costs_no_member() {
        let (mut doc, _ids, group) = grouped(3);
        let rectangle = find_master("rectangle").unwrap();
        let other = drop_shape(&mut doc, 1, rectangle, 9.0, 9.0).unwrap().id;
        WORK.with(|w| w.set(0));
        let (written, after) = edited(&doc, |d| {
            move_shapes(d, 1, &[other], 1.0, 1.0).unwrap();
        });
        assert_eq!((written, WORK.with(|w| w.get())), (0, 0));
        assert!(is_framed(&after, group));
    }

    /// The work counted for one member moved in a group of `n`.
    fn work_of_one_move_among(n: u32) -> usize {
        let (doc, ids, _) = grouped(n);
        WORK.with(|w| w.set(0));
        let (written, _) = edited(&doc, |d| {
            move_shapes(d, 1, &[ids[0]], -2.0, 1.5).unwrap();
        });
        assert_eq!(written, 1);
        WORK.with(|w| w.get())
    }

    #[test]
    fn a_member_moved_costs_the_members_of_its_group_twice_and_one_step_for_the_group() {
        let (small, large) = (work_of_one_move_among(100), work_of_one_move_among(400));
        // The box the members had, the box they have, and the group.
        assert_eq!((small, large), (201, 801));
    }

    #[test]
    fn a_member_moved_at_the_bottom_of_sixty_groups_in_groups_costs_two_members_and_a_group_at_each_level() {
        let (mut doc, ids, inner) = grouped(2);
        let rectangle = find_master("rectangle").unwrap();
        let mut top = inner;
        for level in 0..60 {
            let id = drop_shape(&mut doc, 1, rectangle, 1.0 + f64::from(level % 9), 7.0).unwrap().id;
            top = group_shapes(&mut doc, 1, &[top, id], 0).unwrap().id;
        }
        WORK.with(|w| w.set(0));
        let (written, after) = edited(&doc, |d| {
            move_shapes(d, 1, &[ids[0]], -2.0, 1.5).unwrap();
        });
        assert_eq!(written, 61, "the group of the two and the sixty round it");
        assert_eq!(WORK.with(|w| w.get()), 61 * 5, "two members, twice, and the group, at each level");
        assert!(is_framed(&after, top));
    }

    #[test]
    fn a_frame_that_was_not_the_box_is_left_whatever_clause_of_the_box_it_misses() {
        let (doc, ids, group) = grouped(2);
        let at = doc.find_shape(Owner::Page(1), group).unwrap();
        // The frame of the group made one cell wrong at a time: the size, and the local pin.
        for (name, wrong) in [("Width", 9.0), ("Height", 9.0), ("LocPinX", 0.1), ("LocPinY", 0.1)] {
            let mut stale = doc.clone();
            let cell = Cell::constant(Value::from_text(&wrong.to_string())).with_unit("IN");
            let write = CellWrite { addr: CellAddr::direct(name), edit: CellEdit::Cell(cell), origin: WriteOrigin::Drawing };
            stale.write_batch(vec![(SheetRef::Shape(at.clone()), write)]).unwrap();
            let (written, after) = edited(&stale, |d| {
                move_shapes(d, 1, &[ids[0]], -2.0, 1.5).unwrap();
            });
            assert_eq!(written, 0, "{name} made wrong");
            assert!(!is_framed(&after, group), "{name}");
        }
    }

    fn rect_of(x0: f64, y0: f64, x1: f64, y1: f64) -> Rect {
        Rect { x0, y0, x1, y1 }
    }

    #[test]
    fn a_box_that_the_members_have_off_the_origin_or_the_size_is_not_the_frame() {
        let (doc, _, group) = grouped(2);
        let at = doc.find_shape(Owner::Page(1), group).unwrap();
        let reader = Reader::new(&doc);
        let frame = Frame::of(&reader, &SheetRef::Shape(at));
        let (w, h) = frame.size;
        assert!(frame.is_the_box(&rect_of(0.0, 0.0, w, h)));
        for (name, off) in [
            ("x0", rect_of(0.5, 0.0, w, h)),
            ("y0", rect_of(0.0, 0.5, w, h)),
            ("x1", rect_of(0.0, 0.0, w + 0.5, h)),
            ("y1", rect_of(0.0, 0.0, w, h + 0.5)),
        ] {
            assert!(!frame.is_the_box(&off), "{name}");
        }
        // A millionth of an inch is the same box and two millionths are not.
        assert!(frame.is_the_box(&rect_of(0.0, 0.0, w + 0.5e-6, h)));
        assert!(!frame.is_the_box(&rect_of(0.0, 0.0, w + 2.0e-6, h)));
        assert!(frame.is_the_box(&rect_of(-0.5e-6, -0.5e-6, w, h)));
        assert!(!frame.is_the_box(&rect_of(-2.0e-6, 0.0, w, h)));
        assert!(!frame.is_the_box(&rect_of(0.0, -2.0e-6, w, h)));
    }

    #[test]
    fn the_pin_of_a_frame_is_the_middle_of_its_box_or_it_is_not_the_box() {
        let (doc, _, group) = grouped(2);
        let at = doc.find_shape(Owner::Page(1), group).unwrap();
        let reader = Reader::new(&doc);
        let mut frame = Frame::of(&reader, &SheetRef::Shape(at));
        let on = rect_of(0.0, 0.0, frame.size.0, frame.size.1);
        assert!(frame.is_the_box(&on));
        frame.pose.loc.0 += 0.25;
        assert!(!frame.is_the_box(&on), "a local pin off the middle in x");
        frame.pose.loc.0 -= 0.25;
        frame.pose.loc.1 += 0.25;
        assert!(!frame.is_the_box(&on), "a local pin off the middle in y");
    }

    #[test]
    fn a_formula_text_is_half_of_its_size_in_either_order_and_only_that() {
        for text in ["Width*0.5", " width * 0.5 ", "0.5*Width", "WIDTH*0.5"] {
            assert!(is_half_of(text, "Width"), "{text}");
        }
        for text in ["Width*0.25", "Width", "Height*0.5", "Width*0.5+1", "Sheet.1!Width*0.5", "GUARD(Width*0.5)", ""] {
            assert!(!is_half_of(text, "Width"), "{text}");
        }
        assert!(is_half_of("Height*0.5", "Height") && !is_half_of("Width*0.5", "Height"));
    }

    #[test]
    fn a_tie_is_a_formula_of_the_size_of_that_group_and_no_other() {
        for text in ["Sheet.7!Width*0.5", " Sheet.7!Height*-0.25 ", "Sheet.7!Width*0"] {
            assert!(is_tie_text(text, 7), "{text}");
        }
        for text in ["Sheet.8!Width*0.5", "Sheet.7!PinX*0.5", "Sheet.7!Width*x", "Sheet.7!Width", "Sheet.7!Width*0.5+1", "Width*0.5", ""] {
            assert!(!is_tie_text(text, 7), "{text}");
        }
    }

    /// A chain of `depth` groups, each holding the next, whose innermost holds `inner`: the list of the page that holds the outermost.
    fn chain(depth: usize, inner: &Arc<Shape>, tag: &str) -> ShapeList {
        let mut held = Arc::clone(inner);
        for level in 0..depth {
            let pin = pinned(u32::try_from(level + 1000).unwrap(), tag);
            held = Arc::new(Shape { children: list(&[&held]), ..Shape::clone(&pin) });
        }
        list(&[&held])
    }

    #[test]
    fn a_walk_of_groups_deeper_than_the_model_allows_stops_where_the_model_does() {
        let (old_inner, new_inner) = (pinned(1, "1"), pinned(1, "2"));
        let (old, new) = (chain(MAX_SHAPE_DEPTH + 40, &old_inner, "1"), chain(MAX_SHAPE_DEPTH + 40, &new_inner, "2"));
        let (differs, found) = collected(&old, &new);
        assert!(differs);
        // The groups no deeper than the model allows are found, and no group below them.
        assert_eq!(found.len(), MAX_SHAPE_DEPTH);
        assert!(found.iter().all(|c| c.now.len() <= MAX_SHAPE_DEPTH));
    }

    #[test]
    fn the_groups_a_walk_finds_are_the_ones_on_the_way_to_a_changed_member_innermost_first() {
        let (old_leaf, new_leaf) = (pinned(1, "1"), pinned(1, "2"));
        let (old, new) = (chain(3, &old_leaf, "1"), chain(3, &new_leaf, "1"));
        let (differs, found) = collected(&old, &new);
        assert!(differs);
        let paths: Vec<Vec<u32>> = found.iter().map(|c| c.now.clone()).collect();
        assert_eq!(paths, [vec![0, 0, 0], vec![0, 0], vec![0]], "the group of the leaf, then the ones round it");
        assert!(found.iter().all(|c| c.was == c.now));
        // A walk of lists that are the same finds nothing and says so.
        let (differs, none) = collected(&old, &old.clone());
        assert!(!differs && none.is_empty());
    }

    #[test]
    fn a_walk_hands_out_each_group_as_it_finds_it_and_keeps_none() {
        // A thousand groups on a page, each with a changed member: each is handed out once, in the order of the page, and the walk holds no list of them.
        let groups = |pin: &str| {
            let each = (0..1_000_u32).map(|k| {
                let leaf = pinned(1, pin);
                Arc::new(Shape { children: list(&[&leaf]), ..Shape::new(k + 1) })
            });
            ShapeList::from_vec(each.collect())
        };
        let (old, new) = (groups("1"), groups("2"));
        let mut seen = 0_usize;
        let mut last = None;
        let differs = walk(&old, &new, &mut Vec::new(), &mut Vec::new(), 0, &mut |c: &Candidate| {
            seen += 1;
            last = Some(c.now.clone());
        });
        assert!(differs);
        assert_eq!((seen, last), (1_000, Some(vec![999])));
    }

    #[test]
    fn a_group_whose_members_have_an_id_twice_is_left_and_nothing_fails() {
        // A hostile file can hold two shapes of one `ID` in a group: they are paired by nothing, the group is a candidate and is left as it was.
        let (a, a2, b) = (shape(1), shape(1), shape(2));
        let group = |members: &[&Arc<Shape>]| Arc::new(Shape { children: list(members), ..Shape::new(9) });
        let (old, new) = (list(&[&group(&[&a, &a2])]), list(&[&group(&[&b, &a, &a2])]));
        let (differs, found) = collected(&old, &new);
        assert!(differs);
        assert_eq!(found.len(), 1);
    }
    /// A leaf of 2 by 2 inches at `x` in its group's frame, 1 up.
    pub(super) fn leaf(id: u32, x: &str) -> ShapeBuilder {
        ShapeBuilder::new(id)
            .cell_u("PinX", x, "IN")
            .cell_u("PinY", "1", "IN")
            .cell_u("Width", "2", "IN")
            .cell_u("Height", "2", "IN")
            .formula("LocPinX", "Width*0.5", "1")
            .formula("LocPinY", "Height*0.5", "1")
    }

    /// A group 4 by 2 inches with its pin at (`x`, `y`) in its parent's frame: the box of two leaves at 1 and 3, or of a group like it.
    pub(super) fn group(id: u32, x: &str, y: &str) -> ShapeBuilder {
        ShapeBuilder::new(id)
            .group()
            .cell_u("PinX", x, "IN")
            .cell_u("PinY", y, "IN")
            .cell_u("Width", "4", "IN")
            .cell_u("Height", "2", "IN")
            .formula("LocPinX", "Width*0.5", "2")
            .formula("LocPinY", "Height*0.5", "1")
    }

    /// A connector with its four end cells, which a `Connect` row glues.
    pub(super) fn wire(id: u32) -> ShapeBuilder {
        ShapeBuilder::new(id).cell_u("BeginX", "0", "IN").cell_u("BeginY", "0", "IN").cell_u("EndX", "1", "IN").cell_u("EndY", "1", "IN")
    }

    /// The cell `name` of the shape `id` of page 1 made `cell`.
    fn put(doc: &mut Document, id: u32, name: &str, cell: Cell) {
        let at = doc.find_shape(Owner::Page(1), id).unwrap();
        let write = CellWrite { addr: CellAddr::direct(name), edit: CellEdit::Cell(cell), origin: WriteOrigin::Drawing };
        doc.write_batch(vec![(SheetRef::Shape(at), write)]).unwrap();
    }

    fn constant(v: &str) -> Cell {
        Cell::constant(Value::from_text(v)).with_unit("IN")
    }

    fn formula(f: &str, v: &str) -> Cell {
        Cell::with_formula(f, Some(Value::from_text(v))).with_unit("IN")
    }

    #[test]
    fn a_member_given_a_new_colour_refits_nothing_and_no_box_is_read() {
        let (doc, ids, group) = grouped(3);
        WORK.with(|w| w.set(0));
        let (written, after) = edited(&doc, |d| put(d, ids[0], "FillForegnd", constant("5")));
        assert_eq!((written, WORK.with(|w| w.get())), (0, 0), "a cell that gives no box is no reason to read the boxes");
        assert!(is_framed(&after, group));
        // The same member with a cell that gives its box is a reason.
        let (written, _) = edited(&doc, |d| put(d, ids[0], "Width", constant("2")));
        assert_eq!(written, 1);
    }

    #[test]
    fn a_shape_might_move_its_box_by_each_cell_that_gives_it_by_its_geometry_and_by_being_another_shape() {
        let base = pinned(1, "1");
        let with = |name: &str, v: &str| {
            let mut sheet = (*base.sheet).clone();
            sheet.cells.insert(diacraft_doc::CellName::new(name), constant(v));
            Shape { sheet: Arc::new(sheet), ..Shape::clone(&base) }
        };
        assert!(!might_move(&base, &Shape::clone(&base)), "the same sheet");
        assert!(!might_move(&base, &pinned(1, "1")), "another sheet with the same cells");
        assert!(might_move(&base, &pinned(1, "2")));
        for name in BOX_CELLS {
            assert!(might_move(&base, &with(name, "7")), "{name}");
        }
        assert!(!might_move(&base, &with("FillForegnd", "7")), "a colour is no box");
        assert!(might_move(&base, &Shape { del: true, ..Shape::clone(&base) }));
        assert!(might_move(&base, &Shape { master: Some(3), ..Shape::clone(&base) }));
        assert!(might_move(&base, &Shape { master_shape: Some(3), ..Shape::clone(&base) }));
        // The rows of a geometry section give a line's box: another section, or one that holds another cell.
        let section = |x: &str| {
            let mut section = diacraft_doc::Section::new("Geometry", Some(0));
            section.cells.insert(diacraft_doc::CellName::new("NoFill"), constant(x));
            Arc::new(section)
        };
        let shaped = |sections: Vec<Arc<diacraft_doc::Section>>| {
            let mut sheet = (*base.sheet).clone();
            sheet.sections = sections;
            Shape { sheet: Arc::new(sheet), ..Shape::clone(&base) }
        };
        assert!(!might_move(&shaped(vec![section("0")]), &shaped(vec![section("0")])), "the same section, another allocation");
        assert!(might_move(&shaped(vec![section("0")]), &shaped(vec![section("1")])));
        assert!(might_move(&shaped(vec![section("0")]), &shaped(vec![])), "a section taken away");
        let other = {
            let mut section = diacraft_doc::Section::new("Fill", None);
            section.cells.insert(diacraft_doc::CellName::new("NoFill"), constant("1"));
            Arc::new(section)
        };
        assert!(
            !might_move(&shaped(vec![other.clone()]), &shaped(vec![Arc::new(diacraft_doc::Section::new("Fill", None))])),
            "a section that is no geometry is no box"
        );
    }

    #[test]
    fn a_group_with_a_formula_in_its_size_or_pin_or_a_local_pin_that_is_not_half_is_left_and_one_with_a_constant_local_pin_is_refitted() {
        let (doc, ids, group) = grouped(2);
        let move_a_member = |d: &mut Document| {
            move_shapes(d, 1, &[ids[0]], -2.0, 1.5).unwrap();
        };
        let (_, framed) = edited(&doc, move_a_member);
        let frame_of = |d: &Document, name: &str| {
            let at = d.find_shape(Owner::Page(1), group).unwrap();
            Reader::new(d).number(&SheetRef::Shape(at), name)
        };
        for name in ["Width", "Height", "PinX", "PinY"] {
            let mut held = doc.clone();
            let value = frame_of(&doc, name).to_string();
            put(&mut held, group, name, formula(&value, &value));
            let (written, after) = edited(&held, move_a_member);
            assert_eq!(written, 0, "a formula in {name}");
            assert_eq!(frame_of(&after, name), frame_of(&doc, name), "{name} is left as it was");
        }
        for (name, text) in [("LocPinX", "Width*0.25"), ("LocPinY", "Height"), ("LocPinX", "GUARD(Width*0.5)")] {
            let mut held = doc.clone();
            let value = frame_of(&doc, name).to_string();
            put(&mut held, group, name, formula(text, &value));
            let (written, _) = edited(&held, move_a_member);
            assert_eq!(written, 0, "{name} = {text}");
        }
        // A local pin that is a constant at the middle: it is written with the rest, and stays a constant.
        let mut held = doc.clone();
        for name in ["LocPinX", "LocPinY"] {
            let value = frame_of(&doc, name).to_string();
            put(&mut held, group, name, constant(&value));
        }
        let (written, after) = edited(&held, move_a_member);
        assert_eq!(written, 1);
        assert!(is_framed(&after, group));
        let at = after.find_shape(Owner::Page(1), group).unwrap();
        assert!(after.shape(&at).unwrap().sheet.cell("LocPinX").unwrap().formula.text().is_none(), "a constant stays one");
        assert!((frame_of(&after, "LocPinX") - frame_of(&framed, "LocPinX")).abs() < 1.0e-8);
    }

    #[test]
    fn a_member_whose_pin_is_a_formula_that_is_no_tie_to_the_group_keeps_the_group_as_it_is_and_a_tie_does_not() {
        let (doc, ids, group) = grouped(2);
        // The members of `grouped` are ties to the group; a formula that is a tie to another shape's size is none.
        let number = |name: &str| {
            let at = doc.find_shape(Owner::Page(1), ids[1]).unwrap();
            Reader::new(&doc).number(&SheetRef::Shape(at), name).to_string()
        };
        for (text, written) in [
            (format!("Sheet.{group}!Width*0.5"), 1),
            (format!("Sheet.{}!Width*0.5", group + 1), 0),
            ("1.25".to_owned(), 0),
            ("GUARD(1)".to_owned(), 0),
        ] {
            for name in ["PinX", "PinY"] {
                let mut held = doc.clone();
                // The pin cell with the formula the case gives it and the number it had.
                put(&mut held, ids[1], name, formula(&text, &number(name)));
                let (done, _) = edited(&held, |d| {
                    move_shapes(d, 1, &[ids[0]], -2.0, 1.5).unwrap();
                });
                assert_eq!(done, written, "{name} = {text}");
            }
        }
    }

    #[test]
    fn a_connector_member_with_an_end_that_is_not_glued_and_has_a_formula_keeps_the_group_as_it_is_and_a_glued_one_does_not() {
        let build = |glued_begin: bool, formula_on: &str| {
            let mut wire = wire(30).cell_u("BeginX", "1", "IN");
            if !formula_on.is_empty() {
                wire = wire.formula(formula_on, "1", "1");
            }
            let page = PageBuilder::new().shape(group(20, "5", "5").child(leaf(21, "1")).child(leaf(22, "3")).child(wire));
            let mut doc = DocBuilder::new().page(1, page).build();
            if glued_begin {
                let row = Connect { from_sheet: 30, from_cell: Some(Arc::from("BeginX")), to_sheet: 21, ..Connect::default() };
                page_mut(&mut doc, 1).unwrap().connects = diacraft_doc::Connects::from_vec(vec![row]);
            }
            doc
        };
        for (glued, on, written) in [
            (false, "", 1),
            (false, "BeginX", 0),
            (false, "BeginY", 0),
            (false, "EndX", 0),
            (true, "BeginX", 1),
            (true, "BeginY", 1),
            (true, "EndX", 0),
        ] {
            let doc = build(glued, on);
            let (done, _) = edited(&doc, |d| put(d, 21, "PinX", constant("-1")));
            assert_eq!(done, written, "glued at the begin: {glued}, formula in {on}");
        }
    }

    #[test]
    fn a_member_box_that_is_past_the_most_a_coordinate_may_be_leaves_the_group_and_the_edit_alone() {
        let (doc, ids, group) = grouped(2);
        // Either member, far to the left or to the right: the one on the right leaves the origin of the box where it was, and only the size would be past the most.
        for (member, far) in [(0, "5000000"), (0, "-5000000"), (1, "5000000"), (1, "-5000000")] {
            let (written, after) = edited(&doc, |d| put(d, ids[member], "PinX", constant(far)));
            assert_eq!(written, 0, "member {member} at {far}");
            assert!(!is_framed(&after, group), "the frame is as it was, and the edit stands");
        }
    }

    #[test]
    fn the_glued_ends_of_a_page_are_the_begin_and_end_cells_of_its_connect_rows_counted_once() {
        let mut doc = Document::new_blank();
        let row = |from: u32, cell: Option<&str>| Connect { from_sheet: from, from_cell: cell.map(Arc::from), to_sheet: 1, ..Connect::default() };
        page_mut(&mut doc, 1).unwrap().connects = diacraft_doc::Connects::from_vec(vec![
            row(5, Some("BeginX")),
            row(5, Some("BeginY")),
            row(6, Some("EndY")),
            row(7, Some("Width")),
            row(8, None),
        ]);
        let (ends, rows) = glued_on(&doc, 1);
        assert_eq!(rows, 5);
        assert_eq!(ends, HashSet::from([(5, false), (6, true)]));
        assert_eq!(glued_on(&doc, 99), (HashSet::new(), 0), "a page that is not there");
    }
}

/// What an edit that changes the members of many groups costs (the review's worst case: a member nested a hundred and ninety deep, a thousand sibling groups,
/// connectors glued to their members): counted, in the work of the walk and of the boxes, the `Connect` rows passed over, the commits and the evaluator's steps.
#[cfg(test)]
mod cost {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use diacraft_doc::{Connect, Connects, Document, Owner, SheetRef};
    use diacraft_testkit::build::{DocBuilder, PageBuilder};

    use super::tests::{group, leaf, refit_in, refit_n, wire};
    use super::*;
    use crate::draw::page_mut;

    /// The most steps of the evaluator a group may cost besides the page's own (measured: 138 on this page; one commit of the page costs 27,000 before any
    /// group is written, which the yardstick holds).
    const STEPS_FOR_A_GROUP: u64 = 200;

    /// The deepest group the reader of a file makes: a file may hold this and the commands may not (`MAX_GROUP_DEPTH`).
    const DEEPEST: usize = 190;

    /// The `ID`s the page is built from: the chain's leaf, and for sibling group `j` its leaf.
    const CHAIN_LEAF: u32 = 10;
    pub(super) fn sibling_leaf(j: u32) -> u32 {
        100_000 + j * 10 + 2
    }

    /// A page of a chain of `levels` groups, one in the other, whose innermost holds two leaves, of `siblings` groups of two leaves each, and of `wires`
    /// connectors, each glued at both ends to leaves of sibling groups (two `Connect` rows each).
    pub(super) fn crowded(levels: usize, siblings: u32, wires: u32) -> Document {
        let mut held = group(11, "2", "1").child(leaf(CHAIN_LEAF, "1")).child(leaf(12, "3"));
        for level in 1..levels {
            let pin = if level + 1 == levels { ("10", "10") } else { ("2", "1") };
            held = group(100 + u32::try_from(level).unwrap(), pin.0, pin.1).child(held);
        }
        let mut page = PageBuilder::new().shape(held);
        for j in 0..siblings {
            let (x, y) = (2 + (j % 40) * 5, 20 + (j / 40) * 3);
            let base = 100_000 + j * 10;
            page = page.shape(group(base, &x.to_string(), &y.to_string()).child(leaf(base + 2, "1")).child(leaf(base + 3, "3")));
        }
        for w in 0..wires {
            page = page.shape(wire(900_000 + w));
        }
        let mut doc = DocBuilder::new().page(1, page).build();
        if siblings > 0 {
            let row = |from: u32, cell: &str, to: u32| Connect {
                from_sheet: from,
                from_cell: Some(Arc::from(cell)),
                to_sheet: to,
                to_cell: Some(Arc::from("Connections.X1")),
                ..Connect::default()
            };
            let rows: Vec<Connect> = (0..wires)
                .flat_map(|w| {
                    let (a, b) = (w % siblings, (w + 1) % siblings);
                    [row(900_000 + w, "BeginX", sibling_leaf(a)), row(900_000 + w, "EndX", sibling_leaf(b))]
                })
                .collect();
            page_mut(&mut doc, 1).unwrap().connects = Connects::from_vec(rows);
        }
        doc
    }

    /// `doc` with `PinX` of each leaf in `ids` made -1: it goes out of its group on the left.
    pub(super) fn leaves_moved_out(doc: &Document, ids: &[u32]) -> Document {
        let mut after = doc.clone();
        // Where each shape of page 1 is, from one walk of the page (a search for each of ten thousand would be ten thousand walks).
        let mut places: std::collections::HashMap<u32, ShapeRef> = std::collections::HashMap::new();
        let mut stack: Vec<(ShapeRef, Arc<Shape>)> = doc
            .page(1)
            .unwrap()
            .shapes
            .iter()
            .enumerate()
            .map(|(k, s)| (ShapeRef::top(Owner::Page(1), u32::try_from(k).unwrap()), Arc::clone(s)))
            .collect();
        while let Some((at, shape)) = stack.pop() {
            stack.extend(shape.children.iter().enumerate().map(|(k, c)| (at.child(u32::try_from(k).unwrap()), Arc::clone(c))));
            places.entry(shape.id).or_insert(at);
        }
        for id in ids {
            let at = places.get(id).unwrap().clone();
            write_length(&mut after, &SheetRef::Shape(at), "PinX", -1.0).unwrap();
        }
        after
    }

    /// What one call of the refit counted: the work, the `Connect` rows passed over, the commits, the evaluator's steps, and the groups written.
    #[derive(Debug, PartialEq, Eq)]
    struct Counted {
        work: usize,
        connects: usize,
        commits: usize,
        steps: u64,
        written: usize,
    }

    fn refitted(before: &Document, after: &mut Document) -> Counted {
        WORK.with(|c| c.set(0));
        CONNECTS.with(|c| c.set(0));
        COMMITS.with(|c| c.set(0));
        STEPS.with(|c| c.set(0));
        let written = refit_n(before, after);
        Counted {
            work: WORK.with(|c| c.get()),
            connects: CONNECTS.with(|c| c.get()),
            commits: COMMITS.with(|c| c.get()),
            steps: STEPS.with(|c| c.get()),
            written,
        }
    }

    /// The worst case the review names, and its yardstick: the same page with one sibling's leaf moved out.
    fn worst_and_yardstick(siblings: u32, wires: u32) -> (Counted, Counted) {
        let before = crowded(DEEPEST, siblings, wires);
        let everything: Vec<u32> = std::iter::once(CHAIN_LEAF).chain((0..siblings).map(sibling_leaf)).collect();
        let mut all = leaves_moved_out(&before, &everything);
        let mut one = leaves_moved_out(&before, &[sibling_leaf(0)]);
        (refitted(&before, &mut all), refitted(&before, &mut one))
    }

    #[test]
    fn a_member_in_a_chain_of_190_groups_and_a_thousand_sibling_groups_does_work_linear_in_the_groups_and_one_pass_over_the_connects() {
        let (siblings, wires) = (1_000_u32, 1_500_u32);
        let (worst, yardstick) = worst_and_yardstick(siblings, wires);
        let groups = DEEPEST + siblings as usize;
        assert_eq!(worst.written, groups, "the 190 of the chain and the thousand beside it");
        assert_eq!(yardstick.written, 1);
        // One step for the group, and a pass over its members for the box it had and another for the box it has: 5 for two members, 3 for one.
        assert_eq!(worst.work, 5 + (DEEPEST - 1) * 3 + siblings as usize * 5);
        assert_eq!(yardstick.work, 5);
        // The glued ends are found once for the page: each `Connect` row is passed over once, whatever the number of groups.
        let rows = 2 * wires as usize;
        assert_eq!((worst.connects, yardstick.connects), (rows, rows), "not once for each group");
        // One commit for the page, and the evaluator's steps are within the budget of one commit, however many groups were written.
        assert_eq!((worst.commits, yardstick.commits), (1, 1));
        assert!(worst.steps <= Budget::DEFAULT_STEPS, "{} steps of {}", worst.steps, Budget::DEFAULT_STEPS);
        // And linear: no more than a fixed number of steps for each group written (with the one commit's own steps for the page), by the yardstick's measure.
        assert!(
            worst.steps <= yardstick.steps + STEPS_FOR_A_GROUP * u64::try_from(groups).unwrap(),
            "{} steps for {} groups, {} for one",
            worst.steps,
            groups,
            yardstick.steps
        );
    }

    #[test]
    fn the_work_grows_with_the_groups_and_not_with_the_groups_times_the_connects() {
        // The same groups with more connectors: the work and the steps stay, and only the rows passed over grow, once each.
        let (few, _) = worst_and_yardstick(200, 100);
        let (many, _) = worst_and_yardstick(200, 3_000);
        assert_eq!((few.work, few.written, few.commits), (many.work, many.written, many.commits));
        assert_eq!((few.connects, many.connects), (200, 6_000));
        // Twice the groups, twice the work (the chain being the same).
        let (double, _) = worst_and_yardstick(400, 100);
        assert_eq!(double.work - few.work, 200 * 5);
        assert!(double.steps - few.steps <= 200 * STEPS_FOR_A_GROUP, "{} steps more for 200 groups more", double.steps - few.steps);
    }

    /// The review's input for the cap: `chains` chains of three groups, one in the other, round two leaves each (the leaves are `10_000 + 10 * k` and `+ 1`).
    fn chains_of_three(chains: u32) -> Document {
        let mut page = PageBuilder::new();
        for k in 0..chains {
            let base = 10_000 + 10 * k;
            let inner = group(base + 2, "2", "1").child(leaf(base, "1")).child(leaf(base + 1, "3"));
            let middle = group(base + 3, "2", "1").child(inner);
            let (x, y) = (2 + (k % 60) * 5, 2 + (k / 60) * 3);
            page = page.shape(group(base + 4, &x.to_string(), &y.to_string()).child(middle));
        }
        DocBuilder::new().page(1, page).build()
    }

    #[test]
    fn a_move_of_4096_members_each_in_three_groups_writes_what_the_allowance_reads_and_says_the_rest() {
        let chains = 4_096_u32;
        let before = chains_of_three(chains);
        let ids: Vec<u32> = (0..chains).map(|k| 10_000 + 10 * k).collect();
        let mut after = leaves_moved_out(&before, &ids);
        let kept = after.clone();
        COMMITS.with(|c| c.set(0));
        let report = refit_groups(&before, &mut after).unwrap();
        // A chain reads 2, 1 and 1 members: 2,500 chains take the 10,000 of the allowance, and the other 1,596 chains' groups are left as they were, and said.
        assert_eq!(report.written, 2_500 * 3);
        let left = chains as usize * 3 - report.written;
        assert_eq!(report.left.len() + report.unlisted, left, "every group left is listed or counted");
        assert_eq!((report.left.len(), report.unlisted), (MAX_LEFT_LISTED, left - MAX_LEFT_LISTED));
        assert!(report.left.iter().all(|l| l.page == 1 && l.why.contains("members and an edit may read 10000")), "{:?}", report.left.first());
        assert_eq!(COMMITS.with(|c| c.get()), 1);
        assert!(after != kept, "the groups that fit were written");
    }

    #[test]
    fn a_budget_that_the_commit_does_not_fit_in_leaves_the_groups_as_they_were_and_the_edit_stands() {
        let before = crowded(5, 20, 10);
        let all: Vec<u32> = std::iter::once(CHAIN_LEAF).chain((0..20).map(sibling_leaf)).collect();
        // The whole of the evaluator's steps is enough for a call; one step is not.
        let mut enough = leaves_moved_out(&before, &all);
        assert_eq!(refit_in(&before, &mut enough, Budget::DEFAULT_STEPS), 25);
        let mut starved = leaves_moved_out(&before, &all);
        let kept = starved.clone();
        COMMITS.with(|c| c.set(0));
        let report = refit_within(&before, &mut starved, 1).unwrap();
        assert_eq!(report.written, 0, "no group is written, and no error is the edit's");
        assert!(starved == kept, "the document is as the edit left it, every group whole");
        assert_eq!(COMMITS.with(|c| c.get()), 0);
        // Every group the refits had written is said, with the reason.
        assert_eq!(report.left.len(), 25);
        assert!(report.left.iter().all(|l| l.why.starts_with("what reads its cells could not be worked out")), "{:?}", report.left.first());
    }

    /// Whether the group `11` of page `number` is the box of its members in `doc`.
    fn framed_on(doc: &Document, number: u32) -> bool {
        let at = doc.find_shape(Owner::Page(number), 11).unwrap();
        let reader = Reader::new(doc);
        let count = doc.shape(&at).unwrap().children.len();
        let rect = members_box(&reader, &at, count).unwrap();
        Frame::of(&reader, &SheetRef::Shape(at)).is_the_box(&rect)
    }

    #[test]
    fn the_budget_is_one_for_all_the_pages_of_an_edit_and_not_one_for_each() {
        let page = || PageBuilder::new().shape(group(11, "2", "1").child(leaf(10, "1")).child(leaf(12, "3")));
        let before = DocBuilder::new().page(1, page()).page(2, page()).build();
        let made = || {
            let mut after = before.clone();
            for number in [1, 2] {
                let at = after.find_shape(Owner::Page(number), 10).unwrap();
                write_length(&mut after, &SheetRef::Shape(at), "PinX", -1.0).unwrap();
            }
            after
        };
        STEPS.with(|c| c.set(0));
        let mut full = made();
        assert_eq!(refit_n(&before, &mut full), 2, "a group on each page");
        assert!(framed_on(&full, 1) && framed_on(&full, 2));
        let both = STEPS.with(|c| c.get());
        let one = both / 2;
        // Enough for the first page and half of the second: the second page's commit finds the budget the first left it, and its group is left as it was.
        let mut short = made();
        assert_eq!(refit_in(&before, &mut short, one + one / 2), 1, "{both} steps for two pages");
        assert!(framed_on(&short, 1) && !framed_on(&short, 2), "the first page is whole and the second as the edit left it");
        assert_eq!(refit_in(&before, &mut made(), both + 1), 2);
    }

    #[test]
    fn a_commit_the_budget_does_not_cover_spends_it_and_the_pages_after_it_are_left_too() {
        // Page 1 holds a group of 3,000 leaves, whose commit is far more than the budget given; page 2 a group of two, whose own commit is not.
        let mut big = group(11, "2", "2").cell_u("Width", "2", "IN").formula("LocPinX", "Width*0.5", "1").formula("LocPinY", "Height*0.5", "1");
        for k in 0..3_000_u32 {
            big = big.child(leaf(1_000 + k, "1"));
        }
        let small = group(11, "2", "1").child(leaf(10, "1")).child(leaf(12, "3"));
        let before = DocBuilder::new().page(1, PageBuilder::new().shape(big)).page(2, PageBuilder::new().shape(small)).build();
        let moved_out = |pages: &[(u32, u32)]| {
            let mut after = before.clone();
            for (number, id) in pages {
                let at = after.find_shape(Owner::Page(*number), *id).unwrap();
                write_length(&mut after, &SheetRef::Shape(at), "PinX", -1.0).unwrap();
            }
            after
        };
        // What the small page costs alone.
        STEPS.with(|c| c.set(0));
        assert_eq!(refit_n(&before, &mut moved_out(&[(2, 10)])), 1);
        let small_alone = STEPS.with(|c| c.get());
        // A budget of three times that: the small page would fit in it, and the big page does not.
        let mut after = moved_out(&[(1, 1_000), (2, 10)]);
        assert_eq!(refit_in(&before, &mut after, small_alone * 3), 0, "the big page is left, and the budget is spent for the page after it");
        assert!(!framed_on(&after, 2), "the second page was left as the edit made it");
        // The same budget with the big page not edited: the small page is refitted in it.
        assert_eq!(refit_in(&before, &mut moved_out(&[(2, 10)]), small_alone * 3), 1);
    }

    /// A group 2 by 2 inches of `n` leaves that all fill it, on page 1: any of them moved out makes the group another box.
    fn crowd_in_one_group(n: u32) -> Document {
        let mut group = group(11, "2", "2").cell_u("Width", "2", "IN").formula("LocPinX", "Width*0.5", "1").formula("LocPinY", "Height*0.5", "1");
        for k in 0..n {
            group = group.child(leaf(1_000 + k, "1"));
        }
        DocBuilder::new().page(1, PageBuilder::new().shape(group)).build()
    }

    #[test]
    fn a_group_of_more_members_than_an_edit_may_read_is_left_as_it_was_and_one_of_just_that_many_is_refitted_within_the_budget() {
        let limit = u32::try_from(MAX_REFIT_MEMBERS).unwrap();
        // Just that many: refitted, in one commit that the budget covers.
        let before = crowd_in_one_group(limit);
        let mut after = leaves_moved_out(&before, &[1_000]);
        let counted = refitted(&before, &mut after);
        assert_eq!((counted.written, counted.commits), (1, 1));
        assert!(counted.steps <= Budget::DEFAULT_STEPS, "{} steps for {} members", counted.steps, limit);
        // One more: left, the edit made, nothing committed, no error.
        let before = crowd_in_one_group(limit + 1);
        let mut after = leaves_moved_out(&before, &[1_000]);
        let kept = after.clone();
        let counted = refitted(&before, &mut after);
        assert_eq!((counted.written, counted.commits), (0, 0));
        assert!(after == kept, "the document is as the edit left it");
        // It is said, with the group's `ID` and why.
        let mut again = leaves_moved_out(&before, &[1_000]);
        let report = refit_groups(&before, &mut again).unwrap();
        assert_eq!((report.written, report.unlisted), (0, 0));
        assert_eq!(
            report.left,
            [Left {
                page: 1,
                id: 11,
                kind: "group",
                why: format!("it has {} members and an edit may read {MAX_REFIT_MEMBERS} in all, of which {MAX_REFIT_MEMBERS} are left", limit + 1)
            }]
        );
    }

    #[test]
    fn the_members_an_edit_may_read_are_shared_by_its_groups_and_a_group_that_does_not_fit_is_left_alone() {
        // Two groups of 6,000 on one page: the first takes 6,000 of the 10,000, and the second does not fit in what is left.
        let each = 6_000_u32;
        let mut page = PageBuilder::new();
        for g in 0..2_u32 {
            let mut group = group(11 + g * 100_000, &(2 + g * 5).to_string(), "2");
            for k in 0..each {
                group = group.child(leaf(1_000 + g * 100_000 + k, "1"));
            }
            page = page.shape(group.cell_u("Width", "2", "IN").formula("LocPinX", "Width*0.5", "1").formula("LocPinY", "Height*0.5", "1"));
        }
        let before = DocBuilder::new().page(1, page).build();
        let mut after = leaves_moved_out(&before, &[1_000, 101_000]);
        let report = refit_groups(&before, &mut after).unwrap();
        assert_eq!(report.written, 1, "the first group is refitted and the second left");
        assert_eq!(report.left.len(), 1);
        assert_eq!(report.left.first().map(|l| l.id), Some(100_011), "the second, by its `ID`");
    }

    /// The real time of the worst case beside the yardstick's, for a person to read: not run by the gate (a test does not assert how long it takes).
    #[test]
    #[ignore = "real time: run by hand with --ignored --nocapture"]
    fn the_worst_case_takes_no_more_than_a_few_times_the_yardstick_for_each_group() {
        let before = crowded(DEEPEST, 1_000, 1_500);
        let everything: Vec<u32> = std::iter::once(CHAIN_LEAF).chain((0..1_000).map(sibling_leaf)).collect();
        let mut all = leaves_moved_out(&before, &everything);
        let mut one = leaves_moved_out(&before, &[sibling_leaf(0)]);
        let started = std::time::Instant::now();
        refit_groups(&before, &mut one).unwrap();
        let one_group = started.elapsed();
        let started = std::time::Instant::now();
        refit_groups(&before, &mut all).unwrap();
        let worst = started.elapsed();
        let groups = u32::try_from(DEEPEST + 1_000).unwrap();
        eprintln!("one group {one_group:?}, {groups} groups {worst:?}: {:?} for each group beside {one_group:?}", worst / groups);
        assert!(
            worst < one_group * groups * 3 + std::time::Duration::from_secs(1),
            "the worst case is not linear in the groups: {worst:?} against {one_group:?}"
        );
    }
}

/// The findings of the review and of the scan on the refit: a group's own points, the work of the walks and the routing, each counted.
#[cfg(test)]
mod findings {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use diacraft_doc::{Connect, Connects, Row, RowKey, Section, Value};
    use diacraft_testkit::build::{DocBuilder, PageBuilder};

    use super::cost::{crowded, leaves_moved_out, sibling_leaf};
    use super::tests::{group, leaf, refit_n};
    use super::*;
    use crate::draw::{End, add_connector, page_mut, shape_mut};

    /// Where the point (`x`, `y`) of the frame of the top-level shape at `at` is on the page.
    fn on_page(doc: &Document, at: &ShapeRef, p: P) -> P {
        Reader::new(doc).pose(&SheetRef::Shape(at.clone())).up(p)
    }

    /// The number the cell at `addr` of the shape at `at` holds.
    fn number(doc: &Document, at: &ShapeRef, addr: &CellAddr) -> f64 {
        Reader::new(doc).addr_number(&SheetRef::Shape(at.clone()), addr)
    }

    fn row(_section: &str, k: u32) -> RowKey {
        RowKey::Ix(k)
    }

    /// A group with a shape of its own: constant points in `Geometry`, `Connection` and `Controls` rows and in `TxtPinX` and `TxtPinY`, and a point that follows the
    /// group's width by a formula.
    fn group_with_its_own_content() -> Document {
        let g = group(20, "5", "5")
            .child(leaf(21, "1"))
            .child(leaf(22, "3"))
            .cell_u("TxtPinX", "2", "IN")
            .cell_u("TxtPinY", "1", "IN")
            .row_type("Geometry", Some(0), row("Geometry", 1), "MoveTo")
            .row_cell("Geometry", Some(0), row("Geometry", 1), "X", "0")
            .row_cell("Geometry", Some(0), row("Geometry", 1), "Y", "0")
            .row_type("Geometry", Some(0), row("Geometry", 2), "LineTo")
            .row_cell("Geometry", Some(0), row("Geometry", 2), "X", "4")
            .row_cell("Geometry", Some(0), row("Geometry", 2), "Y", "2")
            .row_type("Geometry", Some(0), row("Geometry", 3), "LineTo")
            .row_formula("Geometry", Some(0), row("Geometry", 3), "X", "Width*0.5", "2")
            .row_cell("Geometry", Some(0), row("Geometry", 3), "Y", "1")
            .row_type("Geometry", Some(0), row("Geometry", 4), "EllipticalArcTo")
            .row_cell("Geometry", Some(0), row("Geometry", 4), "X", "3")
            .row_cell("Geometry", Some(0), row("Geometry", 4), "Y", "1")
            .row_cell("Geometry", Some(0), row("Geometry", 4), "A", "2")
            .row_cell("Geometry", Some(0), row("Geometry", 4), "B", "1.5")
            .row_cell("Geometry", Some(0), row("Geometry", 4), "C", "0")
            .row_cell("Geometry", Some(0), row("Geometry", 4), "D", "1")
            .row_cell("Connection", None, row("Connection", 0), "X", "1")
            .row_cell("Connection", None, row("Connection", 0), "Y", "1")
            .row_formula("Connection", None, row("Connection", 1), "X", "Width*0.5", "2")
            .row_cell("Connection", None, row("Connection", 1), "Y", "0")
            .row_cell("Controls", None, row("Controls", 0), "X", "2")
            .row_cell("Controls", None, row("Controls", 0), "Y", "1");
        DocBuilder::new().page(1, PageBuilder::new().shape(g)).build()
    }

    #[test]
    fn a_groups_own_constant_points_stay_where_they_were_on_the_page_and_the_ones_with_a_formula_follow_the_size() {
        let before = group_with_its_own_content();
        let at = before.find_shape(Owner::Page(1), 20).unwrap();
        let mut after = before.clone();
        let leaf_at = at.child(0);
        write_length(&mut after, &SheetRef::Shape(leaf_at), "PinX", -1.0).unwrap();
        let moved = Document::clone(&after);
        assert_eq!(refit_n(&before, &mut after), 1);
        // The members moved by 2 in the group's frame (the box now starts 2 to the left of where it did), and so did every constant point.
        let constants = [
            CellAddr::direct("TxtPinX"),
            CellAddr::direct("TxtPinY"),
            CellAddr::in_row("Geometry", Some(0), RowKey::Ix(1), "X"),
            CellAddr::in_row("Geometry", Some(0), RowKey::Ix(1), "Y"),
            CellAddr::in_row("Geometry", Some(0), RowKey::Ix(2), "X"),
            CellAddr::in_row("Geometry", Some(0), RowKey::Ix(4), "X"),
            CellAddr::in_row("Geometry", Some(0), RowKey::Ix(4), "A"),
            CellAddr::in_row("Connection", None, RowKey::Ix(0), "X"),
            CellAddr::in_row("Controls", None, RowKey::Ix(0), "X"),
        ];
        for addr in &constants {
            let (was, now) = (number(&before, &at, addr), number(&after, &at, addr));
            let along_x = matches!(addr.cell.as_str(), "X" | "A" | "TxtPinX");
            assert!((now - (was + if along_x { 2.0 } else { 0.0 })).abs() < 1e-9, "{addr:?}: {was} to {now}");
        }
        // Where each is on the page: the same, though the frame is another.
        {
            let (px, py) = (CellAddr::direct("TxtPinX"), CellAddr::direct("TxtPinY"));
            let was = on_page(&before, &at, (number(&before, &at, &px), number(&before, &at, &py)));
            let now = on_page(&after, &at, (number(&after, &at, &px), number(&after, &at, &py)));
            assert!((was.0 - now.0).abs() < 1e-9 && (was.1 - now.1).abs() < 1e-9, "the text's pin was at {was:?} and is at {now:?}");
        }
        let at_row = |doc: &Document, k: u32, section: &str, ix: Option<u32>| {
            let (x, y) = (CellAddr::in_row(section, ix, RowKey::Ix(k), "X"), CellAddr::in_row(section, ix, RowKey::Ix(k), "Y"));
            on_page(doc, &at, (number(doc, &at, &x), number(doc, &at, &y)))
        };
        for (k, section, ix) in
            [(1, "Geometry", Some(0)), (2, "Geometry", Some(0)), (4, "Geometry", Some(0)), (0, "Connection", None), (0, "Controls", None)]
        {
            let (was, now) = (at_row(&before, k, section, ix), at_row(&after, k, section, ix));
            assert!((was.0 - now.0).abs() < 1e-9 && (was.1 - now.1).abs() < 1e-9, "row {k} of {section} was at {was:?} and is at {now:?}");
        }
        // A point that follows the width by its formula keeps the formula and follows: the width is 6 now.
        let follows = CellAddr::in_row("Geometry", Some(0), RowKey::Ix(3), "X");
        assert!((number(&after, &at, &follows) - 3.0).abs() < 1e-9, "Width*0.5 of a group 6 wide");
        let shape = after.shape(&at).unwrap();
        let section = shape.sheet.sections.iter().find(|s| s.name.as_str() == "Geometry").unwrap();
        assert_eq!(section.rows.get(2).and_then(|r| r.cells.get("X")).and_then(|c| c.formula.text()), Some("Width*0.5"));
        // The same edit with no refit moves them all on the page: the test of the test.
        let was = on_page(&before, &at, (number(&before, &at, &CellAddr::direct("TxtPinX")), 1.0));
        let stale = on_page(&moved, &at, (number(&moved, &at, &CellAddr::direct("TxtPinX")), 1.0));
        assert!((was.0 - stale.0).abs() < 1e-9, "the members' edit alone does not touch the group's own cells");
    }

    /// A `Geometry` section of `rows` rows of type `row_type` with constant `X` and `Y`, built directly (a row through the builder is a search of the section).
    fn rows_section(rows: usize, row_type: &str) -> Section {
        let mut section = Section::new("Geometry", Some(0));
        for k in 0..rows {
            let mut cells = diacraft_doc::Cells::new();
            cells.insert(diacraft_doc::CellName::new("X"), Cell::constant(Value::from_text("1")));
            cells.insert(diacraft_doc::CellName::new("Y"), Cell::constant(Value::from_text("1")));
            section.rows.push(Arc::new(Row { ix: Some(u32::try_from(k).unwrap()), row_type: Some(Arc::from(row_type)), cells, ..Row::default() }));
        }
        section
    }

    #[test]
    fn a_groups_own_points_stay_where_they_were_on_the_page_when_a_member_moves_out_on_both_axes() {
        let before = group_with_its_own_content();
        let at = before.find_shape(Owner::Page(1), 20).unwrap();
        let mut after = before.clone();
        // The first leaf, a box (0, 0) to (2, 2) in the group's frame, goes to x -1 and y -2: the box starts 2 to the left of where it did and 3 below it.
        write_length(&mut after, &SheetRef::Shape(at.child(0)), "PinX", -1.0).unwrap();
        write_length(&mut after, &SheetRef::Shape(at.child(0)), "PinY", -2.0).unwrap();
        assert_eq!(refit_n(&before, &mut after), 1);
        let (was_height, height) = (number(&before, &at, &CellAddr::direct("Height")), number(&after, &at, &CellAddr::direct("Height")));
        assert!((height - was_height - 3.0).abs() < 1e-9, "the group grew by 3 below: {was_height} to {height}");
        let geometry = |k: u32, name: &str| CellAddr::in_row("Geometry", Some(0), RowKey::Ix(k), name);
        let pairs = [
            (CellAddr::direct("TxtPinX"), CellAddr::direct("TxtPinY")),
            (geometry(1, "X"), geometry(1, "Y")),
            (geometry(2, "X"), geometry(2, "Y")),
            (geometry(4, "X"), geometry(4, "Y")),
            (geometry(4, "A"), geometry(4, "B")),
            (CellAddr::in_row("Connection", None, RowKey::Ix(0), "X"), CellAddr::in_row("Connection", None, RowKey::Ix(0), "Y")),
            (CellAddr::in_row("Controls", None, RowKey::Ix(0), "X"), CellAddr::in_row("Controls", None, RowKey::Ix(0), "Y")),
        ];
        for (x, y) in &pairs {
            let place = |doc: &Document| on_page(doc, &at, (number(doc, &at, x), number(doc, &at, y)));
            let (was, now) = (place(&before), place(&after));
            assert!((was.0 - now.0).abs() < 1e-9 && (was.1 - now.1).abs() < 1e-9, "{x:?} {y:?} was at {was:?} and is at {now:?}");
        }
    }

    /// A group of two leaves with a section of `rows` rows of constant points of its own.
    fn group_with_rows(rows: usize) -> Document {
        group_with_rows_of(rows, "LineTo")
    }

    fn group_with_rows_of(rows: usize, row_type: &str) -> Document {
        let g = group(20, "5", "5").child(leaf(21, "1")).child(leaf(22, "3"));
        let mut doc = DocBuilder::new().page(1, PageBuilder::new().shape(g)).build();
        let shape = shape_mut(&mut doc, 1, &[0]).unwrap();
        Arc::make_mut(&mut shape.sheet).sections.push(Arc::new(rows_section(rows, row_type)));
        doc
    }

    /// `groups` sibling groups of two leaves each (`crowded`), every one with **the same allocation** of a section of `rows` rows of its own: a page that would
    /// be as large as `groups` times `rows` rows is not built.
    fn groups_with_rows(groups: u32, rows: usize) -> Document {
        let mut doc = crowded(1, groups, 0);
        let section = Arc::new(rows_section(rows, "LineTo"));
        for j in 0..groups {
            let shape = shape_mut(&mut doc, 1, &[j + 1]).unwrap();
            Arc::make_mut(&mut shape.sheet).sections.push(Arc::clone(&section));
        }
        doc
    }

    #[test]
    fn thousands_of_groups_each_just_under_the_cap_of_a_group_carry_the_rows_of_the_edit_in_all_and_the_others_are_left_and_said() {
        // One group carries its 10,001 of the edit's 20,000 rows (an `X` and a `Y` written for each); the 1,999 others of the same size are left as they were, for
        // 9,999 are left. Without an allowance for the edit each is carried, and 2,000 times 10,001 rows are written.
        let (groups, rows) = (2_000_u32, MAX_OWN_POINT_ROWS / 2 + 1);
        let before = groups_with_rows(groups, rows);
        let leaves: Vec<u32> = (0..groups).map(sibling_leaf).collect();
        let mut after = leaves_moved_out(&before, &leaves);
        OWN_POINTS.with(|c| c.set(0));
        let report = refit_groups(&before, &mut after).unwrap();
        assert_eq!(report.written, 1);
        assert_eq!(OWN_POINTS.with(|c| c.get()), 2 * rows, "the writes of the edit are bounded by its allowance, not by the groups");
        assert_eq!(report.left.len() + report.unlisted, groups as usize - 1);
        assert_eq!((report.left.len(), report.unlisted), (MAX_LEFT_LISTED, groups as usize - 1 - MAX_LEFT_LISTED));
        assert!(
            report.left.iter().all(|l| l.kind == "group"
                && l.why.contains(&format!("an edit may carry {MAX_OWN_POINT_ROWS} of them in all, of which {} remain", MAX_OWN_POINT_ROWS - rows))),
            "{:?}",
            report.left.first()
        );
    }

    #[test]
    fn a_row_of_more_than_one_point_counts_for_each_of_its_points_in_the_allowance_of_the_edit() {
        // 6,666 rows of an ellipse (three points each) are 19,998 of the 20,000; 6,667 are 20,001, and the group is left, although 6,667 rows are far under 20,000.
        let fits = MAX_OWN_POINT_ROWS / 3;
        let (report, _) = after_a_member_moved_out(&group_with_rows_of(fits, "Ellipse"));
        assert_eq!((report.written, report.left.len()), (1, 0));
        let (report, after) = after_a_member_moved_out(&group_with_rows_of(fits + 1, "Ellipse"));
        assert_eq!((report.written, report.left.len()), (0, 1));
        assert_eq!(OWN_POINTS.with(|c| c.get()), 0, "not a cell written");
        let (rows, weight) = (fits + 1, 3 * (fits + 1));
        assert!(
            report.left[0].why.contains(&format!("its {rows} rows")) && report.left[0].why.contains(&format!("weigh {weight}")),
            "{}",
            report.left[0].why
        );
        assert!(
            report.left[0].why.contains(&format!("an edit may carry {MAX_OWN_POINT_ROWS} in all, of which {MAX_OWN_POINT_ROWS} remain")),
            "{}",
            report.left[0].why
        );
        let at = after.find_shape(Owner::Page(1), 20).unwrap();
        assert!((number(&after, &at, &CellAddr::in_row("Geometry", Some(0), RowKey::Ix(0), "X")) - 1.0).abs() < 1e-9, "the row is as it was");
    }

    /// The report of a refit after the first leaf of the group is moved out.
    fn after_a_member_moved_out(before: &Document) -> (Refit, Document) {
        let mut after = before.clone();
        let at = after.find_shape(Owner::Page(1), 20).unwrap();
        write_length(&mut after, &SheetRef::Shape(at.child(0)), "PinX", -1.0).unwrap();
        OWN_POINTS.with(|c| c.set(0));
        let report = refit_groups(before, &mut after).unwrap();
        (report, after)
    }

    #[test]
    fn a_group_of_the_most_rows_of_its_own_is_carried_and_one_row_more_is_left_before_a_row_is_read() {
        let (report, after) = after_a_member_moved_out(&group_with_rows(MAX_OWN_POINT_ROWS));
        assert_eq!((report.written, report.left.len()), (1, 0));
        assert_eq!(OWN_POINTS.with(|c| c.get()), 2 * MAX_OWN_POINT_ROWS, "an `X` and a `Y` of each row, one write each");
        let at = after.find_shape(Owner::Page(1), 20).unwrap();
        assert!(
            (number(&after, &at, &CellAddr::in_row("Geometry", Some(0), RowKey::Ix(7), "X")) - 3.0).abs() < 1e-9,
            "carried by the 2 the members were"
        );
        let (report, _) = after_a_member_moved_out(&group_with_rows(MAX_OWN_POINT_ROWS + 1));
        assert_eq!(report.written, 0);
        assert_eq!(OWN_POINTS.with(|c| c.get()), 0, "not a row read, not a cell written");
        assert_eq!(report.left.len(), 1);
        assert_eq!((report.left[0].id, report.left[0].kind), (20, "group"));
        assert!(
            report.left[0].why.contains(&format!(
                "it has {} rows of its own geometry, connection points and controls, and an edit may carry {MAX_OWN_POINT_ROWS} of them in all, of which {MAX_OWN_POINT_ROWS} remain",
                MAX_OWN_POINT_ROWS + 1
            )),
            "{}",
            report.left[0].why
        );
    }

    #[test]
    fn a_group_with_a_hundred_thousand_rows_of_its_own_costs_a_count_of_its_rows_and_no_write() {
        let before = group_with_rows(100_000);
        let (report, after) = after_a_member_moved_out(&before);
        assert_eq!((report.written, OWN_POINTS.with(|c| c.get())), (0, 0));
        assert_eq!(report.left.len(), 1);
        assert!(report.left[0].why.contains("100000 rows"), "{}", report.left[0].why);
        // The edit stands, and the group's sheet is the very allocation it was: nothing was written to it.
        let at = after.find_shape(Owner::Page(1), 20).unwrap();
        let (a, b) = (before.shape(&before.find_shape(Owner::Page(1), 20).unwrap()).unwrap(), after.shape(&at).unwrap());
        assert!(Arc::ptr_eq(&a.sheet, &b.sheet));
    }

    /// The page of the group `20` of two leaves and a Connection point 0 that follows `Width*0.5`.
    fn page_with_a_group() -> PageBuilder {
        let g = group(20, "5", "5")
            .child(leaf(21, "1"))
            .child(leaf(22, "3"))
            .row_formula("Connection", None, RowKey::Ix(0), "X", "Width*0.5", "2")
            .row_formula("Connection", None, RowKey::Ix(0), "Y", "Height*0", "0");
        PageBuilder::new().shape(g)
    }

    /// `n` connectors on page `page` of `doc` cloned from one the command makes, each with an end glued to the point 0 of the group `20`. `ours` keeps the marker
    /// of a connector the engine made, which is what a route is made again for; `broken` holds `Angle` as a constant with an error, so that the route of a
    /// connector of a file comes back as none from the routing (and no end is one the engine has to place).
    fn glue_clones(doc: &mut Document, page: u32, n: u32, ours: bool, broken: bool) {
        let made = add_connector(doc, page, &End::Point { x: 1.0, y: 9.0 }, &End::Shape { id: 20, point: Some(0) }).unwrap();
        let at = doc.find_shape(Owner::Page(page), made.id).unwrap();
        let mut original = Shape::clone(doc.shape(&at).unwrap());
        if !ours {
            Arc::make_mut(&mut original.sheet).sections.retain(|s| s.name.as_str() != "User");
        }
        if broken {
            let mut cell = Cell::constant(Value::from_text("0"));
            cell.error = Some(Arc::from("#REF!"));
            Arc::make_mut(&mut original.sheet).cells.insert(diacraft_doc::CellName::new("Angle"), cell);
        }
        let rows: Vec<Connect> = doc.page(page).unwrap().connects.iter().filter(|c| c.from_sheet == made.id).cloned().collect();
        let list = page_mut(doc, page).unwrap();
        let mut connects: Vec<Connect> = list.connects.iter().cloned().collect();
        for k in 0..n {
            let id = 50_000 + k;
            list.shapes.push(Arc::new(Shape { id, ..original.clone() }));
            connects.extend(rows.iter().map(|c| Connect { from_sheet: id, ..c.clone() }));
        }
        list.connects = Connects::from_vec(connects);
    }

    /// The group `20` and `n` connectors cloned onto it (`glue_clones`), on page 1.
    fn connectors_on_a_group(n: u32, ours: bool) -> Document {
        let mut doc = DocBuilder::new().page(1, page_with_a_group()).build();
        glue_clones(&mut doc, 1, n, ours, false);
        doc
    }

    /// `doc` with the first leaf of the group `20` of each page in `pages` moved out of it on the left.
    fn leaf_moved_out_on(doc: &Document, pages: &[u32]) -> Document {
        let mut after = doc.clone();
        for page in pages {
            let at = after.find_shape(Owner::Page(*page), 20).unwrap();
            write_length(&mut after, &SheetRef::Shape(at.child(0)), "PinX", -1.0).unwrap();
        }
        after
    }

    #[test]
    fn five_thousand_connectors_glued_to_one_refitted_group_route_the_most_and_say_the_others() {
        let connectors = 5_000_u32;
        let before = connectors_on_a_group(connectors, true);
        let mut after = before.clone();
        let at = after.find_shape(Owner::Page(1), 20).unwrap();
        write_length(&mut after, &SheetRef::Shape(at.child(0)), "PinX", -1.0).unwrap();
        COMMITS.with(|c| c.set(0));
        STEPS.with(|c| c.set(0));
        crate::draw::settle::ROUTED.with(|c| c.set(0));
        let report = refit_groups(&before, &mut after).unwrap();
        assert_eq!(report.written, 1);
        // The most are routed again, and the others are said, each by its `ID`: a hundred listed and the rest counted.
        assert_eq!(
            crate::draw::settle::ROUTED.with(|c| c.get()),
            MAX_REROUTED,
            "the work of the routing is bounded by the count, not by the connectors"
        );
        // The connector the command made is glued to the group too: one more than the clones.
        let left = connectors as usize + 1 - MAX_REROUTED;
        assert_eq!((report.left.len(), report.unlisted), (MAX_LEFT_LISTED, left - MAX_LEFT_LISTED));
        assert!(
            report.left.iter().all(|l| l.kind == "connector" && l.why.contains("not routed again") && l.id >= 50_000),
            "{:?}",
            report.left.first()
        );
        // One commit, and the steps it took are about 240 for each connector (measured: 1,190,469 for 5,001), far under the budget of 20 million.
        assert_eq!(COMMITS.with(|c| c.get()), 1);
        assert!(STEPS.with(|c| c.get()) <= 300 * u64::from(connectors) + 100_000, "{} steps", STEPS.with(|c| c.get()));
    }

    #[test]
    fn a_budget_that_the_routing_does_not_have_left_keeps_the_refits_and_says_every_connector_it_did_not_route() {
        let connectors = 600_u32;
        let before = connectors_on_a_group(connectors, false);
        let made = || {
            let mut after = before.clone();
            let at = after.find_shape(Owner::Page(1), 20).unwrap();
            write_length(&mut after, &SheetRef::Shape(at.child(0)), "PinX", -1.0).unwrap();
            after
        };
        STEPS.with(|c| c.set(0));
        let mut whole = made();
        assert_eq!(refit_within(&before, &mut whole, Budget::DEFAULT_STEPS).unwrap().written, 1);
        let commit = STEPS.with(|c| c.get());
        // The commit and one step beyond it: the evaluator has nothing left to place the ends with, and the routing is given up whole.
        let mut short = made();
        let report = refit_within(&before, &mut short, commit + 1).unwrap();
        assert_eq!(report.written, 1, "the group is refitted: its frame and what reads it are worked out");
        let connectors_said = report.left.iter().filter(|l| l.kind == "connector").count() + report.unlisted;
        assert_eq!(
            connectors_said,
            connectors as usize + 1,
            "every connector the routing did not make is said: the clones and the one the command made"
        );
        assert!(report.left.iter().all(|l| l.why.contains("the edit's budget did not cover it")), "{:?}", report.left.first());
        assert_ne!(short, before);
    }

    /// A refit of the group `20` of a page of `connectors` connectors of a file, glued to it, whose route comes back as none (`Angle` is held with an error): the
    /// tries and the routes made, and what the refit said.
    fn refit_connectors_with_no_route(connectors: u32) -> (usize, usize, Refit) {
        let mut before = DocBuilder::new().page(1, page_with_a_group()).build();
        glue_clones(&mut before, 1, connectors, false, true);
        let mut after = leaf_moved_out_on(&before, &[1]);
        crate::draw::settle::ATTEMPTED.with(|c| c.set(0));
        crate::draw::settle::ROUTED.with(|c| c.set(0));
        let report = refit_groups(&before, &mut after).unwrap();
        assert_eq!(report.written, 1);
        (crate::draw::settle::ATTEMPTED.with(|c| c.get()), crate::draw::settle::ROUTED.with(|c| c.get()), report)
    }

    #[test]
    fn connectors_whose_route_comes_back_as_none_are_attempts_all_the_same_and_the_most_are_attempted() {
        // 2,000 connectors of a file, glued to the group, whose route a refresh gives as none. A count of the routes made would never reach the limit, and each
        // would be tried; the work is the tries, and they are bounded by it.
        let connectors = 2_000_u32;
        let (attempted, routed, report) = refit_connectors_with_no_route(connectors);
        assert_eq!(attempted, MAX_REROUTED, "tries, whatever came of them: {:?}", report.left.first());
        assert!(routed <= 1, "only the connector the command made has a route to make");
        // Every connector not tried is said: the clones, and the one the command made, less the tries.
        let left = connectors as usize + 1 - MAX_REROUTED;
        assert_eq!((report.left.len(), report.unlisted), (MAX_LEFT_LISTED, left - MAX_LEFT_LISTED));
        assert!(report.left.iter().all(|l| l.kind == "connector"), "{:?}", report.left.first());
    }

    #[test]
    fn connectors_on_crossover_glued_to_a_refitted_group_are_tested_within_the_tries_and_the_others_are_said() {
        // 2,000 connectors of ours set to reroute on crossover, glued to the group, whose routes cross no box. The test of each for a crossing
        // is a try as a routing is: the most are tested, and every other one keeps its route and is said. (The tests were made outside the limit.)
        // A route that is not made again follows its ends square, and the refit moved the ends: each one tried is followed and written, none is
        // made again by the style.
        let connectors = 2_000_u32;
        let mut before = DocBuilder::new().page(1, page_with_a_group()).build();
        glue_clones(&mut before, 1, connectors, true, false);
        let list = page_mut(&mut before, 1).unwrap();
        for at in 0..list.shapes.len() {
            let shape = list.shapes.get_mut(at).map(Arc::make_mut).unwrap();
            if shape.id != 20 {
                Arc::make_mut(&mut shape.sheet).cells.insert(diacraft_doc::CellName::new("ConFixedCode"), Cell::constant(Value::from_text("3")));
            }
        }
        let mut after = leaf_moved_out_on(&before, &[1]);
        crate::draw::settle::ATTEMPTED.with(|c| c.set(0));
        crate::draw::settle::ROUTED.with(|c| c.set(0));
        let report = refit_groups(&before, &mut after).unwrap();
        assert_eq!(report.written, 1);
        let (attempted, routed) = (crate::draw::settle::ATTEMPTED.with(|c| c.get()), crate::draw::settle::ROUTED.with(|c| c.get()));
        assert_eq!(attempted, MAX_REROUTED, "a test is a try: {:?}", report.left.first());
        assert_eq!(routed, MAX_REROUTED, "no route crosses a box, so each one tried has its route followed to its ends and none is made again");
        let left = connectors as usize + 1 - MAX_REROUTED;
        assert_eq!((report.left.len(), report.unlisted), (MAX_LEFT_LISTED, left - MAX_LEFT_LISTED));
        assert!(report.left.iter().all(|l| l.kind == "connector" && l.why.contains("not routed again")), "{:?}", report.left.first());
    }

    #[test]
    fn five_thousand_connectors_with_no_route_are_tried_the_most_or_not_at_all_and_every_one_is_said() {
        // At 5,000 the evaluator's budget of the edit does not cover what placing the ends takes, and the routing is given up whole (said, with the reason): the
        // work is bounded either way, and none is lost.
        let connectors = 5_000_u32;
        let (attempted, _, report) = refit_connectors_with_no_route(connectors);
        assert!(attempted <= MAX_REROUTED, "{attempted} tries");
        assert_eq!(attempted + report.left.len() + report.unlisted, connectors as usize + 1, "each connector is tried or said");
    }

    #[test]
    fn the_routing_allowance_is_one_for_the_edit_whatever_the_number_of_pages() {
        // Two pages of 400 connectors each, one edit on both: the 500 attempts of the edit are shared, so the second page is given what the first left.
        let mut before = DocBuilder::new().page(1, page_with_a_group()).page(2, page_with_a_group()).build();
        glue_clones(&mut before, 1, 400, false, true);
        glue_clones(&mut before, 2, 400, false, true);
        let mut after = leaf_moved_out_on(&before, &[1, 2]);
        crate::draw::settle::ATTEMPTED.with(|c| c.set(0));
        let report = refit_groups(&before, &mut after).unwrap();
        assert_eq!(report.written, 2);
        assert_eq!(crate::draw::settle::ATTEMPTED.with(|c| c.get()), MAX_REROUTED);
        assert_eq!(report.left.len() + report.unlisted, 2 * 401 - MAX_REROUTED);
    }

    #[test]
    fn the_rows_of_other_sections_and_of_a_deleted_section_are_no_own_points_of_the_group() {
        // Three rows of points, and beside them more rows than an edit may carry in a section that holds none, and in a `Geometry` section that is deleted here.
        let mut doc = group_with_rows(3);
        let shape = shape_mut(&mut doc, 1, &[0]).unwrap();
        let sheet = Arc::make_mut(&mut shape.sheet);
        let mut user = Section::new("User", None);
        for k in 0..=MAX_OWN_POINT_ROWS {
            user.rows.push(Arc::new(Row { name: Some(Arc::from(format!("r{k}"))), ..Row::default() }));
        }
        sheet.sections.push(Arc::new(user));
        let mut gone = rows_section(MAX_OWN_POINT_ROWS + 1, "LineTo");
        gone.ix = Some(1);
        gone.del = true;
        sheet.sections.push(Arc::new(gone));
        let (report, _) = after_a_member_moved_out(&doc);
        assert_eq!((report.written, report.left.len()), (1, 0), "{:?}", report.left.first());
        assert_eq!(OWN_POINTS.with(|c| c.get()), 6, "an `X` and a `Y` of each of the three rows of the live section");
    }

    #[test]
    fn a_formula_that_carries_a_point_between_frames_is_found_in_any_case_in_a_cell_and_in_a_row_and_other_formulas_are_not() {
        let with = |formula: &str, in_row: bool| {
            let mut shape = Shape::new(1);
            let cell = Cell::with_formula(formula, None);
            let sheet = Arc::make_mut(&mut shape.sheet);
            if in_row {
                let mut cells = diacraft_doc::Cells::new();
                cells.insert(diacraft_doc::CellName::new("X"), cell);
                let mut section = Section::new("Geometry", Some(0));
                section.rows.push(Arc::new(Row { ix: Some(0), cells, ..Row::default() }));
                sheet.sections.push(Arc::new(section));
            } else {
                sheet.cells.insert(diacraft_doc::CellName::new("PinX"), cell);
            }
            has_frame_formula(&shape.sheet)
        };
        for in_row in [false, true] {
            assert!(with("PAR(PNT(1,2))", in_row), "PAR, in a row: {in_row}");
            assert!(with("1+par(pnt(1,2))", in_row), "in any case, in a row: {in_row}");
            assert!(with("LOC(Sheet.3!PinX)", in_row), "LOC, in a row: {in_row}");
            assert!(with("x+loc(1)", in_row), "LOC in any case, in a row: {in_row}");
            assert!(!with("Width*0.5", in_row), "a formula of another kind, in a row: {in_row}");
            assert!(!with("PA", in_row), "a text shorter than the word, in a row: {in_row}");
        }
        assert!(contains_ci("anything", "") && contains_ci("", ""), "the empty word is in every text, and is no panic");
        assert!(!contains_ci("", "x") && contains_ci("a loc( b", "LOC("));
    }

    #[test]
    fn groups_all_left_by_a_commit_the_budget_does_not_cover_are_said_up_to_the_list_and_the_rest_counted() {
        // 4,999 groups of two leaves, all of them refitted and then left by a commit that one step of the evaluator does not cover: each is said (up to the list), and
        // none is refused for the file's not holding it.
        let siblings = 4_999_u32;
        let before = crowded(1, siblings, 0);
        let all: Vec<u32> = (0..siblings).map(sibling_leaf).collect();
        let mut after = leaves_moved_out(&before, &all);
        let report = refit_within(&before, &mut after, 1).unwrap();
        assert_eq!((report.written, report.left.len(), report.unlisted), (0, MAX_LEFT_LISTED, siblings as usize - MAX_LEFT_LISTED));
    }

    #[test]
    fn a_group_with_lock_calc_wh_set_keeps_its_frame_and_is_not_reported() {
        let g = group(20, "5", "5").child(leaf(21, "1")).child(leaf(22, "3")).cell("LockCalcWH", "1");
        let before = DocBuilder::new().page(1, PageBuilder::new().shape(g)).build();
        let at = before.find_shape(Owner::Page(1), 20).unwrap();
        let mut after = before.clone();
        write_length(&mut after, &SheetRef::Shape(at.child(0)), "PinX", -1.0).unwrap();
        let kept = after.clone();
        let report = refit_groups(&before, &mut after).unwrap();
        assert_eq!((report.written, report.left.len(), report.unlisted), (0, 0, 0));
        assert!(after == kept, "the group is as the edit left it");
        // The same group without the lock is refitted.
        let mut free = DocBuilder::new().page(1, PageBuilder::new().shape(group(20, "5", "5").child(leaf(21, "1")).child(leaf(22, "3")))).build();
        let before = free.clone();
        write_length(&mut free, &SheetRef::Shape(at.child(0)), "PinX", -1.0).unwrap();
        assert_eq!(refit_n(&before, &mut free), 1);
    }

    #[test]
    fn a_move_inside_the_box_writes_no_group_commits_nothing_and_leaves_the_groups_shape_alone() {
        // Three leaves: A and B give the box, C is inside it.
        let g = group(20, "5", "5").child(leaf(21, "1")).child(leaf(22, "3")).child(leaf(23, "2"));
        let before = DocBuilder::new().page(1, PageBuilder::new().shape(g)).build();
        let at = before.find_shape(Owner::Page(1), 20).unwrap();
        let mut after = before.clone();
        write_length(&mut after, &SheetRef::Shape(at.child(2)), "PinX", 2.5).unwrap();
        let edited = after.clone();
        COMMITS.with(|c| c.set(0));
        let report = refit_groups(&before, &mut after).unwrap();
        assert_eq!((report.written, report.left.len(), COMMITS.with(|c| c.get())), (0, 0, 0));
        let group_of = |doc: &Document| Arc::clone(doc.page(1).unwrap().shapes.get(0).unwrap());
        assert!(Arc::ptr_eq(&group_of(&edited), &group_of(&after)), "the refit did not touch the group");
        assert!(after == edited);
    }

    #[test]
    fn a_failure_of_a_groups_own_refit_is_said_and_not_swallowed() {
        let g = group(20, "5", "5").child(leaf(21, "1")).child(leaf(22, "3"));
        let before = DocBuilder::new().page(1, PageBuilder::new().shape(g)).build();
        let moved = |before: &Document| {
            let mut after = before.clone();
            let at = after.find_shape(Owner::Page(1), 20).unwrap();
            write_length(&mut after, &SheetRef::Shape(at.child(0)), "PinX", -1.0).unwrap();
            after
        };
        FAIL_NEXT.with(|c| c.set(true));
        let report = refit_groups(&before, &mut moved(&before)).unwrap();
        assert_eq!(report.written, 0);
        assert_eq!(report.left, [Left::group(1, 20, "its refit failed: a failure the test made".to_owned())]);
        // Nothing is left over: the next edit is refitted.
        assert_eq!(refit_n(&before, &mut moved(&before)), 1);
    }
}
