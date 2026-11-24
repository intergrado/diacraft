//! The vectors of the path functions (`PATHLENGTH`, `POINTALONGPATH`, `ANGLEALONGPATH`, `NEARESTPOINTONPATH`,
//! `DISTTOPATH`, `PATHSEGMENT`, `SEGMENTCOUNT`, `BOUNDINGBOXRECT`, `BOUNDINGBOXDIST`), and what the spec's counts cannot show: curved rows,
//! `Rounding`, units, turned, flipped and grouped shapes, hostile sections, and the cost of a walk to a count.
//!
//! A path is the `Geometry` section of the shape that holds the formula, built from rows `(type, cells)` by [`section`]. A point is read through
//! `PNTX` and `PNTY`, which apply no frame.

use std::f64::consts::{FRAC_PI_2, PI};
use std::sync::Arc;

use diacraft_calc::CellError::{Ref, Value as Bad};
use diacraft_calc::{Budget, Calculator, CellAddress, CellKind, EvalCtx, EvalOptions, Evaluator, OperandMode, SaltPolicy, Value, recalc_all_salted};
use diacraft_doc::{Cell, CellAddr, Document, Owner, RowKey, ShapeRef, SheetRef, Value as DocValue};
use diacraft_geom::kurbo::{ParamCurve, PathSeg};
use diacraft_geom::{GeomRow, build_paths};
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};
use diacraft_units::Unit::{AngleDefault, DrawingDefault};

use crate::{E, check_table_on, eval_on, probe, probe_addr};

/// A row of a `Geometry` section: its type and its cells.
pub struct R(&'static str, Vec<(&'static str, f64)>);

pub fn mv(x: f64, y: f64) -> R {
    R("MoveTo", vec![("X", x), ("Y", y)])
}

pub fn ln(x: f64, y: f64) -> R {
    R("LineTo", vec![("X", x), ("Y", y)])
}

pub fn arc(x: f64, y: f64, bow: f64) -> R {
    R("ArcTo", vec![("X", x), ("Y", y), ("A", bow)])
}

pub fn earc(x: f64, y: f64, a: f64, b: f64, c: f64, d: f64) -> R {
    R("EllipticalArcTo", vec![("X", x), ("Y", y), ("A", a), ("B", b), ("C", c), ("D", d)])
}

/// The section `ix` of a shape, with these rows.
pub fn section(mut s: ShapeBuilder, ix: u32, rows: Vec<R>) -> ShapeBuilder {
    for (i, R(kind, cells)) in rows.into_iter().enumerate() {
        let key = RowKey::Ix(u32::try_from(i).unwrap());
        s = s.row_type("Geometry", Some(ix), key.clone(), kind);
        for (name, v) in cells {
            s = s.row_cell("Geometry", Some(ix), key.clone(), name, &v.to_string());
        }
    }
    s
}

fn no_show(s: ShapeBuilder, ix: u32) -> ShapeBuilder {
    s.set(CellAddr::in_section("Geometry", Some(ix), "NoShow"), Cell::constant(DocValue::from_text("1")))
}

/// The shape: pin (10, 10), local pin (1.5, 2.5), turned a quarter turn, flipped in x, and a path of a move to (0, 0), a line to (3, 0)
/// and a line to (3, 5).
pub fn path_shape(s: ShapeBuilder) -> ShapeBuilder {
    let s = s
        .cell("PinX", "10")
        .cell("PinY", "10")
        .cell("LocPinX", "1.5")
        .cell("LocPinY", "2.5")
        .cell("Angle", &FRAC_PI_2.to_string())
        .cell("FlipX", "1");
    section(s, 0, vec![mv(0.0, 0.0), ln(3.0, 0.0), ln(3.0, 5.0)])
}

/// The same path on a shape with no pose.
fn plain(s: ShapeBuilder) -> ShapeBuilder {
    section(s, 0, vec![mv(0.0, 0.0), ln(3.0, 0.0), ln(3.0, 5.0)])
}

fn near(a: f64, b: f64, eps: f64) -> bool {
    (a - b).abs() <= eps
}

/// The numbers of a point a formula gives.
fn point(setup: impl Fn(ShapeBuilder) -> ShapeBuilder, formula: &str) -> Result<(f64, f64), Value> {
    let part = |f: &str| match eval_on(&setup, &format!("{f}({formula})")) {
        Value::Number(q) => Ok(q.stored),
        other => Err(other),
    };
    Ok((part("PNTX")?, part("PNTY")?))
}

fn expect_point(id: &str, setup: impl Fn(ShapeBuilder) -> ShapeBuilder, formula: &str, want: (f64, f64), eps: f64) {
    match point(setup, formula) {
        Ok((x, y)) if near(x, want.0, eps) && near(y, want.1, eps) => {}
        other => panic!("{id}: `{formula}`: got {other:?}, expected {want:?} within {eps}"),
    }
}

fn number(setup: impl Fn(ShapeBuilder) -> ShapeBuilder, formula: &str) -> f64 {
    match eval_on(setup, formula) {
        Value::Number(q) => q.stored,
        other => panic!("`{formula}`: got {other:?}, expected a number"),
    }
}

// ---- the shared vectors ---------------------------------------------------------------------------------------------------------

/// The results are of the path's own frame, whatever the pose of the shape: (3, 2), a quarter turn and 8.
/// Naive: answering in the parent's frame gives (10.5, 8.5) and 180 degrees.
#[test]
fn travel_a_turned_flipped_shape_answers_as_if_it_were_not() {
    expect_point("TRAVEL", path_shape, "POINTALONGPATH(Geometry1.Path,0.625)", (3.0, 2.0), 1e-9);
    check_table_on(
        path_shape,
        &[
            ("TRAVEL", "ANGLEALONGPATH(Geometry1.Path,0.625)", E::U(FRAC_PI_2, AngleDefault)),
            ("TRAVEL", "PATHLENGTH(Geometry1.Path)", E::U(8.0, DrawingDefault)),
        ],
    );
    // The point is tagged with the shape that holds the path, so `PAR` carries it from there: (10.5, 8.5), the naive answer, is where it lands.
    expect_point("TRAVEL", path_shape, "PAR(POINTALONGPATH(Geometry1.Path,0.625))", (10.5, 8.5), 1e-9);
    // And a shape with no pose gives the same.
    expect_point("TRAVEL", plain, "POINTALONGPATH(Geometry1.Path,0.625)", (3.0, 2.0), 1e-9);
}

/// A move to (0, 0) and `EllipticalArcTo` with end (4, 0), point (2, 1), `C` 0, `D` 2: `PATHLENGTH` about 4.8442,
/// travel 1/4 about (0.8111, 0.8041) and 1/2 (2, 1). Naive: equal steps of the parameter gives (0.5858, 0.7071) at 1/4; a chord gives 4.
#[test]
fn travel_travel_is_a_share_of_the_length_along_a_curve() {
    let curve = |s: ShapeBuilder| section(s, 0, vec![mv(0.0, 0.0), earc(4.0, 0.0, 2.0, 1.0, 0.0, 2.0)]);
    assert!(near(number(curve, "PATHLENGTH(Geometry1.Path)"), 4.8442, 1e-3));
    expect_point("TRAVEL", curve, "POINTALONGPATH(Geometry1.Path,0.25)", (0.8111, 0.8041), 1e-3);
    expect_point("TRAVEL", curve, "POINTALONGPATH(Geometry1.Path,0.5)", (2.0, 1.0), 1e-6);
}

/// A move to (0, 0), lines to (4, 0) and (4, 3), `Rounding` 1: a corner cut from (3, 0) to (4, 1), `PATHLENGTH`
/// 6.41421, travel 0.8 at (4, 1.71716). Naive: an arc gives 6.57080 and (4, 1.68584); sharp corners give 7 and (4, 1.6).
#[test]
fn travel_rounding_cuts_a_corner_by_a_chord() {
    let rounded = |r: f64| move |s: ShapeBuilder| section(s, 0, vec![mv(0.0, 0.0), ln(4.0, 0.0), ln(4.0, 3.0)]).cell("Rounding", &r.to_string());
    let length = number(rounded(1.0), "PATHLENGTH(Geometry1.Path)");
    assert!(near(length, 3.0 + 2.0f64.sqrt() + 2.0, 1e-9), "{length}");
    expect_point("TRAVEL", rounded(1.0), "POINTALONGPATH(Geometry1.Path,0.8)", (4.0, 1.71716), 1e-5);
    // The mid-cut places are on the chord from (3, 0) to (4, 1), and a place inside the cut lies on it.
    let half = 0.5 * (3.0 + 2.0f64.sqrt() + 2.0) - 3.0;
    expect_point("TRAVEL", rounded(1.0), "POINTALONGPATH(Geometry1.Path,0.5)", (3.0 + half / 2.0f64.sqrt(), half / 2.0f64.sqrt()), 1e-9);
    // Rounding 5, each side cut by 1.5, half the shorter side: 6.12132 and (4, 1.77574).
    assert!(near(number(rounded(5.0), "PATHLENGTH(Geometry1.Path)"), 6.12132, 1e-5));
    expect_point("TRAVEL", rounded(5.0), "POINTALONGPATH(Geometry1.Path,0.8)", (4.0, 1.77574), 1e-5);
    // The nearest place to (4, 0) is the chord's middle, 0.57795.
    assert!(near(number(rounded(1.0), "NEARESTPOINTONPATH(Geometry1.Path,4,0)"), 0.57795, 1e-5));
    assert!(near(number(rounded(0.0), "NEARESTPOINTONPATH(Geometry1.Path,4,0)"), 4.0 / 7.0, 1e-9), "no Rounding: sharp, 0.57143");
}

/// On a shape in a group, turned and flipped, with `Rounding` set on the member: nothing the pose does reaches a path
/// And the rounding is read from the shape that holds the path.
#[test]
fn rounding_and_walks_are_the_same_on_a_grouped_turned_flipped_shape() {
    let member = |formula: &str| {
        let rows = vec![mv(0.0, 0.0), ln(4.0, 0.0), ln(4.0, 3.0)];
        let member = section(ShapeBuilder::new(11), 0, rows)
            .cell("Rounding", "1")
            .cell("PinX", "2")
            .cell("PinY", "3")
            .cell("Angle", "0.7")
            .cell("FlipX", "1")
            .cell("FlipY", "1")
            .row_formula("User", None, RowKey::name("Probe"), "Value", formula, "0");
        let group = ShapeBuilder::new(10).cell("PinX", "5").cell("PinY", "6").cell("Angle", "2").cell("FlipY", "1").child(member);
        let doc = DocBuilder::new().page(1, PageBuilder::new().shape(group)).build();
        let at = CellAddress::new(SheetRef::Shape(ShapeRef { owner: Owner::Page(1), path: vec![0, 0] }), probe_addr());
        let mut ctx = EvalCtx::new(EvalOptions::default(), Budget::default());
        Evaluator::new(&doc, &mut ctx).evaluate(&at)
    };
    let Value::Number(len) = member("PATHLENGTH(Geometry1.Path)") else { panic!("not a number") };
    assert!(near(len.stored, 3.0 + 2.0f64.sqrt() + 2.0, 1e-9), "{len:?}");
    let Value::Number(at) = member("PNTY(POINTALONGPATH(Geometry1.Path,0.8))") else { panic!("not a number") };
    assert!(near(at.stored, 1.71716, 1e-5));
}

/// On the path, `ANGLEALONGPATH(Geometry1.Path,0.25,0.75)` is the first row's direction, 0, and no error.
/// Naive: `#REF!` for a segment below 1; the third as the travel gives 90 degrees.
#[test]
fn travel_a_segment_below_one_is_no_segment() {
    check_table_on(
        path_shape,
        &[
            ("TRAVEL", "ANGLEALONGPATH(Geometry1.Path,0.25,0.75)", E::U(0.0, AngleDefault)),
            ("TRAVEL", "ANGLEALONGPATH(Geometry1.Path,0.25,-3)", E::U(0.0, AngleDefault)),
            ("TRAVEL", "PATHLENGTH(Geometry1.Path,0.5)", E::U(8.0, DrawingDefault)),
        ],
    );
}

/// `IF` chooses the section and only the branch taken is looked at. Naive: both branches give `#REF!`.
#[test]
fn travel_if_may_choose_the_path() {
    expect_point("TRAVEL", path_shape, "POINTALONGPATH(IF(1,Geometry1.Path,Geometry9.Path),0.625)", (3.0, 2.0), 1e-9);
    expect_point("TRAVEL", path_shape, "POINTALONGPATH(IF(0,Geometry9.Path,Geometry1.Path),0.625)", (3.0, 2.0), 1e-9);
    check_table_on(path_shape, &[("TRAVEL", "PATHLENGTH(IF(0,Geometry1.Path,Geometry9.Path))", E::Err(Ref))]);
}

/// No such section is `#REF!`; a move to (1, 1) and a line to (1, 1) has no length, and is `#REF!` from the place and
/// the fraction. Naive: (1, 1) and 0 with no error. `PATHLENGTH` is 0.
#[test]
fn travel_a_missing_path_and_a_path_with_no_length() {
    check_table_on(
        path_shape,
        &[
            ("TRAVEL", "PNTX(POINTALONGPATH(Geometry9.Path,0.625))", E::Err(Ref)),
            ("TRAVEL", "NEARESTPOINTONPATH(Geometry9.Path,4,3)", E::Err(Ref)),
            ("PATHLENGTH", "PATHLENGTH(Geometry9.Path)", E::Err(Ref)),
            ("DISTTOPATH", "DISTTOPATH(Geometry9.Path,1,1)", E::Err(Ref)),
            ("SEGMENTCOUNT", "SEGMENTCOUNT(Geometry9.Path)", E::Err(Ref)),
            ("PATHSEGMENT", "PATHSEGMENT(Geometry9.Path,0.5)", E::Err(Ref)),
            ("ANGLEALONGPATH", "ANGLEALONGPATH(Geometry9.Path,0.5)", E::Err(Ref)),
        ],
    );
    let dot = |s: ShapeBuilder| section(s, 0, vec![mv(1.0, 1.0), ln(1.0, 1.0)]);
    check_table_on(
        dot,
        &[
            ("TRAVEL", "PNTX(POINTALONGPATH(Geometry1.Path,0.5))", E::Err(Ref)),
            ("TRAVEL", "NEARESTPOINTONPATH(Geometry1.Path,4,3)", E::Err(Ref)),
            ("TRAVEL", "ANGLEALONGPATH(Geometry1.Path,0)", E::Err(Ref)),
            ("TRAVEL", "DISTTOPATH(Geometry1.Path,4,3)", E::Err(Ref)),
            ("TRAVEL", "PATHSEGMENT(Geometry1.Path,0)", E::Err(Ref)),
            ("PATHLENGTH", "PATHLENGTH(Geometry1.Path)", E::U(0.0, DrawingDefault)),
            ("SEGMENTCOUNT", "SEGMENTCOUNT(Geometry1.Path)", E::N(1.0)),
        ],
    );
}

/// A section with `NoShow` 1 reads like any other. Naive: a hidden section as missing is `#REF!`.
#[test]
fn travel_a_hidden_section_is_a_path() {
    let hidden = |s: ShapeBuilder| no_show(path_shape(s), 0);
    expect_point("TRAVEL", hidden, "POINTALONGPATH(Geometry1.Path,0.625)", (3.0, 2.0), 1e-9);
    check_table_on(hidden, &[("TRAVEL", "PATHLENGTH(Geometry1.Path)", E::U(8.0, DrawingDefault))]);
}

// ---- one function at a time -----------------------------------------------------------------------------------------------------

/// `ANGLEALONGPATH(Geometry1.Path,0.625)` is pi / 2; with segment 1 it is 0, along the first row alone.
/// A move to (0, 0) and a line to (-3, -1), travel 0.25: about -2.81984. Naive: 0 to 2 pi gives 3.46334.
#[test]
fn anglealongpath_vectors() {
    check_table_on(
        path_shape,
        &[
            ("ANGLEALONGPATH", "ANGLEALONGPATH(Geometry1.Path,0.625)", E::U(FRAC_PI_2, AngleDefault)),
            ("ANGLEALONGPATH", "ANGLEALONGPATH(Geometry1.Path,0.625,1)", E::U(0.0, AngleDefault)),
            ("ANGLEALONGPATH", "ANGLEALONGPATH(Geometry1.Path,0.625,2)", E::U(FRAC_PI_2, AngleDefault)),
            ("ANGLEALONGPATH", "ANGLEALONGPATH(Geometry1.Path,0.625,3)", E::Err(Ref)),
        ],
    );
    let down_left = |s: ShapeBuilder| section(s, 0, vec![mv(0.0, 0.0), ln(-3.0, -1.0)]);
    let a = number(down_left, "ANGLEALONGPATH(Geometry1.Path,0.25)");
    assert!(near(a, (-1.0f64).atan2(-3.0), 1e-12) && near(a, -2.81984, 1e-5), "{a}");
    // The direction along the negative x-axis is minus pi or pi, as ATAN2 gives it.
    let back = |s: ShapeBuilder| section(s, 0, vec![mv(0.0, 0.0), ln(-3.0, 0.0)]);
    assert!(near(number(back, "ANGLEALONGPATH(Geometry1.Path,0.5)").abs(), PI, 1e-12));
}

/// For, a project rule: a 4 by 2 rectangle, pin (10, 10), local pin (2, 1): left 8, right 12, top
/// 11, bottom 9; turned a quarter turn: 9, 11, 12, 8. Naive: the local frame gives 0, 4, 2, 0.
#[test]
fn boundingboxrect_vectors() {
    let rect = |s: ShapeBuilder| {
        section(s, 0, vec![mv(0.0, 0.0), ln(4.0, 0.0), ln(4.0, 2.0), ln(0.0, 2.0), ln(0.0, 0.0)])
            .cell("PinX", "10")
            .cell("PinY", "10")
            .cell("LocPinX", "2")
            .cell("LocPinY", "1")
    };
    let rows = |want: [f64; 4]| -> Vec<(&'static str, &'static str, E)> {
        ["BOUNDINGBOXRECT(0)", "BOUNDINGBOXRECT(1)", "BOUNDINGBOXRECT(2)", "BOUNDINGBOXRECT(3)"]
            .into_iter()
            .zip(want)
            .map(|(f, w)| ("BOUNDINGBOXRECT", f, E::U(w, DrawingDefault)))
            .collect()
    };
    check_table_on(rect, &rows([8.0, 12.0, 11.0, 9.0]));
    let turned = |s: ShapeBuilder| rect(s).cell("Angle", &FRAC_PI_2.to_string());
    check_table_on(turned, &rows([9.0, 11.0, 12.0, 8.0]));
    // A flip moves the box and does not swap the names: flipped in x about the local pin, the box stays where it is.
    let flipped = |s: ShapeBuilder| rect(s).cell("FlipX", "1");
    check_table_on(flipped, &rows([8.0, 12.0, 11.0, 9.0]));
    check_table_on(rect, &[("BOUNDINGBOXRECT", "BOUNDINGBOXRECT(4)", E::Err(Bad)), ("BOUNDINGBOXRECT", "BOUNDINGBOXRECT(-1)", E::Err(Bad))]);
}

/// A 4 by 2 rectangle, pin (10, 10), local pin (1, 1): 4, 2, 1, 3, 1, 1, -1, 0; index 8
/// is `#VALUE!`; at a quarter turn the width is 2 and the height 4.
#[test]
fn boundingboxdist_vectors() {
    let rect = |s: ShapeBuilder| {
        section(s, 0, vec![mv(0.0, 0.0), ln(4.0, 0.0), ln(4.0, 2.0), ln(0.0, 2.0), ln(0.0, 0.0)])
            .cell("PinX", "10")
            .cell("PinY", "10")
            .cell("LocPinX", "1")
            .cell("LocPinY", "1")
    };
    let want = [4.0, 2.0, 1.0, 3.0, 1.0, 1.0, -1.0, 0.0];
    let formulas: Vec<String> = (0..8).map(|i| format!("BOUNDINGBOXDIST({i})")).collect();
    let rows: Vec<(&str, &str, E)> = formulas.iter().zip(want).map(|(f, w)| ("BOUNDINGBOXDIST", f.as_str(), E::U(w, DrawingDefault))).collect();
    check_table_on(rect, &rows);
    check_table_on(rect, &[("BOUNDINGBOXDIST", "BOUNDINGBOXDIST(8)", E::Err(Bad))]);
    let turned = |s: ShapeBuilder| rect(s).cell("Angle", &FRAC_PI_2.to_string());
    check_table_on(
        turned,
        &[("BOUNDINGBOXDIST", "BOUNDINGBOXDIST(0)", E::U(2.0, DrawingDefault)), ("BOUNDINGBOXDIST", "BOUNDINGBOXDIST(1)", E::U(4.0, DrawingDefault))],
    );
}

/// A shape with no path has its upright width-height box, and a group's box holds its members, carried to the group's
/// frame; a member that is not shown (`NoShow`) does not count.
#[test]
fn a_box_with_no_path_is_the_size_and_a_group_holds_its_members() {
    let bare =
        |s: ShapeBuilder| s.cell("Width", "3").cell("Height", "2").cell("PinX", "5").cell("PinY", "5").cell("LocPinX", "1").cell("LocPinY", "1");
    check_table_on(
        bare,
        &[
            ("BOUNDINGBOXRECT", "BOUNDINGBOXRECT(0)", E::U(4.0, DrawingDefault)),
            ("BOUNDINGBOXRECT", "BOUNDINGBOXRECT(1)", E::U(7.0, DrawingDefault)),
            ("BOUNDINGBOXRECT", "BOUNDINGBOXRECT(2)", E::U(6.0, DrawingDefault)),
            ("BOUNDINGBOXRECT", "BOUNDINGBOXRECT(3)", E::U(4.0, DrawingDefault)),
        ],
    );
    // A group with no path of its own, holding a member whose path runs (0, 0) to (6, 4) at pin (2, 2) with local pin (0, 0): the box of the group
    // in its frame is (2, 2) to (8, 6), and the group's pin sits at (10, 10) with local pin (0, 0).
    let doc_of = |hidden: bool, formula: &str| -> Value {
        let member = section(ShapeBuilder::new(11), 0, vec![mv(0.0, 0.0), ln(6.0, 0.0), ln(6.0, 4.0)]).cell("PinX", "2").cell("PinY", "2");
        let member = if hidden { no_show(member, 0) } else { member };
        let group = ShapeBuilder::new(10)
            .group()
            .cell("PinX", "10")
            .cell("PinY", "10")
            .cell("Width", "1")
            .cell("Height", "1")
            .row_formula("User", None, RowKey::name("Probe"), "Value", formula, "0")
            .child(member);
        let doc = DocBuilder::new().page(1, PageBuilder::new().shape(group)).build();
        let at = CellAddress::new(SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0)), probe_addr());
        let mut ctx = EvalCtx::new(EvalOptions::default(), Budget::default());
        Evaluator::new(&doc, &mut ctx).evaluate(&at)
    };
    for (i, want) in [10.0 + 2.0, 10.0 + 8.0, 10.0 + 6.0, 10.0 + 2.0].into_iter().enumerate() {
        let Value::Number(q) = doc_of(false, &format!("BOUNDINGBOXRECT({i})")) else { panic!("not a number") };
        assert!(near(q.stored, want, 1e-9), "index {i}: {q:?} against {want}");
    }
    // With the member's section hidden the member has no shown path, and is its own upright width-height box (0 by 0, at its pin (2, 2)): the
    // right edge of the group's box is that of a point, not of the path.
    let Value::Number(q) = doc_of(true, "BOUNDINGBOXRECT(1)") else { panic!("not a number") };
    assert!(near(q.stored, 10.0 + 2.0, 1e-9), "{q:?}");
}

/// On the path, (1, 1) is 1, left of travel along +x; (1, -1) is -1; (4, 3) is -1, right of travel along
/// +y. Naive: an unsigned distance gives 1 for the second; the sign from the path's sense alone gives 1 for the third.
#[test]
fn disttopath_vectors() {
    check_table_on(
        path_shape,
        &[
            ("DISTTOPATH", "DISTTOPATH(Geometry1.Path,1,1)", E::U(1.0, DrawingDefault)),
            ("DISTTOPATH", "DISTTOPATH(Geometry1.Path,1,-1)", E::U(-1.0, DrawingDefault)),
            ("DISTTOPATH", "DISTTOPATH(Geometry1.Path,4,3)", E::U(-1.0, DrawingDefault)),
            ("DISTTOPATH", "DISTTOPATH(Geometry1.Path,2,0)", E::U(0.0, DrawingDefault)),
            ("DISTTOPATH", "DISTTOPATH(Geometry1.Path,2,3)", E::U(1.0, DrawingDefault)),
        ],
    );
}

/// (1, 1) is 0.125 and (4, 3) is 0.75, on the turned shape. Naive: x, y in the parent's frame
/// gives 1 for the first. On a move to (0, 0) and a line to (4, 0), (-5, 1) is 0 and (9, 1) is 1. Naive: the projection
/// unclamped gives -1.25 and 2.25.
#[test]
fn nearestpointonpath_vectors() {
    check_table_on(
        path_shape,
        &[
            ("NEARESTPOINTONPATH", "NEARESTPOINTONPATH(Geometry1.Path,1,1)", E::N(0.125)),
            ("NEARESTPOINTONPATH", "NEARESTPOINTONPATH(Geometry1.Path,4,3)", E::N(0.75)),
        ],
    );
    let line = |s: ShapeBuilder| section(s, 0, vec![mv(0.0, 0.0), ln(4.0, 0.0)]);
    check_table_on(
        line,
        &[
            ("NEARESTPOINTONPATH", "NEARESTPOINTONPATH(Geometry1.Path,-5,1)", E::N(0.0)),
            ("NEARESTPOINTONPATH", "NEARESTPOINTONPATH(Geometry1.Path,9,1)", E::N(1.0)),
            ("NEARESTPOINTONPATH", "NEARESTPOINTONPATH(Geometry1.Path,2,5)", E::N(0.5)),
        ],
    );
    // Of two equally near places the earlier counts, so the middle of an open path's ends (a U) gives the first end.
    let u = |s: ShapeBuilder| section(s, 0, vec![mv(0.0, 0.0), ln(0.0, 3.0), ln(4.0, 3.0), ln(4.0, 0.0)]);
    check_table_on(u, &[("NEARESTPOINTONPATH", "NEARESTPOINTONPATH(Geometry1.Path,2,-5)", E::N(0.0))]);
    // A closed path whose start is nearest: the start is the earlier place, 0 (not 1).
    let closed = |s: ShapeBuilder| section(s, 0, vec![mv(0.0, 0.0), ln(4.0, 0.0), ln(4.0, 2.0), ln(0.0, 2.0), ln(0.0, 0.0)]);
    check_table_on(closed, &[("NEARESTPOINTONPATH", "NEARESTPOINTONPATH(Geometry1.Path,-1,-1)", E::N(0.0))]);
}

/// A path tracing two sides of a 3 by 4 triangle is 7, not 12 closed; segment 2 of the path
/// is 5 and segment 3 is `#REF!`; a circle `Ellipse` row of centre (1, 1) through (2, 1) and (1, 2) is 2 pi; a move to (0, 0) and `ArcTo` to (2, 0)
/// with `A` 1 is pi. Naive: a chord gives 0 or 2.
#[test]
fn pathlength_vectors() {
    let triangle = |s: ShapeBuilder| section(s, 0, vec![mv(0.0, 0.0), ln(3.0, 0.0), ln(3.0, 4.0)]);
    check_table_on(triangle, &[("PATHLENGTH", "PATHLENGTH(Geometry1.Path)", E::U(7.0, DrawingDefault))]);
    check_table_on(
        path_shape,
        &[
            ("PATHLENGTH", "PATHLENGTH(Geometry1.Path,2)", E::U(5.0, DrawingDefault)),
            ("PATHLENGTH", "PATHLENGTH(Geometry1.Path,1)", E::U(3.0, DrawingDefault)),
            ("PATHLENGTH", "PATHLENGTH(Geometry1.Path,3)", E::Err(Ref)),
            ("PATHLENGTH", "PATHLENGTH(Geometry1.Path,2.9)", E::U(5.0, DrawingDefault)),
        ],
    );
    let circle = |s: ShapeBuilder| section(s, 0, vec![R("Ellipse", vec![("X", 1.0), ("Y", 1.0), ("A", 2.0), ("B", 1.0), ("C", 1.0), ("D", 2.0)])]);
    assert!(near(number(circle, "PATHLENGTH(Geometry1.Path)"), 2.0 * PI, 1e-5));
    let half = |s: ShapeBuilder| section(s, 0, vec![mv(0.0, 0.0), arc(2.0, 0.0, 1.0)]);
    assert!(near(number(half, "PATHLENGTH(Geometry1.Path)"), PI, 1e-5));
    // A positive bow bulges to the right of travel, so the middle of the arc is at (1, -1).
    expect_point("ARCTO", half, "POINTALONGPATH(Geometry1.Path,0.5)", (1.0, -1.0), 1e-5);
    // The direction there is along +x, and a quarter of the way it is turned to point up-right of that (a clockwise bend).
    assert!(near(number(half, "ANGLEALONGPATH(Geometry1.Path,0.5)"), 0.0, 1e-5));
    assert!(near(number(half, "ANGLEALONGPATH(Geometry1.Path,0)"), -FRAC_PI_2, 1e-5), "a semicircle leaves its pen going down");
}

/// On the path, travel 0.25 is segment 1 and 0.625 is 2; the join at 0.375 is the earlier,
/// 1 (a project rule), and travel 0 is 1. Naive: the row number, which counts the move, gives 2 and 2. 2, not 3 rows.
#[test]
fn pathsegment_and_segmentcount_vectors() {
    check_table_on(
        path_shape,
        &[
            ("PATHSEGMENT", "PATHSEGMENT(Geometry1.Path,0.25)", E::N(1.0)),
            ("PATHSEGMENT", "PATHSEGMENT(Geometry1.Path,0.625)", E::N(2.0)),
            ("PATHSEGMENT", "PATHSEGMENT(Geometry1.Path,0.375)", E::N(1.0)),
            ("PATHSEGMENT", "PATHSEGMENT(Geometry1.Path,0)", E::N(1.0)),
            ("PATHSEGMENT", "PATHSEGMENT(Geometry1.Path,1)", E::N(2.0)),
            ("SEGMENTCOUNT", "SEGMENTCOUNT(Geometry1.Path)", E::N(2.0)),
        ],
    );
    // A spline run (a `SplineStart` and its knots) is one segment, and an `Ellipse` one.
    let spline = |s: ShapeBuilder| {
        section(
            s,
            0,
            vec![
                mv(0.0, 0.0),
                R("SplineStart", vec![("X", 1.0), ("Y", 2.0), ("A", 0.0), ("B", 0.0), ("C", 1.0), ("D", 3.0)]),
                R("SplineKnot", vec![("X", 3.0), ("Y", 2.0), ("A", 0.0)]),
                R("SplineKnot", vec![("X", 4.0), ("Y", 0.0), ("A", 0.0)]),
                ln(6.0, 0.0),
            ],
        )
    };
    check_table_on(spline, &[("TRAVEL", "SEGMENTCOUNT(Geometry1.Path)", E::N(2.0)), ("TRAVEL", "PATHSEGMENT(Geometry1.Path,1)", E::N(2.0))]);
}

/// `offset` 1 moves the place left of travel, to (2, 2), and -1 right, to (4, 2); the segment is
/// the fourth argument: segment 1 at 0.625 is (1.875, 0), segment 2 is (3, 3.125), and segment 3 is `#REF!`.
#[test]
fn pointalongpath_vectors() {
    expect_point("POINTALONGPATH", path_shape, "POINTALONGPATH(Geometry1.Path,0.625,1)", (2.0, 2.0), 1e-9);
    expect_point("POINTALONGPATH", path_shape, "POINTALONGPATH(Geometry1.Path,0.625,-1)", (4.0, 2.0), 1e-9);
    expect_point("POINTALONGPATH", path_shape, "POINTALONGPATH(Geometry1.Path,0.625,0,1)", (1.875, 0.0), 1e-9);
    expect_point("POINTALONGPATH", path_shape, "POINTALONGPATH(Geometry1.Path,0.625,0,2)", (3.0, 3.125), 1e-9);
    expect_point("POINTALONGPATH", path_shape, "POINTALONGPATH(Geometry1.Path,0.625,,2)", (3.0, 3.125), 1e-9);
    check_table_on(path_shape, &[("POINTALONGPATH", "PNTX(POINTALONGPATH(Geometry1.Path,0.625,0,3))", E::Err(Ref))]);
    // An offset on a curve is square to the tangent there: on a semicircle bowing down, the middle's tangent is +x, left of it is up.
    let half = |s: ShapeBuilder| section(s, 0, vec![mv(0.0, 0.0), arc(2.0, 0.0, 1.0)]);
    expect_point("POINTALONGPATH", half, "POINTALONGPATH(Geometry1.Path,0.5,0.5)", (1.0, -0.5), 1e-5);
    // At the corner the offset is square to the row leaving it, and at the very end to the row arriving.
    expect_point("TRAVEL", path_shape, "POINTALONGPATH(Geometry1.Path,0.375,1)", (2.0, 0.0), 1e-9);
    expect_point("TRAVEL", path_shape, "POINTALONGPATH(Geometry1.Path,1,1)", (2.0, 5.0), 1e-9);
}

// ---- arguments, and what the spec leaves to a project rule ---------------------------------------------------------------------------

/// A travel outside 0 to 1 is `#VALUE!`, with a rounding error's room; the path argument must be a `Geometry` section; the wrong number
/// of arguments is `#VALUE!`; an error in an argument comes back.
#[test]
fn arguments_are_checked() {
    check_table_on(
        path_shape,
        &[
            ("TRAVEL", "PNTX(POINTALONGPATH(Geometry1.Path,1.5))", E::Err(Bad)),
            ("TRAVEL", "PNTX(POINTALONGPATH(Geometry1.Path,-0.5))", E::Err(Bad)),
            ("TRAVEL", "ANGLEALONGPATH(Geometry1.Path,2)", E::Err(Bad)),
            ("TRAVEL", "PATHSEGMENT(Geometry1.Path,-1)", E::Err(Bad)),
            ("TRAVEL", "PATHLENGTH(Width)", E::Err(Bad)),
            ("TRAVEL", "PATHLENGTH(3)", E::Err(Bad)),
            ("ALL", "PATHLENGTH()", E::Err(Bad)),
            ("ALL", "PATHLENGTH(Geometry1.Path,1,2)", E::Err(Bad)),
            ("ALL", "POINTALONGPATH(Geometry1.Path)", E::Err(Bad)),
            ("ALL", "NEARESTPOINTONPATH(Geometry1.Path,1)", E::Err(Bad)),
            ("ALL", "ANGLEALONGPATH(Geometry1.Path,1/0)", E::Err(diacraft_calc::CellError::DivZero)),
            ("ALL", "NEARESTPOINTONPATH(Geometry1.Path,REF(),1)", E::Err(Ref)),
        ],
    );
    // A travel a rounding error past 1 is the end: NEARESTPOINTONPATH can feed POINTALONGPATH one.
    expect_point("TRAVEL", path_shape, "POINTALONGPATH(Geometry1.Path,1.0000000001)", (3.0, 5.0), 1e-9);
    expect_point("TRAVEL", path_shape, "POINTALONGPATH(Geometry1.Path,-0.0000000001)", (0.0, 0.0), 1e-9);
    // The stock formula: the point of a path nearest another, through the fraction.
    expect_point("TRAVEL", path_shape, "POINTALONGPATH(Geometry1.Path,NEARESTPOINTONPATH(Geometry1.Path,4,3))", (3.0, 3.0), 1e-9);
}

/// Units: a path whose cells are given in millimetres is in inches, and its length is a drawing length (the units): 25.4 MM is 1 inch.
#[test]
fn a_path_in_millimetres_is_in_inches() {
    let mm = |s: ShapeBuilder| {
        let unit = |v: &str| Cell::constant(DocValue::from_text(v)).with_unit("MM");
        let s = s.row_type("Geometry", Some(0), RowKey::Ix(0), "MoveTo").row_type("Geometry", Some(0), RowKey::Ix(1), "LineTo");
        s.set(CellAddr::in_row("Geometry", Some(0), RowKey::Ix(0), "X"), unit("0"))
            .set(CellAddr::in_row("Geometry", Some(0), RowKey::Ix(0), "Y"), unit("0"))
            .set(CellAddr::in_row("Geometry", Some(0), RowKey::Ix(1), "X"), unit("1"))
            .set(CellAddr::in_row("Geometry", Some(0), RowKey::Ix(1), "Y"), unit("0"))
    };
    // `V` holds inches whatever `U` shows, so a stored 1 with `U` MM is an inch.
    check_table_on(mm, &[("units", "PATHLENGTH(Geometry1.Path)", E::U(1.0, DrawingDefault))]);
    // And a formula in the path's rows that names a unit converts: `1 cm` is 0.3937 inches.
    let formula = |s: ShapeBuilder| {
        s.row_type("Geometry", Some(0), RowKey::Ix(0), "MoveTo")
            .row_type("Geometry", Some(0), RowKey::Ix(1), "LineTo")
            .row_cell("Geometry", Some(0), RowKey::Ix(0), "X", "0")
            .row_cell("Geometry", Some(0), RowKey::Ix(0), "Y", "0")
            .row_formula("Geometry", Some(0), RowKey::Ix(1), "X", "1 cm*2", "0.7874015748031497")
            .row_cell("Geometry", Some(0), RowKey::Ix(1), "Y", "0")
    };
    assert!(near(number(formula, "PATHLENGTH(Geometry1.Path)"), 2.0 / 2.54, 1e-9));
}

/// Curved rows: the length of an `ArcTo`, an `EllipticalArcTo` and a `NURBSTo` is the length of the curve the geometry crate draws, measured
/// independently by sampling it, and a place a share along is on it.
#[test]
fn a_curved_row_is_walked_along_its_curve() {
    let nurbs = |s: ShapeBuilder| {
        s.row_type("Geometry", Some(0), RowKey::Ix(0), "MoveTo")
            .row_cell("Geometry", Some(0), RowKey::Ix(0), "X", "0")
            .row_cell("Geometry", Some(0), RowKey::Ix(0), "Y", "0")
            .row_type("Geometry", Some(0), RowKey::Ix(1), "NURBSTo")
            .row_cell("Geometry", Some(0), RowKey::Ix(1), "X", "4")
            .row_cell("Geometry", Some(0), RowKey::Ix(1), "Y", "0")
            .row_cell("Geometry", Some(0), RowKey::Ix(1), "A", "0")
            .row_cell("Geometry", Some(0), RowKey::Ix(1), "B", "1")
            .row_cell("Geometry", Some(0), RowKey::Ix(1), "C", "0")
            .row_cell("Geometry", Some(0), RowKey::Ix(1), "D", "1")
            .row_formula("Geometry", Some(0), RowKey::Ix(1), "E", "NURBS(1,3,1,1,1,2,0,1,3,2,0,1)", "NURBS(1, 3, 1, 1, 1, 2, 0, 1, 3, 2, 0, 1)")
    };
    // The same curve through the geometry crate, sampled finely.
    let reference = {
        let nurbs_row = GeomRow::NurbsTo {
            x: 4.0,
            y: 0.0,
            second_last_knot: 0.0,
            last_weight: 1.0,
            first_knot: 0.0,
            first_weight: 1.0,
            nurbs: Some(diacraft_geom::Nurbs::from_text("NURBS(1,3,1,1,1,2,0,1,3,2,0,1)").unwrap()),
        };
        let built = build_paths(&[GeomRow::MoveTo { x: 0.0, y: 0.0 }, nurbs_row], diacraft_geom::kurbo::Size::new(1.0, 1.0)).unwrap();
        let mut total = 0.0;
        for seg in built.stroke.segments() {
            let PathSeg::Cubic(c) = seg else { panic!("not a cubic") };
            let mut prev = c.eval(0.0);
            for i in 1..=20_000 {
                let next = c.eval(f64::from(i) / 20_000.0);
                total += prev.distance(next);
                prev = next;
            }
        }
        total
    };
    let got = number(nurbs, "PATHLENGTH(Geometry1.Path)");
    assert!(near(got, reference, 1e-6), "{got} against {reference}");
    assert!(reference > 4.0, "the curve rises above its chord");
    // Half way along a symmetric curve is its top.
    expect_point("NURBS", nurbs, "POINTALONGPATH(Geometry1.Path,0.5)", (2.0, 1.5), 1e-6);
}

/// A rounded rectangle (a closed run, four corners of straight rows) loses 2r - r sqrt 2 at each corner, the closing corner included,
/// and a corner that touches a curve stays sharp: the same rows with an arc in place of the last side are as long as without
/// `Rounding` at the corners the arc touches.
#[test]
fn a_rounded_rectangle_loses_each_corner_and_a_corner_at_a_curve_does_not() {
    let rect = |r: f64| {
        move |s: ShapeBuilder| {
            section(s, 0, vec![mv(0.0, 0.0), ln(4.0, 0.0), ln(4.0, 2.0), ln(0.0, 2.0), ln(0.0, 0.0)]).cell("Rounding", &r.to_string())
        }
    };
    let r = 0.5;
    let want = 12.0 - 4.0 * (2.0 * r - 2.0f64.sqrt() * r);
    assert!(near(number(rect(r), "PATHLENGTH(Geometry1.Path)"), want, 1e-9));
    assert!(near(number(rect(0.0), "PATHLENGTH(Geometry1.Path)"), 12.0, 1e-9));
    // The segments are still the four sides: the chords belong to the segment that arrives (project rule), and are no segments of their own.
    check_table_on(rect(r), &[("TRAVEL", "SEGMENTCOUNT(Geometry1.Path)", E::N(4.0))]);
    // A half circle then a line: the corner between them touches a curve and is not cut, with or without Rounding.
    let mixed = |r: f64| move |s: ShapeBuilder| section(s, 0, vec![mv(0.0, 0.0), arc(2.0, 0.0, 1.0), ln(2.0, 3.0)]).cell("Rounding", &r.to_string());
    let (with, without) = (number(mixed(1.0), "PATHLENGTH(Geometry1.Path)"), number(mixed(0.0), "PATHLENGTH(Geometry1.Path)"));
    assert!(near(with, without, 1e-9) && near(without, PI + 3.0, 1e-5), "{with} {without}");
}

/// The marker: 224,411 shapes of the sample files hold `Rounding` as `Themed`, and an operand saved so is no value. The walk asks the
/// cell's own formula what the theme gives (`THEMEVAL()`, which with no theme gives the root style's `Rounding`), as the renderer does, in the
/// oracle's mode (operands as saved) as in the live one. A theme that cannot answer leaves the function unsupported, the saved value standing.
#[test]
fn a_rounding_saved_as_themed_is_read_through_the_theme() {
    let themed = |root: &str| {
        let shape = section(ShapeBuilder::new(1), 0, vec![mv(0.0, 0.0), ln(4.0, 0.0), ln(4.0, 3.0)])
            .formula("Rounding", "THEMEVAL()", "Themed")
            .row_formula("User", None, RowKey::name("Probe"), "Value", "PATHLENGTH(Geometry1.Path)", "0");
        let doc = DocBuilder::new().root(|r| r.cell("Rounding", root)).page(1, PageBuilder::new().shape(shape)).build();
        let at = CellAddress::new(SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0)), probe_addr());
        (doc, at)
    };
    for operands in [OperandMode::Cached, OperandMode::Live] {
        for (root, want) in [("0", 7.0), ("1", 3.0 + 2.0f64.sqrt() + 2.0)] {
            let (doc, at) = themed(root);
            let mut ctx = EvalCtx::new(EvalOptions { operands, ..EvalOptions::default() }, Budget::default());
            let Value::Number(q) = Evaluator::new(&doc, &mut ctx).evaluate(&at) else { panic!("{operands:?} {root}: not a number") };
            assert!(near(q.stored, want, 1e-9), "{operands:?}, root Rounding {root}: {q:?}");
        }
    }
}

/// Relative rows are shares of `Width` and `Height`, read from the shape the path is on, and they follow the cells when those change: a path
/// of `RelMoveTo`, `RelLineTo` and `RelLineTo` over a 4 by 3 shape is 4 + 3 long, and over 8 by 3 it is 8 + 3.
#[test]
fn relative_rows_follow_the_size_of_the_shape() {
    let rel = |w: f64| {
        move |s: ShapeBuilder| {
            section(
                s,
                0,
                vec![
                    R("RelMoveTo", vec![("X", 0.0), ("Y", 0.0)]),
                    R("RelLineTo", vec![("X", 1.0), ("Y", 0.0)]),
                    R("RelLineTo", vec![("X", 1.0), ("Y", 1.0)]),
                ],
            )
            .cell("Width", &w.to_string())
            .cell("Height", "3")
        }
    };
    assert!(near(number(rel(4.0), "PATHLENGTH(Geometry1.Path)"), 7.0, 1e-9));
    assert!(near(number(rel(8.0), "PATHLENGTH(Geometry1.Path)"), 11.0, 1e-9));
    // A cell of the path that is a formula is worked out, and the walk follows it: `X` of the second row is `Width/2`.
    let formula = |s: ShapeBuilder| {
        section(s, 0, vec![mv(0.0, 0.0), ln(1.0, 0.0)]).cell("Width", "10").row_formula("Geometry", Some(0), RowKey::Ix(1), "X", "Width/2", "1")
    };
    assert!(near(number(formula, "PATHLENGTH(Geometry1.Path)"), 5.0, 1e-9), "Live operands: the cell is worked out, not read as saved");
}

/// The application measures curves numerically and its saved values sit up to 2E-5 (in units of the larger of 1 and the sizes) from a
/// true arc length, so the comparer gives a cell that calls one of the nine path functions that room, `Rounding` and `MAX` around the call
/// included, and gives it to nothing else: the geometry functions keep 1E-6 and the rest 1E-9.
#[test]
fn the_oracle_compares_a_path_function_within_two_hundred_thousandths() {
    use diacraft_calc::recalc_all_with;

    // A cell that holds `formula` and saved the value the evaluator gives it, `over` higher in units of the larger of 1 and the value (the
    // comparer's own scale): does the comparer let it agree?
    let agrees = |formula: &str, over: f64| {
        let Value::Number(q) = eval_on(path_shape, formula) else { panic!("{formula}: not a number") };
        let saved = q.stored + over * q.stored.abs().max(1.0);
        let s = path_shape(ShapeBuilder::new(1)).set(user("C"), cell(formula, &saved.to_string(), None));
        let doc = DocBuilder::new().page(1, PageBuilder::new().shape(s)).build();
        let opts = EvalOptions { operands: OperandMode::Cached, ..EvalOptions::default() };
        recalc_all_with(&doc, &Budget::default(), opts).differed.is_empty()
    };
    for formula in [
        "PATHLENGTH(Geometry1.Path)",
        "MAX(1,PATHLENGTH(Geometry1.Path))",
        "PNTX(POINTALONGPATH(Geometry1.Path,0.625))",
        "NEARESTPOINTONPATH(Geometry1.Path,1,1)",
        "DISTTOPATH(Geometry1.Path,4,3)",
        "ANGLEALONGPATH(Geometry1.Path,0.625)",
        "PATHSEGMENT(Geometry1.Path,0.625)",
        "SEGMENTCOUNT(Geometry1.Path)",
        "BOUNDINGBOXDIST(0)",
        "BOUNDINGBOXRECT(1)",
    ] {
        assert!(agrees(formula, 1.5e-5), "{formula}: 1.5E-5 off agrees");
        assert!(!agrees(formula, 6e-5), "{formula}: 6E-5 off differs");
    }
    // Not for another function: 1E-9 for `1+1`, and 1E-6 for a frame function of the same batch.
    assert!(!agrees("1+1", 1.5e-5));
    assert!(!agrees("PNTX(PAR(PNT(5,2)))", 1.5e-5));
    assert!(agrees("PNTX(PAR(PNT(5,2)))", 1e-7));
}

/// Inheritance: an instance of a master draws the master's rows, with its own `Width` and `Height`, and a row it holds
/// itself (the same `IX`) changes just that row.
#[test]
fn an_instance_walks_its_masters_rows() {
    let master = || section(ShapeBuilder::new(5), 0, vec![mv(0.0, 0.0), ln(4.0, 0.0), ln(4.0, 3.0)]);
    let instance = |own: bool| {
        let s = ShapeBuilder::new(1).master(1).row_formula("User", None, RowKey::name("Probe"), "Value", "PATHLENGTH(Geometry1.Path)", "0");
        if own { s.row_cell("Geometry", Some(0), RowKey::Ix(2), "Y", "13") } else { s }
    };
    for (own, want) in [(false, 7.0), (true, 4.0 + 13.0)] {
        let doc = DocBuilder::new()
            .master(1, diacraft_testkit::build::MasterBuilder::new().shape(master()))
            .page(1, PageBuilder::new().shape(instance(own)))
            .build();
        let at = CellAddress::new(SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0)), probe_addr());
        let mut ctx = EvalCtx::new(EvalOptions::default(), Budget::default());
        let Value::Number(q) = Evaluator::new(&doc, &mut ctx).evaluate(&at) else { panic!("not a number") };
        assert!(near(q.stored, want, 1e-9), "own row: {own}: {q:?}");
    }
}

/// A path another shape holds (`Sheet.2!Geometry1.Path`) is walked as that shape's, and the point comes out in its frame: `PAR` carries it
/// from there.
#[test]
fn a_path_of_another_shape_is_walked_in_its_frame() {
    let other = || {
        section(ShapeBuilder::new(2), 0, vec![mv(0.0, 0.0), ln(4.0, 0.0)])
            .cell("PinX", "10")
            .cell("PinY", "20")
            .cell("LocPinX", "1")
            .cell("LocPinY", "1")
    };
    let probe = |formula: &str| ShapeBuilder::new(1).row_formula("User", None, RowKey::name("Probe"), "Value", formula, "0");
    let value = |formula: &str| {
        let doc = DocBuilder::new().page(1, PageBuilder::new().shape(probe(formula)).shape(other())).build();
        let at = CellAddress::new(SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0)), probe_addr());
        let mut ctx = EvalCtx::new(EvalOptions::default(), Budget::default());
        Evaluator::new(&doc, &mut ctx).evaluate(&at)
    };
    let Value::Number(len) = value("PATHLENGTH(Sheet.2!Geometry1.Path)") else { panic!("not a number") };
    assert!(near(len.stored, 4.0, 1e-9));
    // Half way along the path, (2, 0) in shape 2's frame, is (11, 19) on the page; read on shape 1, the point is not shape 1's.
    let Value::Number(x) = value("PNTX(PAR(POINTALONGPATH(Sheet.2!Geometry1.Path,0.5)))") else { panic!("not a number") };
    let Value::Number(y) = value("PNTY(PAR(POINTALONGPATH(Sheet.2!Geometry1.Path,0.5)))") else { panic!("not a number") };
    assert!(near(x.stored, 11.0, 1e-9) && near(y.stored, 19.0, 1e-9), "{x:?} {y:?}");
}

// ----: constructors, the spline cap, degenerate curves, cycles, InfiniteLine, the half turn, the box's depth -------------------

/// `NURBS(1, 0, 1, 1, ...)` with this many tuples: a degree of 0 is no curve, so the row is skipped as soon as it is read, and its cost is the
/// reading of its constructor and no spline.
fn big_constructor(tuples: usize) -> String {
    let body: Vec<String> = (0..tuples).map(|i| format!("{i}, {}, 0, 1", i % 7)).collect();
    format!("NURBS(1, 0, 1, 1, {})", body.join(", "))
}

/// A shape with a move and `rows` `NURBSTo` rows whose `E` is the formula `User.Big`, one constructor of `tuples` tuples that they all read.
fn shared_constructor(s: ShapeBuilder, rows: usize, tuples: usize) -> ShapeBuilder {
    let mut s = section(s, 0, vec![mv(0.0, 0.0)]).set(
        CellAddr::in_row("User", None, RowKey::name("Big"), "Value"),
        Cell::constant(DocValue::from_text(&big_constructor(tuples))).with_unit("NURBS"),
    );
    for i in 1..=rows {
        let key = RowKey::Ix(u32::try_from(i).unwrap());
        s = s
            .row_type("Geometry", Some(0), key.clone(), "NURBSTo")
            .row_cell("Geometry", Some(0), key.clone(), "X", "1")
            .row_cell("Geometry", Some(0), key.clone(), "Y", "1")
            .row_formula("Geometry", Some(0), key, "E", "User.Big", "NURBS(1, 0, 1, 1, 0, 0, 0, 1)");
    }
    s
}

/// M2: a constructor a row reads is charged before it is copied into the row, at a step to 64 bytes, and held while the section is drawn: rows
/// that all read one big constructor cost that many times its size in steps, and a budget smaller than that ends the run before the copies.
#[test]
fn rows_that_read_one_big_constructor_are_charged_for_it() {
    let steps_of = |rows: usize, tuples: usize, budget: Budget| {
        let (doc, at) = probe(|s| shared_constructor(s, rows, tuples), "PATHLENGTH(Geometry1.Path)");
        let mut ctx = EvalCtx::new(EvalOptions::default(), budget);
        let v = Evaluator::new(&doc, &mut ctx).evaluate(&at);
        (ctx.stats.steps, ctx.stats.bytes_peak, ctx.stats.budget_exhausted, v)
    };
    // 2,000 tuples are 8,004 numbers, 64 KB: a thousand steps a row. 40 rows against 10 are 30 rows more, and 30,000 steps more at least.
    let (few, few_bytes, _, v) = steps_of(10, 2000, Budget::default());
    assert!(matches!(v, Value::Number(_)), "{v:?}");
    let (many, many_bytes, _, _) = steps_of(40, 2000, Budget::default());
    let bytes = 8_004u64 * 8;
    assert!(many - few >= 30 * (bytes / 64), "{few} steps for 10 rows and {many} for 40: each row's constructor is charged ({} bytes)", bytes);
    assert!(many_bytes - few_bytes >= 30 * bytes, "the numbers a row keeps are held: {few_bytes} and {many_bytes} bytes");
    // The same rows over a constructor of a few numbers cost almost nothing for it.
    let (small, ..) = steps_of(40, 3, Budget::default());
    assert!(many > 8 * small, "{many} against {small}");
    // A budget smaller than the copies ends the run first, inside the budget.
    let (taken, _, exhausted, v) = steps_of(200, 2000, Budget::with_steps(20_000));
    assert!(exhausted && taken <= 20_000, "{taken} steps of 20,000");
    assert_eq!(v, Value::Error(diacraft_calc::rules::LOOP_ERROR));
}

/// M3: a section whose splines need more work than the budget leaves is the loop error and never a path short of its curves. Six curves of degree
/// 12 through 60 points need 21 million multiplications, 41,000 steps' worth, and some 6,000 pieces: with a budget of 30,000 steps the steps for
/// the pieces are there and the cap the splines get from what is left, 15 million, is not, so the work is the limit that stops the section.
#[test]
fn a_section_whose_splines_pass_their_work_is_an_error_and_not_a_short_path() {
    let tuples: Vec<String> = (0..60).map(|i| format!("{},{},{},1", i, i % 7, f64::from(i) / 60.0)).collect();
    let constructor = format!("NURBS(1,12,1,1,{})", tuples.join(","));
    let rows = |s: ShapeBuilder| {
        let mut s = section(s, 0, vec![mv(0.0, 0.0)]);
        for i in 1..=6 {
            let key = RowKey::Ix(i);
            s = s
                .row_type("Geometry", Some(0), key.clone(), "NURBSTo")
                .row_cell("Geometry", Some(0), key.clone(), "X", "60")
                .row_cell("Geometry", Some(0), key.clone(), "Y", "0")
                .row_cell("Geometry", Some(0), key.clone(), "A", "1")
                .row_cell("Geometry", Some(0), key.clone(), "B", "1")
                .row_cell("Geometry", Some(0), key.clone(), "C", "0")
                .row_cell("Geometry", Some(0), key.clone(), "D", "1")
                .row_formula("Geometry", Some(0), key, "E", &constructor, "0");
        }
        s
    };
    let run = |budget: Budget| {
        let (doc, at) = probe(rows, "PATHLENGTH(Geometry1.Path)");
        let mut ctx = EvalCtx::new(EvalOptions::default(), budget);
        let v = Evaluator::new(&doc, &mut ctx).evaluate(&at);
        (ctx.stats.steps, ctx.stats.budget_exhausted, v)
    };
    let (steps, _, v) = run(Budget::default());
    let Value::Number(whole) = v else { panic!("{v:?}") };
    assert!(whole.stored > 6.0 * 40.0, "six curves of 60 across: {whole:?}");
    assert!(steps > 41_000, "the splines' work is charged: {steps} steps");
    // The same rows with 30,000 steps: the loop error, with steps to spare. Not a shorter length.
    let (taken, exhausted, v) = run(Budget::with_steps(30_000));
    assert_eq!(v, Value::Error(diacraft_calc::rules::LOOP_ERROR), "{v:?} after {taken} steps");
    assert!(!exhausted && taken < 30_000, "the work and not the steps stopped it: {taken} steps, exhausted {exhausted}");
    // A budget that holds the work gives the length it gave before.
    let (_, _, v) = run(Budget::with_steps(steps + 100));
    assert_eq!(v, Value::Number(whole));
}

/// M4: a curve whose control polygon has no length has none: kurbo's arc length of a quadratic whose three points coincide is not a number, and
/// that made the whole section an error. A `RelQuadBezTo` and a `RelCubBezTo` on a shape of no size (their points are shares of 0) are such
/// curves.
#[test]
fn a_curve_on_a_shape_of_no_size_has_no_length_and_is_no_error() {
    let flat = |s: ShapeBuilder| {
        section(
            s,
            0,
            vec![
                mv(0.0, 0.0),
                R("RelQuadBezTo", vec![("X", 0.5), ("Y", 0.5), ("A", 0.25), ("B", 0.75)]),
                R("RelCubBezTo", vec![("X", 1.0), ("Y", 1.0), ("A", 0.25), ("B", 0.5), ("C", 0.75), ("D", 0.5)]),
            ],
        )
        .cell("Width", "0")
        .cell("Height", "0")
    };
    check_table_on(
        flat,
        &[
            ("M4", "PATHLENGTH(Geometry1.Path)", E::U(0.0, DrawingDefault)),
            ("M4", "PATHLENGTH(Geometry1.Path,2)", E::U(0.0, DrawingDefault)),
            ("M4", "SEGMENTCOUNT(Geometry1.Path)", E::N(2.0)),
            ("TRAVEL", "PNTX(POINTALONGPATH(Geometry1.Path,0.5))", E::Err(Ref)),
            ("TRAVEL", "NEARESTPOINTONPATH(Geometry1.Path,1,1)", E::Err(Ref)),
        ],
    );
    // The same curves on a shape with a size are walked.
    let sized = |s: ShapeBuilder| flat(s).cell("Width", "4").cell("Height", "2");
    let Value::Number(q) = eval_on(sized, "PATHLENGTH(Geometry1.Path)") else { panic!("not a number") };
    assert!(q.stored > 1.0);
}

/// Minus pi and plus pi are one direction, and the comparer says so for a direction that is the
/// whole formula, whichever the file saved. It says so for nothing else: not a direction added to something, not a number that is not pi.
#[test]
fn the_oracle_takes_minus_pi_and_plus_pi_as_one_direction() {
    use diacraft_calc::recalc_all_with;

    let along_negative_x = |s: ShapeBuilder| section(s, 0, vec![mv(0.0, 0.0), ln(-3.0, 0.0)]);
    let differs = |formula: &str, saved: f64| {
        let s = along_negative_x(ShapeBuilder::new(1)).set(user("C"), cell(formula, &saved.to_string(), None));
        let doc = DocBuilder::new().page(1, PageBuilder::new().shape(s)).build();
        let opts = EvalOptions { operands: OperandMode::Cached, ..EvalOptions::default() };
        !recalc_all_with(&doc, &Budget::default(), opts).differed.is_empty()
    };
    let Value::Number(q) = eval_on(along_negative_x, "ANGLEALONGPATH(Geometry1.Path,0.5)") else { panic!("not a number") };
    assert!((q.stored.abs() - PI).abs() < 1e-12, "{q:?}");
    for formula in ["ANGLEALONGPATH(Geometry1.Path,0.5)", "ANGLEALONGPATH(Geometry1.Path,0.25,1)"] {
        assert!(!differs(formula, PI) && !differs(formula, -PI), "{formula}: either end of the half turn agrees");
        assert!(differs(formula, 0.0) && differs(formula, FRAC_PI_2) && differs(formula, PI - 1e-3), "{formula}: nothing else agrees");
    }
    // A direction that is part of a formula is its own number.
    let sum_differs = |saved: f64| differs("ANGLEALONGPATH(Geometry1.Path,0.5)+0", saved) || differs("ANGLEALONGPATH(Geometry1.Path,0.5)*1", saved);
    assert!(sum_differs(PI) || sum_differs(-PI), "one of the two is a turn from the number the sum gives");
    // The frame functions that give a direction: ANGLETOPAR of a half turn.
    let turned = |s: ShapeBuilder| s.cell("Width", "1");
    for saved in [PI, -PI] {
        let s = turned(ShapeBuilder::new(1)).set(user("C"), cell("ANGLETOPAR(PI(),Width,Width)", &saved.to_string(), None));
        let doc = DocBuilder::new().page(1, PageBuilder::new().shape(s)).build();
        let opts = EvalOptions { operands: OperandMode::Cached, ..EvalOptions::default() };
        assert!(recalc_all_with(&doc, &Budget::default(), opts).differed.is_empty(), "ANGLETOPAR saved {saved}");
    }
    // `ATAN2` keeps its two allowances.
    let s = ShapeBuilder::new(1).set(user("C"), cell("ATAN2(0,-1)", &(-PI).to_string(), None));
    let doc = DocBuilder::new().page(1, PageBuilder::new().shape(s)).build();
    let opts = EvalOptions { operands: OperandMode::Cached, ..EvalOptions::default() };
    assert!(recalc_all_with(&doc, &Budget::default(), opts).differed.is_empty());
}

/// A section of an `InfiniteLine` row has no end and no length, so the walks are `#REF!` as for a path with no length, `PATHLENGTH`
/// is 0 and the line is not a segment; in a section with other rows it is no part of the path.
#[test]
fn an_infinite_line_is_no_part_of_a_path() {
    let line = R("InfiniteLine", vec![("X", 0.0), ("Y", 0.0), ("A", 1.0), ("B", 1.0)]);
    let only = |s: ShapeBuilder| section(s, 0, vec![R("InfiniteLine", vec![("X", 0.0), ("Y", 0.0), ("A", 1.0), ("B", 1.0)])]);
    check_table_on(
        only,
        &[
            ("TRAVEL", "PNTX(POINTALONGPATH(Geometry1.Path,0.5))", E::Err(Ref)),
            ("TRAVEL", "ANGLEALONGPATH(Geometry1.Path,0.5)", E::Err(Ref)),
            ("TRAVEL", "NEARESTPOINTONPATH(Geometry1.Path,1,1)", E::Err(Ref)),
            ("TRAVEL", "DISTTOPATH(Geometry1.Path,1,1)", E::Err(Ref)),
            ("TRAVEL", "PATHSEGMENT(Geometry1.Path,0.5)", E::Err(Ref)),
            ("PATHLENGTH", "PATHLENGTH(Geometry1.Path)", E::U(0.0, DrawingDefault)),
            ("TRAVEL", "SEGMENTCOUNT(Geometry1.Path)", E::N(0.0)),
        ],
    );
    let beside = |s: ShapeBuilder| {
        let _ = &line;
        section(
            s,
            0,
            vec![mv(0.0, 0.0), ln(3.0, 0.0), R("InfiniteLine", vec![("X", 0.0), ("Y", 0.0), ("A", 1.0), ("B", 1.0)]), mv(5.0, 5.0), ln(5.0, 9.0)],
        )
    };
    check_table_on(
        beside,
        &[("TRAVEL", "PATHLENGTH(Geometry1.Path)", E::U(7.0, DrawingDefault)), ("TRAVEL", "SEGMENTCOUNT(Geometry1.Path)", E::N(2.0))],
    );
}

/// A row cell that calls a path function on its own section reads itself: worked out live, that is a cycle and the loop error (and no hang); read
/// as saved, as the oracle reads operands, it is the number. Two shapes whose paths read each other are a cycle too, which a cell in it ends by
/// reading its last good value, in a few steps.
#[test]
fn a_path_that_reads_itself_or_another_that_reads_it_is_a_cycle_and_ends() {
    let own = |s: ShapeBuilder| {
        section(s, 0, vec![mv(0.0, 0.0), ln(3.0, 0.0), ln(3.0, 4.0)]).row_formula(
            "Geometry",
            Some(0),
            RowKey::Ix(2),
            "X",
            "PATHLENGTH(Geometry1.Path)*0+3",
            "3",
        )
    };
    let at_row = || CellAddress::new(SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0)), CellAddr::in_row("Geometry", Some(0), RowKey::Ix(2), "X"));
    let doc = DocBuilder::new().page(1, PageBuilder::new().shape(own(ShapeBuilder::new(1)))).build();
    for operands in [OperandMode::Live, OperandMode::Cached] {
        let mut ctx = EvalCtx::new(EvalOptions { operands, ..EvalOptions::default() }, Budget::default());
        let v = Evaluator::new(&doc, &mut ctx).evaluate(&at_row());
        match operands {
            OperandMode::Live => assert_eq!(v, Value::Error(diacraft_calc::rules::LOOP_ERROR), "{v:?}"),
            OperandMode::Cached => assert!(matches!(&v, Value::Number(q) if (q.stored - 3.0).abs() < 1e-9), "{v:?}"),
        }
        assert!(ctx.stats.steps < 1000, "{:?}", ctx.stats);
    }
    // Shape 1's row reads shape 2's path and shape 2's row reads shape 1's.
    let pair = |reads: &str| {
        section(ShapeBuilder::new(if reads.contains("Sheet.2") { 1 } else { 2 }), 0, vec![mv(0.0, 0.0), ln(3.0, 0.0), ln(3.0, 4.0)]).row_formula(
            "Geometry",
            Some(0),
            RowKey::Ix(2),
            "X",
            reads,
            "3",
        )
    };
    let doc = DocBuilder::new()
        .page(1, PageBuilder::new().shape(pair("PATHLENGTH(Sheet.2!Geometry1.Path)*0+3")).shape(pair("PATHLENGTH(Sheet.1!Geometry1.Path)*0+3")))
        .build();
    for id in 0..2u32 {
        let at = CellAddress::new(SheetRef::Shape(ShapeRef::top(Owner::Page(1), id)), CellAddr::in_row("Geometry", Some(0), RowKey::Ix(2), "X"));
        let mut ctx = EvalCtx::new(EvalOptions::default(), Budget::default());
        let v = Evaluator::new(&doc, &mut ctx).evaluate(&at);
        // The cell in the middle of the cycle fails and reads as its last good value, so the answer is that value: the cycle is met, counted and
        // ended in a few steps, and nothing waits for the other to be worked out.
        assert!(matches!(&v, Value::Number(q) if (q.stored - 3.0).abs() < 1e-9), "shape {id}: {v:?}");
        assert!(ctx.stats.steps < 2000 && !ctx.stats.budget_exhausted, "shape {id}: {:?}", ctx.stats);
        assert!(ctx.stats.cycles >= 1, "the cycle was met and counted: {:?}", ctx.stats);
    }
}

/// The depth a box looks into is the evaluation's native depth: each level of group counts four of its 160, with the cell reads and the nodes
/// that hold the call, so the deepest tree a box accepts is far inside what a 1 MiB stack holds. Nested groups of any depth either give a number
/// or the loop error; the deepest accepted is found, and runs on a stack of 512 KiB.
#[test]
fn the_deepest_group_a_box_accepts_runs_on_a_small_stack() {
    let nested = |depth: u32| -> Value {
        let mut deepest = ShapeBuilder::new(1000).cell("Width", "1").cell("Height", "1");
        for id in (1..=depth).rev() {
            deepest = ShapeBuilder::new(id).group().cell("Width", "1").cell("Height", "1").child(deepest);
        }
        let top = deepest.row_formula("User", None, RowKey::name("Probe"), "Value", "BOUNDINGBOXRECT(1)", "0");
        let doc = DocBuilder::new().page(1, PageBuilder::new().shape(top)).build();
        let at = CellAddress::new(SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0)), probe_addr());
        Evaluator::new(&doc, &mut EvalCtx::default()).evaluate(&at)
    };
    let mut deepest_ok = 0;
    for depth in 1..=50 {
        match nested(depth) {
            Value::Number(_) => deepest_ok = depth,
            other => {
                assert_eq!(other, Value::Error(diacraft_calc::rules::LOOP_ERROR), "depth {depth}");
                break;
            }
        }
    }
    assert!((20..=40).contains(&deepest_ok), "the deepest group a box accepts is {deepest_ok} levels, with four of the 160 native levels to a level");
    let handle = std::thread::Builder::new().stack_size(512 * 1024).spawn(move || nested(deepest_ok)).unwrap();
    assert!(matches!(handle.join().unwrap(), Value::Number(_)));
}

// ---- the oracle: the walk is a calculation, not the saved value --------------------------------------------------------------------

pub fn cell(formula: &str, v: &str, unit: Option<&str>) -> Cell {
    let mut c = Cell::with_formula(formula, Some(DocValue::from_text(v)));
    if let Some(u) = unit {
        c = c.with_unit(u);
    }
    c
}

pub fn user(name: &str) -> CellAddr {
    CellAddr::in_row("User", None, RowKey::name(name), "Value")
}

/// A document whose cells call the path functions with the values a file would save.
fn saved_document() -> Document {
    let shape = path_shape(ShapeBuilder::new(1))
        .set(user("Len"), cell("PATHLENGTH(Geometry1.Path)", "8", Some("DL")))
        .set(user("Ang"), cell("ANGLEALONGPATH(Geometry1.Path,0.625)", &FRAC_PI_2.to_string(), Some("DA")))
        .set(user("Near"), cell("NEARESTPOINTONPATH(Geometry1.Path,1,1)", "0.125", None))
        .set(user("Dist"), cell("DISTTOPATH(Geometry1.Path,4,3)", "-1", Some("DL")))
        .set(user("Seg"), cell("PATHSEGMENT(Geometry1.Path,0.625)", "2", None))
        .set(user("Pt"), cell("POINTALONGPATH(Geometry1.Path,0.625)", "PNT(3,2)", Some("PNT")));
    DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build()
}

/// A calculator that hands back what the file saved: the echo the real-file salt exists to catch.
struct Echo;

impl Calculator for Echo {
    fn calculate(&mut self, ev: &mut Evaluator<'_, '_>, at: &CellAddress) -> Arc<diacraft_calc::CellEval> {
        let mut e = (*ev.evaluate_cell(at)).clone();
        if e.kind == CellKind::Formula {
            if let Some(saved) = e.saved.clone() {
                e.raw = saved.clone();
                e.view = saved;
            }
            e.error = e.saved_error;
            e.unsupported = None;
        }
        Arc::new(e)
    }
}

/// The real-file salt: with a wrong value put in the file's cell, the path functions compute their own answer and disagree with it, so every
/// salted cell is caught; an echo hands the wrong value back and misses all of them. The six cells agree unsalted, so the walk is real.
#[test]
fn the_real_salt_passes_the_path_functions_and_fails_an_echo() {
    let doc = saved_document();
    let options = || EvalOptions { operands: OperandMode::Cached, ..EvalOptions::default() };
    let r = recalc_all_salted(&doc, &Budget::default(), options(), &mut diacraft_calc::Native, Some(SaltPolicy { seed: 7, one_in: 1 }));
    assert_eq!(r.agreed, 6, "every path cell agrees with what a file would save: {r:?}");
    assert!(r.differed.is_empty() && r.unsupported.is_empty(), "{r:?}");
    let s = &r.real_salt;
    assert!(s.salted >= 6 && s.passed(), "{s:?}");
    assert_eq!(s.salted, s.caught, "{s:?}");
    let echo = recalc_all_salted(&doc, &Budget::default(), options(), &mut Echo, Some(SaltPolicy { seed: 7, one_in: 1 }));
    assert_eq!((echo.real_salt.salted, echo.real_salt.missed), (s.salted, s.salted), "{:?}", echo.real_salt);
}

// ---- hostile sections and the cost of a walk, to a count ------------------------------------------------------------------------------

/// The steps one evaluation of `formula` takes on a shape with a path of `rows` rows (a zigzag of lines), and the value it gave.
fn cost(rows: usize, formula: &str, budget: Budget) -> (u64, bool, Value) {
    let mut r = vec![mv(0.0, 0.0)];
    for i in 0..rows {
        r.push(ln(i as f64 + 1.0, if i % 2 == 0 { 1.0 } else { 0.0 }));
    }
    let (doc, at) = probe(|s| section(s, 0, r), formula);
    let mut ctx = EvalCtx::new(EvalOptions::default(), budget);
    let v = Evaluator::new(&doc, &mut ctx).evaluate(&at);
    (ctx.stats.steps, ctx.stats.budget_exhausted, v)
}

/// A walk costs steps in proportion to the rows of the section it reads, a few for each row and no more: doubling the rows doubles the steps
/// (to within the formula's own), for every function, and a section of a thousand rows is well inside the default budget.
#[test]
fn a_walk_costs_steps_in_proportion_to_its_rows() {
    for formula in [
        "PATHLENGTH(Geometry1.Path)",
        "PNTX(POINTALONGPATH(Geometry1.Path,0.5))",
        "ANGLEALONGPATH(Geometry1.Path,0.5)",
        "NEARESTPOINTONPATH(Geometry1.Path,100.5,0.5)",
        "DISTTOPATH(Geometry1.Path,100.5,0.5)",
        "PATHSEGMENT(Geometry1.Path,0.5)",
        "SEGMENTCOUNT(Geometry1.Path)",
    ] {
        let (small, _, path_shape) = cost(500, formula, Budget::default());
        let (large, _, v2) = cost(1000, formula, Budget::default());
        assert!(!matches!(path_shape, Value::Error(_)) && !matches!(v2, Value::Error(_)), "{formula}: {path_shape:?} {v2:?}");
        let ratio = large as f64 / small as f64;
        assert!((1.5..2.3).contains(&ratio), "{formula}: {small} steps for 500 rows and {large} for 1000 (ratio {ratio})");
        assert!(large < 40 * 1000, "{formula}: {large} steps for 1000 rows is more than a few for each row");
    }
}

/// A section of more rows than the budget has steps for stops the run: the budget is spent before the work is done, the answer is the loop error,
/// and the steps taken stay within the budget (the lesson of the 38-hour projection).
#[test]
fn a_budget_smaller_than_the_section_ends_the_run_before_the_work() {
    for formula in [
        "PATHLENGTH(Geometry1.Path)",
        "PNTX(POINTALONGPATH(Geometry1.Path,0.5))",
        "NEARESTPOINTONPATH(Geometry1.Path,5,5)",
        "SEGMENTCOUNT(Geometry1.Path)",
    ] {
        let budget = Budget::with_steps(3_000);
        let (steps, exhausted, v) = cost(20_000, formula, budget);
        assert!(exhausted, "{formula}: the budget was not spent");
        assert!(steps <= 3_000, "{formula}: {steps} steps taken of 3000");
        assert_eq!(v, Value::Error(diacraft_calc::rules::LOOP_ERROR), "{formula}");
    }
}

/// A section past the geometry crate's limit of rows is the loop error, not a walk.
#[test]
fn a_section_of_too_many_rows_is_refused() {
    let (steps, _, v) = cost(diacraft_geom::MAX_ROWS + 1, "PATHLENGTH(Geometry1.Path)", Budget::default());
    assert_eq!(v, Value::Error(diacraft_calc::rules::LOOP_ERROR));
    assert!(steps < 1_000_000, "refused before the rows were read: {steps}");
}

/// Splines are bounded by the budget: a section of many splines of many points costs steps for the multiplications it spends and is cut at the
/// budget's, however many splines it holds.
#[test]
fn splines_are_charged_for_the_multiplications_they_spend() {
    // 300 rows of NURBSTo, each with 200 control points.
    let tuples: Vec<String> = (0..200).map(|i| format!("{},{},{},1", f64::from(i), f64::from(i % 7), f64::from(i) / 200.0)).collect();
    let constructor = format!("NURBS(1,3,1,1,{})", tuples.join(","));
    let build = |s: ShapeBuilder, rows: usize| {
        let mut s = s.row_type("Geometry", Some(0), RowKey::Ix(0), "MoveTo").row_cell("Geometry", Some(0), RowKey::Ix(0), "X", "0").row_cell(
            "Geometry",
            Some(0),
            RowKey::Ix(0),
            "Y",
            "0",
        );
        for i in 1..=rows {
            let key = RowKey::Ix(u32::try_from(i).unwrap());
            s = s
                .row_type("Geometry", Some(0), key.clone(), "NURBSTo")
                .row_cell("Geometry", Some(0), key.clone(), "X", "200")
                .row_cell("Geometry", Some(0), key.clone(), "Y", "0")
                .row_cell("Geometry", Some(0), key.clone(), "A", "1")
                .row_cell("Geometry", Some(0), key.clone(), "B", "1")
                .row_cell("Geometry", Some(0), key.clone(), "C", "0")
                .row_cell("Geometry", Some(0), key.clone(), "D", "1")
                .row_formula("Geometry", Some(0), key, "E", &constructor, "0");
        }
        s
    };
    let run = |rows: usize, budget: Budget| {
        let (doc, at) = probe(|s| build(s, rows), "PATHLENGTH(Geometry1.Path)");
        let mut ctx = EvalCtx::new(EvalOptions::default(), budget);
        let v = Evaluator::new(&doc, &mut ctx).evaluate(&at);
        (ctx.stats.steps, ctx.stats.budget_exhausted, v)
    };
    let (one, _, _) = run(2, Budget::default());
    let (four, _, _) = run(8, Budget::default());
    assert!(four > 2 * one, "more splines cost more steps: {one} for 2 and {four} for 8");
    // With a small budget the run stops at it.
    let (steps, exhausted, v) = run(300, Budget::with_steps(50_000));
    assert!(exhausted && steps <= 50_000, "{steps} steps, exhausted {exhausted}");
    assert_eq!(v, Value::Error(diacraft_calc::rules::LOOP_ERROR));
}

/// A chain of groups deeper than a box looks into is the loop error, and a wide group of members is walked once for each member: the box of a
/// hostile tree costs steps and ends at the budget's.
#[test]
fn a_deep_tree_of_groups_ends_the_box_and_a_wide_one_is_charged() {
    let mut deepest = ShapeBuilder::new(100).cell("Width", "1").cell("Height", "1");
    for id in (1..60).rev() {
        deepest = ShapeBuilder::new(id).group().cell("Width", "1").cell("Height", "1").child(deepest);
    }
    let deepest = deepest.row_formula("User", None, RowKey::name("Probe"), "Value", "BOUNDINGBOXRECT(1)", "0");
    let doc = DocBuilder::new().page(1, PageBuilder::new().shape(deepest)).build();
    let at = CellAddress::new(SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0)), probe_addr());
    let v = Evaluator::new(&doc, &mut EvalCtx::default()).evaluate(&at);
    assert_eq!(v, Value::Error(diacraft_calc::rules::LOOP_ERROR), "{v:?}");
    // A wide group: every member counts a step, and a small budget ends the walk before the members are visited.
    let mut wide = ShapeBuilder::new(1).group().cell("Width", "1").cell("Height", "1");
    for id in 2..2000 {
        wide = wide.child(ShapeBuilder::new(id).cell("Width", "1").cell("Height", "1"));
    }
    let wide = wide.row_formula("User", None, RowKey::name("Probe"), "Value", "BOUNDINGBOXRECT(1)", "0");
    let doc = DocBuilder::new().page(1, PageBuilder::new().shape(wide)).build();
    let mut ctx = EvalCtx::new(EvalOptions::default(), Budget::with_steps(500));
    let v = Evaluator::new(&doc, &mut ctx).evaluate(&at);
    assert_eq!(v, Value::Error(diacraft_calc::rules::LOOP_ERROR), "{v:?}");
    assert!(ctx.stats.steps <= 500 && ctx.stats.budget_exhausted, "{:?}", ctx.stats);
}

/// Hostile rows: a number that is not finite, an infinite coordinate, a row of a type no spec lists, and a coordinate past what a length can add
/// up to, give an answer or an error and never a panic.
#[test]
fn hostile_rows_give_errors_not_panics() {
    let odd = |s: ShapeBuilder| section(s, 0, vec![mv(0.0, 0.0), R("Nonsense", vec![("X", 1.0)]), ln(1e308, 0.0), ln(-1e308, 0.0), ln(3.0, 0.0)]);
    for f in [
        "PATHLENGTH(Geometry1.Path)",
        "PNTX(POINTALONGPATH(Geometry1.Path,0.5))",
        "NEARESTPOINTONPATH(Geometry1.Path,1,1)",
        "ANGLEALONGPATH(Geometry1.Path,0.5)",
        "DISTTOPATH(Geometry1.Path,1,1)",
    ] {
        let v = eval_on(odd, f);
        assert!(matches!(v, Value::Error(_)), "{f}: an overflowing length is an error: {v:?}");
    }
    let unknown_only = |s: ShapeBuilder| section(s, 0, vec![R("Nonsense", vec![("X", 1.0)])]);
    check_table_on(
        unknown_only,
        &[("hostile", "PATHLENGTH(Geometry1.Path)", E::U(0.0, DrawingDefault)), ("hostile", "SEGMENTCOUNT(Geometry1.Path)", E::N(0.0))],
    );
    // A path argument that nests `IF` without end is bounded.
    let deep = format!("PATHLENGTH({}Geometry1.Path{})", "IF(1,".repeat(40), ",0)".repeat(40));
    let v = eval_on(path_shape, &deep);
    assert!(matches!(v, Value::Error(_)), "{v:?}");
}

// ----: the draws a function holds, a themed Rounding that reads its own path, the nearest place at a curve's start --------------

/// A shape with a move and one `PolylineTo` row whose `A` is a `POLYLINE` of `points` vertices: a path of that many steps from one row.
fn long_polyline(s: ShapeBuilder, points: usize) -> ShapeBuilder {
    let body: Vec<String> = (0..points).map(|i| format!("{},{}", i, i % 7)).collect();
    section(s, 0, vec![mv(0.0, 0.0)])
        .row_type("Geometry", Some(0), RowKey::Ix(1), "PolylineTo")
        .row_cell("Geometry", Some(0), RowKey::Ix(1), "X", "1")
        .row_cell("Geometry", Some(0), RowKey::Ix(1), "Y", "1")
        .row_formula("Geometry", Some(0), RowKey::Ix(1), "A", &format!("POLYLINE(1,1,{})", body.join(",")), "0")
}

/// What a call draws is memory held while the function works, at the size of a step and of a row of steps: a section drawn from one
/// row of 20,000 points holds 10,000 steps more than one of 10,000, and the most memory held at once says so (56 bytes a step, beside the 16
/// the constructor's value keeps in the memo).
#[test]
fn the_steps_a_function_draws_are_held_while_it_works() {
    let peak = |points: usize| {
        let (doc, at) = probe(|s| long_polyline(s, points), "PATHLENGTH(Geometry1.Path)");
        let mut ctx = EvalCtx::new(EvalOptions::default(), Budget::default());
        let v = Evaluator::new(&doc, &mut ctx).evaluate(&at);
        assert!(matches!(v, Value::Number(_)), "{points} points: {v:?}");
        ctx.stats.bytes_peak
    };
    let (few, many) = (peak(10_000), peak(20_000));
    let step = std::mem::size_of::<diacraft_geom::DrawStep>() as u64;
    assert!(many - few >= 10_000 * step, "{few} bytes at the most for 10,000 points and {many} for 20,000: a step is {step} bytes");
}

/// `Rounding` is read before the section is drawn, so a `Rounding` that calls another shape's path function holds that shape's draws
/// and the section's never at once: shape 1's `Rounding` is the length of shape 2's path (the same 10,000 points), and the most memory held is
/// that of one path and the memo, not of two paths.
#[test]
fn a_rounding_that_walks_another_shape_does_not_find_the_sections_draws_held() {
    let points = 10_000;
    let alone = {
        let (doc, at) = probe(|s| long_polyline(s, points), "PATHLENGTH(Geometry1.Path)");
        let mut ctx = EvalCtx::new(EvalOptions::default(), Budget::default());
        let _ = Evaluator::new(&doc, &mut ctx).evaluate(&at);
        ctx.stats.bytes_peak
    };
    let one = long_polyline(ShapeBuilder::new(1), points).formula("Rounding", "PATHLENGTH(Sheet.2!Geometry1.Path)*0", "0").row_formula(
        "User",
        None,
        RowKey::name("Probe"),
        "Value",
        "PATHLENGTH(Geometry1.Path)",
        "0",
    );
    let doc = DocBuilder::new().page(1, PageBuilder::new().shape(one).shape(long_polyline(ShapeBuilder::new(2), points))).build();
    let at = CellAddress::new(SheetRef::Shape(ShapeRef::top(Owner::Page(1), 0)), probe_addr());
    let mut ctx = EvalCtx::new(EvalOptions::default(), Budget::default());
    let v = Evaluator::new(&doc, &mut ctx).evaluate(&at);
    assert!(matches!(v, Value::Number(_)), "{v:?}");
    // The second path's constructor stays in the memo (16 bytes a point); its draws (56 a point) are gone before the first path is drawn. Two
    // sets of draws held at once would put the peak 56 bytes a point over the single path's.
    let extra = ctx.stats.bytes_peak.saturating_sub(alone);
    assert!(extra < 40 * points as u64, "{alone} bytes held at the most for one path and {} for two: {extra} over", ctx.stats.bytes_peak);
}

/// A `Rounding` saved as `Themed` whose own formula walks its own section is a cycle, in both operand modes: the cell is in progress
/// while its own formula is worked out for the theme, so the second read of it is a cycle and not another level of the same walk, a hundred
/// levels deep, until the depth limit. The function is not calculated (`Unsupported`, the saved marker standing) and the run is a few steps.
#[test]
fn a_themed_rounding_that_walks_its_own_section_is_a_cycle_and_not_a_deep_recursion() {
    for operands in [OperandMode::Live, OperandMode::Cached] {
        let (doc, at) = probe(|s| plain(s).formula("Rounding", "PATHLENGTH(Geometry1.Path)", "Themed"), "PATHLENGTH(Geometry1.Path)");
        let mut ctx = EvalCtx::new(EvalOptions { operands, ..EvalOptions::default() }, Budget::default());
        let v = Evaluator::new(&doc, &mut ctx).evaluate(&at);
        assert!(matches!(v, Value::Unsupported(_) | Value::Error(_)), "{operands:?}: {v:?}");
        assert!(ctx.stats.cycles >= 1, "{operands:?}: the cell met itself: {:?}", ctx.stats);
        assert_eq!(ctx.stats.depth_limit_hits, 0, "{operands:?}: a cycle and not the depth limit: {:?}", ctx.stats);
        assert!(ctx.stats.steps < 500, "{operands:?}: {:?}", ctx.stats);
    }
}

/// The nearest place to a point behind the start of a curve is the start: 0 of the way, whatever the first row is. A path of a move
/// then a `RelQuadBezTo` and a point behind its start used to give the quad's end (kurbo's arc length of a degenerate quadratic is not a
/// number, and `f64::min` took the piece's whole length), with `DISTTOPATH` signed there.
#[test]
fn the_nearest_place_behind_the_start_of_a_curve_is_the_start() {
    let curve = |s: ShapeBuilder| {
        section(s, 0, vec![mv(0.0, 0.0), R("RelQuadBezTo", vec![("X", 4.0), ("Y", 0.0), ("A", 2.0), ("B", 2.0)])])
            .cell("Width", "1")
            .cell("Height", "1")
    };
    assert!(number(curve, "NEARESTPOINTONPATH(Geometry1.Path,-1,0)").abs() < 1e-9);
    assert!(near(number(curve, "DISTTOPATH(Geometry1.Path,-1,0)").abs(), 1.0, 1e-9));
    // The same at the end: a point past the end is 1 of the way.
    assert!(near(number(curve, "NEARESTPOINTONPATH(Geometry1.Path,5,0)"), 1.0, 1e-9));
}

/// A row that took another type, a row that has no cells and appeared, and a row of no cells that went change the path, and no cell of
/// them is written: `changes_between` says so (`Changes::rows`) and the commit works the cell that reads the whole section out again, where it
/// had left the length the file saved. A row that changed nothing is no change.
#[test]
fn a_row_retyped_or_added_or_taken_away_with_no_cells_is_a_change_a_path_reader_hears_of() {
    use diacraft_calc::{changes_between, commit_changed};

    // A move, a row of this type to (3, 4), and a row of no cells when `extra`.
    let rows = |kind: &'static str, extra: bool| {
        let mut r = vec![mv(0.0, 0.0), R(kind, vec![("X", 3.0), ("Y", 4.0)])];
        if extra {
            r.push(R("LineTo", vec![]));
        }
        r
    };
    // The same shape with a path of these rows; `User.Len` holds a length the file saved, which is the path's own for none of them.
    let page = |kind: &'static str, extra: bool| {
        let s = section(ShapeBuilder::new(1), 0, rows(kind, extra)).set(user("Len"), cell("PATHLENGTH(Geometry1.Path)", "99", Some("DL")));
        DocBuilder::new().page(1, PageBuilder::new().shape(s)).build()
    };
    let len_of = |kind: &'static str, extra: bool| number(|s| section(s, 0, rows(kind, extra)), "PATHLENGTH(Geometry1.Path)");
    let saved_len = |doc: &Document| -> f64 {
        let at = doc.find_shape(Owner::Page(1), 1).unwrap();
        match doc.shape(&at).unwrap().sheet.lookup(&user("Len")) {
            diacraft_doc::Lookup::Found(c) => c.value.as_ref().unwrap().text().parse().unwrap(),
            other => panic!("{other:?}"),
        }
    };
    let cases = [
        (("LineTo", false), ("RelLineTo", false), "a row that is a RelLineTo where it was a LineTo"),
        (("LineTo", false), ("LineTo", true), "a row with no cells that appeared"),
        (("LineTo", true), ("LineTo", false), "a row with no cells that went"),
    ];
    for ((kind_before, extra_before), (kind_after, extra_after), what) in cases {
        let (before, after) = (page(kind_before, extra_before), page(kind_after, extra_after));
        let changes = changes_between(&before, &after, 1);
        assert!(!changes.rows.is_empty() && !changes.is_empty(), "{what}: the row is a change: {changes:?}");
        let mut doc = after.clone();
        commit_changed(&mut doc, &Budget::default(), EvalOptions::default(), 1, &changes);
        let want = len_of(kind_after, extra_after);
        assert!((want - len_of(kind_before, extra_before)).abs() > 1e-6, "{what}: the case changes the length");
        assert!((saved_len(&doc) - want).abs() < 1e-9, "{what}: the length was left at {} and is {want}", saved_len(&doc));
    }
    // The same rows again are no change.
    assert!(changes_between(&page("LineTo", true), &page("LineTo", true), 1).is_empty());
}
