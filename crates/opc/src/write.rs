//! Writing a package: the ZIP container, from parts and a content types stream.
//!
//! [`PackageWriter`] is the other half of [`crate::Package`]. It is given the bytes of `[Content_Types].xml` first, which it writes as
//! the first item, and then the parts in the order they should have. It writes each part as it is added, so nothing is held twice.
//!
//! **What it promises**, each of which a test reads back with the reader of this crate:
//!
//! * **Only legal names and methods**. A part name is checked by the rules the reader applies: the name, as an item name, has to
//!   come back from the reader as the same part. A name the reader would refuse, two names that compare equal, and a name that is a path
//!   prefix of another are errors. The item name is the part name without its slash, with every byte outside ASCII, and every byte
//!   that is not allowed in a part name, written as `%XX`.
//! * **A type for every part**. A part the content types stream gives no type to is an error and not a package a reader would
//!   take for generic XML.
//! * **Local and central headers agree**: name, flags, method, version, time, date, checksum and both sizes. No data descriptor, no
//!   extra field, no comment, no encryption. Methods 0 and 8 only. The time is the first of January 1980, midnight, the earliest a ZIP
//!   can hold, so the same parts always give the same bytes (times are not content).
//! * **No ZIP64.** The format needs it past 4 GiB or 65,535 items. This crate's limits ([`crate::limits`]) read at most 50,000 parts and
//!   1 GiB of them, so a package the writer cannot hold could not be read back: it is [`WriteError::TooLarge`] or
//!   [`WriteError::TooMany`], not a file of the wrong shape.
//!
//! The ZIP container itself is written by the container crate's `ArchiveWriter`, which keeps each of the last three promises; what is
//! here is OPC: the names, the content types and the choice of method.
//!
//! **Which method**: `.png`, `.jpg`, `.jpeg`, `.gif` and the extensions of packages that are embedded whole are stored, as the
//! sample files R28 counts store them, and everything else is deflated; a part that deflate does not make smaller is stored. A part over 4 MiB is
//! deflated at the fast level, since a person waits for a save and the size of the file is not content.

use std::collections::HashSet;
use std::fmt;

use diacraft_container::ContainerError;
use diacraft_container::zip::{Add, ArchiveWriter, Method, Pack, Workers};

use crate::content_types::{self, CONTENT_TYPES_ITEM, ContentTypes};
use crate::error::OpcError;
use crate::limits::{Limits, MAX_PART_NAME_BYTES};
use crate::name::{fold, from_item_name};

/// Why a package could not be written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteError {
    /// A part name the reader would refuse. Class `invalid_part_name`.
    InvalidPartName { name: String },
    /// Two part names that compare equal. Class `duplicate_part`.
    DuplicatePart { name: String },
    /// A part name that is a path prefix of another part's. Class `part_name_prefix`.
    PartNamePrefix { name: String },
    /// A part the content types stream gives no type. Class `missing_content_type`.
    NoContentType { name: String },
    /// A content types stream that does not read. Class `xml_malformed`.
    BadContentTypes(OpcError),
    /// More than 65,535 items: past what a plain ZIP end record holds. Class `too_many_parts`.
    TooMany,
    /// A part or the archive past 4 GiB. Class `package_too_large`.
    TooLarge,
    /// Memory for the archive could not be had. Class `out_of_memory`.
    OutOfMemory,
    /// The compressor failed. Class `zip_corrupt`.
    Compress,
}

impl WriteError {
    /// The class word, the spec's where it has one.
    pub fn class(&self) -> &'static str {
        match self {
            WriteError::InvalidPartName { .. } => "invalid_part_name",
            WriteError::DuplicatePart { .. } => "duplicate_part",
            WriteError::PartNamePrefix { .. } => "part_name_prefix",
            WriteError::NoContentType { .. } => "missing_content_type",
            WriteError::BadContentTypes(e) => e.class(),
            WriteError::TooMany => "too_many_parts",
            WriteError::TooLarge => "package_too_large",
            WriteError::OutOfMemory => "out_of_memory",
            WriteError::Compress => "zip_corrupt",
        }
    }
}

impl fmt::Display for WriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WriteError::InvalidPartName { name } => write!(f, "\"{}\" cannot be the name of a part of a package", name.escape_debug()),
            WriteError::DuplicatePart { name } => write!(f, "two parts are called \"{}\", or names that differ only in case", name.escape_debug()),
            WriteError::PartNamePrefix { name } => write!(f, "the part \"{}\" is also the folder of another part", name.escape_debug()),
            WriteError::NoContentType { name } => write!(f, "the part \"{}\" has no content type", name.escape_debug()),
            WriteError::BadContentTypes(e) => write!(f, "the content types do not read: {e}"),
            WriteError::TooMany => write!(f, "the package would hold more than 65,535 parts"),
            WriteError::TooLarge => write!(f, "the package would be larger than 4 GiB"),
            WriteError::OutOfMemory => write!(f, "there is not enough memory to write the package"),
            WriteError::Compress => write!(f, "a part could not be compressed"),
        }
    }
}

impl std::error::Error for WriteError {}

/// Extensions of parts that are stored and not deflated: pictures that are compressed already, and packages embedded whole.
const STORED_EXTENSIONS: [&str; 14] = ["png", "jpg", "jpeg", "gif", "vsdx", "vsdm", "vssx", "vssm", "vstx", "vstm", "docx", "xlsx", "pptx", "zip"];

/// A part that passed the checks of [`PackageWriter::add_with`], and what it is written as.
struct Planned {
    /// The name of its item.
    item: String,
    /// Its folded name, and the folded name of every folder above it.
    folded: String,
    folders: Vec<String>,
    /// Whether it is deflated (stored all the same when that does not make it smaller) and not stored.
    deflated: bool,
}

/// A package being written. See the [module documentation](self).
pub struct PackageWriter {
    archive: ArchiveWriter,
    types: ContentTypes,
    /// The folded names of the parts added, for the duplicate and prefix rules.
    names: HashSet<String>,
    /// The folded name of every folder above a part added: a part may not have one of these as its name.
    folders: HashSet<String>,
}

impl PackageWriter {
    /// A package whose first item is `content_types`, the bytes of `[Content_Types].xml`. They are read here, within the default limits: a
    /// stream that does not read, or is not `Types`, is an error and nothing is written.
    pub fn new(content_types: &[u8]) -> Result<PackageWriter, WriteError> {
        let (types, _) = content_types::parse(content_types, &Limits::default()).map_err(WriteError::BadContentTypes)?;
        let mut writer = PackageWriter { archive: ArchiveWriter::new(), types, names: HashSet::new(), folders: HashSet::new() };
        writer.archive.add_smallest(CONTENT_TYPES_ITEM, content_types).map_err(|e| from_archive(e, CONTENT_TYPES_ITEM))?;
        Ok(writer)
    }

    /// The type the stream gives the part named `name`.
    pub fn content_type(&self, name: &str) -> Option<&str> {
        self.types.lookup(name)
    }

    /// Adds the part `name` (with its leading slash, decoded: `/visio/media/café.png`) with these bytes, stored or deflated by its extension.
    pub fn add(&mut self, name: &str, data: &[u8]) -> Result<(), WriteError> {
        self.add_with(name, data, None)
    }

    /// As [`PackageWriter::add`], with the method given: `Some(false)` stores the part and `Some(true)` deflates it (it is stored all the
    /// same if deflate does not make it smaller).
    pub fn add_with(&mut self, name: &str, data: &[u8], deflate: Option<bool>) -> Result<(), WriteError> {
        let planned = self.plan(name, deflate)?;
        // Deflated, but stored all the same when deflate does not make it smaller; or stored.
        let written = if planned.deflated {
            self.archive.add_smallest(&planned.item, data).map(|_| ())
        } else {
            self.archive.add(&planned.item, data, Method::Stored)
        };
        written.map_err(|e| from_archive(e, name))?;
        self.names.insert(planned.folded);
        self.folders.extend(planned.folders);
        Ok(())
    }

    /// Adds every part of `parts` (name and bytes), in order, as [`PackageWriter::add`] adds each, **with the compression made on more than one thread when `workers` allows**
    /// ([`ArchiveWriter::add_many`]): the package is byte for byte the one that adding them one after another makes, at any number of threads. A part refused is refused in its place
    /// as it is one after another: the parts before it are written, the error is the first of the sequence, and nothing after it is.
    pub fn add_all(&mut self, parts: &[(&str, &[u8])], workers: Workers) -> Result<(), WriteError> {
        let mut planned: Vec<Planned> = Vec::new();
        planned.try_reserve_exact(parts.len()).map_err(|_| WriteError::OutOfMemory)?;
        let mut refusal: Option<WriteError> = None;
        for (name, _) in parts {
            match self.plan(name, None) {
                Ok(next) => {
                    // The names of the parts of this call count at once, for the parts after them: the writer is not used again after an error.
                    self.names.insert(next.folded.clone());
                    self.folders.extend(next.folders.iter().cloned());
                    planned.push(next);
                }
                Err(e) => {
                    refusal = Some(e);
                    break;
                }
            }
        }
        let adds: Vec<Add<'_>> = planned
            .iter()
            .zip(parts)
            .map(|(p, (_, data))| Add { name: &p.item, data, how: if p.deflated { Pack::Smallest } else { Pack::Stored } })
            .collect();
        let before = self.archive.len();
        if let Err(e) = self.archive.add_many(&adds, workers) {
            // The entries written are the ones before the one that failed.
            let at = self.archive.len().saturating_sub(before);
            return Err(from_archive(e, parts.get(at).map_or("", |(name, _)| name)));
        }
        refusal.map_or(Ok(()), Err)
    }

    /// The checks of a part that is added, and what it is written as: nothing is written, nor remembered.
    fn plan(&self, name: &str, deflate: Option<bool>) -> Result<Planned, WriteError> {
        let invalid = || WriteError::InvalidPartName { name: name.to_owned() };
        let item = item_name(name).ok_or_else(invalid)?;
        // The reader must take the item for this very part: the same name, in the same case.
        match from_item_name(&item, MAX_PART_NAME_BYTES) {
            Ok(back) if back == name => {}
            _ => return Err(invalid()),
        }
        let folded = fold(name);
        if self.names.contains(&folded) {
            return Err(WriteError::DuplicatePart { name: name.to_owned() });
        }
        if self.folders.contains(&folded) {
            return Err(WriteError::PartNamePrefix { name: name.to_owned() });
        }
        // Every folder above this part, from the name up: none may be a part.
        let mut folders = Vec::new();
        let mut path = folded.as_str();
        while let Some((above, _)) = path.rsplit_once('/') {
            if above.is_empty() {
                break;
            }
            if self.names.contains(above) {
                return Err(WriteError::PartNamePrefix { name: name.to_owned() });
            }
            folders.push(above.to_owned());
            path = above;
        }
        if self.types.lookup(name).is_none() {
            return Err(WriteError::NoContentType { name: name.to_owned() });
        }
        let deflated = match deflate {
            Some(choice) => choice,
            None => !is_stored_extension(&folded),
        };
        Ok(Planned { item, folded, folders, deflated })
    }

    /// The central directory and the end record. The archive is complete.
    pub fn finish(self) -> Result<Vec<u8>, WriteError> {
        self.archive.finish().map_err(|e| from_archive(e, ""))
    }
}

/// The error of the archive writer as this crate's (the classes are the same words). `name` is the part being written.
fn from_archive(error: ContainerError, name: &str) -> WriteError {
    match error {
        ContainerError::TooManyParts { .. } => WriteError::TooMany,
        ContainerError::PackageTooLarge { .. } => WriteError::TooLarge,
        ContainerError::OutOfMemory { .. } => WriteError::OutOfMemory,
        ContainerError::PathTraversal { .. } | ContainerError::InvalidPartName { .. } => WriteError::InvalidPartName { name: name.to_owned() },
        ContainerError::DuplicatePart { first, .. } => WriteError::DuplicatePart { name: first },
        // The writer gives none of these but the first, when the compressor fails.
        ContainerError::ZipCorrupt { .. }
        | ContainerError::NotZip
        | ContainerError::ZipRatioExceeded { .. }
        | ContainerError::PartTooLarge { .. }
        | ContainerError::UnsupportedCompression { .. }
        | ContainerError::EncryptedPackage { .. }
        | ContainerError::Xml { .. } => WriteError::Compress,
    }
}

fn is_stored_extension(folded: &str) -> bool {
    let last = folded.rsplit('/').next().unwrap_or("");
    last.rsplit_once('.').is_some_and(|(_, extension)| STORED_EXTENSIONS.contains(&extension))
}

/// The ZIP item name of a part name: without the leading slash, and with every byte that a part name may not hold, or that is not
/// ASCII, as `%XX`. `None` for a name with no leading slash.
///
/// What a part name keeps as it is: letters, digits, `- . _ ~` (unreserved), `/`, and `! $ & ' ( ) * + , ; = : @` (the other `pchar`s).
/// A `%` is written `%25`, so that a name that holds one comes back as it went.
pub fn item_name(part: &str) -> Option<String> {
    let rest = part.strip_prefix('/')?;
    let mut item = String::with_capacity(rest.len());
    for byte in rest.bytes() {
        let plain = byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'-' | b'.' | b'_' | b'~' | b'/' | b'!' | b'$' | b'&' | b'\'' | b'(' | b')' | b'*' | b'+' | b',' | b';' | b'=' | b':' | b'@'
            );
        if plain {
            item.push(byte as char);
        } else {
            item.push_str(&format!("%{byte:02X}"));
        }
    }
    Some(item)
}
