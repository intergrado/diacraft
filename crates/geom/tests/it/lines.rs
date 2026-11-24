//! `MoveTo`, `RelMoveTo`, `LineTo`, `RelLineTo`, and the pen (MOVETO, LINETO).

use diacraft_geom::kurbo::{ParamCurve, PathEl, PathSeg, Shape};
use diacraft_geom::{GeomError, GeomRow, WarningKind, build_path, build_paths};

use crate::{EPS, assert_near, assert_pt, build, build_plain, extents, ln, mv, rel_ln, rel_mv, segments, size, vertices};

/// Given `MoveTo` (0, 0), `MoveTo` (5, 5), `LineTo` (6, 5), and a last row `MoveTo` (9, 9), expect one
/// drawn segment from (5, 5) to (6, 5). Stroking between moves adds lines.
#[test]
fn moveto_moves_draw_nothing() {
    let built = build_plain(&[mv(0.0, 0.0), mv(5.0, 5.0), ln(6.0, 5.0), mv(9.0, 9.0)]);
    let segs = segments(&built.stroke);
    assert_eq!(segs.len(), 1, "{:?}", built.stroke);
    assert_pt(segs[0].start(), 5.0, 5.0, EPS, "start");
    assert_pt(segs[0].end(), 6.0, 5.0, EPS, "end");
    assert!(matches!(segs[0], PathSeg::Line(_)));
    // The path holds no dangling move: a move that is followed by nothing, or by another move, is not in it.
    assert!(!matches!(built.stroke.elements().last(), Some(PathEl::MoveTo(_))));
    assert_eq!(built.stroke.elements().iter().filter(|e| matches!(e, PathEl::MoveTo(_))).count(), 1);
}

/// Given a pen at (0, 0) and `RelLineTo` (0.5, 1) with `Width` 4 and `Height` 3, expect the end at (2, 3);
/// with a pen at (1, 1), expect (2, 3) again, where an offset from the pen gives (3, 4).
#[test]
fn lineto_a_relative_line_goes_to_shares_of_the_size_not_by_an_offset() {
    for pen in [(0.0, 0.0), (1.0, 1.0)] {
        let built = build(&[mv(pen.0, pen.1), rel_ln(0.5, 1.0)], 4.0, 3.0);
        let segs = segments(&built.stroke);
        assert_eq!(segs.len(), 1);
        assert_pt(segs[0].start(), pen.0, pen.1, EPS, "start is the pen");
        assert_pt(segs[0].end(), 2.0, 3.0, EPS, "end");
    }
}

/// Given `Width` 4, `Height` 2, and `RelMoveTo` (0.25, 0.75), `RelLineTo` (0.5, 0), expect (1, 1.5) then (2, 0)
/// in the shape's frame, and after a write of 8 to `Width`, (2, 1.5) then (4, 0). Reading the shares as inches gives (0.25, 0.75).
#[test]
fn geo_relative_rows_follow_the_size() {
    let rows = [rel_mv(0.25, 0.75), rel_ln(0.5, 0.0)];
    let narrow = vertices(&build(&rows, 4.0, 2.0).stroke);
    assert_eq!(narrow.len(), 2);
    assert_pt(narrow[0], 1.0, 1.5, EPS, "first");
    assert_pt(narrow[1], 2.0, 0.0, EPS, "second");
    let wide = vertices(&build(&rows, 8.0, 2.0).stroke);
    assert_pt(wide[0], 2.0, 1.5, EPS, "first after Width 8");
    assert_pt(wide[1], 4.0, 0.0, EPS, "second after Width 8");
    assert!((narrow[0].x - 0.25).abs() > 0.5, "naive: shares read as inches");
}

/// Given `RelLineTo` `X` 1.25 and `Y` -0.5 with `Width` 4 and `Height` 2, expect (5, -1). Clamping gives (4, 0).
#[test]
fn geo_a_share_outside_0_to_1_is_not_clamped() {
    let v = vertices(&build(&[mv(0.0, 0.0), rel_ln(1.25, -0.5)], 4.0, 2.0).stroke);
    assert_pt(v[1], 5.0, -1.0, EPS, "end");
}

/// Given `X` of 0.5 on `RelLineTo` with `Width` 3, expect 1.5 inches; given `X` of 0.5 on `LineTo`,
/// expect 0.5 inches. Treating them alike fails one.
#[test]
fn x_geometry_x_is_a_share_only_on_a_relative_row() {
    let rel = vertices(&build(&[mv(0.0, 0.0), rel_ln(0.5, 0.0)], 3.0, 1.0).stroke);
    assert_near(rel[1].x, 1.5, EPS, "RelLineTo");
    let abs = vertices(&build(&[mv(0.0, 0.0), ln(0.5, 0.0)], 3.0, 1.0).stroke);
    assert_near(abs[1].x, 0.5, EPS, "LineTo");
}

/// Given `Y` of 0.5 on `RelMoveTo` with `Height` 3 and `Width` 9, expect 1.5. Scaling by `Width` gives 4.5.
#[test]
fn y_geometry_y_is_a_share_of_the_height() {
    let v = vertices(&build(&[rel_mv(0.0, 0.5), rel_ln(1.0, 1.0)], 9.0, 3.0).stroke);
    assert_pt(v[0], 0.0, 1.5, EPS, "RelMoveTo");
    assert_pt(v[1], 9.0, 3.0, EPS, "RelLineTo");
}

/// Given `Y` of -2.5 on `LineTo`, expect it kept as written and the path below the box. Clamping gives 0.
#[test]
fn y_geometry_a_path_may_leave_the_box() {
    let built = build(&[mv(0.0, 0.0), ln(1.0, -2.5)], 2.0, 2.0);
    assert_near(extents(&built.stroke).y0, -2.5, EPS, "lowest y");
}

/// Given `Width` 2, a row `X` 2 and a write of 3 to `X`, expect `Width` still 2, and the path to overrun
/// the box. A rule that refits the box changes `Width`: here nothing takes the path's extents back into the size.
#[test]
fn x_geometry_the_path_and_the_box_are_separate_facts() {
    let built = build(&[mv(0.0, 0.0), ln(3.0, 0.0)], 2.0, 2.0);
    assert_near(extents(&built.stroke).x1, 3.0, EPS, "the path overruns Width 2");
}

/// Our rule, as another reader does: a row that needs a pen when there is none is skipped with a warning, and the rows after it draw.
#[test]
fn geo_a_row_with_no_pen_is_skipped_and_reported() {
    let built = build_plain(&[ln(1.0, 1.0), mv(0.0, 0.0), ln(2.0, 0.0)]);
    assert_eq!(built.warnings.len(), 1);
    assert_eq!(built.warnings[0].row, 0);
    assert_eq!(built.warnings[0].kind, WarningKind::NoPen);
    let segs = segments(&built.stroke);
    assert_eq!(segs.len(), 1);
    assert_pt(segs[0].start(), 0.0, 0.0, EPS, "start");
    assert_pt(segs[0].end(), 2.0, 0.0, EPS, "end");
    // A section of only such rows draws nothing, and says so for each row.
    let only = build_plain(&[ln(1.0, 1.0), ln(2.0, 2.0)]);
    assert!(only.stroke.elements().is_empty() && only.fill.elements().is_empty());
    assert_eq!(only.warnings.len(), 2);
    assert_eq!(only.warnings[1].row, 1);
}

/// Our rule: a section with no rows draws nothing, with no warning.
#[test]
fn geo_no_rows_draw_nothing() {
    let built = build_plain(&[]);
    assert!(built.stroke.elements().is_empty());
    assert!(built.fill.elements().is_empty());
    assert!(built.infinite_lines.is_empty());
    assert!(built.warnings.is_empty());
}

/// `build_path` is the stroke path of `build_paths`.
#[test]
fn build_path_is_the_stroke_of_build_paths() {
    let rows = [mv(0.0, 0.0), ln(2.0, 0.0), ln(2.0, 2.0)];
    let one = build_path(&rows, size(1.0, 1.0)).unwrap();
    let both = build_paths(&rows, size(1.0, 1.0)).unwrap();
    assert_eq!(one, both.stroke);
    assert!(!one.is_empty() && one.bounding_box().area() >= 0.0);
}

/// A rectangle that returns to its start is closed, so its stroke joins at the start: it ends with a close.
/// One that does not return is left open. The decision is per run.
#[test]
fn a_run_that_returns_to_its_start_is_stroked_closed() {
    let closed = build_plain(&[mv(0.0, 0.0), ln(2.0, 0.0), ln(2.0, 2.0), ln(0.0, 2.0), ln(0.0, 0.0)]);
    assert!(crate::ends_with_close(&closed.stroke));
    assert_eq!(segments(&closed.stroke).len(), 4);
    let open = build_plain(&[mv(0.0, 0.0), ln(2.0, 0.0), ln(2.0, 2.0)]);
    assert!(!crate::ends_with_close(&open.stroke));
    // Two runs, the first closed and the second open: one close, in the first.
    let mixed = build_plain(&[mv(0.0, 0.0), ln(1.0, 0.0), ln(1.0, 1.0), ln(0.0, 0.0), mv(5.0, 5.0), ln(6.0, 5.0)]);
    let closes = mixed.stroke.elements().iter().filter(|e| matches!(e, PathEl::ClosePath)).count();
    assert_eq!(closes, 1);
    assert!(!crate::ends_with_close(&mixed.stroke));
}

/// A size that is not a number is an error of the whole path; a row cannot be placed in it.
#[test]
fn a_size_that_is_not_finite_is_an_error() {
    for (w, h) in [(f64::NAN, 1.0), (1.0, f64::INFINITY), (f64::NEG_INFINITY, f64::NAN)] {
        assert_eq!(build_paths(&[rel_mv(0.5, 0.5)], size(w, h)).unwrap_err(), GeomError::BadSize);
    }
    // A shape with no width or height is fine: shares give 0 (the collapsed shapes).
    let v = vertices(&build(&[rel_mv(0.5, 0.5), rel_ln(1.0, 1.0)], 0.0, 0.0).stroke);
    assert_pt(v[1], 0.0, 0.0, EPS, "collapsed");
}

/// Rows whose numbers are not finite are skipped, not drawn at a guess.
#[test]
fn a_row_with_a_number_that_is_not_finite_is_skipped_and_reported() {
    let built = build_plain(&[mv(0.0, 0.0), ln(f64::NAN, 1.0), ln(1.0, f64::INFINITY), ln(2.0, 0.0)]);
    assert_eq!(built.warnings.len(), 2);
    assert!(built.warnings.iter().all(|w| w.kind == WarningKind::NonFinite));
    let v = vertices(&built.stroke);
    assert_eq!(v.len(), 2);
    assert_pt(v[1], 2.0, 0.0, EPS, "the finite row");
    // A move to a non-finite point is skipped too, so the rows after it have the pen they had.
    let moved = build_plain(&[GeomRow::MoveTo { x: f64::NAN, y: 0.0 }]);
    assert!(moved.stroke.elements().is_empty());
}
