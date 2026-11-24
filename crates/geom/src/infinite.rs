//! The unbounded line of an `InfiniteLine` row.

use kurbo::{Affine, Line, Point, Rect};

/// An unbounded straight line through two points. A renderer cannot draw it, and no source says what bounds it; the project rule is to
/// clip it to what is being drawn ([`InfiniteLine::clip`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InfiniteLine {
    pub p0: Point,
    pub p1: Point,
}

impl InfiniteLine {
    /// The same line after a transform (the shape's frame to the page, say): a line stays a line, so the two points carry it.
    pub fn transform(self, affine: Affine) -> InfiniteLine {
        InfiniteLine { p0: affine * self.p0, p1: affine * self.p1 }
    }

    /// The part of the line inside `rect`, or `None` if it misses it. Two equal points make no line, and points or a window that are not finite numbers give
    /// `None`. Two distinct finite points make a line, whatever the size of their difference or of the window's size over it: a line is not lost to a number that does
    /// not fit. Two limits remain, both in rounding and neither in a size. A line that crosses the window by less than about 1E-10 of the largest of the numbers given may be missed:
    /// the numbers place it to 1E-16 of that largest number, so a chord that short is lost in them. And where the difference of two coordinates passes
    /// the largest number, the direction is its half, and half of an odd number of the smallest numbers (5E-324) is rounded to an even one, so a line that leaves an axis by 3 of them
    /// is taken at 2 or 4, which can lose or widen a chord of any length (the old code gave no line at all where a difference passes the largest number).
    ///
    /// The ends are in the window and each is on one of its edges, exactly; they are on the line to within a rounding of the largest of the numbers given, and run from the
    /// side of `p0` to the side of `p1`. How it avoids a number that is too large:
    ///
    /// - The direction is the difference of the points, or half of it if the difference passes the largest number, and is then scaled by a power of two, which does not round,
    ///   so that its larger part is in [1, 2), or higher where that would turn the smaller part to a subnormal number or to 0 (a line leaves an axis by that little, and a window
    ///   off the axis is still crossed: [`scaled_to_unit`]). A distance over a step of at least 1 does not pass the largest number, and the smaller part's, which can, is then
    ///   infinite, far past anything the larger part reaches in the window, which orders it right.
    /// - How far along the direction an edge of the window is, from the point the line is worked from, is the difference of the two at full size where that is a number, divided
    ///   by the direction ([`along`]): a position of a few of the smallest numbers (5E-324 each) is not turned to 0 by a scale, which lost the line or cut it to a wrong segment in
    ///   the first version of the fix. Only where the difference passes the largest number are the two taken at a quarter of their size, which is when any of the numbers is over a
    ///   quarter of the largest, so that no distance between two of them passes it. An end's other coordinate is carried at that scale too, to within the rounding of a few of the
    ///   smallest numbers.
    /// - The line is worked from whichever of the two points is nearer the window (the first when they are as near), so that a point in the window is on the answer whatever
    ///   the other is, and a far point gives only the direction. A window's edge is an end's own coordinate, and the other is carried along the direction from that point.
    pub fn clip(self, rect: Rect) -> Option<Line> {
        if !(self.p0.is_finite() && self.p1.is_finite() && rect.is_finite()) {
            return None;
        }
        if self.p0 == self.p1 {
            return None;
        }
        // (`abs` ignores a number that is not a number, so the window is tested before it.)
        let window = rect.abs();
        let (dx, dy) = way(self.p0, self.p1);
        let (step_x, step_y) = scaled_to_unit(dx, dy)?;
        let largest =
            [self.p0.x, self.p0.y, self.p1.x, self.p1.y, window.x0, window.y0, window.x1, window.y1].iter().fold(0.0f64, |most, n| most.max(n.abs()));
        let (down, up) = if largest > f64::MAX / 4.0 { (0.25, 4.0) } else { (1.0, 1.0) };
        let origin = nearer(self.p0, self.p1, window);
        // How far along the direction, from the origin, the line enters and leaves the window, and on which edge.
        let (mut from, mut to) = (Reach { at: f64::NEG_INFINITY, edge: None }, Reach { at: f64::INFINITY, edge: None });
        for (on_x, place, step, low, high) in [(true, origin.x, step_x, window.x0, window.x1), (false, origin.y, step_y, window.y0, window.y1)] {
            if step == 0.0 {
                if place < low || place > high {
                    return None;
                }
            } else {
                let (at_low, at_high) = (along(low, place, step, down), along(high, place, step, down));
                let (enter, leave) = if step > 0.0 { ((at_low, low), (at_high, high)) } else { ((at_high, high), (at_low, low)) };
                if enter.0 > from.at {
                    from = Reach { at: enter.0, edge: Some((on_x, enter.1)) };
                }
                if leave.0 < to.at {
                    to = Reach { at: leave.0, edge: Some((on_x, leave.1)) };
                }
            }
        }
        if from.at > to.at {
            return None;
        }
        // An end has one coordinate that is its edge's, exactly, and the other carried along the direction, and kept in the window against a last rounding.
        let end = |reach: Reach| {
            reach.edge.map(|(on_x, edge)| {
                if on_x {
                    Point::new(edge, ((origin.y * down + step_y * reach.at) * up).max(window.y0).min(window.y1))
                } else {
                    Point::new(((origin.x * down + step_x * reach.at) * up).max(window.x0).min(window.x1), edge)
                }
            })
        };
        let line = Line::new(end(from)?, end(to)?);
        (line.p0.is_finite() && line.p1.is_finite()).then_some(line)
    }
}

/// Where along the direction from the origin the line meets the window: how far, and the edge it is on (the edge is on x if `true`, and its place).
#[derive(Clone, Copy)]
struct Reach {
    at: f64,
    edge: Option<(bool, f64)>,
}

/// How far along the direction `step`, at the scale `down`, the edge `edge` is from the place `place`. The difference is taken at full size where it is a number, and then divided and
/// scaled: scaling the two numbers first would turn a subnormal one (a unit of 5E-324 is 0 at a quarter), and a line a few units from an edge of the window would be lost or cut where it
/// is not. Where the quotient passes the largest number at full size it is taken from the scaled difference, which is not small there (a step is not under 2^-1074, so the
/// difference is over 2^-50), so that no unit is lost; the smaller part of a direction can pass the largest number at full size and not at the scale, and the larger part's
/// distances are up to half of it at the scale, so that an infinite one would be put after them wrongly. Only where the difference itself passes the largest number
/// is it taken from the numbers at the scale (`down` is a quarter there, and the difference of two numbers at a quarter of their size is a number), as the pair is then of a size where a unit does not matter.
pub(crate) fn along(edge: f64, place: f64, step: f64, down: f64) -> f64 {
    let whole = edge - place;
    if !whole.is_finite() {
        return (edge * down - place * down) / step;
    }
    let far = whole / step;
    if far.is_finite() { far * down } else { whole * down / step }
}

/// The way from `a` to `b` as a pair of numbers: their difference, or half of it where the difference passes the largest number. A half of a coordinate is
/// exact, but for the last bit of a subnormal one, and the difference of two halves cannot pass the largest number. Only the way matters, not the length.
pub(crate) fn way(a: Point, b: Point) -> (f64, f64) {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    if dx.is_finite() && dy.is_finite() { (dx, dy) } else { (b.x * 0.5 - a.x * 0.5, b.y * 0.5 - a.y * 0.5) }
}

/// Whichever of the two points is nearer the window, the first if they are as near (the line is worked from it). How near is the largest distance along either axis, 0 only
/// inside the window (a difference of two numbers is 0 only if they are equal, subnormal ones too), and infinite if it passes the largest number, which still puts it after any point
/// that is nearer. `window` is in order (`Rect::abs`).
pub(crate) fn nearer(a: Point, b: Point, window: Rect) -> Point {
    let apart = |p: Point| (window.x0 - p.x).max(p.x - window.x1).max(window.y0 - p.y).max(p.y - window.y1).max(0.0);
    if apart(b) < apart(a) { b } else { a }
}

/// The pair of a [`way`] times a power of two that puts the larger of the two in [1, 2), or `None` if a power of two is not a number (it never is, for a way). A way is not zero and
/// not infinite: two distinct finite points differ by a number that is not 0 (a difference of two numbers is 0 only if they are equal, a subnormal one too), or the half of a
/// difference that passes the largest number, which is far from 0, and a half does not pass it. A power of two changes only the exponent, so the way is the same to the last bit.
/// That puts the smaller one under 2^-1022 (and to nothing, if it is under 2^-1075) when it is less than 2^-1022 of the larger, and a line that leaves an axis by that little would be
/// taken along it, and lost to a window off the axis it was on. So then the scale is raised to keep the smaller one a normal number, as far as the larger can go up to the largest
/// number: the larger is then over 1, which is all that a quotient by it not passing the largest number needs.
pub(crate) fn scaled_to_unit(x: f64, y: f64) -> Option<(f64, f64)> {
    let shift = unit_shift(x.abs().max(y.abs()), x.abs().min(y.abs()));
    // 2^1074 is past the largest number, so the power is applied in two steps of a size a number holds.
    let first = shift / 2;
    let (a, b) = (power_of_two(first)?, power_of_two(shift - first)?);
    Some((x * a * b, y * a * b))
}

/// The power of two that [`scaled_to_unit`] scales by, for the sizes of the larger and the smaller part (`larger` over 0, `smaller` from 0 to `larger`): the one that
/// puts `larger` in [1, 2), raised where it would put a `smaller` that is not 0 under 2^-1022, but no higher than puts `larger` under the largest number.
pub(crate) fn unit_shift(larger: f64, smaller: f64) -> i32 {
    let top = exponent(larger);
    let mut shift = -top;
    if smaller > 0.0 {
        shift = shift.max((-1022 - exponent(smaller)).min(1023 - top));
    }
    shift
}

/// The `e` for which 2^e <= `x` < 2^(e + 1), for a finite `x` over 0, a subnormal one too.
pub(crate) fn exponent(x: f64) -> i32 {
    let bits = x.to_bits();
    let field = ((bits >> 52) & 0x7ff) as i32;
    if field == 0 {
        // A subnormal number is its 52 fraction bits in units of 2^-1074.
        -1011 - (bits & ((1 << 52) - 1)).leading_zeros() as i32
    } else {
        field - 1023
    }
}

/// 2^`power` for the powers a normal number holds, and `None` for the others.
pub(crate) fn power_of_two(power: i32) -> Option<f64> {
    (-1022..=1023).contains(&power).then(|| f64::from_bits(((power + 1023) as u64) << 52))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The values of a grid of points, from the smallest number to the largest, with both zeros, a unit of the subnormal numbers, and the numbers a way is halved at.
    const GRID: [f64; 17] = [
        0.0,
        -0.0,
        5e-324,
        -5e-324,
        1e-323,
        f64::MIN_POSITIVE,
        -f64::MIN_POSITIVE,
        1.0,
        -1.0,
        1e154,
        -1e154,
        f64::MAX / 4.0,
        f64::MIN / 4.0,
        f64::MAX / 2.0,
        f64::MIN / 2.0,
        f64::MAX,
        f64::MIN,
    ];

    /// The premise of `scaled_to_unit` (it does not test for a way that is 0 or not finite): the way between two distinct finite points is finite and not (0, 0), over every pair of the
    /// points whose parts are of `GRID` (289 points, about 83,000 pairs), and a way is scaled to a pair with a larger part of 1 to the largest number, which is finite and keeps its signs.
    #[test]
    fn the_way_between_two_distinct_points_is_finite_and_not_nothing() {
        let points: Vec<Point> = GRID.iter().flat_map(|&x| GRID.iter().map(move |&y| Point::new(x, y))).collect();
        let mut pairs = 0u32;
        for &a in &points {
            for &b in &points {
                if a == b {
                    continue;
                }
                let (dx, dy) = way(a, b);
                assert!(dx.is_finite() && dy.is_finite(), "the way from {a:?} to {b:?} is finite: ({dx:?}, {dy:?})");
                assert!(dx != 0.0 || dy != 0.0, "the way from {a:?} to {b:?} is not nothing");
                let (sx, sy) = scaled_to_unit(dx, dy).unwrap_or((f64::NAN, f64::NAN));
                let larger = sx.abs().max(sy.abs());
                assert!(sx.is_finite() && sy.is_finite() && larger >= 1.0, "the scaled way from {a:?} to {b:?}: ({sx:?}, {sy:?})");
                assert!(sx == 0.0 || sx.is_sign_negative() == dx.is_sign_negative(), "x keeps its sign: {a:?} to {b:?}");
                assert!(sy == 0.0 || sy.is_sign_negative() == dy.is_sign_negative(), "y keeps its sign: {a:?} to {b:?}");
                pairs += 1;
            }
        }
        assert!(pairs > 80_000, "the pairs were walked: {pairs}");
    }

    /// The nearest point of the window the plain way, by clamping: the distance from a point is the larger of its two distances to the clamped point.
    fn plain_apart(p: Point, w: Rect) -> f64 {
        (p.x - p.x.clamp(w.x0, w.x1)).abs().max((p.y - p.y.clamp(w.y0, w.y1)).abs())
    }

    /// Which point the line is worked from is the nearer one, the first if they are as near. Over pairs of the points of a grid in each of several windows (29 by 29 points, in quarters
    /// of the window's size, from three sizes before the window to three after it on both axes), so that every side, corner, edge, inside, tie and unequal depth inside is paired with
    /// every other kind, the answer is the nearer by the larger axis, worked the plain way by clamping.
    #[test]
    fn the_line_is_worked_from_the_point_nearer_the_window_and_the_first_when_they_are_as_near() {
        let windows = [
            Rect::new(100.0, 200.0, 110.0, 210.0),
            Rect::new(1.0, 1.0, 3.0, 3.0),
            Rect::new(-3.0, -3.0, -1.0, -1.0),
            Rect::new(0.0, 0.0, 4.0, 2.0),
            Rect::new(-2.0, 5.0, -1.0, 9.0),
            Rect::new(0.25, 0.5, 0.5, 0.75),
        ];
        let mut checked = 0u32;
        for w in windows {
            let (step_x, step_y) = ((w.x1 - w.x0) / 2.0, (w.y1 - w.y0) / 2.0);
            let grid: Vec<Point> = (-12..=16)
                .flat_map(|i| (-12..=16).map(move |j| (i, j)))
                .map(|(i, j)| Point::new(w.x0 + f64::from(i) * step_x * 0.5, w.y0 + f64::from(j) * step_y * 0.5))
                .collect();
            for (n, &a) in grid.iter().enumerate() {
                // Every point against every third of the others, from a start that moves with it (the grid has 841 points).
                for &b in grid.iter().skip(n % 3).step_by(3) {
                    let want = if plain_apart(b, w) < plain_apart(a, w) { b } else { a };
                    assert_eq!(nearer(a, b, w), want, "window {w:?}, from {a:?} and {b:?}");
                    checked += 1;
                }
            }
            // The cases that name each of the four sides: a point just outside it, and a far one outside the opposite side, in either order.
            let middle = Point::new((w.x0 + w.x1) / 2.0, (w.y0 + w.y1) / 2.0);
            let (reach_x, reach_y) = (w.x1 - w.x0, w.y1 - w.y0);
            let sides = [
                (Point::new(w.x0 - 0.1 * reach_x, middle.y), Point::new(w.x1 + 0.5 * reach_x, middle.y)),
                (Point::new(w.x1 + 0.1 * reach_x, middle.y), Point::new(w.x0 - 0.5 * reach_x, middle.y)),
                (Point::new(middle.x, w.y0 - 0.1 * reach_y), Point::new(middle.x, w.y1 + 0.5 * reach_y)),
                (Point::new(middle.x, w.y1 + 0.1 * reach_y), Point::new(middle.x, w.y0 - 0.5 * reach_y)),
            ];
            for (near, far) in sides {
                assert_eq!(nearer(near, far, w), near, "window {w:?}: {near:?} is nearer than {far:?}");
                assert_eq!(nearer(far, near, w), near, "window {w:?}: {near:?} is nearer than {far:?}, given second");
            }
        }
        assert!(checked > 100_000, "the grid was walked: {checked} pairs");
        // As near, tie after tie: the first. Two points of the window (one deeper in), and two the same distance out on different sides.
        let w = Rect::new(0.0, 0.0, 4.0, 4.0);
        assert_eq!(nearer(Point::new(2.0, 2.0), Point::new(0.0, 0.0), w), Point::new(2.0, 2.0));
        assert_eq!(nearer(Point::new(0.0, 0.0), Point::new(2.0, 2.0), w), Point::new(0.0, 0.0));
        assert_eq!(nearer(Point::new(-1.0, 2.0), Point::new(2.0, 5.0), w), Point::new(-1.0, 2.0));
        assert_eq!(nearer(Point::new(2.0, 5.0), Point::new(-1.0, 2.0), w), Point::new(2.0, 5.0));
        // One a subnormal number outside the window and the other in it: the one in it, though the difference is lost in a scale of a quarter.
        let outside = Point::new(5e-324, 0.0);
        let in_it = Point::new(-1.0, 3.0);
        let window = Rect::new(-28.9, -1e308, 0.0, 1e299);
        assert_eq!(nearer(outside, in_it, window), in_it);
        assert_eq!(nearer(in_it, outside, window), in_it);
    }
}
