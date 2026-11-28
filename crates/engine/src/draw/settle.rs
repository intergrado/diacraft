//! After an edit: recalculate what it changed through `calc`, then route again the connectors it moved.

use std::collections::{BTreeSet, HashSet};

use diacraft_calc::{Budget, Changes, PageNames};
use diacraft_doc::{CellAddr, Document, Lookup, Owner, Resolver, SectionKey, ShapeRef, SheetRef, Value};

use crate::calc_index::{changes_between, commit_changed};

use super::connectors::{Planned, plan_route, plan_squared, write_planned};
use super::glue;
use super::jumps;
use super::label;
use super::read::Reader;
use super::refit::{Left, MAX_LEFT_LISTED};
use super::reroute::{EndIndex, Fate, connectors_to_route};
use super::scan::Located;
use super::shapes::subtree_ids;
use super::sheet::{changed_pages, comparison_base, moves_shape};
use super::{Problem, require_page};

// How many recalculations this thread has made, for the tests that hold a command to the few it needs. Not kept outside tests.
#[cfg(test)]
thread_local! {
    pub(crate) static SETTLES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Recalculates what the edit changed on page `page`, so that the saved `V` values are right for the renderer and for the writer, then
/// routes again the connectors the edit moved ([`super::reroute`] says which: a connector of ours that follows a shape in `touched` or is named
/// in `touched` or `extra`, and a connector of a file with a glued end that moved, unless its route is fixed). `before` is the document as the
/// command found it, and what differs from it on the page is what the edit changed (`calc::changes_between`).
///
/// **What is recalculated** (`calc::commit_changed`): the cells the edit wrote, and every cell that reads one of them, again and
/// again: through its formula (`Width`, `Sheet.4!PinX`), through a function that carries a point between frames (`PAR(PNT(Sheet.4!Connections.X1,
/// ...))`, which is how a connector follows its shape), through a `Connect` row, or through the master it inherits a formula from, **or through another page** (`Pages[Page-1]!Sheet.10!Width`:
/// the commit goes on to the cells of the pages that name this one, and to what reads those, on whatever page). A cell
/// elsewhere on the page keeps the value the file saved, whatever this evaluator would give for it; a cell that is recalculated is written only
/// where the result differs from what it holds.
///
/// **What it costs.** The cells that read a changed cell are found from the formulas of the page, once for each formula, and the cells
/// worked out are those. `tests/it/edit_cost.rs` measures one move on a page of 500 shapes and 300 connectors. The writes themselves go
/// through `write_batch` in chunks, one lookup build for each chunk.
pub fn settle(doc: &mut Document, before: &Document, page: u32, touched: &[u32], extra: &[u32]) -> Result<(), Problem> {
    settle_forced(doc, before, page, touched, extra, &[])
}

/// [`settle`] with connectors `forced` to be routed again whatever else holds: a hand edit or a fixed code does not stop them, and nor does an end
/// that did not move (the Connectors menu, Reset Connector).
pub fn settle_forced(doc: &mut Document, before: &Document, page: u32, touched: &[u32], extra: &[u32], forced: &[u32]) -> Result<(), Problem> {
    #[cfg(test)]
    SETTLES.with(|c| c.set(c.get().saturating_add(1)));
    require_page(doc, page)?;
    // A shape whose pin the edit wrote, and so no longer reads the cells that held it to a guide, is released first (`snap::hold`):
    // the cells the release writes are worked out with the rest of the edit.
    let changes = changes_between(before, doc, page);
    // The shapes a guide the edit moved or turned carries are changed by it as much as the guide is: the connectors glued to them
    // are placed and routed again.
    let mut touched: Vec<u32> = touched.iter().copied().chain(crate::snap::hold::carried_by_guides(doc, page, &changes.cells)).collect();
    let released = crate::snap::hold::release_broken(doc, page, &changes.cells)?;
    // The drawing as the edit's own writes left it: what the recalculation changes from here is the edit's doing and no one names it. **An edit that wrote a master, a
    // style, a theme or the document** (`whole_page`: a change that is no cell of the page) moves the shapes that read it by inheritance as it writes, before any recalculation, so
    // those are compared with the drawing as the command found it.
    let written = (!changes.whole_page).then(|| doc.clone());
    if released {
        recalculate(doc, before, page)?;
    } else {
        commit(doc, page, &changes)?;
    }
    // A field in a shape the edit changed shows what the shape is now.
    super::fields::refresh_changed(doc, page, &changes.cells);
    // **A shape the recalculation moved or resized is changed by the edit as much as one the edit named**: a theme or a style gives the text another face, a text
    // grows a compartment, a formula reads a size; the connectors glued to those shapes are routed again with the rest.
    touched.extend(recalculated_shapes(written.as_ref().unwrap_or(before), doc, page));
    route_after(doc, before, page, &touched, extra, forced)
}

/// **The shapes of `page` that a recalculation moved or resized**: the shapes that are not connectors and whose pose, size or outline cells
/// hold another value in `doc` than in `was`, the drawing before the recalculation. A command names the shapes it moves (`touched`); a recalculation names none, and the shape a
/// formula or a text measure resized has no one to say so, so what the two documents hold is compared, which costs what the recalculation wrote (`changes_between` skips what is the same
/// allocation) **and nothing on a page that holds no glue**.
///
/// A cell counts only when its value is not the one it held (within [`SAME_VALUE`]): a shape that was given a link to a style, or a formula worked out to the number it had, is listed
/// by the comparison of the two documents and has not moved, and a connector routed again for it would lose the place of its label for no change. **A connector is
/// left out**: it follows the shapes it is glued to by its formulas and is routed because a shape it follows moved, and one the recalculation changed is not a connector
/// the edit is about (`touched` makes a connector a subject, which a route held by `DynamicsOff` or `ConFixedCode` does not stop). The shapes are where `was` has them as `doc` does (a
/// recalculation adds and takes out none); one that is not is listed. A page too large to compare shape by shape gives none: its connectors keep the routes they have.
///
/// **An edit of a master, a style, a theme or the document** (the two drawings differ outside the page's own sheet and shapes, which `changes_between` calls the whole page) moves the
/// shapes that read it by what they inherit, which no cell of the shape records: an instance of a master whose width was written has no cell of its own to hold it. There the shapes
/// that a connector's moved end is glued to are listed too ([`shapes_glued_to_moved_ends`]): a cell of an end follows its shape by a formula and is what the recalculation did to it.
pub(crate) fn recalculated_shapes(was: &Document, doc: &Document, page: u32) -> Vec<u32> {
    if doc.page(page).is_none_or(|p| p.connects.is_empty()) {
        return Vec::new();
    }
    let plain = changes_between(was, doc, page);
    let whole = plain.whole_page;
    let changes = if whole { changes_between(&comparison_base(was, doc), doc, page) } else { plain };
    let mut moved = if changes.whole_page { Vec::new() } else { shapes_with_moved_cells(was, doc, &changes) };
    if whole {
        moved.extend(shapes_glued_to_moved_ends(was, doc, page));
    }
    moved.sort_unstable();
    moved.dedup();
    moved
}

/// The shapes among `changes` (the cells that differ between `was` and `doc`) that are not connectors and hold another value in a cell that moves or reshapes them.
fn shapes_with_moved_cells(was: &Document, doc: &Document, changes: &Changes) -> Vec<u32> {
    let (old, new) = (Resolver::new(was), Resolver::new(doc));
    let reader = Reader::new(doc);
    // The shapes known not to be connectors, and those that need no more cells looked at: connectors, and shapes already listed.
    let mut plain: HashSet<u32> = HashSet::new();
    let mut done: HashSet<u32> = HashSet::new();
    let mut moved: Vec<u32> = Vec::new();
    for (sheet, addr) in changes.cells.iter().filter(|(_, addr)| moves_shape(addr)) {
        let SheetRef::Shape(at) = sheet else { continue };
        let Some(shape) = doc.shape(at).filter(|_| matches!(at.owner, Owner::Page(_))) else { continue };
        if done.contains(&shape.id) {
            continue;
        }
        if !plain.contains(&shape.id) {
            if reader.is_one_d(sheet) {
                done.insert(shape.id);
                continue;
            }
            plain.insert(shape.id);
        }
        let there = was.shape(at).is_some_and(|old_shape| old_shape.id == shape.id);
        if !there || !same_value(was, doc, &old, &new, sheet, addr) {
            done.insert(shape.id);
            moved.push(shape.id);
        }
    }
    moved
}

/// The shapes that a glued end of a connector of `page` has moved from: a connector is looked up in `was` and in `doc` (two walks of the page for all of them, never one for each), and
/// the shape its `Connect` row on an end names is listed when either of the two cells of that end holds another place now (within [`SAME_VALUE`]). A connector that is not in both is
/// left out (a new one is routed by the command that made it).
fn shapes_glued_to_moved_ends(was: &Document, doc: &Document, page: u32) -> Vec<u32> {
    let Some(p) = doc.page(page) else { return Vec::new() };
    let ends = |from: &str| match from {
        "BeginX" | "BeginY" => Some(["BeginX", "BeginY"]),
        "EndX" | "EndY" => Some(["EndX", "EndY"]),
        _ => None,
    };
    let glued: Vec<(u32, [&str; 2], u32)> =
        p.connects.iter().filter_map(|c| Some((c.from_sheet, ends(c.from_cell.as_deref()?)?, c.to_sheet))).collect();
    if glued.is_empty() {
        return Vec::new();
    }
    let wires: HashSet<u32> = glued.iter().map(|(wire, _, _)| *wire).collect();
    let (Ok(new_at), Ok(old_at)) = (super::scan::find_named(doc, page, &wires), super::scan::find_named(was, page, &wires)) else {
        return Vec::new();
    };
    let (old, new) = (Reader::new(was), Reader::new(doc));
    let mut moved: Vec<u32> = Vec::new();
    for (wire, cells, to) in glued {
        let (Some(now), Some(then)) = (new_at.get(&wire), old_at.get(&wire)) else { continue };
        let (now, then) = (SheetRef::Shape(now.clone()), SheetRef::Shape(then.clone()));
        if cells.iter().any(|name| (new.number(&now, name) - old.number(&then, name)).abs() > SAME_VALUE) {
            moved.push(to);
        }
    }
    moved
}

/// How far two numbers of a cell may be and still be the same value: a recalculation gives a size its last digits again.
const SAME_VALUE: f64 = 1.0e-9;

/// Whether the cell at `addr` of `sheet` holds the same value in the two documents (numbers within [`SAME_VALUE`], other values as text).
fn same_value(was: &Document, doc: &Document, old: &Resolver<'_>, new: &Resolver<'_>, sheet: &SheetRef, addr: &CellAddr) -> bool {
    match (value_in(was, old, sheet, addr), value_in(doc, new, sheet, addr)) {
        (None, None) => true,
        (Some(a), Some(b)) => match (a.number(), b.number()) {
            (Some(x), Some(y)) => (x - y).abs() <= SAME_VALUE,
            _ => a == b,
        },
        _ => false,
    }
}

/// The value the cell at `addr` of `sheet` holds in `doc`: the sheet's own, found by one lookup (a page of thousands of shapes that were each given a link to a style holds its cells as
/// it did, and is compared without a walk up the master and the styles for each), and else what the cell inherits.
fn value_in(doc: &Document, res: &Resolver<'_>, sheet: &SheetRef, addr: &CellAddr) -> Option<Value> {
    if let Some(own) = doc.sheet_of(sheet)
        && let Lookup::Found(cell) = own.lookup(addr)
        && let Some(value) = &cell.value
    {
        return Some(value.clone());
    }
    res.resolve(sheet, addr).and_then(|found| found.value.cell.and_then(|cell| cell.value.clone()))
}

/// **After a recalculation that a caller worked out itself** (a theme, a format, a text typed): the connectors glued to the shapes it moved or resized ([`recalculated_shapes`]) are
/// routed again, as they are after a command that moves or resizes the shape. `was` is the drawing before the recalculation.
pub(crate) fn route_recalculated(doc: &mut Document, was: &Document, page: u32) -> Result<(), Problem> {
    let moved = recalculated_shapes(was, doc, page);
    if moved.is_empty() {
        return Ok(());
    }
    route_after(doc, was, page, &moved, &[], &[])
}

/// [`route_recalculated`] for every page that the edit changed (a refresh of the fields of many pages, which works the text of each out and writes it): `was` is the drawing before the
/// edit, and the pages are found by pointer, so a page the edit did not write costs nothing. **The pages share one budget of the evaluator's steps** (`Budget::DEFAULT_STEPS`, the one a
/// page has), as the pages one commit reaches do ([`route_reached_within`]): a drawing of many pages that each cost just under it was routed in as many budgets. Once it is spent the edit is
/// refused, as a recalculation that passes its budget is.
pub(crate) fn route_recalculated_pages(doc: &mut Document, was: &Document) -> Result<(), Problem> {
    let _call = CallScope::enter();
    let mut steps = Budget::DEFAULT_STEPS;
    note_granted(steps);
    let pages = changed_pages(was, doc);
    route_recalculated_on_within(doc, was, &pages, &mut steps)
}

/// [`route_recalculated_within`] for each of `pages`, **one after the other within the same `steps`**: each takes its own off what is left, so that the pages together pass what one page
/// may and not what each of them may. **The commits the routings make draw on the same budget** ([`crate::calc_index::SharedSteps`]: a routing that runs out of steps is no error of its own, it
/// leaves the routes as they were, so the steps it took are taken off by the commit and not by the routing), and a call that has spent them is refused in the words of a recalculation that
/// passes its budget: at the next page that has a shape to route, or at the end of the last.
pub(crate) fn route_recalculated_on_within(doc: &mut Document, was: &Document, pages: &[u32], steps: &mut u64) -> Result<(), Problem> {
    let _shared = crate::calc_index::SharedSteps::enter(*steps);
    // The cells the readers of the pages work out (an end that holds a formula and no saved value) draw on one budget for the call too, and not the 8 million each reader has.
    let _worked = super::worked::WorkedBudget::new(super::worked::WORKED_STEPS).enter();
    let mut routed = false;
    for page in pages {
        routed |= route_recalculated_within(doc, was, *page, steps)?;
    }
    if routed && (*steps == 0 || crate::calc_index::shared_steps_left() == Some(0)) {
        return Err(Problem::failed("the page is too large to recalculate"));
    }
    Ok(())
}

/// [`route_recalculated`] within the `steps` of the evaluator a caller shares among its work: the routing takes its own off, and with none left to start with the edit is refused as a
/// recalculation that passes its budget is. → whether a shape was routed (a page that moved none costs nothing and is no refusal).
pub(crate) fn route_recalculated_within(doc: &mut Document, was: &Document, page: u32, steps: &mut u64) -> Result<bool, Problem> {
    let moved = recalculated_shapes(was, doc, page);
    if moved.is_empty() {
        return Ok(false);
    }
    if *steps == 0 || crate::calc_index::shared_steps_left() == Some(0) {
        return Err(Problem::failed("the page is too large to recalculate"));
    }
    route_after_sharing(doc, was, page, &moved, &[], &[], steps)?;
    Ok(true)
}

/// The second half of [`settle_forced`], for a caller that has worked the cells out itself: the connectors a command moved (those with an end glued to a shape in
/// `touched`, or to one inside it, or named in `extra` or `forced`) are placed where calc cannot place them and **routed again**, and the jumps of those and of the
/// connectors they cross are made again. `before` is the document as it was before the change that moved them (`ends_moved` compares the ends with it). A refit
/// of a group calls this for the group whose frame moved a connection point a connector is glued to.
pub(super) fn route_after(doc: &mut Document, before: &Document, page: u32, touched: &[u32], extra: &[u32], forced: &[u32]) -> Result<(), Problem> {
    let _call = CallScope::enter();
    let mut steps = Budget::DEFAULT_STEPS;
    note_granted(steps);
    route_after_sharing(doc, before, page, touched, extra, forced, &mut steps)
}

/// [`route_after`] within `steps` of the evaluator, which it takes its own off: a caller that routes on many pages (`route_reached_within`) gives them **one budget**, so that the pages together
/// pass what one page may and not what each of them may.
fn route_after_sharing(
    doc: &mut Document,
    before: &Document,
    page: u32,
    touched: &[u32],
    extra: &[u32],
    forced: &[u32],
    steps: &mut u64,
) -> Result<(), Problem> {
    let limit = EDIT.with(|edit| edit.borrow().as_ref().map(|edit| edit.tries)).unwrap_or(MAX_ROUTED_PER_EDIT);
    let rerouted = route_after_within(doc, before, page, touched, extra, forced, Routing { steps, limit, strict: false })?;
    EDIT.with(|edit| {
        if let Some(edit) = edit.borrow_mut().as_mut() {
            edit.tries = edit.tries.saturating_sub(rerouted.attempted);
            for id in rerouted.left {
                if edit.left.len() < MAX_LEFT_LISTED {
                    edit.left.push(Left::not_routed(page, id));
                } else {
                    edit.unlisted = edit.unlisted.saturating_add(1);
                }
            }
        }
    });
    Ok(())
}

/// **The most connectors one edit tries to route again**, outside the refit of a group, which has its own ([`super::MAX_REROUTED`]): 10,000, in all, however
/// many times the edit routes (a move, a resize, a new connector, the Connectors menu), whatever the pages. A try costs the same whatever comes of it (a route that comes back as
/// none, the test of a fixed route for a crossing), so it is the tries that are counted. Past the most the ends are where the recalculation put them (they follow the shapes
/// they are glued to), the route is the one the connector had, and **the command's answer says the connector** (`groups_left`, `kind` `connector`, as a refit says the
/// connectors it left). The specifications say nothing of what the reference application does with very many connectors, so the number is ours: a routing costs a few
/// microseconds once the glue, the shapes and their points are read once for the edit ([`super::reroute::EndIndex`]), and 10,000 of them are less than the recalculation of their
/// ends costs (the measures are in `tests/it/connect_cost.rs`). A call that no session makes (a library caller's) has the number for each call.
pub const MAX_ROUTED_PER_EDIT: usize = 10_000;

/// **The most commits of reached pages one inside another**: the routing of the pages a commit reached may place a glued end or refresh a field, which is a commit that reaches
/// pages in its turn. A chain of pages that change one another through more than this many commits, one after the other, is refused in words with nothing changed, as a budget that runs out is.
const MAX_REACH_DEPTH: u32 = 8;

thread_local! {
    /// How many routings of reached pages are running one inside another on this thread.
    static REACH_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    /// The shapes whose fields the pages an edit reaches may still have refreshed, **in all, whatever the number of commits one inside another** (the edit's own refresh looks at
    /// [`super::fields::MAX_AUTO_SHAPES`]): set when an edit begins ([`EditRoutes::enter`]) or, for a call outside an edit, when the outermost routing of reached pages does.
    static FIELD_SHAPES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// For as long as it lives, one more routing of reached pages is running: `None` when that is more than [`MAX_REACH_DEPTH`].
struct ReachDepth;

impl ReachDepth {
    fn enter() -> Option<ReachDepth> {
        let depth = REACH_DEPTH.with(std::cell::Cell::get);
        if depth >= MAX_REACH_DEPTH {
            return None;
        }
        REACH_DEPTH.with(|d| d.set(depth.saturating_add(1)));
        Some(ReachDepth)
    }
}

impl Drop for ReachDepth {
    fn drop(&mut self) {
        REACH_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
    }
}

/// What is granted of `want` shapes when `left` are left, and what is left then: never more than asked or than there is, and exactly what is taken off.
fn grant(left: usize, want: usize) -> (usize, usize) {
    let granted = want.min(left);
    (granted, left.saturating_sub(granted))
}

thread_local! {
    /// How many calls that settle an edit are running one inside another on this thread ([`CallScope`]).
    static CALL_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// For as long as it lives, the call that settles an edit is running (`settle`, a commit, the routing of the connectors, a refit): **the outermost one of a call outside an edit gives the shapes
/// whose fields the pages it reaches may refresh their allowance, and none inside it does again** (an allowance given again by each commit inside the call was one for each of them, and not for the
/// call). Inside an edit the allowance is the edit's, given once when it begins ([`EditRoutes::enter`]).
pub(crate) struct CallScope;

impl CallScope {
    pub(crate) fn enter() -> CallScope {
        let depth = CALL_DEPTH.with(std::cell::Cell::get);
        if depth == 0 && EDIT.with(|edit| edit.borrow().is_none()) {
            FIELD_SHAPES.with(|left| left.set(super::fields::MAX_AUTO_SHAPES));
        }
        CALL_DEPTH.with(|d| d.set(depth.saturating_add(1)));
        CallScope
    }
}

impl Drop for CallScope {
    fn drop(&mut self) {
        CALL_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
    }
}

/// Takes up to `want` shapes from what the edit has left to refresh the fields of the pages it reached: → how many it may.
fn take_field_shapes(want: usize) -> usize {
    FIELD_SHAPES.with(|left| {
        let (granted, rest) = grant(left.get(), want);
        left.set(rest);
        granted
    })
}

/// What the routings of one edit share: the tries left of [`MAX_ROUTED_PER_EDIT`], and the connectors left past them, the first [`MAX_LEFT_LISTED`] said and the others counted.
struct EditRouting {
    tries: usize,
    left: Vec<Left>,
    unlisted: usize,
}

thread_local! {
    static EDIT: std::cell::RefCell<Option<EditRouting>> = const { std::cell::RefCell::new(None) };
}

/// For as long as it lives, the routings made on this thread are one edit's: they share [`MAX_ROUTED_PER_EDIT`] tries, and keep what they left for the
/// command's answer. `Session::edit_if` takes it around the command's closure, as it takes the index of the page's readers (`calc_index::InUse`); one taken while another
/// lives leaves the other's in place.
pub(crate) struct EditRoutes {
    entered: bool,
}

impl EditRoutes {
    pub(crate) fn enter() -> EditRoutes {
        EditRoutes::enter_with(MAX_ROUTED_PER_EDIT)
    }

    /// [`EditRoutes::enter`] with `tries` for the edit.
    pub(crate) fn enter_with(tries: usize) -> EditRoutes {
        let entered = EDIT.with(|edit| {
            let mut edit = edit.borrow_mut();
            if edit.is_some() {
                return false;
            }
            *edit = Some(EditRouting { tries, left: Vec::new(), unlisted: 0 });
            true
        });
        if entered {
            FIELD_SHAPES.with(|left| left.set(super::fields::MAX_AUTO_SHAPES));
        }
        EditRoutes { entered }
    }

    /// The connectors the edit's routings left, said, and how many more: taken, so that each is said once.
    pub(crate) fn take_left(&self) -> (Vec<Left>, usize) {
        if !self.entered {
            return (Vec::new(), 0);
        }
        EDIT.with(|edit| {
            edit.borrow_mut().as_mut().map(|edit| (std::mem::take(&mut edit.left), std::mem::take(&mut edit.unlisted))).unwrap_or_default()
        })
    }
}

impl Drop for EditRoutes {
    fn drop(&mut self) {
        if self.entered {
            EDIT.with(|edit| *edit.borrow_mut() = None);
        }
    }
}

/// What a routing may spend: the evaluator's steps left (taken off as they are used), the most connectors it routes, and whether a budget that runs out in placing
/// an end is a refusal.
pub(super) struct Routing<'a> {
    pub steps: &'a mut u64,
    pub limit: usize,
    pub strict: bool,
}

/// What [`route_after_within`] did: the connectors the edit moved that it did not route again because `limit` was reached (a refit of a group says them),
/// and how many connectors it **tried** to route, whatever came of the try: the work is the attempts, and a connector whose route comes back as none costs what one
/// that is routed costs. A fixed route set to reroute as needed or on crossover is tried by the test of whether a leg of it crosses a shape it is glued to
/// And a test that finds none is a try all the same.
pub(super) struct Rerouted {
    pub left: Vec<u32>,
    pub attempted: usize,
}

/// [`route_after`] within what `routing` allows: the recalculations it makes take their steps off, a budget that runs out is the refusal in words, and past
/// `limit` attempts the connectors that are left, which the edit moved and which are not routed again, are returned with the count of the attempts.
pub(super) fn route_after_within(
    doc: &mut Document,
    before: &Document,
    page: u32,
    touched: &[u32],
    extra: &[u32],
    forced: &[u32],
    routing: Routing<'_>,
) -> Result<Rerouted, Problem> {
    let Routing { steps, limit, strict } = routing;
    let _call = CallScope::enter();
    // The shapes changed are found in one walk of the page, and the connectors linked to them and the shapes those are glued to in one more: not a search
    // each, which was a walk of the page each (the re-review: a guide that carries a thousand shapes puts them all here).
    let mut located = Located::default();
    located.find(doc, page, touched.iter().copied())?;
    let (reach, inside) = reach_of(doc, &located, touched);
    // The connectors are those with a row naming a shape in `reach` or with one of their own in it (what `glue::place_ends` and `connectors_to_route`
    // look at), and the shapes are what any row of those names: the far end of a connector too, which its placement reads.
    let linked: Vec<u32> = doc
        .page(page)
        .map(|p| {
            let connectors: HashSet<u32> = p
                .connects
                .iter()
                .filter(|c| reach.contains(&c.to_sheet) || reach.contains(&c.from_sheet) || inside.contains(&c.from_sheet))
                .map(|c| c.from_sheet)
                .collect();
            p.connects.iter().filter(|c| connectors.contains(&c.from_sheet)).flat_map(|c| [c.from_sheet, c.to_sheet]).collect()
        })
        .unwrap_or_default();
    located.find(doc, page, linked.into_iter().chain(extra.iter().copied()).chain(forced.iter().copied()))?;
    // An end calc cannot work out is put where the engine's rule says, and what reads it is worked out in its turn.
    if glue::place_ends_within(doc, page, &reach, &inside, &mut located, steps, strict)? {
        recalculate_within(doc, before, page, steps)?;
    }
    let to_route = connectors_to_route(doc, before, page, &reach, touched, extra, forced, &mut located)?;
    // The limit is on the tries: a connector whose route comes back as none cost what one that is routed does, and so does the test of a fixed route for a crossing
    // (a test is not made outside the limit).
    let (tried, past) = to_route.split_at(to_route.len().min(limit));
    let left: Vec<u32> = past.iter().map(|next| next.id).collect();
    let attempted = tried.len();
    note_attempted(attempted);
    // The glue of the ends of every connector tried is read in one pass over the page's `Connect` rows, and the shapes it names are found with the others (
    // a routing searched the page for the connector and for the shape at each end, and passed over the page's `Connect` rows for each end).
    let ids: HashSet<u32> = tried.iter().map(|next| next.id).collect();
    let mut index = EndIndex::new(doc, page, &ids, &mut located)?;
    // The tests of the fixed routes set to reroute as needed or on crossover, the routes and the labels are worked out on one reading of the page as it is before any
    // connector is routed again here (a route reads its connector, the page's cells and the shapes its ends are glued to, which no other route or label writes), and
    // then written.
    let (mut plans, labels) = {
        let reader = Reader::new(doc);
        let mut plans = Vec::new();
        let mut labels = Vec::new();
        // The document as the command found it: a fixed route whose ends moved is followed from the route it had there.
        let old = Reader::new(before);
        for next in tried {
            let Some(at) = located.get(&next.id) else { continue };
            let planned = match index.fate(&reader, &located, &old, next) {
                Fate::Stays => continue,
                Fate::Square(corners) => plan_squared(&reader, next.id, at, corners),
                // A connector routed again because a leg crossed a shape it is glued to keeps the setting that asked for it.
                Fate::Route => plan_route(&reader, page, next.id, at, next.check, &mut index, &located)?,
            };
            if let Some(plan) = planned {
                labels.extend(label::writes_for(&reader, &plan.at, &plan.routed, plan.routed.ours));
                plans.push(plan);
            }
        }
        (plans, labels)
    };
    let routable_cells = write_planned(doc, page, &mut plans)?;
    let mut label_cells: Vec<(SheetRef, CellAddr)> = labels.iter().map(|(sheet, write)| (sheet.clone(), write.addr.clone())).collect();
    label::write(doc, labels)?;
    // The cells that made a connector of ours one the VSDX application routes are worked out with the labels: a formula may read them.
    label_cells.extend(routable_cells);
    let routed_ids: Vec<u32> = plans.iter().map(|plan| plan.id).collect();
    note_routed(routed_ids.len());
    // The jumps of the connectors routed and of those they cross.
    if !routed_ids.is_empty() {
        jumps::refresh(doc, before, page, &routed_ids, false)?;
        // The routes are written after the recalculation of the edit, so what reads one (a shape tied to it by `POINTALONGPATH`, a text that shows its length) is worked out again now,
        // with what reads the label cells that were written: the drawing holds what a full evaluation gives, and the shape follows the route in the edit that moved it.
        // Past the steps the edit has left it is no refusal of the edit (a refit of a group, which is strict, refuses): what reads a route holds what it held, as it did before it was worked out here.
        match recalculate_routes(doc, page, &plans, label_cells, steps) {
            Err(problem) if strict => return Err(problem),
            Ok(()) | Err(_) => {}
        }
    }
    Ok(Rerouted { left, attempted })
}

/// The cells that read the routes of `plans` (any cell of each connector's first `Geometry` section, which was written whole: the cells that read the whole path and those that read one
/// row's `X`, the rows the route gained and the ones it lost) and the cells in `cells` (the label cells that were written with them), and what reads those, worked out within the
/// `steps` left, which it takes its own off.
fn recalculate_routes(doc: &mut Document, page: u32, plans: &[Planned], cells: Vec<(SheetRef, CellAddr)>, steps: &mut u64) -> Result<(), Problem> {
    let sections = plans.iter().map(|plan| (SheetRef::Shape(plan.at.clone()), SectionKey::new("Geometry", Some(0)))).collect();
    // Nothing may read them: a page that no cell reads a route on is not worked out, and the edit made the one commit it made.
    let changes = Changes { cells, sections, only_if_read: true, ..Changes::default() };
    let used = commit_within(doc, page, &changes, *steps)?;
    *steps = steps.saturating_sub(used);
    Ok(())
}

// How many connectors a call routed, for the tests that hold a refit to a count. Not kept outside tests.
#[cfg(test)]
thread_local! {
    pub(crate) static ROUTED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

// The steps of the evaluator the routings of this thread were handed in all: one budget for each place that makes one, for the tests that hold the pages one commit reaches to a total.
#[cfg(test)]
thread_local! {
    pub(crate) static GRANTED: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
fn note_granted(steps: u64) {
    GRANTED.with(|c| c.set(c.get().saturating_add(steps)));
}

#[cfg(not(test))]
fn note_granted(_: u64) {}

// How many connectors a call tried to route (a route that comes back as none is an attempt), for the tests that hold a refit to a count.
#[cfg(test)]
thread_local! {
    pub(crate) static ATTEMPTED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
fn note_attempted(n: usize) {
    ATTEMPTED.with(|c| c.set(c.get().saturating_add(n)));
}

#[cfg(not(test))]
fn note_attempted(_: usize) {}

#[cfg(test)]
fn note_routed(n: usize) {
    ROUTED.with(|c| c.set(c.get().saturating_add(n)));
}

#[cfg(not(test))]
fn note_routed(_: usize) {}

/// After the text of a shape changed: the cells that read it (`SHAPETEXT(TheText)`, `TEXTWIDTH(TheText)`, a reference to `Sheet.4!TheText`) and
/// what reads those are worked out again, and no others (of the M3 review). For this the text is a cell of the shape, `TheText`, so
/// `calc` finds its readers as it finds those of any cell. Nothing is compared with an earlier document: the text is all that changed, and a
/// page that `changes_between` would work out whole (a tree too large to index) is not worked out whole for it.
pub(super) fn settle_text(doc: &mut Document, page: u32, at: &ShapeRef) -> Result<(), Problem> {
    require_page(doc, page)?;
    let changes = Changes { cells: vec![(SheetRef::Shape(at.clone()), CellAddr::direct("TheText"))], ..Changes::default() };
    commit(doc, page, &changes)
}

/// After a command changed the pages themselves (a page added, copied, renamed, deleted, moved, made a background): the cells whose formulas name a page that the change gave another
/// reading, or read the list of pages (`PAGENAME()`, `PAGECOUNT()`), are worked out in one commit, in the budget of the evaluator, and **the connectors glued to the shapes of every page
/// they moved are routed again** (gap F40). `before` is the document as the command found it: what the change gave another reading is found by comparing the lists of pages
/// ([`PageNames::between`]), which costs a comparison by pointer of each page for a change that gave nothing another reading. A commit the budget does not cover refuses the command, in
/// words, with nothing changed.
pub fn settle_pages(doc: &mut Document, before: &Document) -> Result<(), Problem> {
    let names = PageNames::between(before, doc);
    if names.is_empty() {
        return Ok(());
    }
    // The page of the commit is only the one its graph is made for: the readers are found by what the listing says, and the commit goes to the page of the first (`calc::commit_changed_across`).
    let Some(first) = doc.pages.iter().next().map(|p| p.id) else { return Ok(()) };
    let changes = Changes { page_names: names, ..Changes::default() };
    commit_routing(doc, first, &changes, None).map_err(|problem| {
        if problem.bad_params || !problem.message.contains("too large to recalculate") {
            return problem;
        }
        Problem::failed(format!(
            "the cells that read pages are too large to work out again after this change to the pages (they share {} steps of the evaluator): nothing was changed",
            Budget::DEFAULT_STEPS
        ))
    })
}

/// [`settle_text`] within `steps` of the evaluator, **the routing of the pages it reached included**: → the steps it took. A budget that runs out is the refusal [`commit_within`] gives. The fields of a
/// reached page are refreshed by this, so that what their texts' commits take comes off the budget the routing of the pages the first commit reached shares.
pub(super) fn settle_text_within(doc: &mut Document, page: u32, at: &ShapeRef, steps: u64) -> Result<u64, Problem> {
    require_page(doc, page)?;
    let changes = Changes { cells: vec![(SheetRef::Shape(at.clone()), CellAddr::direct("TheText"))], ..Changes::default() };
    commit_within(doc, page, &changes, steps)
}

/// After cells were written by an edit that moved nothing (a replacement of texts, names and the values of shape data): the cells in `cells` and what reads them are
/// worked out again, and no others, in one commit for the page. A text is a cell of its shape, `TheText`, so its readers are found as any cell's are. **The connectors glued to the shapes
/// that recalculation moved or resized are routed again** ([`route_recalculated`]): the edit moved nothing itself, but a cell a formula reads can.
pub(super) fn settle_cells(doc: &mut Document, page: u32, cells: Vec<(SheetRef, CellAddr)>) -> Result<(), Problem> {
    let written = doc.clone();
    if !work_out_cells(doc, page, cells)? {
        return Ok(());
    }
    // A text or a value that a formula reads may resize the shape that reads it: the connectors glued to it follow.
    route_recalculated(doc, &written, page)
}

/// [`settle_cells`] for the cells of several pages (a replacement over a drawing): each page's cells are worked out in a commit of its own, and the connectors glued to the shapes that
/// any of them moved are routed again **within one budget of steps for the call** ([`route_recalculated_pages`]), not a budget for each page. `was` is the drawing before the call's writes.
pub(super) fn settle_cells_on_pages(doc: &mut Document, was: &Document, pages: Vec<(u32, Vec<(SheetRef, CellAddr)>)>) -> Result<(), Problem> {
    for (page, cells) in pages {
        work_out_cells(doc, page, cells)?;
    }
    route_recalculated_pages(doc, was)
}

/// The commit of `cells` and what reads them on `page`, and the refresh of the fields they changed: → whether there was a cell to work out.
fn work_out_cells(doc: &mut Document, page: u32, cells: Vec<(SheetRef, CellAddr)>) -> Result<bool, Problem> {
    require_page(doc, page)?;
    if cells.is_empty() {
        return Ok(false);
    }
    let changes = Changes { cells, ..Changes::default() };
    commit(doc, page, &changes)?;
    super::fields::refresh_changed(doc, page, &changes.cells);
    Ok(true)
}

/// After an edit that changed where shapes are kept and not where they draw (group, ungroup, paste): the cells in `changes` are worked out
/// with what reads them. **No end is placed by the engine's rule and no route is made again.** The shapes are carried together, each cell that
/// holds a place carried with them (a glued end's cached value too, [`super::lift`]), so the ends are where they were; a route is rows in the
/// connector's own frame, which does not change, so the rows a connector has are the route it keeps (a reroute would replace the route of a
/// connector of a file with ours, and a placement the place the file's own application chose).
///
/// `changes` is made by the caller and not by `changes_between`: a shape that is moved to another list is new to that comparison, and every cell
/// of it counts as changed, which works out cells nobody edited (a file's saved value would be replaced by this evaluator's where they differ).
/// The caller names the cells it wrote, the pose cells of the shapes whose parent changed, and the cells of those that carry a point
/// between frames.
pub(super) fn settle_kept(doc: &mut Document, page: u32, changes: &Changes) -> Result<(), Problem> {
    require_page(doc, page)?;
    // A shape carried to another frame has its pin written: a hold to a guide it had is broken. The cells the release writes hold
    // the values they had, so nothing reads them differently.
    crate::snap::hold::release_broken(doc, page, &changes.cells)?;
    commit(doc, page, changes)
}

/// [`settle_kept`] within `steps` of the evaluator's budget (the memory is the default): the caller shares one budget among many commits and takes from it what
/// each used. → the steps this one took. A budget that runs out is the refusal [`commit`] gives.
pub(super) fn settle_kept_within(doc: &mut Document, page: u32, changes: &Changes, steps: u64) -> Result<u64, Problem> {
    require_page(doc, page)?;
    crate::snap::hold::release_broken(doc, page, changes.cells.as_slice())?;
    commit_within(doc, page, changes, steps)
}

/// After a route was written outside the routing of an edit (a leg moved by hand, the hops of the jump pass): the cells that read any cell of the first `Geometry` section of each of
/// `connectors` and the cells in `cells` (a label's, written with a route), and what reads those, are worked out, and no others (`Changes::only_if_read`). The same work
/// [`route_after_within`] does for the routes it writes, in the budget a commit has when no edit shares one; a page that nothing reads a route on makes no commit. A budget that runs
/// out is the refusal [`commit`] gives.
pub(super) fn recalculate_written(doc: &mut Document, page: u32, connectors: &[ShapeRef], cells: Vec<(SheetRef, CellAddr)>) -> Result<(), Problem> {
    let sections = connectors.iter().map(|at| (SheetRef::Shape(at.clone()), SectionKey::new("Geometry", Some(0)))).collect();
    recalculate_sections(doc, page, sections, cells)
}

/// [`recalculate_written`] for the sections it is told were written whole, which are not only the first `Geometry` section of a shape: Reverse Ends writes every section of a shape's path.
pub(super) fn recalculate_sections(
    doc: &mut Document,
    page: u32,
    sections: Vec<(SheetRef, SectionKey)>,
    cells: Vec<(SheetRef, CellAddr)>,
) -> Result<(), Problem> {
    commit(doc, page, &Changes { cells, sections, only_if_read: true, ..Changes::default() })
}

/// The commit of `changes` within `steps`: → the steps it took, **the routing of the connectors of the pages it reached included** (the commits a refit of a group and the placing of a
/// glued end make reached the pages that read what they wrote, and left their connectors where they were), which takes its steps from the same budget.
fn commit_within(doc: &mut Document, page: u32, changes: &Changes, steps: u64) -> Result<u64, Problem> {
    let _call = CallScope::enter();
    // A drawing of one page has no other page for a commit to reach, and keeps no copy of the document for it.
    let was = (doc.pages.len() >= 2).then(|| doc.clone());
    let budget = Budget { steps, bytes: Budget::DEFAULT_BYTES };
    let report = commit_changed(doc, &budget, crate::measure::eval_options(), page, changes);
    if report.budget_exhausted || report.memory_exhausted {
        return Err(Problem::failed("the page is too large to recalculate"));
    }
    let mut used = report.stats.steps;
    if let Some(was) = was {
        let mut left = steps.saturating_sub(used);
        route_reached_within(doc, &was, Some(page), &mut left)?;
        used = steps.saturating_sub(left);
    }
    Ok(used)
}

/// The recalculation of what the edit changed, compared with `before`.
fn recalculate(doc: &mut Document, before: &Document, page: u32) -> Result<(), Problem> {
    let changes = changes_between(before, doc, page);
    commit(doc, page, &changes)
}

/// [`recalculate`] within the `steps` left, which it takes its own off.
fn recalculate_within(doc: &mut Document, before: &Document, page: u32, steps: &mut u64) -> Result<(), Problem> {
    let changes = changes_between(before, doc, page);
    let used = commit_within(doc, page, &changes, *steps)?;
    *steps = steps.saturating_sub(used);
    Ok(())
}

/// The recalculation of the cells `changes` names and what reads them, **and the routing of the connectors glued to the shapes of the other pages that it moved**: this is the settle step
/// every command that changes a cell goes through, so a command that moves, resizes, formats or edits the data of a shape reaches the pages that read it (`Pages[..]`) and routes what is glued
/// there as it routes what is glued on its own page.
pub(crate) fn commit(doc: &mut Document, page: u32, changes: &Changes) -> Result<(), Problem> {
    commit_routing(doc, page, changes, Some(page))
}

/// [`commit`] that leaves the routes of the page `own` to the caller (`None`: of no page): the connectors of every other page the commit worked a cell out on are routed again.
fn commit_routing(doc: &mut Document, page: u32, changes: &Changes, own: Option<u32>) -> Result<(), Problem> {
    let _call = CallScope::enter();
    // A drawing of one page has no other page for a commit to reach, and keeps no copy of the document for it.
    let was = (doc.pages.len() >= 2).then(|| doc.clone());
    let report = commit_changed(doc, &Budget::default(), crate::measure::eval_options(), page, changes);
    if report.budget_exhausted || report.memory_exhausted {
        return Err(Problem::failed("the page is too large to recalculate"));
    }
    match was {
        Some(was) => {
            let mut steps = Budget::DEFAULT_STEPS;
            note_granted(steps);
            route_reached_within(doc, &was, own, &mut steps)
        }
        None => Ok(()),
    }
}

/// [`commit`] for a commit that has no page of its own to leave to its caller (the cells of the sheets no page holds, `unpaged::work_out`): the connectors of **every** page it reached are routed again.
pub(super) fn commit_reaching_all(doc: &mut Document, page: u32, changes: &Changes) -> Result<(), Problem> {
    commit_routing(doc, page, changes, None)
}

/// After a commit: the connectors glued to a shape of a page other than `except` whose pose, size or outline cells the commit worked out are routed again, as after a change made to the
/// shape itself, and **the fields in the text of the shapes of those pages whose cells the commit worked out are refreshed**, as those of the edited page are.
/// `was` is the document before the commit. The pages are found by pointer (a page the commit did not write is the same allocation), and a page is compared with its own copy before, so the cost
/// is what the commit wrote. **The pages share one budget of the evaluator's steps, `steps`** (a page of its own is `Budget::DEFAULT_STEPS`, and the pages a commit reached together are no more),
/// each routing and each refresh of fields taking off what it used; once it is spent the commit is refused, as a recalculation that passes its budget is, and the tries are the edit's, shared with
/// every other routing ([`MAX_ROUTED_PER_EDIT`]), as are the shapes whose fields are refreshed ([`super::fields::MAX_AUTO_SHAPES`] in all). **A routing or a refresh that is itself a commit that
/// reaches pages** (a glued end placed, the text of a field worked out) routes the pages it reaches in its turn, to at most [`MAX_REACH_DEPTH`] deep.
fn route_reached_within(doc: &mut Document, was: &Document, except: Option<u32>, steps: &mut u64) -> Result<(), Problem> {
    let reached: Vec<u32> = changed_pages(was, doc).into_iter().filter(|id| Some(*id) != except).collect();
    if reached.is_empty() {
        return Ok(());
    }
    let Some(_depth) = ReachDepth::enter() else {
        return Err(Problem::failed(format!(
            "the pages this edit reaches change one another through more than {MAX_REACH_DEPTH} recalculations one after the other, and the edit is too deep to finish: nothing was changed"
        )));
    };
    let base = comparison_base(was, doc);
    for id in reached {
        let changes = changes_between(&base, doc, id);
        // A field shows what the cell it reads is now: the shapes of the page whose cells the commit worked out.
        super::fields::refresh_reached(doc, id, &changes.cells, take_field_shapes, steps)?;
        let mut moved: Vec<u32> = changes
            .cells
            .iter()
            .filter(|(_, addr)| moves_shape(addr))
            .filter_map(|(sheet, _)| match sheet {
                SheetRef::Shape(at) if matches!(at.owner, Owner::Page(_)) => doc.shape(at).map(|shape| shape.id),
                _ => None,
            })
            .collect();
        moved.sort_unstable();
        moved.dedup();
        if !moved.is_empty() {
            if *steps == 0 {
                return Err(Problem::failed("the page is too large to recalculate"));
            }
            route_after_sharing(doc, was, id, &moved, &[], &[], steps)?;
        }
    }
    Ok(())
}

/// The `ID`s of the shapes in `touched` and of everything inside them (a connector glued to a member of a group that moved has followed it), and
/// of the shapes strictly inside them: the members, not the shapes the command named (round 1: a connector the command changes itself is not
/// a connector that is inside a shape it changed). `paths` holds where each of `touched` is (one walk of the page, `scan`): the first shape with its `ID`,
/// as `find_shape` gives it.
fn reach_of(doc: &Document, paths: &Located, touched: &[u32]) -> (BTreeSet<u32>, BTreeSet<u32>) {
    let mut out: BTreeSet<u32> = touched.iter().copied().collect();
    let mut inside: BTreeSet<u32> = BTreeSet::new();
    for id in touched {
        if let Some(shape) = paths.get(id).and_then(|at| doc.shape(at)) {
            let ids = subtree_ids(shape);
            out.extend(ids.iter().copied());
            // The first is the shape itself.
            inside.extend(ids.into_iter().skip(1));
        }
    }
    (out, inside)
}

/// Whether a cell of the shape holds an error from its last evaluation: a glue formula that did not work out.
pub(crate) fn has_error(reader: &Reader<'_>, sheet: &diacraft_doc::SheetRef, name: &str) -> bool {
    reader.resolver().resolve(sheet, &diacraft_doc::CellAddr::direct(name)).is_some_and(|found| found.value.cell.is_some_and(|c| c.error.is_some()))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use diacraft_doc::Document;

    use super::*;

    /// What is granted never passes what is asked or what is left, and what is taken off is what is granted.
    #[test]
    fn the_shapes_granted_are_the_least_of_what_is_asked_and_what_is_left() {
        for (left, want) in [(0, 0), (0, 5), (5, 0), (5, 3), (3, 5), (256, 256), (usize::MAX, usize::MAX), (usize::MAX, 1), (1, usize::MAX)] {
            let (granted, rest) = grant(left, want);
            assert!(granted <= left && granted <= want, "{left} {want}: {granted}");
            assert_eq!(granted.saturating_add(rest), left, "{left} {want}: what is granted is taken off");
            assert_eq!(granted, left.min(want));
        }
    }

    /// The shapes whose fields the pages an edit reached may refresh are shared by every commit one inside another: what one takes the next does not have.
    #[test]
    fn the_allowance_for_fields_is_one_for_every_commit_inside_an_edit() {
        FIELD_SHAPES.with(|left| left.set(10));
        assert_eq!(take_field_shapes(4), 4);
        assert_eq!(take_field_shapes(4), 4);
        assert_eq!(take_field_shapes(4), 2, "only what is left");
        assert_eq!(take_field_shapes(4), 0);
        FIELD_SHAPES.with(|left| left.set(0));
    }

    /// Page 1 ("One") holds shape 3, whose width the fields read; each of the `pages` pages after it holds a shape 10 with a field whose row reads that width through `Pages[One]!`.
    fn pages_with_fields(pages: u32) -> Document {
        use diacraft_doc::{RowKey, TextPart};
        use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};
        let mut b = DocBuilder::new().page(1, PageBuilder::new().name("One").shape(ShapeBuilder::new(3).cell("Width", "5")));
        for p in 2..=pages + 1 {
            let shape = ShapeBuilder::new(10).text_parts(vec![TextPart::Fld { ix: 0, text: std::sync::Arc::from("5") }]).row_formula(
                "Field",
                None,
                RowKey::Ix(0),
                "Value",
                "Pages[One]!Sheet.3!Width",
                "5",
            );
            b = b.page(p, PageBuilder::new().shape(shape));
        }
        b.build()
    }

    /// The width of shape 3 written as 7, and what that changed on page 1.
    fn written_width(doc: &mut Document) -> Changes {
        use diacraft_doc::{CellEdit, Value, WriteOrigin};
        let at = ShapeRef::top(Owner::Page(1), 0);
        doc.write_cell(&SheetRef::Shape(at.clone()), &CellAddr::direct("Width"), CellEdit::Value(Value::from_text("7")), WriteOrigin::Drawing)
            .unwrap();
        Changes { cells: vec![(SheetRef::Shape(at), CellAddr::direct("Width"))], ..Changes::default() }
    }

    /// The text of the field of shape 10 of page `page`.
    fn field_text(doc: &Document, page: u32) -> String {
        let shape = doc.shape(&ShapeRef::top(Owner::Page(page), 0)).unwrap();
        shape
            .text
            .as_ref()
            .map(|t| {
                t.parts
                    .iter()
                    .map(|p| match p {
                        diacraft_doc::TextPart::Fld { text, .. } => text.to_string(),
                        _ => String::new(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// **The commits that work the text of a reached page's fields out are charged to the budget the routing shares** (the security scan's finding): the steps `commit_within` reports are every
    /// step the commits took, the commit of the write and the one for the text of each field, and not the first commit's alone.
    #[test]
    fn the_commits_of_the_fields_of_reached_pages_are_charged_to_the_budget_of_the_commit() {
        let mut doc = pages_with_fields(40);
        let changes = written_width(&mut doc);
        diacraft_calc::work::reset();
        let used = commit_within(&mut doc, 1, &changes, Budget::DEFAULT_STEPS).unwrap();
        let committed = diacraft_calc::work::steps();
        assert_eq!(diacraft_calc::work::commits(), 41, "the commit of the write and one for the text of each field");
        assert_eq!(field_text(&doc, 41), "7", "the fields of the reached pages are refreshed");
        assert!(used >= committed, "what the call reports ({used}) is at least what its commits took ({committed})");
    }

    /// A budget the commit of the write fits and the fields of forty pages pass: **the call is refused in words, within the budget**, as a routing that passes it is.
    #[test]
    fn a_budget_the_fields_of_reached_pages_pass_is_refused_in_words_and_not_passed() {
        let mut measured = pages_with_fields(40);
        let changes = written_width(&mut measured);
        diacraft_calc::work::reset();
        commit_within(&mut measured, 1, &changes, Budget::DEFAULT_STEPS).unwrap();
        // The steps of all the commits, the first and the forty after it: half of them is more than the first takes and less than all.
        let budget = diacraft_calc::work::steps() / 2;
        let mut doc = pages_with_fields(40);
        written_width(&mut doc);
        diacraft_calc::work::reset();
        let refused = commit_within(&mut doc, 1, &changes, budget).unwrap_err();
        assert!(refused.message.contains("too large to recalculate"), "{}", refused.message);
        assert!(diacraft_calc::work::steps() <= budget, "the commits took {} steps of {budget}", diacraft_calc::work::steps());
    }

    /// The outermost call of a thread outside an edit gives the allowance, and a call inside it does not give it again.
    #[test]
    fn a_call_inside_a_call_does_not_give_the_allowance_for_fields_again() {
        FIELD_SHAPES.with(|left| left.set(0));
        let outer = CallScope::enter();
        assert_eq!(FIELD_SHAPES.with(std::cell::Cell::get), super::super::fields::MAX_AUTO_SHAPES, "the call is given its allowance");
        assert_eq!(take_field_shapes(10), 10);
        {
            let _inner = CallScope::enter();
            let _routing = ReachDepth::enter();
            assert_eq!(FIELD_SHAPES.with(std::cell::Cell::get), super::super::fields::MAX_AUTO_SHAPES - 10, "what the call took is not given back");
        }
        drop(outer);
        let _next = CallScope::enter();
        assert_eq!(FIELD_SHAPES.with(std::cell::Cell::get), super::super::fields::MAX_AUTO_SHAPES, "the next call is given its own");
    }

    /// A routing of reached pages inside another, to [`MAX_REACH_DEPTH`] deep, and no deeper: the next is refused, and one that ends makes room for the next.
    #[test]
    fn the_routings_of_reached_pages_one_inside_another_are_bounded() {
        let mut held: Vec<ReachDepth> = Vec::new();
        for _ in 0..MAX_REACH_DEPTH {
            held.push(ReachDepth::enter().expect("room"));
        }
        assert!(ReachDepth::enter().is_none(), "one more than the most is refused");
        held.pop();
        let again = ReachDepth::enter();
        assert!(again.is_some(), "a routing that ended leaves room");
        drop(again);
        held.clear();
        assert_eq!(REACH_DEPTH.with(std::cell::Cell::get), 0, "every routing that ended has given its place back");
    }

    /// A page of box 10, box 11 and a connector 12 whose begin is glued to box 10 (the shapes are at places 0, 1 and 2 of the page).
    fn glued() -> Document {
        use diacraft_doc::{Connect, Connects};
        use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};
        let boxed =
            |id: u32| ShapeBuilder::new(id).cell("PinX", "1").cell("PinY", "1").cell("Width", "2").cell("Height", "1").cell("FillForegnd", "#ff0000");
        let wire = ShapeBuilder::new(12).cell("BeginX", "1").cell("BeginY", "1").cell("EndX", "5").cell("EndY", "1").cell("Width", "4");
        let mut doc = DocBuilder::new().page(1, PageBuilder::new().shape(boxed(10)).shape(boxed(11)).shape(wire)).build();
        let page = doc.pages.get_mut(0).map(std::sync::Arc::make_mut).unwrap();
        page.connects = Connects::from_vec(vec![Connect {
            from_sheet: 12,
            from_cell: Some(std::sync::Arc::from("BeginX")),
            to_sheet: 10,
            to_cell: Some(std::sync::Arc::from("Connections.X1")),
            ..Connect::default()
        }]);
        doc
    }

    /// Writes cell `name` of the shape at place `place` of page 1.
    fn write(doc: &mut Document, place: u32, name: &str, value: &str) {
        use diacraft_doc::{CellEdit, WriteOrigin};
        let sheet = SheetRef::Shape(ShapeRef::top(Owner::Page(1), place));
        doc.write_cell(&sheet, &CellAddr::direct(name), CellEdit::Value(Value::from_text(value)), WriteOrigin::Drawing).unwrap();
    }

    /// The shapes a recalculation moved are the shapes that are not connectors and hold another value in a cell of their pose, size or outline: a box that was
    /// resized is one, a box that was only recoloured is not, and a connector whose end followed a box is not (it is a follower of the box, and not a shape the edit is about).
    #[test]
    fn the_shapes_a_recalculation_moved_are_the_boxes_whose_pose_or_size_cells_hold_another_value() {
        let was = glued();
        let mut now = was.clone();
        write(&mut now, 0, "Width", "3");
        write(&mut now, 1, "FillForegnd", "#00ff00");
        write(&mut now, 2, "EndX", "7");
        assert_eq!(recalculated_shapes(&was, &now, 1), vec![10]);
        // The pin of the other box: it moved too.
        write(&mut now, 1, "PinY", "4");
        assert_eq!(recalculated_shapes(&was, &now, 1), vec![10, 11]);
    }

    /// A cell that was worked out to the number it held, or to it in other digits, is no move, and nor is a link to a style: a theme links every shape of a page and changes no size, and a
    /// connector routed again for it would lose the place a person gave its label.
    #[test]
    fn a_value_worked_out_to_the_number_it_had_and_a_link_to_a_style_are_no_move() {
        let was = glued();
        let mut now = was.clone();
        write(&mut now, 0, "Width", "2.0000000000001");
        write(&mut now, 1, "Height", "1");
        let linked = crate::draw::shape_mut(&mut now, 1, &[1]).unwrap();
        std::sync::Arc::make_mut(&mut linked.sheet).links.text = Some(5);
        let linked = crate::draw::shape_mut(&mut now, 1, &[0]).unwrap();
        std::sync::Arc::make_mut(&mut linked.sheet).links.line = Some(5);
        assert_eq!(recalculated_shapes(&was, &now, 1), Vec::<u32>::new());
        // A width that differs by more than a billionth is a move.
        write(&mut now, 0, "Width", "2.000001");
        assert_eq!(recalculated_shapes(&was, &now, 1), vec![10]);
    }

    /// A page that holds no glue has no connector to follow anything, and the two drawings are not compared; a page that is not there gives none.
    #[test]
    fn a_page_with_no_glue_and_a_page_that_is_not_there_list_no_shape() {
        let was = glued();
        let mut now = was.clone();
        write(&mut now, 0, "Width", "3");
        assert_eq!(recalculated_shapes(&was, &now, 1), vec![10]);
        let page = now.pages.get_mut(0).map(std::sync::Arc::make_mut).unwrap();
        page.connects = diacraft_doc::Connects::new();
        assert_eq!(recalculated_shapes(&was, &now, 1), Vec::<u32>::new(), "no glue");
        assert_eq!(recalculated_shapes(&was, &now, 9), Vec::<u32>::new(), "no page 9");
    }
}
