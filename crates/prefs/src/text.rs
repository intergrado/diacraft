//! The store over a [`PrefsHost`]: `config.toml` as text in a place that is not a folder, for the browser and for tests.
//!
//! It does what craft-settings' store does for a file, on the TOML helpers craft-settings offers on every target: the text is parsed with its comments, read section by section so that
//! one that does not fit resets alone, brought up to the current schema, and saved by merging the values into the document that was read, so that comments and keys this build does not
//! know stay. A text that does not parse is kept aside and the defaults are used; a text from a newer build is shown and never written over. **There is no quiet period before a save**:
//! storage in a browser is written at once and is atomic, so a change is written at the next tick. A write that fails is tried again after a wait that doubles.

use std::sync::Arc;
use std::time::Duration;

use craft_settings::document::{self, deserialize_sections, merge_into, set_schema_version};
use craft_settings::migrate::{self, Upgrade};
use toml_edit::DocumentMut;
use web_time::Instant;

use super::files::PrefsError;
use super::host::PrefsHost;
use super::sections::Prefs;
use super::{MAX_CONFIG_BYTES, many_keys_note, newer_note, too_many_keys};
use crate::shown;

/// The first wait before a failed write is tried again; each failure doubles it.
const FIRST_RETRY: Duration = Duration::from_secs(1);

/// The longest wait between tries.
const MAX_RETRY: Duration = Duration::from_secs(30);

/// What a tick did.
pub(super) enum Saved {
    /// Nothing was waiting.
    Idle,
    /// A write is waiting for its time.
    Waiting(Duration),
    /// Written.
    Done,
    /// The write failed; the next try is after the duration.
    Failed(PrefsError, Duration),
}

pub(super) struct HostStore<T: Prefs> {
    host: Arc<dyn PrefsHost>,
    doc: DocumentMut,
    value: T,
    /// The text as last read or written: compared by content, so another program's change is seen.
    seen: Option<String>,
    read_only: Option<String>,
    dirty: bool,
    failures: u32,
    retry_at: Option<Instant>,
}

/// The wait after `failures` failed writes in a row: 1 s, 2 s, 4 s, up to 30 s.
pub(super) fn retry_delay(failures: u32) -> Duration {
    let doublings = failures.saturating_sub(1).min(16);
    FIRST_RETRY.saturating_mul(2_u32.saturating_pow(doublings)).min(MAX_RETRY)
}

impl<T: Prefs> HostStore<T> {
    /// Reads the host's text. Never fails: what is wrong comes back as sentences for the person.
    pub(super) fn open(host: Arc<dyn PrefsHost>) -> (HostStore<T>, Vec<String>) {
        let mut store = HostStore {
            host,
            doc: document::new_document(T::SCHEMA_VERSION),
            value: T::default(),
            seen: None,
            read_only: None,
            dirty: false,
            failures: 0,
            retry_at: None,
        };
        let mut notes = Vec::new();
        match store.host.read_config() {
            Err(e) => {
                notes.push(format!("the settings could not be read ({e}); the settings of this run are not saved"));
                store.read_only = Some(e.to_string());
            }
            Ok(None) => {}
            Ok(Some(text)) if text.len() > MAX_CONFIG_BYTES => {
                notes.push(format!(
                    "the settings are {} bytes, more than the {MAX_CONFIG_BYTES} bytes settings may take; they were not read and the defaults are in use",
                    text.len()
                ));
            }
            Ok(Some(text)) => match too_many_keys(&text) {
                Some(keys) => notes.push(format!("{}; they were not read and the defaults are in use", many_keys_note("the settings have", keys))),
                None => store.load(text, &mut notes),
            },
        }
        (store, notes)
    }

    fn load(&mut self, text: String, notes: &mut Vec<String>) {
        let mut doc = match document::parse(&text) {
            Ok(doc) => doc,
            Err(_) => {
                let kept = match self.host.keep_broken(&text) {
                    Ok(place) => format!("a copy is kept in {place}"),
                    Err(e) => format!("it could not be kept ({e})"),
                };
                notes.push(format!("the settings are not valid TOML; {kept}, and the defaults are in use"));
                return;
            }
        };
        match migrate::upgrade::<T>(&mut doc) {
            Ok(Upgrade::Current) => {}
            Ok(Upgrade::Migrated { .. }) => self.dirty = true,
            Ok(Upgrade::Newer { file }) => self.newer(file, notes),
            Err(e) => {
                notes.push(format!("the settings could not be brought up to date ({e}); they are left as they are"));
                self.read_only = Some(e.to_string());
                return;
            }
        }
        let (value, errors) = deserialize_sections::<T>(&doc);
        for error in errors {
            notes.push(format!(
                "the [{}] part of the settings does not fit ({}); its defaults are in use",
                shown(&error.section, 40),
                shown(&error.message, 160)
            ));
        }
        self.value = value;
        self.doc = doc;
        self.seen = Some(text);
    }

    fn newer(&mut self, file: u32, notes: &mut Vec<String>) {
        notes.push(newer_note(file, T::SCHEMA_VERSION));
        self.read_only = Some(format!("the settings have version {file}, newer than this build's {}", T::SCHEMA_VERSION));
    }

    pub(super) fn get(&self) -> &T {
        &self.value
    }

    /// What opening changed (the editing language a profile took from the system, values brought into range) waits to be saved with the first change that is somebody's: a
    /// host that is a browser's storage holds nothing of a person who has set nothing.
    pub(super) fn settle(&mut self) {
        self.dirty = false;
    }

    pub(super) fn read_only(&self) -> Option<&str> {
        self.read_only.as_deref()
    }

    /// Replaces the value; whether it differs. A change is written at the next tick.
    pub(super) fn set(&mut self, value: T) -> bool {
        if value == self.value {
            return false;
        }
        self.value = value;
        self.dirty = true;
        true
    }

    pub(super) fn tick(&mut self, now: Instant) -> Saved {
        if !self.dirty || self.read_only.is_some() {
            return Saved::Idle;
        }
        if let Some(at) = self.retry_at
            && now < at
        {
            return Saved::Waiting(at.saturating_duration_since(now));
        }
        match self.save() {
            Ok(()) => Saved::Done,
            Err(e) => {
                self.failures = self.failures.saturating_add(1);
                let wait = retry_delay(self.failures);
                self.retry_at = Some(now + wait);
                Saved::Failed(e, wait)
            }
        }
    }

    /// Writes now, whatever the waiting says. Writes the text once even when nothing changed, as craft-settings writes its file.
    pub(super) fn flush(&mut self) -> Result<(), PrefsError> {
        if let Some(why) = &self.read_only {
            return Err(PrefsError::ReadOnly(why.clone()));
        }
        if !self.dirty && self.seen.is_some() {
            return Ok(());
        }
        self.save()
    }

    fn save(&mut self) -> Result<(), PrefsError> {
        if let Some(why) = &self.read_only {
            return Err(PrefsError::ReadOnly(why.clone()));
        }
        self.adopt_what_another_program_wrote()?;
        merge_into(&mut self.doc, &self.value).map_err(|e| PrefsError::Io(format!("the settings cannot be written as TOML: {e}")))?;
        set_schema_version(&mut self.doc, T::SCHEMA_VERSION);
        let text = document::render(&self.doc);
        if text.len() > MAX_CONFIG_BYTES {
            return Err(PrefsError::Refused(format!(
                "the settings would be more than {MAX_CONFIG_BYTES} bytes, which is more than settings may take"
            )));
        }
        self.host.write_config(&text)?;
        self.seen = Some(text);
        self.dirty = false;
        self.failures = 0;
        self.retry_at = None;
        Ok(())
    }

    /// If the text is not what this store last saw, another program (another tab) changed it: its keys and comments are kept, and this store's values win on the keys it knows.
    fn adopt_what_another_program_wrote(&mut self) -> Result<(), PrefsError> {
        let Some(text) = self.host.read_config()? else { return Ok(()) };
        if self.seen.as_deref() == Some(text.as_str()) || text.trim().is_empty() || text.len() > MAX_CONFIG_BYTES || too_many_keys(&text).is_some() {
            return Ok(());
        }
        let Ok(mut doc) = document::parse(&text) else {
            // Kept where a person can find it; whether it could be is no reason to lose the change being saved.
            let _ = self.host.keep_broken(&text);
            return Ok(());
        };
        match migrate::upgrade::<T>(&mut doc) {
            Ok(Upgrade::Newer { file }) => {
                let mut ignored = Vec::new();
                self.newer(file, &mut ignored);
                Err(PrefsError::ReadOnly(self.read_only.clone().unwrap_or_default()))
            }
            Err(e) => {
                self.read_only = Some(e.to_string());
                Err(PrefsError::ReadOnly(e.to_string()))
            }
            Ok(_) => {
                self.doc = doc;
                Ok(())
            }
        }
    }
}
