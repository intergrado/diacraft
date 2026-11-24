//! Comparing a result with the file: the one comparer of the oracle. Nothing else in the project decides that two values
//! agree.
//!
//! **`E` comes first.** A cell is compared by its error and then by its value: the error words must be the same (or both absent), and the
//! values are compared only when the file saved a typed `V` and the result is not itself an error, since the saved `V` of a failing cell
//! is the last good value. A cell that saved an `E` and no usable `V` (empty, absent, `Themed`, a date past
//! the year 9999) is still compared by its `E`.
//!
//! **What agrees.** Numbers within a relative [`ORACLE_TOLERANCE`] with a floor of 1 on the size ("Numbers
//! agree when they differ by at most 1E-9 times the larger of 1 and their sizes"), or the geometry batch's own; infinity only with itself;
//! a date within a second; **text exactly** (no trimming, no case folding: the census says exact); a colour by its channels, one step apart
//! where `MSOTINT` made it, or through the colour table for a number, which is an index only when it is a whole number within the tolerance
//! (1.75 is not index 1); a Boolean as the number it is. `ATAN2` gets the two allowances of and no cell else
//! does.
//!
//! **By the curve (`_ELLIPSE_THETA` and `_ELLIPSE_ECC`).** The `C` and `D` of an `EllipticalArcTo` row are
//! one fact written in two cells: the direction of an axis of the arc's ellipse and the ratio of that half-axis to the other. The rule that fills them
//! (R3 of each function) agrees with 97% of saved `C` and 99.5% of saved `D` by raw value, and every miss names the same ellipse in other words: the
//! other end of the axis (an angle and the same angle plus pi), the other axis with the reciprocal ratio, any angle for a circle, whichever angle
//! rounding chose for an ellipse that is nearly one. So a raw match agrees, and so does a pair that names the same ellipse as the file's within the
//! comparison's tolerance (`same_curve_as_saved`): the file's pair is the cell's saved value with its partner's in the row, the computed pair is the
//! calculation's value with the other number of the pair that the rule gives along it. A pair that names another ellipse differs, a ratio held as its
//! reciprocal included, and the rule is for these two functions in the `C` and `D` of `EllipticalArcTo` and `RelEllipticalArcTo`
//! rows (the `D` reading the row's own `C`) and no other cell. A cell that agrees only through it is counted (`RecalcReport::agreed_by_curve`, and with
//! the allowances in `agreed_by_allowance`), and the real-file salt does not call it a miss when the arc is a circle or the angles are whole half turns
//! apart (`by_design`) and does when the agreement is any wider than that. A commit keeps a saved pair that names the computed pair's ellipse the
//! same way (`names_the_saved_ellipse`).
//!
//! **What the allowances cost.** The relative size above 1, the geometry tolerance, the colour step and the `ATAN2` allowances each let a
//! cell agree that an absolute 1E-9 would not. [`agrees_strictly`] asks the stricter question, and the report counts the cells that agree
//! only through an allowance (`RecalcReport::agreed_by_allowance`) so that a score can be read both ways (the review's M8: scores err low).

use std::sync::Arc;

use diacraft_doc::{CellAddr, CellName, Document, Formula, FormulaSlot};
use diacraft_formula::Expr;
use diacraft_units::Unit;

use crate::cells::CellEval;
use crate::eval::{CellAddress, Env, Evaluator};
use crate::fn_ellipse::ecc;
use crate::recalc::DifferenceKind;
use crate::rules::{
    ATAN2_NO_LENGTH, ATAN2_PI_TOLERANCE, ATAN2_TURN_TOLERANCE, GEOMETRY_COMPARE_TOLERANCE, NON_FINITE_AGREES_ONLY_IF_EQUAL, ORACLE_TOLERANCE,
};
use crate::value::{Rgb, Value};

/// How two values are compared: the tolerance of numbers, whether it is relative to their size, and how far a colour channel may be.
#[derive(Clone, Copy)]
pub(crate) struct Comparison {
    pub tolerance: f64,
    pub colour_step: i32,
    /// Whether the tolerance is relative to the larger of 1 and the sizes (the oracle's rule), or absolute (the strict question).
    pub relative: bool,
}

impl Comparison {
    /// The oracle's own: [`ORACLE_TOLERANCE`], relative, and no colour step.
    pub fn standard() -> Comparison {
        Comparison { tolerance: ORACLE_TOLERANCE, colour_step: 0, relative: true }
    }
}

/// The comparer's whole answer: how the cell differs, if it does, whether it agrees only through `ATAN2`'s zero-length rule (the one allowance that
/// lets any saved value agree), and whether it agrees only through the curve its row describes (`_ELLIPSE_THETA` and
/// `_ELLIPSE_ECC`, below), and if so whether for a reason a wrong value would not have by chance (`curve_by_design`). The real-file salt asks these
/// to tell those rules from a miss.
pub(crate) struct Judged {
    pub difference: Option<DifferenceKind>,
    pub by_atan2: bool,
    pub by_curve: bool,
    /// With `by_curve`: the arc is a circle, or the angles are whole half turns apart.
    pub curve_by_design: bool,
}

/// How the cell differs from the file, if it does, and whether a value that did not agree on its own was passed by `ATAN2`'s rule or by the
/// curve. The error is compared first and always; the value only when the file saved a typed one and the formula worked.
pub(crate) fn judge(ev: &mut Evaluator<'_, '_>, at: &CellAddress, outcome: &CellEval, how: Comparison) -> Judged {
    let error_differs = match (outcome.error, outcome.saved_error) {
        (None, None) => false,
        (Some(a), Some(b)) => a != b,
        _ => true,
    };
    let mut by_atan2 = false;
    let mut by_curve = false;
    let mut curve_by_design = false;
    // An error leaves the last good value, which is what was saved: the values can differ only when the formula worked. A saved `Themed`
    // or a date past the year 9999 is no value to compare, whatever `E` says.
    let value_differs = match (&outcome.raw, &outcome.saved) {
        (Value::Error(_) | Value::Unsupported(_), _) => false,
        (_, None | Some(Value::Unsupported(_))) => false,
        (computed, Some(saved)) => {
            if values_agree(Some(ev.doc), computed, saved, how) {
                false
            } else if atan2_allowance(ev, at, computed, saved) {
                by_atan2 = true;
                false
            } else if let Some(designed) = same_curve_as_saved(ev, at, computed, saved, how) {
                by_curve = true;
                curve_by_design = designed;
                false
            } else {
                true
            }
        }
    };
    let difference = match (value_differs, error_differs) {
        (false, false) => None,
        (true, false) => Some(DifferenceKind::Value),
        (false, true) => Some(DifferenceKind::Error),
        (true, true) => Some(DifferenceKind::Both),
    };
    Judged { difference, by_atan2, by_curve, curve_by_design }
}

/// Whether a cell that agrees would still agree with no allowance but an absolute 1E-9 on numbers and no step on a colour. Only a number-like
/// value can differ between the two questions, so any other is true without a second comparison.
pub(crate) fn agrees_strictly(doc: &Document, outcome: &CellEval) -> bool {
    let (computed, Some(saved)) = (&outcome.raw, &outcome.saved) else { return true };
    if matches!(computed, Value::Text(_) | Value::Bool(_) | Value::Error(_) | Value::Unsupported(_)) || matches!(saved, Value::Unsupported(_)) {
        return true;
    }
    values_agree(Some(doc), computed, saved, Comparison { tolerance: ORACLE_TOLERANCE, colour_step: 0, relative: false })
}

/// Whether two numbers agree within `tolerance` of the larger of 1 and their sizes (or absolutely, when the comparison is not relative). A
/// non-finite number agrees only with an equal one, where the rule is on (`rules::NON_FINITE_AGREES_ONLY_IF_EQUAL`): the test
/// `|a - b| <= tol * max(|a|, |b|)` is true for infinity against anything.
fn close(a: f64, b: f64, how: Comparison) -> bool {
    if a == b {
        return true;
    }
    if NON_FINITE_AGREES_ONLY_IF_EQUAL && !(a.is_finite() && b.is_finite()) {
        return false;
    }
    let size = if how.relative { a.abs().max(b.abs()).max(1.0) } else { 1.0 };
    (a - b).abs() <= how.tolerance * size
}

/// Whether two values are the same to the oracle's tolerance, with the colour table: what a commit asks to keep a saved value as it is.
pub(crate) fn agrees(doc: &Document, saved: &Value, computed: &Value) -> bool {
    values_agree(Some(doc), computed, saved, Comparison::standard())
}

/// The palette index a saved number is, or `None` when it is not one. A number stands for a colour only as an exact index (within the
/// comparison's tolerance of a whole number), never by being cut: a computed 1.75 is not index 1 and is not index 2 (the cut,
/// `as i64`, made 0.75 agree with index 0 and 1.75 with index 1). A number a palette cannot have as an index (non-finite, or past 1E9) is none.
fn palette_index(x: f64, how: Comparison) -> Option<i64> {
    if !x.is_finite() || x.abs() >= 1.0e9 {
        return None;
    }
    let nearest = x.round();
    // In range by the check above, so the cast cannot saturate.
    close(nearest, x, how).then_some(nearest as i64)
}

fn channels_close(a: &Rgb, b: &Rgb, step: i32) -> bool {
    let near = |x: u8, y: u8| (i32::from(x) - i32::from(y)).abs() <= step;
    near(a.r, b.r) && near(a.g, b.g) && near(a.b, b.b)
}

/// Whether a computed value is the saved one, within the tolerance.
pub(crate) fn values_agree(doc: Option<&Document>, computed: &Value, saved: &Value, how: Comparison) -> bool {
    match (computed, saved) {
        (Value::Number(a), Value::Number(b)) => {
            if a.unit == Unit::Date || b.unit == Unit::Date {
                // A date holds whole seconds in a file.
                (a.stored - b.stored).abs() <= 1.0 / 86_400.0 + 1e-9
            } else {
                close(a.stored, b.stored, how)
            }
        }
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Bool(a), Value::Number(n)) | (Value::Number(n), Value::Bool(a)) => close(f64::from(u8::from(*a)), n.stored, how),
        (Value::Text(a), Value::Text(b)) => a == b,
        (Value::Text(t), Value::Number(n)) | (Value::Number(n), Value::Text(t)) => {
            crate::strnum::parse_decimal(t).is_some_and(|x| close(x, n.stored, how))
        }
        (Value::Color(a), Value::Color(b)) => channels_close(a, b, how.colour_step),
        (Value::Color(c), Value::Number(n)) | (Value::Number(n), Value::Color(c)) => palette_index(n.stored, how)
            .and_then(|index| doc.and_then(|d| d.color(index)))
            .is_some_and(|rgba| channels_close(c, &Rgb { r: rgba.r, g: rgba.g, b: rgba.b }, how.colour_step)),
        (Value::Currency { amount: a, currency: ca }, Value::Currency { amount: b, currency: cb }) => close(*a, *b, how) && ca == cb,
        (Value::Currency { amount, .. }, Value::Number(n)) | (Value::Number(n), Value::Currency { amount, .. }) => close(*amount, n.stored, how),
        (Value::Point(a), Value::Point(b)) => close(a.x.stored, b.x.stored, how) && close(a.y.stored, b.y.stored, how),
        (Value::Constructor(a), Value::Constructor(b)) => {
            a.kind == b.kind && a.numbers.len() == b.numbers.len() && a.numbers.iter().zip(&b.numbers).all(|(x, y)| close(*x, *y, how))
        }
        _ => false,
    }
}

/// The functions that give a direction from minus pi to pi, whose result at the half turn is either of the two ends: the file may hold one and the
/// calculation the other, and they are one direction ("minus pi and plus pi
/// count as one direction", 2 `ANGLEALONGPATH` cells and 3 `ANGLETOPAR` cells).
const DIRECTION_FUNCTIONS: [&str; 3] = ["ANGLEALONGPATH", "ANGLETOLOC", "ANGLETOPAR"];

/// Whether two angles are the same half turn, one at minus pi and the other at plus pi: a whole turn apart, and both within 1E-10 of pi in size.
fn half_turn_apart(a: f64, b: f64) -> bool {
    let pi = std::f64::consts::PI;
    ((a - b).abs() - 2.0 * pi).abs() <= ATAN2_TURN_TOLERANCE
        && (a.abs() - pi).abs() <= ATAN2_PI_TOLERANCE
        && (b.abs() - pi).abs() <= ATAN2_PI_TOLERANCE
}

/// The allowances for a direction that is the whole formula, and no other cell gets them. An `ATAN2`: a result a
/// whole turn from the saved one when both are within 1E-10 of pi in size, and any saved value when both operands read under 1E-6. An
/// `ANGLEALONGPATH`, `ANGLETOLOC` or `ANGLETOPAR`: the first only, minus pi and plus pi being one direction.
fn atan2_allowance(ev: &mut Evaluator<'_, '_>, at: &CellAddress, computed: &Value, saved: &Value) -> bool {
    let (Value::Number(a), Value::Number(b)) = (computed, saved) else { return false };
    let Some(res) = ev.res.resolve(&at.sheet, &at.addr) else { return false };
    let Some(FormulaSlot::Local(formula)) = res.formula.cell.map(|c| &c.formula) else { return false };
    let Ok(Expr::Call(call)) = &formula.ast else { return false };
    if call.qualifiers.is_empty() && DIRECTION_FUNCTIONS.iter().any(|name| call.is(name)) {
        return half_turn_apart(a.stored, b.stored);
    }
    if !call.is("ATAN2") || !call.qualifiers.is_empty() || call.args.len() != 2 {
        return false;
    }
    if half_turn_apart(a.stored, b.stored) {
        return true;
    }
    let env = Env::new(res.on.clone(), res.rebind.clone());
    let mut size = |e: &Option<Expr>| -> Option<f64> {
        let v = ev.eval(e.as_ref()?, &env);
        let q = v.quantity()?;
        Some(q.stored.abs())
    };
    match (size(&call.args[0]), size(&call.args[1])) {
        (Some(y), Some(x)) => y < ATAN2_NO_LENGTH && x < ATAN2_NO_LENGTH,
        _ => false,
    }
}

/// What the ellipse a pair names is, up to its size: `ln(ratio)` along the direction `2·angle`, as a point in the plane. The `C` and `D` of an
/// `EllipticalArcTo` row name the arc's ellipse by the direction of one axis and the ratio of that half-axis to the other;
/// the size is fixed by the arc's three points and is no part of the pair. Two pairs name one ellipse exactly when their points are one:
///
/// * an angle and the same angle turned half a turn, 2·angle differing by a whole turn (the two ends of the x-axis);
/// * the other axis with the reciprocal ratio: a quarter turn on adds a half turn to 2·angle, which turns the point round, and the reciprocal turns
///   `ln(ratio)` over, so the two cancel (a stretched circle held at its longer axis);
/// * a ratio of 1 at any angle, a circle with no direction, whose point is the origin (the cells of a kept proportion);
/// * a resized ellipse that is nearly a circle at whichever angle rounding chose, whose point lies within a millionth of the origin.
///
/// `None` for a pair that names no ellipse: an angle or ratio that is not finite, or a ratio of 0 or less.
fn curve_point(angle: f64, ratio: f64) -> Option<(f64, f64)> {
    if !(angle.is_finite() && ratio.is_finite() && ratio > 0.0) {
        return None;
    }
    let length = ratio.ln();
    let (sin, cos) = (2.0 * angle).sin_cos();
    Some((length * cos, length * sin))
}

/// Whether two pairs name one ellipse: their points are within the comparison's tolerance, relative to the larger of 1 and their distances from the
/// origin as every tolerance of the comparer is.
fn same_ellipse(a: (f64, f64), b: (f64, f64), how: Comparison) -> bool {
    let (pa, pb) = (curve_point(a.0, a.1), curve_point(b.0, b.1));
    let (Some(p), Some(q)) = (pa, pb) else { return false };
    let size = if how.relative { p.0.hypot(p.1).max(q.0.hypot(q.1)).max(1.0) } else { 1.0 };
    (p.0 - q.0).hypot(p.1 - q.1) <= how.tolerance * size
}

/// The comparison of `_ELLIPSE_THETA` and `_ELLIPSE_ECC` cells by the curve. A raw match has already
/// agreed when this is asked; a cell that is the `C` or the `D` of a geometry row and the whole call of one of the two agrees as well when the pair it
/// is part of names the ellipse that the pair the file saved names:
///
/// * the **saved pair** is the cell's own saved value with the saved value of its partner in the row (`D` for the angle, `C` for the ratio);
/// * the **computed pair** of an angle is the angle the calculation gave with the ratio the rule gives along it, from the same six arguments
///   ("the miss changes the stored angle and not the curve"); of a ratio it is the angle the call read as its seventh argument
///   with the ratio the calculation gave.
///
/// So a pair that differs from the file's only in the end of its axis, in which axis it names, or in the angle of a circle, agrees, and one that names
/// another ellipse differs, as does a ratio held as its reciprocal (that changes the curve). The rule is for these two functions
/// in these two cells and for nothing else; the tolerance is the cell's own. It never makes a cell agree that a raw match would not, and the
/// real-file salt counts a cell that agrees through it with the allowances and not as a miss (`salt.rs`), since a circle's angle is no part of its
/// curve.
///
/// **Only the pair of an elliptical arc.** The cell must be the `C` of a `(Rel)EllipticalArcTo` row whose whole formula is `_ELLIPSE_THETA` of six
/// arguments, or the `D` of such a row whose whole formula is `_ELLIPSE_ECC` of seven and whose seventh argument is that row's own `C` ([`arc_cell`]):
/// in a row of another type, or with a seventh argument that names some other cell, `C` and `D` are not the pair the rule is about.
///
/// → `None` when the pairs name different ellipses (or the cell is no arc cell); `Some(by_design)` when they name one, where `by_design` says the
/// agreement is one a wrong value would not reach by chance: the arc is a circle, or the two angles are a whole number of half turns apart ([`by_design`]).
fn same_curve_as_saved(ev: &mut Evaluator<'_, '_>, at: &CellAddress, computed: &Value, saved: &Value, how: Comparison) -> Option<bool> {
    let (Value::Number(computed), Value::Number(saved)) = (computed, saved) else { return None };
    let cell = arc_cell(ev, at)?;
    let Ok(Expr::Call(call)) = &cell.formula.ast else { return None };
    // The call's arguments, as the calculation read them.
    let mut numbers: Vec<f64> = Vec::with_capacity(call.args.len());
    for arg in &call.args {
        let expr = arg.as_ref()?;
        numbers.push(ev.eval(expr, &cell.env).quantity()?.stored);
    }
    let resize = <[f64; 6]>::try_from(numbers.get(..6)?).ok()?;
    // The partner's saved value: the file's other half of the pair.
    let partner_addr = CellAddr { section: at.addr.section.clone(), row: at.addr.row.clone(), cell: CellName::new(cell.partner) };
    let sibling = ev.compute_cell(&at.sheet, &partner_addr, false);
    let Some(Value::Number(other)) = &sibling.saved else { return None };
    let (computed_pair, saved_pair) = if cell.theta {
        let ratio = ecc(&resize, computed.stored).ok()?;
        ((computed.stored, ratio), (saved.stored, other.stored))
    } else {
        let along = *numbers.get(6)?;
        ((along, computed.stored), (other.stored, saved.stored))
    };
    same_ellipse(computed_pair, saved_pair, how).then(|| by_design(computed_pair, saved_pair))
}

/// Whether two pairs that name one ellipse do so for a reason a wrong value could not have by chance: both are circles (each point is within
/// [`GEOMETRY_COMPARE_TOLERANCE`] of the origin, so the angles mean nothing and a ratio of 1 is the same ratio), or the angles are a whole number of half
/// turns apart (the two ends of one axis) **and the ratios are the same**. The real-file salt excuses a salted cell that agrees through the curve only
/// for these; the test is **fixed** and does not follow the comparison's tolerance, so that a rule made too loose shows as a salt miss: a wrong ratio at
/// the right angle, or a wrong angle at the right ratio, is not by design.
fn by_design(computed: (f64, f64), saved: (f64, f64)) -> bool {
    let tolerance = GEOMETRY_COMPARE_TOLERANCE;
    let at_origin = |pair: (f64, f64)| curve_point(pair.0, pair.1).is_some_and(|p| p.0.hypot(p.1) <= tolerance);
    if at_origin(computed) && at_origin(saved) {
        return true;
    }
    let off = (computed.0 - saved.0).rem_euclid(std::f64::consts::PI);
    let half_turns = off <= tolerance || std::f64::consts::PI - off <= tolerance;
    half_turns && (computed.1 - saved.1).abs() <= tolerance * computed.1.abs().max(saved.1.abs()).max(1.0)
}

/// A cell that is the `C` or the `D` of an elliptical arc's row and the whole call of its function.
struct ArcCell {
    /// Whether it is the angle (`C`, `_ELLIPSE_THETA`) and not the ratio (`D`, `_ELLIPSE_ECC`).
    theta: bool,
    /// The other cell of the pair.
    partner: &'static str,
    formula: Arc<Formula>,
    env: Env,
}

/// The arc pair `at` is part of, if it is one: the names of the cell, the section and the call first (nothing is read for any other cell), then the
/// type of the row (`EllipticalArcTo` or `RelEllipticalArcTo`, found without merging the section), then, for a `D`, that the seventh argument is the
/// row's own `C`.
fn arc_cell(ev: &mut Evaluator<'_, '_>, at: &CellAddress) -> Option<ArcCell> {
    let (Some(section), Some(row)) = (&at.addr.section, &at.addr.row) else { return None };
    let name = at.addr.cell.as_str();
    if section.name.as_str() != "Geometry" || !(name == "C" || name == "D") {
        return None;
    }
    let res = ev.res.resolve(&at.sheet, &at.addr)?;
    let FormulaSlot::Local(formula) = &res.formula.cell?.formula else { return None };
    let formula = Arc::clone(formula);
    let env = Env::new(res.on.clone(), res.rebind.clone());
    let Ok(Expr::Call(call)) = &formula.ast else { return None };
    if !call.qualifiers.is_empty() {
        return None;
    }
    let (theta, partner) = if name == "C" && call.is("_ELLIPSE_THETA") && call.args.len() == 6 {
        (true, "D")
    } else if name == "D" && call.is("_ELLIPSE_ECC") && call.args.len() == 7 {
        (false, "C")
    } else {
        return None;
    };
    // The type of the row, by a look at each layer and not a merge of the section: a step for the look.
    if !ev.steps(1) {
        return None;
    }
    let kind = ev.res.row_type(&at.sheet, section, row)?;
    if !matches!(&*kind, "EllipticalArcTo" | "RelEllipticalArcTo") {
        return None;
    }
    if !theta {
        let Some(Some(Expr::Ref(reference))) = call.args.get(6) else { return None };
        let (sheet, addr) = ev.locate(reference, &env).ok()?;
        let own = CellAddr { section: at.addr.section.clone(), row: at.addr.row.clone(), cell: CellName::new("C") };
        if sheet != at.sheet || addr != own {
            return None;
        }
    }
    Some(ArcCell { theta, partner, formula, env })
}

/// Whether a cell is the `C` or the `D` of an elliptical arc's row and the whole call of its function ([`arc_cell`]): the cells a pass over the whole
/// page leaves as they are when the arcs' arguments did not change (`Changes::arcs_untouched`).
pub(crate) fn is_arc_pair_cell(ev: &mut Evaluator<'_, '_>, at: &CellAddress) -> bool {
    arc_cell(ev, at).is_some()
}

/// Whether the pair a worked-out `C` or `D` is part of names the ellipse that the pair the file saved names, to the tolerance of the geometry batch
/// ([`same_curve_as_saved`]): what a commit asks to keep the pair a file holds when the rule would write another that is the same curve. A cell that
/// held an error is not kept so.
pub(crate) fn names_the_saved_ellipse(ev: &mut Evaluator<'_, '_>, at: &CellAddress, outcome: &CellEval) -> bool {
    if outcome.saved_error.is_some() || outcome.error.is_some() {
        return false;
    }
    let Some(saved) = outcome.saved.as_ref() else { return false };
    let how = Comparison { tolerance: GEOMETRY_COMPARE_TOLERANCE, colour_step: 0, relative: true };
    same_curve_as_saved(ev, at, &outcome.raw, saved, how).is_some()
}
