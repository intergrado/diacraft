//! Counts of the work a layout does, for the tests that hold a cost to a count. A count does not move with the load on the machine, as a
//! time does, and it tells a quadratic step from a linear one at a size where a time cannot.
//!
//! Compiled only with the `test-support` feature, which this crate's own tests turn on; shipped code carries no counter, and the calls
//! that bump one compile to nothing. The counts are of the calling thread, which is where a layout runs.

#[cfg(feature = "test-support")]
mod on {
    use std::cell::Cell;

    thread_local! {
        static CLUSTER_STEPS: Cell<u64> = const { Cell::new(0) };
        static SHAPER_CALLS: Cell<u64> = const { Cell::new(0) };
        static FACE_SEARCHES: Cell<u64> = const { Cell::new(0) };
        static GLYPH_COPIES: Cell<u64> = const { Cell::new(0) };
    }

    /// Starts every count over.
    pub fn reset() {
        CLUSTER_STEPS.with(|c| c.set(0));
        SHAPER_CALLS.with(|c| c.set(0));
        FACE_SEARCHES.with(|c| c.set(0));
        GLYPH_COPIES.with(|c| c.set(0));
    }

    /// The clusters the line breaker looked at (one for each step of a line, and one for each cluster a tab's segment scans).
    #[must_use]
    pub fn cluster_steps() -> u64 {
        CLUSTER_STEPS.with(Cell::get)
    }

    /// The calls made to a face's shaper.
    #[must_use]
    pub fn shaper_calls() -> u64 {
        SHAPER_CALLS.with(Cell::get)
    }

    /// The searches of the whole font database for a face that has a cluster's glyphs.
    #[must_use]
    pub fn face_searches() -> u64 {
        FACE_SEARCHES.with(Cell::get)
    }

    /// The placed glyphs the turn of vertical text copied: one for each glyph it places, and every glyph of a run it copies whole.
    #[must_use]
    pub fn glyph_copies() -> u64 {
        GLYPH_COPIES.with(Cell::get)
    }

    pub(crate) fn cluster_step() {
        CLUSTER_STEPS.with(|c| c.set(c.get().saturating_add(1)));
    }

    pub(crate) fn glyph_copy(n: usize) {
        GLYPH_COPIES.with(|c| c.set(c.get().saturating_add(n as u64)));
    }

    pub(crate) fn shaper_call() {
        SHAPER_CALLS.with(|c| c.set(c.get().saturating_add(1)));
    }

    pub(crate) fn face_search() {
        FACE_SEARCHES.with(|c| c.set(c.get().saturating_add(1)));
    }
}

#[cfg(feature = "test-support")]
pub(crate) use on::{cluster_step, face_search, glyph_copy, shaper_call};
#[cfg(feature = "test-support")]
pub use on::{cluster_steps, face_searches, glyph_copies, reset, shaper_calls};

#[cfg(not(feature = "test-support"))]
pub(crate) fn cluster_step() {}
#[cfg(not(feature = "test-support"))]
pub(crate) fn shaper_call() {}
#[cfg(not(feature = "test-support"))]
pub(crate) fn face_search() {}
#[cfg(not(feature = "test-support"))]
pub(crate) fn glyph_copy(_n: usize) {}
