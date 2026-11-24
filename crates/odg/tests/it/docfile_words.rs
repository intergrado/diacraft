//! Item 2: the Snap & Glue words of `DocumentSettings` in DiaCraft's document file ("the other snap and glue flags are tier 2
//! in DiaCraft's document file"; the child is `words`, proposed in the report).
//!
//! The words are the four children of the model's `DocumentSettings` node (`Document::snap_words`). A package compared the whole of the document's unknown nodes and
//! refused a change of them; it compares them but for the words now, and the file carries the words.

use std::sync::Arc;

use diacraft_doc::{Document, RawAttrs, RawElement, RawNode, SnapWords};
use diacraft_odg::package::Package;
use diacraft_odg::{Limits, WriteError, read};

use crate::docfile_cost::{file_of, items, package_with};
use crate::drawing::Drawing;
use crate::write_core::{open, open_bytes, part, real};

const PATH: &str = "DiaCraft/document.xml";

fn base() -> crate::write_core::Open {
    open(&Drawing::a4().page(&crate::write_core::custom("s1", "1cm")))
}

fn words(glue: Option<&str>, snap: Option<&str>, extensions: Option<&str>, dynamic: Option<&str>) -> SnapWords {
    let word = |w: Option<&str>| w.map(Arc::from);
    SnapWords { glue: word(glue), snap: word(snap), extensions: word(extensions), dynamic_grid: word(dynamic) }
}

fn refusal(doc: &Document, o: &crate::write_core::Open) -> String {
    match real(doc, &o.bytes, &o.index) {
        Err(WriteError::Unsupported(words)) => words.to_owned(),
        Err(other) => panic!("another refusal than the writer's words: {other}"),
        Ok(saved) => panic!("a save that dropped what the model holds: {} bytes", saved.bytes.len()),
    }
}

/// The four words are in the file as the attributes of `words`, in the order of the model, and nothing else is; the manifest lists the file; they read back; a
/// second save is no edit. The file is exactly this text (as the is for a cell and a colour).
#[test]
fn the_snap_and_glue_words_are_carried_by_the_document_file_and_read_back() {
    let mut o = base();
    o.doc.set_snap_words(&words(Some("9"), Some("65845"), Some("34"), Some("1")));
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    assert!(saved.edited);
    assert_eq!(
        part(&saved.bytes, PATH),
        concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n",
            r#"<diacraft:document xmlns:diacraft="urn:app.diacraft:extension" version="2">"#,
            r#"<diacraft:words glue="9" snap="65845" extensions="34" dynamic-grid="1"/></diacraft:document>"#
        )
    );
    let package = Package::read(&saved.bytes, &Limits::default()).unwrap();
    let row = package.manifest().entries.iter().find(|e| e.full_path == PATH).expect("listed in the manifest");
    assert_eq!(row.media_type.as_deref(), Some("application/vnd.diacraft.document+xml"));
    let (back, report) = read(&saved.bytes, &Limits::default()).unwrap();
    assert_eq!(back.snap_words(), o.doc.snap_words(), "the words come back");
    assert_eq!(report.warnings, Vec::<String>::new());
    assert_eq!(back.passthrough.len(), 1, "as the one node the model keeps them in");
    // Saved again, no edit; and the model read back is saved as no edit too.
    let again = real(&o.doc, &saved.bytes, &saved.index).unwrap();
    assert!(!again.edited && again.bytes == saved.bytes);
    let reread = open_bytes(saved.bytes.clone());
    let third = real(&reread.doc, &reread.bytes, &reread.index).unwrap();
    assert!(!third.edited && third.bytes == saved.bytes);
}

/// A word the document lacks is no attribute: only the words the model has are written, and a model with one word is a file with one.
#[test]
fn a_word_the_document_lacks_is_not_written() {
    let mut o = base();
    o.doc.set_snap_words(&words(None, Some("65845"), None, None));
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap();
    assert!(part(&saved.bytes, PATH).contains(r#"<diacraft:words snap="65845"/>"#), "{}", part(&saved.bytes, PATH));
    assert_eq!(read(&saved.bytes, &Limits::default()).unwrap().0.snap_words(), words(None, Some("65845"), None, None));
}

/// A change of one word of a file that carries them rewrites the file; a model that no longer has any word takes the file out, with its row, when it holds nothing
/// else; one that holds something else keeps it.
#[test]
fn a_changed_word_rewrites_the_file_and_no_word_takes_it_out() {
    let mut o = base();
    o.doc.set_snap_words(&words(Some("9"), Some("65847"), Some("34"), Some("1")));
    let first = real(&o.doc, &o.bytes, &o.index).unwrap();
    let mut changed = open_bytes(first.bytes.clone());
    changed.doc.set_snap_words(&words(Some("9"), Some("65845"), Some("34"), Some("1")));
    let second = real(&changed.doc, &changed.bytes, &changed.index).unwrap();
    assert!(second.edited && part(&second.bytes, PATH).contains(r#"snap="65845""#));
    // The words out of the model: the file and its row go.
    let mut none = open_bytes(second.bytes.clone());
    none.doc.set_snap_words(&SnapWords::default());
    let third = real(&none.doc, &none.bytes, &none.index).unwrap();
    assert!(third.edited);
    let package = Package::read(&third.bytes, &Limits::default()).unwrap();
    assert!(package.part(PATH).is_none() && !package.manifest().entries.iter().any(|e| e.full_path == PATH));
    assert_eq!(read(&third.bytes, &Limits::default()).unwrap().0.snap_words(), SnapWords::default());
    // With a colour in the model beside the words, taking them out keeps the file and what is in it.
    let mut both = open_bytes(second.bytes.clone());
    Arc::make_mut(&mut both.doc.colors).push(diacraft_doc::ColorEntry { ix: 24, rgb: Arc::from("#112233") });
    both.doc.set_snap_words(&SnapWords::default());
    let kept = real(&both.doc, &both.bytes, &both.index).unwrap();
    let text = part(&kept.bytes, PATH);
    assert!(text.contains("diacraft:colors") && !text.contains("diacraft:words"), "{text}");
}

/// The words are all the package compares of the unknown nodes that hold them: a `DocumentSettings` the model made for the words is no change of anything else, and
/// **another node, or another child of `DocumentSettings`, is still refused in words**. (Before this task the words were refused with it.)
#[test]
fn only_the_words_are_set_free_of_the_comparison_of_the_documents_unknown_nodes() {
    let o = base();
    let node = |name: &str| RawNode::Element(RawElement { name: Arc::from(name), attrs: RawAttrs::from_pairs([("k", "v")]), children: Vec::new() });
    // An unknown node is refused, with the words beside it or not.
    let mut other = o.doc.clone();
    other.passthrough.push(0, node("Elsewhere"));
    assert!(refusal(&other, &o).contains("document's own fields"), "an unknown node");
    other.set_snap_words(&words(Some("9"), None, None, None));
    assert!(refusal(&other, &o).contains("document's own fields"), "an unknown node and a word");
    // A `DocumentSettings` that holds something besides the words is another node.
    let mut settings = o.doc.clone();
    settings.set_snap_words(&words(Some("9"), None, None, None));
    assert_eq!(settings.passthrough.iter().count(), 1);
    let with_child = {
        let mut d = o.doc.clone();
        let mut el = RawElement { name: Arc::from("DocumentSettings"), attrs: RawAttrs::new(), children: Vec::new() };
        el.children.push(node("SnapAngles"));
        d.passthrough.push(0, RawNode::Element(el));
        d
    };
    assert!(refusal(&with_child, &o).contains("document's own fields"), "a child of DocumentSettings that is no word");
    // The words alone are no refusal.
    assert!(real(&settings, &o.bytes, &o.index).is_ok());
}

/// A flat drawing has no document file: its whole `passthrough` is compared, the words included, and a change of them is refused in words and never dropped.
#[test]
fn a_change_of_the_words_is_refused_in_a_flat_drawing() {
    let flat = crate::flat_write::three().bytes();
    let mut o = open_bytes(flat);
    o.doc.set_snap_words(&words(Some("9"), None, None, None));
    match crate::flat_write::save(&o.doc, &o.bytes, &o.index) {
        Err(WriteError::Unsupported(words)) => assert!(words.contains("document's own fields"), "{words}"),
        Err(other) => panic!("another refusal than the writer's words: {other}"),
        Ok(saved) => panic!("a save that dropped the words: {} bytes", saved.bytes.len()),
    }
}

/// Each word the reader copies is charged by its bytes before it is copied, over the backstop's charge of the file: 1.8 times the bytes' worth, at 64 bytes an item
/// (the pattern of `docfile_charges`).
#[test]
fn the_words_of_a_file_are_charged_by_their_bytes() {
    const N: usize = 400_000;
    let big = "7".repeat(N);
    let one = |name: &str, v: &str| file_of(&format!(r#"<diacraft:words {name}="{v}"/>"#));
    for name in ["glue", "snap", "extensions", "dynamic-grid"] {
        let (large, tiny) = (items(&one(name, &big)), items(&one(name, "7")));
        let backstop = (N / 64) as u64;
        assert!(
            large - tiny >= backstop * 18 / 10,
            "{name}: {N} bytes cost {} items, fewer than 1.8 times the backstop's {backstop}: not charged",
            large - tiny
        );
    }
}

/// An element `words` repeated many times costs an item for itself and each of its attributes, and the last word named wins; an attribute that is no word is
/// ignored; a file of none reads the document with no words.
#[test]
fn many_words_elements_are_charged_each_and_the_last_wins() {
    let many = |n: usize| file_of(&r#"<diacraft:words glue="1" snap="2"/>"#.repeat(n));
    let (none, lots) = (items(&many(0)), items(&many(2_000)));
    assert!(lots - none >= 2_000 * 3, "2,000 elements of two attributes cost {} items, fewer than three each", lots - none);
    let drawing = package_with(&file_of(r#"<diacraft:words glue="1" snap="2" unknown="x"/><diacraft:words snap="3" dynamic-grid="0"/>"#));
    let (doc, report) = read(&drawing, &Limits::default()).unwrap();
    assert_eq!(doc.snap_words(), words(Some("1"), Some("3"), None, Some("0")), "the later element replaces the word it names and leaves the others");
    assert_eq!(report.warnings, Vec::<String>::new());
    let (doc, _) = read(&package_with(&file_of("<diacraft:words/>")), &Limits::default()).unwrap();
    assert_eq!(doc.snap_words(), SnapWords::default());
    assert!(doc.passthrough.is_empty(), "an element with no word makes no DocumentSettings");
}
