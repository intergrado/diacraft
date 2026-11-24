//! Counts of the recalculation the commits of this thread have made, for the tests of the crates above that hold what an edit costs to a count: a
//! count does not move with the load on the machine, as a time does, and it tells a recalculation of the cells an edit changed from one of the
//! whole page, which a time on a small page cannot.
//!
//! A commit reports its own steps ([`CommitReport`](crate::CommitReport)); these are the totals over every commit the thread made, for the callers
//! that discard the report (the engine's editing commands make theirs inside).
//!
//! Compiled only with the `test-support` feature, which this crate's own tests and the tests of the engine turn on; shipped code carries no counter,
//! and the calls that bump one compile to nothing. The counts are of the calling thread.

#[cfg(feature = "test-support")]
mod on {
    use std::cell::Cell;

    thread_local! {
        static COMMITS: Cell<u64> = const { Cell::new(0) };
        static STEPS: Cell<u64> = const { Cell::new(0) };
        static ASKED: Cell<u64> = const { Cell::new(0) };
        static CELLS_WALKED: Cell<u64> = const { Cell::new(0) };
    }

    /// Starts every count over.
    pub fn reset() {
        COMMITS.with(|c| c.set(0));
        STEPS.with(|c| c.set(0));
        ASKED.with(|c| c.set(0));
        CELLS_WALKED.with(|c| c.set(0));
    }

    /// The commits made (a commit of the cells an edit changed counts one, and so does a commit of a whole page).
    #[must_use]
    pub fn commits() -> u64 {
        COMMITS.with(Cell::get)
    }

    /// The steps those commits took in working out cells (`EvalStats::steps`).
    #[must_use]
    pub fn steps() -> u64 {
        STEPS.with(Cell::get)
    }

    /// The formulas the dependency walk asked whether they read beyond their own sheet (a walk of the formulas of a page asks every one of them; an
    /// edit that works from the index kept from the edit before asks the formulas of the sheets it replaced, and those only): what the edit's
    /// dependency work is counted by, since a time moves with the load.
    #[must_use]
    pub fn formulas_asked() -> u64 {
        ASKED.with(Cell::get)
    }

    /// The cells the walks of a sheet's cells passed over (each cell of each sheet the dependency walk, the index and the comparison of two versions of a sheet
    /// looked at, formula or none): a sheet whose sections hold thousands of rows of constants costs those rows each time it is walked.
    #[must_use]
    pub fn cells_walked() -> u64 {
        CELLS_WALKED.with(Cell::get)
    }

    pub(crate) fn cells_walk(n: usize) {
        CELLS_WALKED.with(|c| c.set(c.get().saturating_add(n as u64)));
    }

    pub(crate) fn asked() {
        ASKED.with(|c| c.set(c.get().saturating_add(1)));
    }

    pub(crate) fn committed(steps: u64) {
        COMMITS.with(|c| c.set(c.get().saturating_add(1)));
        STEPS.with(|c| c.set(c.get().saturating_add(steps)));
    }
}

#[cfg(feature = "test-support")]
pub(crate) use on::{asked, cells_walk, committed};
#[cfg(feature = "test-support")]
pub use on::{cells_walked, commits, formulas_asked, reset, steps};

#[cfg(not(feature = "test-support"))]
pub(crate) fn committed(_: u64) {}
#[cfg(not(feature = "test-support"))]
pub(crate) fn asked() {}
#[cfg(not(feature = "test-support"))]
pub(crate) fn cells_walk(_: usize) {}
