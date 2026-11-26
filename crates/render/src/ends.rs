//! Line ends.
//!
//! **The marks are ours.** The specification gives each number's kind, its size class and a box (how far back from the vertex it reaches,
//! how wide it spreads, where the shaft stops), not an outline (and the asset rules of `AGENTS.md`). The box is measured for 32 of
//! the 45 numbers and set by DiaCraft's own rule for the other 13 (a head of a kind no number measures takes 4's box,
//! so 8 does and 19 takes 8's; a disc takes 1.5 along; a crossbar is 1.5 long leading a mark and 0.75 behind a part; a group's reach is
//! the sum of its parts). Each mark is drawn here from plain pieces (a triangle, an open head, a disc, a bar) as shapes
//! it, in those boxes, so the canvas and a saved `.odg` draw the same shape in the same box.
//!
//! * **Where.** The tip is on the end vertex and the body lies back along the path; a disc or a square sits centred on the vertex.
//!   Nothing is drawn past the tip.
//! * **Size.** A mark's unit `S` is a base for its size class plus the line's weight, on paper and not scaled by the drawing.
//!   Class 6 has a base of 18 points, twice class 5's, and a number outside 0 to 6 is drawn as class 2.
//! * **The shaft** stops short of a solid or hollow mark, so that nothing shows through or past its back: 0.1 `S` inside the back of an opaque
//!   solid mark, exactly at the back otherwise. The back is the rearmost solid or hollow part's. An open mark leaves the
//!   stroke running on to the vertex.
//! * **A custom end** (254) is a master the page names by `USE` (`custom_end.rs`). Here it is a [`CustomEnd`], its drawing already
//!   sized and with its pin on the origin and its x axis along the way it points: [`plan`] puts the pin on the end vertex, turns the drawing to point out of the line there (or leaves it
//!   upright), and for a turning end cuts the shaft where the drawing's back is. Any numbered end on the same path is as it was.
//! * **Which paths.** Each open path of the stroke has its two ends; a closed path has none. A vertex that repeats at
//!   an end (a `LineTo` onto the point the pen is on) is still the end: the mark sits on it, turned along the nearest segment that has a
//!   direction. A path with no such segment has none to turn by, gets no marks, and the frame says so.

use kurbo::{BezPath, ParamCurve, ParamCurveArclen, PathEl, PathSeg, Point, Vec2};

/// Base of each size class in paper inches: 1.44, 1.8, 2.52, 3.24, 3.96 and 9 points.
const SIZE_BASE: [f64; 6] = [0.020, 0.025, 0.035, 0.045, 0.055, 0.125];

/// Base of class 6: 18 points, twice class 5's. No source measures a class 6; the rule is DiaCraft's own.
const COLOSSAL_BASE: f64 = 0.25;

/// How far inside the back of an opaque solid mark the shaft stops, in units of `S`.
const SEAM_OVERLAP: f64 = 0.1;

/// How a closed form is painted: filled in the line's colour, or an outline of one line weight with nothing inside.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Paint {
    Solid,
    Ring,
}

/// A closed form, in the mark's frame: x runs back along the line from the tip, y across it, both in units of `S`.
#[derive(Clone, Debug, PartialEq)]
enum Form {
    /// A closed polygon.
    Poly(Vec<(f64, f64)>),
    /// A disc with its centre `x` back from the tip and this radius.
    Disc(f64, f64),
    /// A square with its centre `x` back from the tip and this half side.
    Square(f64, f64),
}

impl Form {
    /// How far back from the tip the form reaches.
    fn back(&self) -> f64 {
        match self {
            Form::Poly(points) => points.iter().map(|p| p.0).fold(f64::MIN, f64::max),
            Form::Disc(x, r) | Form::Square(x, r) => x + r,
        }
    }
}

/// A piece of a mark.
#[derive(Clone, Debug, PartialEq)]
enum Piece {
    /// A closed form, painted solid (filled) or as a ring (outlined).
    Closed(Form, Paint),
    /// Open strokes.
    Open(Vec<(f64, f64)>),
}

/// One line end's art.
#[derive(Clone, Debug, PartialEq)]
struct Art {
    pieces: Vec<Piece>,
    /// How far from the vertex the shaft stops, in `S`; 0 for an open mark, whose stroke runs on to the vertex.
    stop: f64,
}

impl Art {
    /// Whether the rearmost closed part, whose back the shaft stops at, is solid: an opaque line then overlaps it.
    fn solid_back(&self) -> bool {
        let mut rearmost: Option<(f64, Paint)> = None;
        for piece in &self.pieces {
            if let Piece::Closed(form, paint) = piece {
                let back = form.back();
                if rearmost.is_none_or(|(farthest, _)| back > farthest) {
                    rearmost = Some((back, *paint));
                }
            }
        }
        rearmost.is_some_and(|(_, paint)| paint == Paint::Solid)
    }
}

fn solid(form: Form) -> Piece {
    Piece::Closed(form, Paint::Solid)
}

fn ring(form: Form) -> Piece {
    Piece::Closed(form, Paint::Ring)
}

fn triangle(len: f64) -> Form {
    Form::Poly(vec![(0.0, 0.0), (len, 1.0), (len, -1.0)])
}

fn open_head(from: f64, len: f64) -> Piece {
    Piece::Open(vec![(from + len, 1.0), (from, 0.0), (from + len, -1.0)])
}

fn bar(x: f64) -> Piece {
    Piece::Open(vec![(x, 0.75), (x, -0.75)])
}

/// The shaft run along the axis from the vertex to `to`, where the line of a mark must show between its parts (the `spine`).
fn spine(to: f64) -> Piece {
    Piece::Open(vec![(0.0, 0.0), (to, 0.0)])
}

fn fork() -> Piece {
    Piece::Open(vec![(0.0, 0.75), (1.5, 0.0), (0.0, -0.75)])
}

/// A head 2 back and `half` across at the back corners, with the back cut in to `apex` (a hollowed back when `apex` < 2, the `head(2, half, apex)`).
fn head(half: f64, apex: f64) -> Form {
    Form::Poly(vec![(0.0, 0.0), (2.0, half), (apex, 0.0), (2.0, -half)])
}

fn diamond(from: f64) -> Form {
    Form::Poly(vec![(from, 0.0), (from + 2.25, 1.12), (from + 4.5, 0.0), (from + 2.25, -1.12)])
}

fn disc(x: f64) -> Form {
    Form::Disc(x, 0.75)
}

/// The art of a number: the pieces in the boxes.
fn art(number: i64) -> Option<Art> {
    let make = |pieces: Vec<Piece>, stop: f64| Some(Art { pieces, stop });
    // A disc at the tip, solid or hollow, with these crossbars behind it (31 to 33 hollow, 35 to 37 solid).
    let disc_and_bars = |number: i64, bars: &[f64]| {
        let first = if number < 35 { ring(disc(0.75)) } else { solid(disc(0.75)) };
        make(std::iter::once(first).chain(bars.iter().map(|x| bar(*x))).collect(), 1.5)
    };
    match number {
        1 => make(vec![open_head(0.0, 1.0)], 0.0),
        2 => make(vec![solid(triangle(1.0))], 1.0),
        3 => make(vec![open_head(0.0, 2.0)], 0.0),
        4 => make(vec![solid(triangle(2.0))], 2.0),
        5 => make(vec![solid(head(0.995, 1.75))], 1.75),
        6 => make(vec![solid(head(1.01, 2.25))], 2.25),
        7 => make(vec![Piece::Open(vec![(1.84, 0.96), (0.5, 0.48), (0.08, 0.0), (0.5, -0.48), (1.84, -0.96)])], 0.0),
        8 => make(vec![solid(head(1.0, 1.5))], 1.5),
        9 => make(vec![Piece::Open(vec![(-1.0, 1.0), (1.0, -1.0)])], 0.0),
        10 | 42 => make(vec![solid(disc(0.0))], 0.75),
        11 => make(vec![solid(Form::Square(0.0, 0.75))], 0.75),
        12 => make(vec![open_head(0.0, 3.0)], 0.0),
        13 => make(vec![solid(triangle(3.0))], 3.0),
        14 => make(vec![ring(triangle(3.0))], 3.0),
        15 => make(vec![ring(triangle(1.0))], 1.0),
        16 => make(vec![ring(triangle(2.0))], 2.0),
        17 => make(vec![ring(head(1.0, 1.75))], 1.75),
        18 => make(vec![ring(head(1.01, 2.25))], 2.25),
        19 => make(vec![ring(head(1.0, 1.5))], 1.5),
        20 => make(vec![ring(disc(0.0))], 0.75),
        21 => make(vec![ring(Form::Square(0.0, 0.75))], 0.75),
        22 => make(vec![ring(diamond(0.0))], 4.5),
        23 => make(vec![Piece::Open(vec![(1.5, -1.0), (2.5, 1.0)])], 0.0),
        24 => make(vec![bar(1.5)], 0.0),
        25 => make(vec![bar(1.5), bar(2.25)], 0.0),
        26 => make(vec![bar(1.5), bar(2.25), bar(3.0)], 0.0),
        27 => make(vec![fork()], 0.0),
        28 => make(vec![fork(), bar(2.0)], 0.0),
        29 => make(vec![fork(), ring(disc(2.25))], 3.0),
        30 => make(vec![bar(0.75), spine(1.5), ring(disc(2.25))], 3.0),
        31 | 35 => disc_and_bars(number, &[2.25]),
        32 | 36 => disc_and_bars(number, &[2.25, 3.0]),
        33 | 37 => disc_and_bars(number, &[2.25, 3.0, 3.75]),
        34 => make(vec![ring(disc(0.75)), ring(diamond(1.5))], 6.0),
        38 => make(vec![solid(disc(0.75)), ring(diamond(1.5))], 6.0),
        39 | 40 => {
            let (first, second) = (triangle(2.0), Form::Poly(vec![(2.0, 0.0), (4.0, 1.0), (4.0, -1.0)]));
            let paint = |form: Form| if number == 40 { ring(form) } else { solid(form) };
            make(vec![paint(first), paint(second)], 4.0)
        }
        41 => make(vec![ring(disc(0.75))], 1.5),
        43 => make(vec![open_head(0.0, 2.0), open_head(2.0, 2.0)], 0.0),
        44 => make(vec![open_head(0.0, 2.0), bar(2.75)], 0.0),
        45 => make(vec![open_head(0.0, 2.0), open_head(2.0, 2.0), bar(4.75)], 0.0),
        _ => None,
    }
}

/// Whether a line end's number is one whose box no source measures (the dashes): 8, 18, 19, 31, 32, 34 to 38, 40, 44 and 45. It is drawn
/// in the box DiaCraft's own rule gives it.
pub(crate) fn is_unmeasured(number: i64) -> bool {
    matches!(number, 8 | 18 | 19 | 31 | 32 | 34..=38 | 40 | 44 | 45)
}

/// Whether a mark is drawn for a number (1 to 45): the art there is, so the same table says what is drawn and what the frame reports on.
pub(crate) fn is_drawn(number: i64) -> bool {
    art(number).is_some()
}

/// Whether a size class is outside 0 to 6, so that it is drawn as class 2.
pub(crate) fn size_is_standin(size: i64) -> bool {
    !(0..=6).contains(&size)
}

/// The unit `S` of a size class and a weight, in paper inches. A class outside 0 to 6 reads as 2.
fn unit(size: i64, weight: f64) -> f64 {
    let base = match size {
        0..=5 => SIZE_BASE.get(usize::try_from(size).unwrap_or(2)).copied().unwrap_or(SIZE_BASE[2]),
        6 => COLOSSAL_BASE,
        _ => SIZE_BASE[2],
    };
    base + weight.max(0.0)
}

/// The share of the line's weight that a custom end's alignment box is tall at size class `size` (the project rule for the classes other than Medium): the unit `S` at that
/// class over the unit at Medium (class 2), both at `weight`, so it is 1 at Medium. A class outside 0 to 6 is read as 2.
pub(crate) fn box_share(size: i64, weight: f64) -> f64 {
    let medium = unit(2, weight);
    if medium > 0.0 { unit(size, weight) / medium } else { 1.0 }
}

/// A custom end, drawn and sized: the master's drawing in paper inches with **its pin on the origin and its x axis along the way the end points**, so that it is put
/// on a line's end by one transform. The sections that paint a fill are one ring to be filled by the odd-crossings rule, the sections that paint an outline are strokes of the line's weight.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CustomEnd {
    pub fill: BezPath,
    pub strokes: BezPath,
    /// The end keeps the page's orientation (`PatternFlags` 256) and does not trim the line.
    pub upright: bool,
    /// How far behind the pin the shaft is cut, in paper inches: where the drawing's back is for an end that turns with the line, and 0 for an upright one.
    pub trim: f64,
}

/// What the line ends add to a stroke.
pub(crate) struct Ends {
    /// The stroke path with each shaft shortened.
    pub shaft: BezPath,
    /// The solid pieces, to be filled.
    pub fill: BezPath,
    /// The strokes: open pieces and the outlines of hollow ones, to be stroked at the line weight.
    pub strokes: BezPath,
    /// The sections of a custom end that are filled, to be filled by the odd-crossings rule as a shape's fill is (a hole in a section stays a hole).
    pub fill_even_odd: BezPath,
}

/// What [`plan`] made of a stroke's path.
pub(crate) struct Plan {
    /// The stroke with its marks, or `None` when no path got a mark and the stroke is drawn as it was.
    pub ends: Option<Ends>,
    /// How many open paths had no direction anywhere (every segment of no length), so that no mark could be turned to face one. Their
    /// stroke is unchanged and they got no marks.
    pub directionless: usize,
}

/// The two ends of the line in `begin`/`end` numbers and size classes.
pub(crate) struct Request {
    pub begin: i64,
    pub end: i64,
    pub begin_size: i64,
    pub end_size: i64,
    /// The line's weight in paper inches.
    pub weight: f64,
    /// Whether the line is see-through (the shaft stops exactly at the back then).
    pub see_through: bool,
    /// A custom end drawn in place of the number at the begin or the end (`begin` or `end` is then 0).
    pub begin_custom: Option<CustomEnd>,
    pub end_custom: Option<CustomEnd>,
}

/// Plans the line ends of `path`, which is in paper inches.
pub(crate) fn plan(path: &BezPath, request: &Request) -> Plan {
    let (end_art, begin_art) = (art(request.end), art(request.begin));
    let (end_custom, begin_custom) = (request.end_custom.as_ref(), request.begin_custom.as_ref());
    if end_art.is_none() && begin_art.is_none() && end_custom.is_none() && begin_custom.is_none() {
        return Plan { ends: None, directionless: 0 };
    }
    let mut out = Ends { shaft: BezPath::new(), fill: BezPath::new(), strokes: BezPath::new(), fill_even_odd: BezPath::new() };
    let (mut any, mut directionless) = (false, 0usize);
    for sub in subpaths(path) {
        let open = match classify(&sub) {
            Course::Open(open) => open,
            Course::Closed => {
                // A closed path has no ends: its stroke is as it was.
                push_segments(&mut out.shaft, &sub);
                continue;
            }
            Course::Directionless => {
                // Puts a mark on the first and the last vertex, turned along the path; with no segment of any length there is no
                // along. The stroke is as it was, and the frame is told.
                directionless = directionless.saturating_add(1);
                push_segments(&mut out.shaft, &sub);
                continue;
            }
        };
        let mut segs = sub;
        // The art for each end; an end is trimmed by the distance its mark's shaft stops at.
        if let Some(custom) = end_custom {
            any = true;
            place_custom(custom, open.end, open.end_back, &mut out);
            segs = trim_end(segs, custom.trim);
        } else if let Some(art) = &end_art {
            let s = unit(request.end_size, request.weight);
            let stop = stop_distance(art, s, request);
            any = true;
            place(art, s, open.end, open.end_back, &mut out);
            segs = trim_end(segs, stop);
        }
        if let Some(custom) = begin_custom {
            any = true;
            place_custom(custom, open.begin, open.begin_back, &mut out);
            segs = trim_start(segs, custom.trim);
        } else if let Some(art) = &begin_art {
            let s = unit(request.begin_size, request.weight);
            let stop = stop_distance(art, s, request);
            any = true;
            place(art, s, open.begin, open.begin_back, &mut out);
            segs = trim_start(segs, stop);
        }
        push_segments(&mut out.shaft, &segs);
    }
    Plan { ends: any.then_some(out), directionless }
}

/// How far from its vertex the shaft is cut, in paper inches.
fn stop_distance(art: &Art, s: f64, request: &Request) -> f64 {
    if art.stop <= 0.0 {
        return 0.0;
    }
    // A mark whose rearmost part is hollow and a see-through line stop exactly at the back; an opaque solid mark is overlapped by a tenth of
    // `S`.
    let overlap = if art.solid_back() && !request.see_through { SEAM_OVERLAP } else { 0.0 };
    ((art.stop - overlap) * s).max(0.0)
}

/// The two ends of an open path: the vertex and the direction pointing back along the path from it.
struct OpenEnds {
    begin: Point,
    begin_back: Vec2,
    end: Point,
    end_back: Vec2,
}

/// What kind of path a subpath is, for its line ends.
enum Course {
    /// It starts and ends at different points, and has a direction at both: the marks go on its two ends.
    Open(OpenEnds),
    /// It comes back to where it began: no marks.
    Closed,
    /// Every segment has no length, so there is no direction anywhere to turn a mark by.
    Directionless,
}

/// The two ends of an open path, found by walking in from each end past the segments that have no direction (a `LineTo` onto the point the
/// pen is on, a repeated vertex): puts a mark on the first and the last vertex, and takes them off only a closed path, so a
/// repeated end vertex changes neither. The tip stays on the vertex itself; only the direction is read from the nearest real segment.
fn classify(segs: &[PathSeg]) -> Course {
    let (Some(first), Some(last)) = (segs.first(), segs.last()) else {
        return Course::Directionless;
    };
    let (begin, end) = (first.start(), last.end());
    let forward_at_start = segs.iter().find_map(|s| tangent(s, true));
    let forward_at_end = segs.iter().rev().find_map(|s| tangent(s, false));
    let (Some(forward_at_start), Some(forward_at_end)) = (forward_at_start, forward_at_end) else {
        return Course::Directionless;
    };
    if begin.distance(end) < 1.0e-7 {
        return Course::Closed;
    }
    Course::Open(OpenEnds { begin, begin_back: forward_at_start, end, end_back: -forward_at_end })
}

/// The shortest vector that has a direction: a segment whose every control point lies within this of its start has none.
const NO_LENGTH: f64 = 1.0e-12;

// How many segments `tangent` was asked about, for the test that holds the walk in from the ends of a path to a count. Not kept outside tests.
#[cfg(test)]
thread_local! {
    static TANGENTS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// The unit direction of travel at the start (or the end) of a segment; `None` when the segment has no length, which no direction can
/// be read from.
fn tangent(seg: &PathSeg, at_start: bool) -> Option<Vec2> {
    #[cfg(test)]
    TANGENTS.with(|c| c.set(c.get() + 1));
    // The first of these that is not too short: the handle, then the chord to the farther control point, then the chord of the whole segment.
    let candidates: [Vec2; 3] = match (seg, at_start) {
        (PathSeg::Line(l), _) => [l.p1 - l.p0, Vec2::ZERO, Vec2::ZERO],
        (PathSeg::Quad(q), true) => [q.p1 - q.p0, q.p2 - q.p0, Vec2::ZERO],
        (PathSeg::Quad(q), false) => [q.p2 - q.p1, q.p2 - q.p0, Vec2::ZERO],
        (PathSeg::Cubic(c), true) => [c.p1 - c.p0, c.p2 - c.p0, c.p3 - c.p0],
        (PathSeg::Cubic(c), false) => [c.p3 - c.p2, c.p3 - c.p1, c.p3 - c.p0],
    };
    candidates.into_iter().find(|v| v.hypot() > NO_LENGTH).map(|v| v / v.hypot())
}

/// The segments of each subpath of `path` (a `MoveTo` starts one; a `ClosePath` adds the closing line).
fn subpaths(path: &BezPath) -> Vec<Vec<PathSeg>> {
    let mut out: Vec<Vec<PathSeg>> = Vec::new();
    let mut cur: Vec<PathSeg> = Vec::new();
    let (mut start, mut pen) = (Point::ZERO, Point::ZERO);
    for el in path.elements() {
        match *el {
            PathEl::MoveTo(p) => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
                (start, pen) = (p, p);
            }
            PathEl::LineTo(p) => {
                cur.push(PathSeg::Line(kurbo::Line::new(pen, p)));
                pen = p;
            }
            PathEl::QuadTo(c, p) => {
                cur.push(PathSeg::Quad(kurbo::QuadBez::new(pen, c, p)));
                pen = p;
            }
            PathEl::CurveTo(c1, c2, p) => {
                cur.push(PathSeg::Cubic(kurbo::CubicBez::new(pen, c1, c2, p)));
                pen = p;
            }
            PathEl::ClosePath => {
                if pen.distance(start) > 1.0e-12 {
                    cur.push(PathSeg::Line(kurbo::Line::new(pen, start)));
                }
                pen = start;
            }
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Adds `segs`, one connected subpath, to `path`.
fn push_segments(path: &mut BezPath, segs: &[PathSeg]) {
    let mut first = true;
    for seg in segs {
        if first {
            path.move_to(seg.start());
            first = false;
        }
        match seg {
            PathSeg::Line(l) => path.line_to(l.p1),
            PathSeg::Quad(q) => path.quad_to(q.p1, q.p2),
            PathSeg::Cubic(c) => path.curve_to(c.p1, c.p2, c.p3),
        }
    }
}

/// `segs` with `distance` cut off its end (nothing is left when the cut passes the other end).
fn trim_end(mut segs: Vec<PathSeg>, mut distance: f64) -> Vec<PathSeg> {
    while distance > 0.0 {
        let Some(last) = segs.pop() else {
            break;
        };
        let length = last.arclen(1.0e-6);
        if length <= distance {
            distance -= length;
            continue;
        }
        let t = last.inv_arclen(length - distance, 1.0e-6);
        segs.push(last.subsegment(0.0..t));
        break;
    }
    segs
}

/// `segs` with `distance` cut off its start.
fn trim_start(segs: Vec<PathSeg>, mut distance: f64) -> Vec<PathSeg> {
    let mut iter = segs.into_iter();
    let mut out = Vec::new();
    for seg in iter.by_ref() {
        if distance <= 0.0 {
            out.push(seg);
            break;
        }
        let length = seg.arclen(1.0e-6);
        if length <= distance {
            distance -= length;
            continue;
        }
        let t = seg.inv_arclen(distance, 1.0e-6);
        out.push(seg.subsegment(t..1.0));
        break;
    }
    out.extend(iter);
    out
}

/// Puts a custom end on the vertex `tip` of a path whose unit direction pointing back along the path from it is `back`: the pin on the vertex and the drawing pointing
/// **out of the line**, away from its other vertices, at the begin as at the end; an upright end keeps the page's orientation, pointing right.
fn place_custom(custom: &CustomEnd, tip: Point, back: Vec2, out: &mut Ends) {
    // The drawing's x axis is the way it points, and its y axis that turned a quarter turn counterclockwise, so that the drawing is turned and never mirrored.
    let ex = if custom.upright { Vec2::new(1.0, 0.0) } else { -back };
    let ey = Vec2::new(-ex.y, ex.x);
    let place = kurbo::Affine::new([ex.x, ex.y, ey.x, ey.y, tip.x, tip.y]);
    out.fill_even_odd.extend((place * custom.fill.clone()).elements().iter().copied());
    out.strokes.extend((place * custom.strokes.clone()).elements().iter().copied());
}

/// Draws `art` at `tip`, scaled by `s`, with its x axis along `back` (the unit direction pointing back along the path).
fn place(art: &Art, s: f64, tip: Point, back: Vec2, out: &mut Ends) {
    // The mark's x axis is `back` and its y axis is `back` turned a quarter turn counterclockwise.
    let (ex, ey) = (back, Vec2::new(-back.y, back.x));
    let map = |(x, y): (f64, f64)| tip + ex * (x * s) + ey * (y * s);
    for piece in &art.pieces {
        match piece {
            Piece::Open(points) => {
                let mut pts = points.iter().map(|p| map(*p));
                if let Some(first) = pts.next() {
                    out.strokes.move_to(first);
                    for p in pts {
                        out.strokes.line_to(p);
                    }
                }
            }
            Piece::Closed(form, paint) => {
                let target = match paint {
                    Paint::Solid => &mut out.fill,
                    Paint::Ring => &mut out.strokes,
                };
                match form {
                    Form::Poly(points) => polygon(target, points.iter().map(|p| map(*p))),
                    Form::Disc(cx, r) => {
                        let circle = kurbo::Circle::new(map((*cx, 0.0)), r * s);
                        for el in kurbo::Shape::path_elements(&circle, 0.01) {
                            target.push(el);
                        }
                    }
                    Form::Square(cx, half) => {
                        let square = [(cx - half, -half), (cx + half, -half), (cx + half, *half), (cx - half, *half)];
                        polygon(target, square.iter().map(|p| map(*p)));
                    }
                }
            }
        }
    }
}

fn polygon(path: &mut BezPath, mut points: impl Iterator<Item = Point>) {
    if let Some(first) = points.next() {
        path.move_to(first);
        for p in points {
            path.line_to(p);
        }
        path.close_path();
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use kurbo::Shape as _;

    use super::*;

    fn path(build: impl FnOnce(&mut BezPath)) -> BezPath {
        let mut p = BezPath::new();
        build(&mut p);
        p
    }

    fn open(p: &BezPath) -> OpenEnds {
        match classify(&subpaths(p).remove(0)) {
            Course::Open(open) => open,
            Course::Closed => panic!("closed"),
            Course::Directionless => panic!("directionless"),
        }
    }

    fn near(v: Vec2, x: f64, y: f64) -> bool {
        (v.x - x).abs() < 1.0e-9 && (v.y - y).abs() < 1.0e-9
    }

    #[test]
    fn the_walk_in_from_each_end_skips_segments_with_no_length() {
        // (0, 0) to (3, 0), the last vertex repeated twice and the first once: the tips stay on the vertices, and the directions are the real
        // segment's, pointing back along the path from each tip.
        let p = path(|p| {
            p.move_to((0.0, 0.0));
            p.line_to((0.0, 0.0));
            p.line_to((3.0, 0.0));
            p.line_to((3.0, 0.0));
            p.line_to((3.0, 0.0));
        });
        let ends = open(&p);
        assert_eq!((ends.begin, ends.end), (Point::new(0.0, 0.0), Point::new(3.0, 0.0)));
        assert!(near(ends.begin_back, 1.0, 0.0) && near(ends.end_back, -1.0, 0.0), "{:?} {:?}", ends.begin_back, ends.end_back);
    }

    #[test]
    fn a_curve_before_a_repeated_vertex_gives_its_own_end_tangent() {
        // A cubic arriving at (3, 1) travelling along +x, then a LineTo onto (3, 1): the direction at the end is the cubic's.
        let p = path(|p| {
            p.move_to((0.0, 0.0));
            p.curve_to((0.0, 1.0), (2.0, 1.0), (3.0, 1.0));
            p.line_to((3.0, 1.0));
        });
        let ends = open(&p);
        assert!(near(ends.end_back, -1.0, 0.0) && near(ends.begin_back, 0.0, 1.0), "{:?} {:?}", ends.begin_back, ends.end_back);
    }

    #[test]
    fn a_path_of_segments_with_no_length_has_no_direction_and_a_closed_one_is_closed_whatever_repeats() {
        let dot = path(|p| {
            p.move_to((1.0, 1.0));
            p.line_to((1.0, 1.0));
            p.line_to((1.0, 1.0));
        });
        assert!(matches!(classify(&subpaths(&dot).remove(0)), Course::Directionless));
        let square = path(|p| {
            p.move_to((0.0, 0.0));
            p.line_to((1.0, 0.0));
            p.line_to((1.0, 0.0));
            p.line_to((1.0, 1.0));
            p.line_to((0.0, 0.0));
            p.line_to((0.0, 0.0));
        });
        assert!(matches!(classify(&subpaths(&square).remove(0)), Course::Closed));
    }

    #[test]
    fn the_walk_over_a_million_segments_with_no_length_is_linear_and_finds_the_real_one() {
        // Hostile: a path of a million repeated vertices with one real segment in the middle. The walk is linear and allocates nothing.
        let p = path(|p| {
            p.move_to((0.0, 0.0));
            for _ in 0..500_000 {
                p.line_to((0.0, 0.0));
            }
            p.line_to((2.0, 0.0));
            for _ in 0..500_000 {
                p.line_to((2.0, 0.0));
            }
        });
        // Counted, not timed: the walk asks about each segment at most once in all (from the start until a real segment, from the end until
        // one), 1,000,002 asks for the 1,000,001 segments. A walk that began again at each step would be the square of that.
        let segments = subpaths(&p).remove(0).len();
        TANGENTS.with(|c| c.set(0));
        let ends = open(&p);
        assert!(near(ends.begin_back, 1.0, 0.0) && near(ends.end_back, -1.0, 0.0));
        let asked = TANGENTS.with(std::cell::Cell::get);
        assert!((segments..=segments + 2).contains(&asked), "{asked} segments asked about, of {segments}");
    }

    // ---- The boxes of the marks. ----

    /// The base of each size class in points, typed from the table, with class 6 at the 18 (twice class 5's 9).
    const POINTS: [f64; 7] = [1.44, 1.8, 2.52, 3.24, 3.96, 9.0, 18.0];

    /// Number, then from the vertex back along the line, in `S`: where the mark starts (negative past the vertex), its reach, its spread, how far
    /// the shaft stops, and whether the rearmost closed part is solid (an opaque line then overlaps it by the tenth). Typed
    /// table; the numbers it does not measure (8, 18, 19, 31, 32, 34 to 38, 40, 44, 45) from the rule: a head of a kind no number measures
    /// takes 4's box, so 8 does and 19 takes 8's; 18 takes 6's, 40 takes 39's, 34 and 38 take 22's diamond behind a disc of 1.5, a disc takes 1.5
    /// along, a crossbar 1.5 long when it leads the mark and 0.75 behind a part, and a group's reach is the sum of its parts.
    #[rustfmt::skip]
    const BOXES: [(i64, f64, f64, f64, f64, bool); 45] = [
        (1, 0.0, 1.0, 2.00, 0.0, false), (2, 0.0, 1.0, 2.00, 1.0, true), (3, 0.0, 2.0, 2.00, 0.0, false), (4, 0.0, 2.0, 2.00, 2.0, true),
        (5, 0.0, 2.0, 1.99, 1.75, true), (6, 0.0, 2.25, 2.02, 2.25, true), (7, 0.08, 1.84, 1.92, 0.0, false), (8, 0.0, 2.0, 2.00, 1.5, true),
        (9, -1.0, 1.0, 2.00, 0.0, false), (10, -0.75, 0.75, 1.5, 0.75, true), (11, -0.75, 0.75, 1.5, 0.75, true), (12, 0.0, 3.0, 2.00, 0.0, false),
        (13, 0.0, 3.0, 2.00, 3.0, true), (14, 0.0, 3.0, 2.00, 3.0, false), (15, 0.0, 1.0, 2.00, 1.0, false), (16, 0.0, 2.0, 2.00, 2.0, false),
        (17, 0.0, 2.0, 2.00, 1.75, false), (18, 0.0, 2.25, 2.02, 2.25, false), (19, 0.0, 2.0, 2.00, 1.5, false), (20, -0.75, 0.75, 1.5, 0.75, false),
        (21, -0.75, 0.75, 1.5, 0.75, false), (22, 0.0, 4.5, 2.24, 4.5, false), (23, 1.5, 2.5, 2.00, 0.0, false), (24, 1.5, 1.5, 1.5, 0.0, false),
        (25, 1.5, 2.25, 1.5, 0.0, false), (26, 1.5, 3.0, 1.5, 0.0, false), (27, 0.0, 1.5, 1.5, 0.0, false), (28, 0.0, 2.0, 1.5, 0.0, false),
        (29, 0.0, 3.0, 1.5, 3.0, false), (30, 0.0, 3.0, 1.5, 3.0, false), (31, 0.0, 2.25, 1.5, 1.5, false), (32, 0.0, 3.0, 1.5, 1.5, false),
        (33, 0.0, 3.75, 1.5, 1.5, false), (34, 0.0, 6.0, 2.24, 6.0, false), (35, 0.0, 2.25, 1.5, 1.5, true), (36, 0.0, 3.0, 1.5, 1.5, true),
        (37, 0.0, 3.75, 1.5, 1.5, true), (38, 0.0, 6.0, 2.24, 6.0, false), (39, 0.0, 4.0, 2.00, 4.0, true), (40, 0.0, 4.0, 2.00, 4.0, false),
        (41, 0.0, 1.5, 1.5, 1.5, false), (42, -0.75, 0.75, 1.5, 0.75, true), (43, 0.0, 4.0, 2.00, 0.0, false), (44, 0.0, 2.75, 2.00, 0.0, false),
        (45, 0.0, 4.75, 2.00, 0.0, false),
    ];

    /// The line of the tests: 10 inches along x. Its two vertices are (0, 0), where a mark's back runs toward +x, and (10, 0), toward -x.
    fn straight() -> BezPath {
        path(|p| {
            p.move_to((0.0, 0.0));
            p.line_to((10.0, 0.0));
        })
    }

    /// The marks of `number` on one end of the straight line, in paper inches.
    fn marked(number: i64, at_begin: bool, size: i64, weight: f64, see_through: bool) -> Ends {
        let (begin, end) = if at_begin { (number, 0) } else { (0, number) };
        let request = Request { begin, end, begin_size: size, end_size: size, weight, see_through, begin_custom: None, end_custom: None };
        plan(&straight(), &request).ends.expect("a mark is drawn")
    }

    /// A point of a mark as (distance back from its vertex, across), in `S`.
    fn in_s(p: Point, at_begin: bool, s: f64) -> (f64, f64) {
        let back = if at_begin { p.x } else { 10.0 - p.x };
        (back / s, (if at_begin { p.y } else { -p.y }) / s)
    }

    fn drawn_box(ends: &Ends) -> kurbo::Rect {
        [&ends.fill, &ends.strokes].iter().filter(|p| !p.elements().is_empty()).map(|p| p.bounding_box()).reduce(|a, b| a.union(b)).expect("ink")
    }

    /// Where a mark lies, from the geometry the renderer made: (start, reach, spread, how far the shaft stops), in `S`.
    fn measure(ends: &Ends, at_begin: bool, s: f64) -> (f64, f64, f64, f64) {
        let (ink, shaft) = (drawn_box(ends), ends.shaft.bounding_box());
        let (start, reach, stop) =
            if at_begin { (ink.x0 / s, ink.x1 / s, shaft.x0 / s) } else { ((10.0 - ink.x1) / s, (10.0 - ink.x0) / s, (10.0 - shaft.x1) / s) };
        (start, reach, (ink.y1 - ink.y0) / s, stop)
    }

    fn close(a: (f64, f64, f64, f64), b: (f64, f64, f64, f64)) -> bool {
        [(a.0, b.0), (a.1, b.1), (a.2, b.2), (a.3, b.3)].iter().all(|(x, y)| (x - y).abs() < 2.0e-3)
    }

    /// Every vertex of a path as (back, across) in `S`, sorted, so that two polygons compare whatever vertex they start at.
    fn vertices(path: &BezPath, at_begin: bool, s: f64) -> Vec<(f64, f64)> {
        let mut out: Vec<(f64, f64)> = path
            .elements()
            .iter()
            .filter_map(|el| match *el {
                PathEl::MoveTo(p) | PathEl::LineTo(p) => Some(in_s(p, at_begin, s)),
                _ => None,
            })
            .map(|(a, b)| ((a * 1.0e6).round() / 1.0e6, (b * 1.0e6).round() / 1.0e6))
            .collect();
        out.sort_by(|a, b| a.partial_cmp(b).expect("finite"));
        out
    }

    /// Where the crossbars of a mark lie, in `S` back from the vertex: the strokes of two points across the line, as long as 1.5.
    fn crossbars(ends: &Ends, s: f64) -> Vec<f64> {
        let mut found = Vec::new();
        let mut pen = None;
        let mut start = None;
        for el in ends.strokes.elements() {
            match *el {
                PathEl::MoveTo(p) => (pen, start) = (Some(p), Some(p)),
                PathEl::LineTo(q) => {
                    if let (Some(p), Some(first)) = (pen, start) {
                        // A bar is a subpath of exactly these two points.
                        if p == first && (p.x - q.x).abs() < 1.0e-12 && ((p.y - q.y).abs() / s - 1.5).abs() < 1.0e-9 {
                            found.push(((10.0 - p.x) / s * 1.0e6).round() / 1.0e6);
                        }
                    }
                    pen = Some(q);
                    start = None;
                }
                _ => (pen, start) = (None, None),
            }
        }
        found.sort_by(|a, b| a.partial_cmp(b).expect("finite"));
        found
    }

    #[test]
    fn every_line_end_fills_its_box_and_stops_the_shaft_where_the_rules_say_at_two_classes_and_two_weights() {
        // Both ends, classes 2 and 6 (6 at the 18 points), weights 0.01 and 0.05 inch, an opaque line and a
        // see-through one: the box is measured on what `plan` drew, against numbers typed from the spec.
        let mut wrong = Vec::new();
        for (number, front, reach, spread, stop, solid_back) in BOXES {
            for at_begin in [false, true] {
                for size in [2_i64, 6] {
                    for weight in [0.01, 0.05] {
                        for see_through in [false, true] {
                            let s = POINTS[usize::try_from(size).expect("a small class")] / 72.0 + weight;
                            let got = measure(&marked(number, at_begin, size, weight, see_through), at_begin, s);
                            let overlap = if solid_back && !see_through && stop > 0.0 { 0.1 } else { 0.0 };
                            let want = (front, reach, spread, stop - overlap);
                            if !close(got, want) {
                                wrong.push(format!(
                                    "{number} at_begin {at_begin} size {size} weight {weight} see_through {see_through}: got {got:?}, want {want:?}"
                                ));
                            }
                        }
                    }
                }
            }
        }
        assert!(wrong.is_empty(), "{} cases wrong, the first:\n{}", wrong.len(), wrong.iter().take(10).cloned().collect::<Vec<_>>().join("\n"));
    }

    #[test]
    fn number_8_is_a_solid_head_with_its_back_hollowed_to_1_5_in_the_box_of_number_4() {
        // 8 is the solid `head(2, 1, 1.5)`: the tip, the two back corners at 2 back and 1 across, and the hollow's apex at 1.5.
        // The box is 4's (2 back, 2 across), and the shaft stops at 1.5, not at 5's 1.75.
        let s = 0.035 + 0.02;
        let ends = marked(8, false, 2, 0.02, false);
        assert!(ends.strokes.elements().is_empty(), "solid: nothing is stroked");
        assert_eq!(vertices(&ends.fill, false, s), vec![(0.0, 0.0), (1.5, 0.0), (2.0, -1.0), (2.0, 1.0)]);
        let four = marked(4, false, 2, 0.02, false);
        let (a, b) = (drawn_box(&ends), drawn_box(&four));
        assert!(
            (a.x0 - b.x0).abs() < 1.0e-9 && (a.x1 - b.x1).abs() < 1.0e-9 && (a.y0 - b.y0).abs() < 1.0e-9 && (a.y1 - b.y1).abs() < 1.0e-9,
            "{a:?} {b:?}"
        );
        // Opaque and solid: the shaft stops 0.1 S inside the apex.
        assert!((10.0 - ends.shaft.bounding_box().x1 - 1.4 * s).abs() < 1.0e-9);
    }

    #[test]
    fn number_19_is_the_same_head_hollow_in_8s_box_which_is_4s() {
        // `ring(head(2, 1, 1.5))`, in 8's box, which is 4's. Hollow: the outline is stroked and nothing is filled, and the shaft stops
        // exactly at 1.5 whatever the line (nothing is overlapped, so nothing shows inside the ring).
        let s = 0.035 + 0.02;
        for see_through in [false, true] {
            let ends = marked(19, false, 2, 0.02, see_through);
            assert!(ends.fill.elements().is_empty(), "hollow: nothing is filled");
            assert_eq!(vertices(&ends.strokes, false, s), vec![(0.0, 0.0), (1.5, 0.0), (2.0, -1.0), (2.0, 1.0)]);
            assert!((10.0 - ends.shaft.bounding_box().x1 - 1.5 * s).abs() < 1.0e-9, "see_through {see_through}");
        }
        let (a, b) = (drawn_box(&marked(19, false, 2, 0.02, false)), drawn_box(&marked(8, false, 2, 0.02, false)));
        assert!((a.x0 - b.x0).abs() < 1.0e-9 && (a.x1 - b.x1).abs() < 1.0e-9 && (a.y0 - b.y0).abs() < 1.0e-9 && (a.y1 - b.y1).abs() < 1.0e-9);
    }

    #[test]
    fn number_18_takes_6s_box_and_40_takes_39s_in_one_paint() {
        let s = 0.035 + 0.02;
        // 18 is 6's head hollow: the bulging back at 2.25 and the spread of 2 times 1.01.
        let eighteen = marked(18, false, 2, 0.02, false);
        assert!(eighteen.fill.elements().is_empty());
        assert_eq!(vertices(&eighteen.strokes, false, s), vec![(0.0, 0.0), (2.0, -1.01), (2.0, 1.01), (2.25, 0.0)]);
        // 40 is 39's two triangles, hollow: both outlined, none filled.
        let forty = marked(40, false, 2, 0.02, false);
        assert!(forty.fill.elements().is_empty());
        assert_eq!(forty.strokes.elements().iter().filter(|e| matches!(e, PathEl::ClosePath)).count(), 2);
    }

    #[test]
    fn a_disc_with_a_hollow_long_diamond_is_a_solid_disc_and_an_outlined_diamond_and_the_shaft_stops_at_the_diamond_exactly() {
        // 38 is a solid `disc(0.75, 0.75)` and `ring(diamond(1.5, 6, 1.12))`. The rearmost part is hollow, so even an opaque line stops at the
        // back, not 0.1 S inside it.
        let s = 0.035 + 0.02;
        let ends = marked(38, false, 2, 0.02, false);
        assert!(!ends.fill.elements().is_empty() && !ends.strokes.elements().is_empty());
        let filled = ends.fill.bounding_box();
        assert!(((10.0 - filled.x1) / s).abs() < 1.0e-3 && ((10.0 - filled.x0) / s - 1.5).abs() < 1.0e-3, "the disc, 0 to 1.5: {filled:?}");
        assert!((10.0 - ends.shaft.bounding_box().x1 - 6.0 * s).abs() < 1.0e-9);
        // 34 is the same mark with a hollow disc: nothing filled.
        assert!(marked(34, false, 2, 0.02, false).fill.elements().is_empty());
    }

    #[test]
    fn the_crossbars_sit_where_the_rule_puts_them() {
        // A bar leading a mark is 1.5 long to its back and lies at 1.5 (24, 25's first, 26's first); one behind a part or another
        // bar is 0.75 long, at the back of the part before it, plus 0.75 (25 at 2.25, 26 at 2.25 and 3, a disc and bars at 2.25, 3 and 3.75, an open
        // head's bar at 2.75, two open heads' at 4.75). 28 keeps its measured 2 and 30 its bar at the middle of its leading box, as R29 draws.
        let want: [(i64, &[f64]); 15] = [
            (24, &[1.5]),
            (25, &[1.5, 2.25]),
            (26, &[1.5, 2.25, 3.0]),
            (28, &[2.0]),
            (30, &[0.75]),
            (31, &[2.25]),
            (32, &[2.25, 3.0]),
            (33, &[2.25, 3.0, 3.75]),
            (35, &[2.25]),
            (36, &[2.25, 3.0]),
            (37, &[2.25, 3.0, 3.75]),
            (44, &[2.75]),
            (45, &[4.75]),
            (27, &[]),
            (43, &[]),
        ];
        let s = 0.035 + 0.02;
        for (number, bars) in want {
            assert_eq!(crossbars(&marked(number, false, 2, 0.02, false), s), bars.to_vec(), "number {number}");
        }
    }

    #[test]
    fn number_7_bends_its_strokes_as_draws_them_and_5_and_6_take_their_measured_spreads() {
        let s = 0.035 + 0.02;
        let seven = marked(7, false, 2, 0.02, false);
        assert_eq!(vertices(&seven.strokes, false, s), vec![(0.08, 0.0), (0.5, -0.48), (0.5, 0.48), (1.84, -0.96), (1.84, 0.96)]);
        assert_eq!(vertices(&marked(5, false, 2, 0.02, false).fill, false, s), vec![(0.0, 0.0), (1.75, 0.0), (2.0, -0.995), (2.0, 0.995)]);
        assert_eq!(vertices(&marked(6, false, 2, 0.02, false).fill, false, s), vec![(0.0, 0.0), (2.0, -1.01), (2.0, 1.01), (2.25, 0.0)]);
    }

    #[test]
    fn closed_parts_are_painted_by_number_solid_ones_filled_and_hollow_ones_stroked() {
        // The Paint column, by part: the pure solids fill only; 35 to 38 fill their disc and stroke what else they have; every other number
        // strokes only (open strokes, outlines of hollow parts).
        let pure_solid = [2, 4, 5, 6, 8, 10, 11, 13, 39, 42];
        let mixed = [35, 36, 37, 38];
        for (number, ..) in BOXES {
            let ends = marked(number, false, 2, 0.02, false);
            let (filled, stroked) = (!ends.fill.elements().is_empty(), !ends.strokes.elements().is_empty());
            let want = if pure_solid.contains(&number) {
                (true, false)
            } else if mixed.contains(&number) {
                (true, true)
            } else {
                (false, true)
            };
            assert_eq!((filled, stroked), want, "number {number}");
        }
    }

    #[test]
    fn a_square_is_four_distinct_corners_a_half_side_of_0_75_from_the_vertex_solid_for_11_and_hollow_for_21() {
        // -0.75 to 0.75, spread 1.5: the box alone would pass a square with two corners on one another, so the corners are checked.
        let s = 0.035 + 0.02;
        let corners = vec![(-0.75, -0.75), (-0.75, 0.75), (0.75, -0.75), (0.75, 0.75)];
        let solid = marked(11, false, 2, 0.02, false);
        assert!(solid.strokes.elements().is_empty());
        assert_eq!(vertices(&solid.fill, false, s), corners);
        let hollow = marked(21, false, 2, 0.02, false);
        assert!(hollow.fill.elements().is_empty());
        assert_eq!(vertices(&hollow.strokes, false, s), corners);
    }

    #[test]
    fn a_forms_back_is_how_far_it_reaches_and_the_rearmost_closed_part_decides_the_overlap() {
        // The helpers themselves: a polygon reaches to its farthest vertex, a disc or a square to its centre plus its radius.
        assert_eq!(Form::Poly(vec![(0.0, 0.0), (2.0, 1.0), (1.5, 0.0)]).back(), 2.0);
        assert_eq!(Form::Poly(vec![(-3.0, 0.0), (-1.0, 1.0)]).back(), -1.0);
        assert_eq!(Form::Disc(2.25, 0.75).back(), 3.0);
        assert_eq!(Form::Square(0.5, 0.75).back(), 1.25);
        // The part furthest back decides whether the overlap applies, whatever order the art lists its parts in.
        let made = |pieces: Vec<Piece>| Art { pieces, stop: 3.0 };
        let near = || Form::Poly(vec![(0.0, 0.0), (2.0, 1.0), (2.0, -1.0)]);
        let far = || Form::Disc(2.25, 0.75);
        assert!(!made(vec![solid(near()), ring(far())]).solid_back());
        assert!(!made(vec![ring(far()), solid(near())]).solid_back());
        assert!(made(vec![solid(far()), ring(near())]).solid_back());
        assert!(made(vec![ring(near()), solid(far())]).solid_back());
        // Open pieces have no back to stop at, and a mark with no closed part has no solid back.
        assert!(made(vec![bar(5.0), solid(near())]).solid_back());
        assert!(!made(vec![bar(5.0), open_head(0.0, 3.0)]).solid_back());
        // No mark has two closed parts that end at the same place, so that no tie is left to decide.
        for number in 1..=45 {
            let backs: Vec<f64> = art(number)
                .expect("an art")
                .pieces
                .iter()
                .filter_map(|p| if let Piece::Closed(form, _) = p { Some(form.back()) } else { None })
                .collect();
            for (i, a) in backs.iter().enumerate() {
                assert!(backs.iter().skip(i + 1).all(|b| (a - b).abs() > 1.0e-9), "number {number}: {backs:?}");
            }
        }
    }

    #[test]
    fn the_numbers_no_source_measures_are_the_dashes_of_end_and_43_is_measured() {
        let listed: Vec<i64> = (-2..=300).filter(|n| is_unmeasured(*n)).collect();
        assert_eq!(listed, vec![8, 18, 19, 31, 32, 34, 35, 36, 37, 38, 40, 44, 45]);
    }

    #[test]
    fn a_size_class_is_its_base_plus_the_weight_class_6_is_18_points_and_other_numbers_read_as_class_2() {
        // The bases: class 6 is 18 points, twice class 5's, and a number outside 0 to 6 takes class 2's.
        for (class, points) in POINTS.iter().enumerate() {
            let class = i64::try_from(class).expect("small");
            for weight in [0.0, 0.01, 0.5] {
                assert!((unit(class, weight) - (points / 72.0 + weight)).abs() < 1.0e-12, "class {class} weight {weight}");
            }
        }
        assert!((COLOSSAL_BASE * 72.0 - 18.0).abs() < 1.0e-12 && (COLOSSAL_BASE - 2.0 * SIZE_BASE[5]).abs() < 1.0e-12);
        for outside in [-1, 7, 8, 254, i64::MAX, i64::MIN] {
            assert!((unit(outside, 0.02) - (2.52 / 72.0 + 0.02)).abs() < 1.0e-12, "class {outside}");
        }
        // A weight below 0 adds nothing.
        assert!((unit(2, -1.0) - 2.52 / 72.0).abs() < 1.0e-12);
        assert!(size_is_standin(-1) && size_is_standin(7) && size_is_standin(i64::MAX));
        assert!((0..=6).all(|class| !size_is_standin(class)));
    }

    #[test]
    fn a_mark_is_drawn_for_the_numbers_1_to_45_and_for_no_other() {
        assert!((1..=45).all(is_drawn));
        for other in [0, 46, 47, 99, 253, 254, 255, 1000, -1, i64::MAX, i64::MIN] {
            assert!(!is_drawn(other), "{other}");
        }
    }
}
