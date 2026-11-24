//! The rows of a connector's route: a path of segments, in the connector's own frame, as `Geometry` rows.
//!
//! Every route DiaCraft writes goes through [`rows`]: the plain polyline (`MoveTo`, then `LineTo`), the curve of a curved connector (`MoveTo`, then one
//! `NURBSTo`), and the rows of a line jump (`ArcTo`, a `MoveTo` for a gap, `PolylineTo`). The cells are constants, as the files hold them
//! (no formula on `X`, `Y` or `A` of a jump's `ArcTo`, `U` `DL` on `A`; `E` is the
//! constructor with plain numbers, `U` `NURBS`), except the last row's `X` of a route that was edited by hand, which follows `Width` as the last row of
//! a file's fixed route does (`edit_foreign.rs`).

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Row, Value};

use super::route::P;
use crate::stencil::number_text;

/// The unit of the cell `A` of an `ArcTo` jump row in the files (drawing units).
const BOW_UNIT: &str = "DL";

/// One segment of a path. The pen starts at the first, a [`Seg::Move`].
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Seg {
    /// `MoveTo`: the pen goes to the point without drawing (the start of a path, and the gap of a jump).
    Move(P),
    /// `LineTo`.
    Line(P),
    /// `ArcTo` to a point; `bow` is `A`, the height of the arc above its chord (half the chord for a semicircle), negative to the left of the way the pen
    /// goes.
    Arc { to: P, bow: f64 },
    /// `PolylineTo`: the points on the way to `to`, which is `X` and `Y`; `via` is the constructor `POLYLINE` in `A`.
    Poly { to: P, via: Vec<P> },
    /// `NURBSTo`: a quadratic spline, clamped, from the pen through the control points `via` (the pen is the first control point) to `to`, which is the
    /// last. Needs at least one `via`.
    Spline { to: P, via: Vec<P> },
}

fn length(n: f64) -> Cell {
    Cell::constant(Value::from_text(&number_text(n))).with_unit("IN")
}

fn plain(n: f64) -> Cell {
    Cell::constant(Value::from_text(&number_text(n)))
}

fn put(row: &mut Row, name: &str, cell: Cell) {
    row.cells.insert(CellName::new(name), cell);
}

/// The `POLYLINE(1, 1, x1, y1, ...)` text: lengths in the shape's own units and not shares of its box.
fn polyline_text(via: &[P]) -> String {
    let mut text = String::from("POLYLINE(1,1");
    for p in via {
        text.push(',');
        text.push_str(&number_text(p.0));
        text.push(',');
        text.push_str(&number_text(p.1));
    }
    text.push(')');
    text
}

/// The knot at `place` of the list a clamped quadratic spline of `n` control points writes: the first `n + 1` of the usual `n + 3`
/// (0, 0, 0, 1, 2, ..., n - 2, n - 2, n - 2); a `NURBSTo` row lists those and implies the rest by repeating the last one.
fn knot(place: usize, n: usize) -> f64 {
    if place < 3 { 0.0 } else { place.saturating_sub(2).min(n.saturating_sub(2)) as f64 }
}

/// The `NURBS(...)` constructor and the cells `A`, `B`, `C`, `D` of a quadratic spline from the pen through `via` to `to` (`n` control points counting
/// the pen and `to`): `A` the second-to-last knot, `B` the last weight, `C` the first knot, `D` the first weight.
fn spline_cells(row: &mut Row, via: &[P]) {
    let n = via.len() + 2;
    let mut text = format!("NURBS({},2,1,1", number_text(knot(n, n)));
    for (i, p) in via.iter().enumerate() {
        text.push_str(&format!(",{},{},{},1", number_text(p.0), number_text(p.1), number_text(knot(i + 1, n))));
    }
    text.push(')');
    put(row, "A", plain(knot(n - 1, n)));
    put(row, "B", plain(1.0));
    put(row, "C", plain(knot(0, n)));
    put(row, "D", plain(1.0));
    put(row, "E", Cell::constant(Value::from_text(&text)).with_unit("NURBS"));
}

/// The rows of a path, numbered from 1. With `follow_width` the last row's `X` is `Width*1`, as the last row of a fixed route in a file is: the end
/// of the route stays on the end of the connector when the connector's ends move and its rows do not.
pub(crate) fn rows(segs: &[Seg], follow_width: bool) -> Vec<Arc<Row>> {
    let last = segs.len().saturating_sub(1);
    segs.iter()
        .enumerate()
        .map(|(i, seg)| {
            let ix = u32::try_from(i + 1).unwrap_or(u32::MAX);
            let (kind, to) = match seg {
                Seg::Move(p) => ("MoveTo", *p),
                Seg::Line(p) => ("LineTo", *p),
                Seg::Arc { to, .. } => ("ArcTo", *to),
                Seg::Poly { to, .. } => ("PolylineTo", *to),
                Seg::Spline { to, .. } => ("NURBSTo", *to),
            };
            let mut row = Row { ix: Some(ix), row_type: Some(Arc::from(kind)), ..Row::default() };
            let x = if follow_width && i == last && i > 0 {
                Cell::with_formula("Width*1", Some(Value::from_text(&number_text(to.0)))).with_unit("IN")
            } else {
                length(to.0)
            };
            put(&mut row, "X", x);
            put(&mut row, "Y", length(to.1));
            match seg {
                Seg::Arc { bow, .. } => put(&mut row, "A", plain(*bow).with_unit(BOW_UNIT)),
                Seg::Poly { via, .. } => put(&mut row, "A", Cell::constant(Value::from_text(&polyline_text(via))).with_unit("POLYLINE")),
                Seg::Spline { via, .. } => spline_cells(&mut row, via),
                Seg::Move(_) | Seg::Line(_) => {}
            }
            Arc::new(row)
        })
        .collect()
}

/// The polyline path through points: a `MoveTo` and a `LineTo` for each of the rest.
pub(crate) fn polyline(points: &[P]) -> Vec<Seg> {
    points.iter().enumerate().map(|(i, p)| if i == 0 { Seg::Move(*p) } else { Seg::Line(*p) }).collect()
}

/// The path of a curved connector through the control polygon `points`: a quadratic spline that starts on the first point and ends on the last,
/// both tangent to the legs there, and is drawn smooth inside the corners. Two points are one straight line.
pub(crate) fn curve(points: &[P]) -> Vec<Seg> {
    match points {
        [] => Vec::new(),
        [only] => vec![Seg::Move(*only)],
        [first, last] => vec![Seg::Move(*first), Seg::Line(*last)],
        [first, via @ .., last] => vec![Seg::Move(*first), Seg::Spline { to: *last, via: via.to_vec() }],
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use diacraft_geom::Nurbs;

    use super::*;

    fn text_of(row: &Row, name: &str) -> String {
        row.cells.iter().find(|(n, _)| n.as_str() == name).and_then(|(_, c)| c.value.as_ref()).map(|v| v.text().to_owned()).unwrap_or_default()
    }

    #[test]
    fn a_polyline_is_a_move_and_lines_numbered_from_1() {
        let rows = rows(&polyline(&[(0.0, 0.0), (2.0, 0.0), (2.0, 1.0)]), false);
        let kinds: Vec<_> = rows.iter().map(|r| (r.ix, r.row_type.as_deref().unwrap().to_owned())).collect();
        assert_eq!(kinds, [(Some(1), "MoveTo".to_owned()), (Some(2), "LineTo".to_owned()), (Some(3), "LineTo".to_owned())]);
        assert_eq!(text_of(&rows[2], "X"), "2");
        assert_eq!(text_of(&rows[2], "Y"), "1");
        assert!(rows.iter().all(|r| r.cells.values().all(|c| c.formula.text().is_none())), "constants, as the files hold them");
    }

    #[test]
    fn the_last_x_of_a_route_that_follows_the_width_is_the_width() {
        let rows = rows(&polyline(&[(0.0, 0.0), (2.0, 0.0), (2.0, 1.0), (4.0, 1.0)]), true);
        let x = |r: &Row| r.cells.iter().find(|(n, _)| n.as_str() == "X").map(|(_, c)| c.formula.text().map(str::to_owned)).unwrap();
        assert_eq!(x(&rows[3]), Some("Width*1".to_owned()));
        assert!(rows[..3].iter().all(|r| x(r).is_none()));
        // A lone move has no last row to follow with.
        assert!(super::rows(&[Seg::Move((0.0, 0.0))], true)[0].cells.values().all(|c| c.formula.text().is_none()));
    }

    #[test]
    fn a_jump_arc_has_plain_numbers_and_the_unit_dl_on_its_bow() {
        let rows = rows(&[Seg::Move((0.0, 0.0)), Seg::Line((2.47, 0.0)), Seg::Arc { to: (2.53, 0.0), bow: -0.03 }, Seg::Line((4.0, 0.0))], false);
        let arc = &rows[2];
        assert_eq!(arc.row_type.as_deref(), Some("ArcTo"));
        assert_eq!((text_of(arc, "X"), text_of(arc, "Y"), text_of(arc, "A")), ("2.53".to_owned(), "0".to_owned(), "-0.03".to_owned()));
        let a = arc.cells.iter().find(|(n, _)| n.as_str() == "A").map(|(_, c)| c).unwrap();
        assert_eq!(a.unit.as_deref(), Some("DL"));
        assert!(arc.cells.values().all(|c| c.formula.text().is_none()));
    }

    #[test]
    fn a_polyline_jump_holds_its_control_points_in_a_polyline_constructor_in_inches() {
        let rows = rows(&[Seg::Move((0.0, 0.0)), Seg::Poly { to: (2.53, 0.0), via: vec![(2.5, 0.03)] }], false);
        assert_eq!(text_of(&rows[1], "A"), "POLYLINE(1,1,2.5,0.03)");
        let cell = rows[1].cells.iter().find(|(n, _)| n.as_str() == "A").map(|(_, c)| c).unwrap();
        assert_eq!(cell.unit.as_deref(), Some("POLYLINE"));
        let parsed = diacraft_geom::Polyline::from_text("POLYLINE(1,1,2.5,0.03)").unwrap();
        assert_eq!((parsed.x_is_share, parsed.y_is_share, parsed.points.len()), (false, false, 1));
    }

    #[test]
    fn a_curve_through_four_points_is_a_quadratic_spline_the_geometry_crate_reads_back_as_clamped() {
        let segs = curve(&[(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (4.0, 2.0)]);
        let rows = rows(&segs, false);
        assert_eq!(rows.len(), 2);
        let spline = &rows[1];
        assert_eq!(spline.row_type.as_deref(), Some("NURBSTo"));
        // Four control points, the knots 0, 0, 0, 1, 2 listed and 2, 2 implied: C 0, the tuples' knots 0 and 0, A 1, the last knot 2.
        assert_eq!(text_of(spline, "E"), "NURBS(2,2,1,1,2,0,0,1,2,2,0,1)");
        assert_eq!(
            (text_of(spline, "A"), text_of(spline, "B"), text_of(spline, "C"), text_of(spline, "D")),
            ("1".into(), "1".into(), "0".into(), "1".into())
        );
        let nurbs = Nurbs::from_text(&text_of(spline, "E")).unwrap();
        assert_eq!((nurbs.last_knot, nurbs.degree, nurbs.points.len()), (2.0, 2.0, 2));
        // Three control points: knots 0, 0, 0, 1 listed.
        let l = rows_of_three();
        assert_eq!(text_of(&l[1], "E"), "NURBS(1,2,1,1,2,0,0,1)");
        assert_eq!((text_of(&l[1], "A"), text_of(&l[1], "C")), ("0".to_owned(), "0".to_owned()));
        // Five: 0, 0, 0, 1, 2, 3.
        let five = super::rows(&curve(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (2.0, 1.0), (2.0, 2.0)]), false);
        assert_eq!(text_of(&five[1], "E"), "NURBS(3,2,1,1,1,0,0,1,1,1,0,1,2,1,1,1)");
        assert_eq!(text_of(&five[1], "A"), "2");
    }

    fn rows_of_three() -> Vec<Arc<Row>> {
        rows(&curve(&[(0.0, 0.0), (2.0, 0.0), (2.0, 2.0)]), false)
    }

    #[test]
    fn a_curve_of_two_points_is_a_line_and_of_none_nothing() {
        assert_eq!(curve(&[(0.0, 0.0), (3.0, 1.0)]), vec![Seg::Move((0.0, 0.0)), Seg::Line((3.0, 1.0))]);
        assert!(curve(&[]).is_empty());
    }
}
