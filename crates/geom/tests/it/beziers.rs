//! `RelCubBezTo` and `RelQuadBezTo`.

use diacraft_geom::GeomRow;
use diacraft_geom::kurbo::{ParamCurve, PathSeg};

use crate::{EPS, assert_pt, build, extents, mv, p, segments};

fn cubic(x: f64, y: f64, c1: (f64, f64), c2: (f64, f64)) -> GeomRow {
    GeomRow::RelCubBezTo { x, y, c1_x: c1.0, c1_y: c1.1, c2_x: c2.0, c2_y: c2.1 }
}

fn quad(x: f64, y: f64, c: (f64, f64)) -> GeomRow {
    GeomRow::RelQuadBezTo { x, y, c_x: c.0, c_y: c.1 }
}

/// Given a pen at (0, 0), `Width` 4, `Height` 2, `RelCubBezTo` `X` 1, `Y` 0, `A` 0, `B` 1, `C` 1, `D` 1, expect at t 0.5 the point
/// (2, 1.5). Taking the controls as points on the curve, as a phrase on the page suggests, gives y 2.
#[test]
fn relbez_a_relative_cubic_has_its_controls_as_controls() {
    let built = build(&[mv(0.0, 0.0), cubic(1.0, 0.0, (0.0, 1.0), (1.0, 1.0))], 4.0, 2.0);
    let segs = segments(&built.stroke);
    assert_eq!(segs.len(), 1);
    let PathSeg::Cubic(c) = segs[0] else { panic!("not a cubic: {:?}", segs[0]) };
    assert_pt(c.p0, 0.0, 0.0, EPS, "p0");
    assert_pt(c.p1, 0.0, 2.0, EPS, "first control, shares scaled");
    assert_pt(c.p2, 4.0, 2.0, EPS, "second control");
    assert_pt(c.p3, 4.0, 0.0, EPS, "end");
    assert_pt(c.eval(0.5), 2.0, 1.5, EPS, "the curve at t 0.5");
    assert!((extents(&built.stroke).y1 - 2.0).abs() > 0.1, "naive: the curve reaching its controls would touch y 2");
}

/// Given the same pen and size, `RelQuadBezTo` `X` 1, `Y` 0, `A` 0.5, `B` 1, expect at t 0.5 the point (2, 1). Passing through the
/// control gives y 2. Treating `A` and `B` as inches gives (1.25, 0.5).
#[test]
fn relbez_a_relative_quad() {
    let built = build(&[mv(0.0, 0.0), quad(1.0, 0.0, (0.5, 1.0))], 4.0, 2.0);
    let segs = segments(&built.stroke);
    assert_eq!(segs.len(), 1);
    let PathSeg::Quad(q) = segs[0] else { panic!("not a quad: {:?}", segs[0]) };
    assert_pt(q.p1, 2.0, 2.0, EPS, "the control, shares scaled");
    assert_pt(q.p2, 4.0, 0.0, EPS, "end");
    assert_pt(q.eval(0.5), 2.0, 1.0, EPS, "the curve at t 0.5");
    assert!(q.eval(0.5).distance(p(1.25, 0.5)) > 0.1, "naive: shares read as inches");
}

/// Given `B` of 0.75 on `RelQuadBezTo` with `Height` 2 and `Width` 4, expect the control's y at 1.5. Scaling by `Width` gives 3.
#[test]
fn b_geometry_b_is_a_share_of_the_height() {
    let built = build(&[mv(0.0, 0.0), quad(1.0, 0.0, (0.25, 0.75))], 4.0, 2.0);
    let PathSeg::Quad(q) = segments(&built.stroke)[0] else { panic!("not a quad") };
    assert!((q.p1.y - 1.5).abs() < EPS, "{:?}", q.p1);
    assert!((q.p1.x - 1.0).abs() < EPS, "{:?}", q.p1);
}

/// A relative curve with no pen is skipped, as any row that needs one, and the size of 0 gives a point curve and no NaN.
#[test]
fn a_bezier_row_needs_a_pen_and_survives_a_collapsed_size() {
    let none = build(&[cubic(1.0, 0.0, (0.0, 1.0), (1.0, 1.0))], 4.0, 2.0);
    assert_eq!(none.warnings.len(), 1);
    assert!(none.stroke.elements().is_empty());
    let collapsed = build(&[mv(0.0, 0.0), quad(1.0, 1.0, (0.5, 0.5))], 0.0, 0.0);
    assert!(collapsed.warnings.is_empty());
    assert_eq!(segments(&collapsed.stroke).len(), 1);
}
