//! What a read or a write of a package may hold: the container's limits, the file's own, and **the memory** (the plan's).
//!
//! **The memory is counted per process.** [`Limits::max_memory`] is what this crate's read or write may take at once, **beside what the process holds already**:
//! the engine gives [`Limits::beside`] the figure of everything else it holds (another open drawing, its file, a stencil), as the VSDX reader's `Limits::max_memory`
//! does, so that two sessions and a read in progress share one budget and never each take all of it.
//!
//! **What a read holds** is the file (a slice, as the container reads it), the directory of the archive, the tree of the part in hand (and the document part's, beside it while the pages
//! are read), that part's bytes twice (as inflated, and as the strings of its tree), the blobs and the model it makes ([`OPEN_WORST_BYTES`] adds them: 2.9 GB at the
//! largest file, of the 3 GB an address limit of `ulimit -v 3000000` leaves). The pools of the codec's [`Budget`] and the limits of each parse are what [`Pools`] sets from `max_memory` **before anything
//! is made**: the file's own length, the directory and **the document part's tree up to what no pool counts** ([`OPEN_DOCUMENT_FREE_ITEMS`], 41 MB; it was shared
//! out with the rest, so that a read short of memory held that much more than `max_memory`) are taken first, and **what is left is shared by the four pools in the proportion
//! the open's defaults have them in**, so that a smaller `max_memory` makes every pool smaller and a read that would pass one is refused in words. **What a read may hold
//! is never more than `max_memory`**: [`Limits::read_peak`] is that sum, counted from the pools, and a test holds it to `max_memory` at the default and at the engine's budget.
//!
//! **What a write holds** is the model it writes (as the codec counts it), the codec's own room for the part in hand ([`SAVE_WORST_BYTES`] adds them) and, **made by
//! this crate**, the package: the bytes of its entries as they are written, and the serialized and deflated copies of the part in hand. Each is taken from
//! `max_memory` before it is made, and the package is never more than [`Limits::max_file_bytes`].

use diacraft_container::zip::Entry;
use diacraft_sheetxml::model::cost::{BASE_BYTES, TREE_ITEM_BYTES};
use diacraft_sheetxml::model::{
    ADDRESS_SPACE_BYTES, Budget, OPEN_DOCUMENT_FREE_ITEMS, OPEN_ELEMENT_ITEMS, OPEN_FILE_BYTES, OPEN_PART_BYTES, OPEN_TREE_ITEMS, OPEN_WORST_BYTES,
    SAVE_WORST_BYTES, SECTION_STREAM_BYTES, open_limits, save_child_items, save_tree_items,
};

use crate::error::NativeError;

/// What a read of a package holds in the worst case, apart from what a process has before it opens anything: the pools of [`OPEN_WORST_BYTES`] without
/// [`BASE_BYTES`]. The default of [`Limits::max_memory`].
pub const MAX_MEMORY: u64 = OPEN_WORST_BYTES - BASE_BYTES;

// A write holds the codec's own peak and the package it makes, which an open takes at the most as a file ([`OPEN_FILE_BYTES`]). The sum fits the address space with a
// twentieth to spare, as the two figures of the codec each do, which these constants say at compile time, **for a drawing whose children fit a tree of an open**
// ([`OPEN_TREE_ITEMS`]). (The serialized and the deflated copy of the part in hand are in the package's total while they are made, which is at most the package cap: a part of
// [`OPEN_PART_BYTES`] leaves that much less for the rest.) A child of more items, up to the child limit ([`OPEN_ELEMENT_ITEMS`]), is held beside the package by what
// [`Limits::for_save`] takes out of the memory the write may use for it, so the sum holds whatever the child.
const _: () = assert!(
    SAVE_WORST_BYTES - (OPEN_ELEMENT_ITEMS - OPEN_TREE_ITEMS) * TREE_ITEM_BYTES + OPEN_FILE_BYTES <= ADDRESS_SPACE_BYTES - ADDRESS_SPACE_BYTES / 20,
    "a save of the largest package does not fit the address space"
);

/// What a read or a write is held to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Limits {
    /// The container's: the entries, their sizes, the ratio, the names, and the XML of each part. The default is the codec's [`open_limits`]: 50,000 entries, a
    /// package of 512 MiB inflated and 256 MiB in an entry, a ratio of 1,000, and a tree of 2 million items at the depth of 512 that the format's parts are read at (the document
    /// part's, the manifest's and the state's are trees; **a page or a master part is read as a stream of its children**, each of which may be more than a tree, and the part as a
    /// whole is the format's 12 million items).
    pub container: diacraft_container::Limits,
    /// The most bytes a package file may be: [`OPEN_FILE_BYTES`], the package's cap and 64 MiB (the most the archive's directory and headers add to what its
    /// entries inflate to). A longer file is refused **before it is read**, and a package a write would make longer is refused before it is written.
    pub max_file_bytes: u64,
    /// The most bytes a read or a write of this crate may take at once beside what the process holds: [`MAX_MEMORY`] by default. See [`Limits::beside`].
    pub max_memory: u64,
    /// **The threads a read may use**, its own counted: 1, the default, makes no thread. More inflates the blobs the document names (its pictures and parts) on threads, each into a buffer
    /// the pool was charged for before the read began and of the size the directory states for it; **the document, the notes and every refusal are the single-threaded read's at any count**.
    /// A read holds the count to the address space that is left beside what it may hold (`diacraft_container::spare`).
    pub threads: usize,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits { container: open_limits(), max_file_bytes: OPEN_FILE_BYTES, max_memory: MAX_MEMORY, threads: 1 }
    }
}

impl Limits {
    /// The limits of a read or a write made beside `held` bytes that the process holds: the defaults, with the memory what is left of [`MAX_MEMORY`] (nothing, when
    /// `held` is all of it or more). **A read** is given these. A write is better given [`Limits::for_save`], which also takes out what the codec holds for a save.
    #[must_use]
    pub fn beside(held: u64) -> Limits {
        Limits { max_memory: MAX_MEMORY.saturating_sub(held), ..Limits::default() }
    }

    /// What a read of the file `bytes` at these limits may hold at once, beside what the process holds before it opens anything: the file, its directory, the four pools and the
    /// document part's tree up to what no pool counts. **Never more than [`Limits::max_memory`]**: the pools are shared out of what the file, the directory and that
    /// tree leave of it. A file that leaves no room is refused here, in the words of the read.
    pub fn read_peak(&self, bytes: &[u8]) -> Result<u64, NativeError> {
        let held = pools(self, bytes)?.held_bytes();
        Ok(u64::try_from(bytes.len()).unwrap_or(u64::MAX).saturating_add(directory_bytes(bytes, &self.container)).saturating_add(held))
    }

    /// The limits of a save made when the process holds `held` bytes **in all, the model of the drawing included once**: what the caller passes is the figure it counts for
    /// the whole process (the engine's `held.total()`), and nothing else. A save holds, beside that, what the codec makes while it writes a part and no one else counts: **the
    /// tree of the child in hand** at the items that fit beside the model ([`save_child_items`]) and [`TREE_ITEM_BYTES`] an item, **the tables its check of equal sheets keeps**
    /// ([`SECTION_STREAM_BYTES`]) and **the bytes of a part** at the part cap ([`OPEN_PART_BYTES`]). This takes them out of what is left of [`MAX_MEMORY`], and what remains is
    /// [`Limits::max_memory`], the memory of the package itself, which this crate charges as it makes it; and the tree of a part is held to the items that fit
    /// (`container.xml.max_nodes`), so that the sum of the model and the tree is under the address space whatever the model.
    ///
    /// A process that holds so much that no tree of a part fits, or no memory is left for the package, is refused here in words, before anything is made.
    pub fn for_save(held: u64) -> Result<Limits, NativeError> {
        let items = save_tree_items(held);
        // The tree in hand is a child of a part, which may be more than a tree of an open: room is taken for the most it may have beside the model.
        let child = save_child_items(held);
        let transient = child.saturating_mul(TREE_ITEM_BYTES).saturating_add(SECTION_STREAM_BYTES).saturating_add(OPEN_PART_BYTES);
        let room = MAX_MEMORY.saturating_sub(held).saturating_sub(transient);
        if items == 0 || room == 0 {
            return Err(NativeError::TooLarge(
                "the drawing needs more memory to save than is free: the drawing and the other files that are open leave no room for the tree of a part and the package",
            ));
        }
        let mut limits = Limits { max_memory: room, ..Limits::default() };
        limits.container.xml.max_nodes = limits.container.xml.max_nodes.min(usize::try_from(items).unwrap_or(usize::MAX));
        Ok(limits)
    }
}

/// What a read may spend, from [`Limits`] and the length of the file: each pool is the open's own, shrunk in proportion to the memory that is left.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Pools {
    /// The items of the decode, in all: time, no memory.
    pub items: u64,
    /// The items one part's tree may have.
    pub tree_items: u64,
    /// The bytes of an XML part, inflated.
    pub part_bytes: u64,
    /// The bytes of blobs.
    pub blob_bytes: u64,
    /// The bytes of the model in memory.
    pub model_bytes: u64,
}

/// The bytes an XML part may be, inflated, at `container`'s limits: the container's own cap of an entry, and never more than [`OPEN_PART_BYTES`] (a part is held
/// beside the tree made of it, and an entry that is a picture may be larger).
pub(crate) fn part_cap(container: &diacraft_container::Limits) -> u64 {
    container.max_part_bytes.min(OPEN_PART_BYTES)
}

/// What the directory of the archive in `bytes` takes in memory at the most: the rows the container counts for it (**the count the read is made by**, which is checked against the
/// directory it gives; at most twice the entries a package may have and sixteen; **without a count the most the file could hold**, a row being at least 46 bytes of the file), each kept as a range and a flag, and for each kept the container's
/// [`Entry`], two copies of its name and a few words. The names are in the file, so they come to no more than the file does.
pub(crate) fn directory_bytes(bytes: &[u8], container: &diacraft_container::Limits) -> u64 {
    let file_len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    let most_rows = u64::try_from(container.max_parts.saturating_mul(2).saturating_add(16)).unwrap_or(u64::MAX);
    let by_size = (file_len / 46 + 1).min(most_rows);
    // The count is the container's own, the one the read makes the directory by (a rule of this crate's for where the end record stands was not, and a comment can hold a
    // second one); a file it refuses has no count, and is charged as the most it could hold.
    let rows = diacraft_container::zip::directory_rows(bytes).map_or(by_size, |counted| counted.min(most_rows));
    let kept = rows.min(u64::try_from(container.max_parts).unwrap_or(u64::MAX));
    let name = u64::try_from(container.max_part_name_bytes).unwrap_or(u64::MAX);
    let names = file_len.min(kept.saturating_mul(name)).saturating_mul(2);
    let entry = u64::try_from(std::mem::size_of::<Entry>()).unwrap_or(u64::MAX).saturating_add(96);
    // The row of the directory kept as a range (16 bytes) and a flag each, for every row, and the entry for every one kept.
    rows.saturating_mul(32).saturating_add(kept.saturating_mul(entry)).saturating_add(names)
}

/// The bytes of the document part's tree that no pool counts: its first [`OPEN_DOCUMENT_FREE_ITEMS`] items, which the decode holds beside the tree of the child in hand and the
/// model (what it has beyond them is taken from the model's pool, [`Budget::reserve_tree`]). The tree is parsed at the items of the tree pool, so a pool of fewer holds a
/// tree of fewer. [`OPEN_WORST_BYTES`] holds this much beside the pools, and [`pools`] takes it out of the room before it shares the rest (it was shared out
/// with the rest, so that a read short of memory held this much more than `max_memory`).
pub(crate) fn document_tree_bytes(tree_items: u64) -> u64 {
    tree_items.min(OPEN_DOCUMENT_FREE_ITEMS).saturating_mul(TREE_ITEM_BYTES)
}

/// The pools of a read of the file `bytes` at `limits`, or the refusal in words that the file leaves no room: the file is held by the caller, the directory and the document
/// part's tree (up to what no pool counts) are taken, and what is left is shared by the tree of a part, the two copies of the part's bytes, the blobs and the model.
pub(crate) fn pools(limits: &Limits, bytes: &[u8]) -> Result<Pools, NativeError> {
    let file_len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    let open = Budget::for_open(&limits.container);
    let tree_items = u64::try_from(limits.container.xml.max_nodes).unwrap_or(u64::MAX).min(OPEN_TREE_ITEMS);
    let part_bytes = part_cap(&limits.container);
    let wanted = tree_items
        .saturating_mul(TREE_ITEM_BYTES)
        .saturating_add(part_bytes.saturating_mul(2))
        .saturating_add(open.blob_bytes)
        .saturating_add(open.bytes);
    let taken = file_len.saturating_add(directory_bytes(bytes, &limits.container));
    let no_room = || {
        NativeError::TooLarge("the file needs more memory to open than is free: the drawing and the other files that are open leave no room for it")
    };
    let room = limits.max_memory.checked_sub(taken).filter(|room| *room > 0).ok_or_else(no_room)?;
    // What a pool of `part` gets when `room` is shared out in proportion to what the pools want (the pool itself, when the room holds them all).
    let share = |part: u64, room: u64| -> u64 {
        if wanted <= room || wanted == 0 { part } else { u64::try_from(u128::from(part) * u128::from(room) / u128::from(wanted)).unwrap_or(u64::MAX) }
    };
    // The document part's tree is held beside the pools, and is taken out of the room before the rest is shared. It is no larger than the tree pool would be if the room were shared
    // out whole, and the pool that is left after it is taken is no larger than that: what is held is never more than what was taken.
    let room = room.saturating_sub(document_tree_bytes(share(tree_items, room)));
    if room == 0 {
        return Err(no_room());
    }
    let scale = |part: u64| -> u64 { share(part, room) };
    Ok(Pools {
        items: open.items,
        tree_items: scale(tree_items),
        part_bytes: scale(part_bytes),
        blob_bytes: scale(open.blob_bytes),
        model_bytes: scale(open.bytes),
    })
}

impl Pools {
    /// What a read holds at the most beside the file and its directory: the four pools and the document part's tree up to what no pool counts. **It is never more than the memory
    /// the pools were made from**, less the file and the directory ([`pools`] takes the tree out of the room before it shares out the rest).
    pub(crate) fn held_bytes(&self) -> u64 {
        self.tree_items
            .saturating_mul(TREE_ITEM_BYTES)
            .saturating_add(self.part_bytes.saturating_mul(2))
            .saturating_add(self.blob_bytes)
            .saturating_add(self.model_bytes)
            .saturating_add(document_tree_bytes(self.tree_items))
    }

    /// The budget the decode of a package is charged to: the pools of the read, **with the model allowed the room of the blobs' pool that the blobs have not spent** (as an open's is,
    /// [`Budget::for_open`]: the two pools are sized as a sum that fits, and the model of a drawing with no pictures may have both), and with the items a child of a page or a master
    /// part may have beyond the tree pool's held back from the model's pool while the child is held.
    pub(crate) fn budget(&self, container: &diacraft_container::Limits) -> Budget {
        Budget::new(self.items, self.model_bytes, self.blob_bytes).with_lent_blobs().with_tree_pool(self.tree_items, container)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use diacraft_container::zip::{ArchiveWriter, Method};

    use super::*;

    /// A package of `entries` stored entries of one byte each, and a comment of `comment` bytes after its end record.
    fn archive(entries: usize, comment: usize) -> Vec<u8> {
        let mut w = ArchiveWriter::new();
        for i in 0..entries {
            w.add(&format!("e{i}"), b"x", Method::Stored).unwrap();
        }
        let mut bytes = w.finish().unwrap();
        if comment > 0 {
            // The comment's length is the last two bytes of the end record, which stood at the end of the file.
            let at = bytes.len() - 2;
            bytes[at..].copy_from_slice(&(comment as u16).to_le_bytes());
            bytes.extend(std::iter::repeat_n(b'c', comment));
        }
        bytes
    }

    #[test]
    fn the_directory_is_charged_by_the_rows_the_container_counts_and_by_the_file_without_one() {
        let container = diacraft_container::Limits::default();
        let small = archive(5, 0);
        assert!(directory_bytes(&small, &container) < 20_000, "five entries: {}", directory_bytes(&small, &container));
        // The same count in a file of 5 MB of stored bytes: the rows are still five.
        let mut w = ArchiveWriter::new();
        w.add("big", &vec![7u8; 5 << 20], Method::Stored).unwrap();
        for i in 0..4 {
            w.add(&format!("e{i}"), b"x", Method::Stored).unwrap();
        }
        let big = w.finish().unwrap();
        assert!(directory_bytes(&big, &container) < 20_000, "{}", directory_bytes(&big, &container));
        // A count that lies (60,000 in a file that cannot hold them) is refused by the container, and the charge is the most a file of that size could hold.
        let mut lying = archive(2, 0);
        let at = lying.len() - 22 + 10;
        lying[at..at + 2].copy_from_slice(&60_000u16.to_le_bytes());
        assert!(directory_bytes(&lying, &container) < directory_bytes(&vec![0u8; 5 << 20], &container) / 100);
        // No record: the most the file could hold, which for a file of 5 MB is more than a hundred thousand rows.
        let none = directory_bytes(&vec![0u8; 5 << 20], &container);
        assert!(none > 10 << 20, "{none}");
        // More entries cost more, and the cost is the rows' (32 bytes and an entry each, and the names).
        let (ten, fifty) = (directory_bytes(&archive(10, 0), &container), directory_bytes(&archive(50, 0), &container));
        assert!(fifty > 4 * ten && fifty < 6 * ten, "{ten} and {fifty}");
        // The least the rows come to: 32 bytes of range and flag, and the entry, for each.
        assert!(ten > 10 * (32 + std::mem::size_of::<Entry>() as u64), "{ten}");
    }

    #[test]
    fn the_pools_are_the_opens_own_until_the_memory_is_short_and_then_each_is_shrunk_in_proportion() {
        let bytes = archive(3, 0);
        let len = bytes.len() as u64;
        let dir = directory_bytes(&bytes, &diacraft_container::Limits::default());
        // What is taken out of the room before the pools are shared: the document part's tree up to what no pool counts, at the pool of a room that holds them all.
        let tree = document_tree_bytes(OPEN_TREE_ITEMS);
        let open = Budget::for_open(&diacraft_container::Limits::default());
        let wanted = OPEN_TREE_ITEMS * TREE_ITEM_BYTES + 2 * OPEN_PART_BYTES + open.blob_bytes + open.bytes;
        // Room for all four pools exactly: they are the open's own.
        let whole = pools(&Limits { max_memory: len + dir + tree + wanted, ..Limits::default() }, &bytes).unwrap();
        assert_eq!(
            (whole.tree_items, whole.part_bytes, whole.blob_bytes, whole.model_bytes),
            (OPEN_TREE_ITEMS, OPEN_PART_BYTES, open.blob_bytes, open.bytes)
        );
        assert_eq!(whole.items, open.items);
        // One byte less, and the file and the directory are what is taken first: every pool is its share of the room.
        let short = pools(&Limits { max_memory: len + dir + tree + wanted - 1, ..Limits::default() }, &bytes).unwrap();
        assert!(
            short.tree_items < OPEN_TREE_ITEMS
                && short.blob_bytes < open.blob_bytes
                && short.model_bytes < open.bytes
                && short.part_bytes < OPEN_PART_BYTES
        );
        // Without the directory taken the memory would have been enough: it is taken.
        let no_dir = pools(&Limits { max_memory: len + wanted, ..Limits::default() }, &bytes).unwrap();
        assert!(no_dir.blob_bytes < open.blob_bytes, "the directory is part of what is taken first");
        // Half the room: each pool is half of the open's, to the byte (a share is the pool's size times the room over what the pools want).
        let room = wanted / 2;
        let half = pools(&Limits { max_memory: len + dir + tree + room, ..Limits::default() }, &bytes).unwrap();
        let share = |pool: u64| u64::try_from(u128::from(pool) * u128::from(room) / u128::from(wanted)).unwrap();
        assert_eq!(half.tree_items, share(OPEN_TREE_ITEMS));
        assert_eq!(half.part_bytes, share(OPEN_PART_BYTES));
        assert_eq!(half.blob_bytes, share(open.blob_bytes));
        assert_eq!(half.model_bytes, share(open.bytes));
        // The pools sum to the room (each rounded down) and not more.
        assert!(half.tree_items * TREE_ITEM_BYTES + 2 * half.part_bytes + half.blob_bytes + half.model_bytes <= room);
        // No room at all, or none beside the file and the directory: refused in words.
        for memory in [0, len, len + dir] {
            let error = pools(&Limits { max_memory: memory, ..Limits::default() }, &bytes).unwrap_err();
            assert_eq!(error.class(), "too_large", "{memory}");
            assert!(error.to_string().contains("memory"), "{error}");
        }
        assert!(pools(&Limits { max_memory: len + dir + 1, ..Limits::default() }, &bytes).is_ok());
        // The caps of the container narrow the pools before the memory does.
        let mut small = Limits::default();
        small.container.xml.max_nodes = 1_000;
        small.container.max_part_bytes = 10_000;
        let pools_small = pools(&small, &bytes).unwrap();
        assert_eq!((pools_small.tree_items, pools_small.part_bytes), (1_000, 10_000));
    }

    /// The gap GATE.e142.F1 found and did not test: [`MAX_MEMORY`] counts the document part's tree beside the four pools ([`OPEN_WORST_BYTES`]), but `pools` shared the
    /// whole room out among the four, so that a read short of memory held the tree (41 MB) **beside** a budget that was already spent: up to `max_memory` and 41 MB more. What a read
    /// holds is counted here from the pools themselves (the file, the directory, the four pools and the tree the pool of items allows), at the crate's default and at every budget
    /// below it that leaves any room, and is never more than the memory the read was given.
    #[test]
    fn a_read_holds_no_more_than_the_memory_it_is_given_the_document_parts_tree_beside_the_pools_included() {
        let bytes = archive(3, 0);
        let taken = bytes.len() as u64 + directory_bytes(&bytes, &diacraft_container::Limits::default());
        let mut short = 0;
        for memory in [
            MAX_MEMORY,
            MAX_MEMORY - 1,
            // The budget of a process that holds a drawing already, and of a small machine.
            2_600_000_000,
            2_000_000_000,
            1 << 30,
            600 << 20,
            300 << 20,
            100 << 20,
            50 << 20,
            10 << 20,
        ] {
            let limits = Limits { max_memory: memory, ..Limits::default() };
            let Ok(p) = pools(&limits, &bytes) else { continue };
            let tree = p.tree_items.min(OPEN_DOCUMENT_FREE_ITEMS) * TREE_ITEM_BYTES;
            let held = taken + p.tree_items * TREE_ITEM_BYTES + 2 * p.part_bytes + p.blob_bytes + p.model_bytes + tree;
            assert!(held <= memory, "a read of {memory} bytes holds {held}, {} more", held - memory);
            assert_eq!(limits.read_peak(&bytes).unwrap(), held, "{memory}: the figure the engine asks for is the sum");
            if p.tree_items < OPEN_TREE_ITEMS {
                short += 1;
            }
        }
        assert!(short >= 5, "the budgets that are short of memory, where the pools were shared out of all of it: {short}");
        // At the default the pools are the open's own and the tree is beside them, within the memory.
        let default = Limits::default();
        let own = pools(&default, &bytes).unwrap();
        assert_eq!(own.tree_items, OPEN_TREE_ITEMS);
        assert!(default.read_peak(&bytes).unwrap() <= MAX_MEMORY);
        // No room beside the file, the directory and the tree: refused, as a read refuses it, and no peak is made of it.
        let none = Limits { max_memory: taken, ..Limits::default() };
        assert_eq!(none.read_peak(&bytes).unwrap_err().class(), "too_large");
    }

    #[test]
    fn the_save_budget_is_what_the_process_leaves_less_the_codecs_room() {
        let free = Limits::for_save(0).unwrap();
        // The tree in hand is a child of a part, which may be more than a tree of an open: room is taken for the most it may have.
        assert_eq!(free.max_memory, MAX_MEMORY - (save_child_items(0) * TREE_ITEM_BYTES + SECTION_STREAM_BYTES + OPEN_PART_BYTES));
        assert!(save_child_items(0) > save_tree_items(0));
        let held = 3u64 << 29;
        let tight = Limits::for_save(held).unwrap();
        assert_eq!(tight.max_memory, MAX_MEMORY - held - (save_child_items(held) * TREE_ITEM_BYTES + SECTION_STREAM_BYTES + OPEN_PART_BYTES));
        assert_eq!(tight.container.xml.max_nodes as u64, save_tree_items(held));
        assert_eq!(Limits::for_save(u64::MAX).unwrap_err().class(), "too_large");
        // Where the tree fits and the package has no room: refused as well.
        let almost = MAX_MEMORY - 1_000;
        assert!(Limits::for_save(almost).is_err());
    }
}

#[cfg(test)]
mod comment_tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use diacraft_container::zip::{ArchiveWriter, Method};

    use super::*;

    /// A package of `entries` stored entries of one byte each.
    fn many(entries: usize) -> Vec<u8> {
        let mut w = ArchiveWriter::new();
        for i in 0..entries {
            w.add(&format!("e{i}"), b"x", Method::Stored).unwrap();
        }
        w.finish().unwrap()
    }

    #[test]
    fn a_second_end_record_in_the_comment_does_not_lower_the_charge_of_a_directory_of_fifty_thousand_rows() {
        // The automated scan's finding: a rule for where the end record stands that is not the container's can be given a signature in the comment, ahead of the real directory, that
        // counts three rows. The charge is made from the count the container reads the file by, and covers all 50,000.
        let container = diacraft_container::Limits::default();
        let clean = many(50_000);
        let charged = directory_bytes(&clean, &container);
        assert!(charged > 10 << 20, "50,000 rows: {charged}");
        // A comment that holds the signature of an end record that counts three rows, and does not end where the file does.
        let mut fake = vec![0x50, 0x4B, 0x05, 0x06, 0, 0, 0, 0, 3, 0, 3, 0, 46, 0, 0, 0, 0, 0, 0, 0, 0xFF, 0xFF];
        fake.extend_from_slice(b" the rest of the comment");
        let mut with_comment = clean.clone();
        let at = with_comment.len() - 2;
        with_comment[at..].copy_from_slice(&(fake.len() as u16).to_le_bytes());
        with_comment.extend_from_slice(&fake);
        assert_eq!(
            diacraft_container::zip::Archive::read(&with_comment, &container).unwrap().entries().len(),
            50_000,
            "the container reads all of them"
        );
        let covered = |bytes: &[u8]| {
            let got = directory_bytes(bytes, &container);
            // The file is longer by the comment, and the names are in the file: the charge is the clean file's and a little more, never a charge for three rows.
            assert!(got >= charged && got < charged + 400_000, "{got} against {charged}");
        };
        covered(&with_comment);
        // The same in a file whose real record is behind a long comment of fake ones.
        let mut crowded = clean.clone();
        let at = crowded.len() - 2;
        let comment: Vec<u8> = fake.iter().copied().cycle().take(60_000).collect();
        crowded[at..].copy_from_slice(&(comment.len() as u16).to_le_bytes());
        crowded.extend_from_slice(&comment);
        covered(&crowded);
        // A read of such a file through `pools` takes the directory out of the room as the container counts it: 5 MB beside the file is not room for the directory of 50,000 rows.
        let limits = Limits { max_memory: crowded.len() as u64 + 5_000_000, ..Limits::default() };
        assert!(pools(&limits, &crowded).is_err(), "no room is left for the pools beside the file and its directory");
    }
}
