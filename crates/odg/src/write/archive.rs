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

/// Looks at each XML part the save changes or adds for a start tag that names an attribute twice, or declares a prefix twice (the parts are as the save made them, and none is
/// put into the package that the reader would refuse). Every writer of a package passes here (the edited save, the first save's thumbnail, the change of the kind), whatever it edited.
fn check_names(changes: &Changes, limits: &Limits, work: &mut WriteWork) -> Result<(), WriteError> {
    let is_xml = |name: &str| name.rsplit_once('.').is_some_and(|(_, extension)| extension.eq_ignore_ascii_case("xml"));
    // In the order of the names, so that the part a refusal names is the same on every run.
    let mut replaced: Vec<(&String, &Vec<u8>)> = changes.replace.iter().filter(|(name, _)| is_xml(name)).collect();
    replaced.sort_by(|a, b| a.0.cmp(b.0));
    for (name, bytes) in replaced {
        super::unique::check_part(name, bytes, limits, work)?;
    }
    for (name, bytes, _) in changes.add.iter().filter(|(name, _, _)| is_xml(name)) {
        super::unique::check_part(name, bytes, limits, work)?;
    }
    Ok(())
}

/// The package with `changes` made, entries in the order they lie in the file.
pub(crate) fn assemble(opened: &Opened<'_>, changes: &Changes, limits: &Limits, work: &mut WriteWork) -> Result<Vec<u8>, WriteError> {
    check_names(changes, limits, work)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    const TWICE: &str = r#"<a x="1" x="2"/>"#;

    fn why(r: Result<(), WriteError>) -> String {
        match r {
            Err(WriteError::SelfCheck(words)) => words,
            other => panic!("expected a refusal of the check, got {other:?}"),
        }
    }

    /// Each XML part the save changes or adds is looked through, in the order of the names, and a part that is no XML by its name is not read as markup.
    #[test]
    fn the_xml_parts_a_save_changes_or_adds_are_looked_through_and_the_others_are_not() {
        let (limits, mut work) = (Limits::default(), WriteWork::default());
        let none = Changes::default();
        assert!(check_names(&none, &limits, &mut work).is_ok());
        // A replaced part of any case of `.xml`, and an added one, with a tag that names an attribute twice, is refused, and the words say which part.
        for name in ["content.xml", "META-INF/manifest.xml", "DiaCraft/document.xml", "styles.XML"] {
            let mut changes = Changes::default();
            changes.replace.insert(name.to_owned(), TWICE.as_bytes().to_vec());
            assert!(why(check_names(&changes, &limits, &mut WriteWork::default())).contains(name), "replaced {name}");
            let mut changes = Changes::default();
            changes.add.push((name.to_owned(), TWICE.as_bytes().to_vec(), Method::Deflated));
            assert!(why(check_names(&changes, &limits, &mut WriteWork::default())).contains(name), "added {name}");
        }
        // The names are looked at in order, so the part a refusal names does not depend on the map: `a.xml` before `b.xml`.
        let mut changes = Changes::default();
        for name in ["b.xml", "c.xml", "a.xml"] {
            changes.replace.insert(name.to_owned(), TWICE.as_bytes().to_vec());
        }
        assert!(why(check_names(&changes, &limits, &mut WriteWork::default())).contains("a.xml"));
        // What is no XML by its name is not read as markup: a picture, a thumbnail, a part with no extension, a stored media type.
        let mut changes = Changes::default();
        for name in ["Thumbnails/thumbnail.png", "Pictures/image1.png", "mimetype", "README", "x.xml.bak"] {
            changes.replace.insert(name.to_owned(), TWICE.as_bytes().to_vec());
            changes.add.push((name.to_owned(), TWICE.as_bytes().to_vec(), Method::Stored));
        }
        assert!(check_names(&changes, &limits, &mut work).is_ok());
        assert_eq!(work.checked_name_bytes, 0, "none of them was looked at");
    }
}
