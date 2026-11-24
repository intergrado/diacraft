//! The markup a shape kept of the file it was read from, as the model writer writes it back (`write_with(doc, None, ..)`, the security scan's three findings on
//!). A name or a value XML cannot hold is refused in words and never written; a prefix is written only where it stands for the namespace it was read in (declared again on the
//! element where the place binds it otherwise), and markup whose namespace the model does not know is left out and said; and the children a page kept are taken from the cap of the
//! document's data by their size. Every file written is read again by a namespace-aware walk of the test's own and by the real reader.

use std::collections::HashMap;
use std::sync::Arc;

use diacraft_container::xml::{El, Node};
use diacraft_doc::{Document, Kept, RawAttrs, RawElement, RawNode};
use diacraft_odg::{Limits, SaveNote, WriteError, WriteOptions, Written, read, write_with};

use crate::build::{DRAWING, package};
use crate::drawing::Drawing;

const MY: &str = "urn:example:my";
const OTHER: &str = "urn:example:other";
const SVG: &str = "urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0";

/// A rectangle of the file, with `inside` as its children and `attrs` on its start tag.
fn rect(attrs: &str, inside: &str) -> String {
    format!(r#"<draw:rect {attrs} svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="2cm">{inside}</draw:rect>"#)
}

/// A drawing whose root also binds `my` to [`MY`], with one page holding `shapes`.
fn with_my(shapes: &str) -> Drawing {
    let mut drawing = Drawing::a4().page(shapes);
    drawing.ns.push_str(&format!(r#" xmlns:my="{MY}""#));
    drawing
}

fn model_of(drawing: &Drawing) -> Document {
    let mut drawing = drawing.clone();
    drawing.version = "1.4".to_owned();
    read(&package(DRAWING, drawing.files()), &Limits::default()).unwrap().0
}

/// What the shape at `at` of the first page keeps of the file's markup, to be changed.
fn kept_of(doc: &mut Document, at: usize) -> &mut Kept {
    let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
    let shape = Arc::make_mut(page.shapes.get_mut(at).unwrap());
    &mut shape.ext.make_mut().odf.kept
}

fn save(doc: &Document) -> Result<Written, WriteError> {
    write_with(doc, None, &WriteOptions::default())
}

fn saved(doc: &Document) -> Written {
    save(doc).unwrap_or_else(|e| panic!("the save is refused: {e}"))
}

fn content(bytes: &[u8]) -> String {
    let package = diacraft_odg::package::Package::read(bytes, &Limits::default()).unwrap();
    String::from_utf8(package.part("content.xml").unwrap().to_vec()).unwrap()
}

/// Every element name and prefixed attribute name of `xml` in document order, with the namespace XML Namespaces 1.0 puts it in where it stands: `None` for a prefix that no
/// element at or above it binds (a part that holds one is not namespace-well-formed, and a namespace-aware parser refuses it).
fn names_in(xml: &str) -> Vec<(String, Option<String>)> {
    let root = diacraft_container::xml::parse(xml.as_bytes()).unwrap();
    let mut out = Vec::new();
    walk(&root, &HashMap::new(), &mut out);
    out
}

fn walk(el: &El, outer: &HashMap<String, String>, out: &mut Vec<(String, Option<String>)>) {
    let mut here = outer.clone();
    for (k, v) in &el.attrs {
        if k == "xmlns" {
            here.insert(String::new(), v.clone());
        } else if let Some(p) = k.strip_prefix("xmlns:") {
            here.insert(p.to_owned(), v.clone());
        }
    }
    let uri = |name: &str| match name.split_once(':') {
        Some(("xml", _)) => Some("http://www.w3.org/XML/1998/namespace".to_owned()),
        Some((p, _)) => here.get(p).filter(|u| !u.is_empty()).cloned(),
        None => Some(here.get("").cloned().unwrap_or_default()),
    };
    out.push((el.name.clone(), uri(&el.name)));
    for (k, _) in &el.attrs {
        if k == "xmlns" || k.starts_with("xmlns:") || !k.contains(':') {
            continue;
        }
        out.push((k.clone(), uri(k)));
    }
    for child in &el.children {
        if let Node::El(c) = child {
            walk(c, &here, out);
        }
    }
}

/// The names of `xml` that no element binds the prefix of.
fn unbound(xml: &str) -> Vec<String> {
    names_in(xml).into_iter().filter(|(_, uri)| uri.is_none()).map(|(n, _)| n).collect()
}

/// The namespace of the first name `name` of `xml`.
fn ns_of(xml: &str, name: &str) -> Option<String> {
    names_in(xml).into_iter().find(|(n, _)| n == name).unwrap_or_else(|| panic!("no {name} in the part")).1
}

/// The words of a refusal, or a panic for a save that was written.
fn refusal(saved: Result<Written, WriteError>) -> &'static str {
    match saved {
        Err(WriteError::Unsupported(words)) => words,
        Err(other) => panic!("refused, but not in words: {other}"),
        Ok(w) => panic!("written ({} bytes), where it must be refused", w.bytes.len()),
    }
}

// ---- Finding 1: names and values XML cannot hold -------------------------------------------------------------------------------------------------------------------------

/// A value of an attribute a shape kept that holds U+0001 (a character XML 1.0 does not allow) is refused in the words of the element path, and nothing is written.
#[test]
fn a_kept_attribute_value_xml_cannot_hold_is_refused_in_words_and_nothing_is_written() {
    let mut doc = model_of(&with_my(&rect(r#"my:note="kept""#, "")));
    assert!(saved(&doc).bytes.len() > 100, "the drawing as read is written");
    let raw = &mut kept_of(&mut doc, 0).raw;
    *raw = RawAttrs::from_pairs([("my:note", "a\u{1}b")]);
    let words = refusal(save(&doc));
    assert!(words.contains("XML 1.0 does not allow"), "{words}");
}

/// A name of a kept attribute that is no XML name is refused in the same words and is never written as markup: here one that would put a second attribute on the shape
/// (`my:a="1" my:b`, with the value `2`, wrote ` my:a="1" my:b="2"`).
#[test]
fn a_kept_attribute_name_that_is_no_xml_name_is_refused_and_never_written_as_markup() {
    let mut doc = model_of(&with_my(&rect(r#"my:note="kept""#, "")));
    kept_of(&mut doc, 0).raw = RawAttrs::from_pairs([(r#"my:a="1" my:b"#, "2")]);
    let words = refusal(save(&doc));
    assert!(words.contains("XML 1.0 does not allow"), "{words}");
    // A name with two colons is an XML name and no qualified name: no namespace-aware reader can read it, and it is refused too.
    kept_of(&mut doc, 0).raw = RawAttrs::from_pairs([("my:a:b", "2")]);
    let words = refusal(save(&doc));
    assert!(words.contains("XML 1.0 does not allow"), "{words}");
}

// ---- Finding 2: namespaces -----------------------------------------------------------------------------------------------------------------------------------------------

/// A kept attribute whose prefix the file bound on the shape's own element is kept with that declaration, and written under it. **A prefix nothing binds** (here the one a sibling's kept
/// element declares for itself, and a shape's own, which bind nothing where another shape is) is not written where it is used: the part made is namespace-well-formed, the attribute is left
/// out and the save says so, and the siblings' elements read back in their own namespace.
#[test]
fn kept_markup_whose_prefix_only_a_sibling_declares_is_left_out_and_said_and_the_part_stays_well_formed() {
    let note = format!(r#"<my:note xmlns:my="{MY}"/>"#);
    let a = rect("", &format!(r#"<my:a xmlns:my="{MY}" my:k="v"/>"#));
    let b = rect(&format!(r#"xmlns:my="{MY}" my:b="1""#), "");
    let c = rect(r#"my:c="1""#, "");
    let doc = model_of(&Drawing::a4().page(&format!("{note}{a}{b}{c}")));
    let written = saved(&doc);
    let xml = content(&written.bytes);
    assert_eq!(unbound(&xml), Vec::<String>::new(), "every prefix of the part is bound where it is used:\n{xml}");
    assert_eq!(
        ns_of(&xml, "my:b").as_deref(),
        Some(MY),
        "the attribute of a prefix its own shape declared is written under that declaration:\n{xml}"
    );
    assert!(!xml.contains("my:c="), "the attribute whose prefix nothing binds is not written:\n{xml}");
    assert_eq!(ns_of(&xml, "my:a").as_deref(), Some(MY), "{xml}");
    assert_eq!(ns_of(&xml, "my:k").as_deref(), Some(MY), "{xml}");
    let left: Vec<usize> = written
        .notes
        .iter()
        .filter_map(|n| match n {
            SaveNote::NewShapeUnwritten { attributes, .. } => Some(*attributes),
            _ => None,
        })
        .collect();
    assert_eq!(left, [1], "the one attribute left out is said: {:?}", written.notes);
    // And the file reads back with the sibling's element in the namespace it was read in.
    let (back, _) = read(&written.bytes, &Limits::default()).unwrap();
    let kept = crate::drawing::odf(crate::drawing::shape(&back, 0, 0));
    assert!(kept.kept.iter().any(|p| matches!(&p.node, RawNode::Element(e) if &*e.name == "my:a")), "{kept:?}");
}

/// The namespace a shape's kept markup was read in is the one in force on the element that holds the pages, the root's declarations under the containers': where the file's
/// `office:drawing` binds `my` over the root's binding, the kept attribute and the kept child are read in the drawing's, and are written back in it (the model holds the drawing's
/// declaration, and the part made has it). Where nothing but the root binds it, nothing is declared again.
#[test]
fn kept_markup_is_written_in_the_namespace_in_force_where_the_pages_are() {
    let mut drawing = with_my(&rect(r#"my:b="1""#, "<my:c/>"));
    drawing.version = "1.4".to_owned();
    let mut files = drawing.files();
    let content_xml = drawing.content_xml().replace("<office:drawing>", &format!(r#"<office:drawing xmlns:my="{OTHER}">"#));
    assert_eq!(files[0].0, "content.xml");
    files[0].1 = content_xml.into_bytes();
    let doc = read(&package(DRAWING, files), &Limits::default()).unwrap().0;
    let xml = content(&saved(&doc).bytes);
    assert_eq!(unbound(&xml), Vec::<String>::new(), "{xml}");
    assert_eq!(ns_of(&xml, "my:b").as_deref(), Some(OTHER), "the kept attribute is in the namespace it was read in:\n{xml}");
    assert_eq!(ns_of(&xml, "my:c").as_deref(), Some(OTHER), "and so is the kept child:\n{xml}");
    let plain = model_of(&with_my(&rect(r#"my:b="1""#, "<my:c/>")));
    let xml = content(&saved(&plain).bytes);
    assert_eq!(ns_of(&xml, "my:b").as_deref(), Some(MY), "{xml}");
    assert_eq!(ns_of(&xml, "my:c").as_deref(), Some(MY), "{xml}");
    assert_eq!(xml.matches(&format!(r#"xmlns:my="{MY}""#)).count(), 1, "only the root declares it:\n{xml}");
}

/// A kept attribute that names, under another prefix bound to the same namespace, an attribute the writer writes itself (`s:x` beside the writer's `svg:x`) is not written a
/// second time: one element holding two attributes of one namespace and local name is not namespace-well-formed. The writer's own stands, and the one left out is said.
#[test]
fn a_kept_attribute_the_writer_writes_under_another_prefix_is_not_written_twice() {
    let mut drawing = with_my(&rect(r#"my:b="1""#, ""));
    drawing.ns.push_str(&format!(r#" xmlns:s="{SVG}""#));
    let mut doc = model_of(&drawing);
    kept_of(&mut doc, 0).raw.push("s:x", "9cm");
    let written = saved(&doc);
    let xml = content(&written.bytes);
    let xs = names_in(&xml).into_iter().filter(|(n, uri)| n.ends_with(":x") && uri.as_deref() == Some(SVG)).count();
    assert_eq!(xs, 1, "one svg:x on the shape:\n{xml}");
    assert!(!xml.contains(r#"s:x="9cm""#), "{xml}");
    assert!(written.notes.iter().any(|n| matches!(n, SaveNote::NewShapeUnwritten { attributes: 1, .. })), "{:?}", written.notes);
}

// ---- Finding 3: the cap of a page's kept children ------------------------------------------------------------------------------------------------------------------------

/// A kept child of a page: an element of [`MY`] holding `text`.
fn blob(text: &Arc<str>) -> RawNode {
    RawNode::Element(RawElement {
        name: Arc::from("my:blob"),
        attrs: RawAttrs::from_pairs([("xmlns:my", MY)]),
        children: vec![RawNode::Text(Arc::clone(text))],
    })
}

/// A drawing whose page keeps `count` children of a mebibyte each (one text, shared).
pub(crate) fn page_keeping(count: usize) -> Document {
    let mut doc = model_of(&Drawing::a4().page(""));
    let text: Arc<str> = Arc::from("x".repeat(1 << 20));
    let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
    let kept = &mut page.ext.make_mut().odf.kept.kept;
    for _ in 0..count {
        kept.push(0, blob(&text));
    }
    doc
}

/// The children a page kept are taken from the 16 MiB the document's data may come to in one part, each by its size: seventeen of a mebibyte, each of which fits alone, are
/// past it together and refused in words; two are written, and read back.
#[test]
fn a_page_s_kept_children_are_taken_from_the_cap_by_their_size_and_past_it_refused_in_words() {
    let words = refusal(save(&page_keeping(17)));
    assert!(words.contains("larger than the reader reads of a part"), "{words}");
    let written = saved(&page_keeping(2));
    let xml = content(&written.bytes);
    assert_eq!(xml.matches("<my:blob").count(), 2);
    assert!(written.work.bytes_copied >= 2 << 20, "the copy of the children is charged by their size: {}", written.work.bytes_copied);
}
