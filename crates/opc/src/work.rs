//! Counts of the work the reader does on hostile input, for the tests that hold a cost to a count: a count does not move with the load on the
//! machine, as a time does, and a ratio of two counts is exact where a ratio of two times is not.
//!
//! Compiled only with the `test-support` feature, which this crate's own tests turn on; shipped code carries no counter, and the calls that
//! bump one compile to nothing. The counts are of the calling thread, and the reader works on the thread that calls it.

#[cfg(feature = "test-support")]
mod on {
    use std::cell::Cell;

    thread_local! {
        static NAME_COMPARISONS: Cell<u64> = const { Cell::new(0) };
        static NAME_BYTES: Cell<u64> = const { Cell::new(0) };
    }

    /// Starts every count over.
    pub fn reset() {
        NAME_COMPARISONS.with(|c| c.set(0));
        NAME_BYTES.with(|c| c.set(0));
    }

    /// The comparisons of two part names made by the check that no name is the prefix of another (its sort).
    #[must_use]
    pub fn name_comparisons() -> u64 {
        NAME_COMPARISONS.with(Cell::get)
    }

    /// The bytes of part names that check looked at, in its comparisons and in the walk that follows them.
    #[must_use]
    pub fn name_bytes() -> u64 {
        NAME_BYTES.with(Cell::get)
    }

    pub(crate) fn name_comparison(bytes: usize) {
        NAME_COMPARISONS.with(|c| c.set(c.get().saturating_add(1)));
        name_bytes_looked_at(bytes);
    }

    pub(crate) fn name_bytes_looked_at(bytes: usize) {
        NAME_BYTES.with(|c| c.set(c.get().saturating_add(bytes as u64)));
    }
}

#[cfg(feature = "test-support")]
pub use on::{name_bytes, name_comparisons, reset};
#[cfg(feature = "test-support")]
pub(crate) use on::{name_bytes_looked_at, name_comparison};

#[cfg(not(feature = "test-support"))]
pub(crate) fn name_comparison(_: usize) {}
#[cfg(not(feature = "test-support"))]
pub(crate) fn name_bytes_looked_at(_: usize) {}
