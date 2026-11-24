//! Item 1: **a page is written as a stream of its children** (`save_parts_bytes`) and its content stamp is taken the same way. The part is made a child at a
//! time and goes to the bytes of the part, held to the items of one tree for each child and to the bytes a part may have; the save's check reads those bytes as an open reads them.
use std::borrow::Cow;
use std::sync::Arc;

use diacraft_container::Limits;
use diacraft_container::xml::{El, parse_with, to_bytes};
use diacraft_doc::{Cell, CellAddr, Document, Master, Page, RawNode, Row, Section, Shape, ShapeList, Value};
use diacraft_sheetxml::model::{
    Budget, CodecError, MemBlobs, PartRole, PartSource, SavedBytes, content_stamp, decode_from, encode_for_save, open_limits, save_parts,
    save_parts_bytes,
};
use diacraft_testkit::model::{ModelSize, any_document};
use proptest::prelude::*;

use super::model_diff::first_difference;
use super::model_util::native_limits;

const HUGE: u64 = 1 << 40;

/// What a save handed over: how it ended, each part's role, name and bytes, and the blobs.
type Saved = (Result<(), CodecError>, Vec<(PartRole, String, Vec<u8>)>, MemBlobs);

fn saved_bytes(doc: &Document, limits: &Limits) -> Saved {
    let mut blobs = MemBlobs::new();
    let mut parts = Vec::new();
    let out = save_parts_bytes(doc, &mut blobs, limits, &mut |p: SavedBytes<'_>| {
        parts.push((p.role, p.name.to_owned(), p.bytes.to_vec()));
        Ok(())
    });
    (out, parts, blobs)
}

/// The parts the tree save hands over, written as bytes: the same parts, which the save of bytes must make.
fn saved_trees_as_bytes(doc: &Document, limits: &Limits) -> Vec<(PartRole, String, Vec<u8>)> {
    let mut blobs = MemBlobs::new();
    let mut parts = Vec::new();
    save_parts(doc, &mut blobs, limits, &mut |p| {
        parts.push((p.role, p.name.to_owned(), to_bytes(p.tree, &[], &[], true).unwrap()));
        Ok(())
    })
    .unwrap();
    parts
}

fn cell(v: &str) -> Cell {
    Cell::constant(Value::from_text(v))
}

/// A page of `shapes` shapes, each with a cell of its own.
fn page(id: u32, shapes: u32) -> Arc<Page> {
    let mut page = Page::new(id, &format!("page {id}"));
    page.shapes = ShapeList::from_vec(
        (0..shapes)
            .map(|i| {
                let mut s = Shape::new(i + 1);
                Arc::make_mut(&mut s.sheet).set_cell(&CellAddr::direct("PinX"), cell(&i.to_string())).unwrap();
                Arc::new(s)
            })
            .collect(),
    );
    Arc::new(page)
}

fn drawing(pages: u32, shapes: u32) -> Document {
    let mut doc = Document::empty();
    for p in 0..pages {
        doc.pages.push(page(p + 1, shapes));
    }
    doc.masters.push(Arc::new(Master { id: 10, shapes: ShapeList::from_vec(vec![Arc::new(Shape::new(1))]), ..Master::default() }));
    doc
}

/// A path of `rows` rows, made afresh: no two calls share an allocation.
fn path(rows: u32) -> Arc<Section> {
    let mut s = Section::new("Geometry", Some(0));
    for i in 0..rows {
        let mut row = Row { row_type: Some(Arc::from("LineTo")), ix: Some(i + 1), ..Row::default() };
        row.cells.insert("X".into(), cell(&i.to_string()));
        s.rows.push(Arc::new(row));
    }
    Arc::new(s)
}

/// A drawing whose shapes each have a place of their own and the same path, some as one allocation and some as equal ones, and a master that keeps unknown content.
fn with_paths(pages: u32, shapes: u32) -> Document {
    let shared = path(3);
    let mut doc = Document::empty();
    for p in 0..pages {
        let mut page = Page::new(p + 1, "p");
        page.shapes = ShapeList::from_vec(
            (0..shapes)
                .map(|i| {
                    let mut s = Shape::new(i + 1);
                    let sheet = Arc::make_mut(&mut s.sheet);
                    sheet.set_cell(&CellAddr::direct("PinX"), cell(&i.to_string())).unwrap();
                    sheet.sections.push(if i % 2 == 0 { Arc::clone(&shared) } else { path(3) });
                    Arc::new(s)
                })
                .collect(),
        );
        doc.pages.push(Arc::new(page));
    }
    let mut master = Master { id: 10, ..Master::default() };
    master.ext.make_mut().native.kept.push(1, RawNode::Text(Arc::from("a newer build wrote this")));
    doc.masters.push(Arc::new(master));
    doc
}

/// A source that holds each part as bytes.
struct Bytes {
    pages: Vec<Vec<u8>>,
    masters: Vec<Vec<u8>>,
}

impl PartSource for Bytes {
    type Tree = El;

    fn page(&mut self, _: usize, _: &str) -> Result<El, CodecError> {
        Err(CodecError::Malformed { part: "page".to_owned(), why: "the tree was asked for" })
    }

    fn master(&mut self, _: usize, _: &str) -> Result<El, CodecError> {
        Err(CodecError::Malformed { part: "master".to_owned(), why: "the tree was asked for" })
    }

    fn page_bytes(&mut self, index: usize, _: &str) -> Result<Option<Cow<'_, [u8]>>, CodecError> {
        Ok(self.pages.get(index).map(|b| Cow::Borrowed(b.as_slice())))
    }

    fn master_bytes(&mut self, index: usize, _: &str) -> Result<Option<Cow<'_, [u8]>>, CodecError> {
        Ok(self.masters.get(index).map(|b| Cow::Borrowed(b.as_slice())))
    }
}

/// Opens what a save of bytes handed over, reading each page and master as a stream.
fn opened(parts: &[(PartRole, String, Vec<u8>)], blobs: &MemBlobs) -> Result<Document, CodecError> {
    let document = parse_with(&parts[0].2, &native_limits()).unwrap();
    let pick = |want: fn(&PartRole) -> bool| parts.iter().filter(|(r, _, _)| want(r)).map(|(_, _, b)| b.clone()).collect::<Vec<_>>();
    let mut source = Bytes { pages: pick(|r| matches!(r, PartRole::Page { .. })), masters: pick(|r| matches!(r, PartRole::Master { .. })) };
    decode_from(&document, &mut source, blobs, &mut Budget::for_open(&open_limits()), &mut Vec::new())
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 48, ..ProptestConfig::default() })]

    /// The parts a save of bytes hands over are the bytes of the parts the save of trees hands over: the same roles, the same names, the same bytes, in the same order, over
    /// every generated drawing; and what is handed over is read back as the drawing, the pages and masters as streams.
    #[test]
    fn the_bytes_of_a_save_are_what_the_writer_makes_of_the_trees_of_the_save_and_open_as_the_drawing(m in any_document(ModelSize::Small)) {
        let (out, parts, blobs) = saved_bytes(&m, &open_limits());
        prop_assert!(out.is_ok(), "{:?}", out.err());
        prop_assert_eq!(&parts, &saved_trees_as_bytes(&m, &open_limits()));
        let back = opened(&parts, &blobs).unwrap();
        prop_assert!(back == m, "{}", first_difference(&back, &m).unwrap_or_default());
    }
}

#[test]
fn a_save_of_bytes_hands_over_the_document_then_each_page_then_each_master_and_nothing_before_it_is_checked_whole() {
    let doc = drawing(3, 4);
    let (out, parts, _) = saved_bytes(&doc, &open_limits());
    out.unwrap();
    let roles: Vec<(PartRole, &str)> = parts.iter().map(|(r, n, _)| (*r, n.as_str())).collect();
    assert_eq!(
        roles,
        [
            (PartRole::Document, ""),
            (PartRole::Page { id: 1 }, "pages/page1.xml"),
            (PartRole::Page { id: 2 }, "pages/page2.xml"),
            (PartRole::Page { id: 3 }, "pages/page3.xml"),
            (PartRole::Master { id: 10 }, "masters/master10.xml"),
        ]
    );
}

#[test]
fn a_page_of_more_items_than_the_limits_tree_is_saved_as_a_stream_and_refused_as_a_tree() {
    // 400 shapes of about ten items: four thousand items in one page, and a reader whose trees are of a thousand.
    let doc = drawing(1, 400);
    let mut limits = open_limits();
    limits.xml.max_nodes = 1_000;
    let tree = save_parts(&doc, &mut MemBlobs::new(), &limits, &mut |_| Ok(()));
    assert!(matches!(&tree, Err(CodecError::Limit { what: "XML items in one part", .. })), "{tree:?}");
    assert!(matches!(encode_for_save(&doc, &mut MemBlobs::new(), &limits), Err(CodecError::Limit { .. })));
    let (out, parts, blobs) = saved_bytes(&doc, &limits);
    out.unwrap();
    assert_eq!(opened(&parts, &blobs).unwrap(), doc);
}

#[test]
fn a_page_of_more_items_than_the_format_allows_a_part_is_refused_in_words_though_each_child_is_small_and_before_any_part_is_handed_over() {
    // The spec's 6.1 allows a part 12,000,000 items, and a writer does not pass it, however the part is made. Thirteen thousand shapes that hold the same thousand
    // text nodes (one allocation in the model) are thirteen million items of a page in children of a thousand: a page the open could read as a stream, and not a part of the format.
    let mut base = Shape::new(0);
    for i in 0..1_000 {
        base.ext.make_mut().native.kept.push(0, RawNode::Text(Arc::from(format!("t{i}"))));
    }
    let ext = base.ext.clone();
    let mut page = Page::new(1, "page 1");
    page.shapes = ShapeList::from_vec(
        (1..=13_000)
            .map(|i| {
                let mut s = Shape::new(i);
                s.ext = ext.clone();
                Arc::new(s)
            })
            .collect(),
    );
    let mut doc = Document::empty();
    doc.pages.push(Arc::new(page));
    let (out, parts, _) = saved_bytes(&doc, &open_limits());
    assert_eq!(out, Err(CodecError::Limit { what: "XML items in one part", max: 12_000_000 }));
    assert!(parts.is_empty(), "refused before the first part is handed over");
}

#[test]
fn a_child_of_more_items_than_one_tree_may_have_is_refused_in_words_before_any_part_is_handed_over() {
    let mut doc = drawing(2, 5);
    // A shape that keeps three hundred elements of an unknown kind: past a tree of a hundred items.
    let mut shape = Shape::new(99);
    for i in 0..300 {
        shape.ext.make_mut().native.kept.push(0, RawNode::Text(Arc::from(format!("t{i}"))));
    }
    let mut p = (**doc.pages.get(1).unwrap()).clone();
    p.shapes = ShapeList::from_vec(p.shapes.iter().cloned().chain(std::iter::once(Arc::new(shape))).collect());
    doc.pages = doc.pages.iter().take(1).cloned().chain(std::iter::once(Arc::new(p))).collect();
    let mut limits = open_limits();
    limits.xml.max_nodes = 100;
    let (out, parts, _) = saved_bytes(&doc, &limits);
    assert_eq!(out, Err(CodecError::Limit { what: "XML items in one element of a part", max: 100 }));
    assert!(parts.is_empty(), "refused before the first part is handed over");
    // At exactly what a child may have it is saved: the shape has 300 text nodes, its element and its id.
    limits.xml.max_nodes = 1_000;
    assert!(saved_bytes(&doc, &limits).0.is_ok());
}

#[test]
fn a_part_past_the_bytes_a_part_may_be_is_refused_in_words_before_any_part_is_handed_over_and_at_exactly_them_it_is_saved() {
    let doc = drawing(2, 30);
    let (out, parts, _) = saved_bytes(&doc, &open_limits());
    out.unwrap();
    let largest = parts.iter().map(|(_, _, b)| b.len() as u64).max().unwrap();
    let mut limits = open_limits();
    limits.max_part_bytes = largest;
    assert!(saved_bytes(&doc, &limits).0.is_ok(), "a part of exactly the most bytes");
    limits.max_part_bytes = largest - 1;
    let (out, parts, _) = saved_bytes(&doc, &limits);
    assert_eq!(out, Err(CodecError::Limit { what: "bytes in one part as written", max: largest - 1 }));
    assert!(parts.is_empty());
}

#[test]
fn what_an_open_refuses_a_save_of_bytes_refuses_and_a_package_that_is_saved_is_one_an_open_takes() {
    // A drawing whose pool of the model is past what an open has is refused by the check of the second pass, as the save of trees refuses it: the pools are the open's.
    let doc = drawing(1, 10);
    let mut limits = open_limits();
    limits.max_package_bytes = 1;
    let out = saved_bytes(&doc, &limits).0;
    assert!(matches!(out, Err(CodecError::Limit { .. })), "{out:?}");
}

#[test]
fn the_content_stamp_is_a_stream_of_the_parts_children_and_is_the_stamp_it_was() {
    // Stamps of drawings made at 7aae6f67, before the stamp took a part a child at a time: a drawing has the stamp it had. (`paths` says units once by address and by value,
    // and keeps unknown content in a master.)
    let stamp = |d: &Document| content_stamp(d, &mut Budget::new(HUGE, HUGE, HUGE)).unwrap().text();
    assert_eq!(stamp(&Document::empty()), "2e697c8536d15af8");
    assert_eq!(stamp(&drawing(3, 4)), "f4f4566fb4d2bfee");
    assert_eq!(stamp(&drawing(1, 1)), "f95450ae57649a99");
    assert_eq!(stamp(&with_paths(2, 5)), "2bbc2c5053d9206f");
}

#[test]
fn a_stamp_of_a_page_the_tree_of_a_part_could_not_hold_is_taken_and_charges_what_it_walks() {
    // A reader whose trees are small cannot hold the page as a tree; the stamp walks it a child at a time. The items it charges are the items of the part in full.
    let doc = with_paths(1, 200);
    let mut budget = Budget::new(HUGE, HUGE, HUGE);
    let (_, cost) = diacraft_sheetxml::model::content_stamp_counted(&doc, &mut budget).unwrap();
    assert_eq!(HUGE - budget.items, cost.items, "the budget paid for every item walked");
    assert!(cost.largest_part_items > 200 * 10, "{} items", cost.largest_part_items);
}

// ------------------------------------------------------------------------------------------------------------------------------- one child of more than a tree of an open
// (B1): a part read as a stream may have a child of up to 4,000,000 items (what a part was held to before pages were streams), and a child beyond what the pool
// of one tree holds is held beside the model by what it has beyond it.

/// One group of `n` shapes that have `cells` cells of their own each: one child of the page root of many items.
fn grouped(n: u32, cells: u32) -> Document {
    let kids: Vec<Arc<Shape>> = (0..n)
        .map(|i| {
            let mut s = Shape::new(i + 2);
            let sheet = Arc::make_mut(&mut s.sheet);
            for c in 0..cells {
                sheet.set_cell(&CellAddr::direct(&format!("User.c{c}")), cell(&format!("{i}.{c}"))).unwrap();
            }
            Arc::new(s)
        })
        .collect();
    let mut group = Shape::new(1);
    group.children = ShapeList::from_vec(kids);
    let mut page = Page::new(1, "P");
    page.shapes = ShapeList::from_vec(vec![Arc::new(group)]);
    let mut doc = Document::empty();
    doc.pages.push(Arc::new(page));
    doc
}

/// One shape with a path of `rows` rows of two cells each: one child of the page root of many items.
fn long_path(rows: u32) -> Document {
    let mut s = Shape::new(1);
    let mut sec = Section::new("Geometry", Some(0));
    for i in 0..rows {
        let mut row = Row { row_type: Some(Arc::from("LineTo")), ix: Some(i + 1), ..Row::default() };
        row.cells.insert("X".into(), cell(&i.to_string()));
        row.cells.insert("Y".into(), cell(&(i * 2).to_string()));
        sec.rows.push(Arc::new(row));
    }
    Arc::make_mut(&mut s.sheet).sections.push(Arc::new(sec));
    let mut page = Page::new(1, "P");
    page.shapes = ShapeList::from_vec(vec![Arc::new(s)]);
    let mut doc = Document::empty();
    doc.pages.push(Arc::new(page));
    doc
}

/// The items of the largest child of the first page's root, as written.
fn largest_child_items(parts: &[(PartRole, String, Vec<u8>)]) -> u64 {
    let (_, _, bytes) = parts.iter().find(|(r, _, _)| matches!(r, PartRole::Page { .. })).expect("a page part");
    let tree = parse_with(
        bytes,
        &diacraft_container::xml::XmlLimits { max_depth: 512, max_nodes: usize::MAX, ..diacraft_container::xml::XmlLimits::default() },
    )
    .unwrap();
    fn items(e: &El) -> u64 {
        1 + e.attrs.len() as u64
            + e.children
                .iter()
                .map(|c| match c {
                    diacraft_container::xml::Node::El(c) => items(c),
                    _ => 1,
                })
                .sum::<u64>()
    }
    tree.children.iter().map(|c| if let diacraft_container::xml::Node::El(c) = c { items(c) } else { 0 }).max().unwrap_or(0)
}

#[test]
fn a_page_whose_one_shape_is_a_group_of_seventy_thousand_is_saved_as_a_stream_reopened_equal_and_stamped() {
    // 70,000 members of 10 cells each are about 2.4 million items in the one child of the page's root: more than a tree of an open, and a page that a save of the trees held to 4
    // million items made before pages were streams.
    let doc = grouped(70_000, 10);
    let (out, parts, blobs) = saved_bytes(&doc, &open_limits());
    assert!(out.is_ok(), "the save: {out:?}");
    assert!(
        largest_child_items(&parts) > diacraft_sheetxml::model::OPEN_TREE_ITEMS,
        "a child of more items than a tree of an open: {}",
        largest_child_items(&parts)
    );
    let back = opened(&parts, &blobs).expect("the open of what was saved");
    assert!(back == doc, "{}", first_difference(&back, &doc).unwrap_or_default());
    let stamp = content_stamp(&doc, &mut Budget::new(HUGE, HUGE, HUGE));
    assert!(stamp.is_ok(), "the stamp: {:?}", stamp.err());
}

#[test]
fn a_page_whose_one_shape_has_a_path_of_two_hundred_and_fifty_thousand_rows_is_saved_as_a_stream_reopened_equal_and_stamped() {
    let doc = long_path(250_000);
    let (out, parts, blobs) = saved_bytes(&doc, &open_limits());
    assert!(out.is_ok(), "the save: {out:?}");
    assert!(
        largest_child_items(&parts) > diacraft_sheetxml::model::OPEN_TREE_ITEMS,
        "a child of more items than a tree of an open: {}",
        largest_child_items(&parts)
    );
    let back = opened(&parts, &blobs).expect("the open of what was saved");
    assert!(back == doc, "{}", first_difference(&back, &doc).unwrap_or_default());
    let stamp = content_stamp(&doc, &mut Budget::new(HUGE, HUGE, HUGE));
    assert!(stamp.is_ok(), "the stamp: {:?}", stamp.err());
}

#[test]
fn a_page_of_many_shapes_that_keeps_unknown_content_of_a_newer_build_is_saved_as_a_stream_whatever_its_items_in_all() {
    // Each unknown node the page keeps is a child of the part like a shape, counted against the cap of a child and added to the part's; it was counted
    // against the cap of the whole part as a tree, so that a page of 70,000 shapes (2.4 million items, shapes of 35 items) refused to be saved the moment it kept one.
    let group = grouped(70_000, 10);
    let kids = group.pages.iter().next().unwrap().shapes.iter().next().unwrap().children.clone();
    for at in [0, 1, 70_000] {
        let mut page = Page::clone(group.pages.iter().next().unwrap());
        page.shapes = kids.clone();
        let kept = diacraft_doc::RawElement {
            name: Arc::from("x:newer"),
            attrs: diacraft_doc::RawAttrs::new(),
            children: vec![RawNode::Text(Arc::from("a newer build wrote this"))],
        };
        page.ext.make_mut().native.kept.push(at, RawNode::Element(kept));
        let mut doc = Document::empty();
        doc.pages.push(Arc::new(page));
        let (out, parts, blobs) = saved_bytes(&doc, &open_limits());
        assert!(out.is_ok(), "kept at {at}: {out:?}");
        let back = opened(&parts, &blobs).expect("the open of what was saved");
        assert!(back == doc, "kept at {at}: {}", first_difference(&back, &doc).unwrap_or_default());
    }
}
