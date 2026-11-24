//! The masters of DiaCraft's document file are written and read by the whole-model codec (`sheetxml::model`), and the file's root says
//! `version="2"`. A file of the old version, which no release wrote, is kept as it is and not read.
//!
//! Every case saves a model, reads the file back through the real reader (`diacraft_odg::read`), and compares what came back with what went in. The hostile cases are
//! built by hand, as a hostile file is.

use std::sync::Arc;

use diacraft_container::XmlLimits;
use diacraft_container::xml::{El, Node, parse_with};
use diacraft_doc::{
    Connects, ForeignData, Kept, Master, Media, Passthrough, RawAttrs, RawElement, RawNode, Shape, ShapeList, ShapeType, SnapWords, Text, TextPart,
};
use diacraft_odg::{Limits, read};

use crate::docfile::{a_master, base, body_shape};
use crate::docfile_cost::{file_of, package_with};
use crate::write_core::{open_bytes, part_bytes, real};

const PATH: &str = "DiaCraft/document.xml";
const NS: &str = "urn:app.diacraft:extension";

/// The document file of a saved package, parsed at the depth the reader parses it at.
fn tree(bytes: &[u8]) -> El {
    let limits = XmlLimits { max_depth: diacraft_container::limits::XML_DEPTH_CEILING, ..XmlLimits::default() };
    parse_with(&part_bytes(bytes, PATH), &limits).unwrap_or_else(|e| panic!("the document file is not XML: {e}"))
}

/// Every element of `root`, with its depth (the root is 1), in document order, without recursion.
fn elements(root: &El) -> Vec<(&El, usize)> {
    let mut out = Vec::new();
    let mut stack = vec![(root, 1usize)];
    while let Some((e, depth)) = stack.pop() {
        out.push((e, depth));
        for node in e.children.iter().rev() {
            if let Node::El(child) = node {
                stack.push((child, depth + 1));
            }
        }
    }
    out
}

/// A model with `master` saved into a drawing and read back: the bytes saved, and the master read.
pub fn through_the_file(master: Master) -> (Vec<u8>, Master) {
    let mut o = base();
    o.doc.masters.push(Arc::new(master));
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    let back = read(&saved.bytes, &Limits::default()).unwrap_or_else(|e| panic!("not read: {e}")).0;
    let got = back.masters.get(0).map(|m| (**m).clone()).expect("the master came back");
    (saved.bytes, got)
}

/// A kept element is written as data, never as literal XML (the design's 2.2): its name is an attribute of an `el` element, and what it declares is an `attr`
/// child, so the file holds no element of the kept element's own prefix and no declaration of it. A kept attribute named as a typed one cannot clash with it.
#[test]
fn a_master_bodys_kept_element_is_written_as_data() {
    let mut master = a_master(7);
    {
        let shape = Arc::make_mut(master.shapes.get_mut(0).expect("a shape"));
        // The typed `id`, `name` and `master` of the shape, and the zero-padded number the design's recon names, as kept attributes of the same names.
        shape.raw = RawAttrs::from_pairs([("id", "999"), ("name", "clash"), ("master", "5"), ("TextStyle", "007"), ("odd:name", "v")]);
        shape.passthrough.push(
            0,
            RawNode::Element(RawElement {
                name: Arc::from("x:other"),
                attrs: RawAttrs::from_pairs([("xmlns:x", "urn:x"), ("k", "v")]),
                children: vec![RawNode::Text(Arc::from("t"))],
            }),
        );
    }
    let (bytes, got) = through_the_file(master.clone());
    assert_eq!(got, master, "the master comes back whole, the clashing names included");
    let root = tree(&bytes);
    for (e, _) in elements(&root) {
        assert_ne!(e.name, "x:other", "no literal element of the kept element's name");
        assert!(e.attrs.iter().all(|(k, _)| k != "xmlns:x" && k != "odd:name"), "no attribute of a prefix the kept data declared: {:?}", e.attrs);
    }
    let el =
        elements(&root).into_iter().map(|(e, _)| e).find(|e| e.name == "diacraft:el" && e.attrs.iter().any(|(k, v)| k == "name" && v == "x:other"));
    assert!(el.is_some(), "the kept element is an `el` element that holds its name as an attribute");
}

/// The shapes of a master stand directly in the master, and a group's members directly in the group: no `shapes` wrapper, as the vocabulary of the whole-model codec has
/// it, and the master declares the namespace itself.
#[test]
fn the_master_is_the_codecs_own_element_with_no_shapes_wrapper() {
    let (bytes, _) = through_the_file(a_master(7));
    let root = tree(&bytes);
    assert_eq!(root.attr("version"), Some("2"), "the root says the version of this vocabulary");
    let names: Vec<&str> = elements(&root).into_iter().map(|(e, _)| e.name.as_str()).collect();
    assert!(!names.contains(&"diacraft:shapes"), "no wrapper element: {names:?}");
    let master = root.elements().find(|e| e.name == "diacraft:master").expect("the master");
    assert_eq!(master.attr("xmlns:diacraft"), Some(NS), "the master declares the namespace itself, as a part of the codec's does");
    assert!(master.elements().any(|e| e.name == "diacraft:shape"), "its shapes are its own children");
}

/// What the models of an `.odg` hold beside the cells and text a master's shapes have (held ODF markup, what a newer build wrote, the slots) is the codec's to carry,
/// and comes back.
#[test]
fn a_masters_held_and_unknown_content_survives_the_document_file() {
    let mut master = a_master(3);
    {
        let shape = Arc::make_mut(master.shapes.get_mut(1).expect("a second shape"));
        shape.text = Some(Arc::new(Text {
            parts: vec![
                TextPart::Chars(Arc::from("a")),
                TextPart::OdfRaw(Box::new(RawNode::Comment(Arc::from("odf")))),
                TextPart::NativeRaw(Box::new(RawNode::Comment(Arc::from("native")))),
                TextPart::Raw(RawNode::Comment(Arc::from("vsdx"))),
                TextPart::Chars(Arc::from("b\u{1}c")),
            ],
            ..Default::default()
        }));
        let ext = shape.ext.make_mut();
        ext.odf.style_name = Some(Arc::from("gr7"));
        ext.native = Kept { raw: RawAttrs::from_pairs([("future", "1")]), kept: Passthrough::default() };
    }
    let (_, got) = through_the_file(master.clone());
    assert_eq!(got, master);
}

/// A picture that many shapes of several masters show is one entry of the file's own list, whatever their part names say, and the base64 of it is in the file once.
#[test]
fn a_picture_shown_by_many_shapes_is_one_blob_of_the_document_file() {
    let bytes: Vec<u8> = (0..300u32).map(|k| u8::try_from(k * 7 % 251).unwrap_or(0)).collect();
    let make = |id: u32, part: &str| {
        let mut s = Shape::new(id);
        s.kind = ShapeType::Foreign;
        s.foreign = Some(Arc::new(ForeignData {
            picture: Some(Arc::new(Media { part: Arc::from(part), content_type: Arc::from("image/png"), bytes: Arc::from(bytes.clone()) })),
            ..ForeignData::default()
        }));
        Arc::new(s)
    };
    let mut o = base();
    for id in [1u32, 2] {
        let shapes = ShapeList::from_vec(vec![make(1, "media/a.png"), make(2, "media/b.png"), make(3, "media/a.png")]);
        o.doc.masters.push(Arc::new(Master { id, shapes, ..Master::default() }));
    }
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    let root = tree(&saved.bytes);
    let blobs: Vec<&El> = root.elements().filter(|e| e.name == "diacraft:blob").collect();
    assert_eq!(blobs.len(), 1, "six pictures of equal bytes are one blob");
    let encoded = blobs.first().map(|b| b.text()).unwrap_or_default();
    assert_eq!(part_text(&saved.bytes).matches(&encoded).count(), 1, "and its base64 is in the file once");
    let back = read(&saved.bytes, &Limits::default()).unwrap().0;
    assert_eq!(back.masters.len(), 2);
    for (made, got) in o.doc.masters.iter().zip(back.masters.iter()) {
        assert_eq!(**got, **made, "master {} comes back whole", made.id);
    }
}

pub fn part_text(bytes: &[u8]) -> String {
    String::from_utf8(part_bytes(bytes, PATH)).expect("UTF-8")
}

/// A chain of `n` shapes, each the only member of the one before it, the innermost holding `kept`.
fn chain(n: u32, kept: Option<RawElement>) -> Shape {
    let mut s = Shape::new(n);
    if let Some(k) = kept {
        s.passthrough.push(0, RawNode::Element(k));
    }
    for id in (1..n).rev() {
        let mut parent = Shape::new(id);
        parent.children = ShapeList::from_vec(vec![Arc::new(s)]);
        s = parent;
    }
    s
}

/// An element nested `levels` deep (the first level is the element itself).
fn nest(levels: usize) -> RawElement {
    let mut e = RawElement { name: Arc::from("leaf"), attrs: RawAttrs::new(), children: Vec::new() };
    for _ in 1..levels {
        e = RawElement { name: Arc::from("k"), attrs: RawAttrs::new(), children: vec![RawNode::Element(e)] };
    }
    e
}

/// The deepest tree the model allows is written and read: shapes nested to the model's cap with a kept element nested as deep as the codec writes one, which is more than
/// 90 levels of shapes (the most the old file held, at two elements a level) and more than 200 elements deep (the container's own default).
#[test]
fn shapes_nested_to_the_models_cap_and_a_deep_kept_element_are_written_and_read_back() {
    let most = u32::try_from(diacraft_doc::MAX_SHAPE_DEPTH).unwrap();
    let master = Master { id: 1, shapes: ShapeList::from_vec(vec![Arc::new(chain(most, Some(nest(150))))]), ..Master::default() };
    let (bytes, got) = through_the_file(master.clone());
    assert_eq!(got, master);
    let deepest = elements(&tree(&bytes)).into_iter().map(|(_, d)| d).max().unwrap_or(0);
    assert!(deepest > 300, "the file is {deepest} elements deep, past what the default depth of the container reads");
}

/// A document file whose masters pass the depth the reader parses it at is left unread with a word and is no error, as every file that does not parse.
#[test]
fn a_document_file_nested_past_the_parsers_ceiling_is_left_unread_and_is_no_error() {
    let deep = format!("{}{}", "<diacraft:x>".repeat(600), "</diacraft:x>".repeat(600));
    let (doc, report) = read(&package_with(&file_of(&deep)), &Limits::default()).unwrap();
    assert!(doc.masters.is_empty());
    assert_eq!(report.warning_counts.keys().copied().collect::<Vec<_>>(), ["diacraft_file_unreadable"]);
}

/// The document file is parsed at the container's ceiling and no deeper: a file nested exactly that deep (the root is the first level) is read, with an unknown child
/// that holds the nesting kept as it is, and one level more is left unread with a word.
#[test]
fn a_document_file_nested_to_the_parsers_ceiling_is_read_and_one_level_more_is_left_unread() {
    let ceiling = diacraft_container::limits::XML_DEPTH_CEILING;
    let nested = |levels: usize| format!("{}{}", "<diacraft:x>".repeat(levels), "</diacraft:x>".repeat(levels));
    let (doc, report) = read(&package_with(&file_of(&nested(ceiling - 1))), &Limits::default()).unwrap();
    assert!(doc.masters.is_empty());
    assert!(report.warning_counts.is_empty(), "the file as deep as the ceiling reads: {:?}", report.warnings);
    let (_, report) = read(&package_with(&file_of(&nested(ceiling))), &Limits::default()).unwrap();
    assert_eq!(report.warning_counts.keys().copied().collect::<Vec<_>>(), ["diacraft_file_unreadable"], "one level more is left unread");
}

/// A file of the old version (`1`, which the build before the codec wrote and no release shipped) is kept as it is and not read: the masters it holds are the old
/// element's, which this reader would read as other things. A person is told, in words that name both versions.
#[test]
fn a_document_file_of_the_old_version_is_kept_and_not_read() {
    let old = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><diacraft:document xmlns:diacraft="{NS}" version="1"><diacraft:words glue="9"/><diacraft:master id="7" name="Old"><diacraft:sheet/><diacraft:shapes><diacraft:shape id="1"><diacraft:sheet/></diacraft:shape></diacraft:shapes><diacraft:connects/></diacraft:master><diacraft:theme part="t">QUJD</diacraft:theme></diacraft:document>"#
    );
    let bytes = package_with(&old);
    let (doc, report) = read(&bytes, &Limits::default()).unwrap();
    assert!(doc.masters.is_empty() && doc.themes.is_empty(), "nothing of the old file is read");
    assert_eq!(doc.snap_words(), SnapWords::default());
    assert_eq!(report.warning_counts.keys().copied().collect::<Vec<_>>(), ["diacraft_file_old"], "{:?}", report.warnings);
    let words = report.warnings.first().cloned().unwrap_or_default();
    assert!(words.contains("version 1") && words.contains("version 2") && words.contains("kept"), "{words}");
    // Kept: a save that changes nothing is the file, byte for byte.
    let saved = real(&doc, &bytes, &report.index).unwrap();
    assert!(!saved.edited && saved.bytes == bytes, "an unedited save keeps the file");
    assert!(part_text(&saved.bytes).contains("Old"), "with the old master in it");
    // A change the file carries writes it again, as the version this build writes: what the old file held was never read, so none of it is in the new one.
    let mut o = open_bytes(bytes.clone());
    o.doc.set_snap_words(&SnapWords { snap: Some(Arc::from("65845")), ..SnapWords::default() });
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    assert!(saved.edited);
    let file = part_text(&saved.bytes);
    assert!(file.contains(r#"version="2""#) && !file.contains("Old") && file.contains(r#"snap="65845""#), "{file}");
    let (again, report) = read(&saved.bytes, &Limits::default()).unwrap();
    assert!(report.warning_counts.is_empty(), "{:?}", report.warning_counts);
    assert_eq!(again.snap_words().snap.as_deref(), Some("65845"));
}

/// A document file of a newer version is read for the names this build knows, as it was.
#[test]
fn a_document_file_of_a_newer_version_is_read_for_the_names_it_knows() {
    let file = format!(
        r##"<diacraft:document xmlns:diacraft="{NS}" version="9"><diacraft:colors><diacraft:color ix="3" rgb="#010203"/></diacraft:colors></diacraft:document>"##
    );
    let (doc, report) = read(&package_with(&file), &Limits::default()).unwrap();
    assert_eq!(doc.colors.len(), 1);
    assert_eq!(report.warning_counts.keys().copied().collect::<Vec<_>>(), ["diacraft_version"]);
}

/// A picture is charged to the budget of pictures once, as the bytes it is, whatever the shapes that show it: a picture of `n` bytes shown by two thousand shapes is `n`
/// bytes of the budget, where it was two thousand times.
#[test]
fn a_blob_is_charged_to_the_pictures_budget_once_whatever_the_shapes_that_show_it() {
    const BYTES: usize = 3_000;
    let png: Vec<u8> = (0..BYTES).map(|k| u8::try_from(k * 13 % 251).unwrap_or(0)).collect();
    let media = Arc::new(Media { part: Arc::from("media/p.png"), content_type: Arc::from("image/png"), bytes: Arc::from(png) });
    let shapes: Vec<Arc<Shape>> = (1..=2_000u32)
        .map(|id| {
            let mut s = Shape::new(id);
            s.kind = ShapeType::Foreign;
            s.foreign = Some(Arc::new(ForeignData { picture: Some(Arc::clone(&media)), ..ForeignData::default() }));
            Arc::new(s)
        })
        .collect();
    let mut o = base();
    o.doc.masters.push(Arc::new(Master { id: 1, shapes: ShapeList::from_vec(shapes), ..Master::default() }));
    let saved = real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("not saved: {e}"));
    // The budget holds the entries the model keeps beside the picture: the thumbnail the save wrote, for one.
    let beside: u64 = read(&saved.bytes, &Limits::default()).unwrap().0.parts.iter().map(|p| p.bytes.len() as u64).sum();
    let exact = Limits { max_media_bytes: u64::try_from(BYTES).unwrap() + beside, ..Limits::default() };
    assert_eq!(
        read(&saved.bytes, &exact).unwrap().0.masters.get(0).map(|m| m.shapes.len()),
        Some(2_000),
        "the bytes of the picture, once, are enough"
    );
    let short = Limits { max_media_bytes: u64::try_from(BYTES).unwrap() + beside - 3, ..Limits::default() };
    match read(&saved.bytes, &short) {
        Err(e) => assert_eq!(e.class(), "xml_too_complex", "{e}"),
        Ok(_) => panic!("a picture past the budget of pictures was read"),
    }
}

/// A master of the codec's vocabulary that the codec finds damaged (a shape of a kind it does not list) is left out with a word, and is no error: the drawing reads
/// from its markup, as it does after a file that does not parse.
#[test]
fn a_damaged_master_is_left_out_with_a_word_and_is_no_error() {
    let bad = r#"<diacraft:master id="1"><diacraft:shape id="1" type="Bogus"/></diacraft:master>"#;
    let good = r#"<diacraft:master id="2" name="Good"/>"#;
    let (doc, report) = read(&package_with(&file_of(&format!("{bad}{good}"))), &Limits::default()).unwrap();
    assert_eq!(doc.masters.iter().map(|m| m.id).collect::<Vec<_>>(), [2], "the damaged master is left out and the next is read");
    assert_eq!(report.warning_counts.keys().copied().collect::<Vec<_>>(), ["diacraft_file_unreadable"]);
    assert!(report.warnings.first().is_some_and(|w| w.contains("master")), "{:?}", report.warnings);
}

/// A master is charged to the reader's items by what the codec spent on it, so that the items grow with the shapes, the file is refused as too many items past the
/// budget, and a master past the budget is refused where it stands, before the rest of it is built.
#[test]
fn a_master_is_charged_the_items_the_codec_spent_and_is_refused_past_the_budget() {
    let make = |n: u32| {
        let shapes: Vec<Arc<Shape>> = (1..=n).map(|id| Arc::new(body_shape(id, false))).collect();
        Master { id: 1, shapes: ShapeList::from_vec(shapes), connects: Connects::default(), ..Master::default() }
    };
    let items = |n: u32| {
        let bytes = through_the_file(make(n)).0;
        read(&bytes, &Limits::default()).unwrap().1.work.items
    };
    let (a, b, c) = (items(100), items(200), items(400));
    assert!(b - a >= 100 && c - b >= 200, "each shape costs items: {a} {b} {c}");
    assert!(c - b <= 2 * (b - a) + 64 && 2 * (b - a) <= c - b + 64, "the cost is linear in the shapes: {a} {b} {c}");
    let bytes = through_the_file(make(400)).0;
    let tight = Limits { max_items: usize::try_from(b).unwrap(), ..Limits::default() };
    match read(&bytes, &tight) {
        Err(e) => assert_eq!(e.class(), "too_many_items", "{e}"),
        Ok(_) => panic!("400 shapes were read in the budget of 200"),
    }
    let shapes = Limits { max_shapes: 399, ..Limits::default() };
    match read(&bytes, &shapes) {
        Err(e) => assert_eq!(e.class(), "too_many_shapes", "{e}"),
        Ok(_) => panic!("400 shapes of masters were read where the document may have 399"),
    }
}

/// The cells of a master's sheets are counted against the document's cells, as the cells of every other sheet are.
#[test]
fn the_cells_of_a_masters_sheets_are_counted_against_the_documents_cells() {
    let (bytes, _) = through_the_file(a_master(7));
    let (_, report) = read(&bytes, &Limits::default()).unwrap();
    let cells = report.stats.cells;
    assert!(cells > 0);
    let tight = Limits { max_cells: cells - 1, ..Limits::default() };
    match read(&bytes, &tight) {
        Err(e) => assert_eq!(e.class(), "too_many_cells", "{e}"),
        Ok(_) => panic!("a cell past the document's cells was read"),
    }
}
