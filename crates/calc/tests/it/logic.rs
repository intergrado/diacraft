//! Every vector of the batch, as data.

use diacraft_calc::CellError::{self, DivZero, Na, Ref};
use diacraft_doc::RowKey;
use diacraft_testkit::build::ShapeBuilder;
use diacraft_units::Unit::{Centimetre, Inch};

use crate::{E, check_table, check_table_on, eval_on, mismatch};

/// `Scratch.A1` holds 2 and its formula now fails with `#DIV/0!`; `Scratch.B1` holds `Scratch.A1*5` and 10.
fn failing_cell(s: ShapeBuilder) -> ShapeBuilder {
    s.row_formula("Scratch", None, RowKey::Ix(0), "A", "1/0", "2").row_formula("Scratch", None, RowKey::Ix(0), "B", "Scratch.A1*5", "10")
}

fn with_height_and_pin(height: &str, pin: &str) -> impl Fn(ShapeBuilder) -> ShapeBuilder {
    let (height, pin) = (height.to_owned(), pin.to_owned());
    move |s| s.cell("Height", &height).cell("PinX", &pin)
}

#[test]
fn all_vectors() {
    check_table(&[
        ("ALL", "NOT(-3)", E::B(false)),
        ("ALL", "NOT(0.25)", E::B(false)),
        ("ALL", "NOT(4E-12)", E::B(false)),
        ("ALL", "IF(4,5,7)", E::N(5.0)),
        ("ALL", "AND(2,3)", E::B(true)),
        ("ALL", "NOT(4 ft)", E::B(false)),
        ("ALL", "NOT(0 ft)", E::B(true)),
        ("ALL", "NOT(\"0\")", E::B(true)),
        ("ALL", "NOT(\"1=0\")", E::B(true)),
        ("ALL", "NOT(\"goat\")", E::B(true)),
        ("ALL", "IF(\"goat\",5,7)", E::N(7.0)),
        ("ALL", "NOT(\"\")", E::B(true)),
        ("ALL", "IF(4>2,5,7/0)", E::N(5.0)),
        ("ALL", "IFERROR(4,7/0)", E::N(4.0)),
        ("ALL", "IF(7/0,5,7)", E::Err(DivZero)),
        ("ALL", "ISERROR(7/0)", E::B(true)),
    ]);
}

#[test]
fn all_a_string_naming_a_cell_is_read_as_a_formula() {
    let v = eval_on(|s| s.cell("Width", "3"), "NOT(\"Width\")");
    assert_eq!(mismatch(&v, &E::B(false)), None, "{v:?}");
}

#[test]
fn all_the_tests_see_the_error_of_a_bare_reference_and_arithmetic_does_not() {
    let rows = [
        ("ALL", "ISERROR(Scratch.A1)", E::B(true)),
        ("ALL", "Scratch.A1*5", E::N(10.0)),
        ("ALL", "ISERROR(Scratch.A1*2)", E::B(false)),
        ("ALL", "IFERROR(Scratch.A1*2,9)", E::N(4.0)),
        ("ALL", "ISERROR(Scratch.B1)", E::B(false)),
        ("IFERROR", "IFERROR(Scratch.A1,9)", E::N(9.0)),
        ("operators", "Scratch.A1*5", E::N(10.0)),
        ("cells", "GUARD(Scratch.A1)", E::N(2.0)),
        ("cells", "DEPENDSON(Scratch.A3)", E::Err(Ref)),
    ];
    // The last row names a cell that does not exist, so it is a lost reference and not the failing one.
    check_table_on(failing_cell, &rows[..8]);
}

#[test]
fn all_the_na_spelling_is_the_one_ms_vsdx_requires() {
    assert_eq!(CellError::Na.text(), "#N/A");
    assert_eq!(CellError::from_text("#N/A!"), Some(CellError::Na));
    assert_eq!(CellError::from_text("#NA!"), Some(CellError::Na));
    assert_eq!(CellError::from_text("#DIV/0"), Some(CellError::DivZero));
    assert_eq!(CellError::from_text("#NUM"), Some(CellError::Num));
    assert_eq!(CellError::from_text("nonsense"), None);
}

#[test]
fn and_or_not() {
    check_table_on(
        with_height_and_pin("2", "0.5"),
        &[("AND", "AND(Height > 1, PinX > 1)", E::B(false)), ("OR", "OR(Height > 1,PinX > 1)", E::B(true))],
    );
    check_table_on(with_height_and_pin("2", "3"), &[("AND", "AND(Height > 1, PinX > 1)", E::B(true))]);
    check_table_on(with_height_and_pin("0.5", "0.5"), &[("OR", "OR(Height > 1,PinX > 1)", E::B(false))]);
    check_table(&[
        ("AND", "AND(3>2,1>2,4>2)", E::B(false)),
        ("AND", "AND(5)", E::B(true)),
        ("AND", "AND(0)", E::B(false)),
        ("AND", "AND(1>2,7/0)", E::Err(DivZero)),
        ("AND", "AND(7/0,1>2)", E::Err(DivZero)),
        ("AND", "AND(NA(),2>1)", E::Err(Na)),
        ("OR", "OR(4<2,3>2)", E::B(true)),
        ("OR", "OR(7)", E::B(true)),
        ("OR", "OR(0)", E::B(false)),
        ("OR", "OR(4>2,7/0)", E::Err(DivZero)),
        ("OR", "OR(7/0,4>2)", E::Err(DivZero)),
        ("OR", "OR(4<2,,3<2)", E::B(false)),
        ("OR", "OR(4<2,3>2,)", E::B(true)),
        ("NOT", "NOT(4>2)", E::B(false)),
        ("NOT", "NOT(0)", E::B(true)),
        ("NOT", "NOT(NA())", E::Err(Na)),
        ("NOT", "NOT(0)*7", E::N(7.0)),
        ("AND", "AND(4>2,,3>2)", E::B(true)),
    ]);
    check_table_on(|s| s.cell_u("Height", "2", "IN"), &[("NOT", "NOT(Height > 0.75 in)", E::B(false))]);
    check_table_on(|s| s.cell_u("Height", "0.75", "IN"), &[("NOT", "NOT(Height > 0.75 in)", E::B(true))]);
}

#[test]
fn if_vectors() {
    check_table_on(|s| s.cell_u("Height", "2", "IN"), &[("IF", "IF(Height > 1.25 in,5,7)", E::N(5.0))]);
    check_table_on(|s| s.cell_u("Height", "1.25", "IN"), &[("IF", "IF(Height > 1.25 in,5,7)", E::N(7.0))]);
    check_table(&[
        ("IF", "IF(4>2,5,7/0)", E::N(5.0)),
        ("IF", "IF(4<2,REF(),7)", E::N(7.0)),
        ("IF", "IF(4>2,NA(),7)", E::Err(Na)),
        ("IF", "IF(7/0,5,7)", E::Err(DivZero)),
        ("IF", "IF(NA(),5,5)", E::Err(Na)),
        ("IF", "IF(4>2,3 cm,2 in)", E::U(3.0 / 2.54, Centimetre)),
        ("IF", "IF(4<2,3 cm,2 in)", E::U(2.0, Inch)),
        ("IF", "IF(4>2,\"yes\",7)", E::T("yes")),
        ("IF", "IF(4>2,,7)", E::B(true)),
        ("IF", "IF(4<2,5,)", E::B(false)),
        ("IF", "IF(\"1=0\",5,7)", E::N(7.0)),
        ("ALL", "IF(4>2)", E::Err(CellError::Value)),
        ("ALL", "IF(4>2,5)", E::Err(CellError::Value)),
        ("ALL", "IF(4>2,5,7,9)", E::Err(CellError::Value)),
        ("ALL", "NOT()", E::Err(CellError::Value)),
        ("ALL", "AND()", E::Err(CellError::Value)),
        ("ALL", "IFERROR(4)", E::Err(CellError::Value)),
        ("ALL", "NA(1)", E::Err(CellError::Value)),
        ("ALL", "REF(1)", E::Err(CellError::Value)),
    ]);
}

#[test]
fn if_setf_writes_nothing_while_a_cell_is_evaluated() {
    // The branch's value is 0 and `Scratch.A1` stays what it was, whether or not the branch is taken: the evaluator holds the document and never changes
    // it. The commit that worked the cell out makes the write the call asked for (`setf_effects.rs`).
    for (a1, expected) in [("3", 3.0), ("7", 7.0)] {
        let formula = "IF(Scratch.A1>5,SETF(GetRef(Scratch.A1),5),0)";
        let setup = |s: ShapeBuilder| s.row_cell("Scratch", None, RowKey::Ix(0), "A", a1);
        let v = eval_on(setup, formula);
        assert_eq!(mismatch(&v, &E::N(0.0)), None, "A1 = {a1}: {v:?}");
        // The cell is read again after: nothing wrote to it.
        let v = eval_on(setup, "Scratch.A1");
        assert_eq!(mismatch(&v, &E::N(expected)), None, "A1 = {a1}: {v:?}");
    }
}

#[test]
fn iferror_vectors() {
    check_table(&[
        ("IFERROR", "IFERROR(7/0,5)", E::N(5.0)),
        ("IFERROR", "IFERROR(NA(),5)", E::N(5.0)),
        ("IFERROR", "IFERROR(SQRT(-4),5)", E::N(5.0)),
        ("IFERROR", "IFERROR(4,7/0)", E::N(4.0)),
        ("IFERROR", "IFERROR(3 in,0)", E::U(3.0, Inch)),
        ("IFERROR", "IFERROR(NA(),7/0)", E::Err(DivZero)),
        ("IFERROR", "IFERROR(NOT(7/0),FALSE)", E::B(false)),
        ("IFERROR", "IFERROR(NOT(7/0),TRUE)", E::B(true)),
        ("IFERROR", "IFERROR(7/0,)", E::B(false)),
    ]);
}

#[test]
fn the_error_tests() {
    check_table(&[
        ("ISERR", "ISERR(NA())", E::B(false)),
        ("ISERR", "ISERR(7/0)", E::B(true)),
        ("ISERR", "ISERR(REF())", E::B(true)),
        ("ISERR", "ISERR(SQRT(-4))", E::B(true)),
        ("ISERR", "ISERR(9)", E::B(false)),
        ("ISERR", "1+ISERR(7/0)", E::N(2.0)),
        ("ISERRNA", "ISERRNA(NA())", E::B(true)),
        ("ISERRNA", "ISERRNA(9)", E::B(false)),
        ("ISERRNA", "ISERRNA(7/0)", E::B(false)),
        ("ISERRNA", "ISERRNA(REF())", E::B(false)),
        ("ISERROR", "ISERROR(NA())", E::B(true)),
        ("ISERROR", "ISERROR(7/0)", E::B(true)),
        ("ISERROR", "ISERROR(REF())", E::B(true)),
        ("ISERROR", "ISERROR(SQRT(-4))", E::B(true)),
        ("ISERROR", "ISERROR(9)", E::B(false)),
        ("ISERROR", "ISERROR(\"goat\")", E::B(false)),
        ("ISERROR", "ISERROR(FALSE)", E::B(false)),
        ("ISERROR", "ISERROR(\"\")", E::B(false)),
        ("ISERROR", "NOT(ISERROR(NA()))", E::B(false)),
        ("ISERRVALUE", "ISERRVALUE(4+\"goat\")", E::B(true)),
        ("ISERRVALUE", "ISERRVALUE(7/0)", E::B(false)),
        ("ISERRVALUE", "ISERRVALUE(NA())", E::B(false)),
        ("ISERRVALUE", "ISERRVALUE(REF())", E::B(false)),
        ("ISERRVALUE", "ISERRVALUE(9)", E::B(false)),
        ("ISERRVALUE", "ISERRVALUE(\"goat\")", E::B(false)),
    ]);
}

#[test]
fn iserror_table_over_every_class() {
    // The table, row by row: (error as a formula, ISERROR, ISERR, ISERRNA, ISERRVALUE).
    let rows: [(&str, bool, bool, bool, bool); 6] = [
        ("7/0", true, true, false, false),
        ("4+\"goat\"", true, true, false, true),
        ("REF()", true, true, false, false),
        ("SQRT(-4)", true, true, false, false),
        ("NA()", true, false, true, false),
        ("POW(2 in,1.5)", true, true, false, false),
    ];
    for (f, iserror, iserr, iserrna, iserrvalue) in rows {
        for (name, expected) in [("ISERROR", iserror), ("ISERR", iserr), ("ISERRNA", iserrna), ("ISERRVALUE", iserrvalue)] {
            let v = crate::eval(&format!("{name}({f})"));
            assert_eq!(mismatch(&v, &E::B(expected)), None, "{name}({f})");
        }
    }
}

#[test]
fn iserrvalue_with_a_cell_given_a_word_and_na_ref() {
    check_table(&[
        ("NA", "NA()", E::Err(Na)),
        ("NA", "ISERRNA(NA())", E::B(true)),
        ("NA", "NA()+1", E::Err(Na)),
        ("NA", "MAX(NA(),3)", E::Err(Na)),
        ("REF", "REF()", E::Err(Ref)),
        ("REF", "ISERROR(REF())", E::B(true)),
        ("REF", "REF()*7", E::Err(Ref)),
        ("REF", "REF()=4", E::Err(Ref)),
    ]);
}

#[test]
fn a_reference_to_a_row_that_is_not_there_is_ref() {
    // And cells-sheets: project rule `#REF!`, and a test of it says so.
    check_table(&[
        ("ALL", "Prop.Nothing", E::Err(Ref)),
        ("ALL", "ISERROR(Prop.Nothing)", E::B(true)),
        ("ALL", "IFERROR(Prop.Nothing,3)", E::N(3.0)),
        ("ALL", "Scratch.C7", E::Err(Ref)),
    ]);
}

#[test]
fn a_failing_cell_with_no_saved_value_carries_its_error() {
    // A cell with `E` `#N/A` and no `V`.
    let setup = |s: ShapeBuilder| {
        use diacraft_doc::{Cell, CellAddr};
        let mut cell = Cell::new();
        cell.error = Some("#N/A".into());
        s.set(CellAddr::in_row("Scratch", None, RowKey::Ix(0), "A"), cell)
    };
    let v = eval_on(setup, "ISERRNA(Scratch.A1)");
    assert_eq!(mismatch(&v, &E::B(true)), None, "{v:?}");
}

#[test]
fn a_boolean_result_is_a_boolean_for_the_file() {
    // `V` 1 and `U` `BOOL`, not a plain number with no `U`.
    let v = crate::eval("AND(3>2,4>2)");
    let encoded = diacraft_calc::encode(&v).unwrap();
    assert_eq!((encoded.v.as_str(), encoded.u.as_deref()), ("1", Some("BOOL")));
}

#[test]
fn a_string_that_is_not_a_formula_reads_false() {
    // A string that cannot be read is FALSE, one that does not even parse included.
    check_table(&[
        ("ALL", r#"NOT(")(")"#, E::B(true)),
        ("ALL", r#"NOT("1 +")"#, E::B(true)),
        ("ALL", r#"AND(TRUE,")(")"#, E::B(false)),
        ("ALL", r#"OR(FALSE,")(")"#, E::B(false)),
    ]);
}
