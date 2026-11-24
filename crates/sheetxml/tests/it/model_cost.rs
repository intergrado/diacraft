//! What the codec costs, counted in the units its loops walk (elements, attributes, bytes), never in seconds (the plan's). A cost that is
//! timed is an `#[ignore]` test in `model_hostile.rs`, run beside its yardstick.
use std::sync::Arc;

use diacraft_container::xml::{El, Node};
use diacraft_doc::{Cell, CellName, Document, Page, Passthrough, RawAttrs, RawElement, RawNode, Shape, ShapeList, Sheet, Text, TextPart, Value};
use diacraft_sheetxml::model::cost;
use diacraft_sheetxml::model::{Budget, CodecError, MemBlobs, StampCost, content_stamp_counted, decode_page, decode_shape};

/// A budget that is more than any test spends, so that what is left is what was spent.
const HUGE: u64 = 1 << 40;

fn big_budget() -> Budget {
    Budget::new(HUGE, HUGE, HUGE)
}

/// A rectangle with text, as a drawing holds many: four cells and a run of characters.
pub fn rectangle(i: u32) -> Shape {
    let mut sheet = Sheet::default();
    // Each rectangle is at a place of its own (a sheet that is equal to another's is written once and said again, and these cost tests are of a part that says every unit
    // in full: what the decode charges is then what the parse of the part counted).
    for (name, value) in [("PinX", i.to_string()), ("PinY", "2".to_owned()), ("Width", "3".to_owned()), ("Height", "4".to_owned())] {
        sheet.cells.insert(CellName::new(name), Cell::constant(Value::from_text(&value)).with_unit("IN"));
    }
    Shape {
        id: i,
        name: Some(Arc::from(format!("Sheet.{i}"))),
        sheet: Arc::new(sheet),
        text: Some(Arc::new(Text { parts: vec![TextPart::Chars(Arc::from("text"))], ..Text::default() })),
        ..Shape::default()
    }
}

pub fn page_of_rectangles(n: u32) -> Page {
    let mut p = Page::new(1, "P");
    p.shapes = ShapeList::from_vec((0..n).map(|i| Arc::new(rectangle(i))).collect());
    p
}

/// What decoding the page part of `page` is charged: items, bytes and blob bytes.
fn charged(page: &Page) -> (u64, u64, u64) {
    let mut blobs = MemBlobs::new();
    let el = diacraft_sheetxml::model::encode_page(page, &mut blobs).unwrap();
    let mut budget = big_budget();
    let mut notes = Vec::new();
    decode_page(&el, &blobs, &mut budget, &mut notes).unwrap();
    (HUGE - budget.items, HUGE - budget.bytes, HUGE - budget.blob_bytes)
}

#[test]
fn decode_cost_is_linear_in_elements() {
    let (small, ..) = charged(&page_of_rectangles(10_000));
    let (large, ..) = charged(&page_of_rectangles(40_000));
    let ratio = large as f64 / small as f64;
    assert!((3.95..4.05).contains(&ratio), "{small} items for 10,000 shapes and {large} for 40,000 is {ratio}");
    // And it is what the XML reader counts for the same tree, one item for each element, attribute and text node.
    let mut blobs = MemBlobs::new();
    let el = diacraft_sheetxml::model::encode_page(&page_of_rectangles(1_000), &mut blobs).unwrap();
    assert_eq!(charged(&page_of_rectangles(1_000)).0, tree_items(&el), "the decode charges what the parse of the part counted");
}

/// The items of a tree as the XML reader counts them: each element, each attribute, each text node.
pub fn tree_items(root: &El) -> u64 {
    let mut total = 0u64;
    let mut stack = vec![root];
    while let Some(e) = stack.pop() {
        total += 1 + e.attrs.len() as u64;
        for n in &e.children {
            match n {
                Node::El(c) => stack.push(c),
                _ => total += 1,
            }
        }
    }
    total
}

/// A chain of `n` shapes by hand, each the only member of the one before it: the encoder refuses to write one past 200.
fn chain_el(n: usize) -> El {
    let mut inner = El { name: "diacraft:shape".to_owned(), attrs: vec![("id".to_owned(), "1".to_owned())], children: Vec::new() };
    for _ in 1..n {
        inner = El { name: "diacraft:shape".to_owned(), attrs: vec![("id".to_owned(), "1".to_owned())], children: vec![Node::El(inner)] };
    }
    inner.attrs.push(("xmlns:diacraft".to_owned(), diacraft_sheetxml::model::URN.to_owned()));
    inner
}

#[test]
fn group_depth_is_refused_before_the_shape_is_built() {
    let run = |n: usize| {
        let mut budget = big_budget();
        let out = decode_shape(&chain_el(n), &MemBlobs::new(), &mut budget, &mut Vec::new());
        (out, HUGE - budget.items)
    };
    let (ok, at_cap) = run(200);
    assert!(ok.is_ok());
    let (refused, past_cap) = run(201);
    assert!(matches!(refused, Err(CodecError::Limit { what: "levels of nested shapes", max: 200 })), "{:?}", refused.err());
    // A shape of this chain costs two items (the element and its `id`, and the namespace declaration on the first): the 201st was charged nothing.
    assert_eq!(past_cap, at_cap, "the shape past the cap was refused before anything was charged for it");
}

#[test]
fn a_long_value_is_charged_once_per_copy() {
    // An element nested 200 deep that holds a value of 1 MiB: the bytes are charged once, and not once for each ancestor.
    let mut e = RawElement { name: Arc::from("x:leaf"), attrs: RawAttrs::from_pairs([("big", "v".repeat(1 << 20).as_str())]), children: Vec::new() };
    for _ in 1..200 {
        e = RawElement { name: Arc::from("x:l"), attrs: RawAttrs::new(), children: vec![RawNode::Element(e)] };
    }
    let mut s = Shape::new(1);
    s.passthrough = Passthrough::from_nodes(vec![diacraft_doc::Placed { after: 0, node: RawNode::Element(e) }]);
    let mut p = Page::new(1, "P");
    p.shapes = ShapeList::from_vec(vec![Arc::new(s)]);
    let (_, bytes, _) = charged(&p);
    // Text is charged at nine eighths of its length, rounded up (`cost::text_bytes`).
    let value = cost::text_bytes(1 << 20);
    assert!(bytes >= value, "the value is charged: {bytes}");
    // And once: besides the megabyte, the model pays for the 200 elements it keeps and the attribute, and the shape and the page.
    let model = 200 * (cost::UNKNOWN_ELEMENT_BYTES + 16)
        + cost::UNKNOWN_ATTR_BYTES
        + cost::ATTR_BYTES
        + cost::element_bytes("shape")
        + cost::element_bytes("page");
    assert!(bytes < value + model + 4_096, "and once: {bytes}");
}

#[test]
fn a_blob_is_charged_once_however_many_shapes_name_it() {
    let bytes: Arc<[u8]> = Arc::from(vec![9u8; 5_000]);
    let mut p = Page::new(1, "P");
    p.shapes = ShapeList::from_vec(
        (0..300u32)
            .map(|i| {
                let mut s = Shape::new(i);
                s.foreign = Some(Arc::new(diacraft_doc::ForeignData {
                    picture: Some(Arc::new(diacraft_doc::Media {
                        part: Arc::from("p"),
                        content_type: Arc::from("image/png"),
                        bytes: Arc::clone(&bytes),
                    })),
                    ..diacraft_doc::ForeignData::default()
                }));
                Arc::new(s)
            })
            .collect(),
    );
    let (_, _, blob) = charged(&p);
    assert_eq!(blob, 5_000);
}

/// A drawing of `pages` pages of `shapes` rectangles each, every page naming one shared `Arc` of the shapes, so that it is cheap to hold and its encoding is
/// as large as the pages are.
fn big_document(pages: usize, shapes: usize) -> Document {
    let shared: Vec<Arc<Shape>> = (0..shapes as u32).map(|i| Arc::new(rectangle(i))).collect();
    let mut d = Document::empty();
    for id in 0..pages as u32 {
        let mut p = Page::new(id, "P");
        p.shapes = ShapeList::from_vec(shared.clone());
        d.pages.push(Arc::new(p));
    }
    d
}

#[test]
fn content_stamp_encodes_one_part_at_a_time() {
    let d = big_document(50, 20_000);
    let mut budget = Budget::new(HUGE, HUGE, HUGE);
    let (stamp, cost): (_, StampCost) = content_stamp_counted(&d, &mut budget).unwrap();
    assert_eq!(cost.parts, 51, "the document and each page");
    let one_page = tree_items(&diacraft_sheetxml::model::encode_page(d.pages.get(1).unwrap(), &mut MemBlobs::new()).unwrap());
    assert_eq!(cost.largest_part_items, one_page, "the largest tree held at one time is one page's");
    assert!(cost.items > 50 * one_page, "and the parts together are far more: {} against {one_page}", cost.items);
    assert_eq!(HUGE - budget.items, cost.items, "the budget paid for every tree");
    assert_eq!(stamp, content_stamp_counted(&d, &mut Budget::new(HUGE, HUGE, HUGE)).unwrap().0);
}

/// The elements and attributes of a tree: what a decode must take from its budget at the least.
fn elements_and_attributes(root: &El) -> u64 {
    let mut total = 0u64;
    let mut stack = vec![root];
    while let Some(e) = stack.pop() {
        total += 1 + u64::try_from(e.attrs.len()).unwrap_or(u64::MAX);
        stack.extend(e.elements());
    }
    total
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig { cases: 128, ..proptest::prelude::ProptestConfig::default() })]

    /// A decode takes an item from the budget for every element and every attribute of every part, whatever the element is for: the number the trees
    /// hold, counted here from the parsed bytes and not by the codec, is the least it takes (text and formulas cost more).
    #[test]
    fn a_decode_takes_at_least_an_item_for_each_element_and_attribute_of_every_part(
        m in diacraft_testkit::model::any_document(diacraft_testkit::model::ModelSize::Small)
    ) {
        use super::model_util::{native_limits, pack, unpack_with};
        let packed = pack(&m);
        let counted: u64 = packed
            .all_bytes()
            .iter()
            .map(|b| elements_and_attributes(&diacraft_container::xml::parse_with(b, &native_limits()).unwrap()))
            .sum();
        let mut budget = big_budget();
        unpack_with(&packed, &packed.blobs, &mut budget).unwrap();
        let taken = HUGE - budget.items;
        proptest::prop_assert!(taken >= counted, "the trees hold {counted} elements and attributes and the decode took {taken}");
    }
}
