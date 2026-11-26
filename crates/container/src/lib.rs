//! DiaCraft container: the bounded ZIP archive and the bounded XML tree that both of DiaCraft's package formats are made of. The VSDX
//! package (`diacraft-opc`, Open Packaging Conventions) and the OpenDocument package (`.odg`) are each a ZIP archive of XML parts and
//! other files, and this crate reads and writes that layer once, for both (a standalone crate at layer 0, which `opc`
//! uses through the intra-layer chain `["container", "opc"]`).
//!
//! This is the front door for hostile files, and it is standalone (no workspace dependency). It opens no file and no network connection:
//! names are keys, and nothing a name says reaches a file system.
//!
//! * [`zip::Archive`] reads an archive: the end record, the ZIP64 records and the central directory, from the public PKWARE APPNOTE with
//!   no ZIP library (the library first used spent minutes on a file of a megabyte whose tail was end-record signatures, and
//!   690 MB on a file of 64 MB of empty extra fields, before any limit applied). Each row is put to a [`zip::Judge`], the caller's rules
//!   for names (OPC's part names, or [`zip::Plain`]'s exact names for ODF), and then to this crate's own: the count, encryption (refused),
//!   the method (stored or deflate), the ratio, the caps and the total, before anything is inflated; then the layout (no two entries
//!   share a byte); then each entry is inflated when asked, within its limit, and checked against its size and checksum.
//! * [`zip::ArchiveWriter`] writes one: the method asked for, local and central headers that agree, no extra field, folders and names
//!   with spaces as given, and the same bytes for the same entries. It also copies an entry of an archive it read as it is
//!   ([`zip::ArchiveWriter::copy`]): its compressed bytes, flags, extra fields, dates and attributes, byte for byte
//!   when its headers agree, so that a package copied entry by entry comes back the same file; and [`zip::copy_account`] says, without
//!   writing anything, what such a copy rewrites and leaves out of an archive, so that a copy that is not the same file
//!   always has a stated cause.
//! * [`par`] runs tasks on worker threads and gives the results back **in the order the tasks were given** (`std::thread::scope`, no pool; a task that panics is an error in its
//!   place; the threads are made only when a task needs one and never on the web), and [`zip::ArchiveWriter::add_many`] writes the entries of an archive compressed on threads: **the archive is
//!   byte for byte the one that adding them one after another writes, at any number of threads**. How many threads work may use is the caller's to say ([`par::default_threads`] is the
//!   default of an app: the cores less one, or `DIACRAFT_THREADS`).
//! * [`xml`] is a tree that keeps what a save needs to write back unchanged (attribute order, prefixes and declarations, white space,
//!   comments and instructions), with no DTD, no entity and nothing fetched, and with depth, items and attribute sizes bounded; and the
//!   writer of such a tree ([`xml::to_bytes`]). A document of any number of children is read as a stream, one child of the root's tree at a
//!   time ([`xml::XmlText`], [`xml::Pull`] and [`xml::PullLimits`]), and written child by child ([`xml::RootWriter`]).
//!
//! * [`inflate`] inflates a slice: raw DEFLATE and zlib streams that are not entries of an archive (a page of a draw.io file, a text chunk of a PNG), within a cap on the
//!   output and a ratio of output to input, charged before each chunk is kept, with the bytes made counted in [`Work::inflated`]. Every inflate of bytes that are
//!   not an archive's entries has this one home.
//!
//! * [`canon`] is the content check: whether two packages say the same thing, under the rules of the design's section 4.3 (spacing,
//!   attribute order, prefixes and comments ignored; numbers by value where a format says; keyed siblings as sets), configured by each
//!   format's own lists.
//!
//! **Nothing panics, and memory that runs out while an entry is inflated is an error** ([`ContainerError::OutOfMemory`]), not an abort.
//! Every limit is a named constant with the rule that sets it ([`limits`]) and a field of [`Limits`]; what a read cost is counted in a
//! [`Work`], in the units each loop walks, for the tests that hold a cost to a count. Every error has a [`class`](ContainerError::class),
//! the snake-case word, which keeps for ODF.
//!
//! The rules of the reader in full, and what each costs, are in `zipdir.rs` and `zipread.rs`, moved here from `diacraft-opc`
//! with the reading unchanged; `diacraft-opc`'s documentation keeps the measured costs of its package reads.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use std::fmt;

pub mod canon;
pub mod inflate;
pub mod limits;
pub mod par;
pub mod spare;
pub mod work;
pub mod xml;
mod xml_out;
pub mod xml_scan;
mod zipdir;
mod zipread;
mod zipwrite;

#[cfg(kani)]
mod kani_proofs;

pub use limits::{Limits, XmlLimits};
pub use work::Work;
pub use xml::XmlError;

/// The ZIP layer: reading an archive and writing one.
pub mod zip {
    pub use crate::zipread::{Archive, Entry, IntoDone, IntoJob, Judge, Method, Plain, Verdict, directory_rows};
    pub use crate::zipwrite::{Add, ArchiveWriter, Copied, CopyAccount, Dropped, Pack, Packed, Workers, copy_account, copy_account_counted, pack};
}

/// Why an archive could not be read or written. Every error has a [`class`](ContainerError::class): the snake-case word of the error
/// table in where the spec names one, so that a harness compares classes and not wording; where
/// it has none, the class is the one the VSDX reader has always given, and the variant says so.
///
/// Names and messages come from the file, which is hostile: [`Display`](fmt::Display) escapes them, so an error shown in a log or a
/// dialog cannot carry control characters or terminal escapes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContainerError {
    /// The bytes are not a ZIP archive. Class `not_zip`.
    NotZip,
    /// The central directory cannot be read, entries overlap or lie about where they are, an entry fails to inflate, or its size or
    /// checksum is not what the archive says. Class `zip_corrupt`.
    ZipCorrupt { detail: String },
    /// A name with `..`, a leading slash, a backslash or a drive letter. Class `path_traversal`.
    PathTraversal { name: String },
    /// A name that is not a legal name for another reason: empty, longer than the limit, or a Unicode path that disagrees with the name
    /// as written. Class `invalid_part_name` (the spec's table has no word for these).
    InvalidPartName { name: String, reason: &'static str },
    /// An entry inflates past the ratio limit, by what its header says or by the bytes it really holds. Class
    /// `zip_ratio_exceeded`.
    ZipRatioExceeded { name: String, limit: u64 },
    /// An entry larger than its cap, by its header or in fact. Class `part_too_large`.
    PartTooLarge { name: String, limit: u64 },
    /// All parts together larger than the package limit, by their headers or in fact, or an archive to write past 4 GiB. Class
    /// `package_too_large` (the spec's table has no word for the third cap).
    PackageTooLarge { limit: u64 },
    /// More parts than the limit, or more entries than an archive to write can count. Class `too_many_parts`.
    TooManyParts { limit: usize },
    /// A compression method other than stored (0) or deflate (8). Class `unsupported_compression`.
    UnsupportedCompression { name: String, method: String },
    /// An entry is encrypted. Class `encrypted_package`.
    EncryptedPackage { name: String },
    /// Two entries of one name, by the judge's rule of what one name is. Class `duplicate_part`.
    DuplicatePart { first: String, second: String },
    /// An XML part the caller parses could not be read. The class is the XML one.
    Xml { part: String, error: XmlError },
    /// Memory ran out while an entry was being read or the archive written: reported, not an abort. Class `out_of_memory`.
    OutOfMemory { what: String },
}

impl ContainerError {
    /// The class word. See the variants for which come from the spec and which are the reader's own.
    pub fn class(&self) -> &'static str {
        match self {
            ContainerError::NotZip => "not_zip",
            ContainerError::ZipCorrupt { .. } => "zip_corrupt",
            ContainerError::PathTraversal { .. } => "path_traversal",
            ContainerError::InvalidPartName { .. } => "invalid_part_name",
            ContainerError::ZipRatioExceeded { .. } => "zip_ratio_exceeded",
            ContainerError::PartTooLarge { .. } => "part_too_large",
            ContainerError::PackageTooLarge { .. } => "package_too_large",
            ContainerError::TooManyParts { .. } => "too_many_parts",
            ContainerError::UnsupportedCompression { .. } => "unsupported_compression",
            ContainerError::EncryptedPackage { .. } => "encrypted_package",
            ContainerError::DuplicatePart { .. } => "duplicate_part",
            ContainerError::Xml { error, .. } => error.class(),
            ContainerError::OutOfMemory { .. } => "out_of_memory",
        }
    }
}

impl fmt::Display for ContainerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ContainerError::NotZip => write!(f, "not a ZIP archive"),
            ContainerError::ZipCorrupt { detail } => write!(f, "the ZIP archive is damaged: {}", detail.escape_debug()),
            ContainerError::PathTraversal { name } => write!(f, "item name \"{}\" would leave the package", name.escape_debug()),
            ContainerError::InvalidPartName { name, reason } => write!(f, "item name \"{}\" is not a legal name: {reason}", name.escape_debug()),
            ContainerError::ZipRatioExceeded { name, limit } => {
                write!(f, "part \"{}\" inflates more than {limit} times its size", name.escape_debug())
            }
            ContainerError::PartTooLarge { name, limit } => write!(f, "part \"{}\" is larger than {limit} bytes", name.escape_debug()),
            ContainerError::PackageTooLarge { limit } => write!(f, "the parts together are larger than {limit} bytes"),
            ContainerError::TooManyParts { limit } => write!(f, "the package has more than {limit} parts"),
            ContainerError::UnsupportedCompression { name, method } => write!(
                f,
                "part \"{}\" uses compression method {}, and only stored and deflate are allowed",
                name.escape_debug(),
                method.escape_debug()
            ),
            ContainerError::EncryptedPackage { name } => write!(f, "part \"{}\" is encrypted", name.escape_debug()),
            ContainerError::DuplicatePart { first, second } => {
                write!(f, "items \"{}\" and \"{}\" are the same name", first.escape_debug(), second.escape_debug())
            }
            ContainerError::Xml { part, error } => write!(f, "\"{}\": {error}", part.escape_debug()),
            ContainerError::OutOfMemory { what } => write!(f, "there was not enough memory for {}", what.escape_debug()),
        }
    }
}

impl std::error::Error for ContainerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ContainerError::Xml { error, .. } => Some(error),
            _ => None,
        }
    }
}
