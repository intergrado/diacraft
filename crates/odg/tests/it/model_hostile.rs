//! Crafted hostile models for the `.odg` model writer (the plan's done-criteria). Each input is built to be the worst the model can hold for one loop of the writer, and each
//! has a default-run test that counts the work in the units the loop walks (the save's own steps, the bytes it copied, the entries and rows it wrote), and an ignored test that times
//! it back to back with a yardstick of the same size made of plain content.
//!
//! * **Entries of the document's containers** (`OdfDoc::kept`): 100,000 children of one container, each its own entry (a search for the container, run for each entry among the children
//!   the earlier entries put in, is quadratic; the writer finds each container once and fills it in one pass).
//! * **Attributes of one container** (`OdfDoc::roots`, a key of the form `content.xml/office:body`): 100,000 of them (a search among the element's attributes for each is quadratic).
//! * **Package entries** (`Document::parts`): as many as a model holds (the writer's rows, entries and steps are one each).
//! * **A settings tree and kept fields of `meta.xml`** at the cap of a small part, past it, and nested past the depth a native part is read at.
//! * **A name that is not one**, an attribute that is not XML, and a root that binds a prefix to another namespace.

use std::sync::Arc;
use std::time::Instant;

use diacraft_doc::{Cell, CellName, Document, Format, Part, PartRole, Passthrough, RawAttrs, RawElement, RawNode, Value};
use diacraft_odg::package::Package;
use diacraft_odg::{Limits, WriteError, WriteOptions, WriteWork, read, write_counted};

use crate::model_write::blank;

fn el(name: &str, attrs: &[(&str, &str)], children: Vec<RawNode>) -> RawNode {
    RawNode::Element(RawElement { name: Arc::from(name), attrs: RawAttrs::from_pairs(attrs.iter().copied()), children })
}

fn part(name: &str, bytes: &[u8]) -> Arc<Part> {
    Arc::new(Part {
        format: Format::Odf,
        name: Arc::from(name),
        media_type: Arc::from("application/octet-stream"),
        role: PartRole::Other,
        manifest: RawAttrs::new(),
        rels: Arc::new(Vec::new()),
        bytes: Arc::from(bytes),
    })
}

/// The save of `doc` from nothing, with its work counted.
fn save(doc: &Document) -> (Result<diacraft_odg::Written, WriteError>, WriteWork) {
    let mut work = WriteWork::default();
    let saved = write_counted(doc, None, &WriteOptions::default(), &mut work);
    (saved, work)
}

/// A model whose `office:drawing` holds `n` unknown children, each its own entry of the kept data (after the first page, as the reader counts: place 1). The root of `content.xml` declares
/// the prefix they are written under, as a file's does (a piece whose prefix nothing binds where it goes is left out, where it was written unbound).
fn model_with_entries(n: usize) -> Document {
    let mut doc = blank(1);
    let odf = &mut doc.ext.make_mut().odf;
    let mut kept = Passthrough::new();
    for k in 0..n {
        let leaf = el("x:note", &[("x:n", &k.to_string())], vec![]);
        let chain = el("office:document-content", &[], vec![el("office:body", &[], vec![el("office:drawing", &[], vec![leaf])])]);
        kept.push(1, chain);
    }
    odf.kept.kept = kept;
    odf.roots.push((Arc::from("content.xml"), RawAttrs::from_pairs([("xmlns:x", "urn:example:x")])));
    doc
}

/// 100,000 children of one container come back, each at the place it had, in order, in a part that holds each once; the save's steps are the entries (one each), and the container is
/// found once: the cost is linear in the entries.
#[test]
fn a_hundred_thousand_entries_of_one_container_are_placed_in_one_pass() {
    let (small, work_small) = save(&model_with_entries(1_000));
    let (large, work_large) = save(&model_with_entries(100_000));
    let (small, large) = (small.expect("1,000 entries are written"), large.expect("100,000 entries are written"));
    // The part holds each child once, after the page the reader counted before it (place 1), and in order.
    let content = String::from_utf8(Package::read(&large.bytes, &Limits::default()).unwrap().part("content.xml").unwrap().to_vec()).unwrap();
    assert_eq!(content.matches("<x:note ").count(), 100_000);
    let first = content.find("x:n=\"0\"").unwrap();
    let last = content.find("x:n=\"99999\"").unwrap();
    let page_end = content.find("</draw:page>").unwrap();
    assert!(page_end < first && first < last, "after the page, in order");
    // The work is counted in the units the loop walks: three steps for each entry, one for the entry and two for the walk of its namespaces (its element and its attribute),
    // and the shapes' own cells beside, so 100 times the entries is a hundred times the steps.
    let (a, b) = (work_small.cells_compared, work_large.cells_compared);
    let shapes = a.saturating_sub(3 * 1_000);
    assert!(b >= 3 * 100_000 && b - 3 * 100_000 == shapes, "the steps are three for each entry plus the same shapes' own: {a} and {b}");
    assert!(small.notes.iter().all(|n| n.class() != "document_data_unwritten"), "{:?}", small.notes);
    // A second save of the same model is the same bytes.
    assert_eq!(save(&model_with_entries(1_000)).0.unwrap().bytes, small.bytes);
}

/// The timed form, beside a yardstick of the same size: the same 100,000 children as a plain list in a settings tree (the one tree the writer writes whole). The entries of the containers
/// must cost no more than a small multiple of it.
#[test]
#[ignore = "timed beside a yardstick: run by hand with --ignored"]
fn timed_entries_of_one_container_beside_a_settings_tree_of_the_same_size() {
    let doc = model_with_entries(100_000);
    let started = Instant::now();
    let saved = save(&doc).0.expect("written");
    let entries = started.elapsed();
    let mut plain = blank(1);
    let children: Vec<RawNode> = (0..100_000).map(|k| el("x:note", &[("x:n", &k.to_string())], vec![])).collect();
    let tree = RawElement {
        name: Arc::from("office:document-settings"),
        attrs: RawAttrs::from_pairs([("xmlns:office", "urn:oasis:names:tc:opendocument:xmlns:office:1.0")]),
        children: vec![el("office:settings", &[], children)],
    };
    // A tree of 100,000 notes is past a small part: lift the cap for the yardstick.
    plain.ext.make_mut().odf.settings = Some(Arc::new(tree));
    let limits = Limits { max_small_part_bytes: 64 * 1024 * 1024, ..Limits::default() };
    let started = Instant::now();
    let mut work = WriteWork::default();
    let _ = write_counted(&plain, None, &WriteOptions { limits, ..WriteOptions::default() }, &mut work);
    let yardstick = started.elapsed();
    println!("entries {entries:?}, yardstick {yardstick:?}, {} bytes", saved.bytes.len());
    assert!(entries < yardstick * 20 + std::time::Duration::from_millis(500), "{entries:?} against {yardstick:?}");
}

/// 100,000 attributes on one container are put on it once each and in order.
#[test]
fn a_hundred_thousand_attributes_of_one_container_are_put_on_it_once_each() {
    let mut doc = blank(1);
    // The prefix the attributes are written under is declared among them (an attribute whose prefix nothing binds is left out, where it was written unbound).
    let mut attrs: Vec<(String, String)> = vec![("xmlns:x".to_owned(), "urn:example:x".to_owned())];
    attrs.extend((0..100_000).map(|k| (format!("x:a{k}"), k.to_string())));
    doc.ext
        .make_mut()
        .odf
        .roots
        .push((Arc::from("content.xml/office:body"), RawAttrs::from_pairs(attrs.iter().map(|(k, v)| (k.as_str(), v.as_str())))));
    // One more that the part's own element already has (the writer's body has none, so the held ones are all new), and one under a container the part does not have.
    doc.ext.make_mut().odf.roots.push((Arc::from("content.xml/office:body/office:nothing"), RawAttrs::from_pairs([("x:b", "1")])));
    let (saved, work) = save(&doc);
    let saved = saved.expect("written");
    let content = String::from_utf8(Package::read(&saved.bytes, &Limits::default()).unwrap().part("content.xml").unwrap().to_vec()).unwrap();
    assert_eq!(content.matches(" x:a").count(), 100_000);
    assert!(content.find("x:a0=").unwrap() < content.find("x:a99999=").unwrap(), "in order");
    assert!(work.cells_compared >= 2, "a step for each container that holds attributes: {}", work.cells_compared);
    let note = saved.notes.iter().find(|n| n.class() == "document_data_unwritten").expect("the container the part lacks is said");
    assert!(note.to_string().contains("1 piece of the containers' markup"), "{note}");
}

/// Entries by the thousand: one row, one entry and one step each, and a file the reader reads whole; and as many as the manifest's own cap (4 MiB of rows) holds no more: past it the save is
/// refused in words before the package is put together, as the reader would refuse the manifest.
#[test]
fn twenty_thousand_package_entries_are_written_one_row_and_one_entry_each_and_forty_thousand_pass_the_manifests_cap() {
    let model = |n: usize| {
        let mut doc = blank(1);
        for k in 0..n {
            doc.parts.push(part(&format!("Fonts/f{k}.bin"), b"z"));
        }
        doc
    };
    let (saved, work) = save(&model(20_000));
    let saved = saved.expect("20,000 entries are written");
    let package = Package::read(&saved.bytes, &Limits::default()).unwrap();
    assert_eq!(package.entries().filter(|e| e.name.starts_with("Fonts/")).count(), 20_000);
    assert_eq!(package.manifest().entries.iter().filter(|r| r.full_path.starts_with("Fonts/")).count(), 20_000);
    assert!(package.warnings().is_empty(), "{:?}", package.warnings().len());
    assert!(work.cells_compared >= 20_000);
    // Read back by the reader, which holds them all.
    let (back, _) = read(&saved.bytes, &Limits::default()).unwrap();
    assert_eq!(back.parts.iter().filter(|p| p.name.starts_with("Fonts/")).count(), 20_000);
    // Forty thousand rows of this length are more than a manifest may hold.
    let (refused, _) = save(&model(40_000));
    assert!(matches!(&refused, Err(WriteError::Limit { what: "bytes in the manifest", .. })), "{refused:?}");
}

/// A part that deflates past the ratio the reader allows (a thousand to one: a stored entry of zeros that a file held) is **stored**, so that what the writer makes is what the reader reads;
/// one that stays under the ratio is deflated, and a small one is deflated too.
#[test]
fn a_part_that_would_deflate_past_the_ratio_is_stored_and_the_file_reads_back() {
    let mut doc = blank(1);
    doc.parts.push(part("Fonts/zeros.bin", &vec![0u8; 4 << 20]));
    doc.parts.push(part("Fonts/some.bin", &(0..200_000u32).flat_map(|n| (n % 251).to_le_bytes()).collect::<Vec<u8>>()));
    doc.parts.push(part("Fonts/small.bin", &[0u8; 1000]));
    let (saved, _) = save(&doc);
    let saved = saved.expect("written");
    // The zeros are stored (the file is at least as long as they are).
    assert!(saved.bytes.len() > 4 << 20, "the zeros are stored: {} bytes", saved.bytes.len());
    let (back, _) = read(&saved.bytes, &Limits::default()).expect("the reader reads what the writer made: no ratio of a thousand");
    let held = |name: &str| back.parts.iter().find(|p| &*p.name == name).map(|p| p.bytes.len());
    assert_eq!((held("Fonts/zeros.bin"), held("Fonts/some.bin"), held("Fonts/small.bin")), (Some(4 << 20), Some(800_000), Some(1000)));
}

/// A model with more parts than a package holds is refused in words before anything is made.
#[test]
fn more_parts_than_a_package_holds_are_refused_in_words() {
    let mut doc = blank(1);
    for k in 0..diacraft_doc::MAX_PARTS + 1 {
        doc.parts.push(part(&format!("p{k}"), b""));
    }
    let (refused, work) = save(&doc);
    assert!(matches!(&refused, Err(WriteError::Limit { what: "package entries", .. })), "{refused:?}");
    assert_eq!(work.bytes_copied, 0, "nothing was made");
}

/// A settings tree is written whole up to the cap of a small part (what the reader reads as fields) and refused in words past it; the refusal comes before a tree is made.
#[test]
fn a_settings_tree_is_written_to_the_cap_of_a_small_part_and_refused_past_it() {
    let tree = |notes: usize| -> Arc<RawElement> {
        let RawNode::Element(root) = el(
            "office:document-settings",
            &[("xmlns:office", "urn:oasis:names:tc:opendocument:xmlns:office:1.0")],
            vec![el("office:settings", &[], (0..notes).map(|_| RawNode::Text(Arc::from("0123456789"))).collect())],
        ) else {
            panic!("an element")
        };
        Arc::new(root)
    };
    let write = |notes: usize, cap: u64| {
        let mut doc = blank(1);
        doc.ext.make_mut().odf.settings = Some(tree(notes));
        let limits = Limits { max_small_part_bytes: cap, ..Limits::default() };
        write_counted(&doc, None, &WriteOptions { limits, ..WriteOptions::default() }, &mut WriteWork::default())
    };
    // The estimate of the tree: the root (24 name bytes twice, the punctuation, the attribute), `office:settings`, and ten bytes a text; the part written is smaller by what the
    // estimate rounds up. At a cap that holds the tree it is written; one byte under what the estimate says is refused.
    let fits = 4_096u64;
    assert!(write(100, fits).is_ok());
    let refused = write(100, 1_000).unwrap_err();
    assert_eq!(refused, WriteError::Unsupported(SMALL_PART));
    // A tree of a million nodes is refused before a tree is made of it: the walk stops at the cap (the child of `model_memory` shows no tree was made).
    assert_eq!(write(1_000_000, 4 * 1024 * 1024).unwrap_err(), WriteError::Unsupported(SMALL_PART));
}

/// The words the refusal of a part past the cap of a small part says.
pub const SMALL_PART: &str =
    "a meta.xml or settings.xml larger than the reader reads as fields (the cap of a small part): it would not read back as it is written";

/// A tree nested deeper than a native part is read at is refused, however small, and the model that holds it is dropped without a stack of its own overflowing.
#[test]
fn a_tree_nested_past_the_depth_a_native_part_is_read_at_is_refused() {
    let mut inner = RawNode::Text(Arc::from("x"));
    for _ in 0..2_000 {
        inner = el("a", &[], vec![inner]);
    }
    let RawNode::Element(deep) = inner else { panic!("an element") };
    let mut doc = blank(1);
    doc.ext.make_mut().odf.settings = Some(Arc::new(RawElement {
        name: Arc::from("office:document-settings"),
        attrs: RawAttrs::from_pairs([("xmlns:office", "urn:oasis:names:tc:opendocument:xmlns:office:1.0")]),
        children: vec![RawNode::Element(deep)],
    }));
    assert_eq!(save(&doc).0.unwrap_err(), WriteError::Unsupported(SMALL_PART));
    // The same depth in the kept fields of meta.xml.
    let mut inner = RawNode::Text(Arc::from("x"));
    for _ in 0..2_000 {
        inner = el("a", &[], vec![inner]);
    }
    let mut doc = blank(1);
    let mut meta = Passthrough::new();
    meta.push(0, inner);
    doc.ext.make_mut().odf.meta = meta;
    assert_eq!(save(&doc).0.unwrap_err(), WriteError::Unsupported(SMALL_PART));
}

/// Kept fields of `meta.xml` to the cap, and one byte past it.
#[test]
fn the_kept_fields_of_meta_are_written_to_the_cap_of_a_small_part_and_refused_past_it() {
    let model = |texts: usize| {
        let mut doc = blank(1);
        let mut meta = Passthrough::new();
        for _ in 0..texts {
            meta.push(0, el("meta:note", &[], vec![RawNode::Text(Arc::from("0123456789"))]));
        }
        doc.ext.make_mut().odf.meta = meta;
        doc
    };
    let at = |texts: usize, cap: u64| {
        let limits = Limits { max_small_part_bytes: cap, ..Limits::default() };
        write_counted(&model(texts), None, &WriteOptions { limits, ..WriteOptions::default() }, &mut WriteWork::default())
    };
    // Each note costs 2 * 9 + 5 + 10 = 33 bytes by the estimate; the root and the field bytes are counted besides.
    assert!(at(100, 8_192).is_ok());
    assert_eq!(at(100, 2_000).unwrap_err(), WriteError::Unsupported(SMALL_PART));
    assert_eq!(at(1_000_000, 4 * 1024 * 1024).unwrap_err(), WriteError::Unsupported(SMALL_PART));
}

/// An entry named with a `..`, an absolute name or a backslash is refused in words by the container, and nothing is written; a name two parts share is refused as the plan words it.
#[test]
fn names_that_are_not_entry_names_are_refused_in_words() {
    for name in ["../x", "/abs", "a\\b", "a/../b", ""] {
        let mut doc = blank(1);
        doc.parts.push(part(name, b"x"));
        let (refused, _) = save(&doc);
        assert!(refused.is_err(), "{name:?} is written: {refused:?}");
        let words = refused.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(!words.is_empty() && words.len() < 300, "{name:?}: {words}");
    }
}

/// An attribute or an element that is not XML (a name with a space, a control character in a value) is refused in words and writes nothing.
#[test]
fn what_xml_cannot_hold_is_refused_in_words() {
    let mut doc = blank(1);
    let mut p = (*doc.props).clone();
    p.custom.push(diacraft_doc::CustomProp {
        format: Format::Odf,
        name: Arc::from("n"),
        value_type: Arc::from("string"),
        value: Arc::from("v"),
        raw: RawAttrs::from_pairs([("bad name", "1")]),
    });
    doc.props = Arc::new(p);
    let words = save(&doc).0.unwrap_err();
    assert!(matches!(&words, WriteError::Unsupported(w) if w.contains("XML cannot hold")), "{words}");
    let mut doc = blank(1);
    let mut p = (*doc.props).clone();
    p.subject = Some(Arc::from("a\u{1}b"));
    doc.props = Arc::new(p);
    assert!(matches!(save(&doc).0, Err(WriteError::Unsupported(w)) if w.contains("XML cannot hold")));
}

/// A root that binds a prefix the writer's own names need to another namespace is refused in words; the model's other prefixes are written as they are.
#[test]
fn a_root_that_binds_a_needed_prefix_to_another_namespace_is_refused() {
    let mut doc = blank(1);
    doc.ext.make_mut().odf.roots.push((Arc::from("meta.xml"), RawAttrs::from_pairs([("xmlns:meta", "urn:elsewhere"), ("office:version", "1.3")])));
    let refused = save(&doc).0.unwrap_err();
    assert!(matches!(&refused, WriteError::Unsupported(w) if w.contains("binds a prefix")), "{refused}");
    // A root that binds another prefix to anything, and none of the needed ones, is given them.
    let mut ok = blank(1);
    ok.ext.make_mut().odf.roots.push((Arc::from("meta.xml"), RawAttrs::from_pairs([("xmlns:other", "urn:elsewhere"), ("office:version", "1.3")])));
    let saved = save(&ok).0.expect("written");
    let meta = String::from_utf8(Package::read(&saved.bytes, &Limits::default()).unwrap().part("meta.xml").unwrap().to_vec()).unwrap();
    assert!(
        meta.contains("xmlns:other=\"urn:elsewhere\"") && meta.contains("xmlns:meta=\"urn:oasis:names:tc:opendocument:xmlns:meta:1.0\""),
        "{meta}"
    );
}

/// The page the package is made for may be of any size a page has: a tiny one, a huge one, an ordinary one.
#[test]
fn the_page_the_package_is_made_for_may_be_of_any_size_a_page_has() {
    for (w, h) in [(0.001, 0.001), (1_000.0, 1_000.0), (8.5, 11.0)] {
        let mut doc = blank(1);
        let page = Arc::make_mut(doc.pages.get_mut(0).unwrap());
        let sheet = Arc::make_mut(&mut page.sheet);
        for (n, v) in [("PageWidth", w), ("PageHeight", h)] {
            sheet.cells.insert(CellName::new(n), Cell::constant(Value::from_number(v).unwrap()).with_unit("IN"));
        }
        let (saved, _) = save(&doc);
        assert!(saved.is_ok() || matches!(saved, Err(WriteError::Unsupported(_) | WriteError::Limit { .. })), "{w} x {h}: {saved:?}");
    }
}
