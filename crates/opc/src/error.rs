//! What can go wrong, as data. Every error has a [`class`](OpcError::class): the snake-case word of the error table in
//! where the spec names one, so that a harness compares classes and not wording. Where the
//! spec's table has no word for a case, the class is this crate's own and the variant says so; the spec owner may rename it.
//!
//! Names and messages come from the file, which is hostile: [`Display`](std::fmt::Display) escapes them, so an error shown in a
//! log or a dialog cannot carry control characters or terminal escapes.
//!
//! The errors of the archive and of an XML tree are the container crate's, by the same classes: [`XmlError`] is its type, and its
//! `ContainerError` becomes the [`OpcError`] of the same name and class.

use std::fmt;

use diacraft_container::ContainerError;
/// Why an XML part could not be read: the container crate's, under the name it always had here.
pub use diacraft_container::XmlError;

/// Why a package, or one of its relationship parts, could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpcError {
    /// The bytes are not a ZIP archive. Class `not_zip`.
    NotZip,
    /// The bytes are a compound file (the format of old Office files and of encrypted packages). Nothing is read from it. Class
    /// `ole_with_package_extension` (the crate sees bytes, not an extension: the caller knows the extension).
    OleWithPackageExtension,
    /// The central directory cannot be read, entries overlap or lie about where they are, a part fails to inflate, or its size or
    /// checksum is not what the archive says. Class `zip_corrupt`.
    ZipCorrupt { detail: String },
    /// An item name with `..`, a leading slash, a backslash or a drive letter. Class `path_traversal`. No name from the
    /// archive ever reaches a file system: this crate opens no file.
    PathTraversal { name: String },
    /// An item name that is not a legal part name for another reason: empty, an empty segment, a segment ending in a dot, a
    /// control character, bytes that are not UTF-8 once percent-decoded, or longer than the limit. Class `invalid_part_name`
    /// (the spec's table has no word for these).
    InvalidPartName { name: String, reason: &'static str },
    /// A part inflates past the ratio limit, by what its header says or by the bytes it really holds. Class
    /// `zip_ratio_exceeded`.
    ZipRatioExceeded { name: String, limit: u64 },
    /// A part larger than the per-part limit, by its header or in fact. Class `part_too_large`.
    PartTooLarge { name: String, limit: u64 },
    /// All parts together larger than the package limit, by their headers or in fact. Class `package_too_large` (the
    /// spec's table names `part_too_large` and `too_many_parts` for R23 and has no word for the third cap).
    PackageTooLarge { limit: u64 },
    /// More parts than the limit. Class `too_many_parts`.
    TooManyParts { limit: usize },
    /// A compression method other than stored (0) or deflate (8). Class `unsupported_compression`.
    UnsupportedCompression { name: String, method: String },
    /// A part is encrypted. Class `encrypted_package`.
    EncryptedPackage { name: String },
    /// Two items whose names are equal when ASCII case is folded, or the same name twice. Class `duplicate_part`.
    DuplicatePart { first: String, second: String },
    /// A part name that is a path prefix of another: `/a` and `/a/b`. Class `part_name_prefix` (the spec's table has no
    /// word for it).
    PartNamePrefix { prefix: String, part: String },
    /// There is no `[Content_Types].xml`: this is not an Open Packaging Conventions package. Class `missing_content_types`.
    /// The `vsdx` crate reports what the user sees as `no_drawing_parts`.
    MissingContentTypes,
    /// A relationships part with more rows than the limit. Class `too_many_relationships` (labelled choice).
    TooManyRelationships { part: String, limit: usize },
    /// A content types stream with more rows than the limit. Class `too_many_content_type_entries` (labelled choice).
    TooManyContentTypeEntries { limit: usize },
    /// An XML part this crate parses itself (`[Content_Types].xml`, a `.rels` part) could not be read. The class is the XML one.
    Xml { part: String, error: XmlError },
    /// Memory ran out while a part was being read: reported, not an abort. Class `out_of_memory` (this crate's).
    OutOfMemory { what: String },
}

/// What a caller shows the user, in three values, so that nobody re-derives it from the whole of [`OpcError`] (
/// finding 13). [`OpcError::class`] is for harnesses that compare words; this is for people.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// Not something this reader reads as a package: not a ZIP, a compound file, no content types stream, encrypted, or compressed in a
    /// way the format does not allow.
    NotAPackage,
    /// A package, but past a limit: too many parts, a part or the whole too large, inflating past the ratio, an XML tree or a list of
    /// rows past its limit, or memory that ran out. A larger [`Limits`](crate::Limits) may open it.
    TooBig,
    /// A package that contradicts itself or the format: damaged, truncated, or holding a name or XML that is refused.
    Damaged,
}

impl OpcError {
    /// Which of three things went wrong ([`ErrorKind`]).
    pub fn kind(&self) -> ErrorKind {
        match self {
            OpcError::NotZip
            | OpcError::OleWithPackageExtension
            | OpcError::MissingContentTypes
            | OpcError::EncryptedPackage { .. }
            | OpcError::UnsupportedCompression { .. } => ErrorKind::NotAPackage,
            OpcError::ZipRatioExceeded { .. }
            | OpcError::PartTooLarge { .. }
            | OpcError::PackageTooLarge { .. }
            | OpcError::TooManyParts { .. }
            | OpcError::TooManyRelationships { .. }
            | OpcError::TooManyContentTypeEntries { .. }
            | OpcError::OutOfMemory { .. } => ErrorKind::TooBig,
            OpcError::Xml { error, .. } => match error {
                XmlError::TooDeep { .. } | XmlError::TooManyNodes { .. } | XmlError::AttributeTooLarge { .. } => ErrorKind::TooBig,
                XmlError::Malformed { .. } | XmlError::EntityRefused { .. } => ErrorKind::Damaged,
            },
            OpcError::ZipCorrupt { .. }
            | OpcError::PathTraversal { .. }
            | OpcError::InvalidPartName { .. }
            | OpcError::DuplicatePart { .. }
            | OpcError::PartNamePrefix { .. } => ErrorKind::Damaged,
        }
    }

    /// The class word. See the variants for which come from the spec and which are this crate's.
    pub fn class(&self) -> &'static str {
        match self {
            OpcError::NotZip => "not_zip",
            OpcError::OleWithPackageExtension => "ole_with_package_extension",
            OpcError::ZipCorrupt { .. } => "zip_corrupt",
            OpcError::PathTraversal { .. } => "path_traversal",
            OpcError::InvalidPartName { .. } => "invalid_part_name",
            OpcError::ZipRatioExceeded { .. } => "zip_ratio_exceeded",
            OpcError::PartTooLarge { .. } => "part_too_large",
            OpcError::PackageTooLarge { .. } => "package_too_large",
            OpcError::TooManyParts { .. } => "too_many_parts",
            OpcError::UnsupportedCompression { .. } => "unsupported_compression",
            OpcError::EncryptedPackage { .. } => "encrypted_package",
            OpcError::DuplicatePart { .. } => "duplicate_part",
            OpcError::PartNamePrefix { .. } => "part_name_prefix",
            OpcError::MissingContentTypes => "missing_content_types",
            OpcError::TooManyRelationships { .. } => "too_many_relationships",
            OpcError::TooManyContentTypeEntries { .. } => "too_many_content_type_entries",
            OpcError::Xml { error, .. } => error.class(),
            OpcError::OutOfMemory { .. } => "out_of_memory",
        }
    }
}

impl fmt::Display for OpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OpcError::NotZip => write!(f, "not a ZIP archive"),
            OpcError::OleWithPackageExtension => write!(f, "a compound file, not a ZIP package (an old or an encrypted Office file)"),
            OpcError::ZipCorrupt { detail } => write!(f, "the ZIP archive is damaged: {}", detail.escape_debug()),
            OpcError::PathTraversal { name } => write!(f, "item name \"{}\" would leave the package", name.escape_debug()),
            OpcError::InvalidPartName { name, reason } => write!(f, "item name \"{}\" is not a legal part name: {reason}", name.escape_debug()),
            OpcError::ZipRatioExceeded { name, limit } => write!(f, "part \"{}\" inflates more than {limit} times its size", name.escape_debug()),
            OpcError::PartTooLarge { name, limit } => write!(f, "part \"{}\" is larger than {limit} bytes", name.escape_debug()),
            OpcError::PackageTooLarge { limit } => write!(f, "the parts together are larger than {limit} bytes"),
            OpcError::TooManyParts { limit } => write!(f, "the package has more than {limit} parts"),
            OpcError::UnsupportedCompression { name, method } => {
                write!(
                    f,
                    "part \"{}\" uses compression method {}, and only stored and deflate are allowed",
                    name.escape_debug(),
                    method.escape_debug()
                )
            }
            OpcError::EncryptedPackage { name } => write!(f, "part \"{}\" is encrypted", name.escape_debug()),
            OpcError::DuplicatePart { first, second } => {
                write!(f, "items \"{}\" and \"{}\" are the same part name", first.escape_debug(), second.escape_debug())
            }
            OpcError::PartNamePrefix { prefix, part } => {
                write!(f, "part \"{}\" is a path prefix of part \"{}\"", prefix.escape_debug(), part.escape_debug())
            }
            OpcError::MissingContentTypes => write!(f, "the package has no [Content_Types].xml"),
            OpcError::TooManyRelationships { part, limit } => {
                write!(f, "\"{}\" has more than {limit} relationships", part.escape_debug())
            }
            OpcError::TooManyContentTypeEntries { limit } => write!(f, "[Content_Types].xml has more than {limit} rows"),
            OpcError::Xml { part, error } => write!(f, "\"{}\": {error}", part.escape_debug()),
            OpcError::OutOfMemory { what } => write!(f, "out of memory while reading {}", what.escape_debug()),
        }
    }
}

impl std::error::Error for OpcError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            OpcError::Xml { error, .. } => Some(error),
            _ => None,
        }
    }
}

impl From<ContainerError> for OpcError {
    /// The error of the archive as the one of this crate with the same name, class and fields.
    fn from(error: ContainerError) -> Self {
        match error {
            ContainerError::NotZip => OpcError::NotZip,
            ContainerError::ZipCorrupt { detail } => OpcError::ZipCorrupt { detail },
            ContainerError::PathTraversal { name } => OpcError::PathTraversal { name },
            ContainerError::InvalidPartName { name, reason } => OpcError::InvalidPartName { name, reason },
            ContainerError::ZipRatioExceeded { name, limit } => OpcError::ZipRatioExceeded { name, limit },
            ContainerError::PartTooLarge { name, limit } => OpcError::PartTooLarge { name, limit },
            ContainerError::PackageTooLarge { limit } => OpcError::PackageTooLarge { limit },
            ContainerError::TooManyParts { limit } => OpcError::TooManyParts { limit },
            ContainerError::UnsupportedCompression { name, method } => OpcError::UnsupportedCompression { name, method },
            ContainerError::EncryptedPackage { name } => OpcError::EncryptedPackage { name },
            ContainerError::DuplicatePart { first, second } => OpcError::DuplicatePart { first, second },
            ContainerError::Xml { part, error } => OpcError::Xml { part, error },
            ContainerError::OutOfMemory { what } => OpcError::OutOfMemory { what },
        }
    }
}

/// A thing found while reading that is not an error. The reader keeps the data and says what it saw (the spec's "report it").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Warning {
    /// Bytes after the end record and its comment, ignored: how many. The record was the last one in the final 65,557
    /// bytes whose comment ends inside the file, because none ended exactly at the end.
    BytesAfterEndRecord { count: usize },
    /// A directory item such as `visio/pages/`: not a part. Dropped.
    DirectoryItem { name: String },
    /// More directory items than are named one by one: how many were dropped without a name. Dropped all the same.
    MoreDirectoryItems { count: usize },
    /// A relationships part whose source part is not in the package: kept as a part, and reported. `part` is the relationships
    /// part.
    OrphanRelationships { part: String },
    /// More relationships parts with no source part than are named one by one: how many were reported without a name. They are parts
    /// and are kept all the same.
    MoreOrphanRelationships { count: usize },
    /// Two `Default` rows for one extension: the first wins (R6 allows one).
    DuplicateDefault { extension: String },
    /// Two `Override` rows for one part name: the first wins (R6 allows one).
    DuplicateOverride { part_name: String },
    /// A `Default` or `Override` row without both of its attributes: skipped.
    IncompleteContentTypeEntry { element: String },
    /// A `Relationship` without `Id`, `Type` or `Target`: skipped. `part` is the relationships part.
    IncompleteRelationship { part: String },
    /// Two relationships in one part with the same `Id` (R11 requires unique): both are kept, and a lookup by `Id` finds the first.
    DuplicateRelationshipId { part: String, id: String },
}

impl fmt::Display for Warning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Warning::BytesAfterEndRecord { count } => write!(f, "{count} bytes after the end record of the archive were ignored"),
            Warning::DirectoryItem { name } => write!(f, "directory item \"{}\" is not a part and was dropped", name.escape_debug()),
            Warning::MoreDirectoryItems { count } => write!(f, "{count} more directory items are not parts and were dropped"),
            Warning::OrphanRelationships { part } => {
                write!(f, "\"{}\" holds relationships for a part that is not in the package", part.escape_debug())
            }
            Warning::MoreOrphanRelationships { count } => {
                write!(f, "{count} more relationships parts are for parts that are not in the package")
            }
            Warning::DuplicateDefault { extension } => {
                write!(f, "two Default rows for extension \"{}\": the first is used", extension.escape_debug())
            }
            Warning::DuplicateOverride { part_name } => write!(f, "two Override rows for \"{}\": the first is used", part_name.escape_debug()),
            Warning::IncompleteContentTypeEntry { element } => {
                write!(f, "a {} row of [Content_Types].xml lacks an attribute and was skipped", element.escape_debug())
            }
            Warning::IncompleteRelationship { part } => {
                write!(f, "a relationship in \"{}\" lacks Id, Type or Target and was skipped", part.escape_debug())
            }
            Warning::DuplicateRelationshipId { part, id } => {
                write!(f, "\"{}\" has two relationships with Id \"{}\"", part.escape_debug(), id.escape_debug())
            }
        }
    }
}
