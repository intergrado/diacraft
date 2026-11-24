//! `build_row_draws`: what each row drew, one row at a time, for the path functions of the formula engine (a segment is a drawn row).
//! The steps must be the stroke path `build_paths` makes, row by row.

use diacraft_geom::kurbo::PathEl;
use diacraft_geom::{Bound, DrawStep, GeomError, GeomRow, MAX_PATH_ELEMENTS, MAX_ROWS, Polyline, build_paths, build_paths_bounded, build_row_draws};

use crate::{arc_to, build_plain, ln, mv, p, size};

fn start(x: f64, y: f64, second_knot: f64, first_knot: f64, last_knot: f64, degree: f64) -> GeomRow {
    GeomRow::SplineStart { x, y, second_knot, first_knot, last_knot, degree }
}

fn knot(x: f64, y: f64, k: f64) -> GeomRow {
    GeomRow::SplineKnot { x, y, knot: k }
}

/// The renderer's bound with this much spline work.
fn work(curve_work: u64) -> Bound {
    Bound { curve_work, ..Bound::default() }
}

/// The renderer's bound with this many steps (and elements) at most.
fn steps(path_elements: usize) -> Bound {
    Bound { path_elements, ..Bound::default() }
}

fn draws(rows: &[GeomRow]) -> Vec<Vec<DrawStep>> {
    build_row_draws(rows, size(1.0, 1.0), Bound::default()).unwrap().rows.into_iter().map(|r| r.steps).collect()
}

/// The stroke path as steps, with the closing segments left out.
fn stroke_steps(rows: &[GeomRow]) -> Vec<DrawStep> {
    build_plain(rows)
        .stroke
        .elements()
        .iter()
        .filter_map(|el| match *el {
            PathEl::MoveTo(a) => Some(DrawStep::Jump(a)),
            PathEl::LineTo(a) => Some(DrawStep::Line(a)),
            PathEl::QuadTo(a, b) => Some(DrawStep::Quad(a, b)),
            PathEl::CurveTo(a, b, c) => Some(DrawStep::Cubic(a, b, c)),
            PathEl::ClosePath => None,
        })
        .collect()
}

/// The rows: a move to (0, 0), a line to (3, 0) and a line to (3, 5) are three rows, one step each, and the move jumps.
#[test]
fn each_row_gives_its_own_steps() {
    let d = draws(&[mv(0.0, 0.0), ln(3.0, 0.0), ln(3.0, 5.0)]);
    assert_eq!(d, vec![vec![DrawStep::Jump(p(0.0, 0.0))], vec![DrawStep::Line(p(3.0, 0.0))], vec![DrawStep::Line(p(3.0, 5.0))]]);
}

/// The row-by-row steps joined are the stroke path of the same rows, for every kind of row that draws from the pen: lines, relative lines, an
/// arc, an elliptical arc, a polyline, a cubic and a quadratic, a spline run, and an ellipse. Naive: a second reading of the rows that differs.
#[test]
fn the_steps_joined_are_the_stroke_path() {
    let rows = vec![
        mv(0.0, 0.0),
        ln(2.0, 0.0),
        GeomRow::RelLineTo { x: 0.5, y: 0.5 },
        arc_to(2.0, 2.0, 0.4),
        GeomRow::EllipticalArcTo { x: 0.0, y: 2.0, mid_x: 1.0, mid_y: 2.5, angle: 0.0, ratio: 2.0 },
        GeomRow::PolylineTo {
            x: 0.0,
            y: 0.0,
            polyline: Some(Polyline { x_is_share: false, y_is_share: false, points: vec![(0.0, 1.0), (0.5, 1.5)] }),
        },
        GeomRow::RelCubBezTo { x: 1.0, y: 1.0, c1_x: 0.25, c1_y: 0.0, c2_x: 0.75, c2_y: 1.0 },
        GeomRow::RelQuadBezTo { x: 0.0, y: 0.0, c_x: 0.5, c_y: 1.0 },
        start(1.0, 2.0, 0.0, 0.0, 1.0, 3.0),
        knot(3.0, 2.0, 0.0),
        knot(4.0, 0.0, 0.0),
        mv(5.0, 5.0),
        GeomRow::Ellipse { x: 6.0, y: 6.0, first_x: 7.0, first_y: 6.0, second_x: 6.0, second_y: 7.0 },
    ];
    let mut joined: Vec<DrawStep> = Vec::new();
    for step in draws(&rows).into_iter().flatten() {
        // A move that nothing draws from is no mark in the stroke path (the next jump replaces it); the draws keep it, for the pen went there.
        if matches!((joined.last(), step), (Some(DrawStep::Jump(_)), DrawStep::Jump(_))) {
            joined.pop();
        }
        joined.push(step);
    }
    assert_eq!(joined, stroke_steps(&rows));
}

/// A `SplineStart` and the `SplineKnot` rows after it are one draw that used three rows, so a caller counts one segment.
#[test]
fn a_spline_run_is_one_draw_that_used_all_its_rows() {
    let rows = vec![mv(0.0, 0.0), start(1.0, 2.0, 0.0, 0.0, 1.0, 3.0), knot(3.0, 2.0, 0.0), knot(4.0, 0.0, 0.0), ln(5.0, 0.0)];
    let all = build_row_draws(&rows, size(1.0, 1.0), Bound::default()).unwrap();
    let shape: Vec<(usize, usize)> = all.rows.iter().map(|r| (r.first, r.rows)).collect();
    assert_eq!(shape, vec![(0, 1), (1, 3), (4, 1)]);
    assert!(all.curve_work > 0, "a spline costs multiplications");
}

/// A row that needs a pen and has none draws nothing, and the draw says so with no steps (so a caller does not count it).
#[test]
fn a_row_with_no_pen_has_no_steps() {
    let d = draws(&[ln(1.0, 1.0), mv(0.0, 0.0), ln(2.0, 0.0)]);
    assert!(d[0].is_empty());
    assert_eq!(d[2], vec![DrawStep::Line(p(2.0, 0.0))]);
}

/// An ellipse starts with a jump to its first point, then goes round in cubics, and it leaves no pen for the rows after it.
#[test]
fn an_ellipse_jumps_to_its_start_and_goes_round() {
    let d = draws(&[GeomRow::Ellipse { x: 1.0, y: 1.0, first_x: 2.0, first_y: 1.0, second_x: 1.0, second_y: 2.0 }, ln(5.0, 5.0)]);
    assert_eq!(d[0].first(), Some(&DrawStep::Jump(p(2.0, 1.0))));
    assert!(d[0].len() > 2 && d[0].iter().skip(1).all(|s| matches!(s, DrawStep::Cubic(..))));
    assert!(d[1].is_empty(), "no pen after an ellipse");
}

/// The work a caller allows bounds the splines, and a spline it does not cover is an error and not a gap: with none, a path that holds a spline
/// is refused (`build_paths` would skip the curve and draw the rest, which is right for a picture and wrong for a length). `curve_work` says
/// how much a spline used.
#[test]
fn a_spline_past_the_work_allowed_is_an_error_not_a_gap() {
    let rows = vec![mv(0.0, 0.0), start(1.0, 2.0, 0.0, 0.0, 1.0, 3.0), knot(3.0, 2.0, 0.0), knot(4.0, 0.0, 0.0), ln(5.0, 0.0)];
    assert!(matches!(build_row_draws(&rows, size(1.0, 1.0), work(0)), Err(GeomError::SplineWorkSpent)));
    // A NURBSTo row is the same: the second of two splines is the one that finds the work spent.
    let some = build_row_draws(&rows, size(1.0, 1.0), Bound::default()).unwrap();
    assert!(some.curve_work > 0 && matches!(some.rows[1].steps[..], [DrawStep::Cubic(..)]));
    let two = [rows.clone(), vec![mv(9.0, 9.0), start(1.0, 2.0, 0.0, 0.0, 1.0, 3.0), knot(3.0, 2.0, 0.0), knot(4.0, 0.0, 0.0)]].concat();
    let one_costs = some.curve_work;
    assert!(matches!(build_row_draws(&two, size(1.0, 1.0), work(one_costs)), Err(GeomError::SplineWorkSpent)));
    assert!(build_row_draws(&two, size(1.0, 1.0), work(2 * one_costs + 1000)).is_ok());
    // A caller cannot ask for more than the crate's own cap.
    assert!(some.curve_work <= Bound::default().curve_work);
    // `build_paths` still draws the rest, as a picture does.
    assert!(build_paths(&rows, size(1.0, 1.0)).is_ok());
}

/// Hostile: a size that is not a number, and more rows than a section may hold, are errors of the whole section, as for `build_paths`.
#[test]
fn a_size_that_is_not_a_number_and_too_many_rows_are_errors() {
    assert!(matches!(build_row_draws(&[mv(0.0, 0.0)], size(f64::NAN, 1.0), work(0)), Err(GeomError::BadSize)));
    let many = vec![mv(0.0, 0.0); MAX_ROWS + 1];
    assert!(matches!(build_row_draws(&many, size(1.0, 1.0), work(0)), Err(GeomError::TooManyRows { .. })));
    assert!(build_paths(&many, size(1.0, 1.0)).is_err(), "the same limit");
}

/// A row with a number that is not finite draws nothing and does not stop the rows after it.
#[test]
fn a_row_with_a_number_that_is_not_finite_draws_nothing() {
    let d = draws(&[mv(0.0, 0.0), ln(f64::NAN, 1.0), ln(2.0, 0.0)]);
    assert!(d[1].is_empty());
    assert_eq!(d[2], vec![DrawStep::Line(p(2.0, 0.0))]);
}

/// The most steps a caller allows stop the section after the row that passes them: a flood of rows is cut at the caller's cap, not at the
/// crate's four million.
#[test]
fn a_section_past_the_steps_allowed_is_too_large() {
    let rows: Vec<GeomRow> = std::iter::once(mv(0.0, 0.0)).chain((0..1000).map(|i| ln(f64::from(i), 1.0))).collect();
    assert!(matches!(build_row_draws(&rows, size(1.0, 1.0), steps(500)), Err(GeomError::PathTooLarge { max: 500 })));
    assert_eq!(build_row_draws(&rows, size(1.0, 1.0), steps(1001)).unwrap().rows.len(), 1001);
    assert!(build_row_draws(&rows, size(1.0, 1.0), steps(1000)).is_err(), "1001 steps in all: the move and a thousand lines");
}

/// One bound for the formulas and the snaps: `build_row_draws` takes the same `Bound` as `build_paths_bounded`, held to the renderer's by the
/// same `Bound::clamped`, so a caller cannot ask it for more than the renderer allows and the two cannot disagree about what a section may cost.
/// The same rows spend the same spline work in both; a bound one multiplication short of it is refused by both (an error for the draws, which do
/// not draw a path short of its curve, a skipped curve with a warning for the paths); and the path size is the one cap.
#[test]
fn the_row_draws_and_the_paths_are_held_to_one_bound() {
    let rows = vec![mv(0.0, 0.0), start(1.0, 2.0, 0.0, 0.0, 1.0, 3.0), knot(3.0, 2.0, 0.0), knot(4.0, 0.0, 0.0), ln(5.0, 0.0)];
    let join = diacraft_geom::rules::FILL_RUN_JOIN;
    // A caller cannot lift the renderer's caps through either.
    let wide = Bound { curve_work: u64::MAX, path_elements: usize::MAX };
    assert_eq!(wide.clamped(), Bound::default());
    let drawn = build_row_draws(&rows, size(1.0, 1.0), wide).unwrap();
    let built = build_paths_bounded(&rows, size(1.0, 1.0), join, wide).unwrap();
    assert!(
        drawn.curve_work > 0 && drawn.curve_work == built.curve_work,
        "the same rows spend the same work: {} and {}",
        drawn.curve_work,
        built.curve_work
    );
    assert!(drawn.curve_work <= Bound::default().curve_work);
    // The work the spline costs is enough, and one multiplication less is not: both refuse it.
    let cost = drawn.curve_work;
    assert!(build_row_draws(&rows, size(1.0, 1.0), work(cost)).is_ok());
    assert!(build_paths_bounded(&rows, size(1.0, 1.0), join, work(cost)).unwrap().warnings.is_empty());
    assert!(matches!(build_row_draws(&rows, size(1.0, 1.0), work(cost - 1)), Err(GeomError::SplineWorkSpent)));
    assert!(
        !build_paths_bounded(&rows, size(1.0, 1.0), join, work(cost - 1)).unwrap().warnings.is_empty(),
        "the paths skip the curve with a warning"
    );
    // The path size is one cap too: a thousand lines are too many for 500 elements in both, and no bound is more than the renderer's.
    let lines: Vec<GeomRow> = std::iter::once(mv(0.0, 0.0)).chain((0..1000).map(|i| ln(f64::from(i), 1.0))).collect();
    assert!(matches!(build_row_draws(&lines, size(1.0, 1.0), steps(500)), Err(GeomError::PathTooLarge { max: 500 })));
    assert!(matches!(build_paths_bounded(&lines, size(1.0, 1.0), join, steps(500)), Err(GeomError::PathTooLarge { max: 500 })));
    assert_eq!(steps(usize::MAX).clamped().path_elements, MAX_PATH_ELEMENTS);
}
