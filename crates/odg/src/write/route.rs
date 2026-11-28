//! A connector's route as `svg:d` and `svg:viewBox` (the connector rows):
//! the writer's half of what `read::connectors::route` reads.
//!
//! **The writer does not route.** It writes the route the model has, as a path, when the model's route differs from the file's (a command of the
//! engine rerouted the connector, because the connector or a shape it is glued to moved). A save that no such command preceded keeps `svg:d`
//! as the file has it, whatever the route's marker says: the marker (`User.DiaCraftRoute`) says the engine may route
//! the connector again, and this crate never does.
//!
//! **The form** is LibreOffice's: an absolute `M` at the start in hundredths of a millimetre from the page's top left, then relative
//! commands to the end (`h` and `v` for a segment along an axis, `l` for any other, `c` for a cubic); and `svg:viewBox="0 0 <w+1> <h+1>"`,
//! the extents of the route's points in the same units, each one more, which the 1.3 schema requires on every connector. Every point is
//! rounded to a whole hundredth of a millimetre once and the commands are the differences of the rounded points, so the path ends where the
//! route does, to the unit.
//!
//! **What it reads**: the rows of the connector's `Geometry` section 0, `MoveTo` first, then `LineTo` and `NURBSTo` rows (one cubic with its
//! two control points, in the form the reader writes). The rows are in the connector's own frame (its begin the origin, its x axis toward
//! its end); the points are taken to the page through the map of the group around it. A route of any other row, or of a second `MoveTo`, is
//! [`Route::Unreadable`], and the writer keeps the file's path and says so.

use diacraft_doc::Sheet;

use crate::read::Affine;
use crate::read::geometry::Tokens;

/// One segment of a route: points on the page, in inches, y up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Seg {
    Move((f64, f64)),
    Line((f64, f64)),
    Curve((f64, f64), (f64, f64), (f64, f64)),
}

/// A connector's route in the model.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Route {
    Path(Vec<Seg>),
    /// The model's route has rows the writer cannot write as a path.
    Unreadable,
}

fn number(sheet: &Sheet, name: &str) -> Option<f64> {
    sheet.cell(name)?.value.as_ref()?.number()
}

/// The two control points of a cubic `NURBSTo` row's `E` cell, in the form the reader writes (`NURBS(1,3,1,1,x1,y1,0,1,x2,y2,0,1)`).
pub(crate) fn control_points(row: &diacraft_doc::Row) -> Option<[f64; 4]> {
    let cell = row.cells.get("E")?;
    let text = cell.formula.text().or_else(|| cell.value.as_ref().map(diacraft_doc::Value::text))?;
    let inside = text.trim().strip_prefix("NURBS(")?.strip_suffix(')')?;
    let numbers: Vec<f64> = inside.split(',').map(|n| n.trim().parse::<f64>().ok().filter(|v| v.is_finite())).collect::<Option<_>>()?;
    match numbers.as_slice() {
        [knot, degree, x_type, y_type, x1, y1, _, _, x2, y2, _, _] if (*knot, *degree, *x_type, *y_type) == (1.0, 3.0, 1.0, 1.0) => {
            Some([*x1, *y1, *x2, *y2])
        }
        _ => None,
    }
}

/// The route of the connector whose own sheet is `sheet`, in a group whose map to the page is `outer`: `None` when it is not a route this writes.
pub(crate) fn of(sheet: &Sheet, outer: &Affine) -> Option<Vec<Seg>> {
    let (bx, by, ex, ey) = (number(sheet, "BeginX")?, number(sheet, "BeginY")?, number(sheet, "EndX")?, number(sheet, "EndY")?);
    let angle = (ey - by).atan2(ex - bx);
    let (s, c) = if angle.is_finite() { angle.sin_cos() } else { (0.0, 1.0) };
    let to_page = |u: f64, v: f64| outer.apply(bx + u * c - v * s, by + u * s + v * c);
    let section = sheet.sections.iter().find(|s| s.name.as_str() == "Geometry" && s.ix == Some(0))?;
    let mut out = Vec::with_capacity(section.rows.len());
    for (i, row) in section.rows.iter().enumerate() {
        if row.del {
            return None;
        }
        let at = |name: &str| row.cells.get(name).and_then(|cell| cell.value.as_ref()?.number());
        let end = to_page(at("X")?, at("Y")?);
        out.push(match (row.row_type.as_deref(), i) {
            (Some("MoveTo"), 0) => Seg::Move(end),
            (Some("LineTo"), 1..) => Seg::Line(end),
            (Some("NURBSTo"), 1..) => {
                let [x1, y1, x2, y2] = control_points(row)?;
                Seg::Curve(to_page(x1, y1), to_page(x2, y2), end)
            }
            _ => return None,
        });
    }
    (out.len() >= 2).then_some(out)
}

/// How far two corners of a route may lie apart, inches, and still be one corner (the writer's own unit is 0.01 mm, 3.9E-4 of an inch).
const SAME_CORNER: f64 = 1.0e-6;

/// The corners of the route of the connector whose own sheet is `sheet`, on the page (inches, y up), **with its hops taken out**: a route is the same route
/// whatever way its rows spell it (the rebuild of line jumps writes a connector's rows in the engine's form, and one it takes a hop away from has no
/// hop left, but not the rows the file made). Every row's end is a point (the chord of a hop, an `ArcTo`, a `PolylineTo` or the `MoveTo` of a gap, ends on the
/// leg it hops), and a point that lies on the leg between its two neighbours is dropped, which takes a hop's two chord ends out, and the points a file may hold
/// on a leg. `None` for a route of any other row (a curve), of a deleted row, or of fewer than two rows; **a route whose rows are all one point has that one corner** (a connector whose ends are one point: the fuzz target's first find on the fix), and equals another such. Each row is one step of `steps`.
pub(crate) fn corners(sheet: &Sheet, outer: &Affine, steps: &mut u64) -> Option<Vec<(f64, f64)>> {
    corners_kept(sheet, outer, steps).map(|(corners, _)| corners)
}

/// The route of a new connector whose rows hold line jumps, **without them**: the [`corners`] of the route as a path of lines, when every row that is
/// not a line (an `ArcTo`, a `PolylineTo`, a `MoveTo` past the first) is a hop: its end lies on the leg it hops, so it is no corner of the route. `None` when a row of
/// those kinds is a corner (a rounded corner, a route of two pieces), or the route has no corners. The rows looked at are counted in `steps`.
pub(crate) fn without_hops(sheet: &Sheet, outer: &Affine, steps: &mut u64) -> Option<Vec<Seg>> {
    let (corners, kept) = corners_kept(sheet, outer, steps)?;
    let section = sheet.sections.iter().find(|s| s.name.as_str() == "Geometry" && s.ix == Some(0))?;
    let hop_kept = section
        .rows
        .iter()
        .enumerate()
        .any(|(i, row)| i > 0 && matches!(row.row_type.as_deref(), Some("ArcTo" | "PolylineTo" | "MoveTo")) && kept.binary_search(&i).is_ok());
    if hop_kept || corners.len() < 2 {
        return None;
    }
    let mut out = Vec::new();
    out.try_reserve(corners.len()).ok()?;
    for (i, p) in corners.into_iter().enumerate() {
        out.push(if i == 0 { Seg::Move(p) } else { Seg::Line(p) });
    }
    Some(out)
}

/// The most control points one quadratic spline of a route is cut into cubics for (a route of a connector has a handful): a row with more is not a route this writes.
const MOST_CONTROLS: usize = 4096;

/// The control points a quadratic `NURBSTo` row lists in its `E` cell as the engine writes a curved connector's route (`NURBS(k,2,1,1,x,y,k,1,...)`: the control points
/// between the pen and the row's end, each with its knot and a weight of 1, and the last knot first), or `None` for a row that is not that form: a knot out of the clamped
/// uniform list (`0, 0, 0, 1, 2, ..., n-2, n-2, n-2` for `n` control points counting the pen and the end), a weight other than 1, or another degree.
fn quadratic_controls(row: &diacraft_doc::Row) -> Option<Vec<(f64, f64)>> {
    let cell = row.cells.get("E")?;
    let text = cell.formula.text().or_else(|| cell.value.as_ref().map(diacraft_doc::Value::text))?;
    let inside = text.trim().strip_prefix("NURBS(")?.strip_suffix(')')?;
    let numbers: Vec<f64> = inside.split(',').map(|n| n.trim().parse::<f64>().ok().filter(|v| v.is_finite())).collect::<Option<_>>()?;
    let [last, degree, x_type, y_type, rest @ ..] = numbers.as_slice() else { return None };
    if (*degree, *x_type, *y_type) != (2.0, 1.0, 1.0) || rest.len() % 4 != 0 || rest.len() / 4 + 2 > MOST_CONTROLS {
        return None;
    }
    let n = rest.len() / 4 + 2;
    let knot = |place: usize| if place < 3 { 0.0 } else { place.saturating_sub(2).min(n.saturating_sub(2)) as f64 };
    if *last != knot(n) {
        return None;
    }
    let mut points = Vec::with_capacity(rest.len() / 4);
    let (groups, _) = rest.as_chunks::<4>();
    for (i, [x, y, k, w]) in groups.iter().enumerate() {
        if *w != 1.0 || *k != knot(i + 1) {
            return None;
        }
        points.push((*x, *y));
    }
    Some(points)
}

/// The route of a new connector whose rows are a `MoveTo` and `LineTo` rows and **quadratic `NURBSTo` rows** (the engine's curved connector), as cubics: a clamped quadratic spline of
/// `n` control points is `n - 2` quadratic pieces that join at the middles of the legs of its control polygon, and each piece raised to a cubic is the same curve. `None` for a route of any
/// other row. The result is not the rows the reader makes of a path (it reads a cubic as degree 3), so the caller carries the model's own rows.
pub(crate) fn quadratic(sheet: &Sheet, outer: &Affine) -> Option<Vec<Seg>> {
    let (bx, by, ex, ey) = (number(sheet, "BeginX")?, number(sheet, "BeginY")?, number(sheet, "EndX")?, number(sheet, "EndY")?);
    let angle = (ey - by).atan2(ex - bx);
    let (s, c) = if angle.is_finite() { angle.sin_cos() } else { (0.0, 1.0) };
    let to_page = |u: f64, v: f64| outer.apply(bx + u * c - v * s, by + u * s + v * c);
    let section = sheet.sections.iter().find(|s| s.name.as_str() == "Geometry" && s.ix == Some(0))?;
    let mut out: Vec<Seg> = Vec::new();
    out.try_reserve(section.rows.len()).ok()?;
    let mut pen: Option<(f64, f64)> = None;
    for (i, row) in section.rows.iter().enumerate() {
        if row.del {
            return None;
        }
        let at = |name: &str| row.cells.get(name).and_then(|cell| cell.value.as_ref()?.number());
        let end = to_page(at("X")?, at("Y")?);
        if !(end.0.is_finite() && end.1.is_finite()) {
            return None;
        }
        match (row.row_type.as_deref(), i) {
            (Some("MoveTo"), 0) => out.push(Seg::Move(end)),
            (Some("LineTo"), 1..) => out.push(Seg::Line(end)),
            (Some("NURBSTo"), 1..) => {
                let mut polygon: Vec<(f64, f64)> = vec![pen?];
                for (x, y) in quadratic_controls(row)? {
                    polygon.push(to_page(x, y));
                }
                polygon.push(end);
                let pieces = polygon.len().checked_sub(2)?;
                let mid = |a: (f64, f64), b: (f64, f64)| ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
                for k in 0..pieces {
                    let start = if k == 0 { *polygon.first()? } else { mid(*polygon.get(k)?, *polygon.get(k + 1)?) };
                    let control = *polygon.get(k + 1)?;
                    let stop = if k + 1 == pieces { *polygon.last()? } else { mid(*polygon.get(k + 1)?, *polygon.get(k + 2)?) };
                    let lift = |from: (f64, f64)| (from.0 + (control.0 - from.0) * 2.0 / 3.0, from.1 + (control.1 - from.1) * 2.0 / 3.0);
                    out.try_reserve(1).ok()?;
                    out.push(Seg::Curve(lift(start), lift(stop), stop));
                }
            }
            _ => return None,
        }
        pen = Some(end);
    }
    (out.len() >= 2 && out.iter().any(|s| matches!(s, Seg::Curve(..)))).then_some(out)
}

/// The corners of a route, and for each the row (by its place in the section) whose end it is, rising.
type Kept = (Vec<(f64, f64)>, Vec<usize>);

/// [`corners`], and for each corner the row (by its place in the section) whose end it is, rising.
fn corners_kept(sheet: &Sheet, outer: &Affine, steps: &mut u64) -> Option<Kept> {
    let (bx, by, ex, ey) = (number(sheet, "BeginX")?, number(sheet, "BeginY")?, number(sheet, "EndX")?, number(sheet, "EndY")?);
    let angle = (ey - by).atan2(ex - bx);
    let (s, c) = if angle.is_finite() { angle.sin_cos() } else { (0.0, 1.0) };
    let section = sheet.sections.iter().find(|s| s.name.as_str() == "Geometry" && s.ix == Some(0))?;
    // The rows are counted before they are looked at: the work is theirs.
    *steps = steps.saturating_add(section.rows.len() as u64);
    let mut out: Vec<(f64, f64)> = Vec::new();
    let mut rows: Vec<usize> = Vec::new();
    out.try_reserve(section.rows.len()).ok()?;
    rows.try_reserve(section.rows.len()).ok()?;
    for (i, row) in section.rows.iter().enumerate() {
        if row.del {
            return None;
        }
        match (row.row_type.as_deref(), i) {
            (Some("MoveTo"), 0) | (Some("LineTo" | "ArcTo" | "PolylineTo" | "MoveTo"), 1..) => {}
            _ => return None,
        }
        let at = |name: &str| row.cells.get(name).and_then(|cell| cell.value.as_ref()?.number());
        let (u, v) = (at("X")?, at("Y")?);
        let end = outer.apply(bx + u * c - v * s, by + u * s + v * c);
        if !(end.0.is_finite() && end.1.is_finite()) {
            return None;
        }
        // A point where the pen already is is no corner; and the one before it, when it lies on the leg between its own neighbours, is none either.
        if out.last().is_some_and(|q| near(*q, end)) {
            continue;
        }
        while let [.., a, b] = out[..] {
            if !on_leg(a, b, end) {
                break;
            }
            out.pop();
            rows.pop();
        }
        out.push(end);
        rows.push(i);
    }
    (section.rows.len() >= 2).then_some((out, rows))
}

fn near(a: (f64, f64), b: (f64, f64)) -> bool {
    (a.0 - b.0).abs() <= SAME_CORNER && (a.1 - b.1).abs() <= SAME_CORNER
}

/// Whether `b` lies on the straight leg from `a` to `c`, strictly between them: within [`SAME_CORNER`] of the line, and neither at an end nor past one.
fn on_leg(a: (f64, f64), b: (f64, f64), c: (f64, f64)) -> bool {
    let (ac, ab) = ((c.0 - a.0, c.1 - a.1), (b.0 - a.0, b.1 - a.1));
    // A leg of no length (or a NaN one) has nothing strictly between its ends: the comparisons below are false for it.
    let length = ac.0.hypot(ac.1);
    let (along, off) = ((ab.0 * ac.0 + ab.1 * ac.1) / length, (ab.0 * ac.1 - ab.1 * ac.0).abs() / length);
    off <= SAME_CORNER && along > SAME_CORNER && along < length - SAME_CORNER
}

/// Whether two routes' corners are the same corners, each within [`SAME_CORNER`].
pub(crate) fn same_corners(a: &[(f64, f64)], b: &[(f64, f64)]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(p, q)| near(*p, *q))
}

/// Hundredths of a millimetre in an inch, the unit of a path's numbers.
pub(crate) const HUNDREDTHS_PER_INCH: f64 = 2540.0;

/// The most bytes of `svg:d` that [`shifted`] reads: a connector's path is a few hundred bytes, and the text made is at most twenty times the text read (see
/// [`most_shifted_bytes`]).
const MOST_PATH_BYTES: usize = 1 << 20;

/// The most a number of a path may come to, a trillion hundredths of a millimetre: [`push_number`] writes none past it and [`shifted`] makes none.
const MOST_PATH_NUMBER: f64 = 1.0e12;

/// The fewest places after the point a number that moves is written to (hundredths of a millimetre, so a millionth of a millimetre).
const LEAST_PLACES: usize = 4;

/// The most places after the point a number that moves is written to: a number the file holds to more keeps them up to here, which is every digit a double has at the
/// size of a page (a hundred thousand hundredths of a millimetre is a metre).
const MOST_PLACES: usize = 10;

/// How many times as many bytes as the path has [`shifted`] makes at most: a number of one byte that moves is written as one of up to eighteen characters (a sign,
/// twelve digits, a point and four places; the one value of thirteen digits has no places), and a space may be put on each side of it.
const SHIFT_GROWTH: usize = 20;

/// The most bytes [`shifted`] makes of a path of `len` bytes: twenty times as many, and what a move put in front of it takes (`M 0 `, a number, a space). The caller charges
/// it to the memory a save may hold **before** the text is made, and the pass reserves it, so the text made never grows the memory past what was charged.
pub(crate) fn most_shifted_bytes(len: usize) -> u64 {
    (len as u64).saturating_mul(SHIFT_GROWTH as u64).saturating_add(64)
}

/// The most bytes a shift of a path of `len` bytes holds at once: the path as it was read and the text made of it.
pub(crate) fn shift_peak(len: usize) -> u64 {
    most_shifted_bytes(len).saturating_add(len as u64)
}

/// What one argument of a path command is.
#[derive(Clone, Copy, PartialEq)]
enum Arg {
    /// A distance along the page, or an arc's radius or turn: copied as it is written.
    Other,
    /// A distance down the page, `y`: the one an absolute command shifts.
    Down,
    /// An arc's flag.
    Flag,
}

/// The arguments of one path command, by its upper-case letter.
fn arguments(command: u8) -> Option<&'static [Arg]> {
    use Arg::{Down, Flag, Other};
    Some(match command {
        b'M' | b'L' | b'T' => &[Other, Down],
        b'H' => &[Other],
        b'V' => &[Down],
        b'C' => &[Other, Down, Other, Down, Other, Down],
        b'S' | b'Q' => &[Other, Down, Other, Down],
        b'A' => &[Other, Other, Other, Flag, Flag, Other, Down],
        b'Z' => &[],
        _ => return None,
    })
}

/// The places after the point a number's text has, counting its exponent (`0.00003` has five, `5e-6` six, `2.5e1` none: it is 25), up to [`MOST_PLACES`].
fn places_of(text: &str) -> usize {
    let (mantissa, exponent) = match text.split_once(['e', 'E']) {
        Some((mantissa, exponent)) => (mantissa, exponent.parse::<i64>().unwrap_or(0)),
        None => (text, 0),
    };
    let fraction = mantissa.split_once('.').map_or(0, |(_, places)| places.len()) as i64;
    usize::try_from(fraction.saturating_sub(exponent).clamp(0, MOST_PLACES as i64)).unwrap_or(0)
}

/// Writes a number as a path does, to `places` places after the point, with no trailing zeros and no negative zero (`12000`, `10012.3456`, `-2000`), after what `out` holds,
/// set apart from it by a space where the number would run on from a digit or a point (a number that begins with its sign needs none). `None` for one past
/// [`MOST_PATH_NUMBER`] or not a number, and `out` as it was.
fn push_number(out: &mut String, value: f64, places: usize) -> Option<()> {
    use std::fmt::Write;
    if !value.is_finite() || value.abs() > MOST_PATH_NUMBER {
        return None;
    }
    let start = out.len();
    write!(out, "{value:.places$}").ok()?;
    if out.get(start..)?.contains('.') {
        let kept = out.trim_end_matches('0').trim_end_matches('.').len();
        out.truncate(kept);
    }
    if out.get(start..)? == "-0" {
        out.truncate(start);
        out.push('0');
    }
    let runs_on = start > 0 && out.as_bytes().get(start - 1).is_some_and(|b| b.is_ascii_digit() || *b == b'.');
    if runs_on && out.as_bytes().get(start) != Some(&b'-') {
        out.insert(start, ' ');
    }
    Some(())
}

/// The connector path `path` (`svg:d`, in hundredths of a millimetre from the page's top left) moved **down the page** by `by` of them (up for a negative `by`): the whole
/// path translated, as a height edit that keeps `PinY` translates every shape and every connector end.
///
/// * **What moves** is each `y` an absolute command gives (`M`, `L`, `C`, `S`, `Q`, `T`, `A` and `V`), and the `y` of the path's first move whatever its case (the first `m`
///   is absolute). A relative command moves with the pen and an `H` has no `y`, so LibreOffice's own form, an absolute `M` and relative steps, has its one start moved.
/// * **A path that begins with no move** (the reader reads one: it starts at the origin) has `M 0 <by>` put in front of it, and its absolute `y` move with it, so it moves
///   exactly (the owner's ruling (a) on the review, 2026-11-20). None is put in where nothing moves.
/// * **Every other byte is copied as it is written**: the commands, the separators, the numbers that do not move, the other coordinates. A number that moves is written to
///   [`LEAST_PLACES`] places after the point, or to the places it had when it had more, up to [`MOST_PLACES`] (ruling (b): a shift that cut `100.00003` to `100` would turn an
///   arc from a point to itself into one between two points a hair apart), and set apart from the numbers beside it when they ran together (`1-2`). Nothing is rewritten for a
///   shift of nothing.
/// * **`None`** when the writer cannot place the path, and the caller keeps it as it is: data that breaks SVG's syntax (by the reader's own tokenizer and grammar, so exactly
///   the paths the reader refuses), a result past [`MOST_PATH_NUMBER`] or not a number (a `by` that is not a number makes every result one), or more than
///   [`MOST_PATH_BYTES`] bytes. An empty path has nothing to move and is given back.
///
/// The work is one pass over the path, which takes at least a byte at each step and is held to as many steps as the path has bytes (a step that took none would be a fault of
/// this and ends as `None`, not as a loop), and the text made is at most [`most_shifted_bytes`].
pub(crate) fn shifted(path: &str, by: f64) -> Option<String> {
    shifted_within(path, by, path.len())
}

/// [`shifted`], with the most steps (commands, and the repeats of one) it may take over the path.
fn shifted_within(path: &str, by: f64, most_steps: usize) -> Option<String> {
    if path.len() > MOST_PATH_BYTES {
        return None;
    }
    let mut tokens = Tokens::new(path);
    let mut out = String::new();
    out.try_reserve(usize::try_from(most_shifted_bytes(path.len())).ok()?).ok()?;
    // Bytes of `path` already in `out`; the rest is copied when a number is replaced and at the end.
    let mut copied = 0usize;
    let mut command: Option<u8> = None;
    let mut first = true;
    let mut steps = 0usize;
    loop {
        tokens.skip();
        if tokens.at >= path.len() {
            break;
        }
        // Every step takes a byte at least (a command, a number, a flag), so a path of `n` bytes takes `n` steps at most: more is a fault, and ends here.
        steps += 1;
        if steps > most_steps {
            return None;
        }
        let c = match tokens.command() {
            Some(c) => c,
            // More numbers after a command repeat it. (After a move the pairs are lines, which take the same arguments and move the same way as a move does, so the move
            // is repeated: the reader's rule that they are `L` and `l` changes nothing here.)
            None => match command {
                Some(c) if !matches!(c, b'Z' | b'z') => c,
                _ => return None,
            },
        };
        // The first move is absolute whatever its case. A path that begins with another command starts at the origin: a move to where the shift puts that, in front.
        let absolute = c.is_ascii_uppercase() || (first && c == b'm');
        if first && !matches!(c, b'M' | b'm') && by != 0.0 {
            out.push_str("M 0 ");
            push_number(&mut out, by, LEAST_PLACES)?;
            if out == "M 0 0" {
                // The shift rounds to nothing: the origin is where it is.
                out.clear();
            } else if path.as_bytes().first().is_some_and(|b| !b.is_ascii_whitespace() && *b != b',') {
                out.push(' ');
            }
        }
        first = false;
        command = Some(c);
        for arg in arguments(c.to_ascii_uppercase())? {
            if *arg == Arg::Flag {
                tokens.flag().ok()?;
                continue;
            }
            let ((from, to), value) = tokens.number_at().ok()?;
            if *arg != Arg::Down || !absolute || by == 0.0 {
                continue;
            }
            out.push_str(path.get(copied..from)?);
            // The numbers beside this one may have been run together with it by their signs or points: they are set apart (`push_number` puts the space before it).
            let places = places_of(path.get(from..to)?).max(LEAST_PLACES);
            push_number(&mut out, value + by, places)?;
            if path.as_bytes().get(to).is_some_and(|b| b.is_ascii_digit() || *b == b'.') {
                out.push(' ');
            }
            copied = to;
        }
    }
    out.push_str(path.get(copied..)?);
    Some(out)
}

/// A point of the route in whole hundredths of a millimetre from the page's top left: `None` for one that is not a number or is past what a
/// path can hold.
pub(crate) fn hundredths(p: (f64, f64), height: f64) -> Option<(i64, i64)> {
    let (x, y) = (p.0 * HUNDREDTHS_PER_INCH, (height - p.1) * HUNDREDTHS_PER_INCH);
    if !(x.is_finite() && y.is_finite()) || x.abs() > MOST_PATH_NUMBER || y.abs() > MOST_PATH_NUMBER {
        return None;
    }
    Some((x.round() as i64, y.round() as i64))
}

/// `svg:d` and `svg:viewBox` for a route on a page `height` inches high, or `None` for a route with a point that cannot be written.
pub(crate) fn text(route: &[Seg], height: f64) -> Option<(String, String)> {
    if !matches!(route.first(), Some(Seg::Move(_))) {
        return None;
    }
    let mut path = String::new();
    let mut at = (0i64, 0i64);
    let (mut lo, mut hi) = ((i64::MAX, i64::MAX), (i64::MIN, i64::MIN));
    let mut see = |p: (i64, i64)| {
        lo = (lo.0.min(p.0), lo.1.min(p.1));
        hi = (hi.0.max(p.0), hi.1.max(p.1));
    };
    for seg in route {
        match *seg {
            Seg::Move(p) => {
                let p = hundredths(p, height)?;
                if !path.is_empty() {
                    return None;
                }
                path.push_str(&format!("M {} {}", p.0, p.1));
                see(p);
                at = p;
            }
            Seg::Line(p) => {
                let p = hundredths(p, height)?;
                let (dx, dy) = (p.0.checked_sub(at.0)?, p.1.checked_sub(at.1)?);
                path.push_str(&match (dx, dy) {
                    (_, 0) if dx != 0 => format!(" h {dx}"),
                    (0, _) => format!(" v {dy}"),
                    _ => format!(" l {dx} {dy}"),
                });
                see(p);
                at = p;
            }
            Seg::Curve(c1, c2, p) => {
                let (c1, c2, p) = (hundredths(c1, height)?, hundredths(c2, height)?, hundredths(p, height)?);
                let rel = |q: (i64, i64)| Some((q.0.checked_sub(at.0)?, q.1.checked_sub(at.1)?));
                let (a, b, e) = (rel(c1)?, rel(c2)?, rel(p)?);
                path.push_str(&format!(" c {} {} {} {} {} {}", a.0, a.1, b.0, b.1, e.0, e.1));
                for q in [c1, c2, p] {
                    see(q);
                }
                at = p;
            }
        }
    }
    let (w, h) = (hi.0.checked_sub(lo.0)?.checked_add(1)?, hi.1.checked_sub(lo.1)?.checked_add(1)?);
    Some((path, format!("0 0 {w} {h}")))
}

#[cfg(test)]
mod tests {
    use diacraft_doc::{Cell, CellName, Section, Value};

    use super::*;
    use crate::read::geometry;

    #[test]
    fn a_route_is_written_in_libreoffices_form() {
        // (5 cm, 3 cm), (8.5 cm, 3 cm), (8.5 cm, 9 cm), (12 cm, 9 cm) on the page (y down), a page 29.7 cm high.
        let height = 29.7 / 2.54;
        let up = |x: f64, y: f64| (x / 2.54, height - y / 2.54);
        let route = [Seg::Move(up(5.0, 3.0)), Seg::Line(up(8.5, 3.0)), Seg::Line(up(8.5, 9.0)), Seg::Line(up(12.0, 9.0))];
        let (path, view_box) = text(&route, height).unwrap();
        assert_eq!(path, "M 5000 3000 h 3500 v 6000 h 3500");
        assert_eq!(view_box, "0 0 7001 6001");
        // A slanted segment and a curve are relative too.
        let route = [Seg::Move(up(1.0, 1.0)), Seg::Line(up(2.0, 2.0)), Seg::Curve(up(3.0, 2.0), up(3.0, 3.0), up(4.0, 3.0))];
        assert_eq!(text(&route, height).unwrap().0, "M 1000 1000 l 1000 1000 c 1000 0 1000 1000 2000 1000");
    }

    /// A row of a route: its type and the numbers of its `X` and `Y` cells, and `extra` cells by name.
    fn row(kind: &str, x: f64, y: f64, extra: &[(&str, Cell)]) -> std::sync::Arc<diacraft_doc::Row> {
        let mut row = diacraft_doc::Row { row_type: Some(std::sync::Arc::from(kind)), ..diacraft_doc::Row::default() };
        for (name, v) in [("X", x), ("Y", y)] {
            row.cells.insert(CellName::new(name), Cell::constant(Value::from_number(v).unwrap_or_else(|| Value::from_text("0"))));
        }
        for (name, cell) in extra {
            row.cells.insert(CellName::new(name), cell.clone());
        }
        std::sync::Arc::new(row)
    }

    /// A connector's sheet from (1, 2) to (1, 4), straight up, holding these `Geometry` sections.
    fn connector(sections: Vec<(Option<u32>, Vec<std::sync::Arc<diacraft_doc::Row>>)>) -> Sheet {
        let mut sheet = Sheet::default();
        for (name, v) in [("BeginX", 1.0), ("BeginY", 2.0), ("EndX", 1.0), ("EndY", 4.0)] {
            sheet.cells.insert(CellName::new(name), Cell::constant(Value::from_number(v).unwrap_or_else(|| Value::from_text("0"))));
        }
        for (ix, rows) in sections {
            let mut section = Section::new("Geometry", ix);
            section.rows = rows.into();
            sheet.sections.push(std::sync::Arc::new(section));
        }
        sheet
    }

    fn nurbs(text: &str) -> Cell {
        Cell::constant(Value::from_text(text))
    }

    const CUBIC: &str = "NURBS(1,3,1,1,2.5,1,0,1,2.75,1,0,1)";

    fn close(a: (f64, f64), b: (f64, f64)) -> bool {
        (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9
    }

    #[test]
    fn the_rows_of_geometry_section_0_are_taken_to_the_page_through_the_connectors_own_frame() {
        // From (1, 2) to (1, 4) the connector's x axis runs up the page: its (u, v) is the page's (1 - v, 2 + u).
        let rows = vec![
            row("MoveTo", 0.0, 0.0, &[]),
            row("LineTo", 2.0, 0.0, &[]),
            row("LineTo", 2.0, 1.0, &[]),
            row("NURBSTo", 3.0, 1.0, &[("E", nurbs(CUBIC))]),
        ];
        // A section of another number and one of another name come first: the route is `Geometry` numbered 0.
        let mut sheet = connector(vec![(Some(1), vec![row("MoveTo", 9.0, 9.0, &[])]), (Some(0), rows)]);
        let mut other = Section::new("Control", Some(0));
        other.rows = vec![row("MoveTo", 8.0, 8.0, &[])].into();
        sheet.sections.insert(0, std::sync::Arc::new(other));
        let Some(route) = of(&sheet, &Affine::IDENTITY) else { panic!("the route was not read") };
        let expected = [Seg::Move((1.0, 2.0)), Seg::Line((1.0, 4.0)), Seg::Line((0.0, 4.0)), Seg::Curve((0.0, 4.5), (0.0, 4.75), (0.0, 5.0))];
        assert_eq!(route.len(), 4);
        for (got, want) in route.iter().zip(expected) {
            let same = match (got, want) {
                (Seg::Move(a), Seg::Move(b)) | (Seg::Line(a), Seg::Line(b)) => close(*a, b),
                (Seg::Curve(a1, a2, a3), Seg::Curve(b1, b2, b3)) => close(*a1, b1) && close(*a2, b2) && close(*a3, b3),
                _ => false,
            };
            assert!(same, "{got:?} against {want:?}");
        }
        // A group around the connector moves it on the page.
        let moved = of(&sheet, &Affine { e: 10.0, f: 20.0, ..Affine::IDENTITY });
        assert!(matches!(moved.as_deref(), Some([Seg::Move(p), ..]) if close(*p, (11.0, 22.0))), "{moved:?}");
        // The same connector pointing along the page's x axis: its frame is the page's.
        let mut flat = sheet.clone();
        for (name, v) in [("EndX", 5.0), ("EndY", 2.0)] {
            flat.cells.insert(CellName::new(name), Cell::constant(Value::from_number(v).unwrap_or_else(|| Value::from_text("0"))));
        }
        // A point `v` to the side of the line is `v` up the page: (u, v) is the page's (1 + u, 2 + v).
        let flat_route = of(&flat, &Affine::IDENTITY);
        assert!(
            matches!(
                flat_route.as_deref(),
                Some([Seg::Move(a), Seg::Line(b), Seg::Line(c), Seg::Curve(c1, c2, e)])
                    if close(*a, (1.0, 2.0)) && close(*b, (3.0, 2.0)) && close(*c, (3.0, 3.0))
                        && close(*c1, (3.5, 3.0)) && close(*c2, (3.75, 3.0)) && close(*e, (4.0, 3.0))
            ),
            "{flat_route:?}"
        );
    }

    #[test]
    fn what_is_not_a_route_the_writer_writes_is_none() {
        let two = || vec![row("MoveTo", 0.0, 0.0, &[]), row("LineTo", 2.0, 0.0, &[])];
        assert!(of(&connector(vec![(Some(0), two())]), &Affine::IDENTITY).is_some());
        let none = |rows: Vec<std::sync::Arc<diacraft_doc::Row>>| of(&connector(vec![(Some(0), rows)]), &Affine::IDENTITY);
        assert!(none(vec![row("MoveTo", 0.0, 0.0, &[])]).is_none(), "a route is two rows at least");
        assert!(none(vec![]).is_none());
        assert!(none(vec![row("LineTo", 0.0, 0.0, &[]), row("LineTo", 1.0, 0.0, &[])]).is_none(), "the first row is a move");
        assert!(none(vec![row("MoveTo", 0.0, 0.0, &[]), row("MoveTo", 1.0, 0.0, &[])]).is_none(), "a move only first");
        assert!(none(vec![row("MoveTo", 0.0, 0.0, &[]), row("ArcTo", 1.0, 0.0, &[])]).is_none(), "a row of another type");
        assert!(none(vec![row("MoveTo", 0.0, 0.0, &[]), row("NURBSTo", 1.0, 0.0, &[])]).is_none(), "a curve with no control points");
        let mut deleted = diacraft_doc::Row { del: true, ..diacraft_doc::Row::default() };
        deleted.row_type = Some(std::sync::Arc::from("LineTo"));
        assert!(none(vec![row("MoveTo", 0.0, 0.0, &[]), std::sync::Arc::new(deleted)]).is_none(), "a row deleted here");
        let mut no_x = (*row("LineTo", 1.0, 0.0, &[])).clone();
        no_x.cells.remove("X");
        assert!(none(vec![row("MoveTo", 0.0, 0.0, &[]), std::sync::Arc::new(no_x)]).is_none(), "a row with no X");
        // No section numbered 0, or an end that is not a number.
        assert!(of(&connector(vec![(Some(1), two())]), &Affine::IDENTITY).is_none());
        assert!(of(&connector(vec![(None, two())]), &Affine::IDENTITY).is_none());
        let mut sheet = connector(vec![(Some(0), two())]);
        sheet.cells.insert(CellName::new("EndY"), Cell::constant(Value::from_text("text")));
        assert!(of(&sheet, &Affine::IDENTITY).is_none());
    }

    #[test]
    fn the_control_points_of_a_cubic_are_read_from_the_formula_or_the_value_and_only_in_the_form_the_reader_writes() {
        let points = |cell: Cell| {
            let row = row("NURBSTo", 3.0, 1.0, &[("E", cell)]);
            control_points(&row)
        };
        assert_eq!(points(nurbs(CUBIC)), Some([2.5, 1.0, 2.75, 1.0]));
        assert_eq!(points(nurbs(&format!("  {CUBIC} "))), Some([2.5, 1.0, 2.75, 1.0]), "white space round it");
        assert_eq!(points(nurbs("NURBS( 1, 3, 1, 1, 2.5, 1, 0, 1, 2.75, 1, 0, 1 )")), Some([2.5, 1.0, 2.75, 1.0]), "and between the numbers");
        let formula = Cell::read(Some(CUBIC), Some("0"), None, None, &mut 1_000);
        assert_eq!(points(formula), Some([2.5, 1.0, 2.75, 1.0]), "the formula's text wins over the value's");
        // The knot, degree and the two types are each the numbers the reader writes.
        for wrong in [
            "NURBS(2,3,1,1,2.5,1,0,1,2.75,1,0,1)",
            "NURBS(1,2,1,1,2.5,1,0,1,2.75,1,0,1)",
            "NURBS(1,3,0,1,2.5,1,0,1,2.75,1,0,1)",
            "NURBS(1,3,1,0,2.5,1,0,1,2.75,1,0,1)",
            "NURBS(1,3,1,1,2.5,1,0,1,2.75,1,0)",
            "NURBS(1,3,1,1,2.5,1,0,1,2.75,1,0,1,5)",
            "NURBS(1,3,1,1,x,1,0,1,2.75,1,0,1)",
            "NURBS(1,3,1,1,NaN,1,0,1,2.75,1,0,1)",
            "NURBS(1,3,1,1,inf,1,0,1,2.75,1,0,1)",
            "SPLINE(1,3,1,1,2.5,1,0,1,2.75,1,0,1)",
            "NURBS(1,3,1,1,2.5,1,0,1,2.75,1,0,1",
            "",
        ] {
            assert_eq!(points(nurbs(wrong)), None, "{wrong}");
        }
        assert_eq!(control_points(&diacraft_doc::Row::default()), None, "no E cell");
    }

    #[test]
    fn a_point_is_written_up_to_a_trillion_hundredths_and_no_further() {
        // 1E12 hundredths of a millimetre exactly (this inch value times 2,540 is 1E12), and the next value up.
        let at_limit: f64 = 1.0e12 / 2540.0;
        let past = f64::from_bits(at_limit.to_bits() + 1);
        assert_eq!(at_limit * 2540.0, 1.0e12);
        assert!(past * 2540.0 > 1.0e12);
        let origin = (0.0, 0.0);
        // x: the limit passes, past it is none, in either direction; a huge y alone and a huge x alone are none too.
        assert_eq!(text(&[Seg::Move(origin), Seg::Line((at_limit, 0.0))], 0.0).map(|t| t.0), Some("M 0 0 h 1000000000000".to_owned()));
        assert!(text(&[Seg::Move(origin), Seg::Line((past, 0.0))], 0.0).is_none());
        assert!(text(&[Seg::Move(origin), Seg::Line((-past, 0.0))], 0.0).is_none());
        assert!(text(&[Seg::Move(origin), Seg::Line((1.0e10, 0.0))], 1.0).is_none());
        // y runs down the page from its top: a height of 0 puts a point at -p inches at p.
        assert_eq!(text(&[Seg::Move(origin), Seg::Line((0.0, -at_limit))], 0.0).map(|t| t.0), Some("M 0 0 v 1000000000000".to_owned()));
        assert!(text(&[Seg::Move(origin), Seg::Line((0.0, -past))], 0.0).is_none());
        assert!(text(&[Seg::Move(origin), Seg::Line((0.0, 1.0e10))], 1.0).is_none());
        // A first point of the route is held to it as well.
        assert!(text(&[Seg::Move((past, 0.0)), Seg::Line(origin)], 0.0).is_none());
    }

    #[test]
    fn a_segment_that_goes_nowhere_is_a_vertical_one_of_nothing() {
        let p = (1.0, 1.0);
        assert_eq!(text(&[Seg::Move(p), Seg::Line(p)], 2.0).map(|t| t.0), Some("M 2540 2540 v 0".to_owned()));
        assert_eq!(text(&[Seg::Move(p), Seg::Line((2.0, 1.0))], 2.0).map(|t| t.0), Some("M 2540 2540 h 2540".to_owned()));
        assert_eq!(text(&[Seg::Move(p), Seg::Line((1.0, 2.0))], 2.0).map(|t| t.0), Some("M 2540 2540 v -2540".to_owned()));
    }

    #[test]
    fn a_route_that_cannot_be_written_is_none() {
        let p = (0.0, 0.0);
        assert!(text(&[Seg::Move(p), Seg::Line((f64::NAN, 0.0))], 1.0).is_none());
        assert!(text(&[Seg::Move(p), Seg::Move(p)], 1.0).is_none());
        assert!(text(&[Seg::Line(p)], 1.0).is_none(), "a route starts with its move");
    }

    /// The corners of the `Geometry` section 0 rows of the test connector (from (1, 2) to (1, 4): its (u, v) is the page's (1 - v, 2 + u)), and the steps it took.
    fn corners_of(rows: Vec<std::sync::Arc<diacraft_doc::Row>>) -> (Option<Vec<(f64, f64)>>, u64) {
        let mut steps = 0;
        let found = corners(&connector(vec![(Some(0), rows)]), &Affine::IDENTITY, &mut steps);
        (found, steps)
    }

    fn same_as(found: &Option<Vec<(f64, f64)>>, want: &[(f64, f64)]) -> bool {
        found.as_ref().is_some_and(|f| f.len() == want.len() && f.iter().zip(want).all(|(a, b)| close(*a, *b)))
    }

    /// The milestone-1 rerun's W1: a route is the same route in whatever rows it is spelled. The corners leave out a hop (the chord's two ends lie on its
    /// leg, `ArcTo`, `PolylineTo` or the `MoveTo` of a gap) and the points on a leg, and every row costs a step.
    #[test]
    fn the_corners_of_a_route_leave_out_its_hops_and_the_points_on_its_legs() {
        // (0, 0), (1, 0), then up to (1, 2) with a hop on the way, then to (3, 2): the page's (1, 2), (1, 3), (-1, 3), (-1, 5).
        let want = [(1.0, 2.0), (1.0, 3.0), (-1.0, 3.0), (-1.0, 5.0)];
        let plain = vec![row("MoveTo", 0.0, 0.0, &[]), row("LineTo", 1.0, 0.0, &[]), row("LineTo", 1.0, 2.0, &[]), row("LineTo", 3.0, 2.0, &[])];
        assert!(same_as(&corners_of(plain).0, &want));
        for (kind, name) in [("ArcTo", "an arc"), ("PolylineTo", "a polyline"), ("MoveTo", "the gap of a jump")] {
            let rows = vec![
                row("MoveTo", 0.0, 0.0, &[]),
                row("LineTo", 1.0, 0.0, &[]),
                row("LineTo", 1.0, 0.8, &[]),
                row(kind, 1.0, 1.2, &[]),
                row("LineTo", 1.0, 2.0, &[]),
                row("LineTo", 3.0, 2.0, &[]),
            ];
            let (found, steps) = corners_of(rows);
            assert!(same_as(&found, &want), "{name}: {found:?}");
            assert_eq!(steps, 6, "{name}: a step for each row");
        }
        // A point on a leg is no corner, nor is a run of them, nor one that lies on the leg only once the one before it is gone.
        let spaced = vec![
            row("MoveTo", 0.0, 0.0, &[]),
            row("LineTo", 0.25, 0.0, &[]),
            row("LineTo", 0.5, 0.0, &[]),
            row("LineTo", 1.0, 0.0, &[]),
            row("LineTo", 1.0, 1.0, &[]),
            row("LineTo", 1.0, 2.0, &[]),
            row("LineTo", 3.0, 2.0, &[]),
        ];
        assert!(same_as(&corners_of(spaced).0, &want));
        // A point where the pen already is is none either.
        let twice = vec![row("MoveTo", 0.0, 0.0, &[]), row("LineTo", 0.0, 0.0, &[]), row("LineTo", 2.0, 0.0, &[])];
        assert!(same_as(&corners_of(twice).0, &[(1.0, 2.0), (1.0, 4.0)]));
        // A point past a leg's end, where the route turns back along it, is a corner: the route goes out and back.
        let back = vec![row("MoveTo", 0.0, 0.0, &[]), row("LineTo", 2.0, 0.0, &[]), row("LineTo", 1.0, 0.0, &[])];
        assert!(same_as(&corners_of(back).0, &[(1.0, 2.0), (1.0, 4.0), (1.0, 3.0)]));
        // A point a hair off the line (5E-7 inch) is on it, and one 2E-6 off is a corner.
        let near =
            |off: f64| corners_of(vec![row("MoveTo", 0.0, 0.0, &[]), row("LineTo", 1.0, off, &[]), row("LineTo", 2.0, 0.0, &[])]).0.map(|c| c.len());
        assert_eq!((near(5.0e-7), near(2.0e-6)), (Some(2), Some(3)));
    }

    /// The corners are taken to the page through the connector's own frame whatever way the connector points: from (0, 0) to (3, 4) its x axis runs along (0.6, 0.8), so
    /// (u, v) is the page's (0.6 u - 0.8 v, 0.8 u + 0.6 v), and from (3, 4) to (0, 0) the other way round.
    #[test]
    fn the_corners_of_a_slanted_connector_are_taken_to_the_page_through_its_frame() {
        let slanted = |(bx, by, ex, ey): (f64, f64, f64, f64)| {
            let mut sheet =
                connector(vec![(Some(0), vec![row("MoveTo", 0.0, 0.0, &[]), row("LineTo", 5.0, 0.0, &[]), row("LineTo", 5.0, 1.0, &[])])]);
            for (name, v) in [("BeginX", bx), ("BeginY", by), ("EndX", ex), ("EndY", ey)] {
                sheet.cells.insert(CellName::new(name), Cell::constant(Value::from_number(v).unwrap_or_else(|| Value::from_text("0"))));
            }
            corners(&sheet, &Affine::IDENTITY, &mut 0)
        };
        assert!(same_as(&slanted((0.0, 0.0, 3.0, 4.0)), &[(0.0, 0.0), (3.0, 4.0), (2.2, 4.6)]));
        assert!(same_as(&slanted((3.0, 4.0, 0.0, 0.0)), &[(3.0, 4.0), (0.0, 0.0), (0.8, -0.6)]));
        assert!(same_as(&slanted((1.0, 1.0, 1.0, 1.0)), &[(1.0, 1.0), (6.0, 1.0), (6.0, 2.0)]), "a connector of no length is not turned");
    }

    /// The review's M10: a point is dropped from the leg between its neighbours **again and again** while it lies on the leg the new point makes (a `while`, not an `if`):
    /// a (0, 0), b1 (5, 0.95E-6), b2 (7, -0.3E-6) and p (10, 0). b1 is 1.16E-6 inch off the leg a to b2, so it stays when b2 comes; b2 is 0.87E-6 off the leg b1 to p, so
    /// it goes when p comes; and then b1 is 0.95E-6 off the leg a to p, so it goes too, in the same pass. Only at the edge of the tolerance can the second go be needed.
    #[test]
    fn a_point_goes_from_the_leg_again_when_the_one_before_it_lies_on_the_leg_it_makes_in_turn() {
        let route =
            vec![row("MoveTo", 0.0, 0.0, &[]), row("LineTo", 5.0, 0.95e-6, &[]), row("LineTo", 7.0, -0.3e-6, &[]), row("LineTo", 10.0, 0.0, &[])];
        let (found, _) = corners_of(route);
        assert!(same_as(&found, &[(1.0, 2.0), (1.0, 12.0)]), "both go: {found:?}");
        // b1 further off (2E-6): it stays, though b2, 0.7E-6 off the leg b1 to p, still goes.
        let off = vec![row("MoveTo", 0.0, 0.0, &[]), row("LineTo", 5.0, 2.0e-6, &[]), row("LineTo", 7.0, 0.5e-6, &[]), row("LineTo", 10.0, 0.0, &[])];
        assert_eq!(corners_of(off).0.map(|c| c.len()), Some(3), "b1 stays, b2 goes");
    }

    /// What is not a route of lines is no corners: a curve, a route that does not start with its move, a deleted row, a row with no number, a point that is no
    /// number, one corner only, and a sheet with no ends or no route section 0.
    #[test]
    fn a_route_that_is_not_lines_has_no_corners() {
        let two = || vec![row("MoveTo", 0.0, 0.0, &[]), row("LineTo", 2.0, 0.0, &[])];
        assert!(corners_of(two()).0.is_some());
        let none = |rows: Vec<std::sync::Arc<diacraft_doc::Row>>| corners_of(rows).0;
        assert!(none(vec![row("MoveTo", 0.0, 0.0, &[]), row("NURBSTo", 2.0, 0.0, &[("E", nurbs(CUBIC))])]).is_none(), "a curve");
        assert!(none(vec![row("LineTo", 0.0, 0.0, &[]), row("LineTo", 2.0, 0.0, &[])]).is_none(), "the first row is a move");
        assert!(
            none(vec![row("MoveTo", 0.0, 0.0, &[]), row("ArcTo", 2.0, 0.0, &[]), row("Ellipse", 3.0, 0.0, &[])]).is_none(),
            "a row of another type"
        );
        assert!(none(vec![row("MoveTo", 0.0, 0.0, &[])]).is_none(), "one corner");
        assert!(
            same_as(&none(vec![row("MoveTo", 0.0, 0.0, &[]), row("LineTo", 0.0, 0.0, &[])]), &[(1.0, 2.0)]),
            "two points that are one: the one corner"
        );
        assert!(
            none(vec![row("MoveTo", 0.0, 0.0, &[]), row("LineTo", 0.0, 0.0, &[]), row("LineTo", 5.0e-7, 0.0, &[])]).is_some(),
            "three that are one"
        );
        assert!(none(vec![]).is_none());
        // A row deleted here, with its numbers and all: the route is not the rows that remain.
        let mut deleted = (*row("LineTo", 2.0, 0.0, &[])).clone();
        deleted.del = true;
        assert!(none(vec![row("MoveTo", 0.0, 0.0, &[]), std::sync::Arc::new(deleted)]).is_none(), "a deleted row");
        let mut no_y = (*row("LineTo", 2.0, 0.0, &[])).clone();
        no_y.cells.remove("Y");
        assert!(none(vec![row("MoveTo", 0.0, 0.0, &[]), std::sync::Arc::new(no_y)]).is_none(), "a row with no Y");
        assert!(none(vec![row("MoveTo", 0.0, 0.0, &[]), row("LineTo", f64::INFINITY, 0.0, &[])]).is_none(), "a point at infinity");
        // One coordinate that is not a number, the other one is: a group's map that sends x to infinity and y nowhere strange.
        let mut steps = 0;
        let far = Affine { e: f64::INFINITY, ..Affine::IDENTITY };
        assert!(corners(&connector(vec![(Some(0), two())]), &far, &mut steps).is_none(), "a corner with an x at infinity and a y that is a number");
        let far_y = Affine { f: f64::INFINITY, ..Affine::IDENTITY };
        assert!(corners(&connector(vec![(Some(0), two())]), &far_y, &mut steps).is_none(), "and the other way");
        let mut steps = 0;
        let other_section = connector(vec![(Some(1), two())]);
        assert!(corners(&other_section, &Affine::IDENTITY, &mut steps).is_none() && steps == 0, "no section 0 and no step taken");
        let mut no_ends = connector(vec![(Some(0), two())]);
        no_ends.cells.remove("EndY");
        assert!(corners(&no_ends, &Affine::IDENTITY, &mut steps).is_none());
        // A group around the connector moves the corners on the page.
        let moved = corners(&connector(vec![(Some(0), two())]), &Affine { e: 10.0, f: 20.0, ..Affine::IDENTITY }, &mut steps);
        assert!(same_as(&moved, &[(11.0, 22.0), (11.0, 24.0)]), "{moved:?}");
    }

    /// The limits of the two helpers: a point is on a leg within 1E-6 inch of it and strictly between its ends, and two routes' corners are the same within 1E-6
    /// each way, with as many corners.
    #[test]
    fn a_point_is_on_a_leg_within_a_millionth_of_an_inch_and_between_its_ends_and_corners_are_the_same_within_it() {
        let (a, c) = ((0.0, 0.0), (10.0, 0.0));
        assert!(on_leg(a, (5.0, 0.0), c));
        assert!(on_leg(a, (5.0, 9.0e-7), c) && on_leg(a, (5.0, -9.0e-7), c));
        assert!(!on_leg(a, (5.0, 1.1e-6), c) && !on_leg(a, (5.0, -1.1e-6), c), "off the line");
        assert!(!on_leg(a, a, c) && !on_leg(a, c, c), "at an end");
        assert!(!on_leg(a, (9.0e-7, 0.0), c) && !on_leg(a, (10.0 - 9.0e-7, 0.0), c), "within a millionth of an end is at it");
        assert!(on_leg(a, (1.2e-6, 0.0), c) && on_leg(a, (10.0 - 1.2e-6, 0.0), c));
        assert!(!on_leg(a, (-1.0, 0.0), c) && !on_leg(a, (11.0, 0.0), c), "past an end");
        assert!(!on_leg(a, (0.0, 0.0), a) && !on_leg(a, (5.0, 0.0), (5.0e-7, 0.0)), "a leg of no length has nothing on it");
        assert!(!on_leg(a, (5.0, f64::NAN), c) && !on_leg(a, (5.0, 0.0), (f64::NAN, 0.0)));
        // Exactly a millionth from an end is at it (a leg of length 1 makes the distances exact): not between, and a hair further in is.
        let (one_a, one_c) = ((0.0, 0.0), (1.0, 0.0));
        assert!(!on_leg(one_a, (SAME_CORNER, 0.0), one_c) && !on_leg(one_a, (1.0 - SAME_CORNER, 0.0), one_c));
        assert!(on_leg(one_a, (2.0 * SAME_CORNER, 0.0), one_c) && on_leg(one_a, (1.0 - 2.0 * SAME_CORNER, 0.0), one_c));
        // The diagonal: the same on a slanted leg.
        assert!(on_leg((0.0, 0.0), (3.0, 4.0), (6.0, 8.0)) && !on_leg((0.0, 0.0), (3.0, 4.01), (6.0, 8.0)));
        let corners = [(0.0, 0.0), (1.0, 0.0), (1.0, 2.0)];
        assert!(same_corners(&corners, &corners));
        assert!(same_corners(&corners, &[(0.0, 9.0e-7), (1.0, -9.0e-7), (1.0 + 9.0e-7, 2.0)]), "each within a millionth");
        assert!(
            !same_corners(&corners, &[(0.0, 1.1e-6), (1.0, 0.0), (1.0, 2.0)]) && !same_corners(&corners, &[(1.1e-6, 0.0), (1.0, 0.0), (1.0, 2.0)])
        );
        assert!(!same_corners(&corners, &corners[..2]) && !same_corners(&corners[..2], &corners), "as many corners");
        assert!(same_corners(&[], &[]));
    }

    /// A number written to `places` places after what `before` holds, as `push_number` writes it.
    fn written(before: &str, value: f64, places: usize) -> Option<String> {
        let mut out = before.to_owned();
        push_number(&mut out, value, places)?;
        Some(out.get(before.len()..).unwrap_or_default().to_owned())
    }

    /// A number as a path writes it: to the places asked for, with no trailing zeros, no negative zero, and none past the bound a path can hold; and set apart from a
    /// digit or a point before it, unless it begins with its sign. A number that fails leaves what was written as it was.
    #[test]
    fn a_number_is_written_to_the_places_asked_for_with_no_trailing_zeros() {
        for (value, places, text) in [
            (12000.0, 4, "12000"),
            (-2000.0, 4, "-2000"),
            (0.0, 4, "0"),
            (-0.0, 4, "0"),
            (-0.00001, 4, "0"),
            (10012.3456, 4, "10012.3456"),
            (10012.34564, 4, "10012.3456"),
            (10012.34566, 4, "10012.3457"),
            (0.5, 4, "0.5"),
            (-0.25, 4, "-0.25"),
            (100.0, 4, "100"),
            (1.0e12, 4, "1000000000000"),
            (-1.0e12, 4, "-1000000000000"),
            // More places where they are asked for, and none where there are none.
            (100.00003, 5, "100.00003"),
            (100.00003, 4, "100"),
            (-0.000004, 5, "0"),
            (0.1234567, 10, "0.1234567"),
            (2.0, 0, "2"),
            (2.6, 0, "3"),
        ] {
            assert_eq!(written("", value, places).as_deref(), Some(text), "{value} to {places}");
        }
        for value in [1.0e12 + 1.0, -1.0e12 - 1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1.0e300] {
            assert_eq!(written("", value, 4), None, "{value}");
        }
        // A space between it and a digit or a point before it, none after a letter or a space or before a sign, and for a negative zero (which has no sign left) one.
        for (before, value, text) in [
            ("M 1", 2.0, " 2"),
            ("M 1.", 2.0, " 2"),
            ("M", 2.0, "2"),
            ("M ", 2.0, "2"),
            ("M,", 2.0, "2"),
            ("M1", -2.0, "-2"),
            ("M1", -0.00001, " 0"),
            ("", 5.0, "5"),
        ] {
            assert_eq!(written(before, value, 4).as_deref(), Some(text), "{before:?} then {value}");
        }
        let mut out = String::from("M 1");
        assert_eq!(push_number(&mut out, f64::NAN, 4), None);
        assert_eq!(out, "M 1", "a number that fails leaves what was written");
    }

    /// The places a number's text has: the digits after its point, taken from by its exponent, between none and ten.
    #[test]
    fn the_places_a_number_has_count_its_exponent_and_stop_at_ten() {
        for (text, places) in [
            ("10000", 0),
            ("0.5", 1),
            ("0.00003", 5),
            ("-12.34", 2),
            ("5e-6", 6),
            ("2.5e1", 0),
            ("2.5e0", 1),
            ("2.5E-3", 4),
            ("1e+2", 0),
            ("1.5e+1", 0),
            (".25", 2),
            ("1.", 0),
            ("0.0000000001", 10),
            ("0.00000000001", 10),
            ("1e-400", 10),
            ("1e-99999999999999999999", 0),
            ("", 0),
        ] {
            assert_eq!(places_of(text), places, "{text}");
        }
    }

    /// The path of a connector moved down the page: the `y` of each absolute command and of the first move moves, nothing else does, and every other byte
    /// is copied as it was written.
    #[test]
    fn a_path_is_moved_down_the_page_by_the_y_of_its_absolute_commands() {
        let cases = [
            // LibreOffice's own form: the first absolute move, then relative steps.
            ("M 4000 10000 v 2000 h 5000 v -6000 h 2000 v 1000", 2000.0, "M 4000 12000 v 2000 h 5000 v -6000 h 2000 v 1000"),
            ("M 4000 10000 v 2000 h 5000 v -6000 h 2000 v 1000", -3000.0, "M 4000 7000 v 2000 h 5000 v -6000 h 2000 v 1000"),
            ("M 4000 10000 v 2000", 0.0, "M 4000 10000 v 2000"),
            // Absolute points: every y.
            ("M 1 2 L 3 4 L 5 6", 10.0, "M 1 12 L 3 14 L 5 16"),
            ("M 1 2 H 3 V 4 H 5 V 6", 10.0, "M 1 12 H 3 V 14 H 5 V 16"),
            ("M 1 2 C 3 4 5 6 7 8", 10.0, "M 1 12 C 3 14 5 16 7 18"),
            ("M 1 2 S 3 4 5 6", 10.0, "M 1 12 S 3 14 5 16"),
            ("M 1 2 Q 3 4 5 6", 10.0, "M 1 12 Q 3 14 5 16"),
            ("M 1 2 T 3 4", 10.0, "M 1 12 T 3 14"),
            // An arc: radii, turn and flags stay, the end's y moves.
            ("M 1 2 A 3 4 5 1 0 6 7", 10.0, "M 1 12 A 3 4 5 1 0 6 17"),
            ("M 1 2 A 3 4 5 10 6 7", 10.0, "M 1 12 A 3 4 5 10 6 17"),
            // Relative commands move with the pen.
            (
                "M 1 2 l 3 4 c 1 2 3 4 5 6 s 1 2 3 4 q 1 2 3 4 t 5 6 a 1 2 3 0 1 4 5 h 1 v 2 z",
                10.0,
                "M 1 12 l 3 4 c 1 2 3 4 5 6 s 1 2 3 4 q 1 2 3 4 t 5 6 a 1 2 3 0 1 4 5 h 1 v 2 z",
            ),
            // The first move is absolute whatever its case; the pairs after a lower-case move are relative lines, after an upper-case one absolute lines.
            ("m 1 2 3 4 5 6", 10.0, "m 1 12 3 4 5 6"),
            ("M 1 2 3 4 5 6", 10.0, "M 1 12 3 14 5 16"),
            ("M 1 2 L 3 4 5 6", 10.0, "M 1 12 L 3 14 5 16"),
            ("M 1 2 l 3 4 5 6", 10.0, "M 1 12 l 3 4 5 6"),
            // A later move is as its case says; a close takes the pen back to the start, and a relative step after it is relative to that.
            ("M 1 2 L 3 4 Z m 5 6 l 7 8 M 9 10", 10.0, "M 1 12 L 3 14 Z m 5 6 l 7 8 M 9 20"),
            // Separators, signs and numbers that run together are kept, and a number that moves is set apart from its neighbours.
            ("M1,2L3-4 5-6", 10.0, "M1,12L3 6 5 4"),
            // A number that goes negative and follows a digit is not set apart: its sign does that (the clause of `shifted` that leaves out the space before a `-`).
            ("M1-2L3-4", -10.0, "M1-12L3-14"),
            ("M 1 2 L 3 -4", 10.0, "M 1 12 L 3 6"),
            ("M 1 2 L 3 -4", 4.0, "M 1 6 L 3 0"),
            ("M .5.5 L 1.5.5", 10.0, "M .5 10.5 L 1.5 10.5"),
            ("M 1 2.5 .5 .5", 10.0, "M 1 12.5 .5 10.5"),
            ("M 1 2.5.5.5", 10.0, "M 1 12.5 .5 10.5"),
            ("  M\t1\n2  ", 10.0, "  M\t1\n12  "),
            ("M 1e1 2e1 L +3 +4", 10.0, "M 1e1 30 L +3 14"),
            ("M 1 2E0", 10.0, "M 1 12"),
            ("M 1e+1 2E-1 L 3.5e+1 4e-0", 10.0, "M 1e+1 10.2 L 3.5e+1 14"),
            // (ruling (a)) A path the reader reads that begins with no move starts at the origin: a move to (0, by) is put in front, and every absolute `y`
            // moves with it, so the path moves exactly. The move is not put in where nothing moves.
            ("L 4000 10000 v 2000", 10.0, "M 0 10 L 4000 10010 v 2000"),
            ("l 4000 10000 v 2000", 2000.0, "M 0 2000 l 4000 10000 v 2000"),
            ("H 5 V 9", 10.0, "M 0 10 H 5 V 19"),
            ("C 1 2 3 4 5 6", 10.0, "M 0 10 C 1 12 3 14 5 16"),
            ("A 3 4 5 1 0 6 7", 10.0, "M 0 10 A 3 4 5 1 0 6 17"),
            ("  L 1 2", 10.0, "M 0 10  L 1 12"),
            ("L1,2", -10.0, "M 0 -10 L1,-8"),
            ("Z", 10.0, "M 0 10 Z"),
            ("z M 1 2", 10.0, "M 0 10 z M 1 12"),
            ("ZZZ", 10.0, "M 0 10 ZZZ"),
            ("L 1 2", 0.0, "L 1 2"),
            ("L 1 2", 0.00004, "L 1 2"),
            // (ruling (b)) Fractions to four places, or to the places the number had when it had more (up to ten): a shift must not turn two equal points into two that are
            // not, which draws an arc between them as a near-full circle.
            ("M 0 0 l 1 0.00003 A 5 5 0 1 1 1 0.00003", 100.0, "M 0 100 l 1 0.00003 A 5 5 0 1 1 1 100.00003"),
            ("M 0 0.00001", 0.5, "M 0 0.50001"),
            ("M 0 5e-6", 1.0, "M 0 1.000005"),
            ("M 0 2E+1", 1.0, "M 0 21"),
            ("M 0 1.23456789012345", 1.0, "M 0 2.2345678901"),
            ("M 0 0.0000000000001", 1.0, "M 0 1"),
            ("M 4000 10000", 12.3456, "M 4000 10012.3456"),
            ("M 4000 10012.3456", -12.3456, "M 4000 10000"),
            ("M 0 0", 0.00004, "M 0 0"),
            ("M 0 0", 0.00006, "M 0 0.0001"),
            // Nothing to move.
            ("", 10.0, ""),
            ("   ", 10.0, "   "),
        ];
        for (path, by, expected) in cases {
            assert_eq!(shifted(path, by).as_deref(), Some(expected), "{path:?} by {by}");
        }
    }

    /// A path the writer cannot place gives `None` (the caller keeps it and says so): data that breaks SVG's syntax as the reader reads it, one that begins with no move, a
    /// number or a result past the bound, a shift that is not a number, a path past the size the pass reads.
    #[test]
    fn a_path_that_cannot_be_placed_is_given_back_as_none() {
        for path in [
            // A path with no move first is readable (ruling (a)), but one that is no data at all is not: numbers with no command, a command with no numbers.
            "5 6",
            "L",
            "H",
            "Z 1 2",
            "M",
            "M 1",
            "M 1 2 L 3",
            "M 1 2 X 3 4",
            "M 1 2 Z 3 4",
            "M 1 2 A 3 4 5 2 0 6 7",
            "M 1 2 A 3 4 5 1",
            "M 1 2 L e 4",
            "M 1 5e",
            "M 1 5e+",
            "M 1 2 L 3 4e-",
            "M 1 2 L - 4",
            "M 1 2 L 3 4 .",
            "M 1 1e400",
            // An `x` that is no number the reader can hold: it stays as it is written, but it must still be a number.
            "M 1e400 2",
            "M 1 2 L 3 1e12",
            "M 1 2 L 3 -2e12",
            "M 1 1e300",
            "M \u{e9} 1",
        ] {
            assert_eq!(shifted(path, 10.0), None, "{path:?}");
        }
        for by in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(shifted("M 1 2", by), None, "{by}");
        }
        // Only the numbers that move are held to the bound; an x that is large and stays is as the file has it.
        assert_eq!(shifted("M 1e300 2", 10.0).as_deref(), Some("M 1e300 12"));
        // The size the pass reads: a path of exactly the most bytes is placed, one byte more is not.
        let at_most = |len: usize| {
            let mut path = String::from("M 0 0");
            while path.len() + 4 <= len {
                path.push_str(" 1 1");
            }
            while path.len() < len {
                path.push(' ');
            }
            path
        };
        assert!(shifted(&at_most(MOST_PATH_BYTES), 1.0).is_some());
        assert!(shifted(&at_most(MOST_PATH_BYTES + 1), 1.0).is_none());
    }

    /// The text a path makes is bounded by what the pass takes in: a path of pairs of one digit, every y moved by a number of eighteen characters, is at most about twenty
    /// times as long as it was.
    #[test]
    fn the_text_made_of_a_path_is_bounded_by_the_text_read() {
        let path: String = std::iter::once("M 0 0".to_owned()).chain((0..50_000).map(|_| " 1 1".to_owned())).collect();
        let moved = shifted(&path, 123456789012.1234).unwrap();
        assert!(moved.len() as u64 <= most_shifted_bytes(path.len()), "{} from {}", moved.len(), path.len());
        // Every pair after the move is an absolute line: all 50,001 y values moved, each as a number of its own.
        assert_eq!(moved.split_whitespace().count(), path.split_whitespace().count());
        assert!(moved.starts_with("M 0 123456789012.1234 1 123456789013.1234"), "{}", &moved[..60]);
    }

    /// **`most_shifted_bytes` is an upper bound of what `shifted` makes** (the memory a save charges for a shift before the text is made): the paths that
    /// make the most of it, a one-byte number made of eighteen characters with a space either side, a move put in front, and every path the generator makes, at the shifts
    /// that make the longest numbers. The worst of them is nine times as long (a one-byte number needs a byte after it to end, and the longest number is eighteen characters),
    /// so the bound of twenty is within a factor of three of what can be made.
    #[test]
    fn the_text_a_shift_makes_is_never_more_than_the_bound_the_save_charges() {
        let long = |unit: &str, head: &str| -> String { std::iter::once(head.to_owned()).chain((0..400).map(|_| unit.to_owned())).collect() };
        let mut paths = vec![
            long(" 1 1", "M 0 0"),
            long("V1", "M 0 0 "),
            long(" V1 1", "M 0 0"),
            long(".5.5", "M .5.5"),
            long("V-1-1", "M 0 0 "),
            long(" 1e-10 1e-10", "M 0 0"),
            long(" L1,1", "M 0 0"),
            long("1", "L"),
            long(" 9", "M 0 0 V"),
            long(" 1", "M 0 0 V"),
            "Z".to_owned(),
            "l 1 2".to_owned(),
            "Z Z Z".to_owned(),
        ];
        let mut rng = Rng(0xD1B5_4A32_D192_ED03);
        paths.extend((0..3000).map(|_| random_path(&mut rng)));
        let mut worst = 0.0f64;
        for path in paths.iter().filter(|p| !p.is_empty()) {
            for by in [999_999_999_998.999_9, -999_999_999_998.999_9, 12.3456, -0.5, 1.0, 123_456_789_012.123_4] {
                if let Some(moved) = shifted(path, by) {
                    let bound = most_shifted_bytes(path.len());
                    assert!(moved.len() as u64 <= bound, "{path:?} by {by} made {} bytes of {} the bound allows", moved.len(), bound);
                    assert!(moved.len() as u64 <= shift_peak(path.len()), "the peak holds the path and the text");
                    // The bound's constant of 64 is most of what a short path is allowed: the ratio is of the long ones.
                    if path.len() >= 400 {
                        worst = worst.max(moved.len() as f64 / path.len() as f64);
                    }
                }
            }
        }
        assert!(worst > 8.5 && worst < 10.0, "the worst of them is {worst} times as long: nine is the most a path of one-byte numbers makes");
        assert_eq!(most_shifted_bytes(0), 64);
        assert_eq!(most_shifted_bytes(100), 2064);
        assert_eq!(shift_peak(100), 2164);
        assert_eq!(most_shifted_bytes(usize::MAX), u64::MAX, "no wrap: a charge that saturates is refused");
    }

    /// **The pass is held to as many steps as the path has bytes** (thirteen cargo-mutants mutants of the tokenizer timed out): each step takes a byte at
    /// least, so a path of `n` bytes takes `n` steps at most, and one that would take more (a fault of the pass, which a step that took no byte would be) ends as `None` and
    /// not as a loop. The path that takes exactly as many steps as it has bytes is a run of closes after a move.
    #[test]
    fn the_pass_over_a_path_takes_at_most_a_step_for_each_byte_and_ends_when_it_would_take_more() {
        // `M 1 2 L 3 4` is two steps (the move and the line).
        assert_eq!(shifted_within("M 1 2 L 3 4", 10.0, 2).as_deref(), Some("M 1 12 L 3 14"));
        assert_eq!(shifted_within("M 1 2 L 3 4", 10.0, 1), None, "a third of the steps it needs");
        assert_eq!(shifted_within("M 1 2 L 3 4", 10.0, 0), None);
        // Numbers after a command are steps of their own (the command repeated): `M 1 2 3 4 5 6` is three.
        assert_eq!(shifted_within("M 1 2 3 4 5 6", 10.0, 3).as_deref(), Some("M 1 12 3 14 5 16"));
        assert_eq!(shifted_within("M 1 2 3 4 5 6", 10.0, 2), None);
        // An empty path takes none.
        assert_eq!(shifted_within("", 10.0, 0).as_deref(), Some(""));
        // A byte a step: a run of closes, the path's own length (a path that begins with a close has a move put in front, which is no step).
        let closes = "Z".repeat(1000);
        assert_eq!(shifted(&closes, 10.0), Some(format!("M 0 10 {closes}")));
        assert_eq!(shifted_within(&closes, 10.0, 1000).map(|m| m.len()), Some(1007));
        assert_eq!(shifted_within(&closes, 10.0, 999), None, "one step fewer than the bytes it has");
        // Every path the generator makes takes no more steps than it has bytes (the cap `shifted` has is never the reason it gives one back).
        let mut rng = Rng(0x1234_5678_9ABC_DEF1);
        for _ in 0..5000 {
            let path = random_path(&mut rng);
            let (capped, free) = (shifted_within(&path, 7.0, path.len()), shifted_within(&path, 7.0, usize::MAX));
            assert_eq!(capped, free, "{path:?}");
        }
    }

    /// A path moved and moved back is the path it was, to the number: the text may be spelled another way where a number moved, but the numbers are the file's.
    #[test]
    fn a_path_moved_and_moved_back_has_the_numbers_it_had() {
        for path in [
            "M 4000 10000 v 2000 h 5000 v -6000 h 2000 v 1000",
            "M 9744 11192 L 9744 11500 L 9744 11804 L 7305 11804 L 7305 4172 L 7805 4172",
            "M 4000 10000 c 0 1000 5000 1000 5000 0",
            "m 1 2 l 3 4 z",
            "M 1,2 3,4 5,6",
        ] {
            let moved = shifted(path, 250.0).unwrap_or_else(|| panic!("{path}"));
            // Moved again by the opposite shift, the path is the one it was.
            let back = shifted(&moved, -250.0).unwrap_or_else(|| panic!("{moved}"));
            let numbers =
                |p: &str| -> Vec<f64> { p.split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-')).filter_map(|n| n.parse().ok()).collect() };
            assert_eq!(numbers(&back), numbers(path), "{path} through {moved}");
        }
    }

    /// A small generator of the tests' own (xorshift64*): the inputs are made by code, never taken from a file.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }

        fn below(&mut self, n: u64) -> u64 {
            self.next() % n.max(1)
        }

        fn pick<'a, T>(&mut self, of: &'a [T]) -> &'a T {
            &of[self.below(of.len() as u64) as usize]
        }
    }

    /// A number as a path may write it: a whole number, a fraction of binary places (so a sum of them is exact in a double and the reader's comparison of two points is
    /// not a matter of rounding), with five and six places among them, or an exponent form.
    fn random_number(rng: &mut Rng, signed: bool) -> String {
        let sign = if signed { *rng.pick(&["-", "+"]) } else { *rng.pick(&["", "", "-", "+"]) };
        match rng.below(10) {
            0..=3 => format!("{sign}{}", rng.below(50_000)),
            4..=7 => {
                let places = 1 + rng.below(6) as usize;
                let value = rng.below(40_000 << places) as f64 / (1u64 << places) as f64;
                let text = format!("{value:.places$}");
                // A trailing zero or two of the places may be trimmed, as a program writes them.
                let text = if rng.below(3) == 0 && text.contains('.') { text.trim_end_matches('0').trim_end_matches('.').to_owned() } else { text };
                format!("{sign}{text}")
            }
            8 => format!("{sign}{}{}", rng.below(90) + 1, rng.pick(&["e2", "E1", "e+1", "e-1", "E-2", "e0"])),
            _ => format!("{sign}{}.{}{}", rng.below(300), rng.below(10) * 5, rng.pick(&["e1", "e-1", ""])),
        }
    }

    /// A path of up to ten commands of any letter, upper or lower case, whatever the first one is, with the separators SVG allows, and sometimes a byte put wrong.
    fn random_path(rng: &mut Rng) -> String {
        let mut path = String::new();
        let first_move = rng.below(4) != 0;
        let mut signed = false;
        for k in 0..1 + rng.below(10) {
            let letter = if k == 0 && first_move { *rng.pick(b"Mm") } else { *rng.pick(b"MmLlHhVvCcSsQqTtAaZz") };
            path.push(letter as char);
            let count = match letter.to_ascii_uppercase() {
                b'M' | b'L' | b'T' => 2,
                b'H' | b'V' => 1,
                b'C' => 6,
                b'S' | b'Q' => 4,
                b'A' => 7,
                _ => 0,
            };
            // A command's own arguments, and sometimes another set after them (the command repeated).
            for i in 0..count * (1 + usize::from(rng.below(5) == 0)) {
                // An arc's fourth and fifth arguments are its flags.
                if letter.eq_ignore_ascii_case(&b'A') && matches!(i % 7, 3 | 4) {
                    path.push_str(rng.pick(&["0", "1"]));
                } else {
                    path.push_str(&random_number(rng, signed));
                }
                // No separator at all, which is a number run into the next: only where the next has a sign (or is a flag), which is what ends the one before.
                let separator = *rng.pick(&[" ", " ", ",", " , ", "\n", "  ", "\t", ""]);
                signed = separator.is_empty();
                path.push_str(separator);
            }
            path.push_str(rng.pick(&[" ", "", "\n"]));
        }
        // One time in eight, one byte of it is another: a path the reader may refuse.
        if rng.below(8) == 0 && !path.is_empty() {
            let at = rng.below(path.len() as u64) as usize;
            let other = *rng.pick(b"MLZe+-. x1\n,9A");
            let mut bytes = path.into_bytes();
            if let Some(slot) = bytes.get_mut(at) {
                *slot = other;
            }
            path = String::from_utf8(bytes).unwrap_or_default();
        }
        path
    }

    /// The rows the reader makes of a path, in a unit view box (a share is the coordinate and `1 - y` the other, as the reader's own route does it): `None` when it
    /// refuses the path.
    fn reader_rows(path: &str) -> Option<Vec<geometry::Row>> {
        let unit = geometry::ViewBox { left: 0.0, top: 0.0, width: 1.0, height: 1.0 };
        match geometry::path(path, &unit, (1.0, 1.0), &mut geometry::Budget::new(usize::MAX)) {
            Ok(Ok(sections)) => Some(sections.into_iter().flat_map(|s| s.rows).collect()),
            _ => None,
        }
    }

    /// Whether two rows are the same row to a millionth of their size.
    fn same_row(a: &geometry::Row, b: &geometry::Row) -> bool {
        use geometry::ArcAxes;
        use geometry::Row::{RelCubBezTo, RelEllipticalArcTo, RelLineTo, RelMoveTo, RelQuadBezTo};
        let close = |x: f64, y: f64| (x - y).abs() <= 1e-6 * (1.0 + x.abs());
        match (a, b) {
            (RelMoveTo(a, b), RelMoveTo(c, d)) | (RelLineTo(a, b), RelLineTo(c, d)) => close(*a, *c) && close(*b, *d),
            (RelCubBezTo { x, y, a, b, c, d }, RelCubBezTo { x: x2, y: y2, a: a2, b: b2, c: c2, d: d2 }) => {
                [(x, x2), (y, y2), (a, a2), (b, b2), (c, c2), (d, d2)].iter().all(|(p, q)| close(**p, **q))
            }
            (RelQuadBezTo { x, y, a, b }, RelQuadBezTo { x: x2, y: y2, a: a2, b: b2 }) => {
                [(x, x2), (y, y2), (a, a2), (b, b2)].iter().all(|(p, q)| close(**p, **q))
            }
            (RelEllipticalArcTo { x, y, a, b, axes }, RelEllipticalArcTo { x: x2, y: y2, a: a2, b: b2, axes: axes2 }) => {
                // The axes of a turned arc are worked out from the points (an arc of radii in a ratio of hundreds is ill-conditioned: the same to a millionth).
                let axes_alike = match (axes, axes2) {
                    (ArcAxes::Upright { k: p }, ArcAxes::Upright { k: q }) => close(*p, *q),
                    (ArcAxes::Turned { c0: p, d0: r }, ArcAxes::Turned { c0: q, d0: t }) => close(*p, *q) && close(*r, *t),
                    (ArcAxes::Values { d: p }, ArcAxes::Values { d: q }) => close(*p, *q),
                    _ => false,
                };
                axes_alike && [(x, x2), (y, y2), (a, a2), (b, b2)].iter().all(|(p, q)| close(**p, **q))
            }
            _ => false,
        }
    }

    /// The reader's rows of a path moved down the page by `by`: the same rows with each `y` (a share, `1 - y`) `by` less.
    fn moved_rows(rows: &[geometry::Row], by: f64) -> Vec<geometry::Row> {
        use geometry::Row::{RelCubBezTo, RelEllipticalArcTo, RelLineTo, RelMoveTo, RelQuadBezTo};
        rows.iter()
            .map(|row| match *row {
                RelMoveTo(x, y) => RelMoveTo(x, y - by),
                RelLineTo(x, y) => RelLineTo(x, y - by),
                RelCubBezTo { x, y, a, b, c, d } => RelCubBezTo { x, y: y - by, a, b: b - by, c, d: d - by },
                RelQuadBezTo { x, y, a, b } => RelQuadBezTo { x, y: y - by, a, b: b - by },
                RelEllipticalArcTo { x, y, a, b, axes } => RelEllipticalArcTo { x, y: y - by, a, b: b - by, axes },
            })
            .collect()
    }

    /// **The writer's reading of a path and the reader's are the same**: over paths of every command and separator, the ones that
    /// begin with no move and the ones with a byte put wrong among them, `shifted` gives a path exactly when the reader reads one, and the path it gives reads as the rows of
    /// the first with each `y` moved by the shift. The two share their tokenizer, so this holds the grammar of the commands (which arguments are a `y`) to the reader's.
    #[test]
    fn a_path_shifted_reads_as_the_path_moved_and_the_two_readings_agree_on_what_a_path_is() {
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        let (mut read, mut refused) = (0usize, 0usize);
        for n in 0..30_000 {
            let path = random_path(&mut rng);
            let by = *rng.pick(&[2000.0, -3000.0, 100.0, 12.3125, -7.0625, 0.5, 0.0]);
            let (rows, moved) = (reader_rows(&path), shifted(&path, by));
            match (rows, moved) {
                (Some(rows), Some(moved)) => {
                    read += 1;
                    let back = reader_rows(&moved).unwrap_or_else(|| panic!("#{n}: the reader refuses {moved:?}, made of {path:?} by {by}"));
                    // An arc whose radii are too small for its ends is scaled up to half a turn exactly, and whether it is cut into one piece or two is then a matter of the last bit
                    // of its sweep: such a path is held to what the reader reads and no further (the arcs that are held to their rows are `well_conditioned_arcs_move_by_their_ends`).
                    if path.contains(['A', 'a']) {
                        continue;
                    }
                    let want = moved_rows(&rows, by);
                    assert!(
                        back.len() == want.len() && back.iter().zip(&want).all(|(a, b)| same_row(a, b)),
                        "#{n}: {path:?} by {by} is {moved:?}\n   read {rows:?}\n   back {back:?}"
                    );
                    if by == 0.0 {
                        assert_eq!(moved, path, "#{n}: a shift of nothing is the path");
                    }
                    // And back by the opposite shift: the rows the reader made of the path.
                    let again = shifted(&moved, -by).unwrap_or_else(|| panic!("#{n}: {moved:?} cannot be moved back"));
                    let again_rows = reader_rows(&again).unwrap_or_default();
                    assert!(
                        again_rows.len() == rows.len() && again_rows.iter().zip(&rows).all(|(a, b)| same_row(a, b)),
                        "#{n}: {path:?} there and back is {again:?}"
                    );
                }
                (None, None) => refused += 1,
                // A path the reader reads with a coordinate past what a path may hold (the byte put wrong made `15642.e50`) cannot be moved: the writer keeps it as it is.
                (Some(rows), None)
                    if rows.iter().any(|row| {
                        format!("{row:?}")
                            .split(|c: char| !(c.is_ascii_digit() || c == '.' || c == 'e' || c == '-' || c == '+'))
                            .filter_map(|t| t.parse::<f64>().ok())
                            .any(|v| v.abs() > 1.0e11)
                    }) =>
                {
                    refused += 1;
                }
                (rows, moved) => panic!("#{n}: {path:?} by {by}: the reader reads it: {}, `shifted` gives {moved:?}", rows.is_some()),
            }
        }
        // The generator reaches both kinds of path.
        assert!(read > 15_000 && refused > 1_000, "{read} read and {refused} refused");
    }

    /// The cost of a shift against the reader's own pass over the same path (`shifted` was about five times the reader, with two allocations for each
    /// number that moved). A timing, so it is no part of the suite: run by hand (`cargo test -p diacraft-odg --release -- --ignored --nocapture the_shift_costs`) and read.
    #[test]
    #[ignore = "a timing, read by hand"]
    fn the_shift_costs_about_what_the_reader_does() {
        let path: String = std::iter::once("M 0 0".to_owned()).chain((0..60_000).map(|k| format!(" L {} {}", k % 977, (k * 7) % 1013))).collect();
        assert!(path.len() < MOST_PATH_BYTES, "{}", path.len());
        let time = |what: &dyn Fn() -> usize| {
            let started = std::time::Instant::now();
            let mut sum = 0;
            for _ in 0..30 {
                sum += what();
            }
            (started.elapsed(), sum)
        };
        let (reader, a) = time(&|| reader_rows(&path).map_or(0, |rows| rows.len()));
        let (shift, b) = time(&|| shifted(&path, 12.5).map_or(0, |text| text.len()));
        eprintln!("the reader {reader:?}, the shift {shift:?}: {:.2} times ({a} rows, {b} bytes)", shift.as_secs_f64() / reader.as_secs_f64());
    }

    /// The arcs the rows above are not held to: an arc whose radii are large for its ends (never scaled up, so the sweep is not half a turn exactly), absolute and relative, of
    /// every flag, after a move, a line or another arc. The rows the reader makes of the shifted path are those of the path with every `y` moved.
    #[test]
    fn well_conditioned_arcs_move_by_their_ends() {
        let mut rng = Rng(0xA5A5_5A5A_1357_9BDF);
        let mut arcs = 0;
        for n in 0..4000 {
            let mut path = format!("M {} {}", rng.below(900), rng.below(900));
            for _ in 0..1 + rng.below(3) {
                let (letter, flags) = (*rng.pick(&["A", "a"]), *rng.pick(&["0 0", "0 1", "1 0", "1 1", "11", "01"]));
                // The ends are within a thousand of the origin, the radii two to four thousand: no arc is scaled.
                path.push_str(&format!(
                    " {letter} {} {} {} {flags} {} {}",
                    2000 + rng.below(2000),
                    2000 + rng.below(2000),
                    rng.below(180),
                    rng.below(900),
                    rng.below(900)
                ));
            }
            let by = *rng.pick(&[2000.0, -3000.0, 12.3125, 0.5]);
            let (Some(rows), Some(moved)) = (reader_rows(&path), shifted(&path, by)) else {
                panic!("#{n}: {path:?}: a path the reader reads and `shifted` moves")
            };
            let back = reader_rows(&moved).unwrap_or_else(|| panic!("#{n}: the reader refuses {moved:?}"));
            let want = moved_rows(&rows, by);
            assert!(back.len() == want.len() && back.iter().zip(&want).all(|(a, b)| same_row(a, b)), "#{n}: {path:?} by {by} is {moved:?}");
            arcs += rows.iter().filter(|r| matches!(r, geometry::Row::RelEllipticalArcTo { .. })).count();
        }
        assert!(arcs > 4000, "{arcs} arcs among the rows");
    }
}
