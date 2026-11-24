//! The vectors of the functions that are worked (points, frames, `GRAVITY`, `INTERSECTX`, `INTERSECTY`, `RECTSECT`,
//! `NURBS`, `POLYLINE`). The nine path functions have their vectors in `path_functions.rs`.

use std::f64::consts::{FRAC_PI_2, PI};

use diacraft_calc::CellError::{DivZero, Ref, Value as Bad};
use diacraft_calc::{CellAddress, EvalCtx, Evaluator, Value};
use diacraft_doc::{CellAddr, Owner, RowKey, ShapeRef, SheetRef};
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};
use diacraft_units::Unit::{AngleDefault, Centimetre, DrawingDefault, Inch};

use crate::{E, check_table, eval, eval_on, mismatch, probe_addr};

/// The shape: pin (5, 3), local pin (1, 0.5), turned 90 degrees.
fn base_shape(s: ShapeBuilder) -> ShapeBuilder {
    s.cell("PinX", "5").cell("PinY", "3").cell("LocPinX", "1").cell("LocPinY", "0.5").cell("Angle", &FRAC_PI_2.to_string())
}

fn flipped_x_1(s: ShapeBuilder) -> ShapeBuilder {
    base_shape(s).cell("FlipX", "1")
}

fn flipped_y_1(s: ShapeBuilder) -> ShapeBuilder {
    base_shape(s).cell("FlipY", "1")
}

fn flipped_both_1(s: ShapeBuilder) -> ShapeBuilder {
    base_shape(s).cell("FlipX", "1").cell("FlipY", "1")
}

/// How a vector sets its shape up.
type Setup<'a> = &'a dyn Fn(ShapeBuilder) -> ShapeBuilder;

/// The numbers of a point a formula gives, read through `PNTX` and `PNTY`, which apply no frame.
fn point(setup: impl Fn(ShapeBuilder) -> ShapeBuilder, formula: &str) -> Result<(f64, f64), Value> {
    let part = |f: &str| match eval_on(&setup, &format!("{f}({formula})")) {
        Value::Number(q) => Ok(q.stored),
        other => Err(other),
    };
    Ok((part("PNTX")?, part("PNTY")?))
}

fn close2(got: Result<(f64, f64), Value>, want: (f64, f64)) -> Result<(), String> {
    match got {
        Ok((x, y)) if (x - want.0).abs() < 1e-9 && (y - want.1).abs() < 1e-9 => Ok(()),
        other => Err(format!("got {other:?}, expected {want:?}")),
    }
}

fn expect_points(rows: &[(&str, Setup<'_>, &str, (f64, f64))]) {
    let mut failures = Vec::new();
    for (id, setup, formula, want) in rows {
        if let Err(why) = close2(point(setup, formula), *want) {
            failures.push(format!("{id}: `{formula}`: {why}"));
        }
    }
    assert!(failures.is_empty(), "{} of {} vectors failed:\n{}", failures.len(), rows.len(), failures.join("\n"));
}

/// The pose: a group at pin (10, 10), local pin (2, 1), turned half a turn, holding a member (ID 11) at pin (1, 1), local pin
/// (0.5, 0.5), that holds the probe. `extra` are top-level shapes of the page.
fn member_doc(formula: &str, extra: Vec<ShapeBuilder>) -> (diacraft_doc::Document, CellAddress) {
    let member = ShapeBuilder::new(11).cell("PinX", "1").cell("PinY", "1").cell("LocPinX", "0.5").cell("LocPinY", "0.5").row_formula(
        "User",
        None,
        RowKey::name("Probe"),
        "Value",
        formula,
        "0",
    );
    let group = ShapeBuilder::new(10)
        .cell("PinX", "10")
        .cell("PinY", "10")
        .cell("LocPinX", "2")
        .cell("LocPinY", "1")
        .cell("Angle", &PI.to_string())
        .child(member);
    let mut page = PageBuilder::new().shape(group);
    for e in extra {
        page = page.shape(e);
    }
    let doc = DocBuilder::new().page(1, page).build();
    let at = CellAddress::new(SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0).child(0)), probe_addr());
    (doc, at)
}

fn member_point(formula: &str, extra: &dyn Fn() -> Vec<ShapeBuilder>) -> Result<(f64, f64), Value> {
    let part = |f: &str| {
        let (doc, at) = member_doc(&format!("{f}({formula})"), extra());
        let mut ctx = EvalCtx::default();
        match Evaluator::new(&doc, &mut ctx).evaluate(&at) {
            Value::Number(q) => Ok(q.stored),
            other => Err(other),
        }
    };
    Ok((part("PNTX")?, part("PNTY")?))
}

#[test]
fn errors_come_back_and_units_are_internal() {
    check_table(&[
        ("ALL", "PNT(7/0,1)", E::Err(DivZero)),
        ("ALL", "PNTX(PNT(7/0,1))", E::Err(DivZero)),
        ("ALL", "PNTX(PNT(2.54 cm,3))", E::U(1.0, Centimetre)),
        ("PNT", "PNTY(PNT(2.54 cm,3))", E::Any(3.0)),
        ("PNTX", "PNTX(PNT(7,12))", E::U(7.0, DrawingDefault)),
        ("PNTX", "PNTX(3 in)", E::U(3.0, Inch)),
        ("PNTY", "PNTY(PNT(7,12))", E::U(12.0, DrawingDefault)),
        // The wrong number of arguments.
        ("ALL", "PNT(1)", E::Err(Bad)),
        ("ALL", "PAR()", E::Err(Bad)),
        ("ALL", "GRAVITY(1,2,3,4)", E::Err(Bad)),
    ]);
}

#[test]
fn a_point_is_not_moved_by_the_pose_of_its_shape() {
    // On a turned and flipped shape, `PNT(7,12)` holds 7 and 12.
    expect_points(&[("PNT", &flipped_both_1, "PNT(7,12)", (7.0, 12.0)), ("PNT", &base_shape, "PNT(7,12)", (7.0, 12.0))]);
}

#[test]
fn par_carries_a_point_to_the_parent_by_the_pose() {
    expect_points(&[
        ("FRAME", &base_shape, "PAR(PNT(2,1))", (4.5, 4.0)),
        ("FRAME", &flipped_x_1, "PAR(PNT(2,1))", (4.5, 2.0)),
        ("FRAME", &flipped_y_1, "PAR(PNT(2,1))", (5.5, 4.0)),
        ("FRAME", &flipped_both_1, "PAR(PNT(2,1))", (5.5, 2.0)),
        ("PAR", &base_shape, "PAR(PNT(PinX,PinY))", (2.5, 7.0)),
        ("PNTY", &base_shape, "PAR(PNT(2,1))", (4.5, 4.0)),
    ]);
    // The number is a parent coordinate, 4, and not the local 1.
    let y = eval_on(base_shape, "PNTY(PAR(PNT(2,1)))");
    assert_eq!(mismatch(&y, &E::U(4.0, DrawingDefault)), None);
}

#[test]
fn loc_carries_a_point_into_the_sheets_own_frame() {
    expect_points(&[
        ("LOC", &base_shape, "LOC(PNT(2,1))", (2.0, 1.0)),
        ("LOCTOLOC", &base_shape, "LOCTOLOC(PNT(LocPinX,LocPinY),Width,ThePage!PageWidth)", (5.0, 3.0)),
        ("LOCTOLOC", &base_shape, "LOCTOLOC(PNT(5,3),ThePage!PageWidth,Width)", (1.0, 0.5)),
        ("LOCTOPAR", &base_shape, "LOCTOPAR(PNT(1,0.5),Width,ThePage!PageWidth)", (5.0, 3.0)),
    ]);
    // With `Width` 2, the point (2, 1) of the shape's frame is (4.5, 4) in the parent's.
    expect_points(&[("LOCTOPAR", &|s: ShapeBuilder| base_shape(s).cell("Width", "2"), "LOCTOPAR(PNT(2,1),Width,Width)", (4.5, 4.0))]);
}

#[test]
fn a_point_made_from_another_shapes_cells_is_in_that_shapes_frame() {
    // Shape 937's scratch cells hold (4.5, 4) in its frame, which is the page's; on the shape that is (2, 1).
    let part = |f: &str| {
        let formula = format!("{f}(LOC(PNT(Sheet.937!Scratch.X1,Sheet.937!Scratch.Y1)))");
        let shape = base_shape(ShapeBuilder::new(1)).row_formula("User", None, RowKey::name("Probe"), "Value", &formula, "0");
        let other = ShapeBuilder::new(937)
            .cell("PinX", "0")
            .cell("PinY", "0")
            .cell("LocPinX", "0")
            .cell("LocPinY", "0")
            .row_cell("Scratch", None, RowKey::Ix(0), "X", "4.5")
            .row_cell("Scratch", None, RowKey::Ix(0), "Y", "4");
        let doc = DocBuilder::new().page(1, PageBuilder::new().shape(shape).shape(other)).build();
        let at = CellAddress::new(SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0)), probe_addr());
        let mut ctx = EvalCtx::default();
        Evaluator::new(&doc, &mut ctx).evaluate(&at)
    };
    assert_eq!(mismatch(&part("PNTX"), &E::U(2.0, DrawingDefault)), None, "{:?}", part("PNTX"));
    assert_eq!(mismatch(&part("PNTY"), &E::U(1.0, DrawingDefault)), None, "{:?}", part("PNTY"));
}

#[test]
fn frames_through_a_group() {
    let member = |formula: &str| member_point(formula, &Vec::new);
    let want = |got: Result<(f64, f64), Value>, w: (f64, f64)| close2(got, w).unwrap_or_else(|e| panic!("{e}"));
    // The member's local pin carried to the page is the page's formula (11, 10).
    want(member("LOCTOLOC(PNT(LocPinX,LocPinY),Width,ThePage!PageWidth)"), (11.0, 10.0));
    // A page is its own parent, and the shape's own parent is the group.
    want(member("LOCTOPAR(PNT(LocPinX,LocPinY),Width,ThePage!PageWidth)"), (11.0, 10.0));
    want(member("LOCTOPAR(PNT(LocPinX,LocPinY),Width,Width)"), (1.0, 1.0));
    // A point from the group's cells is in the group's frame, bare or in arithmetic.
    want(member("LOC(PNT(Sheet.10!LocPinX,Sheet.10!LocPinY))"), (1.5, 0.5));
    want(member("LOC(PNT(Sheet.10!LocPinX+1,Sheet.10!LocPinY*1))"), (2.5, 0.5));
    // The member's parent is the group.
    want(member("PAR(PNT(0.5,0.5))"), (1.0, 1.0));
    // A top-level shape 12 at pin (11, 10): its local pin cells are in its frame, which is the page's, and the member's parent is
    // the group.
    let shape_12 = || vec![ShapeBuilder::new(12).cell("PinX", "11").cell("PinY", "10").cell("LocPinX", "0").cell("LocPinY", "0")];
    want(member_point("PAR(PNT(Sheet.12!LocPinX,Sheet.12!LocPinY))", &shape_12), (1.0, 1.0));
}

#[test]
fn a_missing_coordinate_is_ref_and_a_missing_shape_too() {
    check_table(&[
        ("FRAME", "LOC(PNT(REF(),2))", E::Err(Ref)),
        ("FRAME", "LOCTOLOC(PNT(1,2),REF(),Width)", E::Err(Ref)),
        ("FRAME", "LOCTOLOC(PNT(1,2),Sheet.99!Width,Width)", E::Err(Ref)),
    ]);
}

#[test]
fn a_point_that_names_two_sheets_is_not_carried() {
    // `PNT` itself gives its numbers; a carrier gives `#VALUE!`.
    let two = |setup_formula: &str| {
        let shape = ShapeBuilder::new(1).row_formula("User", None, RowKey::name("Probe"), "Value", setup_formula, "0");
        let other = ShapeBuilder::new(2).cell("LocPinX", "3");
        let doc = DocBuilder::new().page(1, PageBuilder::new().shape(shape).shape(other)).build();
        let at = CellAddress::new(SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0)), probe_addr());
        let mut ctx = EvalCtx::default();
        Evaluator::new(&doc, &mut ctx).evaluate(&at)
    };
    assert_eq!(mismatch(&two("PNTX(PNT(Sheet.2!LocPinX,LocPinY))"), &E::U(3.0, DrawingDefault)), None);
    assert_eq!(mismatch(&two("PAR(PNT(Sheet.2!LocPinX,LocPinY))"), &E::Err(Bad)), None);
}

#[test]
fn a_point_lands_in_the_part_of_the_cell_that_its_name_says() {
    // `PNT(3,4)` in each of these cells.
    let in_cell = |name: &str| {
        let shape = ShapeBuilder::new(1).formula(name, "PNT(3,4)", "0");
        let doc = DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build();
        let at = CellAddress::new(SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0)), CellAddr::direct(name));
        let mut ctx = EvalCtx::default();
        Evaluator::new(&doc, &mut ctx).evaluate(&at)
    };
    assert_eq!(mismatch(&in_cell("BeginY"), &E::U(4.0, DrawingDefault)), None);
    assert_eq!(mismatch(&in_cell("PinX"), &E::U(3.0, DrawingDefault)), None);
    // A point in a typed cell with no part named for it is `#VALUE!`.
    assert_eq!(mismatch(&in_cell("Width"), &E::Err(Bad)), None);
    // A `User` cell keeps the whole point.
    let shape = ShapeBuilder::new(1).row_formula("User", None, RowKey::name("Q"), "Value", "PNT(3,4)", "0");
    let doc = DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build();
    let at = CellAddress::new(SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0)), CellAddr::in_row("User", None, RowKey::name("Q"), "Value"));
    let mut ctx = EvalCtx::default();
    assert!(matches!(Evaluator::new(&doc, &mut ctx).evaluate(&at), Value::Point(_)));
}

/// What a formula works out to in one cell of a row of a shape: the cell's own value, as the evaluator gives it.
fn in_row_cell(section: &str, row: RowKey, cell: &str, formula: &str) -> Value {
    let shape = ShapeBuilder::new(1).row_formula(section, None, row.clone(), cell, formula, "0");
    let doc = DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build();
    let at = CellAddress::new(SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0)), CellAddr::in_row(section, None, row, cell));
    let mut ctx = EvalCtx::default();
    Evaluator::new(&doc, &mut ctx).evaluate(&at)
}

/// The numbers of a whole point, or what else the cell held.
fn whole_point(v: &Value) -> Result<(f64, f64), String> {
    match v {
        Value::Point(p) => Ok((p.x.stored, p.y.stored)),
        other => Err(format!("{other:?}")),
    }
}

#[test]
fn a_point_in_the_value_of_a_property_row_keeps_the_whole_point() {
    // A point in a free cell is kept whole, and `Prop.Value` is a free cell. The section a
    // formula spells `Prop` is the one the model names `Property`, so the cell sits in `Property`, which is not the name the formula uses.
    let held = in_row_cell("Property", RowKey::name("Q"), "Value", "PNT(3,4)");
    assert_eq!(whole_point(&held), Ok((3.0, 4.0)));
    // Read through the formula's own spelling of the section, from another cell: the point is there to be read.
    let setup = |s: ShapeBuilder| s.row_formula("Property", None, RowKey::name("Q"), "Value", "PNT(3,4)", "0");
    assert_eq!(mismatch(&eval_on(setup, "PNTX(Prop.Q)"), &E::U(3.0, DrawingDefault)), None);
    assert_eq!(mismatch(&eval_on(setup, "PNTY(Prop.Q.Value)"), &E::U(4.0, DrawingDefault)), None);
}

#[test]
fn a_point_in_a_text_cell_of_a_property_row_is_not_kept() {
    // Only the free cell keeps a point whole. A fix that matched the section's name alone would make these keep it too. They gave
    // `#VALUE!` before the fix (by the name never matching), so this holds on the old code and guards the new.
    for cell in ["Label", "Prompt", "Format", "SortKey", "Type", "Invisible"] {
        let held = in_row_cell("Property", RowKey::name("Q"), cell, "PNT(3,4)");
        assert_eq!(mismatch(&held, &E::Err(Bad)), None, "Property.{cell}");
    }
}

#[test]
fn a_section_named_as_formulas_spell_it_is_no_property_section() {
    // `Prop` is how a formula spells the section, never the name a file gives it: the model's section is `Property`
    // (`catalogue::SECTION_NAMES`), so a section called `Prop` is an extension section like any other name, and keeps no point whole.
    let held = in_row_cell("Prop", RowKey::name("Q"), "Value", "PNT(3,4)");
    assert_eq!(mismatch(&held, &E::Err(Bad)), None);
}

#[test]
fn a_point_in_scratch_keeps_its_part_in_x_and_y_and_the_whole_in_a_to_d() {
    // Given `PNT(3,4)`, `X` gives 3 in the drawing unit, `Y` gives 4, and `A` to `D` keep the whole point.
    let scratch = |cell: &str| in_row_cell("Scratch", RowKey::Ix(0), cell, "PNT(3,4)");
    assert_eq!(mismatch(&scratch("X"), &E::U(3.0, DrawingDefault)), None);
    assert_eq!(mismatch(&scratch("Y"), &E::U(4.0, DrawingDefault)), None);
    for cell in ["A", "B", "C", "D"] {
        assert_eq!(whole_point(&scratch(cell)), Ok((3.0, 4.0)), "Scratch.{cell}");
    }
    // The same through the formula's reference, from another cell.
    let setup = |s: ShapeBuilder| s.row_formula("Scratch", None, RowKey::Ix(0), "A", "PNT(3,4)", "0");
    assert_eq!(mismatch(&eval_on(setup, "PNTY(Scratch.A1)"), &E::U(4.0, DrawingDefault)), None);
}

#[test]
fn a_point_in_the_prompt_of_a_user_row_is_not_kept() {
    // `User.Prompt` is a text cell, not a free one, so a point in it is `#VALUE!`; `User.Value`
    // is free and keeps the whole point.
    let prompt = in_row_cell("User", RowKey::name("Q"), "Prompt", "PNT(3,4)");
    assert_eq!(mismatch(&prompt, &E::Err(Bad)), None);
    let value = in_row_cell("User", RowKey::name("Q"), "Value", "PNT(3,4)");
    assert_eq!(whole_point(&value), Ok((3.0, 4.0)));
}

#[test]
fn angles_are_carried_with_the_turn_and_the_flips() {
    let deg = |d: f64| d.to_radians();
    let rows: &[(&str, Setup<'_>, &str, f64)] = &[
        ("ANGLETOLOC", &base_shape, "ANGLETOLOC(30 deg,Width,Width)", deg(30.0)),
        ("ANGLETOLOC", &base_shape, "ANGLETOLOC(30 deg,ThePage!PageWidth,Width)", deg(-60.0)),
        ("ANGLETOLOC", &flipped_x_1, "ANGLETOLOC(30 deg,Width,ThePage!PageWidth)", deg(-120.0)),
        ("ANGLETOPAR", &base_shape, "ANGLETOPAR(30 deg,Width,Width)", deg(120.0)),
        ("ANGLETOPAR", &base_shape, "ANGLETOPAR(30 deg,Width,ThePage!PageWidth)", deg(120.0)),
        ("ANGLETOPAR", &base_shape, "ANGLETOPAR(180 deg,Width,Width)", deg(-90.0)),
    ];
    let mut failures = Vec::new();
    for (id, setup, formula, want) in rows {
        let v = eval_on(setup, formula);
        if let Some(why) = mismatch(&v, &E::U(*want, AngleDefault)) {
            failures.push(format!("{id}: `{formula}`: {why}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn gravity() {
    check_table(&[
        ("GRAVITY", "GRAVITY(100 deg,110 deg,290 deg)", E::U(0.0, AngleDefault)),
        ("GRAVITY", "GRAVITY(100 deg,290 deg,110 deg)", E::U(0.0, AngleDefault)),
        ("GRAVITY", "GRAVITY(200 deg,-90 deg,90 deg)", E::U(PI, AngleDefault)),
        ("GRAVITY", "GRAVITY(270 deg)", E::U(PI, AngleDefault)),
        ("GRAVITY", "GRAVITY(-90 deg)", E::U(PI, AngleDefault)),
        ("GRAVITY", "GRAVITY(4.712388983 rad)", E::U(PI, AngleDefault)),
        ("GRAVITY", "GRAVITY(90 deg)", E::U(0.0, AngleDefault)),
        ("GRAVITY", "GRAVITY(450 deg)", E::U(0.0, AngleDefault)),
        // The error of an argument comes back.
        ("GRAVITY", "GRAVITY(7/0)", E::Err(DivZero)),
    ]);
    // A bare `Angle` is radians, so 2 is 114 degrees and not 2.
    let v = eval_on(|s| s.cell("Angle", "2"), "GRAVITY(Angle)");
    assert_eq!(mismatch(&v, &E::U(PI, AngleDefault)), None, "{v:?}");
}

/// A limit a hair below 0 is the angle 0, as the turn it is next to, and not a turn: the old fold gave a limit of exactly a turn
/// (the sum `-1.7E-17 + 2 pi` rounds to `2 pi`), which put 90 degrees outside 0 to 180 degrees.
#[test]
fn gravity_takes_a_limit_a_hair_below_zero_as_zero() {
    check_table(&[
        ("GRAVITY", "GRAVITY(90 deg, 0 deg, 180 deg)", E::U(PI, AngleDefault)),
        ("GRAVITY", "GRAVITY(90 deg, -1E-15 deg, 180 deg)", E::U(PI, AngleDefault)),
        ("GRAVITY", "GRAVITY(90 deg, 180 deg, -1E-15 deg)", E::U(PI, AngleDefault)),
        ("GRAVITY", "GRAVITY(90 deg, -1E-20 rad, 180 deg)", E::U(PI, AngleDefault)),
        ("GRAVITY", "GRAVITY(-1E-20 rad, 270 deg, 90 deg)", E::U(0.0, AngleDefault)),
        // A whole turn and its multiples, a huge limit, and a negative one far from any turn, fold into 0 to a turn too.
        ("GRAVITY", "GRAVITY(90 deg, 360 deg, 180 deg)", E::U(PI, AngleDefault)),
        ("GRAVITY", "GRAVITY(90 deg, -1E300 rad, 180 deg)", E::U(PI, AngleDefault)),
        ("GRAVITY", "GRAVITY(1E300 rad, 180 deg, 350 deg)", E::U(PI, AngleDefault)),
    ]);
}

#[test]
fn intersections() {
    check_table(&[
        ("INTERSECTX", "INTERSECTX(0,0,30 deg,4,0,90 deg)", E::N(4.0)),
        ("INTERSECTX", "INTERSECTX(1 cm,0 cm,0 deg,5 cm,0 cm,90 deg)", E::U(5.0 / 2.54, Centimetre)),
        ("INTERSECTX", "INTERSECTX(0,0,30 deg,1,0,30 deg)", E::Err(DivZero)),
        ("INTERSECTY", "INTERSECTY(0,0,45 deg,4,0,135 deg)", E::N(2.0)),
        ("INTERSECTY", "INTERSECTY(0,0,30 deg,4,0,90 deg)", E::N(4.0 * (30f64).to_radians().tan())),
        ("INTERSECTY", "INTERSECTY(0,0,0 deg,0,1,180 deg)", E::Err(DivZero)),
    ]);
}

#[test]
fn rectangle_sectors() {
    check_table(&[
        ("RECTSECT", "RECTSECT(1 in,2 in,1 in,-7 in,0)", E::N(4.0)),
        ("RECTSECT", "RECTSECT(4,2,1.9,0.9,0)", E::N(1.0)),
        ("RECTSECT", "RECTSECT(4,2,-1.9,0.9,0)", E::N(3.0)),
        ("RECTSECT", "RECTSECT(4,2,0.5,0.9,0)", E::N(2.0)),
        ("RECTSECT", "RECTSECT(4,2,1.5,1,0)", E::N(2.0)),
        ("RECTSECT", "RECTSECT(4,2,2,1,0)", E::N(1.0)),
        ("RECTSECT", "RECTSECT(4,2,2,1,1)", E::N(2.0)),
        ("RECTSECT", "RECTSECT(4,2,-2,-1,0)", E::N(3.0)),
        ("RECTSECT", "RECTSECT(4,2,-2,-1,1)", E::N(4.0)),
        ("RECTSECT", "RECTSECT(4,2,0,0,1)", E::N(0.0)),
        // A tie within rounding is a tie.
        ("RECTSECT", "RECTSECT(4,2,2,1.0000000000000002,0)", E::N(1.0)),
    ]);
}

#[test]
fn constructors_gather_their_arguments() {
    let v = eval("NURBS(2,3,0,0,0.25,0.5,0,1)");
    let Value::Constructor(c) = v else { panic!("{v:?}") };
    assert_eq!(c.numbers, vec![2.0, 3.0, 0.0, 0.0, 0.25, 0.5, 0.0, 1.0]);
    let v = eval("POLYLINE(1,1,2.5,0,2.5,1.5,0,1.5)");
    let Value::Constructor(c) = v else { panic!("{v:?}") };
    assert_eq!(c.numbers, vec![1.0, 1.0, 2.5, 0.0, 2.5, 1.5, 0.0, 1.5]);
    check_table(&[
        ("NURBS", "NURBS(2,3,0,0,0.25,0.5)", E::Err(Bad)),
        ("POLYLINE", "POLYLINE(0,0,1)", E::Err(Bad)),
        ("NURBS", "NURBS(2,3,0)", E::Err(Bad)),
    ]);
}

#[test]
fn the_path_functions_are_worked_and_their_vectors_are_in_path_functions_rs() {
    // The nine functions that walk a path or measure a box are worked (`fn_path.rs`): on a shape with no `Geometry` section the path is not there
    // and the answer is `#REF!`, not an unsupported function; the vectors are in `path_functions.rs`.
    for f in [
        "POINTALONGPATH(Geometry1.Path,0.5)",
        "ANGLEALONGPATH(Geometry1.Path,0.5)",
        "NEARESTPOINTONPATH(Geometry1.Path,1,1)",
        "DISTTOPATH(Geometry1.Path,1,1)",
        "PATHLENGTH(Geometry1.Path)",
        "PATHSEGMENT(Geometry1.Path,0.5)",
        "SEGMENTCOUNT(Geometry1.Path)",
    ] {
        let v = eval(f);
        assert_eq!(v, Value::Error(Ref), "{f}: {v:?}");
    }
    for f in ["BOUNDINGBOXRECT(0)", "BOUNDINGBOXDIST(0)"] {
        let v = eval(f);
        assert!(matches!(v, Value::Number(_)), "{f}: {v:?}");
    }
}

#[test]
fn a_carry_between_two_pages_is_value() {
    // The source is on this page and the target on another.
    let here =
        ShapeBuilder::new(1).row_formula("User", None, RowKey::name("Probe"), "Value", "LOCTOLOC(PNT(1,2),Width,Pages[Two]!Sheet.5!Width)", "0");
    let there = ShapeBuilder::new(5).cell("Width", "1");
    let doc = DocBuilder::new().page(1, PageBuilder::new().shape(here)).page(2, PageBuilder::new().name("Two").shape(there)).build();
    let at = CellAddress::new(SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0)), probe_addr());
    let mut ctx = EvalCtx::default();
    let v = Evaluator::new(&doc, &mut ctx).evaluate(&at);
    assert_eq!(mismatch(&v, &E::Err(Bad)), None, "{v:?}");
}

#[test]
fn a_point_that_names_two_sheets_stays_mixed_when_read_back_from_a_cell() {
    // In Live mode: `User.Q` holds a point made from two sheets; `PAR(User.Q)` is `#VALUE!`, though the reference to `User.Q` itself
    // names one sheet.
    let shape = ShapeBuilder::new(1).row_formula("User", None, RowKey::name("Q"), "Value", "PNT(Sheet.2!LocPinX,LocPinY)", "0").row_formula(
        "User",
        None,
        RowKey::name("Probe"),
        "Value",
        "PNTX(PAR(User.Q))",
        "0",
    );
    let other = ShapeBuilder::new(2).cell("LocPinX", "3");
    let doc = DocBuilder::new().page(1, PageBuilder::new().shape(shape).shape(other)).build();
    let at = CellAddress::new(SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0)), probe_addr());
    let mut ctx = EvalCtx::default();
    let v = Evaluator::new(&doc, &mut ctx).evaluate(&at);
    assert_eq!(mismatch(&v, &E::Err(Bad)), None, "{v:?}");
}

#[test]
fn the_groups_two_shapes_share_do_not_cost_rounding() {
    // Sheets the two chains share cancel out. A group at pin (1E16, 1E16) would swallow 0.25 if the carry went up through it and
    // down again; carried between its two members it comes back exactly.
    let member = |id: u32, formula: Option<&str>| {
        let m = ShapeBuilder::new(id);
        match formula {
            Some(f) => m.row_formula("User", None, RowKey::name("Probe"), "Value", f, "0"),
            None => m,
        }
    };
    let part = |f: &str| {
        let group = ShapeBuilder::new(10)
            .cell("PinX", "1E16")
            .cell("PinY", "1E16")
            .child(member(11, Some(&format!("{f}(LOCTOLOC(PNT(0.25,0.75),Sheet.11!Width,Sheet.12!Width))"))))
            .child(member(12, None));
        let doc = DocBuilder::new().page(1, PageBuilder::new().shape(group)).build();
        let at = CellAddress::new(SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0).child(0)), probe_addr());
        let mut ctx = EvalCtx::default();
        Evaluator::new(&doc, &mut ctx).evaluate(&at)
    };
    assert_eq!(mismatch(&part("PNTX"), &E::U(0.25, DrawingDefault)), None, "{:?}", part("PNTX"));
    assert_eq!(mismatch(&part("PNTY"), &E::U(0.75, DrawingDefault)), None, "{:?}", part("PNTY"));
}
