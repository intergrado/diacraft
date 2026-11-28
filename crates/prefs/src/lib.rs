//! The person's settings: the one store of the window, the command line and the MCP server, for the language, translation and help features, the proofing options and
//! the connection-point distance of Options.
//!
//! **What is kept, and where** (design 6.1). `config.toml`, with a `[section]` for each area, in the platform's config folder (`$XDG_CONFIG_HOME/diacraft/` or
//! `~/.config/diacraft/` on Linux, `~/Library/Application Support/diacraft/` on macOS, `%APPDATA%\diacraft\` on Windows), through craft-settings: saved atomically, with a
//! backup (`config.toml.bak`), a broken file moved aside (`config.broken-<time>.toml`) and a file from a newer build never written over. Beside it, `proofing/dictionaries/*.dic` and
//! `proofing/autocorrect/<language>.tsv`, written whole through `craft-atomic`. In the browser the same text is the `localStorage` key `diacraft.config` and each file is a key
//! `diacraft.proofing.<folder>/<name>` (`apps/diacraft-web`). **`DIACRAFT_NO_PREFS`** (set to anything) and an empty **`DIACRAFT_CONFIG_DIR`** open a profile in memory:
//! nothing is read or written, and the run keeps its changes and its words until it ends. `DIACRAFT_CONFIG_DIR` set to a folder moves the profile there. Tests are isolated the
//! same way.
//!
//! **Who owns what.** [`EnginePrefs`] holds the sections the engine reads (`[proofing]`, `[autocorrect]`, `[language]`), so the command line and the MCP server read what the window
//! does. The window's type holds those and its own (`diacraft_ui_egui::prefs_host::WindowPrefs`), and each program opens a [`Profile`] of its type on the same file: craft-settings merges
//! key by key and keeps keys a type does not know, so two programs with different types share one file safely.
//!
//! **A key is added by adding a field** to a section, or a section to a type that implements [`Prefs`]: the store holds no list of keys, names a change by reading the keys
//! ([`diff`]), and keeps what it does not know. The interface language, the tips and search, and CPDOUBLE's minimum distance each add their keys to the window's
//! type and change nothing here.
//!
//! **Hostile input.** The file, its text and the lists in it are read as hostile: `config.toml` is read only up to [`MAX_CONFIG_BYTES`], a file that is not UTF-8 text or is too big
//! is moved aside unread, every list is capped and every entry checked ([`EnginePrefs::bring_into_range`]), and the files beside it are named by [`FileName`], a plain name
//! that cannot leave its folder. What is wrong is said once, in words, in the [`Report`] of opening; the app never stops for it.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod change;
#[cfg(not(target_arch = "wasm32"))]
mod disk;
mod files;
mod host;
mod locale;
mod profile;
mod safe;
mod sections;
mod text;
pub mod words;

pub use change::{Change, MAX_CHANGES, changes_json, diff};
pub use craft_settings::Settings;
#[cfg(not(target_arch = "wasm32"))]
pub use craft_settings::{Location, Probe};
#[cfg(not(target_arch = "wasm32"))]
pub use disk::{NO_PREFS, OpenOptions, location_for};
pub use files::{Edit, FileHost, FileKind, FileName, MAX_STEM_CHARS, PrefsError};
pub use host::{MEMORY_FILES_BYTES, MemoryHost, PrefsHost};
pub use locale::{LocaleSource, MAX_TAG_BYTES, canonical_tag, tag_of_locale};
pub use profile::{Profile, ProfileChange, Report, Tick};
pub(crate) use safe::shown;
pub use sections::{
    ALL_LANGUAGES, AutoCorrectExceptions, AutoCorrectPrefs, DEFAULT_LANGUAGE, DictionaryEntry, EnginePrefs, LanguagePrefs, MAX_DICTIONARIES,
    MAX_DICTIONARY_NAME_CHARS, MAX_EDITING_LANGUAGES, MAX_EXCEPTION_CHARS, MAX_EXCEPTIONS, Prefs, ProofingPrefs,
};
pub use words::{MAX_DICT_FILE_BYTES, MAX_WORD_CHARS, MAX_WORDS, WordError, WordFile};

/// The version of `config.toml`'s layout every program of DiaCraft writes. A program that finds a larger one in the file leaves it as it is.
pub const SCHEMA_VERSION: u32 = 1;

/// The most bytes of `config.toml`: a file past it is not settings and is moved aside unread (the real file is a few kilobytes).
pub const MAX_CONFIG_BYTES: usize = 1 << 20;

/// The most top-level keys of `config.toml` (sections and keys of its own): the file has a handful, and craft-settings tries each key it finds against the type, so a file of
/// a hundred thousand is not read.
pub const MAX_TOP_KEYS: usize = 256;

/// How many top-level keys `text` has when it parses and has more than [`MAX_TOP_KEYS`]; `None` for any other text (one that does not parse is for the reader to say).
pub(crate) fn too_many_keys(text: &str) -> Option<usize> {
    let doc = craft_settings::document::parse(text).ok()?;
    let keys = doc.iter().count();
    (keys > MAX_TOP_KEYS).then_some(keys)
}

/// What to tell the person about a file with more keys than settings have: `has` is `config.toml has` or `the settings have`.
pub(crate) fn many_keys_note(has: &str, keys: usize) -> String {
    format!("{has} {keys} top-level keys, more than the {MAX_TOP_KEYS} settings may have")
}

/// What to tell the person about a file from a newer build.
pub(crate) fn newer_note(file: u32, app: u32) -> String {
    format!("config.toml was written by a newer DiaCraft (settings version {file}; this one reads {app}); it is shown and left as it is")
}
