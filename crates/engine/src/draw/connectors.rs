//! Connectors: one-dimensional shapes glued at their ends, with a right-angle route.
//!
//! **A connector** (and the formulas the specification prints as the ones written on creation)
//! is a shape with no master and these cells: `BeginX`, `BeginY`, `EndX` and `EndY`; `Width` the distance between
//! the ends; `Angle` the direction from the begin to the end; `PinX` and `PinY` the middle between them; `LocPinX` half the
//! width; `Height` 0. So the begin is at (0, `LocPinY`) of its own frame and the end at (`Width`, `LocPinY`), and a point of the page
//! reaches the frame by [`Pose::down`].
//!
//! **Glue**: an end glued to connection point `n` (counting from 1 in formulas, from 0 as
//! `IX`) of shape `S` has the formula `PAR(PNT(Sheet.S!Connections.Xn,Sheet.S!Connections.Yn))` in both its cells, and a `Connect` row
//! (`FromPart` 9 for the begin and 12 for the end, `ToPart` 100 plus `IX`). A recalculation then moves the end with the shape.
//!
//! **The route** is `Geometry` rows of constants in the connector's frame ([`route`]), worked out by [`plan_route`] and written by
//! [`write_planned`] after each recalculation. The marker `User.DiaCraftRoute` says a connector is ours; a connector read from a file has none, and is routed again only when
//! a command moves one of its glued ends and its route is not fixed ([`super::reroute`]).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use diacraft_doc::{
    Cell, CellAddr, CellEdit, CellName, Connect, Connects, Document, Links, Row, RowKey, Rows, Section, SectionKey, Shape, ShapeRef, ShapeType,
    SheetRef, Value,
};

use super::read::{GluedPoints, Pose, Reader};
use super::reroute::MAX_CROSSING_POINT_ROWS;
use super::route::{Dir, P};
use super::route_clear;
use super::rows;
use super::settle::{has_error, settle};
use super::shapes::page_stats;
use super::style::{self, PathStyle};
use super::{Problem, checked, page_mut, require_page, require_shape, shape_mut, write_cells};
use crate::stencil::number_text;

/// How many connection points of a shape are looked at when a connector picks the nearest.
const MAX_CANDIDATES: usize = 256;

/// The `ToPart` of a connection point's row: 100 plus its `IX`.
pub(super) const TO_PART_BASE: u32 = 100;

/// Where one end of a connector goes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum End {
    /// Glued to a shape: at the connection point with this `IX`, or at the one nearest the other end when there is none given.
    Shape { id: u32, point: Option<u32> },
    /// Free, at this place on the page.
    Point { x: f64, y: f64 },
}

/// Which end of a connector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Which {
    Begin,
    End,
}

impl Which {
    pub(super) fn x_cell(self) -> &'static str {
        match self {
            Which::Begin => "BeginX",
            Which::End => "EndX",
        }
    }

    pub(super) fn y_cell(self) -> &'static str {
        match self {
            Which::Begin => "BeginY",
            Which::End => "EndY",
        }
    }

    /// `FromPart` of the `Connect` row of this end (9 begin point, 12 end point).
    fn part_code(self) -> u32 {
        match self {
            Which::Begin => 9,
            Which::End => 12,
        }
    }
}

/// What an edit of a connector made: its `ID`, and where each end is glued as (shape `ID`, point `IX`), if it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnectorInfo {
    pub id: u32,
    pub begin: Option<(u32, u32)>,
    pub end: Option<(u32, u32)>,
}

/// An end worked out: where it is, and what it is glued to.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Anchor {
    glue: Option<(u32, u32)>,
    at: P,
    /// The way a connector leaves it, snapped to an axis (its side of the shape).
    side: Option<Dir>,
    /// Whether a connector other than the one being made already holds this connection point.
    taken: bool,
}

/// The connection points of shape `id` that a connector other than `except` is glued to.
fn taken_points(doc: &Document, page: u32, id: u32, except: Option<u32>) -> HashSet<u32> {
    let Some(p) = doc.page(page) else { return HashSet::new() };
    p.connects.iter().filter(|c| c.to_sheet == id && Some(c.from_sheet) != except).filter_map(|c| c.to_part?.checked_sub(TO_PART_BASE)).collect()
}

/// The places an end can be: one for a free end or a named point, all the points of a shape otherwise. `except` is the connector being moved,
/// whose own place does not count as taken.
fn candidates(reader: &Reader<'_>, page: u32, end: &End, except: Option<u32>) -> Result<Vec<Anchor>, Problem> {
    match *end {
        End::Point { x, y } => Ok(vec![Anchor { glue: None, at: (checked("x", x)?, checked("y", y)?), side: None, taken: false }]),
        End::Shape { id, point } => {
            let at = reader.find(page, id).ok_or_else(|| Problem::bad(format!("page {page} has no shape {id}")))?;
            let taken = taken_points(reader.doc(), page, id, except);
            shape_anchors(reader, page, id, &at, &taken, point)
        }
    }
}

/// The anchors of the connection points of shape `id`, which is at `at`: all of them, or the one `point` names. `taken` are the points other connectors hold.
fn shape_anchors(reader: &Reader<'_>, page: u32, id: u32, at: &ShapeRef, taken: &HashSet<u32>, point: Option<u32>) -> Result<Vec<Anchor>, Problem> {
    // A shape whose layers allow no glue is glued to by no new end: refused in words, whatever else the end asks.
    if let Some(layer) = reader.doc().shape(at).and_then(|shape| super::layers::glue_barred(reader.doc(), page, shape)) {
        return Err(Problem::bad(super::layers::glue_words(id, &layer)));
    }
    let points = reader.connection_points(at);
    if points.is_empty() {
        return Err(Problem::failed(format!("shape {id} has no connection points to glue to")));
    }
    let mut anchors = points.iter().take(MAX_CANDIDATES).map(|p| Anchor {
        glue: Some((id, p.index)),
        at: (p.x, p.y),
        side: Dir::from_vector(p.dir_x, p.dir_y),
        taken: taken.contains(&p.index),
    });
    match point {
        None => Ok(anchors.collect()),
        Some(wanted) => anchors
            .find(|a| a.glue.is_some_and(|(_, index)| index == wanted))
            .map(|a| vec![a])
            .ok_or_else(|| Problem::bad(format!("shape {id} has no connection point {wanted}"))),
    }
}

fn distance_squared(a: P, b: P) -> f64 {
    (a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)
}

/// The nearest pair, one from each list: the first of equals. Two ends on one shape do not take the same point while there is a choice.
fn nearest_pair(a: &[Anchor], b: &[Anchor]) -> Option<(Anchor, Anchor)> {
    let mut best: Option<(f64, Anchor, Anchor)> = None;
    for pa in a {
        for pb in b {
            if pa.glue.is_some() && pa.glue == pb.glue && (a.len() > 1 || b.len() > 1) {
                continue;
            }
            let d = distance_squared(pa.at, pb.at);
            if best.is_none_or(|(least, _, _)| d < least) {
                best = Some((d, *pa, *pb));
            }
        }
    }
    best.map(|(_, x, y)| (x, y))
}

/// Our rule: a connection point that another connector already holds, picked as the nearest for an end whose point
/// was not named, is given up for the free point of `among` on the same side of the shape (the same way out) that is nearest `toward`, when there
/// is one: two connectors between the same shapes then leave by different points, side by side, and do not run on top of each other. With none
/// free on that side the nearest stays, shared; a point that was named is always the point.
fn spread(chosen: Anchor, among: &[Anchor], automatic: bool, toward: P) -> Anchor {
    if !automatic || !chosen.taken {
        return chosen;
    }
    let Some(side) = chosen.side else { return chosen };
    let mut best: Option<(f64, Anchor)> = None;
    for a in among.iter().filter(|a| !a.taken && a.side == Some(side)) {
        let d = distance_squared(a.at, toward);
        if best.is_none_or(|(least, _)| d < least) {
            best = Some((d, *a));
        }
    }
    best.map_or(chosen, |(_, a)| a)
}

/// Whether an end names a shape and not a connection point of it.
fn is_automatic(end: &End) -> bool {
    matches!(end, End::Shape { point: None, .. })
}

/// The formula both cells of an end glued to point `index` of shape `target` hold.
fn glue_formula(target: u32, index: u32) -> String {
    let n = u64::from(index) + 1;
    format!("PAR(PNT(Sheet.{target}!Connections.X{n},Sheet.{target}!Connections.Y{n}))")
}

fn length(n: f64) -> Cell {
    Cell::constant(Value::from_text(&number_text(n))).with_unit("IN")
}

fn formula_cell(formula: &str, value: f64) -> Cell {
    Cell::with_formula(formula, Some(Value::from_text(&number_text(value)))).with_unit("IN")
}

/// The two cells of an end: the glue formula in both if glued, the place as constants if not.
fn end_cells(anchor: &Anchor) -> (Cell, Cell) {
    match anchor.glue {
        Some((target, index)) => {
            let formula = glue_formula(target, index);
            (formula_cell(&formula, anchor.at.0), formula_cell(&formula, anchor.at.1))
        }
        None => (length(anchor.at.0), length(anchor.at.1)),
    }
}

fn put(cells: &mut std::collections::BTreeMap<CellName, Cell>, name: &str, cell: Cell) {
    cells.insert(CellName::new(name), cell);
}

/// The cells that put a connector's frame between its ends (the formulas the specification prints as the ones written on creation),
/// each with the result it has for ends at `begin` and `end` on the page. A connector that was made here has them; Change Shape gives them to a
/// connector that inherited them, so that its frame does not depend on the master it is swapped for (`change.rs`).
pub(super) fn frame_cells(begin: P, end: P) -> [(&'static str, Cell); 7] {
    let mid = ((begin.0 + end.0) / 2.0, (begin.1 + end.1) / 2.0);
    let width = distance_squared(begin, end).sqrt();
    let angle = (end.1 - begin.1).atan2(end.0 - begin.0);
    [
        ("PinX", formula_cell("(BeginX+EndX)/2", mid.0)),
        ("PinY", formula_cell("(BeginY+EndY)/2", mid.1)),
        ("Width", formula_cell("SQRT((EndX-BeginX)^2+(EndY-BeginY)^2)", width)),
        ("Height", length(0.0)),
        ("LocPinX", formula_cell("Width*0.5", width / 2.0)),
        ("LocPinY", formula_cell("Height*0.5", 0.0)),
        ("Angle", Cell::with_formula("ATAN2(EndY-BeginY,EndX-BeginX)", Some(Value::from_text(&number_text(angle))))),
    ]
}

/// A new connector between two anchors, with the route for them.
fn connector_shape(id: u32, begin: &Anchor, end: &Anchor) -> Shape {
    let mut sheet = diacraft_doc::Sheet { links: Links { line: Some(0), fill: Some(0), text: Some(0) }, ..diacraft_doc::Sheet::default() };
    let cells = &mut sheet.cells;
    let (bx, by) = end_cells(begin);
    let (ex, ey) = end_cells(end);
    put(cells, "BeginX", bx);
    put(cells, "BeginY", by);
    put(cells, "EndX", ex);
    put(cells, "EndY", ey);
    let width = distance_squared(begin.at, end.at).sqrt();
    for (name, cell) in frame_cells(begin.at, end.at) {
        put(cells, name, cell);
    }
    // The label sits at the middle of the connector (the formulas).
    put(cells, "TxtPinX", formula_cell("Width*0.5", width / 2.0));
    put(cells, "TxtPinY", formula_cell("Height*0.5", 0.0));
    put(cells, "TxtLocPinX", formula_cell("TxtWidth*0.5", width / 2.0));
    put(cells, "TxtLocPinY", formula_cell("TxtHeight*0.5", 0.0));
    put(cells, "TxtWidth", formula_cell("Width*1", width));
    put(cells, "TxtHeight", formula_cell("Height*1", 0.0));
    // A solid arrowhead at the end (line end 4 of the numbered set) and nothing at the begin.
    put(cells, "EndArrow", Cell::constant(Value::from_text("4")));
    // The marker of a route that is ours to rewrite.
    let mut user = Section::new("User", None);
    let mut marker = Row { name: Some(Arc::from("DiaCraftRoute")), ..Row::default() };
    put(&mut marker.cells, "Value", Cell::constant(Value::from_text("1")));
    user.rows.push(Arc::new(marker));
    sheet.sections.push(Arc::new(user));
    let mut geometry = Section::new("Geometry", Some(0));
    route_flags(&mut geometry);
    sheet.sections.push(Arc::new(geometry));
    let name: Arc<str> = Arc::from(format!("Connector.{id}"));
    Shape { name: Some(Arc::clone(&name)), name_u: Some(name), kind: ShapeType::Shape, sheet: Arc::new(sheet), ..Shape::new(id) }
}

fn connect_row(from: u32, which: Which, to: u32, index: u32) -> Connect {
    Connect {
        from_sheet: from,
        from_cell: Some(Arc::from(which.x_cell())),
        from_part: Some(which.part_code()),
        to_sheet: to,
        to_cell: Some(Arc::from(format!("Connections.X{}", u64::from(index) + 1))),
        to_part: Some(TO_PART_BASE.saturating_add(index)),
        raw: diacraft_doc::RawAttrs::new(),
        ext: Default::default(),
    }
}

/// Makes the `Connect` rows of one end of a connector what `glue` says: none for a free end, one for a glued one.
fn set_glue(doc: &mut Document, page: u32, id: u32, which: Which, glue: Option<(u32, u32)>) -> Result<(), Problem> {
    let p = page_mut(doc, page).ok_or_else(|| Problem::failed("the page went away"))?;
    let is_this_end = |c: &Connect| c.from_sheet == id && matches!(c.from_cell.as_deref(), Some(n) if n == which.x_cell() || n == which.y_cell());
    let mut list: Vec<Connect> = p.connects.iter().filter(|c| !is_this_end(c)).cloned().collect();
    if let Some((target, index)) = glue {
        list.push(connect_row(id, which, target, index));
    }
    p.connects = Connects::from_vec(list);
    Ok(())
}

/// Adds a connector from `from` to `to` on a page, glued at each end that names a shape (the nearest connection points where none is
/// named, ours: the pair of points, one on each shape, with the least distance between them), and routes it.
pub fn add_connector(doc: &mut Document, page: u32, from: &End, to: &End) -> Result<ConnectorInfo, Problem> {
    add_connector_above(doc, page, from, to, 0)
}

/// [`add_connector`] with an `ID` above `above` as well as above every `ID` on the page ([`super::drop_shape_above`] says why).
pub fn add_connector_above(doc: &mut Document, page: u32, from: &End, to: &End, above: u32) -> Result<ConnectorInfo, Problem> {
    let before = doc.clone();
    let info = attach_connector_above(doc, page, from, to, above)?;
    settle(doc, &before, page, &[info.id], &[])?;
    Ok(info)
}

/// [`add_connector`] without the recalculation and the route: the connector is there, glued, with its cached values as they were worked
/// out for its ends and no route rows. For a caller that adds many and then calls [`settle`] once, with a copy of the document from before
/// the first.
pub fn attach_connector(doc: &mut Document, page: u32, from: &End, to: &End) -> Result<ConnectorInfo, Problem> {
    attach_connector_above(doc, page, from, to, 0)
}

/// [`attach_connector`] with an `ID` above `above` as well as above every `ID` on the page.
pub fn attach_connector_above(doc: &mut Document, page: u32, from: &End, to: &End, above: u32) -> Result<ConnectorInfo, Problem> {
    require_page(doc, page)?;
    let (begin, end) = {
        let reader = Reader::new(doc);
        let (a, b) = (candidates(&reader, page, from, None)?, candidates(&reader, page, to, None)?);
        let (begin, end) = nearest_pair(&a, &b).ok_or_else(|| Problem::failed("no pair of connection points to join"))?;
        let begin = spread(begin, &a, is_automatic(from), end.at);
        let end = spread(end, &b, is_automatic(to), begin.at);
        (begin, end)
    };
    let (highest, count) = page_stats(doc, page).ok_or_else(|| Problem::bad(format!("there is no page {page}")))?;
    if count >= diacraft_doc::MAX_SHAPES_PER_PAGE {
        return Err(Problem::failed(format!("a page holds at most {} shapes", diacraft_doc::MAX_SHAPES_PER_PAGE)));
    }
    let id = highest.max(above).checked_add(1).ok_or_else(|| Problem::failed("no shape ids are left on the page"))?;
    page_mut(doc, page).ok_or_else(|| Problem::failed("the page went away"))?.shapes.push(Arc::new(connector_shape(id, &begin, &end)));
    set_glue(doc, page, id, Which::Begin, begin.glue)?;
    set_glue(doc, page, id, Which::End, end.glue)?;
    Ok(ConnectorInfo { id, begin: begin.glue, end: end.glue })
}

/// Moves one end of a connector to `target`: glued to a shape, at the point named or the one nearest the other end, or free at a place.
pub fn reconnect_connector(doc: &mut Document, page: u32, id: u32, which: Which, target: &End) -> Result<ConnectorInfo, Problem> {
    let before = doc.clone();
    reconnect_unsettled(doc, page, id, which, target)?;
    settle(doc, &before, page, &[id], &[])?;
    Ok(info(doc, page, id))
}

/// [`reconnect_connector`] without the recalculation and the route: the end is glued where the shapes are as the cells hold them, and the caller settles once for all the
/// ends it moved (Change Shape glues the ends whose connection point the new master lacks).
pub(super) fn reconnect_unsettled(doc: &mut Document, page: u32, id: u32, which: Which, target: &End) -> Result<(), Problem> {
    let at = require_shape(doc, page, id)?;
    let sheet = SheetRef::Shape(at);
    let (anchor, hand_edited) = {
        let reader = Reader::new(doc);
        if !reader.is_one_d(&sheet) {
            return Err(Problem::bad(format!("shape {id} is not a connector")));
        }
        let hand_edited = (reader.number(&sheet, "ConFixedCode") - super::hand::HAND_EDITED).abs() < 0.5;
        let (other_x, other_y) = match which {
            Which::Begin => ("EndX", "EndY"),
            Which::End => ("BeginX", "BeginY"),
        };
        let other = (reader.number(&sheet, other_x), reader.number(&sheet, other_y));
        let near = [Anchor { glue: None, at: other, side: None, taken: false }];
        let among = candidates(&reader, page, target, Some(id))?;
        let (anchor, _) = nearest_pair(&among, &near).ok_or_else(|| Problem::failed("no connection point to glue to"))?;
        (spread(anchor, &among, is_automatic(target), other), hand_edited)
    };
    let (x_cell, y_cell) = end_cells(&anchor);
    let mut writes = vec![
        (sheet.clone(), CellAddr::direct(which.x_cell()), CellEdit::Cell(x_cell)),
        (sheet.clone(), CellAddr::direct(which.y_cell()), CellEdit::Cell(y_cell)),
    ];
    // A connector whose path was edited by hand and is glued to another shape takes the route of its new ends: the hand edit is undone.
    if hand_edited {
        writes.push((sheet, CellAddr::direct("ConFixedCode"), CellEdit::Value(Value::from_text("0"))));
    }
    write_cells(doc, writes)?;
    set_glue(doc, page, id, which, anchor.glue)
}

/// What [`reglue_all`] did: how many connectors were glued again, and how many were left as they are because a shape they go to has no connection points to glue to, its layers allow
/// no glue, or its points are past what the call reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Reglued {
    pub glued: usize,
    pub skipped: usize,
}

/// A connector to glue again by [`reglue_all`]: where it is, and the shapes its begin and its end go to.
pub(super) struct Reglue {
    pub id: u32,
    pub at: ShapeRef,
    pub from: u32,
    pub to: u32,
    /// The side of the begin's shape the connector leaves by, and of the end's shape it comes in by, when the layout says so (an org chart's: out of the manager along the flow, into the
    /// report from the side it is set off). Taken only if both shapes have a connection point facing that way: else the pair is the nearest.
    pub sides: Option<(Dir, Dir)>,
}

/// The points of the connection points that connectors hold, kept so that taking one connector's points and giving it others touches only its own (at most two) and not every shape's:
/// how many connectors hold each point of each shape, and which points each connector holds. Built by one pass over the page's `Connect` rows.
#[derive(Default)]
struct Held {
    count: HashMap<(u32, u32), u32>,
    by_connector: HashMap<u32, Vec<(u32, u32)>>,
}

impl Held {
    fn of_rows(doc: &Document, page: u32) -> Held {
        let mut held = Held::default();
        if let Some(p) = doc.page(page) {
            for c in p.connects.iter() {
                if let Some(index) = c.to_part.and_then(|part| part.checked_sub(TO_PART_BASE)) {
                    held.add(c.from_sheet, c.to_sheet, index);
                }
            }
        }
        held
    }

    fn add(&mut self, connector: u32, shape: u32, index: u32) {
        *self.count.entry((shape, index)).or_insert(0) += 1;
        self.by_connector.entry(connector).or_default().push((shape, index));
    }

    /// Takes every point `connector` holds off.
    fn release(&mut self, connector: u32) {
        for key in self.by_connector.remove(&connector).unwrap_or_default() {
            if let Some(n) = self.count.get_mut(&key) {
                *n = n.saturating_sub(1);
            }
        }
    }

    /// Whether a connector other than `connector` holds point `index` of `shape`.
    fn held_by_another(&self, shape: u32, index: u32, connector: u32) -> bool {
        let own = self.by_connector.get(&connector).map_or(0, |held| held.iter().filter(|key| **key == (shape, index)).count());
        self.count.get(&(shape, index)).is_some_and(|n| (*n as usize) > own)
    }
}

/// The anchors of a shape's connection points for [`reglue_all`], made once for the call: `None` for a shape that has none to glue to, whose layers allow no glue, or whose points are
/// past what the call reads.
fn reglue_anchors(
    reader: &Reader<'_>,
    page: u32,
    id: u32,
    at: &ShapeRef,
    points: &mut GluedPoints,
    made: &mut HashMap<u32, Option<Vec<Anchor>>>,
) -> Option<Vec<Anchor>> {
    if let Some(done) = made.get(&id) {
        return done.clone();
    }
    let barred = reader.doc().shape(at).is_some_and(|shape| super::layers::glue_barred(reader.doc(), page, shape).is_some());
    let anchors: Option<Vec<Anchor>> = if barred {
        None
    } else {
        let read = points.points(reader, id, at);
        (!read.is_empty()).then(|| {
            read.iter()
                .take(MAX_CANDIDATES)
                .map(|p| Anchor { glue: Some((id, p.index)), at: (p.x, p.y), side: Dir::from_vector(p.dir_x, p.dir_y), taken: false })
                .collect()
        })
    };
    made.insert(id, anchors.clone());
    anchors
}

/// Glues both ends of each connector of `plans` again, the begin to its shape `from` and the end to its shape `to`, at the pair of connection points that face each other (
/// as a new connector between them is glued: a point another connector holds is given up for a free one on the same side). For a layout that moved the shapes
/// The cells of the shapes are the places they have now, and the caller settles once for all. A hand edit of a route is undone. → how many were glued again, and how many
/// were left as they are because a shape they go to has no connection point to glue to, its layers allow no glue, or its points are past what the call reads.
///
/// **What it costs** (counted, `relayout/glue_cost.rs`). One walk of the page finds the shapes. **The points of each shape are read once for the call**, within
/// [`MAX_CROSSING_POINT_ROWS`] rows for all of them ([`GluedPoints`], as the routing reads them), and a shape past that is skipped and counted, not read. The points the connectors hold
/// are indexed by shape and by connector ([`Held`]), so giving a connector other points touches its own and nothing else. One rebuild of the rows and one batch of cell writes put the
/// result in.
pub(super) fn reglue_all(doc: &mut Document, page: u32, plans: &[Reglue]) -> Result<Reglued, Problem> {
    if plans.is_empty() {
        return Ok(Reglued::default());
    }
    let ids: HashSet<u32> = plans.iter().flat_map(|p| [p.from, p.to]).collect();
    let found = super::scan::scan(doc, page, &ids)?.found;
    let mut held = Held::of_rows(doc, page);
    let mut out = Reglued::default();
    let mut chosen: Vec<(&Reglue, Anchor, Anchor, bool)> = Vec::new();
    {
        let reader = Reader::new(doc);
        let mut points = GluedPoints::new(MAX_CROSSING_POINT_ROWS);
        let mut made: HashMap<u32, Option<Vec<Anchor>>> = HashMap::new();
        for plan in plans {
            let (Some(from_at), Some(to_at)) = (found.get(&plan.from), found.get(&plan.to)) else { continue };
            let a = reglue_anchors(&reader, page, plan.from, from_at, &mut points, &mut made);
            let b = reglue_anchors(&reader, page, plan.to, to_at, &mut points, &mut made);
            let (Some(mut a), Some(mut b)) = (a, b) else {
                out.skipped += 1;
                continue;
            };
            // The connector's own points do not count as held while it picks, and the others' do.
            for anchor in a.iter_mut().chain(b.iter_mut()) {
                anchor.taken = anchor.glue.is_some_and(|(shape, index)| held.held_by_another(shape, index, plan.id));
            }
            // The sides a layout asked for, if both shapes have a point that faces so.
            if let Some((out, into)) = plan.sides {
                let (fa, fb): (Vec<Anchor>, Vec<Anchor>) =
                    (a.iter().filter(|x| x.side == Some(out)).copied().collect(), b.iter().filter(|x| x.side == Some(into)).copied().collect());
                if !fa.is_empty() && !fb.is_empty() {
                    (a, b) = (fa, fb);
                }
            }
            let Some((begin, end)) = nearest_pair(&a, &b) else { continue };
            let (begin, end) = (spread(begin, &a, true, end.at), spread(end, &b, true, begin.at));
            let hand_edited = (reader.number(&SheetRef::Shape(plan.at.clone()), "ConFixedCode") - super::hand::HAND_EDITED).abs() < 0.5;
            held.release(plan.id);
            for anchor in [begin, end] {
                if let Some((shape, index)) = anchor.glue {
                    held.add(plan.id, shape, index);
                }
            }
            chosen.push((plan, begin, end, hand_edited));
        }
    }
    let mut writes: Vec<(SheetRef, CellAddr, CellEdit)> = Vec::new();
    for (plan, begin, end, hand_edited) in &chosen {
        let sheet = SheetRef::Shape(plan.at.clone());
        let ((bx, by), (ex, ey)) = (end_cells(begin), end_cells(end));
        writes.push((sheet.clone(), CellAddr::direct(Which::Begin.x_cell()), CellEdit::Cell(bx)));
        writes.push((sheet.clone(), CellAddr::direct(Which::Begin.y_cell()), CellEdit::Cell(by)));
        writes.push((sheet.clone(), CellAddr::direct(Which::End.x_cell()), CellEdit::Cell(ex)));
        writes.push((sheet.clone(), CellAddr::direct(Which::End.y_cell()), CellEdit::Cell(ey)));
        if *hand_edited {
            writes.push((sheet, CellAddr::direct("ConFixedCode"), CellEdit::Value(Value::from_text("0"))));
        }
    }
    if writes.is_empty() {
        return Ok(out);
    }
    write_cells(doc, writes)?;
    // The rows of the ends glued again: the old ones go, the new ones follow the rest.
    let again: HashSet<u32> = chosen.iter().map(|(plan, ..)| plan.id).collect();
    let is_end = |c: &Connect| again.contains(&c.from_sheet) && matches!(c.from_cell.as_deref(), Some("BeginX" | "BeginY" | "EndX" | "EndY"));
    let p = page_mut(doc, page).ok_or_else(|| Problem::failed("the page went away"))?;
    let mut list: Vec<Connect> = p.connects.iter().filter(|c| !is_end(c)).cloned().collect();
    for (plan, begin, end, _) in &chosen {
        for (which, anchor) in [(Which::Begin, begin), (Which::End, end)] {
            if let Some((target, index)) = anchor.glue {
                list.push(connect_row(plan.id, which, target, index));
            }
        }
    }
    p.connects = Connects::from_vec(list);
    out.glued = chosen.len();
    Ok(out)
}

/// Lets go of one end of connector `id`: it stays where it is, as a free end (the two cells hold the place as constants), and the `Connect` rows of the end are gone.
/// For a caller that settles once for all the ends it moved.
pub(super) fn release_end(doc: &mut Document, page: u32, id: u32, which: Which) -> Result<(), Problem> {
    let at = require_shape(doc, page, id)?;
    let sheet = SheetRef::Shape(at);
    let place = {
        let reader = Reader::new(doc);
        (reader.number(&sheet, which.x_cell()), reader.number(&sheet, which.y_cell()))
    };
    let writes = vec![
        (sheet.clone(), CellAddr::direct(which.x_cell()), CellEdit::Cell(length(checked("x", place.0)?))),
        (sheet, CellAddr::direct(which.y_cell()), CellEdit::Cell(length(checked("y", place.1)?))),
    ];
    write_cells(doc, writes)?;
    set_glue(doc, page, id, which, None)
}

/// Where the ends of connector `id` are glued, from the page's `Connect` rows.
fn info(doc: &Document, page: u32, id: u32) -> ConnectorInfo {
    let glue_of = |which: Which| {
        doc.page(page).and_then(|p| {
            p.connects.iter().find(|c| c.from_sheet == id && c.from_cell.as_deref() == Some(which.x_cell())).and_then(|c| {
                let index = c.to_part?.checked_sub(TO_PART_BASE)?;
                Some((c.to_sheet, index))
            })
        })
    };
    ConnectorInfo { id, begin: glue_of(Which::Begin), end: glue_of(Which::End) }
}

/// What a route needs to know of one end of a connector: the way it leaves, and the box of the shape it is glued to (in the frame of the
/// connector's parent, where the end cells are).
pub(super) struct EndFacts {
    pub(super) dir: Option<Dir>,
    pub(super) quad: Option<route_clear::Quad>,
}

/// A route worked out for connector `id` and not written yet ([`plan_route`]): where the connector is, its new rows and the rows its master gives it, the route
/// on the page with the connector's pose (for its label), and the sheet whose `ConFixedCode` 3 is taken back to 0.
pub(crate) struct Planned {
    pub(crate) id: u32,
    pub(crate) at: ShapeRef,
    rows: Vec<Arc<Row>>,
    inherited: Option<Vec<u32>>,
    pub(crate) routed: Routed,
    reset_code: Option<SheetRef>,
}

/// Works out the route of connector `id`, at `at`, from where its ends are now: the `Geometry` section 0 rows it is to have (the caller has decided that it is to be
/// routed, [`super::reroute`]), **written by [`write_planned`]**, so that the routes of every connector an edit moved are worked out on one reading of the page
/// (a route reads the connector, the page's cells and the shapes its ends are glued to, which the routes of the others do not write). The glue of the
/// ends, the shapes they are glued to and those shapes' points come from `index` and `located`, read once for the call. An end whose glue did not work out (an `E` on
/// the cell) is an error for a connector of ours, which the edit that caused it takes back, and leaves a connector of a file alone (`None`), whose saved cells are not
/// ours to refuse an edit for. A connector of ours that holds `ConFixedCode` 3 is taken back to 0 (a route made again is no longer as a hand edit left it)
/// unless `keep_setting`: the route was made because a leg crossed a shape it is glued to, which is what 3 as a setting, on crossover, says to do.
pub(crate) fn plan_route(
    reader: &Reader<'_>,
    page: u32,
    id: u32,
    at: &ShapeRef,
    keep_setting: bool,
    index: &mut super::reroute::EndIndex,
    located: &super::scan::Located,
) -> Result<Option<Planned>, Problem> {
    let sheet = SheetRef::Shape(at.clone());
    if ["BeginX", "BeginY", "EndX", "EndY", "PinX", "Width", "Angle"].iter().any(|name| has_error(reader, &sheet, name)) {
        return if reader.is_routed(&sheet) { Err(Problem::failed(format!("the ends of connector {id} could not be worked out"))) } else { Ok(None) };
    }
    let pose = reader.pose(&sheet);
    let (begin, end) =
        ((reader.number(&sheet, "BeginX"), reader.number(&sheet, "BeginY")), (reader.number(&sheet, "EndX"), reader.number(&sheet, "EndY")));
    let style = style::path_style(reader, page, &sheet);
    // A right-angle route, and the polygon a curve is drawn through, leave each end the way its point faces and go round the shapes the ends are
    // glued to (rule 7); a straight one is the two ends joined.
    let points = match style {
        PathStyle::Straight | PathStyle::CentreToCentre => vec![begin, end],
        PathStyle::RightAngle | PathStyle::Curved => {
            let (from, to) = index.end_facts(reader, located, at, id, begin, end);
            route_clear::route_clear(begin, from.dir, end, to.dir, from.quad.as_ref(), to.quad.as_ref())
        }
    };
    let local: Vec<P> = points.iter().map(|p| pose.down(*p)).collect();
    if !local.iter().all(|p| p.0.is_finite() && p.1.is_finite()) {
        return Ok(None);
    }
    let segs = if style == PathStyle::Curved { rows::curve(&local) } else { rows::polyline(&local) };
    let ours = reader.is_routed(&sheet);
    // A connector of ours that a command routes again is no longer as a hand edit left it.
    let hand_edited = ours && !keep_setting && (reader.number(&sheet, "ConFixedCode") - super::hand::HAND_EDITED).abs() < 0.5;
    Ok(Some(Planned {
        id,
        at: at.clone(),
        rows: rows::rows(&segs, false),
        inherited: inherited_rows(reader, at),
        routed: Routed { points, pose, ours },
        reset_code: hand_edited.then_some(sheet),
    }))
}

/// Writes the routes [`plan_route`] worked out: each connector's `Geometry` section 0 rows, and one batch of the `ConFixedCode` cells taken back to 0 (a cell a `GUARD`
/// protects is an error, as for any drag).
pub(crate) fn write_planned(doc: &mut Document, page: u32, plans: &mut [Planned]) -> Result<(), Problem> {
    let mut resets = Vec::new();
    for plan in plans.iter_mut() {
        write_route(doc, page, &plan.at.path, std::mem::take(&mut plan.rows), &plan.inherited)?;
        if let Some(sheet) = plan.reset_code.take() {
            resets.push((sheet, CellAddr::direct("ConFixedCode"), CellEdit::Value(Value::from_text("0"))));
        }
    }
    write_cells(doc, resets)
}

/// A route written: its points on the page, and the pose of the connector that carries it.
pub(crate) struct Routed {
    pub points: Vec<P>,
    pub pose: Pose,
    /// Whether the connector is one DiaCraft made (it has `User.DiaCraftRoute`), as against one a file holds.
    pub ours: bool,
}

/// The `IX` of the `Geometry` section 0 rows a connector gets from its master: a route with fewer rows has to take the others away, or they
/// would stay in the merge. `None` when the connector's master has no such section.
pub(super) fn inherited_rows(reader: &Reader<'_>, at: &ShapeRef) -> Option<Vec<u32>> {
    let step = reader.resolver().master_step(at).ok().flatten()?;
    let rows = reader.resolver().effective_rows(&SheetRef::Shape(step.shape), &SectionKey::new("Geometry", Some(0)))?;
    Some(rows.iter().filter_map(|row| if let RowKey::Ix(ix) = row.key { Some(ix) } else { None }).collect())
}

/// The flags of a `Geometry` section that is a route's own: open and unfilled.
fn route_flags(section: &mut Section) {
    for (flag, value) in [("NoFill", "1"), ("NoLine", "0"), ("NoShow", "0"), ("NoSnap", "0"), ("NoQuickDrag", "0")] {
        put(&mut section.cells, flag, Cell::constant(Value::from_text(value)));
    }
}

/// The rows a route's section holds: the rows, and a deleted row for each row the master gives the connector that the route does not use, so that
/// it does not stay in the merge.
pub(super) fn route_section_rows(rows: Vec<Arc<Row>>, inherited: &Option<Vec<u32>>) -> Vec<Arc<Row>> {
    let count = rows.len();
    let mut out = rows;
    for ix in inherited.iter().flatten() {
        if usize::try_from(*ix).map_or(true, |n| n == 0 || n > count) {
            out.push(Arc::new(Row { ix: Some(*ix), del: true, ..Row::default() }));
        }
    }
    out
}

/// Puts the route in the connector's `Geometry` section 0: the rows are replaced, and the section's own cells (its flags) are kept as the connector
/// has them, or inherits them. A section nobody holds is made, with the flags of an open, unfilled line. The rows the master gives the connector
/// and the route does not use are deleted here, so that they do not stay in the merge.
pub(super) fn write_route(doc: &mut Document, page: u32, path: &[u32], rows: Vec<Arc<Row>>, inherited: &Option<Vec<u32>>) -> Result<(), Problem> {
    let shape = shape_mut(doc, page, path).ok_or_else(|| Problem::failed("the connector went away"))?;
    let sheet = Arc::make_mut(&mut shape.sheet);
    let at = sheet.sections.iter().position(|s| s.name.as_str() == "Geometry" && s.ix == Some(0));
    // The section held, but for its rows, which the route replaces: a copy of them would be a walk of each (ten thousand connectors sharing a route of
    // 4,000 rows, routed again, copied and let go of forty million pointers).
    let mut section = match at.and_then(|i| sheet.sections.get(i)) {
        Some(held) => Section {
            name: held.name.clone(),
            ix: held.ix,
            del: held.del,
            cells: held.cells.clone(),
            rows: Rows::new(),
            raw: held.raw.clone(),
            passthrough: held.passthrough.clone(),
            ext: held.ext.clone(),
        },
        None => Section::new("Geometry", Some(0)),
    };
    if at.is_none() && inherited.is_none() {
        route_flags(&mut section);
    }
    section.rows = Rows::from(route_section_rows(rows, inherited));
    let section = Arc::new(section);
    match at.and_then(|i| sheet.sections.get_mut(i)) {
        Some(slot) => *slot = section,
        None => sheet.sections.push(section),
    }
    Ok(())
}
