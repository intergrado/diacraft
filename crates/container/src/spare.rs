//! How many threads a piece of work may use **beside what it may hold**: **the one rule** by which a read, a save (`diacraft_engine::threads`) and a frame
//! (`diacraft_render::Parallel::usable_threads`) hold their threads to the address space ([`threads_fit`]).
//!
//! A thread that allocates takes address space for as long as the process lives: its stack and the arena of the allocator that is made for it (66 MiB on Linux), **and the
//! heaps that arena grows to hold what the thread allocates, which the allocator keeps after the thread is gone**: a thread that parses trees of a few megabytes at a time was measured to leave 66 to
//! 150 MiB of address space mapped for the rest of the process's life. That is no memory, but a process under an address-space limit (`ulimit -v`, which the memory tests of this project set, and which a
//! container may set) spends it from what it may map, and a process that cannot map what it asks for **aborts**, and not only in the read: the save that follows needs the space the read's threads left
//! mapped (a frame drawn on threads for the thumbnail of an `.odg` of 50,000 shapes aborted the save that followed it when it took half of what the limit left at 80 MiB a thread). A piece of work is
//! held to a budget of memory, which is what it may hold beside the address space it has already: the threads it may use are the ones whose cost ([`THREAD_COST_BYTES`]) fits in an eighth of what the
//! limit leaves beside the process's own and the work's whole budget ([`threads_fit`]; a read also puts no more than [`READ_THREADS_MOST`] to work, [`threads_beside`]). Where the process has no limit,
//! or the system does not say (only Linux does), every thread asked for is used.

use crate::par;

/// The most threads a read puts to work whatever it is asked for: the read builds its model on one thread, and the others (inflating the package, parsing the parts ahead) keep up with it, which a few do.
pub const READ_THREADS_MOST: usize = 8;

/// What a thread that works for a read, a save or a frame takes of the address space for good: its stack, its arena and the heaps the arena grows to hold what it makes (192 MiB; 66 to 150 MiB was
/// measured for a read).
pub const THREAD_COST_BYTES: u64 = 192 << 20;

/// The memory a read is taken to hold at the most when its limits do not say (the budget of the VSDX reader, 2.6 GB).
pub const READ_ROOM_BYTES: u64 = 2_600_000_000;

/// The address space this process has mapped now, in bytes: `None` where the system does not say.
pub fn address_space_used() -> Option<u64> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    size_in(&std::fs::read_to_string("/proc/self/status").ok()?)
}

/// `VmSize` of the text of `/proc/self/status`, in bytes.
fn size_in(status: &str) -> Option<u64> {
    let line = status.lines().find_map(|line| line.strip_prefix("VmSize:"))?;
    let kib: u64 = line.split_whitespace().next()?.parse().ok()?;
    kib.checked_mul(1024)
}

/// How many of the `wanted` threads (held to 1 to [`par::MAX_THREADS`]) a piece of work that may hold `room` bytes may use: all of them where the process has no address-space limit, and where it
/// has one, the threads ([`THREAD_COST_BYTES`] each, the thread of the caller counted for nothing) that fit in an eighth of what is left of the limit beside the address space in use now and `room`;
/// one (no thread is made) when none fits, or when the figures are not known. **A read, a save and a frame are each held by it**, each passing what it may hold.
pub fn threads_fit(wanted: usize, room: u64) -> usize {
    fit(wanted.clamp(1, par::MAX_THREADS), par::address_space_limit(), address_space_used(), room)
}

/// How many of the `wanted` threads a read that may hold `room` bytes may use, whatever its caller asks (never more than [`READ_THREADS_MOST`]): [`threads_fit`] of them.
pub fn threads_beside(wanted: usize, room: u64) -> usize {
    beside(wanted, par::address_space_limit(), address_space_used(), room)
}

pub(crate) fn beside(wanted: usize, limit: Option<u64>, used: Option<u64>, room: u64) -> usize {
    fit(wanted.clamp(1, READ_THREADS_MOST), limit, used, room)
}

/// `wanted` (at least 1) held to the threads whose cost fits in an eighth of what `limit` leaves beside `used` and `room`.
pub(crate) fn fit(wanted: usize, limit: Option<u64>, used: Option<u64>, room: u64) -> usize {
    match limit {
        None => wanted,
        Some(limit) => {
            let Some(used) = used else { return 1 };
            let fit = usize::try_from(limit.saturating_sub(used).saturating_sub(room) / 8 / THREAD_COST_BYTES).unwrap_or(usize::MAX);
            wanted.min(fit.saturating_add(1))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_threads_of_a_read_are_held_to_the_few_that_keep_up_and_to_the_address_space_that_is_left() {
        let t = THREAD_COST_BYTES;
        assert_eq!(beside(31, None, None, 1 << 40), READ_THREADS_MOST, "no limit: as many as are useful");
        assert_eq!(beside(3, None, None, 0), 3);
        assert_eq!(beside(0, None, None, 0), 1, "no thread asked for is the thread of the caller");
        assert_eq!(
            beside(31, Some(3_072_000_000), Some(250_000_000), 2_600_000_000),
            1,
            "a read that may hold the budget of the VSDX reader leaves no room under 3 GB"
        );
        assert_eq!(beside(31, Some(3_072_000_000), Some(250_000_000), 1_200_000_000), 2, "room for one worker beside the caller's thread");
        assert_eq!(beside(31, Some(3_072_000_000), Some(100_000_000), 0), 2, "under 3 GB even a read of nothing has room for one worker");
        assert_eq!(beside(31, Some(16 << 30), Some(100_000_000), 0), 8, "room for seven");
        assert_eq!(beside(31, Some(8 * 3 * t), Some(0), 0), 4);
        assert_eq!(beside(31, Some(8 * t - 1), Some(0), 0), 1);
        assert_eq!(beside(31, Some(1 << 30), None, 0), 1, "a limit and no figure for what is in use: no thread");
        assert_eq!(beside(31, Some(100), Some(200), 0), 1, "in use past the limit: no thread");
    }

    #[test]
    fn a_frame_and_a_save_are_held_by_the_rule_of_a_read_and_only_a_read_to_a_few_threads() {
        let t = THREAD_COST_BYTES;
        assert_eq!(fit(31, None, None, 1 << 40), 31, "no limit: every thread asked for");
        assert_eq!(fit(64, None, None, 0), 64);
        assert_eq!(fit(31, Some(3_072_000_000), Some(250_000_000), 400_000_000), 2, "under 3 GB beside a frame's 400 MB: one worker");
        assert_eq!(fit(31, Some(16_384_000_000), Some(250_000_000), 400_000_000), 10);
        assert_eq!(fit(31, Some(8 * 3 * t), Some(0), 0), 4, "as a read, past the few a read puts to work");
        assert_eq!(fit(31, Some(8 * t - 1), Some(0), 0), 1);
        assert_eq!(fit(31, Some(1 << 30), None, 0), 1, "a limit and no figure for what is in use: no thread");
        assert_eq!(fit(31, Some(u64::MAX), Some(0), 0), 31, "never more than were wanted, and no overflow");
        assert_eq!(fit(1, Some(u64::MAX), Some(0), 0), 1, "one is the thread of the caller");
        for (limit, used, room) in [
            (None, None, 0),
            (Some(3_072_000_000), Some(250_000_000), 0),
            (Some(16 << 30), Some(100_000_000), 1 << 30),
            (Some(8 * 3 * t), Some(0), 0),
        ] {
            assert_eq!(
                beside(31, limit, used, room),
                fit(31, limit, used, room).min(READ_THREADS_MOST),
                "a read is the rule held to {READ_THREADS_MOST}"
            );
        }
        // In this process (no address-space limit), the count is the one asked for, held to the range.
        if par::address_space_limit().is_none() {
            assert_eq!(threads_fit(31, u64::MAX), 31);
            assert_eq!(threads_fit(0, 0), 1);
            assert_eq!(threads_fit(500, 0), par::MAX_THREADS);
        }
    }

    #[test]
    fn the_address_space_in_use_is_read_from_the_text_of_the_systems_status() {
        assert_eq!(size_in("Name:\tx\nVmPeak:\t  5000 kB\nVmSize:\t  4096 kB\nVmLck:\t0 kB\n"), Some(4_194_304));
        assert_eq!(size_in("VmSize: many kB"), None);
        assert_eq!(size_in("Name: x"), None);
        assert_eq!(size_in("VmSize: 18446744073709551615 kB"), None);
    }
}
