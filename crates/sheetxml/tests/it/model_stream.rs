//! Item 1: **a page is read and written as a stream of its children.** A source that holds a part as bytes ([`PartSource::page_bytes`]) has the decoder read the
//! root's start tag and then each child of the root, whole, one at a time, so that what is held of the part is the bytes and one child's tree; a page of any number of shapes is
//! read, where a page held as a tree is at most [`OPEN_TREE_ITEMS`] items. What a stream reads is what the tree of the same bytes reads, and is charged the same.
use std::borrow::Cow;
use std::sync::Arc;

use diacraft_container::xml::{El, parse_with};
use diacraft_doc::{Cell, CellAddr, Document, Master, Page, Shape, ShapeList, Value};
use diacraft_sheetxml::model::{
    Budget, CodecError, DecodeNote, OPEN_ELEMENT_ITEMS, OPEN_PART_BYTES, PartSource, check_decodes_from, decode, decode_from, open_limits,
    written_len,
};
use diacraft_testkit::model::{ModelSize, any_document};
use proptest::prelude::*;

use super::model_diff::first_difference;
use super::model_util::{Packed, native_limits, pack};

const HUGE: u64 = 1 << 40;

/// A source that holds each part as bytes and says so: it has no tree to give, so a decode that asks for one fails.
struct Bytes {
    pages: Vec<Vec<u8>>,
    masters: Vec<Vec<u8>>,
    asked: Vec<String>,
}

impl Bytes {
    fn of(p: &Packed) -> Bytes {
        Bytes { pages: p.pages.clone(), masters: p.masters.clone(), asked: Vec::new() }
    }

    /// The parts of `p` with its pages as `pages` says.
    fn with_pages(p: &Packed, pages: Vec<Vec<u8>>) -> Bytes {
        Bytes { pages, masters: p.masters.clone(), asked: Vec::new() }
    }
}

impl PartSource for Bytes {
    type Tree = El;

    fn page(&mut self, _index: usize, _name: &str) -> Result<Self::Tree, CodecError> {
        Err(CodecError::Malformed { part: "page".to_owned(), why: "the tree was asked for, and the part is held as bytes" })
    }

    fn master(&mut self, _index: usize, _name: &str) -> Result<Self::Tree, CodecError> {
        Err(CodecError::Malformed { part: "master".to_owned(), why: "the tree was asked for, and the part is held as bytes" })
    }

    fn page_bytes(&mut self, index: usize, name: &str) -> Result<Option<Cow<'_, [u8]>>, CodecError> {
        self.asked.push(format!("page {index} {name}"));
        Ok(self.pages.get(index).map(|b| Cow::Borrowed(b.as_slice())))
    }

    fn master_bytes(&mut self, index: usize, name: &str) -> Result<Option<Cow<'_, [u8]>>, CodecError> {
        self.asked.push(format!("master {index} {name}"));
        Ok(self.masters.get(index).map(|b| Cow::Borrowed(b.as_slice())))
    }
}

/// What a decode gave: the drawing, the notes, and what was left of the budget (items, bytes, blob bytes).
type Opened = Result<(Document, Vec<DecodeNote>, (u64, u64, u64)), CodecError>;

/// Opens the parts of `p` with its pages as `pages` says, as a stream of the children of each part and as the trees of the parts, each within a budget of its own (the same).
fn both(p: &Packed, pages: &[Vec<u8>], budget: &Budget) -> (Opened, Opened) {
    let document = parse_with(&p.document, &native_limits()).unwrap();
    let mut a = budget.clone();
    let mut notes_a = Vec::new();
    let streamed = decode_from(&document, &mut Bytes::with_pages(p, pages.to_vec()), &p.blobs, &mut a, &mut notes_a)
        .map(|d| (d, notes_a, (a.items, a.bytes, a.blob_bytes)));
    let page_trees: Vec<El> = pages.iter().map(|b| parse_with(b, &native_limits()).unwrap()).collect();
    let master_trees: Vec<El> = p.masters.iter().map(|b| parse_with(b, &native_limits()).unwrap()).collect();
    let (page_refs, master_refs) = (page_trees.iter().collect::<Vec<_>>(), master_trees.iter().collect::<Vec<_>>());
    let mut b = budget.clone();
    let mut notes_b = Vec::new();
    let treed = decode(&document, &page_refs, &master_refs, &p.blobs, &mut b, &mut notes_b).map(|d| (d, notes_b, (b.items, b.bytes, b.blob_bytes)));
    (streamed, treed)
}

fn cell(v: f64) -> Cell {
    Cell::constant(Value::from_text(&v.to_string()))
}

/// A drawing of `pages` pages of `shapes` shapes each (every shape with a cell of its own) and a master of two.
fn drawing(pages: u32, shapes: u32) -> Document {
    let mut doc = Document::empty();
    for p in 0..pages {
        let mut page = Page::new(p + 1, &format!("page {p}"));
        page.shapes = ShapeList::from_vec(
            (0..shapes)
                .map(|i| {
                    let mut s = Shape::new(i + 1);
                    Arc::make_mut(&mut s.sheet).set_cell(&CellAddr::direct("PinX"), cell(f64::from(i))).unwrap();
                    Arc::new(s)
                })
                .collect(),
        );
        doc.pages.push(Arc::new(page));
    }
    doc.masters.push(Arc::new(Master {
        id: 10,
        shapes: ShapeList::from_vec(vec![Arc::new(Shape::new(1)), Arc::new(Shape::new(2))]),
        ..Master::default()
    }));
    doc
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, ..ProptestConfig::default() })]

    /// What a stream of the children of each part reads is what the tree of the same bytes reads: the same drawing, the same notes of what was lost, and the same budget
    /// left, over every generated drawing (which says units once, holds unknown content, and uses every field).
    #[test]
    fn a_stream_reads_what_the_tree_of_the_same_bytes_reads_and_spends_what_it_spends(m in any_document(ModelSize::Small)) {
        let p = pack(&m);
        let (streamed, treed) = both(&p, &p.pages, &Budget::new(HUGE, HUGE, HUGE));
        let (streamed, treed) = (streamed.unwrap(), treed.unwrap());
        prop_assert!(streamed.0 == treed.0, "{}", first_difference(&streamed.0, &treed.0).unwrap_or_default());
        prop_assert_eq!(&streamed.1, &treed.1);
        prop_assert_eq!(streamed.2, treed.2);
        prop_assert!(streamed.0 == m, "{}", first_difference(&streamed.0, &m).unwrap_or_default());
    }
}

#[test]
fn a_stream_is_asked_for_each_part_once_in_order_and_the_tree_is_never_asked_for() {
    let doc = drawing(3, 4);
    let p = pack(&doc);
    let document = parse_with(&p.document, &native_limits()).unwrap();
    let mut source = Bytes::of(&p);
    let back = decode_from(&document, &mut source, &p.blobs, &mut Budget::new(HUGE, HUGE, HUGE), &mut Vec::new()).unwrap();
    assert_eq!(back, doc);
    assert_eq!(
        source.asked,
        ["page 0 pages/page1.xml", "page 1 pages/page2.xml", "page 2 pages/page3.xml", "master 0 masters/master10.xml"].map(str::to_owned)
    );
}

#[test]
fn a_check_reads_the_stream_as_a_decode_does_and_spends_what_it_spends() {
    let doc = drawing(2, 6);
    let p = pack(&doc);
    let document = parse_with(&p.document, &native_limits()).unwrap();
    let (mut a, mut b) = (Budget::for_open(&open_limits()), Budget::for_open(&open_limits()));
    decode_from(&document, &mut Bytes::of(&p), &p.blobs, &mut a, &mut Vec::new()).unwrap();
    check_decodes_from(&document, &mut Bytes::of(&p), &p.blobs, &mut b, &mut Vec::new()).unwrap();
    assert_eq!((a.items, a.bytes, a.blob_bytes), (b.items, b.bytes, b.blob_bytes));
}

#[test]
fn a_long_field_on_the_root_is_read_where_it_stands_among_the_children() {
    // A page whose name is too long for an attribute is written with an `s` child for it; one written by another program may put the child after the shapes.
    let name = "n".repeat(300_000);
    let mut doc = drawing(1, 3);
    let mut page = doc.pages.get(0).map(|p| (**p).clone()).unwrap();
    page.name = Arc::from(name.as_str());
    doc.pages = [Arc::new(page)].into_iter().collect();
    let p = pack(&doc);
    let text = String::from_utf8(p.pages[0].clone()).unwrap();
    let s_at = text.find("<diacraft:s ").expect("the name is a child element");
    let s_end = text[s_at..].find("</diacraft:s>").unwrap() + s_at + "</diacraft:s>".len();
    let long = text[s_at..s_end].to_owned();
    // The same page with the child moved after the last shape.
    let moved = format!("{}{}{}", &text[..s_at], &text[s_end..text.rfind("</diacraft:page>").unwrap()], long) + "</diacraft:page>";
    for bytes in [text.clone(), moved] {
        let (streamed, treed) = both(&p, &[bytes.into_bytes()], &Budget::new(HUGE, HUGE, HUGE));
        let (streamed, treed) = (streamed.unwrap(), treed.unwrap());
        assert_eq!(streamed.0.pages.get(0).map(|p| p.name.to_string()), Some(name.clone()));
        assert_eq!(streamed.0, treed.0);
        assert_eq!((streamed.1, streamed.2), (treed.1, treed.2));
    }
}

#[test]
fn a_page_of_more_items_than_a_tree_may_have_is_read_as_a_stream() {
    // 50,000 shapes of about thirty items each: one page of a million and a half items. A tree held to 100,000 refuses it, and a stream, whose children are a shape each,
    // reads it (the same at the open's 4 million items is the capacity test's, which builds a drawing of 150,000 shapes in a child of its own).
    let doc = {
        let mut doc = Document::empty();
        let mut page = Page::new(1, "big");
        page.shapes = ShapeList::from_vec(
            (0..50_000u32)
                .map(|i| {
                    let mut s = Shape::new(i + 1);
                    let sheet = Arc::make_mut(&mut s.sheet);
                    for (n, name) in ["PinX", "PinY", "Width", "Height", "LocPinX", "LocPinY", "Angle", "FillForegnd", "LineColor"].iter().enumerate()
                    {
                        sheet.set_cell(&CellAddr::direct(name), cell(f64::from(i) + n as f64)).unwrap();
                    }
                    Arc::new(s)
                })
                .collect(),
        );
        doc.pages.push(Arc::new(page));
        doc
    };
    let p = pack(&doc);
    let bytes = written_len(&parse_with(&p.pages[0], &native_limits()).unwrap());
    assert!(bytes > 1_000_000, "{bytes} bytes");
    let document = parse_with(&p.document, &native_limits()).unwrap();
    // As a tree held to 100,000 items it is refused...
    let mut limits = open_limits();
    limits.xml.max_nodes = 100_000;
    assert!(parse_with(&p.pages[0], &limits.xml).is_err(), "the tree of the page is past 100,000 items");
    // ...and as a stream, read.
    let back = decode_from(&document, &mut Bytes::of(&p), &p.blobs, &mut Budget::for_open(&open_limits()), &mut Vec::new()).unwrap();
    assert_eq!(back, doc);
}

#[test]
fn a_stream_refuses_what_a_tree_refuses_in_the_codecs_words() {
    let doc = drawing(1, 3);
    let p = pack(&doc);
    let document = parse_with(&p.document, &native_limits()).unwrap();
    let open = |bytes: Vec<u8>| {
        decode_from(&document, &mut Bytes::with_pages(&p, vec![bytes]), &p.blobs, &mut Budget::for_open(&open_limits()), &mut Vec::new())
    };
    let good = String::from_utf8(p.pages[0].clone()).unwrap();
    // A part cut short, a second root, a DTD.
    let malformed = |why: &'static str| CodecError::Malformed { part: "page".to_owned(), why };
    assert_eq!(open(good.as_bytes()[..good.len() - 20].to_vec()).unwrap_err(), malformed("XML that is not well-formed"));
    assert_eq!(open(format!("{good}<a/>").into_bytes()).unwrap_err(), malformed("XML that is not well-formed"));
    assert_eq!(
        open(format!("<!DOCTYPE a [<!ENTITY x \"y\">]>{good}").into_bytes()).unwrap_err(),
        malformed("XML that holds a DTD or an entity, which is refused")
    );
    // A root that is not a page.
    assert_eq!(
        open(good.replace("diacraft:page", "diacraft:master").into_bytes()).unwrap_err(),
        malformed("the root element is not the one this part holds")
    );
    // A page's id that is not the list's is damage, as it is of a tree.
    let swapped = good.replacen("id=\"1\"", "id=\"7\"", 1);
    assert_eq!(open(swapped.into_bytes()).unwrap_err(), malformed("a page part whose ID is not the one the list gives"));
    // A part past what a part may be, before it is read at all (its bytes are what is held).
    let huge = vec![b' '; usize::try_from(OPEN_PART_BYTES).unwrap() + 1];
    assert_eq!(open(huge).unwrap_err(), CodecError::Limit { what: "bytes in one part", max: OPEN_PART_BYTES });
}

#[test]
fn a_child_of_more_items_than_one_tree_may_have_is_refused_and_a_page_of_many_children_is_not() {
    let doc = drawing(1, 2);
    let p = pack(&doc);
    let document = parse_with(&p.document, &native_limits()).unwrap();
    let good = String::from_utf8(p.pages[0].clone()).unwrap();
    let open = |bytes: String| {
        decode_from(&document, &mut Bytes::with_pages(&p, vec![bytes.into_bytes()]), &p.blobs, &mut Budget::new(HUGE, HUGE, HUGE), &mut Vec::new())
    };
    // One shape with `OPEN_ELEMENT_ITEMS` children of two items each is more items than a child may have.
    let wide = format!(
        "<diacraft:shape id=\"9\">{}</diacraft:shape>",
        "<x:y xmlns:x=\"urn:q\"/>".repeat(usize::try_from(OPEN_ELEMENT_ITEMS / 2).unwrap() + 1)
    );
    let page = good.replacen("</diacraft:page>", &format!("{wide}</diacraft:page>"), 1);
    assert_eq!(open(page).unwrap_err(), CodecError::Limit { what: "XML items in one element of a part", max: OPEN_ELEMENT_ITEMS });
}

#[test]
fn a_child_of_more_than_a_tree_that_the_models_room_cannot_hold_beside_it_is_refused_in_the_words_of_the_pool_before_its_tree_is_made() {
    // B1: the child has 2.6 million items; a model with 64 MiB of room has room for the tree pool's two million and 419,430 more.
    let doc = drawing(1, 2);
    let p = pack(&doc);
    let document = parse_with(&p.document, &native_limits()).unwrap();
    let good = String::from_utf8(p.pages[0].clone()).unwrap();
    let wide = format!("<diacraft:shape id=\"9\">{}</diacraft:shape>", "<x:y xmlns:x=\"urn:q\"/>".repeat(1_300_000));
    let page = good.replacen("</diacraft:page>", &format!("{wide}</diacraft:page>"), 1);
    let mut budget = Budget::new(HUGE, 64 << 20, 0);
    let out = decode_from(&document, &mut Bytes::with_pages(&p, vec![page.into_bytes()]), &p.blobs, &mut budget, &mut Vec::new());
    assert_eq!(out.unwrap_err(), CodecError::Limit { what: "bytes of the model in memory", max: 64 << 20 });
}
