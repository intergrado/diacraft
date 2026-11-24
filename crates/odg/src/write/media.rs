//! The media type of a save: a package is a drawing (`.odg`) or a drawing
//! template (`.otg`) by the media type it states, and states it twice, in `mimetype` and in the manifest's root row. A save keeps the kind the
//! package says it is, unless [`WriteOptions::variant`](super::WriteOptions::variant) asks for the other: then **those two statements change
//! and nothing else does**, the one text each, put where the file has it.
//!
//! **A save that asks for a kind always states it**: when the kind changes, in both places; and when it is the kind the package
//! says it is already but its `mimetype` is not as R2 asks (absent, not first, deflated, with an extra field, or at odds with the root row, which
//! wins), in `mimetype`, written as R2 asks, even when the model is not edited (a census file whose `mimetype` said template under a root row
//! that said drawing was copied with the wrong `mimetype` by a save that asked for a drawing). A save that asks for nothing copies an irregular
//! `mimetype` as it is when the model is unedited, and repairs it when it is edited.
//!
//! * `mimetype` is a changed entry like any other: it keeps its place, method, date and every other header field, and only its checksum,
//!   its sizes and its bytes are new. One that is not as R2 asks is written as R2 asks, first and stored.
//! * The manifest is edited by rows, as R11 says: the root row's `manifest:media-type` value is replaced where it lies, in the quote and the
//!   spelling the file has, and no other byte of the manifest moves ([`Manifest::state_root_media`]). The same splice adds the rows a save adds.
//!   A root row that already says the kind is not touched.
//! * A save that changes the kind and cannot state it, for a manifest with no root row or one whose root row has no media type, is refused before
//!   the file is read ([`WriteError::MediaType`]); the rule is that the root row is where the package's own document is stated.
//! * A signed package is changed by it, as both statements are files its document signature covers, so the save says so.
//!
//! The model is not touched, so a retype of a drawing that was not edited writes no part, scans no part and makes one edit ([`retyped_copy`]).

use std::collections::HashSet;

use super::archive::{self, Changes};
use super::manifest::Manifest;
use super::{SaveNote, WriteError, WriteWork};
use crate::Variant;
use crate::limits::Limits;
use crate::package::{MANIFEST_PATH, MIMETYPE_PATH, Opened};

/// What a save that asked for a kind writes: the kind, and whether the manifest's root row says it in the text of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Retype {
    pub variant: Variant,
    /// The root row's media type is replaced: the row says another kind, or says nothing (an empty media type states nothing).
    pub root: bool,
}

/// What a save that asked for `asked` writes, or `None` when it writes nothing of the kind: nothing was asked, or what was asked is the kind the
/// package says it is (by its root row, else its `mimetype`) and its `mimetype` is as R2 asks.
///
/// * **The kind changes** (the package says another kind, or nothing, or two statements disagree and the root row, which wins, says another kind):
///   both statements are written. Refused with [`WriteError::MediaType`] when the package has no root row to state it in, or a root row with no
///   media type attribute: the save would leave one of the two places unchanged. An empty `manifest:media-type` is given the type.
/// * **The kind is the package's own and its `mimetype` is not as R2 asks**: `mimetype` is written as R2 asks, and the root row is
///   written only when it says nothing (an empty media type); a package with no root row is no reason to refuse, since nothing changes there.
pub(crate) fn wanted(opened: &Opened<'_>, asked: Option<Variant>) -> Result<Option<Retype>, WriteError> {
    let Some(variant) = asked else { return Ok(None) };
    let says = opened.media_type.as_deref().and_then(Variant::from_media_type);
    let root = opened.manifest.root();
    if says != Some(variant) {
        return match root {
            None => Err(WriteError::MediaType(
                "the manifest has no root entry (a row for the package itself, with the full path /) to state the media type in",
            )),
            Some(root) if root.media_type.is_none() => Err(WriteError::MediaType("the manifest's root entry has no media type to change")),
            Some(_) => Ok(Some(Retype { variant, root: true })),
        };
    }
    if opened.mimetype_regular {
        return Ok(None);
    }
    let differs = root.and_then(|r| r.media_type.as_deref()).is_some_and(|media| media != variant.media_type());
    Ok(Some(Retype { variant, root: differs }))
}

/// Puts the `mimetype` of `variant` into `changes`: the entry's own bytes replaced when it is as R2 asks, so that its headers are kept, and
/// `mimetype` written anew, first and stored, when it is not.
pub(crate) fn state_mimetype(opened: &Opened<'_>, variant: Variant, changes: &mut Changes) {
    let media = variant.media_type();
    if opened.mimetype_regular {
        changes.replace.insert(MIMETYPE_PATH.to_owned(), media.as_bytes().to_vec());
    } else {
        changes.mimetype = Some(media.to_owned());
    }
}

/// The package of `opened` with what `retype` says written and every other entry copied as it is, and what the save says: an unedited drawing saved
/// as the other kind, or with a `mimetype` as R2 asks.
pub(crate) fn retyped_copy(
    opened: &mut Opened<'_>,
    retype: Retype,
    limits: &Limits,
    work: &mut WriteWork,
) -> Result<(Vec<u8>, Vec<SaveNote>), WriteError> {
    let mut changes = Changes::default();
    state_mimetype(opened, retype.variant, &mut changes);
    if retype.root {
        let bytes = std::mem::take(&mut opened.manifest_bytes);
        let mut manifest = Manifest::scan(&bytes, limits, work)?;
        manifest.state_root_media(retype.variant.media_type())?;
        changes.replace.insert(MANIFEST_PATH.to_owned(), manifest.edit(&HashSet::new(), &[], limits, work)?);
        opened.manifest_bytes = bytes;
    }
    let made = archive::assemble(opened, &changes, limits, work)?;
    check_made(&made, retype, limits, work)?;
    let notes = if opened.signed { vec![SaveNote::SignatureStale] } else { Vec::new() };
    Ok((made, notes))
}

/// Checks the package a save made: it opens, says it is the kind that was asked for, its `mimetype` is as R2 asks and agrees with the root row (a
/// regular `mimetype` is one that does), and the root row says the kind when the save wrote it, so that a program that tells the package by its
/// first bytes tells the same kind as one that reads the manifest.
pub(crate) fn check_made(bytes: &[u8], retype: Retype, limits: &Limits, work: &mut WriteWork) -> Result<(), WriteError> {
    let made = crate::package::open(bytes, limits, &mut work.package).map_err(|e| WriteError::SelfCheck(format!("{e} ({})", e.class())))?;
    let media = retype.variant.media_type();
    let root = made.manifest.root().and_then(|r| r.media_type.as_deref());
    if made.media_type.as_deref() != Some(media) || !made.mimetype_regular || (retype.root && root != Some(media)) {
        return Err(WriteError::SelfCheck(
            "the package made does not state the media type that was asked for, in `mimetype` and in the manifest's root entry".to_owned(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use diacraft_container::Work;
    use diacraft_container::zip::{ArchiveWriter, Method};

    use super::*;
    use crate::package::{DRAWING_MEDIA_TYPE, TEMPLATE_MEDIA_TYPE};

    const URI: &str = "urn:oasis:names:tc:opendocument:xmlns:manifest:1.0";

    /// A package of a manifest whose root row says `root` (none when `None`) and of a `mimetype` that holds `mimetype` by `method`, first in the archive or
    /// after `content.xml`.
    fn package(root: Option<&str>, mimetype: Option<(&str, Method)>, first: bool) -> Vec<u8> {
        let row = root.map(|r| format!("<manifest:file-entry manifest:full-path=\"/\" manifest:media-type=\"{r}\"/>")).unwrap_or_default();
        let manifest = format!("<manifest:manifest xmlns:manifest=\"{URI}\">{row}</manifest:manifest>");
        let mut w = ArchiveWriter::new();
        let put = |w: &mut ArchiveWriter| {
            if let Some((media, method)) = mimetype {
                w.add("mimetype", media.as_bytes(), method).unwrap();
            }
        };
        if first {
            put(&mut w);
        }
        w.add("content.xml", b"<a/>", Method::Deflated).unwrap();
        if !first {
            put(&mut w);
        }
        w.add(MANIFEST_PATH, manifest.as_bytes(), Method::Deflated).unwrap();
        w.finish().unwrap()
    }

    fn retype(variant: Variant, root: bool) -> Retype {
        Retype { variant, root }
    }

    fn checked(bytes: &[u8], retype: Retype) -> Result<(), WriteError> {
        check_made(bytes, retype, &Limits::default(), &mut WriteWork::default())
    }

    fn opened(bytes: &[u8]) -> Opened<'_> {
        match crate::package::open(bytes, &Limits::default(), &mut Work::default()) {
            Ok(opened) => opened,
            Err(e) => panic!("the package did not open: {e}"),
        }
    }

    #[test]
    fn a_package_made_is_checked_for_the_kind_in_both_places_and_for_a_mimetype_as_asks() {
        let (d, t) = (DRAWING_MEDIA_TYPE, TEMPLATE_MEDIA_TYPE);
        let stored = |m| Some((m, Method::Stored));
        // Both say it and `mimetype` is first and stored: the kind it says is the kind it is.
        assert!(checked(&package(Some(d), stored(d), true), retype(Variant::Drawing, true)).is_ok());
        assert!(checked(&package(Some(t), stored(t), true), retype(Variant::Template, true)).is_ok());
        // Each fault alone is one the check finds: the root row says another kind, with `mimetype` saying the asked one or the other; the root row says
        // nothing; there is no root row; `mimetype` says another kind; it is absent, deflated, or not first.
        for (what, bytes) in [
            ("root row says another kind", package(Some(t), stored(d), true)),
            ("both say another kind", package(Some(t), stored(t), true)),
            ("the root row says nothing", package(Some(""), stored(d), true)),
            ("no root row", package(None, stored(d), true)),
            ("mimetype says another kind", package(Some(d), stored(t), true)),
            ("mimetype absent", package(Some(d), None, true)),
            ("mimetype deflated", package(Some(d), Some((d, Method::Deflated)), true)),
            ("mimetype not first", package(Some(d), stored(d), false)),
        ] {
            let refused = checked(&bytes, retype(Variant::Drawing, true));
            assert!(matches!(&refused, Err(WriteError::SelfCheck(why)) if why.contains("media type")), "{what}: {refused:?}");
        }
        // A save that did not write the root row (its kind was the package's own) asks nothing of the row: none at all, or one that says nothing, is
        // a package that says the kind by `mimetype`. What it does ask: `mimetype` as R2 asks, saying the kind.
        for bytes in [package(None, stored(d), true), package(Some(""), stored(d), true), package(Some(d), stored(d), true)] {
            assert!(checked(&bytes, retype(Variant::Drawing, false)).is_ok());
        }
        for (what, bytes) in [
            ("says another kind", package(Some(t), stored(t), true)),
            ("mimetype absent", package(Some(d), None, true)),
            ("mimetype not first", package(Some(d), stored(d), false)),
            ("mimetype says another kind", package(None, stored(t), true)),
        ] {
            assert!(matches!(checked(&bytes, retype(Variant::Drawing, false)), Err(WriteError::SelfCheck(_))), "{what}");
        }
        // A package that does not open is the open's error, as a self-check's.
        assert!(matches!(checked(b"not a package", retype(Variant::Drawing, true)), Err(WriteError::SelfCheck(why)) if why.contains("not_zip")));
    }

    #[test]
    fn the_kind_a_save_states_is_chosen_from_what_the_package_says_and_where_it_can_state_it() {
        let (d, t) = (DRAWING_MEDIA_TYPE, TEMPLATE_MEDIA_TYPE);
        let stored = |m| Some((m, Method::Stored));
        let wanted_of = |bytes: &[u8], asked| wanted(&opened(bytes), asked);
        let template = package(Some(t), stored(t), true);
        // Nothing asked, or what the package says with a `mimetype` as R2 asks: nothing is written. The other kind: both statements are.
        assert_eq!(wanted_of(&template, None).unwrap(), None);
        assert_eq!(wanted_of(&template, Some(Variant::Template)).unwrap(), None);
        assert_eq!(wanted_of(&template, Some(Variant::Drawing)).unwrap(), Some(retype(Variant::Drawing, true)));
        // The root row wins where the two disagree: this says a template, so a drawing asked for is stated in both, and a template, which it
        // says already, is stated in `mimetype` alone, the root row being right.
        let disagree = package(Some(t), stored(d), true);
        assert_eq!(wanted_of(&disagree, Some(Variant::Template)).unwrap(), Some(retype(Variant::Template, false)));
        assert_eq!(wanted_of(&disagree, Some(Variant::Drawing)).unwrap(), Some(retype(Variant::Drawing, true)));
        assert_eq!(wanted_of(&disagree, None).unwrap(), None, "nothing asked, nothing written of the kind");
        // A `mimetype` that is not as R2 asks under a root row that says the kind: written as R2 asks, whatever is wrong with it.
        for bad in [package(Some(t), None, true), package(Some(t), Some((t, Method::Deflated)), true), package(Some(t), stored(t), false)] {
            assert_eq!(wanted_of(&bad, Some(Variant::Template)).unwrap(), Some(retype(Variant::Template, false)));
            assert_eq!(wanted_of(&bad, None).unwrap(), None);
        }
        // A package that says nothing (an empty root row, no `mimetype`) is given what was asked, in both.
        let nothing = package(Some(""), None, true);
        assert_eq!(wanted_of(&nothing, Some(Variant::Template)).unwrap(), Some(retype(Variant::Template, true)));
        // An empty root row beside a `mimetype` that says the kind and is not as R2 asks: the row says nothing, so it is given the kind too.
        let empty = package(Some(""), Some((t, Method::Deflated)), true);
        assert_eq!(wanted_of(&empty, Some(Variant::Template)).unwrap(), Some(retype(Variant::Template, true)));
        // No root row: nowhere to state the other kind, which is refused; the kind it says by `mimetype` alone is stated there, and no root row is
        // asked for.
        let no_root = package(None, stored(t), true);
        assert!(matches!(wanted_of(&no_root, Some(Variant::Drawing)), Err(WriteError::MediaType(why)) if why.contains("root entry")));
        assert_eq!(wanted_of(&no_root, Some(Variant::Template)).unwrap(), None);
        assert_eq!(wanted_of(&no_root, None).unwrap(), None);
        let no_root_bad = package(None, Some((t, Method::Deflated)), true);
        assert_eq!(wanted_of(&no_root_bad, Some(Variant::Template)).unwrap(), Some(retype(Variant::Template, false)));
    }

    #[test]
    fn mimetype_is_replaced_in_its_place_when_it_is_regular_and_written_anew_when_it_is_not() {
        let (d, t) = (DRAWING_MEDIA_TYPE, TEMPLATE_MEDIA_TYPE);
        let regular = package(Some(t), Some((t, Method::Stored)), true);
        let mut changes = Changes::default();
        state_mimetype(&opened(&regular), Variant::Drawing, &mut changes);
        assert_eq!(changes.replace.get("mimetype").map(Vec::as_slice), Some(d.as_bytes()));
        assert_eq!(changes.mimetype, None);
        let irregular = package(Some(t), Some((t, Method::Deflated)), true);
        let mut changes = Changes::default();
        state_mimetype(&opened(&irregular), Variant::Drawing, &mut changes);
        assert!(changes.replace.is_empty());
        assert_eq!(changes.mimetype.as_deref(), Some(d));
    }
}
