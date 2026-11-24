//! Every vector of the batch, as data. The currency and date rows are in `date_time.rs`, which has `CY` and `DATE`.

use diacraft_calc::CellError::{Dim, DivZero, Na, Value};
use diacraft_doc::RowKey;
use diacraft_units::Unit::{Acre, Centimetre, DrawingDefault, FeetInches, Foot, Inch, Percent};

use crate::{E, check_table, eval, eval_on, mismatch};

#[test]
fn all_vectors() {
    check_table(&[
        ("ALL", "_ADD(13, 29)", E::N(42.0)),
        ("ALL", "13+29", E::N(42.0)),
        ("ALL", "_FGT(13, 29)", E::B(false)),
        ("ALL", "13>29", E::B(false)),
        ("ALL", "7 ft*3", E::U(252.0, Foot)),
        ("ALL", "3+2.54 cm", E::U(4.0, Centimetre)),
        ("ALL", "TRUE+TRUE", E::N(2.0)),
        ("ALL", "(9>4)*6", E::N(6.0)),
        ("ALL", "\"7\"*6", E::N(42.0)),
        ("ALL", "\"3 in\"*2", E::U(6.0, Inch)),
        ("ALL", "\" 25%\"*8", E::N(2.0)),
        ("ALL", "\"1/8 in\"*2", E::U(0.25, Inch)),
        ("ALL", "\"2FT 3IN\"+1 in", E::U(28.0, FeetInches)),
        ("ALL", "\"3 sq. ft.\"*1", E::D(432.0, Foot, 2)),
        ("ALL", "4+\"goat\"", E::Err(Value)),
        ("ALL", "4-\"goat\"", E::Err(Value)),
        ("ALL", "4*\"goat\"", E::Err(Value)),
        ("ALL", "4/\"goat\"", E::Err(Value)),
        ("ALL", "\"goat\"=0", E::B(true)),
        ("ALL", "\"goat\"=1", E::B(false)),
        ("ALL", "\"goat\">-1", E::B(true)),
        ("ALL", "(7/0)+1", E::Err(DivZero)),
        ("ALL", "1-NA()", E::Err(Na)),
        ("ALL", "NA()&\"x\"", E::Err(Na)),
        ("ALL", "(7/0)=1", E::Err(DivZero)),
        ("ALL", "-NA()", E::Err(Na)),
        ("ALL", "1 ft=12 in", E::B(true)),
        ("ALL", "1 in=2.54 cm", E::B(true)),
        ("ALL", "1 ft=12", E::B(true)),
        ("ALL", "1=1+5E-10", E::B(true)),
        ("ALL", "1=1+2E-9", E::B(false)),
        ("ALL", "1E-10=9E-10", E::B(true)),
        ("ALL", "1000000000=1000000000.5", E::B(false)),
    ]);
}

#[test]
fn all_a_cell_with_no_unit_acts_as_the_default_unit() {
    // A `Width` cell with `V` 3 and no `U` is 3 `DL`, so a product keeps `DL`.
    let v = eval_on(|s| s.cell("Width", "3"), "Width*0.4321");
    assert_eq!(mismatch(&v, &E::U(1.2963, DrawingDefault)), None, "{v:?}");
}

#[test]
fn all_a_string_naming_cells_is_read_as_the_formula_it_spells() {
    let v = eval_on(|s| s.cell("Width", "3"), "\"Width*2\"*5");
    assert_eq!(mismatch(&v, &E::Any(30.0)), None, "{v:?}");
}

#[test]
fn all_a_reference_to_a_failing_cell_reads_its_last_good_value() {
    // `Scratch.A1` holds 2 and its formula now fails; `Scratch.A1*5` is 10 with no error.
    let setup = |s: diacraft_testkit::build::ShapeBuilder| s.row_formula("Scratch", None, RowKey::Ix(0), "A", "1/0", "2");
    let v = eval_on(setup, "Scratch.A1*5");
    assert_eq!(mismatch(&v, &E::N(10.0)), None, "{v:?}");
    // The cell itself holds the error, with its value kept.
    let v = eval_on(setup, "Scratch.A1");
    assert_eq!(mismatch(&v, &E::N(2.0)), None, "a reference reads the last good value: {v:?}");
}

#[test]
fn add_vectors() {
    check_table(&[
        ("ADD", "5 + 2", E::N(7.0)),
        ("ADD", "11+0.5", E::N(11.5)),
        ("ADD", "1 in+1 cm", E::U(1.0 + 1.0 / 2.54, Inch)),
        ("ADD", "1 in+2.54 cm", E::U(2.0, Inch)),
        ("ADD", "2.54 cm+1 in", E::U(2.0, Centimetre)),
        ("ADD", "1 DL+2.54 cm", E::U(2.0, Centimetre)),
        ("ADD", "2.54 cm+1 DL", E::U(2.0, Centimetre)),
        ("ADD", "3 IN^2+1", E::D(4.0, Inch, 2)),
        ("ADD", "3 IN^2+1 IN", E::U(4.0, Inch)),
        ("ADD", "1 ACRE+2", E::U(6272642.0, Acre)),
        ("ADD", "1 ACRE+1 IN", E::U(6272641.0, DrawingDefault)),
        ("ADD", "\"3\"+4", E::N(7.0)),
        ("ADD", "TRUE+4", E::N(5.0)),
        ("ADD", "SQRT(POW(3 DL,2)+POW(4 DL,2))", E::U(5.0, DrawingDefault)),
        ("ADD", "POW(2 DL,2)+POW(2.54 cm,2)", E::D(5.0, Centimetre, 2)),
    ]);
}

#[test]
fn cat_vectors() {
    check_table(&[
        ("CAT", "\"dog\" & \"house\"", E::T("doghouse")),
        ("CAT", "\"sea\"&\"shell\"&\"s\"", E::T("seashells")),
        ("CAT", "2 * 5 & \" cents\"", E::T("10 cents")),
        ("CAT", "7&8", E::T("78")),
        ("CAT", "1+2&3", E::T("33")),
        ("CAT", "_CAT(\"x\",5)", E::T("x5")),
        ("CAT", "_CAT(\"x\",NA())", E::Err(Na)),
        ("CAT", "25&\"x\"", E::T("25x")),
        ("CAT", "25 mm&\"x\"", E::T("25MMx")),
    ]);
}

#[test]
fn div_vectors() {
    check_table(&[
        ("DIV", "5 / 2", E::N(2.5)),
        ("DIV", "90/3/6", E::N(5.0)),
        ("DIV", "1cm/4", E::U(1.0 / 2.54 / 4.0, Centimetre)),
        ("DIV", "6 in/2", E::U(3.0, Inch)),
        ("DIV", "6 in/2 in", E::N(3.0)),
        ("DIV", "1 ft/1 in", E::N(12.0)),
        ("DIV", "6/2 in", E::U(3.0, Inch)),
        ("DIV", "_DIV(6, 2 in)", E::D(3.0, Inch, -1)),
        ("DIV", "6 IN^2/2 IN", E::U(3.0, Inch)),
        ("DIV", "0/0", E::Err(DivZero)),
        ("DIV", "3 in/0 cm", E::Err(DivZero)),
        ("DIV", "1/1E-12", E::N(1e12)),
        ("DIV", "3 in/50%", E::N(6.0)),
        ("DIV", "50%/2", E::U(0.25, Percent)),
        ("DIV", "50%/50%", E::N(1.0)),
    ]);
}

#[test]
fn exact_family_vectors() {
    check_table(&[
        ("EEQ", "3+5E-10 _EQ_ 3", E::B(false)),
        ("EEQ", "1 ft _EQ_ 12 in", E::B(true)),
        ("EEQ", "5 EQ 2", E::B(false)),
        ("EEQ", "0.1+0.2 _EQ_ 0.3", E::B(false)),
        ("EGE", "3 _GE_ 3+5E-10", E::B(false)),
        ("EGE", "3 _GE_ 3", E::B(true)),
        ("EGE", "5 GE 2", E::B(true)),
        ("EGT", "3+5E-10 _GT_ 3", E::B(true)),
        ("EGT", "3 _GT_ 3", E::B(false)),
        ("EGT", "5 GT 2", E::B(true)),
        ("ELE", "3+5E-10 _LE_ 3", E::B(false)),
        ("ELE", "3 _LE_ 3", E::B(true)),
        ("ELE", "5 LE 2", E::B(false)),
        ("ELT", "3 _LT_ 3+5E-10", E::B(true)),
        ("ELT", "3 _LT_ 3", E::B(false)),
        ("ELT", "5 LT 2", E::B(false)),
        ("ENE", "3+5E-10 _NE_ 3", E::B(true)),
        ("ENE", "1 ft _NE_ 12 in", E::B(false)),
        ("ENE", "5 NE 2", E::B(true)),
    ]);
}

#[test]
fn tolerance_family_vectors() {
    check_table(&[
        ("FEQ", "5 = 2", E::B(false)),
        ("FEQ", "3=3+5E-10", E::B(true)),
        ("FEQ", "3=3+2E-9", E::B(false)),
        ("FEQ", "1E-9=0", E::B(true)),
        ("FGE", "5 >= 2", E::B(true)),
        ("FGE", "5>=5", E::B(true)),
        ("FGE", "3>=3+5E-10", E::B(true)),
        ("FGE", "3>=3+2E-9", E::B(false)),
        ("FGE", "0>=1E-9", E::B(true)),
        ("FGT", "5 > 2", E::B(true)),
        ("FGT", "3+5E-10>3", E::B(false)),
        ("FGT", "3+2E-9>3", E::B(true)),
        ("FGT", "1E-9>0", E::B(false)),
        ("FLE", "5 <= 2", E::B(false)),
        ("FLE", "2<=5", E::B(true)),
        ("FLE", "3+5E-10<=3", E::B(true)),
        ("FLE", "3+2E-9<=3", E::B(false)),
        ("FLE", "1E-9<=0", E::B(true)),
        ("FLT", "5 < 2", E::B(false)),
        ("FLT", "3<3+5E-10", E::B(false)),
        ("FLT", "3<3+2E-9", E::B(true)),
        ("FLT", "0<1E-9", E::B(false)),
        ("FNE", "5 <> 2", E::B(true)),
        ("FNE", "3<>3+5E-10", E::B(false)),
        ("FNE", "3<>3+2E-9", E::B(true)),
        ("FNE", "1E-9<>0", E::B(false)),
    ]);
}

/// The symbol comparisons of two infinities follow IEEE (a ruling of ours: leaves infinity open): an infinity equals itself, is
/// neither above nor below itself, and differs from the other one. The gap `inf - inf` is NaN, and every test of it was false, `=` and `<>`
/// both, so `(1E308*10) = (1E308*10)` was FALSE.
#[test]
fn two_infinities_compare_as_ieee() {
    let (inf, ninf) = ("(1E308*10)", "(-1E308*10)");
    let mut failures = Vec::new();
    for (a, b, rows) in [
        // Equal: `=`, `>=` and `<=` hold, `<>`, `>` and `<` do not.
        (inf, inf, [true, false, true, false, true, false]),
        (ninf, ninf, [true, false, true, false, true, false]),
        // Opposite infinities.
        (inf, ninf, [false, true, true, true, false, false]),
        (ninf, inf, [false, true, false, false, true, true]),
        // An infinity and a number far from it, either side, are as ever.
        (inf, "5", [false, true, true, true, false, false]),
        ("5", inf, [false, true, false, false, true, true]),
        (ninf, "5", [false, true, false, false, true, true]),
        ("5", ninf, [false, true, true, true, false, false]),
        // The largest finite number is not infinity.
        (inf, "1.7976931348623157E308", [false, true, true, true, false, false]),
    ] {
        for (op, want) in ["=", "<>", ">=", ">", "<=", "<"].into_iter().zip(rows) {
            let formula = format!("{a} {op} {b}");
            let v = eval(&formula);
            if mismatch(&v, &E::B(want)).is_some() {
                failures.push(format!("{formula}: {v:?}, wanted {want}"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn mul_vectors() {
    check_table(&[
        ("MUL", "5 * 2", E::N(10.0)),
        ("MUL", "4*25", E::N(100.0)),
        ("MUL", "3 in*2", E::U(6.0, Inch)),
        ("MUL", "2*3 in", E::U(6.0, Inch)),
        ("MUL", "3 in*2 in", E::D(6.0, Inch, 2)),
        ("MUL", "2.54 cm*2.54 cm", E::D(1.0, Centimetre, 2)),
        ("MUL", "3 in*50%", E::U(1.5, Inch)),
        ("MUL", "50%*3 in", E::U(1.5, Percent)),
        ("MUL", "50%*2", E::N(1.0)),
        ("MUL", "2*50%", E::N(1.0)),
        ("MUL", "50%*50%", E::U(0.25, Percent)),
        ("MUL", "1 ACRE*2", E::U(12545280.0, Acre)),
        ("MUL", "1 ACRE*1 IN", E::D(6272640.0, DrawingDefault, 3)),
        ("MUL", "1 IN^100*1 IN^27", E::D(1.0, Inch, 127)),
        ("MUL", "1 IN^100*1 IN^28", E::Err(Dim)),
        ("MUL", "\"4\"*TRUE", E::N(4.0)),
        ("MUL", "\"2 ft\"*3", E::U(72.0, Foot)),
    ]);
}

#[test]
fn pct_vectors() {
    check_table(&[
        ("PCT", "37%", E::U(0.37, Percent)),
        ("PCT", "_PCT(37)", E::U(0.37, Percent)),
        ("PCT", "_PCT(\"50\")", E::U(0.5, Percent)),
        ("PCT", "_PCT(TRUE)", E::U(0.01, Percent)),
        ("PCT", "_PCT(\"goat\")", E::Err(Value)),
        ("PCT", "_PCT(7/0)", E::Err(DivZero)),
        ("PCT", "(20+30)%", E::U(0.5, Percent)),
        ("PCT", "2+50%", E::Any(2.5)),
    ]);
}

#[test]
fn sub_vectors() {
    check_table(&[
        ("SUB", "5 - 2", E::N(3.0)),
        ("SUB", "20-5-3", E::N(12.0)),
        ("SUB", "1 ft-6 in", E::U(6.0, Foot)),
        ("SUB", "6 in-1 ft", E::U(-6.0, Inch)),
        ("SUB", "10-2.54 cm", E::U(9.0, Centimetre)),
        ("SUB", "1 DL-2.54 cm", E::U(0.0, Centimetre)),
        ("SUB", "2.54 cm-1 DL", E::U(0.0, Centimetre)),
        ("SUB", "3 IN^2-1 IN", E::U(2.0, Inch)),
        ("SUB", "1 ACRE-1 IN", E::U(6272639.0, DrawingDefault)),
        ("SUB", "SQRT(POW(5 DL,2)-POW(3 DL,2))", E::U(4.0, DrawingDefault)),
    ]);
}

#[test]
fn signs_vectors() {
    check_table(&[
        ("UMINUS", "-37", E::N(-37.0)),
        ("UMINUS", "-(3 in)", E::U(-3.0, Inch)),
        ("UMINUS", "_UMINUS(2 ft)", E::U(-24.0, Foot)),
        ("UMINUS", "-4^2", E::N(16.0)),
        ("UMINUS", "0-4^2", E::N(-16.0)),
        ("UMINUS", "2^-3", E::N(0.125)),
        ("UMINUS", "-TRUE", E::N(-1.0)),
        ("UMINUS", "-\"3\"", E::N(-3.0)),
        ("UMINUS", "-\"3 in\"", E::U(-3.0, Inch)),
        ("UMINUS", "-\"goat\"", E::Err(Value)),
        ("UMINUS", "2*-3", E::N(-6.0)),
        ("UMINUS", "5--3", E::N(8.0)),
        ("UMINUS", "--5", E::N(5.0)),
        ("UPLUS", "+37", E::N(37.0)),
        ("UPLUS", "_UPLUS(2 ft)", E::U(24.0, Foot)),
        ("UPLUS", "+TRUE", E::N(1.0)),
        ("UPLUS", "+\"3 in\"", E::U(3.0, Inch)),
        ("UPLUS", "+\"3\"", E::N(3.0)),
    ]);
}

#[test]
fn an_operand_error_comes_back_whatever_the_operator() {
    // None of the 20 is an exception, and the leftmost error wins.
    for f in ["_ADD(NA(),1)", "_SUB(NA(),1)", "_MUL(NA(),1)", "_DIV(NA(),1)", "_CAT(NA(),1)", "_FEQ(NA(),1)", "_EEQ(NA(),1)", "_PCT(NA())"] {
        let v = eval(f);
        assert_eq!(mismatch(&v, &E::Err(Na)), None, "{f}");
    }
    let v = eval("(7/0)+NA()");
    assert_eq!(mismatch(&v, &E::Err(DivZero)), None, "the leftmost error wins");
}

#[test]
fn the_wrong_number_of_arguments_is_an_error_and_no_crash() {
    for f in ["_ADD(13)", "_ADD(13,)", "_ADD(1,2,3)", "_UMINUS()", "_PCT(1,2)", "_ADD()"] {
        let v = eval(f);
        assert_eq!(mismatch(&v, &E::Err(Value)), None, "{f}: {v:?}");
    }
}

#[test]
fn a_call_with_no_underscore_is_an_unknown_function() {
    // `ADD(13,29)` parses as an unknown function, and evaluates to an unsupported function.
    let v = eval("ADD(13,29)");
    assert_eq!(mismatch(&v, &E::Unsupported), None, "{v:?}");
}
