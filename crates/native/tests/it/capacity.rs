//! The package reads and writes a page part **as a stream of the children of its root**, so that a page of more items than a tree of an open (2,000,000)
//! is a package this crate writes and reads.

use std::sync::Arc;

use diacraft_doc::{Cell, CellAddr, Document, Page, Shape, ShapeList, Value};
use diacraft_native::{State, WriteOptions, read, write};

use crate::util::*;

/// A drawing of one page of `shapes` shapes that each have `cells` cells of their own: a page of about `shapes * (cells * 4 + 3)` items.
fn big_page(shapes: u32, cells: u32) -> Document {
    let mut page = Page::new(1, "Big");
    page.shapes = ShapeList::from_vec(
        (0..shapes)
            .map(|i| {
                let mut s = Shape::new(i + 1);
                let sheet = Arc::make_mut(&mut s.sheet);
                for c in 0..cells {
                    sheet.set_cell(&CellAddr::direct(&format!("User.c{c}")), Cell::constant(Value::from_text(&format!("{i}.{c}")))).unwrap();
                }
                Arc::new(s)
            })
            .collect(),
    );
    drawing_of(vec![page])
}

#[test]
fn a_page_of_more_items_than_a_tree_of_an_open_is_written_and_read_back_equal() {
    // 60,000 shapes of 12 cells: more than 2,000,000 items in the one page part (a tree of an open is 2,000,000), in children of a few dozen items each.
    let doc = big_page(60_000, 12);
    let written = write(&doc, &State::default(), None, &WriteOptions::default()).unwrap_or_else(|e| panic!("the drawing is written: {e}"));
    let page = entry(&written.bytes, "pages/page1.xml");
    assert!(page.len() > 4_000_000, "a page part of {} bytes", page.len());
    let (back, _) = read(&written.bytes, &limits()).unwrap_or_else(|e| panic!("the package reads: {e}"));
    assert!(back == doc, "{}", first_difference(&back, &doc).unwrap_or_default());
}
