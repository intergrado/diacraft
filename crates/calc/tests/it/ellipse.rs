//! `_ELLIPSE_THETA` and `_ELLIPSE_ECC`: the axis angle and the axis ratio an elliptical arc takes when its shape is
//! resized, and the oracle's comparison of their cells by the curve the `C` and `D` of a row describe together.
//!
//! Every vector of the spec is a test, then each edge case, then the comparer's rule, the
//! real-file salt and the counted cost. A number is compared within 1E-9 as the spec says, and within 1E-8 where a vector says so.

use std::f64::consts::{FRAC_PI_2, PI};

use diacraft_calc::CellError::{Num, Value as Bad};
use diacraft_calc::{
    Budget, Calculator, CellAddress, CellKind, EvalCtx, EvalOptions, Evaluator, OperandMode, RecalcReport, SaltPolicy, Value, recalc_all_salted,
    recalc_all_with,
};
use diacraft_doc::{CellAddr, Document, Owner, RowKey, ShapeRef, SheetRef};
use diacraft_testkit::build::{DocBuilder, MasterBuilder, PageBuilder, ShapeBuilder};
use diacraft_units::Unit;
use diacraft_units::Unit::AngleDefault;

use crate::{E, check_table, check_table_on, probe_addr};

fn number(formula: &str) -> f64 {
    match crate::eval(formula) {
        Value::Number(q) => q.stored,
        other => panic!("`{formula}`: {other:?}"),
    }
}

fn near(a: f64, b: f64, eps: f64) -> bool {
    (a - b).abs() <= eps
}

// ---- _ELLIPSE_THETA's vectors ---------------------------------------------------------------------------------------------------

/// A stretch along the old axis. Naive: the old angle unchanged is -2.6415926536, turning only the old axis's direction -2.8749460267,
/// the shorter axis -1.3647833516. The stretch makes the axis across the old one the longer. A resize in proportion with a ratio under 1
/// holds the old angle plus a quarter turn, since the longer axis is then the one across.
#[test]
fn theta_the_direction_of_the_resized_ellipses_longer_axis() {
    check_table(&[
        ("ELLIPSE_THETA", "_ELLIPSE_THETA(0.5,2,4,2,8,2)", E::U(-2.9355796784, AngleDefault)),
        ("ELLIPSE_THETA", "_ELLIPSE_THETA(0.2,1.5,1,1,1,3)", E::U(-1.6702667908, AngleDefault)),
        ("ELLIPSE_THETA", "_ELLIPSE_THETA(0.3,0.5,1,1,3,3)", E::U(0.3 - FRAC_PI_2, AngleDefault)),
    ]);
    // The naive readings are not what it gives.
    for naive in [-2.6415926536, -2.8749460267, -1.3647833516] {
        assert!(!near(number("_ELLIPSE_THETA(0.5,2,4,2,8,2)"), naive, 1e-6), "{naive}");
    }
    assert!(!near(number("_ELLIPSE_THETA(0.2,1.5,1,1,1,3)"), -0.0994704640, 1e-6));
    assert!(!near(number("_ELLIPSE_THETA(0.3,0.5,1,1,3,3)"), -2.8415926536, 1e-6));
}

/// The answer is between -pi and 0. Naive: an answer in [0, pi) gives 0.7; reducing by whole turns only gives 0.7168146928.
#[test]
fn theta_the_answer_lies_from_minus_pi_to_zero() {
    check_table(&[
        ("ELLIPSE_THETA", "_ELLIPSE_THETA(0.7,3,2,5,2,5)", E::U(0.7 - PI, AngleDefault)),
        ("ELLIPSE_THETA", "_ELLIPSE_THETA(7,2,1,1,1,1)", E::U(7.0 - 3.0 * PI, AngleDefault)),
    ]);
}

/// V5 (for R3, step 2) and E1: a ratio of exactly 1 is a circle and its angle means nothing: 0, before the axes are looked at. Naive: computing the
/// longer axis gives -1.5707963268, keeping the old angle -0.4. V6 (step 1): a width or height collapsed to 0 is 0 and divides by nothing.
#[test]
fn theta_a_ratio_of_one_and_a_collapsed_size_give_zero() {
    check_table(&[
        ("ELLIPSE_THETA", "_ELLIPSE_THETA(1.1,1,2,2,2,6)", E::U(0.0, AngleDefault)),
        ("ELLIPSE_THETA", "_ELLIPSE_THETA(-0.4,1,3,3,6,6)", E::U(0.0, AngleDefault)),
        ("ELLIPSE_THETA", "_ELLIPSE_THETA(0.2,1.5,1,1,0,2)", E::U(0.0, AngleDefault)),
        ("ELLIPSE_THETA", "_ELLIPSE_THETA(0.2,1.5,1,1,2,0)", E::U(0.0, AngleDefault)),
    ]);
    // Exactly 1 only: a ratio a billionth over is an ellipse, and its longer axis is computed (the circle stretched upward has it across).
    let almost = number("_ELLIPSE_THETA(1.1,1.000000001,2,2,2,6)");
    assert!(near(almost, -FRAC_PI_2, 1e-6), "{almost}");
}

/// V7 (project rule): an exact 0 from the axis step stays 0 and is not moved to -pi. The longer axis is along x and nothing is resized.
#[test]
fn theta_an_exact_zero_stays_zero() {
    let Value::Number(q) = crate::eval("_ELLIPSE_THETA(0,2,1,1,1,1)") else { panic!("not a number") };
    assert!(q.stored == 0.0 && q.stored.is_sign_positive() && q.unit == AngleDefault, "{q:?}");
    // A circle after the resize (the ratio 2 along x, stretched twice upward): `atan2(0, 0)` is 0 and stays so.
    let Value::Number(q) = crate::eval("_ELLIPSE_THETA(0,2,1,1,1,2)") else { panic!("not a number") };
    assert!(q.stored == 0.0 && q.stored.is_sign_positive(), "{q:?}");
}

/// The arc lives in the shape's own frame, so the pose changes nothing: a shape 1.5 by 2, turned 30 degrees and flipped, gives what
/// the same shape unturned gives, and resized to 3 by 1 it gives the old angle reduced. Naive: subtracting `Angle` gives -1.8133188901.
#[test]
fn theta_the_pose_of_the_shape_is_not_read() {
    let shape = |w: &str, h: &str| {
        let (w, h) = (w.to_owned(), h.to_owned());
        move |s: ShapeBuilder| s.cell("Width", &w).cell("Height", &h).cell("Angle", &(30f64.to_radians()).to_string()).cell("FlipX", "1")
    };
    check_table_on(shape("1.5", "2"), &[("ELLIPSE_THETA", "_ELLIPSE_THETA(2.5,4,3,1,Width,Height)", E::U(-1.2897201145, AngleDefault))]);
    check_table_on(shape("3", "1"), &[("ELLIPSE_THETA", "_ELLIPSE_THETA(2.5,4,3,1,Width,Height)", E::U(2.5 - PI, AngleDefault))]);
}

/// A ratio of 0 is `#NUM!` by the project rule, and five arguments are `#VALUE!`.
#[test]
fn theta_invalid_arguments() {
    check_table(&[
        ("ELLIPSE_THETA", "_ELLIPSE_THETA(0.5,0,4,2,8,2)", E::Err(Num)),
        ("ELLIPSE_THETA", "_ELLIPSE_THETA(0.5,2,4,2,8)", E::Err(Bad)),
        ("ELLIPSE_THETA", "_ELLIPSE_THETA(0.5,2,4,2,8,2,1)", E::Err(Bad)),
        ("ELLIPSE_THETA", "_ELLIPSE_THETA()", E::Err(Bad)),
    ]);
}

// ---- _ELLIPSE_THETA's edge cases -------------------------------------------------------------------------------------------------

/// With the longer axis on the x-axis `t` is within rounding of 0 and its sign decides between 0 and about -pi: a tiny positive `t` gives about
/// -pi, a tiny negative one about 0.
#[test]
fn theta_the_longer_axis_on_x_takes_either_end_by_the_sign_of_rounding() {
    let up = number("_ELLIPSE_THETA(1e-12,2,1,1,1,1)");
    assert!(near(up, 1e-12 - PI, 1e-15), "a tiny positive angle goes to the other end: {up}");
    let down = number("_ELLIPSE_THETA(-1e-12,2,1,1,1,1)");
    assert!(near(down, -1e-12, 1e-15), "{down}");
}

/// No file holds these, and the project rule is `#NUM!` for each: an old size or a ratio of 0 or less, a negative new size, both new sizes 0, and an
/// argument that is not finite; an error argument comes back as the result.
#[test]
fn theta_the_other_invalid_arguments_are_num_and_an_error_argument_comes_back() {
    check_table(&[
        ("E4", "_ELLIPSE_THETA(0.5,-2,4,2,8,2)", E::Err(Num)),
        ("E4", "_ELLIPSE_THETA(0.5,2,0,2,8,2)", E::Err(Num)),
        ("E4", "_ELLIPSE_THETA(0.5,2,4,-2,8,2)", E::Err(Num)),
        ("E4", "_ELLIPSE_THETA(0.5,2,4,2,-8,2)", E::Err(Num)),
        ("E4", "_ELLIPSE_THETA(0.5,2,4,2,8,-2)", E::Err(Num)),
        ("E4", "_ELLIPSE_THETA(0.5,2,4,2,0,0)", E::Err(Num)),
        ("E4", "_ELLIPSE_THETA(0.5,1,4,2,0,0)", E::Err(Num)),
        ("E4", "_ELLIPSE_THETA(1/0,2,4,2,8,2)", E::Err(diacraft_calc::CellError::DivZero)),
        ("E4", "_ELLIPSE_THETA(0.5,2,4,2,8,1/0)", E::Err(diacraft_calc::CellError::DivZero)),
    ]);
}

/// E5 (our reasoning): an argument given with a unit goes through the usual conversion first: 30 degrees is a
/// sixth of a half turn, and a size in centimetres is a size in inches. The answer is an angle in radians (`U` `DA`).
#[test]
fn theta_units_are_converted_first() {
    let in_degrees = number("_ELLIPSE_THETA(30 deg,2,4,2,8,2)");
    assert!(near(in_degrees, number("_ELLIPSE_THETA(0.5235987755982988,2,4,2,8,2)"), 1e-12), "{in_degrees}");
    let in_cm = number("_ELLIPSE_THETA(0.5,2,4 cm,2 cm,8 cm,2 cm)");
    assert!(near(in_cm, number("_ELLIPSE_THETA(0.5,2,4,2,8,2)"), 1e-12), "{in_cm}");
}

/// An old height of about 1E-307 with a ratio of exactly 1 is 0, because steps 1 and 2 come before any division (dividing first would make `sy`
/// overflow); the same sizes with any other ratio are `#NUM!`, since a scale that is not finite is invalid.
#[test]
fn theta_steps_one_and_two_come_before_any_division() {
    check_table(&[
        ("ELLIPSE_THETA", "_ELLIPSE_THETA(0.3,1,1,1E-307,2,100)", E::U(0.0, AngleDefault)),
        ("ELLIPSE_THETA", "_ELLIPSE_THETA(0.3,2,1,1E-307,2,100)", E::Err(Num)),
        ("ELLIPSE_THETA", "_ELLIPSE_THETA(0.3,2,1E-307,1,100,2)", E::Err(Num)),
        // A collapsed size is step 1 and divides nothing, even where the old size is tiny.
        ("ELLIPSE_THETA", "_ELLIPSE_THETA(0.3,2,1,1E-307,0,100)", E::U(0.0, AngleDefault)),
    ]);
}

/// An instance evaluates its master's formula with its own `Width` and `Height`.
#[test]
fn theta_an_instance_reads_its_own_size() {
    let master = || ShapeBuilder::new(5).row_formula("User", None, RowKey::name("Probe"), "Value", "_ELLIPSE_THETA(2.5,4,3,1,Width,Height)", "0");
    let instance = |w: &str, h: &str| ShapeBuilder::new(1).master(1).cell("Width", w).cell("Height", h);
    let at = CellAddress::new(SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0)), probe_addr());
    for (w, h, want) in [("1.5", "2", -1.2897201145), ("3", "1", 2.5 - PI)] {
        let doc = DocBuilder::new().master(1, MasterBuilder::new().shape(master())).page(1, PageBuilder::new().shape(instance(w, h))).build();
        let mut ctx = EvalCtx::new(EvalOptions::default(), Budget::default());
        let Value::Number(q) = Evaluator::new(&doc, &mut ctx).evaluate(&at) else { panic!("not a number") };
        assert!(near(q.stored, want, 1e-9), "{w} by {h}: {q:?}");
    }
}

// ---- _ELLIPSE_ECC's vectors --------------------------------------------------------------------------------------------------------

fn ecc_is(id: &str, formula: &str, want: f64, eps: f64) {
    match crate::eval(formula) {
        Value::Number(q) if near(q.stored, want, eps) && q.unit == Unit::Unitless => {}
        other => panic!("{id}: `{formula}`: {other:?}, expected {want} within {eps}"),
    }
}

/// V1, within 1E-8. Naive: the old ratio times `sx / sy` gives 4; the old ratio unchanged gives 2. The result is a plain number.
#[test]
fn ecc_the_ratio_along_the_seventh_argument() {
    ecc_is("ELLIPSE_ECC", "_ELLIPSE_ECC(0.5,2,4,2,8,2,-2.9355796784)", 3.4423400298, 1e-8);
}

/// The seventh argument decides: along the angle `_ELLIPSE_THETA` gave, 2.1148670432; along 0, 0.4827230756. Naive: ignoring it gives the first
/// for both.
#[test]
fn ecc_the_seventh_argument_is_followed_as_given() {
    ecc_is("ELLIPSE_ECC", "_ELLIPSE_ECC(0.2,1.5,1,1,1,3,-1.6702667908)", 2.1148670432, 1e-8);
    ecc_is("ELLIPSE_ECC", "_ELLIPSE_ECC(0.2,1.5,1,1,1,3,0)", 0.4827230756, 1e-8);
}

/// A circle stretched upward is 1/3 along x and 3 along minus a quarter turn. Naive: the longer axis over the shorter gives 3 for both.
#[test]
fn ecc_a_ratio_of_one_is_still_measured_along_the_angle() {
    ecc_is("ELLIPSE_ECC", "_ELLIPSE_ECC(1.1,1,2,2,2,6,0)", 1.0 / 3.0, 1e-9);
    ecc_is("ELLIPSE_ECC", "_ELLIPSE_ECC(1.1,1,2,2,2,6,-1.5707963268)", 3.0, 1e-9);
}

/// A proportional resize of a ratio under 1 along the angle `_ELLIPSE_THETA` gave is 2. Naive: the old ratio unchanged gives 0.5. And a circle
/// resized in proportion is 1.
#[test]
fn ecc_a_proportional_resize() {
    ecc_is("ELLIPSE_ECC", "_ELLIPSE_ECC(0.3,0.5,1,1,3,3,-1.2707963268)", 2.0, 1e-9);
    ecc_is("ELLIPSE_ECC", "_ELLIPSE_ECC(-0.4,1,3,3,6,6,0)", 1.0, 1e-9);
}

/// A collapsed width with a seventh argument of 0 is 1. Naive: measuring the collapsed ellipse gives 0, or a division by zero. Any other seventh
/// argument is `#NUM!` (the project rule of R3), and so is a collapsed height with a seventh argument that is not 0.
#[test]
fn ecc_a_collapsed_size_is_one_along_zero() {
    ecc_is("ELLIPSE_ECC", "_ELLIPSE_ECC(0.2,1.5,1,1,0,2,0)", 1.0, 1e-12);
    ecc_is("ELLIPSE_ECC", "_ELLIPSE_ECC(0.2,1.5,1,1,2,0,0)", 1.0, 1e-12);
    check_table(&[
        ("ELLIPSE_ECC", "_ELLIPSE_ECC(0.2,1.5,1,1,0,2,0.1)", E::Err(Num)),
        ("ELLIPSE_ECC", "_ELLIPSE_ECC(0.2,1.5,1,1,2,0,1)", E::Err(Num)),
    ]);
}

/// An `EllipticalArcTo` row on a shape 1.5 by 2 whose `C` holds `_ELLIPSE_THETA(2.5,4,3,1,Width,Height)` and whose `D` holds
/// `_ELLIPSE_ECC(2.5,4,3,1,Width,Height,Geometry1.C2)`: `C` -1.2897201145 and `D` 6.8916470646, with `D` evaluated after `C` (the cached values here
/// are stale, and `D` is worked out from the new `C`). Naive: `D` from `C`'s old value gives another ratio; ignoring the resize gives 4.
#[test]
fn ecc_a_row_works_c_out_first_and_d_along_it() {
    let doc = arc_document(&[ArcRow {
        c: "_ELLIPSE_THETA(2.5,4,3,1,Width,Height)",
        c_saved: "9",
        d: "_ELLIPSE_ECC(2.5,4,3,1,Width,Height,Geometry1.C2)",
        d_saved: "9",
    }]);
    let d_cell =
        |name: &str| CellAddress::new(SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0)), CellAddr::in_row("Geometry", Some(0), RowKey::Ix(2), name));
    let mut ctx = EvalCtx::new(EvalOptions::default(), Budget::default());
    let (Value::Number(c), Value::Number(d)) =
        (Evaluator::new(&doc, &mut ctx).evaluate(&d_cell("C")), Evaluator::new(&doc, &mut ctx).evaluate(&d_cell("D")))
    else {
        panic!("not numbers")
    };
    assert!(near(c.stored, -1.2897201145, 1e-9) && c.unit == AngleDefault, "{c:?}");
    // The number is the function's; the unit a `D` cell shows is the cell's own default (R33 only fits the unit, never the stored number).
    assert!(near(d.stored, 6.8916470646, 1e-8), "{d:?}");
}

/// E2 (project rule): a seventh argument that is no axis of the resized ellipse is measured as written all the same, and the ratio is that of the
/// two lengths along it and across it.
#[test]
fn ecc_a_seventh_argument_that_is_no_axis_is_measured_as_written() {
    let off_axis = number("_ELLIPSE_ECC(0.5,2,4,2,8,2,0.3)");
    assert!(off_axis.is_finite() && off_axis > 0.0 && !near(off_axis, number("_ELLIPSE_ECC(0.5,2,4,2,8,2,-2.9355796784)"), 1e-3), "{off_axis}");
}

/// The project rule for invalid arguments is the same as THETA's, and the seventh argument is one of them; five or eight arguments are
/// `#VALUE!`.
#[test]
fn ecc_invalid_arguments() {
    check_table(&[
        ("E4", "_ELLIPSE_ECC(0.5,0,4,2,8,2,0)", E::Err(Num)),
        ("E4", "_ELLIPSE_ECC(0.5,2,0,2,8,2,0)", E::Err(Num)),
        ("E4", "_ELLIPSE_ECC(0.5,2,4,2,8,2,0)", E::N(number("_ELLIPSE_ECC(0.5,2,4,2,8,2,0)"))),
        ("E4", "_ELLIPSE_ECC(0.5,2,4,2,0,0,0)", E::Err(Num)),
        ("E4", "_ELLIPSE_ECC(0.5,2,4,2,8,2)", E::Err(Bad)),
        ("E4", "_ELLIPSE_ECC(0.5,2,4,2,8,2,0,0)", E::Err(Bad)),
        ("E4", "_ELLIPSE_ECC(0.5,2,4,2,8,2,1/0)", E::Err(diacraft_calc::CellError::DivZero)),
    ]);
    // The scales that are not finite: `sy` overflows.
    assert!(matches!(crate::eval("_ELLIPSE_ECC(0.3,2,1,1E-307,2,100,0)"), Value::Error(Num)));
}

// ---- the oracle compares the cells by the curve ----------------------------------------------------------------------------

/// One `EllipticalArcTo` row: the formulas of its `C` and `D` and the values the file saved for them.
struct ArcRow {
    c: &'static str,
    c_saved: &'static str,
    d: &'static str,
    d_saved: &'static str,
}

/// A shape 1.5 by 2 with a move and these rows, the first at `IX` 2: the section the cells `Geometry1.C2`, `Geometry1.C3`... are in.
fn arc_shape(rows: &[ArcRow]) -> ShapeBuilder {
    arc_shape_of("EllipticalArcTo", rows)
}

/// [`arc_shape`] with the rows of this type.
fn arc_shape_of(kind: &str, rows: &[ArcRow]) -> ShapeBuilder {
    let mut s = ShapeBuilder::new(1)
        .cell("Width", "1.5")
        .cell("Height", "2")
        .row_type("Geometry", Some(0), RowKey::Ix(1), "MoveTo")
        .row_cell("Geometry", Some(0), RowKey::Ix(1), "X", "0")
        .row_cell("Geometry", Some(0), RowKey::Ix(1), "Y", "0");
    for (i, row) in rows.iter().enumerate() {
        let key = RowKey::Ix(u32::try_from(i + 2).unwrap());
        s = s
            .row_type("Geometry", Some(0), key.clone(), kind)
            .row_cell("Geometry", Some(0), key.clone(), "X", "1")
            .row_cell("Geometry", Some(0), key.clone(), "Y", "0")
            .row_cell("Geometry", Some(0), key.clone(), "A", "0.5")
            .row_cell("Geometry", Some(0), key.clone(), "B", "0.5")
            .row_formula("Geometry", Some(0), key.clone(), "C", row.c, row.c_saved)
            .row_formula("Geometry", Some(0), key, "D", row.d, row.d_saved);
    }
    s
}

fn arc_document(rows: &[ArcRow]) -> Document {
    DocBuilder::new().page(1, PageBuilder::new().shape(arc_shape(rows))).build()
}

fn arc_document_of(kind: &str, rows: &[ArcRow]) -> Document {
    DocBuilder::new().page(1, PageBuilder::new().shape(arc_shape_of(kind, rows))).build()
}

fn cached() -> EvalOptions {
    EvalOptions { operands: OperandMode::Cached, ..EvalOptions::default() }
}

fn score(rows: &[ArcRow]) -> RecalcReport {
    recalc_all_with(&arc_document(rows), &Budget::default(), cached())
}

/// The `C` and `D` formulas of the row `IX` `row` from a resize, leaving the row's own `D` reading the row's own `C`.
fn pair(args: &str, row: u32) -> (String, String) {
    (format!("_ELLIPSE_THETA({args})"), format!("_ELLIPSE_ECC({args},Geometry1.C{row})"))
}

/// What the rule gives for `args`: the angle, and the ratio along the angle `along`.
fn rule(args: &str, along: &str) -> (f64, f64) {
    (number(&format!("_ELLIPSE_THETA({args})")), number(&format!("_ELLIPSE_ECC({args},{along})")))
}

/// Leaks a string to the `'static` a row wants: a test builds a handful.
fn keep(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

/// A row whose file saved `c` and `d`, from the resize `args`.
fn saved_row(args: &str, row: u32, c: f64, d: f64) -> ArcRow {
    let (cf, df) = pair(args, row);
    ArcRow { c: keep(cf), c_saved: keep(format!("{c}")), d: keep(df), d_saved: keep(format!("{d}")) }
}

/// A raw match agrees, as before: the pair the rule gives, saved as it is, agrees in both cells, and not through the curve.
#[test]
fn a_raw_match_still_agrees() {
    let args = "0.5,2,4,2,8,2";
    let (c, d) = rule(args, "-2.9355796784");
    let r = score(&[saved_row(args, 2, c, d)]);
    assert_eq!((r.agreed, r.differed.len(), r.agreed_by_curve), (2, 0, 0), "{r:?}");
    assert_eq!(r.by_function.get("_ELLIPSE_THETA").map(|s| s.agreed), Some(1));
    assert_eq!(r.by_function.get("_ELLIPSE_ECC").map(|s| s.agreed), Some(1));
}

/// An angle and the same angle turned half a turn describe one ellipse: the file saved `C` + pi and the `D` along it, and the cell agrees through the
/// curve; the cell for `D` agrees raw, since the ratio along an axis does not change when the axis is turned round.
#[test]
fn a_pair_that_differs_only_by_pi_agrees() {
    let args = "0.5,2,4,2,8,2";
    let (c, _) = rule(args, "0");
    for turn in [PI, -PI] {
        let (c_saved, d_saved) = (c + turn, number(&format!("_ELLIPSE_ECC({args},{})", c + turn)));
        let r = score(&[saved_row(args, 2, c_saved, d_saved)]);
        assert_eq!((r.agreed, r.differed.len(), r.agreed_by_curve), (2, 0, 1), "turn {turn}: {r:?}");
    }
}

/// An axis seen from the other end is the same axis with the ratio turned over: the file saved the longer axis' angle plus a quarter turn and the
/// reciprocal ratio along it, which is the same ellipse.
#[test]
fn the_other_axis_with_the_reciprocal_ratio_is_the_same_ellipse() {
    let args = "0.5,2,4,2,8,2";
    let (c, d) = rule(args, "-2.9355796784");
    let across = c + FRAC_PI_2;
    let d_across = number(&format!("_ELLIPSE_ECC({args},{across})"));
    assert!(near(d_across, 1.0 / d, 1e-9), "the ratio along the other axis is the reciprocal: {d_across} against {d}");
    let r = score(&[saved_row(args, 2, across, d_across)]);
    assert_eq!((r.agreed, r.differed.len(), r.agreed_by_curve), (2, 0, 1), "{r:?}");
}

/// A circle after the resize has no direction: a ratio of 1 kept in proportion saved at any angle (the file held another angle for 354 cells with the
/// very arguments of others that hold 0) is the same curve, and so is a ratio of 1 stretched whose angle is the other end of the x-axis or the longer
/// axis, which are the same ellipse read along them.
#[test]
fn a_ratio_one_pair_at_different_angles_agrees() {
    // Kept in proportion: a circle, any angle, the ratio along it is 1.
    let args = "0.5,1,3,3,6,6";
    for angle in [0.0, 1.0, -PI, -2.2] {
        let r = score(&[saved_row(args, 2, angle, 1.0)]);
        assert_eq!((r.agreed, r.differed.len()), (2, 0), "angle {angle}: {r:?}");
        if angle != 0.0 {
            assert!(r.agreed_by_curve >= 1, "angle {angle}: the angle is not the rule's, so it is the curve that agrees: {r:?}");
        }
    }
    // Stretched upward: the rule says 0 and 1/2; the file may hold the other end of the x-axis, or the longer axis (up) and its ratio, 2.
    let args = "0.5,1,3,3,3,6";
    let (c, d) = rule(args, "0");
    assert!(c == 0.0 && near(d, 0.5, 1e-12), "{c} {d}");
    for (angle, along) in [(-PI, 0.5), (-FRAC_PI_2, 2.0), (FRAC_PI_2, 2.0)] {
        let d_saved = number(&format!("_ELLIPSE_ECC({args},{angle})"));
        assert!(near(d_saved, along, 1e-9), "{angle}: {d_saved}");
        let r = score(&[saved_row(args, 2, angle, d_saved)]);
        assert_eq!((r.agreed, r.differed.len()), (2, 0), "angle {angle}: {r:?}");
    }
}

/// The rounding-decided class: the resized ellipse is nearly a circle, the angle is decided by the last digits, and the file holds another
/// one with a ratio of about 1. Each describes a circle to a millionth.
#[test]
fn a_nearly_circular_ellipse_agrees_whatever_angle_rounding_chose() {
    let args = "0,2,1,1,1,2.0000000001";
    let (c, d) = rule(args, "0");
    assert!(near(d, 1.0, 1e-9), "nearly a circle: {d}");
    for other in [0.0, -FRAC_PI_2, -1.0, -PI] {
        let (c_other, d_other) = (other, number(&format!("_ELLIPSE_ECC({args},{other})")));
        let r = score(&[saved_row(args, 2, c_other, d_other)]);
        assert_eq!((r.agreed, r.differed.len()), (2, 0), "{c} against {other}: {r:?}");
    }
}

/// A different curve differs: another angle at the same ratio, another ratio at the same angle, and the reciprocal ratio at the same angle (the family
/// of that changes the curve: wider than tall where the rule's is taller than wide). Each of the cells of the row differs where the
/// pair it belongs to differs.
#[test]
fn a_different_curve_differs() {
    let args = "0.5,2,4,2,8,2";
    let (c, d) = rule(args, "-2.9355796784");
    // Another angle, the ratio along it re-measured.
    let r = score(&[saved_row(args, 2, c + 0.3, number(&format!("_ELLIPSE_ECC({args},{})", c + 0.3)))]);
    assert_eq!((r.agreed_by_curve, r.differed.len()), (0, 1), "the angle: {r:?}");
    // Another angle, the right ratio for the rule's: the pair describes an ellipse the arguments do not.
    let r = score(&[saved_row(args, 2, c + 0.3, d)]);
    assert!(r.differed.iter().any(|x| x.cell.contains('C')) && r.agreed_by_curve == 0, "{r:?}");
    // The same angle, another ratio.
    let r = score(&[saved_row(args, 2, c, d * 1.01)]);
    assert!(r.differed.iter().any(|x| x.cell.contains('D')) && r.agreed_by_curve == 0, "{r:?}");
    // The reciprocal ratio at the same angle: the C cell agrees raw, the D cell differs (the saved pair is another curve).
    let r = score(&[saved_row(args, 2, c, 1.0 / d)]);
    assert_eq!((r.agreed, r.differed.len(), r.agreed_by_curve), (1, 1, 0), "{r:?}");
    assert!(r.differed[0].cell.contains('D'), "{r:?}");
    // A ratio of 0 or less, or no number, is no ellipse and cannot agree by the curve.
    for bad in ["0", "-2"] {
        let r = score(&[ArcRow { d_saved: bad, ..saved_row(args, 2, c + PI, d) }]);
        assert_eq!(r.agreed_by_curve, 0, "d {bad}: {r:?}");
    }
}

/// The tolerance is the comparison's: a pair within a hundred-thousandth of the curve is another curve at 1E-6, and one within a ten-millionth is
/// the same.
#[test]
fn the_curve_is_compared_within_the_tolerance_of_the_comparison() {
    let args = "0.5,2,4,2,8,2";
    let (c, _) = rule(args, "-2.9355796784");
    let near_pair = |dc: f64| {
        let c_saved = c + PI + dc;
        saved_row(args, 2, c_saved, number(&format!("_ELLIPSE_ECC({args},{c_saved})")))
    };
    // 1e-7 off the half-turn angle: the file's `C` is within the tolerance of the same curve.
    let r = score(&[near_pair(1e-7)]);
    assert_eq!((r.differed.len(), r.agreed_by_curve), (0, 1), "{r:?}");
    let r = score(&[near_pair(1e-5)]);
    assert_eq!(r.differed.len(), 1, "{r:?}");
}

/// Only the `C` and `D` of a row describe a curve: the same functions in a cell of another kind agree by raw value alone.
#[test]
fn a_cell_that_is_no_c_or_d_is_compared_raw() {
    let args = "0.5,2,4,2,8,2";
    let (c, _) = rule(args, "0");
    let shape = ShapeBuilder::new(1)
        .row_formula("User", None, RowKey::name("A"), "Value", &format!("_ELLIPSE_THETA({args})"), &format!("{}", c + PI))
        .row_formula("User", None, RowKey::name("B"), "Value", &format!("_ELLIPSE_THETA({args})"), &format!("{c}"));
    let r = recalc_all_with(&DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build(), &Budget::default(), cached());
    assert_eq!((r.agreed, r.differed.len(), r.agreed_by_curve), (1, 1, 0), "{r:?}");
}

// ---- the echo control and the real-file salt ---------------------------------------------------------------------------------------

/// A calculator that hands back the saved values: the echo the controls exist to catch.
struct Echo;

impl Calculator for Echo {
    fn calculate(&mut self, ev: &mut Evaluator<'_, '_>, at: &CellAddress) -> std::sync::Arc<diacraft_calc::CellEval> {
        let mut e = (*ev.evaluate_cell(at)).clone();
        if e.kind == CellKind::Formula {
            if let Some(saved) = e.saved.clone() {
                e.raw = saved.clone();
                e.view = saved;
            }
            e.error = e.saved_error;
            e.unsupported = None;
        }
        std::sync::Arc::new(e)
    }
}

/// Known-wrong values in the cells of arcs (the harness' control is built so): the native calculator differs on every one, and an echo agrees with
/// every one, so a control of this kind fails an echo even though the comparer now accepts the same curve in other words.
#[test]
fn the_echo_control_still_fails_an_echo() {
    let wrong = [saved_row("0.5,2,4,2,8,2", 2, 0.25, 7.0), saved_row("0.2,1.5,1,1,1,3", 3, -0.5, 0.2), saved_row("0.3,0.5,1,1,3,3", 4, 1.0, 3.0)];
    let doc = arc_document(&wrong);
    let native = recalc_all_with(&doc, &Budget::default(), cached());
    assert_eq!((native.agreed, native.differed.len(), native.agreed_by_curve), (0, 6, 0), "{native:?}");
    let echo = diacraft_calc::recalc_all_using(&doc, &Budget::default(), cached(), &mut Echo);
    assert_eq!((echo.agreed, echo.differed.len()), (6, 0), "an echo agrees with whatever the file says: {echo:?}");
}

/// The real-file salt includes the cells of these functions: with every agreed cell salted, the native calculator catches each wrong value put in `C`
/// or `D`, and an echo misses every one. The salted `C` gets a wrong angle and the `D` beside it is the file's own, so the curve of the pair is not the
/// computed one; the salted `D` gets a wrong ratio.
#[test]
fn the_real_salt_catches_every_salted_ellipse_cell_and_an_echo_misses_every_one() {
    let rows: Vec<ArcRow> = [("0.5,2,4,2,8,2", "-2.9355796784"), ("0.2,1.5,1,1,1,3", "-1.6702667908"), ("0.3,0.5,1,1,3,3", "-1.2707963268")]
        .iter()
        .enumerate()
        .map(|(i, (args, along))| {
            let (c, d) = rule(args, along);
            saved_row(args, u32::try_from(i + 2).unwrap(), c, d)
        })
        .collect();
    let doc = arc_document(&rows);
    let policy = SaltPolicy { seed: 7, one_in: 1 };
    let native = recalc_all_salted(&doc, &Budget::default(), cached(), &mut diacraft_calc::Native, Some(policy));
    assert_eq!((native.agreed, native.differed.len()), (6, 0), "the cells agree unsalted: {native:?}");
    let s = &native.real_salt;
    assert_eq!(s.salted, 6, "{s:?}");
    assert_eq!((s.caught, s.missed, s.by_allowance), (6, 0, 0), "{s:?}");
    assert!(s.passed());
    let echo = recalc_all_salted(&doc, &Budget::default(), cached(), &mut Echo, Some(policy));
    assert_eq!((echo.real_salt.salted, echo.real_salt.caught, echo.real_salt.missed), (6, 0, 6), "{:?}", echo.real_salt);
    assert!(!echo.real_salt.passed());
}

/// Where the curve does not depend on a cell, a wrong value in it is not a miss of the comparer: a circle's angle (the ratio beside it is 1) is
/// not part of the curve, and the salt counts such a cell with the allowances, as it counts `ATAN2`'s zero-length rule, and not as a miss.
#[test]
fn a_salted_angle_of_a_circle_is_an_allowance_and_not_a_miss() {
    let args = "0.5,1,3,3,6,6";
    let (c, d) = rule(args, "0");
    let doc = arc_document(&[saved_row(args, 2, c, d)]);
    let r = recalc_all_salted(&doc, &Budget::default(), cached(), &mut diacraft_calc::Native, Some(SaltPolicy { seed: 7, one_in: 1 }));
    let s = &r.real_salt;
    assert_eq!((r.agreed, r.differed.len(), s.missed), (2, 0, 0), "{r:?}");
    assert_eq!(s.salted, s.caught + s.by_allowance, "{s:?}");
    assert!(s.by_allowance >= 1 && s.passed(), "the circle's angle is no part of its curve: {s:?}");
}

// ---- cost, to a count ------------------------------------------------------------------------------------------------------------

/// A section of `n` arcs whose cells all agree only through the curve (the file saved the other end of each axis).
fn many_arcs(n: usize) -> Document {
    let args = "0.5,2,4,2,8,2";
    let (c, _) = rule(args, "0");
    let mut s = arc_shape(&[]);
    for i in 0..n {
        let key = RowKey::Ix(u32::try_from(i + 2).unwrap());
        let (cf, df) = pair(args, u32::try_from(i + 2).unwrap());
        let c_saved = c + PI;
        let d_saved = number(&format!("_ELLIPSE_ECC({args},{c_saved})"));
        s = s
            .row_type("Geometry", Some(0), key.clone(), "EllipticalArcTo")
            .row_cell("Geometry", Some(0), key.clone(), "X", "1")
            .row_cell("Geometry", Some(0), key.clone(), "Y", "0")
            .row_cell("Geometry", Some(0), key.clone(), "A", "0.5")
            .row_cell("Geometry", Some(0), key.clone(), "B", "0.5")
            .row_formula("Geometry", Some(0), key.clone(), "C", &cf, &format!("{c_saved}"))
            .row_formula("Geometry", Some(0), key, "D", &df, &format!("{d_saved}"));
    }
    DocBuilder::new().page(1, PageBuilder::new().shape(s)).build()
}

/// A section of many `EllipticalArcTo` rows costs steps in proportion to its rows: doubling the rows doubles the steps, with the curve comparison
/// in every cell, and a few dozen steps a cell is the most. Counted, never timed.
#[test]
fn a_section_of_many_arcs_costs_steps_in_proportion_to_its_rows() {
    let run = |n: usize| {
        let r = recalc_all_with(&many_arcs(n), &Budget::default(), cached());
        assert_eq!((r.agreed, r.differed.len(), r.agreed_by_curve), (2 * n as u64, 0, n as u64), "{n} rows: {r:?}");
        r.stats.steps
    };
    let (small, large) = (run(500), run(1000));
    let ratio = large as f64 / small as f64;
    assert!((1.8..2.2).contains(&ratio), "{small} steps for 500 rows and {large} for 1000 (ratio {ratio})");
    assert!(large < 80 * 1000, "{large} steps for 1000 rows (two cells each) is a few dozen a cell");
}

/// A budget smaller than the section ends the run inside the budget: the comparison's extra reads are charged like any other.
#[test]
fn a_budget_smaller_than_the_arcs_ends_the_run_inside_it() {
    let r = recalc_all_with(&many_arcs(2000), &Budget::with_steps(3000), cached());
    assert!(r.budget_exhausted && r.stats.steps <= 3000, "{:?}", r.stats);
}

// ---- a resize works the pair out again ----------------------------------------------------------------------------------------------

/// And the application's own use of the functions: the cells read `Width` and `Height`, so resizing the shape works `C` out again and `D`
/// along the new `C` (the order), and the commit writes the numbers and no unit (a `D` has none, though the catalogue types it as a length).
/// A cell nothing read is left as it was.
#[test]
fn a_resize_of_the_shape_works_c_and_then_d_out_again() {
    use diacraft_calc::{changes_between, commit_changed};
    use diacraft_doc::{CellEdit, Lookup, Value as DocValue, WriteOrigin};

    let args = "2.5,4,3,1,Width,Height";
    let (c_old, d_old) =
        (number("_ELLIPSE_THETA(2.5,4,3,1,1.5,2)"), number(&format!("_ELLIPSE_ECC(2.5,4,3,1,1.5,2,{})", number("_ELLIPSE_THETA(2.5,4,3,1,1.5,2)"))));
    let row = saved_row(args, 2, c_old, d_old);
    let before = arc_document(&[row]);
    let mut after = before.clone();
    let shape = after.find_shape(Owner::Page(1), 1).unwrap();
    for (name, value) in [("Width", "3"), ("Height", "1")] {
        after
            .write_cell(&SheetRef::Shape(shape.clone()), &CellAddr::direct(name), CellEdit::Value(DocValue::from_text(value)), WriteOrigin::Drawing)
            .unwrap();
    }
    let changes = changes_between(&before, &after, 1);
    let report = commit_changed(&mut after, &Budget::default(), EvalOptions::default(), 1, &changes);
    assert_eq!(report.written, 2, "{report:?}");
    let saved = |name: &str| match after.shape(&shape).unwrap().sheet.lookup(&CellAddr::in_row("Geometry", Some(0), RowKey::Ix(2), name)) {
        Lookup::Found(cell) => (cell.value.as_ref().unwrap().text().parse::<f64>().unwrap(), cell.unit.clone()),
        other => panic!("{other:?}"),
    };
    let c_new = 2.5 - PI;
    let d_new = number(&format!("_ELLIPSE_ECC(2.5,4,3,1,3,1,{c_new})"));
    let (c, d) = (saved("C"), saved("D"));
    assert!(near(c.0, c_new, 1e-9) && near(d.0, d_new, 1e-9), "C {c:?} D {d:?}, expected {c_new} and {d_new}");
    assert!(d.1.is_none(), "the ratio is written with no unit: {d:?}");
    // A shape that is not resized is left as it is: nothing read what changed.
    let mut moved = before.clone();
    moved
        .write_cell(&SheetRef::Shape(shape.clone()), &CellAddr::direct("PinX"), CellEdit::Value(DocValue::from_text("5")), WriteOrigin::Drawing)
        .unwrap();
    let changes = changes_between(&before, &moved, 1);
    assert_eq!(commit_changed(&mut moved, &Budget::default(), EvalOptions::default(), 1, &changes).written, 0);
}

// ----: the commit keeps a pair that names the same ellipse ------------------------------------------------------------------

/// The saved text of a cell of the arc row `row`.
fn saved_text(doc: &Document, name: &str, row: u32) -> String {
    use diacraft_doc::Lookup;
    let shape = doc.find_shape(Owner::Page(1), 1).unwrap();
    match doc.shape(&shape).unwrap().sheet.lookup(&CellAddr::in_row("Geometry", Some(0), RowKey::Ix(row), name)) {
        Lookup::Found(cell) => cell.value.as_ref().unwrap().text().to_owned(),
        other => panic!("{other:?}"),
    }
}

/// Writes `value` to a cell of the shape.
fn write(doc: &mut Document, name: &str, value: &str) {
    use diacraft_doc::{CellEdit, Value as DocValue, WriteOrigin};
    let shape = doc.find_shape(Owner::Page(1), 1).unwrap();
    doc.write_cell(&SheetRef::Shape(shape), &CellAddr::direct(name), CellEdit::Value(DocValue::from_text(value)), WriteOrigin::Drawing).unwrap();
}

/// A recalculation that reaches a pair the file holds in other words than the rule leaves it as it is when the rule's pair names the same
/// ellipse (the other end of an axis, the other axis with the reciprocal, a circle at any angle, a nearly round ellipse at whichever angle rounding
/// chose): the bytes of the cells are the file's, and nothing is written. A pair that is another ellipse is written, which is what a recalculation is for.
#[test]
fn a_commit_keeps_a_saved_pair_that_names_the_same_ellipse() {
    use diacraft_calc::{Scope, commit_in};

    // (the resize, the angle the file holds, the ratio it holds): the half turn; the circle kept in proportion at another angle; a circle stretched
    // upward held at its longer axis; the same held at the other end of the x-axis; and a nearly round ellipse at the angle rounding chose.
    let held = |args: &str, c: f64| (c, number(&format!("_ELLIPSE_ECC({args},{c})")));
    let cases = [
        ("0.5,2,4,2,8,2", held("0.5,2,4,2,8,2", number("_ELLIPSE_THETA(0.5,2,4,2,8,2)") + PI)),
        ("0.5,1,3,3,6,6", held("0.5,1,3,3,6,6", 1.0)),
        ("0.5,1,3,3,3,6", held("0.5,1,3,3,3,6", -FRAC_PI_2)),
        ("0.5,1,3,3,3,6", held("0.5,1,3,3,3,6", -PI)),
        ("0,2,1,1,1,2.0000000001", held("0,2,1,1,1,2.0000000001", -1.0)),
    ];
    let rows: Vec<ArcRow> = cases.iter().enumerate().map(|(i, (args, (c, d)))| saved_row(args, u32::try_from(i + 2).unwrap(), *c, *d)).collect();
    let before = arc_document(&rows);
    let mut doc = before.clone();
    let report = commit_in(&mut doc, &Budget::default(), EvalOptions::default(), Scope::Page(1));
    assert_eq!(report.written, 0, "{report:?}");
    assert_eq!(doc, before, "the file's bytes are as they were");
    // A pair that is another ellipse is written: the same rows saved stale.
    let stale: Vec<ArcRow> = cases.iter().enumerate().map(|(i, (args, _))| saved_row(args, u32::try_from(i + 2).unwrap(), 0.123, 4.56)).collect();
    let mut doc = arc_document(&stale);
    let report = commit_in(&mut doc, &Budget::default(), EvalOptions::default(), Scope::Page(1));
    assert!(report.written >= cases.len() as u64, "{report:?}");
}

/// A whole-page pass that answers an edit which changed no argument of an arc leaves the arc's pair as saved, the family that holds the
/// reciprocal (which the rule would turn into another curve) included. Without the word that no argument changed the pass writes
/// the rule's ratio, as any recalculation of a cell does: the control.
#[test]
fn a_whole_page_pass_that_changed_no_argument_leaves_the_reciprocal_family_as_saved() {
    use diacraft_calc::{Changes, commit_changed};

    // A circle made 3 by 3 and now 1.5 by 2: the rule says (0, 0.75), and the family holds the ratio the other way up.
    let args = "0.5,1,3,3,Width,Height";
    let d_rule = number("_ELLIPSE_ECC(0.5,1,3,3,1.5,2,0)");
    assert!(near(d_rule, 0.75, 1e-12), "{d_rule}");
    let before = arc_document(&[saved_row(args, 2, 0.0, 1.0 / d_rule)]);
    // The control: a whole-page pass that knows nothing of the arguments writes the rule's ratio.
    let mut worked = before.clone();
    let report = commit_changed(&mut worked, &Budget::default(), EvalOptions::default(), 1, &Changes { whole_page: true, ..Changes::default() });
    assert_eq!(report.written, 1, "{report:?}");
    assert!(near(saved_text(&worked, "D", 2).parse().unwrap(), 0.75, 1e-12), "{}", saved_text(&worked, "D", 2));
    // The pass that changed no argument leaves the bytes.
    let mut kept = before.clone();
    let report = commit_changed(
        &mut kept,
        &Budget::default(),
        EvalOptions::default(),
        1,
        &Changes { whole_page: true, arcs_untouched: true, ..Changes::default() },
    );
    assert_eq!((report.written, report.added), (0, 0), "{report:?}");
    assert_eq!(kept, before, "the pair is as the file saved it");
}

/// `changes_between` says the arcs are untouched when the styles (or the themes, colours or fonts) changed and the page did not, in the same
/// words for a page that is a copy and for the very page; and does not when the page was edited as well, or the masters were.
#[test]
fn an_edit_of_the_styles_alone_leaves_the_arcs_untouched_and_any_other_whole_page_edit_does_not() {
    use diacraft_calc::changes_between;
    use diacraft_testkit::build::StyleBuilder;

    let page = || PageBuilder::new().shape(arc_shape(&[saved_row("0.5,1,3,3,Width,Height", 2, 0.0, 1.0)]));
    let with_style = |weight: &str| DocBuilder::new().style(7, StyleBuilder::new().cell("LineWeight", weight)).page(1, page());
    let before = with_style("1").build();
    let styled = with_style("2").build();
    let changes = changes_between(&before, &styled, 1);
    assert!(changes.whole_page && changes.arcs_untouched && changes.cells.is_empty(), "{changes:?}");
    // The page edited as well as the style: a size written.
    let mut resized = styled.clone();
    write(&mut resized, "Width", "9");
    let changes = changes_between(&before, &resized, 1);
    assert!(changes.whole_page && !changes.arcs_untouched, "{changes:?}");
    // A master changed, which an instance may take its size from.
    let masters = |name: &str| {
        DocBuilder::new()
            .master(1, MasterBuilder::new().shape(ShapeBuilder::new(5).cell("Width", name)))
            .master(2, MasterBuilder::new().shape(ShapeBuilder::new(5)))
            .page(1, page())
            .build()
    };
    let changes = changes_between(&masters("2"), &masters("3"), 1);
    assert!(changes.whole_page && !changes.arcs_untouched, "{changes:?}");
    // A plain edit of the shape is not a whole-page pass, and says nothing of the arcs.
    let mut moved = before.clone();
    write(&mut moved, "PinX", "5");
    let changes = changes_between(&before, &moved, 1);
    assert!(!changes.whole_page && !changes.arcs_untouched, "{changes:?}");
}

/// A resize writes the rule's values even where the file held the reciprocal (the pair is then the rule's, which is the application's for
/// the new size), and writes nothing else: the angle that came out as it was keeps its text.
#[test]
fn a_resize_writes_the_rules_pair_where_the_file_held_the_reciprocal() {
    use diacraft_calc::{changes_between, commit_changed};

    let args = "0.5,1,3,3,Width,Height";
    let d_rule = number("_ELLIPSE_ECC(0.5,1,3,3,1.5,2,0)");
    let before = arc_document(&[saved_row(args, 2, 0.0, 1.0 / d_rule)]);
    let mut after = before.clone();
    write(&mut after, "Width", "3");
    write(&mut after, "Height", "3");
    let changes = changes_between(&before, &after, 1);
    let report = commit_changed(&mut after, &Budget::default(), EvalOptions::default(), 1, &changes);
    assert_eq!(report.written, 1, "the ratio, a circle again: {report:?}");
    assert!(near(saved_text(&after, "D", 2).parse().unwrap(), 1.0, 1e-12), "{}", saved_text(&after, "D", 2));
    assert_eq!(saved_text(&after, "C", 2), "0", "the angle came out as it was");
}

/// A commit that reaches a pair of another row type or a `D` that reads some other cell as its angle works it out like any cell, and
/// does not keep a pair the curve rule would not have agreed: the rule is for the elliptical arcs' own pair.
#[test]
fn the_curve_rule_is_for_elliptical_arc_rows_and_a_d_that_reads_the_rows_own_c() {
    use diacraft_calc::{Scope, commit_in};

    // The half turn, in a row of each type.
    let args = "0.5,2,4,2,8,2";
    let c = number("_ELLIPSE_THETA(0.5,2,4,2,8,2)");
    let row = saved_row(args, 2, c + PI, number(&format!("_ELLIPSE_ECC({args},{})", c + PI)));
    for (kind, curve) in [("EllipticalArcTo", true), ("RelEllipticalArcTo", true), ("LineTo", false), ("NURBSTo", false), ("ArcTo", false)] {
        let doc = arc_document_of(kind, std::slice::from_ref(&row));
        let report = recalc_all_with(&doc, &Budget::default(), cached());
        assert_eq!((report.agreed_by_curve, report.differed.len()), if curve { (1, 0) } else { (0, 1) }, "{kind}: {report:?}");
        let mut written = doc.clone();
        let commit = commit_in(&mut written, &Budget::default(), EvalOptions::default(), Scope::Page(1));
        assert_eq!(commit.written == 0, curve, "{kind}: {commit:?}");
    }
    // A `D` whose seventh argument is not the row's own `C`: the file holds the other axis (a quarter turn on, the reciprocal ratio), which the rule would
    // agree if it were asked, and a raw comparison does not. The longer axis is along x here: the angle is 0 and the ratio 4.
    let args = "0,2,4,2,8,2";
    let (c, d) = rule(args, "0");
    assert!(c == 0.0 && near(d, 4.0, 1e-12), "{c} {d}");
    // The rule is asked for the row's own pair: the control.
    let own = score(&[saved_row(args, 2, FRAC_PI_2, 0.25)]);
    assert_eq!((own.agreed, own.agreed_by_curve, own.differed.len()), (2, 1, 0), "{own:?}");
    // The `D` of the second row reads the first row's `C`.
    let (cf, _) = pair(args, 3);
    let reads_another_row = ArcRow {
        c: keep(cf),
        c_saved: keep(format!("{FRAC_PI_2}")),
        d: keep(format!("_ELLIPSE_ECC({args},Geometry1.C2)")),
        d_saved: keep("0.25".to_owned()),
    };
    let report = score(&[saved_row(args, 2, c, d), reads_another_row]);
    assert_eq!((report.differed.len(), report.agreed_by_curve), (1, 1), "only the `C` is the arc's pair; the `D` is compared raw: {report:?}");
    assert!(report.differed[0].cell.contains('D'), "{report:?}");
    // And a `D` that reads a number.
    let (cf, _) = pair(args, 2);
    let reads_a_number =
        ArcRow { c: keep(cf), c_saved: keep(format!("{FRAC_PI_2}")), d: keep(format!("_ELLIPSE_ECC({args},0)")), d_saved: keep("0.25".to_owned()) };
    let report = score(&[reads_a_number]);
    assert_eq!((report.differed.len(), report.agreed_by_curve), (1, 1), "{report:?}");
    assert!(report.differed[0].cell.contains('D'), "{report:?}");
}

/// A finite scale whose square overflows is `#NUM!`, and no finite wrong angle comes out of an infinity. The reviewer's example gave about -1.96
/// where its answer is about -1.57; the same arguments with the old height 1E-100 stay an angle.
#[test]
fn an_overflow_of_the_sums_of_squares_is_num_and_not_a_finite_wrong_angle() {
    check_table(&[
        ("", "_ELLIPSE_THETA(0.5,2,1,1E-307,10,2)", E::Err(Num)),
        ("", "_ELLIPSE_THETA(0.5,2,1,1E-100,1,2)", E::U(-FRAC_PI_2, AngleDefault)),
    ]);
    // `_ELLIPSE_ECC` measures with `hypot`, which does not overflow where the squares do: the same arguments give their (tiny, finite) ratio.
    let ratio = number("_ELLIPSE_ECC(0.5,2,1,1E-307,10,2,0)");
    assert!(ratio > 0.0 && ratio < 1e-300, "{ratio}");
}

/// The readings the vectors exist to catch are not what the functions give: the minor axis, the old angle, an answer in [0, pi), reducing
/// by whole turns only, the longer axis of a circle, moving 0 to -pi, a division by the collapsed size, the old ratio times `sx / sy`.
#[test]
fn the_naive_readings_the_vectors_exist_to_catch_are_not_given() {
    let naive: &[(&str, &[f64])] = &[
        ("_ELLIPSE_THETA(0.5,2,4,2,8,2)", &[-2.6415926536, -2.8749460267, -1.3647833516]),
        ("_ELLIPSE_THETA(0.2,1.5,1,1,1,3)", &[-0.0994704640]),
        ("_ELLIPSE_THETA(0.3,0.5,1,1,3,3)", &[-2.8415926536]),
        ("_ELLIPSE_THETA(0.7,3,2,5,2,5)", &[0.7]),
        ("_ELLIPSE_THETA(7,2,1,1,1,1)", &[0.7168146928]),
        ("_ELLIPSE_THETA(1.1,1,2,2,2,6)", &[-FRAC_PI_2]),
        ("_ELLIPSE_THETA(-0.4,1,3,3,6,6)", &[-0.4]),
        ("_ELLIPSE_THETA(0.2,1.5,1,1,0,2)", &[-FRAC_PI_2]),
        ("_ELLIPSE_THETA(0,2,1,1,1,1)", &[-PI]),
        ("_ELLIPSE_ECC(0.5,2,4,2,8,2,-2.9355796784)", &[4.0, 2.0]),
        ("_ELLIPSE_ECC(0.2,1.5,1,1,1,3,0)", &[2.1148670432]),
        ("_ELLIPSE_ECC(1.1,1,2,2,2,6,0)", &[3.0]),
        ("_ELLIPSE_ECC(1.1,1,2,2,2,6,-1.5707963268)", &[1.0 / 3.0]),
        ("_ELLIPSE_ECC(0.3,0.5,1,1,3,3,-1.2707963268)", &[0.5]),
        ("_ELLIPSE_ECC(0.2,1.5,1,1,0,2,0)", &[0.0]),
    ];
    for (formula, wrong) in naive {
        let got = number(formula);
        for w in *wrong {
            assert!(!near(got, *w, 1e-6), "`{formula}` gave {got}, which is the naive reading {w}");
        }
    }
    let shape = |s: ShapeBuilder| s.cell("Width", "1.5").cell("Height", "2").cell("Angle", &(30f64.to_radians()).to_string());
    let v8 = match crate::eval_on(shape, "_ELLIPSE_THETA(2.5,4,3,1,Width,Height)") {
        Value::Number(q) => q.stored,
        other => panic!("{other:?}"),
    };
    assert!(!near(v8, -1.8133188901, 1e-6), "subtracting `Angle` is the naive reading: {v8}");
}
