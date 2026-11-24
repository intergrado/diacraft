//! Files held in memory, for a build that has no file system: the browser.
//!
//! **Why.** The engine's commands name a file by its path: `file.open`, `file.save`, `file.save_as`, `file.export`, `file.print`, `stencil.open` and `insert.picture`. A browser has no path to give
//! them and nothing to read or write one with, so the page puts what the person chose here, under a name of its own making, and takes what a command wrote out again to hand it over (a download,
//! or a write to a file the person chose). **Every rule a command applies to a path stays as it is**: the size is looked at before a byte is read, the budgets and the refusals in words are the same,
//! and the check after a save reads what was written. The commands themselves know no difference, and nothing is added to what they take or answer.
//!
//! **When it is used.** Always on `wasm32`, where there is no file system, and on no other target unless a thread asks for it ([`use_on_this_thread`]): a desktop program never reads or writes
//! a file here, and the tests of the browser's flow, which run on the desktop, ask for it for their own thread. The store belongs to the thread that made it (a browser runs one), so a test
//! that asks never meets another test's files.
//!
//! **What is held, and how much.** An entry is a name and its bytes. A store holds at most [`MAX_ENTRIES`] entries and [`MAX_BYTES`] bytes in all (a write past either is refused, and the engine says so
//! in words: the save did not fit); its bytes are counted in the memory the process holds ([`crate::memory`]), so a read made beside them is given what is left. An entry is read in one of two ways, chosen
//! when it is put: **once** (the bytes are moved out to the reader, which holds them, and the entry is gone: a drawing, a picture), or **held** (every read copies, and the entry stays until it is
//! removed: a stencil, which the window opens again in each drawing it reads).
//!
//! **Nothing here is a link, a folder or a device**: every entry is a regular file, a path names one file only (no spelling of it is another), and a name that is not there is a new file.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::file::{FileError, FileStamp, SaveError};
use crate::memory::Hold;

/// The most entries a store holds. A drawing, a few stencils, a picture and what a save, an export or a print has written and not yet handed over come to far fewer.
pub const MAX_ENTRIES: usize = 64;

/// The most bytes a store holds in all: what the person has chosen to open and what is waiting to be handed over. A file the engine would not read in any case (past the budget of a read) is
/// refused sooner, from its size ([`admit`]).
pub const MAX_BYTES: u64 = 768 << 20;

/// How an entry is read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keep {
    /// The first read takes the bytes out of the store (no copy) and the entry is gone.
    Once,
    /// Every read copies the bytes, and the entry stays until it is removed.
    Held,
}

/// Why a file was not put.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PutError {
    /// The store holds [`MAX_ENTRIES`] already.
    TooMany,
    /// The bytes would pass [`MAX_BYTES`] in all, or the memory for them could not be had.
    NoRoom,
}

struct Entry {
    bytes: Vec<u8>,
    keep: Keep,
}

#[derive(Default)]
struct Store {
    files: BTreeMap<PathBuf, Entry>,
    /// The bytes of every entry, counted in the process's memory as long as they are held.
    hold: Option<Hold>,
}

impl Store {
    fn bytes(&self) -> u64 {
        self.files.values().map(|e| e.bytes.len() as u64).fold(0, u64::saturating_add)
    }

    /// Tells the process's ledger what the store holds now.
    fn recount(&mut self) {
        let held = self.bytes();
        match &self.hold {
            Some(hold) => hold.set(held),
            None => self.hold = Some(Hold::new(held)),
        }
    }
}

thread_local! {
    static ON: Cell<bool> = const { Cell::new(false) };
    static STORE: RefCell<Store> = RefCell::new(Store::default());
}

/// Whether this thread's files are in memory: always on `wasm32`, and on another target only where [`use_on_this_thread`] said so.
pub fn active() -> bool {
    cfg!(target_arch = "wasm32") || ON.with(Cell::get)
}

/// Makes this thread's files live in memory (`true`) or on the disk (`false`, what every thread starts with): for the tests of a flow that runs on the desktop as it runs in the browser. It
/// changes nothing on `wasm32`, where files are always in memory. Turning it off empties the store, so one test's files never reach another's.
pub fn use_on_this_thread(on: bool) {
    ON.with(|cell| cell.set(on));
    if !on {
        clear();
    }
}

/// Puts `bytes` under `path`, which replaces what was there. Refused, with nothing changed, when the store would hold more than [`MAX_ENTRIES`] or [`MAX_BYTES`].
pub fn put(path: &Path, bytes: Vec<u8>, keep: Keep) -> Result<(), PutError> {
    STORE.with(|store| {
        let mut store = store.borrow_mut();
        let replaced = store.files.get(path).map(|e| e.bytes.len() as u64);
        if replaced.is_none() && store.files.len() >= MAX_ENTRIES {
            return Err(PutError::TooMany);
        }
        if bytes_after_put(store.bytes(), replaced.unwrap_or(0), bytes.len() as u64).is_none() {
            return Err(PutError::NoRoom);
        }
        store.files.insert(path.to_path_buf(), Entry { bytes, keep });
        store.recount();
        Ok(())
    })
}

/// The bytes the store holds once an entry of `replaced` bytes among the `held` gives way to one of `new` bytes (`replaced` 0 for a new name), or `None` past [`MAX_BYTES`]: [`put`]'s count.
pub(crate) fn bytes_after_put(held: u64, replaced: u64, new: u64) -> Option<u64> {
    let after = held.saturating_sub(replaced).saturating_add(new);
    (after <= MAX_BYTES).then_some(after)
}

/// Puts a copy of `bytes` under `path`: what a save writes (the engine keeps its own bytes as the file it saved). The copy is made with `try_reserve`, so a drawing that cannot be had twice is
/// [`PutError::NoRoom`] and not an abort.
pub fn put_copy(path: &Path, bytes: &[u8], keep: Keep) -> Result<(), PutError> {
    // Looked at before the copy is made: a copy that would not be kept is not made.
    if bytes.len() as u64 > MAX_BYTES {
        return Err(PutError::NoRoom);
    }
    let mut copy = Vec::new();
    copy.try_reserve_exact(bytes.len()).map_err(|_| PutError::NoRoom)?;
    copy.extend_from_slice(bytes);
    put(path, copy, keep)
}

/// Whether there is an entry under `path`.
pub fn contains(path: &Path) -> bool {
    STORE.with(|store| store.borrow().files.contains_key(path))
}

/// The size of the entry under `path`.
pub fn len(path: &Path) -> Option<u64> {
    STORE.with(|store| store.borrow().files.get(path).map(|e| e.bytes.len() as u64))
}

/// How the entry under `path` is read.
pub fn keep_of(path: &Path) -> Option<Keep> {
    STORE.with(|store| store.borrow().files.get(path).map(|e| e.keep))
}

/// Removes the entry under `path`, and gives back its bytes (`None` when there is none): what the page takes of a file a command wrote, to hand it over.
pub fn take(path: &Path) -> Option<Vec<u8>> {
    STORE.with(|store| {
        let mut store = store.borrow_mut();
        let entry = store.files.remove(path)?;
        store.recount();
        Some(entry.bytes)
    })
}

/// Removes the entry under `path`, if there is one.
pub fn remove(path: &Path) {
    drop(take(path));
}

/// Removes every entry.
pub fn clear() {
    STORE.with(|store| {
        let mut store = store.borrow_mut();
        store.files.clear();
        store.recount();
    });
}

/// How many entries the store holds.
pub fn entries() -> usize {
    STORE.with(|store| store.borrow().files.len())
}

/// How many bytes the store holds in all.
pub fn bytes_held() -> u64 {
    STORE.with(|store| store.borrow().bytes())
}

/// The stamp of the entry under `path`: its size and no modified time, so a file in memory is compared by its size alone.
pub(crate) fn stamp(path: &Path) -> Option<FileStamp> {
    len(path).map(FileStamp::sized)
}

/// What `read_file_stamped` does for an entry: the bytes at most `max_bytes` long, and the stamp.
///
/// A name that is not there is [`FileError::NotFound`]; an entry past `max_bytes` is [`FileError::TooLarge`] and stays; a copy that cannot be had is [`FileError::OutOfMemory`] (an entry read
/// once is moved and needs none, so it is always had). A [`Keep::Once`] entry is gone after the read that has it.
pub(crate) fn read(path: &Path, max_bytes: u64) -> Result<(Vec<u8>, FileStamp), FileError> {
    STORE.with(|store| {
        let mut store = store.borrow_mut();
        let entry = store.files.get(path).ok_or(FileError::NotFound)?;
        let len = entry.bytes.len() as u64;
        if len > max_bytes {
            return Err(FileError::TooLarge { limit: max_bytes });
        }
        let stamp = FileStamp::sized(len);
        if entry.keep == Keep::Held {
            let mut copy = Vec::new();
            copy.try_reserve_exact(entry.bytes.len()).map_err(|_| FileError::OutOfMemory)?;
            copy.extend_from_slice(&entry.bytes);
            return Ok((copy, stamp));
        }
        let Some(moved) = store.files.remove(path) else { return Err(FileError::NotFound) };
        store.recount();
        Ok((moved.bytes, stamp))
    })
}

/// What `write_target` does for a file in memory: puts a copy of `bytes` under `target`, to be taken by whoever hands it over. A file that was held (a stencil's) is written as one read once: it is
/// what a command wrote, and not a file the person chose.
pub(crate) fn write(target: &Path, bytes: &[u8]) -> Result<(), SaveError> {
    // A store with no room, whether for the bytes or for another name, has no room for the file.
    put_copy(target, bytes, Keep::Once).map_err(|_| SaveError::NoRoom)
}

/// Whether a file of `size` bytes may be put at all, judged as the engine judges a file it is asked to read: it must be under the cap a package file has, and **it must leave room for the read**
/// beside what the process holds ([`crate::memory::read_limits`]), or `file.open` would refuse it (`out_of_memory`, or `too_big` when the file alone is more than the whole budget). The page asks
/// this **before it reads a byte of the file**, because a browser that has to make room for a file the engine will not read has nothing to give back (its memory only grows).
pub fn admit(size: u64) -> Result<(), crate::OpenProblem> {
    let cap = crate::file::package_file_cap(&diacraft_vsdx::Limits::default());
    admitted(size, cap, || crate::memory::read_limits().max_memory, bytes_held)
}

/// [`admit`]'s rule, of a file of `size` bytes, the cap of a package file, the room a read has and the bytes the store holds (each of the last two asked only when the rule comes to it).
pub(crate) fn admitted(size: u64, cap: u64, room: impl FnOnce() -> u64, held: impl FnOnce() -> u64) -> Result<(), crate::OpenProblem> {
    if size > cap {
        return Err(crate::OpenProblem::FileTooLarge);
    }
    let room = room();
    // The read counts the file in its own budget, so a file that is all the room is no file it can read (`cmd::file::load_for_read` says the same).
    if size >= room {
        return Err(if size >= diacraft_vsdx::MAX_MEMORY { crate::OpenProblem::TooBig } else { crate::OpenProblem::OutOfMemory });
    }
    // The page holds the file once itself while it reads it and the store holds it until the engine takes it: both must fit beside the rest.
    if size.saturating_add(held()) > MAX_BYTES {
        return Err(crate::OpenProblem::OutOfMemory);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn on() {
        use_on_this_thread(true);
        clear();
    }

    #[test]
    fn a_thread_starts_on_the_disk_and_a_test_turns_memory_on_for_its_own_thread() {
        assert!(!active() || cfg!(target_arch = "wasm32"));
        use_on_this_thread(true);
        assert!(active());
        use_on_this_thread(false);
        assert!(!active() || cfg!(target_arch = "wasm32"));
    }

    #[test]
    fn a_file_read_once_is_moved_out_and_is_gone() {
        on();
        let path = Path::new("/web/1/a.vsdx");
        put(path, vec![7; 10], Keep::Once).unwrap();
        assert_eq!((len(path), keep_of(path), bytes_held()), (Some(10), Some(Keep::Once), 10));
        let (bytes, stamp) = read(path, 100).unwrap();
        assert_eq!(bytes, vec![7; 10]);
        assert_eq!(stamp, FileStamp::sized(10));
        assert!(!contains(path), "the read took it");
        assert_eq!(bytes_held(), 0);
        assert_eq!(read(path, 100).unwrap_err(), FileError::NotFound);
        use_on_this_thread(false);
    }

    #[test]
    fn a_held_file_is_copied_and_stays_until_it_is_removed() {
        on();
        let path = Path::new("/web/2/shapes.vssx");
        put(path, vec![1, 2, 3], Keep::Held).unwrap();
        assert_eq!(read(path, 100).unwrap().0, vec![1, 2, 3]);
        assert_eq!(read(path, 100).unwrap().0, vec![1, 2, 3], "again");
        assert!(contains(path));
        remove(path);
        assert!(!contains(path) && bytes_held() == 0);
        use_on_this_thread(false);
    }

    #[test]
    fn a_file_past_the_cap_is_refused_and_stays() {
        on();
        let path = Path::new("/web/3/big.vsdx");
        put(path, vec![0; 50], Keep::Once).unwrap();
        assert_eq!(read(path, 49).unwrap_err(), FileError::TooLarge { limit: 49 });
        assert!(contains(path), "a refusal takes nothing");
        assert!(read(path, 50).is_ok());
        use_on_this_thread(false);
    }

    #[test]
    fn a_put_past_the_entries_or_the_bytes_is_refused_and_changes_nothing() {
        on();
        for n in 0..MAX_ENTRIES {
            put(Path::new(&format!("/web/{n}/f")), vec![0; 1], Keep::Once).unwrap();
        }
        assert_eq!(put(Path::new("/web/new"), vec![0], Keep::Once), Err(PutError::TooMany));
        // A name that is there is replaced, not added.
        put(Path::new("/web/0/f"), vec![0; 5], Keep::Once).unwrap();
        assert_eq!(entries(), MAX_ENTRIES);
        assert_eq!(len(Path::new("/web/0/f")), Some(5));
        clear();
        assert_eq!(put_copy(Path::new("/x"), &vec![0u8; usize::try_from(MAX_BYTES).unwrap() + 1], Keep::Once), Err(PutError::NoRoom));
        assert_eq!(entries(), 0);
        use_on_this_thread(false);
    }

    /// What the store's hold says it holds now (the process's total also holds other threads' drawings, so a test reads the store's own share).
    fn held_by_the_store() -> Option<u64> {
        STORE.with(|store| store.borrow().hold.as_ref().map(Hold::bytes))
    }

    #[test]
    fn what_the_store_holds_is_counted_in_the_memory_the_process_holds() {
        on();
        put(Path::new("/web/9/a"), vec![0; 4096], Keep::Held).unwrap();
        assert_eq!(held_by_the_store(), Some(4096));
        assert!(crate::memory::held() >= 4096, "the process's total has it");
        put(Path::new("/web/9/a"), vec![0; 1000], Keep::Held).unwrap();
        assert_eq!(held_by_the_store(), Some(1000), "a replaced entry is counted once");
        put(Path::new("/web/9/b"), vec![0; 24], Keep::Once).unwrap();
        assert_eq!(held_by_the_store(), Some(1024));
        let _ = read(Path::new("/web/9/b"), 100).unwrap();
        assert_eq!(held_by_the_store(), Some(1000), "a file moved out is the reader's to count");
        clear();
        assert_eq!(held_by_the_store(), Some(0));
        use_on_this_thread(false);
    }

    #[test]
    fn a_file_is_admitted_by_the_rule_of_a_read() {
        on();
        assert_eq!(admit(1), Ok(()));
        let cap = crate::file::package_file_cap(&diacraft_vsdx::Limits::default());
        assert_eq!(admit(cap + 1), Err(crate::OpenProblem::FileTooLarge), "past the cap of a package file");
        // A size that is all of the budget is too big alone, unless it is past the cap of a package file, which is said first.
        let budget = diacraft_vsdx::MAX_MEMORY;
        let at_budget = if budget > cap { crate::OpenProblem::FileTooLarge } else { crate::OpenProblem::TooBig };
        assert_eq!(admit(budget), Err(at_budget));
        use_on_this_thread(false);
    }
}
