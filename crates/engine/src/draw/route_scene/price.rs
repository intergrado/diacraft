//! What a scene holds, counted: the number of each part it keeps, and the bytes each is charged (as `snap/reads.rs` charges the index and the targets of a page).
//!
//! The charge is an estimate **by counts, at prices measured from the process's resident memory** and rounded up (the allocator's own figure is no use: it depends on the allocator), so that
//! the gate can hold each price and the sum of a scene's parts by pure counts (`each_price_is_pinned_and_they_add_up`), which do not move with the allocator. What a scene really holds is
//! measured by `measure_what_a_scene_holds` (ignored: from the repository, `cargo nextest run -p diacraft-engine --lib --run-ignored only --no-capture -E 'test(/route_scene::tests::memory::/)'`),
//! and `a_scene_is_charged_more_than_it_holds_in_memory` holds the charge over it for the dearest pages the caps allow. **To check the prices** when a part changes, run the first and set
//! them from it, and see the second pass.

/// The counts of what a scene keeps.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Parts {
    /// Top-level shapes held (each with a weak pointer to the allocation it was read from, which keeps that allocation's block of memory, and a record).
    pub tops: u64,
    /// The `ID`s of the members of groups, one in each group's list.
    pub members: u64,
    /// Two-dimensional shapes read, the facts of each, whether an obstacle or not.
    pub candidates: u64,
    /// Obstacles in the router's index.
    pub obstacles: u64,
    /// One-dimensional shapes read.
    pub lines: u64,
    /// Legs the scene keeps of those, a copy beside the router's.
    pub legs: u64,
    /// Wires in the router's index.
    pub wires: u64,
    /// Legs of those wires in the router's index.
    pub wire_legs: u64,
    /// Entries of the table of where each connector's ends are glued.
    pub glue_from: u64,
    /// Entries of the set of shapes a connector is glued to.
    pub glue_to: u64,
}

/// A top-level shape: its record and its entry in the table (measured 100 to 130 bytes). **Not counted:** the block of memory a weak pointer keeps when the shape it points to is replaced
/// before the next put right (a shape is about 250 bytes): it is let go when the scene is put right, which every edit that routes does, and is at most one for each shape an edit replaced.
pub(crate) const TOP: u64 = 128;
/// A member of a group: an `ID` in a list.
pub(crate) const MEMBER: u64 = 8;
/// A candidate: its facts, its entry in the table, and the box it is settled on (measured about 225).
pub(crate) const CANDIDATE: u64 = 256;
/// An obstacle in the router: its slot, its entry in the table of `ID`s, and its entries in the buckets (measured about 285).
pub(crate) const OBSTACLE: u64 = 336;
/// A connector: its record, with the setting, the style and the glue (measured about 90).
pub(crate) const LINE: u64 = 128;
/// A leg the scene keeps: two points, in a list.
pub(crate) const LEG: u64 = 48;
/// A wire in the router: its entry in the table of wires and the list of its legs.
pub(crate) const WIRE: u64 = 96;
/// A leg of a wire in the router: its slot and its entries in the buckets.
pub(crate) const WIRE_LEG: u64 = 112;
/// An entry of the table of glue.
pub(crate) const GLUE_FROM: u64 = 64;
/// An entry of the set of glued shapes.
pub(crate) const GLUE_TO: u64 = 40;

/// The bytes a scene with these parts is charged: each count at its own price, saturating.
pub(crate) fn price(parts: &Parts) -> u64 {
    [
        (parts.tops, TOP),
        (parts.members, MEMBER),
        (parts.candidates, CANDIDATE),
        (parts.obstacles, OBSTACLE),
        (parts.lines, LINE),
        (parts.legs, LEG),
        (parts.wires, WIRE),
        (parts.wire_legs, WIRE_LEG),
        (parts.glue_from, GLUE_FROM),
        (parts.glue_to, GLUE_TO),
    ]
    .into_iter()
    .fold(0u64, |sum, (count, each)| sum.saturating_add(count.saturating_mul(each)))
}
