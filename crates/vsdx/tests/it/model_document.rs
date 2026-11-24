//! Item 1 of the full gate's sample-file report: **the font table of a document part that had none**. The model writer wrote an empty `FaceNames` for every model with no
//! face, and 26 sources of the sample files had no `FaceNames` at all: MS-VSDX asks a `FaceNames` for one `FaceName` at least, so the files written failed the schema
//! where their sources passed (class `child_missing|FaceNames`), and the element was one more typed child for the unknown elements of the part to be put back after, so one of them that
//! followed `DocumentSheet` came out before it. A file that had none is written with none while the model has no face, and so is one that had an empty one (no save writes an
//! empty table, which the schema refuses; the content check counts an empty one as none).
//!
//! Every input is built in code; the schema's check is made here in the shape of `document_shape`, as `new_drawing.rs` makes it of a new drawing's part.

use std::sync::Arc;

use diacraft_doc::{FaceName, RawAttrs};
use diacraft_opc::xml::{self, El};
use diacraft_testkit::package::{MasterSpec, PageSpec, VsdxBuilder, cell, shape};
use diacraft_vsdx::{Kind, Variant};

use crate::model_writer::{DOC, model_write, model_write_as, parts, visit};
use crate::read_ok;

/// The children of `VisioDocument` a schema knows, in the order.
const ORDER: [&str; 9] =
    ["DocumentSettings", "Colors", "FaceNames", "StyleSheets", "DocumentSheet", "EventList", "HeaderFooter", "DataTransferInfo", "PublishSettings"];

/// A foreign element after the typed children: R8 allows elements of other namespaces after `PublishSettings`.
const NOTE: &str = r#"<x:note xmlns:x="urn:example:note" k="v"/>"#;

/// The children of the document part, after the check the published schema makes of their shape: each child R8 names at most once and in the order, an element of another namespace
/// (one with a prefix) only after them, and, when `faces_needed`, a `FaceNames` that holds one `FaceName` at least, each with a `NameU`. → the local names, in order.
fn document_shape(document_xml: &[u8], faces_needed: bool) -> Vec<String> {
    let root = xml::parse(document_xml).unwrap();
    let mut at = 0;
    let mut foreign_seen = false;
    let mut names = Vec::new();
    for child in root.elements() {
        let local = child.name.rsplit(':').next().unwrap().to_owned();
        match ORDER.iter().position(|n| *n == local) {
            Some(place) if !child.name.contains(':') => {
                assert!(!foreign_seen, "{local} comes after an element of another namespace: {names:?}");
                assert!(place >= at, "{local} is out of the order: {names:?}");
                at = place + 1;
                if local == "FaceNames" && faces_needed {
                    let faces: Vec<&El> = child.elements().collect();
                    assert!(!faces.is_empty(), "an empty FaceNames fails the schema: MS-VSDX wants one FaceName at least");
                    for face in faces {
                        assert!(face.name.ends_with("FaceName") && face.attr("NameU").is_some_and(|n| !n.is_empty()), "{}", face.name);
                    }
                }
            }
            _ => {
                assert!(child.name.contains(':'), "an element R8 does not name, in the part's own namespace: {}", child.name);
                foreign_seen = true;
            }
        }
        names.push(local);
    }
    names
}

/// The children of the document part of a package, by local name.
fn children(bytes: &[u8]) -> Vec<String> {
    let root = xml::parse(parts(bytes).get(DOC).unwrap_or_else(|| panic!("no document part"))).unwrap();
    root.elements().map(|e| e.name.rsplit(':').next().unwrap().to_owned()).collect()
}

/// A package of `variant` whose document part has no `FaceNames`, and a foreign element after `DocumentSheet`.
fn without_font_table(variant: Variant) -> Vec<u8> {
    let builder = VsdxBuilder::new()
        .document_type(variant.content_type())
        .document_extra(NOTE)
        .master(MasterSpec::new(1, "Box").shapes(&shape(4, &cell("Width", "1"))));
    // A stencil's pages have no part (`model_writer.rs`, `a_stencil_is_written_as_a_stencil_with_pages_that_have_no_part`).
    let page = if variant.kind == Kind::Stencil {
        PageSpec::new(1, "P").without_part()
    } else {
        PageSpec::new(1, "P").shapes(&shape(1, &cell("PinX", "1")))
    };
    builder
        .page(page)
        .pkg()
        .map_parts(|name, bytes| {
            if name == DOC {
                let text = String::from_utf8(bytes.clone()).unwrap();
                assert!(text.contains("<FaceNames></FaceNames>"), "the builder's part changed: {text}");
                *bytes = text.replace("<FaceNames></FaceNames>", "").into_bytes();
            }
        })
        .build()
}

const VARIANTS: [Variant; 6] = [
    Variant { kind: Kind::Drawing, macro_enabled: false },
    Variant { kind: Kind::Drawing, macro_enabled: true },
    Variant { kind: Kind::Stencil, macro_enabled: false },
    Variant { kind: Kind::Stencil, macro_enabled: true },
    Variant { kind: Kind::Template, macro_enabled: false },
    Variant { kind: Kind::Template, macro_enabled: true },
];

#[test]
fn a_document_part_that_had_no_font_table_is_written_with_none_as_each_type_and_reads_back_the_same() {
    for variant in VARIANTS {
        let what = variant.extension();
        let bytes = without_font_table(variant);
        assert_eq!(children(&bytes), ["StyleSheets", "DocumentSheet", "note"], "{what}: the source");
        let (doc, _) = read_ok(&bytes);
        assert!(doc.fonts.is_empty(), "{what}");
        // As it was read: the same content as the file (the real content check), and a part of the schema's shape with no `FaceNames`.
        let written = visit(what, &bytes);
        assert_eq!(children(&written.bytes), ["StyleSheets", "DocumentSheet", "note"], "{what}: the part written as it was read");
        document_shape(parts(&written.bytes).get(DOC).unwrap(), true);
        // It reads back as the model it was written from.
        let (back, _) = read_ok(&written.bytes);
        assert!(back.fonts.is_empty(), "{what}");
        assert_eq!(back.passthrough, doc.passthrough, "{what}: the unknown elements of the part");
        assert!(back.ext == doc.ext, "{what}: what the document holds beside its fields");
        // An edited save has no face to write either.
        let edited = model_write_as(&doc, false, variant);
        assert_eq!(children(&edited.bytes), ["StyleSheets", "DocumentSheet", "note"], "{what}: the part of an edited save");
        document_shape(parts(&edited.bytes).get(DOC).unwrap(), true);
    }
}

#[test]
fn a_font_table_the_file_had_empty_is_not_written() {
    // E3 of the document spec: a reader accepts an empty `FaceNames`. **No save writes one** (found 10 files of the VSDX direction with an empty table after a trip
    // F1b): MS-VSDX wants one `FaceName` at least, and the content check counts an empty table as none, so a visit is the same content with no table.
    let bytes = VsdxBuilder::new().document_extra(NOTE).page(PageSpec::new(1, "P").shapes(&shape(1, &cell("PinX", "1")))).build();
    assert_eq!(children(&bytes), ["FaceNames", "StyleSheets", "DocumentSheet", "note"]);
    let written = visit("an empty table", &bytes);
    assert_eq!(children(&written.bytes), ["StyleSheets", "DocumentSheet", "note"]);
    document_shape(parts(&written.bytes).get(DOC).unwrap(), true);
    assert!(read_ok(&written.bytes).0.fonts.is_empty());
    assert!(
        diacraft_vsdx::compare_content(&bytes, &written.bytes, &diacraft_vsdx::Limits::default())
            .unwrap()
            .shown
            .iter()
            .all(|d| !d.path.contains("FaceNames"))
    );
}

#[test]
fn a_face_the_model_gains_is_written_in_a_table_where_the_file_had_none() {
    let bytes = without_font_table(Variant { kind: Kind::Drawing, macro_enabled: false });
    let (mut doc, _) = read_ok(&bytes);
    let face = FaceName { name_u: Arc::from("Alpha Face"), raw: RawAttrs::from_pairs([("Flags", "1")]), ext: Default::default() };
    doc.fonts = Arc::new(vec![face.clone()]);
    let written = model_write(&doc, false);
    assert_eq!(children(&written.bytes), ["FaceNames", "StyleSheets", "DocumentSheet", "note"]);
    document_shape(parts(&written.bytes).get(DOC).unwrap(), true);
    assert_eq!(read_ok(&written.bytes).0.fonts.as_ref(), &vec![face.clone()]);

    // With a `DocumentSettings` and a `Colors` before it, and an unknown element between `Colors` and `StyleSheets`: the table goes just before `StyleSheets`, and every other child
    // stays where it was.
    let bytes = VsdxBuilder::new()
        .colors(r##"<ColorEntry IX="24" RGB="#123456"/>"##)
        .document_extra(NOTE)
        .page(PageSpec::new(1, "P").shapes(&shape(1, &cell("PinX", "1"))))
        .pkg()
        .map_parts(|name, bytes| {
            if name == DOC {
                let text = String::from_utf8(bytes.clone()).unwrap().replace("<FaceNames></FaceNames>", "");
                let text =
                    text.replace("<Colors>", "<DocumentSettings/><Colors>").replace("</Colors>", r#"</Colors><y:mid xmlns:y="urn:example:mid"/>"#);
                *bytes = text.into_bytes();
            }
        })
        .build();
    assert_eq!(children(&bytes), ["DocumentSettings", "Colors", "mid", "StyleSheets", "DocumentSheet", "note"]);
    let (mut doc, _) = read_ok(&bytes);
    crate::model_writer::visit("no table, and the children around where it goes", &bytes);
    doc.fonts = Arc::new(vec![face]);
    let written = model_write(&doc, false);
    assert_eq!(children(&written.bytes), ["DocumentSettings", "Colors", "mid", "FaceNames", "StyleSheets", "DocumentSheet", "note"]);
}
