//! A property of the document that a VSDX file states with an empty element (an application writes the fields it has nothing for so) is written by the model writer and read back as it was.
//!
//! `meta.xml` has no field for an empty text: the reader takes `<dc:subject/>` for no field and keeps the element as markup. The model writer used to refuse the drawing ("a change of the document's own
//! fields, styles or masters"), since a read of the package it made gave `None` where the model held an empty text; the document file carries the empty fields now, and the stamp that judges the
//! document item says the empty field as the file does (none).

use std::sync::Arc;

use diacraft_odg::package::Package;
use diacraft_odg::{Limits, WriteOptions, read, write_with};

use crate::model_write::blank;

/// A drawing whose fields are the empty ones an application writes, and some that have text.
fn drawing() -> diacraft_doc::Document {
    let mut doc = blank(1);
    let props = Arc::make_mut(&mut doc.props);
    for slot in [&mut props.subject, &mut props.keywords, &mut props.description, &mut props.language, &mut props.last_modified_by] {
        *slot = Some(Arc::from(""));
    }
    props.creator = Some(Arc::from("A person"));
    props.created = Some(Arc::from("2020-01-02T03:04:05Z"));
    props.modified = Some(Arc::from(""));
    props.category = Some(Arc::from(""));
    doc
}

#[test]
fn the_empty_fields_of_a_drawing_are_written_and_read_back_as_they_were() {
    let doc = drawing();
    let written = write_with(&doc, None, &WriteOptions::default()).unwrap_or_else(|e| panic!("{e}"));
    let (back, report) = read(&written.bytes, &Limits::default()).unwrap();
    assert_eq!(*back.props, *doc.props, "the properties come back as the drawing held them");
    assert_eq!((report.carry.stale, report.carry.gone, report.carry.left_out), (0, 0, 0), "{:?}", report.carry);
    // A second save of what was read is the same drawing: the model writer takes it as it is.
    let again = write_with(&back, None, &WriteOptions::default()).unwrap_or_else(|e| panic!("{e}"));
    let (twice, _) = read(&again.bytes, &Limits::default()).unwrap();
    assert_eq!(*twice.props, *doc.props);
}

/// The field another program gave text is that program's: an empty field the document file carries does not put the empty text over it, and the item is stale (the stamp of the document says it).
#[test]
fn a_field_another_program_gave_text_keeps_that_text() {
    let doc = drawing();
    let written = write_with(&doc, None, &WriteOptions::default()).unwrap();
    let package = Package::read(&written.bytes, &Limits::default()).unwrap();
    let meta = String::from_utf8(package.part("meta.xml").unwrap().to_vec()).unwrap();
    assert!(!meta.contains("<dc:subject"), "an empty field is no element of meta.xml: {meta}");
    // Another program sets the subject in `meta.xml`, leaving the document file as it was.
    let edited = meta.replacen("</office:meta>", "<dc:subject>Set by another program</dc:subject></office:meta>", 1);
    assert_ne!(edited, meta);
    use crate::zipbuild::{Item, build};
    let items: Vec<Item> = package
        .parts()
        .map(|part| {
            let content: Vec<u8> = if part == "meta.xml" { edited.as_bytes().to_vec() } else { package.part(part).unwrap_or_default().to_vec() };
            if part == "mimetype" { Item::stored(part, &content) } else { Item::deflated(part, &content) }
        })
        .collect();
    let bytes = build(&items);
    let (back, _) = read(&bytes, &Limits::default()).unwrap();
    assert_eq!(back.props.subject.as_deref(), Some("Set by another program"), "what the other program wrote wins");
}

/// **A refusal of a change of the document's own fields names the field** (the one sentence for all of them hid that 1,404 refusals were a core property written as an empty element and not the
/// masters the sentence named): a title, a property, a node of the root that no field reads, each said by itself, and each still saying "the document's own fields".
#[test]
fn a_refusal_of_a_change_of_the_documents_own_fields_says_which_field() {
    use crate::write_core::{open, real};
    let o = open(&crate::drawing::Drawing::a4().page(&crate::write_core::custom("s1", "1cm")));
    let words = |doc: &diacraft_doc::Document| match real(doc, &o.bytes, &o.index) {
        Err(diacraft_odg::WriteError::Unsupported(words)) => words.to_owned(),
        Err(other) => panic!("another refusal than the writer's words: {other}"),
        Ok(saved) => panic!("a save that dropped a change: {} bytes", saved.bytes.len()),
    };
    let mut titled = o.doc.clone();
    titled.title = Arc::from("Another title");
    assert_eq!(words(&titled), "a change of the document's own fields (its title)");
    let mut described = o.doc.clone();
    Arc::make_mut(&mut described.props).description = Some(Arc::from("A description"));
    assert_eq!(words(&described), "a change of the document's own fields (a property)");
    let mut versioned = o.doc.clone();
    versioned.version = versioned.version.wrapping_add(1);
    assert_eq!(words(&versioned), "a change of the document's own fields (its version)");
    let mut extra = o.doc.clone();
    extra.passthrough.push(
        0,
        diacraft_doc::RawNode::Element(diacraft_doc::RawElement {
            name: Arc::from("Elsewhere"),
            attrs: diacraft_doc::RawAttrs::from_pairs([("k", "v")]),
            children: Vec::new(),
        }),
    );
    assert_eq!(words(&extra), "a change of the document's own fields (the elements of its root)");
}
