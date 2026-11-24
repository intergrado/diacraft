#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use diacraft_geom::NurbsCurve;
use kurbo::{Point, Shape as _};

use super::bounds::{bounds, flatten, path_of};
use super::shapes::ARC_BOW;
use super::*;

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1.0e-9
}

fn near_p(a: P, b: P) -> bool {
    near(a.0, b.0) && near(a.1, b.1)
}

fn pt(p: Point) -> P {
    (p.x, p.y)
}

/// The rows as plain words and points, for a comparison: `Move`, `Line`, `Arc` (with its bow), `Ellipse` and `Nurbs`.
fn kinds(drawn: &Drawn) -> Vec<&'static str> {
    drawn
        .rows
        .iter()
        .map(|row| match row {
            CRow::Move(_) => "Move",
            CRow::Line(_) => "Line",
            CRow::Arc { .. } => "Arc",
            CRow::Ellipse { .. } => "Ellipse",
            CRow::Nurbs { .. } => "Nurbs",
            _ => "Other",
        })
        .collect()
}

fn end_of(row: &CRow) -> P {
    match row {
        CRow::Move(p) | CRow::Line(p) => pt(*p),
        CRow::Arc { to, .. } | CRow::Nurbs { to, .. } => pt(*to),
        other => panic!("{other:?}"),
    }
}

fn draw_with(tool: Tool, points: &[P], shift: bool) -> Result<Drawn, Problem> {
    draw(tool, points, &Settings { shift, ..Settings::default() })
}

fn refused(tool: Tool, points: &[P]) -> String {
    draw_with(tool, points, false).unwrap_err().message
}

/// A deterministic wobble in -1..1 that is not a pattern a fit could lock on to.
fn wobble(i: usize) -> f64 {
    ((i as f64 * 12.9898).sin() * 43_758.545_3).rem_euclid(1.0) * 2.0 - 1.0
}

/// Points on the circle of centre `c` and radius `r` from `from` to `to` degrees (counterclockwise for `to` above `from`), `n` of them, each moved by up to `noise`.
fn on_circle(c: P, r: f64, from: f64, to: f64, n: usize, noise: f64) -> Vec<P> {
    (0..n)
        .map(|i| {
            let degrees = from + (to - from) * i as f64 / (n - 1) as f64;
            let (s, k) = degrees.to_radians().sin_cos();
            let e = if i == 0 || i + 1 == n { 0.0 } else { noise };
            (c.0 + (r + e * wobble(i)) * k, c.1 + (r + e * wobble(i + 7)) * s)
        })
        .collect()
}

/// Points along the segment from `a` to `b`, `n` of them, each moved by up to `noise` across it.
fn along(a: P, b: P, n: usize, noise: f64) -> Vec<P> {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let length = dx.hypot(dy);
    (0..n)
        .map(|i| {
            let t = i as f64 / (n - 1) as f64;
            let e = if i == 0 || i + 1 == n { 0.0 } else { noise * wobble(i) };
            (a.0 + dx * t - dy / length * e, a.1 + dy * t + dx / length * e)
        })
        .collect()
}

/// How far `p` is from the polyline `line`.
fn distance_to(line: &[P], p: P) -> f64 {
    line.windows(2)
        .map(|w| {
            let (a, b) = (w[0], w[1]);
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let l2 = dx * dx + dy * dy;
            let t = if l2 == 0.0 { 0.0 } else { (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / l2).clamp(0.0, 1.0) };
            (p.0 - a.0 - t * dx).hypot(p.1 - a.1 - t * dy)
        })
        .fold(f64::INFINITY, f64::min)
}

/// The most any point of the stroke is from what was drawn.
fn worst_stand_off(drawn: &Drawn, stroke: &[P]) -> f64 {
    let line = flatten(&drawn.rows);
    stroke.iter().map(|p| distance_to(&line, *p)).fold(0.0, f64::max)
}

// ---- Rectangle, Ellipse, Line, Arc ----------------------------------------------------------------------------------------------

#[test]
fn a_rectangle_is_a_move_and_four_lines_back_to_the_first_corner_and_it_is_closed() {
    let drawn = draw_with(Tool::Rectangle, &[(1.0, 2.0), (4.0, 3.5)], false).unwrap();
    assert_eq!(kinds(&drawn), ["Move", "Line", "Line", "Line", "Line"]);
    let ends: Vec<P> = drawn.rows.iter().map(end_of).collect();
    assert_eq!(ends, [(1.0, 2.0), (4.0, 2.0), (4.0, 3.5), (1.0, 3.5), (1.0, 2.0)]);
    assert!(drawn.closed);
    assert_eq!((drawn.rect.x0, drawn.rect.y0, drawn.rect.x1, drawn.rect.y1), (1.0, 2.0, 4.0, 3.5));
}

#[test]
fn a_rectangle_dragged_up_and_to_the_left_is_the_same_box_from_the_other_corner() {
    let a = draw_with(Tool::Rectangle, &[(4.0, 3.5), (1.0, 2.0)], false).unwrap();
    assert_eq!(a.rect, kurbo::Rect::new(1.0, 2.0, 4.0, 3.5));
    assert_eq!(kinds(&a).len(), 5);
}

#[test]
fn shift_makes_a_rectangle_a_square_with_the_longer_side_toward_the_pointer_in_every_quadrant() {
    let square = |to: P| {
        let r = draw_with(Tool::Rectangle, &[(1.0, 1.0), to], true).unwrap().rect;
        (r.x0, r.y0, r.x1, r.y1)
    };
    assert_eq!(square((5.0, 3.0)), (1.0, 1.0, 5.0, 5.0));
    assert_eq!(square((5.0, -1.0)), (1.0, -3.0, 5.0, 1.0));
    assert_eq!(square((-3.0, 2.5)), (-3.0, 1.0, 1.0, 5.0));
    // The side is 5, the longer of the 1 across and the 5 down: right and below.
    assert_eq!(square((2.0, -4.0)), (1.0, -4.0, 6.0, 1.0));
    // Without Shift it is the box the pointer made.
    let drawn = draw_with(Tool::Rectangle, &[(1.0, 1.0), (5.0, 3.0)], false).unwrap();
    assert_eq!((drawn.rect.x1 - drawn.rect.x0, drawn.rect.y1 - drawn.rect.y0), (4.0, 2.0));
}

#[test]
fn an_ellipse_is_one_ellipse_row_whose_points_are_the_ends_of_its_axes_and_shift_makes_a_circle() {
    let drawn = draw_with(Tool::Ellipse, &[(1.0, 2.0), (5.0, 4.0)], false).unwrap();
    assert_eq!(kinds(&drawn), ["Ellipse"]);
    let CRow::Ellipse { centre, first, second } = &drawn.rows[0] else { panic!() };
    assert_eq!((pt(*centre), pt(*first), pt(*second)), ((3.0, 3.0), (5.0, 3.0), (3.0, 4.0)));
    assert!(drawn.closed);
    assert_eq!((drawn.rect.x0, drawn.rect.y0, drawn.rect.x1, drawn.rect.y1), (1.0, 2.0, 5.0, 4.0));
    let circle = draw_with(Tool::Ellipse, &[(1.0, 2.0), (5.0, 4.0)], true).unwrap();
    assert_eq!((circle.rect.x1 - circle.rect.x0, circle.rect.y1 - circle.rect.y0), (4.0, 4.0));
    // The path the row draws fills its box.
    let path = bounds(&circle.rows).unwrap();
    assert!((path.x1 - path.x0 - 4.0).abs() < 1.0e-3 && (path.y1 - path.y0 - 4.0).abs() < 1.0e-3, "{path:?}");
}

#[test]
fn a_click_or_a_hair_of_a_drag_draws_nothing_and_the_refusal_says_what_to_do() {
    assert!(refused(Tool::Rectangle, &[(1.0, 1.0), (1.0, 1.0)]).contains("drag out a rectangle"));
    assert!(refused(Tool::Rectangle, &[(1.0, 1.0), (4.0, 1.0004)]).contains("at least 0.001 inch on each side"));
    assert!(
        refused(Tool::Ellipse, &[(1.0, 1.0), (1.0004, 3.0)]).contains("drag out a ellipse")
            || refused(Tool::Ellipse, &[(1.0, 1.0), (1.0004, 3.0)]).contains("ellipse")
    );
    assert!(refused(Tool::Line, &[(1.0, 1.0), (1.0, 1.0004)]).contains("drag out a line of at least 0.001 inch"));
    assert!(refused(Tool::Arc, &[(1.0, 1.0), (1.0, 1.0)]).contains("arc"));
}

#[test]
fn a_line_is_a_move_and_a_line_and_it_is_open() {
    let drawn = draw_with(Tool::Line, &[(1.0, 1.0), (4.0, 5.0)], false).unwrap();
    assert_eq!(kinds(&drawn), ["Move", "Line"]);
    assert_eq!((end_of(&drawn.rows[0]), end_of(&drawn.rows[1])), ((1.0, 1.0), (4.0, 5.0)));
    assert!(!drawn.closed);
    assert_eq!((drawn.rect.x0, drawn.rect.y0, drawn.rect.x1, drawn.rect.y1), (1.0, 1.0, 4.0, 5.0));
}

#[test]
fn shift_holds_a_line_to_a_step_of_45_degrees_and_keeps_the_length_the_pointer_went_along_it() {
    // A drag at about 19 degrees is held level, one at about 40 goes up the diagonal, one at about 94 stands upright.
    let level = draw_with(Tool::Line, &[(0.0, 0.0), (4.0, 1.4)], true).unwrap();
    let (x, y) = end_of(&level.rows[1]);
    assert!(y.abs() < 1.0e-9 && x > 3.7 && x <= 4.0 + 1.0e-9, "({x}, {y})");
    let diagonal = draw_with(Tool::Line, &[(0.0, 0.0), (3.0, 2.5)], true).unwrap();
    let (x, y) = end_of(&diagonal.rows[1]);
    assert!(near(x, y) && x > 2.5 && x < 3.0, "({x}, {y})");
    let upright = draw_with(Tool::Line, &[(1.0, 1.0), (0.8, 4.0)], true).unwrap();
    let (x, y) = end_of(&upright.rows[1]);
    assert!(near(x, 1.0) && y > 3.9 && y <= 4.0 + 1.0e-9, "({x}, {y})");
    // Down and to the left.
    let back = draw_with(Tool::Line, &[(0.0, 0.0), (-2.0, -1.9)], true).unwrap();
    let (x, y) = end_of(&back.rows[1]);
    assert!(near(x, y) && x < -1.9, "({x}, {y})");
}

#[test]
fn the_end_a_step_holds_is_never_farther_than_the_pointer_and_a_pointer_on_the_start_stays_there() {
    assert_eq!(step_end((1.0, 1.0), (1.0, 1.0), 45.0), (1.0, 1.0));
    // Straight along a step: the pointer's own place.
    let e = step_end((0.0, 0.0), (3.0, 3.0), 45.0);
    assert!(near_p(e, (3.0, 3.0)), "{e:?}");
    // Off a step: the length the pointer went along it, which is a little shorter than the pointer went.
    let e = step_end((0.0, 0.0), (5.0, 1.0), 45.0);
    assert!(e.1.abs() < 1.0e-9 && (e.0 - 5.0).abs() < 0.05 && e.0 < 26.0_f64.sqrt(), "{e:?}");
    for bad in [0.0, -45.0, f64::NAN, f64::INFINITY] {
        assert_eq!(step_end((0.0, 0.0), (2.0, 1.0), bad), (2.0, 1.0));
    }
}

#[test]
fn a_square_corner_takes_the_longer_side_and_a_pointer_level_with_the_start_counts_as_right_and_above() {
    assert_eq!(square_corner((1.0, 1.0), (5.0, 3.0)), (5.0, 5.0));
    assert_eq!(square_corner((1.0, 1.0), (2.0, -4.0)), (6.0, -4.0));
    assert_eq!(square_corner((1.0, 1.0), (1.0, 1.0)), (1.0, 1.0));
    assert_eq!(square_corner((1.0, 1.0), (3.0, 1.0)), (3.0, 3.0));
}

#[test]
fn the_arc_tool_makes_a_quarter_circle_bowed_to_the_left_of_the_way_the_pointer_went() {
    let drawn = draw_with(Tool::Arc, &[(0.0, 0.0), (4.0, 0.0)], false).unwrap();
    assert_eq!(kinds(&drawn), ["Move", "Arc"]);
    let CRow::Arc { to, bow } = &drawn.rows[1] else { panic!() };
    assert_eq!(pt(*to), (4.0, 0.0));
    assert!(near(*bow, -ARC_BOW * 4.0) && *bow < 0.0, "{bow}");
    // A quarter of a circle: the sweep of an arc is 4 atan(2 bow / chord).
    assert!((4.0 * (2.0 * bow.abs() / 4.0).atan() - std::f64::consts::FRAC_PI_2).abs() < 1.0e-9);
    // Travelling left to right it arches over the chord: the highest point is above it.
    let path = bounds(&drawn.rows).unwrap();
    assert!(path.y1 > 0.5 && path.y0 > -1.0e-6, "{path:?}");
    assert!(!drawn.closed);
    // The other way the arc is below.
    let back = draw_with(Tool::Arc, &[(4.0, 0.0), (0.0, 0.0)], false).unwrap();
    assert!(bounds(&back.rows).unwrap().y0 < -0.5);
    // Shift holds the chord to a step.
    let held = draw_with(Tool::Arc, &[(0.0, 0.0), (4.0, 0.7)], true).unwrap();
    assert!(end_of(&held.rows[1]).1.abs() < 1.0e-9);
}

#[test]
fn the_box_of_an_arc_is_the_box_round_the_curve_the_geometry_crate_builds() {
    let drawn = draw_with(Tool::Arc, &[(0.0, 0.0), (4.0, 0.0)], false).unwrap();
    let curve = path_of(&drawn.rows).unwrap();
    assert_eq!(curve.bounding_box(), drawn.rect);
    // The middle of the arc stands the bow from the chord.
    assert!((drawn.rect.y1 - ARC_BOW * 4.0).abs() < 2.0e-3, "{:?}", drawn.rect);
}

// ---- strokes: what is refused ----------------------------------------------------------------------------------------------------

#[test]
fn a_stroke_that_cannot_be_read_is_refused_in_words() {
    let many: Vec<P> = (0..=MAX_STROKE_POINTS).map(|i| (i as f64 * 1.0e-3, 0.0)).collect();
    assert!(refused(Tool::Pencil, &many).contains("at most 5000 points"));
    assert!(refused(Tool::Freeform, &[(1.0, 1.0)]).contains("at least two points"));
    assert!(refused(Tool::Line, &[]).contains("at least two points"));
    for tool in Tool::ALL {
        assert!(refused(tool, &[(0.0, 0.0), (f64::NAN, 1.0)]).contains("must be a number"), "{tool:?}");
        assert!(refused(tool, &[(0.0, 0.0), (1.0e9, 1.0)]).contains("within"), "{tool:?}");
        assert!(refused(tool, &[(f64::INFINITY, 0.0), (1.0, 1.0)]).contains("must be a number"), "{tool:?}");
    }
    let s = |tolerance: f64, smoothing: f64| Settings { shift: false, tolerance, smoothing };
    for bad in [s(0.0, 0.5), s(f64::NAN, 0.5), s(100.0, 0.5)] {
        assert!(draw(Tool::Pencil, &[(0.0, 0.0), (3.0, 0.0)], &bad).unwrap_err().message.contains("`tolerance`"));
    }
    for bad in [s(0.03, -0.1), s(0.03, 1.5), s(0.03, f64::NAN)] {
        assert!(draw(Tool::Freeform, &[(0.0, 0.0), (3.0, 0.0)], &bad).unwrap_err().message.contains("`smoothing`"));
    }
}

#[test]
fn the_tool_words_name_the_six_tools_in_any_case_and_only_those() {
    for tool in Tool::ALL {
        assert_eq!(Tool::from_name(tool.name()), Some(tool));
        assert_eq!(Tool::from_name(&tool.name().to_uppercase()), Some(tool));
    }
    assert_eq!(Tool::from_name(" Pencil "), Some(Tool::Pencil));
    assert_eq!(Tool::from_name("brush"), None);
    assert_eq!(Tool::ALL.iter().filter(|t| t.goes_on()).count(), 4);
    assert!(!Tool::Rectangle.goes_on() && !Tool::Ellipse.goes_on());
}

// ---- the Pencil ---------------------------------------------------------------------------------------------------------------------

#[test]
fn a_straight_stroke_with_a_shaky_hand_is_one_line_from_where_it_began_to_where_it_ended() {
    let stroke = along((0.5, 0.5), (4.5, 1.5), 60, 0.01);
    let drawn = draw_with(Tool::Pencil, &stroke, false).unwrap();
    assert_eq!(kinds(&drawn), ["Move", "Line"]);
    assert_eq!((end_of(&drawn.rows[0]), end_of(&drawn.rows[1])), (stroke[0], stroke[59]));
    assert!(!drawn.closed);
}

#[test]
fn a_stroke_along_a_circle_is_one_arc_with_the_bow_of_the_circle_and_the_sign_of_the_side_it_bulges_to() {
    // 90 degrees of a circle of radius 2 run counterclockwise: the arc bulges to the right of the way it goes, which is a positive bow.
    let stroke = on_circle((1.0, 1.0), 2.0, 20.0, 110.0, 40, 0.01);
    let drawn = draw_with(Tool::Pencil, &stroke, false).unwrap();
    assert_eq!(kinds(&drawn), ["Move", "Arc"], "{drawn:?}");
    let CRow::Arc { to, bow } = &drawn.rows[1] else { panic!() };
    assert_eq!(pt(*to), stroke[39]);
    let sagitta = 2.0 * (1.0 - 45.0_f64.to_radians().cos());
    assert!(*bow > 0.0 && (bow - sagitta).abs() < 0.03, "{bow} against {sagitta}");
    assert!(worst_stand_off(&drawn, &stroke) < 0.03);
    // The same stroke run the other way bulges to the left: a negative bow of the same size.
    let mut back = stroke.clone();
    back.reverse();
    let drawn = draw_with(Tool::Pencil, &back, false).unwrap();
    let CRow::Arc { bow: back_bow, .. } = &drawn.rows[1] else { panic!("{drawn:?}") };
    assert!(*back_bow < 0.0 && (back_bow + bow).abs() < 0.03, "{back_bow} and {bow}");
}

#[test]
fn a_stroke_that_turns_a_corner_is_two_lines_and_the_corner_is_a_vertex() {
    let mut stroke = along((0.0, 0.0), (3.0, 0.0), 30, 0.008);
    stroke.extend(along((3.0, 0.0), (3.0, 2.0), 20, 0.008).into_iter().skip(1));
    let drawn = draw_with(Tool::Pencil, &stroke, false).unwrap();
    assert_eq!(kinds(&drawn), ["Move", "Line", "Line"], "{drawn:?}");
    let corner = end_of(&drawn.rows[1]);
    assert!((corner.0 - 3.0).abs() < 0.05 && corner.1.abs() < 0.05, "{corner:?}");
}

#[test]
fn a_stroke_of_a_line_and_then_half_a_circle_is_a_line_and_an_arc() {
    let mut stroke = along((0.0, 0.0), (3.0, 0.0), 30, 0.005);
    stroke.extend(on_circle((3.0, 1.0), 1.0, -90.0, 90.0, 40, 0.005).into_iter().skip(1));
    let drawn = draw_with(Tool::Pencil, &stroke, false).unwrap();
    assert_eq!(kinds(&drawn), ["Move", "Line", "Arc"], "{drawn:?}");
    assert!(worst_stand_off(&drawn, &stroke) < 0.04);
}

#[test]
fn a_stroke_that_comes_back_to_where_it_began_is_closed_and_a_circle_is_arcs() {
    let mut stroke = on_circle((2.0, 2.0), 1.5, 0.0, 360.0, 80, 0.005);
    // The loop is closed by putting the last point on the first, as the window does when the end is let go over the first vertex.
    if let (Some(first), Some(last)) = (stroke.first().copied(), stroke.last_mut()) {
        *last = first;
    }
    let drawn = draw_with(Tool::Pencil, &stroke, false).unwrap();
    assert!(drawn.closed);
    assert!(kinds(&drawn).iter().skip(1).all(|k| *k == "Arc"), "{drawn:?}");
    assert!(drawn.rows.len() >= 3, "a circle is at least two arcs: {drawn:?}");
    assert!(worst_stand_off(&drawn, &stroke) < 0.03);
    // An open stroke is not closed.
    let open = on_circle((2.0, 2.0), 1.5, 0.0, 300.0, 70, 0.005);
    assert!(!draw_with(Tool::Pencil, &open, false).unwrap().closed);
}

#[test]
fn two_points_are_one_line_and_shift_holds_a_pencil_stroke_to_a_straight_line_at_a_step() {
    let drawn = draw_with(Tool::Pencil, &[(0.0, 0.0), (2.0, 2.0)], false).unwrap();
    assert_eq!(kinds(&drawn), ["Move", "Line"]);
    let stroke = on_circle((1.0, 1.0), 2.0, 20.0, 110.0, 40, 0.0);
    let held = draw_with(Tool::Pencil, &stroke, true).unwrap();
    assert_eq!(kinds(&held), ["Move", "Line"]);
}

#[test]
fn a_scribble_of_hundreds_of_corners_is_refused_and_not_cut_into_hundreds_of_rows() {
    let zigzag: Vec<P> = (0..2_000).map(|i| (i as f64 * 0.01, if i % 2 == 0 { 0.0 } else { 1.0 })).collect();
    let message = refused(Tool::Pencil, &zigzag);
    assert!(message.contains("too many") && message.contains(&MAX_PIECES.to_string()), "{message}");
}

#[test]
fn the_pencil_does_work_for_each_point_a_few_times_over_and_not_for_every_pair() {
    // A thousand points on a wave: the fit looks at a point a few times for each split, and a wave of this size splits a few dozen times.
    let stroke: Vec<P> = (0..1_000).map(|i| (i as f64 * 0.005, (i as f64 * 0.02).sin() * 0.5)).collect();
    let drawn = draw_with(Tool::Pencil, &stroke, false).unwrap();
    assert!(drawn.rows.len() <= MAX_PIECES + 1);
    assert!(worst_stand_off(&drawn, &stroke) < DEFAULT_TOLERANCE * 1.05);
}

// ---- the Freeform tool ----------------------------------------------------------------------------------------------------------------

/// The curve a `Nurbs` row makes after the pen, as the geometry crate reads it.
fn curve_of(drawn: &Drawn) -> NurbsCurve {
    let pen = match &drawn.rows[0] {
        CRow::Move(p) => *p,
        other => panic!("{other:?}"),
    };
    let geom = super::bounds::geom_rows(&drawn.rows[1..2]);
    NurbsCurve::from_nurbs_to(pen, kurbo::Size::new(1.0, 1.0), &geom[0]).unwrap()
}

fn samples(curve: &NurbsCurve) -> Vec<P> {
    let (low, high) = curve.domain();
    (0..=2_000).map(|i| pt(curve.eval(low + (high - low) * f64::from(i) / 2_000.0))).collect()
}

#[test]
fn a_wavy_stroke_is_one_clamped_cubic_spline_that_starts_and_ends_where_the_stroke_does_and_stays_near_it() {
    let stroke: Vec<P> = (0..200).map(|i| (i as f64 * 0.03, (i as f64 * 0.06).sin() * 1.2 + 0.02 * wobble(i))).collect();
    let drawn = draw_with(Tool::Freeform, &stroke, false).unwrap();
    assert_eq!(kinds(&drawn), ["Move", "Nurbs"], "{drawn:?}");
    let CRow::Nurbs { to, degree, .. } = &drawn.rows[1] else { panic!() };
    assert_eq!(*degree, 3.0);
    assert_eq!((end_of(&drawn.rows[0]), pt(*to)), (stroke[0], stroke[199]));
    let curve = curve_of(&drawn);
    assert!(curve.is_clamped() && curve.degree() == 3);
    let line = samples(&curve);
    assert!(near_p(line[0], stroke[0]) && near_p(*line.last().unwrap(), stroke[199]));
    let worst = stroke.iter().map(|p| distance_to(&line, *p)).fold(0.0, f64::max);
    assert!(worst < DEFAULT_TOLERANCE * 3.0, "the curve stays within {worst} of the stroke");
    assert!(!drawn.closed);
    assert!(drawn.rect.width() > 5.0 && drawn.rect.height() > 2.0);
}

#[test]
fn the_curve_of_a_stroke_is_a_composite_bezier_with_knots_one_apart_each_repeated_three_times() {
    let stroke: Vec<P> = (0..120).map(|i| (i as f64 * 0.05, (i as f64 * 0.1).sin() * 1.5)).collect();
    let drawn = draw_with(Tool::Freeform, &stroke, false).unwrap();
    let CRow::Nurbs { to, knots, last_knot, points, .. } = &drawn.rows[1] else { panic!() };
    // `n` cubics have 3n + 1 control points: the pen, `points`, and `to`.
    let controls = points.len() + 2;
    assert_eq!((controls - 1) % 3, 0, "{controls} control points");
    let pieces = (controls - 1) / 3;
    assert!(pieces >= 2, "a long wave is more than one cubic: {pieces}");
    assert_eq!(*last_knot, pieces as f64);
    // The knots as the row lists them: 0 four times, then each whole number up to `pieces` - 1 three times, then `pieces` once (its other three copies are implied).
    let mut listed = vec![knots.first_knot];
    listed.extend(points.iter().map(|(_, k, _)| *k));
    listed.push(knots.second_last_knot);
    listed.push(*last_knot);
    let mut want = vec![0.0; 4];
    for k in 1..pieces {
        want.extend([k as f64; 3]);
    }
    want.push(pieces as f64);
    assert_eq!(listed, want);
    assert!(points.iter().all(|(_, _, w)| *w == 1.0) && knots.first_weight == 1.0 && knots.last_weight == 1.0);
    let _ = to;
}

#[test]
fn a_stroke_with_a_sharp_turn_keeps_the_corner() {
    let mut stroke = along((0.0, 0.0), (3.0, 0.0), 40, 0.004);
    stroke.extend(along((3.0, 0.0), (3.0, 3.0), 40, 0.004).into_iter().skip(1));
    let drawn = draw_with(Tool::Freeform, &stroke, false).unwrap();
    let curve = curve_of(&drawn);
    let line = samples(&curve);
    assert!(distance_to(&line, (3.0, 0.0)) < DEFAULT_TOLERANCE * 2.0, "the curve passes the corner");
    assert!(worst_stand_off(&drawn, &stroke) < DEFAULT_TOLERANCE * 3.0);
}

#[test]
fn smoothing_takes_the_jitter_out_of_a_stroke_and_never_makes_it_more_pieces() {
    let stroke: Vec<P> = (0..160).map(|i| (i as f64 * 0.04, (i as f64 * 0.05).sin() + 0.06 * wobble(i))).collect();
    let pieces = |smoothing: f64| -> usize {
        let drawn = draw(Tool::Freeform, &stroke, &Settings { shift: false, tolerance: 0.02, smoothing }).unwrap();
        let CRow::Nurbs { points, .. } = &drawn.rows[1] else { panic!() };
        (points.len() + 1) / 3
    };
    let (rough, smooth) = (pieces(0.0), pieces(1.0));
    assert!(smooth <= rough, "smoothing 1 gives {smooth} cubics and none gives {rough}");
    assert!(smooth >= 1);
}

#[test]
fn two_points_of_a_freeform_stroke_are_a_line_and_a_stroke_that_closes_ends_on_its_start() {
    let drawn = draw_with(Tool::Freeform, &[(0.0, 0.0), (3.0, 1.0)], false).unwrap();
    assert_eq!(kinds(&drawn), ["Move", "Line"]);
    let mut stroke = on_circle((2.0, 2.0), 1.5, 0.0, 360.0, 100, 0.01);
    if let (Some(first), Some(last)) = (stroke.first().copied(), stroke.last_mut()) {
        *last = first;
    }
    let drawn = draw_with(Tool::Freeform, &stroke, false).unwrap();
    assert!(drawn.closed);
    assert!(near_p(end_of(&drawn.rows[1]), stroke[0]));
}

#[test]
fn a_stroke_of_the_most_points_is_fitted_and_a_stroke_on_one_spot_is_refused() {
    let wave: Vec<P> = (0..MAX_STROKE_POINTS).map(|i| (i as f64 * 0.002, (i as f64 * 0.01).sin() + 0.01 * wobble(i))).collect();
    let drawn = draw_with(Tool::Freeform, &wave, false).unwrap();
    assert!(matches!(drawn.rows.get(1), Some(CRow::Nurbs { .. })));
    let still: Vec<P> = (0..50).map(|_| (1.0, 1.0)).collect();
    for tool in [Tool::Freeform, Tool::Pencil] {
        assert!(refused(tool, &still).contains("at least"), "{tool:?}");
    }
}

// ---- hostile strokes ------------------------------------------------------------------------------------------------------------------

/// A small deterministic generator, so that a failure is the same on every run.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1_u64 << 53) as f64
    }
}

/// Strokes of every shape a hostile caller or a jittery hand makes: the places a few millionths apart, the whole page's reach apart, a spot, a line, a spiral, noise.
fn hostile_stroke(rng: &mut Rng, kind: u64) -> Vec<P> {
    let n = 2 + (rng.next() % 300) as usize;
    let scale = [1.0e-6, 1.0e-3, 1.0, 100.0, 9.0e5][(rng.next() % 5) as usize];
    match kind % 6 {
        0 => (0..n).map(|_| ((rng.unit() - 0.5) * scale, (rng.unit() - 0.5) * scale)).collect(),
        1 => (0..n).map(|i| (i as f64 * scale * 0.01, 0.0)).collect(),
        2 => (0..n).map(|_| (scale, scale)).collect(),
        3 => (0..n).map(|i| (i as f64 * 0.1 * scale * (i as f64).cos() * 0.01, i as f64 * 0.1 * scale * (i as f64).sin() * 0.01)).collect(),
        4 => (0..n).map(|i| (i as f64 * scale * 0.001, if i % 2 == 0 { 0.0 } else { scale })).collect(),
        _ => {
            let mut walk = (0.0_f64, 0.0_f64);
            (0..n)
                .map(|_| {
                    walk = (walk.0 + (rng.unit() - 0.5) * scale * 0.05, walk.1 + (rng.unit() - 0.5) * scale * 0.05);
                    walk
                })
                .collect()
        }
    }
}

#[test]
fn hostile_strokes_are_drawn_or_refused_in_words_with_every_tool_and_never_panic() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut drawn_count = 0_usize;
    for round in 0..600_u64 {
        let stroke = hostile_stroke(&mut rng, round);
        let tool = Tool::ALL[(round % 6) as usize];
        let settings = Settings { shift: round % 7 == 0, tolerance: [1.0e-4, 0.03, 1.0, 10.0][(round % 4) as usize], smoothing: rng.unit() };
        match draw(tool, &stroke, &settings) {
            Ok(drawn) => {
                drawn_count += 1;
                let r = drawn.rect;
                assert!(
                    r.x0.is_finite() && r.y0.is_finite() && r.x1.is_finite() && r.y1.is_finite() && r.x0 <= r.x1 && r.y0 <= r.y1,
                    "{tool:?} {r:?}"
                );
                assert!(drawn.rows.len() <= MAX_PIECES + 2, "{tool:?}: {} rows", drawn.rows.len());
                assert!(matches!(drawn.rows.first(), Some(CRow::Move(_) | CRow::Ellipse { .. })), "{tool:?} begins with a move or is an ellipse");
            }
            Err(problem) => assert!(!problem.message.is_empty(), "{tool:?}: a refusal says what was wrong"),
        }
    }
    assert!(drawn_count > 100, "a good share of the strokes are drawn: {drawn_count}");
}
