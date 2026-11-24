//! The Snap & Glue words of `DocumentSettings`: where real files keep the document's snap and glue settings
//! Read and changed without touching anything else the element holds.

use std::sync::Arc;

use diacraft_doc::{Document, Passthrough, RawAttrs, RawElement, RawNode, SnapWords};

fn text(name: &str, value: &str) -> RawNode {
    RawNode::Element(RawElement { name: Arc::from(name), attrs: RawAttrs::new(), children: vec![RawNode::Text(Arc::from(value))] })
}

fn empty(name: &str) -> RawNode {
    RawNode::Element(RawElement { name: Arc::from(name), attrs: RawAttrs::new(), children: Vec::new() })
}

fn settings(attrs: &[(&str, &str)], children: Vec<RawNode>) -> RawNode {
    RawNode::Element(RawElement { name: Arc::from("DocumentSettings"), attrs: RawAttrs::from_pairs(attrs.iter().copied()), children })
}

/// A document whose passthrough holds `nodes`, each after `after` typed children.
fn with(nodes: Vec<(u32, RawNode)>) -> Document {
    let mut doc = Document::new_blank();
    let mut passthrough = Passthrough::new();
    for (after, node) in nodes {
        passthrough.push(after, node);
    }
    doc.passthrough = passthrough;
    doc
}

/// The children of the `DocumentSettings` element of a document, as `(name, text)`; an element with children shows `…`.
fn children(doc: &Document) -> Vec<(String, String)> {
    let Some(RawNode::Element(el)) =
        doc.passthrough.iter().map(|p| &p.node).find(|n| matches!(n, RawNode::Element(e) if &*e.name == "DocumentSettings"))
    else {
        return Vec::new();
    };
    el.children
        .iter()
        .filter_map(|c| match c {
            RawNode::Element(e) => {
                let inner: String = e.children.iter().map(|n| if let RawNode::Text(t) = n { t.to_string() } else { "…".to_owned() }).collect();
                Some((e.name.to_string(), inner))
            }
            _ => None,
        })
        .collect()
}

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter().map(|(a, b)| ((*a).to_owned(), (*b).to_owned())).collect()
}

fn words(glue: Option<&str>, snap: Option<&str>, extensions: Option<&str>, dynamic: Option<&str>) -> SnapWords {
    let a = |s: Option<&str>| s.map(Arc::<str>::from);
    SnapWords { glue: a(glue), snap: a(snap), extensions: a(extensions), dynamic_grid: a(dynamic) }
}

#[test]
fn a_document_with_no_settings_has_no_words() {
    let doc = Document::new_blank();
    assert_eq!(doc.snap_words(), SnapWords::default());
}

#[test]
fn the_words_of_a_files_settings_are_read_as_text() {
    // Every sample holds the five children in this order; the words are integers as text.
    let doc = with(vec![(
        0,
        settings(
            &[("TopPage", "0"), ("DefaultTextStyle", "3")],
            vec![
                text("GlueSettings", "9"),
                text("SnapSettings", "65847"),
                text("SnapExtensions", "34"),
                empty("SnapAngles"),
                text("DynamicGridEnabled", "1"),
            ],
        ),
    )]);
    assert_eq!(doc.snap_words(), words(Some("9"), Some("65847"), Some("34"), Some("1")));
}

#[test]
fn a_word_is_trimmed_and_a_prefixed_name_is_matched_by_its_local_name() {
    let doc = with(vec![(0, settings(&[], vec![text("v:SnapSettings", "\n 3 \n"), text("GlueSettings", "")]))]);
    assert_eq!(doc.snap_words().snap.as_deref(), Some("3"));
    assert_eq!(doc.snap_words().glue.as_deref(), Some(""), "an empty element is a word that is empty, not an absent one");
}

#[test]
fn the_first_of_two_children_with_one_name_is_the_one_read_and_the_one_changed() {
    let mut doc = with(vec![(0, settings(&[], vec![text("SnapSettings", "1"), text("SnapSettings", "2")]))]);
    assert_eq!(doc.snap_words().snap.as_deref(), Some("1"));
    doc.set_snap_words(&words(None, Some("7"), None, None));
    assert_eq!(children(&doc), pairs(&[("SnapSettings", "7"), ("SnapSettings", "2")]));
}

#[test]
fn changing_one_word_leaves_everything_else_as_it_was() {
    let mut doc = with(vec![
        (
            0,
            settings(
                &[("TopPage", "0")],
                vec![text("GlueSettings", "9"), text("SnapSettings", "65847"), text("SnapExtensions", "34"), empty("SnapAngles")],
            ),
        ),
        (1, empty("EventList")),
    ]);
    let before = doc.passthrough.clone();
    doc.set_snap_words(&words(Some("9"), Some("65846"), Some("34"), None));
    assert_eq!(children(&doc), pairs(&[("GlueSettings", "9"), ("SnapSettings", "65846"), ("SnapExtensions", "34"), ("SnapAngles", "")]));
    let (b, a): (Vec<_>, Vec<_>) = (before.iter().collect(), doc.passthrough.iter().collect());
    assert_eq!(a.len(), b.len());
    assert_eq!(a.get(1), b.get(1), "the node after the settings is as it was");
    // The attributes of the element are as they were.
    let Some(RawNode::Element(el)) = a.first().map(|p| &p.node) else { panic!("settings first") };
    assert_eq!(el.attrs.get("TopPage"), Some("0"));
    assert_eq!(a.first().map(|p| p.after), Some(0));
}

#[test]
fn words_that_are_not_changed_are_left_byte_for_byte() {
    // A blank around a word is the file's own; setting the same word again must not rewrite it.
    let mut doc = with(vec![(0, settings(&[], vec![text("SnapSettings", " 3 ")]))]);
    let before = doc.passthrough.clone();
    doc.set_snap_words(&doc.snap_words());
    assert_eq!(doc.passthrough, before);
}

#[test]
fn a_word_the_file_lacks_goes_in_where_the_files_put_it() {
    // The order: GlueSettings, SnapSettings, SnapExtensions, SnapAngles, DynamicGridEnabled.
    let mut doc = with(vec![(0, settings(&[], vec![text("GlueSettings", "9"), empty("SnapAngles"), text("ProtectStyles", "0")]))]);
    doc.set_snap_words(&words(Some("9"), Some("3"), Some("34"), Some("1")));
    assert_eq!(
        children(&doc),
        pairs(&[
            ("GlueSettings", "9"),
            ("SnapSettings", "3"),
            ("SnapExtensions", "34"),
            ("SnapAngles", ""),
            ("DynamicGridEnabled", "1"),
            ("ProtectStyles", "0")
        ])
    );
}

#[test]
fn settings_that_are_not_there_are_made_first_in_the_document() {
    let mut doc = with(vec![(0, empty("EventList"))]);
    doc.set_snap_words(&words(None, Some("3"), None, Some("0")));
    let names: Vec<String> = doc
        .passthrough
        .iter()
        .filter_map(|p| match &p.node {
            RawNode::Element(e) => Some(e.name.to_string()),
            _ => None,
        })
        .collect();
    assert_eq!(names, ["DocumentSettings", "EventList"]);
    assert_eq!(children(&doc), pairs(&[("SnapSettings", "3"), ("DynamicGridEnabled", "0")]));
    assert_eq!(doc.passthrough.iter().next().map(|p| p.after), Some(0));
}

#[test]
fn no_words_make_no_settings() {
    let mut doc = Document::new_blank();
    doc.set_snap_words(&SnapWords::default());
    assert!(doc.passthrough.is_empty(), "nothing to say, nothing written");
}

#[test]
fn a_word_set_to_none_is_taken_out() {
    let mut doc = with(vec![(0, settings(&[("TopPage", "0")], vec![text("GlueSettings", "9"), text("DynamicGridEnabled", "1")]))]);
    doc.set_snap_words(&words(Some("9"), None, None, None));
    assert_eq!(children(&doc), pairs(&[("GlueSettings", "9")]));
}

#[test]
fn the_rest_of_the_document_is_what_the_writer_compares() {
    // Two documents that differ in the words alone are the same to `passthrough_but_snap_words`; a difference elsewhere is not.
    let a = with(vec![(0, settings(&[("TopPage", "0")], vec![text("SnapSettings", "3"), text("ProtectStyles", "0")])), (1, empty("EventList"))]);
    let mut b = a.clone();
    b.set_snap_words(&words(Some("9"), Some("1"), None, Some("1")));
    assert_ne!(a.passthrough, b.passthrough);
    assert_eq!(a.passthrough_but_snap_words(), b.passthrough_but_snap_words());
    let mut c = a.clone();
    c.passthrough.push(2, empty("Extra"));
    assert_ne!(a.passthrough_but_snap_words(), c.passthrough_but_snap_words());
}

#[test]
fn the_comparison_in_place_gives_what_the_comparison_of_copies_does() {
    // The writer asks this at every save, and it copies nothing.
    let a = with(vec![(0, settings(&[("TopPage", "0")], vec![text("SnapSettings", "3"), text("ProtectStyles", "0")])), (1, empty("EventList"))]);
    let mut b = a.clone();
    b.set_snap_words(&words(Some("9"), Some("1"), None, Some("1")));
    assert!(a.same_but_snap_words(&b) && b.same_but_snap_words(&a));
    // A difference elsewhere: another node, a changed attribute, a changed other child, another place.
    let mut c = a.clone();
    c.passthrough.push(2, empty("Extra"));
    assert!(!a.same_but_snap_words(&c) && !c.same_but_snap_words(&a));
    let d = with(vec![(0, settings(&[("TopPage", "1")], vec![text("SnapSettings", "3"), text("ProtectStyles", "0")])), (1, empty("EventList"))]);
    assert!(!a.same_but_snap_words(&d));
    let e = with(vec![(0, settings(&[("TopPage", "0")], vec![text("SnapSettings", "3"), text("ProtectStyles", "1")])), (1, empty("EventList"))]);
    assert!(!a.same_but_snap_words(&e));
    let f = with(vec![(0, settings(&[("TopPage", "0")], vec![text("SnapSettings", "3"), text("ProtectStyles", "0")])), (2, empty("EventList"))]);
    assert!(!a.same_but_snap_words(&f));
    // Settings that hold the words alone are nothing, on either side.
    let mut words_only = Document::new_blank();
    words_only.set_snap_words(&words(Some("9"), Some("3"), None, None));
    assert!(words_only.same_but_snap_words(&Document::new_blank()) && Document::new_blank().same_but_snap_words(&words_only));
    assert!(!words_only.holds_more_than_snap_words() && a.holds_more_than_snap_words());
    // The same answer as the comparison of the two lists.
    for (x, y) in [(&a, &b), (&a, &c), (&a, &d), (&a, &e), (&a, &f), (&words_only, &a)] {
        assert_eq!(x.same_but_snap_words(y), x.passthrough_but_snap_words() == y.passthrough_but_snap_words());
    }
}

#[test]
fn settings_that_held_only_words_are_nothing_apart_from_them() {
    // A new drawing's settings made by the model hold the words alone and no attribute: the writer's skeleton adds the attributes.
    let mut doc = Document::new_blank();
    doc.set_snap_words(&words(Some("9"), Some("3"), None, None));
    assert!(doc.passthrough_but_snap_words().is_empty());
    // With an attribute they are not nothing.
    let doc = with(vec![(0, settings(&[("TopPage", "0")], vec![text("SnapSettings", "3")]))]);
    assert_eq!(doc.passthrough_but_snap_words().len(), 1);
}

#[test]
fn a_word_a_hostile_file_made_huge_is_read_and_kept_whole() {
    let big = "9".repeat(1 << 20);
    let doc = with(vec![(0, settings(&[], vec![text("SnapSettings", &big)]))]);
    assert_eq!(doc.snap_words().snap.as_deref().map(str::len), Some(1 << 20));
}

/// A word element that declares the namespace of its own name.
fn foreign(name: &str, declare: (&str, &str), value: &str) -> RawNode {
    RawNode::Element(RawElement { name: Arc::from(name), attrs: RawAttrs::from_pairs([declare]), children: vec![RawNode::Text(Arc::from(value))] })
}

#[test]
fn an_element_named_like_a_word_in_another_namespace_is_not_the_word() {
    // The model took the first element by its local name, so a `SnapSettings` of another application's was read and written while the
    // writer, which resolves namespaces, wrote the real one.
    let mut doc = with(vec![(
        0,
        settings(
            &[("TopPage", "0")],
            vec![
                text("GlueSettings", "9"),
                foreign("SnapSettings", ("xmlns", "urn:x"), "5"),
                foreign("x:SnapExtensions", ("xmlns:x", "urn:x"), "6"),
                text("SnapSettings", "773"),
                text("SnapExtensions", "34"),
            ],
        ),
    )]);
    let read = doc.snap_words();
    assert_eq!((read.snap.as_deref(), read.extensions.as_deref(), read.glue.as_deref()), (Some("773"), Some("34"), Some("9")));
    doc.set_snap_words(&words(Some("9"), Some("775"), Some("98"), None));
    let after = doc.snap_words();
    assert_eq!((after.snap.as_deref(), after.extensions.as_deref()), (Some("775"), Some("98")));
    // The other application's elements are as they were, and are not words: the comparison of what is not a word sees them.
    let mut kept = 0;
    for placed in doc.passthrough.iter() {
        let RawNode::Element(el) = &placed.node else { continue };
        for child in &el.children {
            if let RawNode::Element(c) = child
                && (c.attrs.get("xmlns") == Some("urn:x") || c.attrs.get("xmlns:x") == Some("urn:x"))
            {
                assert!(matches!(c.children.as_slice(), [RawNode::Text(t)] if &**t == "5" || &**t == "6"), "{c:?}");
                kept += 1;
            }
        }
    }
    assert_eq!(kept, 2);
    let rest = doc.passthrough_but_snap_words();
    assert_eq!(rest.len(), 1, "the settings still hold the foreign elements and the attribute");
    // The namespaces of the target application's files, or none, are the file's own; the settings in another one are not the settings.
    let ours = with(vec![(
        0,
        settings(
            &[],
            vec![
                foreign("SnapSettings", ("xmlns", "http://schemas.microsoft.com/office/visio/2012/main"), "3"),
                foreign("GlueSettings", ("xmlns", ""), "9"),
            ],
        ),
    )]);
    assert_eq!((ours.snap_words().snap.as_deref(), ours.snap_words().glue.as_deref()), (Some("3"), Some("9")));
    let mut other = with(vec![(0, foreign("DocumentSettings", ("xmlns", "urn:x"), "ignored"))]);
    assert_eq!(other.snap_words(), SnapWords::default());
    other.set_snap_words(&words(None, Some("3"), None, None));
    assert_eq!(other.snap_words().snap.as_deref(), Some("3"), "a settings element of our own is made beside it");
    assert_eq!(other.passthrough.iter().count(), 2);
}
