//! `EllipticalArcTo` and `RelEllipticalArcTo`.

use diacraft_geom::kurbo::{ParamCurve, PathSeg, Point};
use diacraft_geom::{ArcFallback, GeomRow, WarningKind, elliptical_arc};

use crate::{CURVE_EPS, EPS, assert_near, assert_pt, build, build_plain, extents, mv, passes_through, segments, vertices, x_at_y};

fn earc(x: f64, y: f64, mid: (f64, f64), angle: f64, ratio: f64) -> GeomRow {
    GeomRow::EllipticalArcTo { x, y, mid_x: mid.0, mid_y: mid.1, angle, ratio }
}

fn pt(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

/// Given a pen at (3, 0), `X` -3, `Y` 0, `A` 0, `B` 1, `C` 0, `D` 3, expect the upper half of the ellipse centred at the origin
/// with half-axes 3 and 1, extents from x -3 to 3 and y 0 to 1. Reading `D` as 1/3 passes the same three points on an ellipse centred at (0, -40),
/// with half-axes about 13.67 and 41. The extents match, and x at y 0.5 tells them apart: about 2.598 on the right ellipse and about 2.128 on the wrong one.
#[test]
fn earc_d_is_the_length_of_the_axis_along_c_over_the_other() {
    let a = elliptical_arc(pt(3.0, 0.0), pt(0.0, 1.0), pt(-3.0, 0.0), 0.0, 3.0).unwrap();
    assert_pt(a.center, 0.0, 0.0, EPS, "centre");
    assert_near(a.radii.x, 3.0, EPS, "half-axis along the axis");
    assert_near(a.radii.y, 1.0, EPS, "half-axis across it");
    assert_near(a.rotation, 0.0, EPS, "rotation");

    let built = build_plain(&[mv(3.0, 0.0), earc(-3.0, 0.0, (0.0, 1.0), 0.0, 3.0)]);
    let e = extents(&built.stroke);
    assert_near(e.x0, -3.0, CURVE_EPS, "x0");
    assert_near(e.x1, 3.0, CURVE_EPS, "x1");
    assert_near(e.y0, 0.0, CURVE_EPS, "y0");
    assert_near(e.y1, 1.0, CURVE_EPS, "y1");
    let right = x_at_y(&built.stroke, 0.5);
    assert!(right.iter().any(|x| (x - 2.598).abs() < 1e-3), "x at y 0.5 is about 2.598: {right:?}");
    assert!(!right.iter().any(|x| (x - 2.128).abs() < 1e-2));

    // The wrong ratio (D read as 1/3) is a different ellipse through the same three points, as the spec says.
    let wrong = elliptical_arc(pt(3.0, 0.0), pt(0.0, 1.0), pt(-3.0, 0.0), 0.0, 1.0 / 3.0).unwrap();
    assert_pt(wrong.center, 0.0, -40.0, 1e-6, "the wrong centre");
    assert_near(wrong.radii.x, 13.67, 0.01, "the wrong half-axis along x");
    assert_near(wrong.radii.y, 41.0, 1e-6, "the wrong half-axis along y");
}

/// Given `A` 0 and `B` -1 instead, the same ends, expect the lower half. An arc always taken counterclockwise gives the upper half,
/// and misses the point.
#[test]
fn earc_the_arc_goes_through_the_point_whichever_way_that_is() {
    let built = build_plain(&[mv(3.0, 0.0), earc(-3.0, 0.0, (0.0, -1.0), 0.0, 3.0)]);
    let e = extents(&built.stroke);
    assert_near(e.y0, -1.0, CURVE_EPS, "lowest y");
    assert_near(e.y1, 0.0, CURVE_EPS, "highest y");
    assert!(passes_through(&built.stroke, pt(0.0, -1.0), CURVE_EPS), "through the point");
    let a = elliptical_arc(pt(3.0, 0.0), pt(0.0, -1.0), pt(-3.0, 0.0), 0.0, 3.0).unwrap();
    assert!(a.sweep < 0.0, "clockwise in this frame");
    let up = elliptical_arc(pt(3.0, 0.0), pt(0.0, 1.0), pt(-3.0, 0.0), 0.0, 3.0).unwrap();
    assert!(up.sweep > 0.0, "counterclockwise in this one");
}

/// Given a pen at (3 cos 30, 3 sin 30), an end at the negative of that, a point at (-sin 30, cos 30), `C` 30 degrees and `D` 3,
/// expect the ellipse centred at the origin with half-axes 3 along 30 degrees and 1 across, and a top at about y 1.7321. Ignoring `C` gives a centre near
/// (5, -0.96) and half-axes near 7.77 and 2.59. Using -30 degrees gives a centre near (21.7, -14.4).
#[test]
fn earc_c_turns_the_axis_counterclockwise_in_the_shapes_frame() {
    let (s, c) = (30.0_f64.to_radians().sin(), 30.0_f64.to_radians().cos());
    let (start, end, mid) = (pt(3.0 * c, 3.0 * s), pt(-3.0 * c, -3.0 * s), pt(-s, c));
    let c30 = 30.0_f64.to_radians();
    let a = elliptical_arc(start, mid, end, c30, 3.0).unwrap();
    assert_pt(a.center, 0.0, 0.0, EPS, "centre");
    assert_near(a.radii.x, 3.0, EPS, "half-axis along 30 degrees");
    assert_near(a.radii.y, 1.0, EPS, "half-axis across");
    assert_near(a.rotation, c30, EPS, "the axis is at C");

    let built = build_plain(&[mv(start.x, start.y), earc(end.x, end.y, (mid.x, mid.y), c30, 3.0)]);
    assert_near(extents(&built.stroke).y1, 1.7321, 1e-4, "the top");
    assert!(passes_through(&built.stroke, mid, CURVE_EPS), "through the point");

    let ignoring_c = elliptical_arc(start, mid, end, 0.0, 3.0).unwrap();
    assert_pt(ignoring_c.center, 5.0, -0.96, 0.01, "centre when C is ignored");
    assert_near(ignoring_c.radii.x, 7.77, 0.01, "half-axis when C is ignored");
    assert_near(ignoring_c.radii.y, 2.59, 0.01, "half-axis when C is ignored");
    let minus_30 = elliptical_arc(start, mid, end, -c30, 3.0).unwrap();
    assert_pt(minus_30.center, 21.7, -14.4, 0.05, "centre when the turn is -30 degrees");
}

/// Given `D` 0.5, a pen at (1, 0), an end at (-1, 0), a point at (0, 2), `C` 0, expect half-axes 1 along x and 2 along y, a valid
/// ellipse. A rule requiring `D` of at least 1 rejects it.
#[test]
fn earc_a_ratio_below_one_is_valid() {
    let a = elliptical_arc(pt(1.0, 0.0), pt(0.0, 2.0), pt(-1.0, 0.0), 0.0, 0.5).unwrap();
    assert_pt(a.center, 0.0, 0.0, EPS, "centre");
    assert_near(a.radii.x, 1.0, EPS, "along x");
    assert_near(a.radii.y, 2.0, EPS, "along y");
    let built = build_plain(&[mv(1.0, 0.0), earc(-1.0, 0.0, (0.0, 2.0), 0.0, 0.5)]);
    assert!(built.warnings.is_empty());
    assert_near(extents(&built.stroke).y1, 2.0, CURVE_EPS, "top");
}

/// Given a pen at (0, 0), an end at (2, 0), a point at (1, 0), `C` 0 and `D` 1, expect the straight chord, with no division by zero. A circle
/// fit gives infinity.
#[test]
fn earc_three_points_on_a_line_draw_the_chord() {
    assert_eq!(elliptical_arc(pt(0.0, 0.0), pt(1.0, 0.0), pt(2.0, 0.0), 0.0, 1.0).unwrap_err(), ArcFallback::NoEllipse);
    let built = build_plain(&[mv(0.0, 0.0), earc(2.0, 0.0, (1.0, 0.0), 0.0, 1.0)]);
    let segs = segments(&built.stroke);
    assert_eq!(segs.len(), 1);
    assert!(matches!(segs[0], PathSeg::Line(_)));
    assert_pt(segs[0].end(), 2.0, 0.0, EPS, "end");
    assert!(built.warnings.is_empty());
    // On one line once scaled, though not on a line parallel to an axis.
    assert_eq!(elliptical_arc(pt(0.0, 0.0), pt(1.0, 2.0), pt(3.0, 6.0), 0.3, 2.0).unwrap_err(), ArcFallback::NoEllipse);
}

/// Given `RelEllipticalArcTo`, `Width` 4, `Height` 2, a pen at (0, 1), `X` 1, `Y` 0.5, `A` 0.5, `B` 1, `C` 0 and `D` 2, expect the
/// three points (0, 1), (4, 1), (2, 2) in the shape's frame and half-axes 2 along x and 1 along y, centre (2, 1). Working in share space and scaling afterwards
/// gives half-axes 2.5 and 0.625 about the centre (2, 1.375). Scaling `B` by `Width`, as one line of the page says, puts the point at (2, 4).
#[test]
fn earc_a_relative_arc_scales_its_points_and_not_c_or_d() {
    let row = GeomRow::RelEllipticalArcTo { x: 1.0, y: 0.5, mid_x: 0.5, mid_y: 1.0, angle: 0.0, ratio: 2.0 };
    let built = build(&[mv(0.0, 1.0), row], 4.0, 2.0);
    assert!(built.warnings.is_empty());
    let v = vertices(&built.stroke);
    assert_pt(v[0], 0.0, 1.0, EPS, "start");
    assert_pt(*v.last().unwrap(), 4.0, 1.0, EPS, "end");
    assert!(passes_through(&built.stroke, pt(2.0, 2.0), CURVE_EPS), "through (2, 2)");
    let e = extents(&built.stroke);
    assert_near(e.y1, 2.0, CURVE_EPS, "top");
    assert_near(e.x0, 0.0, CURVE_EPS, "left");
    assert_near(e.x1, 4.0, CURVE_EPS, "right");
    let a = elliptical_arc(pt(0.0, 1.0), pt(2.0, 2.0), pt(4.0, 1.0), 0.0, 2.0).unwrap();
    assert_pt(a.center, 2.0, 1.0, EPS, "centre");
    assert_near(a.radii.x, 2.0, EPS, "half-axis along x");
    assert_near(a.radii.y, 1.0, EPS, "half-axis along y");
    // The naive point, B scaled by Width, is (2, 4): the arc would pass there and not through (2, 2).
    assert!(!passes_through(&built.stroke, pt(2.0, 4.0), 0.1));
}

/// Given `D` of 2 on `RelEllipticalArcTo`, expect a ratio with no scaling by `Height`. Scaling gives another ratio:
/// here the same row on a taller shape keeps its ratio of 2, so the half-axes follow the points alone.
#[test]
fn d_geometry_the_ratio_of_a_relative_row_is_not_scaled() {
    let row = GeomRow::RelEllipticalArcTo { x: 1.0, y: 0.5, mid_x: 0.5, mid_y: 1.0, angle: 0.0, ratio: 2.0 };
    // At Width 4 and Height 4 the points are (0, 2), (4, 2), (2, 4). With D unscaled (2) the ellipse has half-axes 2.5 and 1.25 about (2, 2.75).
    let built = build(&[GeomRow::MoveTo { x: 0.0, y: 2.0 }, row], 4.0, 4.0);
    assert!(built.warnings.is_empty());
    assert_near(extents(&built.stroke).y1, 4.0, CURVE_EPS, "top, at the point on the arc");
    let a = elliptical_arc(pt(0.0, 2.0), pt(2.0, 4.0), pt(4.0, 2.0), 0.0, 2.0).unwrap();
    assert_pt(a.center, 2.0, 2.75, EPS, "centre");
    assert_near(a.radii.x, 2.5, EPS, "half-axis along x");
    assert_near(a.radii.y, 1.25, EPS, "half-axis along y");
    // Scaled by Height (4) the ratio would be 8: a much longer, flatter ellipse through the same points.
    let scaled = elliptical_arc(pt(0.0, 2.0), pt(2.0, 4.0), pt(4.0, 2.0), 0.0, 8.0).unwrap();
    assert!(scaled.radii.x > 8.0, "naive: another ratio gives half-axes {:?}", scaled.radii);
}

/// Given a shape turned by `Angle` 90 degrees, with an arc whose `C` is 0 and `D` is 2, expect the same local curve as when unturned, and
/// then the shape's turn on top. Subtracting `Angle` from `C` gives another ellipse.
///
/// Only a part of this vector is `geom`'s, and it is not the part the vector names: `geom` takes no `Angle`, so it cannot subtract one, and "the same local
/// curve, then the shape's turn on top" is Task 8's. What is checked here is that `C` is used as given, in the local frame (the arc's rotation is `C`), and that
/// another `C` is another ellipse, which is what the naive reading would give.
#[test]
fn earc_c_is_used_as_given_in_the_shapes_own_frame() {
    let local = elliptical_arc(pt(1.0, 0.0), pt(0.0, 1.0), pt(-1.0, 0.0), 0.0, 2.0).unwrap();
    assert_near(local.rotation, 0.0, EPS, "the axis stays at C");
    let naive = elliptical_arc(pt(1.0, 0.0), pt(0.0, 1.0), pt(-1.0, 0.0), -std::f64::consts::FRAC_PI_2, 2.0).unwrap();
    assert!(naive.center.distance(local.center) > 0.1 || (naive.radii - local.radii).hypot() > 0.1, "naive: C minus the angle is another ellipse");
}

/// Given `D` 0, a pen at (2, 0), an end at (2, 3), a point at (2, 1), expect the chord. Dividing by `D` gives no number. On a collapsed
/// shape (zero `Width`), another reader's account (the rule here) visits the point on the arc first.
#[test]
fn earc_a_ratio_of_zero_draws_the_chord_or_visits_the_point_on_a_collapsed_shape() {
    let rows = [mv(2.0, 0.0), earc(2.0, 3.0, (2.0, 1.0), 0.0, 0.0)];
    let positive = build(&rows, 2.0, 2.0);
    let segs = segments(&positive.stroke);
    assert_eq!(segs.len(), 1);
    assert!(matches!(segs[0], PathSeg::Line(_)));
    assert_pt(segs[0].end(), 2.0, 3.0, EPS, "end");
    assert!(positive.warnings.is_empty());
    for (w, h) in [(0.0, 2.0), (2.0, 0.0), (0.0, 0.0)] {
        let collapsed = build(&rows, w, h);
        let v = vertices(&collapsed.stroke);
        assert_eq!(v.len(), 3, "size {w} by {h}: the pen, the point on the arc, the end");
        assert_pt(v[1], 2.0, 1.0, EPS, "the point on the arc");
        assert_pt(v[2], 2.0, 3.0, EPS, "the end");
    }
    // The arc function itself reports a ratio of 0 as a ratio problem, not a number.
    assert_eq!(elliptical_arc(pt(2.0, 0.0), pt(2.0, 1.0), pt(2.0, 3.0), 0.0, 0.0).unwrap_err(), ArcFallback::Ratio);
}

/// Our rule: `D` negative or above 1000: keep `V`, warn, and draw the chord. 1000 itself is the page's limit and draws the arc.
#[test]
fn earc_an_unreliable_ratio_draws_the_chord_with_a_warning() {
    for ratio in [-1.0, -1e-9, 1000.5, 1e300, f64::MAX] {
        let built = build_plain(&[mv(3.0, 0.0), earc(-3.0, 0.0, (0.0, 1.0), 0.0, ratio)]);
        let segs = segments(&built.stroke);
        assert_eq!(segs.len(), 1, "ratio {ratio}");
        assert!(matches!(segs[0], PathSeg::Line(_)), "ratio {ratio}");
        assert_eq!(built.warnings.len(), 1, "ratio {ratio}");
        assert_eq!(built.warnings[0].kind, WarningKind::EllipticalRatio);
    }
    let at_limit = build_plain(&[mv(3.0, 0.0), earc(-3.0, 0.0, (0.0, 1.0), 0.0, 1000.0)]);
    assert!(at_limit.warnings.is_empty());
    assert!(!matches!(segments(&at_limit.stroke)[0], PathSeg::Line(_)));
}

/// Our rule: points that fix no ellipse draw the chord, which may have no length; no warning, since rounded corners whose radius has come to 0 do this
/// on tens of thousands of rows.
#[test]
fn earc_points_that_fix_no_ellipse_draw_the_chord() {
    // All three the same.
    let same = build_plain(&[mv(1.0, 1.0), earc(1.0, 1.0, (1.0, 1.0), 0.0, 2.0)]);
    assert!(same.warnings.is_empty());
    let segs = segments(&same.stroke);
    assert_eq!(segs.len(), 1);
    assert!(matches!(segs[0], PathSeg::Line(l) if l.p0 == l.p1));
    // Both ends the same with the point elsewhere: any ellipse through the two fits.
    let ends = build_plain(&[mv(1.0, 1.0), earc(1.0, 1.0, (2.0, 3.0), 0.0, 2.0)]);
    assert!(ends.warnings.is_empty());
    assert!(matches!(segments(&ends.stroke)[0], PathSeg::Line(_)));
    assert_eq!(elliptical_arc(pt(1.0, 1.0), pt(2.0, 3.0), pt(1.0, 1.0), 0.0, 2.0).unwrap_err(), ArcFallback::NoEllipse);
}

/// Three points a ten-thousandth of their span off a line are an arc: a mutation that raised `COLLINEAR_RATIO` to 1E-3 drew the chord and no test said so (review).
/// (The span 4, the point 0.0004 off the chord's middle: the area over the longest side squared is 1E-4.)
#[test]
fn three_points_barely_off_a_line_still_fix_an_ellipse() {
    let built = build_plain(&[mv(0.0, 0.0), earc(4.0, 0.0, (2.0, 0.0004), 0.0, 1.0)]);
    assert!(built.warnings.is_empty());
    assert!(segments(&built.stroke).iter().all(|s| matches!(s, PathSeg::Cubic(_))), "{:?}", built.stroke);
    assert_near(extents(&built.stroke).y1, 0.0004, 1e-7, "the arc's middle is 0.0004 from the chord");
}

/// Review (small items): points whose arithmetic overflows (coordinates of 1E300 square to infinity) draw the chord, as the fallback, and say so, where a chord of
/// points that fix no ellipse says nothing.
#[test]
fn an_elliptical_arc_whose_arithmetic_overflows_is_a_line_with_a_warning() {
    for coord in [1e200, 1e300, 1e308] {
        let built = build_plain(&[mv(0.0, 0.0), earc(coord, 0.0, (coord / 2.0, coord / 4.0), 0.7, 1.5)]);
        assert_eq!(built.warnings.len(), 1, "coordinate {coord}");
        assert_eq!(built.warnings[0].kind, WarningKind::Overflow);
        assert_eq!(built.warnings[0].row, 1);
        assert!(matches!(segments(&built.stroke)[0], PathSeg::Line(_)), "coordinate {coord}");
    }
    // The arc function says which: overflow is not the same as points on a line.
    assert_eq!(elliptical_arc(pt(0.0, 0.0), pt(5e199, 2.5e199), pt(1e200, 0.0), 0.7, 1.5).unwrap_err(), ArcFallback::Overflow);
    assert_eq!(elliptical_arc(pt(0.0, 0.0), pt(1.0, 0.0), pt(2.0, 0.0), 0.0, 1.0).unwrap_err(), ArcFallback::NoEllipse);
}

/// Given the row and a write of 0 to `C`, expect the three points to stay and the ellipse to change to centre about
/// (5, -0.96). Turning the arc gives a pure turn of the same curve.
#[test]
fn c_geometry_a_write_to_c_keeps_the_points_and_changes_the_ellipse() {
    let (s, c) = (30.0_f64.to_radians().sin(), 30.0_f64.to_radians().cos());
    let (start, end, mid) = (pt(3.0 * c, 3.0 * s), pt(-3.0 * c, -3.0 * s), pt(-s, c));
    let before = build_plain(&[mv(start.x, start.y), earc(end.x, end.y, (mid.x, mid.y), 30.0_f64.to_radians(), 3.0)]);
    let after = build_plain(&[mv(start.x, start.y), earc(end.x, end.y, (mid.x, mid.y), 0.0, 3.0)]);
    for built in [&before, &after] {
        let v = vertices(&built.stroke);
        assert_pt(v[0], start.x, start.y, EPS, "start stays");
        assert_pt(*v.last().unwrap(), end.x, end.y, EPS, "end stays");
        assert!(passes_through(&built.stroke, mid, CURVE_EPS), "the point on the arc stays");
    }
    let a = elliptical_arc(start, mid, end, 0.0, 3.0).unwrap();
    assert_pt(a.center, 5.0, -0.96, 0.01, "the ellipse after the write");
    assert_ne!(before.stroke, after.stroke);
}

/// Given `D` of 0.5, expect a valid ellipse, where requiring at least 1 refuses it.
#[test]
fn d_geometry_a_ratio_of_one_half_is_not_refused() {
    let built = build_plain(&[mv(1.0, 0.0), earc(-1.0, 0.0, (0.0, 2.0), 0.0, 0.5)]);
    assert!(built.warnings.is_empty());
    assert!(!matches!(segments(&built.stroke)[0], PathSeg::Line(_)));
}

/// The arc ends exactly at the row's end, so the next row joins with no gap, and a long arc (more than half a turn) goes the long way.
#[test]
fn an_elliptical_arc_ends_exactly_and_may_exceed_a_half_turn() {
    // Pen (2, 0), end (0, -2), through (-2, 0) on a circle of radius 2: three quarters of a turn counterclockwise.
    let rows = [mv(2.0, 0.0), earc(0.0, -2.0, (-2.0, 0.0), 0.0, 1.0)];
    let with_line = build_plain(&[rows[0].clone(), rows[1].clone(), GeomRow::LineTo { x: 5.0, y: 5.0 }]);
    let v = vertices(&with_line.stroke);
    assert_eq!(*v.last().unwrap(), pt(5.0, 5.0));
    assert_eq!(
        segments(&with_line.stroke).iter().rev().nth(1).unwrap().end(),
        pt(0.0, -2.0),
        "the arc ends exactly at its end, where the line starts"
    );
    let a = elliptical_arc(pt(2.0, 0.0), pt(-2.0, 0.0), pt(0.0, -2.0), 0.0, 1.0).unwrap();
    assert_near(a.sweep, 1.5 * std::f64::consts::PI, EPS, "three quarters of a turn");
    let e = extents(&build_plain(&rows).stroke);
    assert_near(e.x0, -2.0, CURVE_EPS, "leftmost");
    assert_near(e.y1, 2.0, CURVE_EPS, "top");
}

/// Hostile numbers: no panic, and the row is skipped when a number is not finite.
#[test]
fn hostile_elliptical_arcs_do_not_panic() {
    let skipped = build_plain(&[mv(0.0, 0.0), earc(1.0, 0.0, (0.5, f64::NAN), 0.0, 1.0), earc(1.0, 0.0, (0.5, 1.0), f64::INFINITY, 1.0)]);
    assert_eq!(skipped.warnings.len(), 2);
    assert!(skipped.warnings.iter().all(|w| w.kind == WarningKind::NonFinite));
    for ratio in [1e-300, 1e-12, 1e12, 5e-324] {
        for coord in [1e-300, 1e150, 1e300, 1e308] {
            let built = build_plain(&[mv(0.0, 0.0), earc(coord, 0.0, (coord / 2.0, coord / 4.0), 0.7, ratio)]);
            assert!(built.stroke.is_finite(), "ratio {ratio}, coordinate {coord}: {:?}", built.stroke);
        }
    }
}
