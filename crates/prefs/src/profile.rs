//! The profile: the person's settings and the files beside them, in one object every program of DiaCraft holds.
//!
//! The window, the command line and the MCP server each open one. What it holds is a type `T` of the program's own (the engine's [`EnginePrefs`], or the window's, which holds those
//! sections and its own); what it does for `T` is the same on every platform. **Opening never fails**: a file that is broken, too big, from a newer build, or in a folder that cannot
//! be written comes back as notes for the person in the [`Report`], with the defaults or the file's values in use. **A change is named**: [`Profile::update`] returns the keys it
//! changed. **A program changes what the window changes**, in the same store, so that "add DiaCraft to the dictionary" lasts; a run with no profile (`DIACRAFT_NO_PREFS`) keeps all of
//! it in memory.
//!
//! **What opening writes.** On the desktop, a profile is written once so that people can find the file (craft-settings' rule), and what it took from the system (the first editing
//! language) with it. Over a host (the browser's storage) **opening writes nothing**: a person who has set nothing leaves nothing behind, until a change that is somebody's is
//! saved, whole, with the rest.
//!
//! The plan's name for the desktop profile is `DiskProfile`: it is [`Profile::open`] on the desktop, and the same type opened over a [`PrefsHost`] in the browser.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

#[cfg(not(target_arch = "wasm32"))]
use craft_settings::{Location, SaveStatus, SettingsError, SettingsStore};

use super::change::{Change, diff};
#[cfg(not(target_arch = "wasm32"))]
use super::disk::{self, OpenOptions, Precheck};
use super::files::{Edit, FileHost, FileKind, FileName, PrefsError};
use super::host::{MemoryHost, PrefsHost};
use super::locale::LocaleSource;
use super::sections::{EnginePrefs, LanguagePrefs, Prefs};
use super::text::{HostStore, Saved};
use super::words::{self, WordError, WordFile};
use crate::shown;

/// What opening a profile found, for the person: every sentence of `notes` is something to say once, in the window's status bar or on the command line's standard error.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// The folder `config.toml` is in. `None` in the browser, in memory, and when isolated.
    pub dir: Option<PathBuf>,
    /// Which rule chose it, or why there is none (craft-settings' words).
    pub rule: String,
    /// Whether the settings are kept between runs: a folder or a host's storage, and not memory.
    pub persistent: bool,
    /// Why the profile will not save, when it will not.
    pub read_only: Option<String>,
    /// What was wrong, and what was done about it. Empty for a profile that opened as it should.
    pub notes: Vec<String>,
}

impl Report {
    /// Whether there is anything to tell the person.
    pub fn has_problems(&self) -> bool {
        !self.notes.is_empty()
    }

    /// The notes as one sentence, for a place that holds one (the status bar).
    pub fn sentence(&self) -> Option<String> {
        (!self.notes.is_empty()).then(|| self.notes.join("; "))
    }
}

/// What one tick did, for the window to ask for its next frame and to tell the person of a save that failed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tick {
    /// A save was made in this tick.
    pub saved: bool,
    /// A save failed, in words. Said once until a save succeeds: a retry that fails again says nothing.
    pub failed: Option<String>,
    /// When to tick again so that a change that is waiting is saved though no one touches the window.
    pub next_in: Option<Duration>,
}

/// A change to the profile, as a session records it and a host saves it: the unit of `DiskProfile::save_change` of the plan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProfileChange {
    /// The engine's sections are now these (a command changed an option or a list); the keys that differ are written.
    Sections(Box<EnginePrefs>),
    /// A word added to a dictionary, merged into the file as it is now.
    AddWord { file: FileName, word: String },
    /// A word removed from a dictionary, merged into the file as it is now.
    RemoveWord { file: FileName, word: String },
    /// A file written whole (a list a person made, a dictionary attached).
    WriteFile { file: FileName, bytes: Vec<u8> },
    /// A file removed.
    RemoveFile { file: FileName },
}

enum Config<T: Prefs> {
    #[cfg(not(target_arch = "wasm32"))]
    Disk(Box<SettingsStore<T>>),
    Host(Box<HostStore<T>>),
}

/// The person's settings of type `T`, and the files beside them.
pub struct Profile<T: Prefs> {
    config: Config<T>,
    files: Arc<dyn FileHost>,
    /// What the system said its language is, when the profile was opened: a profile with no editing language takes it when it names one.
    locale: Option<String>,
    /// What happened since opening that the person should be told, taken by [`Profile::take_notes`].
    notes: Vec<String>,
    told_failure: bool,
    persistent: bool,
    dir: Option<PathBuf>,
    /// Why a store that became read-only after opening is (craft-settings says it only when a save refuses).
    #[cfg(not(target_arch = "wasm32"))]
    read_only_why: Option<String>,
}

fn refused(e: WordError) -> PrefsError {
    PrefsError::Refused(e.to_string())
}

#[cfg(not(target_arch = "wasm32"))]
fn settings_error(e: SettingsError) -> PrefsError {
    match e {
        SettingsError::ReadOnly(why) => PrefsError::ReadOnly(why),
        SettingsError::Serialize(e) => PrefsError::Io(format!("the settings cannot be written as TOML: {}", shown(&e, 160))),
        SettingsError::Io(e) => PrefsError::io("the settings could not be saved", &e),
    }
}

impl<T: Prefs> Profile<T> {
    /// Opens the profile of `location` on the desktop. Never fails; the report says what was wrong.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open(location: Location) -> (Profile<T>, Report) {
        Profile::open_with(OpenOptions::new(location))
    }

    /// Opens the profile this process's environment names: none under `DIACRAFT_NO_PREFS`, the folder `DIACRAFT_CONFIG_DIR` names (none if it is empty), else the platform's.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open_from_env() -> (Profile<T>, Report) {
        Profile::open(disk::location_for(&|key| std::env::var_os(key)))
    }

    /// Opens a profile on the desktop with every option.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open_with(options: OpenOptions) -> (Profile<T>, Report) {
        let opened = disk::open::<T>(options);
        let persistent = opened.dir.is_some();
        let mut profile = Profile {
            config: Config::Disk(Box::new(opened.store)),
            files: opened.files,
            locale: opened.locale,
            notes: Vec::new(),
            told_failure: false,
            persistent,
            dir: opened.dir.clone(),
            read_only_why: opened.read_only.clone(),
        };
        let mut notes = opened.notes;
        notes.extend(profile.finish_open());
        (profile, Report { dir: opened.dir, rule: opened.rule, persistent, read_only: opened.read_only, notes })
    }

    /// Opens a profile whose text and files are in `host` (the browser's storage). Never fails; the report says what was wrong.
    pub fn open_host(host: Arc<dyn PrefsHost>, locale: LocaleSource) -> (Profile<T>, Report) {
        Profile::from_host(host, locale, true)
    }

    /// A profile that keeps everything in memory for the run: what a run with no profile is, and what a test of the window opens.
    pub fn in_memory(locale: LocaleSource) -> Profile<T> {
        Profile::from_host(Arc::new(MemoryHost::default()), locale, false).0
    }

    fn from_host(host: Arc<dyn PrefsHost>, locale: LocaleSource, persistent: bool) -> (Profile<T>, Report) {
        let (store, mut notes) = HostStore::open(host.clone());
        let read_only = store.read_only().map(str::to_owned);
        let files: Arc<dyn FileHost> = host;
        let mut profile = Profile {
            config: Config::Host(Box::new(store)),
            files,
            locale: locale.read(),
            notes: Vec::new(),
            told_failure: false,
            persistent,
            dir: None,
            #[cfg(not(target_arch = "wasm32"))]
            read_only_why: None,
        };
        notes.extend(profile.finish_open());
        // What opening changed waits for a change that is somebody's, unless opening had something to say: then what was brought into range stays to be saved.
        if let (true, Config::Host(store)) = (notes.is_empty(), &mut profile.config) {
            store.settle();
        }
        let rule = if persistent { "the host's storage" } else { "in memory only" };
        (profile, Report { dir: None, rule: rule.to_owned(), persistent, read_only, notes })
    }

    /// Brings what was read into range and gives a profile with no editing language the system's, if the system names one; the notes it makes are the report's.
    fn finish_open(&mut self) -> Vec<String> {
        let mut value = self.get().clone();
        let notes = self.tidy(&mut value);
        self.set(value);
        notes
    }

    /// `value` brought into range, and with an editing language when the system names one. What it changed, in words.
    fn tidy(&self, value: &mut T) -> Vec<String> {
        let notes = value.sanitize();
        if value.engine().language.editing.is_empty() {
            value.engine_mut().language = LanguagePrefs::default_for(self.locale.as_deref());
        }
        notes
    }

    /// The settings now. In memory; cheap to call every frame.
    pub fn get(&self) -> &T {
        match &self.config {
            #[cfg(not(target_arch = "wasm32"))]
            Config::Disk(store) => store.get(),
            Config::Host(store) => store.get(),
        }
    }

    fn set(&mut self, value: T) {
        match &mut self.config {
            #[cfg(not(target_arch = "wasm32"))]
            Config::Disk(store) => {
                store.set(value);
            }
            Config::Host(store) => {
                store.set(value);
            }
        }
    }

    /// Changes the settings with `f`, brings the result into range, and names the keys that differ. Nothing changed gives an empty list and saves nothing. The change is saved
    /// after a quiet period (`tick`), or at once by `flush`.
    pub fn update(&mut self, f: impl FnOnce(&mut T)) -> Vec<Change> {
        let before = self.get().clone();
        let mut next = before.clone();
        f(&mut next);
        let notes = self.tidy(&mut next);
        self.notes.extend(notes);
        let changes = diff(&before, &next);
        self.set(next);
        changes
    }

    /// Where `config.toml` is; `None` for a profile in memory, isolated, or in a host.
    pub fn dir(&self) -> Option<&Path> {
        self.dir.as_deref()
    }

    /// Whether the settings are kept between runs.
    pub fn is_persistent(&self) -> bool {
        self.persistent
    }

    /// Why the profile will not save, when it will not: a file from a newer build, a folder that cannot be written. A change still lives for the run.
    pub fn read_only(&self) -> Option<&str> {
        match &self.config {
            #[cfg(not(target_arch = "wasm32"))]
            Config::Disk(store) => store.is_read_only().then(|| self.read_only_why.as_deref().unwrap_or("the settings file cannot be written")),
            Config::Host(store) => store.read_only(),
        }
    }

    fn ensure_writable(&self) -> Result<(), PrefsError> {
        match self.read_only() {
            Some(why) => Err(PrefsError::ReadOnly(why.to_owned())),
            None => Ok(()),
        }
    }

    /// Saves a change that has waited out its quiet period. Call it every frame, and again after [`Tick::next_in`], so that an idle window saves too.
    pub fn tick(&mut self) -> Tick {
        let mut tick = match &mut self.config {
            #[cfg(not(target_arch = "wasm32"))]
            Config::Disk(store) => {
                if let Some(path) = store.path()
                    && store.is_dirty()
                    && let Precheck::Moved(note) = disk::precheck(path, false)
                {
                    self.notes.push(note);
                }
                let now = std::time::Instant::now();
                let result = store.tick(now);
                let mut tick = Tick { next_in: result.next_deadline.map(|at| at.saturating_duration_since(now)), ..Tick::default() };
                match result.status {
                    SaveStatus::Saved => tick.saved = true,
                    SaveStatus::Failed(e) => {
                        let e = settings_error(e);
                        if let PrefsError::ReadOnly(why) = &e {
                            self.read_only_why = Some(why.clone());
                        }
                        tick.failed = Some(e.to_string());
                    }
                    SaveStatus::Idle | SaveStatus::Pending => {}
                }
                for moved in store.take_moved_aside() {
                    self.notes.push(format!(
                        "config.toml was changed to something that is not valid TOML while DiaCraft was running; it was moved to {}",
                        shown(&moved.display().to_string(), 200)
                    ));
                }
                tick
            }
            Config::Host(store) => match store.tick(web_time::Instant::now()) {
                Saved::Idle => Tick::default(),
                Saved::Waiting(wait) => Tick { next_in: Some(wait), ..Tick::default() },
                Saved::Done => Tick { saved: true, ..Tick::default() },
                Saved::Failed(e, wait) => Tick { failed: Some(e.to_string()), next_in: Some(wait), ..Tick::default() },
            },
        };
        if tick.saved {
            self.told_failure = false;
        }
        if tick.failed.is_some() && std::mem::replace(&mut self.told_failure, true) {
            tick.failed = None;
        }
        tick
    }

    /// Saves now, whatever the quiet period says. Call it when the program ends. A profile that is read-only refuses, in words.
    pub fn flush(&mut self) -> Result<(), PrefsError> {
        match &mut self.config {
            #[cfg(not(target_arch = "wasm32"))]
            Config::Disk(store) => {
                if let Some(path) = store.path()
                    && let Precheck::Moved(note) = disk::precheck(path, false)
                {
                    self.notes.push(note);
                }
                let saved = store.flush().map_err(settings_error);
                if let Err(PrefsError::ReadOnly(why)) = &saved {
                    self.read_only_why = Some(why.clone());
                }
                saved
            }
            Config::Host(store) => store.flush(),
        }
    }

    /// What happened since the profile opened that the person should be told, once: a file that was broken while the program ran and was moved aside, and edits that
    /// were brought into range. Takes it.
    pub fn take_notes(&mut self) -> Vec<String> {
        std::mem::take(&mut self.notes)
    }

    /// The files of `kind` there are.
    pub fn list_files(&self, kind: FileKind) -> Result<Vec<FileName>, PrefsError> {
        self.files.list(kind)
    }

    /// The bytes of `file`, `None` when there is none. A file past its kind's cap is refused.
    pub fn read_file(&self, file: &FileName) -> Result<Option<Vec<u8>>, PrefsError> {
        self.files.read(file, file.kind().max_bytes())
    }

    /// Changes `file` in one step, as [`FileHost::update`] says: what `change` makes of the bytes it is given is done under the lock. The caller names what it changed.
    pub fn update_file(&mut self, file: &FileName, change: &mut dyn FnMut(Option<Vec<u8>>) -> Result<Edit, PrefsError>) -> Result<(), PrefsError> {
        self.ensure_writable()?;
        self.files.update(file, change)
    }

    /// Writes `file` whole. Names what changed: a file that is new, one whose size changed, none when the bytes are the same.
    pub fn write_file(&mut self, file: &FileName, bytes: &[u8]) -> Result<Vec<Change>, PrefsError> {
        let mut was: Option<usize> = None;
        let mut same = false;
        self.update_file(file, &mut |old| {
            was = old.as_ref().map(Vec::len);
            same = old.as_deref() == Some(bytes);
            Ok(if same { Edit::Keep } else { Edit::Write(bytes.to_vec()) })
        })?;
        if same {
            return Ok(Vec::new());
        }
        Ok(vec![Change { setting: file.key(), from: was.map(size_text), to: Some(size_text(bytes.len())) }])
    }

    /// Removes `file`. Names it when there was one.
    pub fn remove_file(&mut self, file: &FileName) -> Result<Vec<Change>, PrefsError> {
        let mut was: Option<usize> = None;
        self.update_file(file, &mut |old| {
            was = old.as_ref().map(Vec::len);
            Ok(if old.is_some() { Edit::Remove } else { Edit::Keep })
        })?;
        Ok(was.map(|n| Change { setting: file.key(), from: Some(size_text(n)), to: None }).into_iter().collect())
    }

    /// Adds `word` to the dictionary `file`, merged into the file as it is now (so another program's words are kept). Names the word when it was added; a word that is there
    /// already, in any case, changes nothing. A file that is not a word list, or a word that is not one word, is refused and the file is left as it is.
    pub fn add_word(&mut self, file: &FileName, word: &str) -> Result<Vec<Change>, PrefsError> {
        let word = self.dictionary_word(file, word)?;
        let mut added = false;
        self.update_file(file, &mut |old| {
            let mut list = match old {
                Some(bytes) => words::read(&bytes).map_err(refused)?,
                None => WordFile::default(),
            };
            added = list.add(&word).map_err(refused)?;
            Ok(if added { Edit::Write(list.render()) } else { Edit::Keep })
        })?;
        Ok(if added { vec![Change { setting: file.key(), from: None, to: Some(word) }] } else { Vec::new() })
    }

    /// Removes `word` from the dictionary `file`, in any case. Names it when it was there.
    pub fn remove_word(&mut self, file: &FileName, word: &str) -> Result<Vec<Change>, PrefsError> {
        let word = self.dictionary_word(file, word)?;
        let mut removed = false;
        self.update_file(file, &mut |old| {
            let Some(bytes) = old else { return Ok(Edit::Keep) };
            let mut list = words::read(&bytes).map_err(refused)?;
            removed = list.remove(&word);
            Ok(if removed { Edit::Write(list.render()) } else { Edit::Keep })
        })?;
        Ok(if removed { vec![Change { setting: file.key(), from: Some(word), to: None }] } else { Vec::new() })
    }

    /// `word` checked as one word of the dictionary `file`.
    fn dictionary_word(&self, file: &FileName, word: &str) -> Result<String, PrefsError> {
        if file.kind() != FileKind::Dictionary {
            return Err(PrefsError::Refused(format!("{} is not a dictionary", shown(&file.key(), 80))));
        }
        self.ensure_writable()?;
        words::check_word(word).map(str::to_owned).map_err(refused)
    }
}

impl<T: Prefs> Profile<T> {
    /// Saves a change a session recorded, as it was made, and names what it changed (empty: nothing). Changes are applied in the order the session made them.
    pub fn save_change(&mut self, change: &ProfileChange) -> Result<Vec<Change>, PrefsError> {
        match change {
            ProfileChange::Sections(now) => {
                self.ensure_writable()?;
                let now = (**now).clone();
                Ok(self.update(|t| *t.engine_mut() = now))
            }
            ProfileChange::AddWord { file, word } => self.add_word(file, word),
            ProfileChange::RemoveWord { file, word } => self.remove_word(file, word),
            ProfileChange::WriteFile { file, bytes } => self.write_file(file, bytes),
            ProfileChange::RemoveFile { file } => self.remove_file(file),
        }
    }
}

/// `n bytes`, as a change shows the size of a file.
fn size_text(n: usize) -> String {
    format!("{n} bytes")
}
