//! `SplineStart` and `SplineKnot`.

use diacraft_geom::kurbo::{ParamCurve, PathSeg};
use diacraft_geom::{CurveProblem, GeomRow, NurbsCurve, WarningKind};

use crate::{EPS, assert_pt, build_plain, extents, ln, mv, p, segments, vertices};

fn start(x: f64, y: f64, second_knot: f64, first_knot: f64, last_knot: f64, degree: f64) -> GeomRow {
    GeomRow::SplineStart { x, y, second_knot, first_knot, last_knot, degree }
}

fn knot(x: f64, y: f64, k: f64) -> GeomRow {
    GeomRow::SplineKnot { x, y, knot: k }
}

/// The rows: pen (0, 0), `SplineStart` `X` 1, `Y` 2, `A` 0, `B` 0, `C` 1, `D` 3, then `SplineKnot` (3, 2) `A` 0 and `SplineKnot` (4, 0) `A` 0.
fn rows_1() -> Vec<GeomRow> {
    vec![mv(0.0, 0.0), start(1.0, 2.0, 0.0, 0.0, 1.0, 3.0), knot(3.0, 2.0, 0.0), knot(4.0, 0.0, 0.0)]
}

/// Given a pen at (0, 0), `SplineStart` `X` 1, `Y` 2, `A` 0, `B` 0, `C` 1, `D` 3, then `SplineKnot` (3, 2) `A` 0 and `SplineKnot`
/// (4, 0) `A` 0, expect the same curve. Repeating `C` too few times stops the curve short of (4, 0).
#[test]
fn spline_a_run_is_the_same_curve_as_the_nurbs_row() {
    let rows = rows_1();
    let curve = NurbsCurve::from_spline(p(0.0, 0.0), &rows[1..]).unwrap();
    assert_eq!(curve.degree(), 3);
    assert_eq!(curve.points(), vec![p(0.0, 0.0), p(1.0, 2.0), p(3.0, 2.0), p(4.0, 0.0)]);
    assert_eq!(curve.knots(), vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0], "B, A, each knot's A, C, then C degree times");
    assert!(curve.weights().iter().all(|w| *w == 1.0));
    let built = build_plain(&rows);
    assert!(built.warnings.is_empty());
    let segs = segments(&built.stroke);
    assert_eq!(segs.len(), 1);
    let PathSeg::Cubic(c) = segs[0] else { panic!("not a cubic") };
    assert_pt(c.p1, 1.0, 2.0, EPS, "first control");
    assert_pt(c.p2, 3.0, 2.0, EPS, "second control");
    assert_pt(c.p3, 4.0, 0.0, EPS, "ends at the last control point");
    assert_pt(c.eval(0.5), 2.0, 1.5, EPS, "the top,");
}

/// Given that run and then `LineTo` (5, 5), expect the line to start at (4, 0). Starting it from the `SplineStart`'s (1, 2) draws a stray line.
#[test]
fn spline_the_pen_after_a_run_is_its_last_control_point() {
    let mut rows = rows_1();
    rows.push(ln(5.0, 5.0));
    let built = build_plain(&rows);
    let segs = segments(&built.stroke);
    assert_eq!(segs.len(), 2);
    assert_pt(segs[1].start(), 4.0, 0.0, EPS, "the line's start");
    assert_pt(segs[1].end(), 5.0, 5.0, EPS, "the line's end");
}

/// Given a run, then a second `SplineStart` straight after it, expect two splines, the second's first control point being the first's last.
/// Treating the second as a knot of the first gives one curve.
#[test]
fn spline_a_second_start_begins_a_new_spline_at_the_last_point() {
    let mut rows = rows_1();
    // A quadratic of three points: (4, 0), (5, 1), (6, 0), knots 0 0 0 1 (the last implied twice).
    rows.push(start(5.0, 1.0, 0.0, 0.0, 1.0, 2.0));
    rows.push(knot(6.0, 0.0, 0.0));
    let built = build_plain(&rows);
    assert!(built.warnings.is_empty(), "{:?}", built.warnings);
    let v = vertices(&built.stroke);
    assert_eq!(v.len(), 3, "two curves, joined at (4, 0): {v:?}");
    assert_pt(v[0], 0.0, 0.0, EPS, "first spline starts");
    assert_pt(v[1], 4.0, 0.0, EPS, "the first's last control point is the second's first");
    assert_pt(v[2], 6.0, 0.0, EPS, "second spline ends");
    let segs = segments(&built.stroke);
    let PathSeg::Cubic(second) = segs[1] else { panic!("not a cubic") };
    assert_pt(second.eval(0.5), 5.0, 0.5, EPS, "a quadratic's middle: (4 + 2 * 5 + 6) / 4 and (2 * 1) / 4");
}

/// Given the rows with `B`, `A` and both `SplineKnot` `A` cells at 2, and `C` 3, expect the same curve, over a domain of 2 to 3.
/// A reader that assumes a first knot of 0 gets a domain starting where no basis exists.
#[test]
fn spline_the_domain_follows_the_knots() {
    let rows = [mv(0.0, 0.0), start(1.0, 2.0, 2.0, 2.0, 3.0, 3.0), knot(3.0, 2.0, 2.0), knot(4.0, 0.0, 2.0)];
    let curve = NurbsCurve::from_spline(p(0.0, 0.0), &rows[1..]).unwrap();
    assert_eq!(curve.knots(), vec![2.0, 2.0, 2.0, 2.0, 3.0, 3.0, 3.0, 3.0]);
    assert_eq!(curve.domain(), (2.0, 3.0));
    assert_pt(curve.eval(2.5), 2.0, 1.5, EPS, "the same top");
    let same = build_plain(&rows_1());
    let shifted = build_plain(&rows);
    assert_eq!(same.stroke, shifted.stroke, "the same path");
}

/// Our rule: a stray `SplineKnot` is a straight line to its point, with a warning, and a lone `SplineStart` likewise.
#[test]
fn spline_strays_are_straight_lines_with_a_warning() {
    let stray = build_plain(&[mv(0.0, 0.0), knot(3.0, 3.0, 0.0), ln(4.0, 3.0)]);
    assert_eq!(stray.warnings.len(), 1);
    assert_eq!(stray.warnings[0].kind, WarningKind::SplineStray);
    assert_eq!(stray.warnings[0].row, 1);
    let v = vertices(&stray.stroke);
    assert_eq!(v, vec![p(0.0, 0.0), p(3.0, 3.0), p(4.0, 3.0)]);

    let lone = build_plain(&[mv(0.0, 0.0), start(1.0, 2.0, 0.0, 0.0, 1.0, 3.0), ln(4.0, 3.0)]);
    assert_eq!(lone.warnings.len(), 1);
    assert_eq!(lone.warnings[0].kind, WarningKind::SplineStray);
    assert_eq!(vertices(&lone.stroke), vec![p(0.0, 0.0), p(1.0, 2.0), p(4.0, 3.0)]);

    // With no pen a spline start is skipped like any row that needs one, and so are the knots after it.
    let no_pen = build_plain(&[start(1.0, 2.0, 0.0, 0.0, 1.0, 3.0), knot(3.0, 2.0, 0.0)]);
    assert_eq!(no_pen.warnings.len(), 2);
    assert!(no_pen.warnings.iter().all(|w| w.kind == WarningKind::NoPen));
    assert!(no_pen.stroke.elements().is_empty());
}

/// Keep any degree on reading, and refuse to draw degree 0, since a spline of degree 0 is no curve. The pen goes to the run's last point.
#[test]
fn degree_zero_is_refused_and_a_degree_beyond_the_points_too() {
    let zero = build_plain(&[mv(0.0, 0.0), start(1.0, 2.0, 0.0, 0.0, 1.0, 0.0), knot(3.0, 2.0, 0.0), knot(4.0, 0.0, 0.0), ln(5.0, 5.0)]);
    assert_eq!(zero.warnings.len(), 1);
    assert!(matches!(zero.warnings[0].kind, WarningKind::NurbsUnusable(_)));
    let segs = segments(&zero.stroke);
    assert_eq!(segs.len(), 1, "only the line after it");
    assert_pt(segs[0].start(), 4.0, 0.0, EPS, "the pen is at the run's last point");
    assert_eq!(NurbsCurve::from_spline(p(0.0, 0.0), &rows_1()[1..]).map(|c| c.degree()), Ok(3));
    let rows = [start(1.0, 2.0, 0.0, 0.0, 1.0, 0.0), knot(3.0, 2.0, 0.0)];
    assert_eq!(NurbsCurve::from_spline(p(0.0, 0.0), &rows).unwrap_err(), CurveProblem::BadDegree);
    let too_few = [start(1.0, 2.0, 0.0, 0.0, 1.0, 3.0), knot(3.0, 2.0, 0.0)];
    assert_eq!(NurbsCurve::from_spline(p(0.0, 0.0), &too_few).unwrap_err(), CurveProblem::TooFewPoints);
    assert_eq!(NurbsCurve::from_spline(p(0.0, 0.0), &[knot(1.0, 1.0, 0.0)]).unwrap_err(), CurveProblem::NotASplineRow);
    assert_eq!(NurbsCurve::from_spline(p(0.0, 0.0), &[]).unwrap_err(), CurveProblem::NotASplineRow);
}

/// Given a `SplineStart` with `D` of 3, expect a cubic, and with 2 a quadratic. Fixing the degree at 3 draws the second wrongly.
#[test]
fn d_geometry_the_degree_is_the_rows_d() {
    // Three control points (0, 0), (1, 2), (2, 0): a quadratic is a parabola with its middle at (1, 1); degree 3 cannot be drawn through three points.
    let rows = [mv(0.0, 0.0), start(1.0, 2.0, 0.0, 0.0, 1.0, 2.0), knot(2.0, 0.0, 0.0)];
    let curve = NurbsCurve::from_spline(p(0.0, 0.0), &rows[1..]).unwrap();
    assert_eq!(curve.degree(), 2);
    assert_pt(curve.eval(0.5), 1.0, 1.0, EPS, "the parabola's middle");
    let built = build_plain(&rows);
    assert!(built.warnings.is_empty());
    assert!((extents(&built.stroke).y1 - 1.0).abs() < 1e-9, "the parabola's top is 1, not the control point's 2");
    let cubic = [mv(0.0, 0.0), start(1.0, 2.0, 0.0, 0.0, 1.0, 3.0), knot(2.0, 0.0, 0.0)];
    let wrong = build_plain(&cubic);
    assert!(matches!(wrong.warnings[0].kind, WarningKind::NurbsUnusable(_)), "degree 3 through three points does not draw");
}

/// A spline run can follow a line, an arc or a move, and its last point is the pen for what comes next.
#[test]
fn a_run_after_a_line_starts_at_the_line_end() {
    let rows = [mv(0.0, 0.0), ln(2.0, 0.0), start(3.0, 2.0, 0.0, 0.0, 1.0, 2.0), knot(4.0, 0.0, 0.0), ln(4.0, -1.0)];
    let built = build_plain(&rows);
    assert!(built.warnings.is_empty());
    let v = vertices(&built.stroke);
    assert_eq!(*v.first().unwrap(), p(0.0, 0.0));
    assert_eq!(v[1], p(2.0, 0.0));
    assert_pt(v[2], 4.0, 0.0, EPS, "the spline ends at its last point");
    assert_eq!(*v.last().unwrap(), p(4.0, -1.0));
}

/// Review (small items): a number that is not finite in a `SplineStart` skips the whole run, as one in a `SplineKnot` does, and does not leave the knots after it as
/// stray straight lines (is about a knot that has no start, not about one whose start was skipped).
#[test]
fn a_start_that_is_not_a_number_skips_its_run() {
    for bad in [start(f64::NAN, 2.0, 0.0, 0.0, 1.0, 3.0), start(1.0, 2.0, f64::NAN, 0.0, 1.0, 3.0), start(1.0, 2.0, 0.0, 0.0, 1.0, f64::INFINITY)] {
        let built = build_plain(&[mv(0.0, 0.0), bad, knot(3.0, 2.0, 0.0), knot(4.0, 0.0, 0.0), ln(5.0, 5.0)]);
        assert_eq!(built.warnings.len(), 1, "{:?}", built.warnings);
        assert_eq!(built.warnings[0].row, 1);
        assert_eq!(built.warnings[0].kind, WarningKind::NonFinite);
        assert_eq!(vertices(&built.stroke), vec![p(0.0, 0.0), p(5.0, 5.0)], "the knots are not drawn as lines");
    }
    // The same for a knot that is not a number: the run, with its start, is skipped.
    let built = build_plain(&[mv(0.0, 0.0), start(1.0, 2.0, 0.0, 0.0, 1.0, 3.0), knot(f64::NAN, 2.0, 0.0), knot(4.0, 0.0, 0.0), ln(5.0, 5.0)]);
    assert_eq!(built.warnings.len(), 1);
    assert_eq!(built.warnings[0].row, 2);
    assert_eq!(vertices(&built.stroke), vec![p(0.0, 0.0), p(5.0, 5.0)]);
}
