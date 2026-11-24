//! Counts of the work the reader does on hostile input, for the tests that hold a cost to a count: a count does not move with the load on the
//! machine, as a time does.
//!
//! The reader's own measure of work is its budget (`limits`): every cost that can grow with a record is charged to it before it is paid, and a
//! cap that is reached is reported. These count what the budget does not see: the points of a record gathered into a vector, the slots of an
//! object table passed over, the rectangles of a region gathered, which are the costs a test holds to "not at all" or "once for each".
//!
//! Compiled only with the `test-support` feature, which this crate's own tests turn on; shipped code carries no counter, and the calls that
//! bump one compile to nothing. The counts are of the calling thread, and the reader works on the thread that calls it.

#[cfg(feature = "test-support")]
mod on {
    use std::cell::Cell;

    thread_local! {
        static POINTS_GATHERED: Cell<u64> = const { Cell::new(0) };
        static SLOTS_PASSED: Cell<u64> = const { Cell::new(0) };
        static REGION_RECTS_GATHERED: Cell<u64> = const { Cell::new(0) };
        static CLIP_PARTS_COPIED: Cell<u64> = const { Cell::new(0) };
        static CLIP_POINTS_TRANSFORMED: Cell<u64> = const { Cell::new(0) };
        static PEAK_BRACKET_POINTS: Cell<u64> = const { Cell::new(0) };
        static OBJECT_POINTS_MADE: Cell<u64> = const { Cell::new(0) };
    }

    /// Starts every count over.
    pub fn reset() {
        POINTS_GATHERED.with(|c| c.set(0));
        SLOTS_PASSED.with(|c| c.set(0));
        REGION_RECTS_GATHERED.with(|c| c.set(0));
        CLIP_PARTS_COPIED.with(|c| c.set(0));
        CLIP_POINTS_TRANSFORMED.with(|c| c.set(0));
        PEAK_BRACKET_POINTS.with(|c| c.set(0));
        OBJECT_POINTS_MADE.with(|c| c.set(0));
    }

    /// The points of poly records that were gathered into a vector.
    #[must_use]
    pub fn points_gathered() -> u64 {
        POINTS_GATHERED.with(Cell::get)
    }

    /// The occupied slots a WMF object that takes the first free slot passed over to find it.
    #[must_use]
    pub fn slots_passed() -> u64 {
        SLOTS_PASSED.with(Cell::get)
    }

    /// The rectangles of regions that were gathered into a vector.
    #[must_use]
    pub fn region_rects_gathered() -> u64 {
        REGION_RECTS_GATHERED.with(Cell::get)
    }

    /// The parts of clips copied to make the clips that combinations gave.
    #[must_use]
    pub fn clip_parts_copied() -> u64 {
        CLIP_PARTS_COPIED.with(Cell::get)
    }

    /// The points of EMF+ path objects moved into the frame to be a clip.
    #[must_use]
    pub fn clip_points_transformed() -> u64 {
        CLIP_POINTS_TRANSFORMED.with(Cell::get)
    }

    /// The most operations a path bracket held at once.
    #[must_use]
    pub fn peak_bracket_points() -> u64 {
        PEAK_BRACKET_POINTS.with(Cell::get)
    }

    /// The points of the EMF+ path objects that were made and kept.
    #[must_use]
    pub fn object_points_made() -> u64 {
        OBJECT_POINTS_MADE.with(Cell::get)
    }

    pub(crate) fn clip_parts(n: usize) {
        CLIP_PARTS_COPIED.with(|c| c.set(c.get().saturating_add(n as u64)));
    }

    pub(crate) fn clip_points_moved(n: usize) {
        CLIP_POINTS_TRANSFORMED.with(|c| c.set(c.get().saturating_add(n as u64)));
    }

    pub(crate) fn bracket_points_held(n: usize) {
        PEAK_BRACKET_POINTS.with(|c| c.set(c.get().max(n as u64)));
    }

    pub(crate) fn object_points(n: u64) {
        OBJECT_POINTS_MADE.with(|c| c.set(c.get().saturating_add(n)));
    }

    pub(crate) fn points(n: usize) {
        POINTS_GATHERED.with(|c| c.set(c.get().saturating_add(n as u64)));
    }

    pub(crate) fn slot_passed() {
        SLOTS_PASSED.with(|c| c.set(c.get().saturating_add(1)));
    }

    pub(crate) fn region_rects(n: usize) {
        REGION_RECTS_GATHERED.with(|c| c.set(c.get().saturating_add(n as u64)));
    }
}

#[cfg(feature = "test-support")]
pub(crate) use on::{bracket_points_held, clip_parts, clip_points_moved, object_points, points, region_rects, slot_passed};
#[cfg(feature = "test-support")]
pub use on::{
    clip_parts_copied, clip_points_transformed, object_points_made, peak_bracket_points, points_gathered, region_rects_gathered, reset, slots_passed,
};

#[cfg(not(feature = "test-support"))]
pub(crate) fn points(_: usize) {}
#[cfg(not(feature = "test-support"))]
pub(crate) fn slot_passed() {}
#[cfg(not(feature = "test-support"))]
pub(crate) fn region_rects(_: usize) {}
#[cfg(not(feature = "test-support"))]
pub(crate) fn clip_parts(_: usize) {}
#[cfg(not(feature = "test-support"))]
pub(crate) fn clip_points_moved(_: usize) {}
#[cfg(not(feature = "test-support"))]
pub(crate) fn bracket_points_held(_: usize) {}
#[cfg(not(feature = "test-support"))]
pub(crate) fn object_points(_: u64) {}
