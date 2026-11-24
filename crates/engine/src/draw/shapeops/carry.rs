//! Combine's sections: the rows of each original, carried exactly (SHAPEOPS-COMBINE).
//!
//! Combine computes no area, so its sections are the originals' own paths and nothing is approximated: **a row keeps its kind** (a straight row stays straight,
//! and an arc, an elliptical arc, an `Ellipse`, a NURBS, a spline and a Bezier row stay what they were). What changes is the frame. A shape's pose is a move, a
//! turn and flips, never a scale, so every row type is carried by moving its points through the pose and writing its numbers again in the frame of the new
//! shape:
//!
//! * a point (`X`, `Y`, the middle point of an elliptical arc, the points of a constructor, the centre of an `Ellipse`) is moved;
//! * an `ArcTo`'s bow is a distance, so a turn leaves it, and a flip, which swaps right and left of the way of travel, **changes its sign**;
//! * an elliptical arc's axis angle `C` is the direction of a vector, turned and flipped with the points; its ratio `D` is a ratio and stays;
//! * knots, weights and a spline's degree are not lengths and stay;
//! * `RelMoveTo`, `RelLineTo` and `RelEllipticalArcTo` become `MoveTo`, `LineTo` and `EllipticalArcTo` (R10 writes the coordinates as shares of the new box, as
//!   formulas: it is the same move, line and arc), and the two relative Bezier rows (`RelCubBezTo` and `RelQuadBezTo`) keep their kind, their shares worked
//!   out again for the new box.
//!
//! An `InfiniteLine` is not drawn on a page's paper and has no part in the shape's area: it is left out. A row the drawing code skips or draws by a fallback
//! (a constructor that cannot be read) is carried as what it draws: a line to its end for a `PolylineTo`, a move for a `NURBSTo`.

use std::sync::Arc;

use diacraft_doc::{Cell, Row, Section, Value};
use diacraft_geom::{GeomRow, Nurbs, Polyline};
use kurbo::{Affine, Point};

use super::operand::Sect;
use super::rows::{Frame, flagged, length, plain, share_text};

/// A row in absolute coordinates, in whichever frame the points are in.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum CRow {
    Move(Point),
    Line(Point),
    Arc { to: Point, bow: f64 },
    EllipticalArc { to: Point, mid: Point, angle: f64, ratio: f64 },
    Ellipse { centre: Point, first: Point, second: Point },
    Cubic { c1: Point, c2: Point, to: Point },
    Quad { c: Point, to: Point },
    Polyline { to: Point, points: Vec<Point> },
    Nurbs { to: Point, knots: NurbsKnots, last_knot: f64, degree: f64, points: Vec<(Point, f64, f64)> },
    SplineStart { to: Point, second_knot: f64, first_knot: f64, last_knot: f64, degree: f64 },
    SplineKnot { to: Point, knot: f64 },
}

/// The four cells of a `NURBSTo` row that are not a point: `A`, `B`, `C` and `D`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct NurbsKnots {
    pub second_last_knot: f64,
    pub last_weight: f64,
    pub first_knot: f64,
    pub first_weight: f64,
}

/// The rows of a section, moved by `map` (the shape's own frame to the container's) from a shape of `size`, which the relative rows are shares of.
pub(crate) fn carry(rows: &[GeomRow], size: (f64, f64), map: Affine) -> Vec<CRow> {
    let [a, b, c, d, _, _] = map.as_coeffs();
    // The determinant of the linear part is -1 for a flip of one axis (and 1 for a turn, or a flip of both).
    let flipped = a * d - b * c < 0.0;
    let at = |x: f64, y: f64| map * Point::new(x, y);
    let rel = |x: f64, y: f64| map * Point::new(x * size.0, y * size.1);
    // A direction (a vector) goes through the linear part only.
    let direction = |angle: f64| {
        let v = map.as_coeffs();
        let (dx, dy) = (angle.cos(), angle.sin());
        let (x, y) = (v[0] * dx + v[2] * dy, v[1] * dx + v[3] * dy);
        y.atan2(x)
    };
    let constructor_point =
        |x: f64, y: f64, x_share: bool, y_share: bool| at(if x_share { x * size.0 } else { x }, if y_share { y * size.1 } else { y });
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let carried = match row {
            GeomRow::MoveTo { x, y } => CRow::Move(at(*x, *y)),
            GeomRow::RelMoveTo { x, y } => CRow::Move(rel(*x, *y)),
            GeomRow::LineTo { x, y } => CRow::Line(at(*x, *y)),
            GeomRow::RelLineTo { x, y } => CRow::Line(rel(*x, *y)),
            GeomRow::ArcTo { x, y, bow } => CRow::Arc { to: at(*x, *y), bow: if flipped { -*bow } else { *bow } },
            GeomRow::EllipticalArcTo { x, y, mid_x, mid_y, angle, ratio } => {
                CRow::EllipticalArc { to: at(*x, *y), mid: at(*mid_x, *mid_y), angle: direction(*angle), ratio: *ratio }
            }
            GeomRow::RelEllipticalArcTo { x, y, mid_x, mid_y, angle, ratio } => {
                CRow::EllipticalArc { to: rel(*x, *y), mid: rel(*mid_x, *mid_y), angle: direction(*angle), ratio: *ratio }
            }
            GeomRow::PolylineTo { x, y, polyline } => match polyline {
                Some(Polyline { x_is_share, y_is_share, points }) => CRow::Polyline {
                    to: at(*x, *y),
                    points: points.iter().map(|(px, py)| constructor_point(*px, *py, *x_is_share, *y_is_share)).collect(),
                },
                None => CRow::Line(at(*x, *y)),
            },
            GeomRow::NurbsTo { x, y, second_last_knot, last_weight, first_knot, first_weight, nurbs } => match nurbs {
                Some(Nurbs { last_knot, degree, x_is_share, y_is_share, points }) => CRow::Nurbs {
                    to: at(*x, *y),
                    knots: NurbsKnots {
                        second_last_knot: *second_last_knot,
                        last_weight: *last_weight,
                        first_knot: *first_knot,
                        first_weight: *first_weight,
                    },
                    last_knot: *last_knot,
                    degree: *degree,
                    points: points.iter().map(|p| (constructor_point(p.x, p.y, *x_is_share, *y_is_share), p.knot, p.weight)).collect(),
                },
                None => CRow::Move(at(*x, *y)),
            },
            GeomRow::SplineStart { x, y, second_knot, first_knot, last_knot, degree } => {
                CRow::SplineStart { to: at(*x, *y), second_knot: *second_knot, first_knot: *first_knot, last_knot: *last_knot, degree: *degree }
            }
            GeomRow::SplineKnot { x, y, knot } => CRow::SplineKnot { to: at(*x, *y), knot: *knot },
            GeomRow::RelCubBezTo { x, y, c1_x, c1_y, c2_x, c2_y } => CRow::Cubic { c1: rel(*c1_x, *c1_y), c2: rel(*c2_x, *c2_y), to: rel(*x, *y) },
            GeomRow::RelQuadBezTo { x, y, c_x, c_y } => CRow::Quad { c: rel(*c_x, *c_y), to: rel(*x, *y) },
            GeomRow::Ellipse { x, y, first_x, first_y, second_x, second_y } => {
                CRow::Ellipse { centre: at(*x, *y), first: at(*first_x, *first_y), second: at(*second_x, *second_y) }
            }
            GeomRow::InfiniteLine { .. } => continue,
        };
        out.push(carried);
    }
    out
}

// ---- writing --------------------------------------------------------------------------------------------------------------------

fn row(ix: u32, row_type: &str, cells: Vec<(&str, Cell)>) -> Arc<Row> {
    let mut row = Row { ix: Some(ix), row_type: Some(Arc::from(row_type)), ..Row::default() };
    for (name, cell) in cells {
        row.cells.insert(diacraft_doc::CellName::new(name), cell);
    }
    Arc::new(row)
}

/// A number held to twelve places, as a cell: no unit.
fn number(n: f64) -> Cell {
    let n = if n.is_finite() { n } else { 0.0 };
    Cell::constant(Value::from_text(&share_text(n)))
}

/// A share of the box as a value of a cell of a relative row (`RelCubBezTo`, `RelQuadBezTo`).
fn share_value(offset: f64, size: f64) -> Cell {
    number(super::rows::share_of(offset, size))
}

fn pair(frame: &Frame, p: Point) -> String {
    format!(
        "{},{}",
        share_text(super::rows::share_of(p.x - frame.min.x, frame.width)),
        share_text(super::rows::share_of(p.y - frame.min.y, frame.height))
    )
}

/// The row `ix` for a carried row, its coordinates shares of `frame`.
pub(crate) fn row_of(carried: &CRow, ix: u32, frame: &Frame) -> Arc<Row> {
    let x = |p: Point| super::rows::share_cell("Width", super::rows::share_of(p.x - frame.min.x, frame.width), frame.width);
    let y = |p: Point| super::rows::share_cell("Height", super::rows::share_of(p.y - frame.min.y, frame.height), frame.height);
    let xy = |p: Point| [("X", x(p)), ("Y", y(p))];
    match carried {
        CRow::Move(p) => row(ix, "MoveTo", xy(*p).into()),
        CRow::Line(p) => row(ix, "LineTo", xy(*p).into()),
        CRow::Arc { to, bow } => {
            let [px, py] = xy(*to);
            row(ix, "ArcTo", vec![px, py, ("A", length(*bow))])
        }
        CRow::EllipticalArc { to, mid, angle, ratio } => {
            let [px, py] = xy(*to);
            row(ix, "EllipticalArcTo", vec![px, py, ("A", x(*mid)), ("B", y(*mid)), ("C", number(*angle)), ("D", number(*ratio))])
        }
        CRow::Ellipse { centre, first, second } => {
            let [px, py] = xy(*centre);
            row(ix, "Ellipse", vec![px, py, ("A", x(*first)), ("B", y(*first)), ("C", x(*second)), ("D", y(*second))])
        }
        CRow::Cubic { c1, c2, to } => {
            let (sx, sy) = (|p: Point| share_value(p.x - frame.min.x, frame.width), |p: Point| share_value(p.y - frame.min.y, frame.height));
            row(ix, "RelCubBezTo", vec![("X", sx(*to)), ("Y", sy(*to)), ("A", sx(*c1)), ("B", sy(*c1)), ("C", sx(*c2)), ("D", sy(*c2))])
        }
        CRow::Quad { c, to } => {
            let (sx, sy) = (|p: Point| share_value(p.x - frame.min.x, frame.width), |p: Point| share_value(p.y - frame.min.y, frame.height));
            row(ix, "RelQuadBezTo", vec![("X", sx(*to)), ("Y", sy(*to)), ("A", sx(*c)), ("B", sy(*c))])
        }
        CRow::Polyline { to, points } => {
            let [px, py] = xy(*to);
            let mut text = String::from("POLYLINE(0,0");
            for p in points {
                text.push(',');
                text.push_str(&pair(frame, *p));
            }
            text.push(')');
            row(ix, "PolylineTo", vec![px, py, ("A", Cell::constant(Value::from_text(&text)).with_unit("POLYLINE"))])
        }
        CRow::Nurbs { to, knots, last_knot, degree, points } => {
            let [px, py] = xy(*to);
            let mut text = format!("NURBS({},{},0,0", share_text(*last_knot), share_text(*degree));
            for (p, knot, weight) in points {
                text.push_str(&format!(",{},{},{}", pair(frame, *p), share_text(*knot), share_text(*weight)));
            }
            text.push(')');
            row(
                ix,
                "NURBSTo",
                vec![
                    px,
                    py,
                    ("A", number(knots.second_last_knot)),
                    ("B", number(knots.last_weight)),
                    ("C", number(knots.first_knot)),
                    ("D", number(knots.first_weight)),
                    ("E", Cell::constant(Value::from_text(&text)).with_unit("NURBS")),
                ],
            )
        }
        CRow::SplineStart { to, second_knot, first_knot, last_knot, degree } => {
            let [px, py] = xy(*to);
            row(
                ix,
                "SplineStart",
                vec![px, py, ("A", number(*second_knot)), ("B", number(*first_knot)), ("C", number(*last_knot)), ("D", number(*degree))],
            )
        }
        CRow::SplineKnot { to, knot } => {
            let [px, py] = xy(*to);
            row(ix, "SplineKnot", vec![px, py, ("A", number(*knot))])
        }
    }
}

/// The section a carried section becomes: its own rows in the new frame, and the values of its own `NoFill`, `NoLine`, `NoSnap` and `NoQuickDrag`.
pub(super) fn section_of(rows: &[CRow], from: &Sect, frame: &Frame, ix: u32) -> Section {
    let mut section = flagged(ix, from.flags.no_fill);
    for (name, on) in [("NoLine", from.flags.no_line), ("NoSnap", from.no_snap), ("NoQuickDrag", from.no_quick_drag)] {
        section.cells.insert(diacraft_doc::CellName::new(name), plain(if on { 1.0 } else { 0.0 }));
    }
    for (i, carried) in rows.iter().enumerate() {
        section.rows.push(row_of(carried, u32::try_from(i + 1).unwrap_or(u32::MAX), frame));
    }
    section
}

/// A vector, for the tests of a direction: the unit vector at `angle`.
#[cfg(test)]
pub(super) fn unit(angle: f64) -> kurbo::Vec2 {
    kurbo::Vec2::new(angle.cos(), angle.sin())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn turned(angle: f64) -> Affine {
        Affine::translate((3.0, 1.0)) * Affine::rotate(angle)
    }

    fn flipped_x() -> Affine {
        Affine::translate((3.0, 1.0)) * Affine::scale_non_uniform(-1.0, 1.0)
    }

    #[test]
    fn a_turn_leaves_an_arcs_bow_and_a_flip_of_one_axis_changes_its_sign() {
        let rows = [GeomRow::MoveTo { x: 0.0, y: 0.0 }, GeomRow::ArcTo { x: 2.0, y: 0.0, bow: 0.5 }];
        let by_turn = carry(&rows, (2.0, 1.0), turned(0.7));
        let by_flip = carry(&rows, (2.0, 1.0), flipped_x());
        let bow = |rows: &[CRow]| match rows.get(1) {
            Some(CRow::Arc { bow, .. }) => *bow,
            other => panic!("{other:?}"),
        };
        assert_eq!(bow(&by_turn), 0.5);
        assert_eq!(bow(&by_flip), -0.5);
        // A flip of both axes is a half turn, which leaves it.
        let both = Affine::scale_non_uniform(-1.0, -1.0);
        assert_eq!(bow(&carry(&rows, (2.0, 1.0), both)), 0.5);
    }

    #[test]
    fn an_axis_angle_is_turned_and_reflected_with_the_points() {
        let row = [GeomRow::MoveTo { x: 0.0, y: 0.0 }, GeomRow::EllipticalArcTo { x: 2.0, y: 0.0, mid_x: 1.0, mid_y: 1.0, angle: 0.3, ratio: 2.0 }];
        let angle_of = |map: Affine| match carry(&row, (2.0, 1.0), map).get(1) {
            Some(CRow::EllipticalArc { angle, ratio, .. }) => (*angle, *ratio),
            other => panic!("{other:?}"),
        };
        let (turned_angle, ratio) = angle_of(turned(0.7));
        assert!((turned_angle - 1.0).abs() < 1e-12 && ratio == 2.0);
        let (flipped_angle, _) = angle_of(flipped_x());
        // A reflection about the y axis sends the direction at 0.3 to the direction at pi - 0.3.
        assert!((flipped_angle - (std::f64::consts::PI - 0.3)).abs() < 1e-12, "{flipped_angle}");
        let direction = unit(flipped_angle);
        assert!((direction.x + 0.3_f64.cos()).abs() < 1e-12 && (direction.y - 0.3_f64.sin()).abs() < 1e-12);
    }

    #[test]
    fn relative_rows_are_shares_of_the_original_size_and_become_points() {
        let rows = [
            GeomRow::RelMoveTo { x: 0.5, y: 0.0 },
            GeomRow::RelLineTo { x: 1.0, y: 0.5 },
            GeomRow::RelCubBezTo { x: 1.0, y: 1.0, c1_x: 1.0, c1_y: 0.75, c2_x: 0.75, c2_y: 1.0 },
        ];
        let carried = carry(&rows, (4.0, 2.0), Affine::translate((10.0, 20.0)));
        assert_eq!(carried[0], CRow::Move(Point::new(12.0, 20.0)));
        assert_eq!(carried[1], CRow::Line(Point::new(14.0, 21.0)));
        assert_eq!(carried[2], CRow::Cubic { c1: Point::new(14.0, 21.5), c2: Point::new(13.0, 22.0), to: Point::new(14.0, 22.0) });
    }

    #[test]
    fn an_unreadable_constructor_is_carried_as_what_it_draws_and_an_infinite_line_is_left_out() {
        let rows = [
            GeomRow::MoveTo { x: 0.0, y: 0.0 },
            GeomRow::PolylineTo { x: 1.0, y: 1.0, polyline: None },
            GeomRow::NurbsTo { x: 2.0, y: 2.0, second_last_knot: 0.0, last_weight: 1.0, first_knot: 0.0, first_weight: 1.0, nurbs: None },
            GeomRow::InfiniteLine { x: 0.0, y: 0.0, other_x: 1.0, other_y: 1.0 },
        ];
        let carried = carry(&rows, (2.0, 2.0), Affine::IDENTITY);
        assert_eq!(carried.len(), 3);
        assert_eq!(carried[1], CRow::Line(Point::new(1.0, 1.0)));
        assert_eq!(carried[2], CRow::Move(Point::new(2.0, 2.0)));
    }

    #[test]
    fn a_polyline_in_lengths_and_one_in_shares_are_carried_to_the_same_points() {
        let shares = Polyline { x_is_share: true, y_is_share: true, points: vec![(0.5, 0.5)] };
        let lengths = Polyline { x_is_share: false, y_is_share: false, points: vec![(2.0, 1.0)] };
        let row = |polyline| [GeomRow::PolylineTo { x: 4.0, y: 2.0, polyline: Some(polyline) }];
        assert_eq!(carry(&row(shares), (4.0, 2.0), Affine::IDENTITY), carry(&row(lengths), (4.0, 2.0), Affine::IDENTITY));
    }
}
