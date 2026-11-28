//! Reading a file from disk into memory, bounded, and the file a document was opened from.
//!
//! The one place this crate meets the file system, for `file.open` and the CLI's `oracle`. A path is hostile as the file at its end is: a
//! folder, a device, a pipe and a file that is not there are refused before a byte is read, a file larger than the cap is refused from its
//! size and again from the bytes that really came (it may grow while it is read), and the buffer is reserved with `try_reserve`, so a size
//! that cannot be had is an error and not an abort. **The file is opened without waiting** (`O_NONBLOCK` where the system has it) and what
//! was opened is checked again, so a path that is swapped for a pipe between the check and the open is refused and does not block the
//! caller. No message holds the path or the system's own text, which carries it.
//!
//! **With automation roots in force** ([`crate::file_access`]: the control channel and the MCP server started with `--automation-read-root` or `--automation-write-root`)
//! every function here that is given a path asks first whether the roots let it be read or written, and reads and writes **the real path** that answer gives (the name with
//! its `..` and links followed), so that a link put in the name's place afterwards leads nowhere new; a read also looks at the handle it opened. With none in force (what
//! the person at the window does, and the programs started with no roots) nothing changes: the path is used as it is given.
//!
//! **Where there is no file system (the browser) the file is in memory** ([`crate::memfs`]): every function below that meets the disk meets the store instead, with the same
//! answers and the same refusals, and a link, a folder and a device are things a store does not have.

use crate::memory::Hold;
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;

use diacraft_doc::Document;
use diacraft_doc::stamp::Stamp;

use crate::nativefile::NativeState;
use crate::odgfile::OdgState;

/// Why a file was not read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileError {
    /// Nothing is there, or the path is not one a system can name (a NUL in it, say).
    NotFound,
    /// The system would not let the file be read.
    Denied,
    /// A folder, a device, a pipe or anything else that is not a regular file.
    NotAFile,
    /// Larger than the cap, by its size or by what was read.
    TooLarge { limit: u64 },
    /// The memory for it could not be had.
    OutOfMemory,
    /// Anything else the system said while reading.
    Failed,
    /// The automation roots in force do not let this path be read (`file_access`).
    Refused(crate::file_access::Refusal),
}

impl fmt::Display for FileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FileError::NotFound => write!(f, "there is no such file"),
            FileError::Denied => write!(f, "the file cannot be read: permission was refused"),
            FileError::NotAFile => write!(f, "this is not a file (a folder, a device or a pipe)"),
            FileError::TooLarge { limit } => write!(f, "the file is larger than {limit} bytes"),
            FileError::OutOfMemory => write!(f, "there is not enough memory to read the file"),
            FileError::Failed => write!(f, "the file could not be read"),
            FileError::Refused(why) => write!(f, "{why}"),
        }
    }
}

impl std::error::Error for FileError {}

fn classify(error: &std::io::Error) -> FileError {
    match error.kind() {
        std::io::ErrorKind::NotFound | std::io::ErrorKind::InvalidInput => FileError::NotFound,
        std::io::ErrorKind::PermissionDenied => FileError::Denied,
        std::io::ErrorKind::OutOfMemory => FileError::OutOfMemory,
        _ => FileError::Failed,
    }
}

/// Opens `path` for reading without waiting for a writer, where the system can.
fn open_without_waiting(path: &Path) -> std::io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    options.open(path)
}

/// Bytes a package file may hold beyond what its parts may inflate to (`Limits::default().opc.max_package_bytes`): the container's own overhead.
const FILE_SLACK: u64 = 64 * 1024 * 1024;

/// The most bytes a package file (a drawing, a stencil, a template) may be for the reader whose limits are `limits`: what its parts may inflate to
/// and the container's own overhead. `file.open` and `stencil.open` read with this cap, through the one reader below.
pub fn package_file_cap(limits: &diacraft_vsdx::Limits) -> u64 {
    limits.opc.max_package_bytes.saturating_add(FILE_SLACK)
}

/// The bytes of the regular file at `path`, which must be at most `max_bytes`.
///
/// The path is looked at first (`metadata`: a link is followed, so a link to a regular file is read and a link to a folder is not), the file
/// is opened without waiting, and what was opened is looked at again, so that the answer is about the file that is read and not about
/// the one the path named a moment before.
pub fn read_file(path: &Path, max_bytes: u64) -> Result<Vec<u8>, FileError> {
    read_file_stamped(path, max_bytes).map(|(bytes, _)| bytes)
}

/// The size of the regular file at `path`, from a look at the path (a link is followed): the answer before any byte is read, so that a read that must hold the room for a file can refuse one that
/// cannot fit from its size, and a path that names nothing, a folder or a device is refused at once.
pub fn file_len(path: &Path) -> Result<u64, FileError> {
    if crate::memfs::active() {
        return crate::memfs::len(path).ok_or(FileError::NotFound);
    }
    let real = readable(path)?;
    let meta = std::fs::metadata(real).map_err(|e| classify(&e))?;
    if meta.is_file() { Ok(meta.len()) } else { Err(FileError::NotAFile) }
}

/// The path to read `path` by: as it is given when no automation roots are in force, and the real path inside a read root when they are (`file_access`), or why it may not be read.
fn readable(path: &Path) -> Result<PathBuf, FileError> {
    crate::file_access::allow(path, crate::file_access::Access::Read).map_err(FileError::Refused)
}

/// [`read_file`], and the [`FileStamp`] of the file that was read, taken from what was opened.
pub fn read_file_stamped(path: &Path, max_bytes: u64) -> Result<(Vec<u8>, FileStamp), FileError> {
    if crate::memfs::active() {
        return crate::memfs::read(path, max_bytes);
    }
    let real = readable(path)?;
    let meta = std::fs::metadata(&real).map_err(|e| classify(&e))?;
    if !meta.is_file() {
        return Err(FileError::NotAFile);
    }
    read_opened_stamped(&real, max_bytes)
}

/// [`read_file`] without its first look: opens `path` and reads what it opened if that is a regular file. The part of `read_file` that
/// answers for a path that changed after the first look, which a test cannot time and so runs on its own.
#[cfg(test)]
pub(crate) fn read_opened(path: &Path, max_bytes: u64) -> Result<Vec<u8>, FileError> {
    read_opened_stamped(path, max_bytes).map(|(bytes, _)| bytes)
}

fn read_opened_stamped(path: &Path, max_bytes: u64) -> Result<(Vec<u8>, FileStamp), FileError> {
    let file = open_without_waiting(path).map_err(|e| classify(&e))?;
    // Roots in force: what was opened is looked at before a byte of it is read, so a link put in the checked path's place meanwhile is refused (`file_access::verify_opened`).
    crate::file_access::verify_opened(&file, path, crate::file_access::Access::Read).map_err(FileError::Refused)?;
    let meta = file.metadata().map_err(|e| classify(&e))?;
    if !meta.is_file() {
        return Err(FileError::NotAFile);
    }
    if meta.len() > max_bytes {
        return Err(FileError::TooLarge { limit: max_bytes });
    }
    // Taken before the bytes are: a file that changes while it is read then differs from its stamp, and the next save asks.
    let stamp = FileStamp::of(&meta);
    let mut bytes = Vec::new();
    let wanted = usize::try_from(meta.len()).map_err(|_| FileError::TooLarge { limit: max_bytes })?;
    bytes.try_reserve_exact(wanted).map_err(|_| FileError::OutOfMemory)?;
    // One byte past the cap tells a file that grew while it was read.
    file.take(max_bytes.saturating_add(1)).read_to_end(&mut bytes).map_err(|e| classify(&e))?;
    if bytes.len() as u64 > max_bytes {
        return Err(FileError::TooLarge { limit: max_bytes });
    }
    Ok((bytes, stamp))
}

/// What a file looked like when a session read it or wrote it: its size and its modified time. A save compares it with the file as it is on the
/// disk, and a file that differs has been changed by something else since (of the M3 review).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileStamp {
    len: u64,
    /// `None` where the system gives no modified time: the size is all there is to compare.
    modified: Option<SystemTime>,
}

impl FileStamp {
    /// The stamp of a file whose metadata this is.
    pub fn of(meta: &std::fs::Metadata) -> FileStamp {
        FileStamp { len: meta.len(), modified: meta.modified().ok() }
    }

    /// The stamp of a file that has a size and no modified time: one in memory ([`crate::memfs`]), which is compared by its size alone.
    pub(crate) fn sized(len: u64) -> FileStamp {
        FileStamp { len, modified: None }
    }

    /// The stamp of the regular file at `path` now (a link is followed, as a read follows it); `None` when it is not there or not a file.
    pub fn of_path(path: &Path) -> Option<FileStamp> {
        if crate::memfs::active() {
            return crate::memfs::stamp(path);
        }
        std::fs::metadata(path).ok().filter(std::fs::Metadata::is_file).map(|meta| FileStamp::of(&meta))
    }
}

/// Where the link at `path` leads, resolved now; `None` when `path` is not a link (or leads nowhere).
pub fn resolved_link(path: &Path) -> Option<PathBuf> {
    // A file in memory is no link.
    if crate::memfs::active() {
        return None;
    }
    std::fs::symlink_metadata(path).ok().filter(|meta| meta.file_type().is_symlink())?;
    std::fs::canonicalize(path).ok()
}

/// Why a file was not written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveError {
    /// The folder the file is to be in is not there.
    NoFolder,
    /// The system would not let the file or its folder be written.
    Denied,
    /// A folder, a device or a pipe stands where the file is to be.
    NotAFile,
    /// A link stands where the file is to be, and it does not lead to the drawing's own file. A save writes the file it was told to write, and
    /// never the file a link leads to: a path supplied by a program could otherwise replace any file the user can write.
    Link,
    /// A link stands where the file is to be, and what it leads to is not there (or it leads round in a circle).
    DanglingLink,
    /// There is no room left on the disk.
    DiskFull,
    /// A file kept in memory ([`crate::memfs`]) has no room: the memory for it could not be had, or the store holds as much as it may.
    NoRoom,
    /// Anything else the system said.
    Failed,
    /// The automation roots in force do not let this path be written (`file_access`).
    Refused(crate::file_access::Refusal),
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SaveError::NoFolder => write!(f, "the folder does not exist"),
            SaveError::Denied => write!(f, "the file or its folder cannot be written: permission was refused, or the file is read-only"),
            SaveError::NotAFile => write!(f, "a folder, a device or a pipe stands where the file should be"),
            SaveError::Link => {
                write!(f, "a link stands where the file should be, and it does not lead to this drawing's own file; name the file itself")
            }
            SaveError::DanglingLink => write!(f, "a link stands where the file should be, and it leads nowhere"),
            SaveError::DiskFull => write!(f, "the disk is full"),
            SaveError::NoRoom => write!(f, "there is not enough memory to hold the file"),
            SaveError::Failed => write!(f, "the file could not be written"),
            SaveError::Refused(why) => write!(f, "{why}"),
        }
    }
}

impl std::error::Error for SaveError {}

/// The file a save to `path` writes, judged **before anything is written** (a security rule: `file.save_as` is reachable from the control
/// channel and MCP, where a program names the path).
///
/// * A path that names nothing yet is a new file: itself.
/// * A regular file is itself.
/// * **A link is never followed to a file the caller did not name.** The path itself is looked at (`symlink_metadata`), not what it leads to.
///   A link is allowed only when it leads to `own`, the file this drawing was opened from or last saved to (so a drawing opened through a
///   link saves through it, and the link stays a link); the answer is then the file it leads to, with its real name, so that the caller can
///   check that file's extension and the write cannot follow another link swapped in meanwhile. Any other link is [`SaveError::Link`], and a link
///   that leads nowhere is [`SaveError::DanglingLink`].
/// * A folder, a device or a pipe is [`SaveError::NotAFile`].
pub fn save_target(path: &Path, own: Option<&Path>) -> Result<PathBuf, SaveError> {
    // A file in memory is itself, whether it is there or not: no link, folder or device can stand where it is.
    if crate::memfs::active() {
        return Ok(path.to_path_buf());
    }
    // Roots in force: the name is judged first, before anything of what is at it is looked at (a path outside every write root is told as that and no more), and the file the
    // rules below settle on is judged again. The real paths are for the write ([`write_target`]); the answer is the target as it has always been.
    let writable = |at: &Path| crate::file_access::allow(at, crate::file_access::Access::Write).map(drop).map_err(SaveError::Refused);
    writable(path)?;
    let target = save_target_unchecked(path, own)?;
    writable(&target)?;
    Ok(target)
}

/// [`save_target`]'s rules for the file a save writes, with no roots asked.
fn save_target_unchecked(path: &Path, own: Option<&Path>) -> Result<PathBuf, SaveError> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(path.to_path_buf()),
        Err(e) => return Err(classify_save(&e)),
    };
    if meta.file_type().is_symlink() {
        let target = std::fs::canonicalize(path).map_err(|_| SaveError::DanglingLink)?;
        let is_own = own.and_then(|o| std::fs::canonicalize(o).ok()).is_some_and(|o| o == target);
        if !is_own {
            return Err(SaveError::Link);
        }
        return if std::fs::metadata(&target).is_ok_and(|m| m.is_file()) { Ok(target) } else { Err(SaveError::NotAFile) };
    }
    if meta.is_file() { Ok(path.to_path_buf()) } else { Err(SaveError::NotAFile) }
}

fn classify_save(e: &std::io::Error) -> SaveError {
    match e.kind() {
        std::io::ErrorKind::NotFound => SaveError::NoFolder,
        std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::ReadOnlyFilesystem => SaveError::Denied,
        std::io::ErrorKind::StorageFull => SaveError::DiskFull,
        std::io::ErrorKind::IsADirectory | std::io::ErrorKind::NotADirectory => SaveError::NotAFile,
        _ => SaveError::Failed,
    }
}

/// Writes `bytes` to `target` (the answer of [`save_target`]) so that the file is never half written: they go to a temporary file in the same
/// folder, which is flushed and then renamed over `target`. Whatever fails, the file that was at `target` is as it was, and the temporary file is
/// removed. An existing file keeps its permissions. No message holds the path or the system's own words, which carry it.
pub fn write_target(target: &Path, bytes: &[u8]) -> Result<(), SaveError> {
    if crate::memfs::active() {
        return crate::memfs::write(target, bytes);
    }
    // Roots in force: the file is judged again now, after the work of making it, and written at its folder's real path and its own name, so that what is written is the file that was
    // judged. **A link that stands at the name by now is refused** (`Refusal::Swapped`), and never written through: the answer of `save_target` is never a link (the drawing's own file,
    // reached by a link, is the file the link leads to), so a link here was put there since.
    let real = crate::file_access::allow_entry(target, crate::file_access::Access::Write).map_err(SaveError::Refused)?;
    craft_atomic::atomic_write(&real, bytes).map_err(|e| classify_save(&e))
}

/// [`save_target`] and then [`write_target`]: what a save to `path` does.
pub fn save_file(path: &Path, bytes: &[u8], own: Option<&Path>) -> Result<(), SaveError> {
    let target = save_target(path, own)?;
    write_target(&target, bytes)
}

thread_local! {
    /// How many files a save has made in memory in this thread: the count of the writer's runs, which a test holds a save and a dry run to (one each).
    static SAVES_MADE: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Counts a file made in memory by a save or by `file.check_compatibility` (`cmd::file::Made`).
pub(crate) fn note_save_made() {
    SAVES_MADE.with(|made| made.set(made.get().saturating_add(1)));
}

/// How many files a save, or `file.check_compatibility`, has made in memory in the calling thread. Test support, so that the work of a dry run is held to the save's by a count and not a time: the
/// writer runs once for each, and the save does not make its file twice. `#[doc(hidden)]`: nothing in the engine reads it.
#[doc(hidden)]
#[must_use]
pub fn saves_made() -> u64 {
    SAVES_MADE.with(std::cell::Cell::get)
}

/// **The content stamp of a file, made when it is needed** (the design's 4.4): the fingerprint of the model as it was when the drawing was read from a foreign file.
///
/// * **None**: the drawing never came from a foreign file (a new one, one made from a template, a `.diacraft` file that says none, a flat `.fodg`, which Save As does not take).
/// * **Pending**: a named VSDX file or `.odg` package just read (armed the `.odg`: a model writer writes the thumbnail and the generator the file had while the stamp says the
///   drawing is the one it held). **The pass that fingerprints the model is not made at the open** (it was a third of the open's time), **and it is not made at a save of the
///   file either** (a drawing that is only saved as VSDX would pay for a stamp nothing reads: the blocker of the re-review). The pending stamp **keeps the document the drawing
///   was read as** ([`ContentStamp::keep`]: a clone of the `Arc`, which costs no pass and no copy), and the first thing that needs the stamp makes it from that document
///   ([`ContentStamp::resolve`]): a Save As to `.diacraft`, or the question whether the content is unchanged once it is not the document that was read. Every clone of the file sees the answer
///   (the session clones its state for each command, and the pass is made once), and **the kept document is let go of when the stamp is made**. Until then it is memory the session holds
///   beside the drawing, which it counts (`Session::model_held`).
/// * **Known**: a stamp, or none when the content could not be fingerprinted (the pass did not fit beside what the process holds, or the codec's budget refused the model).
#[derive(Clone, Default)]
pub struct ContentStamp(Option<Arc<StampCell>>);

/// What a stamp that is made when it is asked for shares between the clones of its file.
struct StampCell {
    /// The stamp, or `None` for a content that could not be fingerprinted: set once.
    stamp: OnceLock<Option<Stamp>>,
    /// The document the drawing was read as, kept until the stamp is made from it (and then let go of).
    read: Mutex<Option<Arc<Document>>>,
}

impl ContentStamp {
    /// No stamp: the drawing never came from a foreign file.
    pub fn none() -> ContentStamp {
        ContentStamp(None)
    }

    /// A stamp the file says (or none, when it says none).
    pub fn known(stamp: Option<Stamp>) -> ContentStamp {
        match stamp {
            None => ContentStamp(None),
            Some(stamp) => {
                let cell = StampCell { stamp: OnceLock::new(), read: Mutex::new(None) };
                // (A cell that was just made is empty, so this is the first set.)
                let _ = cell.stamp.set(Some(stamp));
                ContentStamp(Some(Arc::new(cell)))
            }
        }
    }

    /// A stamp that is made when it is first asked for, from the document [`ContentStamp::keep`] is given (a stamp that is asked for before it has one is none).
    pub fn pending() -> ContentStamp {
        ContentStamp(Some(Arc::new(StampCell { stamp: OnceLock::new(), read: Mutex::new(None) })))
    }

    /// Keeps `doc`, the document the drawing was read as, for a stamp that is pending: the stamp is made from it, once, when it is asked for. A clone of the `Arc` the session
    /// holds already, so that nothing is copied and nothing is walked. Does nothing for a stamp that is known or none, and when one is kept already.
    pub fn keep(&self, doc: Arc<Document>) {
        let Some(cell) = self.0.as_ref().filter(|cell| cell.stamp.get().is_none()) else { return };
        if let Ok(mut read) = cell.read.lock()
            && read.is_none()
        {
            *read = Some(doc);
        }
    }

    /// The document a pending stamp keeps, as a clone of the `Arc`: `None` once the stamp is made (it let go of the document), and for a stamp that is known or none.
    pub fn read_model(&self) -> Option<Arc<Document>> {
        let cell = self.0.as_ref().filter(|cell| cell.stamp.get().is_none())?;
        cell.read.lock().ok().and_then(|read| read.clone())
    }

    /// Whether the stamp is waiting to be made.
    pub fn is_pending(&self) -> bool {
        self.0.as_ref().is_some_and(|cell| cell.stamp.get().is_none())
    }

    /// The stamp when it is known (`None` while it is pending, and when there is none).
    pub fn value(&self) -> Option<Stamp> {
        self.0.as_ref().and_then(|cell| cell.stamp.get().copied().flatten())
    }

    /// The stamp, **made by `make` from the document the drawing was read as, if it is pending** (once, whoever asks first, and the document is let go of when `make` has had it): `None`
    /// when there is none, when `make` could not make one, and when a pending stamp was kept no document.
    pub fn resolve(&self, make: impl FnOnce(&Document) -> Option<Stamp>) -> Option<Stamp> {
        self.0.as_ref().and_then(|cell| {
            *cell.stamp.get_or_init(|| {
                let read = cell.read.lock().ok().and_then(|mut read| read.take());
                read.and_then(|doc| make(&doc))
            })
        })
    }
}

impl fmt::Debug for ContentStamp {
    /// What state the stamp is in, never the document it keeps.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            None => f.write_str("ContentStamp(none)"),
            Some(cell) => match cell.stamp.get() {
                None => f.write_str("ContentStamp(pending)"),
                Some(stamp) => f.debug_tuple("ContentStamp").field(stamp).finish(),
            },
        }
    }
}

/// The format of a drawing's working file: what a save of the drawing writes unless the person picks another, and what decides which of the two save paths
/// (`cmd/file.rs`, `save_path`) a save takes. `.diacraft` is DiaCraft's own, and the other two are the formats it reads and writes beside it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FileFormat {
    /// DiaCraft's own format, `.diacraft`, `.diacraft-stencil` and `.diacraft-template`.
    Diacraft,
    /// A VSDX package: the six VSDX types.
    Vsdx,
    /// An OpenDocument drawing, `.odg` or `.fodg`.
    Odg,
}

impl FileFormat {
    /// The word `file.status` says: `diacraft`, `vsdx` or `odg`.
    pub const fn word(self) -> &'static str {
        match self {
            FileFormat::Diacraft => "diacraft",
            FileFormat::Vsdx => "vsdx",
            FileFormat::Odg => "odg",
        }
    }
}

/// The file a document was opened from: its path as the client gave it, and its bytes as they were read, shared.
///
/// The reader of a file (`diacraft_vsdx::read`) puts into the model what the model has slots for and counts the rest in its report; the
/// rest is not lost, because it is here. M3's writer patches the original parts and keeps every part it does not edit, and it takes them
/// from these bytes: the session holds the one copy behind an `Arc`, the undo history and every snapshot share it, and no inflated part is
/// held beside it. The cost is the size of the file while it is open, which `file.open` caps at the container's own limit and the
/// overhead of the container (1 GiB and 64 MiB).
#[derive(Clone)]
pub struct OpenFile {
    path: PathBuf,
    bytes: Arc<Vec<u8>>,
    /// How the file looked on the disk when it was read or written, or `None` when that is not known.
    stamp: Option<FileStamp>,
    /// The file the path led to when the drawing was opened or saved, when the path is a link: a save writes that file, though the link may lead
    /// elsewhere by then (of the M3 review). `None` when the path is the file itself.
    target: Option<PathBuf>,
    /// Whether the file has a path of its own: `false` for bytes with no path ([`OpenFile::from_template`]: no command makes such a file now, since a new drawing keeps no file's bytes, NP10),
    /// whose drawing is saved to no file but one it is given.
    named: bool,
    /// What a drawing that came from an `.odg` keeps beside the bytes: the index that maps each shape of the model to its element in them, and the
    /// shapes the saves flattened. `None` for a VSDX file and for a template. Held behind `Arc`s: the session clones its state for every command.
    odg: Option<OdgState>,
    /// What a drawing that came from (or was last saved as) a `.diacraft` file keeps beside the bytes: its kind, and what the next save copies from the package as it was
    /// read or written (`nativefile`). `None` for any other file.
    native: Option<NativeState>,
    /// The stamp of the model's content as it was when the drawing was read from a foreign file (a VSDX one or an `.odg` package), or as a `.diacraft` file says it ([`ContentStamp`]): **pending**
    /// for a VSDX file or an `.odg` until something needs it (the pass that makes it is not made at the open), known for a `.diacraft` file, none for any other.
    content_stamp: ContentStamp,
    /// The extension of the type the file was read as (or was last saved as), by what the package is and not by what the file is called: `vsdx`, `vssx`,
    /// `odg` and so on. `None` for a drawing that has no file of a type yet (a new drawing, one made from a template). What Save As offers and gives a name
    /// typed with no extension (the review: it went by the file's name, and an `.odg` called `a.vsdx` was offered the VSDX kinds, which the
    /// engine refuses for it).
    extension: Option<&'static str>,
    /// What the model of the drawing took of the memory when the file was read (`Stats::memory_bytes` of the reader, and for an `.odg` the read's own estimate of the model):
    /// what a read of another file must leave room for beside the bytes ([`OpenFile::held_bytes`]). What the drawing has grown by since is not in it.
    model_bytes: u64,
    /// The file's bytes and the model, held in the process's ledger for as long as the file is the drawing's (shared by every clone of the file: the session clones its state for each
    /// command, and the bytes are one): [`crate::memory::Hold`].
    hold: Arc<Hold>,
    /// What the Save As that wrote this VSDX file from the model, or the saves into it since, were asked to leave out (`drop_macros`, `drop_objects`): the file lacks it, the drawing keeps it, and every
    /// save into the file leaves it out again, as the Save As did. Nothing for a file read from the disk, and for a file of another format (an `.odg` has its own, `OdgState`).
    asked: crate::across::Options,
    /// The files of the objects that the save which wrote this VSDX file from the model wrote as embeddings, by the name the drawing has them (`diacraft_vsdx::Written::natively`): the file
    /// holds each object once, as an embedding, and the drawing keeps the part the embedding was written from. A save into the file is given the drawing without them (`across::natively`).
    natively: Arc<[Arc<str>]>,
    /// Whether this session wrote the file (a save into it, or the Save As that made it), and did not read it from the disk: a Save of the drawing with no edit since is then no edit
    /// and writes the bytes the last save wrote, the bookkeeping that save wrote included (`diacraft_vsdx::WriteOptions::resave_unedited`).
    written_here: bool,
}

impl OpenFile {
    /// The file at `path` whose bytes are `bytes`.
    pub fn new(path: &Path, bytes: Vec<u8>) -> OpenFile {
        let hold = Arc::new(Hold::new(u64::try_from(bytes.len()).unwrap_or(u64::MAX)));
        OpenFile {
            path: path.to_path_buf(),
            bytes: Arc::new(bytes),
            stamp: None,
            target: None,
            named: true,
            odg: None,
            native: None,
            content_stamp: ContentStamp::none(),
            extension: None,
            model_bytes: 0,
            hold,
            asked: crate::across::Options::default(),
            natively: Arc::default(),
            written_here: false,
        }
    }

    /// This file, whose model took `bytes` of memory when it was read.
    #[must_use]
    pub fn with_model_bytes(mut self, bytes: u64) -> OpenFile {
        self.model_bytes = bytes;
        self.hold.set(u64::try_from(self.bytes.len()).unwrap_or(u64::MAX).saturating_add(bytes));
        self
    }

    /// What the model took of the memory when the file was read.
    pub fn model_bytes(&self) -> u64 {
        self.model_bytes
    }

    /// What the session holds for this file while it is open: the file's bytes and the model made from them.
    pub fn held_bytes(&self) -> u64 {
        self.hold.bytes()
    }

    /// The bytes of a package that a drawing with no file yet stands on: they have no path, and `path()` is empty. **No command makes one now**: a new drawing, from a
    /// template or not, keeps no file's bytes (NP10), and what it was made from is its model. It remains for the `.odg` drawings' tests of a first save.
    pub fn from_template(bytes: Vec<u8>) -> OpenFile {
        let hold = Arc::new(Hold::new(u64::try_from(bytes.len()).unwrap_or(u64::MAX)));
        OpenFile {
            path: PathBuf::new(),
            bytes: Arc::new(bytes),
            stamp: None,
            target: None,
            named: false,
            odg: None,
            native: None,
            content_stamp: ContentStamp::none(),
            extension: None,
            model_bytes: 0,
            hold,
            asked: crate::across::Options::default(),
            natively: Arc::default(),
            written_here: false,
        }
    }

    /// This file, written without what `asked` leaves out (see [`OpenFile::asked`]).
    #[must_use]
    pub(crate) fn with_asked(mut self, asked: crate::across::Options) -> OpenFile {
        self.asked = asked;
        self
    }

    /// What the file was written without, at the choice of the Save As that made it: every save into it leaves that out too.
    pub(crate) fn asked(&self) -> crate::across::Options {
        self.asked
    }

    /// This file, whose objects the names given were written as embeddings from (see [`OpenFile::natively`]).
    #[must_use]
    pub(crate) fn with_natively(mut self, names: Arc<[Arc<str>]>) -> OpenFile {
        self.natively = names;
        self
    }

    /// The parts of the drawing that the file holds as embeddings of objects and not as parts that ride: empty for a file read from the disk, which is what it reads as.
    pub(crate) fn natively(&self) -> &Arc<[Arc<str>]> {
        &self.natively
    }

    /// This file, which this session wrote (see [`OpenFile::written_here`]).
    #[must_use]
    pub(crate) fn with_written_here(mut self) -> OpenFile {
        self.written_here = true;
        self
    }

    /// Whether this session wrote the file, rather than read it from the disk: `false` for a file opened, which its first Save may give DiaCraft's name.
    pub(crate) fn written_here(&self) -> bool {
        self.written_here
    }

    /// This file, an `.odg` whose shapes are where `state` says they are in its bytes.
    #[must_use]
    pub fn with_odg(mut self, state: OdgState) -> OpenFile {
        self.odg = Some(state);
        self
    }

    /// What the drawing keeps of the `.odg` it came from, or `None` when it is not one.
    pub fn odg(&self) -> Option<&OdgState> {
        self.odg.as_ref()
    }

    /// This file, a `.diacraft` package whose kind and copied-from state are `state`.
    #[must_use]
    pub fn with_native(mut self, state: NativeState) -> OpenFile {
        self.native = Some(state);
        self
    }

    /// What a drawing keeps of the `.diacraft` file it came from or was last saved as, or `None` when it is not one.
    pub fn native(&self) -> Option<&NativeState> {
        self.native.as_ref()
    }

    /// This file, with the content stamp of its model as the file says it (a `.diacraft` file's `state.xml`): known, or none when the file has none ([`ContentStamp`]).
    #[must_use]
    pub fn with_content_stamp(mut self, stamp: Option<Stamp>) -> OpenFile {
        self.content_stamp = ContentStamp::known(stamp);
        self
    }

    /// This file, a VSDX file read as a foreign file, whose content stamp is **pending**: the fingerprint of the model as it was read is made when something needs it, from
    /// the document the drawing was read as (which the session gives the stamp to keep when it opens the file, [`ContentStamp::keep`]), and neither at the open nor at a save.
    #[must_use]
    pub fn with_pending_content_stamp(mut self) -> OpenFile {
        self.content_stamp = ContentStamp::pending();
        self
    }

    /// This file, with the content stamp of the file it replaces (a save carries it: the stamp is the drawing's, not the file's).
    #[must_use]
    pub fn with_content_stamp_cell(mut self, stamp: ContentStamp) -> OpenFile {
        self.content_stamp = stamp;
        self
    }

    /// The content stamp of the file, as it stands: pending, known or none. The session resolves a pending one (`Session::content_stamp`).
    pub fn content_stamp_cell(&self) -> &ContentStamp {
        &self.content_stamp
    }

    /// The stamp of the drawing's content **as it was when it was read from a foreign file** (a VSDX one or an `.odg`), kept through every save (4.4: a model writer writes the file's own
    /// bookkeeping while the content's stamp is this one), or as a `.diacraft` file says it; **`None` while it is pending**, and for a drawing that never came from a foreign file and a
    /// file whose model could not be fingerprinted.
    pub fn content_stamp(&self) -> Option<Stamp> {
        self.content_stamp.value()
    }

    /// The format of the file: `.diacraft` when it holds the state of one, `.odg` for an OpenDocument drawing and VSDX for a file of one that has a path
    /// of its own. **`None` for bytes with no path** ([`OpenFile::from_template`]): the drawing has no file yet, whatever the bytes are.
    pub fn format(&self) -> Option<FileFormat> {
        if self.native.is_some() {
            Some(FileFormat::Diacraft)
        } else if self.odg.is_some() {
            Some(FileFormat::Odg)
        } else if self.named {
            Some(FileFormat::Vsdx)
        } else {
            None
        }
    }

    /// Whether the drawing is a stencil: a `.diacraft-stencil`, or a VSDX stencil type.
    pub fn is_stencil(&self) -> bool {
        match &self.native {
            Some(native) => native.kind() == diacraft_native::Kind::Stencil,
            None => self.extension.is_some_and(|e| e == "vssx" || e == "vssm"),
        }
    }

    /// This file, read as the type whose extension is `extension` (see [`OpenFile::extension`]).
    #[must_use]
    pub fn with_extension(mut self, extension: Option<&'static str>) -> OpenFile {
        self.extension = extension;
        self
    }

    /// The extension of the type the drawing was read as or last saved as: `odg` for a drawing that came from an `.odg`, else the VSDX type's. `None` when
    /// the drawing has no file of a type (the window gives a new drawing `.diacraft`, its default).
    pub fn extension(&self) -> Option<&'static str> {
        if let Some(native) = &self.native {
            Some(native.extension())
        } else if self.odg.is_some() {
            // An OpenDocument file is `odg` unless it was read as, or saved as, a drawing template (`otg`).
            self.extension.filter(|extension| *extension == "otg").or(Some("odg"))
        } else {
            self.extension
        }
    }

    /// Whether the drawing came from a **flat** OpenDocument file (`.fodg`: one XML document), told from the file's own bytes as `file.open` told
    /// it, so that what the file is called decides nothing. A drawing opened from one is read like any other and is saved **by Save alone, back as a flat file** (plan
    ///): not by Save As, and not as an `.odg` or another type (turning a flat document into a package is not written yet).
    pub fn is_flat(&self) -> bool {
        self.odg.is_some() && diacraft_odg::Form::of(&self.bytes) == diacraft_odg::Form::Flat
    }

    /// Whether the file has a path of its own. `false` only for [`OpenFile::from_template`]: the drawing it holds the bytes of has no file yet.
    pub fn is_named(&self) -> bool {
        self.named
    }

    /// This file, as it looked on the disk when it was read or written.
    #[must_use]
    pub fn with_stamp(mut self, stamp: Option<FileStamp>) -> OpenFile {
        self.stamp = stamp;
        self
    }

    /// This file, with the file its path led to when it was read or written (the path is a link).
    #[must_use]
    pub fn with_target(mut self, target: Option<PathBuf>) -> OpenFile {
        self.target = target;
        self
    }

    /// The path the client opened it by.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The file a save writes: the file the path led to when the drawing was opened or last saved if the path is a link, else the path.
    pub fn target(&self) -> &Path {
        self.target.as_deref().unwrap_or(&self.path)
    }

    /// How the file looked on the disk when it was read or written.
    pub fn stamp(&self) -> Option<FileStamp> {
        self.stamp
    }

    /// The bytes of the file as they were read.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The bytes behind their `Arc`, for a holder that outlives the session's borrow.
    pub fn shared(&self) -> Arc<Vec<u8>> {
        Arc::clone(&self.bytes)
    }
}

impl fmt::Debug for OpenFile {
    /// The path and the size, not the bytes.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenFile").field("path", &self.path).field("bytes", &self.bytes.len()).finish()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use std::path::PathBuf;
    // Only the tests of a named pipe wait on a channel, and they are Unix's.
    #[cfg(unix)]
    use std::time::Duration;

    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("diacraft-engine-file-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    /// The part of `read_file` that answers for a path swapped for a pipe between the look at it and the open: a pipe opened without waiting is
    /// looked at again and refused, and the reader does not block on a writer that never comes.
    #[cfg(unix)]
    #[test]
    fn a_pipe_that_stands_where_the_file_was_is_refused_after_the_open_and_does_not_block() {
        let fifo = scratch("swapped");
        let _ = std::fs::remove_file(&fifo);
        if !std::process::Command::new("mkfifo").arg(&fifo).status().is_ok_and(|s| s.success()) {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let path = fifo.clone();
        std::thread::spawn(move || {
            let _ = tx.send(read_opened(&path, 100));
        });
        let answer = rx.recv_timeout(Duration::from_secs(10)).expect("the open of a pipe blocked: it is not opened without waiting");
        assert_eq!(answer, Err(FileError::NotAFile));
        let _ = std::fs::remove_file(&fifo);
    }

    /// `file_len` is the look at the path that comes before a read takes its room: a regular file gives its size, a link is followed, and a path that names nothing, a folder
    /// and a pipe are refused at once, **without opening anything**, so that the window's open thread does not wait on a pipe's writer while it asks how large the file is.
    #[test]
    fn the_size_of_a_file_is_looked_at_without_opening_it_and_a_path_that_is_no_file_is_refused_at_once() {
        let file = scratch("sized.bin");
        std::fs::write(&file, [1u8; 37]).unwrap();
        assert_eq!(file_len(&file), Ok(37));
        assert_eq!(file_len(&scratch("nothing-at-all")), Err(FileError::NotFound));
        assert_eq!(file_len(file.parent().unwrap()), Err(FileError::NotAFile), "a folder");
        #[cfg(unix)]
        {
            let link = scratch("sized.link");
            let _ = std::fs::remove_file(&link);
            std::os::unix::fs::symlink(&file, &link).unwrap();
            assert_eq!(file_len(&link), Ok(37), "a link to a regular file is the file");
            let _ = std::fs::remove_file(&link);
            let fifo = scratch("sized.fifo");
            let _ = std::fs::remove_file(&fifo);
            if std::process::Command::new("mkfifo").arg(&fifo).status().is_ok_and(|s| s.success()) {
                let (tx, rx) = std::sync::mpsc::channel();
                let path = fifo.clone();
                std::thread::spawn(move || {
                    let _ = tx.send(file_len(&path));
                });
                let answer = rx.recv_timeout(Duration::from_secs(10)).expect("the look at a pipe blocked");
                assert_eq!(answer, Err(FileError::NotAFile));
                let _ = std::fs::remove_file(&fifo);
            }
        }
        let _ = std::fs::remove_file(&file);
    }

    #[test]
    fn what_is_opened_is_what_is_judged_and_a_regular_file_reads_whatever_the_flag() {
        let file = scratch("regular.bin");
        std::fs::write(&file, [5u8; 20]).unwrap();
        assert_eq!(read_opened(&file, 20).unwrap(), [5u8; 20]);
        assert_eq!(read_opened(&file, 19), Err(FileError::TooLarge { limit: 19 }));
        assert_eq!(read_opened(&scratch("nothing-here"), 20), Err(FileError::NotFound));
        let _ = std::fs::remove_file(&file);
    }

    /// The read under roots (`file_access`): a file inside a read root reads, one outside does not, and **a link put where the checked file was, after the check**,
    /// is refused when the handle that was opened is looked at: the part of `read_file` a test cannot time, so it runs `read_opened` on the path the check gave.
    #[cfg(unix)]
    #[test]
    fn a_file_swapped_for_a_link_after_the_check_is_not_read_under_roots() {
        use crate::file_access::{Access, AutomationRoots, Refusal, allow, confine};
        let base = scratch("roots-swap");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("in")).unwrap();
        std::fs::create_dir_all(base.join("outside")).unwrap();
        std::fs::write(base.join("in/a.bin"), b"inside").unwrap();
        std::fs::write(base.join("outside/secret.bin"), b"SECRET").unwrap();
        let roots = AutomationRoots::new(&[&base.join("in")], &[]).unwrap().unwrap();
        // The reads: inside, and outside.
        assert_eq!(confine(Some(&roots), || read_file(&base.join("in/a.bin"), 100)).unwrap(), b"inside");
        assert!(matches!(
            confine(Some(&roots), || read_file(&base.join("outside/secret.bin"), 100)),
            Err(FileError::Refused(Refusal::Outside(Access::Read)))
        ));
        assert!(matches!(confine(Some(&roots), || file_len(&base.join("outside/secret.bin"))), Err(FileError::Refused(_))));
        // The swap: checked as the file inside, then a link to the file outside put in its place, then opened.
        confine(Some(&roots), || {
            let checked = allow(&base.join("in/a.bin"), Access::Read).unwrap();
            std::fs::remove_file(&checked).unwrap();
            std::os::unix::fs::symlink(base.join("outside/secret.bin"), &checked).unwrap();
            assert_eq!(read_opened(&checked, 100), Err(FileError::Refused(Refusal::Swapped(Access::Read))), "the secret was not read");
        });
        assert_eq!(
            read_opened(&base.join("in/a.bin"), 100).unwrap(),
            b"SECRET",
            "with no roots in force nothing is looked at: the check is the roots'"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    /// A save under roots writes the file that was judged, and a name that leads out is refused after the file is made too: `write_target` judges again.
    #[cfg(unix)]
    #[test]
    fn a_save_under_roots_is_judged_before_and_after_the_file_is_made() {
        use crate::file_access::{Access, AutomationRoots, Refusal, confine};
        let base = scratch("roots-save");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("out")).unwrap();
        std::fs::create_dir_all(base.join("outside")).unwrap();
        let roots = AutomationRoots::new(&[], &[&base.join("out")]).unwrap().unwrap();
        let (inside, outside) = (base.join("out/a.bin"), base.join("outside/b.bin"));
        confine(Some(&roots), || save_file(&inside, b"one", None)).unwrap();
        assert_eq!(std::fs::read(&inside).unwrap(), b"one");
        assert_eq!(confine(Some(&roots), || save_file(&outside, b"two", None)), Err(SaveError::Refused(Refusal::Outside(Access::Write))));
        assert!(!outside.exists(), "nothing was made outside");
        // The target was judged inside, and a link was put in its folder's place before the write: the write judges it again.
        let target = confine(Some(&roots), || save_target(&base.join("out/sub/c.bin"), None)).unwrap();
        std::fs::create_dir_all(base.join("out/sub")).unwrap();
        std::fs::remove_dir(base.join("out/sub")).unwrap();
        std::os::unix::fs::symlink(base.join("outside"), base.join("out/sub")).unwrap();
        assert_eq!(confine(Some(&roots), || write_target(&target, b"three")), Err(SaveError::Refused(Refusal::Outside(Access::Write))));
        assert!(!base.join("outside/c.bin").exists(), "nothing was made where the link leads");
        let _ = std::fs::remove_dir_all(&base);
    }

    /// The name was free when the save was judged, and a link stands at it by the time the file (made in memory meanwhile) is written: under roots the write goes to the folder's
    /// real place and the name, and **a link at the name is refused**, so the file the link leads to (inside the roots too) is not replaced in the name's stead.
    #[cfg(unix)]
    #[test]
    fn a_link_put_at_the_name_after_the_save_was_judged_is_not_written_through_under_roots() {
        use crate::file_access::{Access, AutomationRoots, Refusal, confine};
        let base = scratch("roots-save-link");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("out")).unwrap();
        std::fs::write(base.join("out/other.bin"), b"keep").unwrap();
        let roots = AutomationRoots::new(&[], &[&base.join("out")]).unwrap().unwrap();
        let target = confine(Some(&roots), || save_target(&base.join("out/new.bin"), None)).unwrap();
        assert_eq!(target, base.join("out/new.bin"), "judged as a new file");
        std::os::unix::fs::symlink(base.join("out/other.bin"), &target).unwrap();
        assert_eq!(
            confine(Some(&roots), || write_target(&target, b"new")),
            Err(SaveError::Refused(Refusal::Swapped(Access::Write))),
            "a link that stands at the name when the file is written"
        );
        assert_eq!(std::fs::read(base.join("out/other.bin")).unwrap(), b"keep", "the file the link leads to was not replaced");
        assert!(std::fs::symlink_metadata(&target).unwrap().file_type().is_symlink(), "and the link is as it was");
        // A name with a regular file at it, or with nothing, is written as ever.
        let plain = base.join("out/plain.bin");
        confine(Some(&roots), || write_target(&plain, b"one")).unwrap();
        confine(Some(&roots), || write_target(&plain, b"two")).unwrap();
        assert_eq!(std::fs::read(&plain).unwrap(), b"two");
        let _ = std::fs::remove_dir_all(&base);
    }

    /// The exception stays: a drawing opened through a link is saved through it, under roots too. The target is the file the link leads to, the link stays a link, and any other
    /// link inside the roots is still refused when the save is judged.
    #[cfg(unix)]
    #[test]
    fn a_drawing_opened_through_a_link_is_still_saved_through_it_under_roots() {
        use crate::file_access::{AutomationRoots, confine};
        let base = scratch("roots-save-own");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("out")).unwrap();
        std::fs::write(base.join("out/real.bin"), b"one").unwrap();
        std::fs::write(base.join("out/other.bin"), b"keep").unwrap();
        std::os::unix::fs::symlink(base.join("out/real.bin"), base.join("out/link.bin")).unwrap();
        std::os::unix::fs::symlink(base.join("out/other.bin"), base.join("out/stray.bin")).unwrap();
        let roots = AutomationRoots::new(&[], &[&base.join("out")]).unwrap().unwrap();
        let own = std::fs::canonicalize(base.join("out/real.bin")).unwrap();
        confine(Some(&roots), || save_file(&base.join("out/link.bin"), b"two", Some(&own))).unwrap();
        assert_eq!(std::fs::read(base.join("out/real.bin")).unwrap(), b"two", "the drawing's own file was written");
        assert!(std::fs::symlink_metadata(base.join("out/link.bin")).unwrap().file_type().is_symlink(), "and the link is still a link");
        assert_eq!(confine(Some(&roots), || save_file(&base.join("out/stray.bin"), b"no", Some(&own))), Err(SaveError::Link));
        assert_eq!(std::fs::read(base.join("out/other.bin")).unwrap(), b"keep");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn an_open_file_holds_its_bytes_and_the_model_it_was_read_as_and_its_clones_hold_the_one_figure() {
        // (The process's total is shared with the tests that run in this binary at the same time: the figure of the file's own hold is what is exact; the total is the engine's
        // test of `vsdx_memory.rs`, in a process of its own.)
        let open = OpenFile::new(std::path::Path::new("/x/y.vsdx"), vec![0u8; 12]);
        assert_eq!((open.held_bytes(), open.model_bytes()), (12, 0), "the bytes alone, as `new` leaves it");
        let template = OpenFile::from_template(vec![0u8; 7]);
        assert_eq!(template.held_bytes(), 7);
        let open = open.with_model_bytes(1_000);
        assert_eq!((open.held_bytes(), open.model_bytes()), (1_012, 1_000), "the bytes and the model");
        let clone = open.clone();
        drop(open);
        assert_eq!(clone.held_bytes(), 1_012, "a clone of the file holds the one figure, and the first's drop leaves it");
        assert_eq!(clone.with_model_bytes(0).held_bytes(), 12, "a model of nothing is the bytes alone");
    }

    #[test]
    fn an_open_file_shows_its_size_and_not_its_bytes() {
        let open = OpenFile::new(std::path::Path::new("/x/y.vsdx"), vec![0u8; 12]);
        let shown = format!("{open:?}");
        assert!(shown.contains("12") && !shown.contains("0, 0"), "{shown}");
        let copy = open.clone();
        assert!(Arc::ptr_eq(&open.shared(), &copy.shared()), "a clone shares the bytes");
    }

    /// The smallest flat drawing the reader takes: the root says it is a drawing, and the body holds a drawing with one page.
    const SMALL_FLAT: &str = concat!(
        r#"<office:document xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" "#,
        r#"office:mimetype="application/vnd.oasis.opendocument.graphics" office:version="1.3">"#,
        r#"<office:body><office:drawing><draw:page draw:name="p"/></office:drawing></office:body></office:document>"#
    );

    /// A drawing came from a flat file when its file is XML and the drawing was read as an OpenDocument one: the bytes decide, and not the name; neither does a file of
    /// another kind whose bytes are not a package (no VSDX file is XML), nor a package of an `.odg` called `.fodg`.
    #[test]
    fn a_drawing_is_from_a_flat_file_by_the_bytes_of_its_file_and_not_by_its_name() {
        let (_, report) = diacraft_odg::read(SMALL_FLAT.as_bytes(), &diacraft_odg::Limits::default()).unwrap();
        let state = crate::odgfile::OdgState::new(report.index);
        for name in ["/x/a.fodg", "/x/a.odg", "/x/a"] {
            let flat = OpenFile::new(Path::new(name), SMALL_FLAT.as_bytes().to_vec()).with_odg(state.clone());
            assert!(flat.is_flat(), "{name}");
            // Not an OpenDocument drawing read by the odg reader: not flat, whatever its bytes are.
            assert!(!OpenFile::new(Path::new(name), SMALL_FLAT.as_bytes().to_vec()).is_flat(), "{name}: no odg state");
        }
        // The same state over the bytes of a package, or of nothing: not flat.
        for bytes in [&b"PK\x03\x04 a package"[..], &b""[..], &b"no XML here"[..]] {
            let file = OpenFile::new(Path::new("/x/a.fodg"), bytes.to_vec()).with_odg(state.clone());
            assert!(!file.is_flat(), "{bytes:?}");
        }
    }

    /// The content stamp that is pending keeps the document the drawing was read as, makes the stamp from it once, and lets go of it: the clones of a file share the cell.
    #[test]
    fn a_pending_stamp_keeps_the_document_makes_the_stamp_from_it_once_and_lets_go_of_it() {
        let doc = Arc::new(Document::new_blank());
        let pending = ContentStamp::pending();
        let clone = pending.clone();
        assert!(pending.is_pending() && pending.read_model().is_none(), "nothing is kept until the session gives the document");
        pending.keep(Arc::clone(&doc));
        pending.keep(Arc::new(Document::new_blank()));
        assert!(Arc::ptr_eq(&clone.read_model().expect("kept"), &doc), "the first document kept is kept, and a clone sees it");
        assert_eq!(Arc::strong_count(&doc), 2, "the document and the one the cell keeps");
        let mut made = 0;
        let stamp = Stamp(7);
        assert_eq!(
            pending.resolve(|read| {
                made += 1;
                assert!(std::ptr::eq(read, &*doc), "made from the document that was kept");
                Some(stamp)
            }),
            Some(stamp)
        );
        assert_eq!(made, 1);
        assert!(!pending.is_pending() && !clone.is_pending(), "made for every clone");
        assert!(pending.read_model().is_none(), "the document is let go of");
        assert_eq!(Arc::strong_count(&doc), 1, "and nothing holds it any more");
        assert_eq!(clone.resolve(|_| unreachable_stamp()), Some(stamp), "asked again: the answer, and no second pass");
        assert_eq!((pending.value(), clone.value()), (Some(stamp), Some(stamp)));
    }

    fn unreachable_stamp() -> Option<Stamp> {
        panic!("a stamp that is made is not made again")
    }

    /// A stamp pending that was given no document is none for good, one that cannot be made is none and lets go of the document, and a stamp that is known or none keeps nothing.
    #[test]
    fn a_pending_stamp_with_no_document_or_that_cannot_be_made_is_none_and_a_known_one_keeps_nothing() {
        let bare = ContentStamp::pending();
        assert_eq!(bare.resolve(|_| unreachable_stamp()), None);
        assert!(!bare.is_pending());
        let doc = Arc::new(Document::new_blank());
        let failing = ContentStamp::pending();
        failing.keep(Arc::clone(&doc));
        assert_eq!(failing.resolve(|_| None), None);
        assert!(!failing.is_pending() && failing.read_model().is_none());
        assert_eq!(Arc::strong_count(&doc), 1, "the document is let go of when the pass could not be made");
        let known = ContentStamp::known(Some(Stamp(9)));
        known.keep(Arc::clone(&doc));
        assert!(known.read_model().is_none() && !known.is_pending());
        assert_eq!(Arc::strong_count(&doc), 1, "a stamp that is known keeps nothing");
        let none = ContentStamp::none();
        none.keep(Arc::clone(&doc));
        assert!(none.read_model().is_none() && !none.is_pending() && none.value().is_none());
        assert_eq!(
            format!("{:?} {:?} {:?}", none, ContentStamp::pending(), known),
            "ContentStamp(none) ContentStamp(pending) ContentStamp(Some(Stamp(9)))"
        );
    }
}
