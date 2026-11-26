//! After an edit: recalculate what it changed through `calc`, then route again the connectors it moved.

use std::collections::{BTreeSet, HashSet};

use diacraft_calc::{Budget, Changes};
use diacraft_doc::{CellAddr, Document, ShapeRef, SheetRef};

use crate::calc_index::{changes_between, commit_changed};

use super::connectors::{plan_route, plan_squared, write_planned};
use super::glue;
use super::jumps;
use super::label;
use super::read::Reader;
use super::refit::{Left, MAX_LEFT_LISTED};
use super::reroute::{EndIndex, Fate, connectors_to_route};
use super::scan::Located;
use super::shapes::subtree_ids;
use super::{Problem, require_page};

/// Recalculates what the edit changed on page `page`, so that the saved `V` values are right for the renderer and for the writer, then
/// routes again the connectors the edit moved ([`super::reroute`] says which: a connector of ours that follows a shape in `touched` or is named
/// in `touched` or `extra`, and a connector of a file with a glued end that moved, unless its route is fixed). `before` is the document as the
/// command found it, and what differs from it on the page is what the edit changed (`calc::changes_between`).
///
/// **What is recalculated** (`calc::commit_changed`): the cells the edit wrote, and every cell that reads one of them, again and
/// again: through its formula (`Width`, `Sheet.4!PinX`), through a function that carries a point between frames (`PAR(PNT(Sheet.4!Connections.X1,
/// ...))`, which is how a connector follows its shape), through a `Connect` row, or through the master it inherits a formula from. A cell
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
    require_page(doc, page)?;
    // A shape whose pin the edit wrote, and so no longer reads the cells that held it to a guide, is released first (`snap::hold`):
    // the cells the release writes are worked out with the rest of the edit.
    let changes = changes_between(before, doc, page);
    // The shapes a guide the edit moved or turned carries are changed by it as much as the guide is: the connectors glued to them
    // are placed and routed again.
    let touched: Vec<u32> = touched.iter().copied().chain(crate::snap::hold::carried_by_guides(doc, page, &changes.cells)).collect();
    if crate::snap::hold::release_broken(doc, page, &changes.cells)? {
        recalculate(doc, before, page)?;
    } else {
        commit(doc, page, &changes)?;
    }
    // A field in a shape the edit changed shows what the shape is now.
    super::fields::refresh_changed(doc, page, &changes.cells);
    route_after(doc, before, page, &touched, extra, forced)
}

/// The second half of [`settle_forced`], for a caller that has worked the cells out itself: the connectors a command moved (those with an end glued to a shape in
/// `touched`, or to one inside it, or named in `extra` or `forced`) are placed where calc cannot place them and **routed again**, and the jumps of those and of the
/// connectors they cross are made again. `before` is the document as it was before the change that moved them (`ends_moved` compares the ends with it). A refit
/// of a group calls this for the group whose frame moved a connection point a connector is glued to.
pub(super) fn route_after(doc: &mut Document, before: &Document, page: u32, touched: &[u32], extra: &[u32], forced: &[u32]) -> Result<(), Problem> {
    let mut steps = Budget::DEFAULT_STEPS;
    let limit = EDIT.with(|edit| edit.borrow().as_ref().map(|edit| edit.tries)).unwrap_or(MAX_ROUTED_PER_EDIT);
    let rerouted = route_after_within(doc, before, page, touched, extra, forced, Routing { steps: &mut steps, limit, strict: false })?;
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
    write_planned(doc, page, &mut plans)?;
    label::write(doc, labels)?;
    let routed_ids: Vec<u32> = plans.iter().map(|plan| plan.id).collect();
    note_routed(routed_ids.len());
    // The jumps of the connectors routed and of those they cross.
    if !routed_ids.is_empty() {
        jumps::refresh(doc, before, page, &routed_ids, false)?;
    }
    Ok(Rerouted { left, attempted })
}

// How many connectors a call routed, for the tests that hold a refit to a count. Not kept outside tests.
#[cfg(test)]
thread_local! {
    pub(crate) static ROUTED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

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
    let report = commit_changed(doc, &Budget::default(), crate::measure::eval_options(), page, &changes);
    if report.budget_exhausted || report.memory_exhausted {
        return Err(Problem::failed("the page is too large to recalculate"));
    }
    Ok(())
}

/// After cells were written by an edit that moved nothing (a replacement of texts, names and the values of shape data): the cells in `cells` and what reads them are
/// worked out again, and no others, in one commit for the page. A text is a cell of its shape, `TheText`, so its readers are found as any cell's are.
pub(super) fn settle_cells(doc: &mut Document, page: u32, cells: Vec<(SheetRef, CellAddr)>) -> Result<(), Problem> {
    require_page(doc, page)?;
    if cells.is_empty() {
        return Ok(());
    }
    let changes = Changes { cells, ..Changes::default() };
    commit(doc, page, &changes)?;
    super::fields::refresh_changed(doc, page, &changes.cells);
    Ok(())
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

/// The commit of `changes` within `steps`: → the steps it took.
fn commit_within(doc: &mut Document, page: u32, changes: &Changes, steps: u64) -> Result<u64, Problem> {
    let budget = Budget { steps, bytes: Budget::DEFAULT_BYTES };
    let report = commit_changed(doc, &budget, crate::measure::eval_options(), page, changes);
    if report.budget_exhausted || report.memory_exhausted {
        return Err(Problem::failed("the page is too large to recalculate"));
    }
    Ok(report.stats.steps)
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

/// The recalculation of the cells `changes` names and what reads them.
fn commit(doc: &mut Document, page: u32, changes: &Changes) -> Result<(), Problem> {
    let report = commit_changed(doc, &Budget::default(), crate::measure::eval_options(), page, changes);
    if report.budget_exhausted || report.memory_exhausted {
        return Err(Problem::failed("the page is too large to recalculate"));
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
