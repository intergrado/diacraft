//! A shape's outline written back:
//! the `Geometry` sections of the model as the attribute that holds the outline in the file.
//!
//! * **`draw:points`** of a polygon or polyline: integers in the element's own view box, from the section's `RelMoveTo` and `RelLineTo` rows (a polygon's
//!   closing row left out). A point that does not fall on the integer grid of the view box within 1E-6 of its width is refused in words (scaling the view
//!   box by a power of ten, as the table says, is not written yet).
//! * **`svg:d`** of a path: the sections as `M`, `L`, `C`, `Q` and `Z`, in the element's own view box, which stays. A section that ends where it began
//!   closes with `Z`, so that the reader finds it closed again.
//! * **`draw:enhanced-path`** of a custom shape: the rows of tier 1 (a `MoveTo` and `LineTo` outline in the forms DiaCraft's basic shapes use, a
//!   lone `Ellipse` in the form of V50) as `M`, `L`, `Z` and `U`, in the 21600 view box, as give them. Any other row is refused in words:
//!   its form is tier 2, which the tier-2 writer writes.
//! * **`draw:enhanced-path` of a new custom shape** ([`new_outline`]): the sets of R30 (a section that is not filled, not stroked or not shown goes in a set with
//!   `F`, `S` or both) and the arcs and ellipses of the table (`A` and `W` on the ellipse's box, `U`), for a shape the save adds. [`enhanced_path`] stays the writer of the
//!   outline of a shape of the file whose geometry was edited, which writes only what the reader gives back as the same rows.
//! * **`draw:modifiers`**: the values a handle drag changed, in their places.
//!
//! Every function gives the text or the words of why not; none guesses.

use diacraft_doc::{Cell, FormulaSlot, Row, Section, Sheet};
use diacraft_formula::Expr;

use crate::read::geometry::ViewBox;
use crate::read::names::Ns;

/// The `Geometry` sections of a sheet in the order of their `IX`.
fn geometry_sections(sheet: &Sheet) -> Vec<&Section> {
    let mut sections: Vec<&Section> = sheet.sections.iter().filter(|s| s.name.as_str() == "Geometry").map(|s| &**s).collect();
    sections.sort_by_key(|s| s.ix);
    sections
}

fn number(row: &Row, name: &str) -> Option<f64> {
    row.cells.get(name).and_then(|c| c.value.as_ref()?.number()).filter(|n| n.is_finite())
}

/// A number as the shortest decimal of at most six places, with no exponent and no `-0`.
pub(crate) fn num(v: f64) -> String {
    let r = (v * 1e6).round() / 1e6;
    if r == 0.0 { "0".to_owned() } else { format!("{r}") }
}

/// A point of a `Rel` row, as a share of the frame.
fn share_point(row: &Row) -> Option<(f64, f64)> {
    Some((number(row, "X")?, number(row, "Y")?))
}

fn view_point(vb: &ViewBox, (x, y): (f64, f64)) -> (f64, f64) {
    (vb.left + x * vb.width, vb.top + (1.0 - y) * vb.height)
}

/// The most a coordinate or a modifier may be when it is written: the bound `points_text` always had (a row of `Width*1e300`
/// was written as `inf`).
const MOST: f64 = 1e15;

const WORDS_RANGE: &str = "a coordinate or a modifier too large to write (past 1e15, or not a number)";

/// Whether `v` is a number the writer writes: finite and within [`MOST`].
fn within(v: f64) -> bool {
    v.is_finite() && v.abs() <= MOST
}

const WORDS_FORM: &str = "an outline whose rows are not the plain lines and curves the reader reads from a file (a later version writes tier 2)";

/// The points of the one section of a polygon or polyline, in the view box `vb`, as the text of `draw:points`.
pub(crate) fn points_text(sheet: &Sheet, vb: &ViewBox, closed: bool) -> Result<String, &'static str> {
    let sections = geometry_sections(sheet);
    let [section] = sections.as_slice() else { return Err("a polygon or polyline whose outline is not one section") };
    let mut points: Vec<(f64, f64)> = Vec::new();
    for (i, row) in section.rows.iter().enumerate() {
        let wanted = if i == 0 { "RelMoveTo" } else { "RelLineTo" };
        if row.row_type.as_deref() != Some(wanted) {
            return Err(WORDS_FORM);
        }
        points.push(share_point(row).ok_or("an outline row that holds no number")?);
    }
    // A polygon's last row goes back to its first point, which `draw:points` leaves implicit.
    if closed && points.len() > 1 && points.first() == points.last() {
        points.pop();
    }
    let mut text = String::new();
    for p in points {
        let (x, y) = view_point(vb, p);
        let (rx, ry) = (x.round(), y.round());
        let tolerance = 1e-6 * vb.width.max(vb.height);
        if (x - rx).abs() > tolerance || (y - ry).abs() > tolerance || rx.abs() > MOST || ry.abs() > MOST {
            return Err("a point that does not fall on the integer grid of the view box (the view box is not scaled by a save yet)");
        }
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(&format!("{},{}", num(rx), num(ry)));
    }
    Ok(text)
}

/// The sections of a path as the text of `svg:d`, in the view box `vb`.
pub(crate) fn path_text(sheet: &Sheet, vb: &ViewBox) -> Result<String, &'static str> {
    let mut d = String::new();
    for section in geometry_sections(sheet) {
        let rows: Vec<&Row> = section.rows.iter().map(|r| &**r).collect();
        let start = rows.first().and_then(|r| share_point(r));
        let mut last = start;
        for (i, row) in rows.iter().enumerate() {
            let kind = row.row_type.as_deref().unwrap_or_default();
            let point = |a: &str, b: &str| -> Result<(f64, f64), &'static str> {
                let p = (number(row, a).ok_or("an outline row that holds no number")?, number(row, b).ok_or("an outline row that holds no number")?);
                let (x, y) = view_point(vb, p);
                if !within(x) || !within(y) {
                    return Err(WORDS_RANGE);
                }
                Ok((x, y))
            };
            let at = |p: (f64, f64)| format!("{} {}", num(p.0), num(p.1));
            let end = share_point(row).ok_or("an outline row that holds no number")?;
            // A section begins with its move: a path that began with a line would read with no first point.
            if i == 0 && kind != "RelMoveTo" {
                return Err(WORDS_FORM);
            }
            let closes = kind == "RelLineTo" && Some(end) == start && i + 1 == rows.len();
            match (i, kind) {
                (0, "RelMoveTo") => d.push_str(&format!("{}M {}", if d.is_empty() { "" } else { " " }, at(point("X", "Y")?))),
                (_, "RelLineTo") if closes => d.push_str(" Z"),
                (_, "RelLineTo") => d.push_str(&format!(" L {}", at(point("X", "Y")?))),
                (_, "RelCubBezTo") => {
                    d.push_str(&format!(" C {} {} {}", at(point("A", "B")?), at(point("C", "D")?), at(point("X", "Y")?)));
                }
                (_, "RelQuadBezTo") => d.push_str(&format!(" Q {} {}", at(point("A", "B")?), at(point("X", "Y")?))),
                _ => return Err(WORDS_FORM),
            }
            last = Some(end);
        }
        // The reader makes an open subpath of a path unfilled, and a closed one filled: the flag must be what it would read.
        let open = last != start;
        let no_fill = section.cells.get("NoFill").and_then(|c| c.value.as_ref()?.number()).unwrap_or(0.0) == 1.0;
        if open != no_fill && !rows.is_empty() {
            return Err("a section whose fill flag is not what its form, open or closed, gives when it is read");
        }
        if ["NoLine", "NoShow"].iter().any(|f| section.cells.get(*f).and_then(|c| c.value.as_ref()?.number()).unwrap_or(0.0) == 1.0) {
            return Err("a section that has no line or is not shown (its form is tier 2, which a later version writes)");
        }
    }
    if d.is_empty() { Err("an outline with no row") } else { Ok(d) }
}

/// What a cell's formula and value say as a share of the frame along one axis (`size` is the shape's `Width` or `Height`, `axis` the name `Width` or
/// `Height`): the constant 0, the bare name, or the name times a number: the forms DiaCraft's basic shapes use. `None` for any other.
fn share_of(cell: Option<&Cell>, axis: &str) -> Option<f64> {
    let cell = cell?;
    match cell.formula.text() {
        None => cell.value.as_ref()?.number().filter(|n| *n == 0.0).map(|_| 0.0),
        Some(f) if f == axis => Some(1.0),
        Some(f) => f.strip_prefix(axis)?.strip_prefix('*')?.parse::<f64>().ok().filter(|k| k.is_finite()),
    }
}

/// The text of `draw:enhanced-path` of a shape whose rows are the plain outline of DiaCraft's basic shapes, or the words of why it is not.
pub(crate) fn enhanced_path(sheet: &Sheet) -> Result<String, &'static str> {
    let sections = geometry_sections(sheet);
    if sections.is_empty() {
        return Err("a custom shape with no geometry");
    }
    let mut out = String::new();
    for section in &sections {
        if ["NoFill", "NoLine", "NoShow"].iter().any(|f| section.cells.get(*f).and_then(|c| c.value.as_ref()?.number()).unwrap_or(0.0) == 1.0) {
            return Err("a section that is not filled, not stroked or not shown (a later version writes its sets)");
        }
        let rows: Vec<&Row> = section.rows.iter().map(|r| &**r).collect();
        // An ellipse: one row in the form of V50.
        if let [row] = rows.as_slice()
            && row.row_type.as_deref() == Some("Ellipse")
        {
            let forms = [("X", "Width*0.5"), ("Y", "Height*0.5"), ("A", "Width"), ("B", "Height*0.5"), ("C", "Width*0.5"), ("D", "Height")];
            if forms.iter().all(|(name, f)| row.cells.get(*name).and_then(|c| c.formula.text()) == Some(*f)) {
                out.push_str("U 10800 10800 10800 10800 0 360 ");
                continue;
            }
            return Err(WORDS_FORM);
        }
        let mut points: Vec<(f64, f64)> = Vec::new();
        for (i, row) in rows.iter().enumerate() {
            let wanted = if i == 0 { "MoveTo" } else { "LineTo" };
            if row.row_type.as_deref() != Some(wanted) {
                return Err(WORDS_FORM);
            }
            let x = share_of(row.cells.get("X"), "Width").ok_or(WORDS_FORM)?;
            let y = share_of(row.cells.get("Y"), "Height").ok_or(WORDS_FORM)?;
            let (px, py) = (x * 21600.0, (1.0 - y) * 21600.0);
            if !within(px) || !within(py) {
                return Err(WORDS_RANGE);
            }
            points.push((px, py));
        }
        let Some(first) = points.first().copied() else { return Err("a section with no row") };
        out.push_str(&format!("M {} {}", num(first.0), num(first.1)));
        if points.len() > 1 {
            out.push_str(" L");
            for p in &points[1..] {
                out.push_str(&format!(" {} {}", num(p.0), num(p.1)));
            }
        }
        if points.len() > 1 && points.last() == Some(&first) {
            out.push_str(" Z");
        }
        out.push(' ');
    }
    out.push('N');
    Ok(out)
}

// ---- the outline of a new custom shape: sets and arcs ------------------------------------------------------------------------------------

/// The view box a new custom shape's path is written in.
const VIEW: f64 = 21_600.0;

/// The most subpaths one outline writes: the reader makes a section of each and refuses a sheet at `MAX_SECTIONS_PER_SHEET - 4`, so a file with more is one it cannot read.
const MOST_SUBPATHS: usize = diacraft_doc::MAX_SECTIONS_PER_SHEET - 8;

/// The most rows an outline is written from (the second form of R30 writes each twice, and the reader spends a command on each).
const MOST_ROWS: usize = 100_000;

const WORDS_TOO_MANY: &str = "an outline of more subpaths or rows than a file's reader reads in one shape";

/// What [`new_outline`] makes of the `Geometry` sections of a new custom shape.
#[derive(Debug)]
pub(crate) struct NewOutline {
    /// The text of `draw:enhanced-path`.
    pub path: String,
    /// The subpaths written, which the reader makes a section each of: what a program that rebuilds the shape from the path alone has.
    pub subpaths: usize,
    /// Whether the reader gives the model's own sections back from the path (the same rows, formulas, flags and numbers), so that none of them is tier 2.
    pub reader_form: bool,
    /// The rows looked at, for the work count of the save.
    pub rows: usize,
}

/// A point of the path in the view box, y down.
type Pt = (f64, f64);

/// What a run draws after its start.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Piece {
    Line(Pt),
    /// `A` (counterclockwise as seen) or `W` (clockwise): the ellipse in the box `bx` (left, top, right, bottom), from the ray through `from` to the ray through `to`.
    Arc {
        ccw: bool,
        bx: [f64; 4],
        from: Pt,
        to: Pt,
    },
}

/// The rows of a section from one `MoveTo` to the next: a run of the stroke.
#[derive(Clone, Debug, PartialEq)]
struct Run {
    start: Pt,
    pieces: Vec<Piece>,
}

impl Run {
    fn end(&self) -> Pt {
        match self.pieces.last() {
            Some(Piece::Line(p)) | Some(Piece::Arc { to: p, .. }) => *p,
            None => self.start,
        }
    }

    /// A run that goes on and ends where it began.
    fn closed(&self) -> bool {
        !self.pieces.is_empty() && self.end() == self.start
    }
}

#[derive(Debug)]
enum Body {
    Runs(Vec<Run>),
    /// A whole ellipse: its centre and half-axes in the view box.
    Ellipse([f64; 4]),
}

#[derive(Debug)]
struct Sec {
    body: Body,
    no_fill: bool,
    no_line: bool,
    no_show: bool,
    /// The reader gives this section back from the path as it is: numbered as it stands, its flags and rows in the forms the reader writes, one run.
    reproduced: bool,
}

/// What a section draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Class {
    /// Fills and strokes.
    Both,
    /// Fills and has no line.
    FillOnly,
    /// Strokes and does not fill.
    StrokeOnly,
    /// Draws nothing: `NoShow`, or neither fill nor line.
    Hidden,
}

impl Sec {
    fn class(&self) -> Class {
        match (self.no_show || (self.no_fill && self.no_line), self.no_fill, self.no_line) {
            (true, ..) => Class::Hidden,
            (false, false, false) => Class::Both,
            (false, false, true) => Class::FillOnly,
            (false, true, _) => Class::StrokeOnly,
        }
    }

    fn runs(&self) -> &[Run] {
        match &self.body {
            Body::Runs(runs) => runs,
            Body::Ellipse(_) => &[],
        }
    }

    fn fills(&self) -> bool {
        matches!(self.class(), Class::Both | Class::FillOnly)
    }

    fn strokes(&self) -> bool {
        matches!(self.class(), Class::Both | Class::StrokeOnly)
    }

    /// The single or closed: a section whose runs one ring through them fills as its subpaths do.
    fn fills_as_subpaths(&self) -> bool {
        self.runs().len() <= 1 || self.runs().iter().all(Run::closed)
    }
}

/// The `Width` and `Height` of the shape, where they are lengths above 0: what an arc, and a coordinate that is its value, are worked out in.
#[derive(Clone, Copy)]
struct Size {
    w: Option<f64>,
    h: Option<f64>,
}

impl Size {
    fn of(sheet: &Sheet) -> Size {
        let get = |name: &str| sheet.cell(name).and_then(|c| c.value.as_ref()?.number()).filter(|v| v.is_finite() && *v > 0.0);
        Size { w: get("Width"), h: get("Height") }
    }

    fn along(&self, axis: &str) -> Option<f64> {
        if axis == "Width" { self.w } else { self.h }
    }
}

/// Whether a cell's formula names a cell other than the shape's own size along `axis` (or calls a function): a formula that is neither a share of the size nor one of the
/// equation forms (a length beside the size), which a later version writes as an equation.
fn names_another(cell: &Cell, axis: &str) -> bool {
    let FormulaSlot::Local(formula) = &cell.formula else { return false };
    let Ok(ast) = &formula.ast else { return false };
    let mut stack: Vec<&Expr> = vec![ast];
    while let Some(e) = stack.pop() {
        match e {
            Expr::Ref(r) => {
                let own = matches!(r.path.as_slice(), [s] if s.name.eq_ignore_ascii_case(axis) && s.index.is_none());
                if !r.qualifiers.is_empty() || !own {
                    return true;
                }
            }
            Expr::Call(_) => return true,
            Expr::Unary(_, x) | Expr::Percent(x) => stack.push(x),
            Expr::Binary(_, a, b) => {
                stack.push(a);
                stack.push(b);
            }
            _ => {}
        }
    }
    false
}

/// A coordinate cell as a share of the frame along `axis`, and whether the reader gives back this very cell. A share in the forms DiaCraft's basic shapes use (the constant 0,
/// `Width`, `Width*k`) is itself, and plain when its formula is the text the reader writes for it; a formula that names another cell or calls a function is the share its value
/// is at the size written (any other formula is written as its current value, and its cell is tier 2); any other cell is refused (R33 writes a length beside the size as an
/// equation, which a later version does).
fn coordinate(cell: Option<&Cell>, axis: &str, size: &Size) -> Result<(f64, bool), &'static str> {
    if let Some(k) = share_of(cell, axis) {
        let plain = cell.and_then(|c| c.formula.text()) == crate::read::cells::share_text(axis, k).as_deref();
        return Ok((k, plain));
    }
    let cell = cell.ok_or(WORDS_FORM)?;
    if !names_another(cell, axis) {
        return Err(WORDS_FORM);
    }
    let extent = size.along(axis).ok_or(WORDS_FORM)?;
    let value = cell.value.as_ref().and_then(|v| v.number()).filter(|v| v.is_finite()).ok_or(WORDS_FORM)?;
    Ok((value / extent, false))
}

/// A point at the shares (`x`, `y`) of the frame in the view box, refused past what is written.
fn view((x, y): (f64, f64)) -> Result<Pt, &'static str> {
    let p = (x * VIEW, (1.0 - y) * VIEW);
    if within(p.0) && within(p.1) { Ok(p) } else { Err(WORDS_RANGE) }
}

/// `NoFill`, `NoLine` and `NoShow` of a section, and whether the reader gives the section's cells back as they are: no cell beside the flags and the two the reader never makes
/// (`NoSnap` and `NoQuickDrag`, which the constant 0 stands for), each a plain 0 or 1 with no formula, and at most one of the three set (a path says "not filled and not stroked" as
/// `NoShow`).
fn flags_of(section: &Section) -> ([bool; 3], bool) {
    let mut set = [false; 3];
    let mut exact = true;
    for (name, cell) in section.cells.iter() {
        let value = cell.value.as_ref().and_then(|v| v.number()).filter(|v| v.is_finite());
        let plain = matches!(cell.formula, FormulaSlot::Absent) && cell.extra.is_none() && cell.raw.is_empty();
        let at = match name.as_str() {
            "NoFill" => 0,
            "NoLine" => 1,
            "NoShow" => 2,
            "NoSnap" | "NoQuickDrag" => {
                exact &= plain && value == Some(0.0);
                continue;
            }
            _ => {
                exact = false;
                continue;
            }
        };
        if let Some(slot) = set.get_mut(at) {
            *slot = value.is_some_and(|v| v != 0.0);
        }
        exact &= plain && matches!(value, Some(v) if v == 0.0 || v == 1.0);
    }
    (set, exact && set.iter().filter(|s| **s).count() <= 1)
}

/// The circle through the pen `p0` and `p1` that a bow `bow` bends the chord to (`ArcTo`: the bow is how far the arc's middle stands from the chord's, to the right of the
/// way when it is above 0 in the sign gives, counterclockwise as seen), in the units of the points. `None` when the arc is its chord.
fn sagitta_circle(p0: Pt, p1: Pt, bow: f64) -> Option<(Pt, Pt, bool)> {
    let (dx, dy) = (p1.0 - p0.0, p1.1 - p0.1);
    let chord = dx.hypot(dy);
    if !bow.is_finite() || !chord.is_finite() || chord <= 0.0 || bow.abs() <= 1e-9 * chord {
        return None;
    }
    // The unit normal to the right of the way, and how far along it the centre is from the chord's middle.
    let (nx, ny) = (dy / chord, -dx / chord);
    let along = (bow * bow - chord * chord / 4.0) / (2.0 * bow);
    let radius = (chord * chord / 4.0 + bow * bow) / (2.0 * bow.abs());
    let centre = (p0.0 + dx / 2.0 + nx * along, p0.1 + dy / 2.0 + ny * along);
    (centre.0.is_finite() && centre.1.is_finite() && radius.is_finite()).then_some((centre, (radius, radius), bow > 0.0))
}

/// The ellipse with its axes along the frame's, `d` the length of the one along x over the one along y, through `p0`, `p1` and `p2` (`EllipticalArcTo` with `C` 0): the three
/// points with each x divided by `d` are on a circle, and the ellipse is that circle stretched by `d` along x. Its centre, its half-axes, and whether `p0` to `p2` through `p1` goes
/// counterclockwise as seen. `None` when the points are on a line (the row then draws its chord).
fn ellipse_through(p0: Pt, p1: Pt, p2: Pt, d: f64) -> Option<(Pt, Pt, bool)> {
    let (b, c) = ((p1.0 / d - p0.0 / d, p1.1 - p0.1), (p2.0 / d - p0.0 / d, p2.1 - p0.1));
    let det = 2.0 * (b.0 * c.1 - b.1 * c.0);
    if !det.is_finite() || det.abs() <= 1e-9 * b.0.hypot(b.1) * c.0.hypot(c.1) {
        return None;
    }
    let (b2, c2) = (b.0 * b.0 + b.1 * b.1, c.0 * c.0 + c.1 * c.1);
    let (ux, uy) = ((c.1 * b2 - b.1 * c2) / det, (b.0 * c2 - c.0 * b2) / det);
    let radius = ux.hypot(uy);
    let centre = ((p0.0 / d + ux) * d, p0.1 + uy);
    (centre.0.is_finite() && centre.1.is_finite() && radius.is_finite() && radius > 0.0).then_some((centre, (radius * d, radius), det > 0.0))
}

/// An arc row as the piece the path draws, and the shares its end is at: `A` or `W` on the ellipse's box with the pen and the end as the two rays (the table; drawn exactly at the size
/// written), or the chord when the three points are on a line. `ArcTo` is a circle by its bow and `EllipticalArcTo` an ellipse by its middle point and its ratio; an
/// `EllipticalArcTo` whose `C` is not 0, or whose `D` is not above 0, is refused (a later version writes it as curves).
fn arc_piece(row: &Row, by_bow: bool, pen: (f64, f64), size: &Size) -> Result<(Piece, (f64, f64)), &'static str> {
    let (w, h) = (size.along("Width").ok_or(WORDS_FORM)?, size.along("Height").ok_or(WORDS_FORM)?);
    let end = (coordinate(row.cells.get("X"), "Width", size)?.0, coordinate(row.cells.get("Y"), "Height", size)?.0);
    let (from_v, to_v) = (view(pen)?, view(end)?);
    let inches = |p: (f64, f64)| (p.0 * w, p.1 * h);
    let found = if by_bow {
        sagitta_circle(inches(pen), inches(end), number(row, "A").ok_or(WORDS_FORM)?)
    } else {
        let via = (coordinate(row.cells.get("A"), "Width", size)?.0, coordinate(row.cells.get("B"), "Height", size)?.0);
        let (c, d) = (number(row, "C").ok_or(WORDS_FORM)?, number(row, "D").ok_or(WORDS_FORM)?);
        if c.abs() > 1e-9 || !d.is_finite() || d <= 0.0 {
            return Err(WORDS_FORM);
        }
        ellipse_through(inches(pen), inches(via), inches(end), d)
    };
    let Some((centre, (rx, ry), ccw)) = found else { return Ok((Piece::Line(to_v), end)) };
    let bx = [(centre.0 - rx) / w * VIEW, (1.0 - (centre.1 + ry) / h) * VIEW, (centre.0 + rx) / w * VIEW, (1.0 - (centre.1 - ry) / h) * VIEW];
    if !bx.iter().all(|v| within(*v)) {
        return Err(WORDS_RANGE);
    }
    Ok((Piece::Arc { ccw, bx, from: from_v, to: to_v }, end))
}

/// An `Ellipse` row with its axes along the frame's as the whole ellipse in the view box; whether it is DiaCraft's own form, which the reader gives back as the same row. A row
/// whose axes are turned (the point on one axis is off the other's line), or that has no size, is refused.
fn ellipse_body(row: &Row, size: &Size) -> Result<(Body, bool), &'static str> {
    let forms = [("X", "Width*0.5"), ("Y", "Height*0.5"), ("A", "Width"), ("B", "Height*0.5"), ("C", "Width*0.5"), ("D", "Height")];
    let own = forms.iter().all(|(name, f)| row.cells.get(*name).and_then(|c| c.formula.text()) == Some(*f));
    let get = |name: &str, axis: &str| coordinate(row.cells.get(name), axis, size).map(|(k, _)| k);
    let (x, y) = (get("X", "Width")?, get("Y", "Height")?);
    let (a, b, c, d) = (get("A", "Width")?, get("B", "Height")?, get("C", "Width")?, get("D", "Height")?);
    let (rx, ry) = ((a - x).abs(), (d - y).abs());
    if (b - y).abs() > 1e-9 || (c - x).abs() > 1e-9 || rx <= 1e-9 || ry <= 1e-9 {
        return Err(WORDS_FORM);
    }
    let centre = view((x, y))?;
    let radii = (rx * VIEW, ry * VIEW);
    if !within(radii.0) || !within(radii.1) {
        return Err(WORDS_RANGE);
    }
    Ok((Body::Ellipse([centre.0, centre.1, radii.0, radii.1]), own))
}

/// The runs of a section's rows (a move starts one, a line, an arc go on), and whether every row is in the plain form the reader gives back as it is.
fn runs_of(section: &Section, size: &Size, rows_seen: &mut usize) -> Result<(Vec<Run>, bool), &'static str> {
    let mut runs: Vec<Run> = Vec::new();
    let mut plain = true;
    let mut pen: Option<(f64, f64)> = None;
    for (i, row) in section.rows.iter().enumerate() {
        *rows_seen = rows_seen.saturating_add(1);
        if *rows_seen > MOST_ROWS {
            return Err(WORDS_TOO_MANY);
        }
        let kind = row.row_type.as_deref().unwrap_or_default();
        if row.del || (i == 0 && kind != "MoveTo") {
            return Err(WORDS_FORM);
        }
        match kind {
            "MoveTo" | "LineTo" => {
                let ((x, px), (y, py)) = (coordinate(row.cells.get("X"), "Width", size)?, coordinate(row.cells.get("Y"), "Height", size)?);
                plain &= px && py;
                let at = view((x, y))?;
                if kind == "MoveTo" {
                    runs.push(Run { start: at, pieces: Vec::new() });
                } else {
                    runs.last_mut().ok_or(WORDS_FORM)?.pieces.push(Piece::Line(at));
                }
                pen = Some((x, y));
            }
            "ArcTo" | "EllipticalArcTo" => {
                plain = false;
                let (piece, end) = arc_piece(row, kind == "ArcTo", pen.ok_or(WORDS_FORM)?, size)?;
                runs.last_mut().ok_or(WORDS_FORM)?.pieces.push(piece);
                pen = Some(end);
            }
            _ => return Err(WORDS_FORM),
        }
    }
    Ok((runs, plain))
}

/// One `Geometry` section, the `at`th by `IX`.
fn parse_section(section: &Section, at: usize, size: &Size, rows_seen: &mut usize) -> Result<Sec, &'static str> {
    if section.del || section.rows.is_empty() {
        return Err(if section.del { WORDS_FORM } else { "a section with no row" });
    }
    let (flags, flags_exact) = flags_of(section);
    let numbered = section.ix.is_some_and(|ix| usize::try_from(ix).ok() == Some(at));
    let ellipse = match section.rows.as_slice() {
        [row] if row.row_type.as_deref() == Some("Ellipse") => Some(row),
        _ => None,
    };
    let (body, plain, single) = match ellipse {
        Some(row) => {
            *rows_seen = rows_seen.saturating_add(1);
            let (body, own) = ellipse_body(row, size)?;
            (body, own, true)
        }
        None => {
            let (runs, plain) = runs_of(section, size, rows_seen)?;
            let single = runs.len() == 1;
            (Body::Runs(runs), plain, single)
        }
    };
    Ok(Sec { body, no_fill: flags[0], no_line: flags[1], no_show: flags[2], reproduced: numbered && flags_exact && plain && single })
}

/// A run as the text of a subpath: its move, its lines and arcs, and `Z` when it ends where it began and `close` says so.
fn run_text(run: &Run, close: bool) -> String {
    let mut text = format!("M {} {}", num(run.start.0), num(run.start.1));
    put_pieces(&mut text, &run.pieces);
    if close && run.closed() {
        text.push_str(" Z");
    }
    text
}

/// The commands of the pieces of a run, consecutive lines under one `L`.
fn put_pieces(text: &mut String, pieces: &[Piece]) {
    let mut lines = false;
    for piece in pieces {
        match piece {
            Piece::Line((x, y)) => {
                if !lines {
                    text.push_str(" L");
                    lines = true;
                }
                text.push_str(&format!(" {} {}", num(*x), num(*y)));
            }
            Piece::Arc { ccw, bx, from, to } => {
                lines = false;
                text.push_str(if *ccw { " A" } else { " W" });
                for v in [bx[0], bx[1], bx[2], bx[3], from.0, from.1, to.0, to.1] {
                    text.push(' ');
                    text.push_str(&num(v));
                }
            }
        }
    }
}

/// The runs of a section joined into one ring as the VSDX application fills them: each inner move a line, the whole closed with `Z`.
fn ring_text(runs: &[Run]) -> String {
    let mut text = String::new();
    for (i, run) in runs.iter().enumerate() {
        if i == 0 {
            text = format!("M {} {}", num(run.start.0), num(run.start.1));
        } else {
            text.push_str(&format!(" L {} {}", num(run.start.0), num(run.start.1)));
        }
        put_pieces(&mut text, &run.pieces);
    }
    text.push_str(" Z");
    text
}

/// A whole ellipse as a subpath: `U` from 0 to 360 degrees.
fn ellipse_text([cx, cy, rx, ry]: [f64; 4]) -> String {
    format!("U {} {} {} {} 0 360", num(cx), num(cy), num(rx), num(ry))
}

/// The subpaths of a section that stroke or fill it as it is: a subpath for each run (closed with `Z` where it is), or its ellipse.
fn subpaths_of(sec: &Sec) -> Vec<String> {
    match &sec.body {
        Body::Runs(runs) => runs.iter().map(|r| run_text(r, true)).collect(),
        Body::Ellipse(e) => vec![ellipse_text(*e)],
    }
}

/// The subpath of a section that fills as one ring.
fn ring_of(sec: &Sec) -> String {
    match &sec.body {
        Body::Runs(runs) => ring_text(runs),
        Body::Ellipse(e) => ellipse_text(*e),
    }
}

/// A set of subpaths and the flags it ends with before `N`.
type Set = (Vec<String>, &'static str);

/// The sets where one set holds every filling section as subpaths: the sections that fill (with neither flag, or with `S` when none strokes), those that only stroke (`F`) and
/// those that are not shown (`F` and `S`).
fn plain_sets(secs: &[Sec]) -> Vec<Set> {
    let filled: Vec<&Sec> = secs.iter().filter(|s| s.fills()).collect();
    let fill_flag = if filled.iter().all(|s| s.class() == Class::FillOnly) { " S" } else { "" };
    vec![
        (filled.iter().flat_map(|s| subpaths_of(s)).collect(), fill_flag),
        (secs.iter().filter(|s| s.class() == Class::StrokeOnly).flat_map(subpaths_of).collect(), " F"),
        (secs.iter().filter(|s| s.class() == Class::Hidden).flat_map(subpaths_of).collect(), " F S"),
    ]
}

/// The second form, where a fill-only and a fill-and-stroke section are both there or a fillable section is several runs not all closed: one set holds every filling section as one
/// ring (`S`), another the runs of every section that strokes (`F`), and the sections that are not shown are the last (`F` and `S`).
fn ring_sets(secs: &[Sec]) -> Vec<Set> {
    vec![
        (secs.iter().filter(|s| s.fills()).map(ring_of).collect(), " S"),
        (secs.iter().filter(|s| s.strokes()).flat_map(subpaths_of).collect(), " F"),
        (secs.iter().filter(|s| s.class() == Class::Hidden).flat_map(subpaths_of).collect(), " F S"),
    ]
}

/// The sets an outline is written in, and whether they are the plain ones.
fn sets_of(secs: &[Sec]) -> (Vec<Set>, bool) {
    let mixed = secs.iter().any(|s| s.class() == Class::FillOnly) && secs.iter().any(|s| s.class() == Class::Both);
    let open_runs = secs.iter().any(|s| s.fills() && !s.fills_as_subpaths());
    if mixed || open_runs { (ring_sets(secs), false) } else { (plain_sets(secs), true) }
}

/// Whether the sections are in the order the plain sets put them in, the one the reader numbers them in: those that fill, then those that only stroke, then those not shown.
fn in_set_order(secs: &[Sec]) -> bool {
    let rank = |s: &Sec| match s.class() {
        Class::Both | Class::FillOnly => 0u8,
        Class::StrokeOnly => 1,
        Class::Hidden => 2,
    };
    secs.windows(2).all(|w| matches!(w, [a, b] if rank(a) <= rank(b)))
}

/// The outline of a new custom shape as the text of `draw:enhanced-path`, or the words of why it is not written (the shape then keeps a
/// rectangle for other programs and its rows as tier 2).
///
/// * **Sets.** A section with neither flag, a section with `NoLine` and one with `NoFill` or `NoShow` each go in a set of their own kind: the sections that fill in one set
///   (with `S` when none of them strokes), the ones that only stroke in a set with `F`, and the ones not shown in a set with `F` and `S`; a section of runs is a subpath for each run.
///   Where a fill-only and a fill-and-stroke section are both there, or a section that fills is several runs not all closed, the fill would no longer match with separate sets: one
///   set holds every section that fills as one ring (`S`) and the next the runs of every section that strokes (`F`).
/// * **Rows.** `MoveTo` and `LineTo`, as `Geometry` rows have been written; `ArcTo` and `EllipticalArcTo` with `C` 0 as `A` or `W` on the ellipse's box, drawn exactly at the
///   size written; a section of one `Ellipse` row with its axes along the frame's as `U`. Any other row is refused.
/// * **Coordinates.** A share of the frame (`Width*k`) as that share; a formula that names another cell or calls a function as its value at the size written; any other cell is
///   refused, as the equation is not written yet.
///
/// The reader makes a section of each subpath; whether it gives back the model's own sections is [`NewOutline::reader_form`].
pub(crate) fn new_outline(sheet: &Sheet) -> Result<NewOutline, &'static str> {
    let sections = geometry_sections(sheet);
    if sections.is_empty() {
        return Err("a custom shape with no geometry");
    }
    let size = Size::of(sheet);
    let mut rows = 0usize;
    let mut secs: Vec<Sec> = Vec::new();
    for (at, section) in sections.iter().enumerate() {
        if at >= MOST_SUBPATHS {
            return Err(WORDS_TOO_MANY);
        }
        secs.push(parse_section(section, at, &size, &mut rows)?);
    }
    let (sets, plain) = sets_of(&secs);
    let subpaths: usize = sets.iter().map(|(s, _)| s.len()).sum();
    if subpaths > MOST_SUBPATHS {
        return Err(WORDS_TOO_MANY);
    }
    let mut path = String::new();
    for (parts, flags) in &sets {
        if parts.is_empty() {
            continue;
        }
        if !path.is_empty() {
            path.push(' ');
        }
        path.push_str(&parts.join(" "));
        path.push_str(flags);
        path.push_str(" N");
    }
    if path.is_empty() {
        return Err("an outline with no row");
    }
    let reader_form = plain && in_set_order(&secs) && secs.iter().all(|s| s.reproduced);
    Ok(NewOutline { path, subpaths, reader_form, rows })
}

/// The connection points of a new custom shape as its geometry's own glue points (the case of rows that are not the four standard ones):
/// `draw:glue-points`, every row of the `Connection` section from row 0 in row order, in the view box `0 0 21600 21600` (a share *s* of the width is
/// 21600 *s*, of the height 21600 (1 − *s*)), and `draw:glue-point-leaving-directions` from `DirX` and `DirY` (degrees clockwise as seen from the x axis, as the reader reads
/// them) when every row has a way out. `None` when the rows are not `IX` 0, 1, 2, ... in order, or a coordinate is not in the forms DiaCraft's basic shapes use
/// (the constant 0, `Width`, `Width*k`, and the same of `Height`): the reader would not number them as the model does, or LibreOffice would not keep them at their share
/// when the frame is resized.
pub(crate) fn glue_points_text(sheet: &Sheet) -> Option<(String, Option<String>)> {
    let section = sheet.sections.iter().find(|s| s.name.as_str() == "Connection")?;
    if section.rows.is_empty() {
        return None;
    }
    let mut points = String::new();
    let mut angles = String::new();
    let mut every_way = true;
    for (k, row) in section.rows.iter().enumerate() {
        if row.ix != u32::try_from(k).ok() || row.del {
            return None;
        }
        let x = share_of(row.cells.get("X"), "Width")?;
        let y = share_of(row.cells.get("Y"), "Height")?;
        let (px, py) = (x * 21600.0, (1.0 - y) * 21600.0);
        if !within(px) || !within(py) {
            return None;
        }
        if !points.is_empty() {
            points.push(' ');
        }
        points.push_str(&format!("{} {}", num(px), num(py)));
        let (dx, dy) = (number(row, "DirX").unwrap_or(0.0), number(row, "DirY").unwrap_or(0.0));
        if dx == 0.0 && dy == 0.0 {
            every_way = false;
            continue;
        }
        // The angle the reader reads back as this way out: its unit vector with y flipped (`read::custom::listed_glue`).
        let degrees = (-dy).atan2(dx).to_degrees().rem_euclid(360.0);
        if !angles.is_empty() {
            angles.push(' ');
        }
        angles.push_str(&num(degrees));
    }
    Some((points, every_way.then_some(angles)))
}

/// `draw:modifiers` with the values at `changes` (place and value) replaced, the others as they were written.
pub(crate) fn modifiers_text(old: &str, changes: &[(usize, f64)]) -> Result<String, &'static str> {
    let mut tokens: Vec<String> = old.split_ascii_whitespace().map(str::to_owned).collect();
    for &(at, value) in changes {
        let slot = tokens.get_mut(at).ok_or("a modifier the file does not list (a handle that names a modifier past the list)")?;
        if !within(value) {
            return Err(WORDS_RANGE);
        }
        *slot = num(value);
    }
    // What is kept as the file wrote it must be what the reader reads as a number, or the save is a text the reader refuses.
    if tokens.iter().any(|t| !crate::read::readback::reads(Ns::Draw, "modifiers", t)) {
        return Err(WORDS_MODIFIER_TEXT);
    }
    Ok(tokens.join(" "))
}

/// The words of the refusal of a `draw:modifiers` that holds a modifier the reader reads as 0 (it is not a number) beside one the save writes.
const WORDS_MODIFIER_TEXT: &str = "a modifier of the file that is not a number is kept as it is written only when no other modifier is written (it reads as 0, and the text would not read back)";

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use diacraft_doc::{CellName, Value};

    use super::*;

    fn row(kind: &str, cells: &[(&str, &str, Option<&str>)]) -> Arc<Row> {
        let mut r = Row { row_type: Some(Arc::from(kind)), ..Row::default() };
        for (name, value, formula) in cells {
            let cell = match formula {
                Some(f) => Cell::read(Some(f), Some(value), None, None, &mut 1_000),
                None => Cell::constant(Value::from_text(value)),
            };
            r.cells.insert(CellName::new(name), cell);
        }
        Arc::new(r)
    }

    fn sheet(rows: Vec<Arc<Row>>) -> Sheet {
        let mut s = Sheet::default();
        let mut section = Section::new("Geometry", Some(0));
        for r in rows {
            section.rows.push(r);
        }
        s.sections.push(Arc::new(section));
        s
    }

    fn rel(kind: &str, x: &str, y: &str) -> Arc<Row> {
        row(kind, &[("X", x, None), ("Y", y, None)])
    }

    const VB: ViewBox = ViewBox { left: 0.0, top: 0.0, width: 1000.0, height: 1000.0 };

    #[test]
    fn a_polygon_gives_integer_points_in_its_view_box_and_leaves_its_closing_row_out() {
        // `0,0 1000,0 500,1000` read as (0, 1), (1, 1), (0.5, 0) and a closing row to (0, 1).
        let s = sheet(vec![rel("RelMoveTo", "0", "1"), rel("RelLineTo", "1", "1"), rel("RelLineTo", "0.5", "0"), rel("RelLineTo", "0", "1")]);
        assert_eq!(points_text(&s, &VB, true).unwrap(), "0,0 1000,0 500,1000");
        assert_eq!(points_text(&s, &VB, false).unwrap(), "0,0 1000,0 500,1000 0,0", "a polyline keeps its last point");
        let moved = sheet(vec![rel("RelMoveTo", "0", "1"), rel("RelLineTo", "1", "1"), rel("RelLineTo", "0.25", "0.25"), rel("RelLineTo", "0", "1")]);
        assert_eq!(points_text(&moved, &VB, true).unwrap(), "0,0 1000,0 250,750");
        let off_grid = sheet(vec![rel("RelMoveTo", "0", "1"), rel("RelLineTo", "0.3333333", "0")]);
        assert!(points_text(&off_grid, &VB, true).unwrap_err().contains("integer grid"));
        assert!(points_text(&sheet(vec![rel("RelMoveTo", "0", "0"), rel("LineTo", "1", "1")]), &VB, true).is_err());
    }

    #[test]
    fn a_path_is_m_l_c_q_and_z_and_a_closed_section_closes() {
        let square = sheet(vec![
            rel("RelMoveTo", "0", "1"),
            rel("RelLineTo", "1", "1"),
            rel("RelLineTo", "1", "0"),
            rel("RelLineTo", "0", "0"),
            rel("RelLineTo", "0", "1"),
        ]);
        assert_eq!(path_text(&square, &VB).unwrap(), "M 0 0 L 1000 0 L 1000 1000 L 0 1000 Z");
        // Five points (the fifth added): the section still closes where it began.
        let five = sheet(vec![
            rel("RelMoveTo", "0", "1"),
            rel("RelLineTo", "1", "1"),
            rel("RelLineTo", "1", "0"),
            rel("RelLineTo", "0.5", "-0.25"),
            rel("RelLineTo", "0", "0"),
            rel("RelLineTo", "0", "1"),
        ]);
        assert_eq!(path_text(&five, &VB).unwrap(), "M 0 0 L 1000 0 L 1000 1000 L 500 1250 L 0 1000 Z");
        let curve = sheet(vec![
            rel("RelMoveTo", "0", "0"),
            row("RelCubBezTo", &[("X", "1", None), ("Y", "1", None), ("A", "0.25", None), ("B", "0", None), ("C", "0.75", None), ("D", "1", None)]),
        ]);
        let mut open = curve.clone();
        Arc::make_mut(&mut open.sections[0]).cells.insert(CellName::new("NoFill"), Cell::constant(Value::from_text("1")));
        assert_eq!(path_text(&open, &VB).unwrap(), "M 0 1000 C 250 1000 750 0 1000 0");
        assert!(
            path_text(&curve, &VB).unwrap_err().contains("fill flag"),
            "an open section that is not marked unfilled is not what the reader would read"
        );
        let arc = sheet(vec![rel("RelMoveTo", "0", "0"), rel("RelEllipticalArcTo", "1", "1")]);
        assert!(path_text(&arc, &VB).is_err(), "an arc is not written");
    }

    #[test]
    fn dia_craft_s_rectangle_and_ellipse_are_written_as_give_them() {
        let w = |k: &str| (k.to_owned(), format!("Width*{k}"));
        let _ = w;
        // MoveTo (0, 0), LineTo (Width, 0), (Width, Height), (0, Height), (0, 0).
        let cell = |v: &'static str, f: Option<&'static str>| (v, f);
        let at = |x: (&'static str, Option<&'static str>), y: (&'static str, Option<&'static str>)| row("", &[("X", x.0, x.1), ("Y", y.0, y.1)]);
        let mut rows = vec![
            at(cell("0", None), cell("0", None)),
            at(cell("1.5", Some("Width")), cell("0", None)),
            at(cell("1.5", Some("Width")), cell("1", Some("Height"))),
            at(cell("0", None), cell("1", Some("Height"))),
            at(cell("0", None), cell("0", None)),
        ];
        for (i, r) in rows.iter_mut().enumerate() {
            Arc::make_mut(r).row_type = Some(Arc::from(if i == 0 { "MoveTo" } else { "LineTo" }));
        }
        assert_eq!(enhanced_path(&sheet(rows)).unwrap(), "M 0 21600 L 21600 21600 21600 0 0 0 0 21600 Z N");
        let ellipse = row(
            "Ellipse",
            &[
                ("X", "0.75", Some("Width*0.5")),
                ("Y", "0.5", Some("Height*0.5")),
                ("A", "1.5", Some("Width")),
                ("B", "0.5", Some("Height*0.5")),
                ("C", "0.75", Some("Width*0.5")),
                ("D", "1", Some("Height")),
            ],
        );
        assert_eq!(enhanced_path(&sheet(vec![ellipse.clone()])).unwrap(), "U 10800 10800 10800 10800 0 360 N");
        // A row in another form (a `Rel` row, an arc, a formula the forms do not cover) is tier 2.
        assert!(enhanced_path(&sheet(vec![rel("RelMoveTo", "0", "0")])).is_err());
        let odd = row("MoveTo", &[("X", "0.3", Some("Width*0.2+0.1 in")), ("Y", "0", None)]);
        assert!(enhanced_path(&sheet(vec![odd])).is_err());
        assert!(enhanced_path(&Sheet::default()).is_err());
    }

    /// A sheet of one move row at the share (`x`, `y`) of the frame.
    fn one_point(x: &str, y: &str) -> Sheet {
        sheet(vec![rel("RelMoveTo", x, y)])
    }

    #[test]
    fn a_polygon_of_one_point_keeps_it_and_a_point_that_is_off_the_grid_by_no_more_than_a_millionth_of_the_view_box_is_on_it() {
        // One row is its own last: the closing row is only left out where there is a first row to go back to.
        assert_eq!(points_text(&one_point("0", "1"), &VB, true).unwrap(), "0,0");
        // 500.0001 is a tenth of a thousandth off 500: inside the tolerance of a millionth of 1,000; 500.002 is two thousandths off: outside it.
        assert_eq!(points_text(&one_point("0.5000001", "0"), &VB, false).unwrap(), "500,1000");
        assert!(points_text(&one_point("0.500002", "0"), &VB, false).is_err());
        assert_eq!(points_text(&one_point("0", "0.9999999"), &VB, false).unwrap(), "0,0");
        assert!(points_text(&one_point("0", "0.999998"), &VB, false).is_err());
    }

    #[test]
    fn each_of_the_four_conditions_of_a_point_refuses_it_alone() {
        // x off the grid and y on it, and the reverse; x or y past a thousand million million with the other on the grid.
        assert!(points_text(&one_point("0.0005", "0"), &VB, false).unwrap_err().contains("integer grid"));
        assert!(points_text(&one_point("0", "0.9995"), &VB, false).unwrap_err().contains("integer grid"));
        assert!(points_text(&one_point("2000000000000", "1"), &VB, false).is_err(), "x of 2e15");
        assert!(points_text(&one_point("0", "-1999999999999"), &VB, false).is_err(), "y of 2e15");
        // The limit is 1e15 itself, which is allowed.
        assert_eq!(points_text(&one_point("1000000000000", "1"), &VB, false).unwrap(), "1000000000000000,0");
        assert_eq!(points_text(&one_point("0", "-999999999999"), &VB, false).unwrap(), "0,1000000000000000");
    }

    #[test]
    fn a_point_exactly_the_tolerance_off_the_grid_is_on_it() {
        // A view box 500,000 wide: the tolerance is 0.5, and a point at 0.5 is exactly that far from the grid (rounding goes to 1).
        let wide = ViewBox { left: 0.5, top: 0.5, width: 500_000.0, height: 500_000.0 };
        assert_eq!(1e-6 * wide.width.max(wide.height), 0.5);
        assert_eq!(points_text(&one_point("0", "1"), &wide, false).unwrap(), "1,1", "x and y at 0.5 are on the grid at the tolerance");
        let tall = ViewBox { left: 0.5, top: 0.0, width: 500_000.0, height: 500_000.0 };
        assert_eq!(points_text(&one_point("0", "1"), &tall, false).unwrap(), "1,0", "x alone, at the tolerance");
        let flat = ViewBox { left: 0.0, top: 0.5, width: 500_000.0, height: 500_000.0 };
        assert_eq!(points_text(&one_point("0", "1"), &flat, false).unwrap(), "0,1", "y alone, at the tolerance");
    }

    #[test]
    fn a_path_writes_quadratic_curves_and_goes_back_to_its_start_in_the_middle_without_closing_there() {
        let mut quad =
            sheet(vec![rel("RelMoveTo", "0", "0"), row("RelQuadBezTo", &[("X", "1", None), ("Y", "1", None), ("A", "0.5", None), ("B", "0", None)])]);
        Arc::make_mut(&mut quad.sections[0]).cells.insert(CellName::new("NoFill"), Cell::constant(Value::from_text("1")));
        assert_eq!(path_text(&quad, &VB).unwrap(), "M 0 1000 Q 500 1000 1000 0");
        // A section that passes its first point, goes on, and ends elsewhere: its lines are lines and it does not close at the pass, nor at its end.
        let mut loopy = sheet(vec![rel("RelMoveTo", "0", "1"), rel("RelLineTo", "1", "1"), rel("RelLineTo", "0", "1"), rel("RelLineTo", "1", "0")]);
        Arc::make_mut(&mut loopy.sections[0]).cells.insert(CellName::new("NoFill"), Cell::constant(Value::from_text("1")));
        assert_eq!(path_text(&loopy, &VB).unwrap(), "M 0 0 L 1000 0 L 0 0 L 1000 1000");
        // A section whose first row is a line has no first point.
        assert_eq!(path_text(&sheet(vec![rel("RelLineTo", "0", "1")]), &VB).unwrap_err(), WORDS_FORM);
        assert_eq!(path_text(&sheet(vec![rel("RelLineTo", "0", "1"), rel("RelLineTo", "1", "1")]), &VB).unwrap_err(), WORDS_FORM);
    }

    #[test]
    fn a_custom_shape_path_of_one_point_is_a_move_and_one_of_two_is_open() {
        let at = |kind: &str, x: (&str, Option<&str>), y: (&str, Option<&str>)| row(kind, &[("X", x.0, x.1), ("Y", y.0, y.1)]);
        let (zero, wide, high) = (("0", None), ("1.5", Some("Width")), ("1", Some("Height")));
        assert_eq!(enhanced_path(&sheet(vec![at("MoveTo", zero, zero)])).unwrap(), "M 0 21600 N");
        assert_eq!(enhanced_path(&sheet(vec![at("MoveTo", zero, zero), at("LineTo", wide, high)])).unwrap(), "M 0 21600 L 21600 0 N");
    }

    /// A coordinate or a modifier past 1e15, or that is not a number, is refused in words, and each of a point's two numbers
    /// is looked at by itself, in a path, in a custom shape's path and in a modifier; 1e15 itself is written, whole.
    #[test]
    fn a_coordinate_or_a_modifier_past_what_is_written_is_refused_each_number_by_itself() {
        let range = |r: Result<String, &'static str>| assert!(r.as_ref().is_err_and(|w| w.contains("too large to write")), "{r:?}");
        for (x, y) in [("1e300", "0"), ("0", "1e300"), ("-1e300", "0"), ("0", "-1e300")] {
            range(path_text(&sheet(vec![rel("RelMoveTo", x, y)]), &VB));
        }
        let curve = |a: &str, b: &str, c: &str, d: &str| {
            sheet(vec![
                rel("RelMoveTo", "0", "0"),
                row("RelCubBezTo", &[("X", "1", None), ("Y", "1", None), ("A", a, None), ("B", b, None), ("C", c, None), ("D", d, None)]),
            ])
        };
        for control in [("1e300", "0", "0", "0"), ("0", "1e300", "0", "0"), ("0", "0", "1e300", "0"), ("0", "0", "0", "1e300")] {
            range(path_text(&curve(control.0, control.1, control.2, control.3), &VB));
        }
        let at = |kind: &str, x: (&str, Option<&str>), y: (&str, Option<&str>)| row(kind, &[("X", x.0, x.1), ("Y", y.0, y.1)]);
        let (zero, huge) = (("0", None), ("1", Some("Width*1e300")));
        range(enhanced_path(&sheet(vec![at("MoveTo", zero, zero), at("LineTo", huge, zero)])));
        range(enhanced_path(&sheet(vec![at("MoveTo", zero, zero), at("LineTo", zero, ("1", Some("Height*1e300")))])));
        range(enhanced_path(&sheet(vec![at("MoveTo", zero, zero), at("LineTo", ("1", Some("Width*1e12")), zero)])));
        assert_eq!(
            enhanced_path(&sheet(vec![at("MoveTo", zero, zero), at("LineTo", ("1", Some("Width*1e10")), zero)])).unwrap(),
            "M 0 21600 L 216000000000000 21600 N"
        );
        for bad in [f64::NAN, f64::INFINITY, 1e300, -1e16] {
            assert!(modifiers_text("1 2", &[(1, bad)]).is_err_and(|w| w.contains("too large to write")), "{bad}");
        }
        assert_eq!(modifiers_text("1 2", &[(1, 1e15)]).unwrap(), "1 1000000000000000");
        assert_eq!(modifiers_text("1 2", &[(1, -1e15)]).unwrap(), "1 -1000000000000000");
    }

    #[test]
    fn modifiers_are_replaced_in_their_places_and_the_rest_is_as_written() {
        assert_eq!(modifiers_text("5400 -10 7", &[(1, 2700.0)]).unwrap(), "5400 2700 7");
        assert_eq!(modifiers_text("0.5", &[(0, 0.25)]).unwrap(), "0.25");
        assert!(modifiers_text("1", &[(3, 1.0)]).is_err());
        // A modifier of the file that is no number is read as 0 with a warning, and kept as written it is a text the reader refuses: a save that writes another modifier
        // beside it is refused in words, and not given to the check of the bytes (the fuzz target found it with a handle's drag on a file that held an `x`).
        let refused = modifiers_text("5400 16200 x", &[(0, 0.0)]);
        assert!(refused.as_ref().is_err_and(|w| w.contains("is not a number")), "{refused:?}");
        assert_eq!(modifiers_text("5400 16200 x", &[(2, 3.0)]).unwrap(), "5400 16200 3", "the one that is no number is written over");
        assert_eq!(modifiers_text("5400 1e3 .5 -2", &[(0, 1.0)]).unwrap(), "1 1e3 .5 -2", "any text the reader reads as a number is kept as written");
        assert_eq!(num(-0.0), "0");
        assert_eq!(num(0.1 + 0.2), "0.3");
        assert_eq!(num(250.0), "250");
    }

    // ---- the outline of a new custom shape: sets and arcs ---------------------------------------------------------------------------------

    /// A sheet 2 by 1 inches whose `Geometry` sections are `sections`: the frame the arcs and the coordinates that are values are worked out in.
    fn framed(sections: Vec<Section>) -> Sheet {
        let mut s = Sheet::default();
        for (name, v) in [("Width", "2"), ("Height", "1")] {
            s.cells.insert(CellName::new(name), Cell::read(None, Some(v), Some("IN"), None, &mut 1_000));
        }
        for g in sections {
            s.sections.push(Arc::new(g));
        }
        s
    }

    /// A `Geometry` section `ix` with the flags `flags` set (constants of 1) and rows of the shares given: (`kind`, x, y), x and y each a share of the frame in eighths.
    fn eighths(ix: u32, flags: &[&str], rows: &[(&str, u32, u32)]) -> Section {
        let share = |axis: &str, size: f64, n: u32| match n {
            0 => (String::from("0"), None),
            8 => (size.to_string(), Some(axis.to_owned())),
            n => ((f64::from(n) / 8.0 * size).to_string(), Some(format!("{axis}*{}", f64::from(n) / 8.0))),
        };
        let mut section = Section::new("Geometry", Some(ix));
        for f in flags {
            section.cells.insert(CellName::new(f), Cell::constant(Value::from_text("1")));
        }
        for (i, (kind, x, y)) in rows.iter().enumerate() {
            let (xv, xf) = share("Width", 2.0, *x);
            let (yv, yf) = share("Height", 1.0, *y);
            let mut r = Row { row_type: Some(Arc::from(*kind)), ix: Some(u32::try_from(i + 1).unwrap()), ..Row::default() };
            r.cells.insert(CellName::new("X"), Cell::read(xf.as_deref(), Some(&xv), Some("IN"), None, &mut 1_000));
            r.cells.insert(CellName::new("Y"), Cell::read(yf.as_deref(), Some(&yv), Some("IN"), None, &mut 1_000));
            section.rows.push(Arc::new(r));
        }
        section
    }

    const SQUARE: [(&str, u32, u32); 5] = [("MoveTo", 0, 0), ("LineTo", 8, 0), ("LineTo", 8, 8), ("LineTo", 0, 8), ("LineTo", 0, 0)];
    const LINE: [(&str, u32, u32); 2] = [("MoveTo", 0, 4), ("LineTo", 8, 4)];

    #[test]
    fn a_formula_that_names_another_cell_or_a_function_is_told_from_a_share_and_from_a_length_beside_the_size() {
        let cell = |f: &str| Cell::read(Some(f), Some("1"), Some("IN"), None, &mut 1_000);
        for named in ["MIN(Width,Height)*0.1", "Width*0.5+Height*0.1", "Width-User.Radius", "Sheet.5!Width", "ABS(Width)", "Height*0.5", "Width[2]"] {
            assert!(names_another(&cell(named), "Width"), "{named}");
        }
        for own in ["Width", "Width*0.5", "Width*0.5+0.25", "-Width*2+0.1 in", "0.3", "(Width+1)/2"] {
            assert!(!names_another(&cell(own), "Width"), "{own}");
        }
        assert!(names_another(&cell("Width*0.5"), "Height"), "the other axis's size is another cell");
        assert!(!names_another(&Cell::constant(Value::from_text("1")), "Width"), "a constant names nothing");
        assert!(!names_another(&cell("(("), "Width"), "a formula that does not parse is not one that names another cell");
    }

    #[test]
    fn a_bow_and_three_points_make_the_circle_and_the_ellipse_the_arc_is_drawn_on() {
        // From (0, 0.5) to (2, 0.5) with the bow -0.5 (to the left of the way, clockwise), 0.5 (to the right, counterclockwise) and 0 (a chord).
        let (centre, radii, ccw) = sagitta_circle((0.0, 0.5), (2.0, 0.5), -0.5).unwrap();
        assert!((centre.0 - 1.0).abs() < 1e-12 && (centre.1 + 0.25).abs() < 1e-12 && radii == (1.25, 1.25) && !ccw, "{centre:?} {radii:?}");
        let (centre, radii, ccw) = sagitta_circle((0.0, 0.5), (2.0, 0.5), 0.5).unwrap();
        assert!((centre.0 - 1.0).abs() < 1e-12 && (centre.1 - 1.25).abs() < 1e-12 && radii == (1.25, 1.25) && ccw, "{centre:?}");
        assert!(sagitta_circle((0.0, 0.5), (2.0, 0.5), 0.0).is_none());
        assert!(sagitta_circle((0.0, 0.5), (2.0, 0.5), 1e-10).is_none(), "a bow of a thousand-millionth of the chord is a chord");
        assert!(sagitta_circle((1.0, 1.0), (1.0, 1.0), 0.5).is_none(), "no chord");
        assert!(sagitta_circle((0.0, 0.0), (1.0, 0.0), f64::NAN).is_none());
        // A bow past half the chord: the arc is more than half a circle and its centre is on the side it bulges to.
        let (centre, _, ccw) = sagitta_circle((0.0, 0.0), (0.0, 1.0), -1.0).unwrap();
        assert!(centre.0 < 0.0 && !ccw, "{centre:?}");
        // The ellipse through three points of a half circle, and of a flat half ellipse whose ratio is 2.
        let (centre, radii, ccw) = ellipse_through((0.0, 0.0), (1.0, 1.0), (2.0, 0.0), 1.0).unwrap();
        assert!((centre.0 - 1.0).abs() < 1e-12 && centre.1.abs() < 1e-12 && (radii.0 - 1.0).abs() < 1e-12 && (radii.1 - 1.0).abs() < 1e-12 && !ccw);
        let (centre, radii, ccw) = ellipse_through((0.0, 0.0), (1.0, 0.5), (2.0, 0.0), 2.0).unwrap();
        assert!((centre.0 - 1.0).abs() < 1e-12 && centre.1.abs() < 1e-12 && (radii.0 - 1.0).abs() < 1e-12 && (radii.1 - 0.5).abs() < 1e-12 && !ccw);
        let (_, _, ccw) = ellipse_through((2.0, 0.0), (1.0, 0.5), (0.0, 0.0), 2.0).unwrap();
        assert!(ccw, "the same arc the other way goes counterclockwise");
        // Three points on a line, two the same, and a ratio that is not a number: no ellipse.
        assert!(ellipse_through((0.0, 0.0), (1.0, 1.0), (2.0, 2.0), 1.0).is_none());
        assert!(ellipse_through((0.0, 0.0), (0.0, 0.0), (1.0, 0.0), 1.0).is_none());
        assert!(ellipse_through((0.0, 0.0), (1.0, 1.0), (2.0, 0.0), f64::NAN).is_none());
    }

    #[test]
    fn a_new_outline_writes_a_set_for_each_kind_of_section_and_a_subpath_for_each_run() {
        // A closed square and an open line with `NoFill`.
        let o = new_outline(&framed(vec![eighths(0, &[], &SQUARE), eighths(1, &["NoFill"], &LINE)])).unwrap();
        assert_eq!(o.path, "M 0 21600 L 21600 21600 21600 0 0 0 0 21600 Z N M 0 10800 L 21600 10800 F N");
        assert_eq!((o.subpaths, o.rows, o.reader_form), (2, 7, true));
        // A section with no line is a set with `S`; one not shown is the last, with `F` and `S`; one that does not fill goes before it.
        let o = new_outline(&framed(vec![eighths(0, &["NoLine"], &SQUARE), eighths(1, &["NoFill"], &LINE), eighths(2, &["NoShow"], &LINE)])).unwrap();
        assert_eq!(o.path, "M 0 21600 L 21600 21600 21600 0 0 0 0 21600 Z S N M 0 10800 L 21600 10800 F N M 0 10800 L 21600 10800 F S N");
        assert!(o.reader_form);
        // Sections that are not in the order of the sets are written in it, and are not what the reader numbers them as.
        let o = new_outline(&framed(vec![eighths(0, &["NoFill"], &LINE), eighths(1, &[], &SQUARE)])).unwrap();
        assert_eq!(o.path, "M 0 21600 L 21600 21600 21600 0 0 0 0 21600 Z N M 0 10800 L 21600 10800 F N");
        assert!(!o.reader_form);
        // One section of two open runs is a ring with `S` and its runs with `F`.
        let runs = [("MoveTo", 0, 0), ("LineTo", 8, 0), ("LineTo", 8, 8), ("MoveTo", 0, 4), ("LineTo", 4, 4)];
        let o = new_outline(&framed(vec![eighths(0, &[], &runs)])).unwrap();
        assert_eq!(
            o.path,
            "M 0 21600 L 21600 21600 21600 0 L 0 10800 L 10800 10800 Z S N M 0 21600 L 21600 21600 21600 0 M 0 10800 L 10800 10800 F N"
        );
        assert_eq!((o.subpaths, o.reader_form), (3, false));
        // Runs that are all closed stay in the set as subpaths, and are not one section for the reader.
        let closed = [("MoveTo", 0, 0), ("LineTo", 4, 0), ("LineTo", 0, 0), ("MoveTo", 4, 4), ("LineTo", 8, 4), ("LineTo", 4, 4)];
        let o = new_outline(&framed(vec![eighths(0, &[], &closed)])).unwrap();
        assert_eq!(o.path, "M 0 21600 L 10800 21600 0 21600 Z M 10800 10800 L 21600 10800 10800 10800 Z N");
        assert_eq!((o.subpaths, o.reader_form), (2, false));
        // A fill-only section beside one that fills and strokes: the ring set and the strokes.
        let o = new_outline(&framed(vec![eighths(0, &[], &SQUARE), eighths(1, &["NoLine"], &SQUARE)])).unwrap();
        assert_eq!(
            o.path,
            "M 0 21600 L 21600 21600 21600 0 0 0 0 21600 Z M 0 21600 L 21600 21600 21600 0 0 0 0 21600 Z S N M 0 21600 L 21600 21600 21600 0 0 0 0 21600 Z F N"
        );
        assert!(!o.reader_form);
    }

    #[test]
    fn a_section_is_what_the_reader_gives_back_only_in_the_forms_it_writes() {
        let sheet_of = |section: Section| framed(vec![section]);
        let plain = |section: Section| new_outline(&sheet_of(section)).unwrap().reader_form;
        assert!(plain(eighths(0, &[], &SQUARE)));
        assert!(plain(eighths(0, &["NoFill"], &LINE)));
        // One flag at most: a section with neither fill nor line is read as `NoShow`, and a hidden one with another flag as `NoShow` alone.
        assert!(!plain(eighths(0, &["NoFill", "NoLine"], &LINE)));
        assert!(!plain(eighths(0, &["NoShow", "NoFill"], &LINE)));
        // A section that is not numbered as the reader numbers it.
        assert!(!plain(eighths(1, &[], &SQUARE)));
        // A cell beside the flags, a formula on a flag, a flag that is not 0 or 1, and `NoSnap` that is 1: the reader gives none of them back.
        let with = |name: &str, formula: Option<&str>, value: &str| {
            let mut s = eighths(0, &[], &SQUARE);
            s.cells.insert(CellName::new(name), Cell::read(formula, Some(value), None, None, &mut 1_000));
            plain(s)
        };
        assert!(with("NoSnap", None, "0") && with("NoQuickDrag", None, "0"), "the two the reader never makes, at 0, are its default");
        assert!(!with("NoSnap", None, "1") && !with("Comment", None, "1") && !with("NoFill", Some("1-1"), "0") && !with("NoFill", None, "2"));
        // A row the reader gives back with another formula (`Width*1`, `Width*0`), a coordinate that is a value, an arc.
        let mut starred = eighths(0, &[], &SQUARE);
        let row = Arc::make_mut(starred.rows.get_mut(1).unwrap());
        row.cells.insert(CellName::new("X"), Cell::read(Some("Width*1"), Some("2"), Some("IN"), None, &mut 1_000));
        assert!(!plain(starred.clone()), "the reader reads `Width*1` back as `Width`");
        assert!(new_outline(&sheet_of(starred)).is_ok(), "and the path is still written");
        let with_arc = {
            let mut s = eighths(0, &[], &[("MoveTo", 0, 0), ("LineTo", 8, 0)]);
            let mut r = Row { row_type: Some(Arc::from("EllipticalArcTo")), ix: Some(3), ..Row::default() };
            for (n, f, v) in [("X", "Width", "2"), ("Y", "Height*0.5", "0.5"), ("A", "Width*0.5", "1"), ("B", "Height*0.8", "0.8")] {
                r.cells.insert(CellName::new(n), Cell::read(Some(f), Some(v), Some("IN"), None, &mut 1_000));
            }
            for (n, v) in [("C", "0"), ("D", "1")] {
                r.cells.insert(CellName::new(n), Cell::constant(Value::from_text(v)));
            }
            s.rows.push(Arc::new(r));
            s
        };
        let o = new_outline(&sheet_of(with_arc)).unwrap();
        assert!(o.path.contains(" W ") || o.path.contains(" A "), "{}", o.path);
        assert!(!o.reader_form, "an arc comes back as another row");
    }

    #[test]
    fn a_coordinate_that_is_a_value_is_written_at_the_size_and_a_length_beside_the_size_is_refused() {
        let at = |x: &str, f: Option<&str>| {
            let mut s = eighths(0, &[], &[("MoveTo", 0, 0), ("LineTo", 8, 0)]);
            let row = Arc::make_mut(s.rows.get_mut(1).unwrap());
            row.cells.insert(CellName::new("X"), Cell::read(f, Some(x), Some("IN"), None, &mut 1_000));
            new_outline(&framed(vec![s]))
        };
        assert_eq!(at("0.5", Some("MIN(Width,Height)*0.5")).unwrap().path, "M 0 21600 L 5400 21600 N");
        assert_eq!(at("1", Some("Width*0.25+Height*0.5")).unwrap().path, "M 0 21600 L 10800 21600 N");
        assert!(!at("0.5", Some("MIN(Width,Height)*0.5")).unwrap().reader_form);
        for words in [at("0.5", Some("Width*0.25+0")), at("0.5", None), at("0.5", Some("Width*0.25+0.5 in"))] {
            assert_eq!(words.unwrap_err(), WORDS_FORM);
        }
        // A size that is not a length above 0 is no size to work a value out in.
        let mut s = framed(vec![eighths(0, &[], &[("MoveTo", 0, 0)])]);
        s.cells.insert(CellName::new("Width"), Cell::read(None, Some("0"), Some("IN"), None, &mut 1_000));
        let mut sec = eighths(0, &[], &[("MoveTo", 0, 0), ("LineTo", 8, 0)]);
        let row = Arc::make_mut(sec.rows.get_mut(1).unwrap());
        row.cells.insert(CellName::new("X"), Cell::read(Some("MIN(Width,Height)*0.5"), Some("0.5"), Some("IN"), None, &mut 1_000));
        s.sections = vec![Arc::new(sec)];
        assert_eq!(new_outline(&s).unwrap_err(), WORDS_FORM);
    }

    #[test]
    fn a_new_outline_is_refused_in_words_for_a_row_that_is_not_one_it_writes_and_past_what_the_reader_reads() {
        let refused = |rows: &[(&str, u32, u32)]| new_outline(&framed(vec![eighths(0, &[], rows)])).unwrap_err();
        assert_eq!(refused(&[("LineTo", 8, 0)]), WORDS_FORM, "a section begins with its move");
        assert_eq!(refused(&[("MoveTo", 0, 0), ("RelLineTo", 8, 0)]), WORDS_FORM);
        assert_eq!(refused(&[("MoveTo", 0, 0), ("NURBSTo", 8, 0)]), WORDS_FORM);
        assert!(refused(&[]).contains("no row"));
        assert!(new_outline(&Sheet::default()).unwrap_err().contains("no geometry"));
        let mut deleted = eighths(0, &[], &SQUARE);
        Arc::make_mut(deleted.rows.get_mut(1).unwrap()).del = true;
        assert_eq!(new_outline(&framed(vec![deleted])).unwrap_err(), WORDS_FORM);
        let mut gone = eighths(0, &[], &SQUARE);
        gone.del = true;
        assert_eq!(new_outline(&framed(vec![gone])).unwrap_err(), WORDS_FORM);
        // An arc row whose axis is turned, whose ratio is not above 0 or that has no pen.
        let arc = |c: &str, d: &str| {
            let mut s = eighths(0, &[], &[("MoveTo", 0, 0)]);
            let mut r = Row { row_type: Some(Arc::from("EllipticalArcTo")), ix: Some(2), ..Row::default() };
            for (n, f, v) in [("X", Some("Width"), "2"), ("Y", None, "0"), ("A", Some("Width*0.5"), "1"), ("B", Some("Height"), "1")] {
                r.cells.insert(CellName::new(n), Cell::read(f, Some(v), Some("IN"), None, &mut 1_000));
            }
            r.cells.insert(CellName::new("C"), Cell::constant(Value::from_text(c)));
            r.cells.insert(CellName::new("D"), Cell::constant(Value::from_text(d)));
            s.rows.push(Arc::new(r));
            new_outline(&framed(vec![s]))
        };
        assert!(arc("0", "1").is_ok());
        assert_eq!(arc("0.5", "1").unwrap_err(), WORDS_FORM);
        assert_eq!(arc("0", "0").unwrap_err(), WORDS_FORM);
        assert_eq!(arc("0", "-2").unwrap_err(), WORDS_FORM);
        // Past what the reader reads in one shape: more sections than subpaths it makes, and more rows than a path is written from.
        let many: Vec<Section> = (0..MOST_SUBPATHS + 1).map(|i| eighths(u32::try_from(i).unwrap(), &[], &[("MoveTo", 0, 0)])).collect();
        assert_eq!(new_outline(&framed(many)).unwrap_err(), WORDS_TOO_MANY);
        let rows: Vec<(&str, u32, u32)> = std::iter::once(("MoveTo", 0, 0)).chain(std::iter::repeat_n(("LineTo", 1, 1), MOST_ROWS)).collect();
        assert_eq!(new_outline(&framed(vec![eighths(0, &[], &rows)])).unwrap_err(), WORDS_TOO_MANY);
        let fewer: Vec<(&str, u32, u32)> = std::iter::once(("MoveTo", 0, 0)).chain(std::iter::repeat_n(("LineTo", 1, 1), MOST_ROWS - 1)).collect();
        assert!(new_outline(&framed(vec![eighths(0, &[], &fewer)])).is_ok(), "the last row that is written");
        // A section of runs that are closed and many: more subpaths than the reader makes, though there is only one section.
        let mut runs: Vec<(&str, u32, u32)> = Vec::new();
        for _ in 0..MOST_SUBPATHS / 2 + 1 {
            runs.extend([("MoveTo", 0, 0), ("LineTo", 8, 0), ("LineTo", 0, 0), ("MoveTo", 4, 4), ("LineTo", 8, 4), ("LineTo", 4, 4)]);
        }
        assert_eq!(new_outline(&framed(vec![eighths(0, &[], &runs)])).unwrap_err(), WORDS_TOO_MANY);
    }

    #[test]
    fn a_whole_ellipse_is_u_from_0_to_360_whatever_its_size_and_a_turned_one_is_refused() {
        // The cells of an `Ellipse` row: (name, value, formula).
        let ellipse = |cells: [(&str, &str, &str); 6]| {
            let mut s = Section::new("Geometry", Some(0));
            let mut r = Row { row_type: Some(Arc::from("Ellipse")), ix: Some(1), ..Row::default() };
            for (n, v, f) in cells {
                r.cells.insert(CellName::new(n), Cell::read(Some(f), Some(v), Some("IN"), None, &mut 1_000));
            }
            s.rows.push(Arc::new(r));
            new_outline(&framed(vec![s]))
        };
        let (x, y, b) = (("X", "1", "Width*0.5"), ("Y", "0.5", "Height*0.5"), ("B", "0.5", "Height*0.5"));
        let whole = ellipse([x, y, ("A", "2", "Width"), b, ("C", "1", "Width*0.5"), ("D", "1", "Height")]).unwrap();
        assert_eq!(whole.path, "U 10800 10800 10800 10800 0 360 N");
        assert!(whole.reader_form, "DiaCraft's own form comes back as the same row");
        let small = ellipse([x, y, ("A", "1.5", "Width*0.75"), b, ("C", "1", "Width*0.5"), ("D", "0.75", "Height*0.75")]).unwrap();
        assert_eq!(small.path, "U 10800 10800 5400 5400 0 360 N");
        assert!(!small.reader_form);
        // The point on the other axis is off its line, and there is no half-axis.
        let turned = ellipse([x, y, ("A", "2", "Width"), b, ("C", "1.5", "Width*0.75"), ("D", "1", "Height")]);
        assert_eq!(turned.unwrap_err(), WORDS_FORM);
        let flat = ellipse([x, y, ("A", "1", "Width*0.5"), b, ("C", "1", "Width*0.5"), ("D", "1", "Height")]);
        assert_eq!(flat.unwrap_err(), WORDS_FORM);
    }
}
