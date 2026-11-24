//! `Ellipse` and `InfiniteLine`.
#![allow(clippy::approx_constant, reason = "the spec's vectors give these figures rounded (1.41421, 0.70711), and the tests use them as written")]

use diacraft_geom::kurbo::{Affine, ParamCurve, Rect};
use diacraft_geom::{GeomRow, InfiniteLine, SectionFlags, WarningKind, combined_fill, even_odd_contains};

use crate::{CURVE_EPS, EPS, assert_near, assert_pt, build_plain, ends_with_close, extents, ln, mv, p, passes_through, segments};

fn ellipse(x: f64, y: f64, first: (f64, f64), second: (f64, f64)) -> GeomRow {
    GeomRow::Ellipse { x, y, first_x: first.0, first_y: first.1, second_x: second.0, second_y: second.1 }
}

fn infinite(x: f64, y: f64, other: (f64, f64)) -> GeomRow {
    GeomRow::InfiniteLine { x, y, other_x: other.0, other_y: other.1 }
}

/// Given `X` 2, `Y` 1, `A` 4, `B` 1, `C` 2, `D` 2, expect extents from (0, 0) to (4, 2). Reading `A` as a half-width gives x from -2 to 6.
#[test]
fn ellipse_the_two_points_are_on_the_ellipse_not_half_widths() {
    let built = build_plain(&[ellipse(2.0, 1.0, (4.0, 1.0), (2.0, 2.0))]);
    let e = extents(&built.stroke);
    assert_near(e.x0, 0.0, CURVE_EPS, "x0");
    assert_near(e.y0, 0.0, CURVE_EPS, "y0");
    assert_near(e.x1, 4.0, CURVE_EPS, "x1");
    assert_near(e.y1, 2.0, CURVE_EPS, "y1");
    assert!(built.warnings.is_empty(), "perpendicular axes: nothing is unsettled");
    // The path starts at the first point and runs toward the second.
    assert_pt(segments(&built.stroke)[0].start(), 4.0, 1.0, EPS, "starts at (A, B)");
    assert!(passes_through(&built.stroke, p(2.0, 2.0), CURVE_EPS), "passes the second point");
}

/// Given a centre at the origin, a first point of about (1.41421, 1.41421), and a second of about (-0.70711, 0.70711), expect
/// half-axes 2 and 1 turned by 45 degrees, and extents of about plus or minus 1.5811 on each axis. Taking the lengths and an upright ellipse gives 2 and 1.
#[test]
fn ellipse_the_axes_may_be_turned() {
    let built = build_plain(&[ellipse(0.0, 0.0, (1.41421, 1.41421), (-0.70711, 0.70711))]);
    let e = extents(&built.stroke);
    for (got, want, what) in [(e.x0, -1.5811, "x0"), (e.x1, 1.5811, "x1"), (e.y0, -1.5811, "y0"), (e.y1, 1.5811, "y1")] {
        assert_near(got, want, 1e-4, what);
    }
    assert!((e.x1 - 2.0).abs() > 0.4 && (e.y1 - 1.0).abs() > 0.4, "naive: upright 2 and 1");
    assert!(built.warnings.is_empty(), "the points are on perpendicular axes (to the rounding of the figures)");
    // Every point of the path is on the turned ellipse: its half-axes along the 45 degree directions are 2 and 1.
    for seg in built.stroke.segments() {
        for i in 0..=40 {
            let pt = seg.eval(i as f64 / 40.0);
            let along = (pt.x + pt.y) / std::f64::consts::SQRT_2;
            let across = (pt.y - pt.x) / std::f64::consts::SQRT_2;
            assert_near((along / 2.0).powi(2) + across.powi(2), 1.0, 1e-4, "on the ellipse");
        }
    }
}

/// Given an `Ellipse` and a following row, expect the ellipse closed, with no pen for the next row. Given a filled shape
/// holding only that row, expect the area filled. Starting the next row from the ellipse draws a stray line.
#[test]
fn ellipse_the_ellipse_is_closed_and_leaves_no_pen() {
    let alone = build_plain(&[ellipse(0.0, 0.0, (2.0, 0.0), (0.0, 1.0))]);
    assert!(ends_with_close(&alone.stroke), "the ellipse is closed");
    assert!(ends_with_close(&alone.fill));
    assert!(even_odd_contains(&alone.fill, p(0.0, 0.0)), "the area is filled");
    assert!(even_odd_contains(&alone.fill, p(1.5, 0.1)));
    assert!(!even_odd_contains(&alone.fill, p(1.9, 0.9)), "outside the ellipse, inside its box");

    let after = build_plain(&[ellipse(0.0, 0.0, (2.0, 0.0), (0.0, 1.0)), ln(5.0, 5.0)]);
    assert_eq!(after.warnings.len(), 1, "the row after has no pen");
    assert_eq!(after.warnings[0].row, 1);
    assert_eq!(after.warnings[0].kind, WarningKind::NoPen);
    assert!(!passes_through(&after.stroke, p(5.0, 5.0), 0.01), "no stray line to (5, 5)");

    // A move after it starts a path of its own: the ellipse and the rows are both kept.
    let mixed = build_plain(&[ellipse(0.0, 0.0, (2.0, 0.0), (0.0, 1.0)), mv(5.0, 5.0), ln(6.0, 5.0)]);
    assert!(mixed.warnings.is_empty());
    assert!(passes_through(&mixed.stroke, p(6.0, 5.0), EPS));
    assert!(passes_through(&mixed.stroke, p(2.0, 0.0), CURVE_EPS));
    let filled = combined_fill([(&mixed.fill, SectionFlags::default())]);
    assert!(even_odd_contains(&filled, p(0.0, 0.0)));

    // And rows before an ellipse keep their own path.
    let before = build_plain(&[mv(10.0, 10.0), ln(12.0, 10.0), ln(12.0, 12.0), ellipse(0.0, 0.0, (2.0, 0.0), (0.0, 1.0))]);
    assert!(before.warnings.is_empty());
    assert!(passes_through(&before.stroke, p(12.0, 12.0), EPS));
    assert!(even_odd_contains(&before.fill, p(11.5, 10.5)), "the triangle's ring is filled");
    assert!(even_odd_contains(&before.fill, p(0.0, 0.0)), "and the ellipse's");
}

/// Given a centre at the origin, a first point (1, 0) and a second (1, 1), expect our rule's extents of about plus or minus 1.4142 in x
/// and 1 in y, and the case flagged as unsettled.
#[test]
fn ellipse_oblique_points_are_read_as_conjugate_half_diameters_and_flagged() {
    let built = build_plain(&[ellipse(0.0, 0.0, (1.0, 0.0), (1.0, 1.0))]);
    let e = extents(&built.stroke);
    assert_near(e.x0, -2.0_f64.sqrt(), CURVE_EPS, "x0");
    assert_near(e.x1, 2.0_f64.sqrt(), CURVE_EPS, "x1");
    assert_near(e.y0, -1.0, CURVE_EPS, "y0");
    assert_near(e.y1, 1.0, CURVE_EPS, "y1");
    assert_eq!(built.warnings.len(), 1);
    assert_eq!(built.warnings[0].kind, WarningKind::ObliqueEllipse);
    // A hair off perpendicular, as rounding leaves it, is not flagged.
    let rounding = build_plain(&[ellipse(0.0, 0.0, (2.0, 0.0), (0.001, 1.0))]);
    assert!(rounding.warnings.is_empty());
}

/// Our rule: a zero half-axis is traced by the formula as a line or a point, with no panic and no NaN.
#[test]
fn ellipse_a_zero_half_axis_traces_a_line_or_a_point() {
    let line = build_plain(&[ellipse(0.0, 0.0, (2.0, 0.0), (0.0, 0.0))]);
    let e = extents(&line.stroke);
    assert_near(e.x0, -2.0, CURVE_EPS, "x0");
    assert_near(e.x1, 2.0, CURVE_EPS, "x1");
    assert_near(e.y0, 0.0, CURVE_EPS, "y0");
    assert_near(e.y1, 0.0, CURVE_EPS, "y1");
    let point = build_plain(&[ellipse(3.0, 4.0, (3.0, 4.0), (3.0, 4.0))]);
    assert!(extents(&point.stroke).area().abs() < 1e-12);
    assert!(point.stroke.is_finite());
    // Both points on the same line from the centre (parallel conjugate diameters): flagged, drawn as a line.
    let flat = build_plain(&[ellipse(0.0, 0.0, (1.0, 1.0), (2.0, 2.0))]);
    assert_eq!(flat.warnings.len(), 1);
    assert_eq!(flat.warnings[0].kind, WarningKind::ObliqueEllipse);
}

/// Given a guide with `Angle` 90 degrees, `X` 0, `Y` 0, `A` 1, `B` 0, pin at (5, 0), expect a vertical line at x 5 on
/// the page, running past the page's edges and clipped when drawn. Drawing from (0, 0) to (1, 0) as a segment gives a short line, and treating it as part of a
/// filled path adds an edge.
#[test]
fn inf_line_an_infinite_line_is_clipped_and_is_no_segment_of_the_path() {
    let built = build_plain(&[infinite(0.0, 0.0, (1.0, 0.0))]);
    assert!(built.stroke.elements().is_empty(), "not a segment of the stroke");
    assert!(built.fill.elements().is_empty(), "not an edge of a fill");
    assert_eq!(built.infinite_lines.len(), 1);
    let line = built.infinite_lines[0];
    assert_eq!((line.p0, line.p1), (p(0.0, 0.0), p(1.0, 0.0)));
    // The shape's frame to the page: pin at (5, 0), turned 90 degrees counterclockwise.
    let to_page = Affine::translate((5.0, 0.0)) * Affine::rotate(std::f64::consts::FRAC_PI_2);
    let on_page = line.transform(to_page);
    let clipped = on_page.clip(Rect::new(0.0, 0.0, 11.0, 8.5)).unwrap();
    assert_pt(clipped.p0, 5.0, 0.0, 1e-9, "bottom edge of the page");
    assert_pt(clipped.p1, 5.0, 8.5, 1e-9, "top edge of the page");
    assert!(clipped.p0.x == clipped.p1.x || (clipped.p0.x - 5.0).abs() < 1e-9, "vertical at x 5");
    // It runs past the edges: clipped to a larger rectangle it is longer.
    let longer = on_page.clip(Rect::new(-10.0, -10.0, 20.0, 20.0)).unwrap();
    assert_near((longer.p1 - longer.p0).hypot(), 30.0, 1e-9, "length in the larger window");
    assert!(on_page.clip(Rect::new(6.0, 0.0, 11.0, 8.5)).is_none(), "off the window");
}

/// Our rule: two equal points draw nothing; a slanted line is the line as stated.
#[test]
fn inf_line_equal_points_draw_nothing_and_a_slant_is_kept() {
    let same = InfiniteLine { p0: p(1.0, 1.0), p1: p(1.0, 1.0) };
    assert!(same.clip(Rect::new(0.0, 0.0, 5.0, 5.0)).is_none());
    let slant = InfiniteLine { p0: p(0.0, 0.0), p1: p(1.0, 1.0) };
    let c = slant.clip(Rect::new(0.0, 0.0, 2.0, 3.0)).unwrap();
    assert_pt(c.p0, 0.0, 0.0, 1e-9, "start");
    assert_pt(c.p1, 2.0, 2.0, 1e-9, "end");
    let horizontal = InfiniteLine { p0: p(3.0, 1.0), p1: p(4.0, 1.0) };
    let h = horizontal.clip(Rect::new(0.0, 0.0, 10.0, 5.0)).unwrap();
    assert_pt(h.p0, 0.0, 1.0, 1e-9, "left");
    assert_pt(h.p1, 10.0, 1.0, 1e-9, "right");
    assert!(horizontal.clip(Rect::new(0.0, 2.0, 10.0, 5.0)).is_none(), "a horizontal line above the window's bottom");
    // A rectangle given with its corners the other way round, and hostile numbers: no panic.
    assert!(horizontal.clip(Rect::new(10.0, 5.0, 0.0, 0.0)).is_some());
    for bad in [f64::NAN, f64::INFINITY] {
        assert!(InfiniteLine { p0: p(bad, 0.0), p1: p(1.0, 1.0) }.clip(Rect::new(0.0, 0.0, 1.0, 1.0)).is_none());
        assert!(InfiniteLine { p0: p(0.0, 0.0), p1: p(1.0, 1.0) }.clip(Rect::new(0.0, 0.0, bad, 1.0)).is_none());
    }
    // For the row itself: equal points still make no path and no panic.
    let built = build_plain(&[infinite(1.0, 1.0, (1.0, 1.0))]);
    assert_eq!(built.infinite_lines.len(), 1);
    assert!(built.stroke.elements().is_empty());
    let nan = build_plain(&[infinite(f64::NAN, 1.0, (1.0, 1.0))]);
    assert!(nan.infinite_lines.is_empty());
    assert_eq!(nan.warnings[0].kind, WarningKind::NonFinite);
}

/// The clip of two points and a window given as corners.
fn clip(p0: (f64, f64), p1: (f64, f64), window: ((f64, f64), (f64, f64))) -> Option<(diacraft_geom::kurbo::Point, diacraft_geom::kurbo::Point)> {
    let line = InfiniteLine { p0: p(p0.0, p0.1), p1: p(p1.0, p1.1) };
    line.clip(Rect::new((window.0).0, (window.0).1, (window.1).0, (window.1).1)).map(|l| (l.p0, l.p1))
}

/// Given the points (2, -1E308) and (2, 1E308) and a window from (0, 0) to (4, 4), expect the segment from (2, 0) to (2, 4).
/// Naive: the y difference, 2E308, overflows to infinity, an end becomes NaN, and nothing is drawn. The ends are exact: the line is vertical and the window's
/// edges are the ends, whatever the size of the numbers it was found through.
#[test]
fn inf_line_a_difference_past_the_largest_number_still_draws_the_line() {
    let (a, b) = clip((2.0, -1e308), (2.0, 1e308), ((0.0, 0.0), (4.0, 4.0))).expect("the line crosses the window");
    assert_eq!((a, b), (p(2.0, 0.0), p(2.0, 4.0)));
    // The same in the other axis and the other way round: the ends follow the direction from the first point to the second.
    let (a, b) = clip((1e308, 2.0), (-1e308, 2.0), ((0.0, 0.0), (4.0, 4.0))).expect("the line crosses the window");
    assert_eq!((a, b), (p(4.0, 2.0), p(0.0, 2.0)));
    // A difference past the largest number in both coordinates, and no overflow of the numbers themselves: the line y = x, through a window it crosses.
    let (a, b) = clip((-1e308, -1e308), (1e308, 1e308), ((-4.0, -2.0), (6.0, 8.0))).expect("the line crosses the window");
    // The window is 1E-308 of the size of the numbers the line is given by, so where on the line it lies is not known to its own size: each end is on an edge and in the window.
    for end in [a, b] {
        assert!(end.x == -4.0 || end.x == 6.0 || end.y == -2.0 || end.y == 8.0, "an end is on an edge of the window: {end:?}");
        assert!((-4.0..=6.0).contains(&end.x) && (-2.0..=8.0).contains(&end.y), "an end is in the window: {end:?}");
    }
    // The largest numbers there are, in a window as wide as they can be.
    let (a, b) = clip((f64::MAX, 0.5), (f64::MAX / 2.0, 0.5), ((f64::MIN, 0.0), (f64::MAX, 1.0))).expect("the line crosses the window");
    assert_eq!((a, b), (p(f64::MAX, 0.5), p(f64::MIN, 0.5)));
}

/// Given the points (0, 0.5) and (1E-9, 0.5) and a window from (-2E299, 0) to (2E299, 1), expect the segment from (-2E299, 0.5)
/// to (2E299, 0.5). Naive: the window's half-width over the 1E-9 difference, 2E308, overflows to infinity, and nothing is drawn.
#[test]
fn inf_line_a_difference_tiny_next_to_the_window_still_draws_the_line() {
    let (a, b) = clip((0.0, 0.5), (1e-9, 0.5), ((-2e299, 0.0), (2e299, 1.0))).expect("the line crosses the window");
    assert_eq!((a, b), (p(-2e299, 0.5), p(2e299, 0.5)));
    // The smallest difference there is, either way, and in either axis.
    let (a, b) = clip((0.0, 0.5), (5e-324, 0.5), ((-1e308, 0.0), (1e308, 1.0))).expect("the line crosses the window");
    assert_eq!((a, b), (p(-1e308, 0.5), p(1e308, 0.5)));
    let (a, b) = clip((0.5, 5e-324), (0.5, 0.0), ((0.0, -1e308), (1.0, 1e308))).expect("the line crosses the window");
    assert_eq!((a, b), (p(0.5, 1e308), p(0.5, -1e308)));
    // A line that misses the window by a whisker still misses it.
    assert!(clip((0.0, 1.5), (1e-9, 1.5), ((-2e299, 0.0), (2e299, 1.0))).is_none());
}

/// Given the points of and a window from (-1E299, 0) to (1E299, 1), expect the segment from (-1E299, 0.5) to (1E299, 0.5).
/// Here the quotient is 1E308 and fits, so this one passes whether or not the case is handled; it pins the boundary, and a fix that loses it is wrong.
#[test]
fn inf_line_the_boundary_before_the_quotient_overflows_still_draws_the_line() {
    let (a, b) = clip((0.0, 0.5), (1e-9, 0.5), ((-1e299, 0.0), (1e299, 1.0))).expect("the line crosses the window");
    assert_eq!((a, b), (p(-1e299, 0.5), p(1e299, 0.5)));
}

/// For a line given by one point far from the window and one near it: the line passes through the near point, and the far one only gives its direction.
/// The window here is 1E-308 of the far point's size, so a line made from the far point would be put anywhere in it; made from the near point it is true.
#[test]
fn inf_line_a_far_point_only_gives_the_direction() {
    let (a, b) = clip((1e308, 1e308), (0.6, 0.05), ((0.5, 0.0), (1.0, 0.1))).expect("the near point is in the window");
    // The line y = x - 0.55, walked from the far point to the near one: it enters at (0.65, 0.1) and leaves at (0.55, 0).
    assert_pt(a, 0.65, 0.1, 1e-12, "entry");
    assert_pt(b, 0.55, 0.0, 1e-12, "exit");
}

/// A point in the window is on the answer, whatever the other point is and however the rounding of the numbers falls. Found by the property test of `hostile.rs` on 2026-11-20: the
/// first has a point 1E299 away, which the line was once worked from and lost the window by (the end came out at (0, 0), outside it); the second has a point 5E-324 outside the
/// window, whose direction to a point inside rounds to straight, so that a line worked from the first never reaches it.
#[test]
fn inf_line_a_point_in_the_window_is_on_the_answer() {
    let (a, b) = clip((1e299, 0.0), (0.0, 0.0), ((10.11487219813453, 0.0), (16.361474637337892, 0.0))).expect("the second point is on the window");
    assert_eq!((a, b), (p(16.361474637337892, 0.0), p(10.11487219813453, 0.0)));
    let inside = (-2.2250738585072014e-308, 9007199254740992.0);
    let found = clip((5e-324, 0.0), inside, ((0.0, -1e308), (-28.908492524965116, 1e299)));
    let (a, b) = found.expect("the second point is in the window");
    assert!(a.x <= 0.0 && a.x >= -28.908492524965116 && b.x <= 0.0 && b.x >= -28.908492524965116, "in the window: {a:?} {b:?}");
}

/// The review's: a direction whose smaller part is under 2^-1074 of the larger part (5E-324 over 4) is not along an axis, and a window that is off that axis
/// is still crossed: the line leaves the axis by 1E-24 over 1E300. Scaling the larger part to [1, 2) turned the smaller part to 0 and lost the line, where main kept it. The ends
/// are the exact ones (worked in rationals) to 1E-9, and each has its edge's own coordinate.
#[test]
fn inf_line_a_direction_with_a_part_under_the_smallest_number_still_crosses_the_window() {
    let near = |actual: f64, expected: f64, what: &str| {
        assert!((actual - expected).abs() <= 1e-9 * expected.abs(), "{what}: {actual:e} is not {expected:e} within 1E-9 of itself");
    };
    // The line x = (5E-324 / 4) y through the origin, over a window from x 1E-300 and y 0 to x 1 and y 1E300.
    let (a, b) = clip((0.0, 0.0), (5e-324, 4.0), ((1e-300, 0.0), (1.0, 1e300))).expect("The line crosses the window");
    assert_eq!(a.x, 1e-300, "The entry is on the window's left edge");
    near(a.y, 8.096090132292425e23, "R1 entry y");
    assert_eq!(b.y, 1e300, "The exit is on the window's top edge");
    near(b.x, 1.2351641146031164e-24, "R1 exit x");
    // The same with the axes turned: x and y exchanged in every number.
    let (a, b) = clip((0.0, 0.0), (4.0, 5e-324), ((0.0, 1e-300), (1e300, 1.0))).expect("R1 turned: the line crosses the window");
    assert_eq!(a.y, 1e-300);
    near(a.x, 8.096090132292425e23, "R1 turned entry x");
    assert_eq!(b.x, 1e300);
    near(b.y, 1.2351641146031164e-24, "R1 turned exit y");
    // The line through (0, 0.5) with the direction (5E-324, 2), over a window whose left edge is 1E-310.
    let (a, b) = clip((0.0, 0.5), (5e-324, 2.5), ((1e-310, 0.0), (1.0, 1e300))).expect("The line crosses the window");
    assert_eq!(a.x, 1e-310, "The entry is on the window's left edge");
    near(a.y, 40480450661462.5, "R2 entry y");
    assert_eq!(b.y, 1e300, "The exit is on the window's top edge");
    near(b.x, 2.470328229206233e-24, "R2 exit x");
    // Walked the other way it is the same segment the other way round, and a window the line never reaches is still missed (1E-24 at 1E300 does not reach x = 1).
    let (c, d) = clip((5e-324, 2.5), (0.0, 0.5), ((1e-310, 0.0), (1.0, 1e300))).expect("R2 reversed");
    assert_eq!((c, d), (b, a));
    assert!(clip((0.0, 0.0), (5e-324, 4.0), ((1e-10, 0.0), (1.0, 1e300))).is_none(), "x never reaches 1E-10 inside the window's y");
}

/// F2: where a number passes a quarter of the largest the positions are carried at a quarter of their size, and a position of one or two of the smallest
/// numbers (5E-324 each) was turned to 0 by that. A line was lost, or cut to one point, where main kept its chord. The three inputs the re-review gave: a window edge of one unit and the
/// points 0 and 1E308; a point of two units and a window from one end of the line to the other; and a window whose x edge is one unit, with a point at the end of the range.
#[test]
fn inf_line_a_quartered_scale_keeps_a_position_of_a_few_subnormal_units() {
    // 1. The line y = 5E-324 x / 1E308, over a window from x 1E300 to 1E307 and y 0 to 5E-324 (the line is under it by 5E-325 or less, which is 0 as a number).
    let (a, b) = clip((0.0, 0.0), (1e308, 5e-324), ((1e300, 0.0), (1e307, 5e-324))).expect("1: the line crosses the window");
    assert_eq!((a, b), (p(1e300, 0.0), p(1e307, 0.0)));
    // 2. The whole of the chord, from one point to the other. The first end's y is 2 units (the point's), to within the unit a quarter of it loses.
    let (a, b) =
        clip((-8.98846567431158e307, 1e-323), (0.0, 0.0), ((-8.98846567431158e307, 0.0), (0.0, 2e-323))).expect("2: the line crosses the window");
    assert_eq!((a.x, b), (-8.98846567431158e307, p(0.0, 0.0)));
    assert!((a.y - 1e-323).abs() <= 1e-323, "2: the first end is the point's y, 2 units, to within 2 units: {a:?}");
    // 3. A line that leaves x = 0 by 1 unit over 9E307 of y, and a window whose x edge is 1 unit.
    let found = clip(
        (-0.0, -8.988465674311579e307),
        (5e-324, 2.074291266580556e284),
        ((5e-324, 1.0414828795956993e-187), (-8.988465674311579e307, -1.7976931348623157e308)),
    );
    let (a, b) = found.expect("3: the line crosses the window");
    assert_eq!(a.y, -1.7976931348623157e308, "3: the first end is on the window's bottom edge");
    // The second end is at x 1 unit and y 1E-187 or 0 or the like, which is 0 at a scale of 1E308 (it is on the line to within 1E-12 of that), and not the -9E307 it was lost to.
    assert!(
        (b.y - 1.0414828795956993e-187).abs() <= 1e-12 * 8.988465674311579e307,
        "3: the second end is at the window's top, at about 1E-187 and not -9E307: {b:?}"
    );
    assert!(a.x.abs() <= 5e-324 && b.x.abs() <= 5e-324, "3: the line is at x 0 to 1 unit: {a:?} {b:?}");
}

/// The scale a window needs is the largest of the numbers of the line and of the window, and the window is not the points: a window as wide as the numbers go, around points that are
/// each a fifth of the largest, has a distance from its edge to a point of more than the largest number (-2.2E308, for the edge -1.797E308 and a point at 3.6E307). Worked at that scale the distance is
/// infinite, an end has no place, and the line is lost, unless the scale was taken from the window too.
#[test]
fn inf_line_a_window_wider_than_the_points_sets_the_scale() {
    let fifth = f64::MAX / 5.0;
    // The line y = 0.5 through two points a fifth and a tenth of the largest number from the origin, over the widest window there is.
    let window = ((f64::MIN, 0.0), (f64::MAX, 1.0));
    let (a, b) = clip((fifth, 0.5), (fifth / 2.0, 0.5), window).expect("a horizontal line through the whole window");
    assert_eq!((a, b), (p(f64::MAX, 0.5), p(f64::MIN, 0.5)), "from the first point's side to the second's");
    // The other way round, and in y.
    let (a, b) = clip((fifth / 2.0, 0.5), (fifth, 0.5), window).expect("the same line the other way");
    assert_eq!((a, b), (p(f64::MIN, 0.5), p(f64::MAX, 0.5)));
    let (a, b) = clip((0.5, -fifth), (0.5, -fifth / 2.0), ((0.0, f64::MIN), (1.0, f64::MAX))).expect("a vertical line through the whole window");
    assert_eq!((a, b), (p(0.5, f64::MIN), p(0.5, f64::MAX)));
}

/// Found by the property test of `hostile.rs` at 4.8 million cases, on the way: the part of the direction that is the smaller (here the x, 0.9999999999999999 beside a y of
/// 1.225) may have a distance over it that passes the largest number at full size where it does not at a quarter (-1.8E308 over 0.9999999999999999 is -1.8E308 and a bit, a quarter of
/// that is -4.5E307), and the larger part's distances are up to half the largest number at the quarter. Divided at full size and then scaled, the x edge was infinite, which put it
/// after the y edge and the end on the wrong edge: (-MAX, -MAX) for (-MAX, -1.75E308). The window is the quarter of the plane under and left of the origin.
#[test]
fn inf_line_a_distance_that_passes_the_largest_number_only_at_full_size_is_taken_at_the_quarter() {
    let max = f64::MAX;
    let (a, b) =
        clip((0.0, 4.4942328371557893e307), (4.4942328371557893e307, 1e308), ((0.0, -max), (-max, 0.0))).expect("the line crosses the window");
    // Entering at the left edge, to within a rounding of the largest number, and leaving at the top edge.
    assert_eq!(a.x, -max, "the first end is on the left edge");
    assert!((a.y - -1.7528835814221054e308).abs() <= 1e-12 * max, "the first end is at y -1.75E308, and not on the bottom edge: {a:?}");
    assert_eq!(b.y, 0.0, "the second end is on the top edge");
    assert!((b.x - -3.6685403136690717e307).abs() <= 1e-12 * max, "the second end is at x -3.67E307: {b:?}");
}

/// Ordinary numbers and a position of one or two subnormal units do not meet a quarter scale (it is for a number past a quarter of the largest, and 5E-324 is not that). A line along an
/// axis at y = 5E-324 keeps that y exactly to its ends, in a window two units high. (This is the one an always-quartered scale got wrong: a unit of a quarter is 0.)
#[test]
fn inf_line_an_ordinary_line_at_a_subnormal_height_keeps_its_height() {
    let (a, b) = clip((0.0, 5e-324), (1.0, 5e-324), ((-1.0, 0.0), (2.0, 1e-323))).expect("a horizontal line in the window");
    assert_eq!((a, b), (p(-1.0, 5e-324), p(2.0, 5e-324)));
    let (a, b) = clip((5e-324, 0.0), (5e-324, 1.0), ((0.0, -1.0), (1e-323, 2.0))).expect("a vertical line in the window");
    assert_eq!((a, b), (p(5e-324, -1.0), p(5e-324, 2.0)));
    // A slanted one through a window of 2 units: y = 1.5 x over x from 0, with its first end on the window's x = 0 edge at 0 and its second on the y = 1E-323 edge.
    let (a, b) = clip((0.0, 0.0), (1.0, 1.5), ((0.0, 0.0), (1.0, 1e-323))).expect("a slant through a window of two units");
    assert_eq!(a, p(0.0, 0.0));
    assert_eq!(b.y, 1e-323);
    assert!((b.x - 1e-323 / 1.5).abs() <= 5e-324, "the exit is at x = y / 1.5: {b:?}");
}

/// Q6 is open and the behaviour is not changed: points that differ by rounding noise give the line at the noise's angle, as they did.
#[test]
fn inf_line_points_that_differ_by_noise_still_give_the_line_at_the_noise_angle() {
    let (a, b) = clip((0.0, 0.5), (1e-300, 0.5), ((-1.0, 0.0), (1.0, 1.0))).expect("a horizontal line");
    assert_pt(a, -1.0, 0.5, 1e-12, "left");
    assert_pt(b, 1.0, 0.5, 1e-12, "right");
    let (a, b) = clip((0.0, 0.5), (1e-300, 0.500_000_000_000_000_1), ((-1.0, 0.0), (1.0, 1.0))).expect("an almost vertical line");
    assert_eq!((a.y, b.y), (0.0, 1.0), "from the window's bottom to its top");
    assert!(a.x.abs() < 1e-200 && b.x.abs() < 1e-200, "and almost vertical: {a:?} {b:?}");
    assert!(a.x < 0.0 && b.x > 0.0);
}

/// Review round 2, finding 3 and 4: an `Ellipse` whose arithmetic overflows (a half-diameter whose length is not a number, or control points that pass the largest number) says
/// so, with `Overflow`, and draws nothing: unlike an arc it has no chord to fall back to, and the warning's text says which. Deleting that warning passed every test.
#[test]
fn an_ellipse_whose_arithmetic_overflows_is_skipped_with_a_warning_that_says_so() {
    let big = f64::MAX / 2.0 * 1.5;
    for row in [
        // The half-diameter's length overflows.
        ellipse(0.0, 0.0, (big, big), (0.0, 1.0)),
        // The pen and the control points pass the largest number.
        ellipse(1.7e308, 0.0, (1.79e308, 0.0), (1.7e308, 9e306)),
    ] {
        let built = build_plain(&[mv(0.0, 0.0), ln(1.0, 0.0), row.clone(), ln(2.0, 0.0)]);
        let overflow: Vec<_> = built.warnings.iter().filter(|w| w.kind == WarningKind::Overflow).collect();
        assert_eq!(overflow.len(), 1, "{row:?}: {:?}", built.warnings);
        assert_eq!(overflow[0].row, 2);
        assert!(built.stroke.is_finite() && built.fill.is_finite());
        // Nothing of the ellipse is on the path: the line before it and the line after it are.
        assert_eq!(segments(&built.stroke).len(), 2, "{row:?}");
    }
    // A difference that is infinite is not a number to begin with: `NonFinite`, and skipped.
    let infinite_difference = build_plain(&[ellipse(-1.7e308, 0.0, (1.7e308, 0.0), (0.0, 1.0))]);
    assert_eq!(infinite_difference.warnings.len(), 1);
    assert_eq!(infinite_difference.warnings[0].kind, WarningKind::NonFinite);
    assert!(infinite_difference.stroke.elements().is_empty());
}

/// Review round 2, finding 4: `Overflow` is not always "drawn as a straight line": an arc is, an ellipse row is skipped. The text says both.
#[test]
fn the_overflow_warning_says_what_happened_to_an_arc_and_to_an_ellipse() {
    let text = WarningKind::Overflow.to_string();
    assert!(text.contains("straight line"), "{text}");
    assert!(text.contains("ellipse") && text.contains("skipped"), "{text}");
}
