//! Every vector of the batch, as data and small fixtures.

use diacraft_calc::CellError::{Ref, Value as ValueError};
use diacraft_doc::{Cell, CellAddr, FormulaSlot, RowKey};
use diacraft_testkit::build::ShapeBuilder;
use diacraft_units::Unit::{Centimetre, Inch};

use crate::{E, check_table, check_table_on, eval_on, eval_page, mismatch, relationships};

fn shape(id: u32) -> ShapeBuilder {
    ShapeBuilder::new(id)
}

fn with_relationships(id: u32, codes: &[(u32, &[u32])]) -> ShapeBuilder {
    shape(id).formula("Relationships", &relationships(codes), "0")
}

fn user(s: ShapeBuilder, row: &str, value: &str) -> ShapeBuilder {
    s.row_cell("User", None, RowKey::name(row), "Value", value)
}

#[test]
fn guard_and_the_wrappers_give_their_argument() {
    check_table(&[
        ("GUARD", "GUARD(3.3 cm)", E::U(3.3 / 2.54, Centimetre)),
        ("GUARD", "GUARD(REF())+1", E::Err(Ref)),
        ("ALL", "GUARD(7/0)", E::Err(diacraft_calc::CellError::DivZero)),
        ("SETATREFEVAL", "SETATREFEVAL(3+4)", E::N(7.0)),
        ("SETATREFEXPR", "SETATREFEXPR()", E::N(0.0)),
        ("SETATREFEXPR", "SETATREFEXPR(2.5 in)", E::U(2.5, Inch)),
        ("SETATREFEXPR", "MAX(1.5 in,SETATREFEXPR())", E::U(1.5, Inch)),
        ("SETATREFEXPR", "SETATREFEXPR(REF())", E::Err(Ref)),
        ("GUARD", "GUARD(2)+GUARD(3)", E::N(5.0)),
    ]);
}

#[test]
fn arg_and_evalcell() {
    check_table(&[("ARG", "ARG(\"Breadth\")", E::N(0.0)), ("ARG", "ARG(\"Breadth\",7)", E::N(7.0))]);
    let setup = |s: ShapeBuilder| s.row_formula("User", None, RowKey::name("Fn"), "Value", "ARG(\"P\",2)*ARG(\"Q\",3)", "6");
    check_table_on(
        setup,
        &[
            ("ARG", "User.Fn", E::N(6.0)),
            ("ARG", "EVALCELL(User.Fn,\"Q\",5)", E::N(10.0)),
            ("ARG", "EVALCELL(User.Fn,\"P\",4,\"Q\",5)", E::N(20.0)),
            ("EVALCELL", "EVALCELL(User.Fn,\"q\",5)", E::N(6.0)),
            ("ALL", "EVALCELL(User.Fn,\"Q\")", E::Err(ValueError)),
            ("ALL", "EVALCELL(User.Missing,\"Q\",1)", E::Err(Ref)),
        ],
    );
    // Values are read on the calling sheet and the formula on the named cell's.
    let shapes = vec![
        shape(1).cell("Width", "7"),
        shape(9321).cell("Width", "2").row_formula("User", None, RowKey::name("Fn"), "Value", "Width*ARG(\"K\",1)", "2"),
    ];
    let v = eval_page(shapes, 0, "EVALCELL(Sheet.9321!User.Fn,\"K\",Width)");
    assert_eq!(mismatch(&v, &E::Any(14.0)), None, "{v:?}");
    // The named cell's own value is neither used nor changed.
    let setup = |s: ShapeBuilder| s.row_formula("User", None, RowKey::name("Fn"), "Value", "ARG(\"K\",1)+1", "2");
    check_table_on(setup, &[("EVALCELL", "EVALCELL(User.Fn,\"K\",5)", E::N(6.0)), ("EVALCELL", "User.Fn", E::N(2.0))]);
}

#[test]
fn evalcell_that_reaches_itself_ends() {
    // A loop through `EVALCELL` stops at the depth rule with `#VALUE!`.
    let setup = |s: ShapeBuilder| s.row_formula("User", None, RowKey::name("Loop"), "Value", "EVALCELL(User.Loop,\"A\",1)", "0");
    let v = eval_on(setup, "EVALCELL(User.Loop,\"A\",1)");
    assert_eq!(mismatch(&v, &E::Err(ValueError)), None, "{v:?}");
}

#[test]
fn blob_is_a_value_error() {
    check_table(&[
        ("BLOB", "BLOB(\"QUJD\")", E::Err(ValueError)),
        ("BLOB", "BLOB(\"no base64 here!\")", E::Err(ValueError)),
        ("BLOB", "BLOB(12)", E::Err(ValueError)),
    ]);
}

#[test]
fn dependson_is_false_and_passes_errors() {
    check_table(&[
        ("DEPENDSON", "7+DEPENDSON(Width)", E::N(7.0)),
        ("DEPENDSON", "DEPENDSON(Width,REF())", E::Err(Ref)),
        ("DEPENDSON", "9+DEPENDSON(REF())", E::Err(Ref)),
        ("DEPENDSON", "DEPENDSON(PinX,PinY)", E::B(false)),
        ("DEPENDSON", "DEPENDSON(1,,2)", E::B(false)),
    ]);
    let setup = |s: ShapeBuilder| s.row_formula("Scratch", None, RowKey::Ix(2), "A", "1/0", "6");
    check_table_on(setup, &[("DEPENDSON", "DEPENDSON(Scratch.A3)", E::B(false))]);
}

#[test]
fn getref_getval_and_setf() {
    check_table_on(
        |s| s.cell("PinX", "1").cell("PinY", "2").cell("Width", "3"),
        &[("GETVAL", "GETVAL(PinX) + GETVAL(PinY) + Width", E::Any(6.0)), ("GETVAL", "GetVal(PinX)", E::Any(1.0))],
    );
    let setup = |s: ShapeBuilder| {
        let text = Cell::constant(diacraft_doc::Value::from_text("abc")).with_unit("STR");
        s.set(CellAddr::in_row("Property", None, RowKey::name("Q"), "Value"), text).row_cell("User", None, RowKey::name("Q"), "Value", "0.5")
    };
    check_table_on(
        setup,
        &[
            ("GETREF", "GetRef(Prop.Q)", E::T("abc")),
            ("GETREF", "GetRef(User.Q)*22", E::N(11.0)),
            ("ALL", "SETF(GetRef(User.Q),5)", E::N(0.0)),
            ("ALL", "User.Q", E::N(0.5)),
        ],
    );
}

#[test]
fn setatref_reads_its_target_and_ignores_its_second_argument() {
    let setup = |s: ShapeBuilder| {
        let mut cell = Cell::constant(diacraft_doc::Value::from_text(&(4.0f64 / 2.54).to_string())).with_unit("CM");
        cell.formula = FormulaSlot::Absent;
        s.set(CellAddr::in_row("Property", None, RowKey::name("Width"), "Value"), cell)
    };
    check_table_on(
        setup,
        &[
            ("SETATREF", "SETATREF(Prop.Width)", E::U(4.0 / 2.54, Centimetre)),
            ("SETATREF", "SETATREF(Prop.Width,7/0)", E::U(4.0 / 2.54, Centimetre)),
            ("SETATREF", "SETATREF(Prop.Width,,1)+5", E::N(5.0)),
            ("SETATREF", "SETATREF(REF(),,1)+5", E::N(5.0)),
        ],
    );
    let setup = |s: ShapeBuilder| s.row_cell("Property", None, RowKey::name("Width"), "Value", "3");
    check_table_on(setup, &[("SETATREF", "SETATREF(Prop.Width,,0)+5", E::Any(8.0)), ("SETATREF", "SETATREF(Prop.Width,,TRUE)+5", E::N(5.0))]);
    check_table(&[("SETATREF", "5+SETATREF(REF())", E::Err(Ref)), ("SETATREF", "ISERROR(SETATREF(LISTSHEETREF()!Width))", E::B(true))]);
    // The second argument is not evaluated on recalculation.
    let setup = |s: ShapeBuilder| s.row_cell("User", None, RowKey::name("Q"), "Value", "2");
    check_table_on(setup, &[("SETATREFEVAL", "SETATREF(User.Q,SETATREFEVAL(7/0))", E::N(2.0))]);
}

#[test]
fn all_setatref_in_a_formula_writes_nothing() {
    // `PinX` is 5 after recalculating twice, `User.Q7` is still 3, and the formula text is as loaded.
    let formula = "SETATREF(User.Q7,SETATREFEVAL(SETATREFEXPR()-Width))+Width";
    let setup = |s: ShapeBuilder| s.cell("Width", "2").row_cell("User", None, RowKey::name("Q7"), "Value", "3").formula("PinX", formula, "5");
    let (doc, _) = crate::probe(setup, "PinX");
    let at = diacraft_calc::CellAddress::new(
        diacraft_doc::SheetRef::Shape(diacraft_doc::ShapeRef::top(diacraft_doc::Owner::Page(1), 0)),
        CellAddr::direct("PinX"),
    );
    let mut ctx = diacraft_calc::EvalCtx::default();
    let mut ev = diacraft_calc::Evaluator::new(&doc, &mut ctx);
    for _ in 0..2 {
        let v = ev.evaluate(&at);
        assert_eq!(mismatch(&v, &E::Any(5.0)), None, "{v:?}");
        ev.invalidate_memo();
    }
    let shape = doc.shape(&diacraft_doc::ShapeRef::top(diacraft_doc::Owner::Page(1), 0)).unwrap();
    assert_eq!(shape.sheet.cell("PinX").unwrap().formula.text(), Some(formula));
}

#[test]
fn formula_exists_and_local_formula_exists() {
    let setup = |s: ShapeBuilder| {
        let mut gone = Cell::constant(diacraft_doc::Value::from_text("1"));
        gone.formula = FormulaSlot::NoFormula;
        s.row_formula("User", None, RowKey::name("Q"), "Value", "2*3", "6")
            .set(CellAddr::in_row("User", None, RowKey::name("N"), "Value"), gone)
            .row_cell("User", None, RowKey::name("V"), "Value", "5")
    };
    check_table_on(
        setup,
        &[
            ("FORMULAEXISTS", "FORMULAEXISTS(User.Q)", E::B(true)),
            ("FORMULAEXISTS", "FORMULAEXISTS(User.N)", E::B(false)),
            ("FORMULAEXISTS", "FORMULAEXISTS(User.V)", E::B(true)),
            ("FORMULAEXISTS", "FORMULAEXISTS(User.Nothing)", E::B(false)),
            ("FORMULAEXISTS", "FORMULAEXISTS(FALSE)", E::B(false)),
            ("LOCALFORMULAEXISTS", "LOCALFORMULAEXISTS(User.Q)", E::B(true)),
            ("LOCALFORMULAEXISTS", "LOCALFORMULAEXISTS(User.V)", E::B(true)),
            ("LOCALFORMULAEXISTS", "LOCALFORMULAEXISTS(User.N)", E::B(false)),
        ],
    );
}

#[test]
fn userui_works_out_only_the_expression_it_picks() {
    check_table(&[
        ("USERUI", "USERUI(0,2,7/0)", E::N(2.0)),
        ("USERUI", "USERUI(1,7/0,3)", E::N(3.0)),
        ("USERUI", "USERUI(NA(),2,3)", E::Err(diacraft_calc::CellError::Na)),
        ("USERUI", "USERUI(5,2,3)", E::N(3.0)),
        ("ALL", "USERUI(1,2)", E::Err(ValueError)),
    ]);
    check_table_on(|s| s.cell("Width", "4"), &[("USERUI", "USERUI(1, if(Width>6in, 6in, Width), Width*0.75)", E::Any(3.0))]);
}

#[test]
fn a_sheet_is_not_a_number() {
    // A sheet reference where a value belongs is `#VALUE!`; a shape in no list has no sheet to give, `#REF!` first.
    let member = vec![with_relationships(1, &[(5, &[2])]), with_relationships(2, &[(2, &[1])])];
    let v = eval_page(member, 0, "LISTSHEETREF()+1");
    assert_eq!(mismatch(&v, &E::Err(ValueError)), None, "{v:?}");
    let v = eval_page(vec![shape(1)], 0, "LISTSHEETREF()+1");
    assert_eq!(mismatch(&v, &E::Err(Ref)), None, "{v:?}");
}

#[test]
fn a_missing_sheet_or_row_through_a_sheet_returning_call_is_ref() {
    // A member of a list that holds no row `User.Lacking`.
    let list = || vec![with_relationships(1, &[(5, &[2])]), with_relationships(2, &[(2, &[1])])];
    let v = eval_page(list(), 0, "LISTSHEETREF()!User.Lacking");
    assert_eq!(mismatch(&v, &E::Err(Ref)), None, "{v:?}");
    let v = eval_page(list(), 0, "IFERROR(LISTSHEETREF()!User.Lacking,7)");
    assert_eq!(mismatch(&v, &E::N(7.0)), None, "{v:?}");
    // A page with no shape 9321.
    let v = eval_page(vec![shape(1)], 0, "Sheet.9321!Width");
    assert_eq!(mismatch(&v, &E::Err(Ref)), None, "{v:?}");
    let v = eval_page(vec![shape(1)], 0, "IFERROR(Sheet.9321!Width,3)");
    assert_eq!(mismatch(&v, &E::N(3.0)), None, "{v:?}");
}

#[test]
fn lists() {
    // A member whose code 5 names a list of `Height` 5.
    let page = || vec![with_relationships(1, &[(5, &[2])]), with_relationships(2, &[(2, &[1])]).cell("Height", "5")];
    let v = eval_page(page(), 0, "LISTSHEETREF()!Height");
    assert_eq!(mismatch(&v, &E::Any(5.0)), None, "{v:?}");
    // In no list, and a lost target, both give the alternative.
    let v = eval_page(vec![shape(1).cell("Height", "3")], 0, "IFERROR(LISTSHEETREF()!Height,8)");
    assert_eq!(mismatch(&v, &E::N(8.0)), None, "{v:?}");
    let v = eval_page(vec![with_relationships(1, &[(5, &[0])]).cell("Height", "3")], 0, "IFERROR(LISTSHEETREF()!Height,8)");
    assert_eq!(mismatch(&v, &E::N(8.0)), None, "a lost target is no list, and not this shape: {v:?}");
    let v = eval_page(vec![with_relationships(1, &[(2, &[2, 0, 3])]), shape(2), shape(3)], 0, "LISTMEMBERCOUNT()");
    assert_eq!(mismatch(&v, &E::N(2.0)), None, "{v:?}");
    let v = eval_page(vec![shape(1)], 0, "LISTMEMBERCOUNT()");
    assert_eq!(mismatch(&v, &E::N(0.0)), None, "{v:?}");
    // A list with code 2 naming 30, 10 and 20 in that order, 10 earliest in the page.
    let page = || {
        vec![
            with_relationships(10, &[(5, &[40])]),
            with_relationships(20, &[(5, &[40])]),
            with_relationships(30, &[(5, &[40])]),
            with_relationships(40, &[(2, &[30, 10, 20])]),
        ]
    };
    for (at, expected) in [(0usize, 2.0), (2, 1.0), (1, 3.0)] {
        let v = eval_page(page(), at, "LISTORDER()");
        assert_eq!(mismatch(&v, &E::N(expected)), None, "Shape {at}: {v:?}");
    }
    let v = eval_page(vec![shape(1)], 0, "LISTORDER()");
    assert_eq!(mismatch(&v, &E::N(-1.0)), None, "{v:?}");
}

#[test]
fn containers_and_callouts() {
    let v = eval_page(vec![shape(1)], 0, "CONTAINERMEMBERCOUNT()");
    assert_eq!(mismatch(&v, &E::N(-1.0)), None, "{v:?}");
    let v = eval_page(vec![user(with_relationships(1, &[(1, &[])]), "msvStructureType", "Container")], 0, "CONTAINERMEMBERCOUNT()");
    assert_eq!(mismatch(&v, &E::N(0.0)), None, "{v:?}");
    let v = eval_page(vec![with_relationships(1, &[(1, &[2, 0, 3])]), shape(2), shape(3)], 0, "CONTAINERMEMBERCOUNT()");
    assert_eq!(mismatch(&v, &E::N(2.0)), None, "three targets, one lost: {v:?}");
    // Nesting is walked upward and each container counts once.
    let page = vec![with_relationships(1, &[(4, &[2])]), with_relationships(2, &[(4, &[3])]), shape(3)];
    let v = eval_page(page, 0, "CONTAINERCOUNT()");
    assert_eq!(mismatch(&v, &E::N(2.0)), None, "{v:?}");
    let page = vec![with_relationships(1, &[(4, &[2, 3])]), with_relationships(2, &[(4, &[3])]), shape(3)];
    let v = eval_page(page, 0, "CONTAINERCOUNT()");
    assert_eq!(mismatch(&v, &E::N(2.0)), None, "{v:?}");
    let v = eval_page(vec![shape(1)], 0, "CONTAINERCOUNT()");
    assert_eq!(mismatch(&v, &E::N(0.0)), None, "{v:?}");
    let v = eval_page(vec![with_relationships(1, &[(3, &[2, 3])]), shape(2), shape(3)], 0, "CALLOUTCOUNT()");
    assert_eq!(mismatch(&v, &E::N(2.0)), None, "{v:?}");
    let v = eval_page(vec![with_relationships(1, &[(3, &[2, 0])]), shape(2)], 0, "CALLOUTCOUNT()");
    assert_eq!(mismatch(&v, &E::N(1.0)), None, "{v:?}");
    let v = eval_page(vec![shape(1)], 0, "CALLOUTCOUNT()");
    assert_eq!(mismatch(&v, &E::N(0.0)), None, "{v:?}");
    let page = vec![with_relationships(1, &[(6, &[2])]), shape(2).cell("Height", "2.25")];
    let v = eval_page(page, 0, "CALLOUTTARGETREF()!Height");
    assert_eq!(mismatch(&v, &E::Any(2.25)), None, "{v:?}");
    let v = eval_page(vec![shape(1).cell("Height", "9")], 0, "CALLOUTTARGETREF()!Height");
    assert_eq!(mismatch(&v, &E::Err(Ref)), None, "an empty sheet is not this one: {v:?}");
    let v = eval_page(vec![shape(1).cell("Height", "9")], 0, "IFERROR(CALLOUTTARGETREF()!Height,0.5)");
    assert_eq!(mismatch(&v, &E::N(0.5)), None, "{v:?}");
}

#[test]
fn containersheetref() {
    // Code 4 names A then B, B later in the page part: the frontmost is B.
    let page = || vec![with_relationships(1, &[(4, &[2, 3])]), shape(2).cell("Height", "2"), shape(3).cell("Height", "3")];
    let v = eval_page(page(), 0, "CONTAINERSHEETREF(1)!Height");
    assert_eq!(mismatch(&v, &E::Any(3.0)), None, "{v:?}");
    let v = eval_page(page(), 0, "CONTAINERSHEETREF(2)!Height");
    assert_eq!(mismatch(&v, &E::Any(2.0)), None, "index 2 is the next back: {v:?}");
    // The category decides.
    let page = || {
        vec![
            with_relationships(1, &[(4, &[2, 3])]),
            user(shape(2).cell("Height", "2"), "msvShapeCategories", "Pool;Lane"),
            user(shape(3).cell("Height", "3"), "msvShapeCategories", "Group"),
        ]
    };
    let v = eval_page(page(), 0, "CONTAINERSHEETREF(1,\"Lane\")!Height");
    assert_eq!(mismatch(&v, &E::Any(2.0)), None, "{v:?}");
    let v = eval_page(page(), 0, "CONTAINERSHEETREF(1,\"lane\")!Height");
    assert_eq!(mismatch(&v, &E::Err(Ref)), None, "the category respects case: {v:?}");
    // A sheet in the first slot.
    let page = || vec![with_relationships(1, &[(5, &[2])]), with_relationships(2, &[(2, &[1]), (4, &[3])]), shape(3).cell("Height", "4")];
    let v = eval_page(page(), 0, "CONTAINERSHEETREF(LISTSHEETREF(),1)!Height");
    assert_eq!(mismatch(&v, &E::Any(4.0)), None, "{v:?}");
    let v = eval_page(vec![shape(1)], 0, "CONTAINERSHEETREF(LISTSHEETREF(),1)!Height");
    assert_eq!(mismatch(&v, &E::Err(Ref)), None, "{v:?}");
    // An index past the end, and no category match.
    let page = || vec![with_relationships(1, &[(4, &[2])]), shape(2).cell("Height", "2")];
    let v = eval_page(page(), 0, "CONTAINERSHEETREF(2)!Height");
    assert_eq!(mismatch(&v, &E::Err(Ref)), None, "{v:?}");
    let v = eval_page(page(), 0, "CONTAINERSHEETREF(1,\"Lane\")!Height");
    assert_eq!(mismatch(&v, &E::Err(Ref)), None, "{v:?}");
    // A sheet qualifier in the first slot and the bare form.
    let page = || vec![with_relationships(1, &[(4, &[2])]), shape(2).cell("Height", "7"), with_relationships(9321, &[(4, &[2])]).cell("Height", "5")];
    let v = eval_page(page(), 0, "CONTAINERSHEETREF(Sheet.9321!SheetRef(),1)!Height");
    assert_eq!(mismatch(&v, &E::Any(7.0)), None, "{v:?}");
    let v = eval_page(page(), 0, "CONTAINERSHEETREF(SheetRef(),1)!Height");
    assert_eq!(mismatch(&v, &E::Any(7.0)), None, "the bare form is this sheet's: {v:?}");
}

#[test]
fn relationships_cells_evaluate_to_zero_and_a_lost_target_fails() {
    // The cell holds `SUM` of `DEPENDSON` calls and is 0 with no unit.
    let shapes = vec![with_relationships(21, &[(1, &[9321, 9322]), (4, &[])]), shape(9321), shape(9322)];
    let v = eval_page(shapes, 0, "Relationships");
    assert_eq!(mismatch(&v, &E::N(0.0)), None, "{v:?}");
    // A lost target fails the cell, `V` staying.
    // A reference reads the last good value, so the cell's own error is seen by a test of it.
    let shapes = vec![with_relationships(1, &[(2, &[]), (6, &[0])])];
    let v = eval_page(shapes, 0, "ISERROR(Relationships)");
    assert_eq!(mismatch(&v, &E::B(true)), None, "{v:?}");
    let shapes = vec![with_relationships(1, &[(2, &[]), (6, &[0])])];
    let v = eval_page(shapes, 0, "Relationships");
    assert_eq!(mismatch(&v, &E::N(0.0)), None, "`V` is left as stored: {v:?}");
}

#[test]
fn shape_text_is_a_formula_for_evaltext() {
    let v = eval_page(vec![shape(1).text("4 ft + 0.5 ft")], 0, "EVALTEXT(TheText)");
    assert_eq!(mismatch(&v, &E::U(54.0, diacraft_units::Unit::Foot)), None, "{v:?}");
    let v = eval_page(vec![shape(1)], 0, "EVALTEXT(TheText)");
    assert_eq!(mismatch(&v, &E::N(0.0)), None, "{v:?}");
    let v = eval_page(vec![shape(1).text("four and a half")], 0, "EVALTEXT(TheText)");
    assert_eq!(mismatch(&v, &E::Err(ValueError)), None, "{v:?}");
}
