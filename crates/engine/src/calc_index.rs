//! The readers of the cells of the open drawing's page, kept from one edit to the next.
//!
//! To say which cells an edit reaches, `calc` needs the cells that read a cell through another sheet. Found by asking every formula of the page, that was
//! most of what an edit cost on a page of twelve thousand shapes. `calc` keeps the answer in a [`ReaderIndex`] and puts right what an edit replaced
//! (`diacraft_calc::commit_changed_with`); the index belongs to **the session** and is given to every commit the session's editing commands make, which are
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

use std::cell::RefCell;
use std::sync::{Arc, Mutex, PoisonError};

use diacraft_calc::{Budget, Changes, CommitReport, EvalOptions, ReaderIndex, changes_between_with, commit_changed_with};
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

/// `diacraft_calc::commit_changed` for what an editing command changed: with the index of the session that is editing, if one is.
pub(crate) fn commit_changed(doc: &mut Document, budget: &Budget, options: EvalOptions, page: u32, changes: &Changes) -> CommitReport {
    let Some(kept) = current() else { return diacraft_calc::commit_changed(doc, budget, options, page, changes) };
    // A panic in a commit leaves a lock poisoned and the index with nothing in it (`ReaderIndex::sync` takes the graph out before it works on it), which the next
    // commit makes again: the poison says nothing that is not already true.
    let mut index = kept.lock().unwrap_or_else(PoisonError::into_inner);
    commit_changed_with(&mut index, doc, budget, options, page, changes)
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
