//! Kani proof harnesses for the codec's integer code: the [`Budget`] a decode is charged to (its pools, the room of the blobs the model may borrow, and the trees held
//! back beside it), the items a save may give a tree beside the model it holds (`save_tree_items`, `save_child_items`), the chunks of the table of shared units
//! And the charge of a part made on a thread (`save::expected_bytes`); the price of an allocation (`price::block`, `heap`, `arc_str`),
//! the law the memory tests hold a model to (`model::cost::law_bytes`, and `cells_bytes` up to eleven cells and its floor up to 2^20), and the charge of a
//! text (`model::cost::text_bytes`; `Budget::charge_text` is `charge_bytes` of it, by its definition). Run with
//! `cargo xtask kani` (`docs/development.md`, Proofs).
//!
//! **Not proved:** `cells_bytes` monotone for every count (two walks of the levels above the leaves, each a loop decided by quotients by 12: the trap of a loop that ends
//! by a division); `model::cost::element_bytes` over the vocabulary (a match of names, which the unit test over `vocab::ELEMENTS` enumerates exactly); the figures built
//! from the law (constants, which their unit tests read).
//!
//! **Not proved here: the bytes a `SheetReader` sets aside for the work ahead of a read** (`set_aside`, `set_aside_back`, `give_bytes_later`, `settle_lookahead`, `take_exact`,
//! `give_bytes`). A reader is made with a hash map (`SheetReader::new`), and making it reaches the standard library's futex (`futex_wake_all`) through the foreign function `syscall`, which Kani 0.68 does not
//! model: the harness, an induction on what is left, set aside and charged, failed at once on it. The unit tests of `read.rs` and the open's tests hold the figures.
//!
//! **What a budget is here.** A harness starts from [`Budget::new`] with symbolic pools, with the blobs' room lent or not, and brings it to any state a decode can be in
//! (a blob charged, what is left of each pool any amount below what it had, a tree held back). The pools are at most `2^62` bytes, so that a sum of two of
//! them is no overflow: the budgets the crates make are at most the open's 1,344 MiB ([`OPEN_POOL_BYTES`]) and a package reader's share of it, and the decoder of the
//! `.odg` document file gives its blobs `u64::MAX / 2` with the pools apart, where no sum of the two is made.

use diacraft_container::Limits;

use crate::model::cost::{BASE_BYTES, TREE_ITEM_BYTES};
use crate::model::{
    ADDRESS_SPACE_BYTES, Budget, CodecError, OPEN_BLOB_BYTES, OPEN_ELEMENT_ITEMS, OPEN_ITEMS, OPEN_MODEL_BYTES, OPEN_PACKAGE_BYTES, OPEN_PART_BYTES,
    OPEN_POOL_BYTES, OPEN_TREE_ITEMS, SECTION_STREAM_BYTES, child_items_of, save_child_items, save_tree_items,
};

/// The most a pool of a harness's budget is: a sum of two is no overflow.
const MOST: u64 = 1 << 62;

/// A budget in any state a decode can reach, with what it was made with: `Budget::new(items, bytes, blobs)`, the blobs' room lent when `lend`, a blob charged (perhaps
/// refused), what is left of the model's pool and of the blobs' set to any amount below what they had (what the charges of the model and of the blobs leave), and a tree held
/// back (perhaps refused). → the budget, its `bytes`, its `blobs`, and what the tree held back.
fn state(lend: bool) -> (Budget, u64, u64, u64) {
    let (items, bytes, blobs): (u64, u64, u64) = (kani::any(), kani::any(), kani::any());
    kani::assume(bytes <= MOST && blobs <= MOST);
    let mut b = Budget::new(items, bytes, blobs);
    if lend {
        b = b.with_lent_blobs();
    }
    let _ = b.charge_blob(kani::any()); // refused or not: either is a state a decode can be in
    let (model_left, blobs_left): (u64, u64) = (kani::any(), kani::any());
    kani::assume(model_left <= b.bytes && blobs_left <= b.blob_bytes);
    b.bytes = model_left;
    b.blob_bytes = blobs_left;
    let tree: u64 = kani::any();
    kani::assume(tree <= MOST / TREE_ITEM_BYTES);
    let held = b.reserve_tree(tree).unwrap_or(0);
    (b, bytes, blobs, held)
}

/// What a charge of the model that does not fit is refused as: the pool of the model, and the blobs' when they are lent, as the budget was made.
fn is_model_refusal(e: &CodecError, bytes: u64, blobs: u64, lend: bool) -> bool {
    matches!(e, CodecError::Limit { what: "bytes of the model in memory", max } if *max == bytes + if lend { blobs } else { 0 })
}

/// `Budget::charge_bytes` and `Budget::room` (a formula was afforded by `bytes` where `room` was meant, and 58,000 rectangles were refused with room left): a
/// charge of the model is taken **exactly when it is no more than the room**, and the model's own pool is spent before the blobs' room; a charge past it is refused in the
/// pool's words and changes nothing.
#[kani::proof]
#[kani::unwind(46)] // the refusal's words are compared byte by byte, at most 45 of them
fn a_charge_of_the_model_is_taken_exactly_when_it_fits_the_room_and_spends_its_own_pool_first() {
    let lend: bool = kani::any();
    let (mut b, bytes, blobs, _) = state(lend);
    let before = b.clone();
    let room = b.room();
    let n: u64 = kani::any();
    match b.charge_bytes(n) {
        Ok(()) => {
            assert!(n <= room);
            if n <= before.bytes {
                assert!(b.bytes == before.bytes - n && b.blob_bytes == before.blob_bytes);
            } else {
                assert!(lend && b.bytes == 0 && b.blob_bytes == before.blob_bytes - (n - before.bytes));
            }
            assert!(b.blobs_charged() == before.blobs_charged() && b.items == before.items);
            kani::cover!((n == room) & (n > 0), "the last byte of the room");
            kani::cover!(n > before.bytes, "the model takes the room the blobs have not spent");
        }
        Err(e) => {
            assert!(n > room);
            assert!(is_model_refusal(&e, bytes, blobs, lend));
            assert!(b == before);
            kani::cover!(n == room + 1, "one byte past the room");
            kani::cover!(!lend & (n <= before.bytes + before.blob_bytes), "the blobs' room is not the model's when it is not lent");
        }
    }
}

/// `Budget::reserve_child` and `Budget::child_items` (B1): a child of a part read as a stream is held back **exactly when the room holds what it has beyond
/// the tree pool**, at [`TREE_ITEM_BYTES`] an item; the room falls by that and `release_tree` gives it back to the byte. The items the stream is told a child may have
/// (`child_items`) are never more than the child limit, a child of that many is held back, and one more is refused unless the limit, not the room, is what binds.
#[kani::proof]
fn a_child_is_held_back_exactly_when_the_room_holds_what_it_has_beyond_the_tree_pool() {
    let lend: bool = kani::any();
    let (b, _, _, _) = state(lend);
    let (pool, nodes): (u64, usize) = (kani::any(), kani::any());
    kani::assume(pool <= OPEN_TREE_ITEMS);
    let mut limits = Limits::default();
    limits.xml.max_nodes = nodes;
    let mut b = b.with_tree_pool(pool, &limits);
    let before = b.clone();
    let room = b.room();
    let items: u64 = kani::any();
    kani::assume(items <= MOST / TREE_ITEM_BYTES);
    let beyond = items.saturating_sub(pool) * TREE_ITEM_BYTES;
    match b.reserve_child(items) {
        Ok(held) => {
            assert!(held == beyond && beyond <= room);
            assert!(b.room() == room - held);
            b.release_tree(held);
            assert!(b == before);
            kani::cover!((items > pool) & (beyond == room), "a child that takes the whole room");
            kani::cover!((items <= pool) & (items > 0), "a child that the tree pool holds alone");
        }
        Err(_) => {
            assert!(beyond > room);
            assert!(b == before);
            kani::cover!(beyond == room / TREE_ITEM_BYTES * TREE_ITEM_BYTES + TREE_ITEM_BYTES, "one item past the room");
        }
    }
    let limit = child_items_of(&limits);
    let told = before.child_items();
    assert!(told <= limit);
    let mut fresh = before.clone();
    assert!(fresh.reserve_child(told).is_ok());
    if told < limit {
        let mut fresh = before.clone();
        assert!(fresh.reserve_child(told + 1).is_err());
        kani::cover!(told > pool, "the room, not the limit, binds a child");
    }
    kani::cover!(told == limit, "the limit binds a child");
}

/// `Budget::charge_blob` (second round: with the blobs' room lent, a blob took room that a tree held back and the model counted on): a blob is charged
/// **exactly when its own pool has it and, when the room is lent, the room of the two pools has it too**; it is counted in `blobs_charged` and lowers the room by itself only
/// when that is lent. A blob that does not fit is refused in the blobs' words and changes nothing.
#[kani::proof]
#[kani::unwind(46)] // the refusal's words are compared byte by byte, at most 45 of them
fn a_blob_is_charged_exactly_when_its_pool_and_the_lent_room_have_it() {
    let lend: bool = kani::any();
    let (mut b, _, blobs, _) = state(lend);
    let before = b.clone();
    let room = b.room();
    let n: u64 = kani::any();
    let fits = n <= before.blob_bytes && (!lend || n <= room);
    match b.charge_blob(n) {
        Ok(()) => {
            assert!(fits);
            assert!(b.blob_bytes == before.blob_bytes - n && b.bytes == before.bytes);
            assert!(b.blobs_charged() == before.blobs_charged() + n);
            kani::cover!(lend & (n == room) & (n > 0), "a blob that takes the last of the lent room");
        }
        Err(e) => {
            assert!(!fits);
            assert!(matches!(e, CodecError::Limit { what: "bytes of pictures and parts in the drawing", max } if max == blobs));
            assert!(b == before);
            kani::cover!(lend & (n <= before.blob_bytes), "a blob its own pool has, refused for what the model and a tree count on");
            kani::cover!(n > before.blob_bytes, "a blob past its own pool");
        }
    }
}

/// Whether `b`, made with `bytes` and `blobs` and holding back `held`, is within its pools: the model, the blobs and the trees held back take no more than the two pools together
/// (with the pools apart, the model and the trees no more than the model's pool, and the blobs no more than theirs), no pool is above what it was made with, the blobs were
/// charged no more than their pool lost, and the room is what is left less what is held back.
fn within(b: &Budget, bytes: u64, blobs: u64, held: u64, lend: bool) -> bool {
    if b.bytes > bytes || b.blob_bytes > blobs {
        return false;
    }
    let (model, pictures) = (bytes - b.bytes, blobs - b.blob_bytes);
    let pools = if lend { model + pictures + held <= bytes + blobs } else { model + held <= bytes };
    let room = if lend { b.bytes + b.blob_bytes - held } else { b.bytes - held };
    pools && b.blobs_charged() <= pictures && b.room() == room
}

/// The sum the pools are sized as (and its second round), **by induction over what a decode does**, the first half: a new budget is within its pools.
#[kani::proof]
fn a_new_budget_is_within_its_pools() {
    let lend: bool = kani::any();
    let (bytes, blobs): (u64, u64) = (kani::any(), kani::any());
    kani::assume(bytes <= MOST && blobs <= MOST);
    let fresh = Budget::new(kani::any(), bytes, blobs);
    let fresh = if lend { fresh.with_lent_blobs() } else { fresh };
    assert!(within(&fresh, bytes, blobs, 0, lend));
    kani::cover!(lend & (bytes > 0) & (blobs > 0), "a budget that lends the blobs' room");
}

/// The second half of the induction: from **any** state within the pools (one a decode can reach), the step `step` of any amount, taken or refused, leaves the budget within
/// them: a charge of the model (0) or of a blob (1), a tree (2) or a child (3) held back, or a release of what is held back (4). So whatever a decode does, the model, the blobs
/// and the trees held back never take more than the two pools together. → whether the step was taken, the budget after it, what is held back, the amount, and whether the room is
/// lent, for the harness's covers. (One harness for each step: a symbolic choice among the five took 161 s, each alone takes seconds.)
fn one_step_stays_within(step: u8) -> (bool, Budget, u64, u64, bool) {
    let lend: bool = kani::any();
    let (mut b, bytes, blobs, mut held) = state(lend);
    kani::assume(within(&b, bytes, blobs, held, lend));
    let n: u64 = kani::any();
    kani::assume(n <= MOST);
    let taken = match step {
        0 => b.charge_bytes(n).is_ok(),
        1 => b.charge_blob(n).is_ok(),
        2 => b.reserve_tree(n / TREE_ITEM_BYTES).map(|x| held += x).is_ok(),
        3 => b.reserve_child(n / TREE_ITEM_BYTES).map(|x| held += x).is_ok(),
        _ => {
            let back = n.min(held);
            b.release_tree(back);
            held -= back;
            true
        }
    };
    assert!(within(&b, bytes, blobs, held, lend));
    (taken, b, held, n, lend)
}

#[kani::proof]
fn a_charge_of_the_model_leaves_the_budget_within_its_pools() {
    let (taken, b, _, _, lend) = one_step_stays_within(0);
    kani::cover!(taken & lend & (b.bytes == 0) & (b.blob_bytes > 0), "the model took the blobs' room");
    kani::cover!(!taken, "a charge refused");
}

#[kani::proof]
fn a_charge_of_a_blob_leaves_the_budget_within_its_pools() {
    let (taken, b, held, _, lend) = one_step_stays_within(1);
    kani::cover!(!taken & lend & (held > 0), "a blob refused for what a tree held back counts on");
    kani::cover!(taken & (b.blobs_charged() > 0), "a blob charged");
}

#[kani::proof]
fn a_tree_held_back_leaves_the_budget_within_its_pools() {
    let (taken, _, held, _, lend) = one_step_stays_within(2);
    kani::cover!(taken & lend & (held > 0), "a tree held back from the lent room");
    kani::cover!(!taken, "a tree refused");
}

#[kani::proof]
fn a_child_held_back_leaves_the_budget_within_its_pools() {
    let (taken, _, held, _, _) = one_step_stays_within(3);
    kani::cover!(taken & (held > 0), "a child held back");
    kani::cover!(!taken, "a child refused");
}

#[kani::proof]
fn a_release_leaves_the_budget_within_its_pools() {
    let (_, _, held, n, _) = one_step_stays_within(4);
    kani::cover!((held > 0) & (n > 0), "part of what is held back given back");
}

/// `Budget::for_open`: an open's budget is never more than its pools, whatever the package and the tree its limits allow (items at most 64 million and an eighth
/// of the package, the model 960 MiB, the blobs 384 MiB and the package, and the model may have both: 1,344 MiB); the stream is told a child of at most the child limit, and on a
/// small machine's limits (a tree of fewer items than an open's) of no more than that tree; and a child of what it is told is held back. At the largest package the figures are the
/// open's own.
#[kani::proof]
fn an_opens_budget_is_its_pools_and_a_child_is_no_larger_than_a_small_machines_tree() {
    let mut limits = Limits::default();
    limits.max_package_bytes = kani::any();
    limits.xml.max_nodes = kani::any();
    let package = limits.max_package_bytes;
    let b = Budget::for_open(&limits);
    assert!(b.items <= OPEN_ITEMS && b.items <= package / 8);
    assert!(b.bytes <= OPEN_MODEL_BYTES && b.blob_bytes <= OPEN_BLOB_BYTES && b.blob_bytes <= package);
    assert!(b.room() == b.bytes + b.blob_bytes && b.room() <= OPEN_POOL_BYTES);
    let told = b.child_items();
    assert!(told <= OPEN_ELEMENT_ITEMS);
    let small = (limits.xml.max_nodes as u64) < OPEN_TREE_ITEMS;
    if small {
        assert!(told <= limits.xml.max_nodes as u64);
    }
    let mut child = b.clone();
    assert!(child.reserve_child(told).is_ok());
    if package >= OPEN_PACKAGE_BYTES {
        assert!(b.items == OPEN_ITEMS && b.bytes == OPEN_MODEL_BYTES && b.blob_bytes == OPEN_BLOB_BYTES);
        if !small {
            assert!(told == OPEN_ELEMENT_ITEMS);
        }
    }
    kani::cover!(small & (told == limits.xml.max_nodes as u64) & (told > 0), "a small machine's tree binds a child");
    kani::cover!((package < OPEN_PACKAGE_BYTES) & (package > 0) & (b.blob_bytes == package), "a small package's blobs");
    kani::cover!(!small & (told < OPEN_ELEMENT_ITEMS), "the room binds a child of a small package");
}

/// `save_tree_items` and `save_child_items` (F2): the tree a save holds beside a model of `held` bytes, with the check's tables, a part's bytes, the blobs and what a
/// process has, **fits in the address space with a twentieth to spare**, it uses that room to within one item (or is at its cap: a tree of an open, or the child limit), it is
/// none when the model leaves no room, and it never grows as the model does.
#[kani::proof]
#[kani::unwind(3)]
fn a_saves_tree_fits_beside_the_model_uses_the_room_and_shrinks_as_the_model_grows() {
    let (held, more): (u64, u64) = (kani::any(), kani::any());
    kani::assume(held <= more);
    let most = ADDRESS_SPACE_BYTES - ADDRESS_SPACE_BYTES / 20;
    let fixed = SECTION_STREAM_BYTES + OPEN_PART_BYTES + OPEN_BLOB_BYTES + BASE_BYTES;
    for (items, cap) in [(save_tree_items(held), OPEN_TREE_ITEMS), (save_child_items(held), OPEN_ELEMENT_ITEMS)] {
        assert!(items <= cap);
        if held <= most - fixed {
            assert!(held + fixed + items * TREE_ITEM_BYTES <= most);
            assert!(items == cap || held + fixed + (items + 1) * TREE_ITEM_BYTES > most);
        } else {
            assert!(items == 0);
        }
    }
    assert!(save_tree_items(more) <= save_tree_items(held) && save_child_items(more) <= save_child_items(held));
    assert!(save_tree_items(held) <= save_child_items(held));
    kani::cover!((save_child_items(held) > 0) & (save_child_items(held) < OPEN_ELEMENT_ITEMS), "the room binds a child");
    kani::cover!((save_tree_items(held) == OPEN_TREE_ITEMS) & (save_child_items(held) == OPEN_ELEMENT_ITEMS), "a small model: both at their caps");
    kani::cover!((held <= most - fixed) & (save_child_items(held) == 0), "a model that leaves less than an item");
}

// --- The table of shared units (`model/share.rs`) ---

/// The most a chunk of streams is long, unless a stream is longer: a mebibyte (`LAST_CHUNK_BYTES`).
const LAST_CHUNK: usize = 1 << 20;

/// `Table::new_chunk_for` (`chunk_for`) and `Table::chunk_slack`, for **any** last chunk (any capacity and length, a length past the capacity included) and
/// any stream: a stream that fits in the room at the end of the last chunk makes no chunk and no slack; any other makes a chunk at least as long as itself, as long as itself
/// for the first chunk, and else twice the last one's capacity up to a mebibyte (or the stream, when that is longer); the slack is that chunk less the stream. Nothing wraps.
#[kani::proof]
fn a_stream_makes_a_chunk_exactly_when_the_last_has_no_room_for_it() {
    use crate::model::chunk_for;
    let (capacity, used, len): (usize, usize, usize) = (kani::any(), kani::any(), kani::any());
    let there: bool = kani::any();
    let last = there.then_some((capacity, used));
    let made = chunk_for(last, len);
    let room = if there { capacity.saturating_sub(used) } else { 0 };
    match made {
        None => {
            assert!(there && room >= len);
        }
        Some(size) => {
            assert!(size >= len);
            if there {
                assert!(room < len);
                let doubled = if capacity > usize::MAX / 2 { usize::MAX } else { capacity * 2 };
                let grown = if doubled < LAST_CHUNK { doubled } else { LAST_CHUNK };
                assert!(size == if grown > len { grown } else { len });
            } else {
                assert!(size == len);
            }
        }
    }
    // The slack `Table::chunk_slack` charges: the chunk less the stream, and none when the stream fits.
    let slack = made.map_or(0, |size| size.saturating_sub(len));
    assert!(slack == made.map_or(0, |size| size - len));
    kani::cover!(there & made.is_some_and(|size| size == LAST_CHUNK) & (len < LAST_CHUNK), "a chunk of a mebibyte");
    kani::cover!(there & made.is_some_and(|size| size == len) & (len > LAST_CHUNK), "a stream longer than a mebibyte");
    kani::cover!(there & (capacity > usize::MAX / 2) & made.is_some(), "a capacity whose double does not fit");
}

/// `stream_in` (`Table::stream_of`), on two chunks of up to three bytes, for **any** chunk, start and length a slot can hold (three `u32`s): the stream is there
/// exactly when the chunk is and holds the bytes from the start to the start and the length, and it is those bytes; nothing wraps and nothing panics.
#[kani::proof]
#[kani::unwind(4)]
fn a_slots_stream_is_there_exactly_when_its_chunk_holds_it() {
    use crate::model::stream_in;
    let (first, second): ([u8; 3], [u8; 3]) = (kani::any(), kani::any());
    let (cut_first, cut_second): (usize, usize) = (kani::any(), kani::any());
    kani::assume(cut_first <= 3 && cut_second <= 3);
    let chunks = vec![first[..cut_first].to_vec(), second[..cut_second].to_vec()];
    let (chunk, start, len): (u32, u32, u32) = (kani::any(), kani::any(), kani::any());
    let held = chunks.get(chunk as usize).map(Vec::len);
    let end = u64::from(start) + u64::from(len);
    match stream_in(&chunks, chunk, start, len) {
        Some(bytes) => {
            let whole = held.unwrap_or(0);
            assert!(end <= whole as u64 && bytes.len() == len as usize);
            assert!(bytes.first() == chunks[chunk as usize].get(start as usize) || len == 0);
        }
        None => {
            assert!(held.is_none_or(|whole| end > whole as u64));
        }
    }
    kani::cover!((chunk == 1) & (len > 0) & (end == cut_second as u64), "a stream to the end of the second chunk");
    kani::cover!((chunk < 2) & (start == u32::MAX), "a start past every chunk");
}

// ---: the charge of a part made on a thread ---

/// `save::expected_bytes` (what a part made on a thread is charged and held to) for **every** count of shapes at its top: 4 KiB a shape, and never less than 256 KiB;
/// the largest number where the product passes it.
#[kani::proof]
fn a_part_made_on_a_thread_is_charged_four_kibibytes_a_shape_and_at_least_a_quarter_mebibyte() {
    use crate::model::expected_bytes;
    let shapes: usize = kani::any();
    let got = u128::from(expected_bytes(shapes));
    let product = (shapes as u128 * 4096).min(u128::from(u64::MAX));
    assert!(got == product.max(256 * 1024));
    kani::cover!(got == 256 * 1024, "a part of few shapes");
    kani::cover!(got == u128::from(u64::MAX), "a product past the largest number");
}

// ---: the price of an allocation and the law the memory tests hold a model to ---

/// `price::block` (what a string or a buffer of `n` bytes is priced at) for **every** size: **the bytes asked for and 24 more, rounded up to sixteen and never under
/// thirty-two**. The figure is a multiple of 16, at least 32, at least `n + 24` and the least such (it is 32, or under `n + 40`), exactly as far as the largest number goes, and past
/// it the largest number where the rounding passes it: never a figure under `n + 24` that a wrap would give. From 9 bytes up it is glibc's chunk ([`crate::model::cost::chunk`]) and 16, the law's 24 an
/// allocation where glibc adds 8 to 23; below, both are 32. `heap` is `block`, and an `Arc<str>` is the block of its text and its two counts.
#[kani::proof]
fn an_allocation_is_priced_at_its_bytes_and_twenty_four_more_rounded_up_to_sixteen_and_never_under_thirty_two() {
    use crate::model::cost::chunk;
    use crate::price::{arc_str, block, heap};
    let n: usize = kani::any();
    let got = u128::from(block(n));
    let asked = n as u128 + 24;
    let most = u128::from(u64::MAX);
    let exact = got % 16 == 0 && got >= 32 && got >= asked && (got == 32 || got < asked + 16);
    if asked + 15 <= most {
        assert!(exact);
        if n >= 9 {
            assert!(got == chunk(n) as u128 + 16);
        } else {
            assert!(got == 32 && chunk(n) == 32);
        }
    } else {
        assert!(exact || got == most);
    }
    assert!(got >= asked.min(most));
    assert!(heap(n) == block(n) && arc_str(n) == block(n.saturating_add(16)));
    kani::cover!((got == 32) & (n == 8), "the most bytes the least block holds");
    kani::cover!((got == most) & (asked <= most), "a size just short of the largest number, rounded past it");
}

/// `price::block` is **monotone**: for every two sizes, the larger is priced no lower, so a charge that grew with its object never shrinks (two symbolic sizes).
#[kani::proof]
fn a_larger_allocation_is_never_priced_lower() {
    use crate::price::block;
    let (n, m): (usize, usize) = (kani::any(), kani::any());
    kani::assume(n <= m);
    assert!(block(n) <= block(m));
    kani::cover!((block(n) < block(m)) & (n.checked_add(1) == Some(m)), "one more byte that takes a block of sixteen more");
}

/// `model::cost::law_bytes` (what the law of the memory tests holds an allocation of `n` bytes to) for **every** size: the bytes, an eighth of them (rounded
/// down) and 24, rounded up to sixteen: a multiple of 16, at least that and under it and 16, exactly as far as the largest number goes, and past it the largest number where the
/// rounding passes it. It is never
/// under 32, and monotone (a second size, beside it).
#[kani::proof]
fn the_law_holds_an_allocation_to_its_bytes_an_eighth_and_twenty_four_rounded_up_to_sixteen() {
    use crate::model::cost::law_bytes;
    let (n, m): (usize, usize) = (kani::any(), kani::any());
    let got = law_bytes(n) as u128;
    let need = n as u128 + (n / 8) as u128 + 24;
    let most = usize::MAX as u128;
    let exact = got % 16 == 0 && got >= need && got < need + 16;
    if need + 15 <= most {
        assert!(exact);
    } else {
        assert!(exact || got == most);
    }
    assert!(got >= 32);
    if n <= m {
        assert!(law_bytes(n) <= law_bytes(m));
    }
    kani::cover!(got == need, "a size whose law is a multiple of sixteen already");
    kani::cover!((got == most) & (need <= most), "a size just short of the largest number, rounded past it");
}

/// `model::cost::cells_bytes` (the nodes of a map of `n` cells, built from sorted pairs, at the law) for **every** count up to eleven: nothing for no cell, and the one
/// leaf of eleven slots (`MODEL_CELLS_LEAF_BYTES`) for one to eleven, which a map allocates whole at its first cell.
#[kani::proof]
#[kani::unwind(2)] // the loop over the levels above the leaves does not run: without a bound CBMC unrolls it for ever (a quotient decides it)
fn a_map_of_up_to_eleven_cells_is_one_leaf_and_one_of_none_is_nothing() {
    use crate::model::cost::{MODEL_CELLS_LEAF_BYTES, cells_bytes};
    let n: usize = kani::any();
    kani::assume(n <= 11);
    let got = cells_bytes(n);
    assert!(got == if n == 0 { 0 } else { MODEL_CELLS_LEAF_BYTES });
    kani::cover!((n == 11) & (got == MODEL_CELLS_LEAF_BYTES), "a full leaf");
}

/// `model::cost::cells_bytes` for **every** count of cells up to 2^20 (the most cells a part's budget pays for is far below it): never less than the leaves a map of so many cells
/// must have at eleven a leaf, each at the price of a leaf; so a place the file fills is never charged under the nodes it holds.
#[kani::proof]
#[kani::unwind(8)] // the levels above the leaves: six for 2^20 cells at twelve children a node
fn a_map_of_cells_is_charged_at_least_a_leaf_for_every_eleven_cells() {
    use crate::model::cost::{MODEL_CELLS_LEAF_BYTES, cells_bytes};
    let n: usize = kani::any();
    kani::assume(n <= 1 << 20);
    let got = u128::from(cells_bytes(n));
    // At least `leaves` leaves, where `11 (leaves - 1) < n <= 11 leaves`, stated by products.
    let leaves: u128 = kani::any();
    kani::assume(leaves <= 1 << 20 && 11 * leaves >= n as u128 && (leaves == 0 || 11 * (leaves - 1) < n as u128));
    assert!(got >= leaves * u128::from(MODEL_CELLS_LEAF_BYTES));
    kani::cover!((n == 12) & (got > 2 * u128::from(MODEL_CELLS_LEAF_BYTES)), "twelve cells: two leaves and the node over them");
}

/// `model::cost::text_bytes` (what a text of `len` bytes is charged) for **every** length, and a second beside it: nine eighths of the length rounded up
/// (`8 t >= 9 len` and `8 t < 9 len + 8`, stated by products), the largest number past it; never under the length, nothing for none, and monotone.
#[kani::proof]
fn a_text_is_charged_nine_eighths_of_its_length_rounded_up() {
    use crate::model::cost::text_bytes;
    let (len, more): (usize, usize) = (kani::any(), kani::any());
    let got = u128::from(text_bytes(len));
    let wide = len as u128;
    let most = u128::from(u64::MAX);
    if 9 * wide + 7 <= 8 * most {
        assert!(8 * got >= 9 * wide && 8 * got < 9 * wide + 8);
    } else {
        assert!(got == most);
    }
    assert!(got >= wide.min(most) && text_bytes(0) == 0);
    if len <= more {
        assert!(text_bytes(len) <= text_bytes(more));
    }
    kani::cover!((got == most) & (wide < most), "a length whose eighth passes the largest number");
}
