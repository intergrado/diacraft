//! A formula runs on the inheritor, the commit adds an `Inh` cell only where the result differs, a stored `V`
//! stands until an evaluation, and `BOUND` and `SETATREF` survive a write. Vectors.

use std::sync::Arc;

use diacraft_calc::{Budget, CellAddress, EvalCtx, EvalOptions, Evaluator, OperandMode, Value, commit, write_value};
use diacraft_doc::{Cell, CellAddr, Document, FormulaSlot, Lookup, Owner, RowKey, ShapeRef, SheetRef, WriteOrigin};
use diacraft_testkit::build::{DocBuilder, MasterBuilder, PageBuilder, ShapeBuilder};
use diacraft_units::Quantity;
use diacraft_units::Unit::{DrawingDefault, Inch};

use crate::{E, mismatch};

fn page_shape(i: u32) -> SheetRef {
    SheetRef::Shape(ShapeRef::top(Owner::Page(1), i))
}

fn value_of(doc: &Document, sheet: SheetRef, addr: CellAddr, options: EvalOptions) -> Value {
    let mut ctx = EvalCtx::new(options, Budget::default());
    Evaluator::new(doc, &mut ctx).evaluate(&CellAddress::new(sheet, addr))
}

fn master_doc(instances: Vec<ShapeBuilder>) -> Document {
    let mut page = PageBuilder::new();
    for i in instances {
        page = page.shape(i);
    }
    DocBuilder::new()
        .master(1, MasterBuilder::new().shape(ShapeBuilder::new(5).cell("Width", "2").formula("LocPinX", "Width*0.5", "1")))
        .page(1, page)
        .build()
}

#[test]
fn an_inherited_formula_runs_on_the_instance() {
    let doc = master_doc(vec![ShapeBuilder::new(10).master(1).cell("Width", "6")]);
    let v = value_of(&doc, page_shape(0), CellAddr::direct("LocPinX"), EvalOptions::default());
    assert_eq!(mismatch(&v, &E::U(3.0, DrawingDefault)), None, "copying the master's value would give 1: {v:?}");
}

#[test]
fn a_stored_value_stands_until_an_evaluation() {
    let doc = master_doc(vec![ShapeBuilder::new(10).master(1).cell("Width", "6").inh("LocPinX", Some("7"))]);
    let cached = EvalOptions { operands: OperandMode::Cached, ..EvalOptions::default() };
    let (doc_probe, at) = {
        let shape = ShapeBuilder::new(10).master(1).cell("Width", "6").inh("LocPinX", Some("7")).row_formula(
            "User",
            None,
            RowKey::name("Probe"),
            "Value",
            "LocPinX+0",
            "0",
        );
        let d = master_doc(vec![shape]);
        (d, CellAddress::new(page_shape(0), crate::probe_addr()))
    };
    let mut ctx = EvalCtx::new(cached, Budget::default());
    let v = Evaluator::new(&doc_probe, &mut ctx).evaluate(&at);
    assert_eq!(mismatch(&v, &E::Any(7.0)), None, "a reader of the cell sees the stored 7 before an evaluation: {v:?}");
    // And evaluating the cell itself gives the formula's own result on the instance.
    let v = value_of(&doc, page_shape(0), CellAddr::direct("LocPinX"), EvalOptions::default());
    assert_eq!(mismatch(&v, &E::Any(3.0)), None, "{v:?}");
}

#[test]
fn a_local_formula_ignores_the_master_and_an_instance_with_none_follows_it() {
    let mut doc = master_doc(vec![
        ShapeBuilder::new(10).master(1).cell("Width", "6").formula("LocPinX", "1 in", "1"),
        ShapeBuilder::new(11).master(1).cell("Width", "6"),
    ]);
    // The master's formula is changed.
    let edit = diacraft_doc::CellEdit::Formula { formula: Arc::new(diacraft_formula::parse("Width*0.4321")), value: None };
    doc.write_cell(&SheetRef::Shape(ShapeRef::top(Owner::Master(1), 0)), &CellAddr::direct("LocPinX"), edit, WriteOrigin::AutomationForce).unwrap();
    let first = value_of(&doc, page_shape(0), CellAddr::direct("LocPinX"), EvalOptions::default());
    assert_eq!(mismatch(&first, &E::U(1.0, Inch)), None, "{first:?}");
    let second = value_of(&doc, page_shape(1), CellAddr::direct("LocPinX"), EvalOptions::default());
    assert_eq!(mismatch(&second, &E::Any(6.0 * 0.4321)), None, "{second:?}");
}

#[test]
fn no_formula_stops_the_search_and_inh_does_not() {
    let doc = master_doc(vec![
        ShapeBuilder::new(10).master(1).cell("Width", "8").no_formula("LocPinX", "2"),
        ShapeBuilder::new(11).master(1).cell("Width", "8").inh("LocPinX", Some("2")),
    ]);
    let none = value_of(&doc, page_shape(0), CellAddr::direct("LocPinX"), EvalOptions::default());
    assert_eq!(mismatch(&none, &E::Any(2.0)), None, "no formula and the value 2 after Width changes: {none:?}");
    let inh = value_of(&doc, page_shape(1), CellAddr::direct("LocPinX"), EvalOptions::default());
    assert_eq!(mismatch(&inh, &E::Any(4.0)), None, "the master's formula is found and gives half of Width: {inh:?}");
}

#[test]
fn a_name_in_an_inherited_formula_follows_the_instance() {
    // A master with one top-level shape 12 (Width 4) and a member 13 with `PinX` `Sheet.12!Width*0.47`; an instance 40 of Width 20 with a
    // member 41 standing for 13; another page shape 12 of Width 9 is not what `Sheet.12` means there.
    let master = MasterBuilder::new().shape(ShapeBuilder::new(12).cell("Width", "4").group().child(ShapeBuilder::new(13).formula(
        "PinX",
        "Sheet.12!Width*0.47",
        "1.88",
    )));
    let page = PageBuilder::new()
        .shape(ShapeBuilder::new(12).cell("Width", "9"))
        .shape(ShapeBuilder::new(40).master(1).group().cell("Width", "20").child(ShapeBuilder::new(41).master_shape(13)));
    let doc = DocBuilder::new().master(1, master).page(1, page).build();
    let member = SheetRef::Shape(ShapeRef::top(Owner::Page(1), 1).child(0));
    let v = value_of(&doc, member, CellAddr::direct("PinX"), EvalOptions::default());
    assert_eq!(mismatch(&v, &E::Any(9.4)), None, "binding as written would give 1.88 or 4.23: {v:?}");
}

#[test]
fn a_guard_does_not_freeze_a_value() {
    let mut doc = DocBuilder::new()
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(1).cell("Width", "2").formula("LocPinX", "GUARD(Width*0.47)", "0.94")))
        .build();
    let edit = diacraft_doc::CellEdit::Value(diacraft_doc::Value::from_text("6"));
    doc.write_cell(&page_shape(0), &CellAddr::direct("Width"), edit, WriteOrigin::Automation).unwrap();
    let v = value_of(&doc, page_shape(0), CellAddr::direct("LocPinX"), EvalOptions::default());
    assert_eq!(mismatch(&v, &E::Any(2.82)), None, "freezing gives 0.94: {v:?}");
}

fn instance_cell(doc: &Document, i: u32, addr: &CellAddr) -> Option<Cell> {
    let shape = doc.shape(&ShapeRef::top(Owner::Page(1), i))?;
    match shape.sheet.lookup(addr) {
        Lookup::Found(c) => Some(c.clone()),
        _ => None,
    }
}

#[test]
fn the_commit_adds_an_inh_cell_only_where_the_result_differs() {
    let mut doc = master_doc(vec![ShapeBuilder::new(10).master(1).cell("Width", "6"), ShapeBuilder::new(11).master(1).cell("Width", "2")]);
    let report = commit(&mut doc, &Budget::default(), EvalOptions::default());
    assert_eq!(report.added, 1, "{report:?}");
    let addr = CellAddr::direct("LocPinX");
    let first = instance_cell(&doc, 0, &addr).expect("the instance of another size holds the cell");
    assert_eq!(first.formula, FormulaSlot::Inherit, "no formula text copied down");
    assert_eq!(first.value.as_ref().map(|v| v.text()), Some("3"));
    assert!(instance_cell(&doc, 1, &addr).is_none(), "an instance of the master's size leaves it out");
    // A second commit changes nothing.
    let before = doc.clone();
    let again = commit(&mut doc, &Budget::default(), EvalOptions::default());
    assert_eq!((again.written, again.added), (0, 0), "{again:?}");
    assert_eq!(doc, before);
}

#[test]
fn a_row_of_a_section_is_added_as_files_do_and_a_kept_cell_stays() {
    let master =
        MasterBuilder::new().shape(ShapeBuilder::new(5).cell("Width", "2").row_formula("Geometry", Some(0), RowKey::Ix(2), "X", "Width*0.5", "1"));
    let page = PageBuilder::new()
        .shape(ShapeBuilder::new(10).master(1).cell("Width", "6"))
        .shape(ShapeBuilder::new(11).master(1).cell("Width", "2").row_inh("Geometry", Some(0), RowKey::Ix(2), "X", Some("1")));
    let mut doc = DocBuilder::new().master(1, master).page(1, page).build();
    commit(&mut doc, &Budget::default(), EvalOptions::default());
    let addr = CellAddr::in_row("Geometry", Some(0), RowKey::Ix(2), "X");
    let first = instance_cell(&doc, 0, &addr).expect("a local section with the row holds X");
    assert_eq!((first.formula.clone(), first.value.as_ref().map(|v| v.text().to_owned())), (FormulaSlot::Inherit, Some("3".to_owned())));
    assert!(instance_cell(&doc, 1, &addr).is_some(), "an `Inh` cell whose value comes to equal the source's is kept");
}

#[test]
fn a_cached_result_is_written_through_write_batch_and_a_failure_keeps_the_value() {
    // `V` 5 and a formula that now fails: `E` is set and `V` stays.
    let mut doc = DocBuilder::new()
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(1).formula("PinX", "SQRT(-4)", "5").formula("PinY", "1+1", "1.50")))
        .build();
    let report = commit(&mut doc, &Budget::default(), EvalOptions::default());
    assert_eq!(report.written, 2, "{report:?}");
    let shape = doc.shape(&ShapeRef::top(Owner::Page(1), 0)).unwrap();
    let pin_x = shape.sheet.cell("PinX").unwrap();
    assert_eq!((pin_x.error.as_deref(), pin_x.value.as_ref().map(|v| v.text())), (Some("#NUM!"), Some("5")));
    assert_eq!(pin_x.formula.text(), Some("SQRT(-4)"), "the formula is left exactly");
    let pin_y = shape.sheet.cell("PinY").unwrap();
    assert_eq!(pin_y.value.as_ref().map(|v| v.text()), Some("2"));
    // An equal result keeps the text the file had: `1.50` for 1.5.
    let mut doc = DocBuilder::new().page(1, PageBuilder::new().shape(ShapeBuilder::new(1).formula("PinY", "1.5", "1.50"))).build();
    let report = commit(&mut doc, &Budget::default(), EvalOptions::default());
    assert_eq!((report.written, report.unchanged), (0, 1));
    let shape = doc.shape(&ShapeRef::top(Owner::Page(1), 0)).unwrap();
    assert_eq!(shape.sheet.cell("PinY").unwrap().value.as_ref().map(|v| v.text()), Some("1.50"));
}

#[test]
fn a_guarded_cell_takes_the_commit() {
    // `GUARD(x)` still re-evaluates when something it names changes, and the Evaluation origin is not stopped by a guard.
    let mut doc = DocBuilder::new()
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(1).cell("Width", "6").formula("LocPinX", "GUARD(Width*0.47)", "0.94")))
        .build();
    commit(&mut doc, &Budget::default(), EvalOptions::default());
    let shape = doc.shape(&ShapeRef::top(Owner::Page(1), 0)).unwrap();
    assert_eq!(shape.sheet.cell("LocPinX").unwrap().value.as_ref().map(|v| v.text()), Some("2.82"));
}

#[test]
fn bound_a_write_to_a_cell_with_bound_goes_into_its_first_argument() {
    let formula = "BOUND(2 in, 0, FALSE, 1 in, 3 in)";
    let shape =
        ShapeBuilder::new(1).set(CellAddr::direct("Width"), Cell::with_formula(formula, Some(diacraft_doc::Value::from_text("2"))).with_unit("IN"));
    let mut doc = DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build();
    let q = Quantity::from_value(5.0, Inch);
    write_value(&mut doc, &page_shape(0), &CellAddr::direct("Width"), &q, WriteOrigin::Automation).unwrap();
    let cell = doc.shape(&ShapeRef::top(Owner::Page(1), 0)).unwrap().sheet.cell("Width").unwrap().clone();
    let text = cell.formula.text().unwrap().to_owned();
    let parsed = diacraft_formula::parse(&text);
    let Ok(diacraft_formula::Expr::Call(call)) = &parsed.ast else { panic!("the formula is still a call: {text}") };
    assert!(call.is("BOUND") && call.args.len() == 5, "{text}");
    let Some(Some(diacraft_formula::Expr::Number(first))) = call.args.first() else { panic!("{text}") };
    assert_eq!(first.quantity().map(|q| (q.stored, q.unit)), Some((5.0, Inch)), "the first argument is 5 inches: {text}");
    let original = diacraft_formula::parse(formula);
    let Ok(diacraft_formula::Expr::Call(orig)) = &original.ast else { panic!() };
    assert_eq!(call.args[1..], orig.args[1..], "the type and the range are as they were: {text}");
    // The value of the cell is 3 inches after the next recalculation.
    commit(&mut doc, &Budget::default(), EvalOptions::default());
    let cell = doc.shape(&ShapeRef::top(Owner::Page(1), 0)).unwrap().sheet.cell("Width").unwrap().clone();
    assert_eq!(cell.value.as_ref().map(|v| v.text()), Some("3"));
}

#[test]
fn a_write_to_a_cell_with_setatref_goes_to_its_target() {
    // a cell other than `Width`: the rule is the same for every cell.
    let shape = ShapeBuilder::new(1).formula("PinX", "SETATREF(Prop.Width)", "1").row_cell("Property", None, RowKey::name("Width"), "Value", "1");
    let mut doc = DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build();
    let q = Quantity::from_value(6.0, Inch);
    write_value(&mut doc, &page_shape(0), &CellAddr::direct("PinX"), &q, WriteOrigin::Automation).unwrap();
    let sheet = doc.shape(&ShapeRef::top(Owner::Page(1), 0)).unwrap().sheet.clone();
    assert_eq!(sheet.cell("PinX").unwrap().formula.text(), Some("SETATREF(Prop.Width)"), "the formula stays");
    let target = match sheet.lookup(&CellAddr::in_row("Property", None, RowKey::name("Width"), "Value")) {
        Lookup::Found(c) => c.clone(),
        other => panic!("{other:?}"),
    };
    assert_eq!(target.value.as_ref().map(|v| v.text()), Some("6"));
    let v = value_of(&doc, page_shape(0), CellAddr::direct("PinX"), EvalOptions::default());
    assert_eq!(mismatch(&v, &E::Any(6.0)), None, "{v:?}");
}

#[test]
fn a_setatref_chain_is_followed_ten_deep_and_no_further() {
    // The write follows up to ten calls. A longer chain, and a cycle, end.
    let mut shape = ShapeBuilder::new(1);
    for i in 0..30u32 {
        shape = shape.row_formula("User", None, RowKey::name(&format!("S{i}")), "Value", &format!("SETATREF(User.S{})", i + 1), "0");
    }
    shape = shape.row_cell("User", None, RowKey::name("S30"), "Value", "0");
    let mut doc = DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build();
    let q = Quantity::new(7.0, diacraft_units::Unit::Unitless);
    let at = CellAddr::in_row("User", None, RowKey::name("S0"), "Value");
    write_value(&mut doc, &page_shape(0), &at, &q, WriteOrigin::Automation).unwrap();
    // Ten calls are followed, from S0 to S10, and the value is written there, to a cell that holds a call itself: not to S11 and beyond,
    // and not to S0.
    let sheet = doc.shape(&ShapeRef::top(Owner::Page(1), 0)).unwrap().sheet.clone();
    let value_of_row = |name: &str| match sheet.lookup(&CellAddr::in_row("User", None, RowKey::name(name), "Value")) {
        Lookup::Found(c) => c.value.as_ref().map(|v| v.text().to_owned()),
        other => panic!("{other:?}"),
    };
    assert_eq!(value_of_row("S10").as_deref(), Some("7"));
    assert_eq!(value_of_row("S9").as_deref(), Some("0"));
    assert_eq!(value_of_row("S11").as_deref(), Some("0"));
    let cyc = ShapeBuilder::new(1).row_formula("User", None, RowKey::name("A"), "Value", "SETATREF(User.B)", "0").row_formula(
        "User",
        None,
        RowKey::name("B"),
        "Value",
        "SETATREF(User.A)",
        "0",
    );
    let mut doc = DocBuilder::new().page(1, PageBuilder::new().shape(cyc)).build();
    write_value(&mut doc, &page_shape(0), &CellAddr::in_row("User", None, RowKey::name("A"), "Value"), &q, WriteOrigin::Automation).unwrap();
}
