//! What a tool's rows draw: the path they make (the geometry crate's, as the renderer builds it), the box round it, and the path flattened for a preview.

use diacraft_geom::{GeomRow, Nurbs, NurbsPoint, Polyline, build_path};
use kurbo::{BezPath, PathEl, Point, Rect, Shape as _, Size};

use super::P;
use crate::draw::shapeops::CRow;

/// How far the preview's lines may stand from the curve they stand for, in inches (a fifth of a screen point).
const FLATTEN: f64 = 0.002;

/// The most points a preview outline holds: a curve flattened finer than a screen has pixels is not shown finer.
const MAX_OUTLINE_POINTS: usize = 20_000;

/// The rows as the geometry crate reads them, in absolute coordinates: the relative rows are written as shares of a box of 1 by 1, which is the same numbers, so the path is
/// built for a [`Size`] of 1 by 1.
fn geom_row(row: &CRow) -> GeomRow {
    let at = |p: &Point| (p.x, p.y);
    match row {
        CRow::Move(p) => GeomRow::MoveTo { x: p.x, y: p.y },
        CRow::Line(p) => GeomRow::LineTo { x: p.x, y: p.y },
        CRow::Arc { to, bow } => GeomRow::ArcTo { x: to.x, y: to.y, bow: *bow },
        CRow::EllipticalArc { to, mid, angle, ratio } => {
            GeomRow::EllipticalArcTo { x: to.x, y: to.y, mid_x: mid.x, mid_y: mid.y, angle: *angle, ratio: *ratio }
        }
        CRow::Ellipse { centre, first, second } => {
            GeomRow::Ellipse { x: centre.x, y: centre.y, first_x: first.x, first_y: first.y, second_x: second.x, second_y: second.y }
        }
        CRow::Cubic { c1, c2, to } => GeomRow::RelCubBezTo { x: to.x, y: to.y, c1_x: c1.x, c1_y: c1.y, c2_x: c2.x, c2_y: c2.y },
        CRow::Quad { c, to } => GeomRow::RelQuadBezTo { x: to.x, y: to.y, c_x: c.x, c_y: c.y },
        CRow::Polyline { to, points } => GeomRow::PolylineTo {
            x: to.x,
            y: to.y,
            polyline: Some(Polyline { x_is_share: false, y_is_share: false, points: points.iter().map(at).collect() }),
        },
        CRow::Nurbs { to, knots, last_knot, degree, points } => GeomRow::NurbsTo {
            x: to.x,
            y: to.y,
            second_last_knot: knots.second_last_knot,
            last_weight: knots.last_weight,
            first_knot: knots.first_knot,
            first_weight: knots.first_weight,
            nurbs: Some(Nurbs {
                last_knot: *last_knot,
                degree: *degree,
                x_is_share: false,
                y_is_share: false,
                points: points.iter().map(|(p, knot, weight)| NurbsPoint { x: p.x, y: p.y, knot: *knot, weight: *weight }).collect(),
            }),
        },
        CRow::SplineStart { to, second_knot, first_knot, last_knot, degree } => {
            GeomRow::SplineStart { x: to.x, y: to.y, second_knot: *second_knot, first_knot: *first_knot, last_knot: *last_knot, degree: *degree }
        }
        CRow::SplineKnot { to, knot } => GeomRow::SplineKnot { x: to.x, y: to.y, knot: *knot },
    }
}

/// The rows as the geometry crate reads them.
pub(super) fn geom_rows(rows: &[CRow]) -> Vec<GeomRow> {
    rows.iter().map(geom_row).collect()
}

/// The path the rows draw, as the renderer builds it; `None` when the geometry crate refuses it.
pub(crate) fn path_of(rows: &[CRow]) -> Option<BezPath> {
    build_path(&geom_rows(rows), Size::new(1.0, 1.0)).ok()
}

/// The box round what the rows draw, or `None` when they draw nothing or the path cannot be built.
pub(crate) fn bounds(rows: &[CRow]) -> Option<Rect> {
    let path = path_of(rows)?;
    if path.elements().is_empty() {
        return None;
    }
    let rect = path.bounding_box();
    (rect.x0.is_finite() && rect.y0.is_finite() && rect.x1.is_finite() && rect.y1.is_finite()).then_some(rect)
}

/// The path the rows draw, flattened to a run of points (a preview's outline). A path of several runs is joined end to end.
pub(super) fn flatten(rows: &[CRow]) -> Vec<P> {
    let Some(path) = path_of(rows) else { return Vec::new() };
    let mut out: Vec<P> = Vec::new();
    kurbo::flatten(path.elements().iter().copied(), FLATTEN, |el| {
        if out.len() >= MAX_OUTLINE_POINTS {
            return;
        }
        match el {
            PathEl::MoveTo(p) | PathEl::LineTo(p) => out.push((p.x, p.y)),
            PathEl::ClosePath | PathEl::QuadTo(..) | PathEl::CurveTo(..) => {}
        }
    });
    out
}
