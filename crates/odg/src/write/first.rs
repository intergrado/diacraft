//! The first save of a new drawing: a package DiaCraft made for a drawing that
//! has no file yet (a built-in template, a blank page) holds no thumbnail, and a save that is no edit copies it as it is. R25 lists
//! `Thumbnails/thumbnail.png` among the parts of a new drawing, so a save that [`WriteOptions::first_save`](super::WriteOptions::first_save) says is the
//! first, and that is passed a thumbnail, adds it, and changes **nothing else of the package**:
//!
//! * the picture is a new entry, deflated and put before `META-INF/manifest.xml` when the manifest is the last entry, else at the end;
//! * the manifest gets one row for it, `image/png`, after the last, unless it lists the thumbnail already (a row for a file the package lacks stays as it
//!   is, and is not written twice);
//! * the kind the save is asked to state is stated, in `mimetype` and in the manifest's root row, as for any unedited save ([`media`]);
//! * no part is read or scanned and no `meta.xml` is touched: the generator is the package's own, since the model is not edited.
//!
//! A package that has a thumbnail keeps it, and so does a drawing the save does not call a first one: that is the caller's to say, since only it
//! knows whether the drawing has been saved before. A flat drawing has no thumbnail to write ([`super::flat`]).

use std::collections::HashSet;

use diacraft_container::zip::Method;

use super::archive::{self, Changes};
use super::manifest::Manifest;
use super::media::{self, Retype};
use super::meta::THUMBNAIL;
use super::{SaveNote, WriteError, WriteWork};
use crate::limits::Limits;
use crate::package::{MANIFEST_PATH, Opened};

/// The package of `opened` with `thumbnail` added and its manifest row, and the kind `retype` states written when there is one; every other entry is copied as
/// it is. The caller has checked the picture ([`super::meta::check_thumbnail`]) and that the package has no thumbnail entry.
pub(crate) fn copy_with_thumbnail(
    opened: &mut Opened<'_>,
    thumbnail: &[u8],
    retype: Option<Retype>,
    limits: &Limits,
    work: &mut WriteWork,
) -> Result<(Vec<u8>, Vec<SaveNote>), WriteError> {
    let mut changes = Changes::default();
    if let Some(retype) = retype {
        media::state_mimetype(opened, retype.variant, &mut changes);
    }
    changes.add.push((THUMBNAIL.to_owned(), thumbnail.to_vec(), Method::Deflated));
    // The manifest: a row for the picture unless it lists one, and the root row's text when the kind changes. One scan, one splice.
    let rows: Vec<(String, String)> =
        if opened.manifest.entry(THUMBNAIL).is_some() { Vec::new() } else { vec![(THUMBNAIL.to_owned(), "image/png".to_owned())] };
    let root_media = retype.filter(|r| r.root).map(|r| r.variant.media_type());
    if !rows.is_empty() || root_media.is_some() {
        let bytes = std::mem::take(&mut opened.manifest_bytes);
        let mut manifest = Manifest::scan(&bytes, limits, work)?;
        if let Some(media) = root_media {
            manifest.state_root_media(media)?;
        }
        changes.replace.insert(MANIFEST_PATH.to_owned(), manifest.edit(&HashSet::new(), &rows, limits, work)?);
        opened.manifest_bytes = bytes;
    }
    let made = archive::assemble(opened, &changes, limits, work)?;
    if let Some(retype) = retype {
        media::check_made(&made, retype, limits, work)?;
    }
    check_made(&made, thumbnail, limits, work)?;
    let notes = if opened.signed { vec![SaveNote::SignatureStale] } else { Vec::new() };
    Ok((made, notes))
}

/// Checks the package the save made: it opens, holds the thumbnail with the size of the bytes that were passed, and the manifest lists it (a manifest that
/// listed it twice already is the file's own, kept as it is).
fn check_made(bytes: &[u8], thumbnail: &[u8], limits: &Limits, work: &mut WriteWork) -> Result<(), WriteError> {
    let made = crate::package::open(bytes, limits, &mut work.package).map_err(|e| WriteError::SelfCheck(format!("{e} ({})", e.class())))?;
    let held = made.archive.position(THUMBNAIL).and_then(|at| made.archive.entries().get(at)).map(|e| e.size);
    let listed = made.manifest.entry(THUMBNAIL).is_some();
    if held != Some(thumbnail.len() as u64) || !listed {
        return Err(WriteError::SelfCheck("the package made does not hold the thumbnail and list it in its manifest".to_owned()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use diacraft_container::zip::{ArchiveWriter, Method};

    use super::*;
    use crate::package::DRAWING_MEDIA_TYPE;

    const PNG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

    /// A package whose manifest lists `rows` and that holds `entry` as the thumbnail, when there is one.
    fn package(rows: &[&str], entry: Option<&[u8]>) -> Vec<u8> {
        let listed: String =
            rows.iter().map(|r| format!("<manifest:file-entry manifest:full-path=\"{r}\" manifest:media-type=\"image/png\"/>")).collect();
        let manifest = format!(
            "<manifest:manifest xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\"><manifest:file-entry manifest:full-path=\"/\" manifest:media-type=\"{DRAWING_MEDIA_TYPE}\"/>{listed}</manifest:manifest>"
        );
        let mut w = ArchiveWriter::new();
        w.add("mimetype", DRAWING_MEDIA_TYPE.as_bytes(), Method::Stored).unwrap();
        w.add("content.xml", b"<a/>", Method::Deflated).unwrap();
        if let Some(bytes) = entry {
            w.add(THUMBNAIL, bytes, Method::Deflated).unwrap();
        }
        w.add(MANIFEST_PATH, manifest.as_bytes(), Method::Deflated).unwrap();
        w.finish().unwrap()
    }

    fn checked(bytes: &[u8], thumbnail: &[u8]) -> Result<(), WriteError> {
        check_made(bytes, thumbnail, &Limits::default(), &mut WriteWork::default())
    }

    /// The check of the package a first save made: the thumbnail is in it with the size it was given, and the manifest lists it; each fault alone is found, and
    /// a manifest that lists it twice is the file's own and is let be.
    #[test]
    fn the_package_a_first_save_made_is_checked_for_its_thumbnail_and_the_row_that_lists_it() {
        let listed = [THUMBNAIL];
        assert!(checked(&package(&listed, Some(&PNG)), &PNG).is_ok());
        assert!(checked(&package(&[THUMBNAIL, THUMBNAIL], Some(&PNG)), &PNG).is_ok(), "two rows are the file's own");
        for (what, bytes) in [
            ("the entry is missing", package(&listed, None)),
            ("the row is missing", package(&[], Some(&PNG))),
            ("the entry is of another size", package(&listed, Some(&PNG[..7]))),
        ] {
            let refused = checked(&bytes, &PNG);
            assert!(matches!(&refused, Err(WriteError::SelfCheck(why)) if why.contains("thumbnail")), "{what}: {refused:?}");
        }
        assert!(matches!(checked(b"not a package", &PNG), Err(WriteError::SelfCheck(why)) if why.contains("not_zip")));
    }
}
