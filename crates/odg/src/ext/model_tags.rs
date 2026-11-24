//! ODF's markup and a newer build's, held inside a shape's text (`TextPart::OdfRaw`, `TextPart::NativeRaw`), is not written into a tier-2 text
//! (the native-format design's §3.2). Nothing fills those variants yet: fills the first, the second, and the model writers and
//! the carriers route them. Until then the tier-2 writers leave them out and the fingerprint counts no character for them. A master body in the document file is the
//! whole-model codec's and carries them.

use std::sync::Arc;

use diacraft_doc::{Master, RawNode, Shape, ShapeList, Text, TextPart};
use diacraft_sheetxml::model::MemBlobs;

use super::{body, fingerprint, payload};

fn comment(text: &str) -> RawNode {
    RawNode::Comment(Arc::from(text))
}

fn text() -> Text {
    Text {
        parts: vec![
            TextPart::Chars(Arc::from("a")),
            TextPart::OdfRaw(Box::new(comment("zz-odf-marker"))),
            TextPart::NativeRaw(Box::new(comment("zz-native-marker"))),
            TextPart::Raw(comment("zz-kept-marker")),
            TextPart::Chars(Arc::from("b")),
        ],
        ..Default::default()
    }
}

#[test]
fn a_tier_2_text_fragment_leaves_out_held_odf_and_native_markup() {
    let fragment = payload::text_fragment(&text()).unwrap();
    assert!(fragment.contains("zz-kept-marker"), "the markup the writer carries stays: {fragment}");
    assert!(!fragment.contains("zz-odf-marker") && !fragment.contains("zz-native-marker"), "{fragment}");
    assert!(fragment.contains('a') && fragment.contains('b'));
}

/// The document file's master bodies are the whole-model codec's, which carries what a shape's text holds of ODF's markup and of a newer build's with the
/// rest of the model: a master is self-contained, and nothing routes these parts to another place for it.
#[test]
fn a_document_files_master_carries_held_odf_and_native_markup() {
    let mut shape = Shape::new(1);
    shape.text = Some(Arc::new(text()));
    let master = Master { id: 1, shapes: ShapeList::from_vec(vec![Arc::new(shape)]), ..Master::default() };
    let mut tree = body::master_el(&master, &mut MemBlobs::new()).unwrap();
    let debug = format!("{tree:?}");
    for marker in ["zz-odf-marker", "zz-native-marker", "zz-kept-marker"] {
        assert!(debug.contains(marker), "{marker} is written: {debug}");
    }
    let limits = crate::limits::Limits::default();
    let mut reader = crate::read::Reader::new(&limits);
    let back = body::read_master(&mut reader, &mut tree, &body::Pictures::default()).unwrap().unwrap();
    assert_eq!(*back, master, "and read back");
}

#[test]
fn the_fingerprints_characters_count_no_held_markup() {
    assert_eq!(fingerprint::plain_of(&text()), "ab");
}
