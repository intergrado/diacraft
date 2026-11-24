//! Item 2: **the encoder shares equal units by value.** A model that holds each equal sheet and section as an allocation of its own (the `.vsdx` reader's does:
//! it shares nothing, and a paste or an edit that happens to make two units alike does the same) is written as the file of 3.19 says it: each unit once, with a name (`def`), and
//! a `same` for each repeat. **The key only finds candidates**: a unit is the same as another when `==` says so, and never because two hashes agree.
use std::sync::Arc;

use diacraft_container::xml::El;
use diacraft_doc::{Cell, CellAddr, Document, Page, Row, Section, Shape, ShapeList, Sheet, Value};
use diacraft_sheetxml::model::{Budget, MemBlobs, content_stamp, decode_page, encode_page};

use super::model_util::{pack, round_trip};

const HUGE: u64 = 1 << 40;

fn cell(v: &str) -> Cell {
    Cell::constant(Value::from_text(v))
}

/// A section of `rows` path rows, made afresh: no two calls share an allocation.
fn path(rows: u32) -> Arc<Section> {
    let mut s = Section::new("Geometry", Some(0));
    for i in 0..rows {
        let mut row = Row { row_type: Some(Arc::from("LineTo")), ix: Some(i + 1), ..Row::default() };
        row.cells.insert("X".into(), cell(&i.to_string()));
        row.cells.insert("Y".into(), cell("0"));
        s.rows.push(Arc::new(row));
    }
    Arc::new(s)
}

/// A shape whose sheet is `PinX = pin` and the section `section`, in an allocation of its own.
fn shape(id: u32, pin: &str, section: Arc<Section>) -> Arc<Shape> {
    let mut s = Shape::new(id);
    let sheet = Arc::make_mut(&mut s.sheet);
    sheet.set_cell(&CellAddr::direct("PinX"), cell(pin)).unwrap();
    sheet.sections.push(section);
    Arc::new(s)
}

fn page_of(shapes: Vec<Arc<Shape>>) -> Page {
    let mut page = Page::new(1, "p");
    page.shapes = ShapeList::from_vec(shapes);
    page
}

fn count(e: &El, local: &str) -> usize {
    usize::from(e.is(local)) + e.elements().map(|c| count(c, local)).sum::<usize>()
}

fn doc_of(page: Page) -> Document {
    let mut doc = Document::empty();
    doc.pages.push(Arc::new(page));
    doc
}

#[test]
fn equal_sheets_that_are_distinct_allocations_are_written_once_and_said_again() {
    // Ten shapes, each with a sheet of its own that is equal to the others', and a path of its own that is equal to the others'.
    let page = page_of((0..10).map(|i| shape(i + 1, "5", path(4))).collect());
    assert!(
        !Arc::ptr_eq(&page.shapes.get(0).unwrap().sheet, &page.shapes.get(1).unwrap().sheet),
        "the model holds the sheets apart, as the .vsdx reader's does"
    );
    let tree = encode_page(&page, &mut MemBlobs::new()).unwrap();
    assert_eq!(count(&tree, "sheet"), 1, "one sheet is written in full");
    assert_eq!(count(&tree, "same"), 9, "and the nine others say it");
    assert_eq!(count(&tree, "Section"), 1, "its path is written in full once, inside it");
    // What is read back is the model, and the equal sheets are one allocation.
    let back = decode_page(&tree, &MemBlobs::new(), &mut Budget::new(HUGE, HUGE, HUGE), &mut Vec::new()).unwrap();
    assert_eq!(back, page);
    assert!(Arc::ptr_eq(&back.shapes.get(0).unwrap().sheet, &back.shapes.get(9).unwrap().sheet));
}

#[test]
fn sheets_that_differ_are_not_said_again_and_the_sections_they_have_alike_are() {
    // Every sheet has a pin of its own and the same path (distinct allocations): the sheets are written in full, and the path once.
    let page = page_of((0..10).map(|i| shape(i + 1, &i.to_string(), path(4))).collect());
    let tree = encode_page(&page, &mut MemBlobs::new()).unwrap();
    assert_eq!(count(&tree, "sheet"), 10, "ten different sheets");
    assert_eq!(count(&tree, "same"), 9, "and the path of nine is the path of the first");
    assert_eq!(count(&tree, "Section"), 1);
    let back = decode_page(&tree, &MemBlobs::new(), &mut Budget::new(HUGE, HUGE, HUGE), &mut Vec::new()).unwrap();
    assert_eq!(back, page);
}

#[test]
fn units_that_differ_in_one_cell_or_one_row_are_written_in_full() {
    // Nine paths alike and one that differs in the last cell of its last row, and nine sheets alike and one that differs in a link.
    let mut odd = Section::new("Geometry", Some(0));
    for i in 0..4u32 {
        let mut row = Row { row_type: Some(Arc::from("LineTo")), ix: Some(i + 1), ..Row::default() };
        row.cells.insert("X".into(), cell(&i.to_string()));
        row.cells.insert("Y".into(), cell(if i == 3 { "1" } else { "0" }));
        odd.rows.push(Arc::new(row));
    }
    let mut shapes: Vec<Arc<Shape>> = (0..9).map(|i| shape(i + 1, "5", path(4))).collect();
    shapes.push(shape(10, "5", Arc::new(odd)));
    let mut linked = (*shapes[0]).clone();
    Arc::make_mut(&mut linked.sheet).links.line = Some(3);
    linked.id = 11;
    shapes.push(Arc::new(linked));
    let page = page_of(shapes);
    let tree = encode_page(&page, &mut MemBlobs::new()).unwrap();
    assert_eq!(count(&tree, "Section"), 2, "the path of nine and the path of one");
    let back = decode_page(&tree, &MemBlobs::new(), &mut Budget::new(HUGE, HUGE, HUGE), &mut Vec::new()).unwrap();
    assert_eq!(back, page, "no unit is given another's content");
    assert_eq!(back.shapes.get(9).unwrap().sheet.sections[0].rows[3].cells.get("Y").map(|c| c.value.as_ref().map(Value::text)), Some(Some("1")));
    assert_eq!(back.shapes.get(10).unwrap().sheet.links.line, Some(3));
}

#[test]
fn what_is_written_is_what_the_model_that_shares_by_address_has_always_written() {
    // One allocation held in ten places, and ten allocations of equal units: the same file.
    let shared = shape(1, "5", path(4));
    let by_address = page_of((0..10).map(|i| Arc::new(Shape { id: i + 1, ..(*shared).clone() })).collect());
    let by_value = page_of((0..10).map(|i| shape(i + 1, "5", path(4))).collect());
    let a = encode_page(&by_address, &mut MemBlobs::new()).unwrap();
    let b = encode_page(&by_value, &mut MemBlobs::new()).unwrap();
    assert_eq!(a, b);
}

#[test]
fn the_content_stamp_and_the_decode_of_a_save_are_the_models_whether_it_shares_or_not() {
    let by_value = doc_of(page_of((0..20).map(|i| shape(i + 1, "5", path(3))).collect()));
    let shared = shape(1, "5", path(3));
    let by_address = doc_of(page_of((0..20).map(|i| Arc::new(Shape { id: i + 1, ..(*shared).clone() })).collect()));
    let stamp = |d: &Document| content_stamp(d, &mut Budget::new(HUGE, HUGE, HUGE)).unwrap();
    assert_eq!(stamp(&by_value), stamp(&by_address));
    let back = round_trip(&by_value);
    assert_eq!(back, by_value);
    assert_eq!(stamp(&back), stamp(&by_value));
    // The file is the same file.
    assert_eq!(pack(&by_value).pages, pack(&by_address).pages);
}

#[test]
fn a_part_that_says_each_unit_once_is_smaller_than_one_that_says_them_all() {
    let page = page_of((0..1_000).map(|i| shape(i + 1, "5", path(6))).collect());
    let tree = encode_page(&page, &mut MemBlobs::new()).unwrap();
    let items = count_items(&tree);
    // A shape is its element, its id, a `same` and its `of`: four items, and the one sheet with its path.
    assert!(items < 4 * 1_000 + 200, "{items} items");
}

fn count_items(e: &El) -> u64 {
    1 + e.attrs.len() as u64 + e.children.iter().map(|n| if let diacraft_container::xml::Node::El(c) = n { count_items(c) } else { 1 }).sum::<u64>()
}

#[test]
fn a_sheet_that_holds_other_sheets_sections_in_a_different_order_is_a_different_sheet() {
    let mut a = Sheet::default();
    a.sections.push(path(2));
    a.sections.push(path(3));
    let mut b = Sheet::default();
    b.sections.push(path(3));
    b.sections.push(path(2));
    let mk = |sheet: Sheet, id: u32| {
        let mut s = Shape::new(id);
        s.sheet = Arc::new(sheet);
        Arc::new(s)
    };
    let page = page_of(vec![mk(a.clone(), 1), mk(b.clone(), 2), mk(a, 3), mk(b, 4)]);
    let tree = encode_page(&page, &mut MemBlobs::new()).unwrap();
    assert_eq!(count(&tree, "sheet"), 2);
    let back = decode_page(&tree, &MemBlobs::new(), &mut Budget::new(HUGE, HUGE, HUGE), &mut Vec::new()).unwrap();
    assert_eq!(back, page);
}
