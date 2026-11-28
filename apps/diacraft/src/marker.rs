//! What a start remembers about graphics tests that never returned (public issue #17).
//!
//! A graphics driver that crashes the process, or blocks inside a call, while [`graphics::probe`](crate::graphics::probe) tests an adapter ends the start with nothing said, and
//! the next start tests the same adapter and ends the same way: the report was a Windows 10 laptop whose log stopped after the list of adapters, with Intel UHD Graphics through
//! Vulkan first in the order. The tries of [`fallback`](crate::fallback) only help when a try *returns* an error. So the start remembers across starts:
//!
//! - **A marker** ([`Store::begin`]): before an adapter is tested a small file, `graphics-testing.<process id>.txt`, is written in the settings folder (`%AppData%\diacraft` on
//!   Windows, next to `logs`) and flushed to the disk. It names the adapter (its [`Key`]: name and back end) and DiaCraft's version, and is removed when the test returns
//!   ([`Marker::done`]), pass or fail. It is **locked** while the test runs, so a start that finds a marker can tell whose it is: one it can lock belongs to a start that is gone
//!   (the process died, the driver hung and the person ended it, the machine went down); one it cannot lock belongs to a start that is still running, as when several
//!   drawings are opened from the file manager at once, and is not blamed on anything.
//! - **The left-out list** (`graphics-left-out.txt`): at the next start ([`Store::open`]) the adapter a marker of a gone start names is added to the list, with a line in the
//!   log, and every try of the start leaves out the adapters of the list ([`Memory`](crate::graphics::Memory)). The list also takes an adapter whose test timed out in this
//!   start ([`Store::leave_out`]). It holds at most [`MOST_LEFT_OUT`] adapters, lasts until DiaCraft's version changes (a newer build may have mended the driver's trouble) or the
//!   person starts `diacraft --reset-graphics` ([`Store::reset`]), and is never trusted: a file that is too long, not text, from another version or not ours is ignored with a
//!   line in the log, and a folder that cannot be written is a line in the log too. **None of this can stop a start.**
//!
//! A run with `DIACRAFT_NO_PREFS` (or no settings folder) keeps nothing on the disk: [`Store::default`] reads and writes no file, and what a start leaves out lives in its
//! [`Memory`](crate::graphics::Memory) alone.
//!
//! The files are text: [`HEADER`], a `version` line and one `testing <back end> <name>` (a marker) or `left-out <back end> <name>` line for each adapter. The reading and writing of that
//! text ([`format_file`], [`parse_file`]) is plain and tested without a disk.

use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use eframe::wgpu::Backend;

use crate::graphics::{Key, key_of};

/// The first line of both files.
pub const HEADER: &str = "diacraft-graphics 1";

/// The file that keeps the adapters left out, in the settings folder.
pub const LEFT_OUT_FILE: &str = "graphics-left-out.txt";

/// A marker is `graphics-testing.<process id>.txt`: a file of its own for each start, so that two starts at once never share one.
pub const MARKER_PREFIX: &str = "graphics-testing.";
pub const MARKER_SUFFIX: &str = ".txt";

/// The most adapters the list holds: a machine has a handful, and a list that grew past this holds something else (the oldest are dropped).
pub const MOST_LEFT_OUT: usize = 16;

/// The most bytes of either file that are read: a file past it is not ours.
const MOST_FILE_BYTES: u64 = 4096;

/// The most lines of a file that are looked at, the longest version and name that are kept, and the most entries of the settings folder a start looks through for markers.
const MOST_LINES: usize = 64;
const MOST_VERSION_CHARS: usize = 40;
const MOST_SCANNED: usize = 256;

const LEFT_OUT_VERB: &str = "left-out";
const MARKER_VERB: &str = "testing";

/// `Intel(R) UHD Graphics (dx12)`: an adapter in the words of the log and of a message.
pub fn describe(key: &Key) -> String {
    format!("{} ({})", key.0, key.1)
}

/// The text of a file: the header, the `version` and a line for each adapter, introduced by `verb` ([`parse_file`] reads it).
pub fn format_file(version: &str, verb: &str, keys: &[Key]) -> String {
    let mut text = format!("{HEADER}\nversion {}\n", version.trim());
    for (name, backend) in keys {
        let (name, backend) = key_of(name, *backend);
        text.push_str(&format!("{verb} {backend} {name}\n"));
    }
    text
}

/// What a file says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Parsed {
    /// The DiaCraft version that wrote it.
    pub version: String,
    /// The adapters of the lines that begin with the verb: at most [`MOST_LEFT_OUT`], none twice, each with a back end wgpu has and a name that is not empty.
    pub keys: Vec<Key>,
}

/// Reads the text of a file ([`format_file`]) for the lines that begin with `verb`. `Err` says in words why it is not ours: no [`HEADER`] first, or no version. A line that is none of
/// the lines it knows, and an adapter it cannot make sense of, is skipped, so that a file a later build wrote with more lines is still read.
pub fn parse_file(text: &str, verb: &str) -> Result<Parsed, String> {
    let mut lines = text.lines().take(MOST_LINES);
    if lines.next().map(str::trim_end) != Some(HEADER) {
        return Err(format!("it does not begin with `{HEADER}`"));
    }
    let mut version: Option<String> = None;
    let mut keys: Vec<Key> = Vec::new();
    for line in lines {
        if let Some(said) = line.strip_prefix("version ") {
            version.get_or_insert_with(|| said.trim().chars().take(MOST_VERSION_CHARS).collect());
        } else if let Some(key) = line.strip_prefix(verb).and_then(|rest| rest.strip_prefix(' ')).and_then(key_of_line)
            && keys.len() < MOST_LEFT_OUT
            && !keys.contains(&key)
        {
            keys.push(key);
        }
    }
    let version = version.filter(|v| !v.is_empty()).ok_or_else(|| "it names no version".to_owned())?;
    Ok(Parsed { version, keys })
}

/// The adapter of the rest of a line: `<back end> <name>`.
fn key_of_line(rest: &str) -> Option<Key> {
    let (backend, name) = rest.split_once(' ')?;
    let backend = Backend::ALL.into_iter().find(|b| b.to_str() == backend)?;
    let key = key_of(name, backend);
    (!key.0.is_empty()).then_some(key)
}

/// The text of `file`, read to at most [`MOST_FILE_BYTES`].
fn read_text(file: &mut File) -> Result<String, String> {
    let mut bytes = Vec::new();
    Read::take(&*file, MOST_FILE_BYTES + 1).read_to_end(&mut bytes).map_err(|e| format!("it could not be read: {e}"))?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MOST_FILE_BYTES {
        return Err(format!("it is longer than {MOST_FILE_BYTES} bytes"));
    }
    String::from_utf8(bytes).map_err(|_| "it is not text".to_owned())
}

/// Removes `path`; a file that is not there is removed already.
fn remove(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(format!("{} could not be removed: {e}", path.display())),
        _ => Ok(()),
    }
}

/// What the list file held.
struct Listed {
    keys: Vec<Key>,
    /// What to say in the log when the file was no use.
    note: Option<String>,
}

/// What the markers of the settings folder said.
#[derive(Default)]
struct Found {
    /// The adapters the markers of gone starts of this version name.
    keys: Vec<Key>,
    notes: Vec<String>,
}

/// What one marker was.
enum Taken {
    /// A start of this version ended while it tested this adapter.
    Left(Key),
    /// A start that is still running holds it.
    Alive,
    /// No use: why, in words.
    Ignored(String),
}

/// Where the files are and which DiaCraft this is. [`Store::default`] has no folder and reads and writes nothing.
#[derive(Clone, Debug, Default)]
pub struct Store {
    dir: Option<PathBuf>,
    version: String,
}

impl Store {
    /// The store of the settings folder `dir` (`None`: a run that keeps nothing on the disk) for DiaCraft `version`.
    pub fn new(dir: Option<PathBuf>, version: &str) -> Store {
        Store { dir, version: version.trim().to_owned() }
    }

    /// The folder the files are in, if there is one.
    pub fn dir(&self) -> Option<&Path> {
        self.dir.as_deref()
    }

    /// The adapters this start leaves out of every try: the list, and the adapters the markers of starts that ended while testing them name (which are added to the list, and their
    /// markers removed). Logs what it finds. Never fails: a file or a folder that is no use is a line in the log.
    pub fn open(&self) -> Vec<Key> {
        let Some(dir) = &self.dir else { return Vec::new() };
        let listed = self.read_list(dir);
        let found = self.take_markers(dir);
        for note in listed.note.iter().chain(&found.notes) {
            log::warn!("graphics: {note}");
        }
        let mut keys = listed.keys;
        let mut added = false;
        for key in found.keys {
            log::warn!("the last start ended while testing {}: it is left out", describe(&key));
            if !keys.contains(&key) {
                keys.push(key);
                added = true;
            }
        }
        if keys.len() > MOST_LEFT_OUT {
            keys.drain(..keys.len() - MOST_LEFT_OUT);
        }
        if added {
            self.write_list(dir, &keys);
        }
        if !keys.is_empty() {
            let named: Vec<String> = keys.iter().map(describe).collect();
            log::info!("graphics: left out of every try: {} (start DiaCraft with --reset-graphics to test them again)", named.join("; "));
        }
        keys
    }

    /// Writes the marker of a test of `key`, flushed to the disk, and holds it locked until [`Marker::done`]. With no folder, or when the file cannot be made (it is said in the log),
    /// the marker is none, and the test goes on.
    pub fn begin(&self, key: &Key) -> Marker {
        let Some(dir) = &self.dir else { return Marker { held: None } };
        match self.place(dir, key) {
            Ok(held) => Marker { held: Some(held) },
            Err(e) => {
                log::warn!("graphics: no marker for the test of {}: {e}", describe(key));
                Marker { held: None }
            }
        }
    }

    fn place(&self, dir: &Path, key: &Key) -> io::Result<(PathBuf, File)> {
        fs::create_dir_all(dir)?;
        let path = dir.join(format!("{MARKER_PREFIX}{}{MARKER_SUFFIX}", std::process::id()));
        let mut file = OpenOptions::new().read(true).write(true).create(true).truncate(true).open(&path)?;
        match file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Err(io::Error::other("another start holds a marker of the same name")),
            // A file system with no locks: the marker still says what was tested, and a later start takes the owner of a marker it cannot tell about for gone.
            Err(TryLockError::Error(_)) => {}
        }
        file.write_all(format_file(&self.version, MARKER_VERB, std::slice::from_ref(key)).as_bytes())?;
        // A driver that takes the machine down (a reset that ends in a blue screen) leaves a marker the cache never wrote, unless it was flushed first.
        file.sync_data()?;
        Ok((path, file))
    }

    /// Adds `key` to the list of adapters left out, at the end of the file (the oldest go when it is full). A start that finds a test did not return calls it.
    pub fn leave_out(&self, key: &Key) {
        let Some(dir) = &self.dir else { return };
        let mut keys = self.read_list(dir).keys;
        if keys.contains(key) {
            return;
        }
        keys.push(key.clone());
        if keys.len() > MOST_LEFT_OUT {
            keys.drain(..keys.len() - MOST_LEFT_OUT);
        }
        self.write_list(dir, &keys);
    }

    /// `diacraft --reset-graphics`: forgets every adapter left out, in the list and in the markers of gone starts. Returns what was left out.
    pub fn reset(&self) -> Vec<Key> {
        let Some(dir) = &self.dir else { return Vec::new() };
        let mut keys = self.read_list(dir).keys;
        if let Err(note) = remove(&dir.join(LEFT_OUT_FILE)) {
            log::warn!("graphics: {note}");
        }
        for key in self.take_markers(dir).keys {
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
        keys
    }

    /// The list file's adapters, with a note when the file was no use (it is removed then: another version's list is not this one's, and a file that is not ours is in the way).
    fn read_list(&self, dir: &Path) -> Listed {
        let path = dir.join(LEFT_OUT_FILE);
        let mut file = match File::open(&path) {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Listed { keys: Vec::new(), note: None },
            Err(e) => return Listed { keys: Vec::new(), note: Some(format!("{LEFT_OUT_FILE} could not be opened ({e}): ignored")) },
        };
        let note = match read_text(&mut file).and_then(|text| parse_file(&text, LEFT_OUT_VERB)) {
            Ok(parsed) if parsed.version == self.version => return Listed { keys: parsed.keys, note: None },
            Ok(parsed) => format!("the adapters DiaCraft {} left out are tested again by DiaCraft {}", parsed.version, self.version),
            Err(why) => format!("{LEFT_OUT_FILE}: {why}: ignored and removed"),
        };
        drop(file);
        let note = match remove(&path) {
            Ok(()) => note,
            Err(why) => format!("{note}; {why}"),
        };
        Listed { keys: Vec::new(), note: Some(note) }
    }

    /// Writes the list whole, through a file of its own that replaces it, so that a reader never sees half of it. A failure is a line in the log.
    fn write_list(&self, dir: &Path, keys: &[Key]) {
        let path = dir.join(LEFT_OUT_FILE);
        let temp = dir.join(format!("{LEFT_OUT_FILE}.{}.tmp", std::process::id()));
        let written = fs::create_dir_all(dir)
            .and_then(|()| fs::write(&temp, format_file(&self.version, LEFT_OUT_VERB, keys)))
            .and_then(|()| fs::rename(&temp, &path));
        if let Err(e) = written {
            log::warn!("graphics: {LEFT_OUT_FILE} could not be written in {}: {e}", dir.display());
            // A temporary file that is not there is the common case of the failure: nothing to clean.
            let _ = fs::remove_file(&temp);
        }
    }

    /// Every marker in `dir` that no running start holds, removed, and what each said.
    fn take_markers(&self, dir: &Path) -> Found {
        let mut found = Found::default();
        let Ok(entries) = fs::read_dir(dir) else { return found };
        // An entry that cannot be read is skipped: the folder is the person's, and only the markers in it matter.
        for entry in entries.flatten().take(MOST_SCANNED) {
            let name = entry.file_name().to_string_lossy().into_owned();
            let ours = name.starts_with(MARKER_PREFIX) && name.ends_with(MARKER_SUFFIX);
            if !ours || !entry.file_type().is_ok_and(|kind| kind.is_file()) {
                continue;
            }
            match self.take_marker(&entry.path(), &name) {
                Taken::Left(key) => found.keys.push(key),
                Taken::Alive => log::info!("graphics: {name} belongs to a start that is still running: not blamed on any adapter"),
                Taken::Ignored(why) => found.notes.push(why),
            }
        }
        found
    }

    fn take_marker(&self, path: &Path, name: &str) -> Taken {
        let mut file = match OpenOptions::new().read(true).write(true).open(path) {
            Ok(file) => file,
            Err(e) => return Taken::Ignored(format!("{name} could not be opened ({e}): ignored")),
        };
        match file.try_lock() {
            Err(TryLockError::WouldBlock) => return Taken::Alive,
            // Locked by us: its owner is gone. A file system with no locks: its owner is taken to be gone too, because a start that ended while testing is the worse thing to miss.
            Ok(()) | Err(TryLockError::Error(_)) => {}
        }
        let read = read_text(&mut file).and_then(|text| parse_file(&text, MARKER_VERB));
        // Removed while the handle is open; the lock goes with the handle.
        if let Err(why) = remove(path) {
            log::warn!("graphics: {why}");
        }
        match read {
            Ok(parsed) if parsed.version == self.version => {
                parsed.keys.into_iter().next().map_or_else(|| Taken::Ignored(format!("{name} names no adapter: ignored")), Taken::Left)
            }
            Ok(parsed) => Taken::Ignored(format!("{name} is from DiaCraft {}, not {}: ignored", parsed.version, self.version)),
            Err(why) => Taken::Ignored(format!("{name}: {why}: ignored")),
        }
    }
}

/// The marker of a test that is running ([`Store::begin`]). Dropped without [`Marker::done`] it stays on the disk, as a crash leaves it (its lock goes with the handle).
#[derive(Debug)]
pub struct Marker {
    held: Option<(PathBuf, File)>,
}

impl Marker {
    /// The test returned: removes the marker. A marker that cannot be removed is a line in the log (the next start would blame the adapter).
    pub fn done(self) {
        if let Some((path, file)) = self.held {
            if let Err(why) = remove(&path) {
                log::warn!("graphics: {why}");
            }
            drop(file);
        }
    }
}
