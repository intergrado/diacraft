//! Counts of the work the writer does on a package of many parts, and the reader does to hold the parts the model has no field for, for the
//! tests that hold a cost to a count: a count does not move with the load on the machine, as a time does, and a ratio of two timings (a save that keeps a macro
//! project against one that drops it) is a measure of the machine as much as of the code.
//!
//! Compiled only with the `test-support` feature, which this crate's own tests turn on; shipped code carries no counter, and the calls that bump
//! one compile to nothing. The counts are of the calling thread, and the writer works on the thread that calls it.

#[cfg(feature = "test-support")]
mod on {
    use std::cell::Cell;
    use std::sync::atomic::AtomicU64;

    static LOOKAHEAD_VIOLATIONS: AtomicU64 = AtomicU64::new(0);

    thread_local! {
        static WAITS_FOR_AHEAD: Cell<bool> = const { Cell::new(false) };
        static RELS_VISITED: Cell<u64> = const { Cell::new(0) };
        static OVERRIDES_VISITED: Cell<u64> = const { Cell::new(0) };
        static PARTS_VISITED: Cell<u64> = const { Cell::new(0) };
        static NAME_PROBES: Cell<u64> = const { Cell::new(0) };
        static ID_SCANS: Cell<u64> = const { Cell::new(0) };
        static THEME_BYTES: Cell<u64> = const { Cell::new(0) };
        static COMMENT_ENTRIES: Cell<u64> = const { Cell::new(0) };
        static TYPE_SCANS: Cell<u64> = const { Cell::new(0) };
        static HELD_PARTS_WALKED: Cell<u64> = const { Cell::new(0) };
        static HELD_ROWS_WALKED: Cell<u64> = const { Cell::new(0) };
        static HELD_BYTES_COPIED: Cell<u64> = const { Cell::new(0) };
        static RELS_PARSED: Cell<u64> = const { Cell::new(0) };
        static TYPE_ROWS_WALKED: Cell<u64> = const { Cell::new(0) };
        static MODEL_PARTS: Cell<u64> = const { Cell::new(0) };
        static MODEL_ROWS: Cell<u64> = const { Cell::new(0) };
        static MODEL_BYTES: Cell<u64> = const { Cell::new(0) };
        static MODEL_TYPES: Cell<u64> = const { Cell::new(0) };
        static CARRY_WALKED: Cell<u64> = const { Cell::new(0) };
        static CARRY_STAMPED: Cell<u64> = const { Cell::new(0) };
        static CARRY_JUDGED: Cell<u64> = const { Cell::new(0) };
        static CARRY_BLOB_BYTES: Cell<u64> = const { Cell::new(0) };
        static CARRY_PARTS: Cell<u64> = const { Cell::new(0) };
        static XML_EVENTS: Cell<u64> = const { Cell::new(0) };
        static CARRY_COPIED: Cell<u64> = const { Cell::new(0) };
        static AHEAD_USED: Cell<u64> = const { Cell::new(0) };
        static AHEAD_ABANDONED: Cell<u64> = const { Cell::new(0) };
        static AHEAD_STOPPED: Cell<u64> = const { Cell::new(0) };
        static OBJECT_FRAMES: Cell<u64> = const { Cell::new(0) };
        static OBJECT_PARTS: Cell<u64> = const { Cell::new(0) };
        static OBJECT_BYTES_READ: Cell<u64> = const { Cell::new(0) };
        static OBJECT_BYTES_COPIED: Cell<u64> = const { Cell::new(0) };
    }

    /// Starts every count over.
    pub fn reset() {
        RELS_VISITED.with(|c| c.set(0));
        OVERRIDES_VISITED.with(|c| c.set(0));
        PARTS_VISITED.with(|c| c.set(0));
        NAME_PROBES.with(|c| c.set(0));
        ID_SCANS.with(|c| c.set(0));
        THEME_BYTES.with(|c| c.set(0));
        COMMENT_ENTRIES.with(|c| c.set(0));
        TYPE_SCANS.with(|c| c.set(0));
        HELD_PARTS_WALKED.with(|c| c.set(0));
        HELD_ROWS_WALKED.with(|c| c.set(0));
        HELD_BYTES_COPIED.with(|c| c.set(0));
        RELS_PARSED.with(|c| c.set(0));
        TYPE_ROWS_WALKED.with(|c| c.set(0));
        MODEL_PARTS.with(|c| c.set(0));
        MODEL_ROWS.with(|c| c.set(0));
        MODEL_BYTES.with(|c| c.set(0));
        MODEL_TYPES.with(|c| c.set(0));
        CARRY_WALKED.with(|c| c.set(0));
        CARRY_STAMPED.with(|c| c.set(0));
        CARRY_JUDGED.with(|c| c.set(0));
        CARRY_BLOB_BYTES.with(|c| c.set(0));
        CARRY_PARTS.with(|c| c.set(0));
        XML_EVENTS.with(|c| c.set(0));
        CARRY_COPIED.with(|c| c.set(0));
        AHEAD_USED.with(|c| c.set(0));
        AHEAD_ABANDONED.with(|c| c.set(0));
        AHEAD_STOPPED.with(|c| c.set(0));
        OBJECT_FRAMES.with(|c| c.set(0));
        OBJECT_PARTS.with(|c| c.set(0));
        OBJECT_BYTES_READ.with(|c| c.set(0));
        OBJECT_BYTES_COPIED.with(|c| c.set(0));
    }

    /// The trees the read took from the parts parsed ahead on other threads instead of parsing them itself: none at one thread.
    #[must_use]
    pub fn trees_taken_ahead() -> u64 {
        AHEAD_USED.with(Cell::get)
    }

    /// The trees parsed ahead that the read never asked for and let go of when it ended the work: parts of rows it skipped, or that it was past when the budget ended the work.
    #[must_use]
    pub fn trees_abandoned() -> u64 {
        AHEAD_ABANDONED.with(Cell::get)
    }

    /// The times a charge or a parse of the read's own ended the work ahead because the memory it would hold was needed.
    #[must_use]
    pub fn works_ended_for_memory() -> u64 {
        AHEAD_STOPPED.with(Cell::get)
    }

    pub(crate) fn ahead_abandoned(n: usize) {
        AHEAD_ABANDONED.with(|c| c.set(c.get().saturating_add(n as u64)));
    }

    pub(crate) fn ahead_stopped() {
        AHEAD_STOPPED.with(|c| c.set(c.get().saturating_add(1)));
    }

    pub(crate) fn ahead_used() {
        AHEAD_USED.with(|c| c.set(c.get().saturating_add(1)));
    }

    /// How many times, in any read of this process, the memory the work ahead of a read held was **more than the budget had set aside for it**: none, whatever a read abandons.
    #[must_use]
    pub fn lookahead_violations() -> u64 {
        LOOKAHEAD_VIOLATIONS.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub(crate) fn lookahead_violation() {
        LOOKAHEAD_VIOLATIONS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    /// For a test that needs a part parsed on a thread of the work ahead (one that makes the thread panic): **a read on this thread waits for the threads to parse every part
    /// it asked them for**, instead of parsing a part itself when no thread has begun it. Which thread parses a part is otherwise the system's to decide: on one core the read came to a part
    /// before any thread had begun it. A read ends the same either way; only the thread that parses moves. Off until it is turned on, on this thread only.
    pub fn wait_for_the_work_ahead(on: bool) {
        WAITS_FOR_AHEAD.with(|c| c.set(on));
    }

    pub(crate) fn waits_for_the_work_ahead() -> bool {
        WAITS_FOR_AHEAD.with(Cell::get)
    }

    /// What the judge of the ODF object frames looked at: each shape once, and each node of a shape's kept list at most once, and each node of an inline object's data.
    #[must_use]
    pub fn object_frames_walked() -> u64 {
        OBJECT_FRAMES.with(Cell::get)
    }

    /// The parts of the model put in the map the judge looks an object's entry up in: once each, and only when a frame names an entry.
    #[must_use]
    pub fn object_parts_indexed() -> u64 {
        OBJECT_PARTS.with(Cell::get)
    }

    /// The bytes of objects the check of a compound file looked at: the header and one directory entry of each object judged, whatever its size.
    #[must_use]
    pub fn object_bytes_checked() -> u64 {
        OBJECT_BYTES_READ.with(Cell::get)
    }

    /// The bytes of objects the writers made a copy of: the decoded bytes of an inline object. An object that is a part of the model is written from it, and none is copied.
    #[must_use]
    pub fn object_bytes_decoded() -> u64 {
        OBJECT_BYTES_COPIED.with(Cell::get)
    }

    pub(crate) fn object_frames(n: usize) {
        OBJECT_FRAMES.with(|c| c.set(c.get().saturating_add(n as u64)));
    }

    pub(crate) fn object_parts(n: usize) {
        OBJECT_PARTS.with(|c| c.set(c.get().saturating_add(n as u64)));
    }

    pub(crate) fn object_bytes_read(n: usize) {
        OBJECT_BYTES_READ.with(|c| c.set(c.get().saturating_add(n as u64)));
    }

    pub(crate) fn object_bytes_copied(n: usize) {
        OBJECT_BYTES_COPIED.with(|c| c.set(c.get().saturating_add(n as u64)));
    }

    /// The nodes of sheets and connects the judge charged for copying where an item put data on one (each put once, however many items put data on the one sheet).
    #[must_use]
    pub fn carry_nodes_copied() -> u64 {
        CARRY_COPIED.with(Cell::get)
    }

    pub(crate) fn carry_copied(n: usize) {
        CARRY_COPIED.with(|c| c.set(c.get().saturating_add(n as u64)));
    }

    /// The objects of the model the carrier looked at to find what rides: a shape, a sheet, a section, a row, a cell, a part, a custom property and a face, each time it
    /// was looked at, and a shape's twice when its page has items (the second walk finds a key two shapes share).
    #[must_use]
    pub fn carry_objects_walked() -> u64 {
        CARRY_WALKED.with(Cell::get)
    }

    /// The nodes, cells, rows and sections the carrier hashed to stamp an item: each once, and a sheet shared by many shapes once.
    #[must_use]
    pub fn carry_items_stamped() -> u64 {
        CARRY_STAMPED.with(Cell::get)
    }

    /// The items the reader's judge looked at: once each, whatever it did with them.
    #[must_use]
    pub fn carry_items_judged() -> u64 {
        CARRY_JUDGED.with(Cell::get)
    }

    /// The bytes of parts of DiaCraft's that the reader copied into the model: the bytes of the parts it kept, once each.
    #[must_use]
    pub fn carry_bytes_copied() -> u64 {
        CARRY_BLOB_BYTES.with(Cell::get)
    }

    /// The parts of DiaCraft's the reader looked at to read: once each.
    #[must_use]
    pub fn carry_parts_read() -> u64 {
        CARRY_PARTS.with(Cell::get)
    }

    /// The events the XML parser read in the parts of the document the reader parses (the document, the indexes, the pages and masters, the themes, the properties and the DiaCraft part).
    /// A part the reader does not parse (a macro project, an embedded object, a part of DiaCraft's) adds none.
    #[must_use]
    pub fn xml_events_read() -> u64 {
        XML_EVENTS.with(Cell::get)
    }

    pub(crate) fn carry_walked(n: usize) {
        CARRY_WALKED.with(|c| c.set(c.get().saturating_add(n as u64)));
    }

    pub(crate) fn carry_stamped(n: usize) {
        CARRY_STAMPED.with(|c| c.set(c.get().saturating_add(n as u64)));
    }

    pub(crate) fn carry_judged(n: usize) {
        CARRY_JUDGED.with(|c| c.set(c.get().saturating_add(n as u64)));
    }

    pub(crate) fn carry_blob_bytes(n: usize) {
        CARRY_BLOB_BYTES.with(|c| c.set(c.get().saturating_add(n as u64)));
    }

    pub(crate) fn carry_part_read(n: usize) {
        CARRY_PARTS.with(|c| c.set(c.get().saturating_add(n as u64)));
    }

    pub(crate) fn xml_events(n: u64) {
        XML_EVENTS.with(|c| c.set(c.get().saturating_add(n)));
    }

    /// The parts the model writer looked at to plan and write them: once for each part it writes or leaves out, however many rows and rows of types there are.
    #[must_use]
    pub fn model_parts_planned() -> u64 {
        MODEL_PARTS.with(Cell::get)
    }

    /// The relationship rows the model writer made a row of or looked at to place: once for each row it holds, and for each row it makes.
    #[must_use]
    pub fn model_rows_walked() -> u64 {
        MODEL_ROWS.with(Cell::get)
    }

    /// The bytes of held parts the model writer handed to the package: the bytes of those parts, once, written from the model's own bytes and copied nowhere else.
    #[must_use]
    pub fn model_bytes_written() -> u64 {
        MODEL_BYTES.with(Cell::get)
    }

    /// The rows of the content types the model writer decided: once for each part.
    #[must_use]
    pub fn model_types_decided() -> u64 {
        MODEL_TYPES.with(Cell::get)
    }

    pub(crate) fn model_part() {
        MODEL_PARTS.with(|c| c.set(c.get().saturating_add(1)));
    }

    pub(crate) fn model_row() {
        MODEL_ROWS.with(|c| c.set(c.get().saturating_add(1)));
    }

    pub(crate) fn model_bytes(n: usize) {
        MODEL_BYTES.with(|c| c.set(c.get().saturating_add(n as u64)));
    }

    pub(crate) fn model_type() {
        MODEL_TYPES.with(|c| c.set(c.get().saturating_add(1)));
    }

    /// The rows of relationships parts visited by the removal of rows by `Id`.
    #[must_use]
    pub fn rels_visited() -> u64 {
        RELS_VISITED.with(Cell::get)
    }

    /// The rows of the content types visited by the removal of `Override`s.
    #[must_use]
    pub fn overrides_visited() -> u64 {
        OVERRIDES_VISITED.with(Cell::get)
    }

    /// The parts named to the removal of many parts at once.
    #[must_use]
    pub fn parts_visited() -> u64 {
        PARTS_VISITED.with(Cell::get)
    }

    /// The names tried for a new part before one was free.
    #[must_use]
    pub fn name_probes() -> u64 {
        NAME_PROBES.with(Cell::get)
    }

    /// The rows of a relationships part looked at to find the first free `rId`.
    #[must_use]
    pub fn id_scans() -> u64 {
        ID_SCANS.with(Cell::get)
    }

    /// The bytes of theme parts hashed to pair the model's themes with the baseline's by their bytes (`write/themes.rs`): each theme is hashed at most
    /// twice, once to be found and once to look for it, so the count is at most twice the bytes of the themes.
    #[must_use]
    pub fn theme_bytes_hashed() -> u64 {
        THEME_BYTES.with(Cell::get)
    }

    pub(crate) fn theme_bytes(n: usize) {
        THEME_BYTES.with(|c| c.set(c.get().saturating_add(n as u64)));
    }

    /// The author and comment entries the reader of the comments part has looked at: one for each, whatever the number of the others.
    #[must_use]
    pub fn comment_entries_read() -> u64 {
        COMMENT_ENTRIES.with(Cell::get)
    }

    pub(crate) fn comment_entry() {
        COMMENT_ENTRIES.with(|c| c.set(c.get().saturating_add(1)));
    }

    /// The rows of the content types looked at to find the `Override` of a part by its name: the rows there when the lookup was first made, and
    /// not the rows for each part.
    #[must_use]
    pub fn type_scans() -> u64 {
        TYPE_SCANS.with(Cell::get)
    }

    /// The parts of the package the reader looked at to decide which it holds: once for each, however many relationships and rows of content types
    /// there are.
    #[must_use]
    pub fn held_parts_walked() -> u64 {
        HELD_PARTS_WALKED.with(Cell::get)
    }

    /// The relationship rows the reader looked at to hold them or to find the role of the part a row names: once for each row of each relationships part
    /// it holds the rows of.
    #[must_use]
    pub fn held_rows_walked() -> u64 {
        HELD_ROWS_WALKED.with(Cell::get)
    }

    /// The bytes copied into held parts: the bytes of those parts, once.
    #[must_use]
    pub fn held_bytes_copied() -> u64 {
        HELD_BYTES_COPIED.with(Cell::get)
    }

    /// The relationships parts the reader parsed (each call, whatever it was for).
    #[must_use]
    pub fn rels_parsed() -> u64 {
        RELS_PARSED.with(Cell::get)
    }

    /// The rows of `[Content_Types].xml` the reader looked at to hold the ones no part needs: once for each.
    #[must_use]
    pub fn type_rows_walked() -> u64 {
        TYPE_ROWS_WALKED.with(Cell::get)
    }

    pub(crate) fn held_part() {
        HELD_PARTS_WALKED.with(|c| c.set(c.get().saturating_add(1)));
    }

    pub(crate) fn held_rows(n: usize) {
        HELD_ROWS_WALKED.with(|c| c.set(c.get().saturating_add(n as u64)));
    }

    pub(crate) fn held_bytes(n: usize) {
        HELD_BYTES_COPIED.with(|c| c.set(c.get().saturating_add(n as u64)));
    }

    pub(crate) fn rels_parse() {
        RELS_PARSED.with(|c| c.set(c.get().saturating_add(1)));
    }

    pub(crate) fn type_rows(n: usize) {
        TYPE_ROWS_WALKED.with(|c| c.set(c.get().saturating_add(n as u64)));
    }

    pub(crate) fn rels(n: usize) {
        RELS_VISITED.with(|c| c.set(c.get().saturating_add(n as u64)));
    }

    pub(crate) fn overrides(n: usize) {
        OVERRIDES_VISITED.with(|c| c.set(c.get().saturating_add(n as u64)));
    }

    pub(crate) fn parts(n: usize) {
        PARTS_VISITED.with(|c| c.set(c.get().saturating_add(n as u64)));
    }

    pub(crate) fn name_probe() {
        NAME_PROBES.with(|c| c.set(c.get().saturating_add(1)));
    }

    pub(crate) fn id_scan(n: usize) {
        ID_SCANS.with(|c| c.set(c.get().saturating_add(n as u64)));
    }

    pub(crate) fn type_scan(n: usize) {
        TYPE_SCANS.with(|c| c.set(c.get().saturating_add(n as u64)));
    }
}

#[cfg(feature = "test-support")]
pub(crate) use on::{
    ahead_abandoned, ahead_stopped, ahead_used, carry_blob_bytes, carry_copied, carry_judged, carry_part_read, carry_stamped, carry_walked,
    comment_entry, held_bytes, held_part, held_rows, id_scan, lookahead_violation, model_bytes, model_part, model_row, model_type, name_probe,
    object_bytes_copied, object_bytes_read, object_frames, object_parts, overrides, parts, rels, rels_parse, theme_bytes, type_rows, type_scan,
    waits_for_the_work_ahead, xml_events,
};
#[cfg(feature = "test-support")]
pub use on::{
    carry_bytes_copied, carry_items_judged, carry_items_stamped, carry_nodes_copied, carry_objects_walked, carry_parts_read, comment_entries_read,
    held_bytes_copied, held_parts_walked, held_rows_walked, id_scans, lookahead_violations, model_bytes_written, model_parts_planned,
    model_rows_walked, model_types_decided, name_probes, object_bytes_checked, object_bytes_decoded, object_frames_walked, object_parts_indexed,
    overrides_visited, parts_visited, rels_parsed, rels_visited, reset, theme_bytes_hashed, trees_abandoned, trees_taken_ahead, type_rows_walked,
    type_scans, wait_for_the_work_ahead, works_ended_for_memory, xml_events_read,
};

/// Shipped code never waits for the work ahead: a part no thread has begun is parsed by the read.
#[cfg(not(feature = "test-support"))]
pub(crate) fn waits_for_the_work_ahead() -> bool {
    false
}
#[cfg(not(feature = "test-support"))]
pub(crate) fn ahead_used() {}
#[cfg(not(feature = "test-support"))]
pub(crate) fn ahead_abandoned(_: usize) {}
#[cfg(not(feature = "test-support"))]
pub(crate) fn ahead_stopped() {}
#[cfg(not(feature = "test-support"))]
pub(crate) fn rels(_: usize) {}
#[cfg(not(feature = "test-support"))]
pub(crate) fn overrides(_: usize) {}
#[cfg(not(feature = "test-support"))]
pub(crate) fn parts(_: usize) {}
#[cfg(not(feature = "test-support"))]
pub(crate) fn name_probe() {}
#[cfg(not(feature = "test-support"))]
pub(crate) fn id_scan(_: usize) {}
#[cfg(not(feature = "test-support"))]
pub(crate) fn theme_bytes(_: usize) {}
#[cfg(not(feature = "test-support"))]
pub(crate) fn comment_entry() {}
#[cfg(not(feature = "test-support"))]
pub(crate) fn type_scan(_: usize) {}
#[cfg(not(feature = "test-support"))]
pub(crate) fn held_part() {}
#[cfg(not(feature = "test-support"))]
pub(crate) fn held_rows(_: usize) {}
#[cfg(not(feature = "test-support"))]
pub(crate) fn held_bytes(_: usize) {}
#[cfg(not(feature = "test-support"))]
pub(crate) fn rels_parse() {}
#[cfg(not(feature = "test-support"))]
pub(crate) fn type_rows(_: usize) {}
#[cfg(not(feature = "test-support"))]
pub(crate) fn model_part() {}
#[cfg(not(feature = "test-support"))]
pub(crate) fn model_row() {}
#[cfg(not(feature = "test-support"))]
pub(crate) fn model_bytes(_: usize) {}
#[cfg(not(feature = "test-support"))]
pub(crate) fn model_type() {}
#[cfg(not(feature = "test-support"))]
pub(crate) fn carry_walked(_: usize) {}
#[cfg(not(feature = "test-support"))]
pub(crate) fn carry_stamped(_: usize) {}
#[cfg(not(feature = "test-support"))]
pub(crate) fn carry_judged(_: usize) {}
#[cfg(not(feature = "test-support"))]
pub(crate) fn carry_blob_bytes(_: usize) {}
#[cfg(not(feature = "test-support"))]
pub(crate) fn carry_part_read(_: usize) {}
#[cfg(not(feature = "test-support"))]
pub(crate) fn xml_events(_: u64) {}

/// Parses `bytes` as XML within `limits` and counts the events the parser read ([`xml_events_read`]): **the reader parses a part of the document through this**, so that a part it does
/// not parse (a macro project, an object, a part of DiaCraft's) is seen to cost none.
pub(crate) fn parse(bytes: &[u8], limits: &diacraft_container::XmlLimits) -> Result<diacraft_container::xml::El, diacraft_container::XmlError> {
    let mut work = diacraft_container::Work::default();
    let out = diacraft_container::xml::parse_counted(bytes, limits, &mut work);
    xml_events(work.xml_events);
    out
}
#[cfg(not(feature = "test-support"))]
pub(crate) fn carry_copied(_: usize) {}
#[cfg(not(feature = "test-support"))]
pub(crate) fn object_frames(_: usize) {}
#[cfg(not(feature = "test-support"))]
pub(crate) fn object_parts(_: usize) {}
#[cfg(not(feature = "test-support"))]
pub(crate) fn object_bytes_read(_: usize) {}
#[cfg(not(feature = "test-support"))]
pub(crate) fn object_bytes_copied(_: usize) {}

// The walks of the kept markup of shapes that may name relationships (`write/page_rows/kept.rs`), counted on their own: a test reads them before and after the work it
// measures, so `reset` need not know them.
#[cfg(feature = "test-support")]
mod kept_on {
    use std::cell::Cell;

    thread_local! {
        static KEPT_STEPS: Cell<u64> = const { Cell::new(0) };
        static KEPT_COPIED: Cell<u64> = const { Cell::new(0) };
    }

    /// The elements, attributes and declarations the walks of kept markup looked at, and the entries they put in maps.
    #[must_use]
    pub fn kept_steps() -> u64 {
        KEPT_STEPS.with(Cell::get)
    }

    /// The entries the walks of kept markup copied into lists of their own: each attribute and each declaration put in a new list, whatever it shares with the model.
    #[must_use]
    pub fn kept_entries_copied() -> u64 {
        KEPT_COPIED.with(Cell::get)
    }

    pub(crate) fn kept_step(n: usize) {
        KEPT_STEPS.with(|c| c.set(c.get().saturating_add(n as u64)));
    }

    pub(crate) fn kept_copy(n: usize) {
        KEPT_COPIED.with(|c| c.set(c.get().saturating_add(n as u64)));
    }
}

#[cfg(feature = "test-support")]
pub(crate) use kept_on::{kept_copy, kept_step};
#[cfg(feature = "test-support")]
pub use kept_on::{kept_entries_copied, kept_steps};
#[cfg(not(feature = "test-support"))]
pub(crate) fn kept_step(_: usize) {}
#[cfg(not(feature = "test-support"))]
pub(crate) fn kept_copy(_: usize) {}
