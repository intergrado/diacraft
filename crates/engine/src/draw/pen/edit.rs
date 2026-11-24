//! The edits of the document the drawing tools make: a new shape with the path a tool drew (`shape.draw`), a path carried on from the end vertex of a shape drawn before (`continue`), and
//! a path closed (`shape.close`). Every length here is in the page's **drawing units**; the commands (`cmd/draw_tools.rs`) turn paper inches into them at their edge.
//!
//! **A path is carried on by the shape's own frame.** The rows of the shape are read as the geometry crate reads them, carried exactly (a row keeps its kind, `shape_ops::carry`), the
//! new rows are put into the shape's own frame (the page's points through the pose, an arc's bow turned the other way in a mirrored frame), and the whole is written again as the
//! shares of its new box, as `shape.subtract` writes a shape's geometry. The pin is then moved so that nothing already drawn moves on the page. A shape that is not carried on is not
//! touched, and a refusal leaves the document as it was (the caller edits a copy).

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Document, MAX_SHAPES_PER_PAGE, Row, Rows, SheetRef, Value};
use kurbo::{Affine, Point};

use super::bounds::bounds;
use super::{Drawn, P, Tool};
use crate::draw::shapeops::{CRow, Frame, carry, flagged, row_of};
use crate::draw::{Pose, Problem, Reader, checked, page_mut, page_stats, require_page, require_shape, settle, shape_mut, write_length};
use crate::stencil::{self, DrawnFrame, drawn_shape};

/// The most rows of an existing path that a path is carried on from.
pub(crate) const MAX_CONTINUED_ROWS: usize = 4_096;

/// What an edit made or changed, in drawing units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Made {
    /// The `ID` of the shape.
    pub id: u32,
    /// The size of its box.
    pub width: f64,
    pub height: f64,
    /// How many rows its path has now.
    pub rows: usize,
    /// Whether the path is closed.
    pub closed: bool,
}

fn plain(n: f64) -> Cell {
    Cell::constant(Value::from_text(&stencil::number_text(n)))
}

fn at(p: Point) -> P {
    (p.x, p.y)
}

/// The row with every point carried by `point` and every bow by `bow`. `None` for a row the tools do not make.
fn moved(row: &CRow, point: &dyn Fn(Point) -> Point, bow: &dyn Fn(f64) -> f64) -> Option<CRow> {
    Some(match row {
        CRow::Move(p) => CRow::Move(point(*p)),
        CRow::Line(p) => CRow::Line(point(*p)),
        CRow::Arc { to, bow: b } => CRow::Arc { to: point(*to), bow: bow(*b) },
        CRow::Ellipse { centre, first, second } => CRow::Ellipse { centre: point(*centre), first: point(*first), second: point(*second) },
        CRow::Nurbs { to, knots, last_knot, degree, points } => CRow::Nurbs {
            to: point(*to),
            knots: *knots,
            last_knot: *last_knot,
            degree: *degree,
            points: points.iter().map(|(p, knot, weight)| (point(*p), *knot, *weight)).collect(),
        },
        _ => return None,
    })
}

impl Drawn {
    /// The same path with every length `k` times as long: paper inches to a page's drawing units.
    pub(crate) fn scaled(&self, k: f64) -> Result<Drawn, Problem> {
        let scale = |p: Point| Point::new(p.x * k, p.y * k);
        let rows = self
            .rows
            .iter()
            .map(|row| moved(row, &scale, &|b| b * k))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| Problem::failed("a tool made a row it cannot scale"))?;
        let r = self.rect;
        Ok(Drawn { rows, closed: self.closed, rect: kurbo::Rect::new(r.x0 * k, r.y0 * k, r.x1 * k, r.y1 * k) })
    }
}

/// Adds the shape `drawn` to a page, drawn by `tool`: a shape with no master (`stencil/drawn.rs`) whose path is the rows, as the shares of the box round them. Its `ID` is above
/// `above` as well as above every `ID` on the page (the session's high-water mark, as a dropped shape's is). A coordinate past the page's reach and a full page are refused.
pub(crate) fn add_drawn_shape_above(doc: &mut Document, page: u32, tool: Tool, drawn: &Drawn, above: u32) -> Result<Made, Problem> {
    let r = drawn.rect;
    for (name, value) in [("x", r.x0), ("y", r.y0), ("x", r.x1), ("y", r.y1)] {
        checked(name, value)?;
    }
    require_page(doc, page)?;
    let (highest, count) = page_stats(doc, page).ok_or_else(|| Problem::bad(format!("there is no page {page}")))?;
    if count >= MAX_SHAPES_PER_PAGE {
        return Err(Problem::failed(format!("a page holds at most {MAX_SHAPES_PER_PAGE} shapes")));
    }
    let id = highest.max(above).checked_add(1).ok_or_else(|| Problem::failed("no shape ids are left on the page"))?;
    let frame = Frame::of(r);
    let mut path = flagged(0, !drawn.closed);
    for (i, row) in drawn.rows.iter().enumerate() {
        path.rows.push(row_of(row, u32::try_from(i + 1).unwrap_or(u32::MAX), &frame));
    }
    let spec = DrawnFrame {
        pin: (frame.min.x + frame.width / 2.0, frame.min.y + frame.height / 2.0),
        width: frame.width,
        height: frame.height,
        sides: matches!(tool, Tool::Rectangle | Tool::Ellipse),
    };
    let shape = drawn_shape(id, &spec, path);
    page_mut(doc, page).ok_or_else(|| Problem::failed("the page went away"))?.shapes.push(Arc::new(shape));
    Ok(Made { id, width: frame.width, height: frame.height, rows: drawn.rows.len(), closed: drawn.closed })
}

/// The point a row ends at, the pen after it; `None` for the row that is a whole path (`Ellipse`).
fn end_of(row: &CRow) -> Option<Point> {
    match row {
        CRow::Move(p) | CRow::Line(p) => Some(*p),
        CRow::Arc { to, .. }
        | CRow::EllipticalArc { to, .. }
        | CRow::Cubic { to, .. }
        | CRow::Quad { to, .. }
        | CRow::Polyline { to, .. }
        | CRow::Nurbs { to, .. }
        | CRow::SplineStart { to, .. }
        | CRow::SplineKnot { to, .. } => Some(*to),
        CRow::Ellipse { .. } => None,
    }
}

/// The two places of a path the next stroke may be tied to, on the page, in the page's drawing units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Ends {
    /// Where the path ends: the stroke begins here.
    pub end: P,
    /// Where the path began: a stroke that ends here closes it.
    pub start: P,
}

/// What a path the edit reads from a shape is.
struct Existing {
    rows: Vec<CRow>,
    /// The `IX` of its `Geometry` section.
    ix: Option<u32>,
    pose: Pose,
}

/// Reads the one path of the shape `id` on a page, in its own frame: the shape must be on the page itself (not in a group), have no master, one `Geometry` section of a single run that
/// is open and ends at a vertex.
fn read_existing(doc: &Document, page: u32, id: u32) -> Result<Existing, Problem> {
    let at = require_shape(doc, page, id)?;
    if at.path.len() != 1 {
        return Err(Problem::failed(format!("shape {id} is in a group: a path is carried on only for a shape on the page itself")));
    }
    let shape = doc.shape(&at).ok_or_else(|| Problem::failed("the shape went away"))?;
    if shape.master.is_some() {
        return Err(Problem::failed(format!("shape {id} is an instance of a master: a path is carried on only for a shape that has its own")));
    }
    let reader = Reader::new(doc);
    let sheet = SheetRef::Shape(at.clone());
    let res = reader.resolver();
    let keys = match res.effective_sections_within(&sheet, "Geometry", 8) {
        Some(Ok(keys)) => keys,
        Some(Err(n)) => return Err(Problem::failed(format!("shape {id} has {n} paths: a path is carried on only for a shape of one"))),
        None => return Err(Problem::failed(format!("shape {id} has no path"))),
    };
    let [key] = keys.as_slice() else {
        return Err(Problem::failed(format!("shape {id} has {} paths: a path is carried on only for a shape of one", keys.len())));
    };
    let held = res.rows_held(&sheet, key).unwrap_or(0);
    if held == 0 {
        return Err(Problem::failed(format!("shape {id} has no path")));
    }
    if held > MAX_CONTINUED_ROWS {
        return Err(Problem::failed(format!(
            "the path of shape {id} has {held} rows, and a path of more than {MAX_CONTINUED_ROWS} is not carried on"
        )));
    }
    let Some(Ok(effective)) = res.effective_rows_within(&sheet, key, held) else {
        return Err(Problem::failed(format!("the path of shape {id} could not be read")));
    };
    let (geom, _) = diacraft_render::geom_rows(&effective);
    let size = (reader.number(&sheet, "Width"), reader.number(&sheet, "Height"));
    let rows = carry(&geom, size, Affine::IDENTITY);
    if rows.iter().any(|r| matches!(r, CRow::Ellipse { .. })) {
        return Err(Problem::failed(format!("shape {id}: an ellipse has no end vertex to go on from")));
    }
    let moves = rows.iter().filter(|r| matches!(r, CRow::Move(_))).count();
    if !matches!(rows.first(), Some(CRow::Move(_))) || moves != 1 {
        return Err(Problem::failed(format!("the path of shape {id} is not one run that begins with a move: it cannot be carried on")));
    }
    Ok(Existing { rows, ix: key.ix, pose: reader.pose(&sheet) })
}

/// Where the path begins and ends, in its own frame: an error in words when it has no end vertex to go on from (it is closed, or ends in no vertex).
fn ends_of(rows: &[CRow]) -> Result<(Point, Point), &'static str> {
    let (Some(start), Some(end)) = (rows.first().and_then(end_of), rows.last().and_then(end_of)) else {
        return Err("the path has no end vertex to go on from");
    };
    if rows.len() > 2 && (start.x - end.x).hypot(start.y - end.y) <= 1.0e-9 {
        return Err("the path is closed: it has no end vertex to go on from");
    }
    Ok((start, end))
}

/// The ends of the open path of a shape, in paper inches on the page.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vertices {
    /// The vertex the path goes on from.
    pub end: P,
    /// The first vertex: a stroke let go over it closes the path.
    pub start: P,
}

/// The ends of the path of shape `id` if a path can be carried on from it (`shape.draw` with `continue`): the shape is on the page, has no master, and has one open path that ends in a vertex.
/// The window asks it when a drawing tool is pressed, to tell a stroke that goes on from one that begins.
pub fn continuable(doc: &Document, page: u32, id: u32) -> Option<Vertices> {
    let existing = read_existing(doc, page, id).ok()?;
    let (start, end) = ends_of(&existing.rows).ok()?;
    let k = crate::draw::units_per_inch(doc, page);
    let inches = |p: Point| {
        let q = existing.pose.up(at(p));
        (q.0 / k, q.1 / k)
    };
    Some(Vertices { end: inches(end), start: inches(start) })
}

/// Carries on the path of shape `id` (a shape with one open path, `read_existing`): `build` is given where the path ends and began, on the page, and gives the rows to add, on the page,
/// not counting the move to their start (the path is already there). With `close` the path is closed: `build` is to end the rows on the start, and the path takes the fill (`NoFill`
/// 0). The shape's box grows to the new path and its pin moves so that what was drawn stays where it was.
pub(crate) fn continue_shape(
    doc: &mut Document,
    page: u32,
    id: u32,
    close: bool,
    build: impl FnOnce(Ends) -> Result<Vec<CRow>, Problem>,
) -> Result<Made, Problem> {
    let existing = read_existing(doc, page, id)?;
    let (start, end) = ends_of(&existing.rows).map_err(|why| Problem::failed(format!("{why} (shape {id})")))?;
    let pose = existing.pose;
    let added = build(Ends { end: pose.up(at(end)), start: pose.up(at(start)) })?;
    if added.is_empty() {
        return Err(Problem::bad("there is nothing to add to the path"));
    }
    // The new rows, in the shape's own frame: the page's points through the pose, and a bow to the other side where the frame is a mirror.
    let mirrored = pose.flip_x != pose.flip_y;
    let into_frame = |p: Point| {
        let q = pose.down(at(p));
        Point::new(q.0, q.1)
    };
    let flip = |b: f64| if mirrored { -b } else { b };
    let mut rows = existing.rows;
    for row in &added {
        rows.push(moved(row, &into_frame, &flip).ok_or_else(|| Problem::failed("a tool made a row it cannot carry on with"))?);
    }
    if rows.len() > MAX_CONTINUED_ROWS {
        return Err(Problem::failed(format!("the path would have more than {MAX_CONTINUED_ROWS} rows")));
    }
    let new_end = rows.last().and_then(end_of).ok_or_else(|| Problem::failed("the path has no end"))?;
    let closed = close || (start.x - new_end.x).hypot(start.y - new_end.y) <= 1.0e-9;
    let rect = bounds(&rows).ok_or_else(|| Problem::failed("the path could not be drawn"))?;
    for value in [rect.x0, rect.y0, rect.x1, rect.y1] {
        checked("coordinate", value)?;
    }
    let frame = Frame::of(rect);
    let written: Vec<Arc<Row>> = rows.iter().enumerate().map(|(i, row)| row_of(row, u32::try_from(i + 1).unwrap_or(u32::MAX), &frame)).collect();
    let at_shape = require_shape(doc, page, id)?;
    let sheet = SheetRef::Shape(at_shape.clone());
    let before = doc.clone();
    {
        let shape = shape_mut(doc, page, &at_shape.path).ok_or_else(|| Problem::failed("the shape went away"))?;
        let cells = Arc::make_mut(&mut shape.sheet);
        let place = cells.sections.iter().position(|s| s.name.as_str() == "Geometry" && s.ix == existing.ix);
        let Some(held) = place.and_then(|p| cells.sections.get_mut(p)) else {
            return Err(Problem::failed("the path went away"));
        };
        let mut section = (**held).clone();
        section.rows = Rows::from(written);
        if closed {
            section.cells.insert(CellName::new("NoFill"), plain(0.0));
        }
        *held = Arc::new(section);
    }
    write_length(doc, &sheet, "Width", frame.width)?;
    write_length(doc, &sheet, "Height", frame.height)?;
    settle(doc, &before, page, &[id], &[])?;
    // The local pin follows the new size by its formula; the pin moves by what the box's corner and the local pin moved, so that nothing moves on the page.
    let loc_after = {
        let reader = Reader::new(doc);
        (reader.number(&sheet, "LocPinX"), reader.number(&sheet, "LocPinY"))
    };
    let by = pose.up_vector((frame.min.x + loc_after.0 - pose.loc.0, frame.min.y + loc_after.1 - pose.loc.1));
    let before_pin = doc.clone();
    write_length(doc, &sheet, "PinX", checked("x", pose.pin.0 + by.0)?)?;
    write_length(doc, &sheet, "PinY", checked("y", pose.pin.1 + by.1)?)?;
    settle(doc, &before_pin, page, &[id], &[])?;
    Ok(Made { id, width: frame.width, height: frame.height, rows: rows.len(), closed })
}

/// Closes the open path of shape `id`: a straight segment from its end to its start, and the path takes the fill (`NoFill` 0). A path that is closed already is refused, and the
/// document is as it was.
pub(crate) fn close_shape(doc: &mut Document, page: u32, id: u32) -> Result<Made, Problem> {
    continue_shape(doc, page, id, true, |ends| Ok(vec![CRow::Line(Point::new(ends.start.0, ends.start.1))]))
}
