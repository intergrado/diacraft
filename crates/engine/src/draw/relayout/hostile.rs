//! Hostile pages for Re-Layout Page: the counts are checked before anything is made of a page, and the work of a layout is counted (the walks of the page, the searches from its top),
//! not timed.

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Connect, Connects, Document, Shape, ShapeList, Value};
use diacraft_layout::{Direction, Style};

use super::{Request, relayout};
use crate::draw::page_mut;
use crate::draw::read::FINDS;
use crate::draw::scan::NODES_WALKED;

fn cells(shape: &mut Shape, list: &[(&str, &str)]) {
    let mut sheet = diacraft_doc::Sheet::default();
    for (name, value) in list {
        sheet.cells.insert(CellName::new(name), Cell::constant(Value::from_text(value)).with_unit("IN"));
    }
    shape.sheet = Arc::new(sheet);
}

/// A box 1 by 1 with its pin at its middle, at (`x`, `y`).
fn plain(id: u32, x: f64, y: f64) -> Shape {
    let mut shape = Shape::new(id);
    cells(&mut shape, &[("PinX", &x.to_string()), ("PinY", &y.to_string()), ("Width", "1"), ("Height", "1"), ("LocPinX", "0.5"), ("LocPinY", "0.5")]);
    shape
}

/// A line from (0, 0) to (2, 0): one-dimensional, so a connector when a `Connect` row glues an end of it.
fn line(id: u32) -> Shape {
    let mut line = Shape::new(id);
    cells(&mut line, &[("BeginX", "0"), ("BeginY", "0"), ("EndX", "2"), ("EndY", "0"), ("PinX", "1"), ("PinY", "0")]);
    line
}

fn glue(from: u32, cell: &str, to: u32) -> Connect {
    Connect { from_sheet: from, from_cell: Some(Arc::from(cell)), to_sheet: to, ..Connect::default() }
}

fn doc_of(shapes: Vec<Shape>, rows: Vec<Connect>) -> Document {
    let mut doc = Document::new_blank();
    let page = page_mut(&mut doc, 1).unwrap();
    page.shapes = ShapeList::from_vec(shapes.into_iter().map(Arc::new).collect());
    page.connects = Connects::from_vec(rows);
    doc
}

fn request() -> Request {
    Request {
        style: Style::Flowchart(Direction::TopToBottom),
        avenue: (0.5, 0.5),
        cells: Vec::new(),
        route: None,
        apply_routing: false,
        resize_page: None,
        org: false,
    }
}

/// `n` shapes in a chain, joined by `n - 1` connectors, all on one page: shapes 1 to n, connectors n + 1 on.
fn chain_page(n: u32) -> Document {
    let mut shapes: Vec<Shape> = (1..=n).map(|id| plain(id, f64::from(id), 5.0)).collect();
    let mut rows = Vec::new();
    for k in 1..n {
        let id = n + k;
        shapes.push(line(id));
        rows.push(glue(id, "BeginX", k));
        rows.push(glue(id, "EndX", k + 1));
    }
    doc_of(shapes, rows)
}

#[test]
fn a_page_with_more_shapes_than_a_layout_places_is_refused_in_words_before_anything_is_made_of_it() {
    // 20,001 shapes. The refusal names the limit and says what to do; the document is as it was, and no shape was looked for by its `ID`.
    let mut doc = doc_of((1..=20_001).map(|id| plain(id, f64::from(id % 100), f64::from(id / 100))).collect(), Vec::new());
    let before = doc.clone();
    FINDS.with(|c| c.set(0));
    let err = relayout(&mut doc, 1, None, &request()).unwrap_err();
    assert!(err.message.contains("more than 20000 shapes") && err.message.contains("select part of the diagram"), "{}", err.message);
    assert_eq!(doc, before);
    assert_eq!(FINDS.with(std::cell::Cell::get), 0, "no search from the top of the page for a shape");
}

#[test]
fn a_page_with_more_shapes_at_its_top_than_are_read_is_refused_before_any_is_read() {
    let mut doc = doc_of((1..=60_001).map(|id| plain(id, 1.0, 1.0)).collect(), Vec::new());
    let before = doc.clone();
    let err = relayout(&mut doc, 1, None, &request()).unwrap_err();
    assert!(err.message.contains("more than the 60000"), "{}", err.message);
    assert_eq!(doc, before);
}

#[test]
fn a_page_with_more_glued_ends_than_are_read_is_refused() {
    let rows: Vec<Connect> = (0..100_001).map(|i| glue(10_000 + i, "BeginX", 1)).collect();
    let mut doc = doc_of(vec![plain(1, 1.0, 1.0)], rows);
    let err = relayout(&mut doc, 1, None, &request()).unwrap_err();
    assert!(err.message.contains("100000 a layout reads"), "{}", err.message);
}

#[test]
fn a_page_with_more_connectors_than_one_step_routes_is_refused() {
    // 10,001 connectors between two shapes: the routing of one edit takes 10,000. Refused in words before the shapes are laid out.
    let mut shapes = vec![plain(1, 1.0, 1.0), plain(2, 5.0, 1.0)];
    let mut rows = Vec::new();
    for i in 0..10_001 {
        shapes.push(line(100 + i));
        rows.push(glue(100 + i, "BeginX", 1));
        rows.push(glue(100 + i, "EndX", 2));
    }
    let mut doc = doc_of(shapes, rows);
    let before = doc.clone();
    let err = relayout(&mut doc, 1, None, &request()).unwrap_err();
    assert!(err.message.contains("more than 10000 connectors"), "{}", err.message);
    assert_eq!(doc, before);
}

#[test]
fn the_work_of_a_layout_of_a_page_is_a_few_walks_of_it_and_no_search_for_a_shape() {
    // Counted, not timed: a layout of 2,000 shapes joined in a chain by 1,999 connectors walks the page a handful of times and looks no shape up from the top of the page.
    // Catches a layout that finds each shape by its `ID` (a walk of the page for each) and a quadratic read of the glue.
    let n = 2_000;
    let mut doc = chain_page(n);
    let total = doc.page(1).unwrap().shapes.len();
    NODES_WALKED.with(|c| c.set(0));
    FINDS.with(|c| c.set(0));
    let done = relayout(&mut doc, 1, None, &request()).unwrap();
    assert_eq!((done.nodes, done.joins), (n as usize, n as usize - 1));
    assert!(done.moved > 0);
    let walked = NODES_WALKED.with(std::cell::Cell::get);
    assert!(walked <= 12 * total, "{walked} nodes walked for a page of {total} shapes");
    assert!(FINDS.with(std::cell::Cell::get) <= 8, "searches from the top of the page: {}", FINDS.with(std::cell::Cell::get));
}
