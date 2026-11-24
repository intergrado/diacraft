//! Hostile input to the codec's decoder (the plan's). Every input is built by code. Each crafted case has a counted test that runs by default,
//! with a bound near the measured value, and an `#[ignore]` test that times it back to back with a yardstick of plain content of the same size and fails
//! when the case is slower (`cargo test -p diacraft-sheetxml --test it model_hostile -- --ignored`). No default test asserts a time.
use std::cell::Cell as StdCell;
#[cfg(target_os = "linux")]
use std::process::Command;
use std::sync::Arc;
use std::time::Instant;

use diacraft_container::xml::{El, parse_with};
use diacraft_doc::{Document, RawNode};
use diacraft_sheetxml::model::cost::{
    ATTR_BYTES, ATTR_PAIR_BYTES, LIST_HEADER_BYTES, PLACED_BYTES, RAW_NODE_BYTES, UNKNOWN_ATTR_BYTES, element_bytes, push_bytes, vec_bytes,
};
use diacraft_sheetxml::model::{BlobId, BlobSource, Budget, CodecError, DecodeNote, MemBlobs, URN, decode_page, decode_shape};
use diacraft_testkit::model::{ModelSize, any_document};
use proptest::prelude::*;

use super::model_cost::{page_of_rectangles, rectangle, tree_items};
use super::model_util::*;

const HUGE: u64 = 1 << 40;

fn big_budget() -> Budget {
    Budget::new(HUGE, HUGE, HUGE)
}

fn page_xml(inner: &str) -> String {
    format!("<diacraft:page xmlns:diacraft=\"{URN}\" id=\"1\">{inner}</diacraft:page>")
}

fn parse_page(xml: &str) -> El {
    parse_with(xml.as_bytes(), &native_limits()).unwrap()
}

/// What the model costs of the part's root as `page_xml` makes it: the page, with a declaration and an ID for attributes.
fn page_model_bytes() -> u64 {
    // The page and its two attributes, and the frame of the declaration it makes: one prefix (named twice) and the namespace it binds it to.
    element_bytes("page") + 2 * ATTR_BYTES + diacraft_sheetxml::ns::frame_bytes(1, 2 * "diacraft".len() as u64 + URN.len() as u64)
}

/// Decodes a page part from `xml` with a budget of `HUGE`, and gives what was charged (items, bytes, blob bytes) with the result.
fn decode_charged(xml: &str, blobs: &dyn BlobSource) -> (Result<diacraft_doc::Page, CodecError>, (u64, u64, u64)) {
    let el = parse_page(xml);
    let mut budget = big_budget();
    let out = decode_page(&el, blobs, &mut budget, &mut Vec::new());
    (out, (HUGE - budget.items, HUGE - budget.bytes, HUGE - budget.blob_bytes))
}

// ---------------------------------------------------------------------------------------------------------------------- budgets

#[test]
fn budget_refuses_before_allocating() {
    let page = page_of_rectangles(1_000);
    let mut blobs = MemBlobs::new();
    let el = diacraft_sheetxml::model::encode_page(&page, &mut blobs).unwrap();
    // Ten items left against a page of a thousand shapes: refused, and what was charged never passes what there was.
    let mut budget = Budget::new(10, HUGE, HUGE);
    let out = decode_page(&el, &blobs, &mut budget, &mut Vec::new());
    assert!(matches!(out, Err(CodecError::Limit { what: "items in the drawing", max: 10 })), "{:?}", out.err());
    assert!(budget.items <= 10);
    // The same for the bytes of text, and for the bytes of a blob.
    let mut budget = Budget::new(HUGE, 100, HUGE);
    assert!(matches!(
        decode_page(&el, &blobs, &mut budget, &mut Vec::new()),
        Err(CodecError::Limit { what: "bytes of the model in memory", max: 100 })
    ));
    let bytes: Arc<[u8]> = Arc::from(vec![1u8; 1_000]);
    let mut s = diacraft_doc::Shape::new(1);
    s.foreign = Some(Arc::new(diacraft_doc::ForeignData {
        picture: Some(Arc::new(diacraft_doc::Media { part: Arc::from("p"), content_type: Arc::from("image/png"), bytes })),
        ..diacraft_doc::ForeignData::default()
    }));
    let mut p = diacraft_doc::Page::new(1, "P");
    p.shapes = diacraft_doc::ShapeList::from_vec(vec![Arc::new(s)]);
    let mut blobs = MemBlobs::new();
    let el = diacraft_sheetxml::model::encode_page(&p, &mut blobs).unwrap();
    let mut budget = Budget::new(HUGE, HUGE, 999);
    assert!(matches!(
        decode_page(&el, &blobs, &mut budget, &mut Vec::new()),
        Err(CodecError::Limit { what: "bytes of pictures and parts in the drawing", max: 999 })
    ));
}

#[test]
fn for_open_is_a_fraction_of_the_package_and_a_budget_is_charged_exactly() {
    let limits = diacraft_container::Limits::default();
    // Sized so that the pools and the trees fit the address space together (`model_pools.rs` holds the figures): at a package of a gibibyte, the
    // default of the container, they are the pools; at a smaller one they follow it.
    let b = Budget::for_open(&limits);
    assert_eq!(limits.max_package_bytes, 1 << 30);
    assert_eq!((b.items, b.bytes, b.blob_bytes), (64_000_000, 960 << 20, 384 << 20));
    let small = diacraft_container::Limits { max_package_bytes: 8_000, ..diacraft_container::Limits::default() };
    let b = Budget::for_open(&small);
    assert_eq!((b.items, b.bytes, b.blob_bytes), (1_000, 1_024_000, 8_000), "a package smaller than the memory figures sets them");
    let mut b = Budget::new(5, 6, 7);
    assert!(b.charge_items(5).is_ok() && b.charge_bytes(6).is_ok() && b.charge_blob(7).is_ok(), "a budget pays exactly what it has");
    assert_eq!((b.items, b.bytes, b.blob_bytes), (0, 0, 0));
    assert!(matches!(b.charge_items(1), Err(CodecError::Limit { max: 5, .. })));
    assert!(matches!(b.charge_bytes(1), Err(CodecError::Limit { max: 6, .. })));
    assert!(matches!(b.charge_blob(1), Err(CodecError::Limit { max: 7, .. })));
    assert!(Budget::new(1, 1, 1).charge_items(u64::MAX).is_err(), "no charge overflows");
    assert_eq!(b.items, 0);
}

#[test]
fn a_formula_is_charged_by_its_nodes_and_one_the_budget_cannot_afford_refuses_the_document() {
    let xml = page_xml("<diacraft:shape><diacraft:sheet><diacraft:Cell N=\"A\" F=\"1+1+1+1+1\"/></diacraft:sheet></diacraft:shape>");
    let (out, (items, bytes, _)) = decode_charged(&xml, &MemBlobs::new());
    assert!(out.is_ok());
    // Nine nodes (five numbers and four sums) at the figure the formula crate gives (128 bytes), and the text of the formula and of the names beside it.
    assert!(bytes >= 9 * 128, "{bytes}");
    assert!(items > 0);
    let el = parse_page(&xml);
    let mut tight = Budget::new(HUGE, 8 * 128, HUGE);
    let out = decode_page(&el, &MemBlobs::new(), &mut tight, &mut Vec::new());
    assert!(matches!(out, Err(CodecError::Limit { what: "bytes of the model in memory", .. })), "{:?}", out.err());
}

// ---------------------------------------------------------------------------------------------------------------------- crafted inputs

/// The page of one million shapes that each name one blob: its XML as text.
fn shapes_naming_one_blob(n: usize) -> String {
    let one = "<diacraft:shape><diacraft:foreign><diacraft:picture blob=\"0\"/></diacraft:foreign></diacraft:shape>";
    page_xml(&one.repeat(n))
}

/// Counts what is asked of it.
struct Counting {
    gets: StdCell<u64>,
    bytes: Arc<[u8]>,
}

impl BlobSource for Counting {
    fn get(&self, id: BlobId) -> Option<Arc<[u8]>> {
        self.gets.set(self.gets.get() + 1);
        (id.0 == 0).then(|| Arc::clone(&self.bytes))
    }
}

#[test]
fn one_blob_named_by_a_million_shapes_is_fetched_and_charged_once() {
    let blobs = Counting { gets: StdCell::new(0), bytes: Arc::from(vec![5u8; 1_000]) };
    let n = 1_000_000;
    let xml = shapes_naming_one_blob(n);
    let (out, (items, bytes, blob)) = decode_charged(&xml, &blobs);
    let page = out.unwrap();
    assert_eq!(page.shapes.len(), n);
    assert_eq!(blobs.gets.get(), 1, "the source is asked once");
    assert_eq!(blob, 1_000, "the blob is charged once");
    let first = page.shapes.get(0).unwrap().foreign.as_ref().unwrap().picture.as_ref().unwrap();
    let last = page.shapes.get(n - 1).unwrap().foreign.as_ref().unwrap().picture.as_ref().unwrap();
    assert!(Arc::ptr_eq(first, last), "one picture, shared");
    // Four items a shape (the shape, `foreign`, `picture` and its `blob`), and the page with its declaration and its ID.
    assert_eq!(items, 4 * n as u64 + 3);
    // The text is the blob's number, a byte each, and the page's ID; the model is a shape, its `foreign` and its `picture` with the attribute that names the
    // blob, a million times, and the page.
    let per_shape = element_bytes("shape") + element_bytes("foreign") + element_bytes("picture") + ATTR_BYTES;
    // Text is charged at nine eighths of its length, rounded up (`text_bytes`): two bytes for a byte.
    let byte = diacraft_sheetxml::model::cost::text_bytes(1);
    assert_eq!(bytes, n as u64 * byte + byte + n as u64 * per_shape + page_model_bytes());
}

/// A shape with a million `attr` children, as text.
fn shape_with_attrs(n: usize) -> String {
    page_xml(&format!("<diacraft:shape>{}</diacraft:shape>", "<diacraft:attr name=\"a\" value=\"b\"/>".repeat(n)))
}

#[test]
fn a_million_attr_children_on_one_element_cost_three_items_each() {
    let n = 1_000_000;
    let (out, (items, bytes, _)) = decode_charged(&shape_with_attrs(n), &MemBlobs::new());
    let page = out.unwrap();
    assert_eq!(page.shapes.get(0).unwrap().raw.len(), n);
    assert_eq!(items, 3 * n as u64 + 4, "the element and its two attributes, and the page (its declaration and its ID) and the shape");
    // The text is a name and a value of a byte each, and the page's ID; the model is an `attr` with its two attributes, a million times, the shape and the page.
    let model = n as u64 * (element_bytes("attr") + 2 * ATTR_BYTES) + element_bytes("shape") + page_model_bytes();
    // And the list the shape keeps them in: room for four at the first, and twice as much each time it is full, which is the room of the last.
    let list = vec_bytes(n, ATTR_PAIR_BYTES) + LIST_HEADER_BYTES;
    // Text is charged at nine eighths of its length, rounded up (`text_bytes`): two bytes for a byte.
    let byte = diacraft_sheetxml::model::cost::text_bytes(1);
    assert_eq!(bytes, 2 * n as u64 * byte + byte + model + list);
}

/// A kept tree of `levels` levels, each holding `siblings` text nodes and the next level, as the text of a page with one shape.
fn kept_tree(levels: usize, siblings: usize) -> String {
    let leaves = "<diacraft:txt>t</diacraft:txt>".repeat(siblings);
    let mut inner = String::from("<diacraft:kept><diacraft:el name=\"l\">");
    for _ in 1..levels {
        inner.push_str(&leaves);
        inner.push_str("<diacraft:el name=\"l\">");
    }
    inner.push_str(&leaves);
    for _ in 0..levels {
        inner.push_str("</diacraft:el>");
    }
    inner.push_str("</diacraft:kept>");
    page_xml(&format!("<diacraft:shape>{inner}</diacraft:shape>"))
}

#[test]
fn a_kept_tree_of_200_levels_of_10000_siblings_is_charged_once_for_each_node() {
    let (levels, siblings) = (200usize, 10_000usize);
    let xml = kept_tree(levels, siblings);
    let el = parse_page(&xml);
    let counted = tree_items(&el);
    let (out, (items, bytes, _)) = decode_charged(&xml, &MemBlobs::new());
    assert!(out.is_ok(), "{:?}", out.err());
    assert_eq!(items, counted, "every element, attribute and text of the tree is charged once, as the XML reader counted it");
    // Each sibling is a text of one byte, and each level an element with a name of one: 10,000 + 1 bytes of text a level, and the page's ID; the model is a `txt`
    // for each sibling, an `el` with its `name` for each level, the `kept`, the shape and the page.
    let model = (levels * siblings) as u64 * element_bytes("txt")
        + levels as u64 * (element_bytes("el") + ATTR_BYTES)
        + element_bytes("kept")
        + element_bytes("shape")
        + page_model_bytes();
    // The list each `el` makes for its children (a level has its siblings and the next level, and the last level only its siblings), and the one the shape
    // keeps its `kept` in.
    let lists = (levels as u64 - 1) * vec_bytes(siblings + 1, RAW_NODE_BYTES) + vec_bytes(siblings, RAW_NODE_BYTES) + push_bytes(0, PLACED_BYTES);
    // Text is charged at nine eighths of its length, rounded up (`text_bytes`): two bytes for a byte.
    let byte = diacraft_sheetxml::model::cost::text_bytes(1);
    assert_eq!(bytes, (levels * (siblings + 1)) as u64 * byte + byte + model + lists);
}

/// A shape with `n` attributes no build knows, as text.
fn shape_with_unknown_attributes(n: usize) -> String {
    let attrs: String = (0..n).map(|i| format!(" a{i}=\"x\"")).collect();
    page_xml(&format!("<diacraft:shape{attrs}/>"))
}

#[test]
fn a_million_unknown_attributes_on_one_element_are_one_item_each_and_are_kept() {
    let n = 1_000_000;
    let (out, (items, bytes, _)) = decode_charged(&shape_with_unknown_attributes(n), &MemBlobs::new());
    let page = out.unwrap();
    let native = page.shapes.get(0).unwrap().ext.get().unwrap().native.clone();
    assert_eq!(native.raw.len(), n);
    assert_eq!(items, n as u64 + 4, "an item for each attribute, the shape, and the page with its declaration and ID");
    // The names are `a` and the number, and each value is one byte; the page's ID is one.
    // Text is charged at nine eighths of its length, rounded up (`text_bytes`): a name of up to eight bytes is one byte more, and a value of one is two.
    let text = diacraft_sheetxml::model::cost::text_bytes;
    let names: u64 = (0..n).map(|i| text(format!("a{i}").len())).sum();
    // Each is kept as a pair of strings in the shape's slot, which the model pays for besides the text; and the shape and the page are the model's.
    let list = vec_bytes(n, ATTR_PAIR_BYTES) + LIST_HEADER_BYTES;
    assert_eq!(
        bytes,
        names + n as u64 * text(1) + text(1) + n as u64 * (UNKNOWN_ATTR_BYTES + ATTR_BYTES) + element_bytes("shape") + page_model_bytes() + list
    );
}

/// `levels` levels of an unknown element, each holding `siblings` more of them and the next level, as text.
fn unknown_tree(levels: usize, siblings: usize) -> String {
    let leaves = "<diacraft:future/>".repeat(siblings);
    let mut inner = String::new();
    for _ in 0..levels {
        inner.push_str("<diacraft:future>");
        inner.push_str(&leaves);
    }
    inner.push_str(&"</diacraft:future>".repeat(levels));
    page_xml(&inner)
}

#[test]
fn an_unknown_tree_of_200_levels_of_5000_siblings_is_charged_once_for_each_node() {
    // 199 levels that each hold the next and 5,000 leaves: the leaves of the last are the 200th level, the most a kept element has.
    let (levels, siblings) = (199usize, 5_000usize);
    let xml = unknown_tree(levels, siblings);
    let counted = tree_items(&parse_page(&xml));
    let (out, (items, ..)) = decode_charged(&xml, &MemBlobs::new());
    let page = out.unwrap();
    assert_eq!(page.ext.get().unwrap().native.kept.len(), 1, "the tree is one unknown element");
    // Every element of the tree is charged once.
    assert_eq!(items, counted);
    // And what is kept is written back whole: the same items again, and the declaration of the codec's prefix, which the root of the tree uses and is put on it.
    let written = diacraft_sheetxml::model::encode_page(&page, &mut MemBlobs::new()).unwrap();
    assert_eq!(tree_items(&written), counted + 1, "the declaration of the codec's prefix is put on the tree's root, which uses it");
}

/// An unknown element of `n` attributes with `n` children each under a prefix of its own: its names' prefixes are looked up against its attributes.
fn unknown_with_many_attributes_and_prefixes(n: usize) -> String {
    let attrs: String = (0..n).map(|i| format!(" a{i}=\"x\"")).collect();
    let kids: String = (0..n).map(|i| format!("<p{i}:c/>")).collect();
    page_xml(&format!("<x:hatch xmlns:x=\"urn:x\"{attrs}>{kids}</x:hatch>"))
}

#[test]
fn an_unknown_element_of_many_attributes_and_many_prefixes_is_read_and_kept() {
    let n = 20_000;
    let xml = unknown_with_many_attributes_and_prefixes(n);
    let (out, (items, ..)) = decode_charged(&xml, &MemBlobs::new());
    let page = out.unwrap();
    let native = &page.ext.get().unwrap().native;
    let Some(RawNode::Element(hatch)) = native.kept.iter().next().map(|p| &p.node) else { panic!("an unknown element") };
    assert_eq!(hatch.children.len(), n);
    // The attributes are its own, and nothing is copied on it for a prefix nothing binds.
    assert_eq!(hatch.attrs.len(), n + 1);
    assert_eq!(items, tree_items(&parse_page(&xml)));
}

/// A shape whose text holds one paragraph of `runs` runs in its ODF data, as text. Each run is one element and two attributes.
fn text_of_runs(runs: usize) -> String {
    let run = "<diacraft:run from=\"1\" to=\"2\"/>".repeat(runs);
    page_xml(&format!(
        "<diacraft:shape id=\"1\"><diacraft:text><diacraft:ext><diacraft:odf><diacraft:paragraph>{run}</diacraft:paragraph></diacraft:odf></diacraft:ext></diacraft:text></diacraft:shape>"
    ))
}

#[test]
fn a_text_of_a_million_runs_is_three_items_each_and_is_read() {
    let n = 1_000_000;
    let xml = text_of_runs(n);
    let (out, (items, ..)) = decode_charged(&xml, &MemBlobs::new());
    let page = out.unwrap();
    let odf = &page.shapes.get(0).unwrap().text.as_ref().unwrap().ext.get().unwrap().odf;
    assert_eq!(odf.paragraphs[0].runs.len(), n);
    assert_eq!(items, tree_items(&parse_page(&xml)));
}

/// A text of one paragraph whose style holds `values` values of the 21 groups in turn, each with an attribute and a stamp, as text.
fn text_of_held_values(values: usize) -> String {
    let groups = diacraft_doc::TextGroup::ALL;
    let held: String = (0..values)
        .map(|i| format!("<diacraft:held-style group=\"{}\" stamp=\"0000000000000001\" a=\"{i}\"/>", groups[i % groups.len()].id()))
        .collect();
    page_xml(&format!(
        "<diacraft:shape id=\"1\"><diacraft:text><diacraft:ext><diacraft:odf><diacraft:paragraph><diacraft:text-style>{held}</diacraft:text-style></diacraft:paragraph></diacraft:odf></diacraft:ext></diacraft:text></diacraft:shape>"
    ))
}

#[test]
fn a_style_of_hundreds_of_thousands_of_values_of_one_group_each_costs_its_items_and_keeps_its_own_unknown_content() {
    let n = 400_000;
    let xml = text_of_held_values(n);
    let (out, (items, ..)) = decode_charged(&xml, &MemBlobs::new());
    let page = out.unwrap();
    let data = page.shapes.get(0).unwrap().text.as_ref().unwrap().ext.get().unwrap();
    assert_eq!(data.odf.paragraphs[0].style.held.len(), n);
    // The attribute `a` is unknown to the codec on each value, and each value keeps it: the key is the group and the ordinal of the value among those of its
    // group (Call 24), so that no value shares a key with another and nothing of a newer build's is lost or counted.
    assert_eq!(data.native.kept.iter().count(), n);
    assert_eq!(items, tree_items(&parse_page(&xml)), "the items of the tree, and no note for what was not kept");
}

#[test]
fn a_million_paragraphs_that_each_keep_unknown_content_are_read() {
    let n = 500_000;
    let inner = "<diacraft:paragraph pf=\"1\"/>".repeat(n);
    let xml = page_xml(&format!(
        "<diacraft:shape id=\"1\"><diacraft:text><diacraft:ext><diacraft:odf>{inner}</diacraft:odf></diacraft:ext></diacraft:text></diacraft:shape>"
    ));
    let (out, (items, ..)) = decode_charged(&xml, &MemBlobs::new());
    let page = out.unwrap();
    let data = page.shapes.get(0).unwrap().text.as_ref().unwrap().ext.get().unwrap();
    assert_eq!(data.odf.paragraphs.len(), n);
    assert_eq!(data.native.kept.iter().count(), n, "one entry for each paragraph, under its ordinal");
    assert_eq!(items, tree_items(&parse_page(&xml)));
}

#[test]
fn a_part_at_the_node_cap_decodes_or_refuses_within_budget() {
    // The open limits' cap is 4,000,000 items to a tree: a million and a third of `attr` children (three items each) are the part at the cap.
    let n = 1_333_320;
    let xml = shape_with_attrs(n);
    let el = parse_page(&xml);
    assert!(tree_items(&el) <= diacraft_sheetxml::model::OPEN_ITEMS);
    let mut budget = Budget::for_open(&diacraft_sheetxml::model::open_limits());
    let before = budget.items;
    assert!(decode_page(&el, &MemBlobs::new(), &mut budget, &mut Vec::new()).is_ok());
    assert_eq!(before - budget.items, tree_items(&el));
    // With a budget of a million items it is refused, having charged no more than there was.
    let mut small = Budget::new(1_000_000, HUGE, HUGE);
    let out = decode_page(&el, &MemBlobs::new(), &mut small, &mut Vec::new());
    assert!(matches!(out, Err(CodecError::Limit { what: "items in the drawing", max: 1_000_000 })), "{:?}", out.err());
    assert!(small.items < 3, "refused as soon as the budget was out: {} left", small.items);
}

// ---------------------------------------------------------------------------------------------------------------------- timed beside a yardstick (ignored)

fn time<T>(f: impl FnOnce() -> T) -> (T, f64) {
    let start = Instant::now();
    let out = f();
    (out, start.elapsed().as_secs_f64())
}

/// A plain page of rectangles with text that holds as many items as `items`, the yardstick of every case below.
fn plain_page_of_items(items: u64) -> String {
    let one = {
        let mut blobs = MemBlobs::new();
        let page = diacraft_doc::Page { shapes: diacraft_doc::ShapeList::from_vec(vec![Arc::new(rectangle(1))]), ..diacraft_doc::Page::new(1, "P") };
        let el = diacraft_sheetxml::model::encode_page(&page, &mut blobs).unwrap();
        String::from_utf8(bytes_of_el(&el)).unwrap()
    };
    let shape_start = one.find("<diacraft:shape").unwrap();
    let shape_end = one.rfind("</diacraft:shape>").unwrap() + "</diacraft:shape>".len();
    let shape = &one[shape_start..shape_end];
    let per_shape = tree_items(&parse_page(&page_xml(shape))) - tree_items(&parse_page(&page_xml("")));
    let copies = usize::try_from(items / per_shape).unwrap() + 1;
    page_xml(&shape.repeat(copies))
}

/// Decodes the crafted input and a plain page of the same number of items back to back, and fails when the crafted one is slower than 3 times the plain one
/// (the room a first run's noise needs; the defects this guards against are 10 times or quadratic).
fn beside_the_yardstick(name: &str, crafted: &str, blobs: &dyn BlobSource) {
    let crafted_el = parse_page(crafted);
    let plain_el = parse_page(&plain_page_of_items(tree_items(&crafted_el)));
    let (_, plain_time) = time(|| decode_page(&plain_el, &MemBlobs::new(), &mut big_budget(), &mut Vec::new()).unwrap());
    let (_, crafted_time) = time(|| decode_page(&crafted_el, blobs, &mut big_budget(), &mut Vec::new()).unwrap());
    eprintln!("{name}: crafted {crafted_time:.3} s, yardstick {plain_time:.3} s");
    assert!(
        crafted_time <= 3.0 * plain_time.max(0.05),
        "{name}: {crafted_time:.3} s against {plain_time:.3} s for plain content of the same number of items"
    );
}

#[test]
#[ignore = "timed beside a yardstick; run with --ignored"]
fn timed_one_blob_named_by_a_million_shapes() {
    let blobs = Counting { gets: StdCell::new(0), bytes: Arc::from(vec![5u8; 1_000]) };
    beside_the_yardstick("one blob, a million shapes", &shapes_naming_one_blob(1_000_000), &blobs);
}

#[test]
#[ignore = "timed beside a yardstick; run with --ignored"]
fn timed_a_million_attr_children() {
    beside_the_yardstick("a million attr children", &shape_with_attrs(1_000_000), &MemBlobs::new());
}

#[test]
#[ignore = "timed beside a yardstick; run with --ignored"]
fn timed_a_kept_tree_of_200_levels_of_10000_siblings() {
    beside_the_yardstick("a kept tree 200 by 10,000", &kept_tree(200, 10_000), &MemBlobs::new());
}

#[test]
#[ignore = "timed beside a yardstick; run with --ignored"]
fn timed_a_million_unknown_attributes() {
    beside_the_yardstick("a million unknown attributes", &shape_with_unknown_attributes(1_000_000), &MemBlobs::new());
}

#[test]
#[ignore = "timed beside a yardstick; run with --ignored"]
fn timed_an_unknown_tree_of_200_levels_of_5000_siblings() {
    beside_the_yardstick("an unknown tree 200 by 5,000", &unknown_tree(199, 5_000), &MemBlobs::new());
}

#[test]
#[ignore = "timed beside a yardstick; run with --ignored"]
fn timed_an_unknown_element_of_many_attributes_and_many_prefixes() {
    // The names of its children are looked up against its attributes: linear, so that four times the input takes about four times as long (a lookup in a
    // list of its attributes for each prefix would take sixteen). The yardstick is the same input at a quarter of the size.
    let time_of = |n: usize| {
        let el = parse_page(&unknown_with_many_attributes_and_prefixes(n));
        time(|| decode_page(&el, &MemBlobs::new(), &mut big_budget(), &mut Vec::new()).unwrap()).1
    };
    let (small, large) = (time_of(25_000).max(0.02), time_of(100_000));
    eprintln!("an unknown element of attributes and prefixes: 25,000 take {small:.3} s, 100,000 take {large:.3} s");
    assert!(large <= 8.0 * small, "four times the input took {:.1} times as long", large / small);
}

#[test]
#[ignore = "timed beside a yardstick; run with --ignored"]
fn timed_a_text_of_a_million_runs() {
    beside_the_yardstick("a text of a million runs", &text_of_runs(1_000_000), &MemBlobs::new());
}

#[test]
#[ignore = "timed beside a yardstick; run with --ignored"]
fn timed_a_style_of_400000_held_values() {
    beside_the_yardstick("a style of 400,000 held values", &text_of_held_values(400_000), &MemBlobs::new());
}

#[test]
#[ignore = "timed beside a yardstick; run with --ignored"]
fn timed_500000_paragraphs_with_unknown_content() {
    let inner = "<diacraft:paragraph pf=\"1\"/>".repeat(500_000);
    let xml = page_xml(&format!(
        "<diacraft:shape id=\"1\"><diacraft:text><diacraft:ext><diacraft:odf>{inner}</diacraft:odf></diacraft:ext></diacraft:text></diacraft:shape>"
    ));
    beside_the_yardstick("500,000 paragraphs with unknown content", &xml, &MemBlobs::new());
}

#[test]
#[ignore = "timed beside a yardstick; run with --ignored"]
fn timed_shapes_nested_to_the_cap() {
    let mut chain = String::from("<diacraft:shape id=\"1\"/>");
    for _ in 1..200 {
        chain = format!("<diacraft:shape id=\"1\">{chain}</diacraft:shape>");
    }
    // 2,000 chains of 200 levels each: 400,000 shapes, every one of them below 199 others.
    beside_the_yardstick("shapes nested to the cap", &page_xml(&chain.repeat(2_000)), &MemBlobs::new());
}

// ---------------------------------------------------------------------------------------------------------------------- never panic

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    /// One byte of a valid encoding changed, in any part: a refusal or a document, never a panic. (A byte that makes the XML not well-formed is the XML
    /// reader's refusal, which the decoder never sees.)
    #[test]
    fn mutated_encodings_never_panic(m in any_document(ModelSize::Small), part in 0usize..4, at in any::<prop::sample::Index>(), byte in any::<u8>()) {
        let mut p = pack(&m);
        let target: &mut Vec<u8> = match part {
            0 => &mut p.document,
            1 => match p.pages.first_mut() { Some(b) => b, None => &mut p.document },
            2 => match p.masters.first_mut() { Some(b) => b, None => &mut p.document },
            _ => match p.pages.last_mut() { Some(b) => b, None => &mut p.document },
        };
        if target.is_empty() {
            return Ok(());
        }
        let i = at.index(target.len());
        if let Some(slot) = target.get_mut(i) {
            *slot = byte;
        }
        let limits = native_limits();
        let Ok(document) = parse_with(&p.document, &limits) else { return Ok(()) };
        let pages: Vec<Option<El>> = p.pages.iter().map(|b| parse_with(b, &limits).ok()).collect();
        let masters: Vec<Option<El>> = p.masters.iter().map(|b| parse_with(b, &limits).ok()).collect();
        if pages.iter().chain(&masters).any(Option::is_none) {
            return Ok(());
        }
        let page_refs: Vec<&El> = pages.iter().flatten().collect();
        let master_refs: Vec<&El> = masters.iter().flatten().collect();
        let mut notes: Vec<DecodeNote> = Vec::new();
        let out = diacraft_sheetxml::model::decode(&document, &page_refs, &master_refs, &p.blobs, &mut Budget::for_open(&diacraft_container::Limits::default()), &mut notes);
        if let Ok(doc) = out {
            // What decodes passes the model's own checks, and encodes again.
            prop_assert!(doc.check_limits().is_ok());
            let _: Document = round_trip(&doc);
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------- a refusal is not an abort

const SCENARIO: &str = "SHEETXML_MODEL_MEMORY_SCENARIO";

/// A line of `/proc/self/status`, in kibibytes.
fn status_kib(key: &str) -> u64 {
    let text = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    text.lines()
        .find_map(|l| l.strip_prefix(key))
        .and_then(|rest| rest.trim_start_matches(':').split_whitespace().next().and_then(|n| n.parse().ok()))
        .unwrap_or(0)
}

#[test]
#[ignore = "run by the tests below under `ulimit -v`; alone it needs the scenario in the environment"]
fn child() {
    let scenario = std::env::var(SCENARIO).unwrap_or_default();
    match scenario.as_str() {
        // 300 shapes that each hold a name of 1 MiB (300 MiB of text in the tree), decoded with 8 MiB of bytes: refused, and what was copied is the 8 MiB.
        "text_past_the_budget" => {
            let value = "n".repeat(1 << 20);
            let shapes: String = (0..300).map(|_| format!("<diacraft:shape name=\"{value}\"/>")).collect();
            let el = parse_page(&page_xml(&shapes));
            drop(shapes);
            drop(value);
            // The peak is reset after the input is built and dropped, so that what is measured is the decode's own growth.
            let measure = |budget: &mut Budget| {
                std::fs::write("/proc/self/clear_refs", "5").expect("reset the peak resident size");
                let at_start = status_kib("VmHWM");
                let out = decode_page(&el, &MemBlobs::new(), budget, &mut Vec::new());
                (out, status_kib("VmHWM").saturating_sub(at_start))
            };
            let (out, grew) = measure(&mut Budget::new(HUGE, 8 << 20, HUGE));
            assert!(matches!(out, Err(CodecError::Limit { what: "bytes of the model in memory", .. })), "{:?}", out.err());
            assert!(grew < 64 * 1024, "the decode grew the process by {grew} KiB against a budget of 8 MiB");
            // The control, after: with a budget that pays for every byte the decode copies all 300 MiB, and the measure sees it (or it would see nothing).
            let (control, control_grew) = measure(&mut Budget::new(HUGE, HUGE, HUGE));
            assert!(control.is_ok());
            assert!(control_grew > 200 * 1024, "the measure sees a decode that copies 300 MiB: it grew {control_grew} KiB");
            println!("refused: grew {grew} KiB against {control_grew} KiB unbudgeted");
        }
        // The reviewer's case: eight pages of a million `<shape/>`, 64 MB of file and eight million items, which the open budget allowed because an
        // element cost one item, and built 338 bytes of model each: 2.7 GB, and the process aborted at an allocation under this limit. The model is charged
        // as it is built, so the decode is refused in words with the model at a gibibyte at the most, beside the trees the caller holds.
        "eight_pages_of_a_million_shapes" => {
            let pages = 8usize;
            let page_text = format!("<page xmlns=\"{URN}\">{}</page>", "<shape/>".repeat(1_000_000));
            let doc_text = format!("<document xmlns=\"{URN}\">{}</document>", "<page-ref/>".repeat(pages));
            let document = parse_page(&doc_text);
            let one = parse_page(&page_text);
            drop(page_text);
            let trees: Vec<El> = (0..pages).map(|_| one.clone()).collect();
            drop(one);
            let refs: Vec<&El> = trees.iter().collect();
            let mut budget = Budget::for_open(&diacraft_container::Limits::default());
            let out = diacraft_sheetxml::model::decode(&document, &refs, &[], &MemBlobs::new(), &mut budget, &mut Vec::new());
            let refusal = out.expect_err("eight million shapes are more than the open budget pays for");
            assert!(matches!(&refusal, CodecError::Limit { what: "bytes of the model in memory", .. }), "{refusal:?}");
            assert_eq!(refusal.class(), "model_too_complex");
            println!("refused in words: {refusal}");
        }
        // A blob record that states 2^40 bytes is refused before anything is allocated or fetched.
        "a_blob_that_claims_a_terabyte" => {
            let mut blobs = MemBlobs::new();
            let bytes: Arc<[u8]> = Arc::from(&b"png"[..]);
            let mut s = diacraft_doc::Shape::new(1);
            s.foreign = Some(Arc::new(diacraft_doc::ForeignData {
                picture: Some(Arc::new(diacraft_doc::Media { part: Arc::from("p"), content_type: Arc::from("image/png"), bytes })),
                ..diacraft_doc::ForeignData::default()
            }));
            let mut d = Document::empty();
            let mut p = diacraft_doc::Page::new(1, "P");
            p.shapes = diacraft_doc::ShapeList::from_vec(vec![Arc::new(s)]);
            d.pages.push(Arc::new(p));
            let e = diacraft_sheetxml::model::encode(&d, &mut blobs).unwrap();
            let mut packed = pack_encoded(&e, blobs);
            let text = String::from_utf8(packed.document.clone()).unwrap().replace("size=\"3\"", "size=\"1099511627776\"");
            packed.document = text.into_bytes();
            let out = unpack_with(&packed, &packed.blobs, &mut Budget::for_open(&diacraft_container::Limits::default()));
            assert!(matches!(out, Err(CodecError::Limit { what: "bytes of pictures and parts in the drawing", .. })), "{:?}", out.err());
            println!("refused");
        }
        other => panic!("no scenario {other:?}"),
    }
}

#[cfg(target_os = "linux")]
fn run_child(scenario: &str) -> (bool, String) {
    let exe = std::env::current_exe().unwrap();
    let script = "ulimit -v 3000000 && exec \"$0\" --exact model_hostile::child --ignored --nocapture --test-threads=1";
    let out = Command::new("/bin/sh").arg("-c").arg(script).arg(exe).env(SCENARIO, scenario).output().unwrap();
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    (out.status.success(), text)
}

#[test]
#[cfg(target_os = "linux")]
fn text_past_the_budget_refuses_and_does_not_abort_under_an_address_limit() {
    let (ok, text) = run_child("text_past_the_budget");
    assert!(ok && text.contains("refused: grew"), "{text}");
}

#[test]
#[cfg(target_os = "linux")]
fn eight_pages_of_a_million_shapes_are_refused_in_words_and_do_not_abort_under_an_address_limit() {
    let (ok, text) = run_child("eight_pages_of_a_million_shapes");
    assert!(ok && text.contains("refused in words: "), "{text}");
}

#[test]
#[cfg(target_os = "linux")]
fn a_blob_that_claims_a_terabyte_refuses_and_does_not_abort_under_an_address_limit() {
    let (ok, text) = run_child("a_blob_that_claims_a_terabyte");
    assert!(ok && text.contains("refused"), "{text}");
}

#[test]
fn a_shape_decode_leaves_no_note_for_what_it_understood() {
    let mut notes = Vec::new();
    let el = {
        let mut blobs = MemBlobs::new();
        diacraft_sheetxml::model::encode_shape(&rectangle(1), &mut blobs).unwrap()
    };
    let s = decode_shape(&el, &MemBlobs::new(), &mut big_budget(), &mut notes).unwrap();
    assert_eq!(s, rectangle(1));
    assert!(notes.is_empty());
}
