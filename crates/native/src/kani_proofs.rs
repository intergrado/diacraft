//! Kani proof harnesses for the package's integer code: the memory a read and a save are given (`Limits::beside`, `Limits::for_save`, and the pools of a read, which
//! two changes sized and fixed, `Limits::read_peak`), and what a save on threads keeps of its meter for its parts (`write::keep_of`). Run with
//! `cargo xtask kani` (`docs/development.md`, Proofs).
//!
//! **Not proved here: the scan of a part's bytes for the blobs it names** (`blobs_named_bytes`). It goes through `str::from_utf8` and `str::parse` inside a
//! search of the text, and CBMC unrolls each of those loops inside the others to the bound: a harness of ten symbolic bytes, and one of a `blob` attribute of symbolic
//! blanks, quote and digits, did not finish in 300 s. The unit tests of `decoded.rs` hold it to what a tree names.

use diacraft_sheetxml::model::cost::TREE_ITEM_BYTES;
use diacraft_sheetxml::model::{OPEN_DOCUMENT_FREE_ITEMS, OPEN_PART_BYTES, OPEN_TREE_ITEMS, SECTION_STREAM_BYTES, save_child_items, save_tree_items};

use crate::limits::{directory_bytes, pools};
use crate::{Limits, MAX_MEMORY, NativeError};

/// `Limits::beside`: a read beside `held` bytes is given what is left of [`MAX_MEMORY`], nothing when `held` is all of it, and every other limit is the default.
#[kani::proof]
fn a_read_beside_what_is_held_is_given_what_is_left_of_the_budget() {
    let held: u64 = kani::any();
    let limits = Limits::beside(held);
    let defaults = Limits::default();
    assert!(limits.max_memory <= MAX_MEMORY);
    if held <= MAX_MEMORY {
        assert!(limits.max_memory + held == MAX_MEMORY);
    } else {
        assert!(limits.max_memory == 0);
    }
    assert!(limits.container == defaults.container && limits.max_file_bytes == defaults.max_file_bytes);
    kani::cover!((held > 0) & (limits.max_memory > 0), "something held, room left");
    kani::cover!(held > MAX_MEMORY, "more held than the budget");
}

/// `Limits::for_save`: a save beside a process that holds `held` bytes is given **what is left of [`MAX_MEMORY`] once the tree of the child in hand,
/// the check's tables and a part's bytes are taken out**, to the byte, with a tree of a part held to the items that fit beside the model; it is refused in words exactly
/// when no tree fits or no memory is left; and a process that holds more is never given a save that one holding less was refused.
#[kani::proof]
fn a_save_is_given_what_is_left_beside_the_model_the_child_and_a_part_or_is_refused() {
    let (held, more): (u64, u64) = (kani::any(), kani::any());
    kani::assume(held <= more);
    let child = save_child_items(held);
    let transient = child * TREE_ITEM_BYTES + SECTION_STREAM_BYTES + OPEN_PART_BYTES;
    let fits = save_tree_items(held) > 0 && held < MAX_MEMORY && transient < MAX_MEMORY - held;
    match Limits::for_save(held) {
        Ok(limits) => {
            assert!(fits);
            assert!(held + transient + limits.max_memory == MAX_MEMORY);
            assert!(limits.container.xml.max_nodes as u64 == save_tree_items(held));
            let defaults = Limits::default();
            let mut container = limits.container.clone();
            container.xml.max_nodes = defaults.container.xml.max_nodes;
            assert!(container == defaults.container && limits.max_file_bytes == defaults.max_file_bytes);
            kani::cover!(save_tree_items(held) < OPEN_TREE_ITEMS, "a model large enough that the tree of a part is fewer items");
            kani::cover!(child < save_child_items(0), "a model large enough that the child is fewer items");
        }
        Err(e) => {
            assert!(!fits);
            assert!(matches!(e, NativeError::TooLarge(_)));
            assert!(Limits::for_save(more).is_err());
            kani::cover!(held < MAX_MEMORY, "refused with less held than the budget");
        }
    }
}

/// A read of an empty file at `max_memory`: what it takes before its pools (the directory of the archive), and, when it is not refused, its pools and **what it holds while the
/// pages are read, counted here from the pools**: the file, the directory, the four pools and the part of the document part's tree that is in no pool's charge
/// (`OPEN_DOCUMENT_FREE_ITEMS`, at most the tree pool: the decode holds that tree beside the model, and takes only what it has beyond them from the model's pool). A refusal is
/// in words. (The figure the engine asks for, `Limits::read_peak`, is that sum: asked here beside the pools, the solver must show two runs of the same quotients of 128 bits
/// equal, the trap of a second circuit, so the harness at the two ends asks it only where nothing is divided.)
fn read_of_an_empty_file(max_memory: u64) -> (u64, Option<(crate::limits::Pools, u64)>) {
    let limits = Limits { max_memory, ..Limits::default() };
    let bytes: [u8; 0] = [];
    let directory = directory_bytes(&bytes, &limits.container);
    match pools(&limits, &bytes) {
        Ok(p) => {
            let all = p.tree_items * TREE_ITEM_BYTES + 2 * p.part_bytes + p.blob_bytes + p.model_bytes;
            let free = OPEN_DOCUMENT_FREE_ITEMS.min(p.tree_items) * TREE_ITEM_BYTES;
            let held = directory + all + free;
            (directory, Some((p, held)))
        }
        Err(e) => {
            assert!(matches!(e, NativeError::TooLarge(_)));
            (directory, None)
        }
    }
}

/// What the pools want at the default limits: a tree of an open, a part's bytes twice, the blobs and the model.
fn wanted() -> u64 {
    let open = diacraft_sheetxml::model::Budget::for_open(&Limits::default().container);
    OPEN_TREE_ITEMS * TREE_ITEM_BYTES + 2 * OPEN_PART_BYTES + open.blob_bytes + open.bytes
}

/// The room beside the file and the directory that holds the pools whole and the document part's free tree beside them.
fn whole() -> u64 {
    wanted() + OPEN_DOCUMENT_FREE_ITEMS * TREE_ITEM_BYTES
}

/// The pools of a read (`limits::pools`) at the two ends: a read whose memory leaves nothing beside the file and the directory is refused in words,
/// and one given room for the pools and the free tree beside them has the open's own pools and holds no more than its memory. (That a read with any room at all is not refused,
/// the boundary between, is a bound on a quotient of 128 bits for every room, which did not finish in 300 s; the bands below take it for the rooms they cover.)
#[kani::proof]
fn a_read_with_nothing_beside_the_file_is_refused_and_one_with_room_for_its_pools_has_them() {
    let max_memory: u64 = kani::any();
    let (first, read) = read_of_an_empty_file(max_memory);
    if max_memory <= first {
        assert!(read.is_none());
        assert!(Limits { max_memory, ..Limits::default() }.read_peak(&[]).is_err());
        kani::cover!(max_memory == first, "no byte beside the file");
    } else if max_memory - first >= whole() {
        let open = diacraft_sheetxml::model::Budget::for_open(&Limits::default().container);
        let Some((p, held)) = read else { panic!("refused with room for every pool") };
        assert!((p.tree_items, p.part_bytes, p.blob_bytes, p.model_bytes) == (OPEN_TREE_ITEMS, OPEN_PART_BYTES, open.blob_bytes, open.bytes));
        assert!(held <= max_memory);
        assert!(Limits { max_memory, ..Limits::default() }.read_peak(&[]).ok() == Some(held));
        kani::cover!(max_memory - first == whole(), "room for the pools and the free tree to the byte");
    }
}

/// The pools of a read short of memory, where they are shrunk in proportion (a division of 128 bits): what the read holds is never more than its memory, and each pool is never
/// more than the open's own. **This failed on the code of CAPACITY3** (the pools shared all the memory beside the file and the directory, and the free tree came on top; Kani's
/// counterexample, a memory of 10,288 bytes), which fixed on main while this task found it. Proved for a room of up to 16 MiB beside the file, and for the 4 KiB
/// just short of the room that holds the pools whole (16 MiB there did not finish in 300 s): over every memory the solver must bound the quotients of 128 bits the pools are
/// shared by, which did not finish either. The unit tests of `limits.rs` take ten memories between.
fn a_read_short_of_memory_holds_no_more_than_it_was_given(room: u64) {
    let bytes: [u8; 0] = [];
    let first = directory_bytes(&bytes, &Limits::default().container);
    let (_, read) = read_of_an_empty_file(first + room);
    let open = diacraft_sheetxml::model::Budget::for_open(&Limits::default().container);
    let Some((p, held)) = read else { panic!("refused with room beside the file") };
    assert!(held <= first + room);
    assert!(p.tree_items <= OPEN_TREE_ITEMS && p.part_bytes <= OPEN_PART_BYTES && p.blob_bytes <= open.blob_bytes && p.model_bytes <= open.bytes);
}

/// The band of small rooms a harness of a read short of memory takes: 16 MiB.
const BAND: u64 = 16 << 20;

/// The band of rooms just short of the whole a harness takes: 4 KiB.
const NEAR: u64 = 4 << 10;

#[kani::proof]
fn a_read_with_little_room_holds_no_more_than_it_was_given() {
    let room: u64 = kani::any();
    kani::assume(room > 0 && room <= BAND);
    a_read_short_of_memory_holds_no_more_than_it_was_given(room);
    kani::cover!(room == 10_288, "a room of 10,288 bytes");
}

#[kani::proof]
fn a_read_just_short_of_its_pools_holds_no_more_than_it_was_given() {
    let room: u64 = kani::any();
    kani::assume(room < whole() && room >= whole() - NEAR);
    a_read_short_of_memory_holds_no_more_than_it_was_given(room);
    kani::cover!(room == whole() - 1, "a byte short of the whole");
}

/// `document_tree_bytes`: for **every** tree cap, the document part's tree no pool counts is its first [`OPEN_DOCUMENT_FREE_ITEMS`] items at most, at
/// [`TREE_ITEM_BYTES`] each: never more than 40,960,000 bytes, exactly that many items' worth up to the cap, and no less for a larger cap (monotone). Nothing wraps.
#[kani::proof]
fn the_document_parts_free_tree_is_at_most_its_free_items_and_grows_with_the_cap() {
    use crate::limits::document_tree_bytes;
    let (items, more): (u64, u64) = (kani::any(), kani::any());
    let bytes = document_tree_bytes(items);
    let counted = if items < OPEN_DOCUMENT_FREE_ITEMS { items } else { OPEN_DOCUMENT_FREE_ITEMS };
    assert!(bytes == counted * TREE_ITEM_BYTES);
    assert!(bytes <= 40_960_000);
    assert!(document_tree_bytes(items.saturating_add(more)) >= bytes);
    kani::cover!(bytes == 40_960_000, "the whole free tree");
}

/// `Pools::held_bytes`: for **any** pools within what a `u64` holds of bytes, what a read holds is the trees at [`TREE_ITEM_BYTES`] an item, the part twice, the
/// blobs, the model and the document part's free tree, summed exactly where the sum fits a `u64` and held at the largest number where it does not: it never wraps to less.
#[kani::proof]
fn a_reads_pools_hold_their_sum_and_never_wrap() {
    let pools = crate::limits::Pools {
        items: kani::any(),
        tree_items: kani::any(),
        part_bytes: kani::any(),
        blob_bytes: kani::any(),
        model_bytes: kani::any(),
    };
    let held = pools.held_bytes();
    let exact = u128::from(pools.tree_items) * u128::from(TREE_ITEM_BYTES)
        + u128::from(pools.part_bytes) * 2
        + u128::from(pools.blob_bytes)
        + u128::from(pools.model_bytes)
        + u128::from(crate::limits::document_tree_bytes(pools.tree_items));
    assert!(u128::from(held) == exact.min(u128::from(u64::MAX)));
    kani::cover!(held == u64::MAX, "pools past what a u64 holds");
    kani::cover!((held < u64::MAX) & (pools.blob_bytes > 0), "pools that fit");
}

/// `write::keep_of` (the bytes of parts the codec's first pass may hold for its second and make together, of what the meter has) for **every** figure left: none below a
/// gibibyte, so a save near the edge of what the process may hold is the save it always was; and from it an eighth of what is left, never more than 384 MiB.
#[kani::proof]
fn a_save_keeps_an_eighth_of_its_meter_for_its_parts_from_a_gibibyte_and_never_past_384_mebibytes() {
    let left: u64 = kani::any();
    let got = crate::write::keep_of(left);
    if left < 1 << 30 {
        assert!(got == 0);
    } else {
        assert!(got <= 384 << 20 && u128::from(got) * 8 <= u128::from(left));
        assert!(got == 384 << 20 || u128::from(got) * 8 + 8 > u128::from(left));
    }
    kani::cover!((got > 0) & (got < 384 << 20), "an eighth of the meter");
    kani::cover!(got == 384 << 20, "the most kept");
}
