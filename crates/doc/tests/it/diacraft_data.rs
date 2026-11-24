//! DiaCraft's own data: the `Data` element of the solution part named `DiaCraft`, where the
//! options no specification gives a bit for are kept. `Document::diacraft` holds the element as read; these are the ways to read and change one
//! child of it without touching the rest.

use std::sync::Arc;

use diacraft_doc::{DIACRAFT_URN, DIACRAFT_VERSION, Document, RawAttrs, RawElement, RawNode};

fn data(version: &str, children: Vec<RawNode>) -> Arc<RawElement> {
    Arc::new(RawElement { name: Arc::from("dc:Data"), attrs: RawAttrs::from_pairs([("xmlns:dc", DIACRAFT_URN), ("dc:version", version)]), children })
}

fn child(name: &str, attrs: &[(&str, &str)]) -> RawNode {
    RawNode::Element(RawElement { name: Arc::from(name), attrs: RawAttrs::from_pairs(attrs.iter().copied()), children: Vec::new() })
}

fn names(doc: &Document) -> Vec<String> {
    doc.diacraft
        .as_ref()
        .map(|d| {
            d.children
                .iter()
                .filter_map(|c| match c {
                    RawNode::Element(e) => Some(e.name.to_string()),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn a_document_has_no_diacraft_data_until_something_is_set() {
    let mut doc = Document::new_blank();
    assert!(doc.diacraft.is_none() && doc.diacraft_child("snap").is_none());
    // Taking out what is not there makes nothing.
    assert!(doc.set_diacraft_child("snap", None));
    assert!(doc.diacraft.is_none());
}

#[test]
fn a_child_set_makes_the_data_element_in_the_urn_with_the_version() {
    let mut doc = Document::new_blank();
    assert!(doc.set_diacraft_child("snap", Some(&[("alignment_box", "1"), ("glue", "0")])));
    let d = doc.diacraft.as_ref().unwrap();
    assert_eq!(&*d.name, "dc:Data");
    assert_eq!(d.attrs.get("xmlns:dc"), Some(DIACRAFT_URN));
    assert_eq!(d.attrs.get("dc:version"), Some(DIACRAFT_VERSION));
    let c = doc.diacraft_child("snap").unwrap();
    assert_eq!(&*c.name, "dc:snap");
    assert_eq!(c.attrs.iter().collect::<Vec<_>>(), [("alignment_box", "1"), ("glue", "0")]);
}

#[test]
fn a_child_is_replaced_where_it_is_and_the_others_are_not_touched() {
    let mut doc = Document::new_blank();
    doc.diacraft = Some(data("1", vec![child("dc:other", &[("a", "1")]), child("dc:snap", &[("old", "1")]), RawNode::Comment(Arc::from(" kept "))]));
    assert!(doc.set_diacraft_child("snap", Some(&[("new", "2")])));
    assert_eq!(names(&doc), ["dc:other", "dc:snap"]);
    assert_eq!(doc.diacraft_child("snap").unwrap().attrs.get("new"), Some("2"));
    assert_eq!(doc.diacraft_child("snap").unwrap().attrs.get("old"), None);
    let d = doc.diacraft.as_ref().unwrap();
    assert!(matches!(d.children.get(2), Some(RawNode::Comment(c)) if &**c == " kept "));
    assert_eq!(d.children.first(), Some(&child("dc:other", &[("a", "1")])));
}

#[test]
fn a_child_taken_out_leaves_the_data_and_what_else_it_holds() {
    let mut doc = Document::new_blank();
    doc.diacraft = Some(data("1", vec![child("dc:snap", &[("x", "1")]), child("dc:other", &[])]));
    assert!(doc.set_diacraft_child("snap", None));
    assert_eq!(names(&doc), ["dc:other"]);
    assert!(doc.diacraft_child("snap").is_none());
}

#[test]
fn the_prefix_of_the_data_element_is_the_one_a_new_child_takes() {
    let mut doc = Document::new_blank();
    doc.diacraft = Some(Arc::new(RawElement {
        name: Arc::from("x:Data"),
        attrs: RawAttrs::from_pairs([("xmlns:x", DIACRAFT_URN), ("x:version", "1")]),
        children: Vec::new(),
    }));
    assert!(doc.set_diacraft_child("snap", Some(&[("a", "1")])));
    assert_eq!(names(&doc), ["x:snap"]);
    // And a Data in the default namespace takes a child with no prefix.
    let mut doc = Document::new_blank();
    doc.diacraft = Some(Arc::new(RawElement {
        name: Arc::from("Data"),
        attrs: RawAttrs::from_pairs([("xmlns", DIACRAFT_URN), ("version", "1")]),
        children: Vec::new(),
    }));
    assert!(doc.set_diacraft_child("snap", Some(&[("a", "1")])));
    assert_eq!(names(&doc), ["snap"]);
    assert!(doc.diacraft_child("snap").is_some());
}

#[test]
fn data_of_another_version_is_kept_and_not_interpreted_or_changed() {
    // An unknown `dc:version` must be kept and not interpreted, and a save by an older build must not lose it.
    let mut doc = Document::new_blank();
    doc.diacraft = Some(data("2", vec![child("dc:snap", &[("alignment_box", "1")])]));
    let before = doc.diacraft.clone();
    assert!(doc.diacraft_child("snap").is_none(), "not interpreted");
    assert!(!doc.set_diacraft_child("snap", Some(&[("alignment_box", "0")])), "not changed");
    assert!(!doc.set_diacraft_child("snap", None));
    assert_eq!(doc.diacraft, before);
}

#[test]
fn a_set_that_changes_nothing_leaves_the_element_the_same_allocation() {
    let mut doc = Document::new_blank();
    doc.set_diacraft_child("snap", Some(&[("a", "1")]));
    let before = doc.diacraft.clone().unwrap();
    doc.set_diacraft_child("snap", Some(&[("a", "1")]));
    assert!(Arc::ptr_eq(&before, doc.diacraft.as_ref().unwrap()));
}

#[test]
fn data_with_only_blanks_left_in_it_is_no_data() {
    // A file may indent its children, and the blanks between them are not content.
    let mut doc = Document::new_blank();
    doc.diacraft = Some(data("1", vec![RawNode::Text(Arc::from("\n  ")), child("dc:snap", &[("a", "1")]), RawNode::Text(Arc::from("\n"))]));
    assert!(doc.set_diacraft_child("snap", None));
    assert!(doc.diacraft.is_none(), "{:?}", doc.diacraft);
    // Text that is not blank stays: the data is not empty.
    doc.diacraft = Some(data("1", vec![RawNode::Text(Arc::from("note")), child("dc:snap", &[("a", "1")])]));
    assert!(doc.set_diacraft_child("snap", None));
    assert!(doc.diacraft.is_some());
}

#[test]
fn the_last_child_taken_out_takes_the_data_element_out_and_a_set_of_nothing_leaves_it() {
    let mut doc = Document::new_blank();
    assert!(doc.set_diacraft_child("snap", Some(&[("a", "1")])));
    assert!(doc.set_diacraft_child("snap", None));
    assert!(doc.diacraft.is_none(), "data with nothing in it is none");
    // Data that was empty to begin with, and a take-out of what is not there, are not touched.
    doc.diacraft = Some(data("1", Vec::new()));
    let before = doc.diacraft.clone();
    assert!(doc.set_diacraft_child("snap", None));
    assert_eq!(doc.diacraft, before);
}

// ---- APPID: the namespace moved to the domain `diacraft.app`; the one earlier builds wrote is read, never written.

/// The namespace earlier builds wrote, spelled out here and not through the constant under test.
const OLD: &str = "urn:io.github.one_bit.diacraft:extension";

#[test]
fn the_namespace_is_the_domain_s_and_the_earlier_one_is_read_as_the_same() {
    assert_eq!(DIACRAFT_URN, "urn:app.diacraft:extension");
    assert_eq!(diacraft_doc::DIACRAFT_URN_LEGACY, OLD);
    assert!(diacraft_doc::is_diacraft_urn(DIACRAFT_URN) && diacraft_doc::is_diacraft_urn(OLD));
    for other in [
        "",
        "urn:app.diacraft:extension:blob",
        "urn:app.diacraft:extension ",
        "URN:APP.DIACRAFT:EXTENSION",
        "urn:io.github.one_bit.diacraft:extension:blob",
        "urn:app.diacraft",
    ] {
        assert!(!diacraft_doc::is_diacraft_urn(other), "{other:?} is not DiaCraft's namespace");
    }
    assert_eq!(diacraft_doc::current_namespace(OLD), DIACRAFT_URN);
    assert_eq!(diacraft_doc::current_namespace(DIACRAFT_URN), DIACRAFT_URN);
    assert_eq!(diacraft_doc::current_namespace("urn:other"), "urn:other");
}

fn element(name: &str, attrs: &[(&str, &str)], children: Vec<RawNode>) -> RawElement {
    RawElement { name: Arc::from(name), attrs: RawAttrs::from_pairs(attrs.iter().copied()), children }
}

#[test]
fn a_declaration_of_the_earlier_namespace_anywhere_in_kept_markup_is_made_the_current_one_and_nothing_else_changes() {
    let deep = element("x:deep", &[("xmlns:x", OLD), ("Type", OLD), ("k", "v")], vec![RawNode::Text(Arc::from(OLD))]);
    let mut data = element(
        "dc:Data",
        &[("xmlns:dc", OLD), ("dc:version", "1"), ("note", OLD)],
        vec![
            RawNode::Element(element("dc:snap", &[("a", "1")], vec![RawNode::Element(deep)])),
            RawNode::Element(element("o:other", &[("xmlns:o", "urn:other"), ("xmlns", OLD)], Vec::new())),
            RawNode::Comment(Arc::from(OLD)),
        ],
    );
    let untouched = match data.children.first() {
        Some(RawNode::Element(e)) => e.attrs.shared_list().cloned(),
        _ => None,
    };
    diacraft_doc::use_current_namespace(&mut data);
    assert_eq!(data.attrs.get("xmlns:dc"), Some(DIACRAFT_URN));
    assert_eq!(data.attrs.get("note"), Some(OLD), "an attribute that is no declaration keeps its value, whatever it says");
    assert_eq!(data.attrs.get("dc:version"), Some("1"));
    let RawNode::Element(snap) = &data.children[0] else { panic!("snap") };
    assert!(Arc::ptr_eq(&untouched.unwrap(), snap.attrs.shared_list().unwrap()), "an element that declares nothing of the kind is not copied");
    let RawNode::Element(deep) = &snap.children[0] else { panic!("deep") };
    assert_eq!(deep.attrs.get("xmlns:x"), Some(DIACRAFT_URN));
    assert_eq!(deep.attrs.get("Type"), Some(OLD));
    assert_eq!(deep.children, vec![RawNode::Text(Arc::from(OLD))], "text is the file's");
    let RawNode::Element(other) = &data.children[1] else { panic!("other") };
    assert_eq!(other.attrs.get("xmlns:o"), Some("urn:other"), "another namespace is as it was");
    assert_eq!(other.attrs.get("xmlns"), Some(DIACRAFT_URN), "a default namespace is a declaration too");
    assert_eq!(data.children[2], RawNode::Comment(Arc::from(OLD)));
}

#[test]
fn kept_markup_a_hundred_thousand_levels_deep_is_walked_without_recursion() {
    // The walk keeps its own list of what is left, so that a tree a hostile model built cannot end the thread.
    let mut inner = element("dc:leaf", &[("xmlns:dc", OLD)], Vec::new());
    for _ in 0..100_000 {
        inner = element("dc:n", &[], vec![RawNode::Element(inner)]);
    }
    diacraft_doc::use_current_namespace(&mut inner);
    let mut at = &inner;
    let mut levels = 0;
    while let Some(RawNode::Element(next)) = at.children.first() {
        at = next;
        levels += 1;
    }
    assert_eq!(levels, 100_000);
    assert_eq!(at.attrs.get("xmlns:dc"), Some(DIACRAFT_URN));
    // Dropping the tree whole would recurse, too: take it apart from the top.
    let mut rest = std::mem::take(&mut inner.children);
    while let Some(RawNode::Element(mut e)) = rest.pop() {
        rest = std::mem::take(&mut e.children);
    }
}

#[test]
fn a_prefix_the_element_takes_from_a_declaration_of_the_earlier_namespace_outside_it_is_declared_by_the_element_in_the_current_one() {
    let outer = [("xmlns:dc", OLD), ("xmlns:other", "urn:other"), ("xmlns:unused", OLD), ("office:version", "1.4")];
    let inner = element("dc:snap", &[("n", "1"), ("q:x", "2")], vec![RawNode::Element(element("dc:deep", &[], Vec::new()))]);
    let mut data = element("dc:Data", &[("dc:version", "1")], vec![RawNode::Element(inner)]);
    diacraft_doc::declare_inherited_namespace(&mut data, outer.iter().copied());
    // One declaration, for the prefix the element is in, first; none for a prefix nothing uses or that is another namespace's; the element's own attributes after it, as they were.
    assert_eq!(data.attrs.iter().collect::<Vec<_>>(), [("xmlns:dc", DIACRAFT_URN), ("dc:version", "1")]);
    // A prefix used only by a child, and the default namespace, which an element with no prefix is in.
    let mut by_child = element("x:Data", &[], vec![RawNode::Element(element("dc:snap", &[], Vec::new()))]);
    diacraft_doc::declare_inherited_namespace(&mut by_child, outer.iter().copied());
    assert_eq!(by_child.attrs.iter().collect::<Vec<_>>(), [("xmlns:dc", DIACRAFT_URN)]);
    let mut plain = element("Data", &[("version", "1")], vec![RawNode::Element(element("snap", &[("dc:n", "1")], Vec::new()))]);
    diacraft_doc::declare_inherited_namespace(&mut plain, [("xmlns", OLD), ("xmlns:dc", OLD)]);
    assert_eq!(plain.attrs.iter().collect::<Vec<_>>(), [("xmlns", DIACRAFT_URN), ("xmlns:dc", DIACRAFT_URN), ("version", "1")]);
}

#[test]
fn an_element_that_declares_the_prefix_itself_or_stands_in_the_current_namespace_is_not_changed() {
    let own = element("dc:Data", &[("xmlns:dc", "urn:other"), ("dc:version", "1")], Vec::new());
    for (what, outer) in
        [("a prefix it declares itself", vec![("xmlns:dc", OLD)]), ("the current namespace", vec![("xmlns:dc", DIACRAFT_URN)]), ("none", Vec::new())]
    {
        let mut data = own.clone();
        diacraft_doc::declare_inherited_namespace(&mut data, outer);
        assert!(data == own, "{what}");
        assert!(Arc::ptr_eq(data.attrs.shared_list().unwrap(), own.attrs.shared_list().unwrap()), "{what}: not copied");
    }
    let mut elsewhere = element("dc:Data", &[("dc:version", "1")], Vec::new());
    diacraft_doc::declare_inherited_namespace(&mut elsewhere, [("xmlns:dc", "urn:other")]);
    assert_eq!(elsewhere.attrs.get("xmlns:dc"), None, "a prefix the outer elements bind to another namespace is theirs");
}

#[test]
fn many_declarations_outside_and_a_tree_a_hundred_thousand_levels_deep_are_walked_in_one_pass_each() {
    // The names of a hostile model: a hundred thousand declarations of the earlier namespace outside, a hundred thousand elements each of a prefix of its own inside, and a tree as deep.
    let outer: Vec<(String, &str)> = (0..100_000).map(|k| (format!("xmlns:p{k}"), OLD)).collect();
    let mut wide = element("p0:Data", &[], (0..100_000).map(|k| RawNode::Element(element(&format!("p{k}:e"), &[], Vec::new()))).collect());
    diacraft_doc::declare_inherited_namespace(&mut wide, outer.iter().map(|(k, v)| (k.as_str(), *v)));
    assert_eq!(wide.attrs.len(), 100_000);
    assert_eq!(wide.attrs.get("xmlns:p99999"), Some(DIACRAFT_URN));
    let mut deep = element("dc:leaf", &[], Vec::new());
    for _ in 0..100_000 {
        deep = element("dc:n", &[], vec![RawNode::Element(deep)]);
    }
    diacraft_doc::declare_inherited_namespace(&mut deep, [("xmlns:dc", OLD)]);
    assert_eq!(deep.attrs.get("xmlns:dc"), Some(DIACRAFT_URN));
    // Dropping a tree this deep whole would recurse: take it apart from the top.
    let mut rest = std::mem::take(&mut deep.children);
    while let Some(RawNode::Element(mut e)) = rest.pop() {
        rest = std::mem::take(&mut e.children);
    }
}
