//! An empty text cell given to a function that wants a number.
//!
//! `unknown`: "An empty cell with no `U` given to `MAX` raises no error in 2 cells (1 file, 1 source), and may
//! count as no value at all." Catalogued the `User` and `Property` rows, an empty `Prompt` is the empty string where it was
//! a bare 0, and `MAX` given it was `#VALUE!` where the file's cached value says 2. **Project rule:** an empty string given to a function by
//! `vDouble` is 0, the typed empty value names (reads an empty slot the same way). The rows below say what
//! 0 (and not "no value at all") gives under `MIN` and `MAX`, and that the operators, a word and a string of spaces are as they were.

use diacraft_calc::CellError::Value;
use diacraft_doc::RowKey;
use diacraft_testkit::build::ShapeBuilder;

use crate::{E, check_table_on};

/// A shape with the `User` row of the sample case: `Zed`, whose `Value` is 0 and whose `Prompt` is the empty string.
fn zed(shape: ShapeBuilder) -> ShapeBuilder {
    shape.row_cell("User", None, RowKey::name("Zed"), "Value", "0").row_cell("User", None, RowKey::name("Zed"), "Prompt", "")
}

#[test]
fn max_takes_an_empty_text_cell_as_0() {
    check_table_on(
        zed,
        &[
            // The two cells of the sample file: the cached value is 2 either way the empty cell is read.
            ("ALL", "MAX(2,User.Zed.Prompt)", E::N(2.0)),
            ("ALL", "ROUND(MAX(User.Zed.Prompt,1.5),0)", E::N(2.0)),
            // 0 and "no value" part ways below the other operand: 0 is the larger of -2 and nothing.
            ("ALL", "MAX(-2,User.Zed.Prompt)", E::N(0.0)),
            ("ALL", "MAX(User.Zed.Prompt,-2,-7)", E::N(0.0)),
            // The control of the vector: the number cell beside it.
            ("ALL", "MAX(2,User.Zed)", E::N(2.0)),
            // The empty string typed in the formula is the same empty value.
            ("ALL", "MAX(2,\"\")", E::N(2.0)),
        ],
    );
}

#[test]
fn min_takes_an_empty_text_cell_as_0() {
    check_table_on(
        zed,
        &[
            ("ALL", "MIN(5,User.Zed.Prompt)", E::N(0.0)),
            ("ALL", "MIN(User.Zed.Prompt,3,9)", E::N(0.0)),
            ("ALL", "MIN(User.Zed.Prompt,-3)", E::N(-3.0)),
            ("ALL", "MIN(-3,User.Zed.Prompt)", E::N(-3.0)),
            ("ALL", "MIN(\"\",4)", E::N(0.0)),
        ],
    );
}

#[test]
fn sum_and_the_functions_of_one_number_take_an_empty_text_cell_as_0() {
    check_table_on(
        zed,
        &[
            ("ALL", "SUM(2,User.Zed.Prompt,3)", E::N(5.0)),
            ("ALL", "ABS(User.Zed.Prompt)", E::N(0.0)),
            ("ALL", "SQRT(User.Zed.Prompt)", E::N(0.0)),
            ("ALL", "INT(User.Zed.Prompt)", E::N(0.0)),
            ("ALL", "SIGN(User.Zed.Prompt)", E::N(0.0)),
            ("ALL", "COS(User.Zed.Prompt)", E::N(1.0)),
            ("ALL", "ROUND(User.Zed.Prompt,2)", E::N(0.0)),
            ("ALL", "TRUNC(2.7,User.Zed.Prompt)", E::N(2.0)),
            ("ALL", "POW(User.Zed.Prompt,2)", E::N(0.0)),
            ("ALL", "POW(5,User.Zed.Prompt)", E::N(1.0)),
            ("ALL", "BITOR(User.Zed.Prompt,6)", E::N(6.0)),
        ],
    );
}

#[test]
fn the_empty_cell_is_a_reference_and_a_string_is_a_string() {
    // A cell that holds a string of letters or of spaces still cannot convert, and only the empty string is the empty value.
    check_table_on(
        |s| zed(s).row_cell("User", None, RowKey::name("Word"), "Prompt", "goat").row_cell("User", None, RowKey::name("Blank"), "Prompt", "  "),
        &[
            ("ALL", "MAX(2,User.Word.Prompt)", E::Err(Value)),
            ("ALL", "MAX(2,\"goat\")", E::Err(Value)),
            ("ALL", "MAX(2,User.Blank.Prompt)", E::Err(Value)),
            ("ALL", "MAX(2,\" \")", E::Err(Value)),
            ("ALL", "ABS(User.Word.Prompt)", E::Err(Value)),
        ],
    );
}

#[test]
fn the_operators_keep_the_project_rule_of_a_string_that_cannot_convert() {
    // `+ - * /`, the unary signs and `%` give `#VALUE!`, and files the empty string with them. The empty cell
    // given to a function is the only case this fix changes.
    check_table_on(
        zed,
        &[
            ("ALL", "4+User.Zed.Prompt", E::Err(Value)),
            ("ALL", "4-User.Zed.Prompt", E::Err(Value)),
            ("ALL", "4*User.Zed.Prompt", E::Err(Value)),
            ("ALL", "4/User.Zed.Prompt", E::Err(Value)),
            ("ALL", "4+\"\"", E::Err(Value)),
            ("UMINUS", "-User.Zed.Prompt", E::Err(Value)),
            ("UMINUS", "+User.Zed.Prompt", E::Err(Value)),
            ("PCT", "User.Zed.Prompt%", E::Err(Value)),
            ("ALL", "User.Zed.Prompt^2", E::Err(Value)),
        ],
    );
}

#[test]
fn a_comparison_still_counts_the_empty_cell_as_0() {
    // Unchanged, and the reason the empty value is 0 here too.
    check_table_on(
        zed,
        &[("ALL", "User.Zed.Prompt=0", E::B(true)), ("ALL", "User.Zed.Prompt<1", E::B(true)), ("ALL", "User.Zed.Prompt>-1", E::B(true))],
    );
}
