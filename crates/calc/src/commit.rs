//! Writing a recalculation back: results as cached values, through the document model's `write_batch` under the `Evaluation` origin.
//!
//! **What is written**. A formula cell gets its new result as `V`, with its `U` when the result's kind is one a
//! unit carries (`DATE`, `CY`, `PNT`, `NURBS`, `POLYLINE`, `MULTIDIM`, `STR`, `BOOL`), and `E` when the formula
//! failed, with its formula left exactly as it is. A cell whose result agrees with its saved value is not rewritten: its `V` text stays
//! as the file had it (`1.50` stays `1.50`), and only a stale `E` is cleared, so a second commit changes nothing.
//! A cell an instance does not hold, whose inherited formula gives a result that differs from the master's saved value, is added as `F`
//! `Inh` with that value, in a local row and section made as files do (`COMMIT_ADDS_DIFFERING_INHERITED_CELLS`); where the result equals
//! the source's nothing is added, and a cell already in the file stays. The formula text is never copied down.
//!
//! **What is not written.** A cell whose result is unknown (an unsupported function, a formula that does not parse, an action cell), a
//! cell whose saved value is `Themed` or a date past the year 9999, a cell the chain gives no formula to, a cell whose evaluation met the
//! native depth limit, and every cell once the budget is spent: a spent budget or the depth limit is not a result, so no `E` is put on a
//! cell for it. A real cycle is a result (`rules::LOOP_ERROR`) and is written.
//!
//! **An elliptical arc's `C` and `D`** (`_ELLIPSE_THETA`, `_ELLIPSE_ECC`) are the pair the application saved for the arguments the file had, and the rule
//! differs from it raw in 2.6% of the angles and 0.5% of the ratios. A commit keeps a saved pair whose rule's pair names the same ellipse
//! (`compare::names_the_saved_ellipse`), and a pass over the whole page that answers an edit which changed no argument of an arc
//! (`Changes::arcs_untouched`) does not work them out at all, so that a theme applied or a style edited does not turn the arcs of the reciprocal family
//! into another curve. A resize writes the rule's pair, because the cells read `Width` and `Height`.
//!
//! **Which cells.** [`commit`] and [`commit_in`] work out every formula cell of the document or of one page. [`commit_changed`] works out the cells an
//! edit changed and the cells that read them, transitively ([`crate::changes_between`] finds the first, `affected.rs` the rest), so that a cell the
//! edit did not reach keeps the value and the text the file had even where this evaluator would give another result. **The chain goes on from a
//! cell only if it was written** (of the M3 review): a reader that comes out as it was, or that cannot be worked out, passes nothing on.
//! The cells are written by the same rules.
//!
//! **A pass over the whole page that answers an edit** ([`crate::Changes::whole_page`]: a style, the theme, a master's cell changed, which the comparison
//! cannot follow cell by cell) works out every formula cell the page holds, as [`commit_in`] does, and **is measured from the document the edit started
//! from** ([`crate::Changes::since`]). It used to give every instance on the page each cell it inherits whose result is not its master's
//! saved value, the edit's or not, and to leave the master's own cell as it was: an instance at its master's size then held a copy of the very cell the edit
//! changed (its master still held the value from before the edit), and one of another size the cells its file had left to its master. Now:
//! * **the masters' cells.** Each formula cell of a master shape an instance of the page inherits from is worked out where the edit changed its result
//!   (it differs from the one it has in the document before), and written by the rules above, so that an instance that leaves the cell to its master reads
//!   the new value through it. A master's cell the edit did not change keeps the value its file saved.
//! * **an instance's inherited cells.** An instance is given a cell it does not hold only where the edit changed what the cell works out to on it, and the
//!   result is neither its master's saved value nor what the master's own cell works out to (for "the source's" value after the evaluation). A cell
//!   the edit did not change stays as its file left it, whatever this evaluator gives for it: the cell of a size other than the master's that a file left to
//!   the master, a colour this evaluator works out otherwise than the file did.
//!
//! The cells worked out in the document before are charged to the commit's budget. With no document before, a pass over the whole page gives no
//! inherited cell to any instance and works out no master. [`commit`] and [`commit_in`] answer no edit, and give every instance its differing cells.
//!
//! **What it costs.** All the writes go through [`diacraft_doc::Document::write_batch`] in chunks, so the lookups of the document (the maps of
//! pages, masters, styles and shapes) are built once for a chunk and not once for a write. A chunk the model refuses (a row deleted here) is
//! written again one by one, and the refused ones counted. The writes not yet made are held memory, accounted, and past the budget's memory
//! the commit stops adding and ends as `budget_exhausted`; what it had gathered is still made.
//!
//! **What [`commit_changed`] costs beyond the cells it works out** is finding the cells that read the ones the edit changed. [`commit_changed_with`] is
//! given a [`crate::ReaderIndex`], which holds the readers of the page's cells through other sheets from the edit before and is put right by what the
//! edit replaced (`index.rs`), so a commit costs what the edit touched; `commit_changed` makes one for the commit and drops it, which costs a walk of every
//! formula of the page (the cost of every commit before a change). A graph that is kept is charged to the commit's memory at its start, as it was when it
//! was made, so it is within the budget a commit is given and a commit whose budget cannot hold it ends as `memory_exhausted`.
//!
//! **A write through a formula** ([`write_value`]): the cell with `BOUND` keeps the call and takes the written number as its first argument;
//! the cell with `SETATREF` keeps its formula and the number goes to the cell the call names, up to ten calls deep.

use std::collections::HashSet;
use std::sync::Arc;

use diacraft_doc::{
    CellAddr, CellEdit, CellWrite, DocError, Document, FormulaSlot, Lookup, Owner, ShapeRef, SheetRef, Value as DocValue, WriteOrigin, WriteOutcome,
};
use diacraft_formula::{Call, Expr, Number};
use diacraft_units::{Quantity, Unit, parse_unit};

use crate::affected::Affected;
use crate::cells::{CellEval, CellKind};
use crate::changes::Changes;
use crate::eval::{Budget, CellAddress, EvalCtx, EvalOptions, EvalStats, Evaluator, OperandMode, SetfMode};
use crate::index::ReaderIndex;
use crate::recalc::{for_each_formula_cell, walk_page, walk_sheets};
use crate::rules::{COMMIT_ADDS_DIFFERING_INHERITED_CELLS, KEY_BYTES, MAX_SETATREF_CHAIN};
use crate::setf::SetfEffect;
use crate::store::{decode, encode, number_text};
use crate::value::Value;

/// How many writes go in one batch.
const CHUNK: usize = 4096;

/// What a commit did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CommitReport {
    /// Cells whose cached value or error was written.
    pub written: u64,
    /// Of those, cells that were not in the file: `F` `Inh` cells added for an inherited result that differs from the source's.
    pub added: u64,
    /// Cells whose result agrees with what they hold.
    pub unchanged: u64,
    /// Cells not written because their result is unknown, has no `V` form, or is not a result (the depth limit).
    pub skipped: u64,
    /// Writes the model refused (a deleted row or section).
    pub refused: u64,
    /// Whether the steps or the memory ran out; what was gathered before is still written.
    pub budget_exhausted: bool,
    pub memory_exhausted: bool,
    pub stats: EvalStats,
    /// Formulas the `SETF` calls the commit reached wrote (`setf.rs`): each a cell write of origin `Setf`, whose cell and readers were worked out again.
    pub setf_written: u64,
    /// `SETF` calls whose target held their text already: nothing was written, which is how a call that its own write sets off again comes to rest.
    pub setf_same: u64,
    /// `SETF` calls dropped for the loop budget: past `MAX_SETF_WRITES` writes, past `MAX_SETF_ROUNDS` rounds, past `MAX_SETF_QUEUED` calls in one pass, or
    /// reached by a commit whose budget ran out.
    pub setf_dropped: u64,
    /// `SETF` writes the model refused (a row marked deleted, a limit of a sheet).
    pub setf_refused: u64,
    /// The rounds of `SETF` writes the commit made: each worked out again the cells the one before wrote.
    pub setf_rounds: u64,
}

impl CommitReport {
    /// Adds what a later pass of the same commit did.
    pub(crate) fn absorb(&mut self, next: &CommitReport) {
        self.written = self.written.saturating_add(next.written);
        self.added = self.added.saturating_add(next.added);
        self.unchanged = self.unchanged.saturating_add(next.unchanged);
        self.skipped = self.skipped.saturating_add(next.skipped);
        self.refused = self.refused.saturating_add(next.refused);
        self.budget_exhausted |= next.budget_exhausted;
        self.memory_exhausted |= next.memory_exhausted;
        self.stats.absorb(&next.stats);
        self.setf_written = self.setf_written.saturating_add(next.setf_written);
        self.setf_same = self.setf_same.saturating_add(next.setf_same);
        self.setf_dropped = self.setf_dropped.saturating_add(next.setf_dropped);
        self.setf_refused = self.setf_refused.saturating_add(next.setf_refused);
        self.setf_rounds = self.setf_rounds.saturating_add(next.setf_rounds);
    }
}

/// A write that is not yet made: the sheet, and what to write.
pub(crate) struct Pending {
    sheet: SheetRef,
    write: CellWrite,
    added: bool,
}

/// What a pending write holds, as accounted: the cell's key and the text of `V`.
fn pending_bytes(edit: &CellEdit) -> u64 {
    KEY_BYTES
        + match edit {
            CellEdit::Cached { value, .. } => value.text().len() as u64,
            _ => 0,
        }
}

/// Which sheets a commit works out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// Every sheet of the document: the document's, the styles', the pages' and masters', and every shape's.
    Document,
    /// The sheet of the page with this `ID` and the shapes of its part, and nothing else. What an edit on that page can have changed, given
    /// that the edit wrote no cell of a master, a style or another page. A frame function reaches no other page; a cell
    /// that names a cell of another page (`Pages[..]`) is read as it is, and worked out by [`crate::commit_changed_across`], which goes after those
    /// (`pages.rs`). A page that is not there gives an empty commit.
    Page(u32),
}

/// Calls `f` with each sheet of `scope`.
fn walk_scope(doc: &Document, scope: Scope, f: &mut dyn FnMut(&SheetRef, &diacraft_doc::Sheet)) {
    match scope {
        Scope::Document => walk_sheets(doc, f),
        Scope::Page(id) => walk_page(doc, id, f),
    }
}

/// Works out every formula cell of `doc` live and writes the results back (see the module documentation). The document is changed only by
/// those writes.
pub fn commit(doc: &mut Document, budget: &Budget, options: EvalOptions) -> CommitReport {
    commit_in(doc, budget, options, Scope::Document)
}

/// [`commit`] for the sheets of `scope`: an editing command works out the page it edited and not the document. Formulas outside the scope
/// are read as they are when a formula in it names them, and are not written.
pub fn commit_in(doc: &mut Document, budget: &Budget, options: EvalOptions, scope: Scope) -> CommitReport {
    let (mut report, effects) = commit_scope(doc, budget, options.clone(), scope, false, Inherited::All);
    // The `SETF` calls the working-out reached write when it has ended, and what they changed is worked out again (`setf.rs`).
    crate::setf::settle(doc, budget, effects, &mut report, &mut |doc, left, _| {
        commit_scope(doc, left, options.clone(), scope, false, Inherited::Unknown)
    });
    report
}

/// What a pass over a scope does with the cells an instance does not hold and inherits from its master.
#[derive(Clone, Copy)]
enum Inherited<'b> {
    /// Each is worked out, and added where its result is not its master's saved value: a recalculation that answers no edit.
    All,
    /// None is, and no master is worked out: a pass that answers an edit and is not told what the edit started from.
    Unknown,
    /// The pass answers an edit that started from this document (see the module documentation).
    Since(&'b Document),
}

/// [`commit_in`], and with `arcs_untouched` a pass that leaves the `C` and `D` of every elliptical arc as the file has them, unworked: the edit it
/// answers changed no argument of one (`Changes::arcs_untouched`). `inherited` says what is done with the cells the instances inherit.
fn commit_scope(
    doc: &mut Document,
    budget: &Budget,
    options: EvalOptions,
    scope: Scope,
    arcs_untouched: bool,
    inherited: Inherited<'_>,
) -> (CommitReport, Vec<SetfEffect>) {
    let options = EvalOptions { operands: OperandMode::Live, ..options };
    let mut report = CommitReport::default();
    let mut pending: Vec<Pending> = Vec::new();
    let mut ctx = EvalCtx::new(options.clone(), *budget);
    {
        let snapshot: &Document = doc;
        let mut ev = Evaluator::new(snapshot, &mut ctx);
        walk_scope(snapshot, scope, &mut |sheet, data| {
            for_each_formula_cell(data, |addr, _| {
                work_real(&mut ev, snapshot, sheet, addr, &mut report, &mut pending, arcs_untouched);
            });
        });
        match inherited {
            Inherited::All if COMMIT_ADDS_DIFFERING_INHERITED_CELLS => {
                add_inherited(&mut ev, snapshot, scope, &mut report, &mut pending, arcs_untouched, None);
            }
            Inherited::Since(before) => {
                // The document before is worked out with an evaluator of its own, whose work is charged to this one's budget as it is done.
                // It writes nothing, so a `SETF` there is 0 and queues nothing.
                let mut was_ctx = EvalCtx::new(EvalOptions { setf: SetfMode::Inert, ..options }, *budget);
                let mut was = Evaluator::new(before, &mut was_ctx);
                work_sources(&mut ev, &mut was, snapshot, scope, &mut report, &mut pending);
                if COMMIT_ADDS_DIFFERING_INHERITED_CELLS {
                    add_inherited(&mut ev, snapshot, scope, &mut report, &mut pending, arcs_untouched, Some(&mut was));
                }
            }
            Inherited::All | Inherited::Unknown => {}
        }
    }
    report.stats = ctx.stats;
    report.budget_exhausted = ctx.stats.budget_exhausted;
    report.memory_exhausted = ctx.stats.memory_exhausted;
    report.setf_dropped = ctx.setf_dropped;
    crate::work::committed(report.stats.steps);
    apply(doc, pending, &mut report);
    (report, std::mem::take(&mut ctx.setf))
}

/// [`commit_in`] for what an edit changed on page `page` ([`Changes`], from [`crate::changes_between`]): the cells it wrote and the cells that
/// read them, transitively, and no others ([`crate::affected`] says what reads what). A cell not worked out keeps its saved value and its text,
/// byte for byte, whatever this evaluator would give for it; a cell that is worked out is written only where the result differs, as
/// in [`commit_in`]. When `changes` says [`Changes::whole_page`] every formula cell of the page is worked out, as `commit_in` with [`Scope::Page`] does, and the
/// cells the instances inherit and the masters' cells as the module documentation says, measured from [`Changes::since`].
pub fn commit_changed(doc: &mut Document, budget: &Budget, options: EvalOptions, page: u32, changes: &Changes) -> CommitReport {
    commit_changed_with(&mut ReaderIndex::new(), doc, budget, options, page, changes)
}

/// [`commit_changed`] with a [`ReaderIndex`]: what reads what through other sheets is found once for the page and kept in `index`, and an edit puts
/// right what it read of the sheets it replaced and no more, so that what an edit costs is what it touched and not every formula of the page
/// The index is brought up to date with `doc` by what is different from the page it was made for, whatever edits there were between, so a
/// caller keeps one index for a document and gives it to each commit; an index made for another page or another document is made again. The cells
/// worked out and written are the cells `commit_changed` works out and writes, in the same order (`affected/tests.rs` holds the two to it).
pub fn commit_changed_with(
    index: &mut ReaderIndex,
    doc: &mut Document,
    budget: &Budget,
    options: EvalOptions,
    page: u32,
    changes: &Changes,
) -> CommitReport {
    let (mut report, effects) = changed_pass(index, doc, budget, options.clone(), page, changes);
    // The `SETF` calls the working-out reached write when it has ended, and the cells they wrote and what reads them are worked out again (`setf.rs`).
    crate::setf::settle(doc, budget, effects, &mut report, &mut |doc, left, written| written_pass(index, doc, left, &options, page, written));
    report
}

/// The page a sheet is the sheet of, or of one of the shapes of: none for the document's, a master's and a style's.
fn page_of(sheet: &SheetRef) -> Option<u32> {
    match sheet {
        SheetRef::Page(id) => Some(*id),
        SheetRef::Shape(at) => match at.owner {
            Owner::Page(page) => Some(page),
            _ => None,
        },
        SheetRef::Document | SheetRef::Master(_) | SheetRef::Style(_) => None,
    }
}

/// The pass that works out what the `SETF` calls of one round wrote: the cells of `page` and what reads them, as an edit's pass does; a cell of **another page** (a call that a cell of
/// that page reached through `Pages[..]` made) in a pass of that page, so that its value and what reads it are worked out and not left as they were, with an index
/// made for it; and the whole of `page` when a write is to a sheet no page holds (the document's, a master's, a style's) or to a page the document has not, as before. The
/// reports are added, and the calls the passes queued go on to the next round.
fn written_pass(
    index: &mut ReaderIndex,
    doc: &mut Document,
    left: &Budget,
    options: &EvalOptions,
    page: u32,
    written: Vec<(SheetRef, CellAddr)>,
) -> (CommitReport, Vec<SetfEffect>) {
    let mut here: Vec<(SheetRef, CellAddr)> = Vec::new();
    let mut elsewhere: Vec<(u32, Vec<(SheetRef, CellAddr)>)> = Vec::new();
    let mut whole_page = false;
    for cell in written {
        match page_of(&cell.0) {
            Some(own) if own == page => here.push(cell),
            Some(other) if doc.page(other).is_some() => match elsewhere.iter_mut().find(|(known, _)| *known == other) {
                Some((_, cells)) => cells.push(cell),
                None => elsewhere.push((other, vec![cell])),
            },
            _ => {
                whole_page = true;
                here.push(cell);
            }
        }
    }
    let next = Changes { whole_page, cells: here, ..Changes::default() };
    let (mut report, mut effects) = changed_pass(index, doc, left, options.clone(), page, &next);
    for (other, cells) in elsewhere {
        let left = Budget { steps: left.steps.saturating_sub(report.stats.steps), bytes: left.bytes };
        if left.steps == 0 {
            report.budget_exhausted = true;
            break;
        }
        let (more, queued) = changed_pass(&mut ReaderIndex::new(), doc, &left, options.clone(), other, &Changes { cells, ..Changes::default() });
        report.absorb(&more);
        effects.extend(queued);
    }
    (report, effects)
}

/// One pass of [`commit_changed_with`]: what [`commit_changed_with`] was before it made `SETF` writes, and the writes the cells it worked out asked for.
fn changed_pass(
    index: &mut ReaderIndex,
    doc: &mut Document,
    budget: &Budget,
    options: EvalOptions,
    page: u32,
    changes: &Changes,
) -> (CommitReport, Vec<SetfEffect>) {
    if changes.whole_page || doc.page(page).is_none() {
        let inherited = match changes.since.document() {
            Some(before) if changes.whole_page => Inherited::Since(before),
            _ => Inherited::Unknown,
        };
        return commit_scope(doc, budget, options, Scope::Page(page), changes.whole_page && changes.arcs_untouched, inherited);
    }
    let options = EvalOptions { operands: OperandMode::Live, ..options };
    let mut report = CommitReport::default();
    let mut pending: Vec<Pending> = Vec::new();
    let mut ctx = EvalCtx::new(options, *budget);
    // Whether any cell was found to work out: a commit that is `only_if_read` and found none is not counted.
    let mut worked = false;
    if !changes.is_empty() {
        let snapshot: &Document = doc;
        let mut ev = Evaluator::new(snapshot, &mut ctx);
        let mut work = Affected::new(&mut ev, snapshot, page, changes, &mut *index);
        while let Some((sheet, addr)) = work.next() {
            worked = true;
            if ev.ctx.stats.budget_exhausted {
                break;
            }
            // A cell the sheet does not hold is one it inherits from its master: written only if its result differs from the master's saved
            // value. One it holds is written if its result differs from the one it saved.
            let held = ev.res.sheet(&sheet).map(|own| ev.res.lookup(own, &addr));
            let wrote = match held {
                Some(Lookup::Found(_)) => work_real(&mut ev, snapshot, &sheet, addr.clone(), &mut report, &mut pending, false),
                Some(Lookup::Missing) if COMMIT_ADDS_DIFFERING_INHERITED_CELLS => {
                    work_inherited(&mut ev, snapshot, &sheet, addr.clone(), &mut report, &mut pending, false)
                }
                _ => false,
            };
            // Only a cell whose value is to be written has changed: the cells that read it are next. A cell that comes out as it was, or that
            // this evaluator cannot work out, keeps its readers as the file had them (of the M3 review).
            if wrote {
                work.changed(&mut ev, &(sheet, addr));
            }
        }
    }
    index.finish();
    report.stats = ctx.stats;
    report.budget_exhausted = ctx.stats.budget_exhausted;
    report.memory_exhausted = ctx.stats.memory_exhausted;
    report.setf_dropped = ctx.setf_dropped;
    if worked || !changes.only_if_read {
        crate::work::committed(report.stats.steps);
    }
    apply(doc, pending, &mut report);
    (report, std::mem::take(&mut ctx.setf))
}

/// Works out one cell the sheet holds and plans the write its result calls for. → whether a write was planned.
///
/// **An elliptical arc's `C` and `D`** (`_ELLIPSE_THETA`, `_ELLIPSE_ECC`) are the application's own pair for the arguments the file was saved with, and
/// the rule differs from it raw in 2.6% of the angles and 0.5% of the ratios. So a pass that answers an edit which changed
/// no argument of an arc (`arcs_untouched`) does not work them out at all, and any other leaves a saved pair alone when the pair the rule gives names
/// the same ellipse (`compare::names_the_saved_ellipse`: the other end of an axis, the other axis with the reciprocal, a circle at any angle). A pair
/// that names another ellipse is written, as a resize calls for.
pub(crate) fn work_real(
    ev: &mut Evaluator<'_, '_>,
    snapshot: &Document,
    sheet: &SheetRef,
    addr: CellAddr,
    report: &mut CommitReport,
    pending: &mut Vec<Pending>,
    arcs_untouched: bool,
) -> bool {
    if ev.ctx.stats.budget_exhausted {
        return false;
    }
    let at = CellAddress { sheet: sheet.clone(), addr: addr.clone() };
    if arcs_untouched && crate::compare::is_arc_pair_cell(ev, &at) {
        report.unchanged += 1;
        return false;
    }
    let outcome = ev.evaluate_cell(&at);
    // A spent budget and a met depth limit are not results: nothing is written for them (a healthy cell does not get `#VALUE!`).
    if ev.ctx.stats.budget_exhausted {
        return false;
    }
    let plan = plan_write(snapshot, &outcome);
    let plan = if matches!(plan, Plan::Write(_)) && crate::compare::names_the_saved_ellipse(ev, &at, &outcome) { Plan::Same } else { plan };
    match plan {
        Plan::Skip => report.skipped += 1,
        Plan::Same => report.unchanged += 1,
        Plan::Write(edit) => {
            if ev.ctx.hold_bytes(pending_bytes(&edit)) {
                pending.push(Pending { sheet: sheet.clone(), write: CellWrite { addr, edit, origin: WriteOrigin::Evaluation }, added: false });
                return true;
            }
        }
    }
    false
}

/// Works out one cell an instance does not hold and inherits from its master shape, and plans to add it when its result differs from the master's.
/// → whether a write was planned.
pub(crate) fn work_inherited(
    ev: &mut Evaluator<'_, '_>,
    snapshot: &Document,
    sheet: &SheetRef,
    addr: CellAddr,
    report: &mut CommitReport,
    pending: &mut Vec<Pending>,
    arcs_untouched: bool,
) -> bool {
    work_inherited_in(ev, Pass { arcs_untouched, was: None }, snapshot, sheet, addr, report, pending)
}

/// How a pass works out an inherited cell: whether it leaves the arcs' pairs be, and the evaluator of the document the edit started from when it answers
/// an edit measured from one.
struct Pass<'a, 'd, 'c> {
    arcs_untouched: bool,
    was: Option<&'a mut Evaluator<'d, 'c>>,
}

/// [`work_inherited`] for a [`Pass`]: measured from the document before, the cell is planned only where the edit changed its result, and only where that
/// result is not what the master's own cell works out to either (the module documentation).
fn work_inherited_in(
    ev: &mut Evaluator<'_, '_>,
    pass: Pass<'_, '_, '_>,
    snapshot: &Document,
    sheet: &SheetRef,
    addr: CellAddr,
    report: &mut CommitReport,
    pending: &mut Vec<Pending>,
) -> bool {
    let SheetRef::Shape(shape) = sheet else { return false };
    if !matches!(shape.owner, Owner::Page(_)) || ev.ctx.stats.budget_exhausted {
        return false;
    }
    let Ok(Some(step)) = ev.res.master_step(shape) else { return false };
    let master_sheet = SheetRef::Shape(step.shape);
    let at = CellAddress { sheet: sheet.clone(), addr: addr.clone() };
    // An arc's pair is left as its master holds it, as `work_real` leaves a saved one.
    if pass.arcs_untouched && crate::compare::is_arc_pair_cell(ev, &at) {
        return false;
    }
    let outcome = ev.evaluate_cell(&at);
    if ev.ctx.stats.budget_exhausted {
        return false;
    }
    let Plan::Write(edit) = plan_write_inherited(ev, snapshot, &outcome, &master_sheet, &addr) else { return false };
    if crate::compare::names_the_saved_ellipse(ev, &at, &outcome) {
        return false;
    }
    if let Some(was) = pass.was {
        if !changed_by_edit(ev, was, &at, &outcome) {
            return false;
        }
        // The master's cell is worked out in this pass where the edit changed it: a result equal to what it works out to is the master's after the pass.
        let source = ev.evaluate_cell(&CellAddress { sheet: master_sheet, addr: addr.clone() });
        if ev.ctx.stats.budget_exhausted || same_result(snapshot, &source, &outcome) {
            return false;
        }
    }
    if !ev.ctx.hold_bytes(pending_bytes(&edit)) {
        return false;
    }
    report.added += 1;
    pending.push(Pending { sheet: sheet.clone(), write: CellWrite { addr, edit, origin: WriteOrigin::Evaluation }, added: true });
    true
}

enum Plan {
    Skip,
    Same,
    Write(CellEdit),
}

/// Whether a unit is one that says what kind a value is, as opposed to one that says how a number is shown: the first is written, the
/// second is left as the cell had it (the stored number does not depend on it).
fn is_kind_unit(u: &str) -> bool {
    matches!(u, "DATE" | "CY" | "PNT" | "NURBS" | "POLYLINE" | "MULTIDIM" | "STR" | "BOOL")
}

/// The `U` to write with a result: its kind's unit, and for a free cell (`User`, `Prop`, `Scratch`) the unit of a length too. A cell with a unit of its own kind (`Width`) is not given one: the stored
/// number does not depend on it.
///
/// **The unit is the written cell's own, whatever the cell it takes its formula from says** (a text in `V` with no `U` is no text to a program that reads the `U`): an
/// instance's cell that holds a cached result beside `F="Inh"` says `STR`, `DATE` or `BOOL` itself, though its master's cell says it too. Writing a unit a cell holds already changes nothing.
fn unit_to_write(encoded: Option<&str>, typed: bool) -> Option<Arc<str>> {
    encoded.filter(|u| is_kind_unit(u) || !typed).map(Arc::from)
}

/// What a worked-out cell calls for.
fn plan_write(doc: &Document, outcome: &CellEval) -> Plan {
    if !matches!(outcome.kind, CellKind::Formula) || outcome.default_formula || outcome.unsupported.is_some() || outcome.depth_limit_hits > 0 {
        return Plan::Skip;
    }
    // A `Themed` value and a date past the year 9999 are the file's, and a recalculation without the theme does not replace them.
    if matches!(outcome.saved, Some(Value::Unsupported(_))) {
        return Plan::Skip;
    }
    match &outcome.raw {
        Value::Unsupported(_) => Plan::Skip,
        Value::Error(e) => {
            // The value stays as it was, byte for byte, and the error is recorded. A cell with no value to
            // keep carries nothing.
            let Some(kept) = &outcome.saved_value else { return Plan::Skip };
            if outcome.saved_error == Some(*e) {
                return Plan::Same;
            }
            Plan::Write(CellEdit::Cached { value: kept.clone(), error: Some(Arc::from(e.text())), unit: None })
        }
        value => {
            let Some(enc) = encode(value) else { return Plan::Skip };
            let agrees = outcome.saved.as_ref().is_some_and(|saved| crate::compare::agrees(doc, saved, value));
            if agrees {
                return match (&outcome.saved_error, &outcome.saved_value) {
                    (None, _) => Plan::Same,
                    // The result is what the file holds, and a stale `E` is on it: only the `E` goes, and `V` keeps its text.
                    (Some(_), Some(kept)) => Plan::Write(CellEdit::Cached { value: kept.clone(), error: None, unit: None }),
                    (Some(_), None) => Plan::Skip,
                };
            }
            let unit = unit_to_write(enc.u.as_deref(), outcome.typed);
            Plan::Write(CellEdit::Cached { value: DocValue::from_text(&enc.v), error: None, unit })
        }
    }
}

/// The cells an instance inherits from its master's formulas and does not hold, whose result differs from the master's saved value. With `was`, the
/// evaluator of the document an edit started from, only those whose result the edit changed ([`work_inherited_in`]).
fn add_inherited(
    ev: &mut Evaluator<'_, '_>,
    doc: &Document,
    scope: Scope,
    report: &mut CommitReport,
    pending: &mut Vec<Pending>,
    arcs_untouched: bool,
    mut was: Option<&mut Evaluator<'_, '_>>,
) {
    walk_scope(doc, scope, &mut |sheet, own| {
        let SheetRef::Shape(at) = sheet else { return };
        if !matches!(at.owner, Owner::Page(_)) || ev.ctx.stats.budget_exhausted {
            return;
        }
        let Ok(Some(step)) = ev.res.master_step(at) else { return };
        let master_sheet = SheetRef::Shape(step.shape.clone());
        let Some(master) = ev.res.sheet(&master_sheet) else { return };
        let mut candidates: Vec<CellAddr> = Vec::new();
        for_each_formula_cell(master, |addr, _| candidates.push(addr));
        for addr in candidates {
            if ev.ctx.stats.budget_exhausted || !ev.steps(1) {
                return;
            }
            if !matches!(ev.res.lookup(own, &addr), Lookup::Missing) {
                continue;
            }
            let pass = Pass { arcs_untouched, was: was.as_deref_mut() };
            work_inherited_in(ev, pass, doc, sheet, addr, report, pending);
        }
    });
}

/// The formula cells of the master shapes the instances of `scope` inherit from (their master steps), each worked out where the edit changed its result
/// (it differs from the one it has in the document before, `was`) and written as a cell the master holds is ([`work_real`]): an instance that leaves the
/// cell to its master reads the new value through it. A master cell the edit did not change keeps the value its file saved, whatever this evaluator gives
/// for it. Each master shape is listed once, its key held memory as accounted.
fn work_sources(
    ev: &mut Evaluator<'_, '_>,
    was: &mut Evaluator<'_, '_>,
    doc: &Document,
    scope: Scope,
    report: &mut CommitReport,
    pending: &mut Vec<Pending>,
) {
    let mut listed: HashSet<ShapeRef> = HashSet::new();
    let mut sources: Vec<ShapeRef> = Vec::new();
    walk_scope(doc, scope, &mut |sheet, _| {
        let SheetRef::Shape(at) = sheet else { return };
        if !matches!(at.owner, Owner::Page(_)) || ev.ctx.stats.budget_exhausted {
            return;
        }
        let Ok(Some(step)) = ev.res.master_step(at) else { return };
        if listed.contains(&step.shape) || !ev.ctx.hold_bytes(2 * KEY_BYTES) {
            return;
        }
        listed.insert(step.shape.clone());
        sources.push(step.shape);
    });
    for shape in sources {
        let sheet = SheetRef::Shape(shape);
        let Some(data) = ev.res.sheet(&sheet) else { continue };
        let mut cells: Vec<CellAddr> = Vec::new();
        for_each_formula_cell(data, |addr, _| cells.push(addr));
        for addr in cells {
            if ev.ctx.stats.budget_exhausted || !ev.steps(1) {
                return;
            }
            let at = CellAddress { sheet: sheet.clone(), addr: addr.clone() };
            let now = ev.evaluate_cell(&at);
            if !ev.ctx.stats.budget_exhausted && changed_by_edit(ev, was, &at, &now) {
                work_real(ev, doc, &sheet, addr, report, pending, false);
            }
        }
    }
}

/// Whether the edit changed what `at` works out to: what it works out to now (`now`) and in the document the edit started from (`was`) are not the same
/// result. A sheet that is not the same shape there (another `ID`, master, master shape or `Del` at its place, or nothing) changed. The work done in the
/// document before is charged to `ev`'s budget; when either budget runs out the answer is no, and `ev`'s is spent.
fn changed_by_edit(ev: &mut Evaluator<'_, '_>, was: &mut Evaluator<'_, '_>, at: &CellAddress, now: &CellEval) -> bool {
    if !same_sheet(ev.doc, was.doc, &at.sheet) {
        return !ev.ctx.stats.budget_exhausted;
    }
    let (steps, held) = (was.ctx.stats.steps, was.ctx.bytes_held());
    let then = was.evaluate_cell(at);
    let used = was.ctx.stats.steps.saturating_sub(steps);
    let grown = was.ctx.bytes_held().saturating_sub(held);
    if !ev.ctx.take_steps(used) || !ev.ctx.hold_bytes(grown) {
        return false;
    }
    if was.ctx.stats.budget_exhausted {
        // The document before could not be worked out within the commit's own budget: that is the commit's budget spent.
        ev.ctx.stats.memory_exhausted |= was.ctx.stats.memory_exhausted;
        ev.ctx.stats.budget_exhausted = true;
        return false;
    }
    !same_result(ev.doc, &then, now)
}

/// Whether `sheet` is the same sheet in the document now and in the one before: for a shape, one there at its place with the same `ID`, master, master
/// shape and `Del`; for any other sheet, one there.
fn same_sheet(now: &Document, then: &Document, sheet: &SheetRef) -> bool {
    match sheet {
        SheetRef::Shape(at) => match (now.shape(at), then.shape(at)) {
            (Some(a), Some(b)) => a.id == b.id && a.master == b.master && a.master_shape == b.master_shape && a.del == b.del,
            _ => false,
        },
        other => then.sheet_of(other).is_some(),
    }
}

/// Whether two workings-out of a cell are the same result: of one kind, found or not alike, and equal or agreeing as a commit tells a result from a saved
/// value ([`crate::compare::agrees`]).
fn same_result(doc: &Document, then: &CellEval, now: &CellEval) -> bool {
    then.kind == now.kind && then.found == now.found && (then.raw == now.raw || crate::compare::agrees(doc, &then.raw, &now.raw))
}

/// An inherited result is written only when it differs from the source's saved value.
fn plan_write_inherited(ev: &mut Evaluator<'_, '_>, doc: &Document, outcome: &CellEval, master: &SheetRef, addr: &CellAddr) -> Plan {
    if !matches!(outcome.kind, CellKind::Formula) || outcome.default_formula || outcome.unsupported.is_some() || outcome.depth_limit_hits > 0 {
        return Plan::Skip;
    }
    let value = match &outcome.raw {
        Value::Error(_) | Value::Unsupported(_) => return Plan::Skip,
        v => v,
    };
    let Some(enc) = encode(value) else { return Plan::Skip };
    // The source's saved value, as typed: the master's own cell.
    let held = ev.res.resolve(master, addr).and_then(|r| {
        let c = r.value.cell?;
        Some((c.value.clone()?, c.unit.clone()))
    });
    // The master's saved value is decoded once for each instance that inherits the cell: its length is charged first.
    if let Some((v, _)) = &held
        && !ev.work(v.text().len())
    {
        return Plan::Skip;
    }
    let source = held.map(|(v, unit)| (decode(v.text(), unit.as_deref(), None), unit));
    if let Some((saved, _)) = &source
        && (matches!(saved, Value::Unsupported(_)) || crate::compare::agrees(doc, saved, value))
    {
        return Plan::Same;
    }
    let unit = unit_to_write(enc.u.as_deref(), outcome.typed);
    Plan::Write(CellEdit::Cached { value: DocValue::from_text(&enc.v), error: None, unit })
}

/// Makes the writes in chunks; a chunk the model refuses is made again one write at a time.
pub(crate) fn apply(doc: &mut Document, pending: Vec<Pending>, report: &mut CommitReport) {
    let mut iter = pending.into_iter().peekable();
    while iter.peek().is_some() {
        let chunk: Vec<Pending> = iter.by_ref().take(CHUNK).collect();
        let batch: Vec<(SheetRef, CellWrite)> = chunk.iter().map(|p| (p.sheet.clone(), p.write.clone())).collect();
        match doc.write_batch(batch) {
            Ok(_) => report.written += chunk.len() as u64,
            Err(_) => {
                for p in chunk {
                    match doc.write_cell(&p.sheet, &p.write.addr, p.write.edit, p.write.origin) {
                        Ok(_) => report.written += 1,
                        Err(_) => {
                            report.refused += 1;
                            if p.added {
                                report.added = report.added.saturating_sub(1);
                            }
                        }
                    }
                }
            }
        }
    }
}

// ---- a write through a formula ---------------------------------------------------------------------------------------

/// Sets a cell to a value as a write from the drawing window or a program does, with the formulas of surviving: a cell
/// whose formula is `BOUND(...)` keeps it and takes `value` as its first argument; one whose formula is `SETATREF(target...)` keeps
/// it and `value` goes to the cell it names, following up to [`MAX_SETATREF_CHAIN`] such calls. Any other cell is written as the
/// model writes it, by `origin`. The formula of the cell with `BOUND` is rewritten (printed), and its saved value is left for the next
/// recalculation.
pub fn write_value(doc: &mut Document, sheet: &SheetRef, addr: &CellAddr, value: &Quantity, origin: WriteOrigin) -> Result<WriteOutcome, DocError> {
    let mut sheet = sheet.clone();
    let mut addr = addr.clone();
    for _ in 0..MAX_SETATREF_CHAIN {
        let step = {
            let mut ctx = EvalCtx::default();
            let mut ev = Evaluator::new(doc, &mut ctx);
            redirect(&mut ev, &sheet, &addr)
        };
        match step {
            Redirect::Bound(expr, unit) => {
                let rewritten = rewrite_bound(&expr, value, unit);
                let formula = diacraft_formula::parse(&diacraft_formula::print(&rewritten));
                return doc.write_cell(&sheet, &addr, CellEdit::Formula { formula: Arc::new(formula), value: None }, origin);
            }
            Redirect::Target(next_sheet, next_addr) => {
                sheet = next_sheet;
                addr = next_addr;
            }
            Redirect::Plain => break,
        }
    }
    doc.write_cell(&sheet, &addr, CellEdit::Value(DocValue::from_text(&number_text(value.stored))), origin)
}

enum Redirect {
    /// The formula to rewrite, and the unit its cell shows.
    Bound(Expr, Unit),
    /// The cell a `SETATREF` names.
    Target(SheetRef, CellAddr),
    Plain,
}

fn redirect(ev: &mut Evaluator<'_, '_>, sheet: &SheetRef, addr: &CellAddr) -> Redirect {
    let Some(res) = ev.res.resolve(sheet, addr) else { return Redirect::Plain };
    let Some(cell) = res.formula.cell else { return Redirect::Plain };
    let FormulaSlot::Local(formula) = &cell.formula else { return Redirect::Plain };
    let Ok(Expr::Call(call)) = &formula.ast else { return Redirect::Plain };
    if call.is("BOUND") && call.qualifiers.is_empty() && call.args.len() >= 5 {
        let unit = cell.unit.as_deref().and_then(parse_unit).unwrap_or(Unit::DrawingDefault);
        return Redirect::Bound(Expr::Call(call.clone()), unit);
    }
    if call.is("SETATREF")
        && call.qualifiers.is_empty()
        && let Some(Some(Expr::Ref(target))) = call.args.first()
    {
        let env = crate::eval::Env::new(res.on.clone(), res.rebind.clone());
        if let Ok((s, a)) = ev.locate(target, &env) {
            return Redirect::Target(s, a);
        }
    }
    Redirect::Plain
}

/// The `BOUND` call with its first argument the written number, in the unit the cell shows.
fn rewrite_bound(expr: &Expr, value: &Quantity, unit: Unit) -> Expr {
    let Expr::Call(call) = expr else { return expr.clone() };
    let mut call: Call = (**call).clone();
    let literal = if unit.measure() == diacraft_units::Measure::None || matches!(unit, Unit::DrawingDefault | Unit::Unitless) {
        Number::with_unit(value.stored, Unit::DrawingDefault)
    } else {
        Number::with_unit(value.value_in(unit).unwrap_or(value.stored), unit)
    };
    if let Some(first) = call.args.first_mut() {
        *first = Some(Expr::Number(literal));
    }
    Expr::Call(Box::new(call))
}
