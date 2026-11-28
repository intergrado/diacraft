//! Counts of the work the document model does, for the tests that hold a cost to a count: a count does not move with the load on the machine,
//! as a time does, and it tells a lookup built once from one built for each read where a time cannot.
//!
//! A [`Resolver`](crate::Resolver) keeps its own counts of what it built ([`ResolverStats`](crate::ResolverStats)); these are the totals over
//! every resolver of the thread, so that a test can count what a write built (a write makes its resolver inside), and the two scans a test
//! holds to "none": a section looked for among many, and a table of the rows of a large section built.
//!
//! Compiled only with the `test-support` feature, which this crate's own tests turn on; shipped code carries no counter, and the calls that
//! bump one compile to nothing. The counts are of the calling thread.

#[cfg(feature = "test-support")]
mod on {
    use std::cell::Cell;

    thread_local! {
        static RESOLVER_BUILDS: Cell<u64> = const { Cell::new(0) };
        static ROW_TABLES: Cell<u64> = const { Cell::new(0) };
        static SECTIONS_EXAMINED: Cell<u64> = const { Cell::new(0) };
        static ROWS_MERGED: Cell<u64> = const { Cell::new(0) };
        static SECTIONS_MERGED: Cell<u64> = const { Cell::new(0) };
        static FINDS_VISITED: Cell<u64> = const { Cell::new(0) };
        static LIMIT_ITEMS: Cell<u64> = const { Cell::new(0) };
        static STAMP_ITEMS: Cell<u64> = const { Cell::new(0) };
        static PLACES_READ: Cell<u64> = const { Cell::new(0) };
        static FOLLOW_KEYS: Cell<u64> = const { Cell::new(0) };
    }

    /// Starts every count over.
    pub fn reset() {
        RESOLVER_BUILDS.with(|c| c.set(0));
        ROW_TABLES.with(|c| c.set(0));
        SECTIONS_EXAMINED.with(|c| c.set(0));
        ROWS_MERGED.with(|c| c.set(0));
        SECTIONS_MERGED.with(|c| c.set(0));
        FINDS_VISITED.with(|c| c.set(0));
        LIMIT_ITEMS.with(|c| c.set(0));
        STAMP_ITEMS.with(|c| c.set(0));
        PLACES_READ.with(|c| c.set(0));
        FOLLOW_KEYS.with(|c| c.set(0));
    }

    /// The lookups and walks resolvers built, over all resolvers (each is one bump of a [`ResolverStats`](crate::ResolverStats) count).
    #[must_use]
    pub fn resolver_builds() -> u64 {
        RESOLVER_BUILDS.with(Cell::get)
    }

    /// The tables of the rows of a large section that were built.
    #[must_use]
    pub fn row_tables_built() -> u64 {
        ROW_TABLES.with(Cell::get)
    }

    /// The sections looked at by a scan of a sheet's sections for one key.
    #[must_use]
    pub fn sections_examined() -> u64 {
        SECTIONS_EXAMINED.with(Cell::get)
    }

    /// The rows that `Resolver::effective_rows` walked to merge a section across a shape, its master and the styles: every row of every layer, and so the
    /// cost of reading a section whose rows an instance inherits (64 instances of a master of 100,000 rows read 6.4 million).
    #[must_use]
    pub fn rows_merged() -> u64 {
        ROWS_MERGED.with(Cell::get)
    }

    /// The sections that `Resolver::effective_sections` walked to merge the sections of one name across a shape, its master and the styles: every section of
    /// that name in every layer, and so the cost of listing the `Geometry` sections of an instance of a master that holds thousands (1,000 guides of
    /// such a master were a second for each page read).
    #[must_use]
    pub fn sections_merged() -> u64 {
        SECTIONS_MERGED.with(Cell::get)
    }

    /// The shapes `Document::find_shape` visited looking for a shape by its `ID`, over every search: a search walks the page from the top, so a command that
    /// looks up N shapes on a page of M costs about N times M (a guide move that looked up each shape the guide carries).
    #[must_use]
    pub fn finds_visited() -> u64 {
        FINDS_VISITED.with(Cell::get)
    }

    /// The held definitions `Document::check_limits` counted: one look at each, ending at the first past a cap, and none when the document
    /// holds no ODF data. The parts are a length and the held values on shapes are not read, so neither is counted.
    #[must_use]
    pub fn limit_items() -> u64 {
        LIMIT_ITEMS.with(Cell::get)
    }

    /// The units of work the stamps did (`stamp_cells`, `stamp_row`, `stamp_sections`): each name looked up, each section looked at, and each row and cell
    /// fed. A stamp's cost is this, and never a pass over a sheet for each name it is asked for.
    #[must_use]
    pub fn stamp_items() -> u64 {
        STAMP_ITEMS.with(Cell::get)
    }

    /// The places of unknown nodes that `Passthrough::reposition` read: each call reads every node of the list it is given once, so the count is the nodes of
    /// the lists times the calls, and a copy of thousands of rows into a section that holds a few nodes reads a few times those, not thousands of times.
    #[must_use]
    pub fn places_read() -> u64 {
        PLACES_READ.with(Cell::get)
    }

    pub(crate) fn place_reads(nodes: usize) {
        PLACES_READ.with(|c| c.set(c.get().saturating_add(nodes as u64)));
    }

    /// The keys of children that `Passthrough::children_followed` read: the old list and the new one, once each, for a list that holds an unknown node, and nothing for
    /// one that holds none.
    #[must_use]
    pub fn follow_keys() -> u64 {
        FOLLOW_KEYS.with(Cell::get)
    }

    pub(crate) fn follow_key_reads(keys: usize) {
        FOLLOW_KEYS.with(|c| c.set(c.get().saturating_add(keys as u64)));
    }

    pub(crate) fn limit_item() {
        LIMIT_ITEMS.with(|c| c.set(c.get().saturating_add(1)));
    }

    pub(crate) fn stamp_item() {
        STAMP_ITEMS.with(|c| c.set(c.get().saturating_add(1)));
    }

    pub(crate) fn find_visit() {
        FINDS_VISITED.with(|c| c.set(c.get().saturating_add(1)));
    }

    pub(crate) fn sections_merge(sections: usize) {
        SECTIONS_MERGED.with(|c| c.set(c.get().saturating_add(sections as u64)));
    }

    pub(crate) fn rows_merge(rows: usize) {
        ROWS_MERGED.with(|c| c.set(c.get().saturating_add(rows as u64)));
    }

    pub(crate) fn resolver_built() {
        RESOLVER_BUILDS.with(|c| c.set(c.get().saturating_add(1)));
    }

    pub(crate) fn row_table_built() {
        ROW_TABLES.with(|c| c.set(c.get().saturating_add(1)));
    }

    pub(crate) fn section_examined() {
        SECTIONS_EXAMINED.with(|c| c.set(c.get().saturating_add(1)));
    }
}

#[cfg(feature = "test-support")]
pub(crate) use on::{
    find_visit, follow_key_reads, limit_item, place_reads, resolver_built, row_table_built, rows_merge, section_examined, sections_merge, stamp_item,
};
#[cfg(feature = "test-support")]
pub use on::{
    finds_visited, follow_keys, limit_items, places_read, reset, resolver_builds, row_tables_built, rows_merged, sections_examined, sections_merged,
    stamp_items,
};

#[cfg(not(feature = "test-support"))]
pub(crate) fn find_visit() {}
#[cfg(not(feature = "test-support"))]
pub(crate) fn resolver_built() {}
#[cfg(not(feature = "test-support"))]
pub(crate) fn row_table_built() {}
#[cfg(not(feature = "test-support"))]
pub(crate) fn section_examined() {}
#[cfg(not(feature = "test-support"))]
pub(crate) fn rows_merge(_rows: usize) {}
#[cfg(not(feature = "test-support"))]
pub(crate) fn sections_merge(_sections: usize) {}
#[cfg(not(feature = "test-support"))]
pub(crate) fn limit_item() {}
#[cfg(not(feature = "test-support"))]
pub(crate) fn stamp_item() {}
#[cfg(not(feature = "test-support"))]
pub(crate) fn place_reads(_nodes: usize) {}
#[cfg(not(feature = "test-support"))]
pub(crate) fn follow_key_reads(_keys: usize) {}
