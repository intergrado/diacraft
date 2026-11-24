//! Plain geometry for the pointer: points in paper inches (y up), polygons and polylines, the eight handles of a box, and snapping.
//!
//! Nothing here knows about the document or about egui. Every function takes finite numbers and returns finite numbers; a
//! hostile (NaN or infinite) point is never inside anything and never near anything.

/// A point of the page in paper inches, y up.
pub type P = (f64, f64);

/// The distance between two points.
pub fn distance(a: P, b: P) -> f64 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

/// The distance from `p` to the segment `a` to `b`.
pub fn distance_to_segment(p: P, a: P, b: P) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    if len2 <= f64::MIN_POSITIVE || !len2.is_finite() {
        return distance(p, a);
    }
    let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0);
    distance(p, (a.0 + t * dx, a.1 + t * dy))
}

/// Whether `p` is inside the polygon `poly` (even-odd rule; the polygon closes itself).
pub fn inside_polygon(poly: &[P], p: P) -> bool {
    if poly.len() < 3 || !(p.0.is_finite() && p.1.is_finite()) {
        return false;
    }
    let mut inside = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let (a, b) = (poly.get(i).copied().unwrap_or(p), poly.get(j).copied().unwrap_or(p));
        if (a.1 > p.1) != (b.1 > p.1) {
            let x = (b.0 - a.0) * (p.1 - a.1) / (b.1 - a.1) + a.0;
            if p.0 < x {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

/// Whether `p` is within `tol` of the polyline `line`. `closed` adds the segment from the last point back to the first.
pub fn near_polyline(line: &[P], closed: bool, p: P, tol: f64) -> bool {
    if !(p.0.is_finite() && p.1.is_finite()) {
        return false;
    }
    if let [only] = line {
        return distance(p, *only) <= tol;
    }
    let near = line.windows(2).any(|w| match (w.first(), w.get(1)) {
        (Some(a), Some(b)) => distance_to_segment(p, *a, *b) <= tol,
        _ => false,
    });
    near || (closed && line.len() > 2 && matches!((line.first(), line.last()), (Some(a), Some(b)) if distance_to_segment(p, *b, *a) <= tol))
}

/// The smallest box that holds every point: lower left and upper right. `None` for no point.
pub fn bounds_of<'a>(points: impl IntoIterator<Item = &'a P>) -> Option<(P, P)> {
    let mut found: Option<(P, P)> = None;
    for p in points {
        if !(p.0.is_finite() && p.1.is_finite()) {
            continue;
        }
        found = Some(match found {
            None => (*p, *p),
            Some((lo, hi)) => ((lo.0.min(p.0), lo.1.min(p.1)), (hi.0.max(p.0), hi.1.max(p.1))),
        });
    }
    found
}

/// The points of `points` that are left when every one that stands within `tolerance` of the line between its neighbours is dropped (Douglas and Peucker): the ends are kept, and a curve
/// that is a straight run stays a run. Iterative, with no more than one pass of the points for each point kept.
pub fn simplified(points: &[P], tolerance: f64) -> Vec<P> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let mut keep = vec![false; points.len()];
    if let Some(first) = keep.first_mut() {
        *first = true;
    }
    if let Some(last) = keep.last_mut() {
        *last = true;
    }
    let mut stack = vec![(0usize, points.len() - 1)];
    while let Some((a, b)) = stack.pop() {
        let (Some(pa), Some(pb)) = (points.get(a), points.get(b)) else { continue };
        let farthest = points
            .iter()
            .enumerate()
            .take(b)
            .skip(a + 1)
            .map(|(i, p)| (i, distance_to_segment(*p, *pa, *pb)))
            .filter(|(_, d)| d.is_finite())
            .fold(None, |best: Option<(usize, f64)>, now| if best.is_none_or(|(_, d)| now.1 > d) { Some(now) } else { best });
        if let Some((i, _)) = farthest.filter(|(_, d)| *d > tolerance) {
            if let Some(slot) = keep.get_mut(i) {
                *slot = true;
            }
            stack.push((a, i));
            stack.push((i, b));
        }
    }
    points.iter().zip(keep).filter(|(_, kept)| *kept).map(|(p, _)| *p).collect()
}

/// The two corners of a rectangle given by any two opposite points, lower left first.
pub fn ordered(a: P, b: P) -> (P, P) {
    ((a.0.min(b.0), a.1.min(b.1)), (a.0.max(b.0), a.1.max(b.1)))
}

/// Whether the box `inner` lies wholly inside the box `outer`.
pub fn box_inside(inner: (P, P), outer: (P, P)) -> bool {
    inner.0.0 >= outer.0.0 && inner.0.1 >= outer.0.1 && inner.1.0 <= outer.1.0 && inner.1.1 <= outer.1.1
}

/// Whether the two boxes share any point.
pub fn overlaps(a: (P, P), b: (P, P)) -> bool {
    a.0.0 <= b.1.0 && b.0.0 <= a.1.0 && a.0.1 <= b.1.1 && b.0.1 <= a.1.1
}

/// The eight handles of a 2-D shape's box, going round it from the upper left.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    NorthWest,
    North,
    NorthEast,
    East,
    SouthEast,
    South,
    SouthWest,
    West,
}

/// Every handle, in drawing order.
pub const HANDLES: [Handle; 8] =
    [Handle::NorthWest, Handle::North, Handle::NorthEast, Handle::East, Handle::SouthEast, Handle::South, Handle::SouthWest, Handle::West];

/// The handles worth showing on the box `lo` to `hi` at `scale` points to an inch, for handles `size` points across: the four corners
/// always, and a side's middle handle only when that side is long enough to hold three handles, so that a shape drawn small does not
/// wear a ring of squares over each other.
pub fn visible_handles(lo: P, hi: P, scale: f64, size: f64) -> Vec<Handle> {
    let (wide, tall) = ((hi.0 - lo.0) * scale, (hi.1 - lo.1) * scale);
    HANDLES
        .into_iter()
        .filter(|h| match h {
            Handle::North | Handle::South => wide >= 3.0 * size,
            Handle::East | Handle::West => tall >= 3.0 * size,
            _ => true,
        })
        .collect()
}

impl Handle {
    /// Where the handle is on the box `lo` to `hi`.
    pub fn at(self, lo: P, hi: P) -> P {
        let (mx, my) = ((lo.0 + hi.0) / 2.0, (lo.1 + hi.1) / 2.0);
        match self {
            Handle::NorthWest => (lo.0, hi.1),
            Handle::North => (mx, hi.1),
            Handle::NorthEast => (hi.0, hi.1),
            Handle::East => (hi.0, my),
            Handle::SouthEast => (hi.0, lo.1),
            Handle::South => (mx, lo.1),
            Handle::SouthWest => (lo.0, lo.1),
            Handle::West => (lo.0, my),
        }
    }

    /// Which edges the handle moves: (x: -1 the left edge, 1 the right, 0 neither; y: -1 the bottom edge, 1 the top).
    pub fn edges(self) -> (i8, i8) {
        match self {
            Handle::NorthWest => (-1, 1),
            Handle::North => (0, 1),
            Handle::NorthEast => (1, 1),
            Handle::East => (1, 0),
            Handle::SouthEast => (1, -1),
            Handle::South => (0, -1),
            Handle::SouthWest => (-1, -1),
            Handle::West => (-1, 0),
        }
    }

    /// Whether the handle is at a corner.
    pub fn is_corner(self) -> bool {
        let (x, y) = self.edges();
        x != 0 && y != 0
    }
}

/// The box `lo` to `hi` after the handle is dragged to `to`: the edges it moves follow `to`, the others stay. With `keep_aspect` a corner
/// keeps the box's proportions (it follows the pointer's larger relative move). A box never gets smaller than `least` on a side, and
/// never turns inside out: past the opposite edge it stops there.
pub fn resized(lo: P, hi: P, handle: Handle, to: P, keep_aspect: bool, least: f64) -> (P, P) {
    let (ex, ey) = handle.edges();
    let (mut l, mut b, mut r, mut t) = (lo.0, lo.1, hi.0, hi.1);
    let (w0, h0) = ((hi.0 - lo.0).max(least), (hi.1 - lo.1).max(least));
    let mut to = to;
    if keep_aspect && handle.is_corner() {
        // The scale the pointer asks for on each axis; the larger one rules, and the other side follows it.
        let sx = if ex > 0 { (to.0 - lo.0) / w0 } else { (hi.0 - to.0) / w0 };
        let sy = if ey > 0 { (to.1 - lo.1) / h0 } else { (hi.1 - to.1) / h0 };
        let s = if sx.abs() >= sy.abs() { sx } else { sy };
        let s = s.max(least / w0.min(h0));
        to = (if ex > 0 { lo.0 + w0 * s } else { hi.0 - w0 * s }, if ey > 0 { lo.1 + h0 * s } else { hi.1 - h0 * s });
    }
    match ex {
        1 => r = to.0.max(l + least),
        -1 => l = to.0.min(r - least),
        _ => {}
    }
    match ey {
        1 => t = to.1.max(b + least),
        -1 => b = to.1.min(t - least),
        _ => {}
    }
    ((l, b), (r, t))
}

/// The box `lo` to `hi` after the handle is dragged to `to`, **in proportion to the box now** (a corner keeps the proportions with no key, a side with
/// Shift): a corner follows the pointer's larger relative move with the opposite corner staying (as [`resized`] with `keep_aspect`), and a side moves as the pointer says with
/// the opposite side staying and the other axis growing about its middle by the same share. A box never gets smaller than `least` on a side. For the window's own box, where
/// the engine's snap is not asked to keep the proportions (Alt held, the Snap switch off, a page the engine is still reading).
pub fn resized_in_proportion(lo: P, hi: P, handle: Handle, to: P, least: f64) -> (P, P) {
    if handle.is_corner() {
        return resized(lo, hi, handle, to, true, least);
    }
    let ((l, b), (r, t)) = resized(lo, hi, handle, to, false, least);
    let (ex, ey) = handle.edges();
    let (w0, h0) = ((hi.0 - lo.0).max(least), (hi.1 - lo.1).max(least));
    if ex != 0 {
        // The width as the pointer made it over the width now: the height is that share of the height now, about the middle of it.
        let share = (r - l) / w0;
        let h = (h0 * share).max(least);
        let middle = (lo.1 + hi.1) / 2.0;
        return ((l, middle - h / 2.0), (r, middle + h / 2.0));
    }
    if ey != 0 {
        let share = (t - b) / h0;
        let w = (w0 * share).max(least);
        let middle = (lo.0 + hi.0) / 2.0;
        return ((middle - w / 2.0, b), (middle + w / 2.0, t));
    }
    ((l, b), (r, t))
}

/// A value moved to the nearest line of a grid: lines at `origin + k * pitch`. A pitch that is not above 0 leaves it alone.
pub fn snap(value: f64, pitch: f64, origin: f64) -> f64 {
    if !(pitch > 0.0 && pitch.is_finite() && value.is_finite() && origin.is_finite()) {
        return value;
    }
    origin + ((value - origin) / pitch).round() * pitch
}

/// A shape's own frame laid on the page, in paper inches: where the corner at (0, 0) of its own frame is on the page, the page directions of
/// its two axes (unit vectors, which a turn and a flip carry), and its size along them. A shape that is neither turned nor flipped has its
/// lower-left corner for `origin` and the page's own axes. **A handle drag works in this frame** (`docs/editing-commands.md`):
/// `shape.resize` takes the page place of the corner at (0, 0) and the width and height along the axes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Basis {
    pub origin: P,
    pub ex: P,
    pub ey: P,
    pub w: f64,
    pub h: f64,
}

impl Basis {
    /// A point of the page in the frame: how far along each axis from `origin`.
    pub fn to_local(&self, p: P) -> P {
        let d = (p.0 - self.origin.0, p.1 - self.origin.1);
        (d.0 * self.ex.0 + d.1 * self.ex.1, d.0 * self.ey.0 + d.1 * self.ey.1)
    }

    /// A point of the frame on the page.
    pub fn to_page(&self, q: P) -> P {
        (self.origin.0 + q.0 * self.ex.0 + q.1 * self.ey.0, self.origin.1 + q.0 * self.ex.1 + q.1 * self.ey.1)
    }

    /// The box of the shape in its own frame: (0, 0) to (width, height).
    pub fn local_box(&self) -> (P, P) {
        ((0.0, 0.0), (self.w, self.h))
    }

    /// The four corners of a box of the frame on the page, going round it from its lower left.
    pub fn corners_of(&self, lo: P, hi: P) -> [P; 4] {
        [self.to_page(lo), self.to_page((hi.0, lo.1)), self.to_page(hi), self.to_page((lo.0, hi.1))]
    }

    /// The frame of a shape from its four corners on the page, in the order of its own frame (0,0), (w,0), (w,h), (0,h). `None` for a box
    /// with no width or no height, which has no frame to work in.
    pub fn of_corners(corners: &[P; 4]) -> Option<Basis> {
        let [c0, c1, _, c3] = *corners;
        let (dx, dy) = ((c1.0 - c0.0, c1.1 - c0.1), (c3.0 - c0.0, c3.1 - c0.1));
        let (w, h) = (dx.0.hypot(dx.1), dy.0.hypot(dy.1));
        if !(w > 1.0e-9 && h > 1.0e-9 && w.is_finite() && h.is_finite()) {
            return None;
        }
        Some(Basis { origin: c0, ex: (dx.0 / w, dx.1 / w), ey: (dy.0 / h, dy.1 / h), w, h })
    }
}

/// `p` turned about `centre` by `degrees` counterclockwise (y up).
pub fn turned_about(p: P, centre: P, degrees: f64) -> P {
    let (s, c) = degrees.to_radians().sin_cos();
    let d = (p.0 - centre.0, p.1 - centre.1);
    (centre.0 + d.0 * c - d.1 * s, centre.1 + d.0 * s + d.1 * c)
}

/// An angle in degrees kept from just over -180 up to 180, the way the engine keeps `Angle`.
pub fn normal_degrees(degrees: f64) -> f64 {
    if !degrees.is_finite() {
        return 0.0;
    }
    let a = degrees.rem_euclid(360.0);
    if a > 180.0 { a - 360.0 } else { a }
}

/// How many straight pieces an arc is cut into.
const ARC_PIECES: usize = 16;

/// The points of an elliptical arc from `p0` to `p1` through `via`, after `p0` and ending at `p1`, for a shape's outline.
///
/// The ellipse has its axes along x and y, and `ratio` is the length of its x axis over its y axis (`EllipticalArcTo`'s `D` with its angle
/// `C` 0), so y is scaled by `ratio` to make it a circle, the circle through the three points is walked, and
/// y is scaled back. Three points on a line (or numbers that are not finite) give the straight piece to `p1`: a hit test and a small icon
/// need no more.
pub fn elliptical_arc(p0: P, via: P, p1: P, ratio: f64) -> Vec<P> {
    let ratio = if ratio.is_finite() && ratio > 1.0e-6 && ratio < 1.0e6 { ratio } else { 1.0 };
    let squash = |p: P| (p.0, p.1 * ratio);
    let (a, b, c) = (squash(p0), squash(via), squash(p1));
    let d = 2.0 * (a.0 * (b.1 - c.1) + b.0 * (c.1 - a.1) + c.0 * (a.1 - b.1));
    let (a2, b2, c2) = (a.0 * a.0 + a.1 * a.1, b.0 * b.0 + b.1 * b.1, c.0 * c.0 + c.1 * c.1);
    let centre = ((a2 * (b.1 - c.1) + b2 * (c.1 - a.1) + c2 * (a.1 - b.1)) / d, (a2 * (c.0 - b.0) + b2 * (a.0 - c.0) + c2 * (b.0 - a.0)) / d);
    let radius = distance(a, centre);
    if !(d.abs() > 1.0e-12 && centre.0.is_finite() && centre.1.is_finite() && radius.is_finite() && radius < 1.0e9) {
        return vec![p1];
    }
    let angle = |p: P| (p.1 - centre.1).atan2(p.0 - centre.0);
    let (start, through, end) = (angle(a), angle(b), angle(c));
    let tau = std::f64::consts::TAU;
    // Counterclockwise from the start to the end, and the way round that passes the middle point.
    let ccw = (end - start).rem_euclid(tau);
    let sweep = if (through - start).rem_euclid(tau) <= ccw { ccw } else { ccw - tau };
    let mut points: Vec<P> = (1..ARC_PIECES)
        .map(|i| {
            let at = start + sweep * (i as f64 / ARC_PIECES as f64);
            (centre.0 + radius * at.cos(), (centre.1 + radius * at.sin()) / ratio)
        })
        .collect();
    points.push(p1);
    points
}

/// The triangles that fill a simple polygon (its vertices in order, the closing vertex not repeated or repeated), as indexes into `poly`, by
/// cutting off ears. A polygon that crosses itself, or has no area, is filled as well as it can be: what is left when no ear is found is
/// cut as a fan. Fewer than three vertices give none.
pub fn triangulate(poly: &[P]) -> Vec<[usize; 3]> {
    let mut n = poly.len();
    if n > 1 && poly.first() == poly.last() {
        n -= 1;
    }
    if n < 3 || poly.iter().take(n).any(|p| !(p.0.is_finite() && p.1.is_finite())) {
        return Vec::new();
    }
    let at = |i: usize| poly.get(i).copied().unwrap_or((0.0, 0.0));
    let area2: f64 = (0..n).map(|i| at(i).0 * at((i + 1) % n).1 - at((i + 1) % n).0 * at(i).1).sum();
    let mut ring: Vec<usize> = if area2 >= 0.0 { (0..n).collect() } else { (0..n).rev().collect() };
    let cross = |a: P, b: P, c: P| (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
    let mut out = Vec::with_capacity(n.saturating_sub(2));
    while ring.len() > 3 {
        let m = ring.len();
        let ear = (0..m).find(|&k| {
            let (i, j, l) =
                (ring.get((k + m - 1) % m).copied().unwrap_or(0), ring.get(k).copied().unwrap_or(0), ring.get((k + 1) % m).copied().unwrap_or(0));
            let (a, b, c) = (at(i), at(j), at(l));
            if cross(a, b, c) <= 1.0e-12 {
                return false;
            }
            // No other vertex of the ring lies inside the triangle.
            !ring.iter().filter(|&&v| v != i && v != j && v != l).any(|&v| {
                let p = at(v);
                cross(a, b, p) >= 0.0 && cross(b, c, p) >= 0.0 && cross(c, a, p) >= 0.0
            })
        });
        let Some(k) = ear else {
            // No ear (a polygon that crosses itself, or one with no area): a fan from the first vertex fills the rest.
            let first = ring.first().copied().unwrap_or(0);
            for w in ring.windows(2).skip(1) {
                if let [b, c] = w {
                    out.push([first, *b, *c]);
                }
            }
            return out;
        };
        let (i, j, l) =
            (ring.get((k + m - 1) % m).copied().unwrap_or(0), ring.get(k).copied().unwrap_or(0), ring.get((k + 1) % m).copied().unwrap_or(0));
        out.push([i, j, l]);
        ring.remove(k);
    }
    if let [a, b, c] = ring[..] {
        out.push([a, b, c]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SQUARE: [P; 4] = [(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0)];

    #[test]
    fn a_point_is_inside_a_polygon_by_the_even_odd_rule() {
        assert!(inside_polygon(&SQUARE, (1.0, 1.0)));
        assert!(!inside_polygon(&SQUARE, (3.0, 1.0)));
        // A diamond: the corner of its box is outside it.
        let diamond = [(1.0, 0.0), (2.0, 1.0), (1.0, 2.0), (0.0, 1.0)];
        assert!(inside_polygon(&diamond, (1.0, 1.0)));
        assert!(!inside_polygon(&diamond, (0.1, 0.1)));
        assert!(!inside_polygon(&SQUARE[..2], (1.0, 0.0)), "two points enclose nothing");
        assert!(!inside_polygon(&SQUARE, (f64::NAN, 1.0)));
    }

    #[test]
    fn distance_to_a_polyline_is_to_its_nearest_segment() {
        let line = [(0.0, 0.0), (4.0, 0.0), (4.0, 3.0)];
        assert!(near_polyline(&line, false, (2.0, 0.05), 0.1));
        assert!(near_polyline(&line, false, (4.05, 2.0), 0.1));
        assert!(!near_polyline(&line, false, (2.0, 1.0), 0.1));
        assert!(!near_polyline(&line, false, (0.0, 3.0), 0.1), "open: the last point is not joined to the first");
        assert!(near_polyline(&line, true, (2.0, 1.5), 1.6), "closed: it is");
        assert!(!near_polyline(&line, false, (f64::INFINITY, 0.0), 1.0e9));
        assert!(near_polyline(&[(1.0, 1.0)], false, (1.0, 1.05), 0.1));
    }

    #[test]
    fn a_corner_handle_moves_two_edges_and_an_edge_handle_one() {
        let (lo, hi) = ((1.0, 1.0), (3.0, 2.0));
        assert_eq!(resized(lo, hi, Handle::East, (4.0, 9.0), false, 0.1), ((1.0, 1.0), (4.0, 2.0)));
        assert_eq!(resized(lo, hi, Handle::NorthWest, (0.0, 3.0), false, 0.1), ((0.0, 1.0), (3.0, 3.0)));
        assert_eq!(resized(lo, hi, Handle::South, (9.0, 0.5), false, 0.1), ((1.0, 0.5), (3.0, 2.0)));
    }

    #[test]
    fn a_resize_in_proportion_keeps_the_ratio_from_a_corner_and_grows_the_other_axis_about_its_middle_from_a_side() {
        let (lo, hi) = ((1.0, 1.0), (4.0, 3.0));
        // A corner follows the pointer's larger relative move, the opposite corner staying: 3 by 2 to 6 by 4.
        assert_eq!(resized_in_proportion(lo, hi, Handle::NorthEast, (7.0, 3.5), 0.1), ((1.0, 1.0), (7.0, 5.0)));
        // The south-west corner to (0, 0.5): 4 over 3 and 2.5 over 2, the larger share rules, so the width is 4 and the height 2 2/3, the north-east corner staying.
        let ((l, b), (r, t)) = resized_in_proportion(lo, hi, Handle::SouthWest, (0.0, 0.5), 0.1);
        assert!(l == 0.0 && r == 4.0 && t == 3.0 && (b - (3.0 - 8.0 / 3.0)).abs() < 1e-12, "{l} {b} {r} {t}");
        // The east side to x = 7: 6 wide, so 4 high about the middle (y = 2), and the west side stays.
        assert_eq!(resized_in_proportion(lo, hi, Handle::East, (7.0, 9.0), 0.1), ((1.0, 0.0), (7.0, 4.0)));
        // The north side up to y = 5: 4 high, so 6 wide about the middle (x = 2.5), and the south side stays.
        assert_eq!(resized_in_proportion(lo, hi, Handle::North, (9.0, 5.0), 0.1), ((-0.5, 1.0), (5.5, 5.0)));
        // The west side taken to the right of the east: it stops at the least width, the other axis is at least that too, about its middle.
        let ((l, b), (r, t)) = resized_in_proportion(lo, hi, Handle::West, (9.0, 0.0), 0.1);
        assert!(
            (r - 4.0).abs() < 1e-12 && (r - l - 0.1).abs() < 1e-12 && (t - b - 0.1).abs() < 1e-12 && ((b + t) / 2.0 - 2.0).abs() < 1e-12,
            "{l} {b} {r} {t}"
        );
    }

    #[test]
    fn a_small_box_shows_its_corners_only() {
        let (lo, hi) = ((0.0, 0.0), (0.3, 0.2));
        assert_eq!(visible_handles(lo, hi, 60.0, 7.0).len(), 4, "18 by 12 points: corners only");
        assert_eq!(visible_handles(lo, (1.5, 1.0), 60.0, 7.0).len(), 8);
        // Wide and short: the north and south middles, not the east and west.
        let wide = visible_handles(lo, (2.0, 0.2), 60.0, 7.0);
        assert!(wide.contains(&Handle::North) && !wide.contains(&Handle::East));
    }

    #[test]
    fn a_box_does_not_turn_inside_out() {
        let (lo, hi) = ((1.0, 1.0), (3.0, 2.0));
        let (nlo, nhi) = resized(lo, hi, Handle::East, (0.0, 1.5), false, 0.25);
        assert_eq!((nlo, nhi), ((1.0, 1.0), (1.25, 2.0)), "it stops at the least size past the opposite edge");
        let (nlo, nhi) = resized(lo, hi, Handle::SouthWest, (5.0, 5.0), false, 0.25);
        assert!(nlo.0 <= nhi.0 - 0.25 + 1e-12 && nlo.1 <= nhi.1 - 0.25 + 1e-12, "{nlo:?} {nhi:?}");
    }

    #[test]
    fn shift_on_a_corner_keeps_the_proportions() {
        let (lo, hi) = ((0.0, 0.0), (2.0, 1.0));
        let (nlo, nhi) = resized(lo, hi, Handle::NorthEast, (4.0, 1.2), true, 0.1);
        let (w, h) = (nhi.0 - nlo.0, nhi.1 - nlo.1);
        assert!((w / h - 2.0).abs() < 1e-9, "{w} by {h}");
        assert_eq!(nlo, (0.0, 0.0), "the opposite corner stays");
        assert!((w - 4.0).abs() < 1e-9, "the larger relative move rules: {w}");
        // An edge handle ignores it.
        assert_eq!(resized(lo, hi, Handle::East, (4.0, 7.0), true, 0.1), ((0.0, 0.0), (4.0, 1.0)));
    }

    #[test]
    fn snapping_rounds_to_the_nearest_line_of_the_grid() {
        assert_eq!(snap(1.2, 0.5, 0.0), 1.0);
        assert_eq!(snap(1.3, 0.5, 0.0), 1.5);
        assert_eq!(snap(1.3, 0.5, 0.1), 1.1);
        assert_eq!(snap(1.3, 0.0, 0.0), 1.3, "no pitch: no snap");
        assert!(snap(f64::NAN, 0.5, 0.0).is_nan());
    }

    #[test]
    fn bounds_skip_hostile_points() {
        assert_eq!(bounds_of(&[(1.0, 2.0), (f64::NAN, 0.0), (3.0, -1.0)]), Some(((1.0, -1.0), (3.0, 2.0))));
        assert_eq!(bounds_of(&[]), None);
        assert_eq!(ordered((3.0, 1.0), (1.0, 4.0)), ((1.0, 1.0), (3.0, 4.0)));
        assert!(box_inside(((1.0, 1.0), (2.0, 2.0)), ((0.0, 0.0), (3.0, 3.0))));
        assert!(!box_inside(((1.0, 1.0), (4.0, 2.0)), ((0.0, 0.0), (3.0, 3.0))));
    }

    #[test]
    fn an_arc_through_three_points_stays_on_its_circle_and_ends_where_it_should() {
        // A half circle of radius 1 about (1, 0), from the left to the right through the top.
        let points = elliptical_arc((0.0, 0.0), (1.0, 1.0), (2.0, 0.0), 1.0);
        assert_eq!(points.last().copied(), Some((2.0, 0.0)));
        assert!(points.iter().all(|p| (distance(*p, (1.0, 0.0)) - 1.0).abs() < 1e-9), "{points:?}");
        assert!(points.iter().all(|p| p.1 >= -1e-9), "it goes over the top, the way the middle point says: {points:?}");
        // The same through the bottom.
        let under = elliptical_arc((0.0, 0.0), (1.0, -1.0), (2.0, 0.0), 1.0);
        assert!(under.iter().all(|p| p.1 <= 1e-9), "{under:?}");
        // An ellipse twice as wide as tall (x over y is 2): a half-ellipse from (0, 0) to (4, 0) through (2, 1).
        let wide = elliptical_arc((0.0, 0.0), (2.0, 1.0), (4.0, 0.0), 2.0);
        assert!(
            wide.iter()
                .all(|p| ((p.0 - 2.0) / 2.0).powi(2) + p.1.powi(2) - 1.0 < 1e-9 && (((p.0 - 2.0) / 2.0).powi(2) + p.1.powi(2) - 1.0).abs() < 1e-9),
            "{wide:?}"
        );
        // Three points on a line, and hostile numbers: the straight piece.
        assert_eq!(elliptical_arc((0.0, 0.0), (1.0, 0.0), (2.0, 0.0), 1.0), vec![(2.0, 0.0)]);
        assert_eq!(elliptical_arc((0.0, 0.0), (f64::NAN, 1.0), (2.0, 0.0), 1.0), vec![(2.0, 0.0)]);
        assert_eq!(elliptical_arc((0.0, 0.0), (1.0, 1.0), (2.0, 0.0), f64::NAN).len(), 16, "a ratio that is not a number is a circle");
    }

    #[test]
    fn a_polygon_is_cut_into_triangles_that_cover_its_area_once() {
        let area = |poly: &[P], tris: &[[usize; 3]]| -> f64 {
            tris.iter()
                .map(|t| {
                    let (a, b, c) = (poly[t[0]], poly[t[1]], poly[t[2]]);
                    ((b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)).abs() / 2.0
                })
                .sum()
        };
        assert_eq!(triangulate(&SQUARE).len(), 2);
        assert!((area(&SQUARE, &triangulate(&SQUARE)) - 4.0).abs() < 1e-9);
        // Clockwise, and with the closing vertex repeated: the same.
        let clockwise: Vec<P> = SQUARE.iter().rev().copied().chain(std::iter::once(SQUARE[3])).collect();
        assert!((area(&clockwise, &triangulate(&clockwise)) - 4.0).abs() < 1e-9);
        // An arrowhead (concave): 3 by 2 with a notch of 1.5 square inches, area 4.5.
        let notched = [(0.0, 0.0), (3.0, 0.0), (3.0, 2.0), (1.5, 1.0), (0.0, 2.0)];
        let tris = triangulate(&notched);
        assert_eq!(tris.len(), 3);
        assert!((area(&notched, &tris) - 4.5).abs() < 1e-9, "the notch is not filled: {tris:?}");
        // Nothing to fill, and hostile points.
        assert!(triangulate(&SQUARE[..2]).is_empty());
        assert!(triangulate(&[(0.0, 0.0), (1.0, f64::NAN), (2.0, 0.0)]).is_empty());
        // A line folded on itself has no area and still ends.
        let _ = triangulate(&[(0.0, 0.0), (1.0, 0.0), (2.0, 0.0), (1.0, 0.0)]);
    }
}
