//! `*` and `/` rank alike and run left to right, so a number with a unit that follows a division binds to
//! the division and the product takes the quotient: `Width/1 in*12` with a Width of 4 inches is `(4 in/1 in)*12`, 48.

use diacraft_units::Unit::{Centimetre, Foot, Inch};

use crate::{E, check_table_on};

/// The cause of the finding was the name, not the arithmetic: `WIDTH` read as a cell that is not there (0), where the reference application read it as the cell `Width` (observed on the
/// project's test PC, 2026-11-27, `User.Score = WIDTH/1 in*12` with a Width of 4 in giving 48). A cell the catalogue lists is named in any letter case.
#[test]
fn a_cell_the_catalogue_lists_is_named_in_any_letter_case() {
    check_table_on(
        |s| s.cell_u("Width", "4", "IN").cell_u("Height", "3", "IN").cell_u("PinX", "2", "IN"),
        &[
            ("case", "WIDTH", E::U(4.0, Inch)),
            ("case", "width", E::U(4.0, Inch)),
            ("case", "wIdTh*2", E::U(8.0, Inch)),
            ("case", "HEIGHT+WIDTH", E::U(7.0, Inch)),
            ("case", "pinx", E::U(2.0, Inch)),
            ("case", "PINX*Height", E::D(6.0, Inch, 2)),
            ("case", "WIDTH/1 in*12", E::N(48.0)),
            ("case", "Width/1 in*12", E::N(48.0)),
            ("case", "SQRT(WIDTH^2+HEIGHT^2)", E::U(5.0, Inch)),
            // After a sheet's name too, and with the sheet named by its `ID`.
            ("case", "Sheet.1!WIDTH", E::U(4.0, Inch)),
            ("case", "Sheet.1!HEIGHT*2", E::U(6.0, Inch)),
        ],
    );
}

/// A shape 4 inches wide.
fn four_inches(s: diacraft_testkit::build::ShapeBuilder) -> diacraft_testkit::build::ShapeBuilder {
    s.cell_u("Width", "4", "IN")
}

#[test]
fn a_length_over_a_length_with_a_unit_is_a_plain_number_that_a_product_goes_on_with() {
    check_table_on(
        four_inches,
        &[
            // The finding: a quotient of two lengths is a number, and the product after it is a number.
            ("DIV", "Width/1 in", E::N(4.0)),
            ("DIV", "Width/1 in*12", E::N(48.0)),
            ("DIV", "WIDTH/1 in*12", E::N(48.0)),
            ("DIV", "Width/1 in*12 in", E::U(48.0, Inch)),
            ("DIV", "Width/1 in/2", E::N(2.0)),
            ("DIV", "Width/1 in*0", E::N(0.0)),
            // The same shapes with the spaces left out, and the unit in capitals.
            ("R11", "Width/1in*12", E::N(48.0)),
            ("R11", "Width/1IN*12", E::N(48.0)),
            // Another unit on the divisor: the ratio is of stored inches.
            ("DIV", "Width/1 cm", E::N(4.0 * 2.54)),
            ("DIV", "Width/1 cm*12", E::N(4.0 * 2.54 * 12.0)),
            ("DIV", "Width/1 ft*12", E::N(4.0 / 12.0 * 12.0)),
            // A product first, then a division by a number with a unit: still left to right.
            ("MUL", "Width*12/1 in", E::N(48.0)),
            ("MUL", "Width*1 in", E::D(4.0, Inch, 2)),
            ("MUL", "Width*1 in/1 in", E::U(4.0, Inch)),
            ("MUL", "Width*1 cm", E::D(4.0 / 2.54, Inch, 2)),
            // A number with a unit in parentheses binds the same way.
            ("R12", "Width/(1 in)*12", E::N(48.0)),
            ("R12", "Width/(1 in*1)*12", E::N(48.0)),
            ("R12", "(Width/1 in)*12", E::N(48.0)),
            ("R12", "Width/(1 in*12)", E::N(4.0 / 12.0)),
            ("R12", "12*(Width/1 in)", E::N(48.0)),
            // Mixed units in one formula.
            ("DIV", "Width/1 ft*12 cm", E::U(4.0 / 12.0 * 12.0 / 2.54, Centimetre)),
            ("DIV", "Width/1 in*1 ft", E::U(48.0, Foot)),
        ],
    );
}
