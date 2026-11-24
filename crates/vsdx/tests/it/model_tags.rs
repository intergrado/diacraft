//! ODF's markup and a newer build's, held inside a shape's text, are never written into a VSDX `Text` (the native-format design's §3.2).
//!
//! Nothing fills `TextPart::OdfRaw` and `TextPart::NativeRaw` yet (do); the writer already leaves them out, so that
//! the day they are filled no VSDX file carries markup of another format. Tests may unwrap and panic.

use std::sync::Arc;

use diacraft_doc::{RawNode, TextPart};
use diacraft_opc::{Limits, Package};
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};
use diacraft_vsdx::{WriteOptions, write_with};

use crate::read_ok;

fn comment(text: &str) -> RawNode {
    RawNode::Comment(Arc::from(text))
}

#[test]
fn held_odf_and_native_markup_is_not_written_into_a_vsdx_text() {
    let parts = vec![
        TextPart::Chars(Arc::from("a")),
        TextPart::OdfRaw(Box::new(comment("zz-odf-marker"))),
        TextPart::Cp(0),
        TextPart::NativeRaw(Box::new(comment("zz-native-marker"))),
        TextPart::Raw(comment("zz-vsdx-marker")),
        TextPart::Chars(Arc::from("b")),
    ];
    let doc = DocBuilder::new().page(1, PageBuilder::new().shape(ShapeBuilder::new(1).text_parts(parts))).build();
    let written = write_with(&doc, None, &WriteOptions { now: Some(1_700_000_000), ..WriteOptions::default() }).unwrap();

    // No part that says what the drawing is holds either marker, and the VSDX one is there: the markup of the other format is not written into a text. It rides in DiaCraft's own parts
    // Which a program that does not know them ignores.
    let package = Package::read(&written.bytes, &Limits::default()).unwrap();
    let holds = |part: &str, marker: &str| package.part(part).is_some_and(|bytes| bytes.windows(marker.len()).any(|w| w == marker.as_bytes()));
    let in_the_drawing = |marker: &str| {
        package.parts().filter(|name| name.starts_with("/visio/pages/") || name.starts_with("/visio/document")).any(|name| holds(name, marker))
    };
    assert!(in_the_drawing("zz-vsdx-marker"), "VSDX's own markup is written");
    assert!(!in_the_drawing("zz-odf-marker"), "ODF's markup is not written into a VSDX file's text");
    assert!(!in_the_drawing("zz-native-marker"), "a newer build's markup is not written into a VSDX file's text");
    assert!(
        holds("/visio/solutions/solution1.xml", "zz-odf-marker") && holds("/visio/solutions/solution1.xml", "zz-native-marker"),
        "they ride in DiaCraft's solution part"
    );

    // Read back, the text is what the file says (the characters, the marker and VSDX's own node, in order) with the two that rode put back where they stood.
    let (back, report) = read_ok(&written.bytes);
    assert_eq!((report.carry.kept, report.carry.stale), (1, 0), "{:?}", report.carry);
    let shape = back.pages.get(0).unwrap().shapes.get(0).unwrap();
    let parts: Vec<&TextPart> = shape.text.as_ref().unwrap().parts.iter().collect();
    assert_eq!(parts.len(), 6, "{parts:?}");
    assert!(matches!(parts[0], TextPart::Chars(c) if &**c == "a"));
    assert!(matches!(parts[1], TextPart::OdfRaw(n) if matches!(&**n, RawNode::Comment(c) if &**c == "zz-odf-marker")));
    assert!(matches!(parts[2], TextPart::Cp(0)));
    assert!(matches!(parts[3], TextPart::NativeRaw(n) if matches!(&**n, RawNode::Comment(c) if &**c == "zz-native-marker")));
    assert!(matches!(parts[4], TextPart::Raw(RawNode::Comment(c)) if &**c == "zz-vsdx-marker"));
    assert!(matches!(parts[5], TextPart::Chars(c) if &**c == "b"));
}
