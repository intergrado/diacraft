//! The package of a save, put together: the entries the save did not change copied as they are, in
//! the order they lie in the file; the ones it changed written under their own headers with their new bytes ([`ArchiveWriter::replace`]); the
//! ones it dropped left out; and the new ones appended before the manifest when the manifest was the last entry, at the end otherwise.
//!
//! A `mimetype` written anew goes first, stored with no extra field, in place of the one the package had wherever it lay. Nothing else
//! of the package is written but what a change says: every other entry's compressed bytes, method, flags, extra fields, date and order are
//! the original's (the copy of the container), and the directory is written anew.

use std::collections::{HashMap, HashSet};

use diacraft_container::zip::{ArchiveWriter, Method};

use super::media::Retype;
use super::{WriteError, WriteWork};
use crate::limits::Limits;
use crate::package::Opened;

/// What a save changes of the package.
#[derive(Default)]
pub(crate) struct Changes {
    /// New bytes for the entries of these names.
    pub replace: HashMap<String, Vec<u8>>,
    /// Entries to leave out.
    pub remove: HashSet<String>,
    /// Entries to add: name, bytes and method.
    pub add: Vec<(String, Vec<u8>, Method)>,
    /// The entries of new pictures (`picture.rs`), added before every other: the bytes are the model's own, shared, and nothing is copied until the archive takes them.
    pub pictures: Vec<(String, std::sync::Arc<[u8]>, Method)>,
    /// A regular `mimetype` to write first, with this media type, in place of any other.
    pub mimetype: Option<String>,
    /// What the save writes of the kind of the package: `mimetype`, and the manifest's root row when it is to say another text.
    pub retype: Option<Retype>,
}

fn package_error(e: diacraft_container::ContainerError) -> WriteError {
    WriteError::Package(e)
}

/// The package with `changes` made, entries in the order they lie in the file.
pub(crate) fn assemble(opened: &Opened<'_>, changes: &Changes, limits: &Limits, work: &mut WriteWork) -> Result<Vec<u8>, WriteError> {
    let archive = &opened.archive;
    let mut order: Vec<usize> = (0..archive.entries().len()).collect();
    order.sort_by_key(|&i| archive.entries().get(i).map_or(usize::MAX, |e| e.local_offset));
    let manifest_last = order.last() == Some(&opened.manifest_at);
    let mut writer = ArchiveWriter::new();
    if let Some(media) = &changes.mimetype {
        writer.add("mimetype", media.as_bytes(), Method::Stored).map_err(package_error)?;
    }
    let mut added = false;
    let add_all = |writer: &mut ArchiveWriter, work: &mut WriteWork| -> Result<(), WriteError> {
        for (name, bytes, method) in &changes.pictures {
            work.charge_copy(bytes.len(), limits)?;
            writer.add(name, bytes, *method).map_err(package_error)?;
        }
        for (name, bytes, method) in &changes.add {
            work.charge_copy(bytes.len(), limits)?;
            writer.add(name, bytes, *method).map_err(package_error)?;
        }
        Ok(())
    };
    for &i in &order {
        let Some(entry) = archive.entries().get(i) else { continue };
        // A folder's name ends in a slash, so the name `mimetype` is a file's.
        if changes.remove.contains(&entry.name) || (changes.mimetype.is_some() && entry.name == "mimetype") {
            continue;
        }
        if manifest_last && i == opened.manifest_at && !added {
            add_all(&mut writer, work)?;
            added = true;
        }
        match changes.replace.get(&entry.name).filter(|_| !entry.folder) {
            Some(bytes) => {
                work.charge_copy(bytes.len(), limits)?;
                writer.replace(archive, i, bytes, &mut work.package).map_err(package_error)?;
            }
            None => {
                work.charge_copy(usize::try_from(entry.compressed).unwrap_or(usize::MAX), limits)?;
                writer.copy_counted(archive, i, &mut work.package).map_err(package_error)?;
            }
        }
    }
    if !added {
        add_all(&mut writer, work)?;
    }
    writer.finish().map_err(package_error)
}

/// The package with every entry copied as it is, in the order they lie in the file: what an unedited save is.
pub(crate) fn copy_all(opened: &Opened<'_>, limits: &Limits, work: &mut WriteWork) -> Result<Vec<u8>, WriteError> {
    assemble(opened, &Changes::default(), limits, work)
}
