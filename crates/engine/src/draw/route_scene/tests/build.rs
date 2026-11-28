//! The documents the tests build: boxes, groups, connectors with routes, the glue between them, and the edits a test makes to one.

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Connect, Connects, Document, Owner, RowKey, Shape, SheetRef, Value};
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};

use crate::draw::read::{Reader, route_marker};
use crate::draw::{page_mut, shape_mut};

fn n(v: f64) -> String {
    v.to_string()
}

/// A two-dimensional shape whose box is `w` by `h` with its lower left corner at (`x`, `y`) on the page.
pub(super) fn boxed(id: u32, (x, y, w, h): (f64, f64, f64, f64)) -> ShapeBuilder {
    ShapeBuilder::new(id)
        .cell("PinX", &n(x + w / 2.0))
        .cell("PinY", &n(y + h / 2.0))
        .cell("Width", &n(w))
        .cell("Height", &n(h))
        .cell("LocPinX", &n(w / 2.0))
        .cell("LocPinY", &n(h / 2.0))
}

/// A box that is placeable (`ObjType` 1).
pub(super) fn placeable(id: u32, rect: (f64, f64, f64, f64)) -> ShapeBuilder {
    boxed(id, rect).cell("ObjType", "1")
}

/// A group whose frame has its pin at its lower left corner (`x`, `y`) on the page, so that a member's cells are in the group's frame, which is the page's moved by (`x`, `y`).
pub(super) fn group(id: u32, (x, y, w, h): (f64, f64, f64, f64), objtype: &str, members: Vec<ShapeBuilder>) -> ShapeBuilder {
    let mut g = ShapeBuilder::new(id)
        .group()
        .cell("PinX", &n(x))
        .cell("PinY", &n(y))
        .cell("Width", &n(w))
        .cell("Height", &n(h))
        .cell("LocPinX", "0")
        .cell("LocPinY", "0")
        .cell("ObjType", objtype);
    for member in members {
        g = g.child(member);
    }
    g
}

/// A one-dimensional shape whose route is `points` on the page: its frame is the page's, its begin the first point and its end the last.
pub(super) fn line(id: u32, points: &[(f64, f64)]) -> ShapeBuilder {
    let (begin, end) = (points.first().copied().unwrap_or((0.0, 0.0)), points.last().copied().unwrap_or((0.0, 0.0)));
    let mut s = ShapeBuilder::new(id)
        .cell("PinX", "0")
        .cell("PinY", "0")
        .cell("LocPinX", "0")
        .cell("LocPinY", "0")
        .cell("Width", &n(end.0 - begin.0))
        .cell("Height", &n(end.1 - begin.1))
        .cell("BeginX", &n(begin.0))
        .cell("BeginY", &n(begin.1))
        .cell("EndX", &n(end.0))
        .cell("EndY", &n(end.1));
    for (k, (x, y)) in points.iter().enumerate() {
        let row = RowKey::Ix(u32::try_from(k).unwrap() + 1);
        s = s.row_type("Geometry", Some(0), row.clone(), if k == 0 { "MoveTo" } else { "LineTo" });
        s = s.row_cell("Geometry", Some(0), row.clone(), "X", &n(*x)).row_cell("Geometry", Some(0), row, "Y", &n(*y));
    }
    s
}

/// A connector DiaCraft made: its route is its own (`User.DiaCraftRoute`).
pub(super) fn ours(s: ShapeBuilder) -> ShapeBuilder {
    s.set(route_marker(), Cell::constant(Value::from_text("1")))
}

/// A `Connect` row: the end cell `cell` of shape `from` is glued to shape `to`.
pub(super) fn glue(from: u32, cell: &str, to: u32) -> Connect {
    Connect { from_sheet: from, from_cell: Some(Arc::from(cell)), to_sheet: to, to_part: Some(100), ..Connect::default() }
}

/// Both ends of connector `from` glued: its begin to `a` and its end to `b`.
pub(super) fn glue_both(from: u32, a: u32, b: u32) -> Vec<Connect> {
    vec![glue(from, "BeginX", a), glue(from, "BeginY", a), glue(from, "EndX", b), glue(from, "EndY", b)]
}

/// A document of page 1 holding `shapes` at its top level and the glue `connects`.
pub(super) fn page(shapes: Vec<ShapeBuilder>, connects: Vec<Connect>) -> Document {
    page_in(DocBuilder::new(), shapes, connects)
}

/// [`page`] in the document `base` builds (with its masters, styles and root cells).
pub(super) fn page_in(base: DocBuilder, shapes: Vec<ShapeBuilder>, connects: Vec<Connect>) -> Document {
    let built = shapes.into_iter().fold(PageBuilder::new(), PageBuilder::shape);
    let mut doc = base.page(1, built).build();
    page_mut(&mut doc, 1).unwrap().connects = Connects::from_vec(connects);
    doc
}

/// The `Shape` a builder makes, for a test that adds one to a document.
pub(super) fn made(shape: ShapeBuilder) -> Arc<Shape> {
    let doc = DocBuilder::new().page(9, PageBuilder::new().shape(shape)).build();
    doc.pages.get(0).unwrap().shapes.get(0).unwrap().clone()
}

/// Where shape `id` is among the top-level shapes of page 1.
pub(super) fn index_of(doc: &Document, id: u32) -> usize {
    doc.page(1).unwrap().shapes.iter().position(|s| s.id == id).unwrap()
}

/// The number a cell of top-level shape `id` reads.
pub(super) fn number(doc: &Document, id: u32, name: &str) -> f64 {
    let at = diacraft_doc::ShapeRef { owner: Owner::Page(1), path: vec![u32::try_from(index_of(doc, id)).unwrap()] };
    Reader::new(doc).number(&SheetRef::Shape(at), name)
}

/// Writes a local number into a cell of top-level shape `id`, as an edit does: the shape is copied, and so is its sheet.
pub(super) fn set_number(doc: &mut Document, id: u32, name: &str, value: f64) {
    let at = u32::try_from(index_of(doc, id)).unwrap();
    let shape = shape_mut(doc, 1, &[at]).unwrap();
    Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new(name), Cell::constant(Value::from_number(value).unwrap()));
}

/// Moves top-level shape `id` by (`dx`, `dy`).
pub(super) fn shift(doc: &mut Document, id: u32, dx: f64, dy: f64) {
    let (x, y) = (number(doc, id, "PinX"), number(doc, id, "PinY"));
    set_number(doc, id, "PinX", x + dx);
    set_number(doc, id, "PinY", y + dy);
}

/// Takes top-level shape `id` out of page 1.
pub(super) fn remove(doc: &mut Document, id: u32) {
    let at = index_of(doc, id);
    page_mut(doc, 1).unwrap().shapes.remove(at).unwrap();
}

/// Adds a top-level shape at the end of page 1.
pub(super) fn add(doc: &mut Document, shape: ShapeBuilder) {
    page_mut(doc, 1).unwrap().shapes.push(made(shape));
}

/// Replaces the glue of page 1.
pub(super) fn reglue(doc: &mut Document, connects: Vec<Connect>) {
    page_mut(doc, 1).unwrap().connects = Connects::from_vec(connects);
}

/// Adds layers named `names` to page 1 (their `IX` are 0, 1, 2 and so on).
pub(super) fn add_layers(doc: &mut Document, names: &[&str]) {
    for name in names {
        crate::draw::layers::add_layer(doc, 1, &crate::draw::layers::Settings { name: Some((*name).to_owned()), ..Default::default() }).unwrap();
    }
}

/// Shows or hides the layer `ix` of page 1.
pub(super) fn show_layer(doc: &mut Document, ix: u32, visible: bool) {
    crate::draw::layers::set_layer(doc, 1, ix, &crate::draw::layers::Settings { visible: Some(visible), ..Default::default() }).unwrap();
}

/// Puts top-level shape `id` on the layers `list` (`LayerMember`, such as `"0;1"`).
pub(super) fn on_layers(doc: &mut Document, id: u32, list: &str) {
    let at = u32::try_from(index_of(doc, id)).unwrap();
    let shape = shape_mut(doc, 1, &[at]).unwrap();
    Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new("LayerMember"), Cell::constant(Value::from_text(list)));
}

/// A row of boxes: `count` boxes of 1 by 1 along y = 0, one every 2 inches from the `ID` `first`, each placeable.
pub(super) fn row_of_boxes(first: u32, count: u32) -> Vec<ShapeBuilder> {
    (0..count).map(|k| placeable(first + k, (f64::from(k) * 2.0, 0.0, 1.0, 1.0))).collect()
}

/// Moves the top-level shape at position `at` of page 1 by `dx` along x: the shape is copied, and so is its sheet. By position, so that an edit of every shape of a big page is not a search for each.
pub(super) fn nudge_at(doc: &mut Document, at: u32, dx: f64) {
    let x = {
        let me = diacraft_doc::ShapeRef { owner: Owner::Page(1), path: vec![at] };
        Reader::new(doc).number(&SheetRef::Shape(me), "PinX")
    };
    let shape = shape_mut(doc, 1, &[at]).unwrap();
    Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new("PinX"), Cell::constant(Value::from_number(x + dx).unwrap()));
}
