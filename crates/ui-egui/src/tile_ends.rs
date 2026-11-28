//! The line ends and the pattern of a tile.
//!
//! The tile of a one-dimensional master of DiaCraft's own (a connector, or a figure drawn along a line) is a short line with the marks the master's
//! cells resolve to: `stencil.list` gives each such master `ends: {begin, end, pattern}`, the numbers of `BeginArrow`, `EndArrow` and `LinePattern`
//! (the window knows no master by name). Each mark has the kind its number has, **drawn in code, in that
//! specification's box** (how far back from the vertex it reaches and how wide it spreads, in units of `S`), with the tip on the end
//! vertex and the body lying back along the line. A tile is not a page: the unit `S` is [`UNIT`] of the figure's longest side, whatever the
//! line's weight, and it is made smaller when two marks would take more than [`MARKS_SHARE`] of a short line.
//!
//! * **Paint.** A *solid* mark is filled with the pen's ink, a *hollow* one is an outline whose closed part is painted with the tile's fill (`Tokens::stencil_fill`, transparent in both themes,
//!   as the body of every tile is, the marks of a page taking the shape's fill), an *open* one is strokes. The tile has the graphite treatment of both themes: the
//!   colours come from the stencil ink and fill tokens and nothing else.
//! * **The line** stops short of a solid or a hollow mark so that nothing shows through or past its back, and runs on to the vertex of an open
//!   one. It follows the pattern's table (marks and gaps, in line weights); **the marks are never dashed**.
//! * **Where.** On every open path of the figure's lines, at its first vertex for the begin and its last for the end, turned along the
//!   first and the last segment that has a direction. A closed path has none.
//! * **A custom end (254) is drawn as number 4**, the plain end, as the page draws it: the list gives the number and not the master, so a
//!   tile cannot show the master's own art. A number that is no mark (past 45, and not 254) draws nothing, as on a page.
//! * **A line of pattern 0** is drawn solid: on a page it is no line, and a tile that shows nothing cannot be found.

use egui::{Color32, Mesh, Painter, Pos2, Shape, Stroke};
use serde_json::Value;

use crate::edit::geom::{self, P};
use crate::stencils::Figure;

/// A mark's unit `S` on a tile, as a share of the figure's longest side: the Triangle (number 4, 2 `S` long and 2 across) is 0.18 of the line long, as the head of the
/// Data Flow's tile always was.
pub const UNIT: f64 = 0.09;

/// The most of a line the two marks may take together; past it `S` is made smaller, so that a short line keeps a shaft between a long diamond and a crow's foot.
const MARKS_SHARE: f64 = 0.6;

/// How far inside the back of a solid mark the line stops, in units of `S` (the line runs under a solid mark, so that no seam shows).
const OVERLAP: f64 = 0.1;

/// The most open paths of a figure that get marks.
const MAX_LINES: usize = 16;

/// How many sides the polygon that stands for a disc has: a multiple of four, so that its top, bottom, left and right are on it.
const DISC_SIDES: usize = 16;

/// How long a mark of a pattern is, in line weights as the table gives it, in the tile's pen widths (a pattern of the page, drawn on a figure a fifth of its size).
const DASH_SCALE: f32 = 0.75;

/// The ends and the pattern a one-dimensional master has: `stencil.list`'s `ends`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ends {
    /// `BeginArrow`: 0 for none, 1 to 45, or 254 for a master used as the end.
    pub begin: u8,
    /// `EndArrow`.
    pub end: u8,
    /// `LinePattern`: 1 solid, 2 to 23 the patterns of the table.
    pub pattern: u8,
}

impl Ends {
    /// No marks and a solid line: what a one-dimensional master has when the list says nothing.
    pub(crate) const PLAIN: Ends = Ends { begin: 0, end: 0, pattern: 1 };

    /// The ends of a master in an answer of `stencil.list`; nothing in it is trusted. `None` for an entry that is not an object with the three numbers, a number
    /// that is no byte is 0 (no mark), and a pattern past 23 is a solid line.
    pub(crate) fn read(entry: &Value) -> Option<Ends> {
        let byte = |key: &str| entry.get(key).and_then(Value::as_u64).and_then(|n| u8::try_from(n).ok());
        entry.as_object()?;
        let pattern = byte("pattern").filter(|p| (1..=23).contains(p)).unwrap_or(1);
        Some(Ends { begin: byte("begin").unwrap_or(0), end: byte("end").unwrap_or(0), pattern })
    }
}

// ---- the marks ------------------------------------------------------------------------------------------------------------------------------

/// A piece of a mark: points in units of `S`, `x` back from the tip along the line and `y` across it. A closed piece does not repeat its first point.
#[derive(Clone, Debug, PartialEq)]
enum Piece {
    /// A closed form filled with ink.
    Solid(Vec<(f64, f64)>),
    /// A closed form that is an outline only.
    Hollow(Vec<(f64, f64)>),
    /// Strokes with no closed area.
    Open(Vec<(f64, f64)>),
}

impl Piece {
    fn points(&self) -> &[(f64, f64)] {
        match self {
            Piece::Solid(p) | Piece::Hollow(p) | Piece::Open(p) => p,
        }
    }
}

/// One line end: its pieces, and how far from the vertex the line stops (0 for an open mark).
#[derive(Clone, Debug, PartialEq)]
struct Mark {
    pieces: Vec<Piece>,
    stop: f64,
}

impl Mark {
    /// How far back from the vertex the mark reaches, and how far the line stops: the longer of the two, for the room two marks take.
    fn room(&self) -> f64 {
        let reach = self.pieces.iter().flat_map(|p| p.points().iter().map(|q| q.0)).fold(0.0_f64, f64::max);
        reach.max(self.stop)
    }

    /// Whether the rearmost closed piece, at whose back the line stops, is solid: the line then runs [`OVERLAP`] under it.
    fn solid_back(&self) -> bool {
        let back = |p: &Piece| p.points().iter().map(|q| q.0).fold(f64::MIN, f64::max);
        let rearmost = self.pieces.iter().filter(|p| !matches!(p, Piece::Open(_))).max_by(|a, b| back(a).total_cmp(&back(b)));
        matches!(rearmost, Some(Piece::Solid(_)))
    }
}

fn triangle(from: f64, len: f64) -> Vec<(f64, f64)> {
    vec![(from, 0.0), (from + len, 1.0), (from + len, -1.0)]
}

/// A head 2 long and `half` across at its back corners, the back cut in to `apex`.
fn head(half: f64, apex: f64) -> Vec<(f64, f64)> {
    vec![(0.0, 0.0), (2.0, half), (apex, 0.0), (2.0, -half)]
}

fn diamond(from: f64) -> Vec<(f64, f64)> {
    vec![(from, 0.0), (from + 2.25, 1.12), (from + 4.5, 0.0), (from + 2.25, -1.12)]
}

/// A disc 1.5 across with its centre `x` back from the tip.
fn disc(x: f64) -> Vec<(f64, f64)> {
    (0..DISC_SIDES)
        .map(|k| {
            let turn = std::f64::consts::TAU * k as f64 / DISC_SIDES as f64;
            (x + 0.75 * turn.cos(), 0.75 * turn.sin())
        })
        .collect()
}

/// A square 1.5 across with its centre `x` back from the tip.
fn square(x: f64) -> Vec<(f64, f64)> {
    vec![(x - 0.75, -0.75), (x + 0.75, -0.75), (x + 0.75, 0.75), (x - 0.75, 0.75)]
}

fn open_head(from: f64, len: f64) -> Vec<(f64, f64)> {
    vec![(from + len, 1.0), (from, 0.0), (from + len, -1.0)]
}

fn bar(x: f64) -> Vec<(f64, f64)> {
    vec![(x, 0.75), (x, -0.75)]
}

fn fork() -> Vec<(f64, f64)> {
    vec![(0.0, 0.75), (1.5, 0.0), (0.0, -0.75)]
}

/// The line run along the axis from the vertex to `to`, where the line of a mark must show between its parts.
fn spine(to: f64) -> Vec<(f64, f64)> {
    vec![(0.0, 0.0), (to, 0.0)]
}

/// A disc at the tip, hollow (31 to 33) or solid (35 to 37), with these bars behind it.
fn disc_and_bars(number: u8, bars: &[f64]) -> Mark {
    let first = if number < 35 { Piece::Hollow(disc(0.75)) } else { Piece::Solid(disc(0.75)) };
    Mark { pieces: std::iter::once(first).chain(bars.iter().map(|x| Piece::Open(bar(*x)))).collect(), stop: 1.5 }
}

/// The mark of a line end number: the kind gives it, in the box. **254, a master used as the end, is drawn as number 4**
/// 0, and a number that is none of the 45, have none.
fn mark(number: u8) -> Option<Mark> {
    let make = |pieces: Vec<Piece>, stop: f64| Some(Mark { pieces, stop });
    let (solid, hollow, open) = (Piece::Solid, Piece::Hollow, Piece::Open);
    match number {
        1 => make(vec![open(open_head(0.0, 1.0))], 0.0),
        2 => make(vec![solid(triangle(0.0, 1.0))], 1.0),
        3 => make(vec![open(open_head(0.0, 2.0))], 0.0),
        4 | 254 => make(vec![solid(triangle(0.0, 2.0))], 2.0),
        5 => make(vec![solid(head(0.995, 1.75))], 1.75),
        6 => make(vec![solid(head(1.01, 2.25))], 2.25),
        7 => make(vec![open(vec![(1.84, 0.96), (0.5, 0.48), (0.08, 0.0), (0.5, -0.48), (1.84, -0.96)])], 0.0),
        8 => make(vec![solid(head(1.0, 1.5))], 1.5),
        9 => make(vec![open(vec![(-1.0, 1.0), (1.0, -1.0)])], 0.0),
        10 | 42 => make(vec![solid(disc(0.0))], 0.75),
        11 => make(vec![solid(square(0.0))], 0.75),
        12 => make(vec![open(open_head(0.0, 3.0))], 0.0),
        13 => make(vec![solid(triangle(0.0, 3.0))], 3.0),
        14 => make(vec![hollow(triangle(0.0, 3.0))], 3.0),
        15 => make(vec![hollow(triangle(0.0, 1.0))], 1.0),
        16 => make(vec![hollow(triangle(0.0, 2.0))], 2.0),
        17 => make(vec![hollow(head(1.0, 1.75))], 1.75),
        18 => make(vec![hollow(head(1.01, 2.25))], 2.25),
        19 => make(vec![hollow(head(1.0, 1.5))], 1.5),
        20 => make(vec![hollow(disc(0.0))], 0.75),
        21 => make(vec![hollow(square(0.0))], 0.75),
        22 => make(vec![hollow(diamond(0.0))], 4.5),
        23 => make(vec![open(vec![(1.5, -1.0), (2.5, 1.0)])], 0.0),
        24 => make(vec![open(bar(1.5))], 0.0),
        25 => make(vec![open(bar(1.5)), open(bar(2.25))], 0.0),
        26 => make(vec![open(bar(1.5)), open(bar(2.25)), open(bar(3.0))], 0.0),
        27 => make(vec![open(fork())], 0.0),
        28 => make(vec![open(fork()), open(bar(2.0))], 0.0),
        29 => make(vec![open(fork()), hollow(disc(2.25))], 3.0),
        30 => make(vec![open(bar(0.75)), open(spine(1.5)), hollow(disc(2.25))], 3.0),
        31 | 35 => Some(disc_and_bars(number, &[2.25])),
        32 | 36 => Some(disc_and_bars(number, &[2.25, 3.0])),
        33 | 37 => Some(disc_and_bars(number, &[2.25, 3.0, 3.75])),
        34 => make(vec![hollow(disc(0.75)), hollow(diamond(1.5))], 6.0),
        38 => make(vec![solid(disc(0.75)), hollow(diamond(1.5))], 6.0),
        39 | 40 => {
            let (first, second) = (triangle(0.0, 2.0), vec![(2.0, 0.0), (4.0, 1.0), (4.0, -1.0)]);
            let paint = |form: Vec<(f64, f64)>| if number == 40 { Piece::Hollow(form) } else { Piece::Solid(form) };
            make(vec![paint(first), paint(second)], 4.0)
        }
        41 => make(vec![hollow(disc(0.75))], 1.5),
        43 => make(vec![open(open_head(0.0, 2.0)), open(open_head(2.0, 2.0))], 0.0),
        44 => make(vec![open(open_head(0.0, 2.0)), open(bar(2.75))], 0.0),
        45 => make(vec![open(open_head(0.0, 2.0)), open(open_head(2.0, 2.0)), open(bar(4.75))], 0.0),
        _ => None,
    }
}

// ---- the marks on a line ---------------------------------------------------------------------------------------------------------------

/// What the marks of a figure's lines are made of, in the figure's own units.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Decor {
    /// Solid marks: closed polygons (the first point repeated at the end, as a figure's areas have it) and the triangles that fill them.
    pub fills: Vec<(Vec<P>, Vec<[usize; 3]>)>,
    /// Hollow and open marks, as polylines; a hollow one repeats its first point at the end.
    pub strokes: Vec<Vec<P>>,
}

fn sub(a: P, b: P) -> P {
    (a.0 - b.0, a.1 - b.1)
}

fn length(v: P) -> f64 {
    v.0.hypot(v.1)
}

/// The unit vector from `from` toward the first of `others` that is not on it: the way the line runs from a vertex, which is where a mark's body lies. `None` for a line that
/// has no direction there, or whose points are not numbers.
fn direction<'a>(from: P, others: impl Iterator<Item = &'a P>) -> Option<P> {
    others.map(|p| sub(*p, from)).find(|v| length(*v) > 1.0e-9).filter(|v| length(*v).is_finite()).map(|v| (v.0 / length(v), v.1 / length(v)))
}

/// The two ends of an open path and the way it runs from each (toward the path): `((first, last), (from the first, from the last))`. `None` for a path that has no
/// direction at an end.
fn axes(line: &[P]) -> Option<((P, P), (P, P))> {
    let (first, last) = (line.first().copied()?, line.last().copied()?);
    Some(((first, last), (direction(first, line.iter().skip(1))?, direction(last, line.iter().rev().skip(1))?)))
}

/// The length of a polyline.
fn path_length(line: &[P]) -> f64 {
    line.windows(2).map(|w| w.first().zip(w.get(1)).map_or(0.0, |(a, b)| length(sub(*b, *a)))).sum()
}

/// `line` with `start` taken off its beginning and `end` off its finish, measured along it. Empty when they take it all.
fn trimmed(line: &[P], start: f64, end: f64) -> Vec<P> {
    let total = path_length(line);
    let (from, to) = (start.max(0.0), total - end.max(0.0));
    let room = to - from;
    if room.is_nan() || room <= 1.0e-9 {
        return Vec::new();
    }
    let mut out: Vec<P> = Vec::with_capacity(line.len());
    let mut walked = 0.0;
    for pair in line.windows(2) {
        let (Some(a), Some(b)) = (pair.first().copied(), pair.get(1).copied()) else { continue };
        let leg = length(sub(b, a));
        if leg <= 0.0 {
            continue;
        }
        let at = |d: f64| (a.0 + (b.0 - a.0) * (d / leg), a.1 + (b.1 - a.1) * (d / leg));
        let (lo, hi) = ((from - walked).clamp(0.0, leg), (to - walked).clamp(0.0, leg));
        if hi > lo {
            if out.is_empty() {
                out.push(at(lo));
            }
            out.push(at(hi));
        }
        walked += leg;
    }
    out
}

/// `mark` drawn with its tip at `tip`, its body lying back along `back` (a unit vector), `s` long to the unit: its pieces added to `decor`.
fn place(mark: &Mark, s: f64, tip: P, back: P, decor: &mut Decor) {
    let across = (-back.1, back.0);
    let map = |(x, y): (f64, f64)| (tip.0 + back.0 * x * s + across.0 * y * s, tip.1 + back.1 * x * s + across.1 * y * s);
    for piece in &mark.pieces {
        let mut points: Vec<P> = piece.points().iter().map(|p| map(*p)).collect();
        match piece {
            Piece::Open(_) => decor.strokes.push(points),
            Piece::Hollow(_) | Piece::Solid(_) => {
                if let Some(first) = points.first().copied() {
                    points.push(first);
                }
                if matches!(piece, Piece::Solid(_)) {
                    let triangles = geom::triangulate(&points);
                    decor.fills.push((points, triangles));
                } else {
                    decor.strokes.push(points);
                }
            }
        }
    }
}

/// Puts the marks `ends` names on the open paths of `lines`, and shortens each path where its line stops under a mark. A path with no direction (every point on
/// one) is left as it is, with no mark, and so is every path past the [`MAX_LINES`]th. The pattern is not the marks' business: [`paint`] draws it.
pub(crate) fn decorate(lines: &mut Vec<Vec<P>>, ends: Ends) -> Decor {
    let mut decor = Decor::default();
    let (begin, end) = (mark(ends.begin), mark(ends.end));
    if begin.is_none() && end.is_none() {
        return decor;
    }
    let (room_begin, room_end) = (begin.as_ref().map_or(0.0, Mark::room), end.as_ref().map_or(0.0, Mark::room));
    let mut out: Vec<Vec<P>> = Vec::with_capacity(lines.len());
    for (n, line) in std::mem::take(lines).into_iter().enumerate() {
        let Some(((first, last), (toward_begin, toward_end))) = axes(&line).filter(|_| n < MAX_LINES) else {
            out.push(line);
            continue;
        };
        // One unit for the path: the most that the marks may take of it, shared by their room.
        let room = room_begin + room_end;
        let s = if room > 0.0 { UNIT.min(MARKS_SHARE * path_length(&line) / room) } else { UNIT };
        let stop = |m: &Option<Mark>| m.as_ref().map_or(0.0, |m| (m.stop - if m.solid_back() { OVERLAP } else { 0.0 }).max(0.0) * s);
        if let Some(m) = &begin {
            place(m, s, first, toward_begin, &mut decor);
        }
        if let Some(m) = &end {
            place(m, s, last, toward_end, &mut decor);
        }
        let shaft = trimmed(&line, stop(&begin), stop(&end));
        if shaft.len() >= 2 {
            out.push(shaft);
        }
    }
    *lines = out;
    decor
}

// ---- the pattern -----------------------------------------------------------------------------------------------------------------------

/// The marks and gaps of the line patterns 2 to 23, in line weights: marks and gaps alternate, and a mark of 0 is a dot.
const PATTERNS: [&[u8]; 22] = [
    &[7, 5],
    &[0, 5],
    &[7, 5, 0, 5],
    &[7, 5, 0, 5, 0, 5],
    &[7, 5, 7, 5, 0, 5],
    &[19, 5, 7, 5],
    &[19, 5, 7, 5, 7, 5],
    &[3, 3],
    &[0, 3],
    &[3, 3, 0, 3],
    &[3, 3, 0, 3, 0, 3],
    &[3, 3, 3, 3, 0, 3],
    &[9, 3, 3, 3],
    &[9, 3, 3, 3, 3, 3],
    &[15, 9],
    &[0, 9],
    &[15, 9, 0, 9],
    &[15, 9, 0, 9, 0, 9],
    &[15, 9, 15, 9, 0, 9],
    &[39, 9, 15, 9],
    &[39, 9, 15, 9, 15, 9],
    &[1, 2],
];

/// The marks and gaps of a pattern number, in line weights; `None` for a solid line (1, 0, and a number that is none of the table's).
pub(crate) fn dashes(pattern: u8) -> Option<&'static [u8]> {
    pattern.checked_sub(2).and_then(|k| PATTERNS.get(usize::from(k))).copied()
}

/// A line drawn as the pattern says, with a pen `pen` wide: a dash is as long as the table's mark times [`DASH_SCALE`] pens, and a dot is a pen long.
fn patterned(points: &[Pos2], pen: Stroke, table: &[u8]) -> Vec<Shape> {
    let unit = pen.width * DASH_SCALE;
    let length = |n: u8| if n == 0 { pen.width } else { f32::from(n) * unit };
    let marks: Vec<f32> = table.iter().step_by(2).map(|n| length(*n)).collect();
    let gaps: Vec<f32> = table.iter().skip(1).step_by(2).map(|n| f32::from(*n) * unit).collect();
    Shape::dashed_line_with_offset(points, pen, &marks, &gaps, 0.0)
}

/// A closed polygon's points with the repeated first point left off.
fn ring(points: &[P]) -> &[P] {
    match points.split_last() {
        Some((last, rest)) if points.first() == Some(last) => rest,
        _ => points,
    }
}

// ---- painting --------------------------------------------------------------------------------------------------------------------------

/// The open part of a tile's figure: its lines as the pattern says, then the solid marks filled and the other marks stroked. `at` maps a point of the figure to the tile.
/// **The closed part of a hollow mark is painted with `fill` before its outline** (the inside of a hollow mark takes the fill the drawing's shape has, and a tile's
/// shape has the theme's tile fill, `Tokens::stencil_fill`, the same that the body of a box tile has: transparent in both themes, so the panel shows through).
pub(crate) fn paint(painter: &Painter, figure: &Figure, at: &dyn Fn(&P) -> Pos2, pen: Stroke, fill: Color32) {
    for line in &figure.lines {
        let points: Vec<Pos2> = line.iter().map(at).collect();
        match dashes(figure.pattern) {
            Some(table) => painter.extend(patterned(&points, pen, table)),
            None => {
                painter.add(Shape::line(points, pen));
            }
        }
    }
    for (polygon, triangles) in &figure.fills {
        let mut mesh = Mesh::default();
        for p in polygon {
            mesh.colored_vertex(at(p), pen.color);
        }
        for t in triangles {
            let [a, b, c] = t.map(|i| u32::try_from(i).unwrap_or(0));
            mesh.add_triangle(a, b, c);
        }
        painter.add(Shape::mesh(mesh));
        // The outline is drawn as well, in the same ink: the fill's edge is not smoothed, a stroke's is.
        painter.add(Shape::closed_line(ring(polygon).iter().map(at).collect(), pen));
    }
    for mark in &figure.marks {
        let closed = mark.len() > 3 && mark.first() == mark.last();
        if closed && fill.a() > 0 {
            // Every closed mark is a star-shaped polygon from its first point (a head with a hollowed back is not convex, a disc is 16 sides), so a fan from it covers exactly the form.
            let corners: Vec<Pos2> = ring(mark).iter().map(at).collect();
            if let Some((first, rest)) = corners.split_first() {
                for pair in rest.windows(2) {
                    if let [a, b] = pair {
                        painter.add(Shape::convex_polygon(vec![*first, *a, *b], fill, Stroke::NONE));
                    }
                }
            }
        }
        painter.add(if closed {
            Shape::closed_line(ring(mark).iter().map(at).collect(), pen)
        } else {
            Shape::line(mark.iter().map(at).collect(), pen)
        });
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    /// The boxes of the numbers measured and the others: how far back from the vertex a mark runs, from where to where,
    /// and how wide it is, in units of `S`. And how far the line stops from the vertex, which is the back of the rearmost solid or hollow part.
    const BOXES: [(u8, f64, f64, f64, f64); 45] = [
        (1, 0.0, 1.0, 2.0, 0.0),
        (2, 0.0, 1.0, 2.0, 1.0),
        (3, 0.0, 2.0, 2.0, 0.0),
        (4, 0.0, 2.0, 2.0, 2.0),
        (5, 0.0, 2.0, 1.99, 1.75),
        (6, 0.0, 2.25, 2.02, 2.25),
        (7, 0.08, 1.84, 1.92, 0.0),
        (8, 0.0, 2.0, 2.0, 1.5),
        (9, -1.0, 1.0, 2.0, 0.0),
        (10, -0.75, 0.75, 1.5, 0.75),
        (11, -0.75, 0.75, 1.5, 0.75),
        (12, 0.0, 3.0, 2.0, 0.0),
        (13, 0.0, 3.0, 2.0, 3.0),
        (14, 0.0, 3.0, 2.0, 3.0),
        (15, 0.0, 1.0, 2.0, 1.0),
        (16, 0.0, 2.0, 2.0, 2.0),
        (17, 0.0, 2.0, 2.0, 1.75),
        (18, 0.0, 2.25, 2.02, 2.25),
        (19, 0.0, 2.0, 2.0, 1.5),
        (20, -0.75, 0.75, 1.5, 0.75),
        (21, -0.75, 0.75, 1.5, 0.75),
        (22, 0.0, 4.5, 2.24, 4.5),
        (23, 1.5, 2.5, 2.0, 0.0),
        (24, 1.5, 1.5, 1.5, 0.0),
        (25, 1.5, 2.25, 1.5, 0.0),
        (26, 1.5, 3.0, 1.5, 0.0),
        (27, 0.0, 1.5, 1.5, 0.0),
        (28, 0.0, 2.0, 1.5, 0.0),
        (29, 0.0, 3.0, 1.5, 3.0),
        (30, 0.0, 3.0, 1.5, 3.0),
        (31, 0.0, 2.25, 1.5, 1.5),
        (32, 0.0, 3.0, 1.5, 1.5),
        (33, 0.0, 3.75, 1.5, 1.5),
        (34, 0.0, 6.0, 2.24, 6.0),
        (35, 0.0, 2.25, 1.5, 1.5),
        (36, 0.0, 3.0, 1.5, 1.5),
        (37, 0.0, 3.75, 1.5, 1.5),
        (38, 0.0, 6.0, 2.24, 6.0),
        (39, 0.0, 4.0, 2.0, 4.0),
        (40, 0.0, 4.0, 2.0, 4.0),
        (41, 0.0, 1.5, 1.5, 1.5),
        (42, -0.75, 0.75, 1.5, 0.75),
        (43, 0.0, 4.0, 2.0, 0.0),
        (44, 0.0, 2.75, 2.0, 0.0),
        (45, 0.0, 4.75, 2.0, 0.0),
    ];

    #[test]
    fn each_of_the_45_marks_has_the_box_the_specification_gives_it() {
        for (number, from, to, spread, stop) in BOXES {
            let m = mark(number).unwrap_or_else(|| panic!("no mark for {number}"));
            let points: Vec<(f64, f64)> = m.pieces.iter().flat_map(|p| p.points().iter().copied()).collect();
            let (lo, hi) = (points.iter().map(|p| p.0).fold(f64::MAX, f64::min), points.iter().map(|p| p.0).fold(f64::MIN, f64::max));
            let across = 2.0 * points.iter().map(|p| p.1.abs()).fold(0.0, f64::max);
            let near = |a: f64, b: f64| (a - b).abs() < 0.03;
            assert!(near(lo, from) && near(hi, to), "{number}: runs from {lo} to {hi}, the box says {from} to {to}");
            assert!(near(across, spread), "{number}: {across} across, the box says {spread}");
            assert!(near(m.stop, stop), "{number}: the line stops at {}, the box says {stop}", m.stop);
        }
    }

    #[test]
    fn a_number_that_is_no_mark_has_none_and_a_custom_end_is_drawn_as_number_4() {
        for number in [0, 46, 100, 253, 255] {
            assert_eq!(mark(number), None, "{number}");
        }
        assert_eq!(mark(254), mark(4), "a master used as the end: the plain end, as a page draws it");
    }

    #[test]
    fn solid_hollow_and_open_marks_are_told_apart_by_what_the_specification_says() {
        let kind = |n: u8| {
            mark(n)
                .unwrap()
                .pieces
                .iter()
                .map(|p| matches!(p, Piece::Solid(_)) as u8 * 4 + matches!(p, Piece::Hollow(_)) as u8 * 2 + matches!(p, Piece::Open(_)) as u8)
                .collect::<Vec<_>>()
        };
        assert_eq!(kind(4), [4], "solid");
        assert_eq!(kind(16), [2], "hollow");
        assert_eq!(kind(3), [1], "open");
        assert_eq!(kind(29), [1, 2], "a fork and a hollow disc");
        assert_eq!(kind(38), [4, 2], "a solid disc and a hollow diamond");
        assert!(mark(4).unwrap().solid_back() && !mark(16).unwrap().solid_back() && !mark(3).unwrap().solid_back());
    }

    #[test]
    fn the_pattern_table_gives_marks_and_gaps_and_a_solid_line_none() {
        assert_eq!(dashes(1), None);
        assert_eq!(dashes(0), None, "a line of pattern 0 is drawn solid on a tile");
        assert_eq!(dashes(2), Some(&[7_u8, 5][..]));
        assert_eq!(dashes(3), Some(&[0_u8, 5][..]));
        assert_eq!(dashes(4), Some(&[7_u8, 5, 0, 5][..]));
        assert_eq!(dashes(23), Some(&[1_u8, 2][..]));
        assert_eq!(dashes(24), None);
        assert_eq!(dashes(254), None);
        assert!((2..=23).all(|p| dashes(p).is_some_and(|t| t.len() % 2 == 0 && t.len() >= 2)), "marks and gaps in pairs");
    }

    #[test]
    fn ends_are_read_from_a_list_and_nothing_in_it_is_trusted() {
        use serde_json::json;
        assert_eq!(Ends::read(&json!({"begin": 22, "end": 4, "pattern": 2})), Some(Ends { begin: 22, end: 4, pattern: 2 }));
        assert_eq!(Ends::read(&json!({"begin": 254, "end": 0, "pattern": 1})), Some(Ends { begin: 254, end: 0, pattern: 1 }));
        assert_eq!(
            Ends::read(&json!({"begin": 300, "end": -1, "pattern": 99})),
            Some(Ends::PLAIN),
            "numbers that are none are no mark and a solid line"
        );
        assert_eq!(Ends::read(&json!({"begin": "x", "end": null})), Some(Ends::PLAIN));
        assert_eq!(Ends::read(&json!({})), Some(Ends::PLAIN));
        assert_eq!(Ends::read(&json!(7)), None);
        assert_eq!(Ends::read(&json!("ends")), None);
    }

    fn line(points: &[P]) -> Vec<Vec<P>> {
        vec![points.to_vec()]
    }

    fn near(a: P, b: P) -> bool {
        length(sub(a, b)) < 1.0e-9
    }

    #[test]
    fn a_mark_has_its_tip_on_the_vertex_and_lies_back_along_the_line() {
        let mut lines = line(&[(0.0, 0.0), (1.0, 0.0)]);
        let decor = decorate(&mut lines, Ends { begin: 0, end: 4, pattern: 1 });
        // The triangle: tip on the end, the back 2 units behind it, 1 unit to each side.
        let [(triangle, _)] = &decor.fills[..] else { panic!("one solid mark: {decor:?}") };
        assert!(near(triangle[0], (1.0, 0.0)), "the tip is on the vertex: {triangle:?}");
        assert!(near(triangle[1], (1.0 - 2.0 * UNIT, UNIT)) || near(triangle[1], (1.0 - 2.0 * UNIT, -UNIT)), "{triangle:?}");
        assert_eq!(triangle.first(), triangle.last(), "closed, the first point repeated");
        // The line stops 0.1 unit inside the back of a solid mark, and starts where it did.
        let [shaft] = &lines[..] else { panic!("one line") };
        assert!(near(shaft[0], (0.0, 0.0)) && near(*shaft.last().unwrap(), (1.0 - (2.0 - OVERLAP) * UNIT, 0.0)), "{shaft:?}");
    }

    #[test]
    fn the_begin_mark_lies_along_the_first_segment_pointing_away_from_the_line() {
        let mut lines = line(&[(0.0, 0.0), (0.0, 1.0)]);
        let decor = decorate(&mut lines, Ends { begin: 16, end: 0, pattern: 1 });
        // A hollow triangle on a line that runs up: its tip on the begin, its back above it.
        let [mark] = &decor.strokes[..] else { panic!("one hollow mark: {decor:?}") };
        assert!(near(mark[0], (0.0, 0.0)), "{mark:?}");
        assert!(mark.iter().all(|p| p.1 >= -1.0e-9 && p.1 <= 2.0 * UNIT + 1.0e-9), "its body lies back along the line, which runs up: {mark:?}");
        // The line stops at the back of a hollow mark, exactly.
        let [shaft] = &lines[..] else { panic!("one line") };
        assert!(near(shaft[0], (0.0, 2.0 * UNIT)) && near(*shaft.last().unwrap(), (0.0, 1.0)), "{shaft:?}");
    }

    #[test]
    fn the_end_mark_of_a_zigzag_turns_along_its_last_segment() {
        // A line that runs right and then back left and down: the last segment points down and to the left.
        let mut lines = line(&[(0.0, 0.0), (0.6, 0.2), (0.2, -0.2)]);
        let decor = decorate(&mut lines, Ends { begin: 0, end: 3, pattern: 1 });
        let [head] = &decor.strokes[..] else { panic!("one open mark: {decor:?}") };
        let (tip, middle, wing) = (head[1], head[0], head[2]);
        assert!(near(tip, (0.2, -0.2)), "the tip of an open head is the vertex: {head:?}");
        // The head's body lies back along the last segment, toward (0.6, 0.2): the middle of its two wings is up and to the right of the tip, on that line.
        let centre = ((middle.0 + wing.0) / 2.0, (middle.1 + wing.1) / 2.0);
        let way = sub(centre, tip);
        let (dx, dy) = (0.4_f64, 0.4_f64);
        let along = (dx / dx.hypot(dy), dy / dx.hypot(dy));
        assert!((way.0 / length(way) - along.0).abs() < 1.0e-9 && (way.1 / length(way) - along.1).abs() < 1.0e-9, "{head:?}");
        assert!((length(way) - 2.0 * UNIT).abs() < 1.0e-9, "an open head of number 3 reaches 2 units back: {}", length(way));
        // An open mark leaves the line running on to the vertex.
        assert!(near(*lines[0].last().unwrap(), (0.2, -0.2)), "{:?}", lines[0]);
    }

    #[test]
    fn two_long_marks_on_a_short_line_are_made_smaller_to_leave_a_shaft() {
        let mut lines = line(&[(0.0, 0.0), (1.0, 0.0)]);
        let decor = decorate(&mut lines, Ends { begin: 38, end: 34, pattern: 1 });
        let all: Vec<&Vec<P>> = decor.fills.iter().map(|f| &f.0).chain(decor.strokes.iter()).collect();
        assert!(all.iter().all(|m| m.iter().all(|p| (-0.01..=1.01).contains(&p.0))), "the marks stay on the line: {all:?}");
        let [shaft] = &lines[..] else { panic!("a shaft is left between the marks: {lines:?}") };
        assert!(path_length(shaft) >= 0.3, "at least a third of the line is left: {}", path_length(shaft));
    }

    #[test]
    fn a_line_with_no_direction_or_no_room_has_no_marks_and_a_hostile_one_costs_little() {
        // Every point on one: no direction.
        let mut lines = line(&[(0.5, 0.5), (0.5, 0.5)]);
        assert_eq!(decorate(&mut lines, Ends { begin: 4, end: 4, pattern: 1 }), Decor::default());
        assert_eq!(lines, line(&[(0.5, 0.5), (0.5, 0.5)]));
        // A number that is no mark: the lines are not touched.
        let mut lines = line(&[(0.0, 0.0), (1.0, 0.0)]);
        assert_eq!(decorate(&mut lines, Ends { begin: 200, end: 99, pattern: 1 }), Decor::default());
        assert_eq!(lines, line(&[(0.0, 0.0), (1.0, 0.0)]));
        // A point that is not a number has no direction either: the path is left as it is.
        for points in [[(f64::NAN, 0.0), (1.0, 0.0)], [(0.0, 0.0), (f64::INFINITY, 0.0)], [(0.0, 0.0), (1.0, f64::NAN)]] {
            let mut lines = line(&points);
            assert_eq!(decorate(&mut lines, Ends { begin: 4, end: 4, pattern: 1 }), Decor::default(), "{points:?}");
            assert_eq!(lines.len(), 1, "{points:?}");
        }
        // Many paths: only the first MAX_LINES get marks.
        let mut lines: Vec<Vec<P>> = (0..1_000).map(|k| vec![(0.0, f64::from(k)), (1.0, f64::from(k))]).collect();
        let decor = decorate(&mut lines, Ends { begin: 4, end: 4, pattern: 1 });
        assert_eq!(decor.fills.len(), 2 * MAX_LINES, "a solid mark at each end of each of the first paths");
        assert_eq!(lines.len(), 1_000, "no path is lost");
    }

    /// Every shape a [`paint`] of a one-line figure ending in `number` made, with `fill` for the closed part of a hollow mark: the order they were added in.
    fn painted(number: u8, fill: Color32) -> Vec<Shape> {
        let figure = crate::stencils::tile_figure(
            ((0.0, 0.0), (1.0, 0.2)),
            &[vec![(0.0, 0.1), (1.0, 0.1)]],
            Some(Ends { begin: 0, end: number, pattern: 1 }),
            false,
        )
        .expect("a figure");
        let at = |p: &P| egui::pos2(10.0 + (p.0 * 200.0) as f32, 100.0 - (p.1 * 200.0) as f32);
        let pen = Stroke::new(1.0, crate::theme::Tokens::light().stencil_ink);
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| paint(ui.painter(), &figure, &at, pen, fill));
        out.textures_delta.clear();
        out.shapes.into_iter().map(|clipped| clipped.shape).collect()
    }

    /// How many shapes are a filled polygon of this colour.
    fn fills_of(shapes: &[Shape], colour: Color32) -> usize {
        shapes.iter().filter(|s| matches!(s, Shape::Path(p) if p.fill == colour)).count()
    }

    #[test]
    fn the_closed_part_of_a_hollow_mark_takes_the_tiles_fill_under_its_outline_and_nothing_else_does() {
        // A hollow mark's inside is painted with the tile's fill (`Tokens::stencil_fill`), which is transparent in both themes and so shows the panel; given a colour, it
        // shows. A square (21) is four corners, a fan of two triangles; the disc and the diamond of 34 are a fan of 14 and one of 2.
        let (green, clear) = (crate::theme::Tokens::light().accent, crate::theme::Tokens::light().stencil_fill);
        assert!(green.a() > 0 && clear.a() == 0 && green != crate::theme::Tokens::light().stencil_ink, "the tokens the test relies on");
        for (number, triangles) in [(21, 2), (20, 14), (16, 1), (22, 2), (34, 16), (40, 2)] {
            let shapes = painted(number, green);
            assert_eq!(fills_of(&shapes, green), triangles, "number {number}: the closed part is a fan of triangles");
            // Under the outline: the first fill comes before the first closed outline (a path with no fill, closed), and each mark's own fan before its own outline.
            let first_outline = shapes.iter().position(|s| matches!(s, Shape::Path(p) if p.fill == clear && p.closed)).expect("the outline");
            let first_fill = shapes.iter().position(|s| matches!(s, Shape::Path(p) if p.fill == green)).expect("a fill");
            assert!(first_fill < first_outline, "number {number}: the fill is painted before the outline");
        }
        // Solid marks are ink, open marks have no closed part: the fill colour is not used. Transparent: nothing is added.
        for number in [4, 13, 10, 11, 3, 1, 12, 24, 43] {
            assert_eq!(fills_of(&painted(number, green), green), 0, "number {number} has no hollow part");
        }
        for (number, fan) in [(21, 2), (34, 16), (4, 0), (3, 0)] {
            assert_eq!(painted(number, clear).len() + fan, painted(number, green).len(), "number {number}: a transparent fill adds nothing");
        }
    }
}
