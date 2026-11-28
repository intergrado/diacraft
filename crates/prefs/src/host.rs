//! The host of a profile that has no file system of its own to reach: the text of `config.toml` and the files beside it, behind one trait.
//!
//! The browser's `localStorage` is one (`apps/diacraft-web/src/web_prefs.rs`), and [`MemoryHost`] is the one tests use and a run with no profile keeps its words in. The desktop
//! does not use a host for `config.toml`: craft-settings owns that file, with its recovery and its backup (`disk.rs`), and a host serves the files beside it ([`FileHost`]).

use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

use super::files::{Edit, FileHost, FileKind, FileName, PrefsError};
use crate::shown;

/// The most bytes of files a [`MemoryHost`] holds: a run with no profile keeps its word lists in memory, and this is the end of how much.
pub const MEMORY_FILES_BYTES: usize = 16 << 20;

/// The text of `config.toml` and the files, in a place that is not a folder. Every call returns at once.
pub trait PrefsHost: FileHost {
    /// The text of `config.toml`, `None` when there is none yet.
    fn read_config(&self) -> Result<Option<String>, PrefsError>;

    /// Replaces the text of `config.toml`: all of it or none of it.
    fn write_config(&self, text: &str) -> Result<(), PrefsError>;

    /// Keeps `text`, which would not parse, where a person can find it. Where it is kept, in words (`diacraft.config.broken`).
    fn keep_broken(&self, text: &str) -> Result<String, PrefsError>;
}

#[derive(Default)]
struct State {
    config: Option<String>,
    broken: Vec<String>,
    files: BTreeMap<FileName, Vec<u8>>,
    refuse: Option<String>,
}

/// A host in memory: nothing outside the process is read or written.
#[derive(Default)]
pub struct MemoryHost {
    state: Mutex<State>,
}

impl MemoryHost {
    fn state(&self) -> MutexGuard<'_, State> {
        // A panic while the lock was held leaves plain data behind it, which is still data.
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn writable(state: &State) -> Result<(), PrefsError> {
        match &state.refuse {
            Some(why) => Err(PrefsError::Io(why.clone())),
            None => Ok(()),
        }
    }

    fn too_much(file: &FileName) -> String {
        format!(
            "this run keeps no more than {} MiB of word lists and AutoCorrect lists in memory, and {} would take it to more than that or past its own {} bytes",
            MEMORY_FILES_BYTES >> 20,
            shown(&file.key(), 80),
            file.kind().max_bytes()
        )
    }

    /// The text of `config.toml` as it is now.
    pub fn config_text(&self) -> Option<String> {
        self.state().config.clone()
    }

    /// Sets the text of `config.toml` as if another program had written it.
    pub fn set_config(&self, text: &str) {
        self.state().config = Some(text.to_owned());
    }

    /// The texts kept because they would not parse.
    pub fn broken(&self) -> Vec<String> {
        self.state().broken.clone()
    }

    /// The bytes of `file`, if there is one.
    pub fn file(&self, file: &FileName) -> Option<Vec<u8>> {
        self.state().files.get(file).cloned()
    }

    /// Sets `file` as if another program had written it.
    pub fn put_file(&self, file: &FileName, bytes: &[u8]) {
        self.state().files.insert(file.clone(), bytes.to_vec());
    }

    /// From now on every write fails with `why`, as a full or forbidden storage would.
    pub fn refuse_writes(&self, why: &str) {
        self.state().refuse = Some(why.to_owned());
    }

    /// Lets writes succeed again.
    pub fn allow_writes(&self) {
        self.state().refuse = None;
    }
}

impl FileHost for MemoryHost {
    fn read(&self, file: &FileName, limit: usize) -> Result<Option<Vec<u8>>, PrefsError> {
        match self.state().files.get(file) {
            Some(bytes) if bytes.len() > limit => Err(PrefsError::Refused(format!("{} is more than {limit} bytes", shown(&file.key(), 80)))),
            other => Ok(other.cloned()),
        }
    }

    fn write(&self, file: &FileName, bytes: &[u8]) -> Result<(), PrefsError> {
        let mut state = self.state();
        Self::writable(&state)?;
        let others: usize = state.files.iter().filter(|(name, _)| *name != file).map(|(_, b)| b.len()).sum();
        if others.saturating_add(bytes.len()) > MEMORY_FILES_BYTES {
            return Err(PrefsError::Refused(Self::too_much(file)));
        }
        state.files.insert(file.clone(), bytes.to_vec());
        Ok(())
    }

    fn remove(&self, file: &FileName) -> Result<bool, PrefsError> {
        let mut state = self.state();
        Self::writable(&state)?;
        Ok(state.files.remove(file).is_some())
    }

    fn list(&self, kind: FileKind) -> Result<Vec<FileName>, PrefsError> {
        Ok(self.state().files.keys().filter(|name| name.kind() == kind).cloned().collect())
    }

    /// The whole step under the lock of the state, so that two threads' changes are both made.
    fn update(&self, file: &FileName, change: &mut dyn FnMut(Option<Vec<u8>>) -> Result<Edit, PrefsError>) -> Result<(), PrefsError> {
        let mut state = self.state();
        let old = state.files.get(file).cloned();
        if old.as_ref().is_some_and(|bytes| bytes.len() > file.kind().max_bytes()) {
            return Err(PrefsError::Refused(format!("{} is more than {} bytes", shown(&file.key(), 80), file.kind().max_bytes())));
        }
        match change(old)? {
            Edit::Keep => Ok(()),
            Edit::Remove => {
                Self::writable(&state)?;
                state.files.remove(file);
                Ok(())
            }
            Edit::Write(bytes) => {
                Self::writable(&state)?;
                let others: usize = state.files.iter().filter(|(name, _)| *name != file).map(|(_, b)| b.len()).sum();
                if bytes.len() > file.kind().max_bytes() || others.saturating_add(bytes.len()) > MEMORY_FILES_BYTES {
                    return Err(PrefsError::Refused(Self::too_much(file)));
                }
                state.files.insert(file.clone(), bytes);
                Ok(())
            }
        }
    }
}

impl PrefsHost for MemoryHost {
    fn read_config(&self) -> Result<Option<String>, PrefsError> {
        Ok(self.state().config.clone())
    }

    fn write_config(&self, text: &str) -> Result<(), PrefsError> {
        let mut state = self.state();
        Self::writable(&state)?;
        state.config = Some(text.to_owned());
        Ok(())
    }

    fn keep_broken(&self, text: &str) -> Result<String, PrefsError> {
        let mut state = self.state();
        Self::writable(&state)?;
        state.broken.push(text.to_owned());
        Ok("memory".to_owned())
    }
}
