//! The person's settings in the browser: `localStorage` behind the window's [`PrefsHost`], held to a budget, with everything that does not need a browser.
//!
//! The browser has no craft-settings store, so the window keeps the same TOML text and the same files in the page's `localStorage`: `diacraft.config` is the text of `config.toml`,
//! `diacraft.proofing.dictionaries/Custom.dic` and `diacraft.proofing.autocorrect/en.tsv` are the files (design 6.1). [`StorageHost`] is the [`PrefsHost`] over any [`Storage`];
//! `web_prefs.rs` (`wasm32` only) is the `Storage` of the browser, and the tests give it a map. **A browser gives an origin about 5 MB for everything**, and other pages of the
//! origin and other apps' keys share it, so DiaCraft takes at most [`WEB_PREFS_BYTES`] of it, counted over all of its keys (names and values), and refuses a write past that in
//! words, with nothing changed. A file is text (a browser's storage holds strings): the word lists and the AutoCorrect lists DiaCraft writes are UTF-8, and bytes that are not
//! are refused. Nothing here reads a key that is not DiaCraft's or sends anything anywhere.

use diacraft_ui_egui::prefs_host::{FileHost, FileKind, FileName, PrefsError, PrefsHost};

/// The most bytes (names and values) all of DiaCraft's keys take in the browser's storage: 1 MiB.
pub const WEB_PREFS_BYTES: usize = 1 << 20;

/// What every key of DiaCraft's starts with. The budget counts the keys that do and no others.
pub const KEY_PREFIX: &str = "diacraft.";

/// The key that holds the text of `config.toml`.
pub const CONFIG_KEY: &str = "diacraft.config";

/// The key that holds a copy of the text of `config.toml` that would not parse.
pub const BROKEN_KEY: &str = "diacraft.config.broken";

/// What the key of a file starts with: `diacraft.proofing.dictionaries/Custom.dic`.
pub const FILE_PREFIX: &str = "diacraft.proofing.";

/// The most keys a listing of the storage looks at.
const MAX_KEYS: usize = 10_000;

/// The browser's storage, or a stand-in: strings by name. Every call returns at once. An error is the reason, in words.
pub trait Storage: Send + Sync {
    /// The value of `key`, `None` when there is none.
    fn get(&self, key: &str) -> Result<Option<String>, String>;

    /// Sets `key`: all of it or none of it. A full or forbidden storage says so.
    fn set(&self, key: &str, value: &str) -> Result<(), String>;

    /// Removes `key`.
    fn remove(&self, key: &str) -> Result<(), String>;

    /// The names of the keys there are, at most [`MAX_KEYS`] of them.
    fn keys(&self) -> Result<Vec<String>, String>;
}

/// The key a file has in the storage.
pub fn file_key(file: &FileName) -> String {
    format!("{FILE_PREFIX}{}", file.key())
}

/// A [`PrefsHost`] over a [`Storage`]: the text of `config.toml` and the files, as keys, within the budget.
pub struct StorageHost<S: Storage> {
    storage: S,
    budget: usize,
}

impl<S: Storage> StorageHost<S> {
    /// A host over `storage` with the budget of [`WEB_PREFS_BYTES`].
    pub fn new(storage: S) -> StorageHost<S> {
        StorageHost { storage, budget: WEB_PREFS_BYTES }
    }

    /// A host with a budget of its own, for a test that must reach it without writing a mebibyte.
    pub fn with_budget(storage: S, budget: usize) -> StorageHost<S> {
        StorageHost { storage, budget }
    }

    fn io(e: String) -> PrefsError {
        PrefsError::Io(e)
    }

    /// The bytes DiaCraft's keys take now, `key`'s own not counted.
    fn taken_by_the_others(&self, key: &str) -> Result<usize, PrefsError> {
        let mut total = 0_usize;
        for name in self.storage.keys().map_err(Self::io)?.into_iter().take(MAX_KEYS) {
            if name == key || !name.starts_with(KEY_PREFIX) {
                continue;
            }
            let value = self.storage.get(&name).map_err(Self::io)?.unwrap_or_default();
            total = total.saturating_add(name.len()).saturating_add(value.len());
        }
        Ok(total)
    }

    /// Sets `key` to `value` if all of DiaCraft's keys then take no more than the budget; else refuses in words and changes nothing.
    fn set_within_budget(&self, key: &str, value: &str) -> Result<(), PrefsError> {
        let total = self.taken_by_the_others(key)?.saturating_add(key.len()).saturating_add(value.len());
        if total > self.budget {
            return Err(PrefsError::Refused(format!(
                "DiaCraft's settings in this browser would take more than {} of its storage ({total} bytes); nothing was changed",
                size_words(self.budget)
            )));
        }
        self.storage.set(key, value).map_err(Self::io)
    }
}

/// `1 MiB`, `64 KiB`, `300 bytes`.
fn size_words(bytes: usize) -> String {
    if bytes >= 1 << 20 && bytes.is_multiple_of(1 << 20) {
        format!("{} MiB", bytes >> 20)
    } else if bytes >= 1 << 10 && bytes.is_multiple_of(1 << 10) {
        format!("{} KiB", bytes >> 10)
    } else {
        format!("{bytes} bytes")
    }
}

impl<S: Storage> FileHost for StorageHost<S> {
    fn read(&self, file: &FileName, limit: usize) -> Result<Option<Vec<u8>>, PrefsError> {
        let Some(text) = self.storage.get(&file_key(file)).map_err(Self::io)? else { return Ok(None) };
        if text.len() > limit {
            return Err(PrefsError::Refused(format!("{} is more than {limit} bytes", file.key())));
        }
        Ok(Some(text.into_bytes()))
    }

    fn write(&self, file: &FileName, bytes: &[u8]) -> Result<(), PrefsError> {
        let Ok(text) = std::str::from_utf8(bytes) else {
            return Err(PrefsError::Refused(format!("the browser keeps text files only, and {} is not UTF-8 text", file.key())));
        };
        self.set_within_budget(&file_key(file), text)
    }

    fn remove(&self, file: &FileName) -> Result<bool, PrefsError> {
        let key = file_key(file);
        let was = self.storage.get(&key).map_err(Self::io)?.is_some();
        if was {
            self.storage.remove(&key).map_err(Self::io)?;
        }
        Ok(was)
    }

    fn list(&self, kind: FileKind) -> Result<Vec<FileName>, PrefsError> {
        let mut names: Vec<FileName> = self
            .storage
            .keys()
            .map_err(Self::io)?
            .iter()
            .take(MAX_KEYS)
            .filter_map(|key| key.strip_prefix(FILE_PREFIX))
            .filter_map(FileName::from_key)
            .filter(|name| name.kind() == kind)
            .collect();
        names.sort();
        Ok(names)
    }
}

impl<S: Storage> PrefsHost for StorageHost<S> {
    fn read_config(&self) -> Result<Option<String>, PrefsError> {
        self.storage.get(CONFIG_KEY).map_err(Self::io)
    }

    fn write_config(&self, text: &str) -> Result<(), PrefsError> {
        self.set_within_budget(CONFIG_KEY, text)
    }

    fn keep_broken(&self, text: &str) -> Result<String, PrefsError> {
        self.set_within_budget(BROKEN_KEY, text)?;
        Ok(BROKEN_KEY.to_owned())
    }
}
