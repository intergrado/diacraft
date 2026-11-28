//! The desktop's store: craft-settings keeps `config.toml`, and the files beside it are files of a folder.
//!
//! **Where:** the folder rules are craft-settings' (an `OpenOptions::location` of `Dir`, then `DIACRAFT_CONFIG_DIR`, a `portable` file next to the program, then the platform's folder),
//! with the one addition that `DIACRAFT_NO_PREFS` set to anything, empty too, is [`Location::Memory`]: nothing is read or written ([`location_for`]). An empty `DIACRAFT_CONFIG_DIR`
//! is craft-settings' own way to the same.
//!
//! **Hostile files.** craft-settings reads `config.toml` whole, so a file is looked at before it opens: one past [`MAX_CONFIG_BYTES`], or that is not UTF-8 text, is moved aside
//! unread (`config.broken-<time>.toml`, as a file that is not TOML is) and the defaults are used. The files beside it are read by size first and never past their kind's cap,
//! only if they are regular files, and written whole through `craft-atomic`. A change of a file is made under a lock on `proofing/.lock` that is held from the read to the write, so
//! two programs adding words at once both keep their words. A lock that another program holds is waited for up to ten seconds; past that the change is refused in words and the
//! person tries again, and a lock the platform or the folder does not give at all is not waited for (the change is made without it, as craft-settings' own saves are best
//! effort). A word is never lost to a change made at the same moment.

use std::ffi::OsString;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use craft_atomic::{NewFilePermissions, WriteOptions};
use craft_settings::paths::{self, Portable};
use craft_settings::store::DEFAULT_DEBOUNCE;
use craft_settings::{LoadReport, Location, Probe, ReadOnlyReason, SettingsStore, recovery};

use super::files::{Edit, FileHost, FileKind, FileName, PrefsError, apply};
use super::locale::LocaleSource;
use super::sections::Prefs;
use super::{MAX_CONFIG_BYTES, many_keys_note, newer_note, too_many_keys};
use crate::shown;

/// The variable that opens no file and writes none (set to anything, as `DIACRAFT_NO_SPLASH` is).
pub const NO_PREFS: &str = "DIACRAFT_NO_PREFS";

/// The folder of the files beside `config.toml`.
const PROOFING_DIR: &str = "proofing";

/// The most entries of a folder that a listing looks at.
const MAX_LISTED: usize = 1_024;

/// How many times a lock is tried for, and how long between tries: ten seconds at most. A change holds it for a read and an atomic write (a flush to the disk), which a loaded
/// machine can make slow; giving up early would lose the other program's word or this one's.
const LOCK_TRIES: u32 = 1_000;
const LOCK_WAIT: Duration = Duration::from_millis(10);

/// How to open a profile on the desktop.
pub struct OpenOptions {
    /// Where the settings live. [`Location::Auto`] applies craft-settings' rules in order; [`location_for`] makes this from the environment.
    pub location: Location,
    /// What the folder rules look at: the environment, the program's folder, the platform's folder. A test gives its own.
    pub probe: Probe,
    /// Where the first editing language comes from.
    pub locale: LocaleSource,
    /// The quiet period after a change before it is saved.
    pub debounce: Duration,
}

impl OpenOptions {
    /// The options for `location`: the real environment, the system's language, a half-second quiet period.
    pub fn new(location: Location) -> OpenOptions {
        OpenOptions { location, probe: Probe::default(), locale: LocaleSource::System, debounce: DEFAULT_DEBOUNCE }
    }

    /// What the folder rules look at.
    pub fn probe(mut self, probe: Probe) -> OpenOptions {
        self.probe = probe;
        self
    }

    /// Where the first editing language comes from.
    pub fn locale(mut self, locale: LocaleSource) -> OpenOptions {
        self.locale = locale;
        self
    }

    /// The quiet period after a change before it is saved.
    pub fn debounce(mut self, debounce: Duration) -> OpenOptions {
        self.debounce = debounce;
        self
    }
}

/// [`Location::Memory`] when `DIACRAFT_NO_PREFS` is set to anything (the empty text too), else [`Location::Auto`], where craft-settings reads `DIACRAFT_CONFIG_DIR` itself (set
/// but empty: nothing read or written).
pub fn location_for(env: &dyn Fn(&str) -> Option<OsString>) -> Location {
    if env(NO_PREFS).is_some() { Location::Memory } else { Location::Auto }
}

/// What opening a store on the desktop made.
pub(super) struct Opened<T: Prefs> {
    pub store: SettingsStore<T>,
    pub files: DirOrMemory,
    pub notes: Vec<String>,
    pub dir: Option<PathBuf>,
    pub rule: String,
    pub read_only: Option<String>,
    pub locale: Option<String>,
}

/// The files beside `config.toml`: a folder when the profile has one, else memory.
pub(super) type DirOrMemory = std::sync::Arc<dyn FileHost>;

pub(super) fn open<T: Prefs>(options: OpenOptions) -> Opened<T> {
    let OpenOptions { mut location, probe, locale, debounce } = options;
    let portable = Portable { markers: T::PORTABLE_MARKERS, dir: T::PORTABLE_DIR };
    let resolved = paths::resolve(T::APP_ID, &portable, &location, &probe);
    let mut notes = Vec::new();
    if let Some(dir) = &resolved.dir {
        match precheck(&dir.join("config.toml"), true) {
            Precheck::Fine => {}
            Precheck::Moved(note) => notes.push(note),
            Precheck::Unmovable(note) => {
                // The file stays where it is and is never read: this run keeps its settings in memory.
                notes.push(note);
                location = Location::Memory;
            }
        }
    }
    let (store, load) = SettingsStore::<T>::open_with(craft_settings::OpenOptions::new(location).probe(probe).debounce(debounce));
    notes.extend(notes_of(&load));
    let read_only = load.read_only.as_ref().map(ReadOnlyReason::to_string);
    let files: DirOrMemory = match store.dir() {
        Some(dir) => std::sync::Arc::new(DirFiles { root: dir.join(PROOFING_DIR) }),
        None => std::sync::Arc::new(super::host::MemoryHost::default()),
    };
    Opened { dir: store.dir().map(Path::to_path_buf), rule: load.rule, store, files, notes, read_only, locale: locale.read() }
}

/// What looking at `config.toml` before it opens found.
pub(super) enum Precheck {
    /// Nothing wrong that craft-settings does not handle.
    Fine,
    /// It was moved aside: what to tell the person.
    Moved(String),
    /// It is wrong and could not be moved: what to tell the person. It must not be read.
    Unmovable(String),
}

/// Looks at `path` without reading it past the cap: a file that is too big, or (with `text`) that is not UTF-8 or has too many keys, is moved aside.
pub(super) fn precheck(path: &Path, text: bool) -> Precheck {
    let Ok(meta) = fs::metadata(path) else { return Precheck::Fine };
    // A folder is craft-settings' to refuse (it cannot be read). A device or a pipe (a link to `/dev/zero`) would be read without end, or waited for: it is never read.
    if meta.is_dir() {
        return Precheck::Fine;
    }
    if !meta.is_file() {
        return Precheck::Unmovable(
            "config.toml is not a regular file, and it was not read; the settings of this run are kept in memory only".to_owned(),
        );
    }
    let why = if meta.len() > MAX_CONFIG_BYTES as u64 {
        Some(format!("config.toml is {} bytes, more than the {MAX_CONFIG_BYTES} bytes settings may take", meta.len()))
    } else if !text {
        None
    } else {
        match read_text(path) {
            Some(Ok(text)) => too_many_keys(&text).map(|keys| many_keys_note("config.toml has", keys)),
            Some(Err(())) => Some("config.toml is not text (UTF-8)".to_owned()),
            None => None,
        }
    };
    let Some(why) = why else { return Precheck::Fine };
    match recovery::move_aside(path) {
        Ok(moved) => Precheck::Moved(format!("{why}; it was moved to {} and the defaults are in use", shown(&moved.display().to_string(), 200))),
        Err(e) => Precheck::Unmovable(format!(
            "{why}, and it could not be moved aside ({}); the settings of this run are kept in memory only",
            shown(&e.to_string(), 120)
        )),
    }
}

/// The file's text, read up to [`MAX_CONFIG_BYTES`] bytes: `Some(Err(()))` when it is not UTF-8, `None` when it cannot be read (craft-settings says so).
fn read_text(path: &Path) -> Option<Result<String, ()>> {
    let file = fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.take(MAX_CONFIG_BYTES as u64 + 1).read_to_end(&mut bytes).ok()?;
    Some(String::from_utf8(bytes).map_err(|_| ()))
}

/// The sentences for what craft-settings found when it opened.
fn notes_of(load: &LoadReport) -> Vec<String> {
    let mut notes = Vec::new();
    if load.broken_moved_to.is_some() {
        let moved = load.broken_moved_to.as_ref().map(|p| shown(&p.display().to_string(), 200)).unwrap_or_default();
        notes.push(format!("config.toml is not valid TOML; it was moved to {moved} and the defaults are in use"));
    }
    for error in &load.section_errors {
        notes.push(format!(
            "the [{}] part of config.toml does not fit ({}); its defaults are in use",
            shown(&error.section, 40),
            shown(&error.message, 160)
        ));
    }
    match &load.read_only {
        Some(ReadOnlyReason::NewerSchema { file, app }) => notes.push(newer_note(*file, *app)),
        Some(other) => notes.push(format!("{other}; the settings of this run are not saved")),
        None => {}
    }
    notes
}

/// The files beside `config.toml`, in `<config>/proofing/<folder>/`.
pub(super) struct DirFiles {
    pub root: PathBuf,
}

impl DirFiles {
    fn path(&self, file: &FileName) -> PathBuf {
        self.root.join(file.kind().folder()).join(file.file())
    }
}

impl FileHost for DirFiles {
    fn read(&self, file: &FileName, limit: usize) -> Result<Option<Vec<u8>>, PrefsError> {
        let path = self.path(file);
        let too_big = || PrefsError::Refused(format!("{} is more than {limit} bytes", shown(&file.key(), 80)));
        match fs::metadata(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(PrefsError::io(&file.key(), &e)),
            // A folder, a device or a pipe is not a file of ours, and a pipe would block the open.
            Ok(meta) if !meta.is_file() => return Err(PrefsError::Refused(format!("{} is not a regular file", shown(&file.key(), 80)))),
            Ok(meta) if meta.len() > limit as u64 => return Err(too_big()),
            Ok(_) => {}
        }
        let opened = fs::File::open(&path).map_err(|e| PrefsError::io(&file.key(), &e))?;
        let mut bytes = Vec::new();
        opened.take(limit as u64 + 1).read_to_end(&mut bytes).map_err(|e| PrefsError::io(&file.key(), &e))?;
        if bytes.len() > limit { Err(too_big()) } else { Ok(Some(bytes)) }
    }

    fn write(&self, file: &FileName, bytes: &[u8]) -> Result<(), PrefsError> {
        let path = self.path(file);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| PrefsError::io(&file.key(), &e))?;
        }
        WriteOptions::default()
            .with_new_file_permissions(NewFilePermissions::Private)
            .atomic_write(&path, bytes)
            .map_err(|e| PrefsError::io(&file.key(), &e))
    }

    fn remove(&self, file: &FileName) -> Result<bool, PrefsError> {
        match fs::remove_file(self.path(file)) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(PrefsError::io(&file.key(), &e)),
        }
    }

    fn list(&self, kind: FileKind) -> Result<Vec<FileName>, PrefsError> {
        let folder = self.root.join(kind.folder());
        let entries = match fs::read_dir(&folder) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(PrefsError::io(kind.folder(), &e)),
        };
        let mut names: Vec<FileName> = entries
            .flatten()
            .take(MAX_LISTED)
            .filter(|entry| fs::metadata(entry.path()).is_ok_and(|m| m.is_file()))
            .filter_map(|entry| entry.file_name().to_str().and_then(|name| FileName::new(kind, name).ok()))
            .collect();
        names.sort();
        Ok(names)
    }

    fn update(&self, file: &FileName, change: &mut dyn FnMut(Option<Vec<u8>>) -> Result<Edit, PrefsError>) -> Result<(), PrefsError> {
        fs::create_dir_all(&self.root).map_err(|e| PrefsError::io(PROOFING_DIR, &e))?;
        let _lock = Lock::take(&self.root.join(".lock"))?;
        let old = self.read(file, file.kind().max_bytes())?;
        apply(self, file, change(old)?)
    }
}

/// A lock on a file, held until it is dropped. Without one (the file cannot be made, the platform has no locks) the change goes on; a lock that another program holds for
/// ten seconds is an error, and nothing is changed.
struct Lock(Option<fs::File>);

impl Lock {
    fn take(path: &Path) -> Result<Lock, PrefsError> {
        let Ok(file) = fs::OpenOptions::new().create(true).truncate(false).write(true).open(path) else { return Ok(Lock(None)) };
        for _ in 0..LOCK_TRIES {
            match file.try_lock() {
                Ok(()) => return Ok(Lock(Some(file))),
                Err(fs::TryLockError::WouldBlock) => std::thread::sleep(LOCK_WAIT),
                Err(fs::TryLockError::Error(_)) => return Ok(Lock(None)),
            }
        }
        Err(PrefsError::Io("another program has been changing DiaCraft's word lists for ten seconds; try again in a moment".to_owned()))
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        if let Some(file) = &self.0 {
            // Closing the file lets go of the lock as well; this only says so first.
            let _ = file.unlock();
        }
    }
}
