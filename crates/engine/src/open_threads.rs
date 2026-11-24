//! How many threads the engine's opens may use.
//!
//! The engine chooses the count and each reader takes it in its limits (`diacraft_vsdx::Limits::threads`, `diacraft_odg::Limits::threads`, `diacraft_native::Limits::threads`): by default the
//! cores of the machine less one (a core is left to the window), at least one and at most 64; the environment variable `DIACRAFT_THREADS` sets it, for tests and measurement (`1` is the
//! single-threaded read, which makes no thread). **A file opened is the same document, report and refusal at any count.** On the web build the count is always 1: no thread is made. Each reader holds
//! the count to the few threads that keep up with the one that builds the model (`diacraft_container::spare::READ_THREADS_MOST`) and to the address space the process has left beside the memory the
//! read may hold, so that a process under an address-space limit (`ulimit -v`) uses fewer threads or none and never aborts for them.

/// The threads an open may use: `diacraft_container::par::default_threads`, read when an open begins.
pub(crate) fn open_threads() -> usize {
    diacraft_container::par::default_threads()
}
