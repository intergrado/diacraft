//! The art of DiaCraft's own line-end markers.
//!
//! A marker is **one path** drawn in its `svg:viewBox`, which LibreOffice fills by the even-odd rule, so this module builds, for a number and a band
//! class, **the union of the pieces as contours whose holes run against their outer contour**: the same set of points under the even-odd and the nonzero
//! rule. Each piece is worked out in units of `S` (x across the line, y back along it from the vertex), multiplied by 1000, rounded to whole numbers and moved
//! so that the bounding box of all of them starts at 0, 0.
//!
//! * **Pieces.** A polygon, its ring (the polygon less its inset by one band, β), its open form (the same, with one side left unmoved so the band runs along
//!   every other side), a bar, a spine, a slanted stroke, a disc and a ring of one. [`inset`] moves each side inward and puts each corner where its two moved
//!   sides meet; an inset that is not a simple polygon with every side keeping its direction is empty, and the piece is all of its polygon.
//! * **The union.** Pieces whose bounding boxes overlap are united by [`unite`]: every edge is cut at each point another edge crosses or touches it, each
//!   cut edge is kept when one side of it is outside every piece (a winding count on both sides, which must agree with the edge's own direction), and the
//!   kept edges are chained into contours. Pieces that only touch (a disc and the spine under it) are written as they are. Every number of R29 and every band
//!   class from 1 to 99 is built in the tests and checked against the pieces point by point.
//! * **Bounded.** The input is a number from 1 to 45 and a band class from 0 to 99, so the whole domain is 3,475 markers (35 numbers with a band at 99 band classes, and ten without), all built by a test. A cluster of
//!   pieces holds at most [`MOST_EDGES`] edges, and the work (edge pairs tried, edges walked in a winding count) is counted in `steps`.

use std::collections::BTreeMap;

/// A point in units of `S`.
type Pt = (f64, f64);

/// A point in thousandths of `S`.
pub(crate) type IPt = (i64, i64);

/// The most edges the contours of one cluster of overlapping pieces may have: the largest cluster of any number at any band class has 24 (a test measures it).
const MOST_EDGES: usize = 64;

/// The longest path a marker the writer draws may have, in bytes: the longest, over every number and band class, is 499 (number 33 at band class 50). It bounds
/// only the markers this writer builds. A marker the file already holds under DiaCraft's name is used as it is, whatever its path (a program that
/// saves our file rewrites the path), so a held marker may have a path of any length the part's own limits allow; the writer never copies or reads that path.
pub(crate) const MOST_PATH_BYTES: usize = 1024;

/// How far from the middle of an edge a winding count is taken, in thousandths of `S`.
const SAMPLE: f64 = 1e-3;

/// A piece of a mark, in thousandths of `S`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Piece {
    /// Contours, the first the outer one running positive (twice the signed area is greater than 0), the others its holes running negative.
    Poly(Vec<Vec<IPt>>),
    /// A disc centred at (0, `cy`) of radius `r`, with a hole of radius `hole` when that is above 0.
    Disc { cy: i64, r: i64, hole: i64 },
}

/// What a built marker is: the `svg:viewBox` and `svg:d`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Built {
    pub view_box: String,
    pub d: String,
}

/// Whether the mark of number `n` is drawn with a band (the last column): a hollow or open part is a band, so the marker depends on the band class.
pub(crate) fn has_band(n: u32) -> bool {
    matches!(n, 1 | 3 | 7 | 9 | 12 | 14..=38 | 40 | 41 | 43..=45)
}

/// Whether the mark of number `n` is centred on the vertex: the disc and the square, solid and hollow, and the slanted stroke through it.
pub(crate) fn is_centred(n: u32) -> bool {
    matches!(n, 9 | 10 | 11 | 20 | 21 | 42)
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------
// The pieces of R29.

fn tri(a: f64, r: f64, h: f64) -> Vec<Pt> {
    vec![(0.0, a), (h, r), (-h, r)]
}

fn head(r: f64, h: f64, m: f64) -> Vec<Pt> {
    vec![(0.0, 0.0), (h, r), (0.0, m), (-h, r)]
}

fn diamond(a: f64, b: f64, h: f64) -> Vec<Pt> {
    let mid = (a + b) / 2.0;
    vec![(0.0, a), (h, mid), (0.0, b), (-h, mid)]
}

fn square(c: f64, half: f64) -> Vec<Pt> {
    vec![(-half, c - half), (half, c - half), (half, c + half), (-half, c + half)]
}

fn fork() -> Vec<Pt> {
    vec![(-0.75, 0.0), (0.75, 0.0), (0.0, 1.5)]
}

fn curved() -> Vec<Pt> {
    vec![(0.0, 0.08), (0.48, 0.5), (0.96, 1.84), (-0.96, 1.84), (-0.48, 0.5)]
}

/// Twice the signed area of a polygon: positive when it runs the way the polygons do.
fn area2_f(points: &[Pt]) -> f64 {
    let n = points.len();
    (0..n)
        .map(|i| {
            let (a, b) = (points.get(i).copied().unwrap_or_default(), points.get((i + 1) % n).copied().unwrap_or_default());
            a.0 * b.1 - b.0 * a.1
        })
        .sum()
}

/// Twice the signed area of a contour of whole numbers.
fn area2(points: &[IPt]) -> i128 {
    let n = points.len();
    (0..n)
        .map(|i| {
            let (a, b) = (points.get(i).copied().unwrap_or_default(), points.get((i + 1) % n).copied().unwrap_or_default());
            i128::from(a.0) * i128::from(b.1) - i128::from(b.0) * i128::from(a.1)
        })
        .sum()
}

/// A point in units of `S` as thousandths, rounded to the nearest whole number (half away from zero).
fn thousandths(v: f64) -> i64 {
    // The values are a few units at most, and finite: the band is a share from 0.01 to 0.99.
    (v * 1000.0).round() as i64
}

/// A polygon as a contour of whole numbers: each corner rounded, repeated corners kept once.
fn rounded(points: &[Pt]) -> Vec<IPt> {
    let mut out: Vec<IPt> = Vec::with_capacity(points.len());
    for p in points {
        let q = (thousandths(p.0), thousandths(p.1));
        if out.last() != Some(&q) {
            out.push(q);
        }
    }
    while out.len() > 1 && out.first() == out.last() {
        out.pop();
    }
    out
}

/// `contour` running positive: reversed when it runs the other way.
fn positive(mut contour: Vec<IPt>) -> Vec<IPt> {
    if area2(&contour) < 0 {
        contour.reverse();
    }
    contour
}

/// A solid polygon.
fn solid(points: &[Pt]) -> Piece {
    Piece::Poly(vec![positive(rounded(points))])
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Piece {
    solid(&[(x0, y0), (x1, y0), (x1, y1), (x0, y1)])
}

/// `bar(y, h)`: the rectangle from x = −`h` to `h` and y = `y` − β/2 to `y` + β/2.
fn bar(y: f64, h: f64, beta: f64) -> Piece {
    rect(-h, y - beta / 2.0, h, y + beta / 2.0)
}

/// `spine(y0, y1)`: the rectangle from x = −β/2 to β/2 and y = `y0` to `y1`.
fn spine(y0: f64, y1: f64, beta: f64) -> Piece {
    rect(-beta / 2.0, y0, beta / 2.0, y1)
}

/// `stroke(P, Q)`: the points within β/2 of the line through `p` and `q` whose x lies between theirs, a parallelogram with two vertical sides.
fn stroke(p: Pt, q: Pt, beta: f64) -> Piece {
    let across = (q.0 - p.0).abs();
    let lift = if across > 0.0 { (beta / 2.0) * (q.0 - p.0).hypot(q.1 - p.1) / across } else { 0.0 };
    solid(&[(p.0, p.1 - lift), (q.0, q.1 - lift), (q.0, q.1 + lift), (p.0, p.1 + lift)])
}

/// A disc of radius `r` centred at (0, `c`), solid.
fn disc(c: f64, r: f64) -> Piece {
    Piece::Disc { cy: thousandths(c), r: thousandths(r), hole: 0 }
}

/// A disc less its inset by one band: a ring, or the whole disc when the inset is empty (R − β ≤ 0, once rounded).
fn disc_ring(c: f64, r: f64, beta: f64) -> Piece {
    let hole = thousandths(r - beta);
    Piece::Disc { cy: thousandths(c), r: thousandths(r), hole: hole.max(0) }
}

/// The corner of the inset where side `i - 1` and side `i`, each moved inward by its offset, meet.
fn corner(f: &[Pt], offsets: &[f64], i: usize) -> Option<Pt> {
    let n = f.len();
    let prev = (i + n - 1) % n;
    // A side of no length has normals that are not numbers, which the determinant test below does not pass: such a polygon has no corner.
    let side = |k: usize| -> Option<(Pt, Pt, f64)> {
        let (a, b) = (f.get(k).copied()?, f.get((k + 1) % n).copied()?);
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len = dx.hypot(dy);
        Some((a, (-dy / len, dx / len), offsets.get(k).copied().unwrap_or(0.0)))
    };
    let ((a0, n0, o0), (a1, n1, o1)) = (side(prev)?, side(i)?);
    // n0 . W = o0 + n0 . a0 and n1 . W = o1 + n1 . a1.
    let (c0, c1) = (o0 + n0.0 * a0.0 + n0.1 * a0.1, o1 + n1.0 * a1.0 + n1.1 * a1.1);
    let det = n0.0 * n1.1 - n0.1 * n1.0;
    (det.abs() > 1e-9).then(|| ((c0 * n1.1 - c1 * n0.1) / det, (n0.0 * c1 - n1.0 * c0) / det))
}

/// Whether the closed polygon of whole numbers `c` runs positive and has no two sides that meet but at the corner they share. Sides that share a corner are left
/// to the others: two that fold back on each other put an end of one on the other's inside, which a side that is not next to it meets, and a polygon of three
/// corners folded flat has no area.
fn is_simple(c: &[IPt]) -> bool {
    let n = c.len();
    if n < 3 || area2(c) <= 0 {
        return false;
    }
    for i in 0..n {
        for j in i + 2..n {
            // The last side is next to the first.
            if i == 0 && j + 1 == n {
                continue;
            }
            let (a, b) = (c.get(i).copied().unwrap_or_default(), c.get((i + 1) % n).copied().unwrap_or_default());
            let (p, q) = (c.get(j).copied().unwrap_or_default(), c.get((j + 1) % n).copied().unwrap_or_default());
            if segments_meet(a, b, p, q) {
                return false;
            }
        }
    }
    true
}

/// Whether the segments `a`-`b` and `p`-`q` have a point in common.
fn segments_meet(a: IPt, b: IPt, p: IPt, q: IPt) -> bool {
    let cross = |o: IPt, x: IPt, y: IPt| -> i128 { i128::from(x.0 - o.0) * i128::from(y.1 - o.1) - i128::from(x.1 - o.1) * i128::from(y.0 - o.0) };
    let (d1, d2, d3, d4) = (cross(p, q, a), cross(p, q, b), cross(a, b, p), cross(a, b, q));
    let on = |o: IPt, x: IPt, y: IPt| -> bool { x.0.min(y.0) <= o.0 && o.0 <= x.0.max(y.0) && x.1.min(y.1) <= o.1 && o.1 <= x.1.max(y.1) };
    if ((d1 > 0 && d2 < 0) || (d1 < 0 && d2 > 0)) && ((d3 > 0 && d4 < 0) || (d3 < 0 && d4 > 0)) {
        return true;
    }
    (d1 == 0 && on(a, p, q)) || (d2 == 0 && on(b, p, q)) || (d3 == 0 && on(p, a, b)) || (d4 == 0 && on(q, a, b))
}

/// Whether a side's new vector `new` is a positive multiple of its old one `old` (the two are parallel by the way a corner is found).
fn keeps_direction(old: Pt, new: Pt) -> bool {
    new.0 * old.0 + new.1 * old.1 > 1e-9
}

/// The polygon `f` (running positive) with side `i` moved inward by `offsets[i]`, each corner where its two moved sides meet: `None` when that is not a
/// simple polygon of `f`'s orientation with every side keeping its direction, which is an empty inset.
pub(crate) fn inset(f: &[Pt], offsets: &[f64]) -> Option<Vec<Pt>> {
    let n = f.len();
    let moved: Vec<Pt> = (0..n).map(|i| corner(f, offsets, i)).collect::<Option<Vec<Pt>>>()?;
    for i in 0..n {
        let (a, b) = (f.get(i)?, f.get((i + 1) % n)?);
        let (p, q) = (moved.get(i)?, moved.get((i + 1) % n)?);
        if !keeps_direction((b.0 - a.0, b.1 - a.1), (q.0 - p.0, q.1 - p.1)) {
            return None;
        }
    }
    (area2_f(&moved) > 0.0).then_some(moved)
}

/// Whether the hole `inner` (running positive, as the inset does) is one to cut out of `outer`: both simple, and every corner of the hole inside the outer one.
/// Rounding to whole numbers can collapse a hole that is a few units across.
fn usable_hole(outer: &[IPt], inner: &[IPt]) -> bool {
    is_simple(outer) && is_simple(inner) && inner.iter().all(|p| inside(outer, *p))
}

/// `ring(F)`: `F` less its inset by β, the outline drawn one band thick inside it; all of `F` when the inset is empty.
fn ring(f: &[Pt], beta: f64) -> Piece {
    let whole = solid(f);
    let Some(inner) = inset(f, &vec![beta; f.len()]) else { return whole };
    let outer = positive(rounded(f));
    let inner = rounded(&inner);
    if !usable_hole(&outer, &inner) {
        return whole;
    }
    // The hole runs against its outer contour, from its first corner on.
    let mut hole = inner;
    hole.reverse();
    hole.rotate_right(1);
    Piece::Poly(vec![outer, hole])
}

/// `open(F, s)`: `F` less the inset that moves every side but side `s` by β, so that the band runs along every side but `s`; all of `F` when the inset is
/// empty. The band is one contour: `F`'s corners from the end of side `s` round to its start, then the inset's corners back.
fn open(f: &[Pt], s: usize, beta: f64) -> Piece {
    let whole = solid(f);
    let n = f.len();
    let offsets: Vec<f64> = (0..n).map(|i| if i == s { 0.0 } else { beta }).collect();
    let Some(inner) = inset(f, &offsets) else { return whole };
    let mut band: Vec<Pt> = (1..=n).filter_map(|k| f.get((s + k) % n).copied()).collect();
    band.extend((0..n).filter_map(|k| inner.get((s + n - k) % n).copied()));
    let band = rounded(&band);
    if is_simple(&band) { Piece::Poly(vec![band]) } else { whole }
}

/// Whether `p` is inside the polygon of whole numbers `c`, or on its boundary.
fn inside(c: &[IPt], p: IPt) -> bool {
    let n = c.len();
    let mut winding = 0i32;
    for i in 0..n {
        let (a, b) = (c.get(i).copied().unwrap_or_default(), c.get((i + 1) % n).copied().unwrap_or_default());
        let side = i128::from(b.0 - a.0) * i128::from(p.1 - a.1) - i128::from(b.1 - a.1) * i128::from(p.0 - a.0);
        if side == 0 && p.0 >= a.0.min(b.0) && p.0 <= a.0.max(b.0) && p.1 >= a.1.min(b.1) && p.1 <= a.1.max(b.1) {
            return true;
        }
        if a.1 <= p.1 {
            if b.1 > p.1 && side > 0 {
                winding += 1;
            }
        } else if b.1 <= p.1 && side < 0 {
            winding -= 1;
        }
    }
    winding != 0
}

/// The pieces for number `n` at band β (`beta` is 0 for a mark with no band): `None` for a number that is not 1 to 45.
pub(crate) fn pieces(n: u32, beta: f64) -> Option<Vec<Piece>> {
    let b = beta;
    let (back, front) = (1, 0);
    let bars = |ys: &[f64]| -> Vec<Piece> { ys.iter().map(|y| bar(*y, 0.75, b)).collect() };
    let join = |mut first: Vec<Piece>, rest: Vec<Piece>| -> Vec<Piece> {
        first.extend(rest);
        first
    };
    Some(match n {
        1 => vec![open(&tri(0.0, 1.0, 1.0), back, b), spine(0.0, 1.0, b)],
        2 => vec![solid(&tri(0.0, 1.0, 1.0))],
        3 => vec![open(&tri(0.0, 2.0, 1.0), back, b), spine(0.0, 2.0, b)],
        4 => vec![solid(&tri(0.0, 2.0, 1.0))],
        5 => vec![solid(&head(2.0, 0.995, 1.75))],
        6 => vec![solid(&head(2.0, 1.01, 2.25))],
        7 => vec![open(&curved(), 2, b), spine(0.0, 1.84, b)],
        8 => vec![solid(&head(2.0, 1.0, 1.5))],
        9 => vec![stroke((1.0, -1.0), (-1.0, 1.0), b), spine(0.0, 1.0, b)],
        10 => vec![disc(0.0, 0.75)],
        11 => vec![solid(&square(0.0, 0.75))],
        12 => vec![open(&tri(0.0, 3.0, 1.0), back, b), spine(0.0, 3.0, b)],
        13 => vec![solid(&tri(0.0, 3.0, 1.0))],
        14 => vec![ring(&tri(0.0, 3.0, 1.0), b)],
        15 => vec![ring(&tri(0.0, 1.0, 1.0), b)],
        16 => vec![ring(&tri(0.0, 2.0, 1.0), b)],
        17 => vec![ring(&head(2.0, 1.0, 1.75), b)],
        18 => vec![ring(&head(2.0, 1.01, 2.25), b)],
        19 => vec![ring(&head(2.0, 1.0, 1.5), b)],
        20 => vec![disc_ring(0.0, 0.75, b)],
        21 => vec![ring(&square(0.0, 0.75), b)],
        22 => vec![ring(&diamond(0.0, 4.5, 1.12), b)],
        23 => vec![stroke((1.0, 2.5), (-1.0, 1.5), b), spine(0.0, 2.5, b)],
        24..=26 => {
            let ys = [1.5, 2.25, 3.0];
            let count = usize::try_from(n - 23).ok()?;
            let last = ys.get(count - 1).copied()?;
            join(bars(ys.get(..count)?), vec![spine(0.0, last + b / 2.0, b)])
        }
        27 => vec![open(&fork(), front, b), spine(0.0, 1.5, b)],
        // The row 28 lists 27's pieces, `spine(0, 1.5)` among them, and then `spine(0, 2 + β/2)`, which covers it: the union takes the shorter spine once, so the
        // two give one contour (a redundancy in the specification's text, which the writer follows as it is written).
        28 => join(vec![open(&fork(), front, b), spine(0.0, 1.5, b)], vec![bar(2.0, 0.75, b), spine(0.0, 2.0 + b / 2.0, b)]),
        29 => vec![open(&fork(), front, b), disc_ring(2.25, 0.75, b)],
        30 => vec![bar(0.75, 0.75, b), disc_ring(2.25, 0.75, b), spine(0.0, 1.5, b)],
        31..=33 | 35..=37 => {
            // Disc and one, two or three crossbars; the disc is hollow for 31 to 33 and solid for 35 to 37.
            let ys = [2.25, 3.0, 3.75];
            let count = usize::try_from(if n <= 33 { n - 30 } else { n - 34 }).ok()?;
            let last = ys.get(count - 1).copied()?;
            let ball = if n <= 33 { disc_ring(0.75, 0.75, b) } else { disc(0.75, 0.75) };
            join(vec![ball], join(bars(ys.get(..count)?), vec![spine(1.5, last + b / 2.0, b)]))
        }
        34 => vec![disc_ring(0.75, 0.75, b), ring(&diamond(1.5, 6.0, 1.12), b)],
        38 => vec![disc(0.75, 0.75), ring(&diamond(1.5, 6.0, 1.12), b)],
        39 => vec![solid(&tri(0.0, 2.0, 1.0)), solid(&tri(2.0, 4.0, 1.0))],
        40 => vec![ring(&tri(0.0, 2.0, 1.0), b), ring(&tri(2.0, 4.0, 1.0), b)],
        41 => vec![disc_ring(0.75, 0.75, b)],
        42 => vec![disc(0.0, 0.75)],
        43 => vec![open(&tri(0.0, 2.0, 1.0), back, b), open(&tri(2.0, 4.0, 1.0), back, b), spine(0.0, 4.0, b)],
        44 => vec![open(&tri(0.0, 2.0, 1.0), back, b), bar(2.75, 0.75, b), spine(0.0, 2.75 + b / 2.0, b)],
        45 => vec![
            open(&tri(0.0, 2.0, 1.0), back, b),
            open(&tri(2.0, 4.0, 1.0), back, b),
            spine(0.0, 4.0, b),
            bar(4.75, 0.75, b),
            spine(0.0, 4.75 + b / 2.0, b),
        ],
        _ => return None,
    })
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------
// The union of overlapping pieces.

/// A part of an edge between two cuts: its ends as whole numbers, and where it truly runs (before the cuts were rounded).
#[derive(Clone, Copy, Debug)]
struct Cut {
    p: IPt,
    q: IPt,
    from: Pt,
    to: Pt,
}

/// Where another edge cuts an edge: the share of the way along (`num` over `den`), and the point, rounded.
#[derive(Clone, Copy, Debug)]
struct Meet {
    num: i128,
    den: i128,
    at: IPt,
}

/// What the cuts of one undirected edge add up to: its weight (+1 for each time it runs from its lesser end to its greater, −1 the other way) and its true
/// middle and direction, from the lesser end to the greater.
#[derive(Clone, Copy, Debug)]
struct Weighed {
    weight: i32,
    mid: Pt,
    dir: Pt,
}

fn cross(a: IPt, b: IPt) -> i128 {
    i128::from(a.0) * i128::from(b.1) - i128::from(a.1) * i128::from(b.0)
}

/// `a / b` rounded to the nearest whole number, half away from zero, for `b` > 0.
fn div_round(a: i128, b: i128) -> i128 {
    let half = b / 2;
    if a >= 0 { (a + half) / b } else { -((-a + half) / b) }
}

/// The points where edge `j` meets the inside of edge `a`-`b`: a crossing (rounded), an end of `j` that lies on it, and the ends of the stretch two edges on
/// one line share.
fn meets_of(a: IPt, b: IPt, c: IPt, e: IPt, out: &mut Vec<Meet>) {
    let (d, f, ac) = ((b.0 - a.0, b.1 - a.1), (e.0 - c.0, e.1 - c.1), (c.0 - a.0, c.1 - a.1));
    let den = cross(d, f);
    if den != 0 {
        let (mut tn, mut un, mut den) = (cross(ac, f), cross(ac, d), den);
        if den < 0 {
            (tn, un, den) = (-tn, -un, -den);
        }
        if tn > 0 && tn < den && un >= 0 && un <= den {
            // The point itself is rounded, not its distance from `a`: the other edge that meets here rounds the same point, whichever end it starts from.
            let x = div_round(i128::from(a.0) * den + i128::from(d.0) * tn, den);
            let y = div_round(i128::from(a.1) * den + i128::from(d.1) * tn, den);
            if let (Ok(x), Ok(y)) = (i64::try_from(x), i64::try_from(y)) {
                out.push(Meet { num: tn, den, at: (x, y) });
            }
        }
    } else if cross(ac, d) == 0 {
        let len2 = i128::from(d.0) * i128::from(d.0) + i128::from(d.1) * i128::from(d.1);
        for p in [c, e] {
            let s = i128::from(p.0 - a.0) * i128::from(d.0) + i128::from(p.1 - a.1) * i128::from(d.1);
            if s > 0 && s < len2 {
                out.push(Meet { num: s, den: len2, at: p });
            }
        }
    }
}

/// The edges of the contours, each corner to the next, with no edge of no length.
fn edges_of(contours: &[Vec<IPt>]) -> Vec<(IPt, IPt)> {
    let mut edges = Vec::new();
    for c in contours {
        let n = c.len();
        for i in 0..n {
            if let (Some(a), Some(b)) = (c.get(i).copied(), c.get((i + 1) % n).copied())
                && a != b
            {
                edges.push((a, b));
            }
        }
    }
    edges
}

/// Every edge cut at each point another edge meets it: the parts, in order along each edge. `steps` counts the pairs of edges tried.
fn cut_edges(edges: &[(IPt, IPt)], steps: &mut u64) -> Vec<Cut> {
    let mut cuts = Vec::new();
    for (i, (a, b)) in edges.iter().copied().enumerate() {
        let mut meets: Vec<Meet> = Vec::new();
        for (j, (c, e)) in edges.iter().copied().enumerate() {
            *steps = steps.saturating_add(1);
            if i != j {
                meets_of(a, b, c, e, &mut meets);
            }
        }
        // In order along the edge: num/den ascending, by cross multiplication.
        meets.sort_by(|x, y| (x.num * y.den).cmp(&(y.num * x.den)));
        let (dx, dy) = ((b.0 - a.0) as f64, (b.1 - a.1) as f64);
        let at = |m: Option<&Meet>| -> (IPt, Pt) {
            match m {
                None => (b, (b.0 as f64, b.1 as f64)),
                Some(m) => {
                    let t = m.num as f64 / m.den as f64;
                    (m.at, (a.0 as f64 + dx * t, a.1 as f64 + dy * t))
                }
            }
        };
        let (mut prev, mut prev_true) = (a, (a.0 as f64, a.1 as f64));
        for m in meets.iter().map(Some).chain(std::iter::once(None)) {
            let (next, next_true) = at(m);
            if next != prev {
                cuts.push(Cut { p: prev, q: next, from: prev_true, to: next_true });
                (prev, prev_true) = (next, next_true);
            }
        }
    }
    cuts
}

/// How many times the contours wind round the point `at` (the pieces, each running positive, covering it).
fn winding(contours: &[Vec<IPt>], at: Pt, steps: &mut u64) -> i32 {
    let mut w = 0;
    for c in contours {
        let n = c.len();
        *steps = steps.saturating_add(n as u64);
        for i in 0..n {
            let (Some(a), Some(b)) = (c.get(i).copied(), c.get((i + 1) % n).copied()) else { continue };
            let (a, b) = ((a.0 as f64, a.1 as f64), (b.0 as f64, b.1 as f64));
            let side = (b.0 - a.0) * (at.1 - a.1) - (b.1 - a.1) * (at.0 - a.0);
            if a.1 <= at.1 {
                if b.1 > at.1 && side > 0.0 {
                    w += 1;
                }
            } else if b.1 <= at.1 && side < 0.0 {
                w -= 1;
            }
        }
    }
    w
}

/// The cuts that are the edge of the union, each from the corner it starts at to the one it ends at, with the inside on its left, or `None` when the winding on
/// the two sides of a cut does not agree with its weight (the geometry is too close to a tie for the count).
fn boundary(contours: &[Vec<IPt>], cuts: &[Cut], steps: &mut u64) -> Option<Vec<(IPt, IPt)>> {
    let mut sums: BTreeMap<(IPt, IPt), Weighed> = BTreeMap::new();
    for cut in cuts {
        let forward = cut.p < cut.q;
        let key = if forward { (cut.p, cut.q) } else { (cut.q, cut.p) };
        let (dx, dy) = (cut.to.0 - cut.from.0, cut.to.1 - cut.from.1);
        let entry = sums.entry(key).or_insert(Weighed {
            weight: 0,
            mid: ((cut.from.0 + cut.to.0) / 2.0, (cut.from.1 + cut.to.1) / 2.0),
            dir: if forward { (dx, dy) } else { (-dx, -dy) },
        });
        entry.weight += if forward { 1 } else { -1 };
    }
    let mut kept = Vec::new();
    for ((lo, hi), w) in sums {
        if w.weight == 0 {
            continue;
        }
        let (start, end, weight, dir) = if w.weight > 0 { (lo, hi, w.weight, w.dir) } else { (hi, lo, -w.weight, (-w.dir.0, -w.dir.1)) };
        // A cut has a length; one with none would give normals that are not numbers, and a winding count of no sides (so no agreement with the weight below).
        let len = dir.0.hypot(dir.1);
        // The left normal is (−dy, dx); the inside of a positive contour is on the left.
        let n = (-dir.1 / len * SAMPLE, dir.0 / len * SAMPLE);
        let left = winding(contours, (w.mid.0 + n.0, w.mid.1 + n.1), steps);
        let right = winding(contours, (w.mid.0 - n.0, w.mid.1 - n.1), steps);
        if left - right != weight {
            return None;
        }
        if right == 0 {
            kept.push((start, end));
        }
    }
    Some(kept)
}

/// The kept edges chained into closed contours: an edge, then any edge that starts where it ends, until the first corner comes round again.
fn chain(kept: &[(IPt, IPt)]) -> Option<Vec<Vec<IPt>>> {
    let mut from: BTreeMap<IPt, Vec<usize>> = BTreeMap::new();
    for (i, (a, _)) in kept.iter().enumerate() {
        from.entry(*a).or_default().push(i);
    }
    let mut used = vec![false; kept.len()];
    let mut out = Vec::new();
    for first in 0..kept.len() {
        if used.get(first).copied().unwrap_or(true) {
            continue;
        }
        let start = kept.get(first)?.0;
        let mut contour = vec![start];
        let mut at = first;
        loop {
            *used.get_mut(at)? = true;
            let end = kept.get(at)?.1;
            if end == start {
                break;
            }
            contour.push(end);
            at = *from.get(&end)?.iter().find(|i| !used.get(**i).copied().unwrap_or(true))?;
        }
        out.push(contour);
    }
    Some(out)
}

/// A contour with its repeated corners, the corners on a straight run and its spikes (a corner that turns straight back) taken out.
fn tidy(mut c: Vec<IPt>) -> Vec<IPt> {
    loop {
        let n = c.len();
        if n < 3 {
            return Vec::new();
        }
        let drop = (0..n).find(|&i| {
            let (a, b, d) = (c.get((i + n - 1) % n).copied(), c.get(i).copied(), c.get((i + 1) % n).copied());
            match (a, b, d) {
                (Some(a), Some(b), Some(d)) => b == a || cross((b.0 - a.0, b.1 - a.1), (d.0 - b.0, d.1 - b.1)) == 0,
                _ => false,
            }
        });
        match drop {
            Some(i) => {
                c.remove(i);
            }
            None => return c,
        }
    }
}

/// Starts a contour at its first corner from the top, then the left, and keeps its direction.
fn from_the_top(mut c: Vec<IPt>) -> Vec<IPt> {
    if let Some(at) = (0..c.len()).min_by_key(|&i| c.get(i).map(|p| (p.1, p.0))) {
        c.rotate_left(at);
    }
    c
}

/// The union of `contours`, every piece running positive on its outer contour and negative on its holes: contours, the holes against the outer ones, in a
/// fixed order. `None` when it is too big ([`MOST_EDGES`]) or the winding count could not tell.
pub(crate) fn unite(contours: &[Vec<IPt>], steps: &mut u64) -> Option<Vec<Vec<IPt>>> {
    let edges = edges_of(contours);
    if edges.len() > MOST_EDGES {
        return None;
    }
    let cuts = cut_edges(&edges, steps);
    let kept = boundary(contours, &cuts, steps)?;
    let mut out: Vec<Vec<IPt>> = chain(&kept)?.into_iter().map(tidy).filter(|c| !c.is_empty()).map(from_the_top).collect();
    out.sort();
    Some(out)
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------
// The marker.

/// The box of a piece: x from, y from, x to, y to.
fn bounds(piece: &Piece) -> (i64, i64, i64, i64) {
    match piece {
        Piece::Disc { cy, r, .. } => (-r, cy - r, *r, cy + r),
        Piece::Poly(contours) => {
            let all = contours.iter().flatten();
            let (mut x0, mut y0, mut x1, mut y1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
            for p in all {
                (x0, y0, x1, y1) = (x0.min(p.0), y0.min(p.1), x1.max(p.0), y1.max(p.1));
            }
            (x0, y0, x1, y1)
        }
    }
}

/// Groups the pieces whose boxes overlap, by more than an edge or a corner, directly or through others: the indices of each group, in order.
fn clusters(pieces: &[Piece]) -> Vec<Vec<usize>> {
    let boxes: Vec<_> = pieces.iter().map(bounds).collect();
    let mut group: Vec<usize> = (0..pieces.len()).collect();
    for i in 0..pieces.len() {
        for j in i + 1..pieces.len() {
            let (Some(a), Some(b)) = (boxes.get(i), boxes.get(j)) else { continue };
            if a.0 < b.2 && b.0 < a.2 && a.1 < b.3 && b.1 < a.3 {
                let (from, to) = (group.get(j).copied().unwrap_or(j), group.get(i).copied().unwrap_or(i));
                for g in group.iter_mut().filter(|g| **g == from) {
                    *g = to;
                }
            }
        }
    }
    let mut out: Vec<Vec<usize>> = Vec::new();
    for leader in 0..pieces.len() {
        let members: Vec<usize> = (0..pieces.len()).filter(|k| group.get(*k) == Some(&leader)).collect();
        if !members.is_empty() {
            out.push(members);
        }
    }
    out
}

/// What a built mark is made of before it is moved to the corner: polygon contours, and the discs.
struct Drawn {
    contours: Vec<Vec<IPt>>,
    discs: Vec<(i64, i64, i64)>,
}

/// The contours and discs of the pieces, the overlapping ones united. `None` when a disc overlaps another piece or a union fails.
fn draw(pieces: &[Piece], steps: &mut u64) -> Option<Drawn> {
    let mut out = Drawn { contours: Vec::new(), discs: Vec::new() };
    for members in clusters(pieces) {
        let group: Vec<&Piece> = members.iter().filter_map(|i| pieces.get(*i)).collect();
        match group.as_slice() {
            [Piece::Disc { cy, r, hole }] => {
                out.discs.push((*cy, *r, 0));
                if *hole > 0 {
                    out.discs.push((*cy, *hole, 1));
                }
            }
            [Piece::Poly(contours)] => out.contours.extend(contours.iter().cloned()),
            many => {
                let mut contours = Vec::new();
                for piece in many {
                    match piece {
                        Piece::Poly(c) => contours.extend(c.iter().cloned()),
                        Piece::Disc { .. } => return None,
                    }
                }
                out.contours.extend(unite(&contours, steps)?);
            }
        }
    }
    Some(out)
}

/// A circle of radius `r` about (`cx`, `cy`) as four cubic curves, from the top, running positive (the way the polygons run), or against that for a hole.
fn circle(cx: i64, cy: i64, r: i64, hole: bool) -> String {
    let k = (0.552_284_749_8 * r as f64).round() as i64;
    let (top, right, bottom, left) = ((cx, cy - r), (cx + r, cy), (cx, cy + r), (cx - r, cy));
    let arc = |c1: IPt, c2: IPt, to: IPt| format!("C {} {} {} {} {} {}", c1.0, c1.1, c2.0, c2.1, to.0, to.1);
    let parts = if hole {
        [
            arc((cx - k, top.1), (left.0, cy - k), left),
            arc((left.0, cy + k), (cx - k, bottom.1), bottom),
            arc((cx + k, bottom.1), (right.0, cy + k), right),
            arc((right.0, cy - k), (cx + k, top.1), top),
        ]
    } else {
        [
            arc((cx + k, top.1), (right.0, cy - k), right),
            arc((right.0, cy + k), (cx + k, bottom.1), bottom),
            arc((cx - k, bottom.1), (left.0, cy + k), left),
            arc((left.0, cy - k), (cx - k, top.1), top),
        ]
    };
    format!("M {} {} {} Z", top.0, top.1, parts.join(" "))
}

/// A contour as `M x y L x y ... Z`.
fn polygon(c: &[IPt], dx: i64, dy: i64) -> String {
    let mut out = String::new();
    for (k, p) in c.iter().enumerate() {
        out.push_str(if k == 0 { "M " } else { " L " });
        out.push_str(&format!("{} {}", p.0 - dx, p.1 - dy));
    }
    out.push_str(" Z");
    out
}

/// The box of all the pieces: `None` when there are none, or none has a corner.
fn box_of_all(pieces: &[Piece]) -> Option<(i64, i64, i64, i64)> {
    let (mut x0, mut y0, mut x1, mut y1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
    for (a, b, c, d) in pieces.iter().map(bounds) {
        (x0, y0, x1, y1) = (x0.min(a), y0.min(b), x1.max(c), y1.max(d));
    }
    // Every piece with a corner narrows both axes, so one test is enough.
    (x0 <= x1).then_some((x0, y0, x1, y1))
}

/// The marker of DiaCraft's filled diamond, the custom end of UML composition: a box of 1000 by 2000 with the tip at the middle of the top side, which is where a program
/// that draws a marker puts the line's end, and the back at the bottom, so that it is twice as long as it is wide, as the master is. One closed path, filled by every rule alike.
pub(crate) fn filled_diamond() -> Built {
    Built { view_box: "0 0 1000 2000".to_owned(), d: "M 500 0 L 1000 1000 L 500 2000 L 0 1000 Z".to_owned() }
}

/// Builds the marker of number `n` at band class `band` (0 for a mark with no band): its view box and path, or `None` for a number that is not 1 to 45, a band
/// class that does not belong to the number, or a union that could not be made. `steps` counts the work.
pub(crate) fn build(n: u32, band: u32, steps: &mut u64) -> Option<Built> {
    if if has_band(n) { !(1..=99).contains(&band) } else { band != 0 } {
        return None;
    }
    let all = pieces(n, f64::from(band) / 100.0)?;
    let drawn = draw(&all, steps)?;
    // The box of the pieces: what a union takes away from a piece is inside the union of the others, so the box is the same.
    let (x0, y0, x1, y1) = box_of_all(&all)?;
    let mut parts: Vec<String> = drawn.contours.iter().map(|c| polygon(c, x0, y0)).collect();
    parts.extend(drawn.discs.iter().map(|(cy, r, hole)| circle(-x0, cy - y0, *r, *hole == 1)));
    let d = parts.join(" ");
    (d.len() <= MOST_PATH_BYTES).then(|| Built { view_box: format!("0 0 {} {}", x1 - x0, y1 - y0), d })
}

#[cfg(test)]
mod tests;
