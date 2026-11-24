//! The objects a VSDX file holds once, as embeddings (the mirror of the `.odg` side).
//!
//! A drawing of an `.odg` (or of a `.diacraft` file made from one) that holds an object frame is written to a VSDX type from the model, and **the object goes in as a real embedding**: the file's
//! `oleObject` part, which the carrier of DiaCraft's data then leaves out of what rides (`diacraft_vsdx::Written::natively`). The drawing keeps the part the embedding was written from (`Object 1`, the
//! `.odg`'s own file of the object), and the file's baseline holds the model's parts. A Save into the file is a splice, which judges what rides by comparing the drawing with the baseline: an *edit*
//! (a shape moved) rewrites DiaCraft's part from the drawing, and **the object's file, which the drawing holds and the file does not carry, rode along a second time** beside the embedding.
//!
//! * [`without_parts`]: the drawing, and the baseline it is compared with, **as the file holds them**: without the parts of the objects that went natively. The save that wrote the file says which
//!   they were; `OpenFile` keeps the names, and every save into the file leaves them out of what the writer is given, as the model writer left them out of what rides. The drawing keeps the parts: a
//!   `.diacraft` file saved from it holds the object.

use std::borrow::Cow;
use std::collections::HashSet;
use std::sync::Arc;

use diacraft_doc::{Document, Format, SharedVec};

/// Whether `doc` holds a part of the other format with one of these names: the walk that tells a drawing which needs no copy.
fn holds_any(doc: &Document, names: &HashSet<&str>) -> bool {
    doc.parts.iter().any(|part| part.format != Format::Vsdx && names.contains(&*part.name))
}

/// `doc` without the parts of the other format that `names` call, which a VSDX file holds as embeddings (`OpenFile::natively`). `doc` itself, uncopied, when it holds none of them, or when `names` is
/// empty (the file was read from the disk, or wrote no embedding). A copy shares every page and master with `doc`. **Only parts of the other format**: a VSDX part of the same name is the file's own.
pub(crate) fn without_parts<'d>(doc: Cow<'d, Document>, names: &[Arc<str>]) -> Cow<'d, Document> {
    if names.is_empty() {
        return doc;
    }
    let named: HashSet<&str> = names.iter().map(|name| &**name).collect();
    if !holds_any(&doc, &named) {
        return doc;
    }
    let mut copy = doc.into_owned();
    let kept: Vec<_> = copy.parts.iter().filter(|part| part.format == Format::Vsdx || !named.contains(&*part.name)).cloned().collect();
    copy.parts = SharedVec::from_vec(kept);
    Cow::Owned(copy)
}

/// What the file holds natively once a save into it is written: the names the file held before, and the ones this save wrote as embeddings, each once and in the order they were found.
pub(crate) fn union(before: &[Arc<str>], now: &[Arc<str>]) -> Arc<[Arc<str>]> {
    let mut seen: HashSet<&str> = HashSet::new();
    before.iter().chain(now).filter(|name| seen.insert(&***name)).cloned().collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use diacraft_doc::{Part, PartRole, RawAttrs};

    use super::*;

    fn part(format: Format, name: &str, bytes: &[u8]) -> Arc<Part> {
        Arc::new(Part {
            format,
            name: Arc::from(name),
            media_type: Arc::from("application/octet-stream"),
            role: PartRole::Other,
            manifest: RawAttrs::new(),
            rels: Arc::new(Vec::new()),
            bytes: Arc::from(bytes),
        })
    }

    fn holding(parts: Vec<Arc<Part>>) -> Document {
        let mut doc = Document::empty();
        doc.parts = SharedVec::from_vec(parts);
        doc
    }

    fn names_of(doc: &Document) -> Vec<String> {
        doc.parts.iter().map(|p| p.name.to_string()).collect()
    }

    fn named(names: &[&str]) -> Vec<Arc<str>> {
        names.iter().map(|n| Arc::from(*n)).collect()
    }

    #[test]
    fn the_parts_of_the_other_format_that_went_natively_are_taken_out_and_nothing_else() {
        let doc = holding(vec![
            part(Format::Odf, "Object 1", b"a"),
            part(Format::Odf, "Basic/Standard/Module1.xml", b"m"),
            part(Format::Vsdx, "Object 1", b"v"),
            part(Format::Odf, "Object 2", b"b"),
        ]);
        let out = without_parts(Cow::Borrowed(&doc), &named(&["Object 1", "Object 3"]));
        assert_eq!(
            names_of(&out),
            ["Basic/Standard/Module1.xml", "Object 1", "Object 2"],
            "the ODF part of the name goes, and the VSDX part of the same name is the file's own"
        );
        assert!(out.parts.iter().any(|p| p.format == Format::Vsdx && &*p.name == "Object 1"));
        assert_eq!(names_of(&doc).len(), 4, "the drawing the caller holds keeps them");
    }

    #[test]
    fn a_drawing_that_holds_none_of_them_is_not_copied() {
        let doc = holding(vec![part(Format::Odf, "Object 2", b"b"), part(Format::Vsdx, "Object 1", b"v")]);
        assert!(
            matches!(without_parts(Cow::Borrowed(&doc), &named(&["Object 1"])), Cow::Borrowed(_)),
            "the only part of that name is the file's own"
        );
        assert!(matches!(without_parts(Cow::Borrowed(&doc), &[]), Cow::Borrowed(_)), "no names, no walk");
        let empty = Document::empty();
        assert!(matches!(without_parts(Cow::Borrowed(&empty), &named(&["Object 1"])), Cow::Borrowed(_)));
    }

    #[test]
    fn a_copy_that_is_given_is_taken_out_of_in_place_and_not_copied_again() {
        let doc = holding(vec![part(Format::Odf, "Object 1", b"a")]);
        let out = without_parts(Cow::Owned(doc), &named(&["Object 1"]));
        assert!(out.parts.is_empty());
        assert!(matches!(out, Cow::Owned(_)));
    }

    #[test]
    fn the_names_of_two_saves_are_joined_without_repeating_one() {
        let joined = union(&named(&["Object 1", "Object 2"]), &named(&["Object 2", "Object 3"]));
        assert_eq!(joined.iter().map(|n| &**n).collect::<Vec<_>>(), ["Object 1", "Object 2", "Object 3"]);
        assert!(union(&[], &[]).is_empty());
    }
}
