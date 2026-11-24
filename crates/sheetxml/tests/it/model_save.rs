//! Findings 5 and 6: **no file is written that an open refuses.** The save's pre-flight ([`encode_for_save`]) makes the check an open will
//! make, with the limits and the budget it will have, before anything is written: two pages of 50,000 shapes were written by the encoder and refused by the
//! budget of the open, and a model of a namespace of 300 KB on a thousand kept elements was 0.9 MB of input and a part of 295 MiB.
use diacraft_container::Limits;
use diacraft_container::xml::{El, Node, parse_with, to_bytes};
use diacraft_doc::{Document, Passthrough, Placed, RawAttrs, RawElement, RawNode, Shape, ShapeList};
use diacraft_sheetxml::model::{
    Budget, CodecError, DecodeNote, MemBlobs, OPEN_ITEMS, URN, check_decodes, decode, decode_page, encode, encode_for_save, open_limits, preflight,
    written_len,
};
use diacraft_testkit::model::{ModelSize, any_document};
use proptest::prelude::*;
use std::sync::Arc;

use super::model_util::native_limits;

const HUGE: u64 = 1 << 40;

// ------------------------------------------------------------------------------------------------------------------------------------- written length

fn el(name: &str, attrs: &[(&str, &str)], children: Vec<Node>) -> El {
    El { name: name.to_owned(), attrs: attrs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect(), children }
}

#[test]
fn the_written_length_of_a_tree_is_what_the_writer_makes_of_it_for_every_kind_of_node_and_escape() {
    let tree = el(
        "d:root",
        &[("xmlns:d", "urn:d"), ("a", "plain"), ("b", "a&b<c\"d\ne\rf\tg"), ("c", ""), ("é", "ünïcode €😀")],
        vec![
            Node::Text("some text & <more> with \r and unicode é€😀".to_owned()),
            Node::El(el("d:empty", &[], vec![])),
            Node::El(el("d:one", &[("k", "v")], vec![Node::Text("t".to_owned())])),
            Node::CData("cdata with ]]> inside and ]]>]]> twice".to_owned()),
            Node::CData("cdata with a \r carriage return".to_owned()),
            Node::CData(String::new()),
            Node::Comment(" a comment ".to_owned()),
            Node::Pi { target: "pi".to_owned(), data: "some data".to_owned() },
            Node::Pi { target: "empty".to_owned(), data: String::new() },
            Node::El(el("d:deep", &[], vec![Node::El(el("d:deeper", &[("x", "y")], vec![Node::El(el("d:deepest", &[], vec![]))]))])),
            Node::Text(String::new()),
        ],
    );
    let bytes = to_bytes(&tree, &[], &[], true).expect("a tree the writer takes");
    assert_eq!(written_len(&tree), bytes.len() as u64);
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 24, ..ProptestConfig::default() })]

    #[test]
    fn the_written_length_of_every_part_of_a_generated_model_is_what_the_writer_makes_of_it(doc in any_document(ModelSize::Small)) {
        let encoded = encode(&doc, &mut MemBlobs::new()).unwrap();
        let parts = std::iter::once(&encoded.document).chain(encoded.pages.iter().map(|(_, e)| e)).chain(encoded.masters.iter().map(|(_, e)| e));
        for part in parts {
            let bytes = to_bytes(part, &[], &[], true).unwrap();
            prop_assert_eq!(written_len(part), bytes.len() as u64);
        }
    }
}

// ------------------------------------------------------------------------------------------------------------------------------------- a save is what an open takes

/// A shape of the kind a drawing has, as the codec writes it: eight cells, two with formulas, a text and a path of five rows of two cells.
pub(super) fn typical_shape_xml(id: u32) -> String {
    let d = "diacraft";
    let cells = format!(
        "<{d}:Cell N=\"PinX\" V=\"{}\"/><{d}:Cell N=\"PinY\" V=\"{}\"/><{d}:Cell N=\"Width\" V=\"1\"/><{d}:Cell N=\"Height\" V=\"0.75\"/><{d}:Cell N=\"LocPinX\" V=\"0.5\" F=\"Width*0.5\"/><{d}:Cell N=\"LocPinY\" V=\"0.375\" F=\"Height*0.5\"/><{d}:Cell N=\"Angle\" V=\"0\"/>",
        id % 100,
        id / 100
    );
    let points = [("MoveTo", "0", "0"), ("LineTo", "Width", "0"), ("LineTo", "Width", "Height"), ("LineTo", "0", "Height"), ("LineTo", "0", "0")];
    let rows: String = points
        .iter()
        .enumerate()
        .map(|(ix, (t, x, y))| {
            format!("<{d}:Row IX=\"{}\" T=\"{t}\"><{d}:Cell N=\"X\" V=\"1\" F=\"{x}\"/><{d}:Cell N=\"Y\" V=\"1\" F=\"{y}\"/></{d}:Row>", ix + 1)
        })
        .collect();
    format!(
        "<{d}:shape id=\"{id}\"><{d}:sheet>{cells}<{d}:Section N=\"Geometry\" IX=\"0\">{rows}</{d}:Section></{d}:sheet><{d}:text><{d}:t>Label</{d}:t></{d}:text></{d}:shape>"
    )
}

/// A drawing of `pages` pages of `shapes` shapes of that kind: each shape's sheet is its own when `alone` and is shared with the shapes whose sheets are equal when not
/// (what a decode that shares makes, and what a file that says a sheet once is written as).
fn drawing_with(pages: u32, shapes: u32, alone: bool) -> Document {
    let mut doc = Document::empty();
    for id in 1..=pages {
        let body: String = (1..=shapes).map(typical_shape_xml).collect();
        let xml = format!("<diacraft:page xmlns:diacraft=\"{URN}\" id=\"{id}\">{body}</diacraft:page>");
        let tree = parse_with(xml.as_bytes(), &native_limits()).unwrap();
        let mut budget = Budget::new(HUGE, HUGE, HUGE);
        if alone {
            budget = budget.without_sharing();
        }
        doc.pages.push(Arc::new(decode_page(&tree, &MemBlobs::new(), &mut budget, &mut Vec::new()).unwrap()));
    }
    doc
}

/// A drawing of the kind a decode that shares makes.
fn drawing(pages: u32, shapes: u32) -> Document {
    drawing_with(pages, shapes, false)
}

/// A drawing in which no two units are equal (each shape's path starts at a place of its own), so that the file says every unit in full whatever the model holds: the
/// encoder shares equal units by value, and a drawing of equal paths is written once.
fn drawing_unique(pages: u32, shapes: u32) -> Document {
    let mut doc = Document::empty();
    for id in 1..=pages {
        let body: String = (1..=shapes).map(|i| typical_shape_xml(i).replacen("V=\"1\" F=\"0\"", &format!("V=\"{i}\" F=\"0\""), 1)).collect();
        let xml = format!("<diacraft:page xmlns:diacraft=\"{URN}\" id=\"{id}\">{body}</diacraft:page>");
        let tree = parse_with(xml.as_bytes(), &native_limits()).unwrap();
        let mut budget = Budget::new(HUGE, HUGE, HUGE).without_sharing();
        doc.pages.push(Arc::new(decode_page(&tree, &MemBlobs::new(), &mut budget, &mut Vec::new()).unwrap()));
    }
    doc
}

/// A drawing in which each shape has a sheet of its own: what a reader that shares nothing makes.
fn drawing_alone(pages: u32, shapes: u32) -> Document {
    drawing_with(pages, shapes, true)
}

/// The limits of an open scaled down: a package of `bytes`, which sets the pools in proportion, and no tree of more items than its pool.
fn small_limits(package: u64) -> Limits {
    let mut limits = Limits { max_package_bytes: package, ..Limits::default() };
    // The tree of a part is held to what a file of the package's size can hold of items (an item is at least three bytes written), whatever a `same` stands for.
    limits.xml.max_nodes = usize::try_from(package / 8).unwrap();
    limits
}

/// Whether an open of the parts of `doc` at `limits` takes them: the reader takes each part (its bytes as written within the part and the package, its items within
/// one tree), and the decode is within the budget the limits give.
fn an_open_takes(doc: &Document, limits: &Limits) -> bool {
    let mut blobs = MemBlobs::new();
    let encoded = encode(doc, &mut blobs).unwrap();
    let parts: Vec<&El> =
        std::iter::once(&encoded.document).chain(encoded.pages.iter().map(|(_, e)| e)).chain(encoded.masters.iter().map(|(_, e)| e)).collect();
    let mut package = 0u64;
    for part in &parts {
        package += written_len(part);
        let bytes = to_bytes(part, &[], &[], true).unwrap();
        if bytes.len() as u64 > limits.max_part_bytes || parse_with(&bytes, &limits.xml).is_err() {
            return false;
        }
    }
    if package > limits.max_package_bytes {
        return false;
    }
    let pages: Vec<&El> = encoded.pages.iter().map(|(_, e)| e).collect();
    let masters: Vec<&El> = encoded.masters.iter().map(|(_, e)| e).collect();
    decode(&encoded.document, &pages, &masters, &blobs, &mut Budget::for_open(limits), &mut Vec::new()).is_ok()
}

#[test]
fn a_drawing_is_saved_exactly_when_an_open_of_the_same_limits_takes_it_and_otherwise_refused_in_words() {
    // A package of 1 MiB: 131,072 items, 128 MiB of model. A shape of this kind is about 80 items.
    let limits = small_limits(1 << 20);
    let mut saved = 0;
    let mut refused = 0;
    for (pages, shapes) in [(1u32, 100u32), (1, 800), (1, 1_400), (1, 1_700), (1, 2_500), (2, 600), (2, 800), (2, 1_000), (3, 800), (4, 1_000)] {
        let doc = drawing_alone(pages, shapes);
        let takes = an_open_takes(&doc, &limits);
        let out = encode_for_save(&doc, &mut MemBlobs::new(), &limits);
        match (&out, takes) {
            (Ok(_), true) => saved += 1,
            (Err(CodecError::Limit { .. }), false) => refused += 1,
            (out, takes) => panic!("{pages} pages of {shapes} shapes: the open takes it: {takes}, the save: {:?}", out.as_ref().err()),
        }
    }
    assert!(saved >= 2 && refused >= 2, "saved {saved}, refused {refused}: the sizes straddle the limit");
}

#[test]
fn a_save_that_passes_is_a_package_an_open_reads() {
    let limits = small_limits(1 << 20);
    let doc = drawing(2, 500);
    let mut blobs = MemBlobs::new();
    let encoded = encode_for_save(&doc, &mut blobs, &limits).expect("a drawing within the limits");
    // Written and read as a package would hold it: each part's bytes parsed again at the open's limits.
    let parsed: Vec<El> = std::iter::once(&encoded.document)
        .chain(encoded.pages.iter().map(|(_, e)| e))
        .map(|e| parse_with(&to_bytes(e, &[], &[], true).unwrap(), &limits.xml).expect("a part the reader takes at the open's limits"))
        .collect();
    let refs: Vec<&El> = parsed.iter().skip(1).collect();
    let back = decode(&parsed[0], &refs, &[], &blobs, &mut Budget::for_open(&limits), &mut Vec::new()).expect("an open of what was saved");
    assert_eq!(back, doc);
}

/// Every part of a drawing as a save a part at a time makes it, written as a package would hold it, and what an open that asks for a part at a time makes of them.
fn saved_and_opened(doc: &Document, limits: &Limits) -> Result<(Document, u64), CodecError> {
    use diacraft_sheetxml::model::{PartRole, PartSource, decode_from, save_parts};
    struct Bytes<'a> {
        pages: &'a [Vec<u8>],
        limits: &'a Limits,
    }
    impl PartSource for Bytes<'_> {
        type Tree = El;
        fn page(&mut self, index: usize, _name: &str) -> Result<El, CodecError> {
            let bytes = self.pages.get(index).ok_or(CodecError::Malformed { part: "page".to_owned(), why: "no such part" })?;
            parse_with(bytes, &self.limits.xml).map_err(|_| CodecError::Malformed { part: "page".to_owned(), why: "a part the reader refuses" })
        }
        fn master(&mut self, _index: usize, _name: &str) -> Result<El, CodecError> {
            Err(CodecError::Malformed { part: "master".to_owned(), why: "no such part" })
        }
    }
    let mut blobs = MemBlobs::new();
    let (mut document, mut pages) = (Vec::new(), Vec::new());
    save_parts(doc, &mut blobs, limits, &mut |p| {
        let bytes = to_bytes(p.tree, &[], &[], true)?;
        match p.role {
            PartRole::Document => document = bytes,
            PartRole::Page { .. } => pages.push(bytes),
            PartRole::Master { .. } => {}
        }
        Ok(())
    })?;
    let mut budget = Budget::for_open(limits);
    let back = decode_from(&parse_with(&document, &limits.xml).unwrap(), &mut Bytes { pages: &pages, limits }, &blobs, &mut budget, &mut Vec::new())?;
    Ok((back, OPEN_ITEMS - budget.items))
}

#[test]
fn two_pages_of_50000_shapes_are_saved_a_part_at_a_time_and_the_open_takes_them() {
    // The review's drawing: two pages of 50,000 shapes, 8.6 million items if each shape's sheet is written out. A drawing that shares says each
    // page's sheet once, and a page of it is the shapes and what is each its own: saved, as a part at a time and as every part held at once, and read back as it was.
    let doc = drawing(2, 50_000);
    let (back, items) = saved_and_opened(&doc, &open_limits()).expect("a drawing the open budget takes");
    assert_eq!(back, doc);
    assert!(items > 8_000_000, "what each page says once is charged for every place that stands for it: {items}");
    // Held at once, as trees, the two pages are more than a tree of an open may have between them (2,000,000 items); a part at a time they are not.
    let held = encode_for_save(&doc, &mut MemBlobs::new(), &open_limits());
    assert!(matches!(&held, Err(CodecError::Limit { what: "XML items in the parts held at once", max: 2_000_000 })), "{held:?}");
}

#[test]
fn a_drawing_of_more_shapes_than_a_pool_can_hold_unshared_is_refused_at_save_in_words_before_a_part_is_made() {
    // Forty thousand shapes that each have a sheet of their own, saved at the limits of a package of 1 MiB (a pool of 128 MiB, and a figure of 1.6 times it): 424 MB of model as it
    // is held (the rows the decoder makes equal are one allocation, and held once), which is more than the pool of such an open holds. The count of what the model is as it is held
    // is made before any part is, so no part is handed over.
    let limits = small_limits(1 << 20);
    let most = Budget::for_open(&limits).bytes.saturating_mul(8) / 5;
    let mut doc = drawing_alone(1, 40_000);
    let page = doc.pages.get(0).unwrap().clone();
    for id in 2..=6u32 {
        let mut copy = (*page).clone();
        copy.id = id;
        doc.pages.push(Arc::new(copy));
    }
    // The copies share the first page's shapes (the one allocation), which is what held once means: the model is the 40,000.
    let held = diacraft_sheetxml::model::held_bytes(&doc);
    assert!(held > most, "40,000 shapes: {held} against {most}");
    let mut handed = 0usize;
    let out = diacraft_sheetxml::model::save_parts(&doc, &mut MemBlobs::new(), &limits, &mut |_| {
        handed += 1;
        Ok(())
    });
    match out {
        Ok(()) => panic!("40,000 shapes that each have a sheet of their own are more model than the open budget takes"),
        Err(e) => {
            assert!(matches!(&e, CodecError::Limit { what: "bytes of the model in memory", max } if *max == most), "{e:?}");
            assert_eq!(e.class(), "model_too_complex");
        }
    }
    assert_eq!(handed, 0);
}

// ------------------------------------------------------------------------------------------------------------------------------------- the part, the package, the items

/// A shape that keeps `n` unknown elements in a namespace of `uri_bytes` bytes, which each copy of the declaration is written with.
fn shape_with_namespace_on_many_elements(n: usize, uri_bytes: usize) -> Document {
    let uri = "u".repeat(uri_bytes);
    let kept: Vec<Placed> = (0..n)
        .map(|i| Placed {
            after: 0,
            node: RawNode::Element(RawElement {
                name: Arc::from(format!("k:x{i}").as_str()),
                attrs: RawAttrs::from_pairs([("xmlns:k", uri.as_str())]),
                children: Vec::new(),
            }),
        })
        .collect();
    let mut shape = Shape::new(1);
    shape.ext.make_mut().native.kept = Passthrough::from_nodes(kept);
    let mut doc = Document::empty();
    let mut page = diacraft_doc::Page::new(1, "P");
    page.shapes = ShapeList::from_vec(vec![Arc::new(shape)]);
    doc.pages.push(Arc::new(page));
    doc
}

#[test]
fn a_part_past_the_most_bytes_a_part_may_have_as_written_is_refused_at_save() {
    // A namespace of 40 KB on 200 kept elements: 8 MB written, from a model that is 8 MB of kept strings, within every pool.
    let doc = shape_with_namespace_on_many_elements(200, 40_000);
    let mut limits = open_limits();
    limits.max_part_bytes = 4 << 20;
    let out = encode_for_save(&doc, &mut MemBlobs::new(), &limits);
    assert!(matches!(&out, Err(CodecError::Limit { what: "bytes in one part as written", max }) if *max == 4 << 20), "{:?}", out.err());
    // At exactly what it is, a part is written: the limit is the part's size as written and not a little under.
    let encoded = encode(&doc, &mut MemBlobs::new()).unwrap();
    let size = written_len(&encoded.pages[0].1);
    assert!(size > 8_000_000 && size < 8_100_000, "{size}");
    limits.max_part_bytes = size;
    assert!(encode_for_save(&doc, &mut MemBlobs::new(), &limits).is_ok(), "a part of exactly the most bytes");
    limits.max_part_bytes = size - 1;
    assert!(encode_for_save(&doc, &mut MemBlobs::new(), &limits).is_err(), "and one byte less is refused");
}

#[test]
fn the_part_of_the_review_is_refused_at_save_at_the_container_limit_and_not_written() {
    // 0.9 MB of input, 295 MiB of part: a namespace of 300 KB copied onto a thousand kept elements, which an open at the container's own limit (256 MiB to a
    // part) refuses as a part too large. The model is 300 MB of kept strings, inside the pool of an open at those limits.
    let doc = shape_with_namespace_on_many_elements(1_000, 300_000);
    let mut limits = Limits::default();
    limits.xml.max_nodes = 4_000_000;
    let out = encode_for_save(&doc, &mut MemBlobs::new(), &limits);
    assert!(matches!(&out, Err(CodecError::Limit { what: "bytes in one part as written", .. })), "{:?}", out.err());
}

#[test]
fn a_package_past_the_most_bytes_the_most_parts_or_the_most_items_in_a_part_is_refused_at_save() {
    // Each shape its own sheet and its own path: what the file says is what the items are, and a package of the size of its parts takes it.
    let doc = drawing_unique(1, 200);
    let ok = encode_for_save(&doc, &mut MemBlobs::new(), &open_limits()).expect("a small drawing");
    let part = written_len(&ok.pages[0].1);
    let document = written_len(&ok.document);
    // The bytes of the package: the parts, and the blobs when it has any.
    let mut limits = open_limits();
    limits.max_package_bytes = part + document;
    assert!(preflight(&ok, &MemBlobs::new(), &limits).is_ok(), "the parts of exactly the package's bytes");
    limits.max_package_bytes = part + document - 1;
    let out = preflight(&ok, &MemBlobs::new(), &limits);
    assert!(matches!(&out, Err(CodecError::Limit { what: "bytes in the package as written", .. })), "{:?}", out.err());
    // The parts: the document and the page are two.
    let mut limits = open_limits();
    limits.max_parts = 2;
    assert!(preflight(&ok, &MemBlobs::new(), &limits).is_ok(), "two parts in a package of two");
    limits.max_parts = 1;
    let out = preflight(&ok, &MemBlobs::new(), &limits);
    assert!(matches!(&out, Err(CodecError::Limit { what: "parts in the package", max: 1 })), "{:?}", out.err());
    // The items of a tree: what the reader takes in one.
    let items = {
        let mut n = 0u64;
        let mut stack = vec![&ok.pages[0].1];
        while let Some(e) = stack.pop() {
            n += 1 + e.attrs.len() as u64;
            for c in &e.children {
                match c {
                    Node::El(c) => stack.push(c),
                    _ => n += 1,
                }
            }
        }
        n
    };
    let mut limits = open_limits();
    limits.xml.max_nodes = items as usize;
    assert!(preflight(&ok, &MemBlobs::new(), &limits).is_ok(), "a tree of exactly the most items");
    limits.xml.max_nodes = items as usize - 1;
    let out = preflight(&ok, &MemBlobs::new(), &limits);
    assert!(matches!(&out, Err(CodecError::Limit { what: "items in one part", .. })), "{:?}", out.err());
}

#[test]
fn the_blobs_are_part_of_the_package_the_save_counts() {
    let mut blobs = MemBlobs::new();
    let mut shape = Shape::new(1);
    shape.foreign = Some(Arc::new(diacraft_doc::ForeignData {
        picture: Some(Arc::new(diacraft_doc::Media {
            part: Arc::from("p"),
            content_type: Arc::from("image/png"),
            bytes: Arc::from(vec![7u8; 100_000]),
        })),
        ..diacraft_doc::ForeignData::default()
    }));
    let mut page = diacraft_doc::Page::new(1, "P");
    page.shapes = ShapeList::from_vec(vec![Arc::new(shape)]);
    let mut doc = Document::empty();
    doc.pages.push(Arc::new(page));
    let mut limits = open_limits();
    limits.max_package_bytes = 50_000;
    let out = encode_for_save(&doc, &mut blobs, &limits);
    assert!(matches!(&out, Err(CodecError::Limit { what: "bytes in the package as written", .. })), "{:?}", out.err());
    limits.max_package_bytes = 200_000;
    assert!(encode_for_save(&doc, &mut MemBlobs::new(), &limits).is_ok());
}

// ------------------------------------------------------------------------------------------------------------------------------------- the dry decode

#[test]
fn a_decode_that_keeps_nothing_is_charged_what_a_decode_is_and_refuses_what_it_refuses() {
    let doc = drawing(2, 300);
    let mut blobs = MemBlobs::new();
    let e = encode(&doc, &mut blobs).unwrap();
    let pages: Vec<&El> = e.pages.iter().map(|(_, p)| p).collect();
    let (mut kept, mut checked) = (Budget::new(HUGE, HUGE, HUGE), Budget::new(HUGE, HUGE, HUGE));
    let mut notes: Vec<DecodeNote> = Vec::new();
    let back = decode(&e.document, &pages, &[], &blobs, &mut kept, &mut notes).unwrap();
    assert_eq!(back, doc);
    check_decodes(&e.document, &pages, &[], &blobs, &mut checked, &mut Vec::new()).unwrap();
    assert_eq!(kept, checked, "the same items, bytes and blobs");
    // A budget one byte short refuses both, with the same words.
    let needed = HUGE - kept.bytes;
    for short in [1u64, 1_000, needed / 2] {
        let (a, b) = (
            decode(&e.document, &pages, &[], &blobs, &mut Budget::new(HUGE, needed - short, HUGE), &mut Vec::new()).map(drop),
            check_decodes(&e.document, &pages, &[], &blobs, &mut Budget::new(HUGE, needed - short, HUGE), &mut Vec::new()),
        );
        assert!(matches!(&a, Err(CodecError::Limit { what: "bytes of the model in memory", .. })), "{short}: {a:?}");
        assert_eq!(a, b, "{short} short");
    }
}

#[test]
fn a_decode_that_keeps_nothing_still_refuses_a_part_that_is_not_the_one_the_list_names() {
    let doc = drawing(1, 5);
    let e = encode(&doc, &mut MemBlobs::new()).unwrap();
    let wrong = parse_with(b"<d:page xmlns:d=\"urn:other\"/>", &native_limits()).unwrap();
    let out = check_decodes(&e.document, &[&wrong], &[], &MemBlobs::new(), &mut Budget::new(HUGE, HUGE, HUGE), &mut Vec::new());
    assert!(out.is_err(), "{out:?}");
    let out = check_decodes(&e.document, &[], &[], &MemBlobs::new(), &mut Budget::new(HUGE, HUGE, HUGE), &mut Vec::new());
    assert!(matches!(&out, Err(CodecError::Malformed { .. })), "the lists name a page that was not given: {out:?}");
}

/// `pages` pages of `shapes` plain shapes each: few items to the model's bytes, so that the pool of the model's bytes is what an open of them spends first.
fn plain_drawing(pages: u32, shapes: u32) -> Document {
    let mut doc = Document::empty();
    for id in 1..=pages {
        let mut page = diacraft_doc::Page::new(id, "P");
        page.shapes = ShapeList::from_vec((1..=shapes).map(|s| Arc::new(Shape::new(s))).collect());
        doc.pages.push(Arc::new(page));
    }
    doc
}

#[test]
fn a_drawing_that_spends_the_pool_of_the_models_bytes_before_the_items_is_refused_at_save_as_an_open_refuses_it() {
    // A shape is two items and 480 bytes of model: 800,000 of them are 1.6 million items and 384 MB, and the pool is 1,344 MiB (a drawing with no pictures has the blobs' room too).
    // A page is a stream, so that its items are not what is spent first.
    let limits = open_limits();
    let two = plain_drawing(2, 800_000);
    let saved = diacraft_sheetxml::model::save_parts_bytes(&two, &mut MemBlobs::new(), &limits, &mut |_| Ok(()));
    assert!(saved.is_ok(), "two pages: {:?}", saved.err());
    let three = plain_drawing(4, 800_000);
    let out = diacraft_sheetxml::model::save_parts_bytes(&three, &mut MemBlobs::new(), &limits, &mut |_| Ok(()));
    assert!(matches!(&out, Err(CodecError::Limit { what: "bytes of the model in memory", .. })), "four pages: {:?}", out.err());
}

#[test]
#[ignore = "a measurement, by hand: `cargo test -p diacraft-sheetxml --test it model_save::capacity -- --ignored --nocapture`"]
fn capacity_in_typical_shapes() {
    // What one shape of the kind a drawing has costs the open: its items, and the model the budget is charged for it.
    let one = drawing(1, 1_000);
    let mut blobs = MemBlobs::new();
    let e = encode(&one, &mut blobs).unwrap();
    let pages: Vec<&El> = e.pages.iter().map(|(_, p)| p).collect();
    let mut budget = Budget::for_open(&open_limits());
    decode(&e.document, &pages, &[], &blobs, &mut budget, &mut Vec::new()).unwrap();
    let (items, bytes) = ((OPEN_ITEMS - budget.items) / 1_000, (diacraft_sheetxml::model::OPEN_MODEL_BYTES - budget.bytes) / 1_000);
    println!("CAPACITY a typical shape costs {items} items and {bytes} bytes of model; the pools are {OPEN_ITEMS} items and 960 MiB");
    // The most that one page of them can be saved: the bisection of the pre-flight.
    let (mut low, mut high) = (1_000u32, 60_000u32);
    while high - low > 250 {
        let mid = (low + high) / 2;
        let ok = encode_for_save(&drawing(1, mid), &mut MemBlobs::new(), &open_limits()).is_ok();
        if ok {
            low = mid;
        } else {
            high = mid;
        }
    }
    let refusal = encode_for_save(&drawing(1, high), &mut MemBlobs::new(), &open_limits()).err();
    println!("CAPACITY one page of typical shapes: saved at {low}, refused at {high}: {refusal:?}");
}

// ------------------------------------------------------------------------------------------------------------------------------------- the items, part by part

/// The items of a tree as the XML reader counts them.
fn tree_items(root: &El) -> u64 {
    let mut n = 0u64;
    let mut stack = vec![root];
    while let Some(e) = stack.pop() {
        n += 1 + e.attrs.len() as u64;
        for c in &e.children {
            match c {
                Node::El(c) => stack.push(c),
                _ => n += 1,
            }
        }
    }
    n
}

/// A kept element nested deeper than the model keeps one: the encoder refuses it where it reaches it.
fn poison() -> Passthrough {
    let level = |children: Vec<RawNode>| RawNode::Element(RawElement { name: Arc::from("q:e"), attrs: RawAttrs::new(), children });
    let mut node = level(Vec::new());
    for _ in 1..201 {
        node = level(vec![node]);
    }
    Passthrough::from_nodes(vec![Placed { after: 0, node }])
}

fn page_of_shapes(id: u32, shapes: u32) -> Arc<diacraft_doc::Page> {
    let mut page = diacraft_doc::Page::new(id, "P");
    page.shapes = ShapeList::from_vec((1..=shapes).map(|s| Arc::new(Shape::new(s))).collect());
    Arc::new(page)
}

fn poisoned_page(id: u32) -> Arc<diacraft_doc::Page> {
    let mut page = diacraft_doc::Page::new(id, "poison");
    page.ext.make_mut().native.kept = poison();
    Arc::new(page)
}

/// The limits of an open whose pool of items is exactly `items`.
fn limits_of_items(items: u64) -> Limits {
    let mut limits = Limits { max_package_bytes: items * 8, ..Limits::default() };
    limits.xml.max_nodes = usize::MAX;
    assert_eq!(Budget::for_open(&limits).items, items);
    limits
}

#[test]
fn a_save_refuses_at_the_first_part_that_passes_the_items_and_makes_none_after_it() {
    let (one, two) = (page_of_shapes(1, 20), page_of_shapes(2, 20));
    let mut base = Document::empty();
    base.pages.push(Arc::clone(&one));
    base.pages.push(Arc::clone(&two));
    // The document part names three pages: the figures are of a drawing that has a third, as the poisoned one is.
    base.pages.push(page_of_shapes(3, 0));
    let e = encode(&base, &mut MemBlobs::new()).unwrap();
    let (document, first, second) = (tree_items(&e.document), tree_items(&e.pages[0].1), tree_items(&e.pages[1].1));
    // A third page that the encoder refuses where it reaches it: the save is refused for the items, or it reached the page and was refused for the page.
    let mut doc = Document::empty();
    doc.pages.push(Arc::clone(&one));
    doc.pages.push(Arc::clone(&two));
    doc.pages.push(poisoned_page(3));
    let at = |items: u64| encode_for_save(&doc, &mut MemBlobs::new(), &limits_of_items(items)).err();
    // One item short of the three parts that are made: refused after the second page, which passes the figure, and the third is not made.
    let out = at(document + first + second - 1);
    assert!(matches!(&out, Some(CodecError::Limit { what: "items in the drawing", max }) if *max == document + first + second - 1), "{out:?}");
    // The document and the first page are within it, and the second is what passes it: the first alone is under, and the sum is what counts.
    assert!(document + first < document + first + second - 1);
    let out = at(document + first);
    assert!(matches!(&out, Some(CodecError::Limit { what: "items in the drawing", .. })), "{out:?}");
    // Exactly the items of the three: the third page is made, and it is the one that is refused.
    let out = at(document + first + second);
    assert!(matches!(&out, Some(CodecError::Limit { what: "levels of a kept element", .. })), "the poisoned page was reached: {out:?}");
}

#[test]
fn a_save_refuses_at_the_document_part_when_that_alone_passes_the_items() {
    let mut base = Document::empty();
    base.pages.push(page_of_shapes(1, 1));
    let document = tree_items(&encode(&base, &mut MemBlobs::new()).unwrap().document);
    // The first page is the one the encoder refuses for itself: it is made only when the document part is within the figure.
    let mut doc = Document::empty();
    doc.pages.push(poisoned_page(1));
    let out = encode_for_save(&doc, &mut MemBlobs::new(), &limits_of_items(document - 1)).err();
    assert!(matches!(&out, Some(CodecError::Limit { what: "items in the drawing", max }) if *max == document - 1), "{out:?}");
    // With room for the document part exactly, the page is made, and it is refused for itself.
    let out = encode_for_save(&doc, &mut MemBlobs::new(), &limits_of_items(document)).err();
    assert!(matches!(&out, Some(CodecError::Limit { what: "levels of a kept element", .. })), "{out:?}");
}

#[test]
fn a_save_refuses_at_the_first_master_that_passes_the_items_and_makes_none_after_it() {
    let master = |id: u32, shapes: u32| {
        let mut m = diacraft_doc::Master { id, ..diacraft_doc::Master::default() };
        m.shapes = ShapeList::from_vec((1..=shapes).map(|s| Arc::new(Shape::new(s))).collect());
        Arc::new(m)
    };
    let mut base = Document::empty();
    base.pages.push(page_of_shapes(1, 10));
    base.masters.push(master(1, 20));
    base.masters.push(master(2, 0));
    let e = encode(&base, &mut MemBlobs::new()).unwrap();
    let (document, page, first) = (tree_items(&e.document), tree_items(&e.pages[0].1), tree_items(&e.masters[0].1));
    let mut doc = Document::empty();
    doc.pages.push(page_of_shapes(1, 10));
    doc.masters.push(master(1, 20));
    let mut bad = diacraft_doc::Master { id: 2, ..diacraft_doc::Master::default() };
    bad.ext.make_mut().native.kept = poison();
    doc.masters.push(Arc::new(bad));
    let at = |items: u64| encode_for_save(&doc, &mut MemBlobs::new(), &limits_of_items(items)).err();
    let out = at(document + page + first - 1);
    assert!(matches!(&out, Some(CodecError::Limit { what: "items in the drawing", .. })), "{out:?}");
    let out = at(document + page + first);
    assert!(matches!(&out, Some(CodecError::Limit { what: "levels of a kept element", .. })), "the poisoned master was reached: {out:?}");
}

#[test]
fn an_element_of_the_model_that_declares_more_prefixes_than_an_open_takes_is_refused_at_save() {
    // A kept element written as itself with a thousand and a few declarations: the encoder writes it, and an open would refuse the element it is read as.
    let attrs = |k: usize| RawAttrs::from_pairs((0..k).map(|i| (format!("xmlns:p{i:04}"), "urn:u".to_owned())));
    let doc_with = |k: usize| {
        let mut shape = Shape::new(1);
        shape.ext.make_mut().native.kept = Passthrough::from_nodes(vec![Placed {
            after: 0,
            node: RawNode::Element(RawElement { name: Arc::from("p0000:x"), attrs: attrs(k), children: Vec::new() }),
        }]);
        let mut page = diacraft_doc::Page::new(1, "P");
        page.shapes = ShapeList::from_vec(vec![Arc::new(shape)]);
        let mut doc = Document::empty();
        doc.pages.push(Arc::new(page));
        doc
    };
    let most = diacraft_sheetxml::ns::MAX_PREFIXES_PER_ELEMENT;
    assert!(encode_for_save(&doc_with(most), &mut MemBlobs::new(), &open_limits()).is_ok(), "the most an element may declare");
    let out = encode_for_save(&doc_with(most + 1), &mut MemBlobs::new(), &open_limits());
    assert!(matches!(&out, Err(CodecError::Limit { what: "namespace prefixes declared by one element", .. })), "{:?}", out.err());
}

#[test]
fn a_model_past_the_caps_of_the_model_is_refused_before_anything_of_it_is_encoded() {
    // A place of 4,097 cells is past the model's cap of 4,096, as a model built in memory may be. The check is the first thing the save does: a drawing whose
    // second page the encoder would refuse for itself is refused for the first's cells, so the check was made before the parts were.
    let mut sheet = diacraft_doc::Sheet::default();
    for i in 0..4_097 {
        sheet.cells.insert(diacraft_doc::CellName::new(&format!("c{i}")), diacraft_doc::Cell::default());
    }
    let mut shape = Shape::new(1);
    shape.sheet = Arc::new(sheet);
    let mut first = diacraft_doc::Page::new(1, "P");
    first.shapes = ShapeList::from_vec(vec![Arc::new(shape)]);
    let mut doc = Document::empty();
    doc.pages.push(Arc::new(first));
    doc.pages.push(poisoned_page(2));
    let out = encode_for_save(&doc, &mut MemBlobs::new(), &open_limits());
    assert!(matches!(&out, Err(CodecError::Limit { what, .. }) if *what != "levels of a kept element"), "{:?}", out.err());
}
