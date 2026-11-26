//! A group's style name is checked as a shape's is (`write/shapes/asread.rs`, `AsRead::of_group`), found by the security scan.
//!
//! The record of what a group of the file held was made by hand (`AsRead { style, ..default }`) and skipped the check `AsRead::of` makes of a shape's: a style name with a character XML 1.0 does
//! not allow. The output was never malformed on the base (the splice refuses any text it inserts that holds such a character, with the same words), but the check belongs where the record is made.
//! Every read-back goes through the real reader, and every input is built in code from the ODF 1.4 Part 3 vocabulary.

use std::sync::Arc;

use diacraft_doc::Document;
use diacraft_odg::{Limits, WriteOptions, read, write_with};

use crate::drawing::Drawing;
use crate::visit_styles::{GR1, part, rect, source_of, through_codec};

fn unedited() -> WriteOptions {
    WriteOptions { content_unchanged: true, ..WriteOptions::default() }
}

/// The model of the file `bytes`.
fn model(bytes: &[u8]) -> Document {
    read(bytes, &Limits::default()).unwrap().0
}

const GR3: &str =
    r##"<style:style style:name="gr3" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#ff0000"/></style:style>"##;

/// A drawing whose one group is on the automatic style `gr3`.
fn styled_group() -> Drawing {
    Drawing::a4().auto(GR1).auto(GR3).page(&format!(r#"<draw:g draw:style-name="gr3">{}{}</draw:g>"#, rect("gr1"), rect("gr1")))
}

/// The model of `drawing` with the style name of its first shape (the group) set to `name`.
fn named(drawing: &Drawing, name: &str) -> Document {
    let mut doc = model(&source_of(drawing));
    let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
    let group = Arc::make_mut(page.shapes.get_mut(0).unwrap());
    assert!(!group.children.is_empty(), "the first shape is the group");
    group.ext.make_mut().odf.style_name = Some(Arc::from(name));
    doc
}

/// **A group's style name is checked as a shape's is** (`AsRead::of`): a name with a character XML 1.0 does not allow reached the output as malformed XML, because the group's record of what the
/// file held was made by hand and skipped the check. The save is refused in words, and nothing is written; a name that holds only characters XML takes is written.
#[test]
fn a_group_whose_style_name_has_a_character_xml_does_not_allow_is_refused_in_words() {
    for (what, name) in [
        ("a control character", "gr\u{1}3"),
        ("a nul", "gr\u{0}3"),
        ("U+FFFE", "gr\u{FFFE}3"),
        ("U+FFFF", "gr3\u{FFFF}"),
        ("a name that is all of one", "\u{8}"),
    ] {
        let doc = named(&styled_group(), name);
        let refused = write_with(&through_codec(&doc), None, &unedited()).map(|w| String::from_utf8_lossy(&part_bytes(&w.bytes)).into_owned());
        match refused {
            Err(e) => assert!(
                e.to_string().contains("a character XML 1.0 does not allow"),
                "{what}: refused, but not in the words of a character XML does not allow: {e}"
            ),
            Ok(content) => panic!("{what}: the group was written with a name XML cannot carry: {content:?}"),
        }
    }
    // The control: a name of characters XML takes, with a quote and an ampersand in it, is written, escaped, and the file reads again.
    let doc = named(&styled_group(), "gr\"&3");
    let written = write_with(&through_codec(&doc), None, &unedited()).unwrap();
    let again = model(&written.bytes);
    assert_eq!(
        again.pages.get(0).and_then(|p| p.shapes.get(0)).and_then(|g| g.ext.get().and_then(|d| d.odf.style_name.clone())).as_deref(),
        Some("gr\"&3")
    );
}

/// The content.xml of the package, as bytes (for a message of a test that failed).
fn part_bytes(bytes: &[u8]) -> Vec<u8> {
    part(bytes, "content.xml").into_bytes()
}

/// **The document file and the markup agree on which groups are styled**: a group of the file with no style name, or an empty one, is a new group (no style of its own on the element, and no
/// cell of one in the document file); one that has a name is written with a style of its own.
#[test]
fn the_document_file_and_the_markup_agree_on_which_groups_are_styled() {
    let unstyled = Drawing::a4().auto(GR1).page(&format!("<draw:g>{}{}</draw:g>", rect("gr1"), rect("gr1")));
    for (what, name) in [("no name", None), ("an empty name", Some(""))] {
        let mut doc = model(&source_of(&unstyled));
        if let Some(name) = name {
            let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
            Arc::make_mut(page.shapes.get_mut(0).unwrap()).ext.make_mut().odf.style_name = Some(Arc::from(name));
        }
        let written = write_with(&through_codec(&doc), None, &unedited()).unwrap_or_else(|e| panic!("{what}: an unstyled group is written: {e}"));
        let content = part(&written.bytes, "content.xml");
        let group = &content[content.find("<draw:g").expect("the group")..];
        let tag = &group[..group.find('>').unwrap()];
        assert!(!tag.contains("draw:style-name"), "{what}: no style of its own: {tag}");
    }
    let written = write_with(&through_codec(&named(&styled_group(), "gr3")), None, &unedited()).unwrap();
    let content = part(&written.bytes, "content.xml");
    let group = &content[content.find("<draw:g").expect("the group")..];
    assert!(group[..group.find('>').unwrap()].contains("draw:style-name="), "a group with a style name is written with a style: {content}");
}
