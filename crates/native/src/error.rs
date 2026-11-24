//! Why a `.diacraft` file could not be read or written, in words a person can be shown and a class word a harness compares.
//!
//! **Names and words in a message come from a hostile file**, so [`Display`](fmt::Display) escapes them (the container's errors do the same), cuts a role word
//! to 64 characters, and never prints a part, a blob or a shape's name from the file: the one word of the file a message does carry is the role of a part the
//! file requires (the public spec's 5.2 asks for it by name).

use std::fmt;

use diacraft_container::ContainerError;
use diacraft_sheetxml::model::CodecError;

/// The class words of [`NativeError::Damaged`], each with the sentence it says, so that no class is without words (the tests walk the table).
pub(crate) const DAMAGE: &[(&str, &str)] = &[
    ("mimetype_bad", "its first entry, the media type, is not as the format says"),
    ("kind_unknown", "it names a kind of file this DiaCraft does not know"),
    ("kind_mismatch", "the kind the manifest names is not the kind the media type names"),
    ("manifest_missing", "the manifest is missing"),
    ("manifest_malformed", "the manifest is not what the format says a manifest is"),
    ("manifest_version", "the version in the manifest is not two numbers"),
    ("manifest_kind", "the manifest does not name a kind of file"),
    ("manifest_row", "a row of the manifest has no path or no role, or a path that is no name of an entry"),
    ("entry_unlisted", "an entry of the package is not in the manifest"),
    ("entry_missing", "the manifest lists an entry the package does not hold"),
    ("entry_listed_twice", "the manifest lists an entry twice"),
    ("document_missing", "the document part is missing"),
    ("part_missing", "a page or master part that the document part lists is missing"),
    ("part_unlisted", "a page or master part is not in the lists of the document part"),
    ("part_listed_twice", "a page or master part is named more than once by the lists of the document part"),
    ("state_malformed", "the state part is not what the format says it is"),
];

/// Why a file could not be read or written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NativeError {
    /// The bytes are no `.diacraft` file: the first entry is not a stored `mimetype` with no extra field that names a type of the format. Whatever the file is
    /// called. Class `not_native`.
    NotNative,
    /// The ZIP package or an XML part of it is refused: damaged, too large, too many entries, a name that would leave the package, and the rest of the
    /// container's classes.
    Container(ContainerError),
    /// The file is damaged (2.10). The word is the class (`kind_mismatch`, `entry_unlisted`, ...), and [`Display`](fmt::Display) says it in a sentence.
    Damaged(&'static str),
    /// The file was made by a version of the format this DiaCraft does not know: a major version other than 1. Class `newer_major`. Opened: a higher minor
    /// version of a major version known.
    NewerMajor {
        /// The major version the manifest states (up to nineteen digits).
        major: u64,
        /// Its minor version.
        minor: u64,
    },
    /// The file holds a part of a role this DiaCraft does not know and says the part is required (5.2): refused, naming the role. Class `required_role`.
    RequiredRole(String),
    /// The model is refused: a model part the vocabulary does not allow or a limit of the model, a budget or the XML reader, from the codec.
    Codec(CodecError),
    /// A file, a package or a part that is larger than a reader or a writer here holds, said in words that name what is large. Class `too_large`.
    TooLarge(&'static str),
    /// A model that cannot be written as a `.diacraft` file, and the reason in words: **a writer that cannot keep what the model holds does not write the file
    /// as if the model had not held it** (5.2). Class `unwritable`.
    Unwritable(&'static str),
}

impl NativeError {
    /// The class word: the container's or the codec's where one of them refused, and `not_native`, `newer_major`, `required_role`, `too_large` and
    /// `unwritable`; for a damaged file the word it carries.
    pub fn class(&self) -> &'static str {
        match self {
            NativeError::NotNative => "not_native",
            NativeError::Container(e) => e.class(),
            NativeError::Damaged(class) => class,
            NativeError::NewerMajor { .. } => "newer_major",
            NativeError::RequiredRole(_) => "required_role",
            NativeError::Codec(e) => e.class(),
            NativeError::TooLarge(_) => "too_large",
            NativeError::Unwritable(_) => "unwritable",
        }
    }

    /// The damage of class `class`: a word of [`DAMAGE`], which every caller here passes.
    pub(crate) fn damaged(class: &'static str) -> NativeError {
        NativeError::Damaged(class)
    }

    /// A refusal naming `role`, which the file requires and this DiaCraft does not know (cut to 64 characters).
    pub(crate) fn required_role(role: &str) -> NativeError {
        NativeError::RequiredRole(role.chars().take(64).collect())
    }
}

impl fmt::Display for NativeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NativeError::NotNative => write!(f, "this is not a DiaCraft file"),
            NativeError::Container(e) => write!(f, "{e}"),
            NativeError::Damaged(class) => {
                let words = DAMAGE.iter().find(|(c, _)| c == class).map_or("a part of it is not what the format says", |(_, words)| words);
                write!(f, "the file is damaged: {words}")
            }
            NativeError::NewerMajor { major, minor } => write!(f, "made by a newer DiaCraft (format {major}.{minor})"),
            NativeError::RequiredRole(role) => {
                write!(f, "the file needs a part of the role \"{}\" and this DiaCraft does not know it", role.escape_debug())
            }
            NativeError::Codec(e) => write!(f, "{e}"),
            NativeError::TooLarge(what) => write!(f, "{what}"),
            NativeError::Unwritable(why) => write!(f, "{why}"),
        }
    }
}

impl std::error::Error for NativeError {}

impl From<ContainerError> for NativeError {
    fn from(e: ContainerError) -> NativeError {
        NativeError::Container(e)
    }
}

impl From<CodecError> for NativeError {
    fn from(e: CodecError) -> NativeError {
        NativeError::Codec(e)
    }
}
