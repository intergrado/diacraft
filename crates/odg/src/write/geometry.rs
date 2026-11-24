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
//! * **`draw:modifiers`**: the values a handle drag changed, in their places.
//!
//! Every function gives the text or the words of why not; none guesses.

use diacraft_doc::{Cell, Row, Section, Sheet};

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

/// **Whether the rows [`enhanced_path`] writes a path from are the rows the reader gives back for it, formulas and all** (a formula that does not come back is a loss, even when its
/// value does): each `X` and `Y` of a `MoveTo` or `LineTo` row is the constant 0 or the share the reader writes (`read::cells::share_text`: `Width`, `Width*0.5`, never `Width*1` or `Width*0`, which
/// read back as `Width` and the constant 0), and no cell of a section holds a formula of its own. A drawing of a VSDX file writes its rectangles' rows as `Width*0` and `Width*1`: counted
/// 783 of 10,041 expressions over the shape's own cells lost on a trip through an `.odg`. Rows that are not are carried whole beside the path, which the file draws.
pub(crate) fn rows_in_reader_form(sheet: &Sheet) -> bool {
    for section in geometry_sections(sheet) {
        if section.cells.values().any(|c| c.formula.text().is_some()) {
            return false;
        }
        for row in section.rows.iter() {
            if row.row_type.as_deref() == Some("Ellipse") {
                continue;
            }
            for (name, axis) in [("X", "Width"), ("Y", "Height")] {
                let cell = row.cells.get(name);
                let Some(k) = share_of(cell, axis) else { return false };
                if cell.and_then(|c| c.formula.text()) != crate::read::cells::share_text(axis, k).as_deref() {
                    return false;
                }
            }
        }
    }
    true
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
}
