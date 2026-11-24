//! Hostile input: rows of any numbers, in any order, give a path or an error, never a panic, a hang or a NaN (AGENTS.md, Never crash).

use diacraft_geom::kurbo::{Line, PathEl, Point, Rect};
use diacraft_geom::{GeomError, GeomRow, InfiniteLine, MAX_ROWS, Nurbs, NurbsPoint, Polyline, WarningKind, build_paths};
use proptest::prelude::*;
use proptest::test_runner::TestCaseError;

use crate::{ln, mv, size};

/// A number from the places that break arithmetic: zero, tiny, huge, not a number, infinite, and ordinary.
fn number() -> impl Strategy<Value = f64> {
    prop_oneof![
        4 => -50.0f64..50.0,
        1 => Just(0.0),
        1 => Just(-0.0),
        1 => Just(1.0),
        1 => Just(f64::NAN),
        1 => Just(f64::INFINITY),
        1 => Just(f64::NEG_INFINITY),
        1 => Just(f64::MAX),
        1 => Just(f64::MIN),
        1 => Just(1e-300),
        1 => Just(5e-324),
        1 => Just(1e150),
        1 => Just(-1e308),
    ]
}

/// A whole number of the range a degree or a type holds, and beyond.
fn small() -> impl Strategy<Value = f64> {
    prop_oneof![Just(0.0), Just(1.0), Just(2.0), Just(3.0), Just(25.0), Just(26.0), Just(-1.0), 0.0f64..5.0, number()]
}

fn polyline() -> impl Strategy<Value = Option<Polyline>> {
    prop::option::of(
        (any::<bool>(), any::<bool>(), prop::collection::vec((number(), number()), 0..6)).prop_map(|(x_is_share, y_is_share, points)| Polyline {
            x_is_share,
            y_is_share,
            points,
        }),
    )
}

fn nurbs() -> impl Strategy<Value = Option<Nurbs>> {
    prop::option::of(
        (number(), small(), any::<bool>(), any::<bool>(), prop::collection::vec((number(), number(), number(), number()), 0..6)).prop_map(
            |(last_knot, degree, x_is_share, y_is_share, points)| Nurbs {
                last_knot,
                degree,
                x_is_share,
                y_is_share,
                points: points.into_iter().map(|(x, y, knot, weight)| NurbsPoint { x, y, knot, weight }).collect(),
            },
        ),
    )
}

/// A point's two numbers.
fn xy() -> impl Strategy<Value = (f64, f64)> {
    (number(), number())
}

fn row() -> impl Strategy<Value = GeomRow> {
    prop_oneof![
        xy().prop_map(|(x, y)| GeomRow::MoveTo { x, y }),
        xy().prop_map(|(x, y)| GeomRow::RelMoveTo { x, y }),
        xy().prop_map(|(x, y)| GeomRow::LineTo { x, y }),
        xy().prop_map(|(x, y)| GeomRow::RelLineTo { x, y }),
        (number(), number(), number()).prop_map(|(x, y, bow)| GeomRow::ArcTo { x, y, bow }),
        (xy(), xy(), number(), number()).prop_map(|((x, y), (mid_x, mid_y), angle, ratio)| GeomRow::EllipticalArcTo {
            x,
            y,
            mid_x,
            mid_y,
            angle,
            ratio
        }),
        (xy(), xy(), number(), number()).prop_map(|((x, y), (mid_x, mid_y), angle, ratio)| GeomRow::RelEllipticalArcTo {
            x,
            y,
            mid_x,
            mid_y,
            angle,
            ratio
        }),
        (xy(), polyline()).prop_map(|((x, y), polyline)| GeomRow::PolylineTo { x, y, polyline }),
        (xy(), number(), number(), number(), number(), nurbs()).prop_map(
            |((x, y), second_last_knot, last_weight, first_knot, first_weight, nurbs)| GeomRow::NurbsTo {
                x,
                y,
                second_last_knot,
                last_weight,
                first_knot,
                first_weight,
                nurbs
            }
        ),
        (xy(), number(), number(), number(), small()).prop_map(|((x, y), second_knot, first_knot, last_knot, degree)| GeomRow::SplineStart {
            x,
            y,
            second_knot,
            first_knot,
            last_knot,
            degree
        }),
        (xy(), number()).prop_map(|((x, y), knot)| GeomRow::SplineKnot { x, y, knot }),
        (xy(), xy(), xy()).prop_map(|((x, y), (c1_x, c1_y), (c2_x, c2_y))| GeomRow::RelCubBezTo { x, y, c1_x, c1_y, c2_x, c2_y }),
        (xy(), xy()).prop_map(|((x, y), (c_x, c_y))| GeomRow::RelQuadBezTo { x, y, c_x, c_y }),
        (xy(), xy(), xy()).prop_map(|((x, y), (first_x, first_y), (second_x, second_y))| GeomRow::Ellipse {
            x,
            y,
            first_x,
            first_y,
            second_x,
            second_y
        }),
        (xy(), xy()).prop_map(|((x, y), (other_x, other_y))| GeomRow::InfiniteLine { x, y, other_x, other_y }),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(400))]

    /// Any rows, any size that is a number: a path whose every point is finite and which begins with a move, warnings that name real rows, and no panic.
    #[test]
    fn any_rows_give_a_finite_path(rows in prop::collection::vec(row(), 0..24), width in -10.0f64..1e6, height in -10.0f64..1e6) {
        let built = build_paths(&rows, size(width, height)).unwrap();
        prop_assert!(built.stroke.is_finite());
        prop_assert!(built.fill.is_finite());
        for path in [&built.stroke, &built.fill] {
            prop_assert!(path.elements().is_empty() || matches!(path.elements().first(), Some(PathEl::MoveTo(_))));
        }
        for w in &built.warnings {
            prop_assert!(w.row < rows.len());
            prop_assert!(!w.to_string().is_empty());
        }
        for line in &built.infinite_lines {
            prop_assert!(line.p0.is_finite() && line.p1.is_finite());
        }
    }

    /// A size with NaN or infinity is an error and not a panic, whatever the rows.
    #[test]
    fn a_bad_size_is_an_error(rows in prop::collection::vec(row(), 0..6), bad in prop_oneof![Just(f64::NAN), Just(f64::INFINITY), Just(f64::NEG_INFINITY)]) {
        prop_assert_eq!(build_paths(&rows, size(bad, 1.0)).unwrap_err(), GeomError::BadSize);
        prop_assert_eq!(build_paths(&rows, size(1.0, bad)).unwrap_err(), GeomError::BadSize);
    }

    /// The constructor text of a cell is hostile too: any text gives a result or an error.
    #[test]
    fn any_constructor_text_is_read_or_refused(text in ".{0,80}") {
        let _ = Polyline::from_text(&text);
        let _ = Nurbs::from_text(&text);
        let numbers = format!("POLYLINE(0,0,{text})");
        let _ = Polyline::from_text(&numbers);
        let _ = Nurbs::from_text(&format!("NURBS({text})"));
    }
}

// ---- `InfiniteLine::clip` --------------------------------------------------------------------------

/// A finite number of any size: ordinary, zero, the edges of the range, the numbers are made of, subnormal ones, and numbers whose bits
/// are anything, which spread over every size there is.
fn any_finite() -> impl Strategy<Value = f64> {
    prop_oneof![
        3 => -50.0f64..50.0,
        2 => prop::sample::select(vec![
            0.0, -0.0, 1.0, -1.0, 0.5, 2.0, 4.0, 1e-9, 1e-300, -1e-300, 1e299, 2e299, 1e308, -1e308, f64::MAX, f64::MIN, f64::MAX / 2.0, f64::MIN / 2.0,
            f64::MAX / 4.0, f64::MIN_POSITIVE, -f64::MIN_POSITIVE, 5e-324, -5e-324,
        ]),
        3 => any::<u64>().prop_map(|bits| {
            let number = f64::from_bits(bits);
            if number.is_finite() { number } else { 0.0 }
        }),
    ]
}

fn point() -> impl Strategy<Value = Point> {
    (any_finite(), any_finite()).prop_map(|(x, y)| Point::new(x, y))
}

fn window() -> impl Strategy<Value = Rect> {
    (any_finite(), any_finite(), any_finite(), any_finite()).prop_map(|(x0, y0, x1, y1)| Rect::new(x0, y0, x1, y1))
}

fn inside(window: Rect, p: Point) -> bool {
    window.x0 <= p.x && p.x <= window.x1 && window.y0 <= p.y && p.y <= window.y1
}

/// What a result of `InfiniteLine::clip` must be for the line through `p0` and `p1` and the window `rect`: finite, with each end in the window and on one of its edges, on the line to
/// within 1E-12 of the largest magnitude among the inputs (the points' rounding is 1E-16 of it), and running the way the points do.
fn check_clipped(p0: Point, p1: Point, rect: Rect, clipped: Line) -> Result<(), TestCaseError> {
    let window = rect.abs();
    let scale = [p0.x, p0.y, p1.x, p1.y, window.x0, window.y0, window.x1, window.y1].iter().fold(0.0f64, |m, v| m.max(v.abs()));
    // The direction of the points, with no overflow: halved when a difference would pass the largest number. Every other number is divided by the scale, so none can overflow.
    let plain = (p1.x - p0.x, p1.y - p0.y);
    let (dx, dy) = if plain.0.is_finite() && plain.1.is_finite() { plain } else { (p1.x * 0.5 - p0.x * 0.5, p1.y * 0.5 - p0.y * 0.5) };
    let largest = dx.abs().max(dy.abs());
    prop_assert!(largest > 0.0, "distinct points have a direction");
    let (ux, uy) = (dx / largest, dy / largest);
    let along = ux.hypot(uy);
    for end in [clipped.p0, clipped.p1] {
        prop_assert!(end.is_finite(), "finite: {end:?}");
        prop_assert!(inside(window, end), "in the window {window:?}: {end:?}");
        prop_assert!(
            end.x == window.x0 || end.x == window.x1 || end.y == window.y0 || end.y == window.y1,
            "on an edge of the window {window:?}: {end:?}"
        );
        let (ox, oy) = (end.x / scale - p0.x / scale, end.y / scale - p0.y / scale);
        let off = ((ox * uy - oy * ux) / along).abs();
        prop_assert!(off <= 1e-12, "{end:?} is {off} (of the scale {scale}) off the line through {p0:?} and {p1:?}");
    }
    let (sx, sy) = ((clipped.p1.x - clipped.p0.x) / scale, (clipped.p1.y - clipped.p0.y) / scale);
    prop_assert!((sx * ux + sy * uy) / along >= -1e-12, "from the first point's side to the second's: {clipped:?} for {p0:?} to {p1:?}");
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(4000))]

    /// For finite points and a finite window of any size, a result is a finite segment on the line and in the window, equal points give none, and a line
    /// through a point that is in the window is never lost (the point the line is worked from is the one nearer the window, so that holds whatever the other point's size).
    #[test]
    fn a_clipped_line_is_finite_on_the_line_and_in_the_window(p0 in point(), p1 in point(), rect in window()) {
        let clipped = InfiniteLine { p0, p1 }.clip(rect);
        if p0 == p1 {
            prop_assert!(clipped.is_none(), "INFLINE");
            return Ok(());
        }
        if inside(rect.abs(), p0) || inside(rect.abs(), p1) {
            prop_assert!(clipped.is_some(), "a point of the line is in the window {rect:?}: {p0:?} {p1:?}");
        }
        if let Some(line) = clipped {
            check_clipped(p0, p1, rect, line)?;
        }
    }

    /// A line through a point well inside the window is kept, whatever the sizes of the window and of the points that give the line. The points are up to 10^`top` from the window's
    /// middle, for a `top` from -300 to 308 (the end of the range often, where the difference of two points on either side passes the largest number), and the window is 10^`gap`
    /// of that smaller, 1 to 1E12 times, down to 2E-300 across, and in every direction, the axes included. The line passes the point to within the rounding of the points, which
    /// is 1E-4 of the window at the most.
    #[test]
    fn a_line_through_the_middle_of_the_window_is_kept(
        top in prop_oneof![3 => Just(308i32), 1 => Just(307i32), 6 => -290i32..=308],
        gap in 0i32..=12,
        centre in (-5i32..=5, -5i32..=5),
        middle in (-0.8f64..0.8, -0.8f64..0.8),
        way in prop_oneof![
            Just((1.0, 0.0)),
            Just((0.0, 1.0)),
            Just((-1.0, 0.0)),
            Just((0.0, -1.0)),
            (0.0f64..std::f64::consts::TAU).prop_map(|angle| (angle.cos(), angle.sin())),
        ],
        reach in (0.1f64..1.0, 0.1f64..1.0),
    ) {
        let size = 10f64.powi((top - gap).min(296));
        let (cx, cy) = (f64::from(centre.0) * size, f64::from(centre.1) * size);
        let rect = Rect::new(cx - size, cy - size, cx + size, cy + size);
        let q = Point::new(cx + middle.0 * size, cy + middle.1 * size);
        let (back, front) = (10f64.powi(top) * reach.0, 10f64.powi(top) * reach.1);
        let p0 = Point::new(q.x - way.0 * back, q.y - way.1 * back);
        let p1 = Point::new(q.x + way.0 * front, q.y + way.1 * front);
        prop_assume!(p0.is_finite() && p1.is_finite() && p0 != p1);
        let clipped = InfiniteLine { p0, p1 }.clip(rect);
        prop_assert!(clipped.is_some(), "the line through {p0:?} and {p1:?} crosses the window {rect:?} at {q:?}");
        if let Some(line) = clipped {
            check_clipped(p0, p1, rect, line)?;
        }
    }
}

/// A number with its low nine bits cleared and small enough that 1024 times it is a number: halving it and scaling it by any power of two from 2^-8 to 2^8 is then exact, a
/// subnormal one too (it is a whole number of 2^-1065).
fn tame(number: f64) -> f64 {
    let number = f64::from_bits(number.to_bits() & !0x1ff);
    if number.abs() > f64::MAX / 1024.0 { number / 1024.0 } else { number }
}

/// A subnormal number of several bits.
fn subnormal() -> impl Strategy<Value = f64> {
    (1u64..(1 << 31), any::<bool>()).prop_map(|(bits, negative)| {
        let number = f64::from_bits(bits << 9);
        if negative { -number } else { number }
    })
}

/// A point of any shape that is not the origin: either part of any size, a subnormal one with a huge one included, so that the larger part of a direction through it is more than
/// 2^1074 times the smaller. Its low bits are cleared (`tame`).
fn point_of_any_shape() -> impl Strategy<Value = Point> {
    prop_oneof![
        3 => (any_finite(), any_finite()),
        3 => (subnormal(), any_finite()),
        3 => (any_finite(), subnormal()),
        1 => (subnormal(), subnormal()),
    ]
    .prop_map(|(x, y)| {
        let (x, y) = (tame(x), tame(y));
        if x == 0.0 && y == 0.0 { Point::new(1.0, 0.0) } else { Point::new(x, y) }
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(4000))]

    /// A line that crosses the window is kept when neither of its two points is in it, whatever the shape of its direction (one whose smaller part is under 2^-1074 of
    /// the larger was lost). The oracle is exact: the line goes through the origin and a point `q`, and the window is the box from `q`/2 to 2`q`, so the line crosses it from `q`/2
    /// to 2`q` (every point of it is `s q`) and the segment is known in every digit; the two points are `q` times 2^j for a `j` of 2 to 8 either way, each exactly on the line and
    /// outside the window. Each end is that point to 1E-9 (and a unit of the subnormal numbers), in the order of the points.
    #[test]
    fn a_line_crossing_the_window_is_kept_when_neither_point_is_in_it(q in point_of_any_shape(), first in 0usize..14, then in 1usize..14) {
        // Two different powers of the fourteen (none is rejected, so that the run can be as long as one wants).
        const POWERS: [i32; 14] = [-8, -7, -6, -5, -4, -3, -2, 2, 3, 4, 5, 6, 7, 8];
        let (j0, j1) = (POWERS[first], POWERS[(first + then) % 14]);
        let at = |j: i32| Point::new(q.x * 2f64.powi(j), q.y * 2f64.powi(j));
        let (p0, p1) = (at(j0), at(j1));
        let rect = Rect::new(q.x * 0.5, q.y * 0.5, q.x * 2.0, q.y * 2.0);
        prop_assert!(!inside(rect.abs(), p0) && !inside(rect.abs(), p1), "the construction: neither point is in the window");
        let (low, high) = (Point::new(q.x * 0.5, q.y * 0.5), Point::new(q.x * 2.0, q.y * 2.0));
        let line = InfiniteLine { p0, p1 }
            .clip(rect)
            .ok_or_else(|| TestCaseError::fail(format!("the line through {p0:?} and {p1:?} crosses the window {rect:?} from {low:?} to {high:?} and was lost")))?;
        let (first, last) = if j1 > j0 { (low, high) } else { (high, low) };
        for (got, want) in [(line.p0, first), (line.p1, last)] {
            let near = |a: f64, b: f64| (a - b).abs() <= 1e-9 * b.abs() + 1e-323;
            prop_assert!(near(got.x, want.x) && near(got.y, want.y), "{got:?} is not {want:?}: the line through {p0:?} and {p1:?}, the window {rect:?}");
        }
    }
}

/// One unit of the subnormal numbers, 5E-324 or 2^-1074.
const UNIT: f64 = 5e-324;

/// Where the window's x edges are, as fractions of the line's big part: on it, round it, and well off it (1E-8 of 1E308 is the 1E300 of a window the points are far from).
const EDGES: [f64; 9] = [-1.0, -0.5, -0.1, -1e-8, 0.0, 1e-8, 0.1, 0.5, 1.0];

proptest! {
    #![proptest_config(ProptestConfig::with_cases(4000))]

    /// A line with a part of a few units of the smallest number is cut where its exact chord is, whatever the size of its big part (where a number passed a quarter of the
    /// largest, the positions were carried at a quarter of their size and a position of one or two units was turned to 0, which lost the line or cut it to a wrong segment; a scale of a
    /// quarter taken for every line did the same to ordinary sizes, so the big part is ordinary too). The line is `s` times the direction (`big`, `units` units) through the origin, and its
    /// points are at `s` -1, 0 or 1. The window's x edges are fractions of `big`, and its y edges whole units, so that the chord is known by its `s` (the larger of the two lower limits to the smaller of the
    /// upper): each end is that `s` to 1E-9 of `big` in the big part and to a unit in the small part (6 where a number passes a quarter of the largest, as the positions are
    /// rounded there at the quarter); a chord that is clearly empty is none. The numbers are turned: x and y swapped, and either negated, so that the big part lies along either axis, either way.
    #[test]
    fn a_line_with_a_part_of_a_few_subnormal_units_is_cut_at_its_exact_chord(
        big in prop_oneof![
            4 => prop::sample::select(vec![f64::MAX, f64::MAX / 2.0, 2f64.powi(1022), 8.98846567431158e307, 1e308, f64::MAX / 4.0 * 1.000_000_1, f64::MAX * 0.6]),
            2 => 1e100f64..1e300,
            3 => 0.01f64..100.0,
        ],
        units in 1i32..=6,
        turn in (any::<bool>(), any::<bool>(), any::<bool>()),
        points in (0usize..3, 1usize..3),
        across in (0usize..EDGES.len(), 0usize..EDGES.len()),
        height in (-6i32..=6, -6i32..=6),
    ) {
        let (swap, flip_x, flip_y) = turn;
        let (a, b) = ([-1.0, 0.0, 1.0][points.0], [-1.0, 0.0, 1.0][(points.0 + points.1) % 3]);
        // Two points on either side of the origin that are far enough apart for the difference to pass the largest number are given by half of it, and half of an odd number of
        // units is not a number of units: the direction is then 2 units to the point's 3, say, and a window a unit from the line is on either side of it. So their units are even.
        let halved = a * b < 0.0 && 2.0 * big > f64::MAX;
        let j = f64::from(if halved && units % 2 == 1 { units + 1 } else { units });
        let to_page = |(x, y): (f64, f64)| {
            let (x, y) = if swap { (y, x) } else { (x, y) };
            Point::new(if flip_x { -x } else { x }, if flip_y { -y } else { y })
        };
        let from_page = |p: Point| {
            let (x, y) = (if flip_x { -p.x } else { p.x }, if flip_y { -p.y } else { p.y });
            if swap { (y, x) } else { (x, y) }
        };
        let at = |s: f64| (s * big, s * j * UNIT);
        let (xs, ys) = ((EDGES[across.0], EDGES[across.1]), (f64::from(height.0), f64::from(height.1)));
        let (corner0, corner1) = (to_page((xs.0 * big, ys.0 * UNIT)), to_page((xs.1 * big, ys.1 * UNIT)));
        let rect = Rect::new(corner0.x, corner0.y, corner1.x, corner1.y);
        // The line is at x = s * big and at y = s * j units, so the window's edges are at these s, and the chord is the overlap of the two ranges.
        let lower = xs.0.min(xs.1).max(ys.0.min(ys.1) / j);
        let upper = xs.0.max(xs.1).min(ys.0.max(ys.1) / j);
        let (p0, p1) = (to_page(at(a)), to_page(at(b)));
        let clipped = InfiniteLine { p0, p1 }.clip(rect);
        let what = format!("the line through {p0:?} and {p1:?}, the window {rect:?}");
        if upper - lower >= 1e-6 {
            let line = clipped.ok_or_else(|| TestCaseError::fail(format!("{what} has a chord from s {lower} to {upper} and was lost")))?;
            let (first, last) = if b > a { (lower, upper) } else { (upper, lower) };
            for (got, s) in [(line.p0, first), (line.p1, last)] {
                let (got_x, got_y) = from_page(got);
                let want = at(s);
                prop_assert!((got_x - want.0).abs() <= 1e-9 * big, "{got:?} is not the end at s {s} ({want:?} before the turn): {what}");
                // A unit at most where no number passes a quarter of the largest (nothing is then scaled, and a scale of a quarter that is taken all the same turns 1 to 0 and 3 to 1),
                // and the 6 where one does (a position of a few units is then rounded at the quarter, up to 2 units there, and again on its way back).
                let slack = if big > f64::MAX / 4.0 { 6.0 } else { 1.0 };
                prop_assert!((got_y - want.1).abs() <= slack * UNIT, "{got:?} is not the end at s {s} ({want:?} before the turn): {what}");
            }
        } else if lower - upper >= 1e-6 {
            prop_assert!(clipped.is_none(), "{what} has no chord (s {lower} is past {upper}), and gave {clipped:?}");
        }
    }
}

/// The line cut to the window the plain way (the way `InfiniteLine::clip` was made before a change), for numbers of ordinary size, where nothing can overflow: the reference the
/// clip of any size is compared with.
fn plain_clip(p0: Point, p1: Point, window: Rect) -> Option<(Point, Point)> {
    let (dx, dy) = (p1.x - p0.x, p1.y - p0.y);
    let (mut from, mut to) = (f64::NEG_INFINITY, f64::INFINITY);
    for (origin, step, low, high) in [(p0.x, dx, window.x0, window.x1), (p0.y, dy, window.y0, window.y1)] {
        if step == 0.0 {
            if origin < low || origin > high {
                return None;
            }
        } else {
            let (a, b) = ((low - origin) / step, (high - origin) / step);
            from = from.max(a.min(b));
            to = to.min(a.max(b));
        }
    }
    (from <= to).then(|| (Point::new(p0.x + dx * from, p0.y + dy * from), Point::new(p0.x + dx * to, p0.y + dy * to)))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(4000))]

    /// A line and a window of ordinary size, made as large or as small as the numbers go (by a power of ten, from 1E-300 to 1E300), are cut to the same segment, scaled: each end of the
    /// cut is where the plain cut of the ordinary numbers puts it, to 1E-6 of the window's size, and a segment longer than that is not lost. Nothing visible changes for ordinary
    /// values (and the guides of are 0 to 232.3), and the segment is the line's chord of the window and not any two points on its edges.
    #[test]
    fn a_line_of_any_size_is_cut_where_the_same_line_of_ordinary_size_is(
        k in -300i32..=300,
        start in (-50.0f64..50.0, -50.0f64..50.0),
        step in (-50.0f64..50.0, -50.0f64..50.0),
        corners in (-60.0f64..60.0, -60.0f64..60.0, -60.0f64..60.0, -60.0f64..60.0),
    ) {
        prop_assume!(step.0.hypot(step.1) >= 0.1);
        let (p0, p1) = (Point::new(start.0, start.1), Point::new(start.0 + step.0, start.1 + step.1));
        let rect = Rect::new(corners.0, corners.1, corners.2, corners.3);
        let size = 10f64.powi(k);
        let scaled = |p: Point| Point::new(p.x * size, p.y * size);
        let reference = plain_clip(p0, p1, rect.abs());
        let clipped = InfiniteLine { p0: scaled(p0), p1: scaled(p1) }.clip(Rect::new(corners.0 * size, corners.1 * size, corners.2 * size, corners.3 * size));
        if let Some((a, b)) = reference.filter(|(a, b)| (*b - *a).hypot() >= 1e-6) {
            let line = clipped.ok_or_else(|| TestCaseError::fail(format!("the line {p0:?} to {p1:?} crosses the window {rect:?} and was lost at size {size}")))?;
            let (got_a, got_b) = (Point::new(line.p0.x / size, line.p0.y / size), Point::new(line.p1.x / size, line.p1.y / size));
            let width = (rect.abs().width().max(rect.abs().height())).max(1.0);
            prop_assert!((got_a - a).hypot() <= 1e-6 * width && (got_b - b).hypot() <= 1e-6 * width, "{got_a:?} {got_b:?} are not {a:?} {b:?} at size {size}");
        }
    }
}

/// The limits are past every figure in files, and a section over the row cap is refused before any work is done.
#[test]
fn too_many_rows_is_an_error() {
    let rows = vec![ln(1.0, 1.0); MAX_ROWS + 1];
    assert_eq!(build_paths(&rows, size(1.0, 1.0)).unwrap_err(), GeomError::TooManyRows { rows: MAX_ROWS + 1, max: MAX_ROWS });
    const { assert!(MAX_ROWS > 31_273, "The most rows in a section in files") };
    let mut at_cap = vec![mv(0.0, 0.0)];
    at_cap.extend(std::iter::repeat_n(ln(1.0, 1.0), MAX_ROWS - 1));
    let built = build_paths(&at_cap, size(1.0, 1.0)).unwrap();
    assert_eq!(built.stroke.elements().len(), MAX_ROWS);
}

/// A section of rows that each make many elements stops at the element cap with an error, not an allocation of the product.
#[test]
fn a_path_that_would_be_too_large_is_an_error() {
    let ellipse = GeomRow::Ellipse { x: 0.0, y: 0.0, first_x: 1e6, first_y: 0.0, second_x: 0.0, second_y: 1e6 };
    // 100,000 ellipses of a huge radius, each over a hundred pieces: well past the cap of path elements.
    let rows = vec![ellipse; 100_000];
    assert_eq!(build_paths(&rows, size(1.0, 1.0)).unwrap_err(), GeomError::PathTooLarge { max: diacraft_geom::MAX_PATH_ELEMENTS });
}

/// A spline row of `tuples` control points, degree 25: the most work a point can cost.
fn big_spline(tuples: usize) -> GeomRow {
    let head = [(tuples + 2) as f64, 25.0, 1.0, 1.0];
    let body = (0..tuples).flat_map(|i| [i as f64, ((i * 7) % 5) as f64, (i + 1) as f64, 1.0]);
    let args: Vec<f64> = head.into_iter().chain(body).collect();
    GeomRow::NurbsTo {
        x: tuples as f64,
        y: 0.0,
        second_last_knot: (tuples + 1) as f64,
        last_weight: 1.0,
        first_knot: 0.0,
        first_weight: 1.0,
        nurbs: Some(Nurbs::from_args(&args).unwrap()),
    }
}

/// The work a section may spend on splines is bounded: rows of the biggest splines run out of it, and the ones past that are skipped with a warning
/// that says so. That is the claim, and it is counted: which rows ran out of work. The 60 s at the end is a hang guard only (the test takes
/// well under a second, and a machine at load 50 does not need a minute for it), not a measure of cost.
#[test]
fn a_section_of_huge_splines_runs_out_of_work_and_says_so() {
    let rows: Vec<GeomRow> = [mv(0.0, 0.0), big_spline(300)].into_iter().chain(std::iter::repeat_n(big_spline(20_000), 8)).collect();
    let started = std::time::Instant::now();
    let built = build_paths(&rows, size(1.0, 1.0)).unwrap();
    let out_of_work: Vec<usize> =
        built.warnings.iter().filter(|w| matches!(w.kind, WarningKind::NurbsUnusable(why) if why.contains("work"))).map(|w| w.row).collect();
    assert!(!out_of_work.is_empty(), "some row ran out of work: {:?}", built.warnings.iter().map(|w| w.kind).collect::<Vec<_>>());
    assert!(!out_of_work.contains(&1), "the small spline still drew");
    assert!(out_of_work.contains(&9), "and the last of the huge ones did not: {out_of_work:?}");
    assert!(started.elapsed().as_secs() < 60, "bounded: took {:?}", started.elapsed());
}

/// A caller may hold a section to less than the renderer does (a snap builds the outlines of up to 16 shapes at each move). Counted: the elements
/// the path may have, and the spline work, which the answer reports.
#[test]
fn a_caller_may_bound_a_section_to_less_than_the_renderer_does() {
    use diacraft_geom::rules::FILL_RUN_JOIN;
    use diacraft_geom::{Bound, MAX_PATH_ELEMENTS, build_paths_bounded};
    // The elements: 1,000 segments against a bound of 100.
    let mut rows = vec![mv(0.0, 0.0)];
    rows.extend(std::iter::repeat_n(ln(1.0, 1.0), 1_000));
    let small = Bound { path_elements: 100, ..Bound::default() };
    assert_eq!(build_paths_bounded(&rows, size(1.0, 1.0), FILL_RUN_JOIN, small).unwrap_err(), GeomError::PathTooLarge { max: 100 });
    let same = build_paths_bounded(&rows, size(1.0, 1.0), FILL_RUN_JOIN, Bound::default()).unwrap();
    assert_eq!(same.stroke, build_paths(&rows, size(1.0, 1.0)).unwrap().stroke, "the default bound is the renderer's");
    assert_eq!(Bound::default().path_elements, MAX_PATH_ELEMENTS);
    // The work: a spline that costs some, then the same spline with less work than that.
    let rows = [mv(0.0, 0.0), big_spline(300)];
    let full = build_paths(&rows, size(1.0, 1.0)).unwrap();
    assert!(full.curve_work > 1_000, "the spline costs work: {}", full.curve_work);
    assert!(!full.warnings.iter().any(|w| matches!(w.kind, WarningKind::NurbsUnusable(why) if why.contains("work"))), "{:?}", full.warnings);
    let starved = build_paths_bounded(&rows, size(1.0, 1.0), FILL_RUN_JOIN, Bound { curve_work: 1_000, ..Bound::default() }).unwrap();
    assert!(starved.curve_work <= 1_000, "{}", starved.curve_work);
    assert!(
        starved.warnings.iter().any(|w| matches!(w.kind, WarningKind::NurbsUnusable(why) if why.contains("work"))),
        "the spline is skipped and says so: {:?}",
        starved.warnings
    );
    assert!(starved.stroke.elements().len() < full.stroke.elements().len());
}

/// A caller's bound is held to the renderer's: `u64::MAX` of spline work and `usize::MAX` elements are the renderer's caps and not more (L-b of the
/// re-review: the bound was taken as given, so any caller of the public function could lift the caps).
#[test]
fn a_bound_cannot_lift_the_renderers_caps() {
    use diacraft_geom::rules::FILL_RUN_JOIN;
    use diacraft_geom::{Bound, MAX_PATH_ELEMENTS, build_paths_bounded};
    let lifted = Bound { curve_work: u64::MAX, path_elements: usize::MAX };
    assert_eq!(lifted.clamped(), Bound::default());
    let between = Bound { curve_work: 5, path_elements: 7 };
    assert_eq!(between.clamped(), between, "a smaller bound is kept");
    // 28,000 ellipses of a huge radius (152 elements each) make over 4,000,000 elements, the renderer's cap, whatever the caller asks for.
    let ellipse = GeomRow::Ellipse { x: 0.0, y: 0.0, first_x: 1e6, first_y: 0.0, second_x: 0.0, second_y: 1e6 };
    let rows = vec![ellipse; 28_000];
    // (Not `unwrap_err`: a build that wrongly succeeds would print four million elements into the failure message.)
    let refused =
        matches!(build_paths_bounded(&rows, size(1.0, 1.0), FILL_RUN_JOIN, lifted), Err(GeomError::PathTooLarge { max }) if max == MAX_PATH_ELEMENTS);
    assert!(refused, "the renderer's cap of path elements stopped the build");
}
