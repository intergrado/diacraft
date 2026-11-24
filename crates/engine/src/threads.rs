//! How many threads the engine's heavy work may use.
//!
//! The engine chooses the count and the work takes it as a parameter (`diacraft_vsdx::write_with_threads`, `diacraft_native::write_threads`): by default the cores of the machine less
//! one (a core is left to the window), at least one and at most 64; the environment variable `DIACRAFT_THREADS` sets it, for tests and measurement (`1` is the single-threaded path, which
//! makes no thread). **A file saved is the same bytes at any count.** On the web build the count is always 1: no thread is made. **The count is the one an open and a frame take**: each is
//! `diacraft_container::par::default_threads` (an open's `open_threads`, and `diacraft_render::default_threads`), and the engine's tests read them in child processes.
//!
//! **A process under an address-space limit** (`ulimit -v`, which the memory tests set) spends address space on each thread that allocates for as long as the process lives (its stack, the
//! allocator's arena and the heaps the arena keeps after the thread), and a process that cannot map what it asks for aborts, in the save or in what comes after it. So a save is held by **the rule
//! an open and a frame are held by** (`diacraft_container::spare::threads_fit`): where the system says there is a limit, the threads whose cost (192 MiB each) fits in an eighth of what the
//! limit leaves beside the address space in use and what the save may hold ([`save_threads_within`]), which is none (the single-threaded save) for a drawing near the edge of what a save may hold.
//! A process with no limit makes the threads it was asked for.

use diacraft_container::{par, spare};

/// The most threads a save may use: [`diacraft_container::par::default_threads`], read when a save begins.
pub fn save_threads() -> usize {
    par::default_threads()
}

/// The threads a save that may hold `room` bytes beside what the process has mapped may use: [`save_threads`], held to what a limit on the process's address space leaves room for
/// ([`diacraft_container::spare::threads_fit`]).
pub fn save_threads_within(room: u64) -> usize {
    spare::threads_fit(save_threads(), room)
}
