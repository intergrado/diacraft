//! The cost of gluing the connectors of a layout again (`connectors::reglue_all`): counted, not timed. The points of a shape are read once for all the connectors glued to it, and
//! the held points are kept by shape and by connector, so a shape with ten thousand points and ten thousand connectors glued to it is not a hundred million rows; a page of a hundred thousand
//! connectors is refused in words before any of it is read.

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Cells, Connect, Connects, Document, Owner, Row, Section, Shape, ShapeList, ShapeRef, Value};
use diacraft_layout::{Direction, Style};

use super::{Request, relayout};
use crate::draw::connectors::{Reglue, reglue_all};
use crate::draw::page_mut;

fn cells(shape: &mut Shape, list: &[(&str, &str)]) {
    let mut sheet = diacraft_doc::Sheet::default();
    for (name, value) in list {
        sheet.cells.insert(CellName::new(name), Cell::constant(Value::from_text(value)).with_unit("IN"));
    }
    shape.sheet = Arc::new(sheet);
}

/// A box 2 by 2 at (`x`, `y`) with `points` connection points on its edge, the first four at the middles of its sides (left, top, right, bottom).
fn boxed(id: u32, x: f64, y: f64, points: u32) -> Shape {
    let mut shape = Shape::new(id);
    cells(&mut shape, &[("PinX", &x.to_string()), ("PinY", &y.to_string()), ("Width", "2"), ("Height", "2"), ("LocPinX", "1"), ("LocPinY", "1")]);
    let mut section = Section::new("Connection", None);
    for k in 0..points {
        let (px, py, dx, dy) = match k {
            0 => (0.0, 1.0, -1.0, 0.0),
            1 => (1.0, 2.0, 0.0, 1.0),
            2 => (2.0, 1.0, 1.0, 0.0),
            3 => (1.0, 0.0, 0.0, -1.0),
            _ => (2.0 * f64::from(k) / f64::from(points), 0.0, 0.0, -1.0),
        };
        let mut row = Cells::new();
        for (name, v) in [("X", px), ("Y", py), ("DirX", dx), ("DirY", dy)] {
            row.insert(CellName::new(name), Cell::constant(Value::from_text(&v.to_string())));
        }
        section.rows.push(Arc::new(Row { ix: Some(k), cells: row, ..Row::default() }));
    }
    let sheet = Arc::make_mut(&mut shape.sheet);
    sheet.sections.push(Arc::new(section));
    shape
}

fn line(id: u32) -> Shape {
    let mut line = Shape::new(id);
    cells(&mut line, &[("BeginX", "0"), ("BeginY", "0"), ("EndX", "2"), ("EndY", "0"), ("PinX", "1"), ("PinY", "0")]);
    line
}

/// The rows of a connector glued at its begin to point `a` of shape `from` and at its end to point `b` of shape `to`.
fn glue(id: u32, (from, a): (u32, u32), (to, b): (u32, u32)) -> [Connect; 2] {
    let row = |cell: &str, part: u32, target: u32, index: u32| Connect {
        from_sheet: id,
        from_cell: Some(Arc::from(cell)),
        from_part: Some(part),
        to_sheet: target,
        to_cell: Some(Arc::from(format!("Connections.X{}", index + 1))),
        to_part: Some(100 + index),
        ..Connect::default()
    };
    [row("BeginX", 9, from, a), row("EndX", 12, to, b)]
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

/// A hub (shape 1, with `points` connection points) and a plain shape (shape 2, four points), `n` connectors from the hub's points to the other's: shapes 3 to n + 2.
fn hub_page(n: u32, points: u32) -> (Document, Vec<Reglue>) {
    let mut shapes = vec![boxed(1, 2.0, 10.0, points), boxed(2, 12.0, 2.0, 4)];
    let (mut rows, mut plans) = (Vec::new(), Vec::new());
    for k in 0..n {
        let id = 3 + k;
        shapes.push(line(id));
        rows.extend(glue(id, (1, k % points), (2, k % 4)));
        plans.push((id, k));
    }
    let doc = doc_of(shapes, rows);
    let plans = plans
        .into_iter()
        .map(|(id, _)| Reglue { id, at: ShapeRef { owner: Owner::Page(1), path: vec![id - 1] }, from: 1, to: 2, sides: None })
        .collect();
    (doc, plans)
}

fn rows_merged_by(work: impl FnOnce()) -> u64 {
    diacraft_doc::work::reset();
    work();
    diacraft_doc::work::rows_merged()
}

#[test]
fn ten_thousand_connectors_on_a_shape_of_ten_thousand_points_read_its_points_once() {
    // The finding: each connector read the points of both its shapes again, so the hub's ten thousand rows were read ten thousand times (a hundred million rows). Counted: the rows the
    // model merged to read the two shapes' points are the rows they hold (10,004), with room for the call's other reads, and not that times the connectors.
    let (mut doc, plans) = hub_page(10_000, 10_000);
    let merged = rows_merged_by(|| {
        let done = reglue_all(&mut doc, 1, &plans).unwrap();
        assert_eq!(done.glued, 10_000);
    });
    assert!(merged <= 2 * 10_004, "{merged} rows merged for shapes that hold 10,004");
}

#[test]
fn a_relayout_of_ten_thousand_connectors_on_a_hub_costs_a_few_reads_of_its_points() {
    // The whole relayout against the same page with a hub of four points: the extra rows merged are a few reads of the hub's ten thousand (the glue's placing, the routing and this
    // pass each read it once), and never one for each connector.
    let merged_for = |points: u32| {
        let (mut doc, _) = hub_page(10_000, points);
        rows_merged_by(|| {
            relayout(&mut doc, 1, None, &request()).unwrap();
        })
    };
    let (yardstick, hub) = (merged_for(4), merged_for(10_000));
    assert!(hub <= yardstick + 6 * 10_000, "{hub} rows merged against a yardstick of {yardstick}");
}

#[test]
fn the_held_points_are_kept_by_connector_so_that_ten_thousand_connectors_over_a_thousand_shapes_cost_a_few_passes() {
    // 1,000 shapes of four points, 5,000 connectors between neighbours: every shape's points are read once for the call (4,000 rows), and the passes over the held points are not one for
    // each connector (counted by the rows merged here, and by the call finishing: a list of every shape's points walked for each connector is 5,000 times 1,000).
    let mut shapes: Vec<Shape> = (1..=1_000).map(|id| boxed(id, f64::from(id % 40) * 4.0, f64::from(id / 40) * 4.0, 4)).collect();
    let (mut rows, mut plans) = (Vec::new(), Vec::new());
    for k in 0..5_000_u32 {
        let (from, to) = (1 + k % 1_000, 1 + (k * 7 + 3) % 1_000);
        if from == to {
            continue;
        }
        let id = 2_000 + k;
        shapes.push(line(id));
        rows.extend(glue(id, (from, k % 4), (to, (k + 1) % 4)));
        plans.push(Reglue { id, at: ShapeRef { owner: Owner::Page(1), path: vec![shapes.len() as u32 - 1] }, from, to, sides: None });
    }
    let mut doc = doc_of(shapes, rows);
    let merged = rows_merged_by(|| {
        let done = reglue_all(&mut doc, 1, &plans).unwrap();
        assert_eq!(done.glued, plans.len());
    });
    assert!(merged <= 2 * 4_000, "{merged} rows merged for 1,000 shapes of four points");
}

#[test]
fn a_hundred_thousand_connectors_over_a_thousand_shapes_are_refused_in_words_before_any_point_is_read() {
    let mut shapes: Vec<Shape> = (1..=1_000).map(|id| boxed(id, f64::from(id % 40) * 4.0, f64::from(id / 40) * 4.0, 4)).collect();
    let mut rows = Vec::new();
    for k in 0..100_000_u32 {
        let id = 2_000 + k;
        shapes.push(line(id));
        rows.extend(glue(id, (1 + k % 1_000, 0), (1 + (k + 1) % 1_000, 2)));
    }
    let mut doc = doc_of(shapes, rows);
    let before = doc.clone();
    let mut refused = None;
    let merged = rows_merged_by(|| refused = Some(relayout(&mut doc, 1, None, &request()).unwrap_err()));
    let words = refused.unwrap().message;
    assert!(words.contains("more than the 100000") || words.contains("more than 10000 connectors"), "{words}");
    assert_eq!(merged, 0, "no point was read");
    assert_eq!(doc, before);
}

#[test]
fn a_shape_past_the_budget_of_point_rows_is_skipped_and_counted_and_its_connectors_keep_their_points() {
    // A hub of 70,000 points is past the 65,536 rows the call reads: it is not read at all, and the connectors glued to it stay as they are, said in the count.
    let (mut doc, plans) = hub_page(50, 70_000);
    let before = doc.page(1).unwrap().connects.clone();
    let merged = rows_merged_by(|| {
        let done = reglue_all(&mut doc, 1, &plans).unwrap();
        assert_eq!((done.glued, done.skipped), (0, 50));
    });
    assert!(merged <= 4, "the hub was not read: {merged} rows merged");
    assert_eq!(doc.page(1).unwrap().connects, before, "the glue is as it was");
}
