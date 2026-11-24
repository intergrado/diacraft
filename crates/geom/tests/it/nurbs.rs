//! `NURBSTo` and the `NURBS` constructor (E-R2).
#![allow(clippy::approx_constant, reason = "Gives the middle weight as about 0.7071068, and the test uses it as written")]

use diacraft_geom::kurbo::{BezPath, ParamCurve, ParamCurveNearest, PathSeg, Point};
use diacraft_geom::{CurveProblem, GeomError, GeomRow, Nurbs, NurbsCurve, WarningKind};

use crate::{CURVE_EPS, EPS, assert_near, assert_pt, build, build_plain, extents, ln, mv, p, segments, size, vertices};

/// A `NURBSTo` row: `X`, `Y`, `A` (second-to-last knot), `B` (last weight), `C` (first knot), `D` (first weight) and the `E` constructor.
fn nurbs_row(end: (f64, f64), a: f64, b: f64, c: f64, d: f64, args: &[f64]) -> GeomRow {
    GeomRow::NurbsTo {
        x: end.0,
        y: end.1,
        second_last_knot: a,
        last_weight: b,
        first_knot: c,
        first_weight: d,
        nurbs: Some(Nurbs::from_args(args).unwrap()),
    }
}

/// The row: pen (0, 0), `X` 4, `Y` 0, `C` 0, `A` 0, `B` 1, `D` 1 and `E` `NURBS(1,3,1,1,1,2,0,1,3,2,0,1)`.
fn row_1() -> GeomRow {
    nurbs_row((4.0, 0.0), 0.0, 1.0, 0.0, 1.0, &[1.0, 3.0, 1.0, 1.0, 1.0, 2.0, 0.0, 1.0, 3.0, 2.0, 0.0, 1.0])
}

/// Given a pen at (0, 0), `X` 4, `Y` 0, `C` 0, `A` 0, `B` 1, `D` 1 and `E` `NURBS(1,3,1,1,1,2,0,1,3,2,0,1)`, expect control points
/// (0, 0), (1, 2), (3, 2), (4, 0), knots 0, 0, 0, 0, 1, 1, 1, 1, and a top at (2, 1.5). Drawing the control polygon reaches y 2. Dropping the pen as first control point
/// gives a curve from (1, 2).
#[test]
fn nurbs_the_row_is_split_across_its_cells_with_the_pen_first() {
    let curve = NurbsCurve::from_nurbs_to(p(0.0, 0.0), size(1.0, 1.0), &row_1()).unwrap();
    assert_eq!(curve.degree(), 3);
    assert_eq!(curve.points(), vec![p(0.0, 0.0), p(1.0, 2.0), p(3.0, 2.0), p(4.0, 0.0)]);
    assert_eq!(curve.weights(), vec![1.0; 4]);
    assert_eq!(curve.knots(), vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]);
    assert_eq!(curve.domain(), (0.0, 1.0));
    assert_pt(curve.eval(0.5), 2.0, 1.5, EPS, "the top");
    assert!(curve.is_clamped());

    let built = build_plain(&[mv(0.0, 0.0), row_1()]);
    assert!(built.warnings.is_empty());
    let segs = segments(&built.stroke);
    assert_eq!(segs.len(), 1, "one cubic piece");
    let PathSeg::Cubic(c) = segs[0] else { panic!("not a cubic: {:?}", segs[0]) };
    assert_pt(c.p0, 0.0, 0.0, EPS, "starts on the pen");
    assert_pt(c.p1, 1.0, 2.0, EPS, "first control");
    assert_pt(c.p2, 3.0, 2.0, EPS, "second control");
    assert_pt(c.p3, 4.0, 0.0, EPS, "ends at (X, Y)");
    let e = extents(&built.stroke);
    assert_near(e.y1, 1.5, CURVE_EPS, "the curve's top, not the control polygon's 2");
    assert!((e.y1 - 2.0).abs() > 0.4);
}

/// Given the page's list, degree 2 with knots 0, 0, 0, 2, 5, 8 and control points (0, 0), (1, 2), (3, 3), (5, 2), (6, 0), expect the full vector
/// 0, 0, 0, 2, 5, 8, 8, 8, and the curve at t 2 at (1.8, 2.4), at t 5 at (4, 2.5) and at t 8 at (6, 0). Uniform knots give (2, 2.5) at their first interior knot, and so a
/// different curve.
#[test]
fn nurbs_the_missing_knots_repeat_the_last_one() {
    let points = vec![p(0.0, 0.0), p(1.0, 2.0), p(3.0, 3.0), p(5.0, 2.0), p(6.0, 0.0)];
    let curve = NurbsCurve::new(2.0, points.clone(), vec![1.0; 5], &[0.0, 0.0, 0.0, 2.0, 5.0, 8.0]).unwrap();
    assert_eq!(curve.knots(), vec![0.0, 0.0, 0.0, 2.0, 5.0, 8.0, 8.0, 8.0]);
    assert_eq!(curve.domain(), (0.0, 8.0));
    assert_pt(curve.eval(2.0), 1.8, 2.4, EPS, "at t 2");
    assert_pt(curve.eval(5.0), 4.0, 2.5, EPS, "at t 5");
    assert_pt(curve.eval(8.0), 6.0, 0.0, EPS, "at t 8");
    assert_pt(curve.eval(0.0), 0.0, 0.0, EPS, "at t 0");
    // Naive: knots repeating the first one, or uniform, are another curve.
    let uniform = NurbsCurve::new(2.0, points.clone(), vec![1.0; 5], &[0.0, 1.0, 2.0, 3.0, 4.0, 5.0]).unwrap();
    assert!(uniform.eval(uniform.domain().0 + 1.0).distance(p(2.0, 2.5)) < 1.0, "the uniform curve is near (2, 2.5) at its first interior knot");
    assert!(uniform.eval(uniform.domain().0 + 1.0).distance(curve.eval(2.0)) > 0.1);

    // The same spline as a row: its path passes through the three points at the joins of the knot spans.
    let row = nurbs_row((6.0, 0.0), 5.0, 1.0, 0.0, 1.0, &[8.0, 2.0, 1.0, 1.0, 1.0, 2.0, 0.0, 1.0, 3.0, 3.0, 0.0, 1.0, 5.0, 2.0, 2.0, 1.0]);
    let built = build_plain(&[mv(0.0, 0.0), row]);
    assert!(built.warnings.is_empty());
    let v = vertices(&built.stroke);
    assert_eq!(v.len(), 4, "three knot spans: {v:?}");
    assert_pt(v[0], 0.0, 0.0, EPS, "t 0");
    assert_pt(v[1], 1.8, 2.4, 1e-9, "t 2");
    assert_pt(v[2], 4.0, 2.5, 1e-9, "t 5");
    assert_pt(v[3], 6.0, 0.0, EPS, "t 8");
}

/// Given a row with `C` 0, `A` 2 and `E` `NURBS(3,2,1,1,1,2,0,1,3,3,0,1,5,2,0.5,1)`, expect five control points counting the pen and
/// (`X`, `Y`), the listed knots 0, 0, 0, 0.5, 2, 3, and after padding 0, 0, 0, 0.5, 2, 3, 3, 3. Swapping `A` and `C` gives 2, 0, 0, 0.5, 0, 3, which falls.
#[test]
fn nurbs_the_listed_knots_come_from_c_the_tuples_a_and_the_last_knot() {
    let args = [3.0, 2.0, 1.0, 1.0, 1.0, 2.0, 0.0, 1.0, 3.0, 3.0, 0.0, 1.0, 5.0, 2.0, 0.5, 1.0];
    let row = nurbs_row((6.0, 0.0), 2.0, 1.0, 0.0, 1.0, &args);
    let curve = NurbsCurve::from_nurbs_to(p(0.0, 0.0), size(1.0, 1.0), &row).unwrap();
    assert_eq!(curve.points().len(), 5);
    assert_eq!(curve.points(), vec![p(0.0, 0.0), p(1.0, 2.0), p(3.0, 3.0), p(5.0, 2.0), p(6.0, 0.0)]);
    assert_eq!(curve.knots(), vec![0.0, 0.0, 0.0, 0.5, 2.0, 3.0, 3.0, 3.0]);
    // Swapped: `A` 0 and `C` 2 list 2, 0, 0, 0.5, 0, 3.
    let swapped = nurbs_row((6.0, 0.0), 0.0, 1.0, 2.0, 1.0, &args);
    assert_eq!(NurbsCurve::from_nurbs_to(p(0.0, 0.0), size(1.0, 1.0), &swapped).unwrap_err(), CurveProblem::KnotsFall);
    let built = build_plain(&[mv(0.0, 0.0), swapped]);
    assert!(matches!(built.warnings[0].kind, WarningKind::NurbsUnusable(_)));
}

/// Given quadratic control points (1, 0), (1, 1), (0, 1) with weights 1, `D` 1, and a middle weight of about 0.7071068 in `E`, and `B` 1, with knots
/// 0, 0, 0, 1, 1, 1, expect at t 0.5 about (0.7071068, 0.7071068), on the unit circle. Ignoring weights gives (0.75, 0.75).
#[test]
fn nurbs_weights_are_honoured() {
    let row = nurbs_row((0.0, 1.0), 0.0, 1.0, 0.0, 1.0, &[1.0, 2.0, 1.0, 1.0, 1.0, 1.0, 0.0, 0.7071068]);
    let curve = NurbsCurve::from_nurbs_to(p(1.0, 0.0), size(1.0, 1.0), &row).unwrap();
    assert_eq!(curve.knots(), vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
    assert_eq!(curve.weights(), vec![1.0, 0.7071068, 1.0], "first weight D, the tuple's, last weight B");
    assert_pt(curve.eval(0.5), 0.7071068, 0.7071068, 1e-7, "on the unit circle");
    // Ignoring the weights, the middle of the quadratic is (0.75, 0.75).
    let unweighted = NurbsCurve::new(2.0, curve.points().to_vec(), vec![1.0; 3], &[0.0, 0.0, 0.0, 1.0]).unwrap();
    assert_pt(unweighted.eval(0.5), 0.75, 0.75, EPS, "naive");
    // The path of the row stays on the circle, to the crate's tolerance.
    let built = build_plain(&[mv(1.0, 0.0), row]);
    assert!(built.warnings.is_empty());
    for seg in built.stroke.segments() {
        for i in 0..=20 {
            let pt = seg.eval(i as f64 / 20.0);
            assert_near(pt.to_vec2().hypot(), 1.0, 1e-6, "radius of the path");
        }
    }
    assert_pt(vertices(&built.stroke).last().copied().unwrap(), 0.0, 1.0, EPS, "ends at (X, Y)");
}

/// Given `E` with `xType` 0 and `yType` 0, `Width` 4, `Height` 2, pairs (0.25, 1) and (0.75, 1), `X` 4 and `Y` 0, expect middle control points
/// (1, 2) and (3, 2) and last control point (4, 0). Scaling `X` and `Y` as well gives (16, 0).
#[test]
fn nurbs_the_types_apply_to_the_pairs_and_not_to_x_and_y() {
    let row = nurbs_row((4.0, 0.0), 0.0, 1.0, 0.0, 1.0, &[1.0, 3.0, 0.0, 0.0, 0.25, 1.0, 0.0, 1.0, 0.75, 1.0, 0.0, 1.0]);
    let curve = NurbsCurve::from_nurbs_to(p(0.0, 0.0), size(4.0, 2.0), &row).unwrap();
    assert_eq!(curve.points(), vec![p(0.0, 0.0), p(1.0, 2.0), p(3.0, 2.0), p(4.0, 0.0)]);
    let built = build(&[mv(0.0, 0.0), row], 4.0, 2.0);
    assert_pt(vertices(&built.stroke).last().copied().unwrap(), 4.0, 0.0, EPS, "last control point");
}

/// Given a knot of 0.33333333333333331 in `A` and 0.3333333333333333 in `E` repeated beside it, expect them to read as one repeated knot. An exact
/// test reports knots that fall and loses the row. (Those two texts are one double, so the test uses doubles one unit in the last place apart.)
#[test]
fn nurbs_a_fall_within_rounding_is_equal() {
    let k = 1.0_f64 / 3.0;
    let k_up = f64::from_bits(k.to_bits() + 1);
    let k_down = f64::from_bits(k.to_bits() - 1);
    assert!(k_up > k && k_down < k);
    let points: Vec<Point> = (0..6).map(|i| p(i as f64, (i % 2) as f64)).collect();
    for listed in [[0.0, 0.0, 0.0, 0.0, k_up, k, 1.0], [0.0, 0.0, 0.0, 0.0, k, k_down, 1.0], [0.0, 0.0, 0.0, 0.0, k, k + 5e-10, 1.0]] {
        let curve = NurbsCurve::new(3.0, points.clone(), vec![1.0; 6], &listed).unwrap();
        assert!(curve.knots().windows(2).all(|w| w[0] <= w[1]), "the knot vector never falls: {:?}", curve.knots());
    }
    // A real fall is a fall: 1E-6 is no rounding.
    let fall = NurbsCurve::new(3.0, points, vec![1.0; 6], &[0.0, 0.0, 0.0, 0.0, k, k - 1e-6, 1.0]);
    assert_eq!(fall.unwrap_err(), CurveProblem::KnotsFall);
}

/// E-V2 (for E-R2): given `NURBS(1,3,1,1,1,2,0)`, seven arguments, expect `#VALUE!`. Reading the first group as complete with a weight of 1 passes silently.
#[test]
fn a_wrong_count_of_arguments_is_an_error() {
    for args in [&[1.0, 3.0, 1.0, 1.0, 1.0, 2.0, 0.0][..], &[1.0, 3.0, 1.0][..], &[][..], &[1.0, 3.0, 1.0, 1.0, 1.0][..]] {
        assert!(matches!(Nurbs::from_args(args), Err(GeomError::BadConstructor { name: "NURBS", .. })), "{args:?}");
    }
    assert!(matches!(Nurbs::from_text("NURBS(1,3,1,1,1,2,0)"), Err(GeomError::BadConstructor { .. })));
    // The four leading arguments alone are allowed by the ABNF (zero groups); the row then cannot draw, which covers.
    let none = Nurbs::from_args(&[1.0, 3.0, 1.0, 1.0]).unwrap();
    assert!(none.points.is_empty());
}

/// E-V1 (for E-R3): given `E` with `V` `NURBS(1,3,1,1,1,2,0,1,3,2,0,1)`, expect the numbers from the local `V`. Parsing `F` finds no numbers.
#[test]
fn the_constructor_is_read_from_the_cached_value_text() {
    let n = Nurbs::from_text("NURBS(1,3,1,1,1,2,0,1,3,2,0,1)").unwrap();
    assert_eq!(n.last_knot, 1.0);
    assert_eq!(n.degree, 3.0);
    assert!(!n.x_is_share && !n.y_is_share, "types 1 are lengths");
    assert_eq!(n.points.len(), 2);
    assert_eq!(n.points[1].x, 3.0);
    assert_eq!(n.points[1].weight, 1.0);
    let row = GeomRow::NurbsTo { x: 4.0, y: 0.0, second_last_knot: 0.0, last_weight: 1.0, first_knot: 0.0, first_weight: 1.0, nurbs: Some(n) };
    assert!(NurbsCurve::from_nurbs_to(p(0.0, 0.0), size(1.0, 1.0), &row).is_ok());
    assert!(Nurbs::from_text("NURBS(1,3,1,1,Width,2,0,1)").is_err(), "a cell name is no number");
    assert!(Nurbs::from_text("polyline(0,0,1,2)").is_err());
    assert!(Nurbs::from_text("NURBS(1,3,1,1,1,2,0,1,3,2,0,1) ").is_ok(), "white space around");
}

/// Given `B` of 1 on a `NURBSTo` and `B` of 2 on a `SplineStart`, expect a last weight of 1 and a first knot of 2. A reader with one
/// meaning for `B` fails one.
#[test]
fn b_geometry_b_is_the_last_weight_on_a_nurbs_and_the_first_knot_on_a_spline() {
    let row = nurbs_row((4.0, 0.0), 0.0, 2.5, 0.0, 1.0, &[1.0, 3.0, 1.0, 1.0, 1.0, 2.0, 0.0, 1.0, 3.0, 2.0, 0.0, 1.0]);
    let curve = NurbsCurve::from_nurbs_to(p(0.0, 0.0), size(1.0, 1.0), &row).unwrap();
    assert_eq!(*curve.weights().last().unwrap(), 2.5, "B is the last weight");
    assert_eq!(curve.knots()[0], 0.0, "and no knot");
    let rows = [
        GeomRow::SplineStart { x: 1.0, y: 2.0, second_knot: 2.0, first_knot: 2.0, last_knot: 3.0, degree: 3.0 },
        GeomRow::SplineKnot { x: 3.0, y: 2.0, knot: 2.0 },
        GeomRow::SplineKnot { x: 4.0, y: 0.0, knot: 2.0 },
    ];
    let spline = NurbsCurve::from_spline(p(0.0, 0.0), &rows).unwrap();
    assert_eq!(spline.knots()[0], 2.0, "B is the first knot");
    assert!(spline.weights().iter().all(|w| *w == 1.0), "no weight on a spline");
}

/// For a `NURBSTo`: `D` is the first weight, and a weight of 0 is no valid weight.
#[test]
fn d_is_the_first_weight() {
    let row = nurbs_row((4.0, 0.0), 0.0, 1.0, 0.0, 2.0, &[1.0, 3.0, 1.0, 1.0, 1.0, 2.0, 0.0, 1.0, 3.0, 2.0, 0.0, 1.0]);
    let curve = NurbsCurve::from_nurbs_to(p(0.0, 0.0), size(1.0, 1.0), &row).unwrap();
    assert_eq!(curve.weights()[0], 2.0);
    let zero = nurbs_row((4.0, 0.0), 0.0, 1.0, 0.0, 0.0, &[1.0, 3.0, 1.0, 1.0, 1.0, 2.0, 0.0, 1.0, 3.0, 2.0, 0.0, 1.0]);
    assert_eq!(NurbsCurve::from_nurbs_to(p(0.0, 0.0), size(1.0, 1.0), &zero).unwrap_err(), CurveProblem::BadWeight);
}

/// Our rule, as another reader does: a row with no usable `E`, a missing weight or a weight of 0 or less, knots that fall, or control points no more than the
/// degree is skipped with a warning, and the pen moves to (`X`, `Y`).
#[test]
fn nurbs_an_unusable_row_moves_the_pen_to_its_end() {
    let good = [1.0, 3.0, 1.0, 1.0, 1.0, 2.0, 0.0, 1.0, 3.0, 2.0, 0.0, 1.0];
    let cases: Vec<(&str, GeomRow, CurveProblem)> = vec![
        (
            "no constructor",
            GeomRow::NurbsTo { x: 4.0, y: 0.0, second_last_knot: 0.0, last_weight: 1.0, first_knot: 0.0, first_weight: 1.0, nurbs: None },
            CurveProblem::NoConstructor,
        ),
        ("last weight 0", nurbs_row((4.0, 0.0), 0.0, 0.0, 0.0, 1.0, &good), CurveProblem::BadWeight),
        ("weight below 0", nurbs_row((4.0, 0.0), 0.0, 1.0, 0.0, -1.0, &good), CurveProblem::BadWeight),
        (
            "a tuple's weight 0",
            nurbs_row((4.0, 0.0), 0.0, 1.0, 0.0, 1.0, &[1.0, 3.0, 1.0, 1.0, 1.0, 2.0, 0.0, 0.0, 3.0, 2.0, 0.0, 1.0]),
            CurveProblem::BadWeight,
        ),
        ("knots fall", nurbs_row((4.0, 0.0), 0.0, 1.0, 5.0, 1.0, &good), CurveProblem::KnotsFall),
        ("no tuples", nurbs_row((4.0, 0.0), 0.0, 1.0, 0.0, 1.0, &[1.0, 3.0, 1.0, 1.0]), CurveProblem::TooFewPoints),
        (
            "degree 4 for 4 points",
            nurbs_row((4.0, 0.0), 0.0, 1.0, 0.0, 1.0, &[1.0, 4.0, 1.0, 1.0, 1.0, 2.0, 0.0, 1.0, 3.0, 2.0, 0.0, 1.0]),
            CurveProblem::TooFewPoints,
        ),
        (
            "degree 0",
            nurbs_row((4.0, 0.0), 0.0, 1.0, 0.0, 1.0, &[1.0, 0.0, 1.0, 1.0, 1.0, 2.0, 0.0, 1.0, 3.0, 2.0, 0.0, 1.0]),
            CurveProblem::BadDegree,
        ),
        (
            "degree 2.5",
            nurbs_row((4.0, 0.0), 0.0, 1.0, 0.0, 1.0, &[1.0, 2.5, 1.0, 1.0, 1.0, 2.0, 0.0, 1.0, 3.0, 2.0, 0.0, 1.0]),
            CurveProblem::BadDegree,
        ),
        (
            "degree 26",
            nurbs_row((4.0, 0.0), 0.0, 1.0, 0.0, 1.0, &[1.0, 26.0, 1.0, 1.0, 1.0, 2.0, 0.0, 1.0, 3.0, 2.0, 0.0, 1.0]),
            CurveProblem::BadDegree,
        ),
        (
            "all knots equal",
            nurbs_row((4.0, 0.0), 0.0, 1.0, 0.0, 1.0, &[0.0, 3.0, 1.0, 1.0, 1.0, 2.0, 0.0, 1.0, 3.0, 2.0, 0.0, 1.0]),
            CurveProblem::EmptyDomain,
        ),
        ("a point that is not a number", nurbs_row((4.0, f64::NAN), 0.0, 1.0, 0.0, 1.0, &good), CurveProblem::NotFinite),
    ];
    for (what, row, problem) in cases {
        assert_eq!(NurbsCurve::from_nurbs_to(p(0.0, 0.0), size(1.0, 1.0), &row).unwrap_err(), problem, "{what}");
        let built = build_plain(&[mv(0.0, 0.0), row, ln(9.0, 9.0)]);
        if what == "a point that is not a number" {
            assert_eq!(built.warnings[0].kind, WarningKind::NonFinite, "{what}");
            continue;
        }
        assert_eq!(built.warnings.len(), 1, "{what}");
        assert!(matches!(built.warnings[0].kind, WarningKind::NurbsUnusable(_)), "{what}");
        assert_eq!(built.warnings[0].row, 1, "{what}");
        // The pen moved to (X, Y): the next row draws from there, and nothing was drawn between.
        let segs = segments(&built.stroke);
        assert_eq!(segs.len(), 1, "{what}");
        assert_pt(segs[0].start(), 4.0, 0.0, EPS, what);
        assert_pt(segs[0].end(), 9.0, 9.0, EPS, what);
    }
    assert!(matches!(NurbsCurve::from_nurbs_to(p(0.0, 0.0), size(1.0, 1.0), &ln(1.0, 1.0)), Err(CurveProblem::NotASplineRow)));
}

/// Our rule: a first knot repeated fewer than `degree` + 1 times is periodic: keep the cells, warn, and draw by the knot rule as the data stand, which does
/// not start on the pen.
#[test]
fn nurbs_a_periodic_spline_is_flagged_and_drawn_from_its_knots() {
    // Degree 2, four points, listed knots 0 to 4: the first knot is repeated once, not three times.
    let args = [4.0, 2.0, 1.0, 1.0, 2.0, 2.0, 1.0, 1.0, 3.0, 2.0, 2.0, 1.0];
    let row = nurbs_row((4.0, 0.0), 3.0, 1.0, 0.0, 1.0, &args);
    let curve = NurbsCurve::from_nurbs_to(p(0.0, 0.0), size(1.0, 1.0), &row).unwrap();
    assert!(!curve.is_clamped());
    assert_eq!(curve.knots(), vec![0.0, 1.0, 2.0, 3.0, 4.0, 4.0, 4.0]);
    assert_eq!(curve.domain(), (2.0, 4.0));
    assert!(curve.eval(2.0).distance(p(0.0, 0.0)) > 0.1, "the curve does not start on the pen");
    let built = build_plain(&[mv(0.0, 0.0), row, ln(9.0, 9.0)]);
    assert!(built.warnings.iter().any(|w| w.kind == WarningKind::PeriodicSpline && w.row == 1), "{:?}", built.warnings);
    let segs = segments(&built.stroke);
    assert!(!segs.is_empty());
    assert_pt(segs[0].start(), curve.eval(2.0).x, curve.eval(2.0).y, 1e-9, "starts where the curve starts");
    let last = segs.last().unwrap();
    assert_pt(last.start(), 4.0, 0.0, EPS, "the next row starts at (X, Y)");
    assert_pt(last.end(), 9.0, 9.0, EPS, "and ends where it says");
    // A clamped spline raises no such warning.
    assert!(build_plain(&[mv(0.0, 0.0), row_1()]).warnings.is_empty());
}

/// Degrees 1 to 3 draw: a degree 1 spline is straight lines through its control points, a degree 2 and 3 piece is exact.
#[test]
fn degree_one_is_a_polyline_through_the_control_points() {
    // Degree 1, 3 points: listed knots (n + 1 = 4) 0, 0, 1, 2 padded with one 2.
    let curve = NurbsCurve::new(1.0, vec![p(0.0, 0.0), p(1.0, 1.0), p(2.0, 0.0)], vec![1.0; 3], &[0.0, 0.0, 1.0, 2.0]).unwrap();
    assert_eq!(curve.knots(), vec![0.0, 0.0, 1.0, 2.0, 2.0]);
    assert_pt(curve.eval(1.0), 1.0, 1.0, EPS, "the middle control point");
    assert_pt(curve.eval(1.5), 1.5, 0.5, EPS, "halfway on the second line");
}

/// A degree 3 spline of several pieces: each span is a cubic Bezier through the spline's own points at the knots, the path is continuous, and it is smooth
/// (C1) at the joins, which a cubic B-spline with simple knots is.
#[test]
fn a_spline_of_many_pieces_is_continuous_and_smooth() {
    let pts: Vec<Point> = (0..7).map(|i| p(i as f64, ((i * 7) % 5) as f64)).collect();
    // Seven points, degree 3, clamped knots 0 0 0 0 1 2 3 4 (4 4 4 are implied): four spans.
    let curve = NurbsCurve::new(3.0, pts.clone(), vec![1.0; 7], &[0.0, 0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 4.0]).unwrap();
    assert_eq!(curve.domain(), (0.0, 4.0));
    let args = [
        4.0, 3.0, 1.0, 1.0, pts[1].x, pts[1].y, 0.0, 1.0, pts[2].x, pts[2].y, 0.0, 1.0, pts[3].x, pts[3].y, 0.0, 1.0, pts[4].x, pts[4].y, 1.0, 1.0,
        pts[5].x, pts[5].y, 2.0, 1.0,
    ];
    let row = nurbs_row((pts[6].x, pts[6].y), 3.0, 1.0, 0.0, 1.0, &args);
    let built = build_plain(&[mv(pts[0].x, pts[0].y), row]);
    assert!(built.warnings.is_empty(), "{:?}", built.warnings);
    let segs = segments(&built.stroke);
    assert_eq!(segs.len(), 4);
    for (i, seg) in segs.iter().enumerate() {
        assert_pt(seg.start(), curve.eval(i as f64).x, curve.eval(i as f64).y, 1e-9, "start of the span");
        assert_pt(seg.end(), curve.eval(i as f64 + 1.0).x, curve.eval(i as f64 + 1.0).y, 1e-9, "end of the span");
        assert_pt(seg.eval(0.5), curve.eval(i as f64 + 0.5).x, curve.eval(i as f64 + 0.5).y, 1e-9, "the middle of the span");
    }
    for pair in segs.windows(2) {
        let (PathSeg::Cubic(a), PathSeg::Cubic(b)) = (pair[0], pair[1]) else { panic!("not cubics") };
        assert_pt(a.p3, b.p0.x, b.p0.y, 1e-9, "continuous");
        let (out, into) = (a.p3 - a.p2, b.p1 - b.p0);
        assert!((out - into).hypot() < 1e-9, "smooth at the join: {out:?} {into:?}");
    }
}

/// Hostile splines: no panic, and a spline over the cap of points is unusable and not allocated.
#[test]
fn hostile_splines_are_refused_or_finite() {
    let many = vec![Point::new(0.0, 0.0); 200_000];
    let weights = vec![1.0; 200_000];
    let knots: Vec<f64> = (0..200_001).map(|i| i as f64).collect();
    assert_eq!(NurbsCurve::new(3.0, many, weights, &knots).unwrap_err(), CurveProblem::TooLarge);
    for degree in [f64::NAN, f64::INFINITY, -3.0, 1e300] {
        assert!(NurbsCurve::new(degree, vec![p(0.0, 0.0); 5], vec![1.0; 5], &[0.0, 0.0, 0.0, 1.0, 2.0, 2.0]).is_err());
    }
    // Weights or knots that are not numbers.
    assert_eq!(
        NurbsCurve::new(2.0, vec![p(0.0, 0.0), p(1.0, 1.0), p(2.0, 0.0)], vec![1.0, f64::NAN, 1.0], &[0.0, 0.0, 0.0, 1.0]).unwrap_err(),
        CurveProblem::BadWeight
    );
    assert_eq!(
        NurbsCurve::new(2.0, vec![p(0.0, 0.0), p(1.0, 1.0), p(2.0, 0.0)], vec![1.0; 3], &[0.0, f64::NAN, 0.0, 1.0]).unwrap_err(),
        CurveProblem::NotFinite
    );
    // The count of knots must be points + 1, and of weights points.
    assert!(NurbsCurve::new(2.0, vec![p(0.0, 0.0), p(1.0, 1.0), p(2.0, 0.0)], vec![1.0; 3], &[0.0, 0.0, 1.0]).is_err());
    assert!(NurbsCurve::new(2.0, vec![p(0.0, 0.0), p(1.0, 1.0), p(2.0, 0.0)], vec![1.0; 2], &[0.0, 0.0, 0.0, 1.0]).is_err());
    // A wild spline of extreme coordinates and degree 25.
    let wild: Vec<Point> = (0..40).map(|i| p(1e300 * (i as f64 - 20.0), -1e300 * i as f64)).collect();
    let curve = NurbsCurve::new(25.0, wild, vec![1.0; 40], &(0..41).map(|i| (i as f64).max(0.0)).collect::<Vec<_>>());
    if let Ok(c) = curve {
        let _ = c.eval(12.5);
    }
}

/// How far the exact curve strays from `path`: the largest distance, over 400 parameters of the curve, of a point of the curve from the path's nearest point.
fn gap(path: &BezPath, curve: &NurbsCurve) -> f64 {
    let (low, high) = curve.domain();
    (0..=400)
        .map(|i| curve.eval(low + (high - low) * i as f64 / 400.0))
        .map(|q| path.segments().map(|seg| seg.nearest(q, 1e-9).distance_sq.sqrt()).fold(f64::INFINITY, f64::min))
        .fold(0.0, f64::max)
}

/// The row and the exact curve of a single span spline of `n` control points `(i, ys[i])` with the weights `weights`, of degree `n - 1`: one Bezier span, the
/// worst case for a cubic that stands for it.
fn one_span(ys: &[f64], weights: &[f64]) -> (GeomRow, NurbsCurve) {
    let n = ys.len();
    let points: Vec<Point> = ys.iter().enumerate().map(|(i, y)| p(i as f64, *y)).collect();
    let mut args = vec![1.0, (n - 1) as f64, 1.0, 1.0];
    for i in 1..n - 1 {
        args.extend([i as f64, ys[i], 0.0, weights[i]]);
    }
    let row = nurbs_row((points[n - 1].x, points[n - 1].y), 0.0, weights[n - 1], 0.0, weights[0], &args);
    let mut listed = vec![0.0; n];
    listed.push(1.0);
    (row, NurbsCurve::new((n - 1) as f64, points, weights.to_vec(), &listed).unwrap())
}

/// The spline's curve is within the tolerance of the path (the curve is the textbook one; weights are honoured). Review finding 1: a Hermite
/// cubic tested only at the middle of its span passes an error that is antisymmetric about it, and a degree 5 S curve came out as one flat cubic, 8.94 off.
#[test]
fn a_degree_five_s_curve_is_drawn_within_the_tolerance() {
    let (row, exact) = one_span(&[0.0, 0.0, -50.0, 50.0, 0.0, 0.0], &[1.0; 6]);
    let built = build_plain(&[mv(0.0, 0.0), row]);
    assert!(built.warnings.is_empty(), "{:?}", built.warnings);
    let off = gap(&built.stroke, &exact);
    assert!(off < 2e-7, "the path is {off} from the curve");
    assert!(segments(&built.stroke).len() > 1, "a degree 5 curve is not one cubic");
}

/// `n choose k`, as a number.
fn binomial(n: usize, k: usize) -> f64 {
    (0..k).fold(1.0, |acc, i| acc * (n - i) as f64 / (i + 1) as f64)
}

/// The y control points (the x ones are `i`, so x is linear in the parameter) of the degree `degree` Bezier whose y is `c u^2 (1 - u)^2` times `(u - r)` for each
/// root `r`: a curve that a cubic through its ends and their slopes follows exactly except by `y` itself, which is 0 at the roots and nowhere else but the ends. So
/// the Hermite error of this curve is zero at the roots and at no other point, and a check made at a root cannot see it. `roots` must be `degree - 4` long.
fn error_curve_ys(degree: usize, roots: &[f64], c: f64) -> Vec<f64> {
    assert_eq!(roots.len() + 4, degree);
    let mut power = vec![0.0, 0.0, 1.0, -2.0, 1.0]; // u^2 - 2 u^3 + u^4
    for r in roots {
        let mut next = vec![0.0; power.len() + 1];
        for (j, a) in power.iter().enumerate() {
            next[j + 1] += a;
            next[j] -= r * a;
        }
        power = next;
    }
    // Power basis to Bernstein: b_k = sum over j up to k of C(k, j) / C(degree, j) a_j.
    (0..=degree).map(|k| c * (0..=k).map(|j| binomial(k, j) / binomial(degree, j) * power[j]).sum::<f64>()).collect()
}

/// The path is within `limit` of the curve, or the row carries `CurveApproximate`; and nothing else is said.
fn assert_within_or_warned(ys: &[f64], limit: f64, what: &str) {
    let (row, exact) = one_span(ys, &vec![1.0; ys.len()]);
    let built = build_plain(&[mv(0.0, ys[0]), row]);
    let warned = built.warnings.iter().any(|w| w.kind == WarningKind::CurveApproximate);
    let off = gap(&built.stroke, &exact);
    assert!(warned || off < limit, "{what}: the path is {off} from the curve and the row says nothing ({:?})", built.warnings);
    assert!(built.warnings.iter().all(|w| w.kind == WarningKind::CurveApproximate), "{what}: {:?}", built.warnings);
}

/// Review round 2, finding 2: x 0 to 7 and y controls 0, 0, 5, -13, 13, -5, 0, 0, degree 7, came out as one flat cubic, 0.39 off, with no warning: three checks (a quarter,
/// a half, three quarters) leave a degree 7 error free to be 0 at all three. A polynomial curve of degree `d` has a Hermite error of the form `u^2 (1-u)^2 q(u)` with
/// `q` of degree `d - 4`, so it takes `d - 3` interior checks to be sure it is not 0 at all of them (`d` for a rational curve, whose numerator has degree `d + 3`).
#[test]
fn a_degree_seven_curve_that_three_checks_miss_is_drawn_within_the_tolerance() {
    assert_within_or_warned(&[0.0, 0.0, 5.0, -13.0, 13.0, -5.0, 0.0, 0.0], 2e-7, "the review's degree 7 curve");
}

/// Every check point matters: for each degree whose checks are exactly `d - 3` points (so the middle is one of them, and the degrees are even: 6, 10, 16), and for each of
/// the points, a curve whose error is 0 at every other check point and nowhere else is drawn within the tolerance. Dropping any one of the points, or fewer than `d - 3`
/// checks, lets one of these curves through as a flat cubic. At degree 6 the points are a quarter, a half and three quarters, and the curves are the review's, with the
/// error's roots at {1/2, 1/4} (caught only at 3/4) and {1/2, 3/4} (caught only at 1/4).
#[test]
fn every_check_point_is_needed_to_see_a_curve_whose_error_is_zero_at_the_others() {
    for degree in [6usize, 10, 16] {
        let checks = degree - 3;
        let points: Vec<f64> = (1..=checks).map(|k| k as f64 / (checks + 1) as f64).collect();
        assert!(points.contains(&0.5), "an odd number of checks has the middle among them");
        for omitted in 0..checks {
            let roots: Vec<f64> = points.iter().enumerate().filter(|(i, _)| *i != omitted).map(|(_, r)| *r).collect();
            let ys = error_curve_ys(degree, &roots, 1.0);
            let top = ys.iter().fold(0.0_f64, |m, y| m.max(y.abs()));
            let ys: Vec<f64> = ys.iter().map(|y| y * 10.0 / top).collect();
            assert_within_or_warned(&ys, 2e-7, &format!("degree {degree}, error 0 at every check point but {}", points[omitted]));
        }
    }
}

/// And degrees 7, 11 and 16 with the most roots a nonzero error can have (`d - 4`), among them the three points three checks would make (a quarter, a half, three
/// quarters), so that a flat cubic is as good as the curve at every one of those three. Review round 2 names degrees 11 and 16 beside the degree 7 curve above.
#[test]
fn degrees_seven_eleven_and_sixteen_are_drawn_within_the_tolerance() {
    for degree in [7usize, 11, 16] {
        let mut roots = vec![0.25, 0.5, 0.75];
        roots.extend((0..degree - 7).map(|i| 0.03 + 0.1 * i as f64));
        let ys = error_curve_ys(degree, &roots, 1.0);
        let top = ys.iter().fold(0.0_f64, |m, y| m.max(y.abs()));
        let ys: Vec<f64> = ys.iter().map(|y| y * 10.0 / top).collect();
        assert_within_or_warned(&ys, 2e-7, &format!("degree {degree}, error 0 at {} points", roots.len()));
    }
}

/// Review finding 1, the warning: higher degrees and wild controls are drawn within the tolerance of the curve, or the row says they are not. The cutting into cubics
/// stops at a depth; a curve that needs more is drawn as near as it came, with `CurveApproximate`, and is not drawn flat with no word.
#[test]
fn a_curve_is_within_the_tolerance_or_the_row_says_it_is_not() {
    for degree in [5usize, 9, 15, 25] {
        for amplitude in [1.0, 40.0, 1e6] {
            let ys: Vec<f64> = (0..=degree).map(|i| if i % 2 == 0 { amplitude } else { -amplitude }).collect();
            let (row, exact) = one_span(&ys, &vec![1.0; degree + 1]);
            let built = build_plain(&[mv(0.0, ys[0]), row]);
            let warned = built.warnings.iter().any(|w| w.kind == WarningKind::CurveApproximate);
            let off = gap(&built.stroke, &exact);
            assert!(warned || off < 2e-7, "degree {degree}, amplitude {amplitude}: {off} off and no warning");
            assert!(built.warnings.iter().all(|w| w.kind == WarningKind::CurveApproximate), "{:?}", built.warnings);
            let last = vertices(&built.stroke).last().copied().unwrap();
            assert_pt(last, degree as f64, ys[degree], EPS * amplitude.max(1.0), "the curve ends at its end");
        }
    }
}

/// And a curve that no number of pieces within the depth can follow does warn: degree 25 with controls a million apart.
#[test]
fn a_curve_the_cutting_cannot_follow_is_warned() {
    let ys: Vec<f64> = (0..=25).map(|i| if i % 2 == 0 { 1e6 } else { -1e6 }).collect();
    let (row, _) = one_span(&ys, &[1.0; 26]);
    let built = build_plain(&[mv(0.0, ys[0]), row]);
    assert!(built.warnings.iter().any(|w| w.kind == WarningKind::CurveApproximate), "{:?}", built.warnings);
    assert!(!segments(&built.stroke).is_empty(), "and it is drawn");
}

/// The same for a rational cubic, which is not a polynomial and so not one cubic either: an S of control points with the weights 1, 4, 4, 1, which turns the curve
/// about its middle, where the error of a cubic through the ends and their slopes is 0 (review: 0.598 off before). Weights are honoured, and the
/// path is not the curve of equal weights either.
#[test]
fn a_rational_cubic_is_drawn_within_the_tolerance() {
    let (row, exact) = one_span(&[0.0, 3.0, -3.0, 0.0], &[1.0, 4.0, 4.0, 1.0]);
    let built = build_plain(&[mv(0.0, 0.0), row]);
    assert!(built.warnings.is_empty(), "{:?}", built.warnings);
    let off = gap(&built.stroke, &exact);
    assert!(off < 2e-7, "the path is {off} from the curve");
    let (_, unweighted) = one_span(&[0.0, 3.0, -3.0, 0.0], &[1.0; 4]);
    assert!(gap(&built.stroke, &unweighted) > 0.1, "the weights make a different curve");
}
