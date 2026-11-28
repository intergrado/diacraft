//! The readers of the cells of the open drawing's page, kept from one edit to the next.
//!
//! To say which cells an edit reaches, `calc` needs the cells that read a cell through another sheet. Found by asking every formula of the page, that was
//! most of what an edit cost on a page of twelve thousand shapes. `calc` keeps the answer in a [`ReaderIndex`] and puts right what an edit replaced
//! (`diacraft_calc::commit_changed_across`, which also goes after the cells of other pages that name a cell that changed); the index belongs to **the session** and is given to every commit the session's editing commands make, which are
//! made deep in those commands, where a document is held and no session (as the faces text is measured in are, `measure.rs`). A session enters its index
//! for the time an edit runs ([`InUse`], taken by `Session::edit_if` around the command's closure and the refit that follows), and
//! [`commit_changed`] is the one call the commands make: it uses the index of the session that is editing, and with none (a call outside an edit, a
//! document no session holds) it works as it did before, with an index that lives for the commit.
//!
//! **Nothing depends on the index being right.** `calc` finds what an edit replaced by what is different from the page the index was made for (by pointer, so
//! what an edit did not write costs nothing), whatever happened between: an undo, a redo, a command that failed and was put back, a file opened over the
//! drawing, another thread's session. An index that is for another page is made again. **What it holds is counted**: [`KeptIndex::held_bytes`] is in the
//! session's hold on the process's ledger ([`crate::memory::Hold`]), so that a read of a file leaves room for it, and the index is let go of when the
//! drawing is replaced.

use std::cell::{Cell, RefCell};
use std::sync::{Arc, Mutex, PoisonError};

use diacraft_calc::{Budget, Changes, CommitReport, EvalOptions, ReaderIndex, changes_between_with, commit_changed_across};
use diacraft_doc::Document;

/// The index a session keeps: shared with the thread that is editing for the time of an edit.
#[derive(Debug, Default)]
pub(crate) struct KeptIndex(Arc<Mutex<ReaderIndex>>);

impl KeptIndex {
    /// What the index is charged, as accounted: what the session counts of it ([`ReaderIndex::held_bytes`]).
    pub(crate) fn held_bytes(&self) -> u64 {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).held_bytes()
    }

    /// Lets the index go: the drawing it was made for is replaced.
    pub(crate) fn clear(&self) {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).clear();
    }
}

thread_local! {
    /// The index of the session that is editing on this thread, while it is.
    static CURRENT: RefCell<Option<Arc<Mutex<ReaderIndex>>>> = const { RefCell::new(None) };
}

/// For as long as it lives, the commits made on this thread use the index of the session it was made for. One inside another (a command that runs
/// a command) leaves the outer one's in place when it ends.
pub(crate) struct InUse {
    previous: Option<Arc<Mutex<ReaderIndex>>>,
}

impl InUse {
    pub(crate) fn enter(kept: &KeptIndex) -> InUse {
        InUse { previous: CURRENT.with(|current| current.replace(Some(Arc::clone(&kept.0)))) }
    }
}

impl Drop for InUse {
    fn drop(&mut self) {
        let previous = self.previous.take();
        CURRENT.with(|current| *current.borrow_mut() = previous);
    }
}

/// The index of the session that is editing on this thread, if one is.
fn current() -> Option<Arc<Mutex<ReaderIndex>>> {
    CURRENT.with(|current| current.borrow().clone())
}

thread_local! {
    /// The steps of the evaluator that the commits of the call that is making them have left, while a call shares one budget.
    static CALL_STEPS: Cell<Option<u64>> = const { Cell::new(None) };
}

/// The steps of the evaluator that the call that is making the commits on this thread has left, while a call shares one budget ([`SharedSteps`]); `None` while none does.
pub(crate) fn shared_steps_left() -> Option<u64> {
    CALL_STEPS.with(Cell::get)
}

/// For as long as it lives, the commits made on this thread draw on **one budget of steps, `total`: the budget of one call**. Each commit is given what is left and takes
/// off what it used, so the pages one call works out (the document's sheet or a style reaches every page) pass a total together and not one budget each: the budget of
/// [`Budget::DEFAULT_STEPS`] is the budget of a whole document (`diacraft_calc::rules::DEFAULT_STEPS`), and a drawing of many pages each just under it was worked out in as many budgets, which
/// a file made for it holds for hours. A commit that finds nothing left is refused as one that passed its budget is: the call is refused whole, in words. With none entered every commit has
/// the budget it asks for, as it had.
pub(crate) struct SharedSteps {
    entered: bool,
}

impl SharedSteps {
    /// One inside another leaves the outer budget in place.
    pub(crate) fn enter(total: u64) -> SharedSteps {
        let entered = CALL_STEPS.with(|left| {
            if left.get().is_some() {
                return false;
            }
            left.set(Some(total));
            true
        });
        SharedSteps { entered }
    }
}

impl Drop for SharedSteps {
    fn drop(&mut self) {
        if self.entered {
            CALL_STEPS.with(|left| left.set(None));
        }
    }
}

thread_local! {
    /// Whether the call that is making the commits on this thread works every page of the document out whole.
    static EVERY_PAGE: Cell<bool> = const { Cell::new(false) };
}

/// For as long as it lives, the whole-page commits made on this thread say that **every page of the document is worked out the same way in the call** ([`Changes::other_pages_worked`]):
/// an edit of the document's sheet or of a style, which `sheet.set_cell` works out page by page. A pass over a page then does not go after the cells of other pages that name it, which
/// the pass over each of them works out where they stand. One inside another leaves the outer one in place.
pub(crate) struct EveryPage {
    entered: bool,
}

impl EveryPage {
    pub(crate) fn enter() -> EveryPage {
        EveryPage { entered: !EVERY_PAGE.replace(true) }
    }
}

impl Drop for EveryPage {
    fn drop(&mut self) {
        if self.entered {
            EVERY_PAGE.set(false);
        }
    }
}

/// `diacraft_calc::commit_changed` for what an editing command changed: with the index of the session that is editing, if one is, and within the steps the call has left, if it shares a
/// budget ([`SharedSteps`]).
pub(crate) fn commit_changed(doc: &mut Document, budget: &Budget, options: EvalOptions, page: u32, changes: &Changes) -> CommitReport {
    let every = EVERY_PAGE.get() && changes.whole_page && !changes.other_pages_worked;
    let said = every.then(|| Changes { other_pages_worked: true, ..changes.clone() });
    let changes = said.as_ref().unwrap_or(changes);
    let left = CALL_STEPS.with(Cell::get);
    let budget = left.map_or(*budget, |left| Budget { steps: budget.steps.min(left), ..*budget });
    let report = commit_with_index(doc, &budget, options, page, changes);
    if let Some(left) = left {
        CALL_STEPS.with(|cell| cell.set(Some(left.saturating_sub(report.stats.steps))));
    }
    report
}

fn commit_with_index(doc: &mut Document, budget: &Budget, options: EvalOptions, page: u32, changes: &Changes) -> CommitReport {
    let Some(kept) = current() else { return commit_changed_across(&mut ReaderIndex::new(), doc, budget, options, page, changes) };
    // A panic in a commit leaves a lock poisoned and the index with nothing in it (`ReaderIndex::sync` takes the graph out before it works on it), which the next
    // commit makes again: the poison says nothing that is not already true.
    let mut index = kept.lock().unwrap_or_else(PoisonError::into_inner);
    let report = commit_changed_across(&mut index, doc, budget, options, page, changes);
    // The `SETF` calls the commit dropped past its caps are added to the edit's count, which the command's answer and the window's status bar say (`setf_note.rs`). A commit
    // with no index is no edit's (the edit that makes commits has both), so the early return above has nothing to add.
    crate::setf_note::note(&report);
    report
}

/// Whether any sheet of `doc` names a master (`diacraft_calc::masters_are_named`), with the index of the session that is editing, if one is, and within the steps the call has left, if it shares a
/// budget ([`SharedSteps`]), which it takes what the listing took off.
pub(crate) fn masters_are_named(doc: &Document) -> bool {
    let options = crate::measure::eval_options();
    let left = CALL_STEPS.with(Cell::get);
    let default = Budget::default();
    let budget = left.map_or(default, |left| Budget { steps: default.steps.min(left), ..default });
    let (named, used) = match current() {
        Some(kept) => {
            let mut index = kept.lock().unwrap_or_else(PoisonError::into_inner);
            diacraft_calc::masters_are_named(&mut index, doc, &budget, options)
        }
        None => diacraft_calc::masters_are_named(&mut ReaderIndex::new(), doc, &budget, options),
    };
    if let Some(left) = left {
        CALL_STEPS.with(|cell| cell.set(Some(left.saturating_sub(used))));
    }
    named
}

/// `diacraft_calc::changes_between` for what an editing command changed: with the index of the session that is editing, if one is, which remembers the page it compared last.
pub(crate) fn changes_between(before: &Document, after: &Document, page: u32) -> Changes {
    let Some(kept) = current() else { return diacraft_calc::changes_between(before, after, page) };
    let mut index = kept.lock().unwrap_or_else(PoisonError::into_inner);
    changes_between_with(&mut index, before, after, page)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An entry that ends restores what was there, so a command that runs a command is not left with none.
    #[test]
    fn entering_inside_an_entry_gives_the_outer_one_back() {
        let (outer, inner) = (KeptIndex::default(), KeptIndex::default());
        let ptr = |kept: &KeptIndex| Arc::as_ptr(&kept.0);
        let current = || CURRENT.with(|c| c.borrow().as_ref().map(Arc::as_ptr));
        assert_eq!(current(), None);
        let first = InUse::enter(&outer);
        assert_eq!(current(), Some(ptr(&outer)));
        {
            let _second = InUse::enter(&inner);
            assert_eq!(current(), Some(ptr(&inner)));
        }
        assert_eq!(current(), Some(ptr(&outer)));
        drop(first);
        assert_eq!(current(), None);
    }
}
