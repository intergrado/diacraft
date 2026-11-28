//! Connectors and glue: `draw:connector`'s ends, the glue formulas
//! and `Connect` rows of an attached end, the route as `Geometry`, and a shape's own `draw:glue-point` elements as `Connection` rows.
//!
//! **What an end is.** An end names a shape by `draw:start-shape` or `draw:end-shape` (the shape's `xml:id`, else its `draw:id`) and may name
//! one of that shape's glue points by number. An end that names a shape the page does not model is free, with a warning. A named glue point
//! is point glue (`ToPart` 100 plus the row, `Connections` in the end cells' formulas, as `connector.add` writes it), and a named shape with
//! no point is whole-shape glue (`ToPart` 3, plain values placed later). The glue id is turned into a row by the fixed map of
//! R38: 0 to row 0, 1 to row 3, 2 to row 2, 3 to row 1, and any other id to the row of its own number; on a shape with exactly one flip whose standard points
//! LibreOffice numbers on the frame it draws, 1 and 3 are rows 1 and 3 ([`glue_row_beside`]). A `draw:glue-point` given as lengths is placed
//! in the page's axes, from the box the shape takes up ([`Place`]).
//!
//! **Cost.** An end is one lookup in the page's map of ids, whatever the number of connectors that name one shape or one point, and a
//! connector glued to a connector is a connector glued to a shape: the glue is a name and a number, and nothing follows a chain of them. A
//! `Connect` row is charged as an item before it is kept. The route is the path data's commands, charged to the page's budget, and a
//! glue point's row is charged as a row.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Connect, RawAttrs, Row, Section};

use super::Reader;
use super::cells::{self, Forms};
use super::geometry::{self, Budget, ViewBox};
use super::names::{Attrs, Names, Ns};
use super::shapes::{Units, row};
use super::units::{self};
use crate::error::OdgError;

/// The ids of glue points that name a row: below the rows a section holds. A larger id names no row.
const MOST_ID: usize = diacraft_doc::MAX_ROWS_PER_SECTION;

/// The row of the `Connection` section a glue id is: the four standard points in the order of DiaCraft's basic
/// shapes (top, left, bottom, right), which ODF numbers top, right, bottom, left; any other id is its own row.
pub(crate) fn glue_row(id: u32) -> u32 {
    match id {
        0 => 0,
        1 => 3,
        2 => 2,
        3 => 1,
        n => n,
    }
}

/// The row a glue id is on a shape whose standard glue points are numbered across its frame the other way round (`flipped`: a shape with exactly one flip,
/// `read::shapes::glue_flipped`): LibreOffice's points 1 and 3 are the right and left sides' middles of the frame as it draws it, which beside one flip are the
/// model's rows 1 and 3 (its own left and right, mirrored). Every other id, and every id of a shape that is not `flipped`, is [`glue_row`]'s. Its own inverse
/// with [`glue_row`]'s: the writer uses it both ways.
pub(crate) fn glue_row_beside(id: u32, flipped: bool) -> u32 {
    match id {
        1 | 3 if flipped => id,
        n => glue_row(n),
    }
}

/// The formula both cells of an end glued to row `row` of shape `target` hold (what `connector.add` writes).
pub(crate) fn glue_formula(target: u32, row: u32) -> String {
    let n = u64::from(row) + 1;
    format!("PAR(PNT(Sheet.{target}!Connections.X{n},Sheet.{target}!Connections.Y{n}))")
}

/// What an end is attached to: the shape's `ID` and, for point glue, the `Connection` row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Glue {
    pub target: u32,
    pub row: Option<u32>,
}

/// Which end of a connector.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Which {
    Begin,
    End,
}

/// What the page knows of its shapes when a connector is read: the `ID` of each shape that has an `xml:id` or a `draw:id`, for the
/// shapes it models.
#[derive(Default)]
pub(crate) struct Ids<'t> {
    pub by_name: HashMap<&'t str, u32>,
    /// The shapes whose standard glue points 1 and 3 are rows 1 and 3 ([`glue_row_beside`]).
    pub flipped: HashSet<u32>,
    /// The shapes whose rows start at 0 with their first listed glue point (`diacraft:glue="own"`): glue id 4 + *r* is row *r*, and a standard point is
    /// no row of theirs.
    pub own: HashSet<u32>,
}

/// The row a glue id names on a shape whose rows start at 0 with its first listed point (`diacraft:glue="own"`): id 4 + *r* is row *r*; a standard
/// point (0 to 3) is no row. The writer names row *r* by id *r* + 4, the inverse.
pub(crate) fn own_glue_row(id: u32) -> Option<u32> {
    id.checked_sub(4)
}

/// The end `which` of a connector: what `draw:start-shape` (or `end`) and the glue point it names attach it to. The attributes are taken.
/// A shape the page does not model, or an end with an empty name, is free (a warning for a name nothing has).
pub(crate) fn end_of(r: &mut Reader, attrs: &mut Attrs<'_>, which: Which, ids: &Ids<'_>) -> Option<Glue> {
    let (shape, point) = match which {
        Which::Begin => ("start-shape", "start-glue-point"),
        Which::End => ("end-shape", "end-glue-point"),
    };
    let name = attrs.take(Ns::Draw, shape);
    let point = attrs.take(Ns::Draw, point);
    let name = name.filter(|n| !n.is_empty())?;
    // One lookup in the page's map, hashing the end's own attribute (a name of the connector's, not a style's).
    r.report.work.name_lookups = r.report.work.name_lookups.saturating_add(1);
    r.name_bytes(name.len());
    let Some(target) = ids.by_name.get(name).copied() else {
        r.warn("glue_target_missing", || "a connector names a shape that is not on the page; that end is free".to_owned());
        return None;
    };
    // A glue point is a row of a `Connection` section, and no section holds more rows than `MAX_ROWS_PER_SECTION`: an id past that names no
    // point, and `ToPart` (100 plus the row) would saturate at the largest number. The end is glued to the whole shape instead, with a warning.
    let id = point.and_then(units::integer).and_then(|n| u32::try_from(n).ok());
    let row = match id {
        // A frame's point past the four standard ones is its own `draw:glue-point`, a row like any shape's ([`glue_points`]).
        Some(n) if (n as usize) < MOST_ID && ids.own.contains(&target) => match own_glue_row(n) {
            Some(row) => Some(row),
            None => {
                r.warn("glue_point_id", || {
                    "a connector names a standard glue point of a shape whose connection points DiaCraft wrote as its own; that end is glued to the whole shape".to_owned()
                });
                None
            }
        },
        Some(n) if (n as usize) < MOST_ID => Some(glue_row_beside(n, ids.flipped.contains(&target))),
        Some(_) => {
            r.warn("glue_point_id", || {
                "a connector names a glue point past the rows a shape can hold; that end is glued to the whole shape".to_owned()
            });
            None
        }
        None => None,
    };
    Some(Glue { target, row })
}

/// The route cells a connector's `draw:type` gives: `ShapeRouteStyle` and `ConLineRouteExt`. The attribute is taken.
pub(crate) fn route_cells(attrs: &mut Attrs<'_>) -> [(&'static str, Cell); 2] {
    let (style, ext) = match attrs.take(Ns::Draw, "type") {
        None | Some("standard") => (1.0, 1.0),
        Some("line" | "lines") => (2.0, 1.0),
        Some("curve") => (1.0, 2.0),
        // A type the standard does not define reads as the default.
        Some(_) => (1.0, 1.0),
    };
    [("ShapeRouteStyle", cells::plain(style)), ("ConLineRouteExt", cells::plain(ext))]
}

/// A `Connect` row of an attached end: the connector `from`, the end `which`, the shape and the glue.
pub(crate) fn connect(from: u32, which: Which, glue: Glue) -> Connect {
    let (cell, part) = match which {
        Which::Begin => ("BeginX", 9),
        Which::End => ("EndX", 12),
    };
    let (to_cell, to_part) = match glue.row {
        Some(row) => (format!("Connections.X{}", u64::from(row) + 1), 100u32.saturating_add(row)),
        None => ("PinX".to_owned(), 3),
    };
    Connect {
        from_sheet: from,
        from_cell: Some(Arc::from(cell)),
        from_part: Some(part),
        to_sheet: glue.target,
        to_cell: Some(Arc::from(to_cell)),
        to_part: Some(to_part),
        raw: RawAttrs::new(),
        ext: Default::default(),
    }
}

/// The marker section `User.DiaCraftRoute`: a route DiaCraft may route again.
pub(crate) fn route_marker() -> Section {
    let mut user = Section::new("User", None);
    let mut marker = Row { name: Some(Arc::from("DiaCraftRoute")), ..Row::default() };
    marker.cells.insert(CellName::new("Value"), cells::plain(1.0));
    user.rows.push(Arc::new(marker));
    user
}

/// The connector's own frame: its begin and end on the page (inches, y up), and the frame they make (the
/// begin is the frame's origin and the x axis runs to the end).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Line {
    pub begin: (f64, f64),
    pub end: (f64, f64),
    pub unit: Option<&'static str>,
}

impl Line {
    fn angle(&self) -> f64 {
        let a = (self.end.1 - self.begin.1).atan2(self.end.0 - self.begin.0);
        if a.is_finite() { a } else { 0.0 }
    }

    /// A point of the page (inches, y up) in the frame.
    fn to_frame(self, p: (f64, f64)) -> (f64, f64) {
        let (s, c) = self.angle().sin_cos();
        let (dx, dy) = (p.0 - self.begin.0, p.1 - self.begin.1);
        (dx * c + dy * s, -dx * s + dy * c)
    }
}

/// A coordinate of `svg:d`: hundredths of a millimetre.
const HUNDREDTHS_PER_INCH: f64 = 2540.0;

/// The `Geometry` section through the points of a connector's `svg:d`: `MoveTo` at the
/// start, `LineTo` for each straight segment, a `NURBSTo` of degree 3 for each cubic one (a quadratic is raised to a cubic, an arc is its
/// chord), as lengths in the connector's own frame, with the view box ignored. `None` when the connector has no readable path: the caller
/// gives the route DiaCraft's router makes, which this reader does not have, as the straight segment of a line.
pub(crate) fn route(r: &mut Reader, attrs: &mut Attrs<'_>, line: &Line, page_height: f64, budget: &mut Budget) -> Result<Option<Section>, OdgError> {
    let Some(text) = attrs.peek(Ns::Svg, "d") else { return Ok(None) };
    // The path is read by the shapes' parser in a unit box, so a share is the coordinate itself and `1 - y` the other.
    let unit_box = ViewBox { left: 0.0, top: 0.0, width: 1.0, height: 1.0 };
    let parsed = match geometry::path(text, &unit_box, (1.0, 1.0), budget)? {
        Ok(parsed) => parsed,
        Err(_) => {
            r.warn("geometry_unreadable", || "a connector's path data breaks the syntax; its route is a straight segment".to_owned());
            return Ok(None);
        }
    };
    attrs.take(Ns::Svg, "d");
    attrs.take(Ns::Svg, "viewBox");
    let unit = line.unit;
    let to_page = |sx: f64, sy: f64| (sx / HUNDREDTHS_PER_INCH, page_height - (1.0 - sy) / HUNDREDTHS_PER_INCH);
    let mut section = Section::new("Geometry", Some(0));
    let mut pen = (0.0, 0.0);
    let mut count = 0usize;
    for sub in parsed {
        for g in sub.rows {
            count += 1;
            if count > diacraft_doc::MAX_ROWS_PER_SECTION {
                return Err(OdgError::TooComplex { what: "rows in one subpath", max: diacraft_doc::MAX_ROWS_PER_SECTION as u64 });
            }
            let ix = u32::try_from(count).unwrap_or(u32::MAX);
            let frame = |sx: f64, sy: f64| line.to_frame(to_page(sx, sy));
            let (made, end) = match g {
                geometry::Row::RelMoveTo(sx, sy) | geometry::Row::RelLineTo(sx, sy) | geometry::Row::RelEllipticalArcTo { x: sx, y: sy, .. } => {
                    let ty = if matches!(g, geometry::Row::RelMoveTo(..)) { "MoveTo" } else { "LineTo" };
                    let (u, v) = frame(sx, sy);
                    (row(ix, Some(ty), vec![("X", cells::length(u, unit)), ("Y", cells::length(v, unit))]), (sx, sy))
                }
                geometry::Row::RelCubBezTo { x, y, a, b, c, d } => (nurbs(r, ix, unit, frame(x, y), frame(a, b), frame(c, d)), (x, y)),
                geometry::Row::RelQuadBezTo { x, y, a, b } => {
                    // A quadratic with control q from p0 to p is the cubic with controls p0 + 2/3 (q - p0) and p + 2/3 (q - p).
                    let (q, p) = ((a, b), (x, y));
                    let c1 = (pen.0 + 2.0 / 3.0 * (q.0 - pen.0), pen.1 + 2.0 / 3.0 * (q.1 - pen.1));
                    let c2 = (p.0 + 2.0 / 3.0 * (q.0 - p.0), p.1 + 2.0 / 3.0 * (q.1 - p.1));
                    (nurbs(r, ix, unit, frame(p.0, p.1), frame(c1.0, c1.1), frame(c2.0, c2.1)), p)
                }
            };
            r.take_items(super::cost::ROW)?;
            r.take_cells(made.cells.len())?;
            section.rows.push(made);
            pen = end;
        }
    }
    r.take_items(super::cost::SECTION)?;
    Ok(Some(section))
}

/// A `NURBSTo` of degree 3 holding one cubic exactly: `A` 0, `B` 1, `C` 0, `D` 1 and `E` the constructor
/// with the two control points as lengths.
fn nurbs(r: &mut Reader, ix: u32, unit: Option<&'static str>, end: (f64, f64), c1: (f64, f64), c2: (f64, f64)) -> Arc<Row> {
    let n = |v: f64| cells::number(v).text().to_owned();
    let text = format!("NURBS(1,3,1,1,{},{},0,1,{},{},0,1)", n(c1.0), n(c1.1), n(c2.0), n(c2.1));
    let mut e = r.forms.text_cell(&text, &text);
    e.unit = Some(Arc::from("NURBS"));
    row(
        ix,
        Some("NURBSTo"),
        vec![
            ("X", cells::length(end.0, unit)),
            ("Y", cells::length(end.1, unit)),
            ("A", cells::plain(0.0)),
            ("B", cells::plain(1.0)),
            ("C", cells::plain(0.0)),
            ("D", cells::plain(1.0)),
            ("E", e),
        ],
    )
}

/// A shape's `draw:glue-point` children as `Connection` rows: the row `IX` is the element's `draw:id`, its
/// position by percentage, by length from the centre (LibreOffice's own form), or by `draw:align`, and its escape direction. A point
/// whose id is 0 to 3 (the standard points'), has none or is past the rows a section holds, or that repeats an earlier element's, or
/// takes the id of one of the `listed` points the shape's own `draw:glue-points` give (rows 4 on), is not made a row (a warning): the rows of
/// a `Connection` section have one `IX` each.
///
/// **A frame's points are rows too** (a `draw:frame`, holding a text box, a picture or an object): a connector end glued to a glue point of a
/// shape follows that point when the shape moves, whatever kind of point it is, so a frame's `draw:glue-point` is a row, placed as a custom shape's is (`place`'s
/// [`Boxed::Frame`]: LibreOffice 26.8.2.2 ends a connector named to one there, turned 0 and 30 degrees, text frames and picture frames alike, every end within 0.001 cm), and
/// an end that names it is glued to it. **Whatever the order of the frame's children**: LibreOffice drops a frame's `draw:glue-point` that comes before the text box or the
/// picture (the standard puts it after them) and glues a connector that names one to the frame as a whole, which measured and took for every frame, making such
/// an element no row and such an end whole-shape glue, which nothing moved with the frame (a 0.25 inch move left 16 ends of three files 0.25 inch behind).
pub(crate) fn glue_points<'t>(
    r: &mut Reader,
    names: &mut Names<'t>,
    el: &'t diacraft_container::xml::El,
    size: (f64, f64),
    u: Units,
    listed: usize,
    place: Place,
) -> Result<Vec<Arc<Row>>, OdgError> {
    let mut found: Vec<(u32, Arc<Row>)> = Vec::new();
    let mut seen: HashSet<u32> = HashSet::new();
    for child in el.elements() {
        let mark = names.enter(child);
        if names.is(child, Ns::Draw, "glue-point") {
            let attrs = names.attrs(child);
            r.saw_attrs(attrs.len());
            if found.len() >= diacraft_doc::MAX_ROWS_PER_SECTION.saturating_sub(4) {
                names.leave(mark);
                return Err(OdgError::TooComplex { what: "glue points in one shape", max: diacraft_doc::MAX_ROWS_PER_SECTION as u64 });
            }
            match attrs.peek(Ns::Draw, "id").and_then(units::integer).and_then(|n| u32::try_from(n).ok()) {
                Some(id) if id >= 4 && (id as usize) < MOST_ID => {
                    if (id as usize) < 4usize.saturating_add(listed) {
                        r.warn("glue_point_id", || "a draw:glue-point has the id of a listed glue point; it is kept and makes no row".to_owned());
                    } else if seen.insert(id) {
                        let made = glue_point_row(r, &attrs, id, size, u, place)?;
                        found.push((id, made));
                    } else {
                        r.warn("glue_point_repeated", || "a draw:glue-point repeats an earlier one's id; it is kept and makes no row".to_owned());
                    }
                }
                _ => r.warn("glue_point_id", || {
                    "a draw:glue-point has no usable id (4 up to the rows a section holds); it is kept and makes no row".to_owned()
                }),
            }
        }
        names.leave(mark);
    }
    found.sort_by_key(|(id, _)| *id);
    Ok(found.into_iter().map(|(_, row)| row).collect())
}

/// A glue point's value: a percentage (of the frame, from its centre), or a length.
enum Pos {
    Percent(f64),
    Length(units::Length),
}

fn pos(text: Option<&str>) -> Option<Pos> {
    let text = text?;
    units::percent(text).map(Pos::Percent).or_else(|| units::length(text).map(Pos::Length))
}

/// The box a shape's `draw:glue-point` lengths are measured in, and how the model's frame stands to the page.
///
/// **LibreOffice 26.8.2.2 places such a point in the page's axes, from the box the shape takes up on the page, and turns and mirrors it with nothing**: a point
/// written as lengths with no `draw:align` (LibreOffice's own form, shares of the box from its centre) and one with `draw:align` (a length from the box's corner or side)
/// are where they would be on the shape's box, whatever the turn of `draw:transform`, its own flip and the mirrors of a custom shape's geometry (measured on custom
/// shapes in every mirror state at 0, 30 and 135 degrees and beside `scale (1 -1)` and `scale (-1 1)`, and on rectangles and ellipses turned and flipped by their
/// transform: every end within 0.005 inch of where LibreOffice put it). The model's rows are in the shape's own frame, which turns and flips with it, so the row is the
/// point taken back through the shape's turn and flips. With no turn and no flip that is the frame's own reading, the one gives.
///
/// **A point's way out** (`draw:escape-direction` `left`, `right`, `up`, `down`) is the page's own direction too (right-angled connectors to such points of
/// custom shapes, rectangles and polygons turned 0, 30, 90 and 135 degrees, mirrored and flipped by their transform all leave the point that way on the page), so
/// `DirX` and `DirY` are that direction taken back through the turn and the flips.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Place {
    /// The model's `Angle` (radians, counterclockwise) and its flips (`FlipX`, `FlipY`).
    pub angle: f64,
    pub flip: (bool, bool),
    pub boxed: Boxed,
}

/// The box LibreOffice measures a shape's glue points in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Boxed {
    /// The box of the shape's frame as turned on the page: a custom shape's, a rectangle's.
    Frame,
    /// The box of a whole ellipse as turned on the page (its own extent, tighter than its frame's when it is turned).
    Ellipse,
    /// The box of the outline the shape draws, on the page: a polygon's, a polyline's, a path's, its curves' own extent and not their control points'.
    /// LibreOffice puts the four standard glue points at the middles of its sides too, numbered on the page (0 top, 1 right, 2 bottom, 3 left, whatever the turn
    /// and the flips), each leaving the box outwards.
    Outline(Extent),
    /// Not measured: the shape's own frame, turning and flipping with it, as before (an arc, a caption, a line).
    Own,
}

/// A box on the page about a shape's pin: its sides' distances from the pin in the page's axes (y up), each as a sum of the shape's size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Extent {
    pub left: Linear,
    pub right: Linear,
    pub bottom: Linear,
    pub top: Linear,
}

impl Extent {
    /// The frame of a shape square on the page: `Width` and `Height` about the pin.
    const FRAME: Extent = Extent {
        left: Linear { w: -0.5, h: 0.0, c: 0.0 },
        right: Linear { w: 0.5, h: 0.0, c: 0.0 },
        bottom: Linear { w: 0.0, h: -0.5, c: 0.0 },
        top: Linear { w: 0.0, h: 0.5, c: 0.0 },
    };

    /// Whether the box is the frame's, within what twelve places keep.
    fn is_frame(&self) -> bool {
        let near = |a: Linear, b: Linear| (a.w - b.w).abs() < 1e-12 && (a.h - b.h).abs() < 1e-12 && (a.c - b.c).abs() < 1e-12;
        let f = Extent::FRAME;
        near(self.left, f.left) && near(self.right, f.right) && near(self.bottom, f.bottom) && near(self.top, f.top)
    }

    fn mid_x(&self) -> Linear {
        self.left.plus(self.right).times(0.5)
    }

    fn mid_y(&self) -> Linear {
        self.bottom.plus(self.top).times(0.5)
    }
}

impl Place {
    /// Whether the reading of R17 in the frame is LibreOffice's: the shape's frame stands square on the page and unflipped and the box is the frame's, or the box is not
    /// measured.
    pub(crate) fn is_own(&self) -> bool {
        let square = self.angle == 0.0 && !self.flip.0 && !self.flip.1;
        match self.boxed {
            Boxed::Own => true,
            Boxed::Outline(e) => square && e.is_frame(),
            Boxed::Frame | Boxed::Ellipse => square,
        }
    }

    /// The turn's sine and cosine, with what is past twelve places left out: a quarter turn read from radians is a quarter turn.
    fn sin_cos(&self) -> (f64, f64) {
        let snap = |v: f64| if v.abs() < 1.0e-12 { 0.0 } else { v };
        (snap(self.angle.sin()), snap(self.angle.cos()))
    }

    /// The box on the page about the pin, for a shape of `w` by `h` (inches).
    fn extent(&self, (w, h): (f64, f64)) -> Extent {
        let (s, c) = self.sin_cos();
        match self.boxed {
            Boxed::Outline(e) => e,
            // A whole ellipse's own extent at this size: `hypot(W cos, H sin)` by `hypot(W sin, H cos)`, a share of the width and of the height.
            Boxed::Ellipse if w > 0.0 && h > 0.0 => {
                let (ew, eh) = ((w * c).hypot(h * s), (w * s).hypot(h * c));
                let x = Linear { w: ew / w / 2.0, ..Linear::ZERO };
                let y = Linear { h: eh / h / 2.0, ..Linear::ZERO };
                Extent { left: x.times(-1.0), right: x, bottom: y.times(-1.0), top: y }
            }
            // The turned frame's box: `|c| W + |s| H` by `|s| W + |c| H`.
            _ => {
                let x = Linear { w: c.abs() / 2.0, h: s.abs() / 2.0, c: 0.0 };
                let y = Linear { w: s.abs() / 2.0, h: c.abs() / 2.0, c: 0.0 };
                Extent { left: x.times(-1.0), right: x, bottom: y.times(-1.0), top: y }
            }
        }
    }

    /// A point of the page about the pin (`x`, `y`, page axes, y up) in the shape's own frame, from its bottom left: back through the turn, then the flips, plus half the
    /// size.
    fn local(&self, x: Linear, y: Linear) -> (Linear, Linear) {
        let (s, c) = self.sin_cos();
        let (fx, fy) = (if self.flip.0 { -1.0 } else { 1.0 }, if self.flip.1 { -1.0 } else { 1.0 });
        let lx = x.times(c).plus(y.times(s)).times(fx);
        let ly = x.times(-s).plus(y.times(c)).times(fy);
        (Linear { w: 0.5, ..Linear::ZERO }.plus(lx), Linear { h: 0.5, ..Linear::ZERO }.plus(ly))
    }

    /// A direction of the page (y up) in the shape's own frame: back through the turn, then the flips. What is past twelve places is left out.
    fn local_direction(&self, (dx, dy): (f64, f64)) -> (f64, f64) {
        let (s, c) = self.sin_cos();
        let (fx, fy) = (if self.flip.0 { -1.0 } else { 1.0 }, if self.flip.1 { -1.0 } else { 1.0 });
        let snap = |v: f64| {
            let r = (v * 1.0e12).round() / 1.0e12;
            if r.is_finite() && r != 0.0 { r } else { 0.0 }
        };
        (snap((dx * c + dy * s) * fx), snap((-dx * s + dy * c) * fy))
    }

    /// **The inverse of [`Place::local`] and of how [`placed`] reads LibreOffice's own form** (for the writer of a `draw:glue-point`): a point of the shape's own frame
    /// (`x` and `y` inches from its bottom left, y up) as the place LibreOffice reads it at, a share of the box on the page of the shape of `size` from the box's centre, `x` right and `y`
    /// **down** the page (10 cm of `svg:x` and `svg:y` is the whole box). Forward through the mirrors and then the turn (the reader undoes the turn and then the mirrors); the box is
    /// [`Place::extent`]'s. `None` for a box with no width or height, or a place that is no number.
    pub(crate) fn page_share(&self, (x, y): (f64, f64), size: (f64, f64)) -> Option<(f64, f64)> {
        let (s, c) = self.sin_cos();
        let (fx, fy) = (if self.flip.0 { -1.0 } else { 1.0 }, if self.flip.1 { -1.0 } else { 1.0 });
        // About the frame's centre, mirrored: the reader's `local` ends with the mirrors, so the page's offset is found from the local one with them undone and the turn put on.
        let (ax, ay) = ((x - size.0 / 2.0) * fx, (y - size.1 / 2.0) * fy);
        let (px, py) = (ax * c - ay * s, ax * s + ay * c);
        let e = self.extent(size);
        let (bw, bh) = (e.right.plus(e.left.times(-1.0)).at(size), e.top.plus(e.bottom.times(-1.0)).at(size));
        let (mx, my) = (e.mid_x().at(size), e.mid_y().at(size));
        let share = ((px - mx) / bw, -(py - my) / bh);
        (bw > 0.0 && bh > 0.0 && share.0.is_finite() && share.1.is_finite()).then_some(share)
    }

    /// A direction of the shape's own frame (y up) as the page's: the inverse of [`Place::local_direction`], forward through the mirrors and then the turn. The turn's sine and cosine are
    /// the reader's, with what is past twelve places left out, so a quarter turn gives an axis exactly.
    pub(crate) fn page_direction(&self, (dx, dy): (f64, f64)) -> (f64, f64) {
        let (s, c) = self.sin_cos();
        let (fx, fy) = (if self.flip.0 { -1.0 } else { 1.0 }, if self.flip.1 { -1.0 } else { 1.0 });
        let (ax, ay) = (dx * fx, dy * fy);
        let snap = |v: f64| {
            let r = (v * 1.0e12).round() / 1.0e12;
            if r.is_finite() && r != 0.0 { r } else { 0.0 }
        };
        (snap(ax * c - ay * s), snap(ax * s + ay * c))
    }
}

/// A length on a shape of `w` by `h` (inches) as a sum `Width*a+Height*b+c` (inches): what a glue point's coordinate in the model is, as a formula, so that a resize
/// keeps the point at its share of the shape as LibreOffice's do.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Linear {
    w: f64,
    h: f64,
    c: f64,
}

impl Linear {
    const ZERO: Linear = Linear { w: 0.0, h: 0.0, c: 0.0 };

    fn plus(self, o: Linear) -> Linear {
        Linear { w: self.w + o.w, h: self.h + o.h, c: self.c + o.c }
    }

    fn times(self, k: f64) -> Linear {
        Linear { w: self.w * k, h: self.h * k, c: self.c * k }
    }

    fn at(self, (w, h): (f64, f64)) -> f64 {
        self.w * w + self.h * h + self.c
    }

    /// The value at a size, and the formula: each factor rounded to twelve places (what a turn's cosine leaves past them is noise), a term of 0 left out.
    fn cell(self, forms: &mut Forms, size: (f64, f64), unit: Option<&str>) -> Cell {
        let round = |v: f64| {
            let r = (v * 1.0e12).round() / 1.0e12;
            if r.is_finite() && r != 0.0 { r } else { 0.0 }
        };
        let (a, b, c) = (round(self.w), round(self.h), round(self.c));
        let n = |v: f64| cells::number(v).text().to_owned();
        let mut text = String::new();
        for (name, k) in [("Width", a), ("Height", b)] {
            if k != 0.0 {
                if !text.is_empty() {
                    text.push('+');
                }
                text.push_str(&format!("{name}*{}", n(k)));
            }
        }
        let value = a * size.0 + b * size.1 + c;
        if text.is_empty() {
            return cells::length(c, unit);
        }
        if c != 0.0 {
            text.push_str(&format!("+{} in", n(c)));
        }
        forms.cell(&text, value, unit)
    }
}

/// The box on the page about the pin of the outline that `sections` draw (shares of the frame, y up, `geometry::path`'s and `geometry::poly`'s rows) on a shape of `size`
/// turned by `angle` and flipped by `flip`: the extremes along each page axis among the ends of the rows and the places where a curve turns back along that
/// axis, each a point of the outline as a sum of the shape's size. `None` for an outline with an elliptical arc (its extent is not worked out here: the shape keeps the
/// frame's reading) or with no row. The work is a few steps for each row, which the path's budget has already paid for.
pub(crate) fn outline_extent(sections: &[super::geometry::Section], angle: f64, flip: (bool, bool), size: (f64, f64)) -> Option<Extent> {
    use super::geometry::Row as G;
    let place = Place { angle, flip, boxed: Boxed::Own };
    // A point of the outline (shares) about the pin, on the page, as sums of the size.
    let on_page = |p: (f64, f64)| {
        let (s, c) = place.sin_cos();
        let (fx, fy) = (if flip.0 { -1.0 } else { 1.0 }, if flip.1 { -1.0 } else { 1.0 });
        let (x, y) = (Linear { w: (p.0 - 0.5) * fx, ..Linear::ZERO }, Linear { h: (p.1 - 0.5) * fy, ..Linear::ZERO });
        (x.times(c).plus(y.times(-s)), x.times(s).plus(y.times(c)))
    };
    let mut best: Option<[(f64, Linear); 4]> = None;
    let mut take = |p: (f64, f64)| {
        let (x, y) = on_page(p);
        let (vx, vy) = (x.at(size), y.at(size));
        if !(vx.is_finite() && vy.is_finite()) {
            return;
        }
        let b = best.get_or_insert([(vx, x), (vx, x), (vy, y), (vy, y)]);
        if vx < b[0].0 {
            b[0] = (vx, x);
        }
        if vx > b[1].0 {
            b[1] = (vx, x);
        }
        if vy < b[2].0 {
            b[2] = (vy, y);
        }
        if vy > b[3].0 {
            b[3] = (vy, y);
        }
    };
    // The page coordinates of a point (shares), at this size, for the turning points of a curve.
    let values = |p: (f64, f64)| {
        let (x, y) = on_page(p);
        (x.at(size), y.at(size))
    };
    for section in sections {
        let mut pen = (0.0, 0.0);
        for row in &section.rows {
            match *row {
                G::RelMoveTo(x, y) | G::RelLineTo(x, y) => {
                    take((x, y));
                    pen = (x, y);
                }
                G::RelCubBezTo { x, y, a, b, c, d } => {
                    let pts = [pen, (a, b), (c, d), (x, y)];
                    let cubic = |t: f64| {
                        let u = 1.0 - t;
                        let k = [u * u * u, 3.0 * t * u * u, 3.0 * t * t * u, t * t * t];
                        (pts.iter().zip(k).map(|(p, k)| p.0 * k).sum::<f64>(), pts.iter().zip(k).map(|(p, k)| p.1 * k).sum::<f64>())
                    };
                    let v = pts.map(values);
                    for axis in [0usize, 1] {
                        let q = v.map(|p| if axis == 0 { p.0 } else { p.1 });
                        let (d0, d1, d2) = (q[1] - q[0], q[2] - q[1], q[3] - q[2]);
                        for t in quadratic_roots(d0 - 2.0 * d1 + d2, 2.0 * (d1 - d0), d0) {
                            take(cubic(t));
                        }
                    }
                    take((x, y));
                    pen = (x, y);
                }
                G::RelQuadBezTo { x, y, a, b } => {
                    let pts = [pen, (a, b), (x, y)];
                    let quad = |t: f64| {
                        let u = 1.0 - t;
                        let k = [u * u, 2.0 * t * u, t * t];
                        (pts.iter().zip(k).map(|(p, k)| p.0 * k).sum::<f64>(), pts.iter().zip(k).map(|(p, k)| p.1 * k).sum::<f64>())
                    };
                    let v = pts.map(values);
                    for axis in [0usize, 1] {
                        let q = v.map(|p| if axis == 0 { p.0 } else { p.1 });
                        let den = q[0] - 2.0 * q[1] + q[2];
                        if den != 0.0 {
                            let t = (q[0] - q[1]) / den;
                            if t > 0.0 && t < 1.0 {
                                take(quad(t));
                            }
                        }
                    }
                    take((x, y));
                    pen = (x, y);
                }
                G::RelEllipticalArcTo { .. } => return None,
            }
        }
    }
    let [left, right, bottom, top] = best?;
    Some(Extent { left: left.1, right: right.1, bottom: bottom.1, top: top.1 })
}

/// The roots in (0, 1) of `a t² + b t + c` (a linear one when `a` is 0), at most two.
fn quadratic_roots(a: f64, b: f64, c: f64) -> Vec<f64> {
    let inside = |t: f64| t.is_finite() && t > 0.0 && t < 1.0;
    if a.abs() < 1e-15 {
        if b == 0.0 {
            return Vec::new();
        }
        return [-c / b].into_iter().filter(|t| inside(*t)).collect();
    }
    let disc = b * b - 4.0 * a * c;
    if disc.is_nan() || disc < 0.0 {
        return Vec::new();
    }
    let r = disc.sqrt();
    [(-b + r) / (2.0 * a), (-b - r) / (2.0 * a)].into_iter().filter(|t| inside(*t)).collect()
}

/// The four standard `Connection` rows of a shape whose glue points LibreOffice puts on the box of its outline (`Boxed::Outline`) when that box is not the square frame's
/// In the order (top, left, bottom, right: ODF ids 0, 3, 2, 1), each at the middle of a side of the box on the page and leaving it outwards on the page,
/// taken back through the shape's turn and flips; `X` and `Y` sums of the shape's size. `None` for any other shape, whose rows are those of R15.
pub(crate) fn outline_standard_rows(forms: &mut Forms, place: Place, size: (f64, f64), u: Units) -> Option<Section> {
    if !matches!(place.boxed, Boxed::Outline(_)) || place.is_own() {
        return None;
    }
    let e = place.extent(size);
    let sides = [
        ((e.mid_x(), e.top), (0.0, 1.0)),
        ((e.left, e.mid_y()), (-1.0, 0.0)),
        ((e.mid_x(), e.bottom), (0.0, -1.0)),
        ((e.right, e.mid_y()), (1.0, 0.0)),
    ];
    let mut section = Section::new("Connection", None);
    for (ix, ((x, y), dir)) in sides.into_iter().enumerate() {
        let (lx, ly) = place.local(x, y);
        let (dx, dy) = place.local_direction(dir);
        section.rows.push(row(
            u32::try_from(ix).unwrap_or(u32::MAX),
            None,
            vec![
                ("X", lx.cell(forms, size, u.width)),
                ("Y", ly.cell(forms, size, u.height)),
                ("DirX", cells::plain(dx)),
                ("DirY", cells::plain(dy)),
                ("Type", cells::plain(0.0)),
            ],
        ));
    }
    Some(section)
}

/// Where a `draw:glue-point` given as lengths is in the shape's own frame, for a shape whose frame is turned or flipped on the page or whose box is not its frame
/// (`place`, [`Place`]): `xy` the two lengths (inches), `align` the sides of `draw:align` (`None` for LibreOffice's own form, shares of the box from its centre in hundredths
/// of a millimetre per hundredth of a percent). Gives the row's `X` and `Y` as sums of the shape's size, measured from its frame's bottom left (y up).
fn placed(place: Place, size: (f64, f64), xy: (f64, f64), align: Option<(Side, Side)>) -> (Linear, Linear) {
    let e = place.extent(size);
    let (bw, bh) = (e.right.plus(e.left.times(-1.0)), e.top.plus(e.bottom.times(-1.0)));
    let along_x = |side: &Side| match side {
        Side::Start => e.left,
        Side::Middle => e.mid_x(),
        Side::End => e.right,
    };
    // `draw:align`'s top is the box's top, which is up the page.
    let along_y = |side: &Side| match side {
        Side::Start => e.top,
        Side::Middle => e.mid_y(),
        Side::End => e.bottom,
    };
    // The point about the pin on the page, x right and y up.
    let (ox, oy) = match align {
        None => (e.mid_x().plus(bw.times(xy.0 / 10_000.0)), e.mid_y().plus(bh.times(-xy.1 / 10_000.0))),
        Some((ax, ay)) => (along_x(&ax).plus(Linear { c: xy.0, ..Linear::ZERO }), along_y(&ay).plus(Linear { c: -xy.1, ..Linear::ZERO })),
    };
    place.local(ox, oy)
}

/// The row of one `draw:glue-point`.
fn glue_point_row(r: &mut Reader, attrs: &Attrs<'_>, id: u32, size: (f64, f64), u: Units, place: Place) -> Result<Arc<Row>, OdgError> {
    let (w, h) = size;
    let n = |v: f64| cells::number(v).text().to_owned();
    let (px, py) = (pos(attrs.peek(Ns::Svg, "x")), pos(attrs.peek(Ns::Svg, "y")));
    let align = attrs.peek(Ns::Draw, "align");
    let mut cells_out: Vec<(&'static str, Cell)> = Vec::new();
    match (px, py, align) {
        // A length on a shape turned or flipped on the page, or on one whose outline's box is not its frame: in the page's axes, from the shape's box ([`Place`]).
        (Some(Pos::Length(x)), Some(Pos::Length(y)), align) if !place.is_own() => {
            let (xy, sides) = match align {
                None => ((x.inches * HUNDREDTHS_PER_INCH, y.inches * HUNDREDTHS_PER_INCH), None),
                Some(word) => ((x.inches, y.inches), Some(aligned(word))),
            };
            let (gx, gy) = placed(place, size, xy, sides);
            cells_out.push(("X", gx.cell(&mut r.forms, size, u.width)));
            cells_out.push(("Y", gy.cell(&mut r.forms, size, u.height)));
        }
        (Some(Pos::Percent(x)), Some(Pos::Percent(y)), _) => {
            cells_out.push(("X", r.forms.cell(&format!("Width*(0.5+{}/100)", n(x)), w * (0.5 + x / 100.0), u.width)));
            cells_out.push(("Y", r.forms.cell(&format!("Height*(0.5-{}/100)", n(y)), h * (0.5 - y / 100.0), u.height)));
        }
        (Some(Pos::Length(x)), Some(Pos::Length(y)), None) => {
            // LibreOffice's own form: the length in hundredths of a millimetre is the share in hundredths of a percent, in any unit.
            let (lx, ly) = (x.inches * HUNDREDTHS_PER_INCH, y.inches * HUNDREDTHS_PER_INCH);
            cells_out.push(("X", r.forms.cell(&format!("Width*(0.5+{}/10000)", n(lx)), w * (0.5 + lx / 10_000.0), u.width)));
            cells_out.push(("Y", r.forms.cell(&format!("Height*(0.5-{}/10000)", n(ly)), h * (0.5 - ly / 10_000.0), u.height)));
        }
        (Some(Pos::Length(x)), Some(Pos::Length(y)), Some(align)) => {
            let (ax, ay) = aligned(align);
            let (dx, dy) = (x.inches, y.inches);
            let unit_text = |v: f64| format!("{} in", n(v));
            let x_cell = match ax {
                Side::Start => cells::length(dx, x.unit.word()),
                Side::Middle => r.forms.cell(&format!("Width*0.5+{}", unit_text(dx)), w * 0.5 + dx, u.width),
                Side::End => r.forms.cell(&format!("Width+{}", unit_text(dx)), w + dx, u.width),
            };
            let y_cell = match ay {
                Side::Start => r.forms.cell(&format!("Height-{}", unit_text(dy)), h - dy, u.height),
                Side::Middle => r.forms.cell(&format!("Height*0.5-{}", unit_text(dy)), h * 0.5 - dy, u.height),
                Side::End => cells::length(-dy, y.unit.word()),
            };
            cells_out.push(("X", x_cell));
            cells_out.push(("Y", y_cell));
        }
        _ => {
            r.warn("glue_point_position", || "a draw:glue-point's position cannot be read; it is placed at the centre".to_owned());
            cells_out.push(("X", r.forms.cell("Width*0.5", w * 0.5, u.width)));
            cells_out.push(("Y", r.forms.cell("Height*0.5", h * 0.5, u.height)));
        }
    }
    let direction = match attrs.peek(Ns::Draw, "escape-direction") {
        Some("left") => Some((-1.0, 0.0)),
        Some("right") => Some((1.0, 0.0)),
        Some("up") => Some((0.0, 1.0)),
        Some("down") => Some((0.0, -1.0)),
        _ => None,
    };
    // The way out is the page's own direction on a shape whose box LibreOffice measures ([`Place`]): taken back through the turn and the flips. A kind that was not
    // measured keeps it in its own frame, as before.
    let direction = direction.map(|d| if place.boxed == Boxed::Own { d } else { place.local_direction(d) });
    if let Some((dx, dy)) = direction {
        cells_out.push(("DirX", cells::plain(dx)));
        cells_out.push(("DirY", cells::plain(dy)));
    }
    r.take_items(super::cost::ROW)?;
    r.take_cells(cells_out.len())?;
    let mut made = row(id, None, cells_out);
    // The glue point's element, held exactly beside the row it made (Glue): its attributes as written (the escape direction, `draw:align`, a position as a percentage or a
    // length), stamped by the row, whole. The element stays in the shape's kept markup, where the splice writer reads it. Charged before it is made.
    let (items, bytes) = attrs.iter().fold((0usize, 0usize), |(i, b), a| (i.saturating_add(2), b.saturating_add(a.name.len() + a.value.len())));
    r.take_items(super::cost::EXT_ROW.saturating_add(items))?;
    r.take_bytes(bytes)?;
    if let Some(row) = Arc::get_mut(&mut made) {
        let held = diacraft_doc::Held {
            group: diacraft_doc::HeldGroup::Glue,
            stamp: Some(diacraft_doc::stamp_row(row)),
            attrs: RawAttrs::from_pairs(attrs.iter().map(|a| (a.name, a.value))),
            kept: diacraft_doc::Passthrough::new(),
        };
        row.ext.make_mut().odf.held = Some(held);
    }
    Ok(made)
}

/// The rows of a sheet's `Connection` section that a `draw:glue-point` of the markup made, each with the element held beside it (Glue), as the markup gave them: what
/// [`restore_held_glue`] gives back to the rows DiaCraft's data carried in their place.
pub(crate) fn markup_glue_rows(sheet: &diacraft_doc::Sheet) -> Vec<Arc<Row>> {
    let glue = |row: &Arc<Row>| row.ext.get().and_then(|e| e.odf.held.as_ref()).is_some_and(|h| h.group == diacraft_doc::HeldGroup::Glue);
    sheet
        .sections
        .iter()
        .filter(|s| s.name.as_str() == "Connection" && s.ix.is_none())
        .flat_map(|s| s.rows.iter())
        .filter(|row| glue(row))
        .cloned()
        .collect()
}

/// **A `Connection` row DiaCraft's data carried keeps the element of the glue point it is**. A shape whose rows the writer could not say by the markup alone carries its
/// whole `Connection` section in the shape's style (a custom shape whose geometry lists glue points of its own, beside a `draw:glue-point` child of its own, carries both: the model's rows 4 on
/// are the geometry's, and the writer cannot tell by the markup that they are unchanged), and the section replaces the one the markup gave, so the rows read from it lost the element each
/// `draw:glue-point` was held by. The element is still in the file, and the row is what the element gives: **a carried row with the `IX` of one of the `markup` rows and the cells whose stamp the
/// markup row's element holds is that glue point**, and is given its element again, as it was held. A row whose cells changed since the markup gave it (a point moved in DiaCraft's session)
/// has another stamp, and is left as it is. Each is charged before it is given, as the held element of a row the reader makes is.
pub(crate) fn restore_held_glue(r: &mut Reader, sheet: &mut diacraft_doc::Sheet, markup: &[Arc<Row>]) -> Result<(), OdgError> {
    if markup.is_empty() {
        return Ok(());
    }
    let by_ix: HashMap<u32, &diacraft_doc::Held> = markup.iter().filter_map(|row| Some((row.ix?, row.ext.get()?.odf.held.as_ref()?))).collect();
    let Some(section) = sheet.sections.iter_mut().find(|s| s.name.as_str() == "Connection" && s.ix.is_none()) else { return Ok(()) };
    let wanted: Vec<(usize, diacraft_doc::Held)> = section
        .rows
        .iter()
        .enumerate()
        .filter_map(|(at, row)| {
            let held = by_ix.get(&row.ix?)?;
            let free = row.ext.get().is_none_or(|e| e.odf.held.is_none());
            (free && held.stamp == Some(diacraft_doc::stamp_row(row))).then(|| (at, (*held).clone()))
        })
        .collect();
    if wanted.is_empty() {
        return Ok(());
    }
    let section = Arc::make_mut(section);
    for (at, held) in wanted {
        let (items, bytes) = held.attrs.iter().fold((0usize, 0usize), |(i, b), (k, v)| (i.saturating_add(2), b.saturating_add(k.len() + v.len())));
        r.take_items(super::cost::EXT_ROW.saturating_add(items))?;
        r.take_bytes(bytes)?;
        if let Some(row) = section.rows.get_mut(at) {
            Arc::make_mut(row).ext.make_mut().odf.held = Some(held);
        }
    }
    Ok(())
}

/// Which side a `draw:align` names along one axis.
enum Side {
    Start,
    Middle,
    End,
}

/// The sides `draw:align` names: along x (left, centre, right) and along y (top, centre, bottom).
fn aligned(word: &str) -> (Side, Side) {
    let x = if word.contains("left") {
        Side::Start
    } else if word.contains("right") {
        Side::End
    } else {
        Side::Middle
    };
    let y = if word.contains("top") {
        Side::Start
    } else if word.contains("bottom") {
        Side::End
    } else {
        Side::Middle
    };
    (x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The turns, mirrors and boxes a shape of the file can have, with the sizes the checks use.
    fn places() -> Vec<(Place, (f64, f64))> {
        let mut out = Vec::new();
        for degrees in [0.0_f64, 30.0, 45.0, 90.0, 135.0, 180.0, 270.0, -60.0, 359.0] {
            for flip in [(false, false), (true, false), (false, true), (true, true)] {
                for boxed in [Boxed::Frame, Boxed::Ellipse] {
                    for size in [(3.0, 2.0), (1.5, 4.0), (2.0, 2.0)] {
                        out.push((Place { angle: degrees.to_radians(), flip, boxed }, size));
                    }
                }
            }
        }
        out
    }

    /// **The writer's place is read back as the point by the reader's own mapping**: a point of the shape's frame as [`Place::page_share`] puts it, written as LibreOffice's own
    /// form (10 cm of a length for the whole box) and read by [`placed`], is the same point of the frame, for every turn, mirror and box.
    #[test]
    fn a_point_placed_on_the_page_is_read_back_as_the_same_point_of_the_frame() {
        for (place, size) in places() {
            for (x, y) in [(0.0, 0.0), (1.0, 0.0), (0.5, 1.0), (0.25, 0.75), (0.9, 0.1), (0.0, 0.5), (1.0, 1.0), (1.4, -0.2)] {
                let local = (x * size.0, y * size.1);
                let (sx, sy) = place.page_share(local, size).expect("a box with a size");
                let (gx, gy) = placed(place, size, (sx * 10_000.0, sy * 10_000.0), None);
                let (rx, ry) = (gx.at(size), gy.at(size));
                assert!(
                    (rx - local.0).abs() < 1e-9 && (ry - local.1).abs() < 1e-9,
                    "{place:?} {size:?}: ({x}, {y}) came back as ({rx}, {ry}) of {local:?}"
                );
            }
        }
    }

    /// A way out of the frame as [`Place::page_direction`] puts it is read back by [`Place::local_direction`] as the same way.
    #[test]
    fn a_direction_placed_on_the_page_is_read_back_as_the_same_direction_of_the_frame() {
        for (place, _) in places() {
            for way in [(0.0, 1.0), (-1.0, 0.0), (0.0, -1.0), (1.0, 0.0), (0.6, -0.8)] {
                let page = place.page_direction(way);
                let back = place.local_direction(page);
                assert!(
                    (back.0 - way.0).abs() < 1e-9 && (back.1 - way.1).abs() < 1e-9,
                    "{place:?}: {way:?} went out as {page:?} and came back as {back:?}"
                );
            }
        }
    }

    /// A quarter turn gives the way out along an axis of the page exactly, so that the writer can say `left`, `right`, `up` or `down`; any other turn gives none of the four.
    #[test]
    fn a_quarter_turn_puts_the_way_out_on_an_axis_exactly() {
        let at = |degrees: f64, flip: (bool, bool)| Place { angle: degrees.to_radians(), flip, boxed: Boxed::Frame }.page_direction((0.0, 1.0));
        assert_eq!(at(90.0, (false, false)), (-1.0, 0.0), "up, turned a quarter counterclockwise, is left");
        assert_eq!(at(180.0, (false, false)), (0.0, -1.0));
        assert_eq!(at(270.0, (false, false)), (1.0, 0.0));
        assert_eq!(at(0.0, (false, true)), (0.0, -1.0), "mirrored top to bottom, up is down");
        let slant = at(30.0, (false, false));
        assert!(slant.0 != 0.0 && slant.1 != 0.0, "{slant:?}");
    }

    /// A box with no width or height has no place to say.
    #[test]
    fn a_frame_with_no_size_has_no_place() {
        let place = Place { angle: 0.5, flip: (false, false), boxed: Boxed::Frame };
        assert_eq!(place.page_share((0.0, 0.0), (0.0, 0.0)), None);
        assert_eq!(place.page_share((f64::NAN, 0.5), (2.0, 1.0)), None);
    }
}
