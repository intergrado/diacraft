//! `_MARKER` and the forms of the cells a shape glued to a guide holds,
//! worked out by the evaluator on their own: writes these formulas and the recalculation
//! of an edit works them out.

use diacraft_calc::CellError::{DivZero, Value as Bad};
use diacraft_units::Unit::DrawingDefault;

use crate::{E, check_table};

#[test]
fn marker_gives_back_its_argument_as_a_plain_number() {
    check_table(&[
        // The only argument the samples hold, 1, gives 1.
        ("MARKER", "_MARKER(1)", E::N(1.0)),
        // By the project rule: the argument comes back.
        ("MARKER", "_MARKER(3)", E::N(3.0)),
        ("MARKER", "_MARKER(0.25)", E::N(0.25)),
        // A plain number with no unit, so a length handed in comes back as its number (ours: no sample has one).
        ("MARKER", "_MARKER(2 in)", E::N(2.0)),
        // Our rule: an error argument comes back as the result.
        ("MARKER", "_MARKER(1/0)", E::Err(DivZero)),
        // One argument and no other count (as every function's: `#VALUE!`).
        ("MARKER", "_MARKER()", E::Err(Bad)),
        ("MARKER", "_MARKER(1,2)", E::Err(Bad)),
    ]);
}

#[test]
fn the_glued_cells_forms_give_the_guide_s_place_and_the_shape_s_own() {
    check_table(&[
        // An upright guide at x 3 (its pin at (3, 0), a quarter turn) and the shape's place (3.6, 1.5) in drawing units: the left edge's cell
        // is the guide's x, and the middle cell the shape's own y.
        ("ALIGN", "INTERSECTX(3,0,90 deg,3.6 DL,1.5 DL,90 deg+90 deg)", E::U(3.0, DrawingDefault)),
        ("ALIGN", "INTERSECTY(3,0,90 deg,3.6 DL,1.5 DL,90 deg+90 deg)", E::U(1.5, DrawingDefault)),
        // A crossing of an upright guide at x 2.5 and a level guide at y 4, both cells naming both in one order.
        ("ALIGN", "INTERSECTX(2.5,0,90 deg,0,4,0 deg)", E::N(2.5)),
        ("ALIGN", "INTERSECTY(2.5,0,90 deg,0,4,0 deg)", E::N(4.0)),
        // The naive form: the upright guide twice is parallel to itself.
        ("ALIGN", "INTERSECTY(2.5,0,90 deg,2.5,0,90 deg)", E::Err(DivZero)),
        // Our reasoning: a guide turned to 45 degrees through (0, 0) and the place (2, 0) give the foot of the square line, (1, 1).
        ("ALIGN", "INTERSECTX(0,0,45 deg,2 DL,0 DL,45 deg+90 deg)", E::U(1.0, DrawingDefault)),
        ("ALIGN", "INTERSECTY(0,0,45 deg,2 DL,0 DL,45 deg+90 deg)", E::U(1.0, DrawingDefault)),
    ]);
}
