//! The files beside `config.toml`: custom dictionaries and AutoCorrect lists, their names, and the host that keeps them.
//!
//! **On the desktop** they are files of the config folder: `proofing/dictionaries/<name>.dic` and `proofing/autocorrect/<language>.tsv`. **In the browser** each is a `localStorage` key.
//! **With no profile** (`DIACRAFT_NO_PREFS`, an empty `DIACRAFT_CONFIG_DIR`, a test) they are kept in memory for the run. The rest of the engine sees one thing, a [`FileHost`], and
//! names a file by a [`FileName`], which is a plain name in a folder of the kind and nothing else: the names come from `config.toml` and from commands, which are hostile.

use thiserror::Error;

use crate::shown;

/// Why the store refused something, or could not do it. Each says it in words for the person.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum PrefsError {
    /// A name, a word or a file the store will not take: it is not what it must be, or it is past a cap. Nothing was changed.
    #[error("{0}")]
    Refused(String),
    /// The settings are read-only (a file from a newer build, a folder that cannot be written): nothing is changed or saved.
    #[error("settings are read-only: {0}")]
    ReadOnly(String),
    /// The file system or the browser's storage said no.
    #[error("{0}")]
    Io(String),
}

impl PrefsError {
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn io(what: &str, e: &std::io::Error) -> PrefsError {
        PrefsError::Io(format!("{what}: {}", shown(&e.to_string(), 160)))
    }
}

/// The two kinds of file the settings keep beside `config.toml`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FileKind {
    /// A custom dictionary: a word to a line (`words.rs`).
    Dictionary,
    /// An AutoCorrect replacement list for a language: `typed<TAB>replacement` rows.
    AutoCorrect,
}

impl FileKind {
    /// Both kinds.
    pub const ALL: [FileKind; 2] = [FileKind::Dictionary, FileKind::AutoCorrect];

    /// The folder of `proofing/` the kind lives in.
    pub const fn folder(self) -> &'static str {
        match self {
            FileKind::Dictionary => "dictionaries",
            FileKind::AutoCorrect => "autocorrect",
        }
    }

    /// What a file of the kind is called in a sentence.
    pub const fn what(self) -> &'static str {
        match self {
            FileKind::Dictionary => "dictionary",
            FileKind::AutoCorrect => "AutoCorrect list",
        }
    }

    /// The extension a file of the kind has, in lower case.
    pub const fn extension(self) -> &'static str {
        match self {
            FileKind::Dictionary => "dic",
            FileKind::AutoCorrect => "tsv",
        }
    }

    /// The most bytes of a file of the kind (design 6.2 and 6.3: 1 MB a dictionary, 1 MiB an AutoCorrect list).
    pub const fn max_bytes(self) -> usize {
        match self {
            FileKind::Dictionary => super::words::MAX_DICT_FILE_BYTES,
            FileKind::AutoCorrect => 1 << 20,
        }
    }

    /// The kind whose folder is `folder`.
    pub fn of_folder(folder: &str) -> Option<FileKind> {
        FileKind::ALL.into_iter().find(|kind| kind.folder() == folder)
    }
}

/// The most characters of a file's name before its extension.
pub const MAX_STEM_CHARS: usize = 64;

/// The most bytes of a whole file name (what common file systems allow).
const MAX_NAME_BYTES: usize = 255;

/// A file of the settings: its kind and a plain name ending in the kind's extension.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileName {
    kind: FileKind,
    stem: String,
}

/// The device names Windows will not make a file of.
const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5",
    "LPT6", "LPT7", "LPT8", "LPT9",
];

impl FileName {
    /// `name` as a file of `kind`: a plain name of at most 64 characters before the extension, which is the kind's (`.dic`, `.tsv`, in any case). A name with a path in it
    /// (`/`, `\`, `:`), a character a file system will not take or a control character, a leading dot or space, a trailing dot or space, a Windows device name, or a wrong
    /// extension is refused.
    pub fn new(kind: FileKind, name: &str) -> Result<FileName, PrefsError> {
        let refuse = |why: &str| PrefsError::Refused(format!("`{}` is not a name for a {} file: {why}", shown(name, 40), kind.what()));
        if name.len() > MAX_NAME_BYTES {
            return Err(refuse("it is too long"));
        }
        let Some((stem, extension)) = name.rsplit_once('.') else {
            return Err(refuse(&format!("it must end in .{}", kind.extension())));
        };
        if !extension.eq_ignore_ascii_case(kind.extension()) {
            return Err(refuse(&format!("it must end in .{}", kind.extension())));
        }
        if stem.is_empty() || stem.chars().count() > MAX_STEM_CHARS {
            return Err(refuse(&format!("the name before the dot is 1 to {MAX_STEM_CHARS} characters")));
        }
        if stem.starts_with(['.', ' ']) || stem.ends_with(['.', ' ']) {
            return Err(refuse("it cannot begin or end with a dot or a space"));
        }
        if stem.chars().any(|c| c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')) {
            return Err(refuse("it has a character a file name cannot have"));
        }
        let device = stem.split('.').next().unwrap_or_default().to_ascii_uppercase();
        if RESERVED.contains(&device.as_str()) {
            return Err(refuse("it is the name of a device"));
        }
        Ok(FileName { kind, stem: stem.to_owned() })
    }

    /// The name `key` gives (`dictionaries/Custom.dic`), when it is one.
    pub fn from_key(key: &str) -> Option<FileName> {
        let (folder, name) = key.split_once('/')?;
        FileName::new(FileKind::of_folder(folder)?, name).ok()
    }

    pub fn kind(&self) -> FileKind {
        self.kind
    }

    /// The name before the extension.
    pub fn stem(&self) -> &str {
        &self.stem
    }

    /// The file's name, with the extension in lower case: `Custom.dic`.
    pub fn file(&self) -> String {
        format!("{}.{}", self.stem, self.kind.extension())
    }

    /// The name by which the settings and the browser's storage know it: `dictionaries/Custom.dic`.
    pub fn key(&self) -> String {
        format!("{}/{}", self.kind.folder(), self.file())
    }
}

/// What a [`FileHost::update`] makes of a file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Edit {
    /// Leave the file as it is: nothing is written.
    Keep,
    /// The file becomes these bytes.
    Write(Vec<u8>),
    /// The file is removed.
    Remove,
}

/// Where the settings keep their files. Every call returns at once; a host that has no thread or lock has none to wait for.
pub trait FileHost: Send + Sync {
    /// The bytes of `file`, `None` when there is no such file. A file of more than `limit` bytes is refused, not cut.
    fn read(&self, file: &FileName, limit: usize) -> Result<Option<Vec<u8>>, PrefsError>;

    /// Writes `file` whole: all of it or none of it.
    fn write(&self, file: &FileName, bytes: &[u8]) -> Result<(), PrefsError>;

    /// Removes `file`. Whether there was one.
    fn remove(&self, file: &FileName) -> Result<bool, PrefsError>;

    /// The files of `kind` there are, by name.
    fn list(&self, kind: FileKind) -> Result<Vec<FileName>, PrefsError>;

    /// Reads `file`, lets `change` decide what it becomes and does that: **one step**, so that a host that can lock holds the lock from the read to the write and two programs'
    /// changes of one file are both made (design call). A `change` that fails changes nothing.
    fn update(&self, file: &FileName, change: &mut dyn FnMut(Option<Vec<u8>>) -> Result<Edit, PrefsError>) -> Result<(), PrefsError> {
        let old = self.read(file, file.kind().max_bytes())?;
        apply(self, file, change(old)?)
    }
}

/// Does `edit` to `file` on `host`: a file past its kind's cap is refused.
pub(super) fn apply<H: FileHost + ?Sized>(host: &H, file: &FileName, edit: Edit) -> Result<(), PrefsError> {
    match edit {
        Edit::Keep => Ok(()),
        Edit::Write(bytes) if bytes.len() > file.kind().max_bytes() => {
            Err(PrefsError::Refused(format!("the {} would be more than {} bytes", file.kind().what(), file.kind().max_bytes())))
        }
        Edit::Write(bytes) => host.write(file, &bytes),
        Edit::Remove => host.remove(file).map(|_| ()),
    }
}
