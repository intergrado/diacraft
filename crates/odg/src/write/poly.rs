//! A one-dimensional shape whose geometry is not the plain line, written as the polyline it draws.
//!
//! A segment is a `draw:line` of its two ends when its `Geometry` is the line between them ([`super::shapes`]'s `is_line_outline`). A segment whose geometry is its own (the Comm-link's
//! zigzag, the Self Message's loop: masters of the `Fixed1D` kind, whose geometry is not a route and which a reroute leaves alone) was written as that line too, with the geometry carried in
//! DiaCraft's data alone, so that another program drew a straight line where DiaCraft draws a zigzag. **It is written as the outline it draws**: a `draw:polyline` of the points of the
//! geometry on the page, or a `draw:path` when a row is a curve, with the style of the segment (its stroke, its dash, its line ends) as a line has them.
//!
//! * **The place.** The rows of the section are in the segment's own frame (its x along the line from its pin, its y across it): [`segs_of`] takes them to the page by the map of the shape's
//!   frame cells (`own::local_to_parent`, the same map the renderer draws them by) and of the groups around it, so a pin that is not the middle of the ends, a turn and a flip are the
//!   shape's own. The page is measured from its top left in hundredths of a millimetre, as LibreOffice writes a polyline, and [`placed`] gives the box of the outline in those units, the
//!   `svg:viewBox` of that box (`0 0 w h`), and the data (`draw:points` or `svg:d`) in it, each point a whole number as the format asks.
//! * **Only a segment glued at no end**. A polyline has no glue another program keeps: LibreOffice left the end of a Comm-link where it was when a rectangle it was glued to in
//!   DiaCraft moved. A segment of this kind that is glued at an end is a `draw:connector` of the type `line` glued to its shapes, with these points as its path and the geometry in DiaCraft's data
//!   (`Editor::connector_text`); LibreOffice routes it its own way and keeps it glued.
//! * **What is not written so** is written as before, a `draw:line` and the geometry in DiaCraft's data: a geometry of more than one section, of a row that is no line or cubic curve, of a
//!   section that has a fill, no line or no show, of one row or fewer, and one whose box has no width or no height (every point on a line level with an axis: a view box of no size is no view
//!   box).
//! * **What the reader gives back** is the polyline as a 2-D shape of its box; DiaCraft's data carries the segment's frame over it (the ends, the pose they work out, the geometry), as
//!   `native::Frame::Segment` says, so that a read gives the same shape back and a polyline another program changed is told by its box (`ext::tier3`).

use diacraft_doc::{Section, Sheet};

use super::own;
use super::route::{Seg, control_points, hundredths};
use crate::read::Affine;

/// The cells a plain outline's section holds: its five flags. A section with any other cell of its own is not one whose form a polyline says whole.
const FLAGS: [&str; 5] = ["NoFill", "NoLine", "NoShow", "NoSnap", "NoQuickDrag"];

/// The number a flag of `section` holds (`false` for none): `None` for a cell that is no flag of the five, or that holds no number.
fn flag(section: &Section, name: &str) -> Option<bool> {
    match section.cells.get(name) {
        None => Some(false),
        Some(cell) => cell.value.as_ref()?.number().filter(|n| n.is_finite()).map(|n| n == 1.0),
    }
}

/// Whether the section is a stroked outline with no fill that is shown: `NoFill` 1, `NoLine` and `NoShow` 0 (or none), and no cell of its own but the flags.
fn plain_flags(section: &Section) -> Option<()> {
    if section.cells.keys().any(|name| !FLAGS.contains(&name.as_str())) {
        return None;
    }
    // (A flag that is not a number is none: `flag` is `None`.)
    let (fill, line, shown) = (flag(section, "NoFill")?, flag(section, "NoLine")?, flag(section, "NoShow")?);
    (fill && !line && !shown && flag(section, "NoSnap").is_some() && flag(section, "NoQuickDrag").is_some()).then_some(())
}

/// The outline of the segment whose own sheet is `sheet`, on the page (inches, y up), as segments of a path: the rows of its one `Geometry` section taken through the map of the shape's frame
/// and of `outer`, the groups around it. `None` for a geometry this writer does not say (the module documentation). Each row is a step of `steps`, counted before it is looked at.
pub(crate) fn segs_of(sheet: &Sheet, outer: &Affine, steps: &mut u64) -> Option<Vec<Seg>> {
    let mut found = sheet.sections.iter().filter(|s| s.name.as_str() == "Geometry");
    let section = found.next()?;
    if found.next().is_some() || section.ix != Some(0) {
        return None;
    }
    plain_flags(section)?;
    let map = own::compose(outer, &own::local_to_parent(sheet)?);
    *steps = steps.saturating_add(section.rows.len() as u64);
    let mut out: Vec<Seg> = Vec::new();
    out.try_reserve(section.rows.len()).ok()?;
    for (i, row) in section.rows.iter().enumerate() {
        if row.del {
            return None;
        }
        let at = |name: &str| row.cells.get(name).and_then(|cell| cell.value.as_ref()?.number()).filter(|v| v.is_finite());
        let end = map.apply(at("X")?, at("Y")?);
        out.push(match (row.row_type.as_deref(), i) {
            (Some("MoveTo"), 0) => Seg::Move(end),
            (Some("LineTo"), 1..) => Seg::Line(end),
            (Some("NURBSTo"), 1..) => {
                let [x1, y1, x2, y2] = control_points(row)?;
                Seg::Curve(map.apply(x1, y1), map.apply(x2, y2), end)
            }
            _ => return None,
        });
    }
    (out.len() >= 2).then_some(out)
}

/// An outline placed on a page: its box and its data, in hundredths of a millimetre.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Placed {
    /// The box's left and top edges, from the page's top left.
    pub left: i64,
    pub top: i64,
    /// The box's size: both more than 0.
    pub width: i64,
    pub height: i64,
    /// `draw:points` (a polyline), or `svg:d` (a path), in the view box: from the corner of the box, each coordinate a whole number.
    pub data: String,
    /// `svg:viewBox`: `0 0 <width> <height>`.
    pub view_box: String,
    /// The data is a path's: a row was a curve.
    pub path: bool,
}

/// The outline `route` (inches, y up) placed on a page `height` inches high. `None` for a route that does not begin with its one move, for a point that cannot be written (not a number, or past
/// what a path can hold), and for a box with no width or no height. **A curve's box is the box of its control points** (a view box maps the box to the frame whatever it is, so the outline
/// draws as it is; another program that saves the file makes the box of the curve's own extent, and the fingerprint of the data then tells the file as changed).
pub(crate) fn placed(route: &[Seg], height: f64) -> Option<Placed> {
    if route.len() < 2 || !matches!(route.first(), Some(Seg::Move(_))) {
        return None;
    }
    // Every point in order, the control points of a curve among them: the box is theirs.
    let mut points: Vec<(i64, i64)> = Vec::new();
    points.try_reserve(route.len().saturating_mul(3)).ok()?;
    let mut path = false;
    for (i, seg) in route.iter().enumerate() {
        match *seg {
            Seg::Move(p) if i == 0 => points.push(hundredths(p, height)?),
            Seg::Move(_) => return None,
            Seg::Line(p) => points.push(hundredths(p, height)?),
            Seg::Curve(a, b, p) => {
                path = true;
                for q in [a, b, p] {
                    points.push(hundredths(q, height)?);
                }
            }
        }
    }
    let first = *points.first()?;
    let (mut lo, mut hi) = (first, first);
    for p in &points {
        lo = (lo.0.min(p.0), lo.1.min(p.1));
        hi = (hi.0.max(p.0), hi.1.max(p.1));
    }
    let (width, box_height) = (hi.0.checked_sub(lo.0)?, hi.1.checked_sub(lo.1)?);
    if width <= 0 || box_height <= 0 {
        return None;
    }
    // Each point from the corner of the box.
    let mut next = points.iter().map(|p| Some((p.0.checked_sub(lo.0)?, p.1.checked_sub(lo.1)?)));
    let mut data = String::new();
    if path {
        for seg in route {
            let mut take = || next.next().flatten();
            match seg {
                Seg::Move(_) => {
                    let p = take()?;
                    data.push_str(&format!("M {} {}", p.0, p.1));
                }
                Seg::Line(_) => {
                    let p = take()?;
                    data.push_str(&format!(" L {} {}", p.0, p.1));
                }
                Seg::Curve(..) => {
                    let (a, b, p) = (take()?, take()?, take()?);
                    data.push_str(&format!(" C {} {} {} {} {} {}", a.0, a.1, b.0, b.1, p.0, p.1));
                }
            }
        }
    } else {
        for item in next {
            let p = item?;
            if !data.is_empty() {
                data.push(' ');
            }
            data.push_str(&format!("{},{}", p.0, p.1));
        }
    }
    Some(Placed { left: lo.0, top: lo.1, width, height: box_height, data, view_box: format!("0 0 {width} {box_height}"), path })
}

/// The box of every point of `route`, control points of a curve among them, in the route's own units: `[left, bottom, right, top]` (the least and the greatest of each coordinate), or
/// `None` for a route with no point or one that holds a point that is no number.
pub(crate) fn extent(route: &[Seg]) -> Option<[f64; 4]> {
    let mut found: Option<[f64; 4]> = None;
    for seg in route {
        let (points, count) = match seg {
            Seg::Move(p) | Seg::Line(p) => ([*p; 3], 1),
            Seg::Curve(a, b, p) => ([*a, *b, *p], 3),
        };
        for &(x, y) in points.iter().take(count) {
            if !(x.is_finite() && y.is_finite()) {
                return None;
            }
            found = Some(match found {
                None => [x, y, x, y],
                Some([x0, y0, x1, y1]) => [x0.min(x), y0.min(y), x1.max(x), y1.max(y)],
            });
        }
    }
    found
}

/// The end of each segment of `route`: the point a move or a line goes to, and the one a curve ends at.
pub(crate) fn ends_of(route: &[Seg]) -> Vec<(f64, f64)> {
    route
        .iter()
        .map(|seg| match seg {
            Seg::Move(p) | Seg::Line(p) | Seg::Curve(_, _, p) => *p,
        })
        .collect()
}

/// A length of `hundredths` hundredths of a millimetre, in inches.
pub(crate) fn inches(hundredths: i64) -> f64 {
    hundredths as f64 / super::route::HUNDREDTHS_PER_INCH
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use diacraft_doc::{Cell, CellName, Row, Section, Sheet, Value};

    use super::*;

    fn number(sheet: &mut Sheet, name: &str, v: f64) {
        sheet.cells.insert(CellName::new(name), Cell::constant(Value::from_number(v).unwrap()));
    }

    fn row(kind: &str, ix: u32, x: f64, y: f64) -> Row {
        let mut r = Row { row_type: Some(Arc::from(kind)), ix: Some(ix), ..Row::default() };
        r.cells.insert(CellName::new("X"), Cell::constant(Value::from_number(x).unwrap()));
        r.cells.insert(CellName::new("Y"), Cell::constant(Value::from_number(y).unwrap()));
        r
    }

    /// A segment from (3.25, 3) to (4.75, 3) whose pin is the middle of its ends, with `rows` (in its own frame) as its one `Geometry` section.
    fn segment(rows: Vec<Row>, cells: &[(&str, f64)]) -> Sheet {
        let mut sheet = Sheet::default();
        for (name, v) in
            [("BeginX", 3.25), ("BeginY", 3.0), ("EndX", 4.75), ("EndY", 3.0), ("PinX", 4.0), ("PinY", 3.0), ("Width", 1.5), ("Height", 0.0)]
        {
            number(&mut sheet, name, v);
        }
        number(&mut sheet, "LocPinX", 0.75);
        number(&mut sheet, "LocPinY", 0.0);
        let mut section = Section::new("Geometry", Some(0));
        for (name, v) in cells {
            section.cells.insert(CellName::new(name), Cell::constant(Value::from_number(*v).unwrap()));
        }
        section.rows = rows.into_iter().map(Arc::new).collect();
        sheet.sections.push(Arc::new(section));
        sheet
    }

    const OPEN: [(&str, f64); 1] = [("NoFill", 1.0)];

    fn zigzag() -> Vec<Row> {
        vec![
            row("MoveTo", 1, 0.0, 0.0),
            row("LineTo", 2, 0.3, 0.1),
            row("LineTo", 3, 0.6, -0.1),
            row("LineTo", 4, 0.9, 0.1),
            row("LineTo", 5, 1.2, -0.1),
            row("LineTo", 6, 1.5, 0.0),
        ]
    }

    /// The zigzag of a Comm-link is a polyline of six points from the corner of its box, on a page 8.5 inches high.
    #[test]
    fn the_zigzag_is_a_polyline_in_hundredths_of_a_millimetre_from_the_corner_of_its_box() {
        let sheet = segment(zigzag(), &OPEN);
        let mut steps = 0;
        let segs = segs_of(&sheet, &Affine::IDENTITY, &mut steps).unwrap();
        assert_eq!(steps, 6, "a step for each row");
        let placed = placed(&segs, 8.5).unwrap();
        assert_eq!(placed.data, "0,254 762,0 1524,508 2286,0 3048,508 3810,254");
        assert_eq!(placed.view_box, "0 0 3810 508");
        assert_eq!((placed.left, placed.top, placed.width, placed.height), (8255, 13716, 3810, 508));
        assert!(!placed.path);
        assert!((inches(placed.left) - 3.25).abs() < 1e-9);
    }

    /// A pin that is not the middle, and a turn, are the shape's own: the points go through the map of its frame cells.
    #[test]
    fn the_points_follow_the_pin_the_turn_and_the_groups_around() {
        // Turned a quarter turn about its pin: the line runs up the page from (4, 2.25) to (4, 3.75).
        let sheet = segment(zigzag(), &[("NoFill", 1.0)]);
        let mut turned = sheet.clone();
        number(&mut turned, "Angle", std::f64::consts::FRAC_PI_2);
        let segs = segs_of(&turned, &Affine::IDENTITY, &mut 0).unwrap();
        let first = match segs.first() {
            Some(Seg::Move(p)) => *p,
            other => panic!("{other:?}"),
        };
        assert!((first.0 - 4.0).abs() < 1e-9 && (first.1 - 2.25).abs() < 1e-9, "{first:?}");
        // A group around it that shifts the page by (1, 2) moves every point by that.
        let outer = Affine { e: 1.0, f: 2.0, ..Affine::IDENTITY };
        let shifted = segs_of(&sheet, &outer, &mut 0).unwrap();
        let plain = segs_of(&sheet, &Affine::IDENTITY, &mut 0).unwrap();
        for (a, b) in shifted.iter().zip(plain.iter()) {
            match (a, b) {
                (Seg::Line(p) | Seg::Move(p), Seg::Line(q) | Seg::Move(q)) => {
                    assert!((p.0 - q.0 - 1.0).abs() < 1e-9 && (p.1 - q.1 - 2.0).abs() < 1e-9)
                }
                other => panic!("{other:?}"),
            }
        }
    }

    /// What is not a stroked outline of lines and cubics is not written as one: more than one section, a row that is no line, a section that fills, or has no line, is not shown or has a
    /// cell of its own, a deleted row, a row that holds no number, one row, and a section that is not the first.
    #[test]
    fn a_geometry_the_writer_does_not_say_is_none() {
        let ok = |sheet: &Sheet| segs_of(sheet, &Affine::IDENTITY, &mut 0).is_some();
        assert!(ok(&segment(zigzag(), &OPEN)));
        assert!(!ok(&segment(zigzag(), &[])), "no NoFill: the section fills");
        assert!(!ok(&segment(zigzag(), &[("NoFill", 0.0)])), "NoFill 0");
        assert!(!ok(&segment(zigzag(), &[("NoFill", 1.0), ("NoLine", 1.0)])), "no line");
        assert!(!ok(&segment(zigzag(), &[("NoFill", 1.0), ("NoShow", 1.0)])), "not shown");
        assert!(!ok(&segment(zigzag(), &[("NoFill", 1.0), ("Extra", 1.0)])), "a cell of its own");
        assert!(ok(&segment(zigzag(), &[("NoFill", 1.0), ("NoSnap", 1.0), ("NoQuickDrag", 0.0)])), "the flags that draw nothing differently");
        let mut gone = zigzag();
        gone[2].del = true;
        assert!(!ok(&segment(gone, &OPEN)), "a deleted row");
        let mut arc = zigzag();
        arc[3].row_type = Some(Arc::from("ArcTo"));
        assert!(!ok(&segment(arc, &OPEN)), "an arc");
        let mut line_first = zigzag();
        line_first[0].row_type = Some(Arc::from("LineTo"));
        assert!(!ok(&segment(line_first, &OPEN)), "a line first");
        assert!(!ok(&segment(vec![row("MoveTo", 1, 0.0, 0.0)], &OPEN)), "one row");
        let mut empty = zigzag();
        empty[1].cells = Default::default();
        assert!(!ok(&segment(empty, &OPEN)), "a row that holds no number");
        let mut second = segment(zigzag(), &OPEN);
        let mut other = Section::new("Geometry", Some(1));
        other.cells.insert(CellName::new("NoFill"), Cell::constant(Value::from_number(1.0).unwrap()));
        second.sections.push(Arc::new(other));
        assert!(!ok(&second), "two sections");
        let mut ix = segment(zigzag(), &OPEN);
        let mut moved = (*ix.sections[0]).clone();
        moved.ix = Some(1);
        ix.sections = vec![Arc::new(moved)];
        assert!(!ok(&ix), "a section that is not the first");
    }

    /// An outline with no width or no height has no view box, and one point is no outline.
    #[test]
    fn a_box_of_no_size_is_none() {
        let level = [Seg::Move((1.0, 1.0)), Seg::Line((2.0, 1.0)), Seg::Line((3.0, 1.0))];
        assert_eq!(placed(&level, 8.0), None, "every point on one level");
        let plumb = [Seg::Move((1.0, 1.0)), Seg::Line((1.0, 2.0))];
        assert_eq!(placed(&plumb, 8.0), None, "every point in one column");
        assert_eq!(placed(&[Seg::Move((1.0, 1.0))], 8.0), None, "one point");
        assert_eq!(placed(&[], 8.0), None);
        assert_eq!(placed(&[Seg::Line((1.0, 1.0)), Seg::Line((2.0, 2.0))], 8.0), None, "not begun with a move");
        assert_eq!(placed(&[Seg::Move((1.0, 1.0)), Seg::Move((2.0, 2.0))], 8.0), None, "a second move");
        assert_eq!(placed(&[Seg::Move((f64::NAN, 1.0)), Seg::Line((2.0, 2.0))], 8.0), None, "not a number");
        assert_eq!(placed(&[Seg::Move((1.0, 1.0)), Seg::Line((1e300, 2.0))], 8.0), None, "past what a path can hold");
    }

    /// A route with a curve is a path of absolute commands from the corner of the box of its control points.
    #[test]
    fn a_curve_makes_a_path() {
        let route = [Seg::Move((1.0, 1.0)), Seg::Curve((1.5, 2.0), (2.5, 0.0), (3.0, 1.0)), Seg::Line((4.0, 1.0))];
        let placed = placed(&route, 4.0).unwrap();
        assert!(placed.path);
        // The page is 4 inches high: the control points reach from y 0 to y 2 up the page, which is 4 to 2 inches down from its top.
        assert_eq!(placed.view_box, "0 0 7620 5080", "x from 1 to 4 inches, y from 0 to 2 inches");
        assert_eq!(placed.data, "M 0 2540 C 1270 0 3810 5080 5080 2540 L 7620 2540", "{placed:?}");
    }
}
