//! The folders automation may read and write (gaps A2 and A5).
//!
//! `diacraft-cli mcp --automation-read-root <dir> --automation-write-root <dir>`, and `diacraft --control <port>` with the same flags, keep
//! the files an agent's commands reach to the folders the person granted; each flag may be given more than once. [`AutomationRoots`] holds the
//! folders of each kind. **Every file a command names goes through this module** (`crate::file` is the one place the engine meets the file
//! system, and each of its functions asks [`allow`] before it looks at the path): a read of `file.open`, `file.new` with a template file,
//! `stencil.open`, `insert.picture` and `orgchart.picture_set` is a read, and a `file.save`, `file.save_as`, `file.export`, `file.print` (and
//! `file.check_compatibility`, which asks what a save would do) is a write.
//!
//! * **The path is resolved before it is compared.** It is made absolute against the working directory, then its `..` and its links are
//!   followed (`std::fs::canonicalize`: symbolic links, and on Windows junctions and every other reparse point, in the way the system
//!   follows them), and for a file that is not there yet its nearest folder that is there is resolved and the rest must be plain names (a
//!   `..` below a missing folder is refused, **on Windows too, where the system would fold `nope\..` away by the look of the name** (each
//!   `..` is asked of the folder it steps out of first: `parent_steps_leave_folders`), and so is a link that leads nowhere, since a write through
//!   it would make the file where it points). The comparison is of the real path, by whole components (`/work2` is not inside `/work`).
//! * **What is opened is the real path**, not the name the caller gave, so a link that was put in the name's place after the check does not
//!   lead anywhere new; and the handle that was opened is looked at ([`verify_opened`]) before a byte is read from it. **Where the system says
//!   which file a handle is (`/proc/self/fd` on Linux) its answer is final**: a handle it places outside the roots is refused. Where it does
//!   not, the file's identity is asked: the handle must be the file now standing at the checked path, told by the device and the inode on Unix
//!   and by the volume serial number and the file index on Windows (`same_file::Handle`), and never by the size and the modified time, which
//!   another file put in the place can have as well. **A link standing at the path is looked at as itself and never followed**, so one put
//!   there after the check that the path has none is refused (`verify_by_identity`).
//! * **A write is at the folder's real path and the file's own name** ([`allow_entry`]), and **a link standing at the name is refused**
//!   ([`Refusal::Swapped`]): the save judged the name as a new file, or as the drawing's own file (which it reaches through its link, at the file
//!   the link leads to), so a link there when the file is written was put there since, and is never written through.
//! * **Read and write are separate.** A root given for one kind grants nothing of the other, and a kind with no root grants no access:
//!   [`Refusal::NotGranted`]. A root is a folder that exists when it is given, resolved once then.
//! * **A refusal is in words, names the rule and the flag that grants the folder, and never repeats the path** or says what is there
//!   ([`Refusal::words`]): a path outside every root is told as outside whether or not anything is there, so a program cannot use it to
//!   look about the machine.
//! * **Hostile paths cost a bounded walk**: a path of more than [`MOST_PATH_BYTES`] bytes or [`MOST_COMPONENTS`] parts, an empty one and one
//!   with a NUL are refused before the file system is asked anything, and the walk up from a path that is not there is at most two calls
//!   for each part ([`probes_made`] counts them).
//!
//! The roots are in force on a thread inside [`confine`], and in the whole process after [`confine_process`] (the command line's MCP
//! server, which is all agent), except inside [`unconfined`]: what the person at the keyboard does. **With no roots in force nothing is
//! checked**, as before: the programs put roots in force only when the person gave some.
//!
//! **What is not guarded.** A path is checked, then opened or written, and a program that can change the folders in between (swap a folder
//! for a link) could still win that race on a write: the roots keep an agent to the folders it was given, and are no defence against other
//! programs on the computer that already write there. The read side looks at what it opened, which closes the race for reads where the system
//! says which file a handle is (Linux), by its answer. Elsewhere it narrows it to what the file's identity cannot tell: the standard library gives
//! no real path for a handle, so a folder on the way that is swapped for a link and back between the three looks of `verify_by_identity` (two
//! swaps, each timed between two calls) could still lead them astray; and a hard link inside a root to a file outside is the same file, and is no
//! defence either; and the Windows file index is 64 bits, where ReFS names its files with 128, which has not been tried. A write looks at the name
//! just before the file is made, and the two looks are not one step.

pub mod request;

pub use request::{READ_ENV, READ_FLAG, RootRequest, WRITE_ENV, WRITE_FLAG};

use std::cell::{Cell, RefCell};
use std::ffi::OsString;
use std::fmt;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, OnceLock};

/// The most bytes of text a path may have: the longest the systems name (Windows' 32,767 characters). A path of more is refused before the file system is asked anything.
pub const MOST_PATH_BYTES: usize = 32 * 1024;

/// The most parts a path may have (`a/b/c` has three) once it is absolute. The system's own limit on a path is lower on every system but Windows, so no real path
/// is refused for this; it bounds the walk a hostile one costs.
pub const MOST_COMPONENTS: usize = 4096;

/// The most roots of one kind: more is a mistake in the command line, not a list.
pub const MOST_ROOTS: usize = 64;

/// Read access or write access.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Read,
    Write,
}

impl Access {
    fn kind(self) -> &'static str {
        match self {
            Access::Read => "read",
            Access::Write => "write",
        }
    }
}

/// Why a path was refused. One word each, fixed: the path itself is never in them (see [`Refusal::words`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// No root of this kind was given: nothing may be read, or nothing may be written.
    NotGranted(Access),
    /// The path leads, once resolved, outside every root of its kind.
    Outside(Access),
    /// The path is inside a root by its name and cannot be resolved: a link that leads nowhere, a loop of links, a missing folder under a `..` or a folder that may not be looked into.
    Unresolved(Access),
    /// An empty path, one with a NUL, one past [`MOST_PATH_BYTES`] or [`MOST_COMPONENTS`], or (Windows) a device name.
    Unusable,
    /// The file that was opened is not the file that was checked: something was put in the path's place meanwhile.
    Swapped(Access),
}

impl Refusal {
    /// The refusal in words for a person and for an agent, which begin `automation ` and hold no text of the path.
    pub fn words(self) -> &'static str {
        match self {
            Refusal::NotGranted(Access::Read) => {
                "automation filesystem access is not granted: no folder may be read (grant one with --automation-read-root)"
            }
            Refusal::NotGranted(Access::Write) => {
                "automation filesystem access is not granted: no folder may be written (grant one with --automation-write-root)"
            }
            Refusal::Outside(Access::Read) => {
                "automation path rejected: the file is outside the folders granted for reading (--automation-read-root)"
            }
            Refusal::Outside(Access::Write) => {
                "automation path rejected: the file is outside the folders granted for writing (--automation-write-root)"
            }
            Refusal::Unresolved(Access::Read) => {
                "automation path rejected: the path cannot be resolved to a place inside the folders granted for reading (a link that leads nowhere, a loop of links, a missing folder or no permission)"
            }
            Refusal::Unresolved(Access::Write) => {
                "automation path rejected: the path cannot be resolved to a place inside the folders granted for writing (a link that leads nowhere, a loop of links, a missing folder or no permission)"
            }
            Refusal::Unusable => "automation path rejected: the path is empty, too long, has too many parts or is not a name the system can use",
            Refusal::Swapped(Access::Read) => {
                "automation path rejected: the file that was opened is not the file that was checked (a link was put in its place meanwhile), so it was not read"
            }
            Refusal::Swapped(Access::Write) => {
                "automation path rejected: the file that was opened is not the file that was checked (a link was put in its place meanwhile), so it was not written"
            }
        }
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.words())
    }
}

impl std::error::Error for Refusal {}

/// One folder that is granted: the real folder, and its name as it was given, made absolute.
#[derive(Debug)]
struct Root {
    /// The folder made absolute with its links followed: what real paths are compared with.
    resolved: PathBuf,
    /// The folder as given, made absolute and not resolved: a path that cannot be resolved is told apart from one outside by comparing its name with this (and with `resolved`).
    given: PathBuf,
    /// `resolved` without Windows' `\\?\` prefix, for people.
    shown: String,
}

impl Root {
    /// The folder at `path`, which must be there.
    fn open(path: &Path, access: Access) -> Result<Root, String> {
        let words = |why: &str| format!("cannot use the automation {} root `{}`: {why}", access.kind(), crate::shown(&path.to_string_lossy(), 200));
        let resolved = std::fs::canonicalize(path).map_err(|e| words(&e.to_string()))?;
        if !resolved.is_dir() {
            return Err(words("it is not a folder"));
        }
        let given = std::path::absolute(path).map_err(|e| words(&e.to_string()))?;
        let shown = plain(&resolved).to_string_lossy().into_owned();
        Ok(Root { resolved, given, shown })
    }
}

/// The folders automation may read and write; a kind with no root grants no access of its kind.
#[derive(Debug)]
pub struct AutomationRoots {
    read: Vec<Root>,
    write: Vec<Root>,
}

impl AutomationRoots {
    /// The roots `read` and `write`, which must be existing folders (one given twice counts once, and at most [`MOST_ROOTS`] of a kind are taken);
    /// `None` when there are none of either kind: nothing is confined then.
    pub fn new(read: &[&Path], write: &[&Path]) -> Result<Option<Arc<AutomationRoots>>, String> {
        if read.is_empty() && write.is_empty() {
            return Ok(None);
        }
        Ok(Some(Arc::new(AutomationRoots { read: open_all(read, Access::Read)?, write: open_all(write, Access::Write)? })))
    }

    fn of(&self, access: Access) -> &[Root] {
        match access {
            Access::Read => &self.read,
            Access::Write => &self.write,
        }
    }

    /// The real path to use for `access` to `path`, or why it is refused: `path` made absolute with its `..` and links followed, and for a file that is not there yet its folder's real
    /// path and its name. It lies inside a root of the kind, or this is an `Err`.
    pub fn check(&self, path: &Path, access: Access) -> Result<PathBuf, Refusal> {
        let roots = self.of(access);
        if roots.is_empty() {
            return Err(Refusal::NotGranted(access));
        }
        match resolve(path) {
            Ok(real) if roots.iter().any(|r| inside(&real, &r.resolved)) => Ok(real),
            Ok(_) => Err(Refusal::Outside(access)),
            Err(Unresolvable::Unusable) => Err(Refusal::Unusable),
            // Why a path cannot be resolved says what is there, so it is told only for a path whose name puts it inside a root: any other is outside, as it is when it resolves.
            Err(Unresolvable::Failed) if lexically_inside(path, roots) => Err(Refusal::Unresolved(access)),
            Err(Unresolvable::Failed) => Err(Refusal::Outside(access)),
        }
    }

    /// [`AutomationRoots::check`] for the file a write makes or replaces, **with the file's own name kept apart from the real path of its folder**: the answer is the folder's real
    /// path and the name, and **a link standing at the name is [`Refusal::Swapped`]**, so that a write never lands on the file a link leads to, in the name's stead (`check` follows
    /// a link at the end of a path, and a write at what it answers would replace the file the link leads to). A name is judged as `check` judges it first (its words are the
    /// same: outside, unresolved, unusable), and a path with no name (one that ends in `..`) is unusable.
    pub fn check_entry(&self, path: &Path, access: Access) -> Result<PathBuf, Refusal> {
        self.check(path, access)?;
        let Some(name) = path.file_name() else { return Err(Refusal::Unusable) };
        let folder = match path.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => parent,
            _ => Path::new("."),
        };
        let entry = self.check(folder, access)?.join(name);
        probe();
        match std::fs::symlink_metadata(&entry) {
            Ok(meta) if meta.file_type().is_symlink() => Err(Refusal::Swapped(access)),
            // Not there, or there as itself (or not to be looked at: the write says why it cannot be made).
            _ => Ok(entry),
        }
    }

    /// The folders that may be read, as people read them (links followed, no `\\?\`).
    pub fn read_roots(&self) -> Vec<String> {
        self.read.iter().map(|r| r.shown.clone()).collect()
    }

    /// The folders that may be written, as people read them.
    pub fn write_roots(&self) -> Vec<String> {
        self.write.iter().map(|r| r.shown.clone()).collect()
    }

    /// The roots in a line: `read A, B; write C`, and `none` for a kind with no root.
    pub fn describe(&self) -> String {
        let list = |roots: Vec<String>| if roots.is_empty() { "none".to_owned() } else { roots.join(", ") };
        format!("read {}; write {}", list(self.read_roots()), list(self.write_roots()))
    }

    /// The roots as JSON: `{read: [folder, ..], write: [folder, ..]}`.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({ "read": self.read_roots(), "write": self.write_roots() })
    }

    /// Whether what `file` opened is a file inside a root of the kind. **The system's own answer is the last word where it gives one** (`/proc/self/fd` on Linux): a handle it
    /// places outside every root is refused, and no look at the checked path overrules it, since that look is of another path at another moment. Only where it gives none (macOS,
    /// Windows, a Linux with no `/proc`) is the file's identity asked (see [`verify_by_identity`]).
    fn verify(&self, file: &std::fs::File, checked: &Path, access: Access) -> Result<(), Refusal> {
        #[cfg(target_os = "linux")]
        if let Some(real) = linux_real_path(file) {
            return if self.of(access).iter().any(|r| inside(&real, &r.resolved)) { Ok(()) } else { Err(Refusal::Swapped(access)) };
        }
        // No answer from the system: the file at the checked path, which has no link on its way, must be the file that was opened.
        verify_by_identity(file, checked).map_err(|_| Refusal::Swapped(access))
    }
}

/// The roots `paths` name, each opened; at most [`MOST_ROOTS`] given, and one that is named twice is held once.
fn open_all(paths: &[&Path], access: Access) -> Result<Vec<Root>, String> {
    if paths.len() > MOST_ROOTS {
        return Err(format!("at most {MOST_ROOTS} automation {} roots are taken, and {} were given", access.kind(), paths.len()));
    }
    let mut roots: Vec<Root> = Vec::with_capacity(paths.len());
    for path in paths {
        let root = Root::open(path, access)?;
        if !roots.iter().any(|r| r.resolved == root.resolved) {
            roots.push(root);
        }
    }
    Ok(roots)
}

/// Where the roots are in force on a thread ([`confine`], [`unconfined`]).
#[derive(Clone, Debug)]
enum Scope {
    Confined(Arc<AutomationRoots>),
    Unconfined,
}

thread_local! {
    /// This thread's scope; `None`: the process's roots ([`confine_process`]).
    static SCOPE: RefCell<Option<Scope>> = const { RefCell::new(None) };
    /// The calls to the file system `resolve` has made on this thread ([`probes_made`]).
    static PROBES: Cell<u64> = const { Cell::new(0) };
}

/// The roots in force in every thread outside [`confine`] and [`unconfined`].
static PROCESS: OnceLock<Arc<AutomationRoots>> = OnceLock::new();

/// Puts `roots` in force in the whole process, for good. `false` when roots were in force already (they stay as they were).
pub fn confine_process(roots: Arc<AutomationRoots>) -> bool {
    PROCESS.set(roots).is_ok()
}

/// Puts the scope it replaced back when dropped, also when the work panics.
struct Restore(Option<Scope>);

impl Drop for Restore {
    fn drop(&mut self) {
        let previous = self.0.take();
        SCOPE.with(|s| *s.borrow_mut() = previous);
    }
}

fn with_scope<T>(scope: Option<Scope>, f: impl FnOnce() -> T) -> T {
    let _restore = Restore(SCOPE.with(|s| s.replace(scope)));
    f()
}

/// Runs `f` with `roots` in force on this thread (`None`: as things stand, which is the process's roots or none).
pub fn confine<T>(roots: Option<&Arc<AutomationRoots>>, f: impl FnOnce() -> T) -> T {
    match roots {
        Some(r) => with_scope(Some(Scope::Confined(r.clone())), f),
        None => f(),
    }
}

/// Runs `f` with no roots in force on this thread: what the person at the keyboard does, and the window's own work with the paths it holds (the stencils it opened).
pub fn unconfined<T>(f: impl FnOnce() -> T) -> T {
    with_scope(Some(Scope::Unconfined), f)
}

/// This thread's scope, to carry to work it hands to another thread: the window's reading of a file, its export and its print run on threads of their own, and the roots of
/// the request that started them are in force there ([`ScopeHandle::run`]).
#[derive(Clone, Debug, Default)]
pub struct ScopeHandle(Option<Scope>);

impl ScopeHandle {
    /// The scope of the calling thread.
    pub fn capture() -> ScopeHandle {
        ScopeHandle(SCOPE.with(|s| s.borrow().clone()))
    }

    /// Runs `f` in the captured scope.
    pub fn run<T>(self, f: impl FnOnce() -> T) -> T {
        with_scope(self.0, f)
    }

    /// Runs `f` as the work was first run, for work that is repeated later and on another thread (a stencil opened again in a drawing that is read): **in the captured scope
    /// when that holds roots, and unconfined when it holds none**. Work that a request started stays under the request's roots, whatever the thread that repeats it is under,
    /// and work that the person started stays the person's even when it is repeated inside a request (a request that has a drawing read opens the person's stencils again).
    pub fn run_replay<T>(self, f: impl FnOnce() -> T) -> T {
        match self.0 {
            Some(Scope::Confined(_)) => with_scope(self.0, f),
            _ => with_scope(Some(Scope::Unconfined), f),
        }
    }

    /// Runs `f` in the captured scope **unless roots are in force already, which it never loosens**: for a card that is answered in a frame after the one that raised it.
    /// A card a request raised is answered under the request's roots, by whoever answers it; a card the person raised and a request answers is answered under the roots
    /// of the frame that answers.
    pub fn run_beneath<T>(self, f: impl FnOnce() -> T) -> T {
        if current().is_some() { f() } else { with_scope(self.0, f) }
    }
}

/// The roots in force on this thread, if any.
pub fn current() -> Option<Arc<AutomationRoots>> {
    match SCOPE.with(|s| s.borrow().clone()) {
        Some(Scope::Confined(r)) => Some(r),
        Some(Scope::Unconfined) => None,
        None => PROCESS.get().cloned(),
    }
}

/// The path to use for `access` to `path`: the real path, inside a root, when roots are in force; `path` as it is when none are.
pub fn allow(path: &Path, access: Access) -> Result<PathBuf, Refusal> {
    match current() {
        Some(roots) => roots.check(path, access),
        None => Ok(path.to_path_buf()),
    }
}

/// The path to write the file at `path` by: [`allow`] with the file's name kept apart from the real path of its folder, so that **a link standing at the name is refused** and
/// never written through (see [`AutomationRoots::check_entry`]); `path` as it is when no roots are in force.
pub fn allow_entry(path: &Path, access: Access) -> Result<PathBuf, Refusal> {
    match current() {
        Some(roots) => roots.check_entry(path, access),
        None => Ok(path.to_path_buf()),
    }
}

/// Looks at what `file` opened, when roots are in force: `checked` is the real path [`allow`] gave, and the file opened at it must be a file inside a root of the kind. A link put
/// in the path's place between the check and the open (or a folder on its way turned into a link) is [`Refusal::Swapped`]. With no roots in force it asks nothing.
pub fn verify_opened(file: &std::fs::File, checked: &Path, access: Access) -> Result<(), Refusal> {
    match current() {
        Some(roots) => roots.verify(file, checked, access),
        None => Ok(()),
    }
}

/// The check that needs nothing of the system but the file's identity: the file at `checked` (the real path of a file that was there) must be the file that was opened, **by
/// the file's identity** (the device and the inode on Unix, the volume serial number and the file index on Windows: `same_file::Handle`), and `checked` must still be its own
/// real path, with no link on the way. The size and the modified time are no identity (another file of that size and time put in the place has them too) and are not looked
/// at. Used where the system does not say which file a handle is, and never to overrule an answer it gives. A system that tells no identity (neither Unix nor Windows) refuses.
///
/// **The identity is looked at without following a link at the path** ([`is_the_file_at`]), **before and after the look at the path's real path**: a link that stands at the
/// path at either look is refused as itself, and not followed to the file that was opened, so the one swap that the real-path look cannot see (a link put at the path after it)
/// is seen by the look that comes next, and a link there before it by the look that comes first. What is left is a folder on the way swapped for a link and back between the
/// three looks, which needs two swaps, each timed between two calls: the standard library does not give the real path of a handle, as `/proc` does on Linux, and a stronger look
/// than this one needs a call that is not safe to make.
pub(crate) fn verify_by_identity(file: &std::fs::File, checked: &Path) -> Result<(), Refusal> {
    if is_the_file_at(file, checked) && std::fs::canonicalize(checked).is_ok_and(|now| now == checked) && is_the_file_at(file, checked) {
        Ok(())
    } else {
        Err(Refusal::Swapped(Access::Read))
    }
}

/// Whether `file`, which was opened, is the file standing at `checked`, by the identity the system gives a file (an error in asking is no). **A link standing at `checked` is
/// looked at as itself and never followed**: it is no regular file, and not the file it leads to.
#[cfg(unix)]
fn is_the_file_at(file: &std::fs::File, checked: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    // What `same_file::Handle` compares on Unix (the device and the inode), from a look at the path that does not follow a link at its end, and not a second open of it: a path
    // opened for reading follows a link and waits for a writer when a pipe was put in its place (`file.rs` opens without waiting for the same reason).
    matches!((file.metadata(), std::fs::symlink_metadata(checked)), (Ok(a), Ok(b)) if b.file_type().is_file() && a.dev() == b.dev() && a.ino() == b.ino())
}

/// [`is_the_file_at`] on Windows: the volume serial number and the file index, through `same_file::Handle`, of the file the path opens **as itself**.
#[cfg(windows)]
fn is_the_file_at(file: &std::fs::File, checked: &Path) -> bool {
    use std::os::windows::fs::OpenOptionsExt;
    // FILE_FLAG_BACKUP_SEMANTICS lets a folder be opened too (and refused below), and FILE_FLAG_OPEN_REPARSE_POINT opens a symbolic link or a junction standing at the path as
    // itself and does not follow it.
    const BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    let there = std::fs::OpenOptions::new().read(true).custom_flags(BACKUP_SEMANTICS | OPEN_REPARSE_POINT).open(checked);
    let Ok(there) = there else { return false };
    if !there.metadata().is_ok_and(|meta| meta.file_type().is_file()) {
        return false;
    }
    // `Handle::from_file` takes the file it is given, so the opened file gives a duplicate of its handle, which is the same open file. Both handles are held while they are
    // compared, as a file index is only stable while a handle to the file is open.
    let opened = file.try_clone().and_then(same_file::Handle::from_file);
    matches!((opened, same_file::Handle::from_file(there)), (Ok(a), Ok(b)) if a == b)
}

/// [`is_the_file_at`] where there is no identity to ask: no file is shown to be the one that was checked.
#[cfg(not(any(unix, windows)))]
fn is_the_file_at(_file: &std::fs::File, _checked: &Path) -> bool {
    false
}

/// The real path of the file a handle holds, as the kernel gives it (`/proc/self/fd`); `None` where `/proc` is not there.
#[cfg(target_os = "linux")]
fn linux_real_path(file: &std::fs::File) -> Option<PathBuf> {
    use std::os::fd::AsRawFd;
    std::fs::read_link(format!("/proc/self/fd/{}", file.as_raw_fd())).ok()
}

/// How many calls to the file system [`AutomationRoots::check`] has made on this thread, for the tests that hold a hostile path to a bounded walk (a count does not move with
/// the load on the machine, as a time does). `#[doc(hidden)]`: nothing in the engine reads it.
#[doc(hidden)]
#[must_use]
pub fn probes_made() -> u64 {
    PROBES.with(Cell::get)
}

fn probe() {
    PROBES.with(|p| p.set(p.get().saturating_add(1)));
}

/// Why a path could not be resolved.
enum Unresolvable {
    /// Refused before the file system was asked anything.
    Unusable,
    /// The file system could not say where it leads.
    Failed,
}

/// `path` made absolute with its `..` and links followed; for a path that is not there (yet), its nearest folder that is there resolved and the names below it appended.
fn resolve(path: &Path) -> Result<PathBuf, Unresolvable> {
    let text = path.as_os_str().as_encoded_bytes();
    if text.is_empty() || text.len() > MOST_PATH_BYTES || text.contains(&0) || has_too_many_parts(path) {
        return Err(Unresolvable::Unusable);
    }
    // Checked as given: making `...\NUL` absolute gives the device path `\\.\NUL` on Windows.
    if cfg!(windows) && path.components().any(|c| matches!(c, Component::Normal(n) if is_device_name(&n.to_string_lossy()))) {
        return Err(Unresolvable::Unusable);
    }
    // Windows folds `nope\..` away before the file system is asked: `absolute` does, and so does the system when it opens a path. Every `..` is looked at first, so that one below a
    // folder that is not there is refused here as it is where the system fails it itself.
    if cfg!(windows) && !parent_steps_leave_folders(path, &mut is_a_folder) {
        return Err(Unresolvable::Failed);
    }
    let absolute = std::path::absolute(path).map_err(|_| Unresolvable::Failed)?;
    if has_too_many_parts(&absolute) {
        return Err(Unresolvable::Unusable);
    }
    // The names below the nearest folder that is there, last first.
    let mut missing: Vec<OsString> = Vec::new();
    let mut at: &Path = &absolute;
    loop {
        probe();
        match std::fs::canonicalize(at) {
            Ok(mut found) => {
                found.extend(missing.iter().rev());
                return Ok(found);
            }
            Err(_) => {
                probe();
                match std::fs::symlink_metadata(at) {
                    // There, and its real path cannot be had: a link that leads nowhere (a write would make the file where it points), a loop, no permission, a file with a path below it.
                    Ok(_) => return Err(Unresolvable::Failed),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(_) => return Err(Unresolvable::Failed),
                }
            }
        }
        match (at.components().next_back(), at.parent()) {
            (Some(Component::Normal(name)), Some(parent)) => {
                missing.push(name.to_os_string());
                at = parent;
            }
            // A `..` below a folder that is not there (the system would fail it, so it is never trusted to land inside), or down to the root and it is not there either.
            _ => return Err(Unresolvable::Failed),
        }
    }
}

/// Whether `folder` is a folder that is there (a link to one is). One call to the file system.
fn is_a_folder(folder: &Path) -> bool {
    probe();
    std::fs::metadata(folder).is_ok_and(|meta| meta.is_dir())
}

/// Whether every `..` of `path` steps out of a folder that is there, as `is_folder` says of the part of the path before it. A `..` at the start of a relative path, or at the root,
/// steps out of nothing that has to be looked at. A chain that leaves the same folder again and again asks once, and at most one question is asked of each `..`.
///
/// The system fails `nope/..` where `nope` is not a folder (or is not there); Windows takes it as `.` by the look of the name alone, so there it must be asked.
fn parent_steps_leave_folders(path: &Path, is_folder: &mut dyn FnMut(&Path) -> bool) -> bool {
    let mut at = PathBuf::new();
    // The folder last found to be there.
    let mut found: Option<PathBuf> = None;
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(at.components().next_back(), Some(Component::Normal(_))) {
                    if found.as_deref() != Some(at.as_path()) {
                        if !is_folder(&at) {
                            return false;
                        }
                        found = Some(at.clone());
                    }
                    at.pop();
                } else if !at.has_root() {
                    at.push("..");
                }
            }
            other => at.push(other.as_os_str()),
        }
    }
    true
}

/// Whether `path` has more than [`MOST_COMPONENTS`] parts. Counts no further than the limit.
fn has_too_many_parts(path: &Path) -> bool {
    path.components().take(MOST_COMPONENTS + 1).count() > MOST_COMPONENTS
}

/// Whether the name of `path` (made absolute, its `.` and `..` taken by name and not by what is there) lies in one of `roots`, resolved or as given.
fn lexically_inside(path: &Path, roots: &[Root]) -> bool {
    let Ok(absolute) = std::path::absolute(path) else { return false };
    let mut lexical = PathBuf::new();
    for part in absolute.components() {
        match part {
            Component::ParentDir => {
                lexical.pop();
            }
            Component::CurDir => {}
            other => lexical.push(other.as_os_str()),
        }
    }
    roots.iter().any(|r| inside(&lexical, &r.resolved) || inside(&lexical, &r.given))
}

/// Whether `path` (resolved) is `root` or inside it, part by part.
fn inside(path: &Path, root: &Path) -> bool {
    let mut path = path.components();
    root.components().all(|r| path.next().is_some_and(|p| same_component_with(p, r, cfg!(windows))))
}

/// Whether two parts are one. Only a drive prefix is compared without regard to case, and only where `fold_prefix` is set (Windows): folder names are compared as they are,
/// since both paths are real paths by now with the case the file system keeps, and a folder that differs in case alone may be another one (a case-sensitive folder on Windows, a
/// volume on macOS).
fn same_component_with(a: Component, b: Component, fold_prefix: bool) -> bool {
    if a == b {
        return true;
    }
    if !fold_prefix || !matches!((a, b), (Component::Prefix(_), Component::Prefix(_))) {
        return false;
    }
    match (a.as_os_str().to_str(), b.as_os_str().to_str()) {
        (Some(a), Some(b)) => a.to_lowercase() == b.to_lowercase(),
        _ => false,
    }
}

/// A Windows device name (`CON`, `NUL`, `COM1`, also with an extension), which names no file.
fn is_device_name(component: &str) -> bool {
    let stem = component.split('.').next().unwrap_or(component).trim_end_matches(' ').to_ascii_uppercase();
    let numbered = |prefix: &str| stem.strip_prefix(prefix).is_some_and(|n| matches!(n, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9"));
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$" | "CONIN$" | "CONOUT$") || numbered("COM") || numbered("LPT")
}

/// `path` without Windows' `\\?\` prefix (which `canonicalize` adds), for people to read.
fn plain(path: &Path) -> PathBuf {
    if !cfg!(windows) {
        return path.to_path_buf();
    }
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    match text.strip_prefix(r"\\?\") {
        Some(rest) => PathBuf::from(rest),
        None => path.to_path_buf(),
    }
}

#[cfg(test)]
mod tests;
