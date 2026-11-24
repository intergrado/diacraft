//! Hostile org charts: a chart of a hundred thousand positions, a manager with thousands of reports, a deep chain and a reporting ring. The counts are checked
//! before anything is made of a page, a refusal is in words, and the work of a layout is counted (the walks of the page, the searches from its top), not timed.

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Connect, Connects, Document, Row, Section, Shape, ShapeList, Value};

use super::org::{Arrangement, ChartRequest, arrange, chart, lay_out, link, positions};
use crate::draw::page_mut;
use crate::draw::read::FINDS;
use crate::draw::scan::NODES_WALKED;
use crate::stencil::orgchart::MARKER_ROW;

fn cells(shape: &mut Shape, list: &[(&str, &str)]) {
    let mut sheet = diacraft_doc::Sheet::default();
    for (name, value) in list {
        sheet.cells.insert(CellName::new(name), Cell::constant(Value::from_text(value)).with_unit("IN"));
    }
    shape.sheet = Arc::new(sheet);
}

/// A position 1 by 1 with its pin at its middle, at (`x`, `y`): the marker is its own row, as a shape dropped from the stencil has it.
fn position(id: u32, x: f64, y: f64) -> Shape {
    let mut shape = Shape::new(id);
    cells(&mut shape, &[("PinX", &x.to_string()), ("PinY", &y.to_string()), ("Width", "1"), ("Height", "1"), ("LocPinX", "0.5"), ("LocPinY", "0.5")]);
    let mut row = Row { name: Some(Arc::from(MARKER_ROW)), ..Row::default() };
    row.cells.insert(CellName::new("Value"), Cell::constant(Value::from_text("Position")));
    let mut user = Section::new("User", None);
    user.rows.push(Arc::new(row));
    Arc::make_mut(&mut shape.sheet).sections.push(Arc::new(user));
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

/// A boss (shape 1) with `reports` positions under it, each joined by a connector: positions 1 to reports + 1, connectors after them.
fn boss_page(reports: u32) -> Document {
    let n = reports + 1;
    let mut shapes: Vec<Shape> = (1..=n).map(|id| position(id, f64::from(id % 500) * 2.0, f64::from(id / 500))).collect();
    let mut rows = Vec::new();
    for k in 0..reports {
        let id = n + 1 + k;
        shapes.push(line(id));
        rows.push(glue(id, "BeginX", 1));
        rows.push(glue(id, "EndX", k + 2));
    }
    doc_of(shapes, rows)
}

/// `n` positions in a chain (each manages the next), closed into a ring when `ring`.
fn chain_page(n: u32, ring: bool) -> Document {
    let mut shapes: Vec<Shape> = (1..=n).map(|id| position(id, f64::from(id) * 2.0, 5.0)).collect();
    let mut rows = Vec::new();
    let links = if ring { n } else { n - 1 };
    for k in 0..links {
        let id = n + 1 + k;
        shapes.push(line(id));
        rows.push(glue(id, "BeginX", k + 1));
        rows.push(glue(id, "EndX", (k + 1) % n + 1));
    }
    doc_of(shapes, rows)
}

#[test]
fn a_page_of_a_hundred_thousand_positions_is_refused_before_any_is_read() {
    // 100,000 shapes at the top of the page. The refusal names the limit; the document is as it was and no shape was looked for by its `ID`.
    let mut doc = doc_of((1..=100_000).map(|id| position(id, f64::from(id % 300), f64::from(id / 300))).collect(), Vec::new());
    let before = doc.clone();
    FINDS.with(|c| c.set(0));
    let err = lay_out(&mut doc, 1, &ChartRequest::default()).unwrap_err();
    assert!(err.message.contains("more than the 60000") && err.message.contains("split the drawing"), "{}", err.message);
    assert!(positions(&doc, 1).is_err() && chart(&doc, 1).is_err());
    assert_eq!(doc, before);
    assert_eq!(FINDS.with(std::cell::Cell::get), 0, "no search from the top of the page for a shape");
}

#[test]
fn thirty_thousand_positions_are_more_than_a_layout_places_and_are_refused_in_words() {
    let mut doc = doc_of((1..=30_000).map(|id| position(id, f64::from(id % 300), f64::from(id / 300))).collect(), Vec::new());
    let before = doc.clone();
    let err = lay_out(&mut doc, 1, &ChartRequest::default()).unwrap_err();
    assert!(err.message.contains("30000 positions") && err.message.contains("more than the 20000"), "{}", err.message);
    assert_eq!(doc, before);
}

#[test]
fn a_manager_with_three_thousand_reports_is_laid_out_in_a_few_walks_of_the_page() {
    // Counted, not timed: the page is walked a handful of times and no shape is looked up from its top. Catches a search for each report's place, and a quadratic read of the glue.
    let mut doc = boss_page(3_000);
    let total = doc.page(1).unwrap().shapes.len();
    NODES_WALKED.with(|c| c.set(0));
    FINDS.with(|c| c.set(0));
    let laid = lay_out(&mut doc, 1, &ChartRequest::default()).unwrap();
    assert_eq!((laid.positions, laid.done.joins), (3_001, 3_000));
    assert!(laid.done.moved > 0);
    let walked = NODES_WALKED.with(std::cell::Cell::get);
    assert!(walked <= 16 * total, "{walked} nodes walked for a page of {total} shapes");
    assert!(FINDS.with(std::cell::Cell::get) <= 8, "searches from the top of the page: {}", FINDS.with(std::cell::Cell::get));
    // The same boss hanging its reports in a column costs the same.
    arrange(&mut doc, 1, 1, Arrangement::VerticalRight).unwrap();
    NODES_WALKED.with(|c| c.set(0));
    let again = lay_out(&mut doc, 1, &ChartRequest::default()).unwrap();
    assert_eq!(again.done.joins, 3_000);
    assert!(NODES_WALKED.with(std::cell::Cell::get) <= 16 * total);
}

#[test]
fn a_chain_of_nine_thousand_managers_is_laid_out_and_a_link_that_closes_it_is_refused() {
    // A tree 9,000 deep: no recursion (the stack is the same as for a chain of two), the page is walked a few times, and a link that would make the last manager the first one's manager is
    // refused by one walk down the chain.
    let mut doc = chain_page(9_000, false);
    let total = doc.page(1).unwrap().shapes.len();
    NODES_WALKED.with(|c| c.set(0));
    let laid = lay_out(&mut doc, 1, &ChartRequest::default()).unwrap();
    assert_eq!(laid.positions, 9_000);
    assert!(NODES_WALKED.with(std::cell::Cell::get) <= 16 * total);
    NODES_WALKED.with(|c| c.set(0));
    let err = link(&mut doc, 1, 1, 9_000, 0).unwrap_err();
    assert!(err.message.contains("loop"), "{}", err.message);
    assert!(NODES_WALKED.with(std::cell::Cell::get) <= 4 * total, "the refusal is a few reads of the page");
}

#[test]
fn a_ring_of_positions_each_managing_the_next_is_laid_out_and_every_one_is_in_the_loop() {
    // A manages B manages ... manages A: the layout finishes (the forest takes each position once), and the chart says all of them are in the loop.
    let mut doc = chain_page(2_000, true);
    assert_eq!(chart(&doc, 1).unwrap().in_loops(), 2_000);
    let laid = lay_out(&mut doc, 1, &ChartRequest::default()).unwrap();
    assert_eq!(laid.positions, 2_000);
}

/// Gives a shape the four middles of its sides as connection points (a shape made by hand has none, and a connector cannot be glued to it).
fn with_points(mut shape: Shape) -> Shape {
    let mut section = Section::new("Connection", None);
    for (ix, (x, y)) in [(0.5, 1.0), (0.0, 0.5), (0.5, 0.0), (1.0, 0.5)].into_iter().enumerate() {
        let mut row = Row { ix: Some(ix as u32), ..Row::default() };
        for (name, value) in [("X", x), ("Y", y), ("DirX", 0.0), ("DirY", 0.0), ("Type", 0.0)] {
            row.cells.insert(CellName::new(name), Cell::constant(Value::from_text(&value.to_string())));
        }
        section.rows.push(Arc::new(row));
    }
    Arc::make_mut(&mut shape.sheet).sections.push(Arc::new(section));
    shape
}

/// A manager (shape 1) with 10,000 reports (shapes 2 to 10,001) joined by 10,000 connectors, a position that reports to nobody (shape 10,002) and `padding` plain boxes after them.
fn crowded_page(padding: u32) -> Document {
    let reports = 10_000;
    let mut shapes: Vec<Shape> =
        (1..=reports + 2).map(|id| with_points(position(id, f64::from(id % 400) * 2.0, f64::from(id / 400) * 2.0))).collect();
    let mut rows = Vec::new();
    for k in 0..reports {
        let id = reports + 3 + k;
        shapes.push(line(id));
        rows.push(glue(id, "BeginX", 1));
        rows.push(glue(id, "EndX", k + 2));
    }
    let first_plain = reports * 2 + 3;
    for k in 0..padding {
        let mut plain = Shape::new(first_plain + k);
        cells(&mut plain, &[("PinX", "500"), ("PinY", "500"), ("Width", "1"), ("Height", "1"), ("LocPinX", "0.5"), ("LocPinY", "0.5")]);
        shapes.push(plain);
    }
    doc_of(shapes, rows)
}

#[test]
fn a_link_to_a_manager_with_ten_thousand_reports_on_a_crowded_page_is_a_few_walks_and_no_search_for_each_report() {
    // The new report is set past its manager's other reports by reading each of them from the one list of the page's positions, never by a search from the top of the page for
    // each (10,000 searches over a page of 40,000 shapes are hundreds of millions of steps). Counted, not timed: the nodes a search visits (`finds_visited`), the nodes the page's walks visit.
    let mut doc = crowded_page(20_000);
    let total = doc.page(1).unwrap().shapes.len();
    assert_eq!(total, 40_002);
    NODES_WALKED.with(|c| c.set(0));
    FINDS.with(|c| c.set(0));
    diacraft_doc::work::reset();
    let linked = link(&mut doc, 1, 10_002, 1, 0).unwrap();
    assert!(linked.changed && linked.replaced.is_empty());
    let searched = diacraft_doc::work::finds_visited();
    let walked = NODES_WALKED.with(std::cell::Cell::get);
    assert!(
        searched <= 4 * total as u64,
        "{searched} nodes visited by searches from the top of a page of {total} shapes: a search for each of 10,000 reports"
    );
    assert!(walked <= 16 * total, "{walked} nodes walked for a page of {total} shapes");
    assert!(FINDS.with(std::cell::Cell::get) <= 8, "searches from the top of the page: {}", FINDS.with(std::cell::Cell::get));
    // The report is the manager's last: past every other in the layout's frame (down and to the right on a page that flows down).
    let doc_ref = &doc;
    let reader = crate::draw::read::Reader::new(doc_ref);
    let at = |id: u32| {
        let found = positions(doc_ref, 1).unwrap().into_iter().find(|p| p.id == id).unwrap();
        let c = reader.corners(&found.at);
        (c.iter().map(|p| p.0).sum::<f64>() / 4.0, c.iter().map(|p| p.1).sum::<f64>() / 4.0)
    };
    let (own, other) = (at(10_002), at(2));
    assert!(own.0 > other.0 && own.1 < other.1, "{own:?} past {other:?}");
}

#[test]
fn a_page_of_a_hundred_thousand_shapes_refuses_a_link_in_words_before_any_search() {
    let mut doc = crowded_page(80_000);
    let before = doc.clone();
    NODES_WALKED.with(|c| c.set(0));
    diacraft_doc::work::reset();
    let err = link(&mut doc, 1, 10_002, 1, 0).unwrap_err();
    assert!(err.message.contains("more than the 60000") && err.message.contains("split the drawing"), "{}", err.message);
    assert_eq!(doc, before);
    assert_eq!(diacraft_doc::work::finds_visited(), 0, "no search");
    assert_eq!(NODES_WALKED.with(std::cell::Cell::get), 0, "no walk");
}
